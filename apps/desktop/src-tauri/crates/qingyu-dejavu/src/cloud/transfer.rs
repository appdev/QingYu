use std::pin::Pin;

use tokio::io::AsyncRead;

use super::CloudError;

/// A repository upload body that can be reopened from byte zero for a retry.
pub trait CloudUploadSource: Send + Sync {
    fn content_length(&self) -> u64;

    fn open(&self) -> Result<Pin<Box<dyn AsyncRead + Send>>, CloudError>;
}
