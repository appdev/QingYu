//! Sync service composition boundary.

use std::{
    fmt,
    future::{poll_fn, Future},
    panic::{catch_unwind, AssertUnwindSafe},
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Weak,
    },
    task::{Context, Poll},
};

use async_trait::async_trait;

pub use crate::runtime::{
    ActiveWorkspaceSnapshotIdentity as SyncWorkspaceSnapshotIdentity, SyncApiService,
    SyncCancellation,
};

use crate::{
    contract::{
        DomainEvent, ErrorCode, ErrorDetails, Nullable, PatchSyncConfigRequest, ResourceRefDto,
        RunId, SyncConfigReadiness, SyncConfigViewDto, SyncConnectionTestDto, SyncRunAcceptedDto,
        SyncStatusDto, SyncTrigger, TestSyncConnectionRequest, TriggerSyncRunRequest,
    },
    events::{EventPublication, EventSink as _},
    ports::BoxTaskFuture,
    runtime::{ClaimedSyncRun, KernelRuntime, ServiceFailure, SyncRunClaim},
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
        context: SyncRunContext,
    ) -> Result<(), SyncExecutionError>;
}

pub struct SyncRunContext {
    run_id: RunId,
    trigger: SyncTrigger,
    snapshot: Arc<crate::runtime::ActiveWorkspaceSnapshot>,
    cancellation: SyncCancellation,
}

struct SyncBackgroundTaskDropState {
    trigger_active: AtomicBool,
    dropped: AtomicBool,
}

struct SyncBackgroundTaskGuard {
    runtime: Weak<KernelRuntime>,
    run_id: RunId,
    state: Arc<SyncBackgroundTaskDropState>,
}

impl Drop for SyncBackgroundTaskGuard {
    fn drop(&mut self) {
        self.state.dropped.store(true, Ordering::Release);
        if !self.state.trigger_active.load(Ordering::Acquire) {
            if let Some(runtime) = self.runtime.upgrade() {
                runtime.abandon_sync_background_task(self.run_id);
            }
        }
    }
}

struct SyncBackgroundTaskEnvelope {
    inner: Option<BoxTaskFuture>,
    guard: Option<SyncBackgroundTaskGuard>,
}

impl Future for SyncBackgroundTaskEnvelope {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        self.inner
            .as_mut()
            .expect("sync task envelope retains its inner future")
            .as_mut()
            .poll(context)
    }
}

impl Drop for SyncBackgroundTaskEnvelope {
    fn drop(&mut self) {
        drop(self.inner.take());
        drop(self.guard.take());
    }
}

impl SyncRunContext {
    fn new(claimed: ClaimedSyncRun) -> Self {
        Self {
            run_id: claimed.run_id,
            trigger: claimed.trigger,
            snapshot: claimed.snapshot,
            cancellation: claimed.cancellation,
        }
    }

    pub const fn run_id(&self) -> RunId {
        self.run_id
    }

    pub const fn trigger(&self) -> SyncTrigger {
        self.trigger
    }

    pub fn workspace(&self) -> &crate::contract::WorkspaceDto {
        self.snapshot.workspace()
    }

    pub fn snapshot_identity(&self) -> SyncWorkspaceSnapshotIdentity {
        self.snapshot.identity()
    }

    pub const fn cancellation(&self) -> &SyncCancellation {
        &self.cancellation
    }
}

impl fmt::Debug for SyncRunContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SyncRunContext { workspace: [OPAQUE] }")
    }
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
            status: runtime.sync_status(),
            runtime,
            store,
            executor,
            editing: Arc::new(SyncEditingRegistry::new()),
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
        let mutation = self.runtime.mutation_coordinator().lock().await;
        self.verify_instance(ErrorCode::SyncConfigInvalid)?;
        if self
            .runtime
            .sync_run_registered(&mutation)
            .map_err(|_| failure(ErrorCode::SyncNotReady))?
            || self
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
        let mutation = self.runtime.mutation_coordinator().lock().await;
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
        let queued = self
            .runtime
            .queue_sync_run(
                admitted_workspace,
                &exposed,
                run_id,
                accepted_at.clone(),
                SyncTrigger::Manual,
                &mutation,
            )
            .map_err(|_| failure(ErrorCode::SyncRunUnavailable))?;
        publish_status(
            self.runtime.as_ref(),
            queued.attempting,
            request.expected_config_revision.clone(),
            Nullable::value(run_id),
        );

        let background_runtime = Arc::downgrade(&self.runtime);
        let executor = self.executor.clone();
        let fallback_completed_at = accepted_at.clone();
        let drop_state = Arc::new(SyncBackgroundTaskDropState {
            trigger_active: AtomicBool::new(true),
            dropped: AtomicBool::new(false),
        });
        let drop_guard = SyncBackgroundTaskGuard {
            runtime: Arc::downgrade(&self.runtime),
            run_id,
            state: drop_state.clone(),
        };
        let inner: BoxTaskFuture = Box::pin(async move {
            let Some(runtime) = background_runtime.upgrade() else {
                return;
            };
            let claim = {
                let mutation = runtime.mutation_coordinator().lock().await;
                runtime.claim_sync_run(run_id, &mutation)
            };
            let claimed = match claim {
                Ok(SyncRunClaim::Ready(claimed)) => claimed,
                Ok(SyncRunClaim::Rejected(Some(terminal))) => {
                    runtime.publish_sync_terminal(&terminal);
                    let _finished = runtime.finish_sync_terminal(run_id);
                    return;
                }
                Ok(SyncRunClaim::Rejected(None)) | Err(_) => return,
            };
            let mut run = executor.run(config, SyncRunContext::new(claimed));
            let result = poll_fn(|context| {
                match catch_unwind(AssertUnwindSafe(|| run.as_mut().poll(context))) {
                    Ok(Poll::Ready(result)) => Poll::Ready(Ok(result)),
                    Ok(Poll::Pending) => Poll::Pending,
                    Err(_) => Poll::Ready(Err(())),
                }
            })
            .await;
            drop(run);
            let mutation = runtime.mutation_coordinator().lock().await;
            let completed_at = runtime
                .ports()
                .clock()
                .now()
                .unwrap_or(fallback_completed_at);
            let terminal = runtime
                .finalize_running_sync_run(
                    run_id,
                    matches!(result, Ok(Ok(()))),
                    completed_at,
                    &mutation,
                )
                .ok()
                .flatten();
            drop(mutation);
            if let Some(terminal) = terminal {
                runtime.publish_sync_terminal(&terminal);
                let _finished = runtime.finish_sync_terminal(run_id);
            }
        });
        let spawn_result =
            self.runtime
                .spawn_sync_background(Box::pin(SyncBackgroundTaskEnvelope {
                    inner: Some(inner),
                    guard: Some(drop_guard),
                }));
        drop_state.trigger_active.store(false, Ordering::Release);
        if spawn_result.is_err() || drop_state.dropped.load(Ordering::Acquire) {
            let terminal = self
                .runtime
                .fail_queued_sync_spawn(run_id, &mutation)
                .map_err(|_| failure(ErrorCode::SyncRunUnavailable))?;
            drop(mutation);
            self.runtime.publish_sync_terminal(&terminal);
            self.runtime
                .finish_sync_terminal(run_id)
                .map_err(|_| failure(ErrorCode::SyncRunUnavailable))?;
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
