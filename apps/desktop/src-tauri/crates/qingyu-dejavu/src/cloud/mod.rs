#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CloudObject {
    /// Object key relative to the prefix passed to [`Cloud::list`].
    pub key: String,
    pub size: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CloudOperation {
    Get,
    Put,
    Remove,
    List,
}

#[derive(Debug, thiserror::Error)]
pub enum CloudError {
    #[error("cloud object was not found")]
    NotFound,
    #[error("cloud object key is unsafe")]
    UnsafeKey,
    #[error("cloud repository is locked")]
    Locked,
    #[error("cloud lock operation failed")]
    LockFailed,
    #[error("cloud lock release failed")]
    UnlockFailed,
    #[error("injected cloud {0:?} failure")]
    Injected(CloudOperation),
    #[error("cloud filesystem I/O failed")]
    Io(#[source] std::io::Error),
    #[error("cloud backend failed")]
    Backend,
}

#[async_trait::async_trait]
pub trait Cloud: Send + Sync {
    async fn get(&self, key: &str) -> Result<Vec<u8>, CloudError>;
    async fn put(&self, key: &str, bytes: &[u8], overwrite: bool) -> Result<u64, CloudError>;
    async fn remove(&self, key: &str) -> Result<(), CloudError>;
    async fn list(&self, prefix: &str) -> Result<Vec<CloudObject>, CloudError>;
    async fn available_size(&self) -> Result<u64, CloudError>;
}

pub mod local;

pub use local::LocalCloud;
