use thiserror::Error;

#[derive(Debug, Error)]
pub enum Yun139Error {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("API error: code={code}, message={message}")]
    Api { code: String, message: String },

    #[error("cloud path not found: {0}")]
    PathNotFound(String),

    #[error("path is a directory: {0}")]
    IsDirectory(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("no download URL in response")]
    NoDownloadUrl,

    #[error("route discovery failed: {0}")]
    RouteDiscovery(String),
}

pub type Result<T> = std::result::Result<T, Yun139Error>;
