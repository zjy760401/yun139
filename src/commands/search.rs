//! 搜索命令 — 在云盘中搜索文件/文件夹。
//!
//! 对应 API: `POST /file/search`

use crate::api::*;
use crate::commands::list::ListItem;
use crate::error::Result;
use crate::Yun139Client;

/// 搜索结果。
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// 搜索关键词
    pub keyword: String,
    /// 匹配的条目
    pub items: Vec<ListItem>,
}

impl Yun139Client {
    /// 搜索云盘文件/文件夹（按名称关键词）。
    ///
    /// # 参数
    /// - `keyword`: 搜索关键词
    /// - `limit`: 最大返回条目数（0 表示不限制，默认遍历全部）
    pub async fn search(&self, keyword: &str, limit: usize) -> Result<SearchResult> {
        let host = self.personal_host().await?;
        let url = format!("{}/file/search", host);
        let mut all_items = Vec::new();
        let mut cursor = String::new();

        loop {
            let body = serde_json::json!({
                "parentFileId": "/",
                "keyword": keyword,
                "pageInfo": {
                    "pageCursor": cursor,
                    "pageSize": 100
                }
            });

            let resp: FileSearchResp = self.post_checked(&url, &body).await?;

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

                if limit > 0 && all_items.len() >= limit {
                    return Ok(SearchResult {
                        keyword: keyword.to_string(),
                        items: all_items,
                    });
                }
            }

            match data.next_page_cursor {
                Some(ref c) if !c.is_empty() => cursor = c.clone(),
                _ => break,
            }
        }

        tracing::info!(keyword = %keyword, count = all_items.len(), "search complete");
        Ok(SearchResult {
            keyword: keyword.to_string(),
            items: all_items,
        })
    }
}
