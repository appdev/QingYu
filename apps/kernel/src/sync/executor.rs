//! Production remote-sync execution owned by the Kernel.

use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fmt,
    path::{Path, PathBuf},
    sync::Arc,
};

use async_trait::async_trait;

use crate::{
    contract::{
        Revision, RunId, SafeUnsignedInteger, SyncProvider, SyncSafeErrorCategory,
        SyncSafeErrorCode, SyncSafeErrorDto, SyncSafeErrorOperation, SyncSummaryDto,
    },
    protected_paths::is_qingyu_control_directory_name,
    runtime::KernelRuntime,
    services::sync::{SyncExecutionError, SyncExecutor, SyncRunContext},
    settings::{
        model::portable_settings_from_bytes,
        service::{
            DeferredSettingsPublication, SettingsGroup, SettingsPublicationEvent, SettingsService,
        },
    },
    sync::{
        backend::{
            notebook_name_available_on_current_platform, sync_state_key, RemoteSyncBackend,
            RemoteSyncError, RemoteSyncFile, ValidRemoteRoot,
        },
        config::{SyncConfig, SyncExecutionPlan, SyncExecutionTarget},
        execution::{
            complete_remote_first_restore_locked,
            execute_remote_sync_pair_locked_with_cancellation, preserve_remote_settings_conflict,
            RemoteSyncSummary, SyncExecutionCancellation,
        },
        scope::RemoteSyncScope,
        settings_scope::{
            capture_portable_settings_manifest_revision, capture_settings_file_state,
            clear_portable_settings_manifest, clear_portable_settings_pending,
            portable_settings_pending_contains_legacy_mcp, read_portable_settings_pending,
            replace_portable_settings_stage, write_portable_settings_pending,
            PortableSettingsJournal, PortableSettingsJournalPhase,
        },
        webdav_backend::{WebDavBackend, WebDavSyncSettings},
    },
};

/// Kernel-owned executor for the providers whose full run lifecycle is composed.
///
/// S3 deliberately remains unavailable until its Dejavu runner owns the same
/// cancellation, state, and settings-publication boundaries as WebDAV.
pub(crate) struct ProductionSyncExecutor {
    runtime: Arc<KernelRuntime>,
    settings: Arc<SettingsService>,
}

impl ProductionSyncExecutor {
    pub(crate) fn new(runtime: Arc<KernelRuntime>, settings: Arc<SettingsService>) -> Self {
        Self { runtime, settings }
    }

    async fn run_webdav(
        &self,
        plan: SyncExecutionPlan,
        context: SyncRunContext,
    ) -> Result<SyncSummaryDto, SyncExecutionError> {
        let provider = plan.provider;
        let run_id = context.run_id();
        let SyncExecutionPlan {
            provider: _,
            remote_root,
            generate_conflict_document: _,
            target,
        } = plan;
        let SyncExecutionTarget::WebDav {
            server_url,
            username,
            password,
        } = target
        else {
            return Err(unavailable_error(provider, Some(run_id)));
        };

        self.runtime
            .verify_instance_lock()
            .map_err(|_| local_error(provider, run_id))?;
        let active = self
            .runtime
            .active_workspace_snapshot()
            .map_err(|_| local_error(provider, run_id))?;
        if active.identity() != context.snapshot_identity() {
            return Err(local_error(provider, run_id));
        }
        let authority = context.workspace_authority();
        authority
            .verify_held_directory()
            .map_err(|_| local_error(provider, run_id))?;
        let workspace_root = authority.root().canonical_path().to_path_buf();
        let workspace_directory = authority
            .root()
            .try_clone_dir()
            .map_err(|_| local_error(provider, run_id))?;
        let app_data_root = self
            .runtime
            .instance_data_root()
            .canonical_path()
            .to_path_buf();
        self.runtime
            .instance_data_root()
            .verify_held_directory()
            .map_err(|_| local_error(provider, run_id))?;

        let remote_root = ValidRemoteRoot::parse(&remote_root)
            .map_err(|_| configuration_error(provider, Some(run_id)))?;
        let notebook_name = notebook_name(&workspace_root)
            .map_err(|_| configuration_error(provider, Some(run_id)))?;
        let backend = WebDavBackend::connect(WebDavSyncSettings::new(
            server_url,
            username,
            password,
            remote_root.clone(),
        ))
        .await
        .map_err(|error| remote_error(provider, run_id, &error, None))?;
        let notes_backend = PrefixedRemoteBackend::new(&backend, format!("notes/{notebook_name}"));
        let settings_backend = PrefixedRemoteBackend::new(&backend, "app".to_string());
        let sync_state_root = app_data_root.join("sync-state");
        let workspace_identity = format!(
            "{}:{}",
            context.workspace().id.as_uuid(),
            context.workspace().generation.as_str()
        );
        let notes_state = sync_state_root.join("notes").join(sync_state_key(
            "notes",
            &[
                notes_backend.target_fingerprint_source().as_bytes(),
                remote_root.as_str().as_bytes(),
                workspace_identity.as_bytes(),
            ],
        ));
        let settings_state = sync_state_root.join("settings").join(sync_state_key(
            "settings",
            &[
                settings_backend.target_fingerprint_source().as_bytes(),
                remote_root.as_str().as_bytes(),
            ],
        ));
        let global_ignore_rules = self
            .settings
            .read_group(SettingsGroup::FileIgnoreSettings)
            .map_err(|_| local_error(provider, run_id))?
            .and_then(|value| {
                value
                    .get("rules")
                    .and_then(serde_json::Value::as_str)
                    .filter(|rules| !rules.is_empty())
                    .map(str::to_string)
            });
        let notes_scope = RemoteSyncScope::notes_from_prepared_directory(
            workspace_root,
            workspace_directory,
            notes_state,
            "manifest.json",
            Some(workspace_identity),
            global_ignore_rules,
        )
        .map_err(|_| local_error(provider, run_id))?;
        let mut prepared =
            prepare_portable_settings_sync(&self.settings, &app_data_root, settings_state.clone())
                .map_err(|_| local_error(provider, run_id))?;
        if prepared.phase == PortableSettingsJournalPhase::Publication {
            let pending = resume_portable_settings_publication(&self.settings, &prepared)
                .map_err(|_| local_error(provider, run_id))?;
            finish_portable_settings_publication(&self.settings, &prepared.scope, pending)
                .map_err(|_| local_error(provider, run_id))?;
            prepared =
                prepare_portable_settings_sync(&self.settings, &app_data_root, settings_state)
                    .map_err(|_| local_error(provider, run_id))?;
        }

        let service_cancellation = context.cancellation().clone();
        let cancellation =
            SyncExecutionCancellation::from_callback(move || service_cancellation.is_cancelled());
        let mut publication: Option<DeferredSettingsPublication> = None;
        let (notes_result, settings_result) = execute_remote_sync_pair_locked_with_cancellation(
            &notes_scope,
            &notes_backend,
            &prepared.scope,
            &settings_backend,
            &cancellation,
            |expected_local_hash| {
                publication = Some(reconcile_portable_settings(
                    &self.settings,
                    &prepared,
                    expected_local_hash,
                )?);
                Ok(())
            },
        )
        .await;

        let partial = combined_summary(
            notes_result.as_ref().ok(),
            settings_result
                .as_ref()
                .ok()
                .map(|outcome| &outcome.summary),
        )
        .map_err(|_| local_error(provider, run_id))?;

        if context.cancellation().is_cancelled() {
            if let Some(publication) = publication.take() {
                publication
                    .supersede()
                    .map_err(|_| local_error(provider, run_id))?;
                replace_publication_with_current_settings(&self.settings, &prepared.scope)
                    .map_err(|_| local_error(provider, run_id))?;
            }
            return Err(cancelled_error(provider, run_id).with_partial_summary(partial));
        }
        if let Some(publication) = publication.take() {
            finish_portable_settings_publication(&self.settings, &prepared.scope, publication)
                .map_err(|_| local_error(provider, run_id).with_partial_summary(partial.clone()))?;
        }
        if let Err(error) = notes_result {
            return Err(remote_error(provider, run_id, &error, Some(partial)));
        }
        if let Err(error) = settings_result {
            return Err(remote_error(provider, run_id, &error, Some(partial)));
        }
        complete_remote_first_restore_locked(&notes_scope)
            .map_err(|_| local_error(provider, run_id).with_partial_summary(partial.clone()))?;
        Ok(partial)
    }
}

impl fmt::Debug for ProductionSyncExecutor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ProductionSyncExecutor(..)")
    }
}

#[async_trait]
impl SyncExecutor for ProductionSyncExecutor {
    async fn test_connection(&self, config: SyncConfig) -> Result<(), SyncExecutionError> {
        let provider = config.provider();
        let plan = config
            .into_execution_plan()
            .map_err(|_| test_configuration_error(provider))?;
        match plan.target {
            SyncExecutionTarget::WebDav {
                server_url,
                username,
                password,
            } => {
                let remote_root = ValidRemoteRoot::parse(&plan.remote_root)
                    .map_err(|_| test_configuration_error(provider))?;
                let settings = WebDavSyncSettings::new(server_url, username, password, remote_root);
                WebDavBackend::test_connection(&settings)
                    .await
                    .map(|_| ())
                    .map_err(|error| connection_error(provider, &error))
            }
            SyncExecutionTarget::S3 { .. } => Err(test_unavailable_error(provider)),
        }
    }

    async fn run(
        &self,
        config: SyncConfig,
        context: SyncRunContext,
    ) -> Result<SyncSummaryDto, SyncExecutionError> {
        let provider = config.provider();
        let plan = config
            .into_execution_plan()
            .map_err(|_| configuration_error(provider, Some(context.run_id())))?;
        match plan.target {
            SyncExecutionTarget::WebDav { .. } => self.run_webdav(plan, context).await,
            SyncExecutionTarget::S3 { .. } => {
                Err(unavailable_error(provider, Some(context.run_id())))
            }
        }
    }
}

struct PreparedPortableSettingsSync {
    applied_portable_revision: Option<String>,
    expected_portable_revision: String,
    phase: PortableSettingsJournalPhase,
    publication_events: Vec<SettingsPublicationEvent>,
    scope: RemoteSyncScope,
}

fn prepare_portable_settings_sync(
    service: &SettingsService,
    app_data_root: &Path,
    settings_state: PathBuf,
) -> Result<PreparedPortableSettingsSync, String> {
    let scope = RemoteSyncScope::portable_settings(app_data_root, settings_state, "manifest.json")?;
    if portable_settings_pending_contains_legacy_mcp(&scope)? {
        clear_portable_settings_pending(&scope)?;
        clear_portable_settings_manifest(&scope)?;
    }
    let snapshot = service
        .portable_snapshot()
        .map_err(|_| settings_reconcile_error())?;
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
                if snapshot.revision().as_str() != journal.expected_portable_revision => {}
            PortableSettingsJournalPhase::Publication
                if journal.applied_portable_revision.as_deref()
                    != Some(snapshot.revision().as_str()) => {}
            PortableSettingsJournalPhase::Reconcile
                if snapshot.revision().as_str() != journal.expected_portable_revision =>
            {
                if journal.applied_portable_revision.as_deref()
                    == Some(snapshot.revision().as_str())
                {
                    journal.phase = PortableSettingsJournalPhase::Publication;
                    write_portable_settings_pending(&scope, &journal)?;
                    let staged = journal.staged_bytes()?;
                    replace_portable_settings_stage(&scope, staged.as_deref())?;
                    return Ok(prepared_from_journal(scope, journal));
                }
                let staged = journal.staged_bytes()?;
                preserve_remote_settings_conflict(&scope, staged.as_deref())?;
            }
            _ => {
                let staged = journal.staged_bytes()?;
                replace_portable_settings_stage(&scope, staged.as_deref())?;
                return Ok(prepared_from_journal(scope, journal));
            }
        }
    }
    let mut journal =
        PortableSettingsJournal::prepared(snapshot.revision().as_str(), snapshot.bytes());
    journal.prepared_manifest_revision = capture_portable_settings_manifest_revision(&scope)?;
    write_portable_settings_pending(&scope, &journal)?;
    replace_portable_settings_stage(&scope, snapshot.bytes())?;
    Ok(prepared_from_journal(scope, journal))
}

fn prepared_from_journal(
    scope: RemoteSyncScope,
    journal: PortableSettingsJournal,
) -> PreparedPortableSettingsSync {
    PreparedPortableSettingsSync {
        applied_portable_revision: journal.applied_portable_revision,
        expected_portable_revision: journal.expected_portable_revision,
        phase: journal.phase,
        publication_events: journal.publication_events,
        scope,
    }
}

fn reconcile_portable_settings(
    service: &SettingsService,
    prepared: &PreparedPortableSettingsSync,
    expected_local_hash: Option<&str>,
) -> Result<DeferredSettingsPublication, String> {
    let mut journal =
        read_portable_settings_pending(&prepared.scope)?.ok_or_else(settings_reconcile_error)?;
    if journal.phase == PortableSettingsJournalPhase::Publication {
        return resume_portable_settings_publication(service, prepared);
    }
    let staged = capture_settings_file_state(prepared.scope.source_root())?;
    journal.phase = PortableSettingsJournalPhase::Reconcile;
    journal.set_staged_bytes(staged.bytes());
    journal.expected_local_hash = expected_local_hash.map(str::to_string);
    write_portable_settings_pending(&prepared.scope, &journal)?;
    if !staged.matches_hash(expected_local_hash) {
        return Err(settings_reconcile_error());
    }
    let desired =
        portable_settings_from_bytes(staged.bytes()).map_err(|_| settings_reconcile_error())?;
    let expected_revision = Revision::parse(prepared.expected_portable_revision.clone())
        .map_err(|_| settings_reconcile_error())?;
    if service
        .portable_snapshot()
        .map_err(|_| settings_reconcile_error())?
        .revision()
        != &expected_revision
    {
        return Err(settings_reconcile_error());
    }
    let preview = service
        .preview_merge(staged.bytes(), &expected_revision)
        .map_err(|_| settings_reconcile_error())?;
    journal.applied_portable_revision = Some(preview.applied_revision().as_str().to_string());
    journal.publication_events = preview.publications().to_vec();
    write_portable_settings_pending(&prepared.scope, &journal)?;
    let publication = service
        .replace_portable_deferred_with_preflight_and_verify(
            staged.bytes(),
            &expected_revision,
            || {
                capture_settings_file_state(prepared.scope.source_root())
                    .is_ok_and(|actual| actual == staged)
            },
            |actual| actual == &desired,
        )
        .map_err(|_| settings_reconcile_error())?;
    journal.phase = PortableSettingsJournalPhase::Publication;
    if let Err(error) = write_portable_settings_pending(&prepared.scope, &journal) {
        publication
            .supersede()
            .map_err(|_| settings_reconcile_error())?;
        return Err(error);
    }
    Ok(publication)
}

fn resume_portable_settings_publication(
    service: &SettingsService,
    prepared: &PreparedPortableSettingsSync,
) -> Result<DeferredSettingsPublication, String> {
    let revision = prepared
        .applied_portable_revision
        .as_deref()
        .ok_or_else(settings_reconcile_error)
        .and_then(|value| {
            Revision::parse(value.to_string()).map_err(|_| settings_reconcile_error())
        })?;
    service
        .resume_portable_publication(&revision, prepared.publication_events.clone())
        .map_err(|_| settings_reconcile_error())
}

fn finish_portable_settings_publication(
    service: &SettingsService,
    scope: &RemoteSyncScope,
    publication: DeferredSettingsPublication,
) -> Result<(), String> {
    let journal = match read_portable_settings_pending(scope) {
        Ok(Some(journal)) if journal.phase == PortableSettingsJournalPhase::Publication => journal,
        _ => {
            publication
                .supersede()
                .map_err(|_| settings_reconcile_error())?;
            return Err(settings_reconcile_error());
        }
    };
    let Some(expected_revision) = journal.applied_portable_revision.as_deref() else {
        publication
            .supersede()
            .map_err(|_| settings_reconcile_error())?;
        return Err(settings_reconcile_error());
    };
    let expected_revision = match Revision::parse(expected_revision.to_string()) {
        Ok(revision) => revision,
        Err(_) => {
            publication
                .supersede()
                .map_err(|_| settings_reconcile_error())?;
            return Err(settings_reconcile_error());
        }
    };
    let current = match service.portable_snapshot() {
        Ok(snapshot) => snapshot,
        Err(_) => {
            publication
                .supersede()
                .map_err(|_| settings_reconcile_error())?;
            return Err(settings_reconcile_error());
        }
    };
    if current.revision() != &expected_revision {
        publication
            .supersede()
            .map_err(|_| settings_reconcile_error())?;
        replace_publication_with_current_settings(service, scope)?;
        return Err(settings_reconcile_error());
    }
    if !service
        .publish_if_portable_revision(publication, &expected_revision)
        .map_err(|_| settings_reconcile_error())?
    {
        replace_publication_with_current_settings(service, scope)?;
        return Err(settings_reconcile_error());
    }
    clear_portable_settings_pending(scope)
}

fn replace_publication_with_current_settings(
    service: &SettingsService,
    scope: &RemoteSyncScope,
) -> Result<(), String> {
    loop {
        let snapshot = service
            .portable_snapshot()
            .map_err(|_| settings_reconcile_error())?;
        let mut journal =
            PortableSettingsJournal::prepared(snapshot.revision().as_str(), snapshot.bytes());
        journal.prepared_manifest_revision = capture_portable_settings_manifest_revision(scope)?;
        write_portable_settings_pending(scope, &journal)?;
        replace_portable_settings_stage(scope, snapshot.bytes())?;
        if service
            .portable_snapshot()
            .map_err(|_| settings_reconcile_error())?
            .revision()
            == snapshot.revision()
        {
            return Ok(());
        }
    }
}

fn settings_reconcile_error() -> String {
    "settings-reconcile-failed: Portable settings are unavailable.".to_string()
}

struct PrefixedRemoteBackend<'a, Backend> {
    backend: &'a Backend,
    prefix: String,
}

impl<'a, Backend> PrefixedRemoteBackend<'a, Backend> {
    fn new(backend: &'a Backend, prefix: String) -> Self {
        Self { backend, prefix }
    }

    fn provider_path(&self, path: &str) -> String {
        format!("{}/{path}", self.prefix)
    }
}

impl<Backend: RemoteSyncBackend> RemoteSyncBackend for PrefixedRemoteBackend<'_, Backend> {
    fn target_fingerprint_source(&self) -> String {
        format!(
            "{}|prefix={}",
            self.backend.target_fingerprint_source(),
            self.prefix
        )
    }

    async fn list_files(&self) -> Result<BTreeMap<String, RemoteSyncFile>, RemoteSyncError> {
        let prefix = format!("{}/", self.prefix);
        Ok(self
            .backend
            .list_files()
            .await?
            .into_iter()
            .filter_map(|(path, file)| {
                path.strip_prefix(&prefix)
                    .filter(|relative| !relative.is_empty())
                    .map(|relative| (relative.to_string(), file))
            })
            .collect())
    }

    async fn download(
        &self,
        path: &str,
        expected_identity: &str,
    ) -> Result<Vec<u8>, RemoteSyncError> {
        self.backend
            .download(&self.provider_path(path), expected_identity)
            .await
    }

    async fn upload(
        &self,
        path: &str,
        bytes: &[u8],
        expected_identity: Option<&str>,
    ) -> Result<String, RemoteSyncError> {
        self.backend
            .upload(&self.provider_path(path), bytes, expected_identity)
            .await
    }

    async fn delete(&self, path: &str, expected_identity: &str) -> Result<(), RemoteSyncError> {
        self.backend
            .delete(&self.provider_path(path), expected_identity)
            .await
    }
}

fn notebook_name(root: &Path) -> Result<String, ()> {
    let name = root.file_name().and_then(OsStr::to_str).ok_or(())?;
    if name.is_empty()
        || matches!(name, "." | "..")
        || name.contains(['/', '\\', '\0'])
        || is_qingyu_control_directory_name(OsStr::new(name))
        || !notebook_name_available_on_current_platform(name)
    {
        return Err(());
    }
    Ok(name.to_string())
}

fn combined_summary(
    notes: Option<&RemoteSyncSummary>,
    settings: Option<&RemoteSyncSummary>,
) -> Result<SyncSummaryDto, ()> {
    fn value(
        notes: Option<&RemoteSyncSummary>,
        settings: Option<&RemoteSyncSummary>,
        select: impl Fn(&RemoteSyncSummary) -> u64,
    ) -> Result<SafeUnsignedInteger, ()> {
        let notes = notes.map(&select).unwrap_or(0);
        let settings = settings.map(select).unwrap_or(0);
        notes
            .checked_add(settings)
            .and_then(|value| SafeUnsignedInteger::new(value).ok())
            .ok_or(())
    }

    Ok(SyncSummaryDto {
        bytes_downloaded: value(notes, settings, |summary| summary.bytes_downloaded)?,
        bytes_uploaded: value(notes, settings, |summary| summary.bytes_uploaded)?,
        conflict_files: value(notes, settings, |summary| summary.conflict_files)?,
        downloaded_files: value(notes, settings, |summary| summary.downloaded_files)?,
        scanned_files: value(notes, settings, |summary| summary.scanned_files)?,
        skipped_files: value(notes, settings, |summary| summary.skipped_files)?,
        uploaded_files: value(notes, settings, |summary| summary.uploaded_files)?,
    })
}

fn execution_error(
    provider: SyncProvider,
    operation: SyncSafeErrorOperation,
    code: SyncSafeErrorCode,
    category: Option<SyncSafeErrorCategory>,
    run_id: Option<RunId>,
) -> SyncExecutionError {
    let mut safe = SyncSafeErrorDto::new(provider, operation, code);
    if let Some(category) = category {
        safe = safe.with_category(category);
    }
    if let Some(run_id) = run_id {
        safe = safe.with_run_id(run_id);
    }
    SyncExecutionError::new(safe)
}

fn configuration_error(provider: SyncProvider, run_id: Option<RunId>) -> SyncExecutionError {
    execution_error(
        provider,
        SyncSafeErrorOperation::SyncRun,
        SyncSafeErrorCode::ConfigurationInvalid,
        Some(SyncSafeErrorCategory::Configuration),
        run_id,
    )
}

fn test_configuration_error(provider: SyncProvider) -> SyncExecutionError {
    execution_error(
        provider,
        SyncSafeErrorOperation::TestConnection,
        SyncSafeErrorCode::ConfigurationInvalid,
        Some(SyncSafeErrorCategory::Configuration),
        None,
    )
}

fn unavailable_error(provider: SyncProvider, run_id: Option<RunId>) -> SyncExecutionError {
    execution_error(
        provider,
        SyncSafeErrorOperation::SyncRun,
        SyncSafeErrorCode::RemoteUnavailable,
        Some(SyncSafeErrorCategory::Provider),
        run_id,
    )
}

fn test_unavailable_error(provider: SyncProvider) -> SyncExecutionError {
    execution_error(
        provider,
        SyncSafeErrorOperation::TestConnection,
        SyncSafeErrorCode::RemoteUnavailable,
        Some(SyncSafeErrorCategory::Provider),
        None,
    )
}

fn local_error(provider: SyncProvider, run_id: RunId) -> SyncExecutionError {
    execution_error(
        provider,
        SyncSafeErrorOperation::SyncRun,
        SyncSafeErrorCode::LocalIo,
        Some(SyncSafeErrorCategory::Storage),
        Some(run_id),
    )
}

fn cancelled_error(provider: SyncProvider, run_id: RunId) -> SyncExecutionError {
    execution_error(
        provider,
        SyncSafeErrorOperation::SyncRun,
        SyncSafeErrorCode::Cancelled,
        None,
        Some(run_id),
    )
}

fn connection_error(provider: SyncProvider, error: &RemoteSyncError) -> SyncExecutionError {
    let (code, category) = match error.safe_code() {
        "webdav-endpoint-invalid" | "webdav-remote-path-invalid" => (
            SyncSafeErrorCode::ConfigurationInvalid,
            SyncSafeErrorCategory::Configuration,
        ),
        "webdav-transport-failed" => (
            SyncSafeErrorCode::ConnectionFailed,
            SyncSafeErrorCategory::Network,
        ),
        _ => (
            SyncSafeErrorCode::RequestFailed,
            SyncSafeErrorCategory::Provider,
        ),
    };
    execution_error(
        provider,
        SyncSafeErrorOperation::TestConnection,
        code,
        Some(category),
        None,
    )
}

fn remote_error(
    provider: SyncProvider,
    run_id: RunId,
    error: &RemoteSyncError,
    partial: Option<SyncSummaryDto>,
) -> SyncExecutionError {
    let (code, category) = match error.safe_code() {
        "sync-run-cancelled" => (SyncSafeErrorCode::Cancelled, None),
        "webdav-endpoint-invalid" | "webdav-remote-path-invalid" => (
            SyncSafeErrorCode::ConfigurationInvalid,
            Some(SyncSafeErrorCategory::Configuration),
        ),
        "webdav-remote-changed" => (
            SyncSafeErrorCode::Conflict,
            Some(SyncSafeErrorCategory::Conflict),
        ),
        "webdav-transport-failed" => (
            SyncSafeErrorCode::RemoteUnavailable,
            Some(SyncSafeErrorCategory::Network),
        ),
        "webdav-http-failed" => (
            SyncSafeErrorCode::RequestFailed,
            Some(SyncSafeErrorCategory::Provider),
        ),
        _ => (
            SyncSafeErrorCode::LocalIo,
            Some(SyncSafeErrorCategory::Storage),
        ),
    };
    let result = execution_error(
        provider,
        SyncSafeErrorOperation::SyncRun,
        code,
        category,
        Some(run_id),
    );
    partial.map_or(result.clone(), |summary| {
        result.with_partial_summary(summary)
    })
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, BTreeSet},
        io::{Read as _, Write as _},
        net::{TcpListener, TcpStream},
        path::{Path, PathBuf},
        sync::{
            atomic::{AtomicBool, AtomicU64, Ordering},
            Arc, Mutex,
        },
        thread,
        time::Duration,
    };

    use tempfile::{tempdir, TempDir};

    use crate::{
        config::KernelConfig,
        contract::{SyncCompletionState, TriggerSyncRunRequest},
        events::EventSink,
        paths::KernelPaths,
        ports::system::system_kernel_ports,
        runtime::{KernelRuntime, SyncApiService},
        services::{
            sync::{SyncExecutor as _, SyncService},
            workspace::WorkspaceService,
        },
        settings::{
            service::SettingsService,
            storage::{AtomicJsonSettingsStore, SettingsStore as _},
        },
        storage::DurableFileStore,
        sync::config::{SyncConfig, SyncConfigStore},
        workspace::{
            managed::ManagedWorkspaceCollection,
            primary::{FixedPrimaryWorkspaceStore, PrimaryWorkspaceState},
        },
    };

    use super::ProductionSyncExecutor;

    #[tokio::test]
    async fn connection_test_is_read_only_and_uses_the_nearest_existing_webdav_parent() {
        let server = WebDavFixture::start();
        let kernel = TestKernel::start(&server.endpoint(), false).await;
        let config = webdav_config(&server.endpoint(), "qingyu/team", "connection-secret");

        kernel
            .executor
            .test_connection(config)
            .await
            .expect("the existing fixture root should be a valid read-only connection target");

        let requests = server.requests();
        assert_eq!(
            requests,
            vec![
                "PROPFIND /dav/qingyu/team/".to_string(),
                "PROPFIND /dav/qingyu/".to_string(),
                "PROPFIND /dav/".to_string(),
            ]
        );
        assert!(requests.iter().all(|request| {
            !request.starts_with("MKCOL ")
                && !request.starts_with("PUT ")
                && !request.starts_with("DELETE ")
        }));
    }

    #[tokio::test]
    async fn one_webdav_run_syncs_notes_and_portable_settings_from_the_retained_workspace() {
        let server = WebDavFixture::start();
        let kernel = TestKernel::start(&server.endpoint(), true).await;
        std::fs::write(kernel.workspace.join("note.md"), b"retained workspace note")
            .expect("write local note");
        let sync = SyncService::new(
            kernel.runtime.clone(),
            Arc::new(SyncConfigStore::new(kernel.sync_store).expect("sync config store")),
            kernel.executor.clone(),
        );
        let exposed = SyncApiService::get_sync_config(&sync)
            .await
            .expect("ready sync config");

        SyncApiService::trigger_sync_run(
            &sync,
            TriggerSyncRunRequest {
                expected_config_revision: exposed.revision,
            },
        )
        .await
        .expect("accept production sync run");

        let completed = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let status = SyncApiService::get_sync_status(&sync)
                    .await
                    .expect("read sync status");
                if status.completion_state != SyncCompletionState::Attempting {
                    break status;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("production sync should complete");

        assert_eq!(completed.completion_state, SyncCompletionState::Succeeded);
        assert!(completed.error.as_ref().is_none());
        assert_eq!(
            server.file("/dav/qingyu/notes/Workspace/note.md"),
            Some(b"retained workspace note".to_vec())
        );
        let remote_settings = server
            .file("/dav/qingyu/app/settings.json")
            .expect("portable settings should be uploaded");
        let remote_settings: serde_json::Value =
            serde_json::from_slice(&remote_settings).expect("portable settings JSON");
        assert_eq!(remote_settings, serde_json::json!({ "language": "en" }));
        assert!(has_manifest_below(
            &kernel.app_data.join("sync-state/notes")
        ));
        assert!(has_manifest_below(
            &kernel.app_data.join("sync-state/settings")
        ));
    }

    #[tokio::test]
    async fn s3_remains_safely_unavailable_without_exposing_credentials() {
        let server = WebDavFixture::start();
        let kernel = TestKernel::start(&server.endpoint(), false).await;
        let config: SyncConfig = serde_json::from_value(serde_json::json!({
            "version": 3,
            "enabled": true,
            "provider": "s3",
            "remoteRoot": "qingyu",
            "mode": "automatic",
            "intervalSeconds": 30,
            "generateConflictDocument": false,
            "webdav": { "serverUrl": "", "username": "", "password": "" },
            "s3": {
                "endpointUrl": "https://s3.example.test",
                "region": "test-1",
                "bucket": "notes",
                "accessKeyId": "executor-access-secret",
                "secretAccessKey": "executor-signing-secret",
                "requestTimeoutSeconds": 60,
                "addressingStyle": "auto",
                "tlsVerification": "verify"
            }
        }))
        .expect("valid S3 config");

        let error = kernel
            .executor
            .test_connection(config)
            .await
            .expect_err("S3 stays unavailable until the Dejavu runner is composed");
        let rendered = format!("{error:?} {error}");

        assert_eq!(rendered.matches("sync execution failed").count(), 1);
        assert!(!rendered.contains("executor-access-secret"));
        assert!(!rendered.contains("executor-signing-secret"));
        assert!(server.requests().is_empty());
    }

    struct TestKernel {
        _temporary: TempDir,
        app_data: PathBuf,
        executor: Arc<ProductionSyncExecutor>,
        runtime: Arc<KernelRuntime>,
        sync_store: DurableFileStore,
        workspace: PathBuf,
        _workspace_service: WorkspaceService,
    }

    impl TestKernel {
        async fn start(endpoint: &str, ready_sync_config: bool) -> Self {
            let temporary = tempdir().expect("temporary Kernel roots");
            let workspace = temporary.path().join("Workspace");
            let app_data = temporary.path().join("app-data");
            let cache = temporary.path().join("cache");
            std::fs::create_dir(&workspace).expect("workspace");
            std::fs::create_dir(&app_data).expect("app data");
            std::fs::create_dir(&cache).expect("cache");
            if ready_sync_config {
                let config = webdav_config(endpoint, "qingyu", "run-secret");
                std::fs::write(
                    app_data.join("sync-config.json"),
                    serde_json::to_vec_pretty(&config).expect("serialize sync config"),
                )
                .expect("write sync config");
            }

            let config = KernelConfig::generate().expect("Kernel config");
            let paths = KernelPaths::desktop(&workspace, &app_data, &cache).expect("Kernel paths");
            let managed =
                ManagedWorkspaceCollection::from_paths(&paths).expect("managed collection");
            let settings_store = DurableFileStore::at_instance_data(
                paths.instance_data_root(),
                config.launch_epoch(),
            )
            .expect("settings durable store");
            let sync_store = DurableFileStore::at_instance_data(
                paths.instance_data_root(),
                config.launch_epoch(),
            )
            .expect("sync durable store");
            let settings_store =
                Arc::new(AtomicJsonSettingsStore::new(settings_store).expect("settings store"));
            settings_store
                .set("language", serde_json::json!("en"))
                .expect("set portable language");
            settings_store.save().expect("save portable language");
            let runtime = KernelRuntime::activate(config, paths, system_kernel_ports())
                .expect("Kernel runtime");
            let primary = PrimaryWorkspaceState::new("Workspace").expect("primary workspace");
            let primary = Arc::new(
                FixedPrimaryWorkspaceStore::new(primary).expect("fixed primary workspace"),
            );
            let events: Arc<dyn EventSink> = runtime.clone();
            let workspace_service =
                WorkspaceService::new(&runtime, primary, managed, events, "Workspace")
                    .await
                    .expect("active workspace snapshot");
            let settings = Arc::new(SettingsService::new(settings_store, runtime.clone()));
            let executor = Arc::new(ProductionSyncExecutor::new(runtime.clone(), settings));

            Self {
                _temporary: temporary,
                app_data,
                executor,
                runtime,
                sync_store,
                workspace,
                _workspace_service: workspace_service,
            }
        }
    }

    fn webdav_config(endpoint: &str, remote_root: &str, password: &str) -> SyncConfig {
        serde_json::from_value(serde_json::json!({
            "version": 3,
            "enabled": true,
            "provider": "webdav",
            "remoteRoot": remote_root,
            "mode": "automatic",
            "intervalSeconds": 30,
            "generateConflictDocument": false,
            "webdav": {
                "serverUrl": endpoint,
                "username": "fixture-user",
                "password": password
            },
            "s3": {
                "endpointUrl": "",
                "region": "",
                "bucket": "",
                "accessKeyId": "",
                "secretAccessKey": "",
                "requestTimeoutSeconds": 60,
                "addressingStyle": "auto",
                "tlsVerification": "verify"
            }
        }))
        .expect("valid WebDAV config")
    }

    fn has_manifest_below(root: &Path) -> bool {
        let Ok(entries) = std::fs::read_dir(root) else {
            return false;
        };
        entries.filter_map(Result::ok).any(|entry| {
            let path = entry.path();
            path.file_name().is_some_and(|name| name == "manifest.json")
                || (path.is_dir() && has_manifest_below(&path))
        })
    }

    struct WebDavFixture {
        address: std::net::SocketAddr,
        requests: Arc<Mutex<Vec<String>>>,
        state: Arc<Mutex<WebDavState>>,
        stop: Arc<AtomicBool>,
        thread: Option<thread::JoinHandle<()>>,
    }

    #[derive(Default)]
    struct WebDavState {
        directories: BTreeSet<String>,
        files: BTreeMap<String, StoredFixtureFile>,
    }

    struct StoredFixtureFile {
        bytes: Vec<u8>,
        version: u64,
    }

    impl WebDavFixture {
        fn start() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("WebDAV fixture bind");
            listener
                .set_nonblocking(true)
                .expect("nonblocking WebDAV fixture");
            let address = listener.local_addr().expect("WebDAV fixture address");
            let requests = Arc::new(Mutex::new(Vec::new()));
            let state = Arc::new(Mutex::new(WebDavState {
                directories: BTreeSet::from(["/dav/".to_string()]),
                files: BTreeMap::new(),
            }));
            let stop = Arc::new(AtomicBool::new(false));
            let recorded = requests.clone();
            let shared_state = state.clone();
            let should_stop = stop.clone();
            let thread = thread::spawn(move || {
                while !should_stop.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            stream
                                .set_nonblocking(false)
                                .expect("blocking accepted WebDAV connection");
                            handle_request(stream, &recorded, &shared_state);
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(1));
                        }
                        Err(_) => break,
                    }
                }
            });
            Self {
                address,
                requests,
                state,
                stop,
                thread: Some(thread),
            }
        }

        fn endpoint(&self) -> String {
            format!("http://{}/dav", self.address)
        }

        fn requests(&self) -> Vec<String> {
            self.requests.lock().expect("request log").clone()
        }

        fn file(&self, path: &str) -> Option<Vec<u8>> {
            self.state
                .lock()
                .expect("WebDAV state")
                .files
                .get(path)
                .map(|file| file.bytes.clone())
        }
    }

    impl Drop for WebDavFixture {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            let _wake = TcpStream::connect(self.address);
            if let Some(thread) = self.thread.take() {
                thread.join().expect("WebDAV fixture thread");
            }
        }
    }

    struct FixtureRequest {
        body: Vec<u8>,
        depth: Option<String>,
        method: String,
        path: String,
    }

    fn handle_request(
        mut stream: TcpStream,
        requests: &Mutex<Vec<String>>,
        state: &Mutex<WebDavState>,
    ) {
        let request = read_request(&mut stream);
        requests
            .lock()
            .expect("request log")
            .push(format!("{} {}", request.method, request.path));
        let response = match request.method.as_str() {
            "MKCOL" => fixture_mkcol(state, &request.path),
            "PROPFIND" => fixture_propfind(state, &request.path, request.depth.as_deref()),
            "PUT" => fixture_put(state, &request.path, request.body),
            "GET" => fixture_get(state, &request.path, false),
            "HEAD" => fixture_get(state, &request.path, true),
            "DELETE" => fixture_delete(state, &request.path),
            _ => empty_response("405 Method Not Allowed"),
        };
        stream
            .write_all(&response)
            .expect("write WebDAV fixture response");
    }

    fn read_request(stream: &mut TcpStream) -> FixtureRequest {
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("fixture read timeout");
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        let header_end = loop {
            let read = stream.read(&mut buffer).expect("read WebDAV request");
            assert_ne!(read, 0, "request ended before headers");
            bytes.extend_from_slice(&buffer[..read]);
            if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                break index + 4;
            }
        };
        let headers = String::from_utf8_lossy(&bytes[..header_end]);
        let mut lines = headers.lines();
        let mut start = lines.next().expect("request line").split_whitespace();
        let method = start.next().expect("request method").to_string();
        let path = start.next().expect("request path").to_string();
        let mut content_length = 0_usize;
        let mut depth = None;
        for line in lines {
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().expect("content length");
            } else if name.eq_ignore_ascii_case("depth") {
                depth = Some(value.trim().to_string());
            }
        }
        while bytes.len() < header_end + content_length {
            let read = stream.read(&mut buffer).expect("read WebDAV request body");
            assert_ne!(read, 0, "request ended before body");
            bytes.extend_from_slice(&buffer[..read]);
        }
        FixtureRequest {
            body: bytes[header_end..header_end + content_length].to_vec(),
            depth,
            method,
            path,
        }
    }

    fn fixture_mkcol(state: &Mutex<WebDavState>, path: &str) -> Vec<u8> {
        let path = directory_path(path);
        let mut state = state.lock().expect("WebDAV state");
        if state.directories.insert(path) {
            empty_response("201 Created")
        } else {
            empty_response("405 Method Not Allowed")
        }
    }

    fn fixture_propfind(state: &Mutex<WebDavState>, path: &str, depth: Option<&str>) -> Vec<u8> {
        let state = state.lock().expect("WebDAV state");
        let directory = directory_path(path);
        let is_directory = state.directories.contains(&directory);
        let file = state.files.get(path);
        if !is_directory && file.is_none() {
            return empty_response("404 Not Found");
        }
        let mut entries = Vec::new();
        if is_directory {
            entries.push(directory_response(&directory));
            if depth == Some("1") {
                for child in state
                    .directories
                    .iter()
                    .filter(|candidate| immediate_child(&directory, candidate))
                {
                    entries.push(directory_response(child));
                }
                for (child, file) in state
                    .files
                    .iter()
                    .filter(|(candidate, _)| immediate_file_child(&directory, candidate))
                {
                    entries.push(file_response(child, file));
                }
            }
        } else if let Some(file) = file {
            entries.push(file_response(path, file));
        }
        xml_response(entries.join(""))
    }

    fn fixture_put(state: &Mutex<WebDavState>, path: &str, body: Vec<u8>) -> Vec<u8> {
        static VERSION: AtomicU64 = AtomicU64::new(1);
        let version = VERSION.fetch_add(1, Ordering::Relaxed);
        state.lock().expect("WebDAV state").files.insert(
            path.to_string(),
            StoredFixtureFile {
                bytes: body,
                version,
            },
        );
        format!(
            "HTTP/1.1 201 Created\r\nETag: \"v{version}\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
        .into_bytes()
    }

    fn fixture_get(state: &Mutex<WebDavState>, path: &str, head_only: bool) -> Vec<u8> {
        let state = state.lock().expect("WebDAV state");
        let Some(file) = state.files.get(path) else {
            return empty_response("404 Not Found");
        };
        let body = if head_only {
            &[][..]
        } else {
            file.bytes.as_slice()
        };
        let mut response = format!(
            "HTTP/1.1 200 OK\r\nETag: \"v{}\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            file.version,
            file.bytes.len()
        )
        .into_bytes();
        response.extend_from_slice(body);
        response
    }

    fn fixture_delete(state: &Mutex<WebDavState>, path: &str) -> Vec<u8> {
        state.lock().expect("WebDAV state").files.remove(path);
        empty_response("204 No Content")
    }

    fn directory_path(path: &str) -> String {
        if path.ends_with('/') {
            path.to_string()
        } else {
            format!("{path}/")
        }
    }

    fn immediate_child(parent: &str, child: &str) -> bool {
        child.strip_prefix(parent).is_some_and(|relative| {
            !relative.is_empty() && relative.trim_end_matches('/').find('/').is_none()
        })
    }

    fn immediate_file_child(parent: &str, child: &str) -> bool {
        child
            .strip_prefix(parent)
            .is_some_and(|relative| !relative.is_empty() && !relative.contains('/'))
    }

    fn directory_response(path: &str) -> String {
        format!(
            "<d:response><d:href>{path}</d:href><d:propstat><d:prop><d:resourcetype><d:collection /></d:resourcetype></d:prop></d:propstat></d:response>"
        )
    }

    fn file_response(path: &str, file: &StoredFixtureFile) -> String {
        format!(
            "<d:response><d:href>{path}</d:href><d:propstat><d:prop><d:getetag>&quot;v{}&quot;</d:getetag><d:getcontentlength>{}</d:getcontentlength><d:resourcetype /></d:prop></d:propstat></d:response>",
            file.version,
            file.bytes.len()
        )
    }

    fn xml_response(entries: String) -> Vec<u8> {
        let body = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?><d:multistatus xmlns:d=\"DAV:\">{entries}</d:multistatus>"
        );
        format!(
            "HTTP/1.1 207 Multi-Status\r\nContent-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    fn empty_response(status: &str) -> Vec<u8> {
        format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").into_bytes()
    }
}
