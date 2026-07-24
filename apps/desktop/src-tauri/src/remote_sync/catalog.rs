use std::collections::BTreeSet;
use std::time::Duration;

use reqwest::header::CONTENT_TYPE;
use reqwest::redirect::Policy;
use reqwest::{Client, Url};
use serde::Serialize;

use super::backend::{notebook_name_available_on_current_platform, ValidRemoteRoot};
use super::diagnostics::{create_sync_run_id, SyncDiagnosticContext};
use super::s3_backend::{S3Backend, S3SyncSettings, S3TransportOptions};
use super::{
    apply_basic_auth, parse_webdav_propfind_response, raw_relative_href_path,
    validated_remote_path_segments, webdav_propfind_method, webdav_url_with_segments,
    WebDavPropResponse, REMOTE_SYNC_TIMEOUT_SECS,
};
use crate::sync_config::model::{SyncSnapshot, SyncTarget};

const CATALOG_UNAVAILABLE: &str =
    "remote-notebook-catalog-unavailable: The remote notebook catalog is unavailable.";

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoteNotebookCatalogEntry {
    pub(crate) available: bool,
    pub(crate) disabled_reason: Option<String>,
    pub(crate) name: String,
}

pub(crate) async fn list_remote_notebooks(
    snapshot: SyncSnapshot,
) -> Result<Vec<RemoteNotebookCatalogEntry>, String> {
    let names = match snapshot.target {
        SyncTarget::S3 {
            access_key_id,
            addressing_style,
            bucket,
            endpoint_url,
            region,
            remote_root,
            request_timeout_seconds,
            secret_access_key,
            tls_verification,
        } => {
            let notes_prefix = ValidRemoteRoot::parse(&remote_root)
                .map(|root| root.notes_prefix())
                .map_err(|_| CATALOG_UNAVAILABLE.to_string())?;
            let backend = S3Backend::new_at_validated_prefix_with_transport(
                S3SyncSettings {
                    access_key_id,
                    bucket,
                    endpoint_url,
                    region,
                    remote_path: notes_prefix,
                    secret_access_key,
                },
                S3TransportOptions {
                    addressing_style,
                    request_timeout_seconds,
                    tls_verification,
                },
            )
            .map_err(|_| CATALOG_UNAVAILABLE.to_string())?
            .with_diagnostic_context(SyncDiagnosticContext::new(create_sync_run_id(), "catalog"));
            backend
                .list_notebook_names()
                .await
                .map_err(|_| CATALOG_UNAVAILABLE.to_string())?
        }
        SyncTarget::Webdav {
            password,
            remote_root,
            server_url,
            username,
        } => list_webdav_notebook_names(server_url, remote_root, username, password).await?,
    };

    Ok(names.into_iter().filter_map(catalog_entry).collect())
}

fn catalog_entry(name: String) -> Option<RemoteNotebookCatalogEntry> {
    let name = crate::notebook_scope::validate_notebook_name(&name).ok()?;
    let available = notebook_name_available_on_current_platform(&name);
    Some(RemoteNotebookCatalogEntry {
        available,
        disabled_reason: (!available).then(|| "notebook-name-unavailable".to_string()),
        name,
    })
}

async fn list_webdav_notebook_names(
    server_url: String,
    remote_root: String,
    username: String,
    password: String,
) -> Result<Vec<String>, String> {
    let notes_prefix = ValidRemoteRoot::parse(&remote_root)
        .map(|root| root.notes_prefix())
        .map_err(|_| CATALOG_UNAVAILABLE.to_string())?;
    let root_url = webdav_url_with_segments(
        &server_url,
        &validated_remote_path_segments(&notes_prefix)
            .map_err(|_| CATALOG_UNAVAILABLE.to_string())?,
        true,
    )
    .map_err(|_| CATALOG_UNAVAILABLE.to_string())?;
    let client = Client::builder()
        .timeout(Duration::from_secs(REMOTE_SYNC_TIMEOUT_SECS))
        .redirect(Policy::none())
        .build()
        .map_err(|_| CATALOG_UNAVAILABLE.to_string())?;
    let response = apply_basic_auth(
        client
            .request(
                webdav_propfind_method().map_err(|_| CATALOG_UNAVAILABLE.to_string())?,
                root_url.clone(),
            )
            .header("Depth", "1")
            .header(CONTENT_TYPE, "application/xml; charset=utf-8")
            .body(
                r#"<?xml version="1.0" encoding="utf-8" ?>
<propfind xmlns="DAV:">
  <prop><resourcetype /></prop>
</propfind>"#,
            ),
        &username,
        &password,
    )
    .send()
    .await
    .map_err(|_| CATALOG_UNAVAILABLE.to_string())?;
    if response.status().as_u16() == 404 {
        return Ok(Vec::new());
    }
    if !(response.status().is_success() || response.status().as_u16() == 207) {
        return Err(CATALOG_UNAVAILABLE.to_string());
    }
    let body = response
        .text()
        .await
        .map_err(|_| CATALOG_UNAVAILABLE.to_string())?;
    let responses =
        parse_webdav_propfind_response(&body).map_err(|_| CATALOG_UNAVAILABLE.to_string())?;
    let names = responses
        .iter()
        .filter_map(|response| direct_webdav_collection_name(&root_url, response))
        .collect::<BTreeSet<_>>();
    Ok(names.into_iter().collect())
}

fn direct_webdav_collection_name(root_url: &Url, response: &WebDavPropResponse) -> Option<String> {
    if !response.is_collection || response.href.trim().is_empty() {
        return None;
    }
    let parsed = Url::parse(&response.href);
    let has_authority = parsed.is_ok() || response.href.starts_with("//");
    if has_authority {
        let href_url = parsed.or_else(|_| root_url.join(&response.href)).ok()?;
        if href_url.scheme() != root_url.scheme()
            || href_url.host_str() != root_url.host_str()
            || href_url.port_or_known_default() != root_url.port_or_known_default()
        {
            return None;
        }
    }
    let relative = raw_relative_href_path(root_url, &response.href, has_authority)?;
    let relative = relative.strip_suffix('/').unwrap_or(relative);
    if relative.is_empty() || relative.contains('/') {
        return None;
    }
    let name = strict_percent_decode(relative)?;
    crate::notebook_scope::validate_notebook_name(&name).ok()
}

fn strict_percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = value.get(index + 1..index + 3)?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread;

    use crate::sync_config::model::{SyncConfig, SyncProvider, SyncSnapshot, SyncTarget};

    use super::list_remote_notebooks;

    fn webdav_snapshot(server_url: String) -> SyncSnapshot {
        let mut config = SyncConfig {
            enabled: false,
            ..SyncConfig::default()
        };
        config.provider = SyncProvider::Webdav;
        config.remote_root = "root".into();
        config.webdav.server_url = server_url.clone();
        SyncSnapshot {
            config,
            revision: "rev-1".into(),
            state_root: "/unused".into(),
            target: SyncTarget::Webdav {
                password: "private-password".into(),
                remote_root: "root".into(),
                server_url,
                username: "private-user".into(),
            },
        }
    }

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

    fn multistatus(body: &str) -> String {
        format!(
            "HTTP/1.1 207 Multi-Status\r\nContent-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    fn collection_response(href: &str) -> String {
        format!(
            "<d:response><d:href>{href}</d:href><d:propstat><d:prop><d:resourcetype><d:collection /></d:resourcetype></d:prop></d:propstat></d:response>"
        )
    }

    #[test]
    fn webdav_catalog_lists_only_direct_child_collections_with_safe_entries() {
        let long_name = "x".repeat(256);
        let body = format!(
            r#"<?xml version="1.0"?><d:multistatus xmlns:d="DAV:">
  {}
  {}
  {}
  {}
  {}
  {}
  {}
  {}
  {}
  {}
  <d:response><d:href>/dav/root/notes/file.md</d:href><d:propstat><d:prop><d:resourcetype /></d:prop></d:propstat></d:response>
  {}
  {}
  {}
  {}
</d:multistatus>"#,
            collection_response("/dav/root/notes/"),
            collection_response("/dav/root/notes/Alpha/"),
            collection_response("/dav/root/notes/R&amp;D/"),
            collection_response("/dav/root/notes/.QINGYU/"),
            collection_response("/dav/root/notes/.MARKRA-SYNC/"),
            collection_response("/dav/root/notes/.markra-sync-stage-interrupted/"),
            collection_response(
                "/dav/root/notes/%20%20%E4%B8%AA%E4%BA%BA%20%E7%AC%94%E8%AE%B0%20%20/"
            ),
            collection_response(&format!("/dav/root/notes/{long_name}/")),
            collection_response("/dav/root/notes/Alpha/nested/"),
            collection_response("/dav/root/notes/.qingyu/"),
            collection_response("/dav/root/notes/%2E%2E/"),
            collection_response("/dav/root/other/Outside/"),
            collection_response("http://elsewhere.invalid/dav/root/notes/Outside/"),
            collection_response("//elsewhere.invalid/dav/root/notes/Outside/")
        );
        let (server_url, requests, handle) = spawn_webdav_fixture(vec![multistatus(&body)]);

        let entries =
            tauri::async_runtime::block_on(list_remote_notebooks(webdav_snapshot(server_url)))
                .expect("catalog should list direct WebDAV notebook collections");
        handle.join().expect("fixture should finish");

        let mut expected_names = vec![
            "Alpha".to_string(),
            "R&D".to_string(),
            "  个人 笔记  ".to_string(),
            long_name,
        ];
        expected_names.sort();
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.name.clone())
                .collect::<Vec<_>>(),
            expected_names
        );
        assert!(
            entries
                .iter()
                .find(|entry| entry.name == "Alpha")
                .unwrap()
                .available
        );
        assert!(
            entries
                .iter()
                .find(|entry| entry.name == "  个人 笔记  ")
                .unwrap()
                .available
        );
        let disabled = entries
            .iter()
            .find(|entry| entry.name.len() == 256)
            .unwrap();
        assert!(!disabled.available);
        assert_eq!(
            disabled.disabled_reason.as_deref(),
            Some("notebook-name-unavailable")
        );

        let requests = requests.lock().expect("fixture request log");
        assert_eq!(requests.len(), 1);
        assert!(requests[0].starts_with("PROPFIND /dav/root/notes/ HTTP/1.1\r\n"));
        assert!(requests[0].to_ascii_lowercase().contains("depth: 1\r\n"));
        for forbidden in ["MKCOL ", "PUT ", "DELETE ", "GET "] {
            assert!(!requests[0].starts_with(forbidden), "sent {forbidden}");
        }
    }

    #[test]
    fn webdav_catalog_treats_a_missing_notes_parent_as_empty() {
        let (server_url, requests, handle) = spawn_webdav_fixture(vec![
            "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string(),
        ]);

        let entries =
            tauri::async_runtime::block_on(list_remote_notebooks(webdav_snapshot(server_url)))
                .expect("a missing notes parent is an empty catalog");
        handle.join().expect("fixture should finish");

        assert!(entries.is_empty());
        let requests = requests.lock().expect("fixture request log");
        assert_eq!(requests.len(), 1);
        assert!(requests[0].starts_with("PROPFIND "));
    }

    #[test]
    fn catalog_errors_and_entries_expose_only_safe_payloads() {
        let response_body = "private-provider-response";
        let (server_url, _requests, handle) = spawn_webdav_fixture(vec![format!(
            "HTTP/1.1 500 Server Error\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response_body}",
            response_body.len()
        )]);
        let snapshot = webdav_snapshot(server_url.clone());

        let error = tauri::async_runtime::block_on(list_remote_notebooks(snapshot))
            .expect_err("provider errors should fail safely");
        handle.join().expect("fixture should finish");

        assert_eq!(
            error,
            "remote-notebook-catalog-unavailable: The remote notebook catalog is unavailable."
        );
        for forbidden in [
            server_url.as_str(),
            "private-user",
            "private-password",
            response_body,
            "PROPFIND",
            "HTTP 500",
        ] {
            assert!(!error.contains(forbidden), "exposed {forbidden}");
        }
    }
}
