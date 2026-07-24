pub(crate) mod editing;
pub(crate) mod model;
pub(crate) mod status;
pub(crate) mod storage;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};

use editing::{
    begin_sync_apply, cancel_sync_apply as cancel_sync_apply_in_registry, complete_sync_apply,
    load_sync_editing_state, request_sync_apply_with_notify, set_sync_editing_with_notify,
    sync_editing_active, wait_sync_apply, CancelSyncConfigApplyRequest, RequestSyncConfigApply,
    SetSyncConfigEditingRequest, SyncApplyDisposition, SyncEditingEvent, SyncEditingSnapshot,
    SyncPendingApply,
};
use model::{
    RecoverSyncConfigRequest, ResetSyncConfigRequest, SyncConfigDocument, SyncConfigLoadResponse,
    SyncConfigPatchRequest, SyncConfigReadiness, SyncConnectionTestResult, SyncSnapshot,
    SyncTarget,
};
use status::{
    emit_sync_status_changed, load_sync_status_at_app_data, sync_status_timestamp,
    write_sync_status_at_app_data, SyncRunResult, SyncSafeError, SyncStatus, SyncTrigger,
};
use storage::{
    enable_at_app_data, load_from_app_data, patch_at_app_data, recover_at_app_data,
    reset_at_app_data, SyncConfigDurability, SyncConfigWriteOutcome,
};

const SYNC_CONFIG_CHANGED_EVENT: &str = "qingyu://sync-config-changed";
const SYNC_CONFIG_EDITING_EVENT: &str = "qingyu://sync-config-editing";
const SYNC_CONFIG_APPLY_REQUESTED_EVENT: &str = "qingyu://sync-config-apply-requested";
const INVALID_SYNC_CONFIG_PATCH_ERROR: &str =
    "sync-config-invalid-patch: Submit a supported sync configuration field update.";
const INVALID_SYNC_CONFIG_DRAFT_ERROR: &str =
    "sync-config-invalid-draft: Submit a complete supported sync configuration.";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SyncConfigChangedEvent {
    revision: String,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct SyncApplicationRequest {
    apply_token: Option<String>,
    #[serde(default)]
    bootstrap: bool,
    #[serde(default)]
    notebook_name: Option<String>,
    #[serde(default)]
    notes_root: Option<String>,
    #[serde(default)]
    prepared_target_lease: Option<String>,
    revision: String,
    trigger: SyncTrigger,
}

fn validate_sync_application_notebook(
    request: &SyncApplicationRequest,
    canonical_notes_root: &Path,
) -> Result<String, String> {
    let actual = crate::notebook_scope::notebook_name_from_root(canonical_notes_root)?;
    match (request.bootstrap, request.notebook_name.as_deref()) {
        (true, None) => Ok(actual),
        (false, Some(expected)) if actual == expected => Ok(actual),
        _ => Err("sync-notebook-scope-mismatch: The notebook sync identity changed.".to_string()),
    }
}

fn validate_sync_application_result(
    request: &SyncApplicationRequest,
    canonical_notes_root: &Path,
    result: SyncRunResult,
) -> Result<SyncRunResult, String> {
    let notebook_name = validate_sync_application_notebook(request, canonical_notes_root)?;
    if result.notes_root == canonical_notes_root.to_string_lossy()
        && result.notebook_name == notebook_name
        && result.revision == request.revision
    {
        Ok(result)
    } else {
        Err("sync-result-mismatch: The synchronization result identity changed.".to_string())
    }
}

fn validate_sync_application_mode(request: &SyncApplicationRequest) -> Result<(), String> {
    if request.bootstrap
        && (request.trigger != SyncTrigger::Manual
            || request.apply_token.is_some()
            || request.notebook_name.is_some())
    {
        return Err(
            "sync-bootstrap-invalid: Bootstrap sync requires a native-derived notebook identity."
                .to_string(),
        );
    }
    if !request.bootstrap && request.notebook_name.is_none() {
        return Err(
            "sync-notebook-scope-mismatch: The notebook sync identity changed.".to_string(),
        );
    }
    if !request.bootstrap
        && (request.notes_root.is_none() || request.prepared_target_lease.is_some())
    {
        return Err(
            "sync-notebook-scope-mismatch: The notebook sync identity changed.".to_string(),
        );
    }
    #[cfg(mobile)]
    if request.bootstrap
        && (request.notes_root.is_none() || request.prepared_target_lease.is_some())
    {
        return Err(
            "sync-bootstrap-invalid: Bootstrap sync requires a managed notes root.".to_string(),
        );
    }
    #[cfg(not(mobile))]
    if request.bootstrap
        && (request.notes_root.is_some() || request.prepared_target_lease.is_none())
    {
        return Err(
            "sync-bootstrap-invalid: Bootstrap sync requires a prepared target lease.".to_string(),
        );
    }
    Ok(())
}

struct ValidatedSyncApplicationRoot {
    canonical: PathBuf,
    source: ValidatedSyncApplicationSource,
}

enum ValidatedSyncApplicationSource {
    Regular,
    #[cfg(mobile)]
    ManagedBootstrap(cap_std::fs::Dir),
    #[cfg(not(mobile))]
    PreparedDirectory(crate::primary_workspace::ConsumedPreparedDesktopNotebookTarget),
}

fn validate_sync_application_root(
    app: &tauri::AppHandle,
    request: &SyncApplicationRequest,
) -> Result<ValidatedSyncApplicationRoot, String> {
    if request.bootstrap {
        #[cfg(mobile)]
        {
            let root = request.notes_root.as_deref().ok_or_else(|| {
                "sync-bootstrap-invalid: Bootstrap sync requires a managed notes root.".to_string()
            })?;
            return crate::primary_workspace::validate_bootstrap_notes_root(app, root).and_then(
                |canonical| {
                    let directory =
                        crate::storage_capability::open_canonical_directory_nofollow(&canonical)
                            .map_err(|_| {
                                crate::primary_workspace::sync_primary_workspace_mismatch()
                            })?;
                    Ok(ValidatedSyncApplicationRoot {
                        canonical,
                        source: ValidatedSyncApplicationSource::ManagedBootstrap(directory),
                    })
                },
            );
        }
        #[cfg(not(mobile))]
        {
            let _ = app;
            let lease = request.prepared_target_lease.as_deref().ok_or_else(|| {
                "sync-bootstrap-invalid: Bootstrap sync requires a prepared target lease."
                    .to_string()
            })?;
            return crate::primary_workspace::consume_prepared_desktop_notebook_target(lease).map(
                |prepared| ValidatedSyncApplicationRoot {
                    canonical: prepared.notes_root.clone(),
                    source: ValidatedSyncApplicationSource::PreparedDirectory(prepared),
                },
            );
        }
    } else {
        let root = request.notes_root.as_deref().ok_or_else(|| {
            "sync-notebook-scope-mismatch: The notebook sync identity changed.".to_string()
        })?;
        crate::primary_workspace::validate_sync_notes_root(app, root).map(|canonical| {
            ValidatedSyncApplicationRoot {
                canonical,
                source: ValidatedSyncApplicationSource::Regular,
            }
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct TestSyncConnectionRequest {
    revision: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct ListRemoteNotebooksRequest {
    revision: String,
}

fn app_data_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|_| {
        "app-data-unavailable: The application data directory is unavailable.".to_string()
    })
}

pub(crate) fn emit_sync_config_changed(
    app: &tauri::AppHandle,
    revision: &str,
) -> Result<(), String> {
    app.emit(
        SYNC_CONFIG_CHANGED_EVENT,
        SyncConfigChangedEvent {
            revision: revision.to_string(),
        },
    )
    .map_err(|_| {
        "sync-config-event-failed: The sync configuration change could not be announced."
            .to_string()
    })
}

fn installed_document(outcome: SyncConfigWriteOutcome) -> SyncConfigDocument {
    match outcome {
        SyncConfigWriteOutcome {
            document,
            durability: SyncConfigDurability::Durable,
        }
        | SyncConfigWriteOutcome {
            document,
            durability: SyncConfigDurability::ParentSyncUncertain,
        } => document,
    }
}

fn finish_installed_write<Notify>(
    outcome: SyncConfigWriteOutcome,
    notify: Notify,
) -> SyncConfigDocument
where
    Notify: FnOnce(&str) -> Result<(), String>,
{
    let document = installed_document(outcome);
    let _notification = notify(&document.revision);
    document
}

fn parse_patch_request(value: serde_json::Value) -> Result<SyncConfigPatchRequest, String> {
    serde_json::from_value(value).map_err(|_| INVALID_SYNC_CONFIG_PATCH_ERROR.to_string())
}

fn parse_recover_request(value: serde_json::Value) -> Result<RecoverSyncConfigRequest, String> {
    serde_json::from_value(value).map_err(|_| INVALID_SYNC_CONFIG_DRAFT_ERROR.to_string())
}

pub(crate) fn ready_snapshot_at_app_data(
    app_data: &Path,
    expected_revision: Option<&str>,
) -> Result<SyncSnapshot, String> {
    let document = match load_from_app_data(app_data).map_err(|error| error.to_string())? {
        SyncConfigLoadResponse::Loaded { document } => document,
        SyncConfigLoadResponse::Absent { .. } => {
            return Err("sync-config-absent: The sync configuration does not exist.".into())
        }
        SyncConfigLoadResponse::Malformed { .. } => {
            return Err("sync-config-malformed: Reset or recover sync configuration first.".into())
        }
        SyncConfigLoadResponse::Unsupported { .. } => {
            return Err(
                "sync-config-unsupported: Reset or recover sync configuration first.".into(),
            )
        }
    };
    if expected_revision.is_some_and(|expected| expected != document.revision) {
        return Err("revision-conflict: The sync configuration changed before this run.".into());
    }
    match document.readiness {
        SyncConfigReadiness::Disabled => {
            return Err("sync-disabled: Synchronization is disabled.".into())
        }
        SyncConfigReadiness::Incomplete => {
            return Err("sync-not-ready: The sync configuration is incomplete.".into())
        }
        SyncConfigReadiness::Ready => {}
    }
    let revision = document.revision;
    let config = document.config;
    let target = match config.provider {
        model::SyncProvider::Webdav => SyncTarget::Webdav {
            remote_root: config.remote_root.clone(),
            server_url: config.webdav.server_url.clone(),
            username: config.webdav.username.clone(),
            password: config.webdav.password.clone(),
        },
        model::SyncProvider::S3 => SyncTarget::S3 {
            access_key_id: config.s3.access_key_id.clone(),
            addressing_style: config.s3.addressing_style,
            bucket: config.s3.bucket.clone(),
            endpoint_url: config.s3.endpoint_url.clone(),
            region: config.s3.region.clone(),
            remote_root: config.remote_root.clone(),
            request_timeout_seconds: config.s3.request_timeout_seconds,
            secret_access_key: config.s3.secret_access_key.clone(),
            tls_verification: config.s3.tls_verification,
        },
    };
    Ok(SyncSnapshot {
        config,
        revision,
        state_root: app_data.join("sync-state"),
        target,
    })
}

pub(crate) fn configured_snapshot_at_app_data(
    app_data: &Path,
    expected_revision: Option<&str>,
) -> Result<SyncSnapshot, String> {
    let document = match load_from_app_data(app_data).map_err(|error| error.to_string())? {
        SyncConfigLoadResponse::Loaded { document } => document,
        SyncConfigLoadResponse::Absent { .. } => {
            return Err("sync-config-absent: The sync configuration does not exist.".into())
        }
        SyncConfigLoadResponse::Malformed { .. } => {
            return Err("sync-config-malformed: Reset or recover sync configuration first.".into())
        }
        SyncConfigLoadResponse::Unsupported { .. } => {
            return Err(
                "sync-config-unsupported: Reset or recover sync configuration first.".into(),
            )
        }
    };
    if expected_revision.is_some_and(|expected| expected != document.revision) {
        return Err("revision-conflict: The sync configuration changed before this run.".into());
    }
    if !document.configured {
        return Err("sync-not-ready: The sync configuration is incomplete.".into());
    }
    let revision = document.revision;
    let config = document.config;
    let target = match config.provider {
        model::SyncProvider::Webdav => SyncTarget::Webdav {
            remote_root: config.remote_root.clone(),
            server_url: config.webdav.server_url.clone(),
            username: config.webdav.username.clone(),
            password: config.webdav.password.clone(),
        },
        model::SyncProvider::S3 => SyncTarget::S3 {
            access_key_id: config.s3.access_key_id.clone(),
            addressing_style: config.s3.addressing_style,
            bucket: config.s3.bucket.clone(),
            endpoint_url: config.s3.endpoint_url.clone(),
            region: config.s3.region.clone(),
            remote_root: config.remote_root.clone(),
            request_timeout_seconds: config.s3.request_timeout_seconds,
            secret_access_key: config.s3.secret_access_key.clone(),
            tls_verification: config.s3.tls_verification,
        },
    };
    Ok(SyncSnapshot {
        config,
        revision,
        state_root: app_data.join("sync-state"),
        target,
    })
}

fn validated_application_sync_apply_token(
    trigger: SyncTrigger,
    apply_token: Option<&str>,
) -> Result<Option<&str>, String> {
    match (trigger, apply_token) {
        (SyncTrigger::SettingsExit, Some(token)) if !token.trim().is_empty() => Ok(Some(token)),
        (SyncTrigger::SettingsExit, _) => {
            Err("sync-apply-unavailable: The sync settings apply is unavailable.".to_string())
        }
        (_, Some(_)) => {
            Err("sync-apply-mismatch: Only a settings apply may use an apply token.".to_string())
        }
        (_, None) => Ok(None),
    }
}

fn enforce_application_sync_editing_barrier(
    trigger: SyncTrigger,
    editing_active: bool,
) -> Result<(), String> {
    if editing_active
        && matches!(
            trigger,
            SyncTrigger::AppLaunch | SyncTrigger::Interval | SyncTrigger::Save
        )
    {
        return Err(
            "sync-editing-active: Automatic sync is suspended while settings are being edited."
                .to_string(),
        );
    }
    Ok(())
}

type SyncApplyCompletion =
    Box<dyn Fn(&str, &str, Result<SyncRunResult, String>) -> Result<(), String> + Send>;

fn application_sync_worker_stopped() -> String {
    "sync-failed: Application sync execution stopped unexpectedly.".to_string()
}

fn sync_error_code(error: &str) -> String {
    error
        .split_once(':')
        .map(|(code, _)| code)
        .filter(|code| !code.is_empty())
        .unwrap_or("notebook-target-invalid")
        .to_string()
}

fn failed_bootstrap_commit_status(
    result: &SyncRunResult,
    previous: Option<&SyncStatus>,
    attempted_at: String,
    error: &str,
) -> SyncStatus {
    SyncStatus::attempting_for_run(
        result.provider,
        result.trigger,
        attempted_at,
        result.notes_root.clone(),
        result.notebook_name.clone(),
        result.revision.clone(),
        previous,
    )
    .failed(
        SyncSafeError {
            category: None,
            code: sync_error_code(error),
            http_status: None,
            method: None,
            object_id: None,
            operation: "commit-notebook".to_string(),
            provider: result.provider,
            provider_error_code: None,
            relative_path: None,
            request_id: None,
            run_id: None,
        },
        result.summary.clone(),
    )
}

fn persist_failed_bootstrap_commit<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    app_data: &Path,
    result: &SyncRunResult,
    previous: Option<&SyncStatus>,
    error: &str,
) -> Result<(), String> {
    let attempted_at = load_sync_status_at_app_data(app_data)?
        .filter(|status| {
            status.notes_root.as_deref() == Some(result.notes_root.as_str())
                && status.notebook_name.as_deref() == Some(result.notebook_name.as_str())
                && status.revision.as_deref() == Some(result.revision.as_str())
        })
        .map(|status| status.last_attempt_at)
        .unwrap_or_else(sync_status_timestamp);
    let failed = failed_bootstrap_commit_status(result, previous, attempted_at, error);
    write_sync_status_at_app_data(app_data, &failed)?;
    let _notification =
        emit_sync_status_changed(app, &result.notes_root, &result.revision, &failed);
    Ok(())
}

fn emit_primary_workspace_changed<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use std::sync::atomic::{AtomicU64, Ordering};

    static GENERATION: AtomicU64 = AtomicU64::new(0);
    let generation = GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
    let _notification = app.emit(
        "qingyu://primary-workspace-changed",
        serde_json::json!({
            "generation": generation,
            "sourceId": "native-sync-bootstrap"
        }),
    );
}

struct SyncApplyCompletionGuard {
    completion: Option<SyncApplyCompletion>,
    revision: String,
    token: String,
}

impl SyncApplyCompletionGuard {
    fn for_registry(revision: String, token: String) -> Self {
        Self::with_completion(revision, token, Box::new(complete_sync_apply))
    }

    fn with_completion(revision: String, token: String, completion: SyncApplyCompletion) -> Self {
        Self {
            completion: Some(completion),
            revision,
            token,
        }
    }

    fn complete(mut self, outcome: Result<SyncRunResult, String>) -> Result<SyncRunResult, String> {
        if let Some(completion) = self.completion.take() {
            completion(&self.revision, &self.token, outcome.clone())?;
        }
        outcome
    }
}

impl Drop for SyncApplyCompletionGuard {
    fn drop(&mut self) {
        if let Some(completion) = self.completion.take() {
            let _ = completion(
                &self.revision,
                &self.token,
                Err(application_sync_worker_stopped()),
            );
        }
    }
}

async fn execute_application_sync(
    app: tauri::AppHandle,
    validated_root: ValidatedSyncApplicationRoot,
    request: SyncApplicationRequest,
) -> Result<SyncRunResult, String> {
    enforce_application_sync_editing_barrier(request.trigger, sync_editing_active()?)?;
    if !request.bootstrap {
        let current_primary = validate_sync_application_root(&app, &request)?;
        if current_primary.canonical != validated_root.canonical {
            return Err(crate::primary_workspace::sync_primary_workspace_mismatch());
        }
    }
    let ValidatedSyncApplicationRoot {
        canonical: canonical_notes_root,
        source,
    } = validated_root;
    validate_sync_application_notebook(&request, &canonical_notes_root)?;
    let app_data = app_data_dir(&app)?;
    let snapshot = if request.bootstrap {
        configured_snapshot_at_app_data(&app_data, Some(&request.revision))?
    } else {
        ready_snapshot_at_app_data(&app_data, Some(&request.revision))?
    };
    let previous_status = if request.bootstrap {
        load_sync_status_at_app_data(&app_data)?
    } else {
        None
    };
    let result = match source {
        #[cfg(not(mobile))]
        ValidatedSyncApplicationSource::PreparedDirectory(prepared) => {
            let directory = prepared.directory.try_clone().map_err(|_| {
                "notebook-target-invalid: The notebook target is unavailable.".to_string()
            })?;
            let result = crate::remote_sync::service::run_application_sync_from_prepared_directory(
                &app,
                canonical_notes_root.clone(),
                directory,
                prepared.restore_generation().to_string(),
                snapshot,
                request.trigger,
            )
            .await
            .map_err(|error| error.to_string());
            match result {
                Ok(result) => match prepared.commit_primary_workspace(&app) {
                    Ok(_) => {
                        emit_primary_workspace_changed(&app);
                        Ok(result)
                    }
                    Err(error) => {
                        persist_failed_bootstrap_commit(
                            &app,
                            &app_data,
                            &result,
                            previous_status.as_ref(),
                            &error,
                        )?;
                        Err(error)
                    }
                },
                Err(error) => Err(error),
            }
        }
        #[cfg(mobile)]
        ValidatedSyncApplicationSource::ManagedBootstrap(directory) => {
            crate::remote_sync::service::run_application_sync_from_managed_bootstrap(
                &app,
                canonical_notes_root.clone(),
                directory,
                snapshot,
                request.trigger,
            )
            .await
            .map_err(|error| error.to_string())
        }
        ValidatedSyncApplicationSource::Regular => {
            crate::remote_sync::service::run_application_sync(
                &app,
                canonical_notes_root.clone(),
                snapshot,
                request.trigger,
            )
            .await
            .map_err(|error| error.to_string())
        }
    }?;
    validate_sync_application_result(&request, &canonical_notes_root, result)
}

#[tauri::command]
pub(crate) async fn sync_application(
    app: tauri::AppHandle,
    request: SyncApplicationRequest,
) -> Result<SyncRunResult, String> {
    validate_sync_application_mode(&request)?;
    let validated_root = validate_sync_application_root(&app, &request)?;
    let canonical_notes_root = validated_root.canonical.clone();
    validate_sync_application_notebook(&request, &canonical_notes_root)?;
    let apply_token =
        validated_application_sync_apply_token(request.trigger, request.apply_token.as_deref())?
            .map(str::to_string);
    if let Some(token) = apply_token {
        match begin_sync_apply(&request.revision, &token)? {
            SyncApplyDisposition::Completed(outcome) => {
                return outcome.and_then(|result| {
                    validate_sync_application_result(&request, &canonical_notes_root, result)
                });
            }
            SyncApplyDisposition::Wait => {
                return wait_sync_apply(&request.revision, &token)
                    .await
                    .and_then(|result| {
                        validate_sync_application_result(&request, &canonical_notes_root, result)
                    });
            }
            SyncApplyDisposition::Execute => {}
        }

        let guard = SyncApplyCompletionGuard::for_registry(request.revision.clone(), token);
        let task = tokio::spawn(async move {
            let outcome = execute_application_sync(app, validated_root, request).await;
            guard.complete(outcome)
        });
        return task
            .await
            .unwrap_or_else(|_| Err(application_sync_worker_stopped()));
    }

    execute_application_sync(app, validated_root, request).await
}

#[tauri::command]
pub(crate) async fn test_sync_connection(
    app: tauri::AppHandle,
    request: TestSyncConnectionRequest,
) -> Result<SyncConnectionTestResult, String> {
    let snapshot = ready_snapshot_at_app_data(&app_data_dir(&app)?, Some(&request.revision))?;
    crate::remote_sync::test_application_connection(snapshot).await
}

#[tauri::command]
pub(crate) async fn list_remote_notebooks(
    app: tauri::AppHandle,
    request: ListRemoteNotebooksRequest,
) -> Result<Vec<crate::remote_sync::catalog::RemoteNotebookCatalogEntry>, String> {
    let snapshot = configured_snapshot_at_app_data(&app_data_dir(&app)?, Some(&request.revision))?;
    crate::remote_sync::catalog::list_remote_notebooks(snapshot).await
}

#[tauri::command]
pub(crate) fn load_sync_config(app: tauri::AppHandle) -> Result<SyncConfigLoadResponse, String> {
    load_from_app_data(&app_data_dir(&app)?).map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn enable_sync_config(
    app: tauri::AppHandle,
    expected_revision: Option<String>,
) -> Result<SyncConfigDocument, String> {
    let outcome = enable_at_app_data(&app_data_dir(&app)?, expected_revision.as_deref())
        .map_err(|error| error.to_string())?;
    Ok(finish_installed_write(outcome, |revision| {
        emit_sync_config_changed(&app, revision)
    }))
}

#[tauri::command]
pub(crate) fn patch_sync_config(
    app: tauri::AppHandle,
    request: serde_json::Value,
) -> Result<SyncConfigDocument, String> {
    let request = parse_patch_request(request)?;
    let outcome = patch_at_app_data(
        &app_data_dir(&app)?,
        &request.expected_revision,
        request.patch,
    )
    .map_err(|error| error.to_string())?;
    Ok(finish_installed_write(outcome, |revision| {
        emit_sync_config_changed(&app, revision)
    }))
}

#[tauri::command]
pub(crate) fn recover_sync_config(
    app: tauri::AppHandle,
    request: serde_json::Value,
) -> Result<SyncConfigDocument, String> {
    let request = parse_recover_request(request)?;
    let outcome = recover_at_app_data(
        &app_data_dir(&app)?,
        &request.expected_revision,
        request.config,
    )
    .map_err(|error| error.to_string())?;
    Ok(finish_installed_write(outcome, |revision| {
        emit_sync_config_changed(&app, revision)
    }))
}

#[tauri::command]
pub(crate) fn reset_sync_config(
    app: tauri::AppHandle,
    request: ResetSyncConfigRequest,
) -> Result<SyncConfigDocument, String> {
    let outcome = reset_at_app_data(
        &app_data_dir(&app)?,
        request.confirmed,
        request.expected_revision.as_deref(),
    )
    .map_err(|error| error.to_string())?;
    Ok(finish_installed_write(outcome, |revision| {
        emit_sync_config_changed(&app, revision)
    }))
}

#[tauri::command]
pub(crate) fn load_sync_config_editing() -> Result<SyncEditingSnapshot, String> {
    load_sync_editing_state()
}

#[tauri::command]
pub(crate) fn set_sync_config_editing(
    app: tauri::AppHandle,
    request: SetSyncConfigEditingRequest,
) -> Result<SyncEditingEvent, String> {
    set_sync_editing_with_notify(request, |event| {
        app.emit(SYNC_CONFIG_EDITING_EVENT, event).map_err(|_| {
            "sync-editing-event-unavailable: The sync editing state could not be announced."
                .to_string()
        })
    })
}

#[tauri::command]
pub(crate) fn request_sync_config_apply(
    app: tauri::AppHandle,
    request: RequestSyncConfigApply,
) -> Result<SyncPendingApply, String> {
    request_sync_apply_with_notify(request, |event| {
        app.emit(SYNC_CONFIG_APPLY_REQUESTED_EVENT, event)
            .map_err(|_| {
                "sync-apply-event-unavailable: The sync settings apply could not be announced."
                    .to_string()
            })
    })
}

#[tauri::command]
pub(crate) fn cancel_sync_config_apply(
    request: CancelSyncConfigApplyRequest,
) -> Result<SyncPendingApply, String> {
    cancel_sync_apply_in_registry(request)
}

#[tauri::command]
pub(crate) fn load_sync_status(app: tauri::AppHandle) -> Result<Option<SyncStatus>, String> {
    load_sync_status_at_app_data(&app_data_dir(&app)?)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use tempfile::tempdir;

    use super::editing::{SyncApplyDisposition, SyncEditingTestRegistry};
    use super::model::{SyncConfig, SyncConfigLoadResponse, SyncConfigPatch, SyncProvider};
    use super::status::{
        status_for_failed_run, SyncCompletionState, SyncRunResult, SyncStatus, SyncSummary,
        SyncTrigger,
    };
    use super::storage::{
        config_path, enable_at_app_data, load_from_app_data, patch_at_app_data,
        recover_at_app_data, reset_at_app_data,
    };
    use super::SyncConfigChangedEvent;
    use super::{
        enforce_application_sync_editing_barrier, failed_bootstrap_commit_status,
        parse_patch_request, parse_recover_request, validate_sync_application_mode,
        validate_sync_application_notebook, validate_sync_application_result,
        validated_application_sync_apply_token, SyncApplicationRequest, SyncApplyCompletionGuard,
    };

    #[test]
    fn application_sync_request_name_must_match_the_immutable_canonical_root() {
        let directory = tempdir().unwrap();
        let notes = directory.path().join("  个人 笔记  ");
        fs::create_dir(&notes).unwrap();
        let canonical = notes.canonicalize().unwrap();
        let matching = SyncApplicationRequest {
            apply_token: None,
            bootstrap: false,
            notebook_name: Some("  个人 笔记  ".into()),
            notes_root: Some(canonical.to_string_lossy().into_owned()),
            prepared_target_lease: None,
            revision: "rev".into(),
            trigger: SyncTrigger::Manual,
        };
        assert_eq!(
            validate_sync_application_notebook(&matching, &canonical).unwrap(),
            "  个人 笔记  "
        );
        let matching_result = SyncRunResult {
            notebook_name: "  个人 笔记  ".into(),
            notes_root: canonical.to_string_lossy().into_owned(),
            provider: SyncProvider::Webdav,
            revision: "rev".into(),
            summary: SyncSummary::default(),
            trigger: SyncTrigger::Manual,
        };
        assert!(validate_sync_application_result(&matching, &canonical, matching_result).is_ok());
        let mismatched_result = SyncRunResult {
            notebook_name: "other".into(),
            notes_root: canonical.to_string_lossy().into_owned(),
            provider: SyncProvider::Webdav,
            revision: "rev".into(),
            summary: SyncSummary::default(),
            trigger: SyncTrigger::Manual,
        };
        assert!(
            validate_sync_application_result(&matching, &canonical, mismatched_result).is_err()
        );

        let mismatched = SyncApplicationRequest {
            notebook_name: Some("other".into()),
            ..matching
        };
        assert!(validate_sync_application_notebook(&mismatched, &canonical).is_err());
    }

    #[test]
    fn bootstrap_application_sync_derives_the_notebook_name_from_the_target_root() {
        let directory = tempdir().unwrap();
        let notes = directory.path().join("  个人 笔记  ");
        fs::create_dir(&notes).unwrap();
        let canonical = notes.canonicalize().unwrap();
        let bootstrap = SyncApplicationRequest {
            apply_token: None,
            bootstrap: true,
            notebook_name: None,
            notes_root: None,
            prepared_target_lease: Some("prepared-capability".into()),
            revision: "rev".into(),
            trigger: SyncTrigger::Manual,
        };

        assert_eq!(
            validate_sync_application_notebook(&bootstrap, &canonical).unwrap(),
            "  个人 笔记  "
        );
        assert!(validate_sync_application_mode(&bootstrap).is_ok());

        let supplied_identity = SyncApplicationRequest {
            notebook_name: Some("other".into()),
            ..bootstrap
        };
        assert!(validate_sync_application_mode(&supplied_identity).is_err());
    }

    #[test]
    fn failed_native_bootstrap_commit_replaces_the_premature_success_status() {
        let previous = SyncStatus::attempting(
            SyncProvider::Webdav,
            SyncTrigger::Manual,
            "2026-01-01T00:00:00Z".into(),
            None,
        )
        .for_run("/Workspace/B".into(), "B".into(), "rev".into())
        .succeeded("2026-01-01T00:00:01Z".into(), SyncSummary::default());
        let result = SyncRunResult {
            notebook_name: "B".into(),
            notes_root: "/Workspace/B".into(),
            provider: SyncProvider::Webdav,
            revision: "rev".into(),
            summary: SyncSummary {
                downloaded_files: 1,
                ..SyncSummary::default()
            },
            trigger: SyncTrigger::Manual,
        };

        let failed = failed_bootstrap_commit_status(
            &result,
            Some(&previous),
            "2026-01-01T00:00:02Z".into(),
            "notebook-target-invalid: replaced",
        );

        assert_eq!(failed.completion_state, SyncCompletionState::Failed);
        assert_eq!(failed.notes_root.as_deref(), Some("/Workspace/B"));
        assert_eq!(
            failed.error.as_ref().map(|error| error.code.as_str()),
            Some("notebook-target-invalid")
        );
        assert_eq!(
            failed.last_successful_sync_at.as_deref(),
            Some("2026-01-01T00:00:01Z")
        );
        assert_eq!(failed.summary, Some(result.summary));
    }

    #[test]
    fn application_sync_request_requires_exact_apply_token_semantics() {
        for trigger in [
            SyncTrigger::AppLaunch,
            SyncTrigger::Interval,
            SyncTrigger::Manual,
            SyncTrigger::Save,
        ] {
            assert!(validated_application_sync_apply_token(trigger, None)
                .unwrap()
                .is_none());
            assert_eq!(
                validated_application_sync_apply_token(trigger, Some("token")).unwrap_err(),
                "sync-apply-mismatch: Only a settings apply may use an apply token."
            );
        }
        assert_eq!(
            validated_application_sync_apply_token(SyncTrigger::SettingsExit, None).unwrap_err(),
            "sync-apply-unavailable: The sync settings apply is unavailable."
        );
        assert_eq!(
            validated_application_sync_apply_token(SyncTrigger::SettingsExit, Some("token")),
            Ok(Some("token"))
        );
    }

    #[test]
    fn application_sync_native_editing_barrier_only_blocks_automatic_triggers() {
        for trigger in [
            SyncTrigger::AppLaunch,
            SyncTrigger::Interval,
            SyncTrigger::Save,
        ] {
            assert_eq!(
                enforce_application_sync_editing_barrier(trigger, true).unwrap_err(),
                "sync-editing-active: Automatic sync is suspended while settings are being edited."
            );
        }
        for trigger in [SyncTrigger::Manual, SyncTrigger::SettingsExit] {
            enforce_application_sync_editing_barrier(trigger, true).unwrap();
        }
    }

    #[test]
    fn sync_config_is_written_below_app_data_only() {
        let app_data = tempdir().unwrap();
        let notes = tempdir().unwrap();
        fs::create_dir(notes.path().join(".qingyu")).unwrap();
        fs::create_dir(notes.path().join(".markra-sync")).unwrap();
        fs::write(notes.path().join(".qingyu/config.json"), b"legacy-secret").unwrap();
        fs::write(
            notes.path().join(".markra-sync/config.json"),
            b"legacy-secret",
        )
        .unwrap();

        let stored = enable_at_app_data(app_data.path(), None).unwrap();

        assert_eq!(
            config_path(app_data.path()),
            app_data.path().join("sync-config.json")
        );
        assert_eq!(stored.document.config.version, 2);
        assert!(!stored.document.config.enabled);
        assert_eq!(
            fs::read(notes.path().join(".qingyu/config.json")).unwrap(),
            b"legacy-secret"
        );
        assert_eq!(
            fs::read(notes.path().join(".markra-sync/config.json")).unwrap(),
            b"legacy-secret"
        );
    }

    #[test]
    fn absent_is_disabled_without_creating_a_file() {
        let app_data = tempdir().unwrap();

        let loaded = load_from_app_data(app_data.path()).unwrap();

        assert!(matches!(
            loaded,
            SyncConfigLoadResponse::Absent { revision: None }
        ));
        assert!(!config_path(app_data.path()).exists());
        assert!(!SyncConfig::default().enabled);
    }

    #[test]
    fn configured_snapshot_accepts_a_complete_disabled_provider_without_mutation() {
        let app_data = tempdir().unwrap();
        let mut config = SyncConfig {
            enabled: false,
            provider: SyncProvider::Webdav,
            ..SyncConfig::default()
        };
        config.remote_root = "root".into();
        config.webdav.server_url = "https://dav.example.test/base".into();
        fs::write(
            config_path(app_data.path()),
            serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
        let SyncConfigLoadResponse::Loaded { document } =
            load_from_app_data(app_data.path()).unwrap()
        else {
            panic!("disabled complete config should load");
        };
        let before = fs::read(config_path(app_data.path())).unwrap();

        let snapshot =
            super::configured_snapshot_at_app_data(app_data.path(), Some(&document.revision))
                .expect("catalog may read a complete disabled config");

        assert!(!snapshot.config.enabled);
        assert_eq!(snapshot.revision, document.revision);
        assert_eq!(fs::read(config_path(app_data.path())).unwrap(), before);
        assert!(!app_data.path().join("sync-state").exists());
    }

    #[test]
    fn configured_snapshot_still_requires_revision_and_complete_provider_fields() {
        let app_data = tempdir().unwrap();
        let mut config = SyncConfig {
            enabled: false,
            ..SyncConfig::default()
        };
        config.remote_root = "root".into();
        fs::write(
            config_path(app_data.path()),
            serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
        let SyncConfigLoadResponse::Loaded { document } =
            load_from_app_data(app_data.path()).unwrap()
        else {
            panic!("disabled incomplete config should load");
        };

        assert_eq!(
            super::configured_snapshot_at_app_data(app_data.path(), Some("stale-revision"))
                .err()
                .unwrap(),
            "revision-conflict: The sync configuration changed before this run."
        );
        assert_eq!(
            super::configured_snapshot_at_app_data(app_data.path(), Some(&document.revision))
                .err()
                .unwrap(),
            "sync-not-ready: The sync configuration is incomplete."
        );
    }

    #[test]
    fn patch_requires_the_current_revision() {
        let app_data = tempdir().unwrap();
        let enabled = enable_at_app_data(app_data.path(), None).unwrap();
        let patched = patch_at_app_data(
            app_data.path(),
            &enabled.document.revision,
            SyncConfigPatch::Provider(SyncProvider::Webdav),
        )
        .unwrap();

        assert_eq!(patched.document.config.provider, SyncProvider::Webdav);
        let error = patch_at_app_data(
            app_data.path(),
            &enabled.document.revision,
            SyncConfigPatch::RemoteRoot("other".into()),
        )
        .err()
        .unwrap();
        assert_eq!(error.code, "revision-conflict");
    }

    #[test]
    fn malformed_and_unsupported_configs_block_normal_edits() {
        let malformed_root = tempdir().unwrap();
        fs::write(config_path(malformed_root.path()), b"{broken").unwrap();
        assert!(matches!(
            load_from_app_data(malformed_root.path()).unwrap(),
            SyncConfigLoadResponse::Malformed { .. }
        ));

        let unsupported_root = tempdir().unwrap();
        fs::write(
            config_path(unsupported_root.path()),
            br#"{"version":1,"password":"do-not-echo"}"#,
        )
        .unwrap();
        assert!(matches!(
            load_from_app_data(unsupported_root.path()).unwrap(),
            SyncConfigLoadResponse::Unsupported { version: 1, .. }
        ));
    }

    #[test]
    fn malformed_config_reset_preserves_a_damaged_copy_under_app_data() {
        let app_data = tempdir().unwrap();
        let invalid = b"{broken-with-secret";
        fs::write(config_path(app_data.path()), invalid).unwrap();

        assert!(reset_at_app_data(app_data.path(), true, None).is_err());
        let loaded = load_from_app_data(app_data.path()).unwrap();
        let SyncConfigLoadResponse::Malformed { revision, .. } = loaded else {
            panic!("malformed content should remain after a stale reset");
        };
        reset_at_app_data(app_data.path(), true, Some(&revision)).unwrap();

        let damaged = fs::read_dir(app_data.path())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("sync-config.damaged-"))
            })
            .expect("damaged copy");
        assert_eq!(fs::read(damaged).unwrap(), invalid);
    }

    #[test]
    fn persisted_credentials_never_appear_in_safe_status_or_debug() {
        let mut config = SyncConfig::default();
        config.provider = SyncProvider::S3;
        config.s3.access_key_id = "access-value".into();
        config.s3.secret_access_key = "secret-value".into();
        config.webdav.password = "password-value".into();

        let serialized_status = serde_json::to_string(&status_for_failed_run(&config)).unwrap();
        let debug = format!("{:?}", status_for_failed_run(&config));
        for secret in ["access-value", "secret-value", "password-value"] {
            assert!(!serialized_status.contains(secret));
            assert!(!debug.contains(secret));
        }
    }

    #[test]
    fn editing_and_apply_registry_has_one_application_identity() {
        let mut registry = SyncEditingTestRegistry::default();
        registry.set(true, "session-a", Some("rev-a")).unwrap();
        registry.set(true, "session-b", Some("rev-b")).unwrap();

        let snapshot = registry.load();
        assert_eq!(snapshot.state.unwrap().session_id, "session-b");

        let first = registry
            .request_apply("session-b", "rev-b", "apply-1")
            .unwrap();
        let duplicate = registry
            .request_apply("session-b", "rev-b", "apply-1")
            .unwrap();
        assert_eq!(first.token, duplicate.token);
        assert_eq!(registry.pending_apply_count(), 1);
    }

    #[test]
    fn apply_token_is_claimed_and_completed_exactly_once() {
        let mut registry = SyncEditingTestRegistry::default();
        registry.set(true, "session", Some("rev")).unwrap();
        registry.request_apply("session", "rev", "token").unwrap();

        assert!(matches!(
            registry.begin_apply("rev", "token").unwrap(),
            SyncApplyDisposition::Execute
        ));
        assert!(matches!(
            registry.begin_apply("rev", "token").unwrap(),
            SyncApplyDisposition::Wait
        ));
        let outcome = SyncRunResult {
            notebook_name: "notes".into(),
            notes_root: "/notes".into(),
            provider: SyncProvider::Webdav,
            revision: "rev".into(),
            summary: SyncSummary::default(),
            trigger: SyncTrigger::SettingsExit,
        };
        registry
            .complete_apply("rev", "token", Ok(outcome))
            .unwrap();
        let SyncApplyDisposition::Completed(Ok(completed)) =
            registry.begin_apply("rev", "token").unwrap()
        else {
            panic!("completed token should replay its exact outcome");
        };
        assert_eq!(completed.revision, "rev");
    }

    #[tokio::test]
    async fn claimed_apply_worker_panic_completes_every_waiter_with_the_same_safe_error() {
        let registry = Arc::new(Mutex::new(SyncEditingTestRegistry::default()));
        let mut waiter = {
            let mut registry = registry.lock().unwrap();
            registry.set(true, "session", Some("rev-panic")).unwrap();
            registry
                .request_apply("session", "rev-panic", "token-panic")
                .unwrap();
            assert!(matches!(
                registry.begin_apply("rev-panic", "token-panic").unwrap(),
                SyncApplyDisposition::Execute
            ));
            registry
                .subscribe_apply("rev-panic", "token-panic")
                .unwrap()
        };
        let completion_registry = Arc::clone(&registry);
        let guard = SyncApplyCompletionGuard::with_completion(
            "rev-panic".into(),
            "token-panic".into(),
            Box::new(move |revision, token, outcome| {
                completion_registry
                    .lock()
                    .unwrap()
                    .complete_apply(revision, token, outcome)
            }),
        );

        let worker = tokio::spawn(async move {
            let _guard = guard;
            panic!("injected application sync worker panic");
        });
        assert!(worker.await.is_err());

        let waiter_outcome = loop {
            if let Some(outcome) = waiter.borrow_and_update().clone() {
                break outcome;
            }
            waiter.changed().await.unwrap();
        };
        let expected =
            Err("sync-failed: Application sync execution stopped unexpectedly.".to_string());
        assert_eq!(waiter_outcome, expected);
        let replay = registry
            .lock()
            .unwrap()
            .begin_apply("rev-panic", "token-panic")
            .unwrap();
        let SyncApplyDisposition::Completed(replayed_outcome) = replay else {
            panic!("panicked worker must leave the apply token completed");
        };
        assert_eq!(replayed_outcome, expected);
    }

    #[test]
    fn apply_completion_callback_error_is_not_retried_by_the_guard_drop() {
        let calls = Arc::new(AtomicUsize::new(0));
        let completion_calls = Arc::clone(&calls);
        let guard = SyncApplyCompletionGuard::with_completion(
            "rev-error".into(),
            "token-error".into(),
            Box::new(move |_, _, _| {
                completion_calls.fetch_add(1, Ordering::Relaxed);
                Err("sync-editing-state-unavailable: synthetic completion failure".into())
            }),
        );

        let error = guard
            .complete(Err("sync-failed: synthetic execution failure".into()))
            .unwrap_err();

        assert_eq!(
            error,
            "sync-editing-state-unavailable: synthetic completion failure"
        );
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn config_change_event_contains_only_the_revision() {
        let event = SyncConfigChangedEvent {
            revision: "rev-safe".into(),
        };
        assert_eq!(
            serde_json::to_value(event).unwrap(),
            serde_json::json!({ "revision": "rev-safe" })
        );
    }

    #[test]
    fn installed_documents_remain_authoritative_when_change_notification_fails() {
        fn assert_installed(
            app_data: &std::path::Path,
            outcome: super::storage::SyncConfigWriteOutcome,
        ) {
            let expected_revision = outcome.document.revision.clone();
            let document =
                super::finish_installed_write(outcome, |_| Err("event unavailable".to_string()));
            assert_eq!(document.revision, expected_revision);
            let SyncConfigLoadResponse::Loaded { document: loaded } =
                load_from_app_data(app_data).unwrap()
            else {
                panic!("installed config should remain loaded");
            };
            assert_eq!(loaded.revision, expected_revision);
        }

        let enabled_root = tempdir().unwrap();
        assert_installed(
            enabled_root.path(),
            enable_at_app_data(enabled_root.path(), None).unwrap(),
        );

        let patched_root = tempdir().unwrap();
        let enabled = enable_at_app_data(patched_root.path(), None).unwrap();
        assert_installed(
            patched_root.path(),
            patch_at_app_data(
                patched_root.path(),
                &enabled.document.revision,
                SyncConfigPatch::RemoteRoot("patched".into()),
            )
            .unwrap(),
        );

        let recovered_root = tempdir().unwrap();
        fs::write(config_path(recovered_root.path()), b"{invalid").unwrap();
        let SyncConfigLoadResponse::Malformed { revision, .. } =
            load_from_app_data(recovered_root.path()).unwrap()
        else {
            panic!("invalid fixture should be malformed");
        };
        assert_installed(
            recovered_root.path(),
            recover_at_app_data(recovered_root.path(), &revision, SyncConfig::default()).unwrap(),
        );

        let reset_root = tempdir().unwrap();
        let enabled = enable_at_app_data(reset_root.path(), None).unwrap();
        assert_installed(
            reset_root.path(),
            reset_at_app_data(reset_root.path(), true, Some(&enabled.document.revision)).unwrap(),
        );
    }

    #[test]
    fn credential_bearing_mutation_commands_parse_opaque_payloads_with_fixed_errors() {
        let source = include_str!("sync_config.rs");
        for command in ["patch_sync_config", "recover_sync_config"] {
            let command_source = &source[source
                .find(&format!("pub(crate) fn {command}("))
                .expect("command should exist")..];
            assert!(
                command_source.contains("request: serde_json::Value"),
                "{command} must parse credential-bearing payloads inside the command"
            );
        }

        let patch = serde_json::json!({
            "expectedRevision": "rev",
            "patch": { "field": "provider", "value": "private-provider-secret" }
        });
        let patch_error = parse_patch_request(patch).err().unwrap();
        assert_eq!(
            patch_error,
            "sync-config-invalid-patch: Submit a supported sync configuration field update."
        );
        assert!(!patch_error.contains("private-provider-secret"));

        let mut recovery = serde_json::json!({
            "config": SyncConfig::default(),
            "expectedRevision": "rev"
        });
        recovery["config"]["provider"] = serde_json::json!("private-provider-secret");
        let recovery_error = parse_recover_request(recovery).err().unwrap();
        assert_eq!(
            recovery_error,
            "sync-config-invalid-draft: Submit a complete supported sync configuration."
        );
        assert!(!recovery_error.contains("private-provider-secret"));
    }
}
