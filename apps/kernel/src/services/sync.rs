//! Sync service composition boundary.

use std::{
    fmt,
    future::{poll_fn, Future},
    panic::{catch_unwind, AssertUnwindSafe},
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex as StdMutex, Weak,
    },
    task::{Context, Poll},
};

use async_trait::async_trait;
use tokio::sync::oneshot;

pub use crate::runtime::{
    ActiveWorkspaceSnapshotIdentity as SyncWorkspaceSnapshotIdentity, SyncApiService,
    SyncCancellation,
};

use crate::{
    contract::{
        DomainEvent, ErrorCode, ErrorDetails, Nullable, PatchSyncConfigRequest, ResourceRefDto,
        Revision, RunId, SyncConfigReadiness, SyncConfigViewDto, SyncConnectionTestDto, SyncMode,
        SyncRunAcceptedDto, SyncSafeErrorCategory, SyncSafeErrorCode, SyncSafeErrorDto,
        SyncSafeErrorOperation, SyncStatusDto, SyncSummaryDto, SyncTrigger,
        TestSyncConnectionRequest, TriggerSyncRunRequest,
    },
    events::{EventPublication, EventSink as _},
    ports::BoxTaskFuture,
    runtime::{ClaimedSyncRun, KernelRuntime, MutationPermit, ServiceFailure, SyncRunClaim},
    sync::config::{
        SyncConfig, SyncConfigChangeError, SyncConfigLoad, SyncConfigStore,
        SyncConfigStoreErrorKind,
    },
    sync::editing::SyncEditingRegistry,
    sync::status::{SyncRunCompletion, SyncStatusState},
};

#[async_trait]
pub trait SyncExecutor: Send + Sync {
    async fn test_connection(&self, config: SyncConfig) -> Result<(), SyncExecutionError>;
    async fn run(
        &self,
        config: SyncConfig,
        context: SyncRunContext,
    ) -> Result<SyncSummaryDto, SyncExecutionError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KernelSyncTriggerRejection {
    ActiveRun,
    Closing,
    Disabled,
    Incomplete,
    ModeDisallowed,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KernelSyncTriggerDisposition {
    Accepted(SyncRunAcceptedDto),
    Rejected(KernelSyncTriggerRejection),
}

#[must_use = "Kernel trigger results carry a settlement barrier"]
pub struct KernelSyncTriggerResult {
    disposition: KernelSyncTriggerDisposition,
    settlement: SyncRunSettlement,
}

impl KernelSyncTriggerResult {
    fn accepted(run: StartedSyncRun) -> Self {
        Self {
            disposition: KernelSyncTriggerDisposition::Accepted(run.accepted),
            settlement: run.settlement,
        }
    }

    fn rejected(rejection: KernelSyncTriggerRejection) -> Self {
        Self {
            disposition: KernelSyncTriggerDisposition::Rejected(rejection),
            settlement: SyncRunSettlement::settled(),
        }
    }

    pub fn into_parts(self) -> (KernelSyncTriggerDisposition, SyncRunSettlement) {
        (self.disposition, self.settlement)
    }
}

#[must_use = "wait on the settlement barrier when shutdown ordering matters"]
pub struct SyncRunSettlement {
    receiver: oneshot::Receiver<()>,
}

impl SyncRunSettlement {
    fn channel() -> (Arc<SyncRunSettlementState>, Self) {
        let (sender, receiver) = oneshot::channel();
        (
            Arc::new(SyncRunSettlementState {
                sender: StdMutex::new(Some(sender)),
            }),
            Self { receiver },
        )
    }

    fn settled() -> Self {
        let (sender, receiver) = oneshot::channel();
        let _settled = sender.send(());
        Self { receiver }
    }

    pub async fn wait(self) {
        let _settled = self.receiver.await;
    }
}

struct SyncRunSettlementState {
    sender: StdMutex<Option<oneshot::Sender<()>>>,
}

impl SyncRunSettlementState {
    fn settle(&self) {
        let sender = self
            .sender
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(sender) = sender {
            let _settled = sender.send(());
        }
    }
}

struct StartedSyncRun {
    accepted: SyncRunAcceptedDto,
    settlement: SyncRunSettlement,
}

enum SyncRunTriggerFailure {
    ActiveRun,
    Disabled,
    Incomplete,
    ModeDisallowed,
    NotReady,
    RevisionConflict(Revision),
    Unavailable,
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
    settlement: Arc<SyncRunSettlementState>,
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
        let settlement = self
            .guard
            .as_ref()
            .map(|guard| guard.state.settlement.clone());
        drop(self.inner.take());
        drop(self.guard.take());
        if let Some(settlement) = settlement {
            settlement.settle();
        }
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

    #[allow(dead_code)] // Consumed by the Kernel-owned sync scope extraction.
    pub(crate) fn workspace_authority(&self) -> &Arc<crate::runtime::ActiveWorkspaceAuthority> {
        self.snapshot.authority()
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyncExecutionError {
    error: SyncSafeErrorDto,
    partial_summary: Option<SyncSummaryDto>,
}

impl SyncExecutionError {
    pub fn new(error: SyncSafeErrorDto) -> Self {
        Self {
            error,
            partial_summary: None,
        }
    }

    pub fn unknown(provider: crate::contract::SyncProvider) -> Self {
        Self::new(
            SyncSafeErrorDto::new(
                provider,
                SyncSafeErrorOperation::SyncRun,
                SyncSafeErrorCode::Unknown,
            )
            .with_category(SyncSafeErrorCategory::Provider),
        )
    }

    pub fn with_partial_summary(mut self, partial_summary: SyncSummaryDto) -> Self {
        self.partial_summary = Some(partial_summary);
        self
    }

    fn into_completion(
        self,
        provider: crate::contract::SyncProvider,
        run_id: RunId,
    ) -> SyncRunCompletion {
        if self.error.provider() != provider || self.error.run_id().is_some_and(|id| id != run_id) {
            return SyncRunCompletion::UnknownFailure;
        }
        let error = if self.error.run_id().is_none() {
            self.error.with_run_id(run_id)
        } else {
            self.error
        };
        SyncRunCompletion::Failed {
            error: Box::new(error),
            partial_summary: self.partial_summary,
        }
    }
}

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
    kernel_trigger_gate: StdMutex<bool>,
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
            kernel_trigger_gate: StdMutex::new(false),
        }
    }

    pub fn editing_registry(&self) -> Arc<SyncEditingRegistry> {
        self.editing.clone()
    }

    pub async fn trigger_kernel_sync(&self, trigger: SyncTrigger) -> KernelSyncTriggerResult {
        if self.verify_instance(ErrorCode::SyncNotReady).is_err() {
            return KernelSyncTriggerResult::rejected(KernelSyncTriggerRejection::Unavailable);
        }
        let mutation = self.runtime.mutation_coordinator().lock().await;
        if self.verify_instance(ErrorCode::SyncNotReady).is_err() {
            return KernelSyncTriggerResult::rejected(KernelSyncTriggerRejection::Unavailable);
        }
        let closing = match self.kernel_trigger_gate.lock() {
            Ok(closing) => closing,
            Err(_) => {
                return KernelSyncTriggerResult::rejected(KernelSyncTriggerRejection::Unavailable)
            }
        };
        if *closing {
            return KernelSyncTriggerResult::rejected(KernelSyncTriggerRejection::Closing);
        }
        let registered = self.runtime.sync_run_registered(&mutation);
        let attempting = self.status.is_attempting();
        match (registered, attempting) {
            (Ok(true), _) | (_, Ok(true)) => {
                return KernelSyncTriggerResult::rejected(KernelSyncTriggerRejection::ActiveRun)
            }
            (Ok(false), Ok(false)) => {}
            (Err(_), _) | (_, Err(_)) => {
                return KernelSyncTriggerResult::rejected(KernelSyncTriggerRejection::Unavailable)
            }
        }
        match self.start_sync_run(trigger, None, mutation) {
            Ok(run) => KernelSyncTriggerResult::accepted(run),
            Err(error) => KernelSyncTriggerResult::rejected(kernel_trigger_rejection(error)),
        }
    }

    pub fn close_kernel_triggers(&self) {
        let mut closing = self
            .kernel_trigger_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *closing = true;
    }

    fn verify_instance(&self, code: ErrorCode) -> Result<(), ServiceFailure> {
        self.runtime
            .verify_instance_lock()
            .map_err(|_| failure(code))
    }

    fn start_sync_run(
        &self,
        trigger: SyncTrigger,
        expected_revision: Option<Revision>,
        mutation: MutationPermit<'_>,
    ) -> Result<StartedSyncRun, SyncRunTriggerFailure> {
        let admitted_workspace = self
            .runtime
            .active_workspace_snapshot()
            .map_err(|_| SyncRunTriggerFailure::NotReady)?;
        let (config, revision) = match self
            .store
            .load()
            .map_err(|_| SyncRunTriggerFailure::NotReady)?
        {
            SyncConfigLoad::Loaded { config, revision } => (config, revision),
            SyncConfigLoad::Absent
            | SyncConfigLoad::Corrupt { .. }
            | SyncConfigLoad::Unsupported { .. } => return Err(SyncRunTriggerFailure::NotReady),
        };
        if expected_revision
            .as_ref()
            .is_some_and(|expected| expected != &revision)
        {
            return Err(SyncRunTriggerFailure::RevisionConflict(revision));
        }
        let exposed = config
            .to_view(revision.clone())
            .map_err(|_| SyncRunTriggerFailure::NotReady)?;
        match exposed.readiness {
            SyncConfigReadiness::Disabled => return Err(SyncRunTriggerFailure::Disabled),
            SyncConfigReadiness::Incomplete => return Err(SyncRunTriggerFailure::Incomplete),
            SyncConfigReadiness::Ready => {}
        }
        if !sync_mode_allows_trigger(exposed.mode, trigger) {
            return Err(SyncRunTriggerFailure::ModeDisallowed);
        }
        let config = *config;
        let accepted_at = self
            .runtime
            .ports()
            .clock()
            .now()
            .map_err(|_| SyncRunTriggerFailure::Unavailable)?;
        let run_id = RunId::new(uuid::Uuid::new_v4());
        let queued = self
            .runtime
            .queue_sync_run(
                admitted_workspace,
                &exposed,
                run_id,
                accepted_at.clone(),
                trigger,
                &mutation,
            )
            .map_err(|_| SyncRunTriggerFailure::ActiveRun)?;
        publish_status(
            self.runtime.as_ref(),
            queued.attempting,
            revision.clone(),
            Nullable::value(run_id),
        );

        let (settlement_state, settlement) = SyncRunSettlement::channel();
        let background_runtime = Arc::downgrade(&self.runtime);
        let executor = self.executor.clone();
        let fallback_completed_at = accepted_at.clone();
        let drop_state = Arc::new(SyncBackgroundTaskDropState {
            trigger_active: AtomicBool::new(true),
            dropped: AtomicBool::new(false),
            settlement: settlement_state.clone(),
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
            let provider = config.provider();
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
            let completion = match result {
                Ok(Ok(summary)) => SyncRunCompletion::Succeeded(summary),
                Ok(Err(error)) => error.into_completion(provider, run_id),
                Err(()) => SyncRunCompletion::UnknownFailure,
            };
            let terminal = runtime
                .finalize_running_sync_run(run_id, completion, completed_at, &mutation)
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
            let terminal = match self.runtime.fail_queued_sync_spawn(run_id, &mutation) {
                Ok(terminal) => terminal,
                Err(_) => {
                    settlement_state.settle();
                    return Err(SyncRunTriggerFailure::Unavailable);
                }
            };
            drop(mutation);
            self.runtime.publish_sync_terminal(&terminal);
            let finished = self.runtime.finish_sync_terminal(run_id);
            settlement_state.settle();
            finished.map_err(|_| SyncRunTriggerFailure::Unavailable)?;
            return Err(SyncRunTriggerFailure::Unavailable);
        }
        drop(mutation);
        Ok(StartedSyncRun {
            accepted: SyncRunAcceptedDto {
                run_id,
                accepted_at,
                config_revision: revision,
            },
            settlement,
        })
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
        self.start_sync_run(
            SyncTrigger::Manual,
            Some(request.expected_config_revision),
            mutation,
        )
        .map(|run| run.accepted)
        .map_err(api_trigger_failure)
    }
}

const fn sync_mode_allows_trigger(mode: SyncMode, trigger: SyncTrigger) -> bool {
    match mode {
        SyncMode::Automatic => true,
        SyncMode::StartupExit => matches!(
            trigger,
            SyncTrigger::AppLaunch | SyncTrigger::Manual | SyncTrigger::SettingsExit
        ),
        SyncMode::FullyManual => matches!(trigger, SyncTrigger::Manual),
    }
}

fn kernel_trigger_rejection(error: SyncRunTriggerFailure) -> KernelSyncTriggerRejection {
    match error {
        SyncRunTriggerFailure::ActiveRun => KernelSyncTriggerRejection::ActiveRun,
        SyncRunTriggerFailure::Disabled => KernelSyncTriggerRejection::Disabled,
        SyncRunTriggerFailure::Incomplete => KernelSyncTriggerRejection::Incomplete,
        SyncRunTriggerFailure::ModeDisallowed => KernelSyncTriggerRejection::ModeDisallowed,
        SyncRunTriggerFailure::NotReady
        | SyncRunTriggerFailure::RevisionConflict(_)
        | SyncRunTriggerFailure::Unavailable => KernelSyncTriggerRejection::Unavailable,
    }
}

fn api_trigger_failure(error: SyncRunTriggerFailure) -> ServiceFailure {
    match error {
        SyncRunTriggerFailure::RevisionConflict(current_revision) => {
            revision_conflict(current_revision)
        }
        SyncRunTriggerFailure::Disabled
        | SyncRunTriggerFailure::Incomplete
        | SyncRunTriggerFailure::NotReady => failure(ErrorCode::SyncNotReady),
        SyncRunTriggerFailure::ActiveRun
        | SyncRunTriggerFailure::ModeDisallowed
        | SyncRunTriggerFailure::Unavailable => failure(ErrorCode::SyncRunUnavailable),
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
