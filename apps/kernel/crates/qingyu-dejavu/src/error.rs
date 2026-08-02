#[derive(Debug, thiserror::Error)]
pub enum RepoError {
    #[error("repository cloud operation failed")]
    Cloud(#[from] crate::CloudError),
    #[error("repository I/O failed")]
    Io(#[from] std::io::Error),
    #[error("repository serialization failed")]
    Serialization(#[from] serde_json::Error),
    #[error("repository compression failed")]
    Compression(#[source] std::io::Error),
    #[error("repository decoded data exceeds the {limit}-byte limit")]
    DecodedSizeLimitExceeded { limit: usize },
    #[error("repository key derivation failed")]
    KeyDerivationFailed,
    #[error("repository encryption failed")]
    EncryptionFailed,
    #[error("repository decryption failed")]
    DecryptionFailed,
    #[error("cryptographic randomness is unavailable")]
    RandomnessUnavailable,
    #[error("repository data is invalid: {0}")]
    InvalidData(&'static str),
    #[error("repository object was not found: {0}")]
    NotFound(String),
    #[error("repository operation was cancelled")]
    Cancelled,
    #[error("repository lifecycle operation is already in progress")]
    RepositoryBusy,
    #[error("repository data directory contains no indexable files")]
    EmptyIndex,
    #[error("a file changed while the repository index was being built")]
    IndexFileChanged,
    #[error("a repository file identity resolved to different immutable contents")]
    FileIdentityCollision,
    #[error("the working tree changed after the operation was planned")]
    WorkingTreeChanged,
    #[error(transparent)]
    RemoteLockUnhealthy(#[from] crate::RemoteLockHealthError),
    #[error("sync operation failed: {operation}; remote lock release also failed: {unlock}")]
    OperationAndUnlockFailed {
        operation: Box<RepoError>,
        unlock: crate::CloudError,
    },
    #[error("repository indexing encountered a fatal invariant failure")]
    RepoFatal,
    #[error("repository path is unsafe")]
    UnsafePath,
    #[error("repository path contains a non-portable component")]
    PortableNameRequired { component: String },
}
