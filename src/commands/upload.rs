//! 上传命令 — 分片上传文件到云盘。
//!
//! 提供两个入口：
//! - `upload_file`        — 内部自动计算 SHA256，适合单文件上传
//! - `upload_file_prehashed` — 接受外部预计算的 SHA256，供 sync 使用
//!
//! **为什么要拆分 SHA256？**
//! sync 时若在持有 global_sem 后才计算 SHA256，会导致所有并发槽被磁盘 IO
//! 占满、没有槽留给真正的网络上传。拆分后 SHA256 可以在 sem 之外自由并行。
//!
//! 上传流程（与 libcloud.dylib 对齐）：
//!   1. SHA256                     → 秒传检测输入（可外部预计算）
//!   2. POST /file/create          → 获取 uploadId + partInfos，或直接秒传
//!   3. POST /file/getUploadUrl    → 获取每个分片的 OSS 预签名 URL
//!   4. PUT  分片 → OSS            → 实际上传字节流
//!   5. POST /file/complete        → 通知服务器合并分片

use crate::api::*;
use crate::error::{Result, Yun139Error};
use crate::Yun139Client;

const MAX_RETRIES: u32 = 5;
/// ≤ 此值走单次上传，> 此值走分片
const SMALL_FILE_THRESHOLD: u64 = 10 * 1024 * 1024;

impl Yun139Client {
    /// 计算本地文件的 SHA256（hex 小写）及文件大小。
    ///
    /// 返回 `(hash_hex, file_size_bytes)`。文件大小从实际读取字节数得出，
    /// 避免单独 stat 调用（且不受磁盘休眠导致后续 stat 返回 0 的影响）。
    ///
    /// **sync 场景应在获取 global_sem 之前调用此函数**，否则磁盘 IO 会
    /// 占用上传并发槽，导致所有文件卡在 0 B/s。
    pub async fn sha256_file(local_path: &std::path::Path) -> Result<(String, u64)> {
        compute_sha256(local_path).await
    }

    /// 上传文件（自动计算 SHA256）。
    ///
    /// 适合单文件上传场景；sync 场景请使用 [`upload_file_prehashed`]。
    pub async fn upload_file(
        &self,
        local_path: &std::path::Path,
        cloud_dir: &str,
        on_progress: impl Fn(u64, u64) + Send + Sync + 'static,
    ) -> Result<String> {
        let metadata = tokio::fs::metadata(local_path).await?;
        let file_size = metadata.len();

        tracing::debug!(file = %local_path.display(), size = file_size, "upload_file: computing SHA256");
        let t0 = std::time::Instant::now();
        let (content_hash, _) = compute_sha256(local_path).await?;
        tracing::debug!(hash = %content_hash, elapsed_ms = t0.elapsed().as_millis() as u64, "upload_file: SHA256 done");

        self.upload_file_prehashed(local_path, file_size, &content_hash, cloud_dir, on_progress).await
    }

    /// 上传文件（接受外部预计算的 SHA256）。
    ///
    /// sync 使用此接口：SHA256 在 global_sem 之外预先计算完毕，
    /// 调用本函数时只占用网络上传槽，不再因磁盘 IO 阻塞并发。
    pub async fn upload_file_prehashed(
        &self,
        local_path: &std::path::Path,
        file_size: u64,
        content_hash: &str,
        cloud_dir: &str,
        on_progress: impl Fn(u64, u64) + Send + Sync + 'static,
    ) -> Result<String> {
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

        tracing::debug!(file = %local_path.display(), size = file_size, hash = %content_hash, "upload_file_prehashed: start");

        // 1. 确保目标目录存在，获取 parentFileId
        tracing::debug!(cloud_dir, "ensuring cloud dir");
        let parent_file_id = self.ensure_dir(cloud_dir).await?;
        let host = self.personal_host().await?;

        // 2. 计算分片参数
        //    - ≤ 30 GB: 100 MB/片
        //    - > 30 GB: 512 MB/片
        let part_size = calc_part_size(file_size);
        let part_count = ((file_size as i64 + part_size - 1) / part_size) as usize;
        tracing::debug!(part_count, part_size, "upload parts");

        let first_batch: Vec<serde_json::Value> = (0..part_count.min(100))
            .map(|i| {
                let start = i as i64 * part_size;
                let byte_size = (file_size as i64 - start).min(part_size);
                // 注意：不发送 parallelHashCtx。
                // 若携带 parallelHashCtx，服务器进入"并行 hash 模式"并要求客户端在每个分片
                // 的上传 URL 请求中提供 hash chain 上下文。我们不计算 per-part hash，
                // 所以省略该字段，让服务器用默认的顺序 hash 模式（按分片到达顺序计算）。
                serde_json::json!({
                    "partNumber": (i + 1) as i32,
                    "partSize": byte_size,
                })
            })
            .collect();

        // 3. POST /file/create — 服务器用 SHA256 做秒传检测
        //    若 rapidUpload=true 或 data=null → 秒传成功，直接返回
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
            "fileRenameMode": "force_rename"
        });

        tracing::debug!(url = %create_url, "POST /file/create");
        let create_resp: UploadCreateResp = self.post_checked(&create_url, &create_body).await?;

        let data = match create_resp.data {
            Some(d) => d,
            None => {
                // data=null 代表秒传成功（服务器已有此文件）
                tracing::debug!(file = %file_name, "rapid upload: data=null (instant)");
                on_progress(file_size, file_size);
                return Ok(String::new());
            }
        };

        let file_id = data.file_id.unwrap_or_default();
        let upload_id = data.upload_id.unwrap_or_default();

        // 秒传：服务器已有同 SHA256 的文件，无需真正上传
        if data.rapid_upload.unwrap_or(false)
            || upload_id.is_empty()
            || data.part_infos.as_ref().is_some_and(|p| p.is_empty())
        {
            tracing::debug!(file = %file_name, file_id = %file_id, "rapid upload success");
            on_progress(file_size, file_size);
            return Ok(file_id);
        }

        tracing::debug!(file = %file_name, file_id = %file_id, upload_id = %upload_id, "upload task created, fetching OSS URLs");

        // 4. POST /file/getUploadUrl — 获取每个分片的 OSS 预签名 PUT URL
        let upload_urls = fetch_all_upload_urls(self, host, &file_id, &upload_id, part_count, part_size, file_size as i64).await?;
        tracing::debug!(file = %file_name, url_count = upload_urls.len(), "got OSS upload URLs");

        // 5. 开始上传分片
        let oss_client = self.transfer_http().clone();
        on_progress(0, file_size);  // 通知调用方"开始上传"（0 bytes done）

        // Arc 包装以便在分片循环内克隆给每片的进度闭包
        let on_progress = std::sync::Arc::new(on_progress);

        if file_size <= SMALL_FILE_THRESHOLD {
            // 小文件（≤ 10 MB）：一次性读取 + 单次 PUT（速度很快，无需分片内进度）
            tracing::debug!(file = %file_name, size = file_size, "small file: single PUT");
            let buf = tokio::fs::read(local_path).await?;
            let url = upload_urls.get(&1).ok_or_else(|| Yun139Error::Api {
                code: "NO_URL".into(),
                message: "no upload URL for part 1".into(),
            })?;
            put_bytes_with_retry(&oss_client, url, &buf, 1).await?;
            on_progress(file_size, file_size);
        } else {
            // 大文件（> 10 MB）：分片顺序上传
            // 139 云 OSS 使用链式 hash 上下文（InvalidPartOrder），必须 part 1 → part 2 → … 依次完成，
            // 不能并行发送多片，否则服务器返回 400 InvalidPartOrder。
            tracing::debug!(file = %file_name, parts = part_count, "large file: multipart PUT (sequential)");

            let local_path = local_path.to_path_buf();
            let mut uploaded: u64 = 0;

            for i in 0..part_count {
                let offset = i as u64 * part_size as u64;
                let read_size = ((file_size - offset) as i64).min(part_size) as usize;
                let part_number = (i + 1) as i32;
                let url = upload_urls.get(&part_number).ok_or_else(|| Yun139Error::Api {
                    code: "NO_URL".into(),
                    message: format!("no upload URL for part {part_number}"),
                })?.clone();

                // 每 500ms 向调用方报告分片内的传输估算进度
                let base = uploaded;  // 本分片开始前已完成字节数
                let op = on_progress.clone();
                let part_progress: std::sync::Arc<dyn Fn(u64) + Send + Sync + 'static> =
                    std::sync::Arc::new(move |sent_in_part| op(base + sent_in_part, file_size));

                tracing::debug!(file = %file_name, part = part_number, offset, size = read_size, "PUT part start");
                stream_put_with_retry(&oss_client, &url, &local_path, offset, read_size, part_number, part_progress).await?;
                uploaded += read_size as u64;
                tracing::debug!(file = %file_name, part = part_number, uploaded, "PUT part done");
                on_progress(uploaded, file_size); // 分片完成后的精确锚点
            }
        }

        // 6. POST /file/complete — 通知服务器分片合并完毕
        tracing::debug!(file = %file_name, file_id = %file_id, "POST /file/complete");
        let complete_url = format!("{}/file/complete", host);
        let complete_body = serde_json::json!({
            "contentHash": content_hash,
            "contentHashAlgorithm": "SHA256",
            "uploadId": upload_id,
            "fileId": file_id,
        });
        let _: UploadCompleteResp = self.post_checked(&complete_url, &complete_body).await?;

        tracing::debug!(file = %file_name, file_id = %file_id, "upload complete ✓");
        Ok(file_id)
    }
}

// ── 内部实现 ──

async fn compute_sha256(path: &std::path::Path) -> Result<(String, u64)> {
    let path = path.to_path_buf();
    let result = tokio::task::spawn_blocking(move || {
        use digest::Digest;
        let mut file = std::fs::File::open(&path)?;
        let mut hasher = sha2::Sha256::new();
        let mut buf = vec![0u8; 2 * 1024 * 1024];
        let mut total: u64 = 0;
        loop {
            let n = std::io::Read::read(&mut file, &mut buf)?;
            if n == 0 { break; }
            hasher.update(&buf[..n]);
            total += n as u64;
        }
        Ok::<(String, u64), std::io::Error>((hex::encode(hasher.finalize()), total))
    })
    .await
    .map_err(|e| Yun139Error::Io(std::io::Error::other(e)))??;
    Ok(result)
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

/// PUT bytes body（带重试 + 指数退避）。
/// 使用 bytes body（而非流式 body）确保 reqwest 发送 Content-Length 而不是
/// Transfer-Encoding: chunked —— 部分 OSS 实现拒绝 chunked 上传（400）。
///
/// **不可重试的情况**：
/// - `InvalidPartOrder`：服务器已处理该分片（本次 PUT 的字节已进入 hash 链），
///   用同一 URL 重试会继续 400。应立即返回错误，让整体上传失败，
///   下次 sync 会用新 uploadId/URL 重新上传。
async fn put_bytes_with_retry(oss: &reqwest::Client, url: &str, buf: &[u8], part: i32) -> Result<()> {
    let mut last_err = None;
    for attempt in 1..=MAX_RETRIES {
        match oss.put(url)
            .header("Content-Type", "application/octet-stream")
            .header("Content-Length", buf.len().to_string())
            .body(buf.to_vec())
            .send().await
        {
            Ok(r) if r.status().is_success() => return Ok(()),
            Ok(r) => {
                let s = r.status();
                let b = r.text().await.unwrap_or_default();
                // 记录完整的响应体，方便诊断 400/403 等错误原因
                tracing::warn!(part, attempt, status = %s, body = %b, "PUT failed");
                // InvalidPartOrder：服务器 hash 链状态不可恢复，立即放弃（不重试）。
                // 同 URL 重试只会继续 400，并额外浪费重试时间。
                if b.contains("InvalidPartOrder") {
                    return Err(Yun139Error::Api { code: s.as_u16().to_string(), message: b });
                }
                last_err = Some(Yun139Error::Api { code: s.as_u16().to_string(), message: b });
            }
            Err(e) => { tracing::warn!(part, attempt, err = %e, "PUT error"); last_err = Some(Yun139Error::Http(e)); }
        }
        let delay = std::time::Duration::from_secs(1 << attempt.min(4));
        tokio::time::sleep(delay).await;
    }
    Err(last_err.unwrap())
}

/// 大文件分片 PUT：先读入内存，再用 put_bytes_with_retry 上传。
/// 流式 body 会导致 reqwest 使用 Transfer-Encoding: chunked，OSS 可能以 400 拒绝。
/// 分片最大 20 MB（30 GB 以下文件），内存开销可接受。
///
/// `on_part_progress(bytes_in_part)`：每 500ms 以时间线性估算调用一次，让进度条持续更新。
/// 估算保守（600 KB/s，上限 90%），PUT 完成后由调用方用精确值覆盖。
async fn stream_put_with_retry(
    oss: &reqwest::Client,
    url: &str,
    path: &std::path::Path,
    offset: u64,
    size: usize,
    part: i32,
    on_part_progress: std::sync::Arc<dyn Fn(u64) + Send + Sync + 'static>,
) -> Result<()> {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    tracing::debug!(part, offset, size, "PUT part: reading into memory");
    let mut f = tokio::fs::File::open(path).await.map_err(Yun139Error::Io)?;
    f.seek(std::io::SeekFrom::Start(offset)).await.map_err(Yun139Error::Io)?;
    let mut buf = vec![0u8; size];
    f.read_exact(&mut buf).await.map_err(Yun139Error::Io)?;

    // 后台定时器：PUT 进行中每 500ms 以线性估算更新进度，避免进度条长时间显示 0 B/s。
    // 上限 90%，PUT 完成后由调用方用精确锚点覆盖。
    let size_u64 = size as u64;
    let prog = on_part_progress.clone();
    let started_at = std::time::Instant::now();
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let done2 = done.clone();
    let timer = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(500));
        interval.tick().await; // 跳过立即触发的首次 tick
        loop {
            interval.tick().await;
            if done2.load(std::sync::atomic::Ordering::Relaxed) { break; }
            // 保守估计 600 KB/s；实际速度通常 700-900 KB/s，这样进度条不会超前
            let secs = started_at.elapsed().as_secs_f64();
            let estimated = ((secs * 600.0 * 1024.0) as u64).min(size_u64 * 9 / 10);
            prog(estimated);
        }
    });

    let result = put_bytes_with_retry(oss, url, &buf, part).await;
    done.store(true, std::sync::atomic::Ordering::Relaxed);
    let _ = timer.await;
    result
}

fn calc_part_size(file_size: u64) -> i64 {
    // 20 MB/片（≤30 GB 文件）：
    //   - 每次 PUT 约 30s（@700 KB/s），网络中断概率远低于 100 MB/片
    //   - 进度条每完成一片就更新（比 100 MB 片更频繁）
    //   - 即使某片因网络中断后用同 URL 重试触发 InvalidPartOrder，
    //     下次 sync 只需重传最多 20 MB 而非 100 MB
    if file_size > 30 * 1024 * 1024 * 1024 { 512 * 1024 * 1024 }
    else { 20 * 1024 * 1024 }
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
