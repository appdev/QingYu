use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use crate::sync_validation::normalize_remote_root;

pub(crate) fn sync_state_key(namespace: &str, fields: &[&[u8]]) -> String {
    let mut digest = Sha256::new();
    digest.update(b"qingyu-sync-state-v1");
    digest.update((namespace.len() as u64).to_be_bytes());
    digest.update(namespace.as_bytes());
    for field in fields {
        digest.update((field.len() as u64).to_be_bytes());
        digest.update(field);
    }
    format!("{:x}", digest.finalize())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ValidRemoteRoot {
    normalized: String,
}

impl ValidRemoteRoot {
    pub(crate) fn parse(value: &str) -> Result<Self, String> {
        normalize_remote_root(value)
            .map(|normalized| Self { normalized })
            .map_err(|_| "sync-remote-root-invalid: The remote root is invalid.".to_string())
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.normalized
    }

    pub(crate) fn notes_prefix(&self) -> String {
        self.child_prefix("notes")
    }

    pub(crate) fn app_prefix(&self) -> String {
        self.child_prefix("app")
    }

    fn child_prefix(&self, child: &'static str) -> String {
        format!("{}/{child}", self.normalized)
    }
}

pub(crate) fn notebook_name_available_on_current_platform(name: &str) -> bool {
    #[cfg(windows)]
    {
        let stem = name
            .split('.')
            .next()
            .unwrap_or(name)
            .trim_end_matches(['.', ' '])
            .to_ascii_uppercase();
        let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || stem
                .strip_prefix("COM")
                .or_else(|| stem.strip_prefix("LPT"))
                .is_some_and(|suffix| {
                    suffix.len() == 1 && matches!(suffix.as_bytes()[0], b'1'..=b'9')
                });
        return name.encode_utf16().count() <= 255
            && !name.chars().any(|character| {
                character.is_control()
                    || matches!(character, '<' | '>' | ':' | '"' | '|' | '?' | '*')
            })
            && !name.ends_with(['.', ' '])
            && !reserved;
    }

    #[cfg(not(windows))]
    {
        name.len() <= 255
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RemoteSyncFile {
    pub(crate) identity: String,
    pub(crate) size: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SyncFailureCategory {
    Http,
    Integrity,
    Local,
    Transport,
}

impl SyncFailureCategory {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Integrity => "integrity",
            Self::Local => "local",
            Self::Transport => "transport",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SyncProviderOperation {
    Catalog,
    Delete,
    Download,
    List,
    Metadata,
    Upload,
}

impl SyncProviderOperation {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Catalog => "catalog",
            Self::Delete => "delete",
            Self::Download => "download",
            Self::List => "list",
            Self::Metadata => "metadata",
            Self::Upload => "upload",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RemoteSyncDiagnostic {
    pub(crate) category: SyncFailureCategory,
    pub(crate) code: String,
    pub(crate) http_status: Option<u16>,
    pub(crate) method: Option<String>,
    pub(crate) object_id: Option<String>,
    pub(crate) operation: SyncProviderOperation,
    pub(crate) provider_error_code: Option<String>,
    pub(crate) request_id: Option<String>,
    pub(crate) run_id: String,
    pub(crate) scope: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RemoteSyncError {
    diagnostic: Option<RemoteSyncDiagnostic>,
    message: String,
}

impl RemoteSyncError {
    pub(crate) fn unclassified(message: impl Into<String>) -> Self {
        Self {
            diagnostic: None,
            message: message.into(),
        }
    }

    pub(crate) fn diagnostic(diagnostic: RemoteSyncDiagnostic) -> Self {
        let message = format!(
            "{}: S3 {} failed.",
            diagnostic.code,
            diagnostic.operation.as_str()
        );
        Self {
            diagnostic: Some(diagnostic),
            message,
        }
    }

    pub(crate) fn details(&self) -> Option<&RemoteSyncDiagnostic> {
        self.diagnostic.as_ref()
    }

    pub(crate) fn safe_code(&self) -> &str {
        if let Some(diagnostic) = self.diagnostic.as_ref() {
            return &diagnostic.code;
        }
        let code = self.message.split(':').next().unwrap_or_default();
        if !code.is_empty()
            && code.len() <= 80
            && code
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            code
        } else {
            "sync-run-failed"
        }
    }
}

impl std::fmt::Display for RemoteSyncError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for RemoteSyncError {}

impl std::ops::Deref for RemoteSyncError {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.message
    }
}

impl PartialEq<&str> for RemoteSyncError {
    fn eq(&self, other: &&str) -> bool {
        self.message == *other
    }
}

impl From<String> for RemoteSyncError {
    fn from(message: String) -> Self {
        Self::unclassified(message)
    }
}

impl From<&str> for RemoteSyncError {
    fn from(message: &str) -> Self {
        Self::unclassified(message)
    }
}

impl From<RemoteSyncError> for String {
    fn from(error: RemoteSyncError) -> Self {
        error.to_string()
    }
}

#[allow(async_fn_in_trait)]
pub(crate) trait RemoteSyncBackend: Sync {
    fn target_fingerprint_source(&self) -> String;

    async fn list_files(&self) -> Result<BTreeMap<String, RemoteSyncFile>, RemoteSyncError>;
    async fn download(
        &self,
        path: &str,
        expected_identity: &str,
    ) -> Result<Vec<u8>, RemoteSyncError>;
    async fn upload(
        &self,
        path: &str,
        bytes: &[u8],
        expected_identity: Option<&str>,
    ) -> Result<String, RemoteSyncError>;
    async fn delete(&self, path: &str, expected_identity: &str) -> Result<(), RemoteSyncError>;
}

#[cfg(test)]
mod tests {
    use super::{
        notebook_name_available_on_current_platform, RemoteSyncDiagnostic, RemoteSyncError,
        SyncFailureCategory, SyncProviderOperation, ValidRemoteRoot,
    };

    #[test]
    fn typed_remote_error_preserves_safe_diagnostics_without_parsing_display_text() {
        let diagnostic = RemoteSyncDiagnostic {
            category: SyncFailureCategory::Http,
            code: "s3-upload-http-failed".into(),
            http_status: Some(403),
            method: Some("PUT".into()),
            object_id: Some("object-a1".into()),
            operation: SyncProviderOperation::Upload,
            provider_error_code: Some("AccessDenied".into()),
            request_id: Some("request-1".into()),
            run_id: "run-1".into(),
            scope: "notes".into(),
        };
        let error = RemoteSyncError::diagnostic(diagnostic.clone());

        assert_eq!(error.safe_code(), "s3-upload-http-failed");
        assert_eq!(error.details(), Some(&diagnostic));
        assert_eq!(
            error.to_string(),
            "s3-upload-http-failed: S3 upload failed."
        );
    }

    #[test]
    fn unclassified_remote_error_retains_existing_local_message() {
        let error = RemoteSyncError::unclassified("manifest-write-failed: unavailable");
        assert_eq!(error.safe_code(), "manifest-write-failed");
        assert!(error.details().is_none());
    }

    #[test]
    fn provider_paths_use_disjoint_notes_and_app_namespaces() {
        let root = ValidRemoteRoot::parse("qingyu/team").unwrap();

        assert_eq!(root.notes_prefix(), "qingyu/team/notes");
        assert_eq!(root.app_prefix(), "qingyu/team/app");
    }

    #[test]
    fn remote_root_is_strict_and_never_resolves_to_the_provider_root() {
        for invalid in ["", "/", ".", "qingyu/../other", "qingyu//other"] {
            assert!(
                ValidRemoteRoot::parse(invalid).is_err(),
                "accepted {invalid:?}"
            );
        }

        assert_eq!(
            ValidRemoteRoot::parse(" qingyu/team ").unwrap().as_str(),
            "qingyu/team"
        );
        assert_eq!(
            ValidRemoteRoot::parse(r"qingyu\team").unwrap().as_str(),
            "qingyu/team"
        );
    }

    #[test]
    fn overlong_notebook_names_are_not_available_on_the_current_platform() {
        assert!(notebook_name_available_on_current_platform("Notes"));
        assert!(!notebook_name_available_on_current_platform(
            &"x".repeat(256)
        ));
    }
}
