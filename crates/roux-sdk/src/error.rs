pub type RouxResult<T> = Result<T, RouxError>;

#[derive(Debug, thiserror::Error)]
pub enum RouxError {
    #[error("Roux is not running")]
    NotRunning,
    #[error("{0}")]
    Command(String),
    #[error("transport error: {0}")]
    Transport(String),
    #[error("decode error: {0}")]
    Decode(#[from] serde_json::Error),
}
