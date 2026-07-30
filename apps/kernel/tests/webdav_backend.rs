mod ports {
    pub use qingyu_kernel::ports::*;
}

mod protected_paths {
    pub use qingyu_kernel::protected_paths::*;
}

mod sync {
    pub mod backend {
        pub use qingyu_kernel::sync::backend::*;
    }

    pub mod execution {
        pub use qingyu_kernel::sync::execution::*;
    }
}

#[path = "../src/sync/webdav_backend.rs"]
mod webdav_backend;

use ports::CredentialSecret;
use sync::backend::{RemoteSyncBackend, ValidRemoteRoot};
use webdav_backend::{WebDavBackend, WebDavSyncSettings};

fn spawn_webdav_fixture(
    responses: Vec<String>,
) -> (String, Arc<Mutex<Vec<String>>>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("fixture should bind");
    let address = listener.local_addr().expect("fixture address");
    let requests = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&requests);
    let handle = thread::spawn(move || {
        for response in responses {
            let (mut stream, _) = listener.accept().expect("fixture request");
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            loop {
                let read = stream.read(&mut buffer).expect("fixture should read");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            recorded
                .lock()
                .expect("fixture request log")
                .push(String::from_utf8_lossy(&request).into_owned());
            stream
                .write_all(response.as_bytes())
                .expect("fixture should respond");
        }
    });

    (format!("http://{address}/dav"), requests, handle)
}

fn empty_response(status: &str) -> String {
    format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
}

#[test]
fn settings_debug_redacts_endpoint_and_basic_auth_credentials() {
    let settings = WebDavSyncSettings::new(
        "https://dav.example.test/base?token=endpoint-secret",
        "private-user",
        CredentialSecret::new("never-print-this-password"),
        ValidRemoteRoot::parse("qingyu/team/notes").unwrap(),
    );

    let debug = format!("{settings:?}");

    assert!(debug.contains("[REDACTED]"));
    for secret in [
        "never-print-this-password",
        "endpoint-secret",
        "private-user",
    ] {
        assert!(!debug.contains(secret));
    }
}

#[tokio::test]
async fn connect_rejects_credentials_embedded_in_server_url_without_disclosing_them() {
    let settings = WebDavSyncSettings::new(
        "https://url-user:url-password@dav.example.test/base",
        "alice",
        CredentialSecret::new("settings-password"),
        ValidRemoteRoot::parse("qingyu/notes").unwrap(),
    );

    let error = WebDavBackend::connect(settings)
        .await
        .expect_err("URL userinfo must be rejected before any network request");
    let message = error.to_string();

    assert_eq!(
        message,
        "webdav-endpoint-invalid: WebDAV endpoint is invalid."
    );
    for secret in ["url-user", "url-password", "settings-password"] {
        assert!(!message.contains(secret));
    }
}

#[tokio::test]
async fn connect_creates_encoded_root_and_lists_remote_files() {
    let listing_body = r#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/dav/qingyu/team%20notes/notes/</d:href>
    <d:propstat><d:prop><d:resourcetype><d:collection /></d:resourcetype></d:prop></d:propstat>
  </d:response>
  <d:response>
    <d:href>/dav/qingyu/team%20notes/notes/draft.md</d:href>
    <d:propstat><d:prop>
      <d:getetag>W/&quot;draft-etag&quot;</d:getetag>
      <d:getcontentlength>12</d:getcontentlength>
      <d:resourcetype />
    </d:prop></d:propstat>
  </d:response>
</d:multistatus>"#;
    let responses = vec![
        empty_response("405 Method Not Allowed"),
        empty_response("405 Method Not Allowed"),
        empty_response("405 Method Not Allowed"),
        format!(
            "HTTP/1.1 207 Multi-Status\r\nContent-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{listing_body}",
            listing_body.len()
        ),
    ];
    let (server_url, requests, handle) = spawn_webdav_fixture(responses);
    let backend = WebDavBackend::connect(WebDavSyncSettings::new(
        server_url,
        "alice",
        CredentialSecret::new("private-password"),
        ValidRemoteRoot::parse("qingyu/team notes/notes").unwrap(),
    ))
    .await
    .expect("existing or newly created WebDAV root should connect");

    let files = backend.list_files().await.expect("listing should succeed");
    handle.join().expect("fixture should finish");

    assert_eq!(files.len(), 1);
    assert_eq!(files["draft.md"].identity, "\"draft-etag\"");
    assert_eq!(files["draft.md"].size, 12);
    let fingerprint = backend.target_fingerprint_source();
    assert!(fingerprint.ends_with("/dav/qingyu/team%20notes/notes/"));
    assert!(!fingerprint.contains("alice"));
    assert!(!fingerprint.contains("private-password"));

    let requests = requests.lock().expect("fixture request log");
    assert_eq!(requests.len(), 4);
    assert!(requests[0].starts_with("MKCOL /dav/qingyu/ HTTP/1.1"));
    assert!(requests[1].starts_with("MKCOL /dav/qingyu/team%20notes/ HTTP/1.1"));
    assert!(requests[2].starts_with("MKCOL /dav/qingyu/team%20notes/notes/ HTTP/1.1"));
    assert!(requests[3].starts_with("PROPFIND /dav/qingyu/team%20notes/notes/ HTTP/1.1"));
    assert!(requests[3].contains("depth: 1\r\n"));
}

#[tokio::test]
async fn connection_probe_is_read_only_and_falls_back_to_nearest_existing_parent() {
    let (server_url, requests, handle) = spawn_webdav_fixture(vec![
        empty_response("404 Not Found"),
        empty_response("207 Multi-Status"),
    ]);
    let settings = WebDavSyncSettings::new(
        server_url,
        "writer",
        CredentialSecret::new("private-password"),
        ValidRemoteRoot::parse("notes/2026").unwrap(),
    );

    let checked_target = WebDavBackend::test_connection(&settings)
        .await
        .expect("nearest existing collection should pass");
    handle.join().expect("fixture should finish");

    assert_eq!(checked_target, "notes");
    let requests = requests.lock().expect("fixture request log");
    assert_eq!(requests.len(), 2);
    assert!(requests[0].starts_with("PROPFIND /dav/notes/2026/ HTTP/1.1"));
    assert!(requests[1].starts_with("PROPFIND /dav/notes/ HTTP/1.1"));
    assert!(requests
        .iter()
        .all(|request| request.contains("depth: 0\r\n")));
    assert!(requests.iter().all(|request| {
        !request.starts_with("MKCOL ")
            && !request.starts_with("PUT ")
            && !request.starts_with("DELETE ")
    }));
}

#[tokio::test]
async fn connection_probe_does_not_follow_redirects_or_disclose_location() {
    let location = "http://127.0.0.1:9/outside?token=location-secret";
    let response = format!(
        "HTTP/1.1 307 Temporary Redirect\r\nLocation: {location}\r\nContent-Length: 20\r\nConnection: close\r\n\r\nredirect-secret-body"
    );
    let (server_url, requests, handle) = spawn_webdav_fixture(vec![response]);
    let settings = WebDavSyncSettings::new(
        server_url.clone(),
        "private-user",
        CredentialSecret::new("private-password"),
        ValidRemoteRoot::parse("notes").unwrap(),
    );

    let error = WebDavBackend::test_connection(&settings)
        .await
        .expect_err("redirect must be returned as a safe status error");
    handle.join().expect("fixture should finish");

    assert_eq!(error.safe_code(), "webdav-http-failed");
    assert!(error.to_string().contains("HTTP 307"));
    for forbidden in [
        location,
        server_url.as_str(),
        "location-secret",
        "redirect-secret-body",
        "private-user",
        "private-password",
    ] {
        assert!(
            !error.to_string().contains(forbidden),
            "exposed {forbidden}"
        );
    }
    let requests = requests.lock().expect("fixture request log");
    assert_eq!(requests.len(), 1);
}

#[tokio::test]
async fn download_fails_closed_when_get_identity_differs_from_metadata_probe() {
    let replacement_body = "secret replacement body";
    let metadata_body = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/dav/notes/draft.md</d:href>
    <d:propstat><d:prop>
      <d:getetag>&quot;listed-etag&quot;</d:getetag>
      <d:getcontentlength>{}</d:getcontentlength>
    </d:prop></d:propstat>
  </d:response>
</d:multistatus>"#,
        replacement_body.len()
    );
    let responses = vec![
        empty_response("405 Method Not Allowed"),
        format!(
            "HTTP/1.1 207 Multi-Status\r\nContent-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{metadata_body}",
            metadata_body.len()
        ),
        format!(
            "HTTP/1.1 200 OK\r\nETag: \"replacement-etag\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{replacement_body}",
            replacement_body.len()
        ),
    ];
    let (server_url, requests, handle) = spawn_webdav_fixture(responses);
    let backend = WebDavBackend::connect(WebDavSyncSettings::new(
        server_url.clone(),
        "private-user",
        CredentialSecret::new("private-password"),
        ValidRemoteRoot::parse("notes").unwrap(),
    ))
    .await
    .expect("root should connect");

    let error = backend
        .download("draft.md", "\"listed-etag\"")
        .await
        .expect_err("replacement between PROPFIND and GET must fail closed");
    handle.join().expect("fixture should finish");

    assert_eq!(error.safe_code(), "webdav-remote-changed");
    assert!(error.to_string().contains("changed during sync"));
    for forbidden in [
        server_url.as_str(),
        "private-user",
        "private-password",
        replacement_body,
    ] {
        assert!(
            !error.to_string().contains(forbidden),
            "exposed {forbidden}"
        );
    }
    let requests = requests.lock().expect("fixture request log");
    assert_eq!(requests.len(), 3);
    assert!(requests[1].starts_with("PROPFIND /dav/notes/draft.md HTTP/1.1"));
    assert!(requests[2].starts_with("GET /dav/notes/draft.md HTTP/1.1"));
}

#[tokio::test]
async fn upload_of_absent_file_uses_create_only_precondition() {
    let responses = vec![
        empty_response("405 Method Not Allowed"),
        empty_response("405 Method Not Allowed"),
        empty_response("404 Not Found"),
        "HTTP/1.1 201 Created\r\nETag: \"created-etag\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string(),
    ];
    let (server_url, requests, handle) = spawn_webdav_fixture(responses);
    let backend = WebDavBackend::connect(WebDavSyncSettings::new(
        server_url,
        "writer",
        CredentialSecret::new("private-password"),
        ValidRemoteRoot::parse("notes").unwrap(),
    ))
    .await
    .expect("root should connect");

    let identity = backend
        .upload("folder/draft.md", b"hello", None)
        .await
        .expect("new file should upload");
    handle.join().expect("fixture should finish");

    assert_eq!(identity, "\"created-etag\"");
    let requests = requests.lock().expect("fixture request log");
    assert_eq!(requests.len(), 4);
    assert!(requests[1].starts_with("MKCOL /dav/notes/folder/ HTTP/1.1"));
    assert!(requests[2].starts_with("PROPFIND /dav/notes/folder/draft.md HTTP/1.1"));
    assert!(requests[3].starts_with("PUT /dav/notes/folder/draft.md HTTP/1.1"));
    assert!(requests[3].contains("if-none-match: *\r\n"));
}

#[tokio::test]
async fn delete_checks_remote_identity_before_mutation() {
    let metadata_body = r#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/dav/notes/draft.md</d:href>
    <d:propstat><d:prop>
      <d:getetag>&quot;listed-etag&quot;</d:getetag>
      <d:getcontentlength>5</d:getcontentlength>
    </d:prop></d:propstat>
  </d:response>
</d:multistatus>"#;
    let responses = vec![
        empty_response("405 Method Not Allowed"),
        format!(
            "HTTP/1.1 207 Multi-Status\r\nContent-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{metadata_body}",
            metadata_body.len()
        ),
        empty_response("204 No Content"),
    ];
    let (server_url, requests, handle) = spawn_webdav_fixture(responses);
    let backend = WebDavBackend::connect(WebDavSyncSettings::new(
        server_url,
        "writer",
        CredentialSecret::new("private-password"),
        ValidRemoteRoot::parse("notes").unwrap(),
    ))
    .await
    .expect("root should connect");

    backend
        .delete("draft.md", "\"listed-etag\"")
        .await
        .expect("matching remote file should delete");
    handle.join().expect("fixture should finish");

    let requests = requests.lock().expect("fixture request log");
    assert_eq!(requests.len(), 3);
    assert!(requests[1].starts_with("PROPFIND /dav/notes/draft.md HTTP/1.1"));
    assert!(requests[2].starts_with("DELETE /dav/notes/draft.md HTTP/1.1"));
}

#[tokio::test]
async fn listing_rejects_encoded_traversal_before_protected_path_filtering() {
    let listing_body = r#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/dav/notes/.qingyu/%2e%2e/escape.md</d:href>
    <d:propstat><d:prop><d:getcontentlength>1</d:getcontentlength></d:prop></d:propstat>
  </d:response>
</d:multistatus>"#;
    let responses = vec![
        empty_response("405 Method Not Allowed"),
        format!(
            "HTTP/1.1 207 Multi-Status\r\nContent-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{listing_body}",
            listing_body.len()
        ),
    ];
    let (server_url, _requests, handle) = spawn_webdav_fixture(responses);
    let backend = WebDavBackend::connect(WebDavSyncSettings::new(
        server_url,
        "writer",
        CredentialSecret::new("private-password"),
        ValidRemoteRoot::parse("notes").unwrap(),
    ))
    .await
    .expect("root should connect");

    let error = backend
        .list_files()
        .await
        .expect_err("encoded traversal must fail the entire listing");
    handle.join().expect("fixture should finish");

    assert_eq!(error.safe_code(), "webdav-remote-path-invalid");
}
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;
