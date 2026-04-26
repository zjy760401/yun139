//! 上传命令 — 分片上传文件到云盘。
//!
//! 提供单一入口 `upload_file`，内部自动判断策略：
//! - ≤ 10MB: 单次 PUT 上传（不分片）
//! - > 10MB: 分片上传（100MB/片，按序 PUT）
//!
//! 流程（与 libcloud.dylib 对齐）：
//!   1. SHA256 → /file/create（秒传检测）
//!   2. /file/getUploadUrl
//!   3. PUT 分片到 OSS
//!   4. /file/complete

use crate::api::*;
use crate::error::{Result, Yun139Error};
use crate::Yun139Client;

const MAX_RETRIES: u32 = 5;
/// ≤ 此值走单次上传，> 此值走分片
const SMALL_FILE_THRESHOLD: u64 = 10 * 1024 * 1024;

impl Yun139Client {
    /// 上传本地文件到云盘指定目录。
    ///
    /// 内部自动选择策略：≤ 10MB 单次上传，> 10MB 分片上传。
    ///
    /// # 参数
    /// - `local_path`: 本地文件路径
    /// - `cloud_dir`: 云盘目标目录路径，如 `/test`，`/` 表示根目录
    /// - `on_progress`: 进度回调 `fn(bytes_uploaded, total_size)`
    ///
    /// # 返回
    /// 上传后的 fileId。
    pub async fn upload_file(
        &self,
        local_path: &std::path::Path,
        cloud_dir: &str,
        on_progress: impl Fn(u64, u64) + Send + Sync,
    ) -> Result<String> {
        let metadata = tokio::fs::metadata(local_path).await?;
        let file_size = metadata.len();
        let file_name = local_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| {
                Yun139Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "invalid file name",
                ))
            })?
            .to_string();

        if file_size == 0 {
            return Err(Yun139Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "cannot upload empty file (0 bytes)",
            )));
        }

        // 1. SHA256
        tracing::info!(file = %local_path.display(), size = file_size, "computing SHA256");
        let content_hash = compute_sha256(local_path).await?;
        tracing::debug!(hash = %content_hash, "SHA256 done");

        // 2. 确保目标目录存在
        let parent_file_id = self.ensure_dir(cloud_dir).await?;
        let host = self.personal_host().await?;

        // 3. 分片参数
        let part_size = calc_part_size(file_size);
        let part_count = ((file_size as i64 + part_size - 1) / part_size) as usize;

        let first_batch: Vec<serde_json::Value> = (0..part_count.min(100))
            .map(|i| {
                let start = i as i64 * part_size;
                let byte_size = (file_size as i64 - start).min(part_size);
                serde_json::json!({
                    "partNumber": (i + 1) as i32,
                    "partSize": byte_size,
                    "parallelHashCtx": { "partOffset": start }
                })
            })
            .collect();

        // 4. /file/create
        let create_url = format!("{}/file/create", host);
        let create_body = serde_json::json!({
            "contentHash": content_hash,
            "contentHashAlgorithm": "SHA256",
            "contentType": guess_content_type(&file_name),
            "parallelUpload": false,
            "partInfos": first_batch,
            "size": file_size,
            "parentFileId": parent_file_id,
            "name": file_name,
            "type": "file",
            "fileRenameMode": "overwrite"
        });

        tracing::info!("creating upload task");
        let create_resp: UploadCreateResp = self.post_checked(&create_url, &create_body).await?;

        let data = match create_resp.data {
            Some(d) => d,
            None => {
                tracing::info!("rapid upload (no data returned)");
                on_progress(file_size, file_size);
                return Ok(String::new());
            }
        };

        let file_id = data.file_id.unwrap_or_default();
        let upload_id = data.upload_id.unwrap_or_default();

        // 秒传检测
        if data.rapid_upload.unwrap_or(false)
            || upload_id.is_empty()
            || data.part_infos.as_ref().map_or(false, |p| p.is_empty())
        {
            tracing::info!(file_id = %file_id, "rapid upload success");
            on_progress(file_size, file_size);
            return Ok(file_id);
        }

        // 5. 获取上传 URL
        tracing::info!(parts = part_count, part_size, "fetching upload URLs");
        let upload_urls = fetch_all_upload_urls(self, host, &file_id, &upload_id, part_count, part_size, file_size as i64).await?;

        // 6. 上传分片
        let oss_client = self.transfer_http().clone();
        on_progress(0, file_size);

        if file_size <= SMALL_FILE_THRESHOLD {
            // 小文件：单次读取 + 单次 PUT
            tracing::info!("small file, single PUT");
            let buf = tokio::fs::read(local_path).await?;
            let url = upload_urls.get(&1).ok_or_else(|| Yun139Error::Api {
                code: "NO_URL".into(),
                message: "no upload URL for part 1".into(),
            })?;
            put_bytes_with_retry(&oss_client, url, &buf, 1).await?;
            on_progress(file_size, file_size);
        } else {
            // 大文件：并行分片上传
            use std::sync::atomic::{AtomicU64, Ordering};
            use std::sync::Arc;

            const UPLOAD_PARALLEL: usize = 2;
            tracing::info!(parts = part_count, parallel = UPLOAD_PARALLEL, "uploading parts in parallel");

            let uploaded_total = Arc::new(AtomicU64::new(0));
            let sem = Arc::new(tokio::sync::Semaphore::new(UPLOAD_PARALLEL));
            let local_path = local_path.to_path_buf();
            let mut handles = Vec::with_capacity(part_count);

            for i in 0..part_count {
                let offset = i as u64 * part_size as u64;
                let read_size = ((file_size - offset) as i64).min(part_size) as usize;
                let part_number = (i + 1) as i32;
                let url = upload_urls.get(&part_number).ok_or_else(|| Yun139Error::Api {
                    code: "NO_URL".into(),
                    message: format!("no upload URL for part {part_number}"),
                })?.clone();

                let sem = sem.clone();
                let oss = oss_client.clone();
                let path = local_path.clone();
                let uploaded_total = uploaded_total.clone();

                handles.push(tokio::spawn(async move {
                    let _permit = sem.acquire().await.unwrap();
                    stream_put_with_retry(&oss, &url, &path, offset, read_size, part_number).await?;
                    uploaded_total.fetch_add(read_size as u64, Ordering::Relaxed);
                    Ok::<u64, Yun139Error>(read_size as u64)
                }));
            }

            for h in handles {
                h.await.map_err(|e| Yun139Error::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))??;
                let current = uploaded_total.load(Ordering::Relaxed);
                on_progress(current, file_size);
            }
        }

        // 7. /file/complete
        tracing::info!("completing upload");
        let complete_url = format!("{}/file/complete", host);
        let complete_body = serde_json::json!({
            "contentHash": content_hash,
            "contentHashAlgorithm": "SHA256",
            "uploadId": upload_id,
            "fileId": file_id,
        });
        let _: UploadCompleteResp = self.post_checked(&complete_url, &complete_body).await?;

        tracing::info!(file_id = %file_id, "upload complete");
        Ok(file_id)
    }
}

// ── 内部实现 ──

async fn compute_sha256(path: &std::path::Path) -> Result<String> {
    let path = path.to_path_buf();
    let hash = tokio::task::spawn_blocking(move || {
        use digest::Digest;
        let mut file = std::fs::File::open(&path)?;
        let mut hasher = sha2::Sha256::new();
        let mut buf = vec![0u8; 2 * 1024 * 1024];
        loop {
            let n = std::io::Read::read(&mut file, &mut buf)?;
            if n == 0 { break; }
            hasher.update(&buf[..n]);
        }
        Ok::<String, std::io::Error>(hex::encode(hasher.finalize()))
    })
    .await
    .map_err(|e| Yun139Error::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))??;
    Ok(hash)
}

async fn fetch_all_upload_urls(
    client: &Yun139Client,
    host: &str,
    file_id: &str,
    upload_id: &str,
    part_count: usize,
    part_size: i64,
    file_size: i64,
) -> Result<std::collections::HashMap<i32, String>> {
    let mut urls = std::collections::HashMap::new();
    let url = format!("{}/file/getUploadUrl", host);

    for batch_start in (0..part_count).step_by(100) {
        let batch_end = (batch_start + 100).min(part_count);
        let part_infos: Vec<serde_json::Value> = (batch_start..batch_end)
            .map(|i| {
                let start = i as i64 * part_size;
                let byte_size = (file_size - start).min(part_size);
                serde_json::json!({ "partNumber": (i + 1) as i32, "partSize": byte_size })
            })
            .collect();

        let body = serde_json::json!({ "fileId": file_id, "uploadId": upload_id, "partInfos": part_infos });
        let resp: GetUploadUrlResp = client.post_checked(&url, &body).await?;
        if let Some(data) = resp.data {
            if let Some(infos) = data.part_infos {
                for info in infos {
                    if let Some(u) = info.upload_url { urls.insert(info.part_number, u); }
                }
            }
        }
    }
    Ok(urls)
}

/// 小文件：整块 bytes PUT（带重试 + 指数退避）。
async fn put_bytes_with_retry(oss: &reqwest::Client, url: &str, buf: &[u8], part: i32) -> Result<()> {
    let mut last_err = None;
    for attempt in 1..=MAX_RETRIES {
        match oss.put(url).header("Content-Type", "application/octet-stream").header("Content-Length", buf.len().to_string()).body(buf.to_vec()).send().await {
            Ok(r) if r.status().is_success() => return Ok(()),
            Ok(r) => { let s = r.status(); let b = r.text().await.unwrap_or_default(); tracing::warn!(part, attempt, status = %s, "PUT failed"); last_err = Some(Yun139Error::Api { code: s.as_u16().to_string(), message: b }); }
            Err(e) => { tracing::warn!(part, attempt, err = %e, "PUT error"); last_err = Some(Yun139Error::Http(e)); }
        }
        let delay = std::time::Duration::from_secs(1 << attempt.min(4));
        tokio::time::sleep(delay).await;
    }
    Err(last_err.unwrap())
}

/// 大文件分片：流式 PUT（带重试 + 指数退避）。
async fn stream_put_with_retry(oss: &reqwest::Client, url: &str, path: &std::path::Path, offset: u64, size: usize, part: i32) -> Result<()> {
    let mut last_err = None;
    for attempt in 1..=MAX_RETRIES {
        use tokio::io::{AsyncReadExt, AsyncSeekExt};
        const CHUNK: usize = 256 * 1024; // 256KB chunks

        let (tx, rx) = tokio::sync::mpsc::channel::<std::result::Result<Vec<u8>, std::io::Error>>(4);
        let p = path.to_path_buf();
        let producer = tokio::spawn(async move {
            let mut f = tokio::fs::File::open(&p).await?;
            f.seek(std::io::SeekFrom::Start(offset)).await?;
            let mut rem = size;
            while rem > 0 {
                let n = rem.min(CHUNK);
                let mut buf = vec![0u8; n];
                f.read_exact(&mut buf).await?;
                if tx.send(Ok(buf)).await.is_err() { break; }
                rem -= n;
            }
            Ok::<(), std::io::Error>(())
        });

        let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        let body = reqwest::Body::wrap_stream(stream);
        let resp = oss.put(url).header("Content-Type", "application/octet-stream").header("Content-Length", size.to_string()).body(body).send().await;
        let _ = producer.await;

        match resp {
            Ok(r) if r.status().is_success() => return Ok(()),
            Ok(r) => { let s = r.status(); let b = r.text().await.unwrap_or_default(); tracing::warn!(part, attempt, status = %s, "PUT failed"); last_err = Some(Yun139Error::Api { code: s.as_u16().to_string(), message: b }); }
            Err(e) => { tracing::warn!(part, attempt, err = %e, "PUT error"); last_err = Some(Yun139Error::Http(e)); }
        }
        let delay = std::time::Duration::from_secs(1 << attempt.min(4));
        tokio::time::sleep(delay).await;
    }
    Err(last_err.unwrap())
}

fn calc_part_size(file_size: u64) -> i64 {
    if file_size > 30 * 1024 * 1024 * 1024 { 512 * 1024 * 1024 }
    else { 100 * 1024 * 1024 }
}

fn guess_content_type(name: &str) -> &'static str {
    match name.rsplit('.').next().map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("mp4") => "video/mp4",     Some("mov") => "video/quicktime",
        Some("avi") => "video/x-msvideo", Some("mkv") => "video/x-matroska",
        Some("mp3") => "audio/mpeg",     Some("pdf") => "application/pdf",
        Some("zip") => "application/zip", Some("gz") | Some("tgz") => "application/gzip",
        Some("tar") => "application/x-tar", Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg", Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",  Some("txt") => "text/plain",
        Some("html") | Some("htm") => "text/html", Some("json") => "application/json",
        Some("xml") => "application/xml", _ => "application/octet-stream",
    }
}
