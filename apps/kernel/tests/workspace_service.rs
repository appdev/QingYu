use std::{
    fs,
    future::{poll_fn, Future as _},
    path::PathBuf,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc, Mutex, Weak,
    },
    time::Duration,
};

use async_trait::async_trait;
use qingyu_kernel::{
    config::KernelConfig,
    contract::{
        DomainEvent, ResourceRefDto, Revision, Rfc3339Utc, TriggerSyncRunRequest, WorkspaceDto,
    },
    events::{EventPublication, EventSink, EventSinkError},
    paths::KernelPaths,
    ports::{
        BoxSleepFuture, BoxTaskFuture, Clock, CredentialSecret, CredentialSlot, CredentialStore,
        DiagnosticRecord, DiagnosticsSink, KernelPorts, NetworkReachability, PortError, Sleeper,
        TaskSpawner,
    },
    runtime::{
        KernelRuntime, KernelStartupErrorKind, SyncApiService, WorkspaceApiService,
        WorkspaceAuthorityErrorKind,
    },
    services::{
        sync::{SyncExecutionError, SyncExecutor, SyncRunContext, SyncService},
        workspace::{WorkspaceService, WorkspaceServiceErrorKind},
    },
    storage::DurableFileStore,
    sync::config::{SyncConfig, SyncConfigStore},
    workspace::{
        managed::ManagedWorkspaceCollection,
        primary::{
            AtomicHostWorkspaceCommitError, AtomicHostWorkspaceTransaction,
            PreparedWorkspaceAuthorityBinding, PrimaryWorkspaceRepositoryBinding,
            PrimaryWorkspaceStore, PrimaryWorkspaceStoreError,
        },
    },
};
use serde_json::Value;
use tempfile::tempdir;
use tokio::sync::Notify;

#[derive(Default)]
struct MemoryPrimaryWorkspaceStore {
    binding: PrimaryWorkspaceRepositoryBinding,
    value: Mutex<Option<Value>>,
    durable: Mutex<Option<Value>>,
    fail_next_save: AtomicBool,
    fail_replace_on_call: std::sync::atomic::AtomicUsize,
    replace_target_on_next_save: Mutex<Option<(PathBuf, PathBuf)>>,
    loads: std::sync::atomic::AtomicUsize,
    replaces: std::sync::atomic::AtomicUsize,
    saves: std::sync::atomic::AtomicUsize,
    order: Arc<Mutex<Vec<&'static str>>>,
}

impl PrimaryWorkspaceStore for MemoryPrimaryWorkspaceStore {
    fn repository_binding(&self) -> PrimaryWorkspaceRepositoryBinding {
        self.binding.clone()
    }

    fn load(&self) -> Result<Option<Value>, PrimaryWorkspaceStoreError> {
        self.loads.fetch_add(1, Ordering::SeqCst);
        Ok(self.value.lock().unwrap().clone())
    }

    fn replace(&self, value: Option<Value>) -> Result<(), PrimaryWorkspaceStoreError> {
        let call = self.replaces.fetch_add(1, Ordering::SeqCst) + 1;
        if self.fail_replace_on_call.load(Ordering::SeqCst) == call {
            return Err(PrimaryWorkspaceStoreError::unavailable());
        }
        *self.value.lock().unwrap() = value;
        Ok(())
    }

    fn save(&self) -> Result<(), PrimaryWorkspaceStoreError> {
        self.saves.fetch_add(1, Ordering::SeqCst);
        if self.fail_next_save.swap(false, Ordering::SeqCst) {
            return Err(PrimaryWorkspaceStoreError::unavailable());
        }
        *self.durable.lock().unwrap() = self.value.lock().unwrap().clone();
        if let Some((target, displaced)) = self.replace_target_on_next_save.lock().unwrap().take() {
            fs::rename(&target, &displaced).unwrap();
            fs::create_dir(&target).unwrap();
        }
        self.order.lock().unwrap().push("save");
        Ok(())
    }
}

impl MemoryPrimaryWorkspaceStore {
    fn fail_next_save(&self) {
        self.fail_next_save.store(true, Ordering::SeqCst);
    }

    fn fail_replace_on_call(&self, call: usize) {
        self.fail_replace_on_call.store(call, Ordering::SeqCst);
    }

    fn replace_target_on_next_save(&self, target: PathBuf, displaced: PathBuf) {
        *self.replace_target_on_next_save.lock().unwrap() = Some((target, displaced));
    }

    fn durable(&self) -> Option<Value> {
        self.durable.lock().unwrap().clone()
    }

    fn access_counts(&self) -> (usize, usize, usize) {
        (
            self.loads.load(Ordering::SeqCst),
            self.replaces.load(Ordering::SeqCst),
            self.saves.load(Ordering::SeqCst),
        )
    }
}

struct BlockingLoadPrimaryWorkspaceStore {
    inner: Arc<MemoryPrimaryWorkspaceStore>,
    started: Mutex<Option<mpsc::Sender<()>>>,
    release: Mutex<mpsc::Receiver<()>>,
}

#[derive(Default)]
struct ConstructorRollbackFailureStore {
    binding: PrimaryWorkspaceRepositoryBinding,
    value: Mutex<Option<Value>>,
    save_calls: std::sync::atomic::AtomicUsize,
}

impl PrimaryWorkspaceStore for ConstructorRollbackFailureStore {
    fn repository_binding(&self) -> PrimaryWorkspaceRepositoryBinding {
        self.binding.clone()
    }

    fn load(&self) -> Result<Option<Value>, PrimaryWorkspaceStoreError> {
        Ok(self.value.lock().unwrap().clone())
    }

    fn replace(&self, value: Option<Value>) -> Result<(), PrimaryWorkspaceStoreError> {
        *self.value.lock().unwrap() = value;
        Ok(())
    }

    fn save(&self) -> Result<(), PrimaryWorkspaceStoreError> {
        self.save_calls.fetch_add(1, Ordering::SeqCst);
        Err(PrimaryWorkspaceStoreError::unavailable())
    }
}

impl PrimaryWorkspaceStore for BlockingLoadPrimaryWorkspaceStore {
    fn repository_binding(&self) -> PrimaryWorkspaceRepositoryBinding {
        self.inner.repository_binding()
    }

    fn load(&self) -> Result<Option<Value>, PrimaryWorkspaceStoreError> {
        if let Some(started) = self.started.lock().unwrap().take() {
            started.send(()).unwrap();
            self.release.lock().unwrap().recv().unwrap();
        }
        self.inner.load()
    }

    fn replace(&self, value: Option<Value>) -> Result<(), PrimaryWorkspaceStoreError> {
        self.inner.replace(value)
    }

    fn save(&self) -> Result<(), PrimaryWorkspaceStoreError> {
        self.inner.save()
    }
}

#[derive(Clone)]
struct MemoryHostRecordValue {
    kernel: Option<Value>,
    private_workspace: String,
}

struct MemoryAtomicHostRecord {
    value: Arc<Mutex<MemoryHostRecordValue>>,
    commits: Arc<std::sync::atomic::AtomicUsize>,
    binding: PrimaryWorkspaceRepositoryBinding,
}

impl MemoryAtomicHostRecord {
    fn new(private_workspace: &str) -> Arc<Self> {
        Arc::new(Self {
            value: Arc::new(Mutex::new(MemoryHostRecordValue {
                kernel: None,
                private_workspace: private_workspace.to_string(),
            })),
            commits: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            binding: PrimaryWorkspaceRepositoryBinding::new(),
        })
    }

    fn snapshot(&self) -> MemoryHostRecordValue {
        self.value.lock().unwrap().clone()
    }

    fn replace_private_workspace(&self, value: &str) {
        self.value.lock().unwrap().private_workspace = value.to_string();
    }

    fn transaction(
        self: &Arc<Self>,
        next: &str,
        authority_binding: PreparedWorkspaceAuthorityBinding,
    ) -> MemoryHostWorkspaceTransaction {
        MemoryHostWorkspaceTransaction {
            record: self.clone(),
            binding: self.binding.clone(),
            authority_binding,
            expected_record: self.snapshot(),
            next: next.to_string(),
            fail_persist: false,
            outcome_unknown_after_commit: false,
            replace_target_after_commit: None,
        }
    }
}

impl PrimaryWorkspaceStore for MemoryAtomicHostRecord {
    fn repository_binding(&self) -> PrimaryWorkspaceRepositoryBinding {
        self.binding.clone()
    }

    fn load(&self) -> Result<Option<Value>, PrimaryWorkspaceStoreError> {
        Ok(self.value.lock().unwrap().kernel.clone())
    }

    fn replace(&self, value: Option<Value>) -> Result<(), PrimaryWorkspaceStoreError> {
        self.value.lock().unwrap().kernel = value;
        Ok(())
    }

    fn save(&self) -> Result<(), PrimaryWorkspaceStoreError> {
        Ok(())
    }
}

struct MemoryHostWorkspaceTransaction {
    record: Arc<MemoryAtomicHostRecord>,
    binding: PrimaryWorkspaceRepositoryBinding,
    authority_binding: PreparedWorkspaceAuthorityBinding,
    expected_record: MemoryHostRecordValue,
    next: String,
    fail_persist: bool,
    outcome_unknown_after_commit: bool,
    replace_target_after_commit: Option<(PathBuf, PathBuf)>,
}

struct OutcomeUnknownMemoryStoreTransaction {
    authority_binding: PreparedWorkspaceAuthorityBinding,
    repository_binding: PrimaryWorkspaceRepositoryBinding,
    store: Arc<MemoryPrimaryWorkspaceStore>,
}

impl AtomicHostWorkspaceTransaction for OutcomeUnknownMemoryStoreTransaction {
    fn repository_binding(&self) -> PrimaryWorkspaceRepositoryBinding {
        self.repository_binding.clone()
    }

    fn authority_binding(&self) -> PreparedWorkspaceAuthorityBinding {
        self.authority_binding.clone()
    }

    fn compare_and_commit(
        self: Box<Self>,
        expected_kernel_value: Option<&Value>,
        next_kernel_value: Value,
    ) -> Result<(), AtomicHostWorkspaceCommitError> {
        if self.store.load().ok().as_ref().and_then(Option::as_ref) != expected_kernel_value {
            return Err(AtomicHostWorkspaceCommitError::conflict());
        }
        self.store
            .replace(Some(next_kernel_value))
            .map_err(|_| AtomicHostWorkspaceCommitError::outcome_unknown())?;
        self.store
            .save()
            .map_err(|_| AtomicHostWorkspaceCommitError::outcome_unknown())?;
        Err(AtomicHostWorkspaceCommitError::outcome_unknown())
    }
}

impl AtomicHostWorkspaceTransaction for MemoryHostWorkspaceTransaction {
    fn repository_binding(&self) -> PrimaryWorkspaceRepositoryBinding {
        self.binding.clone()
    }

    fn authority_binding(&self) -> PreparedWorkspaceAuthorityBinding {
        self.authority_binding.clone()
    }

    fn compare_and_commit(
        self: Box<Self>,
        expected_kernel_value: Option<&Value>,
        next_kernel_value: Value,
    ) -> Result<(), AtomicHostWorkspaceCommitError> {
        if self.fail_persist {
            return Err(AtomicHostWorkspaceCommitError::no_commit());
        }
        let mut record = self.record.value.lock().unwrap();
        if record.kernel.as_ref() != expected_kernel_value
            || record.kernel != self.expected_record.kernel
            || record.private_workspace != self.expected_record.private_workspace
        {
            return Err(AtomicHostWorkspaceCommitError::conflict());
        }
        record.kernel = Some(next_kernel_value);
        record.private_workspace = self.next.clone();
        drop(record);
        self.record.commits.fetch_add(1, Ordering::SeqCst);
        if let Some((target, displaced)) = self.replace_target_after_commit.as_ref() {
            fs::rename(target, displaced).unwrap();
            fs::create_dir(target).unwrap();
        }
        if self.outcome_unknown_after_commit {
            return Err(AtomicHostWorkspaceCommitError::outcome_unknown());
        }
        Ok(())
    }
}

struct BlockingHostWorkspaceTransaction {
    inner: MemoryHostWorkspaceTransaction,
    started: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
}

impl AtomicHostWorkspaceTransaction for BlockingHostWorkspaceTransaction {
    fn repository_binding(&self) -> PrimaryWorkspaceRepositoryBinding {
        self.inner.repository_binding()
    }

    fn authority_binding(&self) -> PreparedWorkspaceAuthorityBinding {
        self.inner.authority_binding()
    }

    fn compare_and_commit(
        self: Box<Self>,
        expected_kernel_value: Option<&Value>,
        next_kernel_value: Value,
    ) -> Result<(), AtomicHostWorkspaceCommitError> {
        let Self {
            inner,
            started,
            release,
        } = *self;
        started
            .send(())
            .map_err(|_| AtomicHostWorkspaceCommitError::no_commit())?;
        release
            .recv()
            .map_err(|_| AtomicHostWorkspaceCommitError::no_commit())?;
        Box::new(inner).compare_and_commit(expected_kernel_value, next_kernel_value)
    }
}

struct ReadOnlyReentrantHostWorkspaceTransaction {
    inner: MemoryHostWorkspaceTransaction,
    runtime: Arc<KernelRuntime>,
    service: Arc<WorkspaceService>,
    observed: Arc<AtomicBool>,
}

struct RacingDirectAuthorityCommitTransaction {
    inner: MemoryHostWorkspaceTransaction,
    racer_started: mpsc::Sender<()>,
    racer_finished: mpsc::Receiver<Result<(), WorkspaceAuthorityErrorKind>>,
    racer_outcome: Arc<Mutex<Option<Result<(), WorkspaceAuthorityErrorKind>>>>,
}

impl AtomicHostWorkspaceTransaction for RacingDirectAuthorityCommitTransaction {
    fn repository_binding(&self) -> PrimaryWorkspaceRepositoryBinding {
        self.inner.repository_binding()
    }

    fn authority_binding(&self) -> PreparedWorkspaceAuthorityBinding {
        self.inner.authority_binding()
    }

    fn compare_and_commit(
        self: Box<Self>,
        expected_kernel_value: Option<&Value>,
        next_kernel_value: Value,
    ) -> Result<(), AtomicHostWorkspaceCommitError> {
        self.racer_started
            .send(())
            .map_err(|_| AtomicHostWorkspaceCommitError::no_commit())?;
        let outcome = self
            .racer_finished
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| AtomicHostWorkspaceCommitError::no_commit())?;
        *self.racer_outcome.lock().unwrap() = Some(outcome);
        Box::new(self.inner).compare_and_commit(expected_kernel_value, next_kernel_value)
    }
}

impl AtomicHostWorkspaceTransaction for ReadOnlyReentrantHostWorkspaceTransaction {
    fn repository_binding(&self) -> PrimaryWorkspaceRepositoryBinding {
        self.inner.repository_binding()
    }

    fn authority_binding(&self) -> PreparedWorkspaceAuthorityBinding {
        self.inner.authority_binding()
    }

    fn compare_and_commit(
        self: Box<Self>,
        expected_kernel_value: Option<&Value>,
        next_kernel_value: Value,
    ) -> Result<(), AtomicHostWorkspaceCommitError> {
        self.runtime
            .active_workspace_authority()
            .expect("workspace authority")
            .verify_held_directory()
            .map_err(|_| AtomicHostWorkspaceCommitError::no_commit())?;
        self.service
            .current()
            .map_err(|_| AtomicHostWorkspaceCommitError::no_commit())?;
        self.observed.store(true, Ordering::SeqCst);
        Box::new(self.inner).compare_and_commit(expected_kernel_value, next_kernel_value)
    }
}

#[derive(Default)]
struct RecordingEventSink {
    publications: Mutex<Vec<EventPublication>>,
    fail: AtomicBool,
    order: Arc<Mutex<Vec<&'static str>>>,
}

struct SnapshotObservingEventSink {
    runtime: Weak<KernelRuntime>,
    observed: Mutex<Vec<WorkspaceDto>>,
}

impl EventSink for SnapshotObservingEventSink {
    fn publish(&self, _publication: &EventPublication) -> Result<(), EventSinkError> {
        let runtime = self.runtime.upgrade().ok_or(EventSinkError)?;
        let snapshot = runtime
            .active_workspace_snapshot()
            .map_err(|_| EventSinkError)?;
        self.observed
            .lock()
            .unwrap()
            .push(snapshot.workspace().clone());
        Ok(())
    }
}

impl EventSink for RecordingEventSink {
    fn publish(&self, publication: &EventPublication) -> Result<(), EventSinkError> {
        self.publications.lock().unwrap().push(publication.clone());
        self.order.lock().unwrap().push("event");
        if self.fail.load(Ordering::SeqCst) {
            return Err(EventSinkError);
        }
        Ok(())
    }
}

#[derive(Default)]
struct SyncTestHost;

impl EventSink for SyncTestHost {
    fn publish(&self, _publication: &EventPublication) -> Result<(), EventSinkError> {
        Ok(())
    }
}

impl Clock for SyncTestHost {
    fn now(&self) -> Result<Rfc3339Utc, PortError> {
        Rfc3339Utc::parse("2026-07-30T00:00:00Z").map_err(|_| PortError::unavailable())
    }
}

impl Sleeper for SyncTestHost {
    fn sleep(&self, _duration: Duration) -> BoxSleepFuture<'_> {
        Box::pin(async { Ok(()) })
    }
}

impl TaskSpawner for SyncTestHost {
    fn spawn(&self, task: BoxTaskFuture) -> Result<(), PortError> {
        tokio::spawn(task);
        Ok(())
    }
}

impl CredentialStore for SyncTestHost {
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

impl DiagnosticsSink for SyncTestHost {
    fn emit(&self, _record: DiagnosticRecord) -> Result<(), PortError> {
        Ok(())
    }
}

impl NetworkReachability for SyncTestHost {
    fn is_reachable(&self) -> Result<bool, PortError> {
        Ok(true)
    }
}

#[derive(Default)]
struct GatedCancellationExecutor {
    cancellation_seen: Notify,
    release: Notify,
    runs: AtomicUsize,
    started: Notify,
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
    ) -> Result<(), SyncExecutionError> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        self.started.notify_one();
        context.cancellation().cancelled().await;
        self.cancellation_seen.notify_one();
        self.release.notified().await;
        Ok(())
    }
}

fn sync_test_ports() -> KernelPorts {
    let host = Arc::new(SyncTestHost);
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

struct DesktopFixture {
    runtime: Arc<KernelRuntime>,
    managed: ManagedWorkspaceCollection,
    workspace: PathBuf,
    app_data: PathBuf,
    cache: PathBuf,
}

impl DesktopFixture {
    fn new(root: &std::path::Path) -> Self {
        let workspace = root.join("workspace");
        let app_data = root.join("app-data");
        let cache = root.join("cache");
        fs::create_dir_all(&workspace).unwrap();
        fs::create_dir_all(&app_data).unwrap();
        fs::create_dir_all(&cache).unwrap();
        let paths = KernelPaths::desktop(&workspace, &app_data, &cache).unwrap();
        let managed = ManagedWorkspaceCollection::from_paths(&paths).unwrap();
        let runtime = KernelRuntime::activate(
            KernelConfig::generate().unwrap(),
            paths,
            KernelPorts::unavailable(),
        )
        .unwrap();
        Self {
            runtime,
            managed,
            workspace,
            app_data,
            cache,
        }
    }

    async fn into_service(
        self,
        store: Arc<dyn PrimaryWorkspaceStore>,
        events: Arc<dyn EventSink>,
    ) -> (Arc<KernelRuntime>, WorkspaceService) {
        let service = WorkspaceService::new(
            &self.runtime,
            store,
            self.managed,
            events,
            "Initial Workspace",
        )
        .await
        .unwrap();
        (self.runtime, service)
    }
}

struct ActiveRunningSyncFixture {
    executor: Arc<GatedCancellationExecutor>,
    rebuild_managed: ManagedWorkspaceCollection,
    runtime: Arc<KernelRuntime>,
    store: Arc<MemoryPrimaryWorkspaceStore>,
    sync: Arc<SyncService>,
    workspace: Arc<WorkspaceService>,
}

async fn active_running_sync_fixture(root: &std::path::Path) -> ActiveRunningSyncFixture {
    let workspace_path = root.join("workspace");
    let app_data = root.join("app-data");
    let cache = root.join("cache");
    for path in [&workspace_path, &app_data, &cache] {
        fs::create_dir(path).unwrap();
    }
    fs::write(
        app_data.join("sync-config.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
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
                "region": "us-east-1",
                "bucket": "notes",
                "accessKeyId": "access-key-id",
                "secretAccessKey": "secret-access-key",
                "requestTimeoutSeconds": 60,
                "addressingStyle": "auto",
                "tlsVerification": "verify"
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let config = KernelConfig::generate().unwrap();
    let paths = KernelPaths::desktop(&workspace_path, &app_data, &cache).unwrap();
    let managed = ManagedWorkspaceCollection::from_paths(&paths).unwrap();
    let rebuild_managed = ManagedWorkspaceCollection::from_paths(&paths).unwrap();
    let durable =
        DurableFileStore::at_instance_data(paths.instance_data_root(), config.launch_epoch())
            .unwrap();
    let runtime = KernelRuntime::activate(config, paths, sync_test_ports()).unwrap();
    let store = Arc::new(MemoryPrimaryWorkspaceStore::default());
    let workspace = Arc::new(
        WorkspaceService::new(
            &runtime,
            store.clone(),
            managed,
            Arc::new(RecordingEventSink::default()),
            "Initial Workspace",
        )
        .await
        .unwrap(),
    );
    let executor = Arc::new(GatedCancellationExecutor::default());
    let sync = Arc::new(SyncService::new(
        runtime.clone(),
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        executor.clone(),
    ));
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

    ActiveRunningSyncFixture {
        executor,
        rebuild_managed,
        runtime,
        store,
        sync,
        workspace,
    }
}

async fn assert_task_is_pending<T>(task: &mut tokio::task::JoinHandle<T>) {
    poll_fn(|context| match Pin::new(&mut *task).poll(context) {
        std::task::Poll::Pending => std::task::Poll::Ready(()),
        std::task::Poll::Ready(_) => panic!("task completed before its deterministic release"),
    })
    .await;
}

async fn cancel_running_sync_and_reopen(
    runtime: &Arc<KernelRuntime>,
    executor: &GatedCancellationExecutor,
) {
    let transition = runtime
        .begin_sync_workspace_transition_for_test()
        .await
        .unwrap();
    executor.cancellation_seen.notified().await;
    executor.release.notify_one();
    transition.wait_drained().await.unwrap();
    transition.reopen_for_test().await.unwrap();
}

async fn assert_sync_admission_is_closed(sync: &SyncService) {
    let config = SyncApiService::get_sync_config(sync).await.unwrap();
    assert!(SyncApiService::trigger_sync_run(
        sync,
        TriggerSyncRunRequest {
            expected_config_revision: config.revision,
        },
    )
    .await
    .is_err());
}

#[tokio::test]
async fn current_workspace_identity_is_stable_and_matches_the_api_adapter() {
    let temporary = tempdir().unwrap();
    let store = Arc::new(MemoryPrimaryWorkspaceStore::default());
    let events = Arc::new(RecordingEventSink::default());
    let (_runtime, service) = DesktopFixture::new(temporary.path())
        .into_service(store, events.clone())
        .await;

    let first = service.current().unwrap();
    let second = service.current().unwrap();
    let api = WorkspaceApiService::get_workspace(&service).await.unwrap();

    assert_eq!(first, second);
    assert_eq!(api, first);
    assert_eq!(first.display_name, "Initial Workspace");
    assert!(events.publications.lock().unwrap().is_empty());
    assert_workspace_identity_is_populated(&first);
}

#[tokio::test]
async fn rebuilding_the_service_in_one_runtime_preserves_current_id_generation_and_revision() {
    let temporary = tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    let app_data = temporary.path().join("app-data");
    let cache = temporary.path().join("cache");
    for path in [&workspace, &app_data, &cache] {
        fs::create_dir(path).unwrap();
    }
    let paths = KernelPaths::desktop(&workspace, &app_data, &cache).unwrap();
    let first_managed = ManagedWorkspaceCollection::from_paths(&paths).unwrap();
    let second_managed = ManagedWorkspaceCollection::from_paths(&paths).unwrap();
    let runtime = KernelRuntime::activate(
        KernelConfig::generate().unwrap(),
        paths,
        KernelPorts::unavailable(),
    )
    .unwrap();
    let store = Arc::new(MemoryPrimaryWorkspaceStore::default());
    let first = WorkspaceService::new(
        &runtime,
        store.clone(),
        first_managed,
        Arc::new(RecordingEventSink::default()),
        "Initial Workspace",
    )
    .await
    .unwrap();
    let second = WorkspaceService::new(
        &runtime,
        store,
        second_managed,
        Arc::new(RecordingEventSink::default()),
        "Ignored Existing Display Name",
    )
    .await
    .unwrap();

    assert_eq!(first.current().unwrap(), second.current().unwrap());
}

#[tokio::test]
async fn active_workspace_snapshot_retains_authority_and_metadata_as_one_unit() {
    let temporary = tempdir().unwrap();
    let target = temporary.path().join("snapshot-target");
    fs::create_dir(&target).unwrap();
    let store = MemoryAtomicHostRecord::new("workspace-initial");
    let events = Arc::new(RecordingEventSink::default());
    let (runtime, service) = DesktopFixture::new(temporary.path())
        .into_service(store.clone(), events)
        .await;
    let before = service.current().unwrap();
    let retained = runtime.active_workspace_snapshot().unwrap();
    let old_workspace_path = temporary.path().join("workspace");
    let prepared = runtime.prepare_host_workspace_authority(&target).unwrap();
    let transaction = store.transaction("snapshot-target", prepared.binding());

    let committed = service
        .compare_and_set_host_workspace_transaction(
            &before.revision,
            prepared,
            "Snapshot Target",
            Box::new(transaction),
        )
        .await
        .unwrap();

    let installed = runtime.active_workspace_snapshot().unwrap();
    assert_eq!(retained.workspace(), &before);
    assert_eq!(installed.workspace(), &committed);
    assert!(!Arc::ptr_eq(&retained, &installed));
    drop(service);
    drop(runtime);
    assert_workspace_is_locked(&old_workspace_path, temporary.path(), "retained-old");
    drop(retained);
    assert_workspace_is_acquirable(&old_workspace_path, temporary.path(), "released-old");
}

#[tokio::test]
async fn direct_authority_commit_is_rejected_after_workspace_initialization() {
    let temporary = tempdir().unwrap();
    let target = temporary.path().join("direct-target");
    let displaced = temporary.path().join("direct-target-displaced");
    fs::create_dir(&target).unwrap();
    let store = Arc::new(MemoryPrimaryWorkspaceStore::default());
    let events = Arc::new(RecordingEventSink::default());
    let (runtime, service) = DesktopFixture::new(temporary.path())
        .into_service(store, events)
        .await;
    let before = runtime.active_workspace_snapshot().unwrap();
    let prepared = runtime.prepare_host_workspace_authority(&target).unwrap();
    fs::rename(&target, &displaced).unwrap();
    fs::create_dir(&target).unwrap();

    let error = runtime
        .commit_host_workspace_authority(prepared)
        .unwrap_err();

    assert_eq!(
        error.kind(),
        WorkspaceAuthorityErrorKind::WorkspaceUnavailable
    );
    assert!(Arc::ptr_eq(
        &before,
        &runtime.active_workspace_snapshot().unwrap()
    ));
    assert_eq!(service.current().unwrap(), before.workspace().clone());
}

#[tokio::test]
async fn atomic_commit_installs_one_snapshot_before_publishing_event() {
    let temporary = tempdir().unwrap();
    let target = temporary.path().join("atomic-target");
    fs::create_dir(&target).unwrap();
    let fixture = DesktopFixture::new(temporary.path());
    let store = MemoryAtomicHostRecord::new("workspace-initial");
    let events = Arc::new(SnapshotObservingEventSink {
        runtime: Arc::downgrade(&fixture.runtime),
        observed: Mutex::new(Vec::new()),
    });
    let (runtime, service) = fixture.into_service(store.clone(), events.clone()).await;
    let before = service.current().unwrap();
    let prepared = runtime.prepare_host_workspace_authority(&target).unwrap();
    let transaction = store.transaction("atomic-target", prepared.binding());

    let committed = service
        .compare_and_set_host_workspace_transaction(
            &before.revision,
            prepared,
            "Atomic Target",
            Box::new(transaction),
        )
        .await
        .unwrap();

    assert_eq!(*events.observed.lock().unwrap(), vec![committed.clone()]);
    assert_eq!(
        runtime.active_workspace_snapshot().unwrap().workspace(),
        &committed
    );
}

#[tokio::test]
async fn same_runtime_equal_rebuild_is_idempotent_and_foreign_store_is_rejected() {
    let temporary = tempdir().unwrap();
    let fixture = DesktopFixture::new(temporary.path());
    let rebuild_paths =
        KernelPaths::desktop(&fixture.workspace, &fixture.app_data, &fixture.cache).unwrap();
    let equal_managed = ManagedWorkspaceCollection::from_paths(&rebuild_paths).unwrap();
    let foreign_managed = ManagedWorkspaceCollection::from_paths(&rebuild_paths).unwrap();
    let runtime = fixture.runtime.clone();
    let store = Arc::new(MemoryPrimaryWorkspaceStore::default());
    let first = WorkspaceService::new(
        &runtime,
        store.clone(),
        fixture.managed,
        Arc::new(RecordingEventSink::default()),
        "Initial Workspace",
    )
    .await
    .unwrap();
    let first_snapshot = runtime.active_workspace_snapshot().unwrap();
    let equal = WorkspaceService::new(
        &runtime,
        store,
        equal_managed,
        Arc::new(RecordingEventSink::default()),
        "Ignored",
    )
    .await
    .unwrap();
    let equal_snapshot = runtime.active_workspace_snapshot().unwrap();
    let foreign_store = Arc::new(MemoryPrimaryWorkspaceStore::default());

    let foreign = WorkspaceService::new(
        &runtime,
        foreign_store.clone(),
        foreign_managed,
        Arc::new(RecordingEventSink::default()),
        "Foreign",
    )
    .await;
    let foreign = match foreign {
        Ok(_) => panic!("foreign repository must not install into one runtime"),
        Err(error) => error,
    };

    assert!(Arc::ptr_eq(&first_snapshot, &equal_snapshot));
    assert_eq!(first.current().unwrap(), equal.current().unwrap());
    assert_eq!(
        foreign.kind(),
        WorkspaceServiceErrorKind::PersistenceUnavailable
    );
    assert_eq!(foreign_store.access_counts(), (0, 0, 0));
    assert!(Arc::ptr_eq(
        &first_snapshot,
        &runtime.active_workspace_snapshot().unwrap()
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_workspace_service_construction_serializes_before_store_io() {
    let temporary = tempdir().unwrap();
    let fixture = DesktopFixture::new(temporary.path());
    let second_paths =
        KernelPaths::desktop(&fixture.workspace, &fixture.app_data, &fixture.cache).unwrap();
    let second_managed = ManagedWorkspaceCollection::from_paths(&second_paths).unwrap();
    let runtime = fixture.runtime.clone();
    let first_runtime = runtime.clone();
    let (started_sender, started_receiver) = mpsc::channel();
    let (release_sender, release_receiver) = mpsc::channel();
    let first_store = Arc::new(BlockingLoadPrimaryWorkspaceStore {
        inner: Arc::new(MemoryPrimaryWorkspaceStore::default()),
        started: Mutex::new(Some(started_sender)),
        release: Mutex::new(release_receiver),
    });
    let first = tokio::spawn(async move {
        WorkspaceService::new(
            &first_runtime,
            first_store,
            fixture.managed,
            Arc::new(RecordingEventSink::default()),
            "First",
        )
        .await
    });
    tokio::task::spawn_blocking(move || started_receiver.recv_timeout(Duration::from_secs(5)))
        .await
        .unwrap()
        .expect("first constructor must hold initialization serialization before load");
    let foreign_store = Arc::new(MemoryPrimaryWorkspaceStore::default());
    let second_store = foreign_store.clone();

    let second = tokio::spawn(async move {
        WorkspaceService::new(
            &runtime,
            second_store,
            second_managed,
            Arc::new(RecordingEventSink::default()),
            "Second",
        )
        .await
    });

    tokio::task::yield_now().await;
    assert!(!second.is_finished());
    assert_eq!(foreign_store.access_counts(), (0, 0, 0));
    release_sender.send(()).unwrap();
    assert!(first.await.unwrap().is_ok());
    assert!(second.await.unwrap().is_err());
}

#[tokio::test]
async fn constructor_rollback_save_failure_enters_recovery_before_returning() {
    let temporary = tempdir().unwrap();
    let target = temporary.path().join("target");
    fs::create_dir(&target).unwrap();
    let fixture = DesktopFixture::new(temporary.path());
    let runtime = fixture.runtime.clone();
    let store = Arc::new(ConstructorRollbackFailureStore::default());

    let result = WorkspaceService::new(
        &runtime,
        store.clone(),
        fixture.managed,
        Arc::new(RecordingEventSink::default()),
        "Initial",
    )
    .await;

    assert!(result.is_err());
    assert_eq!(store.save_calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        runtime
            .prepare_host_workspace_authority(&target)
            .unwrap_err()
            .kind(),
        WorkspaceAuthorityErrorKind::WorkspaceUnavailable
    );
}

#[tokio::test]
async fn constructor_post_save_authority_failure_enters_recovery_before_returning() {
    let temporary = tempdir().unwrap();
    let target = temporary.path().join("target");
    let displaced = temporary.path().join("workspace-displaced");
    fs::create_dir(&target).unwrap();
    let fixture = DesktopFixture::new(temporary.path());
    let runtime = fixture.runtime.clone();
    let store = Arc::new(MemoryPrimaryWorkspaceStore::default());
    store.replace_target_on_next_save(fixture.workspace.clone(), displaced);

    let result = WorkspaceService::new(
        &runtime,
        store,
        fixture.managed,
        Arc::new(RecordingEventSink::default()),
        "Initial",
    )
    .await;

    assert!(result.is_err());
    assert_eq!(
        runtime
            .prepare_host_workspace_authority(&target)
            .unwrap_err()
            .kind(),
        WorkspaceAuthorityErrorKind::WorkspaceUnavailable
    );
}

#[tokio::test]
async fn same_binding_changed_canonical_rebuild_enters_global_recovery_without_overwrite() {
    let temporary = tempdir().unwrap();
    let fixture = DesktopFixture::new(temporary.path());
    let rebuild_paths =
        KernelPaths::desktop(&fixture.workspace, &fixture.app_data, &fixture.cache).unwrap();
    let rebuild_managed = ManagedWorkspaceCollection::from_paths(&rebuild_paths).unwrap();
    let runtime = fixture.runtime.clone();
    let store = Arc::new(MemoryPrimaryWorkspaceStore::default());
    let first = WorkspaceService::new(
        &runtime,
        store.clone(),
        fixture.managed,
        Arc::new(RecordingEventSink::default()),
        "Initial",
    )
    .await
    .unwrap();
    let before = first.current().unwrap();
    let external = serde_json::json!({
        "schemaVersion": 1,
        "revisionSeed": "external-change",
        "displayName": "External"
    });
    PrimaryWorkspaceStore::replace(store.as_ref(), Some(external.clone())).unwrap();
    PrimaryWorkspaceStore::save(store.as_ref()).unwrap();

    let rebuilt = WorkspaceService::new(
        &runtime,
        store.clone(),
        rebuild_managed,
        Arc::new(RecordingEventSink::default()),
        "Ignored",
    )
    .await;

    assert!(rebuilt.is_err());
    assert_eq!(store.durable(), Some(external));
    assert_eq!(
        first.current().unwrap_err().kind(),
        WorkspaceServiceErrorKind::WorkspaceUnavailable
    );
    assert_eq!(
        runtime.active_workspace_snapshot().unwrap_err().kind(),
        WorkspaceAuthorityErrorKind::WorkspaceUnavailable
    );
    assert_ne!(before.display_name, "External");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn changed_canonical_rebuild_reloads_after_running_sync_drain_before_recovery() {
    let temporary = tempdir().unwrap();
    let fixture = active_running_sync_fixture(temporary.path()).await;
    let before = fixture.runtime.active_workspace_snapshot().unwrap();
    let phase_a_value = serde_json::json!({
        "schemaVersion": 1,
        "revisionSeed": "phase-a-external",
        "displayName": "Phase A External"
    });
    fixture.store.replace(Some(phase_a_value)).unwrap();
    fixture.store.save().unwrap();
    let loads_before_rebuild = fixture.store.access_counts().0;
    let runtime = fixture.runtime.clone();
    let store = fixture.store.clone();
    let mut rebuild = tokio::spawn(async move {
        WorkspaceService::new(
            &runtime,
            store,
            fixture.rebuild_managed,
            Arc::new(RecordingEventSink::default()),
            "Ignored",
        )
        .await
    });

    fixture.executor.cancellation_seen.notified().await;
    assert_task_is_pending(&mut rebuild).await;
    assert_eq!(
        fixture.store.access_counts().0,
        loads_before_rebuild + 1,
        "Phase A must read the changed canonical exactly once before drain"
    );
    assert!(Arc::ptr_eq(
        &before,
        &fixture.runtime.active_workspace_snapshot().unwrap()
    ));
    fixture
        .store
        .replace(Some(serde_json::json!({
            "schemaVersion": 1,
            "revisionSeed": "phase-b-external",
            "displayName": "Phase B External"
        })))
        .unwrap();

    fixture.executor.release.notify_one();
    let error = match rebuild.await.unwrap() {
        Ok(_) => panic!("changed canonical rebuild must enter recovery"),
        Err(error) => error,
    };

    assert_eq!(
        error.kind(),
        WorkspaceServiceErrorKind::PersistenceUnavailable
    );
    assert_eq!(
        fixture.store.access_counts().0,
        loads_before_rebuild + 2,
        "Phase B must reload and revalidate the store after drain"
    );
    assert_eq!(fixture.executor.runs.load(Ordering::SeqCst), 1);
    assert_eq!(
        fixture
            .runtime
            .active_workspace_snapshot()
            .unwrap_err()
            .kind(),
        WorkspaceAuthorityErrorKind::WorkspaceUnavailable
    );
    assert_sync_admission_is_closed(fixture.sync.as_ref()).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn equal_rebuild_preserves_running_sync_without_cancellation() {
    let temporary = tempdir().unwrap();
    let fixture = active_running_sync_fixture(temporary.path()).await;
    let before = fixture.runtime.active_workspace_snapshot().unwrap();
    let runtime = fixture.runtime.clone();
    let store = fixture.store.clone();
    let mut rebuild = tokio::spawn(async move {
        WorkspaceService::new(
            &runtime,
            store,
            fixture.rebuild_managed,
            Arc::new(RecordingEventSink::default()),
            "Ignored",
        )
        .await
    });

    let rebuilt = tokio::select! {
        result = &mut rebuild => result.unwrap().unwrap(),
        _ = fixture.executor.cancellation_seen.notified() => {
            fixture.executor.release.notify_one();
            let _result = rebuild.await;
            panic!("equal rebuild cancelled the active sync run")
        }
    };

    assert_eq!(
        rebuilt.current().unwrap(),
        fixture.workspace.current().unwrap()
    );
    assert!(Arc::ptr_eq(
        &before,
        &fixture.runtime.active_workspace_snapshot().unwrap()
    ));
    assert_eq!(fixture.executor.runs.load(Ordering::SeqCst), 1);
    cancel_running_sync_and_reopen(&fixture.runtime, fixture.executor.as_ref()).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn foreign_rebuild_rejects_before_store_io_without_cancelling_running_sync() {
    let temporary = tempdir().unwrap();
    let fixture = active_running_sync_fixture(temporary.path()).await;
    let before = fixture.runtime.active_workspace_snapshot().unwrap();
    let foreign_store = Arc::new(MemoryPrimaryWorkspaceStore::default());
    let runtime = fixture.runtime.clone();
    let tested_store = foreign_store.clone();
    let mut rebuild = tokio::spawn(async move {
        WorkspaceService::new(
            &runtime,
            tested_store,
            fixture.rebuild_managed,
            Arc::new(RecordingEventSink::default()),
            "Foreign",
        )
        .await
    });
    let result = tokio::select! {
        result = &mut rebuild => result.unwrap(),
        _ = fixture.executor.cancellation_seen.notified() => {
            fixture.executor.release.notify_one();
            let _result = rebuild.await;
            panic!("foreign rebuild cancelled the active sync run")
        }
    };
    let error = match result {
        Ok(_) => panic!("foreign repository must be rejected"),
        Err(error) => error,
    };

    assert_eq!(
        error.kind(),
        WorkspaceServiceErrorKind::PersistenceUnavailable
    );
    assert_eq!(foreign_store.access_counts(), (0, 0, 0));
    assert!(Arc::ptr_eq(
        &before,
        &fixture.runtime.active_workspace_snapshot().unwrap()
    ));
    assert_eq!(fixture.executor.runs.load(Ordering::SeqCst), 1);
    cancel_running_sync_and_reopen(&fixture.runtime, fixture.executor.as_ref()).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn outcome_unknown_waits_for_running_sync_drain_then_closes_admission() {
    let temporary = tempdir().unwrap();
    let fixture = active_running_sync_fixture(temporary.path()).await;
    let before = fixture.runtime.active_workspace_snapshot().unwrap();
    let target = temporary.path().join("outcome-unknown-target");
    fs::create_dir(&target).unwrap();
    let prepared = fixture
        .runtime
        .prepare_host_workspace_authority(&target)
        .unwrap();
    let transaction = OutcomeUnknownMemoryStoreTransaction {
        authority_binding: prepared.binding(),
        repository_binding: fixture.store.repository_binding(),
        store: fixture.store.clone(),
    };
    let service = fixture.workspace.clone();
    let expected_revision = before.workspace().revision.clone();
    let mut switching = tokio::spawn(async move {
        service
            .compare_and_set_host_workspace_transaction(
                &expected_revision,
                prepared,
                "Outcome Unknown Target",
                Box::new(transaction),
            )
            .await
    });

    fixture.executor.cancellation_seen.notified().await;
    assert_task_is_pending(&mut switching).await;
    assert!(Arc::ptr_eq(
        &before,
        &fixture.runtime.active_workspace_snapshot().unwrap()
    ));

    fixture.executor.release.notify_one();
    let error = switching.await.unwrap().unwrap_err();

    assert_eq!(
        error.kind(),
        WorkspaceServiceErrorKind::PersistenceUnavailable
    );
    assert_eq!(
        fixture
            .runtime
            .active_workspace_snapshot()
            .unwrap_err()
            .kind(),
        WorkspaceAuthorityErrorKind::WorkspaceUnavailable
    );
    assert_sync_admission_is_closed(fixture.sync.as_ref()).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failed_legacy_rollback_waits_for_running_sync_drain_then_closes_admission() {
    let temporary = tempdir().unwrap();
    let fixture = active_running_sync_fixture(temporary.path()).await;
    let before = fixture.runtime.active_workspace_snapshot().unwrap();
    let target = temporary.path().join("failed-rollback-target");
    let displaced = temporary.path().join("failed-rollback-target-displaced");
    fs::create_dir(&target).unwrap();
    let prepared = fixture
        .runtime
        .prepare_host_workspace_authority(&target)
        .unwrap();
    fixture
        .store
        .replace_target_on_next_save(target.clone(), displaced.clone());
    fixture
        .store
        .fail_replace_on_call(fixture.store.access_counts().1 + 2);
    let service = fixture.workspace.clone();
    let expected_revision = before.workspace().revision.clone();
    let mut switching = tokio::spawn(async move {
        service
            .compare_and_set_host_workspace(&expected_revision, prepared, "Failed Rollback Target")
            .await
    });

    fixture.executor.cancellation_seen.notified().await;
    assert_task_is_pending(&mut switching).await;
    assert!(Arc::ptr_eq(
        &before,
        &fixture.runtime.active_workspace_snapshot().unwrap()
    ));

    fixture.executor.release.notify_one();
    let error = switching.await.unwrap().unwrap_err();

    assert_eq!(
        error.kind(),
        WorkspaceServiceErrorKind::PersistenceUnavailable
    );
    assert!(target.is_dir());
    assert!(displaced.is_dir());
    assert_eq!(
        fixture
            .runtime
            .active_workspace_snapshot()
            .unwrap_err()
            .kind(),
        WorkspaceAuthorityErrorKind::WorkspaceUnavailable
    );
    assert_sync_admission_is_closed(fixture.sync.as_ref()).await;
}

#[tokio::test]
async fn stale_cas_leaves_identity_authority_and_events_unchanged() {
    let temporary = tempdir().unwrap();
    let next = temporary.path().join("next");
    fs::create_dir(&next).unwrap();
    let store = Arc::new(MemoryPrimaryWorkspaceStore::default());
    let events = Arc::new(RecordingEventSink::default());
    let (runtime, service) = DesktopFixture::new(temporary.path())
        .into_service(store, events.clone())
        .await;
    let before = service.current().unwrap();
    let before_authority = runtime
        .active_workspace_authority()
        .expect("workspace authority");
    let prepared = runtime.prepare_host_workspace_authority(&next).unwrap();

    let error = service
        .compare_and_set_host_workspace(
            &Revision::parse("stale-revision").unwrap(),
            prepared,
            "Next Workspace",
        )
        .await
        .unwrap_err();

    assert_eq!(error.kind(), WorkspaceServiceErrorKind::RevisionConflict);
    assert_eq!(error.current_revision(), Some(&before.revision));
    assert_eq!(service.current().unwrap(), before);
    assert!(Arc::ptr_eq(
        &before_authority,
        &runtime
            .active_workspace_authority()
            .expect("workspace authority")
    ));
    assert!(events.publications.lock().unwrap().is_empty());
}

#[tokio::test]
async fn save_failure_rolls_back_store_and_authority_without_an_event() {
    let temporary = tempdir().unwrap();
    let next = temporary.path().join("next");
    fs::create_dir(&next).unwrap();
    let store = Arc::new(MemoryPrimaryWorkspaceStore::default());
    let events = Arc::new(RecordingEventSink::default());
    let (runtime, service) = DesktopFixture::new(temporary.path())
        .into_service(store.clone(), events.clone())
        .await;
    let before = service.current().unwrap();
    let before_durable = store.durable();
    let before_authority = runtime
        .active_workspace_authority()
        .expect("workspace authority");
    let prepared = runtime.prepare_host_workspace_authority(&next).unwrap();
    store.fail_next_save();

    let error = service
        .compare_and_set_host_workspace(&before.revision, prepared, "Next Workspace")
        .await
        .unwrap_err();

    assert_eq!(
        error.kind(),
        WorkspaceServiceErrorKind::PersistenceUnavailable
    );
    assert_eq!(store.durable(), before_durable);
    assert_eq!(store.load().unwrap(), before_durable);
    assert_eq!(service.current().unwrap(), before);
    assert!(Arc::ptr_eq(
        &before_authority,
        &runtime
            .active_workspace_authority()
            .expect("workspace authority")
    ));
    assert!(events.publications.lock().unwrap().is_empty());
}

#[tokio::test]
async fn host_persistence_failure_does_not_switch_kernel_authority() {
    let temporary = tempdir().unwrap();
    let next = temporary.path().join("next");
    fs::create_dir(&next).unwrap();
    let store = MemoryAtomicHostRecord::new("workspace-a");
    let events = Arc::new(RecordingEventSink::default());
    let (runtime, service) = DesktopFixture::new(temporary.path())
        .into_service(store.clone(), events.clone())
        .await;
    let before = service.current().unwrap();
    let before_host_record = store.snapshot();
    let before_authority = runtime
        .active_workspace_authority()
        .expect("workspace authority");
    let prepared = runtime.prepare_host_workspace_authority(&next).unwrap();
    let mut host_transaction = store.transaction("workspace-b", prepared.binding());
    host_transaction.fail_persist = true;

    let error = service
        .compare_and_set_host_workspace_transaction(
            &before.revision,
            prepared,
            "Next Workspace",
            Box::new(host_transaction),
        )
        .await
        .unwrap_err();

    assert_eq!(
        error.kind(),
        WorkspaceServiceErrorKind::PersistenceUnavailable
    );
    let after_host_record = store.snapshot();
    assert_eq!(
        after_host_record.private_workspace,
        before_host_record.private_workspace
    );
    assert_eq!(after_host_record.kernel, before_host_record.kernel);
    assert_eq!(store.commits.load(Ordering::SeqCst), 0);
    assert_eq!(service.current().unwrap(), before);
    assert!(Arc::ptr_eq(
        &before_authority,
        &runtime
            .active_workspace_authority()
            .expect("workspace authority")
    ));
    assert!(events.publications.lock().unwrap().is_empty());
}

#[tokio::test]
async fn stale_prepared_authority_is_rejected_before_host_persistence() {
    let temporary = tempdir().unwrap();
    let stale_target = temporary.path().join("stale-target");
    let winning_target = temporary.path().join("winning-target");
    fs::create_dir(&stale_target).unwrap();
    fs::create_dir(&winning_target).unwrap();
    let store = MemoryAtomicHostRecord::new("workspace-a");
    let events = Arc::new(RecordingEventSink::default());
    let (runtime, service) = DesktopFixture::new(temporary.path())
        .into_service(store.clone(), events.clone())
        .await;
    let before = service.current().unwrap();
    let before_host_record = store.snapshot();
    let stale = runtime
        .prepare_host_workspace_authority(&stale_target)
        .unwrap();
    let winning = runtime
        .prepare_host_workspace_authority(&winning_target)
        .unwrap();
    let transaction = store.transaction("stale-target", stale.binding());
    let winning_transaction = store.transaction("winning-target", winning.binding());
    let committed = service
        .compare_and_set_host_workspace_transaction(
            &before.revision,
            winning,
            "Winning Target",
            Box::new(winning_transaction),
        )
        .await
        .unwrap();
    let after_winner = store.snapshot();

    let error = service
        .compare_and_set_host_workspace_transaction(
            &committed.revision,
            stale,
            "Stale Target",
            Box::new(transaction),
        )
        .await
        .unwrap_err();

    assert_eq!(
        error.kind(),
        WorkspaceServiceErrorKind::PreparedAuthorityMismatch
    );
    assert_eq!(store.commits.load(Ordering::SeqCst), 1);
    let after_host_record = store.snapshot();
    assert_eq!(after_host_record.kernel, after_winner.kernel);
    assert_eq!(
        after_host_record.private_workspace,
        after_winner.private_workspace
    );
    assert_ne!(after_host_record.kernel, before_host_record.kernel);
    assert_eq!(service.current().unwrap(), committed);
    assert_eq!(events.publications.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn stale_revision_is_rejected_before_host_persistence() {
    let temporary = tempdir().unwrap();
    let next = temporary.path().join("next");
    fs::create_dir(&next).unwrap();
    let store = MemoryAtomicHostRecord::new("workspace-a");
    let events = Arc::new(RecordingEventSink::default());
    let (runtime, service) = DesktopFixture::new(temporary.path())
        .into_service(store.clone(), events.clone())
        .await;
    let before = service.current().unwrap();
    let before_host_record = store.snapshot();
    let before_authority = runtime
        .active_workspace_authority()
        .expect("workspace authority");
    let prepared = runtime.prepare_host_workspace_authority(&next).unwrap();
    let transaction = store.transaction("workspace-b", prepared.binding());

    let error = service
        .compare_and_set_host_workspace_transaction(
            &Revision::parse("stale-revision").unwrap(),
            prepared,
            "Next Workspace",
            Box::new(transaction),
        )
        .await
        .unwrap_err();

    assert_eq!(error.kind(), WorkspaceServiceErrorKind::RevisionConflict);
    assert_eq!(error.current_revision(), Some(&before.revision));
    assert_eq!(store.commits.load(Ordering::SeqCst), 0);
    let after_host_record = store.snapshot();
    assert_eq!(after_host_record.kernel, before_host_record.kernel);
    assert_eq!(
        after_host_record.private_workspace,
        before_host_record.private_workspace
    );
    assert!(Arc::ptr_eq(
        &before_authority,
        &runtime
            .active_workspace_authority()
            .expect("workspace authority")
    ));
    assert!(events.publications.lock().unwrap().is_empty());
}

#[tokio::test]
async fn stale_host_record_cas_does_not_overwrite_newer_private_state() {
    let temporary = tempdir().unwrap();
    let next = temporary.path().join("next");
    fs::create_dir(&next).unwrap();
    let store = MemoryAtomicHostRecord::new("workspace-a");
    let events = Arc::new(RecordingEventSink::default());
    let (runtime, service) = DesktopFixture::new(temporary.path())
        .into_service(store.clone(), events.clone())
        .await;
    let before = service.current().unwrap();
    let before_authority = runtime
        .active_workspace_authority()
        .expect("workspace authority");
    let prepared = runtime.prepare_host_workspace_authority(&next).unwrap();
    let transaction = store.transaction("workspace-b", prepared.binding());
    store.replace_private_workspace("newer-host-state");

    let error = service
        .compare_and_set_host_workspace_transaction(
            &before.revision,
            prepared,
            "Next Workspace",
            Box::new(transaction),
        )
        .await
        .unwrap_err();

    assert_eq!(
        error.kind(),
        WorkspaceServiceErrorKind::PersistenceUnavailable
    );
    assert_eq!(store.commits.load(Ordering::SeqCst), 0);
    assert_eq!(
        store.snapshot().private_workspace,
        "newer-host-state".to_string()
    );
    assert!(Arc::ptr_eq(
        &before_authority,
        &runtime
            .active_workspace_authority()
            .expect("workspace authority")
    ));
    assert_eq!(service.current().unwrap(), before);
    assert!(events.publications.lock().unwrap().is_empty());
}

#[tokio::test]
async fn stale_service_canonical_cannot_overwrite_a_newer_host_canonical_value() {
    let temporary = tempdir().unwrap();
    let next = temporary.path().join("next");
    fs::create_dir(&next).unwrap();
    let store = MemoryAtomicHostRecord::new("workspace-a");
    let events = Arc::new(RecordingEventSink::default());
    let (runtime, service) = DesktopFixture::new(temporary.path())
        .into_service(store.clone(), events.clone())
        .await;
    let before = service.current().unwrap();
    let prepared = runtime.prepare_host_workspace_authority(&next).unwrap();
    let newer_kernel_value = serde_json::json!({
        "schemaVersion": 1,
        "revisionSeed": "external-newer-revision",
        "displayName": "Externally Updated Workspace"
    });
    PrimaryWorkspaceStore::replace(store.as_ref(), Some(newer_kernel_value.clone())).unwrap();
    let transaction = store.transaction("workspace-b", prepared.binding());

    let error = service
        .compare_and_set_host_workspace_transaction(
            &before.revision,
            prepared,
            "Next Workspace",
            Box::new(transaction),
        )
        .await
        .unwrap_err();

    assert_eq!(
        error.kind(),
        WorkspaceServiceErrorKind::PersistenceUnavailable
    );
    assert_eq!(store.commits.load(Ordering::SeqCst), 0);
    assert_eq!(store.snapshot().kernel, Some(newer_kernel_value));
    assert_eq!(
        runtime.active_workspace_authority().unwrap_err().kind(),
        WorkspaceAuthorityErrorKind::WorkspaceUnavailable
    );
    assert_eq!(
        service.current().unwrap_err().kind(),
        WorkspaceServiceErrorKind::WorkspaceUnavailable
    );
    assert!(events.publications.lock().unwrap().is_empty());
}

#[tokio::test]
async fn foreign_repository_transaction_is_rejected_before_any_host_write() {
    let temporary = tempdir().unwrap();
    let next = temporary.path().join("next");
    fs::create_dir(&next).unwrap();
    let local_store = MemoryAtomicHostRecord::new("local-workspace");
    let foreign_store = MemoryAtomicHostRecord::new("foreign-workspace");
    let foreign_before = foreign_store.snapshot();
    let events = Arc::new(RecordingEventSink::default());
    let (runtime, service) = DesktopFixture::new(temporary.path())
        .into_service(local_store.clone(), events.clone())
        .await;
    let before = service.current().unwrap();
    let local_before = local_store.snapshot();
    let before_authority = runtime
        .active_workspace_authority()
        .expect("workspace authority");
    let prepared = runtime.prepare_host_workspace_authority(&next).unwrap();
    let foreign_transaction = foreign_store.transaction("foreign-next", prepared.binding());

    let error = service
        .compare_and_set_host_workspace_transaction(
            &before.revision,
            prepared,
            "Next Workspace",
            Box::new(foreign_transaction),
        )
        .await
        .unwrap_err();

    assert_eq!(
        error.kind(),
        WorkspaceServiceErrorKind::PersistenceUnavailable
    );
    assert_eq!(local_store.commits.load(Ordering::SeqCst), 0);
    assert_eq!(foreign_store.commits.load(Ordering::SeqCst), 0);
    assert_eq!(local_store.snapshot().kernel, local_before.kernel);
    assert_eq!(
        local_store.snapshot().private_workspace,
        local_before.private_workspace
    );
    assert_eq!(foreign_store.snapshot().kernel, foreign_before.kernel);
    assert_eq!(
        foreign_store.snapshot().private_workspace,
        foreign_before.private_workspace
    );
    assert!(Arc::ptr_eq(
        &before_authority,
        &runtime
            .active_workspace_authority()
            .expect("workspace authority")
    ));
    assert_eq!(service.current().unwrap(), before);
    assert!(events.publications.lock().unwrap().is_empty());
    assert_eq!(
        format!("{:?}", local_store.repository_binding()),
        "PrimaryWorkspaceRepositoryBinding([REDACTED])"
    );
}

#[tokio::test]
async fn transaction_for_another_prepared_authority_is_rejected_before_host_write() {
    let temporary = tempdir().unwrap();
    let target_a = temporary.path().join("target-a");
    let target_b = temporary.path().join("target-b");
    fs::create_dir(&target_a).unwrap();
    fs::create_dir(&target_b).unwrap();
    let store = MemoryAtomicHostRecord::new("workspace-a");
    let events = Arc::new(RecordingEventSink::default());
    let (runtime, service) = DesktopFixture::new(temporary.path())
        .into_service(store.clone(), events.clone())
        .await;
    let before = service.current().unwrap();
    let before_host_record = store.snapshot();
    let before_authority = runtime
        .active_workspace_authority()
        .expect("workspace authority");
    let prepared_a = runtime.prepare_host_workspace_authority(&target_a).unwrap();
    let prepared_b = runtime.prepare_host_workspace_authority(&target_b).unwrap();
    let transaction = store.transaction("workspace-b", prepared_b.binding());

    let error = service
        .compare_and_set_host_workspace_transaction(
            &before.revision,
            prepared_a,
            "Target A",
            Box::new(transaction),
        )
        .await
        .unwrap_err();

    assert_eq!(
        error.kind(),
        WorkspaceServiceErrorKind::PersistenceUnavailable
    );
    assert_eq!(store.commits.load(Ordering::SeqCst), 0);
    let after_host_record = store.snapshot();
    assert_eq!(after_host_record.kernel, before_host_record.kernel);
    assert_eq!(
        after_host_record.private_workspace,
        before_host_record.private_workspace
    );
    assert!(Arc::ptr_eq(
        &before_authority,
        &runtime
            .active_workspace_authority()
            .expect("workspace authority")
    ));
    assert_eq!(service.current().unwrap(), before);
    assert!(events.publications.lock().unwrap().is_empty());
    assert_eq!(
        format!("{:?}", prepared_b.binding()),
        "PreparedWorkspaceAuthorityBinding([REDACTED])"
    );
}

#[tokio::test]
async fn atomic_host_commit_publishes_once_and_rebuilds_the_same_kernel_current() {
    let temporary = tempdir().unwrap();
    let target = temporary.path().join("Private Absolute Workspace");
    fs::create_dir(&target).unwrap();
    let fixture = DesktopFixture::new(temporary.path());
    let rebuild_paths =
        KernelPaths::desktop(&fixture.workspace, &fixture.app_data, &fixture.cache).unwrap();
    let rebuild_managed = ManagedWorkspaceCollection::from_paths(&rebuild_paths).unwrap();
    let store = MemoryAtomicHostRecord::new("workspace-a");
    let events = Arc::new(RecordingEventSink::default());
    let (runtime, service) = fixture.into_service(store.clone(), events.clone()).await;
    let before = service.current().unwrap();
    let prepared = runtime.prepare_host_workspace_authority(&target).unwrap();
    let transaction = store.transaction(target.to_str().unwrap(), prepared.binding());

    let committed = service
        .compare_and_set_host_workspace_transaction(
            &before.revision,
            prepared,
            "Private Absolute Workspace",
            Box::new(transaction),
        )
        .await
        .unwrap();

    assert_eq!(store.commits.load(Ordering::SeqCst), 1);
    assert_eq!(service.current().unwrap(), committed);
    let host_record = store.snapshot();
    assert_eq!(
        host_record.private_workspace,
        target.to_string_lossy().as_ref()
    );
    assert_eq!(host_record.kernel, store.load().unwrap());
    assert!(!serde_json::to_string(&host_record.kernel)
        .unwrap()
        .contains(target.to_string_lossy().as_ref()));
    {
        let publications = events.publications.lock().unwrap();
        assert_eq!(publications.len(), 1);
        assert!(matches!(
            &publications[0].event,
            DomainEvent::WorkspaceChanged { workspace } if workspace == &committed
        ));
    }

    let rebuilt = WorkspaceService::new(
        &runtime,
        store,
        rebuild_managed,
        Arc::new(RecordingEventSink::default()),
        "Ignored Rebuild Name",
    )
    .await
    .unwrap();
    assert_eq!(rebuilt.current().unwrap(), committed);

    let wire_json = serde_json::to_string(&committed).unwrap();
    assert!(!wire_json.contains(target.to_string_lossy().as_ref()));
    assert!(!wire_json.contains("desktopPath"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_host_switches_are_serialized_and_only_one_revision_wins() {
    let temporary = tempdir().unwrap();
    let target_a = temporary.path().join("Target A");
    let target_b = temporary.path().join("Target B");
    fs::create_dir(&target_a).unwrap();
    fs::create_dir(&target_b).unwrap();
    let store = MemoryAtomicHostRecord::new("workspace-initial");
    let events = Arc::new(RecordingEventSink::default());
    let (runtime, service) = DesktopFixture::new(temporary.path())
        .into_service(store.clone(), events.clone())
        .await;
    let service = Arc::new(service);
    let before = service.current().unwrap();
    let prepared_a = runtime.prepare_host_workspace_authority(&target_a).unwrap();
    let prepared_b = runtime.prepare_host_workspace_authority(&target_b).unwrap();
    let first_transaction = store.transaction("host-a", prepared_a.binding());
    let second_transaction = store.transaction("host-b", prepared_b.binding());
    let (first_started_sender, first_started_receiver) = mpsc::channel();
    let (release_first_sender, release_first_receiver) = mpsc::channel();
    let first_service = service.clone();
    let expected_revision = before.revision.clone();
    let first = tokio::spawn(async move {
        first_service
            .compare_and_set_host_workspace_transaction(
                &expected_revision,
                prepared_a,
                "Target A",
                Box::new(BlockingHostWorkspaceTransaction {
                    inner: first_transaction,
                    started: first_started_sender,
                    release: release_first_receiver,
                }),
            )
            .await
    });
    tokio::task::spawn_blocking(move || {
        first_started_receiver.recv_timeout(Duration::from_secs(5))
    })
    .await
    .unwrap()
    .expect("first transaction must reach its host commit");

    let (second_started_sender, second_started_receiver) = mpsc::channel();
    let second_service = service.clone();
    let expected_revision = before.revision.clone();
    let second = tokio::spawn(async move {
        second_started_sender.send(()).unwrap();
        second_service
            .compare_and_set_host_workspace_transaction(
                &expected_revision,
                prepared_b,
                "Target B",
                Box::new(second_transaction),
            )
            .await
    });
    tokio::task::spawn_blocking(move || {
        second_started_receiver.recv_timeout(Duration::from_secs(5))
    })
    .await
    .unwrap()
    .expect("second task must start while the first transaction is blocked");
    tokio::task::yield_now().await;
    assert!(!second.is_finished());
    assert_eq!(store.commits.load(Ordering::SeqCst), 0);

    release_first_sender.send(()).unwrap();
    let winner = first.await.unwrap().unwrap();
    let loser = second.await.unwrap().unwrap_err();

    assert_eq!(loser.kind(), WorkspaceServiceErrorKind::RevisionConflict);
    assert_eq!(loser.current_revision(), Some(&winner.revision));
    assert_eq!(store.commits.load(Ordering::SeqCst), 1);
    assert_eq!(service.current().unwrap(), winner);
    assert_eq!(events.publications.lock().unwrap().len(), 1);
    assert_eq!(service.current().unwrap().display_name, "Target A");
    assert_eq!(store.snapshot().private_workspace, "host-a");
}

#[tokio::test]
async fn host_commit_may_reenter_read_only_runtime_and_service_checks_without_deadlock() {
    let temporary = tempdir().unwrap();
    let target = temporary.path().join("target");
    fs::create_dir(&target).unwrap();
    let store = MemoryAtomicHostRecord::new("workspace-a");
    let events = Arc::new(RecordingEventSink::default());
    let (runtime, service) = DesktopFixture::new(temporary.path())
        .into_service(store.clone(), events.clone())
        .await;
    let service = Arc::new(service);
    let before = service.current().unwrap();
    let prepared = runtime.prepare_host_workspace_authority(&target).unwrap();
    let inner = store.transaction("workspace-b", prepared.binding());
    let observed = Arc::new(AtomicBool::new(false));
    let transaction = ReadOnlyReentrantHostWorkspaceTransaction {
        inner,
        runtime: runtime.clone(),
        service: service.clone(),
        observed: observed.clone(),
    };
    let (completed_sender, completed_receiver) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let asynchronous = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = asynchronous.block_on(service.compare_and_set_host_workspace_transaction(
            &before.revision,
            prepared,
            "Target",
            Box::new(transaction),
        ));
        completed_sender.send(result).unwrap();
    });

    let committed = completed_receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("read-only host callback must not deadlock")
        .unwrap();
    worker.join().unwrap();

    assert!(observed.load(Ordering::SeqCst));
    assert_eq!(committed.display_name, "Target");
    assert_eq!(store.commits.load(Ordering::SeqCst), 1);
    assert_eq!(events.publications.lock().unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn direct_authority_commit_cannot_cross_atomic_host_transaction_window() {
    let temporary = tempdir().unwrap();
    let service_target = temporary.path().join("service-target");
    let direct_target = temporary.path().join("direct-target");
    fs::create_dir(&service_target).unwrap();
    fs::create_dir(&direct_target).unwrap();
    let store = MemoryAtomicHostRecord::new("workspace-a");
    let events = Arc::new(RecordingEventSink::default());
    let (runtime, service) = DesktopFixture::new(temporary.path())
        .into_service(store.clone(), events.clone())
        .await;
    let before = service.current().unwrap();
    let service_prepared = runtime
        .prepare_host_workspace_authority(&service_target)
        .unwrap();
    let direct_prepared = runtime
        .prepare_host_workspace_authority(&direct_target)
        .unwrap();
    let inner = store.transaction("service-target", service_prepared.binding());
    let (racer_started_sender, racer_started_receiver) = mpsc::channel();
    let (racer_finished_sender, racer_finished_receiver) = mpsc::channel();
    let racer_outcome = Arc::new(Mutex::new(None));
    let racing_runtime = runtime.clone();
    let racer = std::thread::spawn(move || {
        racer_started_receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("host callback must open the direct-commit race window");
        let result = racing_runtime
            .commit_host_workspace_authority(direct_prepared)
            .map(|_| ())
            .map_err(|error| error.kind());
        racer_finished_sender.send(result).unwrap();
    });

    let result = service
        .compare_and_set_host_workspace_transaction(
            &before.revision,
            service_prepared,
            "Service Target",
            Box::new(RacingDirectAuthorityCommitTransaction {
                inner,
                racer_started: racer_started_sender,
                racer_finished: racer_finished_receiver,
                racer_outcome: racer_outcome.clone(),
            }),
        )
        .await;
    racer.join().unwrap();

    assert_eq!(store.commits.load(Ordering::SeqCst), 1);
    assert_eq!(
        *racer_outcome.lock().unwrap(),
        Some(Err(WorkspaceAuthorityErrorKind::WorkspaceUnavailable))
    );
    let committed = result.unwrap();
    assert_eq!(committed.display_name, "Service Target");
    assert_eq!(service.current().unwrap(), committed);
    assert_eq!(store.snapshot().private_workspace, "service-target");
    assert_eq!(events.publications.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn outcome_unknown_quarantines_runtime_not_only_one_service() {
    let temporary = tempdir().unwrap();
    let target = temporary.path().join("target");
    fs::create_dir(&target).unwrap();
    let store = MemoryAtomicHostRecord::new("workspace-a");
    let events = Arc::new(RecordingEventSink::default());
    let fixture = DesktopFixture::new(temporary.path());
    let rebuild_paths =
        KernelPaths::desktop(&fixture.workspace, &fixture.app_data, &fixture.cache).unwrap();
    let second_managed = ManagedWorkspaceCollection::from_paths(&rebuild_paths).unwrap();
    let (runtime, service) = fixture.into_service(store.clone(), events.clone()).await;
    let second = WorkspaceService::new(
        &runtime,
        store.clone(),
        second_managed,
        Arc::new(RecordingEventSink::default()),
        "Ignored",
    )
    .await
    .unwrap();
    let before = service.current().unwrap();
    let prepared = runtime.prepare_host_workspace_authority(&target).unwrap();
    let mut transaction = store.transaction("workspace-b", prepared.binding());
    transaction.outcome_unknown_after_commit = true;

    let error = service
        .compare_and_set_host_workspace_transaction(
            &before.revision,
            prepared,
            "Target",
            Box::new(transaction),
        )
        .await
        .unwrap_err();

    assert_eq!(
        error.kind(),
        WorkspaceServiceErrorKind::PersistenceUnavailable
    );
    assert_eq!(
        runtime.active_workspace_authority().unwrap_err().kind(),
        WorkspaceAuthorityErrorKind::WorkspaceUnavailable
    );
    assert_eq!(
        service.current().unwrap_err().kind(),
        WorkspaceServiceErrorKind::WorkspaceUnavailable
    );
    assert_eq!(
        second.current().unwrap_err().kind(),
        WorkspaceServiceErrorKind::WorkspaceUnavailable
    );
    assert_eq!(
        runtime.active_workspace_snapshot().unwrap_err().kind(),
        WorkspaceAuthorityErrorKind::WorkspaceUnavailable
    );
    assert!(events.publications.lock().unwrap().is_empty());
}

#[tokio::test]
async fn quarantine_cannot_be_cleared_by_rebuilding_workspace_service() {
    let temporary = tempdir().unwrap();
    let target = temporary.path().join("target");
    fs::create_dir(&target).unwrap();
    let fixture = DesktopFixture::new(temporary.path());
    let rebuild_paths =
        KernelPaths::desktop(&fixture.workspace, &fixture.app_data, &fixture.cache).unwrap();
    let rebuild_managed = ManagedWorkspaceCollection::from_paths(&rebuild_paths).unwrap();
    let store = MemoryAtomicHostRecord::new("workspace-a");
    let events = Arc::new(RecordingEventSink::default());
    let (runtime, service) = fixture.into_service(store.clone(), events).await;
    let before = service.current().unwrap();
    let prepared = runtime.prepare_host_workspace_authority(&target).unwrap();
    let mut transaction = store.transaction("workspace-b", prepared.binding());
    transaction.outcome_unknown_after_commit = true;
    service
        .compare_and_set_host_workspace_transaction(
            &before.revision,
            prepared,
            "Target",
            Box::new(transaction),
        )
        .await
        .unwrap_err();
    drop(service);

    let rebuilt = WorkspaceService::new(
        &runtime,
        store,
        rebuild_managed,
        Arc::new(RecordingEventSink::default()),
        "Ignored",
    )
    .await;
    let error = match rebuilt {
        Ok(_) => panic!("runtime recovery must reject a rebuilt workspace service"),
        Err(error) => error,
    };

    assert_eq!(
        error.kind(),
        WorkspaceServiceErrorKind::WorkspaceUnavailable
    );
    assert_eq!(
        runtime.active_workspace_snapshot().unwrap_err().kind(),
        WorkspaceAuthorityErrorKind::WorkspaceUnavailable
    );
    assert_eq!(
        runtime
            .prepare_host_workspace_authority(&target)
            .unwrap_err()
            .kind(),
        WorkspaceAuthorityErrorKind::WorkspaceUnavailable
    );
}

#[tokio::test]
async fn outcome_unknown_retains_old_and_candidate_workspace_leases() {
    let temporary = tempdir().unwrap();
    let old_workspace = temporary.path().join("workspace");
    let target = temporary.path().join("target");
    fs::create_dir(&target).unwrap();
    let store = MemoryAtomicHostRecord::new("workspace-a");
    let events = Arc::new(RecordingEventSink::default());
    let (runtime, service) = DesktopFixture::new(temporary.path())
        .into_service(store.clone(), events)
        .await;
    let before = service.current().unwrap();
    let prepared = runtime.prepare_host_workspace_authority(&target).unwrap();
    let mut transaction = store.transaction("workspace-b", prepared.binding());
    transaction.outcome_unknown_after_commit = true;

    service
        .compare_and_set_host_workspace_transaction(
            &before.revision,
            prepared,
            "Target",
            Box::new(transaction),
        )
        .await
        .unwrap_err();
    drop(service);

    assert_workspace_is_locked(&old_workspace, temporary.path(), "recovery-old");
    assert_workspace_is_locked(&target, temporary.path(), "recovery-candidate");
    drop(runtime);
    assert_workspace_is_acquirable(&old_workspace, temporary.path(), "released-old");
    assert_workspace_is_acquirable(&target, temporary.path(), "released-candidate");
}

#[tokio::test]
async fn conflict_and_no_commit_do_not_quarantine_runtime() {
    let temporary = tempdir().unwrap();

    for failure in ["conflict", "no-commit"] {
        let root = temporary.path().join(failure);
        let target = root.join("target");
        fs::create_dir_all(&target).unwrap();
        let store = MemoryAtomicHostRecord::new("workspace-a");
        let events = Arc::new(RecordingEventSink::default());
        let (runtime, service) = DesktopFixture::new(&root)
            .into_service(store.clone(), events.clone())
            .await;
        let before = service.current().unwrap();
        let before_snapshot = runtime.active_workspace_snapshot().unwrap();
        let prepared = runtime.prepare_host_workspace_authority(&target).unwrap();
        let mut transaction = store.transaction("workspace-b", prepared.binding());
        if failure == "conflict" {
            store.replace_private_workspace("external-host-change");
        } else {
            transaction.fail_persist = true;
        }

        let error = service
            .compare_and_set_host_workspace_transaction(
                &before.revision,
                prepared,
                "Target",
                Box::new(transaction),
            )
            .await
            .unwrap_err();

        assert_eq!(
            error.kind(),
            WorkspaceServiceErrorKind::PersistenceUnavailable
        );
        assert!(Arc::ptr_eq(
            &before_snapshot,
            &runtime.active_workspace_snapshot().unwrap()
        ));
        assert_eq!(service.current().unwrap(), before);
        assert!(events.publications.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn candidate_replacement_after_durable_commit_quarantines_without_publication() {
    let temporary = tempdir().unwrap();
    let target = temporary.path().join("target");
    let displaced = temporary.path().join("target-displaced");
    fs::create_dir(&target).unwrap();
    let store = MemoryAtomicHostRecord::new("workspace-a");
    let events = Arc::new(RecordingEventSink::default());
    let (runtime, service) = DesktopFixture::new(temporary.path())
        .into_service(store.clone(), events.clone())
        .await;
    let before = service.current().unwrap();
    let prepared = runtime.prepare_host_workspace_authority(&target).unwrap();
    let mut transaction = store.transaction("workspace-b", prepared.binding());
    transaction.replace_target_after_commit = Some((target.clone(), displaced.clone()));

    let error = service
        .compare_and_set_host_workspace_transaction(
            &before.revision,
            prepared,
            "Target",
            Box::new(transaction),
        )
        .await
        .unwrap_err();

    assert_eq!(
        error.kind(),
        WorkspaceServiceErrorKind::PersistenceUnavailable
    );
    assert_eq!(
        runtime.active_workspace_authority().unwrap_err().kind(),
        WorkspaceAuthorityErrorKind::WorkspaceUnavailable
    );
    assert_eq!(
        service.current().unwrap_err().kind(),
        WorkspaceServiceErrorKind::WorkspaceUnavailable
    );
    assert!(events.publications.lock().unwrap().is_empty());
    assert!(displaced.is_dir());
    assert!(target.is_dir());
}

#[tokio::test]
async fn successful_cas_persists_before_one_event_and_rotates_runtime_identity() {
    let temporary = tempdir().unwrap();
    let next = temporary.path().join("next");
    fs::create_dir(&next).unwrap();
    let order = Arc::new(Mutex::new(Vec::new()));
    let store = Arc::new(MemoryPrimaryWorkspaceStore {
        order: order.clone(),
        ..MemoryPrimaryWorkspaceStore::default()
    });
    let events = Arc::new(RecordingEventSink {
        order: order.clone(),
        ..RecordingEventSink::default()
    });
    let (runtime, service) = DesktopFixture::new(temporary.path())
        .into_service(store.clone(), events.clone())
        .await;
    let before = service.current().unwrap();
    order.lock().unwrap().clear();
    let prepared = runtime.prepare_host_workspace_authority(&next).unwrap();

    let committed = service
        .compare_and_set_host_workspace(&before.revision, prepared, "Next Workspace")
        .await
        .unwrap();

    assert_eq!(*order.lock().unwrap(), vec!["save", "event"]);
    assert_ne!(committed.id, before.id);
    assert_ne!(committed.generation, before.generation);
    assert_ne!(committed.revision, before.revision);
    assert_eq!(service.current().unwrap(), committed);
    assert_eq!(store.load().unwrap(), store.durable());
    let publications = events.publications.lock().unwrap();
    assert_eq!(publications.len(), 1);
    assert_eq!(publications[0].revision, committed.revision);
    assert!(matches!(
        &publications[0].resource,
        ResourceRefDto::Workspace { id } if *id == committed.id
    ));
    assert!(matches!(
        &publications[0].event,
        DomainEvent::WorkspaceChanged { workspace } if workspace == &committed
    ));
}

#[tokio::test]
async fn event_failure_does_not_roll_back_the_durable_workspace_commit() {
    let temporary = tempdir().unwrap();
    let next = temporary.path().join("next");
    fs::create_dir(&next).unwrap();
    let store = Arc::new(MemoryPrimaryWorkspaceStore::default());
    let events = Arc::new(RecordingEventSink::default());
    events.fail.store(true, Ordering::SeqCst);
    let (runtime, service) = DesktopFixture::new(temporary.path())
        .into_service(store.clone(), events.clone())
        .await;
    let before = service.current().unwrap();
    let prepared = runtime.prepare_host_workspace_authority(&next).unwrap();

    let committed = service
        .compare_and_set_host_workspace(&before.revision, prepared, "Next Workspace")
        .await
        .unwrap();

    assert_eq!(service.current().unwrap(), committed);
    assert_ne!(committed.revision, before.revision);
    assert_eq!(store.load().unwrap(), store.durable());
    assert_eq!(events.publications.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn legacy_rollback_failure_quarantines_runtime() {
    let temporary = tempdir().unwrap();
    let target = temporary.path().join("legacy-target");
    let displaced = temporary.path().join("legacy-target-displaced");
    fs::create_dir(&target).unwrap();
    let store = Arc::new(MemoryPrimaryWorkspaceStore::default());
    let events = Arc::new(RecordingEventSink::default());
    let fixture = DesktopFixture::new(temporary.path());
    let rebuild_paths =
        KernelPaths::desktop(&fixture.workspace, &fixture.app_data, &fixture.cache).unwrap();
    let rebuild_managed = ManagedWorkspaceCollection::from_paths(&rebuild_paths).unwrap();
    let (runtime, service) = fixture.into_service(store.clone(), events.clone()).await;
    let before = service.current().unwrap();
    let prepared = runtime.prepare_host_workspace_authority(&target).unwrap();
    store.replace_target_on_next_save(target, displaced);
    store.fail_replace_on_call(store.replaces.load(Ordering::SeqCst) + 2);

    let error = service
        .compare_and_set_host_workspace(&before.revision, prepared, "Legacy Target")
        .await
        .unwrap_err();

    assert_eq!(
        error.kind(),
        WorkspaceServiceErrorKind::PersistenceUnavailable
    );
    assert_eq!(
        runtime.active_workspace_snapshot().unwrap_err().kind(),
        WorkspaceAuthorityErrorKind::WorkspaceUnavailable
    );
    let rebuilt = WorkspaceService::new(
        &runtime,
        store,
        rebuild_managed,
        Arc::new(RecordingEventSink::default()),
        "Ignored",
    )
    .await;
    assert!(rebuilt.is_err());
    assert!(events.publications.lock().unwrap().is_empty());
}

#[tokio::test]
async fn managed_list_is_sorted_shallow_read_only_and_instance_scoped() {
    let temporary = tempdir().unwrap();
    let first_fixture = DesktopFixture::new(&temporary.path().join("first"));
    let first_app_data = first_fixture.app_data.clone();
    let (_first_runtime, first) = first_fixture
        .into_service(
            Arc::new(MemoryPrimaryWorkspaceStore::default()),
            Arc::new(RecordingEventSink::default()),
        )
        .await;
    let second_fixture = DesktopFixture::new(&temporary.path().join("second"));
    let (_second_runtime, second) = second_fixture
        .into_service(
            Arc::new(MemoryPrimaryWorkspaceStore::default()),
            Arc::new(RecordingEventSink::default()),
        )
        .await;

    assert_eq!(
        first.list_managed_workspaces().unwrap(),
        Vec::<String>::new()
    );
    assert!(!first_app_data.join("workspaces").exists());

    for name in ["beta", "Alpha", "随笔"] {
        assert_eq!(first.create_managed_workspace(name).await.unwrap(), name);
    }
    second
        .create_managed_workspace("second-only")
        .await
        .unwrap();
    fs::create_dir_all(first_app_data.join("workspaces/beta/nested")).unwrap();
    fs::write(first_app_data.join("workspaces/file.md"), "not a workspace").unwrap();
    fs::create_dir(first_app_data.join("workspaces/.qingyu")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let outside = temporary.path().join("outside-list");
        fs::create_dir(&outside).unwrap();
        symlink(&outside, first_app_data.join("workspaces/linked")).unwrap();
    }

    assert_eq!(
        first.list_managed_workspaces().unwrap(),
        vec!["Alpha".to_string(), "beta".to_string(), "随笔".to_string()]
    );
    assert_eq!(
        second.list_managed_workspaces().unwrap(),
        vec!["second-only".to_string()]
    );
}

#[cfg(unix)]
#[tokio::test]
async fn managed_create_rejects_collection_and_child_symlinks_without_writing_through() {
    use std::os::unix::fs::symlink;

    let temporary = tempdir().unwrap();
    let collection_fixture = DesktopFixture::new(&temporary.path().join("collection"));
    let collection_app_data = collection_fixture.app_data.clone();
    let outside_collection = temporary.path().join("outside-collection");
    fs::create_dir(&outside_collection).unwrap();
    symlink(&outside_collection, collection_app_data.join("workspaces")).unwrap();
    let (_runtime, collection_service) = collection_fixture
        .into_service(
            Arc::new(MemoryPrimaryWorkspaceStore::default()),
            Arc::new(RecordingEventSink::default()),
        )
        .await;

    let collection_error = collection_service
        .create_managed_workspace("personal")
        .await
        .unwrap_err();

    assert_eq!(
        collection_error.kind(),
        WorkspaceServiceErrorKind::UnsafeManagedWorkspace
    );
    assert!(fs::read_dir(&outside_collection).unwrap().next().is_none());

    let child_fixture = DesktopFixture::new(&temporary.path().join("child"));
    let child_app_data = child_fixture.app_data.clone();
    let outside_child = temporary.path().join("outside-child");
    fs::create_dir_all(child_app_data.join("workspaces")).unwrap();
    fs::create_dir(&outside_child).unwrap();
    symlink(&outside_child, child_app_data.join("workspaces/personal")).unwrap();
    let (_runtime, child_service) = child_fixture
        .into_service(
            Arc::new(MemoryPrimaryWorkspaceStore::default()),
            Arc::new(RecordingEventSink::default()),
        )
        .await;

    let child_error = child_service
        .create_managed_workspace("personal")
        .await
        .unwrap_err();

    assert_eq!(
        child_error.kind(),
        WorkspaceServiceErrorKind::UnsafeManagedWorkspace
    );
    assert!(fs::read_dir(&outside_child).unwrap().next().is_none());
}

#[tokio::test]
async fn retained_managed_capability_fails_closed_when_its_ambient_address_is_replaced() {
    let temporary = tempdir().unwrap();
    let fixture = DesktopFixture::new(temporary.path());
    let app_data = fixture.app_data.clone();
    let displaced = temporary.path().join("displaced-app-data");
    let (_runtime, service) = fixture
        .into_service(
            Arc::new(MemoryPrimaryWorkspaceStore::default()),
            Arc::new(RecordingEventSink::default()),
        )
        .await;
    fs::rename(&app_data, &displaced).unwrap();
    fs::create_dir(&app_data).unwrap();

    let error = service.list_managed_workspaces().unwrap_err();

    assert_eq!(
        error.kind(),
        WorkspaceServiceErrorKind::UnsafeManagedWorkspace
    );
    assert!(!app_data.join("workspaces").exists());
    assert!(!displaced.join("workspaces").exists());
}

#[tokio::test]
async fn commit_address_replacement_restores_persisted_state_and_maps_to_workspace_unavailable() {
    let temporary = tempdir().unwrap();
    let next = temporary.path().join("next");
    let displaced = temporary.path().join("next-displaced");
    fs::create_dir(&next).unwrap();
    let store = Arc::new(MemoryPrimaryWorkspaceStore::default());
    let events = Arc::new(RecordingEventSink::default());
    let (runtime, service) = DesktopFixture::new(temporary.path())
        .into_service(store.clone(), events.clone())
        .await;
    let before = service.current().unwrap();
    let before_store = store.durable();
    let before_authority = runtime
        .active_workspace_authority()
        .expect("workspace authority");
    let prepared = runtime.prepare_host_workspace_authority(&next).unwrap();
    fs::rename(&next, &displaced).unwrap();
    fs::create_dir(&next).unwrap();

    let error = service
        .compare_and_set_host_workspace(&before.revision, prepared, "Next Workspace")
        .await
        .unwrap_err();

    assert_eq!(
        error.kind(),
        WorkspaceServiceErrorKind::WorkspaceUnavailable
    );
    assert_eq!(store.load().unwrap(), before_store);
    assert_eq!(store.durable(), before_store);
    assert_eq!(service.current().unwrap(), before);
    assert!(Arc::ptr_eq(
        &before_authority,
        &runtime
            .active_workspace_authority()
            .expect("workspace authority")
    ));
    assert!(events.publications.lock().unwrap().is_empty());
}

#[tokio::test]
async fn api_maps_replaced_active_workspace_address_to_safe_unavailable() {
    let temporary = tempdir().unwrap();
    let fixture = DesktopFixture::new(temporary.path());
    let workspace = fixture.workspace.clone();
    let displaced = temporary.path().join("workspace-displaced");
    let (_runtime, service) = fixture
        .into_service(
            Arc::new(MemoryPrimaryWorkspaceStore::default()),
            Arc::new(RecordingEventSink::default()),
        )
        .await;
    fs::rename(&workspace, &displaced).unwrap();
    fs::create_dir(&workspace).unwrap();

    let direct = service.current().unwrap_err();
    let api = WorkspaceApiService::get_workspace(&service)
        .await
        .unwrap_err();

    assert_eq!(
        direct.kind(),
        WorkspaceServiceErrorKind::WorkspaceUnavailable
    );
    assert_eq!(
        api.code(),
        qingyu_kernel::contract::ErrorCode::WorkspaceUnavailable
    );
    assert!(api.details().is_none());
    assert!(!format!("{direct:?}").contains(temporary.path().to_string_lossy().as_ref()));
}

#[cfg(unix)]
#[tokio::test]
async fn constructor_verifies_both_lock_addresses_before_any_store_access() {
    for lock_kind in ["instance", "workspace"] {
        let temporary = tempdir().unwrap();
        let fixture = DesktopFixture::new(temporary.path());
        let lock_path = match lock_kind {
            "instance" => fixture.app_data.join("kernel.lock"),
            "workspace" => fixture.workspace.join(".qingyu/workspace.lock"),
            _ => unreachable!(),
        };
        let displaced = temporary.path().join(format!("{lock_kind}-lock-displaced"));
        fs::rename(&lock_path, &displaced).unwrap();
        fs::write(&lock_path, "replacement").unwrap();
        let store = Arc::new(MemoryPrimaryWorkspaceStore::default());

        let result = WorkspaceService::new(
            &fixture.runtime,
            store.clone(),
            fixture.managed,
            Arc::new(RecordingEventSink::default()),
            "Initial Workspace",
        )
        .await;
        let error = match result {
            Ok(_) => panic!("{lock_kind} lock replacement must fail before persistence"),
            Err(error) => error,
        };

        assert_eq!(
            error.kind(),
            WorkspaceServiceErrorKind::WorkspaceUnavailable
        );
        assert_eq!(store.access_counts(), (0, 0, 0));
    }
}

fn assert_workspace_identity_is_populated(workspace: &WorkspaceDto) {
    assert!(!workspace.id.as_uuid().is_nil());
    assert!(!workspace.generation.as_str().is_empty());
    assert!(!workspace.revision.as_str().is_empty());
}

fn assert_workspace_is_locked(workspace: &std::path::Path, root: &std::path::Path, label: &str) {
    let app_data = root.join(format!("{label}-app-data"));
    let cache = root.join(format!("{label}-cache"));
    fs::create_dir_all(&app_data).unwrap();
    fs::create_dir_all(&cache).unwrap();
    let paths = KernelPaths::desktop(workspace, &app_data, &cache).unwrap();

    let error = KernelRuntime::activate(
        KernelConfig::generate().unwrap(),
        paths,
        KernelPorts::unavailable(),
    )
    .unwrap_err();

    assert_eq!(error.kind(), KernelStartupErrorKind::WorkspaceLocked);
}

fn assert_workspace_is_acquirable(
    workspace: &std::path::Path,
    root: &std::path::Path,
    label: &str,
) {
    let app_data = root.join(format!("{label}-app-data"));
    let cache = root.join(format!("{label}-cache"));
    fs::create_dir_all(&app_data).unwrap();
    fs::create_dir_all(&cache).unwrap();
    let paths = KernelPaths::desktop(workspace, &app_data, &cache).unwrap();

    let runtime = KernelRuntime::activate(
        KernelConfig::generate().unwrap(),
        paths,
        KernelPorts::unavailable(),
    )
    .unwrap();
    drop(runtime);
}
