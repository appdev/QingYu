use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read, Seek, SeekFrom, Write},
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

use axum::{
    body::{to_bytes, Body},
    http::{header, Request, StatusCode},
};
use qingyu_kernel::{
    api::{build_router, TransportPolicy},
    config::KernelConfig,
    contract::{
        DocumentKind, ListWorkspaceInventoryQuery, PageLimit, ResourceKind, WorkspaceRelativePath,
    },
    documents::DocumentIgnorePort,
    ignore_rules::{
        MarkdownIgnoreRules, WorkspaceIgnoreError, WorkspaceIgnorePort, WorkspaceIgnoreSnapshot,
    },
    paths::KernelPaths,
    ports::KernelPorts,
    resources::{
        resolve_markdown_href, ResourceServiceErrorKind, RetainedResource, WorkspaceInventoryEntry,
        WorkspaceResourceService,
    },
    runtime::{KernelRuntime, ResourcesApiService},
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

static_assertions::assert_impl_all!(RetainedResource: Read, Send);
static_assertions::assert_impl_all!(WorkspaceResourceService: Send, Sync);

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
    let bytes = b"\x89PNG\r\n\x1a\nverified image bytes";
    fs::write(fixture.root.join("image.png"), bytes).unwrap();
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
    fs::write(
        fixture.root.join("image.png"),
        b"\x89PNG\r\n\x1a\nimage bytes",
    )
    .unwrap();
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
    fs::write(fixture.root.join("photo.JPEG"), b"\xff\xd8\xff\xe0jpeg").unwrap();
    fs::write(fixture.root.join("animation.gif"), b"GIF89agif").unwrap();
    fs::write(
        fixture.root.join("picture.webp"),
        b"RIFF\x04\x00\x00\x00WEBPpayload",
    )
    .unwrap();
    fs::write(
        fixture.root.join("disguised.bin"),
        b"\x89PNG\r\n\x1a\nimage bytes",
    )
    .unwrap();

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
