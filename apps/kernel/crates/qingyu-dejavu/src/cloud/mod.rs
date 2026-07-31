use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CloudObject {
    /// Full repository object key. [`Cloud::list`] filters with a string prefix.
    pub key: String,
    pub size: u64,
}

/// Opaque, credential-free identity for one logical cloud repository target.
///
/// Callers must derive the identity only from stable storage coordinates, such as a normalized
/// endpoint, bucket and repository prefix. Credentials and other rotating connection settings
/// must not be included.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct CloudTargetIdentity([u8; 32]);

impl CloudTargetIdentity {
    pub fn from_stable_parts(parts: &[&[u8]]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(b"qingyu-cloud-target-v1\0");
        for part in parts {
            hasher.update((part.len() as u64).to_be_bytes());
            hasher.update(part);
        }
        Self(hasher.finalize().into())
    }

    pub(crate) fn ref_component(self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut encoded = String::with_capacity(self.0.len() * 2);
        for byte in self.0 {
            encoded.push(HEX[(byte >> 4) as usize] as char);
            encoded.push(HEX[(byte & 0x0f) as usize] as char);
        }
        encoded
    }
}

impl std::fmt::Debug for CloudTargetIdentity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("CloudTargetIdentity([REDACTED])")
    }
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
    #[error("cloud object already exists")]
    AlreadyExists,
    #[error("cloud object key is unsafe")]
    UnsafeKey,
    #[error("cloud authentication failed")]
    Auth,
    #[error("cloud access is forbidden")]
    Forbidden,
    #[error("cloud request clock skew is too large")]
    ClockSkew,
    #[error("cloud request was rate limited")]
    RateLimited,
    #[error("cloud backend is unavailable")]
    Unavailable,
    #[error("cloud hostname resolution failed")]
    Dns,
    #[error("cloud quota was exceeded")]
    QuotaExceeded,
    #[error("cloud response exceeded the {limit}-byte limit")]
    ResponseTooLarge { limit: u64 },
    #[error("cloud transfer length differed from the declared {expected} bytes")]
    LengthMismatch { expected: u64, actual: u64 },
    #[error("cloud repository is locked")]
    Locked,
    #[error("cloud lock operation failed")]
    LockFailed {
        #[source]
        source: Box<CloudError>,
    },
    #[error("cloud lock release failed")]
    UnlockFailed {
        #[source]
        source: Box<CloudError>,
    },
    #[error("injected cloud {0:?} failure")]
    Injected(CloudOperation),
    #[error("cloud filesystem I/O failed")]
    Io(#[source] std::io::Error),
    #[error("S3 request failed with HTTP status {status}")]
    S3Response {
        status: u16,
        request_id: Option<String>,
        retryable: bool,
    },
    #[error("cloud backend failed: {code}")]
    Backend { code: &'static str, retryable: bool },
}

impl CloudError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound => "not_found",
            Self::AlreadyExists => "already_exists",
            Self::UnsafeKey => "unsafe_key",
            Self::Auth => "auth",
            Self::Forbidden => "forbidden",
            Self::ClockSkew => "clock_skew",
            Self::RateLimited => "rate_limited",
            Self::Unavailable => "unavailable",
            Self::Dns => "dns",
            Self::QuotaExceeded => "quota_exceeded",
            Self::ResponseTooLarge { .. } => "response_too_large",
            Self::LengthMismatch { .. } => "length_mismatch",
            Self::Locked => "locked",
            Self::LockFailed { .. } => "lock_failed",
            Self::UnlockFailed { .. } => "unlock_failed",
            Self::Injected(_) => "injected",
            Self::Io(_) => "io",
            Self::S3Response { status, .. } => match *status {
                401 => "auth",
                403 => "forbidden",
                408 => "unavailable",
                429 => "rate_limited",
                500 | 502 | 503 | 504 => "unavailable",
                _ => "s3_response",
            },
            Self::Backend { code, .. } => code,
        }
    }

    pub fn is_retryable(&self) -> bool {
        match self {
            Self::RateLimited | Self::Unavailable | Self::Dns | Self::Locked | Self::Io(_) => true,
            Self::LockFailed { source } | Self::UnlockFailed { source } => source.is_retryable(),
            Self::S3Response { retryable, .. } => *retryable,
            Self::Backend { retryable, .. } => *retryable,
            Self::NotFound
            | Self::AlreadyExists
            | Self::UnsafeKey
            | Self::Auth
            | Self::Forbidden
            | Self::ClockSkew
            | Self::QuotaExceeded
            | Self::ResponseTooLarge { .. }
            | Self::LengthMismatch { .. }
            | Self::Injected(_) => false,
        }
    }

    pub(crate) fn backend(code: &'static str) -> Self {
        Self::Backend {
            code,
            retryable: false,
        }
    }

    pub fn is_dns(&self) -> bool {
        match self {
            Self::Dns => true,
            Self::LockFailed { source } | Self::UnlockFailed { source } => source.is_dns(),
            _ => false,
        }
    }

    pub(crate) fn lock_failed(source: CloudError) -> Self {
        Self::LockFailed {
            source: Box::new(source),
        }
    }

    pub(crate) fn unlock_failed(source: CloudError) -> Self {
        Self::UnlockFailed {
            source: Box::new(source),
        }
    }
}

#[async_trait::async_trait]
pub trait Cloud: Send + Sync {
    /// Returns a credential-free identity when the backend can distinguish logical targets.
    fn target_identity(&self) -> Option<CloudTargetIdentity> {
        None
    }

    /// Reads a protocol-small object, stopping before retaining bytes beyond `max_bytes`.
    async fn get_bounded(&self, key: &str, max_bytes: u64) -> Result<Vec<u8>, CloudError>;
    /// Streams an object into caller-owned staging and returns the exact bytes written.
    /// Once any body byte is written, an implementation must return a mid-body failure instead
    /// of retrying or appending a replacement response to the same destination.
    async fn download_to(
        &self,
        key: &str,
        destination: &mut (dyn tokio::io::AsyncWrite + Unpin + Send),
    ) -> Result<u64, CloudError>;
    /// Publishes `bytes` and returns their exact payload length on success.
    ///
    /// S3 implementations may use ordinary PUT for either `overwrite` value, matching Dejavu.
    /// Deterministic local implementations may retain stricter no-clobber behavior when false.
    async fn put(&self, key: &str, bytes: &[u8], overwrite: bool) -> Result<u64, CloudError>;
    /// Streams a repository object from a reopenable source, rejecting any source whose bytes
    /// are shorter or longer than [`CloudUploadSource::content_length`]. Each retry must call
    /// [`CloudUploadSource::open`] again to obtain a reader positioned at byte zero.
    async fn upload_from(
        &self,
        key: &str,
        source: &dyn CloudUploadSource,
        overwrite: bool,
    ) -> Result<u64, CloudError>;
    async fn remove(&self, key: &str) -> Result<(), CloudError>;
    /// Returns full keys whose strings start with `prefix`, globally sorted by key.
    /// Empty prefixes and one trailing slash are allowed.
    async fn list(&self, prefix: &str) -> Result<Vec<CloudObject>, CloudError>;
    /// Returns `u64::MAX` when the backend cannot report a bounded quota.
    async fn available_size(&self) -> Result<u64, CloudError>;
}

#[cfg(test)]
mod tests {
    use super::CloudError;

    #[test]
    fn stable_error_codes_and_retryability_are_explicit() {
        let cases = [
            (CloudError::Auth, "auth", false),
            (CloudError::Forbidden, "forbidden", false),
            (CloudError::ClockSkew, "clock_skew", false),
            (CloudError::RateLimited, "rate_limited", true),
            (CloudError::Unavailable, "unavailable", true),
            (CloudError::Dns, "dns", true),
            (CloudError::QuotaExceeded, "quota_exceeded", false),
            (
                CloudError::ResponseTooLarge { limit: 42 },
                "response_too_large",
                false,
            ),
            (
                CloudError::LengthMismatch {
                    expected: 4,
                    actual: 5,
                },
                "length_mismatch",
                false,
            ),
        ];
        for (error, code, retryable) in cases {
            assert_eq!(error.code(), code);
            assert_eq!(error.is_retryable(), retryable);
        }
    }

    #[test]
    fn lock_wrappers_preserve_the_underlying_cloud_error() {
        let error = CloudError::lock_failed(CloudError::Auth);
        assert_eq!(error.code(), "lock_failed");
        let CloudError::LockFailed { source } = error else {
            panic!("expected lock wrapper");
        };
        assert!(matches!(*source, CloudError::Auth));
    }

    #[test]
    fn repository_error_preserves_the_cloud_error_variant() {
        let error = crate::RepoError::from(CloudError::Forbidden);
        assert!(matches!(
            error,
            crate::RepoError::Cloud(CloudError::Forbidden)
        ));
    }
}

pub mod local;
mod s3;
mod s3_signing;
mod s3_xml;
mod transfer;

pub use local::LocalCloud;
pub use s3::{S3Cloud, S3TransportOptions};
pub use s3_signing::{S3AddressingStyle, S3Connection, S3RequestSigner, S3TlsVerification};
pub use transfer::CloudUploadSource;
