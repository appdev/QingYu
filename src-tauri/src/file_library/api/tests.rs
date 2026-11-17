use super::*;
use axum::body::{to_bytes, Body};
use tower::ServiceExt;

const TOKEN: &str = "synthetic-write-token-for-api-tests-12345";
const READ_TOKEN: &str = "synthetic-read-token-for-api-tests-12345";

fn fixture() -> (tempfile::TempDir, Arc<Service>, Router) {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("项目/子目录")).unwrap();
    fs::write(dir.path().join("开始.md"), "# Alpha\n中文正文 🌸").unwrap();
    fs::write(dir.path().join("项目/任务.md"), "# Beta\n发布任务").unwrap();
    let state = Service::open(dir.path().into(), Some(TOKEN), Some(READ_TOKEN), None).unwrap();
    (dir, state.clone(), router(state))
}

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: serde_json::Value,
    etag: Option<&str>,
) -> (StatusCode, HeaderMap, serde_json::Value) {
    let mut request = axum::http::Request::builder()
        .method(method)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    if let Some(etag) = etag {
        request = request.header(header::IF_MATCH, etag);
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), MAX_BODY * 2).await.unwrap();
    let json = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, headers, json)
}

async fn list(app: &Router) -> serde_json::Value {
    let (status, _, body) = call(app, "GET", "/api/v1/notes", Some(TOKEN), json!(null), None).await;
    assert_eq!(status, StatusCode::OK);
    body
}

#[tokio::test]
async fn mcp_reuses_read_write_access_and_document_versions() {
    let (dir, _, app) = fixture();
    let rpc = |token, method, params| {
        call(
            &app,
            "POST",
            "/mcp",
            token,
            json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}),
            None,
        )
    };
    assert_eq!(
        rpc(None, "tools/list", json!({})).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (status, headers, init) = rpc(Some(READ_TOKEN), "initialize",
        json!({"protocolVersion":"2025-06-18","clientInfo":{"name":"test","version":"1"},"capabilities":{}})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    let read = rpc(Some(READ_TOKEN), "tools/list", json!({})).await.2;
    assert_eq!(read["result"]["tools"].as_array().unwrap().len(), 3);
    let write = rpc(Some(TOKEN), "tools/list", json!({})).await.2;
    assert_eq!(write["result"]["tools"].as_array().unwrap().len(), 5);
    let create = json!({"name":"create_note","arguments":{"folderId":"default","title":"回归 MCP","source":"# 文本"}});
    let forbidden = rpc(Some(READ_TOKEN), "tools/call", create.clone()).await.2;
    assert_eq!(forbidden["result"]["isError"], true);
    let created = rpc(Some(TOKEN), "tools/call", create).await.2;
    let note = &created["result"]["structuredContent"];
    let id = note["id"].as_str().unwrap();
    let version = note["version"].as_str().unwrap();
    let found = rpc(
        Some(READ_TOKEN),
        "tools/call",
        json!({"name":"search_notes","arguments":{"q":"回归 MCP"}}),
    )
    .await
    .2;
    assert_eq!(found["result"]["structuredContent"]["items"][0]["id"], id);
    let update =
        json!({"name":"update_note","arguments":{"id":id,"version":version,"source":"新版正文"}});
    let edited = rpc(Some(TOKEN), "tools/call", update.clone()).await.2;
    assert_eq!(edited["result"]["structuredContent"]["source"], "新版正文");
    let conflict = rpc(Some(TOKEN), "tools/call", update).await.2;
    assert!(conflict["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("version_conflict (412)"));
    let path = edited["result"]["structuredContent"]["path"]
        .as_str()
        .unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join(path)).unwrap(),
        "新版正文"
    );
}

#[tokio::test]
async fn mcp_rejects_browser_origins_and_duplicate_auth_and_accepts_notifications() {
    let (_dir, _, app) = fixture();
    let exchange = |method| {
        axum::http::Request::builder()
            .method(method)
            .uri("/mcp")
            .header(header::CONTENT_TYPE, "application/json")
    };
    let notification = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
    let request = exchange("POST")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header(header::ORIGIN, "https://example.com")
        .body(Body::from(notification))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(request).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    let request = exchange("POST")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header(header::AUTHORIZATION, "Bearer other")
        .body(Body::from(notification))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(request).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    let request = exchange("POST")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header("mcp-protocol-version", "1970-01-01")
        .body(Body::from(notification))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(request).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    let request = exchange("POST")
        .header(header::AUTHORIZATION, format!("Bearer {READ_TOKEN}"))
        .body(Body::from(notification))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(request).await.unwrap().status(),
        StatusCode::ACCEPTED
    );
    let request = exchange("GET")
        .header(header::AUTHORIZATION, format!("Bearer {READ_TOKEN}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.oneshot(request).await.unwrap().status(),
        StatusCode::METHOD_NOT_ALLOWED
    );
}

#[tokio::test]
async fn authentication_covers_every_route_and_restricts_read_tokens() {
    let (_dir, _state, app) = fixture();
    for (method, path) in [
        ("GET", "/api/v1/folders"),
        ("GET", "/api/v1/notes"),
        ("GET", "/api/v1/notes/id"),
        ("POST", "/api/v1/notes"),
        ("PUT", "/api/v1/notes/id"),
        ("GET", "/unknown"),
    ] {
        for token in [None, Some("incorrect")] {
            let (status, headers, body) = call(&app, method, path, token, json!({}), None).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {path}");
            assert_eq!(headers[header::WWW_AUTHENTICATE], "Bearer");
            assert_eq!(headers[header::CACHE_CONTROL], "no-store");
            assert_eq!(body["error"]["code"], "unauthorized");
        }
    }
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/notes",
            Some(READ_TOKEN),
            json!(null),
            None
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/notes",
            Some(READ_TOKEN),
            json!({}),
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/notes/id",
            Some(READ_TOKEN),
            json!({}),
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let request = axum::http::Request::builder()
        .uri("/api/v1/notes")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header(header::AUTHORIZATION, "Bearer invalid")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.oneshot(request).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn folders_lists_pagination_and_search_return_only_public_library_data() {
    let (dir, state, app) = fixture();
    {
        let mut store = state.store.lock().unwrap();
        let mut next = store.data.clone();
        next.sync = Some(json!({"privateCheckpoint": "not-for-the-api"}));
        store.commit(next, "edit").unwrap();
    }
    let (status, _, body) = call(
        &app,
        "GET",
        "/api/v1/folders",
        Some(TOKEN),
        json!(null),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let folders = body["items"].as_array().unwrap();
    assert_eq!(folders.len(), 3);
    let parent = folders.iter().find(|f| f["path"] == "项目").unwrap();
    let child = folders.iter().find(|f| f["path"] == "项目/子目录").unwrap();
    assert_eq!(child["parentId"], parent["id"]);
    assert!(folders.iter().find(|f| f["path"] == "").unwrap()["parentId"].is_null());
    let (status, _, page) = call(
        &app,
        "GET",
        "/api/v1/notes?limit=1",
        Some(TOKEN),
        json!(null),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    let cursor = page["nextCursor"].as_str().unwrap();
    let (status, _, last) = call(
        &app,
        "GET",
        &format!("/api/v1/notes?limit=1&cursor={cursor}"),
        Some(TOKEN),
        json!(null),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_ne!(page["items"][0]["id"], last["items"][0]["id"]);
    assert!(last["nextCursor"].is_null());
    let path = format!("/api/v1/notes?folderId={}", parent["id"].as_str().unwrap());
    let (_, _, scoped) = call(&app, "GET", &path, Some(TOKEN), json!(null), None).await;
    assert_eq!(scoped["items"].as_array().unwrap().len(), 1);
    assert_eq!(scoped["items"][0]["title"], "任务");
    for (query, count) in [("ALPHA", 1), ("%E5%8F%91%E5%B8%83", 1), ("not-present", 0)] {
        let (status, _, result) = call(
            &app,
            "GET",
            &format!("/api/v1/notes?q={query}"),
            Some(TOKEN),
            json!(null),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(result["items"].as_array().unwrap().len(), count);
    }
    let public = list(&app).await.to_string();
    assert!(!public.contains("privateCheckpoint"));
    assert!(!public.contains(dir.path().to_str().unwrap()));
    assert!(!public.contains("\"source\""));
    assert!(!public.contains("activeId"));
    for query in ["limit=0", "limit=201", "limit=abc", "unknown=1", "cursor="] {
        assert_eq!(
            call(
                &app,
                "GET",
                &format!("/api/v1/notes?{query}"),
                Some(TOKEN),
                json!(null),
                None
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/notes?folderId=missing",
            Some(TOKEN),
            json!(null),
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn create_read_update_and_restart_preserve_markdown_ids_and_versions() {
    let (dir, state, app) = fixture();
    let active = state.store.lock().unwrap().data.active_id.clone();
    let body = json!({"folderId": "default", "title": "AI 草稿", "source": "---\ntag: ai\n---\n# 你好 🌸\n- [ ] task\n"});
    let (status, headers, created) = call(
        &app,
        "POST",
        "/api/v1/notes",
        Some(TOKEN),
        body.clone(),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let path = headers[header::LOCATION].to_str().unwrap();
    let version = headers[header::ETAG].to_str().unwrap();
    assert_eq!(created["source"], body["source"]);
    assert_eq!(
        fs::read_to_string(dir.path().join("AI 草稿.md")).unwrap(),
        body["source"]
    );
    assert_eq!(state.store.lock().unwrap().data.active_id, active);
    let (_, read_headers, read) =
        call(&app, "GET", path, Some(READ_TOKEN), json!(null), None).await;
    assert_eq!(read, created);
    assert_eq!(read_headers[header::ETAG], version);
    assert_eq!(
        call(
            &app,
            "PUT",
            path,
            Some(TOKEN),
            json!({"source": "changed"}),
            None
        )
        .await
        .0,
        StatusCode::PRECONDITION_REQUIRED
    );
    for invalid in ["*", "W/\"abc\"", "unquoted"] {
        assert_eq!(
            call(
                &app,
                "PUT",
                path,
                Some(TOKEN),
                json!({"source": "changed"}),
                Some(invalid)
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    let (status, headers, updated) = call(
        &app,
        "PUT",
        path,
        Some(TOKEN),
        json!({"source": "# Updated 中文\r\n", "title": "新标题"}),
        Some(version),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["id"], created["id"]);
    assert_ne!(headers[header::ETAG], version);
    assert!(!dir.path().join("AI 草稿.md").exists());
    assert_eq!(
        fs::read_to_string(dir.path().join("新标题.md")).unwrap(),
        "# Updated 中文\r\n"
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            path,
            Some(TOKEN),
            json!({"source": "stale"}),
            Some(version)
        )
        .await
        .0,
        StatusCode::PRECONDITION_FAILED
    );
    let restarted = router(Service::open(dir.path().into(), Some(TOKEN), None, None).unwrap());
    let (status, restarted_headers, read) =
        call(&restarted, "GET", path, Some(TOKEN), json!(null), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(read["source"], updated["source"]);
    assert_eq!(restarted_headers[header::ETAG], headers[header::ETAG]);
    let (status, no_op_headers, _) = call(
        &restarted,
        "PUT",
        path,
        Some(TOKEN),
        json!({"source": updated["source"]}),
        Some(headers[header::ETAG].to_str().unwrap()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(no_op_headers[header::ETAG], headers[header::ETAG]);
}

#[tokio::test]
async fn concurrent_updates_allow_one_winner_and_external_edits_invalidate_versions() {
    let (dir, _state, app) = fixture();
    let initial = list(&app).await;
    let note = initial["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["title"] == "开始")
        .unwrap();
    let path = format!("/api/v1/notes/{}", note["id"].as_str().unwrap());
    let etag = format!("\"{}\"", note["version"].as_str().unwrap());
    // A change to another document must not invalidate this document's version.
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/notes",
            Some(TOKEN),
            json!({"folderId": "default", "title": "another", "source": "separate document"}),
            None
        )
        .await
        .0,
        StatusCode::CREATED
    );
    let (first, second) = tokio::join!(
        call(
            &app,
            "PUT",
            &path,
            Some(TOKEN),
            json!({"source": "writer one"}),
            Some(&etag)
        ),
        call(
            &app,
            "PUT",
            &path,
            Some(TOKEN),
            json!({"source": "writer two"}),
            Some(&etag)
        ),
    );
    let mut statuses = [first.0.as_u16(), second.0.as_u16()];
    statuses.sort();
    assert_eq!(statuses, [200, 412]);
    let (_, headers, _) = call(&app, "GET", &path, Some(TOKEN), json!(null), None).await;
    fs::write(dir.path().join("开始.md"), "external edit after reading").unwrap();
    assert_eq!(
        call(
            &app,
            "PUT",
            &path,
            Some(TOKEN),
            json!({"source": "overwrite"}),
            Some(headers[header::ETAG].to_str().unwrap())
        )
        .await
        .0,
        StatusCode::PRECONDITION_FAILED
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("开始.md")).unwrap(),
        "external edit after reading"
    );
    let (_, new_headers, read) = call(&app, "GET", &path, Some(TOKEN), json!(null), None).await;
    assert_eq!(read["source"], "external edit after reading");
    assert_ne!(headers[header::ETAG], new_headers[header::ETAG]);
    fs::remove_file(dir.path().join("开始.md")).unwrap();
    assert_eq!(
        call(
            &app,
            "PUT",
            &path,
            Some(TOKEN),
            json!({"source": "recreate"}),
            Some(new_headers[header::ETAG].to_str().unwrap())
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert!(!dir.path().join("开始.md").exists());
}

#[tokio::test]
async fn validation_limits_and_storage_conflicts_do_not_overwrite_existing_files() {
    let (dir, state, app) = fixture();
    for body in [
        json!({"folderId": "default", "title": "../escape", "source": "x"}),
        json!({"folderId": "default", "title": "sub\\escape", "source": "x"}),
        json!({"folderId": "default", "title": "", "source": "x"}),
        json!({"folderId": "default", "title": "safe", "source": "x", "path": "/tmp/escape.md"}),
        json!({"folderId": "default", "title": "safe", "source": "x", "sync": {}}),
    ] {
        assert!(call(&app, "POST", "/api/v1/notes", Some(TOKEN), body, None)
            .await
            .0
            .is_client_error());
    }
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/notes",
            Some(TOKEN),
            json!({"folderId": "missing", "title": "x", "source": "x"}),
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/api/v1/notes")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{".repeat(MAX_BODY + 1)))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(request).await.unwrap().status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let notes = list(&app).await;
    let note = notes["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["title"] == "开始")
        .unwrap();
    let path = format!("/api/v1/notes/{}", note["id"].as_str().unwrap());
    let etag = format!("\"{}\"", note["version"].as_str().unwrap());
    fs::write(dir.path().join("occupied.md"), "keep existing").unwrap();
    assert_eq!(
        call(
            &app,
            "PUT",
            &path,
            Some(TOKEN),
            json!({"source": "changed", "title": "occupied"}),
            Some(&etag)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("occupied.md")).unwrap(),
        "keep existing"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("开始.md")).unwrap(),
        "# Alpha\n中文正文 🌸"
    );
    // Existing creation rules allocate a new name rather than overwriting a duplicate.
    let (status, _, created) = call(
        &app,
        "POST",
        "/api/v1/notes",
        Some(TOKEN),
        json!({"folderId": "default", "title": "occupied", "source": "new copy"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["path"], "occupied 1.md");
    assert_eq!(
        fs::read_to_string(dir.path().join("occupied.md")).unwrap(),
        "keep existing"
    );
    {
        let mut store = state.store.lock().unwrap();
        let mut next = store.data.clone();
        next.notes
            .iter_mut()
            .find(|n| n.id == note["id"])
            .unwrap()
            .trashed = true;
        store.commit(next, "edit").unwrap();
    }
    assert_eq!(
        call(&app, "GET", &path, Some(TOKEN), json!(null), None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert!(!list(&app).await["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n["id"] == note["id"]));
}

#[test]
fn invalid_tokens_fail_before_initializing_storage() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("not-created");
    for token in [
        "",
        "short",
        "has spaces but otherwise long enough 123456789",
    ] {
        assert!(Service::open(root.clone(), Some(token), None, None).is_err());
        assert!(!root.exists());
    }
    assert!(Service::open(root.clone(), Some(TOKEN), Some(TOKEN), None).is_err());
    assert!(!root.exists());
}

#[tokio::test]
async fn damaged_documents_hidden_files_and_symlinks_cannot_be_used_to_overwrite_or_escape() {
    let (dir, _state, app) = fixture();
    let mut damaged = "# 中文正文\n".as_bytes().to_vec();
    damaged.push(0xff);
    fs::write(dir.path().join("bad.md"), &damaged).unwrap();
    fs::write(dir.path().join(".hidden.md"), "hidden content").unwrap();
    #[cfg(unix)]
    let outside = {
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("outside.md"), "outside secret").unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("outside.md"),
            dir.path().join("link.md"),
        )
        .unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("linked-folder")).unwrap();
        outside
    };
    let notes = list(&app).await;
    assert!(!notes.to_string().contains("hidden content"));
    assert!(!notes.to_string().contains("outside secret"));
    let bad = notes["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["title"] == "bad")
        .unwrap();
    assert_eq!(bad["readOnly"], true);
    let path = format!("/api/v1/notes/{}", bad["id"].as_str().unwrap());
    let etag = format!("\"{}\"", bad["version"].as_str().unwrap());
    assert_eq!(
        call(
            &app,
            "PUT",
            &path,
            Some(TOKEN),
            json!({"source": "overwrite"}),
            Some(&etag)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(fs::read(dir.path().join("bad.md")).unwrap(), damaged);
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/notes/..%2Foutside.md",
            Some(TOKEN),
            json!(null),
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    #[cfg(unix)]
    assert_eq!(
        fs::read_to_string(outside.path().join("outside.md")).unwrap(),
        "outside secret"
    );
}
