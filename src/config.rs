//! 配置文件管理 — 保存/加载 token 到 `~/.config/yun139/config.toml`。
//!
//! 参考 cloud139 (zjy760401/cloud139) 的配置机制，支持：
//! - `login` 命令保存 token
//! - 后续命令自动从配置文件读取，无需每次传 `--auth`
//! - `logout` 删除配置文件
//! - `login` 无参数时显示当前登录状态

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML parse error: {0}")]
    TomlParse(#[from] toml::de::Error),
    #[error("TOML serialize error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),
    #[error("配置文件不存在，请先运行 `yun139 login`")]
    NotFound,
    #[error("无法确定配置目录")]
    NoConfigDir,
    #[error("Token 格式无效: {0}")]
    InvalidToken(String),
}

/// 默认并行数。
pub const DEFAULT_PARALLEL: usize = 16;

/// 持久化配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// base64 部分（不含 "Basic " 前缀）
    pub authorization: String,
    /// 手机号（从 token 解码获取）
    pub account: String,
    /// 并行传输数（下载并发、sync 并发共用）
    #[serde(default = "default_parallel")]
    pub parallel: usize,
    /// 日志级别 (trace, debug, info, warn, error, off)
    #[serde(default = "default_log_level")]
    pub log_level: String,
    /// 日志输出到文件路径（设置后 stderr 不输出日志，不干扰进度条）
    #[serde(default)]
    pub log_file: Option<String>,
    /// 上传/同步时排除的文件名模式列表
    #[serde(default = "default_exclude")]
    pub exclude: Vec<String>,
    /// token 过期时间戳（毫秒）
    #[serde(default)]
    pub token_expire_time: Option<i64>,
    /// 缓存的个人云主机地址（可选，加速首次请求）
    #[serde(default)]
    pub personal_cloud_host: Option<String>,
}

fn default_parallel() -> usize {
    DEFAULT_PARALLEL
}

fn default_log_level() -> String {
    "warn".to_string()
}

/// macOS 常见无用文件默认排除列表。
pub fn default_exclude() -> Vec<String> {
    vec![
        ".DS_Store".into(),
        ".Spotlight-V100".into(),
        ".Trashes".into(),
        ".fseventsd".into(),
        ".TemporaryItems".into(),
        "Thumbs.db".into(),
        "desktop.ini".into(),
        "._*".into(),
        ".AppleDouble".into(),
    ]
}

impl Config {
    /// 配置文件路径: `~/.config/yun139/config.toml`
    pub fn config_path() -> Result<PathBuf, ConfigError> {
        let home = dirs::home_dir().ok_or(ConfigError::NoConfigDir)?;
        Ok(home.join(".config").join("yun139").join("config.toml"))
    }

    /// 从配置文件加载。
    pub fn load() -> Result<Self, ConfigError> {
        let path = Self::config_path()?;
        if !path.exists() {
            return Err(ConfigError::NotFound);
        }
        let content = std::fs::read_to_string(&path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    /// 保存到配置文件。
    pub fn save(&self) -> Result<PathBuf, ConfigError> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        Ok(path)
    }

    /// 删除配置文件。
    pub fn remove() -> Result<(), ConfigError> {
        let path = Self::config_path()?;
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        Ok(())
    }

    /// 从 authorization token 字符串解析并构造 Config。
    ///
    /// token 格式: base64("pc:<phone>:<token_info>")
    /// token_info 格式: <type>|<flag>|<method>|<expire_ms>|<rest...>
    pub fn from_token(token: &str) -> Result<Self, ConfigError> {
        let b64 = token.strip_prefix("Basic ").unwrap_or(token).to_string();

        let decoded = base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            &b64,
        )
        .map_err(|e| ConfigError::InvalidToken(format!("base64 decode: {e}")))?;

        let decoded_str = String::from_utf8(decoded)
            .map_err(|e| ConfigError::InvalidToken(format!("utf8: {e}")))?;

        let parts: Vec<&str> = decoded_str.splitn(3, ':').collect();
        if parts.len() < 3 {
            return Err(ConfigError::InvalidToken(
                "格式应为 base64(\"pc:<phone>:<token>\")".into(),
            ));
        }

        let account = parts[1].to_string();
        let token_info = parts[2];

        // 尝试从 token_info 提取过期时间
        let expire_time = token_info
            .split('|')
            .nth(3)
            .and_then(|s| s.parse::<i64>().ok());

        Ok(Config {
            authorization: b64,
            account,
            parallel: DEFAULT_PARALLEL,
            log_level: default_log_level(),
            log_file: None,
            exclude: default_exclude(),
            token_expire_time: expire_time,
            personal_cloud_host: None,
        })
    }

    /// 检查 token 是否已过期或即将过期（< 1 天）。
    pub fn is_expired(&self) -> bool {
        match self.token_expire_time {
            Some(expire_ms) => {
                let now_ms = chrono::Utc::now().timestamp_millis();
                expire_ms - now_ms < 24 * 60 * 60 * 1000 // < 1 day
            }
            None => false, // 无过期信息时不报过期
        }
    }

    /// 格式化过期时间为可读字符串。
    pub fn expire_time_display(&self) -> String {
        match self.token_expire_time {
            Some(ms) => {
                let dt = chrono::DateTime::from_timestamp_millis(ms);
                match dt {
                    Some(t) => t.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string(),
                    None => "未知".into(),
                }
            }
            None => "未知".into(),
        }
    }

    /// 返回 "Basic <b64>" 格式的 authorization 字符串。
    pub fn authorization_header(&self) -> String {
        format!("Basic {}", self.authorization)
    }
}
