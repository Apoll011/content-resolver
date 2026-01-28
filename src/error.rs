use thiserror::Error;

/// Errors that can occur during content resolution
#[derive(Error, Debug)]
pub enum ContentError {
    #[error("Content not found: {path}")]
    NotFound { path: String },

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("Rate limited by remote service: {message}")]
    RateLimited { message: String },

    #[error("Invalid remote structure: {message}")]
    InvalidStructure { message: String },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Cache error: {message}")]
    Cache { message: String },

    #[error("Invalid configuration: {message}")]
    InvalidConfig { message: String },

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Result type alias for content operations
pub type Result<T> = std::result::Result<T, ContentError>;
