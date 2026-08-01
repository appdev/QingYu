pub(crate) mod editing;
pub(crate) mod model;
pub(crate) mod status;
pub(crate) mod storage;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};

use editing::{
    cancel_sync_apply as cancel_sync_apply_in_registry, complete_sync_apply,
    load_sync_editing_state, request_sync_apply_with_notify, set_sync_editing_with_notify,
    CancelSyncConfigApplyRequest, RequestSyncConfigApply, SetSyncConfigEditingRequest,
    SyncEditingEvent, SyncEditingSnapshot, SyncPendingApply,
};
use model::{
    RecoverSyncConfigRequest, ResetSyncConfigRequest, SyncConfigDocument, SyncConfigLoadResponse,
    SyncConfigPatchRequest, SyncConfigReadiness, SyncConnectionTestResult, SyncSnapshot,
    SyncTarget,
};
use status::{load_sync_status_at_app_data, SyncRunResult, SyncStatus, SyncSummary, SyncTrigger};
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
pub(crate) struct KernelSyncApplySettlementRequest {
    outcome: KernelSyncApplySettlementOutcome,
    revision: String,
    token: String,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "lowercase", tag = "status")]
enum KernelSyncApplySettlementOutcome {
    Completed {
        result: KernelSyncApplySettlementRunResult,
    },
    Failed,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct KernelSyncApplySettlementRunResult {
    notebook_name: String,
    notes_root: String,
    provider: model::SyncProvider,
    revision: String,
    summary: KernelSyncApplySettlementSummary,
    trigger: SyncTrigger,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct KernelSyncApplySettlementSummary {
    bytes_downloaded: u64,
    bytes_uploaded: u64,
    conflict_files: u64,
    downloaded_files: u64,
    scanned_files: u64,
    skipped_files: u64,
    uploaded_files: u64,
}

impl From<KernelSyncApplySettlementRunResult> for SyncRunResult {
    fn from(result: KernelSyncApplySettlementRunResult) -> Self {
        Self {
            notebook_name: result.notebook_name,
            notes_root: result.notes_root,
            provider: result.provider,
            revision: result.revision,
            summary: SyncSummary {
                bytes_downloaded: result.summary.bytes_downloaded,
                bytes_uploaded: result.summary.bytes_uploaded,
                conflict_files: result.summary.conflict_files,
                downloaded_files: result.summary.downloaded_files,
                scanned_files: result.summary.scanned_files,
                skipped_files: result.summary.skipped_files,
                uploaded_files: result.summary.uploaded_files,
            },
            trigger: result.trigger,
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "lowercase", tag = "status")]
pub(crate) enum SyncDispatchResult {
    Accepted {
        job: crate::dejavu_sync::service::AcceptedSyncJob,
    },
    Completed {
        result: SyncRunResult,
    },
}

impl std::fmt::Debug for SyncDispatchResult {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Accepted { job } => formatter
                .debug_struct("Accepted")
                .field("job_id", &job.job_id)
                .field("repository_id", &job.repository_id)
                .field("notes_root", &job.notes_root)
                .finish(),
            Self::Completed { result } => formatter
                .debug_struct("Completed")
                .field("result", result)
                .finish(),
        }
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
pub(crate) fn settle_kernel_sync_config_apply(
    request: KernelSyncApplySettlementRequest,
) -> Result<(), String> {
    let (revision, token, outcome) = validated_kernel_sync_apply_settlement(request)?;
    complete_sync_apply(&revision, &token, outcome)
}

fn validated_kernel_sync_apply_settlement(
    request: KernelSyncApplySettlementRequest,
) -> Result<(String, String, Result<SyncDispatchResult, String>), String> {
    let outcome = match request.outcome {
        KernelSyncApplySettlementOutcome::Completed { result } => {
            if result.revision != request.revision || result.trigger != SyncTrigger::SettingsExit {
                return Err(
                    "sync-apply-mismatch: The sync settings apply identity changed.".to_string(),
                );
            }
            Ok(SyncDispatchResult::Completed {
                result: result.into(),
            })
        }
        KernelSyncApplySettlementOutcome::Failed => Err(
            "kernel-sync-apply-failed: The Kernel sync settings apply did not complete."
                .to_string(),
        ),
    };
    Ok((request.revision, request.token, outcome))
}

#[tauri::command]
pub(crate) fn load_sync_status(app: tauri::AppHandle) -> Result<Option<SyncStatus>, String> {
    load_sync_status_at_app_data(&app_data_dir(&app)?)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::{json, Value};
    use tempfile::tempdir;

    fn kernel_sync_apply_settlement_json() -> Value {
        json!({
            "outcome": {
                "status": "completed",
                "result": {
                    "notebookName": "Notes",
                    "notesRoot": "kernel-workspace://primary",
                    "provider": "s3",
                    "revision": "revision-1",
                    "summary": {
                        "bytesDownloaded": 1,
                        "bytesUploaded": 2,
                        "conflictFiles": 0,
                        "downloadedFiles": 1,
                        "scannedFiles": 3,
                        "skippedFiles": 0,
                        "uploadedFiles": 2
                    },
                    "trigger": "settings-exit"
                }
            },
            "revision": "revision-1",
            "token": "apply-1"
        })
    }

    #[test]
    fn kernel_sync_apply_settlement_dto_rejects_unknown_fields_at_every_level() {
        for pointer in ["", "/outcome", "/outcome/result", "/outcome/result/summary"] {
            let mut value = kernel_sync_apply_settlement_json();
            value
                .pointer_mut(pointer)
                .and_then(Value::as_object_mut)
                .expect("settlement fixture level should be an object")
                .insert("unexpected".into(), Value::Bool(true));
            assert!(serde_json::from_value::<KernelSyncApplySettlementRequest>(value).is_err());
        }
    }

    #[test]
    fn kernel_sync_apply_settlement_requires_matching_revision_and_settings_exit() {
        let mut mismatched_revision = kernel_sync_apply_settlement_json();
        mismatched_revision["outcome"]["result"]["revision"] = json!("revision-2");
        let request = serde_json::from_value(mismatched_revision).unwrap();
        assert!(validated_kernel_sync_apply_settlement(request)
            .unwrap_err()
            .starts_with("sync-apply-mismatch:"));

        let mut wrong_trigger = kernel_sync_apply_settlement_json();
        wrong_trigger["outcome"]["result"]["trigger"] = json!("manual");
        let request = serde_json::from_value(wrong_trigger).unwrap();
        assert!(validated_kernel_sync_apply_settlement(request)
            .unwrap_err()
            .starts_with("sync-apply-mismatch:"));
    }

    use super::editing::{SyncApplyDisposition, SyncEditingTestRegistry};
    use super::model::{SyncConfig, SyncConfigLoadResponse, SyncConfigPatch, SyncProvider};
    use super::status::{status_for_failed_run, SyncRunResult, SyncSummary, SyncTrigger};
    use super::storage::{
        config_path, enable_at_app_data, load_from_app_data, patch_at_app_data,
        recover_at_app_data, reset_at_app_data,
    };
    use super::SyncConfigChangedEvent;
    use super::{
        parse_patch_request, parse_recover_request, validated_kernel_sync_apply_settlement,
        KernelSyncApplySettlementRequest, SyncDispatchResult,
    };

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
        assert_eq!(stored.document.config.version, 3);
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
            .complete_apply(
                "rev",
                "token",
                Ok(SyncDispatchResult::Completed { result: outcome }),
            )
            .unwrap();
        let SyncApplyDisposition::Completed(Ok(SyncDispatchResult::Completed {
            result: completed,
        })) = registry.begin_apply("rev", "token").unwrap()
        else {
            panic!("completed token should replay its exact outcome");
        };
        assert_eq!(completed.revision, "rev");
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
