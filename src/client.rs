//! 139 云盘 HTTP 客户端。
//!
//! 核心结构体和基础 API（路由发现、文件列表、路径解析、目录操作）。
//! 下载/上传的具体实现在 [`crate::commands`] 模块中，以 `impl Yun139Client` 扩展。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use reqwest::Client;
use tokio::sync::OnceCell;

use crate::api::*;
use crate::error::{Result, Yun139Error};
use crate::sign;

/// 路由发现地址（固定）
const ROUTE_POLICY_URL: &str = "https://user-njs.yun.139.com/user/route/qryRoutePolicy";

#[derive(Clone)]
pub struct Yun139Client {
    /// API 请求客户端（30s 超时，用于路由发现、文件列表等轻量 API）
    http: Client,
    /// 传输客户端（无超时，用于文件上传/下载的实际数据传输）
    transfer_http: Client,
    /// base64 部分（不含 "Basic " 前缀）
    authorization: String,
    /// 手机号
    account: String,
    /// 已解析的个人云主机地址，如 `https://personal-kd-njs.yun.139.com/hcy`
    personal_host: OnceCell<String>,
    /// 路径 → fileId 缓存（不含前缀 "/"，用 "/" 分隔）。
    ///
    /// 避免 resolve_path / ensure_dir 对相同路径前缀重复发出 HTTP 请求。
    /// scan_dir 在获得目录 file_id 后调用 [`Self::cache_path_id`] 写入；
    /// 上传任务的 ensure_dir 命中缓存后可直接返回，无需任何 HTTP 调用。
    path_id_cache: Arc<Mutex<HashMap<String, String>>>,
}

impl Yun139Client {
    /// 创建客户端。
    ///
    /// `authorization` — 完整 "Basic xxx" 值，或仅 base64 部分均可。
    pub fn new(authorization: impl Into<String>) -> Result<Self> {
        let raw = authorization.into();
        let b64 = raw.strip_prefix("Basic ").unwrap_or(&raw).to_string();

        let account = Self::extract_account(&b64);

        let ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

        let http = Client::builder()
            .user_agent(ua)
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        let transfer_http = Client::builder()
            .user_agent(ua)
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(30))
            .pool_max_idle_per_host(20)
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .build()?;

        Ok(Self {
            http,
            transfer_http,
            authorization: b64,
            account,
            personal_host: OnceCell::new(),
            path_id_cache: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// 手动指定个人云主机地址（跳过路由发现）。
    pub fn with_personal_host(self, host: impl Into<String>) -> Self {
        let _ = self.personal_host.set(host.into());
        self
    }

    fn extract_account(b64: &str) -> String {
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .map(|s| {
                s.split(':')
                    .nth(1)
                    .unwrap_or_default()
                    .to_string()
            })
            .unwrap_or_default()
    }

    // ── 通用请求 ──

    fn build_headers(&self, body_str: &str) -> reqwest::header::HeaderMap {
        let mut h = reqwest::header::HeaderMap::new();
        h.insert("Accept", "application/json, text/plain, */*".parse().unwrap());
        h.insert("Content-Type", "application/json;charset=UTF-8".parse().unwrap());
        h.insert("Authorization", format!("Basic {}", self.authorization).parse().unwrap());
        h.insert("Caller", "web".parse().unwrap());
        h.insert("CMS-DEVICE", "default".parse().unwrap());
        h.insert("mcloud-channel", "1000101".parse().unwrap());
        h.insert("mcloud-client", "10701".parse().unwrap());
        h.insert("mcloud-route", "001".parse().unwrap());
        h.insert("mcloud-sign", sign::make_mcloud_sign(body_str).parse().unwrap());
        h.insert("mcloud-version", "7.14.0".parse().unwrap());
        h.insert("Origin", "https://yun.139.com".parse().unwrap());
        h.insert("Referer", "https://yun.139.com/w/".parse().unwrap());
        h.insert("x-DeviceInfo", "||9|7.14.0|chrome|120.0.0.0|||windows 10||zh-CN|||".parse().unwrap());
        h.insert("x-huawei-channelSrc", "10000034".parse().unwrap());
        h.insert("x-inner-ntwk", "2".parse().unwrap());
        h.insert("x-m4c-caller", "PC".parse().unwrap());
        h.insert("x-m4c-src", "10002".parse().unwrap());
        h.insert("x-SvcType", "1".parse().unwrap());
        h.insert("x-yun-api-version", "v1".parse().unwrap());
        h.insert("x-yun-app-channel", "10000034".parse().unwrap());
        h.insert("x-yun-channel-source", "10000034".parse().unwrap());
        h.insert("x-yun-client-info", "||9|7.14.0|chrome|120.0.0.0|||windows 10||zh-CN|||dW5kZWZpbmVk||".parse().unwrap());
        h.insert("x-yun-module-type", "100".parse().unwrap());
        h.insert("x-yun-svc-type", "1".parse().unwrap());
        h
    }

    // ── 文件列表 ──

    pub async fn list_files(
        &self,
        parent_file_id: &str,
        page_cursor: &str,
    ) -> Result<FileListResp> {
        let host = self.personal_host().await?;
        let url = format!("{}/file/list", host);

        let body = serde_json::json!({
            "imageThumbnailStyleList": ["Small", "Large"],
            "parentFileId": parent_file_id,
            "pageInfo": {
                "pageCursor": page_cursor,
                "pageSize": 100
            },
            "orderBy": "updated_at",
            "orderDirection": "DESC"
        });

        self.post_checked(&url, &body).await
    }

    // ── 路径解析 ──

    /// 将 cloud 路径（不含前缀 "/"）与 fileId 的映射写入缓存。
    ///
    /// scan_dir 在获得目录 file_id 后调用此方法，后续 ensure_dir/resolve_path
    /// 可命中缓存，无需重复发出 HTTP 请求。
    pub(crate) fn cache_path_id(&self, path: &str, file_id: &str) {
        let key = path.trim_start_matches('/').to_string();
        if !key.is_empty() && !file_id.is_empty() {
            self.path_id_cache.lock().unwrap().insert(key, file_id.to_string());
        }
    }

    /// 将 `/test/test.mp4` 解析为 fileId。
    ///
    /// 使用路径缓存：已解析过的路径前缀不再发出 HTTP 请求。
    pub async fn resolve_path(&self, cloud_path: &str) -> Result<FileItem> {
        let parts: Vec<&str> = cloud_path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        if parts.is_empty() {
            return Err(Yun139Error::PathNotFound(cloud_path.into()));
        }

        let mut parent_id = "/".to_string();
        let mut path_so_far = String::new();

        for (i, name) in parts.iter().enumerate() {
            let is_last = i == parts.len() - 1;

            if !path_so_far.is_empty() { path_so_far.push('/'); }
            path_so_far.push_str(name);

            // 先查缓存
            let cached = self.path_id_cache.lock().unwrap().get(&path_so_far).cloned();
            if let Some(cached_id) = cached {
                if is_last {
                    // 从缓存构造 FileItem（resolve_parent_id 只需 file_id + type）
                    return Ok(FileItem {
                        file_id: Some(cached_id),
                        file_type: Some("folder".to_string()),
                        name: Some(name.to_string()),
                        size: None,
                        updated_at: None,
                        content_hash: None,
                    });
                }
                parent_id = cached_id;
                continue;
            }

            let item = self.find_in_dir(&parent_id, name).await?
                .ok_or_else(|| {
                    let partial: String = parts[..=i].join("/");
                    Yun139Error::PathNotFound(format!("/{partial}"))
                })?;

            // 写入缓存（包括最后一个节点）
            let new_id = item.file_id.clone().unwrap_or_default();
            self.path_id_cache.lock().unwrap().insert(path_so_far.clone(), new_id.clone());

            if is_last {
                return Ok(item);
            }

            if item.file_type.as_deref() != Some("folder") {
                let partial: String = parts[..=i].join("/");
                return Err(Yun139Error::PathNotFound(format!("/{partial} is not a folder")));
            }

            parent_id = new_id;
        }

        unreachable!()
    }

    async fn find_in_dir(&self, parent_id: &str, name: &str) -> Result<Option<FileItem>> {
        let mut cursor = String::new();
        loop {
            let resp = self.list_files(parent_id, &cursor).await?;

            let data = match resp.data {
                Some(d) => d,
                None => return Ok(None),
            };

            for item in &data.items {
                if item.name.as_deref() == Some(name) {
                    return Ok(Some(item.clone()));
                }
            }

            match data.next_page_cursor {
                Some(ref c) if !c.is_empty() => cursor = c.clone(),
                _ => return Ok(None),
            }
        }
    }

    // ── 获取下载链接 ──

    pub async fn get_download_url(&self, file_id: &str) -> Result<String> {
        let host = self.personal_host().await?;
        let url = format!("{}/file/getDownloadUrl", host);

        let body = serde_json::json!({ "fileId": file_id });
        let resp: DownloadUrlResp = self.post_checked(&url, &body).await?;

        resp.download_url()
            .map(|s| s.to_string())
            .ok_or(Yun139Error::NoDownloadUrl)
    }

    // ── 创建文件夹 ──

    /// 在指定父目录下创建文件夹，返回新文件夹的 fileId。
    pub async fn create_folder(&self, parent_file_id: &str, name: &str) -> Result<String> {
        let host = self.personal_host().await?;
        let url = format!("{}/file/create", host);

        let body = serde_json::json!({
            "parentFileId": parent_file_id,
            "name": name,
            "type": "folder",
            "fileRenameMode": "refuse"
        });

        let resp: CreateFolderResp = self.post_checked(&url, &body).await?;

        Ok(resp.data
            .and_then(|d| d.file_id)
            .unwrap_or_default())
    }

    /// 确保云盘路径存在（类似 `mkdir -p`），返回最终目录的 fileId。
    ///
    /// 逐级检查并创建不存在的目录。命中路径缓存时无需任何 HTTP 请求。
    pub async fn ensure_dir(&self, cloud_dir: &str) -> Result<String> {
        if cloud_dir.is_empty() || cloud_dir == "/" {
            return Ok("/".to_string());
        }

        let parts: Vec<&str> = cloud_dir
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        // 先检查完整路径是否已缓存（scan_dir 在扫描目录后写入）
        let full_key = parts.join("/");
        if let Some(cached) = self.path_id_cache.lock().unwrap().get(&full_key).cloned() {
            return Ok(cached);
        }

        let mut parent_id = "/".to_string();
        let mut path_so_far = String::new();

        for name in &parts {
            if !path_so_far.is_empty() { path_so_far.push('/'); }
            path_so_far.push_str(name);

            // 检查前缀缓存
            if let Some(cached) = self.path_id_cache.lock().unwrap().get(&path_so_far).cloned() {
                parent_id = cached;
                continue;
            }

            match self.find_in_dir(&parent_id, name).await? {
                Some(item) if item.file_type.as_deref() == Some("folder") => {
                    let new_id = item.file_id.unwrap_or_default();
                    self.path_id_cache.lock().unwrap().insert(path_so_far.clone(), new_id.clone());
                    parent_id = new_id;
                }
                Some(_) => {
                    return Err(Yun139Error::PathNotFound(
                        format!("{cloud_dir}: component '{name}' is not a folder"),
                    ));
                }
                None => {
                    tracing::info!(parent = %parent_id, name = %name, "creating folder");
                    let new_id = self.create_folder(&parent_id, name).await?;
                    self.path_id_cache.lock().unwrap().insert(path_so_far.clone(), new_id.clone());
                    parent_id = new_id;
                }
            }
        }

        Ok(parent_id)
    }

    // ── 高层便捷方法（路径解析 + 操作） ──

    /// 从云盘路径下载文件到本地（自动解析路径 → 获取下载链接 → 分片下载）。
    pub async fn download(
        &self,
        cloud_path: &str,
        local_path: &str,
        parallel: usize,
        on_progress: impl Fn(u64, Option<u64>) + Send + Sync + 'static,
    ) -> Result<u64> {
        tracing::info!(path = %cloud_path, "resolving cloud path");
        let item = self.resolve_path(cloud_path).await?;

        if item.file_type.as_deref() == Some("folder") {
            return Err(Yun139Error::IsDirectory(cloud_path.into()));
        }

        let file_id = item.file_id.as_deref().unwrap_or_default();
        tracing::info!(file_id = %file_id, name = ?item.name, size = ?item.size, "resolved");

        let url = self.get_download_url(file_id).await?;

        if parallel <= 1 {
            self.download_single(&url, std::path::Path::new(local_path), on_progress).await
        } else {
            self.download_parallel(&url, std::path::Path::new(local_path), parallel, on_progress).await
        }
    }

    /// 上传本地文件到云盘路径（自动解析目标目录 → SHA256 → 分片上传）。
    pub async fn upload(
        &self,
        local_path: &str,
        cloud_dir: &str,
        on_progress: impl Fn(u64, u64) + Send + Sync + 'static,
    ) -> Result<String> {
        self.upload_file(std::path::Path::new(local_path), cloud_dir, on_progress).await
    }

    // ── 内部方法 & 供 commands 模块使用的访问器 ──

    /// 获取传输 HTTP 客户端引用（无超时，用于文件上传/下载）。
    pub(crate) fn transfer_http(&self) -> &Client {
        &self.transfer_http
    }

    /// 通过路由策略接口获取个人云主机地址。
    async fn discover_personal_host(&self) -> Result<String> {
        let body = serde_json::json!({
            "userInfo": {
                "userType": 1,
                "accountType": 1,
                "accountName": self.account,
            },
            "modAddrType": 1
        });

        let resp: RoutePolicyResp = self.post_json(ROUTE_POLICY_URL, &body).await?;

        if !resp.success() {
            return Err(Yun139Error::RouteDiscovery(format!(
                "code={}, message={}",
                resp.code().unwrap_or("?"),
                resp.message().unwrap_or("?"),
            )));
        }

        resp.data
            .ok_or_else(|| Yun139Error::RouteDiscovery("empty data in route policy".into()))?
            .route_policy_list
            .into_iter()
            .find(|p| p.mod_name.as_deref() == Some("personal"))
            .and_then(|p| p.https_url)
            .map(|u| u.trim_end_matches('/').to_string())
            .ok_or_else(|| Yun139Error::RouteDiscovery("no personal host in route policy".into()))
    }

    /// 获取（缓存的）个人云主机地址。
    pub(crate) async fn personal_host(&self) -> Result<&str> {
        self.personal_host
            .get_or_try_init(|| self.discover_personal_host())
            .await
            .map(|s| s.as_str())
    }

    /// 发送签名 POST 请求并反序列化响应（供 commands 模块使用）。
    pub(crate) async fn post_json<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        body: &serde_json::Value,
    ) -> Result<T> {
        let body_str = body.to_string();
        let headers = self.build_headers(&body_str);

        tracing::debug!(url = %url, "POST");

        let resp = self.http
            .post(url)
            .headers(headers)
            .body(body_str)
            .send()
            .await?;

        let status = resp.status();
        let text = resp.text().await?;
        tracing::trace!(status = %status, body = %text, "response");

        if !status.is_success() {
            return Err(Yun139Error::Api {
                code: status.as_u16().to_string(),
                message: text,
            });
        }

        Ok(serde_json::from_str(&text)?)
    }

    /// 发送签名 POST → 反序列化 → 检查 `success` 字段。
    ///
    /// 等价于 `self.post_json().await?.check()`。
    pub(crate) async fn post_checked<T>(&self, url: &str, body: &serde_json::Value) -> Result<T>
    where
        T: serde::de::DeserializeOwned + crate::api::ApiResponse,
    {
        let resp: T = self.post_json(url, body).await?;
        resp.check()
    }
}
