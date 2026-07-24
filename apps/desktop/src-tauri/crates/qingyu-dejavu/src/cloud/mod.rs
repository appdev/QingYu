#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CloudObject {
    /// Full repository object key. [`Cloud::list`] filters with a string prefix.
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
    #[error("cloud quota was exceeded")]
    QuotaExceeded,
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
            Self::QuotaExceeded => "quota_exceeded",
            Self::Locked => "locked",
            Self::LockFailed { .. } => "lock_failed",
            Self::UnlockFailed { .. } => "unlock_failed",
            Self::Injected(_) => "injected",
            Self::Io(_) => "io",
            Self::Backend { code, .. } => code,
        }
    }

    pub fn is_retryable(&self) -> bool {
        match self {
            Self::RateLimited | Self::Unavailable | Self::Locked | Self::Io(_) => true,
            Self::LockFailed { source } | Self::UnlockFailed { source } => source.is_retryable(),
            Self::Backend { retryable, .. } => *retryable,
            Self::NotFound
            | Self::AlreadyExists
            | Self::UnsafeKey
            | Self::Auth
            | Self::Forbidden
            | Self::ClockSkew
            | Self::QuotaExceeded
            | Self::Injected(_) => false,
        }
    }

    pub(crate) fn backend(code: &'static str) -> Self {
        Self::Backend {
            code,
            retryable: false,
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
    async fn get(&self, key: &str) -> Result<Vec<u8>, CloudError>;
    /// Publishes `bytes` and returns their exact payload length on success.
    ///
    /// With `overwrite == false`, an existing safe regular object returns
    /// [`CloudError::AlreadyExists`]. Implementations own any transport timeout.
    async fn put(&self, key: &str, bytes: &[u8], overwrite: bool) -> Result<u64, CloudError>;
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
            (CloudError::QuotaExceeded, "quota_exceeded", false),
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

pub use local::LocalCloud;
