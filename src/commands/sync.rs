//! 同步命令 — 本地目录与云盘目录流式并行同步。
//!
//! **流式模型（JoinSet + 固定最大并发数）**:
//!
//! 单个生产者协程 BFS 遍历目录树，逐目录对比本地 vs 云盘。
//! 发现差异时立即 spawn 任务到 JoinSet。JoinSet 满时 await 等待
//! 一个完成后再继续遍历。遍历和传输完全交叉，无需等扫描完毕。
//!
//! ```text
//! streaming_sync()
//! ┌─────────────────────────────────────────────┐
//! │  Producer: BFS(VecDeque)                     │
//! │    for each dir:                             │
//! │      read local + list cloud                 │
//! │      new dir    → ensure_dir / mkdir 串行     │
//! │      need xfer  → join_set.spawn(upload/dl)  │
//! │      need delete→ pending_deletes.push()     │
//! │      same size  → skip++                     │
//! │                                              │
//! │      while join_set.len() >= max_parallel:   │
//! │        join_set.join_next().await  ← 背压     │
//! │                                              │
//! │  drain: while join_set.join_next() {}        │
//! │  execute pending_deletes serially            │
//! └─────────────────────────────────────────────┘
//! ```

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

use crate::commands::list::ListItem;
use crate::config::DEFAULT_PARALLEL;
use crate::error::{Result, Yun139Error};
use crate::Yun139Client;

/// 同步方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncDirection {
    LocalToCloud,
    CloudToLocal,
}

/// 同步配置。
#[derive(Debug, Clone)]
pub struct SyncOptions {
    pub delete: bool,
    pub concurrency: usize,
}

impl Default for SyncOptions {
    fn default() -> Self {
        Self { delete: false, concurrency: DEFAULT_PARALLEL }
    }
}

impl SyncOptions {
    pub fn with_delete(mut self, v: bool) -> Self { self.delete = v; self }
    pub fn with_concurrency(mut self, n: usize) -> Self { self.concurrency = n.max(1); self }
}

/// 单条同步动作（仅用于删除的延迟执行）。
#[derive(Debug, Clone)]
pub enum SyncAction {
    Upload { local: PathBuf, cloud: String },
    Download { cloud: String, local: PathBuf },
    MkdirCloud { cloud: String },
    MkdirLocal { local: PathBuf },
    DeleteCloud { cloud: String },
    DeleteLocal { local: PathBuf },
    Skip { name: String },
}

/// 同步执行摘要。
#[derive(Debug, Default, Clone)]
pub struct SyncSummary {
    pub uploaded: u32,
    pub downloaded: u32,
    pub dirs_created: u32,
    pub deleted: u32,
    pub skipped: u32,
    pub failed: u32,
}

// ── 公开 API ──

impl Yun139Client {
    pub async fn sync_to_cloud(
        &self, local_dir: &Path, cloud_dir: &str, delete: bool,
        on_progress: impl Fn(&str) + Send + Sync,
    ) -> Result<SyncSummary> {
        let opts = SyncOptions::default().with_delete(delete);
        self.sync_to_cloud_with_options(local_dir, cloud_dir, &opts, on_progress).await
    }

    pub async fn sync_to_cloud_with_options(
        &self, local_dir: &Path, cloud_dir: &str, opts: &SyncOptions,
        _on_progress: impl Fn(&str) + Send + Sync,
    ) -> Result<SyncSummary> {
        streaming_sync(self, local_dir, cloud_dir, SyncDirection::LocalToCloud, opts).await
    }

    pub async fn sync_to_local(
        &self, cloud_dir: &str, local_dir: &Path, delete: bool,
        on_progress: impl Fn(&str) + Send + Sync,
    ) -> Result<SyncSummary> {
        let opts = SyncOptions::default().with_delete(delete);
        self.sync_to_local_with_options(cloud_dir, local_dir, &opts, on_progress).await
    }

    pub async fn sync_to_local_with_options(
        &self, cloud_dir: &str, local_dir: &Path, opts: &SyncOptions,
        _on_progress: impl Fn(&str) + Send + Sync,
    ) -> Result<SyncSummary> {
        streaming_sync(self, local_dir, cloud_dir, SyncDirection::CloudToLocal, opts).await
    }
}

// ── 流式并行同步核心 ──

/// BFS 目录队列条目: (本地目录, 云盘目录, rel_prefix)
struct DirJob {
    local_dir: PathBuf,
    cloud_dir: String,
    prefix: String,
}

async fn streaming_sync(
    client: &Yun139Client,
    local_root: &Path,
    cloud_root: &str,
    direction: SyncDirection,
    opts: &SyncOptions,
) -> Result<SyncSummary> {
    let max_parallel = opts.concurrency;

    // 共享计数器
    let uploaded = Arc::new(AtomicU32::new(0));
    let downloaded = Arc::new(AtomicU32::new(0));
    let dirs_created = Arc::new(AtomicU32::new(0));
    let skipped = Arc::new(AtomicU32::new(0));
    let failed = Arc::new(AtomicU32::new(0));
    let failed_files: Arc<std::sync::Mutex<Vec<(String, String)>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));

    // 延迟删除列表（遍历完后串行执行）
    let mut pending_deletes: Vec<SyncAction> = Vec::new();

    // ── indicatif ──
    let mp = MultiProgress::new();

    let scan_pb = mp.add(ProgressBar::new_spinner());
    scan_pb.set_style(
        ProgressStyle::with_template("{spinner:.cyan} {prefix} {msg}")
            .unwrap()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
    );
    scan_pb.set_prefix("scan");
    scan_pb.enable_steady_tick(std::time::Duration::from_millis(100));

    let overall_pb = mp.add(ProgressBar::new(0));
    overall_pb.set_style(
        ProgressStyle::with_template("sync [{bar:30.cyan/dim}] {pos}/{len} ({percent}%) {msg}")
            .unwrap()
            .progress_chars("█▓░"),
    );

    let task_style = ProgressStyle::with_template(
        "     {prefix} [{bar:25.green/dim}] {bytes}/{total_bytes} {bytes_per_sec}",
    )
    .unwrap()
    .progress_chars("━╸─");

    // ── JoinSet ──
    let mut join_set = tokio::task::JoinSet::new();

    // BFS 队列
    let ct = cloud_root.trim_end_matches('/').to_string();
    let mut queue = VecDeque::new();
    queue.push_back(DirJob {
        local_dir: local_root.to_path_buf(),
        cloud_dir: ct.clone(),
        prefix: String::new(),
    });

    // ── 生产者: BFS 遍历 + spawn 任务 ──
    while let Some(job) = queue.pop_front() {
        scan_pb.set_message(if job.prefix.is_empty() {
            "/".to_string()
        } else {
            truncate_name(&job.prefix, 50)
        });

        // 并行获取本地和云盘列表
        let local_dir_owned = job.local_dir.clone();
        let local_entries_handle = tokio::task::spawn_blocking(move || {
            read_local_dir(&local_dir_owned)
        });

        let cloud_items = client.list_all_quiet(&job.cloud_dir).await;

        let local_entries = match local_entries_handle.await {
            Ok(v) => v,
            Err(_) => Vec::new(),
        };

        let cloud_items = cloud_items.unwrap_or_default();

        let local_map: HashMap<&str, &LocalEntry> =
            local_entries.iter().map(|e| (e.name.as_str(), e)).collect();
        let cloud_map: HashMap<&str, &ListItem> =
            cloud_items.iter().map(|e| (e.name.as_str(), e)).collect();

        match direction {
            SyncDirection::LocalToCloud => {
                // 本地有的
                for le in &local_entries {
                    if le.is_dir {
                        if !cloud_map.contains_key(le.name.as_str()) {
                            // 新目录 → 串行创建
                            let cloud_path = rel_cloud(&ct, &job.prefix, &le.name);
                            scan_pb.set_message(format!("📁 {cloud_path}"));
                            match client.ensure_dir(&cloud_path).await {
                                Ok(_) => { dirs_created.fetch_add(1, Ordering::Relaxed); }
                                Err(e) => {
                                    tracing::error!(err = %e, dir = %cloud_path, "mkdir cloud failed");
                                    failed.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                        }
                        // 子目录加入队列
                        queue.push_back(DirJob {
                            local_dir: job.local_dir.join(&le.name),
                            cloud_dir: rel_cloud(&ct, &job.prefix, &le.name),
                            prefix: rel_path(&job.prefix, &le.name),
                        });
                    } else {
                        // 文件：对比 size
                        let need_upload = match cloud_map.get(le.name.as_str()) {
                            None => true,
                            Some(ci) => ci.size != le.size as i64,
                        };
                        if need_upload {
                            overall_pb.inc_length(1);
                            let local = job.local_dir.join(&le.name);
                            let cloud_dir = job.cloud_dir.clone();
                            spawn_upload(
                                &mut join_set, client, local, cloud_dir,
                                &uploaded, &downloaded, &failed, &failed_files,
                                &mp, &overall_pb, &task_style,
                            );
                        } else {
                            skipped.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                // 云盘有、本地无 → 收集删除
                if opts.delete {
                    for ci in &cloud_items {
                        if !local_map.contains_key(ci.name.as_str()) {
                            let cloud = rel_cloud(&ct, &job.prefix, &ci.name);
                            pending_deletes.push(SyncAction::DeleteCloud { cloud });
                        }
                    }
                }
            }
            SyncDirection::CloudToLocal => {
                // 云盘有的
                for ci in &cloud_items {
                    if ci.is_folder {
                        if !local_map.contains_key(ci.name.as_str()) {
                            let local_path = job.local_dir.join(&ci.name);
                            scan_pb.set_message(format!("📁 {}", local_path.display()));
                            match tokio::fs::create_dir_all(&local_path).await {
                                Ok(_) => { dirs_created.fetch_add(1, Ordering::Relaxed); }
                                Err(e) => {
                                    tracing::error!(err = %e, dir = %local_path.display(), "mkdir local failed");
                                    failed.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                        }
                        queue.push_back(DirJob {
                            local_dir: job.local_dir.join(&ci.name),
                            cloud_dir: rel_cloud(&ct, &job.prefix, &ci.name),
                            prefix: rel_path(&job.prefix, &ci.name),
                        });
                    } else {
                        let need_download = match local_map.get(ci.name.as_str()) {
                            None => true,
                            Some(le) => le.size as i64 != ci.size,
                        };
                        if need_download {
                            overall_pb.inc_length(1);
                            let cloud = rel_cloud(&ct, &job.prefix, &ci.name);
                            let local = job.local_dir.join(&ci.name);
                            spawn_download(
                                &mut join_set, client, cloud, local, ci.size as u64,
                                &uploaded, &downloaded, &failed, &failed_files,
                                &mp, &overall_pb, &task_style,
                            );
                        } else {
                            skipped.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                if opts.delete {
                    for le in &local_entries {
                        if !cloud_map.contains_key(le.name.as_str()) {
                            pending_deletes.push(SyncAction::DeleteLocal {
                                local: job.local_dir.join(&le.name),
                            });
                        }
                    }
                }
            }
        }

        // ── 背压: JoinSet 满了就等一个完成 ──
        while join_set.len() >= max_parallel {
            join_set.join_next().await;
        }
    }

    // ── 扫描完毕 ──
    scan_pb.set_style(ProgressStyle::with_template("  {prefix} {msg}").unwrap());
    scan_pb.set_prefix("✓");
    scan_pb.finish_with_message(format!(
        "扫描完成 ({} 跳过, {} 目录, {} 待删除)",
        skipped.load(Ordering::Relaxed),
        dirs_created.load(Ordering::Relaxed),
        pending_deletes.len(),
    ));

    // ── drain 剩余传输任务 ──
    while join_set.join_next().await.is_some() {}
    overall_pb.finish_and_clear();

    // ── 串行执行删除 ──
    if !pending_deletes.is_empty() {
        // 排序：文件先于目录，深层先于浅层
        pending_deletes.sort_by(|a, b| {
            let (a_dir, a_path) = match a {
                SyncAction::DeleteCloud { cloud } => (false, cloud.as_str()),
                SyncAction::DeleteLocal { local } => (local.is_dir(), local.to_str().unwrap_or("")),
                _ => (false, ""),
            };
            let (b_dir, b_path) = match b {
                SyncAction::DeleteCloud { cloud } => (false, cloud.as_str()),
                SyncAction::DeleteLocal { local } => (local.is_dir(), local.to_str().unwrap_or("")),
                _ => (false, ""),
            };
            a_dir.cmp(&b_dir).then_with(|| {
                b_path.matches('/').count().cmp(&a_path.matches('/').count())
            })
        });

        let del_pb = mp.add(ProgressBar::new(pending_deletes.len() as u64));
        del_pb.set_style(
            ProgressStyle::with_template("  🗑️  [{bar:20.red/dim}] {pos}/{len} {msg}")
                .unwrap()
                .progress_chars("━╸─"),
        );
        let deleted = Arc::new(AtomicU32::new(0));

        for action in &pending_deletes {
            match action {
                SyncAction::DeleteCloud { cloud } => {
                    del_pb.set_message(truncate_name(cloud, 40));
                    match client.trash(cloud).await {
                        Ok(_) => { deleted.fetch_add(1, Ordering::Relaxed); }
                        Err(e) => {
                            tracing::error!(err = %e, file = %cloud, "delete cloud failed");
                            failed.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                SyncAction::DeleteLocal { local } => {
                    del_pb.set_message(truncate_name(&local.display().to_string(), 40));
                    let res = if local.is_dir() {
                        tokio::fs::remove_dir_all(local).await
                    } else {
                        tokio::fs::remove_file(local).await
                    };
                    if let Err(e) = res {
                        tracing::error!(err = %e, "delete local failed");
                        failed.fetch_add(1, Ordering::Relaxed);
                    } else {
                        deleted.fetch_add(1, Ordering::Relaxed);
                    }
                }
                _ => {}
            }
            del_pb.inc(1);
        }
        del_pb.finish_and_clear();

        // 打印失败列表
        let failures = failed_files.lock().unwrap();
        if !failures.is_empty() {
            eprintln!("\n以下文件传输失败:");
            for (path, reason) in failures.iter() {
                eprintln!("  {path} — {reason}");
            }
        }

        return Ok(SyncSummary {
            uploaded: uploaded.load(Ordering::Relaxed),
            downloaded: downloaded.load(Ordering::Relaxed),
            dirs_created: dirs_created.load(Ordering::Relaxed),
            deleted: deleted.load(Ordering::Relaxed),
            skipped: skipped.load(Ordering::Relaxed),
            failed: failed.load(Ordering::Relaxed),
        });
    }

    // 打印失败列表
    let failures = failed_files.lock().unwrap();
    if !failures.is_empty() {
        eprintln!("\n以下文件传输失败:");
        for (path, reason) in failures.iter() {
            eprintln!("  {path} — {reason}");
        }
    }

    Ok(SyncSummary {
        uploaded: uploaded.load(Ordering::Relaxed),
        downloaded: downloaded.load(Ordering::Relaxed),
        dirs_created: dirs_created.load(Ordering::Relaxed),
        deleted: 0,
        skipped: skipped.load(Ordering::Relaxed),
        failed: failed.load(Ordering::Relaxed),
    })
}

// ── spawn 辅助 ──

#[allow(clippy::too_many_arguments)]
fn spawn_upload(
    join_set: &mut tokio::task::JoinSet<()>,
    client: &Yun139Client,
    local: PathBuf,
    cloud_dir: String,
    uploaded: &Arc<AtomicU32>,
    downloaded: &Arc<AtomicU32>,
    failed: &Arc<AtomicU32>,
    failed_files: &Arc<std::sync::Mutex<Vec<(String, String)>>>,
    mp: &MultiProgress,
    overall_pb: &ProgressBar,
    task_style: &ProgressStyle,
) {
    let client = client.clone();
    let uploaded = uploaded.clone();
    let downloaded = downloaded.clone();
    let failed = failed.clone();
    let failed_files = failed_files.clone();
    let mp = mp.clone();
    let overall_pb = overall_pb.clone();
    let task_style = task_style.clone();

    join_set.spawn(async move {
        let file_size = tokio::fs::metadata(&local).await.map(|m| m.len()).unwrap_or(0);
        let pb = mp.insert_before(&overall_pb, ProgressBar::new(file_size));
        pb.set_style(task_style);
        let name = local.file_name().unwrap_or_default().to_string_lossy().to_string();
        pb.set_prefix(format!("↑ {}", truncate_name(&name, 28)));

        let pb2 = pb.clone();
        match client.upload_file(&local, &cloud_dir, move |bytes, _| { pb2.set_position(bytes); }).await {
            Ok(_) => {
                pb.set_position(file_size);
                pb.finish_and_clear();
                uploaded.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => {
                let msg = e.to_string();
                pb.abandon_with_message(format!("失败: {}", truncate_name(&msg, 40)));
                failed.fetch_add(1, Ordering::Relaxed);
                failed_files.lock().unwrap().push((format!("↑ {}", local.display()), msg));
            }
        }
        overall_pb.inc(1);
        overall_pb.set_message(format!(
            "↑{} ↓{}", uploaded.load(Ordering::Relaxed), downloaded.load(Ordering::Relaxed),
        ));
    });
}

#[allow(clippy::too_many_arguments)]
fn spawn_download(
    join_set: &mut tokio::task::JoinSet<()>,
    client: &Yun139Client,
    cloud_path: String,
    local: PathBuf,
    est_size: u64,
    uploaded: &Arc<AtomicU32>,
    downloaded: &Arc<AtomicU32>,
    failed: &Arc<AtomicU32>,
    failed_files: &Arc<std::sync::Mutex<Vec<(String, String)>>>,
    mp: &MultiProgress,
    overall_pb: &ProgressBar,
    task_style: &ProgressStyle,
) {
    let client = client.clone();
    let uploaded = uploaded.clone();
    let downloaded = downloaded.clone();
    let failed = failed.clone();
    let failed_files = failed_files.clone();
    let mp = mp.clone();
    let overall_pb = overall_pb.clone();
    let task_style = task_style.clone();

    join_set.spawn(async move {
        // resolve path → download URL
        let (url, size) = match async {
            let item = client.resolve_path(&cloud_path).await?;
            let s = item.size.unwrap_or(0) as u64;
            let fid = item.file_id.as_deref().unwrap_or_default();
            let url = client.get_download_url(fid).await?;
            Ok::<_, Yun139Error>((url, s))
        }.await {
            Ok(v) => v,
            Err(e) => {
                let msg = e.to_string();
                tracing::error!(err = %msg, file = %cloud_path, "resolve/download_url failed");
                failed.fetch_add(1, Ordering::Relaxed);
                failed_files.lock().unwrap().push((format!("↓ {cloud_path}"), msg));
                overall_pb.inc(1);
                return;
            }
        };

        let actual_size = if size > 0 { size } else { est_size };
        let pb = mp.insert_before(&overall_pb, ProgressBar::new(actual_size));
        pb.set_style(task_style);
        let name = cloud_path.rsplit('/').next().unwrap_or(&cloud_path);
        pb.set_prefix(format!("↓ {}", truncate_name(name, 28)));

        let pb2 = pb.clone();
        let result = client.download_parallel(&url, &local, 4, move |bytes, _| {
            pb2.set_position(bytes);
        }).await;

        match result {
            Ok(_) => {
                pb.finish_and_clear();
                downloaded.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => {
                let msg = e.to_string();
                pb.abandon_with_message(format!("失败: {}", truncate_name(&msg, 40)));
                failed.fetch_add(1, Ordering::Relaxed);
                failed_files.lock().unwrap().push((format!("↓ {cloud_path}"), msg));
            }
        }
        overall_pb.inc(1);
        overall_pb.set_message(format!(
            "↑{} ↓{}", uploaded.load(Ordering::Relaxed), downloaded.load(Ordering::Relaxed),
        ));
    });
}

// ── 工具函数 ──

/// 读取单层本地目录条目。
struct LocalEntry {
    name: String,
    is_dir: bool,
    size: u64,
}

fn read_local_dir(dir: &Path) -> Vec<LocalEntry> {
    let rd = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    let mut entries = Vec::new();
    for entry in rd.flatten() {
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') { continue; }
        entries.push(LocalEntry { name, is_dir: meta.is_dir(), size: meta.len() });
    }
    entries
}

fn truncate_name(name: &str, max_len: usize) -> String {
    let count = name.chars().count();
    if count <= max_len { return name.to_string(); }
    if max_len <= 3 { return name.chars().take(max_len).collect(); }
    let tail: String = name.chars().skip(count - (max_len - 3)).collect();
    format!("...{tail}")
}

fn rel_path(prefix: &str, name: &str) -> String {
    if prefix.is_empty() { name.to_string() } else { format!("{prefix}/{name}") }
}

fn rel_cloud(ct: &str, prefix: &str, name: &str) -> String {
    if prefix.is_empty() { format!("{ct}/{name}") } else { format!("{ct}/{prefix}/{name}") }
}

// ── Yun139Client 辅助: 静默 list_all ──

impl Yun139Client {
    /// list_all 但不在找不到目录时报错（新目录可能尚未存在）。
    async fn list_all_quiet(&self, cloud_dir: &str) -> Result<Vec<ListItem>> {
        match self.list_all(cloud_dir).await {
            Ok(r) => Ok(r.items),
            Err(_) => Ok(Vec::new()),
        }
    }
}
