use std::{ffi::OsString, path::Path, sync::Arc, time::Duration};

use axum::{
    body::{to_bytes, Body},
    http::{header, Request, StatusCode},
    response::Response,
    Router,
};
use tempfile::{tempdir, TempDir};
use tower::ServiceExt as _;

use super::{
    build_router, build_server_router, build_server_web_router, ServerApiActivation,
    ServerApiProcess, TransportPolicy,
};
use crate::{
    config::KernelConfig,
    paths::{KernelPaths, ServerPathLayout},
    ports::KernelPorts,
    runtime::KernelRuntime,
    server::{
        AuthenticationRateLimiter, RateLimitPolicy, ServerAuthenticationSecurity,
        ServerAuthenticationStore, ServerLaunchEnvironment, SessionPolicy, SessionStore,
    },
};

const HOST: &str = "notes.example.test";
const ORIGIN: &str = "https://notes.example.test";
const INITIALIZATION_TOKEN: &str = "server-web-test-initialization-token-at-least-32-bytes";
const INDEX: &str = "<!doctype html><main>QingYu Web</main>";
const JAVASCRIPT: &str = "globalThis.__qingyuWeb = true;";

struct ServerWebFixture {
    router: Router,
    _root: TempDir,
    _runtime: Arc<KernelRuntime>,
}

impl ServerWebFixture {
    fn new() -> Self {
        let root = tempdir().unwrap();
        let web = root.path().join("web");
        std::fs::create_dir_all(web.join("assets")).unwrap();
        std::fs::write(web.join("index.html"), INDEX).unwrap();
        std::fs::write(web.join("assets/app.js"), JAVASCRIPT).unwrap();
        std::fs::write(web.join(".hidden"), "hidden secret").unwrap();
        let (activation, runtime) = server_activation(root.path());
        let router = build_server_web_router(
            activation,
            TransportPolicy::same_origin(HOST, ORIGIN).unwrap(),
            &web,
        )
        .unwrap();
        Self {
            router,
            _root: root,
            _runtime: runtime,
        }
    }

    fn request(&self, method: &str, path: &str) -> Request<Body> {
        request(method, path)
    }

    async fn response(&self, method: &str, path: &str) -> Response {
        self.router
            .clone()
            .oneshot(self.request(method, path))
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn server_web_entry_assets_and_spa_fallback_are_public_without_a_session() {
    let web = ServerWebFixture::new();

    let root = web.response("GET", "/").await;
    assert_eq!(root.status(), StatusCode::OK);
    assert_eq!(
        root.headers()[header::CONTENT_TYPE],
        "text/html; charset=utf-8"
    );
    assert_eq!(response_text(root).await, INDEX);

    let asset = web.response("GET", "/assets/app.js").await;
    assert_eq!(asset.status(), StatusCode::OK);
    assert_eq!(
        asset.headers()[header::CONTENT_TYPE],
        "text/javascript; charset=utf-8"
    );
    assert_eq!(response_text(asset).await, JAVASCRIPT);

    let spa = web.response("GET", "/documents/welcome").await;
    assert_eq!(spa.status(), StatusCode::OK);
    assert_eq!(response_text(spa).await, INDEX);

    let head = web.response("HEAD", "/assets/app.js").await;
    assert_eq!(head.status(), StatusCode::OK);
    assert_eq!(
        head.headers()[header::CONTENT_LENGTH],
        JAVASCRIPT.len().to_string()
    );
    assert!(response_text(head).await.is_empty());
}

#[tokio::test]
async fn every_unknown_api_namespace_path_stays_json_and_never_uses_the_spa_fallback() {
    let web = ServerWebFixture::new();

    for path in ["/api", "/api/unknown", "/api/v1/unknown"] {
        let response = web.response("GET", path).await;
        assert_ne!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "application/json",
            "{path}"
        );
        let body = response_text(response).await;
        assert!(!body.contains("QingYu Web"), "{path}");
        assert!(body.contains("\"requestId\""), "{path}");
    }
}

#[tokio::test]
async fn static_requests_keep_the_exact_host_and_origin_policy() {
    let web = ServerWebFixture::new();

    let wrong_host = Request::builder()
        .method("GET")
        .uri("/")
        .header(header::HOST, "attacker.example.test")
        .body(Body::empty())
        .unwrap();
    let wrong_host = web.router.clone().oneshot(wrong_host).await.unwrap();
    assert_eq!(wrong_host.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        wrong_host.headers()[header::CONTENT_TYPE],
        "application/json"
    );

    let wrong_origin = Request::builder()
        .method("GET")
        .uri("/")
        .header(header::HOST, HOST)
        .header(header::ORIGIN, "https://attacker.example.test")
        .body(Body::empty())
        .unwrap();
    let wrong_origin = web.router.clone().oneshot(wrong_origin).await.unwrap();
    assert_eq!(wrong_origin.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        wrong_origin.headers()[header::CONTENT_TYPE],
        "application/json"
    );
}

#[tokio::test]
async fn static_routes_reject_non_get_head_methods_and_apply_browser_security_headers() {
    let web = ServerWebFixture::new();

    for method in ["POST", "PUT", "PATCH", "DELETE"] {
        let response = web.response(method, "/").await;
        assert_eq!(
            response.status(),
            StatusCode::METHOD_NOT_ALLOWED,
            "{method}"
        );
        assert_ne!(response_text(response).await, INDEX, "{method}");
    }

    let response = web.response("GET", "/").await;
    assert_eq!(
        response.headers()[header::CONTENT_SECURITY_POLICY],
        "default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob: https:; font-src 'self' data:; object-src 'none'; base-uri 'none'; frame-ancestors 'none'"
    );
    assert_eq!(
        response.headers()[header::X_CONTENT_TYPE_OPTIONS],
        "nosniff"
    );
    assert_eq!(response.headers()[header::X_FRAME_OPTIONS], "DENY");
    assert_eq!(response.headers()[header::REFERRER_POLICY], "no-referrer");
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
}

#[tokio::test]
async fn traversal_hidden_and_directory_requests_never_expose_file_bytes_or_listings() {
    let web = ServerWebFixture::new();
    let outside = web._root.path().join("outside-secret.txt");
    std::fs::write(&outside, "outside secret").unwrap();

    for path in [
        "/../outside-secret.txt",
        "/%2e%2e/outside-secret.txt",
        "/..%2Foutside-secret.txt",
        "/.hidden",
    ] {
        let response = web.response("GET", path).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        let body = response_text(response).await;
        assert!(!body.contains("secret"), "{path}");
        assert!(!body.contains("QingYu Web"), "{path}");
    }

    let directory = web.response("GET", "/assets/").await;
    assert_eq!(directory.status(), StatusCode::OK);
    let body = response_text(directory).await;
    assert_eq!(body, INDEX);
    assert!(!body.contains("app.js"));
}

#[cfg(unix)]
#[tokio::test]
async fn symlinked_assets_are_not_followed() {
    use std::os::unix::fs::symlink;

    let web = ServerWebFixture::new();
    let outside = web._root.path().join("outside-secret.js");
    std::fs::write(&outside, "outside secret").unwrap();
    symlink(&outside, web._root.path().join("web/assets/alias.js")).unwrap();

    let response = web.response("GET", "/assets/alias.js").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(!response_text(response).await.contains("outside secret"));
}

#[tokio::test]
async fn web_asset_constructor_fails_closed_for_missing_or_non_regular_root_and_index() {
    for case in [
        "missing-root",
        "root-file",
        "missing-index",
        "index-directory",
    ] {
        let root = tempdir().unwrap();
        let web = root.path().join("web");
        match case {
            "missing-root" => {}
            "root-file" => std::fs::write(&web, "not a directory").unwrap(),
            "missing-index" => std::fs::create_dir(&web).unwrap(),
            "index-directory" => {
                std::fs::create_dir_all(web.join("index.html")).unwrap();
            }
            _ => unreachable!(),
        }
        let (activation, _runtime) = server_activation(root.path());
        let result = build_server_web_router(
            activation,
            TransportPolicy::same_origin(HOST, ORIGIN).unwrap(),
            &web,
        );
        assert!(result.is_err(), "{case}");
    }
}

#[cfg(unix)]
#[tokio::test]
async fn web_asset_constructor_rejects_symlinked_root_and_index() {
    use std::os::unix::fs::symlink;

    let root_case = tempdir().unwrap();
    let actual = root_case.path().join("actual");
    std::fs::create_dir(&actual).unwrap();
    std::fs::write(actual.join("index.html"), INDEX).unwrap();
    let linked = root_case.path().join("web");
    symlink(&actual, &linked).unwrap();
    let (activation, _runtime) = server_activation(root_case.path());
    assert!(build_server_web_router(
        activation,
        TransportPolicy::same_origin(HOST, ORIGIN).unwrap(),
        &linked,
    )
    .is_err());

    let index_case = tempdir().unwrap();
    let web = index_case.path().join("web");
    std::fs::create_dir(&web).unwrap();
    let actual_index = index_case.path().join("actual-index.html");
    std::fs::write(&actual_index, INDEX).unwrap();
    symlink(&actual_index, web.join("index.html")).unwrap();
    let (activation, _runtime) = server_activation(index_case.path());
    assert!(build_server_web_router(
        activation,
        TransportPolicy::same_origin(HOST, ORIGIN).unwrap(),
        &web,
    )
    .is_err());
}

#[tokio::test]
async fn native_router_keeps_the_api_only_json_fallback() {
    let root = tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let data = root.path().join("data");
    let cache = root.path().join("cache");
    for path in [&workspace, &data, &cache] {
        std::fs::create_dir(path).unwrap();
    }
    let paths = KernelPaths::desktop(&workspace, &data, &cache).unwrap();
    let config = KernelConfig::generate().unwrap();
    let credential = config.native_launch_credential().expose_secret().to_owned();
    let runtime = KernelRuntime::activate(config, paths, KernelPorts::unavailable()).unwrap();
    let router = build_router(
        runtime,
        TransportPolicy::loopback("127.0.0.1:43123", "tauri://localhost").unwrap(),
    );
    let request = Request::builder()
        .method("GET")
        .uri("/api/v1/not-a-route")
        .header(header::HOST, "127.0.0.1:43123")
        .header(header::ORIGIN, "tauri://localhost")
        .header(header::AUTHORIZATION, format!("Bearer {credential}"))
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    assert!(!response_text(response).await.contains("QingYu Web"));
}

#[tokio::test]
async fn server_api_only_router_does_not_serve_the_web_entrypoint() {
    let root = tempdir().unwrap();
    let (activation, _runtime) = server_activation(root.path());
    let router = build_server_router(
        activation,
        TransportPolicy::same_origin(HOST, ORIGIN).unwrap(),
    );

    let response = router.oneshot(request("GET", "/")).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    assert!(!response_text(response).await.contains("QingYu Web"));
}

fn server_activation(root: &Path) -> (ServerApiActivation, Arc<KernelRuntime>) {
    let data = root.join("data");
    let cache = root.join("cache");
    std::fs::create_dir(&data).unwrap();
    std::fs::create_dir(&cache).unwrap();
    let paths = ServerPathLayout::for_test(&data, &cache)
        .activate()
        .unwrap();
    let authentication = Arc::new(ServerAuthenticationStore::open(paths.config_root()).unwrap());
    let rate = RateLimitPolicy::new(2, Duration::from_secs(60), Duration::from_secs(30)).unwrap();
    let security = ServerAuthenticationSecurity::claim(
        authentication,
        AuthenticationRateLimiter::new(rate, rate),
        SessionStore::new(SessionPolicy::new(Duration::from_secs(300)).unwrap()),
    )
    .unwrap();
    let environment = ServerLaunchEnvironment::from_lookup(|name| {
        (name == crate::server::SERVER_INITIALIZATION_TOKEN_ENV)
            .then(|| OsString::from(INITIALIZATION_TOKEN))
    })
    .unwrap();
    let runtime = KernelRuntime::activate(
        KernelConfig::generate().unwrap(),
        paths,
        KernelPorts::unavailable(),
    )
    .unwrap();
    let activation = ServerApiProcess::new(runtime.clone(), 2)
        .unwrap()
        .activate(security, environment)
        .unwrap();
    (activation, runtime)
}

fn request(method: &str, path: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, HOST)
        .body(Body::empty())
        .unwrap()
}

async fn response_text(response: Response) -> String {
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}
