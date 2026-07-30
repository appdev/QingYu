use std::{
    fmt,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    body::Body,
    extract::{ConnectInfo, Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, patch, post},
    Json, Router,
};
use zeroize::Zeroizing;

use crate::{
    contract::{
        ChangeServerOwnerPasswordRequest, CreateServerSessionRequest, ErrorCode, ErrorDetails,
        InitializeServerOwnerRequest, PositiveSafeInteger, ServerAuthenticationSecret,
        ServerAuthenticationStatusDto, ServerInitializationState, ServerSessionDto,
        ServerSessionState,
    },
    host::server::{
        MissingDirectPeer, ServerBlockingError, ServerHostEpochSlot, ServerHostLease,
        ServerHostProcessResources, ServerHostProcessResourcesError,
    },
    runtime::KernelRuntime,
    server::{
        InitializationStatus, IssuedSession, RequestIntent, ServerAuthenticationCoordinator,
        ServerAuthenticationCoordinatorError, ServerAuthenticationSecurity,
        ServerInitializationCoordinator, ServerOwnerInitializationError, SessionAuthorization,
    },
};

use super::{api_error, routes::parse_sensitive_auth_json, ApiState};

pub(crate) const SESSION_COOKIE_NAME: &str = "__Host-qingyu_session";
pub(crate) const CSRF_COOKIE_NAME: &str = "__Host-qingyu_csrf";
pub(crate) const CSRF_HEADER_NAME: &str = "x-csrf-token";

const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

struct ServerApiAuthenticationState {
    initialization: Mutex<ServerInitializationCoordinator>,
    authentication: ServerAuthenticationCoordinator,
}

/// Owns process-wide admission and direct-peer identity resources across
/// replaceable server launch epochs.
pub struct ServerApiProcess {
    resources: Arc<ServerHostProcessResources>,
    slot: ServerHostEpochSlot<ServerApiAuthenticationState>,
    started_at: Instant,
}

impl ServerApiProcess {
    pub fn new(blocking_capacity: usize) -> Result<Self, ServerApiProcessError> {
        let resources = ServerHostProcessResources::new(blocking_capacity)
            .map_err(ServerApiProcessError::from)?;
        let slot = resources.epoch_slot();
        Ok(Self {
            resources,
            slot,
            started_at: Instant::now(),
        })
    }

    pub fn activate(
        &self,
        runtime: &KernelRuntime,
        security: &ServerAuthenticationSecurity,
        initialization: ServerInitializationCoordinator,
    ) -> Result<ServerApiHost, ServerApiActivationError> {
        let state = ServerApiAuthenticationState {
            initialization: Mutex::new(initialization),
            authentication: security.authentication_coordinator(),
        };
        Ok(ServerApiHost {
            resources: Arc::clone(&self.resources),
            lease: self.slot.replace(runtime.launch_epoch(), state),
            started_at: self.started_at,
        })
    }
}

impl fmt::Debug for ServerApiProcess {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ServerApiProcess(..)")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServerApiProcessError {
    InvalidBlockingCapacity,
    ClientIdentityUnavailable,
}

impl From<ServerHostProcessResourcesError> for ServerApiProcessError {
    fn from(error: ServerHostProcessResourcesError) -> Self {
        match error {
            ServerHostProcessResourcesError::ZeroBlockingCapacity => Self::InvalidBlockingCapacity,
            ServerHostProcessResourcesError::ClientIdentityKeyGeneration => {
                Self::ClientIdentityUnavailable
            }
        }
    }
}

impl fmt::Display for ServerApiProcessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidBlockingCapacity => "server blocking capacity must be positive",
            Self::ClientIdentityUnavailable => "server client identity is unavailable",
        })
    }
}

impl std::error::Error for ServerApiProcessError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServerApiActivationError {
    NonServerProfile,
}

impl fmt::Display for ServerApiActivationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NonServerProfile => "server API requires the server host profile",
        })
    }
}

impl std::error::Error for ServerApiActivationError {}

#[derive(Clone)]
pub struct ServerApiHost {
    resources: Arc<ServerHostProcessResources>,
    lease: ServerHostLease<ServerApiAuthenticationState>,
    started_at: Instant,
}

impl ServerApiHost {
    pub(crate) fn identify_request(
        &self,
        direct_peer: Option<SocketAddr>,
        headers: &HeaderMap,
    ) -> Result<u64, MissingDirectPeer> {
        self.resources.identify_request(direct_peer, headers)
    }

    pub(crate) fn now(&self) -> Duration {
        self.started_at.elapsed()
    }

    async fn status(&self) -> Result<InitializationStatus, ServerApiOperationError> {
        self.lease
            .run_blocking(|state| {
                state
                    .initialization
                    .lock()
                    .map(|initialization| initialization.status())
                    .map_err(|_poisoned| ServerApiOperationError::Unavailable)
            })
            .await
            .map_err(ServerApiOperationError::from)?
    }

    async fn initialize(
        &self,
        client_id: u64,
        token: ServerAuthenticationSecret,
        password: ServerAuthenticationSecret,
    ) -> Result<IssuedSession, ServerApiOperationError> {
        let started_at = self.started_at;
        self.lease
            .run_blocking(move |state| {
                let initialization_now = started_at.elapsed();
                let mut initialization = state
                    .initialization
                    .lock()
                    .map_err(|_poisoned| ServerApiOperationError::Unavailable)?;
                initialization
                    .initialize(
                        client_id,
                        initialization_now,
                        token.expose_secret(),
                        password.expose_secret().to_owned(),
                    )
                    .map_err(ServerApiOperationError::Initialization)?;
                let login_now = started_at.elapsed();
                state
                    .authentication
                    .login(client_id, login_now, password.expose_secret().to_owned())
                    .map(|login| login.into_session())
                    .map_err(ServerApiOperationError::Authentication)
            })
            .await
            .map_err(ServerApiOperationError::from)?
    }

    async fn login(
        &self,
        client_id: u64,
        password: ServerAuthenticationSecret,
    ) -> Result<IssuedSession, ServerApiOperationError> {
        let started_at = self.started_at;
        self.lease
            .run_blocking(move |state| {
                let initialization = state
                    .initialization
                    .lock()
                    .map_err(|_poisoned| ServerApiOperationError::Unavailable)?;
                match initialization.status() {
                    InitializationStatus::Pending | InitializationStatus::InProgress => {
                        return Err(ServerApiOperationError::InitializationRequired);
                    }
                    InitializationStatus::Initialized => {}
                    InitializationStatus::Unavailable => {
                        return Err(ServerApiOperationError::Unavailable);
                    }
                }
                drop(initialization);
                let now = started_at.elapsed();
                state
                    .authentication
                    .login(client_id, now, password.expose_secret().to_owned())
                    .map(|login| login.into_session())
                    .map_err(ServerApiOperationError::Authentication)
            })
            .await
            .map_err(ServerApiOperationError::from)?
    }

    pub(crate) async fn authorize_browser_session(
        &self,
        credential: BrowserSessionCredential,
        csrf: Option<BrowserCsrfProof>,
        intent: RequestIntent,
    ) -> Result<AuthenticatedBrowserSession, ServerApiOperationError> {
        let started_at = self.started_at;
        self.lease
            .run_blocking(move |state| {
                let now = started_at.elapsed();
                let authorization = state
                    .authentication
                    .authorize(
                        credential.expose_secret(),
                        csrf.as_ref().map(BrowserCsrfProof::expose_secret),
                        intent,
                        now,
                    )
                    .map_err(ServerApiOperationError::Authentication)?;
                match authorization {
                    SessionAuthorization::Authorized { expires_at } => {
                        Ok(AuthenticatedBrowserSession {
                            credential,
                            expires_at,
                        })
                    }
                    SessionAuthorization::InvalidSession => {
                        Err(ServerApiOperationError::Authentication(
                            ServerAuthenticationCoordinatorError::InvalidSession,
                        ))
                    }
                    SessionAuthorization::CsrfRejected => {
                        Err(ServerApiOperationError::Authentication(
                            ServerAuthenticationCoordinatorError::CsrfRejected,
                        ))
                    }
                }
            })
            .await
            .map_err(ServerApiOperationError::from)?
    }

    async fn logout(
        &self,
        session: AuthenticatedBrowserSession,
    ) -> Result<(), ServerApiOperationError> {
        self.lease
            .run_blocking(move |state| {
                state
                    .authentication
                    .logout(session.credential.expose_secret())
                    .map(|_revoked| ())
                    .map_err(ServerApiOperationError::Authentication)
            })
            .await
            .map_err(ServerApiOperationError::from)?
    }

    async fn change_password(
        &self,
        session: AuthenticatedBrowserSession,
        csrf: BrowserCsrfProof,
        current_password: ServerAuthenticationSecret,
        new_password: ServerAuthenticationSecret,
    ) -> Result<(), ServerApiOperationError> {
        let started_at = self.started_at;
        self.lease
            .run_blocking(move |state| {
                let now = started_at.elapsed();
                state
                    .authentication
                    .change_password(
                        session.credential.expose_secret(),
                        Some(csrf.expose_secret()),
                        now,
                        current_password.expose_secret().to_owned(),
                        new_password.expose_secret().to_owned(),
                    )
                    .map(|_revoked| ())
                    .map_err(ServerApiOperationError::Authentication)
            })
            .await
            .map_err(ServerApiOperationError::from)?
    }
}

impl fmt::Debug for ServerApiHost {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ServerApiHost(..)")
    }
}

#[derive(Clone)]
pub(crate) struct BrowserSessionCredential(Arc<Zeroizing<String>>);

impl BrowserSessionCredential {
    fn new(value: String) -> Self {
        Self(Arc::new(Zeroizing::new(value)))
    }

    pub(crate) fn expose_secret(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Debug for BrowserSessionCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("BrowserSessionCredential([REDACTED])")
    }
}

pub(crate) struct BrowserCsrfProof(Zeroizing<String>);

impl BrowserCsrfProof {
    fn new(value: String) -> Self {
        Self(Zeroizing::new(value))
    }

    fn expose_secret(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Debug for BrowserCsrfProof {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("BrowserCsrfProof([REDACTED])")
    }
}

#[derive(Clone, Debug)]
pub(crate) struct AuthenticatedBrowserSession {
    pub(crate) credential: BrowserSessionCredential,
    pub(crate) expires_at: Duration,
}

#[derive(Debug)]
pub(crate) enum ServerApiOperationError {
    Initialization(ServerOwnerInitializationError),
    Authentication(ServerAuthenticationCoordinatorError),
    InitializationRequired,
    Unavailable,
}

impl From<ServerBlockingError> for ServerApiOperationError {
    fn from(_error: ServerBlockingError) -> Self {
        Self::Unavailable
    }
}

pub(crate) fn router() -> Router<ApiState> {
    Router::new()
        .route("/api/v1/auth/status", get(status))
        .route("/api/v1/auth/initialize", post(initialize))
        .route("/api/v1/auth/session", post(login).get(get_session))
        .route("/api/v1/auth/logout", post(logout))
        .route("/api/v1/auth/password", patch(change_password))
}

pub(crate) fn is_public_route(path: &str, method: &axum::http::Method) -> bool {
    matches!(
        (path, method.as_str()),
        ("/api/v1/auth/status", "GET")
            | ("/api/v1/auth/initialize", "POST")
            | ("/api/v1/auth/session", "POST")
    )
}

pub(crate) fn is_browser_only_route(path: &str) -> bool {
    matches!(
        path,
        "/api/v1/auth/session" | "/api/v1/auth/logout" | "/api/v1/auth/password"
    )
}

pub(crate) fn request_intent(method: &axum::http::Method) -> RequestIntent {
    if *method == axum::http::Method::GET {
        RequestIntent::ReadOnly
    } else {
        RequestIntent::StateChanging
    }
}

pub(crate) fn browser_credentials(
    headers: &HeaderMap,
    intent: RequestIntent,
) -> Result<Option<(BrowserSessionCredential, Option<BrowserCsrfProof>)>, BrowserCredentialParseError>
{
    let Some(credential) = one_cookie(headers, SESSION_COOKIE_NAME)
        .map_err(|()| BrowserCredentialParseError::Session)?
    else {
        return Ok(None);
    };
    let csrf = if intent == RequestIntent::StateChanging {
        one_header(headers, CSRF_HEADER_NAME)
            .map_err(|()| BrowserCredentialParseError::Csrf)?
            .map(BrowserCsrfProof::new)
    } else {
        None
    };
    Ok(Some((BrowserSessionCredential::new(credential), csrf)))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BrowserCredentialParseError {
    Session,
    Csrf,
}

fn one_cookie(headers: &HeaderMap, name: &str) -> Result<Option<String>, ()> {
    let values = headers.get_all(header::COOKIE);
    if values.iter().count() > 1 {
        return Err(());
    }
    let Some(value) = values.iter().next() else {
        return Ok(None);
    };
    let value = value.to_str().map_err(|_| ())?;
    let mut found = None;
    for cookie in value.split(';') {
        let Some((cookie_name, cookie_value)) = cookie.trim().split_once('=') else {
            return Err(());
        };
        if cookie_name == name {
            if found.is_some() || cookie_value.is_empty() {
                return Err(());
            }
            found = Some(cookie_value.to_owned());
        }
    }
    Ok(found)
}

fn one_header(headers: &HeaderMap, name: &'static str) -> Result<Option<String>, ()> {
    let values = headers.get_all(name);
    if values.iter().count() > 1 {
        return Err(());
    }
    values
        .iter()
        .next()
        .map(|value| value.to_str().map(str::to_owned).map_err(|_invalid| ()))
        .transpose()
}

async fn status(State(state): State<ApiState>) -> Response {
    let Some(host) = state.server.as_ref() else {
        return api_error(ErrorCode::AuthenticationUnavailable, None);
    };
    match host.status().await {
        Ok(status) => Json(ServerAuthenticationStatusDto {
            initialization: match status {
                InitializationStatus::Pending | InitializationStatus::InProgress => {
                    ServerInitializationState::Required
                }
                InitializationStatus::Initialized => ServerInitializationState::Initialized,
                InitializationStatus::Unavailable => ServerInitializationState::Unavailable,
            },
        })
        .into_response(),
        Err(error) => operation_error_response(error),
    }
}

async fn initialize(State(state): State<ApiState>, request: Request<Body>) -> Response {
    let Some(host) = state.server.as_ref() else {
        return api_error(ErrorCode::AuthenticationUnavailable, None);
    };
    let client_id = match client_id(host, &request) {
        Ok(client_id) => client_id,
        Err(_missing) => return api_error(ErrorCode::AuthenticationUnavailable, None),
    };
    let request: InitializeServerOwnerRequest = match parse_sensitive_auth_json(request).await {
        Ok(request) => request,
        Err(response) => return response,
    };
    let (token, password) = request.into_parts();
    match host.initialize(client_id, token, password).await {
        Ok(session) => session_response(session),
        Err(error) => operation_error_response(error),
    }
}

async fn login(State(state): State<ApiState>, request: Request<Body>) -> Response {
    let Some(host) = state.server.as_ref() else {
        return api_error(ErrorCode::AuthenticationUnavailable, None);
    };
    let client_id = match client_id(host, &request) {
        Ok(client_id) => client_id,
        Err(_missing) => return api_error(ErrorCode::AuthenticationUnavailable, None),
    };
    let request: CreateServerSessionRequest = match parse_sensitive_auth_json(request).await {
        Ok(request) => request,
        Err(response) => return response,
    };
    match host.login(client_id, request.into_password()).await {
        Ok(session) => session_response(session),
        Err(error) => operation_error_response(error),
    }
}

async fn get_session() -> Json<ServerSessionDto> {
    Json(authenticated_session_dto())
}

async fn logout(State(state): State<ApiState>, mut request: Request<Body>) -> Response {
    let Some(host) = state.server.as_ref() else {
        return api_error(ErrorCode::AuthenticationUnavailable, None);
    };
    let Some(session) = request
        .extensions_mut()
        .remove::<AuthenticatedBrowserSession>()
    else {
        return api_error(ErrorCode::Unauthorized, None);
    };
    match host.logout(session).await {
        Ok(()) => cleared_session_response(),
        Err(error) => operation_error_response(error),
    }
}

async fn change_password(State(state): State<ApiState>, mut request: Request<Body>) -> Response {
    let Some(host) = state.server.as_ref() else {
        return api_error(ErrorCode::AuthenticationUnavailable, None);
    };
    let Some(session) = request
        .extensions_mut()
        .remove::<AuthenticatedBrowserSession>()
    else {
        return api_error(ErrorCode::Unauthorized, None);
    };
    let csrf = match one_header(request.headers(), CSRF_HEADER_NAME) {
        Ok(Some(csrf)) => BrowserCsrfProof::new(csrf),
        Ok(None) | Err(()) => return api_error(ErrorCode::CsrfRejected, None),
    };
    let request: ChangeServerOwnerPasswordRequest = match parse_sensitive_auth_json(request).await {
        Ok(request) => request,
        Err(response) => return response,
    };
    let (current_password, new_password) = request.into_parts();
    match host
        .change_password(session, csrf, current_password, new_password)
        .await
    {
        Ok(()) => cleared_session_response(),
        Err(error) => operation_error_response(error),
    }
}

fn client_id(host: &ServerApiHost, request: &Request<Body>) -> Result<u64, MissingDirectPeer> {
    let direct_peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|peer| peer.0);
    host.identify_request(direct_peer, request.headers())
}

fn authenticated_session_dto() -> ServerSessionDto {
    ServerSessionDto {
        state: ServerSessionState::Authenticated,
    }
}

fn session_response(session: IssuedSession) -> Response {
    let mut response = (StatusCode::CREATED, Json(authenticated_session_dto())).into_response();
    append_session_cookies(&mut response, session.credential(), session.csrf_token());
    response
}

fn append_session_cookies(response: &mut Response, credential: &str, csrf: &str) {
    let session =
        format!("{SESSION_COOKIE_NAME}={credential}; Path=/; Secure; HttpOnly; SameSite=Strict");
    let csrf = format!("{CSRF_COOKIE_NAME}={csrf}; Path=/; Secure; SameSite=Strict");
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&session).expect("generated session cookies are valid headers"),
    );
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&csrf).expect("generated CSRF cookies are valid headers"),
    );
}

fn cleared_session_response() -> Response {
    let mut response = StatusCode::NO_CONTENT.into_response();
    for cookie in [
        format!("{SESSION_COOKIE_NAME}=; Path=/; Max-Age=0; Secure; HttpOnly; SameSite=Strict"),
        format!("{CSRF_COOKIE_NAME}=; Path=/; Max-Age=0; Secure; SameSite=Strict"),
    ] {
        response.headers_mut().append(
            header::SET_COOKIE,
            HeaderValue::from_str(&cookie).expect("cookie clearing headers are valid"),
        );
    }
    response
}

pub(crate) fn operation_error_response(error: ServerApiOperationError) -> Response {
    match error {
        ServerApiOperationError::Initialization(error) => match error {
            ServerOwnerInitializationError::InvalidToken => {
                api_error(ErrorCode::InvalidCredentials, None)
            }
            ServerOwnerInitializationError::InProgress
            | ServerOwnerInitializationError::AlreadyInitialized => {
                api_error(ErrorCode::AlreadyInitialized, None)
            }
            ServerOwnerInitializationError::InvalidPassword => {
                api_error(ErrorCode::InvalidRequest, None)
            }
            ServerOwnerInitializationError::RateLimited { retry_after } => {
                rate_limited_response(retry_after)
            }
            ServerOwnerInitializationError::AtCapacity
            | ServerOwnerInitializationError::StateUnavailable
            | ServerOwnerInitializationError::StateUncertain => {
                api_error(ErrorCode::AuthenticationUnavailable, None)
            }
        },
        ServerApiOperationError::Authentication(error) => match error {
            ServerAuthenticationCoordinatorError::InvalidCredentials => {
                api_error(ErrorCode::InvalidCredentials, None)
            }
            ServerAuthenticationCoordinatorError::InvalidSession => {
                api_error(ErrorCode::Unauthorized, None)
            }
            ServerAuthenticationCoordinatorError::CsrfRejected => {
                api_error(ErrorCode::CsrfRejected, None)
            }
            ServerAuthenticationCoordinatorError::InvalidPassword => {
                api_error(ErrorCode::InvalidRequest, None)
            }
            ServerAuthenticationCoordinatorError::RateLimited { retry_after } => {
                rate_limited_response(retry_after)
            }
            ServerAuthenticationCoordinatorError::AtCapacity
            | ServerAuthenticationCoordinatorError::StateUnavailable
            | ServerAuthenticationCoordinatorError::StateUncertain => {
                api_error(ErrorCode::AuthenticationUnavailable, None)
            }
        },
        ServerApiOperationError::InitializationRequired => {
            api_error(ErrorCode::InitializationRequired, None)
        }
        ServerApiOperationError::Unavailable => {
            api_error(ErrorCode::AuthenticationUnavailable, None)
        }
    }
}

fn rate_limited_response(retry_after: Duration) -> Response {
    let seconds = retry_after
        .as_secs()
        .saturating_add(u64::from(retry_after.subsec_nanos() > 0))
        .clamp(1, MAX_SAFE_INTEGER);
    let details = ErrorDetails::RateLimit {
        retry_after_seconds: PositiveSafeInteger::new(seconds)
            .expect("clamped retry intervals are positive safe integers"),
    };
    let mut response = api_error(ErrorCode::AuthenticationRateLimited, Some(details));
    response.headers_mut().insert(
        header::RETRY_AFTER,
        HeaderValue::from_str(&seconds.to_string())
            .expect("positive retry intervals are valid header values"),
    );
    response
}
