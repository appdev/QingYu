use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::sync_validation::{
    normalize_interval, normalize_non_secret, normalize_remote_root, validate_http_url,
    validate_remote_root, validate_required, SyncValueIssue,
};

pub(crate) const SYNC_CONFIG_VERSION: u32 = 2;
pub(crate) const MIN_S3_REQUEST_TIMEOUT_SECONDS: u32 = 5;
pub(crate) const MAX_S3_REQUEST_TIMEOUT_SECONDS: u32 = 600;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct SyncConfig {
    pub(crate) version: u32,
    pub(crate) enabled: bool,
    pub(crate) provider: SyncProvider,
    pub(crate) remote_root: String,
    pub(crate) auto_sync_on_save: bool,
    pub(crate) interval_minutes: u32,
    pub(crate) webdav: WebDavConfig,
    pub(crate) s3: S3Config,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum SyncProvider {
    S3,
    Webdav,
}

#[derive(Clone, Deserialize, Default, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct WebDavConfig {
    pub(crate) server_url: String,
    pub(crate) username: String,
    pub(crate) password: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum S3AddressingStyle {
    Auto,
    Path,
    VirtualHosted,
}

impl Default for S3AddressingStyle {
    fn default() -> Self {
        Self::Auto
    }
}

impl S3AddressingStyle {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Path => "path",
            Self::VirtualHosted => "virtual-hosted",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum S3TlsVerification {
    Verify,
    Skip,
}

impl Default for S3TlsVerification {
    fn default() -> Self {
        Self::Verify
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct S3Config {
    pub(crate) endpoint_url: String,
    pub(crate) region: String,
    pub(crate) bucket: String,
    pub(crate) access_key_id: String,
    pub(crate) secret_access_key: String,
    pub(crate) request_timeout_seconds: u32,
    pub(crate) addressing_style: S3AddressingStyle,
    pub(crate) tls_verification: S3TlsVerification,
}

impl Default for S3Config {
    fn default() -> Self {
        Self {
            endpoint_url: String::new(),
            region: "us-east-1".into(),
            bucket: String::new(),
            access_key_id: String::new(),
            secret_access_key: String::new(),
            request_timeout_seconds: 60,
            addressing_style: S3AddressingStyle::Auto,
            tls_verification: S3TlsVerification::Verify,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SyncConfigReadiness {
    Disabled,
    Incomplete,
    Ready,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncConfigIssue {
    pub(crate) code: String,
    pub(crate) field: String,
    pub(crate) message: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncConfigLoadIssue {
    pub(crate) code: String,
    pub(crate) message: String,
}

impl SyncConfigLoadIssue {
    pub(crate) fn malformed() -> Self {
        Self {
            code: "sync-config-malformed".into(),
            message: "The sync configuration is malformed. Reset or recover it to continue.".into(),
        }
    }

    pub(crate) fn unsupported() -> Self {
        Self {
            code: "sync-config-unsupported".into(),
            message:
                "The sync configuration version is unsupported. Reset or recover it to continue."
                    .into(),
        }
    }

    pub(crate) fn oversized_too_large() -> Self {
        Self {
            code: "sync-config-oversized-too-large".into(),
            message:
                "The sync configuration is too large to inspect. Reset or recover it to continue."
                    .into(),
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncConfigDocument {
    pub(crate) config: SyncConfig,
    pub(crate) configured: bool,
    pub(crate) issues: Vec<SyncConfigIssue>,
    pub(crate) readiness: SyncConfigReadiness,
    pub(crate) revision: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "lowercase", tag = "status")]
pub(crate) enum SyncConfigLoadResponse {
    Absent {
        revision: Option<String>,
    },
    Loaded {
        #[serde(flatten)]
        document: SyncConfigDocument,
    },
    Malformed {
        issue: SyncConfigLoadIssue,
        revision: String,
    },
    Unsupported {
        issue: SyncConfigLoadIssue,
        revision: String,
        version: u64,
    },
}

#[derive(Clone, Deserialize)]
#[serde(tag = "field", content = "value")]
pub(crate) enum SyncConfigPatch {
    #[serde(rename = "enabled")]
    Enabled(bool),
    #[serde(rename = "provider")]
    Provider(SyncProvider),
    #[serde(rename = "remoteRoot")]
    RemoteRoot(String),
    #[serde(rename = "autoSyncOnSave")]
    AutoSyncOnSave(bool),
    #[serde(rename = "intervalMinutes")]
    IntervalMinutes(u32),
    #[serde(rename = "webdav.serverUrl")]
    WebDavServerUrl(String),
    #[serde(rename = "webdav.username")]
    WebDavUsername(String),
    #[serde(rename = "webdav.password")]
    WebDavPassword(String),
    #[serde(rename = "s3.endpointUrl")]
    S3EndpointUrl(String),
    #[serde(rename = "s3.region")]
    S3Region(String),
    #[serde(rename = "s3.bucket")]
    S3Bucket(String),
    #[serde(rename = "s3.accessKeyId")]
    S3AccessKeyId(String),
    #[serde(rename = "s3.secretAccessKey")]
    S3SecretAccessKey(String),
    #[serde(rename = "s3.requestTimeoutSeconds")]
    S3RequestTimeoutSeconds(u32),
    #[serde(rename = "s3.addressingStyle")]
    S3AddressingStyle(S3AddressingStyle),
    #[serde(rename = "s3.tlsVerification")]
    S3TlsVerification(S3TlsVerification),
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct SyncConfigPatchRequest {
    pub(crate) expected_revision: String,
    pub(crate) patch: SyncConfigPatch,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct RecoverSyncConfigRequest {
    pub(crate) config: SyncConfig,
    pub(crate) expected_revision: String,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct ResetSyncConfigRequest {
    pub(crate) confirmed: bool,
    pub(crate) expected_revision: Option<String>,
}

#[derive(Clone)]
pub(crate) struct SyncSnapshot {
    pub(crate) config: SyncConfig,
    pub(crate) revision: String,
    pub(crate) state_root: PathBuf,
    pub(crate) target: SyncTarget,
}

#[derive(Clone)]
pub(crate) enum SyncTarget {
    Webdav {
        remote_root: String,
        server_url: String,
        username: String,
        password: String,
    },
    S3 {
        access_key_id: String,
        bucket: String,
        endpoint_url: String,
        region: String,
        remote_root: String,
        secret_access_key: String,
        request_timeout_seconds: u32,
        addressing_style: S3AddressingStyle,
        tls_verification: S3TlsVerification,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncConnectionTestResult {
    pub(crate) checked_target: String,
    pub(crate) provider: SyncProvider,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            version: SYNC_CONFIG_VERSION,
            enabled: false,
            provider: SyncProvider::S3,
            remote_root: "qingyu".into(),
            auto_sync_on_save: true,
            interval_minutes: 5,
            webdav: WebDavConfig::default(),
            s3: S3Config::default(),
        }
    }
}

impl SyncConfig {
    pub(crate) fn configured(&self) -> bool {
        let mut validation_config = self.clone();
        validation_config.enabled = true;
        validation_config.issues().is_empty()
    }

    pub(crate) fn normalize(&mut self) {
        self.interval_minutes = normalize_interval(self.interval_minutes);
        if let Ok(remote_root) = normalize_remote_root(&self.remote_root) {
            self.remote_root = remote_root;
        } else {
            normalize_non_secret(&mut self.remote_root);
        }
        normalize_non_secret(&mut self.webdav.server_url);
        normalize_non_secret(&mut self.webdav.username);
        normalize_non_secret(&mut self.s3.endpoint_url);
        normalize_non_secret(&mut self.s3.region);
        normalize_non_secret(&mut self.s3.bucket);
        normalize_non_secret(&mut self.s3.access_key_id);
    }

    pub(crate) fn readiness(&self) -> SyncConfigReadiness {
        if !self.enabled {
            SyncConfigReadiness::Disabled
        } else if self.issues().is_empty() {
            SyncConfigReadiness::Ready
        } else {
            SyncConfigReadiness::Incomplete
        }
    }

    pub(crate) fn issues(&self) -> Vec<SyncConfigIssue> {
        if !self.enabled {
            return Vec::new();
        }
        let mut issues = Vec::new();
        if let Some(issue) = validate_remote_root(&self.remote_root) {
            issues.push(sync_issue("remoteRoot", issue));
        }
        match self.provider {
            SyncProvider::Webdav => {
                if let Some(issue) = validate_http_url(&self.webdav.server_url) {
                    issues.push(sync_issue("webdav.serverUrl", issue));
                }
            }
            SyncProvider::S3 => {
                if let Some(issue) = validate_http_url(&self.s3.endpoint_url) {
                    issues.push(sync_issue("s3.endpointUrl", issue));
                }
                for (field, value) in [
                    ("s3.region", self.s3.region.as_str()),
                    ("s3.bucket", self.s3.bucket.as_str()),
                    ("s3.accessKeyId", self.s3.access_key_id.as_str()),
                ] {
                    if let Some(issue) = validate_required(value) {
                        issues.push(sync_issue(field, issue));
                    }
                }
                if self.s3.secret_access_key.is_empty() {
                    issues.push(sync_issue("s3.secretAccessKey", SyncValueIssue::Required));
                }
                if !(MIN_S3_REQUEST_TIMEOUT_SECONDS..=MAX_S3_REQUEST_TIMEOUT_SECONDS)
                    .contains(&self.s3.request_timeout_seconds)
                {
                    issues.push(sync_issue(
                        "s3.requestTimeoutSeconds",
                        SyncValueIssue::OutOfRange,
                    ));
                }
            }
        }
        issues
    }

    pub(crate) fn apply_patch(&mut self, patch: SyncConfigPatch) {
        match patch {
            SyncConfigPatch::Enabled(value) => self.enabled = value,
            SyncConfigPatch::Provider(value) => self.provider = value,
            SyncConfigPatch::RemoteRoot(value) => self.remote_root = value,
            SyncConfigPatch::AutoSyncOnSave(value) => self.auto_sync_on_save = value,
            SyncConfigPatch::IntervalMinutes(value) => self.interval_minutes = value,
            SyncConfigPatch::WebDavServerUrl(value) => self.webdav.server_url = value,
            SyncConfigPatch::WebDavUsername(value) => self.webdav.username = value,
            SyncConfigPatch::WebDavPassword(value) => self.webdav.password = value,
            SyncConfigPatch::S3EndpointUrl(value) => self.s3.endpoint_url = value,
            SyncConfigPatch::S3Region(value) => self.s3.region = value,
            SyncConfigPatch::S3Bucket(value) => self.s3.bucket = value,
            SyncConfigPatch::S3AccessKeyId(value) => self.s3.access_key_id = value,
            SyncConfigPatch::S3SecretAccessKey(value) => self.s3.secret_access_key = value,
            SyncConfigPatch::S3RequestTimeoutSeconds(value) => {
                self.s3.request_timeout_seconds = value
            }
            SyncConfigPatch::S3AddressingStyle(value) => self.s3.addressing_style = value,
            SyncConfigPatch::S3TlsVerification(value) => self.s3.tls_verification = value,
        }
    }
}

fn sync_issue(field: &str, issue: SyncValueIssue) -> SyncConfigIssue {
    let (code, message) = match issue {
        SyncValueIssue::Required => ("required", "This field is required."),
        SyncValueIssue::InvalidUrl => ("invalid-url", "Enter a valid HTTP or HTTPS URL."),
        SyncValueIssue::OutOfRange => ("out-of-range", "Enter a value from 5 through 600."),
        SyncValueIssue::InvalidPath => {
            ("invalid-path", "Remote root must be a safe relative path.")
        }
    };
    SyncConfigIssue {
        code: code.into(),
        field: field.into(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_shape_is_flat_disabled_and_versioned() {
        let value = serde_json::to_value(SyncConfig::default()).unwrap();
        assert_eq!(value["version"], 2);
        assert_eq!(value["enabled"], false);
        assert_eq!(value["provider"], "s3");
        assert_eq!(value["remoteRoot"], "qingyu");
        assert_eq!(value["autoSyncOnSave"], true);
        assert_eq!(value["intervalMinutes"], 5);
        assert_eq!(value["s3"]["endpointUrl"], "");
        assert_eq!(value["s3"]["region"], "us-east-1");
        assert_eq!(value["s3"]["bucket"], "");
        assert_eq!(value["s3"]["accessKeyId"], "");
        assert_eq!(value["s3"]["secretAccessKey"], "");
        assert_eq!(value["s3"]["requestTimeoutSeconds"], 60);
        assert_eq!(value["s3"]["addressingStyle"], "auto");
        assert_eq!(value["s3"]["tlsVerification"], "verify");
        assert!(value.get("sync").is_none());
        assert!(value.get("projectRoot").is_none());
    }

    #[test]
    fn normalizes_non_secrets_but_preserves_secret_bytes() {
        let mut config = SyncConfig {
            enabled: true,
            provider: SyncProvider::Webdav,
            ..SyncConfig::default()
        };
        config.remote_root = " team/notes ".into();
        config.interval_minutes = 2_000;
        config.webdav.server_url = " https://dav.example.test ".into();
        config.webdav.password = " secret ".into();
        config.normalize();
        assert_eq!(config.remote_root, "team/notes");
        assert_eq!(config.interval_minutes, 1_440);
        assert_eq!(config.webdav.password, " secret ");
        assert_eq!(config.readiness(), SyncConfigReadiness::Ready);
    }

    #[test]
    fn issue_debug_never_includes_field_values() {
        let mut config = SyncConfig {
            enabled: true,
            ..SyncConfig::default()
        };
        config.remote_root = "../private-root".into();
        config.s3.secret_access_key = "private-secret".into();
        let debug = format!("{:?}", config.issues());
        assert!(!debug.contains("private-root"));
        assert!(!debug.contains("private-secret"));
    }

    #[test]
    fn s3_request_timeout_must_be_between_five_and_six_hundred_seconds() {
        for timeout in [4, 601] {
            let mut config = complete_s3_config();
            config.s3.request_timeout_seconds = timeout;

            assert!(config.issues().iter().any(|issue| {
                issue.field == "s3.requestTimeoutSeconds" && issue.code == "out-of-range"
            }));
        }

        let mut config = complete_s3_config();
        config.s3.request_timeout_seconds = 5;
        assert!(config.issues().is_empty());
        config.s3.request_timeout_seconds = 600;
        assert!(config.issues().is_empty());
    }

    fn complete_s3_config() -> SyncConfig {
        let mut config = SyncConfig {
            enabled: true,
            provider: SyncProvider::S3,
            ..SyncConfig::default()
        };
        config.s3.endpoint_url = "https://s3.example.test".into();
        config.s3.region = "us-east-1".into();
        config.s3.bucket = "notes".into();
        config.s3.access_key_id = "access".into();
        config.s3.secret_access_key = "secret".into();
        config
    }

    #[test]
    fn rejects_project_identity_fields_in_the_app_config() {
        let mut value = serde_json::to_value(SyncConfig::default()).unwrap();
        value["projectRoot"] = serde_json::json!("/Notes");
        assert!(serde_json::from_value::<SyncConfig>(value).is_err());
    }
}
