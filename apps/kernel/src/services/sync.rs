//! Sync service composition boundary.

use std::{fmt, sync::Arc};

use async_trait::async_trait;

pub use crate::runtime::SyncApiService;

use crate::{
    contract::{
        DomainEvent, ErrorCode, ErrorDetails, Nullable, PatchSyncConfigRequest, ResourceRefDto,
        RunId, SyncConfigReadiness, SyncConfigViewDto, SyncConnectionTestDto, SyncRunAcceptedDto,
        SyncStatusDto, SyncTrigger, TestSyncConnectionRequest, TriggerSyncRunRequest,
    },
    events::{EventPublication, EventSink as _},
    runtime::{KernelRuntime, ServiceFailure},
    sync::config::{
        SyncConfig, SyncConfigChangeError, SyncConfigLoad, SyncConfigStore,
        SyncConfigStoreErrorKind,
    },
    sync::editing::SyncEditingRegistry,
    sync::status::SyncStatusState,
};

#[async_trait]
pub trait SyncExecutor: Send + Sync {
    async fn test_connection(&self, config: SyncConfig) -> Result<(), SyncExecutionError>;
    async fn run(
        &self,
        config: SyncConfig,
        run_id: RunId,
        trigger: SyncTrigger,
    ) -> Result<(), SyncExecutionError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyncExecutionError;

impl fmt::Display for SyncExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("sync execution failed")
    }
}

impl std::error::Error for SyncExecutionError {}

pub struct SyncService {
    runtime: Arc<KernelRuntime>,
    store: Arc<SyncConfigStore>,
    executor: Arc<dyn SyncExecutor>,
    editing: Arc<SyncEditingRegistry>,
    status: Arc<SyncStatusState>,
}

impl SyncService {
    pub fn new(
        runtime: Arc<KernelRuntime>,
        store: Arc<SyncConfigStore>,
        executor: Arc<dyn SyncExecutor>,
    ) -> Self {
        Self {
            runtime,
            store,
            executor,
            editing: Arc::new(SyncEditingRegistry::new()),
            status: Arc::new(SyncStatusState::new()),
        }
    }

    pub fn editing_registry(&self) -> Arc<SyncEditingRegistry> {
        self.editing.clone()
    }

    fn verify_instance(&self, code: ErrorCode) -> Result<(), ServiceFailure> {
        self.runtime
            .verify_instance_lock()
            .map_err(|_| failure(code))
    }
}

#[async_trait]
impl SyncApiService for SyncService {
    async fn get_sync_config(&self) -> Result<SyncConfigViewDto, ServiceFailure> {
        self.verify_instance(ErrorCode::SyncConfigInvalid)?;
        match self
            .store
            .load()
            .map_err(|_| failure(ErrorCode::SyncConfigInvalid))?
        {
            SyncConfigLoad::Absent => Err(failure(ErrorCode::SyncConfigAbsent)),
            SyncConfigLoad::Loaded { config, revision } => config
                .to_view(revision)
                .map_err(|_| failure(ErrorCode::SyncConfigInvalid)),
            SyncConfigLoad::Corrupt { .. } | SyncConfigLoad::Unsupported { .. } => {
                Err(failure(ErrorCode::SyncConfigInvalid))
            }
        }
    }

    async fn patch_sync_config(
        &self,
        request: PatchSyncConfigRequest,
    ) -> Result<SyncConfigViewDto, ServiceFailure> {
        self.verify_instance(ErrorCode::SyncConfigInvalid)?;
        request
            .changes
            .validate()
            .map_err(|_| failure(ErrorCode::InvalidRequest))?;
        let _mutation = self.runtime.mutation_coordinator().lock().await;
        self.verify_instance(ErrorCode::SyncConfigInvalid)?;
        if self
            .status
            .is_attempting()
            .map_err(|_| failure(ErrorCode::SyncNotReady))?
        {
            return Err(failure(ErrorCode::SyncNotReady));
        }
        let (mut config, current_revision) = match self.store.load().map_err(store_failure)? {
            SyncConfigLoad::Absent => return Err(failure(ErrorCode::SyncConfigAbsent)),
            SyncConfigLoad::Loaded { config, revision } => (config, revision),
            SyncConfigLoad::Corrupt { .. } | SyncConfigLoad::Unsupported { .. } => {
                return Err(failure(ErrorCode::SyncConfigInvalid));
            }
        };
        if request.expected_revision != current_revision {
            return Err(revision_conflict(current_revision));
        }
        config
            .apply_changes(&request.changes)
            .map_err(change_failure)?;
        let (config, revision) = self
            .store
            .replace(&request.expected_revision, *config)
            .map_err(store_failure)?;
        let exposed = config
            .to_view(revision.clone())
            .map_err(|_| failure(ErrorCode::SyncConfigInvalid))?;
        let installed_status = self
            .status
            .install_config(&exposed)
            .map_err(|_| failure(ErrorCode::SyncNotReady))?;
        let publication = EventPublication {
            resource: ResourceRefDto::SyncConfig {},
            revision,
            event: DomainEvent::SyncConfigChanged {
                config: exposed.clone(),
            },
        };
        let _publication_result = self.runtime.publish(&publication);
        publish_status(
            self.runtime.as_ref(),
            installed_status,
            exposed.revision.clone(),
            Nullable::null(),
        );
        Ok(exposed)
    }

    async fn test_sync_connection(
        &self,
        request: TestSyncConnectionRequest,
    ) -> Result<SyncConnectionTestDto, ServiceFailure> {
        self.verify_instance(ErrorCode::SyncConfigInvalid)?;
        let (config, revision) = {
            let _mutation = self.runtime.mutation_coordinator().lock().await;
            self.verify_instance(ErrorCode::SyncConfigInvalid)?;
            let (mut config, revision) = match self.store.load().map_err(store_failure)? {
                SyncConfigLoad::Absent => return Err(failure(ErrorCode::SyncConfigAbsent)),
                SyncConfigLoad::Loaded { config, revision } => (config, revision),
                SyncConfigLoad::Corrupt { .. } | SyncConfigLoad::Unsupported { .. } => {
                    return Err(failure(ErrorCode::SyncConfigInvalid));
                }
            };
            if request.expected_revision != revision {
                return Err(revision_conflict(revision));
            }
            config
                .apply_changes(&request.changes)
                .map_err(change_failure)?;
            let exposed = config
                .to_view(request.expected_revision.clone())
                .map_err(|_| failure(ErrorCode::SyncConfigInvalid))?;
            if exposed.readiness != SyncConfigReadiness::Ready {
                return Err(failure(ErrorCode::SyncNotReady));
            }
            (*config, request.expected_revision)
        };
        self.verify_instance(ErrorCode::SyncConfigInvalid)?;
        self.executor
            .test_connection(config.clone())
            .await
            .map_err(|_| failure(ErrorCode::SyncNotReady))?;
        Ok(SyncConnectionTestDto {
            provider: config.provider(),
            checked_target: config.checked_target(),
            config_revision: revision,
        })
    }

    async fn get_sync_status(&self) -> Result<SyncStatusDto, ServiceFailure> {
        self.verify_instance(ErrorCode::SyncNotReady)?;
        let _mutation = self.runtime.mutation_coordinator().lock().await;
        self.verify_instance(ErrorCode::SyncNotReady)?;
        let exposed = match self
            .store
            .load()
            .map_err(|_| failure(ErrorCode::SyncNotReady))?
        {
            SyncConfigLoad::Loaded { config, revision } => config
                .to_view(revision)
                .map_err(|_| failure(ErrorCode::SyncNotReady))?,
            SyncConfigLoad::Absent
            | SyncConfigLoad::Corrupt { .. }
            | SyncConfigLoad::Unsupported { .. } => {
                return Err(failure(ErrorCode::SyncNotReady));
            }
        };
        self.status
            .snapshot_for(&exposed)
            .map_err(|_| failure(ErrorCode::SyncNotReady))
    }

    async fn trigger_sync_run(
        &self,
        request: TriggerSyncRunRequest,
    ) -> Result<SyncRunAcceptedDto, ServiceFailure> {
        self.verify_instance(ErrorCode::SyncNotReady)?;
        let _mutation = self.runtime.mutation_coordinator().lock().await;
        self.verify_instance(ErrorCode::SyncNotReady)?;
        let admitted_workspace = self
            .runtime
            .active_workspace_snapshot()
            .map_err(|_| failure(ErrorCode::SyncNotReady))?;
        let (config, revision) = match self
            .store
            .load()
            .map_err(|_| failure(ErrorCode::SyncNotReady))?
        {
            SyncConfigLoad::Loaded { config, revision } => (config, revision),
            SyncConfigLoad::Absent
            | SyncConfigLoad::Corrupt { .. }
            | SyncConfigLoad::Unsupported { .. } => {
                return Err(failure(ErrorCode::SyncNotReady));
            }
        };
        if request.expected_config_revision != revision {
            return Err(revision_conflict(revision));
        }
        let exposed = config
            .to_view(request.expected_config_revision.clone())
            .map_err(|_| failure(ErrorCode::SyncNotReady))?;
        if exposed.readiness != SyncConfigReadiness::Ready {
            return Err(failure(ErrorCode::SyncNotReady));
        }
        let config = *config;
        let accepted_at = self
            .runtime
            .ports()
            .clock()
            .now()
            .map_err(|_| failure(ErrorCode::SyncRunUnavailable))?;
        let run_id = RunId::new(uuid::Uuid::new_v4());
        let attempting = self
            .status
            .begin_run(&exposed, run_id, accepted_at.clone(), SyncTrigger::Manual)
            .map_err(|_| failure(ErrorCode::SyncRunUnavailable))?;
        publish_status(
            self.runtime.as_ref(),
            attempting,
            request.expected_config_revision.clone(),
            Nullable::value(run_id),
        );

        let background_runtime = self.runtime.clone();
        let background_status = self.status.clone();
        let executor = self.executor.clone();
        let config_revision = request.expected_config_revision.clone();
        let fallback_completed_at = accepted_at.clone();
        let spawn_result = self.runtime.spawn_background(Box::pin(async move {
            let same_workspace = background_runtime.verify_instance_lock().is_ok()
                && background_runtime
                    .active_workspace_snapshot()
                    .is_ok_and(|current| Arc::ptr_eq(&current, &admitted_workspace));
            let result = if same_workspace {
                executor.run(config, run_id, SyncTrigger::Manual).await
            } else {
                Err(SyncExecutionError)
            };
            let _mutation = background_runtime.mutation_coordinator().lock().await;
            let completed_at = background_runtime
                .ports()
                .clock()
                .now()
                .unwrap_or(fallback_completed_at);
            if let Ok(completed) =
                background_status.complete_run(run_id, completed_at, result.is_ok())
            {
                publish_status(
                    background_runtime.as_ref(),
                    completed,
                    config_revision,
                    Nullable::value(run_id),
                );
            }
        }));
        if spawn_result.is_err() {
            let failed = self
                .status
                .complete_run(run_id, accepted_at.clone(), false)
                .map_err(|_| failure(ErrorCode::SyncRunUnavailable))?;
            publish_status(
                self.runtime.as_ref(),
                failed,
                request.expected_config_revision.clone(),
                Nullable::value(run_id),
            );
            return Err(failure(ErrorCode::SyncRunUnavailable));
        }
        Ok(SyncRunAcceptedDto {
            run_id,
            accepted_at,
            config_revision: request.expected_config_revision,
        })
    }
}

fn failure(code: ErrorCode) -> ServiceFailure {
    ServiceFailure::new(code, None).expect("sync service uses compatible public error details")
}

fn revision_conflict(current_revision: crate::contract::Revision) -> ServiceFailure {
    ServiceFailure::new(
        ErrorCode::SyncConfigRevisionConflict,
        Some(ErrorDetails::RevisionConflict {
            current_revision: Some(current_revision),
        }),
    )
    .expect("sync revision conflict uses compatible public details")
}

fn store_failure(error: crate::sync::config::SyncConfigStoreError) -> ServiceFailure {
    match error.kind() {
        SyncConfigStoreErrorKind::RevisionConflict => {
            failure(ErrorCode::SyncConfigRevisionConflict)
        }
        SyncConfigStoreErrorKind::InvalidDraft
        | SyncConfigStoreErrorKind::NotRecoverable
        | SyncConfigStoreErrorKind::RecoveryRequired
        | SyncConfigStoreErrorKind::Unavailable => failure(ErrorCode::SyncConfigInvalid),
    }
}

fn change_failure(error: SyncConfigChangeError) -> ServiceFailure {
    match error {
        SyncConfigChangeError::UnsafeEndpoint | SyncConfigChangeError::UnsafeRemoteRoot => {
            failure(ErrorCode::SyncConfigInvalid)
        }
        SyncConfigChangeError::CredentialStoreUnavailable => failure(ErrorCode::SyncNotReady),
    }
}

fn publish_status(
    runtime: &KernelRuntime,
    status: SyncStatusDto,
    revision: crate::contract::Revision,
    run_id: Nullable<RunId>,
) {
    let publication = EventPublication {
        resource: ResourceRefDto::SyncStatus { run_id },
        revision,
        event: DomainEvent::SyncStatusChanged { status },
    };
    let _publication_result = runtime.publish(&publication);
}

impl fmt::Debug for SyncService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SyncService(..)")
    }
}
