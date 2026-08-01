use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read, Seek, SeekFrom, Write},
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Condvar, Mutex,
    },
    time::{Duration, Instant},
};

use axum::{
    body::{to_bytes, Body},
    http::{header, Request, StatusCode},
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use qingyu_kernel::{
    api::{build_router, TransportPolicy},
    config::KernelConfig,
    contract::{
        CreateWorkspaceResourceBatchItem, CreateWorkspaceResourceBatchRequest,
        CreateWorkspaceResourceQuery, DocumentContents, DocumentId, DocumentKind,
        ListDocumentsQuery, ListWorkspaceInventoryQuery, PageLimit, ResourceBatchId, ResourceKind,
        ResourceName, UpdateDocumentRequest, WorkspaceGeneration, WorkspaceRelativePath,
    },
    documents::{
        history::MemoryDocumentHistoryStore, service::CapabilityAtomicInstallPort,
        service::WorkspaceDocumentService, AtomicInstallPort, AtomicInstallPortError,
        AtomicInstallRequest, DeletionPort, DeletionPortError, DocumentDeletionTarget,
        DocumentIgnorePort,
    },
    ignore_rules::{
        MarkdownIgnoreRules, WorkspaceIgnoreError, WorkspaceIgnorePort, WorkspaceIgnoreSnapshot,
    },
    paths::KernelPaths,
    ports::KernelPorts,
    resources::{
        resolve_markdown_href, CreateResourceBatchItem, ResourceServiceErrorKind, RetainedResource,
        WorkspaceInventoryEntry, WorkspaceResourceService, MAX_RESOURCE_BODY_BYTES,
    },
    runtime::{KernelRuntime, ResourcesApiService, WorkspaceApiService},
    services::workspace::WorkspaceService,
    workspace::{
        managed::ManagedWorkspaceCollection,
        primary::{
            PrimaryWorkspaceRepositoryBinding, PrimaryWorkspaceStore, PrimaryWorkspaceStoreError,
        },
    },
};
use serde_json::Value;
use tempfile::tempdir;
use tower::ServiceExt as _;
use uuid::Uuid;

static_assertions::assert_impl_all!(RetainedResource: Read, Send);
static_assertions::assert_impl_all!(WorkspaceResourceService: Send, Sync);

fn image_fixture(extension: &str) -> Vec<u8> {
    let encoded = match extension {
        "png" => "iVBORw0KGgoAAAANSUhEUgAAABAAAAAMCAIAAADkharWAAAACXBIWXMAAAABAAAAAQBPJcTWAAAAtUlEQVR4nGP8x4AATP8ZgeS///9BbKb/cPH/DCA2I1iWhYFEANXgCKEc4AQSzcBgD7YBIkCuDfTSoPwZ5NaVoRJA8q/LVyBpXnsTSAoxMAHJW97SWGwoKgpjeHxowYIXZ26+mNhsMrF7Qn5pAVC8traOgWHu1KnXUDTc8WVguL3K/gADgwTDoZv2a2sZ/tcnB9dDJIsZt9oxKNhRw9OGHxgSEvR+eGrlV0xHFvd8fSg7O4RkGwDyqTc3JJObhAAAAABJRU5ErkJggg==",
        "jpg" => "/9j/4AAQSkZJRgABAgAAAQABAAD//gAQTGF2YzYyLjI4LjEwMgD/2wBDAAgEBAQEBAUFBQUFBQYGBgYGBgYGBgYGBgYHBwcICAgHBwcGBgcHCAgICAkJCQgICAgJCQoKCgwMCwsODg4RERT/xAB4AAEBAQAAAAAAAAAAAAAAAAAGBAUBAQEBAQAAAAAAAAAAAAAAAAYHAwUQAAICAgICAQUBAQAAAAAAAAEDAgQREgYTBQAVUUEzIiQhFhEAAQMCBAQFBQEAAAAAAAAAAQIDBAURADIGEjETIaFRYSNBB4FxFFIzIv/AABEIAAwAEAMBEgACEgADEgD/2gAMAwEAAhEDEQA/ABPGuTcz8Tyi4rj1rokpcXTJTRktSoVpGbGsuKktcB2SBkyQGZAZzj2riCFWncsrujup1ziimRyY7Qna1kMxIkMgkZBB+npafTKJJp8R6oNlRQ6haSFEG6FgiwHU/bGFadWyqgrQbELc8xx9xwOE/wAuxZdZ13Sxs5ppodMQ5Usl5Fl7jw/0PHHa1ey3Iq9ebcFwUx/JQ6ex4j6YwOI/93x3zskUv5rJqQv9o+PdVNMpaRbFpnZTNfWZj3BvXudM7/567o+Lor4T8VFOKcfPvpRTuw4rjlU1Be5n2fj/AF223++c+0qqfJjsyjMKcqQVHQ2hIRyrOCwAyZ+2Jc/Upitcfllz1NqelvTyW/nl7YBTNPz5Wna5QVMXZnqZVIb3hKXC2rckh3gLHwOFUqBGXoxcUpPLJ8Tvzfvm74//2Q==",
        "gif" => "R0lGODlhEAAMAPcfMQAAAAAAVQAAqgAA/wAkAAAkVQAkqgAk/wBIAABIVQBIqgBI/wBsAABsVQBsqgBs/wCQAACQVQCQqgCQ/wC0AAC0VQC0qgC0/wDYAADYVQDYqgDY/wD8AAD8VQD8qgD8/yQAACQAVSQAqiQA/yQkACQkVSQkqiQk/yRIACRIVSRIqiRI/yRsACRsVSRsqiRs/ySQACSQVSSQqiSQ/yS0ACS0VSS0qiS0/yTYACTYVSTYqiTY/yT8ACT8VST8qiT8/0gAAEgAVUgAqkgA/0gkAEgkVUgkqkgk/0hIAEhIVUhIqkhI/0hsAEhsVUhsqkhs/0iQAEiQVUiQqkiQ/0i0AEi0VUi0qki0/0jYAEjYVUjYqkjY/0j8AEj8VUj8qkj8/2wAAGwAVWwAqmwA/2wkAGwkVWwkqmwk/2xIAGxIVWxIqmxI/2xsAGxsVWxsqmxs/2yQAGyQVWyQqmyQ/2y0AGy0VWy0qmy0/2zYAGzYVWzYqmzY/2z8AGz8VWz8qmz8/5AAAJAAVZAAqpAA/5AkAJAkVZAkqpAk/5BIAJBIVZBIqpBI/5BsAJBsVZBsqpBs/5CQAJCQVZCQqpCQ/5C0AJC0VZC0qpC0/5DYAJDYVZDYqpDY/5D8AJD8VZD8qpD8/7QAALQAVbQAqrQA/7QkALQkVbQkqrQk/7RIALRIVbRIqrRI/7RsALRsVbRsqrRs/7SQALSQVbSQqrSQ/7S0ALS0VbS0qrS0/7TYALTYVbTYqrTY/7T8ALT8VbT8qrT8/9gAANgAVdgAqtgA/9gkANgkVdgkqtgk/9hIANhIVdhIqthI/9hsANhsVdhsqths/9iQANiQVdiQqtiQ/9i0ANi0Vdi0qti0/9jYANjYVdjYqtjY/9j8ANj8Vdj8qtj8//wAAPwAVfwAqvwA//wkAPwkVfwkqvwk//xIAPxIVfxIqvxI//xsAPxsVfxsqvxs//yQAPyQVfyQqvyQ//y0APy0Vfy0qvy0//zYAPzYVfzYqvzY//z8APz8Vfz8qvz8/yH/C05FVFNDQVBFMi4wAwEAAAAh+QQEZAAfACwAAAAAEAAMAAAIlgDBCQTHgQM/fgIGKBw37sOHgQQN8lO4sOFDcECAIECABAWAAB+BBEGQABg4EEAIICCBJIDLACGCFEggMCMCAkiQfAwZgqRAlCpZvoQJZGZNIChIQIu2FATKIEt/pqzEDBGQeJJQSotG6Cg8djk/whvLzudJIOzgCQWQNq1RjEC+kokWDcTXr0RimUSZlgy0VmjvEqkVEAA7",
        "webp" => "UklGRq4AAABXRUJQVlA4TKEAAAAvD8ACAAZXtW2rym7yPv1hBU4y3P2LBuR0hwJ7KYltK3o9vqQD8qNxWxBikIAWSDqgGDx6a8FXE8lOdRGQSRsN/CrqYILBA2KQgFXof+gDnCGcPgjftuT5jM8nMhAQgrEwAHE4EPyno2D+nPHuFqnlzKuVeDiWNX+eZhQZ776uV6Ry1h1ntVKSuPcFgrEw6AtAIPi0mdjPtuhbmnthObT5HgA=",
        "avif" => "AAAAIGZ0eXBhdmlmAAAAAGF2aWZtaWYxbWlhZk1BMUIAAAD5bWV0YQAAAAAAAAAvaGRscgAAAAAAAAAAcGljdAAAAAAAAAAAAAAAAFBpY3R1cmVIYW5kbGVyAAAAAA5waXRtAAAAAAABAAAAHmlsb2MAAAAARAAAAQABAAAAAQAAASEAAADxAAAAKGlpbmYAAAAAAAEAAAAaaW5mZQIAAAAAAQAAYXYwMUNvbG9yAAAAAGppcHJwAAAAS2lwY28AAAAUaXNwZQAAAAAAAAAQAAAADAAAABBwaXhpAAAAAAMICAgAAAAMYXYxQ4EADAAAAAATY29scm5jbHgAAgACAAIAAAAAF2lwbWEAAAAAAAAAAQABBAECgwQAAAD5bWRhdAoKAgAABQz+xK+QBDLiARAAloAQQIKB94DAXp2W8xbG+qGYQZDfijM9kuWB+kLCAK0jeG84US9KCgPrGaIlb6RX2S+/CTm9h9eO/0yZfAVy1st6Kph10tEPbSTiSfV8a5tcoiCXpmFwOXQbmQC6zsUbgLky/8U3zfOMCtoKw+dVyhmdhx2OrSfxIiKp6rp6aBkwN1nFpwS7i8XPXaq8hK0F05roGuiwTlitOUb8xmkMGs/WxLdHiBxYt24BeFZqTpUoODjjym8ViX/b1dXd9b2SjQaR6vhB+Ymz0A+xMrMuEc/qJK6p2O5/JiNxb7byCDA=",
        "bmp" => "Qk02AwAAAAAAADYAAAAoAAAAEAAAAAwAAAABACAAAAAAAAADAAAAAAAAAAAAAAAAAAAAAAAAAAD+/wAA/v8AAD//AAA//wB///8Af///AC9v/y6Pz/9piMf/AAA2/wB///8Af///AH///wB///8AKkn/VJW0/wAA/v8AAP7/AAA//wAAP/8Af///AH///wAvb/8uj8//aYjH/wAANv8Af///AH///wB///8Af///ACpJ/1SVtP8AAP7/AAD+/wAAP/8AAD//AH///wB///8APz7/AD8+/z8AAP8/AAD/AH///wB///8Af///AH///z4/AP8+PwD/AAD+/wAA/v8AAD//AAA//wB///8Af///AD8+/wA/Pv8/AAD/PwAA/wB///8Af///AH///wB///8+PwD/Pj8A/wAA/v8AAP7/AAA//wAAP/8AMiP/VqSV/xiH5/8AJ4f/AABT/zSD5P+di6z/DQAb/w8AUP+Nfc3/Kora/wAfb/8AAP7/AAD+/wAAP/8AAD//ADIj/wAyI/8Yh+f/GIfn/zSD5P80g+T/DQAb/w0AG/8PAFD/DwBQ/yqK2v8qitr/AAD+/wAA/v8AAD//AAA//wE/AP8BPwD/AD8+/wA/Pv8/AAD/PwAA/z8AAP8/AAD/PgA+/z4APv8+PwD/Pj8A/wAA/v8AAP7/AAA//wAAP/8BPwD/AT8A/wA/Pv8APz7/PwAA/z8AAP8/AAD/PwAA/z4APv8+AD7/Pj8A/z4/AP8AAP7/AAD+/wAAP/8AAD//AT8A/wE/AP8APz7/AD8+/z8AAP8/AAD/PwAA/z8AAP8+AD7/PgA+/z4/AP8+PwD/AAD+/wAA/v8AAD//AAA//wE/AP8BPwD/AD8+/wA/Pv8/AAD/PwAA/z8AAP8/AAD/PgA+/z4APv8+PwD/Pj8A/wAA/v8AAP7/AAD+/wAA/v8B/wD/Af8A/wD+/v8A/v7//wAA//8AAP//AAD//wAA//4A///+AP////8A////AP8AAP7/AAD+/wAA/v8AAP7/Af8A/wH/AP8A/v7/AP7+//8AAP//AAD//wAA//8AAP/+AP///gD/////AP///wD/",
        _ => panic!("unknown fixture"),
    };
    STANDARD.decode(encoded).unwrap()
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

struct Fixture {
    runtime: Arc<KernelRuntime>,
    _workspace: Arc<WorkspaceService>,
    service: WorkspaceResourceService,
    ignore: Arc<LiveIgnorePort>,
    root: PathBuf,
}

struct LiveIgnorePort {
    captures: AtomicUsize,
    global_rules: Mutex<String>,
    replacement_after_capture: Mutex<Option<(PathBuf, PathBuf)>>,
}

impl LiveIgnorePort {
    fn set_global_rules(&self, rules: &str) {
        *self.global_rules.lock().unwrap() = rules.to_string();
    }

    fn replace_root_after_capture(&self, root: PathBuf, retired: PathBuf) {
        *self.replacement_after_capture.lock().unwrap() = Some((root, retired));
    }
}

struct CapturedIgnorePort {
    root: PathBuf,
    rules: MarkdownIgnoreRules,
}

struct UnusedDeletionPort;

struct RejectingAtomicInstallPort;

impl AtomicInstallPort for RejectingAtomicInstallPort {
    fn install(&self, _request: AtomicInstallRequest<'_>) -> Result<(), AtomicInstallPortError> {
        Err(AtomicInstallPortError)
    }
}

struct InstallThenReportFailurePort;

impl AtomicInstallPort for InstallThenReportFailurePort {
    fn install(&self, request: AtomicInstallRequest<'_>) -> Result<(), AtomicInstallPortError> {
        CapabilityAtomicInstallPort.install(request)?;
        Err(AtomicInstallPortError)
    }
}

impl DeletionPort for UnusedDeletionPort {
    fn delete(
        &self,
        _target: &DocumentDeletionTarget,
        _policy: qingyu_kernel::contract::DeletionPolicy,
    ) -> Result<(), DeletionPortError> {
        Err(DeletionPortError)
    }
}

#[derive(Default)]
struct BlockingIgnorePort {
    condition: Condvar,
    state: Mutex<BlockingIgnoreState>,
}

#[derive(Default)]
struct BlockingIgnoreState {
    entered: usize,
    released: bool,
}

impl BlockingIgnorePort {
    fn wait_until_entered(&self, expected: usize) {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut state = self.state.lock().unwrap();
        while state.entered < expected {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .expect("inventory captures should enter before timeout");
            let (next, timeout) = self.condition.wait_timeout(state, remaining).unwrap();
            state = next;
            assert!(!timeout.timed_out(), "inventory captures should enter");
        }
    }

    fn entered(&self) -> usize {
        self.state.lock().unwrap().entered
    }

    fn release(&self) {
        self.state.lock().unwrap().released = true;
        self.condition.notify_all();
    }
}

impl WorkspaceIgnorePort for BlockingIgnorePort {
    fn capture(
        &self,
        root_path: &std::path::Path,
        retained_root: &cap_std::fs::Dir,
    ) -> Result<WorkspaceIgnoreSnapshot, WorkspaceIgnoreError> {
        let mut state = self.state.lock().unwrap();
        state.entered += 1;
        let should_block = state.entered <= 2;
        self.condition.notify_all();
        while should_block && !state.released {
            state = self.condition.wait(state).unwrap();
        }
        drop(state);
        let rules = MarkdownIgnoreRules::try_for_retained_root(root_path, retained_root, None)?;
        Ok(WorkspaceIgnoreSnapshot::from_matcher(Arc::new(
            CapturedIgnorePort {
                root: root_path.to_path_buf(),
                rules,
            },
        )))
    }
}

impl DocumentIgnorePort for CapturedIgnorePort {
    fn is_ignored(&self, path: &WorkspaceRelativePath, kind: DocumentKind) -> bool {
        self.rules.ignores(
            &self.root.join(path.as_str()),
            kind == DocumentKind::Directory,
        )
    }
}

impl WorkspaceIgnorePort for LiveIgnorePort {
    fn capture(
        &self,
        root_path: &std::path::Path,
        retained_root: &cap_std::fs::Dir,
    ) -> Result<WorkspaceIgnoreSnapshot, WorkspaceIgnoreError> {
        self.captures.fetch_add(1, Ordering::SeqCst);
        let global_rules = self.global_rules.lock().unwrap().clone();
        let rules = MarkdownIgnoreRules::try_for_retained_root(
            root_path,
            retained_root,
            Some(&global_rules),
        )?;
        if let Some((root, retired)) = self.replacement_after_capture.lock().unwrap().take() {
            fs::rename(&root, retired).map_err(|_| WorkspaceIgnoreError)?;
            fs::create_dir(root).map_err(|_| WorkspaceIgnoreError)?;
        }
        Ok(WorkspaceIgnoreSnapshot::from_matcher(Arc::new(
            CapturedIgnorePort {
                root: root_path.to_path_buf(),
                rules,
            },
        )))
    }
}

impl Fixture {
    async fn new() -> Self {
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
        let workspace = Arc::new(
            WorkspaceService::new(
                &runtime,
                Arc::new(MemoryWorkspaceStore::default()),
                managed,
                runtime.event_broker().clone(),
                "Resources",
            )
            .await
            .unwrap(),
        );
        let ignore = Arc::new(LiveIgnorePort {
            captures: AtomicUsize::new(0),
            global_rules: Mutex::new(String::new()),
            replacement_after_capture: Mutex::new(None),
        });
        let service = WorkspaceResourceService::new(&runtime, ignore.clone());
        Self {
            runtime,
            _workspace: workspace,
            service,
            ignore,
            root,
        }
    }
}

async fn activate_resources_at(
    root: &std::path::Path,
    app_data: &std::path::Path,
    cache: &std::path::Path,
    workspace_store: Arc<MemoryWorkspaceStore>,
) -> (
    Arc<KernelRuntime>,
    Arc<WorkspaceService>,
    Arc<LiveIgnorePort>,
) {
    let paths = KernelPaths::desktop(root, app_data, cache).unwrap();
    let managed = ManagedWorkspaceCollection::from_paths(&paths).unwrap();
    let runtime = KernelRuntime::activate(
        KernelConfig::generate().unwrap(),
        paths,
        KernelPorts::unavailable(),
    )
    .unwrap();
    let workspace = Arc::new(
        WorkspaceService::new(
            &runtime,
            workspace_store,
            managed,
            runtime.event_broker().clone(),
            "Resources",
        )
        .await
        .unwrap(),
    );
    let ignore = Arc::new(LiveIgnorePort {
        captures: AtomicUsize::new(0),
        global_rules: Mutex::new(String::new()),
        replacement_after_capture: Mutex::new(None),
    });
    (runtime, workspace, ignore)
}

fn batch_state_directory(
    app_data: &std::path::Path,
    workspace: &qingyu_kernel::contract::WorkspaceDto,
) -> PathBuf {
    app_data
        .join("resource-batches-v1")
        .join(workspace.id.as_uuid().to_string())
}

fn batch_record_path(state: &std::path::Path, prefix: &str) -> PathBuf {
    let mut matches = fs::read_dir(state)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(prefix) && name.ends_with(".json"))
        })
        .collect::<Vec<_>>();
    matches.sort();
    assert_eq!(matches.len(), 1, "expected exactly one {prefix} record");
    matches.pop().unwrap()
}

fn first_document_id(service: &WorkspaceResourceService) -> DocumentId {
    service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap()
        .into_iter()
        .find_map(|entry| match entry {
            WorkspaceInventoryEntry::Document(entry) if entry.kind == DocumentKind::File => {
                Some(entry.id)
            }
            _ => None,
        })
        .unwrap()
}

fn png_batch_items(names: &[&str]) -> Vec<CreateResourceBatchItem> {
    names
        .iter()
        .map(|name| {
            CreateResourceBatchItem::image(
                ResourceName::parse(*name).unwrap(),
                "image/png",
                image_fixture("png"),
            )
        })
        .collect()
}

#[test]
fn markdown_href_resolves_portable_percent_encoded_paths_within_the_workspace() {
    let document = WorkspaceRelativePath::parse("notes/chapter/note.md").unwrap();

    let resolved = resolve_markdown_href(&document, "../../assets/image%20one.png").unwrap();
    let unicode = resolve_markdown_href(&document, "./%E5%9B%BE%E7%89%87.webp").unwrap();

    assert_eq!(resolved.as_str(), "assets/image one.png");
    assert_eq!(unicode.as_str(), "notes/chapter/图片.webp");
}

#[test]
fn markdown_href_rejects_external_ambiguous_or_escaping_paths() {
    let document = WorkspaceRelativePath::parse("notes/chapter/note.md").unwrap();
    let rejected = [
        "",
        "../../../outside.png",
        "/absolute.png",
        "//host/share.png",
        "C:/windows.png",
        "https://example.test/image.png",
        "data:image/png;base64,AAAA",
        "..\\outside.png",
        "image.png?../../outside",
        "image.png#../../outside",
        "../../.markra-sync/private.bin",
        "../../.git/config",
        "%2Fabsolute.png",
        "%5Coutside.png",
        "%ZZ.png",
        "con.png",
        "trailing-space.png ",
    ];

    for href in rejected {
        let error = resolve_markdown_href(&document, href).unwrap_err();
        assert_eq!(
            error.kind(),
            ResourceServiceErrorKind::InvalidPath,
            "{href}"
        );
    }
}

#[test]
fn markdown_href_rejects_a_document_inside_a_protected_parent() {
    for document in [
        ".qingyu/note.md",
        "notes/.markra-sync/note.md",
        "notes/.git/note.md",
        "notes/node_modules/note.md",
    ] {
        let document = WorkspaceRelativePath::parse(document).unwrap();

        let error = resolve_markdown_href(&document, "image.png").unwrap_err();

        assert_eq!(error.kind(), ResourceServiceErrorKind::InvalidPath);
    }
}

#[tokio::test]
async fn inventory_applies_workspace_ignore_rules_to_documents_and_resources() {
    let fixture = Fixture::new().await;
    fs::create_dir(fixture.root.join("ignored")).unwrap();
    fs::write(fixture.root.join("ignored/hidden.bin"), b"hidden").unwrap();
    fs::write(fixture.root.join("hidden.bin"), b"hidden").unwrap();
    fs::write(fixture.root.join("visible.bin"), b"visible").unwrap();
    fs::write(
        fixture.root.join(".markraignore"),
        b"ignored/\nhidden.bin\n",
    )
    .unwrap();

    let inventory = fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap();

    assert_eq!(
        inventory
            .iter()
            .map(|entry| entry.path().as_str())
            .collect::<Vec<_>>(),
        ["visible.bin"]
    );
}

#[tokio::test]
async fn each_inventory_operation_captures_ignore_rules_exactly_once() {
    let fixture = Fixture::new().await;
    for name in ["first.bin", "second.bin", "third.bin"] {
        fs::write(fixture.root.join(name), name).unwrap();
    }

    fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap();

    assert_eq!(fixture.ignore.captures.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn inventory_scan_gate_rejects_a_third_concurrent_scan_without_queueing() {
    use std::{sync::mpsc, thread};

    let fixture = Fixture::new().await;
    fs::write(fixture.root.join("visible.bin"), b"visible").unwrap();
    let blocking = Arc::new(BlockingIgnorePort::default());
    let service = WorkspaceResourceService::new(&fixture.runtime, blocking.clone());
    let parent = WorkspaceRelativePath::default();
    let first_service = service.clone();
    let first_parent = parent.clone();
    let first = thread::spawn(move || first_service.list_inventory(&first_parent));
    let second_service = service.clone();
    let second = thread::spawn(move || {
        second_service.list_inventory_page(ListWorkspaceInventoryQuery {
            cursor: None,
            limit: Some(PageLimit::new(10).unwrap()),
            parent,
        })
    });
    blocking.wait_until_entered(2);

    let (sender, receiver) = mpsc::channel();
    let third = thread::spawn(move || {
        sender
            .send(service.list_inventory_page(ListWorkspaceInventoryQuery {
                cursor: None,
                limit: Some(PageLimit::new(10).unwrap()),
                parent: WorkspaceRelativePath::default(),
            }))
            .unwrap();
    });
    let prompt_result = receiver.recv_timeout(Duration::from_millis(250));
    let entered_before_release = blocking.entered();
    blocking.release();
    first.join().unwrap().unwrap();
    second.join().unwrap().unwrap();
    third.join().unwrap();

    let error = prompt_result
        .expect("the third inventory scan must fail immediately")
        .unwrap_err();
    assert_eq!(error.kind(), ResourceServiceErrorKind::Unavailable);
    assert_eq!(entered_before_release, 2);
}

#[tokio::test]
async fn inventory_scan_gate_does_not_gate_open_resource() {
    use std::thread;

    let fixture = Fixture::new().await;
    fs::write(fixture.root.join("visible.bin"), b"visible").unwrap();
    let resource_id = fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap()
        .into_iter()
        .find_map(|entry| match entry {
            WorkspaceInventoryEntry::Resource(resource) => Some(resource.id),
            WorkspaceInventoryEntry::Document(_) => None,
        })
        .unwrap();
    let blocking = Arc::new(BlockingIgnorePort::default());
    let service = WorkspaceResourceService::new(&fixture.runtime, blocking.clone());
    let first_service = service.clone();
    let first =
        thread::spawn(move || first_service.list_inventory(&WorkspaceRelativePath::default()));
    let second_service = service.clone();
    let second =
        thread::spawn(move || second_service.list_inventory(&WorkspaceRelativePath::default()));
    blocking.wait_until_entered(2);

    let opened = service
        .open_resource(&resource_id, ResourceKind::Attachment)
        .unwrap();
    blocking.release();
    first.join().unwrap().unwrap();
    second.join().unwrap().unwrap();

    assert_eq!(opened.entry().id, resource_id);
    assert_eq!(blocking.entered(), 3);
}

#[tokio::test]
async fn inventory_pages_continue_unchanged_and_reject_a_changed_collection() {
    let fixture = Fixture::new().await;
    fs::write(fixture.root.join("first.bin"), b"first").unwrap();
    fs::write(fixture.root.join("second.bin"), b"second").unwrap();
    let query = ListWorkspaceInventoryQuery {
        cursor: None,
        limit: Some(PageLimit::new(1).unwrap()),
        parent: WorkspaceRelativePath::default(),
    };

    let first = fixture.service.list_inventory_page(query.clone()).unwrap();
    let cursor = first.next_cursor.into_option().expect("next cursor");
    let second = fixture
        .service
        .list_inventory_page(ListWorkspaceInventoryQuery {
            cursor: Some(cursor),
            ..query.clone()
        })
        .unwrap();
    assert_eq!(
        first
            .items
            .iter()
            .chain(&second.items)
            .map(|entry| entry.path().as_str())
            .collect::<Vec<_>>(),
        ["first.bin", "second.bin"]
    );

    let first = fixture.service.list_inventory_page(query.clone()).unwrap();
    let cursor = first.next_cursor.into_option().expect("next cursor");
    fs::write(fixture.root.join("third.bin"), b"third").unwrap();
    let error = fixture
        .service
        .list_inventory_page(ListWorkspaceInventoryQuery {
            cursor: Some(cursor),
            ..query
        })
        .unwrap_err();

    assert_eq!(error.kind(), ResourceServiceErrorKind::InvalidCursor);
}

#[tokio::test]
async fn inventory_page_rejects_a_replaced_workspace_root_before_returning() {
    let fixture = Fixture::new().await;
    fs::write(fixture.root.join("visible.bin"), b"visible").unwrap();
    let retired = fixture.root.with_extension("retired");
    fixture
        .ignore
        .replace_root_after_capture(fixture.root.clone(), retired);

    let error = fixture
        .service
        .list_inventory_page(ListWorkspaceInventoryQuery {
            cursor: None,
            limit: Some(PageLimit::new(10).unwrap()),
            parent: WorkspaceRelativePath::default(),
        })
        .unwrap_err();

    assert_eq!(error.kind(), ResourceServiceErrorKind::Unavailable);
}

#[cfg(unix)]
#[tokio::test]
async fn a_one_item_inventory_page_does_not_read_later_resource_contents() {
    use std::os::unix::fs::PermissionsExt as _;

    let fixture = Fixture::new().await;
    fs::write(fixture.root.join("first.bin"), b"first").unwrap();
    let later = fixture.root.join("second.bin");
    fs::write(&later, b"second").unwrap();
    let original_permissions = fs::metadata(&later).unwrap().permissions();
    let mut unreadable = original_permissions.clone();
    unreadable.set_mode(0o000);
    fs::set_permissions(&later, unreadable).unwrap();

    let result = fixture
        .service
        .list_inventory_page(ListWorkspaceInventoryQuery {
            cursor: None,
            limit: Some(PageLimit::new(1).unwrap()),
            parent: WorkspaceRelativePath::default(),
        });
    fs::set_permissions(&later, original_permissions).unwrap();

    let page = result.expect("later pages must not amplify content reads into the first page");
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].path().as_str(), "first.bin");
    assert!(page.next_cursor.into_option().is_some());
}

#[cfg(unix)]
#[tokio::test]
async fn inventory_cursor_rejects_a_same_length_rewrite_with_restored_mtime() {
    use std::{thread, time::Duration};

    let fixture = Fixture::new().await;
    fs::write(fixture.root.join("first.bin"), b"first").unwrap();
    let second = fixture.root.join("second.bin");
    fs::write(&second, b"second").unwrap();
    let query = ListWorkspaceInventoryQuery {
        cursor: None,
        limit: Some(PageLimit::new(1).unwrap()),
        parent: WorkspaceRelativePath::default(),
    };
    let first = fixture.service.list_inventory_page(query.clone()).unwrap();
    let cursor = first.next_cursor.into_option().unwrap();
    let modified = fs::metadata(&second).unwrap().modified().unwrap();
    thread::sleep(Duration::from_millis(2));
    fs::write(&second, b"change").unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&second)
        .unwrap()
        .set_modified(modified)
        .unwrap();

    let error = fixture
        .service
        .list_inventory_page(ListWorkspaceInventoryQuery {
            cursor: Some(cursor),
            ..query
        })
        .unwrap_err();

    assert_eq!(error.kind(), ResourceServiceErrorKind::InvalidCursor);
}

#[cfg(unix)]
#[tokio::test]
async fn inventory_cursor_rejects_a_descendant_rewrite_in_a_listed_directory() {
    use std::{thread, time::Duration};

    let fixture = Fixture::new().await;
    fs::create_dir(fixture.root.join("folder")).unwrap();
    let nested = fixture.root.join("folder/nested.bin");
    fs::write(&nested, b"before").unwrap();
    fs::write(fixture.root.join("later.bin"), b"later").unwrap();
    let query = ListWorkspaceInventoryQuery {
        cursor: None,
        limit: Some(PageLimit::new(1).unwrap()),
        parent: WorkspaceRelativePath::default(),
    };
    let first = fixture.service.list_inventory_page(query.clone()).unwrap();
    assert_eq!(first.items[0].path().as_str(), "folder");
    let cursor = first.next_cursor.into_option().unwrap();
    let modified = fs::metadata(&nested).unwrap().modified().unwrap();
    thread::sleep(Duration::from_millis(2));
    fs::write(&nested, b"after!").unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&nested)
        .unwrap()
        .set_modified(modified)
        .unwrap();

    let error = fixture
        .service
        .list_inventory_page(ListWorkspaceInventoryQuery {
            cursor: Some(cursor),
            ..query
        })
        .unwrap_err();

    assert_eq!(error.kind(), ResourceServiceErrorKind::InvalidCursor);
}

#[tokio::test]
async fn resource_http_adapter_matches_inventory_and_streams_verified_bytes() {
    let fixture = Fixture::new().await;
    let bytes = image_fixture("png");
    fs::write(fixture.root.join("image.png"), &bytes).unwrap();
    let query = ListWorkspaceInventoryQuery {
        cursor: None,
        limit: Some(PageLimit::new(10).unwrap()),
        parent: WorkspaceRelativePath::default(),
    };
    let direct = ResourcesApiService::list_workspace_inventory(&fixture.service, query)
        .await
        .unwrap();
    let resource = direct
        .items
        .iter()
        .find_map(|entry| match entry {
            qingyu_kernel::contract::WorkspaceInventoryEntryDto::Resource { resource } => {
                Some(resource)
            }
            qingyu_kernel::contract::WorkspaceInventoryEntryDto::Document { .. } => None,
        })
        .unwrap();
    fixture
        .runtime
        .install_resources_api_service(Arc::new(fixture.service.clone()))
        .unwrap();
    let credential = fixture.runtime.expose_native_launch_credential();
    let router = build_router(
        fixture.runtime.clone(),
        TransportPolicy::loopback("127.0.0.1:43123", "http://127.0.0.1:43123").unwrap(),
    );

    let request = Request::builder()
        .method("GET")
        .uri("/api/v1/inventory?limit=10")
        .header(header::HOST, "127.0.0.1:43123")
        .header(header::AUTHORIZATION, format!("Bearer {credential}"))
        .body(Body::empty())
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let http: qingyu_kernel::contract::WorkspaceInventoryPageDto =
        serde_json::from_slice(&body).unwrap();
    assert_eq!(http, direct);

    let request = Request::builder()
        .method("GET")
        .uri(format!(
            "/api/v1/resources/{}?kind=image",
            resource.id.as_str()
        ))
        .header(header::HOST, "127.0.0.1:43123")
        .header(header::AUTHORIZATION, format!("Bearer {credential}"))
        .body(Body::empty())
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "image/png"
    );
    assert_eq!(
        response.headers().get(header::CONTENT_LENGTH).unwrap(),
        bytes.len().to_string().as_str()
    );
    assert_eq!(
        response.headers().get("x-content-type-options").unwrap(),
        "nosniff"
    );
    assert_eq!(
        to_bytes(response.into_body(), 1024 * 1024).await.unwrap(),
        bytes.as_slice()
    );

    let request = Request::builder()
        .method("GET")
        .uri(format!(
            "/api/v1/resources/{}?kind=attachment",
            resource.id.as_str()
        ))
        .header(header::HOST, "127.0.0.1:43123")
        .header(header::AUTHORIZATION, format!("Bearer {credential}"))
        .body(Body::empty())
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let envelope: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
            .unwrap();
    assert_eq!(envelope["code"], "resource_not_found");
}

#[tokio::test]
async fn resource_http_writer_accepts_a_bounded_raw_body_and_returns_an_openable_resource() {
    let fixture = Fixture::new().await;
    fs::write(fixture.root.join("note.md"), b"# Note").unwrap();
    let workspace = fixture._workspace.get_workspace().await.unwrap();
    let document_id = fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap()
        .into_iter()
        .find_map(|entry| match entry {
            WorkspaceInventoryEntry::Document(entry) if entry.path.as_str() == "note.md" => {
                Some(entry.id)
            }
            _ => None,
        })
        .unwrap();
    fixture
        .runtime
        .install_resources_api_service(Arc::new(fixture.service.clone()))
        .unwrap();
    let credential = fixture.runtime.expose_native_launch_credential();
    let router = build_router(
        fixture.runtime.clone(),
        TransportPolicy::loopback("127.0.0.1:43123", "http://127.0.0.1:43123").unwrap(),
    );
    let bytes = image_fixture("png");

    let request = Request::builder()
        .method("POST")
        .uri(format!(
            "/api/v1/documents/{}/resources?workspaceGeneration={}&folder=assets&name=pasted.png&kind=image",
            document_id.as_str(),
            workspace.generation.as_str(),
        ))
        .header(header::HOST, "127.0.0.1:43123")
        .header(header::AUTHORIZATION, format!("Bearer {credential}"))
        .header(header::CONTENT_TYPE, "image/png")
        .header(header::CONTENT_LENGTH, bytes.len())
        .body(Body::from(bytes.clone()))
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let response_body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(
        status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&response_body)
    );
    let created: qingyu_kernel::contract::ResourceEntryDto =
        serde_json::from_slice(&response_body).unwrap();
    assert_eq!(created.path.as_str(), "assets/pasted.png");
    assert_eq!(
        created.revision.as_str().split_once(':').unwrap().0,
        "sha256"
    );

    let request = Request::builder()
        .method("GET")
        .uri(format!(
            "/api/v1/resources/{}?kind=image",
            created.id.as_str()
        ))
        .header(header::HOST, "127.0.0.1:43123")
        .header(header::AUTHORIZATION, format!("Bearer {credential}"))
        .body(Body::empty())
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(response.into_body(), 1024 * 1024).await.unwrap(),
        bytes.as_slice()
    );
}

#[tokio::test]
async fn resource_http_writer_rejects_oversize_unauthenticated_and_wrong_host_requests() {
    let fixture = Fixture::new().await;
    fs::write(fixture.root.join("note.md"), b"# Note").unwrap();
    let workspace = fixture._workspace.get_workspace().await.unwrap();
    let document_id = fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap()
        .into_iter()
        .find_map(|entry| match entry {
            WorkspaceInventoryEntry::Document(entry) if entry.kind == DocumentKind::File => {
                Some(entry.id)
            }
            WorkspaceInventoryEntry::Resource(_) => None,
            WorkspaceInventoryEntry::Document(_) => None,
        })
        .unwrap();
    fixture
        .runtime
        .install_resources_api_service(Arc::new(fixture.service.clone()))
        .unwrap();
    let credential = fixture.runtime.expose_native_launch_credential();
    let router = build_router(
        fixture.runtime.clone(),
        TransportPolicy::loopback("127.0.0.1:43123", "http://127.0.0.1:43123").unwrap(),
    );
    let uri = format!(
        "/api/v1/documents/{}/resources?workspaceGeneration={}&folder=assets&name=asset.bin&kind=attachment",
        document_id.as_str(),
        workspace.generation.as_str(),
    );

    let request = Request::builder()
        .method("POST")
        .uri(&uri)
        .header(header::HOST, "127.0.0.1:43123")
        .header(header::AUTHORIZATION, format!("Bearer {credential}"))
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_LENGTH, MAX_RESOURCE_BODY_BYTES + 1)
        .body(Body::empty())
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let envelope: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
            .unwrap();
    assert_eq!(envelope["code"], "resource_too_large");

    let request = Request::builder()
        .method("POST")
        .uri(&uri)
        .header(header::HOST, "127.0.0.1:43123")
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .body(Body::from("asset"))
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let request = Request::builder()
        .method("POST")
        .uri(&uri)
        .header(header::HOST, "attacker.example")
        .header(header::AUTHORIZATION, format!("Bearer {credential}"))
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .body(Body::from("asset"))
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(!fixture.root.join("assets").exists());
}

#[tokio::test]
async fn resource_http_stream_fails_before_its_final_chunk_when_the_file_changes() {
    let fixture = Fixture::new().await;
    let path = fixture.root.join("changing.bin");
    fs::write(&path, vec![b'a'; 128 * 1024]).unwrap();
    let entry = fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap()
        .into_iter()
        .find_map(|entry| match entry {
            WorkspaceInventoryEntry::Resource(entry) => Some(entry),
            WorkspaceInventoryEntry::Document(_) => None,
        })
        .unwrap();
    fixture
        .runtime
        .install_resources_api_service(Arc::new(fixture.service.clone()))
        .unwrap();
    let credential = fixture.runtime.expose_native_launch_credential();
    let router = build_router(
        fixture.runtime.clone(),
        TransportPolicy::loopback("127.0.0.1:43123", "http://127.0.0.1:43123").unwrap(),
    );
    let request = Request::builder()
        .method("GET")
        .uri(format!(
            "/api/v1/resources/{}?kind=attachment",
            entry.id.as_str()
        ))
        .header(header::HOST, "127.0.0.1:43123")
        .header(header::AUTHORIZATION, format!("Bearer {credential}"))
        .body(Body::empty())
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    fs::write(path, vec![b'b'; 128 * 1024]).unwrap();

    assert!(to_bytes(response.into_body(), 1024 * 1024).await.is_err());
}

#[tokio::test]
async fn an_existing_resource_id_cannot_bypass_changed_workspace_ignore_rules() {
    let fixture = Fixture::new().await;
    fs::write(fixture.root.join("asset.bin"), b"asset").unwrap();
    let entry = fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap()
        .into_iter()
        .find_map(|entry| match entry {
            WorkspaceInventoryEntry::Resource(entry) => Some(entry),
            WorkspaceInventoryEntry::Document(_) => None,
        })
        .unwrap();
    fs::write(fixture.root.join(".markraignore"), b"asset.bin\n").unwrap();

    let error = fixture
        .service
        .open_resource(&entry.id, ResourceKind::Attachment)
        .unwrap_err();

    assert_eq!(error.kind(), ResourceServiceErrorKind::NotFound);
}

#[tokio::test]
async fn inventory_and_existing_resource_ids_apply_changed_global_ignore_rules() {
    let fixture = Fixture::new().await;
    fs::write(fixture.root.join("asset.bin"), b"asset").unwrap();
    fs::write(fixture.root.join("visible.bin"), b"visible").unwrap();
    let entry = fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap()
        .into_iter()
        .find_map(|entry| match entry {
            WorkspaceInventoryEntry::Resource(entry) if entry.path.as_str() == "asset.bin" => {
                Some(entry)
            }
            _ => None,
        })
        .unwrap();
    fixture.ignore.set_global_rules("asset.bin\n");

    let inventory = fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap();
    let error = fixture
        .service
        .open_resource(&entry.id, ResourceKind::Attachment)
        .unwrap_err();

    assert_eq!(
        inventory
            .iter()
            .map(|entry| entry.path().as_str())
            .collect::<Vec<_>>(),
        ["visible.bin"]
    );
    assert_eq!(error.kind(), ResourceServiceErrorKind::NotFound);
}

#[tokio::test]
async fn inventory_lists_every_immediate_kind_and_classifies_images_from_magic_and_extension() {
    let fixture = Fixture::new().await;
    fs::create_dir(fixture.root.join("folder")).unwrap();
    fs::write(fixture.root.join("folder/nested.bin"), b"nested").unwrap();
    fs::write(fixture.root.join("note.md"), b"# Note").unwrap();
    fs::write(fixture.root.join("image.png"), image_fixture("png")).unwrap();
    fs::write(fixture.root.join("fake.png"), b"not a png").unwrap();
    fs::write(fixture.root.join("vector.svg"), b"<svg></svg>").unwrap();
    fs::write(fixture.root.join("archive.bin"), b"attachment").unwrap();
    fs::create_dir_all(fixture.root.join(".qingyu")).unwrap();
    fs::write(fixture.root.join(".qingyu/private.bin"), b"private").unwrap();
    fs::write(
        fixture.root.join(".qingyu-kernel-update-private.tmp"),
        b"private",
    )
    .unwrap();
    fs::write(fixture.root.join(".markraignore"), b"*.bin").unwrap();

    let inventory = fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap();
    let by_path = inventory
        .into_iter()
        .map(|entry| (entry.path().as_str().to_string(), entry))
        .collect::<BTreeMap<_, _>>();

    assert_eq!(
        by_path.keys().map(String::as_str).collect::<Vec<_>>(),
        ["fake.png", "folder", "image.png", "note.md", "vector.svg"]
    );
    assert!(matches!(
        by_path.get("folder").unwrap(),
        WorkspaceInventoryEntry::Document(entry) if entry.kind == DocumentKind::Directory
    ));
    assert!(matches!(
        by_path.get("note.md").unwrap(),
        WorkspaceInventoryEntry::Document(entry) if entry.kind == DocumentKind::File
    ));
    assert!(matches!(
        by_path.get("image.png").unwrap(),
        WorkspaceInventoryEntry::Resource(entry)
            if entry.kind == ResourceKind::Image
                && entry.media_type == "image/png"
                && entry.previewable
    ));
    for path in ["fake.png", "vector.svg"] {
        assert!(matches!(
            by_path.get(path).unwrap(),
            WorkspaceInventoryEntry::Resource(entry)
                if entry.kind == ResourceKind::Attachment
                    && entry.media_type == "application/octet-stream"
                    && !entry.previewable
        ));
    }
}

#[tokio::test]
async fn inventory_document_revision_is_accepted_by_the_document_mutation_contract() {
    let fixture = Fixture::new().await;
    fs::write(fixture.root.join("note.md"), b"first").unwrap();
    let document = fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap()
        .into_iter()
        .find_map(|entry| match entry {
            WorkspaceInventoryEntry::Document(entry) if entry.path.as_str() == "note.md" => {
                Some(entry)
            }
            _ => None,
        })
        .unwrap();
    let workspace = fixture._workspace.get_workspace().await.unwrap();
    let documents = WorkspaceDocumentService::new(
        &fixture.runtime,
        Arc::new(UnusedDeletionPort),
        Arc::new(MemoryDocumentHistoryStore::default()),
    );

    let updated = documents
        .update_document(
            document.id,
            UpdateDocumentRequest {
                workspace_generation: workspace.generation,
                expected_revision: document.revision,
                contents: DocumentContents::parse("second").unwrap(),
            },
        )
        .await
        .unwrap();

    assert_eq!(updated.contents.as_str(), "second");
    assert_eq!(
        fs::read_to_string(fixture.root.join("note.md")).unwrap(),
        "second"
    );
}

#[tokio::test]
async fn resource_writer_creates_a_document_relative_image_and_returns_an_openable_snapshot() {
    let fixture = Fixture::new().await;
    fs::write(fixture.root.join("note.md"), b"# Note").unwrap();
    let workspace = fixture._workspace.get_workspace().await.unwrap();
    let document_id = fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap()
        .into_iter()
        .find_map(|entry| match entry {
            WorkspaceInventoryEntry::Document(entry) if entry.path.as_str() == "note.md" => {
                Some(entry.id)
            }
            _ => None,
        })
        .unwrap();

    let png = image_fixture("png");
    let created = fixture
        .service
        .create_resource(
            &document_id,
            CreateWorkspaceResourceQuery {
                workspace_generation: workspace.generation,
                folder: WorkspaceRelativePath::parse("assets").unwrap(),
                name: ResourceName::parse("pasted.png").unwrap(),
                kind: ResourceKind::Image,
            },
            "image/png",
            &png,
        )
        .await
        .unwrap();

    assert_eq!(created.path.as_str(), "assets/pasted.png");
    assert_eq!(created.parent.as_str(), "assets");
    assert_eq!(created.name.as_str(), "pasted.png");
    assert_eq!(created.kind, ResourceKind::Image);
    assert_eq!(created.media_type, "image/png");
    assert!(created.previewable);
    assert!(created.revision.as_str().starts_with("sha256:"));
    assert_eq!(
        fs::read(fixture.root.join("assets/pasted.png")).unwrap(),
        png
    );

    let mut opened = fixture
        .service
        .open_resource(&created.id, ResourceKind::Image)
        .unwrap();
    let mut bytes = Vec::new();
    opened.read_to_end(&mut bytes).unwrap();
    opened.verify_complete().unwrap();
    assert_eq!(bytes, image_fixture("png"));
}

#[tokio::test]
async fn resource_writer_uses_atomic_unique_names_without_overwriting_existing_files() {
    let fixture = Fixture::new().await;
    fs::write(fixture.root.join("note.md"), b"# Note").unwrap();
    fs::create_dir(fixture.root.join("assets")).unwrap();
    fs::write(fixture.root.join("assets/pasted.png"), b"keep existing").unwrap();
    let workspace = fixture._workspace.get_workspace().await.unwrap();
    let document_id = fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap()
        .into_iter()
        .find_map(|entry| match entry {
            WorkspaceInventoryEntry::Document(entry) if entry.path.as_str() == "note.md" => {
                Some(entry.id)
            }
            _ => None,
        })
        .unwrap();

    let png = image_fixture("png");
    let created = fixture
        .service
        .create_resource(
            &document_id,
            CreateWorkspaceResourceQuery {
                workspace_generation: workspace.generation,
                folder: WorkspaceRelativePath::parse("assets").unwrap(),
                name: ResourceName::parse("pasted.png").unwrap(),
                kind: ResourceKind::Image,
            },
            "image/png",
            &png,
        )
        .await
        .unwrap();

    assert_eq!(created.path.as_str(), "assets/pasted-2.png");
    assert_eq!(
        fs::read(fixture.root.join("assets/pasted.png")).unwrap(),
        b"keep existing"
    );
    assert_eq!(
        fs::read(fixture.root.join("assets/pasted-2.png")).unwrap(),
        png
    );
}

#[tokio::test]
async fn resource_writer_rejects_image_mime_or_magic_mismatches_without_a_partial_file() {
    let fixture = Fixture::new().await;
    fs::write(fixture.root.join("note.md"), b"# Note").unwrap();
    let workspace = fixture._workspace.get_workspace().await.unwrap();
    let document_id = fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap()
        .into_iter()
        .find_map(|entry| match entry {
            WorkspaceInventoryEntry::Document(entry) if entry.path.as_str() == "note.md" => {
                Some(entry.id)
            }
            _ => None,
        })
        .unwrap();

    let png = image_fixture("png");
    for (kind, media_type, body) in [
        (
            ResourceKind::Image,
            "application/octet-stream",
            png.as_slice(),
        ),
        (ResourceKind::Image, "image/png", b"not a png".as_slice()),
        (
            ResourceKind::Attachment,
            "application/octet-stream",
            png.as_slice(),
        ),
    ] {
        let error = fixture
            .service
            .create_resource(
                &document_id,
                CreateWorkspaceResourceQuery {
                    workspace_generation: workspace.generation.clone(),
                    folder: WorkspaceRelativePath::parse("assets").unwrap(),
                    name: ResourceName::parse("pasted.png").unwrap(),
                    kind,
                },
                media_type,
                body,
            )
            .await
            .unwrap_err();

        assert_eq!(error.kind(), ResourceServiceErrorKind::InvalidMediaType);
        assert!(!fixture.root.join("assets/pasted.png").exists());
    }
}

#[tokio::test]
async fn resource_writer_creates_document_relative_attachments_in_nested_workspaces() {
    let fixture = Fixture::new().await;
    fs::create_dir(fixture.root.join("notes")).unwrap();
    fs::write(fixture.root.join("notes/note.md"), b"# Note").unwrap();
    let workspace = fixture._workspace.get_workspace().await.unwrap();
    let document_id = fixture
        .service
        .list_inventory(&WorkspaceRelativePath::parse("notes").unwrap())
        .unwrap()
        .into_iter()
        .find_map(|entry| match entry {
            WorkspaceInventoryEntry::Document(entry) => Some(entry.id),
            WorkspaceInventoryEntry::Resource(_) => None,
        })
        .unwrap();

    let created = fixture
        .service
        .create_resource(
            &document_id,
            CreateWorkspaceResourceQuery {
                workspace_generation: workspace.generation,
                folder: WorkspaceRelativePath::parse("files/reports").unwrap(),
                name: ResourceName::parse("report.pdf").unwrap(),
                kind: ResourceKind::Attachment,
            },
            "application/octet-stream",
            b"%PDF-1.7 attachment",
        )
        .await
        .unwrap();

    assert_eq!(created.path.as_str(), "notes/files/reports/report.pdf");
    assert_eq!(created.kind, ResourceKind::Attachment);
    assert!(!created.previewable);
    assert_eq!(
        fs::read(fixture.root.join("notes/files/reports/report.pdf")).unwrap(),
        b"%PDF-1.7 attachment"
    );
}

#[tokio::test]
async fn resource_writer_rejects_stale_generations_protected_and_ignored_paths() {
    let fixture = Fixture::new().await;
    fs::write(fixture.root.join("note.md"), b"# Note").unwrap();
    fs::write(fixture.root.join("foreign.bin"), b"foreign").unwrap();
    let inventory = fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap();
    let document_id = inventory
        .iter()
        .find_map(|entry| match entry {
            WorkspaceInventoryEntry::Document(entry) => Some(entry.id.clone()),
            WorkspaceInventoryEntry::Resource(_) => None,
        })
        .unwrap();
    let foreign_id = inventory
        .iter()
        .find_map(|entry| match entry {
            WorkspaceInventoryEntry::Resource(entry) => {
                Some(DocumentId::parse(entry.id.as_str()).unwrap())
            }
            WorkspaceInventoryEntry::Document(_) => None,
        })
        .unwrap();
    let workspace = fixture._workspace.get_workspace().await.unwrap();
    let stale = fixture
        .service
        .create_resource(
            &document_id,
            CreateWorkspaceResourceQuery {
                workspace_generation: WorkspaceGeneration::parse("stale-generation").unwrap(),
                folder: WorkspaceRelativePath::parse("assets").unwrap(),
                name: ResourceName::parse("asset.bin").unwrap(),
                kind: ResourceKind::Attachment,
            },
            "application/octet-stream",
            b"asset",
        )
        .await
        .unwrap_err();
    assert_eq!(stale.kind(), ResourceServiceErrorKind::StaleWorkspace);

    let foreign = fixture
        .service
        .create_resource(
            &foreign_id,
            CreateWorkspaceResourceQuery {
                workspace_generation: workspace.generation.clone(),
                folder: WorkspaceRelativePath::parse("assets").unwrap(),
                name: ResourceName::parse("asset.bin").unwrap(),
                kind: ResourceKind::Attachment,
            },
            "application/octet-stream",
            b"asset",
        )
        .await
        .unwrap_err();
    assert_eq!(foreign.kind(), ResourceServiceErrorKind::NotFound);

    let protected = fixture
        .service
        .create_resource(
            &document_id,
            CreateWorkspaceResourceQuery {
                workspace_generation: workspace.generation.clone(),
                folder: WorkspaceRelativePath::parse(".git/assets").unwrap(),
                name: ResourceName::parse("asset.bin").unwrap(),
                kind: ResourceKind::Attachment,
            },
            "application/octet-stream",
            b"asset",
        )
        .await
        .unwrap_err();
    assert_eq!(protected.kind(), ResourceServiceErrorKind::InvalidPath);

    fixture.ignore.set_global_rules("assets/\n");
    let ignored = fixture
        .service
        .create_resource(
            &document_id,
            CreateWorkspaceResourceQuery {
                workspace_generation: workspace.generation,
                folder: WorkspaceRelativePath::parse("assets").unwrap(),
                name: ResourceName::parse("asset.bin").unwrap(),
                kind: ResourceKind::Attachment,
            },
            "application/octet-stream",
            b"asset",
        )
        .await
        .unwrap_err();
    assert_eq!(ignored.kind(), ResourceServiceErrorKind::InvalidPath);
    assert!(!fixture.root.join("assets").exists());
}

#[tokio::test]
async fn resource_writer_rejects_a_replaced_workspace_root_without_stranding_the_upload() {
    let fixture = Fixture::new().await;
    fs::write(fixture.root.join("note.md"), b"# Note").unwrap();
    let workspace = fixture._workspace.get_workspace().await.unwrap();
    let document_id = fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap()
        .into_iter()
        .find_map(|entry| match entry {
            WorkspaceInventoryEntry::Document(entry) => Some(entry.id),
            WorkspaceInventoryEntry::Resource(_) => None,
        })
        .unwrap();
    let retired = fixture.root.with_extension("retired");
    fixture
        .ignore
        .replace_root_after_capture(fixture.root.clone(), retired.clone());

    let error = fixture
        .service
        .create_resource(
            &document_id,
            CreateWorkspaceResourceQuery {
                workspace_generation: workspace.generation,
                folder: WorkspaceRelativePath::parse("assets").unwrap(),
                name: ResourceName::parse("asset.bin").unwrap(),
                kind: ResourceKind::Attachment,
            },
            "application/octet-stream",
            b"asset",
        )
        .await
        .unwrap_err();

    assert_eq!(error.kind(), ResourceServiceErrorKind::Unavailable);
    assert!(!fixture.root.join("assets").exists());
    assert!(!retired.join("assets").exists());
}

#[tokio::test]
async fn resource_writer_rolls_back_staging_and_new_directories_when_publication_fails() {
    let fixture = Fixture::new().await;
    fs::write(fixture.root.join("note.md"), b"# Note").unwrap();
    let workspace = fixture._workspace.get_workspace().await.unwrap();
    let document_id = fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap()
        .into_iter()
        .find_map(|entry| match entry {
            WorkspaceInventoryEntry::Document(entry) => Some(entry.id),
            WorkspaceInventoryEntry::Resource(_) => None,
        })
        .unwrap();
    let service = WorkspaceResourceService::new_with_atomic_install(
        &fixture.runtime,
        fixture.ignore.clone(),
        Arc::new(RejectingAtomicInstallPort),
    );
    let mut before = fs::read_dir(&fixture.root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    before.sort();

    let error = service
        .create_resource(
            &document_id,
            CreateWorkspaceResourceQuery {
                workspace_generation: workspace.generation,
                folder: WorkspaceRelativePath::parse("assets/nested").unwrap(),
                name: ResourceName::parse("asset.bin").unwrap(),
                kind: ResourceKind::Attachment,
            },
            "application/octet-stream",
            b"asset",
        )
        .await
        .unwrap_err();

    assert_eq!(error.kind(), ResourceServiceErrorKind::Unavailable);
    assert!(!fixture.root.join("assets").exists());
    let mut after = fs::read_dir(&fixture.root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    after.sort();
    assert_eq!(after, before);
}

#[tokio::test]
async fn resource_writer_settles_a_commit_unknown_publication_by_pinned_identity() {
    let fixture = Fixture::new().await;
    fs::write(fixture.root.join("note.md"), b"# Note").unwrap();
    let workspace = fixture._workspace.get_workspace().await.unwrap();
    let document_id = fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap()
        .into_iter()
        .find_map(|entry| match entry {
            WorkspaceInventoryEntry::Document(entry) => Some(entry.id),
            WorkspaceInventoryEntry::Resource(_) => None,
        })
        .unwrap();
    let service = WorkspaceResourceService::new_with_atomic_install(
        &fixture.runtime,
        fixture.ignore.clone(),
        Arc::new(InstallThenReportFailurePort),
    );

    let created = service
        .create_resource(
            &document_id,
            CreateWorkspaceResourceQuery {
                workspace_generation: workspace.generation,
                folder: WorkspaceRelativePath::parse("assets").unwrap(),
                name: ResourceName::parse("asset.bin").unwrap(),
                kind: ResourceKind::Attachment,
            },
            "application/octet-stream",
            b"asset",
        )
        .await
        .unwrap();

    assert_eq!(created.path.as_str(), "assets/asset.bin");
    assert_eq!(
        fs::read(fixture.root.join("assets/asset.bin")).unwrap(),
        b"asset"
    );
}

#[tokio::test]
async fn resource_batch_closes_the_runtime_when_a_prepared_publication_fails() {
    struct FailSecondInstall {
        calls: AtomicUsize,
    }
    impl AtomicInstallPort for FailSecondInstall {
        fn install(&self, request: AtomicInstallRequest<'_>) -> Result<(), AtomicInstallPortError> {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 1 {
                return Err(AtomicInstallPortError);
            }
            CapabilityAtomicInstallPort.install(request)
        }
    }

    let fixture = Fixture::new().await;
    fs::write(fixture.root.join("note.md"), b"# Note").unwrap();
    let workspace = fixture._workspace.get_workspace().await.unwrap();
    let document_id = fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap()
        .into_iter()
        .find_map(|entry| match entry {
            WorkspaceInventoryEntry::Document(entry) => Some(entry.id),
            WorkspaceInventoryEntry::Resource(_) => None,
        })
        .unwrap();
    let service = WorkspaceResourceService::new_with_atomic_install(
        &fixture.runtime,
        fixture.ignore.clone(),
        Arc::new(FailSecondInstall {
            calls: AtomicUsize::new(0),
        }),
    );

    let error = service
        .create_resource_batch(
            ResourceBatchId::new(Uuid::new_v4()),
            &document_id,
            workspace.generation.clone(),
            WorkspaceRelativePath::parse("assets").unwrap(),
            vec![
                CreateResourceBatchItem::image(
                    ResourceName::parse("one.png").unwrap(),
                    "image/png",
                    image_fixture("png"),
                ),
                CreateResourceBatchItem::image(
                    ResourceName::parse("two.png").unwrap(),
                    "image/png",
                    image_fixture("png"),
                ),
            ],
        )
        .await
        .unwrap_err();

    assert_eq!(error.kind(), ResourceServiceErrorKind::Unavailable);
    assert!(fixture.root.join("assets/one.png").exists());
    assert!(!fixture.root.join("assets/two.png").exists());
    assert!(fixture.runtime.active_workspace_snapshot().is_err());
}

#[tokio::test]
async fn resource_batch_preserves_request_order_and_reserves_collision_names() {
    let fixture = Fixture::new().await;
    fs::write(fixture.root.join("note.md"), b"# Note").unwrap();
    fs::create_dir(fixture.root.join("assets")).unwrap();
    fs::write(fixture.root.join("assets/picture.png"), b"existing").unwrap();
    let workspace = fixture._workspace.get_workspace().await.unwrap();
    let document_id = fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap()
        .into_iter()
        .find_map(|entry| match entry {
            WorkspaceInventoryEntry::Document(entry) if entry.kind == DocumentKind::File => {
                Some(entry.id)
            }
            _ => None,
        })
        .unwrap();

    let created = fixture
        .service
        .create_resource_batch(
            ResourceBatchId::new(Uuid::new_v4()),
            &document_id,
            workspace.generation,
            WorkspaceRelativePath::parse("assets").unwrap(),
            vec![
                CreateResourceBatchItem::image(
                    ResourceName::parse("picture.png").unwrap(),
                    "image/png",
                    image_fixture("png"),
                ),
                CreateResourceBatchItem::image(
                    ResourceName::parse("picture.png").unwrap(),
                    "image/png",
                    image_fixture("png"),
                ),
            ],
        )
        .await
        .unwrap();

    assert_eq!(
        created
            .iter()
            .map(|resource| resource.path.as_str())
            .collect::<Vec<_>>(),
        ["assets/picture-2.png", "assets/picture-3.png"]
    );
}

#[tokio::test]
async fn resource_batch_replays_the_committed_outcome_for_the_same_batch_id() {
    let fixture = Fixture::new().await;
    fs::write(fixture.root.join("note.md"), b"# Note").unwrap();
    let workspace = fixture._workspace.get_workspace().await.unwrap();
    let document_id = fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap()
        .into_iter()
        .find_map(|entry| match entry {
            WorkspaceInventoryEntry::Document(entry) if entry.kind == DocumentKind::File => {
                Some(entry.id)
            }
            _ => None,
        })
        .unwrap();
    let request = CreateWorkspaceResourceBatchRequest {
        batch_id: ResourceBatchId::new(Uuid::from_u128(0x42)),
        workspace_generation: workspace.generation,
        folder: WorkspaceRelativePath::parse("assets").unwrap(),
        items: vec![CreateWorkspaceResourceBatchItem {
            name: ResourceName::parse("picture.png").unwrap(),
            kind: ResourceKind::Image,
            media_type: "image/png".to_string(),
            body_base64: STANDARD.encode(image_fixture("png")),
        }],
    };

    let first = fixture
        .service
        .create_workspace_resource_batch(document_id.clone(), request.clone())
        .await
        .unwrap();
    let replay = fixture
        .service
        .create_workspace_resource_batch(document_id, request)
        .await
        .unwrap();

    assert_eq!(replay.resources, first.resources);
    assert_eq!(
        fs::read_dir(fixture.root.join("assets")).unwrap().count(),
        1
    );
}

#[tokio::test]
async fn resource_batch_rejects_reusing_a_batch_id_for_a_different_request() {
    let fixture = Fixture::new().await;
    fs::write(fixture.root.join("note.md"), b"# Note").unwrap();
    let workspace = fixture._workspace.get_workspace().await.unwrap();
    let document_id = first_document_id(&fixture.service);
    let batch_id = ResourceBatchId::new(Uuid::from_u128(0x43));

    fixture
        .service
        .create_resource_batch(
            batch_id,
            &document_id,
            workspace.generation.clone(),
            WorkspaceRelativePath::parse("assets").unwrap(),
            png_batch_items(&["first.png"]),
        )
        .await
        .unwrap();
    let error = fixture
        .service
        .create_resource_batch(
            batch_id,
            &document_id,
            workspace.generation,
            WorkspaceRelativePath::parse("assets").unwrap(),
            png_batch_items(&["different.png"]),
        )
        .await
        .unwrap_err();

    assert_eq!(error.kind(), ResourceServiceErrorKind::Conflict);
    assert!(fixture.runtime.active_workspace_snapshot().is_ok());
    assert_eq!(
        fs::read_dir(fixture.root.join("assets")).unwrap().count(),
        1
    );
}

#[tokio::test]
async fn resource_batch_oversized_journal_is_rejected_before_workspace_side_effects() {
    let fixture = Fixture::new().await;
    fs::write(fixture.root.join("note.md"), b"# Note").unwrap();
    let workspace = fixture._workspace.get_workspace().await.unwrap();
    let folder = WorkspaceRelativePath::parse(format!("new/{}end", "x/".repeat(8_000)))
        .expect("contract permits a long relative path");
    let items = (0..32)
        .map(|index| {
            CreateResourceBatchItem::image(
                ResourceName::parse(format!("picture-{index}.png")).unwrap(),
                "image/png",
                image_fixture("png"),
            )
        })
        .collect();

    let error = fixture
        .service
        .create_resource_batch(
            ResourceBatchId::new(Uuid::from_u128(0x432)),
            &first_document_id(&fixture.service),
            workspace.generation.clone(),
            folder,
            items,
        )
        .await
        .unwrap_err();

    assert_eq!(error.kind(), ResourceServiceErrorKind::TooLarge);
    assert!(fixture.runtime.active_workspace_snapshot().is_ok());
    assert!(!fixture.root.join("new").exists());
    let app_data = fixture.root.parent().unwrap().join("app-data");
    let state = batch_state_directory(&app_data, &workspace);
    assert_eq!(fs::read_dir(state).unwrap().count(), 0);
}

#[tokio::test]
async fn resource_batch_replay_rejects_a_malformed_standalone_receipt() {
    let fixture = Fixture::new().await;
    fs::write(fixture.root.join("note.md"), b"# Note").unwrap();
    let workspace = fixture._workspace.get_workspace().await.unwrap();
    let document_id = first_document_id(&fixture.service);
    let batch_id = ResourceBatchId::new(Uuid::from_u128(0x431));
    fixture
        .service
        .create_resource_batch(
            batch_id,
            &document_id,
            workspace.generation.clone(),
            WorkspaceRelativePath::parse("assets").unwrap(),
            png_batch_items(&["picture.png"]),
        )
        .await
        .unwrap();
    let app_data = fixture.root.parent().unwrap().join("app-data");
    let state = batch_state_directory(&app_data, &workspace);
    let receipt = batch_record_path(&state, "resource-batch-receipt-v1-");
    let mut value: Value = serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
    value["unexpected"] = Value::Bool(true);
    fs::write(receipt, serde_json::to_vec(&value).unwrap()).unwrap();

    let error = fixture
        .service
        .create_resource_batch(
            batch_id,
            &document_id,
            workspace.generation,
            WorkspaceRelativePath::parse("assets").unwrap(),
            png_batch_items(&["picture.png"]),
        )
        .await
        .unwrap_err();

    assert_eq!(error.kind(), ResourceServiceErrorKind::Unavailable);
    assert!(fixture.runtime.active_workspace_snapshot().is_err());
    assert!(fixture.root.join("assets/picture.png").exists());
}

#[tokio::test]
async fn resource_batch_replays_a_durable_receipt_after_a_real_new_launch() {
    let temporary = tempdir().unwrap();
    let root = temporary.path().join("workspace");
    let app_data = temporary.path().join("app-data");
    let cache = temporary.path().join("cache");
    for path in [&root, &app_data, &cache] {
        fs::create_dir(path).unwrap();
    }
    fs::write(root.join("note.md"), b"# Note").unwrap();
    let batch_id = ResourceBatchId::new(Uuid::from_u128(0x44));
    let workspace_store = Arc::new(MemoryWorkspaceStore::default());

    let (first_runtime, first_workspace, first_ignore) =
        activate_resources_at(&root, &app_data, &cache, Arc::clone(&workspace_store)).await;
    let first_service = WorkspaceResourceService::new(&first_runtime, first_ignore);
    let first_snapshot = first_workspace.get_workspace().await.unwrap();
    let first = first_service
        .create_resource_batch(
            batch_id,
            &first_document_id(&first_service),
            first_snapshot.generation.clone(),
            WorkspaceRelativePath::parse("assets").unwrap(),
            png_batch_items(&["picture.png"]),
        )
        .await
        .unwrap();
    let first_id = first[0].id.clone();
    let first_path = first[0].path.clone();
    drop(first_service);
    drop(first_workspace);
    drop(first_runtime);

    let (second_runtime, second_workspace, second_ignore) =
        activate_resources_at(&root, &app_data, &cache, workspace_store).await;
    let second_service = WorkspaceResourceService::new(&second_runtime, second_ignore);
    second_service.recover_pending().await.unwrap();
    let second_snapshot = second_workspace.get_workspace().await.unwrap();
    let replay = second_service
        .create_resource_batch(
            batch_id,
            &first_document_id(&second_service),
            second_snapshot.generation,
            WorkspaceRelativePath::parse("assets").unwrap(),
            png_batch_items(&["picture.png"]),
        )
        .await
        .unwrap();

    assert_eq!(replay[0].path, first_path);
    assert_ne!(replay[0].id, first_id, "new launch must re-sign identities");
    assert!(second_service
        .open_resource(&first_id, ResourceKind::Image)
        .is_err());
    assert_eq!(fs::read_dir(root.join("assets")).unwrap().count(), 1);
}

#[tokio::test]
async fn resource_batch_replay_closes_the_runtime_when_a_committed_target_is_altered() {
    let fixture = Fixture::new().await;
    fs::write(fixture.root.join("note.md"), b"# Note").unwrap();
    let workspace = fixture._workspace.get_workspace().await.unwrap();
    let document_id = first_document_id(&fixture.service);
    let batch_id = ResourceBatchId::new(Uuid::from_u128(0x45));
    fixture
        .service
        .create_resource_batch(
            batch_id,
            &document_id,
            workspace.generation.clone(),
            WorkspaceRelativePath::parse("assets").unwrap(),
            png_batch_items(&["picture.png"]),
        )
        .await
        .unwrap();
    fs::write(fixture.root.join("assets/picture.png"), b"tampered").unwrap();

    let error = fixture
        .service
        .create_resource_batch(
            batch_id,
            &document_id,
            workspace.generation,
            WorkspaceRelativePath::parse("assets").unwrap(),
            png_batch_items(&["picture.png"]),
        )
        .await
        .unwrap_err();

    assert_eq!(error.kind(), ResourceServiceErrorKind::UnsafeTarget);
    assert!(fixture.runtime.active_workspace_snapshot().is_err());
    assert_eq!(
        fs::read(fixture.root.join("assets/picture.png")).unwrap(),
        b"tampered"
    );
}

#[tokio::test]
async fn resource_batch_replay_closes_the_runtime_when_a_committed_target_is_missing() {
    let fixture = Fixture::new().await;
    fs::write(fixture.root.join("note.md"), b"# Note").unwrap();
    let workspace = fixture._workspace.get_workspace().await.unwrap();
    let document_id = first_document_id(&fixture.service);
    let batch_id = ResourceBatchId::new(Uuid::from_u128(0x46));
    fixture
        .service
        .create_resource_batch(
            batch_id,
            &document_id,
            workspace.generation.clone(),
            WorkspaceRelativePath::parse("assets").unwrap(),
            png_batch_items(&["picture.png"]),
        )
        .await
        .unwrap();
    fs::remove_file(fixture.root.join("assets/picture.png")).unwrap();

    let error = fixture
        .service
        .create_resource_batch(
            batch_id,
            &document_id,
            workspace.generation,
            WorkspaceRelativePath::parse("assets").unwrap(),
            png_batch_items(&["picture.png"]),
        )
        .await
        .unwrap_err();

    assert_eq!(error.kind(), ResourceServiceErrorKind::Unavailable);
    assert!(fixture.runtime.active_workspace_snapshot().is_err());
    assert!(!fixture.root.join("assets/picture.png").exists());
}

#[tokio::test]
async fn resource_batch_restart_rolls_forward_a_prepared_partial_publication() {
    struct FailSecondInstall {
        calls: AtomicUsize,
    }
    impl AtomicInstallPort for FailSecondInstall {
        fn install(&self, request: AtomicInstallRequest<'_>) -> Result<(), AtomicInstallPortError> {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 1 {
                return Err(AtomicInstallPortError);
            }
            CapabilityAtomicInstallPort.install(request)
        }
    }

    let temporary = tempdir().unwrap();
    let root = temporary.path().join("workspace");
    let app_data = temporary.path().join("app-data");
    let cache = temporary.path().join("cache");
    for path in [&root, &app_data, &cache] {
        fs::create_dir(path).unwrap();
    }
    fs::write(root.join("note.md"), b"# Note").unwrap();
    let batch_id = ResourceBatchId::new(Uuid::from_u128(0xfeed));
    let workspace_store = Arc::new(MemoryWorkspaceStore::default());

    let (first_runtime, first_workspace, first_ignore) =
        activate_resources_at(&root, &app_data, &cache, Arc::clone(&workspace_store)).await;
    let first_service = WorkspaceResourceService::open_with_atomic_install(
        &first_runtime,
        first_ignore,
        Arc::new(FailSecondInstall {
            calls: AtomicUsize::new(0),
        }),
    )
    .unwrap();
    let first_snapshot = first_workspace.get_workspace().await.unwrap();
    let first_document = first_service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap()
        .into_iter()
        .find_map(|entry| match entry {
            WorkspaceInventoryEntry::Document(entry) if entry.kind == DocumentKind::File => {
                Some(entry.id)
            }
            _ => None,
        })
        .unwrap();
    let error = first_service
        .create_resource_batch(
            batch_id,
            &first_document,
            first_snapshot.generation,
            WorkspaceRelativePath::parse("assets").unwrap(),
            vec![
                CreateResourceBatchItem::image(
                    ResourceName::parse("one.png").unwrap(),
                    "image/png",
                    image_fixture("png"),
                ),
                CreateResourceBatchItem::image(
                    ResourceName::parse("two.png").unwrap(),
                    "image/png",
                    image_fixture("png"),
                ),
            ],
        )
        .await
        .unwrap_err();
    assert_eq!(error.kind(), ResourceServiceErrorKind::Unavailable);
    assert!(root.join("assets/one.png").exists());
    assert!(!root.join("assets/two.png").exists());
    drop(first_service);
    drop(first_workspace);
    drop(first_runtime);

    let (second_runtime, second_workspace, second_ignore) =
        activate_resources_at(&root, &app_data, &cache, workspace_store).await;
    let second_service = WorkspaceResourceService::new(&second_runtime, second_ignore);
    second_service.recover_pending().await.unwrap();
    assert_eq!(
        fs::read(root.join("assets/one.png")).unwrap(),
        image_fixture("png")
    );
    assert_eq!(
        fs::read(root.join("assets/two.png")).unwrap(),
        image_fixture("png")
    );
    let second_snapshot = second_workspace.get_workspace().await.unwrap();
    let second_document = second_service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap()
        .into_iter()
        .find_map(|entry| match entry {
            WorkspaceInventoryEntry::Document(entry) if entry.kind == DocumentKind::File => {
                Some(entry.id)
            }
            _ => None,
        })
        .unwrap();
    let replay = second_service
        .create_resource_batch(
            batch_id,
            &second_document,
            second_snapshot.generation,
            WorkspaceRelativePath::parse("assets").unwrap(),
            vec![
                CreateResourceBatchItem::image(
                    ResourceName::parse("one.png").unwrap(),
                    "image/png",
                    image_fixture("png"),
                ),
                CreateResourceBatchItem::image(
                    ResourceName::parse("two.png").unwrap(),
                    "image/png",
                    image_fixture("png"),
                ),
            ],
        )
        .await
        .unwrap();
    assert_eq!(replay.len(), 2);
    assert_eq!(fs::read_dir(root.join("assets")).unwrap().count(), 2);
}

#[tokio::test]
async fn resource_batch_restart_rolls_forward_from_each_rename_boundary() {
    struct FailAtInstall {
        fail_at: usize,
        calls: AtomicUsize,
    }
    impl AtomicInstallPort for FailAtInstall {
        fn install(&self, request: AtomicInstallRequest<'_>) -> Result<(), AtomicInstallPortError> {
            if self.calls.fetch_add(1, Ordering::SeqCst) == self.fail_at {
                return Err(AtomicInstallPortError);
            }
            CapabilityAtomicInstallPort.install(request)
        }
    }

    for fail_at in 0..3 {
        let temporary = tempdir().unwrap();
        let root = temporary.path().join("workspace");
        let app_data = temporary.path().join("app-data");
        let cache = temporary.path().join("cache");
        for path in [&root, &app_data, &cache] {
            fs::create_dir(path).unwrap();
        }
        fs::write(root.join("note.md"), b"# Note").unwrap();
        let workspace_store = Arc::new(MemoryWorkspaceStore::default());
        let (runtime, workspace, ignore) =
            activate_resources_at(&root, &app_data, &cache, Arc::clone(&workspace_store)).await;
        let service = WorkspaceResourceService::open_with_atomic_install(
            &runtime,
            ignore,
            Arc::new(FailAtInstall {
                fail_at,
                calls: AtomicUsize::new(0),
            }),
        )
        .unwrap();
        let snapshot = workspace.get_workspace().await.unwrap();
        let error = service
            .create_resource_batch(
                ResourceBatchId::new(Uuid::from_u128(0x0fee_d100 + fail_at as u128)),
                &first_document_id(&service),
                snapshot.generation,
                WorkspaceRelativePath::parse("assets").unwrap(),
                png_batch_items(&["one.png", "two.png", "three.png"]),
            )
            .await
            .unwrap_err();
        assert_eq!(error.kind(), ResourceServiceErrorKind::Unavailable);
        drop(service);
        drop(workspace);
        drop(runtime);

        let (runtime, _workspace, ignore) =
            activate_resources_at(&root, &app_data, &cache, workspace_store).await;
        let service = WorkspaceResourceService::new(&runtime, ignore);
        service.recover_pending().await.unwrap();

        for name in ["one.png", "two.png", "three.png"] {
            assert_eq!(
                fs::read(root.join("assets").join(name)).unwrap(),
                image_fixture("png"),
                "fail_at={fail_at}, name={name}"
            );
        }
        assert!(fs::read_dir(root.join("assets")).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".qingyu-resource-batch-")
        }));
        assert!(runtime.active_workspace_snapshot().is_ok());
    }
}

#[tokio::test]
async fn resource_batch_restart_aborts_preparing_and_removes_its_exact_stage() {
    let temporary = tempdir().unwrap();
    let root = temporary.path().join("workspace");
    let app_data = temporary.path().join("app-data");
    let cache = temporary.path().join("cache");
    for path in [&root, &app_data, &cache] {
        fs::create_dir(path).unwrap();
    }
    fs::write(root.join("note.md"), b"# Note").unwrap();
    let workspace_store = Arc::new(MemoryWorkspaceStore::default());
    let (runtime, workspace, ignore) =
        activate_resources_at(&root, &app_data, &cache, Arc::clone(&workspace_store)).await;
    let service = WorkspaceResourceService::new(&runtime, ignore);
    let snapshot = workspace.get_workspace().await.unwrap();
    service
        .create_resource_batch(
            ResourceBatchId::new(Uuid::from_u128(0x47)),
            &first_document_id(&service),
            snapshot.generation.clone(),
            WorkspaceRelativePath::parse("assets").unwrap(),
            png_batch_items(&["picture.png"]),
        )
        .await
        .unwrap();
    let state = batch_state_directory(&app_data, &snapshot);
    let receipt = batch_record_path(&state, "resource-batch-receipt-v1-");
    let mut record: Value = serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
    record["phase"] = Value::String("preparing".to_string());
    let stage_name = record["items"][0]["stageName"]
        .as_str()
        .unwrap()
        .to_string();
    let pending = receipt.with_file_name(
        receipt
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .replace("resource-batch-receipt-v1-", "resource-batch-pending-v1-"),
    );
    fs::rename(&receipt, &pending).unwrap();
    fs::write(&pending, serde_json::to_vec(&record).unwrap()).unwrap();
    fs::rename(
        root.join("assets/picture.png"),
        root.join("assets").join(&stage_name),
    )
    .unwrap();
    drop(service);
    drop(workspace);
    drop(runtime);

    let (runtime, _workspace, ignore) =
        activate_resources_at(&root, &app_data, &cache, workspace_store).await;
    let service = WorkspaceResourceService::new(&runtime, ignore);
    service.recover_pending().await.unwrap();

    assert!(!root.join("assets").join(stage_name).exists());
    assert!(!root.join("assets/picture.png").exists());
    assert!(
        !pending.exists(),
        "durable Aborted marker must be garbage-collected"
    );
    assert!(runtime.active_workspace_snapshot().is_ok());
}

#[tokio::test]
async fn resource_batch_restart_finalizes_aborted_without_touching_a_conflicting_target() {
    let temporary = tempdir().unwrap();
    let root = temporary.path().join("workspace");
    let app_data = temporary.path().join("app-data");
    let cache = temporary.path().join("cache");
    for path in [&root, &app_data, &cache] {
        fs::create_dir(path).unwrap();
    }
    fs::write(root.join("note.md"), b"# Note").unwrap();
    let workspace_store = Arc::new(MemoryWorkspaceStore::default());
    let (runtime, workspace, ignore) =
        activate_resources_at(&root, &app_data, &cache, Arc::clone(&workspace_store)).await;
    let service = WorkspaceResourceService::new(&runtime, ignore);
    let snapshot = workspace.get_workspace().await.unwrap();
    service
        .create_resource_batch(
            ResourceBatchId::new(Uuid::from_u128(0x471)),
            &first_document_id(&service),
            snapshot.generation.clone(),
            WorkspaceRelativePath::parse("assets").unwrap(),
            png_batch_items(&["picture.png"]),
        )
        .await
        .unwrap();
    let state = batch_state_directory(&app_data, &snapshot);
    let receipt = batch_record_path(&state, "resource-batch-receipt-v1-");
    let mut record: Value = serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
    record["phase"] = Value::String("aborted".to_string());
    let pending = receipt.with_file_name(
        receipt
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .replace("resource-batch-receipt-v1-", "resource-batch-pending-v1-"),
    );
    fs::remove_file(receipt).unwrap();
    fs::write(&pending, serde_json::to_vec(&record).unwrap()).unwrap();
    fs::write(root.join("assets/picture.png"), b"external-target").unwrap();
    drop(service);
    drop(workspace);
    drop(runtime);

    let (runtime, _workspace, ignore) =
        activate_resources_at(&root, &app_data, &cache, workspace_store).await;
    let service = WorkspaceResourceService::new(&runtime, ignore);
    service.recover_pending().await.unwrap();

    assert_eq!(
        fs::read(root.join("assets/picture.png")).unwrap(),
        b"external-target"
    );
    assert!(!pending.exists());
    assert!(runtime.active_workspace_snapshot().is_ok());
}

#[cfg(unix)]
#[tokio::test]
async fn resource_batch_preparing_cleanup_failure_closes_the_new_launch() {
    use std::os::unix::fs::PermissionsExt as _;

    let temporary = tempdir().unwrap();
    let root = temporary.path().join("workspace");
    let app_data = temporary.path().join("app-data");
    let cache = temporary.path().join("cache");
    for path in [&root, &app_data, &cache] {
        fs::create_dir(path).unwrap();
    }
    fs::write(root.join("note.md"), b"# Note").unwrap();
    let workspace_store = Arc::new(MemoryWorkspaceStore::default());
    let (runtime, workspace, ignore) =
        activate_resources_at(&root, &app_data, &cache, Arc::clone(&workspace_store)).await;
    let service = WorkspaceResourceService::new(&runtime, ignore);
    let snapshot = workspace.get_workspace().await.unwrap();
    service
        .create_resource_batch(
            ResourceBatchId::new(Uuid::from_u128(0x48)),
            &first_document_id(&service),
            snapshot.generation.clone(),
            WorkspaceRelativePath::parse("assets").unwrap(),
            png_batch_items(&["picture.png"]),
        )
        .await
        .unwrap();
    let state = batch_state_directory(&app_data, &snapshot);
    let receipt = batch_record_path(&state, "resource-batch-receipt-v1-");
    let mut record: Value = serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
    record["phase"] = Value::String("preparing".to_string());
    let stage_name = record["items"][0]["stageName"]
        .as_str()
        .unwrap()
        .to_string();
    let pending = receipt.with_file_name(
        receipt
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .replace("resource-batch-receipt-v1-", "resource-batch-pending-v1-"),
    );
    fs::rename(&receipt, &pending).unwrap();
    fs::write(&pending, serde_json::to_vec(&record).unwrap()).unwrap();
    fs::rename(
        root.join("assets/picture.png"),
        root.join("assets").join(&stage_name),
    )
    .unwrap();
    let mut read_only = fs::metadata(root.join("assets")).unwrap().permissions();
    read_only.set_mode(0o500);
    fs::set_permissions(root.join("assets"), read_only).unwrap();
    drop(service);
    drop(workspace);
    drop(runtime);

    let (runtime, _workspace, ignore) =
        activate_resources_at(&root, &app_data, &cache, workspace_store).await;
    let service = WorkspaceResourceService::new(&runtime, ignore);
    let result = service.recover_pending().await;
    let mut restored = fs::metadata(root.join("assets")).unwrap().permissions();
    restored.set_mode(0o700);
    fs::set_permissions(root.join("assets"), restored).unwrap();

    assert_eq!(
        result.unwrap_err().kind(),
        ResourceServiceErrorKind::Unavailable
    );
    assert!(root.join("assets").join(stage_name).exists());
    assert!(runtime.active_workspace_snapshot().is_err());
}

#[tokio::test]
async fn resource_batch_startup_rejects_malformed_or_misaddressed_records() {
    for scenario in [
        "unknown-field",
        "oversized",
        "filename-id",
        "generation",
        "multiple",
    ] {
        let temporary = tempdir().unwrap();
        let root = temporary.path().join("workspace");
        let app_data = temporary.path().join("app-data");
        let cache = temporary.path().join("cache");
        for path in [&root, &app_data, &cache] {
            fs::create_dir(path).unwrap();
        }
        fs::write(root.join("note.md"), b"# Note").unwrap();
        let workspace_store = Arc::new(MemoryWorkspaceStore::default());
        let (runtime, workspace, ignore) =
            activate_resources_at(&root, &app_data, &cache, Arc::clone(&workspace_store)).await;
        let service = WorkspaceResourceService::new(&runtime, ignore);
        let snapshot = workspace.get_workspace().await.unwrap();
        service
            .create_resource_batch(
                ResourceBatchId::new(Uuid::from_u128(0x50)),
                &first_document_id(&service),
                snapshot.generation.clone(),
                WorkspaceRelativePath::parse("assets").unwrap(),
                png_batch_items(&["picture.png"]),
            )
            .await
            .unwrap();
        let state = batch_state_directory(&app_data, &snapshot);
        let receipt = batch_record_path(&state, "resource-batch-receipt-v1-");
        let pending = receipt.with_file_name(
            receipt
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .replace("resource-batch-receipt-v1-", "resource-batch-pending-v1-"),
        );
        let mut value: Value = serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
        value["phase"] = Value::String("prepared".to_string());
        fs::remove_file(&receipt).unwrap();
        fs::write(&pending, serde_json::to_vec(&value).unwrap()).unwrap();
        match scenario {
            "unknown-field" => {
                value["unexpected"] = Value::Bool(true);
                fs::write(&pending, serde_json::to_vec(&value).unwrap()).unwrap();
            }
            "oversized" => fs::write(&pending, vec![b'x'; 128 * 1024 + 1]).unwrap(),
            "filename-id" => {
                fs::rename(
                    &pending,
                    state.join(format!(
                        "resource-batch-pending-v1-{}.json",
                        Uuid::from_u128(0x51)
                    )),
                )
                .unwrap();
            }
            "generation" => {
                value["workspaceGeneration"] = Value::String("tampered-generation".to_string());
                fs::write(&pending, serde_json::to_vec(&value).unwrap()).unwrap();
            }
            "multiple" => {
                let second_id = Uuid::from_u128(0x53);
                value["batchId"] = Value::String(second_id.to_string());
                fs::write(
                    state.join(format!("resource-batch-pending-v1-{second_id}.json")),
                    serde_json::to_vec(&value).unwrap(),
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        drop(service);
        drop(workspace);
        drop(runtime);

        let (runtime, _workspace, ignore) =
            activate_resources_at(&root, &app_data, &cache, workspace_store).await;
        let service = WorkspaceResourceService::new(&runtime, ignore);
        let error = service.recover_pending().await.unwrap_err();

        assert_eq!(
            error.kind(),
            ResourceServiceErrorKind::Unavailable,
            "{scenario}"
        );
        assert!(runtime.active_workspace_snapshot().is_err(), "{scenario}");
    }
}

#[tokio::test]
async fn resource_batch_startup_rejects_a_tampered_or_nonregular_prepared_stage() {
    struct FailSecondInstall {
        calls: AtomicUsize,
    }
    impl AtomicInstallPort for FailSecondInstall {
        fn install(&self, request: AtomicInstallRequest<'_>) -> Result<(), AtomicInstallPortError> {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 1 {
                return Err(AtomicInstallPortError);
            }
            CapabilityAtomicInstallPort.install(request)
        }
    }

    for scenario in ["hash", "nonregular"] {
        let temporary = tempdir().unwrap();
        let root = temporary.path().join("workspace");
        let app_data = temporary.path().join("app-data");
        let cache = temporary.path().join("cache");
        for path in [&root, &app_data, &cache] {
            fs::create_dir(path).unwrap();
        }
        fs::write(root.join("note.md"), b"# Note").unwrap();
        let workspace_store = Arc::new(MemoryWorkspaceStore::default());
        let (runtime, workspace, ignore) =
            activate_resources_at(&root, &app_data, &cache, Arc::clone(&workspace_store)).await;
        let service = WorkspaceResourceService::open_with_atomic_install(
            &runtime,
            ignore,
            Arc::new(FailSecondInstall {
                calls: AtomicUsize::new(0),
            }),
        )
        .unwrap();
        let snapshot = workspace.get_workspace().await.unwrap();
        let error = service
            .create_resource_batch(
                ResourceBatchId::new(Uuid::from_u128(0x52)),
                &first_document_id(&service),
                snapshot.generation,
                WorkspaceRelativePath::parse("assets").unwrap(),
                png_batch_items(&["one.png", "two.png"]),
            )
            .await
            .unwrap_err();
        assert_eq!(error.kind(), ResourceServiceErrorKind::Unavailable);
        let stage = fs::read_dir(root.join("assets"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name.starts_with(".qingyu-resource-batch-") && name.ends_with(".tmp")
                    })
            })
            .unwrap();
        match scenario {
            "hash" => fs::write(&stage, b"tampered").unwrap(),
            "nonregular" => {
                fs::remove_file(&stage).unwrap();
                fs::create_dir(&stage).unwrap();
            }
            _ => unreachable!(),
        }
        drop(service);
        drop(workspace);
        drop(runtime);

        let (runtime, _workspace, ignore) =
            activate_resources_at(&root, &app_data, &cache, workspace_store).await;
        let service = WorkspaceResourceService::new(&runtime, ignore);
        let error = service.recover_pending().await.unwrap_err();

        assert!(matches!(
            error.kind(),
            ResourceServiceErrorKind::UnsafeTarget | ResourceServiceErrorKind::Unavailable
        ));
        assert!(stage.exists(), "{scenario}");
        assert!(runtime.active_workspace_snapshot().is_err(), "{scenario}");
    }
}

#[tokio::test]
async fn resource_batch_settles_each_commit_unknown_install() {
    let fixture = Fixture::new().await;
    fs::write(fixture.root.join("note.md"), b"# Note").unwrap();
    let workspace = fixture._workspace.get_workspace().await.unwrap();
    let document_id = fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap()
        .into_iter()
        .find_map(|entry| match entry {
            WorkspaceInventoryEntry::Document(entry) => Some(entry.id),
            WorkspaceInventoryEntry::Resource(_) => None,
        })
        .unwrap();
    let service = WorkspaceResourceService::new_with_atomic_install(
        &fixture.runtime,
        fixture.ignore.clone(),
        Arc::new(InstallThenReportFailurePort),
    );

    let created = service
        .create_resource_batch(
            ResourceBatchId::new(Uuid::new_v4()),
            &document_id,
            workspace.generation,
            WorkspaceRelativePath::parse("assets").unwrap(),
            vec![
                CreateResourceBatchItem::image(
                    ResourceName::parse("one.png").unwrap(),
                    "image/png",
                    image_fixture("png"),
                ),
                CreateResourceBatchItem::image(
                    ResourceName::parse("two.png").unwrap(),
                    "image/png",
                    image_fixture("png"),
                ),
            ],
        )
        .await
        .unwrap();

    assert_eq!(created.len(), 2);
    assert!(fixture.root.join("assets/one.png").exists());
    assert!(fixture.root.join("assets/two.png").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resource_batch_hides_intermediate_publication_from_inventory_readers() {
    use std::{sync::mpsc, thread};

    struct PauseSecondInstall {
        condition: Condvar,
        state: Mutex<(usize, bool)>,
    }
    impl AtomicInstallPort for PauseSecondInstall {
        fn install(&self, request: AtomicInstallRequest<'_>) -> Result<(), AtomicInstallPortError> {
            let call = {
                let mut state = self.state.lock().unwrap();
                let call = state.0;
                state.0 += 1;
                if call == 1 {
                    self.condition.notify_all();
                    while !state.1 {
                        state = self.condition.wait(state).unwrap();
                    }
                }
                call
            };
            assert!(call < 2);
            CapabilityAtomicInstallPort.install(request)
        }
    }

    let fixture = Fixture::new().await;
    fs::write(fixture.root.join("note.md"), b"# Note").unwrap();
    let workspace = fixture._workspace.get_workspace().await.unwrap();
    let document_id = fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap()
        .into_iter()
        .find_map(|entry| match entry {
            WorkspaceInventoryEntry::Document(entry) => Some(entry.id),
            WorkspaceInventoryEntry::Resource(_) => None,
        })
        .unwrap();
    let pause = Arc::new(PauseSecondInstall {
        condition: Condvar::new(),
        state: Mutex::new((0, false)),
    });
    let service = WorkspaceResourceService::new_with_atomic_install(
        &fixture.runtime,
        fixture.ignore.clone(),
        pause.clone(),
    );
    let documents = Arc::new(WorkspaceDocumentService::new(
        &fixture.runtime,
        Arc::new(UnusedDeletionPort),
        Arc::new(MemoryDocumentHistoryStore::default()),
    ));
    let writer_service = service.clone();
    let writing = tokio::spawn(async move {
        writer_service
            .create_resource_batch(
                ResourceBatchId::new(Uuid::new_v4()),
                &document_id,
                workspace.generation,
                WorkspaceRelativePath::parse("assets").unwrap(),
                vec![
                    CreateResourceBatchItem::image(
                        ResourceName::parse("one.png").unwrap(),
                        "image/png",
                        image_fixture("png"),
                    ),
                    CreateResourceBatchItem::image(
                        ResourceName::parse("two.png").unwrap(),
                        "image/png",
                        image_fixture("png"),
                    ),
                ],
            )
            .await
    });
    let wait_port = pause.clone();
    tokio::task::spawn_blocking(move || {
        let mut state = wait_port.state.lock().unwrap();
        while state.0 < 2 {
            state = wait_port.condition.wait(state).unwrap();
        }
    })
    .await
    .unwrap();

    let (sender, receiver) = mpsc::channel();
    let reader_service = service.clone();
    let reader = thread::spawn(move || {
        sender
            .send(reader_service.list_inventory(&WorkspaceRelativePath::parse("assets").unwrap()))
            .unwrap();
    });
    assert!(receiver.recv_timeout(Duration::from_millis(200)).is_err());
    let (document_sender, document_receiver) = mpsc::channel();
    let document_reader = thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        document_sender
            .send(
                runtime.block_on(documents.list_documents(ListDocumentsQuery {
                    cursor: None,
                    limit: None,
                    parent: WorkspaceRelativePath::default(),
                })),
            )
            .unwrap();
    });
    assert!(document_receiver
        .recv_timeout(Duration::from_millis(200))
        .is_err());
    {
        let mut state = pause.state.lock().unwrap();
        state.1 = true;
        pause.condition.notify_all();
    }
    writing.await.unwrap().unwrap();
    let inventory = receiver
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    reader.join().unwrap();
    let documents = document_receiver
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    document_reader.join().unwrap();
    assert_eq!(inventory.len(), 2);
    assert_eq!(documents.items.len(), 2);
}

#[tokio::test]
async fn concurrent_resource_writes_preserve_both_bodies_under_unique_names() {
    let fixture = Fixture::new().await;
    fs::write(fixture.root.join("note.md"), b"# Note").unwrap();
    let workspace = fixture._workspace.get_workspace().await.unwrap();
    let document_id = fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap()
        .into_iter()
        .find_map(|entry| match entry {
            WorkspaceInventoryEntry::Document(entry) => Some(entry.id),
            WorkspaceInventoryEntry::Resource(_) => None,
        })
        .unwrap();
    let query = CreateWorkspaceResourceQuery {
        workspace_generation: workspace.generation,
        folder: WorkspaceRelativePath::parse("assets").unwrap(),
        name: ResourceName::parse("asset.bin").unwrap(),
        kind: ResourceKind::Attachment,
    };

    let (first, second) = tokio::join!(
        fixture.service.create_resource(
            &document_id,
            query.clone(),
            "application/octet-stream",
            b"first",
        ),
        fixture
            .service
            .create_resource(&document_id, query, "application/octet-stream", b"second",),
    );
    let mut paths =
        [first.unwrap().path, second.unwrap().path].map(|path| path.as_str().to_string());
    paths.sort();

    assert_eq!(paths, ["assets/asset-2.bin", "assets/asset.bin"]);
    let mut bodies = [
        fs::read(fixture.root.join("assets/asset.bin")).unwrap(),
        fs::read(fixture.root.join("assets/asset-2.bin")).unwrap(),
    ];
    bodies.sort();
    assert_eq!(bodies, [b"first".to_vec(), b"second".to_vec()]);
}

#[tokio::test]
async fn inventory_scopes_paths_and_suggested_names_to_the_requested_parent() {
    let fixture = Fixture::new().await;
    fs::create_dir_all(fixture.root.join("notes/assets/nested")).unwrap();
    fs::write(fixture.root.join("notes/assets/file.bin"), b"resource").unwrap();
    fs::write(
        fixture.root.join("notes/assets/nested/descendant.bin"),
        b"nested",
    )
    .unwrap();
    let parent = WorkspaceRelativePath::parse("notes/assets").unwrap();

    let inventory = fixture.service.list_inventory(&parent).unwrap();

    assert_eq!(inventory.len(), 2);
    let resource = inventory
        .iter()
        .find_map(|entry| match entry {
            WorkspaceInventoryEntry::Resource(entry) => Some(entry),
            WorkspaceInventoryEntry::Document(_) => None,
        })
        .unwrap();
    assert_eq!(resource.path.as_str(), "notes/assets/file.bin");
    assert_eq!(resource.parent, parent);
    assert_eq!(resource.name.as_str(), "file.bin");
}

#[tokio::test]
async fn image_preview_requires_a_matching_allowed_extension_and_magic_signature() {
    let fixture = Fixture::new().await;
    fs::write(fixture.root.join("photo.JPEG"), image_fixture("jpg")).unwrap();
    fs::write(fixture.root.join("animation.gif"), image_fixture("gif")).unwrap();
    fs::write(fixture.root.join("picture.webp"), image_fixture("webp")).unwrap();
    fs::write(fixture.root.join("disguised.bin"), image_fixture("png")).unwrap();

    let inventory = fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap();
    let by_path = inventory
        .into_iter()
        .map(|entry| (entry.path().as_str().to_string(), entry))
        .collect::<BTreeMap<_, _>>();

    for (path, media_type) in [
        ("photo.JPEG", "image/jpeg"),
        ("animation.gif", "image/gif"),
        ("picture.webp", "image/webp"),
    ] {
        assert!(matches!(
            by_path.get(path).unwrap(),
            WorkspaceInventoryEntry::Resource(entry)
                if entry.kind == ResourceKind::Image
                    && entry.media_type == media_type
                    && entry.previewable
        ));
    }
    assert!(matches!(
        by_path.get("disguised.bin").unwrap(),
        WorkspaceInventoryEntry::Resource(entry)
            if entry.kind == ResourceKind::Attachment
                && entry.media_type == "application/octet-stream"
                && !entry.previewable
    ));
}

#[tokio::test]
async fn resource_writer_accepts_the_mobile_image_contract_after_authoritative_content_validation()
{
    let fixture = Fixture::new().await;
    fs::write(fixture.root.join("note.md"), b"# Note").unwrap();
    let workspace = fixture._workspace.get_workspace().await.unwrap();
    let document_id = fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap()
        .into_iter()
        .find_map(|entry| match entry {
            WorkspaceInventoryEntry::Document(entry) => Some(entry.id),
            WorkspaceInventoryEntry::Resource(_) => None,
        })
        .unwrap();
    let cases = vec![
        ("picture.avif", "image/avif", image_fixture("avif")),
        ("picture.bmp", "image/bmp", image_fixture("bmp")),
        ("picture.gif", "image/gif", image_fixture("gif")),
        ("picture.jpg", "image/jpeg", image_fixture("jpg")),
        ("picture.png", "image/png", image_fixture("png")),
        ("picture.webp", "image/webp", image_fixture("webp")),
        (
            "picture.svg",
            "image/svg+xml",
            br##"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="10" height="10" fill="#123456"/></svg>"##.to_vec(),
        ),
    ];

    for (name, media_type, body) in cases {
        let created = fixture
            .service
            .create_resource(
                &document_id,
                CreateWorkspaceResourceQuery {
                    workspace_generation: workspace.generation.clone(),
                    folder: WorkspaceRelativePath::parse("assets").unwrap(),
                    name: ResourceName::parse(name).unwrap(),
                    kind: ResourceKind::Image,
                },
                media_type,
                &body,
            )
            .await
            .unwrap_or_else(|error| panic!("{name}: {error:?}"));

        assert_eq!(created.media_type, media_type, "{name}");
        assert!(created.previewable, "{name}");
        let stored = fs::read(fixture.root.join(created.path.as_str())).unwrap();
        if media_type == "image/svg+xml" {
            assert_eq!(
                stored,
                br##"<svg viewBox="0 0 10 10" xmlns="http://www.w3.org/2000/svg"><rect fill="#123456" height="10" width="10"/></svg>"##
            );
        } else {
            assert_eq!(stored, body);
        }
    }

    for (name, media_type, body) in [
        (
            "sequence.avif",
            "image/avif",
            b"\x00\x00\x00\x18ftypavis\x00\x00\x00\x00avismif1".as_slice(),
        ),
        (
            "truncated.bmp",
            "image/bmp",
            b"BM\x1a\x00\x00\x00".as_slice(),
        ),
        (
            "truncated.png",
            "image/png",
            b"\x89PNG\r\n\x1a\n".as_slice(),
        ),
        ("truncated.jpg", "image/jpeg", b"\xff\xd8\xff".as_slice()),
        ("truncated.gif", "image/gif", b"GIF89a".as_slice()),
        (
            "truncated.webp",
            "image/webp",
            b"RIFF\x04\0\0\0WEBP".as_slice(),
        ),
    ] {
        let error = fixture
            .service
            .create_resource(
                &document_id,
                CreateWorkspaceResourceQuery {
                    workspace_generation: workspace.generation.clone(),
                    folder: WorkspaceRelativePath::parse("assets").unwrap(),
                    name: ResourceName::parse(name).unwrap(),
                    kind: ResourceKind::Image,
                },
                media_type,
                body,
            )
            .await
            .unwrap_err();
        assert_eq!(
            error.kind(),
            ResourceServiceErrorKind::InvalidMediaType,
            "{name}"
        );
    }
}

#[tokio::test]
async fn resource_writer_rejects_active_or_external_svg_without_leaving_a_file() {
    let fixture = Fixture::new().await;
    fs::write(fixture.root.join("note.md"), b"# Note").unwrap();
    let workspace = fixture._workspace.get_workspace().await.unwrap();
    let document_id = fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap()
        .into_iter()
        .find_map(|entry| match entry {
            WorkspaceInventoryEntry::Document(entry) => Some(entry.id),
            WorkspaceInventoryEntry::Resource(_) => None,
        })
        .unwrap();
    let rejected = [
        br#"<svg xmlns="http://www.w3.org/2000/svg" onload="alert(1)"/>"#.as_slice(),
        br#"<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>"#.as_slice(),
        br#"<svg xmlns="http://www.w3.org/2000/svg"><foreignObject><iframe src="https://example.test"/></foreignObject></svg>"#.as_slice(),
        br#"<svg xmlns="http://www.w3.org/2000/svg"><image href="https://example.test/leak.png"/></svg>"#.as_slice(),
        br#"<svg xmlns="http://www.w3.org/2000/svg"><style>@import url(https://example.test/x.css)</style></svg>"#.as_slice(),
        br#"<!DOCTYPE svg [<!ENTITY xxe SYSTEM "file:///etc/passwd">]><svg xmlns="http://www.w3.org/2000/svg">&xxe;</svg>"#.as_slice(),
        br#"<?target data?><svg xmlns="http://www.w3.org/2000/svg"/>"#.as_slice(),
    ];

    for body in rejected {
        let error = fixture
            .service
            .create_resource(
                &document_id,
                CreateWorkspaceResourceQuery {
                    workspace_generation: workspace.generation.clone(),
                    folder: WorkspaceRelativePath::parse("assets").unwrap(),
                    name: ResourceName::parse("unsafe.svg").unwrap(),
                    kind: ResourceKind::Image,
                },
                "image/svg+xml",
                body,
            )
            .await
            .unwrap_err();

        assert_eq!(error.kind(), ResourceServiceErrorKind::InvalidMediaType);
        assert!(!fixture.root.join("assets/unsafe.svg").exists());
    }
}

#[tokio::test]
async fn resource_revision_streams_beyond_the_document_limit_and_opens_by_signed_id() {
    let fixture = Fixture::new().await;
    let path = fixture.root.join("large.bin");
    let mut file = fs::File::create(&path).unwrap();
    file.set_len(16 * 1024 * 1024 + 1).unwrap();
    file.seek(SeekFrom::End(-1)).unwrap();
    file.write_all(&[1]).unwrap();
    file.sync_all().unwrap();
    let before = fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap()
        .into_iter()
        .find_map(|entry| match entry {
            WorkspaceInventoryEntry::Resource(entry) if entry.path.as_str() == "large.bin" => {
                Some(entry)
            }
            _ => None,
        })
        .unwrap();
    file.seek(SeekFrom::End(-1)).unwrap();
    file.write_all(&[2]).unwrap();
    file.sync_all().unwrap();
    let after = fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap()
        .into_iter()
        .find_map(|entry| match entry {
            WorkspaceInventoryEntry::Resource(entry) if entry.path.as_str() == "large.bin" => {
                Some(entry)
            }
            _ => None,
        })
        .unwrap();

    assert_ne!(before.revision, after.revision);
    let mut retained = fixture
        .service
        .open_resource(&after.id, ResourceKind::Attachment)
        .unwrap();
    assert_eq!(retained.entry(), &after);
    assert_eq!(
        io::copy(&mut retained, &mut io::sink()).unwrap(),
        16 * 1024 * 1024 + 1
    );
    retained.verify_complete().unwrap();
}

#[tokio::test]
async fn retained_resource_fails_at_eof_when_its_workspace_name_is_replaced() {
    let fixture = Fixture::new().await;
    let path = fixture.root.join("asset.bin");
    fs::write(&path, b"original").unwrap();
    let entry = fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap()
        .into_iter()
        .find_map(|entry| match entry {
            WorkspaceInventoryEntry::Resource(entry) if entry.path.as_str() == "asset.bin" => {
                Some(entry)
            }
            _ => None,
        })
        .unwrap();
    let mut retained = fixture
        .service
        .open_resource(&entry.id, ResourceKind::Attachment)
        .unwrap();
    fs::rename(&path, fixture.root.join("retired.bin")).unwrap();
    fs::write(&path, b"replaced").unwrap();

    let mut bytes = Vec::new();
    retained.read_to_end(&mut bytes).unwrap();
    let error = retained.verify_complete().unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(bytes, b"original");
}

#[tokio::test]
async fn retained_resource_rejects_a_same_inode_rewrite_with_restored_length_and_mtime() {
    let fixture = Fixture::new().await;
    let path = fixture.root.join("asset.bin");
    fs::write(&path, b"original").unwrap();
    let original_mtime = fs::metadata(&path).unwrap().modified().unwrap();
    let entry = fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap()
        .into_iter()
        .find_map(|entry| match entry {
            WorkspaceInventoryEntry::Resource(entry) => Some(entry),
            WorkspaceInventoryEntry::Document(_) => None,
        })
        .unwrap();
    let mut retained = fixture
        .service
        .open_resource(&entry.id, ResourceKind::Attachment)
        .unwrap();
    let mut writer = fs::OpenOptions::new().write(true).open(&path).unwrap();
    writer.write_all(b"modified").unwrap();
    writer.sync_all().unwrap();
    writer
        .set_times(fs::FileTimes::new().set_modified(original_mtime))
        .unwrap();

    let mut bytes = Vec::new();
    retained.read_to_end(&mut bytes).unwrap();
    let error = retained.verify_complete().unwrap_err();

    assert_eq!(bytes, b"modified");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}

#[tokio::test]
async fn retained_resource_rejects_mixed_bytes_even_when_metadata_is_restored() {
    let fixture = Fixture::new().await;
    let path = fixture.root.join("asset.bin");
    let original = vec![b'a'; 128 * 1024];
    fs::write(&path, &original).unwrap();
    let original_mtime = fs::metadata(&path).unwrap().modified().unwrap();
    let entry = fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap()
        .into_iter()
        .find_map(|entry| match entry {
            WorkspaceInventoryEntry::Resource(entry) => Some(entry),
            WorkspaceInventoryEntry::Document(_) => None,
        })
        .unwrap();
    let mut retained = fixture
        .service
        .open_resource(&entry.id, ResourceKind::Attachment)
        .unwrap();
    let mut streamed = vec![0_u8; 64 * 1024];
    retained.read_exact(&mut streamed).unwrap();
    let mut writer = fs::OpenOptions::new().write(true).open(&path).unwrap();
    let replacement = vec![b'b'; 64 * 1024];
    writer.seek(SeekFrom::Start(64 * 1024)).unwrap();
    writer.write_all(&replacement).unwrap();
    writer.sync_all().unwrap();
    writer
        .set_times(fs::FileTimes::new().set_modified(original_mtime))
        .unwrap();

    retained.read_to_end(&mut streamed).unwrap();
    let error = retained.verify_complete().unwrap_err();

    assert_eq!(&streamed[..64 * 1024], &original[..64 * 1024]);
    assert_eq!(&streamed[64 * 1024..], replacement.as_slice());
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}

#[tokio::test]
async fn exact_content_length_requires_explicit_completion_verification() {
    let fixture = Fixture::new().await;
    fs::write(fixture.root.join("asset.bin"), b"original").unwrap();
    let entry = fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap()
        .into_iter()
        .find_map(|entry| match entry {
            WorkspaceInventoryEntry::Resource(entry) => Some(entry),
            WorkspaceInventoryEntry::Document(_) => None,
        })
        .unwrap();
    let mut retained = fixture
        .service
        .open_resource(&entry.id, ResourceKind::Attachment)
        .unwrap();
    let mut bytes = [0_u8; 8];

    retained.read_exact(&mut bytes).unwrap();

    assert_eq!(&bytes, b"original");
    retained.verify_complete().unwrap();
}

#[tokio::test]
async fn an_empty_read_does_not_mark_the_resource_as_verified() {
    let fixture = Fixture::new().await;
    let path = fixture.root.join("asset.bin");
    fs::write(&path, b"original").unwrap();
    let entry = fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap()
        .into_iter()
        .find_map(|entry| match entry {
            WorkspaceInventoryEntry::Resource(entry) => Some(entry),
            WorkspaceInventoryEntry::Document(_) => None,
        })
        .unwrap();
    let mut retained = fixture
        .service
        .open_resource(&entry.id, ResourceKind::Attachment)
        .unwrap();
    let mut bytes = [0_u8; 8];
    retained.read_exact(&mut bytes).unwrap();
    assert_eq!(retained.read(&mut []).unwrap(), 0);
    fs::rename(&path, fixture.root.join("retired.bin")).unwrap();
    fs::write(&path, b"replaced").unwrap();

    let error = retained.verify_complete().unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}

#[tokio::test]
async fn completion_verification_rejects_an_incomplete_stream() {
    let fixture = Fixture::new().await;
    fs::write(fixture.root.join("asset.bin"), b"original").unwrap();
    let entry = fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap()
        .into_iter()
        .find_map(|entry| match entry {
            WorkspaceInventoryEntry::Resource(entry) => Some(entry),
            WorkspaceInventoryEntry::Document(_) => None,
        })
        .unwrap();
    let mut retained = fixture
        .service
        .open_resource(&entry.id, ResourceKind::Attachment)
        .unwrap();
    let mut prefix = [0_u8; 4];
    retained.read_exact(&mut prefix).unwrap();

    let error = retained.verify_complete().unwrap_err();

    assert_eq!(prefix, *b"orig");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}

#[tokio::test]
async fn inventory_rejects_hard_linked_files() {
    let fixture = Fixture::new().await;
    let outside = fixture.root.parent().unwrap().join("outside.bin");
    fs::write(&outside, b"outside").unwrap();
    fs::hard_link(&outside, fixture.root.join("linked.bin")).unwrap();

    let error = fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap_err();

    assert_eq!(error.kind(), ResourceServiceErrorKind::UnsafeTarget);
}

#[cfg(unix)]
#[tokio::test]
async fn inventory_rejects_symbolic_links_non_regular_entries_and_invalid_portable_names() {
    use std::{os::unix::fs::symlink, os::unix::net::UnixListener};

    let symlink_fixture = Fixture::new().await;
    let outside = symlink_fixture.root.parent().unwrap().join("outside.bin");
    fs::write(&outside, b"outside").unwrap();
    symlink(&outside, symlink_fixture.root.join("linked.bin")).unwrap();
    let symlink_error = symlink_fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap_err();
    assert_eq!(symlink_error.kind(), ResourceServiceErrorKind::UnsafeTarget);

    let socket_fixture = Fixture::new().await;
    let _socket = UnixListener::bind(socket_fixture.root.join("socket")).unwrap();
    let socket_error = socket_fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap_err();
    assert_eq!(socket_error.kind(), ResourceServiceErrorKind::UnsafeTarget);

    let name_fixture = Fixture::new().await;
    fs::write(name_fixture.root.join("bad:name.bin"), b"invalid").unwrap();
    let name_error = name_fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap_err();
    assert_eq!(name_error.kind(), ResourceServiceErrorKind::InvalidPath);
}

#[cfg(unix)]
#[tokio::test]
async fn inventory_excludes_control_links_without_following_them() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new().await;
    let outside = fixture.root.parent().unwrap().join("outside-private");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("secret.bin"), b"secret").unwrap();
    symlink(&outside, fixture.root.join(".qingyu-private-link")).unwrap();
    fs::write(fixture.root.join("safe.bin"), b"safe").unwrap();

    let inventory = fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap();

    assert_eq!(inventory.len(), 1);
    assert_eq!(inventory[0].path().as_str(), "safe.bin");
}

#[tokio::test]
async fn inventory_rejects_direct_access_to_excluded_control_directories() {
    let fixture = Fixture::new().await;
    let control = fixture.root.join(".markra-sync");
    fs::create_dir(&control).unwrap();
    fs::write(control.join("private.bin"), b"private").unwrap();

    for parent in [".qingyu", ".markra-sync"] {
        let error = fixture
            .service
            .list_inventory(&WorkspaceRelativePath::parse(parent).unwrap())
            .unwrap_err();
        assert_eq!(
            error.kind(),
            ResourceServiceErrorKind::InvalidPath,
            "{parent}"
        );
    }
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn inventory_rejects_non_unicode_names() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};

    let fixture = Fixture::new().await;
    fs::write(
        fixture.root.join(OsString::from_vec(vec![b'f', 0xff])),
        b"invalid",
    )
    .unwrap();

    let error = fixture
        .service
        .list_inventory(&WorkspaceRelativePath::default())
        .unwrap_err();

    assert_eq!(error.kind(), ResourceServiceErrorKind::InvalidPath);
}
