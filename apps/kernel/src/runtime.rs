use std::{
    fmt,
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex as StdMutex, OnceLock, RwLock, Weak,
    },
};

use async_trait::async_trait;
use tokio::sync::{Mutex, MutexGuard, Notify};
use uuid::Uuid;

use crate::{
    config::{KernelConfig, KernelLaunchEpoch},
    contract::{
        CreateDocumentRequest, CreatedDocumentDto, DeleteDocumentRequest, DocumentContentDto,
        DocumentHistoryPageDto, DocumentId, DocumentPageDto, ErrorCode, ErrorDetails, HostProfile,
        InstanceId, ListDocumentsQuery, MoveDocumentRequest, PageQuery, PatchSettingsRequest,
        PatchSyncConfigRequest, ReadyHealthResponse, RestoreDocumentHistoryRequest, Revision,
        Rfc3339Utc, SearchPageDto, SearchWorkspaceQuery, SettingsSnapshotDto, SnapshotId,
        SyncConfigViewDto, SyncConnectionTestDto, SyncRunAcceptedDto, SyncStatusDto, SyncTrigger,
        SystemVersionResponse, TestSyncConnectionRequest, TriggerSyncRunRequest,
        UpdateDocumentRequest, WireIdentityKey, WorkspaceDto,
    },
    error::{safe_error_envelope, safe_message_for_error_code},
    events::{EventBroker, EventPublication, EventSink, EventSinkError},
    paths::{KernelPaths, PathPolicyError, PathPolicyErrorKind, WorkspaceRoot},
    ports::{BoxTaskFuture, KernelPorts, PortError},
    sync::status::SyncStatusState,
    workspace::{
        lock::{InstanceLockLease, KernelLockError, KernelLockErrorKind, WorkspaceLockLease},
        primary::{PreparedWorkspaceAuthorityBinding, PrimaryWorkspaceRepositoryBinding},
    },
};

pub struct KernelRuntime {
    config: KernelConfig,
    paths: KernelPaths,
    ports: KernelPorts,
    mutation_coordinator: Arc<MutationCoordinator>,
    owner: KernelRuntimeOwner,
    sync_status: Arc<SyncStatusState>,
    workspace_run_lifecycle: WorkspaceRunLifecycle,
    event_broker: Arc<EventBroker>,
    system_api: OnceLock<Arc<dyn SystemApiService>>,
    workspace_api: OnceLock<Arc<dyn WorkspaceApiService>>,
    documents_api: OnceLock<Arc<dyn DocumentsApiService>>,
    settings_api: OnceLock<Arc<dyn SettingsApiService>>,
    sync_api: OnceLock<Arc<dyn SyncApiService>>,
    _instance_lease: Arc<InstanceLockLease>,
}

impl KernelRuntime {
    pub fn activate(
        config: KernelConfig,
        paths: KernelPaths,
        mut ports: KernelPorts,
    ) -> Result<Arc<Self>, KernelStartupError> {
        let instance_lease = Arc::new(
            InstanceLockLease::acquire(paths.instance_data_root())
                .map_err(KernelStartupError::from_lock)?,
        );
        let workspace_root = paths.workspace_root_authority();
        workspace_root
            .verify_held_directory()
            .map_err(|_| KernelStartupError::workspace_unavailable())?;
        let workspace_lease = Arc::new(
            WorkspaceLockLease::acquire(workspace_root.as_ref())
                .map_err(KernelStartupError::from_lock)?,
        );
        let active_workspace = Arc::new(ActiveWorkspaceAuthority::new(
            workspace_root,
            workspace_lease,
        ));
        ports.bind_instance_lease(instance_lease.clone());
        Ok(Arc::new(Self {
            config,
            paths,
            ports,
            mutation_coordinator: Arc::new(MutationCoordinator::new()),
            owner: KernelRuntimeOwner::new(active_workspace),
            sync_status: Arc::new(SyncStatusState::new()),
            workspace_run_lifecycle: WorkspaceRunLifecycle::new(),
            event_broker: Arc::new(EventBroker::new()),
            system_api: OnceLock::new(),
            workspace_api: OnceLock::new(),
            documents_api: OnceLock::new(),
            settings_api: OnceLock::new(),
            sync_api: OnceLock::new(),
            _instance_lease: instance_lease,
        }))
    }

    pub const fn instance_id(&self) -> InstanceId {
        self.config.instance_id()
    }

    pub const fn wire_identity_key(&self) -> &WireIdentityKey {
        self.config.wire_identity_key()
    }

    pub const fn launch_epoch(&self) -> &KernelLaunchEpoch {
        self.config.launch_epoch()
    }

    /// Deliberately exposes the per-launch bearer value only for the native
    /// host's inherited startup payload.
    pub fn expose_native_launch_credential(&self) -> &str {
        self.config.native_launch_credential().expose_secret()
    }

    pub fn matches_native_launch_credential(&self, candidate: &str) -> bool {
        self.config.native_launch_credential().matches(candidate)
    }

    pub const fn ports(&self) -> &KernelPorts {
        &self.ports
    }

    pub const fn mutation_coordinator(&self) -> &Arc<MutationCoordinator> {
        &self.mutation_coordinator
    }

    pub(crate) fn sync_status(&self) -> Arc<SyncStatusState> {
        self.sync_status.clone()
    }

    pub fn verify_instance_lock(&self) -> Result<(), KernelLockError> {
        self._instance_lease.verify_held_lock()
    }

    /// Transitional authority-only accessor for path-policy compatibility.
    /// Workspace consumers must use `active_workspace_snapshot` so recovery
    /// cannot be bypassed and metadata cannot be observed separately.
    pub fn active_workspace_authority(
        &self,
    ) -> Result<Arc<ActiveWorkspaceAuthority>, WorkspaceAuthorityError> {
        self.owner.compatibility_authority()
    }

    pub fn active_workspace_snapshot(
        &self,
    ) -> Result<Arc<ActiveWorkspaceSnapshot>, WorkspaceAuthorityError> {
        let snapshot = self.owner.active_snapshot()?;
        snapshot.authority.verify_held_directory()?;
        Ok(snapshot)
    }

    pub fn prepare_host_workspace_authority(
        &self,
        path: &Path,
    ) -> Result<PreparedWorkspaceAuthority, WorkspaceAuthorityError> {
        if self.paths.profile() != HostProfile::Desktop {
            return Err(WorkspaceAuthorityError::unsupported_profile());
        }
        let expected = self.owner.preparation_identity()?;
        let expected_authority = expected.authority();
        let root = self
            .paths
            .prepare_host_workspace_root(path)
            .map_err(WorkspaceAuthorityError::from_path)?;
        let candidate = if expected_authority.root.same_identity(root.as_ref()) {
            expected_authority
        } else {
            let lease = Arc::new(
                WorkspaceLockLease::acquire(root.as_ref())
                    .map_err(WorkspaceAuthorityError::from_lock)?,
            );
            root.verify_held_directory()
                .map_err(WorkspaceAuthorityError::from_path)?;
            Arc::new(ActiveWorkspaceAuthority::new(root, lease))
        };
        self.owner.verify_identity(&expected)?;
        Ok(PreparedWorkspaceAuthority {
            expected,
            candidate,
            binding: PreparedWorkspaceAuthorityBinding::new(),
        })
    }

    /// Attempts a direct authority installation under the shared mutation
    /// coordinator. This synchronous compatibility seam fails unavailable
    /// instead of waiting when another mutation owns the coordinator.
    pub fn commit_host_workspace_authority(
        &self,
        prepared: PreparedWorkspaceAuthority,
    ) -> Result<Arc<ActiveWorkspaceAuthority>, WorkspaceAuthorityError> {
        let mutation = self
            .mutation_coordinator
            .try_lock()
            .map_err(|_| WorkspaceAuthorityError::unavailable())?;
        self.commit_host_workspace_authority_with_mutation(prepared, &mutation)
    }

    pub(crate) fn commit_host_workspace_authority_with_mutation(
        &self,
        prepared: PreparedWorkspaceAuthority,
        mutation: &MutationPermit<'_>,
    ) -> Result<Arc<ActiveWorkspaceAuthority>, WorkspaceAuthorityError> {
        if self.paths.profile() != HostProfile::Desktop {
            return Err(WorkspaceAuthorityError::unsupported_profile());
        }
        if !self.mutation_coordinator.recognizes(mutation) {
            return Err(WorkspaceAuthorityError::unavailable());
        }
        self.owner
            .verify_authority_only_identity(&prepared.expected)?;
        prepared.candidate.verify_held_directory()?;
        self.paths
            .validate_host_workspace_root(prepared.candidate.root.as_ref())
            .map_err(WorkspaceAuthorityError::from_path)?;
        self.owner.commit_authority_only(prepared)
    }

    pub(crate) fn verify_prepared_host_workspace_authority(
        &self,
        prepared: &PreparedWorkspaceAuthority,
    ) -> Result<(), WorkspaceAuthorityError> {
        if self.paths.profile() != HostProfile::Desktop {
            return Err(WorkspaceAuthorityError::unsupported_profile());
        }
        self.owner.verify_identity(&prepared.expected)?;
        prepared.candidate.verify_held_directory()?;
        self.paths
            .validate_host_workspace_root(prepared.candidate.root.as_ref())
            .map_err(WorkspaceAuthorityError::from_path)?;
        self.owner.verify_identity(&prepared.expected)
    }

    pub(crate) fn workspace_initialization(
        &self,
        repository_binding: &PrimaryWorkspaceRepositoryBinding,
        mutation: &MutationPermit<'_>,
    ) -> Result<WorkspaceInitialization, WorkspaceInitializationError> {
        if !self.mutation_coordinator.recognizes(mutation) {
            return Err(WorkspaceInitializationError::unavailable());
        }
        self.owner.initialization(repository_binding)
    }

    pub(crate) fn initialize_workspace_snapshot(
        &self,
        initialization: &WorkspaceInitialization,
        workspace: WorkspaceDto,
        repository_binding: PrimaryWorkspaceRepositoryBinding,
        mutation: &MutationPermit<'_>,
    ) -> Result<Arc<ActiveWorkspaceSnapshot>, WorkspaceInitializationError> {
        if !self.mutation_coordinator.recognizes(mutation) {
            return Err(WorkspaceInitializationError::unavailable());
        }
        self.owner.initialize_snapshot(
            initialization,
            workspace,
            repository_binding,
            &self.workspace_run_lifecycle,
        )
    }

    pub(crate) fn verify_workspace_initialization(
        &self,
        initialization: &WorkspaceInitialization,
    ) -> Result<(), WorkspaceAuthorityError> {
        self.owner.verify_identity(&initialization.expected)?;
        initialization.expected.authority().verify_held_directory()
    }

    pub(crate) fn enter_workspace_initialization_recovery(
        &self,
        initialization: &WorkspaceInitialization,
        mutation: &MutationPermit<'_>,
    ) -> Result<(), WorkspaceAuthorityError> {
        if !self.mutation_coordinator.recognizes(mutation) {
            return Err(WorkspaceAuthorityError::unavailable());
        }
        if !matches!(
            &initialization.expected,
            WorkspaceOwnerIdentity::AuthorityOnly(_)
        ) {
            return Err(WorkspaceAuthorityError::unavailable());
        }
        self.owner.enter_recovery(
            initialization.expected.clone(),
            Some(initialization.expected.authority()),
        )
    }

    pub const fn event_broker(&self) -> &Arc<EventBroker> {
        &self.event_broker
    }

    pub fn spawn_background(&self, task: BoxTaskFuture) -> Result<(), PortError> {
        let workspace = self
            .active_workspace_snapshot()
            .map_err(|_| PortError::unavailable())?;
        self.ports.spawn_background(Box::pin(async move {
            task.await;
            drop(workspace);
        }))
    }

    pub(crate) fn spawn_sync_background(&self, task: BoxTaskFuture) -> Result<(), PortError> {
        self.ports.spawn_background(task)
    }

    pub(crate) fn abandon_sync_background_task(&self, run_id: crate::contract::RunId) {
        let Ok(mutation) = self.mutation_coordinator.try_lock() else {
            let _recovery = self.quarantine_dropped_sync_task(run_id);
            self.workspace_run_lifecycle.drained.notify_waiters();
            return;
        };
        let registered = self
            .workspace_run_lifecycle
            .state
            .lock()
            .ok()
            .and_then(|lifecycle| match &lifecycle.run {
                RegisteredSyncRun::Queued(run) if run.run_id == run_id => Some(0_u8),
                RegisteredSyncRun::Running(run) if run.run_id == run_id => Some(1_u8),
                RegisteredSyncRun::Finalizing(run) if run.run_id == run_id => Some(2_u8),
                _ => None,
            });
        match registered {
            Some(0) => {
                let terminal = self.fail_queued_sync_spawn(run_id, &mutation).ok();
                drop(mutation);
                if let Some(terminal) = terminal {
                    self.publish_sync_terminal(&terminal);
                    let _finished = self.finish_sync_terminal(run_id);
                }
            }
            Some(1) => {
                let _recovery = self.quarantine_dropped_sync_task(run_id);
                drop(mutation);
                self.workspace_run_lifecycle.drained.notify_waiters();
            }
            Some(2) => {
                drop(mutation);
                let _finished = self.finish_sync_terminal(run_id);
            }
            _ => {}
        }
    }

    fn quarantine_dropped_sync_task(
        &self,
        run_id: crate::contract::RunId,
    ) -> Result<(), WorkspaceRunLifecycleError> {
        let mut owner = self
            .owner
            .workspace
            .write()
            .map_err(|_| WorkspaceRunLifecycleError)?;
        let mut lifecycle = self
            .workspace_run_lifecycle
            .state
            .lock()
            .map_err(|_| WorkspaceRunLifecycleError)?;
        let dropped = match &lifecycle.run {
            RegisteredSyncRun::Queued(run)
            | RegisteredSyncRun::Running(run)
            | RegisteredSyncRun::Finalizing(run)
                if run.run_id == run_id =>
            {
                run.clone()
            }
            _ => return Ok(()),
        };
        dropped.cancellation.cancel();
        let retained_candidate = match &lifecycle.admission {
            SyncRunAdmission::Transitioning {
                task_drop_candidate: Some(candidate),
                ..
            } => candidate.clone(),
            _ => dropped.snapshot.authority.clone(),
        };
        let current = match &*owner {
            WorkspaceOwnerState::Active(current) => current,
            WorkspaceOwnerState::RecoveryRequired { .. } => {
                lifecycle.admission = SyncRunAdmission::RecoveryClosed;
                return Ok(());
            }
            WorkspaceOwnerState::AuthorityOnly(_) => {
                lifecycle.admission = SyncRunAdmission::RecoveryClosed;
                return Err(WorkspaceRunLifecycleError);
            }
        };
        if !Arc::ptr_eq(current, &dropped.snapshot) {
            lifecycle.admission = SyncRunAdmission::RecoveryClosed;
            return Err(WorkspaceRunLifecycleError);
        }
        *owner = WorkspaceOwnerState::RecoveryRequired {
            _hold: WorkspaceRecoveryHold {
                _last_known: Some(dropped.snapshot.clone()),
                _retained_candidate: Some(retained_candidate),
            },
        };
        lifecycle.admission = SyncRunAdmission::RecoveryClosed;
        Ok(())
    }

    pub(crate) fn queue_sync_run(
        &self,
        snapshot: Arc<ActiveWorkspaceSnapshot>,
        config: &SyncConfigViewDto,
        run_id: crate::contract::RunId,
        accepted_at: Rfc3339Utc,
        trigger: SyncTrigger,
        mutation: &MutationPermit<'_>,
    ) -> Result<QueuedSyncRun, WorkspaceRunLifecycleError> {
        if !self.mutation_coordinator.recognizes(mutation) {
            return Err(WorkspaceRunLifecycleError);
        }
        let current = self
            .owner
            .active_snapshot()
            .map_err(|_| WorkspaceRunLifecycleError)?;
        if !Arc::ptr_eq(&current, &snapshot) {
            return Err(WorkspaceRunLifecycleError);
        }
        let mut lifecycle = self
            .workspace_run_lifecycle
            .state
            .lock()
            .map_err(|_| WorkspaceRunLifecycleError)?;
        if !matches!(lifecycle.admission, SyncRunAdmission::Open)
            || !matches!(lifecycle.run, RegisteredSyncRun::Empty)
        {
            return Err(WorkspaceRunLifecycleError);
        }
        lifecycle.run = RegisteredSyncRun::Queued(SyncRunRegistration {
            run_id,
            trigger,
            config_revision: config.revision.clone(),
            fallback_completed_at: accepted_at.clone(),
            snapshot,
            cancellation: SyncCancellation::new(),
        });
        let attempting = match self
            .sync_status
            .begin_run(config, run_id, accepted_at, trigger)
        {
            Ok(status) => status,
            Err(_) => {
                lifecycle.admission = SyncRunAdmission::RecoveryClosed;
                lifecycle.run = RegisteredSyncRun::Empty;
                return Err(WorkspaceRunLifecycleError);
            }
        };
        Ok(QueuedSyncRun { attempting })
    }

    pub(crate) fn sync_run_registered(
        &self,
        mutation: &MutationPermit<'_>,
    ) -> Result<bool, WorkspaceRunLifecycleError> {
        if !self.mutation_coordinator.recognizes(mutation) {
            return Err(WorkspaceRunLifecycleError);
        }
        self.workspace_run_lifecycle
            .state
            .lock()
            .map(|state| !matches!(state.run, RegisteredSyncRun::Empty))
            .map_err(|_| WorkspaceRunLifecycleError)
    }

    pub(crate) fn claim_sync_run(
        &self,
        run_id: crate::contract::RunId,
        mutation: &MutationPermit<'_>,
    ) -> Result<SyncRunClaim, WorkspaceRunLifecycleError> {
        if !self.mutation_coordinator.recognizes(mutation) {
            return Err(WorkspaceRunLifecycleError);
        }
        let current = match self.owner.active_snapshot() {
            Ok(current) => current,
            Err(_) => {
                let mut lifecycle = self
                    .workspace_run_lifecycle
                    .state
                    .lock()
                    .map_err(|_| WorkspaceRunLifecycleError)?;
                lifecycle.admission = SyncRunAdmission::RecoveryClosed;
                return Err(WorkspaceRunLifecycleError);
            }
        };
        let mut lifecycle = self
            .workspace_run_lifecycle
            .state
            .lock()
            .map_err(|_| WorkspaceRunLifecycleError)?;
        let queued = match std::mem::replace(&mut lifecycle.run, RegisteredSyncRun::Empty) {
            RegisteredSyncRun::Queued(queued) if queued.run_id == run_id => queued,
            other => {
                lifecycle.run = other;
                return Ok(SyncRunClaim::Rejected(None));
            }
        };
        let admissible = matches!(lifecycle.admission, SyncRunAdmission::Open)
            && Arc::ptr_eq(&current, &queued.snapshot);
        if admissible {
            let claimed = ClaimedSyncRun {
                run_id: queued.run_id,
                trigger: queued.trigger,
                snapshot: queued.snapshot.clone(),
                cancellation: queued.cancellation.clone(),
            };
            lifecycle.run = RegisteredSyncRun::Running(queued);
            return Ok(SyncRunClaim::Ready(claimed));
        }
        let status = self
            .sync_status
            .complete_run(run_id, queued.fallback_completed_at.clone(), false)
            .map_err(|_| {
                lifecycle.admission = SyncRunAdmission::RecoveryClosed;
                lifecycle.run = RegisteredSyncRun::Queued(queued.clone());
                WorkspaceRunLifecycleError
            })?;
        let terminal = SyncTerminalPublication {
            run_id,
            revision: queued.config_revision.clone(),
            status,
        };
        lifecycle.run = RegisteredSyncRun::Finalizing(queued);
        Ok(SyncRunClaim::Rejected(Some(Box::new(terminal))))
    }

    pub(crate) fn finalize_running_sync_run(
        &self,
        run_id: crate::contract::RunId,
        succeeded: bool,
        completed_at: Rfc3339Utc,
        mutation: &MutationPermit<'_>,
    ) -> Result<Option<SyncTerminalPublication>, WorkspaceRunLifecycleError> {
        if !self.mutation_coordinator.recognizes(mutation) {
            return Err(WorkspaceRunLifecycleError);
        }
        let current = match self.owner.active_snapshot() {
            Ok(current) => current,
            Err(_) => {
                let mut lifecycle = self
                    .workspace_run_lifecycle
                    .state
                    .lock()
                    .map_err(|_| WorkspaceRunLifecycleError)?;
                lifecycle.admission = SyncRunAdmission::RecoveryClosed;
                return Err(WorkspaceRunLifecycleError);
            }
        };
        let mut lifecycle = self
            .workspace_run_lifecycle
            .state
            .lock()
            .map_err(|_| WorkspaceRunLifecycleError)?;
        let running = match std::mem::replace(&mut lifecycle.run, RegisteredSyncRun::Empty) {
            RegisteredSyncRun::Running(running) if running.run_id == run_id => running,
            other => {
                lifecycle.run = other;
                return Ok(None);
            }
        };
        let transition_cancelled = matches!(
            &lifecycle.admission,
            SyncRunAdmission::Transitioning { expected, .. }
                if Arc::ptr_eq(expected, &running.snapshot)
        );
        let naturally_admissible = matches!(lifecycle.admission, SyncRunAdmission::Open)
            && Arc::ptr_eq(&current, &running.snapshot);
        let status = if transition_cancelled || running.cancellation.is_cancelled() {
            self.sync_status.complete_cancelled(run_id)
        } else {
            self.sync_status
                .complete_run(run_id, completed_at, succeeded && naturally_admissible)
        }
        .map_err(|_| {
            lifecycle.admission = SyncRunAdmission::RecoveryClosed;
            lifecycle.run = RegisteredSyncRun::Running(running.clone());
            WorkspaceRunLifecycleError
        })?;
        let terminal = SyncTerminalPublication {
            run_id,
            revision: running.config_revision.clone(),
            status,
        };
        lifecycle.run = RegisteredSyncRun::Finalizing(running);
        Ok(Some(terminal))
    }

    pub(crate) fn fail_queued_sync_spawn(
        &self,
        run_id: crate::contract::RunId,
        mutation: &MutationPermit<'_>,
    ) -> Result<SyncTerminalPublication, WorkspaceRunLifecycleError> {
        if !self.mutation_coordinator.recognizes(mutation) {
            return Err(WorkspaceRunLifecycleError);
        }
        let mut lifecycle = self
            .workspace_run_lifecycle
            .state
            .lock()
            .map_err(|_| WorkspaceRunLifecycleError)?;
        let queued = match std::mem::replace(&mut lifecycle.run, RegisteredSyncRun::Empty) {
            RegisteredSyncRun::Queued(queued) if queued.run_id == run_id => queued,
            other => {
                lifecycle.run = other;
                return Err(WorkspaceRunLifecycleError);
            }
        };
        let status = self
            .sync_status
            .complete_run(run_id, queued.fallback_completed_at.clone(), false)
            .map_err(|_| {
                lifecycle.admission = SyncRunAdmission::RecoveryClosed;
                lifecycle.run = RegisteredSyncRun::Queued(queued.clone());
                WorkspaceRunLifecycleError
            })?;
        let terminal = SyncTerminalPublication {
            run_id,
            revision: queued.config_revision.clone(),
            status,
        };
        lifecycle.run = RegisteredSyncRun::Finalizing(queued);
        Ok(terminal)
    }

    pub(crate) fn publish_sync_terminal(&self, terminal: &SyncTerminalPublication) {
        let publication = EventPublication {
            resource: crate::contract::ResourceRefDto::SyncStatus {
                run_id: crate::contract::Nullable::value(terminal.run_id),
            },
            revision: terminal.revision.clone(),
            event: crate::contract::DomainEvent::SyncStatusChanged {
                status: terminal.status.clone(),
            },
        };
        let _publication_result = self.publish(&publication);
    }

    pub(crate) fn finish_sync_terminal(
        &self,
        run_id: crate::contract::RunId,
    ) -> Result<(), WorkspaceRunLifecycleError> {
        let mut owner = self
            .owner
            .workspace
            .write()
            .map_err(|_| WorkspaceRunLifecycleError)?;
        let mut lifecycle = self
            .workspace_run_lifecycle
            .state
            .lock()
            .map_err(|_| WorkspaceRunLifecycleError)?;
        let finalizing = match std::mem::replace(&mut lifecycle.run, RegisteredSyncRun::Empty) {
            RegisteredSyncRun::Finalizing(finalizing) if finalizing.run_id == run_id => finalizing,
            other => {
                lifecycle.run = other;
                return Err(WorkspaceRunLifecycleError);
            }
        };
        match &lifecycle.admission {
            SyncRunAdmission::Transitioning {
                expected,
                recovery_on_drain: Some(retained_candidate),
                ..
            } => {
                let owner_matches = matches!(
                    &*owner,
                    WorkspaceOwnerState::Active(current) if Arc::ptr_eq(current, expected)
                );
                if !owner_matches {
                    lifecycle.admission = SyncRunAdmission::RecoveryClosed;
                    lifecycle.run = RegisteredSyncRun::Finalizing(finalizing.clone());
                    return Err(WorkspaceRunLifecycleError);
                }
                *owner = WorkspaceOwnerState::RecoveryRequired {
                    _hold: WorkspaceRecoveryHold {
                        _last_known: Some(expected.clone()),
                        _retained_candidate: Some(retained_candidate.clone()),
                    },
                };
                lifecycle.admission = SyncRunAdmission::RecoveryClosed;
            }
            SyncRunAdmission::Transitioning {
                expected,
                abandoned: true,
                ..
            } => {
                if matches!(
                    &*owner,
                    WorkspaceOwnerState::Active(current) if Arc::ptr_eq(current, expected)
                ) {
                    lifecycle.admission = SyncRunAdmission::Open;
                } else {
                    lifecycle.admission = SyncRunAdmission::RecoveryClosed;
                    lifecycle.run = RegisteredSyncRun::Finalizing(finalizing.clone());
                    return Err(WorkspaceRunLifecycleError);
                }
            }
            _ => {}
        }
        drop(finalizing);
        drop(owner);
        drop(lifecycle);
        self.workspace_run_lifecycle.drained.notify_waiters();
        Ok(())
    }

    pub(crate) fn begin_sync_workspace_transition(
        self: &Arc<Self>,
        expected: Arc<ActiveWorkspaceSnapshot>,
        mutation: &MutationPermit<'_>,
    ) -> Result<
        (SyncWorkspaceTransition, Option<SyncTerminalPublication>),
        WorkspaceRunLifecycleError,
    > {
        if !self.mutation_coordinator.recognizes(mutation) {
            return Err(WorkspaceRunLifecycleError);
        }
        let current = self
            .owner
            .active_snapshot()
            .map_err(|_| WorkspaceRunLifecycleError)?;
        if !Arc::ptr_eq(&current, &expected) {
            return Err(WorkspaceRunLifecycleError);
        }
        let mut lifecycle = self
            .workspace_run_lifecycle
            .state
            .lock()
            .map_err(|_| WorkspaceRunLifecycleError)?;
        if !matches!(lifecycle.admission, SyncRunAdmission::Open)
            || matches!(lifecycle.run, RegisteredSyncRun::Finalizing(_))
        {
            return Err(WorkspaceRunLifecycleError);
        }
        let token = SyncWorkspaceTransitionToken(
            self.workspace_run_lifecycle
                .next_transition
                .fetch_add(1, Ordering::Relaxed),
        );
        lifecycle.admission = SyncRunAdmission::Transitioning {
            token,
            expected: expected.clone(),
            abandoned: false,
            publication_pending: false,
            recovery_on_drain: None,
            task_drop_candidate: None,
        };
        let terminal = match std::mem::replace(&mut lifecycle.run, RegisteredSyncRun::Empty) {
            RegisteredSyncRun::Queued(queued) => {
                queued.cancellation.cancel();
                let status = self
                    .sync_status
                    .complete_cancelled(queued.run_id)
                    .map_err(|_| {
                        lifecycle.admission = SyncRunAdmission::RecoveryClosed;
                        lifecycle.run = RegisteredSyncRun::Queued(queued.clone());
                        WorkspaceRunLifecycleError
                    })?;
                let terminal = SyncTerminalPublication {
                    run_id: queued.run_id,
                    revision: queued.config_revision.clone(),
                    status,
                };
                lifecycle.run = RegisteredSyncRun::Finalizing(queued);
                Some(terminal)
            }
            RegisteredSyncRun::Running(running) => {
                running.cancellation.cancel();
                lifecycle.run = RegisteredSyncRun::Running(running);
                None
            }
            RegisteredSyncRun::Empty => None,
            RegisteredSyncRun::Finalizing(finalizing) => {
                lifecycle.run = RegisteredSyncRun::Finalizing(finalizing);
                return Err(WorkspaceRunLifecycleError);
            }
        };
        Ok((
            SyncWorkspaceTransition {
                runtime: Arc::downgrade(self),
                token,
                drop_policy: SyncWorkspaceTransitionDropPolicy::Reopen,
            },
            terminal,
        ))
    }

    pub(crate) fn finish_sync_workspace_transition_start(
        &self,
        terminal: Option<SyncTerminalPublication>,
    ) -> Result<(), WorkspaceRunLifecycleError> {
        if let Some(terminal) = terminal {
            self.publish_sync_terminal(&terminal);
            self.finish_sync_terminal(terminal.run_id)
        } else {
            self.workspace_run_lifecycle.drained.notify_waiters();
            Ok(())
        }
    }

    #[doc(hidden)]
    pub async fn begin_sync_workspace_transition_for_test(
        self: &Arc<Self>,
    ) -> Result<SyncWorkspaceTransition, WorkspaceRunLifecycleError> {
        let mutation = self.mutation_coordinator.lock().await;
        let expected = self
            .owner
            .active_snapshot()
            .map_err(|_| WorkspaceRunLifecycleError)?;
        let (transition, terminal) = self.begin_sync_workspace_transition(expected, &mutation)?;
        drop(mutation);
        self.finish_sync_workspace_transition_start(terminal)?;
        Ok(transition)
    }

    #[doc(hidden)]
    pub fn try_begin_sync_workspace_transition_for_test(
        self: &Arc<Self>,
    ) -> Result<SyncWorkspaceTransition, WorkspaceRunLifecycleError> {
        let mutation = self
            .mutation_coordinator
            .try_lock()
            .map_err(|_| WorkspaceRunLifecycleError)?;
        let expected = self
            .owner
            .active_snapshot()
            .map_err(|_| WorkspaceRunLifecycleError)?;
        let (transition, terminal) = self.begin_sync_workspace_transition(expected, &mutation)?;
        drop(mutation);
        self.finish_sync_workspace_transition_start(terminal)?;
        Ok(transition)
    }

    #[doc(hidden)]
    pub fn mutation_is_available_for_test(&self) -> bool {
        self.mutation_coordinator.try_lock().is_ok()
    }

    #[doc(hidden)]
    pub async fn wait_for_empty_sync_run_for_test(&self) -> Result<(), WorkspaceRunLifecycleError> {
        loop {
            let notified = self.workspace_run_lifecycle.drained.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let lifecycle = self
                    .workspace_run_lifecycle
                    .state
                    .lock()
                    .map_err(|_| WorkspaceRunLifecycleError)?;
                if matches!(lifecycle.run, RegisteredSyncRun::Empty) {
                    return Ok(());
                }
            }
            notified.await;
        }
    }

    #[doc(hidden)]
    pub async fn wait_for_sync_workspace_publication_attempt_for_test(&self) {
        loop {
            if self
                .workspace_run_lifecycle
                .last_publication_attempted
                .load(Ordering::Acquire)
                != 0
            {
                return;
            }
            let notified = self
                .workspace_run_lifecycle
                .publication_attempted
                .notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self
                .workspace_run_lifecycle
                .last_publication_attempted
                .load(Ordering::Acquire)
                != 0
            {
                return;
            }
            notified.await;
        }
    }

    #[doc(hidden)]
    pub fn poison_sync_lifecycle_for_test(&self) {
        let _caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = self
                .workspace_run_lifecycle
                .state
                .lock()
                .expect("lifecycle lock before poison");
            panic!("deterministic lifecycle poison");
        }));
    }

    #[doc(hidden)]
    pub fn poison_sync_status_for_test(&self) {
        self.sync_status.poison_for_test();
    }

    #[doc(hidden)]
    pub fn close_sync_admission_for_test(&self) -> Result<(), WorkspaceRunLifecycleError> {
        let mut lifecycle = self
            .workspace_run_lifecycle
            .state
            .lock()
            .map_err(|_| WorkspaceRunLifecycleError)?;
        if !matches!(lifecycle.admission, SyncRunAdmission::Open) {
            return Err(WorkspaceRunLifecycleError);
        }
        lifecycle.admission = SyncRunAdmission::RecoveryClosed;
        Ok(())
    }

    fn reopen_sync_workspace_transition(
        &self,
        token: SyncWorkspaceTransitionToken,
        mutation: &MutationPermit<'_>,
    ) -> Result<(), WorkspaceRunLifecycleError> {
        if !self.mutation_coordinator.recognizes(mutation) {
            return Err(WorkspaceRunLifecycleError);
        }
        self.complete_sync_workspace_transition(token)
    }

    fn complete_sync_workspace_transition(
        &self,
        token: SyncWorkspaceTransitionToken,
    ) -> Result<(), WorkspaceRunLifecycleError> {
        if self.verify_instance_lock().is_err() {
            let _recovery = self.recover_sync_workspace_transition(token, None);
            self.fail_close_sync_workspace_transition(token);
            return Err(WorkspaceRunLifecycleError);
        }
        let verified = match self.active_workspace_snapshot() {
            Ok(verified) => verified,
            Err(_) => {
                let _recovery = self.recover_sync_workspace_transition(token, None);
                self.fail_close_sync_workspace_transition(token);
                return Err(WorkspaceRunLifecycleError);
            }
        };
        let mut owner = self
            .owner
            .workspace
            .write()
            .map_err(|_| WorkspaceRunLifecycleError)?;
        let mut lifecycle = self
            .workspace_run_lifecycle
            .state
            .lock()
            .map_err(|_| WorkspaceRunLifecycleError)?;
        if !matches!(lifecycle.run, RegisteredSyncRun::Empty) {
            lifecycle.admission = SyncRunAdmission::RecoveryClosed;
            return Err(WorkspaceRunLifecycleError);
        }
        let exact = match &lifecycle.admission {
            SyncRunAdmission::Transitioning {
                token: current_token,
                expected,
                ..
            } if *current_token == token => matches!(
                &*owner,
                WorkspaceOwnerState::Active(current)
                    if Arc::ptr_eq(current, expected) && Arc::ptr_eq(current, &verified)
            ),
            _ => false,
        };
        if exact {
            lifecycle.admission = SyncRunAdmission::Open;
            return Ok(());
        }
        if matches!(
            lifecycle.admission,
            SyncRunAdmission::Transitioning { token: current, .. } if current == token
        ) {
            if let WorkspaceOwnerState::Active(current) = &*owner {
                let retained = current.clone();
                *owner = WorkspaceOwnerState::RecoveryRequired {
                    _hold: WorkspaceRecoveryHold {
                        _last_known: Some(retained.clone()),
                        _retained_candidate: Some(retained.authority.clone()),
                    },
                };
            }
            lifecycle.admission = SyncRunAdmission::RecoveryClosed;
        }
        Err(WorkspaceRunLifecycleError)
    }

    fn fail_close_sync_workspace_transition(&self, token: SyncWorkspaceTransitionToken) {
        let Ok(mut lifecycle) = self.workspace_run_lifecycle.state.lock() else {
            return;
        };
        if matches!(
            lifecycle.admission,
            SyncRunAdmission::Transitioning { token: current, .. } if current == token
        ) {
            lifecycle.admission = SyncRunAdmission::RecoveryClosed;
        }
    }

    fn recover_sync_workspace_transition(
        &self,
        token: SyncWorkspaceTransitionToken,
        retained_candidate: Option<Arc<ActiveWorkspaceAuthority>>,
    ) -> Result<(), WorkspaceRunLifecycleError> {
        let mut owner = self
            .owner
            .workspace
            .write()
            .map_err(|_| WorkspaceRunLifecycleError)?;
        let mut lifecycle = self
            .workspace_run_lifecycle
            .state
            .lock()
            .map_err(|_| WorkspaceRunLifecycleError)?;
        let expected = match &lifecycle.admission {
            SyncRunAdmission::Transitioning {
                token: current,
                expected,
                ..
            } if *current == token => expected.clone(),
            _ => {
                lifecycle.admission = SyncRunAdmission::RecoveryClosed;
                return Err(WorkspaceRunLifecycleError);
            }
        };
        let owner_matches = matches!(
            &*owner,
            WorkspaceOwnerState::Active(current) if Arc::ptr_eq(current, &expected)
        );
        if !owner_matches {
            lifecycle.admission = SyncRunAdmission::RecoveryClosed;
            return Err(WorkspaceRunLifecycleError);
        }
        let retained_candidate = retained_candidate.or_else(|| Some(expected.authority.clone()));
        if !matches!(lifecycle.run, RegisteredSyncRun::Empty) {
            let SyncRunAdmission::Transitioning {
                recovery_on_drain, ..
            } = &mut lifecycle.admission
            else {
                return Err(WorkspaceRunLifecycleError);
            };
            *recovery_on_drain = retained_candidate;
            return Ok(());
        }
        *owner = WorkspaceOwnerState::RecoveryRequired {
            _hold: WorkspaceRecoveryHold {
                _last_known: Some(expected.clone()),
                _retained_candidate: retained_candidate,
            },
        };
        lifecycle.admission = SyncRunAdmission::RecoveryClosed;
        Ok(())
    }

    fn retain_sync_workspace_transition_candidate(
        &self,
        token: SyncWorkspaceTransitionToken,
        retained_candidate: Option<Arc<ActiveWorkspaceAuthority>>,
    ) -> Result<(), WorkspaceRunLifecycleError> {
        let mut lifecycle = self
            .workspace_run_lifecycle
            .state
            .lock()
            .map_err(|_| WorkspaceRunLifecycleError)?;
        let SyncRunAdmission::Transitioning {
            token: current,
            expected,
            task_drop_candidate,
            ..
        } = &mut lifecycle.admission
        else {
            return Err(WorkspaceRunLifecycleError);
        };
        if *current != token {
            return Err(WorkspaceRunLifecycleError);
        }
        *task_drop_candidate =
            Some(retained_candidate.unwrap_or_else(|| expected.authority.clone()));
        Ok(())
    }

    pub(crate) fn commit_sync_workspace_transition(
        &self,
        transition: &mut SyncWorkspaceTransition,
        prepared: &PreparedWorkspaceAuthority,
        workspace: WorkspaceDto,
        repository_binding: PrimaryWorkspaceRepositoryBinding,
        mutation: &MutationPermit<'_>,
    ) -> Result<Arc<ActiveWorkspaceSnapshot>, WorkspaceAuthorityError> {
        if !self.mutation_coordinator.recognizes(mutation) {
            return Err(WorkspaceAuthorityError::unavailable());
        }
        prepared.candidate.verify_held_directory()?;
        self.paths
            .validate_host_workspace_root(prepared.candidate.root.as_ref())
            .map_err(WorkspaceAuthorityError::from_path)?;
        let mut owner = self
            .owner
            .workspace
            .write()
            .map_err(|_| WorkspaceAuthorityError::unavailable())?;
        let mut lifecycle = self
            .workspace_run_lifecycle
            .state
            .lock()
            .map_err(|_| WorkspaceAuthorityError::unavailable())?;
        let WorkspaceOwnerState::Active(current) = &*owner else {
            lifecycle.admission = SyncRunAdmission::RecoveryClosed;
            return Err(WorkspaceAuthorityError::unavailable());
        };
        if !matches!(
            &lifecycle.admission,
            SyncRunAdmission::Transitioning { token, expected, .. }
                if *token == transition.token && Arc::ptr_eq(expected, current)
        ) || !matches!(
            &prepared.expected,
            WorkspaceOwnerIdentity::Active(prepared_expected)
                if Arc::ptr_eq(prepared_expected, current)
        ) || !current.repository_binding.matches(&repository_binding)
            || !matches!(lifecycle.run, RegisteredSyncRun::Empty)
        {
            lifecycle.admission = SyncRunAdmission::RecoveryClosed;
            return Err(WorkspaceAuthorityError::prepared_authority_mismatch());
        }
        let snapshot = Arc::new(ActiveWorkspaceSnapshot::new(
            prepared.candidate.clone(),
            workspace,
            repository_binding,
        ));
        *owner = WorkspaceOwnerState::Active(snapshot.clone());
        let SyncRunAdmission::Transitioning {
            expected,
            publication_pending,
            ..
        } = &mut lifecycle.admission
        else {
            lifecycle.admission = SyncRunAdmission::RecoveryClosed;
            return Err(WorkspaceAuthorityError::unavailable());
        };
        *expected = snapshot.clone();
        *publication_pending = true;
        transition.drop_policy = SyncWorkspaceTransitionDropPolicy::Recovery {
            retained_candidate: Some(prepared.candidate.clone()),
        };
        Ok(snapshot)
    }

    pub(crate) fn verify_document_mutation_admission(
        &self,
        mutation: &MutationPermit<'_>,
    ) -> Result<(), WorkspaceRunLifecycleError> {
        if !self.mutation_coordinator.recognizes(mutation) {
            return Err(WorkspaceRunLifecycleError);
        }
        let lifecycle = self
            .workspace_run_lifecycle
            .state
            .lock()
            .map_err(|_| WorkspaceRunLifecycleError)?;
        match &lifecycle.admission {
            SyncRunAdmission::Open
            | SyncRunAdmission::Transitioning {
                publication_pending: false,
                ..
            } => Ok(()),
            SyncRunAdmission::Uninitialized
            | SyncRunAdmission::Transitioning {
                publication_pending: true,
                ..
            }
            | SyncRunAdmission::RecoveryClosed => Err(WorkspaceRunLifecycleError),
        }
    }

    pub fn install_system_api_service(
        &self,
        service: Arc<dyn SystemApiService>,
    ) -> Result<(), ApiServiceAlreadyInstalled> {
        self.system_api
            .set(service)
            .map_err(|_| ApiServiceAlreadyInstalled)
    }

    pub fn install_workspace_api_service(
        &self,
        service: Arc<dyn WorkspaceApiService>,
    ) -> Result<(), ApiServiceAlreadyInstalled> {
        self.workspace_api
            .set(service)
            .map_err(|_| ApiServiceAlreadyInstalled)
    }

    pub fn install_documents_api_service(
        &self,
        service: Arc<dyn DocumentsApiService>,
    ) -> Result<(), ApiServiceAlreadyInstalled> {
        self.documents_api
            .set(service)
            .map_err(|_| ApiServiceAlreadyInstalled)
    }

    pub fn install_settings_api_service(
        &self,
        service: Arc<dyn SettingsApiService>,
    ) -> Result<(), ApiServiceAlreadyInstalled> {
        self.settings_api
            .set(service)
            .map_err(|_| ApiServiceAlreadyInstalled)
    }

    pub fn install_sync_api_service(
        &self,
        service: Arc<dyn SyncApiService>,
    ) -> Result<(), ApiServiceAlreadyInstalled> {
        self.sync_api
            .set(service)
            .map_err(|_| ApiServiceAlreadyInstalled)
    }

    pub(crate) fn system_api_service(&self) -> Option<&Arc<dyn SystemApiService>> {
        self.system_api.get()
    }

    pub(crate) fn workspace_api_service(&self) -> Option<&Arc<dyn WorkspaceApiService>> {
        self.workspace_api.get()
    }

    pub(crate) fn documents_api_service(&self) -> Option<&Arc<dyn DocumentsApiService>> {
        self.documents_api.get()
    }

    pub(crate) fn settings_api_service(&self) -> Option<&Arc<dyn SettingsApiService>> {
        self.settings_api.get()
    }

    pub(crate) fn sync_api_service(&self) -> Option<&Arc<dyn SyncApiService>> {
        self.sync_api.get()
    }
}

impl fmt::Debug for KernelRuntime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("KernelRuntime")
            .field("instance_id", &self.instance_id())
            .field("profile", &self.paths.profile())
            .field("ports", &"KernelPorts(..)")
            .field("lease", &"held")
            .finish()
    }
}

pub struct KernelRuntimeOwner {
    workspace: RwLock<WorkspaceOwnerState>,
}

impl KernelRuntimeOwner {
    fn new(authority: Arc<ActiveWorkspaceAuthority>) -> Self {
        Self {
            workspace: RwLock::new(WorkspaceOwnerState::AuthorityOnly(authority)),
        }
    }

    fn compatibility_authority(
        &self,
    ) -> Result<Arc<ActiveWorkspaceAuthority>, WorkspaceAuthorityError> {
        let state = self
            .workspace
            .read()
            .map_err(|_| WorkspaceAuthorityError::unavailable())?;
        match &*state {
            WorkspaceOwnerState::AuthorityOnly(authority) => Ok(authority.clone()),
            WorkspaceOwnerState::Active(snapshot) => Ok(snapshot.authority.clone()),
            WorkspaceOwnerState::RecoveryRequired { .. } => {
                Err(WorkspaceAuthorityError::unavailable())
            }
        }
    }

    fn active_snapshot(&self) -> Result<Arc<ActiveWorkspaceSnapshot>, WorkspaceAuthorityError> {
        let state = self
            .workspace
            .read()
            .map_err(|_| WorkspaceAuthorityError::unavailable())?;
        match &*state {
            WorkspaceOwnerState::Active(snapshot) => Ok(snapshot.clone()),
            WorkspaceOwnerState::AuthorityOnly(_)
            | WorkspaceOwnerState::RecoveryRequired { .. } => {
                Err(WorkspaceAuthorityError::unavailable())
            }
        }
    }

    fn preparation_identity(&self) -> Result<WorkspaceOwnerIdentity, WorkspaceAuthorityError> {
        let state = self
            .workspace
            .read()
            .map_err(|_| WorkspaceAuthorityError::unavailable())?;
        match &*state {
            WorkspaceOwnerState::AuthorityOnly(authority) => {
                Ok(WorkspaceOwnerIdentity::AuthorityOnly(authority.clone()))
            }
            WorkspaceOwnerState::Active(snapshot) => {
                Ok(WorkspaceOwnerIdentity::Active(snapshot.clone()))
            }
            WorkspaceOwnerState::RecoveryRequired { .. } => {
                Err(WorkspaceAuthorityError::unavailable())
            }
        }
    }

    fn verify_identity(
        &self,
        expected: &WorkspaceOwnerIdentity,
    ) -> Result<(), WorkspaceAuthorityError> {
        let state = self
            .workspace
            .read()
            .map_err(|_| WorkspaceAuthorityError::unavailable())?;
        if expected.matches(&state) {
            Ok(())
        } else if matches!(&*state, WorkspaceOwnerState::RecoveryRequired { .. }) {
            Err(WorkspaceAuthorityError::unavailable())
        } else {
            Err(WorkspaceAuthorityError::prepared_authority_mismatch())
        }
    }

    fn verify_authority_only_identity(
        &self,
        expected: &WorkspaceOwnerIdentity,
    ) -> Result<(), WorkspaceAuthorityError> {
        let state = self
            .workspace
            .read()
            .map_err(|_| WorkspaceAuthorityError::unavailable())?;
        match (&*state, expected) {
            (
                WorkspaceOwnerState::AuthorityOnly(current),
                WorkspaceOwnerIdentity::AuthorityOnly(expected),
            ) if Arc::ptr_eq(current, expected) => Ok(()),
            (WorkspaceOwnerState::AuthorityOnly(_), _) => {
                Err(WorkspaceAuthorityError::prepared_authority_mismatch())
            }
            (WorkspaceOwnerState::Active(_), _)
            | (WorkspaceOwnerState::RecoveryRequired { .. }, _) => {
                Err(WorkspaceAuthorityError::unavailable())
            }
        }
    }

    fn commit_authority_only(
        &self,
        prepared: PreparedWorkspaceAuthority,
    ) -> Result<Arc<ActiveWorkspaceAuthority>, WorkspaceAuthorityError> {
        let mut state = self
            .workspace
            .write()
            .map_err(|_| WorkspaceAuthorityError::unavailable())?;
        match (&*state, &prepared.expected) {
            (
                WorkspaceOwnerState::AuthorityOnly(current),
                WorkspaceOwnerIdentity::AuthorityOnly(expected),
            ) if Arc::ptr_eq(current, expected) => {
                let installed = prepared.candidate;
                *state = WorkspaceOwnerState::AuthorityOnly(installed.clone());
                Ok(installed)
            }
            (WorkspaceOwnerState::AuthorityOnly(_), _) => {
                Err(WorkspaceAuthorityError::prepared_authority_mismatch())
            }
            (WorkspaceOwnerState::Active(_), _)
            | (WorkspaceOwnerState::RecoveryRequired { .. }, _) => {
                Err(WorkspaceAuthorityError::unavailable())
            }
        }
    }

    fn initialization(
        &self,
        repository_binding: &PrimaryWorkspaceRepositoryBinding,
    ) -> Result<WorkspaceInitialization, WorkspaceInitializationError> {
        let state = self
            .workspace
            .read()
            .map_err(|_| WorkspaceInitializationError::unavailable())?;
        match &*state {
            WorkspaceOwnerState::AuthorityOnly(authority) => Ok(WorkspaceInitialization {
                expected: WorkspaceOwnerIdentity::AuthorityOnly(authority.clone()),
            }),
            WorkspaceOwnerState::Active(snapshot)
                if snapshot.repository_binding.matches(repository_binding) =>
            {
                Ok(WorkspaceInitialization {
                    expected: WorkspaceOwnerIdentity::Active(snapshot.clone()),
                })
            }
            WorkspaceOwnerState::Active(_) => {
                Err(WorkspaceInitializationError::foreign_repository())
            }
            WorkspaceOwnerState::RecoveryRequired { .. } => {
                Err(WorkspaceInitializationError::recovery_required())
            }
        }
    }

    fn initialize_snapshot(
        &self,
        initialization: &WorkspaceInitialization,
        workspace: WorkspaceDto,
        repository_binding: PrimaryWorkspaceRepositoryBinding,
        lifecycle: &WorkspaceRunLifecycle,
    ) -> Result<Arc<ActiveWorkspaceSnapshot>, WorkspaceInitializationError> {
        let mut owner = self
            .workspace
            .write()
            .map_err(|_| WorkspaceInitializationError::unavailable())?;
        match (&*owner, &initialization.expected) {
            (
                WorkspaceOwnerState::AuthorityOnly(current),
                WorkspaceOwnerIdentity::AuthorityOnly(expected),
            ) if Arc::ptr_eq(current, expected) => {
                let mut lifecycle = lifecycle
                    .state
                    .lock()
                    .map_err(|_| WorkspaceInitializationError::unavailable())?;
                if !matches!(lifecycle.admission, SyncRunAdmission::Uninitialized)
                    || !matches!(lifecycle.run, RegisteredSyncRun::Empty)
                {
                    return Err(WorkspaceInitializationError::unavailable());
                }
                let snapshot = Arc::new(ActiveWorkspaceSnapshot::new(
                    current.clone(),
                    workspace,
                    repository_binding,
                ));
                *owner = WorkspaceOwnerState::Active(snapshot.clone());
                lifecycle.admission = SyncRunAdmission::Open;
                Ok(snapshot)
            }
            (WorkspaceOwnerState::Active(current), WorkspaceOwnerIdentity::Active(expected))
                if Arc::ptr_eq(current, expected)
                    && current.repository_binding.matches(&repository_binding)
                    && current.workspace == workspace =>
            {
                let lifecycle = lifecycle
                    .state
                    .lock()
                    .map_err(|_| WorkspaceInitializationError::unavailable())?;
                if !matches!(lifecycle.admission, SyncRunAdmission::Open) {
                    return Err(WorkspaceInitializationError::unavailable());
                }
                Ok(current.clone())
            }
            (WorkspaceOwnerState::Active(current), WorkspaceOwnerIdentity::Active(expected))
                if Arc::ptr_eq(current, expected)
                    && current.repository_binding.matches(&repository_binding) =>
            {
                let lifecycle = lifecycle
                    .state
                    .lock()
                    .map_err(|_| WorkspaceInitializationError::unavailable())?;
                if !matches!(lifecycle.admission, SyncRunAdmission::Open) {
                    return Err(WorkspaceInitializationError::unavailable());
                }
                Err(WorkspaceInitializationError::changed_canonical())
            }
            (WorkspaceOwnerState::RecoveryRequired { .. }, _) => {
                Err(WorkspaceInitializationError::recovery_required())
            }
            _ => Err(WorkspaceInitializationError::foreign_repository()),
        }
    }

    fn enter_recovery(
        &self,
        expected: WorkspaceOwnerIdentity,
        retained_candidate: Option<Arc<ActiveWorkspaceAuthority>>,
    ) -> Result<(), WorkspaceAuthorityError> {
        let mut state = self
            .workspace
            .write()
            .map_err(|_| WorkspaceAuthorityError::unavailable())?;
        if matches!(&*state, WorkspaceOwnerState::RecoveryRequired { .. }) {
            return Ok(());
        }
        if !expected.matches(&state) {
            return Err(WorkspaceAuthorityError::prepared_authority_mismatch());
        }
        let last_known = match &expected {
            WorkspaceOwnerIdentity::AuthorityOnly(_) => None,
            WorkspaceOwnerIdentity::Active(snapshot) => Some(snapshot.clone()),
        };
        let retained_candidate = retained_candidate.or_else(|| Some(expected.authority()));
        *state = WorkspaceOwnerState::RecoveryRequired {
            _hold: WorkspaceRecoveryHold {
                _last_known: last_known,
                _retained_candidate: retained_candidate,
            },
        };
        Ok(())
    }
}

impl fmt::Debug for KernelRuntimeOwner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("KernelRuntimeOwner { workspace: [REDACTED] }")
    }
}

enum WorkspaceOwnerState {
    AuthorityOnly(Arc<ActiveWorkspaceAuthority>),
    Active(Arc<ActiveWorkspaceSnapshot>),
    RecoveryRequired { _hold: WorkspaceRecoveryHold },
}

struct WorkspaceRecoveryHold {
    _last_known: Option<Arc<ActiveWorkspaceSnapshot>>,
    _retained_candidate: Option<Arc<ActiveWorkspaceAuthority>>,
}

#[derive(Clone)]
enum WorkspaceOwnerIdentity {
    AuthorityOnly(Arc<ActiveWorkspaceAuthority>),
    Active(Arc<ActiveWorkspaceSnapshot>),
}

impl WorkspaceOwnerIdentity {
    fn authority(&self) -> Arc<ActiveWorkspaceAuthority> {
        match self {
            Self::AuthorityOnly(authority) => authority.clone(),
            Self::Active(snapshot) => snapshot.authority.clone(),
        }
    }

    fn matches(&self, state: &WorkspaceOwnerState) -> bool {
        match (self, state) {
            (Self::AuthorityOnly(expected), WorkspaceOwnerState::AuthorityOnly(current)) => {
                Arc::ptr_eq(expected, current)
            }
            (Self::Active(expected), WorkspaceOwnerState::Active(current)) => {
                Arc::ptr_eq(expected, current)
            }
            _ => false,
        }
    }
}

pub(crate) struct WorkspaceInitialization {
    expected: WorkspaceOwnerIdentity,
}

impl WorkspaceInitialization {
    pub(crate) fn active_snapshot(&self) -> Option<Arc<ActiveWorkspaceSnapshot>> {
        match &self.expected {
            WorkspaceOwnerIdentity::AuthorityOnly(_) => None,
            WorkspaceOwnerIdentity::Active(snapshot) => Some(snapshot.clone()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WorkspaceInitializationErrorKind {
    ForeignRepository,
    ChangedCanonical,
    RecoveryRequired,
    Unavailable,
}

pub(crate) struct WorkspaceInitializationError {
    kind: WorkspaceInitializationErrorKind,
}

impl WorkspaceInitializationError {
    const fn foreign_repository() -> Self {
        Self {
            kind: WorkspaceInitializationErrorKind::ForeignRepository,
        }
    }

    const fn changed_canonical() -> Self {
        Self {
            kind: WorkspaceInitializationErrorKind::ChangedCanonical,
        }
    }

    const fn recovery_required() -> Self {
        Self {
            kind: WorkspaceInitializationErrorKind::RecoveryRequired,
        }
    }

    const fn unavailable() -> Self {
        Self {
            kind: WorkspaceInitializationErrorKind::Unavailable,
        }
    }

    pub(crate) const fn kind(&self) -> WorkspaceInitializationErrorKind {
        self.kind
    }
}

pub struct ActiveWorkspaceSnapshot {
    authority: Arc<ActiveWorkspaceAuthority>,
    identity: ActiveWorkspaceSnapshotIdentity,
    workspace: WorkspaceDto,
    repository_binding: PrimaryWorkspaceRepositoryBinding,
}

impl ActiveWorkspaceSnapshot {
    fn new(
        authority: Arc<ActiveWorkspaceAuthority>,
        workspace: WorkspaceDto,
        repository_binding: PrimaryWorkspaceRepositoryBinding,
    ) -> Self {
        Self {
            authority,
            identity: ActiveWorkspaceSnapshotIdentity(Uuid::new_v4()),
            workspace,
            repository_binding,
        }
    }

    pub const fn workspace(&self) -> &WorkspaceDto {
        &self.workspace
    }

    pub(crate) const fn identity(&self) -> ActiveWorkspaceSnapshotIdentity {
        self.identity
    }

    #[allow(dead_code)] // Consumed by the Task 2 document snapshot migration.
    pub(crate) fn authority(&self) -> &Arc<ActiveWorkspaceAuthority> {
        &self.authority
    }

    pub(crate) fn repository_binding(&self) -> PrimaryWorkspaceRepositoryBinding {
        self.repository_binding.clone()
    }

    pub(crate) fn matches_repository_binding(
        &self,
        candidate: &PrimaryWorkspaceRepositoryBinding,
    ) -> bool {
        self.repository_binding.matches(candidate)
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct ActiveWorkspaceSnapshotIdentity(Uuid);

impl fmt::Debug for ActiveWorkspaceSnapshotIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ActiveWorkspaceSnapshotIdentity([OPAQUE])")
    }
}

#[derive(Clone)]
pub struct SyncCancellation {
    state: Arc<SyncCancellationState>,
}

impl SyncCancellation {
    fn new() -> Self {
        Self {
            state: Arc::new(SyncCancellationState {
                cancelled: AtomicBool::new(false),
                notification: Notify::new(),
            }),
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.state.cancelled.load(Ordering::Acquire)
    }

    pub async fn cancelled(&self) {
        loop {
            if self.is_cancelled() {
                return;
            }
            let notification = self.state.notification.notified();
            tokio::pin!(notification);
            notification.as_mut().enable();
            if self.is_cancelled() {
                return;
            }
            notification.await;
        }
    }

    fn cancel(&self) {
        if !self.state.cancelled.swap(true, Ordering::AcqRel) {
            self.state.notification.notify_waiters();
        }
    }
}

impl fmt::Debug for SyncCancellation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SyncCancellation([READ_ONLY])")
    }
}

struct SyncCancellationState {
    cancelled: AtomicBool,
    notification: Notify,
}

struct WorkspaceRunLifecycle {
    next_transition: AtomicU64,
    last_publication_attempted: AtomicU64,
    state: StdMutex<WorkspaceRunLifecycleState>,
    drained: Notify,
    publication_attempted: Notify,
}

impl WorkspaceRunLifecycle {
    const fn new() -> Self {
        Self {
            next_transition: AtomicU64::new(1),
            last_publication_attempted: AtomicU64::new(0),
            state: StdMutex::new(WorkspaceRunLifecycleState {
                admission: SyncRunAdmission::Uninitialized,
                run: RegisteredSyncRun::Empty,
            }),
            drained: Notify::const_new(),
            publication_attempted: Notify::const_new(),
        }
    }

    fn abandon_transition(&self, token: SyncWorkspaceTransitionToken) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let SyncRunAdmission::Transitioning {
            token: current,
            abandoned,
            ..
        } = &mut state.admission
        else {
            return;
        };
        if *current != token {
            return;
        }
        *abandoned = true;
        if matches!(state.run, RegisteredSyncRun::Empty) {
            state.admission = SyncRunAdmission::Open;
        }
    }
}

struct WorkspaceRunLifecycleState {
    admission: SyncRunAdmission,
    run: RegisteredSyncRun,
}

enum SyncRunAdmission {
    Uninitialized,
    Open,
    Transitioning {
        token: SyncWorkspaceTransitionToken,
        expected: Arc<ActiveWorkspaceSnapshot>,
        abandoned: bool,
        publication_pending: bool,
        recovery_on_drain: Option<Arc<ActiveWorkspaceAuthority>>,
        task_drop_candidate: Option<Arc<ActiveWorkspaceAuthority>>,
    },
    RecoveryClosed,
}

enum RegisteredSyncRun {
    Empty,
    Queued(SyncRunRegistration),
    Running(SyncRunRegistration),
    Finalizing(SyncRunRegistration),
}

#[derive(Clone)]
struct SyncRunRegistration {
    run_id: crate::contract::RunId,
    trigger: SyncTrigger,
    config_revision: Revision,
    fallback_completed_at: Rfc3339Utc,
    snapshot: Arc<ActiveWorkspaceSnapshot>,
    cancellation: SyncCancellation,
}

pub(crate) struct QueuedSyncRun {
    pub(crate) attempting: SyncStatusDto,
}

pub(crate) struct ClaimedSyncRun {
    pub(crate) run_id: crate::contract::RunId,
    pub(crate) trigger: SyncTrigger,
    pub(crate) snapshot: Arc<ActiveWorkspaceSnapshot>,
    pub(crate) cancellation: SyncCancellation,
}

pub(crate) enum SyncRunClaim {
    Ready(ClaimedSyncRun),
    Rejected(Option<Box<SyncTerminalPublication>>),
}

pub(crate) struct SyncTerminalPublication {
    run_id: crate::contract::RunId,
    revision: Revision,
    status: SyncStatusDto,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SyncWorkspaceTransitionToken(u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceRunLifecycleError;

impl fmt::Display for WorkspaceRunLifecycleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("workspace sync lifecycle is unavailable")
    }
}

impl std::error::Error for WorkspaceRunLifecycleError {}

pub struct SyncWorkspaceTransition {
    runtime: Weak<KernelRuntime>,
    token: SyncWorkspaceTransitionToken,
    drop_policy: SyncWorkspaceTransitionDropPolicy,
}

enum SyncWorkspaceTransitionDropPolicy {
    Reopen,
    Recovery {
        retained_candidate: Option<Arc<ActiveWorkspaceAuthority>>,
    },
    PublicationAttempted,
    Complete,
}

impl SyncWorkspaceTransition {
    pub async fn wait_drained(&self) -> Result<(), WorkspaceRunLifecycleError> {
        let runtime = self.runtime.upgrade().ok_or(WorkspaceRunLifecycleError)?;
        loop {
            let notified = runtime.workspace_run_lifecycle.drained.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let state = runtime
                    .workspace_run_lifecycle
                    .state
                    .lock()
                    .map_err(|_| WorkspaceRunLifecycleError)?;
                match &state.admission {
                    SyncRunAdmission::Transitioning { token, .. } if *token == self.token => {
                        if matches!(state.run, RegisteredSyncRun::Empty) {
                            return Ok(());
                        }
                    }
                    _ => return Err(WorkspaceRunLifecycleError),
                }
            }
            notified.await;
        }
    }

    #[doc(hidden)]
    pub async fn reopen_for_test(mut self) -> Result<(), WorkspaceRunLifecycleError> {
        let runtime = self.runtime.upgrade().ok_or(WorkspaceRunLifecycleError)?;
        let mutation = runtime.mutation_coordinator.lock().await;
        if let Err(error) = runtime.reopen_sync_workspace_transition(self.token, &mutation) {
            runtime.fail_close_sync_workspace_transition(self.token);
            self.drop_policy = SyncWorkspaceTransitionDropPolicy::Complete;
            return Err(error);
        }
        self.drop_policy = SyncWorkspaceTransitionDropPolicy::Complete;
        Ok(())
    }

    pub(crate) fn arm_recovery_on_drop(
        &mut self,
        retained_candidate: Option<Arc<ActiveWorkspaceAuthority>>,
    ) -> Result<(), WorkspaceRunLifecycleError> {
        self.drop_policy = SyncWorkspaceTransitionDropPolicy::Recovery {
            retained_candidate: retained_candidate.clone(),
        };
        let runtime = self.runtime.upgrade().ok_or(WorkspaceRunLifecycleError)?;
        runtime.retain_sync_workspace_transition_candidate(self.token, retained_candidate)
    }

    pub(crate) fn publication_attempted(&mut self) {
        self.drop_policy = SyncWorkspaceTransitionDropPolicy::PublicationAttempted;
        if let Some(runtime) = self.runtime.upgrade() {
            runtime
                .workspace_run_lifecycle
                .last_publication_attempted
                .store(self.token.0, Ordering::Release);
            runtime
                .workspace_run_lifecycle
                .publication_attempted
                .notify_waiters();
        }
    }

    pub(crate) fn reopen(
        &mut self,
        mutation: &MutationPermit<'_>,
    ) -> Result<(), WorkspaceRunLifecycleError> {
        let runtime = self.runtime.upgrade().ok_or(WorkspaceRunLifecycleError)?;
        match runtime.reopen_sync_workspace_transition(self.token, mutation) {
            Ok(()) => {
                self.drop_policy = SyncWorkspaceTransitionDropPolicy::Complete;
                Ok(())
            }
            Err(error) => {
                let _recovery = runtime.recover_sync_workspace_transition(self.token, None);
                self.drop_policy = SyncWorkspaceTransitionDropPolicy::Complete;
                Err(error)
            }
        }
    }

    pub(crate) fn enter_recovery(
        &mut self,
        retained_candidate: Option<Arc<ActiveWorkspaceAuthority>>,
        mutation: &MutationPermit<'_>,
    ) -> Result<(), WorkspaceRunLifecycleError> {
        let runtime = self.runtime.upgrade().ok_or(WorkspaceRunLifecycleError)?;
        if !runtime.mutation_coordinator.recognizes(mutation) {
            return Err(WorkspaceRunLifecycleError);
        }
        let result = runtime.recover_sync_workspace_transition(self.token, retained_candidate);
        self.drop_policy = SyncWorkspaceTransitionDropPolicy::Complete;
        result
    }
}

impl Drop for SyncWorkspaceTransition {
    fn drop(&mut self) {
        let Some(runtime) = self.runtime.upgrade() else {
            return;
        };
        match &self.drop_policy {
            SyncWorkspaceTransitionDropPolicy::Reopen => runtime
                .workspace_run_lifecycle
                .abandon_transition(self.token),
            SyncWorkspaceTransitionDropPolicy::Recovery { retained_candidate } => {
                if runtime
                    .recover_sync_workspace_transition(self.token, retained_candidate.clone())
                    .is_err()
                {
                    runtime.fail_close_sync_workspace_transition(self.token);
                }
            }
            SyncWorkspaceTransitionDropPolicy::PublicationAttempted => {
                if runtime
                    .complete_sync_workspace_transition(self.token)
                    .is_err()
                {
                    runtime.fail_close_sync_workspace_transition(self.token);
                }
            }
            SyncWorkspaceTransitionDropPolicy::Complete => {}
        }
    }
}

impl fmt::Debug for ActiveWorkspaceSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ActiveWorkspaceSnapshot { authority: held, workspace: opaque }")
    }
}

pub struct ActiveWorkspaceAuthority {
    root: Arc<WorkspaceRoot>,
    lease: Arc<WorkspaceLockLease>,
}

impl ActiveWorkspaceAuthority {
    fn new(root: Arc<WorkspaceRoot>, lease: Arc<WorkspaceLockLease>) -> Self {
        Self { root, lease }
    }

    pub fn verify_held_directory(&self) -> Result<(), WorkspaceAuthorityError> {
        self.root
            .verify_held_directory()
            .map_err(WorkspaceAuthorityError::from_path)?;
        self.lease
            .verify_held_lock()
            .map_err(WorkspaceAuthorityError::from_lock)
    }

    pub fn root(&self) -> &WorkspaceRoot {
        self.root.as_ref()
    }
}

impl fmt::Debug for ActiveWorkspaceAuthority {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ActiveWorkspaceAuthority { capability: held, lock: held }")
    }
}

pub struct PreparedWorkspaceAuthority {
    expected: WorkspaceOwnerIdentity,
    candidate: Arc<ActiveWorkspaceAuthority>,
    binding: PreparedWorkspaceAuthorityBinding,
}

impl PreparedWorkspaceAuthority {
    pub fn binding(&self) -> PreparedWorkspaceAuthorityBinding {
        self.binding.clone()
    }

    pub(crate) fn candidate(&self) -> &Arc<ActiveWorkspaceAuthority> {
        &self.candidate
    }
}

impl fmt::Debug for PreparedWorkspaceAuthority {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PreparedWorkspaceAuthority { capability: held, lock: held }")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceAuthorityErrorKind {
    UnsupportedProfile,
    PreparedAuthorityMismatch,
    WorkspaceLocked,
    WorkspaceUnavailable,
    OverlappingRoots,
    UnsafeEntry,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct WorkspaceAuthorityError {
    kind: WorkspaceAuthorityErrorKind,
}

impl WorkspaceAuthorityError {
    pub const fn kind(self) -> WorkspaceAuthorityErrorKind {
        self.kind
    }

    const fn unsupported_profile() -> Self {
        Self {
            kind: WorkspaceAuthorityErrorKind::UnsupportedProfile,
        }
    }

    const fn prepared_authority_mismatch() -> Self {
        Self {
            kind: WorkspaceAuthorityErrorKind::PreparedAuthorityMismatch,
        }
    }

    const fn unavailable() -> Self {
        Self {
            kind: WorkspaceAuthorityErrorKind::WorkspaceUnavailable,
        }
    }

    fn from_path(error: PathPolicyError) -> Self {
        let kind = match error.kind() {
            PathPolicyErrorKind::OverlappingRoots => WorkspaceAuthorityErrorKind::OverlappingRoots,
            PathPolicyErrorKind::UnsafeEntry | PathPolicyErrorKind::InvalidManagedName => {
                WorkspaceAuthorityErrorKind::UnsafeEntry
            }
            PathPolicyErrorKind::Unavailable => WorkspaceAuthorityErrorKind::WorkspaceUnavailable,
        };
        Self { kind }
    }

    fn from_lock(error: KernelLockError) -> Self {
        let kind = match error.kind() {
            KernelLockErrorKind::WorkspaceLocked => WorkspaceAuthorityErrorKind::WorkspaceLocked,
            KernelLockErrorKind::UnsafeLockFile => WorkspaceAuthorityErrorKind::UnsafeEntry,
            KernelLockErrorKind::WorkspaceUnavailable
            | KernelLockErrorKind::InstanceLocked
            | KernelLockErrorKind::InstanceStateUnavailable => {
                WorkspaceAuthorityErrorKind::WorkspaceUnavailable
            }
        };
        Self { kind }
    }
}

impl fmt::Debug for WorkspaceAuthorityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WorkspaceAuthorityError")
            .field("kind", &self.kind)
            .finish()
    }
}

impl fmt::Display for WorkspaceAuthorityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            WorkspaceAuthorityErrorKind::UnsupportedProfile => {
                formatter.write_str("the host profile cannot switch workspaces")
            }
            WorkspaceAuthorityErrorKind::PreparedAuthorityMismatch => {
                formatter.write_str("the prepared workspace authority is stale")
            }
            WorkspaceAuthorityErrorKind::WorkspaceLocked => {
                formatter.write_str("the workspace is already in use")
            }
            WorkspaceAuthorityErrorKind::WorkspaceUnavailable => {
                formatter.write_str("the workspace is unavailable")
            }
            WorkspaceAuthorityErrorKind::OverlappingRoots => {
                formatter.write_str("the workspace overlaps a private Kernel root")
            }
            WorkspaceAuthorityErrorKind::UnsafeEntry => {
                formatter.write_str("the workspace address is unsafe")
            }
        }
    }
}

impl std::error::Error for WorkspaceAuthorityError {}

impl EventSink for KernelRuntime {
    fn publish(&self, publication: &EventPublication) -> Result<(), EventSinkError> {
        let host_result = self.ports.event_sink().publish(publication);
        let broker_result = self.event_broker.publish(publication);
        host_result.and(broker_result)
    }
}

#[derive(Debug, Default)]
pub struct MutationCoordinator {
    gate: Mutex<()>,
}

impl MutationCoordinator {
    pub const fn new() -> Self {
        Self {
            gate: Mutex::const_new(()),
        }
    }

    pub async fn lock(&self) -> MutationPermit<'_> {
        MutationPermit {
            coordinator: self,
            _guard: self.gate.lock().await,
        }
    }

    pub(crate) fn try_lock(&self) -> Result<MutationPermit<'_>, tokio::sync::TryLockError> {
        Ok(MutationPermit {
            coordinator: self,
            _guard: self.gate.try_lock()?,
        })
    }

    fn recognizes(&self, permit: &MutationPermit<'_>) -> bool {
        std::ptr::eq(self, permit.coordinator)
    }
}

#[must_use = "the mutation gate is released when this permit is dropped"]
pub struct MutationPermit<'a> {
    coordinator: &'a MutationCoordinator,
    _guard: MutexGuard<'a, ()>,
}

impl fmt::Debug for MutationPermit<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("MutationPermit { gate: held }")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KernelStartupErrorKind {
    InstanceLocked,
    WorkspaceLocked,
    InstanceStateUnavailable,
    WorkspaceUnavailable,
    UnsafeLockFile,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct KernelStartupError {
    kind: KernelStartupErrorKind,
}

impl KernelStartupError {
    pub const fn kind(self) -> KernelStartupErrorKind {
        self.kind
    }

    fn from_lock(error: KernelLockError) -> Self {
        let kind = match error.kind() {
            KernelLockErrorKind::InstanceLocked => KernelStartupErrorKind::InstanceLocked,
            KernelLockErrorKind::WorkspaceLocked => KernelStartupErrorKind::WorkspaceLocked,
            KernelLockErrorKind::InstanceStateUnavailable => {
                KernelStartupErrorKind::InstanceStateUnavailable
            }
            KernelLockErrorKind::WorkspaceUnavailable => {
                KernelStartupErrorKind::WorkspaceUnavailable
            }
            KernelLockErrorKind::UnsafeLockFile => KernelStartupErrorKind::UnsafeLockFile,
        };
        Self { kind }
    }

    const fn workspace_unavailable() -> Self {
        Self {
            kind: KernelStartupErrorKind::WorkspaceUnavailable,
        }
    }
}

impl fmt::Debug for KernelStartupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("KernelStartupError")
            .field("kind", &self.kind)
            .finish()
    }
}

impl fmt::Display for KernelStartupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            KernelStartupErrorKind::InstanceLocked => {
                formatter.write_str("the Kernel instance is already running")
            }
            KernelStartupErrorKind::WorkspaceLocked => {
                formatter.write_str("the workspace is already in use")
            }
            KernelStartupErrorKind::InstanceStateUnavailable => {
                formatter.write_str("the Kernel instance state is unavailable")
            }
            KernelStartupErrorKind::WorkspaceUnavailable => {
                formatter.write_str("the workspace is unavailable")
            }
            KernelStartupErrorKind::UnsafeLockFile => {
                formatter.write_str("a Kernel lock file is unsafe")
            }
        }
    }
}

impl std::error::Error for KernelStartupError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiServiceAlreadyInstalled;

impl fmt::Display for ApiServiceAlreadyInstalled {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a Kernel API service is already installed")
    }
}

impl std::error::Error for ApiServiceAlreadyInstalled {}

#[derive(Clone, Eq, PartialEq)]
pub struct ServiceFailure {
    code: ErrorCode,
    details: Option<ErrorDetails>,
}

impl ServiceFailure {
    pub fn new(
        code: ErrorCode,
        details: Option<ErrorDetails>,
    ) -> Result<Self, InvalidServiceFailure> {
        safe_error_envelope(
            code,
            crate::contract::RequestId::new(Uuid::nil()),
            details.clone(),
        )
        .map_err(|_| InvalidServiceFailure)?;
        Ok(Self { code, details })
    }

    pub const fn code(&self) -> ErrorCode {
        self.code
    }

    pub const fn details(&self) -> Option<&ErrorDetails> {
        self.details.as_ref()
    }
}

impl fmt::Debug for ServiceFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ServiceFailure")
            .field("code", &self.code)
            .field("details", &self.details)
            .finish()
    }
}

impl fmt::Display for ServiceFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(safe_message_for_error_code(self.code))
    }
}

impl std::error::Error for ServiceFailure {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidServiceFailure;

impl fmt::Display for InvalidServiceFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("service error details do not apply to the error code")
    }
}

impl std::error::Error for InvalidServiceFailure {}

#[async_trait]
pub trait SystemApiService: Send + Sync {
    async fn ready(&self) -> Result<ReadyHealthResponse, ServiceFailure>;
    async fn version(&self) -> Result<SystemVersionResponse, ServiceFailure>;
    async fn runtime_state(&self) -> Result<crate::contract::RuntimeStateDto, ServiceFailure>;
}

#[async_trait]
pub trait WorkspaceApiService: Send + Sync {
    async fn get_workspace(&self) -> Result<WorkspaceDto, ServiceFailure>;
}

#[async_trait]
pub trait DocumentsApiService: Send + Sync {
    async fn list_documents(
        &self,
        query: ListDocumentsQuery,
    ) -> Result<DocumentPageDto, ServiceFailure>;
    async fn create_document(
        &self,
        request: CreateDocumentRequest,
    ) -> Result<CreatedDocumentDto, ServiceFailure>;
    async fn get_document(
        &self,
        document_id: DocumentId,
    ) -> Result<DocumentContentDto, ServiceFailure>;
    async fn update_document(
        &self,
        document_id: DocumentId,
        request: UpdateDocumentRequest,
    ) -> Result<DocumentContentDto, ServiceFailure>;
    async fn move_document(
        &self,
        document_id: DocumentId,
        request: MoveDocumentRequest,
    ) -> Result<crate::contract::DocumentEntryDto, ServiceFailure>;
    async fn delete_document(
        &self,
        document_id: DocumentId,
        request: DeleteDocumentRequest,
    ) -> Result<(), ServiceFailure>;
    async fn list_document_history(
        &self,
        document_id: DocumentId,
        query: PageQuery,
    ) -> Result<DocumentHistoryPageDto, ServiceFailure>;
    async fn restore_document_history(
        &self,
        document_id: DocumentId,
        snapshot_id: SnapshotId,
        request: RestoreDocumentHistoryRequest,
    ) -> Result<DocumentContentDto, ServiceFailure>;
    async fn search_workspace(
        &self,
        query: SearchWorkspaceQuery,
    ) -> Result<SearchPageDto, ServiceFailure>;
}

#[async_trait]
pub trait SettingsApiService: Send + Sync {
    async fn get_settings(&self) -> Result<SettingsSnapshotDto, ServiceFailure>;
    async fn patch_settings(
        &self,
        request: PatchSettingsRequest,
    ) -> Result<SettingsSnapshotDto, ServiceFailure>;
}

#[async_trait]
pub trait SyncApiService: Send + Sync {
    async fn get_sync_config(&self) -> Result<SyncConfigViewDto, ServiceFailure>;
    async fn patch_sync_config(
        &self,
        request: PatchSyncConfigRequest,
    ) -> Result<SyncConfigViewDto, ServiceFailure>;
    async fn test_sync_connection(
        &self,
        request: TestSyncConnectionRequest,
    ) -> Result<SyncConnectionTestDto, ServiceFailure>;
    async fn get_sync_status(&self) -> Result<SyncStatusDto, ServiceFailure>;
    async fn trigger_sync_run(
        &self,
        request: TriggerSyncRunRequest,
    ) -> Result<SyncRunAcceptedDto, ServiceFailure>;
}

#[cfg(test)]
mod tests {
    use std::{fs, sync::Arc, thread};

    use tempfile::tempdir;

    use super::{KernelRuntime, WorkspaceAuthorityErrorKind};
    use crate::{config::KernelConfig, paths::ServerPathLayout, ports::KernelPorts};

    #[test]
    fn server_runtime_rejects_host_workspace_prepare() {
        let temporary = tempdir().expect("temporary server authority fixture");
        let data_root = temporary.path().join("data");
        let cache_root = temporary.path().join("cache");
        let alternate = temporary.path().join("alternate");
        fs::create_dir(&data_root).expect("server data root");
        fs::create_dir(&alternate).expect("alternate workspace");
        let paths = ServerPathLayout::for_test(&data_root, &cache_root)
            .activate()
            .expect("server paths");
        let runtime = KernelRuntime::activate(
            KernelConfig::generate().expect("kernel config"),
            paths,
            KernelPorts::unavailable(),
        )
        .expect("server runtime");

        let error = runtime
            .prepare_host_workspace_authority(&alternate)
            .expect_err("server profile must not prepare host workspaces");

        assert_eq!(
            error.kind(),
            WorkspaceAuthorityErrorKind::UnsupportedProfile
        );
    }

    #[test]
    fn raw_workspace_authority_fails_closed_when_owner_lock_is_poisoned() {
        let temporary = tempdir().expect("temporary server authority fixture");
        let data_root = temporary.path().join("data");
        let cache_root = temporary.path().join("cache");
        fs::create_dir(&data_root).expect("server data root");
        let paths = ServerPathLayout::for_test(&data_root, &cache_root)
            .activate()
            .expect("server paths");
        let runtime = Arc::new(
            KernelRuntime::activate(
                KernelConfig::generate().expect("kernel config"),
                paths,
                KernelPorts::unavailable(),
            )
            .expect("server runtime"),
        );
        let poisoning_runtime = Arc::clone(&runtime);

        let poisoned = thread::spawn(move || {
            let _guard = poisoning_runtime
                .owner
                .workspace
                .write()
                .expect("workspace owner lock");
            panic!("poison workspace owner lock");
        });
        assert!(poisoned.join().is_err());

        let error = runtime
            .active_workspace_authority()
            .expect_err("poisoned owner lock must not expose retained authority");
        assert_eq!(
            error.kind(),
            WorkspaceAuthorityErrorKind::WorkspaceUnavailable
        );
    }
}
