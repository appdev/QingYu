use std::collections::BTreeMap;
use std::time::Duration;

use crate::protected_paths::is_protected_sync_relative_path;
use crate::sync_config::model::{SyncConnectionTestResult, SyncSnapshot, SyncTarget};
use quick_xml::escape::unescape;
use quick_xml::events::Event;
use quick_xml::Reader;
use reqwest::header::{CONTENT_LENGTH, CONTENT_TYPE, ETAG, IF_NONE_MATCH, LAST_MODIFIED};
use reqwest::redirect::Policy;
use reqwest::{Client, Method, RequestBuilder, Url};
use sha2::{Digest, Sha256};

mod backend;
pub(crate) mod catalog;
mod diagnostics;
mod engine;
#[cfg(test)]
mod live_tests;
#[cfg(any(desktop, feature = "desktop-sidecar"))]
#[allow(dead_code)]
pub(crate) mod mcp_service;
mod s3_backend;
mod scope;
pub(crate) mod service;
mod settings_scope;

pub(crate) use backend::{sync_state_key, ValidRemoteRoot};
use backend::{RemoteSyncBackend, RemoteSyncError, RemoteSyncFile as BackendRemoteSyncFile};
use engine::validate_relative_path;
use s3_backend::{S3Backend, S3SyncSettings, S3TransportOptions};

const REMOTE_SYNC_TIMEOUT_SECS: u64 = 60;

struct WebDavSyncSettings {
    password: String,
    remote_path: String,
    server_url: String,
    username: String,
}

struct WebDavBackend {
    client: Client,
    request: WebDavSyncSettings,
    root_url: Url,
}

#[derive(Clone, Debug)]
struct RemoteSyncFile {
    identity: String,
    size: u64,
}

#[derive(Debug, Default)]
struct WebDavPropResponse {
    content_length: Option<u64>,
    etag: Option<String>,
    href: String,
    is_collection: bool,
    last_modified: Option<String>,
}

#[derive(Debug)]
struct WebDavCollectionTarget {
    relative_path: String,
    url: Url,
}

enum ConnectionTestTarget {
    Webdav {
        password: String,
        remote_path: String,
        server_url: String,
        username: String,
    },
    S3 {
        access_key_id: String,
        addressing_style: crate::sync_config::model::S3AddressingStyle,
        bucket: String,
        endpoint_url: String,
        region: String,
        remote_path: String,
        request_timeout_seconds: u32,
        secret_access_key: String,
        tls_verification: crate::sync_config::model::S3TlsVerification,
    },
}

impl From<SyncTarget> for ConnectionTestTarget {
    fn from(target: SyncTarget) -> Self {
        match target {
            SyncTarget::Webdav {
                password,
                remote_root,
                server_url,
                username,
            } => Self::Webdav {
                password,
                remote_path: remote_root,
                server_url,
                username,
            },
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
            } => Self::S3 {
                access_key_id,
                addressing_style,
                bucket,
                endpoint_url,
                region,
                remote_path: remote_root,
                request_timeout_seconds,
                secret_access_key,
                tls_verification,
            },
        }
    }
}

async fn check_connection(target: ConnectionTestTarget) -> Result<String, String> {
    match target {
        ConnectionTestTarget::Webdav {
            password,
            remote_path,
            server_url,
            username,
        } => {
            let request = WebDavSyncSettings {
                password,
                remote_path: remote_path.clone(),
                server_url,
                username,
            };
            let client = connection_test_http_client()
                .map_err(|_| connection_test_transport_error("webdav", "PROPFIND", &remote_path))?;
            test_webdav_connection(&client, &request).await
        }
        ConnectionTestTarget::S3 {
            access_key_id,
            addressing_style,
            bucket,
            endpoint_url,
            region,
            remote_path,
            request_timeout_seconds,
            secret_access_key,
            tls_verification,
        } => {
            let backend = S3Backend::new_with_transport(
                S3SyncSettings {
                    access_key_id,
                    bucket,
                    endpoint_url,
                    region,
                    remote_path: remote_path.clone(),
                    secret_access_key,
                },
                S3TransportOptions {
                    addressing_style,
                    request_timeout_seconds,
                    tls_verification,
                },
            )
            .map_err(|_| connection_test_transport_error("s3", "GET", &remote_path))?;
            backend.test_connection().await
        }
    }
}

pub(crate) async fn test_application_connection(
    snapshot: SyncSnapshot,
) -> Result<SyncConnectionTestResult, String> {
    let provider = snapshot.config.provider;
    let checked_target = check_connection(snapshot.target.into())
        .await
        .map_err(|error| {
            error.replacen(
                "project-connection-test-failed:",
                "sync-connection-test-failed:",
                1,
            )
        })?;

    Ok(SyncConnectionTestResult {
        checked_target,
        provider,
    })
}

#[cfg(test)]
async fn create_webdav_backend(request: WebDavSyncSettings) -> Result<WebDavBackend, String> {
    let segments = normalize_remote_path_segments(&request.remote_path)?;
    create_webdav_backend_with_segments(request, segments).await
}

async fn create_webdav_backend_at_validated_prefix(
    request: WebDavSyncSettings,
) -> Result<WebDavBackend, String> {
    let segments = validated_remote_path_segments(&request.remote_path)?;
    create_webdav_backend_with_segments(request, segments).await
}

async fn create_webdav_backend_with_segments(
    request: WebDavSyncSettings,
    segments: Vec<String>,
) -> Result<WebDavBackend, String> {
    let root_url = webdav_url_with_segments(&request.server_url, &segments, true)?;
    let client = remote_sync_http_client()?;
    ensure_webdav_root_collections_from_segments(&client, &request, &segments).await?;
    Ok(WebDavBackend {
        client,
        request,
        root_url,
    })
}

async fn ensure_webdav_root_collections_from_segments(
    client: &Client,
    request: &WebDavSyncSettings,
    segments: &[String],
) -> Result<(), String> {
    for target in webdav_collection_targets_from_segments(&request.server_url, &segments)? {
        let response = apply_basic_auth(
            client.request(webdav_mkcol_method()?, target.url),
            &request.username,
            &request.password,
        )
        .send()
        .await
        .map_err(|error| {
            webdav_request_error("folder creation", "MKCOL", &target.relative_path, error)
        })?;
        if !response.status().is_success() && response.status().as_u16() != 405 {
            return Err(webdav_status_error(
                "folder creation",
                "MKCOL",
                &target.relative_path,
                response.status().as_u16(),
            ));
        }
    }
    Ok(())
}

impl RemoteSyncBackend for WebDavBackend {
    fn target_fingerprint_source(&self) -> String {
        format!("webdav|{}", self.root_url)
    }

    async fn list_files(&self) -> Result<BTreeMap<String, BackendRemoteSyncFile>, RemoteSyncError> {
        list_webdav_remote_files(&self.client, &self.request, &self.root_url)
            .await
            .map(|files| {
                files
                    .into_iter()
                    .map(|(path, file)| {
                        (
                            path,
                            BackendRemoteSyncFile {
                                identity: file.identity,
                                size: file.size,
                            },
                        )
                    })
                    .collect()
            })
            .map_err(RemoteSyncError::from)
    }

    async fn download(
        &self,
        relative_path: &str,
        expected_identity: &str,
    ) -> Result<Vec<u8>, RemoteSyncError> {
        let file_url = webdav_child_url(&self.root_url, relative_path, false)
            .map_err(|error| webdav_request_error("download", "GET", relative_path, error))?;
        ensure_remote_sync_identity(
            &self.client,
            &self.request,
            relative_path,
            &file_url,
            Some(expected_identity),
        )
        .await?;
        let response = apply_basic_auth(
            self.client.get(file_url),
            &self.request.username,
            &self.request.password,
        )
        .send()
        .await
        .map_err(|error| webdav_request_error("download", "GET", relative_path, error))?;
        if !response.status().is_success() {
            return Err(webdav_status_error(
                "download",
                "GET",
                relative_path,
                response.status().as_u16(),
            )
            .into());
        }
        let response_identity = remote_identity(
            response
                .headers()
                .get(ETAG)
                .and_then(|value| value.to_str().ok()),
            response
                .headers()
                .get(LAST_MODIFIED)
                .and_then(|value| value.to_str().ok()),
            response
                .headers()
                .get(CONTENT_LENGTH)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0),
        );
        if !same_remote_identity(&response_identity, expected_identity) {
            return Err(sync_file_changed_error("Remote", relative_path).into());
        }
        response
            .bytes()
            .await
            .map(|bytes| bytes.to_vec())
            .map_err(|error| webdav_request_error("download", "GET", relative_path, error).into())
    }

    async fn upload(
        &self,
        relative_path: &str,
        bytes: &[u8],
        expected_identity: Option<&str>,
    ) -> Result<String, RemoteSyncError> {
        ensure_webdav_parent_collections(
            &self.client,
            &self.request,
            &self.root_url,
            relative_path,
        )
        .await?;
        let file_url = webdav_child_url(&self.root_url, relative_path, false)
            .map_err(|error| webdav_request_error("upload", "PUT", relative_path, error))?;
        ensure_remote_sync_identity(
            &self.client,
            &self.request,
            relative_path,
            &file_url,
            expected_identity,
        )
        .await?;
        let response = apply_basic_auth(
            apply_webdav_remote_precondition(
                self.client
                    .put(file_url.clone())
                    .header(CONTENT_TYPE, "application/octet-stream")
                    .body(bytes.to_vec()),
                expected_identity,
            ),
            &self.request.username,
            &self.request.password,
        )
        .send()
        .await
        .map_err(|error| webdav_request_error("upload", "PUT", relative_path, error))?;
        if !response.status().is_success() {
            return Err(webdav_status_error(
                "upload",
                "PUT",
                relative_path,
                response.status().as_u16(),
            )
            .into());
        }
        if let Some(etag) = response
            .headers()
            .get(ETAG)
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.trim().is_empty())
        {
            return Ok(remote_identity(Some(etag), None, bytes.len() as u64));
        }
        webdav_file_identity(
            &self.client,
            &self.request,
            relative_path,
            &file_url,
            bytes.len() as u64,
        )
        .await
        .or_else(|_| Ok::<String, String>(format!("sha256:{}", sha256_hex(bytes))))
        .map_err(RemoteSyncError::from)
    }

    async fn delete(
        &self,
        relative_path: &str,
        expected_identity: &str,
    ) -> Result<(), RemoteSyncError> {
        delete_webdav_file(
            &self.client,
            &self.request,
            &self.root_url,
            relative_path,
            expected_identity,
        )
        .await
        .map_err(RemoteSyncError::from)
    }
}

fn remote_sync_http_client() -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(REMOTE_SYNC_TIMEOUT_SECS))
        .build()
        .map_err(|error| error.to_string())
}

fn connection_test_http_client() -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(REMOTE_SYNC_TIMEOUT_SECS))
        .redirect(Policy::none())
        .build()
        .map_err(|error| error.to_string())
}

#[cfg(test)]
fn webdav_sync_root_url(server_url: &str, remote_path: &str) -> Result<Url, String> {
    let segments = normalize_remote_path_segments(remote_path)?;
    let mut url = validated_webdav_base_url(server_url)?;
    {
        let mut path_segments = url
            .path_segments_mut()
            .map_err(|_| "WebDAV sync URL cannot be used as a base URL".to_string())?;

        for segment in segments {
            path_segments.push(&segment);
        }
        path_segments.push("");
    }

    Ok(url)
}

fn validated_webdav_base_url(value: &str) -> Result<Url, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("WebDAV sync URL is required".to_string());
    }

    let mut url = Url::parse(trimmed).map_err(|error| error.to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Only HTTP and HTTPS WebDAV sync URLs are supported".to_string());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("WebDAV sync URL must not contain userinfo".to_string());
    }

    url.set_query(None);
    url.set_fragment(None);
    let normalized_path = url.path().trim_end_matches('/').to_string();
    url.set_path(&normalized_path);

    Ok(url)
}

fn normalize_remote_path_segments(remote_path: &str) -> Result<Vec<String>, String> {
    let normalized = remote_path.trim().replace('\\', "/");
    let normalized = normalized.trim_matches('/');
    if normalized.is_empty() || normalized == "." {
        return Err("Remote sync path cannot be the WebDAV root".to_string());
    }

    let mut segments = Vec::new();
    for segment in normalized.split('/') {
        let segment = segment.trim();
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." {
            return Err("Remote sync path cannot contain parent directory segments".to_string());
        }

        segments.push(segment.to_string());
    }

    Ok(segments)
}

fn validated_remote_path_segments(remote_path: &str) -> Result<Vec<String>, String> {
    if remote_path.is_empty()
        || remote_path.starts_with(['/', '\\'])
        || remote_path.ends_with(['/', '\\'])
        || remote_path.contains('\0')
    {
        return Err("Remote sync path is invalid".to_string());
    }
    let normalized = remote_path.replace('\\', "/");
    let segments = normalized
        .split('/')
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if segments
        .iter()
        .any(|segment| segment.is_empty() || matches!(segment.as_str(), "." | ".."))
    {
        return Err("Remote sync path is invalid".to_string());
    }
    Ok(segments)
}

#[cfg(test)]
fn webdav_collection_targets(
    server_url: &str,
    remote_path: &str,
) -> Result<Vec<WebDavCollectionTarget>, String> {
    let segments = normalize_remote_path_segments(remote_path)?;
    webdav_collection_targets_from_segments(server_url, &segments)
}

fn webdav_collection_targets_from_segments(
    server_url: &str,
    segments: &[String],
) -> Result<Vec<WebDavCollectionTarget>, String> {
    let mut targets = Vec::with_capacity(segments.len());

    for index in 0..segments.len() {
        targets.push(WebDavCollectionTarget {
            relative_path: segments[..=index].join("/"),
            url: webdav_url_with_segments(server_url, &segments[..=index], true)?,
        });
    }

    Ok(targets)
}

fn webdav_connection_test_targets(
    server_url: &str,
    remote_path: &str,
) -> Result<Vec<WebDavCollectionTarget>, String> {
    let segments = normalize_remote_path_segments(remote_path)?;
    let mut targets = Vec::with_capacity(segments.len() + 1);
    for length in (0..=segments.len()).rev() {
        targets.push(WebDavCollectionTarget {
            relative_path: segments[..length].join("/"),
            url: webdav_url_with_segments(server_url, &segments[..length], true)?,
        });
    }
    Ok(targets)
}

fn webdav_connection_test_request(
    client: &Client,
    request: &WebDavSyncSettings,
    target: &WebDavCollectionTarget,
) -> Result<RequestBuilder, String> {
    Ok(apply_basic_auth(
        client
            .request(webdav_propfind_method()?, target.url.clone())
            .header("Depth", "0"),
        &request.username,
        &request.password,
    ))
}

async fn test_webdav_connection(
    client: &Client,
    request: &WebDavSyncSettings,
) -> Result<String, String> {
    let targets = webdav_connection_test_targets(&request.server_url, &request.remote_path)
        .map_err(|_| connection_test_transport_error("webdav", "PROPFIND", &request.remote_path))?;
    for (index, target) in targets.iter().enumerate() {
        let response = webdav_connection_test_request(client, request, target)
            .map_err(|_| {
                connection_test_transport_error("webdav", "PROPFIND", &target.relative_path)
            })?
            .send()
            .await
            .map_err(|_| {
                connection_test_transport_error("webdav", "PROPFIND", &target.relative_path)
            })?;
        let status = response.status().as_u16();
        if matches!(status, 200 | 207) {
            return Ok(connection_test_checked_target(&target.relative_path));
        }
        if status == 404 && index + 1 < targets.len() {
            continue;
        }
        return Err(connection_test_status_error(
            "webdav",
            "PROPFIND",
            &target.relative_path,
            status,
        ));
    }

    Err(connection_test_transport_error("webdav", "PROPFIND", ""))
}

fn connection_test_checked_target(relative_path: &str) -> String {
    if relative_path.is_empty() {
        "<base>".to_string()
    } else {
        webdav_diagnostic_relative_path(relative_path)
    }
}

fn connection_test_status_error(
    provider: &str,
    method: &str,
    relative_path: &str,
    status: u16,
) -> String {
    format!(
        "project-connection-test-failed: {provider} {method} {}: HTTP {status}",
        connection_test_checked_target(relative_path)
    )
}

fn connection_test_transport_error(provider: &str, method: &str, relative_path: &str) -> String {
    format!(
        "project-connection-test-failed: {provider} {method} {}: request failed",
        connection_test_checked_target(relative_path)
    )
}

fn webdav_url_with_segments(
    server_url: &str,
    upload_segments: &[String],
    trailing_slash: bool,
) -> Result<Url, String> {
    let mut url = validated_webdav_base_url(server_url)?;
    {
        let mut path_segments = url
            .path_segments_mut()
            .map_err(|_| "WebDAV sync URL cannot be used as a base URL".to_string())?;

        for segment in upload_segments {
            path_segments.push(segment);
        }
        if trailing_slash {
            path_segments.push("");
        }
    }

    Ok(url)
}

async fn list_webdav_remote_files(
    client: &Client,
    request: &WebDavSyncSettings,
    root_url: &Url,
) -> Result<BTreeMap<String, RemoteSyncFile>, String> {
    let mut files = BTreeMap::new();
    let mut directories = vec![(root_url.clone(), String::new())];

    while let Some((directory_url, directory_path)) = directories.pop() {
        let responses =
            propfind_webdav_directory(client, request, &directory_url, &directory_path).await?;

        for response in responses {
            if response.href.trim().is_empty() {
                continue;
            }
            let Some(relative_path) =
                remote_relative_path(root_url, &response.href).map_err(|error| {
                    webdav_request_error("listing", "PROPFIND depth=1", &directory_path, error)
                })?
            else {
                continue;
            };
            if should_skip_webdav_listing_path(&relative_path, &directory_path) {
                continue;
            }
            if relative_path.split('/').any(|segment| segment == ".git") {
                continue;
            }

            if response.is_collection {
                directories.push((
                    webdav_child_url(root_url, &relative_path, true).map_err(|error| {
                        webdav_request_error("listing", "PROPFIND depth=1", &relative_path, error)
                    })?,
                    relative_path.clone(),
                ));
            } else {
                let size = response.content_length.unwrap_or(0);
                files.insert(
                    relative_path,
                    RemoteSyncFile {
                        identity: remote_identity(
                            response.etag.as_deref(),
                            response.last_modified.as_deref(),
                            size,
                        ),
                        size,
                    },
                );
            }
        }
    }

    Ok(files)
}

fn should_skip_webdav_listing_path(relative_path: &str, directory_path: &str) -> bool {
    relative_path.is_empty()
        || relative_path == directory_path
        || is_protected_sync_relative_path(relative_path)
}

async fn propfind_webdav_directory(
    client: &Client,
    request: &WebDavSyncSettings,
    directory_url: &Url,
    relative_path: &str,
) -> Result<Vec<WebDavPropResponse>, String> {
    let response = apply_basic_auth(
        client
            .request(webdav_propfind_method()?, directory_url.clone())
            .header("Depth", "1")
            .header(CONTENT_TYPE, "application/xml; charset=utf-8")
            .body(
                r#"<?xml version="1.0" encoding="utf-8" ?>
<propfind xmlns="DAV:">
  <prop>
    <resourcetype />
    <getetag />
    <getcontentlength />
    <getlastmodified />
  </prop>
</propfind>"#,
            ),
        &request.username,
        &request.password,
    )
    .send()
    .await
    .map_err(|error| webdav_request_error("listing", "PROPFIND depth=1", relative_path, error))?;

    if !(response.status().is_success() || response.status().as_u16() == 207) {
        return Err(webdav_status_error(
            "listing",
            "PROPFIND depth=1",
            relative_path,
            response.status().as_u16(),
        ));
    }

    let body = response.text().await.map_err(|error| {
        webdav_request_error("listing", "PROPFIND depth=1", relative_path, error)
    })?;

    parse_webdav_propfind_response(&body)
        .map_err(|error| webdav_request_error("listing", "PROPFIND depth=1", relative_path, error))
}

fn parse_webdav_propfind_response(body: &str) -> Result<Vec<WebDavPropResponse>, String> {
    let mut reader = Reader::from_str(body);
    reader.config_mut().trim_text(false);
    let mut responses = Vec::new();
    let mut current: Option<WebDavPropResponse> = None;
    let mut current_field: Option<String> = None;
    let mut current_field_value: Option<String> = None;

    loop {
        match reader.read_event().map_err(|error| error.to_string())? {
            Event::Start(element) => {
                let name = xml_local_name(element.local_name().as_ref());
                match name.as_str() {
                    "response" => current = Some(WebDavPropResponse::default()),
                    "href" | "getetag" | "getcontentlength" | "getlastmodified" => {
                        current_field = Some(name);
                        current_field_value = None;
                    }
                    "collection" => {
                        if let Some(response) = current.as_mut() {
                            response.is_collection = true;
                        }
                    }
                    _ => {}
                }
            }
            Event::Empty(element) => {
                if xml_local_name(element.local_name().as_ref()) == "collection" {
                    if let Some(response) = current.as_mut() {
                        response.is_collection = true;
                    }
                }
            }
            Event::Text(text) => {
                if current.is_some() && current_field.is_some() {
                    let value = text
                        .decode()
                        .map_err(|error| error.to_string())?
                        .into_owned();
                    current_field_value
                        .get_or_insert_with(String::new)
                        .push_str(&value);
                }
            }
            Event::GeneralRef(reference) => {
                if current.is_some() && current_field.is_some() {
                    let reference = reference.decode().map_err(|error| error.to_string())?;
                    let encoded = format!("&{reference};");
                    let value = unescape(&encoded)
                        .map_err(|error| error.to_string())?
                        .into_owned();
                    current_field_value
                        .get_or_insert_with(String::new)
                        .push_str(&value);
                }
            }
            Event::End(element) => {
                let name = xml_local_name(element.local_name().as_ref());
                if current_field.as_deref() == Some(name.as_str()) {
                    if let (Some(response), Some(value)) =
                        (current.as_mut(), current_field_value.take())
                    {
                        let value = value.trim().to_string();
                        match name.as_str() {
                            "href" => response.href = value,
                            "getetag" => response.etag = Some(value),
                            "getcontentlength" => {
                                response.content_length = value.parse::<u64>().ok()
                            }
                            "getlastmodified" => response.last_modified = Some(value),
                            _ => {}
                        }
                    }
                    current_field = None;
                }
                if name == "response" {
                    if let Some(response) = current.take() {
                        responses.push(response);
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    Ok(responses)
}

fn xml_local_name(name: &[u8]) -> String {
    String::from_utf8_lossy(name).into_owned()
}

fn remote_relative_path(root_url: &Url, href: &str) -> Result<Option<String>, String> {
    let parsed_href = Url::parse(href);
    let href_is_absolute = parsed_href.is_ok();
    let href_has_authority = href_is_absolute || href.starts_with("//");
    let href_url = match parsed_href {
        Ok(url) => url,
        Err(_) => root_url.join(href).map_err(|error| error.to_string())?,
    };
    if href_url.scheme() != root_url.scheme() || href_url.host_str() != root_url.host_str() {
        return Ok(None);
    }
    let Some(relative_path) = raw_relative_href_path(root_url, href, href_has_authority) else {
        return Ok(None);
    };

    let normalized = normalize_decoded_path_segments(&decode_path_segments(relative_path))?;
    if !normalized.is_empty() && is_protected_sync_relative_path(&normalized) {
        return Ok(None);
    }
    Ok(Some(normalized))
}

fn raw_href_path(href: &str, has_authority: bool) -> &str {
    let without_fragment = href.split_once('#').map_or(href, |(path, _)| path);
    let without_query = without_fragment
        .split_once('?')
        .map_or(without_fragment, |(path, _)| path);
    if !has_authority {
        return without_query;
    }

    let authority_and_path = match without_query.split_once("://") {
        Some((_, authority_and_path)) => authority_and_path,
        None => without_query.strip_prefix("//").unwrap_or(""),
    };
    authority_and_path
        .find('/')
        .map_or("", |path| &authority_and_path[path..])
}

fn raw_relative_href_path<'a>(
    root_url: &Url,
    href: &'a str,
    href_has_authority: bool,
) -> Option<&'a str> {
    let href_path = raw_href_path(href, href_has_authority);
    if !href_has_authority && !href_path.starts_with('/') {
        return Some(href_path);
    }

    let root_path = root_url.path().trim_end_matches('/');
    if href_path.trim_end_matches('/') == root_path {
        return Some("");
    }
    let prefix = if root_path.is_empty() {
        "/".to_string()
    } else {
        format!("{root_path}/")
    };
    href_path.strip_prefix(&prefix)
}

fn decode_path_segments(path: &str) -> Vec<String> {
    path.split('/').map(percent_decode_segment).collect()
}

fn normalize_decoded_path_segments(decoded_segments: &[String]) -> Result<String, String> {
    let mut normalized_segments = Vec::new();
    for (index, decoded_segment) in decoded_segments.iter().enumerate() {
        if decoded_segment.is_empty() && decoded_segments.len() == 1 {
            continue;
        }
        if decoded_segment.is_empty() && index + 1 == decoded_segments.len() {
            continue;
        }
        if decoded_segment.is_empty()
            || matches!(decoded_segment.as_str(), "." | "..")
            || decoded_segment.contains('/')
            || decoded_segment.contains('\\')
            || decoded_segment.contains('\0')
        {
            return Err(
                "Remote sync path cannot contain parent directory segments or encoded path separators"
                    .to_string(),
            );
        }

        normalized_segments.push(decoded_segment.as_str());
    }

    let normalized = normalized_segments.join("/");
    if !normalized.is_empty() {
        validate_relative_path(&normalized)?;
    }
    Ok(normalized)
}

fn percent_decode_segment(segment: &str) -> String {
    let bytes = segment.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(hex) = u8::from_str_radix(&segment[index + 1..index + 3], 16) {
                output.push(hex);
                index += 3;
                continue;
            }
        }
        output.push(bytes[index]);
        index += 1;
    }

    String::from_utf8_lossy(&output).into_owned()
}

fn webdav_child_url(
    root_url: &Url,
    relative_path: &str,
    trailing_slash: bool,
) -> Result<Url, String> {
    let mut url = root_url.clone();
    {
        let mut path_segments = url
            .path_segments_mut()
            .map_err(|_| "WebDAV sync URL cannot be used as a base URL".to_string())?;
        // The sync root ends with `/`; remove its empty segment so appending does not create `//`.
        path_segments.pop_if_empty();
        for segment in relative_path
            .split('/')
            .filter(|segment| !segment.is_empty())
        {
            path_segments.push(segment);
        }
        if trailing_slash {
            path_segments.push("");
        }
    }

    Ok(url)
}

async fn ensure_webdav_parent_collections(
    client: &Client,
    request: &WebDavSyncSettings,
    root_url: &Url,
    relative_path: &str,
) -> Result<(), String> {
    let mut parent_segments = relative_path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    parent_segments.pop();

    for index in 0..parent_segments.len() {
        let collection_path = parent_segments[..=index].join("/");
        let collection_url =
            webdav_child_url(root_url, &collection_path, true).map_err(|error| {
                webdav_request_error("folder creation", "MKCOL", &collection_path, error)
            })?;
        let response = apply_basic_auth(
            client.request(webdav_mkcol_method()?, collection_url),
            &request.username,
            &request.password,
        )
        .send()
        .await
        .map_err(|error| {
            webdav_request_error("folder creation", "MKCOL", &collection_path, error)
        })?;

        if !(response.status().is_success() || response.status().as_u16() == 405) {
            return Err(webdav_status_error(
                "folder creation",
                "MKCOL",
                &collection_path,
                response.status().as_u16(),
            ));
        }
    }

    Ok(())
}

async fn delete_webdav_file(
    client: &Client,
    request: &WebDavSyncSettings,
    root_url: &Url,
    relative_path: &str,
    expected_remote_identity: &str,
) -> Result<(), String> {
    let file_url = webdav_child_url(root_url, relative_path, false)
        .map_err(|error| webdav_request_error("delete", "DELETE", relative_path, error))?;
    ensure_remote_sync_identity(
        client,
        request,
        relative_path,
        &file_url,
        Some(expected_remote_identity),
    )
    .await?;
    let response = apply_basic_auth(
        client.request(webdav_delete_method()?, file_url),
        &request.username,
        &request.password,
    )
    .send()
    .await
    .map_err(|error| webdav_request_error("delete", "DELETE", relative_path, error))?;

    if response.status().is_success() || response.status().as_u16() == 404 {
        return Ok(());
    }

    Err(webdav_status_error(
        "delete",
        "DELETE",
        relative_path,
        response.status().as_u16(),
    ))
}

async fn webdav_file_identity(
    client: &Client,
    request: &WebDavSyncSettings,
    relative_path: &str,
    file_url: &Url,
    fallback_size: u64,
) -> Result<String, String> {
    let response = apply_basic_auth(
        client.head(file_url.clone()),
        &request.username,
        &request.password,
    )
    .send()
    .await
    .map_err(|error| webdav_request_error("metadata", "HEAD", relative_path, error))?;

    if !response.status().is_success() {
        return Err(webdav_status_error(
            "metadata",
            "HEAD",
            relative_path,
            response.status().as_u16(),
        ));
    }

    Ok(remote_identity(
        response
            .headers()
            .get(ETAG)
            .and_then(|value| value.to_str().ok()),
        response
            .headers()
            .get(LAST_MODIFIED)
            .and_then(|value| value.to_str().ok()),
        response
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(fallback_size),
    ))
}

async fn webdav_file_identity_optional(
    client: &Client,
    request: &WebDavSyncSettings,
    relative_path: &str,
    file_url: &Url,
) -> Result<Option<String>, String> {
    let response = apply_basic_auth(
        client
            .request(webdav_propfind_method()?, file_url.clone())
            .header("Depth", "0")
            .header(CONTENT_TYPE, "application/xml; charset=utf-8")
            .body(
                r#"<?xml version="1.0" encoding="utf-8" ?>
<propfind xmlns="DAV:">
  <prop>
    <resourcetype />
    <getetag />
    <getcontentlength />
    <getlastmodified />
  </prop>
</propfind>"#,
            ),
        &request.username,
        &request.password,
    )
    .send()
    .await
    .map_err(|error| webdav_request_error("metadata", "PROPFIND depth=0", relative_path, error))?;

    if response.status().as_u16() == 404 {
        return Ok(None);
    }

    if !(response.status().is_success() || response.status().as_u16() == 207) {
        return Err(webdav_status_error(
            "metadata",
            "PROPFIND depth=0",
            relative_path,
            response.status().as_u16(),
        ));
    }

    let body = response.text().await.map_err(|error| {
        webdav_request_error("metadata", "PROPFIND depth=0", relative_path, error)
    })?;
    let responses = parse_webdav_propfind_response(&body).map_err(|error| {
        webdav_request_error("metadata", "PROPFIND depth=0", relative_path, error)
    })?;
    let Some(response) = responses
        .into_iter()
        .find(|response| !response.is_collection)
    else {
        return Ok(None);
    };

    Ok(Some(remote_identity(
        response.etag.as_deref(),
        response.last_modified.as_deref(),
        response.content_length.unwrap_or(0),
    )))
}

async fn ensure_remote_sync_identity(
    client: &Client,
    request: &WebDavSyncSettings,
    relative_path: &str,
    file_url: &Url,
    expected_identity: Option<&str>,
) -> Result<(), String> {
    let actual_identity =
        webdav_file_identity_optional(client, request, relative_path, file_url).await?;
    if same_optional_remote_identity(actual_identity.as_deref(), expected_identity) {
        return Ok(());
    }

    Err(sync_file_changed_error("Remote", relative_path))
}

fn apply_webdav_remote_precondition(
    builder: RequestBuilder,
    expected_remote_identity: Option<&str>,
) -> RequestBuilder {
    if expected_remote_identity.is_none() {
        return builder.header(IF_NONE_MATCH, "*");
    }

    // Some WebDAV servers expose weak/strong ETag variants across methods, so rely on the explicit identity probe above.
    builder
}

fn webdav_status_error(action: &str, method: &str, relative_path: &str, status: u16) -> String {
    format!(
        "WebDAV sync {action} failed: {method} {}: HTTP {status}",
        webdav_diagnostic_relative_path(relative_path)
    )
}

fn webdav_request_error(
    action: &str,
    method: &str,
    relative_path: &str,
    error: impl std::fmt::Display,
) -> String {
    format!(
        "WebDAV sync {action} failed: {method} {}: {error}",
        webdav_diagnostic_relative_path(relative_path)
    )
}

fn sync_file_changed_error(side: &str, relative_path: &str) -> String {
    format!(
        "{side} sync file changed during sync: {}",
        webdav_diagnostic_relative_path(relative_path)
    )
}

fn webdav_diagnostic_relative_path(relative_path: &str) -> String {
    let normalized = relative_path
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    let normalized = normalized.trim();

    if normalized.is_empty() {
        "<root>".to_string()
    } else {
        normalized.to_string()
    }
}

fn same_optional_remote_identity(actual: Option<&str>, expected: Option<&str>) -> bool {
    match (actual, expected) {
        (Some(actual), Some(expected)) => same_remote_identity(actual, expected),
        (None, None) => true,
        _ => false,
    }
}

fn same_remote_identity(left: &str, right: &str) -> bool {
    canonical_webdav_etag_identity(left) == canonical_webdav_etag_identity(right)
}

fn canonical_webdav_etag_identity(identity: &str) -> &str {
    let trimmed = identity.trim();
    let weak_value = trimmed
        .strip_prefix("W/")
        .or_else(|| trimmed.strip_prefix("w/"));

    if let Some(value) = weak_value {
        let value = value.trim_start();
        if value.starts_with('"') {
            return value;
        }
    }

    trimmed
}

fn remote_identity(etag: Option<&str>, last_modified: Option<&str>, size: u64) -> String {
    if let Some(etag) = etag.map(str::trim).filter(|value| !value.is_empty()) {
        return canonical_webdav_etag_identity(etag).to_string();
    }

    if let Some(last_modified) = last_modified
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return format!("modified:{last_modified};len:{size}");
    }

    format!("len:{size}")
}

fn apply_basic_auth(builder: RequestBuilder, username: &str, password: &str) -> RequestBuilder {
    if username.is_empty() && password.is_empty() {
        return builder;
    }

    builder.basic_auth(username.to_string(), Some(password.to_string()))
}

fn webdav_mkcol_method() -> Result<Method, String> {
    Method::from_bytes(b"MKCOL").map_err(|error| error.to_string())
}

fn webdav_propfind_method() -> Result<Method, String> {
    Method::from_bytes(b"PROPFIND").map_err(|error| error.to_string())
}

fn webdav_delete_method() -> Result<Method, String> {
    Method::from_bytes(b"DELETE").map_err(|error| error.to_string())
}

fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);

    for byte in digest {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }

    output
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Instant;

    use super::*;
    use crate::sync_config::model::{SyncConfig, SyncProvider};

    fn application_connection_snapshot(provider: SyncProvider, target: SyncTarget) -> SyncSnapshot {
        let mut config = SyncConfig {
            enabled: true,
            ..SyncConfig::default()
        };
        config.provider = provider;
        SyncSnapshot {
            config,
            revision: "rev-1".into(),
            state_root: std::path::PathBuf::from("/unused"),
            target,
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

    fn spawn_redirect_target_fixture() -> (String, Arc<Mutex<Vec<String>>>, thread::JoinHandle<()>)
    {
        let listener = TcpListener::bind("127.0.0.1:0").expect("redirect target should bind");
        listener
            .set_nonblocking(true)
            .expect("redirect target should be nonblocking");
        let address = listener.local_addr().expect("redirect target address");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&requests);
        let handle = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_millis(500);
            loop {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let mut request = Vec::new();
                        let mut buffer = [0_u8; 1024];
                        loop {
                            let read = stream.read(&mut buffer).expect("redirect target read");
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
                            .expect("redirect target request log")
                            .push(String::from_utf8_lossy(&request).into_owned());
                        stream
                            .write_all(
                                b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                            )
                            .expect("redirect target response");
                        break;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        if Instant::now() >= deadline {
                            break;
                        }
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("redirect target accept failed: {error}"),
                }
            }
        });

        (format!("http://{address}"), requests, handle)
    }

    #[test]
    fn builds_webdav_sync_root_url_from_relative_remote_path() {
        let url = webdav_sync_root_url(
            "https://dav.example.test/remote.php/dav/files/ada/",
            "notes/2026",
        )
        .expect("sync root URL should be built");

        assert_eq!(
            url.as_str(),
            "https://dav.example.test/remote.php/dav/files/ada/notes/2026/"
        );

        let exact_name = webdav_url_with_segments(
            "https://dav.example.test/base/",
            &validated_remote_path_segments("root/notes/  个人 笔记  ").unwrap(),
            true,
        )
        .expect("validated notebook names remain exact remote path segments");
        assert_eq!(
            exact_name.as_str(),
            "https://dav.example.test/base/root/notes/%20%20%E4%B8%AA%E4%BA%BA%20%E7%AC%94%E8%AE%B0%20%20/"
        );
    }

    #[test]
    fn webdav_target_and_state_identity_ignore_basic_auth_rotation() {
        let backend = |username: &str, password: &str| WebDavBackend {
            client: remote_sync_http_client().expect("WebDAV client"),
            request: WebDavSyncSettings {
                password: password.to_string(),
                remote_path: "root/notes/Personal".to_string(),
                server_url: "https://dav.example.test/base".to_string(),
                username: username.to_string(),
            },
            root_url: webdav_sync_root_url("https://dav.example.test/base", "root/notes/Personal")
                .expect("WebDAV target URL"),
        };
        let original = backend("first-user", "first-password");
        let rotated = backend("second-user", "second-password");
        let original_target = original.target_fingerprint_source();
        let rotated_target = rotated.target_fingerprint_source();

        assert_eq!(original_target, rotated_target);
        assert_eq!(
            crate::notebook_scope::notebook_state_key(
                &original_target,
                "root",
                "Personal",
                std::path::Path::new("/notes/Personal"),
            ),
            crate::notebook_scope::notebook_state_key(
                &rotated_target,
                "root",
                "Personal",
                std::path::Path::new("/notes/Personal"),
            )
        );
    }

    #[test]
    fn rejects_webdav_server_url_userinfo() {
        for value in [
            "https://embedded-user@dav.example.test/base",
            "https://:embedded-password@dav.example.test/base",
        ] {
            assert!(
                validated_webdav_base_url(value).is_err(),
                "WebDAV URL userinfo must be rejected"
            );
        }
    }

    #[test]
    fn builds_webdav_child_urls_without_duplicate_separators() {
        let root_url = webdav_sync_root_url("https://dav.example.test/base/", "notes")
            .expect("sync root URL should be built");

        assert_eq!(
            webdav_child_url(&root_url, "draft.md", false)
                .expect("file URL should be built")
                .as_str(),
            "https://dav.example.test/base/notes/draft.md"
        );
        assert_eq!(
            webdav_child_url(&root_url, "assets/images", true)
                .expect("directory URL should be built")
                .as_str(),
            "https://dav.example.test/base/notes/assets/images/"
        );
    }

    #[test]
    fn rejects_webdav_sync_parent_segments() {
        let error = webdav_sync_root_url("https://dav.example.test/base/", "../notes")
            .expect_err("parent segments should be rejected");

        assert!(error.contains("Remote sync path cannot contain parent directory segments"));
    }

    #[test]
    fn rejects_webdav_sync_root_remote_paths() {
        for remote_path in ["", "/", ".", " ./ "] {
            let error = webdav_sync_root_url("https://dav.example.test/base/", remote_path)
                .expect_err("root remote paths should be rejected");

            assert!(error.contains("Remote sync path cannot be the WebDAV root"));
        }
    }

    #[test]
    fn rejects_remote_href_parent_segments_after_percent_decoding() {
        let root_url = webdav_sync_root_url("https://dav.example.test/base/", "notes")
            .expect("sync root URL should be built");
        let error = remote_relative_path(
            &root_url,
            "https://dav.example.test/base/notes/folder%2f..%2fsecrets.md",
        )
        .expect_err("encoded parent segments should be rejected");

        assert!(error.contains("Remote sync path cannot contain parent directory segments"));
    }

    #[test]
    fn webdav_hrefs_reject_invalid_descendants_before_protected_filtering() {
        let root_url = webdav_sync_root_url("https://dav.example.test/base/", "notes")
            .expect("sync root URL should be built");

        for href in [
            "https://dav.example.test/base/notes/.qingyu/%2e%2e/escape.md",
            "https://dav.example.test/base/notes/.markra-sync/child%2fescape.md",
            "https://dav.example.test/base/notes/folder/.qingyu/child%5cescape.md",
        ] {
            assert!(remote_relative_path(&root_url, href).is_err(), "{href}");
        }

        for href in [
            "https://dav.example.test/base/notes/ordinary/%2e%2e/escape.md",
            "https://dav.example.test/base/notes/ordinary/child%2fescape.md",
            "https://dav.example.test/base/notes/ordinary/child%5cescape.md",
        ] {
            assert!(remote_relative_path(&root_url, href).is_err(), "{href}");
        }
    }

    #[test]
    fn webdav_protection_ignores_control_names_inside_the_sync_root_itself() {
        for (server_url, remote_path, href) in [
            (
                "https://dav.example.test/.qingyu/base/",
                "notes",
                "https://dav.example.test/.qingyu/base/notes/ordinary/file.md",
            ),
            (
                "https://dav.example.test/base/",
                ".markra-sync/notes",
                "https://dav.example.test/base/.markra-sync/notes/ordinary/file.md",
            ),
        ] {
            let root_url = webdav_sync_root_url(server_url, remote_path)
                .expect("protected names are valid inside the configured sync root");

            assert_eq!(
                remote_relative_path(&root_url, href).unwrap(),
                Some("ordinary/file.md".to_string()),
                "{href}"
            );
        }

        let ordinary_root = webdav_sync_root_url("https://dav.example.test/base/", "notes")
            .expect("sync root URL should be built");
        assert_eq!(
            remote_relative_path(
                &ordinary_root,
                "//dav.example.test/base/notes/ordinary/network-path.md"
            )
            .unwrap(),
            Some("ordinary/network-path.md".to_string())
        );
    }

    #[test]
    fn webdav_hrefs_reject_unsafe_ancestors_before_protected_filtering() {
        let root_url = webdav_sync_root_url("https://dav.example.test/base/", "notes")
            .expect("sync root URL should be built");

        for href in [
            "https://dav.example.test/base/notes/ordinary/%2e%2e/.qingyu/file.md",
            "https://dav.example.test/base/notes/ordinary/child%2fescape/.markra-sync/file.md",
        ] {
            assert!(remote_relative_path(&root_url, href).is_err(), "{href}");
        }

        for href in [
            "https://dav.example.test/base/notes/ordinary/%2e%2e/file.md",
            "https://dav.example.test/base/notes/ordinary/child%2fescape/file.md",
        ] {
            assert!(remote_relative_path(&root_url, href).is_err(), "{href}");
        }
    }

    #[test]
    fn uses_remote_last_modified_when_etag_is_missing() {
        assert_eq!(
            remote_identity(None, Some("Sun, 07 Jun 2026 02:00:00 GMT"), 128),
            "modified:Sun, 07 Jun 2026 02:00:00 GMT;len:128"
        );
    }

    #[test]
    fn normalizes_webdav_weak_etags_for_remote_identity() {
        assert_eq!(
            remote_identity(Some(" W/\"8-656032d37efc2\" "), None, 8),
            "\"8-656032d37efc2\""
        );
    }

    #[test]
    fn webdav_propfind_parser_appends_text_and_xml_entity_references() {
        let body = r#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/dav/notes/R&amp;D/</d:href>
    <d:propstat><d:prop>
      <d:getetag>&quot;etag&amp;part&quot;</d:getetag>
      <d:getcontentlength>1&#50;</d:getcontentlength>
      <d:getlastmodified>Sun, 0&#55; Jun 2026</d:getlastmodified>
      <d:resourcetype><d:collection /></d:resourcetype>
    </d:prop></d:propstat>
  </d:response>
</d:multistatus>"#;

        let responses = parse_webdav_propfind_response(body).expect("PROPFIND response");

        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0].href, "/dav/notes/R&D/");
        assert_eq!(responses[0].etag.as_deref(), Some("\"etag&part\""));
        assert_eq!(responses[0].content_length, Some(12));
        assert_eq!(
            responses[0].last_modified.as_deref(),
            Some("Sun, 07 Jun 2026")
        );
        assert!(responses[0].is_collection);
    }

    #[test]
    fn download_rejects_a_get_response_with_a_different_webdav_identity() {
        let replacement_body = "secret replacement body";
        let metadata_body = format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/dav/notes/draft.md</d:href>
    <d:propstat><d:prop>
      <d:getetag>"listed-etag"</d:getetag>
      <d:getcontentlength>{}</d:getcontentlength>
    </d:prop></d:propstat>
  </d:response>
</d:multistatus>"#,
            replacement_body.len()
        );
        let responses = vec![
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
        let settings = WebDavSyncSettings {
            password: "private-password".to_string(),
            remote_path: "notes".to_string(),
            server_url: server_url.clone(),
            username: "private-user".to_string(),
        };
        let backend = WebDavBackend {
            client: remote_sync_http_client().expect("WebDAV client"),
            request: settings,
            root_url: webdav_sync_root_url(&server_url, "notes").expect("WebDAV root"),
        };
        let expected_identity =
            remote_identity(Some("\"listed-etag\""), None, replacement_body.len() as u64);

        let error =
            tauri::async_runtime::block_on(backend.download("draft.md", &expected_identity))
                .expect_err("replacement between PROPFIND and GET must fail closed");
        handle.join().expect("fixture should finish");

        assert!(error.contains("changed during sync"), "{error}");
        for forbidden in [
            server_url.as_str(),
            "private-user",
            "private-password",
            replacement_body,
        ] {
            assert!(!error.contains(forbidden), "exposed {forbidden}");
        }
        let requests = requests.lock().expect("fixture request log");
        assert_eq!(requests.len(), 2);
        assert!(requests[0].starts_with("PROPFIND /dav/notes/draft.md HTTP/1.1"));
        assert!(requests[1].starts_with("GET /dav/notes/draft.md HTTP/1.1"));
    }

    #[test]
    fn omits_webdav_if_match_after_explicit_remote_identity_check() {
        let client = Client::new();
        let request = apply_webdav_remote_precondition(
            client
                .put("https://dav.example.test/base/draft.md")
                .body("hello"),
            Some("\"8-656032d37efc2\""),
        )
        .build()
        .expect("request should be built");

        assert!(request.headers().get("if-match").is_none());
    }

    #[test]
    fn formats_webdav_http_errors_with_request_context() {
        assert_eq!(
            webdav_status_error("folder creation", "MKCOL", "notes", 409),
            "WebDAV sync folder creation failed: MKCOL notes: HTTP 409"
        );
        assert_eq!(
            webdav_status_error("listing", "PROPFIND depth=1", "", 400),
            "WebDAV sync listing failed: PROPFIND depth=1 <root>: HTTP 400"
        );
        assert_eq!(
            webdav_status_error("metadata", "PROPFIND depth=0", "notes/draft.md", 400),
            "WebDAV sync metadata failed: PROPFIND depth=0 notes/draft.md: HTTP 400"
        );
        assert_eq!(
            webdav_status_error("metadata", "HEAD", "folder/image.png", 405),
            "WebDAV sync metadata failed: HEAD folder/image.png: HTTP 405"
        );
        assert_eq!(
            webdav_status_error("upload", "PUT", "folder/image.png", 507),
            "WebDAV sync upload failed: PUT folder/image.png: HTTP 507"
        );
        assert_eq!(
            webdav_status_error("download", "GET", "folder/image.png", 503),
            "WebDAV sync download failed: GET folder/image.png: HTTP 503"
        );
        assert_eq!(
            webdav_status_error("delete", "DELETE", "folder/image.png", 423),
            "WebDAV sync delete failed: DELETE folder/image.png: HTTP 423"
        );
    }

    #[test]
    fn formats_webdav_transport_and_file_guard_errors_with_context() {
        assert_eq!(
            webdav_request_error(
                "listing",
                "PROPFIND depth=1",
                "folder\nbad",
                "mock transport"
            ),
            "WebDAV sync listing failed: PROPFIND depth=1 folder bad: mock transport"
        );
        assert_eq!(
            sync_file_changed_error("Remote", "notes/draft.md"),
            "Remote sync file changed during sync: notes/draft.md"
        );
        assert_eq!(
            sync_file_changed_error("Local", ""),
            "Local sync file changed during sync: <root>"
        );
    }

    #[test]
    fn builds_webdav_collection_targets_with_diagnostic_paths() {
        let targets = webdav_collection_targets("https://dav.example.test/base/", "notes/2026")
            .expect("collection targets should be built");
        let diagnostic_paths = targets
            .iter()
            .map(|target| target.relative_path.as_str())
            .collect::<Vec<_>>();

        assert_eq!(diagnostic_paths, vec!["notes", "notes/2026"]);
    }

    #[test]
    fn connection_test_builds_only_depth_zero_propfind_requests() {
        let client = Client::new();
        let request = WebDavSyncSettings {
            password: "secret-password".to_string(),
            remote_path: "notes/2026".to_string(),
            server_url: "https://dav.example.test/base/".to_string(),
            username: "writer".to_string(),
        };
        let targets = webdav_connection_test_targets(&request.server_url, &request.remote_path)
            .expect("connection targets");

        assert_eq!(
            targets
                .iter()
                .map(|target| target.relative_path.as_str())
                .collect::<Vec<_>>(),
            vec!["notes/2026", "notes", ""]
        );
        for target in targets {
            let built = webdav_connection_test_request(&client, &request, &target)
                .expect("connection request")
                .build()
                .expect("built request");
            assert_eq!(built.method().as_str(), "PROPFIND");
            assert_eq!(built.headers().get("Depth").unwrap(), "0");
            assert!(!matches!(
                built.method().as_str(),
                "MKCOL" | "PUT" | "DELETE"
            ));
        }
    }

    #[test]
    fn connection_test_webdav_fallback_is_bounded_to_the_nearest_existing_parent() {
        let (server_url, requests, handle) = spawn_webdav_fixture(vec![
            "HTTP/1.1 404 Not Found\r\nContent-Length: 18\r\nConnection: close\r\n\r\nsecret target body".to_string(),
            "HTTP/1.1 207 Multi-Status\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string(),
        ]);
        let settings = WebDavSyncSettings {
            password: "secret-password".to_string(),
            remote_path: "notes/2026".to_string(),
            server_url,
            username: "writer".to_string(),
        };

        let checked_target = tauri::async_runtime::block_on(test_webdav_connection(
            &remote_sync_http_client().expect("connection client"),
            &settings,
        ))
        .expect("nearest existing collection should pass");
        handle.join().expect("fixture should finish");

        assert_eq!(checked_target, "notes");
        let requests = requests.lock().expect("fixture request log");
        assert_eq!(requests.len(), 2);
        assert!(requests[0].starts_with("PROPFIND /dav/notes/2026/ HTTP/1.1\r\n"));
        assert!(requests[1].starts_with("PROPFIND /dav/notes/ HTTP/1.1\r\n"));
        assert!(requests
            .iter()
            .all(|request| request.contains("depth: 0\r\n")));
        assert!(requests.iter().all(|request| {
            !request.starts_with("MKCOL ")
                && !request.starts_with("PUT ")
                && !request.starts_with("DELETE ")
        }));
    }

    #[test]
    fn connection_test_webdav_error_excludes_credentials_url_and_response_body() {
        let (server_url, _requests, handle) = spawn_webdav_fixture(vec![
            "HTTP/1.1 401 Unauthorized\r\nContent-Length: 20\r\nConnection: close\r\n\r\nsecret response body".to_string(),
        ]);
        let settings = WebDavSyncSettings {
            password: "secret-password".to_string(),
            remote_path: "notes".to_string(),
            server_url: server_url.clone(),
            username: "private-user".to_string(),
        };

        let error = tauri::async_runtime::block_on(test_webdav_connection(
            &remote_sync_http_client().expect("connection client"),
            &settings,
        ))
        .expect_err("unauthorized probe should fail safely");
        handle.join().expect("fixture should finish");

        assert!(error.contains("webdav"));
        assert!(error.contains("PROPFIND"));
        assert!(error.contains("HTTP 401"));
        for forbidden in [
            "secret-password",
            "private-user",
            server_url.as_str(),
            "secret response body",
            "Authorization",
        ] {
            assert!(!error.contains(forbidden), "exposed {forbidden}");
        }
    }

    #[test]
    fn connection_test_webdav_rejects_redirects_without_visiting_location() {
        for (status, reason) in [(303, "See Other"), (307, "Temporary Redirect")] {
            let (redirect_target, redirected_requests, redirect_handle) =
                spawn_redirect_target_fixture();
            let location = format!("{redirect_target}/outside?token=location-secret");
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\nLocation: {location}\r\nContent-Length: 20\r\nConnection: close\r\n\r\nredirect-secret-body"
            );
            let (server_url, original_requests, source_handle) =
                spawn_webdav_fixture(vec![response]);
            let result = tauri::async_runtime::block_on(test_application_connection(
                application_connection_snapshot(
                    SyncProvider::Webdav,
                    SyncTarget::Webdav {
                        password: "provider-password".to_string(),
                        remote_root: "notes".to_string(),
                        server_url: server_url.clone(),
                        username: "provider-user".to_string(),
                    },
                ),
            ));
            source_handle.join().expect("redirect source should finish");
            redirect_handle
                .join()
                .expect("redirect target should finish");

            let error = result.expect_err("WebDAV redirect must be a safe status error");
            assert_eq!(
                error,
                format!("sync-connection-test-failed: webdav PROPFIND notes: HTTP {status}")
            );
            let original_requests = original_requests.lock().expect("source request log");
            assert_eq!(original_requests.len(), 1);
            assert!(original_requests[0].starts_with("PROPFIND /dav/notes/ HTTP/1.1\r\n"));
            assert!(original_requests[0].contains("depth: 0\r\n"));
            assert!(
                redirected_requests
                    .lock()
                    .expect("redirect target request log")
                    .is_empty(),
                "HTTP {status} followed Location"
            );
            for forbidden in [
                redirect_target.as_str(),
                location.as_str(),
                server_url.as_str(),
                "location-secret",
                "redirect-secret-body",
                "provider-user",
                "provider-password",
            ] {
                assert!(
                    !error.contains(forbidden),
                    "HTTP {status} exposed {forbidden}"
                );
            }
        }
    }

    #[test]
    fn connection_test_webdav_encoded_collection_falls_back_to_server_base() {
        let (server_url, requests, handle) = spawn_webdav_fixture(vec![
            "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string(),
            "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string(),
            "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string(),
        ]);
        let settings = WebDavSyncSettings {
            password: String::new(),
            remote_path: "团队 notes/2026 #".to_string(),
            server_url,
            username: String::new(),
        };

        let checked_target = tauri::async_runtime::block_on(test_webdav_connection(
            &remote_sync_http_client().expect("connection client"),
            &settings,
        ))
        .expect("server base should pass");
        handle.join().expect("fixture should finish");

        assert_eq!(checked_target, "<base>");
        let requests = requests.lock().expect("fixture request log");
        assert_eq!(requests.len(), 3);
        assert!(requests[0]
            .starts_with("PROPFIND /dav/%E5%9B%A2%E9%98%9F%20notes/2026%20%23/ HTTP/1.1\r\n"));
        assert!(requests[1].starts_with("PROPFIND /dav/%E5%9B%A2%E9%98%9F%20notes/ HTTP/1.1\r\n"));
        assert!(requests[2].starts_with("PROPFIND /dav/ HTTP/1.1\r\n"));
        assert!(requests
            .iter()
            .all(|request| request.contains("depth: 0\r\n")));
    }

    #[test]
    fn connection_test_s3_preflight_error_uses_only_safe_provider_method_and_target() {
        let s3_error = tauri::async_runtime::block_on(test_application_connection(
            application_connection_snapshot(
                SyncProvider::S3,
                SyncTarget::S3 {
                    access_key_id: "private-access-key".to_string(),
                    bucket: "invalid/bucket".to_string(),
                    endpoint_url: "https://s3.example.test".to_string(),
                    region: "test-region-1".to_string(),
                    remote_root: "notes/personal".to_string(),
                    secret_access_key: "private-secret-key".to_string(),
                    addressing_style: Default::default(),
                    request_timeout_seconds: 60,
                    tls_verification: Default::default(),
                },
            ),
        ))
        .expect_err("invalid S3 backend should fail before a request");
        assert_eq!(
            s3_error,
            "sync-connection-test-failed: s3 GET notes/personal: request failed"
        );

        for forbidden in [
            "private-access-key",
            "private-secret-key",
            "s3.example.test",
        ] {
            assert!(!s3_error.contains(forbidden), "exposed {forbidden}");
        }
    }

    #[test]
    fn skips_current_collection_href_when_listing_nested_webdav_directory() {
        assert!(should_skip_webdav_listing_path("", ""));
        assert!(should_skip_webdav_listing_path("notes", "notes"));
        assert!(should_skip_webdav_listing_path(".qingyu", ""));
        assert!(should_skip_webdav_listing_path(
            ".qingyu/sync/status.json",
            ".qingyu"
        ));
        assert!(should_skip_webdav_listing_path(
            "folder/.markra-sync/manifest.json",
            "folder"
        ));
        assert!(!should_skip_webdav_listing_path("notes/draft.md", "notes"));
        assert!(!should_skip_webdav_listing_path("notes/child", "notes"));
    }
}
