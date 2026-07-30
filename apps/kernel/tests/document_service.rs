use std::{
    collections::HashSet,
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Condvar, Mutex, Weak,
    },
    time::Duration,
};

use axum::{
    body::{to_bytes, Body},
    http::{header, Request, StatusCode},
};

use qingyu_kernel::{
    api::{build_router, TransportPolicy},
    config::KernelConfig,
    contract::{
        CreateDocumentRequest, DeleteDocumentRequest, DeletionPolicy, DocumentContents,
        DocumentEntryDto, DocumentKind, DocumentName, DomainEvent, FileDocumentName,
        ListDocumentsQuery, MoveDocumentRequest, Nullable, PageLimit, PageQuery, Revision,
        SearchQuery, SearchWorkspaceQuery, UpdateDocumentRequest, WireIdentityKey, WorkspaceDto,
        WorkspaceGeneration, WorkspaceId, WorkspaceReadiness, WorkspaceRelativePath,
    },
    documents::{
        history::MemoryDocumentHistoryStore,
        history::MemoryDocumentRecoveryStore,
        identity::{DocumentIdentityCodec, DocumentIdentityErrorKind},
        service::{
            CapabilityAtomicInstallPort, DocumentServiceErrorKind, WorkspaceDocumentService,
        },
        DeletionPort, DeletionPortError, DocumentDeletionTarget, DocumentIgnorePort,
    },
    events::{EventPublication, EventSink, EventSinkError},
    paths::KernelPaths,
    ports::KernelPorts,
    runtime::{DocumentsApiService, KernelRuntime, KernelStartupErrorKind},
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
use tempfile::tempdir;
use tower::ServiceExt as _;
use uuid::Uuid;

fn workspace(generation: &str) -> WorkspaceDto {
    WorkspaceDto {
        id: WorkspaceId::new(Uuid::from_u128(1)),
        generation: WorkspaceGeneration::parse(generation).unwrap(),
        display_name: "Documents".to_string(),
        readiness: WorkspaceReadiness::Ready,
        revision: Revision::parse("workspace-revision").unwrap(),
    }
}

#[test]
fn document_identity_is_scoped_to_workspace_generation_and_kind() {
    let key = WireIdentityKey::generate().unwrap();
    let codec = DocumentIdentityCodec::new(&key);
    let current = workspace("generation-a");
    let stale = workspace("generation-b");
    let path = WorkspaceRelativePath::parse("folder/note.md").unwrap();
    let document_id = codec
        .issue(&current, DocumentKind::File, &path)
        .expect("issue document identity");

    assert_eq!(
        codec
            .verify(&document_id, &current, DocumentKind::File)
            .unwrap(),
        path
    );
    assert_eq!(
        codec
            .verify(&document_id, &stale, DocumentKind::File)
            .unwrap_err()
            .kind(),
        DocumentIdentityErrorKind::InvalidOrStaleIdentity
    );
    assert_eq!(
        codec
            .verify(&document_id, &current, DocumentKind::Directory)
            .unwrap_err()
            .kind(),
        DocumentIdentityErrorKind::WrongKind
    );
}

#[test]
fn document_identity_does_not_expose_absolute_paths() {
    let key = WireIdentityKey::generate().unwrap();
    let codec = DocumentIdentityCodec::new(&key);
    let current = workspace("generation-a");
    let path = WorkspaceRelativePath::parse("note.md").unwrap();
    let document_id = codec
        .issue(&current, DocumentKind::File, &path)
        .expect("issue document identity");

    assert_eq!(
        codec
            .verify(&document_id, &current, DocumentKind::File)
            .unwrap(),
        path
    );
    assert!(!document_id.as_str().contains("/Volumes/"));
    assert!(!document_id.as_str().contains("note.md"));
}

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

struct RecordingDeletionPort {
    root: PathBuf,
    calls: Mutex<Vec<(String, qingyu_kernel::contract::DeletionPolicy)>>,
}

struct PathIgnorePort(HashSet<String>);

impl DocumentIgnorePort for PathIgnorePort {
    fn is_ignored(&self, path: &WorkspaceRelativePath, _kind: DocumentKind) -> bool {
        self.0.contains(path.as_str())
    }
}

struct BlockingIgnorePort {
    started: Mutex<Option<mpsc::Sender<()>>>,
    release: Mutex<mpsc::Receiver<()>>,
}

#[derive(Default)]
struct WorkspacePublicationGate {
    publication_entered: tokio::sync::Notify,
    released: (Mutex<bool>, Condvar),
    runtime: Mutex<Option<Weak<KernelRuntime>>>,
}

impl WorkspacePublicationGate {
    fn bind_runtime(&self, runtime: &Arc<KernelRuntime>) {
        *self.runtime.lock().unwrap() = Some(Arc::downgrade(runtime));
    }

    async fn wait_for_workspace_publication(&self) {
        self.publication_entered.notified().await;
    }

    fn release_workspace_publication(&self) {
        let (released, condition) = &self.released;
        *released.lock().unwrap() = true;
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
        let runtime = self
            .runtime
            .lock()
            .unwrap()
            .as_ref()
            .and_then(Weak::upgrade)
            .ok_or(EventSinkError)?;
        runtime.event_broker().publish(publication)
    }
}

impl DocumentIgnorePort for BlockingIgnorePort {
    fn is_ignored(&self, _path: &WorkspaceRelativePath, _kind: DocumentKind) -> bool {
        if let Some(started) = self.started.lock().unwrap().take() {
            started.send(()).unwrap();
            self.release.lock().unwrap().recv().unwrap();
        }
        false
    }
}

impl DeletionPort for RecordingDeletionPort {
    fn delete(
        &self,
        target: &DocumentDeletionTarget,
        policy: qingyu_kernel::contract::DeletionPolicy,
    ) -> Result<(), DeletionPortError> {
        self.calls
            .lock()
            .unwrap()
            .push((target.path.as_str().to_string(), policy));
        fs::remove_file(self.root.join(target.path.as_str())).map_err(|_| DeletionPortError)
    }
}

struct NoopDeletionPort;

impl DeletionPort for NoopDeletionPort {
    fn delete(
        &self,
        _target: &DocumentDeletionTarget,
        _policy: qingyu_kernel::contract::DeletionPolicy,
    ) -> Result<(), DeletionPortError> {
        Ok(())
    }
}

struct Fixture {
    runtime: Arc<KernelRuntime>,
    store: Arc<MemoryWorkspaceStore>,
    workspace: Arc<WorkspaceService>,
    service: Arc<WorkspaceDocumentService>,
    root: PathBuf,
    deletion: Arc<RecordingDeletionPort>,
}

impl Fixture {
    async fn new() -> Self {
        Self::new_with_workspace_events(None).await
    }

    async fn new_with_workspace_events(events: Option<Arc<dyn EventSink>>) -> Self {
        let temporary = tempdir().unwrap().keep();
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
        let events = events.unwrap_or_else(|| runtime.event_broker().clone());
        let workspace = Arc::new(
            WorkspaceService::new(&runtime, store.clone(), managed, events, "Documents")
                .await
                .unwrap(),
        );
        let deletion = Arc::new(RecordingDeletionPort {
            root: root.clone(),
            calls: Mutex::new(Vec::new()),
        });
        let service = Arc::new(WorkspaceDocumentService::new(
            &runtime,
            deletion.clone(),
            Arc::new(MemoryDocumentHistoryStore::default()),
        ));
        Self {
            runtime,
            store,
            workspace,
            service,
            root,
            deletion,
        }
    }
}

async fn create_listed_directory_with_binary_resource(
    fixture: &Fixture,
    name: &str,
) -> DocumentEntryDto {
    fixture
        .service
        .create_document(CreateDocumentRequest::Directory {
            workspace_generation: fixture.workspace.current().unwrap().generation,
            parent: WorkspaceRelativePath::default(),
            name: DocumentName::parse(name).unwrap(),
        })
        .await
        .unwrap();
    fs::write(fixture.root.join(name).join("resource.bin"), b"before").unwrap();
    fixture
        .service
        .list_documents(ListDocumentsQuery {
            cursor: None,
            limit: None,
            parent: WorkspaceRelativePath::default(),
        })
        .await
        .unwrap()
        .items
        .into_iter()
        .find(|entry| entry.path.as_str() == name)
        .unwrap()
}

#[tokio::test]
async fn directory_move_rejects_a_stale_revision_after_binary_resource_change() {
    let fixture = Fixture::new().await;
    let directory = create_listed_directory_with_binary_resource(&fixture, "folder").await;
    fs::write(fixture.root.join("folder/resource.bin"), b"after!").unwrap();

    let error = fixture
        .service
        .move_document(
            directory.id,
            MoveDocumentRequest {
                workspace_generation: fixture.workspace.current().unwrap().generation,
                expected_revision: directory.revision,
                target_parent: WorkspaceRelativePath::default(),
                name: DocumentName::parse("moved").unwrap(),
            },
        )
        .await
        .unwrap_err();

    assert_eq!(error.kind(), DocumentServiceErrorKind::RevisionConflict);
    assert!(fixture.root.join("folder/resource.bin").is_file());
    assert!(!fixture.root.join("moved").exists());
}

#[tokio::test]
async fn directory_delete_rejects_a_stale_revision_after_binary_resource_change() {
    let fixture = Fixture::new().await;
    let directory = create_listed_directory_with_binary_resource(&fixture, "folder").await;
    fs::write(fixture.root.join("folder/resource.bin"), b"after!").unwrap();

    let error = fixture
        .service
        .delete_document(
            directory.id,
            DeleteDocumentRequest {
                workspace_generation: fixture.workspace.current().unwrap().generation,
                expected_revision: directory.revision,
                deletion_policy: DeletionPolicy::Recoverable,
            },
        )
        .await
        .unwrap_err();

    assert_eq!(error.kind(), DocumentServiceErrorKind::RevisionConflict);
    assert!(fixture.root.join("folder/resource.bin").is_file());
    assert!(fixture.deletion.calls.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn document_mutation_is_rejected_until_workspace_changed_is_published() {
    let publication_gate = Arc::new(WorkspacePublicationGate::default());
    let fixture = Fixture::new_with_workspace_events(Some(publication_gate.clone())).await;
    publication_gate.bind_runtime(&fixture.runtime);
    let mut events = fixture.runtime.event_broker().subscribe();
    let current = fixture.workspace.current().unwrap();
    let next = fixture.root.parent().unwrap().join("next-workspace-events");
    fs::create_dir(&next).unwrap();
    let prepared = fixture
        .runtime
        .prepare_host_workspace_authority(&next)
        .unwrap();
    let workspace = fixture.workspace.clone();
    let switch = tokio::spawn(async move {
        workspace
            .compare_and_set_host_workspace(&current.revision, prepared, "Next")
            .await
    });
    publication_gate.wait_for_workspace_publication().await;

    let committed = fixture.workspace.current().unwrap();
    let blocked = fixture
        .service
        .create_document(CreateDocumentRequest::File {
            workspace_generation: committed.generation.clone(),
            parent: WorkspaceRelativePath::default(),
            name: FileDocumentName::parse("ordered.md").unwrap(),
            contents: DocumentContents::parse("blocked").unwrap(),
        })
        .await;
    let wrote_before_publication = next.join("ordered.md").exists();

    publication_gate.release_workspace_publication();
    let switched = switch.await.unwrap().unwrap();
    assert_eq!(switched, committed);
    assert_eq!(
        blocked.unwrap_err().kind(),
        DocumentServiceErrorKind::Unavailable
    );
    assert!(!wrote_before_publication);

    fixture
        .service
        .create_document(CreateDocumentRequest::File {
            workspace_generation: committed.generation,
            parent: WorkspaceRelativePath::default(),
            name: FileDocumentName::parse("ordered.md").unwrap(),
            contents: DocumentContents::parse("published").unwrap(),
        })
        .await
        .unwrap();
    assert_eq!(
        fs::read_to_string(next.join("ordered.md")).unwrap(),
        "published"
    );

    let workspace_event = tokio::time::timeout(Duration::from_secs(5), events.recv())
        .await
        .unwrap()
        .unwrap();
    let document_event = tokio::time::timeout(Duration::from_secs(5), events.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        workspace_event.event,
        DomainEvent::WorkspaceChanged { .. }
    ));
    assert!(matches!(
        document_event.event,
        DomainEvent::DocumentCreated { .. }
    ));
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
async fn document_request_retains_old_snapshot_lease_until_request_finishes() {
    let fixture = Fixture::new().await;
    fs::write(fixture.root.join("old.md"), "old").unwrap();
    let (started_sender, started_receiver) = mpsc::channel();
    let (release_sender, release_receiver) = mpsc::channel();
    let service = Arc::new(
        WorkspaceDocumentService::new_with_ports(
            &fixture.runtime,
            fixture.deletion.clone(),
            Arc::new(MemoryDocumentHistoryStore::default()),
            Arc::new(MemoryDocumentRecoveryStore::default()),
            Arc::new(CapabilityAtomicInstallPort),
            Arc::new(BlockingIgnorePort {
                started: Mutex::new(Some(started_sender)),
                release: Mutex::new(release_receiver),
            }),
        )
        .unwrap(),
    );
    let request = tokio::spawn(async move {
        service
            .list_documents(ListDocumentsQuery {
                cursor: None,
                limit: None,
                parent: WorkspaceRelativePath::default(),
            })
            .await
    });
    tokio::task::spawn_blocking(move || started_receiver.recv_timeout(Duration::from_secs(5)))
        .await
        .unwrap()
        .expect("document request must retain its context before workspace switch");

    let next = fixture.root.parent().unwrap().join("next-workspace-lease");
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
        "document-request-retained",
    );
    release_sender.send(()).unwrap();
    request.await.unwrap().unwrap();
    assert_workspace_is_acquirable(
        &fixture.root,
        fixture.root.parent().unwrap(),
        "document-request-released",
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn document_request_never_crosses_authority_and_generation() {
    let fixture = Fixture::new().await;
    fs::write(fixture.root.join("old.md"), "old").unwrap();
    let admitted = fixture.workspace.current().unwrap();
    let (started_sender, started_receiver) = mpsc::channel();
    let (release_sender, release_receiver) = mpsc::channel();
    let service = Arc::new(
        WorkspaceDocumentService::new_with_ports(
            &fixture.runtime,
            fixture.deletion.clone(),
            Arc::new(MemoryDocumentHistoryStore::default()),
            Arc::new(MemoryDocumentRecoveryStore::default()),
            Arc::new(CapabilityAtomicInstallPort),
            Arc::new(BlockingIgnorePort {
                started: Mutex::new(Some(started_sender)),
                release: Mutex::new(release_receiver),
            }),
        )
        .unwrap(),
    );
    let first_service = service.clone();
    let first = tokio::spawn(async move {
        first_service
            .list_documents(ListDocumentsQuery {
                cursor: None,
                limit: None,
                parent: WorkspaceRelativePath::default(),
            })
            .await
    });
    tokio::task::spawn_blocking(move || started_receiver.recv_timeout(Duration::from_secs(5)))
        .await
        .unwrap()
        .expect("first request must retain its context before workspace switch");

    let next = fixture
        .root
        .parent()
        .unwrap()
        .join("next-workspace-consistency");
    fs::create_dir(&next).unwrap();
    fs::write(next.join("new.md"), "new").unwrap();
    let prepared = fixture
        .runtime
        .prepare_host_workspace_authority(&next)
        .unwrap();
    let installed = fixture
        .workspace
        .compare_and_set_host_workspace(&admitted.revision, prepared, "Next")
        .await
        .unwrap();
    release_sender.send(()).unwrap();

    let first_page = first.await.unwrap().unwrap();
    assert_eq!(first_page.items.len(), 1);
    assert_eq!(first_page.items[0].path.as_str(), "old.md");
    DocumentIdentityCodec::new(fixture.runtime.wire_identity_key())
        .verify(&first_page.items[0].id, &admitted, DocumentKind::File)
        .unwrap();

    let second_page = service
        .list_documents(ListDocumentsQuery {
            cursor: None,
            limit: None,
            parent: WorkspaceRelativePath::default(),
        })
        .await
        .unwrap();
    assert_eq!(second_page.items.len(), 1);
    assert_eq!(second_page.items[0].path.as_str(), "new.md");
    DocumentIdentityCodec::new(fixture.runtime.wire_identity_key())
        .verify(&second_page.items[0].id, &installed, DocumentKind::File)
        .unwrap();
}

#[tokio::test]
async fn documents_remain_unavailable_after_quarantine_even_through_service_rebuild() {
    let fixture = Fixture::new().await;
    enter_global_workspace_recovery(&fixture).await;
    let query = || ListDocumentsQuery {
        cursor: None,
        limit: None,
        parent: WorkspaceRelativePath::default(),
    };

    assert_eq!(
        fixture
            .service
            .list_documents(query())
            .await
            .unwrap_err()
            .kind(),
        DocumentServiceErrorKind::Unavailable
    );
    let rebuilt = WorkspaceDocumentService::new(
        &fixture.runtime,
        fixture.deletion.clone(),
        Arc::new(MemoryDocumentHistoryStore::default()),
    );
    assert_eq!(
        rebuilt.list_documents(query()).await.unwrap_err().kind(),
        DocumentServiceErrorKind::Unavailable
    );
}

#[tokio::test]
async fn service_creates_without_clobbering_and_lists_signed_relative_entries() {
    let fixture = Fixture::new().await;
    let generation = fixture.workspace.current().unwrap().generation;
    let created = fixture
        .service
        .create_document(CreateDocumentRequest::File {
            workspace_generation: generation.clone(),
            parent: WorkspaceRelativePath::default(),
            name: FileDocumentName::parse("note.md").unwrap(),
            contents: DocumentContents::parse("first").unwrap(),
        })
        .await
        .unwrap();
    assert_eq!(
        fs::read_to_string(fixture.root.join("note.md")).unwrap(),
        "first"
    );

    let error = fixture
        .service
        .create_document(CreateDocumentRequest::File {
            workspace_generation: generation,
            parent: WorkspaceRelativePath::default(),
            name: FileDocumentName::parse("note.md").unwrap(),
            contents: DocumentContents::parse("second").unwrap(),
        })
        .await
        .unwrap_err();
    assert_eq!(
        error.code(),
        qingyu_kernel::contract::ErrorCode::DocumentAlreadyExists
    );
    assert_eq!(
        fs::read_to_string(fixture.root.join("note.md")).unwrap(),
        "first"
    );

    let page = fixture
        .service
        .list_documents(ListDocumentsQuery {
            cursor: None,
            limit: None,
            parent: WorkspaceRelativePath::default(),
        })
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].path.as_str(), "note.md");
    assert!(!page.items[0].id.as_str().contains("note.md"));
    assert!(matches!(
        created,
        qingyu_kernel::contract::CreatedDocumentDto::File { .. }
    ));
    assert_eq!(page.next_cursor, Nullable::null());
}

#[tokio::test]
async fn service_rejects_stale_generation_traversal_and_symlink_or_hardlink_replacement() {
    let fixture = Fixture::new().await;
    let stale = WorkspaceGeneration::parse("stale").unwrap();
    let error = fixture
        .service
        .create_document(CreateDocumentRequest::Directory {
            workspace_generation: stale,
            parent: WorkspaceRelativePath::default(),
            name: DocumentName::parse("folder").unwrap(),
        })
        .await
        .unwrap_err();
    assert_eq!(error.kind(), DocumentServiceErrorKind::RevisionConflict);

    assert!(WorkspaceRelativePath::parse("../outside.md").is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        fs::write(fixture.root.join("outside.md"), "outside").unwrap();
        symlink(
            fixture.root.join("outside.md"),
            fixture.root.join("linked.md"),
        )
        .unwrap();
        fs::hard_link(
            fixture.root.join("outside.md"),
            fixture.root.join("hard.md"),
        )
        .unwrap();
        for path in ["linked.md", "hard.md"] {
            let id = DocumentIdentityCodec::new(fixture.runtime.wire_identity_key())
                .issue(
                    &fixture.workspace.current().unwrap(),
                    DocumentKind::File,
                    &WorkspaceRelativePath::parse(path).unwrap(),
                )
                .unwrap();
            assert_eq!(
                fixture.service.get_document(id).await.unwrap_err().kind(),
                DocumentServiceErrorKind::UnsafeTarget
            );
        }
    }
}

#[tokio::test]
async fn update_history_move_and_delete_preserve_conflict_and_identity_semantics() {
    let fixture = Fixture::new().await;
    let generation = fixture.workspace.current().unwrap().generation;
    let created = fixture
        .service
        .create_document(CreateDocumentRequest::File {
            workspace_generation: generation.clone(),
            parent: WorkspaceRelativePath::default(),
            name: FileDocumentName::parse("note.md").unwrap(),
            contents: DocumentContents::parse("first").unwrap(),
        })
        .await
        .unwrap();
    let (id, first_revision) = match created {
        qingyu_kernel::contract::CreatedDocumentDto::File { id, revision, .. } => (id, revision),
        _ => panic!("file expected"),
    };

    let conflict = fixture
        .service
        .update_document(
            id.clone(),
            UpdateDocumentRequest {
                workspace_generation: generation.clone(),
                expected_revision: Revision::parse("stale").unwrap(),
                contents: DocumentContents::parse("wrong").unwrap(),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(conflict.kind(), DocumentServiceErrorKind::RevisionConflict);
    assert_eq!(conflict.current_revision(), Some(&first_revision));

    let updated = fixture
        .service
        .update_document(
            id.clone(),
            UpdateDocumentRequest {
                workspace_generation: generation.clone(),
                expected_revision: first_revision,
                contents: DocumentContents::parse("second").unwrap(),
            },
        )
        .await
        .unwrap();
    let history = fixture
        .service
        .list_document_history(id.clone(), qingyu_kernel::contract::PageQuery::default())
        .await
        .unwrap();
    assert_eq!(history.items.len(), 1);

    let moved = fixture
        .service
        .move_document(
            id.clone(),
            MoveDocumentRequest {
                workspace_generation: generation.clone(),
                expected_revision: updated.revision.clone(),
                target_parent: WorkspaceRelativePath::default(),
                name: DocumentName::parse("renamed.md").unwrap(),
            },
        )
        .await
        .unwrap();
    assert_ne!(moved.id, id);
    assert_eq!(moved.path.as_str(), "renamed.md");
    assert!(!fixture.root.join("note.md").exists());

    fixture
        .service
        .delete_document(
            moved.id.clone(),
            DeleteDocumentRequest {
                workspace_generation: generation,
                expected_revision: moved.revision,
                deletion_policy: DeletionPolicy::Recoverable,
            },
        )
        .await
        .unwrap();
    assert!(!fixture.root.join("renamed.md").exists());
    assert_eq!(
        fixture.deletion.calls.lock().unwrap().as_slice(),
        &[("renamed.md".to_string(), DeletionPolicy::Recoverable)]
    );
}

#[tokio::test]
async fn moving_a_document_preserves_history_under_the_new_document_identity() {
    let fixture = Fixture::new().await;
    let generation = fixture.workspace.current().unwrap().generation;
    let created = fixture
        .service
        .create_document(CreateDocumentRequest::File {
            workspace_generation: generation.clone(),
            parent: WorkspaceRelativePath::default(),
            name: FileDocumentName::parse("history-source.md").unwrap(),
            contents: DocumentContents::parse("first").unwrap(),
        })
        .await
        .unwrap();
    let (id, revision) = match created {
        qingyu_kernel::contract::CreatedDocumentDto::File { id, revision, .. } => (id, revision),
        _ => panic!("file expected"),
    };
    let updated = fixture
        .service
        .update_document(
            id.clone(),
            UpdateDocumentRequest {
                workspace_generation: generation.clone(),
                expected_revision: revision,
                contents: DocumentContents::parse("second").unwrap(),
            },
        )
        .await
        .unwrap();
    let moved = fixture
        .service
        .move_document(
            id,
            MoveDocumentRequest {
                workspace_generation: generation.clone(),
                expected_revision: updated.revision.clone(),
                target_parent: WorkspaceRelativePath::default(),
                name: DocumentName::parse("history-target.md").unwrap(),
            },
        )
        .await
        .unwrap();

    let history = fixture
        .service
        .list_document_history(moved.id.clone(), PageQuery::default())
        .await
        .unwrap();
    assert_eq!(history.items.len(), 1);
    let preview = fixture
        .service
        .get_document_history(moved.id.clone(), history.items[0].snapshot_id)
        .await
        .unwrap();
    assert_eq!(preview.snapshot_id, history.items[0].snapshot_id);
    assert_eq!(preview.document_id, moved.id);
    assert_eq!(preview.created_at, history.items[0].created_at);
    assert_eq!(preview.size_bytes, history.items[0].size_bytes);
    assert_eq!(preview.revision, history.items[0].revision);
    assert_eq!(preview.contents.as_str(), "first");
    let restored = fixture
        .service
        .restore_document_history(
            moved.id,
            history.items[0].snapshot_id,
            qingyu_kernel::contract::RestoreDocumentHistoryRequest {
                workspace_generation: generation,
                expected_revision: moved.revision,
            },
        )
        .await
        .unwrap();
    assert_eq!(restored.contents.as_str(), "first");
}

#[tokio::test]
async fn delete_does_not_publish_success_until_the_original_capability_target_is_absent() {
    let fixture = Fixture::new().await;
    let service = WorkspaceDocumentService::new(
        &fixture.runtime,
        Arc::new(NoopDeletionPort),
        Arc::new(MemoryDocumentHistoryStore::default()),
    );
    let created = service
        .create_document(CreateDocumentRequest::File {
            workspace_generation: fixture.workspace.current().unwrap().generation,
            parent: WorkspaceRelativePath::default(),
            name: FileDocumentName::parse("undeleted.md").unwrap(),
            contents: DocumentContents::parse("contents").unwrap(),
        })
        .await
        .unwrap();
    let (id, revision) = match created {
        qingyu_kernel::contract::CreatedDocumentDto::File { id, revision, .. } => (id, revision),
        _ => panic!("file expected"),
    };
    let mut events = fixture.runtime.event_broker().subscribe();
    let error = service
        .delete_document(
            id,
            DeleteDocumentRequest {
                workspace_generation: fixture.workspace.current().unwrap().generation,
                expected_revision: revision,
                deletion_policy: DeletionPolicy::Permanent,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.kind(), DocumentServiceErrorKind::RecoveryRequired);
    assert!(fixture.root.join("undeleted.md").is_file());
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(20), events.recv())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn content_limits_invalid_utf8_and_generation_bound_cursors_are_enforced() {
    let fixture = Fixture::new().await;
    let generation = fixture.workspace.current().unwrap().generation;
    let maximum = "x".repeat(16 * 1024 * 1024);
    let created = fixture
        .service
        .create_document(CreateDocumentRequest::File {
            workspace_generation: generation.clone(),
            parent: WorkspaceRelativePath::default(),
            name: FileDocumentName::parse("maximum.md").unwrap(),
            contents: DocumentContents::parse(maximum).unwrap(),
        })
        .await
        .unwrap();
    let maximum_id = match created {
        qingyu_kernel::contract::CreatedDocumentDto::File { id, .. } => id,
        _ => panic!("file expected"),
    };
    assert_eq!(
        fixture
            .service
            .get_document(maximum_id)
            .await
            .unwrap()
            .size_bytes
            .get(),
        16 * 1024 * 1024
    );

    fs::write(fixture.root.join("invalid.md"), [0xff, 0xfe]).unwrap();
    let invalid_id = DocumentIdentityCodec::new(fixture.runtime.wire_identity_key())
        .issue(
            &fixture.workspace.current().unwrap(),
            DocumentKind::File,
            &WorkspaceRelativePath::parse("invalid.md").unwrap(),
        )
        .unwrap();
    assert_eq!(
        fixture
            .service
            .get_document(invalid_id)
            .await
            .unwrap_err()
            .kind(),
        DocumentServiceErrorKind::InvalidEncoding
    );

    fixture
        .service
        .create_document(CreateDocumentRequest::File {
            workspace_generation: generation,
            parent: WorkspaceRelativePath::default(),
            name: FileDocumentName::parse("second.md").unwrap(),
            contents: DocumentContents::parse("second").unwrap(),
        })
        .await
        .unwrap();
    let first_page = fixture
        .service
        .list_documents(ListDocumentsQuery {
            cursor: None,
            limit: Some(PageLimit::new(1).unwrap()),
            parent: WorkspaceRelativePath::default(),
        })
        .await
        .unwrap();
    let cursor = first_page.next_cursor.into_option().expect("next cursor");
    let next = fixture.root.parent().unwrap().join("next-workspace");
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
    let error = fixture
        .service
        .list_documents(ListDocumentsQuery {
            cursor: Some(cursor),
            limit: Some(PageLimit::new(1).unwrap()),
            parent: WorkspaceRelativePath::default(),
        })
        .await
        .unwrap_err();
    assert_eq!(error.kind(), DocumentServiceErrorKind::InvalidCursor);
}

#[tokio::test]
async fn document_page_cursor_is_rejected_when_the_collection_changes_between_pages() {
    let fixture = Fixture::new().await;
    let generation = fixture.workspace.current().unwrap().generation;
    for name in ["first.md", "second.md"] {
        fixture
            .service
            .create_document(CreateDocumentRequest::File {
                workspace_generation: generation.clone(),
                parent: WorkspaceRelativePath::default(),
                name: FileDocumentName::parse(name).unwrap(),
                contents: DocumentContents::parse(name).unwrap(),
            })
            .await
            .unwrap();
    }
    let first_page = fixture
        .service
        .list_documents(ListDocumentsQuery {
            cursor: None,
            limit: Some(PageLimit::new(1).unwrap()),
            parent: WorkspaceRelativePath::default(),
        })
        .await
        .unwrap();
    let cursor = first_page.next_cursor.into_option().expect("next cursor");

    fixture
        .service
        .create_document(CreateDocumentRequest::File {
            workspace_generation: generation,
            parent: WorkspaceRelativePath::default(),
            name: FileDocumentName::parse("third.md").unwrap(),
            contents: DocumentContents::parse("third").unwrap(),
        })
        .await
        .unwrap();

    let error = fixture
        .service
        .list_documents(ListDocumentsQuery {
            cursor: Some(cursor),
            limit: Some(PageLimit::new(1).unwrap()),
            parent: WorkspaceRelativePath::default(),
        })
        .await
        .unwrap_err();
    assert_eq!(error.kind(), DocumentServiceErrorKind::InvalidCursor);
}

#[tokio::test]
async fn history_page_cursor_is_rejected_when_a_snapshot_is_added_between_pages() {
    let fixture = Fixture::new().await;
    let generation = fixture.workspace.current().unwrap().generation;
    let created = fixture
        .service
        .create_document(CreateDocumentRequest::File {
            workspace_generation: generation.clone(),
            parent: WorkspaceRelativePath::default(),
            name: FileDocumentName::parse("history.md").unwrap(),
            contents: DocumentContents::parse("first").unwrap(),
        })
        .await
        .unwrap();
    let (document_id, first_revision) = match created {
        qingyu_kernel::contract::CreatedDocumentDto::File { id, revision, .. } => (id, revision),
        _ => panic!("file expected"),
    };
    let second = fixture
        .service
        .update_document(
            document_id.clone(),
            UpdateDocumentRequest {
                workspace_generation: generation.clone(),
                expected_revision: first_revision,
                contents: DocumentContents::parse("second").unwrap(),
            },
        )
        .await
        .unwrap();
    let third = fixture
        .service
        .update_document(
            document_id.clone(),
            UpdateDocumentRequest {
                workspace_generation: generation.clone(),
                expected_revision: second.revision,
                contents: DocumentContents::parse("third").unwrap(),
            },
        )
        .await
        .unwrap();
    let first_page = fixture
        .service
        .list_document_history(
            document_id.clone(),
            PageQuery {
                cursor: None,
                limit: Some(PageLimit::new(1).unwrap()),
            },
        )
        .await
        .unwrap();
    let cursor = first_page.next_cursor.into_option().expect("next cursor");

    fixture
        .service
        .update_document(
            document_id.clone(),
            UpdateDocumentRequest {
                workspace_generation: generation,
                expected_revision: third.revision,
                contents: DocumentContents::parse("fourth").unwrap(),
            },
        )
        .await
        .unwrap();

    let error = fixture
        .service
        .list_document_history(
            document_id,
            PageQuery {
                cursor: Some(cursor),
                limit: Some(PageLimit::new(1).unwrap()),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.kind(), DocumentServiceErrorKind::InvalidCursor);
}

#[tokio::test]
async fn search_page_cursor_is_rejected_when_matches_change_between_pages() {
    let fixture = Fixture::new().await;
    let generation = fixture.workspace.current().unwrap().generation;
    for name in ["first.md", "second.md"] {
        fixture
            .service
            .create_document(CreateDocumentRequest::File {
                workspace_generation: generation.clone(),
                parent: WorkspaceRelativePath::default(),
                name: FileDocumentName::parse(name).unwrap(),
                contents: DocumentContents::parse("needle").unwrap(),
            })
            .await
            .unwrap();
    }
    let first_page = fixture
        .service
        .search_workspace(SearchWorkspaceQuery {
            cursor: None,
            limit: Some(PageLimit::new(1).unwrap()),
            query: SearchQuery::parse("needle").unwrap(),
        })
        .await
        .unwrap();
    let cursor = first_page.next_cursor.into_option().expect("next cursor");

    fixture
        .service
        .create_document(CreateDocumentRequest::File {
            workspace_generation: generation,
            parent: WorkspaceRelativePath::default(),
            name: FileDocumentName::parse("third.md").unwrap(),
            contents: DocumentContents::parse("needle").unwrap(),
        })
        .await
        .unwrap();

    let error = fixture
        .service
        .search_workspace(SearchWorkspaceQuery {
            cursor: Some(cursor),
            limit: Some(PageLimit::new(1).unwrap()),
            query: SearchQuery::parse("needle").unwrap(),
        })
        .await
        .unwrap_err();
    assert_eq!(error.kind(), DocumentServiceErrorKind::InvalidCursor);
}

#[tokio::test]
async fn read_dto_contents_size_and_revision_always_come_from_one_stable_retained_snapshot() {
    let fixture = Fixture::new().await;
    let created = fixture
        .service
        .create_document(CreateDocumentRequest::File {
            workspace_generation: fixture.workspace.current().unwrap().generation,
            parent: WorkspaceRelativePath::default(),
            name: FileDocumentName::parse("racing.md").unwrap(),
            contents: DocumentContents::parse("short").unwrap(),
        })
        .await
        .unwrap();
    let id = match created {
        qingyu_kernel::contract::CreatedDocumentDto::File { id, .. } => id,
        _ => panic!("file expected"),
    };
    let running = Arc::new(AtomicBool::new(true));
    let writer_running = running.clone();
    let path = fixture.root.join("racing.md");
    let writer = std::thread::spawn(move || {
        while writer_running.load(Ordering::Relaxed) {
            let _ = fs::write(&path, "short");
            let _ = fs::write(&path, "a much longer externally written document");
        }
    });

    for _ in 0..200 {
        match fixture.service.get_document(id.clone()).await {
            Ok(snapshot) => {
                assert_eq!(
                    snapshot.size_bytes.get(),
                    snapshot.contents.as_str().len() as u64
                );
                assert_eq!(
                    snapshot.revision.as_str(),
                    format!(
                        "{:x}",
                        Sha256::digest(snapshot.contents.as_str().as_bytes())
                    )
                );
            }
            Err(error) => assert_eq!(error.kind(), DocumentServiceErrorKind::Unavailable),
        }
    }
    running.store(false, Ordering::Relaxed);
    writer.join().unwrap();
}

#[tokio::test]
async fn search_is_utf8_precise_bounded_cursor_bound_and_skips_unsafe_or_ignored_inputs() {
    let fixture = Fixture::new().await;
    let ignored = HashSet::from([
        "workspace-hidden.md".to_string(),
        "global-hidden.md".to_string(),
        "ignored-dir".to_string(),
    ]);
    let service = Arc::new(
        WorkspaceDocumentService::new_with_ports(
            &fixture.runtime,
            fixture.deletion.clone(),
            Arc::new(MemoryDocumentHistoryStore::default()),
            Arc::new(MemoryDocumentRecoveryStore::default()),
            Arc::new(CapabilityAtomicInstallPort),
            Arc::new(PathIgnorePort(ignored)),
        )
        .unwrap(),
    );
    fs::write(
        fixture.root.join("visible.md"),
        "zero\n你好 needle 世界\nemoji 😀 needle tail",
    )
    .unwrap();
    fs::write(fixture.root.join("second.md"), "needle on another file").unwrap();
    fs::write(fixture.root.join("workspace-hidden.md"), "needle").unwrap();
    fs::write(fixture.root.join("global-hidden.md"), "needle").unwrap();
    fs::write(fixture.root.join("plain.txt"), "needle").unwrap();
    fs::write(fixture.root.join("invalid.md"), [0xff, 0xfe]).unwrap();
    let oversized = fs::File::create(fixture.root.join("oversized.md")).unwrap();
    oversized.set_len(16 * 1024 * 1024 + 1).unwrap();
    for protected in [".QINGYU", ".MARKRA-SYNC", ".QINGYU-STAGE"] {
        fs::create_dir_all(fixture.root.join(protected)).unwrap();
        fs::write(fixture.root.join(protected).join("hidden.md"), "needle").unwrap();
    }
    fs::create_dir(fixture.root.join("ignored-dir")).unwrap();
    fs::write(fixture.root.join("ignored-dir").join("hidden.md"), "needle").unwrap();
    let long_line = format!(
        "{} marker {} marker",
        "😀".repeat(10_000),
        "x".repeat(10_000)
    );
    fs::write(fixture.root.join("long.md"), long_line).unwrap();
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt as _};
        fs::write(
            fixture
                .root
                .join(OsString::from_vec(vec![0xff, b'.', b'm', b'd'])),
            "needle",
        )
        .unwrap();
    }

    let listed = service
        .list_documents(ListDocumentsQuery {
            cursor: None,
            limit: None,
            parent: WorkspaceRelativePath::default(),
        })
        .await
        .unwrap();
    let listed_paths = listed
        .items
        .iter()
        .map(|item| item.path.as_str())
        .collect::<Vec<_>>();
    assert!(!listed_paths.iter().any(|path| {
        matches!(
            *path,
            "workspace-hidden.md" | "global-hidden.md" | "ignored-dir"
        )
    }));

    let first = service
        .search_workspace(SearchWorkspaceQuery {
            cursor: None,
            limit: Some(PageLimit::new(1).unwrap()),
            query: SearchQuery::parse("needle").unwrap(),
        })
        .await
        .unwrap();
    assert_eq!(first.items.len(), 1);
    let cursor = first.next_cursor.into_option().unwrap();
    let second = service
        .search_workspace(SearchWorkspaceQuery {
            cursor: Some(cursor.clone()),
            limit: Some(PageLimit::new(10).unwrap()),
            query: SearchQuery::parse("needle").unwrap(),
        })
        .await
        .unwrap();
    let all = first
        .items
        .into_iter()
        .chain(second.items)
        .collect::<Vec<_>>();
    assert_eq!(all.len(), 3);
    assert!(all
        .iter()
        .all(|item| { matches!(item.document.path.as_str(), "second.md" | "visible.md") }));
    let chinese = all
        .iter()
        .find(|item| item.preview.contains("你好"))
        .unwrap();
    assert_eq!(chinese.line.get(), 2);
    assert_eq!(chinese.column.get(), 4);
    assert_eq!(chinese.preview, "你好 needle 世界");
    let emoji = all.iter().find(|item| item.preview.contains('😀')).unwrap();
    assert_eq!(emoji.line.get(), 3);
    assert_eq!(emoji.column.get(), 9);

    let mismatch = service
        .search_workspace(SearchWorkspaceQuery {
            cursor: Some(cursor),
            limit: Some(PageLimit::new(1).unwrap()),
            query: SearchQuery::parse("other").unwrap(),
        })
        .await
        .unwrap_err();
    assert_eq!(mismatch.kind(), DocumentServiceErrorKind::InvalidCursor);

    let bounded = service
        .search_workspace(SearchWorkspaceQuery {
            cursor: None,
            limit: None,
            query: SearchQuery::parse("marker").unwrap(),
        })
        .await
        .unwrap();
    assert_eq!(bounded.items.len(), 2);
    assert!(bounded
        .items
        .iter()
        .all(|item| item.preview.chars().count() <= 168 && item.preview.contains("marker")));
}

#[tokio::test]
async fn direct_and_http_adapters_return_the_same_dto_and_safe_error_code() {
    let fixture = Fixture::new().await;
    let generation = fixture.workspace.current().unwrap().generation;
    let created = fixture
        .service
        .create_document(CreateDocumentRequest::File {
            workspace_generation: generation.clone(),
            parent: WorkspaceRelativePath::default(),
            name: FileDocumentName::parse("note.md").unwrap(),
            contents: DocumentContents::parse("needle note needle").unwrap(),
        })
        .await
        .unwrap();
    let (document_id, initial_revision) = match created {
        qingyu_kernel::contract::CreatedDocumentDto::File { id, revision, .. } => (id, revision),
        _ => panic!("file expected"),
    };
    let query = ListDocumentsQuery {
        cursor: None,
        limit: None,
        parent: WorkspaceRelativePath::default(),
    };
    let direct = DocumentsApiService::list_documents(fixture.service.as_ref(), query)
        .await
        .unwrap();
    fixture
        .runtime
        .install_documents_api_service(fixture.service.clone())
        .unwrap();
    let credential = fixture
        .runtime
        .expose_native_launch_credential()
        .to_string();
    let router = build_router(
        fixture.runtime.clone(),
        TransportPolicy::loopback("127.0.0.1:43123", "tauri://localhost").unwrap(),
    );
    let request = Request::builder()
        .method("GET")
        .uri("/api/v1/documents")
        .header(header::HOST, "127.0.0.1:43123")
        .header(header::AUTHORIZATION, format!("Bearer {credential}"))
        .body(Body::empty())
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    let http: qingyu_kernel::contract::DocumentPageDto = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(http, direct);

    let search_query = SearchWorkspaceQuery {
        cursor: None,
        limit: Some(PageLimit::new(1).unwrap()),
        query: SearchQuery::parse("needle").unwrap(),
    };
    let direct_search =
        DocumentsApiService::search_workspace(fixture.service.as_ref(), search_query.clone())
            .await
            .unwrap();
    let request = Request::builder()
        .method("GET")
        .uri("/api/v1/search?query=needle&limit=1")
        .header(header::HOST, "127.0.0.1:43123")
        .header(header::AUTHORIZATION, format!("Bearer {credential}"))
        .body(Body::empty())
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    let http_search: qingyu_kernel::contract::SearchPageDto =
        serde_json::from_slice(&bytes).unwrap();
    assert_eq!(http_search, direct_search);

    let search_cursor = direct_search.next_cursor.into_option().unwrap();
    let direct_search_error = DocumentsApiService::search_workspace(
        fixture.service.as_ref(),
        SearchWorkspaceQuery {
            cursor: Some(search_cursor.clone()),
            limit: Some(PageLimit::new(1).unwrap()),
            query: SearchQuery::parse("other").unwrap(),
        },
    )
    .await
    .unwrap_err();
    let request = Request::builder()
        .method("GET")
        .uri(format!(
            "/api/v1/search?query=other&limit=1&cursor={}",
            search_cursor.as_str()
        ))
        .header(header::HOST, "127.0.0.1:43123")
        .header(header::AUTHORIZATION, format!("Bearer {credential}"))
        .body(Body::empty())
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let search_envelope: qingyu_kernel::contract::ApiErrorEnvelope =
        serde_json::from_slice(&bytes).unwrap();
    assert_eq!(search_envelope.code(), direct_search_error.code());
    assert_eq!(search_envelope.details(), direct_search_error.details());

    let direct_read =
        DocumentsApiService::get_document(fixture.service.as_ref(), document_id.clone())
            .await
            .unwrap();
    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/v1/documents/{}", document_id.as_str()))
        .header(header::HOST, "127.0.0.1:43123")
        .header(header::AUTHORIZATION, format!("Bearer {credential}"))
        .body(Body::empty())
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    let http_read: qingyu_kernel::contract::DocumentContentDto =
        serde_json::from_slice(&bytes).unwrap();
    assert_eq!(http_read, direct_read);

    let update_request = UpdateDocumentRequest {
        workspace_generation: generation,
        expected_revision: initial_revision.clone(),
        contents: DocumentContents::parse("updated through HTTP").unwrap(),
    };
    let mut events = fixture.runtime.event_broker().subscribe();
    let request = Request::builder()
        .method("PUT")
        .uri(format!("/api/v1/documents/{}", document_id.as_str()))
        .header(header::HOST, "127.0.0.1:43123")
        .header(header::AUTHORIZATION, format!("Bearer {credential}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&update_request).unwrap()))
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    let http_updated: qingyu_kernel::contract::DocumentContentDto =
        serde_json::from_slice(&bytes).unwrap();
    let direct_after_update =
        DocumentsApiService::get_document(fixture.service.as_ref(), document_id.clone())
            .await
            .unwrap();
    assert_eq!(http_updated, direct_after_update);
    let publication = events.recv().await.unwrap();
    assert_eq!(publication.revision, http_updated.revision);
    match publication.event {
        qingyu_kernel::contract::DomainEvent::DocumentChanged { document } => {
            assert_eq!(document.revision, http_updated.revision);
        }
        _ => panic!("document changed event expected"),
    }

    let history_query = PageQuery {
        cursor: None,
        limit: None,
    };
    let direct_history = DocumentsApiService::list_document_history(
        fixture.service.as_ref(),
        document_id.clone(),
        history_query,
    )
    .await
    .unwrap();
    let request = Request::builder()
        .method("GET")
        .uri(format!(
            "/api/v1/documents/{}/history",
            document_id.as_str()
        ))
        .header(header::HOST, "127.0.0.1:43123")
        .header(header::AUTHORIZATION, format!("Bearer {credential}"))
        .body(Body::empty())
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    let http_history: qingyu_kernel::contract::DocumentHistoryPageDto =
        serde_json::from_slice(&bytes).unwrap();
    assert_eq!(http_history, direct_history);

    let snapshot_id = http_history.items[0].snapshot_id;
    let direct_preview = DocumentsApiService::get_document_history(
        fixture.service.as_ref(),
        document_id.clone(),
        snapshot_id,
    )
    .await
    .unwrap();
    let request = Request::builder()
        .method("GET")
        .uri(format!(
            "/api/v1/documents/{}/history/{}",
            document_id.as_str(),
            snapshot_id.as_uuid()
        ))
        .header(header::HOST, "127.0.0.1:43123")
        .header(header::AUTHORIZATION, format!("Bearer {credential}"))
        .body(Body::empty())
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    let http_preview: qingyu_kernel::contract::DocumentHistorySnapshotDto =
        serde_json::from_slice(&bytes).unwrap();
    assert_eq!(http_preview, direct_preview);

    let direct_write_error = DocumentsApiService::update_document(
        fixture.service.as_ref(),
        document_id.clone(),
        UpdateDocumentRequest {
            workspace_generation: fixture.workspace.current().unwrap().generation,
            expected_revision: initial_revision.clone(),
            contents: DocumentContents::parse("stale direct write").unwrap(),
        },
    )
    .await
    .unwrap_err();
    let stale_update = UpdateDocumentRequest {
        workspace_generation: fixture.workspace.current().unwrap().generation,
        expected_revision: initial_revision,
        contents: DocumentContents::parse("stale HTTP write").unwrap(),
    };
    let request = Request::builder()
        .method("PUT")
        .uri(format!("/api/v1/documents/{}", document_id.as_str()))
        .header(header::HOST, "127.0.0.1:43123")
        .header(header::AUTHORIZATION, format!("Bearer {credential}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&stale_update).unwrap()))
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let write_envelope: qingyu_kernel::contract::ApiErrorEnvelope =
        serde_json::from_slice(&bytes).unwrap();
    assert_eq!(write_envelope.code(), direct_write_error.code());
    assert_eq!(write_envelope.details(), direct_write_error.details());

    let stale_id = fixture
        .runtime
        .wire_identity_key()
        .issue_document_id(
            fixture.workspace.current().unwrap().id,
            &WorkspaceGeneration::parse("stale-generation").unwrap(),
            DocumentKind::File,
            &WorkspaceRelativePath::parse("note.md").unwrap(),
        )
        .unwrap();
    let direct_error =
        DocumentsApiService::get_document(fixture.service.as_ref(), stale_id.clone())
            .await
            .unwrap_err();
    assert_eq!(
        direct_error.code(),
        qingyu_kernel::contract::ErrorCode::DocumentNotFound
    );
    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/v1/documents/{}", stale_id.as_str()))
        .header(header::HOST, "127.0.0.1:43123")
        .header(header::AUTHORIZATION, format!("Bearer {credential}"))
        .body(Body::empty())
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let envelope: qingyu_kernel::contract::ApiErrorEnvelope =
        serde_json::from_slice(&bytes).unwrap();
    assert_eq!(envelope.code(), direct_error.code());
    assert_eq!(envelope.details(), direct_error.details());
}
