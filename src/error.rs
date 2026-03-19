use std::time::Duration;

/// Errors that can occur during URI-based data transfers.
#[derive(Debug, thiserror::Error)]
pub enum AyurlError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("unsupported scheme: {0}")]
    UnsupportedScheme(String),

    #[error("HTTP error {status}: {message}")]
    Http { status: u16, message: String },

    #[error("connection failed: {0}")]
    Connection(String),

    #[error("timeout after {0:?}")]
    Timeout(Duration),

    #[error("invalid URI: {0}")]
    InvalidUri(String),

    #[error("scheme handler error: {0}")]
    Handler(Box<dyn std::error::Error + Send + Sync>),

    #[error("default client already configured")]
    AlreadyConfigured,
}

/// Convenience Result type for ayurl operations.
pub type Result<T> = std::result::Result<T, AyurlError>;
