use super::*;
use axum::body::{to_bytes, Body};
use tower::ServiceExt;

const PASSWORD: &str = "synthetic-browser-password-12345";
const TOKEN: &str = "synthetic-write-token-for-web-tests-12345";
const READ_TOKEN: &str = "synthetic-read-token-for-web-tests-12345";
const ORIGIN: &str = "https://notes.test";

fn fixture() -> (tempfile::TempDir, Arc<Service>, Router) {
    let dir = tempfile::tempdir().unwrap();
    let public = dir.path().join("web");
    fs::create_dir_all(public.join("assets")).unwrap();
    fs::write(
        public.join("index.html"),
        "<html><head></head><body>private editor</body></html>",
    )
    .unwrap();
    for name in ["settings.html", "preview.html", "auth/private.html"] {
        let path = public.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "private page").unwrap();
    }
    fs::write(public.join("login.html"), "public login").unwrap();
    fs::write(public.join("assets/login-test.js"), "public script").unwrap();
    let web = Web::open(public, PASSWORD, true).unwrap();
    let state = Service::open(
        dir.path().join("notes"),
        Some(TOKEN),
        Some(READ_TOKEN),
        Some(web),
    )
    .unwrap();
    (dir, state.clone(), super::super::router(state))
}

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    cookie: Option<&str>,
    origin: Option<&str>,
    body: serde_json::Value,
    bearer: Option<&str>,
) -> (StatusCode, HeaderMap, String) {
    let mut request = axum::http::Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, "notes.test")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    if let Some(origin) = origin {
        request = request.header(header::ORIGIN, origin);
    }
    if let Some(bearer) = bearer {
        request = request.header(header::AUTHORIZATION, format!("Bearer {bearer}"));
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), MAX_BODY * 2).await.unwrap();
    (status, headers, String::from_utf8(bytes.to_vec()).unwrap())
}

async fn sign_in(app: &Router) -> String {
    let (status, headers, _) = call(
        app,
        "POST",
        "/auth/login",
        None,
        Some(ORIGIN),
        json!({"password": PASSWORD}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let cookie = headers[header::SET_COOKIE].to_str().unwrap();
    for flag in [
        "HttpOnly",
        "SameSite=Strict",
        "Secure",
        "Path=/",
        "Max-Age=43200",
    ] {
        assert!(cookie.contains(flag));
    }
    assert!(cookie.starts_with("__Host-qingyu_session="));
    assert!(!cookie.contains(PASSWORD) && !cookie.contains(TOKEN));
    cookie.split(';').next().unwrap().into()
}

#[tokio::test]
async fn pages_require_a_session_and_encoded_assets_cannot_bypass_the_gate() {
    let (_dir, _state, app) = fixture();
    for path in [
        "/",
        "/index.html",
        "/settings.html",
        "/preview.html",
        "/auth/private.html",
        "/assets/../index.html",
        "/assets/%2e%2e/index.html",
        "/assets/%2e%2e%2fsettings.html",
        "/%69ndex.html",
    ] {
        let (status, headers, body) = call(&app, "GET", path, None, None, json!(null), None).await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{path}");
        assert_eq!(headers[header::LOCATION], "/login.html");
        assert_eq!(headers[header::CACHE_CONTROL], "no-store");
        assert!(!body.contains("private"));
    }
    for path in ["/api/v1/notes", "/api/not-real", "/auth/session"] {
        assert_eq!(
            call(&app, "GET", path, None, None, json!(null), None)
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
    }
    for path in ["/login.html", "/assets/login-test.js"] {
        let (status, headers, _) = call(&app, "GET", path, None, None, json!(null), None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
        assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    }
    let cookie = sign_in(&app).await;
    let (status, _, body) = call(&app, "GET", "/", Some(&cookie), None, json!(null), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("qingyu-web-auth") && body.contains("private editor"));
    assert_eq!(
        call(
            &app,
            "HEAD",
            "/settings.html",
            None,
            None,
            json!(null),
            None
        )
        .await
        .0,
        StatusCode::SEE_OTHER
    );
}

#[tokio::test]
async fn password_login_rejects_csrf_invalid_payloads_and_wrong_passwords() {
    let (_dir, _state, app) = fixture();
    for origin in [
        None,
        Some("https://evil.test"),
        Some("http://notes.test"),
        Some("null"),
    ] {
        let (status, headers, _) = call(
            &app,
            "POST",
            "/auth/login",
            None,
            origin,
            json!({"password": PASSWORD}),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(!headers.contains_key(header::SET_COOKIE));
    }
    let (status, headers, _) = call(
        &app,
        "POST",
        "/auth/login",
        None,
        Some(ORIGIN),
        json!({"password": "wrong"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(!headers.contains_key(header::SET_COOKIE));
    for body in [
        json!({}),
        json!({"password": PASSWORD, "username": "unneeded"}),
    ] {
        assert_eq!(
            call(&app, "POST", "/auth/login", None, Some(ORIGIN), body, None)
                .await
                .0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    assert_eq!(
        call(
            &app,
            "POST",
            "/auth/login",
            None,
            Some(ORIGIN),
            json!({"password": "x".repeat(5000)}),
            None
        )
        .await
        .0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let cookie = sign_in(&app).await;
    assert_eq!(
        call(
            &app,
            "GET",
            "/auth/session",
            Some(&cookie),
            None,
            json!(null),
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let duplicate = format!("{cookie}; {cookie}");
    assert_eq!(
        call(
            &app,
            "GET",
            "/auth/session",
            Some(&duplicate),
            None,
            json!(null),
            None
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn cookie_api_writes_need_origin_and_bearer_permissions_are_preserved() {
    let (_dir, _state, app) = fixture();
    let cookie = sign_in(&app).await;
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/notes",
            Some(&cookie),
            None,
            json!(null),
            None
        )
        .await
        .0,
        StatusCode::OK
    );
    for origin in [None, Some("https://evil.test")] {
        assert_eq!(
            call(
                &app,
                "POST",
                "/api/v1/notes",
                Some(&cookie),
                origin,
                json!({}),
                None
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/notes",
            Some(&cookie),
            None,
            json!(null),
            Some("wrong")
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/notes",
            Some(&cookie),
            Some(ORIGIN),
            json!({}),
            Some(READ_TOKEN)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (_, _, folders) = call(
        &app,
        "GET",
        "/api/v1/folders",
        Some(&cookie),
        None,
        json!(null),
        None,
    )
    .await;
    let folder =
        serde_json::from_str::<serde_json::Value>(&folders).unwrap()["items"][0]["id"].clone();
    let body = json!({"folderId": folder, "source": "# cookie-authenticated note", "title": "Web"});
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/notes",
            Some(&cookie),
            Some(ORIGIN),
            body,
            None
        )
        .await
        .0,
        StatusCode::CREATED
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/notes",
            None,
            None,
            json!(null),
            Some(TOKEN)
        )
        .await
        .0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn logout_revokes_the_cookie_and_expiration_and_restart_require_login() {
    let (_dir, state, app) = fixture();
    let cookie = sign_in(&app).await;
    assert_eq!(
        call(
            &app,
            "POST",
            "/auth/logout",
            Some(&cookie),
            Some("https://evil.test"),
            json!(null),
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/auth/session",
            Some(&cookie),
            None,
            json!(null),
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let (status, headers, _) = call(
        &app,
        "POST",
        "/auth/logout",
        Some(&cookie),
        Some(ORIGIN),
        json!(null),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(headers[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .contains("Max-Age=0"));
    assert_eq!(
        call(
            &app,
            "GET",
            "/auth/session",
            Some(&cookie),
            None,
            json!(null),
            None
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let new_cookie = sign_in(&app).await;
    assert_ne!(cookie, new_cookie);
    let web = state.web.as_ref().unwrap();
    let restarted = Web::open(web.directory.clone(), PASSWORD, true).unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(header::COOKIE, new_cookie.parse().unwrap());
    assert!(!restarted.authenticated(&headers));
    for expiry in web.sessions.lock().unwrap().active.values_mut() {
        *expiry = Instant::now() - Duration::from_secs(1);
    }
    assert_eq!(
        call(
            &app,
            "GET",
            "/auth/session",
            Some(&new_cookie),
            None,
            json!(null),
            None
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&app, "GET", "/", Some(&new_cookie), None, json!(null), None)
            .await
            .0,
        StatusCode::SEE_OTHER
    );
}

#[tokio::test]
async fn failed_logins_are_bounded_and_the_cooldown_expires() {
    let (_dir, state, app) = fixture();
    for _ in 0..MAX_FAILURES {
        assert_eq!(
            call(
                &app,
                "POST",
                "/auth/login",
                None,
                Some(ORIGIN),
                json!({"password": "wrong"}),
                None
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
    }
    let (status, headers, _) = call(
        &app,
        "POST",
        "/auth/login",
        None,
        Some(ORIGIN),
        json!({"password": PASSWORD}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(headers[header::RETRY_AFTER], "60");
    for time in &mut state
        .web
        .as_ref()
        .unwrap()
        .sessions
        .lock()
        .unwrap()
        .failures
    {
        *time = Instant::now() - LOGIN_WINDOW;
    }
    sign_in(&app).await;
}

#[tokio::test]
async fn web_only_needs_a_password_and_invalid_configuration_does_not_create_a_library() {
    let (dir, state, _app) = fixture();
    let public = state.web.as_ref().unwrap().directory.clone();
    let root = dir.path().join("new-notes");
    assert!(Service::open(root.clone(), None, None, None).is_err());
    assert!(!root.exists());
    for password in ["", "short", "            "] {
        assert!(Web::open(public.clone(), password, false).is_err());
    }
    assert!(Web::open(dir.path().into(), PASSWORD, true).is_err());
    let web = Web::open(public, PASSWORD, false).unwrap();
    assert!(!web.set_cookie("test", 1).contains("Secure"));
    let app = super::super::router(Service::open(root, None, None, Some(web)).unwrap());
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/notes",
            None,
            None,
            json!(null),
            Some(TOKEN)
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/auth/login",
            None,
            Some("http://notes.test"),
            json!({"password": PASSWORD}),
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
}
