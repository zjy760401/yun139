//! # yun139
//!
//! 中国移动云盘 (139 网盘) 文件操作库。
//!
//! 支持多线程共享同一个 [`Yun139Client`] 并发执行不同操作。
//! 本库依赖 tokio 但不自行启动 runtime。
//!
//! ```rust,no_run
//! use yun139::Yun139Client;
//!
//! #[tokio::main]
//! async fn main() {
//!     let client = Yun139Client::new("Basic REDACTED...").unwrap();
//!
//!     // 同一个 client 可在多个 tokio::spawn 中并发使用
//!     let c1 = client.clone();
//!     let t1 = tokio::spawn(async move {
//!         c1.upload_file(std::path::Path::new("/tmp/a.bin"), "/backup", |_, _| {}).await
//!     });
//!
//!     let c2 = client.clone();
//!     let t2 = tokio::spawn(async move {
//!         c2.download_parallel("https://...", std::path::Path::new("/tmp/b.bin"), 4, |_, _| {}).await
//!     });
//!
//!     let c3 = client.clone();
//!     let t3 = tokio::spawn(async move {
//!         c3.list_all("/photos").await
//!     });
//!
//!     let _ = tokio::join!(t1, t2, t3);
//! }
//! ```

pub mod api;
pub mod client;
pub mod commands;
pub mod config;
pub mod error;
pub mod sign;

pub use client::Yun139Client;
pub use commands::list::{ListItem, ListResult};
pub use commands::search::SearchResult;
pub use commands::sync::{SyncAction, SyncDirection, SyncOptions, SyncSummary};
pub use error::{Result, Yun139Error};

// ── 便捷顶层函数（内部创建 client，适合一次性调用） ──

/// 从云盘下载文件到本地。
pub async fn download(
    authorization: &str,
    cloud_path: &str,
    local_path: &str,
    parallel: usize,
    on_progress: impl Fn(u64, Option<u64>) + Send + Sync + 'static,
) -> Result<u64> {
    let client = Yun139Client::new(authorization)?;
    client.download(cloud_path, local_path, parallel, on_progress).await
}

/// 上传本地文件到云盘。
pub async fn upload(
    authorization: &str,
    cloud_dir: &str,
    local_path: &str,
    on_progress: impl Fn(u64, u64) + Send + Sync,
) -> Result<String> {
    let client = Yun139Client::new(authorization)?;
    client.upload(local_path, cloud_dir, on_progress).await
}

/// 列出云盘目录内容。
pub async fn list(authorization: &str, cloud_dir: &str) -> Result<ListResult> {
    let client = Yun139Client::new(authorization)?;
    client.list_all(cloud_dir).await
}

/// 创建云盘目录。
pub async fn mkdir(authorization: &str, cloud_path: &str, recursive: bool) -> Result<String> {
    let client = Yun139Client::new(authorization)?;
    if recursive {
        client.mkdir_recursive(cloud_path).await
    } else {
        client.mkdir(cloud_path).await
    }
}

/// 删除云盘文件/目录（移入回收站）。
pub async fn trash(authorization: &str, cloud_path: &str) -> Result<()> {
    let client = Yun139Client::new(authorization)?;
    client.trash(cloud_path).await
}

/// 永久删除云盘文件/目录。
pub async fn delete(authorization: &str, cloud_path: &str) -> Result<()> {
    let client = Yun139Client::new(authorization)?;
    client.delete(cloud_path).await
}

/// 同步本地目录到云盘。
pub async fn sync_to_cloud(
    authorization: &str,
    local_dir: &std::path::Path,
    cloud_dir: &str,
    delete_extra: bool,
    on_progress: impl Fn(&str) + Send + Sync,
) -> Result<SyncSummary> {
    let client = Yun139Client::new(authorization)?;
    client.sync_to_cloud(local_dir, cloud_dir, delete_extra, on_progress).await
}

/// 同步云盘目录到本地。
pub async fn sync_to_local(
    authorization: &str,
    cloud_dir: &str,
    local_dir: &std::path::Path,
    delete_extra: bool,
    on_progress: impl Fn(&str) + Send + Sync,
) -> Result<SyncSummary> {
    let client = Yun139Client::new(authorization)?;
    client.sync_to_local(cloud_dir, local_dir, delete_extra, on_progress).await
}

/// 搜索云盘文件。
pub async fn search(authorization: &str, keyword: &str, limit: usize) -> Result<SearchResult> {
    let client = Yun139Client::new(authorization)?;
    client.search(keyword, limit).await
}
