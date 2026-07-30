use std::{
    fs,
    io::{Seek, SeekFrom, Write},
    path::PathBuf,
    sync::{
        atomic::{AtomicU8, Ordering},
        mpsc, Arc, Mutex,
    },
    time::Duration,
};

use qingyu_kernel::{
    config::KernelConfig,
    contract::{
        CreateDocumentRequest, DocumentContents, DocumentName, FileDocumentName,
        ListDocumentsQuery, MoveDocumentRequest, PageLimit, PageQuery,
        RestoreDocumentHistoryRequest, Revision, Rfc3339Utc, UpdateDocumentRequest,
        WorkspaceRelativePath,
    },
    documents::{
        history::{
            DocumentHistoryError, DocumentHistoryStore, DocumentRecoveryError,
            DocumentRecoveryIntent, DocumentRecoveryOutcome, DocumentRecoveryStore,
            FileDocumentRecoveryStore, MemoryDocumentHistoryStore, MemoryDocumentRecoveryStore,
        },
        service::{
            CapabilityAtomicInstallPort, DocumentServiceErrorKind, WorkspaceDocumentService,
        },
        types::HistorySnapshot,
        AtomicInstallMode, AtomicInstallPort, AtomicInstallPortError, AtomicInstallRequest,
        CapabilityMoveInstallPort, DeletionPort, DeletionPortError, DocumentDeletionTarget,
        MoveInstallPort, MoveInstallPortError, MoveInstallRequest, PinnedInstallSource,
    },
    ignore_rules::AllowAllWorkspaceIgnorePort,
    paths::KernelPaths,
    ports::KernelPorts,
    runtime::{KernelRuntime, KernelStartupErrorKind},
    services::workspace::WorkspaceService,
    workspace::{
        managed::ManagedWorkspaceCollection,
        primary::{
            PrimaryWorkspaceRepositoryBinding, PrimaryWorkspaceStore, PrimaryWorkspaceStoreError,
        },
    },
};
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use uuid::Uuid;

#[derive(Default)]
struct MemoryWorkspaceStore {
    binding: PrimaryWorkspaceRepositoryBinding,
    value: Mutex<Option<Value>>,
}

impl PrimaryWorkspaceStore for MemoryWorkspaceStore {
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
        Ok(())
    }
}

struct PermanentDeletion(PathBuf);

impl DeletionPort for PermanentDeletion {
    fn delete(
        &self,
        target: &DocumentDeletionTarget,
        _policy: qingyu_kernel::contract::DeletionPolicy,
    ) -> Result<(), DeletionPortError> {
        fs::remove_file(self.0.join(target.path.as_str())).map_err(|_| DeletionPortError)
    }
}

#[derive(Default)]
struct TamperingAtomicInstallPort {
    tamper_next: AtomicU8,
}

#[derive(Default)]
struct ReplacingTargetAtomicInstallPort {
    replace_next: AtomicU8,
}

impl ReplacingTargetAtomicInstallPort {
    fn replace_next(&self) {
        self.replace_next.store(1, Ordering::SeqCst);
    }
}

impl AtomicInstallPort for ReplacingTargetAtomicInstallPort {
    fn install(&self, request: AtomicInstallRequest<'_>) -> Result<(), AtomicInstallPortError> {
        if request.mode == AtomicInstallMode::ReplaceExisting
            && self.replace_next.swap(0, Ordering::SeqCst) == 1
        {
            request
                .directory
                .remove_file(request.target_name)
                .map_err(|_| AtomicInstallPortError)?;
            request
                .directory
                .write(request.target_name, b"external replacement")
                .map_err(|_| AtomicInstallPortError)?;
        }
        CapabilityAtomicInstallPort.install(request)
    }
}

#[derive(Default)]
struct ReplacingSourceMoveInstallPort {
    replace_next: AtomicU8,
}

struct ReplacingOnCompletionRecoveryStore {
    inner: MemoryDocumentRecoveryStore,
    root: PathBuf,
    replace_next: AtomicU8,
}

struct BlockingPendingRecoveryStore {
    inner: MemoryDocumentRecoveryStore,
    block_next: AtomicU8,
    started: Mutex<Option<mpsc::Sender<()>>>,
    release: Mutex<mpsc::Receiver<()>>,
}

#[derive(Default)]
struct CountingRecoveryStore {
    inner: MemoryDocumentRecoveryStore,
    pending_calls: AtomicU8,
}

impl CountingRecoveryStore {
    fn pending_calls(&self) -> u8 {
        self.pending_calls.load(Ordering::SeqCst)
    }
}

impl DocumentRecoveryStore for CountingRecoveryStore {
    fn prepare(&self, intent: &DocumentRecoveryIntent) -> Result<(), DocumentRecoveryError> {
        self.inner.prepare(intent)
    }

    fn pending(&self) -> Result<Vec<DocumentRecoveryIntent>, DocumentRecoveryError> {
        self.pending_calls.fetch_add(1, Ordering::SeqCst);
        self.inner.pending()
    }

    fn complete(&self, transaction_id: Uuid) -> Result<(), DocumentRecoveryError> {
        self.inner.complete(transaction_id)
    }

    fn clear(&self, transaction_id: Uuid) -> Result<(), DocumentRecoveryError> {
        self.inner.clear(transaction_id)
    }
}

impl BlockingPendingRecoveryStore {
    fn new(started: mpsc::Sender<()>, release: mpsc::Receiver<()>) -> Self {
        Self {
            inner: MemoryDocumentRecoveryStore::default(),
            block_next: AtomicU8::new(0),
            started: Mutex::new(Some(started)),
            release: Mutex::new(release),
        }
    }

    fn block_next_pending(&self) {
        self.block_next.store(1, Ordering::SeqCst);
    }
}

impl DocumentRecoveryStore for BlockingPendingRecoveryStore {
    fn prepare(&self, intent: &DocumentRecoveryIntent) -> Result<(), DocumentRecoveryError> {
        self.inner.prepare(intent)
    }

    fn pending(&self) -> Result<Vec<DocumentRecoveryIntent>, DocumentRecoveryError> {
        if self.block_next.swap(0, Ordering::SeqCst) == 1 {
            self.started
                .lock()
                .unwrap()
                .take()
                .unwrap()
                .send(())
                .unwrap();
            self.release.lock().unwrap().recv().unwrap();
        }
        self.inner.pending()
    }

    fn complete(&self, transaction_id: Uuid) -> Result<(), DocumentRecoveryError> {
        self.inner.complete(transaction_id)
    }

    fn clear(&self, transaction_id: Uuid) -> Result<(), DocumentRecoveryError> {
        self.inner.clear(transaction_id)
    }
}

impl ReplacingOnCompletionRecoveryStore {
    fn new(root: PathBuf) -> Self {
        Self {
            inner: MemoryDocumentRecoveryStore::default(),
            root,
            replace_next: AtomicU8::new(0),
        }
    }

    fn replace_next(&self) {
        self.replace_next.store(1, Ordering::SeqCst);
    }
}

impl DocumentRecoveryStore for ReplacingOnCompletionRecoveryStore {
    fn prepare(&self, intent: &DocumentRecoveryIntent) -> Result<(), DocumentRecoveryError> {
        self.inner.prepare(intent)
    }

    fn pending(&self) -> Result<Vec<DocumentRecoveryIntent>, DocumentRecoveryError> {
        self.inner.pending()
    }

    fn complete(&self, transaction_id: Uuid) -> Result<(), DocumentRecoveryError> {
        self.inner.complete(transaction_id)?;
        if self.replace_next.swap(0, Ordering::SeqCst) == 1 {
            fs::remove_file(self.root.join("note.md")).map_err(|_| DocumentRecoveryError)?;
            fs::write(self.root.join("note.md"), "external replacement")
                .map_err(|_| DocumentRecoveryError)?;
        }
        Ok(())
    }

    fn clear(&self, transaction_id: Uuid) -> Result<(), DocumentRecoveryError> {
        self.inner.clear(transaction_id)
    }
}

impl ReplacingSourceMoveInstallPort {
    fn replace_next(&self) {
        self.replace_next.store(1, Ordering::SeqCst);
    }
}

impl MoveInstallPort for ReplacingSourceMoveInstallPort {
    fn install(&self, request: MoveInstallRequest<'_>) -> Result<(), MoveInstallPortError> {
        if self.replace_next.swap(0, Ordering::SeqCst) == 1 {
            match request.kind {
                qingyu_kernel::contract::DocumentKind::File => request
                    .source_directory
                    .remove_file(request.source_name)
                    .map_err(|_| MoveInstallPortError::RecoveryRequired)?,
                qingyu_kernel::contract::DocumentKind::Directory => request
                    .source_directory
                    .remove_dir_all(request.source_name)
                    .map_err(|_| MoveInstallPortError::RecoveryRequired)?,
            }
            request
                .source_directory
                .write(request.source_name, b"external replacement")
                .map_err(|_| MoveInstallPortError::RecoveryRequired)?;
        }
        CapabilityMoveInstallPort.install(request)
    }
}

impl TamperingAtomicInstallPort {
    fn tamper_next(&self, mode: u8) {
        self.tamper_next.store(mode, Ordering::SeqCst);
    }
}

impl AtomicInstallPort for TamperingAtomicInstallPort {
    fn install(&self, request: AtomicInstallRequest<'_>) -> Result<(), AtomicInstallPortError> {
        match self.tamper_next.swap(0, Ordering::SeqCst) {
            1 => request
                .directory
                .write(request.stage_name, b"externally-tampered")
                .map_err(|_| AtomicInstallPortError)?,
            2 => {
                request
                    .directory
                    .remove_file(request.stage_name)
                    .map_err(|_| AtomicInstallPortError)?;
                request
                    .directory
                    .write(request.stage_name, b"replacement-stage")
                    .map_err(|_| AtomicInstallPortError)?;
            }
            3 => request
                .directory
                .hard_link(
                    request.stage_name,
                    request.directory,
                    format!(".external-hardlink-{}", Uuid::new_v4()),
                )
                .map_err(|_| AtomicInstallPortError)?,
            4 => match request.expected_stage {
                PinnedInstallSource::Directory(directory) => directory
                    .write("externally-added.md", b"tampered")
                    .map_err(|_| AtomicInstallPortError)?,
                PinnedInstallSource::File(_) => return Err(AtomicInstallPortError),
            },
            _ => {}
        }
        CapabilityAtomicInstallPort.install(request)
    }
}

#[derive(Default)]
struct FailingHistory;

impl DocumentHistoryStore for FailingHistory {
    fn preserve(
        &self,
        _path: &WorkspaceRelativePath,
        _contents: &[u8],
        _revision: &Revision,
        _created_at: &Rfc3339Utc,
    ) -> Result<qingyu_kernel::contract::SnapshotId, DocumentHistoryError> {
        Err(DocumentHistoryError)
    }

    fn list(
        &self,
        _path: &WorkspaceRelativePath,
    ) -> Result<Vec<HistorySnapshot>, DocumentHistoryError> {
        Err(DocumentHistoryError)
    }

    fn get(
        &self,
        _path: &WorkspaceRelativePath,
        _snapshot_id: qingyu_kernel::contract::SnapshotId,
    ) -> Result<Option<HistorySnapshot>, DocumentHistoryError> {
        Err(DocumentHistoryError)
    }

    fn relocate(
        &self,
        _source: &WorkspaceRelativePath,
        _target: &WorkspaceRelativePath,
        _kind: qingyu_kernel::contract::DocumentKind,
    ) -> Result<(), DocumentHistoryError> {
        Err(DocumentHistoryError)
    }
}

struct Fixture {
    runtime: Arc<KernelRuntime>,
    store: Arc<MemoryWorkspaceStore>,
    workspace: Arc<WorkspaceService>,
    root: PathBuf,
}

impl Fixture {
    async fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap().keep();
        let root = temporary.join("workspace");
        let app_data = temporary.join("app-data");
        let cache = temporary.join("cache");
        for path in [&root, &app_data, &cache] {
            fs::create_dir(path).unwrap();
        }
        let paths = KernelPaths::desktop(&root, &app_data, &cache).unwrap();
        let managed = ManagedWorkspaceCollection::from_paths(&paths).unwrap();
        let runtime = KernelRuntime::activate(
            KernelConfig::generate().unwrap(),
            paths,
            KernelPorts::unavailable(),
        )
        .unwrap();
        let store = Arc::new(MemoryWorkspaceStore::default());
        let workspace = Arc::new(
            WorkspaceService::new(
                &runtime,
                store.clone(),
                managed,
                runtime.event_broker().clone(),
                "Recovery",
            )
            .await
            .unwrap(),
        );
        Self {
            runtime,
            store,
            workspace,
            root,
        }
    }

    fn service(&self, history: Arc<dyn DocumentHistoryStore>) -> Arc<WorkspaceDocumentService> {
        Arc::new(WorkspaceDocumentService::new(
            &self.runtime,
            Arc::new(PermanentDeletion(self.root.clone())),
            history,
        ))
    }
}

async fn enter_global_workspace_recovery(fixture: &Fixture) {
    fixture
        .store
        .replace(Some(serde_json::json!({
            "schemaVersion": 1,
            "revisionSeed": "external-change",
            "displayName": "External"
        })))
        .unwrap();
    fixture.store.save().unwrap();
    let root = fixture.root.parent().unwrap();
    let paths =
        KernelPaths::desktop(&fixture.root, &root.join("app-data"), &root.join("cache")).unwrap();
    let managed = ManagedWorkspaceCollection::from_paths(&paths).unwrap();
    assert!(WorkspaceService::new(
        &fixture.runtime,
        fixture.store.clone(),
        managed,
        fixture.runtime.event_broker().clone(),
        "Ignored",
    )
    .await
    .is_err());
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn recovery_retains_one_snapshot_and_old_lease_until_completion() {
    let fixture = Fixture::new().await;
    let (started_sender, started_receiver) = mpsc::channel();
    let (release_sender, release_receiver) = mpsc::channel();
    let recovery = Arc::new(BlockingPendingRecoveryStore::new(
        started_sender,
        release_receiver,
    ));
    let service = Arc::new(
        WorkspaceDocumentService::new_with_recovery(
            &fixture.runtime,
            Arc::new(PermanentDeletion(fixture.root.clone())),
            Arc::new(MemoryDocumentHistoryStore::default()),
            recovery.clone(),
        )
        .unwrap(),
    );
    recovery.block_next_pending();
    let recovery_service = service.clone();
    let request = std::thread::spawn(move || recovery_service.recover());
    tokio::task::spawn_blocking(move || started_receiver.recv_timeout(Duration::from_secs(5)))
        .await
        .unwrap()
        .expect("recovery must retain its context before reading the journal");

    let next = fixture
        .root
        .parent()
        .unwrap()
        .join("next-recovery-workspace");
    fs::create_dir(&next).unwrap();
    let current = fixture.workspace.current().unwrap();
    let prepared = fixture
        .runtime
        .prepare_host_workspace_authority(&next)
        .unwrap();
    fixture
        .workspace
        .compare_and_set_host_workspace(&current.revision, prepared, "Next")
        .await
        .unwrap();

    assert_workspace_is_locked(
        &fixture.root,
        fixture.root.parent().unwrap(),
        "recovery-retained",
    );
    release_sender.send(()).unwrap();
    request.join().unwrap().unwrap();
    assert_workspace_is_acquirable(
        &fixture.root,
        fixture.root.parent().unwrap(),
        "recovery-released",
    );
}

#[tokio::test]
async fn recovery_does_not_touch_journal_after_global_quarantine() {
    let fixture = Fixture::new().await;
    let recovery = Arc::new(CountingRecoveryStore::default());
    let service = WorkspaceDocumentService::new_with_recovery(
        &fixture.runtime,
        Arc::new(PermanentDeletion(fixture.root.clone())),
        Arc::new(MemoryDocumentHistoryStore::default()),
        recovery.clone(),
    )
    .unwrap();
    assert_eq!(recovery.pending_calls(), 1);
    enter_global_workspace_recovery(&fixture).await;

    assert_eq!(
        service.recover().unwrap_err().kind(),
        DocumentServiceErrorKind::Unavailable
    );
    assert_eq!(recovery.pending_calls(), 1);
    let rebuilt = WorkspaceDocumentService::new_with_recovery(
        &fixture.runtime,
        Arc::new(PermanentDeletion(fixture.root.clone())),
        Arc::new(MemoryDocumentHistoryStore::default()),
        recovery.clone(),
    );
    assert!(matches!(
        rebuilt,
        Err(error) if error.kind() == DocumentServiceErrorKind::Unavailable
    ));
    assert_eq!(recovery.pending_calls(), 1);
}

async fn create(
    service: &WorkspaceDocumentService,
    workspace: &WorkspaceService,
) -> (qingyu_kernel::contract::DocumentId, Revision) {
    let created = service
        .create_document(CreateDocumentRequest::File {
            workspace_generation: workspace.current().unwrap().generation,
            parent: WorkspaceRelativePath::default(),
            name: FileDocumentName::parse("note.md").unwrap(),
            contents: DocumentContents::parse("first").unwrap(),
        })
        .await
        .unwrap();
    match created {
        qingyu_kernel::contract::CreatedDocumentDto::File { id, revision, .. } => (id, revision),
        _ => panic!("file expected"),
    }
}

fn content_revision(contents: &[u8]) -> Revision {
    Revision::parse(format!("{:x}", Sha256::digest(contents))).unwrap()
}

#[tokio::test]
async fn create_returns_contents_and_revision_from_one_installed_snapshot() {
    let fixture = Fixture::new().await;
    let recovery = Arc::new(ReplacingOnCompletionRecoveryStore::new(
        fixture.root.clone(),
    ));
    let service = WorkspaceDocumentService::new_with_recovery(
        &fixture.runtime,
        Arc::new(PermanentDeletion(fixture.root.clone())),
        Arc::new(MemoryDocumentHistoryStore::default()),
        recovery.clone(),
    )
    .unwrap();
    recovery.replace_next();

    let created = service
        .create_document(CreateDocumentRequest::File {
            workspace_generation: fixture.workspace.current().unwrap().generation,
            parent: WorkspaceRelativePath::default(),
            name: FileDocumentName::parse("note.md").unwrap(),
            contents: DocumentContents::parse("first").unwrap(),
        })
        .await
        .unwrap();

    let qingyu_kernel::contract::CreatedDocumentDto::File {
        contents, revision, ..
    } = created
    else {
        panic!("file expected");
    };
    assert_eq!(contents.as_str(), "external replacement");
    assert_eq!(revision, content_revision(contents.as_str().as_bytes()));
}

#[tokio::test]
async fn history_failure_prevents_publication_and_leaves_the_document_unchanged() {
    let fixture = Fixture::new().await;
    let service = fixture.service(Arc::new(FailingHistory));
    let (id, revision) = create(&service, &fixture.workspace).await;
    let mut events = fixture.runtime.event_broker().subscribe();

    let error = service
        .update_document(
            id,
            UpdateDocumentRequest {
                workspace_generation: fixture.workspace.current().unwrap().generation,
                expected_revision: revision,
                contents: DocumentContents::parse("second").unwrap(),
            },
        )
        .await
        .unwrap_err();

    assert_eq!(error.kind(), DocumentServiceErrorKind::HistoryUnavailable);
    assert_eq!(
        fs::read_to_string(fixture.root.join("note.md")).unwrap(),
        "first"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(20), events.recv())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn restore_preserves_the_current_revision_before_atomically_publishing_history() {
    let fixture = Fixture::new().await;
    let history = Arc::new(MemoryDocumentHistoryStore::default());
    let service = fixture.service(history);
    let (id, revision) = create(&service, &fixture.workspace).await;
    let updated = service
        .update_document(
            id.clone(),
            UpdateDocumentRequest {
                workspace_generation: fixture.workspace.current().unwrap().generation,
                expected_revision: revision,
                contents: DocumentContents::parse("second").unwrap(),
            },
        )
        .await
        .unwrap();
    let history = service
        .list_document_history(id.clone(), qingyu_kernel::contract::PageQuery::default())
        .await
        .unwrap();
    let restored = service
        .restore_document_history(
            id,
            history.items[0].snapshot_id,
            RestoreDocumentHistoryRequest {
                workspace_generation: fixture.workspace.current().unwrap().generation,
                expected_revision: updated.revision,
            },
        )
        .await
        .unwrap();

    assert_eq!(restored.contents.as_str(), "first");
    assert_eq!(
        fs::read_to_string(fixture.root.join("note.md")).unwrap(),
        "first"
    );
}

#[tokio::test]
async fn history_pagination_uses_the_same_created_at_and_snapshot_id_order_as_its_cursor() {
    let fixture = Fixture::new().await;
    let history = Arc::new(MemoryDocumentHistoryStore::default());
    let service = fixture.service(history.clone());
    let (id, _) = create(&service, &fixture.workspace).await;
    let path = WorkspaceRelativePath::parse("note.md").unwrap();
    let created_at = Rfc3339Utc::parse("2026-07-29T00:00:00Z").unwrap();
    for index in 0..5 {
        history
            .preserve(
                &path,
                format!("snapshot-{index}").as_bytes(),
                &Revision::parse(format!("revision-{index}")).unwrap(),
                &created_at,
            )
            .unwrap();
    }

    let mut cursor = None;
    let mut seen = Vec::new();
    loop {
        let page = service
            .list_document_history(
                id.clone(),
                PageQuery {
                    cursor,
                    limit: Some(PageLimit::new(2).unwrap()),
                },
            )
            .await
            .unwrap();
        seen.extend(page.items.iter().map(|item| item.snapshot_id));
        cursor = page.next_cursor.into_option();
        if cursor.is_none() {
            break;
        }
    }
    let unique = seen
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(seen.len(), 5);
    assert_eq!(unique.len(), 5);
}

#[tokio::test]
async fn document_mutations_share_the_runtime_sync_serialization_gate() {
    let fixture = Fixture::new().await;
    let service = fixture.service(Arc::new(MemoryDocumentHistoryStore::default()));
    let (id, revision) = create(&service, &fixture.workspace).await;
    let guard = fixture.runtime.mutation_coordinator().lock().await;
    let generation = fixture.workspace.current().unwrap().generation;
    let task = tokio::spawn(async move {
        service
            .update_document(
                id,
                UpdateDocumentRequest {
                    workspace_generation: generation,
                    expected_revision: revision,
                    contents: DocumentContents::parse("second").unwrap(),
                },
            )
            .await
    });

    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(!task.is_finished());
    drop(guard);
    assert!(task.await.unwrap().is_ok());
}

#[tokio::test]
async fn no_clobber_failures_leave_no_staged_document_artifacts() {
    let fixture = Fixture::new().await;
    let service = fixture.service(Arc::new(MemoryDocumentHistoryStore::default()));
    create(&service, &fixture.workspace).await;
    let error = service
        .create_document(CreateDocumentRequest::File {
            workspace_generation: fixture.workspace.current().unwrap().generation,
            parent: WorkspaceRelativePath::default(),
            name: FileDocumentName::parse("note.md").unwrap(),
            contents: DocumentContents::parse("replacement").unwrap(),
        })
        .await
        .unwrap_err();
    assert_eq!(error.kind(), DocumentServiceErrorKind::AlreadyExists);
    assert!(fs::read_dir(&fixture.root).unwrap().all(|entry| !entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".qingyu-kernel-update-")));
}

#[tokio::test]
async fn rename_that_published_before_completion_failure_is_finalized_on_restart_idempotently() {
    let fixture = Fixture::new().await;
    let recovery = Arc::new(MemoryDocumentRecoveryStore::default());
    let service = Arc::new(
        WorkspaceDocumentService::new_with_recovery(
            &fixture.runtime,
            Arc::new(PermanentDeletion(fixture.root.clone())),
            Arc::new(MemoryDocumentHistoryStore::default()),
            recovery.clone(),
        )
        .unwrap(),
    );
    let (id, revision) = create(&service, &fixture.workspace).await;
    recovery.fail_next_completion();

    let error = service
        .update_document(
            id.clone(),
            UpdateDocumentRequest {
                workspace_generation: fixture.workspace.current().unwrap().generation,
                expected_revision: revision,
                contents: DocumentContents::parse("published-before-error").unwrap(),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.kind(), DocumentServiceErrorKind::RecoveryRequired);
    assert_eq!(
        fs::read_to_string(fixture.root.join("note.md")).unwrap(),
        "published-before-error"
    );
    assert_eq!(recovery.intent_count(), 1);

    let restarted = WorkspaceDocumentService::new_with_recovery(
        &fixture.runtime,
        Arc::new(PermanentDeletion(fixture.root.clone())),
        Arc::new(MemoryDocumentHistoryStore::default()),
        recovery.clone(),
    )
    .unwrap();
    assert_eq!(recovery.intent_count(), 0);
    assert_eq!(
        restarted.get_document(id).await.unwrap().contents.as_str(),
        "published-before-error"
    );
    assert!(restarted.recover().unwrap().is_empty());
}

#[tokio::test]
async fn stage_tampering_during_atomic_install_keeps_recovery_intent_and_publishes_no_event() {
    for mode in [1, 2, 3] {
        let fixture = Fixture::new().await;
        let recovery = Arc::new(MemoryDocumentRecoveryStore::default());
        let atomic = Arc::new(TamperingAtomicInstallPort::default());
        let service = Arc::new(
            WorkspaceDocumentService::new_with_ports(
                &fixture.runtime,
                Arc::new(PermanentDeletion(fixture.root.clone())),
                Arc::new(MemoryDocumentHistoryStore::default()),
                recovery.clone(),
                atomic.clone(),
                Arc::new(AllowAllWorkspaceIgnorePort),
            )
            .unwrap(),
        );
        let (id, revision) = create(&service, &fixture.workspace).await;
        let mut events = fixture.runtime.event_broker().subscribe();
        atomic.tamper_next(mode);

        let error = service
            .update_document(
                id,
                UpdateDocumentRequest {
                    workspace_generation: fixture.workspace.current().unwrap().generation,
                    expected_revision: revision,
                    contents: DocumentContents::parse("intended").unwrap(),
                },
            )
            .await
            .unwrap_err();

        assert_eq!(error.kind(), DocumentServiceErrorKind::RecoveryRequired);
        assert_eq!(recovery.intent_count(), 1);
        assert!(
            tokio::time::timeout(Duration::from_millis(20), events.recv())
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn directory_content_change_during_install_fails_closed_after_publication() {
    let fixture = Fixture::new().await;
    let recovery = Arc::new(MemoryDocumentRecoveryStore::default());
    let atomic = Arc::new(TamperingAtomicInstallPort::default());
    let service = WorkspaceDocumentService::new_with_ports(
        &fixture.runtime,
        Arc::new(PermanentDeletion(fixture.root.clone())),
        Arc::new(MemoryDocumentHistoryStore::default()),
        recovery.clone(),
        atomic.clone(),
        Arc::new(AllowAllWorkspaceIgnorePort),
    )
    .unwrap();
    let mut events = fixture.runtime.event_broker().subscribe();
    atomic.tamper_next(4);

    let error = service
        .create_document(CreateDocumentRequest::Directory {
            workspace_generation: fixture.workspace.current().unwrap().generation,
            parent: WorkspaceRelativePath::default(),
            name: DocumentName::parse("folder").unwrap(),
        })
        .await
        .unwrap_err();

    assert_eq!(error.kind(), DocumentServiceErrorKind::RecoveryRequired);
    assert_eq!(
        fs::read(fixture.root.join("folder/externally-added.md")).unwrap(),
        b"tampered"
    );
    assert_eq!(recovery.intent_count(), 1);
    assert!(
        tokio::time::timeout(Duration::from_millis(20), events.recv())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn update_does_not_overwrite_a_target_replaced_during_the_final_atomic_install() {
    let fixture = Fixture::new().await;
    let recovery = Arc::new(MemoryDocumentRecoveryStore::default());
    let atomic = Arc::new(ReplacingTargetAtomicInstallPort::default());
    let service = Arc::new(
        WorkspaceDocumentService::new_with_ports(
            &fixture.runtime,
            Arc::new(PermanentDeletion(fixture.root.clone())),
            Arc::new(MemoryDocumentHistoryStore::default()),
            recovery.clone(),
            atomic.clone(),
            Arc::new(AllowAllWorkspaceIgnorePort),
        )
        .unwrap(),
    );
    let (id, revision) = create(&service, &fixture.workspace).await;
    atomic.replace_next();

    let error = service
        .update_document(
            id,
            UpdateDocumentRequest {
                workspace_generation: fixture.workspace.current().unwrap().generation,
                expected_revision: revision,
                contents: DocumentContents::parse("kernel update").unwrap(),
            },
        )
        .await
        .expect_err("an external replacement must win the conditional update race");

    assert_eq!(error.kind(), DocumentServiceErrorKind::RevisionConflict);
    assert_eq!(
        fs::read_to_string(fixture.root.join("note.md")).unwrap(),
        "external replacement"
    );
    assert_eq!(recovery.intent_count(), 0);
}

#[tokio::test]
async fn move_rolls_back_when_the_source_is_replaced_during_the_final_rename() {
    let fixture = Fixture::new().await;
    let recovery = Arc::new(MemoryDocumentRecoveryStore::default());
    let move_install = Arc::new(ReplacingSourceMoveInstallPort::default());
    let service = Arc::new(
        WorkspaceDocumentService::new_with_mutation_ports(
            &fixture.runtime,
            Arc::new(PermanentDeletion(fixture.root.clone())),
            Arc::new(MemoryDocumentHistoryStore::default()),
            recovery.clone(),
            Arc::new(CapabilityAtomicInstallPort),
            move_install.clone(),
            Arc::new(AllowAllWorkspaceIgnorePort),
        )
        .unwrap(),
    );
    let (id, revision) = create(&service, &fixture.workspace).await;
    move_install.replace_next();

    let error = service
        .move_document(
            id,
            qingyu_kernel::contract::MoveDocumentRequest {
                workspace_generation: fixture.workspace.current().unwrap().generation,
                expected_revision: revision,
                target_parent: WorkspaceRelativePath::default(),
                name: DocumentName::parse("moved.md").unwrap(),
            },
        )
        .await
        .expect_err("the moved source must still be the pinned expected document");

    assert_eq!(error.kind(), DocumentServiceErrorKind::RevisionConflict);
    assert_eq!(
        fs::read_to_string(fixture.root.join("note.md")).unwrap(),
        "external replacement"
    );
    assert!(!fixture.root.join("moved.md").exists());
    assert_eq!(recovery.intent_count(), 0);
}

#[tokio::test]
async fn directory_create_published_before_completion_failure_is_finalized_on_restart() {
    let fixture = Fixture::new().await;
    let recovery = Arc::new(MemoryDocumentRecoveryStore::default());
    let service = WorkspaceDocumentService::new_with_recovery(
        &fixture.runtime,
        Arc::new(PermanentDeletion(fixture.root.clone())),
        Arc::new(MemoryDocumentHistoryStore::default()),
        recovery.clone(),
    )
    .unwrap();
    recovery.fail_next_completion();

    let error = service
        .create_document(CreateDocumentRequest::Directory {
            workspace_generation: fixture.workspace.current().unwrap().generation,
            parent: WorkspaceRelativePath::default(),
            name: DocumentName::parse("folder").unwrap(),
        })
        .await
        .unwrap_err();
    assert_eq!(error.kind(), DocumentServiceErrorKind::RecoveryRequired);
    assert!(fixture.root.join("folder").is_dir());
    assert_eq!(recovery.intent_count(), 1);

    WorkspaceDocumentService::new_with_recovery(
        &fixture.runtime,
        Arc::new(PermanentDeletion(fixture.root.clone())),
        Arc::new(MemoryDocumentHistoryStore::default()),
        recovery.clone(),
    )
    .unwrap();
    assert_eq!(recovery.intent_count(), 0);
}

#[tokio::test]
async fn orphan_stage_is_rolled_back_and_repeated_recovery_is_a_noop() {
    let fixture = Fixture::new().await;
    let recovery = Arc::new(MemoryDocumentRecoveryStore::default());
    let stage_name = ".qingyu-kernel-update-00000000000000000000000000000000.tmp";
    fs::write(fixture.root.join(stage_name), "orphan").unwrap();
    recovery
        .prepare(&DocumentRecoveryIntent {
            transaction_id: Uuid::new_v4(),
            source: None,
            target: WorkspaceRelativePath::parse("missing.md").unwrap(),
            stage_name: Some(stage_name.to_string()),
            kind: qingyu_kernel::contract::DocumentKind::File,
            previous_revision: None,
            intended_revision: content_revision(b"orphan"),
        })
        .unwrap();

    let service = WorkspaceDocumentService::new_with_recovery(
        &fixture.runtime,
        Arc::new(PermanentDeletion(fixture.root.clone())),
        Arc::new(MemoryDocumentHistoryStore::default()),
        recovery.clone(),
    )
    .unwrap();
    assert!(!fixture.root.join(stage_name).exists());
    assert_eq!(recovery.intent_count(), 0);
    assert_eq!(
        service.recover().unwrap(),
        Vec::<DocumentRecoveryOutcome>::new()
    );
}

#[tokio::test]
async fn recovery_never_deletes_an_unknown_entry_at_a_valid_stage_name() {
    let fixture = Fixture::new().await;
    let recovery = Arc::new(MemoryDocumentRecoveryStore::default());
    let stage_name = ".qingyu-kernel-update-22222222222222222222222222222222.tmp";
    fs::write(fixture.root.join("note.md"), "final contents").unwrap();
    fs::write(fixture.root.join(stage_name), "unknown entry").unwrap();
    recovery
        .prepare(&DocumentRecoveryIntent {
            transaction_id: Uuid::new_v4(),
            source: None,
            target: WorkspaceRelativePath::parse("note.md").unwrap(),
            stage_name: Some(stage_name.to_string()),
            kind: qingyu_kernel::contract::DocumentKind::File,
            previous_revision: Some(content_revision(b"previous contents")),
            intended_revision: content_revision(b"final contents"),
        })
        .unwrap();

    let result = WorkspaceDocumentService::new_with_recovery(
        &fixture.runtime,
        Arc::new(PermanentDeletion(fixture.root.clone())),
        Arc::new(MemoryDocumentHistoryStore::default()),
        recovery.clone(),
    );

    assert!(matches!(
        result,
        Err(error) if error.kind() == DocumentServiceErrorKind::RecoveryRequired
    ));
    assert_eq!(
        fs::read_to_string(fixture.root.join(stage_name)).unwrap(),
        "unknown entry"
    );
    assert_eq!(recovery.intent_count(), 1);
}

#[tokio::test]
async fn recovery_rejects_an_unowned_stage_name_without_deleting_a_workspace_entry() {
    let fixture = Fixture::new().await;
    let recovery = Arc::new(MemoryDocumentRecoveryStore::default());
    fs::write(fixture.root.join("victim.md"), "must remain").unwrap();
    recovery
        .prepare(&DocumentRecoveryIntent {
            transaction_id: Uuid::new_v4(),
            source: None,
            target: WorkspaceRelativePath::parse("missing.md").unwrap(),
            stage_name: Some("victim.md".to_string()),
            kind: qingyu_kernel::contract::DocumentKind::File,
            previous_revision: None,
            intended_revision: Revision::parse("intended").unwrap(),
        })
        .unwrap();

    let result = WorkspaceDocumentService::new_with_recovery(
        &fixture.runtime,
        Arc::new(PermanentDeletion(fixture.root.clone())),
        Arc::new(MemoryDocumentHistoryStore::default()),
        recovery.clone(),
    );

    assert!(matches!(
        result,
        Err(error) if error.kind() == DocumentServiceErrorKind::RecoveryRequired
    ));
    assert_eq!(
        fs::read_to_string(fixture.root.join("victim.md")).unwrap(),
        "must remain"
    );
    assert_eq!(recovery.intent_count(), 1);
}

#[test]
fn file_recovery_store_persists_intents_across_store_reconstruction() {
    let directory = tempfile::tempdir().unwrap();
    let transaction_id = Uuid::new_v4();
    let intent = DocumentRecoveryIntent {
        transaction_id,
        source: None,
        target: WorkspaceRelativePath::parse("note.md").unwrap(),
        stage_name: Some(".qingyu-kernel-update-11111111111111111111111111111111.tmp".to_string()),
        kind: qingyu_kernel::contract::DocumentKind::File,
        previous_revision: Some(Revision::parse("before").unwrap()),
        intended_revision: Revision::parse("after").unwrap(),
    };
    let first = FileDocumentRecoveryStore::new(
        cap_std::fs::Dir::open_ambient_dir(directory.path(), cap_std::ambient_authority()).unwrap(),
    );
    first.prepare(&intent).unwrap();
    first.prepare(&intent).unwrap();
    drop(first);

    let second = FileDocumentRecoveryStore::new(
        cap_std::fs::Dir::open_ambient_dir(directory.path(), cap_std::ambient_authority()).unwrap(),
    );
    assert_eq!(second.pending().unwrap(), vec![intent]);
    second.clear(transaction_id).unwrap();
    second.clear(transaction_id).unwrap();
    drop(second);

    let third = FileDocumentRecoveryStore::new(
        cap_std::fs::Dir::open_ambient_dir(directory.path(), cap_std::ambient_authority()).unwrap(),
    );
    assert!(third.pending().unwrap().is_empty());
    assert!(fs::read_dir(directory.path()).unwrap().next().is_none());
}

#[test]
fn file_recovery_store_fails_closed_on_an_invalid_orphan_journal_stage() {
    let directory = tempfile::tempdir().unwrap();
    let stage_name = format!(".document-recovery-v1-{}.tmp", Uuid::new_v4());
    fs::write(directory.path().join(&stage_name), "invalid").unwrap();
    let store = FileDocumentRecoveryStore::new(
        cap_std::fs::Dir::open_ambient_dir(directory.path(), cap_std::ambient_authority()).unwrap(),
    );

    assert!(store.pending().is_err());
    assert_eq!(
        fs::read_to_string(directory.path().join(stage_name)).unwrap(),
        "invalid"
    );
}

#[test]
fn file_recovery_store_finalizes_a_valid_orphan_journal_stage() {
    let directory = tempfile::tempdir().unwrap();
    let transaction_id = Uuid::new_v4();
    let intent = DocumentRecoveryIntent {
        transaction_id,
        source: None,
        target: WorkspaceRelativePath::parse("note.md").unwrap(),
        stage_name: Some(".qingyu-kernel-update-33333333333333333333333333333333.tmp".to_string()),
        kind: qingyu_kernel::contract::DocumentKind::File,
        previous_revision: Some(Revision::parse("before").unwrap()),
        intended_revision: Revision::parse("after").unwrap(),
    };
    let stage_name = format!(".document-recovery-v1-{transaction_id}.tmp");
    fs::write(
        directory.path().join(&stage_name),
        serde_json::to_vec(&intent).unwrap(),
    )
    .unwrap();

    let store = FileDocumentRecoveryStore::new(
        cap_std::fs::Dir::open_ambient_dir(directory.path(), cap_std::ambient_authority()).unwrap(),
    );

    assert_eq!(store.pending().unwrap(), vec![intent]);
    assert!(!directory.path().join(stage_name).exists());
    assert!(directory
        .path()
        .join(format!("document-recovery-v1-{transaction_id}.json"))
        .exists());
}

#[test]
fn directory_revision_length_prefixes_entries_and_file_contents() {
    let fixture = tempfile::tempdir().unwrap();
    let one_file = fixture.path().join("one-file");
    let two_files = fixture.path().join("two-files");
    fs::create_dir(&one_file).unwrap();
    fs::create_dir(&two_files).unwrap();
    fs::write(one_file.join("a.md"), b"x\0f\0b.md\0y").unwrap();
    fs::write(two_files.join("a.md"), b"x").unwrap();
    fs::write(two_files.join("b.md"), b"y").unwrap();
    let one_file =
        cap_std::fs::Dir::open_ambient_dir(&one_file, cap_std::ambient_authority()).unwrap();
    let two_files =
        cap_std::fs::Dir::open_ambient_dir(&two_files, cap_std::ambient_authority()).unwrap();

    let one_revision =
        qingyu_kernel::documents::service::directory_revision_for_capability(&one_file).unwrap();
    let two_revision =
        qingyu_kernel::documents::service::directory_revision_for_capability(&two_files).unwrap();

    assert_ne!(one_revision, two_revision);
}

#[test]
fn directory_revision_includes_binary_relative_paths() {
    let fixture = tempfile::tempdir().unwrap();
    let first = fixture.path().join("first");
    let second = fixture.path().join("second");
    fs::create_dir_all(first.join("assets")).unwrap();
    fs::create_dir_all(second.join("assets")).unwrap();
    fs::write(first.join("assets/first.bin"), b"same bytes").unwrap();
    fs::write(second.join("assets/second.bin"), b"same bytes").unwrap();
    let first = cap_std::fs::Dir::open_ambient_dir(&first, cap_std::ambient_authority()).unwrap();
    let second = cap_std::fs::Dir::open_ambient_dir(&second, cap_std::ambient_authority()).unwrap();

    let first_revision =
        qingyu_kernel::documents::service::directory_revision_for_capability(&first).unwrap();
    let second_revision =
        qingyu_kernel::documents::service::directory_revision_for_capability(&second).unwrap();

    assert_ne!(first_revision, second_revision);
}

#[test]
fn directory_revision_includes_exact_binary_contents() {
    let fixture = tempfile::tempdir().unwrap();
    fs::write(fixture.path().join("asset.bin"), [0_u8, 1, 2, 3]).unwrap();
    let directory =
        cap_std::fs::Dir::open_ambient_dir(fixture.path(), cap_std::ambient_authority()).unwrap();
    let before =
        qingyu_kernel::documents::service::directory_revision_for_capability(&directory).unwrap();
    fs::write(fixture.path().join("asset.bin"), [3_u8, 2, 1, 0]).unwrap();

    let after =
        qingyu_kernel::documents::service::directory_revision_for_capability(&directory).unwrap();

    assert_ne!(before, after);
}

#[test]
fn directory_revision_streams_binary_contents_beyond_the_document_limit() {
    let fixture = tempfile::tempdir().unwrap();
    let path = fixture.path().join("large.bin");
    let mut file = fs::File::create(&path).unwrap();
    file.set_len(16 * 1024 * 1024 + 1).unwrap();
    file.seek(SeekFrom::End(-1)).unwrap();
    file.write_all(&[1]).unwrap();
    file.sync_all().unwrap();
    let directory =
        cap_std::fs::Dir::open_ambient_dir(fixture.path(), cap_std::ambient_authority()).unwrap();
    let before =
        qingyu_kernel::documents::service::directory_revision_for_capability(&directory).unwrap();
    file.seek(SeekFrom::End(-1)).unwrap();
    file.write_all(&[2]).unwrap();
    file.sync_all().unwrap();

    let after =
        qingyu_kernel::documents::service::directory_revision_for_capability(&directory).unwrap();

    assert_ne!(before, after);
}

#[cfg(unix)]
#[test]
fn directory_revision_rejects_symbolic_links() {
    use std::os::unix::fs::symlink;

    let fixture = tempfile::tempdir().unwrap();
    fs::write(fixture.path().join("target.bin"), b"target").unwrap();
    symlink("target.bin", fixture.path().join("linked.bin")).unwrap();
    let directory =
        cap_std::fs::Dir::open_ambient_dir(fixture.path(), cap_std::ambient_authority()).unwrap();

    let error = qingyu_kernel::documents::service::directory_revision_for_capability(&directory)
        .unwrap_err();

    assert_eq!(error.kind(), DocumentServiceErrorKind::UnsafeTarget);
}

#[cfg(any(unix, windows))]
#[test]
fn directory_revision_rejects_hard_links() {
    let fixture = tempfile::tempdir().unwrap();
    let directory_path = fixture.path().join("workspace");
    fs::create_dir(&directory_path).unwrap();
    fs::write(fixture.path().join("outside.bin"), b"outside").unwrap();
    fs::hard_link(
        fixture.path().join("outside.bin"),
        directory_path.join("linked.bin"),
    )
    .unwrap();
    let directory =
        cap_std::fs::Dir::open_ambient_dir(&directory_path, cap_std::ambient_authority()).unwrap();

    let error = qingyu_kernel::documents::service::directory_revision_for_capability(&directory)
        .unwrap_err();

    assert_eq!(error.kind(), DocumentServiceErrorKind::UnsafeTarget);
}

#[cfg(unix)]
#[test]
fn directory_revision_rejects_non_regular_entries() {
    let fixture = tempfile::tempdir().unwrap();
    let _listener = std::os::unix::net::UnixListener::bind(fixture.path().join("socket")).unwrap();
    let directory =
        cap_std::fs::Dir::open_ambient_dir(fixture.path(), cap_std::ambient_authority()).unwrap();

    let error = qingyu_kernel::documents::service::directory_revision_for_capability(&directory)
        .unwrap_err();

    assert_eq!(error.kind(), DocumentServiceErrorKind::UnsafeTarget);
}

#[cfg(target_os = "linux")]
#[test]
fn directory_revision_rejects_non_unicode_names() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};

    let fixture = tempfile::tempdir().unwrap();
    fs::write(
        fixture.path().join(OsString::from_vec(vec![b'f', 0xff])),
        b"unsafe name",
    )
    .unwrap();
    let directory =
        cap_std::fs::Dir::open_ambient_dir(fixture.path(), cap_std::ambient_authority()).unwrap();

    let error = qingyu_kernel::documents::service::directory_revision_for_capability(&directory)
        .unwrap_err();

    assert_eq!(error.kind(), DocumentServiceErrorKind::UnsafeTarget);
}

#[tokio::test]
async fn directory_move_published_before_completion_is_recovered_with_path_stable_revision() {
    let fixture = Fixture::new().await;
    let recovery = Arc::new(MemoryDocumentRecoveryStore::default());
    let service = Arc::new(
        WorkspaceDocumentService::new_with_recovery(
            &fixture.runtime,
            Arc::new(PermanentDeletion(fixture.root.clone())),
            Arc::new(MemoryDocumentHistoryStore::default()),
            recovery.clone(),
        )
        .unwrap(),
    );
    let created = service
        .create_document(CreateDocumentRequest::Directory {
            workspace_generation: fixture.workspace.current().unwrap().generation,
            parent: WorkspaceRelativePath::default(),
            name: DocumentName::parse("folder").unwrap(),
        })
        .await
        .unwrap();
    assert!(matches!(
        created,
        qingyu_kernel::contract::CreatedDocumentDto::Directory { .. }
    ));
    fs::write(fixture.root.join("folder/nested.md"), "nested").unwrap();
    let listed = service
        .list_documents(ListDocumentsQuery {
            cursor: None,
            limit: None,
            parent: WorkspaceRelativePath::default(),
        })
        .await
        .unwrap();
    let folder = listed
        .items
        .into_iter()
        .find(|entry| entry.path.as_str() == "folder")
        .unwrap();
    recovery.fail_next_completion();
    let error = service
        .move_document(
            folder.id,
            MoveDocumentRequest {
                workspace_generation: fixture.workspace.current().unwrap().generation,
                expected_revision: folder.revision,
                target_parent: WorkspaceRelativePath::default(),
                name: DocumentName::parse("renamed").unwrap(),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.kind(), DocumentServiceErrorKind::RecoveryRequired);
    assert!(fixture.root.join("renamed").is_dir());

    WorkspaceDocumentService::new_with_recovery(
        &fixture.runtime,
        Arc::new(PermanentDeletion(fixture.root.clone())),
        Arc::new(MemoryDocumentHistoryStore::default()),
        recovery.clone(),
    )
    .unwrap();
    assert_eq!(recovery.intent_count(), 0);
}
