//! 创建目录命令。
//!
//! 对应 libcloud.dylib: `POST /file/create` (type="folder")
//!
//! 提供两种模式：
//! - `mkdir`: 创建单层目录（父目录必须存在）
//! - `mkdir_recursive`: 递归创建目录（等价于 `mkdir -r`）

use crate::api::*;
use crate::error::{Result, Yun139Error};
use crate::Yun139Client;

impl Yun139Client {
    /// 创建单层目录（父目录必须存在）。
    ///
    /// # 参数
    /// - `cloud_path`: 完整路径，如 `/photos/2024`
    ///   最后一级为要创建的目录名，前面部分为父目录（必须存在）。
    ///
    /// # 返回
    /// 新目录的 fileId。
    pub async fn mkdir(&self, cloud_path: &str) -> Result<String> {
        let (parent_dir, name) = parse_mkdir_path(cloud_path)?;

        let parent_file_id = if parent_dir == "/" {
            "/".to_string()
        } else {
            let item = self.resolve_path(&parent_dir).await?;
            if item.file_type.as_deref() != Some("folder") {
                return Err(Yun139Error::PathNotFound(
                    format!("{parent_dir} is not a folder"),
                ));
            }
            item.file_id.unwrap_or_else(|| "/".to_string())
        };

        let host = self.personal_host().await?;
        let url = format!("{}/file/create", host);

        let body = serde_json::json!({
            "parentFileId": parent_file_id,
            "name": name,
            "type": "folder",
            "fileRenameMode": "refuse"
        });

        let resp: CreateFolderResp = self.post_checked(&url, &body).await?;

        let file_id = resp.data.and_then(|d| d.file_id).unwrap_or_default();
        tracing::info!(path = %cloud_path, file_id = %file_id, "directory created");
        Ok(file_id)
    }

    /// 递归创建目录（等价于 `mkdir -r`）。
    ///
    /// 逐级检查并创建不存在的中间目录。
    /// 已存在的同名目录不会报错。
    ///
    /// # 返回
    /// 最终目录的 fileId。
    pub async fn mkdir_recursive(&self, cloud_path: &str) -> Result<String> {
        self.ensure_dir(cloud_path).await
    }
}

/// 将路径拆分为 (父目录, 目录名)。
fn parse_mkdir_path(path: &str) -> Result<(String, String)> {
    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() {
        return Err(Yun139Error::Api {
            code: "INVALID".into(),
            message: "path cannot be empty or root".into(),
        });
    }

    let parts: Vec<&str> = trimmed.split('/').collect();
    let name = parts.last().unwrap().to_string();

    let parent = if parts.len() == 1 {
        "/".to_string()
    } else {
        format!("/{}", parts[..parts.len() - 1].join("/"))
    };

    Ok((parent, name))
}
