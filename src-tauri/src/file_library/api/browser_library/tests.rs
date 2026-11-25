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
async fn browser_title_edit_checks_the_library_version_before_renaming_and_rewriting_links() {
    let (dir, _, app) = fixture();
    let cookie = login(&app).await;
    let note = Uuid::new_v4().to_string();
    let link = Uuid::new_v4().to_string();
    let first = json_call(&app, &cookie, json!({
        "kind":"createNote","id":note,"folderId":"default","title":"Old","source":"# Old"
    })).await;
    let second = json_call(&app, &cookie, json!({
        "kind":"createNote","id":link,"folderId":"default","title":"Links","source":"[[Old]]"
    })).await;
    let edit = |version: &serde_json::Value| json!({
        "kind":"editTitle","id":note,"version":version,"title":"New","followTitle":"New",
        "edits":[{"id":note,"source":"# New"},{"id":link,"source":"[[New]]"}]
    });
    let rejected = call(&app, "POST", "/api/web/commands", &cookie, edit(&first["version"])).await;
    assert_eq!(rejected.0, StatusCode::PRECONDITION_FAILED);
    let root = dir.path().join("notes");
    assert_eq!(fs::read_to_string(root.join("Old.md")).unwrap(), "# Old");
    assert_eq!(fs::read_to_string(root.join("Links.md")).unwrap(), "[[Old]]");
    json_call(&app, &cookie, edit(&second["version"])).await;
    assert!(!root.join("Old.md").exists());
    assert_eq!(fs::read_to_string(root.join("New.md")).unwrap(), "# New");
    assert_eq!(fs::read_to_string(root.join("Links.md")).unwrap(), "[[New]]");
}

#[tokio::test]
async fn browser_history_restores_the_saved_source_and_rejects_stale_writes() {
    let (dir, _, app) = fixture();
    let cookie = login(&app).await;
    let id = Uuid::new_v4().to_string();
    let created = json_call(
        &app,
        &cookie,
        json!({"kind":"createNote","id":id,"folderId":"default","title":"历史","source":"# 原文"}),
    )
    .await;
    let updated = json_call(
        &app,
        &cookie,
        json!({
            "kind":"updateNote","id":id,"version":created["versions"][&id],"source":"# 修改后",
            "title":"历史","pinned":true,"trashed":false,"folderId":"default"
        }),
    )
    .await;
    let path = format!("/api/web/notes/{id}/versions");
    assert_eq!(
        call(&app, "GET", &path, "", json!(null)).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (status, _, bytes) = call(&app, "GET", &path, &cookie, json!(null)).await;
    assert_eq!(status, StatusCode::OK);
    let entries: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let revision = entries[0]["id"].as_str().unwrap();
    let (status, _, bytes) = call(
        &app,
        "GET",
        &format!("{path}/{revision}"),
        &cookie,
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(serde_json::from_slice::<String>(&bytes).unwrap(), "# 原文");

    let restore = |version: &serde_json::Value| json!({"kind":"restoreNoteVersion","id":id,"version":version,"revision":revision});
    let stale = call(
        &app,
        "POST",
        "/api/web/commands",
        &cookie,
        restore(&created["versions"][&id]),
    )
    .await;
    assert_eq!(stale.0, StatusCode::PRECONDITION_FAILED);
    assert_eq!(
        fs::read_to_string(dir.path().join("notes/历史.md")).unwrap(),
        "# 修改后"
    );
    let restored = json_call(&app, &cookie, restore(&updated["versions"][&id])).await;
    assert_eq!(restored["notes"][0]["source"], "# 原文");
    assert_eq!(restored["notes"][0]["pinned"], true);
    assert_eq!(
        fs::read_to_string(dir.path().join("notes/历史.md")).unwrap(),
        "# 原文"
    );
    let (_, _, bytes) = call(&app, "GET", &path, &cookie, json!(null)).await;
    let entries: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(entries.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn storage_usage_reports_counts_from_the_mounted_library_to_the_editor_session() {
    let (dir, _, app) = fixture();
    let root = dir.path().join("notes");
    fs::write(root.join("测试.md"), "正文").unwrap();
    fs::write(root.join("图片.png"), [0; 21]).unwrap();
    let path = "/api/web/storage-usage";
    assert_eq!(
        call(&app, "GET", path, "", json!(null)).await.0,
        StatusCode::UNAUTHORIZED
    );
    let cookie = login(&app).await;
    let (status, headers, body) = call(&app, "GET", path, &cookie, json!(null)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
        json!({"notes":6,"files":21,"trash":0})
    );
}

#[tokio::test]
async fn link_preview_requires_browser_session_and_rejects_private_targets() {
    let (_dir, _state, app) = fixture();
    let path = "/api/web/link-preview";
    let target = json!({"target":"http://127.0.0.1/","image":false});
    assert_eq!(
        call(&app, "POST", path, "", target.clone()).await.0,
        StatusCode::UNAUTHORIZED
    );
    let cookie = login(&app).await;
    let request = axum::http::Request::builder()
        .method("POST")
        .uri(path)
        .header(header::HOST, "notes.test")
        .header(header::ORIGIN, "http://unrelated.test")
        .header(header::COOKIE, &cookie)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(target.to_string()))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(request).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    let (status, headers, body) = call(&app, "POST", path, &cookie, target).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
        json!(null)
    );
}

#[tokio::test]
async fn browser_routes_require_sessions_and_keep_public_token_scope_and_csrf_checks() {
    let (_dir, _state, app) = fixture();
    for path in [
        "/api/web/library",
        "/api/web/commands",
        "/api/web/assets/id?path=image.svg",
        "/api/web/sync",
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
    let (status, _, bytes) = call(&app, "GET", "/api/web/sync", &cookie, json!(null)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
        json!({"busy":false,"error":false,"message":"","lastSyncedAt":null,"lastSyncedAccount":null,"unavailable":"同步不可用"})
    );
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
    for path in ["/api/web/commands", "/api/web/sync"] {
        let request = axum::http::Request::builder()
            .method("POST")
            .uri(path)
            .header(header::HOST, "notes.test")
            .header(header::ORIGIN, "http://other.test")
            .header(header::COOKIE, &cookie)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from("{}"))
            .unwrap();
        assert_eq!(
            app.clone().oneshot(request).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
    }
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
async fn update_note_keeps_share_state_and_rejects_non_web_links() {
    let (_dir, _state, app) = fixture();
    let cookie = login(&app).await;
    let id = Uuid::new_v4().to_string();
    let created = json_call(
        &app,
        &cookie,
        json!({"kind":"createNote","id":id,"folderId":"default","title":"分享","source":"# 分享"}),
    )
    .await;
    let update = |version: &serde_json::Value, share: serde_json::Value| json!({"kind":"updateNote","id":id,"version":version,"source":"# 分享","title":"分享","pinned":false,"trashed":false,"folderId":"default","share":share});
    let bad = call(
        &app,
        "POST",
        "/api/web/commands",
        &cookie,
        update(
            &created["versions"][&id],
            json!({"status":"shared","url":"javascript:alert(1)"}),
        ),
    )
    .await;
    assert_eq!(bad.0, StatusCode::BAD_REQUEST);
    let shared = json_call(
        &app,
        &cookie,
        update(
            &created["versions"][&id],
            json!({"status":"shared","url":"https://share.example.com/n/a"}),
        ),
    )
    .await;
    let note = |snapshot: &serde_json::Value| {
        snapshot["notes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["id"] == id.as_str())
            .unwrap()
            .clone()
    };
    assert_eq!(
        note(&shared)["share"],
        json!({"status":"shared","url":"https://share.example.com/n/a"})
    );
    assert_eq!(
        note(&load(&app, &cookie).await)["share"]["status"],
        "shared"
    );
    let stopped = json_call(
        &app,
        &cookie,
        update(&shared["versions"][&id], json!({"status":"unshared"})),
    )
    .await;
    assert_eq!(note(&stopped)["share"], json!({"status":"unshared"}));
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
    assert_eq!(
        call(&app, "POST", &endpoint, "", body.clone()).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &endpoint,
            &cookie,
            json!({"version":"stale","upload":{"name":"a.pdf","bytes":[]}})
        )
        .await
        .0,
        StatusCode::PRECONDITION_FAILED
    );
    let (status, _, bytes) = call(&app, "POST", &endpoint, &cookie, body).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    let receipt: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(receipt["name"], "说明.txt");
    assert_eq!(receipt["image"], false);
    let path = receipt["path"].as_str().unwrap();
    assert!(path.starts_with(".attachments/"));
    let query: String = path
        .as_bytes()
        .iter()
        .map(|byte| format!("%{byte:02X}"))
        .collect();
    let (status, headers, bytes) = call(
        &app,
        "GET",
        &format!("{endpoint}?path={query}"),
        &cookie,
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, [0, 255, 128]);
    assert!(headers[header::CONTENT_DISPOSITION]
        .to_str()
        .unwrap()
        .contains("filename*=UTF-8''"));
    let after = load(&app, &cookie).await;
    assert_eq!(after["notes"].as_array().unwrap().len(), 1); // A .txt attachment must not become another note.
    let large = json!({"version":version,"upload":{"name":"large.bin","bytes":vec![255u8; MAX_BODY / 4 + 1]}});
    assert_eq!(
        call(&app, "POST", &endpoint, &cookie, large).await.0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn account_routes_require_the_unlocked_browser_and_a_private_config_directory() {
    let dir = tempfile::tempdir().unwrap();
    let public = dir.path().join("web");
    fs::create_dir(&public).unwrap();
    fs::write(public.join("index.html"), "<html><head></head></html>").unwrap();
    fs::write(public.join("login.html"), "login").unwrap();
    let web = web::Web::open(public, PASSWORD, false).unwrap();
    let root = dir.path().join("notes");
    let mut state = Service::open(root.clone(), None, None, Some(web)).unwrap();
    let app = super::super::router(state.clone());
    let (status, _, _) = call(&app, "GET", "/api/web/account", "", json!(null)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let cookie = login(&app).await;
    let (status, _, body) = call(&app, "GET", "/api/web/account", &cookie, json!(null)).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&body).unwrap(), json!({"error":"config_required"}));

    assert!(super::super::config_dir(root.join("config"), &root).is_err());
    let config = super::super::config_dir(dir.path().join("config"), &root).unwrap();
    drop(app);
    Arc::get_mut(&mut state).unwrap().config = Some(config);
    let app = super::super::router(state.clone());
    let cookie = login(&app).await;
    let (status, _, body) = call(&app, "GET", "/api/web/account", &cookie, json!(null)).await;
    assert_eq!((status, body.as_slice()), (StatusCode::OK, b"null".as_slice()));
    let (status, headers, _) = call(&app, "GET", "/api/web/account/portal?page=purchase", &cookie, json!(null)).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "https://qynotes.com/purchase");
    let (status, _, _) = call(&app, "GET", "/api/web/account/portal?page=elsewhere", &cookie, json!(null)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _, body) = call(&app, "POST", "/api/web/account/login", &cookie, json!({"username":"bad","password":"x"})).await;
    assert_eq!((status, serde_json::from_slice::<serde_json::Value>(&body).unwrap()), (StatusCode::BAD_REQUEST, json!({"error":"invalid_input"})));
}

#[tokio::test]
async fn sync_settings_live_in_the_private_config_directory() {
    let dir = tempfile::tempdir().unwrap();
    let public = dir.path().join("web");
    fs::create_dir(&public).unwrap();
    fs::write(public.join("index.html"), "<html><head></head></html>").unwrap();
    fs::write(public.join("login.html"), "login").unwrap();
    let web = web::Web::open(public, PASSWORD, false).unwrap();
    let root = dir.path().join("notes");
    let mut state = Service::open(root.clone(), None, None, Some(web)).unwrap();
    let config = super::super::config_dir(dir.path().join("config"), &root).unwrap();
    Arc::get_mut(&mut state).unwrap().sync =
        Some(server_sync::ServerSync::open(Some(config.clone()), &root).unwrap());
    let app = super::super::router(state.clone());
    let cookie = login(&app).await;

    let (status, _, body) = call(&app, "GET", "/api/web/sync/profile", &cookie, json!(null)).await;
    assert_eq!((status, body.as_slice()), (StatusCode::OK, b"null".as_slice()));
    let profile = json!({"endpoint":"https://s3.example.test","bucket":"notes","region":"us-east-1","prefix":"qingyu",
        "accessKeyId":"key","secretAccessKey":"secret","mode":"s3","timeout":30});
    let (status, _, _) = call(&app, "PUT", "/api/web/sync/profile", &cookie, profile.clone()).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, _, body) = call(&app, "GET", "/api/web/sync/profile", &cookie, json!(null)).await;
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&body).unwrap(), profile);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(config.join("s3-connection.json")).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    let (_, _, body) = call(&app, "GET", "/api/web/sync/schedule", &cookie, json!(null)).await;
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&body).unwrap(), json!({"s3":true,"cloudAccount":"","interval":3}));
    let (status, _, _) = call(&app, "PUT", "/api/web/sync/schedule", &cookie, json!({"s3":false,"cloudAccount":"","interval":7})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _, _) = call(&app, "PUT", "/api/web/sync/schedule", &cookie, json!({"s3":false,"cloudAccount":"account","interval":10})).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, _, body) = call(&app, "GET", "/api/web/sync/schedule", &cookie, json!(null)).await;
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&body).unwrap()["interval"], 10);

    // 没有同步程序时说明原因，而不是悄悄失败
    let (status, _, body) = call(&app, "POST", "/api/web/sync", &cookie, json!({})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(String::from_utf8(body).unwrap().contains("QINGYU_SYNC_WORKER"));
}

#[tokio::test]
async fn browser_folder_actions_and_purge_match_the_mobile_library_operations() {
    let (_dir, _, app) = fixture();
    let cookie = login(&app).await;
    let folder_of = |library: &serde_json::Value, path: &str| {
        library["folders"].as_array().unwrap().iter()
            .find(|folder| folder["path"] == path).map(|folder| folder["id"].as_str().unwrap().to_owned())
    };
    let library = json_call(&app, &cookie, json!({"kind":"createFolder","parentId":"default","name":"项目"})).await;
    let project = folder_of(&library, "项目").unwrap();
    let library = json_call(&app, &cookie, json!({"kind":"createFolder","parentId":"default","name":"归档"})).await;
    let archive = folder_of(&library, "归档").unwrap();
    let note = Uuid::new_v4().to_string();
    let library = json_call(&app, &cookie, json!({"kind":"createNote","id":note,"folderId":project,"title":"草稿","source":"# 原文"})).await;

    // 复制文件夹
    let library = json_call(&app, &cookie, json!({"kind":"folderAction","action":"duplicate","id":project,"version":library["version"]})).await;
    let copy = folder_of(&library, "项目 副本").unwrap();
    // 移到指定文件夹
    let library = json_call(&app, &cookie, json!({"kind":"folderAction","action":"move-to-folder","id":copy,"version":library["version"],"value":archive})).await;
    assert!(folder_of(&library, "归档/项目 副本").is_some());
    // 移到笔记库废纸篓，再恢复
    let library = json_call(&app, &cookie, json!({"kind":"folderAction","action":"trash-local","id":project,"version":library["version"]})).await;
    assert_eq!(library["trashedFolders"], json!(["项目"]));
    assert!(folder_of(&library, "项目").is_none());
    let library = json_call(&app, &cookie, json!({"kind":"folderAction","action":"restore-folder","id":null,"version":library["version"],"value":"项目"})).await;
    assert_eq!(library["trashedFolders"], json!([]));
    assert!(folder_of(&library, "项目").is_some());
    // 只能永久删除废纸篓里的笔记，原因和 App 相同
    let (status, _, body) = call(&app, "POST", "/api/web/commands", &cookie, json!({"kind":"purgeNotes","ids":[note],"version":library["version"]})).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(String::from_utf8(body).unwrap().contains("只能永久删除仍在废纸篓里的笔记"));
    let (status, _, _) = call(&app, "POST", "/api/web/commands", &cookie, json!({"kind":"folderAction","action":"reveal","id":project,"version":library["version"]})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn browser_import_copies_markdown_files_into_the_library_like_the_desktop_import() {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let (_dir, _, app) = fixture();
    let cookie = login(&app).await;
    let file = |path: &str, text: &str| json!({"path": path, "data": STANDARD.encode(text)});
    let (status, _, body) = call(&app, "POST", "/api/web/import", &cookie,
        json!([file("清单.md", "# 清单"), file("旅行/京都.md", "# 京都"), file("旅行/封面.png", "png")])).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let library: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let sources: Vec<_> = library["notes"].as_array().unwrap().iter().map(|note| note["source"].as_str().unwrap()).collect();
    assert!(sources.contains(&"# 清单") && sources.contains(&"# 京都"));
    assert_eq!(sources.len(), 2);
    for path in ["../逃逸.md", "旅行/../../逃逸.md", ".隐藏.md", "a\\b.md"] {
        let (status, _, _) = call(&app, "POST", "/api/web/import", &cookie, json!([file(path, "x")])).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{path}");
    }
    let (status, _, body) = call(&app, "POST", "/api/web/import", &cookie, json!([file("图片.png", "png")])).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(String::from_utf8(body).unwrap().contains("没有可导入的 Markdown 文件"));
}
