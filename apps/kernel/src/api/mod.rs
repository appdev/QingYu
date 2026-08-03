mod auth;
mod resource_body;
mod routes;
mod web;
pub mod ws;

pub(crate) use auth::ServerApiHost;
pub use auth::{
    ServerApiActivation, ServerApiActivationError, ServerApiProcess, ServerApiProcessError,
};

#[cfg(test)]
mod server_auth_tests;
#[cfg(test)]
mod web_tests;

use std::{
    fmt,
    net::SocketAddr,
    path::Path,
    sync::{Arc, Mutex as StdMutex, MutexGuard as StdMutexGuard},
};

use axum::{
    body::Body,
    extract::{Request, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode, Uri},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    Json, Router,
};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    contract::{
        ApiErrorEnvelope, AppConfigSnapshotDto, BindSyncRepositoryRequest,
        ChangeServerOwnerPasswordRequest, ConnectionId, CreateDocumentRequest,
        CreateServerSessionRequest, CreateWorkspaceResourceBatchItem,
        CreateWorkspaceResourceBatchRequest, CreateWorkspaceResourceBatchResponse,
        CreateWorkspaceResourceQuery, CreatedDocumentDto, DejavuKeyStateDto, DeleteDocumentRequest,
        DocumentContentDto, DocumentContents, DocumentEntryDto, DocumentHistoryPageDto,
        DocumentHistorySnapshotDto, DocumentId, DocumentPageDto, DomainEvent, ErrorCode,
        ErrorDetails, EventSequence, ExportDejavuKeyRequest, ExportedDejavuKeyDto, GapReason,
        ImportDejavuKeyRequest, InitializeServerOwnerRequest, InstanceId, ListDocumentsQuery,
        ListRemoteNotebooksQuery, ListWorkspaceInventoryQuery, LiveHealthResponse,
        MoveDocumentRequest, PageQuery, PatchAppConfigStateRequest, PatchSettingsRequest,
        PatchSyncConfigRequest, ProtocolVersion, ReadyHealthResponse, ReadySequence, ReloadScope,
        RemoteNotebookCatalogDto, RemoteNotebookCatalogEntryDto, RequestId, ResourceEntryDto,
        ResourceKind, ResourceRefDto, RestoreDocumentHistoryRequest, Revision, SearchPageDto,
        SearchWorkspaceQuery, ServerAuthenticationStatusDto, ServerFrame, ServerSessionDto,
        SettingsSnapshotDto, SnapshotRequired, SyncConfigViewDto, SyncConnectionTestDto,
        SyncRepositoryBindingDto, SyncRepositoryBindingViewDto, SyncRunAcceptedDto,
        SyncRunStatusDto, SyncSafeErrorDto, SyncStatusDto, SystemVersionResponse,
        TestSyncConnectionRequest, TriggerSyncRunRequest, UpdateDocumentRequest, WorkspaceDto,
        WorkspaceInventoryEntryDto, WorkspaceInventoryPageDto,
    },
    error::{http_status_for_error_code, safe_error_envelope},
    runtime::KernelRuntime,
    server::ServerKernelLifecycle,
};

const API_PREFIX: &str = "/api/v1/";
const API_ROOT: &str = "/api/v1";
const LIVE_PATH: &str = "/api/v1/health/live";
const EVENTS_PATH: &str = "/api/v1/events";

#[derive(Clone)]
pub struct TransportPolicy {
    host: HeaderValue,
    origin: HeaderValue,
    secure_cookies: bool,
}

impl TransportPolicy {
    pub fn loopback(host: &str, origin: &str) -> Result<Self, InvalidTransportPolicy> {
        let address = host
            .parse::<SocketAddr>()
            .map_err(|_| InvalidTransportPolicy)?;
        if address.ip().to_string() != "127.0.0.1" || address.port() == 0 || origin == "*" {
            return Err(InvalidTransportPolicy);
        }
        let host = HeaderValue::from_str(host).map_err(|_| InvalidTransportPolicy)?;
        let origin = HeaderValue::from_str(origin).map_err(|_| InvalidTransportPolicy)?;
        Ok(Self {
            host,
            origin,
            secure_cookies: false,
        })
    }

    pub fn same_origin(host: &str, origin: &str) -> Result<Self, InvalidTransportPolicy> {
        let host = HeaderValue::from_str(host).map_err(|_| InvalidTransportPolicy)?;
        let exact_host = host.to_str().map_err(|_| InvalidTransportPolicy)?;
        let uri = origin.parse::<Uri>().map_err(|_| InvalidTransportPolicy)?;
        let scheme = uri.scheme_str().ok_or(InvalidTransportPolicy)?;
        if !matches!(scheme, "http" | "https")
            || uri.authority().map(|authority| authority.as_str()) != Some(exact_host)
            || uri.path() != "/"
            || uri.query().is_some()
            || origin != format!("{scheme}://{exact_host}")
        {
            return Err(InvalidTransportPolicy);
        }
        let secure_cookies = scheme == "https";
        let origin = HeaderValue::from_str(origin).map_err(|_| InvalidTransportPolicy)?;
        Ok(Self {
            host,
            origin,
            secure_cookies,
        })
    }
}

impl fmt::Debug for TransportPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TransportPolicy")
            .field("host", &self.host)
            .field("origin", &self.origin)
            .field("secure_cookies", &self.secure_cookies)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidTransportPolicy;

impl fmt::Display for InvalidTransportPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("transport policy must use an exact allowed host and origin")
    }
}

impl std::error::Error for InvalidTransportPolicy {}

#[derive(Clone)]
pub(crate) struct ApiState {
    runtime: Arc<KernelRuntime>,
    policy: TransportPolicy,
    server: Option<ServerApiHost>,
    web: Option<web::ServerWebAssets>,
    _kernel_lifecycle: Option<ServerKernelLifecycle>,
    connection_lifecycle: Option<ApiConnectionLifecycle>,
}

pub fn build_router(runtime: Arc<KernelRuntime>, policy: TransportPolicy) -> Router {
    build_router_with_server(runtime, policy, None, None, None, None)
}

pub(crate) fn build_router_with_connection_lifecycle(
    runtime: Arc<KernelRuntime>,
    policy: TransportPolicy,
    connection_lifecycle: ApiConnectionLifecycle,
) -> Router {
    build_router_with_server(
        runtime,
        policy,
        None,
        None,
        None,
        Some(connection_lifecycle),
    )
}

pub fn build_server_router(activation: ServerApiActivation, policy: TransportPolicy) -> Router {
    let (runtime, server, lifecycle) = activation.into_parts();
    build_router_with_server(runtime, policy, Some(server), None, lifecycle, None)
}

pub fn build_server_web_router(
    activation: ServerApiActivation,
    policy: TransportPolicy,
    web_root: impl AsRef<Path>,
) -> Result<Router, InvalidServerWebAssets> {
    let web = web::ServerWebAssets::open(web_root.as_ref())?;
    let (runtime, server, lifecycle) = activation.into_parts();
    Ok(build_router_with_server(
        runtime,
        policy,
        Some(server),
        Some(web),
        lifecycle,
        None,
    ))
}

fn build_router_with_server(
    runtime: Arc<KernelRuntime>,
    policy: TransportPolicy,
    server: Option<ServerApiHost>,
    web: Option<web::ServerWebAssets>,
    kernel_lifecycle: Option<ServerKernelLifecycle>,
    connection_lifecycle: Option<ApiConnectionLifecycle>,
) -> Router {
    let state = ApiState {
        runtime,
        policy,
        server,
        web,
        _kernel_lifecycle: kernel_lifecycle,
        connection_lifecycle,
    };
    let router = if state.server.is_some() {
        routes::router().merge(auth::router())
    } else {
        routes::router()
    };
    router
        .fallback(web::fallback)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            enforce_transport,
        ))
        .with_state(state)
}

#[derive(Clone)]
pub(crate) struct ApiConnectionLifecycle {
    inner: Arc<ApiConnectionLifecycleInner>,
}

impl ApiConnectionLifecycle {
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(ApiConnectionLifecycleInner {
                state: StdMutex::new(ApiConnectionLifecycleState {
                    active: 0,
                    closing: false,
                }),
                changed: tokio::sync::Notify::new(),
            }),
        }
    }

    pub(crate) fn register(&self) -> Option<ApiConnectionGuard> {
        let mut state = lock_connection_state(&self.inner.state);
        if state.closing {
            return None;
        }
        let Some(active) = state.active.checked_add(1) else {
            state.closing = true;
            drop(state);
            self.inner.changed.notify_waiters();
            return None;
        };
        state.active = active;
        Some(ApiConnectionGuard {
            lifecycle: self.clone(),
        })
    }

    pub(crate) fn begin_shutdown(&self) {
        lock_connection_state(&self.inner.state).closing = true;
        self.inner.changed.notify_waiters();
    }

    pub(crate) async fn wait_drained(&self) {
        loop {
            let changed = self.inner.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if lock_connection_state(&self.inner.state).active == 0 {
                return;
            }
            changed.await;
        }
    }

    fn release(&self) {
        let drained = {
            let mut state = lock_connection_state(&self.inner.state);
            debug_assert!(state.active > 0);
            state.active = state.active.saturating_sub(1);
            state.active == 0
        };
        if drained {
            self.inner.changed.notify_waiters();
        }
    }
}

struct ApiConnectionLifecycleInner {
    state: StdMutex<ApiConnectionLifecycleState>,
    changed: tokio::sync::Notify,
}

struct ApiConnectionLifecycleState {
    active: usize,
    closing: bool,
}

pub(crate) struct ApiConnectionGuard {
    lifecycle: ApiConnectionLifecycle,
}

impl ApiConnectionGuard {
    pub(crate) async fn cancelled(&self) {
        loop {
            let changed = self.lifecycle.inner.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if lock_connection_state(&self.lifecycle.inner.state).closing {
                return;
            }
            changed.await;
        }
    }
}

impl Drop for ApiConnectionGuard {
    fn drop(&mut self) {
        self.lifecycle.release();
    }
}

fn lock_connection_state<T>(lock: &StdMutex<T>) -> StdMutexGuard<'_, T> {
    lock.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

async fn enforce_transport(
    State(state): State<ApiState>,
    mut request: Request,
    next: Next,
) -> Response {
    if !has_one_exact_header(request.headers(), header::HOST, &state.policy.host) {
        return api_error(ErrorCode::HostNotAllowed, None);
    }

    let origin = request.headers().get_all(header::ORIGIN);
    let origin_count = origin.iter().count();
    let has_allowed_origin =
        origin_count == 1 && origin.iter().next() == Some(&state.policy.origin);
    if origin_count > 0 && !has_allowed_origin {
        return api_error(ErrorCode::OriginNotAllowed, None);
    }

    if request.method() == Method::OPTIONS {
        if !has_allowed_origin {
            return api_error(ErrorCode::OriginNotAllowed, None);
        }
        return preflight_response(request, &state.policy);
    }

    if request.uri().path().starts_with("/api/v1/auth/")
        && !route_accepts_method(request.uri().path(), request.method())
    {
        let mut response = api_error(ErrorCode::InvalidRequest, None);
        decorate_response(
            &mut response,
            has_allowed_origin.then_some(&state.policy.origin),
        );
        return response;
    }

    if request.uri().path() == EVENTS_PATH {
        if !has_allowed_origin {
            return api_error(ErrorCode::OriginNotAllowed, None);
        }
        if let Some(server) = state.server.as_ref() {
            let intent = crate::server::RequestIntent::ReadOnly;
            let credentials = match parse_browser_credentials(
                request.headers(),
                intent,
                state.policy.secure_cookies,
            ) {
                Ok(credentials) => credentials,
                Err(error) => {
                    let mut response = auth::operation_error_response(error);
                    decorate_response(
                        &mut response,
                        has_allowed_origin.then_some(&state.policy.origin),
                    );
                    return response;
                }
            };
            match authenticate_browser_request(server, credentials, intent).await {
                Ok(Some(session)) => {
                    request.extensions_mut().insert(session);
                }
                Ok(None) => {}
                Err(error) => {
                    let mut response = auth::operation_error_response(error);
                    decorate_response(
                        &mut response,
                        has_allowed_origin.then_some(&state.policy.origin),
                    );
                    return response;
                }
            }
        }
    } else if (state.web.is_none() || is_api_namespace_path(request.uri().path()))
        && request.uri().path() != LIVE_PATH
        && !(state.server.is_some()
            && auth::is_public_route(request.uri().path(), request.method()))
    {
        let native_authorized = has_valid_bearer(request.headers(), &state.runtime)
            && !state
                .server
                .as_ref()
                .is_some_and(|_| auth::is_browser_only_route(request.uri().path()));
        if !native_authorized {
            let Some(server) = state.server.as_ref() else {
                let mut response = api_error(ErrorCode::Unauthorized, None);
                decorate_response(
                    &mut response,
                    has_allowed_origin.then_some(&state.policy.origin),
                );
                return response;
            };
            let intent = auth::request_intent(request.method());
            let credentials = match parse_browser_credentials(
                request.headers(),
                intent,
                state.policy.secure_cookies,
            ) {
                Ok(credentials) => credentials,
                Err(error) => {
                    let mut response = auth::operation_error_response(error);
                    decorate_response(
                        &mut response,
                        has_allowed_origin.then_some(&state.policy.origin),
                    );
                    return response;
                }
            };
            match authenticate_browser_request(server, credentials, intent).await {
                Ok(Some(session)) => {
                    request.extensions_mut().insert(session);
                }
                Ok(None)
                | Err(auth::ServerApiOperationError::Authentication(
                    crate::server::ServerAuthenticationCoordinatorError::InvalidSession,
                )) => {
                    let mut response = api_error(ErrorCode::Unauthorized, None);
                    decorate_response(
                        &mut response,
                        has_allowed_origin.then_some(&state.policy.origin),
                    );
                    return response;
                }
                Err(error) => {
                    let mut response = auth::operation_error_response(error);
                    decorate_response(
                        &mut response,
                        has_allowed_origin.then_some(&state.policy.origin),
                    );
                    return response;
                }
            }
        }
    }

    if is_api_path(request.uri().path())
        && !route_accepts_method(request.uri().path(), request.method())
    {
        let mut response = api_error(ErrorCode::InvalidRequest, None);
        decorate_response(
            &mut response,
            has_allowed_origin.then_some(&state.policy.origin),
        );
        return response;
    }

    let mut response = next.run(request).await;
    decorate_response(
        &mut response,
        has_allowed_origin.then_some(&state.policy.origin),
    );
    response
}

fn has_one_exact_header(
    headers: &HeaderMap,
    name: header::HeaderName,
    expected: &HeaderValue,
) -> bool {
    let values = headers.get_all(name);
    values.iter().count() == 1 && values.iter().next() == Some(expected)
}

fn has_valid_bearer(headers: &HeaderMap, runtime: &KernelRuntime) -> bool {
    let values = headers.get_all(header::AUTHORIZATION);
    if values.iter().count() != 1 {
        return false;
    }
    values
        .iter()
        .next()
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|candidate| runtime.matches_native_launch_credential(candidate))
}

async fn authenticate_browser_request(
    server: &ServerApiHost,
    credentials: Option<(
        auth::BrowserSessionCredential,
        Option<auth::BrowserCsrfProof>,
    )>,
    intent: crate::server::RequestIntent,
) -> Result<Option<auth::AuthenticatedBrowserSession>, auth::ServerApiOperationError> {
    let Some((credential, csrf)) = credentials else {
        return Ok(None);
    };
    server
        .authorize_browser_session(credential, csrf, intent)
        .await
        .map(Some)
}

fn parse_browser_credentials(
    headers: &HeaderMap,
    intent: crate::server::RequestIntent,
    secure_cookies: bool,
) -> Result<
    Option<(
        auth::BrowserSessionCredential,
        Option<auth::BrowserCsrfProof>,
    )>,
    auth::ServerApiOperationError,
> {
    auth::browser_credentials(headers, intent, secure_cookies).map_err(|error| {
        auth::ServerApiOperationError::Authentication(match error {
            auth::BrowserCredentialParseError::Session => {
                crate::server::ServerAuthenticationCoordinatorError::InvalidSession
            }
            auth::BrowserCredentialParseError::Csrf => {
                crate::server::ServerAuthenticationCoordinatorError::CsrfRejected
            }
        })
    })
}

fn preflight_response(request: Request<Body>, policy: &TransportPolicy) -> Response {
    let requested_methods = request
        .headers()
        .get_all(header::ACCESS_CONTROL_REQUEST_METHOD);
    if requested_methods.iter().count() != 1 {
        return api_error(ErrorCode::InvalidRequest, None);
    }
    let Some(requested_method) = requested_methods
        .iter()
        .next()
        .and_then(|value| value.to_str().ok())
        .and_then(|value| Method::from_bytes(value.as_bytes()).ok())
    else {
        return api_error(ErrorCode::InvalidRequest, None);
    };
    if !route_accepts_method(request.uri().path(), &requested_method)
        || !requested_headers_are_allowed(request.headers())
    {
        return api_error(ErrorCode::InvalidRequest, None);
    }

    let mut response = StatusCode::NO_CONTENT.into_response();
    let headers = response.headers_mut();
    headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, policy.origin.clone());
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_str(requested_method.as_str()).expect("HTTP method is a valid header"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("Authorization, Content-Type, X-CSRF-Token"),
    );
    headers.insert(header::VARY, HeaderValue::from_static("Origin"));
    decorate_response(&mut response, None);
    response
}

fn requested_headers_are_allowed(headers: &HeaderMap) -> bool {
    let values = headers.get_all(header::ACCESS_CONTROL_REQUEST_HEADERS);
    if values.iter().count() > 1 {
        return false;
    }
    values.iter().next().is_none_or(|value| {
        value.to_str().is_ok_and(|value| {
            value.split(',').all(|header| {
                matches!(
                    header.trim().to_ascii_lowercase().as_str(),
                    "authorization" | "content-type" | "x-csrf-token"
                )
            })
        })
    })
}

fn route_accepts_method(path: &str, method: &Method) -> bool {
    let accepted: &[Method] = match path {
        "/api/v1/health/live"
        | "/api/v1/auth/status"
        | "/api/v1/health/ready"
        | "/api/v1/system/version"
        | "/api/v1/runtime"
        | "/api/v1/workspace"
        | "/api/v1/inventory"
        | "/api/v1/search"
        | "/api/v1/app-config"
        | "/api/v1/sync/repositories"
        | "/api/v1/sync/dejavu/key"
        | "/api/v1/sync/status"
        | "/api/v1/events" => &[Method::GET],
        "/api/v1/auth/initialize" | "/api/v1/auth/logout" => &[Method::POST],
        "/api/v1/auth/session" | "/api/v1/sync/repository-binding" => &[Method::GET, Method::POST],
        "/api/v1/auth/password" => &[Method::PATCH],
        "/api/v1/documents" => &[Method::GET, Method::POST],
        "/api/v1/settings" | "/api/v1/sync/config" => &[Method::GET, Method::PATCH],
        "/api/v1/app-config/state" => &[Method::PATCH],
        "/api/v1/sync/connection-test"
        | "/api/v1/sync/runs"
        | "/api/v1/sync/dejavu/key/import"
        | "/api/v1/sync/dejavu/key/export" => &[Method::POST],
        _ => match path.split('/').collect::<Vec<_>>().as_slice() {
            ["", "api", "v1", "resources", resource_id] if !resource_id.is_empty() => {
                &[Method::GET]
            }
            ["", "api", "v1", "sync", "runs", run_id] if !run_id.is_empty() => &[Method::GET],
            ["", "api", "v1", "documents", document_id] if !document_id.is_empty() => {
                &[Method::GET, Method::PUT]
            }
            ["", "api", "v1", "documents", document_id, "move" | "delete"]
                if !document_id.is_empty() =>
            {
                &[Method::POST]
            }
            ["", "api", "v1", "documents", document_id, "resources"] if !document_id.is_empty() => {
                &[Method::POST]
            }
            ["", "api", "v1", "documents", document_id, "resource-batches"]
                if !document_id.is_empty() =>
            {
                &[Method::POST]
            }
            ["", "api", "v1", "documents", document_id, "history"] if !document_id.is_empty() => {
                &[Method::GET]
            }
            ["", "api", "v1", "documents", document_id, "history", snapshot_id]
                if !document_id.is_empty() && !snapshot_id.is_empty() =>
            {
                &[Method::GET]
            }
            ["", "api", "v1", "documents", document_id, "history", snapshot_id, "restore"]
                if !document_id.is_empty() && !snapshot_id.is_empty() =>
            {
                &[Method::POST]
            }
            _ => &[],
        },
    };
    accepted.contains(method)
}

fn decorate_response(response: &mut Response, allowed_origin: Option<&HeaderValue>) {
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    if !headers.contains_key("x-request-id") {
        headers.insert(
            "x-request-id",
            HeaderValue::from_str(&Uuid::new_v4().to_string())
                .expect("UUID request ID is a valid header"),
        );
    }
    if let Some(origin) = allowed_origin {
        headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin.clone());
        headers.insert(
            header::ACCESS_CONTROL_EXPOSE_HEADERS,
            HeaderValue::from_static(
                "Retry-After, X-Request-Id, X-Content-Type-Options, X-Resource-Revision",
            ),
        );
        headers.insert(header::VARY, HeaderValue::from_static("Origin"));
    }
}

pub(crate) fn api_error(code: ErrorCode, details: Option<ErrorDetails>) -> Response {
    let request_id = RequestId::new(Uuid::new_v4());
    let envelope = safe_error_envelope(code, request_id, details).unwrap_or_else(|_| {
        safe_error_envelope(ErrorCode::InternalError, request_id, None).unwrap()
    });
    let status = StatusCode::from_u16(http_status_for_error_code(envelope.code()))
        .expect("contract status codes are valid HTTP statuses");
    let mut response = (status, Json::<ApiErrorEnvelope>(envelope)).into_response();
    response.headers_mut().insert(
        "x-request-id",
        HeaderValue::from_str(&request_id.as_uuid().to_string())
            .expect("UUID request ID is a valid header"),
    );
    decorate_response(&mut response, None);
    response
}

pub(crate) fn runtime(state: &ApiState) -> &Arc<KernelRuntime> {
    &state.runtime
}

pub(crate) fn is_api_path(path: &str) -> bool {
    path == API_ROOT || path.starts_with(API_PREFIX)
}

pub(crate) fn is_api_namespace_path(path: &str) -> bool {
    path == "/api" || path.starts_with("/api/")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidServerWebAssets;

impl fmt::Display for InvalidServerWebAssets {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("server Web assets are unavailable")
    }
}

impl std::error::Error for InvalidServerWebAssets {}

pub struct ApiDoc;

impl ApiDoc {
    pub fn openapi() -> serde_json::Value {
        let mut document = serde_json::to_value(<SchemaApiDoc as utoipa::OpenApi>::openapi())
            .expect("the static OpenAPI schema must serialize");
        install_paths(&mut document);
        install_operation_inputs(&mut document);
        install_operation_errors(&mut document);
        install_security_scheme(&mut document);
        patch_literal_and_nullable_schemas(&mut document);
        document["x-cors-exposed-response-headers"] =
            serde_json::json!(["Retry-After", "X-Request-Id", "X-Resource-Revision"]);
        document
    }
}

pub fn export_openapi_to_string() -> Result<String, OpenApiExportError> {
    let mut output = serde_json::to_string_pretty(&ApiDoc::openapi())
        .map_err(|_| OpenApiExportError::Serialization)?;
    output.push('\n');
    Ok(output)
}

pub fn check_openapi_artifact(path: &std::path::Path) -> Result<(), OpenApiExportError> {
    let actual = std::fs::read(path).map_err(|_| OpenApiExportError::Read)?;
    let expected = export_openapi_to_string()?;
    if actual == expected.as_bytes() {
        Ok(())
    } else {
        Err(OpenApiExportError::Drift)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpenApiExportError {
    Read,
    Serialization,
    Drift,
}

impl fmt::Display for OpenApiExportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read => formatter.write_str("the OpenAPI artifact could not be read"),
            Self::Serialization => {
                formatter.write_str("the OpenAPI document could not be serialized")
            }
            Self::Drift => {
                formatter.write_str("the OpenAPI artifact differs from the Rust contract")
            }
        }
    }
}

impl std::error::Error for OpenApiExportError {}

#[derive(utoipa::OpenApi)]
#[openapi(
    info(title = "QingYu Kernel API", version = "1.0.0"),
    components(schemas(
        ApiErrorEnvelope,
        ErrorCode,
        LiveHealthResponse,
        ReadyHealthResponse,
        SystemVersionResponse,
        ServerAuthenticationStatusDto,
        ServerSessionDto,
        InitializeServerOwnerRequest,
        CreateServerSessionRequest,
        ChangeServerOwnerPasswordRequest,
        crate::contract::RuntimeStateDto,
        WorkspaceDto,
        ListWorkspaceInventoryQuery,
        CreateWorkspaceResourceQuery,
        CreateWorkspaceResourceBatchItem,
        CreateWorkspaceResourceBatchRequest,
        CreateWorkspaceResourceBatchResponse,
        WorkspaceInventoryEntryDto,
        WorkspaceInventoryPageDto,
        ResourceEntryDto,
        ResourceKind,
        PageQuery,
        ListDocumentsQuery,
        DocumentEntryDto,
        DocumentContentDto,
        CreatedDocumentDto,
        DocumentPageDto,
        CreateDocumentRequest,
        UpdateDocumentRequest,
        MoveDocumentRequest,
        DeleteDocumentRequest,
        DocumentHistoryPageDto,
        DocumentHistorySnapshotDto,
        RestoreDocumentHistoryRequest,
        SearchWorkspaceQuery,
        SearchPageDto,
        SettingsSnapshotDto,
        PatchSettingsRequest,
        AppConfigSnapshotDto,
        PatchAppConfigStateRequest,
        SyncConfigViewDto,
        PatchSyncConfigRequest,
        TestSyncConnectionRequest,
        SyncConnectionTestDto,
        ListRemoteNotebooksQuery,
        RemoteNotebookCatalogEntryDto,
        RemoteNotebookCatalogDto,
        BindSyncRepositoryRequest,
        SyncRepositoryBindingDto,
        SyncRepositoryBindingViewDto,
        DejavuKeyStateDto,
        ImportDejavuKeyRequest,
        ExportDejavuKeyRequest,
        ExportedDejavuKeyDto,
        SyncSafeErrorDto,
        SyncStatusDto,
        TriggerSyncRunRequest,
        SyncRunAcceptedDto,
        SyncRunStatusDto,
        AuthenticateFrameSchema,
        ReadyFrame,
        EventFrame,
        GapFrame,
        ErrorFrame,
        ServerFrame,
        ResourceRefDto,
        WorkspaceChangedEvent,
        DocumentCreatedEvent,
        DocumentChangedEvent,
        DocumentMovedEvent,
        DocumentDeletedEvent,
        SettingsChangedEvent,
        AppConfigStateChangedEvent,
        SyncConfigChangedEvent,
        SyncStatusChangedEvent,
        DomainEvent,
        SnapshotRequired,
    ))
)]
struct SchemaApiDoc;

#[allow(dead_code)]
#[derive(ToSchema)]
#[serde(rename_all = "camelCase")]
struct AuthenticateFrameSchema {
    #[serde(rename = "type")]
    frame_type: AuthenticateFrameKind,
    protocol_version: ProtocolVersion,
    #[schema(write_only)]
    credential: String,
}

#[allow(dead_code)]
#[derive(ToSchema)]
#[serde(rename_all = "kebab-case")]
enum AuthenticateFrameKind {
    Authenticate,
}

#[allow(dead_code)]
#[derive(ToSchema)]
#[serde(rename_all = "camelCase")]
struct ReadyFrame {
    #[serde(rename = "type")]
    frame_type: ReadyFrameKind,
    protocol_version: ProtocolVersion,
    connection_id: ConnectionId,
    instance_id: InstanceId,
    sequence: ReadySequence,
    snapshot_required: SnapshotRequired,
}

#[allow(dead_code)]
#[derive(ToSchema)]
#[serde(rename_all = "kebab-case")]
enum ReadyFrameKind {
    Ready,
}

#[allow(dead_code)]
#[derive(ToSchema)]
#[serde(rename_all = "camelCase")]
struct EventFrame {
    #[serde(rename = "type")]
    frame_type: EventFrameKind,
    protocol_version: ProtocolVersion,
    connection_id: ConnectionId,
    sequence: EventSequence,
    resource: ResourceRefDto,
    revision: Revision,
    event: DomainEvent,
}

#[allow(dead_code)]
#[derive(ToSchema)]
#[serde(rename_all = "kebab-case")]
enum EventFrameKind {
    Event,
}

#[allow(dead_code)]
#[derive(ToSchema)]
#[serde(rename_all = "camelCase")]
struct GapFrame {
    #[serde(rename = "type")]
    frame_type: GapFrameKind,
    protocol_version: ProtocolVersion,
    connection_id: ConnectionId,
    sequence: EventSequence,
    reason: GapReason,
    reload_scopes: Vec<ReloadScope>,
}

#[allow(dead_code)]
#[derive(ToSchema)]
#[serde(rename_all = "kebab-case")]
enum GapFrameKind {
    Gap,
}

#[allow(dead_code)]
#[derive(ToSchema)]
#[serde(rename_all = "camelCase")]
struct ErrorFrame {
    #[serde(rename = "type")]
    frame_type: ErrorFrameKind,
    protocol_version: ProtocolVersion,
    code: crate::contract::FrameErrorCode,
    message: String,
}

#[allow(dead_code)]
#[derive(ToSchema)]
#[serde(rename_all = "kebab-case")]
enum ErrorFrameKind {
    Error,
}

#[allow(dead_code)]
#[derive(ToSchema)]
#[serde(rename_all = "camelCase")]
struct WorkspaceChangedEvent {
    #[serde(rename = "type")]
    event_type: WorkspaceChangedKind,
    workspace: WorkspaceDto,
}

#[allow(dead_code)]
#[derive(ToSchema)]
#[serde(rename_all = "kebab-case")]
enum WorkspaceChangedKind {
    WorkspaceChanged,
}

#[allow(dead_code)]
#[derive(ToSchema)]
#[serde(rename_all = "camelCase")]
struct DocumentCreatedEvent {
    #[serde(rename = "type")]
    event_type: DocumentCreatedKind,
    document: DocumentEntryDto,
}

#[allow(dead_code)]
#[derive(ToSchema)]
#[serde(rename_all = "kebab-case")]
enum DocumentCreatedKind {
    DocumentCreated,
}

#[allow(dead_code)]
#[derive(ToSchema)]
#[serde(rename_all = "camelCase")]
struct DocumentChangedEvent {
    #[serde(rename = "type")]
    event_type: DocumentChangedKind,
    document: DocumentEntryDto,
}

#[allow(dead_code)]
#[derive(ToSchema)]
#[serde(rename_all = "kebab-case")]
enum DocumentChangedKind {
    DocumentChanged,
}

#[allow(dead_code)]
#[derive(ToSchema)]
#[serde(rename_all = "camelCase")]
struct DocumentMovedEvent {
    #[serde(rename = "type")]
    event_type: DocumentMovedKind,
    document: DocumentEntryDto,
    previous_path: crate::contract::WorkspaceRelativePath,
}

#[allow(dead_code)]
#[derive(ToSchema)]
#[serde(rename_all = "kebab-case")]
enum DocumentMovedKind {
    DocumentMoved,
}

#[allow(dead_code)]
#[derive(ToSchema)]
#[serde(rename_all = "camelCase")]
struct DocumentDeletedEvent {
    #[serde(rename = "type")]
    event_type: DocumentDeletedKind,
    document_id: DocumentId,
    previous_path: crate::contract::WorkspaceRelativePath,
    workspace_generation: crate::contract::WorkspaceGeneration,
    revision: Revision,
}

#[allow(dead_code)]
#[derive(ToSchema)]
#[serde(rename_all = "kebab-case")]
enum DocumentDeletedKind {
    DocumentDeleted,
}

#[allow(dead_code)]
#[derive(ToSchema)]
#[serde(rename_all = "camelCase")]
struct SettingsChangedEvent {
    #[serde(rename = "type")]
    event_type: SettingsChangedKind,
    settings: SettingsSnapshotDto,
}

#[allow(dead_code)]
#[derive(ToSchema)]
#[serde(rename_all = "kebab-case")]
enum SettingsChangedKind {
    SettingsChanged,
}

#[allow(dead_code)]
#[derive(ToSchema)]
#[serde(rename_all = "camelCase")]
struct AppConfigStateChangedEvent {
    #[serde(rename = "type")]
    event_type: AppConfigStateChangedKind,
    workspace_id: crate::contract::WorkspaceId,
    workspace_generation: crate::contract::WorkspaceGeneration,
    revision: Revision,
}

#[allow(dead_code)]
#[derive(ToSchema)]
#[serde(rename_all = "kebab-case")]
enum AppConfigStateChangedKind {
    AppConfigStateChanged,
}

#[allow(dead_code)]
#[derive(ToSchema)]
#[serde(rename_all = "camelCase")]
struct SyncConfigChangedEvent {
    #[serde(rename = "type")]
    event_type: SyncConfigChangedKind,
    config: SyncConfigViewDto,
}

#[allow(dead_code)]
#[derive(ToSchema)]
#[serde(rename_all = "kebab-case")]
enum SyncConfigChangedKind {
    SyncConfigChanged,
}

#[allow(dead_code)]
#[derive(ToSchema)]
#[serde(rename_all = "camelCase")]
struct SyncStatusChangedEvent {
    #[serde(rename = "type")]
    event_type: SyncStatusChangedKind,
    status: SyncStatusDto,
}

#[allow(dead_code)]
#[derive(ToSchema)]
#[serde(rename_all = "kebab-case")]
enum SyncStatusChangedKind {
    SyncStatusChanged,
}

fn install_paths(document: &mut serde_json::Value) {
    let paths = document
        .get_mut("paths")
        .and_then(serde_json::Value::as_object_mut)
        .expect("OpenAPI paths is an object");
    let operations = [
        (
            "get",
            "/api/v1/auth/status",
            "getAuthenticationStatus",
            "200",
            "ServerAuthenticationStatusDto",
            false,
        ),
        (
            "post",
            "/api/v1/auth/initialize",
            "initializeServerOwner",
            "201",
            "ServerSessionDto",
            false,
        ),
        (
            "post",
            "/api/v1/auth/session",
            "createServerSession",
            "201",
            "ServerSessionDto",
            false,
        ),
        (
            "get",
            "/api/v1/auth/session",
            "getServerSession",
            "200",
            "ServerSessionDto",
            false,
        ),
        (
            "post",
            "/api/v1/auth/logout",
            "logoutServerSession",
            "204",
            "",
            false,
        ),
        (
            "patch",
            "/api/v1/auth/password",
            "changeServerOwnerPassword",
            "204",
            "",
            false,
        ),
        (
            "get",
            "/api/v1/health/live",
            "healthLive",
            "200",
            "LiveHealthResponse",
            false,
        ),
        (
            "get",
            "/api/v1/health/ready",
            "healthReady",
            "200",
            "ReadyHealthResponse",
            true,
        ),
        (
            "get",
            "/api/v1/system/version",
            "getSystemVersion",
            "200",
            "SystemVersionResponse",
            true,
        ),
        (
            "get",
            "/api/v1/runtime",
            "getRuntimeState",
            "200",
            "RuntimeStateDto",
            true,
        ),
        (
            "get",
            "/api/v1/workspace",
            "getWorkspace",
            "200",
            "WorkspaceDto",
            true,
        ),
        (
            "get",
            "/api/v1/inventory",
            "listWorkspaceInventory",
            "200",
            "WorkspaceInventoryPageDto",
            true,
        ),
        (
            "get",
            "/api/v1/resources/{resourceId}",
            "openWorkspaceResource",
            "200",
            "",
            true,
        ),
        (
            "post",
            "/api/v1/documents/{documentId}/resources",
            "createWorkspaceResource",
            "201",
            "ResourceEntryDto",
            true,
        ),
        (
            "post",
            "/api/v1/documents/{documentId}/resource-batches",
            "createWorkspaceResourceBatch",
            "201",
            "CreateWorkspaceResourceBatchResponse",
            true,
        ),
        (
            "get",
            "/api/v1/documents",
            "listDocuments",
            "200",
            "DocumentPageDto",
            true,
        ),
        (
            "post",
            "/api/v1/documents",
            "createDocument",
            "201",
            "CreatedDocumentDto",
            true,
        ),
        (
            "get",
            "/api/v1/documents/{documentId}",
            "getDocument",
            "200",
            "DocumentContentDto",
            true,
        ),
        (
            "put",
            "/api/v1/documents/{documentId}",
            "updateDocument",
            "200",
            "DocumentContentDto",
            true,
        ),
        (
            "post",
            "/api/v1/documents/{documentId}/move",
            "moveDocument",
            "200",
            "DocumentEntryDto",
            true,
        ),
        (
            "post",
            "/api/v1/documents/{documentId}/delete",
            "deleteDocument",
            "204",
            "",
            true,
        ),
        (
            "get",
            "/api/v1/documents/{documentId}/history",
            "listDocumentHistory",
            "200",
            "DocumentHistoryPageDto",
            true,
        ),
        (
            "get",
            "/api/v1/documents/{documentId}/history/{snapshotId}",
            "getDocumentHistory",
            "200",
            "DocumentHistorySnapshotDto",
            true,
        ),
        (
            "post",
            "/api/v1/documents/{documentId}/history/{snapshotId}/restore",
            "restoreDocumentHistory",
            "200",
            "DocumentContentDto",
            true,
        ),
        (
            "get",
            "/api/v1/search",
            "searchWorkspace",
            "200",
            "SearchPageDto",
            true,
        ),
        (
            "get",
            "/api/v1/settings",
            "getSettings",
            "200",
            "SettingsSnapshotDto",
            true,
        ),
        (
            "patch",
            "/api/v1/settings",
            "patchSettings",
            "200",
            "SettingsSnapshotDto",
            true,
        ),
        (
            "get",
            "/api/v1/app-config",
            "getAppConfig",
            "200",
            "AppConfigSnapshotDto",
            true,
        ),
        (
            "patch",
            "/api/v1/app-config/state",
            "patchAppConfigState",
            "200",
            "AppConfigSnapshotDto",
            true,
        ),
        (
            "get",
            "/api/v1/sync/config",
            "getSyncConfig",
            "200",
            "SyncConfigViewDto",
            true,
        ),
        (
            "patch",
            "/api/v1/sync/config",
            "patchSyncConfig",
            "200",
            "SyncConfigViewDto",
            true,
        ),
        (
            "post",
            "/api/v1/sync/connection-test",
            "testSyncConnection",
            "200",
            "SyncConnectionTestDto",
            true,
        ),
        (
            "get",
            "/api/v1/sync/status",
            "getSyncStatus",
            "200",
            "SyncStatusDto",
            true,
        ),
        (
            "post",
            "/api/v1/sync/runs",
            "triggerSyncRun",
            "202",
            "SyncRunAcceptedDto",
            true,
        ),
        (
            "get",
            "/api/v1/sync/runs/{runId}",
            "getSyncRun",
            "200",
            "SyncRunStatusDto",
            true,
        ),
        (
            "get",
            "/api/v1/sync/repositories",
            "listRemoteNotebooks",
            "200",
            "RemoteNotebookCatalogDto",
            true,
        ),
        (
            "get",
            "/api/v1/sync/repository-binding",
            "getSyncRepositoryBinding",
            "200",
            "SyncRepositoryBindingViewDto",
            true,
        ),
        (
            "post",
            "/api/v1/sync/repository-binding",
            "bindSyncRepository",
            "202",
            "SyncRepositoryBindingDto",
            true,
        ),
        (
            "get",
            "/api/v1/sync/dejavu/key",
            "getDejavuKeyState",
            "200",
            "DejavuKeyStateDto",
            true,
        ),
        (
            "post",
            "/api/v1/sync/dejavu/key/import",
            "importDejavuKey",
            "200",
            "DejavuKeyStateDto",
            true,
        ),
        (
            "post",
            "/api/v1/sync/dejavu/key/export",
            "exportDejavuKey",
            "200",
            "ExportedDejavuKeyDto",
            true,
        ),
    ];
    for (method, path, operation_id, status, schema, protected) in operations {
        let mut success = if schema.is_empty() {
            serde_json::json!({ "description": "Success" })
        } else {
            serde_json::json!({
                "description": "Success",
                "content": {
                    "application/json": {
                        "schema": { "$ref": format!("#/components/schemas/{schema}") }
                    }
                }
            })
        };
        success["headers"] = serde_json::json!({
            "X-Request-Id": request_id_response_header()
        });
        let mut operation = serde_json::json!({
            "operationId": operation_id,
            "responses": { (status): success }
        });
        if protected {
            operation["security"] = protected_operation_security(method);
        }
        paths
            .entry(path.to_owned())
            .or_insert_with(|| serde_json::json!({}))[method] = operation;
    }
    paths["/api/v1/resources/{resourceId}"]["get"]["responses"]["200"]["content"] = serde_json::json!({
        "application/octet-stream": { "schema": { "type": "string", "format": "binary" } },
        "image/gif": { "schema": { "type": "string", "format": "binary" } },
        "image/jpeg": { "schema": { "type": "string", "format": "binary" } },
        "image/png": { "schema": { "type": "string", "format": "binary" } },
        "image/webp": { "schema": { "type": "string", "format": "binary" } }
    });
    paths["/api/v1/resources/{resourceId}"]["get"]["responses"]["200"]["headers"]
        ["Content-Length"] = serde_json::json!({
        "description": "Exact resource size in bytes.",
        "required": true,
        "schema": { "type": "integer", "format": "int64", "minimum": 0 }
    });
    paths["/api/v1/resources/{resourceId}"]["get"]["responses"]["200"]["headers"]
        ["X-Content-Type-Options"] = serde_json::json!({
        "description": "Disables content sniffing for untrusted workspace resources.",
        "required": true,
        "schema": { "type": "string", "const": "nosniff" }
    });
    paths["/api/v1/resources/{resourceId}"]["get"]["responses"]["200"]["headers"]
        ["X-Resource-Revision"] = serde_json::json!({
        "description": "Revision of the exact resource bytes opened for this response.",
        "required": true,
        "schema": { "$ref": "#/components/schemas/Revision" }
    });
    operation_mut(document, "/api/v1/auth/session", "get")["security"] = browser_session_security();
    for (path, method) in [
        ("/api/v1/auth/logout", "post"),
        ("/api/v1/auth/password", "patch"),
    ] {
        operation_mut(document, path, method)["security"] = browser_mutation_security();
    }
}

fn protected_operation_security(method: &str) -> serde_json::Value {
    if method == "get" {
        serde_json::json!([
            { "nativeBearer": [] },
            { "browserSessionHttps": [] },
            { "browserSessionHttp": [] }
        ])
    } else {
        serde_json::json!([
            { "nativeBearer": [] },
            { "browserSessionHttps": [], "csrfTokenHttps": [] },
            { "browserSessionHttp": [], "csrfTokenHttp": [] }
        ])
    }
}

fn browser_session_security() -> serde_json::Value {
    serde_json::json!([
        { "browserSessionHttps": [] },
        { "browserSessionHttp": [] }
    ])
}

fn browser_mutation_security() -> serde_json::Value {
    serde_json::json!([
        { "browserSessionHttps": [], "csrfTokenHttps": [] },
        { "browserSessionHttp": [], "csrfTokenHttp": [] }
    ])
}

fn install_security_scheme(document: &mut serde_json::Value) {
    document["components"]["securitySchemes"]["nativeBearer"] = serde_json::json!({
        "type": "http",
        "scheme": "bearer"
    });
    document["components"]["securitySchemes"]["browserSessionHttps"] = serde_json::json!({
        "type": "apiKey",
        "in": "cookie",
        "name": "__Host-qingyu_session"
    });
    document["components"]["securitySchemes"]["browserSessionHttp"] = serde_json::json!({
        "type": "apiKey",
        "in": "cookie",
        "name": "qingyu_session"
    });
    document["components"]["securitySchemes"]["csrfTokenHttps"] = serde_json::json!({
        "type": "apiKey",
        "in": "header",
        "name": "X-CSRF-Token",
        "x-csrf-cookie-name": "__Host-qingyu_csrf"
    });
    document["components"]["securitySchemes"]["csrfTokenHttp"] = serde_json::json!({
        "type": "apiKey",
        "in": "header",
        "name": "X-CSRF-Token",
        "x-csrf-cookie-name": "qingyu_csrf"
    });
}

fn install_operation_inputs(document: &mut serde_json::Value) {
    for (path, method, schema) in [
        (
            "/api/v1/auth/initialize",
            "post",
            "InitializeServerOwnerRequest",
        ),
        ("/api/v1/auth/session", "post", "CreateServerSessionRequest"),
        (
            "/api/v1/auth/password",
            "patch",
            "ChangeServerOwnerPasswordRequest",
        ),
    ] {
        set_request_body(document, path, method, schema, 16 * 1024);
    }
    set_query_parameters(
        document,
        "/api/v1/inventory",
        "get",
        &[
            ("cursor", "PageCursor", false),
            ("limit", "PageLimit", false),
            ("parent", "WorkspaceRelativePath", false),
        ],
    );
    push_parameter(
        document,
        "/api/v1/sync/runs/{runId}",
        "get",
        "runId",
        "path",
        "RunId",
        true,
    );
    push_parameter(
        document,
        "/api/v1/resources/{resourceId}",
        "get",
        "resourceId",
        "path",
        "ResourceId",
        true,
    );
    push_parameter(
        document,
        "/api/v1/documents/{documentId}/resources",
        "post",
        "documentId",
        "path",
        "DocumentId",
        true,
    );
    set_query_parameters(
        document,
        "/api/v1/documents/{documentId}/resources",
        "post",
        &[
            ("workspaceGeneration", "WorkspaceGeneration", true),
            ("folder", "WorkspaceRelativePath", true),
            ("name", "ResourceName", true),
            ("kind", "ResourceKind", true),
        ],
    );
    set_binary_request_body(
        document,
        "/api/v1/documents/{documentId}/resources",
        "post",
        64 * 1024 * 1024,
    );
    push_parameter(
        document,
        "/api/v1/documents/{documentId}/resource-batches",
        "post",
        "documentId",
        "path",
        "DocumentId",
        true,
    );
    set_request_body(
        document,
        "/api/v1/documents/{documentId}/resource-batches",
        "post",
        "CreateWorkspaceResourceBatchRequest",
        100 * 1024 * 1024,
    );
    push_parameter(
        document,
        "/api/v1/resources/{resourceId}",
        "get",
        "kind",
        "query",
        "ResourceKind",
        true,
    );
    set_query_parameters(
        document,
        "/api/v1/documents",
        "get",
        &[
            ("cursor", "PageCursor", false),
            ("limit", "PageLimit", false),
            ("parent", "WorkspaceRelativePath", false),
        ],
    );
    set_request_body(
        document,
        "/api/v1/documents",
        "post",
        "CreateDocumentRequest",
        100 * 1024 * 1024,
    );
    for (path, method) in [
        ("/api/v1/documents/{documentId}", "get"),
        ("/api/v1/documents/{documentId}", "put"),
        ("/api/v1/documents/{documentId}/move", "post"),
        ("/api/v1/documents/{documentId}/delete", "post"),
        ("/api/v1/documents/{documentId}/history", "get"),
        ("/api/v1/documents/{documentId}/history/{snapshotId}", "get"),
        (
            "/api/v1/documents/{documentId}/history/{snapshotId}/restore",
            "post",
        ),
    ] {
        push_parameter(
            document,
            path,
            method,
            "documentId",
            "path",
            "DocumentId",
            true,
        );
    }
    for (path, schema, limit) in [
        (
            "/api/v1/documents/{documentId}",
            "UpdateDocumentRequest",
            100 * 1024 * 1024,
        ),
        (
            "/api/v1/documents/{documentId}/move",
            "MoveDocumentRequest",
            1024 * 1024,
        ),
        (
            "/api/v1/documents/{documentId}/delete",
            "DeleteDocumentRequest",
            1024 * 1024,
        ),
        (
            "/api/v1/documents/{documentId}/history/{snapshotId}/restore",
            "RestoreDocumentHistoryRequest",
            1024 * 1024,
        ),
    ] {
        let method = if path == "/api/v1/documents/{documentId}" {
            "put"
        } else {
            "post"
        };
        set_request_body(document, path, method, schema, limit);
    }
    set_query_parameters(
        document,
        "/api/v1/documents/{documentId}/history",
        "get",
        &[
            ("cursor", "PageCursor", false),
            ("limit", "PageLimit", false),
        ],
    );
    push_parameter(
        document,
        "/api/v1/documents/{documentId}/history/{snapshotId}",
        "get",
        "snapshotId",
        "path",
        "SnapshotId",
        true,
    );
    push_parameter(
        document,
        "/api/v1/documents/{documentId}/history/{snapshotId}/restore",
        "post",
        "snapshotId",
        "path",
        "SnapshotId",
        true,
    );
    set_query_parameters(
        document,
        "/api/v1/search",
        "get",
        &[
            ("query", "SearchQuery", true),
            ("cursor", "PageCursor", false),
            ("limit", "PageLimit", false),
        ],
    );
    for (path, method, schema) in [
        ("/api/v1/settings", "patch", "PatchSettingsRequest"),
        ("/api/v1/sync/config", "patch", "PatchSyncConfigRequest"),
        (
            "/api/v1/sync/connection-test",
            "post",
            "TestSyncConnectionRequest",
        ),
        ("/api/v1/sync/runs", "post", "TriggerSyncRunRequest"),
        (
            "/api/v1/sync/repository-binding",
            "post",
            "BindSyncRepositoryRequest",
        ),
        (
            "/api/v1/sync/dejavu/key/import",
            "post",
            "ImportDejavuKeyRequest",
        ),
        (
            "/api/v1/sync/dejavu/key/export",
            "post",
            "ExportDejavuKeyRequest",
        ),
    ] {
        set_request_body(document, path, method, schema, 1024 * 1024);
    }
    push_parameter(
        document,
        "/api/v1/sync/repositories",
        "get",
        "expectedRevision",
        "query",
        "Revision",
        true,
    );
    set_request_body(
        document,
        "/api/v1/app-config/state",
        "patch",
        "PatchAppConfigStateRequest",
        64 * 1024 * 1024,
    );
}

fn operation_mut<'a>(
    document: &'a mut serde_json::Value,
    path: &str,
    method: &str,
) -> &'a mut serde_json::Value {
    document
        .get_mut("paths")
        .and_then(serde_json::Value::as_object_mut)
        .and_then(|paths| paths.get_mut(path))
        .and_then(serde_json::Value::as_object_mut)
        .and_then(|path| path.get_mut(method))
        .expect("the operation is registered")
}

fn set_request_body(
    document: &mut serde_json::Value,
    path: &str,
    method: &str,
    schema: &str,
    body_limit: usize,
) {
    let operation = operation_mut(document, path, method);
    operation["requestBody"] = serde_json::json!({
        "required": true,
        "content": {
            "application/json": {
                "schema": { "$ref": format!("#/components/schemas/{schema}") }
            }
        }
    });
    operation["x-body-limit-bytes"] = serde_json::json!(body_limit);
}

fn set_binary_request_body(
    document: &mut serde_json::Value,
    path: &str,
    method: &str,
    body_limit: usize,
) {
    let schema = serde_json::json!({
        "type": "string",
        "format": "binary",
        "maxLength": body_limit,
    });
    operation_mut(document, path, method)["requestBody"] = serde_json::json!({
        "required": true,
        "content": {
            "application/octet-stream": { "schema": schema.clone() },
            "image/gif": { "schema": schema.clone() },
            "image/jpeg": { "schema": schema.clone() },
            "image/png": { "schema": schema.clone() },
            "image/webp": { "schema": schema },
        }
    });
    operation_mut(document, path, method)["x-body-limit-bytes"] = serde_json::json!(body_limit);
}

fn set_query_parameters(
    document: &mut serde_json::Value,
    path: &str,
    method: &str,
    parameters: &[(&str, &str, bool)],
) {
    for (name, schema, required) in parameters {
        push_parameter(document, path, method, name, "query", schema, *required);
    }
}

fn push_parameter(
    document: &mut serde_json::Value,
    path: &str,
    method: &str,
    name: &str,
    location: &str,
    schema: &str,
    required: bool,
) {
    let parameters = operation_mut(document, path, method)["parameters"]
        .as_array_mut()
        .map(|parameters| parameters as &mut Vec<serde_json::Value>);
    let parameter = serde_json::json!({
        "name": name,
        "in": location,
        "required": required,
        "schema": { "$ref": format!("#/components/schemas/{schema}") }
    });
    if let Some(parameters) = parameters {
        parameters.push(parameter);
    } else {
        operation_mut(document, path, method)["parameters"] = serde_json::json!([parameter]);
    }
}

fn install_operation_errors(document: &mut serde_json::Value) {
    const PUBLIC_TRANSPORT: &[&str] = &["host_not_allowed", "origin_not_allowed", "internal_error"];
    const TRANSPORT: &[&str] = &[
        "host_not_allowed",
        "origin_not_allowed",
        "unauthorized",
        "authentication_unavailable",
        "internal_error",
    ];
    const WORKSPACE: &[&str] = &[
        "kernel_not_ready",
        "workspace_unavailable",
        "workspace_locked",
    ];

    add_errors_with(
        document,
        "/api/v1/auth/status",
        "get",
        PUBLIC_TRANSPORT,
        &["authentication_unavailable"],
    );
    add_errors_with(
        document,
        "/api/v1/auth/initialize",
        "post",
        PUBLIC_TRANSPORT,
        &[
            "invalid_request",
            "already_initialized",
            "invalid_credentials",
            "authentication_rate_limited",
            "authentication_unavailable",
        ],
    );
    add_errors_with(
        document,
        "/api/v1/auth/session",
        "post",
        PUBLIC_TRANSPORT,
        &[
            "invalid_request",
            "initialization_required",
            "invalid_credentials",
            "authentication_rate_limited",
            "authentication_unavailable",
        ],
    );
    add_errors_with(
        document,
        "/api/v1/auth/session",
        "get",
        PUBLIC_TRANSPORT,
        &["unauthorized", "authentication_unavailable"],
    );
    add_errors_with(
        document,
        "/api/v1/auth/logout",
        "post",
        PUBLIC_TRANSPORT,
        &[
            "unauthorized",
            "csrf_rejected",
            "authentication_unavailable",
        ],
    );
    add_errors_with(
        document,
        "/api/v1/auth/password",
        "patch",
        PUBLIC_TRANSPORT,
        &[
            "invalid_request",
            "unauthorized",
            "invalid_credentials",
            "csrf_rejected",
            "authentication_rate_limited",
            "authentication_unavailable",
        ],
    );

    add_operation_errors(
        document,
        "/api/v1/health/live",
        "get",
        &["host_not_allowed", "origin_not_allowed", "internal_error"],
    );
    add_errors_with(
        document,
        "/api/v1/health/ready",
        "get",
        TRANSPORT,
        &["kernel_not_ready"],
    );
    for (path, method) in [
        ("/api/v1/system/version", "get"),
        ("/api/v1/runtime", "get"),
    ] {
        add_errors_with(document, path, method, TRANSPORT, &["kernel_not_ready"]);
    }
    add_errors_with(document, "/api/v1/workspace", "get", TRANSPORT, WORKSPACE);
    add_errors_with(
        document,
        "/api/v1/inventory",
        "get",
        TRANSPORT,
        &[
            "kernel_not_ready",
            "workspace_unavailable",
            "workspace_locked",
            "invalid_request",
            "invalid_workspace_path",
        ],
    );
    add_errors_with(
        document,
        "/api/v1/resources/{resourceId}",
        "get",
        TRANSPORT,
        &[
            "kernel_not_ready",
            "workspace_unavailable",
            "workspace_locked",
            "invalid_request",
            "resource_not_found",
        ],
    );
    add_errors_with(
        document,
        "/api/v1/documents/{documentId}/resources",
        "post",
        TRANSPORT,
        &[
            "kernel_not_ready",
            "workspace_unavailable",
            "workspace_locked",
            "invalid_request",
            "document_not_found",
            "resource_too_large",
            "revision_conflict",
        ],
    );

    let document_routes = [
        (
            "/api/v1/documents",
            "get",
            &["invalid_request", "invalid_workspace_path"][..],
        ),
        (
            "/api/v1/documents",
            "post",
            &[
                "invalid_request",
                "invalid_workspace_path",
                "invalid_document_name",
                "document_already_exists",
                "document_too_large",
                "document_invalid_encoding",
                "revision_conflict",
            ][..],
        ),
        (
            "/api/v1/documents/{documentId}",
            "get",
            &[
                "invalid_request",
                "document_not_found",
                "document_invalid_encoding",
            ][..],
        ),
        (
            "/api/v1/documents/{documentId}",
            "put",
            &[
                "invalid_request",
                "document_not_found",
                "document_too_large",
                "document_invalid_encoding",
                "revision_conflict",
            ][..],
        ),
        (
            "/api/v1/documents/{documentId}/move",
            "post",
            &[
                "invalid_request",
                "invalid_workspace_path",
                "invalid_document_name",
                "document_not_found",
                "document_already_exists",
                "revision_conflict",
            ][..],
        ),
        (
            "/api/v1/documents/{documentId}/delete",
            "post",
            &["invalid_request", "document_not_found", "revision_conflict"][..],
        ),
        (
            "/api/v1/documents/{documentId}/history",
            "get",
            &["invalid_request", "document_not_found"][..],
        ),
        (
            "/api/v1/documents/{documentId}/history/{snapshotId}",
            "get",
            &[
                "invalid_request",
                "document_not_found",
                "document_too_large",
                "document_invalid_encoding",
            ][..],
        ),
        (
            "/api/v1/documents/{documentId}/history/{snapshotId}/restore",
            "post",
            &[
                "invalid_request",
                "document_not_found",
                "document_too_large",
                "document_invalid_encoding",
                "revision_conflict",
            ][..],
        ),
        (
            "/api/v1/search",
            "get",
            &["invalid_request", "document_invalid_encoding"][..],
        ),
    ];
    for (path, method, specific) in document_routes {
        let mut codes = TRANSPORT.to_vec();
        codes.extend_from_slice(WORKSPACE);
        codes.extend_from_slice(specific);
        add_operation_errors(document, path, method, &codes);
    }

    add_errors_with(
        document,
        "/api/v1/settings",
        "get",
        TRANSPORT,
        &["settings_unavailable"],
    );
    add_errors_with(
        document,
        "/api/v1/settings",
        "patch",
        TRANSPORT,
        &[
            "invalid_request",
            "settings_unavailable",
            "settings_revision_conflict",
            "invalid_settings_field",
        ],
    );
    add_errors_with(
        document,
        "/api/v1/app-config",
        "get",
        TRANSPORT,
        &["app_config_unavailable"],
    );
    add_errors_with(
        document,
        "/api/v1/app-config/state",
        "patch",
        TRANSPORT,
        &[
            "invalid_request",
            "resource_too_large",
            "invalid_app_config_state",
            "workspace_generation_stale",
            "app_config_unavailable",
        ],
    );
    add_errors_with(
        document,
        "/api/v1/sync/config",
        "get",
        TRANSPORT,
        &["sync_config_absent", "sync_config_invalid"],
    );
    for (path, method) in [
        ("/api/v1/sync/config", "patch"),
        ("/api/v1/sync/connection-test", "post"),
    ] {
        add_errors_with(
            document,
            path,
            method,
            TRANSPORT,
            &[
                "invalid_request",
                "sync_config_absent",
                "sync_config_invalid",
                "sync_config_revision_conflict",
                "sync_not_ready",
            ],
        );
    }
    add_errors_with(
        document,
        "/api/v1/sync/status",
        "get",
        TRANSPORT,
        &["invalid_request", "sync_not_ready"],
    );
    add_errors_with(
        document,
        "/api/v1/sync/runs/{runId}",
        "get",
        TRANSPORT,
        &["invalid_request", "sync_not_ready", "resource_not_found"],
    );
    add_errors_with(
        document,
        "/api/v1/sync/runs",
        "post",
        TRANSPORT,
        &[
            "invalid_request",
            "sync_not_ready",
            "sync_run_unavailable",
            "sync_config_revision_conflict",
        ],
    );
    add_errors_with(
        document,
        "/api/v1/sync/repositories",
        "get",
        TRANSPORT,
        &[
            "sync_config_absent",
            "sync_config_invalid",
            "sync_config_revision_conflict",
            "sync_not_ready",
        ],
    );
    add_errors_with(
        document,
        "/api/v1/sync/repository-binding",
        "get",
        TRANSPORT,
        &["invalid_request", "sync_not_ready"],
    );
    add_errors_with(
        document,
        "/api/v1/sync/repository-binding",
        "post",
        TRANSPORT,
        &[
            "invalid_request",
            "sync_config_absent",
            "sync_config_invalid",
            "sync_config_revision_conflict",
            "sync_not_ready",
            "sync_run_unavailable",
        ],
    );
    for (path, method) in [
        ("/api/v1/sync/dejavu/key", "get"),
        ("/api/v1/sync/dejavu/key/import", "post"),
        ("/api/v1/sync/dejavu/key/export", "post"),
    ] {
        add_errors_with(
            document,
            path,
            method,
            TRANSPORT,
            &["invalid_request", "sync_not_ready", "sync_run_unavailable"],
        );
    }
}

fn add_errors_with(
    document: &mut serde_json::Value,
    path: &str,
    method: &str,
    common: &[&str],
    specific: &[&str],
) {
    let mut codes = common.to_vec();
    codes.extend_from_slice(specific);
    add_operation_errors(document, path, method, &codes);
}

fn add_operation_errors(
    document: &mut serde_json::Value,
    path: &str,
    method: &str,
    codes: &[&str],
) {
    let mut grouped = std::collections::BTreeMap::<u16, Vec<&str>>::new();
    for code in codes {
        grouped.entry(error_status(code)).or_default().push(code);
    }
    let csrf_required = operation_mut(document, path, method)["security"]
        .as_array()
        .is_some_and(|requirements| {
            requirements.iter().any(|requirement| {
                requirement.get("csrfTokenHttps").is_some()
                    || requirement.get("csrfTokenHttp").is_some()
            })
        });
    if csrf_required {
        grouped.entry(403).or_default().push("csrf_rejected");
    }
    let responses = operation_mut(document, path, method)["responses"]
        .as_object_mut()
        .expect("operation responses is an object");
    for (status, mut codes) in grouped {
        codes.sort_unstable();
        codes.dedup();
        let mut headers = serde_json::json!({
            "X-Request-Id": request_id_response_header()
        });
        if status == 429 {
            headers["Retry-After"] = serde_json::json!({
                "description": "Whole seconds until another authentication attempt is allowed.",
                "required": true,
                "schema": {
                    "$ref": "#/components/schemas/PositiveSafeInteger"
                }
            });
        }
        let mut error_refinement = serde_json::json!({
            "type": "object",
            "properties": {
                "code": { "type": "string", "enum": codes }
            }
        });
        if status == 429 {
            error_refinement["required"] = serde_json::json!(["code", "details"]);
            error_refinement["properties"]["details"] = serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["type", "retryAfterSeconds"],
                "properties": {
                    "type": { "type": "string", "enum": ["rate-limit"] },
                    "retryAfterSeconds": {
                        "$ref": "#/components/schemas/PositiveSafeInteger"
                    }
                }
            });
        }
        responses.insert(
            status.to_string(),
            serde_json::json!({
                "description": "Error",
                "headers": headers,
                "content": {
                    "application/json": {
                        "schema": {
                            "allOf": [
                                { "$ref": "#/components/schemas/ApiErrorEnvelope" },
                                error_refinement
                            ]
                        }
                    }
                }
            }),
        );
    }
}

fn request_id_response_header() -> serde_json::Value {
    serde_json::json!({
        "description": "Correlation ID for this response.",
        "required": true,
        "schema": {
            "type": "string",
            "format": "uuid"
        }
    })
}

fn error_status(code: &str) -> u16 {
    match code {
        "invalid_request" | "invalid_workspace_path" | "invalid_document_name" => 400,
        "unauthorized" | "invalid_credentials" => 401,
        "host_not_allowed" | "origin_not_allowed" | "csrf_rejected" => 403,
        "document_not_found" | "resource_not_found" | "sync_config_absent" => 404,
        "document_already_exists"
        | "initialization_required"
        | "already_initialized"
        | "revision_conflict"
        | "settings_revision_conflict"
        | "workspace_generation_stale"
        | "sync_config_revision_conflict" => 409,
        "document_too_large" | "resource_too_large" => 413,
        "document_invalid_encoding"
        | "invalid_settings_field"
        | "invalid_app_config_state"
        | "sync_config_invalid" => 422,
        "workspace_locked" => 423,
        "authentication_rate_limited" => 429,
        "kernel_not_ready"
        | "authentication_unavailable"
        | "workspace_unavailable"
        | "settings_unavailable"
        | "app_config_unavailable"
        | "sync_not_ready"
        | "sync_run_unavailable" => 503,
        "internal_error" => 500,
        _ => 500,
    }
}

fn patch_literal_and_nullable_schemas(document: &mut serde_json::Value) {
    document["components"]["schemas"]["SnapshotRequired"] = serde_json::json!({
        "type": "boolean",
        "const": true
    });
    let schemas = document["components"]["schemas"]
        .as_object_mut()
        .expect("OpenAPI schemas is an object");
    let authenticate = schemas
        .remove("AuthenticateFrameSchema")
        .expect("authenticate frame schema is registered");
    schemas.insert("AuthenticateFrame".to_owned(), authenticate);
    let positive_safe_integer = schemas
        .get_mut("PositiveSafeInteger")
        .and_then(serde_json::Value::as_object_mut)
        .expect("PositiveSafeInteger schema is an object");
    positive_safe_integer.insert("minimum".to_owned(), serde_json::json!(1));
    positive_safe_integer.insert(
        "maximum".to_owned(),
        serde_json::json!(crate::contract::MAX_SAFE_INTEGER),
    );
    schemas
        .get_mut("DocumentContents")
        .and_then(serde_json::Value::as_object_mut)
        .expect("DocumentContents schema is an object")
        .insert(
            "x-max-utf8-bytes".to_owned(),
            serde_json::json!(DocumentContents::maximum_bytes()),
        );

    for schema in schemas.values_mut() {
        rename_schema_properties_to_camel_case(schema);
        strip_null_from_optional_properties(schema);
    }
    for (schema_name, property_name) in [
        ("AppConfigSnapshotDto", "appConfigVersion"),
        ("StoredWorkspaceLayoutDto", "schemaVersion"),
    ] {
        schemas
            .get_mut(schema_name)
            .and_then(|schema| schema.pointer_mut(&format!("/properties/{property_name}")))
            .and_then(serde_json::Value::as_object_mut)
            .expect("AppConfig version property schema is an object")
            .insert("const".to_owned(), serde_json::json!(1));
    }
    schemas
        .get_mut("AppConfigStateChangedEvent")
        .and_then(serde_json::Value::as_object_mut)
        .expect("AppConfig event schema is an object")
        .insert("additionalProperties".to_owned(), serde_json::json!(false));
    let domain_app_config_variant = schemas
        .get_mut("DomainEvent")
        .and_then(|schema| schema.get_mut("oneOf"))
        .and_then(serde_json::Value::as_array_mut)
        .and_then(|variants| {
            variants.iter_mut().find(|variant| {
                variant["properties"]["type"]["enum"]
                    == serde_json::json!(["app-config-state-changed"])
            })
        })
        .expect("DomainEvent AppConfig variant is registered");
    domain_app_config_variant
        .as_object_mut()
        .expect("DomainEvent AppConfig variant is an object")
        .insert("additionalProperties".to_owned(), serde_json::json!(false));
    let operation_variants = schemas
        .get_mut("AppConfigStateOperationDto")
        .and_then(|schema| schema.get_mut("oneOf"))
        .and_then(serde_json::Value::as_array_mut)
        .expect("AppConfig operation schema variants are an array");
    for variant in operation_variants {
        variant
            .as_object_mut()
            .expect("AppConfig operation variant is an object")
            .insert("additionalProperties".to_owned(), serde_json::json!(false));
    }
    let nullable_names = schemas
        .keys()
        .filter(|name| name.starts_with("Nullable_"))
        .cloned()
        .collect::<Vec<_>>();
    for name in nullable_names {
        let schema = schemas
            .remove(&name)
            .expect("the nullable schema was collected from this map");
        schemas.insert(
            name,
            serde_json::json!({
                "oneOf": [
                    { "type": "null" },
                    schema
                ]
            }),
        );
    }
}

fn rename_schema_properties_to_camel_case(schema: &mut serde_json::Value) {
    match schema {
        serde_json::Value::Object(object) => {
            if let Some(properties) = object
                .get_mut("properties")
                .and_then(serde_json::Value::as_object_mut)
            {
                let original = std::mem::take(properties);
                *properties = original
                    .into_iter()
                    .map(|(name, value)| (snake_to_lower_camel(&name), value))
                    .collect();
            }
            if let Some(required) = object
                .get_mut("required")
                .and_then(serde_json::Value::as_array_mut)
            {
                for name in required {
                    if let Some(value) = name.as_str() {
                        *name = serde_json::Value::String(snake_to_lower_camel(value));
                    }
                }
            }
            for value in object.values_mut() {
                rename_schema_properties_to_camel_case(value);
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                rename_schema_properties_to_camel_case(value);
            }
        }
        _ => {}
    }
}

fn snake_to_lower_camel(value: &str) -> String {
    let mut parts = value.split('_');
    let Some(first) = parts.next() else {
        return String::new();
    };
    let mut output = first.to_owned();
    for part in parts {
        let mut characters = part.chars();
        if let Some(first) = characters.next() {
            output.extend(first.to_uppercase());
            output.extend(characters);
        }
    }
    output
}

fn strip_null_from_optional_properties(schema: &mut serde_json::Value) {
    let Some(object) = schema.as_object_mut() else {
        return;
    };
    let required = object
        .get("required")
        .and_then(serde_json::Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_owned)
                .collect::<std::collections::HashSet<_>>()
        })
        .unwrap_or_default();
    if let Some(properties) = object
        .get_mut("properties")
        .and_then(serde_json::Value::as_object_mut)
    {
        for (name, property) in properties {
            if !required.contains(name) {
                strip_null_schema(property);
            }
            strip_null_from_optional_properties(property);
        }
    }
    for key in ["allOf", "oneOf", "anyOf"] {
        if let Some(parts) = object
            .get_mut(key)
            .and_then(serde_json::Value::as_array_mut)
        {
            for part in parts {
                strip_null_from_optional_properties(part);
            }
        }
    }
}

fn strip_null_schema(schema: &mut serde_json::Value) {
    let Some(object) = schema.as_object_mut() else {
        return;
    };
    let single_type = object
        .get_mut("type")
        .and_then(serde_json::Value::as_array_mut)
        .and_then(|types| {
            types.retain(|value| value.as_str() != Some("null"));
            (types.len() == 1).then(|| types[0].clone())
        });
    if let Some(single_type) = single_type {
        object.insert("type".to_owned(), single_type);
    }
    if let Some(values) = object
        .get_mut("enum")
        .and_then(serde_json::Value::as_array_mut)
    {
        values.retain(|value| !value.is_null());
    }
    for key in ["oneOf", "anyOf"] {
        let replacement = object
            .get_mut(key)
            .and_then(serde_json::Value::as_array_mut)
            .and_then(|parts| {
                parts.retain(|part| {
                    part.get("type").and_then(serde_json::Value::as_str) != Some("null")
                });
                (parts.len() == 1).then(|| parts[0].clone())
            });
        if let Some(replacement) = replacement {
            *schema = replacement;
            return;
        }
    }
    object.remove("nullable");
}
