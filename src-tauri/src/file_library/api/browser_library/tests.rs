use super::*;
use axum::body::{to_bytes, Body};
use tower::ServiceExt;

const PASSWORD: &str = "synthetic-docker-web-password";
const TOKEN: &str = "synthetic-api-token-for-browser-tests-12345";

fn fixture() -> (tempfile::TempDir, Arc<Service>, Router) {
    let dir = tempfile::tempdir().unwrap();
    let public = dir.path().join("web");
    fs::create_dir(&public).unwrap();
    fs::write(public.join("index.html"), "<html><head></head></html>").unwrap();
    fs::write(public.join("login.html"), "login").unwrap();
    let web = web::Web::open(public, PASSWORD, false).unwrap();
    let root = dir.path().join("notes");
    let state = Service::open(root, Some(TOKEN), None, Some(web)).unwrap();
    let app = super::super::router(state.clone());
    (dir, state, app)
}

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    cookie: &str,
    body: serde_json::Value,
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let request = axum::http::Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, "notes.test")
        .header(header::ORIGIN, "http://notes.test")
        .header(header::COOKIE, cookie)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    (
        status,
        headers,
        to_bytes(response.into_body(), MAX_BODY * 2)
            .await
            .unwrap()
            .to_vec(),
    )
}

async fn login(app: &Router) -> String {
    let (status, headers, _) =
        call(app, "POST", "/auth/login", "", json!({"password":PASSWORD})).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    headers[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .into()
}

async fn json_call(app: &Router, cookie: &str, body: serde_json::Value) -> serde_json::Value {
    let (status, _, bytes) = call(app, "POST", "/api/web/commands", cookie, body).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    serde_json::from_slice(&bytes).unwrap()
}

async fn load(app: &Router, cookie: &str) -> serde_json::Value {
    let (status, _, bytes) = call(app, "GET", "/api/web/library", cookie, json!(null)).await;
    assert_eq!(status, StatusCode::OK);
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn browser_routes_require_sessions_and_keep_public_token_scope_and_csrf_checks() {
    let (_dir, _state, app) = fixture();
    for path in [
        "/api/web/library",
        "/api/web/commands",
        "/api/web/assets/id?path=image.svg",
    ] {
        assert_eq!(
            call(
                &app,
                if path.ends_with("commands") {
                    "POST"
                } else {
                    "GET"
                },
                path,
                "",
                json!({})
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
    }
    let cookie = login(&app).await;
    let request = axum::http::Request::builder()
        .uri("/api/web/library")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header(header::COOKIE, &cookie)
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.clone().oneshot(request).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/api/web/commands")
        .header(header::HOST, "notes.test")
        .header(header::ORIGIN, "http://other.test")
        .header(header::COOKIE, &cookie)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{}"))
        .unwrap();
    assert_eq!(
        app.oneshot(request).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn editor_operations_use_real_files_and_reject_stale_note_and_folder_versions() {
    let (dir, state, app) = fixture();
    let cookie = login(&app).await;
    let initial = load(&app, &cookie).await;
    assert!(
        initial.get("root").is_none()
            && initial.get("sync").is_none()
            && initial.get("activeId").is_none()
    );
    let folders = json_call(
        &app,
        &cookie,
        json!({"kind":"createFolder","parentId":"default","name":"项目"}),
    )
    .await;
    let folder = folders["folders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "项目")
        .unwrap()["id"]
        .as_str()
        .unwrap();
    let id = Uuid::new_v4().to_string();
    let created = json_call(
        &app,
        &cookie,
        json!({"kind":"createNote","id":id,"folderId":folder,"title":"草稿","source":"# 原文"}),
    )
    .await;
    assert_eq!(
        fs::read_to_string(dir.path().join("notes/项目/草稿.md")).unwrap(),
        "# 原文"
    );
    let selected = state.store.lock().unwrap().data.active_id.clone();
    let body = json!({"kind":"updateNote","id":id,"version":created["versions"][&id],"source":"# 新内容","title":"草稿","pinned":true,"trashed":false,"folderId":folder});
    let (a, b) = tokio::join!(
        call(&app, "POST", "/api/web/commands", &cookie, body.clone()),
        call(&app, "POST", "/api/web/commands", &cookie, body)
    );
    assert!(matches!(
        (a.0, b.0),
        (StatusCode::OK, StatusCode::PRECONDITION_FAILED)
            | (StatusCode::PRECONDITION_FAILED, StatusCode::OK)
    ));
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/web/commands",
            &cookie,
            json!({"kind":"renameFolder","id":folder,"version":folders["version"],"name":"已改名"})
        )
        .await
        .0,
        StatusCode::PRECONDITION_FAILED
    );
    let current = load(&app, &cookie).await;
    json_call(
        &app,
        &cookie,
        json!({"kind":"renameFolder","id":folder,"version":current["version"],"name":"已改名"}),
    )
    .await;
    let current = load(&app, &cookie).await;
    json_call(&app,&cookie,json!({"kind":"duplicateNote","id":id,"version":current["versions"][&id],"newId":Uuid::new_v4().to_string()})).await;
    assert!(dir.path().join("notes/已改名/草稿 Copy.md").exists());
    let current = load(&app, &cookie).await;
    json_call(&app,&cookie,json!({"kind":"updateNote","id":id,"version":current["versions"][&id],"source":"# 新内容","title":"草稿","pinned":true,"trashed":true,"folderId":folder})).await;
    let current = load(&app, &cookie).await;
    assert!(current["notes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == id)
        .unwrap()["trashed"]
        .as_bool()
        .unwrap());
    json_call(&app,&cookie,json!({"kind":"updateNote","id":id,"version":current["versions"][&id],"source":"# 新内容","title":"草稿","pinned":true,"trashed":false,"folderId":"default"})).await;
    assert_eq!(state.store.lock().unwrap().data.active_id, selected);
    let folders = json_call(
        &app,
        &cookie,
        json!({"kind":"createFolder","parentId":"default","name":"归档"}),
    )
    .await;
    let destination = folders["folders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "归档")
        .unwrap()["id"]
        .as_str()
        .unwrap();
    json_call(&app, &cookie, json!({"kind":"moveFolder","id":folder,"version":folders["version"],"destinationId":destination})).await;
    assert!(dir.path().join("notes/归档/已改名/草稿 Copy.md").exists());
    let reopened = DiskLibrary::open(dir.path().join("notes"), None).unwrap();
    let note = reopened.data.notes.iter().find(|n| n.id == id).unwrap();
    assert_eq!(note.source, "# 新内容");
    assert!(note.pinned && !note.trashed);
    fs::write(dir.path().join("notes").join(&note.path), "# 外部修改").unwrap();
    let previous = load(&app, &cookie).await;
    fs::write(dir.path().join("notes").join(&note.path), "# 再次修改").unwrap();
    assert_eq!(call(&app,"POST","/api/web/commands",&cookie,json!({"kind":"updateNote","id":id,"version":previous["versions"][&id],"source":"旧客户端","title":"草稿","pinned":true,"trashed":false,"folderId":"default"})).await.0,StatusCode::PRECONDITION_FAILED);
    assert_eq!(call(&app,"POST","/api/web/commands",&cookie,json!({"kind":"createNote","id":Uuid::new_v4().to_string(),"folderId":"default","title":"错误","source":"x","path":"/outside"})).await.0,StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn attachment_paths_are_relative_to_notes_and_cannot_expose_metadata_or_symlinks() {
    let (dir, _state, app) = fixture();
    let cookie = login(&app).await;
    fs::create_dir_all(dir.path().join("notes/项目")).unwrap();
    fs::create_dir_all(dir.path().join("notes/images")).unwrap();
    fs::write(
        dir.path().join("notes/项目/笔记.md"),
        "![图](../images/test.svg)",
    )
    .unwrap();
    fs::write(
        dir.path().join("notes/images/test.svg"),
        "<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>",
    )
    .unwrap();
    fs::write(dir.path().join("notes/.env"), "synthetic-secret").unwrap();
    fs::write(dir.path().join("outside.txt"), "outside").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        dir.path().join("outside.txt"),
        dir.path().join("notes/images/escape.bin"),
    )
    .unwrap();
    let snapshot = load(&app, &cookie).await;
    let id = snapshot["notes"][0]["id"].as_str().unwrap();
    let (status, headers, bytes) = call(
        &app,
        "GET",
        &format!("/api/web/assets/{id}?path=../images/test.svg"),
        &cookie,
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(bytes.starts_with(b"<svg"));
    assert!(headers
        .get_all(header::CONTENT_SECURITY_POLICY)
        .iter()
        .any(|v| v.to_str().unwrap().contains("sandbox")));
    for path in [
        "../../outside.txt",
        "../.miaoyan/library.json",
        "笔记.md",
        "../images/escape.bin",
        "/etc/passwd",
        "../s3-connection.json",
        "../.env",
    ] {
        let query = path.replace('/', "%2F");
        assert_eq!(
            call(
                &app,
                "GET",
                &format!("/api/web/assets/{id}?path={query}"),
                &cookie,
                json!(null)
            )
            .await
            .0,
            StatusCode::NOT_FOUND,
            "{path}"
        );
    }
}

#[tokio::test]
async fn attachment_upload_uses_note_version_and_preserves_named_binary_resources() {
    let (dir, _state, app) = fixture();
    let cookie = login(&app).await;
    fs::write(dir.path().join("notes/test.md"), "# attachments").unwrap();
    let snapshot = load(&app, &cookie).await;
    let id = snapshot["notes"][0]["id"].as_str().unwrap();
    let version = snapshot["versions"][id].as_str().unwrap();
    let endpoint = format!("/api/web/assets/{id}");
    let body = json!({"version":version,"upload":{"name":"说明.txt","bytes":[0,255,128]}});
    assert_eq!(call(&app, "POST", &endpoint, "", body.clone()).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(call(&app, "POST", &endpoint, &cookie, json!({"version":"stale","upload":{"name":"a.pdf","bytes":[]}})).await.0, StatusCode::PRECONDITION_FAILED);
    let (status, _, bytes) = call(&app, "POST", &endpoint, &cookie, body).await;
    assert_eq!(status, StatusCode::OK, "{}",String::from_utf8_lossy(&bytes));
    let receipt: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(receipt["name"], "说明.txt"); assert_eq!(receipt["image"], false);
    let path = receipt["path"].as_str().unwrap();
    assert!(path.starts_with(".attachments/"));
    let query: String = path.as_bytes().iter().map(|byte| format!("%{byte:02X}")).collect();
    let (status, headers, bytes) = call(&app, "GET", &format!("{endpoint}?path={query}"), &cookie, json!(null)).await;
    assert_eq!(status, StatusCode::OK); assert_eq!(bytes, [0,255,128]);
    assert!(headers[header::CONTENT_DISPOSITION].to_str().unwrap().contains("filename*=UTF-8''"));
    let after = load(&app, &cookie).await;
    assert_eq!(after["notes"].as_array().unwrap().len(), 1); // A .txt attachment must not become another note.
    let large = json!({"version":version,"upload":{"name":"large.bin","bytes":vec![255u8; MAX_BODY / 4 + 1]}});
    assert_eq!(call(&app, "POST", &endpoint, &cookie, large).await.0, StatusCode::OK);
}
