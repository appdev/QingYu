use std::collections::VecDeque;
use std::future::{poll_fn, Future as _};
use std::panic::AssertUnwindSafe;
use std::pin::Pin;
use std::process::{Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc, Arc, Condvar, Mutex, Weak,
};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use axum::{
    body::{to_bytes, Body},
    http::{header, Request, StatusCode},
};
use futures::FutureExt as _;
use qingyu_kernel::{
    api::{build_router, TransportPolicy},
    config::KernelConfig,
    contract::{
        ApiErrorEnvelope, BindSyncRepositoryRequest, CredentialChange, DomainEvent, ErrorCode,
        ErrorDetails, PatchSyncConfigRequest, ResourceRefDto, Revision, Rfc3339Utc, RunId,
        SafeUnsignedInteger, SyncCompletionState, SyncConfigChangesDto, SyncConfigReadiness,
        SyncIntervalSeconds, SyncProvider, SyncRunCompletionState, SyncSafeErrorCategory,
        SyncSafeErrorCode, SyncSafeErrorDto, SyncSafeErrorOperation, SyncSummaryDto, SyncTrigger,
        TriggerSyncRunRequest, WorkspaceDto,
    },
    events::{EventPublication, EventSink, EventSinkError},
    paths::KernelPaths,
    ports::{
        BoxSleepFuture, BoxTaskFuture, Clock, CredentialSecret, CredentialSlot, CredentialStore,
        DiagnosticRecord, DiagnosticsSink, KernelPorts, NetworkReachability, PortError, Sleeper,
        TaskSpawner,
    },
    runtime::{KernelRuntime, KernelStartupErrorKind, SyncApiService},
    services::{
        sync::{
            KernelSyncSchedulerCloseTestHook, KernelSyncTriggerDisposition,
            KernelSyncTriggerRejection, SyncCancellation, SyncExecutionError, SyncExecutor,
            SyncRunContext, SyncService, SyncWorkspaceSnapshotIdentity,
        },
        sync_scheduler::KernelSyncScheduler,
        workspace::WorkspaceService,
    },
    storage::DurableFileStore,
    sync::config::{SyncConfig, SyncConfigLoad, SyncConfigStore, SyncConfigStoreErrorKind},
    sync::editing::{
        SyncApplyDisposition, SyncApplyExitReason, SyncApplyFailure, SyncApplyRequest,
        SyncApplySource, SyncApplyState, SyncApplySuccess, SyncEditingRegistry,
        SyncEditingRegistryErrorKind,
    },
    workspace::{
        managed::ManagedWorkspaceCollection,
        primary::{
            AtomicHostWorkspaceCommitError, AtomicHostWorkspaceTransaction,
            PreparedWorkspaceAuthorityBinding, PrimaryWorkspaceRepositoryBinding,
            PrimaryWorkspaceStore, PrimaryWorkspaceStoreError,
        },
    },
};
use sha2::Digest as _;
use tempfile::tempdir;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::{oneshot, Barrier, Notify},
};
use tower::ServiceExt as _;

const HOST: &str = "127.0.0.1:43125";
const ORIGIN: &str = "tauri://localhost";

#[derive(Default)]
struct RecordingExecutor;

#[async_trait]
impl SyncExecutor for RecordingExecutor {
    async fn test_connection(&self, _config: SyncConfig) -> Result<(), SyncExecutionError> {
        Ok(())
    }

    async fn run(
        &self,
        _config: SyncConfig,
        _context: SyncRunContext,
    ) -> Result<SyncSummaryDto, SyncExecutionError> {
        Ok(SyncSummaryDto::empty())
    }
}

#[derive(Default)]
struct CountingExecutor {
    connection_tests: AtomicUsize,
    runs: AtomicUsize,
}

#[derive(Default)]
struct MemoryPrimaryWorkspaceStore {
    binding: PrimaryWorkspaceRepositoryBinding,
    block_next_save: Mutex<Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>>,
    notify_next_save: AtomicBool,
    save_started: Notify,
    value: Mutex<Option<serde_json::Value>>,
    failing_saves: AtomicUsize,
}

impl MemoryPrimaryWorkspaceStore {
    fn fail_next_saves(&self, count: usize) {
        self.failing_saves.store(count, Ordering::SeqCst);
    }

    fn block_next_save(&self) -> (mpsc::Receiver<()>, mpsc::Sender<()>) {
        let (started_sender, started_receiver) = mpsc::channel();
        let (release_sender, release_receiver) = mpsc::channel();
        *self.block_next_save.lock().unwrap() = Some((started_sender, release_receiver));
        (started_receiver, release_sender)
    }

    fn observe_next_save(&self) {
        self.notify_next_save.store(true, Ordering::SeqCst);
    }

    async fn wait_for_observed_save(&self) {
        self.save_started.notified().await;
    }
}

impl PrimaryWorkspaceStore for MemoryPrimaryWorkspaceStore {
    fn repository_binding(&self) -> PrimaryWorkspaceRepositoryBinding {
        self.binding.clone()
    }

    fn load(&self) -> Result<Option<serde_json::Value>, PrimaryWorkspaceStoreError> {
        Ok(self.value.lock().unwrap().clone())
    }

    fn replace(&self, value: Option<serde_json::Value>) -> Result<(), PrimaryWorkspaceStoreError> {
        *self.value.lock().unwrap() = value;
        Ok(())
    }

    fn save(&self) -> Result<(), PrimaryWorkspaceStoreError> {
        if self.notify_next_save.swap(false, Ordering::SeqCst) {
            self.save_started.notify_one();
        }
        if let Some((started, release)) = self.block_next_save.lock().unwrap().take() {
            started.send(()).unwrap();
            release.recv().unwrap();
        }
        let failed = self
            .failing_saves
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                if remaining > 0 {
                    Some(remaining - 1)
                } else {
                    None
                }
            })
            .is_ok();
        if failed {
            Err(PrimaryWorkspaceStoreError::unavailable())
        } else {
            Ok(())
        }
    }
}

struct MemoryHostWorkspaceTransaction {
    store: Arc<MemoryPrimaryWorkspaceStore>,
    repository_binding: PrimaryWorkspaceRepositoryBinding,
    authority_binding: PreparedWorkspaceAuthorityBinding,
}

struct NoCommitHostWorkspaceTransaction {
    repository_binding: PrimaryWorkspaceRepositoryBinding,
    authority_binding: PreparedWorkspaceAuthorityBinding,
}

impl AtomicHostWorkspaceTransaction for NoCommitHostWorkspaceTransaction {
    fn repository_binding(&self) -> PrimaryWorkspaceRepositoryBinding {
        self.repository_binding.clone()
    }

    fn authority_binding(&self) -> PreparedWorkspaceAuthorityBinding {
        self.authority_binding.clone()
    }

    fn compare_and_commit(
        self: Box<Self>,
        _expected_kernel_value: Option<&serde_json::Value>,
        _next_kernel_value: serde_json::Value,
    ) -> Result<(), AtomicHostWorkspaceCommitError> {
        Err(AtomicHostWorkspaceCommitError::no_commit())
    }
}

impl AtomicHostWorkspaceTransaction for MemoryHostWorkspaceTransaction {
    fn repository_binding(&self) -> PrimaryWorkspaceRepositoryBinding {
        self.repository_binding.clone()
    }

    fn authority_binding(&self) -> PreparedWorkspaceAuthorityBinding {
        self.authority_binding.clone()
    }

    fn compare_and_commit(
        self: Box<Self>,
        expected_kernel_value: Option<&serde_json::Value>,
        next_kernel_value: serde_json::Value,
    ) -> Result<(), AtomicHostWorkspaceCommitError> {
        if self.store.load().ok().as_ref().and_then(Option::as_ref) != expected_kernel_value {
            return Err(AtomicHostWorkspaceCommitError::conflict());
        }
        self.store
            .replace(Some(next_kernel_value))
            .map_err(|_| AtomicHostWorkspaceCommitError::outcome_unknown())?;
        self.store
            .save()
            .map_err(|_| AtomicHostWorkspaceCommitError::outcome_unknown())
    }
}

struct InstalledWorkspace {
    service: Arc<WorkspaceService>,
    store: Arc<MemoryPrimaryWorkspaceStore>,
}

async fn install_active_workspace(
    runtime: &Arc<KernelRuntime>,
    managed: ManagedWorkspaceCollection,
) -> InstalledWorkspace {
    let store = Arc::new(MemoryPrimaryWorkspaceStore::default());
    let service = Arc::new(
        WorkspaceService::new(
            runtime,
            store.clone(),
            managed,
            runtime.clone(),
            "Workspace",
        )
        .await
        .unwrap(),
    );
    InstalledWorkspace { service, store }
}

async fn enter_workspace_recovery(
    runtime: &Arc<KernelRuntime>,
    workspace: &InstalledWorkspace,
    target: &std::path::Path,
) {
    std::fs::create_dir(target).unwrap();
    let before = workspace.service.current().unwrap();
    let prepared = runtime.prepare_host_workspace_authority(target).unwrap();
    workspace.store.fail_next_saves(2);
    workspace
        .service
        .compare_and_set_host_workspace(&before.revision, prepared, "Recovery Target")
        .await
        .unwrap_err();
    assert!(runtime.active_workspace_snapshot().is_err());
}

async fn switch_workspace(
    runtime: &Arc<KernelRuntime>,
    workspace: &InstalledWorkspace,
    target: &std::path::Path,
) {
    std::fs::create_dir(target).unwrap();
    let before = workspace.service.current().unwrap();
    let prepared = runtime.prepare_host_workspace_authority(target).unwrap();
    workspace
        .service
        .compare_and_set_host_workspace(&before.revision, prepared, "Next Workspace")
        .await
        .unwrap();
}

#[async_trait]
impl SyncExecutor for CountingExecutor {
    async fn test_connection(&self, _config: SyncConfig) -> Result<(), SyncExecutionError> {
        self.connection_tests.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    async fn run(
        &self,
        _config: SyncConfig,
        _context: SyncRunContext,
    ) -> Result<SyncSummaryDto, SyncExecutionError> {
        self.runs.fetch_add(1, Ordering::Relaxed);
        Ok(SyncSummaryDto::empty())
    }
}

#[derive(Default)]
struct ManualExecutor {
    completed: Notify,
    fail: AtomicBool,
    runs: AtomicUsize,
}

#[derive(Default)]
struct BlockingExecutor {
    release: Notify,
    runs: AtomicUsize,
    started: Notify,
}

#[derive(Default)]
struct BlockingConnectionExecutor {
    connection_started: Notify,
    release_connection: Notify,
}

#[async_trait]
impl SyncExecutor for BlockingConnectionExecutor {
    async fn test_connection(&self, _config: SyncConfig) -> Result<(), SyncExecutionError> {
        self.connection_started.notify_one();
        self.release_connection.notified().await;
        Ok(())
    }

    async fn run(
        &self,
        _config: SyncConfig,
        _context: SyncRunContext,
    ) -> Result<SyncSummaryDto, SyncExecutionError> {
        Ok(SyncSummaryDto::empty())
    }
}

#[async_trait]
impl SyncExecutor for BlockingExecutor {
    async fn test_connection(&self, _config: SyncConfig) -> Result<(), SyncExecutionError> {
        Ok(())
    }

    async fn run(
        &self,
        _config: SyncConfig,
        _context: SyncRunContext,
    ) -> Result<SyncSummaryDto, SyncExecutionError> {
        self.runs.fetch_add(1, Ordering::Relaxed);
        self.started.notify_one();
        self.release.notified().await;
        Ok(SyncSummaryDto::empty())
    }
}

#[async_trait]
impl SyncExecutor for ManualExecutor {
    async fn test_connection(&self, _config: SyncConfig) -> Result<(), SyncExecutionError> {
        Ok(())
    }

    async fn run(
        &self,
        config: SyncConfig,
        _context: SyncRunContext,
    ) -> Result<SyncSummaryDto, SyncExecutionError> {
        self.runs.fetch_add(1, Ordering::Relaxed);
        self.completed.notify_one();
        if self.fail.load(Ordering::Relaxed) {
            Err(SyncExecutionError::unknown(config.provider()))
        } else {
            Ok(SyncSummaryDto::empty())
        }
    }
}

#[derive(Default)]
struct ClassifiedFailureExecutor {
    completed: Notify,
}

#[async_trait]
impl SyncExecutor for ClassifiedFailureExecutor {
    async fn test_connection(&self, _config: SyncConfig) -> Result<(), SyncExecutionError> {
        Ok(())
    }

    async fn run(
        &self,
        config: SyncConfig,
        _context: SyncRunContext,
    ) -> Result<SyncSummaryDto, SyncExecutionError> {
        self.completed.notify_one();
        let mut partial = SyncSummaryDto::empty();
        partial.scanned_files = SafeUnsignedInteger::new(2).unwrap();
        partial.uploaded_files = SafeUnsignedInteger::new(1).unwrap();
        Err(SyncExecutionError::new(
            SyncSafeErrorDto::new(
                config.provider(),
                SyncSafeErrorOperation::UploadObject,
                SyncSafeErrorCode::RemoteUnavailable,
            )
            .with_category(SyncSafeErrorCategory::Network),
        )
        .with_partial_summary(partial))
    }
}

struct ContextBindingExecutor {
    observed: Mutex<
        Vec<(
            WorkspaceDto,
            SyncWorkspaceSnapshotIdentity,
            RunId,
            SyncTrigger,
        )>,
    >,
    release: Notify,
    started: Notify,
}

impl Default for ContextBindingExecutor {
    fn default() -> Self {
        Self {
            observed: Mutex::new(Vec::new()),
            release: Notify::new(),
            started: Notify::new(),
        }
    }
}

#[async_trait]
impl SyncExecutor for ContextBindingExecutor {
    async fn test_connection(&self, _config: SyncConfig) -> Result<(), SyncExecutionError> {
        Ok(())
    }

    async fn run(
        &self,
        _config: SyncConfig,
        context: SyncRunContext,
    ) -> Result<SyncSummaryDto, SyncExecutionError> {
        self.observed.lock().unwrap().push((
            context.workspace().clone(),
            context.snapshot_identity(),
            context.run_id(),
            context.trigger(),
        ));
        self.started.notify_one();
        self.release.notified().await;
        Ok(SyncSummaryDto::empty())
    }
}

#[derive(Default)]
struct CancellationAwareExecutor {
    started: Notify,
}

#[async_trait]
impl SyncExecutor for CancellationAwareExecutor {
    async fn test_connection(&self, _config: SyncConfig) -> Result<(), SyncExecutionError> {
        Ok(())
    }

    async fn run(
        &self,
        _config: SyncConfig,
        context: SyncRunContext,
    ) -> Result<SyncSummaryDto, SyncExecutionError> {
        self.started.notify_one();
        context.cancellation().cancelled().await;
        Ok(SyncSummaryDto::empty())
    }
}

struct CancellationContextExecutor {
    sender: Mutex<Option<tokio::sync::oneshot::Sender<SyncCancellation>>>,
    release: Notify,
}

#[derive(Default)]
struct GatedCancellationExecutor {
    cancellation_seen: Notify,
    release: Notify,
    started: Notify,
}

#[derive(Default)]
struct PanicOnceExecutor {
    runs: AtomicUsize,
}

#[derive(Default)]
struct DropRetainingExecutor {
    future_dropped: AtomicBool,
    future_dropped_notification: Notify,
}

impl DropRetainingExecutor {
    async fn wait_for_future_drop(&self) {
        loop {
            if self.future_dropped.load(Ordering::Acquire) {
                return;
            }
            let notified = self.future_dropped_notification.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.future_dropped.load(Ordering::Acquire) {
                return;
            }
            notified.await;
        }
    }
}

struct ReadyRunFuture<'a> {
    executor: &'a DropRetainingExecutor,
    _context: Option<SyncRunContext>,
}

impl std::future::Future for ReadyRunFuture<'_> {
    type Output = Result<SyncSummaryDto, SyncExecutionError>;

    fn poll(
        self: Pin<&mut Self>,
        _context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        std::task::Poll::Ready(Ok(SyncSummaryDto::empty()))
    }
}

impl Drop for ReadyRunFuture<'_> {
    fn drop(&mut self) {
        self.executor.future_dropped.store(true, Ordering::Release);
        self.executor.future_dropped_notification.notify_waiters();
    }
}

impl SyncExecutor for DropRetainingExecutor {
    fn test_connection<'life0, 'async_trait>(
        &'life0 self,
        _config: SyncConfig,
    ) -> Pin<
        Box<dyn std::future::Future<Output = Result<(), SyncExecutionError>> + Send + 'async_trait>,
    >
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async { Ok(()) })
    }

    fn run<'life0, 'async_trait>(
        &'life0 self,
        _config: SyncConfig,
        context: SyncRunContext,
    ) -> Pin<
        Box<
            dyn std::future::Future<Output = Result<SyncSummaryDto, SyncExecutionError>>
                + Send
                + 'async_trait,
        >,
    >
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(ReadyRunFuture {
            executor: self,
            _context: Some(context),
        })
    }
}

#[async_trait]
impl SyncExecutor for PanicOnceExecutor {
    async fn test_connection(&self, _config: SyncConfig) -> Result<(), SyncExecutionError> {
        Ok(())
    }

    async fn run(
        &self,
        _config: SyncConfig,
        _context: SyncRunContext,
    ) -> Result<SyncSummaryDto, SyncExecutionError> {
        if self.runs.fetch_add(1, Ordering::SeqCst) == 0 {
            panic!("deterministic executor panic");
        }
        Ok(SyncSummaryDto::empty())
    }
}

#[async_trait]
impl SyncExecutor for GatedCancellationExecutor {
    async fn test_connection(&self, _config: SyncConfig) -> Result<(), SyncExecutionError> {
        Ok(())
    }

    async fn run(
        &self,
        _config: SyncConfig,
        context: SyncRunContext,
    ) -> Result<SyncSummaryDto, SyncExecutionError> {
        self.started.notify_one();
        context.cancellation().cancelled().await;
        self.cancellation_seen.notify_one();
        self.release.notified().await;
        Ok(SyncSummaryDto::empty())
    }
}

impl CancellationContextExecutor {
    fn new() -> (Arc<Self>, tokio::sync::oneshot::Receiver<SyncCancellation>) {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        (
            Arc::new(Self {
                sender: Mutex::new(Some(sender)),
                release: Notify::new(),
            }),
            receiver,
        )
    }
}

#[async_trait]
impl SyncExecutor for CancellationContextExecutor {
    async fn test_connection(&self, _config: SyncConfig) -> Result<(), SyncExecutionError> {
        Ok(())
    }

    async fn run(
        &self,
        _config: SyncConfig,
        context: SyncRunContext,
    ) -> Result<SyncSummaryDto, SyncExecutionError> {
        self.sender
            .lock()
            .unwrap()
            .take()
            .unwrap()
            .send(context.cancellation().clone())
            .unwrap();
        self.release.notified().await;
        Ok(SyncSummaryDto::empty())
    }
}

#[derive(Default)]
struct TestHost;

#[derive(Default)]
struct DeferredTaskSpawner {
    spawned: AtomicUsize,
    task: Mutex<Option<BoxTaskFuture>>,
}

#[derive(Default)]
struct CollectingDeferredTaskSpawner {
    spawned: AtomicUsize,
    tasks: Mutex<Vec<BoxTaskFuture>>,
}

impl CollectingDeferredTaskSpawner {
    fn take_only_task(&self) -> BoxTaskFuture {
        let mut tasks = self.tasks.lock().unwrap();
        assert_eq!(tasks.len(), 1, "exactly one background task is expected");
        tasks.pop().unwrap()
    }
}

impl TaskSpawner for CollectingDeferredTaskSpawner {
    fn spawn(&self, task: BoxTaskFuture) -> Result<(), PortError> {
        self.spawned.fetch_add(1, Ordering::SeqCst);
        self.tasks.lock().unwrap().push(task);
        Ok(())
    }
}

impl DeferredTaskSpawner {
    fn take_task(&self) -> BoxTaskFuture {
        self.task.lock().unwrap().take().expect("one deferred task")
    }
}

impl TaskSpawner for DeferredTaskSpawner {
    fn spawn(&self, task: BoxTaskFuture) -> Result<(), PortError> {
        self.spawned.fetch_add(1, Ordering::SeqCst);
        let mut pending = self.task.lock().unwrap();
        assert!(pending.is_none(), "only one deferred task is expected");
        *pending = Some(task);
        Ok(())
    }
}

#[derive(Default)]
struct FailingTaskSpawner {
    attempts: AtomicUsize,
}

impl TaskSpawner for FailingTaskSpawner {
    fn spawn(&self, _task: BoxTaskFuture) -> Result<(), PortError> {
        self.attempts.fetch_add(1, Ordering::SeqCst);
        Err(PortError::unavailable())
    }
}

#[derive(Default)]
struct DroppingSuccessfulTaskSpawner;

impl TaskSpawner for DroppingSuccessfulTaskSpawner {
    fn spawn(&self, task: BoxTaskFuture) -> Result<(), PortError> {
        drop(task);
        Ok(())
    }
}

#[derive(Default)]
struct ControlledSleeper {
    completed: Notify,
    registered: Notify,
    requests: Mutex<VecDeque<ControlledSleepRequest>>,
}

struct ControlledSleepRequest {
    duration: Duration,
    release: Option<oneshot::Sender<()>>,
}

impl ControlledSleepRequest {
    fn fire(mut self) -> bool {
        self.release
            .take()
            .expect("controlled sleep can fire once")
            .send(())
            .is_ok()
    }

    async fn wait_cancelled(mut self) {
        let release = self
            .release
            .as_mut()
            .expect("controlled sleep retains its release handle");
        release.closed().await;
        assert!(
            !self.fire(),
            "a cancelled controlled sleep must reject a late wake"
        );
    }
}

impl ControlledSleeper {
    async fn wait_for_completion(&self) {
        self.completed.notified().await;
    }

    async fn next_request(&self) -> ControlledSleepRequest {
        loop {
            if let Some(request) = self.requests.lock().unwrap().pop_front() {
                return request;
            }
            let registered = self.registered.notified();
            tokio::pin!(registered);
            registered.as_mut().enable();
            if let Some(request) = self.requests.lock().unwrap().pop_front() {
                return request;
            }
            registered.await;
        }
    }
}

impl Sleeper for ControlledSleeper {
    fn sleep(&self, duration: Duration) -> BoxSleepFuture<'_> {
        let (release, released) = oneshot::channel();
        self.requests
            .lock()
            .unwrap()
            .push_back(ControlledSleepRequest {
                duration,
                release: Some(release),
            });
        self.registered.notify_one();
        Box::pin(async move {
            released.await.map_err(|_| PortError::unavailable())?;
            self.completed.notify_one();
            Ok(())
        })
    }
}

#[derive(Default)]
struct TriggerRecordingExecutor {
    completed: Notify,
    triggers: Mutex<Vec<SyncTrigger>>,
}

#[async_trait]
impl SyncExecutor for TriggerRecordingExecutor {
    async fn test_connection(&self, _config: SyncConfig) -> Result<(), SyncExecutionError> {
        Ok(())
    }

    async fn run(
        &self,
        _config: SyncConfig,
        context: SyncRunContext,
    ) -> Result<SyncSummaryDto, SyncExecutionError> {
        self.triggers.lock().unwrap().push(context.trigger());
        self.completed.notify_one();
        Ok(SyncSummaryDto::empty())
    }
}

#[derive(Default)]
struct StatusPoisoningFailSpawner {
    runtime: Mutex<Weak<KernelRuntime>>,
}

impl StatusPoisoningFailSpawner {
    fn bind_runtime(&self, runtime: &Arc<KernelRuntime>) {
        *self.runtime.lock().unwrap() = Arc::downgrade(runtime);
    }
}

impl TaskSpawner for StatusPoisoningFailSpawner {
    fn spawn(&self, _task: BoxTaskFuture) -> Result<(), PortError> {
        self.runtime
            .lock()
            .unwrap()
            .upgrade()
            .expect("runtime must be bound before spawning")
            .poison_sync_status_for_test();
        Err(PortError::unavailable())
    }
}

async fn assert_sync_run_is_not_reported_drained(runtime: &KernelRuntime) {
    let mut drained = Box::pin(runtime.wait_for_empty_sync_run_for_test());
    poll_fn(|context| match drained.as_mut().poll(context) {
        std::task::Poll::Pending => std::task::Poll::Ready(()),
        std::task::Poll::Ready(_) => {
            panic!("a failed lifecycle transition must not synthesize an empty run")
        }
    })
    .await;
}

#[derive(Default)]
struct CompletionPublicationGate {
    completion_entered: Notify,
    released: (Mutex<bool>, Condvar),
}

impl CompletionPublicationGate {
    async fn wait_for_completion_publication(&self) {
        self.completion_entered.notified().await;
    }

    fn release_completion_publication(&self) {
        let (released, condition) = &self.released;
        let mut released = released.lock().unwrap();
        *released = true;
        condition.notify_all();
    }
}

impl EventSink for CompletionPublicationGate {
    fn publish(&self, publication: &EventPublication) -> Result<(), EventSinkError> {
        match &publication.event {
            DomainEvent::SyncStatusChanged { status }
                if matches!(
                    status.completion_state,
                    SyncCompletionState::Succeeded | SyncCompletionState::Failed
                ) =>
            {
                self.completion_entered.notify_one();
                let (released, condition) = &self.released;
                let mut released = released.lock().unwrap();
                while !*released {
                    released = condition.wait(released).unwrap();
                }
            }
            _ => {}
        }
        Ok(())
    }
}

#[derive(Default)]
struct SyncConfigPublicationGate {
    publication_entered: Notify,
    released: (Mutex<bool>, Condvar),
}

impl SyncConfigPublicationGate {
    async fn wait_for_sync_config_publication(&self) {
        self.publication_entered.notified().await;
    }

    fn release_sync_config_publication(&self) {
        let (released, condition) = &self.released;
        *released.lock().unwrap() = true;
        condition.notify_all();
    }
}

impl EventSink for SyncConfigPublicationGate {
    fn publish(&self, publication: &EventPublication) -> Result<(), EventSinkError> {
        if matches!(publication.event, DomainEvent::SyncConfigChanged { .. }) {
            self.publication_entered.notify_one();
            let (released, condition) = &self.released;
            let mut released = released.lock().unwrap();
            while !*released {
                released = condition.wait(released).unwrap();
            }
        }
        Ok(())
    }
}

#[derive(Default)]
struct WorkspacePublicationGate {
    publication_entered: Notify,
    released: (Mutex<bool>, Condvar),
}

impl WorkspacePublicationGate {
    async fn wait_for_workspace_publication(&self) {
        self.publication_entered.notified().await;
    }

    fn release_workspace_publication(&self) {
        let (released, condition) = &self.released;
        let mut released = released.lock().unwrap();
        *released = true;
        condition.notify_all();
    }
}

impl EventSink for WorkspacePublicationGate {
    fn publish(&self, publication: &EventPublication) -> Result<(), EventSinkError> {
        if matches!(publication.event, DomainEvent::WorkspaceChanged { .. }) {
            self.publication_entered.notify_one();
            let (released, condition) = &self.released;
            let mut released = released.lock().unwrap();
            while !*released {
                released = condition.wait(released).unwrap();
            }
        }
        Ok(())
    }
}

#[derive(Default)]
struct TerminalReentrantSink {
    mutation_available: AtomicBool,
    runtime: Mutex<Option<Weak<KernelRuntime>>>,
    terminal_seen: Notify,
    transition_rejected: AtomicBool,
}

impl TerminalReentrantSink {
    fn bind_runtime(&self, runtime: &Arc<KernelRuntime>) {
        *self.runtime.lock().unwrap() = Some(Arc::downgrade(runtime));
    }
}

impl EventSink for TerminalReentrantSink {
    fn publish(&self, publication: &EventPublication) -> Result<(), EventSinkError> {
        let DomainEvent::SyncStatusChanged { status } = &publication.event else {
            return Ok(());
        };
        if !matches!(
            status.completion_state,
            SyncCompletionState::Succeeded | SyncCompletionState::Failed
        ) {
            return Ok(());
        }
        let runtime = self
            .runtime
            .lock()
            .unwrap()
            .as_ref()
            .and_then(Weak::upgrade)
            .unwrap();
        self.mutation_available
            .store(runtime.mutation_is_available_for_test(), Ordering::SeqCst);
        self.transition_rejected.store(
            runtime
                .try_begin_sync_workspace_transition_for_test()
                .is_err(),
            Ordering::SeqCst,
        );
        self.terminal_seen.notify_one();
        Ok(())
    }
}

#[derive(Default)]
struct AttemptingReentrantCloseSink {
    close_returned: AtomicBool,
    service: Mutex<Option<Weak<SyncService>>>,
}

impl AttemptingReentrantCloseSink {
    fn bind_service(&self, service: &Arc<SyncService>) {
        *self.service.lock().unwrap() = Some(Arc::downgrade(service));
    }
}

impl EventSink for AttemptingReentrantCloseSink {
    fn publish(&self, publication: &EventPublication) -> Result<(), EventSinkError> {
        let DomainEvent::SyncStatusChanged { status } = &publication.event else {
            return Ok(());
        };
        if status.completion_state != SyncCompletionState::Attempting {
            return Ok(());
        }
        self.service
            .lock()
            .unwrap()
            .as_ref()
            .and_then(Weak::upgrade)
            .expect("sync service must be bound before triggering")
            .close_kernel_triggers();
        self.close_returned.store(true, Ordering::Release);
        Ok(())
    }
}

struct TerminalOrderingSink {
    order: Mutex<Vec<&'static str>>,
    released: (Mutex<bool>, Condvar),
    terminal_finished: Notify,
    terminal_started: Mutex<Option<mpsc::Sender<()>>>,
}

impl TerminalOrderingSink {
    fn new() -> (Arc<Self>, mpsc::Receiver<()>) {
        let (sender, receiver) = mpsc::channel();
        (
            Arc::new(Self {
                order: Mutex::new(Vec::new()),
                released: (Mutex::new(false), Condvar::new()),
                terminal_finished: Notify::new(),
                terminal_started: Mutex::new(Some(sender)),
            }),
            receiver,
        )
    }

    fn release_terminal(&self) {
        let (released, condition) = &self.released;
        *released.lock().unwrap() = true;
        condition.notify_all();
    }
}

impl EventSink for TerminalOrderingSink {
    fn publish(&self, publication: &EventPublication) -> Result<(), EventSinkError> {
        match &publication.event {
            DomainEvent::SyncStatusChanged { status }
                if matches!(
                    status.completion_state,
                    SyncCompletionState::Succeeded | SyncCompletionState::Failed
                ) =>
            {
                self.order.lock().unwrap().push("sync-terminal-start");
                if let Some(sender) = self.terminal_started.lock().unwrap().take() {
                    sender.send(()).unwrap();
                }
                let (released, condition) = &self.released;
                let mut released = released.lock().unwrap();
                while !*released {
                    released = condition.wait(released).unwrap();
                }
                self.order.lock().unwrap().push("sync-terminal-end");
                self.terminal_finished.notify_one();
            }
            DomainEvent::WorkspaceChanged { .. } => {
                self.order.lock().unwrap().push("workspace-changed");
            }
            _ => {}
        }
        Ok(())
    }
}

impl EventSink for TestHost {
    fn publish(&self, _publication: &EventPublication) -> Result<(), EventSinkError> {
        Ok(())
    }
}

impl Clock for TestHost {
    fn now(&self) -> Result<Rfc3339Utc, PortError> {
        Rfc3339Utc::parse("2026-07-29T00:00:00Z").map_err(|_| PortError::unavailable())
    }
}

impl Sleeper for TestHost {
    fn sleep(&self, _duration: Duration) -> BoxSleepFuture<'_> {
        Box::pin(async { Ok(()) })
    }
}

impl TaskSpawner for TestHost {
    fn spawn(&self, task: BoxTaskFuture) -> Result<(), PortError> {
        tokio::spawn(task);
        Ok(())
    }
}

impl CredentialStore for TestHost {
    fn is_present(&self, _slot: CredentialSlot) -> Result<bool, PortError> {
        Ok(false)
    }

    fn replace(&self, _slot: CredentialSlot, _value: &CredentialSecret) -> Result<(), PortError> {
        Ok(())
    }

    fn clear(&self, _slot: CredentialSlot) -> Result<(), PortError> {
        Ok(())
    }
}

impl DiagnosticsSink for TestHost {
    fn emit(&self, _record: DiagnosticRecord) -> Result<(), PortError> {
        Ok(())
    }
}

impl NetworkReachability for TestHost {
    fn is_reachable(&self) -> Result<bool, PortError> {
        Ok(true)
    }
}

fn test_ports() -> KernelPorts {
    let host = Arc::new(TestHost);
    KernelPorts::new(
        host.clone(),
        host.clone(),
        host.clone(),
        host.clone(),
        host.clone(),
        host.clone(),
        host,
    )
}

fn test_ports_with_event_sink(event_sink: Arc<dyn EventSink>) -> KernelPorts {
    let host = Arc::new(TestHost);
    KernelPorts::new(
        event_sink,
        host.clone(),
        host.clone(),
        host.clone(),
        host.clone(),
        host.clone(),
        host,
    )
}

fn test_ports_with_task_spawner(spawner: Arc<dyn TaskSpawner>) -> KernelPorts {
    let host = Arc::new(TestHost);
    KernelPorts::new(
        host.clone(),
        host.clone(),
        host.clone(),
        spawner,
        host.clone(),
        host.clone(),
        host,
    )
}

fn test_ports_with_sleeper(sleeper: Arc<dyn Sleeper>) -> KernelPorts {
    let host = Arc::new(TestHost);
    KernelPorts::new(
        host.clone(),
        host.clone(),
        sleeper,
        host.clone(),
        host.clone(),
        host.clone(),
        host,
    )
}

fn test_ports_with_event_sink_and_sleeper(
    event_sink: Arc<dyn EventSink>,
    sleeper: Arc<dyn Sleeper>,
) -> KernelPorts {
    let host = Arc::new(TestHost);
    KernelPorts::new(
        event_sink,
        host.clone(),
        sleeper,
        host.clone(),
        host.clone(),
        host.clone(),
        host,
    )
}

async fn active_sync_runtime(
    root: &std::path::Path,
    ports: KernelPorts,
) -> (Arc<KernelRuntime>, InstalledWorkspace, DurableFileStore) {
    let workspace = root.join("workspace");
    let app_data = root.join("app-data");
    let cache = root.join("cache");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&app_data).unwrap();
    std::fs::create_dir(&cache).unwrap();
    let mut config_value: serde_json::Value =
        serde_json::from_slice(&s3_config_bytes("https://s3.example.test")).unwrap();
    config_value["enabled"] = serde_json::json!(true);
    std::fs::write(
        app_data.join("sync-config.json"),
        serde_json::to_vec_pretty(&config_value).unwrap(),
    )
    .unwrap();
    let config = KernelConfig::generate().unwrap();
    let paths = KernelPaths::desktop(&workspace, &app_data, &cache).unwrap();
    let managed = ManagedWorkspaceCollection::from_paths(&paths).unwrap();
    let durable = DurableFileStore::at_config(paths.config_root(), config.launch_epoch()).unwrap();
    let runtime = KernelRuntime::activate(config, paths, ports).unwrap();
    let workspace = install_active_workspace(&runtime, managed).await;
    (runtime, workspace, durable)
}

struct RepositoryCatalogFixture {
    endpoint: String,
    requests: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl RepositoryCatalogFixture {
    async fn start(repository_id: &str, display_name: &str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let expected_path = format!("/qingyu/repositories/{repository_id}/metadata.json");
        let body = serde_json::to_vec(&serde_json::json!({
            "formatVersion": 1,
            "repositoryId": repository_id,
            "displayName": display_name,
            "createdAt": 1,
            "updatedAt": 1
        }))
        .unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let requests_for_task = requests.clone();
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0_u8; 1024];
            loop {
                let read = stream.read(&mut chunk).await.unwrap();
                assert_ne!(read, 0, "catalog request ended before its headers");
                request.extend_from_slice(&chunk[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            requests_for_task.fetch_add(1, Ordering::SeqCst);
            let request = String::from_utf8(request).unwrap();
            let request_target = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .expect("catalog request target");
            assert!(
                request_target.contains(&expected_path),
                "unexpected catalog target: {request_target}"
            );
            let headers = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(headers.as_bytes()).await.unwrap();
            stream.write_all(&body).await.unwrap();
            stream.shutdown().await.unwrap();
        });
        Self {
            endpoint,
            requests,
            task,
        }
    }

    async fn finish(self) {
        tokio::time::timeout(Duration::from_secs(2), self.task)
            .await
            .expect("catalog fixture timed out")
            .expect("catalog fixture failed");
        assert_eq!(self.requests.load(Ordering::SeqCst), 1);
    }
}

fn replace_fixture_s3_endpoint(root: &std::path::Path, endpoint: &str) {
    let config_path = root.join("app-data/sync-config.json");
    let mut config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&config_path).unwrap()).unwrap();
    config["s3"]["endpointUrl"] = serde_json::json!(endpoint);
    config["s3"]["addressingStyle"] = serde_json::json!("path");
    std::fs::write(config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
}

fn seed_local_sync_binding(root: &std::path::Path) -> Vec<u8> {
    let workspace = std::fs::canonicalize(root.join("workspace")).unwrap();
    let bytes = serde_json::to_vec_pretty(&serde_json::json!({
        "version": 1,
        "deviceId": "eb473600-dace-4d7e-bdad-7dac05933099",
        "repoKey": "BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc=",
        "bindings": [{
            "repositoryId": "f56c9192-414e-436b-bf3e-74648a434c54",
            "displayName": "QingYu",
            "notesRoot": workspace,
            "enabled": true
        }]
    }))
    .unwrap();
    std::fs::write(root.join("app-data/local-sync.json"), &bytes).unwrap();
    bytes
}

#[tokio::test]
async fn existing_v3_anonymous_webdav_config_remains_ready() {
    let temporary = tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    let app_data = temporary.path().join("app-data");
    let cache = temporary.path().join("cache");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&app_data).unwrap();
    std::fs::create_dir(&cache).unwrap();
    std::fs::write(
        app_data.join("sync-config.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "version": 3,
            "enabled": true,
            "provider": "webdav",
            "remoteRoot": "qingyu",
            "mode": "automatic",
            "intervalSeconds": 30,
            "generateConflictDocument": false,
            "webdav": {
                "serverUrl": "https://dav.example.test/root",
                "username": "",
                "password": ""
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
        .unwrap(),
    )
    .unwrap();
    let config = KernelConfig::generate().unwrap();
    let paths = KernelPaths::desktop(&workspace, &app_data, &cache).unwrap();
    let durable = DurableFileStore::at_config(paths.config_root(), config.launch_epoch()).unwrap();
    let runtime = KernelRuntime::activate(config, paths, KernelPorts::unavailable()).unwrap();
    let service = SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        Arc::new(RecordingExecutor),
    );

    let exposed = SyncApiService::get_sync_config(&service).await.unwrap();

    assert_eq!(exposed.readiness, SyncConfigReadiness::Ready);
    assert!(exposed.configured);
    assert!(exposed.issues.is_empty());
    assert!(!exposed.webdav.password.present);
}

#[tokio::test]
async fn disabled_v3_config_with_unrepresentable_bounds_is_rejected_without_rewrite() {
    let temporary = tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    let app_data = temporary.path().join("app-data");
    let cache = temporary.path().join("cache");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&app_data).unwrap();
    std::fs::create_dir(&cache).unwrap();
    let bytes = serde_json::to_vec_pretty(&serde_json::json!({
        "version": 3,
        "enabled": false,
        "provider": "s3",
        "remoteRoot": "qingyu",
        "mode": "automatic",
        "intervalSeconds": 1,
        "generateConflictDocument": false,
        "webdav": { "serverUrl": "", "username": "", "password": "" },
        "s3": {
            "endpointUrl": "",
            "region": "",
            "bucket": "",
            "accessKeyId": "",
            "secretAccessKey": "",
            "requestTimeoutSeconds": 999,
            "addressingStyle": "auto",
            "tlsVerification": "verify"
        }
    }))
    .unwrap();
    std::fs::write(app_data.join("sync-config.json"), &bytes).unwrap();
    let config = KernelConfig::generate().unwrap();
    let paths = KernelPaths::desktop(&workspace, &app_data, &cache).unwrap();
    let durable = DurableFileStore::at_config(paths.config_root(), config.launch_epoch()).unwrap();
    let runtime = KernelRuntime::activate(config, paths, KernelPorts::unavailable()).unwrap();
    let service = SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        Arc::new(RecordingExecutor),
    );

    let error = SyncApiService::get_sync_config(&service).await.unwrap_err();

    assert_eq!(error.code(), ErrorCode::SyncConfigInvalid);
    let patch_error = SyncApiService::patch_sync_config(
        &service,
        PatchSyncConfigRequest {
            expected_revision: Revision::parse(format!("{:x}", sha2::Sha256::digest(&bytes)))
                .unwrap(),
            changes: SyncConfigChangesDto {
                remote_root: Some("must-not-rewrite".to_string()),
                ..SyncConfigChangesDto::default()
            },
        },
    )
    .await
    .unwrap_err();
    assert_eq!(patch_error.code(), ErrorCode::SyncConfigInvalid);
    assert_eq!(
        std::fs::read(app_data.join("sync-config.json")).unwrap(),
        bytes
    );
}

#[test]
fn corrupt_config_recovery_preserves_exact_damaged_bytes_before_installing_v3() {
    let temporary = tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    let app_data = temporary.path().join("app-data");
    let cache = temporary.path().join("cache");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&app_data).unwrap();
    std::fs::create_dir(&cache).unwrap();
    let damaged = b"{not-json";
    std::fs::write(app_data.join("sync-config.json"), damaged).unwrap();
    let expected = Revision::parse(format!("{:x}", sha2::Sha256::digest(damaged))).unwrap();
    let config = KernelConfig::generate().unwrap();
    let paths = KernelPaths::desktop(&workspace, &app_data, &cache).unwrap();
    let durable = DurableFileStore::at_config(paths.config_root(), config.launch_epoch()).unwrap();
    let store = SyncConfigStore::new(durable).unwrap();

    let installed = store
        .recover_invalid(&expected, SyncConfig::default())
        .unwrap();

    assert_eq!(installed.as_str().len(), 64);
    assert!(matches!(
        store.load().unwrap(),
        SyncConfigLoad::Loaded { .. }
    ));
    let damaged_copies = std::fs::read_dir(&app_data)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("sync-config.damaged-")
        })
        .collect::<Vec<_>>();
    assert_eq!(damaged_copies.len(), 1);
    assert_eq!(std::fs::read(damaged_copies[0].path()).unwrap(), damaged);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;

        assert_eq!(
            std::fs::metadata(app_data.join("sync-config.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(damaged_copies[0].path())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn stale_recovery_does_not_replace_or_preserve_the_current_config() {
    let temporary = tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    let app_data = temporary.path().join("app-data");
    let cache = temporary.path().join("cache");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&app_data).unwrap();
    std::fs::create_dir(&cache).unwrap();
    let damaged = b"{current-invalid";
    std::fs::write(app_data.join("sync-config.json"), damaged).unwrap();
    let stale = Revision::parse("0".repeat(64)).unwrap();
    let config = KernelConfig::generate().unwrap();
    let paths = KernelPaths::desktop(&workspace, &app_data, &cache).unwrap();
    let durable = DurableFileStore::at_config(paths.config_root(), config.launch_epoch()).unwrap();
    let store = SyncConfigStore::new(durable).unwrap();

    let error = store
        .recover_invalid(&stale, SyncConfig::default())
        .unwrap_err();

    assert_eq!(error.kind(), SyncConfigStoreErrorKind::RevisionConflict);
    assert_eq!(
        std::fs::read(app_data.join("sync-config.json")).unwrap(),
        damaged
    );
    assert_eq!(std::fs::read_dir(&app_data).unwrap().count(), 1);
}

#[tokio::test]
async fn patch_applies_inline_credential_keep_replace_clear_without_secret_events() {
    let temporary = tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    let app_data = temporary.path().join("app-data");
    let cache = temporary.path().join("cache");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&app_data).unwrap();
    std::fs::create_dir(&cache).unwrap();
    std::fs::write(
        app_data.join("sync-config.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "version": 3,
            "enabled": false,
            "provider": "s3",
            "remoteRoot": "qingyu",
            "mode": "automatic",
            "intervalSeconds": 30,
            "generateConflictDocument": false,
            "webdav": {
                "serverUrl": "https://dav.example.test/root",
                "username": "alice",
                "password": "keep-webdav-password"
            },
            "s3": {
                "endpointUrl": "https://s3.example.test",
                "region": "us-east-1",
                "bucket": "notes",
                "accessKeyId": "old-access-key-id",
                "secretAccessKey": "old-secret-access-key",
                "requestTimeoutSeconds": 60,
                "addressingStyle": "auto",
                "tlsVerification": "verify"
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let config = KernelConfig::generate().unwrap();
    let paths = KernelPaths::desktop(&workspace, &app_data, &cache).unwrap();
    let durable = DurableFileStore::at_config(paths.config_root(), config.launch_epoch()).unwrap();
    let runtime = KernelRuntime::activate(config, paths, KernelPorts::unavailable()).unwrap();
    let mut events = runtime.event_broker().subscribe();
    let service = SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        Arc::new(RecordingExecutor),
    );
    let before = SyncApiService::get_sync_config(&service).await.unwrap();
    let request = PatchSyncConfigRequest {
        expected_revision: before.revision,
        changes: SyncConfigChangesDto {
            webdav_password: Some(CredentialChange::Keep {}),
            s3_access_key_id: Some(CredentialChange::Replace {
                value: "new-access-key-id".to_string(),
            }),
            s3_secret_access_key: Some(CredentialChange::Clear {}),
            ..SyncConfigChangesDto::default()
        },
    };
    let request_debug = format!("{request:?}");

    let after = SyncApiService::patch_sync_config(&service, request)
        .await
        .unwrap();
    let publication = events.recv().await.unwrap();

    assert!(after.webdav.password.present);
    assert!(after.s3.access_key_id.present);
    assert!(!after.s3.secret_access_key.present);
    let persisted: serde_json::Value =
        serde_json::from_slice(&std::fs::read(app_data.join("sync-config.json")).unwrap()).unwrap();
    assert_eq!(persisted["webdav"]["password"], "keep-webdav-password");
    assert_eq!(persisted["s3"]["accessKeyId"], "new-access-key-id");
    assert_eq!(persisted["s3"]["secretAccessKey"], "");
    assert!(matches!(
        publication.event,
        DomainEvent::SyncConfigChanged { .. }
    ));
    let event_json = serde_json::to_string(&publication.event).unwrap();
    for secret in [
        "keep-webdav-password",
        "old-access-key-id",
        "old-secret-access-key",
        "new-access-key-id",
    ] {
        assert!(!event_json.contains(secret));
        assert!(!request_debug.contains(secret));
    }
}

#[tokio::test]
async fn unsafe_legacy_endpoint_is_fully_redacted_from_public_config() {
    let temporary = tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    let app_data = temporary.path().join("app-data");
    let cache = temporary.path().join("cache");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&app_data).unwrap();
    std::fs::create_dir(&cache).unwrap();
    std::fs::write(
        app_data.join("sync-config.json"),
        s3_config_bytes("https://user:password@s3.example.test/root?token=secret#fragment"),
    )
    .unwrap();
    let config = KernelConfig::generate().unwrap();
    let paths = KernelPaths::desktop(&workspace, &app_data, &cache).unwrap();
    let durable = DurableFileStore::at_config(paths.config_root(), config.launch_epoch()).unwrap();
    let runtime = KernelRuntime::activate(config, paths, KernelPorts::unavailable()).unwrap();
    let service = SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        Arc::new(RecordingExecutor),
    );

    let exposed = SyncApiService::get_sync_config(&service).await.unwrap();

    assert!(exposed.s3.endpoint_url.redacted);
    assert!(exposed.s3.endpoint_url.value.as_ref().is_none());
    let serialized = serde_json::to_string(&exposed).unwrap();
    for sensitive in [
        "s3.example.test",
        "user:password",
        "token=secret",
        "#fragment",
    ] {
        assert!(!serialized.contains(sensitive));
    }
}

#[tokio::test]
async fn submitted_userinfo_query_and_fragment_endpoints_are_rejected_without_write() {
    for unsafe_endpoint in [
        "https://user:password@s3.example.test/root",
        "https://s3.example.test/root?token=secret",
        "https://s3.example.test/root#fragment",
    ] {
        let temporary = tempdir().unwrap();
        let workspace = temporary.path().join("workspace");
        let app_data = temporary.path().join("app-data");
        let cache = temporary.path().join("cache");
        std::fs::create_dir(&workspace).unwrap();
        std::fs::create_dir(&app_data).unwrap();
        std::fs::create_dir(&cache).unwrap();
        let original = s3_config_bytes("https://s3.example.test");
        std::fs::write(app_data.join("sync-config.json"), &original).unwrap();
        let config = KernelConfig::generate().unwrap();
        let paths = KernelPaths::desktop(&workspace, &app_data, &cache).unwrap();
        let durable =
            DurableFileStore::at_config(paths.config_root(), config.launch_epoch()).unwrap();
        let runtime = KernelRuntime::activate(config, paths, KernelPorts::unavailable()).unwrap();
        let service = SyncService::new(
            runtime.clone(),
            Arc::new(SyncConfigStore::new(durable).unwrap()),
            Arc::new(RecordingExecutor),
        );
        let before = SyncApiService::get_sync_config(&service).await.unwrap();

        let error = SyncApiService::patch_sync_config(
            &service,
            PatchSyncConfigRequest {
                expected_revision: before.revision,
                changes: SyncConfigChangesDto {
                    s3_endpoint_url: Some(unsafe_endpoint.to_string()),
                    ..SyncConfigChangesDto::default()
                },
            },
        )
        .await
        .unwrap_err();

        assert_eq!(error.code(), ErrorCode::SyncConfigInvalid);
        assert_eq!(
            std::fs::read(app_data.join("sync-config.json")).unwrap(),
            original
        );
        assert!(!format!("{error:?}").contains(unsafe_endpoint));
    }
}

#[tokio::test]
async fn stale_patch_returns_current_revision_without_writing() {
    let temporary = tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    let app_data = temporary.path().join("app-data");
    let cache = temporary.path().join("cache");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&app_data).unwrap();
    std::fs::create_dir(&cache).unwrap();
    let original = s3_config_bytes("https://s3.example.test");
    std::fs::write(app_data.join("sync-config.json"), &original).unwrap();
    let current_revision = format!("{:x}", sha2::Sha256::digest(&original));
    let config = KernelConfig::generate().unwrap();
    let paths = KernelPaths::desktop(&workspace, &app_data, &cache).unwrap();
    let durable = DurableFileStore::at_config(paths.config_root(), config.launch_epoch()).unwrap();
    let runtime = KernelRuntime::activate(config, paths, KernelPorts::unavailable()).unwrap();
    let service = SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        Arc::new(RecordingExecutor),
    );

    let error = SyncApiService::patch_sync_config(
        &service,
        PatchSyncConfigRequest {
            expected_revision: Revision::parse("0".repeat(64)).unwrap(),
            changes: SyncConfigChangesDto {
                remote_root: Some("stale-write".to_string()),
                ..SyncConfigChangesDto::default()
            },
        },
    )
    .await
    .unwrap_err();

    assert_eq!(error.code(), ErrorCode::SyncConfigRevisionConflict);
    assert!(matches!(
        error.details(),
        Some(ErrorDetails::RevisionConflict {
            current_revision: Some(revision)
        }) if revision.as_str() == current_revision
    ));
    assert_eq!(
        std::fs::read(app_data.join("sync-config.json")).unwrap(),
        original
    );
}

#[tokio::test]
async fn stale_repository_bind_revision_mutates_neither_local_state_nor_run_status() {
    let temporary = tempdir().unwrap();
    let (runtime, _workspace, durable) = active_sync_runtime(temporary.path(), test_ports()).await;
    let executor = Arc::new(CountingExecutor::default());
    let service = SyncService::new(
        runtime,
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        executor.clone(),
    );
    let current = SyncApiService::get_sync_config(&service).await.unwrap();
    let local_state = temporary.path().join("app-data/local-sync.json");
    assert!(!local_state.exists());

    let error = SyncApiService::bind_sync_repository(
        &service,
        BindSyncRepositoryRequest {
            display_name: "Shared notes".to_string(),
            expected_revision: Revision::parse("0".repeat(64)).unwrap(),
            repository_id: "323df833-764a-44b3-a534-492640c258f2".to_string(),
        },
    )
    .await
    .unwrap_err();

    assert_eq!(error.code(), ErrorCode::SyncConfigRevisionConflict);
    assert!(matches!(
        error.details(),
        Some(ErrorDetails::RevisionConflict {
            current_revision: Some(revision)
        }) if revision == &current.revision
    ));
    assert!(!local_state.exists());
    assert_eq!(executor.runs.load(Ordering::SeqCst), 0);
    let status = SyncApiService::get_sync_status(&service).await.unwrap();
    assert_eq!(status.completion_state, SyncCompletionState::Idle);
    assert!(status.active_run_id.as_ref().is_none());
}

#[tokio::test]
async fn current_repository_binding_reports_only_the_exact_active_workspace_repository() {
    const REPOSITORY_ID: &str = "5223e8c9-1346-4d59-8c22-12d68ce16fcf";
    const DISPLAY_NAME: &str = "Server notes";

    let temporary = tempdir().unwrap();
    let fixture = RepositoryCatalogFixture::start(REPOSITORY_ID, DISPLAY_NAME).await;
    let (runtime, _workspace, durable) = active_sync_runtime(temporary.path(), test_ports()).await;
    replace_fixture_s3_endpoint(temporary.path(), &fixture.endpoint);
    let service = SyncService::new(
        runtime,
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        Arc::new(CountingExecutor::default()),
    );
    let config = SyncApiService::get_sync_config(&service).await.unwrap();
    let config = SyncApiService::patch_sync_config(
        &service,
        PatchSyncConfigRequest {
            expected_revision: config.revision,
            changes: SyncConfigChangesDto {
                enabled: Some(false),
                ..SyncConfigChangesDto::default()
            },
        },
    )
    .await
    .expect("disable automatic sync while preserving one-shot recovery");
    assert_eq!(config.readiness, SyncConfigReadiness::Disabled);
    let unbound = SyncApiService::get_sync_repository_binding(&service)
        .await
        .expect("read unbound active workspace");
    assert!(unbound.repository_id.as_ref().is_none());
    seed_local_sync_binding(temporary.path());

    let before = SyncApiService::get_sync_repository_binding(&service)
        .await
        .expect("read original active binding");
    assert_eq!(
        before.repository_id.as_ref().map(String::as_str),
        Some("f56c9192-414e-436b-bf3e-74648a434c54")
    );

    SyncApiService::bind_sync_repository(
        &service,
        BindSyncRepositoryRequest {
            display_name: DISPLAY_NAME.to_string(),
            expected_revision: config.revision,
            repository_id: REPOSITORY_ID.to_string(),
        },
    )
    .await
    .expect("accepted exact repository recovery");
    fixture.finish().await;

    let after = SyncApiService::get_sync_repository_binding(&service)
        .await
        .expect("read rebound active binding");
    assert_eq!(
        after.repository_id.as_ref().map(String::as_str),
        Some(REPOSITORY_ID)
    );
    assert_eq!(
        serde_json::to_value(&after).unwrap(),
        serde_json::json!({ "repositoryId": REPOSITORY_ID })
    );
    assert!(!format!("{after:?}").contains("BwcHBwcH"));
}

#[tokio::test]
async fn closed_recovery_admission_rejects_repository_bind_before_local_state_mutation() {
    const REPOSITORY_ID: &str = "5223e8c9-1346-4d59-8c22-12d68ce16fcf";
    const DISPLAY_NAME: &str = "Server notes";

    let temporary = tempdir().unwrap();
    let fixture = RepositoryCatalogFixture::start(REPOSITORY_ID, DISPLAY_NAME).await;
    let (runtime, _workspace, durable) = active_sync_runtime(temporary.path(), test_ports()).await;
    replace_fixture_s3_endpoint(temporary.path(), &fixture.endpoint);
    let executor = Arc::new(CountingExecutor::default());
    let service = SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        executor.clone(),
    );
    let config = SyncApiService::get_sync_config(&service).await.unwrap();
    let local_state = temporary.path().join("app-data/local-sync.json");
    let original = seed_local_sync_binding(temporary.path());
    runtime.close_sync_admission_for_test().unwrap();

    let error = SyncApiService::bind_sync_repository(
        &service,
        BindSyncRepositoryRequest {
            display_name: DISPLAY_NAME.to_string(),
            expected_revision: config.revision,
            repository_id: REPOSITORY_ID.to_string(),
        },
    )
    .await
    .unwrap_err();

    fixture.finish().await;
    assert_eq!(error.code(), ErrorCode::SyncRunUnavailable);
    assert_eq!(
        std::fs::read(local_state).unwrap(),
        original,
        "rejected recovery admission must preserve the existing repository binding byte-for-byte"
    );
    assert_eq!(executor.runs.load(Ordering::SeqCst), 0);
    let status = SyncApiService::get_sync_status(&service).await.unwrap();
    assert_eq!(status.completion_state, SyncCompletionState::Idle);
    assert!(status.active_run_id.as_ref().is_none());
}

#[tokio::test]
async fn active_run_rejects_repository_bind_without_replacing_the_existing_binding() {
    const REPOSITORY_ID: &str = "5223e8c9-1346-4d59-8c22-12d68ce16fcf";
    const DISPLAY_NAME: &str = "Server notes";

    let temporary = tempdir().unwrap();
    let fixture = RepositoryCatalogFixture::start(REPOSITORY_ID, DISPLAY_NAME).await;
    let (runtime, _workspace, durable) = active_sync_runtime(temporary.path(), test_ports()).await;
    replace_fixture_s3_endpoint(temporary.path(), &fixture.endpoint);
    let executor = Arc::new(BlockingExecutor::default());
    let service = SyncService::new(
        runtime,
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        executor.clone(),
    );
    let config = SyncApiService::get_sync_config(&service).await.unwrap();
    let local_state = temporary.path().join("app-data/local-sync.json");
    let original = seed_local_sync_binding(temporary.path());
    SyncApiService::trigger_sync_run(
        &service,
        TriggerSyncRunRequest {
            expected_config_revision: config.revision.clone(),
        },
    )
    .await
    .unwrap();
    executor.started.notified().await;

    let error = SyncApiService::bind_sync_repository(
        &service,
        BindSyncRepositoryRequest {
            display_name: DISPLAY_NAME.to_string(),
            expected_revision: config.revision,
            repository_id: REPOSITORY_ID.to_string(),
        },
    )
    .await
    .unwrap_err();

    fixture.finish().await;
    assert_eq!(error.code(), ErrorCode::SyncRunUnavailable);
    assert_eq!(std::fs::read(local_state).unwrap(), original);
    assert_eq!(executor.runs.load(Ordering::SeqCst), 1);
    let attempting = SyncApiService::get_sync_status(&service).await.unwrap();
    assert_eq!(attempting.completion_state, SyncCompletionState::Attempting);
    executor.release.notify_one();
    runtime_wait_for_idle(&service).await;
}

#[tokio::test]
async fn repository_bind_spawn_failure_returns_one_accepted_terminal_job_and_one_binding() {
    const REPOSITORY_ID: &str = "5223e8c9-1346-4d59-8c22-12d68ce16fcf";
    const DISPLAY_NAME: &str = "Server notes";

    let temporary = tempdir().unwrap();
    let fixture = RepositoryCatalogFixture::start(REPOSITORY_ID, DISPLAY_NAME).await;
    let spawner = Arc::new(FailingTaskSpawner::default());
    let (runtime, _workspace, durable) = active_sync_runtime(
        temporary.path(),
        test_ports_with_task_spawner(spawner.clone()),
    )
    .await;
    replace_fixture_s3_endpoint(temporary.path(), &fixture.endpoint);
    let executor = Arc::new(CountingExecutor::default());
    let service = SyncService::new(
        runtime,
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        executor.clone(),
    );
    let config = SyncApiService::get_sync_config(&service).await.unwrap();
    seed_local_sync_binding(temporary.path());

    let binding = SyncApiService::bind_sync_repository(
        &service,
        BindSyncRepositoryRequest {
            display_name: DISPLAY_NAME.to_string(),
            expected_revision: config.revision,
            repository_id: REPOSITORY_ID.to_string(),
        },
    )
    .await
    .expect("a committed bind must return its accepted recovery job");

    fixture.finish().await;
    assert_eq!(binding.repository_id, REPOSITORY_ID);
    assert_eq!(spawner.attempts.load(Ordering::SeqCst), 1);
    assert_eq!(executor.runs.load(Ordering::SeqCst), 0);
    let persisted: serde_json::Value = serde_json::from_slice(
        &std::fs::read(temporary.path().join("app-data/local-sync.json")).unwrap(),
    )
    .unwrap();
    let bindings = persisted["bindings"].as_array().unwrap();
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0]["repositoryId"], REPOSITORY_ID);
    assert_eq!(bindings[0]["displayName"], DISPLAY_NAME);

    let run_id = RunId::new(uuid::Uuid::parse_str(&binding.job_id).unwrap());
    let exact = SyncApiService::get_sync_run(&service, run_id)
        .await
        .unwrap();
    assert_eq!(exact.completion_state, SyncRunCompletionState::Failed);
    assert_eq!(
        exact.error.as_ref().map(SyncSafeErrorDto::run_id),
        Some(Some(run_id))
    );
    let status = SyncApiService::get_sync_status(&service).await.unwrap();
    assert_eq!(status.completion_state, SyncCompletionState::Failed);
    assert!(status.active_run_id.as_ref().is_none());
}

#[tokio::test]
async fn patching_one_kernel_instance_does_not_change_another_instance() {
    let first = tempdir().unwrap();
    let first_workspace = first.path().join("workspace");
    let first_app_data = first.path().join("app-data");
    let first_cache = first.path().join("cache");
    let second = tempdir().unwrap();
    let second_workspace = second.path().join("workspace");
    let second_app_data = second.path().join("app-data");
    let second_cache = second.path().join("cache");
    for directory in [
        &first_workspace,
        &first_app_data,
        &first_cache,
        &second_workspace,
        &second_app_data,
        &second_cache,
    ] {
        std::fs::create_dir(directory).unwrap();
    }
    std::fs::write(
        first_app_data.join("sync-config.json"),
        s3_config_bytes("https://first.example.test"),
    )
    .unwrap();
    std::fs::write(
        second_app_data.join("sync-config.json"),
        s3_config_bytes("https://second.example.test"),
    )
    .unwrap();
    let first_config = KernelConfig::generate().unwrap();
    let first_paths =
        KernelPaths::desktop(&first_workspace, &first_app_data, &first_cache).unwrap();
    let first_durable =
        DurableFileStore::at_config(first_paths.config_root(), first_config.launch_epoch())
            .unwrap();
    let first_runtime =
        KernelRuntime::activate(first_config, first_paths, KernelPorts::unavailable()).unwrap();
    let first_service = SyncService::new(
        first_runtime.clone(),
        Arc::new(SyncConfigStore::new(first_durable).unwrap()),
        Arc::new(RecordingExecutor),
    );
    let second_config = KernelConfig::generate().unwrap();
    let second_paths =
        KernelPaths::desktop(&second_workspace, &second_app_data, &second_cache).unwrap();
    let second_durable =
        DurableFileStore::at_config(second_paths.config_root(), second_config.launch_epoch())
            .unwrap();
    let second_runtime =
        KernelRuntime::activate(second_config, second_paths, KernelPorts::unavailable()).unwrap();
    let second_service = SyncService::new(
        second_runtime.clone(),
        Arc::new(SyncConfigStore::new(second_durable).unwrap()),
        Arc::new(RecordingExecutor),
    );
    let first_before = SyncApiService::get_sync_config(&first_service)
        .await
        .unwrap();
    let second_before = SyncApiService::get_sync_config(&second_service)
        .await
        .unwrap();

    let first_after = SyncApiService::patch_sync_config(
        &first_service,
        PatchSyncConfigRequest {
            expected_revision: first_before.revision,
            changes: SyncConfigChangesDto {
                remote_root: Some("first-only".to_string()),
                ..SyncConfigChangesDto::default()
            },
        },
    )
    .await
    .unwrap();
    let second_after = SyncApiService::get_sync_config(&second_service)
        .await
        .unwrap();

    assert_eq!(first_after.remote_root, "first-only");
    assert_eq!(second_after, second_before);
}

#[tokio::test]
async fn corrupt_and_unsupported_configs_return_the_same_safe_api_error() {
    for bytes in [
        b"{not-json".to_vec(),
        serde_json::to_vec(&serde_json::json!({ "version": 99 })).unwrap(),
    ] {
        let temporary = tempdir().unwrap();
        let workspace = temporary.path().join("workspace");
        let app_data = temporary.path().join("app-data");
        let cache = temporary.path().join("cache");
        std::fs::create_dir(&workspace).unwrap();
        std::fs::create_dir(&app_data).unwrap();
        std::fs::create_dir(&cache).unwrap();
        std::fs::write(app_data.join("sync-config.json"), bytes).unwrap();
        let config = KernelConfig::generate().unwrap();
        let paths = KernelPaths::desktop(&workspace, &app_data, &cache).unwrap();
        let durable =
            DurableFileStore::at_config(paths.config_root(), config.launch_epoch()).unwrap();
        let runtime = KernelRuntime::activate(config, paths, KernelPorts::unavailable()).unwrap();
        let service = SyncService::new(
            runtime.clone(),
            Arc::new(SyncConfigStore::new(durable).unwrap()),
            Arc::new(RecordingExecutor),
        );

        let error = SyncApiService::get_sync_config(&service).await.unwrap_err();

        assert_eq!(error.code(), ErrorCode::SyncConfigInvalid);
        assert!(error.details().is_none());
        assert_eq!(format!("{error}"), "The sync configuration is invalid.");
    }
}

#[cfg(unix)]
#[tokio::test]
async fn symlinked_sync_config_is_rejected_without_reading_its_target() {
    let temporary = tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    let app_data = temporary.path().join("app-data");
    let cache = temporary.path().join("cache");
    let outside = temporary.path().join("outside.json");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&app_data).unwrap();
    std::fs::create_dir(&cache).unwrap();
    std::fs::write(&outside, s3_config_bytes("https://outside.example.test")).unwrap();
    std::os::unix::fs::symlink(&outside, app_data.join("sync-config.json")).unwrap();
    let config = KernelConfig::generate().unwrap();
    let paths = KernelPaths::desktop(&workspace, &app_data, &cache).unwrap();
    let durable = DurableFileStore::at_config(paths.config_root(), config.launch_epoch()).unwrap();
    let runtime = KernelRuntime::activate(config, paths, KernelPorts::unavailable()).unwrap();
    let service = SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        Arc::new(RecordingExecutor),
    );

    let error = SyncApiService::get_sync_config(&service).await.unwrap_err();

    assert_eq!(error.code(), ErrorCode::SyncConfigInvalid);
    assert!(std::fs::symlink_metadata(app_data.join("sync-config.json"))
        .unwrap()
        .file_type()
        .is_symlink());
}

#[tokio::test]
async fn connection_test_applies_ephemeral_changes_and_calls_executor_exactly_once() {
    let temporary = tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    let app_data = temporary.path().join("app-data");
    let cache = temporary.path().join("cache");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&app_data).unwrap();
    std::fs::create_dir(&cache).unwrap();
    let mut config_value: serde_json::Value =
        serde_json::from_slice(&s3_config_bytes("https://s3.example.test")).unwrap();
    config_value["enabled"] = serde_json::json!(true);
    let original = serde_json::to_vec_pretty(&config_value).unwrap();
    std::fs::write(app_data.join("sync-config.json"), &original).unwrap();
    let config = KernelConfig::generate().unwrap();
    let paths = KernelPaths::desktop(&workspace, &app_data, &cache).unwrap();
    let managed = ManagedWorkspaceCollection::from_paths(&paths).unwrap();
    let durable = DurableFileStore::at_config(paths.config_root(), config.launch_epoch()).unwrap();
    let runtime = KernelRuntime::activate(config, paths, KernelPorts::unavailable()).unwrap();
    let _workspace = install_active_workspace(&runtime, managed).await;
    let executor = Arc::new(CountingExecutor::default());
    let service = SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        executor.clone(),
    );
    let before = SyncApiService::get_sync_config(&service).await.unwrap();

    let tested = SyncApiService::test_sync_connection(
        &service,
        qingyu_kernel::contract::TestSyncConnectionRequest {
            expected_revision: before.revision.clone(),
            changes: SyncConfigChangesDto {
                remote_root: Some("ephemeral-target".to_string()),
                ..SyncConfigChangesDto::default()
            },
        },
    )
    .await
    .unwrap();

    assert_eq!(executor.connection_tests.load(Ordering::Relaxed), 1);
    assert_eq!(executor.runs.load(Ordering::Relaxed), 0);
    assert_eq!(tested.provider, qingyu_kernel::contract::SyncProvider::S3);
    assert_eq!(tested.checked_target, "ephemeral-target");
    assert_eq!(tested.config_revision, before.revision);
    assert_eq!(
        std::fs::read(app_data.join("sync-config.json")).unwrap(),
        original
    );
    assert_eq!(
        SyncApiService::get_sync_config(&service)
            .await
            .unwrap()
            .remote_root,
        "qingyu"
    );

    let stale = Revision::parse("0".repeat(64)).unwrap();
    let stale_connection = SyncApiService::test_sync_connection(
        &service,
        qingyu_kernel::contract::TestSyncConnectionRequest {
            expected_revision: stale.clone(),
            changes: SyncConfigChangesDto::default(),
        },
    )
    .await
    .unwrap_err();
    let stale_run = SyncApiService::trigger_sync_run(
        &service,
        TriggerSyncRunRequest {
            expected_config_revision: stale,
        },
    )
    .await
    .unwrap_err();
    assert_eq!(
        stale_connection.code(),
        ErrorCode::SyncConfigRevisionConflict
    );
    assert_eq!(stale_run.code(), ErrorCode::SyncConfigRevisionConflict);
    assert_eq!(executor.connection_tests.load(Ordering::Relaxed), 1);
    assert_eq!(executor.runs.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn initial_sync_status_is_revision_bound_and_contains_no_config_secrets() {
    let temporary = tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    let app_data = temporary.path().join("app-data");
    let cache = temporary.path().join("cache");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&app_data).unwrap();
    std::fs::create_dir(&cache).unwrap();
    let original = s3_config_bytes("https://private-s3.example.test");
    std::fs::write(app_data.join("sync-config.json"), original).unwrap();
    let config = KernelConfig::generate().unwrap();
    let paths = KernelPaths::desktop(&workspace, &app_data, &cache).unwrap();
    let durable = DurableFileStore::at_config(paths.config_root(), config.launch_epoch()).unwrap();
    let runtime = KernelRuntime::activate(config, paths, KernelPorts::unavailable()).unwrap();
    let service = SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        Arc::new(RecordingExecutor),
    );
    let exposed = SyncApiService::get_sync_config(&service).await.unwrap();

    let status = SyncApiService::get_sync_status(&service).await.unwrap();

    assert_eq!(status.completion_state, SyncCompletionState::Idle);
    assert_eq!(
        status.config_revision.as_ref().map(Revision::as_str),
        Some(exposed.revision.as_str())
    );
    assert!(status.active_run_id.as_ref().is_none());
    assert!(status.last_attempt_at.as_ref().is_none());
    assert!(status.last_successful_sync_at.as_ref().is_none());
    assert!(status.last_trigger.as_ref().is_none());
    assert!(status.summary.as_ref().is_none());
    assert!(status.error.as_ref().is_none());
    let serialized = serde_json::to_string(&status).unwrap();
    for secret in [
        "private-s3.example.test",
        "access-key-id",
        "secret-access-key",
    ] {
        assert!(!serialized.contains(secret));
    }
}

#[tokio::test]
async fn manual_run_is_spawned_once_and_completes_with_safe_status_events() {
    let temporary = tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    let app_data = temporary.path().join("app-data");
    let cache = temporary.path().join("cache");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&app_data).unwrap();
    std::fs::create_dir(&cache).unwrap();
    let mut config_value: serde_json::Value =
        serde_json::from_slice(&s3_config_bytes("https://private-s3.example.test")).unwrap();
    config_value["enabled"] = serde_json::json!(true);
    std::fs::write(
        app_data.join("sync-config.json"),
        serde_json::to_vec_pretty(&config_value).unwrap(),
    )
    .unwrap();
    let config = KernelConfig::generate().unwrap();
    let paths = KernelPaths::desktop(&workspace, &app_data, &cache).unwrap();
    let managed = ManagedWorkspaceCollection::from_paths(&paths).unwrap();
    let durable = DurableFileStore::at_config(paths.config_root(), config.launch_epoch()).unwrap();
    let runtime = KernelRuntime::activate(config, paths, test_ports()).unwrap();
    let _workspace = install_active_workspace(&runtime, managed).await;
    let mut events = runtime.event_broker().subscribe();
    let executor = Arc::new(ManualExecutor::default());
    let service = SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        executor.clone(),
    );
    let config = SyncApiService::get_sync_config(&service).await.unwrap();

    let accepted = SyncApiService::trigger_sync_run(
        &service,
        TriggerSyncRunRequest {
            expected_config_revision: config.revision.clone(),
        },
    )
    .await
    .unwrap();
    executor.completed.notified().await;
    let mut completed = None;
    for _attempt in 0..100 {
        let status = SyncApiService::get_sync_status(&service).await.unwrap();
        if status.completion_state == SyncCompletionState::Succeeded {
            completed = Some(status);
            break;
        }
        tokio::task::yield_now().await;
    }
    let completed = completed.expect("background sync should complete");
    let attempting_event = events.recv().await.unwrap();
    let completed_event = events.recv().await.unwrap();

    assert_eq!(executor.runs.load(Ordering::Relaxed), 1);
    assert_eq!(accepted.config_revision, config.revision);
    assert_eq!(accepted.accepted_at.as_str(), "2026-07-29T00:00:00Z");
    assert_eq!(completed.completion_state, SyncCompletionState::Succeeded);
    assert!(completed.active_run_id.as_ref().is_none());
    assert_eq!(completed.last_trigger.as_ref(), Some(&SyncTrigger::Manual));
    assert_eq!(
        completed
            .last_successful_sync_at
            .as_ref()
            .map(Rfc3339Utc::as_str),
        Some("2026-07-29T00:00:00Z")
    );
    assert_status_publication(&attempting_event, accepted.run_id, true, false);
    assert_status_publication(&completed_event, accepted.run_id, false, false);
    for publication in [&attempting_event, &completed_event] {
        assert!(matches!(
            publication.event,
            DomainEvent::SyncStatusChanged { .. }
        ));
        let serialized = serde_json::to_string(&publication.event).unwrap();
        for secret in [
            "private-s3.example.test",
            "access-key-id",
            "secret-access-key",
        ] {
            assert!(!serialized.contains(secret));
        }
    }

    executor.fail.store(true, Ordering::Relaxed);
    let failed_accepted = SyncApiService::trigger_sync_run(
        &service,
        TriggerSyncRunRequest {
            expected_config_revision: config.revision,
        },
    )
    .await
    .unwrap();
    executor.completed.notified().await;
    let mut failed = None;
    for _attempt in 0..100 {
        let status = SyncApiService::get_sync_status(&service).await.unwrap();
        if status.completion_state == SyncCompletionState::Failed {
            failed = Some(status);
            break;
        }
        tokio::task::yield_now().await;
    }
    let failed = failed.expect("failed background sync should update status");
    let failed_attempting_event = events.recv().await.unwrap();
    let failed_completed_event = events.recv().await.unwrap();

    assert_eq!(executor.runs.load(Ordering::Relaxed), 2);
    let safe_error = failed.error.as_ref().expect("safe failure details");
    assert_eq!(safe_error.code(), "unknown");
    assert_eq!(safe_error.operation(), "sync_run");
    assert_eq!(safe_error.run_id(), Some(failed_accepted.run_id));
    assert_status_publication(
        &failed_attempting_event,
        failed_accepted.run_id,
        true,
        false,
    );
    assert_status_publication(&failed_completed_event, failed_accepted.run_id, false, true);
    for publication in [&failed_attempting_event, &failed_completed_event] {
        let serialized = serde_json::to_string(&publication.event).unwrap();
        for secret in [
            "private-s3.example.test",
            "access-key-id",
            "secret-access-key",
        ] {
            assert!(!serialized.contains(secret));
        }
    }
}

#[tokio::test]
async fn executor_failure_preserves_partial_summary_and_classified_safe_error() {
    let temporary = tempdir().unwrap();
    let (runtime, _workspace, durable) = active_sync_runtime(temporary.path(), test_ports()).await;
    let executor = Arc::new(ClassifiedFailureExecutor::default());
    let service = SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        executor.clone(),
    );
    let config = SyncApiService::get_sync_config(&service).await.unwrap();

    let accepted = SyncApiService::trigger_sync_run(
        &service,
        TriggerSyncRunRequest {
            expected_config_revision: config.revision,
        },
    )
    .await
    .unwrap();
    executor.completed.notified().await;
    let mut terminal = None;
    for _attempt in 0..100 {
        let status = SyncApiService::get_sync_status(&service).await.unwrap();
        if status.completion_state == SyncCompletionState::Failed {
            terminal = Some(status);
            break;
        }
        tokio::task::yield_now().await;
    }
    let terminal = terminal.expect("classified failure should reach terminal status");

    let summary = terminal.summary.as_ref().expect("partial summary");
    assert_eq!(summary.scanned_files.get(), 2);
    assert_eq!(summary.uploaded_files.get(), 1);
    let error = terminal.error.as_ref().expect("safe error");
    assert_eq!(error.category(), Some("network"));
    assert_eq!(error.code(), "remote_unavailable");
    assert_eq!(error.operation(), "upload_object");
    assert_eq!(error.provider(), SyncProvider::S3);
    assert_eq!(error.run_id(), Some(accepted.run_id));
}

#[tokio::test]
async fn completed_status_resets_to_idle_when_config_revision_or_provider_changes() {
    for failed in [false, true] {
        let temporary = tempdir().unwrap();
        let workspace = temporary.path().join("workspace");
        let app_data = temporary.path().join("app-data");
        let cache = temporary.path().join("cache");
        std::fs::create_dir(&workspace).unwrap();
        std::fs::create_dir(&app_data).unwrap();
        std::fs::create_dir(&cache).unwrap();
        let mut config_value: serde_json::Value =
            serde_json::from_slice(&s3_config_bytes("https://s3.example.test")).unwrap();
        config_value["enabled"] = serde_json::json!(true);
        std::fs::write(
            app_data.join("sync-config.json"),
            serde_json::to_vec_pretty(&config_value).unwrap(),
        )
        .unwrap();
        let config = KernelConfig::generate().unwrap();
        let paths = KernelPaths::desktop(&workspace, &app_data, &cache).unwrap();
        let managed = ManagedWorkspaceCollection::from_paths(&paths).unwrap();
        let durable =
            DurableFileStore::at_config(paths.config_root(), config.launch_epoch()).unwrap();
        let runtime = KernelRuntime::activate(config, paths, test_ports()).unwrap();
        let _workspace = install_active_workspace(&runtime, managed).await;
        let executor = Arc::new(ManualExecutor::default());
        executor.fail.store(failed, Ordering::Relaxed);
        let service = SyncService::new(
            runtime.clone(),
            Arc::new(SyncConfigStore::new(durable).unwrap()),
            executor.clone(),
        );
        let original = SyncApiService::get_sync_config(&service).await.unwrap();

        SyncApiService::trigger_sync_run(
            &service,
            TriggerSyncRunRequest {
                expected_config_revision: original.revision.clone(),
            },
        )
        .await
        .unwrap();
        executor.completed.notified().await;
        let expected_completion = if failed {
            SyncCompletionState::Failed
        } else {
            SyncCompletionState::Succeeded
        };
        for _attempt in 0..100 {
            let status = SyncApiService::get_sync_status(&service).await.unwrap();
            if status.completion_state == expected_completion {
                break;
            }
            tokio::task::yield_now().await;
        }
        let completed = SyncApiService::get_sync_status(&service).await.unwrap();
        assert_eq!(completed.completion_state, expected_completion);
        assert_eq!(completed.provider, SyncProvider::S3);
        assert_eq!(completed.config_revision.as_ref(), Some(&original.revision));
        assert_eq!(completed.error.as_ref().is_some(), failed);

        let patched = SyncApiService::patch_sync_config(
            &service,
            PatchSyncConfigRequest {
                expected_revision: original.revision,
                changes: SyncConfigChangesDto {
                    provider: Some(SyncProvider::Webdav),
                    ..SyncConfigChangesDto::default()
                },
            },
        )
        .await
        .unwrap();
        let status = SyncApiService::get_sync_status(&service).await.unwrap();

        assert_eq!(status.completion_state, SyncCompletionState::Idle);
        assert_eq!(status.provider, SyncProvider::Webdav);
        assert_eq!(status.config_revision.as_ref(), Some(&patched.revision));
        assert!(status.active_run_id.as_ref().is_none());
        assert!(status.last_attempt_at.as_ref().is_none());
        assert!(status.last_successful_sync_at.as_ref().is_none());
        assert!(status.last_trigger.as_ref().is_none());
        assert!(status.summary.as_ref().is_none());
        assert!(status.error.as_ref().is_none());
    }
}

fn assert_status_publication(
    publication: &EventPublication,
    expected_run_id: RunId,
    expected_active: bool,
    expected_error: bool,
) {
    let resource_run_id = match &publication.resource {
        ResourceRefDto::SyncStatus { run_id } => run_id
            .as_ref()
            .copied()
            .expect("status event resource run id"),
        _ => panic!("sync status resource expected"),
    };
    let status = match &publication.event {
        DomainEvent::SyncStatusChanged { status } => status,
        _ => panic!("sync status event expected"),
    };
    assert_eq!(resource_run_id, expected_run_id);
    assert_eq!(
        status.active_run_id.as_ref().copied(),
        expected_active.then_some(expected_run_id)
    );
    assert_eq!(status.config_revision.as_ref(), Some(&publication.revision));
    if expected_error {
        assert_eq!(
            status
                .error
                .as_ref()
                .and_then(qingyu_kernel::contract::SyncSafeErrorDto::run_id),
            Some(resource_run_id)
        );
    } else {
        assert!(status.error.as_ref().is_none());
    }
}

#[tokio::test]
async fn active_manual_run_rejects_duplicate_without_spawning_a_second_executor() {
    let temporary = tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    let app_data = temporary.path().join("app-data");
    let cache = temporary.path().join("cache");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&app_data).unwrap();
    std::fs::create_dir(&cache).unwrap();
    let mut config_value: serde_json::Value =
        serde_json::from_slice(&s3_config_bytes("https://s3.example.test")).unwrap();
    config_value["enabled"] = serde_json::json!(true);
    std::fs::write(
        app_data.join("sync-config.json"),
        serde_json::to_vec_pretty(&config_value).unwrap(),
    )
    .unwrap();
    let config = KernelConfig::generate().unwrap();
    let paths = KernelPaths::desktop(&workspace, &app_data, &cache).unwrap();
    let managed = ManagedWorkspaceCollection::from_paths(&paths).unwrap();
    let durable = DurableFileStore::at_config(paths.config_root(), config.launch_epoch()).unwrap();
    let runtime = KernelRuntime::activate(config, paths, test_ports()).unwrap();
    let _workspace = install_active_workspace(&runtime, managed).await;
    let executor = Arc::new(BlockingExecutor::default());
    let service = SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        executor.clone(),
    );
    let revision = SyncApiService::get_sync_config(&service)
        .await
        .unwrap()
        .revision;
    let request = TriggerSyncRunRequest {
        expected_config_revision: revision,
    };

    let _accepted = SyncApiService::trigger_sync_run(&service, request.clone())
        .await
        .unwrap();
    executor.started.notified().await;
    let duplicate = SyncApiService::trigger_sync_run(&service, request)
        .await
        .unwrap_err();

    assert_eq!(duplicate.code(), ErrorCode::SyncRunUnavailable);
    assert_eq!(executor.runs.load(Ordering::Relaxed), 1);
    assert_eq!(
        SyncApiService::get_sync_status(&service)
            .await
            .unwrap()
            .completion_state,
        SyncCompletionState::Attempting
    );
    executor.release.notify_one();
    for _attempt in 0..100 {
        if SyncApiService::get_sync_status(&service)
            .await
            .unwrap()
            .completion_state
            == SyncCompletionState::Succeeded
        {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("the released sync run did not complete");
}

#[tokio::test]
async fn second_sync_service_observes_attempting_and_cannot_patch_or_run() {
    let temporary = tempdir().unwrap();
    let (runtime, _workspace, durable) = active_sync_runtime(temporary.path(), test_ports()).await;
    let store = Arc::new(SyncConfigStore::new(durable).unwrap());
    let first_executor = Arc::new(BlockingExecutor::default());
    let second_executor = Arc::new(CountingExecutor::default());
    let first = SyncService::new(runtime.clone(), store.clone(), first_executor.clone());
    let second = SyncService::new(runtime.clone(), store, second_executor.clone());
    let config = SyncApiService::get_sync_config(&first).await.unwrap();

    SyncApiService::trigger_sync_run(
        &first,
        TriggerSyncRunRequest {
            expected_config_revision: config.revision.clone(),
        },
    )
    .await
    .unwrap();
    first_executor.started.notified().await;

    let observed = SyncApiService::get_sync_status(&second).await.unwrap();
    let patch_error = SyncApiService::patch_sync_config(
        &second,
        PatchSyncConfigRequest {
            expected_revision: config.revision.clone(),
            changes: SyncConfigChangesDto {
                remote_root: Some("must-not-install".to_string()),
                ..SyncConfigChangesDto::default()
            },
        },
    )
    .await
    .unwrap_err();
    let run_error = SyncApiService::trigger_sync_run(
        &second,
        TriggerSyncRunRequest {
            expected_config_revision: config.revision,
        },
    )
    .await
    .unwrap_err();

    assert_eq!(observed.completion_state, SyncCompletionState::Attempting);
    assert_eq!(patch_error.code(), ErrorCode::SyncNotReady);
    assert_eq!(run_error.code(), ErrorCode::SyncRunUnavailable);
    assert_eq!(second_executor.runs.load(Ordering::SeqCst), 0);

    first_executor.release.notify_one();
}

#[tokio::test]
async fn sync_executor_context_is_bound_to_one_workspace_snapshot() {
    let temporary = tempdir().unwrap();
    let (runtime, workspace, durable) = active_sync_runtime(temporary.path(), test_ports()).await;
    let executor = Arc::new(ContextBindingExecutor::default());
    let service = SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        executor.clone(),
    );
    let config = SyncApiService::get_sync_config(&service).await.unwrap();
    let before = workspace.service.current().unwrap();
    let mut events = runtime.event_broker().subscribe();

    let first = SyncApiService::trigger_sync_run(
        &service,
        TriggerSyncRunRequest {
            expected_config_revision: config.revision.clone(),
        },
    )
    .await
    .unwrap();
    executor.started.notified().await;
    let first_observed = executor.observed.lock().unwrap()[0].clone();
    assert_eq!(first_observed.0, before);
    assert_eq!(first_observed.2, first.run_id);
    assert_eq!(first_observed.3, SyncTrigger::Manual);
    executor.release.notify_one();
    let first_attempting = events.recv().await.unwrap();
    let first_terminal = events.recv().await.unwrap();
    assert!(matches!(
        first_attempting.event,
        DomainEvent::SyncStatusChanged { status }
            if status.completion_state == SyncCompletionState::Attempting
    ));
    assert!(matches!(
        first_terminal.event,
        DomainEvent::SyncStatusChanged { status }
            if status.completion_state == SyncCompletionState::Succeeded
    ));

    switch_workspace(
        &runtime,
        &workspace,
        &temporary.path().join("context-next-workspace"),
    )
    .await;
    let after = workspace.service.current().unwrap();
    let workspace_changed = events.recv().await.unwrap();
    assert!(matches!(
        workspace_changed.event,
        DomainEvent::WorkspaceChanged { workspace } if workspace == after
    ));
    let second = SyncApiService::trigger_sync_run(
        &service,
        TriggerSyncRunRequest {
            expected_config_revision: config.revision,
        },
    )
    .await
    .unwrap();
    executor.started.notified().await;
    let second_attempting = events.recv().await.unwrap();
    assert!(matches!(
        second_attempting.event,
        DomainEvent::SyncStatusChanged { status }
            if status.completion_state == SyncCompletionState::Attempting
    ));
    let second_observed = executor.observed.lock().unwrap()[1].clone();

    assert_eq!(second_observed.0, after);
    assert_eq!(second_observed.2, second.run_id);
    assert_ne!(first_observed.1, second_observed.1);

    executor.release.notify_one();
    let second_terminal = events.recv().await.unwrap();
    assert!(matches!(
        second_terminal.event,
        DomainEvent::SyncStatusChanged { status }
            if status.completion_state == SyncCompletionState::Succeeded
    ));
}

#[tokio::test]
async fn active_manual_run_rejects_config_patch_without_write_or_status_drift() {
    let temporary = tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    let app_data = temporary.path().join("app-data");
    let cache = temporary.path().join("cache");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&app_data).unwrap();
    std::fs::create_dir(&cache).unwrap();
    let mut config_value: serde_json::Value =
        serde_json::from_slice(&s3_config_bytes("https://s3.example.test")).unwrap();
    config_value["enabled"] = serde_json::json!(true);
    let original = serde_json::to_vec_pretty(&config_value).unwrap();
    std::fs::write(app_data.join("sync-config.json"), &original).unwrap();
    let config = KernelConfig::generate().unwrap();
    let credential = config
        .native_launch_credential()
        .expose_secret()
        .to_string();
    let paths = KernelPaths::desktop(&workspace, &app_data, &cache).unwrap();
    let managed = ManagedWorkspaceCollection::from_paths(&paths).unwrap();
    let durable = DurableFileStore::at_config(paths.config_root(), config.launch_epoch()).unwrap();
    let runtime = KernelRuntime::activate(config, paths, test_ports()).unwrap();
    let _workspace = install_active_workspace(&runtime, managed).await;
    let mut events = runtime.event_broker().subscribe();
    let executor = Arc::new(BlockingExecutor::default());
    let service = Arc::new(SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        executor.clone(),
    ));
    runtime.install_sync_api_service(service.clone()).unwrap();
    let router = build_router(runtime, TransportPolicy::loopback(HOST, ORIGIN).unwrap());
    let before = SyncApiService::get_sync_config(service.as_ref())
        .await
        .unwrap();
    SyncApiService::trigger_sync_run(
        service.as_ref(),
        TriggerSyncRunRequest {
            expected_config_revision: before.revision.clone(),
        },
    )
    .await
    .unwrap();
    executor.started.notified().await;
    let attempting_event = events.recv().await.unwrap();
    assert!(matches!(
        attempting_event.event,
        DomainEvent::SyncStatusChanged { .. }
    ));

    let patch = PatchSyncConfigRequest {
        expected_revision: before.revision.clone(),
        changes: SyncConfigChangesDto {
            remote_root: Some("must-not-install".to_string()),
            ..SyncConfigChangesDto::default()
        },
    };
    let error = SyncApiService::patch_sync_config(service.as_ref(), patch.clone())
        .await
        .unwrap_err();
    let response = router
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/api/v1/sync/config")
                .header(header::HOST, HOST)
                .header(header::AUTHORIZATION, format!("Bearer {credential}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&patch).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(error.code(), ErrorCode::SyncNotReady);
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let envelope: ApiErrorEnvelope = serde_json::from_slice(
        &to_bytes(response.into_body(), 2 * 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(envelope.code(), ErrorCode::SyncNotReady);
    assert_eq!(
        std::fs::read(app_data.join("sync-config.json")).unwrap(),
        original
    );
    let status = SyncApiService::get_sync_status(service.as_ref())
        .await
        .unwrap();
    assert_eq!(status.completion_state, SyncCompletionState::Attempting);
    assert_eq!(status.config_revision.as_ref(), Some(&before.revision));
    assert!(
        tokio::time::timeout(Duration::from_millis(10), events.recv())
            .await
            .is_err()
    );

    executor.release.notify_one();
    for _attempt in 0..100 {
        if SyncApiService::get_sync_status(service.as_ref())
            .await
            .unwrap()
            .completion_state
            == SyncCompletionState::Succeeded
        {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("released run should complete");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn completed_run_event_precedes_a_new_config_and_its_idle_status() {
    let temporary = tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    let app_data = temporary.path().join("app-data");
    let cache = temporary.path().join("cache");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&app_data).unwrap();
    std::fs::create_dir(&cache).unwrap();
    let mut config_value: serde_json::Value =
        serde_json::from_slice(&s3_config_bytes("https://s3.example.test")).unwrap();
    config_value["enabled"] = serde_json::json!(true);
    std::fs::write(
        app_data.join("sync-config.json"),
        serde_json::to_vec_pretty(&config_value).unwrap(),
    )
    .unwrap();
    let config = KernelConfig::generate().unwrap();
    let paths = KernelPaths::desktop(&workspace, &app_data, &cache).unwrap();
    let managed = ManagedWorkspaceCollection::from_paths(&paths).unwrap();
    let durable = DurableFileStore::at_config(paths.config_root(), config.launch_epoch()).unwrap();
    let publication_gate = Arc::new(CompletionPublicationGate::default());
    let runtime = KernelRuntime::activate(
        config,
        paths,
        test_ports_with_event_sink(publication_gate.clone()),
    )
    .unwrap();
    let _workspace = install_active_workspace(&runtime, managed).await;
    let mut events = runtime.event_broker().subscribe();
    let executor = Arc::new(BlockingExecutor::default());
    let service = Arc::new(SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        executor.clone(),
    ));
    let before = SyncApiService::get_sync_config(service.as_ref())
        .await
        .unwrap();
    let retry_revision = before.revision.clone();
    SyncApiService::trigger_sync_run(
        service.as_ref(),
        TriggerSyncRunRequest {
            expected_config_revision: before.revision.clone(),
        },
    )
    .await
    .unwrap();
    executor.started.notified().await;
    let attempting = events.recv().await.unwrap();
    assert!(matches!(
        attempting.event,
        DomainEvent::SyncStatusChanged { .. }
    ));

    executor.release.notify_one();
    publication_gate.wait_for_completion_publication().await;
    let patch_started = Arc::new(Notify::new());
    let patch_service = service.clone();
    let patch_started_in_task = patch_started.clone();
    let patch = tokio::spawn(async move {
        patch_started_in_task.notify_one();
        SyncApiService::patch_sync_config(
            patch_service.as_ref(),
            PatchSyncConfigRequest {
                expected_revision: before.revision,
                changes: SyncConfigChangesDto {
                    remote_root: Some("next-root".to_string()),
                    ..SyncConfigChangesDto::default()
                },
            },
        )
        .await
    });
    patch_started.notified().await;
    let rejected = patch.await.unwrap().unwrap_err();
    assert_eq!(rejected.code(), ErrorCode::SyncNotReady);
    publication_gate.release_completion_publication();
    let completed = events.recv().await.unwrap();
    runtime.wait_for_empty_sync_run_for_test().await.unwrap();
    let after = SyncApiService::patch_sync_config(
        service.as_ref(),
        PatchSyncConfigRequest {
            expected_revision: retry_revision,
            changes: SyncConfigChangesDto {
                remote_root: Some("next-root".to_string()),
                ..SyncConfigChangesDto::default()
            },
        },
    )
    .await
    .unwrap();
    let config_changed = events.recv().await.unwrap();
    let idle = events.recv().await.unwrap();

    assert!(matches!(
        completed.event,
        DomainEvent::SyncStatusChanged { status }
            if status.completion_state == SyncCompletionState::Succeeded
    ));
    assert!(matches!(
        config_changed.event,
        DomainEvent::SyncConfigChanged { .. }
    ));
    assert!(matches!(
        idle.event,
        DomainEvent::SyncStatusChanged { status }
            if status.completion_state == SyncCompletionState::Idle
                && status.config_revision.as_ref() == Some(&after.revision)
    ));
}

#[test]
fn editing_registry_is_instance_owned_and_session_scoped() {
    let first = SyncEditingRegistry::new();
    let second = SyncEditingRegistry::new();
    let revision = Revision::parse("a".repeat(64)).unwrap();

    let active = first
        .set_active("session-a".to_string(), Some(revision.clone()))
        .unwrap();

    assert_eq!(active.counter, 1);
    assert_eq!(active.session_id.as_deref(), Some("session-a"));
    assert_eq!(
        active.revision.as_ref().map(Revision::as_str),
        Some(revision.as_str())
    );
    assert!(second.snapshot().unwrap().session_id.is_none());
    assert!(first
        .clear("different-session")
        .unwrap()
        .session_id
        .is_some());
    let cleared = first.clear("session-a").unwrap();
    assert_eq!(cleared.counter, 3);
    assert!(cleared.session_id.is_none());
    assert!(second.snapshot().unwrap().session_id.is_none());
}

#[test]
fn sync_service_owns_one_shared_editing_registry_per_runtime_instance() {
    let temporary = tempdir().unwrap();
    let first = sync_service_for_editing(temporary.path().join("first"));
    let second = sync_service_for_editing(temporary.path().join("second"));

    let first_caller = first.editing_registry();
    let same_runtime_caller = first.editing_registry();
    let second_runtime_caller = second.editing_registry();

    assert!(Arc::ptr_eq(&first_caller, &same_runtime_caller));
    assert!(!Arc::ptr_eq(&first_caller, &second_runtime_caller));
    first_caller
        .set_active("first-session".to_string(), None)
        .unwrap();
    assert_eq!(
        same_runtime_caller
            .snapshot()
            .unwrap()
            .session_id
            .as_deref(),
        Some("first-session")
    );
    assert!(second_runtime_caller
        .snapshot()
        .unwrap()
        .session_id
        .is_none());
}

#[test]
fn editing_notify_failures_restore_the_exact_prior_state() {
    let registry = SyncEditingRegistry::new();
    let revision = Revision::parse("1".repeat(64)).unwrap();
    registry
        .set_active("session".to_string(), Some(revision.clone()))
        .unwrap();
    let before_edit = registry.snapshot().unwrap();

    let edit_error = registry
        .set_active_with_notify("other-session".to_string(), None, |_| Err(()))
        .unwrap_err();

    assert_eq!(
        edit_error.kind(),
        SyncEditingRegistryErrorKind::NotificationUnavailable
    );
    assert_eq!(registry.snapshot().unwrap(), before_edit);

    let request = SyncApplyRequest {
        exit_reason: SyncApplyExitReason::CategoryLeave,
        revision,
        session_id: "session".to_string(),
        source: SyncApplySource::SettingsExit,
        token: "rollback-token".to_string(),
    };
    let before_apply = registry.snapshot().unwrap();

    let apply_error = registry
        .request_apply_with_notify(request, |_| Err(()))
        .unwrap_err();

    assert_eq!(
        apply_error.kind(),
        SyncEditingRegistryErrorKind::NotificationUnavailable
    );
    assert_eq!(registry.snapshot().unwrap(), before_apply);
}

#[test]
fn editing_apply_registry_deduplicates_only_an_exact_pending_identity() {
    let registry = SyncEditingRegistry::new();
    let revision = Revision::parse("b".repeat(64)).unwrap();
    registry
        .set_active("session-a".to_string(), Some(revision.clone()))
        .unwrap();
    let request = SyncApplyRequest {
        exit_reason: SyncApplyExitReason::CategoryLeave,
        revision: revision.clone(),
        session_id: "session-a".to_string(),
        source: SyncApplySource::SettingsExit,
        token: "private-apply-token".to_string(),
    };

    let first = registry.request_apply(request.clone()).unwrap();
    let duplicate = registry.request_apply(request.clone()).unwrap();
    let before_mismatch = registry.snapshot().unwrap();
    let mismatch = registry
        .request_apply(SyncApplyRequest {
            exit_reason: request.exit_reason,
            revision: Revision::parse("c".repeat(64)).unwrap(),
            session_id: request.session_id.clone(),
            source: request.source,
            token: request.token.clone(),
        })
        .unwrap_err();

    assert_eq!(first.state, SyncApplyState::Pending);
    assert_eq!(duplicate.token, first.token);
    assert_eq!(mismatch.kind(), SyncEditingRegistryErrorKind::ApplyMismatch);
    assert_eq!(registry.snapshot().unwrap(), before_mismatch);
    for safe_debug in [
        format!("{request:?}"),
        format!("{first:?}"),
        format!("{before_mismatch:?}"),
        format!("{registry:?}"),
    ] {
        assert!(!safe_debug.contains("private-apply-token"));
    }
}

#[tokio::test]
async fn editing_apply_registry_claims_once_waits_and_replays_exact_completion() {
    let registry = Arc::new(SyncEditingRegistry::new());
    let revision = Revision::parse("d".repeat(64)).unwrap();
    registry
        .set_active("session".to_string(), Some(revision.clone()))
        .unwrap();
    registry
        .request_apply(SyncApplyRequest {
            exit_reason: SyncApplyExitReason::WindowClose,
            revision: revision.clone(),
            session_id: "session".to_string(),
            source: SyncApplySource::SettingsExit,
            token: "token".to_string(),
        })
        .unwrap();

    assert!(matches!(
        registry.begin_apply(&revision, "token").unwrap(),
        SyncApplyDisposition::Execute
    ));
    assert!(matches!(
        registry.begin_apply(&revision, "token").unwrap(),
        SyncApplyDisposition::Wait
    ));
    let waiter_registry = registry.clone();
    let waiter_revision = revision.clone();
    let waiter = tokio::spawn(async move {
        waiter_registry
            .wait_apply(&waiter_revision, "token")
            .await
            .unwrap()
    });
    let outcome = Ok(SyncApplySuccess {
        revision: revision.clone(),
    });

    registry
        .complete_apply(&revision, "token", outcome.clone())
        .unwrap();
    registry
        .complete_apply(
            &revision,
            "token",
            Err(SyncApplyFailure::ExecutionUnavailable),
        )
        .unwrap();

    assert_eq!(waiter.await.unwrap(), outcome);
    assert_eq!(
        registry.begin_apply(&revision, "token").unwrap(),
        SyncApplyDisposition::Completed(outcome)
    );
}

#[tokio::test]
async fn editing_apply_cancellation_settles_all_waiters_and_allows_next_session() {
    let registry = Arc::new(SyncEditingRegistry::new());
    let revision = Revision::parse("e".repeat(64)).unwrap();
    registry
        .set_active("old-session".to_string(), Some(revision.clone()))
        .unwrap();
    registry
        .request_apply(SyncApplyRequest {
            exit_reason: SyncApplyExitReason::CategoryLeave,
            revision: revision.clone(),
            session_id: "old-session".to_string(),
            source: SyncApplySource::SettingsExit,
            token: "old-token".to_string(),
        })
        .unwrap();
    let waiters = (0..2)
        .map(|_| {
            let registry = registry.clone();
            let revision = revision.clone();
            tokio::spawn(async move { registry.wait_apply(&revision, "old-token").await.unwrap() })
        })
        .collect::<Vec<_>>();

    let cancelled = registry
        .cancel_apply("old-session", &revision, "old-token")
        .unwrap();

    assert_eq!(cancelled.state, SyncApplyState::Completed);
    for waiter in waiters {
        assert_eq!(waiter.await.unwrap(), Err(SyncApplyFailure::Cancelled));
    }
    assert_eq!(
        registry.begin_apply(&revision, "old-token").unwrap(),
        SyncApplyDisposition::Completed(Err(SyncApplyFailure::Cancelled))
    );
    let next_revision = Revision::parse("f".repeat(64)).unwrap();
    registry
        .set_active("new-session".to_string(), Some(next_revision.clone()))
        .unwrap();
    let next = registry
        .request_apply(SyncApplyRequest {
            exit_reason: SyncApplyExitReason::WindowClose,
            revision: next_revision,
            session_id: "new-session".to_string(),
            source: SyncApplySource::SettingsExit,
            token: "new-token".to_string(),
        })
        .unwrap();
    assert_eq!(next.state, SyncApplyState::Pending);
}

#[test]
fn completed_apply_token_cannot_be_reused_by_a_later_session() {
    let registry = SyncEditingRegistry::new();
    let revision = Revision::parse("7".repeat(64)).unwrap();
    registry
        .set_active("old-session".to_string(), Some(revision.clone()))
        .unwrap();
    registry
        .request_apply(SyncApplyRequest {
            exit_reason: SyncApplyExitReason::WindowClose,
            revision: revision.clone(),
            session_id: "old-session".to_string(),
            source: SyncApplySource::SettingsExit,
            token: "reused-token".to_string(),
        })
        .unwrap();
    registry
        .cancel_apply("old-session", &revision, "reused-token")
        .unwrap();
    registry
        .set_active("new-session".to_string(), Some(revision.clone()))
        .unwrap();

    let error = registry
        .request_apply(SyncApplyRequest {
            exit_reason: SyncApplyExitReason::WindowClose,
            revision,
            session_id: "new-session".to_string(),
            source: SyncApplySource::SettingsExit,
            token: "reused-token".to_string(),
        })
        .unwrap_err();

    assert_eq!(error.kind(), SyncEditingRegistryErrorKind::ApplyMismatch);
    assert!(registry.snapshot().unwrap().pending_apply.is_none());
}

#[tokio::test]
async fn direct_and_http_get_patch_have_identical_sync_dtos_and_revisions() {
    let temporary = tempdir().unwrap();
    let http_workspace = temporary.path().join("http-workspace");
    let http_app_data = temporary.path().join("http-app-data");
    let http_cache = temporary.path().join("http-cache");
    let direct_workspace = temporary.path().join("direct-workspace");
    let direct_app_data = temporary.path().join("direct-app-data");
    let direct_cache = temporary.path().join("direct-cache");
    for directory in [
        &http_workspace,
        &http_app_data,
        &http_cache,
        &direct_workspace,
        &direct_app_data,
        &direct_cache,
    ] {
        std::fs::create_dir(directory).unwrap();
    }
    let bytes = s3_config_bytes("https://s3.example.test");
    std::fs::write(http_app_data.join("sync-config.json"), &bytes).unwrap();
    std::fs::write(direct_app_data.join("sync-config.json"), &bytes).unwrap();

    let http_config = KernelConfig::generate().unwrap();
    let credential = http_config
        .native_launch_credential()
        .expose_secret()
        .to_string();
    let http_paths = KernelPaths::desktop(&http_workspace, &http_app_data, &http_cache).unwrap();
    let http_durable =
        DurableFileStore::at_config(http_paths.config_root(), http_config.launch_epoch()).unwrap();
    let http_runtime =
        KernelRuntime::activate(http_config, http_paths, KernelPorts::unavailable()).unwrap();
    let mut http_events = http_runtime.event_broker().subscribe();
    let http_service = Arc::new(SyncService::new(
        http_runtime.clone(),
        Arc::new(SyncConfigStore::new(http_durable).unwrap()),
        Arc::new(RecordingExecutor),
    ));
    http_runtime.install_sync_api_service(http_service).unwrap();
    let router = build_router(
        http_runtime,
        TransportPolicy::loopback(HOST, ORIGIN).unwrap(),
    );

    let direct_config = KernelConfig::generate().unwrap();
    let direct_paths =
        KernelPaths::desktop(&direct_workspace, &direct_app_data, &direct_cache).unwrap();
    let direct_durable =
        DurableFileStore::at_config(direct_paths.config_root(), direct_config.launch_epoch())
            .unwrap();
    let direct_runtime =
        KernelRuntime::activate(direct_config, direct_paths, KernelPorts::unavailable()).unwrap();
    let mut direct_events = direct_runtime.event_broker().subscribe();
    let direct_service = SyncService::new(
        direct_runtime.clone(),
        Arc::new(SyncConfigStore::new(direct_durable).unwrap()),
        Arc::new(RecordingExecutor),
    );
    let direct_before = SyncApiService::get_sync_config(&direct_service)
        .await
        .unwrap();

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/sync/config")
                .header(header::HOST, HOST)
                .header(header::AUTHORIZATION, format!("Bearer {credential}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let http_before: qingyu_kernel::contract::SyncConfigViewDto = serde_json::from_slice(
        &to_bytes(response.into_body(), 2 * 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(http_before, direct_before);

    let patch = PatchSyncConfigRequest {
        expected_revision: direct_before.revision,
        changes: SyncConfigChangesDto {
            enabled: Some(true),
            remote_root: Some("parity".to_string()),
            ..SyncConfigChangesDto::default()
        },
    };
    let direct_after = SyncApiService::patch_sync_config(&direct_service, patch.clone())
        .await
        .unwrap();
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/api/v1/sync/config")
                .header(header::HOST, HOST)
                .header(header::AUTHORIZATION, format!("Bearer {credential}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&patch).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let http_after: qingyu_kernel::contract::SyncConfigViewDto = serde_json::from_slice(
        &to_bytes(response.into_body(), 2 * 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(http_after, direct_after);
    assert_eq!(http_after.revision.as_str().len(), 64);

    let direct_config_event = direct_events.recv().await.unwrap();
    let direct_status_event =
        tokio::time::timeout(Duration::from_millis(100), direct_events.recv())
            .await
            .expect("direct patch must publish its idle status")
            .unwrap();
    let http_config_event = http_events.recv().await.unwrap();
    let http_status_event = tokio::time::timeout(Duration::from_millis(100), http_events.recv())
        .await
        .expect("HTTP patch must publish its idle websocket status")
        .unwrap();
    assert_eq!(http_config_event.event, direct_config_event.event);
    assert_eq!(http_status_event.event, direct_status_event.event);
    assert!(matches!(
        http_config_event.event,
        DomainEvent::SyncConfigChanged { .. }
    ));
    let status_from_event = match &http_status_event.event {
        DomainEvent::SyncStatusChanged { status } => status,
        other => panic!("expected sync status event, got {other:?}"),
    };
    assert_eq!(http_status_event.revision, http_after.revision);
    assert_eq!(
        status_from_event.completion_state,
        SyncCompletionState::Idle
    );
    assert_eq!(
        status_from_event.config_revision.as_ref(),
        Some(&http_after.revision)
    );
    assert!(matches!(
        http_status_event.resource,
        ResourceRefDto::SyncStatus { ref run_id } if run_id.as_ref().is_none()
    ));

    let direct_status = SyncApiService::get_sync_status(&direct_service)
        .await
        .unwrap();
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/sync/status")
                .header(header::HOST, HOST)
                .header(header::AUTHORIZATION, format!("Bearer {credential}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let http_status: qingyu_kernel::contract::SyncStatusDto = serde_json::from_slice(
        &to_bytes(response.into_body(), 2 * 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(http_status, direct_status);
    assert_eq!(&http_status, status_from_event);

    let connection_test = qingyu_kernel::contract::TestSyncConnectionRequest {
        expected_revision: direct_after.revision,
        changes: SyncConfigChangesDto::default(),
    };
    let direct_connection =
        SyncApiService::test_sync_connection(&direct_service, connection_test.clone())
            .await
            .unwrap();
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/sync/connection-test")
                .header(header::HOST, HOST)
                .header(header::AUTHORIZATION, format!("Bearer {credential}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&connection_test).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let http_connection: qingyu_kernel::contract::SyncConnectionTestDto = serde_json::from_slice(
        &to_bytes(response.into_body(), 2 * 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(http_connection, direct_connection);
}

#[tokio::test]
async fn workspace_recovery_rejects_sync_run_before_executor_or_spawn() {
    let temporary = tempdir().unwrap();
    let spawner = Arc::new(DeferredTaskSpawner::default());
    let (runtime, workspace, durable) = active_sync_runtime(
        temporary.path(),
        test_ports_with_task_spawner(spawner.clone()),
    )
    .await;
    let executor = Arc::new(CountingExecutor::default());
    let service = SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        executor.clone(),
    );
    let config = SyncApiService::get_sync_config(&service).await.unwrap();
    enter_workspace_recovery(
        &runtime,
        &workspace,
        &temporary.path().join("recovery-target"),
    )
    .await;

    let error = SyncApiService::trigger_sync_run(
        &service,
        TriggerSyncRunRequest {
            expected_config_revision: config.revision,
        },
    )
    .await
    .unwrap_err();

    assert_eq!(error.code(), ErrorCode::SyncNotReady);
    assert_eq!(spawner.spawned.load(Ordering::SeqCst), 0);
    assert_eq!(executor.runs.load(Ordering::SeqCst), 0);
    assert_eq!(
        SyncApiService::get_sync_status(&service)
            .await
            .unwrap()
            .completion_state,
        SyncCompletionState::Idle
    );
}

#[tokio::test]
async fn sync_config_and_connection_test_remain_available_during_workspace_recovery() {
    let temporary = tempdir().unwrap();
    let (runtime, workspace, durable) =
        active_sync_runtime(temporary.path(), KernelPorts::unavailable()).await;
    let executor = Arc::new(CountingExecutor::default());
    let service = SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        executor.clone(),
    );
    let before = SyncApiService::get_sync_config(&service).await.unwrap();
    enter_workspace_recovery(
        &runtime,
        &workspace,
        &temporary.path().join("recovery-target"),
    )
    .await;

    let observed = SyncApiService::get_sync_config(&service).await.unwrap();
    assert_eq!(observed, before);
    let patched = SyncApiService::patch_sync_config(
        &service,
        PatchSyncConfigRequest {
            expected_revision: before.revision,
            changes: SyncConfigChangesDto {
                remote_root: Some("recovery-tools".to_string()),
                ..SyncConfigChangesDto::default()
            },
        },
    )
    .await
    .unwrap();
    let tested = SyncApiService::test_sync_connection(
        &service,
        qingyu_kernel::contract::TestSyncConnectionRequest {
            expected_revision: patched.revision.clone(),
            changes: SyncConfigChangesDto::default(),
        },
    )
    .await
    .unwrap();
    let status = SyncApiService::get_sync_status(&service).await.unwrap();

    assert_eq!(tested.config_revision, patched.revision);
    assert_eq!(tested.checked_target, "recovery-tools");
    assert_eq!(executor.connection_tests.load(Ordering::SeqCst), 1);
    assert_eq!(executor.runs.load(Ordering::SeqCst), 0);
    assert_eq!(status.completion_state, SyncCompletionState::Idle);
}

#[tokio::test]
async fn workspace_background_spawn_after_recovery_never_calls_task_spawner() {
    let temporary = tempdir().unwrap();
    let spawner = Arc::new(DeferredTaskSpawner::default());
    let (runtime, workspace, _durable) = active_sync_runtime(
        temporary.path(),
        test_ports_with_task_spawner(spawner.clone()),
    )
    .await;
    enter_workspace_recovery(
        &runtime,
        &workspace,
        &temporary.path().join("recovery-target"),
    )
    .await;

    let result = runtime.spawn_background(Box::pin(async {}));

    assert!(result.is_err());
    assert_eq!(spawner.spawned.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn deferred_sync_run_does_not_invoke_executor_after_snapshot_change() {
    let temporary = tempdir().unwrap();
    let spawner = Arc::new(DeferredTaskSpawner::default());
    let (runtime, workspace, durable) = active_sync_runtime(
        temporary.path(),
        test_ports_with_task_spawner(spawner.clone()),
    )
    .await;
    let executor = Arc::new(CountingExecutor::default());
    let service = SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        executor.clone(),
    );
    let config = SyncApiService::get_sync_config(&service).await.unwrap();
    SyncApiService::trigger_sync_run(
        &service,
        TriggerSyncRunRequest {
            expected_config_revision: config.revision,
        },
    )
    .await
    .unwrap();
    assert_eq!(spawner.spawned.load(Ordering::SeqCst), 1);
    assert_eq!(executor.runs.load(Ordering::SeqCst), 0);

    switch_workspace(
        &runtime,
        &workspace,
        &temporary.path().join("next-workspace"),
    )
    .await;
    spawner.take_task().await;

    assert_eq!(executor.runs.load(Ordering::SeqCst), 0);
    assert_eq!(
        SyncApiService::get_sync_status(&service)
            .await
            .unwrap()
            .completion_state,
        SyncCompletionState::Failed
    );
}

#[tokio::test]
async fn accepted_but_unpolled_run_can_be_revoked_and_late_poll_never_calls_executor() {
    let temporary = tempdir().unwrap();
    let spawner = Arc::new(DeferredTaskSpawner::default());
    let (runtime, _workspace, durable) = active_sync_runtime(
        temporary.path(),
        test_ports_with_task_spawner(spawner.clone()),
    )
    .await;
    let executor = Arc::new(CountingExecutor::default());
    let service = SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        executor.clone(),
    );
    let config = SyncApiService::get_sync_config(&service).await.unwrap();
    let accepted = SyncApiService::trigger_sync_run(
        &service,
        TriggerSyncRunRequest {
            expected_config_revision: config.revision,
        },
    )
    .await
    .unwrap();
    let deferred = spawner.take_task();

    let transition = runtime
        .begin_sync_workspace_transition_for_test()
        .await
        .unwrap();
    transition.wait_drained().await.unwrap();
    let cancelled = SyncApiService::get_sync_status(&service).await.unwrap();

    assert_eq!(cancelled.completion_state, SyncCompletionState::Failed);
    assert_eq!(cancelled.active_run_id.as_ref(), None);
    assert_eq!(
        cancelled.error.as_ref().map(|error| error.code()),
        Some("cancelled")
    );
    assert_eq!(
        cancelled
            .error
            .as_ref()
            .and_then(qingyu_kernel::contract::SyncSafeErrorDto::run_id),
        Some(accepted.run_id)
    );

    deferred.await;
    assert_eq!(executor.runs.load(Ordering::SeqCst), 0);

    transition.reopen_for_test().await.unwrap();
}

#[tokio::test]
async fn dropping_an_accepted_unpolled_task_revokes_its_queued_registration() {
    let temporary = tempdir().unwrap();
    let spawner = Arc::new(DeferredTaskSpawner::default());
    let (runtime, _workspace, durable) = active_sync_runtime(
        temporary.path(),
        test_ports_with_task_spawner(spawner.clone()),
    )
    .await;
    let service = SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        Arc::new(CountingExecutor::default()),
    );
    let config = SyncApiService::get_sync_config(&service).await.unwrap();
    SyncApiService::trigger_sync_run(
        &service,
        TriggerSyncRunRequest {
            expected_config_revision: config.revision,
        },
    )
    .await
    .unwrap();

    drop(spawner.take_task());

    let mut drained = Box::pin(runtime.wait_for_empty_sync_run_for_test());
    let drained_after_drop = poll_fn(|context| match drained.as_mut().poll(context) {
        std::task::Poll::Ready(Ok(())) => std::task::Poll::Ready(true),
        std::task::Poll::Ready(Err(error)) => panic!("unexpected drain error: {error}"),
        std::task::Poll::Pending => std::task::Poll::Ready(false),
    })
    .await;
    assert!(
        drained_after_drop,
        "dropped queued task remained registered"
    );
    let status = SyncApiService::get_sync_status(&service).await.unwrap();
    assert_eq!(status.completion_state, SyncCompletionState::Failed);
}

#[tokio::test]
async fn dropping_an_unpolled_task_while_mutation_is_busy_quarantines() {
    let temporary = tempdir().unwrap();
    let spawner = Arc::new(DeferredTaskSpawner::default());
    let (runtime, _workspace, durable) = active_sync_runtime(
        temporary.path(),
        test_ports_with_task_spawner(spawner.clone()),
    )
    .await;
    let service = SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        Arc::new(CountingExecutor::default()),
    );
    let config = SyncApiService::get_sync_config(&service).await.unwrap();
    SyncApiService::trigger_sync_run(
        &service,
        TriggerSyncRunRequest {
            expected_config_revision: config.revision,
        },
    )
    .await
    .unwrap();
    let queued = spawner.take_task();
    let mutation = runtime.mutation_coordinator().lock().await;

    drop(queued);

    assert!(runtime.active_workspace_snapshot().is_err());
    drop(mutation);
    assert!(runtime.active_workspace_snapshot().is_err());
    assert!(runtime
        .begin_sync_workspace_transition_for_test()
        .await
        .is_err());
}

#[tokio::test]
async fn aborting_a_running_background_task_quarantines_its_workspace() {
    let temporary = tempdir().unwrap();
    let spawner = Arc::new(DeferredTaskSpawner::default());
    let (runtime, _workspace, durable) = active_sync_runtime(
        temporary.path(),
        test_ports_with_task_spawner(spawner.clone()),
    )
    .await;
    let executor = Arc::new(BlockingExecutor::default());
    let service = SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        executor.clone(),
    );
    let config = SyncApiService::get_sync_config(&service).await.unwrap();
    SyncApiService::trigger_sync_run(
        &service,
        TriggerSyncRunRequest {
            expected_config_revision: config.revision,
        },
    )
    .await
    .unwrap();
    let task = tokio::spawn(spawner.take_task());
    executor.started.notified().await;

    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());

    assert!(runtime.active_workspace_snapshot().is_err());
    assert!(runtime
        .begin_sync_workspace_transition_for_test()
        .await
        .is_err());
}

#[tokio::test]
async fn aborting_a_running_background_task_while_mutation_is_busy_still_quarantines() {
    let temporary = tempdir().unwrap();
    let spawner = Arc::new(DeferredTaskSpawner::default());
    let (runtime, _workspace, durable) = active_sync_runtime(
        temporary.path(),
        test_ports_with_task_spawner(spawner.clone()),
    )
    .await;
    let executor = Arc::new(BlockingExecutor::default());
    let service = SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        executor.clone(),
    );
    let config = SyncApiService::get_sync_config(&service).await.unwrap();
    SyncApiService::trigger_sync_run(
        &service,
        TriggerSyncRunRequest {
            expected_config_revision: config.revision,
        },
    )
    .await
    .unwrap();
    let task = tokio::spawn(spawner.take_task());
    executor.started.notified().await;
    let mutation = runtime.mutation_coordinator().lock().await;

    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());

    assert!(
        runtime.active_workspace_snapshot().is_err(),
        "a dropped running task must quarantine even while another mutation owns the permit"
    );
    drop(mutation);
    assert!(runtime.active_workspace_snapshot().is_err());
    assert!(runtime
        .begin_sync_workspace_transition_for_test()
        .await
        .is_err());
}

#[tokio::test]
async fn revoked_unpolled_run_releases_old_workspace_lease_before_commit() {
    let temporary = tempdir().unwrap();
    let spawner = Arc::new(DeferredTaskSpawner::default());
    let (runtime, _workspace, durable) = active_sync_runtime(
        temporary.path(),
        test_ports_with_task_spawner(spawner.clone()),
    )
    .await;
    let service = SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        Arc::new(CountingExecutor::default()),
    );
    let old_snapshot = runtime.active_workspace_snapshot().unwrap();
    let baseline_holds = Arc::strong_count(&old_snapshot);
    let config = SyncApiService::get_sync_config(&service).await.unwrap();
    SyncApiService::trigger_sync_run(
        &service,
        TriggerSyncRunRequest {
            expected_config_revision: config.revision,
        },
    )
    .await
    .unwrap();
    let deferred = spawner.take_task();

    let transition = runtime
        .begin_sync_workspace_transition_for_test()
        .await
        .unwrap();
    transition.wait_drained().await.unwrap();

    assert_eq!(
        Arc::strong_count(&old_snapshot),
        baseline_holds + 1,
        "only the transition identity may retain the old snapshot before commit"
    );
    deferred.await;
    transition.reopen_for_test().await.unwrap();
}

#[tokio::test]
async fn spawn_failure_releases_runtime_registration_and_shared_status() {
    let temporary = tempdir().unwrap();
    let spawner = Arc::new(FailingTaskSpawner::default());
    let (runtime, _workspace, durable) = active_sync_runtime(
        temporary.path(),
        test_ports_with_task_spawner(spawner.clone()),
    )
    .await;
    let service = SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        Arc::new(CountingExecutor::default()),
    );
    let config = SyncApiService::get_sync_config(&service).await.unwrap();

    for _attempt in 0..2 {
        let error = SyncApiService::trigger_sync_run(
            &service,
            TriggerSyncRunRequest {
                expected_config_revision: config.revision.clone(),
            },
        )
        .await
        .unwrap_err();
        let status = SyncApiService::get_sync_status(&service).await.unwrap();

        assert_eq!(error.code(), ErrorCode::SyncRunUnavailable);
        assert_eq!(status.completion_state, SyncCompletionState::Failed);
        assert_eq!(
            status.error.as_ref().map(|error| error.code()),
            Some("unknown")
        );
    }

    assert_eq!(spawner.attempts.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn cancelled_run_is_failed_with_cancelled_error_not_unknown_provider_error() {
    let temporary = tempdir().unwrap();
    let (runtime, _workspace, durable) = active_sync_runtime(temporary.path(), test_ports()).await;
    let executor = Arc::new(CancellationAwareExecutor::default());
    let service = SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        executor.clone(),
    );
    let config = SyncApiService::get_sync_config(&service).await.unwrap();
    let accepted = SyncApiService::trigger_sync_run(
        &service,
        TriggerSyncRunRequest {
            expected_config_revision: config.revision,
        },
    )
    .await
    .unwrap();
    executor.started.notified().await;

    let transition = runtime
        .begin_sync_workspace_transition_for_test()
        .await
        .unwrap();
    transition.wait_drained().await.unwrap();
    let cancelled = SyncApiService::get_sync_status(&service).await.unwrap();
    let error = cancelled.error.as_ref().unwrap();

    assert_eq!(cancelled.completion_state, SyncCompletionState::Failed);
    assert_eq!(error.code(), "cancelled");
    assert_eq!(error.category(), None);
    assert_eq!(error.provider_error_code(), None);
    assert_eq!(error.run_id(), Some(accepted.run_id));

    transition.reopen_for_test().await.unwrap();
}

#[tokio::test]
async fn cancel_before_waiter_and_waiter_before_cancel_are_lossless() {
    for waiter_first in [false, true] {
        let temporary = tempdir().unwrap();
        let (runtime, _workspace, durable) =
            active_sync_runtime(temporary.path(), test_ports()).await;
        let (executor, cancellation_receiver) = CancellationContextExecutor::new();
        let service = SyncService::new(
            runtime.clone(),
            Arc::new(SyncConfigStore::new(durable).unwrap()),
            executor.clone(),
        );
        let config = SyncApiService::get_sync_config(&service).await.unwrap();
        SyncApiService::trigger_sync_run(
            &service,
            TriggerSyncRunRequest {
                expected_config_revision: config.revision,
            },
        )
        .await
        .unwrap();
        let cancellation = cancellation_receiver.await.unwrap();

        if waiter_first {
            let mut waiter = Box::pin(cancellation.cancelled());
            poll_fn(|context| match waiter.as_mut().poll(context) {
                std::task::Poll::Pending => std::task::Poll::Ready(()),
                std::task::Poll::Ready(()) => {
                    panic!("cancellation waiter completed before transition cancellation")
                }
            })
            .await;
            let transition = runtime
                .begin_sync_workspace_transition_for_test()
                .await
                .unwrap();
            waiter.await;
            executor.release.notify_one();
            transition.wait_drained().await.unwrap();
            transition.reopen_for_test().await.unwrap();
        } else {
            let transition = runtime
                .begin_sync_workspace_transition_for_test()
                .await
                .unwrap();
            cancellation.cancelled().await;
            executor.release.notify_one();
            transition.wait_drained().await.unwrap();
            transition.reopen_for_test().await.unwrap();
        }
    }
}

#[tokio::test]
async fn running_cancel_finalizer_acquires_mutation_before_reporting_drained() {
    let temporary = tempdir().unwrap();
    let (runtime, _workspace, durable) = active_sync_runtime(temporary.path(), test_ports()).await;
    let executor = Arc::new(GatedCancellationExecutor::default());
    let service = SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        executor.clone(),
    );
    let config = SyncApiService::get_sync_config(&service).await.unwrap();
    SyncApiService::trigger_sync_run(
        &service,
        TriggerSyncRunRequest {
            expected_config_revision: config.revision,
        },
    )
    .await
    .unwrap();
    executor.started.notified().await;
    let transition = runtime
        .begin_sync_workspace_transition_for_test()
        .await
        .unwrap();
    executor.cancellation_seen.notified().await;

    let mutation = runtime.mutation_coordinator().lock().await;
    executor.release.notify_one();
    let mut drained = Box::pin(transition.wait_drained());
    poll_fn(|context| match drained.as_mut().poll(context) {
        std::task::Poll::Pending => std::task::Poll::Ready(()),
        std::task::Poll::Ready(_) => {
            panic!("run reported drained before its finalizer acquired mutation")
        }
    })
    .await;

    drop(mutation);
    drained.await.unwrap();
    transition.reopen_for_test().await.unwrap();
}

#[tokio::test]
async fn terminal_event_callback_runs_after_mutation_is_released() {
    let temporary = tempdir().unwrap();
    let sink = Arc::new(TerminalReentrantSink::default());
    let (runtime, _workspace, durable) =
        active_sync_runtime(temporary.path(), test_ports_with_event_sink(sink.clone())).await;
    sink.bind_runtime(&runtime);
    let executor = Arc::new(ManualExecutor::default());
    let service = SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        executor.clone(),
    );
    let config = SyncApiService::get_sync_config(&service).await.unwrap();

    SyncApiService::trigger_sync_run(
        &service,
        TriggerSyncRunRequest {
            expected_config_revision: config.revision,
        },
    )
    .await
    .unwrap();
    executor.completed.notified().await;
    sink.terminal_seen.notified().await;

    assert!(sink.mutation_available.load(Ordering::SeqCst));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn executor_future_is_dropped_before_terminal_publication_and_drain() {
    let temporary = tempdir().unwrap();
    let publication_gate = Arc::new(CompletionPublicationGate::default());
    let (runtime, _workspace, durable) = active_sync_runtime(
        temporary.path(),
        test_ports_with_event_sink(publication_gate.clone()),
    )
    .await;
    let executor = Arc::new(DropRetainingExecutor::default());
    let service = SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        executor.clone(),
    );
    let config = SyncApiService::get_sync_config(&service).await.unwrap();

    SyncApiService::trigger_sync_run(
        &service,
        TriggerSyncRunRequest {
            expected_config_revision: config.revision,
        },
    )
    .await
    .unwrap();
    publication_gate.wait_for_completion_publication().await;

    let mut future_dropped = Box::pin(executor.wait_for_future_drop());
    let dropped_before_publication =
        poll_fn(|context| match future_dropped.as_mut().poll(context) {
            std::task::Poll::Ready(()) => std::task::Poll::Ready(true),
            std::task::Poll::Pending => std::task::Poll::Ready(false),
        })
        .await;
    publication_gate.release_completion_publication();

    assert!(
        dropped_before_publication,
        "the completed executor future retained its workspace context into terminal publication"
    );
    runtime.wait_for_empty_sync_run_for_test().await.unwrap();
}

#[tokio::test]
async fn workspace_transition_rejects_during_terminal_publication_without_deadlock() {
    let temporary = tempdir().unwrap();
    let sink = Arc::new(TerminalReentrantSink::default());
    let (runtime, _workspace, durable) =
        active_sync_runtime(temporary.path(), test_ports_with_event_sink(sink.clone())).await;
    sink.bind_runtime(&runtime);
    let executor = Arc::new(ManualExecutor::default());
    let service = SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        executor.clone(),
    );
    let config = SyncApiService::get_sync_config(&service).await.unwrap();

    SyncApiService::trigger_sync_run(
        &service,
        TriggerSyncRunRequest {
            expected_config_revision: config.revision,
        },
    )
    .await
    .unwrap();
    executor.completed.notified().await;
    sink.terminal_seen.notified().await;

    assert!(sink.transition_rejected.load(Ordering::SeqCst));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn natural_terminal_event_precedes_concurrent_workspace_changed() {
    let temporary = tempdir().unwrap();
    let (sink, terminal_started) = TerminalOrderingSink::new();
    let (runtime, workspace, durable) =
        active_sync_runtime(temporary.path(), test_ports_with_event_sink(sink.clone())).await;
    let executor = Arc::new(ManualExecutor::default());
    let service = SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        executor.clone(),
    );
    let config = SyncApiService::get_sync_config(&service).await.unwrap();
    SyncApiService::trigger_sync_run(
        &service,
        TriggerSyncRunRequest {
            expected_config_revision: config.revision,
        },
    )
    .await
    .unwrap();
    executor.completed.notified().await;
    terminal_started
        .recv_timeout(Duration::from_secs(5))
        .expect("terminal callback must enter deterministically");

    let concurrent = runtime.try_begin_sync_workspace_transition_for_test();
    assert!(concurrent.is_err());
    sink.release_terminal();
    sink.terminal_finished.notified().await;
    runtime.wait_for_empty_sync_run_for_test().await.unwrap();

    let transition = runtime
        .begin_sync_workspace_transition_for_test()
        .await
        .unwrap();
    let current = workspace.service.current().unwrap();
    EventSink::publish(
        runtime.as_ref(),
        &EventPublication {
            resource: ResourceRefDto::Workspace { id: current.id },
            revision: current.revision.clone(),
            event: DomainEvent::WorkspaceChanged { workspace: current },
        },
    )
    .unwrap();

    assert_eq!(
        *sink.order.lock().unwrap(),
        [
            "sync-terminal-start",
            "sync-terminal-end",
            "workspace-changed"
        ]
    );
    transition.reopen_for_test().await.unwrap();
}

#[tokio::test]
async fn executor_panic_does_not_strand_running_registration() {
    let temporary = tempdir().unwrap();
    let spawner = Arc::new(DeferredTaskSpawner::default());
    let (runtime, _workspace, durable) = active_sync_runtime(
        temporary.path(),
        test_ports_with_task_spawner(spawner.clone()),
    )
    .await;
    let executor = Arc::new(PanicOnceExecutor::default());
    let service = SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        executor.clone(),
    );
    let config = SyncApiService::get_sync_config(&service).await.unwrap();

    SyncApiService::trigger_sync_run(
        &service,
        TriggerSyncRunRequest {
            expected_config_revision: config.revision.clone(),
        },
    )
    .await
    .unwrap();
    let first_wrapper = AssertUnwindSafe(spawner.take_task()).catch_unwind().await;
    let failed = SyncApiService::get_sync_status(&service).await.unwrap();

    assert!(
        first_wrapper.is_ok(),
        "executor unwind escaped the sync wrapper"
    );
    assert_eq!(failed.completion_state, SyncCompletionState::Failed);
    assert_eq!(
        failed.error.as_ref().map(|error| error.code()),
        Some("unknown")
    );

    SyncApiService::trigger_sync_run(
        &service,
        TriggerSyncRunRequest {
            expected_config_revision: config.revision,
        },
    )
    .await
    .unwrap();
    spawner.take_task().await;
    let succeeded = SyncApiService::get_sync_status(&service).await.unwrap();

    assert_eq!(executor.runs.load(Ordering::SeqCst), 2);
    assert_eq!(succeeded.completion_state, SyncCompletionState::Succeeded);
}

#[tokio::test]
async fn first_snapshot_install_and_lifecycle_open_are_atomic() {
    let temporary = tempdir().unwrap();
    let workspace_root = temporary.path().join("workspace");
    let app_data = temporary.path().join("app-data");
    let cache = temporary.path().join("cache");
    std::fs::create_dir(&workspace_root).unwrap();
    std::fs::create_dir(&app_data).unwrap();
    std::fs::create_dir(&cache).unwrap();
    let paths = KernelPaths::desktop(&workspace_root, &app_data, &cache).unwrap();
    let managed = ManagedWorkspaceCollection::from_paths(&paths).unwrap();
    let runtime =
        KernelRuntime::activate(KernelConfig::generate().unwrap(), paths, test_ports()).unwrap();
    runtime.poison_sync_lifecycle_for_test();

    let result = WorkspaceService::new(
        &runtime,
        Arc::new(MemoryPrimaryWorkspaceStore::default()),
        managed,
        Arc::new(TestHost),
        "Workspace",
    )
    .await;

    assert!(result.is_err());
    assert!(runtime.active_workspace_snapshot().is_err());
}

#[tokio::test]
async fn poisoned_lifecycle_or_status_never_reports_drained_or_allows_admission() {
    let lifecycle_temporary = tempdir().unwrap();
    let lifecycle_spawner = Arc::new(DeferredTaskSpawner::default());
    let (lifecycle_runtime, _workspace, lifecycle_durable) = active_sync_runtime(
        lifecycle_temporary.path(),
        test_ports_with_task_spawner(lifecycle_spawner.clone()),
    )
    .await;
    let lifecycle_service = SyncService::new(
        lifecycle_runtime.clone(),
        Arc::new(SyncConfigStore::new(lifecycle_durable).unwrap()),
        Arc::new(CountingExecutor::default()),
    );
    let lifecycle_config = SyncApiService::get_sync_config(&lifecycle_service)
        .await
        .unwrap();
    lifecycle_runtime.poison_sync_lifecycle_for_test();

    let lifecycle_run = SyncApiService::trigger_sync_run(
        &lifecycle_service,
        TriggerSyncRunRequest {
            expected_config_revision: lifecycle_config.revision,
        },
    )
    .await;
    let lifecycle_transition = lifecycle_runtime
        .begin_sync_workspace_transition_for_test()
        .await;

    assert!(lifecycle_run.is_err());
    assert!(lifecycle_transition.is_err());
    assert_eq!(lifecycle_spawner.spawned.load(Ordering::SeqCst), 0);

    let status_temporary = tempdir().unwrap();
    let status_spawner = Arc::new(DeferredTaskSpawner::default());
    let (status_runtime, _workspace, status_durable) = active_sync_runtime(
        status_temporary.path(),
        test_ports_with_task_spawner(status_spawner.clone()),
    )
    .await;
    let status_service = SyncService::new(
        status_runtime.clone(),
        Arc::new(SyncConfigStore::new(status_durable).unwrap()),
        Arc::new(CountingExecutor::default()),
    );
    let status_config = SyncApiService::get_sync_config(&status_service)
        .await
        .unwrap();
    status_runtime.poison_sync_status_for_test();

    let status_run = SyncApiService::trigger_sync_run(
        &status_service,
        TriggerSyncRunRequest {
            expected_config_revision: status_config.revision,
        },
    )
    .await;
    let status_transition = status_runtime
        .begin_sync_workspace_transition_for_test()
        .await;

    assert!(status_run.is_err());
    assert!(status_transition.is_err());
    assert_eq!(status_spawner.spawned.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn queued_revocation_status_failure_retains_a_non_drained_registration() {
    let temporary = tempdir().unwrap();
    let spawner = Arc::new(DeferredTaskSpawner::default());
    let (runtime, _workspace, durable) = active_sync_runtime(
        temporary.path(),
        test_ports_with_task_spawner(spawner.clone()),
    )
    .await;
    let service = SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        Arc::new(CountingExecutor::default()),
    );
    let config = SyncApiService::get_sync_config(&service).await.unwrap();
    SyncApiService::trigger_sync_run(
        &service,
        TriggerSyncRunRequest {
            expected_config_revision: config.revision,
        },
    )
    .await
    .unwrap();
    runtime.poison_sync_status_for_test();

    let transition = runtime.begin_sync_workspace_transition_for_test().await;

    assert!(transition.is_err());
    assert_sync_run_is_not_reported_drained(runtime.as_ref()).await;
    drop(spawner.take_task());
}

#[tokio::test]
async fn rejected_claim_status_failure_retains_a_non_drained_registration() {
    let temporary = tempdir().unwrap();
    let spawner = Arc::new(DeferredTaskSpawner::default());
    let (runtime, _workspace, durable) = active_sync_runtime(
        temporary.path(),
        test_ports_with_task_spawner(spawner.clone()),
    )
    .await;
    let service = SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        Arc::new(CountingExecutor::default()),
    );
    let config = SyncApiService::get_sync_config(&service).await.unwrap();
    SyncApiService::trigger_sync_run(
        &service,
        TriggerSyncRunRequest {
            expected_config_revision: config.revision,
        },
    )
    .await
    .unwrap();
    runtime.close_sync_admission_for_test().unwrap();
    runtime.poison_sync_status_for_test();

    spawner.take_task().await;

    assert_sync_run_is_not_reported_drained(runtime.as_ref()).await;
}

#[tokio::test]
async fn spawn_failure_status_failure_retains_a_non_drained_registration() {
    let temporary = tempdir().unwrap();
    let spawner = Arc::new(StatusPoisoningFailSpawner::default());
    let (runtime, _workspace, durable) = active_sync_runtime(
        temporary.path(),
        test_ports_with_task_spawner(spawner.clone()),
    )
    .await;
    spawner.bind_runtime(&runtime);
    let service = SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        Arc::new(CountingExecutor::default()),
    );
    let config = SyncApiService::get_sync_config(&service).await.unwrap();

    let result = SyncApiService::trigger_sync_run(
        &service,
        TriggerSyncRunRequest {
            expected_config_revision: config.revision,
        },
    )
    .await;

    assert!(result.is_err());
    assert_sync_run_is_not_reported_drained(runtime.as_ref()).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deferred_sync_run_is_revoked_before_inflight_workspace_commit() {
    let temporary = tempdir().unwrap();
    let spawner = Arc::new(DeferredTaskSpawner::default());
    let (runtime, workspace, durable) = active_sync_runtime(
        temporary.path(),
        test_ports_with_task_spawner(spawner.clone()),
    )
    .await;
    let executor = Arc::new(CountingExecutor::default());
    let service = SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        executor.clone(),
    );
    let config = SyncApiService::get_sync_config(&service).await.unwrap();
    SyncApiService::trigger_sync_run(
        &service,
        TriggerSyncRunRequest {
            expected_config_revision: config.revision,
        },
    )
    .await
    .unwrap();

    let target = temporary.path().join("inflight-target");
    std::fs::create_dir(&target).unwrap();
    let before = workspace.service.current().unwrap();
    let prepared = runtime.prepare_host_workspace_authority(&target).unwrap();
    let (commit_started, release_commit) = workspace.store.block_next_save();
    let workspace_service = workspace.service.clone();
    let switch = tokio::spawn(async move {
        workspace_service
            .compare_and_set_host_workspace(&before.revision, prepared, "Inflight Target")
            .await
    });
    commit_started
        .recv_timeout(Duration::from_secs(5))
        .expect("workspace commit must hold the mutation permit before deferred admission");
    assert_eq!(executor.runs.load(Ordering::SeqCst), 0);

    release_commit.send(()).unwrap();
    switch.await.unwrap().unwrap();
    spawner.take_task().await;

    assert_eq!(executor.runs.load(Ordering::SeqCst), 0);
    assert_eq!(
        SyncApiService::get_sync_status(&service)
            .await
            .unwrap()
            .completion_state,
        SyncCompletionState::Failed
    );
}

#[tokio::test]
async fn disabled_legacy_invalid_remote_roots_fail_closed_without_debug_or_event_disclosure() {
    for invalid_remote_root in [
        "/Users/alice/private-notes",
        "../private-notes",
        "notes\\private",
        "notes/\u{0001}private",
        "notes//private",
        "C:/private-notes",
        "notes/",
    ] {
        let temporary = tempdir().unwrap();
        let workspace = temporary.path().join("workspace");
        let app_data = temporary.path().join("app-data");
        let cache = temporary.path().join("cache");
        std::fs::create_dir(&workspace).unwrap();
        std::fs::create_dir(&app_data).unwrap();
        std::fs::create_dir(&cache).unwrap();
        let mut config_value: serde_json::Value =
            serde_json::from_slice(&s3_config_bytes("https://s3.example.test")).unwrap();
        config_value["remoteRoot"] = serde_json::json!(invalid_remote_root);
        let original = serde_json::to_vec_pretty(&config_value).unwrap();
        std::fs::write(app_data.join("sync-config.json"), &original).unwrap();
        let config = KernelConfig::generate().unwrap();
        let paths = KernelPaths::desktop(&workspace, &app_data, &cache).unwrap();
        let durable =
            DurableFileStore::at_config(paths.config_root(), config.launch_epoch()).unwrap();
        let runtime = KernelRuntime::activate(config, paths, KernelPorts::unavailable()).unwrap();
        let mut events = runtime.event_broker().subscribe();
        let store = Arc::new(SyncConfigStore::new(durable).unwrap());
        let loaded = store.load().unwrap();
        let loaded_debug = match &loaded {
            SyncConfigLoad::Loaded { config, .. } => format!("{config:?}"),
            other => panic!("expected loaded sync config, got {other:?}"),
        };
        let service = SyncService::new(runtime.clone(), store, Arc::new(RecordingExecutor));

        let get_error = SyncApiService::get_sync_config(&service).await.unwrap_err();
        let status_error = SyncApiService::get_sync_status(&service).await.unwrap_err();
        let patch_error = SyncApiService::patch_sync_config(
            &service,
            PatchSyncConfigRequest {
                expected_revision: Revision::parse(format!(
                    "{:x}",
                    sha2::Sha256::digest(&original)
                ))
                .unwrap(),
                changes: SyncConfigChangesDto {
                    generate_conflict_document: Some(true),
                    ..SyncConfigChangesDto::default()
                },
            },
        )
        .await
        .unwrap_err();

        assert_eq!(get_error.code(), ErrorCode::SyncConfigInvalid);
        assert_eq!(status_error.code(), ErrorCode::SyncNotReady);
        assert_eq!(patch_error.code(), ErrorCode::SyncConfigInvalid);
        for safe_output in [
            format!("{get_error:?}"),
            format!("{status_error:?}"),
            format!("{patch_error:?}"),
            loaded_debug,
        ] {
            assert!(!safe_output.contains(invalid_remote_root));
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(10), events.recv())
                .await
                .is_err()
        );
        assert_eq!(
            std::fs::read(app_data.join("sync-config.json")).unwrap(),
            original
        );
    }
}

#[tokio::test]
async fn submitted_absolute_or_noncanonical_remote_roots_are_rejected_without_normalization() {
    for invalid_remote_root in [
        "/Users/alice/private-notes",
        "../private-notes",
        "notes\\private",
        "notes/\u{0001}private",
        "notes//private",
        "C:/private-notes",
        " notes/private ",
    ] {
        let temporary = tempdir().unwrap();
        let workspace = temporary.path().join("workspace");
        let app_data = temporary.path().join("app-data");
        let cache = temporary.path().join("cache");
        std::fs::create_dir(&workspace).unwrap();
        std::fs::create_dir(&app_data).unwrap();
        std::fs::create_dir(&cache).unwrap();
        let original = s3_config_bytes("https://s3.example.test");
        std::fs::write(app_data.join("sync-config.json"), &original).unwrap();
        let config = KernelConfig::generate().unwrap();
        let paths = KernelPaths::desktop(&workspace, &app_data, &cache).unwrap();
        let durable =
            DurableFileStore::at_config(paths.config_root(), config.launch_epoch()).unwrap();
        let runtime = KernelRuntime::activate(config, paths, KernelPorts::unavailable()).unwrap();
        let mut events = runtime.event_broker().subscribe();
        let service = SyncService::new(
            runtime.clone(),
            Arc::new(SyncConfigStore::new(durable).unwrap()),
            Arc::new(RecordingExecutor),
        );
        let before = SyncApiService::get_sync_config(&service).await.unwrap();

        let error = SyncApiService::patch_sync_config(
            &service,
            PatchSyncConfigRequest {
                expected_revision: before.revision,
                changes: SyncConfigChangesDto {
                    remote_root: Some(invalid_remote_root.to_string()),
                    ..SyncConfigChangesDto::default()
                },
            },
        )
        .await
        .unwrap_err();

        assert_eq!(error.code(), ErrorCode::SyncConfigInvalid);
        assert!(!format!("{error:?}").contains(invalid_remote_root));
        assert!(
            tokio::time::timeout(Duration::from_millis(10), events.recv())
                .await
                .is_err()
        );
        assert_eq!(
            std::fs::read(app_data.join("sync-config.json")).unwrap(),
            original
        );
    }
}

fn s3_config_bytes(endpoint: &str) -> Vec<u8> {
    serde_json::to_vec_pretty(&serde_json::json!({
        "version": 3,
        "enabled": false,
        "provider": "s3",
        "remoteRoot": "qingyu",
        "mode": "automatic",
        "intervalSeconds": 30,
        "generateConflictDocument": false,
        "webdav": { "serverUrl": "", "username": "", "password": "" },
        "s3": {
            "endpointUrl": endpoint,
            "region": "us-east-1",
            "bucket": "notes",
            "accessKeyId": "access-key-id",
            "secretAccessKey": "secret-access-key",
            "requestTimeoutSeconds": 60,
            "addressingStyle": "auto",
            "tlsVerification": "verify"
        }
    }))
    .unwrap()
}

fn sync_service_for_editing(root: std::path::PathBuf) -> SyncService {
    let workspace = root.join("workspace");
    let app_data = root.join("app-data");
    let cache = root.join("cache");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::create_dir(&app_data).unwrap();
    std::fs::create_dir(&cache).unwrap();
    let config = KernelConfig::generate().unwrap();
    let paths = KernelPaths::desktop(&workspace, &app_data, &cache).unwrap();
    let durable = DurableFileStore::at_config(paths.config_root(), config.launch_epoch()).unwrap();
    let runtime = KernelRuntime::activate(config, paths, KernelPorts::unavailable()).unwrap();
    SyncService::new(
        runtime,
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        Arc::new(RecordingExecutor),
    )
}

#[tokio::test]
async fn absent_config_is_reported_without_creating_a_file() {
    let temporary = tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    let app_data = temporary.path().join("app-data");
    let cache = temporary.path().join("cache");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&app_data).unwrap();
    std::fs::create_dir(&cache).unwrap();
    let config = KernelConfig::generate().unwrap();
    let paths = KernelPaths::desktop(&workspace, &app_data, &cache).unwrap();
    let durable = DurableFileStore::at_config(paths.config_root(), config.launch_epoch()).unwrap();
    let runtime = KernelRuntime::activate(config, paths, KernelPorts::unavailable()).unwrap();
    let store = Arc::new(SyncConfigStore::new(durable).unwrap());
    let service = SyncService::new(runtime.clone(), store, Arc::new(RecordingExecutor));

    let error = SyncApiService::get_sync_config(&service).await.unwrap_err();

    assert_eq!(error.code(), ErrorCode::SyncConfigAbsent);
    assert!(!app_data.join("sync-config.json").exists());
}

#[tokio::test]
async fn existing_v3_config_is_exposed_without_inline_credentials() {
    let temporary = tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    let app_data = temporary.path().join("app-data");
    let cache = temporary.path().join("cache");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&app_data).unwrap();
    std::fs::create_dir(&cache).unwrap();
    let bytes = serde_json::to_vec_pretty(&serde_json::json!({
        "version": 3,
        "enabled": false,
        "provider": "s3",
        "remoteRoot": "qingyu",
        "mode": "automatic",
        "intervalSeconds": 30,
        "generateConflictDocument": false,
        "webdav": {
            "serverUrl": "https://dav.example.test/root",
            "username": "alice",
            "password": "webdav-password"
        },
        "s3": {
            "endpointUrl": "https://s3.example.test",
            "region": "us-east-1",
            "bucket": "notes",
            "accessKeyId": "access-key-id",
            "secretAccessKey": "secret-access-key",
            "requestTimeoutSeconds": 60,
            "addressingStyle": "auto",
            "tlsVerification": "verify"
        }
    }))
    .unwrap();
    std::fs::write(app_data.join("sync-config.json"), &bytes).unwrap();
    let config = KernelConfig::generate().unwrap();
    let paths = KernelPaths::desktop(&workspace, &app_data, &cache).unwrap();
    let durable = DurableFileStore::at_config(paths.config_root(), config.launch_epoch()).unwrap();
    let runtime = KernelRuntime::activate(config, paths, KernelPorts::unavailable()).unwrap();
    let store = Arc::new(SyncConfigStore::new(durable).unwrap());
    let service = SyncService::new(runtime.clone(), store, Arc::new(RecordingExecutor));

    let exposed = SyncApiService::get_sync_config(&service).await.unwrap();

    assert_eq!(
        exposed.revision.as_str(),
        format!("{:x}", sha2::Sha256::digest(&bytes))
    );
    assert_eq!(exposed.revision.as_str().len(), 64);
    assert_eq!(
        exposed.s3.endpoint_url.value.as_ref().map(String::as_str),
        Some("https://s3.example.test/")
    );
    assert!(!exposed.s3.endpoint_url.redacted);
    assert!(exposed.s3.access_key_id.present);
    assert!(exposed.s3.secret_access_key.present);
    assert!(exposed.webdav.password.present);
    let serialized = serde_json::to_string(&exposed).unwrap();
    assert!(!serialized.contains("access-key-id"));
    assert!(!serialized.contains("secret-access-key"));
    assert!(!serialized.contains("webdav-password"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sync_shutdown_closes_admission_cancels_and_waits_for_running_settlement() {
    let temporary = tempdir().unwrap();
    let (runtime, _workspace, durable) = active_sync_runtime(temporary.path(), test_ports()).await;
    let executor = Arc::new(GatedCancellationExecutor::default());
    let service = Arc::new(SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        executor.clone(),
    ));
    let config = SyncApiService::get_sync_config(service.as_ref())
        .await
        .unwrap();
    let request = TriggerSyncRunRequest {
        expected_config_revision: config.revision,
    };
    SyncApiService::trigger_sync_run(service.as_ref(), request.clone())
        .await
        .unwrap();
    executor.started.notified().await;

    let shutdown_service = service.clone();
    let mut shutdown = tokio::spawn(async move { shutdown_service.shutdown().await });
    tokio::time::timeout(
        Duration::from_secs(1),
        executor.cancellation_seen.notified(),
    )
    .await
    .expect("shutdown must cooperatively cancel the running executor");

    let rejected = SyncApiService::trigger_sync_run(service.as_ref(), request)
        .await
        .unwrap_err();
    assert_eq!(rejected.code(), ErrorCode::SyncRunUnavailable);
    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut shutdown)
            .await
            .is_err(),
        "shutdown must wait for the cancelled executor to settle"
    );

    executor.release.notify_one();
    tokio::time::timeout(Duration::from_secs(1), shutdown)
        .await
        .expect("shutdown must complete after the executor settles")
        .unwrap()
        .unwrap();
    runtime.wait_for_empty_sync_run_for_test().await.unwrap();
    let status = SyncApiService::get_sync_status(service.as_ref())
        .await
        .unwrap();
    assert_eq!(status.completion_state, SyncCompletionState::Failed);
    assert!(status
        .error
        .as_ref()
        .is_some_and(|error| error.code() == "cancelled"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sync_shutdown_waits_for_a_queued_background_task_to_settle() {
    let temporary = tempdir().unwrap();
    let spawner = Arc::new(DeferredTaskSpawner::default());
    let (runtime, service) = sync_service_with_policy(
        &temporary.path().join("shutdown-queued-run"),
        true,
        true,
        "automatic",
        test_ports_with_task_spawner(spawner.clone()),
        Arc::new(CountingExecutor::default()),
    )
    .await;
    let config = SyncApiService::get_sync_config(service.as_ref())
        .await
        .unwrap();
    let request = TriggerSyncRunRequest {
        expected_config_revision: config.revision,
    };
    SyncApiService::trigger_sync_run(service.as_ref(), request.clone())
        .await
        .unwrap();

    let shutdown_service = service.clone();
    let mut shutdown = tokio::spawn(async move { shutdown_service.shutdown().await });
    let rejected = SyncApiService::trigger_sync_run(service.as_ref(), request)
        .await
        .unwrap_err();
    assert_eq!(rejected.code(), ErrorCode::SyncRunUnavailable);
    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut shutdown)
            .await
            .is_err(),
        "shutdown must retain a queued run until its background task settles"
    );

    let background = tokio::spawn(spawner.take_task());
    tokio::time::timeout(Duration::from_secs(1), shutdown)
        .await
        .expect("queued run must drain after its task observes shutdown")
        .unwrap()
        .unwrap();
    background.await.unwrap();
    runtime.wait_for_empty_sync_run_for_test().await.unwrap();
    let status = SyncApiService::get_sync_status(service.as_ref())
        .await
        .unwrap();
    assert!(status
        .error
        .as_ref()
        .is_some_and(|error| error.code() == "cancelled"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_sync_shutdown_clones_share_one_idempotent_drain() {
    let temporary = tempdir().unwrap();
    let (runtime, _workspace, durable) = active_sync_runtime(temporary.path(), test_ports()).await;
    let executor = Arc::new(GatedCancellationExecutor::default());
    let service = Arc::new(SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        executor.clone(),
    ));
    let config = SyncApiService::get_sync_config(service.as_ref())
        .await
        .unwrap();
    SyncApiService::trigger_sync_run(
        service.as_ref(),
        TriggerSyncRunRequest {
            expected_config_revision: config.revision,
        },
    )
    .await
    .unwrap();
    executor.started.notified().await;

    let first_service = service.clone();
    let second_service = service.clone();
    let mut first = tokio::spawn(async move { first_service.shutdown().await });
    let mut second = tokio::spawn(async move { second_service.shutdown().await });
    tokio::time::timeout(
        Duration::from_secs(1),
        executor.cancellation_seen.notified(),
    )
    .await
    .expect("one shutdown clone must cancel the shared run");
    assert!(tokio::time::timeout(Duration::from_millis(50), &mut first)
        .await
        .is_err());
    assert!(tokio::time::timeout(Duration::from_millis(50), &mut second)
        .await
        .is_err());

    executor.release.notify_one();
    let (first, second) = tokio::join!(first, second);
    first.unwrap().unwrap();
    second.unwrap().unwrap();
    service.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn running_sync_blocks_workspace_commit_until_executor_cooperatively_drains() {
    let temporary = tempdir().unwrap();
    let (runtime, workspace, durable) = active_sync_runtime(temporary.path(), test_ports()).await;
    let executor = Arc::new(GatedCancellationExecutor::default());
    let service = Arc::new(SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        executor.clone(),
    ));
    let mut events = runtime.event_broker().subscribe();
    let config = SyncApiService::get_sync_config(service.as_ref())
        .await
        .unwrap();
    SyncApiService::trigger_sync_run(
        service.as_ref(),
        TriggerSyncRunRequest {
            expected_config_revision: config.revision,
        },
    )
    .await
    .unwrap();
    executor.started.notified().await;
    let attempting = events.recv().await.unwrap();
    assert!(matches!(
        attempting.event,
        DomainEvent::SyncStatusChanged { status }
            if status.completion_state == SyncCompletionState::Attempting
    ));

    let before = workspace.service.current().unwrap();
    let target = temporary.path().join("drained-workspace");
    std::fs::create_dir(&target).unwrap();
    let prepared = runtime.prepare_host_workspace_authority(&target).unwrap();
    workspace.store.observe_next_save();
    let switch_service = workspace.service.clone();
    let expected_revision = before.revision.clone();
    let mut switch = Box::pin(tokio::spawn(async move {
        switch_service
            .compare_and_set_host_workspace(&expected_revision, prepared, "Drained Workspace")
            .await
    }));

    tokio::select! {
        _ = executor.cancellation_seen.notified() => {}
        _ = workspace.store.wait_for_observed_save() => {
            panic!("workspace persistence started before the running executor observed cancellation")
        }
    }
    assert!(runtime.mutation_is_available_for_test());
    assert_eq!(workspace.service.current().unwrap(), before);
    poll_fn(|context| match switch.as_mut().poll(context) {
        std::task::Poll::Pending => std::task::Poll::Ready(()),
        std::task::Poll::Ready(_) => {
            panic!("workspace switch completed before the running executor drained")
        }
    })
    .await;

    executor.release.notify_one();
    let committed = switch.await.unwrap().unwrap();
    let cancelled = events.recv().await.unwrap();
    let workspace_changed = events.recv().await.unwrap();

    assert_eq!(workspace.service.current().unwrap(), committed);
    assert!(matches!(
        cancelled.event,
        DomainEvent::SyncStatusChanged { status }
            if status.completion_state == SyncCompletionState::Failed
                && status.error.as_ref().is_some_and(|error| error.code() == "cancelled")
    ));
    assert!(matches!(
        workspace_changed.event,
        DomainEvent::WorkspaceChanged { workspace } if workspace == committed
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn running_sync_blocks_atomic_workspace_commit_until_executor_drains() {
    let temporary = tempdir().unwrap();
    let (runtime, workspace, durable) = active_sync_runtime(temporary.path(), test_ports()).await;
    let executor = Arc::new(GatedCancellationExecutor::default());
    let sync = Arc::new(SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        executor.clone(),
    ));
    let mut events = runtime.event_broker().subscribe();
    let config = SyncApiService::get_sync_config(sync.as_ref())
        .await
        .unwrap();
    SyncApiService::trigger_sync_run(
        sync.as_ref(),
        TriggerSyncRunRequest {
            expected_config_revision: config.revision,
        },
    )
    .await
    .unwrap();
    executor.started.notified().await;
    let attempting = events.recv().await.unwrap();
    assert!(matches!(
        attempting.event,
        DomainEvent::SyncStatusChanged { status }
            if status.completion_state == SyncCompletionState::Attempting
    ));

    let before = workspace.service.current().unwrap();
    let target = temporary.path().join("atomic-drained-workspace");
    std::fs::create_dir(&target).unwrap();
    let prepared = runtime.prepare_host_workspace_authority(&target).unwrap();
    let transaction = MemoryHostWorkspaceTransaction {
        store: workspace.store.clone(),
        repository_binding: workspace.store.repository_binding(),
        authority_binding: prepared.binding(),
    };
    workspace.store.observe_next_save();
    let switch_service = workspace.service.clone();
    let expected_revision = before.revision;
    let switch = tokio::spawn(async move {
        switch_service
            .compare_and_set_host_workspace_transaction(
                &expected_revision,
                prepared,
                "Atomic Drained Workspace",
                Box::new(transaction),
            )
            .await
    });

    tokio::select! {
        _ = executor.cancellation_seen.notified() => {}
        _ = workspace.store.wait_for_observed_save() => {
            panic!("atomic host commit started before the running executor observed cancellation")
        }
    }
    assert!(runtime.mutation_is_available_for_test());
    executor.release.notify_one();
    let committed = switch.await.unwrap().unwrap();
    let cancelled = events.recv().await.unwrap();
    let workspace_changed = events.recv().await.unwrap();

    assert!(matches!(
        cancelled.event,
        DomainEvent::SyncStatusChanged { status }
            if status.completion_state == SyncCompletionState::Failed
                && status.error.as_ref().is_some_and(|error| error.code() == "cancelled")
    ));
    assert!(matches!(
        workspace_changed.event,
        DomainEvent::WorkspaceChanged { workspace } if workspace == committed
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_workspace_switch_waiter_does_not_strand_transition() {
    let temporary = tempdir().unwrap();
    let (runtime, workspace, durable) = active_sync_runtime(temporary.path(), test_ports()).await;
    let executor = Arc::new(GatedCancellationExecutor::default());
    let sync = Arc::new(SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        executor.clone(),
    ));
    let config = SyncApiService::get_sync_config(sync.as_ref())
        .await
        .unwrap();
    let request = TriggerSyncRunRequest {
        expected_config_revision: config.revision,
    };
    SyncApiService::trigger_sync_run(sync.as_ref(), request.clone())
        .await
        .unwrap();
    executor.started.notified().await;

    let before = workspace.service.current().unwrap();
    let target = temporary.path().join("abandoned-switch-target");
    std::fs::create_dir(&target).unwrap();
    let prepared = runtime.prepare_host_workspace_authority(&target).unwrap();
    let switch_service = workspace.service.clone();
    let expected_revision = before.revision.clone();
    let mut switch = tokio::spawn(async move {
        switch_service
            .compare_and_set_host_workspace(&expected_revision, prepared, "Abandoned Switch Target")
            .await
    });

    executor.cancellation_seen.notified().await;
    poll_fn(|context| match Pin::new(&mut switch).poll(context) {
        std::task::Poll::Pending => std::task::Poll::Ready(()),
        std::task::Poll::Ready(_) => {
            panic!("workspace switch escaped Phase B before the executor drained")
        }
    })
    .await;
    switch.abort();
    assert!(switch.await.unwrap_err().is_cancelled());

    executor.release.notify_one();
    runtime.wait_for_empty_sync_run_for_test().await.unwrap();
    assert_eq!(workspace.service.current().unwrap(), before);

    let accepted = SyncApiService::trigger_sync_run(sync.as_ref(), request)
        .await
        .unwrap();
    executor.started.notified().await;
    assert_eq!(
        SyncApiService::get_sync_status(sync.as_ref())
            .await
            .unwrap()
            .active_run_id
            .as_ref()
            .copied(),
        Some(accepted.run_id)
    );

    let cleanup = runtime
        .begin_sync_workspace_transition_for_test()
        .await
        .unwrap();
    executor.cancellation_seen.notified().await;
    executor.release.notify_one();
    cleanup.wait_drained().await.unwrap();
    cleanup.reopen_for_test().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sync_run_and_second_workspace_transition_are_rejected_while_transitioning() {
    let temporary = tempdir().unwrap();
    let (runtime, workspace, durable) = active_sync_runtime(temporary.path(), test_ports()).await;
    let executor = Arc::new(GatedCancellationExecutor::default());
    let sync = Arc::new(SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        executor.clone(),
    ));
    let config = SyncApiService::get_sync_config(sync.as_ref())
        .await
        .unwrap();
    let request = TriggerSyncRunRequest {
        expected_config_revision: config.revision,
    };
    SyncApiService::trigger_sync_run(sync.as_ref(), request.clone())
        .await
        .unwrap();
    executor.started.notified().await;

    let before = workspace.service.current().unwrap();
    let first_target = temporary.path().join("first-transition-target");
    std::fs::create_dir(&first_target).unwrap();
    let first_prepared = runtime
        .prepare_host_workspace_authority(&first_target)
        .unwrap();
    let first_service = workspace.service.clone();
    let first_revision = before.revision.clone();
    let first_switch = tokio::spawn(async move {
        first_service
            .compare_and_set_host_workspace(
                &first_revision,
                first_prepared,
                "First Transition Target",
            )
            .await
    });
    executor.cancellation_seen.notified().await;

    let run_error = SyncApiService::trigger_sync_run(sync.as_ref(), request)
        .await
        .unwrap_err();
    let second_transition = runtime.try_begin_sync_workspace_transition_for_test();

    assert_eq!(run_error.code(), ErrorCode::SyncRunUnavailable);
    assert!(second_transition.is_err());
    executor.release.notify_one();
    let committed = first_switch.await.unwrap().unwrap();
    assert_eq!(workspace.service.current().unwrap(), committed);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn running_connection_test_is_not_cancelled_by_workspace_transition() {
    let temporary = tempdir().unwrap();
    let (runtime, workspace, durable) = active_sync_runtime(temporary.path(), test_ports()).await;
    let executor = Arc::new(BlockingConnectionExecutor::default());
    let sync = Arc::new(SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        executor.clone(),
    ));
    let config = SyncApiService::get_sync_config(sync.as_ref())
        .await
        .unwrap();
    let expected_revision = config.revision.clone();
    let connection_service = sync.clone();
    let mut connection = tokio::spawn(async move {
        SyncApiService::test_sync_connection(
            connection_service.as_ref(),
            qingyu_kernel::contract::TestSyncConnectionRequest {
                expected_revision,
                changes: SyncConfigChangesDto::default(),
            },
        )
        .await
    });
    executor.connection_started.notified().await;

    switch_workspace(
        &runtime,
        &workspace,
        &temporary.path().join("connection-test-next-workspace"),
    )
    .await;
    poll_fn(|context| match Pin::new(&mut connection).poll(context) {
        std::task::Poll::Pending => std::task::Poll::Ready(()),
        std::task::Poll::Ready(_) => {
            panic!("workspace transition completed by cancelling the connection test")
        }
    })
    .await;

    executor.release_connection.notify_one();
    let tested = connection.await.unwrap().unwrap();
    assert_eq!(tested.config_revision, config.revision);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn successful_switch_admits_only_one_concurrent_run_bound_to_new_workspace() {
    let temporary = tempdir().unwrap();
    let spawner = Arc::new(CollectingDeferredTaskSpawner::default());
    let (runtime, workspace, durable) = active_sync_runtime(
        temporary.path(),
        test_ports_with_task_spawner(spawner.clone()),
    )
    .await;
    switch_workspace(
        &runtime,
        &workspace,
        &temporary.path().join("concurrent-trigger-workspace"),
    )
    .await;
    let workspace_b = workspace.service.current().unwrap();
    let executor = Arc::new(ContextBindingExecutor::default());
    let sync = Arc::new(SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        executor.clone(),
    ));
    let config = SyncApiService::get_sync_config(sync.as_ref())
        .await
        .unwrap();
    let request = TriggerSyncRunRequest {
        expected_config_revision: config.revision,
    };
    let barrier = Arc::new(Barrier::new(3));
    let first_service = sync.clone();
    let first_request = request.clone();
    let first_barrier = barrier.clone();
    let first = tokio::spawn(async move {
        first_barrier.wait().await;
        SyncApiService::trigger_sync_run(first_service.as_ref(), first_request).await
    });
    let second_service = sync.clone();
    let second_barrier = barrier.clone();
    let second = tokio::spawn(async move {
        second_barrier.wait().await;
        SyncApiService::trigger_sync_run(second_service.as_ref(), request).await
    });
    barrier.wait().await;

    let first = first.await.unwrap();
    let second = second.await.unwrap();
    let (_accepted, rejected) = match (first, second) {
        (Ok(accepted), Err(rejected)) | (Err(rejected), Ok(accepted)) => (accepted, rejected),
        (Ok(_), Ok(_)) => panic!("both concurrent sync runs were accepted"),
        (Err(first), Err(second)) => {
            panic!("both concurrent sync runs were rejected: {first:?}, {second:?}")
        }
    };
    assert_eq!(rejected.code(), ErrorCode::SyncRunUnavailable);

    assert_eq!(spawner.spawned.load(Ordering::SeqCst), 1);
    let mut background = tokio::spawn(spawner.take_only_task());
    tokio::select! {
        _ = executor.started.notified() => {}
        completed = &mut background => {
            panic!("the unique accepted run never reached its executor: {completed:?}")
        }
    }
    let observed = executor.observed.lock().unwrap().clone();
    assert_eq!(observed.len(), 1);
    assert_eq!(observed[0].0, workspace_b);
    assert_eq!(observed[0].3, SyncTrigger::Manual);
    executor.release.notify_one();
    background.await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn atomic_no_commit_reopens_old_workspace_sync_admission() {
    let temporary = tempdir().unwrap();
    let spawner = Arc::new(DeferredTaskSpawner::default());
    let (runtime, workspace, durable) = active_sync_runtime(
        temporary.path(),
        test_ports_with_task_spawner(spawner.clone()),
    )
    .await;
    let workspace_a = workspace.service.current().unwrap();
    let snapshot_a = runtime.active_workspace_snapshot().unwrap();
    let target = temporary.path().join("no-commit-workspace");
    std::fs::create_dir(&target).unwrap();
    let prepared = runtime.prepare_host_workspace_authority(&target).unwrap();
    let transaction = NoCommitHostWorkspaceTransaction {
        repository_binding: workspace.store.repository_binding(),
        authority_binding: prepared.binding(),
    };

    workspace
        .service
        .compare_and_set_host_workspace_transaction(
            &workspace_a.revision,
            prepared,
            "No Commit Workspace",
            Box::new(transaction),
        )
        .await
        .unwrap_err();
    assert_eq!(workspace.service.current().unwrap(), workspace_a);
    assert!(Arc::ptr_eq(
        &runtime.active_workspace_snapshot().unwrap(),
        &snapshot_a
    ));

    let executor = Arc::new(ContextBindingExecutor::default());
    let sync = SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        executor.clone(),
    );
    let config = SyncApiService::get_sync_config(&sync).await.unwrap();
    SyncApiService::trigger_sync_run(
        &sync,
        TriggerSyncRunRequest {
            expected_config_revision: config.revision,
        },
    )
    .await
    .unwrap();

    let background = tokio::spawn(spawner.take_task());
    executor.started.notified().await;
    let observed = executor.observed.lock().unwrap().clone();
    assert_eq!(observed.len(), 1);
    assert_eq!(observed[0].0, workspace_a);
    executor.release.notify_one();
    background.await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_switch_after_workspace_publication_reopens_the_committed_snapshot() {
    let temporary = tempdir().unwrap();
    let (runtime, workspace, _durable) = active_sync_runtime(temporary.path(), test_ports()).await;
    let paths = KernelPaths::desktop(
        &temporary.path().join("workspace"),
        &temporary.path().join("app-data"),
        &temporary.path().join("cache"),
    )
    .unwrap();
    let managed = ManagedWorkspaceCollection::from_paths(&paths).unwrap();
    let publication_gate = Arc::new(WorkspacePublicationGate::default());
    let switching_service = Arc::new(
        WorkspaceService::new(
            &runtime,
            workspace.store.clone(),
            managed,
            publication_gate.clone(),
            "Ignored",
        )
        .await
        .unwrap(),
    );
    let before = switching_service.current().unwrap();
    let target = temporary.path().join("published-workspace");
    std::fs::create_dir(&target).unwrap();
    let prepared = runtime.prepare_host_workspace_authority(&target).unwrap();
    let switch_service = switching_service.clone();
    let mut switch = tokio::spawn(async move {
        switch_service
            .compare_and_set_host_workspace(&before.revision, prepared, "Published Workspace")
            .await
    });

    publication_gate.wait_for_workspace_publication().await;
    let mutation = runtime.mutation_coordinator().lock().await;
    publication_gate.release_workspace_publication();
    runtime
        .wait_for_sync_workspace_publication_attempt_for_test()
        .await;
    poll_fn(|context| match Pin::new(&mut switch).poll(context) {
        std::task::Poll::Pending => std::task::Poll::Ready(()),
        std::task::Poll::Ready(_) => {
            panic!("switch completed while the final mutation gate was held")
        }
    })
    .await;

    switch.abort();
    assert!(switch.await.unwrap_err().is_cancelled());
    drop(mutation);

    assert_eq!(
        switching_service.current().unwrap().display_name,
        "Published Workspace"
    );
    let transition = runtime
        .begin_sync_workspace_transition_for_test()
        .await
        .unwrap();
    transition.wait_drained().await.unwrap();
    transition.reopen_for_test().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failed_final_reopen_validation_remains_closed_when_guard_drops() {
    let temporary = tempdir().unwrap();
    let (runtime, workspace, _durable) = active_sync_runtime(temporary.path(), test_ports()).await;
    let paths = KernelPaths::desktop(
        &temporary.path().join("workspace"),
        &temporary.path().join("app-data"),
        &temporary.path().join("cache"),
    )
    .unwrap();
    let managed = ManagedWorkspaceCollection::from_paths(&paths).unwrap();
    let publication_gate = Arc::new(WorkspacePublicationGate::default());
    let switching_service = Arc::new(
        WorkspaceService::new(
            &runtime,
            workspace.store.clone(),
            managed,
            publication_gate.clone(),
            "Ignored",
        )
        .await
        .unwrap(),
    );
    let before = switching_service.current().unwrap();
    let target = temporary.path().join("invalid-final-workspace");
    let displaced = temporary.path().join("invalid-final-workspace-displaced");
    std::fs::create_dir(&target).unwrap();
    let prepared = runtime.prepare_host_workspace_authority(&target).unwrap();
    let switch_service = switching_service.clone();
    let switch = tokio::spawn(async move {
        switch_service
            .compare_and_set_host_workspace(&before.revision, prepared, "Invalid Final Workspace")
            .await
    });

    publication_gate.wait_for_workspace_publication().await;
    std::fs::rename(&target, &displaced).unwrap();
    std::fs::create_dir(&target).unwrap();
    publication_gate.release_workspace_publication();

    assert!(switch.await.unwrap().is_err());
    std::fs::remove_dir(&target).unwrap();
    std::fs::rename(&displaced, &target).unwrap();
    assert!(runtime.active_workspace_snapshot().is_err());
    assert!(runtime
        .begin_sync_workspace_transition_for_test()
        .await
        .is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_prewrite_mismatch_switch_during_drain_stays_in_recovery() {
    let temporary = tempdir().unwrap();
    let (runtime, workspace, durable) = active_sync_runtime(temporary.path(), test_ports()).await;
    let executor = Arc::new(GatedCancellationExecutor::default());
    let sync = SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        executor.clone(),
    );
    let config = SyncApiService::get_sync_config(&sync).await.unwrap();
    SyncApiService::trigger_sync_run(
        &sync,
        TriggerSyncRunRequest {
            expected_config_revision: config.revision,
        },
    )
    .await
    .unwrap();
    executor.started.notified().await;

    let before = workspace.service.current().unwrap();
    let original_store = workspace.store.load().unwrap();
    workspace
        .store
        .replace(Some(serde_json::json!({"corrupt": true})))
        .unwrap();
    let target = temporary.path().join("mismatch-target");
    std::fs::create_dir(&target).unwrap();
    let prepared = runtime.prepare_host_workspace_authority(&target).unwrap();
    let switch_service = workspace.service.clone();
    let mut switch = tokio::spawn(async move {
        switch_service
            .compare_and_set_host_workspace(&before.revision, prepared, "Mismatch Target")
            .await
    });

    executor.cancellation_seen.notified().await;
    poll_fn(|context| match Pin::new(&mut switch).poll(context) {
        std::task::Poll::Pending => std::task::Poll::Ready(()),
        std::task::Poll::Ready(_) => {
            panic!("mismatched switch completed before the running executor drained")
        }
    })
    .await;
    switch.abort();
    assert!(switch.await.unwrap_err().is_cancelled());
    executor.release.notify_one();
    runtime.wait_for_empty_sync_run_for_test().await.unwrap();
    workspace.store.replace(original_store).unwrap();

    assert!(runtime.active_workspace_snapshot().is_err());
    assert!(runtime
        .begin_sync_workspace_transition_for_test()
        .await
        .is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn aborting_sync_during_mismatch_recovery_retains_the_candidate_lease() {
    let temporary = tempdir().unwrap();
    let spawner = Arc::new(DeferredTaskSpawner::default());
    let (runtime, workspace, durable) = active_sync_runtime(
        temporary.path(),
        test_ports_with_task_spawner(spawner.clone()),
    )
    .await;
    let executor = Arc::new(GatedCancellationExecutor::default());
    let sync = SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        executor.clone(),
    );
    let config = SyncApiService::get_sync_config(&sync).await.unwrap();
    SyncApiService::trigger_sync_run(
        &sync,
        TriggerSyncRunRequest {
            expected_config_revision: config.revision,
        },
    )
    .await
    .unwrap();
    let background = tokio::spawn(spawner.take_task());
    executor.started.notified().await;

    let before = workspace.service.current().unwrap();
    workspace
        .store
        .replace(Some(serde_json::json!({"corrupt": true})))
        .unwrap();
    let target = temporary.path().join("retained-mismatch-candidate");
    std::fs::create_dir(&target).unwrap();
    let prepared = runtime.prepare_host_workspace_authority(&target).unwrap();
    let switch_service = workspace.service.clone();
    let switch = tokio::spawn(async move {
        switch_service
            .compare_and_set_host_workspace(&before.revision, prepared, "Retained Candidate")
            .await
    });
    executor.cancellation_seen.notified().await;

    background.abort();
    assert!(background.await.unwrap_err().is_cancelled());
    assert!(switch.await.unwrap().is_err());

    let contender_app_data = temporary.path().join("candidate-contender-app-data");
    let contender_cache = temporary.path().join("candidate-contender-cache");
    std::fs::create_dir(&contender_app_data).unwrap();
    std::fs::create_dir(&contender_cache).unwrap();
    let contender_paths =
        KernelPaths::desktop(&target, &contender_app_data, &contender_cache).unwrap();
    let error = KernelRuntime::activate(
        KernelConfig::generate().unwrap(),
        contender_paths,
        KernelPorts::unavailable(),
    )
    .unwrap_err();
    assert_eq!(error.kind(), KernelStartupErrorKind::WorkspaceLocked);
}

async fn sync_service_with_policy(
    root: &std::path::Path,
    enabled: bool,
    complete: bool,
    mode: &str,
    ports: KernelPorts,
    executor: Arc<dyn SyncExecutor>,
) -> (Arc<KernelRuntime>, Arc<SyncService>) {
    std::fs::create_dir(root).unwrap();
    let (runtime, _workspace, durable) = active_sync_runtime(root, ports).await;
    let config_path = root.join("app-data/sync-config.json");
    let mut config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&config_path).unwrap()).unwrap();
    config["enabled"] = serde_json::json!(enabled);
    config["mode"] = serde_json::json!(mode);
    if !complete {
        config["s3"]["endpointUrl"] = serde_json::json!("");
    }
    std::fs::write(&config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    let service = Arc::new(SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        executor,
    ));
    (runtime, service)
}

#[tokio::test(start_paused = true)]
async fn pending_app_launch_run_reaches_a_safe_terminal_timeout_and_releases_admission() {
    let temporary = tempdir().unwrap();
    let executor = Arc::new(BlockingExecutor::default());
    let (_runtime, service) = sync_service_with_policy(
        &temporary.path().join("app-launch-timeout"),
        true,
        true,
        "automatic",
        test_ports(),
        executor.clone(),
    )
    .await;

    let (disposition, settlement) = service
        .trigger_kernel_sync(SyncTrigger::AppLaunch)
        .await
        .into_parts();
    let accepted = match disposition {
        KernelSyncTriggerDisposition::Accepted(accepted) => accepted,
        other => panic!("expected accepted app-launch trigger, got {other:?}"),
    };
    executor.started.notified().await;

    tokio::time::advance(Duration::from_secs(299)).await;
    tokio::task::yield_now().await;
    let attempting = SyncApiService::get_sync_status(service.as_ref())
        .await
        .unwrap();
    assert_eq!(attempting.completion_state, SyncCompletionState::Attempting);
    assert_eq!(attempting.active_run_id.as_ref(), Some(&accepted.run_id));

    tokio::time::advance(Duration::from_secs(1)).await;
    settlement.wait().await;

    let terminal = SyncApiService::get_sync_status(service.as_ref())
        .await
        .unwrap();
    assert_eq!(terminal.completion_state, SyncCompletionState::Failed);
    assert!(terminal.active_run_id.as_ref().is_none());
    assert_eq!(
        terminal.last_trigger.as_ref(),
        Some(&SyncTrigger::AppLaunch)
    );
    let error = terminal.error.as_ref().expect("safe terminal timeout");
    assert_eq!(error.code(), "request_failed");
    assert_eq!(error.category(), Some("network"));
    assert_eq!(error.operation(), "sync_run");
    assert_eq!(error.provider_error_code(), Some("RequestTimeout"));
    assert_eq!(error.run_id(), Some(accepted.run_id));

    let exact = SyncApiService::get_sync_run(service.as_ref(), accepted.run_id)
        .await
        .unwrap();
    assert_eq!(exact.completion_state, SyncRunCompletionState::Failed);
    assert_eq!(
        exact.error.as_ref().map(SyncSafeErrorDto::run_id),
        Some(Some(accepted.run_id))
    );

    let config = SyncApiService::get_sync_config(service.as_ref())
        .await
        .unwrap();
    SyncApiService::trigger_sync_run(
        service.as_ref(),
        TriggerSyncRunRequest {
            expected_config_revision: config.revision,
        },
    )
    .await
    .expect("the terminal launch timeout must release the next manual run");
    executor.started.notified().await;
    executor.release.notify_one();
    runtime_wait_for_idle(service.as_ref()).await;
}

#[tokio::test]
async fn kernel_trigger_flows_through_queue_status_events_and_executor_context_with_one_revision() {
    let temporary = tempdir().unwrap();
    let spawner = Arc::new(DeferredTaskSpawner::default());
    let executor = Arc::new(ContextBindingExecutor::default());
    let (runtime, service) = sync_service_with_policy(
        &temporary.path().join("trigger-flow"),
        true,
        true,
        "automatic",
        test_ports_with_task_spawner(spawner.clone()),
        executor.clone(),
    )
    .await;
    let mut events = runtime.event_broker().subscribe();

    let (disposition, settlement) = service
        .trigger_kernel_sync(SyncTrigger::Save)
        .await
        .into_parts();
    let accepted = match disposition {
        KernelSyncTriggerDisposition::Accepted(accepted) => accepted,
        other => panic!("expected accepted save trigger, got {other:?}"),
    };
    let attempting_event = events.recv().await.unwrap();
    let attempting = match attempting_event.event {
        DomainEvent::SyncStatusChanged { status } => status,
        other => panic!("expected attempting sync event, got {other:?}"),
    };
    assert_eq!(attempting.last_trigger.as_ref(), Some(&SyncTrigger::Save));
    assert_eq!(attempting.active_run_id.as_ref(), Some(&accepted.run_id));
    assert_eq!(
        attempting.config_revision.as_ref(),
        Some(&accepted.config_revision)
    );
    assert_eq!(attempting_event.revision, accepted.config_revision);

    let background = tokio::spawn(spawner.take_task());
    executor.started.notified().await;
    {
        let observed = executor.observed.lock().unwrap();
        assert_eq!(observed.len(), 1);
        assert_eq!(observed[0].2, accepted.run_id);
        assert_eq!(observed[0].3, SyncTrigger::Save);
    }
    let running = SyncApiService::get_sync_status(service.as_ref())
        .await
        .unwrap();
    assert_eq!(running.last_trigger.as_ref(), Some(&SyncTrigger::Save));
    assert_eq!(
        running.config_revision.as_ref(),
        Some(&attempting_event.revision)
    );

    executor.release.notify_one();
    settlement.wait().await;
    background.await.unwrap();
    let terminal = events.recv().await.unwrap();
    let terminal_status = match terminal.event {
        DomainEvent::SyncStatusChanged { status } => status,
        other => panic!("expected terminal sync event, got {other:?}"),
    };
    assert_eq!(terminal.revision, attempting_event.revision);
    assert_eq!(
        terminal_status.last_trigger.as_ref(),
        Some(&SyncTrigger::Save)
    );
    assert_eq!(terminal_status.active_run_id.as_ref(), None);
}

#[tokio::test]
async fn kernel_trigger_mode_matrix_is_exact_and_rejections_leave_status_idle() {
    let cases = [
        ("automatic", SyncTrigger::AppLaunch, true),
        ("automatic", SyncTrigger::Interval, true),
        ("automatic", SyncTrigger::Manual, true),
        ("automatic", SyncTrigger::Save, true),
        ("automatic", SyncTrigger::SettingsExit, true),
        ("startup-exit", SyncTrigger::AppLaunch, true),
        ("startup-exit", SyncTrigger::Interval, false),
        ("startup-exit", SyncTrigger::Manual, true),
        ("startup-exit", SyncTrigger::Save, false),
        ("startup-exit", SyncTrigger::SettingsExit, true),
        ("fully-manual", SyncTrigger::AppLaunch, false),
        ("fully-manual", SyncTrigger::Interval, false),
        ("fully-manual", SyncTrigger::Manual, true),
        ("fully-manual", SyncTrigger::Save, false),
        ("fully-manual", SyncTrigger::SettingsExit, false),
    ];
    let temporary = tempdir().unwrap();

    for (index, (mode, trigger, allowed)) in cases.into_iter().enumerate() {
        let executor = Arc::new(CountingExecutor::default());
        let (_runtime, service) = sync_service_with_policy(
            &temporary.path().join(format!("mode-{index}")),
            true,
            true,
            mode,
            test_ports(),
            executor.clone(),
        )
        .await;
        let (disposition, settlement) = service.trigger_kernel_sync(trigger).await.into_parts();
        if allowed {
            assert!(
                matches!(disposition, KernelSyncTriggerDisposition::Accepted(_)),
                "{mode} rejected {trigger:?}: {disposition:?}"
            );
            settlement.wait().await;
            assert_eq!(
                executor.runs.load(Ordering::SeqCst),
                1,
                "{mode} {trigger:?}"
            );
        } else {
            assert_eq!(
                disposition,
                KernelSyncTriggerDisposition::Rejected(KernelSyncTriggerRejection::ModeDisallowed),
                "{mode} {trigger:?}"
            );
            settlement.wait().await;
            assert_eq!(
                executor.runs.load(Ordering::SeqCst),
                0,
                "{mode} {trigger:?}"
            );
            let status = SyncApiService::get_sync_status(service.as_ref())
                .await
                .unwrap();
            assert_eq!(status.completion_state, SyncCompletionState::Idle);
            assert!(status.last_trigger.as_ref().is_none());
        }
    }
}

#[tokio::test]
async fn disabled_incomplete_active_and_closing_kernel_triggers_reject_without_status_drift() {
    let temporary = tempdir().unwrap();
    for (name, enabled, complete, rejection) in [
        (
            "disabled",
            false,
            true,
            KernelSyncTriggerRejection::Disabled,
        ),
        (
            "incomplete",
            true,
            false,
            KernelSyncTriggerRejection::Incomplete,
        ),
    ] {
        let executor = Arc::new(CountingExecutor::default());
        let (_runtime, service) = sync_service_with_policy(
            &temporary.path().join(name),
            enabled,
            complete,
            "automatic",
            test_ports(),
            executor.clone(),
        )
        .await;
        let (disposition, settlement) = service
            .trigger_kernel_sync(SyncTrigger::AppLaunch)
            .await
            .into_parts();
        assert_eq!(
            disposition,
            KernelSyncTriggerDisposition::Rejected(rejection)
        );
        settlement.wait().await;
        assert_eq!(executor.runs.load(Ordering::SeqCst), 0);
        let status = SyncApiService::get_sync_status(service.as_ref())
            .await
            .unwrap();
        assert_eq!(status.completion_state, SyncCompletionState::Idle);
        assert!(status.last_trigger.as_ref().is_none());
    }

    let executor = Arc::new(BlockingExecutor::default());
    let (_runtime, service) = sync_service_with_policy(
        &temporary.path().join("active"),
        true,
        true,
        "automatic",
        test_ports(),
        executor.clone(),
    )
    .await;
    let revision = SyncApiService::get_sync_config(service.as_ref())
        .await
        .unwrap()
        .revision;
    SyncApiService::trigger_sync_run(
        service.as_ref(),
        TriggerSyncRunRequest {
            expected_config_revision: revision,
        },
    )
    .await
    .unwrap();
    executor.started.notified().await;
    let (active_disposition, active_settlement) = service
        .trigger_kernel_sync(SyncTrigger::Save)
        .await
        .into_parts();
    assert_eq!(
        active_disposition,
        KernelSyncTriggerDisposition::Rejected(KernelSyncTriggerRejection::ActiveRun)
    );
    active_settlement.wait().await;
    let active_status = SyncApiService::get_sync_status(service.as_ref())
        .await
        .unwrap();
    assert_eq!(
        active_status.last_trigger.as_ref(),
        Some(&SyncTrigger::Manual)
    );
    assert_eq!(executor.runs.load(Ordering::SeqCst), 1);
    executor.release.notify_one();
    runtime_wait_for_idle(service.as_ref()).await;

    service.close_kernel_triggers();
    let (closing_disposition, closing_settlement) = service
        .trigger_kernel_sync(SyncTrigger::SettingsExit)
        .await
        .into_parts();
    assert_eq!(
        closing_disposition,
        KernelSyncTriggerDisposition::Rejected(KernelSyncTriggerRejection::Closing)
    );
    closing_settlement.wait().await;

    let revision = SyncApiService::get_sync_config(service.as_ref())
        .await
        .unwrap()
        .revision;
    SyncApiService::trigger_sync_run(
        service.as_ref(),
        TriggerSyncRunRequest {
            expected_config_revision: revision,
        },
    )
    .await
    .unwrap();
    executor.started.notified().await;
    executor.release.notify_one();
    runtime_wait_for_idle(service.as_ref()).await;
    assert_eq!(executor.runs.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn http_sync_run_rejects_a_forged_trigger_and_still_runs_only_manual() {
    let temporary = tempdir().unwrap();
    let executor = Arc::new(ContextBindingExecutor::default());
    let (runtime, service) = sync_service_with_policy(
        &temporary.path().join("http-trigger"),
        true,
        true,
        "automatic",
        test_ports(),
        executor.clone(),
    )
    .await;
    let credential = runtime.expose_native_launch_credential().to_owned();
    let revision = SyncApiService::get_sync_config(service.as_ref())
        .await
        .unwrap()
        .revision;
    runtime.install_sync_api_service(service.clone()).unwrap();
    let router = build_router(
        runtime.clone(),
        TransportPolicy::loopback(HOST, ORIGIN).unwrap(),
    );

    let forged = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/sync/runs")
                .header(header::HOST, HOST)
                .header(header::AUTHORIZATION, format!("Bearer {credential}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "expectedConfigRevision": revision,
                        "trigger": "save"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(forged.status(), StatusCode::BAD_REQUEST);
    assert!(executor.observed.lock().unwrap().is_empty());

    let manual = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/sync/runs")
                .header(header::HOST, HOST)
                .header(header::AUTHORIZATION, format!("Bearer {credential}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&TriggerSyncRunRequest {
                        expected_config_revision: revision,
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(manual.status(), StatusCode::ACCEPTED);
    tokio::time::timeout(Duration::from_secs(1), executor.started.notified())
        .await
        .expect("accepted HTTP sync run must start its executor");
    assert_eq!(executor.observed.lock().unwrap()[0].3, SyncTrigger::Manual);
    executor.release.notify_one();
}

#[tokio::test]
async fn kernel_trigger_settlement_covers_spawner_rejection_and_terminal_failure() {
    let temporary = tempdir().unwrap();
    let failing_spawner = Arc::new(FailingTaskSpawner::default());
    let (_runtime, rejected_service) = sync_service_with_policy(
        &temporary.path().join("spawn-rejected"),
        true,
        true,
        "automatic",
        test_ports_with_task_spawner(failing_spawner.clone()),
        Arc::new(CountingExecutor::default()),
    )
    .await;

    let (disposition, settlement) = rejected_service
        .trigger_kernel_sync(SyncTrigger::SettingsExit)
        .await
        .into_parts();
    assert_eq!(
        disposition,
        KernelSyncTriggerDisposition::Rejected(KernelSyncTriggerRejection::Unavailable)
    );
    tokio::time::timeout(Duration::from_secs(1), settlement.wait())
        .await
        .expect("spawner rejection must settle the barrier");
    assert_eq!(failing_spawner.attempts.load(Ordering::SeqCst), 1);

    let executor = Arc::new(ManualExecutor::default());
    executor.fail.store(true, Ordering::SeqCst);
    let (_runtime, failed_service) = sync_service_with_policy(
        &temporary.path().join("executor-failed"),
        true,
        true,
        "automatic",
        test_ports(),
        executor,
    )
    .await;
    let (disposition, settlement) = failed_service
        .trigger_kernel_sync(SyncTrigger::SettingsExit)
        .await
        .into_parts();
    assert!(matches!(
        disposition,
        KernelSyncTriggerDisposition::Accepted(_)
    ));
    tokio::time::timeout(Duration::from_secs(1), settlement.wait())
        .await
        .expect("executor failure must settle after terminal status");
    let status = SyncApiService::get_sync_status(failed_service.as_ref())
        .await
        .unwrap();
    assert_eq!(status.completion_state, SyncCompletionState::Failed);
    assert!(status.active_run_id.as_ref().is_none());
}

#[tokio::test]
async fn kernel_trigger_settlement_covers_unpolled_and_running_task_drop() {
    let temporary = tempdir().unwrap();
    let queued_spawner = Arc::new(DeferredTaskSpawner::default());
    let (queued_runtime, queued_service) = sync_service_with_policy(
        &temporary.path().join("queued-drop"),
        true,
        true,
        "automatic",
        test_ports_with_task_spawner(queued_spawner.clone()),
        Arc::new(CountingExecutor::default()),
    )
    .await;
    let (disposition, settlement) = queued_service
        .trigger_kernel_sync(SyncTrigger::SettingsExit)
        .await
        .into_parts();
    assert!(matches!(
        disposition,
        KernelSyncTriggerDisposition::Accepted(_)
    ));
    drop(queued_spawner.take_task());
    tokio::time::timeout(Duration::from_secs(1), settlement.wait())
        .await
        .expect("dropping a queued task must settle the barrier");
    queued_runtime
        .wait_for_empty_sync_run_for_test()
        .await
        .unwrap();

    let running_spawner = Arc::new(DeferredTaskSpawner::default());
    let executor = Arc::new(BlockingExecutor::default());
    let (_runtime, running_service) = sync_service_with_policy(
        &temporary.path().join("running-drop"),
        true,
        true,
        "automatic",
        test_ports_with_task_spawner(running_spawner.clone()),
        executor.clone(),
    )
    .await;
    let (disposition, settlement) = running_service
        .trigger_kernel_sync(SyncTrigger::SettingsExit)
        .await
        .into_parts();
    assert!(matches!(
        disposition,
        KernelSyncTriggerDisposition::Accepted(_)
    ));
    let task = tokio::spawn(running_spawner.take_task());
    executor.started.notified().await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    tokio::time::timeout(Duration::from_secs(1), settlement.wait())
        .await
        .expect("dropping a running task must settle the barrier");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn kernel_trigger_settlement_covers_workspace_cancellation_terminal() {
    let temporary = tempdir().unwrap();
    let executor = Arc::new(GatedCancellationExecutor::default());
    let (runtime, service) = sync_service_with_policy(
        &temporary.path().join("cancelled"),
        true,
        true,
        "automatic",
        test_ports(),
        executor.clone(),
    )
    .await;
    let (disposition, settlement) = service
        .trigger_kernel_sync(SyncTrigger::SettingsExit)
        .await
        .into_parts();
    assert!(matches!(
        disposition,
        KernelSyncTriggerDisposition::Accepted(_)
    ));
    executor.started.notified().await;

    let transition_runtime = runtime.clone();
    let transition = tokio::spawn(async move {
        transition_runtime
            .begin_sync_workspace_transition_for_test()
            .await
    });
    executor.cancellation_seen.notified().await;
    executor.release.notify_one();
    tokio::time::timeout(Duration::from_secs(1), settlement.wait())
        .await
        .expect("workspace cancellation must settle after terminal status");
    let transition = transition.await.unwrap().unwrap();
    let status = SyncApiService::get_sync_status(service.as_ref())
        .await
        .unwrap();
    assert_eq!(status.completion_state, SyncCompletionState::Failed);
    assert_eq!(
        status.error.as_ref().map(|error| error.code()),
        Some("cancelled")
    );
    transition.reopen_for_test().await.unwrap();
}

#[test]
fn attempting_status_callback_can_close_kernel_triggers_without_deadlock() {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .arg("--ignored")
        .arg("--exact")
        .arg("attempting_status_reentrant_close_child")
        .arg("--test-threads=1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "reentrant close child failed: {status}");
            return;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let _status = child.wait().unwrap();
            panic!("Attempting event callback deadlocked while closing Kernel triggers");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[tokio::test]
#[ignore = "child-process helper"]
async fn attempting_status_reentrant_close_child() {
    let temporary = tempdir().unwrap();
    let sink = Arc::new(AttemptingReentrantCloseSink::default());
    let (_runtime, service) = sync_service_with_policy(
        &temporary.path().join("attempting-reentrant-close"),
        true,
        true,
        "automatic",
        test_ports_with_event_sink(sink.clone()),
        Arc::new(CountingExecutor::default()),
    )
    .await;
    sink.bind_service(&service);

    wait_for_accepted_kernel_trigger(service.trigger_kernel_sync(SyncTrigger::AppLaunch).await)
        .await;
    assert!(sink.close_returned.load(Ordering::Acquire));
    assert_kernel_trigger_is_closing(service.trigger_kernel_sync(SyncTrigger::Save).await).await;
}

#[tokio::test]
async fn kernel_sync_scheduler_exposes_explicit_host_seams_and_closes_idempotently() {
    let temporary = tempdir().unwrap();
    let sleeper = Arc::new(ControlledSleeper::default());
    let executor = Arc::new(TriggerRecordingExecutor::default());
    let (_runtime, service) = sync_service_with_policy(
        &temporary.path().join("scheduler-seams"),
        true,
        true,
        "automatic",
        test_ports_with_sleeper(sleeper.clone()),
        executor.clone(),
    )
    .await;
    let scheduler = KernelSyncScheduler::start(service).unwrap();
    let pending_interval = sleeper.next_request().await;

    wait_for_accepted_kernel_trigger(scheduler.app_launch().await).await;
    wait_for_accepted_kernel_trigger(scheduler.save().await).await;
    wait_for_accepted_kernel_trigger(scheduler.settings_exit().await).await;
    assert_eq!(
        executor.triggers.lock().unwrap().as_slice(),
        [
            SyncTrigger::AppLaunch,
            SyncTrigger::Save,
            SyncTrigger::SettingsExit
        ]
    );

    scheduler.close().await;
    assert!(
        !pending_interval.fire(),
        "closing must cancel the outstanding interval sleep"
    );
    tokio::time::timeout(Duration::from_secs(1), scheduler.close())
        .await
        .expect("closing an already closed scheduler must be waitable and idempotent");
    let (disposition, settlement) = scheduler.save().await.into_parts();
    assert_eq!(
        disposition,
        KernelSyncTriggerDisposition::Rejected(KernelSyncTriggerRejection::Closing)
    );
    settlement.wait().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn scheduler_close_is_not_observable_before_its_service_gate_is_closed() {
    let temporary = tempdir().unwrap();
    let sleeper = Arc::new(ControlledSleeper::default());
    let close_hook = Arc::new(KernelSyncSchedulerCloseTestHook::default());
    let (_runtime, service) = sync_service_with_policy(
        &temporary.path().join("scheduler-close-linearization"),
        true,
        true,
        "automatic",
        test_ports_with_sleeper(sleeper.clone()),
        Arc::new(CountingExecutor::default()),
    )
    .await;
    let scheduler = Arc::new(
        KernelSyncScheduler::start_with_close_hook_for_test(service.clone(), close_hook.clone())
            .unwrap(),
    );
    let pending_interval = sleeper.next_request().await;

    let first_scheduler = scheduler.clone();
    let first_close = tokio::spawn(async move {
        first_scheduler.close().await;
    });
    close_hook.wait_until_blocked().await;
    assert!(
        pending_interval.fire(),
        "the blocked close must leave the scheduler task available to observe close state"
    );

    let second_scheduler = scheduler.clone();
    let mut second_close = tokio::spawn(async move {
        second_scheduler.close().await;
    });
    let early_second_close =
        tokio::time::timeout(Duration::from_millis(100), &mut second_close).await;
    let second_close_returned_before_gate = early_second_close.is_ok();

    close_hook.release();
    first_close.await.unwrap();
    match early_second_close {
        Ok(result) => result.unwrap(),
        Err(_) => second_close.await.unwrap(),
    }

    assert!(
        !second_close_returned_before_gate,
        "a concurrent close must not observe scheduler end before the service gate closes"
    );
    assert_kernel_trigger_is_closing(scheduler.app_launch().await).await;
    assert_kernel_trigger_is_closing(scheduler.save().await).await;
    assert_kernel_trigger_is_closing(scheduler.settings_exit().await).await;
    assert_kernel_trigger_is_closing(service.trigger_kernel_sync(SyncTrigger::Interval).await)
        .await;
}

#[tokio::test]
async fn kernel_sync_scheduler_rejects_a_task_spawner_that_drops_the_scheduler_task() {
    let temporary = tempdir().unwrap();
    let (_runtime, service) = sync_service_with_policy(
        &temporary.path().join("scheduler-spawn-drop"),
        true,
        true,
        "automatic",
        test_ports_with_task_spawner(Arc::new(DroppingSuccessfulTaskSpawner)),
        Arc::new(CountingExecutor::default()),
    )
    .await;

    KernelSyncScheduler::start(service.clone()).unwrap_err();
    let (disposition, settlement) = service
        .trigger_kernel_sync(SyncTrigger::AppLaunch)
        .await
        .into_parts();
    assert_eq!(
        disposition,
        KernelSyncTriggerDisposition::Rejected(KernelSyncTriggerRejection::Closing)
    );
    settlement.wait().await;
}

#[tokio::test]
async fn kernel_sync_scheduler_task_drop_closes_every_internal_trigger_seam() {
    let temporary = tempdir().unwrap();
    let spawner = Arc::new(DeferredTaskSpawner::default());
    let (_runtime, service) = sync_service_with_policy(
        &temporary.path().join("scheduler-delayed-task-drop"),
        true,
        true,
        "automatic",
        test_ports_with_task_spawner(spawner.clone()),
        Arc::new(CountingExecutor::default()),
    )
    .await;
    let scheduler = KernelSyncScheduler::start(service.clone()).unwrap();

    drop(spawner.take_task());

    assert_kernel_trigger_is_closing(scheduler.app_launch().await).await;
    assert_kernel_trigger_is_closing(scheduler.save().await).await;
    assert_kernel_trigger_is_closing(scheduler.settings_exit().await).await;
    assert_kernel_trigger_is_closing(service.trigger_kernel_sync(SyncTrigger::Interval).await)
        .await;
}

#[tokio::test]
async fn dropping_the_kernel_sync_scheduler_handle_closes_its_owned_service_gate() {
    let temporary = tempdir().unwrap();
    let sleeper = Arc::new(ControlledSleeper::default());
    let (_runtime, service) = sync_service_with_policy(
        &temporary.path().join("scheduler-handle-drop"),
        true,
        true,
        "automatic",
        test_ports_with_sleeper(sleeper.clone()),
        Arc::new(CountingExecutor::default()),
    )
    .await;
    let scheduler = KernelSyncScheduler::start(service.clone()).unwrap();
    let pending = sleeper.next_request().await;

    drop(scheduler);

    tokio::time::timeout(Duration::from_secs(1), pending.wait_cancelled())
        .await
        .expect("dropping the owner handle must stop its scheduler task");
    assert_kernel_trigger_is_closing(service.trigger_kernel_sync(SyncTrigger::Save).await).await;
}

#[tokio::test]
async fn kernel_sync_scheduler_has_one_service_owner_and_rejected_starts_do_not_close_it() {
    let temporary = tempdir().unwrap();
    let sleeper = Arc::new(ControlledSleeper::default());
    let executor = Arc::new(CountingExecutor::default());
    let (_runtime, service) = sync_service_with_policy(
        &temporary.path().join("scheduler-unique-owner"),
        true,
        true,
        "automatic",
        test_ports_with_sleeper(sleeper.clone()),
        executor.clone(),
    )
    .await;
    let owner = KernelSyncScheduler::start(service.clone()).unwrap();
    let pending = sleeper.next_request().await;

    KernelSyncScheduler::start(service.clone()).unwrap_err();
    wait_for_accepted_kernel_trigger(owner.app_launch().await).await;
    assert_eq!(executor.runs.load(Ordering::SeqCst), 1);

    owner.close().await;
    assert!(!pending.fire());
    assert_kernel_trigger_is_closing(service.trigger_kernel_sync(SyncTrigger::Save).await).await;
}

#[tokio::test]
async fn kernel_sync_scheduler_uses_only_its_sync_service_runtime() {
    let temporary = tempdir().unwrap();
    let service_sleeper = Arc::new(ControlledSleeper::default());
    let (_service_runtime, service) = sync_service_with_policy(
        &temporary.path().join("scheduler-service-runtime"),
        true,
        true,
        "automatic",
        test_ports_with_sleeper(service_sleeper.clone()),
        Arc::new(CountingExecutor::default()),
    )
    .await;
    let foreign_sleeper = Arc::new(ControlledSleeper::default());
    let (_foreign_runtime, _foreign_service) = sync_service_with_policy(
        &temporary.path().join("scheduler-foreign-runtime"),
        true,
        true,
        "automatic",
        test_ports_with_sleeper(foreign_sleeper.clone()),
        Arc::new(CountingExecutor::default()),
    )
    .await;

    let scheduler = KernelSyncScheduler::start(service).unwrap();
    let pending = service_sleeper.next_request().await;
    assert!(
        tokio::time::timeout(Duration::from_millis(50), foreign_sleeper.next_request())
            .await
            .is_err(),
        "a scheduler must not read ports from another Kernel runtime"
    );
    scheduler.close().await;
    assert!(!pending.fire());
}

#[tokio::test]
async fn kernel_sync_scheduler_reloads_revision_and_replaces_one_interval_timer_on_config_changes()
{
    let temporary = tempdir().unwrap();
    let sleeper = Arc::new(ControlledSleeper::default());
    let executor = Arc::new(TriggerRecordingExecutor::default());
    let (runtime, service) = sync_service_with_policy(
        &temporary.path().join("scheduler-refresh"),
        true,
        true,
        "automatic",
        test_ports_with_sleeper(sleeper.clone()),
        executor.clone(),
    )
    .await;
    let scheduler = KernelSyncScheduler::start(service.clone()).unwrap();

    let first = sleeper.next_request().await;
    assert_eq!(first.duration, Duration::from_secs(30));
    let before = SyncApiService::get_sync_config(service.as_ref())
        .await
        .unwrap();
    let sixty_seconds = SyncApiService::patch_sync_config(
        service.as_ref(),
        PatchSyncConfigRequest {
            expected_revision: before.revision,
            changes: SyncConfigChangesDto {
                interval_seconds: Some(SyncIntervalSeconds::new(60).unwrap()),
                ..SyncConfigChangesDto::default()
            },
        },
    )
    .await
    .unwrap();
    let second = sleeper.next_request().await;
    assert_eq!(second.duration, Duration::from_secs(60));
    assert!(!first.fire(), "the superseded interval must be cancelled");

    let fully_manual = SyncApiService::patch_sync_config(
        service.as_ref(),
        PatchSyncConfigRequest {
            expected_revision: sixty_seconds.revision,
            changes: SyncConfigChangesDto {
                mode: Some(qingyu_kernel::contract::SyncMode::FullyManual),
                ..SyncConfigChangesDto::default()
            },
        },
    )
    .await
    .unwrap();
    tokio::time::timeout(Duration::from_secs(1), second.wait_cancelled())
        .await
        .expect("mode changes must cancel the old timer");
    assert!(
        tokio::time::timeout(Duration::from_millis(50), sleeper.next_request())
            .await
            .is_err(),
        "fully-manual mode must not retain or create an interval timer"
    );

    let automatic = SyncApiService::patch_sync_config(
        service.as_ref(),
        PatchSyncConfigRequest {
            expected_revision: fully_manual.revision,
            changes: SyncConfigChangesDto {
                mode: Some(qingyu_kernel::contract::SyncMode::Automatic),
                ..SyncConfigChangesDto::default()
            },
        },
    )
    .await
    .unwrap();
    let current = sleeper.next_request().await;
    assert_eq!(current.duration, Duration::from_secs(60));
    let mut events = runtime.event_broker().subscribe();
    assert!(current.fire());
    executor.completed.notified().await;
    let attempting = events.recv().await.unwrap();
    let terminal = events.recv().await.unwrap();
    assert_eq!(attempting.revision, automatic.revision);
    assert_eq!(terminal.revision, automatic.revision);
    assert!(matches!(
        attempting.event,
        DomainEvent::SyncStatusChanged { ref status }
            if status.last_trigger.as_ref() == Some(&SyncTrigger::Interval)
                && status.completion_state == SyncCompletionState::Attempting
    ));
    assert!(matches!(
        terminal.event,
        DomainEvent::SyncStatusChanged { ref status }
            if status.last_trigger.as_ref() == Some(&SyncTrigger::Interval)
                && status.completion_state == SyncCompletionState::Succeeded
    ));
    assert_eq!(
        executor.triggers.lock().unwrap().as_slice(),
        [SyncTrigger::Interval]
    );

    let next = sleeper.next_request().await;
    scheduler.close().await;
    assert!(!next.fire());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn kernel_sync_scheduler_rejects_an_expired_timer_before_config_change_publication() {
    let temporary = tempdir().unwrap();
    let publication_gate = Arc::new(SyncConfigPublicationGate::default());
    let sleeper = Arc::new(ControlledSleeper::default());
    let executor = Arc::new(TriggerRecordingExecutor::default());
    let (_runtime, service) = sync_service_with_policy(
        &temporary.path().join("scheduler-stale-timer"),
        true,
        true,
        "automatic",
        test_ports_with_event_sink_and_sleeper(publication_gate.clone(), sleeper.clone()),
        executor.clone(),
    )
    .await;
    let scheduler = KernelSyncScheduler::start(service.clone()).unwrap();
    let expired = sleeper.next_request().await;
    let before = SyncApiService::get_sync_config(service.as_ref())
        .await
        .unwrap();
    let patch_service = service.clone();
    let patch = tokio::spawn(async move {
        SyncApiService::patch_sync_config(
            patch_service.as_ref(),
            PatchSyncConfigRequest {
                expected_revision: before.revision,
                changes: SyncConfigChangesDto {
                    interval_seconds: Some(SyncIntervalSeconds::new(60).unwrap()),
                    ..SyncConfigChangesDto::default()
                },
            },
        )
        .await
    });
    publication_gate.wait_for_sync_config_publication().await;
    assert!(expired.fire());
    sleeper.wait_for_completion().await;
    publication_gate.release_sync_config_publication();
    let patched = patch.await.unwrap().unwrap();
    let replacement = tokio::time::timeout(Duration::from_secs(1), sleeper.next_request())
        .await
        .expect("the stale generation must settle and reload the installed config");
    let runs_after_replacement = executor.triggers.lock().unwrap().clone();

    assert!(
        runs_after_replacement.is_empty(),
        "an expired 30-second timer ran immediately against revision {}",
        patched.revision.as_str()
    );
    assert_eq!(replacement.duration, Duration::from_secs(60));
    scheduler.close().await;
    assert!(!replacement.fire());
}

#[tokio::test]
async fn kernel_sync_scheduler_waits_for_interval_settlement_before_arming_the_next_timer() {
    let temporary = tempdir().unwrap();
    let sleeper = Arc::new(ControlledSleeper::default());
    let executor = Arc::new(BlockingExecutor::default());
    let (_runtime, service) = sync_service_with_policy(
        &temporary.path().join("scheduler-singleflight"),
        true,
        true,
        "automatic",
        test_ports_with_sleeper(sleeper.clone()),
        executor.clone(),
    )
    .await;
    let scheduler = KernelSyncScheduler::start(service).unwrap();

    assert!(sleeper.next_request().await.fire());
    executor.started.notified().await;
    assert!(
        tokio::time::timeout(Duration::from_millis(50), sleeper.next_request())
            .await
            .is_err(),
        "a running interval sync must not overlap a second scheduler timer"
    );
    executor.release.notify_one();
    let next = sleeper.next_request().await;
    scheduler.close().await;
    assert!(!next.fire());
    assert_eq!(executor.runs.load(Ordering::SeqCst), 1);
}

async fn runtime_wait_for_idle(service: &SyncService) {
    for _ in 0..100 {
        if SyncApiService::get_sync_status(service)
            .await
            .unwrap()
            .active_run_id
            .as_ref()
            .is_none()
        {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("sync run did not settle");
}

async fn wait_for_accepted_kernel_trigger(
    result: qingyu_kernel::services::sync::KernelSyncTriggerResult,
) {
    let (disposition, settlement) = result.into_parts();
    assert!(
        matches!(disposition, KernelSyncTriggerDisposition::Accepted(_)),
        "explicit scheduler seam was rejected: {disposition:?}"
    );
    settlement.wait().await;
}

async fn assert_kernel_trigger_is_closing(
    result: qingyu_kernel::services::sync::KernelSyncTriggerResult,
) {
    let (disposition, settlement) = result.into_parts();
    assert_eq!(
        disposition,
        KernelSyncTriggerDisposition::Rejected(KernelSyncTriggerRejection::Closing)
    );
    settlement.wait().await;
}
