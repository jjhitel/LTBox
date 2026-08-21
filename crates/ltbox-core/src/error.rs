//! Workspace-wide error type. Every fallible API returns [`Result<T>`].

use thiserror::Error;

#[derive(Error, Debug)]
pub enum LtboxError {
    #[error("Patch error: {0}")]
    Patch(String),

    #[error("AVB error: {0}")]
    Avb(String),

    #[error("Boot image error: {0}")]
    BootImage(String),

    #[error("Download error: {0}")]
    Download(String),

    #[error("Config error: {0}")]
    Config(String),

    #[error("File not found: {0}")]
    FileNotFound(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, LtboxError>;
