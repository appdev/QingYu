use std::{ffi::OsString, net::SocketAddr, sync::Arc, time::Duration};

use axum::{
    body::{to_bytes, Body},
    extract::ConnectInfo,
    http::{header, Request, StatusCode},
    response::Response,
    Router,
};
use serde_json::{json, Value};
use tempfile::tempdir;
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    time::timeout,
};
use tower::ServiceExt as _;

use super::{
    build_router, build_server_router, ServerApiActivationError, ServerApiProcess,
    ServerApiProcessError, TransportPolicy,
};
use crate::{
    config::KernelConfig,
    contract::{ApiErrorEnvelope, ErrorCode, FrameErrorCode, ServerFrame},
    paths::{KernelPaths, ServerPathLayout},
    ports::KernelPorts,
    runtime::KernelRuntime,
    server::{
        AuthenticationRateLimiter, RateLimitPolicy, ServerAuthenticationSecurity,
        ServerAuthenticationStore, ServerLaunchEnvironment, SessionPolicy, SessionStore,
    },
};

const HOST: &str = "127.0.0.1:43123";
const ORIGIN: &str = "https://127.0.0.1:43123";
const INITIALIZATION_TOKEN: &str = "injected-random-initialization-token-at-least-32-bytes";
const OWNER_PASSWORD: &str = "correct horse battery staple";
const NEW_OWNER_PASSWORD: &str = "new correct horse battery staple";

struct ServerApiFixture {
    router: Router,
    native_credential: String,
    origin: String,
    _root: tempfile::TempDir,
    _runtime: Arc<KernelRuntime>,
}

impl ServerApiFixture {
    fn new() -> Self {
        Self::with_session_lifetime(Duration::from_secs(300))
    }

    fn with_session_lifetime(session_lifetime: Duration) -> Self {
        Self::with_origin_and_session_lifetime(ORIGIN, session_lifetime)
    }

    fn with_origin_and_session_lifetime(origin: &str, session_lifetime: Duration) -> Self {
        let root = tempdir().unwrap();
        let data = root.path().join("data");
        let cache = root.path().join("cache");
        std::fs::create_dir(&data).unwrap();
        std::fs::create_dir(&cache).unwrap();
        let paths = ServerPathLayout::for_test(&data, &cache)
            .activate()
            .unwrap();
        let security = server_security(&paths, session_lifetime);
        let environment = server_launch_environment();
        let config = KernelConfig::generate().unwrap();
        let native_credential = config.native_launch_credential().expose_secret().to_owned();
        let runtime = KernelRuntime::activate(config, paths, KernelPorts::unavailable()).unwrap();
        let process = ServerApiProcess::new(runtime.clone(), 2).unwrap();
        let activation = process.activate(security, environment).unwrap();
        let router = build_server_router(
            activation,
            TransportPolicy::same_origin(HOST, origin).unwrap(),
        );
        Self {
            router,
            native_credential,
            origin: origin.to_owned(),
            _root: root,
            _runtime: runtime,
        }
    }

    fn request(&self, method: &str, path: &str) -> Request<Body> {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, HOST)
            .header(header::ORIGIN, &self.origin)
            .body(Body::empty())
            .unwrap();
        request.extensions_mut().insert(ConnectInfo(
            "192.0.2.10:4242".parse::<SocketAddr>().unwrap(),
        ));
        request
    }

    fn json_request(&self, method: &str, path: &str, body: Value) -> Request<Body> {
        let mut request = self.request(method, path);
        request
            .headers_mut()
            .insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
        *request.body_mut() = Body::from(serde_json::to_vec(&body).unwrap());
        request
    }

    async fn initialize(&self) -> (String, String) {
        let response = self.initialize_response().await;
        let cookies = session_cookie_pair(&response);
        assert_eq!(
            response_json(response).await,
            json!({ "state": "authenticated" })
        );
        cookies
    }

    async fn initialize_response(&self) -> Response {
        let response = self
            .router
            .clone()
            .oneshot(self.json_request(
                "POST",
                "/api/v1/auth/initialize",
                json!({
                    "initializationToken": INITIALIZATION_TOKEN,
                    "password": OWNER_PASSWORD
                }),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        response
    }

    async fn login(&self, password: &str) -> Response {
        self.router
            .clone()
            .oneshot(self.json_request(
                "POST",
                "/api/v1/auth/session",
                json!({ "password": password }),
            ))
            .await
            .unwrap()
    }
}

fn server_launch_environment() -> ServerLaunchEnvironment {
    ServerLaunchEnvironment::from_lookup(|name| {
        (name == crate::server::SERVER_INITIALIZATION_TOKEN_ENV)
            .then(|| OsString::from(INITIALIZATION_TOKEN))
    })
    .unwrap()
}

fn server_security(
    paths: &KernelPaths,
    session_lifetime: Duration,
) -> ServerAuthenticationSecurity {
    let authentication = Arc::new(ServerAuthenticationStore::open(paths.config_root()).unwrap());
    let rate_policy =
        RateLimitPolicy::new(2, Duration::from_secs(60), Duration::from_secs(30)).unwrap();
    ServerAuthenticationSecurity::claim(
        authentication,
        AuthenticationRateLimiter::new(rate_policy, rate_policy),
        SessionStore::new(SessionPolicy::new(session_lifetime).unwrap()),
    )
    .unwrap()
}

async fn response_json(response: Response) -> Value {
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn session_cookie_pair(response: &Response) -> (String, String) {
    let cookies = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|value| value.to_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(cookies.len(), 2);
    let session = cookies
        .iter()
        .find(|value| value.starts_with("__Host-qingyu_session="))
        .unwrap();
    let csrf = cookies
        .iter()
        .find(|value| value.starts_with("__Host-qingyu_csrf="))
        .unwrap();
    assert!(session.contains("; Path=/"));
    assert!(session.contains("; Secure"));
    assert!(session.contains("; HttpOnly"));
    assert!(session.contains("; SameSite=Strict"));
    assert!(!session.contains("Domain="));
    assert!(csrf.contains("; Path=/"));
    assert!(csrf.contains("; Secure"));
    assert!(csrf.contains("; SameSite=Strict"));
    assert!(!csrf.contains("HttpOnly"));
    assert!(!csrf.contains("Domain="));
    let session_pair = session.split(';').next().unwrap().to_owned();
    let csrf_pair = csrf.split(';').next().unwrap().to_owned();
    (session_pair, csrf_pair)
}

fn cookie_header(session: &str, csrf: &str) -> String {
    format!("{session}; {csrf}")
}

fn csrf_value(csrf_pair: &str) -> &str {
    csrf_pair.split_once('=').unwrap().1
}

#[tokio::test]
async fn server_status_initialize_and_session_routes_use_only_host_cookies() {
    let api = ServerApiFixture::new();
    let status = api
        .router
        .clone()
        .oneshot(api.request("GET", "/api/v1/auth/status"))
        .await
        .unwrap();
    assert_eq!(status.status(), StatusCode::OK);
    assert_eq!(
        response_json(status).await,
        json!({ "initialization": "required" })
    );

    let before_initialization = api.login(OWNER_PASSWORD).await;
    assert_eq!(before_initialization.status(), StatusCode::CONFLICT);
    assert_eq!(
        response_json(before_initialization).await["code"],
        "initialization_required"
    );

    let (session, csrf) = api.initialize().await;
    assert_eq!(
        [session.as_str(), csrf.as_str()]
            .iter()
            .filter(|cookie| cookie.starts_with("qingyu_"))
            .count(),
        0
    );

    let mut get_session = api.request("GET", "/api/v1/auth/session");
    get_session.headers_mut().insert(
        header::COOKIE,
        cookie_header(&session, &csrf).parse().unwrap(),
    );
    let get_session = api.router.clone().oneshot(get_session).await.unwrap();
    assert_eq!(get_session.status(), StatusCode::OK);
    assert_eq!(
        response_json(get_session).await,
        json!({ "state": "authenticated" })
    );
}

#[tokio::test]
async fn http_server_uses_non_host_cookies_without_secure_and_rejects_https_cookie_names() {
    let api = ServerApiFixture::with_origin_and_session_lifetime(
        "http://127.0.0.1:43123",
        Duration::from_secs(300),
    );
    let response = api.initialize_response().await;
    let cookies = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|value| value.to_str().unwrap().to_owned())
        .collect::<Vec<_>>();

    assert_eq!(cookies.len(), 2);
    let session = cookies
        .iter()
        .find(|value| value.starts_with("qingyu_session="))
        .expect("HTTP session cookie");
    let csrf = cookies
        .iter()
        .find(|value| value.starts_with("qingyu_csrf="))
        .expect("HTTP CSRF cookie");
    assert!(session.contains("; Path=/"));
    assert!(session.contains("; HttpOnly"));
    assert!(session.contains("; SameSite=Strict"));
    assert!(!session.contains("; Secure"));
    assert!(!session.contains("Domain="));
    assert!(csrf.contains("; Path=/"));
    assert!(csrf.contains("; SameSite=Strict"));
    assert!(!csrf.contains("; HttpOnly"));
    assert!(!csrf.contains("; Secure"));
    assert!(!csrf.contains("Domain="));
    assert!(cookies.iter().all(|value| !value.starts_with("__Host-")));

    let session_pair = session.split(';').next().unwrap().to_owned();
    let csrf_pair = csrf.split(';').next().unwrap().to_owned();
    let mut accepted = api.request("GET", "/api/v1/auth/session");
    accepted.headers_mut().insert(
        header::COOKIE,
        cookie_header(&session_pair, &csrf_pair).parse().unwrap(),
    );
    assert_eq!(
        api.router.clone().oneshot(accepted).await.unwrap().status(),
        StatusCode::OK
    );

    let mut mismatched = api.request("GET", "/api/v1/auth/session");
    let session_value = session_pair.split_once('=').unwrap().1;
    let csrf_token = csrf_value(&csrf_pair);
    mismatched.headers_mut().insert(
        header::COOKIE,
        format!("__Host-qingyu_session={session_value}; __Host-qingyu_csrf={csrf_token}")
            .parse()
            .unwrap(),
    );
    assert_eq!(
        api.router
            .clone()
            .oneshot(mismatched)
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );

    let mut coexisting = api.request("GET", "/api/v1/auth/session");
    coexisting.headers_mut().insert(
        header::COOKIE,
        format!(
            "{}; __Host-qingyu_session={session_value}",
            cookie_header(&session_pair, &csrf_pair)
        )
        .parse()
        .unwrap(),
    );
    assert_eq!(
        api.router
            .clone()
            .oneshot(coexisting)
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );

    let mut wrong_host = api.request("GET", "/api/v1/auth/status");
    wrong_host
        .headers_mut()
        .insert(header::HOST, "attacker.invalid".parse().unwrap());
    assert_eq!(
        api.router
            .clone()
            .oneshot(wrong_host)
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );

    let mut wrong_origin = api.request("GET", "/api/v1/auth/status");
    wrong_origin
        .headers_mut()
        .insert(header::ORIGIN, "https://127.0.0.1:43123".parse().unwrap());
    assert_eq!(
        api.router
            .clone()
            .oneshot(wrong_origin)
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );

    let mut wrong_csrf = api.request("POST", "/api/v1/auth/logout");
    wrong_csrf.headers_mut().insert(
        header::COOKIE,
        cookie_header(&session_pair, &csrf_pair).parse().unwrap(),
    );
    wrong_csrf
        .headers_mut()
        .insert("x-csrf-token", "wrong-proof".parse().unwrap());
    assert_eq!(
        api.router
            .clone()
            .oneshot(wrong_csrf)
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );

    let mut logout = api.request("POST", "/api/v1/auth/logout");
    logout.headers_mut().insert(
        header::COOKIE,
        cookie_header(&session_pair, &csrf_pair).parse().unwrap(),
    );
    logout
        .headers_mut()
        .insert("x-csrf-token", csrf_value(&csrf_pair).parse().unwrap());
    let logout = api.router.clone().oneshot(logout).await.unwrap();
    assert_eq!(logout.status(), StatusCode::NO_CONTENT);
    let cleared = logout
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|value| value.to_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(cleared.len(), 2);
    assert!(cleared
        .iter()
        .any(|value| value.starts_with("qingyu_session=")));
    assert!(cleared
        .iter()
        .any(|value| value.starts_with("qingyu_csrf=")));
    assert!(cleared.iter().all(|value| value.contains("Max-Age=0")));
    assert!(cleared.iter().all(|value| !value.contains("; Secure")));
}

#[tokio::test]
async fn https_server_rejects_http_cookie_names() {
    let api = ServerApiFixture::new();
    let (session, csrf) = api.initialize().await;
    let session_value = session.split_once('=').unwrap().1;
    let csrf_token = csrf_value(&csrf);
    let mut request = api.request("GET", "/api/v1/auth/session");
    request.headers_mut().insert(
        header::COOKIE,
        format!("qingyu_session={session_value}; qingyu_csrf={csrf_token}")
            .parse()
            .unwrap(),
    );

    assert_eq!(
        api.router.clone().oneshot(request).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );

    let mut coexisting = api.request("GET", "/api/v1/auth/session");
    coexisting.headers_mut().insert(
        header::COOKIE,
        format!(
            "{}; qingyu_session={session_value}",
            cookie_header(&session, &csrf)
        )
        .parse()
        .unwrap(),
    );
    assert_eq!(
        api.router
            .clone()
            .oneshot(coexisting)
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn protected_routes_accept_native_or_browser_and_require_csrf_for_browser_mutations() {
    let api = ServerApiFixture::new();
    let (session, csrf) = api.initialize().await;

    let mut browser_read = api.request("GET", "/api/v1/runtime");
    browser_read.headers_mut().insert(
        header::COOKIE,
        cookie_header(&session, &csrf).parse().unwrap(),
    );
    assert_ne!(
        api.router
            .clone()
            .oneshot(browser_read)
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );

    let mut browser_mutation = api.request("POST", "/api/v1/documents");
    browser_mutation.headers_mut().insert(
        header::COOKIE,
        cookie_header(&session, &csrf).parse().unwrap(),
    );
    let rejected = api.router.clone().oneshot(browser_mutation).await.unwrap();
    assert_eq!(rejected.status(), StatusCode::FORBIDDEN);
    let envelope: ApiErrorEnvelope = serde_json::from_value(response_json(rejected).await).unwrap();
    assert_eq!(envelope.code(), ErrorCode::CsrfRejected);

    let mut duplicate_cookie = api.request("GET", "/api/v1/runtime");
    duplicate_cookie
        .headers_mut()
        .append(header::COOKIE, session.parse().unwrap());
    duplicate_cookie
        .headers_mut()
        .append(header::COOKIE, csrf.parse().unwrap());
    assert_eq!(
        api.router
            .clone()
            .oneshot(duplicate_cookie)
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );

    let mut malformed_after_session = api.request("GET", "/api/v1/runtime");
    malformed_after_session.headers_mut().insert(
        header::COOKIE,
        format!("{session}; malformed").parse().unwrap(),
    );
    assert_eq!(
        api.router
            .clone()
            .oneshot(malformed_after_session)
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );

    let mut duplicate_csrf = api.request("POST", "/api/v1/documents");
    duplicate_csrf.headers_mut().insert(
        header::COOKIE,
        cookie_header(&session, &csrf).parse().unwrap(),
    );
    duplicate_csrf
        .headers_mut()
        .append("x-csrf-token", csrf_value(&csrf).parse().unwrap());
    duplicate_csrf
        .headers_mut()
        .append("x-csrf-token", csrf_value(&csrf).parse().unwrap());
    assert_eq!(
        api.router
            .clone()
            .oneshot(duplicate_csrf)
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );

    let mut accepted = api.request("POST", "/api/v1/documents");
    accepted.headers_mut().insert(
        header::COOKIE,
        cookie_header(&session, &csrf).parse().unwrap(),
    );
    accepted
        .headers_mut()
        .insert("x-csrf-token", csrf_value(&csrf).parse().unwrap());
    assert_eq!(
        api.router.clone().oneshot(accepted).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );

    let mut native = api.request("POST", "/api/v1/documents");
    native.headers_mut().insert(
        header::AUTHORIZATION,
        format!("Bearer {}", api.native_credential).parse().unwrap(),
    );
    assert_eq!(
        api.router.clone().oneshot(native).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn resource_upload_route_enforces_origin_session_csrf_and_native_bearer() {
    let api = ServerApiFixture::new();
    let (session, csrf) = api.initialize().await;
    let path = "/api/v1/documents/document-1/resources?workspaceGeneration=generation-1&folder=assets&name=asset.bin&kind=attachment";

    let mut missing_csrf = api.request("POST", path);
    missing_csrf.headers_mut().insert(
        header::COOKIE,
        cookie_header(&session, &csrf).parse().unwrap(),
    );
    missing_csrf.headers_mut().insert(
        header::CONTENT_TYPE,
        "application/octet-stream".parse().unwrap(),
    );
    *missing_csrf.body_mut() = Body::from("asset");
    let rejected = api.router.clone().oneshot(missing_csrf).await.unwrap();
    assert_eq!(rejected.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        serde_json::from_value::<ApiErrorEnvelope>(response_json(rejected).await)
            .unwrap()
            .code(),
        ErrorCode::CsrfRejected
    );

    let mut browser = api.request("POST", path);
    browser.headers_mut().insert(
        header::COOKIE,
        cookie_header(&session, &csrf).parse().unwrap(),
    );
    browser
        .headers_mut()
        .insert("x-csrf-token", csrf_value(&csrf).parse().unwrap());
    browser.headers_mut().insert(
        header::CONTENT_TYPE,
        "application/octet-stream".parse().unwrap(),
    );
    *browser.body_mut() = Body::from("asset");
    assert_eq!(
        api.router.clone().oneshot(browser).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );

    let mut wrong_origin = api.request("POST", path);
    wrong_origin
        .headers_mut()
        .insert(header::ORIGIN, "https://attacker.invalid".parse().unwrap());
    wrong_origin.headers_mut().insert(
        header::AUTHORIZATION,
        format!("Bearer {}", api.native_credential).parse().unwrap(),
    );
    wrong_origin.headers_mut().insert(
        header::CONTENT_TYPE,
        "application/octet-stream".parse().unwrap(),
    );
    *wrong_origin.body_mut() = Body::from("asset");
    let rejected = api.router.clone().oneshot(wrong_origin).await.unwrap();
    assert_eq!(rejected.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        serde_json::from_value::<ApiErrorEnvelope>(response_json(rejected).await)
            .unwrap()
            .code(),
        ErrorCode::OriginNotAllowed
    );

    let mut unauthenticated = api.request("POST", path);
    unauthenticated.headers_mut().insert(
        header::CONTENT_TYPE,
        "application/octet-stream".parse().unwrap(),
    );
    *unauthenticated.body_mut() = Body::from("asset");
    assert_eq!(
        api.router
            .clone()
            .oneshot(unauthenticated)
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );

    let mut native = api.request("POST", path);
    native.headers_mut().insert(
        header::AUTHORIZATION,
        format!("Bearer {}", api.native_credential).parse().unwrap(),
    );
    native.headers_mut().insert(
        header::CONTENT_TYPE,
        "application/octet-stream".parse().unwrap(),
    );
    *native.body_mut() = Body::from("asset");
    assert_eq!(
        api.router.oneshot(native).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn resource_batch_route_enforces_host_origin_session_csrf_and_native_bearer() {
    let api = ServerApiFixture::new();
    let (session, csrf) = api.initialize().await;
    let path = "/api/v1/documents/document-1/resource-batches";

    let mut missing_csrf = api.json_request("POST", path, json!({}));
    missing_csrf.headers_mut().insert(
        header::COOKIE,
        cookie_header(&session, &csrf).parse().unwrap(),
    );
    let rejected = api.router.clone().oneshot(missing_csrf).await.unwrap();
    assert_eq!(rejected.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        serde_json::from_value::<ApiErrorEnvelope>(response_json(rejected).await)
            .unwrap()
            .code(),
        ErrorCode::CsrfRejected
    );

    let mut browser = api.json_request("POST", path, json!({}));
    browser.headers_mut().insert(
        header::COOKIE,
        cookie_header(&session, &csrf).parse().unwrap(),
    );
    browser
        .headers_mut()
        .insert("x-csrf-token", csrf_value(&csrf).parse().unwrap());
    assert_eq!(
        api.router.clone().oneshot(browser).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );

    let mut wrong_origin = api.json_request("POST", path, json!({}));
    wrong_origin
        .headers_mut()
        .insert(header::ORIGIN, "https://attacker.invalid".parse().unwrap());
    wrong_origin.headers_mut().insert(
        header::AUTHORIZATION,
        format!("Bearer {}", api.native_credential).parse().unwrap(),
    );
    let rejected = api.router.clone().oneshot(wrong_origin).await.unwrap();
    assert_eq!(rejected.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        serde_json::from_value::<ApiErrorEnvelope>(response_json(rejected).await)
            .unwrap()
            .code(),
        ErrorCode::OriginNotAllowed
    );

    let mut wrong_host = api.json_request("POST", path, json!({}));
    wrong_host
        .headers_mut()
        .insert(header::HOST, "attacker.invalid".parse().unwrap());
    wrong_host.headers_mut().insert(
        header::AUTHORIZATION,
        format!("Bearer {}", api.native_credential).parse().unwrap(),
    );
    let rejected = api.router.clone().oneshot(wrong_host).await.unwrap();
    assert_eq!(rejected.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        serde_json::from_value::<ApiErrorEnvelope>(response_json(rejected).await)
            .unwrap()
            .code(),
        ErrorCode::HostNotAllowed
    );

    let unauthenticated = api.json_request("POST", path, json!({}));
    assert_eq!(
        api.router
            .clone()
            .oneshot(unauthenticated)
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );

    let mut native = api.json_request("POST", path, json!({}));
    native.headers_mut().remove(header::ORIGIN);
    native.headers_mut().insert(
        header::AUTHORIZATION,
        format!("Bearer {}", api.native_credential).parse().unwrap(),
    );
    assert_eq!(
        api.router.oneshot(native).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn logout_requires_csrf_clears_both_cookies_and_revokes_the_session() {
    let api = ServerApiFixture::new();
    let (session, csrf) = api.initialize().await;

    let mut request = api.request("POST", "/api/v1/auth/logout");
    request.headers_mut().insert(
        header::COOKIE,
        cookie_header(&session, &csrf).parse().unwrap(),
    );
    request
        .headers_mut()
        .insert("x-csrf-token", csrf_value(&csrf).parse().unwrap());
    let response = api.router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let cleared = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|value| value.to_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(cleared.len(), 2);
    assert!(cleared.iter().all(|value| value.contains("Max-Age=0")));

    let mut stale = api.request("GET", "/api/v1/auth/session");
    stale.headers_mut().insert(
        header::COOKIE,
        cookie_header(&session, &csrf).parse().unwrap(),
    );
    let stale = api.router.clone().oneshot(stale).await.unwrap();
    assert_eq!(stale.status(), StatusCode::UNAUTHORIZED);
    assert!(stale.headers().get(header::SET_COOKIE).is_none());
}

#[tokio::test]
async fn password_change_revokes_old_sessions_clears_cookies_and_uses_the_new_password() {
    let api = ServerApiFixture::new();
    let (session, csrf) = api.initialize().await;

    let mut wrong_csrf = api.json_request(
        "PATCH",
        "/api/v1/auth/password",
        json!({
            "currentPassword": OWNER_PASSWORD,
            "newPassword": NEW_OWNER_PASSWORD
        }),
    );
    wrong_csrf.headers_mut().insert(
        header::COOKIE,
        cookie_header(&session, &csrf).parse().unwrap(),
    );
    wrong_csrf
        .headers_mut()
        .insert("x-csrf-token", "wrong-csrf".parse().unwrap());
    let wrong_csrf = api.router.clone().oneshot(wrong_csrf).await.unwrap();
    assert_eq!(wrong_csrf.status(), StatusCode::FORBIDDEN);
    assert!(wrong_csrf.headers().get(header::SET_COOKIE).is_none());

    let mut change = api.json_request(
        "PATCH",
        "/api/v1/auth/password",
        json!({
            "currentPassword": OWNER_PASSWORD,
            "newPassword": NEW_OWNER_PASSWORD
        }),
    );
    change.headers_mut().insert(
        header::COOKIE,
        cookie_header(&session, &csrf).parse().unwrap(),
    );
    change
        .headers_mut()
        .insert("x-csrf-token", csrf_value(&csrf).parse().unwrap());
    let changed = api.router.clone().oneshot(change).await.unwrap();
    assert_eq!(changed.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        changed.headers().get_all(header::SET_COOKIE).iter().count(),
        2
    );
    assert!(changed
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .all(|value| value.to_str().unwrap().contains("Max-Age=0")));

    let mut stale = api.request("GET", "/api/v1/auth/session");
    stale.headers_mut().insert(
        header::COOKIE,
        cookie_header(&session, &csrf).parse().unwrap(),
    );
    assert_eq!(
        api.router.clone().oneshot(stale).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );

    let old_password = api.login(OWNER_PASSWORD).await;
    assert_eq!(old_password.status(), StatusCode::UNAUTHORIZED);
    assert!(old_password.headers().get(header::SET_COOKIE).is_none());

    let new_password = api.login(NEW_OWNER_PASSWORD).await;
    assert_eq!(new_password.status(), StatusCode::CREATED);
    let _cookies = session_cookie_pair(&new_password);
}

#[tokio::test]
async fn rate_limit_header_matches_the_safe_body_and_forwarded_headers_are_ignored() {
    let api = ServerApiFixture::new();
    let _session = api.initialize().await;

    let mut first = api.json_request(
        "POST",
        "/api/v1/auth/session",
        json!({ "password": "definitely-wrong" }),
    );
    first
        .headers_mut()
        .insert("x-forwarded-for", "198.51.100.1".parse().unwrap());
    let first = api.router.clone().oneshot(first).await.unwrap();
    assert_eq!(first.status(), StatusCode::UNAUTHORIZED);
    assert!(first.headers().get(header::SET_COOKIE).is_none());

    let mut second = api.json_request(
        "POST",
        "/api/v1/auth/session",
        json!({ "password": "still-wrong" }),
    );
    second
        .headers_mut()
        .insert("x-forwarded-for", "203.0.113.200".parse().unwrap());
    second
        .headers_mut()
        .insert("forwarded", "for=203.0.113.201".parse().unwrap());
    let second = api.router.clone().oneshot(second).await.unwrap();
    assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(second.headers().get(header::SET_COOKIE).is_none());
    assert_eq!(
        second.headers()[header::ACCESS_CONTROL_EXPOSE_HEADERS],
        "Retry-After, X-Request-Id, X-Content-Type-Options"
    );
    let retry_after = second.headers()[header::RETRY_AFTER]
        .to_str()
        .unwrap()
        .parse::<u64>()
        .unwrap();
    let body = response_json(second).await;
    assert_eq!(body["code"], "authentication_rate_limited");
    assert_eq!(body["details"]["retryAfterSeconds"], retry_after);
}

#[tokio::test]
async fn auth_route_method_allowlist_and_same_origin_policy_fail_closed() {
    let api = ServerApiFixture::new();
    let wrong_method = api
        .router
        .clone()
        .oneshot(api.request("PATCH", "/api/v1/auth/status"))
        .await
        .unwrap();
    assert_eq!(wrong_method.status(), StatusCode::BAD_REQUEST);

    let mut wrong_origin = api.request("GET", "/api/v1/auth/status");
    wrong_origin
        .headers_mut()
        .insert(header::ORIGIN, "https://attacker.invalid".parse().unwrap());
    let wrong_origin = api.router.clone().oneshot(wrong_origin).await.unwrap();
    assert_eq!(wrong_origin.status(), StatusCode::FORBIDDEN);
    assert!(wrong_origin
        .headers()
        .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
        .is_none());
}

#[tokio::test]
async fn missing_direct_peer_fails_before_initialization_and_does_not_spend_the_token() {
    let api = ServerApiFixture::new();
    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/initialize")
        .header(header::HOST, HOST)
        .header(header::ORIGIN, ORIGIN)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&json!({
                "initializationToken": INITIALIZATION_TOKEN,
                "password": OWNER_PASSWORD
            }))
            .unwrap(),
        ))
        .unwrap();
    let missing_peer = api.router.clone().oneshot(request).await.unwrap();
    assert_eq!(missing_peer.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(missing_peer.headers().get(header::SET_COOKIE).is_none());

    let _session = api.initialize().await;
}

#[tokio::test]
async fn oversized_sensitive_body_is_rejected_without_spending_initialization() {
    let api = ServerApiFixture::new();
    let oversized = api.json_request(
        "POST",
        "/api/v1/auth/initialize",
        json!({
            "initializationToken": INITIALIZATION_TOKEN,
            "password": "x".repeat(17 * 1024)
        }),
    );
    let oversized = api.router.clone().oneshot(oversized).await.unwrap();
    assert_eq!(oversized.status(), StatusCode::BAD_REQUEST);
    assert!(oversized.headers().get(header::SET_COOKIE).is_none());

    let _session = api.initialize().await;
}

#[tokio::test]
async fn browser_websocket_uses_the_session_cookie_and_closes_after_revocation() {
    let api = ServerApiFixture::new();
    let (session, csrf) = api.initialize().await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = api
        .router
        .clone()
        .into_make_service_with_connect_info::<SocketAddr>();
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let cookies = cookie_header(&session, &csrf);
    let mut socket = RawWebSocket::connect(address, HOST, ORIGIN, Some(&cookies)).await;
    assert!(matches!(
        socket.read_server_frame().await,
        ServerFrame::Ready { .. }
    ));

    let mut logout = api.request("POST", "/api/v1/auth/logout");
    logout.headers_mut().insert(
        header::COOKIE,
        cookie_header(&session, &csrf).parse().unwrap(),
    );
    logout
        .headers_mut()
        .insert("x-csrf-token", csrf_value(&csrf).parse().unwrap());
    assert_eq!(
        api.router.clone().oneshot(logout).await.unwrap().status(),
        StatusCode::NO_CONTENT
    );

    timeout(Duration::from_secs(2), async {
        match socket.read_server_frame().await {
            ServerFrame::Error { code, .. } => assert_eq!(code, FrameErrorCode::Unauthorized),
            other => panic!("expected revoked websocket error, got {other:?}"),
        }
        assert_eq!(socket.read_message().await, WsMessage::Close(4001));
    })
    .await
    .expect("revoked browser websocket must close promptly");
    server.abort();
}

#[tokio::test]
async fn browser_websocket_closes_at_session_expiry_without_a_csrf_frame() {
    let api = ServerApiFixture::with_session_lifetime(Duration::from_secs(5));
    let (session, csrf) = api.initialize().await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = api
        .router
        .clone()
        .into_make_service_with_connect_info::<SocketAddr>();
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let cookies = cookie_header(&session, &csrf);
    let mut socket = RawWebSocket::connect(address, HOST, ORIGIN, Some(&cookies)).await;
    assert!(matches!(
        socket.read_server_frame().await,
        ServerFrame::Ready { .. }
    ));

    timeout(Duration::from_secs(7), async {
        match socket.read_server_frame().await {
            ServerFrame::Error { code, .. } => assert_eq!(code, FrameErrorCode::Unauthorized),
            other => panic!("expected expired websocket error, got {other:?}"),
        }
        assert_eq!(socket.read_message().await, WsMessage::Close(4001));
    })
    .await
    .expect("expired browser websocket must close at the session deadline");
    server.abort();
}

#[tokio::test]
async fn desktop_router_does_not_register_server_authentication_routes() {
    let root = tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let app_data = root.path().join("app-data");
    let cache = root.path().join("cache");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&app_data).unwrap();
    std::fs::create_dir(&cache).unwrap();
    let config = KernelConfig::generate().unwrap();
    let credential = config.native_launch_credential().expose_secret().to_owned();
    let runtime = KernelRuntime::activate(
        config,
        KernelPaths::desktop(&workspace, &app_data, &cache).unwrap(),
        KernelPorts::unavailable(),
    )
    .unwrap();
    let router = build_router(
        runtime,
        TransportPolicy::loopback(HOST, "tauri://localhost").unwrap(),
    );
    let request = Request::builder()
        .method("GET")
        .uri("/api/v1/auth/status")
        .header(header::HOST, HOST)
        .header(header::AUTHORIZATION, format!("Bearer {credential}"))
        .body(Body::empty())
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[test]
fn server_activation_rejects_a_non_server_runtime() {
    let root = tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let app_data = root.path().join("app-data");
    let cache = root.path().join("cache");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&app_data).unwrap();
    std::fs::create_dir(&cache).unwrap();
    let paths = KernelPaths::desktop(&workspace, &app_data, &cache).unwrap();
    let security = server_security(&paths, Duration::from_secs(300));
    let runtime = KernelRuntime::activate(
        KernelConfig::generate().unwrap(),
        paths,
        KernelPorts::unavailable(),
    )
    .unwrap();

    drop(security);
    assert_eq!(
        ServerApiProcess::new(runtime, 2).unwrap_err(),
        ServerApiProcessError::NonServerProfile
    );
}

#[test]
fn server_activation_rejects_authentication_from_another_config_root() {
    let runtime_root = tempdir().unwrap();
    let runtime_data = runtime_root.path().join("data");
    let runtime_cache = runtime_root.path().join("cache");
    std::fs::create_dir(&runtime_data).unwrap();
    std::fs::create_dir(&runtime_cache).unwrap();
    let runtime_paths = ServerPathLayout::for_test(&runtime_data, &runtime_cache)
        .activate()
        .unwrap();
    let runtime = KernelRuntime::activate(
        KernelConfig::generate().unwrap(),
        runtime_paths,
        KernelPorts::unavailable(),
    )
    .unwrap();

    let authentication_root = tempdir().unwrap();
    let authentication_data = authentication_root.path().join("data");
    let authentication_cache = authentication_root.path().join("cache");
    std::fs::create_dir(&authentication_data).unwrap();
    std::fs::create_dir(&authentication_cache).unwrap();
    let authentication_paths =
        ServerPathLayout::for_test(&authentication_data, &authentication_cache)
            .activate()
            .unwrap();
    let security = server_security(&authentication_paths, Duration::from_secs(300));

    assert_eq!(
        ServerApiProcess::new(runtime.clone(), 2)
            .unwrap()
            .activate(security, server_launch_environment())
            .unwrap_err(),
        ServerApiActivationError::AuthenticationRootMismatch
    );
    assert_eq!(
        ServerApiProcess::new(runtime, 2).unwrap_err(),
        ServerApiProcessError::AlreadyClaimed
    );
}

#[test]
fn server_process_claim_survives_an_unactivated_owner_drop() {
    let root = tempdir().unwrap();
    let data = root.path().join("data");
    let cache = root.path().join("cache");
    std::fs::create_dir(&data).unwrap();
    std::fs::create_dir(&cache).unwrap();
    let paths = ServerPathLayout::for_test(&data, &cache)
        .activate()
        .unwrap();
    let runtime = KernelRuntime::activate(
        KernelConfig::generate().unwrap(),
        paths,
        KernelPorts::unavailable(),
    )
    .unwrap();

    drop(ServerApiProcess::new(runtime.clone(), 2).unwrap());

    assert_eq!(
        ServerApiProcess::new(runtime, 2).unwrap_err(),
        ServerApiProcessError::AlreadyClaimed
    );
}

#[test]
fn server_transport_policy_requires_one_exact_same_origin_authority() {
    assert!(TransportPolicy::same_origin(HOST, ORIGIN).is_ok());
    assert!(TransportPolicy::same_origin(HOST, "http://127.0.0.1:43123").is_ok());
    assert!(TransportPolicy::same_origin(HOST, "*").is_err());
    assert!(TransportPolicy::same_origin(HOST, "https://attacker.invalid").is_err());
    assert!(TransportPolicy::same_origin(HOST, "https://127.0.0.1:43123/path").is_err());
    assert!(TransportPolicy::same_origin(HOST, "http://127.0.0.1:43123/path").is_err());
    assert!(TransportPolicy::same_origin(HOST, "https://127.0.0.1:43123/").is_err());
    assert!(TransportPolicy::same_origin(HOST, "http://127.0.0.1:43123/").is_err());
    assert!(TransportPolicy::same_origin(HOST, "HTTPS://127.0.0.1:43123").is_err());
    assert!(TransportPolicy::same_origin(HOST, "HTTP://127.0.0.1:43123").is_err());
}

#[derive(Debug, Eq, PartialEq)]
enum WsMessage {
    Text(String),
    Close(u16),
}

struct RawWebSocket {
    stream: TcpStream,
}

impl RawWebSocket {
    async fn connect(address: SocketAddr, host: &str, origin: &str, cookie: Option<&str>) -> Self {
        let mut stream = TcpStream::connect(address).await.unwrap();
        let cookie = cookie.map_or(String::new(), |cookie| format!("Cookie: {cookie}\r\n"));
        let request = format!(
            "GET /api/v1/events HTTP/1.1\r\n\
             Host: {host}\r\n\
             Origin: {origin}\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: AAECAwQFBgcICQoLDA0ODw==\r\n\
             Sec-WebSocket-Version: 13\r\n\
             {cookie}\r\n"
        );
        stream.write_all(request.as_bytes()).await.unwrap();

        let mut response = Vec::new();
        let mut byte = [0_u8; 1];
        while !response.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).await.unwrap();
            response.push(byte[0]);
            assert!(response.len() < 16 * 1024);
        }
        let response = String::from_utf8(response).unwrap();
        assert!(
            response.starts_with("HTTP/1.1 101 "),
            "websocket upgrade failed: {response}"
        );
        Self { stream }
    }

    async fn read_server_frame(&mut self) -> ServerFrame {
        match self.read_message().await {
            WsMessage::Text(text) => serde_json::from_str(&text).unwrap(),
            WsMessage::Close(code) => panic!("expected a server frame before close {code}"),
        }
    }

    async fn read_message(&mut self) -> WsMessage {
        loop {
            let mut header = [0_u8; 2];
            self.stream.read_exact(&mut header).await.unwrap();
            assert_ne!(header[0] & 0x80, 0);
            assert_eq!(header[1] & 0x80, 0);
            let length = match header[1] & 0x7f {
                length @ 0..=125 => u64::from(length),
                126 => {
                    let mut bytes = [0_u8; 2];
                    self.stream.read_exact(&mut bytes).await.unwrap();
                    u64::from(u16::from_be_bytes(bytes))
                }
                127 => {
                    let mut bytes = [0_u8; 8];
                    self.stream.read_exact(&mut bytes).await.unwrap();
                    u64::from_be_bytes(bytes)
                }
                _ => unreachable!(),
            };
            let length = usize::try_from(length).unwrap();
            assert!(length <= 1024 * 1024);
            let mut payload = vec![0_u8; length];
            self.stream.read_exact(&mut payload).await.unwrap();
            match header[0] & 0x0f {
                0x1 => return WsMessage::Text(String::from_utf8(payload).unwrap()),
                0x8 => {
                    let code = payload
                        .get(..2)
                        .map(|bytes| u16::from_be_bytes([bytes[0], bytes[1]]))
                        .unwrap_or(1005);
                    return WsMessage::Close(code);
                }
                0x9 => self.send_pong(&payload).await,
                opcode => panic!("unexpected server websocket opcode {opcode:#x}"),
            }
        }
    }

    async fn send_pong(&mut self, payload: &[u8]) {
        let mask = [0x52, 0x0b, 0xa6, 0xd1];
        let mut frame = vec![0x8a, 0x80 | payload.len() as u8];
        frame.extend_from_slice(&mask);
        frame.extend(
            payload
                .iter()
                .enumerate()
                .map(|(index, byte)| byte ^ mask[index % mask.len()]),
        );
        self.stream.write_all(&frame).await.unwrap();
    }
}
