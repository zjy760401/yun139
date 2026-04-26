//! 删除命令 — 将文件/文件夹移入回收站或永久删除。
//!
//! 对应 libcloud.dylib 中的 API:
//! - `/recyclebin/batchTrash` — 移入回收站
//! - `/file/batchDelete`     — 永久删除

use crate::api::*;
use crate::error::{Result, Yun139Error};
use crate::Yun139Client;

impl Yun139Client {
    /// 删除云盘文件/文件夹（移入回收站）。
    ///
    /// # 参数
    /// - `cloud_path`: 云盘路径，如 `/test/old.txt`
    pub async fn trash(&self, cloud_path: &str) -> Result<()> {
        let file_id = self.resolve_file_id(cloud_path).await?;
        let host = self.personal_host().await?;
        let url = format!("{}/recyclebin/batchTrash", host);

        let body = serde_json::json!({ "fileIds": [file_id] });
        let _: GenericResp = self.post_checked(&url, &body).await?;

        tracing::info!(path = %cloud_path, "trashed");
        Ok(())
    }

    /// 永久删除云盘文件/文件夹（不经过回收站）。
    ///
    /// # 参数
    /// - `cloud_path`: 云盘路径，如 `/test/old.txt`
    pub async fn delete(&self, cloud_path: &str) -> Result<()> {
        let file_id = self.resolve_file_id(cloud_path).await?;
        let host = self.personal_host().await?;
        let url = format!("{}/file/batchDelete", host);

        let body = serde_json::json!({ "fileIds": [file_id] });
        let _: GenericResp = self.post_checked(&url, &body).await?;

        tracing::info!(path = %cloud_path, "permanently deleted");
        Ok(())
    }
}

// ── 内部辅助 ──

impl Yun139Client {
    /// 解析云盘路径为 fileId（禁止操作根目录）。
    async fn resolve_file_id(&self, cloud_path: &str) -> Result<String> {
        let trimmed = cloud_path.trim_matches('/');
        if trimmed.is_empty() {
            return Err(Yun139Error::Api {
                code: "INVALID".into(),
                message: "cannot operate on root directory".into(),
            });
        }
        let item = self.resolve_path(cloud_path).await?;
        item.file_id.ok_or_else(|| Yun139Error::PathNotFound(cloud_path.into()))
    }
}
