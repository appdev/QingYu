use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Condvar, Mutex,
};
use std::time::Duration;

use async_trait::async_trait;
use axum::{
    body::{to_bytes, Body},
    http::{header, Request, StatusCode},
};
use qingyu_kernel::{
    api::{build_router, TransportPolicy},
    config::KernelConfig,
    contract::{
        ApiErrorEnvelope, CredentialChange, DomainEvent, ErrorCode, ErrorDetails,
        PatchSyncConfigRequest, ResourceRefDto, Revision, Rfc3339Utc, RunId, SyncCompletionState,
        SyncConfigChangesDto, SyncConfigReadiness, SyncProvider, SyncTrigger,
        TriggerSyncRunRequest,
    },
    events::{EventPublication, EventSink, EventSinkError},
    paths::KernelPaths,
    ports::{
        BoxSleepFuture, BoxTaskFuture, Clock, CredentialSecret, CredentialSlot, CredentialStore,
        DiagnosticRecord, DiagnosticsSink, KernelPorts, NetworkReachability, PortError, Sleeper,
        TaskSpawner,
    },
    runtime::{KernelRuntime, SyncApiService},
    services::{
        sync::{SyncExecutionError, SyncExecutor, SyncService},
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
            PrimaryWorkspaceRepositoryBinding, PrimaryWorkspaceStore, PrimaryWorkspaceStoreError,
        },
    },
};
use sha2::Digest as _;
use tempfile::tempdir;
use tokio::sync::Notify;
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
        _run_id: RunId,
        _trigger: SyncTrigger,
    ) -> Result<(), SyncExecutionError> {
        Ok(())
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
    value: Mutex<Option<serde_json::Value>>,
    failing_saves: AtomicUsize,
}

impl MemoryPrimaryWorkspaceStore {
    fn fail_next_saves(&self, count: usize) {
        self.failing_saves.store(count, Ordering::SeqCst);
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

struct InstalledWorkspace {
    service: Arc<WorkspaceService>,
    store: Arc<MemoryPrimaryWorkspaceStore>,
}

fn install_active_workspace(
    runtime: &Arc<KernelRuntime>,
    managed: ManagedWorkspaceCollection,
) -> InstalledWorkspace {
    let store = Arc::new(MemoryPrimaryWorkspaceStore::default());
    let service = Arc::new(
        WorkspaceService::new(
            runtime,
            store.clone(),
            managed,
            Arc::new(TestHost),
            "Workspace",
        )
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
        _run_id: RunId,
        _trigger: SyncTrigger,
    ) -> Result<(), SyncExecutionError> {
        self.runs.fetch_add(1, Ordering::Relaxed);
        Ok(())
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

#[async_trait]
impl SyncExecutor for BlockingExecutor {
    async fn test_connection(&self, _config: SyncConfig) -> Result<(), SyncExecutionError> {
        Ok(())
    }

    async fn run(
        &self,
        _config: SyncConfig,
        _run_id: RunId,
        _trigger: SyncTrigger,
    ) -> Result<(), SyncExecutionError> {
        self.runs.fetch_add(1, Ordering::Relaxed);
        self.started.notify_one();
        self.release.notified().await;
        Ok(())
    }
}

#[async_trait]
impl SyncExecutor for ManualExecutor {
    async fn test_connection(&self, _config: SyncConfig) -> Result<(), SyncExecutionError> {
        Ok(())
    }

    async fn run(
        &self,
        _config: SyncConfig,
        _run_id: RunId,
        _trigger: SyncTrigger,
    ) -> Result<(), SyncExecutionError> {
        self.runs.fetch_add(1, Ordering::Relaxed);
        self.completed.notify_one();
        if self.fail.load(Ordering::Relaxed) {
            Err(SyncExecutionError)
        } else {
            Ok(())
        }
    }
}

#[derive(Default)]
struct TestHost;

#[derive(Default)]
struct DeferredTaskSpawner {
    spawned: AtomicUsize,
    task: Mutex<Option<BoxTaskFuture>>,
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
struct CompletionPublicationGate {
    completion_entered: Notify,
    config_entered: Notify,
    released: (Mutex<bool>, Condvar),
}

impl CompletionPublicationGate {
    async fn wait_for_completion_publication(&self) {
        self.completion_entered.notified().await;
    }

    async fn wait_for_config_publication(&self) {
        self.config_entered.notified().await;
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
            DomainEvent::SyncConfigChanged { .. } => self.config_entered.notify_one(),
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

fn active_sync_runtime(
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
    let durable =
        DurableFileStore::at_instance_data(paths.instance_data_root(), config.launch_epoch())
            .unwrap();
    let runtime = KernelRuntime::activate(config, paths, ports).unwrap();
    let workspace = install_active_workspace(&runtime, managed);
    (runtime, workspace, durable)
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
    let durable =
        DurableFileStore::at_instance_data(paths.instance_data_root(), config.launch_epoch())
            .unwrap();
    let runtime = KernelRuntime::activate(config, paths, KernelPorts::unavailable()).unwrap();
    let service = SyncService::new(
        runtime,
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
    let durable =
        DurableFileStore::at_instance_data(paths.instance_data_root(), config.launch_epoch())
            .unwrap();
    let runtime = KernelRuntime::activate(config, paths, KernelPorts::unavailable()).unwrap();
    let service = SyncService::new(
        runtime,
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
    let durable =
        DurableFileStore::at_instance_data(paths.instance_data_root(), config.launch_epoch())
            .unwrap();
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
    let durable =
        DurableFileStore::at_instance_data(paths.instance_data_root(), config.launch_epoch())
            .unwrap();
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
    let durable =
        DurableFileStore::at_instance_data(paths.instance_data_root(), config.launch_epoch())
            .unwrap();
    let runtime = KernelRuntime::activate(config, paths, KernelPorts::unavailable()).unwrap();
    let mut events = runtime.event_broker().subscribe();
    let service = SyncService::new(
        runtime,
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
    let durable =
        DurableFileStore::at_instance_data(paths.instance_data_root(), config.launch_epoch())
            .unwrap();
    let runtime = KernelRuntime::activate(config, paths, KernelPorts::unavailable()).unwrap();
    let service = SyncService::new(
        runtime,
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
            DurableFileStore::at_instance_data(paths.instance_data_root(), config.launch_epoch())
                .unwrap();
        let runtime = KernelRuntime::activate(config, paths, KernelPorts::unavailable()).unwrap();
        let service = SyncService::new(
            runtime,
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
    let durable =
        DurableFileStore::at_instance_data(paths.instance_data_root(), config.launch_epoch())
            .unwrap();
    let runtime = KernelRuntime::activate(config, paths, KernelPorts::unavailable()).unwrap();
    let service = SyncService::new(
        runtime,
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
    let first_durable = DurableFileStore::at_instance_data(
        first_paths.instance_data_root(),
        first_config.launch_epoch(),
    )
    .unwrap();
    let first_runtime =
        KernelRuntime::activate(first_config, first_paths, KernelPorts::unavailable()).unwrap();
    let first_service = SyncService::new(
        first_runtime,
        Arc::new(SyncConfigStore::new(first_durable).unwrap()),
        Arc::new(RecordingExecutor),
    );
    let second_config = KernelConfig::generate().unwrap();
    let second_paths =
        KernelPaths::desktop(&second_workspace, &second_app_data, &second_cache).unwrap();
    let second_durable = DurableFileStore::at_instance_data(
        second_paths.instance_data_root(),
        second_config.launch_epoch(),
    )
    .unwrap();
    let second_runtime =
        KernelRuntime::activate(second_config, second_paths, KernelPorts::unavailable()).unwrap();
    let second_service = SyncService::new(
        second_runtime,
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
            DurableFileStore::at_instance_data(paths.instance_data_root(), config.launch_epoch())
                .unwrap();
        let runtime = KernelRuntime::activate(config, paths, KernelPorts::unavailable()).unwrap();
        let service = SyncService::new(
            runtime,
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
    let durable =
        DurableFileStore::at_instance_data(paths.instance_data_root(), config.launch_epoch())
            .unwrap();
    let runtime = KernelRuntime::activate(config, paths, KernelPorts::unavailable()).unwrap();
    let service = SyncService::new(
        runtime,
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
    let durable =
        DurableFileStore::at_instance_data(paths.instance_data_root(), config.launch_epoch())
            .unwrap();
    let runtime = KernelRuntime::activate(config, paths, KernelPorts::unavailable()).unwrap();
    let _workspace = install_active_workspace(&runtime, managed);
    let executor = Arc::new(CountingExecutor::default());
    let service = SyncService::new(
        runtime,
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
    let durable =
        DurableFileStore::at_instance_data(paths.instance_data_root(), config.launch_epoch())
            .unwrap();
    let runtime = KernelRuntime::activate(config, paths, KernelPorts::unavailable()).unwrap();
    let service = SyncService::new(
        runtime,
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
    let durable =
        DurableFileStore::at_instance_data(paths.instance_data_root(), config.launch_epoch())
            .unwrap();
    let runtime = KernelRuntime::activate(config, paths, test_ports()).unwrap();
    let _workspace = install_active_workspace(&runtime, managed);
    let mut events = runtime.event_broker().subscribe();
    let executor = Arc::new(ManualExecutor::default());
    let service = SyncService::new(
        runtime,
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
            DurableFileStore::at_instance_data(paths.instance_data_root(), config.launch_epoch())
                .unwrap();
        let runtime = KernelRuntime::activate(config, paths, test_ports()).unwrap();
        let _workspace = install_active_workspace(&runtime, managed);
        let executor = Arc::new(ManualExecutor::default());
        executor.fail.store(failed, Ordering::Relaxed);
        let service = SyncService::new(
            runtime,
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
    let durable =
        DurableFileStore::at_instance_data(paths.instance_data_root(), config.launch_epoch())
            .unwrap();
    let runtime = KernelRuntime::activate(config, paths, test_ports()).unwrap();
    let _workspace = install_active_workspace(&runtime, managed);
    let executor = Arc::new(BlockingExecutor::default());
    let service = SyncService::new(
        runtime,
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
    let durable =
        DurableFileStore::at_instance_data(paths.instance_data_root(), config.launch_epoch())
            .unwrap();
    let runtime = KernelRuntime::activate(config, paths, test_ports()).unwrap();
    let _workspace = install_active_workspace(&runtime, managed);
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
    let durable =
        DurableFileStore::at_instance_data(paths.instance_data_root(), config.launch_epoch())
            .unwrap();
    let publication_gate = Arc::new(CompletionPublicationGate::default());
    let runtime = KernelRuntime::activate(
        config,
        paths,
        test_ports_with_event_sink(publication_gate.clone()),
    )
    .unwrap();
    let _workspace = install_active_workspace(&runtime, managed);
    let mut events = runtime.event_broker().subscribe();
    let executor = Arc::new(BlockingExecutor::default());
    let service = Arc::new(SyncService::new(
        runtime,
        Arc::new(SyncConfigStore::new(durable).unwrap()),
        executor.clone(),
    ));
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
    let _patch_reached_publication = tokio::time::timeout(
        Duration::from_millis(100),
        publication_gate.wait_for_config_publication(),
    )
    .await;
    publication_gate.release_completion_publication();
    let after = patch.await.unwrap().unwrap();

    let mut observed = Vec::new();
    for _ in 0..3 {
        observed.push(events.recv().await.unwrap());
    }
    let completed_index = observed
        .iter()
        .position(|publication| {
            matches!(
                &publication.event,
                DomainEvent::SyncStatusChanged { status }
                    if status.completion_state == SyncCompletionState::Succeeded
            )
        })
        .unwrap();
    let config_index = observed
        .iter()
        .position(|publication| matches!(publication.event, DomainEvent::SyncConfigChanged { .. }))
        .unwrap();
    let idle_index = observed
        .iter()
        .position(|publication| {
            matches!(
                &publication.event,
                DomainEvent::SyncStatusChanged { status }
                    if status.completion_state == SyncCompletionState::Idle
                        && status.config_revision.as_ref() == Some(&after.revision)
            )
        })
        .unwrap();

    assert!(completed_index < config_index);
    assert!(config_index < idle_index);
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
    let http_durable = DurableFileStore::at_instance_data(
        http_paths.instance_data_root(),
        http_config.launch_epoch(),
    )
    .unwrap();
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
    let direct_durable = DurableFileStore::at_instance_data(
        direct_paths.instance_data_root(),
        direct_config.launch_epoch(),
    )
    .unwrap();
    let direct_runtime =
        KernelRuntime::activate(direct_config, direct_paths, KernelPorts::unavailable()).unwrap();
    let mut direct_events = direct_runtime.event_broker().subscribe();
    let direct_service = SyncService::new(
        direct_runtime,
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
    );
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
        active_sync_runtime(temporary.path(), KernelPorts::unavailable());
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
    );
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
    );
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
            DurableFileStore::at_instance_data(paths.instance_data_root(), config.launch_epoch())
                .unwrap();
        let runtime = KernelRuntime::activate(config, paths, KernelPorts::unavailable()).unwrap();
        let mut events = runtime.event_broker().subscribe();
        let store = Arc::new(SyncConfigStore::new(durable).unwrap());
        let loaded = store.load().unwrap();
        let loaded_debug = match &loaded {
            SyncConfigLoad::Loaded { config, .. } => format!("{config:?}"),
            other => panic!("expected loaded sync config, got {other:?}"),
        };
        let service = SyncService::new(runtime, store, Arc::new(RecordingExecutor));

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
            DurableFileStore::at_instance_data(paths.instance_data_root(), config.launch_epoch())
                .unwrap();
        let runtime = KernelRuntime::activate(config, paths, KernelPorts::unavailable()).unwrap();
        let mut events = runtime.event_broker().subscribe();
        let service = SyncService::new(
            runtime,
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
    let durable =
        DurableFileStore::at_instance_data(paths.instance_data_root(), config.launch_epoch())
            .unwrap();
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
    let durable =
        DurableFileStore::at_instance_data(paths.instance_data_root(), config.launch_epoch())
            .unwrap();
    let runtime = KernelRuntime::activate(config, paths, KernelPorts::unavailable()).unwrap();
    let store = Arc::new(SyncConfigStore::new(durable).unwrap());
    let service = SyncService::new(runtime, store, Arc::new(RecordingExecutor));

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
    let durable =
        DurableFileStore::at_instance_data(paths.instance_data_root(), config.launch_epoch())
            .unwrap();
    let runtime = KernelRuntime::activate(config, paths, KernelPorts::unavailable()).unwrap();
    let store = Arc::new(SyncConfigStore::new(durable).unwrap());
    let service = SyncService::new(runtime, store, Arc::new(RecordingExecutor));

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
