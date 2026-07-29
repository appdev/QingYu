use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

use qingyu_kernel::{
    config::KernelConfig,
    contract::{DomainEvent, ResourceRefDto, Revision, WorkspaceDto},
    events::{EventPublication, EventSink, EventSinkError},
    paths::KernelPaths,
    ports::KernelPorts,
    runtime::{KernelRuntime, WorkspaceApiService},
    services::workspace::{WorkspaceService, WorkspaceServiceErrorKind},
    workspace::{
        managed::ManagedWorkspaceCollection,
        primary::{
            AtomicHostWorkspaceCommitError, AtomicHostWorkspaceTransaction, PrimaryWorkspaceStore,
            PrimaryWorkspaceStoreError,
        },
    },
};
use serde_json::Value;
use tempfile::tempdir;

#[derive(Default)]
struct MemoryPrimaryWorkspaceStore {
    value: Mutex<Option<Value>>,
    durable: Mutex<Option<Value>>,
    fail_next_save: AtomicBool,
    loads: std::sync::atomic::AtomicUsize,
    replaces: std::sync::atomic::AtomicUsize,
    saves: std::sync::atomic::AtomicUsize,
    order: Arc<Mutex<Vec<&'static str>>>,
}

impl PrimaryWorkspaceStore for MemoryPrimaryWorkspaceStore {
    fn load(&self) -> Result<Option<Value>, PrimaryWorkspaceStoreError> {
        self.loads.fetch_add(1, Ordering::SeqCst);
        Ok(self.value.lock().unwrap().clone())
    }

    fn replace(&self, value: Option<Value>) -> Result<(), PrimaryWorkspaceStoreError> {
        self.replaces.fetch_add(1, Ordering::SeqCst);
        *self.value.lock().unwrap() = value;
        Ok(())
    }

    fn save(&self) -> Result<(), PrimaryWorkspaceStoreError> {
        self.saves.fetch_add(1, Ordering::SeqCst);
        if self.fail_next_save.swap(false, Ordering::SeqCst) {
            return Err(PrimaryWorkspaceStoreError::unavailable());
        }
        *self.durable.lock().unwrap() = self.value.lock().unwrap().clone();
        self.order.lock().unwrap().push("save");
        Ok(())
    }
}

impl MemoryPrimaryWorkspaceStore {
    fn fail_next_save(&self) {
        self.fail_next_save.store(true, Ordering::SeqCst);
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

#[derive(Clone)]
struct MemoryHostRecordValue {
    kernel: Option<Value>,
    private_workspace: String,
}

struct MemoryAtomicHostRecord {
    value: Arc<Mutex<MemoryHostRecordValue>>,
    commits: Arc<std::sync::atomic::AtomicUsize>,
}

impl MemoryAtomicHostRecord {
    fn new(private_workspace: &str) -> Arc<Self> {
        Arc::new(Self {
            value: Arc::new(Mutex::new(MemoryHostRecordValue {
                kernel: None,
                private_workspace: private_workspace.to_string(),
            })),
            commits: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        })
    }

    fn snapshot(&self) -> MemoryHostRecordValue {
        self.value.lock().unwrap().clone()
    }

    fn replace_private_workspace(&self, value: &str) {
        self.value.lock().unwrap().private_workspace = value.to_string();
    }

    fn transaction(self: &Arc<Self>, next: &str) -> MemoryHostWorkspaceTransaction {
        MemoryHostWorkspaceTransaction {
            record: self.clone(),
            expected_record: self.snapshot(),
            next: next.to_string(),
            fail_persist: false,
            outcome_unknown_after_commit: false,
            replace_target_after_commit: None,
        }
    }
}

impl PrimaryWorkspaceStore for MemoryAtomicHostRecord {
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
    expected_record: MemoryHostRecordValue,
    next: String,
    fail_persist: bool,
    outcome_unknown_after_commit: bool,
    replace_target_after_commit: Option<(PathBuf, PathBuf)>,
}

impl AtomicHostWorkspaceTransaction for MemoryHostWorkspaceTransaction {
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

#[derive(Default)]
struct RecordingEventSink {
    publications: Mutex<Vec<EventPublication>>,
    fail: AtomicBool,
    order: Arc<Mutex<Vec<&'static str>>>,
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

    fn into_service(
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
        .unwrap();
        (self.runtime, service)
    }
}

#[tokio::test]
async fn current_workspace_identity_is_stable_and_matches_the_api_adapter() {
    let temporary = tempdir().unwrap();
    let store = Arc::new(MemoryPrimaryWorkspaceStore::default());
    let events = Arc::new(RecordingEventSink::default());
    let (_runtime, service) =
        DesktopFixture::new(temporary.path()).into_service(store, events.clone());

    let first = service.current().unwrap();
    let second = service.current().unwrap();
    let api = WorkspaceApiService::get_workspace(&service).await.unwrap();

    assert_eq!(first, second);
    assert_eq!(api, first);
    assert_eq!(first.display_name, "Initial Workspace");
    assert!(events.publications.lock().unwrap().is_empty());
    assert_workspace_identity_is_populated(&first);
}

#[test]
fn rebuilding_the_service_in_one_runtime_preserves_current_id_generation_and_revision() {
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
    .unwrap();
    let second = WorkspaceService::new(
        &runtime,
        store,
        second_managed,
        Arc::new(RecordingEventSink::default()),
        "Ignored Existing Display Name",
    )
    .unwrap();

    assert_eq!(first.current().unwrap(), second.current().unwrap());
}

#[tokio::test]
async fn stale_cas_leaves_identity_authority_and_events_unchanged() {
    let temporary = tempdir().unwrap();
    let next = temporary.path().join("next");
    fs::create_dir(&next).unwrap();
    let store = Arc::new(MemoryPrimaryWorkspaceStore::default());
    let events = Arc::new(RecordingEventSink::default());
    let (runtime, service) =
        DesktopFixture::new(temporary.path()).into_service(store, events.clone());
    let before = service.current().unwrap();
    let before_authority = runtime.active_workspace_authority();
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
        &runtime.active_workspace_authority()
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
    let (runtime, service) =
        DesktopFixture::new(temporary.path()).into_service(store.clone(), events.clone());
    let before = service.current().unwrap();
    let before_durable = store.durable();
    let before_authority = runtime.active_workspace_authority();
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
        &runtime.active_workspace_authority()
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
    let (runtime, service) =
        DesktopFixture::new(temporary.path()).into_service(store.clone(), events.clone());
    let before = service.current().unwrap();
    let before_host_record = store.snapshot();
    let before_authority = runtime.active_workspace_authority();
    let prepared = runtime.prepare_host_workspace_authority(&next).unwrap();
    let mut host_transaction = store.transaction("workspace-b");
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
        &runtime.active_workspace_authority()
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
    let (runtime, service) =
        DesktopFixture::new(temporary.path()).into_service(store.clone(), events.clone());
    let before = service.current().unwrap();
    let before_host_record = store.snapshot();
    let stale = runtime
        .prepare_host_workspace_authority(&stale_target)
        .unwrap();
    let winning = runtime
        .prepare_host_workspace_authority(&winning_target)
        .unwrap();
    runtime.commit_host_workspace_authority(winning).unwrap();

    let error = service
        .compare_and_set_host_workspace_transaction(
            &before.revision,
            stale,
            "Stale Target",
            Box::new(store.transaction("stale-target")),
        )
        .await
        .unwrap_err();

    assert_eq!(
        error.kind(),
        WorkspaceServiceErrorKind::PreparedAuthorityMismatch
    );
    assert_eq!(store.commits.load(Ordering::SeqCst), 0);
    let after_host_record = store.snapshot();
    assert_eq!(after_host_record.kernel, before_host_record.kernel);
    assert_eq!(
        after_host_record.private_workspace,
        before_host_record.private_workspace
    );
    assert_eq!(service.current().unwrap(), before);
    assert!(events.publications.lock().unwrap().is_empty());
}

#[tokio::test]
async fn stale_revision_is_rejected_before_host_persistence() {
    let temporary = tempdir().unwrap();
    let next = temporary.path().join("next");
    fs::create_dir(&next).unwrap();
    let store = MemoryAtomicHostRecord::new("workspace-a");
    let events = Arc::new(RecordingEventSink::default());
    let (runtime, service) =
        DesktopFixture::new(temporary.path()).into_service(store.clone(), events.clone());
    let before = service.current().unwrap();
    let before_host_record = store.snapshot();
    let before_authority = runtime.active_workspace_authority();
    let prepared = runtime.prepare_host_workspace_authority(&next).unwrap();

    let error = service
        .compare_and_set_host_workspace_transaction(
            &Revision::parse("stale-revision").unwrap(),
            prepared,
            "Next Workspace",
            Box::new(store.transaction("workspace-b")),
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
        &runtime.active_workspace_authority()
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
    let (runtime, service) =
        DesktopFixture::new(temporary.path()).into_service(store.clone(), events.clone());
    let before = service.current().unwrap();
    let before_authority = runtime.active_workspace_authority();
    let prepared = runtime.prepare_host_workspace_authority(&next).unwrap();
    let transaction = store.transaction("workspace-b");
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
        &runtime.active_workspace_authority()
    ));
    assert_eq!(service.current().unwrap(), before);
    assert!(events.publications.lock().unwrap().is_empty());
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
    let (runtime, service) = fixture.into_service(store.clone(), events.clone());
    let before = service.current().unwrap();
    let prepared = runtime.prepare_host_workspace_authority(&target).unwrap();

    let committed = service
        .compare_and_set_host_workspace_transaction(
            &before.revision,
            prepared,
            "Private Absolute Workspace",
            Box::new(store.transaction(target.to_str().unwrap())),
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
    let publications = events.publications.lock().unwrap();
    assert_eq!(publications.len(), 1);
    assert!(matches!(
        &publications[0].event,
        DomainEvent::WorkspaceChanged { workspace } if workspace == &committed
    ));
    drop(publications);

    let rebuilt = WorkspaceService::new(
        &runtime,
        store,
        rebuild_managed,
        Arc::new(RecordingEventSink::default()),
        "Ignored Rebuild Name",
    )
    .unwrap();
    assert_eq!(rebuilt.current().unwrap(), committed);

    let wire_json = serde_json::to_string(&committed).unwrap();
    assert!(!wire_json.contains(target.to_string_lossy().as_ref()));
    assert!(!wire_json.contains("desktopPath"));
}

#[tokio::test]
async fn concurrent_host_switches_are_serialized_and_only_one_revision_wins() {
    let temporary = tempdir().unwrap();
    let target_a = temporary.path().join("Target A");
    let target_b = temporary.path().join("Target B");
    fs::create_dir(&target_a).unwrap();
    fs::create_dir(&target_b).unwrap();
    let store = MemoryAtomicHostRecord::new("workspace-initial");
    let events = Arc::new(RecordingEventSink::default());
    let (runtime, service) =
        DesktopFixture::new(temporary.path()).into_service(store.clone(), events.clone());
    let service = Arc::new(service);
    let before = service.current().unwrap();
    let prepared_a = runtime.prepare_host_workspace_authority(&target_a).unwrap();
    let prepared_b = runtime.prepare_host_workspace_authority(&target_b).unwrap();
    let first = service.compare_and_set_host_workspace_transaction(
        &before.revision,
        prepared_a,
        "Target A",
        Box::new(store.transaction("host-a")),
    );
    let second = service.compare_and_set_host_workspace_transaction(
        &before.revision,
        prepared_b,
        "Target B",
        Box::new(store.transaction("host-b")),
    );

    let (first_result, second_result) = tokio::join!(first, second);

    let (winner, loser) = match (first_result, second_result) {
        (Ok(winner), Err(loser)) | (Err(loser), Ok(winner)) => (winner, loser),
        unexpected => panic!("exactly one host switch must win: {unexpected:?}"),
    };
    assert_eq!(loser.kind(), WorkspaceServiceErrorKind::RevisionConflict);
    assert_eq!(loser.current_revision(), Some(&winner.revision));
    assert_eq!(store.commits.load(Ordering::SeqCst), 1);
    assert_eq!(service.current().unwrap(), winner);
    assert_eq!(events.publications.lock().unwrap().len(), 1);
    let private_workspace = store.snapshot().private_workspace;
    match service.current().unwrap().display_name.as_str() {
        "Target A" => assert_eq!(private_workspace, "host-a"),
        "Target B" => assert_eq!(private_workspace, "host-b"),
        unexpected => panic!("unexpected winner: {unexpected}"),
    }
}

#[tokio::test]
async fn unknown_host_commit_outcome_quarantines_the_workspace_service() {
    let temporary = tempdir().unwrap();
    let target = temporary.path().join("target");
    fs::create_dir(&target).unwrap();
    let store = MemoryAtomicHostRecord::new("workspace-a");
    let events = Arc::new(RecordingEventSink::default());
    let (runtime, service) =
        DesktopFixture::new(temporary.path()).into_service(store.clone(), events.clone());
    let before = service.current().unwrap();
    let before_authority = runtime.active_workspace_authority();
    let prepared = runtime.prepare_host_workspace_authority(&target).unwrap();
    let mut transaction = store.transaction("workspace-b");
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
    assert!(Arc::ptr_eq(
        &before_authority,
        &runtime.active_workspace_authority()
    ));
    assert_eq!(
        service.current().unwrap_err().kind(),
        WorkspaceServiceErrorKind::WorkspaceUnavailable
    );
    assert!(events.publications.lock().unwrap().is_empty());
}

#[tokio::test]
async fn candidate_replacement_after_durable_commit_quarantines_without_publication() {
    let temporary = tempdir().unwrap();
    let target = temporary.path().join("target");
    let displaced = temporary.path().join("target-displaced");
    fs::create_dir(&target).unwrap();
    let store = MemoryAtomicHostRecord::new("workspace-a");
    let events = Arc::new(RecordingEventSink::default());
    let (runtime, service) =
        DesktopFixture::new(temporary.path()).into_service(store.clone(), events.clone());
    let before = service.current().unwrap();
    let before_authority = runtime.active_workspace_authority();
    let prepared = runtime.prepare_host_workspace_authority(&target).unwrap();
    let mut transaction = store.transaction("workspace-b");
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
    assert!(Arc::ptr_eq(
        &before_authority,
        &runtime.active_workspace_authority()
    ));
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
    let (runtime, service) =
        DesktopFixture::new(temporary.path()).into_service(store.clone(), events.clone());
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
    let (runtime, service) =
        DesktopFixture::new(temporary.path()).into_service(store.clone(), events.clone());
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
async fn managed_list_is_sorted_shallow_read_only_and_instance_scoped() {
    let temporary = tempdir().unwrap();
    let first_fixture = DesktopFixture::new(&temporary.path().join("first"));
    let first_app_data = first_fixture.app_data.clone();
    let (_first_runtime, first) = first_fixture.into_service(
        Arc::new(MemoryPrimaryWorkspaceStore::default()),
        Arc::new(RecordingEventSink::default()),
    );
    let second_fixture = DesktopFixture::new(&temporary.path().join("second"));
    let (_second_runtime, second) = second_fixture.into_service(
        Arc::new(MemoryPrimaryWorkspaceStore::default()),
        Arc::new(RecordingEventSink::default()),
    );

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
    let (_runtime, collection_service) = collection_fixture.into_service(
        Arc::new(MemoryPrimaryWorkspaceStore::default()),
        Arc::new(RecordingEventSink::default()),
    );

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
    let (_runtime, child_service) = child_fixture.into_service(
        Arc::new(MemoryPrimaryWorkspaceStore::default()),
        Arc::new(RecordingEventSink::default()),
    );

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

#[test]
fn retained_managed_capability_fails_closed_when_its_ambient_address_is_replaced() {
    let temporary = tempdir().unwrap();
    let fixture = DesktopFixture::new(temporary.path());
    let app_data = fixture.app_data.clone();
    let displaced = temporary.path().join("displaced-app-data");
    let (_runtime, service) = fixture.into_service(
        Arc::new(MemoryPrimaryWorkspaceStore::default()),
        Arc::new(RecordingEventSink::default()),
    );
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
    let (runtime, service) =
        DesktopFixture::new(temporary.path()).into_service(store.clone(), events.clone());
    let before = service.current().unwrap();
    let before_store = store.durable();
    let before_authority = runtime.active_workspace_authority();
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
        &runtime.active_workspace_authority()
    ));
    assert!(events.publications.lock().unwrap().is_empty());
}

#[tokio::test]
async fn api_maps_replaced_active_workspace_address_to_safe_unavailable() {
    let temporary = tempdir().unwrap();
    let fixture = DesktopFixture::new(temporary.path());
    let workspace = fixture.workspace.clone();
    let displaced = temporary.path().join("workspace-displaced");
    let (_runtime, service) = fixture.into_service(
        Arc::new(MemoryPrimaryWorkspaceStore::default()),
        Arc::new(RecordingEventSink::default()),
    );
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
#[test]
fn constructor_verifies_both_lock_addresses_before_any_store_access() {
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
        );
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
