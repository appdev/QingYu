use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use axum::{
    body::{to_bytes, Body, Bytes},
    http::{header, Request, StatusCode},
    Router,
};
use qingyu_kernel::{
    api::{build_router, TransportPolicy},
    config::KernelConfig,
    contract::{
        ApiErrorEnvelope, ApiVersion, AppConfigSnapshotDto, CreateDocumentRequest,
        CreatedDocumentDto, DeleteDocumentRequest, DocumentContentDto, DocumentEntryDto,
        DocumentHistoryPageDto, DocumentHistorySnapshotDto, DocumentId, DocumentKind,
        DocumentPageDto, ErrorCode, ErrorDetails, ListDocumentsQuery, ListRemoteNotebooksQuery,
        MoveDocumentRequest, PageQuery, PatchAppConfigStateRequest, PatchSettingsRequest,
        ReadyHealthResponse, ReadyStatus, RemoteNotebookCatalogDto, RestoreDocumentHistoryRequest,
        Revision, RunId, RuntimeCapabilitiesDto, RuntimeStateDto, SearchPageDto,
        SearchWorkspaceQuery, SettingsSnapshotDto, SnapshotId, StartupState, SyncConfigViewDto,
        SyncConnectionTestDto, SyncRunAcceptedDto, SyncRunStatusDto, SyncStatusDto,
        SystemVersionResponse, TestSyncConnectionRequest, TriggerSyncRunRequest,
        UpdateDocumentRequest, WorkspaceDto, WorkspaceGeneration, WorkspaceId, WorkspaceReadiness,
        WorkspaceRelativePath,
    },
    paths::KernelPaths,
    ports::KernelPorts,
    runtime::{
        AppConfigApiService, DocumentsApiService, KernelRuntime, ServiceFailure,
        SettingsApiService, SyncApiService, SystemApiService, WorkspaceApiService,
    },
};
use serde_json::{json, Value};
use tempfile::tempdir;
use tower::ServiceExt as _;

const HOST: &str = "127.0.0.1:43123";
const ORIGIN: &str = "tauri://localhost";
const FIELD_SYNC_REVISION: &str =
    "b7f99541b1f844aa3f172d1e3dcff03721b3c7b110c8acaf46d5bbb77e32b8f8";

struct TestApi {
    router: Router,
    credential: String,
    document_id: String,
    runtime_calls: Arc<AtomicUsize>,
    document_create_calls: Arc<AtomicUsize>,
    settings_patch_calls: Arc<AtomicUsize>,
    app_config_calls: Arc<AtomicUsize>,
    app_config_failure: Arc<AtomicUsize>,
    sync_connection_test_calls: Arc<AtomicUsize>,
    repository_list_calls: Arc<AtomicUsize>,
    sync_run_calls: Arc<AtomicUsize>,
    _root: tempfile::TempDir,
}

struct TestSystemService {
    instance_id: qingyu_kernel::contract::InstanceId,
    runtime_calls: Arc<AtomicUsize>,
}

struct TestWorkspaceService;

struct TestDocumentsService {
    create_calls: Arc<AtomicUsize>,
}

struct TestSettingsService {
    patch_calls: Arc<AtomicUsize>,
}

struct TestAppConfigService {
    calls: Arc<AtomicUsize>,
    failure: Arc<AtomicUsize>,
}

struct TestSyncService {
    connection_test_calls: Arc<AtomicUsize>,
    repository_list_calls: Arc<AtomicUsize>,
    run_calls: Arc<AtomicUsize>,
}

#[async_trait::async_trait]
impl WorkspaceApiService for TestWorkspaceService {
    async fn get_workspace(&self) -> Result<WorkspaceDto, ServiceFailure> {
        Ok(WorkspaceDto {
            id: WorkspaceId::new(uuid::Uuid::from_u128(1)),
            generation: WorkspaceGeneration::parse("generation-1").unwrap(),
            display_name: "Workspace".to_owned(),
            readiness: WorkspaceReadiness::Ready,
            revision: Revision::parse("revision-1").unwrap(),
        })
    }
}

#[async_trait::async_trait]
impl DocumentsApiService for TestDocumentsService {
    async fn list_documents(
        &self,
        _query: ListDocumentsQuery,
    ) -> Result<DocumentPageDto, ServiceFailure> {
        unreachable!("not used by this transport fixture")
    }

    async fn create_document(
        &self,
        _request: CreateDocumentRequest,
    ) -> Result<CreatedDocumentDto, ServiceFailure> {
        self.create_calls.fetch_add(1, Ordering::SeqCst);
        Err(ServiceFailure::new(ErrorCode::InternalError, None).unwrap())
    }

    async fn get_document(
        &self,
        _document_id: DocumentId,
    ) -> Result<DocumentContentDto, ServiceFailure> {
        unreachable!("not used by this transport fixture")
    }

    async fn update_document(
        &self,
        _document_id: DocumentId,
        _request: UpdateDocumentRequest,
    ) -> Result<DocumentContentDto, ServiceFailure> {
        unreachable!("not used by this transport fixture")
    }

    async fn move_document(
        &self,
        _document_id: DocumentId,
        _request: MoveDocumentRequest,
    ) -> Result<DocumentEntryDto, ServiceFailure> {
        unreachable!("not used by this transport fixture")
    }

    async fn delete_document(
        &self,
        _document_id: DocumentId,
        _request: DeleteDocumentRequest,
    ) -> Result<(), ServiceFailure> {
        unreachable!("not used by this transport fixture")
    }

    async fn list_document_history(
        &self,
        _document_id: DocumentId,
        _query: PageQuery,
    ) -> Result<DocumentHistoryPageDto, ServiceFailure> {
        unreachable!("not used by this transport fixture")
    }

    async fn get_document_history(
        &self,
        _document_id: DocumentId,
        _snapshot_id: SnapshotId,
    ) -> Result<DocumentHistorySnapshotDto, ServiceFailure> {
        unreachable!("not used by this transport fixture")
    }

    async fn restore_document_history(
        &self,
        _document_id: DocumentId,
        _snapshot_id: SnapshotId,
        _request: RestoreDocumentHistoryRequest,
    ) -> Result<DocumentContentDto, ServiceFailure> {
        unreachable!("not used by this transport fixture")
    }

    async fn search_workspace(
        &self,
        _query: SearchWorkspaceQuery,
    ) -> Result<SearchPageDto, ServiceFailure> {
        unreachable!("not used by this transport fixture")
    }
}

#[async_trait::async_trait]
impl SettingsApiService for TestSettingsService {
    async fn get_settings(&self) -> Result<SettingsSnapshotDto, ServiceFailure> {
        Ok(SettingsSnapshotDto {
            revision: Revision::parse("settings-1").unwrap(),
            values: vec![],
        })
    }

    async fn patch_settings(
        &self,
        _request: PatchSettingsRequest,
    ) -> Result<SettingsSnapshotDto, ServiceFailure> {
        self.patch_calls.fetch_add(1, Ordering::SeqCst);
        Ok(SettingsSnapshotDto {
            revision: Revision::parse("settings-2").unwrap(),
            values: vec![],
        })
    }
}

#[async_trait::async_trait]
impl AppConfigApiService for TestAppConfigService {
    async fn get_app_config(&self) -> Result<AppConfigSnapshotDto, ServiceFailure> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.failure.load(Ordering::SeqCst) != 0 {
            return Err(ServiceFailure::new(ErrorCode::AppConfigUnavailable, None).unwrap());
        }
        Ok(app_config_snapshot())
    }

    async fn patch_app_config_state(
        &self,
        request: PatchAppConfigStateRequest,
    ) -> Result<AppConfigSnapshotDto, ServiceFailure> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.failure.load(Ordering::SeqCst) != 0 {
            return Err(ServiceFailure::new(ErrorCode::AppConfigUnavailable, None).unwrap());
        }
        match request.workspace_generation.as_str() {
            "retired-generation" => {
                Err(ServiceFailure::new(ErrorCode::WorkspaceGenerationStale, None).unwrap())
            }
            "invalid-state" => {
                Err(ServiceFailure::new(ErrorCode::InvalidAppConfigState, None).unwrap())
            }
            _ => Ok(app_config_snapshot()),
        }
    }
}

fn app_config_snapshot() -> AppConfigSnapshotDto {
    serde_json::from_value(json!({
        "appConfigVersion": 1,
        "settings": {
            "revision": "settings-1",
            "values": []
        },
        "workspace": {
            "id": uuid::Uuid::from_u128(1),
            "generation": "generation-1"
        },
        "localState": {
            "revision": "app-config-1",
            "uiLayout": {
                "schemaVersion": 1,
                "windowStates": {},
                "openWindows": []
            },
            "recentMarkdownFiles": [],
            "fileTreeSort": {
                "key": "name",
                "direction": "ascending"
            },
            "pandocPath": null
        }
    }))
    .unwrap()
}

#[async_trait::async_trait]
impl SyncApiService for TestSyncService {
    async fn get_sync_config(&self) -> Result<SyncConfigViewDto, ServiceFailure> {
        unreachable!("not used by this transport fixture")
    }

    async fn patch_sync_config(
        &self,
        _request: qingyu_kernel::contract::PatchSyncConfigRequest,
    ) -> Result<SyncConfigViewDto, ServiceFailure> {
        unreachable!("not used by this transport fixture")
    }

    async fn test_sync_connection(
        &self,
        _request: TestSyncConnectionRequest,
    ) -> Result<SyncConnectionTestDto, ServiceFailure> {
        self.connection_test_calls.fetch_add(1, Ordering::SeqCst);
        Err(ServiceFailure::new(ErrorCode::InternalError, None).unwrap())
    }

    async fn get_sync_status(&self) -> Result<SyncStatusDto, ServiceFailure> {
        Err(ServiceFailure::new(
            ErrorCode::KernelNotReady,
            Some(ErrorDetails::Startup {
                state: StartupState::Starting,
            }),
        )
        .unwrap())
    }

    async fn get_sync_run(&self, _run_id: RunId) -> Result<SyncRunStatusDto, ServiceFailure> {
        Err(ServiceFailure::new(ErrorCode::ResourceNotFound, None).unwrap())
    }

    async fn trigger_sync_run(
        &self,
        _request: TriggerSyncRunRequest,
    ) -> Result<SyncRunAcceptedDto, ServiceFailure> {
        self.run_calls.fetch_add(1, Ordering::SeqCst);
        Err(ServiceFailure::new(ErrorCode::InternalError, None).unwrap())
    }

    async fn list_remote_notebooks(
        &self,
        request: ListRemoteNotebooksQuery,
    ) -> Result<RemoteNotebookCatalogDto, ServiceFailure> {
        if request.expected_revision.as_str() != FIELD_SYNC_REVISION {
            return Err(ServiceFailure::new(
                ErrorCode::SyncConfigRevisionConflict,
                Some(ErrorDetails::RevisionConflict {
                    current_revision: Some(Revision::parse(FIELD_SYNC_REVISION).unwrap()),
                }),
            )
            .unwrap());
        }
        self.repository_list_calls.fetch_add(1, Ordering::SeqCst);
        Ok(RemoteNotebookCatalogDto { entries: vec![] })
    }
}

#[async_trait::async_trait]
impl SystemApiService for TestSystemService {
    async fn ready(&self) -> Result<ReadyHealthResponse, ServiceFailure> {
        Ok(ReadyHealthResponse {
            status: ReadyStatus::Ready,
            api_version: ApiVersion::V1,
            instance_id: self.instance_id,
        })
    }

    async fn version(&self) -> Result<SystemVersionResponse, ServiceFailure> {
        Ok(SystemVersionResponse {
            api_version: ApiVersion::V1,
            kernel_version: env!("CARGO_PKG_VERSION").to_owned(),
            instance_id: self.instance_id,
        })
    }

    async fn runtime_state(&self) -> Result<RuntimeStateDto, ServiceFailure> {
        self.runtime_calls.fetch_add(1, Ordering::SeqCst);
        Ok(RuntimeStateDto {
            profile: qingyu_kernel::contract::HostProfile::Desktop,
            startup_state: StartupState::Ready,
            capabilities: RuntimeCapabilitiesDto {
                documents: false,
                history: false,
                resources: false,
                search: false,
                settings: false,
                sync: false,
                webdav: false,
                s3: false,
                portable_settings: false,
            },
            instance_id: self.instance_id,
        })
    }
}

impl TestApi {
    fn new() -> Self {
        Self::with_sync_service(true)
    }

    fn without_sync_service() -> Self {
        Self::with_sync_service(false)
    }

    fn with_sync_service(install_sync_service: bool) -> Self {
        let root = tempdir().unwrap();
        let workspace = root.path().join("workspace");
        let app_data = root.path().join("app-data");
        let cache = root.path().join("cache");
        std::fs::create_dir(&workspace).unwrap();
        std::fs::create_dir(&app_data).unwrap();
        std::fs::create_dir(&cache).unwrap();
        let config = KernelConfig::generate().unwrap();
        let credential = config.native_launch_credential().expose_secret().to_owned();
        let document_id = config
            .wire_identity_key()
            .issue_document_id(
                WorkspaceId::new(uuid::Uuid::from_u128(1)),
                &WorkspaceGeneration::parse("generation-1").unwrap(),
                DocumentKind::File,
                &WorkspaceRelativePath::parse("existing.md").unwrap(),
            )
            .unwrap()
            .as_str()
            .to_owned();
        let runtime = KernelRuntime::activate(
            config,
            KernelPaths::desktop(&workspace, &app_data, &cache).unwrap(),
            KernelPorts::unavailable(),
        )
        .unwrap();
        let runtime_calls = Arc::new(AtomicUsize::new(0));
        runtime
            .install_system_api_service(Arc::new(TestSystemService {
                instance_id: runtime.instance_id(),
                runtime_calls: runtime_calls.clone(),
            }))
            .expect("install system API service once");
        runtime
            .install_workspace_api_service(Arc::new(TestWorkspaceService))
            .expect("install workspace API service once");
        let policy = TransportPolicy::loopback(HOST, ORIGIN).unwrap();
        let document_create_calls = Arc::new(AtomicUsize::new(0));
        let settings_patch_calls = Arc::new(AtomicUsize::new(0));
        let app_config_calls = Arc::new(AtomicUsize::new(0));
        let app_config_failure = Arc::new(AtomicUsize::new(0));
        let sync_connection_test_calls = Arc::new(AtomicUsize::new(0));
        let repository_list_calls = Arc::new(AtomicUsize::new(0));
        let sync_run_calls = Arc::new(AtomicUsize::new(0));
        runtime
            .install_documents_api_service(Arc::new(TestDocumentsService {
                create_calls: document_create_calls.clone(),
            }))
            .expect("install document API service once");
        runtime
            .install_settings_api_service(Arc::new(TestSettingsService {
                patch_calls: settings_patch_calls.clone(),
            }))
            .expect("install settings API service once");
        runtime
            .install_app_config_api_service(Arc::new(TestAppConfigService {
                calls: app_config_calls.clone(),
                failure: app_config_failure.clone(),
            }))
            .expect("install app config API service once");
        if install_sync_service {
            runtime
                .install_sync_api_service(Arc::new(TestSyncService {
                    connection_test_calls: sync_connection_test_calls.clone(),
                    repository_list_calls: repository_list_calls.clone(),
                    run_calls: sync_run_calls.clone(),
                }))
                .expect("install sync API service once");
        }

        Self {
            router: build_router(runtime, policy),
            credential,
            document_id,
            runtime_calls,
            document_create_calls,
            settings_patch_calls,
            app_config_calls,
            app_config_failure,
            sync_connection_test_calls,
            repository_list_calls,
            sync_run_calls,
            _root: root,
        }
    }

    fn request(&self, method: &str, path: &str) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, HOST)
            .body(Body::empty())
            .unwrap()
    }

    fn authorized_request(&self, method: &str, path: &str) -> Request<Body> {
        let mut request = self.request(method, path);
        request.headers_mut().insert(
            header::AUTHORIZATION,
            format!("Bearer {}", self.credential).parse().unwrap(),
        );
        request
    }

    fn authorized_json_request(&self, method: &str, path: &str, value: Value) -> Request<Body> {
        let mut request = self.authorized_request(method, path);
        request
            .headers_mut()
            .insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
        *request.body_mut() = Body::from(serde_json::to_vec(&value).unwrap());
        request
    }
}

async fn body_json(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn app_config_get_returns_one_aggregate_snapshot() {
    let api = TestApi::new();
    let response = api
        .router
        .clone()
        .oneshot(api.authorized_request("GET", "/api/v1/app-config"))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["workspace"]["generation"], "generation-1");
    assert!(body["settings"].is_object());
    assert!(body["localState"]["uiLayout"]["windowStates"].is_object());
    assert_eq!(api.app_config_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn stale_app_config_patch_maps_to_conflict() {
    let api = TestApi::new();
    let request = api.authorized_json_request(
        "PATCH",
        "/api/v1/app-config/state",
        json!({
            "workspaceGeneration": "retired-generation",
            "operations": [{ "type": "clear-recent-files" }]
        }),
    );
    let response = api.router.clone().oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        body_json(response).await["code"],
        "workspace_generation_stale"
    );
}

#[tokio::test]
async fn app_config_routes_require_authentication() {
    let api = TestApi::new();
    let get = api
        .router
        .clone()
        .oneshot(api.request("GET", "/api/v1/app-config"))
        .await
        .unwrap();
    assert_eq!(get.status(), StatusCode::UNAUTHORIZED);

    let mut patch = api.request("PATCH", "/api/v1/app-config/state");
    patch
        .headers_mut()
        .insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
    *patch.body_mut() = Body::from(
        serde_json::to_vec(&json!({
            "workspaceGeneration": "generation-1",
            "operations": [{ "type": "clear-recent-files" }]
        }))
        .unwrap(),
    );
    let patch = api.router.clone().oneshot(patch).await.unwrap();
    assert_eq!(patch.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(api.app_config_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn invalid_app_config_state_maps_to_unprocessable_entity_before_service_invocation() {
    let api = TestApi::new();
    for body in [
        json!({
            "workspaceGeneration": "generation-1",
            "operations": [{ "type": "remove-recent-file", "path": "../secret.md" }]
        }),
        json!({
            "workspaceGeneration": "generation-1",
            "operations": [{ "type": "clear-recent-files", "extra": true }]
        }),
    ] {
        let response = api
            .router
            .clone()
            .oneshot(api.authorized_json_request("PATCH", "/api/v1/app-config/state", body))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            body_json(response).await["code"],
            "invalid_app_config_state"
        );
    }
    assert_eq!(api.app_config_calls.load(Ordering::SeqCst), 0);

    let response = api
        .router
        .clone()
        .oneshot(api.authorized_json_request(
            "PATCH",
            "/api/v1/app-config/state",
            json!({
                "workspaceGeneration": "invalid-state",
                "operations": [{ "type": "clear-recent-files" }]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body_json(response).await["code"],
        "invalid_app_config_state"
    );
}

#[tokio::test]
async fn app_config_body_limit_accepts_maximum_draft_content_envelope_and_rejects_over_limit() {
    let api = TestApi::new();
    let draft_content = "x".repeat(16 * 1024 * 1024);
    let operations = (0..3)
        .map(|index| {
            json!({
                "type": "patch-ui-layout",
                "windowLabel": format!("window-{index}"),
                "patch": {
                    "draftTabs": [{
                        "id": format!("draft-{index}"),
                        "name": format!("Draft {index}"),
                        "path": null,
                        "content": draft_content
                    }]
                }
            })
        })
        .collect::<Vec<_>>();
    let accepted = api
        .router
        .clone()
        .oneshot(api.authorized_json_request(
            "PATCH",
            "/api/v1/app-config/state",
            json!({
                "workspaceGeneration": "generation-1",
                "operations": operations
            }),
        ))
        .await
        .unwrap();
    assert_eq!(accepted.status(), StatusCode::OK);
    assert_eq!(api.app_config_calls.load(Ordering::SeqCst), 1);

    let mut request = api.authorized_request("PATCH", "/api/v1/app-config/state");
    request
        .headers_mut()
        .insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
    *request.body_mut() = Body::from(vec![b' '; 64 * 1024 * 1024 + 1]);
    let rejected = api.router.clone().oneshot(request).await.unwrap();
    assert_eq!(rejected.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(api.app_config_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn unavailable_app_config_maps_to_service_unavailable() {
    let api = TestApi::new();
    api.app_config_failure.store(1, Ordering::SeqCst);
    let response = api
        .router
        .clone()
        .oneshot(api.authorized_request("GET", "/api/v1/app-config"))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body_json(response).await["code"], "app_config_unavailable");
}

#[tokio::test]
async fn app_config_route_methods_and_preflight_are_exact() {
    let api = TestApi::new();
    for (path, accepted, rejected) in [
        ("/api/v1/app-config", "GET", "PATCH"),
        ("/api/v1/app-config/state", "PATCH", "GET"),
    ] {
        let accepted = Request::builder()
            .method("OPTIONS")
            .uri(path)
            .header(header::HOST, HOST)
            .header(header::ORIGIN, ORIGIN)
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, accepted)
            .header(
                header::ACCESS_CONTROL_REQUEST_HEADERS,
                "authorization, content-type, x-csrf-token",
            )
            .body(Body::empty())
            .unwrap();
        let accepted = api.router.clone().oneshot(accepted).await.unwrap();
        assert_eq!(accepted.status(), StatusCode::NO_CONTENT, "{path}");

        let rejected = Request::builder()
            .method("OPTIONS")
            .uri(path)
            .header(header::HOST, HOST)
            .header(header::ORIGIN, ORIGIN)
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, rejected)
            .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "authorization")
            .body(Body::empty())
            .unwrap();
        let rejected = api.router.clone().oneshot(rejected).await.unwrap();
        assert_eq!(rejected.status(), StatusCode::BAD_REQUEST, "{path}");
    }
}

#[tokio::test]
async fn live_probe_is_originless_and_bearerless_after_exact_host_validation() {
    let api = TestApi::new();
    let response = api
        .router
        .clone()
        .oneshot(api.request("GET", "/api/v1/health/live"))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(
        body_json(response).await,
        json!({ "status": "live", "apiVersion": "v1" })
    );
}

#[tokio::test]
async fn cross_origin_http_responses_expose_required_contract_headers() {
    let api = TestApi::new();

    let mut success = api.request("GET", "/api/v1/health/live");
    success
        .headers_mut()
        .insert(header::ORIGIN, ORIGIN.parse().unwrap());
    let success = api.router.clone().oneshot(success).await.unwrap();
    assert_eq!(success.status(), StatusCode::OK);
    assert_eq!(
        success.headers()[header::ACCESS_CONTROL_EXPOSE_HEADERS],
        "Retry-After, X-Request-Id, X-Content-Type-Options, X-Resource-Revision"
    );
    uuid::Uuid::parse_str(success.headers()["x-request-id"].to_str().unwrap())
        .expect("successful browser response request ID must be a UUID");

    let mut error = api.request("GET", "/api/v1/runtime");
    error
        .headers_mut()
        .insert(header::ORIGIN, ORIGIN.parse().unwrap());
    let error = api.router.clone().oneshot(error).await.unwrap();
    assert_eq!(error.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        error.headers()[header::ACCESS_CONTROL_EXPOSE_HEADERS],
        "Retry-After, X-Request-Id, X-Content-Type-Options, X-Resource-Revision"
    );
    let request_id = error.headers()["x-request-id"].clone();
    let envelope: ApiErrorEnvelope = serde_json::from_value(body_json(error).await).unwrap();
    assert_eq!(
        request_id.to_str().unwrap(),
        envelope.request_id().as_uuid().to_string()
    );
}

#[tokio::test]
async fn host_and_origin_are_rejected_before_route_or_auth_processing() {
    let api = TestApi::new();
    let mut wrong_host = api.authorized_request("GET", "/api/v1/runtime");
    wrong_host
        .headers_mut()
        .insert(header::HOST, "localhost:43123".parse().unwrap());
    let first = api.router.clone().oneshot(wrong_host).await.unwrap();
    let first_request_id = first.headers()["x-request-id"].clone();

    assert_eq!(first.status(), StatusCode::FORBIDDEN);
    assert_eq!(first.headers()[header::CACHE_CONTROL], "no-store");
    let envelope: ApiErrorEnvelope = serde_json::from_value(body_json(first).await).unwrap();
    assert_eq!(
        envelope.code(),
        qingyu_kernel::contract::ErrorCode::HostNotAllowed
    );
    assert_eq!(
        first_request_id.to_str().unwrap(),
        envelope.request_id().as_uuid().to_string()
    );

    let mut wrong_origin = api.authorized_request("GET", "/api/v1/runtime");
    wrong_origin
        .headers_mut()
        .insert(header::ORIGIN, "https://attacker.invalid".parse().unwrap());
    let second = api.router.clone().oneshot(wrong_origin).await.unwrap();

    assert_eq!(second.status(), StatusCode::FORBIDDEN);
    assert_ne!(first_request_id, second.headers()["x-request-id"]);
    let envelope: ApiErrorEnvelope = serde_json::from_value(body_json(second).await).unwrap();
    assert_eq!(
        envelope.code(),
        qingyu_kernel::contract::ErrorCode::OriginNotAllowed
    );
}

#[tokio::test]
async fn protected_routes_require_the_exact_bearer_without_leaking_it() {
    let api = TestApi::new();
    let missing = api
        .router
        .clone()
        .oneshot(api.request("GET", "/api/v1/runtime"))
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);

    let mut browser_missing = api.request("GET", "/api/v1/runtime");
    browser_missing
        .headers_mut()
        .insert(header::ORIGIN, ORIGIN.parse().unwrap());
    let browser_missing = api.router.clone().oneshot(browser_missing).await.unwrap();
    assert_eq!(browser_missing.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        browser_missing.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
        ORIGIN
    );

    let mut wrong = api.request("GET", "/api/v1/runtime");
    wrong.headers_mut().insert(
        header::AUTHORIZATION,
        "Bearer definitely-wrong".parse().unwrap(),
    );
    let wrong = api.router.clone().oneshot(wrong).await.unwrap();
    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);
    let wrong_body = body_json(wrong).await;
    assert!(!wrong_body.to_string().contains(&api.credential));
    assert!(!wrong_body.to_string().contains("definitely-wrong"));

    let allowed = api
        .router
        .clone()
        .oneshot(api.authorized_request("GET", "/api/v1/runtime"))
        .await
        .unwrap();
    assert_eq!(allowed.status(), StatusCode::OK);
    assert_eq!(allowed.headers()[header::CACHE_CONTROL], "no-store");
}

#[tokio::test]
async fn authenticated_routes_delegate_to_the_installed_domain_service() {
    let api = TestApi::new();
    let response = api
        .router
        .clone()
        .oneshot(api.authorized_request("GET", "/api/v1/workspace"))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_json(response).await,
        json!({
            "id": uuid::Uuid::from_u128(1),
            "generation": "generation-1",
            "displayName": "Workspace",
            "readiness": "ready",
            "revision": "revision-1"
        })
    );
}

#[tokio::test]
async fn body_limits_are_route_specific_and_run_before_domain_delegation() {
    let api = TestApi::new();
    let mut settings_body = vec![b' '; 1024 * 1024];
    settings_body.extend_from_slice(br#"{"expectedRevision":"settings-1","values":[]}"#);
    let mut settings = api.authorized_request("PATCH", "/api/v1/settings");
    settings
        .headers_mut()
        .insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
    *settings.body_mut() = Body::from(settings_body);
    let settings = api.router.clone().oneshot(settings).await.unwrap();

    assert_eq!(settings.status(), StatusCode::BAD_REQUEST);
    assert_eq!(api.settings_patch_calls.load(Ordering::SeqCst), 0);

    let document = json!({
        "kind": "file",
        "workspaceGeneration": "generation-1",
        "parent": "",
        "name": "large.md",
        "contents": "x".repeat(2 * 1024 * 1024)
    });
    let mut create = api.authorized_request("POST", "/api/v1/documents");
    create
        .headers_mut()
        .insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
    *create.body_mut() = Body::from(serde_json::to_vec(&document).unwrap());
    let create = api.router.clone().oneshot(create).await.unwrap();

    assert_eq!(create.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(api.document_create_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn decoded_document_content_limit_maps_to_document_too_large() {
    let api = TestApi::new();
    let document = json!({
        "kind": "file",
        "workspaceGeneration": "generation-1",
        "parent": "",
        "name": "too-large.md",
        "contents": "x".repeat(16 * 1024 * 1024 + 1)
    });
    let mut create = api.authorized_request("POST", "/api/v1/documents");
    create
        .headers_mut()
        .insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
    *create.body_mut() = Body::from(serde_json::to_vec(&document).unwrap());
    let response = api.router.clone().oneshot(create).await.unwrap();

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(api.document_create_calls.load(Ordering::SeqCst), 0);
    let envelope: ApiErrorEnvelope = serde_json::from_value(body_json(response).await).unwrap();
    assert_eq!(envelope.code(), ErrorCode::DocumentTooLarge);
}

#[tokio::test]
async fn document_name_and_relative_path_failures_keep_route_specific_codes() {
    let api = TestApi::new();
    let cases = [
        (
            "POST".to_owned(),
            "/api/v1/documents".to_owned(),
            json!({
                "kind": "file",
                "workspaceGeneration": "generation-1",
                "parent": "",
                "name": "not-markdown.txt",
                "contents": ""
            }),
            ErrorCode::InvalidDocumentName,
        ),
        (
            "POST".to_owned(),
            "/api/v1/documents".to_owned(),
            json!({
                "kind": "file",
                "workspaceGeneration": "generation-1",
                "parent": "../escape",
                "name": "valid.md",
                "contents": ""
            }),
            ErrorCode::InvalidWorkspacePath,
        ),
        (
            "POST".to_owned(),
            format!("/api/v1/documents/{}/move", api.document_id),
            json!({
                "workspaceGeneration": "generation-1",
                "expectedRevision": "revision-1",
                "targetParent": "",
                "name": "bad/name.md"
            }),
            ErrorCode::InvalidDocumentName,
        ),
        (
            "POST".to_owned(),
            format!("/api/v1/documents/{}/move", api.document_id),
            json!({
                "workspaceGeneration": "generation-1",
                "expectedRevision": "revision-1",
                "targetParent": "../escape",
                "name": "valid.md"
            }),
            ErrorCode::InvalidWorkspacePath,
        ),
    ];

    for (method, path, body, expected) in cases {
        let mut request = api.authorized_request(&method, &path);
        request
            .headers_mut()
            .insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
        *request.body_mut() = Body::from(serde_json::to_vec(&body).unwrap());
        let response = api.router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let envelope: ApiErrorEnvelope = serde_json::from_value(body_json(response).await).unwrap();
        assert_eq!(envelope.code(), expected);
    }
    assert_eq!(api.document_create_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn list_documents_invalid_parent_keeps_the_route_specific_code() {
    let api = TestApi::new();
    let response = api
        .router
        .clone()
        .oneshot(api.authorized_request("GET", "/api/v1/documents?parent=..%2Fescape"))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let envelope: ApiErrorEnvelope = serde_json::from_value(body_json(response).await).unwrap();
    assert_eq!(envelope.code(), ErrorCode::InvalidWorkspacePath);
}

#[tokio::test]
async fn document_body_stream_failures_are_invalid_requests_not_size_errors() {
    let api = TestApi::new();
    let stream = futures_util::stream::once(async {
        Err::<Bytes, _>(std::io::Error::other("fixture body stream failed"))
    });
    let mut request = api.authorized_request("POST", "/api/v1/documents");
    request
        .headers_mut()
        .insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
    *request.body_mut() = Body::from_stream(stream);
    let response = api.router.clone().oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(api.document_create_calls.load(Ordering::SeqCst), 0);
    let envelope: ApiErrorEnvelope = serde_json::from_value(body_json(response).await).unwrap();
    assert_eq!(envelope.code(), ErrorCode::InvalidRequest);
}

#[tokio::test]
async fn api_version_root_uses_the_safe_invalid_request_envelope() {
    let api = TestApi::new();
    let response = api
        .router
        .clone()
        .oneshot(api.authorized_request("GET", "/api/v1"))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let envelope: ApiErrorEnvelope = serde_json::from_value(body_json(response).await).unwrap();
    assert_eq!(envelope.code(), ErrorCode::InvalidRequest);
}

#[tokio::test]
async fn preflight_rejects_duplicate_requested_method_headers() {
    let api = TestApi::new();
    let mut request = Request::builder()
        .method("OPTIONS")
        .uri("/api/v1/settings")
        .header(header::HOST, HOST)
        .header(header::ORIGIN, ORIGIN)
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "PATCH")
        .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "authorization")
        .body(Body::empty())
        .unwrap();
    request.headers_mut().append(
        header::ACCESS_CONTROL_REQUEST_METHOD,
        "GET".parse().unwrap(),
    );
    let response = api.router.clone().oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(response
        .headers()
        .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
        .is_none());
}

#[tokio::test]
async fn sync_mutation_routes_delegate_exactly_once_without_transport_retries() {
    let api = TestApi::new();
    let cases = [
        (
            "/api/v1/sync/connection-test",
            json!({
                "expectedRevision": "sync-1",
                "changes": { "enabled": true }
            }),
        ),
        (
            "/api/v1/sync/runs",
            json!({ "expectedConfigRevision": "sync-1" }),
        ),
    ];

    for (path, body) in cases {
        let mut request = api.authorized_request("POST", path);
        request
            .headers_mut()
            .insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
        *request.body_mut() = Body::from(serde_json::to_vec(&body).unwrap());
        let response = api.router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    assert_eq!(api.sync_connection_test_calls.load(Ordering::SeqCst), 1);
    assert_eq!(api.sync_run_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn native_bearer_catalog_get_accepts_the_exact_camel_case_revision() {
    let api = TestApi::new();
    let response = api
        .router
        .clone()
        .oneshot(api.authorized_request(
            "GET",
            &format!("/api/v1/sync/repositories?expectedRevision={FIELD_SYNC_REVISION}"),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await, json!({ "entries": [] }));
    assert_eq!(api.repository_list_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn sync_repository_catalog_query_remains_strict_and_fail_closed() {
    let api = TestApi::new();
    for path in [
        "/api/v1/sync/repositories",
        "/api/v1/sync/repositories?expectedRevision=",
        "/api/v1/sync/repositories?expected_revision=b7f99541b1f844aa3f172d1e3dcff03721b3c7b110c8acaf46d5bbb77e32b8f8",
        "/api/v1/sync/repositories?expectedRevision=b7f99541b1f844aa3f172d1e3dcff03721b3c7b110c8acaf46d5bbb77e32b8f8&expectedRevision=b7f99541b1f844aa3f172d1e3dcff03721b3c7b110c8acaf46d5bbb77e32b8f8",
        "/api/v1/sync/repositories?expectedRevision=b7f99541b1f844aa3f172d1e3dcff03721b3c7b110c8acaf46d5bbb77e32b8f8&unknown=value",
    ] {
        let response = api
            .router
            .clone()
            .oneshot(api.authorized_request("GET", path))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path}");
        assert_eq!(body_json(response).await["code"], "invalid_request", "{path}");
    }
    assert_eq!(api.repository_list_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn sync_repository_routes_preflight_only_their_exact_methods() {
    let api = TestApi::new();
    for (path, accepted_method, rejected_method) in [
        ("/api/v1/sync/repositories", "GET", "POST"),
        ("/api/v1/sync/dejavu/key", "GET", "POST"),
        ("/api/v1/sync/dejavu/key/import", "POST", "GET"),
        ("/api/v1/sync/dejavu/key/export", "POST", "GET"),
    ] {
        let accepted = Request::builder()
            .method("OPTIONS")
            .uri(path)
            .header(header::HOST, HOST)
            .header(header::ORIGIN, ORIGIN)
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, accepted_method)
            .header(
                header::ACCESS_CONTROL_REQUEST_HEADERS,
                "authorization, content-type, x-csrf-token",
            )
            .body(Body::empty())
            .unwrap();
        let accepted = api.router.clone().oneshot(accepted).await.unwrap();
        assert_eq!(accepted.status(), StatusCode::NO_CONTENT, "{path}");
        assert_eq!(
            accepted.headers()[header::ACCESS_CONTROL_ALLOW_METHODS],
            accepted_method,
            "{path}"
        );

        let rejected = Request::builder()
            .method("OPTIONS")
            .uri(path)
            .header(header::HOST, HOST)
            .header(header::ORIGIN, ORIGIN)
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, rejected_method)
            .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "authorization")
            .body(Body::empty())
            .unwrap();
        let rejected = api.router.clone().oneshot(rejected).await.unwrap();
        assert_eq!(rejected.status(), StatusCode::BAD_REQUEST, "{path}");
        assert!(
            rejected
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .is_none(),
            "{path}"
        );
    }

    for accepted_method in ["GET", "POST"] {
        let accepted = Request::builder()
            .method("OPTIONS")
            .uri("/api/v1/sync/repository-binding")
            .header(header::HOST, HOST)
            .header(header::ORIGIN, ORIGIN)
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, accepted_method)
            .header(
                header::ACCESS_CONTROL_REQUEST_HEADERS,
                "authorization, content-type, x-csrf-token",
            )
            .body(Body::empty())
            .unwrap();
        let accepted = api.router.clone().oneshot(accepted).await.unwrap();
        assert_eq!(accepted.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            accepted.headers()[header::ACCESS_CONTROL_ALLOW_METHODS],
            accepted_method
        );
    }

    let rejected = Request::builder()
        .method("OPTIONS")
        .uri("/api/v1/sync/repository-binding")
        .header(header::HOST, HOST)
        .header(header::ORIGIN, ORIGIN)
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "PATCH")
        .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "authorization")
        .body(Body::empty())
        .unwrap();
    let rejected = api.router.clone().oneshot(rejected).await.unwrap();
    assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
    assert!(rejected
        .headers()
        .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
        .is_none());
}

#[tokio::test]
async fn sync_repository_binding_view_requires_authentication() {
    let api = TestApi::new();
    let response = api
        .router
        .clone()
        .oneshot(api.request("GET", "/api/v1/sync/repository-binding"))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn sync_run_status_route_parses_the_run_id_and_preserves_resource_not_found() {
    let api = TestApi::new();
    let response = api
        .router
        .clone()
        .oneshot(api.authorized_request(
            "GET",
            "/api/v1/sync/runs/123e4567-e89b-42d3-a456-426614174000",
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let envelope: ApiErrorEnvelope = serde_json::from_value(body_json(response).await).unwrap();
    assert_eq!(envelope.code(), ErrorCode::ResourceNotFound);

    let malformed = api
        .router
        .clone()
        .oneshot(api.authorized_request("GET", "/api/v1/sync/runs/not-a-uuid"))
        .await
        .unwrap();
    assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);
    let envelope: ApiErrorEnvelope = serde_json::from_value(body_json(malformed).await).unwrap();
    assert_eq!(envelope.code(), ErrorCode::InvalidRequest);
}

#[tokio::test]
async fn preflight_is_origin_scoped_bearerless_and_never_wildcard() {
    let api = TestApi::new();
    let request = Request::builder()
        .method("OPTIONS")
        .uri("/api/v1/settings")
        .header(header::HOST, HOST)
        .header(header::ORIGIN, ORIGIN)
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "PATCH")
        .header(
            header::ACCESS_CONTROL_REQUEST_HEADERS,
            "authorization, content-type",
        )
        .body(Body::empty())
        .unwrap();
    let response = api.router.clone().oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
        ORIGIN
    );
    assert_ne!(response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert!(response.headers()[header::VARY]
        .to_str()
        .unwrap()
        .split(',')
        .any(|value| value.trim().eq_ignore_ascii_case("origin")));
}

#[tokio::test]
async fn preflight_rejects_unregistered_subroutes_methods_and_headers() {
    let api = TestApi::new();
    for (path, method, headers) in [
        (
            "/api/v1/documents/not-an-id/unknown",
            "GET",
            "authorization",
        ),
        ("/api/v1/documents/not-an-id/", "GET", "authorization"),
        ("//api/v1/documents/not-an-id", "GET", "authorization"),
        ("/api/v1/documents//move", "POST", "authorization"),
        (
            "/api/v1/documents/id/history//restore",
            "POST",
            "authorization",
        ),
        ("/api/v1/documents", "PATCH", "authorization"),
        ("/api/v1/settings", "PATCH", "x-unsafe-header"),
    ] {
        let request = Request::builder()
            .method("OPTIONS")
            .uri(path)
            .header(header::HOST, HOST)
            .header(header::ORIGIN, ORIGIN)
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, method)
            .header(header::ACCESS_CONTROL_REQUEST_HEADERS, headers)
            .body(Body::empty())
            .unwrap();
        let response = api.router.clone().oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path}");
        assert!(response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none());
    }
}

#[tokio::test]
async fn registered_routes_return_safe_errors_for_unsupported_methods() {
    let api = TestApi::new();
    let mut request = api.authorized_request("POST", "/api/v1/runtime");
    request
        .headers_mut()
        .insert("x-request-id", "untrusted-client-id".parse().unwrap());
    let response = api.router.clone().oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_ne!(response.headers()["x-request-id"], "untrusted-client-id");
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let header_request_id = response.headers()["x-request-id"].clone();
    let envelope: ApiErrorEnvelope = serde_json::from_value(body_json(response).await).unwrap();
    assert_eq!(envelope.code(), ErrorCode::InvalidRequest);
    assert_eq!(
        header_request_id.to_str().unwrap(),
        envelope.request_id().as_uuid().to_string()
    );
}

#[tokio::test]
async fn implicit_head_routes_are_rejected_before_domain_delegation() {
    let api = TestApi::new();
    let response = api
        .router
        .clone()
        .oneshot(api.authorized_request("HEAD", "/api/v1/runtime"))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(api.runtime_calls.load(Ordering::SeqCst), 0);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
}

#[tokio::test]
async fn websocket_upgrade_rejections_use_the_safe_api_envelope() {
    let api = TestApi::new();
    let mut request = api.request("GET", "/api/v1/events");
    request
        .headers_mut()
        .insert(header::ORIGIN, ORIGIN.parse().unwrap());
    let response = api.router.clone().oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(
        response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
        ORIGIN
    );
    let envelope: ApiErrorEnvelope = serde_json::from_value(body_json(response).await).unwrap();
    assert_eq!(envelope.code(), ErrorCode::InvalidRequest);
}

#[tokio::test]
async fn unavailable_services_return_the_exact_safe_startup_details() {
    let api = TestApi::without_sync_service();
    let response = api
        .router
        .clone()
        .oneshot(api.authorized_request("GET", "/api/v1/sync/status"))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let envelope = body_json(response).await;
    assert_eq!(envelope["code"], "sync_not_ready");
    assert_eq!(
        envelope["details"],
        json!({ "type": "startup", "state": "starting" })
    );
}

#[tokio::test]
async fn service_errors_outside_the_operation_contract_become_internal_errors() {
    let api = TestApi::new();
    let response = api
        .router
        .clone()
        .oneshot(api.authorized_request("GET", "/api/v1/sync/status"))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let envelope = body_json(response).await;
    assert_eq!(envelope["code"], "internal_error");
    assert!(envelope.get("details").is_none());
}
