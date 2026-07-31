use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use axum::{
    body::{to_bytes, Body, Bytes},
    http::{header, Request, Response, StatusCode},
    Router,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use qingyu_kernel::{
    api::{build_router, TransportPolicy},
    config::KernelConfig,
    contract::{
        ApiErrorEnvelope, DocumentKind, ErrorCode, ResourceKind, ResourceName,
        WorkspaceRelativePath,
    },
    documents::DocumentIgnorePort,
    ignore_rules::{
        MarkdownIgnoreRules, WorkspaceIgnoreError, WorkspaceIgnorePort, WorkspaceIgnoreSnapshot,
    },
    paths::KernelPaths,
    ports::KernelPorts,
    resources::{WorkspaceInventoryEntry, WorkspaceResourceService, MAX_RESOURCE_BODY_BYTES},
    runtime::{KernelRuntime, WorkspaceApiService},
    services::workspace::WorkspaceService,
    workspace::{
        managed::ManagedWorkspaceCollection,
        primary::{
            PrimaryWorkspaceRepositoryBinding, PrimaryWorkspaceStore, PrimaryWorkspaceStoreError,
        },
    },
};
use serde_json::{json, Value};
use tempfile::tempdir;
use tower::ServiceExt as _;

const HOST: &str = "127.0.0.1:43123";
const ORIGIN: &str = "http://127.0.0.1:43123";
const JSON_BODY_LIMIT: usize = 100 * 1024 * 1024;

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

struct AllowAllIgnorePort;

struct CapturedIgnorePort {
    root: PathBuf,
    rules: MarkdownIgnoreRules,
}

impl WorkspaceIgnorePort for AllowAllIgnorePort {
    fn capture(
        &self,
        root_path: &std::path::Path,
        retained_root: &cap_std::fs::Dir,
    ) -> Result<WorkspaceIgnoreSnapshot, WorkspaceIgnoreError> {
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

struct HttpFixture {
    credential: String,
    document_id: String,
    generation: String,
    root: PathBuf,
    router: Router,
    service: WorkspaceResourceService,
    _runtime: Arc<KernelRuntime>,
    _workspace: Arc<WorkspaceService>,
}

impl HttpFixture {
    async fn new() -> Self {
        let temporary = tempdir().unwrap().keep();
        let root = temporary.join("workspace");
        let app_data = temporary.join("app-data");
        let cache = temporary.join("cache");
        for path in [&root, &app_data, &cache] {
            fs::create_dir(path).unwrap();
        }
        fs::write(root.join("note.md"), b"# Note").unwrap();
        let paths = KernelPaths::desktop(&root, &app_data, &cache).unwrap();
        let managed = ManagedWorkspaceCollection::from_paths(&paths).unwrap();
        let config = KernelConfig::generate().unwrap();
        let credential = config.native_launch_credential().expose_secret().to_owned();
        let runtime = KernelRuntime::activate(config, paths, KernelPorts::unavailable()).unwrap();
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
        let identity = workspace.get_workspace().await.unwrap();
        let service = WorkspaceResourceService::new(&runtime, Arc::new(AllowAllIgnorePort));
        let document_id = service
            .list_inventory(&WorkspaceRelativePath::default())
            .unwrap()
            .into_iter()
            .find_map(|entry| match entry {
                WorkspaceInventoryEntry::Document(entry) if entry.kind == DocumentKind::File => {
                    Some(entry.id.as_str().to_owned())
                }
                _ => None,
            })
            .unwrap();
        runtime
            .install_resources_api_service(Arc::new(service.clone()))
            .unwrap();
        let router = build_router(
            runtime.clone(),
            TransportPolicy::loopback(HOST, ORIGIN).unwrap(),
        );
        Self {
            credential,
            document_id,
            generation: identity.generation.as_str().to_owned(),
            root,
            router,
            service,
            _runtime: runtime,
            _workspace: workspace,
        }
    }

    fn request(&self, body: Body) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(format!(
                "/api/v1/documents/{}/resource-batches",
                self.document_id
            ))
            .header(header::HOST, HOST)
            .header(header::AUTHORIZATION, format!("Bearer {}", self.credential))
            .header(header::CONTENT_TYPE, "application/json")
            .body(body)
            .unwrap()
    }

    fn json_request(&self, value: Value) -> Request<Body> {
        self.request(Body::from(serde_json::to_vec(&value).unwrap()))
    }

    fn batch(&self, folder: &str, count: usize, body_base64: &str) -> Value {
        json!({
            "batchId": "8a39c63c-20be-4ee0-bbb4-bafbbc068e78",
            "workspaceGeneration": self.generation,
            "folder": folder,
            "items": (0..count).map(|index| json!({
                "name": format!("item-{index}.bin"),
                "kind": "attachment",
                "mediaType": "application/octet-stream",
                "bodyBase64": body_base64,
            })).collect::<Vec<_>>(),
        })
    }
}

async fn response_json(response: Response<Body>) -> Value {
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

async fn assert_api_error(
    response: Response<Body>,
    expected_status: StatusCode,
    expected_code: ErrorCode,
) {
    assert_eq!(response.status(), expected_status);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    let request_id = response.headers()["x-request-id"]
        .to_str()
        .unwrap()
        .to_owned();
    let envelope = serde_json::from_value::<ApiErrorEnvelope>(response_json(response).await)
        .expect("every batch rejection uses the standard API error envelope");
    assert_eq!(envelope.code(), expected_code);
    assert!(!envelope.message().is_empty());
    assert_eq!(envelope.request_id().as_uuid().to_string(), request_id);
}

#[tokio::test]
async fn batch_http_accepts_thirty_two_items_and_rejects_empty_or_thirty_three() {
    let fixture = HttpFixture::new().await;

    let accepted = fixture
        .router
        .clone()
        .oneshot(fixture.json_request(fixture.batch("accepted", 32, "")))
        .await
        .unwrap();
    assert_eq!(accepted.status(), StatusCode::CREATED);
    assert_eq!(
        response_json(accepted).await["resources"]
            .as_array()
            .unwrap()
            .len(),
        32
    );

    for (folder, count) in [("empty", 0), ("too-many", 33)] {
        let rejected = fixture
            .router
            .clone()
            .oneshot(fixture.json_request(fixture.batch(folder, count, "")))
            .await
            .unwrap();
        assert_api_error(rejected, StatusCode::BAD_REQUEST, ErrorCode::InvalidRequest).await;
        assert!(!fixture.root.join(folder).exists());
    }
}

#[tokio::test]
async fn batch_http_rejects_decoded_total_over_sixty_four_mibibytes() {
    let fixture = HttpFixture::new().await;
    let individually_valid = STANDARD.encode(vec![0_u8; MAX_RESOURCE_BODY_BYTES / 2 + 1]);

    let rejected = fixture
        .router
        .clone()
        .oneshot(fixture.json_request(fixture.batch("oversized", 2, &individually_valid)))
        .await
        .unwrap();

    assert_api_error(
        rejected,
        StatusCode::PAYLOAD_TOO_LARGE,
        ErrorCode::ResourceTooLarge,
    )
    .await;
    assert!(!fixture.root.join("oversized").exists());
}

#[tokio::test]
async fn batch_http_rejects_declared_or_streamed_json_over_one_hundred_mibibytes() {
    let fixture = HttpFixture::new().await;
    let mut request = fixture.request(Body::empty());
    request.headers_mut().insert(
        header::CONTENT_LENGTH,
        (JSON_BODY_LIMIT + 1).to_string().parse().unwrap(),
    );

    let rejected = fixture.router.clone().oneshot(request).await.unwrap();

    assert_api_error(
        rejected,
        StatusCode::PAYLOAD_TOO_LARGE,
        ErrorCode::ResourceTooLarge,
    )
    .await;

    let stream =
        futures_util::stream::unfold(JSON_BODY_LIMIT / (1024 * 1024) + 1, |chunks| async move {
            (chunks > 0).then(|| {
                (
                    Ok::<Bytes, std::io::Error>(Bytes::from(vec![b' '; 1024 * 1024])),
                    chunks - 1,
                )
            })
        });
    let rejected = fixture
        .router
        .clone()
        .oneshot(fixture.request(Body::from_stream(stream)))
        .await
        .unwrap();
    assert_api_error(
        rejected,
        StatusCode::PAYLOAD_TOO_LARGE,
        ErrorCode::ResourceTooLarge,
    )
    .await;
}

#[tokio::test]
async fn batch_http_returns_the_standard_error_envelope_for_malformed_base64() {
    let fixture = HttpFixture::new().await;
    let rejected = fixture
        .router
        .clone()
        .oneshot(fixture.json_request(fixture.batch("invalid", 1, "%%%")))
        .await
        .unwrap();

    assert_api_error(rejected, StatusCode::BAD_REQUEST, ErrorCode::InvalidRequest).await;
    assert!(!fixture.root.join("invalid").exists());
}

#[tokio::test]
async fn svg_resource_response_is_inline_and_fail_closed() {
    let fixture = HttpFixture::new().await;
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
        .create_resource(
            &document_id,
            qingyu_kernel::contract::CreateWorkspaceResourceQuery {
                workspace_generation: workspace.generation,
                folder: WorkspaceRelativePath::parse("assets").unwrap(),
                name: ResourceName::parse("safe.svg").unwrap(),
                kind: ResourceKind::Image,
            },
            "image/svg+xml",
            br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"><rect width="1" height="1" fill="#123456"/></svg>"##,
        )
        .await
        .unwrap();
    let response = fixture
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/api/v1/resources/{}?kind=image",
                    created.id.as_str()
                ))
                .header(header::HOST, HOST)
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", fixture.credential),
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "image/svg+xml");
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    assert_eq!(
        response.headers()[header::CONTENT_SECURITY_POLICY],
        "sandbox; default-src 'none'; script-src 'none'; style-src 'none'; img-src 'none'; font-src 'none'; connect-src 'none'; media-src 'none'; object-src 'none'; frame-src 'none'; worker-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'"
    );
    assert_eq!(response.headers()[header::REFERRER_POLICY], "no-referrer");
    assert_eq!(
        response.headers()["cross-origin-resource-policy"],
        "same-origin"
    );
    assert_eq!(response.headers()[header::X_FRAME_OPTIONS], "DENY");
    assert_eq!(response.headers()[header::CONTENT_DISPOSITION], "inline");
}
