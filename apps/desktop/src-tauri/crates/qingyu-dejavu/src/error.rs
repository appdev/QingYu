#[derive(Debug, thiserror::Error)]
pub enum RepoError {
    #[error("repository I/O failed")]
    Io(#[from] std::io::Error),
    #[error("repository data is invalid: {0}")]
    InvalidData(&'static str),
    #[error("repository object was not found: {0}")]
    NotFound(String),
    #[error("repository operation was cancelled")]
    Cancelled,
}
