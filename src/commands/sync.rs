//! 同步命令 — 本地目录与云盘目录流式并行同步。
//!
//! **并行模型**:
//!
//! ```text
//!   scan (递归 spawn)
//!     ├─ 子目录 → tokio::spawn(scan_dir)     ← scan_sem(P) 控制
//!     ├─ 文件上传/下载 → JoinSet.spawn()      ← 不限大小
//!     │    └─ acquire transfer_sem permit      ← transfer_sem(P) 控制
//!     └─ 删除 → pending_deletes
//!
//!   scan_sem(P):     控制同时扫描的目录数
//!   transfer_sem(P): 控制同时执行的传输数
//!   总并行度 ≈ P(scan) + P(transfer) = 2P
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use tokio::sync::Mutex as TokioMutex;

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

// ── 共享上下文 ──

/// 在所有 scan/transfer 协程间共享的状态。
struct SyncCtx {
    client: Yun139Client,
    direction: SyncDirection,
    delete: bool,
    cloud_root: String,
    parallel: usize,

    // JoinSet（TokioMutex 保护，因为多个 scan 协程会并发 push）
    join_set: TokioMutex<tokio::task::JoinSet<()>>,
    // 传输信号量（控制同时执行的上传/下载数）
    transfer_sem: Arc<tokio::sync::Semaphore>,
    // 扫描信号量（控制同时扫描的目录数）
    scan_sem: Arc<tokio::sync::Semaphore>,

    // 延迟删除（scan 完毕后串行执行）
    pending_deletes: TokioMutex<Vec<SyncAction>>,

    // 计数器
    uploaded: Arc<AtomicU32>,
    downloaded: Arc<AtomicU32>,
    dirs_created: Arc<AtomicU32>,
    skipped: Arc<AtomicU32>,
    failed: Arc<AtomicU32>,
    failed_files: Arc<std::sync::Mutex<Vec<(String, String)>>>,

    // 进度条
    mp: MultiProgress,
    scan_pb: ProgressBar,
    overall_pb: ProgressBar,
    task_style: ProgressStyle,
}

// ── 入口 ──

async fn streaming_sync(
    client: &Yun139Client,
    local_root: &Path,
    cloud_root: &str,
    direction: SyncDirection,
    opts: &SyncOptions,
) -> Result<SyncSummary> {
    let p = opts.concurrency;
    let ct = cloud_root.trim_end_matches('/').to_string();

    // 进度条
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

    let ctx = Arc::new(SyncCtx {
        client: client.clone(),
        direction,
        delete: opts.delete,
        cloud_root: ct.clone(),
        parallel: p,
        join_set: TokioMutex::new(tokio::task::JoinSet::new()),
        transfer_sem: Arc::new(tokio::sync::Semaphore::new(p)),
        scan_sem: Arc::new(tokio::sync::Semaphore::new(p)),
        pending_deletes: TokioMutex::new(Vec::new()),
        uploaded: Arc::new(AtomicU32::new(0)),
        downloaded: Arc::new(AtomicU32::new(0)),
        dirs_created: Arc::new(AtomicU32::new(0)),
        skipped: Arc::new(AtomicU32::new(0)),
        failed: Arc::new(AtomicU32::new(0)),
        failed_files: Arc::new(std::sync::Mutex::new(Vec::new())),
        mp: mp.clone(),
        scan_pb: scan_pb.clone(),
        overall_pb: overall_pb.clone(),
        task_style,
    });

    // ── 递归扫描（自动扩散，无并行度限制） ──
    scan_dir(
        ctx.clone(),
        local_root.to_path_buf(),
        ct.clone(),
        String::new(),
        false,
    ).await;

    // ── 扫描完毕 ──
    scan_pb.set_style(ProgressStyle::with_template("  {prefix} {msg}").unwrap());
    scan_pb.set_prefix("✓");
    let pending_len = ctx.pending_deletes.lock().await.len();
    scan_pb.finish_with_message(format!(
        "扫描完成 ({} 跳过, {} 目录, {} 待删除)",
        ctx.skipped.load(Ordering::Relaxed),
        ctx.dirs_created.load(Ordering::Relaxed),
        pending_len,
    ));

    // ── drain JoinSet 中剩余传输任务 ──
    {
        let mut js = ctx.join_set.lock().await;
        while js.join_next().await.is_some() {}
    }
    overall_pb.finish_and_clear();

    // ── 串行执行删除 ──
    let mut pending = ctx.pending_deletes.lock().await.clone();
    let deleted = Arc::new(AtomicU32::new(0));

    if !pending.is_empty() {
        pending.sort_by(|a, b| {
            let (ad, ap) = delete_sort_key(a);
            let (bd, bp) = delete_sort_key(b);
            ad.cmp(&bd).then_with(|| bp.matches('/').count().cmp(&ap.matches('/').count()))
        });

        let del_pb = mp.add(ProgressBar::new(pending.len() as u64));
        del_pb.set_style(
            ProgressStyle::with_template("  🗑️  [{bar:20.red/dim}] {pos}/{len} {msg}")
                .unwrap().progress_chars("━╸─"),
        );

        for action in &pending {
            match action {
                SyncAction::DeleteCloud { cloud } => {
                    del_pb.set_message(truncate_name(cloud, 40));
                    match client.trash(cloud).await {
                        Ok(_) => { deleted.fetch_add(1, Ordering::Relaxed); }
                        Err(e) => {
                            tracing::error!(err = %e, file = %cloud, "delete cloud failed");
                            ctx.failed.fetch_add(1, Ordering::Relaxed);
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
                    match res {
                        Ok(_) => { deleted.fetch_add(1, Ordering::Relaxed); }
                        Err(e) => {
                            tracing::error!(err = %e, "delete local failed");
                            ctx.failed.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                _ => {}
            }
            del_pb.inc(1);
        }
        del_pb.finish_and_clear();
    }

    // 打印失败列表
    let failures = ctx.failed_files.lock().unwrap();
    if !failures.is_empty() {
        eprintln!("\n以下文件传输失败:");
        for (path, reason) in failures.iter() {
            eprintln!("  {path} — {reason}");
        }
    }

    Ok(SyncSummary {
        uploaded: ctx.uploaded.load(Ordering::Relaxed),
        downloaded: ctx.downloaded.load(Ordering::Relaxed),
        dirs_created: ctx.dirs_created.load(Ordering::Relaxed),
        deleted: deleted.load(Ordering::Relaxed),
        skipped: ctx.skipped.load(Ordering::Relaxed),
        failed: ctx.failed.load(Ordering::Relaxed),
    })
}

fn delete_sort_key(a: &SyncAction) -> (bool, &str) {
    match a {
        SyncAction::DeleteCloud { cloud } => (false, cloud.as_str()),
        SyncAction::DeleteLocal { local } => (local.is_dir(), local.to_str().unwrap_or("")),
        _ => (false, ""),
    }
}

// ── 递归扫描（无并行度限制，自由扩散） ──

/// 扫描单个目录：对比本地 vs 云盘，子目录递归 spawn，文件差异推入 JoinSet。
fn scan_dir(
    ctx: Arc<SyncCtx>,
    local_dir: PathBuf,
    cloud_dir: String,
    prefix: String,
    local_only: bool,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
    Box::pin(scan_dir_inner(ctx, local_dir, cloud_dir, prefix, local_only))
}

async fn scan_dir_inner(
    ctx: Arc<SyncCtx>,
    local_dir: PathBuf,
    cloud_dir: String,
    prefix: String,
    local_only: bool,
) {
    // 获取扫描 permit（控制同时扫描的目录数）
    let _scan_permit = ctx.scan_sem.acquire().await.unwrap();

    ctx.scan_pb.set_message(if prefix.is_empty() {
        "/".to_string()
    } else {
        truncate_name(&prefix, 50)
    });

    // 并行获取本地和云盘列表
    let ld = local_dir.clone();
    let local_handle = tokio::task::spawn_blocking(move || read_local_dir(&ld));

    let cloud_items = if local_only {
        Vec::new()
    } else {
        ctx.client.list_all_quiet(&cloud_dir).await.unwrap_or_default()
    };

    let local_entries = local_handle.await.unwrap_or_default();

    let local_map: HashMap<&str, &LocalEntry> =
        local_entries.iter().map(|e| (e.name.as_str(), e)).collect();
    let cloud_map: HashMap<&str, &ListItem> =
        cloud_items.iter().map(|e| (e.name.as_str(), e)).collect();

    let ct = &ctx.cloud_root;

    match ctx.direction {
        SyncDirection::LocalToCloud => {
            // ── Phase A: 对比，收集结果 ──
            struct DirInfo { name: String, cloud_path: String, is_new: bool }
            struct FileInfo { local: PathBuf, cloud_dir: String }

            let mut new_dirs: Vec<DirInfo> = Vec::new();
            let mut to_upload: Vec<FileInfo> = Vec::new();
            let mut skip_count: u32 = 0;

            for le in local_entries.iter().filter(|e| e.is_dir) {
                let cloud_path = rel_cloud(ct, &prefix, &le.name);
                let is_new = !cloud_map.contains_key(le.name.as_str());
                new_dirs.push(DirInfo { name: le.name.clone(), cloud_path, is_new });
            }

            for le in local_entries.iter().filter(|e| !e.is_dir) {
                let need = match cloud_map.get(le.name.as_str()) {
                    None => true,
                    Some(ci) => ci.size != le.size as i64,
                };
                if need {
                    to_upload.push(FileInfo {
                        local: local_dir.join(&le.name),
                        cloud_dir: cloud_dir.clone(),
                    });
                } else {
                    skip_count += 1;
                }
            }

            // ── Phase B: 更新进度条 ──
            if !to_upload.is_empty() {
                ctx.overall_pb.inc_length(to_upload.len() as u64);
            }
            ctx.skipped.fetch_add(skip_count, Ordering::Relaxed);

            // ── Phase C: 创建目录 + spawn 子目录 scan ──
            let mut sub_handles = Vec::new();
            for d in &new_dirs {
                if d.is_new {
                    ctx.scan_pb.set_message(format!("📁 {}", d.cloud_path));
                    match ctx.client.ensure_dir(&d.cloud_path).await {
                        Ok(_) => { ctx.dirs_created.fetch_add(1, Ordering::Relaxed); }
                        Err(e) => {
                            tracing::error!(err = %e, dir = %d.cloud_path, "mkdir cloud failed");
                            ctx.failed.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                let ctx2 = ctx.clone();
                let sub_local = local_dir.join(&d.name);
                let cp = d.cloud_path.clone();
                let sp = rel_path(&prefix, &d.name);
                let lo = d.is_new;
                sub_handles.push(tokio::spawn(async move {
                    scan_dir(ctx2, sub_local, cp, sp, lo).await;
                }));
            }

            // ── Phase D: 排队提交传输到 JoinSet ──
            for f in to_upload {
                push_upload(&ctx, f.local, f.cloud_dir).await;
            }

            // ── Phase E: 收集删除 ──
            if ctx.delete {
                for ci in &cloud_items {
                    if !local_map.contains_key(ci.name.as_str()) {
                        let cloud = rel_cloud(ct, &prefix, &ci.name);
                        if ci.is_folder {
                            collect_cloud_deletes(&ctx.client, &cloud, &ctx.pending_deletes).await;
                        }
                        ctx.pending_deletes.lock().await.push(SyncAction::DeleteCloud { cloud });
                    }
                }
            }

            // ── Phase F: 等待子目录 scan 完成 ──
            for h in sub_handles {
                let _ = h.await;
            }
        }
        SyncDirection::CloudToLocal => {
            // ── Phase A: 对比，收集结果 ──
            struct DirInfo { name: String, is_new: bool }
            struct FileInfo { cloud: String, local: PathBuf, size: u64 }

            let mut new_dirs: Vec<DirInfo> = Vec::new();
            let mut to_download: Vec<FileInfo> = Vec::new();
            let mut skip_count: u32 = 0;

            for ci in cloud_items.iter().filter(|e| e.is_folder) {
                let is_new = !local_map.contains_key(ci.name.as_str());
                new_dirs.push(DirInfo { name: ci.name.clone(), is_new });
            }

            for ci in cloud_items.iter().filter(|e| !e.is_folder) {
                let need = match local_map.get(ci.name.as_str()) {
                    None => true,
                    Some(le) => le.size as i64 != ci.size,
                };
                if need {
                    to_download.push(FileInfo {
                        cloud: rel_cloud(ct, &prefix, &ci.name),
                        local: local_dir.join(&ci.name),
                        size: ci.size as u64,
                    });
                } else {
                    skip_count += 1;
                }
            }

            // ── Phase B: 更新进度条 ──
            if !to_download.is_empty() {
                ctx.overall_pb.inc_length(to_download.len() as u64);
            }
            ctx.skipped.fetch_add(skip_count, Ordering::Relaxed);

            // ── Phase C: 创建目录 + spawn 子目录 scan ──
            let mut sub_handles = Vec::new();
            for d in &new_dirs {
                if d.is_new {
                    let lp = local_dir.join(&d.name);
                    ctx.scan_pb.set_message(format!("📁 {}", lp.display()));
                    match tokio::fs::create_dir_all(&lp).await {
                        Ok(_) => { ctx.dirs_created.fetch_add(1, Ordering::Relaxed); }
                        Err(e) => {
                            tracing::error!(err = %e, dir = %lp.display(), "mkdir local failed");
                            ctx.failed.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                let ctx2 = ctx.clone();
                let sub_local = local_dir.join(&d.name);
                let sub_cloud = rel_cloud(ct, &prefix, &d.name);
                let sp = rel_path(&prefix, &d.name);
                sub_handles.push(tokio::spawn(async move {
                    scan_dir(ctx2, sub_local, sub_cloud, sp, false).await;
                }));
            }

            // ── Phase D: 排队提交传输到 JoinSet ──
            for f in to_download {
                push_download(&ctx, f.cloud, f.local, f.size).await;
            }

            // ── Phase E: 收集删除 ──
            if ctx.delete {
                for le in &local_entries {
                    if !cloud_map.contains_key(le.name.as_str()) {
                        ctx.pending_deletes.lock().await.push(SyncAction::DeleteLocal {
                            local: local_dir.join(&le.name),
                        });
                    }
                }
            }

            // ── Phase F: 等待子目录 scan 完成 ──
            for h in sub_handles {
                let _ = h.await;
            }
        }
    }
}

// ── JoinSet push（无背压，并发由 Semaphore 独控） ──

/// 推入上传任务到 JoinSet。并发由 transfer_sem 控制。
async fn push_upload(ctx: &Arc<SyncCtx>, local: PathBuf, cloud_dir: String) {
    let ctx2 = ctx.clone();
    let sem = ctx.transfer_sem.clone();
    let mut js = ctx.join_set.lock().await;
    js.spawn(async move {
        let _permit = sem.acquire().await.unwrap();
        do_upload_task(&ctx2, local, cloud_dir).await;
    });
}

/// 推入下载任务到 JoinSet。并发由 transfer_sem 控制。
async fn push_download(ctx: &Arc<SyncCtx>, cloud: String, local: PathBuf, est_size: u64) {
    let ctx2 = ctx.clone();
    let sem = ctx.transfer_sem.clone();
    let parallel = ctx.parallel;
    let mut js = ctx.join_set.lock().await;
    js.spawn(async move {
        let _permit = sem.acquire().await.unwrap();
        do_download_task(&ctx2, cloud, local, est_size, parallel).await;
    });
}

// ── 传输任务实现 ──

async fn do_upload_task(ctx: &SyncCtx, local: PathBuf, cloud_dir: String) {
    let file_size = tokio::fs::metadata(&local).await.map(|m| m.len()).unwrap_or(0);
    let pb = ctx.mp.insert_before(&ctx.overall_pb, ProgressBar::new(file_size));
    pb.set_style(ctx.task_style.clone());
    let name = local.file_name().unwrap_or_default().to_string_lossy().to_string();
    pb.set_prefix(format!("↑ {}", truncate_name(&name, 28)));

    let pb2 = pb.clone();
    match ctx.client.upload_file(&local, &cloud_dir, move |bytes, _| { pb2.set_position(bytes); }).await {
        Ok(_) => {
            pb.set_position(file_size);
            pb.finish_and_clear();
            ctx.uploaded.fetch_add(1, Ordering::Relaxed);
        }
        Err(e) => {
            let msg = e.to_string();
            pb.abandon_with_message(format!("失败: {}", truncate_name(&msg, 40)));
            ctx.failed.fetch_add(1, Ordering::Relaxed);
            ctx.failed_files.lock().unwrap().push((format!("↑ {}", local.display()), msg));
        }
    }
    ctx.overall_pb.inc(1);
    ctx.overall_pb.set_message(format!(
        "↑{} ↓{}", ctx.uploaded.load(Ordering::Relaxed), ctx.downloaded.load(Ordering::Relaxed),
    ));
}

async fn do_download_task(ctx: &SyncCtx, cloud: String, local: PathBuf, est_size: u64, parallel: usize) {
    let (url, size) = match async {
        let item = ctx.client.resolve_path(&cloud).await?;
        let s = item.size.unwrap_or(0) as u64;
        let fid = item.file_id.as_deref().unwrap_or_default();
        let url = ctx.client.get_download_url(fid).await?;
        Ok::<_, Yun139Error>((url, s))
    }.await {
        Ok(v) => v,
        Err(e) => {
            let msg = e.to_string();
            tracing::error!(err = %msg, file = %cloud, "resolve failed");
            ctx.failed.fetch_add(1, Ordering::Relaxed);
            ctx.failed_files.lock().unwrap().push((format!("↓ {cloud}"), msg));
            ctx.overall_pb.inc(1);
            return;
        }
    };

    let actual = if size > 0 { size } else { est_size };
    let pb = ctx.mp.insert_before(&ctx.overall_pb, ProgressBar::new(actual));
    pb.set_style(ctx.task_style.clone());
    let name = cloud.rsplit('/').next().unwrap_or(&cloud);
    pb.set_prefix(format!("↓ {}", truncate_name(name, 28)));

    let pb2 = pb.clone();
    match ctx.client.download_parallel(&url, &local, parallel, move |bytes, _| {
        pb2.set_position(bytes);
    }).await {
        Ok(_) => {
            pb.finish_and_clear();
            ctx.downloaded.fetch_add(1, Ordering::Relaxed);
        }
        Err(e) => {
            let msg = e.to_string();
            pb.abandon_with_message(format!("失败: {}", truncate_name(&msg, 40)));
            ctx.failed.fetch_add(1, Ordering::Relaxed);
            ctx.failed_files.lock().unwrap().push((format!("↓ {cloud}"), msg));
        }
    }
    ctx.overall_pb.inc(1);
    ctx.overall_pb.set_message(format!(
        "↑{} ↓{}", ctx.uploaded.load(Ordering::Relaxed), ctx.downloaded.load(Ordering::Relaxed),
    ));
}

// ── 递归收集云盘删除 ──

async fn collect_cloud_deletes(
    client: &Yun139Client,
    cloud_dir: &str,
    pending: &TokioMutex<Vec<SyncAction>>,
) {
    let items = match client.list_all_quiet(cloud_dir).await {
        Ok(items) => items,
        Err(_) => return,
    };
    for item in &items {
        let child = format!("{}/{}", cloud_dir.trim_end_matches('/'), item.name);
        if item.is_folder {
            Box::pin(collect_cloud_deletes(client, &child, pending)).await;
        }
        pending.lock().await.push(SyncAction::DeleteCloud { cloud: child });
    }
}

// ── Yun139Client 辅助 ──

impl Yun139Client {
    async fn list_all_quiet(&self, cloud_dir: &str) -> Result<Vec<ListItem>> {
        match self.list_all(cloud_dir).await {
            Ok(r) => Ok(r.items),
            Err(_) => Ok(Vec::new()),
        }
    }
}

// ── 工具函数 ──

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
