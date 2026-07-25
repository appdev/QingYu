use std::pin::Pin;

use tokio::io::AsyncRead;

use super::CloudError;

/// A repository upload body that can be reopened from byte zero for a retry.
///
/// Reopens are sequential: callers must drop the reader returned by a previous [`Self::open`]
/// call before opening the source again. Implementations may retain a stable file handle whose
/// cloned readers share an operating-system file cursor.
pub trait CloudUploadSource: Send + Sync {
    fn content_length(&self) -> u64;

    fn open(&self) -> Result<Pin<Box<dyn AsyncRead + Send>>, CloudError>;
}
