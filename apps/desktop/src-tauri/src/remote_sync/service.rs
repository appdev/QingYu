use std::{
    future::Future,
    path::{Path, PathBuf},
    time::Instant,
};

use cap_std::fs::Dir;
use tauri::{Manager, Runtime};

use super::backend::{
    sync_state_key, RemoteSyncBackend, RemoteSyncDiagnostic, RemoteSyncError, ValidRemoteRoot,
};
use super::diagnostics::{
    create_sync_run_id, record_sync_failed, record_sync_started, record_sync_succeeded,
    SyncDiagnosticContext,
};
#[cfg(test)]
use super::engine::execute_remote_sync_pair;
use super::engine::{
    complete_remote_first_restore_locked, execute_remote_sync_pair_locked,
    preserve_remote_settings_conflict, with_remote_sync_execution_lock, RemoteSyncSummary,
    SettingsSyncOutcome, MAX_IMMEDIATE_RECHECK_PASSES,
};
use super::s3_backend::{S3Backend, S3SyncSettings, S3TransportOptions};
use super::scope::RemoteSyncScope;
use super::settings_scope::{
    capture_portable_settings_manifest_revision, capture_settings_file_state,
    clear_portable_settings_manifest, clear_portable_settings_pending,
    portable_settings_pending_contains_legacy_mcp, read_portable_settings_pending,
    replace_portable_settings_stage, write_portable_settings_pending, PortableSettingsJournal,
    PortableSettingsJournalPhase,
};
use super::{create_webdav_backend_at_validated_prefix, WebDavSyncSettings};
use crate::app_settings::{
    portable_settings_from_bytes, sanitize_legacy_remote_portable_settings, AppSettingsError,
    AppSettingsService, DeferredSettingsPublication, SettingsPublicationEvent,
};
use crate::notebook_scope::{
    notebook_name_from_root, notes_remote_prefix, resolve_notebook_sync_scope,
    resolve_notebook_sync_scope_from_canonical, NotebookSyncScope,
};
use crate::sync_config::model::{SyncConfig, SyncProvider, SyncSnapshot, SyncTarget};
use crate::sync_config::status::{
    emit_sync_status_changed, load_sync_status_at_app_data, sync_status_timestamp,
    write_sync_status_at_app_data, SyncRunResult, SyncSafeError, SyncStatus, SyncSummary,
    SyncTrigger,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SyncRunError {
    code: String,
    diagnostic: Option<RemoteSyncDiagnostic>,
    partial_summary: SyncSummary,
    revision: String,
    trigger: SyncTrigger,
}

struct PreparedPortableSettingsSync {
    expected_portable_revision: String,
    publication_events: Vec<SettingsPublicationEvent>,
    phase: PortableSettingsJournalPhase,
    scope: RemoteSyncScope,
}

impl PreparedPortableSettingsSync {
    fn scope(&self) -> &RemoteSyncScope {
        &self.scope
    }
}

impl SyncRunError {
    #[cfg(test)]
    pub(crate) fn code(&self) -> &str {
        &self.code
    }

    #[cfg(test)]
    pub(crate) fn partial_summary(&self) -> &SyncSummary {
        &self.partial_summary
    }

    #[cfg(test)]
    pub(crate) fn revision(&self) -> &str {
        &self.revision
    }
}

impl std::fmt::Display for SyncRunError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{}: Application synchronization did not complete.",
            self.code
        )
    }
}

impl std::error::Error for SyncRunError {}

enum ApplicationSyncSource {
    Regular,
    #[cfg(any(mobile, test))]
    ManagedBootstrap(Dir),
    #[cfg(not(mobile))]
    PreparedDirectory {
        directory: Dir,
        restore_generation: String,
    },
}

impl ApplicationSyncSource {
    fn retains_source_directory(&self) -> bool {
        match self {
            Self::Regular => false,
            #[cfg(any(mobile, test))]
            Self::ManagedBootstrap(_) => true,
            #[cfg(not(mobile))]
            Self::PreparedDirectory { .. } => true,
        }
    }
}

pub(crate) async fn run_application_sync<R: Runtime>(
    app: &tauri::AppHandle<R>,
    notes_root: PathBuf,
    snapshot: SyncSnapshot,
    trigger: SyncTrigger,
) -> Result<SyncRunResult, SyncRunError> {
    run_application_sync_with_source(
        app,
        notes_root,
        ApplicationSyncSource::Regular,
        snapshot,
        trigger,
    )
    .await
}

#[cfg(mobile)]
pub(crate) async fn run_application_sync_from_managed_bootstrap<R: Runtime>(
    app: &tauri::AppHandle<R>,
    notes_root: PathBuf,
    source_directory: Dir,
    snapshot: SyncSnapshot,
    trigger: SyncTrigger,
) -> Result<SyncRunResult, SyncRunError> {
    run_application_sync_with_source(
        app,
        notes_root,
        ApplicationSyncSource::ManagedBootstrap(source_directory),
        snapshot,
        trigger,
    )
    .await
}

#[cfg(not(mobile))]
pub(crate) async fn run_application_sync_from_prepared_directory<R: Runtime>(
    app: &tauri::AppHandle<R>,
    notes_root: PathBuf,
    source_directory: Dir,
    restore_generation: String,
    snapshot: SyncSnapshot,
    trigger: SyncTrigger,
) -> Result<SyncRunResult, SyncRunError> {
    run_application_sync_with_source(
        app,
        notes_root,
        ApplicationSyncSource::PreparedDirectory {
            directory: source_directory,
            restore_generation,
        },
        snapshot,
        trigger,
    )
    .await
}

async fn run_application_sync_with_source<R: Runtime>(
    app: &tauri::AppHandle<R>,
    notes_root: PathBuf,
    source: ApplicationSyncSource,
    snapshot: SyncSnapshot,
    trigger: SyncTrigger,
) -> Result<SyncRunResult, SyncRunError> {
    let run_id = create_sync_run_id();
    let started_at = Instant::now();
    let provider = snapshot.config.provider;
    record_sync_started(
        &run_id,
        provider,
        trigger,
        source.retains_source_directory(),
    );
    let app_data = app.path().app_data_dir().map_err(|_| {
        run_error(
            "app-data-unavailable",
            &snapshot.revision,
            trigger,
            SyncSummary::default(),
        )
    })?;
    let revision = snapshot.revision.clone();
    let notes_root_identity = notes_root.to_string_lossy().into_owned();
    let notebook_name = notebook_name_from_root(&notes_root).map_err(|error| {
        run_error(
            safe_error_code(&error),
            &revision,
            trigger,
            SyncSummary::default(),
        )
    })?;
    let previous = load_sync_status_at_app_data(&app_data).map_err(|error| {
        run_error(
            safe_error_code(&error),
            &revision,
            trigger,
            SyncSummary::default(),
        )
    })?;
    let attempting = SyncStatus::attempting_for_run(
        provider,
        trigger,
        sync_status_timestamp(),
        notes_root_identity.clone(),
        notebook_name,
        revision.clone(),
        previous.as_ref(),
    );
    persist_status(app, &app_data, &notes_root_identity, &revision, &attempting).map_err(
        |error| {
            run_error(
                safe_error_code(&error),
                &revision,
                trigger,
                SyncSummary::default(),
            )
        },
    )?;

    match run_application_sync_inner(app, notes_root, source, snapshot, trigger, &run_id).await {
        Ok(result) => {
            let succeeded = attempting.succeeded(sync_status_timestamp(), result.summary.clone());
            persist_status(app, &app_data, &notes_root_identity, &revision, &succeeded).map_err(
                |error| {
                    run_error(
                        safe_error_code(&error),
                        &revision,
                        trigger,
                        result.summary.clone(),
                    )
                },
            )?;
            record_sync_succeeded(
                &run_id,
                provider,
                trigger,
                &result.summary,
                started_at.elapsed(),
            );
            Ok(result)
        }
        Err(error) => {
            let failed = attempting.failed(
                sync_safe_error(&error, provider, &run_id),
                error.partial_summary.clone(),
            );
            persist_status(app, &app_data, &notes_root_identity, &revision, &failed).map_err(
                |status_error| {
                    run_error(
                        safe_error_code(&status_error),
                        &revision,
                        trigger,
                        error.partial_summary.clone(),
                    )
                },
            )?;
            record_sync_failed(
                &run_id,
                provider,
                trigger,
                &error.code,
                error.diagnostic.as_ref(),
                &error.partial_summary,
                started_at.elapsed(),
            );
            Err(error)
        }
    }
}

fn persist_status<R: Runtime>(
    app: &tauri::AppHandle<R>,
    app_data: &Path,
    notes_root: &str,
    revision: &str,
    status: &SyncStatus,
) -> Result<(), String> {
    write_sync_status_at_app_data(app_data, status)?;
    let _notification = emit_sync_status_changed(app, notes_root, revision, status);
    Ok(())
}

async fn run_application_sync_inner<R: Runtime>(
    app: &tauri::AppHandle<R>,
    notes_root: PathBuf,
    source: ApplicationSyncSource,
    snapshot: SyncSnapshot,
    trigger: SyncTrigger,
    run_id: &str,
) -> Result<SyncRunResult, SyncRunError> {
    let app_data = app.path().app_data_dir().map_err(|_| {
        run_error(
            "app-data-unavailable",
            &snapshot.revision,
            trigger,
            SyncSummary::default(),
        )
    })?;
    let expected_state_root = app_data.join("sync-state");
    if snapshot.state_root != expected_state_root {
        return Err(run_error(
            "sync-state-mismatch",
            &snapshot.revision,
            trigger,
            SyncSummary::default(),
        ));
    }
    let canonical_notes = if source.retains_source_directory() {
        notes_root
    } else {
        notes_root.canonicalize().map_err(|_| {
            run_error(
                "notes-root-unavailable",
                &snapshot.revision,
                trigger,
                SyncSummary::default(),
            )
        })?
    };
    let remote_root = ValidRemoteRoot::parse(&snapshot.config.remote_root).map_err(|error| {
        run_error(
            safe_error_code(&error),
            &snapshot.revision,
            trigger,
            SyncSummary::default(),
        )
    })?;
    validate_snapshot_target(&snapshot.config, &snapshot.target, &remote_root).map_err(|()| {
        run_error(
            "sync-snapshot-mismatch",
            &snapshot.revision,
            trigger,
            SyncSummary::default(),
        )
    })?;
    let settings_service = AppSettingsService::from_app(app).map_err(|error| {
        run_error(
            safe_error_code(&error.to_string()),
            &snapshot.revision,
            trigger,
            SyncSummary::default(),
        )
    })?;
    let global_ignore_rules = settings_service.file_ignore_rules().map_err(|error| {
        run_error(
            safe_error_code(&error.to_string()),
            &snapshot.revision,
            trigger,
            SyncSummary::default(),
        )
    })?;
    let provider = snapshot.config.provider;
    let notebook_name = notebook_name_from_root(&canonical_notes).map_err(|error| {
        run_error(
            safe_error_code(&error),
            &snapshot.revision,
            trigger,
            SyncSummary::default(),
        )
    })?;
    let notes_prefix = notes_remote_prefix(&remote_root, &notebook_name).map_err(|error| {
        run_error(
            safe_error_code(&error),
            &snapshot.revision,
            trigger,
            SyncSummary::default(),
        )
    })?;
    let settings_prefix = remote_root.app_prefix();
    match snapshot.target {
        SyncTarget::Webdav {
            remote_root: _,
            server_url,
            username,
            password,
        } => {
            let notes_backend = create_webdav_backend_at_validated_prefix(WebDavSyncSettings {
                password: password.clone(),
                remote_path: notes_prefix.clone(),
                server_url: server_url.clone(),
                username: username.clone(),
            })
            .await
            .map_err(|error| {
                run_error(
                    safe_error_code(&error),
                    &snapshot.revision,
                    trigger,
                    SyncSummary::default(),
                )
            })?;
            let settings_backend = create_webdav_backend_at_validated_prefix(WebDavSyncSettings {
                password,
                remote_path: settings_prefix,
                server_url,
                username,
            })
            .await
            .map_err(|error| {
                run_error(
                    safe_error_code(&error),
                    &snapshot.revision,
                    trigger,
                    SyncSummary::default(),
                )
            })?;
            let (notebook, notes_scope, settings_state) = build_sync_scopes(
                &canonical_notes,
                &snapshot.state_root,
                &remote_root,
                &notes_backend,
                &settings_backend,
                global_ignore_rules,
                source,
            )
            .map_err(|error| {
                run_error(
                    safe_error_code(&error),
                    &snapshot.revision,
                    trigger,
                    SyncSummary::default(),
                )
            })?;
            if notebook.name != notebook_name || notebook.remote_prefix != notes_prefix {
                return Err(run_error(
                    "sync-notebook-scope-mismatch",
                    &snapshot.revision,
                    trigger,
                    SyncSummary::default(),
                ));
            }
            run_prepared_scoped_sync_pair(
                &snapshot.revision,
                provider,
                trigger,
                &app_data,
                &notes_scope,
                &notes_backend,
                settings_state,
                &settings_backend,
                &settings_service,
                || async { Ok(()) },
            )
            .await
        }
        SyncTarget::S3 {
            access_key_id,
            addressing_style,
            bucket,
            endpoint_url,
            region,
            remote_root: _,
            request_timeout_seconds,
            secret_access_key,
            tls_verification,
        } => {
            let transport = S3TransportOptions {
                addressing_style,
                request_timeout_seconds,
                tls_verification,
            };
            let notes_backend = S3Backend::new_at_validated_prefix_with_transport(
                S3SyncSettings {
                    access_key_id: access_key_id.clone(),
                    bucket: bucket.clone(),
                    endpoint_url: endpoint_url.clone(),
                    region: region.clone(),
                    remote_path: notes_prefix.clone(),
                    secret_access_key: secret_access_key.clone(),
                },
                transport,
            )
            .map_err(|error| {
                run_error(
                    safe_error_code(&error),
                    &snapshot.revision,
                    trigger,
                    SyncSummary::default(),
                )
            })?
            .with_diagnostic_context(SyncDiagnosticContext::new(run_id, "notes"));
            let settings_backend = S3Backend::new_at_validated_prefix_with_transport(
                S3SyncSettings {
                    access_key_id,
                    bucket,
                    endpoint_url,
                    region,
                    remote_path: settings_prefix,
                    secret_access_key,
                },
                transport,
            )
            .map_err(|error| {
                run_error(
                    safe_error_code(&error),
                    &snapshot.revision,
                    trigger,
                    SyncSummary::default(),
                )
            })?
            .with_diagnostic_context(SyncDiagnosticContext::new(run_id, "settings"));
            let (notebook, notes_scope, settings_state) = build_sync_scopes(
                &canonical_notes,
                &snapshot.state_root,
                &remote_root,
                &notes_backend,
                &settings_backend,
                global_ignore_rules,
                source,
            )
            .map_err(|error| {
                run_error(
                    safe_error_code(&error),
                    &snapshot.revision,
                    trigger,
                    SyncSummary::default(),
                )
            })?;
            if notebook.name != notebook_name || notebook.remote_prefix != notes_prefix {
                return Err(run_error(
                    "sync-notebook-scope-mismatch",
                    &snapshot.revision,
                    trigger,
                    SyncSummary::default(),
                ));
            }
            run_prepared_scoped_sync_pair(
                &snapshot.revision,
                provider,
                trigger,
                &app_data,
                &notes_scope,
                &notes_backend,
                settings_state,
                &settings_backend,
                &settings_service,
                || async { Ok(()) },
            )
            .await
        }
    }
}

fn build_sync_scopes<NotesBackend, SettingsBackend>(
    canonical_notes: &Path,
    sync_state_root: &Path,
    remote_root: &ValidRemoteRoot,
    notes_backend: &NotesBackend,
    settings_backend: &SettingsBackend,
    global_ignore_rules: Option<String>,
    source: ApplicationSyncSource,
) -> Result<(NotebookSyncScope, RemoteSyncScope, PathBuf), String>
where
    NotesBackend: RemoteSyncBackend,
    SettingsBackend: RemoteSyncBackend,
{
    let notebook = if source.retains_source_directory() {
        resolve_notebook_sync_scope_from_canonical(
            &notes_backend.target_fingerprint_source(),
            remote_root,
            canonical_notes.to_path_buf(),
            sync_state_root,
        )?
    } else {
        resolve_notebook_sync_scope(
            &notes_backend.target_fingerprint_source(),
            remote_root,
            canonical_notes,
            sync_state_root,
        )?
    };
    let local_identity = Some(notebook.canonical_root.to_string_lossy().into_owned());
    let notes_scope = match source {
        #[cfg(not(mobile))]
        ApplicationSyncSource::PreparedDirectory {
            directory,
            restore_generation,
        } => RemoteSyncScope::notes_from_prepared_directory_with_restore_generation(
            notebook.canonical_root.clone(),
            directory,
            &notebook.state_root,
            "manifest.json",
            local_identity,
            global_ignore_rules,
            restore_generation,
        )?,
        #[cfg(any(mobile, test))]
        ApplicationSyncSource::ManagedBootstrap(directory) => {
            RemoteSyncScope::notes_from_managed_bootstrap(
                notebook.canonical_root.clone(),
                directory,
                &notebook.state_root,
                "manifest.json",
                local_identity,
                global_ignore_rules,
            )?
        }
        ApplicationSyncSource::Regular => RemoteSyncScope::notes(
            &notebook.canonical_root,
            &notebook.state_root,
            "manifest.json",
            local_identity,
            global_ignore_rules,
        )?,
    };
    let settings_state = settings_state_root(
        sync_state_root,
        &settings_backend.target_fingerprint_source(),
        remote_root.as_str(),
    );
    Ok((notebook, notes_scope, settings_state))
}

fn prepare_portable_settings_sync(
    service: &AppSettingsService,
    app_data: &Path,
    settings_state: PathBuf,
    manifest_name: &str,
) -> Result<PreparedPortableSettingsSync, String> {
    prepare_portable_settings_sync_with_conflict_preserver(
        service,
        app_data,
        settings_state,
        manifest_name,
        preserve_remote_settings_conflict,
    )
}

fn prepare_portable_settings_sync_with_conflict_preserver<PreserveConflict>(
    service: &AppSettingsService,
    app_data: &Path,
    settings_state: PathBuf,
    manifest_name: &str,
    preserve_conflict: PreserveConflict,
) -> Result<PreparedPortableSettingsSync, String>
where
    PreserveConflict: Fn(&RemoteSyncScope, Option<&[u8]>) -> Result<PathBuf, String>,
{
    let scope = RemoteSyncScope::portable_settings(app_data, settings_state, manifest_name)?;
    if portable_settings_pending_contains_legacy_mcp(&scope)? {
        clear_portable_settings_pending(&scope)?;
        clear_portable_settings_manifest(&scope)?;
    }
    let snapshot = service
        .portable_settings_snapshot()
        .map_err(|error| error.to_string())?;
    if let Some(mut journal) = read_portable_settings_pending(&scope)? {
        if journal.phase == PortableSettingsJournalPhase::Prepared
            && capture_portable_settings_manifest_revision(&scope)?
                != journal.prepared_manifest_revision
        {
            let checkpointed = capture_settings_file_state(scope.source_root())?;
            if checkpointed.bytes() != journal.staged_bytes()?.as_deref() {
                journal.phase = PortableSettingsJournalPhase::Reconcile;
                journal.set_staged_bytes(checkpointed.bytes());
                write_portable_settings_pending(&scope, &journal)?;
            }
        }
        match journal.phase {
            PortableSettingsJournalPhase::Prepared
                if snapshot.revision() != journal.expected_portable_revision => {}
            PortableSettingsJournalPhase::Publication
                if journal.applied_portable_revision.as_deref() != Some(snapshot.revision()) => {}
            PortableSettingsJournalPhase::Reconcile
                if snapshot.revision() != journal.expected_portable_revision =>
            {
                if journal.applied_portable_revision.as_deref() == Some(snapshot.revision()) {
                    let mut publication = journal;
                    publication.phase = PortableSettingsJournalPhase::Publication;
                    write_portable_settings_pending(&scope, &publication)?;
                    let staged = publication.staged_bytes()?;
                    restore_portable_settings_stage(&scope, staged.as_deref())?;
                    return Ok(PreparedPortableSettingsSync {
                        expected_portable_revision: publication.expected_portable_revision,
                        publication_events: publication.publication_events,
                        phase: publication.phase,
                        scope,
                    });
                }
                let staged = journal.staged_bytes()?;
                preserve_conflict(&scope, staged.as_deref())?;
            }
            _ => {
                let staged = journal.staged_bytes()?;
                restore_portable_settings_stage(&scope, staged.as_deref())?;
                return Ok(PreparedPortableSettingsSync {
                    expected_portable_revision: journal.expected_portable_revision,
                    publication_events: journal.publication_events,
                    phase: journal.phase,
                    scope,
                });
            }
        }
    }
    let mut journal = PortableSettingsJournal::prepared(snapshot.revision(), snapshot.bytes());
    journal.prepared_manifest_revision = capture_portable_settings_manifest_revision(&scope)?;
    write_portable_settings_pending(&scope, &journal)?;
    restore_portable_settings_stage(&scope, snapshot.bytes())?;
    Ok(PreparedPortableSettingsSync {
        expected_portable_revision: journal.expected_portable_revision,
        publication_events: Vec::new(),
        phase: PortableSettingsJournalPhase::Prepared,
        scope,
    })
}

fn restore_portable_settings_stage(
    scope: &RemoteSyncScope,
    staged: Option<&[u8]>,
) -> Result<(), String> {
    replace_portable_settings_stage(scope, staged)
}

fn remote_identity_changed(error: &RemoteSyncError) -> bool {
    error.safe_code() == "s3-object-changed"
        || error
            .to_string()
            .starts_with("Remote sync file changed during sync:")
}

async fn sanitize_legacy_remote_settings<Backend: RemoteSyncBackend>(
    scope: &RemoteSyncScope,
    backend: &Backend,
) -> Result<bool, RemoteSyncError> {
    let mut last_change = None;
    for _ in 0..MAX_IMMEDIATE_RECHECK_PASSES {
        let files = backend.list_files().await?;
        let Some(remote) = files.get("settings.json") else {
            return Ok(false);
        };
        let bytes = match backend.download("settings.json", &remote.identity).await {
            Ok(bytes) => bytes,
            Err(error) if remote_identity_changed(&error) => {
                last_change = Some(error);
                continue;
            }
            Err(error) => return Err(error),
        };
        let Some(sanitized) = sanitize_legacy_remote_portable_settings(&bytes)
            .ok()
            .flatten()
        else {
            return Ok(false);
        };
        clear_portable_settings_manifest(scope).map_err(RemoteSyncError::from)?;
        match backend
            .upload("settings.json", &sanitized, Some(&remote.identity))
            .await
        {
            Ok(_) => return Ok(true),
            Err(error) if remote_identity_changed(&error) => {
                last_change = Some(error);
            }
            Err(error) => return Err(error),
        }
    }
    Err(last_change.unwrap_or_else(|| {
        RemoteSyncError::unclassified(
            "settings-state-changed: Remote settings did not stabilize during migration.",
        )
    }))
}

#[cfg(test)]
fn reconcile_prepared_settings_after_scope(
    service: &AppSettingsService,
    app_data: &Path,
    prepared: &PreparedPortableSettingsSync,
    expected_local_hash: Option<&str>,
) -> Result<(), String> {
    reconcile_prepared_settings_after_scope_defer_publication(
        service,
        app_data,
        prepared,
        expected_local_hash,
    )?
    .publish()
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn reconcile_prepared_settings_after_scope_defer_publication(
    service: &AppSettingsService,
    _app_data: &Path,
    prepared: &PreparedPortableSettingsSync,
    expected_local_hash: Option<&str>,
) -> Result<DeferredSettingsPublication, String> {
    let mut journal = read_portable_settings_pending(&prepared.scope)?
        .ok_or_else(|| AppSettingsError::reconcile_failed().to_string())?;
    if journal.phase == PortableSettingsJournalPhase::Publication {
        return Ok(service.deferred_settings_publication(journal.publication_events));
    }
    let staged = capture_settings_file_state(prepared.scope.source_root())?;
    journal.phase = PortableSettingsJournalPhase::Reconcile;
    journal.set_staged_bytes(staged.bytes());
    journal.expected_local_hash = expected_local_hash.map(str::to_string);
    write_portable_settings_pending(&prepared.scope, &journal)?;
    if !staged.matches_hash(expected_local_hash) {
        return Err(AppSettingsError::reconcile_failed().to_string());
    }
    let desired =
        portable_settings_from_bytes(staged.bytes()).map_err(|error| error.to_string())?;
    if service
        .portable_settings_snapshot()
        .map_err(|error| error.to_string())?
        .revision()
        != prepared.expected_portable_revision
    {
        return Err(AppSettingsError::reconcile_failed().to_string());
    }
    let (applied_portable_revision, publication_events) = service
        .preview_portable_settings_merge(staged.bytes(), &prepared.expected_portable_revision)
        .map_err(|error| error.to_string())?;
    journal.applied_portable_revision = Some(applied_portable_revision);
    journal.publication_events = publication_events;
    write_portable_settings_pending(&prepared.scope, &journal)?;
    let result = service.merge_portable_settings_bytes_defer_publication_with_preflight(
        staged.bytes(),
        &prepared.expected_portable_revision,
        || {
            let actual_staged = capture_settings_file_state(prepared.scope.source_root())
                .map_err(|_| AppSettingsError::reconcile_failed())?;
            if actual_staged != staged {
                return Err(AppSettingsError::reconcile_failed());
            }
            Ok(())
        },
        |actual_store| {
            if &desired != actual_store {
                return Err(AppSettingsError::reconcile_failed());
            }
            Ok(())
        },
    );
    match result {
        Ok(publication) => {
            journal.phase = PortableSettingsJournalPhase::Publication;
            journal.publication_events = publication.publications().to_vec();
            write_portable_settings_pending(&prepared.scope, &journal)?;
            Ok(publication)
        }
        Err(error) => Err(error.to_string()),
    }
}

fn replace_publication_with_current_prepared_settings(
    service: &AppSettingsService,
    scope: &RemoteSyncScope,
) -> Result<(), String> {
    loop {
        let snapshot = service
            .portable_settings_snapshot()
            .map_err(|error| error.to_string())?;
        let mut journal = PortableSettingsJournal::prepared(snapshot.revision(), snapshot.bytes());
        journal.prepared_manifest_revision = capture_portable_settings_manifest_revision(scope)?;
        write_portable_settings_pending(scope, &journal)?;
        restore_portable_settings_stage(scope, snapshot.bytes())?;
        if service
            .portable_settings_snapshot()
            .map_err(|error| error.to_string())?
            .revision()
            == snapshot.revision()
        {
            return Ok(());
        }
    }
}

async fn finish_portable_settings_publication<Reload, ReloadFuture>(
    scope: &RemoteSyncScope,
    service: &AppSettingsService,
    publication: &DeferredSettingsPublication,
    mut reload: Reload,
) -> Result<(), String>
where
    Reload: FnMut() -> ReloadFuture,
    ReloadFuture: Future<Output = Result<(), String>>,
{
    let reload_result = reload().await;
    let journal = read_portable_settings_pending(scope)?
        .filter(|journal| journal.phase == PortableSettingsJournalPhase::Publication)
        .ok_or_else(|| AppSettingsError::reconcile_failed().to_string())?;
    let expected_revision = journal
        .applied_portable_revision
        .as_deref()
        .ok_or_else(|| AppSettingsError::reconcile_failed().to_string())?;
    if service
        .portable_settings_snapshot()
        .map_err(|error| error.to_string())?
        .revision()
        != expected_revision
    {
        replace_publication_with_current_prepared_settings(service, scope)?;
        return Err("settings-state-changed: The settings changed during publication.".to_string());
    }
    reload_result?;
    if !service
        .publish_deferred_if_portable_revision(publication, expected_revision)
        .map_err(|error| error.to_string())?
    {
        replace_publication_with_current_prepared_settings(service, scope)?;
        return Err("settings-state-changed: The settings changed during publication.".to_string());
    }
    clear_portable_settings_pending(scope)
}

async fn run_prepared_scoped_sync_pair<NotesBackend, SettingsBackend, Reload, ReloadFuture>(
    revision: &str,
    provider: SyncProvider,
    trigger: SyncTrigger,
    app_data: &Path,
    notes_scope: &RemoteSyncScope,
    notes_backend: &NotesBackend,
    settings_state: PathBuf,
    settings_backend: &SettingsBackend,
    settings_service: &AppSettingsService,
    mut reload: Reload,
) -> Result<SyncRunResult, SyncRunError>
where
    NotesBackend: RemoteSyncBackend,
    SettingsBackend: RemoteSyncBackend,
    Reload: FnMut() -> ReloadFuture,
    ReloadFuture: Future<Output = Result<(), String>>,
{
    with_remote_sync_execution_lock(|| async {
        let mut prepared = prepare_portable_settings_sync(
            settings_service,
            app_data,
            settings_state.clone(),
            "manifest.json",
        )
        .map_err(|error| {
            run_error(
                safe_error_code(&error),
                revision,
                trigger,
                SyncSummary::default(),
            )
        })?;
        if prepared.phase == PortableSettingsJournalPhase::Publication {
            let pending_publication =
                settings_service.deferred_settings_publication(prepared.publication_events.clone());
            finish_portable_settings_publication(
                prepared.scope(),
                settings_service,
                &pending_publication,
                &mut reload,
            )
            .await
            .map_err(|error| {
                run_error(
                    safe_error_code(&error),
                    revision,
                    trigger,
                    SyncSummary::default(),
                )
            })?;
            prepared = prepare_portable_settings_sync(
                settings_service,
                app_data,
                settings_state,
                "manifest.json",
            )
            .map_err(|error| {
                run_error(
                    safe_error_code(&error),
                    revision,
                    trigger,
                    SyncSummary::default(),
                )
            })?;
        }
        sanitize_legacy_remote_settings(prepared.scope(), settings_backend)
            .await
            .map_err(|error| {
                run_error(
                    safe_error_code(&error.to_string()),
                    revision,
                    trigger,
                    SyncSummary::default(),
                )
            })?;
        let mut publication = None;
        let (notes_result, settings_result) = execute_remote_sync_pair_locked(
            notes_scope,
            notes_backend,
            prepared.scope(),
            settings_backend,
            |expected| {
                publication = Some(reconcile_prepared_settings_after_scope_defer_publication(
                    settings_service,
                    app_data,
                    &prepared,
                    expected,
                )?);
                Ok(())
            },
        )
        .await;
        let result = scoped_sync_pair_result(
            revision,
            provider,
            trigger,
            notes_scope,
            notes_result,
            settings_result,
        );
        let publication_result = match publication.as_ref() {
            Some(publication) => {
                finish_portable_settings_publication(
                    prepared.scope(),
                    settings_service,
                    publication,
                    &mut reload,
                )
                .await
            }
            None => Ok(()),
        };
        match (result, publication_result) {
            (result @ Err(_), _) => result,
            (Ok(result), Ok(())) => complete_remote_first_restore_locked(notes_scope)
                .map(|()| result)
                .map_err(|error| {
                    run_error(
                        safe_error_code(&error),
                        revision,
                        trigger,
                        SyncSummary::default(),
                    )
                }),
            (Ok(result), Err(error)) => Err(run_error(
                safe_error_code(&error),
                revision,
                trigger,
                result.summary,
            )),
        }
    })
    .await
}

#[cfg(test)]
async fn run_scoped_sync_pair<NotesBackend, SettingsBackend, Reconcile>(
    revision: &str,
    provider: SyncProvider,
    trigger: SyncTrigger,
    _app_data: &Path,
    notes_scope: &RemoteSyncScope,
    notes_backend: &NotesBackend,
    settings_scope: &RemoteSyncScope,
    settings_backend: &SettingsBackend,
    reconcile: Reconcile,
) -> Result<SyncRunResult, SyncRunError>
where
    NotesBackend: RemoteSyncBackend,
    SettingsBackend: RemoteSyncBackend,
    Reconcile: FnOnce(Option<&str>) -> Result<(), String>,
{
    let (notes_result, settings_result) = execute_remote_sync_pair(
        notes_scope,
        notes_backend,
        settings_scope,
        settings_backend,
        reconcile,
    )
    .await;
    scoped_sync_pair_result(
        revision,
        provider,
        trigger,
        notes_scope,
        notes_result,
        settings_result,
    )
}

fn scoped_sync_pair_result(
    revision: &str,
    provider: SyncProvider,
    trigger: SyncTrigger,
    notes_scope: &RemoteSyncScope,
    notes_result: Result<RemoteSyncSummary, RemoteSyncError>,
    settings_result: Result<SettingsSyncOutcome, RemoteSyncError>,
) -> Result<SyncRunResult, SyncRunError> {
    let notes_summary = notes_result
        .as_ref()
        .ok()
        .map(remote_summary)
        .unwrap_or_default();
    let settings_summary = settings_result
        .as_ref()
        .ok()
        .map(|outcome| remote_summary(&outcome.summary))
        .unwrap_or_default();
    let combined = combine_summaries(&notes_summary, &settings_summary);
    if let Err(error) = notes_result {
        return Err(remote_run_error(error, revision, trigger, combined));
    }
    if let Err(error) = settings_result {
        return Err(remote_run_error(error, revision, trigger, combined));
    }
    Ok(SyncRunResult {
        notebook_name: notebook_name_from_root(notes_scope.source_root()).map_err(|error| {
            run_error(safe_error_code(&error), revision, trigger, combined.clone())
        })?,
        notes_root: notes_scope.source_root().to_string_lossy().into_owned(),
        provider,
        revision: revision.to_string(),
        summary: combined,
        trigger,
    })
}

fn settings_state_root(
    sync_state_root: &Path,
    target_fingerprint_source: &str,
    remote_root: &str,
) -> PathBuf {
    let key = sync_state_key(
        "settings",
        &[target_fingerprint_source.as_bytes(), remote_root.as_bytes()],
    );
    sync_state_root.join("settings").join(key)
}

fn validate_snapshot_target(
    config: &SyncConfig,
    target: &SyncTarget,
    remote_root: &ValidRemoteRoot,
) -> Result<(), ()> {
    match (config.provider, target) {
        (
            SyncProvider::Webdav,
            SyncTarget::Webdav {
                remote_root: target_root,
                server_url,
                username,
                password,
            },
        ) if target_root == remote_root.as_str()
            && server_url == &config.webdav.server_url
            && username == &config.webdav.username
            && password == &config.webdav.password =>
        {
            Ok(())
        }
        (
            SyncProvider::S3,
            SyncTarget::S3 {
                access_key_id,
                addressing_style,
                bucket,
                endpoint_url,
                region,
                remote_root: target_root,
                request_timeout_seconds,
                secret_access_key,
                tls_verification,
            },
        ) if target_root == remote_root.as_str()
            && access_key_id == &config.s3.access_key_id
            && bucket == &config.s3.bucket
            && endpoint_url == &config.s3.endpoint_url
            && region == &config.s3.region
            && addressing_style == &config.s3.addressing_style
            && request_timeout_seconds == &config.s3.request_timeout_seconds
            && secret_access_key == &config.s3.secret_access_key
            && tls_verification == &config.s3.tls_verification =>
        {
            Ok(())
        }
        _ => Err(()),
    }
}

fn remote_summary(summary: &RemoteSyncSummary) -> SyncSummary {
    SyncSummary {
        bytes_downloaded: summary.bytes_downloaded,
        bytes_uploaded: summary.bytes_uploaded,
        conflict_files: summary.conflict_files,
        downloaded_files: summary.downloaded_files,
        scanned_files: summary.scanned_files,
        skipped_files: summary.skipped_files,
        uploaded_files: summary.uploaded_files,
    }
}

fn combine_summaries(first: &SyncSummary, second: &SyncSummary) -> SyncSummary {
    SyncSummary {
        bytes_downloaded: first
            .bytes_downloaded
            .saturating_add(second.bytes_downloaded),
        bytes_uploaded: first.bytes_uploaded.saturating_add(second.bytes_uploaded),
        conflict_files: first.conflict_files.saturating_add(second.conflict_files),
        downloaded_files: first
            .downloaded_files
            .saturating_add(second.downloaded_files),
        scanned_files: first.scanned_files.saturating_add(second.scanned_files),
        skipped_files: first.skipped_files.saturating_add(second.skipped_files),
        uploaded_files: first.uploaded_files.saturating_add(second.uploaded_files),
    }
}

fn safe_error_code(error: &str) -> &str {
    let code = error.split(':').next().unwrap_or_default();
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

fn run_error(
    code: &str,
    revision: &str,
    trigger: SyncTrigger,
    partial_summary: SyncSummary,
) -> SyncRunError {
    SyncRunError {
        code: code.to_string(),
        diagnostic: None,
        partial_summary,
        revision: revision.to_string(),
        trigger,
    }
}

fn sync_safe_error(error: &SyncRunError, provider: SyncProvider, run_id: &str) -> SyncSafeError {
    match error.diagnostic.as_ref() {
        Some(diagnostic) => SyncSafeError {
            category: Some(diagnostic.category.as_str().to_string()),
            code: diagnostic.code.clone(),
            http_status: diagnostic.http_status,
            method: diagnostic.method.clone(),
            object_id: diagnostic.object_id.clone(),
            operation: diagnostic.operation.as_str().to_string(),
            provider,
            provider_error_code: diagnostic.provider_error_code.clone(),
            relative_path: None,
            request_id: diagnostic.request_id.clone(),
            run_id: Some(diagnostic.run_id.clone()),
        },
        None => SyncSafeError {
            category: Some("local".to_string()),
            code: error.code.clone(),
            http_status: None,
            method: None,
            object_id: None,
            operation: "synchronize".to_string(),
            provider,
            provider_error_code: None,
            relative_path: None,
            request_id: None,
            run_id: Some(run_id.to_string()),
        },
    }
}

fn remote_run_error(
    error: RemoteSyncError,
    revision: &str,
    trigger: SyncTrigger,
    partial_summary: SyncSummary,
) -> SyncRunError {
    let code = error.safe_code().to_string();
    SyncRunError {
        code,
        diagnostic: error.details().cloned(),
        partial_summary,
        revision: revision.to_string(),
        trigger,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::io;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{mpsc, Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};
    use tempfile::tempdir;

    use super::{
        build_sync_scopes, prepare_portable_settings_sync,
        prepare_portable_settings_sync_with_conflict_preserver,
        reconcile_prepared_settings_after_scope, run_prepared_scoped_sync_pair,
        run_scoped_sync_pair, scoped_sync_pair_result, settings_state_root, sync_safe_error,
        validate_snapshot_target, ApplicationSyncSource,
    };
    use crate::app_settings::{
        AppSettingsError, AppSettingsGroup, AppSettingsService, SettingsBackend, SettingsEventSink,
        SettingsPublicationEvent,
    };
    use crate::remote_sync::backend::{
        RemoteSyncBackend, RemoteSyncDiagnostic, RemoteSyncError, RemoteSyncFile,
        SyncFailureCategory, SyncProviderOperation, ValidRemoteRoot,
    };
    use crate::remote_sync::engine::{
        execute_remote_sync_pair_locked, preserve_remote_settings_conflict_with_directory_syncs,
        RemoteSyncSummary, SettingsSyncOutcome,
    };
    use crate::remote_sync::s3_backend::{S3Backend, S3SyncSettings};
    use crate::remote_sync::scope::RemoteSyncScope;
    use crate::remote_sync::settings_scope::{
        capture_settings_file_state, read_portable_settings_pending,
        replace_portable_settings_stage, write_portable_settings_pending, PortableSettingsJournal,
        PortableSettingsJournalPhase,
    };
    use crate::sync_config::model::{S3Config, SyncConfig, SyncProvider, SyncTarget, WebDavConfig};
    use crate::sync_config::status::SyncTrigger;

    struct FakeBackend {
        files: Mutex<BTreeMap<String, Vec<u8>>>,
        fail_listing: bool,
        target: &'static str,
    }

    struct SignalingBackend {
        inner: FakeBackend,
        listed: Mutex<Option<mpsc::Sender<()>>>,
    }

    struct CleanupRaceBackend {
        inner: FakeBackend,
        replacement: Vec<u8>,
        uploads: AtomicUsize,
    }

    struct FileSettingsBackend {
        fail_saves: AtomicUsize,
        gets: AtomicUsize,
        local_only_write: Mutex<Option<(usize, String, Value)>>,
        path: PathBuf,
        values: Mutex<BTreeMap<String, Value>>,
    }

    impl FileSettingsBackend {
        fn new(path: PathBuf, values: BTreeMap<String, Value>) -> Self {
            Self {
                fail_saves: AtomicUsize::new(0),
                gets: AtomicUsize::new(0),
                local_only_write: Mutex::new(None),
                path,
                values: Mutex::new(values),
            }
        }

        fn write_local_only_after_next_prepare(&self, key: &str, value: Value) {
            let write_at = self.gets.load(Ordering::SeqCst) + 11;
            *self.local_only_write.lock().unwrap() = Some((write_at, key.into(), value));
        }
    }

    impl SettingsBackend for FileSettingsBackend {
        fn get(&self, key: &str) -> Result<Option<Value>, AppSettingsError> {
            let call = self.gets.fetch_add(1, Ordering::SeqCst) + 1;
            let pending = self.local_only_write.lock().unwrap().take();
            if let Some((write_at, local_key, value)) = pending {
                if call == write_at {
                    let mut values = self.values.lock().unwrap();
                    values.insert(local_key, value);
                    fs::write(&self.path, serde_json::to_vec(&*values).unwrap())
                        .map_err(|_| AppSettingsError::reconcile_failed())?;
                } else {
                    *self.local_only_write.lock().unwrap() = Some((write_at, local_key, value));
                }
            }
            Ok(self.values.lock().unwrap().get(key).cloned())
        }

        fn set(&self, key: &str, value: Value) -> Result<(), AppSettingsError> {
            self.values.lock().unwrap().insert(key.into(), value);
            Ok(())
        }

        fn delete(&self, key: &str) -> Result<(), AppSettingsError> {
            self.values.lock().unwrap().remove(key);
            Ok(())
        }

        fn save(&self) -> Result<(), AppSettingsError> {
            if self
                .fail_saves
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
                    remaining.checked_sub(1)
                })
                .is_ok()
            {
                fs::write(&self.path, br#"{"language":"zh"#)
                    .map_err(|_| AppSettingsError::reconcile_failed())?;
                return Err(AppSettingsError::reconcile_failed());
            }
            fs::write(
                &self.path,
                serde_json::to_vec(&*self.values.lock().unwrap()).unwrap(),
            )
            .map_err(|_| AppSettingsError::reconcile_failed())
        }

        fn replace_portable_atomically(
            &self,
            desired: &serde_json::Map<String, Value>,
        ) -> Result<(), AppSettingsError> {
            let mut replacement = self.values.lock().unwrap().clone();
            for key in [
                "appearanceMode",
                "lightThemeId",
                "darkThemeId",
                "lightCustomThemeCss",
                "darkCustomThemeCss",
                "language",
                "editorPreferences",
                "fileIgnoreSettings",
                "exportSettings",
            ] {
                replacement.remove(key);
            }
            replacement.extend(
                desired
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone())),
            );
            let staged = self.path.with_extension("settings-atomic.tmp");
            if self
                .fail_saves
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
                    remaining.checked_sub(1)
                })
                .is_ok()
            {
                fs::write(&staged, br#"{"language":"zh"#)
                    .map_err(|_| AppSettingsError::reconcile_failed())?;
                let _cleanup = fs::remove_file(staged);
                return Err(AppSettingsError::reconcile_failed());
            }
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&staged)
                .map_err(|_| AppSettingsError::reconcile_failed())?;
            use std::io::Write;
            file.write_all(&serde_json::to_vec(&replacement).unwrap())
                .and_then(|()| file.sync_all())
                .map_err(|_| AppSettingsError::reconcile_failed())?;
            drop(file);
            fs::rename(&staged, &self.path).map_err(|_| AppSettingsError::reconcile_failed())?;
            *self.values.lock().unwrap() = replacement;
            Ok(())
        }
    }

    #[derive(Default)]
    struct RetryingSettingsEvents {
        emitted: Mutex<Vec<(String, Value)>>,
        fail_event: Mutex<Option<String>>,
    }

    impl RetryingSettingsEvents {
        fn fail_once(&self, event: &str) {
            *self.fail_event.lock().unwrap() = Some(event.to_string());
        }
    }

    impl SettingsEventSink for RetryingSettingsEvents {
        fn emit(&self, event: &str, payload: Value) -> Result<(), AppSettingsError> {
            if self.fail_event.lock().unwrap().as_deref() == Some(event) {
                *self.fail_event.lock().unwrap() = None;
                return Err(AppSettingsError::reconcile_failed());
            }
            self.emitted
                .lock()
                .unwrap()
                .push((event.to_string(), payload));
            Ok(())
        }
    }

    #[test]
    fn ordinary_settings_event_failure_keeps_and_retries_the_publication_outbox() {
        tauri::async_runtime::block_on(async {
            let notes = tempdir().unwrap();
            let app_data = tempdir().unwrap();
            let app_data_root = app_data.path().canonicalize().unwrap();
            let state = app_data_root.join("sync-state");
            let settings_state = state.join("settings-ordinary-event-outbox");
            let settings_path = app_data_root.join("settings.json");
            let live = BTreeMap::from([("language".into(), json!("en"))]);
            fs::write(&settings_path, serde_json::to_vec(&live).unwrap()).unwrap();
            let events = Arc::new(RetryingSettingsEvents::default());
            let settings_service = AppSettingsService::new_for_test(
                Arc::new(FileSettingsBackend::new(settings_path, live)),
                Some(events.clone()),
            );
            let notes_scope = RemoteSyncScope::notes(
                notes.path(),
                &state,
                "notes-s3-manifest.json",
                Some(notes.path().to_string_lossy().into_owned()),
                None,
            )
            .unwrap();
            let notes_backend = FakeBackend::new("notes-ordinary-event-outbox");
            let settings_backend = FakeBackend::new("settings-ordinary-event-outbox");

            run_prepared_scoped_sync_pair(
                "revision-ordinary-event-outbox",
                SyncProvider::S3,
                SyncTrigger::Manual,
                &app_data_root,
                &notes_scope,
                &notes_backend,
                settings_state.clone(),
                &settings_backend,
                &settings_service,
                || async { Ok(()) },
            )
            .await
            .unwrap();
            settings_backend
                .files
                .lock()
                .unwrap()
                .insert("settings.json".into(), br#"{"language":"zh-CN"}"#.to_vec());
            events.fail_once("markra://language-changed");

            let first = run_prepared_scoped_sync_pair(
                "revision-ordinary-event-outbox",
                SyncProvider::S3,
                SyncTrigger::Interval,
                &app_data_root,
                &notes_scope,
                &notes_backend,
                settings_state.clone(),
                &settings_backend,
                &settings_service,
                || async { Ok(()) },
            )
            .await
            .expect_err("failed ordinary settings publication must remain retryable");
            assert_eq!(first.code(), "settings-reconcile-failed");
            let scope = RemoteSyncScope::portable_settings(
                &app_data_root,
                settings_state.clone(),
                "manifest.json",
            )
            .unwrap();
            assert_eq!(
                read_portable_settings_pending(&scope)
                    .unwrap()
                    .expect("failed publication stays durable")
                    .phase,
                PortableSettingsJournalPhase::Publication
            );

            run_prepared_scoped_sync_pair(
                "revision-ordinary-event-outbox",
                SyncProvider::S3,
                SyncTrigger::Interval,
                &app_data_root,
                &notes_scope,
                &notes_backend,
                settings_state,
                &settings_backend,
                &settings_service,
                || async { Ok(()) },
            )
            .await
            .expect("the next sync retries the ordinary settings event");

            assert!(read_portable_settings_pending(&scope).unwrap().is_none());
            let emitted = events.emitted.lock().unwrap();
            assert_eq!(
                emitted
                    .iter()
                    .filter(|(event, _)| event == "markra://language-changed")
                    .count(),
                1
            );
            assert_eq!(
                emitted
                    .iter()
                    .find(|(event, _)| event == "markra://language-changed")
                    .unwrap()
                    .1["language"],
                json!("zh-CN")
            );
        });
    }

    #[test]
    fn changed_portable_settings_replace_a_stale_publication_outbox() {
        tauri::async_runtime::block_on(async {
            let notes = tempdir().unwrap();
            let app_data = tempdir().unwrap();
            let app_data_root = app_data.path().canonicalize().unwrap();
            let state = app_data_root.join("sync-state");
            let settings_state = state.join("settings-stale-publication-outbox");
            let settings_path = app_data_root.join("settings.json");
            let live = BTreeMap::from([("language".into(), json!("en"))]);
            fs::write(&settings_path, serde_json::to_vec(&live).unwrap()).unwrap();
            let events = Arc::new(RetryingSettingsEvents::default());
            let settings_service = AppSettingsService::new_for_test(
                Arc::new(FileSettingsBackend::new(settings_path.clone(), live)),
                Some(events.clone()),
            );
            let notes_scope = RemoteSyncScope::notes(
                notes.path(),
                &state,
                "notes-s3-manifest.json",
                Some(notes.path().to_string_lossy().into_owned()),
                None,
            )
            .unwrap();
            let notes_backend = FakeBackend::new("notes-stale-publication-outbox");
            let settings_backend = FakeBackend::new("settings-stale-publication-outbox");

            run_prepared_scoped_sync_pair(
                "revision-stale-publication-outbox",
                SyncProvider::S3,
                SyncTrigger::Manual,
                &app_data_root,
                &notes_scope,
                &notes_backend,
                settings_state.clone(),
                &settings_backend,
                &settings_service,
                || async { Ok(()) },
            )
            .await
            .unwrap();
            settings_backend
                .files
                .lock()
                .unwrap()
                .insert("settings.json".into(), br#"{"language":"zh-CN"}"#.to_vec());
            events.fail_once("markra://language-changed");

            run_prepared_scoped_sync_pair(
                "revision-stale-publication-outbox",
                SyncProvider::S3,
                SyncTrigger::Interval,
                &app_data_root,
                &notes_scope,
                &notes_backend,
                settings_state.clone(),
                &settings_backend,
                &settings_service,
                || async { Ok(()) },
            )
            .await
            .expect_err("failed publication remains durable until the next sync");

            settings_service
                .write_group(AppSettingsGroup::Language, json!("fr"))
                .expect("the user can change language after publication fails");

            run_prepared_scoped_sync_pair(
                "revision-stale-publication-outbox",
                SyncProvider::S3,
                SyncTrigger::Interval,
                &app_data_root,
                &notes_scope,
                &notes_backend,
                settings_state.clone(),
                &settings_backend,
                &settings_service,
                || async { Ok(()) },
            )
            .await
            .expect("the current portable settings replace the stale outbox");

            let scope =
                RemoteSyncScope::portable_settings(&app_data_root, settings_state, "manifest.json")
                    .unwrap();
            assert!(read_portable_settings_pending(&scope).unwrap().is_none());
            let language_events = events
                .emitted
                .lock()
                .unwrap()
                .iter()
                .filter(|(event, _)| event == "markra://language-changed")
                .map(|(_, payload)| payload["language"].clone())
                .collect::<Vec<_>>();
            assert_eq!(language_events, vec![json!("fr")]);
            let persisted: Value =
                serde_json::from_slice(&fs::read(settings_path).unwrap()).unwrap();
            assert_eq!(persisted["language"], json!("fr"));
            let remote: Value =
                serde_json::from_slice(&settings_backend.files.lock().unwrap()["settings.json"])
                    .unwrap();
            assert_eq!(remote["language"], json!("fr"));
        });
    }

    impl FakeBackend {
        fn new(target: &'static str) -> Self {
            Self {
                files: Mutex::new(BTreeMap::new()),
                fail_listing: false,
                target,
            }
        }

        fn failing(target: &'static str) -> Self {
            Self {
                fail_listing: true,
                ..Self::new(target)
            }
        }

        fn identity(bytes: &[u8]) -> String {
            format!("sha256:{:x}", Sha256::digest(bytes))
        }
    }

    impl RemoteSyncBackend for FakeBackend {
        fn target_fingerprint_source(&self) -> String {
            self.target.to_string()
        }

        async fn list_files(&self) -> Result<BTreeMap<String, RemoteSyncFile>, RemoteSyncError> {
            if self.fail_listing {
                return Err("settings-listing-failed: listing unavailable".into());
            }
            Ok(self
                .files
                .lock()
                .unwrap()
                .iter()
                .map(|(path, bytes)| {
                    (
                        path.clone(),
                        RemoteSyncFile {
                            identity: Self::identity(bytes),
                            size: bytes.len() as u64,
                        },
                    )
                })
                .collect())
        }

        async fn download(
            &self,
            path: &str,
            expected_identity: &str,
        ) -> Result<Vec<u8>, RemoteSyncError> {
            let bytes = self.files.lock().unwrap().get(path).cloned().unwrap();
            assert_eq!(Self::identity(&bytes), expected_identity);
            Ok(bytes)
        }

        async fn upload(
            &self,
            path: &str,
            bytes: &[u8],
            _expected_identity: Option<&str>,
        ) -> Result<String, RemoteSyncError> {
            self.files
                .lock()
                .unwrap()
                .insert(path.to_string(), bytes.to_vec());
            Ok(Self::identity(bytes))
        }

        async fn delete(
            &self,
            path: &str,
            _expected_identity: &str,
        ) -> Result<(), RemoteSyncError> {
            self.files.lock().unwrap().remove(path);
            Ok(())
        }
    }

    impl RemoteSyncBackend for CleanupRaceBackend {
        fn target_fingerprint_source(&self) -> String {
            self.inner.target_fingerprint_source()
        }

        async fn list_files(&self) -> Result<BTreeMap<String, RemoteSyncFile>, RemoteSyncError> {
            self.inner.list_files().await
        }

        async fn download(
            &self,
            path: &str,
            expected_identity: &str,
        ) -> Result<Vec<u8>, RemoteSyncError> {
            self.inner.download(path, expected_identity).await
        }

        async fn upload(
            &self,
            path: &str,
            bytes: &[u8],
            expected_identity: Option<&str>,
        ) -> Result<String, RemoteSyncError> {
            if self.uploads.fetch_add(1, Ordering::SeqCst) == 0 {
                self.inner
                    .files
                    .lock()
                    .unwrap()
                    .insert(path.to_string(), self.replacement.clone());
                return Err(RemoteSyncError::unclassified(
                    "s3-object-changed: The remote object changed.",
                ));
            }
            self.inner.upload(path, bytes, expected_identity).await
        }

        async fn delete(&self, path: &str, expected_identity: &str) -> Result<(), RemoteSyncError> {
            self.inner.delete(path, expected_identity).await
        }
    }

    #[test]
    fn remote_legacy_mcp_is_conditionally_removed_before_remote_settings_apply() {
        tauri::async_runtime::block_on(async {
            let notes = tempdir().unwrap();
            let app_data = tempdir().unwrap();
            let app_data_root = app_data.path().canonicalize().unwrap();
            let state = app_data_root.join("sync-state");
            let settings_path = app_data_root.join("settings.json");
            let live = BTreeMap::from([("language".into(), json!("en"))]);
            fs::write(&settings_path, serde_json::to_vec(&live).unwrap()).unwrap();
            let settings_service = AppSettingsService::new_for_test(
                Arc::new(FileSettingsBackend::new(settings_path, live)),
                None,
            );
            let mcp_settings =
                crate::mcp::local_settings::McpLocalSettingsService::memory_for_test();
            let current_mcp = mcp_settings.load_migrated().unwrap();
            mcp_settings
                .write(
                    &current_mcp.revision,
                    crate::mcp::config::McpConfig {
                        enabled: true,
                        ..current_mcp.config
                    },
                )
                .unwrap();
            let notes_scope = RemoteSyncScope::notes(
                notes.path(),
                &state,
                "notes-manifest.json",
                Some(notes.path().to_string_lossy().into_owned()),
                None,
            )
            .unwrap();
            let notes_backend = FakeBackend::new("notes-remote-legacy-mcp");
            let settings_backend = FakeBackend::new("settings-remote-legacy-mcp");
            let settings_state = state.join("settings-remote-legacy-mcp");

            run_prepared_scoped_sync_pair(
                "revision-remote-legacy-mcp",
                SyncProvider::S3,
                SyncTrigger::Manual,
                &app_data_root,
                &notes_scope,
                &notes_backend,
                settings_state.clone(),
                &settings_backend,
                &settings_service,
                || async { Ok(()) },
            )
            .await
            .unwrap();
            settings_service
                .write_group(AppSettingsGroup::Language, json!("de"))
                .unwrap();
            settings_backend.files.lock().unwrap().insert(
                "settings.json".into(),
                br#"{"language":"zh-CN","mcp":"ignored-without-validation"}"#.to_vec(),
            );

            run_prepared_scoped_sync_pair(
                "revision-remote-legacy-mcp",
                SyncProvider::S3,
                SyncTrigger::Interval,
                &app_data_root,
                &notes_scope,
                &notes_backend,
                settings_state,
                &settings_backend,
                &settings_service,
                || async { Ok(()) },
            )
            .await
            .unwrap();

            assert_eq!(
                settings_service
                    .read_group(AppSettingsGroup::Language)
                    .unwrap(),
                Some(json!("zh-CN"))
            );
            let remote: Value =
                serde_json::from_slice(&settings_backend.files.lock().unwrap()["settings.json"])
                    .unwrap();
            assert_eq!(remote, json!({ "language": "zh-CN" }));
            assert!(mcp_settings.load_migrated().unwrap().config.enabled);
        });
    }

    #[test]
    fn remote_mcp_cleanup_race_reloads_and_sanitizes_the_newer_remote_object() {
        tauri::async_runtime::block_on(async {
            let notes = tempdir().unwrap();
            let app_data = tempdir().unwrap();
            let app_data_root = app_data.path().canonicalize().unwrap();
            let state = app_data_root.join("sync-state");
            let settings_path = app_data_root.join("settings.json");
            let live = BTreeMap::from([("language".into(), json!("en"))]);
            fs::write(&settings_path, serde_json::to_vec(&live).unwrap()).unwrap();
            let settings_service = AppSettingsService::new_for_test(
                Arc::new(FileSettingsBackend::new(settings_path, live)),
                None,
            );
            let notes_scope = RemoteSyncScope::notes(
                notes.path(),
                &state,
                "notes-manifest.json",
                Some(notes.path().to_string_lossy().into_owned()),
                None,
            )
            .unwrap();
            let notes_backend = FakeBackend::new("notes-remote-mcp-race");
            let inner = FakeBackend::new("settings-remote-mcp-race");
            inner.files.lock().unwrap().insert(
                "settings.json".into(),
                br#"{"language":"zh-CN","mcp":{"old":true}}"#.to_vec(),
            );
            let settings_backend = CleanupRaceBackend {
                inner,
                replacement: br#"{"language":"fr","mcp":{"new":true}}"#.to_vec(),
                uploads: AtomicUsize::new(0),
            };

            run_prepared_scoped_sync_pair(
                "revision-remote-mcp-race",
                SyncProvider::S3,
                SyncTrigger::Manual,
                &app_data_root,
                &notes_scope,
                &notes_backend,
                state.join("settings-remote-mcp-race"),
                &settings_backend,
                &settings_service,
                || async { Ok(()) },
            )
            .await
            .unwrap();

            assert_eq!(settings_backend.uploads.load(Ordering::SeqCst), 2);
            assert_eq!(
                settings_service
                    .read_group(AppSettingsGroup::Language)
                    .unwrap(),
                Some(json!("fr"))
            );
            let remote: Value = serde_json::from_slice(
                &settings_backend.inner.files.lock().unwrap()["settings.json"],
            )
            .unwrap();
            assert_eq!(remote, json!({ "language": "fr" }));
        });
    }

    impl RemoteSyncBackend for SignalingBackend {
        fn target_fingerprint_source(&self) -> String {
            self.inner.target_fingerprint_source()
        }

        async fn list_files(&self) -> Result<BTreeMap<String, RemoteSyncFile>, RemoteSyncError> {
            if let Some(listed) = self.listed.lock().unwrap().take() {
                listed.send(()).unwrap();
            }
            self.inner.list_files().await
        }

        async fn download(
            &self,
            path: &str,
            expected_identity: &str,
        ) -> Result<Vec<u8>, RemoteSyncError> {
            self.inner.download(path, expected_identity).await
        }

        async fn upload(
            &self,
            path: &str,
            bytes: &[u8],
            expected_identity: Option<&str>,
        ) -> Result<String, RemoteSyncError> {
            self.inner.upload(path, bytes, expected_identity).await
        }

        async fn delete(&self, path: &str, expected_identity: &str) -> Result<(), RemoteSyncError> {
            self.inner.delete(path, expected_identity).await
        }
    }

    fn webdav_config() -> SyncConfig {
        SyncConfig {
            enabled: true,
            provider: SyncProvider::Webdav,
            remote_root: "notes".into(),
            webdav: WebDavConfig {
                server_url: "https://dav.example.test".into(),
                username: "writer".into(),
                password: "secret".into(),
            },
            ..SyncConfig::default()
        }
    }

    fn webdav_target() -> SyncTarget {
        SyncTarget::Webdav {
            remote_root: "notes".into(),
            server_url: "https://dav.example.test".into(),
            username: "writer".into(),
            password: "secret".into(),
        }
    }

    fn s3_config() -> SyncConfig {
        SyncConfig {
            enabled: true,
            provider: SyncProvider::S3,
            remote_root: "notes".into(),
            s3: S3Config {
                endpoint_url: "https://s3.example.test".into(),
                region: "ap-east-1".into(),
                bucket: "qingyu".into(),
                access_key_id: "access".into(),
                secret_access_key: "secret".into(),
                ..S3Config::default()
            },
            ..SyncConfig::default()
        }
    }

    fn s3_target() -> SyncTarget {
        SyncTarget::S3 {
            access_key_id: "access".into(),
            bucket: "qingyu".into(),
            endpoint_url: "https://s3.example.test".into(),
            region: "ap-east-1".into(),
            remote_root: "notes".into(),
            secret_access_key: "secret".into(),
            addressing_style: Default::default(),
            request_timeout_seconds: 60,
            tls_verification: Default::default(),
        }
    }

    #[test]
    fn snapshot_target_requires_the_complete_selected_provider_configuration() {
        let webdav_root = ValidRemoteRoot::parse("notes").unwrap();
        let s3_root = ValidRemoteRoot::parse("notes").unwrap();
        assert!(validate_snapshot_target(&webdav_config(), &webdav_target(), &webdav_root).is_ok());
        assert!(validate_snapshot_target(&s3_config(), &s3_target(), &s3_root).is_ok());

        let webdav_mutations: [Box<dyn FnOnce(&mut SyncTarget)>; 4] = [
            Box::new(|target| match target {
                SyncTarget::Webdav { remote_root, .. } => *remote_root = "other".into(),
                SyncTarget::S3 { .. } => unreachable!(),
            }),
            Box::new(|target| match target {
                SyncTarget::Webdav { server_url, .. } => *server_url = "https://other.test".into(),
                SyncTarget::S3 { .. } => unreachable!(),
            }),
            Box::new(|target| match target {
                SyncTarget::Webdav { username, .. } => *username = "other".into(),
                SyncTarget::S3 { .. } => unreachable!(),
            }),
            Box::new(|target| match target {
                SyncTarget::Webdav { password, .. } => *password = "other".into(),
                SyncTarget::S3 { .. } => unreachable!(),
            }),
        ];
        for mutate in webdav_mutations {
            let mut target = webdav_target();
            mutate(&mut target);
            assert!(validate_snapshot_target(&webdav_config(), &target, &webdav_root).is_err());
        }

        let s3_mutations: [Box<dyn FnOnce(&mut SyncTarget)>; 9] = [
            Box::new(|target| match target {
                SyncTarget::S3 { remote_root, .. } => *remote_root = "other".into(),
                SyncTarget::Webdav { .. } => unreachable!(),
            }),
            Box::new(|target| match target {
                SyncTarget::S3 { endpoint_url, .. } => *endpoint_url = "https://other.test".into(),
                SyncTarget::Webdav { .. } => unreachable!(),
            }),
            Box::new(|target| match target {
                SyncTarget::S3 { region, .. } => *region = "us-east-1".into(),
                SyncTarget::Webdav { .. } => unreachable!(),
            }),
            Box::new(|target| match target {
                SyncTarget::S3 { bucket, .. } => *bucket = "other".into(),
                SyncTarget::Webdav { .. } => unreachable!(),
            }),
            Box::new(|target| match target {
                SyncTarget::S3 { access_key_id, .. } => *access_key_id = "other".into(),
                SyncTarget::Webdav { .. } => unreachable!(),
            }),
            Box::new(|target| match target {
                SyncTarget::S3 {
                    secret_access_key, ..
                } => *secret_access_key = "other".into(),
                SyncTarget::Webdav { .. } => unreachable!(),
            }),
            Box::new(|target| match target {
                SyncTarget::S3 {
                    addressing_style, ..
                } => *addressing_style = crate::sync_config::model::S3AddressingStyle::Path,
                SyncTarget::Webdav { .. } => unreachable!(),
            }),
            Box::new(|target| match target {
                SyncTarget::S3 {
                    request_timeout_seconds,
                    ..
                } => *request_timeout_seconds = 120,
                SyncTarget::Webdav { .. } => unreachable!(),
            }),
            Box::new(|target| match target {
                SyncTarget::S3 {
                    tls_verification, ..
                } => *tls_verification = crate::sync_config::model::S3TlsVerification::Skip,
                SyncTarget::Webdav { .. } => unreachable!(),
            }),
        ];
        for mutate in s3_mutations {
            let mut target = s3_target();
            mutate(&mut target);
            assert!(validate_snapshot_target(&s3_config(), &target, &s3_root).is_err());
        }
    }

    #[test]
    fn snapshot_target_rejects_cross_provider_data_before_a_backend_can_be_selected() {
        let root = ValidRemoteRoot::parse("notes").unwrap();
        let result = validate_snapshot_target(&webdav_config(), &s3_target(), &root);
        assert!(result.is_err());

        let result = validate_snapshot_target(&s3_config(), &webdav_target(), &root);
        assert!(result.is_err());
    }

    #[test]
    fn provider_notes_target_preserves_the_exact_validated_child_name() {
        let remote_root = ValidRemoteRoot::parse("root").unwrap();
        let remote_path =
            crate::notebook_scope::notes_remote_prefix(&remote_root, "  个人 笔记  ").unwrap();
        let backend = S3Backend::new_at_validated_prefix(S3SyncSettings {
            access_key_id: "access".into(),
            bucket: "bucket".into(),
            endpoint_url: "https://s3.example.test".into(),
            region: "region".into(),
            remote_path,
            secret_access_key: "secret".into(),
        })
        .unwrap();

        assert!(backend
            .target_fingerprint_source()
            .ends_with("|root/notes/  个人 笔记  /"));
    }

    #[test]
    fn settings_state_is_global_to_the_target_and_uses_one_manifest_name() {
        let directory = tempdir().unwrap();
        let app_data = directory.path().canonicalize().unwrap();
        let state = app_data.join("sync-state");
        let first = settings_state_root(&state, "s3|target|root/app", "root");
        let after_notebook_switch = settings_state_root(&state, "s3|target|root/app", "root");
        let other_target = settings_state_root(&state, "s3|other|root/app", "root");

        assert_eq!(first, after_notebook_switch);
        assert_ne!(first, other_target);
        assert_eq!(first.parent(), Some(state.join("settings").as_path()));
        assert_eq!(first.file_name().unwrap().len(), 64);

        let scope = RemoteSyncScope::portable_settings(&app_data, &first, "manifest.json").unwrap();
        assert_eq!(scope.manifest_name(), "manifest.json");
        assert_eq!(scope.source_root(), first.canonicalize().unwrap());
        assert_eq!(
            scope.state_root(),
            first.join("engine").canonicalize().unwrap()
        );
    }

    #[test]
    fn legacy_mcp_pending_journal_is_rebuilt_without_touching_note_state() {
        let app_data = tempdir().unwrap();
        let app_data_root = app_data.path().canonicalize().unwrap();
        let settings_path = app_data_root.join("settings.json");
        let live = BTreeMap::from([("language".into(), json!("en"))]);
        fs::write(&settings_path, serde_json::to_vec(&live).unwrap()).unwrap();
        let settings_service = AppSettingsService::new_for_test(
            Arc::new(FileSettingsBackend::new(settings_path, live)),
            None,
        );
        let settings_state = app_data_root.join("sync-state/settings-legacy-mcp-journal");
        let note_state = app_data_root.join("sync-state/notes-preserved");
        fs::create_dir_all(&note_state).unwrap();
        fs::write(note_state.join("manifest.json"), b"note-manifest").unwrap();
        let scope = RemoteSyncScope::portable_settings(
            &app_data_root,
            settings_state.clone(),
            "manifest.json",
        )
        .unwrap();
        fs::write(
            settings_state.join("engine/manifest.json"),
            b"legacy-settings-manifest",
        )
        .unwrap();
        let legacy = PortableSettingsJournal::prepared(
            "legacy-portable-revision",
            Some(br#"{"language":"zh-CN","mcp":{"enabled":true}}"#),
        );
        write_portable_settings_pending(&scope, &legacy).unwrap();

        let prepared = prepare_portable_settings_sync(
            &settings_service,
            &app_data_root,
            settings_state.clone(),
            "manifest.json",
        )
        .unwrap();

        let rebuilt = read_portable_settings_pending(prepared.scope())
            .unwrap()
            .unwrap();
        let rebuilt_value: Value =
            serde_json::from_slice(&rebuilt.staged_bytes().unwrap().unwrap()).unwrap();
        assert_eq!(rebuilt_value["language"], json!("en"));
        assert!(rebuilt_value.get("mcp").is_none());
        assert!(!settings_state.join("engine/manifest.json").exists());
        assert_eq!(
            fs::read(note_state.join("manifest.json")).unwrap(),
            b"note-manifest"
        );
    }

    #[test]
    fn managed_mobile_bootstrap_builds_a_remote_first_scope_without_changing_regular_sync() {
        let workspace = tempdir().unwrap();
        let notes_root = workspace.path().join("B");
        fs::create_dir(&notes_root).unwrap();
        let canonical_notes = notes_root.canonicalize().unwrap();
        let app_data = tempdir().unwrap();
        let sync_state = app_data.path().canonicalize().unwrap().join("sync-state");
        let remote_root = ValidRemoteRoot::parse("root").unwrap();
        let notes_backend = FakeBackend::new("notes-b");
        let settings_backend = FakeBackend::new("settings");
        let managed_directory =
            cap_std::fs::Dir::open_ambient_dir(&canonical_notes, cap_std::ambient_authority())
                .unwrap();

        let (_, managed_bootstrap, _) = build_sync_scopes(
            &canonical_notes,
            &sync_state,
            &remote_root,
            &notes_backend,
            &settings_backend,
            None,
            ApplicationSyncSource::ManagedBootstrap(managed_directory),
        )
        .unwrap();
        let (_, regular, _) = build_sync_scopes(
            &canonical_notes,
            &sync_state,
            &remote_root,
            &notes_backend,
            &settings_backend,
            None,
            ApplicationSyncSource::Regular,
        )
        .unwrap();

        assert!(managed_bootstrap.remote_first_restore());
        assert!(!regular.remote_first_restore());
    }

    #[test]
    fn managed_mobile_bootstrap_restores_remote_content_instead_of_replaying_a_stale_deletion() {
        tauri::async_runtime::block_on(async {
            let workspace = tempdir().unwrap();
            let notes_root = workspace.path().join("B");
            fs::create_dir(&notes_root).unwrap();
            fs::write(notes_root.join("b-remote.md"), b"remote notebook B").unwrap();

            let app_data = tempdir().unwrap();
            let app_data_root = app_data.path().canonicalize().unwrap();
            let sync_state = app_data_root.join("sync-state");
            let settings_path = app_data_root.join("settings.json");
            fs::write(&settings_path, b"{}").unwrap();
            let settings_service = AppSettingsService::new_for_test(
                Arc::new(FileSettingsBackend::new(settings_path, BTreeMap::new())),
                None,
            );
            let notes_backend = FakeBackend::new("notes-b");
            let settings_backend = FakeBackend::new("settings");
            let remote_root = ValidRemoteRoot::parse("root").unwrap();
            let canonical_notes = notes_root.canonicalize().unwrap();

            let (_, original_scope, settings_state) = build_sync_scopes(
                &canonical_notes,
                &sync_state,
                &remote_root,
                &notes_backend,
                &settings_backend,
                None,
                ApplicationSyncSource::Regular,
            )
            .unwrap();
            run_prepared_scoped_sync_pair(
                "revision-mobile-restore-b",
                SyncProvider::S3,
                SyncTrigger::Manual,
                &app_data_root,
                &original_scope,
                &notes_backend,
                settings_state,
                &settings_backend,
                &settings_service,
                || async { Ok(()) },
            )
            .await
            .unwrap();

            fs::remove_dir_all(&notes_root).unwrap();
            fs::create_dir(&notes_root).unwrap();
            let canonical_notes = notes_root.canonicalize().unwrap();
            let managed_directory =
                cap_std::fs::Dir::open_ambient_dir(&canonical_notes, cap_std::ambient_authority())
                    .unwrap();
            let (_, restore_scope, settings_state) = build_sync_scopes(
                &canonical_notes,
                &sync_state,
                &remote_root,
                &notes_backend,
                &settings_backend,
                None,
                ApplicationSyncSource::ManagedBootstrap(managed_directory),
            )
            .unwrap();
            run_prepared_scoped_sync_pair(
                "revision-mobile-restore-b",
                SyncProvider::S3,
                SyncTrigger::Manual,
                &app_data_root,
                &restore_scope,
                &notes_backend,
                settings_state,
                &settings_backend,
                &settings_service,
                || async { Ok(()) },
            )
            .await
            .unwrap();

            assert_eq!(
                fs::read(notes_root.join("b-remote.md")).unwrap(),
                b"remote notebook B"
            );
            assert_eq!(
                notes_backend
                    .files
                    .lock()
                    .unwrap()
                    .get("b-remote.md")
                    .cloned(),
                Some(b"remote notebook B".to_vec())
            );
        });
    }

    #[test]
    fn remote_first_notebook_restore_discards_only_the_stale_target_notebook_baseline() {
        tauri::async_runtime::block_on(async {
            let workspace = tempdir().unwrap();
            let notes_root = workspace.path().join("B");
            fs::create_dir(&notes_root).unwrap();
            fs::write(notes_root.join("b-remote.md"), b"remote notebook B").unwrap();

            let app_data = tempdir().unwrap();
            let app_data_root = app_data.path().canonicalize().unwrap();
            let sync_state = app_data_root.join("sync-state");
            let notes_state = sync_state.join("notes-b");
            let settings_state = sync_state.join("settings");
            let settings_path = app_data_root.join("settings.json");
            fs::write(&settings_path, b"{}").unwrap();
            let settings_service = AppSettingsService::new_for_test(
                Arc::new(FileSettingsBackend::new(settings_path, BTreeMap::new())),
                None,
            );
            let notes_backend = FakeBackend::new("notes-b");
            let settings_backend = FakeBackend::new("settings");

            let original_scope = RemoteSyncScope::notes(
                &notes_root,
                &notes_state,
                "manifest.json",
                Some(notes_root.to_string_lossy().into_owned()),
                None,
            )
            .unwrap();
            run_prepared_scoped_sync_pair(
                "revision-restore-b",
                SyncProvider::S3,
                SyncTrigger::Manual,
                &app_data_root,
                &original_scope,
                &notes_backend,
                settings_state.clone(),
                &settings_backend,
                &settings_service,
                || async { Ok(()) },
            )
            .await
            .unwrap();
            assert_eq!(
                notes_backend
                    .files
                    .lock()
                    .unwrap()
                    .get("b-remote.md")
                    .cloned(),
                Some(b"remote notebook B".to_vec())
            );

            fs::remove_dir_all(&notes_root).unwrap();
            fs::create_dir(&notes_root).unwrap();
            let sibling_notebook_state = sync_state.join("notes-a");
            fs::create_dir(&sibling_notebook_state).unwrap();
            fs::write(
                sibling_notebook_state.join("manifest.json"),
                b"A notebook state must remain untouched",
            )
            .unwrap();
            let restored_scope = RemoteSyncScope::notes_from_prepared_directory(
                notes_root.canonicalize().unwrap(),
                cap_std::fs::Dir::open_ambient_dir(&notes_root, cap_std::ambient_authority())
                    .unwrap(),
                &notes_state,
                "manifest.json",
                Some(notes_root.to_string_lossy().into_owned()),
                None,
            )
            .unwrap();

            run_prepared_scoped_sync_pair(
                "revision-restore-b",
                SyncProvider::S3,
                SyncTrigger::Manual,
                &app_data_root,
                &restored_scope,
                &notes_backend,
                settings_state,
                &settings_backend,
                &settings_service,
                || async { Ok(()) },
            )
            .await
            .unwrap();

            assert_eq!(
                fs::read(notes_root.join("b-remote.md")).unwrap(),
                b"remote notebook B"
            );
            assert_eq!(
                notes_backend
                    .files
                    .lock()
                    .unwrap()
                    .get("b-remote.md")
                    .cloned(),
                Some(b"remote notebook B".to_vec())
            );
            assert_eq!(
                fs::read(sibling_notebook_state.join("manifest.json")).unwrap(),
                b"A notebook state must remain untouched"
            );
        });
    }

    #[test]
    fn mixed_live_settings_upload_only_the_portable_snapshot() {
        tauri::async_runtime::block_on(async {
            let notes = tempdir().unwrap();
            let app_data = tempdir().unwrap();
            let app_data_root = app_data.path().canonicalize().unwrap();
            let state = app_data_root.join("sync-state");
            let settings_path = app_data_root.join("settings.json");
            let live = BTreeMap::from([
                ("language".into(), json!("zh-CN")),
                (
                    "workspace".into(),
                    json!({ "path": "/Users/example/Workspace/A" }),
                ),
                ("welcomeDocumentSeen".into(), json!(true)),
            ]);
            fs::write(&settings_path, serde_json::to_vec(&live).unwrap()).unwrap();
            let settings_service = AppSettingsService::new_for_test(
                Arc::new(FileSettingsBackend::new(
                    settings_path.clone(),
                    live.clone(),
                )),
                None,
            );
            let notes_scope = RemoteSyncScope::notes(
                notes.path(),
                &state,
                "notes-s3-manifest.json",
                Some(notes.path().to_string_lossy().into_owned()),
                None,
            )
            .unwrap();
            let prepared = prepare_portable_settings_sync(
                &settings_service,
                &app_data_root,
                state.join("settings-portable"),
                "settings-s3-manifest.json",
            )
            .unwrap();
            let settings_backend = FakeBackend::new("settings");

            run_scoped_sync_pair(
                "revision-portable-stage",
                SyncProvider::S3,
                SyncTrigger::Manual,
                &app_data_root,
                &notes_scope,
                &FakeBackend::new("notes"),
                prepared.scope(),
                &settings_backend,
                |expected| {
                    reconcile_prepared_settings_after_scope(
                        &settings_service,
                        &app_data_root,
                        &prepared,
                        expected,
                    )
                },
            )
            .await
            .unwrap();

            let remote = settings_backend.files.lock().unwrap();
            assert_eq!(
                serde_json::from_slice::<Value>(&remote["settings.json"]).unwrap(),
                json!({ "language": "zh-CN" })
            );
            drop(remote);
            assert_eq!(
                serde_json::from_slice::<Value>(&fs::read(settings_path).unwrap()).unwrap(),
                serde_json::to_value(live).unwrap()
            );
        });
    }

    #[test]
    fn failed_remote_merge_preserves_live_settings_and_retries_the_download() {
        tauri::async_runtime::block_on(async {
            let notes = tempdir().unwrap();
            let app_data = tempdir().unwrap();
            let app_data_root = app_data.path().canonicalize().unwrap();
            let state = app_data_root.join("sync-state");
            let settings_state = state.join("settings-portable");
            let settings_path = app_data_root.join("settings.json");
            let live = BTreeMap::from([
                ("language".into(), json!("en")),
                (
                    "workspace".into(),
                    json!({ "path": "/Users/example/Workspace/A" }),
                ),
                ("welcomeDocumentSeen".into(), json!(true)),
            ]);
            fs::write(&settings_path, serde_json::to_vec(&live).unwrap()).unwrap();
            let store_backend = Arc::new(FileSettingsBackend::new(
                settings_path.clone(),
                live.clone(),
            ));
            let settings_service = AppSettingsService::new_for_test(store_backend.clone(), None);
            let notes_scope = RemoteSyncScope::notes(
                notes.path(),
                &state,
                "notes-s3-manifest.json",
                Some(notes.path().to_string_lossy().into_owned()),
                None,
            )
            .unwrap();
            let notes_backend = FakeBackend::new("notes-retry-portable");
            let settings_backend = FakeBackend::new("settings-retry-portable");

            run_prepared_scoped_sync_pair(
                "revision-portable-retry",
                SyncProvider::S3,
                SyncTrigger::Manual,
                &app_data_root,
                &notes_scope,
                &notes_backend,
                settings_state.clone(),
                &settings_backend,
                &settings_service,
                || async { Ok(()) },
            )
            .await
            .unwrap();
            settings_backend.files.lock().unwrap().insert(
                "settings.json".into(),
                b"{\n  \"language\": \"zh-CN\"\n}\n".to_vec(),
            );
            store_backend.fail_saves.store(1, Ordering::Relaxed);

            let error = run_prepared_scoped_sync_pair(
                "revision-portable-retry",
                SyncProvider::S3,
                SyncTrigger::Interval,
                &app_data_root,
                &notes_scope,
                &notes_backend,
                settings_state.clone(),
                &settings_backend,
                &settings_service,
                || async { Ok(()) },
            )
            .await
            .unwrap_err();
            assert_eq!(error.code(), "settings-reconcile-failed");
            assert_eq!(
                serde_json::from_slice::<Value>(&fs::read(&settings_path).unwrap()).unwrap(),
                serde_json::to_value(&live).unwrap()
            );

            run_prepared_scoped_sync_pair(
                "revision-portable-retry",
                SyncProvider::S3,
                SyncTrigger::Interval,
                &app_data_root,
                &notes_scope,
                &notes_backend,
                settings_state,
                &settings_backend,
                &settings_service,
                || async { Ok(()) },
            )
            .await
            .unwrap();

            let settled: Value =
                serde_json::from_slice(&fs::read(&settings_path).unwrap()).unwrap();
            assert_eq!(settled["language"], json!("zh-CN"));
            assert_eq!(settled["workspace"], live["workspace"]);
            assert_eq!(settled["welcomeDocumentSeen"], json!(true));
            let remote: Value =
                serde_json::from_slice(&settings_backend.files.lock().unwrap()["settings.json"])
                    .unwrap();
            assert_eq!(remote, json!({ "language": "zh-CN" }));
        });
    }

    #[test]
    fn remote_download_and_deletion_change_only_portable_live_keys() {
        tauri::async_runtime::block_on(async {
            let notes = tempdir().unwrap();
            let app_data = tempdir().unwrap();
            let app_data_root = app_data.path().canonicalize().unwrap();
            let state = app_data_root.join("sync-state");
            let settings_state = state.join("settings-portable");
            let settings_path = app_data_root.join("settings.json");
            let live = BTreeMap::from([
                ("language".into(), json!("en")),
                (
                    "workspace".into(),
                    json!({ "path": "/Users/example/Workspace/A" }),
                ),
                ("welcomeDocumentSeen".into(), json!(true)),
            ]);
            fs::write(&settings_path, serde_json::to_vec(&live).unwrap()).unwrap();
            let settings_service = AppSettingsService::new_for_test(
                Arc::new(FileSettingsBackend::new(
                    settings_path.clone(),
                    live.clone(),
                )),
                None,
            );
            let notes_scope = RemoteSyncScope::notes(
                notes.path(),
                &state,
                "notes-s3-manifest.json",
                Some(notes.path().to_string_lossy().into_owned()),
                None,
            )
            .unwrap();
            let notes_backend = FakeBackend::new("notes-download-portable");
            let settings_backend = FakeBackend::new("settings-download-portable");
            settings_backend
                .files
                .lock()
                .unwrap()
                .insert("settings.json".into(), br#"{"language":"zh-CN"}"#.to_vec());

            run_prepared_scoped_sync_pair(
                "revision-portable-download-delete",
                SyncProvider::Webdav,
                SyncTrigger::Manual,
                &app_data_root,
                &notes_scope,
                &notes_backend,
                settings_state.clone(),
                &settings_backend,
                &settings_service,
                || async { Ok(()) },
            )
            .await
            .unwrap();
            let downloaded: Value =
                serde_json::from_slice(&fs::read(&settings_path).unwrap()).unwrap();
            assert_eq!(downloaded["language"], json!("zh-CN"));
            assert_eq!(downloaded["workspace"], live["workspace"]);
            assert_eq!(downloaded["welcomeDocumentSeen"], json!(true));

            settings_backend.files.lock().unwrap().clear();
            run_prepared_scoped_sync_pair(
                "revision-portable-download-delete",
                SyncProvider::Webdav,
                SyncTrigger::Interval,
                &app_data_root,
                &notes_scope,
                &notes_backend,
                settings_state,
                &settings_backend,
                &settings_service,
                || async { Ok(()) },
            )
            .await
            .unwrap();

            let deleted: Value =
                serde_json::from_slice(&fs::read(&settings_path).unwrap()).unwrap();
            assert!(deleted.get("language").is_none());
            assert_eq!(deleted["workspace"], live["workspace"]);
            assert_eq!(deleted["welcomeDocumentSeen"], json!(true));
            assert!(settings_backend.files.lock().unwrap().is_empty());
        });
    }

    #[test]
    fn pair_result_combines_both_scopes_with_one_revision_and_trigger() {
        tauri::async_runtime::block_on(async {
            let notes = tempdir().unwrap();
            let app_data = tempdir().unwrap();
            let app_data_root = app_data.path().canonicalize().unwrap();
            let state = app_data_root.join("sync-state");
            fs::write(notes.path().join("note.md"), b"note").unwrap();
            fs::write(app_data_root.join("settings.json"), b"{}").unwrap();
            let notes_scope = RemoteSyncScope::notes(
                notes.path(),
                &state,
                "notes-s3-manifest.json",
                Some(notes.path().to_string_lossy().into_owned()),
                None,
            )
            .unwrap();
            let settings_scope = RemoteSyncScope::portable_settings(
                &app_data_root,
                &state,
                "settings-s3-manifest.json",
            )
            .unwrap();
            replace_portable_settings_stage(&settings_scope, Some(br#"{"language":"en"}"#))
                .unwrap();

            let result = run_scoped_sync_pair(
                "revision-7",
                SyncProvider::S3,
                SyncTrigger::Manual,
                &app_data_root,
                &notes_scope,
                &FakeBackend::new("notes"),
                &settings_scope,
                &FakeBackend::new("settings"),
                |_| Ok(()),
            )
            .await
            .unwrap();

            assert_eq!(result.revision, "revision-7");
            assert_eq!(result.trigger, SyncTrigger::Manual);
            assert_eq!(result.summary.uploaded_files, 2);
            assert_eq!(result.summary.scanned_files, 2);
        });
    }

    #[test]
    fn settings_failure_reports_partial_notes_success_without_claiming_success() {
        tauri::async_runtime::block_on(async {
            let notes = tempdir().unwrap();
            let app_data = tempdir().unwrap();
            let app_data_root = app_data.path().canonicalize().unwrap();
            let state = app_data_root.join("sync-state");
            fs::write(notes.path().join("note.md"), b"note").unwrap();
            fs::write(app_data_root.join("settings.json"), b"{}").unwrap();
            let notes_scope = RemoteSyncScope::notes(
                notes.path(),
                &state,
                "notes-webdav-manifest.json",
                Some(notes.path().to_string_lossy().into_owned()),
                None,
            )
            .unwrap();
            let settings_scope = RemoteSyncScope::portable_settings(
                &app_data_root,
                &state,
                "settings-webdav-manifest.json",
            )
            .unwrap();

            let error = run_scoped_sync_pair(
                "revision-8",
                SyncProvider::Webdav,
                SyncTrigger::AppLaunch,
                &app_data_root,
                &notes_scope,
                &FakeBackend::new("notes"),
                &settings_scope,
                &FakeBackend::failing("settings"),
                |_| Ok(()),
            )
            .await
            .unwrap_err();

            assert_eq!(error.code(), "settings-listing-failed");
            assert_eq!(error.revision(), "revision-8");
            assert_eq!(error.partial_summary().uploaded_files, 1);
        });
    }

    #[test]
    fn typed_provider_failure_survives_the_pair_result_boundary() {
        let notes = tempdir().unwrap();
        let state = tempdir().unwrap();
        let state_root = state.path().canonicalize().unwrap().join("sync-state");
        let notes_scope = RemoteSyncScope::notes(
            notes.path(),
            &state_root,
            "notes-s3-manifest.json",
            Some(notes.path().to_string_lossy().into_owned()),
            None,
        )
        .unwrap();
        let diagnostic = RemoteSyncDiagnostic {
            category: SyncFailureCategory::Http,
            code: "s3-upload-http-failed".into(),
            http_status: Some(403),
            method: Some("PUT".into()),
            object_id: Some("object-a1".into()),
            operation: SyncProviderOperation::Upload,
            provider_error_code: Some("AccessDenied".into()),
            request_id: Some("request-403".into()),
            run_id: "run-1".into(),
            scope: "notes".into(),
        };

        let error = scoped_sync_pair_result(
            "revision-typed",
            SyncProvider::S3,
            SyncTrigger::Manual,
            &notes_scope,
            Err(RemoteSyncError::diagnostic(diagnostic.clone())),
            Ok(SettingsSyncOutcome {
                expected_local_hash: None,
                summary: RemoteSyncSummary::default(),
            }),
        )
        .unwrap_err();

        assert_eq!(error.code(), "s3-upload-http-failed");
        assert_eq!(error.diagnostic.as_ref(), Some(&diagnostic));
        assert!(!error.to_string().contains("note"));
        let safe = sync_safe_error(&error, SyncProvider::S3, "fallback-run");
        assert_eq!(safe.category.as_deref(), Some("http"));
        assert_eq!(safe.operation, "upload");
        assert_eq!(safe.method.as_deref(), Some("PUT"));
        assert_eq!(safe.http_status, Some(403));
        assert_eq!(safe.provider_error_code.as_deref(), Some("AccessDenied"));
        assert_eq!(safe.request_id.as_deref(), Some("request-403"));
        assert_eq!(safe.run_id.as_deref(), Some("run-1"));
        assert!(safe.relative_path.is_none());
    }

    #[test]
    fn failed_settings_reconcile_is_retried_after_the_engine_has_checkpointed() {
        tauri::async_runtime::block_on(async {
            let notes = tempdir().unwrap();
            let app_data = tempdir().unwrap();
            let app_data_root = app_data.path().canonicalize().unwrap();
            let state = app_data_root.join("sync-state");
            fs::write(app_data_root.join("settings.json"), b"{}").unwrap();
            let notes_scope = RemoteSyncScope::notes(
                notes.path(),
                &state,
                "notes-s3-manifest.json",
                Some(notes.path().to_string_lossy().into_owned()),
                None,
            )
            .unwrap();
            let settings_scope = RemoteSyncScope::portable_settings(
                &app_data_root,
                &state,
                "settings-s3-manifest.json",
            )
            .unwrap();
            let notes_backend = FakeBackend::new("notes");
            let settings_backend = FakeBackend::new("settings");
            run_scoped_sync_pair(
                "revision-retry",
                SyncProvider::S3,
                SyncTrigger::Manual,
                &app_data_root,
                &notes_scope,
                &notes_backend,
                &settings_scope,
                &settings_backend,
                |_| Ok(()),
            )
            .await
            .unwrap();
            settings_backend
                .files
                .lock()
                .unwrap()
                .insert("settings.json".into(), br#"{"language":"zh-CN"}"#.to_vec());
            let attempts = AtomicUsize::new(0);

            let first_error = run_scoped_sync_pair(
                "revision-retry",
                SyncProvider::S3,
                SyncTrigger::Interval,
                &app_data_root,
                &notes_scope,
                &notes_backend,
                &settings_scope,
                &settings_backend,
                |_| {
                    attempts.fetch_add(1, Ordering::SeqCst);
                    Err("settings-reconcile-failed: injected once".into())
                },
            )
            .await
            .unwrap_err();
            assert_eq!(first_error.code(), "settings-reconcile-failed");

            let result = run_scoped_sync_pair(
                "revision-retry",
                SyncProvider::S3,
                SyncTrigger::Interval,
                &app_data_root,
                &notes_scope,
                &notes_backend,
                &settings_scope,
                &settings_backend,
                |_| {
                    attempts.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                },
            )
            .await
            .unwrap();

            assert_eq!(result.summary.downloaded_files, 0);
            assert_eq!(attempts.load(Ordering::SeqCst), 2);
        });
    }

    async fn recover_engine_checkpointed_settings_mutation(
        remote_after_baseline: Option<&[u8]>,
    ) -> Option<Vec<u8>> {
        let notes = tempdir().unwrap();
        let app_data = tempdir().unwrap();
        let app_data_root = app_data.path().canonicalize().unwrap();
        let state = app_data_root.join("sync-state");
        let settings_state = state.join("settings-prepared-crash");
        let settings_path = app_data_root.join("settings.json");
        let live = BTreeMap::from([("language".into(), json!("en"))]);
        fs::write(&settings_path, serde_json::to_vec(&live).unwrap()).unwrap();
        let settings_service = AppSettingsService::new_for_test(
            Arc::new(FileSettingsBackend::new(settings_path, live)),
            None,
        );
        let notes_scope = RemoteSyncScope::notes(
            notes.path(),
            &state,
            "notes-s3-manifest.json",
            Some(notes.path().to_string_lossy().into_owned()),
            None,
        )
        .unwrap();
        let notes_backend = FakeBackend::new("notes-prepared-crash");
        let settings_backend = FakeBackend::new("settings-prepared-crash");
        run_prepared_scoped_sync_pair(
            "revision-prepared-crash",
            SyncProvider::S3,
            SyncTrigger::Manual,
            &app_data_root,
            &notes_scope,
            &notes_backend,
            settings_state.clone(),
            &settings_backend,
            &settings_service,
            || async { Ok(()) },
        )
        .await
        .unwrap();
        match remote_after_baseline {
            Some(bytes) => {
                settings_backend
                    .files
                    .lock()
                    .unwrap()
                    .insert("settings.json".into(), bytes.to_vec());
            }
            None => settings_backend.files.lock().unwrap().clear(),
        }
        let prepared = prepare_portable_settings_sync(
            &settings_service,
            &app_data_root,
            settings_state.clone(),
            "manifest.json",
        )
        .unwrap();
        let (_, settings_result) = execute_remote_sync_pair_locked(
            &notes_scope,
            &notes_backend,
            prepared.scope(),
            &settings_backend,
            |_| Err("simulated-crash-before-reconcile-journal".into()),
        )
        .await;
        assert!(settings_result.is_err());
        let checkpointed = capture_settings_file_state(prepared.scope().source_root())
            .unwrap()
            .bytes()
            .map(ToOwned::to_owned);

        let recovered = prepare_portable_settings_sync(
            &settings_service,
            &app_data_root,
            settings_state,
            "manifest.json",
        )
        .expect("the durable engine checkpoint must be recovered");
        assert_eq!(recovered.phase, PortableSettingsJournalPhase::Reconcile);
        assert_eq!(
            capture_settings_file_state(recovered.scope().source_root())
                .unwrap()
                .bytes(),
            checkpointed.as_deref()
        );
        checkpointed
    }

    #[test]
    fn prepared_journal_recovery_preserves_an_engine_checkpointed_download() {
        let recovered = tauri::async_runtime::block_on(
            recover_engine_checkpointed_settings_mutation(Some(br#"{"language":"zh-CN"}"#)),
        );
        assert_eq!(
            recovered.as_deref(),
            Some(br#"{"language":"zh-CN"}"#.as_slice())
        );
    }

    #[test]
    fn prepared_journal_recovery_preserves_an_engine_checkpointed_remote_deletion() {
        let recovered =
            tauri::async_runtime::block_on(recover_engine_checkpointed_settings_mutation(None));
        assert!(recovered.is_none());
    }

    #[test]
    fn local_only_writer_between_publication_and_reconcile_preserves_remote_portable_settings() {
        tauri::async_runtime::block_on(async {
            let notes = tempdir().unwrap();
            let app_data = tempdir().unwrap();
            let app_data_root = app_data.path().canonicalize().unwrap();
            let state = app_data_root.join("sync-state");
            let settings_state = state.join("settings-race");
            let settings_path = app_data_root.join("settings.json");
            let live = BTreeMap::from([(
                "workspace".into(),
                json!({ "path": "/Users/example/Workspace/A" }),
            )]);
            fs::write(&settings_path, serde_json::to_vec(&live).unwrap()).unwrap();
            let notes_scope = RemoteSyncScope::notes(
                notes.path(),
                &state,
                "notes-s3-manifest.json",
                Some(notes.path().to_string_lossy().into_owned()),
                None,
            )
            .unwrap();
            let notes_backend = FakeBackend::new("notes");
            let settings_backend = FakeBackend::new("settings");
            let store_backend = Arc::new(FileSettingsBackend::new(settings_path.clone(), live));
            let settings_service = AppSettingsService::new_for_test(store_backend.clone(), None);

            run_prepared_scoped_sync_pair(
                "revision-race",
                SyncProvider::S3,
                SyncTrigger::Manual,
                &app_data_root,
                &notes_scope,
                &notes_backend,
                settings_state.clone(),
                &settings_backend,
                &settings_service,
                || async { Ok(()) },
            )
            .await
            .unwrap();
            settings_backend
                .files
                .lock()
                .unwrap()
                .insert("settings.json".into(), br#"{"language":"zh-CN"}"#.to_vec());
            store_backend.write_local_only_after_next_prepare(
                "workspace",
                json!({ "path": "/Users/example/Workspace/B" }),
            );

            run_prepared_scoped_sync_pair(
                "revision-race",
                SyncProvider::S3,
                SyncTrigger::Interval,
                &app_data_root,
                &notes_scope,
                &notes_backend,
                settings_state.clone(),
                &settings_backend,
                &settings_service,
                || async { Ok(()) },
            )
            .await
            .unwrap();

            run_prepared_scoped_sync_pair(
                "revision-race",
                SyncProvider::S3,
                SyncTrigger::Interval,
                &app_data_root,
                &notes_scope,
                &notes_backend,
                settings_state,
                &settings_backend,
                &settings_service,
                || async { Ok(()) },
            )
            .await
            .unwrap();

            let settled: Value =
                serde_json::from_slice(&fs::read(&settings_path).unwrap()).unwrap();
            assert_eq!(settled["language"], json!("zh-CN"));
            assert_eq!(
                settled["workspace"],
                json!({ "path": "/Users/example/Workspace/B" })
            );
            let remote: Value =
                serde_json::from_slice(&settings_backend.files.lock().unwrap()["settings.json"])
                    .unwrap();
            assert_eq!(remote, json!({ "language": "zh-CN" }));
        });
    }

    #[test]
    fn concurrent_portable_writer_is_preserved_and_the_next_sync_recovers() {
        tauri::async_runtime::block_on(async {
            let notes = tempdir().unwrap();
            let app_data = tempdir().unwrap();
            let app_data_root = app_data.path().canonicalize().unwrap();
            let state = app_data_root.join("sync-state");
            let settings_state = state.join("settings-portable-conflict");
            let settings_path = app_data_root.join("settings.json");
            let live = BTreeMap::from([
                ("language".into(), json!("en")),
                (
                    "workspace".into(),
                    json!({ "path": "/Users/example/Workspace/A" }),
                ),
            ]);
            fs::write(&settings_path, serde_json::to_vec(&live).unwrap()).unwrap();
            let store_backend = Arc::new(FileSettingsBackend::new(settings_path.clone(), live));
            let settings_service = AppSettingsService::new_for_test(store_backend.clone(), None);
            let notes_scope = RemoteSyncScope::notes(
                notes.path(),
                &state,
                "notes-s3-manifest.json",
                Some(notes.path().to_string_lossy().into_owned()),
                None,
            )
            .unwrap();
            let notes_backend = FakeBackend::new("notes-portable-conflict");
            let settings_backend = FakeBackend::new("settings-portable-conflict");

            run_prepared_scoped_sync_pair(
                "revision-portable-conflict",
                SyncProvider::S3,
                SyncTrigger::Manual,
                &app_data_root,
                &notes_scope,
                &notes_backend,
                settings_state.clone(),
                &settings_backend,
                &settings_service,
                || async { Ok(()) },
            )
            .await
            .unwrap();
            settings_backend
                .files
                .lock()
                .unwrap()
                .insert("settings.json".into(), br#"{"language":"zh-CN"}"#.to_vec());
            store_backend.write_local_only_after_next_prepare("language", json!("fr"));

            let first_error = run_prepared_scoped_sync_pair(
                "revision-portable-conflict",
                SyncProvider::S3,
                SyncTrigger::Interval,
                &app_data_root,
                &notes_scope,
                &notes_backend,
                settings_state.clone(),
                &settings_backend,
                &settings_service,
                || async { Ok(()) },
            )
            .await
            .unwrap_err();
            assert_eq!(first_error.code(), "settings-reconcile-failed");
            let scope = RemoteSyncScope::portable_settings(
                &app_data_root,
                settings_state.clone(),
                "manifest.json",
            )
            .unwrap();
            let pending_before_durability_failure = read_portable_settings_pending(&scope)
                .unwrap()
                .expect("the reconcile journal must remain pending");
            let durability_error = match prepare_portable_settings_sync_with_conflict_preserver(
                &settings_service,
                &app_data_root,
                settings_state.clone(),
                "manifest.json",
                |scope, bytes| {
                    preserve_remote_settings_conflict_with_directory_syncs(
                        scope,
                        bytes,
                        |_| Err(io::Error::other("injected conflict directory sync failure")),
                        |_| panic!("state root must not sync after child directory sync failure"),
                    )
                },
            ) {
                Ok(_) => {
                    panic!("an undurable conflict copy must not replace the reconcile journal")
                }
                Err(error) => error,
            };
            assert!(durability_error.contains("conflict publication"));
            assert_eq!(
                read_portable_settings_pending(&scope).unwrap(),
                Some(pending_before_durability_failure)
            );
            assert_eq!(fs::read_dir(scope.conflict_root()).unwrap().count(), 0);
            let restarted_live = serde_json::from_slice::<BTreeMap<String, Value>>(
                &fs::read(&settings_path).unwrap(),
            )
            .unwrap();
            let restarted_service = AppSettingsService::new_for_test(
                Arc::new(FileSettingsBackend::new(
                    settings_path.clone(),
                    restarted_live,
                )),
                None,
            );

            run_prepared_scoped_sync_pair(
                "revision-portable-conflict",
                SyncProvider::S3,
                SyncTrigger::Interval,
                &app_data_root,
                &notes_scope,
                &notes_backend,
                settings_state.clone(),
                &settings_backend,
                &restarted_service,
                || async { Ok(()) },
            )
            .await
            .unwrap();
            assert!(read_portable_settings_pending(&scope).unwrap().is_none());
            let persisted: Value =
                serde_json::from_slice(&fs::read(settings_path).unwrap()).unwrap();
            assert_eq!(persisted["language"], json!("fr"));
            let remote: Value =
                serde_json::from_slice(&settings_backend.files.lock().unwrap()["settings.json"])
                    .unwrap();
            assert_eq!(remote, json!({ "language": "fr" }));
            assert!(fs::read_dir(scope.state_root().join("conflicts"))
                .unwrap()
                .flatten()
                .any(|entry| fs::read(entry.path()).ok().as_deref()
                    == Some(br#"{"language":"zh-CN"}"#)));
        });
    }

    #[test]
    fn publication_retry_finishes_the_outbox_before_syncing_a_new_remote_revision() {
        tauri::async_runtime::block_on(async {
            let notes = tempdir().unwrap();
            let app_data = tempdir().unwrap();
            let app_data_root = app_data.path().canonicalize().unwrap();
            let state = app_data_root.join("sync-state");
            let settings_state = state.join("settings-publication-retry");
            let settings_path = app_data_root.join("settings.json");
            let live = BTreeMap::from([("language".into(), json!("en"))]);
            fs::write(&settings_path, serde_json::to_vec(&live).unwrap()).unwrap();
            let settings_service = AppSettingsService::new_for_test(
                Arc::new(FileSettingsBackend::new(settings_path.clone(), live)),
                None,
            );
            let notes_scope = RemoteSyncScope::notes(
                notes.path(),
                &state,
                "notes-s3-manifest.json",
                Some(notes.path().to_string_lossy().into_owned()),
                None,
            )
            .unwrap();
            let notes_backend = FakeBackend::new("notes-publication-retry");
            let settings_backend = FakeBackend::new("settings-publication-retry");

            run_prepared_scoped_sync_pair(
                "revision-publication-retry",
                SyncProvider::S3,
                SyncTrigger::Manual,
                &app_data_root,
                &notes_scope,
                &notes_backend,
                settings_state.clone(),
                &settings_backend,
                &settings_service,
                || async { Ok(()) },
            )
            .await
            .unwrap();
            settings_backend
                .files
                .lock()
                .unwrap()
                .insert("settings.json".into(), br#"{"language":"zh-CN"}"#.to_vec());
            let failure = run_prepared_scoped_sync_pair(
                "revision-publication-retry",
                SyncProvider::S3,
                SyncTrigger::Interval,
                &app_data_root,
                &notes_scope,
                &notes_backend,
                settings_state.clone(),
                &settings_backend,
                &settings_service,
                || async { Err("settings-publication-hook-failed: injected once".to_string()) },
            )
            .await
            .unwrap_err();
            assert_eq!(failure.code(), "settings-publication-hook-failed");
            settings_backend
                .files
                .lock()
                .unwrap()
                .insert("settings.json".into(), br#"{"language":"fr"}"#.to_vec());

            let reloads = AtomicUsize::new(0);
            run_prepared_scoped_sync_pair(
                "revision-publication-retry",
                SyncProvider::S3,
                SyncTrigger::Interval,
                &app_data_root,
                &notes_scope,
                &notes_backend,
                settings_state.clone(),
                &settings_backend,
                &settings_service,
                || {
                    reloads.fetch_add(1, Ordering::SeqCst);
                    async { Ok(()) }
                },
            )
            .await
            .unwrap();

            assert_eq!(reloads.load(Ordering::SeqCst), 2);
            let persisted: Value =
                serde_json::from_slice(&fs::read(settings_path).unwrap()).unwrap();
            assert_eq!(persisted["language"], json!("fr"));
            let scope =
                RemoteSyncScope::portable_settings(&app_data_root, settings_state, "manifest.json")
                    .unwrap();
            assert!(read_portable_settings_pending(&scope).unwrap().is_none());
        });
    }

    #[test]
    fn reconcile_journal_recovers_a_settings_commit_before_publication_was_recorded() {
        let app_data = tempdir().unwrap();
        let app_data_root = app_data.path().canonicalize().unwrap();
        let settings_state = app_data_root.join("sync-state/settings-commit-recovery");
        let settings_path = app_data_root.join("settings.json");
        let live = BTreeMap::from([("language".into(), json!("en"))]);
        fs::write(&settings_path, serde_json::to_vec(&live).unwrap()).unwrap();
        let store_backend = Arc::new(FileSettingsBackend::new(settings_path, live));
        let settings_service = AppSettingsService::new_for_test(store_backend.clone(), None);
        let prepared = prepare_portable_settings_sync(
            &settings_service,
            &app_data_root,
            settings_state.clone(),
            "manifest.json",
        )
        .unwrap();
        let downloaded = br#"{"language":"zh-CN"}"#;
        replace_portable_settings_stage(prepared.scope(), Some(downloaded)).unwrap();
        let staged = capture_settings_file_state(prepared.scope().source_root()).unwrap();
        let mut journal = read_portable_settings_pending(prepared.scope())
            .unwrap()
            .unwrap();
        journal.phase = PortableSettingsJournalPhase::Reconcile;
        journal.set_staged_bytes(staged.bytes());
        journal.expected_local_hash = staged.hash().map(str::to_string);
        journal.publication_events = vec![SettingsPublicationEvent::new(
            "markra://language-changed",
            json!({ "language": "zh-CN" }),
        )];
        let desired = serde_json::from_slice::<Value>(downloaded).unwrap();
        store_backend
            .replace_portable_atomically(desired.as_object().unwrap())
            .unwrap();
        journal.applied_portable_revision = Some(
            settings_service
                .portable_settings_snapshot()
                .unwrap()
                .revision()
                .to_string(),
        );
        write_portable_settings_pending(prepared.scope(), &journal).unwrap();

        let recovered = prepare_portable_settings_sync(
            &settings_service,
            &app_data_root,
            settings_state,
            "manifest.json",
        )
        .expect("an applied merge must resume at durable publication");

        assert_eq!(recovered.phase, PortableSettingsJournalPhase::Publication);
        assert_eq!(recovered.publication_events, journal.publication_events);
        assert_eq!(
            read_portable_settings_pending(recovered.scope())
                .unwrap()
                .unwrap()
                .phase,
            PortableSettingsJournalPhase::Publication
        );
    }

    #[test]
    fn application_sync_lock_covers_settings_reconciliation() {
        let (reconcile_entered_tx, reconcile_entered_rx) = mpsc::channel();
        let (release_reconcile_tx, release_reconcile_rx) = mpsc::channel();
        let first = thread::spawn(move || {
            tauri::async_runtime::block_on(async move {
                let notes = tempdir().unwrap();
                let app_data = tempdir().unwrap();
                let app_data_root = app_data.path().canonicalize().unwrap();
                let state = app_data_root.join("sync-state");
                fs::write(app_data_root.join("settings.json"), b"{}").unwrap();
                let notes_scope = RemoteSyncScope::notes(
                    notes.path(),
                    &state,
                    "notes-s3-manifest.json",
                    Some(notes.path().to_string_lossy().into_owned()),
                    None,
                )
                .unwrap();
                let settings_scope = RemoteSyncScope::portable_settings(
                    &app_data_root,
                    &state,
                    "settings-s3-manifest.json",
                )
                .unwrap();

                run_scoped_sync_pair(
                    "revision-lock-a",
                    SyncProvider::S3,
                    SyncTrigger::Manual,
                    &app_data_root,
                    &notes_scope,
                    &FakeBackend::new("notes-lock-a"),
                    &settings_scope,
                    &FakeBackend::new("settings-lock-a"),
                    |_| {
                        reconcile_entered_tx.send(()).unwrap();
                        release_reconcile_rx
                            .recv_timeout(Duration::from_secs(5))
                            .unwrap();
                        Ok(())
                    },
                )
                .await
                .unwrap();
            });
        });
        reconcile_entered_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap();

        let (second_listed_tx, second_listed_rx) = mpsc::channel();
        let (second_state_tx, second_state_rx) = mpsc::channel();
        let second = thread::spawn(move || {
            tauri::async_runtime::block_on(async move {
                let notes = tempdir().unwrap();
                let app_data = tempdir().unwrap();
                let app_data_root = app_data.path().canonicalize().unwrap();
                let state = app_data_root.join("sync-state");
                fs::write(app_data_root.join("settings.json"), b"{}").unwrap();
                let notes_scope = RemoteSyncScope::notes(
                    notes.path(),
                    &state,
                    "notes-s3-manifest.json",
                    Some(notes.path().to_string_lossy().into_owned()),
                    None,
                )
                .unwrap();
                let settings_state = state.join("settings-prepare-lock");
                second_state_tx.send(settings_state.clone()).unwrap();
                let notes_backend = SignalingBackend {
                    inner: FakeBackend::new("notes-lock-b"),
                    listed: Mutex::new(Some(second_listed_tx)),
                };

                let store_backend = Arc::new(FileSettingsBackend::new(
                    app_data_root.join("settings.json"),
                    BTreeMap::new(),
                ));
                let settings_service = AppSettingsService::new_for_test(store_backend, None);
                run_prepared_scoped_sync_pair(
                    "revision-lock-b",
                    SyncProvider::S3,
                    SyncTrigger::Manual,
                    &app_data_root,
                    &notes_scope,
                    &notes_backend,
                    settings_state,
                    &FakeBackend::new("settings-lock-b"),
                    &settings_service,
                    || async { Ok(()) },
                )
                .await
                .unwrap();
            });
        });

        let second_state = second_state_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap();

        assert!(matches!(
            second_listed_rx.recv_timeout(Duration::from_millis(150)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        assert!(!second_state.join("portable-settings-pending.json").exists());
        release_reconcile_tx.send(()).unwrap();
        second_listed_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        first.join().unwrap();
        second.join().unwrap();
    }

    #[test]
    fn notes_scope_uses_application_file_ignore_rules_for_ordinary_files() {
        tauri::async_runtime::block_on(async {
            let notes = tempdir().unwrap();
            let app_data = tempdir().unwrap();
            let app_data_root = app_data.path().canonicalize().unwrap();
            let state = app_data_root.join("sync-state");
            fs::write(notes.path().join("keep.md"), b"keep").unwrap();
            fs::write(notes.path().join("draft.tmp"), b"ignored").unwrap();
            fs::write(app_data_root.join("settings.json"), b"{}").unwrap();
            let notes_scope = RemoteSyncScope::notes(
                notes.path(),
                &state,
                "notes-s3-manifest.json",
                Some(notes.path().to_string_lossy().into_owned()),
                Some("*.tmp".into()),
            )
            .unwrap();
            let settings_scope = RemoteSyncScope::portable_settings(
                &app_data_root,
                &state,
                "settings-s3-manifest.json",
            )
            .unwrap();
            let notes_backend = FakeBackend::new("notes");

            run_scoped_sync_pair(
                "revision-ignore",
                SyncProvider::S3,
                SyncTrigger::Manual,
                &app_data_root,
                &notes_scope,
                &notes_backend,
                &settings_scope,
                &FakeBackend::new("settings"),
                |_| Ok(()),
            )
            .await
            .unwrap();

            let remote_paths = notes_backend
                .files
                .lock()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            assert_eq!(remote_paths, vec!["keep.md"]);
        });
    }
}
