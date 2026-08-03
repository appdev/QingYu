//! Production remote-sync execution owned by the Kernel.

use std::{
    collections::BTreeMap,
    fmt,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Weak,
    },
    time::Duration,
};

use async_trait::async_trait;
use qingyu_dejavu::{
    CloudError, Device, RepositoryRuntimeState, S3AddressingStyle as DejavuAddressingStyle,
    S3TlsVerification as DejavuTlsVerification,
};

use crate::{
    contract::{
        DocumentName, Revision, RunId, S3AddressingStyle, S3TlsVerification, SafeUnsignedInteger,
        SyncProvider, SyncSafeErrorCategory, SyncSafeErrorCode, SyncSafeErrorDto,
        SyncSafeErrorOperation, SyncSummaryDto, WorkspaceRelativePath, MAX_SAFE_INTEGER,
    },
    runtime::{ActiveInstanceAuthority, KernelRuntime},
    services::sync::{SyncExecutionError, SyncExecutor, SyncRunContext},
    settings::{
        model::portable_settings_from_bytes,
        service::{
            DeferredSettingsPublication, SettingsGroup, SettingsPublicationEvent, SettingsService,
        },
    },
    sync::{
        backend::{
            sync_state_key, RemoteSyncBackend, RemoteSyncError, RemoteSyncFile,
            SyncFailureCategory, ValidRemoteRoot,
        },
        catalog::KernelS3RepositoryCatalog,
        config::{SyncConfig, SyncExecutionPlan, SyncExecutionTarget},
        dejavu_runner::{
            DejavuRunError, DejavuRunResult, DejavuRunnerInputs, DejavuS3Config, DejavuSecret,
            KernelDejavuRunner, MutationWorkingTreeCoordinator,
        },
        execution::{
            complete_remote_first_restore_locked,
            execute_portable_settings_sync_locked_with_cancellation,
            execute_remote_sync_pair_locked_with_cancellation, preserve_remote_settings_conflict,
            RemoteSyncSummary, SyncExecutionCancellation,
        },
        local_state::{read_active_dejavu_binding, DejavuLocalStateError},
        s3_backend::{S3Backend, S3SyncSettings, S3TransportOptions},
        scope::RemoteSyncScope,
        settings_scope::{
            capture_portable_settings_manifest_revision, capture_scoped_settings_file_state,
            clear_portable_settings_manifest, clear_portable_settings_pending,
            portable_settings_pending_contains_legacy_mcp, read_portable_settings_pending,
            replace_portable_settings_stage, write_portable_settings_pending,
            PortableSettingsJournal, PortableSettingsJournalPhase,
        },
        webdav_backend::{WebDavBackend, WebDavSyncSettings},
    },
};

/// Kernel-owned executor for the providers whose full run lifecycle is composed.
pub(crate) struct ProductionSyncExecutor {
    runtime: Weak<KernelRuntime>,
    settings: Arc<SettingsService>,
    dejavu_factory: Arc<dyn DejavuRunnerFactory>,
    dejavu_runtime: RepositoryRuntimeState,
}

impl ProductionSyncExecutor {
    pub(crate) fn new(runtime: Arc<KernelRuntime>, settings: Arc<SettingsService>) -> Self {
        Self {
            runtime: Arc::downgrade(&runtime),
            settings,
            dejavu_factory: Arc::new(ProductionDejavuRunnerFactory),
            dejavu_runtime: RepositoryRuntimeState::default(),
        }
    }

    #[cfg(test)]
    fn new_with_dejavu_factory(
        runtime: Arc<KernelRuntime>,
        settings: Arc<SettingsService>,
        dejavu_factory: Arc<dyn DejavuRunnerFactory>,
    ) -> Self {
        Self {
            runtime: Arc::downgrade(&runtime),
            settings,
            dejavu_factory,
            dejavu_runtime: RepositoryRuntimeState::default(),
        }
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

        let runtime = self
            .runtime
            .upgrade()
            .ok_or_else(|| local_error(provider, run_id))?;
        runtime
            .verify_instance_lock()
            .map_err(|_| local_error(provider, run_id))?;
        let active = runtime
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
        let notebook_name = workspace_root_portable_name(&workspace_root)
            .map_err(|error| dejavu_error(provider, run_id, error))?;
        let workspace_directory = authority
            .root()
            .try_clone_dir()
            .map_err(|_| local_error(provider, run_id))?;
        let instance_authority = runtime.active_instance_authority();
        instance_authority
            .verify_held_directory()
            .map_err(|_| local_error(provider, run_id))?;
        let app_data_root = instance_authority.root().canonical_path().to_path_buf();
        let app_data_directory = instance_authority
            .root()
            .try_clone_dir()
            .map_err(|_| local_error(provider, run_id))?;

        let remote_root = ValidRemoteRoot::parse(&remote_root)
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
        let settings_state_relative =
            PathBuf::from("sync-state")
                .join("settings")
                .join(sync_state_key(
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
        let mut prepared = prepare_portable_settings_sync(
            &self.settings,
            &app_data_root,
            &app_data_directory,
            settings_state_relative.clone(),
            &instance_authority,
        )
        .map_err(|_| local_error(provider, run_id))?;
        if prepared.phase == PortableSettingsJournalPhase::Publication {
            let pending = resume_portable_settings_publication(
                &self.settings,
                &prepared,
                &instance_authority,
            )
            .map_err(|_| local_error(provider, run_id))?;
            finish_portable_settings_publication(
                &self.settings,
                &prepared.scope,
                pending,
                &instance_authority,
            )
            .map_err(|_| local_error(provider, run_id))?;
            prepared = prepare_portable_settings_sync(
                &self.settings,
                &app_data_root,
                &app_data_directory,
                settings_state_relative,
                &instance_authority,
            )
            .map_err(|_| local_error(provider, run_id))?;
        }

        let service_cancellation = context.cancellation().clone();
        let instance_invalidated = Arc::new(AtomicBool::new(false));
        let invalidated_for_cancellation = Arc::clone(&instance_invalidated);
        let instance_for_cancellation = Arc::clone(&instance_authority);
        let cancellation = SyncExecutionCancellation::from_callback(move || {
            if service_cancellation.is_cancelled() {
                return true;
            }
            if instance_for_cancellation.verify_held_directory().is_err() {
                invalidated_for_cancellation.store(true, Ordering::Release);
                return true;
            }
            false
        });
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
                    &instance_authority,
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

        if instance_invalidated.load(Ordering::Acquire)
            || instance_authority.verify_held_directory().is_err()
        {
            if let Some(publication) = publication.take() {
                publication.supersede().map_err(|_| {
                    local_error(provider, run_id).with_partial_summary(partial.clone())
                })?;
            }
            return Err(local_error(provider, run_id).with_partial_summary(partial));
        }
        if context.cancellation().is_cancelled() {
            if let Some(publication) = publication.take() {
                publication.supersede().map_err(|_| {
                    local_error(provider, run_id).with_partial_summary(partial.clone())
                })?;
                replace_publication_with_current_settings(
                    &self.settings,
                    &prepared.scope,
                    &instance_authority,
                )
                .map_err(|_| local_error(provider, run_id).with_partial_summary(partial.clone()))?;
            }
            return Err(cancelled_error(provider, run_id).with_partial_summary(partial));
        }
        if let Some(publication) = publication.take() {
            finish_portable_settings_publication(
                &self.settings,
                &prepared.scope,
                publication,
                &instance_authority,
            )
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

    async fn run_s3(
        &self,
        plan: SyncExecutionPlan,
        context: SyncRunContext,
    ) -> Result<SyncSummaryDto, SyncExecutionError> {
        let provider = plan.provider;
        let run_id = context.run_id();
        let SyncExecutionPlan {
            provider: _,
            remote_root,
            generate_conflict_document,
            target,
        } = plan;
        let SyncExecutionTarget::S3 {
            endpoint_url,
            region,
            bucket,
            access_key_id,
            secret_access_key,
            request_timeout_seconds,
            addressing_style,
            tls_verification,
        } = target
        else {
            return Err(unavailable_error(provider, Some(run_id)));
        };

        let runtime = self
            .runtime
            .upgrade()
            .ok_or_else(|| local_error(provider, run_id))?;
        runtime
            .verify_instance_lock()
            .map_err(|_| local_error(provider, run_id))?;
        let active = runtime
            .active_workspace_snapshot()
            .map_err(|_| local_error(provider, run_id))?;
        if active.identity() != context.snapshot_identity() {
            return Err(local_error(provider, run_id));
        }
        let authority = context.workspace_authority();
        authority
            .verify_held_directory()
            .map_err(|_| local_error(provider, run_id))?;
        workspace_root_portable_name(authority.root().canonical_path())
            .map_err(|error| dejavu_error(provider, run_id, error))?;
        let instance_authority = runtime.active_instance_authority();
        instance_authority
            .verify_held_directory()
            .map_err(|_| local_error(provider, run_id))?;
        let app_data_root = instance_authority.root().canonical_path().to_path_buf();
        let app_data_directory = instance_authority
            .root()
            .try_clone_dir()
            .map_err(|_| local_error(provider, run_id))?;
        let remote_root = ValidRemoteRoot::parse(&remote_root)
            .map_err(|_| configuration_error(provider, Some(run_id)))?;

        let binding = read_active_dejavu_binding(runtime.instance_data_root(), authority.root())
            .map_err(|error| match error {
                DejavuLocalStateError::InvalidState => configuration_error(provider, Some(run_id)),
                DejavuLocalStateError::Storage => local_error(provider, run_id),
            })?
            .ok_or_else(|| configuration_error(provider, Some(run_id)))?;
        instance_authority
            .verify_held_directory()
            .map_err(|_| local_error(provider, run_id))?;
        let (repository_id, repository_display_name, device_id, repository_key) =
            binding.into_parts();
        let catalog = KernelS3RepositoryCatalog::from_s3_parts(
            &endpoint_url,
            &region,
            &bucket,
            access_key_id.expose_secret(),
            secret_access_key.expose_secret(),
            request_timeout_seconds,
            addressing_style,
            tls_verification,
        )
        .map_err(|error| dejavu_error(provider, run_id, catalog_dejavu_error(&error)))?;
        let catalog_display_name = repository_display_name.trim();
        let catalog_display_name = if catalog_display_name.is_empty()
            || catalog_display_name.len() > 255
            || catalog_display_name.chars().any(char::is_control)
        {
            "QingYu notes"
        } else {
            catalog_display_name
        };
        let verify_catalog_authority = || {
            if context.cancellation().is_cancelled() {
                return Err(cancelled_error(provider, run_id));
            }
            authority
                .verify_held_directory()
                .map_err(|_| local_error(provider, run_id))?;
            instance_authority
                .verify_held_directory()
                .map_err(|_| local_error(provider, run_id))?;
            runtime
                .verify_instance_lock()
                .map_err(|_| local_error(provider, run_id))
        };
        verify_catalog_authority()?;
        match catalog.read(&repository_id).await {
            Ok(_) => verify_catalog_authority()?,
            Err(CloudError::NotFound) => {
                verify_catalog_authority()?;
                match catalog
                    .create(
                        &repository_id,
                        catalog_display_name,
                        time::OffsetDateTime::now_utc().unix_timestamp(),
                    )
                    .await
                {
                    Ok(_) => verify_catalog_authority()?,
                    Err(CloudError::AlreadyExists) => {
                        verify_catalog_authority()?;
                        catalog.read(&repository_id).await.map_err(|error| {
                            dejavu_error(provider, run_id, catalog_dejavu_error(&error))
                        })?;
                        verify_catalog_authority()?;
                    }
                    Err(error) => {
                        return Err(dejavu_error(provider, run_id, catalog_dejavu_error(&error)));
                    }
                }
            }
            Err(error) => {
                return Err(dejavu_error(provider, run_id, catalog_dejavu_error(&error)));
            }
        }
        let transport = S3TransportOptions {
            addressing_style,
            request_timeout_seconds,
            tls_verification,
        };
        let settings_backend = S3Backend::new_at_validated_prefix_with_transport(
            S3SyncSettings {
                access_key_id: access_key_id.expose_secret().to_owned(),
                bucket: bucket.clone(),
                endpoint_url: endpoint_url.clone(),
                region: region.clone(),
                remote_path: remote_root.app_prefix(),
                secret_access_key: secret_access_key.expose_secret().to_owned(),
            },
            transport,
        )
        .map_err(|_| configuration_error(provider, Some(run_id)))?
        .with_diagnostic_context(crate::sync::diagnostics::SyncDiagnosticContext::new(
            run_id.as_uuid().to_string(),
            "settings",
        ));
        let settings_state_relative =
            PathBuf::from("sync-state")
                .join("settings")
                .join(sync_state_key(
                    "settings",
                    &[
                        settings_backend.target_fingerprint_source().as_bytes(),
                        remote_root.as_str().as_bytes(),
                    ],
                ));
        let mut prepared = prepare_portable_settings_sync(
            &self.settings,
            &app_data_root,
            &app_data_directory,
            settings_state_relative.clone(),
            &instance_authority,
        )
        .map_err(|_| local_error(provider, run_id))?;
        if prepared.phase == PortableSettingsJournalPhase::Publication {
            let pending = resume_portable_settings_publication(
                &self.settings,
                &prepared,
                &instance_authority,
            )
            .map_err(|_| local_error(provider, run_id))?;
            finish_portable_settings_publication(
                &self.settings,
                &prepared.scope,
                pending,
                &instance_authority,
            )
            .map_err(|_| local_error(provider, run_id))?;
            prepared = prepare_portable_settings_sync(
                &self.settings,
                &app_data_root,
                &app_data_directory,
                settings_state_relative,
                &instance_authority,
            )
            .map_err(|_| local_error(provider, run_id))?;
        }
        let service_cancellation = context.cancellation().clone();
        let instance_invalidated = Arc::new(AtomicBool::new(false));
        let invalidated_for_cancellation = Arc::clone(&instance_invalidated);
        let instance_for_cancellation = Arc::clone(&instance_authority);
        let settings_cancellation = SyncExecutionCancellation::from_callback(move || {
            if service_cancellation.is_cancelled() {
                return true;
            }
            if instance_for_cancellation.verify_held_directory().is_err() {
                invalidated_for_cancellation.store(true, Ordering::Release);
                return true;
            }
            false
        });
        let mut publication: Option<DeferredSettingsPublication> = None;
        let settings_result = execute_portable_settings_sync_locked_with_cancellation(
            &prepared.scope,
            &settings_backend,
            &settings_cancellation,
            |expected_local_hash| {
                publication = Some(reconcile_portable_settings(
                    &self.settings,
                    &prepared,
                    expected_local_hash,
                    &instance_authority,
                )?);
                Ok(())
            },
        )
        .await;
        let settings_partial = combined_summary(
            None,
            settings_result
                .as_ref()
                .ok()
                .map(|outcome| &outcome.summary),
        )
        .map_err(|_| local_error(provider, run_id))?;
        if instance_invalidated.load(Ordering::Acquire)
            || instance_authority.verify_held_directory().is_err()
        {
            if let Some(publication) = publication.take() {
                publication.supersede().map_err(|_| {
                    local_error(provider, run_id).with_partial_summary(settings_partial.clone())
                })?;
            }
            return Err(local_error(provider, run_id).with_partial_summary(settings_partial));
        }
        if context.cancellation().is_cancelled() {
            if let Some(publication) = publication.take() {
                publication.supersede().map_err(|_| {
                    local_error(provider, run_id).with_partial_summary(settings_partial.clone())
                })?;
                replace_publication_with_current_settings(
                    &self.settings,
                    &prepared.scope,
                    &instance_authority,
                )
                .map_err(|_| {
                    local_error(provider, run_id).with_partial_summary(settings_partial.clone())
                })?;
            }
            return Err(cancelled_error(provider, run_id).with_partial_summary(settings_partial));
        }
        if let Some(publication) = publication.take() {
            finish_portable_settings_publication(
                &self.settings,
                &prepared.scope,
                publication,
                &instance_authority,
            )
            .map_err(|_| {
                local_error(provider, run_id).with_partial_summary(settings_partial.clone())
            })?;
        }
        let settings_summary = match settings_result {
            Ok(outcome) => outcome.summary,
            Err(error) => {
                return Err(remote_error(
                    provider,
                    run_id,
                    &error,
                    Some(settings_partial),
                ));
            }
        };
        let inputs = DejavuRunnerInputs {
            workspace: authority.clone(),
            instance_data: instance_authority.clone(),
            repository_id,
            device: Device {
                id: device_id,
                name: "QingYu".to_owned(),
                os: std::env::consts::OS.to_owned(),
            },
            repository_key,
            runtime: self.dejavu_runtime.clone(),
            coordinator: Arc::new(MutationWorkingTreeCoordinator::new(
                runtime.mutation_coordinator().clone(),
            )),
        };
        let config = DejavuS3Config {
            endpoint_url,
            region,
            bucket,
            access_key_id: DejavuSecret::new(access_key_id.expose_secret()),
            secret_access_key: DejavuSecret::new(secret_access_key.expose_secret()),
            request_timeout: Duration::from_secs(u64::from(request_timeout_seconds)),
            addressing_style: match addressing_style {
                S3AddressingStyle::Auto => DejavuAddressingStyle::Auto,
                S3AddressingStyle::Path => DejavuAddressingStyle::Path,
                S3AddressingStyle::VirtualHosted => DejavuAddressingStyle::VirtualHosted,
            },
            tls_verification: match tls_verification {
                S3TlsVerification::Verify => DejavuTlsVerification::Verify,
                S3TlsVerification::Skip => DejavuTlsVerification::Skip,
            },
        };
        let attempt = self
            .dejavu_factory
            .create(inputs, config)
            .map_err(|error| {
                dejavu_error(provider, run_id, error).with_partial_summary(settings_partial.clone())
            })?;
        let cancellation = context.cancellation().clone();
        let cancelled: Arc<dyn Fn() -> bool + Send + Sync> =
            Arc::new(move || cancellation.is_cancelled());
        let result = attempt.run(cancelled).await.map_err(|error| {
            dejavu_error(provider, run_id, error).with_partial_summary(settings_partial.clone())
        })?;
        let notes_summary = dejavu_remote_summary(&result);
        let completed_partial = combined_summary(Some(&notes_summary), Some(&settings_summary))
            .map_err(|_| local_error(provider, run_id).with_partial_summary(settings_partial))?;
        if generate_conflict_document {
            crate::sync::dejavu_runner::create_conflict_documents(
                authority.clone(),
                instance_authority,
                &result.conflicts,
                Arc::new(MutationWorkingTreeCoordinator::new(
                    runtime.mutation_coordinator().clone(),
                )),
            )
            .await
            .map_err(|error| {
                dejavu_error(provider, run_id, error)
                    .with_partial_summary(completed_partial.clone())
            })?;
        }
        Ok(completed_partial)
    }
}

#[async_trait]
trait DejavuAttempt: Send + Sync {
    async fn run(
        &self,
        cancelled: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<DejavuRunResult, DejavuRunError>;
}

#[async_trait]
impl DejavuAttempt for KernelDejavuRunner {
    async fn run(
        &self,
        cancelled: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<DejavuRunResult, DejavuRunError> {
        KernelDejavuRunner::run(self, cancelled).await
    }
}

trait DejavuRunnerFactory: Send + Sync {
    fn create(
        &self,
        inputs: DejavuRunnerInputs,
        config: DejavuS3Config,
    ) -> Result<Box<dyn DejavuAttempt>, DejavuRunError>;
}

struct ProductionDejavuRunnerFactory;

impl DejavuRunnerFactory for ProductionDejavuRunnerFactory {
    fn create(
        &self,
        inputs: DejavuRunnerInputs,
        config: DejavuS3Config,
    ) -> Result<Box<dyn DejavuAttempt>, DejavuRunError> {
        KernelDejavuRunner::new_s3(inputs, config)
            .map(|runner| Box::new(runner) as Box<dyn DejavuAttempt>)
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
            SyncExecutionTarget::S3 {
                endpoint_url,
                region,
                bucket,
                access_key_id,
                secret_access_key,
                request_timeout_seconds,
                addressing_style,
                tls_verification,
            } => {
                let remote_root = ValidRemoteRoot::parse(&plan.remote_root)
                    .map_err(|_| test_configuration_error(provider))?;
                let backend = S3Backend::new_at_validated_prefix_with_transport(
                    S3SyncSettings {
                        access_key_id: access_key_id.expose_secret().to_owned(),
                        bucket,
                        endpoint_url,
                        region,
                        remote_path: remote_root.as_str().to_owned(),
                        secret_access_key: secret_access_key.expose_secret().to_owned(),
                    },
                    S3TransportOptions {
                        addressing_style,
                        request_timeout_seconds,
                        tls_verification,
                    },
                )
                .map_err(|_| test_configuration_error(provider))?;
                backend
                    .test_connection_typed()
                    .await
                    .map(|_| ())
                    .map_err(|error| s3_connection_error(provider, &error))
            }
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
            SyncExecutionTarget::S3 { .. } => self.run_s3(plan, context).await,
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
    app_data_directory: &cap_std::fs::Dir,
    settings_state_relative: PathBuf,
    instance_authority: &ActiveInstanceAuthority,
) -> Result<PreparedPortableSettingsSync, String> {
    let scope = with_instance_authority(instance_authority, || {
        RemoteSyncScope::portable_settings_from_prepared_directory(
            app_data_root.to_path_buf(),
            app_data_directory
                .try_clone()
                .map_err(|_| settings_reconcile_error())?,
            settings_state_relative,
            "manifest.json",
        )
    })?;
    if portable_settings_pending_contains_legacy_mcp(&scope)? {
        with_instance_authority(instance_authority, || {
            clear_portable_settings_pending(&scope)
        })?;
        with_instance_authority(instance_authority, || {
            clear_portable_settings_manifest(&scope)
        })?;
    }
    let snapshot = service
        .portable_snapshot()
        .map_err(|_| settings_reconcile_error())?;
    if let Some(mut journal) = read_portable_settings_pending(&scope)? {
        if journal.phase == PortableSettingsJournalPhase::Prepared
            && capture_portable_settings_manifest_revision(&scope)?
                != journal.prepared_manifest_revision
        {
            let checkpointed = capture_scoped_settings_file_state(&scope)?;
            if checkpointed.bytes() != journal.staged_bytes()?.as_deref() {
                journal.phase = PortableSettingsJournalPhase::Reconcile;
                journal.set_staged_bytes(checkpointed.bytes());
                with_instance_authority(instance_authority, || {
                    write_portable_settings_pending(&scope, &journal)
                })?;
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
                    with_instance_authority(instance_authority, || {
                        write_portable_settings_pending(&scope, &journal)
                    })?;
                    let staged = journal.staged_bytes()?;
                    with_instance_authority(instance_authority, || {
                        replace_portable_settings_stage(&scope, staged.as_deref())
                    })?;
                    return Ok(prepared_from_journal(scope, journal));
                }
                let staged = journal.staged_bytes()?;
                with_instance_authority(instance_authority, || {
                    preserve_remote_settings_conflict(&scope, staged.as_deref())
                })?;
            }
            _ => {
                let staged = journal.staged_bytes()?;
                with_instance_authority(instance_authority, || {
                    replace_portable_settings_stage(&scope, staged.as_deref())
                })?;
                return Ok(prepared_from_journal(scope, journal));
            }
        }
    }
    let mut journal =
        PortableSettingsJournal::prepared(snapshot.revision().as_str(), snapshot.bytes());
    journal.prepared_manifest_revision = capture_portable_settings_manifest_revision(&scope)?;
    with_instance_authority(instance_authority, || {
        write_portable_settings_pending(&scope, &journal)
    })?;
    with_instance_authority(instance_authority, || {
        replace_portable_settings_stage(&scope, snapshot.bytes())
    })?;
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
    instance_authority: &ActiveInstanceAuthority,
) -> Result<DeferredSettingsPublication, String> {
    let mut journal =
        read_portable_settings_pending(&prepared.scope)?.ok_or_else(settings_reconcile_error)?;
    if journal.phase == PortableSettingsJournalPhase::Publication {
        return resume_portable_settings_publication(service, prepared, instance_authority);
    }
    let staged = capture_scoped_settings_file_state(&prepared.scope)?;
    journal.phase = PortableSettingsJournalPhase::Reconcile;
    journal.set_staged_bytes(staged.bytes());
    journal.expected_local_hash = expected_local_hash.map(str::to_string);
    with_instance_authority(instance_authority, || {
        write_portable_settings_pending(&prepared.scope, &journal)
    })?;
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
    with_instance_authority(instance_authority, || {
        write_portable_settings_pending(&prepared.scope, &journal)
    })?;
    let publication = service
        .replace_portable_deferred_with_preflight_and_verify(
            staged.bytes(),
            &expected_revision,
            || {
                instance_authority.verify_held_directory().is_ok()
                    && capture_scoped_settings_file_state(&prepared.scope)
                        .is_ok_and(|actual| actual == staged)
            },
            |actual| instance_authority.verify_held_directory().is_ok() && actual == &desired,
        )
        .map_err(|_| settings_reconcile_error())?;
    journal.phase = PortableSettingsJournalPhase::Publication;
    if let Err(error) = with_instance_authority(instance_authority, || {
        write_portable_settings_pending(&prepared.scope, &journal)
    }) {
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
    instance_authority: &ActiveInstanceAuthority,
) -> Result<DeferredSettingsPublication, String> {
    let revision = prepared
        .applied_portable_revision
        .as_deref()
        .ok_or_else(settings_reconcile_error)
        .and_then(|value| {
            Revision::parse(value.to_string()).map_err(|_| settings_reconcile_error())
        })?;
    with_instance_authority(instance_authority, || {
        service
            .resume_portable_publication(&revision, prepared.publication_events.clone())
            .map_err(|_| settings_reconcile_error())
    })
}

fn finish_portable_settings_publication(
    service: &SettingsService,
    scope: &RemoteSyncScope,
    publication: DeferredSettingsPublication,
    instance_authority: &ActiveInstanceAuthority,
) -> Result<(), String> {
    if instance_authority.verify_held_directory().is_err() {
        publication
            .supersede()
            .map_err(|_| settings_reconcile_error())?;
        return Err(settings_reconcile_error());
    }
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
        replace_publication_with_current_settings(service, scope, instance_authority)?;
        return Err(settings_reconcile_error());
    }
    if instance_authority.verify_held_directory().is_err() {
        publication
            .supersede()
            .map_err(|_| settings_reconcile_error())?;
        return Err(settings_reconcile_error());
    }
    if !service
        .publish_if_portable_revision(publication, &expected_revision)
        .map_err(|_| settings_reconcile_error())?
    {
        replace_publication_with_current_settings(service, scope, instance_authority)?;
        return Err(settings_reconcile_error());
    }
    with_instance_authority(instance_authority, || {
        clear_portable_settings_pending(scope)
    })
}

fn replace_publication_with_current_settings(
    service: &SettingsService,
    scope: &RemoteSyncScope,
    instance_authority: &ActiveInstanceAuthority,
) -> Result<(), String> {
    loop {
        instance_authority
            .verify_held_directory()
            .map_err(|_| settings_reconcile_error())?;
        let snapshot = service
            .portable_snapshot()
            .map_err(|_| settings_reconcile_error())?;
        let mut journal =
            PortableSettingsJournal::prepared(snapshot.revision().as_str(), snapshot.bytes());
        journal.prepared_manifest_revision = capture_portable_settings_manifest_revision(scope)?;
        with_instance_authority(instance_authority, || {
            write_portable_settings_pending(scope, &journal)
        })?;
        with_instance_authority(instance_authority, || {
            replace_portable_settings_stage(scope, snapshot.bytes())
        })?;
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

fn with_instance_authority<T>(
    instance_authority: &ActiveInstanceAuthority,
    mutation: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    instance_authority
        .verify_held_directory()
        .map_err(|_| settings_reconcile_error())?;
    mutation()
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

fn workspace_root_portable_name(root: &Path) -> Result<String, DejavuRunError> {
    let name = root
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(DejavuRunError::WorkspaceUnavailable)?;
    DocumentName::parse(name.to_owned())
        .map(|name| name.as_str().to_owned())
        .map_err(|_| DejavuRunError::PortableNameRequired {
            component: name.to_owned(),
        })
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
        SafeUnsignedInteger::new(notes.saturating_add(settings).min(MAX_SAFE_INTEGER))
            .map_err(|_| ())
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

fn dejavu_remote_summary(result: &DejavuRunResult) -> RemoteSyncSummary {
    let conflicts = u64::try_from(result.conflicts.len()).unwrap_or(u64::MAX);
    RemoteSyncSummary {
        bytes_downloaded: result.transfer.download_bytes,
        bytes_uploaded: result.transfer.upload_bytes,
        conflict_files: conflicts,
        downloaded_files: result.transfer.download_files,
        scanned_files: result.scanned_files,
        skipped_files: 0,
        uploaded_files: result.transfer.upload_files,
    }
}

fn dejavu_error(
    provider: SyncProvider,
    run_id: RunId,
    error: DejavuRunError,
) -> SyncExecutionError {
    match error {
        DejavuRunError::InvalidConfiguration => configuration_error(provider, Some(run_id)),
        DejavuRunError::WorkspaceUnavailable | DejavuRunError::RepositoryUnavailable => {
            local_error(provider, run_id)
        }
        DejavuRunError::PortableNameRequired { component } => {
            let relative_path = portable_name_diagnostic_component(&component);
            let safe = SyncSafeErrorDto::new(
                provider,
                SyncSafeErrorOperation::SyncRun,
                SyncSafeErrorCode::PortableNameRequired,
            )
            .with_category(SyncSafeErrorCategory::Storage)
            .with_relative_path(relative_path)
            .with_run_id(run_id);
            SyncExecutionError::new(safe)
        }
        DejavuRunError::WorkingTreeChanged => execution_error(
            provider,
            SyncSafeErrorOperation::SyncRun,
            SyncSafeErrorCode::Conflict,
            Some(SyncSafeErrorCategory::Conflict),
            Some(run_id),
        ),
        DejavuRunError::Cancelled => cancelled_error(provider, run_id),
        DejavuRunError::CloudUnavailable => unavailable_error(provider, Some(run_id)),
        DejavuRunError::DnsUnavailable => execution_error(
            provider,
            SyncSafeErrorOperation::SyncRun,
            SyncSafeErrorCode::ConnectionFailed,
            Some(SyncSafeErrorCategory::Network),
            Some(run_id),
        ),
        DejavuRunError::AuthenticationFailed => execution_error(
            provider,
            SyncSafeErrorOperation::SyncRun,
            SyncSafeErrorCode::AuthenticationFailed,
            Some(SyncSafeErrorCategory::Authentication),
            Some(run_id),
        ),
        DejavuRunError::PermissionDenied => execution_error(
            provider,
            SyncSafeErrorOperation::SyncRun,
            SyncSafeErrorCode::PermissionDenied,
            Some(SyncSafeErrorCategory::Authorization),
            Some(run_id),
        ),
        DejavuRunError::RateLimited => execution_error(
            provider,
            SyncSafeErrorOperation::SyncRun,
            SyncSafeErrorCode::RateLimited,
            Some(SyncSafeErrorCategory::Provider),
            Some(run_id),
        ),
        DejavuRunError::QuotaExceeded | DejavuRunError::ClockSkew => execution_error(
            provider,
            SyncSafeErrorOperation::SyncRun,
            SyncSafeErrorCode::RequestFailed,
            Some(SyncSafeErrorCategory::Provider),
            Some(run_id),
        ),
        DejavuRunError::IntegrityFailure => execution_error(
            provider,
            SyncSafeErrorOperation::SyncRun,
            SyncSafeErrorCode::RequestFailed,
            Some(SyncSafeErrorCategory::Transport),
            Some(run_id),
        ),
        DejavuRunError::RemoteConflict => execution_error(
            provider,
            SyncSafeErrorOperation::SyncRun,
            SyncSafeErrorCode::Conflict,
            Some(SyncSafeErrorCategory::Conflict),
            Some(run_id),
        ),
    }
}

fn catalog_dejavu_error(error: &CloudError) -> DejavuRunError {
    match error {
        CloudError::Auth => DejavuRunError::AuthenticationFailed,
        CloudError::Forbidden => DejavuRunError::PermissionDenied,
        CloudError::Dns => DejavuRunError::DnsUnavailable,
        CloudError::RateLimited => DejavuRunError::RateLimited,
        CloudError::QuotaExceeded => DejavuRunError::QuotaExceeded,
        CloudError::ClockSkew => DejavuRunError::ClockSkew,
        CloudError::Locked | CloudError::AlreadyExists => DejavuRunError::RemoteConflict,
        CloudError::UnsafeKey => DejavuRunError::InvalidConfiguration,
        CloudError::ResponseTooLarge { .. }
        | CloudError::LengthMismatch { .. }
        | CloudError::Backend { .. } => DejavuRunError::IntegrityFailure,
        CloudError::NotFound
        | CloudError::Unavailable
        | CloudError::S3Response { .. }
        | CloudError::Injected(_)
        | CloudError::Io(_)
        | CloudError::LockFailed { .. }
        | CloudError::UnlockFailed { .. } => DejavuRunError::CloudUnavailable,
    }
}

fn portable_name_diagnostic_component(component: &str) -> WorkspaceRelativePath {
    if !component.is_empty() && !component.contains(['/', '\\']) {
        if let Ok(relative_path) = WorkspaceRelativePath::parse(component.to_owned()) {
            return relative_path;
        }
    }

    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(component.len().saturating_mul(3));
    for byte in component.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b' ' | b'-') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    if encoded.is_empty() {
        encoded.push_str("%00");
    }
    if matches!(encoded.as_str(), "." | "..") {
        encoded = encoded.bytes().fold(String::new(), |mut escaped, byte| {
            escaped.push('%');
            escaped.push(char::from(HEX[usize::from(byte >> 4)]));
            escaped.push(char::from(HEX[usize::from(byte & 0x0f)]));
            escaped
        });
    }
    WorkspaceRelativePath::parse(encoded)
        .expect("percent-encoded portable-name diagnostic must be one safe relative component")
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

fn s3_connection_error(provider: SyncProvider, error: &RemoteSyncError) -> SyncExecutionError {
    let (mut code, category) = classify_remote_error(provider, error);
    if code == SyncSafeErrorCode::RemoteUnavailable {
        code = SyncSafeErrorCode::ConnectionFailed;
    }
    execution_error(
        provider,
        SyncSafeErrorOperation::TestConnection,
        code,
        category,
        None,
    )
}

fn remote_error(
    provider: SyncProvider,
    run_id: RunId,
    error: &RemoteSyncError,
    partial: Option<SyncSummaryDto>,
) -> SyncExecutionError {
    let (code, category) = classify_remote_error(provider, error);
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

fn classify_remote_error(
    provider: SyncProvider,
    error: &RemoteSyncError,
) -> (SyncSafeErrorCode, Option<SyncSafeErrorCategory>) {
    if provider == SyncProvider::S3 {
        if let Some(classification) = classify_s3_remote_error(error) {
            return classification;
        }
    }
    match error.safe_code() {
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
    }
}

fn classify_s3_remote_error(
    error: &RemoteSyncError,
) -> Option<(SyncSafeErrorCode, Option<SyncSafeErrorCategory>)> {
    let diagnostic = error.details()?;
    if let Some(code) = diagnostic.provider_error_code.as_deref() {
        let classified = match code {
            "InvalidAccessKeyId"
            | "InvalidToken"
            | "ExpiredToken"
            | "TokenRefreshRequired"
            | "SignatureDoesNotMatch"
            | "AuthorizationHeaderMalformed" => (
                SyncSafeErrorCode::AuthenticationFailed,
                SyncSafeErrorCategory::Authentication,
            ),
            "AccessDenied" | "AllAccessDisabled" => (
                SyncSafeErrorCode::PermissionDenied,
                SyncSafeErrorCategory::Authorization,
            ),
            "SlowDown"
            | "Throttling"
            | "ThrottlingException"
            | "RequestLimitExceeded"
            | "TooManyRequests" => (
                SyncSafeErrorCode::RateLimited,
                SyncSafeErrorCategory::Provider,
            ),
            "RequestTimeTooSkewed"
            | "RequestExpired"
            | "QuotaExceeded"
            | "StorageQuotaExceeded"
            | "BucketQuotaExceeded"
            | "TooManyBuckets"
            | "TooManyAccessPoints" => (
                SyncSafeErrorCode::RequestFailed,
                SyncSafeErrorCategory::Provider,
            ),
            _ => match diagnostic.http_status {
                Some(401) => (
                    SyncSafeErrorCode::AuthenticationFailed,
                    SyncSafeErrorCategory::Authentication,
                ),
                Some(403) => (
                    SyncSafeErrorCode::PermissionDenied,
                    SyncSafeErrorCategory::Authorization,
                ),
                Some(409 | 412) => (SyncSafeErrorCode::Conflict, SyncSafeErrorCategory::Conflict),
                Some(429) => (
                    SyncSafeErrorCode::RateLimited,
                    SyncSafeErrorCategory::Provider,
                ),
                Some(408 | 500 | 502 | 503 | 504) => (
                    SyncSafeErrorCode::RemoteUnavailable,
                    SyncSafeErrorCategory::Provider,
                ),
                _ => (
                    SyncSafeErrorCode::RequestFailed,
                    SyncSafeErrorCategory::Provider,
                ),
            },
        };
        return Some((classified.0, Some(classified.1)));
    }
    let classified = match diagnostic.category {
        SyncFailureCategory::Transport => (
            SyncSafeErrorCode::RemoteUnavailable,
            SyncSafeErrorCategory::Network,
        ),
        SyncFailureCategory::Integrity if diagnostic.code == "s3-object-changed" => {
            (SyncSafeErrorCode::Conflict, SyncSafeErrorCategory::Conflict)
        }
        SyncFailureCategory::Integrity => (
            SyncSafeErrorCode::RequestFailed,
            SyncSafeErrorCategory::Transport,
        ),
        SyncFailureCategory::Http => match diagnostic.http_status {
            Some(401) => (
                SyncSafeErrorCode::AuthenticationFailed,
                SyncSafeErrorCategory::Authentication,
            ),
            Some(403) => (
                SyncSafeErrorCode::PermissionDenied,
                SyncSafeErrorCategory::Authorization,
            ),
            Some(409 | 412) => (SyncSafeErrorCode::Conflict, SyncSafeErrorCategory::Conflict),
            Some(429) => (
                SyncSafeErrorCode::RateLimited,
                SyncSafeErrorCategory::Provider,
            ),
            Some(408 | 500 | 502 | 503 | 504) => (
                SyncSafeErrorCode::RemoteUnavailable,
                SyncSafeErrorCategory::Provider,
            ),
            _ => (
                SyncSafeErrorCode::RequestFailed,
                SyncSafeErrorCategory::Provider,
            ),
        },
        SyncFailureCategory::Local => (SyncSafeErrorCode::LocalIo, SyncSafeErrorCategory::Storage),
    };
    Some((classified.0, Some(classified.1)))
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
        time::{Duration, SystemTime},
    };

    use tempfile::{tempdir, TempDir};

    use crate::{
        composition::install_fixed_kernel_services,
        config::KernelConfig,
        contract::{
            BindSyncRepositoryRequest, CreateDocumentRequest, CreatedDocumentDto,
            DeleteDocumentRequest, DeletionPolicy, DocumentContents, DocumentId, DocumentName,
            ExportDejavuKeyRequest, FileDocumentName, HostProfile, ImportDejavuKeyRequest,
            ListRemoteNotebooksQuery, MoveDocumentRequest, PatchSyncConfigRequest, Revision, RunId,
            SyncCompletionState, SyncConfigChangesDto, SyncProvider, SyncRunCompletionState,
            SyncTrigger, TriggerSyncRunRequest, UpdateDocumentRequest, WorkspaceGeneration,
            WorkspaceRelativePath,
        },
        events::EventSink,
        paths::KernelPaths,
        ports::system::system_kernel_ports,
        runtime::{DocumentsApiService, KernelRuntime, SyncApiService},
        services::{
            sync::{KernelSyncTriggerDisposition, SyncExecutor as _, SyncRunContext, SyncService},
            workspace::WorkspaceService,
        },
        settings::{
            service::SettingsService,
            storage::{AtomicJsonSettingsStore, SettingsStore as _},
        },
        storage::DurableFileStore,
        sync::{
            backend::{
                RemoteSyncDiagnostic, RemoteSyncError, SyncFailureCategory, SyncProviderOperation,
            },
            config::{SyncConfig, SyncConfigStore},
            local_state::initialize_server_dejavu_binding,
        },
        workspace::{
            managed::ManagedWorkspaceCollection,
            primary::{FixedPrimaryWorkspaceStore, PrimaryWorkspaceState},
        },
    };

    use super::{DejavuAttempt, DejavuRunnerFactory, ProductionSyncExecutor};

    const SHARED_EXECUTOR_REPOSITORY_ID: &str = "323df833-764a-44b3-a534-492640c258f2";
    const SHARED_EXECUTOR_DISPLAY_NAME: &str = "Shared executor notes";

    #[test]
    fn combined_summary_saturates_successful_counts_at_the_wire_limit() {
        let notes = super::RemoteSyncSummary {
            bytes_uploaded: u64::MAX,
            uploaded_files: u64::MAX,
            ..Default::default()
        };
        let settings = super::RemoteSyncSummary {
            bytes_uploaded: 1,
            uploaded_files: 1,
            ..Default::default()
        };

        let summary =
            super::combined_summary(Some(&notes), Some(&settings)).expect("saturated safe summary");

        assert_eq!(
            summary.bytes_uploaded.get(),
            crate::contract::MAX_SAFE_INTEGER
        );
        assert_eq!(
            summary.uploaded_files.get(),
            crate::contract::MAX_SAFE_INTEGER
        );
    }

    #[test]
    fn portable_name_error_keeps_a_directly_safe_component() {
        let run_id = crate::contract::RunId::new(uuid::Uuid::new_v4());
        let actual = super::dejavu_error(
            SyncProvider::S3,
            run_id,
            crate::sync::dejavu_runner::DejavuRunError::PortableNameRequired {
                component: "CON.md".to_owned(),
            },
        );
        let safe = crate::contract::SyncSafeErrorDto::new(
            SyncProvider::S3,
            crate::contract::SyncSafeErrorOperation::SyncRun,
            crate::contract::SyncSafeErrorCode::PortableNameRequired,
        )
        .with_category(crate::contract::SyncSafeErrorCategory::Storage)
        .with_relative_path(crate::contract::WorkspaceRelativePath::parse("CON.md").unwrap())
        .with_run_id(run_id);

        assert_eq!(
            actual,
            crate::services::sync::SyncExecutionError::new(safe.clone())
        );
        assert_eq!(safe.code(), "portable-name-required");
        assert_eq!(safe.category(), Some("storage"));
        assert_eq!(safe.operation(), "sync_run");
        assert_eq!(safe.run_id(), Some(run_id));
        let serialized = serde_json::to_string(&safe).expect("serialize safe error");
        assert!(!serialized.contains("/Users/"));
        assert!(!serialized.contains(r"C:\Users\"));
    }

    #[test]
    fn portable_name_error_percent_encodes_only_its_unsafe_component() {
        let run_id = crate::contract::RunId::new(uuid::Uuid::new_v4());
        let component = "/Users/alice/C:\\Users\\alice\\bad\u{0001}é";
        let actual = super::dejavu_error(
            SyncProvider::Webdav,
            run_id,
            crate::sync::dejavu_runner::DejavuRunError::PortableNameRequired {
                component: component.to_owned(),
            },
        );
        let relative_path = crate::contract::WorkspaceRelativePath::parse(
            "%2FUsers%2Falice%2FC%3A%5CUsers%5Calice%5Cbad%01%C3%A9",
        )
        .expect("encoded safe diagnostic component");
        let safe = crate::contract::SyncSafeErrorDto::new(
            SyncProvider::Webdav,
            crate::contract::SyncSafeErrorOperation::SyncRun,
            crate::contract::SyncSafeErrorCode::PortableNameRequired,
        )
        .with_category(crate::contract::SyncSafeErrorCategory::Storage)
        .with_relative_path(relative_path.clone())
        .with_run_id(run_id);

        assert_eq!(
            actual,
            crate::services::sync::SyncExecutionError::new(safe.clone())
        );
        assert_eq!(safe.code(), "portable-name-required");
        assert_eq!(safe.category(), Some("storage"));
        assert_eq!(safe.operation(), "sync_run");
        assert_eq!(safe.run_id(), Some(run_id));
        assert!(!relative_path.as_str().contains(['/', '\\']));
        let serialized = serde_json::to_string(&safe).expect("serialize safe error");
        assert!(!serialized.contains("/Users/"));
        assert!(!serialized.contains(r"C:\Users\"));
    }

    #[test]
    fn portable_name_diagnostic_encoding_is_total_for_nonempty_components() {
        for (component, expected) in [
            (".", "%2E"),
            ("..", "%2E%2E"),
            ("%", "%"),
            ("\0", "%00"),
            ("名", "名"),
            ("foo/bar", "foo%2Fbar"),
        ] {
            assert_eq!(
                super::portable_name_diagnostic_component(component).as_str(),
                expected,
                "wrong diagnostic encoding for {component:?}"
            );
        }
    }

    #[test]
    fn s3_settings_failures_keep_provider_network_conflict_and_authentication_types() {
        let cases = [
            (
                SyncFailureCategory::Http,
                "s3-upload-http-failed",
                Some(403),
                Some("InvalidAccessKeyId"),
                "authentication_failed",
                Some("authentication"),
            ),
            (
                SyncFailureCategory::Http,
                "s3-upload-http-failed",
                Some(403),
                Some("AccessDenied"),
                "permission_denied",
                Some("authorization"),
            ),
            (
                SyncFailureCategory::Http,
                "s3-upload-http-failed",
                Some(429),
                Some("SlowDown"),
                "rate_limited",
                Some("provider"),
            ),
            (
                SyncFailureCategory::Transport,
                "s3-list-request-failed",
                None,
                None,
                "remote_unavailable",
                Some("network"),
            ),
            (
                SyncFailureCategory::Integrity,
                "s3-object-changed",
                Some(412),
                None,
                "conflict",
                Some("conflict"),
            ),
            (
                SyncFailureCategory::Integrity,
                "s3-upload-verification-failed",
                None,
                None,
                "request_failed",
                Some("transport"),
            ),
        ];
        for (category, diagnostic_code, status, provider_code, code, safe_category) in cases {
            let error = RemoteSyncError::diagnostic(RemoteSyncDiagnostic {
                category,
                code: diagnostic_code.to_owned(),
                http_status: status,
                method: Some("PUT".to_owned()),
                object_id: Some("redacted-object".to_owned()),
                operation: SyncProviderOperation::Upload,
                provider_error_code: provider_code.map(str::to_owned),
                request_id: None,
                run_id: "test-run".to_owned(),
                scope: "settings".to_owned(),
            });

            let (actual_code, actual_category) =
                super::classify_remote_error(SyncProvider::S3, &error);

            assert_eq!(actual_code.as_str(), code);
            assert_eq!(actual_category.map(|value| value.as_str()), safe_category);
        }
    }

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
    async fn webdav_portable_name_required_precedes_connecting() {
        let server = WebDavFixture::start();
        let kernel = TestKernel::start_named(&server.endpoint(), true, "CON.md").await;
        let sync = SyncService::new(
            kernel.runtime.clone(),
            Arc::new(SyncConfigStore::new(kernel.sync_store).expect("sync config store")),
            kernel.executor.clone(),
        );
        let exposed = SyncApiService::get_sync_config(&sync)
            .await
            .expect("ready WebDAV config");

        SyncApiService::trigger_sync_run(
            &sync,
            TriggerSyncRunRequest {
                expected_config_revision: exposed.revision,
            },
        )
        .await
        .expect("accept WebDAV run");
        let failed = wait_for_settled_sync(&sync).await;

        assert_eq!(failed.completion_state, SyncCompletionState::Failed);
        let error = failed.error.as_ref().expect("portable workspace error");
        assert_eq!(error.code(), "portable-name-required");
        assert_eq!(error.category(), Some("storage"));
        assert_eq!(error.operation(), "sync_run");
        assert_eq!(
            error
                .relative_path()
                .map(crate::contract::WorkspaceRelativePath::as_str),
            Some("CON.md")
        );
        assert!(server.requests().is_empty());
    }

    #[tokio::test]
    async fn s3_portable_name_required_precedes_cloud_or_settings_mutation() {
        let server = S3Fixture::start();
        let factory = Arc::new(FixedDejavuFactory::default());
        let kernel =
            TestKernel::start_s3_named(factory.clone(), &server.endpoint(), "CON.md").await;
        let sync = SyncService::new(
            kernel.runtime.clone(),
            Arc::new(SyncConfigStore::new(kernel.sync_store).expect("sync config store")),
            kernel.executor.clone(),
        );
        let exposed = SyncApiService::get_sync_config(&sync)
            .await
            .expect("ready S3 config");

        SyncApiService::trigger_sync_run(
            &sync,
            TriggerSyncRunRequest {
                expected_config_revision: exposed.revision,
            },
        )
        .await
        .expect("accept S3 run");
        let failed = wait_for_settled_sync(&sync).await;

        assert_eq!(failed.completion_state, SyncCompletionState::Failed);
        let error = failed.error.as_ref().expect("portable workspace error");
        assert_eq!(error.code(), "portable-name-required");
        assert_eq!(error.category(), Some("storage"));
        assert_eq!(error.operation(), "sync_run");
        assert_eq!(
            error
                .relative_path()
                .map(crate::contract::WorkspaceRelativePath::as_str),
            Some("CON.md")
        );
        assert!(server.requests().is_empty());
        assert_eq!(factory.calls.load(Ordering::Acquire), 0);
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

    #[cfg(unix)]
    #[tokio::test]
    async fn portable_settings_prepare_stops_before_mutation_after_instance_lock_replacement() {
        let server = WebDavFixture::start();
        let kernel = TestKernel::start(&server.endpoint(), false).await;
        let authority = kernel.runtime.active_instance_authority();
        let app_data_root = authority.root().canonical_path().to_path_buf();
        let app_data_directory = authority
            .root()
            .try_clone_dir()
            .expect("retained app data directory");
        replace_instance_lock_address(&kernel.app_data);

        let result = super::prepare_portable_settings_sync(
            &kernel.executor.settings,
            &app_data_root,
            &app_data_directory,
            PathBuf::from("sync-state/settings/authority-prepare"),
            &authority,
        );

        assert!(result.is_err());
        assert!(!kernel
            .app_data
            .join("sync-state/settings/authority-prepare")
            .exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn portable_settings_publication_stops_after_instance_lock_replacement() {
        let server = WebDavFixture::start();
        let kernel = TestKernel::start(&server.endpoint(), false).await;
        let authority = kernel.runtime.active_instance_authority();
        let app_data_root = authority.root().canonical_path().to_path_buf();
        let app_data_directory = authority
            .root()
            .try_clone_dir()
            .expect("retained app data directory");
        let relative_state = PathBuf::from("sync-state/settings/authority-publication");
        let prepared = super::prepare_portable_settings_sync(
            &kernel.executor.settings,
            &app_data_root,
            &app_data_directory,
            relative_state.clone(),
            &authority,
        )
        .expect("prepare settings journal");
        let expected_hash =
            crate::sync::settings_scope::capture_scoped_settings_file_state(&prepared.scope)
                .expect("capture staged settings")
                .hash()
                .map(str::to_owned);
        let publication = super::reconcile_portable_settings(
            &kernel.executor.settings,
            &prepared,
            expected_hash.as_deref(),
            &authority,
        )
        .expect("defer settings publication");
        replace_instance_lock_address(&kernel.app_data);

        let result = super::finish_portable_settings_publication(
            &kernel.executor.settings,
            &prepared.scope,
            publication,
            &authority,
        );

        assert!(result.is_err());
        assert!(kernel
            .app_data
            .join(relative_state)
            .join("portable-settings-pending.json")
            .exists());
    }

    #[tokio::test]
    async fn s3_executor_preserves_android_minimal_settings_without_expanding_absent_groups() {
        let server = S3Fixture::start();
        let factory = Arc::new(FixedDejavuFactory::watching(server.state.clone()));
        let preserved = android_minimal_preserved_settings();
        let kernel = TestKernel::start_s3_with_settings(
            factory.clone(),
            &server.endpoint(),
            preserved.clone(),
        )
        .await;
        let before = std::fs::read(kernel.app_data.join("settings.json"))
            .expect("read preserved Android fixture");
        let sync = SyncService::new(
            kernel.runtime.clone(),
            Arc::new(SyncConfigStore::new(kernel.sync_store).expect("sync config store")),
            kernel.executor.clone(),
        );
        let exposed = SyncApiService::get_sync_config(&sync)
            .await
            .expect("ready S3 sync config");

        SyncApiService::trigger_sync_run(
            &sync,
            TriggerSyncRunRequest {
                expected_config_revision: exposed.revision,
            },
        )
        .await
        .expect("accept Android-style S3 run");
        let completed = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let status = SyncApiService::get_sync_status(&sync)
                    .await
                    .expect("read Android-style S3 status");
                if status.completion_state != SyncCompletionState::Attempting {
                    break status;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("Android-style S3 sync should complete");

        assert_eq!(completed.completion_state, SyncCompletionState::Succeeded);
        assert_eq!(factory.calls.load(Ordering::Acquire), 1);
        assert_eq!(
            std::fs::read(kernel.app_data.join("settings.json"))
                .expect("reread preserved Android fixture"),
            before
        );
        assert_eq!(
            server
                .file("/notes/qingyu/app/settings.json")
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok()),
            Some(serde_json::json!({
                "appearanceMode": "system",
                "darkThemeId": "dark",
                "lightThemeId": "light"
            }))
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&before).unwrap(),
            preserved
        );
    }

    #[tokio::test]
    async fn s3_executor_projects_macos_fuller_legacy_settings_before_dejavu() {
        let server = S3Fixture::start();
        let factory = Arc::new(FixedDejavuFactory::watching(server.state.clone()));
        let preserved = macos_fuller_legacy_settings();
        let kernel = TestKernel::start_s3_with_settings(
            factory.clone(),
            &server.endpoint(),
            preserved.clone(),
        )
        .await;
        let before = std::fs::read(kernel.app_data.join("settings.json"))
            .expect("read preserved macOS fixture");
        let sync = SyncService::new(
            kernel.runtime.clone(),
            Arc::new(SyncConfigStore::new(kernel.sync_store).expect("sync config store")),
            kernel.executor.clone(),
        );
        let exposed = SyncApiService::get_sync_config(&sync)
            .await
            .expect("ready S3 sync config");

        SyncApiService::trigger_sync_run(
            &sync,
            TriggerSyncRunRequest {
                expected_config_revision: exposed.revision,
            },
        )
        .await
        .expect("accept macOS-style S3 run");
        let completed = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let status = SyncApiService::get_sync_status(&sync)
                    .await
                    .expect("read macOS-style S3 status");
                if status.completion_state != SyncCompletionState::Attempting {
                    break status;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("macOS-style S3 sync should complete");

        assert_eq!(completed.completion_state, SyncCompletionState::Succeeded);
        assert_eq!(factory.calls.load(Ordering::Acquire), 1);
        assert_eq!(
            std::fs::read(kernel.app_data.join("settings.json"))
                .expect("reread preserved macOS fixture"),
            before
        );
        let remote = server
            .file("/notes/qingyu/app/settings.json")
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .expect("canonical portable settings uploaded");
        assert_eq!(
            remote["editorPreferences"]["markdownShortcuts"]["toggleViewMode"],
            "F8"
        );
        assert_eq!(
            remote["editorPreferences"]["viewModeCustomizations"]["viewModeToggle"],
            "visible"
        );
        assert!(remote.get("workspace").is_none());
        assert!(remote["editorPreferences"]
            .get("aiQuickActionPrompts")
            .is_none());
        assert!(remote["editorPreferences"]["imageUpload"]
            .get("provider")
            .is_none());
        assert!(remote["editorPreferences"]["viewModeCustomizations"]
            .get("recentFolders")
            .is_none());
        assert_eq!(
            remote["editorPreferences"]["markdownTemplates"][0],
            serde_json::json!({
                "fileName": "weekly-review.md",
                "id": "weekly-review",
                "name": "Weekly review"
            })
        );
        assert_eq!(
            remote["editorPreferences"]["titlebarActions"]
                .as_array()
                .expect("portable titlebar actions")
                .iter()
                .map(|action| action["id"].as_str().expect("titlebar action id"))
                .collect::<Vec<_>>(),
            vec!["save", "viewMode", "theme", "history", "sourceMode"]
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&before).unwrap(),
            preserved
        );
    }

    #[tokio::test]
    async fn one_s3_run_uses_the_active_legacy_binding_and_dejavu_attempt() {
        let server = S3Fixture::start();
        let factory = Arc::new(FixedDejavuFactory::watching(server.state.clone()));
        let kernel = TestKernel::start_s3(factory.clone(), &server.endpoint()).await;
        let sync = SyncService::new(
            kernel.runtime.clone(),
            Arc::new(SyncConfigStore::new(kernel.sync_store).expect("sync config store")),
            kernel.executor.clone(),
        );
        let exposed = SyncApiService::get_sync_config(&sync)
            .await
            .expect("ready S3 sync config");

        SyncApiService::trigger_sync_run(
            &sync,
            TriggerSyncRunRequest {
                expected_config_revision: exposed.revision,
            },
        )
        .await
        .expect("accept S3 sync run");

        let completed = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let status = SyncApiService::get_sync_status(&sync)
                    .await
                    .expect("read S3 sync status");
                if status.completion_state != SyncCompletionState::Attempting {
                    break status;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("S3 sync should complete");

        assert_eq!(completed.completion_state, SyncCompletionState::Succeeded);
        let summary = completed.summary.into_option().expect("DejaVu summary");
        assert_eq!(summary.bytes_downloaded.get(), 11);
        assert_eq!(summary.bytes_uploaded.get(), 30);
        assert_eq!(summary.downloaded_files.get(), 2);
        assert_eq!(summary.uploaded_files.get(), 4);
        assert_eq!(summary.scanned_files.get(), 5);
        assert_eq!(summary.conflict_files.get(), 1);
        let remote_settings = server
            .file("/notes/qingyu/app/settings.json")
            .expect("portable settings should be uploaded before DejaVu runs");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&remote_settings)
                .expect("portable settings JSON"),
            serde_json::json!({ "language": "en" })
        );
        assert!(has_manifest_below(
            &kernel.app_data.join("sync-state/settings")
        ));
        assert_eq!(factory.calls.load(Ordering::Acquire), 1);
        assert!(factory.settings_seen_before_dejavu.load(Ordering::Acquire));
        assert_eq!(
            factory
                .repository_id
                .lock()
                .expect("recorded repository")
                .as_deref(),
            Some("323df833-764a-44b3-a534-492640c258f2")
        );
    }

    #[tokio::test]
    async fn fresh_server_s3_run_reaches_dejavu_without_a_preexisting_binding() {
        let server = S3Fixture::start();
        let factory = Arc::new(FixedDejavuFactory::watching(server.state.clone()));
        let kernel = TestKernel::start_fresh_server_s3(factory.clone(), &server.endpoint()).await;
        assert_eq!(
            kernel.runtime.host_profile(),
            crate::contract::HostProfile::Server
        );
        let config_root = kernel.runtime.config_root().canonical_path();
        let instance_data_root = kernel.runtime.instance_data_root().canonical_path();
        assert!(config_root.join("settings.json").is_file());
        assert!(config_root.join("sync-config.json").is_file());
        assert!(!instance_data_root.join("settings.json").exists());
        assert!(!instance_data_root.join("sync-config.json").exists());
        assert!(instance_data_root.join("local-sync.json").is_file());
        let sync = SyncService::new(
            kernel.runtime.clone(),
            Arc::new(SyncConfigStore::new(kernel.sync_store).expect("sync config store")),
            kernel.executor.clone(),
        );
        let exposed = SyncApiService::get_sync_config(&sync)
            .await
            .expect("ready fresh Server S3 config");

        SyncApiService::trigger_sync_run(
            &sync,
            TriggerSyncRunRequest {
                expected_config_revision: exposed.revision,
            },
        )
        .await
        .expect("accept fresh Server S3 run");

        let completed = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let status = SyncApiService::get_sync_status(&sync)
                    .await
                    .expect("read fresh Server S3 status");
                if status.completion_state != SyncCompletionState::Attempting {
                    break status;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("fresh Server S3 sync should complete");

        assert_eq!(completed.completion_state, SyncCompletionState::Succeeded);
        assert_eq!(factory.calls.load(Ordering::Acquire), 1);
        let repository_id = factory
            .repository_id
            .lock()
            .expect("recorded repository")
            .clone()
            .expect("fresh repository identity");
        let repository = uuid::Uuid::parse_str(&repository_id).expect("repository UUID");
        assert_eq!(repository.get_version(), Some(uuid::Version::Random));
        assert_eq!(repository.to_string(), repository_id);
    }

    #[tokio::test]
    async fn s3_dejavu_failure_retains_the_completed_settings_summary_and_error_class() {
        let server = S3Fixture::start();
        let kernel = TestKernel::start_s3(
            Arc::new(FailingDejavuFactory(
                crate::sync::dejavu_runner::DejavuRunError::AuthenticationFailed,
            )),
            &server.endpoint(),
        )
        .await;
        let sync = SyncService::new(
            kernel.runtime.clone(),
            Arc::new(SyncConfigStore::new(kernel.sync_store).expect("sync config store")),
            kernel.executor.clone(),
        );
        let exposed = SyncApiService::get_sync_config(&sync)
            .await
            .expect("ready S3 sync config");
        SyncApiService::trigger_sync_run(
            &sync,
            TriggerSyncRunRequest {
                expected_config_revision: exposed.revision,
            },
        )
        .await
        .expect("accept S3 sync run");

        let failed = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let status = SyncApiService::get_sync_status(&sync)
                    .await
                    .expect("read S3 sync status");
                if status.completion_state != SyncCompletionState::Attempting {
                    break status;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("S3 sync should settle");

        assert_eq!(failed.completion_state, SyncCompletionState::Failed);
        let error = failed.error.as_ref().expect("typed DejaVu error");
        assert_eq!(error.code(), "authentication_failed");
        assert_eq!(error.category(), Some("authentication"));
        let summary = failed.summary.as_ref().expect("settings partial summary");
        assert_eq!(summary.bytes_uploaded.get(), 17);
        assert_eq!(summary.uploaded_files.get(), 1);
        assert!(server.file("/notes/qingyu/app/settings.json").is_some());
    }

    #[tokio::test]
    async fn s3_dejavu_factory_failure_retains_the_completed_settings_summary() {
        let server = S3Fixture::start();
        let kernel = TestKernel::start_s3(
            Arc::new(RejectingDejavuFactory(
                crate::sync::dejavu_runner::DejavuRunError::InvalidConfiguration,
            )),
            &server.endpoint(),
        )
        .await;
        let sync = SyncService::new(
            kernel.runtime.clone(),
            Arc::new(SyncConfigStore::new(kernel.sync_store).expect("sync config store")),
            kernel.executor.clone(),
        );
        let exposed = SyncApiService::get_sync_config(&sync)
            .await
            .expect("ready S3 sync config");
        SyncApiService::trigger_sync_run(
            &sync,
            TriggerSyncRunRequest {
                expected_config_revision: exposed.revision,
            },
        )
        .await
        .expect("accept S3 sync run");

        let failed = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let status = SyncApiService::get_sync_status(&sync)
                    .await
                    .expect("read S3 sync status");
                if status.completion_state != SyncCompletionState::Attempting {
                    break status;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("S3 sync should settle");

        assert_eq!(failed.completion_state, SyncCompletionState::Failed);
        assert_eq!(
            failed.error.as_ref().expect("typed factory error").code(),
            "configuration_invalid"
        );
        let summary = failed.summary.as_ref().expect("settings partial summary");
        assert_eq!(summary.bytes_uploaded.get(), 17);
        assert_eq!(summary.uploaded_files.get(), 1);
        assert!(server.file("/notes/qingyu/app/settings.json").is_some());
    }

    #[tokio::test]
    async fn s3_connection_test_is_read_only_and_does_not_expose_credentials() {
        let server = S3Fixture::start();
        let kernel = TestKernel::start(&server.endpoint(), false).await;

        kernel
            .executor
            .test_connection(s3_config(&server.endpoint()))
            .await
            .expect("read-only S3 catalog probe");

        let requests = server.requests();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].starts_with("GET /notes?"));
        assert!(requests[0].contains("list-type=2"));
        assert!(requests[0].contains("max-keys=1"));
        assert!(!requests[0].contains("executor-access-secret"));
        assert!(!requests[0].contains("executor-signing-secret"));
        assert!(server.state.lock().expect("S3 state").files.is_empty());
    }

    #[tokio::test]
    async fn cancelled_s3_execution_writes_neither_catalog_nor_repository_objects() {
        let server = S3Fixture::start();
        let kernel =
            TestKernel::start_s3(Arc::new(FixedDejavuFactory::default()), &server.endpoint()).await;
        let run_id = RunId::new(uuid::Uuid::new_v4());
        let context = SyncRunContext::cancelled_for_test(
            run_id,
            kernel
                .runtime
                .active_workspace_snapshot()
                .expect("active S3 workspace"),
        );

        let error = kernel
            .executor
            .run(s3_config(&server.endpoint()), context)
            .await
            .expect_err("pre-cancelled execution must stop before remote catalog I/O");

        assert_eq!(error, super::cancelled_error(SyncProvider::S3, run_id));
        assert!(server.requests().is_empty());
        assert!(server.state.lock().expect("S3 state").files.is_empty());
    }

    #[tokio::test]
    async fn repository_with_wrong_key_reports_authentication_without_rewriting_local_state() {
        let server = S3Fixture::start();
        let source = CompleteExecutorFixture::start(
            HostProfile::Server,
            &server.endpoint(),
            CompleteBinding::Shared,
        )
        .await;
        source
            .create_file("", "Shared.md", "# Shared repository\n")
            .await;
        let uploaded = source.run_manual_sync().await;
        assert_eq!(uploaded.completion_state, SyncRunCompletionState::Succeeded);

        let target = CompleteExecutorFixture::start(
            HostProfile::Desktop,
            &server.endpoint(),
            CompleteBinding::SharedWrongKey,
        )
        .await;
        let local_state_path = target
            .runtime
            .instance_data_root()
            .canonical_path()
            .join("local-sync.json");
        let local_state_before = std::fs::read(&local_state_path).expect("read local sync state");

        let failed = target.run_manual_sync().await;

        assert_eq!(failed.completion_state, SyncRunCompletionState::Failed);
        let error = failed.error.as_ref().expect("safe sync error");
        assert_eq!(error.code(), "authentication_failed");
        assert_eq!(error.category(), Some("authentication"));
        assert_eq!(error.operation(), "sync_run");
        assert_eq!(
            std::fs::read(&local_state_path).expect("reread local sync state"),
            local_state_before,
        );
        let local_state: serde_json::Value =
            serde_json::from_slice(&local_state_before).expect("parse local sync state");
        assert_eq!(
            local_state["bindings"][0]["repositoryId"],
            SHARED_EXECUTOR_REPOSITORY_ID,
        );
    }

    #[tokio::test]
    async fn complete_s3_executor_converges_server_mobile_and_desktop_document_workspaces() {
        let server = S3Fixture::start();
        let source = CompleteExecutorFixture::start(
            HostProfile::Server,
            &server.endpoint(),
            CompleteBinding::Shared,
        )
        .await;
        assert_eq!(source.runtime.host_profile(), HostProfile::Server);

        source.create_directory("", "Endurance-A").await;
        let root = source
            .create_file("", "Web-Seed-A.md", "# Web Seed A\n")
            .await;
        let nested = source
            .create_file(
                "Endurance-A",
                "Nested-Web-B.md",
                "| A | B |\n|---|---|\n| 1 | 2 |\n",
            )
            .await;
        let empty = source.create_file("Endurance-A", "Untitled.md", "").await;

        let first_mtime = SystemTime::UNIX_EPOCH + Duration::from_millis(1_900_000_000_100);
        set_fixture_mtime(
            &source.workspace.join("Endurance-A/Nested-Web-B.md"),
            first_mtime,
        );
        let nested = source
            .documents
            .get_document(nested.id.clone())
            .await
            .expect("refresh nested revision after deterministic mtime");

        let uploaded = source.run_manual_sync().await;
        assert_eq!(uploaded.completion_state, SyncRunCompletionState::Succeeded);
        let uploaded_summary = uploaded.summary.into_option().expect("upload summary");
        assert!(uploaded_summary.uploaded_files.get() > 0);
        assert_eq!(
            uploaded_summary.scanned_files.get(),
            5,
            "three Markdown notes, protected syncignore, and portable settings are scanned"
        );

        let exported = source
            .sync
            .export_dejavu_key(ExportDejavuKeyRequest { confirmed: true })
            .await
            .expect("explicitly export the source repository key");
        assert!(!format!("{exported:?}").contains(&exported.key));

        let mobile = CompleteExecutorFixture::start(
            HostProfile::Mobile,
            &server.endpoint(),
            CompleteBinding::Distinct,
        )
        .await;
        assert_eq!(mobile.runtime.host_profile(), HostProfile::Mobile);
        let revision = mobile.set_enabled(false).await;
        let catalog = mobile
            .sync
            .list_remote_notebooks(ListRemoteNotebooksQuery {
                expected_revision: revision.clone(),
            })
            .await
            .expect("list the server-created remote notebook");
        let shared = catalog
            .entries
            .iter()
            .find(|entry| entry.repository_id == SHARED_EXECUTOR_REPOSITORY_ID)
            .expect("shared notebook catalog entry");
        assert_eq!(shared.display_name, SHARED_EXECUTOR_DISPLAY_NAME);
        mobile
            .sync
            .import_dejavu_key(ImportDejavuKeyRequest {
                key: exported.key.clone(),
            })
            .await
            .expect("import the shared repository key");
        let bound = mobile
            .sync
            .bind_sync_repository(BindSyncRepositoryRequest {
                expected_revision: revision,
                display_name: shared.display_name.clone(),
                repository_id: shared.repository_id.clone(),
            })
            .await
            .expect("bind managed mobile workspace to shared notebook");
        let initial_mobile = mobile.wait_for_job(&bound.job_id).await;
        assert_eq!(
            initial_mobile.completion_state,
            SyncRunCompletionState::Succeeded,
            "mobile restore failed: {:?}; requests: {:?}",
            initial_mobile.error.as_ref().map(|error| (
                error.code(),
                error.category(),
                error.operation()
            )),
            server.requests(),
        );
        assert_eq!(
            std::fs::read_to_string(mobile.workspace.join("Web-Seed-A.md"))
                .expect("mobile root note"),
            "# Web Seed A\n"
        );
        assert_eq!(
            std::fs::read_to_string(mobile.workspace.join("Endurance-A/Nested-Web-B.md"))
                .expect("mobile nested note"),
            "| A | B |\n|---|---|\n| 1 | 2 |\n"
        );
        assert!(mobile.workspace.join("Endurance-A/Untitled.md").is_file());
        let disabled = mobile
            .sync
            .get_sync_config()
            .await
            .expect("disabled S3 config remains readable");
        assert!(
            !disabled.enabled,
            "one-shot restore must not enable scheduled sync"
        );

        source
            .documents
            .update_document(
                nested.id.clone(),
                UpdateDocumentRequest {
                    workspace_generation: source.generation.clone(),
                    expected_revision: nested.revision,
                    contents: DocumentContents::parse("# Temporary Web Note\n")
                        .expect("updated nested contents"),
                },
            )
            .await
            .expect("update nested note through documents-v1");
        let second_mtime = SystemTime::UNIX_EPOCH + Duration::from_millis(1_900_000_000_900);
        set_fixture_mtime(
            &source.workspace.join("Endurance-A/Nested-Web-B.md"),
            second_mtime,
        );
        source
            .documents
            .move_document(
                root.id,
                MoveDocumentRequest {
                    workspace_generation: source.generation.clone(),
                    expected_revision: root.revision,
                    target_parent: WorkspaceRelativePath::parse("").expect("workspace root"),
                    name: DocumentName::parse("Web-Seed-Renamed.md").expect("renamed document"),
                },
            )
            .await
            .expect("rename root note through documents-v1");
        source
            .documents
            .delete_document(
                empty.id,
                DeleteDocumentRequest {
                    workspace_generation: source.generation.clone(),
                    expected_revision: empty.revision,
                    deletion_policy: DeletionPolicy::Permanent,
                },
            )
            .await
            .expect("delete empty note through documents-v1");

        let updated = source.run_manual_sync().await;
        assert_eq!(updated.completion_state, SyncRunCompletionState::Succeeded);
        assert!(
            updated
                .summary
                .into_option()
                .expect("update summary")
                .uploaded_files
                .get()
                > 0
        );
        assert!(server
            .file(&format!(
                "/notes/qingyu/repositories/{SHARED_EXECUTOR_REPOSITORY_ID}/metadata.json"
            ))
            .is_some());
        assert!(server
            .state
            .lock()
            .expect("S3 object state")
            .files
            .keys()
            .any(|path| path.starts_with(&format!(
                "/notes/qingyu/repositories/{SHARED_EXECUTOR_REPOSITORY_ID}/repo/"
            ))));

        mobile.set_enabled(true).await;
        let converged_mobile = mobile.run_manual_sync().await;
        assert_eq!(
            converged_mobile.completion_state,
            SyncRunCompletionState::Succeeded
        );
        assert_eq!(
            std::fs::read_to_string(mobile.workspace.join("Endurance-A/Nested-Web-B.md"))
                .expect("updated mobile nested note"),
            "# Temporary Web Note\n"
        );
        assert!(mobile.workspace.join("Web-Seed-Renamed.md").is_file());
        assert!(!mobile.workspace.join("Web-Seed-A.md").exists());
        assert!(!mobile.workspace.join("Endurance-A/Untitled.md").exists());

        let desktop = CompleteExecutorFixture::start(
            HostProfile::Desktop,
            &server.endpoint(),
            CompleteBinding::Shared,
        )
        .await;
        assert_eq!(desktop.runtime.host_profile(), HostProfile::Desktop);
        let converged_desktop = desktop.run_manual_sync().await;
        assert_eq!(
            converged_desktop.completion_state,
            SyncRunCompletionState::Succeeded,
            "desktop convergence failed: {:?}",
            converged_desktop.error.as_ref().map(|error| (
                error.code(),
                error.category(),
                error.operation()
            )),
        );
        assert_eq!(
            std::fs::read_to_string(desktop.workspace.join("Endurance-A/Nested-Web-B.md"))
                .expect("desktop nested note"),
            "# Temporary Web Note\n"
        );
        assert!(desktop.workspace.join("Web-Seed-Renamed.md").is_file());
        assert!(!desktop.workspace.join("Web-Seed-A.md").exists());
        assert!(!desktop.workspace.join("Endurance-A/Untitled.md").exists());
    }

    #[tokio::test]
    async fn server_repository_restore_keeps_the_binding_for_a_saved_note_and_later_sync() {
        let server = S3Fixture::start();
        let source = CompleteExecutorFixture::start(
            HostProfile::Desktop,
            &server.endpoint(),
            CompleteBinding::Shared,
        )
        .await;
        source.create_file("", "Mac-Seed.md", "# MAC-SEED\n").await;
        let uploaded = source.run_manual_sync().await;
        assert_eq!(uploaded.completion_state, SyncRunCompletionState::Succeeded);
        let exported = source
            .sync
            .export_dejavu_key(ExportDejavuKeyRequest { confirmed: true })
            .await
            .expect("export source repository key");
        source
            .sync
            .import_dejavu_key(ImportDejavuKeyRequest {
                key: exported.key.clone(),
            })
            .await
            .expect("replay the already active Desktop repository key");
        assert_eq!(
            source
                .sync
                .get_sync_repository_binding()
                .await
                .expect("read Desktop binding after an identical key replay")
                .repository_id
                .as_ref()
                .map(String::as_str),
            Some(SHARED_EXECUTOR_REPOSITORY_ID),
        );
        assert_eq!(
            source.run_manual_sync().await.completion_state,
            SyncRunCompletionState::Succeeded,
        );

        let target = CompleteExecutorFixture::start(
            HostProfile::Server,
            &server.endpoint(),
            CompleteBinding::Distinct,
        )
        .await;
        target
            .sync
            .import_dejavu_key(ImportDejavuKeyRequest {
                key: exported.key.clone(),
            })
            .await
            .expect("import the shared repository key");
        let revision = target.sync_revision().await;
        let accepted = target
            .sync
            .bind_sync_repository(BindSyncRepositoryRequest {
                expected_revision: revision,
                display_name: SHARED_EXECUTOR_DISPLAY_NAME.to_owned(),
                repository_id: SHARED_EXECUTOR_REPOSITORY_ID.to_owned(),
            })
            .await
            .expect("accept exact Server repository recovery");
        let restored = target.wait_for_job(&accepted.job_id).await;
        assert_eq!(
            restored.completion_state,
            SyncRunCompletionState::Succeeded,
            "Server restore failed: {:?}; requests: {:?}",
            restored.error.as_ref().map(|error| (
                error.code(),
                error.category(),
                error.operation()
            )),
            server.requests(),
        );
        assert_eq!(
            std::fs::read_to_string(target.workspace.join("Mac-Seed.md"))
                .expect("restored source note"),
            "# MAC-SEED\n",
        );
        let after_restore = target
            .sync
            .get_sync_repository_binding()
            .await
            .expect("read authoritative binding after restore");
        assert_eq!(
            after_restore.repository_id.as_ref().map(String::as_str),
            Some(SHARED_EXECUTOR_REPOSITORY_ID),
        );

        target
            .sync
            .import_dejavu_key(ImportDejavuKeyRequest {
                key: exported.key.clone(),
            })
            .await
            .expect("replay the already accepted repository key");
        let after_key_replay = target
            .sync
            .get_sync_repository_binding()
            .await
            .expect("read authoritative binding after an identical key replay");

        target.create_file("", "Web-Gate.md", "# WEB-GATE\n").await;
        let after_write = target
            .sync
            .get_sync_repository_binding()
            .await
            .expect("read authoritative binding after document write");

        let propagated = target.run_manual_sync().await;
        let after_sync = target
            .sync
            .get_sync_repository_binding()
            .await
            .expect("read authoritative binding after later sync");
        assert_eq!(
            (
                after_key_replay.repository_id.as_ref().map(String::as_str),
                after_write.repository_id.as_ref().map(String::as_str),
                propagated.completion_state,
                propagated.error.as_ref().map(|error| (
                    error.code(),
                    error.category(),
                    error.operation()
                )),
                after_sync.repository_id.as_ref().map(String::as_str),
            ),
            (
                Some(SHARED_EXECUTOR_REPOSITORY_ID),
                Some(SHARED_EXECUTOR_REPOSITORY_ID),
                SyncRunCompletionState::Succeeded,
                None,
                Some(SHARED_EXECUTOR_REPOSITORY_ID),
            ),
            "an idempotent key replay must preserve the restored binding and later sync; requests: {:?}",
            server.requests(),
        );

        let (interval_disposition, interval_settlement) = target
            .sync
            .trigger_kernel_sync(SyncTrigger::Interval)
            .await
            .into_parts();
        let interval_run_id = match interval_disposition {
            KernelSyncTriggerDisposition::Accepted(accepted) => accepted.run_id,
            KernelSyncTriggerDisposition::Rejected(rejection) => {
                panic!("automatic interval run was rejected: {rejection:?}")
            }
        };
        interval_settlement.wait().await;
        let interval = target
            .sync
            .get_sync_run(interval_run_id)
            .await
            .expect("read settled automatic interval run");
        let interval_status = target
            .sync
            .get_sync_status()
            .await
            .expect("read automatic interval status");
        let after_interval = target
            .sync
            .get_sync_repository_binding()
            .await
            .expect("read authoritative binding after automatic interval sync");
        assert_eq!(interval.completion_state, SyncRunCompletionState::Succeeded);
        assert_eq!(
            interval_status.last_trigger.as_ref(),
            Some(&SyncTrigger::Interval),
        );
        assert_eq!(
            after_interval.repository_id.as_ref().map(String::as_str),
            Some(SHARED_EXECUTOR_REPOSITORY_ID),
        );

        let native = CompleteExecutorFixture::start(
            HostProfile::Mobile,
            &server.endpoint(),
            CompleteBinding::Shared,
        )
        .await;
        let converged = native.run_manual_sync().await;
        assert_eq!(
            converged.completion_state,
            SyncRunCompletionState::Succeeded
        );
        assert_eq!(
            std::fs::read_to_string(native.workspace.join("Web-Gate.md"))
                .expect("native target received the Web note"),
            "# WEB-GATE\n",
        );
        native
            .sync
            .import_dejavu_key(ImportDejavuKeyRequest {
                key: exported.key.clone(),
            })
            .await
            .expect("replay the already active Mobile repository key");
        assert_eq!(
            native
                .sync
                .get_sync_repository_binding()
                .await
                .expect("read Mobile binding after an identical key replay")
                .repository_id
                .as_ref()
                .map(String::as_str),
            Some(SHARED_EXECUTOR_REPOSITORY_ID),
        );
        native
            .create_file("", "Android-Gate.md", "# ANDROID-GATE\n")
            .await;
        let android_upload = native.run_manual_sync().await;
        assert_eq!(
            android_upload.completion_state,
            SyncRunCompletionState::Succeeded,
            "Mobile sync after an identical key replay failed: {:?}; requests: {:?}",
            android_upload.error.as_ref().map(|error| (
                error.code(),
                error.category(),
                error.operation(),
            )),
            server.requests(),
        );
        assert!(
            android_upload
                .summary
                .as_ref()
                .is_some_and(|summary| summary.uploaded_files.get() > 0),
            "Mobile sync must upload the Android note after the binding-preserving key replay",
        );
    }

    struct CompleteExecutorFixture {
        _temporary: TempDir,
        documents: Arc<dyn DocumentsApiService>,
        generation: WorkspaceGeneration,
        runtime: Arc<KernelRuntime>,
        sync: Arc<SyncService>,
        workspace: PathBuf,
    }

    #[derive(Clone, Copy)]
    enum CompleteBinding {
        Shared,
        SharedWrongKey,
        Distinct,
    }

    struct CreatedFileFixture {
        id: DocumentId,
        revision: Revision,
    }

    impl CompleteExecutorFixture {
        async fn start(profile: HostProfile, endpoint: &str, binding: CompleteBinding) -> Self {
            let temporary = tempdir().expect("complete executor roots");
            let cache = temporary.path().join("cache");
            std::fs::create_dir(&cache).expect("complete executor cache");
            let paths = match profile {
                HostProfile::Server => {
                    let data = temporary.path().join("data");
                    std::fs::create_dir(&data).expect("server data root");
                    crate::paths::ServerPathLayout::for_test(&data, &cache)
                        .activate()
                        .expect("server Kernel paths")
                }
                HostProfile::Mobile => {
                    let app_data = temporary.path().join("mobile-data");
                    std::fs::create_dir(&app_data).expect("mobile app data");
                    KernelPaths::mobile(&app_data, &cache, "primary")
                        .expect("managed mobile Kernel paths")
                }
                HostProfile::Desktop => {
                    let workspace = temporary.path().join("desktop-workspace");
                    let app_data = temporary.path().join("desktop-data");
                    std::fs::create_dir(&workspace).expect("desktop workspace");
                    std::fs::create_dir(&app_data).expect("desktop app data");
                    KernelPaths::desktop(&workspace, &app_data, &cache)
                        .expect("desktop Kernel paths")
                }
            };
            let workspace = paths.workspace_root().canonical_path().to_path_buf();
            let config_root = paths.config_root().canonical_path().to_path_buf();
            let instance_root = paths.instance_data_root().canonical_path().to_path_buf();
            std::fs::write(
                config_root.join("sync-config.json"),
                serde_json::to_vec_pretty(&s3_config(endpoint)).expect("serialize S3 config"),
            )
            .expect("write S3 config");
            std::fs::write(
                config_root.join("settings.json"),
                br#"{"appConfigVersion":1,"language":"en"}"#,
            )
            .expect("write portable settings");

            let (repository_id, repository_key, display_name) = match binding {
                CompleteBinding::Shared => (
                    SHARED_EXECUTOR_REPOSITORY_ID,
                    [7_u8; 32],
                    SHARED_EXECUTOR_DISPLAY_NAME,
                ),
                CompleteBinding::SharedWrongKey => (
                    SHARED_EXECUTOR_REPOSITORY_ID,
                    [9_u8; 32],
                    SHARED_EXECUTOR_DISPLAY_NAME,
                ),
                CompleteBinding::Distinct => (
                    "fb62f56a-1ed1-49ec-b6b9-b525c5994880",
                    [9_u8; 32],
                    "Different local notebook",
                ),
            };
            let device_id = match profile {
                HostProfile::Server => "9ba9e553-0d21-4222-9a19-2ea87d89fa47",
                HostProfile::Mobile => "8c3d853f-9407-47b2-91c6-d79fe57b55cf",
                HostProfile::Desktop => "618da7e6-da96-4db7-b081-d53442365e17",
            };
            std::fs::write(
                instance_root.join("local-sync.json"),
                serde_json::to_vec_pretty(&serde_json::json!({
                    "version": 1,
                    "deviceId": device_id,
                    "repoKey": base64::Engine::encode(
                        &base64::engine::general_purpose::STANDARD,
                        repository_key,
                    ),
                    "bindings": [{
                        "repositoryId": repository_id,
                        "displayName": display_name,
                        "notesRoot": workspace,
                        "enabled": true
                    }]
                }))
                .expect("serialize complete local sync state"),
            )
            .expect("write complete local sync state");

            let managed =
                ManagedWorkspaceCollection::from_paths(&paths).expect("managed workspace set");
            let runtime = KernelRuntime::activate(
                KernelConfig::generate().expect("Kernel config"),
                paths,
                system_kernel_ports(),
            )
            .expect("complete Kernel runtime");
            let primary = Arc::new(
                FixedPrimaryWorkspaceStore::new(
                    PrimaryWorkspaceState::new("Executor fixture")
                        .expect("primary workspace state"),
                )
                .expect("fixed primary workspace"),
            );
            let services =
                install_fixed_kernel_services(&runtime, primary, managed, "Executor fixture")
                    .await
                    .expect("complete fixed Kernel composition");
            let active = runtime
                .active_workspace_snapshot()
                .expect("active complete workspace");
            let generation = active.workspace().generation.clone();
            let documents = runtime
                .documents_api_service()
                .expect("documents-v1 service")
                .clone();
            Self {
                _temporary: temporary,
                documents,
                generation,
                runtime,
                sync: services.sync,
                workspace,
            }
        }

        async fn create_directory(&self, parent: &str, name: &str) {
            let created = self
                .documents
                .create_document(CreateDocumentRequest::Directory {
                    workspace_generation: self.generation.clone(),
                    parent: WorkspaceRelativePath::parse(parent).expect("directory parent"),
                    name: DocumentName::parse(name).expect("directory name"),
                })
                .await
                .expect("create directory through documents-v1");
            assert!(matches!(created, CreatedDocumentDto::Directory { .. }));
        }

        async fn create_file(
            &self,
            parent: &str,
            name: &str,
            contents: &str,
        ) -> CreatedFileFixture {
            let created = self
                .documents
                .create_document(CreateDocumentRequest::File {
                    workspace_generation: self.generation.clone(),
                    parent: WorkspaceRelativePath::parse(parent).expect("file parent"),
                    name: FileDocumentName::parse(name).expect("Markdown file name"),
                    contents: DocumentContents::parse(contents).expect("document contents"),
                })
                .await
                .expect("create file through documents-v1");
            match created {
                CreatedDocumentDto::File { id, revision, .. } => {
                    CreatedFileFixture { id, revision }
                }
                CreatedDocumentDto::Directory { .. } => panic!("expected created file"),
            }
        }

        async fn sync_revision(&self) -> Revision {
            self.sync
                .get_sync_config()
                .await
                .expect("S3 sync config")
                .revision
        }

        async fn set_enabled(&self, enabled: bool) -> Revision {
            self.sync
                .patch_sync_config(PatchSyncConfigRequest {
                    expected_revision: self.sync_revision().await,
                    changes: SyncConfigChangesDto {
                        enabled: Some(enabled),
                        ..SyncConfigChangesDto::default()
                    },
                })
                .await
                .expect("toggle complete executor sync config")
                .revision
        }

        async fn run_manual_sync(&self) -> crate::contract::SyncRunStatusDto {
            let accepted = self
                .sync
                .trigger_sync_run(TriggerSyncRunRequest {
                    expected_config_revision: self.sync_revision().await,
                })
                .await
                .expect("accept manual full executor sync");
            wait_for_complete_sync(&self.sync, accepted.run_id).await
        }

        async fn wait_for_job(&self, job_id: &str) -> crate::contract::SyncRunStatusDto {
            let run_id = RunId::new(uuid::Uuid::parse_str(job_id).expect("binding run UUID"));
            wait_for_complete_sync(&self.sync, run_id).await
        }
    }

    async fn wait_for_complete_sync(
        sync: &SyncService,
        run_id: RunId,
    ) -> crate::contract::SyncRunStatusDto {
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let status = sync
                    .get_sync_run(run_id)
                    .await
                    .expect("read full executor run");
                if status.completion_state != SyncRunCompletionState::Attempting {
                    break status;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("full executor sync should settle")
    }

    fn set_fixture_mtime(path: &Path, modified: SystemTime) {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .expect("open fixture note for mtime");
        file.set_times(std::fs::FileTimes::new().set_modified(modified))
            .expect("set deterministic fixture mtime");
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

    #[derive(Clone, Copy)]
    enum LocalBindingFixture {
        Absent,
        LegacyDesktop,
        FreshServer,
    }

    impl TestKernel {
        async fn start(endpoint: &str, ready_sync_config: bool) -> Self {
            Self::start_named(endpoint, ready_sync_config, "Workspace").await
        }

        async fn start_named(
            endpoint: &str,
            ready_sync_config: bool,
            workspace_name: &str,
        ) -> Self {
            let config = ready_sync_config.then(|| webdav_config(endpoint, "qingyu", "run-secret"));
            Self::start_with_config(
                config,
                None,
                LocalBindingFixture::Absent,
                None,
                workspace_name,
            )
            .await
        }

        async fn start_s3(factory: Arc<dyn DejavuRunnerFactory>, endpoint: &str) -> Self {
            Self::start_s3_named(factory, endpoint, "Workspace").await
        }

        async fn start_s3_named(
            factory: Arc<dyn DejavuRunnerFactory>,
            endpoint: &str,
            workspace_name: &str,
        ) -> Self {
            Self::start_with_config(
                Some(s3_config(endpoint)),
                Some(factory),
                LocalBindingFixture::LegacyDesktop,
                None,
                workspace_name,
            )
            .await
        }

        async fn start_s3_with_settings(
            factory: Arc<dyn DejavuRunnerFactory>,
            endpoint: &str,
            settings: serde_json::Value,
        ) -> Self {
            Self::start_with_config(
                Some(s3_config(endpoint)),
                Some(factory),
                LocalBindingFixture::LegacyDesktop,
                Some(settings),
                "Workspace",
            )
            .await
        }

        async fn start_fresh_server_s3(
            factory: Arc<dyn DejavuRunnerFactory>,
            endpoint: &str,
        ) -> Self {
            Self::start_with_config(
                Some(s3_config(endpoint)),
                Some(factory),
                LocalBindingFixture::FreshServer,
                None,
                "Workspace",
            )
            .await
        }

        async fn start_with_config(
            ready_sync_config: Option<SyncConfig>,
            dejavu_factory: Option<Arc<dyn DejavuRunnerFactory>>,
            local_binding: LocalBindingFixture,
            settings_fixture: Option<serde_json::Value>,
            workspace_name: &str,
        ) -> Self {
            let temporary = tempdir().expect("temporary Kernel roots");
            let cache = temporary.path().join("cache");
            let (workspace, app_data, config_root, paths) = match local_binding {
                LocalBindingFixture::FreshServer => {
                    let data = temporary.path().join("data");
                    std::fs::create_dir(&data).expect("Server data");
                    let paths = crate::paths::ServerPathLayout::for_test(&data, &cache)
                        .activate()
                        .expect("Server Kernel paths");
                    (
                        paths.workspace_root().canonical_path().to_path_buf(),
                        paths.instance_data_root().canonical_path().to_path_buf(),
                        paths.config_root().canonical_path().to_path_buf(),
                        paths,
                    )
                }
                LocalBindingFixture::Absent | LocalBindingFixture::LegacyDesktop => {
                    let workspace = temporary.path().join(workspace_name);
                    let app_data = temporary.path().join("app-data");
                    std::fs::create_dir(&workspace).expect("workspace");
                    std::fs::create_dir(&app_data).expect("app data");
                    std::fs::create_dir(&cache).expect("cache");
                    let paths =
                        KernelPaths::desktop(&workspace, &app_data, &cache).expect("Kernel paths");
                    (workspace, app_data.clone(), app_data, paths)
                }
            };
            if let Some(config) = ready_sync_config {
                std::fs::write(
                    config_root.join("sync-config.json"),
                    serde_json::to_vec_pretty(&config).expect("serialize sync config"),
                )
                .expect("write sync config");
            }
            if matches!(local_binding, LocalBindingFixture::LegacyDesktop) {
                std::fs::write(
                    app_data.join("local-sync.json"),
                    serde_json::to_vec_pretty(&serde_json::json!({
                        "version": 1,
                        "deviceId": "eb473600-dace-4d7e-bdad-7dac05933099",
                        "repoKey": base64::Engine::encode(
                            &base64::engine::general_purpose::STANDARD,
                            [7_u8; 32],
                        ),
                        "bindings": [{
                            "repositoryId": "323df833-764a-44b3-a534-492640c258f2",
                            "displayName": "Workspace",
                            "notesRoot": workspace.canonicalize().expect("canonical workspace"),
                            "enabled": true
                        }]
                    }))
                    .expect("serialize local sync state"),
                )
                .expect("write local sync state");
            }

            if let Some(settings_fixture) = settings_fixture.as_ref() {
                std::fs::write(
                    config_root.join("settings.json"),
                    serde_json::to_vec_pretty(settings_fixture)
                        .expect("serialize preserved settings fixture"),
                )
                .expect("write preserved settings fixture");
            }

            let config = KernelConfig::generate().expect("Kernel config");
            let managed =
                ManagedWorkspaceCollection::from_paths(&paths).expect("managed collection");
            let runtime = KernelRuntime::activate(config, paths, system_kernel_ports())
                .expect("Kernel runtime");
            if matches!(local_binding, LocalBindingFixture::FreshServer) {
                let instance = runtime.active_instance_authority();
                let workspace = runtime
                    .active_workspace_authority()
                    .expect("locked Server workspace");
                initialize_server_dejavu_binding(
                    instance.as_ref(),
                    workspace.as_ref(),
                    runtime.launch_epoch(),
                )
                .expect("fresh Server DejaVu binding");
            }
            let settings_store =
                DurableFileStore::at_config(runtime.config_root(), runtime.launch_epoch())
                    .expect("settings durable store");
            let sync_store =
                DurableFileStore::at_config(runtime.config_root(), runtime.launch_epoch())
                    .expect("sync durable store");
            let settings_store =
                Arc::new(AtomicJsonSettingsStore::new(settings_store).expect("settings store"));
            if settings_fixture.is_none() {
                settings_store
                    .set("language", serde_json::json!("en"))
                    .expect("set portable language");
                settings_store.save().expect("save portable language");
            }
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
            let executor = Arc::new(match dejavu_factory {
                Some(factory) => ProductionSyncExecutor::new_with_dejavu_factory(
                    runtime.clone(),
                    settings,
                    factory,
                ),
                None => ProductionSyncExecutor::new(runtime.clone(), settings),
            });

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

    async fn wait_for_settled_sync(sync: &SyncService) -> crate::contract::SyncStatusDto {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let status = SyncApiService::get_sync_status(sync)
                    .await
                    .expect("read sync status");
                if status.completion_state != SyncCompletionState::Attempting {
                    break status;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("sync should settle")
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

    fn s3_config(endpoint: &str) -> SyncConfig {
        serde_json::from_value(serde_json::json!({
            "version": 3,
            "enabled": true,
            "provider": "s3",
            "remoteRoot": "qingyu",
            "mode": "automatic",
            "intervalSeconds": 30,
            "generateConflictDocument": false,
            "webdav": { "serverUrl": "", "username": "", "password": "" },
            "s3": {
                "endpointUrl": endpoint,
                "region": "test-1",
                "bucket": "notes",
                "accessKeyId": "executor-access-secret",
                "secretAccessKey": "executor-signing-secret",
                "requestTimeoutSeconds": 60,
                "addressingStyle": "auto",
                "tlsVerification": "verify"
            }
        }))
        .expect("valid S3 config")
    }

    fn android_minimal_preserved_settings() -> serde_json::Value {
        serde_json::json!({
            "appConfigVersion": 1,
            "appearanceMode": "system",
            "darkThemeId": "dark",
            "fileTreeSortByWorkspace": {},
            "lightThemeId": "light",
            "pandocPath": null,
            "recentMarkdownFilesByWorkspace": {},
            "uiLayout": {}
        })
    }

    fn macos_fuller_legacy_settings() -> serde_json::Value {
        let mut editor = serde_json::Value::Object(crate::settings::model::default_editor());
        editor["aiQuickActionPrompts"] = serde_json::json!({ "continue": "local prompt" });
        editor["imageUpload"]["provider"] = serde_json::json!("s3");
        editor["imageUpload"]["s3"] = serde_json::json!({ "bucket": "local-only" });
        editor["markdownShortcuts"]
            .as_object_mut()
            .expect("shortcut fixture")
            .remove("toggleViewMode");
        editor["markdownShortcuts"]["openSpellcheckSuggestions"] = serde_json::json!("Mod+.");
        editor["markdownTemplates"] = serde_json::json!([{
            "id": "weekly-review",
            "name": "Weekly review",
            "suggestedName": "obsolete-name",
            "legacyLocalField": "preserve locally"
        }]);
        editor["titlebarActions"] = serde_json::json!([
            { "id": "save", "visible": true },
            { "id": "aiAgent", "visible": true },
            { "id": "viewMode", "visible": false },
            { "id": "theme", "visible": true },
            { "id": "history", "visible": true },
            { "id": "sourceMode", "visible": true }
        ]);
        editor["viewModeCustomizations"]
            .as_object_mut()
            .expect("view-mode fixture")
            .remove("viewModeToggle");
        editor["viewModeCustomizations"]["aiPanel"] = serde_json::json!("visible");
        editor["viewModeCustomizations"]["recentFolders"] = serde_json::json!("hidden");
        serde_json::json!({
            "appConfigVersion": 1,
            "appearanceMode": "system",
            "darkThemeId": "dark",
            "editorPreferences": editor,
            "fileTreeSortByWorkspace": {},
            "language": "en",
            "lightThemeId": "light",
            "pandocPath": null,
            "recentMarkdownFilesByWorkspace": {},
            "settingsSchemaVersion": 1,
            "themeCatalogVersion": 1,
            "uiLayout": {},
            "welcomeDocumentSeen": true,
            "workspace": { "kind": "legacy-local" }
        })
    }

    #[cfg(unix)]
    fn replace_instance_lock_address(app_data: &Path) {
        let lock = app_data.join("kernel.lock");
        let displaced = app_data.join("displaced-kernel.lock");
        std::fs::rename(&lock, displaced).expect("displace retained instance lock");
        std::fs::write(lock, b"replacement").expect("install replacement instance lock");
    }

    #[derive(Default)]
    struct FixedDejavuFactory {
        calls: AtomicU64,
        repository_id: Mutex<Option<String>>,
        settings_state: Option<Arc<Mutex<S3FixtureState>>>,
        settings_seen_before_dejavu: AtomicBool,
    }

    impl FixedDejavuFactory {
        fn watching(settings_state: Arc<Mutex<S3FixtureState>>) -> Self {
            Self {
                settings_state: Some(settings_state),
                ..Self::default()
            }
        }
    }

    impl DejavuRunnerFactory for FixedDejavuFactory {
        fn create(
            &self,
            inputs: crate::sync::dejavu_runner::DejavuRunnerInputs,
            _config: crate::sync::dejavu_runner::DejavuS3Config,
        ) -> Result<Box<dyn DejavuAttempt>, crate::sync::dejavu_runner::DejavuRunError> {
            self.calls.fetch_add(1, Ordering::AcqRel);
            *self.repository_id.lock().expect("record repository") = Some(inputs.repository_id);
            if self.settings_state.as_ref().is_some_and(|state| {
                state
                    .lock()
                    .expect("S3 state before DejaVu")
                    .files
                    .contains_key("/notes/qingyu/app/settings.json")
            }) {
                self.settings_seen_before_dejavu
                    .store(true, Ordering::Release);
            }
            Ok(Box::new(FixedDejavuAttempt))
        }
    }

    struct FixedDejavuAttempt;

    #[async_trait::async_trait]
    impl DejavuAttempt for FixedDejavuAttempt {
        async fn run(
            &self,
            _cancelled: Arc<dyn Fn() -> bool + Send + Sync>,
        ) -> Result<
            crate::sync::dejavu_runner::DejavuRunResult,
            crate::sync::dejavu_runner::DejavuRunError,
        > {
            Ok(crate::sync::dejavu_runner::DejavuRunResult {
                data_changed: true,
                scanned_files: 4,
                transfer: crate::sync::dejavu_runner::DejavuTransferSummary {
                    download_bytes: 11,
                    download_chunks: 1,
                    download_files: 2,
                    upload_bytes: 13,
                    upload_chunks: 1,
                    upload_files: 3,
                },
                conflicts: vec![crate::sync::dejavu_runner::DejavuConflict {
                    conflict_id: "4e8d1180-bd21-4d5b-bcbf-f977032a02e3".to_owned(),
                    repository_id: "323df833-764a-44b3-a534-492640c258f2".to_owned(),
                    relative_path: "note.md".to_owned(),
                    occurred_at: "2026-07-30T00:00:00Z".to_owned(),
                    resolution: crate::sync::dejavu_runner::DejavuConflictResolution::KeepLocal,
                }],
            })
        }
    }

    struct FailingDejavuFactory(crate::sync::dejavu_runner::DejavuRunError);

    impl DejavuRunnerFactory for FailingDejavuFactory {
        fn create(
            &self,
            _inputs: crate::sync::dejavu_runner::DejavuRunnerInputs,
            _config: crate::sync::dejavu_runner::DejavuS3Config,
        ) -> Result<Box<dyn DejavuAttempt>, crate::sync::dejavu_runner::DejavuRunError> {
            Ok(Box::new(FailingDejavuAttempt(self.0.clone())))
        }
    }

    struct RejectingDejavuFactory(crate::sync::dejavu_runner::DejavuRunError);

    impl DejavuRunnerFactory for RejectingDejavuFactory {
        fn create(
            &self,
            _inputs: crate::sync::dejavu_runner::DejavuRunnerInputs,
            _config: crate::sync::dejavu_runner::DejavuS3Config,
        ) -> Result<Box<dyn DejavuAttempt>, crate::sync::dejavu_runner::DejavuRunError> {
            Err(self.0.clone())
        }
    }

    struct FailingDejavuAttempt(crate::sync::dejavu_runner::DejavuRunError);

    #[async_trait::async_trait]
    impl DejavuAttempt for FailingDejavuAttempt {
        async fn run(
            &self,
            _cancelled: Arc<dyn Fn() -> bool + Send + Sync>,
        ) -> Result<
            crate::sync::dejavu_runner::DejavuRunResult,
            crate::sync::dejavu_runner::DejavuRunError,
        > {
            Err(self.0.clone())
        }
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

    struct S3Fixture {
        address: std::net::SocketAddr,
        requests: Arc<Mutex<Vec<String>>>,
        state: Arc<Mutex<S3FixtureState>>,
        stop: Arc<AtomicBool>,
        thread: Option<thread::JoinHandle<()>>,
    }

    #[derive(Default)]
    struct S3FixtureState {
        files: BTreeMap<String, StoredFixtureFile>,
    }

    impl S3Fixture {
        fn start() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("S3 fixture bind");
            listener
                .set_nonblocking(true)
                .expect("nonblocking S3 fixture");
            let address = listener.local_addr().expect("S3 fixture address");
            let requests = Arc::new(Mutex::new(Vec::new()));
            let state = Arc::new(Mutex::new(S3FixtureState::default()));
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
                                .expect("blocking accepted S3 connection");
                            handle_s3_request(stream, &recorded, &shared_state);
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
            format!("http://{}", self.address)
        }

        fn requests(&self) -> Vec<String> {
            self.requests.lock().expect("S3 request log").clone()
        }

        fn file(&self, path: &str) -> Option<Vec<u8>> {
            self.state
                .lock()
                .expect("S3 state")
                .files
                .get(path)
                .map(|file| file.bytes.clone())
        }
    }

    impl Drop for S3Fixture {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            let _wake = TcpStream::connect(self.address);
            if let Some(thread) = self.thread.take() {
                thread.join().expect("S3 fixture thread");
            }
        }
    }

    fn handle_s3_request(
        mut stream: TcpStream,
        requests: &Mutex<Vec<String>>,
        state: &Mutex<S3FixtureState>,
    ) {
        let request = read_request(&mut stream);
        requests
            .lock()
            .expect("S3 request log")
            .push(format!("{} {}", request.method, request.path));
        let object_path = request.path.split('?').next().unwrap_or(&request.path);
        let response = match request.method.as_str() {
            "GET" if request.path.contains("list-type=2") => s3_list_response(state, &request.path),
            "PUT" => fixture_s3_put(state, object_path, request.body, request.if_none_match),
            "HEAD" => fixture_s3_get(state, object_path, true),
            "GET" => fixture_s3_get(state, object_path, false),
            "DELETE" => {
                state.lock().expect("S3 state").files.remove(object_path);
                empty_response("204 No Content")
            }
            _ => empty_response("405 Method Not Allowed"),
        };
        stream.write_all(&response).expect("write S3 response");
    }

    fn fixture_s3_put(
        state: &Mutex<S3FixtureState>,
        path: &str,
        body: Vec<u8>,
        if_none_match: bool,
    ) -> Vec<u8> {
        static VERSION: AtomicU64 = AtomicU64::new(1);
        let mut state = state.lock().expect("S3 state");
        if if_none_match && state.files.contains_key(path) {
            return empty_response("412 Precondition Failed");
        }
        let version = VERSION.fetch_add(1, Ordering::Relaxed);
        state.files.insert(
            path.to_owned(),
            StoredFixtureFile {
                bytes: body,
                version,
            },
        );
        format!(
            "HTTP/1.1 200 OK\r\nETag: \"v{version}\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
        .into_bytes()
    }

    fn fixture_s3_get(state: &Mutex<S3FixtureState>, path: &str, head_only: bool) -> Vec<u8> {
        let state = state.lock().expect("S3 state");
        let Some(file) = state.files.get(path) else {
            return empty_response("404 Not Found");
        };
        let body = if head_only {
            &[][..]
        } else {
            file.bytes.as_slice()
        };
        let mut response = format!(
            "HTTP/1.1 200 OK\r\nETag: \"v{}\"\r\nLast-Modified: Wed, 30 Jul 2026 00:00:00 GMT\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            file.version,
            file.bytes.len()
        )
        .into_bytes();
        response.extend_from_slice(body);
        response
    }

    fn s3_list_response(state: &Mutex<S3FixtureState>, request_path: &str) -> Vec<u8> {
        let prefix = fixture_query_value(request_path, "prefix").unwrap_or_default();
        let delimiter = fixture_query_value(request_path, "delimiter");
        let state = state.lock().expect("S3 list state");
        let mut objects = Vec::new();
        let mut common_prefixes = BTreeSet::new();
        for (path, file) in &state.files {
            let Some(key) = path.strip_prefix("/notes/") else {
                continue;
            };
            if !key.starts_with(&prefix) {
                continue;
            }
            if let Some(delimiter) = delimiter.as_deref() {
                let remainder = &key[prefix.len()..];
                if let Some(index) = remainder.find(delimiter) {
                    common_prefixes.insert(format!("{prefix}{}{}", &remainder[..index], delimiter));
                    continue;
                }
            }
            objects.push((key, file));
        }
        let contents = objects
            .into_iter()
            .map(|(key, file)| format!(
                "<Contents><Key>{}</Key><LastModified>2026-07-30T00:00:00Z</LastModified><ETag>&quot;v{}&quot;</ETag><Size>{}</Size><StorageClass>STANDARD</StorageClass></Contents>",
                fixture_xml_escape(key),
                file.version,
                file.bytes.len(),
            ))
            .collect::<String>();
        let prefixes = common_prefixes
            .into_iter()
            .map(|prefix| {
                format!(
                    "<CommonPrefixes><Prefix>{}</Prefix></CommonPrefixes>",
                    fixture_xml_escape(&prefix),
                )
            })
            .collect::<String>();
        let body = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><ListBucketResult><Name>notes</Name><Prefix>{}</Prefix><KeyCount>{}</KeyCount><MaxKeys>1000</MaxKeys><IsTruncated>false</IsTruncated>{contents}{prefixes}</ListBucketResult>",
            fixture_xml_escape(&prefix),
            state.files.len(),
        );
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    fn fixture_query_value(request_path: &str, name: &str) -> Option<String> {
        let query = request_path.split_once('?')?.1;
        query.split('&').find_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            (fixture_percent_decode(key).as_deref() == Some(name))
                .then(|| fixture_percent_decode(value))
                .flatten()
        })
    }

    fn fixture_percent_decode(value: &str) -> Option<String> {
        let bytes = value.as_bytes();
        let mut decoded = Vec::with_capacity(bytes.len());
        let mut index = 0;
        while index < bytes.len() {
            match bytes[index] {
                b'%' if index + 2 < bytes.len() => {
                    let high = fixture_hex_value(bytes[index + 1])?;
                    let low = fixture_hex_value(bytes[index + 2])?;
                    decoded.push(high * 16 + low);
                    index += 3;
                }
                b'+' => {
                    decoded.push(b' ');
                    index += 1;
                }
                byte => {
                    decoded.push(byte);
                    index += 1;
                }
            }
        }
        String::from_utf8(decoded).ok()
    }

    const fn fixture_hex_value(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    }

    fn fixture_xml_escape(value: &str) -> String {
        value
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
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
        if_none_match: bool,
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
        let mut if_none_match = false;
        for line in lines {
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().expect("content length");
            } else if name.eq_ignore_ascii_case("depth") {
                depth = Some(value.trim().to_string());
            } else if name.eq_ignore_ascii_case("if-none-match") {
                if_none_match = value.trim() == "*";
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
            if_none_match,
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
