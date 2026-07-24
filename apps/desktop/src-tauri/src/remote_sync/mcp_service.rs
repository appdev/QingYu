use std::{
    collections::{HashMap, VecDeque},
    future::Future,
    path::PathBuf,
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};

use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tauri::Manager;
use tokio::sync::Notify;
use uuid::Uuid;

use crate::{
    mcp::config::SyncExecutionPolicy,
    sync_config::{
        emit_sync_config_changed,
        model::{
            SyncConfigDocument, SyncConfigIssue, SyncConfigLoadResponse, SyncConfigPatch,
            SyncConfigReadiness, SyncConnectionTestResult, SyncProvider,
        },
        ready_snapshot_at_app_data,
        status::{
            SyncCompletionState, SyncRunResult, SyncSafeError, SyncStatus, SyncSummary, SyncTrigger,
        },
        storage::{load_from_app_data, patch_batch_at_app_data},
    },
};

const RUN_RETENTION_LIMIT: usize = 500;

#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct SyncConfigPatchInput {
    pub(crate) expected_revision: String,
    pub(crate) enabled: Option<bool>,
    #[schemars(with = "Option<String>")]
    pub(crate) provider: Option<SyncProvider>,
    pub(crate) remote_root: Option<String>,
    pub(crate) auto_sync_on_save: Option<bool>,
    pub(crate) interval_minutes: Option<u32>,
    pub(crate) webdav_server_url: Option<String>,
    pub(crate) s3_endpoint_url: Option<String>,
    pub(crate) s3_region: Option<String>,
    pub(crate) s3_bucket: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct SyncCredentialPatchInput {
    pub(crate) expected_revision: String,
    pub(crate) webdav_username: Option<String>,
    pub(crate) webdav_password: Option<String>,
    pub(crate) s3_access_key_id: Option<String>,
    pub(crate) s3_secret_access_key: Option<String>,
    pub(crate) clear_credentials: Option<bool>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SanitizedSyncConfig {
    pub(crate) revision: String,
    pub(crate) enabled: bool,
    pub(crate) provider: SyncProvider,
    pub(crate) remote_root: String,
    pub(crate) auto_sync_on_save: bool,
    pub(crate) interval_minutes: u32,
    pub(crate) webdav_server_url: String,
    pub(crate) s3_endpoint_url: String,
    pub(crate) s3_region: String,
    pub(crate) s3_bucket: String,
    pub(crate) webdav_credentials_configured: bool,
    pub(crate) s3_credentials_configured: bool,
    pub(crate) readiness: SyncConfigReadiness,
    pub(crate) issues: Vec<SyncConfigIssue>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SyncServiceError {
    pub(crate) code: &'static str,
    message: &'static str,
}

impl SyncServiceError {
    fn workspace() -> Self {
        Self {
            code: "workspace_not_authorized",
            message: "The primary sync workspace is unavailable.",
        }
    }

    fn config() -> Self {
        Self {
            code: "sync_config_unavailable",
            message: "The application sync configuration is unavailable.",
        }
    }

    fn invalid_patch() -> Self {
        Self {
            code: "invalid_sync_config_patch",
            message: "The sync configuration patch is empty or invalid.",
        }
    }

    fn invalid_credentials() -> Self {
        Self {
            code: "invalid_sync_credentials",
            message: "Sync credentials must be non-empty or explicitly cleared.",
        }
    }

    fn revision() -> Self {
        Self {
            code: "sync_revision_conflict",
            message: "The sync configuration changed before the operation.",
        }
    }

    fn run() -> Self {
        Self {
            code: "sync_run_unavailable",
            message: "The requested sync run is unavailable.",
        }
    }

    fn runtime() -> Self {
        Self {
            code: "sync_runtime_unavailable",
            message: "The sync runtime is unavailable.",
        }
    }
}

impl std::fmt::Display for SyncServiceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for SyncServiceError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SyncRunState {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl SyncRunState {
    fn terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncRunStatus {
    pub(crate) run_id: Uuid,
    pub(crate) revision: String,
    pub(crate) provider: SyncProvider,
    pub(crate) state: SyncRunState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) summary: Option<SyncSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error_code: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SanitizedPersistedSyncStatus {
    pub(crate) completion_state: SyncCompletionState,
    pub(crate) error: Option<SyncSafeError>,
    pub(crate) last_attempt_at: String,
    pub(crate) last_successful_sync_at: Option<String>,
    pub(crate) last_trigger: SyncTrigger,
    pub(crate) provider: SyncProvider,
    pub(crate) revision: Option<String>,
    pub(crate) summary: Option<SyncSummary>,
    pub(crate) version: u32,
}

impl From<SyncStatus> for SanitizedPersistedSyncStatus {
    fn from(status: SyncStatus) -> Self {
        Self {
            completion_state: status.completion_state,
            error: status.error,
            last_attempt_at: status.last_attempt_at,
            last_successful_sync_at: status.last_successful_sync_at,
            last_trigger: status.last_trigger,
            provider: status.provider,
            revision: status.revision,
            summary: status.summary,
            version: status.version,
        }
    }
}

pub(crate) trait SyncRunner: Send + Sync {
    fn run(
        &self,
        notes_root: PathBuf,
        revision: String,
    ) -> Pin<Box<dyn Future<Output = Result<SyncRunResult, String>> + Send + 'static>>;
}

struct NativeSyncRunner {
    app: tauri::AppHandle,
}

impl SyncRunner for NativeSyncRunner {
    fn run(
        &self,
        notes_root: PathBuf,
        revision: String,
    ) -> Pin<Box<dyn Future<Output = Result<SyncRunResult, String>> + Send + 'static>> {
        let app = self.app.clone();
        Box::pin(async move {
            let current = crate::primary_workspace::resolve_sync_primary_workspace(&app)?;
            if current != notes_root {
                return Err(crate::primary_workspace::sync_primary_workspace_mismatch());
            }
            let app_data = app.path().app_data_dir().map_err(|_| {
                "app-data-unavailable: The application data directory is unavailable.".to_string()
            })?;
            let snapshot = ready_snapshot_at_app_data(&app_data, Some(&revision))?;
            crate::remote_sync::service::run_application_sync(
                &app,
                notes_root,
                snapshot,
                SyncTrigger::Manual,
            )
            .await
            .map_err(|error| error.to_string())
        })
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct SyncRunKey {
    notes_root: PathBuf,
    revision: String,
}

struct SyncRunEntry {
    key: SyncRunKey,
    notify: Arc<Notify>,
    status: SyncRunStatus,
}

#[derive(Default)]
struct SyncRunRegistryInner {
    active: HashMap<SyncRunKey, Uuid>,
    order: VecDeque<Uuid>,
    runs: HashMap<Uuid, SyncRunEntry>,
}

#[derive(Clone, Default)]
pub(crate) struct SyncRunRegistry {
    inner: Arc<Mutex<SyncRunRegistryInner>>,
}

impl SyncRunRegistry {
    fn begin(
        &self,
        notes_root: PathBuf,
        revision: &str,
        provider: SyncProvider,
    ) -> Result<(SyncRunStatus, bool), SyncServiceError> {
        let mut inner = self.inner.lock().map_err(|_| SyncServiceError::runtime())?;
        let key = SyncRunKey {
            notes_root,
            revision: revision.to_string(),
        };
        if let Some(run_id) = inner.active.get(&key) {
            let status = inner
                .runs
                .get(run_id)
                .map(|entry| entry.status.clone())
                .ok_or_else(SyncServiceError::run)?;
            return Ok((status, false));
        }
        let run_id = Uuid::new_v4();
        let status = SyncRunStatus {
            run_id,
            revision: revision.to_string(),
            provider,
            state: SyncRunState::Queued,
            summary: None,
            error_code: None,
        };
        inner.active.insert(key.clone(), run_id);
        inner.order.push_back(run_id);
        inner.runs.insert(
            run_id,
            SyncRunEntry {
                key,
                notify: Arc::new(Notify::new()),
                status: status.clone(),
            },
        );
        Ok((status, true))
    }

    fn mark_running(&self, run_id: Uuid) {
        if let Ok(mut inner) = self.inner.lock() {
            if let Some(entry) = inner.runs.get_mut(&run_id) {
                entry.status.state = SyncRunState::Running;
            }
        }
    }

    fn complete(&self, run_id: Uuid, outcome: Result<SyncRunResult, String>) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        let Some(entry) = inner.runs.get_mut(&run_id) else {
            return;
        };
        match outcome {
            Ok(result) => {
                entry.status.state = SyncRunState::Succeeded;
                entry.status.summary = Some(result.summary);
                entry.status.error_code = None;
            }
            Err(error) => {
                entry.status.state = SyncRunState::Failed;
                entry.status.summary = None;
                entry.status.error_code = Some(safe_sync_error_code(&error));
            }
        }
        let key = entry.key.clone();
        let notify = Arc::clone(&entry.notify);
        if inner.active.get(&key) == Some(&run_id) {
            inner.active.remove(&key);
        }
        prune_runs(&mut inner);
        drop(inner);
        notify.notify_waiters();
    }

    pub(crate) fn status(&self, run_id: Uuid) -> Result<SyncRunStatus, SyncServiceError> {
        self.inner
            .lock()
            .map_err(|_| SyncServiceError::runtime())?
            .runs
            .get(&run_id)
            .map(|entry| entry.status.clone())
            .ok_or_else(SyncServiceError::run)
    }

    async fn wait(&self, run_id: Uuid) -> Result<SyncRunStatus, SyncServiceError> {
        loop {
            let (status, notify) = {
                let inner = self.inner.lock().map_err(|_| SyncServiceError::runtime())?;
                let entry = inner.runs.get(&run_id).ok_or_else(SyncServiceError::run)?;
                (entry.status.clone(), Arc::clone(&entry.notify))
            };
            if status.state.terminal() {
                return Ok(status);
            }
            notify.notified().await;
        }
    }
}

fn prune_runs(inner: &mut SyncRunRegistryInner) {
    while inner.runs.len() > RUN_RETENTION_LIMIT {
        let Some(run_id) = inner.order.pop_front() else {
            return;
        };
        if inner
            .runs
            .get(&run_id)
            .is_some_and(|entry| entry.status.state.terminal())
        {
            inner.runs.remove(&run_id);
        } else {
            inner.order.push_back(run_id);
            return;
        }
    }
}

#[derive(Clone)]
pub(crate) struct SyncService {
    app: Option<tauri::AppHandle>,
    #[cfg(test)]
    app_data_for_test: Option<PathBuf>,
    #[cfg(test)]
    primary_root_for_test: Option<PathBuf>,
    #[cfg(test)]
    primary_root_resolver_for_test: Option<Arc<dyn Fn() -> Option<PathBuf> + Send + Sync>>,
    runs: SyncRunRegistry,
    runner: Arc<dyn SyncRunner>,
}

impl SyncService {
    pub(crate) fn new(app: tauri::AppHandle) -> Self {
        Self {
            app: Some(app.clone()),
            #[cfg(test)]
            app_data_for_test: None,
            #[cfg(test)]
            primary_root_for_test: None,
            #[cfg(test)]
            primary_root_resolver_for_test: None,
            runs: SyncRunRegistry::default(),
            runner: Arc::new(NativeSyncRunner { app }),
        }
    }

    #[cfg(test)]
    pub(crate) fn new_for_test_with_app_data(
        runner: Arc<dyn SyncRunner>,
        app_data: PathBuf,
        primary_root: Option<PathBuf>,
    ) -> Self {
        Self {
            app: None,
            app_data_for_test: Some(app_data),
            primary_root_for_test: primary_root,
            primary_root_resolver_for_test: None,
            runs: SyncRunRegistry::default(),
            runner,
        }
    }

    pub(crate) fn get_config(&self) -> Result<SanitizedSyncConfig, SyncServiceError> {
        let document = loaded_document(&self.app_data_dir()?)?;
        Ok(sanitize_document(document))
    }

    #[cfg(test)]
    pub(crate) fn with_primary_root_resolver_for_test(
        mut self,
        resolver: impl Fn() -> Option<PathBuf> + Send + Sync + 'static,
    ) -> Self {
        self.primary_root_resolver_for_test = Some(Arc::new(resolver));
        self
    }

    pub(crate) fn sync_enabled_for_workspace(&self, workspace_root: &std::path::Path) -> bool {
        let Some(workspace_root) = workspace_root.to_str() else {
            return false;
        };
        let Ok(workspace_root) =
            crate::workspace_membership::canonical_workspace_root(workspace_root)
        else {
            return false;
        };
        self.primary_root()
            .is_ok_and(|primary| primary == workspace_root)
            && self.get_config().is_ok_and(|config| config.enabled)
    }

    pub(crate) fn sync_enabled_for_authoritative_primary(&self) -> bool {
        self.get_config().is_ok_and(|config| config.enabled)
    }

    pub(crate) fn update_config(
        &self,
        input: SyncConfigPatchInput,
    ) -> Result<SanitizedSyncConfig, SyncServiceError> {
        let mut patches = Vec::new();
        push_patch(&mut patches, input.enabled.map(SyncConfigPatch::Enabled));
        push_patch(&mut patches, input.provider.map(SyncConfigPatch::Provider));
        push_patch(
            &mut patches,
            input.remote_root.map(SyncConfigPatch::RemoteRoot),
        );
        push_patch(
            &mut patches,
            input.auto_sync_on_save.map(SyncConfigPatch::AutoSyncOnSave),
        );
        push_patch(
            &mut patches,
            input.interval_minutes.map(SyncConfigPatch::IntervalMinutes),
        );
        push_patch(
            &mut patches,
            input
                .webdav_server_url
                .map(SyncConfigPatch::WebDavServerUrl),
        );
        push_patch(
            &mut patches,
            input.s3_endpoint_url.map(SyncConfigPatch::S3EndpointUrl),
        );
        push_patch(&mut patches, input.s3_region.map(SyncConfigPatch::S3Region));
        push_patch(&mut patches, input.s3_bucket.map(SyncConfigPatch::S3Bucket));
        if patches.is_empty() {
            return Err(SyncServiceError::invalid_patch());
        }
        let app_data = self.app_data_dir()?;
        let outcome = patch_batch_at_app_data(&app_data, &input.expected_revision, patches)
            .map_err(map_storage_error)?;
        self.emit_config_changed(&outcome.document.revision);
        Ok(sanitize_document(outcome.document))
    }

    pub(crate) fn update_credentials(
        &self,
        input: SyncCredentialPatchInput,
    ) -> Result<SanitizedSyncConfig, SyncServiceError> {
        let clear = input.clear_credentials.unwrap_or(false);
        let provided = [
            input.webdav_username.as_ref(),
            input.webdav_password.as_ref(),
            input.s3_access_key_id.as_ref(),
            input.s3_secret_access_key.as_ref(),
        ];
        if clear && provided.iter().any(|value| value.is_some()) {
            return Err(SyncServiceError::invalid_credentials());
        }
        if provided.iter().flatten().any(|value| value.is_empty()) {
            return Err(SyncServiceError::invalid_credentials());
        }
        let mut patches = if clear {
            vec![
                SyncConfigPatch::WebDavUsername(String::new()),
                SyncConfigPatch::WebDavPassword(String::new()),
                SyncConfigPatch::S3AccessKeyId(String::new()),
                SyncConfigPatch::S3SecretAccessKey(String::new()),
            ]
        } else {
            Vec::new()
        };
        if !clear {
            push_patch(
                &mut patches,
                input.webdav_username.map(SyncConfigPatch::WebDavUsername),
            );
            push_patch(
                &mut patches,
                input.webdav_password.map(SyncConfigPatch::WebDavPassword),
            );
            push_patch(
                &mut patches,
                input.s3_access_key_id.map(SyncConfigPatch::S3AccessKeyId),
            );
            push_patch(
                &mut patches,
                input
                    .s3_secret_access_key
                    .map(SyncConfigPatch::S3SecretAccessKey),
            );
        }
        if patches.is_empty() {
            return Err(SyncServiceError::invalid_credentials());
        }
        let app_data = self.app_data_dir()?;
        let outcome = patch_batch_at_app_data(&app_data, &input.expected_revision, patches)
            .map_err(map_storage_error)?;
        self.emit_config_changed(&outcome.document.revision);
        Ok(sanitize_document(outcome.document))
    }

    pub(crate) async fn test(
        &self,
        expected_revision: &str,
    ) -> Result<SyncConnectionTestResult, SyncServiceError> {
        let snapshot = ready_snapshot_at_app_data(&self.app_data_dir()?, Some(expected_revision))
            .map_err(map_sync_error)?;
        super::test_application_connection(snapshot)
            .await
            .map_err(|_| SyncServiceError::runtime())
    }

    pub(crate) fn run_background(
        &self,
        expected_revision: &str,
    ) -> Result<SyncRunStatus, SyncServiceError> {
        self.start_run(expected_revision)
    }

    pub(crate) async fn run(
        &self,
        expected_revision: &str,
        execution: SyncExecutionPolicy,
        timeout: Duration,
    ) -> Result<SyncRunStatus, SyncServiceError> {
        let started = self.start_run(expected_revision)?;
        if execution == SyncExecutionPolicy::Background {
            return Ok(started);
        }
        match tokio::time::timeout(timeout, self.runs.wait(started.run_id)).await {
            Ok(status) => status,
            Err(_) => self.runs.status(started.run_id),
        }
    }

    pub(crate) fn status(&self, run_id: Uuid) -> Result<SyncRunStatus, SyncServiceError> {
        self.runs.status(run_id)
    }

    pub(crate) fn persisted_status(
        &self,
    ) -> Result<Option<SanitizedPersistedSyncStatus>, SyncServiceError> {
        let app_data = self.app_data_dir()?;
        let Some(status) = crate::sync_config::status::load_sync_status_at_app_data(&app_data)
            .map_err(|_| SyncServiceError::runtime())?
        else {
            return Ok(None);
        };
        let Ok(primary_root) = self.primary_root() else {
            return Ok(None);
        };
        let Some(status_root) = status.notes_root.as_deref() else {
            return Ok(None);
        };
        let Ok(status_root) = crate::workspace_membership::canonical_workspace_root(status_root)
        else {
            return Ok(None);
        };
        let Ok(document) = loaded_document(&app_data) else {
            return Ok(None);
        };
        if status_root != primary_root
            || status.revision.as_deref() != Some(document.revision.as_str())
        {
            return Ok(None);
        }
        Ok(Some(status.into()))
    }

    fn app_data_dir(&self) -> Result<PathBuf, SyncServiceError> {
        #[cfg(test)]
        if let Some(app_data) = &self.app_data_for_test {
            return Ok(app_data.clone());
        }
        self.app
            .as_ref()
            .ok_or_else(SyncServiceError::runtime)?
            .path()
            .app_data_dir()
            .map_err(|_| SyncServiceError::runtime())
    }

    fn primary_root(&self) -> Result<PathBuf, SyncServiceError> {
        #[cfg(test)]
        if let Some(resolver) = &self.primary_root_resolver_for_test {
            return resolver().ok_or_else(SyncServiceError::workspace);
        }
        #[cfg(test)]
        if let Some(root) = &self.primary_root_for_test {
            let root = root.to_str().ok_or_else(SyncServiceError::workspace)?;
            return crate::workspace_membership::canonical_workspace_root(root)
                .map_err(|_| SyncServiceError::workspace());
        }
        let app = self.app.as_ref().ok_or_else(SyncServiceError::workspace)?;
        crate::primary_workspace::resolve_sync_primary_workspace(app)
            .map_err(|_| SyncServiceError::workspace())
    }

    fn start_run(&self, expected_revision: &str) -> Result<SyncRunStatus, SyncServiceError> {
        let notes_root = self.primary_root()?;
        let snapshot = ready_snapshot_at_app_data(&self.app_data_dir()?, Some(expected_revision))
            .map_err(map_sync_error)?;
        let provider = snapshot.config.provider;
        let revision = snapshot.revision;
        let (status, leader) = self.runs.begin(notes_root.clone(), &revision, provider)?;
        if !leader {
            return Ok(status);
        }
        let runner = Arc::clone(&self.runner);
        let runs = self.runs.clone();
        let run_id = status.run_id;
        tokio::runtime::Handle::try_current().map_err(|_| SyncServiceError::runtime())?;
        tokio::spawn(async move {
            runs.mark_running(run_id);
            let outcome = runner.run(notes_root, revision).await;
            runs.complete(run_id, outcome);
        });
        Ok(status)
    }

    fn emit_config_changed(&self, revision: &str) {
        if let Some(app) = &self.app {
            let _emit_result = emit_sync_config_changed(app, revision);
        }
    }
}

fn push_patch(patches: &mut Vec<SyncConfigPatch>, patch: Option<SyncConfigPatch>) {
    if let Some(patch) = patch {
        patches.push(patch);
    }
}

fn loaded_document(app_data: &std::path::Path) -> Result<SyncConfigDocument, SyncServiceError> {
    match load_from_app_data(app_data).map_err(|_| SyncServiceError::config())? {
        SyncConfigLoadResponse::Loaded { document } => Ok(document),
        _ => Err(SyncServiceError::config()),
    }
}

fn sanitize_document(document: SyncConfigDocument) -> SanitizedSyncConfig {
    let SyncConfigDocument {
        config,
        configured: _,
        issues,
        readiness,
        revision,
    } = document;
    SanitizedSyncConfig {
        revision,
        enabled: config.enabled,
        provider: config.provider,
        remote_root: config.remote_root,
        auto_sync_on_save: config.auto_sync_on_save,
        interval_minutes: config.interval_minutes,
        webdav_server_url: config.webdav.server_url,
        s3_endpoint_url: config.s3.endpoint_url,
        s3_region: config.s3.region,
        s3_bucket: config.s3.bucket,
        webdav_credentials_configured: !config.webdav.username.is_empty()
            && !config.webdav.password.is_empty(),
        s3_credentials_configured: !config.s3.access_key_id.is_empty()
            && !config.s3.secret_access_key.is_empty(),
        readiness,
        issues,
    }
}

fn map_storage_error(
    error: crate::sync_config::storage::SyncConfigStorageError,
) -> SyncServiceError {
    if error.code == "revision-conflict" {
        SyncServiceError::revision()
    } else {
        SyncServiceError::config()
    }
}

fn map_sync_error(error: String) -> SyncServiceError {
    if error.starts_with("revision-conflict:") {
        SyncServiceError::revision()
    } else if error.starts_with("sync-config-")
        || error.starts_with("sync-disabled:")
        || error.starts_with("sync-not-ready:")
    {
        SyncServiceError::config()
    } else if error.starts_with("sync-primary-workspace-") {
        SyncServiceError::workspace()
    } else {
        SyncServiceError::runtime()
    }
}

fn safe_sync_error_code(error: &str) -> String {
    let code = error.split(':').next().unwrap_or_default();
    match code {
        "app-data-unavailable"
        | "notes-root-unavailable"
        | "remote-http-error"
        | "revision-conflict"
        | "settings-listing-failed"
        | "settings-reconcile-failed"
        | "sync-config-absent"
        | "sync-config-malformed"
        | "sync-config-unsupported"
        | "sync-disabled"
        | "sync-failed"
        | "sync-not-ready"
        | "sync-primary-workspace-mismatch"
        | "sync-run-failed"
        | "sync-snapshot-mismatch"
        | "sync-state-mismatch"
        | "sync-status-event-failed"
        | "sync-status-invalid"
        | "sync-status-path-unsafe"
        | "sync-status-read-failed"
        | "sync-status-write-failed" => code.to_string(),
        _ => "sync-failed".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::{future::Future, pin::Pin, sync::Arc};

    use tempfile::tempdir;

    use super::{safe_sync_error_code, SyncRunner, SyncService};
    use crate::sync_config::{
        model::{SyncConfigPatch, SyncProvider},
        status::{
            write_sync_status_at_app_data, SyncRunResult, SyncStatus, SyncSummary, SyncTrigger,
        },
        storage::{enable_at_app_data, patch_batch_at_app_data},
    };

    struct UnusedRunner;

    impl SyncRunner for UnusedRunner {
        fn run(
            &self,
            _notes_root: std::path::PathBuf,
            _revision: String,
        ) -> Pin<Box<dyn Future<Output = Result<SyncRunResult, String>> + Send + 'static>> {
            Box::pin(async { unreachable!("status reads do not start sync") })
        }
    }

    fn ready_app_config(app_data: &std::path::Path) -> String {
        let enabled = enable_at_app_data(app_data, None).unwrap();
        patch_batch_at_app_data(
            app_data,
            &enabled.document.revision,
            vec![
                SyncConfigPatch::Enabled(true),
                SyncConfigPatch::RemoteRoot("notes".into()),
                SyncConfigPatch::WebDavServerUrl("https://dav.example.test".into()),
                SyncConfigPatch::WebDavUsername("user".into()),
                SyncConfigPatch::WebDavPassword("secret".into()),
            ],
        )
        .unwrap()
        .document
        .revision
    }

    #[test]
    fn persisted_status_reads_application_state_without_exposing_notes_root() {
        let fixture = tempdir().unwrap();
        let notes = fixture.path().join("notes");
        let app_data = fixture.path().join("app-data");
        std::fs::create_dir_all(&notes).unwrap();
        std::fs::create_dir_all(&app_data).unwrap();
        let revision = ready_app_config(&app_data);
        let status = SyncStatus::attempting_for_run(
            SyncProvider::S3,
            SyncTrigger::Save,
            "2026-07-21T00:00:00Z".to_string(),
            notes.to_string_lossy().into_owned(),
            "notes".into(),
            revision,
            None,
        )
        .succeeded("2026-07-21T00:00:01Z".to_string(), SyncSummary::default());
        write_sync_status_at_app_data(&app_data, &status).unwrap();
        let service = SyncService::new_for_test_with_app_data(
            Arc::new(UnusedRunner),
            app_data,
            Some(notes.clone()),
        );

        let sanitized = service.persisted_status().unwrap().unwrap();
        let serialized = serde_json::to_string(&sanitized).unwrap();
        assert_eq!(sanitized.provider, SyncProvider::S3);
        assert!(!serialized.contains(&notes.to_string_lossy().to_string()));
        assert!(!serialized.contains("notesRoot"));
    }

    #[test]
    fn persisted_status_hides_stale_primary_root_and_stale_config_revision() {
        let fixture = tempdir().unwrap();
        let notes = fixture.path().join("notes");
        let stale_notes = fixture.path().join("stale-notes");
        let app_data = fixture.path().join("app-data");
        std::fs::create_dir_all(&notes).unwrap();
        std::fs::create_dir_all(&stale_notes).unwrap();
        std::fs::create_dir_all(&app_data).unwrap();
        let revision = ready_app_config(&app_data);
        let service = SyncService::new_for_test_with_app_data(
            Arc::new(UnusedRunner),
            app_data.clone(),
            Some(notes.clone()),
        );

        let stale_root = SyncStatus::attempting_for_run(
            SyncProvider::Webdav,
            SyncTrigger::Manual,
            "2026-07-21T00:00:00Z".into(),
            stale_notes.to_string_lossy().into_owned(),
            "stale-notes".into(),
            revision.clone(),
            None,
        );
        write_sync_status_at_app_data(&app_data, &stale_root).unwrap();
        assert_eq!(service.persisted_status().unwrap(), None);

        let stale_revision = SyncStatus::attempting_for_run(
            SyncProvider::Webdav,
            SyncTrigger::Manual,
            "2026-07-21T00:00:01Z".into(),
            notes.to_string_lossy().into_owned(),
            "notes".into(),
            format!("stale-{revision}"),
            None,
        );
        write_sync_status_at_app_data(&app_data, &stale_revision).unwrap();
        assert_eq!(service.persisted_status().unwrap(), None);
    }

    #[test]
    fn run_error_codes_use_an_explicit_safe_allowlist() {
        assert_eq!(
            safe_sync_error_code("private-secret-token: provider detail"),
            "sync-failed"
        );
        assert_eq!(
            safe_sync_error_code("sync-secret-token: provider detail"),
            "sync-failed"
        );
        assert_eq!(
            safe_sync_error_code("remote-http-error: response body omitted"),
            "remote-http-error"
        );
        assert_eq!(
            safe_sync_error_code("settings-reconcile-failed: path omitted"),
            "settings-reconcile-failed"
        );
    }
}
