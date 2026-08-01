use std::borrow::Cow;

use axum::{
    body::{to_bytes, Body, Bytes},
    extract::{rejection::PathRejection, Path, Query, Request, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, patch, post},
    Json, Router,
};
use http_body_util::LengthLimitError;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use zeroize::Zeroize;

use crate::{
    contract::{
        ApiVersion, CreateDocumentRequest, CreateWorkspaceResourceBatchRequest,
        CreateWorkspaceResourceQuery, DocumentContents, DocumentId, DocumentName, ErrorCode,
        ErrorDetails, FileDocumentName, ListDocumentsQuery, ListWorkspaceInventoryQuery,
        LiveHealthResponse, LiveStatus, MoveDocumentRequest, PageQuery, PatchAppConfigStateRequest,
        ResourceId, ResourceKind, RunId, SearchWorkspaceQuery, SnapshotId, StartupState,
        UpdateDocumentRequest, WorkspaceRelativePath,
    },
    runtime::ServiceFailure,
};

use super::{api_error, is_api_path, resource_body, runtime, ws, ApiState};

const STANDARD_JSON_BODY_LIMIT: usize = 1024 * 1024;
const AUTH_JSON_BODY_LIMIT: usize = 16 * 1024;
const DOCUMENT_JSON_BODY_LIMIT: usize = 100 * 1024 * 1024;
const APP_CONFIG_JSON_BODY_LIMIT: usize = 64 * 1024 * 1024;
const RESOURCE_BODY_LIMIT: usize = crate::resources::MAX_RESOURCE_BODY_BYTES;

#[derive(Clone, Copy)]
enum ServiceOperation {
    Ready,
    System,
    Workspace,
    ListWorkspaceInventory,
    OpenWorkspaceResource,
    CreateWorkspaceResource,
    CreateWorkspaceResourceBatch,
    ListDocuments,
    CreateDocument,
    GetDocument,
    UpdateDocument,
    MoveDocument,
    DeleteDocument,
    ListDocumentHistory,
    GetDocumentHistory,
    RestoreDocumentHistory,
    SearchWorkspace,
    GetSettings,
    PatchSettings,
    GetAppConfig,
    PatchAppConfigState,
    GetSyncConfig,
    PatchSyncConfig,
    TestSyncConnection,
    GetSyncStatus,
    GetSyncRun,
    TriggerSyncRun,
}

pub(crate) fn router() -> Router<ApiState> {
    Router::new()
        .route("/api/v1/health/live", get(health_live))
        .route("/api/v1/health/ready", get(health_ready))
        .route("/api/v1/system/version", get(system_version))
        .route("/api/v1/runtime", get(runtime_state))
        .route("/api/v1/workspace", get(workspace))
        .route("/api/v1/inventory", get(list_workspace_inventory))
        .route(
            "/api/v1/resources/{resource_id}",
            get(open_workspace_resource),
        )
        .route(
            "/api/v1/documents/{document_id}/resources",
            post(create_workspace_resource),
        )
        .route(
            "/api/v1/documents/{document_id}/resource-batches",
            post(create_workspace_resource_batch),
        )
        .route(
            "/api/v1/documents",
            get(list_documents).post(create_document),
        )
        .route(
            "/api/v1/documents/{document_id}",
            get(get_document).put(update_document),
        )
        .route("/api/v1/documents/{document_id}/move", post(move_document))
        .route(
            "/api/v1/documents/{document_id}/delete",
            post(delete_document),
        )
        .route(
            "/api/v1/documents/{document_id}/history",
            get(list_document_history),
        )
        .route(
            "/api/v1/documents/{document_id}/history/{snapshot_id}",
            get(get_document_history),
        )
        .route(
            "/api/v1/documents/{document_id}/history/{snapshot_id}/restore",
            post(restore_document_history),
        )
        .route("/api/v1/search", get(search_workspace))
        .route("/api/v1/settings", get(get_settings).patch(patch_settings))
        .route("/api/v1/app-config", get(get_app_config))
        .route("/api/v1/app-config/state", patch(patch_app_config_state))
        .route(
            "/api/v1/sync/config",
            get(get_sync_config).patch(patch_sync_config),
        )
        .route("/api/v1/sync/connection-test", post(test_sync_connection))
        .route("/api/v1/sync/status", get(get_sync_status))
        .route("/api/v1/sync/runs", post(trigger_sync_run))
        .route("/api/v1/sync/runs/{run_id}", get(get_sync_run))
        .route("/api/v1/events", get(ws::upgrade))
        .method_not_allowed_fallback(method_not_allowed)
}

async fn health_live() -> Json<LiveHealthResponse> {
    Json(LiveHealthResponse {
        status: LiveStatus::Live,
        api_version: ApiVersion::V1,
    })
}

async fn health_ready(State(state): State<ApiState>) -> Response {
    let Some(service) = runtime(&state).system_api_service() else {
        return unavailable(ServiceOperation::Ready);
    };
    service_response(
        service.ready().await,
        StatusCode::OK,
        ServiceOperation::Ready,
    )
}

async fn system_version(State(state): State<ApiState>) -> Response {
    let Some(service) = runtime(&state).system_api_service() else {
        return unavailable(ServiceOperation::System);
    };
    service_response(
        service.version().await,
        StatusCode::OK,
        ServiceOperation::System,
    )
}

async fn runtime_state(State(state): State<ApiState>) -> Response {
    let Some(service) = runtime(&state).system_api_service() else {
        return unavailable(ServiceOperation::System);
    };
    service_response(
        service.runtime_state().await,
        StatusCode::OK,
        ServiceOperation::System,
    )
}

async fn workspace(State(state): State<ApiState>) -> Response {
    let Some(service) = runtime(&state).workspace_api_service() else {
        return unavailable(ServiceOperation::Workspace);
    };
    service_response(
        service.get_workspace().await,
        StatusCode::OK,
        ServiceOperation::Workspace,
    )
}

async fn list_workspace_inventory(
    State(state): State<ApiState>,
    parent_probe: Result<Query<ListDocumentsParentProbe>, axum::extract::rejection::QueryRejection>,
    query: Result<Query<ListWorkspaceInventoryQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let Ok(Query(parent_probe)) = parent_probe else {
        return api_error(ErrorCode::InvalidRequest, None);
    };
    if parent_probe
        .parent
        .as_deref()
        .is_some_and(|parent| WorkspaceRelativePath::parse(parent).is_err())
    {
        return api_error(ErrorCode::InvalidWorkspacePath, None);
    }
    let Ok(Query(query)) = query else {
        return api_error(ErrorCode::InvalidRequest, None);
    };
    let Some(service) = runtime(&state).resources_api_service() else {
        return unavailable(ServiceOperation::ListWorkspaceInventory);
    };
    service_response(
        service.list_workspace_inventory(query).await,
        StatusCode::OK,
        ServiceOperation::ListWorkspaceInventory,
    )
}

async fn open_workspace_resource(
    State(state): State<ApiState>,
    resource_id: Result<Path<ResourceId>, PathRejection>,
    query: Result<Query<OpenWorkspaceResourceQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let (Ok(Path(resource_id)), Ok(Query(query))) = (resource_id, query) else {
        return api_error(ErrorCode::InvalidRequest, None);
    };
    let Some(service) = runtime(&state).resources_api_service() else {
        return unavailable(ServiceOperation::OpenWorkspaceResource);
    };
    match service
        .open_workspace_resource(resource_id, query.kind)
        .await
    {
        Ok(resource) => resource_body::response(resource)
            .await
            .unwrap_or_else(|()| api_error(ErrorCode::WorkspaceUnavailable, None)),
        Err(error) => service_failure_response(error, ServiceOperation::OpenWorkspaceResource),
    }
}

async fn create_workspace_resource(
    State(state): State<ApiState>,
    document_id: Result<Path<DocumentId>, PathRejection>,
    query: Result<Query<CreateWorkspaceResourceQuery>, axum::extract::rejection::QueryRejection>,
    request: Request<Body>,
) -> Response {
    let (Ok(Path(document_id)), Ok(Query(query))) = (document_id, query) else {
        return api_error(ErrorCode::InvalidRequest, None);
    };
    let (media_type, body) = match read_resource_body(request).await {
        Ok(body) => body,
        Err(response) => return response,
    };
    let Some(service) = runtime(&state).resources_api_service() else {
        return unavailable(ServiceOperation::CreateWorkspaceResource);
    };
    service_response(
        service
            .create_workspace_resource(document_id, query, media_type, body.to_vec())
            .await,
        StatusCode::CREATED,
        ServiceOperation::CreateWorkspaceResource,
    )
}

async fn create_workspace_resource_batch(
    State(state): State<ApiState>,
    document_id: Result<Path<DocumentId>, PathRejection>,
    request: Request<Body>,
) -> Response {
    let Ok(Path(document_id)) = document_id else {
        return api_error(ErrorCode::InvalidRequest, None);
    };
    let request = match parse_json::<CreateWorkspaceResourceBatchRequest>(
        request,
        DOCUMENT_JSON_BODY_LIMIT,
        ErrorCode::ResourceTooLarge,
    )
    .await
    {
        Ok(request) => request,
        Err(response) => return response,
    };
    let Some(service) = runtime(&state).resources_api_service() else {
        return unavailable(ServiceOperation::CreateWorkspaceResourceBatch);
    };
    service_response(
        service
            .create_workspace_resource_batch(document_id, request)
            .await,
        StatusCode::CREATED,
        ServiceOperation::CreateWorkspaceResourceBatch,
    )
}

async fn list_documents(
    State(state): State<ApiState>,
    parent_probe: Result<Query<ListDocumentsParentProbe>, axum::extract::rejection::QueryRejection>,
    query: Result<Query<ListDocumentsQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let Ok(Query(parent_probe)) = parent_probe else {
        return api_error(ErrorCode::InvalidRequest, None);
    };
    if parent_probe
        .parent
        .as_deref()
        .is_some_and(|parent| WorkspaceRelativePath::parse(parent).is_err())
    {
        return api_error(ErrorCode::InvalidWorkspacePath, None);
    }
    let Ok(Query(query)) = query else {
        return api_error(ErrorCode::InvalidRequest, None);
    };
    let Some(service) = runtime(&state).documents_api_service() else {
        return unavailable(ServiceOperation::ListDocuments);
    };
    service_response(
        service.list_documents(query).await,
        StatusCode::OK,
        ServiceOperation::ListDocuments,
    )
}

async fn create_document(State(state): State<ApiState>, request: Request<Body>) -> Response {
    let request = match parse_create_document(request).await {
        Ok(request) => request,
        Err(response) => return response,
    };
    let Some(service) = runtime(&state).documents_api_service() else {
        return unavailable(ServiceOperation::CreateDocument);
    };
    service_response(
        service.create_document(request).await,
        StatusCode::CREATED,
        ServiceOperation::CreateDocument,
    )
}

async fn get_document(
    State(state): State<ApiState>,
    document_id: Result<Path<DocumentId>, PathRejection>,
) -> Response {
    let Ok(Path(document_id)) = document_id else {
        return api_error(ErrorCode::InvalidRequest, None);
    };
    let Some(service) = runtime(&state).documents_api_service() else {
        return unavailable(ServiceOperation::GetDocument);
    };
    service_response(
        service.get_document(document_id).await,
        StatusCode::OK,
        ServiceOperation::GetDocument,
    )
}

async fn update_document(
    State(state): State<ApiState>,
    document_id: Result<Path<DocumentId>, PathRejection>,
    request: Request<Body>,
) -> Response {
    let Ok(Path(document_id)) = document_id else {
        return api_error(ErrorCode::InvalidRequest, None);
    };
    let request = match parse_update_document(request).await {
        Ok(request) => request,
        Err(response) => return response,
    };
    let Some(service) = runtime(&state).documents_api_service() else {
        return unavailable(ServiceOperation::UpdateDocument);
    };
    service_response(
        service.update_document(document_id, request).await,
        StatusCode::OK,
        ServiceOperation::UpdateDocument,
    )
}

async fn move_document(
    State(state): State<ApiState>,
    document_id: Result<Path<DocumentId>, PathRejection>,
    request: Request<Body>,
) -> Response {
    let Ok(Path(document_id)) = document_id else {
        return api_error(ErrorCode::InvalidRequest, None);
    };
    let request = match parse_move_document(request).await {
        Ok(request) => request,
        Err(response) => return response,
    };
    let Some(service) = runtime(&state).documents_api_service() else {
        return unavailable(ServiceOperation::MoveDocument);
    };
    service_response(
        service.move_document(document_id, request).await,
        StatusCode::OK,
        ServiceOperation::MoveDocument,
    )
}

async fn delete_document(
    State(state): State<ApiState>,
    document_id: Result<Path<DocumentId>, PathRejection>,
    request: Request<Body>,
) -> Response {
    let Ok(Path(document_id)) = document_id else {
        return api_error(ErrorCode::InvalidRequest, None);
    };
    let request = match parse_standard_json(request).await {
        Ok(request) => request,
        Err(response) => return response,
    };
    let Some(service) = runtime(&state).documents_api_service() else {
        return unavailable(ServiceOperation::DeleteDocument);
    };
    match service.delete_document(document_id, request).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => service_failure_response(error, ServiceOperation::DeleteDocument),
    }
}

async fn list_document_history(
    State(state): State<ApiState>,
    document_id: Result<Path<DocumentId>, PathRejection>,
    query: Result<Query<PageQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let (Ok(Path(document_id)), Ok(Query(query))) = (document_id, query) else {
        return api_error(ErrorCode::InvalidRequest, None);
    };
    let Some(service) = runtime(&state).documents_api_service() else {
        return unavailable(ServiceOperation::ListDocumentHistory);
    };
    service_response(
        service.list_document_history(document_id, query).await,
        StatusCode::OK,
        ServiceOperation::ListDocumentHistory,
    )
}

async fn get_document_history(
    State(state): State<ApiState>,
    path: Result<Path<(DocumentId, SnapshotId)>, PathRejection>,
) -> Response {
    let Ok(Path((document_id, snapshot_id))) = path else {
        return api_error(ErrorCode::InvalidRequest, None);
    };
    let Some(service) = runtime(&state).documents_api_service() else {
        return unavailable(ServiceOperation::GetDocumentHistory);
    };
    service_response(
        service.get_document_history(document_id, snapshot_id).await,
        StatusCode::OK,
        ServiceOperation::GetDocumentHistory,
    )
}

async fn restore_document_history(
    State(state): State<ApiState>,
    path: Result<Path<(DocumentId, SnapshotId)>, PathRejection>,
    request: Request<Body>,
) -> Response {
    let Ok(Path((document_id, snapshot_id))) = path else {
        return api_error(ErrorCode::InvalidRequest, None);
    };
    let request = match parse_standard_json(request).await {
        Ok(request) => request,
        Err(response) => return response,
    };
    let Some(service) = runtime(&state).documents_api_service() else {
        return unavailable(ServiceOperation::RestoreDocumentHistory);
    };
    service_response(
        service
            .restore_document_history(document_id, snapshot_id, request)
            .await,
        StatusCode::OK,
        ServiceOperation::RestoreDocumentHistory,
    )
}

async fn search_workspace(
    State(state): State<ApiState>,
    query: Result<Query<SearchWorkspaceQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let Ok(Query(query)) = query else {
        return api_error(ErrorCode::InvalidRequest, None);
    };
    let Some(service) = runtime(&state).documents_api_service() else {
        return unavailable(ServiceOperation::SearchWorkspace);
    };
    service_response(
        service.search_workspace(query).await,
        StatusCode::OK,
        ServiceOperation::SearchWorkspace,
    )
}

async fn get_settings(State(state): State<ApiState>) -> Response {
    let Some(service) = runtime(&state).settings_api_service() else {
        return unavailable(ServiceOperation::GetSettings);
    };
    service_response(
        service.get_settings().await,
        StatusCode::OK,
        ServiceOperation::GetSettings,
    )
}

async fn patch_settings(State(state): State<ApiState>, request: Request<Body>) -> Response {
    let request = match parse_standard_json(request).await {
        Ok(request) => request,
        Err(response) => return response,
    };
    let Some(service) = runtime(&state).settings_api_service() else {
        return unavailable(ServiceOperation::PatchSettings);
    };
    service_response(
        service.patch_settings(request).await,
        StatusCode::OK,
        ServiceOperation::PatchSettings,
    )
}

async fn get_app_config(State(state): State<ApiState>) -> Response {
    let Some(service) = runtime(&state).app_config_api_service() else {
        return unavailable(ServiceOperation::GetAppConfig);
    };
    service_response(
        service.get_app_config().await,
        StatusCode::OK,
        ServiceOperation::GetAppConfig,
    )
}

async fn patch_app_config_state(State(state): State<ApiState>, request: Request<Body>) -> Response {
    let request = match parse_app_config_json(request).await {
        Ok(request) => request,
        Err(response) => return response,
    };
    let Some(service) = runtime(&state).app_config_api_service() else {
        return unavailable(ServiceOperation::PatchAppConfigState);
    };
    service_response(
        service.patch_app_config_state(request).await,
        StatusCode::OK,
        ServiceOperation::PatchAppConfigState,
    )
}

async fn get_sync_config(State(state): State<ApiState>) -> Response {
    let Some(service) = runtime(&state).sync_api_service() else {
        return unavailable(ServiceOperation::GetSyncConfig);
    };
    service_response(
        service.get_sync_config().await,
        StatusCode::OK,
        ServiceOperation::GetSyncConfig,
    )
}

async fn patch_sync_config(State(state): State<ApiState>, request: Request<Body>) -> Response {
    let request = match parse_sensitive_standard_json(request).await {
        Ok(request) => request,
        Err(response) => return response,
    };
    let Some(service) = runtime(&state).sync_api_service() else {
        return unavailable(ServiceOperation::PatchSyncConfig);
    };
    service_response(
        service.patch_sync_config(request).await,
        StatusCode::OK,
        ServiceOperation::PatchSyncConfig,
    )
}

async fn test_sync_connection(State(state): State<ApiState>, request: Request<Body>) -> Response {
    let request = match parse_sensitive_standard_json(request).await {
        Ok(request) => request,
        Err(response) => return response,
    };
    let Some(service) = runtime(&state).sync_api_service() else {
        return unavailable(ServiceOperation::TestSyncConnection);
    };
    service_response(
        service.test_sync_connection(request).await,
        StatusCode::OK,
        ServiceOperation::TestSyncConnection,
    )
}

async fn get_sync_status(State(state): State<ApiState>) -> Response {
    let Some(service) = runtime(&state).sync_api_service() else {
        return unavailable(ServiceOperation::GetSyncStatus);
    };
    service_response(
        service.get_sync_status().await,
        StatusCode::OK,
        ServiceOperation::GetSyncStatus,
    )
}

async fn trigger_sync_run(State(state): State<ApiState>, request: Request<Body>) -> Response {
    let request = match parse_standard_json(request).await {
        Ok(request) => request,
        Err(response) => return response,
    };
    let Some(service) = runtime(&state).sync_api_service() else {
        return unavailable(ServiceOperation::TriggerSyncRun);
    };
    service_response(
        service.trigger_sync_run(request).await,
        StatusCode::ACCEPTED,
        ServiceOperation::TriggerSyncRun,
    )
}

async fn get_sync_run(
    State(state): State<ApiState>,
    run_id: Result<Path<RunId>, PathRejection>,
) -> Response {
    let Ok(Path(run_id)) = run_id else {
        return api_error(ErrorCode::InvalidRequest, None);
    };
    let Some(service) = runtime(&state).sync_api_service() else {
        return unavailable(ServiceOperation::GetSyncRun);
    };
    service_response(
        service.get_sync_run(run_id).await,
        StatusCode::OK,
        ServiceOperation::GetSyncRun,
    )
}

async fn parse_standard_json<T>(request: Request<Body>) -> Result<T, Response>
where
    T: DeserializeOwned,
{
    parse_json(request, STANDARD_JSON_BODY_LIMIT, ErrorCode::InvalidRequest).await
}

async fn parse_app_config_json(
    request: Request<Body>,
) -> Result<PatchAppConfigStateRequest, Response> {
    let bytes = read_json_body(
        request,
        APP_CONFIG_JSON_BODY_LIMIT,
        ErrorCode::ResourceTooLarge,
    )
    .await?;
    serde_json::from_slice(&bytes).map_err(|_| api_error(ErrorCode::InvalidAppConfigState, None))
}

async fn parse_sensitive_standard_json<T>(request: Request<Body>) -> Result<T, Response>
where
    T: DeserializeOwned,
{
    let bytes =
        read_json_body(request, STANDARD_JSON_BODY_LIMIT, ErrorCode::InvalidRequest).await?;
    let mut bytes = match bytes.try_into_mut() {
        Ok(bytes) => bytes,
        Err(bytes) => bytes.into(),
    };
    let result = serde_json::from_slice(&bytes);
    bytes.as_mut().zeroize();
    result.map_err(|_| api_error(ErrorCode::InvalidRequest, None))
}

pub(crate) async fn parse_sensitive_auth_json<T>(request: Request<Body>) -> Result<T, Response>
where
    T: DeserializeOwned,
{
    let bytes = read_json_body(request, AUTH_JSON_BODY_LIMIT, ErrorCode::InvalidRequest).await?;
    let mut bytes = match bytes.try_into_mut() {
        Ok(bytes) => bytes,
        Err(bytes) => bytes.into(),
    };
    let result = serde_json::from_slice(&bytes);
    bytes.as_mut().zeroize();
    result.map_err(|_| api_error(ErrorCode::InvalidRequest, None))
}

async fn parse_create_document(request: Request<Body>) -> Result<CreateDocumentRequest, Response> {
    let bytes = read_json_body(
        request,
        DOCUMENT_JSON_BODY_LIMIT,
        ErrorCode::DocumentTooLarge,
    )
    .await?;
    let probe: CreateDocumentProbe<'_> =
        serde_json::from_slice(&bytes).map_err(|_| api_error(ErrorCode::InvalidRequest, None))?;
    let kind = probe
        .kind
        .as_deref()
        .ok_or_else(|| api_error(ErrorCode::InvalidRequest, None))?;
    let parent = probe
        .parent
        .as_deref()
        .ok_or_else(|| api_error(ErrorCode::InvalidRequest, None))?;
    if WorkspaceRelativePath::parse(parent).is_err() {
        return Err(api_error(ErrorCode::InvalidWorkspacePath, None));
    }
    let name = probe
        .name
        .as_deref()
        .ok_or_else(|| api_error(ErrorCode::InvalidRequest, None))?;
    let valid_name = match kind {
        "file" => FileDocumentName::parse(name).is_ok(),
        "directory" => DocumentName::parse(name).is_ok(),
        _ => return Err(api_error(ErrorCode::InvalidRequest, None)),
    };
    if !valid_name {
        return Err(api_error(ErrorCode::InvalidDocumentName, None));
    }
    if kind == "file"
        && probe
            .contents
            .as_deref()
            .is_some_and(DocumentContents::exceeds_limit)
    {
        return Err(api_error(ErrorCode::DocumentTooLarge, None));
    }
    serde_json::from_slice(&bytes).map_err(|_| api_error(ErrorCode::InvalidRequest, None))
}

async fn parse_update_document(request: Request<Body>) -> Result<UpdateDocumentRequest, Response> {
    let bytes = read_json_body(
        request,
        DOCUMENT_JSON_BODY_LIMIT,
        ErrorCode::DocumentTooLarge,
    )
    .await?;
    let probe: UpdateDocumentProbe<'_> =
        serde_json::from_slice(&bytes).map_err(|_| api_error(ErrorCode::InvalidRequest, None))?;
    if probe
        .contents
        .as_deref()
        .is_some_and(DocumentContents::exceeds_limit)
    {
        return Err(api_error(ErrorCode::DocumentTooLarge, None));
    }
    serde_json::from_slice(&bytes).map_err(|_| api_error(ErrorCode::InvalidRequest, None))
}

async fn parse_move_document(request: Request<Body>) -> Result<MoveDocumentRequest, Response> {
    let bytes =
        read_json_body(request, STANDARD_JSON_BODY_LIMIT, ErrorCode::InvalidRequest).await?;
    let probe: MoveDocumentProbe<'_> =
        serde_json::from_slice(&bytes).map_err(|_| api_error(ErrorCode::InvalidRequest, None))?;
    let target_parent = probe
        .target_parent
        .as_deref()
        .ok_or_else(|| api_error(ErrorCode::InvalidRequest, None))?;
    if WorkspaceRelativePath::parse(target_parent).is_err() {
        return Err(api_error(ErrorCode::InvalidWorkspacePath, None));
    }
    let name = probe
        .name
        .as_deref()
        .ok_or_else(|| api_error(ErrorCode::InvalidRequest, None))?;
    if DocumentName::parse(name).is_err() {
        return Err(api_error(ErrorCode::InvalidDocumentName, None));
    }
    serde_json::from_slice(&bytes).map_err(|_| api_error(ErrorCode::InvalidRequest, None))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateDocumentProbe<'a> {
    #[serde(borrow)]
    kind: Option<Cow<'a, str>>,
    #[serde(borrow)]
    parent: Option<Cow<'a, str>>,
    #[serde(borrow)]
    name: Option<Cow<'a, str>>,
    #[serde(borrow)]
    contents: Option<Cow<'a, str>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateDocumentProbe<'a> {
    #[serde(borrow)]
    contents: Option<Cow<'a, str>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MoveDocumentProbe<'a> {
    #[serde(borrow)]
    target_parent: Option<Cow<'a, str>>,
    #[serde(borrow)]
    name: Option<Cow<'a, str>>,
}

#[derive(Deserialize)]
struct ListDocumentsParentProbe {
    #[serde(default)]
    parent: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenWorkspaceResourceQuery {
    kind: ResourceKind,
}

async fn parse_json<T>(
    request: Request<Body>,
    limit: usize,
    oversized_code: ErrorCode,
) -> Result<T, Response>
where
    T: DeserializeOwned,
{
    let bytes = read_json_body(request, limit, oversized_code).await?;
    serde_json::from_slice(&bytes).map_err(|_| api_error(ErrorCode::InvalidRequest, None))
}

async fn read_json_body(
    request: Request<Body>,
    limit: usize,
    oversized_code: ErrorCode,
) -> Result<Bytes, Response> {
    if !request_has_json_content_type(&request) {
        return Err(api_error(ErrorCode::InvalidRequest, None));
    }
    let declared_length = request
        .headers()
        .get(header::CONTENT_LENGTH)
        .map(|value| {
            value
                .to_str()
                .ok()
                .filter(|value| {
                    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
                })
                .and_then(|value| value.parse::<usize>().ok())
                .ok_or_else(|| api_error(ErrorCode::InvalidRequest, None))
        })
        .transpose()?;
    if declared_length.is_some_and(|length| length > limit) {
        return Err(api_error(oversized_code, None));
    }
    to_bytes(request.into_body(), limit).await.map_err(|error| {
        let code = if error.into_inner().is::<LengthLimitError>() {
            oversized_code
        } else {
            ErrorCode::InvalidRequest
        };
        api_error(code, None)
    })
}

fn request_has_json_content_type(request: &Request<Body>) -> bool {
    request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"))
}

async fn read_resource_body(request: Request<Body>) -> Result<(String, Bytes), Response> {
    let media_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .filter(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "application/octet-stream"
                    | "image/gif"
                    | "image/jpeg"
                    | "image/png"
                    | "image/webp"
            )
        })
        .ok_or_else(|| api_error(ErrorCode::InvalidRequest, None))?
        .to_ascii_lowercase();
    let declared_length = request
        .headers()
        .get(header::CONTENT_LENGTH)
        .map(|value| {
            value
                .to_str()
                .ok()
                .filter(|value| {
                    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
                })
                .and_then(|value| value.parse::<usize>().ok())
                .ok_or_else(|| api_error(ErrorCode::InvalidRequest, None))
        })
        .transpose()?;
    if declared_length.is_some_and(|length| length > RESOURCE_BODY_LIMIT) {
        return Err(api_error(ErrorCode::ResourceTooLarge, None));
    }
    let body = to_bytes(request.into_body(), RESOURCE_BODY_LIMIT)
        .await
        .map_err(|error| {
            let code = if error.into_inner().is::<LengthLimitError>() {
                ErrorCode::ResourceTooLarge
            } else {
                ErrorCode::InvalidRequest
            };
            api_error(code, None)
        })?;
    if declared_length.is_some_and(|length| length != body.len()) {
        return Err(api_error(ErrorCode::InvalidRequest, None));
    }
    Ok((media_type, body))
}

impl ServiceOperation {
    fn allowed_errors(self) -> &'static [ErrorCode] {
        use ErrorCode as E;
        match self {
            Self::Ready | Self::System => &[E::KernelNotReady],
            Self::Workspace => &[
                E::KernelNotReady,
                E::WorkspaceUnavailable,
                E::WorkspaceLocked,
            ],
            Self::ListWorkspaceInventory => &[
                E::KernelNotReady,
                E::WorkspaceUnavailable,
                E::WorkspaceLocked,
                E::InvalidRequest,
                E::InvalidWorkspacePath,
            ],
            Self::OpenWorkspaceResource => &[
                E::KernelNotReady,
                E::WorkspaceUnavailable,
                E::WorkspaceLocked,
                E::InvalidRequest,
                E::ResourceNotFound,
            ],
            Self::CreateWorkspaceResource => &[
                E::KernelNotReady,
                E::WorkspaceUnavailable,
                E::WorkspaceLocked,
                E::InvalidRequest,
                E::DocumentNotFound,
                E::ResourceTooLarge,
                E::RevisionConflict,
            ],
            Self::CreateWorkspaceResourceBatch => &[
                E::KernelNotReady,
                E::WorkspaceUnavailable,
                E::WorkspaceLocked,
                E::InvalidRequest,
                E::DocumentNotFound,
                E::ResourceTooLarge,
                E::RevisionConflict,
            ],
            Self::ListDocuments => &[
                E::KernelNotReady,
                E::WorkspaceUnavailable,
                E::WorkspaceLocked,
                E::InvalidRequest,
                E::InvalidWorkspacePath,
            ],
            Self::CreateDocument => &[
                E::KernelNotReady,
                E::WorkspaceUnavailable,
                E::WorkspaceLocked,
                E::InvalidRequest,
                E::InvalidWorkspacePath,
                E::InvalidDocumentName,
                E::DocumentAlreadyExists,
                E::DocumentTooLarge,
                E::DocumentInvalidEncoding,
                E::RevisionConflict,
            ],
            Self::GetDocument => &[
                E::KernelNotReady,
                E::WorkspaceUnavailable,
                E::WorkspaceLocked,
                E::InvalidRequest,
                E::DocumentNotFound,
                E::DocumentInvalidEncoding,
            ],
            Self::UpdateDocument => &[
                E::KernelNotReady,
                E::WorkspaceUnavailable,
                E::WorkspaceLocked,
                E::InvalidRequest,
                E::DocumentNotFound,
                E::DocumentTooLarge,
                E::DocumentInvalidEncoding,
                E::RevisionConflict,
            ],
            Self::MoveDocument => &[
                E::KernelNotReady,
                E::WorkspaceUnavailable,
                E::WorkspaceLocked,
                E::InvalidRequest,
                E::InvalidWorkspacePath,
                E::InvalidDocumentName,
                E::DocumentNotFound,
                E::DocumentAlreadyExists,
                E::RevisionConflict,
            ],
            Self::DeleteDocument => &[
                E::KernelNotReady,
                E::WorkspaceUnavailable,
                E::WorkspaceLocked,
                E::InvalidRequest,
                E::DocumentNotFound,
                E::RevisionConflict,
            ],
            Self::ListDocumentHistory => &[
                E::KernelNotReady,
                E::WorkspaceUnavailable,
                E::WorkspaceLocked,
                E::InvalidRequest,
                E::DocumentNotFound,
            ],
            Self::GetDocumentHistory => &[
                E::KernelNotReady,
                E::WorkspaceUnavailable,
                E::WorkspaceLocked,
                E::InvalidRequest,
                E::DocumentNotFound,
                E::DocumentTooLarge,
                E::DocumentInvalidEncoding,
            ],
            Self::RestoreDocumentHistory => &[
                E::KernelNotReady,
                E::WorkspaceUnavailable,
                E::WorkspaceLocked,
                E::InvalidRequest,
                E::DocumentNotFound,
                E::DocumentTooLarge,
                E::DocumentInvalidEncoding,
                E::RevisionConflict,
            ],
            Self::SearchWorkspace => &[
                E::KernelNotReady,
                E::WorkspaceUnavailable,
                E::WorkspaceLocked,
                E::InvalidRequest,
                E::DocumentInvalidEncoding,
            ],
            Self::GetSettings => &[E::SettingsUnavailable],
            Self::PatchSettings => &[
                E::InvalidRequest,
                E::SettingsUnavailable,
                E::SettingsRevisionConflict,
                E::InvalidSettingsField,
            ],
            Self::GetAppConfig => &[E::AppConfigUnavailable],
            Self::PatchAppConfigState => &[
                E::ResourceTooLarge,
                E::InvalidAppConfigState,
                E::WorkspaceGenerationStale,
                E::AppConfigUnavailable,
            ],
            Self::GetSyncConfig => &[E::SyncConfigAbsent, E::SyncConfigInvalid],
            Self::PatchSyncConfig | Self::TestSyncConnection => &[
                E::InvalidRequest,
                E::SyncConfigAbsent,
                E::SyncConfigInvalid,
                E::SyncConfigRevisionConflict,
                E::SyncNotReady,
            ],
            Self::GetSyncStatus => &[E::SyncNotReady],
            Self::GetSyncRun => &[E::InvalidRequest, E::SyncNotReady, E::ResourceNotFound],
            Self::TriggerSyncRun => &[
                E::InvalidRequest,
                E::SyncNotReady,
                E::SyncRunUnavailable,
                E::SyncConfigRevisionConflict,
            ],
        }
    }

    fn missing_service_error(self) -> ErrorCode {
        match self {
            Self::Ready
            | Self::System
            | Self::Workspace
            | Self::ListWorkspaceInventory
            | Self::OpenWorkspaceResource
            | Self::CreateWorkspaceResource
            | Self::CreateWorkspaceResourceBatch
            | Self::ListDocuments
            | Self::CreateDocument
            | Self::GetDocument
            | Self::UpdateDocument
            | Self::MoveDocument
            | Self::DeleteDocument
            | Self::ListDocumentHistory
            | Self::GetDocumentHistory
            | Self::RestoreDocumentHistory
            | Self::SearchWorkspace => ErrorCode::KernelNotReady,
            Self::GetSettings | Self::PatchSettings => ErrorCode::SettingsUnavailable,
            Self::GetAppConfig | Self::PatchAppConfigState => ErrorCode::AppConfigUnavailable,
            Self::GetSyncConfig => ErrorCode::SyncConfigAbsent,
            Self::PatchSyncConfig
            | Self::TestSyncConnection
            | Self::GetSyncStatus
            | Self::GetSyncRun
            | Self::TriggerSyncRun => ErrorCode::SyncNotReady,
        }
    }
}

fn unavailable(operation: ServiceOperation) -> Response {
    let code = operation.missing_service_error();
    let details = (!matches!(code, ErrorCode::SyncConfigAbsent)).then_some(ErrorDetails::Startup {
        state: StartupState::Starting,
    });
    api_error(code, details)
}

fn service_response<T>(
    result: Result<T, ServiceFailure>,
    status: StatusCode,
    operation: ServiceOperation,
) -> Response
where
    T: Serialize,
{
    match result {
        Ok(value) => (status, Json(value)).into_response(),
        Err(error) => service_failure_response(error, operation),
    }
}

fn service_failure_response(error: ServiceFailure, operation: ServiceOperation) -> Response {
    if error.code() == ErrorCode::InternalError
        || operation.allowed_errors().contains(&error.code())
    {
        api_error(error.code(), error.details().cloned())
    } else {
        api_error(ErrorCode::InternalError, None)
    }
}

pub(crate) async fn not_found(request: Request) -> Response {
    if is_api_path(request.uri().path()) {
        api_error(ErrorCode::InvalidRequest, None)
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

async fn method_not_allowed() -> Response {
    api_error(ErrorCode::InvalidRequest, None)
}
