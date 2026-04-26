//! 同步命令 — 本地目录与云盘目录双向同步。
//!
//! 参考 cloud139 (zjy760401/cloud139) 的并发架构：
//!   1. 扫描本地（spawn_blocking）+ 扫描云盘（递归 BFS）
//!   2. 比较差异，生成同步动作列表
//!   3. 目录操作串行执行
//!   4. 文件传输通过 Semaphore + JoinSet 并行执行
//!   5. 删除操作串行执行
//!   6. 单个失败不中断整体（continue-on-error）

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

use crate::config::DEFAULT_PARALLEL;
use crate::error::{Result, Yun139Error};
use crate::Yun139Client;
/// 同步方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncDirection {
    /// 本地 → 云盘
    LocalToCloud,
    /// 云盘 → 本地
    CloudToLocal,
}

/// 同步配置。
#[derive(Debug, Clone)]
pub struct SyncOptions {
    /// 是否删除目标端多余的文件
    pub delete: bool,
    /// 并行文件传输数（默认 4）
    pub concurrency: usize,
}

impl Default for SyncOptions {
    fn default() -> Self {
        Self {
            delete: false,
            concurrency: DEFAULT_PARALLEL,
        }
    }
}

impl SyncOptions {
    pub fn with_delete(mut self, delete: bool) -> Self {
        self.delete = delete;
        self
    }
    pub fn with_concurrency(mut self, n: usize) -> Self {
        self.concurrency = n.max(1);
        self
    }
}

/// 单条同步动作。
#[derive(Debug, Clone)]
pub enum SyncAction {
    /// 上传本地文件到云盘
    Upload { local: PathBuf, cloud: String },
    /// 从云盘下载文件到本地
    Download { cloud: String, local: PathBuf },
    /// 在云盘创建目录
    MkdirCloud { cloud: String },
    /// 在本地创建目录
    MkdirLocal { local: PathBuf },
    /// 删除云盘文件
    DeleteCloud { cloud: String },
    /// 删除本地文件
    DeleteLocal { local: PathBuf },
    /// 跳过（已一致）
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

/// 本地文件条目。
#[derive(Debug, Clone)]
struct LocalEntry {
    rel_path: String,
    is_dir: bool,
    size: u64,
}

/// 云盘文件条目。
#[derive(Debug, Clone)]
struct CloudEntry {
    rel_path: String,
    is_dir: bool,
    size: i64,
}

// ── 公开 API ──

impl Yun139Client {
    /// 单向同步：本地 → 云盘。
    pub async fn sync_to_cloud(
        &self,
        local_dir: &Path,
        cloud_dir: &str,
        delete: bool,
        on_progress: impl Fn(&str) + Send + Sync,
    ) -> Result<SyncSummary> {
        let opts = SyncOptions::default().with_delete(delete);
        self.sync_to_cloud_with_options(local_dir, cloud_dir, &opts, on_progress).await
    }

    /// 单向同步：本地 → 云盘（带自定义选项）。
    pub async fn sync_to_cloud_with_options(
        &self,
        local_dir: &Path,
        cloud_dir: &str,
        opts: &SyncOptions,
        on_progress: impl Fn(&str) + Send + Sync,
    ) -> Result<SyncSummary> {
        let actions = self.compute_sync_actions(
            local_dir, cloud_dir, SyncDirection::LocalToCloud, opts.delete,
        ).await?;
        self.execute_sync(actions, opts.concurrency, &on_progress).await
    }

    /// 单向同步：云盘 → 本地。
    pub async fn sync_to_local(
        &self,
        cloud_dir: &str,
        local_dir: &Path,
        delete: bool,
        on_progress: impl Fn(&str) + Send + Sync,
    ) -> Result<SyncSummary> {
        let opts = SyncOptions::default().with_delete(delete);
        self.sync_to_local_with_options(cloud_dir, local_dir, &opts, on_progress).await
    }

    /// 单向同步：云盘 → 本地（带自定义选项）。
    pub async fn sync_to_local_with_options(
        &self,
        cloud_dir: &str,
        local_dir: &Path,
        opts: &SyncOptions,
        on_progress: impl Fn(&str) + Send + Sync,
    ) -> Result<SyncSummary> {
        let actions = self.compute_sync_actions(
            local_dir, cloud_dir, SyncDirection::CloudToLocal, opts.delete,
        ).await?;
        self.execute_sync(actions, opts.concurrency, &on_progress).await
    }
}

// ── diff 计算 ──

impl Yun139Client {
    async fn compute_sync_actions(
        &self,
        local_dir: &Path,
        cloud_dir: &str,
        direction: SyncDirection,
        delete: bool,
    ) -> Result<Vec<SyncAction>> {
        // 本地扫描放到 blocking 线程，避免阻塞 tokio runtime
        let local_dir_owned = local_dir.to_path_buf();
        let local_entries = tokio::task::spawn_blocking(move || {
            scan_local_recursive(&local_dir_owned)
        })
        .await
        .map_err(|e| Yun139Error::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))??;

        let local_map: HashMap<&str, &LocalEntry> =
            local_entries.iter().map(|e| (e.rel_path.as_str(), e)).collect();

        let cloud_entries = self.scan_cloud_recursive(cloud_dir).await?;
        let cloud_map: HashMap<&str, &CloudEntry> =
            cloud_entries.iter().map(|e| (e.rel_path.as_str(), e)).collect();

        let ct = cloud_dir.trim_end_matches('/');
        let mut actions = Vec::new();

        match direction {
            SyncDirection::LocalToCloud => {
                // 目录优先（按深度排序确保父目录先创建）
                let mut dirs: Vec<&LocalEntry> = local_entries.iter().filter(|e| e.is_dir).collect();
                dirs.sort_by_key(|e| e.rel_path.matches('/').count());
                for entry in dirs {
                    if !cloud_map.contains_key(entry.rel_path.as_str()) {
                        actions.push(SyncAction::MkdirCloud {
                            cloud: format!("{ct}/{}", entry.rel_path),
                        });
                    }
                }
                // 文件：不存在或大小不匹配 → 上传
                for entry in local_entries.iter().filter(|e| !e.is_dir) {
                    let parent_rel = Path::new(&entry.rel_path)
                        .parent()
                        .and_then(|p| p.to_str())
                        .unwrap_or("");
                    let target = if parent_rel.is_empty() {
                        cloud_dir.to_string()
                    } else {
                        format!("{ct}/{parent_rel}")
                    };
                    match cloud_map.get(entry.rel_path.as_str()) {
                        Some(ce) if ce.size == entry.size as i64 => {
                            actions.push(SyncAction::Skip { name: entry.rel_path.clone() });
                        }
                        _ => {
                            actions.push(SyncAction::Upload {
                                local: local_dir.join(&entry.rel_path),
                                cloud: target,
                            });
                        }
                    }
                }
                if delete {
                    push_delete_cloud(&local_map, &cloud_entries, ct, &mut actions);
                }
            }
            SyncDirection::CloudToLocal => {
                let mut dirs: Vec<&CloudEntry> = cloud_entries.iter().filter(|e| e.is_dir).collect();
                dirs.sort_by_key(|e| e.rel_path.matches('/').count());
                for entry in dirs {
                    if !local_map.contains_key(entry.rel_path.as_str()) {
                        actions.push(SyncAction::MkdirLocal {
                            local: local_dir.join(&entry.rel_path),
                        });
                    }
                }
                for entry in cloud_entries.iter().filter(|e| !e.is_dir) {
                    match local_map.get(entry.rel_path.as_str()) {
                        Some(le) if le.size as i64 == entry.size => {
                            actions.push(SyncAction::Skip { name: entry.rel_path.clone() });
                        }
                        _ => {
                            actions.push(SyncAction::Download {
                                cloud: format!("{ct}/{}", entry.rel_path),
                                local: local_dir.join(&entry.rel_path),
                            });
                        }
                    }
                }
                if delete {
                    push_delete_local(&cloud_map, &local_entries, local_dir, &mut actions);
                }
            }
        }

        Ok(actions)
    }
}

/// 生成云盘端删除动作（文件先于目录，深层先于浅层）。
fn push_delete_cloud(
    local_map: &HashMap<&str, &LocalEntry>,
    cloud_entries: &[CloudEntry],
    ct: &str,
    actions: &mut Vec<SyncAction>,
) {
    let mut to_delete: Vec<&CloudEntry> = cloud_entries
        .iter()
        .filter(|e| !local_map.contains_key(e.rel_path.as_str()))
        .collect();
    to_delete.sort_by(|a, b| {
        a.is_dir.cmp(&b.is_dir).then_with(|| {
            b.rel_path.matches('/').count().cmp(&a.rel_path.matches('/').count())
        })
    });
    for entry in to_delete {
        actions.push(SyncAction::DeleteCloud {
            cloud: format!("{ct}/{}", entry.rel_path),
        });
    }
}

/// 生成本地端删除动作。
fn push_delete_local(
    cloud_map: &HashMap<&str, &CloudEntry>,
    local_entries: &[LocalEntry],
    local_dir: &Path,
    actions: &mut Vec<SyncAction>,
) {
    let mut to_delete: Vec<&LocalEntry> = local_entries
        .iter()
        .filter(|e| !cloud_map.contains_key(e.rel_path.as_str()))
        .collect();
    to_delete.sort_by(|a, b| {
        a.is_dir.cmp(&b.is_dir).then_with(|| {
            b.rel_path.matches('/').count().cmp(&a.rel_path.matches('/').count())
        })
    });
    for entry in to_delete {
        actions.push(SyncAction::DeleteLocal {
            local: local_dir.join(&entry.rel_path),
        });
    }
}

// ── 云盘递归扫描 ──

impl Yun139Client {
    async fn scan_cloud_recursive(&self, cloud_dir: &str) -> Result<Vec<CloudEntry>> {
        let mut result = Vec::new();
        self.scan_cloud_inner(cloud_dir, "", &mut result).await?;
        Ok(result)
    }

    async fn scan_cloud_inner(
        &self,
        cloud_dir: &str,
        prefix: &str,
        out: &mut Vec<CloudEntry>,
    ) -> Result<()> {
        let items = match self.list_all(cloud_dir).await {
            Ok(r) => r.items,
            Err(_) => return Ok(()),
        };

        for item in &items {
            let rel = if prefix.is_empty() {
                item.name.clone()
            } else {
                format!("{}/{}", prefix, item.name)
            };

            out.push(CloudEntry {
                rel_path: rel.clone(),
                is_dir: item.is_folder,
                size: item.size,
            });

            if item.is_folder {
                let sub = format!("{}/{}", cloud_dir.trim_end_matches('/'), item.name);
                Box::pin(self.scan_cloud_inner(&sub, &rel, out)).await?;
            }
        }
        Ok(())
    }
}

// ── 三阶段并行执行引擎（indicatif 进度条） ──

/// 截断文件名用于进度条显示。
fn truncate_name(name: &str, max_len: usize) -> String {
    let count = name.chars().count();
    if count <= max_len {
        name.to_string()
    } else if max_len <= 3 {
        name.chars().take(max_len).collect()
    } else {
        let tail: String = name.chars().skip(count - (max_len - 3)).collect();
        format!("...{tail}")
    }
}

impl Yun139Client {
    /// 执行同步动作：目录串行 → 文件并行(Semaphore+JoinSet) → 删除串行。
    ///
    /// 使用 indicatif 进度条显示实时状态（参考 cloud139）。
    async fn execute_sync(
        &self,
        actions: Vec<SyncAction>,
        concurrency: usize,
        _on_progress: &(impl Fn(&str) + Send + Sync),
    ) -> Result<SyncSummary> {
        let uploaded = Arc::new(AtomicU32::new(0));
        let downloaded = Arc::new(AtomicU32::new(0));
        let dirs_created = Arc::new(AtomicU32::new(0));
        let deleted = Arc::new(AtomicU32::new(0));
        let skipped = Arc::new(AtomicU32::new(0));
        let failed = Arc::new(AtomicU32::new(0));
        let failed_files: Arc<std::sync::Mutex<Vec<(String, String)>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));

        // 统计各类 action 数
        let total_transfers = actions.iter()
            .filter(|a| matches!(a, SyncAction::Upload { .. } | SyncAction::Download { .. }))
            .count() as u64;
        let total_deletes = actions.iter()
            .filter(|a| matches!(a, SyncAction::DeleteCloud { .. } | SyncAction::DeleteLocal { .. }))
            .count();

        // ── indicatif 进度条 ──
        let mp = MultiProgress::new();

        let scan_pb = mp.add(ProgressBar::new_spinner());
        scan_pb.set_style(
            ProgressStyle::with_template("{spinner:.cyan} {prefix} {msg}")
                .unwrap()
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
        );
        scan_pb.set_prefix("scan");
        scan_pb.enable_steady_tick(std::time::Duration::from_millis(100));

        let overall_style = ProgressStyle::with_template(
            "sync [{bar:30.cyan/dim}] {pos}/{len} ({percent}%) {msg}",
        )
        .unwrap()
        .progress_chars("█▓░");

        let overall_pb = mp.add(ProgressBar::new(total_transfers));
        overall_pb.set_style(overall_style);

        let task_style = ProgressStyle::with_template(
            "     {prefix} [{bar:25.green/dim}] {bytes}/{total_bytes} {bytes_per_sec} {msg}",
        )
        .unwrap()
        .progress_chars("━╸─");

        // ── Phase 1: 目录 + skip（串行） ──
        for action in &actions {
            match action {
                SyncAction::MkdirCloud { cloud } => {
                    scan_pb.set_message(format!("📁 {cloud}"));
                    match self.ensure_dir(cloud).await {
                        Ok(_) => { dirs_created.fetch_add(1, Ordering::Relaxed); }
                        Err(e) => {
                            tracing::error!(err = %e, dir = %cloud, "mkdir cloud failed");
                            failed.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                SyncAction::MkdirLocal { local } => {
                    scan_pb.set_message(format!("📁 {}", local.display()));
                    match tokio::fs::create_dir_all(local).await {
                        Ok(_) => { dirs_created.fetch_add(1, Ordering::Relaxed); }
                        Err(e) => {
                            tracing::error!(err = %e, dir = %local.display(), "mkdir local failed");
                            failed.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                SyncAction::Skip { .. } => {
                    skipped.fetch_add(1, Ordering::Relaxed);
                }
                _ => {}
            }
        }

        let dir_count = dirs_created.load(Ordering::Relaxed);
        let skip_count = skipped.load(Ordering::Relaxed);
        scan_pb.set_style(ProgressStyle::with_template("  {prefix} {msg}").unwrap());
        scan_pb.set_prefix("✓");
        scan_pb.finish_with_message(format!(
            "扫描完成: {} 个目录, {} 个传输, {} 个跳过, {} 个删除",
            dir_count, total_transfers, skip_count, total_deletes
        ));

        // ── Phase 2: 文件传输（Semaphore + JoinSet + 每文件进度条） ──
        if total_transfers > 0 {
            let sem = Arc::new(tokio::sync::Semaphore::new(concurrency));
            let mut join_set = tokio::task::JoinSet::new();

            for action in &actions {
                match action {
                    SyncAction::Upload { local, cloud } => {
                        let sem = sem.clone();
                        let client = self.clone();
                        let uploaded = uploaded.clone();
                        let downloaded = downloaded.clone();
                        let failed = failed.clone();
                        let failed_files = failed_files.clone();
                        let local = local.clone();
                        let cloud = cloud.clone();
                        let mp = mp.clone();
                        let overall_pb = overall_pb.clone();
                        let task_style = task_style.clone();

                        join_set.spawn(async move {
                            let _permit = sem.acquire().await.unwrap();

                            // 获取文件大小
                            let file_size = tokio::fs::metadata(&local).await
                                .map(|m| m.len()).unwrap_or(0);

                            let pb = mp.insert_before(&overall_pb, ProgressBar::new(file_size));
                            pb.set_style(task_style);
                            let display = truncate_name(&local.file_name().unwrap_or_default().to_string_lossy(), 30);
                            pb.set_prefix(format!("↑ {display}"));

                            let pb2 = pb.clone();
                            match client.upload_file(&local, &cloud, move |bytes, _total| {
                                pb2.set_position(bytes);
                            }).await {
                                Ok(_) => {
                                    pb.set_position(file_size);
                                    pb.finish_and_clear();
                                    uploaded.fetch_add(1, Ordering::Relaxed);
                                }
                                Err(e) => {
                                    let msg = e.to_string();
                                    pb.abandon_with_message(format!("失败: {msg}"));
                                    failed.fetch_add(1, Ordering::Relaxed);
                                    failed_files.lock().unwrap().push((
                                        format!("↑ {}", local.display()), msg,
                                    ));
                                }
                            }
                            overall_pb.inc(1);
                            overall_pb.set_message(format!(
                                "↑{} ↓{}",
                                uploaded.load(Ordering::Relaxed),
                                downloaded.load(Ordering::Relaxed),
                            ));
                        });
                    }
                    SyncAction::Download { cloud, local } => {
                        let sem = sem.clone();
                        let client = self.clone();
                        let downloaded = downloaded.clone();
                        let uploaded = uploaded.clone();
                        let failed = failed.clone();
                        let failed_files = failed_files.clone();
                        let cloud = cloud.clone();
                        let local = local.clone();
                        let mp = mp.clone();
                        let overall_pb = overall_pb.clone();
                        let task_style = task_style.clone();

                        join_set.spawn(async move {
                            let _permit = sem.acquire().await.unwrap();

                            // 先 resolve 路径获取文件大小
                            let (url, est_size) = match async {
                                let item = client.resolve_path(&cloud).await?;
                                let size = item.size.unwrap_or(0) as u64;
                                let fid = item.file_id.as_deref().unwrap_or_default();
                                let url = client.get_download_url(fid).await?;
                                Ok::<_, Yun139Error>((url, size))
                            }.await {
                                Ok(v) => v,
                                Err(e) => {
                                    let msg = e.to_string();
                                    tracing::error!(err = %msg, file = %cloud, "resolve failed");
                                    failed.fetch_add(1, Ordering::Relaxed);
                                    failed_files.lock().unwrap().push((
                                        format!("↓ {cloud}"), msg,
                                    ));
                                    overall_pb.inc(1);
                                    return;
                                }
                            };

                            let pb = mp.insert_before(&overall_pb, ProgressBar::new(est_size));
                            pb.set_style(task_style);
                            let name = cloud.rsplit('/').next().unwrap_or(&cloud);
                            let display = truncate_name(name, 30);
                            pb.set_prefix(format!("↓ {display}"));

                            let pb2 = pb.clone();
                            let result = client.download_parallel(
                                &url, &local, 4,
                                move |bytes, _total| { pb2.set_position(bytes); },
                            ).await;

                            match result {
                                Ok(_) => {
                                    pb.finish_and_clear();
                                    downloaded.fetch_add(1, Ordering::Relaxed);
                                }
                                Err(e) => {
                                    let msg = e.to_string();
                                    pb.abandon_with_message(format!("失败: {msg}"));
                                    failed.fetch_add(1, Ordering::Relaxed);
                                    failed_files.lock().unwrap().push((
                                        format!("↓ {cloud}"), msg,
                                    ));
                                }
                            }
                            overall_pb.inc(1);
                            overall_pb.set_message(format!(
                                "↑{} ↓{}",
                                uploaded.load(Ordering::Relaxed),
                                downloaded.load(Ordering::Relaxed),
                            ));
                        });
                    }
                    _ => {}
                }
            }

            // 等待所有传输完成
            while join_set.join_next().await.is_some() {}
        }

        overall_pb.finish_and_clear();

        // ── Phase 3: 删除（串行） ──
        if total_deletes > 0 {
            let del_pb = mp.add(ProgressBar::new(total_deletes as u64));
            del_pb.set_style(
                ProgressStyle::with_template("  🗑️  [{bar:20.red/dim}] {pos}/{len} {msg}")
                    .unwrap()
                    .progress_chars("━╸─"),
            );

            for action in &actions {
                match action {
                    SyncAction::DeleteCloud { cloud } => {
                        del_pb.set_message(truncate_name(cloud, 40));
                        match self.trash(cloud).await {
                            Ok(_) => { deleted.fetch_add(1, Ordering::Relaxed); }
                            Err(e) => {
                                tracing::error!(err = %e, file = %cloud, "delete cloud failed");
                                failed.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                        del_pb.inc(1);
                    }
                    SyncAction::DeleteLocal { local } => {
                        del_pb.set_message(truncate_name(&local.display().to_string(), 40));
                        let res = if local.is_dir() {
                            tokio::fs::remove_dir_all(local).await
                        } else {
                            tokio::fs::remove_file(local).await
                        };
                        if let Err(e) = res {
                            tracing::error!(err = %e, file = %local.display(), "delete local failed");
                            failed.fetch_add(1, Ordering::Relaxed);
                        } else {
                            deleted.fetch_add(1, Ordering::Relaxed);
                        }
                        del_pb.inc(1);
                    }
                    _ => {}
                }
            }
            del_pb.finish_and_clear();
        }

        // 打印失败文件列表
        let failures = failed_files.lock().unwrap();
        if !failures.is_empty() {
            eprintln!();
            eprintln!("以下文件传输失败:");
            for (path, reason) in failures.iter() {
                eprintln!("  {path} — {reason}");
            }
        }

        Ok(SyncSummary {
            uploaded: uploaded.load(Ordering::Relaxed),
            downloaded: downloaded.load(Ordering::Relaxed),
            dirs_created: dirs_created.load(Ordering::Relaxed),
            deleted: deleted.load(Ordering::Relaxed),
            skipped: skipped.load(Ordering::Relaxed),
            failed: failed.load(Ordering::Relaxed),
        })
    }
}

// ── 本地递归扫描 ──

fn scan_local_recursive(dir: &Path) -> Result<Vec<LocalEntry>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    if !dir.is_dir() {
        return Err(Yun139Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotADirectory,
            format!("{} is not a directory", dir.display()),
        )));
    }
    let mut entries = Vec::new();
    scan_local_inner(dir, "", &mut entries)?;
    Ok(entries)
}

fn scan_local_inner(dir: &Path, prefix: &str, out: &mut Vec<LocalEntry>) -> Result<()> {
    let rd = std::fs::read_dir(dir).map_err(Yun139Error::Io)?;
    for entry in rd {
        let entry = entry.map_err(Yun139Error::Io)?;
        let meta = entry.metadata().map_err(Yun139Error::Io)?;
        let name = entry.file_name().to_string_lossy().to_string();

        if name.starts_with('.') {
            continue;
        }

        let rel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };

        out.push(LocalEntry {
            rel_path: rel.clone(),
            is_dir: meta.is_dir(),
            size: meta.len(),
        });

        if meta.is_dir() {
            scan_local_inner(&dir.join(&name), &rel, out)?;
        }
    }
    Ok(())
}
