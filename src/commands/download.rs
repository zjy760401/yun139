//! 下载命令 — 流式下载文件到本地。
//!
//! 提供两种下载模式：
//! - `download_single`: 单流下载（适合小文件或不支持 Range 的场景）
//! - `download_parallel`: 并行分片下载（大文件，8MB 分片 + 并发 Range 请求）
//!
//! 内部自动重试（每片最多 3 次）。

use std::sync::Arc;

use reqwest::Client;

use crate::error::{Result, Yun139Error};
use crate::Yun139Client;

/// 分片大小 8MB（libcloud 默认值）
const CHUNK_SIZE: u64 = 8 * 1024 * 1024;
/// 单个请求最大重试次数
const MAX_RETRIES: u32 = 3;

impl Yun139Client {
    /// 单流下载文件到本地（带重试和进度回调）。
    ///
    /// 适用于小文件或不需要并行加速的场景。
    pub async fn download_single(
        &self,
        download_url: &str,
        local_path: &std::path::Path,
        on_progress: impl Fn(u64, Option<u64>) + Send + Sync,
    ) -> Result<u64> {
        if let Some(parent) = local_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let total = probe_content_length(self.transfer_http(), download_url).await;
        on_progress(0, total);

        stream_download(self.transfer_http(), download_url, local_path, total, &on_progress).await
    }

    /// 并行分片下载文件到本地。
    ///
    /// 自动探测文件大小和 Range 支持；若不支持 Range 或文件 < 8MB，自动回退到单流。
    ///
    /// # 参数
    /// - `download_url`: CDN 下载地址
    /// - `local_path`: 本地保存路径
    /// - `parallel`: 并发分片数（推荐 4~8）
    /// - `on_progress`: 进度回调 `fn(bytes_written, total_size)`
    pub async fn download_parallel(
        &self,
        download_url: &str,
        local_path: &std::path::Path,
        parallel: usize,
        on_progress: impl Fn(u64, Option<u64>) + Send + Sync + 'static,
    ) -> Result<u64> {
        let parallel = parallel.max(1);

        if let Some(parent) = local_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let probe = self
            .transfer_http()
            .get(download_url)
            .header("Range", "bytes=0-0")
            .send()
            .await?;

        let (supports_range, total) = if probe.status() == reqwest::StatusCode::PARTIAL_CONTENT {
            let cr = probe
                .headers()
                .get("content-range")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            let size = cr.rsplit('/').next().and_then(|s| s.parse::<u64>().ok());
            drop(probe);
            (true, size)
        } else {
            let cl = probe.content_length();
            drop(probe);
            (false, cl)
        };

        tracing::info!(size = ?total, range = supports_range, dest = %local_path.display(), "downloading");
        on_progress(0, total);

        // 回退到单流
        if !supports_range || total.map_or(true, |t| t < CHUNK_SIZE) {
            return stream_download(self.transfer_http(), download_url, local_path, total, &on_progress).await;
        }

        let file_size = total.unwrap();
        let on_progress = Arc::new(on_progress) as Arc<dyn Fn(u64, Option<u64>) + Send + Sync>;
        chunked_download(self.transfer_http(), download_url, local_path, file_size, parallel, on_progress).await
    }
}

// ── 内部实现 ──

async fn probe_content_length(http: &Client, url: &str) -> Option<u64> {
    http.head(url).send().await.ok().and_then(|r| r.content_length())
}

/// 并行分片下载核心逻辑。
async fn chunked_download(
    http: &Client,
    url: &str,
    path: &std::path::Path,
    file_size: u64,
    parallel: usize,
    on_progress: Arc<dyn Fn(u64, Option<u64>) + Send + Sync>,
) -> Result<u64> {
    use std::sync::atomic::AtomicU64;

    let file = tokio::fs::File::create(path).await?;
    file.set_len(file_size).await?;
    drop(file);

    let written_total = Arc::new(AtomicU64::new(0));
    let total = Some(file_size);

    let mut chunks = Vec::new();
    let mut offset: u64 = 0;
    while offset < file_size {
        let end = (offset + CHUNK_SIZE).min(file_size) - 1;
        chunks.push((offset, end));
        offset = end + 1;
    }

    tracing::debug!(chunks = chunks.len(), parallel, "parallel download");

    let sem = Arc::new(tokio::sync::Semaphore::new(parallel));
    let mut handles = Vec::with_capacity(chunks.len());

    for (start, end) in chunks {
        let sem = sem.clone();
        let http = http.clone();
        let url = url.to_string();
        let path = path.to_path_buf();
        let written_total = written_total.clone();
        let on_progress = on_progress.clone();

        handles.push(tokio::spawn(async move {
            let _permit = sem.acquire().await.unwrap();
            let mut last_err = None;
            for attempt in 1..=MAX_RETRIES {
                // 记录尝试前的全局计数，失败时回退
                let before = written_total.load(std::sync::atomic::Ordering::Relaxed);
                match range_write_with_progress(&http, &url, &path, start, end, &written_total, total, &*on_progress).await {
                    Ok(n) => return Ok(n),
                    Err(e) => {
                        // 回退本次部分写入的字节计数
                        let after = written_total.load(std::sync::atomic::Ordering::Relaxed);
                        if after > before {
                            written_total.fetch_sub(after - before, std::sync::atomic::Ordering::Relaxed);
                        }
                        tracing::warn!(range = %format!("{start}-{end}"), attempt, err = %e, "chunk retry");
                        last_err = Some(e);
                        tokio::time::sleep(std::time::Duration::from_millis(500 * attempt as u64)).await;
                    }
                }
            }
            Err(last_err.unwrap())
        }));
    }

    let mut total_written: u64 = 0;
    for h in handles {
        let n = h.await.map_err(|e| Yun139Error::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))??;
        total_written += n;
    }

    if total_written != file_size {
        return Err(Yun139Error::Io(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            format!("size mismatch: expected {file_size}, got {total_written}"),
        )));
    }

    on_progress(total_written, total);
    tracing::info!(bytes = total_written, "download complete");
    Ok(total_written)
}

/// 下载一个 Range 分片并写入文件对应偏移，实时更新全局进度。
async fn range_write_with_progress(
    http: &Client,
    url: &str,
    path: &std::path::Path,
    start: u64,
    end: u64,
    written_total: &std::sync::atomic::AtomicU64,
    total: Option<u64>,
    on_progress: &(dyn Fn(u64, Option<u64>) + Send + Sync),
) -> Result<u64> {
    use futures_util::StreamExt;
    use std::sync::atomic::Ordering;
    use tokio::io::{AsyncSeekExt, AsyncWriteExt};

    let resp = http.get(url).header("Range", format!("bytes={start}-{end}")).send().await?.error_for_status()?;
    let mut file = tokio::fs::OpenOptions::new().write(true).open(path).await?;
    file.seek(std::io::SeekFrom::Start(start)).await?;

    let mut stream = resp.bytes_stream();
    let mut written: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        written += chunk.len() as u64;
        let global = written_total.fetch_add(chunk.len() as u64, Ordering::Relaxed) + chunk.len() as u64;
        on_progress(global, total);
    }
    file.flush().await?;
    Ok(written)
}

/// 单流下载（带重试）。
async fn stream_download(
    http: &Client,
    url: &str,
    path: &std::path::Path,
    total: Option<u64>,
    on_progress: &impl Fn(u64, Option<u64>),
) -> Result<u64> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    let mut last_err = None;
    for attempt in 1..=MAX_RETRIES {
        let resp = match http.get(url).send().await {
            Ok(r) => match r.error_for_status() {
                Ok(r) => r,
                Err(e) => { last_err = Some(Yun139Error::Http(e)); retry_sleep(attempt).await; continue; }
            },
            Err(e) => { last_err = Some(Yun139Error::Http(e)); retry_sleep(attempt).await; continue; }
        };

        let mut file = tokio::fs::File::create(path).await?;
        let mut stream = resp.bytes_stream();
        let mut written: u64 = 0;
        let mut failed = false;

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(data) => { file.write_all(&data).await?; written += data.len() as u64; on_progress(written, total); }
                Err(e) => { last_err = Some(Yun139Error::Http(e)); failed = true; break; }
            }
        }

        if failed { retry_sleep(attempt).await; continue; }
        file.flush().await?;
        tracing::info!(bytes = written, "download complete");
        return Ok(written);
    }

    Err(last_err.unwrap_or_else(|| Yun139Error::Io(std::io::Error::new(std::io::ErrorKind::Other, "download failed after retries"))))
}

async fn retry_sleep(attempt: u32) {
    tokio::time::sleep(std::time::Duration::from_millis(500 * attempt as u64)).await;
}
