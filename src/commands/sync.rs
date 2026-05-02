//! 同步命令 — 本地目录与云盘目录流水线并行同步。
//!
//! **流水线模型**:
//!
//! ```text
//!   Stage 1: Walker（单协程，BFS 广度优先）
//!     - 遍历本地目录树，每层: read_local + list_cloud
//!     - 对比后将任务分发到 Stage 2
//!
//!   Stage 2: Workers（多协程，JoinSet）
//!     - mkdir: 并行创建云盘目录
//!     - hash+upload/download: 先 SHA256 对比再传输
//!     - 直接 upload/download: 无需对比，直接传输
//!
//!   global_sem(P): 唯一并发控制，Walker 调 list API 时短暂持有，
//!                  Workers 在执行任务时持有。
//! ```

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use tokio::sync::Mutex as TokioMutex;

use crate::commands::list::ListItem;
use crate::config::default_parallel;
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
    /// 仅上传（跳过下载）
    pub upload_only: bool,
    /// 仅下载（跳过上传）
    pub download_only: bool,
    /// 对同名同大小的文件做 SHA256 校验（默认 false）。
    ///
    /// 默认行为（false）：同名同大小时用 **mtime 比较**决定是否跳过
    ///   - local_mtime ≤ cloud_updated_at → skip（本地未改动）
    ///   - local_mtime > cloud_updated_at → 重新上传（本地比云端新）
    ///
    /// `--checksum`（true）：仍走 SHA256 精确对比（慢但绝对准确）。
    pub checksum: bool,
}

impl Default for SyncOptions {
    fn default() -> Self {
        Self { delete: false, concurrency: default_parallel(), upload_only: false, download_only: false, checksum: false }
    }
}

impl SyncOptions {
    pub fn with_delete(mut self, v: bool) -> Self { self.delete = v; self }
    pub fn with_concurrency(mut self, n: usize) -> Self { self.concurrency = n.max(1); self }
    pub fn with_upload_only(mut self, v: bool) -> Self { self.upload_only = v; self }
    pub fn with_download_only(mut self, v: bool) -> Self { self.download_only = v; self }
    pub fn with_checksum(mut self, v: bool) -> Self { self.checksum = v; self }
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

struct SyncCtx {
    client: Yun139Client,
    direction: SyncDirection,
    delete: bool,
    upload_only: bool,
    download_only: bool,
    /// 是否对同名同大小的文件做 SHA256 校验（对应 --checksum flag）
    checksum: bool,
    cloud_root: String,
    parallel: usize,
    exclude: Vec<String>,

    // Worker JoinSet
    join_set: TokioMutex<tokio::task::JoinSet<()>>,

    // 网络上传/下载并发控制（同时也给 Walker list 短暂使用）
    global_sem: Arc<tokio::sync::Semaphore>,

    // SHA256 计算并发控制（独立于 global_sem，防止外置硬盘 IO 争抢）
    //
    // 外置 HDD/SSD 顺序读吞吐远优于随机并发读。限制同时计算 SHA256 的
    // 文件数量可显著减少 IO 争抢，避免 File::open 因资源耗尽而静默失败。
    // 推荐值 4（对外置 USB 盘友好，也不让 CPU 闲置）。
    hash_sem: Arc<tokio::sync::Semaphore>,

    // 延迟删除
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

    let exclude = crate::config::Config::load()
        .map(|c| c.exclude)
        .unwrap_or_else(|_| crate::config::default_exclude());

    let ctx = Arc::new(SyncCtx {
        client: client.clone(),
        direction,
        delete: opts.delete,
        upload_only: opts.upload_only,
        download_only: opts.download_only,
        checksum: opts.checksum,
        cloud_root: ct.clone(),
        parallel: p,
        exclude,
        join_set: TokioMutex::new(tokio::task::JoinSet::new()),
        global_sem: Arc::new(tokio::sync::Semaphore::new(p)),
        // 外置盘最多 4 个文件同时计算 SHA256；内置盘也不必超过 8
        hash_sem: Arc::new(tokio::sync::Semaphore::new(p.min(4))),
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

    // ── Stage 1: Walker（BFS 广度优先遍历） ──
    bfs_walk(ctx.clone(), local_root.to_path_buf(), ct.clone()).await;

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

    // ── drain JoinSet 中剩余 Worker 任务 ──
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

// ── Stage 1: BFS Walker ──

/// BFS 待扫描目录项
struct WalkItem {
    local_dir: PathBuf,
    cloud_dir: String,
    prefix: String,
    /// 父目录是新目录 → 跳过云端 list，所有文件直接上传/下载
    local_only: bool,
}

/// 广度优先遍历目录树。单协程执行，每层目录：
/// 1. 获取 global_sem permit → list 云端 + 读本地
/// 2. 释放 permit
/// 3. 对比结果，分发任务到 JoinSet（mkdir / hash+upload / upload / download）
/// 4. 子目录加入 BFS 队列
async fn bfs_walk(ctx: Arc<SyncCtx>, local_root: PathBuf, cloud_root: String) {
    let mut queue: VecDeque<WalkItem> = VecDeque::new();
    queue.push_back(WalkItem {
        local_dir: local_root,
        cloud_dir: cloud_root,
        prefix: String::new(),
        local_only: false,
    });

    while let Some(item) = queue.pop_front() {
        ctx.scan_pb.set_message(if item.prefix.is_empty() {
            "/".to_string()
        } else {
            truncate_name(&item.prefix, 50)
        });

        // ── 获取 permit → list ──
        let list_start = std::time::Instant::now();
        let _permit = ctx.global_sem.acquire().await.unwrap();

        let ld = item.local_dir.clone();
        let excl = ctx.exclude.clone();
        let local_handle = tokio::task::spawn_blocking(move || read_local_dir(&ld, &excl));

        let cloud_items = if item.local_only {
            Vec::new()
        } else {
            ctx.client.list_all_quiet(&item.cloud_dir).await.unwrap_or_default()
        };

        let local_entries = local_handle.await.unwrap_or_default();

        // 释放 permit — list 完成，后续分发不需要持有
        drop(_permit);

        let list_elapsed = list_start.elapsed();
        tracing::info!(
            dir = %item.prefix, local_count = local_entries.len(), cloud_count = cloud_items.len(),
            list_ms = list_elapsed.as_millis() as u64,
            "scan_dir listed"
        );

        // ── 对比 + 分发 ──
        let local_map: HashMap<&str, &LocalEntry> =
            local_entries.iter().map(|e| (e.name.as_str(), e)).collect();
        let cloud_map: HashMap<&str, &ListItem> =
            cloud_items.iter().map(|e| (e.name.as_str(), e)).collect();

        let ct = &ctx.cloud_root;

        match ctx.direction {
            SyncDirection::LocalToCloud => {
                walk_local_to_cloud(
                    &ctx, &item, ct, &local_entries, &cloud_items,
                    &local_map, &cloud_map, &mut queue,
                ).await;
            }
            SyncDirection::CloudToLocal => {
                walk_cloud_to_local(
                    &ctx, &item, ct, &local_entries, &cloud_items,
                    &local_map, &cloud_map, &mut queue,
                ).await;
            }
        }
    }
}

/// LocalToCloud: 对比本地 vs 云端，分发 mkdir / upload / hash+upload
async fn walk_local_to_cloud(
    ctx: &Arc<SyncCtx>,
    item: &WalkItem,
    ct: &str,
    local_entries: &[LocalEntry],
    cloud_items: &[ListItem],
    local_map: &HashMap<&str, &LocalEntry>,
    cloud_map: &HashMap<&str, &ListItem>,
    queue: &mut VecDeque<WalkItem>,
) {
    struct FileInfo { local: PathBuf, cloud_dir: String }
    struct HashCheck { local: PathBuf, cloud_dir: String, cloud_hash: String }

    let mut to_upload: Vec<FileInfo> = Vec::new();
    let mut to_hash_check: Vec<HashCheck> = Vec::new();
    let mut skip_count: u32 = 0;

    // 收集需要创建的新目录
    struct DirInfo { name: String, cloud_path: String, is_new: bool }
    let mut dirs: Vec<DirInfo> = Vec::new();

    for le in local_entries.iter().filter(|e| e.is_dir) {
        let cloud_path = rel_cloud(ct, &item.prefix, &le.name);
        let is_new = !cloud_map.contains_key(le.name.as_str());
        dirs.push(DirInfo { name: le.name.clone(), cloud_path, is_new });
    }

    // 文件对比
    for le in local_entries.iter().filter(|e| !e.is_dir) {
        match cloud_map.get(le.name.as_str()) {
            None => {
                to_upload.push(FileInfo {
                    local: item.local_dir.join(&le.name),
                    cloud_dir: item.cloud_dir.clone(),
                });
            }
            Some(ci) if ci.size != le.size as i64 => {
                let cloud_mtime = parse_cloud_mtime_ms(&ci.updated_at);
                if le.mtime_ms >= cloud_mtime {
                    to_upload.push(FileInfo {
                        local: item.local_dir.join(&le.name),
                        cloud_dir: item.cloud_dir.clone(),
                    });
                } else {
                    tracing::debug!(file = %le.name, "skip: cloud is newer");
                    skip_count += 1;
                }
            }
            Some(ci) => {
                // 同名同大小：按 checksum 模式决定处理方式
                if ctx.checksum && !ci.content_hash.is_empty() {
                    // --checksum 模式：做 SHA256 精确对比
                    to_hash_check.push(HashCheck {
                        local: item.local_dir.join(&le.name),
                        cloud_dir: item.cloud_dir.clone(),
                        cloud_hash: ci.content_hash.clone(),
                    });
                } else {
                    // 默认模式（mtime 比较）：
                    //   本地 mtime > 云端 updated_at → 本地文件比云端新 → 重新上传
                    //   否则 → 跳过（文件未改动）
                    let cloud_mtime = parse_cloud_mtime_ms(&ci.updated_at);
                    if le.mtime_ms > cloud_mtime {
                        tracing::debug!(file = %le.name, local_mtime = le.mtime_ms, cloud_mtime, "mtime newer → upload");
                        to_upload.push(FileInfo {
                            local: item.local_dir.join(&le.name),
                            cloud_dir: item.cloud_dir.clone(),
                        });
                    } else {
                        tracing::debug!(file = %le.name, "mtime match → skip (no checksum)");
                        skip_count += 1;
                    }
                }
            }
        }
    }

    tracing::info!(
        dir = %item.prefix, to_upload = to_upload.len(), to_hash = to_hash_check.len(),
        skipped = skip_count, sub_dirs = dirs.len(),
        "scan_dir decision"
    );

    // 更新进度条
    if ctx.download_only {
        ctx.skipped.fetch_add(to_upload.len() as u32, Ordering::Relaxed);
        to_upload.clear();
    }
    if !to_upload.is_empty() {
        ctx.overall_pb.inc_length(to_upload.len() as u64);
    }
    ctx.skipped.fetch_add(skip_count, Ordering::Relaxed);

    // 分发 hash 检查任务（只在 --checksum 模式下非空）
    if !ctx.download_only {
        for hc in to_hash_check {
            push_hash_then_upload(ctx, hc.local, hc.cloud_dir, hc.cloud_hash).await;
        }
    } else {
        ctx.skipped.fetch_add(to_hash_check.len() as u32, Ordering::Relaxed);
    }

    // 并行创建新目录
    let new_dir_paths: Vec<&DirInfo> = dirs.iter().filter(|d| d.is_new).collect();
    if !new_dir_paths.is_empty() {
        ctx.scan_pb.set_message(format!("📁 creating {} dirs", new_dir_paths.len()));
        let mkdir_futures: Vec<_> = new_dir_paths.iter().map(|d| {
            let client = ctx.client.clone();
            let path = d.cloud_path.clone();
            async move { (client.ensure_dir(&path).await, path) }
        }).collect();
        let results = futures_util::future::join_all(mkdir_futures).await;
        for (result, path) in results {
            match result {
                Ok(_) => { ctx.dirs_created.fetch_add(1, Ordering::Relaxed); }
                Err(e) => {
                    tracing::error!(err = %e, dir = %path, "mkdir cloud failed");
                    ctx.failed.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }

    // 子目录加入 BFS 队列
    for d in &dirs {
        queue.push_back(WalkItem {
            local_dir: item.local_dir.join(&d.name),
            cloud_dir: d.cloud_path.clone(),
            prefix: rel_path(&item.prefix, &d.name),
            local_only: d.is_new,
        });
    }

    // 分发上传任务
    for f in to_upload {
        push_upload(ctx, f.local, f.cloud_dir).await;
    }

    // 收集删除
    if ctx.delete {
        for ci in cloud_items {
            if !local_map.contains_key(ci.name.as_str()) {
                let cloud = rel_cloud(ct, &item.prefix, &ci.name);
                if ci.is_folder {
                    collect_cloud_deletes(&ctx.client, &cloud, &ctx.pending_deletes).await;
                }
                ctx.pending_deletes.lock().await.push(SyncAction::DeleteCloud { cloud });
            }
        }
    }
}

/// CloudToLocal: 对比云端 vs 本地，分发 mkdir / download / hash+download
async fn walk_cloud_to_local(
    ctx: &Arc<SyncCtx>,
    item: &WalkItem,
    ct: &str,
    local_entries: &[LocalEntry],
    cloud_items: &[ListItem],
    local_map: &HashMap<&str, &LocalEntry>,
    cloud_map: &HashMap<&str, &ListItem>,
    queue: &mut VecDeque<WalkItem>,
) {
    struct FileInfo { cloud: String, local: PathBuf, size: u64 }
    struct HashCheck { cloud: String, local: PathBuf, size: u64, cloud_hash: String }

    let mut to_download: Vec<FileInfo> = Vec::new();
    let mut to_hash_check: Vec<HashCheck> = Vec::new();
    let mut skip_count: u32 = 0;

    struct DirInfo { name: String, is_new: bool }
    let mut dirs: Vec<DirInfo> = Vec::new();

    for ci in cloud_items.iter().filter(|e| e.is_folder) {
        let is_new = !local_map.contains_key(ci.name.as_str());
        dirs.push(DirInfo { name: ci.name.clone(), is_new });
    }

    for ci in cloud_items.iter().filter(|e| !e.is_folder) {
        match local_map.get(ci.name.as_str()) {
            None => {
                to_download.push(FileInfo {
                    cloud: rel_cloud(ct, &item.prefix, &ci.name),
                    local: item.local_dir.join(&ci.name),
                    size: ci.size as u64,
                });
            }
            Some(le) if le.size as i64 != ci.size => {
                let cloud_mtime = parse_cloud_mtime_ms(&ci.updated_at);
                if cloud_mtime >= le.mtime_ms {
                    to_download.push(FileInfo {
                        cloud: rel_cloud(ct, &item.prefix, &ci.name),
                        local: item.local_dir.join(&ci.name),
                        size: ci.size as u64,
                    });
                } else {
                    tracing::debug!(file = %ci.name, "skip: local is newer");
                    skip_count += 1;
                }
            }
            Some(_le) => {
                // 同名同大小：按 checksum 模式决定处理方式
                if ctx.checksum && !ci.content_hash.is_empty() {
                    // --checksum 模式：做 SHA256 精确对比
                    to_hash_check.push(HashCheck {
                        cloud: rel_cloud(ct, &item.prefix, &ci.name),
                        local: item.local_dir.join(&ci.name),
                        size: ci.size as u64,
                        cloud_hash: ci.content_hash.clone(),
                    });
                } else {
                    // 默认模式（mtime 比较）：
                    //   云端 updated_at > 本地 mtime → 云端比本地新 → 重新下载
                    //   否则 → 跳过
                    let cloud_mtime = parse_cloud_mtime_ms(&ci.updated_at);
                    if cloud_mtime > _le.mtime_ms {
                        tracing::debug!(file = %ci.name, cloud_mtime, local_mtime = _le.mtime_ms, "cloud mtime newer → download");
                        to_download.push(FileInfo {
                            cloud: rel_cloud(ct, &item.prefix, &ci.name),
                            local: item.local_dir.join(&ci.name),
                            size: ci.size as u64,
                        });
                    } else {
                        tracing::debug!(file = %ci.name, "mtime match → skip (no checksum)");
                        skip_count += 1;
                    }
                }
            }
        }
    }

    // 更新进度条
    if ctx.upload_only {
        ctx.skipped.fetch_add(to_download.len() as u32, Ordering::Relaxed);
        to_download.clear();
    }
    if !to_download.is_empty() {
        ctx.overall_pb.inc_length(to_download.len() as u64);
    }
    ctx.skipped.fetch_add(skip_count, Ordering::Relaxed);

    // 分发 hash 检查任务（只在 --checksum 模式下非空）
    if !ctx.upload_only {
        let parallel = ctx.parallel;
        for hc in to_hash_check {
            push_hash_then_download(ctx, hc.cloud, hc.local, hc.size, hc.cloud_hash, parallel).await;
        }
    } else {
        ctx.skipped.fetch_add(to_hash_check.len() as u32, Ordering::Relaxed);
    }

    // 创建本地目录（本地 IO 很快，直接串行即可）
    for d in &dirs {
        if d.is_new {
            let lp = item.local_dir.join(&d.name);
            ctx.scan_pb.set_message(format!("📁 {}", lp.display()));
            match tokio::fs::create_dir_all(&lp).await {
                Ok(_) => { ctx.dirs_created.fetch_add(1, Ordering::Relaxed); }
                Err(e) => {
                    tracing::error!(err = %e, dir = %lp.display(), "mkdir local failed");
                    ctx.failed.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }

    // 子目录加入 BFS 队列
    for d in &dirs {
        queue.push_back(WalkItem {
            local_dir: item.local_dir.join(&d.name),
            cloud_dir: rel_cloud(ct, &item.prefix, &d.name),
            prefix: rel_path(&item.prefix, &d.name),
            local_only: false,
        });
    }

    // 分发下载任务
    for f in to_download {
        push_download(ctx, f.cloud, f.local, f.size).await;
    }

    // 收集删除
    if ctx.delete {
        for le in local_entries {
            if !cloud_map.contains_key(le.name.as_str()) {
                ctx.pending_deletes.lock().await.push(SyncAction::DeleteLocal {
                    local: item.local_dir.join(&le.name),
                });
            }
        }
    }
}

// ── Stage 2: Worker 任务分发 ──
//
// 并发模型（两个独立信号量）：
//
//   hash_sem(min(parallel,4))  — 控制磁盘读取（SHA256）并发
//     外置 HDD/SSD 顺序吞吐好但并发随机读差。限 4 个文件同时做 SHA256，
//     避免 IO 争抢导致 File::open 静默失败返回空字符串。
//
//   global_sem(parallel)       — 控制网络 IO（上传/下载/list）并发
//     同时也给 Walker list 短暂使用。
//
//   每个 push_upload / push_hash_then_upload 的正确执行顺序：
//     1. acquire hash_sem  → 计算 SHA256（磁盘 IO）
//     2. release hash_sem  → 释放磁盘槽
//     3. acquire global_sem → 执行网络上传
//     4. release global_sem

async fn push_upload(ctx: &Arc<SyncCtx>, local: PathBuf, cloud_dir: String) {
    let ctx2 = ctx.clone();
    let global = ctx.global_sem.clone();
    let hash_sem = ctx.hash_sem.clone();
    let mut js = ctx.join_set.lock().await;
    js.spawn(async move {
        let name = local.file_name().unwrap_or_default().to_string_lossy().to_string();

        // Phase 1: 计算 SHA256 — 持有 hash_sem，不持有 global_sem
        let hash_start = std::time::Instant::now();
        let _hash_permit = hash_sem.acquire().await.unwrap();
        tracing::debug!(file = %name, "pre-hash start (hash_sem acquired)");
        let hash = match Yun139Client::sha256_file(&local).await {
            Ok(h) => h,
            Err(e) => {
                tracing::error!(file = %name, err = %e, "SHA256 failed, skip upload");
                ctx2.failed.fetch_add(1, Ordering::Relaxed);
                ctx2.failed_files.lock().unwrap().push((format!("↑ {}", local.display()), e.to_string()));
                ctx2.overall_pb.inc(1);
                return;
            }
        };
        let hash_ms = hash_start.elapsed().as_millis() as u64;
        drop(_hash_permit); // 立即释放磁盘槽，后续只需网络槽
        tracing::debug!(file = %name, hash_ms, "pre-hash done (hash_sem released)");

        // Phase 2: 获取网络上传槽
        tracing::debug!(file = %name, "waiting for global_sem");
        let _permit = global.acquire().await.unwrap();
        tracing::debug!(file = %name, "got global_sem, starting upload");
        do_upload_task(&ctx2, local, cloud_dir, hash).await;
    });
}

async fn push_download(ctx: &Arc<SyncCtx>, cloud: String, local: PathBuf, est_size: u64) {
    let ctx2 = ctx.clone();
    let global = ctx.global_sem.clone();
    let parallel = ctx.parallel;
    let mut js = ctx.join_set.lock().await;
    js.spawn(async move {
        let _permit = global.acquire().await.unwrap();
        do_download_task(&ctx2, cloud, local, est_size, parallel).await;
    });
}

async fn push_hash_then_upload(ctx: &Arc<SyncCtx>, local: PathBuf, cloud_dir: String, cloud_hash: String) {
    let ctx2 = ctx.clone();
    let global = ctx.global_sem.clone();
    let hash_sem = ctx.hash_sem.clone();
    let mut js = ctx.join_set.lock().await;
    js.spawn(async move {
        let name = local.file_name().unwrap_or_default().to_string_lossy().to_string();

        // Phase 1: 计算本地 SHA256 — 持有 hash_sem，不持有 global_sem
        // 用 sha256_file（async + 内部 spawn_blocking）以获得正确错误传播
        let hash_start = std::time::Instant::now();
        let _hash_permit = hash_sem.acquire().await.unwrap();
        tracing::debug!(file = %name, "hash_check: computing local SHA256 (hash_sem acquired)");
        let local_hash = match Yun139Client::sha256_file(&local).await {
            Ok(h) => h,
            Err(e) => {
                // 文件读取失败（权限/句柄耗尽等）→ 记录错误，不要用空 hash 当 mismatch
                tracing::error!(file = %name, err = %e, "hash_check SHA256 failed, skip");
                ctx2.failed.fetch_add(1, Ordering::Relaxed);
                ctx2.failed_files.lock().unwrap().push((format!("↑ {}", local.display()), format!("sha256 failed: {e}")));
                ctx2.overall_pb.inc(1);
                return;
            }
        };
        let hash_ms = hash_start.elapsed().as_millis() as u64;
        drop(_hash_permit); // 立即释放磁盘槽
        tracing::debug!(file = %name, hash_ms, local_hash = %local_hash, cloud_hash = %cloud_hash, "hash_check done (hash_sem released)");

        if local_hash != cloud_hash {
            // 哈希不一致 → 需要重新上传
            tracing::debug!(file = %name, hash_ms, "hash mismatch → upload");
            ctx2.overall_pb.inc_length(1);
            // Phase 2: 获取网络上传槽
            let _permit = global.acquire().await.unwrap();
            tracing::debug!(file = %name, "got global_sem, starting upload (hash mismatch)");
            do_upload_task(&ctx2, local, cloud_dir, local_hash).await;
        } else {
            tracing::debug!(file = %name, hash_ms, "hash match → skip ✓");
            ctx2.skipped.fetch_add(1, Ordering::Relaxed);
        }
    });
}

async fn push_hash_then_download(ctx: &Arc<SyncCtx>, cloud: String, local: PathBuf, est_size: u64, cloud_hash: String, parallel: usize) {
    let ctx2 = ctx.clone();
    let global = ctx.global_sem.clone();
    let hash_sem = ctx.hash_sem.clone();
    let mut js = ctx.join_set.lock().await;
    js.spawn(async move {
        let name = local.file_name().unwrap_or_default().to_string_lossy().to_string();

        // Phase 1: 计算本地 SHA256 — 持有 hash_sem，不持有 global_sem
        let hash_start = std::time::Instant::now();
        let _hash_permit = hash_sem.acquire().await.unwrap();
        tracing::debug!(file = %name, "hash_check: computing local SHA256 (hash_sem acquired)");
        let local_hash = match Yun139Client::sha256_file(&local).await {
            Ok(h) => h,
            Err(e) => {
                tracing::error!(file = %name, err = %e, "hash_check SHA256 failed, skip");
                ctx2.failed.fetch_add(1, Ordering::Relaxed);
                ctx2.failed_files.lock().unwrap().push((format!("↓ {cloud}"), format!("sha256 failed: {e}")));
                ctx2.overall_pb.inc(1);
                return;
            }
        };
        let hash_ms = hash_start.elapsed().as_millis() as u64;
        drop(_hash_permit);
        tracing::debug!(file = %name, hash_ms, local_hash = %local_hash, cloud_hash = %cloud_hash, "hash_check done (hash_sem released)");

        if local_hash != cloud_hash {
            // 哈希不一致 → 需要重新下载
            tracing::debug!(file = %name, hash_ms, "hash mismatch → download");
            ctx2.overall_pb.inc_length(1);
            // Phase 2: 获取网络下载槽
            let _permit = global.acquire().await.unwrap();
            tracing::debug!(file = %name, "got global_sem, starting download (hash mismatch)");
            do_download_task(&ctx2, cloud, local, est_size, parallel).await;
        } else {
            tracing::debug!(file = %name, hash_ms, "hash match → skip ✓");
            ctx2.skipped.fetch_add(1, Ordering::Relaxed);
        }
    });
}

// ── 传输任务实现 ──

/// 上传任务主体（调用方须已持有 global_sem）。
/// `prehashed` 为已在 sem 外预计算好的 SHA256。
async fn do_upload_task(ctx: &SyncCtx, local: PathBuf, cloud_dir: String, prehashed: String) {
    let file_size = tokio::fs::metadata(&local).await.map(|m| m.len()).unwrap_or(0);
    let pb = ctx.mp.insert_before(&ctx.overall_pb, ProgressBar::new(file_size));
    pb.set_style(ctx.task_style.clone());
    let name = local.file_name().unwrap_or_default().to_string_lossy().to_string();
    pb.set_prefix(format!("↑ {}", truncate_name(&name, 28)));

    tracing::debug!(file = %name, size = file_size, "do_upload_task: calling upload_file_prehashed");
    let pb2 = pb.clone();
    match ctx.client.upload_file_prehashed(&local, file_size, &prehashed, &cloud_dir, move |bytes, _| { pb2.set_position(bytes); }).await {
        Ok(_) => {
            pb.set_position(file_size);
            pb.finish_and_clear();
            ctx.uploaded.fetch_add(1, Ordering::Relaxed);
            tracing::debug!(file = %name, "upload done ✓");
        }
        Err(e) => {
            let msg = e.to_string();
            tracing::warn!(file = %name, err = %msg, "upload failed");
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
    mtime_ms: i64,
}

fn read_local_dir(dir: &Path, exclude: &[String]) -> Vec<LocalEntry> {
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
        if is_excluded(&name, exclude) { continue; }
        let mtime_ms = meta.modified().ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        entries.push(LocalEntry { name, is_dir: meta.is_dir(), size: meta.len(), mtime_ms });
    }
    entries
}

fn is_excluded(name: &str, patterns: &[String]) -> bool {
    for pat in patterns {
        if pat == ".*" {
            if name.starts_with('.') { return true; }
        } else if let Some(suffix) = pat.strip_prefix('*') {
            if name.ends_with(suffix) { return true; }
        } else if let Some(prefix) = pat.strip_suffix('*') {
            if name.starts_with(prefix) { return true; }
        } else if pat == name {
            return true;
        }
    }
    false
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

fn parse_cloud_mtime_ms(updated_at: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(updated_at)
        .or_else(|_| chrono::DateTime::parse_from_str(updated_at, "%Y-%m-%dT%H:%M:%S%.f%:z"))
        .map(|dt| dt.timestamp_millis())
        .unwrap_or(0)
}

fn compute_sha256_hex(path: &Path) -> String {
    use digest::Digest;
    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => {
            // 记录错误而不是静默返回空字符串（空字符串会被误当成 hash mismatch）
            tracing::error!(path = %path.display(), err = %e, "compute_sha256_hex: failed to open file");
            return String::new();
        }
    };
    let mut hasher = sha2::Sha256::new();
    let mut buf = vec![0u8; 2 * 1024 * 1024];
    loop {
        let n = match std::io::Read::read(&mut file, &mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                tracing::error!(path = %path.display(), err = %e, "compute_sha256_hex: read error");
                return String::new();
            }
        };
        hasher.update(&buf[..n]);
    }
    hex::encode(hasher.finalize())
}
