//! API 请求 / 响应类型定义。

use serde::Deserialize;

use crate::error::{Result, Yun139Error};

/// 所有 API 响应共有的成功/错误字段。
pub trait ApiResponse {
    fn success(&self) -> bool;
    fn code(&self) -> Option<&str>;
    fn message(&self) -> Option<&str>;

    /// 若 `success == false`，返回 `Err(Yun139Error::Api)`。
    fn check(self) -> Result<Self>
    where
        Self: Sized,
    {
        if self.success() {
            Ok(self)
        } else {
            Err(Yun139Error::Api {
                code: self.code().unwrap_or_default().to_string(),
                message: self.message().unwrap_or_default().to_string(),
            })
        }
    }
}

macro_rules! impl_api_response {
    ($t:ty) => {
        impl ApiResponse for $t {
            fn success(&self) -> bool { self.success }
            fn code(&self) -> Option<&str> { self.code.as_deref() }
            fn message(&self) -> Option<&str> { self.message.as_deref() }
        }
    };
}

// ── 路由策略 ──

#[derive(Debug, Deserialize)]
pub struct RoutePolicyResp {
    pub success: bool,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub data: Option<RoutePolicyData>,
}

#[derive(Debug, Deserialize)]
pub struct RoutePolicyData {
    #[serde(rename = "routePolicyList")]
    pub route_policy_list: Vec<RoutePolicy>,
}

#[derive(Debug, Deserialize)]
pub struct RoutePolicy {
    #[serde(rename = "modName", default)]
    pub mod_name: Option<String>,
    #[serde(rename = "httpsUrl", default)]
    pub https_url: Option<String>,
}

// ── 文件列表 ──

#[derive(Debug, Deserialize)]
pub struct FileListResp {
    pub success: bool,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub data: Option<FileListData>,
}

#[derive(Debug, Deserialize)]
pub struct FileListData {
    #[serde(default)]
    pub items: Vec<FileItem>,
    #[serde(rename = "nextPageCursor", default)]
    pub next_page_cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FileItem {
    #[serde(rename = "fileId", default)]
    pub file_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub size: Option<i64>,
    #[serde(rename = "type", default)]
    pub file_type: Option<String>,
    #[serde(rename = "updatedAt", default)]
    pub updated_at: Option<String>,
}

// ── 下载链接 ──

#[derive(Debug, Deserialize)]
pub struct DownloadUrlResp {
    pub success: bool,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub data: Option<DownloadUrlData>,
}

#[derive(Debug, Deserialize)]
pub struct DownloadUrlData {
    #[serde(default)]
    pub url: Option<String>,
    #[serde(rename = "cdnUrl", default)]
    pub cdn_url: Option<String>,
}

impl DownloadUrlResp {
    pub fn download_url(&self) -> Option<&str> {
        self.data.as_ref().and_then(|d| {
            d.cdn_url
                .as_deref()
                .filter(|u| !u.is_empty())
                .or(d.url.as_deref().filter(|u| !u.is_empty()))
        })
    }
}

// ── 创建文件夹 ──

#[derive(Debug, Deserialize)]
pub struct CreateFolderResp {
    pub success: bool,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub data: Option<CreateFolderData>,
}

#[derive(Debug, Deserialize)]
pub struct CreateFolderData {
    #[serde(rename = "fileId", default)]
    pub file_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

// ── 上传：创建文件 ──

#[derive(Debug, Deserialize)]
pub struct UploadCreateResp {
    pub success: bool,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub data: Option<UploadCreateData>,
}

#[derive(Debug, Deserialize)]
pub struct UploadCreateData {
    #[serde(rename = "fileId", default)]
    pub file_id: Option<String>,
    #[serde(rename = "fileName", default)]
    pub file_name: Option<String>,
    #[serde(rename = "uploadId", default)]
    pub upload_id: Option<String>,
    #[serde(default)]
    pub exist: Option<bool>,
    #[serde(rename = "rapidUpload", default)]
    pub rapid_upload: Option<bool>,
    #[serde(rename = "partInfos", default)]
    pub part_infos: Option<Vec<UploadPartInfo>>,
}

#[derive(Debug, Deserialize)]
pub struct UploadPartInfo {
    #[serde(rename = "partNumber")]
    pub part_number: i32,
    #[serde(rename = "uploadUrl", default)]
    pub upload_url: Option<String>,
}

// ── 上传：获取分片 URL ──

#[derive(Debug, Deserialize)]
pub struct GetUploadUrlResp {
    pub success: bool,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub data: Option<GetUploadUrlData>,
}

#[derive(Debug, Deserialize)]
pub struct GetUploadUrlData {
    #[serde(rename = "partInfos", default)]
    pub part_infos: Option<Vec<UploadPartInfo>>,
}

// ── 上传：完成 ──

#[derive(Debug, Deserialize)]
pub struct UploadCompleteResp {
    pub success: bool,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

// ── 通用操作响应（trash / delete / search 等） ──

#[derive(Debug, Deserialize)]
pub struct GenericResp {
    pub success: bool,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

// ── 文件搜索 ──

#[derive(Debug, Deserialize)]
pub struct FileSearchResp {
    pub success: bool,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub data: Option<FileSearchData>,
}

#[derive(Debug, Deserialize)]
pub struct FileSearchData {
    #[serde(default)]
    pub items: Vec<FileItem>,
    #[serde(rename = "nextPageCursor", default)]
    pub next_page_cursor: Option<String>,
}

// ── ApiResponse 实现 ──

impl_api_response!(RoutePolicyResp);
impl_api_response!(FileListResp);
impl_api_response!(DownloadUrlResp);
impl_api_response!(CreateFolderResp);
impl_api_response!(UploadCreateResp);
impl_api_response!(GetUploadUrlResp);
impl_api_response!(UploadCompleteResp);
impl_api_response!(GenericResp);
impl_api_response!(FileSearchResp);
