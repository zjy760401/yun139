//! 列表命令 — 列出云盘目录内容。
//!
//! 对应 libcloud.dylib: `POST /file/list`
//! 支持分页遍历，返回文件和文件夹列表。

use crate::api::*;
use crate::error::{Result, Yun139Error};
use crate::Yun139Client;

/// 列表查询结果。
#[derive(Debug, Clone)]
pub struct ListResult {
    /// 查询的云盘路径
    pub path: String,
    /// 本页条目
    pub items: Vec<ListItem>,
    /// 是否还有下一页
    pub has_more: bool,
}

/// 列表中的单个条目。
#[derive(Debug, Clone)]
pub struct ListItem {
    pub file_id: String,
    pub name: String,
    pub size: i64,
    pub is_folder: bool,
    pub updated_at: String,
    /// 文件内容 SHA256（如果 API 返回了的话）
    pub content_hash: String,
}

impl Yun139Client {
    /// 列出云盘目录内容（全部遍历，不分页）。
    ///
    /// # 参数
    /// - `cloud_dir`: 云盘目录路径，`/` 表示根目录
    pub async fn list_all(&self, cloud_dir: &str) -> Result<ListResult> {
        let parent_file_id = self.resolve_parent_id(cloud_dir).await?;
        let mut all_items = Vec::new();
        let mut cursor = String::new();

        loop {
            let resp = self.list_files(&parent_file_id, &cursor).await?;

            let data = match resp.data {
                Some(d) => d,
                None => break,
            };

            for item in &data.items {
                all_items.push(ListItem {
                    file_id: item.file_id.clone().unwrap_or_default(),
                    name: item.name.clone().unwrap_or_default(),
                    size: item.size.unwrap_or(0),
                    is_folder: item.file_type.as_deref() == Some("folder"),
                    updated_at: item.updated_at.clone().unwrap_or_default(),
                    content_hash: item.content_hash.clone().unwrap_or_default(),
                });
            }

            match data.next_page_cursor {
                Some(ref c) if !c.is_empty() => cursor = c.clone(),
                _ => break,
            }
        }

        tracing::info!(path = %cloud_dir, count = all_items.len(), "listed");
        Ok(ListResult {
            path: cloud_dir.to_string(),
            items: all_items,
            has_more: false,
        })
    }

    /// 列出云盘目录内容（单页，指定页大小）。
    ///
    /// # 参数
    /// - `cloud_dir`: 云盘目录路径
    /// - `page_cursor`: 分页游标，首页传 `""`
    /// - `page_size`: 每页条目数
    pub async fn list_page(
        &self,
        cloud_dir: &str,
        page_cursor: &str,
        page_size: u32,
    ) -> Result<(Vec<ListItem>, Option<String>)> {
        let parent_file_id = self.resolve_parent_id(cloud_dir).await?;
        let host = self.personal_host().await?;
        let url = format!("{}/file/list", host);

        let body = serde_json::json!({
            "imageThumbnailStyleList": ["Small", "Large"],
            "parentFileId": parent_file_id,
            "pageInfo": {
                "pageCursor": page_cursor,
                "pageSize": page_size,
            },
            "orderBy": "updated_at",
            "orderDirection": "DESC"
        });

        let resp: FileListResp = self.post_checked(&url, &body).await?;

        let data = match resp.data {
            Some(d) => d,
            None => return Ok((Vec::new(), None)),
        };

        let items: Vec<ListItem> = data
            .items
            .iter()
            .map(|item| ListItem {
                file_id: item.file_id.clone().unwrap_or_default(),
                name: item.name.clone().unwrap_or_default(),
                size: item.size.unwrap_or(0),
                is_folder: item.file_type.as_deref() == Some("folder"),
                updated_at: item.updated_at.clone().unwrap_or_default(),
                content_hash: item.content_hash.clone().unwrap_or_default(),
            })
            .collect();

        let next = data
            .next_page_cursor
            .filter(|c| !c.is_empty());

        Ok((items, next))
    }
}

impl Yun139Client {
    /// 通过已知 `file_id` 直接列出目录内容（全部遍历，不分页）。
    ///
    /// 与 [`list_all`] 的区别：跳过路径解析 (`resolve_parent_id`)，
    /// 直接用 `file_id` 请求 `/file/list`，减少 O(depth) 次 HTTP 调用。
    ///
    /// scan_dir 在持有 cloud_file_id 时调用此方法代替 `list_all`。
    pub async fn list_all_by_id(&self, file_id: &str) -> Result<Vec<ListItem>> {
        let mut all_items = Vec::new();
        let mut cursor = String::new();

        loop {
            let resp = self.list_files(file_id, &cursor).await?;

            let data = match resp.data {
                Some(d) => d,
                None => break,
            };

            for item in &data.items {
                all_items.push(ListItem {
                    file_id: item.file_id.clone().unwrap_or_default(),
                    name: item.name.clone().unwrap_or_default(),
                    size: item.size.unwrap_or(0),
                    is_folder: item.file_type.as_deref() == Some("folder"),
                    updated_at: item.updated_at.clone().unwrap_or_default(),
                    content_hash: item.content_hash.clone().unwrap_or_default(),
                });
            }

            match data.next_page_cursor {
                Some(ref c) if !c.is_empty() => cursor = c.clone(),
                _ => break,
            }
        }

        tracing::debug!(file_id = %file_id, count = all_items.len(), "listed by id");
        Ok(all_items)
    }
}

// ── 内部辅助 ──

impl Yun139Client {
    async fn resolve_parent_id(&self, cloud_dir: &str) -> Result<String> {
        if cloud_dir.is_empty() || cloud_dir == "/" {
            Ok("/".to_string())
        } else {
            let item = self.resolve_path(cloud_dir).await?;
            if item.file_type.as_deref() != Some("folder") {
                return Err(Yun139Error::PathNotFound(
                    format!("{cloud_dir} is not a folder"),
                ));
            }
            Ok(item.file_id.unwrap_or_else(|| "/".to_string()))
        }
    }
}
