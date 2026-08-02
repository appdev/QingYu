//! Kernel-owned v3 sync configuration model and durable storage.

use std::{fmt, sync::Mutex};

use serde::{Deserialize, Serialize};
use zeroize::{Zeroize as _, Zeroizing};

use crate::{
    contract::{
        CredentialState, Nullable, RequestTimeoutSeconds, Revision, S3AddressingStyle,
        S3ConfigViewDto, S3TlsVerification, SafeEndpointViewDto, SyncConfigChangesDto,
        SyncConfigReadiness, SyncConfigViewDto, SyncIntervalSeconds, SyncIssueCode, SyncIssueDto,
        SyncMode, SyncProvider, WebDavConfigViewDto,
    },
    ports::{CredentialSecret, CredentialSlot},
    storage::{
        CommitState, DurableFileFailure, DurableFileFailureKind, DurableFileStore, ExpectedFile,
        FileRevision, PreservePrevious, RecoveryOutcome, ReplaceRequest, StorageFileName,
        StoredFile,
    },
};
use uuid::Uuid;

use super::credentials::{apply_credential_change, LegacyInlineCredentialStore};

pub const SYNC_CONFIG_VERSION: u32 = 3;
const MAX_SYNC_CONFIG_BYTES: u64 = 1024 * 1024;

#[cfg(test)]
thread_local! {
    static SENSITIVE_CONFIG_DROP_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn reset_sensitive_config_drop_count() {
    SENSITIVE_CONFIG_DROP_COUNT.with(|count| count.set(0));
}

#[cfg(test)]
fn record_sensitive_config_drop() {
    SENSITIVE_CONFIG_DROP_COUNT.with(|count| count.set(count.get().saturating_add(1)));
}

#[cfg(test)]
fn sensitive_config_drop_count() -> usize {
    SENSITIVE_CONFIG_DROP_COUNT.with(std::cell::Cell::get)
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SyncConfig {
    version: u32,
    enabled: bool,
    provider: SyncProvider,
    remote_root: String,
    mode: SyncMode,
    interval_seconds: u32,
    #[serde(default)]
    generate_conflict_document: bool,
    webdav: WebDavConfig,
    s3: S3Config,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct WebDavConfig {
    server_url: String,
    username: String,
    password: SensitiveConfigString,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct S3Config {
    endpoint_url: String,
    region: String,
    bucket: String,
    access_key_id: SensitiveConfigString,
    secret_access_key: SensitiveConfigString,
    request_timeout_seconds: u32,
    addressing_style: S3AddressingStyle,
    tls_verification: S3TlsVerification,
}

impl Default for S3Config {
    fn default() -> Self {
        Self {
            endpoint_url: String::new(),
            region: String::new(),
            bucket: String::new(),
            access_key_id: SensitiveConfigString::default(),
            secret_access_key: SensitiveConfigString::default(),
            request_timeout_seconds: 60,
            addressing_style: S3AddressingStyle::Auto,
            tls_verification: S3TlsVerification::Verify,
        }
    }
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(transparent)]
struct SensitiveConfigString(String);

impl SensitiveConfigString {
    #[cfg(test)]
    fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    fn as_str(&self) -> &str {
        &self.0
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn clone_exposed(&self) -> String {
        self.0.clone()
    }

    fn replace(&mut self, value: String) {
        self.0.zeroize();
        self.0 = value;
    }
}

impl fmt::Debug for SensitiveConfigString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SensitiveConfigString([REDACTED])")
    }
}

impl Drop for SensitiveConfigString {
    fn drop(&mut self) {
        self.0.zeroize();
        #[cfg(test)]
        record_sensitive_config_drop();
    }
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            version: SYNC_CONFIG_VERSION,
            enabled: false,
            provider: SyncProvider::S3,
            remote_root: "qingyu".to_string(),
            mode: SyncMode::Automatic,
            interval_seconds: 30,
            generate_conflict_document: false,
            webdav: WebDavConfig::default(),
            s3: S3Config::default(),
        }
    }
}

impl SyncConfig {
    pub fn to_view(
        &self,
        revision: Revision,
    ) -> Result<SyncConfigViewDto, SyncConfigValidationError> {
        if !remote_root_is_valid(&self.remote_root) {
            return Err(SyncConfigValidationError);
        }
        let interval_seconds = SyncIntervalSeconds::new(self.interval_seconds)
            .map_err(|_| SyncConfigValidationError)?;
        let request_timeout_seconds = u16::try_from(self.s3.request_timeout_seconds)
            .ok()
            .and_then(|value| RequestTimeoutSeconds::new(value).ok())
            .ok_or(SyncConfigValidationError)?;
        let issues = self.issues();
        let configured = self.configured();
        let readiness = if !self.enabled {
            SyncConfigReadiness::Disabled
        } else if issues.is_empty() {
            SyncConfigReadiness::Ready
        } else {
            SyncConfigReadiness::Incomplete
        };
        Ok(SyncConfigViewDto {
            revision,
            enabled: self.enabled,
            provider: self.provider,
            remote_root: self.remote_root.clone(),
            mode: self.mode,
            interval_seconds,
            generate_conflict_document: self.generate_conflict_document,
            configured,
            readiness,
            issues,
            webdav: WebDavConfigViewDto {
                server_url: endpoint_view(&self.webdav.server_url),
                username: self.webdav.username.clone(),
                password: CredentialState {
                    present: !self.webdav.password.is_empty(),
                },
            },
            s3: S3ConfigViewDto {
                endpoint_url: endpoint_view(&self.s3.endpoint_url),
                region: self.s3.region.clone(),
                bucket: self.s3.bucket.clone(),
                access_key_id: CredentialState {
                    present: !self.s3.access_key_id.is_empty(),
                },
                secret_access_key: CredentialState {
                    present: !self.s3.secret_access_key.is_empty(),
                },
                request_timeout_seconds,
                addressing_style: self.s3.addressing_style,
                tls_verification: self.s3.tls_verification,
            },
        })
    }

    pub const fn provider(&self) -> SyncProvider {
        self.provider
    }

    pub fn checked_target(&self) -> String {
        self.remote_root.clone()
    }

    #[allow(dead_code)] // Consumed by the staged production executor.
    pub(crate) fn into_execution_plan(
        mut self,
    ) -> Result<SyncExecutionPlan, SyncExecutionPlanError> {
        let view = self
            .to_view(Revision::parse("execution-plan").expect("static revision is valid"))
            .map_err(|_| SyncExecutionPlanError)?;
        if view.readiness != SyncConfigReadiness::Ready {
            return Err(SyncExecutionPlanError);
        }
        let target = match self.provider {
            SyncProvider::Webdav => SyncExecutionTarget::WebDav {
                server_url: std::mem::take(&mut self.webdav.server_url),
                username: std::mem::take(&mut self.webdav.username),
                password: CredentialSecret::new(std::mem::take(&mut self.webdav.password.0)),
            },
            SyncProvider::S3 => SyncExecutionTarget::S3 {
                endpoint_url: std::mem::take(&mut self.s3.endpoint_url),
                region: std::mem::take(&mut self.s3.region),
                bucket: std::mem::take(&mut self.s3.bucket),
                access_key_id: CredentialSecret::new(std::mem::take(&mut self.s3.access_key_id.0)),
                secret_access_key: CredentialSecret::new(std::mem::take(
                    &mut self.s3.secret_access_key.0,
                )),
                request_timeout_seconds: self.s3.request_timeout_seconds,
                addressing_style: self.s3.addressing_style,
                tls_verification: self.s3.tls_verification,
            },
        };
        Ok(SyncExecutionPlan {
            provider: self.provider,
            remote_root: std::mem::take(&mut self.remote_root),
            generate_conflict_document: self.generate_conflict_document,
            target,
        })
    }

    pub(crate) fn into_configured_s3_target(
        mut self,
    ) -> Result<SyncExecutionTarget, SyncExecutionPlanError> {
        if self.provider != SyncProvider::S3 {
            return Err(SyncExecutionPlanError);
        }
        self.enabled = true;
        let plan = self.into_execution_plan()?;
        match plan.target {
            target @ SyncExecutionTarget::S3 { .. } => Ok(target),
            SyncExecutionTarget::WebDav { .. } => Err(SyncExecutionPlanError),
        }
    }

    pub(crate) fn into_repository_recovery_config(
        mut self,
    ) -> Result<Self, SyncExecutionPlanError> {
        if self.provider != SyncProvider::S3 {
            return Err(SyncExecutionPlanError);
        }
        self.enabled = true;
        let readiness = self
            .to_view(Revision::parse("repository-recovery").expect("static revision is valid"))
            .map_err(|_| SyncExecutionPlanError)?
            .readiness;
        if readiness != SyncConfigReadiness::Ready {
            return Err(SyncExecutionPlanError);
        }
        Ok(self)
    }

    pub fn apply_changes(
        &mut self,
        changes: &SyncConfigChangesDto,
    ) -> Result<(), SyncConfigChangeError> {
        for endpoint in [
            changes.webdav_server_url.as_deref(),
            changes.s3_endpoint_url.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if matches!(endpoint_assessment(endpoint), EndpointAssessment::Unsafe) {
                return Err(SyncConfigChangeError::UnsafeEndpoint);
            }
        }
        if changes
            .remote_root
            .as_deref()
            .is_some_and(|value| !remote_root_is_valid(value))
        {
            return Err(SyncConfigChangeError::UnsafeRemoteRoot);
        }
        if let Some(value) = changes.enabled {
            self.enabled = value;
        }
        if let Some(value) = changes.provider {
            self.provider = value;
        }
        if let Some(value) = &changes.remote_root {
            self.remote_root.clone_from(value);
        }
        if let Some(value) = changes.mode {
            self.mode = value;
        }
        if let Some(value) = changes.interval_seconds {
            self.interval_seconds = value.get();
        }
        if let Some(value) = changes.generate_conflict_document {
            self.generate_conflict_document = value;
        }
        if let Some(value) = &changes.webdav_server_url {
            self.webdav.server_url = value.trim().to_string();
        }
        if let Some(value) = &changes.webdav_username {
            self.webdav.username = value.trim().to_string();
        }
        if let Some(value) = &changes.s3_endpoint_url {
            self.s3.endpoint_url = value.trim().to_string();
        }
        if let Some(value) = &changes.s3_region {
            self.s3.region = value.trim().to_string();
        }
        if let Some(value) = &changes.s3_bucket {
            self.s3.bucket = value.trim().to_string();
        }
        if let Some(value) = changes.s3_request_timeout_seconds {
            self.s3.request_timeout_seconds = u32::from(value.get());
        }
        if let Some(value) = changes.s3_addressing_style {
            self.s3.addressing_style = value;
        }
        if let Some(value) = changes.s3_tls_verification {
            self.s3.tls_verification = value;
        }

        let credentials = LegacyInlineCredentialStore::new(
            self.webdav.password.clone_exposed(),
            self.s3.access_key_id.clone_exposed(),
            self.s3.secret_access_key.clone_exposed(),
        );
        apply_credential_change(
            &credentials,
            CredentialSlot::WebDavPassword,
            changes.webdav_password.as_ref(),
        )
        .map_err(|_| SyncConfigChangeError::CredentialStoreUnavailable)?;
        apply_credential_change(
            &credentials,
            CredentialSlot::S3AccessKeyId,
            changes.s3_access_key_id.as_ref(),
        )
        .map_err(|_| SyncConfigChangeError::CredentialStoreUnavailable)?;
        apply_credential_change(
            &credentials,
            CredentialSlot::S3SecretAccessKey,
            changes.s3_secret_access_key.as_ref(),
        )
        .map_err(|_| SyncConfigChangeError::CredentialStoreUnavailable)?;
        let (webdav_password, s3_access_key_id, s3_secret_access_key) = credentials
            .snapshot()
            .map_err(|_| SyncConfigChangeError::CredentialStoreUnavailable)?
            .into_parts();
        self.webdav.password.replace(webdav_password);
        self.s3.access_key_id.replace(s3_access_key_id);
        self.s3.secret_access_key.replace(s3_secret_access_key);
        Ok(())
    }

    fn configured(&self) -> bool {
        let mut candidate = self.clone();
        candidate.enabled = true;
        candidate.issues().is_empty()
    }

    fn issues(&self) -> Vec<SyncIssueDto> {
        if !self.enabled {
            return Vec::new();
        }
        let mut issues = Vec::new();
        if !remote_root_is_valid(&self.remote_root) {
            issues.push(issue(
                "remoteRoot",
                SyncIssueCode::InvalidPath,
                "Remote root must be a safe relative path.",
            ));
        }
        match self.provider {
            SyncProvider::Webdav => {
                append_endpoint_issue(&mut issues, "webdav.serverUrl", &self.webdav.server_url);
            }
            SyncProvider::S3 => {
                append_endpoint_issue(&mut issues, "s3.endpointUrl", &self.s3.endpoint_url);
                for (field, value) in [
                    ("s3.bucket", self.s3.bucket.as_str()),
                    ("s3.accessKeyId", self.s3.access_key_id.as_str()),
                    ("s3.secretAccessKey", self.s3.secret_access_key.as_str()),
                ] {
                    if value.trim().is_empty() {
                        issues.push(issue(
                            field,
                            SyncIssueCode::Required,
                            "This field is required.",
                        ));
                    }
                }
                if !(5..=600).contains(&self.s3.request_timeout_seconds) {
                    issues.push(issue(
                        "s3.requestTimeoutSeconds",
                        SyncIssueCode::OutOfRange,
                        "Enter a value from 5 through 600.",
                    ));
                }
            }
        }
        issues
    }
}

#[allow(dead_code)] // Consumed by the staged production executor.
pub(crate) struct SyncExecutionPlan {
    pub(crate) provider: SyncProvider,
    pub(crate) remote_root: String,
    pub(crate) generate_conflict_document: bool,
    pub(crate) target: SyncExecutionTarget,
}

#[allow(dead_code)] // Consumed by the staged production executor.
pub(crate) enum SyncExecutionTarget {
    WebDav {
        server_url: String,
        username: String,
        password: CredentialSecret,
    },
    S3 {
        endpoint_url: String,
        region: String,
        bucket: String,
        access_key_id: CredentialSecret,
        secret_access_key: CredentialSecret,
        request_timeout_seconds: u32,
        addressing_style: S3AddressingStyle,
        tls_verification: S3TlsVerification,
    },
}

impl fmt::Debug for SyncExecutionPlan {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SyncExecutionPlan([REDACTED])")
    }
}

impl fmt::Debug for SyncExecutionTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SyncExecutionTarget([REDACTED])")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)] // Consumed by the staged production executor.
pub(crate) struct SyncExecutionPlanError;

impl fmt::Display for SyncExecutionPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("sync execution plan is unavailable")
    }
}

impl std::error::Error for SyncExecutionPlanError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncConfigChangeError {
    CredentialStoreUnavailable,
    UnsafeEndpoint,
    UnsafeRemoteRoot,
}

impl fmt::Display for SyncConfigChangeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("sync configuration changes are invalid")
    }
}

impl std::error::Error for SyncConfigChangeError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyncConfigValidationError;

impl fmt::Display for SyncConfigValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("sync configuration cannot be represented safely")
    }
}

impl std::error::Error for SyncConfigValidationError {}

impl fmt::Debug for SyncConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SyncConfig")
            .field("version", &self.version)
            .field("enabled", &self.enabled)
            .field("provider", &self.provider)
            .field("remote_root", &"[REDACTED]")
            .field("mode", &self.mode)
            .field("interval_seconds", &self.interval_seconds)
            .field(
                "generate_conflict_document",
                &self.generate_conflict_document,
            )
            .field("webdav", &"WebDavConfig([REDACTED])")
            .field("s3", &"S3Config([REDACTED])")
            .finish()
    }
}

pub struct SyncConfigStore {
    durable: DurableFileStore,
    state: Mutex<SyncConfigStoreState>,
    target: StorageFileName,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SyncConfigInitialization {
    Ready,
    ExistingInvalid,
}

struct SyncConfigStoreState {
    recovery_required: bool,
}

impl SyncConfigStore {
    pub fn new(durable: DurableFileStore) -> Result<Self, SyncConfigStoreError> {
        let recovery_required = match durable.recover() {
            Ok(outcomes) => outcomes.iter().any(|outcome| {
                matches!(
                    outcome,
                    RecoveryOutcome::Committed {
                        commit_state: CommitState::PublishedDurabilityUncertain,
                        ..
                    } | RecoveryOutcome::ManualInterventionRequired { .. }
                )
            }),
            Err(error)
                if matches!(
                    error.kind(),
                    DurableFileFailureKind::PublishStateUncertain
                        | DurableFileFailureKind::RecoveryRequired
                ) =>
            {
                true
            }
            Err(error) => return Err(SyncConfigStoreError::from(error)),
        };
        let target =
            StorageFileName::parse("sync-config.json").map_err(SyncConfigStoreError::from)?;
        Ok(Self {
            durable,
            state: Mutex::new(SyncConfigStoreState { recovery_required }),
            target,
        })
    }

    pub fn load(&self) -> Result<SyncConfigLoad, SyncConfigStoreError> {
        let state = self.lock_state()?;
        Self::ensure_available(&state)?;
        let Some(stored) = self
            .durable
            .read(&self.target, MAX_SYNC_CONFIG_BYTES)
            .map_err(SyncConfigStoreError::from)?
        else {
            return Ok(SyncConfigLoad::Absent);
        };
        classify(&stored)
    }

    /// Installs the disabled v3 default exactly once for a genuinely empty
    /// instance. Existing valid state is retained byte-for-byte, while corrupt
    /// or unsupported state remains visible for explicit recovery instead of
    /// being replaced during startup.
    pub(crate) fn initialize_default_if_absent(
        &self,
    ) -> Result<SyncConfigInitialization, SyncConfigStoreError> {
        let mut state = self.lock_state()?;
        Self::ensure_available(&state)?;
        if let Some(stored) = self
            .durable
            .read(&self.target, MAX_SYNC_CONFIG_BYTES)
            .map_err(SyncConfigStoreError::from)?
        {
            return match classify(&stored)? {
                SyncConfigLoad::Loaded { .. } => Ok(SyncConfigInitialization::Ready),
                SyncConfigLoad::Corrupt { .. } | SyncConfigLoad::Unsupported { .. } => {
                    Ok(SyncConfigInitialization::ExistingInvalid)
                }
                SyncConfigLoad::Absent => unreachable!("a retained file cannot classify absent"),
            };
        }

        let bytes = serialized_config(&SyncConfig::default())?;
        self.replace_durably(
            &mut state,
            &bytes,
            ReplaceRequest {
                target: &self.target,
                bytes: &bytes,
                expected: ExpectedFile::Absent,
                preserve_previous: PreservePrevious::None,
            },
        )?;
        Ok(SyncConfigInitialization::Ready)
    }

    pub fn is_absent(&self) -> Result<bool, SyncConfigStoreError> {
        self.load()
            .map(|load| matches!(load, SyncConfigLoad::Absent))
    }

    pub fn recover_invalid(
        &self,
        expected_revision: &Revision,
        replacement: SyncConfig,
    ) -> Result<Revision, SyncConfigStoreError> {
        let mut state = self.lock_state()?;
        Self::ensure_available(&state)?;
        let Some(stored) = self
            .durable
            .read(&self.target, MAX_SYNC_CONFIG_BYTES)
            .map_err(SyncConfigStoreError::from)?
        else {
            return Err(SyncConfigStoreError::new(
                SyncConfigStoreErrorKind::NotRecoverable,
            ));
        };
        let current_revision = public_revision(&stored.revision)?;
        if &current_revision != expected_revision {
            return Err(SyncConfigStoreError::new(
                SyncConfigStoreErrorKind::RevisionConflict,
            ));
        }
        if matches!(classify(&stored)?, SyncConfigLoad::Loaded { .. }) {
            return Err(SyncConfigStoreError::new(
                SyncConfigStoreErrorKind::NotRecoverable,
            ));
        }
        if replacement.version != SYNC_CONFIG_VERSION
            || replacement
                .to_view(Revision::parse("candidate").expect("static candidate revision"))
                .is_err()
        {
            return Err(SyncConfigStoreError::new(
                SyncConfigStoreErrorKind::InvalidDraft,
            ));
        }
        let bytes = serialized_config(&replacement)?;
        let recovery_name =
            StorageFileName::parse(format!("sync-config.damaged-{}.json", Uuid::new_v4()))
                .map_err(SyncConfigStoreError::from)?;
        let outcome = self.replace_durably(
            &mut state,
            &bytes,
            ReplaceRequest {
                target: &self.target,
                bytes: &bytes,
                expected: ExpectedFile::Revision(&stored.revision),
                preserve_previous: PreservePrevious::Required {
                    recovery_name: &recovery_name,
                },
            },
        )?;
        public_revision(&outcome.installed_revision)
    }

    pub fn replace(
        &self,
        expected_revision: &Revision,
        config: SyncConfig,
    ) -> Result<(SyncConfig, Revision), SyncConfigStoreError> {
        let mut state = self.lock_state()?;
        Self::ensure_available(&state)?;
        let Some(stored) = self
            .durable
            .read(&self.target, MAX_SYNC_CONFIG_BYTES)
            .map_err(SyncConfigStoreError::from)?
        else {
            return Err(SyncConfigStoreError::new(
                SyncConfigStoreErrorKind::NotRecoverable,
            ));
        };
        let current_revision = public_revision(&stored.revision)?;
        if &current_revision != expected_revision {
            return Err(SyncConfigStoreError::new(
                SyncConfigStoreErrorKind::RevisionConflict,
            ));
        }
        if !matches!(classify(&stored)?, SyncConfigLoad::Loaded { .. }) {
            return Err(SyncConfigStoreError::new(
                SyncConfigStoreErrorKind::NotRecoverable,
            ));
        }
        if config.version != SYNC_CONFIG_VERSION
            || config.to_view(expected_revision.clone()).is_err()
        {
            return Err(SyncConfigStoreError::new(
                SyncConfigStoreErrorKind::InvalidDraft,
            ));
        }
        let bytes = serialized_config(&config)?;
        let outcome = self.replace_durably(
            &mut state,
            &bytes,
            ReplaceRequest {
                target: &self.target,
                bytes: &bytes,
                expected: ExpectedFile::Revision(&stored.revision),
                preserve_previous: PreservePrevious::None,
            },
        )?;
        let revision = public_revision(&outcome.installed_revision)?;
        Ok((config, revision))
    }

    fn lock_state(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, SyncConfigStoreState>, SyncConfigStoreError> {
        self.state
            .lock()
            .map_err(|_| SyncConfigStoreError::new(SyncConfigStoreErrorKind::RecoveryRequired))
    }

    fn ensure_available(state: &SyncConfigStoreState) -> Result<(), SyncConfigStoreError> {
        if state.recovery_required {
            Err(SyncConfigStoreError::new(
                SyncConfigStoreErrorKind::RecoveryRequired,
            ))
        } else {
            Ok(())
        }
    }

    fn replace_durably(
        &self,
        state: &mut SyncConfigStoreState,
        candidate_bytes: &[u8],
        request: ReplaceRequest<'_>,
    ) -> Result<crate::storage::ReplaceOutcome, SyncConfigStoreError> {
        match self.durable.replace(request) {
            Ok(outcome) => {
                let verified =
                    self.verify_installed(candidate_bytes, Some(&outcome.installed_revision));
                if matches!(
                    outcome.commit_state,
                    CommitState::Durable | CommitState::AtomicVisibility
                ) && verified.is_ok()
                {
                    Ok(outcome)
                } else {
                    state.recovery_required = true;
                    Err(SyncConfigStoreError::new(
                        SyncConfigStoreErrorKind::RecoveryRequired,
                    ))
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    DurableFileFailureKind::PublishStateUncertain
                        | DurableFileFailureKind::RecoveryRequired
                ) =>
            {
                let _actual_state = self.verify_installed(candidate_bytes, None);
                state.recovery_required = true;
                Err(SyncConfigStoreError::new(
                    SyncConfigStoreErrorKind::RecoveryRequired,
                ))
            }
            Err(error) => Err(SyncConfigStoreError::from(error)),
        }
    }

    fn verify_installed(
        &self,
        candidate_bytes: &[u8],
        expected_revision: Option<&FileRevision>,
    ) -> Result<FileRevision, SyncConfigStoreError> {
        let stored = self
            .durable
            .read(&self.target, MAX_SYNC_CONFIG_BYTES)
            .map_err(SyncConfigStoreError::from)?
            .ok_or_else(|| SyncConfigStoreError::new(SyncConfigStoreErrorKind::RecoveryRequired))?;
        if stored.bytes.as_slice() != candidate_bytes
            || expected_revision.is_some_and(|revision| revision != &stored.revision)
            || !matches!(classify(&stored)?, SyncConfigLoad::Loaded { .. })
        {
            return Err(SyncConfigStoreError::new(
                SyncConfigStoreErrorKind::RecoveryRequired,
            ));
        }
        Ok(stored.revision.clone())
    }
}

pub enum SyncConfigLoad {
    Absent,
    Loaded {
        config: Box<SyncConfig>,
        revision: Revision,
    },
    Corrupt {
        revision: Revision,
    },
    Unsupported {
        revision: Revision,
        version: u64,
    },
}

impl fmt::Debug for SyncConfigLoad {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Absent => formatter.write_str("SyncConfigLoad::Absent"),
            Self::Loaded { revision, .. } => formatter
                .debug_struct("SyncConfigLoad::Loaded")
                .field("revision", revision)
                .finish_non_exhaustive(),
            Self::Corrupt { revision } => formatter
                .debug_struct("SyncConfigLoad::Corrupt")
                .field("revision", revision)
                .finish(),
            Self::Unsupported { revision, version } => formatter
                .debug_struct("SyncConfigLoad::Unsupported")
                .field("revision", revision)
                .field("version", version)
                .finish(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncConfigStoreErrorKind {
    InvalidDraft,
    NotRecoverable,
    RecoveryRequired,
    RevisionConflict,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyncConfigStoreError {
    kind: SyncConfigStoreErrorKind,
}

impl SyncConfigStoreError {
    const fn new(kind: SyncConfigStoreErrorKind) -> Self {
        Self { kind }
    }

    pub const fn kind(self) -> SyncConfigStoreErrorKind {
        self.kind
    }
}

impl From<DurableFileFailure> for SyncConfigStoreError {
    fn from(error: DurableFileFailure) -> Self {
        let kind = match error.kind() {
            DurableFileFailureKind::RevisionConflict => SyncConfigStoreErrorKind::RevisionConflict,
            DurableFileFailureKind::PublishStateUncertain
            | DurableFileFailureKind::RecoveryRequired => {
                SyncConfigStoreErrorKind::RecoveryRequired
            }
            _ => SyncConfigStoreErrorKind::Unavailable,
        };
        Self::new(kind)
    }
}

impl fmt::Display for SyncConfigStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("sync configuration storage is unavailable")
    }
}

impl std::error::Error for SyncConfigStoreError {}

fn public_revision(revision: &FileRevision) -> Result<Revision, SyncConfigStoreError> {
    let value = revision
        .as_str()
        .strip_prefix("sha256:")
        .ok_or_else(|| SyncConfigStoreError::new(SyncConfigStoreErrorKind::Unavailable))?;
    Revision::parse(value)
        .map_err(|_| SyncConfigStoreError::new(SyncConfigStoreErrorKind::Unavailable))
}

#[derive(Deserialize)]
struct SyncConfigVersionProbe {
    version: Option<u64>,
}

fn classify(stored: &StoredFile) -> Result<SyncConfigLoad, SyncConfigStoreError> {
    let revision = public_revision(&stored.revision)?;
    let probe: SyncConfigVersionProbe = match serde_json::from_slice(&stored.bytes) {
        Ok(probe) => probe,
        Err(_) => return Ok(SyncConfigLoad::Corrupt { revision }),
    };
    let Some(version) = probe.version else {
        return Ok(SyncConfigLoad::Corrupt { revision });
    };
    if version != u64::from(SYNC_CONFIG_VERSION) {
        return Ok(SyncConfigLoad::Unsupported { revision, version });
    }
    match serde_json::from_slice(&stored.bytes) {
        Ok(config) => Ok(SyncConfigLoad::Loaded {
            config: Box::new(config),
            revision,
        }),
        Err(_) => Ok(SyncConfigLoad::Corrupt { revision }),
    }
}

fn serialized_config(config: &SyncConfig) -> Result<Zeroizing<Vec<u8>>, SyncConfigStoreError> {
    let mut bytes = Zeroizing::new(Vec::new());
    let formatter = serde_json::ser::PrettyFormatter::new();
    let mut serializer = serde_json::Serializer::with_formatter(&mut *bytes, formatter);
    config
        .serialize(&mut serializer)
        .map_err(|_| SyncConfigStoreError::new(SyncConfigStoreErrorKind::InvalidDraft))?;
    bytes.push(b'\n');
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_SYNC_CONFIG_BYTES {
        return Err(SyncConfigStoreError::new(
            SyncConfigStoreErrorKind::InvalidDraft,
        ));
    }
    Ok(bytes)
}

fn endpoint_view(value: &str) -> SafeEndpointViewDto {
    match endpoint_assessment(value) {
        EndpointAssessment::Empty => SafeEndpointViewDto {
            value: Nullable::null(),
            redacted: false,
        },
        EndpointAssessment::Safe(value) => SafeEndpointViewDto {
            value: Nullable::value(value),
            redacted: false,
        },
        EndpointAssessment::Invalid | EndpointAssessment::Unsafe => SafeEndpointViewDto {
            value: Nullable::null(),
            redacted: true,
        },
    }
}

fn append_endpoint_issue(issues: &mut Vec<SyncIssueDto>, field: &str, value: &str) {
    match endpoint_assessment(value) {
        EndpointAssessment::Safe(_) => {}
        EndpointAssessment::Unsafe => issues.push(issue(
            field,
            SyncIssueCode::UnsafeUrlComponents,
            "Remove credentials, query parameters, and fragments from this URL.",
        )),
        EndpointAssessment::Empty | EndpointAssessment::Invalid => issues.push(issue(
            field,
            SyncIssueCode::InvalidUrl,
            "Enter a valid HTTP or HTTPS URL.",
        )),
    }
}

enum EndpointAssessment {
    Empty,
    Safe(String),
    Unsafe,
    Invalid,
}

fn endpoint_assessment(value: &str) -> EndpointAssessment {
    let value = value.trim();
    if value.is_empty() {
        return EndpointAssessment::Empty;
    }
    let Ok(parsed) = reqwest::Url::parse(value) else {
        return EndpointAssessment::Invalid;
    };
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return EndpointAssessment::Invalid;
    }
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return EndpointAssessment::Unsafe;
    }
    EndpointAssessment::Safe(parsed.to_string())
}

fn remote_root_is_valid(value: &str) -> bool {
    let bytes = value.as_bytes();
    !value.is_empty()
        && value.trim() == value
        && !value.starts_with('/')
        && !value.starts_with('\\')
        && !value.contains('\\')
        && !value.chars().any(char::is_control)
        && !(bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
        && value
            .split('/')
            .all(|segment| !segment.is_empty() && !matches!(segment, "." | ".."))
}

fn issue(field: &str, code: SyncIssueCode, message: &str) -> SyncIssueDto {
    SyncIssueDto {
        field: field.to_string(),
        code,
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::KernelConfig,
        contract::{ErrorCode, PatchSyncConfigRequest, SyncConfigChangesDto},
        paths::KernelPaths,
        ports::KernelPorts,
        runtime::{KernelRuntime, SyncApiService},
        services::sync::{SyncExecutionError, SyncExecutor, SyncService},
        storage::DurableFileTestFault,
    };
    use async_trait::async_trait;
    use std::{sync::Arc, time::Duration};
    use tempfile::{tempdir, TempDir};

    #[test]
    fn ready_config_moves_secrets_into_one_redacted_execution_plan() {
        let mut config = SyncConfig {
            enabled: true,
            ..SyncConfig::default()
        };
        config.s3.endpoint_url = "https://s3.example.test".to_owned();
        config.s3.region = "test-1".to_owned();
        config.s3.bucket = "notes".to_owned();
        config.s3.access_key_id = SensitiveConfigString::new("access-secret");
        config.s3.secret_access_key = SensitiveConfigString::new("key-secret");
        config.generate_conflict_document = true;

        let plan = config.into_execution_plan().unwrap();

        assert_eq!(plan.provider, SyncProvider::S3);
        assert_eq!(plan.remote_root, "qingyu");
        assert!(plan.generate_conflict_document);
        let SyncExecutionTarget::S3 {
            endpoint_url,
            region,
            bucket,
            access_key_id,
            secret_access_key,
            request_timeout_seconds,
            addressing_style,
            tls_verification,
        } = &plan.target
        else {
            panic!("expected S3 execution target");
        };
        assert_eq!(endpoint_url, "https://s3.example.test");
        assert_eq!(region, "test-1");
        assert_eq!(bucket, "notes");
        assert_eq!(access_key_id.expose_secret(), "access-secret");
        assert_eq!(secret_access_key.expose_secret(), "key-secret");
        assert_eq!(*request_timeout_seconds, 60);
        assert_eq!(*addressing_style, S3AddressingStyle::Auto);
        assert_eq!(*tls_verification, S3TlsVerification::Verify);
        let debug = format!("{plan:?} {:?}", plan.target);
        assert_eq!(
            debug,
            "SyncExecutionPlan([REDACTED]) SyncExecutionTarget([REDACTED])"
        );
        assert!(!debug.contains("secret"));
        assert!(!debug.contains("s3.example"));
    }

    #[test]
    fn disabled_or_incomplete_config_cannot_create_an_execution_plan() {
        assert!(SyncConfig::default().into_execution_plan().is_err());

        let incomplete = SyncConfig {
            enabled: true,
            ..SyncConfig::default()
        };
        assert!(incomplete.into_execution_plan().is_err());
    }

    #[test]
    fn webdav_execution_plan_retains_credentials_only_in_the_redacted_target() {
        let mut config = SyncConfig {
            enabled: true,
            provider: SyncProvider::Webdav,
            ..SyncConfig::default()
        };
        config.webdav.server_url = "https://dav.example.test".to_owned();
        config.webdav.username = "alice".to_owned();
        config.webdav.password = SensitiveConfigString::new("webdav-secret");

        let plan = config.into_execution_plan().unwrap();
        let SyncExecutionTarget::WebDav {
            server_url,
            username,
            password,
        } = &plan.target
        else {
            panic!("expected WebDAV execution target");
        };

        assert_eq!(server_url, "https://dav.example.test");
        assert_eq!(username, "alice");
        assert_eq!(password.expose_secret(), "webdav-secret");
        assert_eq!(
            format!("{:?}", plan.target),
            "SyncExecutionTarget([REDACTED])"
        );
    }

    #[test]
    fn serialized_sync_config_bytes_are_zeroizing_from_the_api_boundary() {
        let mut config = SyncConfig::default();
        config.webdav.password = SensitiveConfigString::new("webdav-secret");
        config.s3.access_key_id = SensitiveConfigString::new("access-secret");
        config.s3.secret_access_key = SensitiveConfigString::new("key-secret");

        let bytes: zeroize::Zeroizing<Vec<u8>> = serialized_config(&config).unwrap();

        assert!(bytes.windows(13).any(|window| window == b"webdav-secret"));
        assert!(bytes.windows(13).any(|window| window == b"access-secret"));
        assert!(bytes.windows(10).any(|window| window == b"key-secret"));
    }

    #[test]
    fn classify_borrows_one_sensitive_stored_buffer_without_consuming_it() {
        let bytes = serde_json::to_vec_pretty(&SyncConfig::default()).unwrap();
        let stored = StoredFile {
            revision: serde_json::from_str(
                "\"sha256:0000000000000000000000000000000000000000000000000000000000000000\"",
            )
            .unwrap(),
            bytes,
        };

        let classified = classify(&stored).unwrap();

        assert!(matches!(classified, SyncConfigLoad::Loaded { .. }));
        assert!(!stored.bytes.is_empty());
    }

    #[test]
    fn credentials_parsed_before_a_later_config_error_are_zeroized_on_drop() {
        reset_sensitive_config_drop_count();
        let malformed = serde_json::json!({
            "version": 3,
            "enabled": false,
            "provider": "s3",
            "remoteRoot": "qingyu",
            "mode": "automatic",
            "intervalSeconds": 30,
            "generateConflictDocument": false,
            "webdav": {
                "serverUrl": "https://dav.example.test",
                "username": "alice",
                "password": "must-zeroize-after-later-error"
            },
            "s3": {
                "endpointUrl": "https://s3.example.test",
                "region": "us-east-1",
                "bucket": "notes",
                "accessKeyId": "access-key",
                "secretAccessKey": "secret-key",
                "requestTimeoutSeconds": "not-an-integer",
                "addressingStyle": "auto",
                "tlsVerification": "verify"
            }
        });

        let error = serde_json::from_value::<SyncConfig>(malformed).unwrap_err();

        assert!(error.is_data());
        assert!(sensitive_config_drop_count() >= 1);
    }

    #[test]
    fn uncertain_parent_sync_latches_store_and_never_reports_patch_success() {
        let (_temporary, store, current_revision) =
            faulted_sync_store(DurableFileTestFault::ParentSyncFailure);
        let candidate = SyncConfig {
            remote_root: "candidate-root".to_string(),
            ..SyncConfig::default()
        };

        let result = store.replace(&current_revision, candidate);

        assert_eq!(
            result.unwrap_err().kind(),
            SyncConfigStoreErrorKind::RecoveryRequired
        );
        assert_eq!(
            store.load().unwrap_err().kind(),
            SyncConfigStoreErrorKind::RecoveryRequired
        );
    }

    #[test]
    fn platform_atomic_visibility_is_verified_and_does_not_latch_the_sync_store() {
        let (_temporary, store, current_revision) =
            faulted_sync_store(DurableFileTestFault::PlatformDirectorySyncUncertain);
        let candidate = SyncConfig {
            remote_root: "candidate-root".to_string(),
            ..SyncConfig::default()
        };

        let (_, installed_revision) = store
            .replace(&current_revision, candidate)
            .expect("atomic visibility is a completed platform publication");
        let loaded = store.load().expect("future reads stay available");

        assert!(matches!(
            loaded,
            SyncConfigLoad::Loaded {
                config,
                revision
            } if config.remote_root == "candidate-root" && revision == installed_revision
        ));
    }

    #[test]
    fn uncertain_prepublication_intent_latches_store_even_when_old_target_remains() {
        let (_temporary, store, current_revision) =
            faulted_sync_store(DurableFileTestFault::LeavePrepared);
        let candidate = SyncConfig {
            remote_root: "candidate-root".to_string(),
            ..SyncConfig::default()
        };

        let result = store.replace(&current_revision, candidate);

        assert_eq!(
            result.unwrap_err().kind(),
            SyncConfigStoreErrorKind::RecoveryRequired
        );
        assert_eq!(
            store.load().unwrap_err().kind(),
            SyncConfigStoreErrorKind::RecoveryRequired
        );
    }

    #[test]
    fn finalization_uncertainty_latches_store_after_candidate_is_visible() {
        let (_temporary, store, current_revision) =
            faulted_sync_store(DurableFileTestFault::FinalizeFailure);
        let candidate = SyncConfig {
            remote_root: "candidate-root".to_string(),
            ..SyncConfig::default()
        };

        let error = store.replace(&current_revision, candidate).unwrap_err();

        assert_eq!(error.kind(), SyncConfigStoreErrorKind::RecoveryRequired);
        assert_eq!(
            store.load().unwrap_err().kind(),
            SyncConfigStoreErrorKind::RecoveryRequired
        );
    }

    #[test]
    fn publish_report_failure_returns_verified_installed_candidate() {
        let (_temporary, store, current_revision) =
            faulted_sync_store(DurableFileTestFault::AfterPublishReportsFailure);
        let candidate = SyncConfig {
            remote_root: "verified-root".to_string(),
            ..SyncConfig::default()
        };

        let (_, installed_revision) = store.replace(&current_revision, candidate).unwrap();
        let loaded = store.load().unwrap();

        let SyncConfigLoad::Loaded { config, revision } = loaded else {
            panic!("verified candidate must remain loaded");
        };
        assert_eq!(revision, installed_revision);
        assert_eq!(config.remote_root, "verified-root");
    }

    #[cfg(unix)]
    #[test]
    fn manual_recovery_outcome_constructs_a_latched_fail_closed_store() {
        use std::os::unix::fs::symlink;

        let temporary = tempdir().unwrap();
        let workspace = temporary.path().join("workspace");
        let app_data = temporary.path().join("app-data");
        let cache = temporary.path().join("cache");
        std::fs::create_dir(&workspace).unwrap();
        std::fs::create_dir(&app_data).unwrap();
        std::fs::create_dir(&cache).unwrap();
        let transaction = uuid::Uuid::new_v4();
        symlink(
            "attacker-controlled-target",
            app_data.join(format!(".qingyu-storage-{transaction}.stage")),
        )
        .unwrap();
        let kernel_config = KernelConfig::generate().unwrap();
        let paths = KernelPaths::desktop(&workspace, &app_data, &cache).unwrap();
        let durable =
            DurableFileStore::at_config(paths.config_root(), kernel_config.launch_epoch()).unwrap();

        let store = SyncConfigStore::new(durable).unwrap();

        assert_eq!(
            store.load().unwrap_err().kind(),
            SyncConfigStoreErrorKind::RecoveryRequired
        );
    }

    #[test]
    fn same_launch_recovery_of_uncertain_commit_keeps_rebuilt_store_latched() {
        let temporary = tempdir().unwrap();
        let workspace = temporary.path().join("workspace");
        let app_data = temporary.path().join("app-data");
        let cache = temporary.path().join("cache");
        std::fs::create_dir(&workspace).unwrap();
        std::fs::create_dir(&app_data).unwrap();
        std::fs::create_dir(&cache).unwrap();
        let original = serialized_config(&SyncConfig::default()).unwrap();
        std::fs::write(app_data.join("sync-config.json"), &*original).unwrap();
        let kernel_config = KernelConfig::generate().unwrap();
        let launch_epoch = *kernel_config.launch_epoch();
        let paths = KernelPaths::desktop(&workspace, &app_data, &cache).unwrap();
        let faulted = DurableFileStore::at_config_with_test_fault(
            paths.config_root(),
            &launch_epoch,
            DurableFileTestFault::FinalizeFailure,
        )
        .unwrap();
        let store = SyncConfigStore::new(faulted).unwrap();
        let revision = match store.load().unwrap() {
            SyncConfigLoad::Loaded { revision, .. } => revision,
            other => panic!("expected loaded config, got {other:?}"),
        };
        let candidate = SyncConfig {
            remote_root: "uncertain-root".to_string(),
            ..SyncConfig::default()
        };
        assert_eq!(
            store.replace(&revision, candidate).unwrap_err().kind(),
            SyncConfigStoreErrorKind::RecoveryRequired
        );
        drop(store);
        let recovering = DurableFileStore::at_config(paths.config_root(), &launch_epoch).unwrap();

        let rebuilt = SyncConfigStore::new(recovering).unwrap();

        assert_eq!(
            rebuilt.load().unwrap_err().kind(),
            SyncConfigStoreErrorKind::RecoveryRequired
        );
    }

    #[tokio::test]
    async fn uncertain_published_patch_emits_no_success_event_and_service_fails_closed() {
        let temporary = tempdir().unwrap();
        let workspace = temporary.path().join("workspace");
        let app_data = temporary.path().join("app-data");
        let cache = temporary.path().join("cache");
        std::fs::create_dir(&workspace).unwrap();
        std::fs::create_dir(&app_data).unwrap();
        std::fs::create_dir(&cache).unwrap();
        let original = serialized_config(&SyncConfig::default()).unwrap();
        std::fs::write(app_data.join("sync-config.json"), &*original).unwrap();
        let kernel_config = KernelConfig::generate().unwrap();
        let paths = KernelPaths::desktop(&workspace, &app_data, &cache).unwrap();
        let durable = DurableFileStore::at_config_with_test_fault(
            paths.config_root(),
            kernel_config.launch_epoch(),
            DurableFileTestFault::ParentSyncFailure,
        )
        .unwrap();
        let runtime =
            KernelRuntime::activate(kernel_config, paths, KernelPorts::unavailable()).unwrap();
        let mut events = runtime.event_broker().subscribe();
        let service = SyncService::new(
            runtime.clone(),
            Arc::new(SyncConfigStore::new(durable).unwrap()),
            Arc::new(UnitExecutor),
        );
        let before = SyncApiService::get_sync_config(&service).await.unwrap();

        let error = SyncApiService::patch_sync_config(
            &service,
            PatchSyncConfigRequest {
                expected_revision: before.revision,
                changes: SyncConfigChangesDto {
                    remote_root: Some("uncertain-root".to_string()),
                    ..SyncConfigChangesDto::default()
                },
            },
        )
        .await
        .unwrap_err();

        assert_eq!(error.code(), ErrorCode::SyncConfigInvalid);
        assert_eq!(
            SyncApiService::get_sync_config(&service)
                .await
                .unwrap_err()
                .code(),
            ErrorCode::SyncConfigInvalid
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(10), events.recv())
                .await
                .is_err()
        );
    }

    fn faulted_sync_store(fault: DurableFileTestFault) -> (TempDir, SyncConfigStore, Revision) {
        let temporary = tempdir().unwrap();
        let workspace = temporary.path().join("workspace");
        let app_data = temporary.path().join("app-data");
        let cache = temporary.path().join("cache");
        std::fs::create_dir(&workspace).unwrap();
        std::fs::create_dir(&app_data).unwrap();
        std::fs::create_dir(&cache).unwrap();
        let original = serialized_config(&SyncConfig::default()).unwrap();
        std::fs::write(app_data.join("sync-config.json"), &*original).unwrap();
        let kernel_config = KernelConfig::generate().unwrap();
        let paths = KernelPaths::desktop(&workspace, &app_data, &cache).unwrap();
        let durable = DurableFileStore::at_config_with_test_fault(
            paths.config_root(),
            kernel_config.launch_epoch(),
            fault,
        )
        .unwrap();
        let store = SyncConfigStore::new(durable).unwrap();
        let revision = match store.load().unwrap() {
            SyncConfigLoad::Loaded { revision, .. } => revision,
            other => panic!("expected loaded config, got {other:?}"),
        };
        (temporary, store, revision)
    }

    struct UnitExecutor;

    #[async_trait]
    impl SyncExecutor for UnitExecutor {
        async fn test_connection(&self, _config: SyncConfig) -> Result<(), SyncExecutionError> {
            Ok(())
        }

        async fn run(
            &self,
            _config: SyncConfig,
            _context: crate::services::sync::SyncRunContext,
        ) -> Result<crate::contract::SyncSummaryDto, SyncExecutionError> {
            Ok(crate::contract::SyncSummaryDto::empty())
        }
    }
}
