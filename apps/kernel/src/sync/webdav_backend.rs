//! Kernel-owned WebDAV remote-sync provider.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    time::Duration,
};

use quick_xml::{escape::unescape, events::Event, Reader};
use reqwest::{
    header::{CONTENT_LENGTH, CONTENT_TYPE, ETAG, IF_NONE_MATCH, LAST_MODIFIED},
    redirect::Policy,
    Client, Method, RequestBuilder, Url,
};
use sha2::{Digest, Sha256};

use crate::{
    ports::CredentialSecret,
    protected_paths::is_protected_sync_relative_path,
    sync::{
        backend::{RemoteSyncBackend, RemoteSyncError, RemoteSyncFile, ValidRemoteRoot},
        execution::validate_relative_path,
    },
};

const REMOTE_SYNC_TIMEOUT: Duration = Duration::from_secs(60);
const INVALID_ENDPOINT_ERROR: &str = "webdav-endpoint-invalid: WebDAV endpoint is invalid.";

pub struct WebDavSyncSettings {
    password: CredentialSecret,
    remote_root: ValidRemoteRoot,
    server_url: String,
    username: String,
}

impl WebDavSyncSettings {
    pub fn new(
        server_url: impl Into<String>,
        username: impl Into<String>,
        password: CredentialSecret,
        remote_root: ValidRemoteRoot,
    ) -> Self {
        Self {
            password,
            remote_root,
            server_url: server_url.into(),
            username: username.into(),
        }
    }
}

impl fmt::Debug for WebDavSyncSettings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebDavSyncSettings")
            .field("password", &"[REDACTED]")
            .field("remote_root", &self.remote_root)
            .field("server_url", &"[REDACTED]")
            .field("username", &"[REDACTED]")
            .finish()
    }
}

pub struct WebDavBackend {
    client: Client,
    root_url: Url,
    settings: WebDavSyncSettings,
}

impl WebDavBackend {
    pub async fn connect(settings: WebDavSyncSettings) -> Result<Self, RemoteSyncError> {
        let root_url = webdav_root_url(&settings.server_url, &settings.remote_root)?;
        let client = remote_sync_http_client()?;
        ensure_root_collections(&client, &settings).await?;
        Ok(Self {
            client,
            root_url,
            settings,
        })
    }

    /// Tests the nearest configured or existing ancestor without mutating the server.
    pub async fn test_connection(settings: &WebDavSyncSettings) -> Result<String, RemoteSyncError> {
        let client = connection_test_http_client()?;
        let segments = remote_root_segments(&settings.remote_root);
        for length in (0..=segments.len()).rev() {
            let relative_path = segments[..length].join("/");
            let url = webdav_url_with_segments(&settings.server_url, &segments[..length], true)?;
            let response = apply_basic_auth(
                client
                    .request(webdav_propfind_method()?, url)
                    .header("Depth", "0"),
                settings,
            )
            .send()
            .await
            .map_err(|_| request_error("connection probe", "PROPFIND", &relative_path))?;
            let status = response.status().as_u16();
            if matches!(status, 200 | 207) {
                return Ok(checked_target(&relative_path));
            }
            if status == 404 && length > 0 {
                continue;
            }
            return Err(status_error(
                "connection probe",
                "PROPFIND",
                &relative_path,
                status,
            ));
        }

        Err(request_error("connection probe", "PROPFIND", ""))
    }
}

impl fmt::Debug for WebDavBackend {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebDavBackend")
            .field("root_url", &self.root_url)
            .field("settings", &self.settings)
            .finish_non_exhaustive()
    }
}

impl RemoteSyncBackend for WebDavBackend {
    fn target_fingerprint_source(&self) -> String {
        format!("webdav|{}", self.root_url)
    }

    async fn list_files(&self) -> Result<BTreeMap<String, RemoteSyncFile>, RemoteSyncError> {
        list_remote_files(&self.client, &self.settings, &self.root_url).await
    }

    async fn download(
        &self,
        relative_path: &str,
        expected_identity: &str,
    ) -> Result<Vec<u8>, RemoteSyncError> {
        validate_provider_path(relative_path)?;
        let file_url = webdav_child_url(&self.root_url, relative_path, false)?;
        ensure_remote_identity(
            &self.client,
            &self.settings,
            relative_path,
            &file_url,
            Some(expected_identity),
        )
        .await?;
        let response = apply_basic_auth(self.client.get(file_url), &self.settings)
            .send()
            .await
            .map_err(|_| request_error("download", "GET", relative_path))?;
        if !response.status().is_success() {
            return Err(status_error(
                "download",
                "GET",
                relative_path,
                response.status().as_u16(),
            ));
        }

        let response_identity = identity_from_headers(response.headers(), 0);
        if !same_remote_identity(&response_identity, expected_identity) {
            return Err(remote_changed_error(relative_path));
        }
        response
            .bytes()
            .await
            .map(|bytes| bytes.to_vec())
            .map_err(|_| request_error("download", "GET", relative_path))
    }

    async fn upload(
        &self,
        relative_path: &str,
        bytes: &[u8],
        expected_identity: Option<&str>,
    ) -> Result<String, RemoteSyncError> {
        validate_provider_path(relative_path)?;
        ensure_parent_collections(&self.client, &self.settings, &self.root_url, relative_path)
            .await?;
        let file_url = webdav_child_url(&self.root_url, relative_path, false)?;
        ensure_remote_identity(
            &self.client,
            &self.settings,
            relative_path,
            &file_url,
            expected_identity,
        )
        .await?;
        let response = apply_basic_auth(
            apply_remote_precondition(
                self.client
                    .put(file_url.clone())
                    .header(CONTENT_TYPE, "application/octet-stream")
                    .body(bytes.to_vec()),
                expected_identity,
            ),
            &self.settings,
        )
        .send()
        .await
        .map_err(|_| request_error("upload", "PUT", relative_path))?;
        if !response.status().is_success() {
            return Err(status_error(
                "upload",
                "PUT",
                relative_path,
                response.status().as_u16(),
            ));
        }
        if let Some(etag) = response
            .headers()
            .get(ETAG)
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.trim().is_empty())
        {
            return Ok(remote_identity(Some(etag), None, bytes.len() as u64));
        }

        match file_identity(
            &self.client,
            &self.settings,
            relative_path,
            &file_url,
            bytes.len() as u64,
        )
        .await
        {
            Ok(identity) => Ok(identity),
            Err(_) => Ok(format!("sha256:{}", sha256_hex(bytes))),
        }
    }

    async fn delete(
        &self,
        relative_path: &str,
        expected_identity: &str,
    ) -> Result<(), RemoteSyncError> {
        validate_provider_path(relative_path)?;
        let file_url = webdav_child_url(&self.root_url, relative_path, false)?;
        ensure_remote_identity(
            &self.client,
            &self.settings,
            relative_path,
            &file_url,
            Some(expected_identity),
        )
        .await?;
        let response = apply_basic_auth(
            self.client.request(webdav_delete_method()?, file_url),
            &self.settings,
        )
        .send()
        .await
        .map_err(|_| request_error("delete", "DELETE", relative_path))?;

        if response.status().is_success() || response.status().as_u16() == 404 {
            return Ok(());
        }
        Err(status_error(
            "delete",
            "DELETE",
            relative_path,
            response.status().as_u16(),
        ))
    }
}

#[derive(Debug, Default)]
struct WebDavPropResponse {
    content_length: Option<u64>,
    etag: Option<String>,
    href: String,
    is_collection: bool,
    last_modified: Option<String>,
}

fn remote_sync_http_client() -> Result<Client, String> {
    Client::builder()
        .timeout(REMOTE_SYNC_TIMEOUT)
        .build()
        .map_err(|_| request_error("client setup", "HTTP", "").to_string())
}

fn connection_test_http_client() -> Result<Client, String> {
    Client::builder()
        .timeout(REMOTE_SYNC_TIMEOUT)
        .redirect(Policy::none())
        .build()
        .map_err(|_| request_error("client setup", "HTTP", "").to_string())
}

fn webdav_root_url(server_url: &str, remote_root: &ValidRemoteRoot) -> Result<Url, String> {
    webdav_url_with_segments(server_url, &remote_root_segments(remote_root), true)
}

fn remote_root_segments(remote_root: &ValidRemoteRoot) -> Vec<String> {
    remote_root
        .as_str()
        .split('/')
        .map(ToString::to_string)
        .collect()
}

fn validated_webdav_base_url(value: &str) -> Result<Url, String> {
    let mut url = Url::parse(value.trim()).map_err(|_| INVALID_ENDPOINT_ERROR.to_string())?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.host_str().is_none()
    {
        return Err(INVALID_ENDPOINT_ERROR.to_string());
    }
    url.set_query(None);
    url.set_fragment(None);
    let normalized_path = url.path().trim_end_matches('/').to_string();
    url.set_path(&normalized_path);
    Ok(url)
}

fn webdav_url_with_segments(
    server_url: &str,
    segments: &[String],
    trailing_slash: bool,
) -> Result<Url, String> {
    let mut url = validated_webdav_base_url(server_url)?;
    {
        let mut path_segments = url
            .path_segments_mut()
            .map_err(|_| INVALID_ENDPOINT_ERROR.to_string())?;
        for segment in segments {
            path_segments.push(segment);
        }
        if trailing_slash {
            path_segments.push("");
        }
    }
    Ok(url)
}

async fn ensure_root_collections(
    client: &Client,
    settings: &WebDavSyncSettings,
) -> Result<(), RemoteSyncError> {
    let segments = remote_root_segments(&settings.remote_root);
    for index in 0..segments.len() {
        let relative_path = segments[..=index].join("/");
        let url = webdav_url_with_segments(&settings.server_url, &segments[..=index], true)?;
        let response = apply_basic_auth(client.request(webdav_mkcol_method()?, url), settings)
            .send()
            .await
            .map_err(|_| request_error("folder creation", "MKCOL", &relative_path))?;
        if !response.status().is_success() && response.status().as_u16() != 405 {
            return Err(status_error(
                "folder creation",
                "MKCOL",
                &relative_path,
                response.status().as_u16(),
            ));
        }
    }
    Ok(())
}

async fn list_remote_files(
    client: &Client,
    settings: &WebDavSyncSettings,
    root_url: &Url,
) -> Result<BTreeMap<String, RemoteSyncFile>, RemoteSyncError> {
    let mut files = BTreeMap::new();
    let mut directories = vec![(root_url.clone(), String::new())];
    let mut visited = BTreeSet::new();

    while let Some((directory_url, directory_path)) = directories.pop() {
        if !visited.insert(directory_path.clone()) {
            continue;
        }
        let responses =
            propfind_directory(client, settings, &directory_url, &directory_path).await?;
        for response in responses {
            if response.href.trim().is_empty() {
                continue;
            }
            let Some(relative_path) = remote_relative_path(root_url, &response.href)? else {
                continue;
            };
            if relative_path.is_empty()
                || relative_path == directory_path
                || is_protected_sync_relative_path(&relative_path)
                || relative_path.split('/').any(|segment| segment == ".git")
            {
                continue;
            }
            validate_provider_path(&relative_path)?;

            if response.is_collection {
                directories.push((
                    webdav_child_url(root_url, &relative_path, true)?,
                    relative_path,
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

async fn propfind_directory(
    client: &Client,
    settings: &WebDavSyncSettings,
    directory_url: &Url,
    relative_path: &str,
) -> Result<Vec<WebDavPropResponse>, RemoteSyncError> {
    let response = apply_basic_auth(
        client
            .request(webdav_propfind_method()?, directory_url.clone())
            .header("Depth", "1")
            .header(CONTENT_TYPE, "application/xml; charset=utf-8")
            .body(propfind_body()),
        settings,
    )
    .send()
    .await
    .map_err(|_| request_error("listing", "PROPFIND depth=1", relative_path))?;
    if !(response.status().is_success() || response.status().as_u16() == 207) {
        return Err(status_error(
            "listing",
            "PROPFIND depth=1",
            relative_path,
            response.status().as_u16(),
        ));
    }
    let body = response
        .text()
        .await
        .map_err(|_| request_error("listing", "PROPFIND depth=1", relative_path))?;
    parse_propfind_response(&body)
        .map_err(|_| request_error("listing", "PROPFIND depth=1", relative_path))
}

fn propfind_body() -> &'static str {
    r#"<?xml version="1.0" encoding="utf-8" ?>
<propfind xmlns="DAV:">
  <prop>
    <resourcetype />
    <getetag />
    <getcontentlength />
    <getlastmodified />
  </prop>
</propfind>"#
}

fn parse_propfind_response(body: &str) -> Result<Vec<WebDavPropResponse>, String> {
    let mut reader = Reader::from_str(body);
    reader.config_mut().trim_text(false);
    let mut responses = Vec::new();
    let mut current: Option<WebDavPropResponse> = None;
    let mut current_field: Option<String> = None;
    let mut current_value: Option<String> = None;

    loop {
        match reader.read_event().map_err(|error| error.to_string())? {
            Event::Start(element) => {
                let name = xml_local_name(element.local_name().as_ref());
                match name.as_str() {
                    "response" => current = Some(WebDavPropResponse::default()),
                    "href" | "getetag" | "getcontentlength" | "getlastmodified" => {
                        current_field = Some(name);
                        current_value = None;
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
                    let value = text.decode().map_err(|error| error.to_string())?;
                    current_value
                        .get_or_insert_with(String::new)
                        .push_str(&value);
                }
            }
            Event::GeneralRef(reference) => {
                if current.is_some() && current_field.is_some() {
                    let reference = reference.decode().map_err(|error| error.to_string())?;
                    let encoded = format!("&{reference};");
                    let value = unescape(&encoded).map_err(|error| error.to_string())?;
                    current_value
                        .get_or_insert_with(String::new)
                        .push_str(&value);
                }
            }
            Event::End(element) => {
                let name = xml_local_name(element.local_name().as_ref());
                if current_field.as_deref() == Some(name.as_str()) {
                    if let (Some(response), Some(value)) = (current.as_mut(), current_value.take())
                    {
                        let value = value.trim().to_string();
                        match name.as_str() {
                            "href" => response.href = value,
                            "getetag" => response.etag = Some(value),
                            "getcontentlength" => {
                                response.content_length = value.parse::<u64>().ok();
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
    let href_has_authority = parsed_href.is_ok() || href.starts_with("//");
    let href_url = match parsed_href {
        Ok(url) => url,
        Err(_) => root_url
            .join(href)
            .map_err(|_| unsafe_remote_path_error().to_string())?,
    };
    if href_url.origin() != root_url.origin() {
        return Ok(None);
    }
    let Some(relative_path) = raw_relative_href_path(root_url, href, href_has_authority) else {
        return Ok(None);
    };
    let normalized = normalize_decoded_segments(&decode_path_segments(relative_path))?;
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
        Some((_, value)) => value,
        None => without_query.strip_prefix("//").unwrap_or(""),
    };
    authority_and_path
        .find('/')
        .map_or("", |index| &authority_and_path[index..])
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

fn normalize_decoded_segments(segments: &[String]) -> Result<String, String> {
    let mut normalized = Vec::new();
    for (index, segment) in segments.iter().enumerate() {
        if segment.is_empty() && (segments.len() == 1 || index + 1 == segments.len()) {
            continue;
        }
        if segment.is_empty()
            || matches!(segment.as_str(), "." | "..")
            || segment.contains(['/', '\\', '\0'])
        {
            return Err(unsafe_remote_path_error().to_string());
        }
        normalized.push(segment.as_str());
    }
    let normalized = normalized.join("/");
    if !normalized.is_empty() {
        validate_provider_path(&normalized)?;
    }
    Ok(normalized)
}

fn percent_decode_segment(segment: &str) -> String {
    let bytes = segment.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(value) = u8::from_str_radix(&segment[index + 1..index + 3], 16) {
                output.push(value);
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
    validate_provider_path(relative_path)?;
    let mut url = root_url.clone();
    {
        let mut path_segments = url
            .path_segments_mut()
            .map_err(|_| unsafe_remote_path_error().to_string())?;
        path_segments.pop_if_empty();
        for segment in relative_path.split('/') {
            path_segments.push(segment);
        }
        if trailing_slash {
            path_segments.push("");
        }
    }
    Ok(url)
}

async fn ensure_parent_collections(
    client: &Client,
    settings: &WebDavSyncSettings,
    root_url: &Url,
    relative_path: &str,
) -> Result<(), RemoteSyncError> {
    let mut segments = relative_path.split('/').collect::<Vec<_>>();
    segments.pop();
    for index in 0..segments.len() {
        let collection_path = segments[..=index].join("/");
        let collection_url = webdav_child_url(root_url, &collection_path, true)?;
        let response = apply_basic_auth(
            client.request(webdav_mkcol_method()?, collection_url),
            settings,
        )
        .send()
        .await
        .map_err(|_| request_error("folder creation", "MKCOL", &collection_path))?;
        if !response.status().is_success() && response.status().as_u16() != 405 {
            return Err(status_error(
                "folder creation",
                "MKCOL",
                &collection_path,
                response.status().as_u16(),
            ));
        }
    }
    Ok(())
}

async fn file_identity(
    client: &Client,
    settings: &WebDavSyncSettings,
    relative_path: &str,
    file_url: &Url,
    fallback_size: u64,
) -> Result<String, RemoteSyncError> {
    let response = apply_basic_auth(client.head(file_url.clone()), settings)
        .send()
        .await
        .map_err(|_| request_error("metadata", "HEAD", relative_path))?;
    if !response.status().is_success() {
        return Err(status_error(
            "metadata",
            "HEAD",
            relative_path,
            response.status().as_u16(),
        ));
    }
    Ok(identity_from_headers(response.headers(), fallback_size))
}

async fn optional_file_identity(
    client: &Client,
    settings: &WebDavSyncSettings,
    relative_path: &str,
    file_url: &Url,
) -> Result<Option<String>, RemoteSyncError> {
    let response = apply_basic_auth(
        client
            .request(webdav_propfind_method()?, file_url.clone())
            .header("Depth", "0")
            .header(CONTENT_TYPE, "application/xml; charset=utf-8")
            .body(propfind_body()),
        settings,
    )
    .send()
    .await
    .map_err(|_| request_error("metadata", "PROPFIND depth=0", relative_path))?;
    if response.status().as_u16() == 404 {
        return Ok(None);
    }
    if !(response.status().is_success() || response.status().as_u16() == 207) {
        return Err(status_error(
            "metadata",
            "PROPFIND depth=0",
            relative_path,
            response.status().as_u16(),
        ));
    }
    let body = response
        .text()
        .await
        .map_err(|_| request_error("metadata", "PROPFIND depth=0", relative_path))?;
    let response = parse_propfind_response(&body)
        .map_err(|_| request_error("metadata", "PROPFIND depth=0", relative_path))?
        .into_iter()
        .find(|response| !response.is_collection);
    Ok(response.map(|response| {
        remote_identity(
            response.etag.as_deref(),
            response.last_modified.as_deref(),
            response.content_length.unwrap_or(0),
        )
    }))
}

async fn ensure_remote_identity(
    client: &Client,
    settings: &WebDavSyncSettings,
    relative_path: &str,
    file_url: &Url,
    expected_identity: Option<&str>,
) -> Result<(), RemoteSyncError> {
    let actual = optional_file_identity(client, settings, relative_path, file_url).await?;
    if same_optional_remote_identity(actual.as_deref(), expected_identity) {
        Ok(())
    } else {
        Err(remote_changed_error(relative_path))
    }
}

fn apply_remote_precondition(
    builder: RequestBuilder,
    expected_identity: Option<&str>,
) -> RequestBuilder {
    if expected_identity.is_none() {
        builder.header(IF_NONE_MATCH, "*")
    } else {
        // Explicit PROPFIND guards weak/strong ETag differences across WebDAV methods.
        builder
    }
}

fn identity_from_headers(headers: &reqwest::header::HeaderMap, fallback_size: u64) -> String {
    remote_identity(
        headers.get(ETAG).and_then(|value| value.to_str().ok()),
        headers
            .get(LAST_MODIFIED)
            .and_then(|value| value.to_str().ok()),
        headers
            .get(CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse().ok())
            .unwrap_or(fallback_size),
    )
}

fn remote_identity(etag: Option<&str>, last_modified: Option<&str>, size: u64) -> String {
    if let Some(etag) = etag.map(str::trim).filter(|value| !value.is_empty()) {
        return canonical_etag(etag).to_string();
    }
    if let Some(last_modified) = last_modified
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return format!("modified:{last_modified};len:{size}");
    }
    format!("len:{size}")
}

fn same_optional_remote_identity(actual: Option<&str>, expected: Option<&str>) -> bool {
    match (actual, expected) {
        (Some(actual), Some(expected)) => same_remote_identity(actual, expected),
        (None, None) => true,
        _ => false,
    }
}

fn same_remote_identity(left: &str, right: &str) -> bool {
    canonical_etag(left) == canonical_etag(right)
}

fn canonical_etag(identity: &str) -> &str {
    let trimmed = identity.trim();
    if let Some(value) = trimmed
        .strip_prefix("W/")
        .or_else(|| trimmed.strip_prefix("w/"))
    {
        let value = value.trim_start();
        if value.starts_with('"') {
            return value;
        }
    }
    trimmed
}

fn apply_basic_auth(builder: RequestBuilder, settings: &WebDavSyncSettings) -> RequestBuilder {
    if settings.username.is_empty() && settings.password.expose_secret().is_empty() {
        builder
    } else {
        builder.basic_auth(&settings.username, Some(settings.password.expose_secret()))
    }
}

fn validate_provider_path(path: &str) -> Result<(), String> {
    validate_relative_path(path).map_err(|_| unsafe_remote_path_error().to_string())
}

fn unsafe_remote_path_error() -> RemoteSyncError {
    RemoteSyncError::unclassified(
        "webdav-remote-path-invalid: WebDAV returned an unsafe remote path.",
    )
}

fn request_error(action: &str, method: &str, relative_path: &str) -> RemoteSyncError {
    RemoteSyncError::unclassified(format!(
        "webdav-transport-failed: WebDAV {action} failed: {method} {}: request failed.",
        diagnostic_path(relative_path)
    ))
}

fn status_error(action: &str, method: &str, relative_path: &str, status: u16) -> RemoteSyncError {
    RemoteSyncError::unclassified(format!(
        "webdav-http-failed: WebDAV {action} failed: {method} {}: HTTP {status}.",
        diagnostic_path(relative_path)
    ))
}

fn remote_changed_error(relative_path: &str) -> RemoteSyncError {
    RemoteSyncError::unclassified(format!(
        "webdav-remote-changed: Remote sync file changed during sync: {}.",
        diagnostic_path(relative_path)
    ))
}

fn diagnostic_path(path: &str) -> String {
    let sanitized = path
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    let sanitized = sanitized.trim();
    if sanitized.is_empty() {
        "<root>".to_string()
    } else {
        sanitized.to_string()
    }
}

fn checked_target(path: &str) -> String {
    if path.is_empty() {
        "<base>".to_string()
    } else {
        diagnostic_path(path)
    }
}

fn webdav_mkcol_method() -> Result<Method, String> {
    Method::from_bytes(b"MKCOL").map_err(|_| request_error("client setup", "MKCOL", "").to_string())
}

fn webdav_propfind_method() -> Result<Method, String> {
    Method::from_bytes(b"PROPFIND")
        .map_err(|_| request_error("client setup", "PROPFIND", "").to_string())
}

fn webdav_delete_method() -> Result<Method, String> {
    Method::from_bytes(b"DELETE")
        .map_err(|_| request_error("client setup", "DELETE", "").to_string())
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
