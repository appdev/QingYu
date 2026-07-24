use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use quick_xml::escape::unescape;
use quick_xml::events::Event;
use quick_xml::Reader;
use reqwest::header::{
    HeaderMap, HeaderValue, CONTENT_LENGTH, ETAG, IF_MATCH, IF_NONE_MATCH, LAST_MODIFIED,
};
use reqwest::redirect::Policy;
use reqwest::{Client, ClientBuilder, Method, Response, StatusCode, Url};
use time::OffsetDateTime;

use super::backend::{RemoteSyncBackend, RemoteSyncError, RemoteSyncFile, SyncProviderOperation};
use super::diagnostics::{
    create_sync_run_id, record_s3_request_retrying, record_s3_request_succeeded, s3_http_failure,
    s3_integrity_failure, s3_replan_required, s3_transport_failure, SyncDiagnosticContext,
};
use super::engine::validate_relative_path;
use crate::protected_paths::is_protected_sync_relative_path;
use crate::s3_http::{
    s3_bucket_url, s3_object_url, signed_s3_headers, S3Connection, S3Payload,
    S3_LOGICAL_EMPTY_HEADER, S3_LOGICAL_EMPTY_SENTINEL,
};
use crate::sync_config::model::{S3AddressingStyle, S3TlsVerification};

const REMOTE_SYNC_TIMEOUT_SECS: u64 = 60;
const S3_REQUEST_MAX_ATTEMPTS: u8 = 3;

pub(crate) struct S3SyncSettings {
    pub(crate) access_key_id: String,
    pub(crate) bucket: String,
    pub(crate) endpoint_url: String,
    pub(crate) region: String,
    pub(crate) remote_path: String,
    pub(crate) secret_access_key: String,
}

#[derive(Clone, Copy)]
pub(crate) struct S3TransportOptions {
    pub(crate) addressing_style: S3AddressingStyle,
    pub(crate) request_timeout_seconds: u32,
    pub(crate) tls_verification: S3TlsVerification,
}

impl Default for S3TransportOptions {
    fn default() -> Self {
        Self {
            addressing_style: S3AddressingStyle::Auto,
            request_timeout_seconds: REMOTE_SYNC_TIMEOUT_SECS as u32,
            tls_verification: S3TlsVerification::Verify,
        }
    }
}

#[derive(Clone, Copy)]
struct S3ClientPolicy {
    accept_invalid_certificates: bool,
    timeout: Duration,
}

fn s3_client_policy(options: S3TransportOptions) -> S3ClientPolicy {
    S3ClientPolicy {
        accept_invalid_certificates: matches!(options.tls_verification, S3TlsVerification::Skip),
        timeout: Duration::from_secs(u64::from(options.request_timeout_seconds)),
    }
}

fn s3_client_builder(policy: S3ClientPolicy) -> ClientBuilder {
    Client::builder()
        .timeout(policy.timeout)
        .danger_accept_invalid_certs(policy.accept_invalid_certificates)
}

pub(crate) struct S3Backend {
    client: Client,
    connection_test_client: Client,
    connection: S3Connection,
    diagnostic_context: SyncDiagnosticContext,
    prefix: String,
    prefix_segments: Vec<String>,
}

#[derive(Debug, Default)]
struct ListObjectsPage {
    files: BTreeMap<String, RemoteSyncFile>,
    next_continuation_token: Option<String>,
}

#[derive(Debug, Default)]
struct ListNotebookPrefixesPage {
    names: BTreeSet<String>,
    next_continuation_token: Option<String>,
}

#[derive(Debug, Default)]
struct ListObjectEntry {
    etag: Option<String>,
    key: String,
    last_modified: Option<String>,
    size: u64,
}

struct BufferedS3Response {
    body: Vec<u8>,
    headers: HeaderMap,
    started_at: Instant,
    status: StatusCode,
}

impl S3Backend {
    #[cfg(test)]
    pub(crate) fn new(settings: S3SyncSettings) -> Result<Self, String> {
        Self::new_with_transport(settings, S3TransportOptions::default())
    }

    pub(crate) fn new_with_transport(
        settings: S3SyncSettings,
        transport: S3TransportOptions,
    ) -> Result<Self, String> {
        let prefix_segments = normalize_prefix_segments(&settings.remote_path)?;
        Self::new_with_prefix_segments(settings, prefix_segments, transport)
    }

    #[cfg(test)]
    pub(crate) fn new_at_validated_prefix(settings: S3SyncSettings) -> Result<Self, String> {
        Self::new_at_validated_prefix_with_transport(settings, S3TransportOptions::default())
    }

    pub(crate) fn new_at_validated_prefix_with_transport(
        settings: S3SyncSettings,
        transport: S3TransportOptions,
    ) -> Result<Self, String> {
        let prefix_segments = validated_prefix_segments(&settings.remote_path)?;
        Self::new_with_prefix_segments(settings, prefix_segments, transport)
    }

    fn new_with_prefix_segments(
        settings: S3SyncSettings,
        prefix_segments: Vec<String>,
        transport: S3TransportOptions,
    ) -> Result<Self, String> {
        let connection = S3Connection::new_with_addressing_style(
            &settings.endpoint_url,
            &settings.region,
            &settings.bucket,
            &settings.access_key_id,
            &settings.secret_access_key,
            transport.addressing_style,
        )?;
        let prefix = format!("{}/", prefix_segments.join("/"));
        let policy = s3_client_policy(transport);
        let client = s3_client_builder(policy)
            .build()
            .map_err(|error| error.to_string())?;
        let connection_test_client = s3_client_builder(policy)
            .redirect(Policy::none())
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            client,
            connection_test_client,
            connection,
            diagnostic_context: SyncDiagnosticContext::new(create_sync_run_id(), "connection"),
            prefix,
            prefix_segments,
        })
    }

    pub(crate) fn with_diagnostic_context(mut self, context: SyncDiagnosticContext) -> Self {
        self.diagnostic_context = context;
        self
    }

    fn object_url(&self, relative_path: &str) -> Result<Url, String> {
        let segments = object_segments(&self.prefix_segments, relative_path)?;
        s3_object_url(&self.connection, &segments)
    }

    async fn send_with_retry(
        &self,
        client: &Client,
        method: Method,
        url: &Url,
        payload: S3Payload<'_>,
        content_type: Option<&str>,
        body: Option<&[u8]>,
        extra_headers: Option<&HeaderMap>,
        operation: SyncProviderOperation,
        relative_path: &str,
    ) -> Result<(Response, Instant), RemoteSyncError> {
        let started_at = Instant::now();
        for attempt in 1..=S3_REQUEST_MAX_ATTEMPTS {
            let mut headers = signed_s3_headers(
                &method,
                url,
                payload,
                content_type,
                &self.connection,
                OffsetDateTime::now_utc(),
            )?;
            if let Some(extra_headers) = extra_headers {
                headers.extend(extra_headers.clone());
            }
            let mut request = client.request(method.clone(), url.clone()).headers(headers);
            if let Some(bytes) = body {
                request = request.body(bytes.to_vec());
            }
            match request.send().await {
                Ok(response)
                    if is_retryable_s3_status(response.status())
                        && attempt < S3_REQUEST_MAX_ATTEMPTS =>
                {
                    record_s3_request_retrying(
                        &self.diagnostic_context,
                        method.as_str(),
                        operation,
                        relative_path,
                        Some(response.status().as_u16()),
                        attempt,
                        S3_REQUEST_MAX_ATTEMPTS,
                        started_at.elapsed(),
                    );
                    wait_before_s3_retry(attempt).await;
                }
                Ok(response) => return Ok((response, started_at)),
                Err(_) if attempt < S3_REQUEST_MAX_ATTEMPTS => {
                    record_s3_request_retrying(
                        &self.diagnostic_context,
                        method.as_str(),
                        operation,
                        relative_path,
                        None,
                        attempt,
                        S3_REQUEST_MAX_ATTEMPTS,
                        started_at.elapsed(),
                    );
                    wait_before_s3_retry(attempt).await;
                }
                Err(_) => {
                    return Err(s3_transport_failure(
                        &self.diagnostic_context,
                        operation,
                        method.as_str(),
                        relative_path,
                        started_at.elapsed(),
                    ));
                }
            }
        }
        unreachable!("S3 retry loop always returns on its final attempt")
    }

    async fn send_get_bytes_with_retry(
        &self,
        client: &Client,
        url: &Url,
        operation: SyncProviderOperation,
        relative_path: &str,
    ) -> Result<BufferedS3Response, RemoteSyncError> {
        let started_at = Instant::now();
        for attempt in 1..=S3_REQUEST_MAX_ATTEMPTS {
            let headers = signed_s3_headers(
                &Method::GET,
                url,
                S3Payload::Empty,
                None,
                &self.connection,
                OffsetDateTime::now_utc(),
            )?;
            let response = match client.get(url.clone()).headers(headers).send().await {
                Ok(response) => response,
                Err(_) if attempt < S3_REQUEST_MAX_ATTEMPTS => {
                    record_s3_request_retrying(
                        &self.diagnostic_context,
                        "GET",
                        operation,
                        relative_path,
                        None,
                        attempt,
                        S3_REQUEST_MAX_ATTEMPTS,
                        started_at.elapsed(),
                    );
                    wait_before_s3_retry(attempt).await;
                    continue;
                }
                Err(_) => {
                    return Err(s3_transport_failure(
                        &self.diagnostic_context,
                        operation,
                        "GET",
                        relative_path,
                        started_at.elapsed(),
                    ));
                }
            };
            if is_retryable_s3_status(response.status()) && attempt < S3_REQUEST_MAX_ATTEMPTS {
                record_s3_request_retrying(
                    &self.diagnostic_context,
                    "GET",
                    operation,
                    relative_path,
                    Some(response.status().as_u16()),
                    attempt,
                    S3_REQUEST_MAX_ATTEMPTS,
                    started_at.elapsed(),
                );
                wait_before_s3_retry(attempt).await;
                continue;
            }
            if !response.status().is_success() {
                return Err(s3_http_failure(
                    &self.diagnostic_context,
                    operation,
                    "GET",
                    relative_path,
                    response,
                    started_at.elapsed(),
                )
                .await);
            }
            let status = response.status();
            let headers = response.headers().clone();
            match response.bytes().await {
                Ok(body) => {
                    return Ok(BufferedS3Response {
                        body: body.to_vec(),
                        headers,
                        started_at,
                        status,
                    });
                }
                Err(_) if attempt < S3_REQUEST_MAX_ATTEMPTS => {
                    record_s3_request_retrying(
                        &self.diagnostic_context,
                        "GET",
                        operation,
                        relative_path,
                        None,
                        attempt,
                        S3_REQUEST_MAX_ATTEMPTS,
                        started_at.elapsed(),
                    );
                    wait_before_s3_retry(attempt).await;
                }
                Err(_) => {
                    return Err(s3_transport_failure(
                        &self.diagnostic_context,
                        operation,
                        "GET",
                        relative_path,
                        started_at.elapsed(),
                    ));
                }
            }
        }
        unreachable!("S3 buffered retry loop always returns on its final attempt")
    }

    async fn head_identity(&self, relative_path: &str) -> Result<Option<String>, RemoteSyncError> {
        let url = self.object_url(relative_path)?;
        let (response, started_at) = self
            .send_with_retry(
                &self.client,
                Method::HEAD,
                &url,
                S3Payload::Empty,
                None,
                None,
                None,
                SyncProviderOperation::Metadata,
                relative_path,
            )
            .await?;
        if response.status().as_u16() == 404 {
            record_s3_request_succeeded(
                &self.diagnostic_context,
                "HEAD",
                SyncProviderOperation::Metadata,
                relative_path,
                404,
                started_at.elapsed(),
            );
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(s3_http_failure(
                &self.diagnostic_context,
                SyncProviderOperation::Metadata,
                "HEAD",
                relative_path,
                response,
                started_at.elapsed(),
            )
            .await);
        }
        record_s3_request_succeeded(
            &self.diagnostic_context,
            "HEAD",
            SyncProviderOperation::Metadata,
            relative_path,
            response.status().as_u16(),
            started_at.elapsed(),
        );
        Ok(Some(object_identity(
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
        )))
    }

    async fn ensure_identity(
        &self,
        relative_path: &str,
        expected: Option<&str>,
    ) -> Result<(), RemoteSyncError> {
        let started_at = Instant::now();
        let actual = self.head_identity(relative_path).await?;
        if actual.as_deref() == expected {
            return Ok(());
        }
        Err(s3_replan_required(
            &self.diagnostic_context,
            SyncProviderOperation::Metadata,
            "HEAD",
            relative_path,
            None,
            started_at.elapsed(),
        ))
    }

    async fn list_page(
        &self,
        continuation_token: Option<&str>,
    ) -> Result<ListObjectsPage, RemoteSyncError> {
        let mut url = s3_bucket_url(&self.connection)?;
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("list-type", "2");
            query.append_pair("prefix", &self.prefix);
            if let Some(token) = continuation_token {
                query.append_pair("continuation-token", token);
            }
        }
        let response = self
            .send_get_bytes_with_retry(
                &self.client,
                &url,
                SyncProviderOperation::List,
                &self.prefix,
            )
            .await?;
        let status = response.status.as_u16();
        let body = std::str::from_utf8(&response.body).map_err(|_| {
            s3_integrity_failure(
                &self.diagnostic_context,
                SyncProviderOperation::List,
                "GET",
                &self.prefix,
                "s3-list-response-invalid",
                response.started_at.elapsed(),
            )
        })?;
        let page = parse_list_objects_v2(&body, &self.prefix).map_err(|_| {
            s3_integrity_failure(
                &self.diagnostic_context,
                SyncProviderOperation::List,
                "GET",
                &self.prefix,
                "s3-list-response-invalid",
                response.started_at.elapsed(),
            )
        })?;
        record_s3_request_succeeded(
            &self.diagnostic_context,
            "GET",
            SyncProviderOperation::List,
            &self.prefix,
            status,
            response.started_at.elapsed(),
        );
        Ok(page)
    }

    async fn list_notebook_page(
        &self,
        continuation_token: Option<&str>,
    ) -> Result<ListNotebookPrefixesPage, RemoteSyncError> {
        let mut url = s3_bucket_url(&self.connection)?;
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("list-type", "2");
            query.append_pair("prefix", &self.prefix);
            query.append_pair("delimiter", "/");
            if let Some(token) = continuation_token {
                query.append_pair("continuation-token", token);
            }
        }
        let response = self
            .send_get_bytes_with_retry(
                &self.connection_test_client,
                &url,
                SyncProviderOperation::Catalog,
                &self.prefix,
            )
            .await?;
        let status = response.status.as_u16();
        let body = std::str::from_utf8(&response.body).map_err(|_| {
            s3_integrity_failure(
                &self.diagnostic_context,
                SyncProviderOperation::Catalog,
                "GET",
                &self.prefix,
                "s3-catalog-response-invalid",
                response.started_at.elapsed(),
            )
        })?;
        let page = parse_list_notebook_prefixes(&body, &self.prefix).map_err(|_| {
            s3_integrity_failure(
                &self.diagnostic_context,
                SyncProviderOperation::Catalog,
                "GET",
                &self.prefix,
                "s3-catalog-response-invalid",
                response.started_at.elapsed(),
            )
        })?;
        record_s3_request_succeeded(
            &self.diagnostic_context,
            "GET",
            SyncProviderOperation::Catalog,
            &self.prefix,
            status,
            response.started_at.elapsed(),
        );
        Ok(page)
    }

    pub(crate) async fn list_notebook_names(&self) -> Result<Vec<String>, String> {
        let mut names = BTreeSet::new();
        let mut continuation_token = None;
        let mut seen_continuation_tokens = BTreeSet::new();
        loop {
            let page = self
                .list_notebook_page(continuation_token.as_deref())
                .await?;
            names.extend(page.names);
            let Some(next_token) = page.next_continuation_token else {
                break;
            };
            if next_token.is_empty() {
                return Err("S3 catalog returned an invalid continuation token".to_string());
            }
            if !seen_continuation_tokens.insert(next_token.clone()) {
                return Err("S3 catalog returned a repeated continuation token".to_string());
            }
            continuation_token = Some(next_token);
        }
        Ok(names.into_iter().collect())
    }

    pub(crate) async fn test_connection(&self) -> Result<String, String> {
        let checked_target = self.prefix.trim_end_matches('/');
        let mut url =
            s3_bucket_url(&self.connection).map_err(|_| s3_connection_test_transport_error())?;
        url.set_query(Some(&connection_test_query(checked_target)?));
        let (response, started_at) = self
            .send_with_retry(
                &self.connection_test_client,
                Method::GET,
                &url,
                S3Payload::Empty,
                None,
                None,
                None,
                SyncProviderOperation::Catalog,
                &self.prefix,
            )
            .await
            .map_err(|_| s3_connection_test_transport_error())?;
        if response.status().as_u16() == 200 {
            record_s3_request_succeeded(
                &self.diagnostic_context,
                "GET",
                SyncProviderOperation::Catalog,
                &self.prefix,
                200,
                started_at.elapsed(),
            );
            return Ok(checked_target.to_string());
        }
        let error = s3_http_failure(
            &self.diagnostic_context,
            SyncProviderOperation::Catalog,
            "GET",
            &self.prefix,
            response,
            started_at.elapsed(),
        )
        .await;
        Err(s3_connection_test_error(&error))
    }
}

fn is_retryable_s3_status(status: StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 429 | 500 | 502 | 503 | 504)
}

#[cfg(test)]
fn s3_retry_delay(_attempt: u8) -> Duration {
    Duration::ZERO
}

#[cfg(not(test))]
fn s3_retry_delay(attempt: u8) -> Duration {
    let exponential_ms = 150_u64.saturating_mul(1_u64 << u32::from(attempt.saturating_sub(1)));
    let jitter_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_millis() as u64
        % 75;
    Duration::from_millis(exponential_ms.saturating_add(jitter_ms))
}

async fn wait_before_s3_retry(attempt: u8) {
    let delay = s3_retry_delay(attempt);
    if !delay.is_zero() {
        tokio::time::sleep(delay).await;
    }
}

fn object_segments(prefix_segments: &[String], relative_path: &str) -> Result<Vec<String>, String> {
    validate_relative_path(relative_path)?;
    Ok(prefix_segments
        .iter()
        .cloned()
        .chain(relative_path.split('/').map(ToString::to_string))
        .collect())
}

#[cfg(test)]
pub(super) fn object_key_for_test(
    remote_path: &str,
    relative_path: &str,
) -> Result<String, String> {
    object_segments(&normalize_prefix_segments(remote_path)?, relative_path)
        .map(|segments| segments.join("/"))
}

fn connection_test_query(remote_path: &str) -> Result<String, String> {
    let prefix = format!("{}/", normalize_prefix_segments(remote_path)?.join("/"));
    let mut url = Url::parse("http://connection.test").map_err(|_| {
        "project-connection-test-failed: s3 GET <prefix>: request failed".to_string()
    })?;
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("list-type", "2");
        query.append_pair("max-keys", "1");
        query.append_pair("prefix", &prefix);
    }
    url.query().map(ToString::to_string).ok_or_else(|| {
        "project-connection-test-failed: s3 GET <prefix>: request failed".to_string()
    })
}

fn s3_connection_test_error(error: &RemoteSyncError) -> String {
    let Some(diagnostic) = error.details() else {
        return s3_connection_test_transport_error();
    };
    let mut details = Vec::new();
    if let Some(status) = diagnostic.http_status {
        details.push(format!("HTTP {status}"));
    }
    if let Some(code) = diagnostic.provider_error_code.as_deref() {
        details.push(code.to_string());
    }
    if let Some(request_id) = diagnostic.request_id.as_deref() {
        details.push(format!("request {request_id}"));
    }
    if details.is_empty() {
        s3_connection_test_transport_error()
    } else {
        format!(
            "project-connection-test-failed: S3 GET failed ({}).",
            details.join(", ")
        )
    }
}

fn s3_connection_test_transport_error() -> String {
    "project-connection-test-failed: S3 GET request failed.".to_string()
}

impl RemoteSyncBackend for S3Backend {
    fn target_fingerprint_source(&self) -> String {
        format!(
            "s3|{}|{}|{}|{}|{}",
            self.connection.endpoint_url,
            self.connection.region,
            self.connection.bucket,
            self.connection.addressing_style.as_str(),
            self.prefix
        )
    }

    async fn list_files(&self) -> Result<BTreeMap<String, RemoteSyncFile>, RemoteSyncError> {
        let mut files = BTreeMap::new();
        let mut continuation_token = None;
        loop {
            let page = self.list_page(continuation_token.as_deref()).await?;
            files.extend(page.files);
            let Some(next_token) = page.next_continuation_token else {
                break;
            };
            if next_token.is_empty() {
                return Err("S3 listing returned an empty continuation token".into());
            }
            continuation_token = Some(next_token);
        }
        Ok(files)
    }

    async fn download(
        &self,
        relative_path: &str,
        expected_identity: &str,
    ) -> Result<Vec<u8>, RemoteSyncError> {
        self.ensure_identity(relative_path, Some(expected_identity))
            .await?;
        let url = self.object_url(relative_path)?;
        let response = self
            .send_get_bytes_with_retry(
                &self.client,
                &url,
                SyncProviderOperation::Download,
                relative_path,
            )
            .await?;
        let response_identity = object_identity(
            response
                .headers
                .get(ETAG)
                .and_then(|value| value.to_str().ok()),
            response
                .headers
                .get(LAST_MODIFIED)
                .and_then(|value| value.to_str().ok()),
            response
                .headers
                .get(CONTENT_LENGTH)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0),
        );
        let logical_empty = response
            .headers
            .get(S3_LOGICAL_EMPTY_HEADER)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value == "1");
        if response_identity != expected_identity {
            return Err(s3_replan_required(
                &self.diagnostic_context,
                SyncProviderOperation::Download,
                "GET",
                relative_path,
                None,
                response.started_at.elapsed(),
            ));
        }
        let status = response.status.as_u16();
        let bytes = response.body;
        let bytes = if logical_empty {
            if bytes.as_slice() != S3_LOGICAL_EMPTY_SENTINEL {
                return Err(s3_integrity_failure(
                    &self.diagnostic_context,
                    SyncProviderOperation::Download,
                    "GET",
                    relative_path,
                    "s3-logical-empty-object-invalid",
                    response.started_at.elapsed(),
                ));
            }
            Vec::new()
        } else {
            bytes
        };
        record_s3_request_succeeded(
            &self.diagnostic_context,
            "GET",
            SyncProviderOperation::Download,
            relative_path,
            status,
            response.started_at.elapsed(),
        );
        Ok(bytes)
    }

    async fn upload(
        &self,
        relative_path: &str,
        bytes: &[u8],
        expected_identity: Option<&str>,
    ) -> Result<String, RemoteSyncError> {
        self.ensure_identity(relative_path, expected_identity)
            .await?;
        let url = self.object_url(relative_path)?;
        let precondition = s3_mutation_precondition(expected_identity).map_err(|_| {
            s3_integrity_failure(
                &self.diagnostic_context,
                SyncProviderOperation::Upload,
                "PUT",
                relative_path,
                "s3-object-identity-unsupported",
                Duration::ZERO,
            )
        })?;
        let (payload, body) = if bytes.is_empty() {
            (S3Payload::LogicalEmpty, S3_LOGICAL_EMPTY_SENTINEL.to_vec())
        } else {
            (S3Payload::Bytes(bytes), bytes.to_vec())
        };
        let (response, started_at) = self
            .send_with_retry(
                &self.client,
                Method::PUT,
                &url,
                payload,
                Some("application/octet-stream"),
                Some(&body),
                Some(&precondition),
                SyncProviderOperation::Upload,
                relative_path,
            )
            .await?;
        if matches!(response.status().as_u16(), 409 | 412) {
            return Err(s3_replan_required(
                &self.diagnostic_context,
                SyncProviderOperation::Upload,
                "PUT",
                relative_path,
                Some(response.status().as_u16()),
                started_at.elapsed(),
            ));
        }
        if !response.status().is_success() {
            return Err(s3_http_failure(
                &self.diagnostic_context,
                SyncProviderOperation::Upload,
                "PUT",
                relative_path,
                response,
                started_at.elapsed(),
            )
            .await);
        }
        let uploaded_etag = response
            .headers()
            .get(ETAG)
            .and_then(|value| value.to_str().ok())
            .map(normalize_etag)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .ok_or_else(|| {
                s3_integrity_failure(
                    &self.diagnostic_context,
                    SyncProviderOperation::Upload,
                    "PUT",
                    relative_path,
                    "s3-upload-verification-failed",
                    started_at.elapsed(),
                )
            })?;
        let status = response.status().as_u16();
        record_s3_request_succeeded(
            &self.diagnostic_context,
            "PUT",
            SyncProviderOperation::Upload,
            relative_path,
            status,
            started_at.elapsed(),
        );
        let verified_identity = self.head_identity(relative_path).await?.ok_or_else(|| {
            s3_integrity_failure(
                &self.diagnostic_context,
                SyncProviderOperation::Upload,
                "PUT",
                relative_path,
                "s3-upload-verification-failed",
                started_at.elapsed(),
            )
        })?;
        if identity_etag(&verified_identity) != Some(uploaded_etag.as_str()) {
            return Err(s3_replan_required(
                &self.diagnostic_context,
                SyncProviderOperation::Upload,
                "HEAD",
                relative_path,
                None,
                started_at.elapsed(),
            ));
        }
        Ok(verified_identity)
    }

    async fn delete(
        &self,
        relative_path: &str,
        expected_identity: &str,
    ) -> Result<(), RemoteSyncError> {
        self.ensure_identity(relative_path, Some(expected_identity))
            .await?;
        let url = self.object_url(relative_path)?;
        let precondition = s3_mutation_precondition(Some(expected_identity)).map_err(|_| {
            s3_integrity_failure(
                &self.diagnostic_context,
                SyncProviderOperation::Delete,
                "DELETE",
                relative_path,
                "s3-object-identity-unsupported",
                Duration::ZERO,
            )
        })?;
        let (response, started_at) = self
            .send_with_retry(
                &self.client,
                Method::DELETE,
                &url,
                S3Payload::Empty,
                None,
                None,
                Some(&precondition),
                SyncProviderOperation::Delete,
                relative_path,
            )
            .await?;
        if matches!(response.status().as_u16(), 409 | 412) {
            return Err(s3_replan_required(
                &self.diagnostic_context,
                SyncProviderOperation::Delete,
                "DELETE",
                relative_path,
                Some(response.status().as_u16()),
                started_at.elapsed(),
            ));
        }
        if response.status().is_success() || response.status().as_u16() == 404 {
            record_s3_request_succeeded(
                &self.diagnostic_context,
                "DELETE",
                SyncProviderOperation::Delete,
                relative_path,
                response.status().as_u16(),
                started_at.elapsed(),
            );
            return Ok(());
        }
        Err(s3_http_failure(
            &self.diagnostic_context,
            SyncProviderOperation::Delete,
            "DELETE",
            relative_path,
            response,
            started_at.elapsed(),
        )
        .await)
    }
}

fn normalize_prefix_segments(value: &str) -> Result<Vec<String>, String> {
    let normalized = value.trim().replace('\\', "/");
    let normalized = normalized.trim_matches('/');
    if normalized.is_empty() || normalized == "." {
        return Err("S3 sync prefix cannot be the bucket root".to_string());
    }
    let mut segments = Vec::new();
    for segment in normalized.split('/') {
        let segment = segment.trim();
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." {
            return Err("S3 sync prefix cannot contain parent directory segments".to_string());
        }
        segments.push(segment.to_string());
    }
    if segments.is_empty() {
        return Err("S3 sync prefix cannot be the bucket root".to_string());
    }
    Ok(segments)
}

fn validated_prefix_segments(value: &str) -> Result<Vec<String>, String> {
    if value.is_empty() || value.starts_with('/') || value.ends_with('/') || value.contains('\\') {
        return Err("S3 sync prefix is invalid".to_string());
    }
    let segments = value
        .split('/')
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if segments
        .iter()
        .any(|segment| segment.is_empty() || matches!(segment.as_str(), "." | ".."))
    {
        return Err("S3 sync prefix is invalid".to_string());
    }
    Ok(segments)
}

fn parse_list_objects_v2(body: &str, prefix: &str) -> Result<ListObjectsPage, String> {
    let mut reader = Reader::from_str(body);
    reader.config_mut().trim_text(true);
    let mut page = ListObjectsPage::default();
    let mut current_entry: Option<ListObjectEntry> = None;
    let mut current_field = String::new();
    let mut is_truncated = false;

    loop {
        match reader.read_event().map_err(|error| error.to_string())? {
            Event::Start(element) => {
                let name = String::from_utf8_lossy(element.local_name().as_ref()).into_owned();
                if name == "Contents" {
                    current_entry = Some(ListObjectEntry::default());
                }
                current_field = name;
            }
            Event::Text(text) => {
                let decoded = text
                    .decode()
                    .map_err(|error| error.to_string())?
                    .into_owned();
                let value = unescape(&decoded)
                    .map_err(|error| error.to_string())?
                    .into_owned();
                if let Some(entry) = current_entry.as_mut() {
                    match current_field.as_str() {
                        "Key" => entry.key = value,
                        "LastModified" => entry.last_modified = Some(value),
                        "ETag" => entry.etag = Some(value),
                        "Size" => entry.size = value.parse().unwrap_or(0),
                        _ => {}
                    }
                } else {
                    match current_field.as_str() {
                        "IsTruncated" => is_truncated = value.eq_ignore_ascii_case("true"),
                        "NextContinuationToken" => page.next_continuation_token = Some(value),
                        _ => {}
                    }
                }
            }
            Event::End(element) => {
                let name = String::from_utf8_lossy(element.local_name().as_ref()).into_owned();
                if name == "Contents" {
                    if let Some(entry) = current_entry.take() {
                        add_list_entry(&mut page.files, entry, prefix)?;
                    }
                }
                current_field.clear();
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !is_truncated {
        page.next_continuation_token = None;
    } else if page.next_continuation_token.is_none() {
        return Err("S3 listing is truncated without a continuation token".to_string());
    }
    Ok(page)
}

fn parse_list_notebook_prefixes(
    body: &str,
    prefix: &str,
) -> Result<ListNotebookPrefixesPage, String> {
    let mut reader = Reader::from_str(body);
    reader.config_mut().trim_text(true);
    let mut page = ListNotebookPrefixesPage::default();
    let mut current_field = String::new();
    let mut common_prefix = None;
    let mut is_truncated = false;

    loop {
        match reader.read_event().map_err(|error| error.to_string())? {
            Event::Start(element) => {
                let name = String::from_utf8_lossy(element.local_name().as_ref()).into_owned();
                if name == "CommonPrefixes" {
                    common_prefix = Some(String::new());
                }
                current_field = name;
            }
            Event::Text(text) => {
                let decoded = text
                    .decode()
                    .map_err(|error| error.to_string())?
                    .into_owned();
                let value = unescape(&decoded)
                    .map_err(|error| error.to_string())?
                    .into_owned();
                if let Some(common_prefix) =
                    common_prefix.as_mut().filter(|_| current_field == "Prefix")
                {
                    common_prefix.push_str(&value);
                } else {
                    match current_field.as_str() {
                        "IsTruncated" => is_truncated = value.eq_ignore_ascii_case("true"),
                        "NextContinuationToken" => {
                            page.next_continuation_token
                                .get_or_insert_with(String::new)
                                .push_str(&value);
                        }
                        _ => {}
                    }
                }
            }
            Event::GeneralRef(reference) => {
                let reference = reference.decode().map_err(|error| error.to_string())?;
                let encoded = format!("&{reference};");
                let value = unescape(&encoded)
                    .map_err(|error| error.to_string())?
                    .into_owned();
                if let Some(common_prefix) =
                    common_prefix.as_mut().filter(|_| current_field == "Prefix")
                {
                    common_prefix.push_str(&value);
                } else if current_field == "NextContinuationToken" {
                    page.next_continuation_token
                        .get_or_insert_with(String::new)
                        .push_str(&value);
                }
            }
            Event::End(element) => {
                let name = String::from_utf8_lossy(element.local_name().as_ref()).into_owned();
                if name == "CommonPrefixes" {
                    if let Some(common_prefix) = common_prefix.take() {
                        add_notebook_common_prefix(&mut page.names, &common_prefix, prefix);
                    }
                }
                current_field.clear();
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !is_truncated {
        page.next_continuation_token = None;
    } else if page.next_continuation_token.is_none() {
        return Err("S3 catalog is truncated without a continuation token".to_string());
    }
    Ok(page)
}

fn add_notebook_common_prefix(names: &mut BTreeSet<String>, value: &str, prefix: &str) {
    let Some(relative) = value
        .strip_prefix(prefix)
        .and_then(|value| value.strip_suffix('/'))
    else {
        return;
    };
    if relative.contains('/') {
        return;
    }
    if let Ok(name) = crate::notebook_scope::validate_notebook_name(relative) {
        names.insert(name);
    }
}

fn add_list_entry(
    files: &mut BTreeMap<String, RemoteSyncFile>,
    entry: ListObjectEntry,
    prefix: &str,
) -> Result<(), String> {
    if !entry.key.starts_with(prefix) {
        return Err("S3 listing returned an object outside the sync prefix".to_string());
    }
    let relative_path = &entry.key[prefix.len()..];
    if relative_path.is_empty() || relative_path.ends_with('/') {
        return Ok(());
    }
    validate_relative_path(relative_path)?;
    if is_protected_sync_relative_path(relative_path) {
        return Ok(());
    }
    if relative_path.split('/').any(|segment| segment == ".git") {
        return Ok(());
    }
    files.insert(
        relative_path.to_string(),
        RemoteSyncFile {
            identity: object_identity(
                entry.etag.as_deref(),
                entry.last_modified.as_deref(),
                entry.size,
            ),
            size: entry.size,
        },
    );
    Ok(())
}

fn object_identity(etag: Option<&str>, last_modified: Option<&str>, size: u64) -> String {
    let etag = etag.map(normalize_etag).filter(|value| !value.is_empty());
    let modified = last_modified
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match (etag, modified) {
        (Some(etag), _) => format!("etag:{etag};len:{size}"),
        (None, Some(modified)) => format!("modified:{modified};len:{size}"),
        (None, None) => format!("len:{size}"),
    }
}

fn identity_etag(identity: &str) -> Option<&str> {
    let value = identity.strip_prefix("etag:")?;
    let (etag, length) = value.rsplit_once(";len:")?;
    if etag.is_empty()
        || length.parse::<u64>().is_err()
        || !etag
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return None;
    }
    Some(etag)
}

fn s3_mutation_precondition(expected_identity: Option<&str>) -> Result<HeaderMap, String> {
    let mut headers = HeaderMap::new();
    let Some(expected_identity) = expected_identity else {
        headers.insert(IF_NONE_MATCH, HeaderValue::from_static("*"));
        return Ok(headers);
    };
    let etag = identity_etag(expected_identity)
        .ok_or_else(|| "S3 object identity does not contain a conditional ETag".to_string())?;
    let value = HeaderValue::from_str(&format!("\"{etag}\""))
        .map_err(|_| "S3 object ETag cannot be used as a request precondition".to_string())?;
    headers.insert(IF_MATCH, value);
    Ok(headers)
}

fn normalize_etag(value: &str) -> &str {
    value
        .trim()
        .strip_prefix("W/")
        .unwrap_or(value.trim())
        .trim()
        .trim_matches('"')
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    use super::{
        connection_test_query, normalize_prefix_segments, object_identity,
        parse_list_notebook_prefixes, parse_list_objects_v2, s3_client_policy, S3Backend,
        S3SyncSettings, S3TransportOptions,
    };
    use crate::remote_sync::backend::RemoteSyncBackend;
    use crate::remote_sync::diagnostics::SyncDiagnosticContext;
    use crate::sync_config::model::{S3AddressingStyle, S3TlsVerification};

    enum S3FixtureStep {
        Disconnect,
        Respond(String),
    }

    fn spawn_s3_fixture(
        response: String,
    ) -> (String, Arc<Mutex<Vec<String>>>, thread::JoinHandle<()>) {
        spawn_s3_responses_fixture(vec![response])
    }

    fn spawn_s3_responses_fixture(
        responses: Vec<String>,
    ) -> (String, Arc<Mutex<Vec<String>>>, thread::JoinHandle<()>) {
        spawn_s3_steps_fixture(responses.into_iter().map(S3FixtureStep::Respond).collect())
    }

    fn spawn_s3_steps_fixture(
        steps: Vec<S3FixtureStep>,
    ) -> (String, Arc<Mutex<Vec<String>>>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("fixture should bind");
        listener
            .set_nonblocking(true)
            .expect("fixture should be nonblocking");
        let address = listener.local_addr().expect("fixture address");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&requests);
        let handle = thread::spawn(move || {
            for step in steps {
                let deadline = Instant::now() + Duration::from_secs(3);
                let (mut stream, _) = loop {
                    match listener.accept() {
                        Ok(connection) => break connection,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            if Instant::now() >= deadline {
                                panic!("fixture timed out waiting for the next expected request");
                            }
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("fixture request failed: {error}"),
                    }
                };
                stream
                    .set_nonblocking(false)
                    .expect("fixture connection should use blocking reads");
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .expect("fixture connection should bound request reads");
                stream
                    .set_write_timeout(Some(Duration::from_secs(3)))
                    .expect("fixture connection should bound response writes");
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
                if let S3FixtureStep::Respond(response) = step {
                    stream
                        .write_all(response.as_bytes())
                        .expect("fixture should respond");
                }
            }
        });

        (format!("http://{address}"), requests, handle)
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

    const LIST_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
  <IsTruncated>true</IsTruncated>
  <Contents>
    <Key>notes/personal/daily/note.md</Key>
    <LastModified>2026-07-16T06:00:00.000Z</LastModified>
    <ETag>&quot;etag-1&quot;</ETag>
    <Size>12</Size>
  </Contents>
  <Contents><Key>notes/personal/folder/</Key><Size>0</Size></Contents>
  <NextContinuationToken>next token</NextContinuationToken>
</ListBucketResult>"#;

    #[test]
    fn upload_403_returns_typed_access_denied_without_object_path() {
        let body = "<Error><Code>AccessDenied</Code><Message>private</Message></Error>";
        let response = format!(
            "HTTP/1.1 403 Forbidden\r\nx-amz-request-id: request-403\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let (endpoint_url, requests, handle) = spawn_s3_responses_fixture(vec![
            "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string(),
            response,
        ]);
        let backend = S3Backend::new(S3SyncSettings {
            access_key_id: "access-key".into(),
            bucket: "bucket".into(),
            endpoint_url,
            region: "us-east-1".into(),
            remote_path: "root/notes/personal".into(),
            secret_access_key: "secret-key".into(),
        })
        .unwrap()
        .with_diagnostic_context(SyncDiagnosticContext::new("run-403", "notes"));

        let error =
            tauri::async_runtime::block_on(backend.upload("private/面试.md", b"body", None))
                .expect_err("upload must fail");
        handle.join().unwrap();

        let diagnostic = error.details().expect("typed S3 failure");
        assert_eq!(diagnostic.code, "s3-upload-http-failed");
        assert_eq!(diagnostic.http_status, Some(403));
        assert_eq!(
            diagnostic.provider_error_code.as_deref(),
            Some("AccessDenied")
        );
        assert_eq!(diagnostic.request_id.as_deref(), Some("request-403"));
        assert_eq!(diagnostic.method.as_deref(), Some("PUT"));
        assert_eq!(diagnostic.run_id, "run-403");
        assert_eq!(diagnostic.scope, "notes");
        let requests = requests.lock().unwrap();
        let puts = requests
            .iter()
            .filter(|request| request.starts_with("PUT "))
            .collect::<Vec<_>>();
        assert_eq!(puts.len(), 1);
        assert!(puts[0].to_ascii_lowercase().contains("if-none-match: *"));
        assert!(!diagnostic.object_id.as_deref().unwrap().contains("private"));
        assert!(!error.to_string().contains("面试"));
    }

    #[test]
    fn upload_retries_a_transient_response_and_verifies_the_final_object() {
        let responses = vec![
            "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .to_string(),
            "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .to_string(),
            "HTTP/1.1 200 OK\r\nETag: \"uploaded-etag\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .to_string(),
            "HTTP/1.1 200 OK\r\nETag: \"uploaded-etag\"\r\nContent-Length: 4\r\nConnection: close\r\n\r\n"
                .to_string(),
        ];
        let (endpoint_url, requests, handle) = spawn_s3_responses_fixture(responses);
        let backend = S3Backend::new(S3SyncSettings {
            access_key_id: "access-key".into(),
            bucket: "bucket".into(),
            endpoint_url,
            region: "us-east-1".into(),
            remote_path: "root/notes/personal".into(),
            secret_access_key: "secret-key".into(),
        })
        .unwrap();

        let identity = tauri::async_runtime::block_on(backend.upload("note.md", b"body", None))
            .expect("transient PUT should recover within the request retry budget");
        handle.join().unwrap();

        assert_eq!(identity, "etag:uploaded-etag;len:4");
        let requests = requests.lock().unwrap();
        let puts = requests
            .iter()
            .filter(|request| request.starts_with("PUT "))
            .collect::<Vec<_>>();
        assert_eq!(puts.len(), 2);
        assert!(puts
            .iter()
            .all(|request| request.to_ascii_lowercase().contains("if-none-match: *")));
    }

    #[test]
    fn upload_retries_a_transport_disconnect_and_verifies_the_final_object() {
        let steps = vec![
            S3FixtureStep::Respond(
                "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    .to_string(),
            ),
            S3FixtureStep::Disconnect,
            S3FixtureStep::Respond(
                "HTTP/1.1 200 OK\r\nETag: \"uploaded-etag\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    .to_string(),
            ),
            S3FixtureStep::Respond(
                "HTTP/1.1 200 OK\r\nETag: \"uploaded-etag\"\r\nContent-Length: 4\r\nConnection: close\r\n\r\n"
                    .to_string(),
            ),
        ];
        let (endpoint_url, requests, handle) = spawn_s3_steps_fixture(steps);
        let backend = S3Backend::new(S3SyncSettings {
            access_key_id: "access-key".into(),
            bucket: "bucket".into(),
            endpoint_url,
            region: "us-east-1".into(),
            remote_path: "root/notes/personal".into(),
            secret_access_key: "secret-key".into(),
        })
        .unwrap();

        let identity = tauri::async_runtime::block_on(backend.upload("note.md", b"body", None))
            .expect("a disconnected PUT should recover within the request retry budget");
        handle.join().unwrap();

        assert_eq!(identity, "etag:uploaded-etag;len:4");
        let requests = requests.lock().unwrap();
        let puts = requests
            .iter()
            .filter(|request| request.starts_with("PUT "))
            .collect::<Vec<_>>();
        assert_eq!(puts.len(), 2);
        assert!(puts
            .iter()
            .all(|request| request.to_ascii_lowercase().contains("if-none-match: *")));
    }

    #[test]
    fn ambiguous_create_retry_requests_a_replan_instead_of_overwriting() {
        let responses = vec![
            "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string(),
            "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .to_string(),
            "HTTP/1.1 412 Precondition Failed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .to_string(),
        ];
        let (endpoint_url, requests, handle) = spawn_s3_responses_fixture(responses);
        let backend = S3Backend::new(S3SyncSettings {
            access_key_id: "access-key".into(),
            bucket: "bucket".into(),
            endpoint_url,
            region: "us-east-1".into(),
            remote_path: "root/notes/personal".into(),
            secret_access_key: "secret-key".into(),
        })
        .unwrap();

        let error = tauri::async_runtime::block_on(backend.upload("note.md", b"body", None))
            .expect_err("a retry that observes a committed create must request a fresh plan");
        handle.join().unwrap();

        assert_eq!(error.safe_code(), "s3-object-changed");
        let requests = requests.lock().unwrap();
        let puts = requests
            .iter()
            .filter(|request| request.starts_with("PUT "))
            .collect::<Vec<_>>();
        assert_eq!(puts.len(), 2);
        assert!(puts
            .iter()
            .all(|request| request.to_ascii_lowercase().contains("if-none-match: *")));
    }

    #[test]
    fn ambiguous_disconnected_create_requests_a_replan_instead_of_overwriting() {
        let steps = vec![
            S3FixtureStep::Respond(
                "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    .to_string(),
            ),
            S3FixtureStep::Disconnect,
            S3FixtureStep::Respond(
                "HTTP/1.1 412 Precondition Failed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    .to_string(),
            ),
        ];
        let (endpoint_url, requests, handle) = spawn_s3_steps_fixture(steps);
        let backend = S3Backend::new(S3SyncSettings {
            access_key_id: "access-key".into(),
            bucket: "bucket".into(),
            endpoint_url,
            region: "us-east-1".into(),
            remote_path: "root/notes/personal".into(),
            secret_access_key: "secret-key".into(),
        })
        .unwrap();

        let error = tauri::async_runtime::block_on(backend.upload("note.md", b"body", None))
            .expect_err("a disconnected create that committed must request a fresh plan");
        handle.join().unwrap();

        assert_eq!(error.safe_code(), "s3-object-changed");
        let requests = requests.lock().unwrap();
        let puts = requests
            .iter()
            .filter(|request| request.starts_with("PUT "))
            .collect::<Vec<_>>();
        assert_eq!(puts.len(), 2);
        assert!(puts
            .iter()
            .all(|request| request.to_ascii_lowercase().contains("if-none-match: *")));
    }

    #[test]
    fn upload_precondition_failure_requests_a_replan_without_retrying() {
        let responses = vec![
            "HTTP/1.1 200 OK\r\nETag: \"old-etag\"\r\nContent-Length: 3\r\nConnection: close\r\n\r\n"
                .to_string(),
            "HTTP/1.1 412 Precondition Failed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .to_string(),
        ];
        let (endpoint_url, requests, handle) = spawn_s3_responses_fixture(responses);
        let backend = S3Backend::new(S3SyncSettings {
            access_key_id: "access-key".into(),
            bucket: "bucket".into(),
            endpoint_url,
            region: "us-east-1".into(),
            remote_path: "root/notes/personal".into(),
            secret_access_key: "secret-key".into(),
        })
        .unwrap();

        let error = tauri::async_runtime::block_on(backend.upload(
            "note.md",
            b"new",
            Some("etag:old-etag;len:3"),
        ))
        .expect_err("a failed atomic precondition must request a fresh sync plan");
        handle.join().unwrap();

        assert_eq!(error.safe_code(), "s3-object-changed");
        let requests = requests.lock().unwrap();
        let puts = requests
            .iter()
            .filter(|request| request.starts_with("PUT "))
            .collect::<Vec<_>>();
        assert_eq!(puts.len(), 1);
        assert!(puts[0]
            .to_ascii_lowercase()
            .contains("if-match: \"old-etag\""));
    }

    #[test]
    fn zero_length_upload_encodes_logical_empty_with_metadata() {
        let responses = vec![
            "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .to_string(),
            "HTTP/1.1 200 OK\r\nETag: \"empty-etag\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .to_string(),
            "HTTP/1.1 200 OK\r\nETag: \"empty-etag\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .to_string(),
        ];
        let (endpoint_url, requests, handle) = spawn_s3_responses_fixture(responses);
        let backend = S3Backend::new(S3SyncSettings {
            access_key_id: "access-key".into(),
            bucket: "bucket".into(),
            endpoint_url,
            region: "us-east-1".into(),
            remote_path: "root/notes/personal".into(),
            secret_access_key: "secret-key".into(),
        })
        .unwrap();

        tauri::async_runtime::block_on(backend.upload("empty.md", b"", None))
            .expect("zero-length upload should succeed");
        handle.join().unwrap();

        let requests = requests.lock().unwrap();
        let put = requests[1].to_ascii_lowercase();
        assert!(put.starts_with("put "));
        assert!(put.contains("\r\ncontent-length: 1\r\n"), "{put}");
        assert!(
            put.contains("\r\nx-amz-meta-qingyu-logical-empty: 1\r\n"),
            "{put}"
        );
        assert!(!put.contains("\r\ntransfer-encoding:"), "{put}");
    }

    #[test]
    fn logical_empty_download_decodes_the_signed_sentinel() {
        let responses = vec![
            "HTTP/1.1 200 OK\r\nETag: \"empty-etag\"\r\nContent-Length: 1\r\nConnection: close\r\n\r\n\0"
                .to_string(),
            "HTTP/1.1 200 OK\r\nETag: \"empty-etag\"\r\nContent-Length: 1\r\nx-amz-meta-qingyu-logical-empty: 1\r\nConnection: close\r\n\r\n\0"
                .to_string(),
        ];
        let (endpoint_url, _, handle) = spawn_s3_responses_fixture(responses);
        let backend = S3Backend::new(S3SyncSettings {
            access_key_id: "access-key".into(),
            bucket: "bucket".into(),
            endpoint_url,
            region: "us-east-1".into(),
            remote_path: "root/notes/personal".into(),
            secret_access_key: "secret-key".into(),
        })
        .unwrap();

        let bytes =
            tauri::async_runtime::block_on(backend.download("empty.md", "etag:empty-etag;len:1"))
                .expect("logical empty download should succeed");
        handle.join().unwrap();

        assert!(bytes.is_empty());
    }

    #[test]
    fn list_500_preserves_safe_provider_diagnostics() {
        let body = "<Error><Code>InternalError</Code><Message>secret</Message></Error>";
        let response = format!(
            "HTTP/1.1 500 Internal Server Error\r\nx-amz-request-id: list-request-500\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let (endpoint_url, requests, handle) =
            spawn_s3_responses_fixture(vec![response.clone(), response.clone(), response]);
        let backend = S3Backend::new(S3SyncSettings {
            access_key_id: "access-key".into(),
            bucket: "bucket".into(),
            endpoint_url,
            region: "us-east-1".into(),
            remote_path: "root/notes/personal".into(),
            secret_access_key: "secret-key".into(),
        })
        .unwrap()
        .with_diagnostic_context(SyncDiagnosticContext::new("run-500", "notes"));

        let error =
            tauri::async_runtime::block_on(backend.list_files()).expect_err("listing must fail");
        handle.join().unwrap();

        let diagnostic = error.details().expect("typed S3 failure");
        assert_eq!(diagnostic.code, "s3-list-http-failed");
        assert_eq!(diagnostic.http_status, Some(500));
        assert_eq!(
            diagnostic.provider_error_code.as_deref(),
            Some("InternalError")
        );
        assert_eq!(diagnostic.request_id.as_deref(), Some("list-request-500"));
        assert_eq!(requests.lock().unwrap().len(), 3);
        assert!(!error.to_string().contains("root/notes"));
        assert!(!error.to_string().contains("secret"));
    }

    #[test]
    fn list_retries_when_a_success_response_body_is_truncated() {
        let valid_body = "<ListBucketResult><IsTruncated>false</IsTruncated></ListBucketResult>";
        let responses = vec![
            "HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\nshort".to_string(),
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{valid_body}",
                valid_body.len()
            ),
        ];
        let (endpoint_url, requests, handle) = spawn_s3_responses_fixture(responses);
        let backend = S3Backend::new(S3SyncSettings {
            access_key_id: "access-key".into(),
            bucket: "bucket".into(),
            endpoint_url,
            region: "us-east-1".into(),
            remote_path: "root/notes/personal".into(),
            secret_access_key: "secret-key".into(),
        })
        .unwrap();

        let files = tauri::async_runtime::block_on(backend.list_files())
            .expect("a truncated success body should be fetched again");
        handle.join().unwrap();

        assert!(files.is_empty());
        assert_eq!(
            requests
                .lock()
                .unwrap()
                .iter()
                .filter(|request| request.starts_with("GET "))
                .count(),
            2
        );
    }

    #[test]
    fn download_retries_when_a_success_response_body_is_truncated() {
        let responses = vec![
            "HTTP/1.1 200 OK\r\nETag: \"etag-1\"\r\nContent-Length: 4\r\nConnection: close\r\n\r\n"
                .to_string(),
            "HTTP/1.1 200 OK\r\nETag: \"etag-1\"\r\nContent-Length: 12\r\nConnection: close\r\n\r\nshort"
                .to_string(),
            "HTTP/1.1 200 OK\r\nETag: \"etag-1\"\r\nContent-Length: 4\r\nConnection: close\r\n\r\nbody"
                .to_string(),
        ];
        let (endpoint_url, requests, handle) = spawn_s3_responses_fixture(responses);
        let backend = S3Backend::new(S3SyncSettings {
            access_key_id: "access-key".into(),
            bucket: "bucket".into(),
            endpoint_url,
            region: "us-east-1".into(),
            remote_path: "root/notes/personal".into(),
            secret_access_key: "secret-key".into(),
        })
        .unwrap();

        let bytes =
            tauri::async_runtime::block_on(backend.download("note.md", "etag:etag-1;len:4"))
                .expect("a truncated object body should be downloaded again");
        handle.join().unwrap();

        assert_eq!(bytes, b"body");
        assert_eq!(
            requests
                .lock()
                .unwrap()
                .iter()
                .filter(|request| request.starts_with("GET "))
                .count(),
            2
        );
    }

    #[test]
    fn delete_404_remains_a_success() {
        let responses = vec![
            "HTTP/1.1 200 OK\r\nETag: \"etag-1\"\r\nContent-Length: 4\r\nConnection: close\r\n\r\n"
                .to_string(),
            "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string(),
        ];
        let (endpoint_url, requests, handle) = spawn_s3_responses_fixture(responses);
        let backend = S3Backend::new(S3SyncSettings {
            access_key_id: "access-key".into(),
            bucket: "bucket".into(),
            endpoint_url,
            region: "us-east-1".into(),
            remote_path: "root/notes/personal".into(),
            secret_access_key: "secret-key".into(),
        })
        .unwrap();

        tauri::async_runtime::block_on(backend.delete("note.md", "etag:etag-1;len:4"))
            .expect("delete 404 should be idempotent");
        handle.join().unwrap();

        let requests = requests.lock().unwrap();
        let delete = requests
            .iter()
            .find(|request| request.starts_with("DELETE "))
            .expect("delete request");
        assert!(delete.to_ascii_lowercase().contains("if-match: \"etag-1\""));
    }

    #[test]
    fn delete_precondition_failure_requests_a_replan_without_retrying() {
        let responses = vec![
            "HTTP/1.1 200 OK\r\nETag: \"etag-1\"\r\nContent-Length: 4\r\nConnection: close\r\n\r\n"
                .to_string(),
            "HTTP/1.1 412 Precondition Failed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .to_string(),
        ];
        let (endpoint_url, requests, handle) = spawn_s3_responses_fixture(responses);
        let backend = S3Backend::new(S3SyncSettings {
            access_key_id: "access-key".into(),
            bucket: "bucket".into(),
            endpoint_url,
            region: "us-east-1".into(),
            remote_path: "root/notes/personal".into(),
            secret_access_key: "secret-key".into(),
        })
        .unwrap();

        let error = tauri::async_runtime::block_on(backend.delete("note.md", "etag:etag-1;len:4"))
            .expect_err("a stale delete must request a fresh sync plan");
        handle.join().unwrap();

        assert_eq!(error.safe_code(), "s3-object-changed");
        let requests = requests.lock().unwrap();
        let deletes = requests
            .iter()
            .filter(|request| request.starts_with("DELETE "))
            .collect::<Vec<_>>();
        assert_eq!(deletes.len(), 1);
        assert!(deletes[0]
            .to_ascii_lowercase()
            .contains("if-match: \"etag-1\""));
    }

    #[test]
    fn parses_only_direct_common_prefixes_for_the_notebook_catalog() {
        let body = r#"<?xml version="1.0" encoding="UTF-8"?>
<ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
  <IsTruncated>true</IsTruncated>
  <Contents><Key>root/notes/root.md</Key><Size>12</Size></Contents>
  <Contents><Key>root/notes/Phantom/nested.md</Key><Size>12</Size></Contents>
  <CommonPrefixes><Prefix>root/notes/Alpha/</Prefix></CommonPrefixes>
  <CommonPrefixes><Prefix>root/notes/R&amp;D/</Prefix></CommonPrefixes>
  <CommonPrefixes><Prefix>root/notes/  个人 笔记  /</Prefix></CommonPrefixes>
  <CommonPrefixes><Prefix>root/notes/.QINGYU/</Prefix></CommonPrefixes>
  <CommonPrefixes><Prefix>root/notes/.MARKRA-SYNC/</Prefix></CommonPrefixes>
  <CommonPrefixes><Prefix>root/notes/.markra-sync-stage-123/</Prefix></CommonPrefixes>
  <CommonPrefixes><Prefix>root/notes/Alpha/nested/</Prefix></CommonPrefixes>
  <CommonPrefixes><Prefix>root/notes/.markra-sync/</Prefix></CommonPrefixes>
  <CommonPrefixes><Prefix>outside/notes/Outside/</Prefix></CommonPrefixes>
  <NextContinuationToken>next token</NextContinuationToken>
</ListBucketResult>"#;

        let page =
            parse_list_notebook_prefixes(body, "root/notes/").expect("catalog page should parse");

        assert_eq!(
            page.names.into_iter().collect::<Vec<_>>(),
            vec!["  个人 笔记  ", "Alpha", "R&D"]
        );
        assert_eq!(page.next_continuation_token.as_deref(), Some("next token"));
    }

    #[test]
    fn notebook_catalog_uses_delimited_paginated_get_requests_only() {
        let first_body = r#"<?xml version="1.0"?><ListBucketResult>
  <IsTruncated>true</IsTruncated>
  <CommonPrefixes><Prefix>root/notes/Beta/</Prefix></CommonPrefixes>
  <NextContinuationToken>next token</NextContinuationToken>
</ListBucketResult>"#;
        let second_body = r#"<?xml version="1.0"?><ListBucketResult>
  <IsTruncated>false</IsTruncated>
  <CommonPrefixes><Prefix>root/notes/Alpha/</Prefix></CommonPrefixes>
  <CommonPrefixes><Prefix>root/notes/Beta/</Prefix></CommonPrefixes>
</ListBucketResult>"#;
        let response = |body: &str| {
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
        };
        let (endpoint_url, requests, handle) =
            spawn_s3_responses_fixture(vec![response(first_body), response(second_body)]);
        let backend = S3Backend::new_at_validated_prefix(S3SyncSettings {
            access_key_id: "private-access-key".into(),
            bucket: "notes-bucket".into(),
            endpoint_url,
            region: "test-region-1".into(),
            remote_path: "root/notes".into(),
            secret_access_key: "private-secret-key".into(),
        })
        .expect("S3 catalog backend");

        let names = tauri::async_runtime::block_on(backend.list_notebook_names())
            .expect("catalog should paginate");
        handle.join().expect("fixture should finish");

        assert_eq!(names, vec!["Alpha", "Beta"]);
        let requests = requests.lock().expect("fixture request log");
        assert_eq!(requests.len(), 2);
        assert!(requests[0].starts_with("GET /notes-bucket?"));
        assert!(requests[0].contains("list-type=2"));
        assert!(requests[0].contains("prefix=root%2Fnotes%2F"));
        assert!(requests[0].contains("delimiter=%2F"));
        assert!(!requests[0].contains("continuation-token="));
        assert!(requests[1].starts_with("GET /notes-bucket?"));
        assert!(requests[1].contains("continuation-token=next+token"));
        for request in requests.iter() {
            assert!(!request.starts_with("HEAD "));
            assert!(!request.starts_with("PUT "));
            assert!(!request.starts_with("DELETE "));
        }
    }

    #[test]
    fn notebook_catalog_rejects_any_repeated_continuation_token() {
        let page = |token: &str| {
            let body = format!(
                r#"<?xml version="1.0"?><ListBucketResult>
  <IsTruncated>true</IsTruncated>
  <NextContinuationToken>{token}</NextContinuationToken>
</ListBucketResult>"#
            );
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
        };
        let (endpoint_url, requests, handle) =
            spawn_s3_responses_fixture(vec![page("A"), page("B"), page("A")]);
        let backend = S3Backend::new_at_validated_prefix(S3SyncSettings {
            access_key_id: "private-access-key".into(),
            bucket: "notes-bucket".into(),
            endpoint_url,
            region: "test-region-1".into(),
            remote_path: "root/notes".into(),
            secret_access_key: "private-secret-key".into(),
        })
        .expect("S3 catalog backend");

        let error = tauri::async_runtime::block_on(backend.list_notebook_names())
            .expect_err("a repeated pagination token must fail closed");
        handle.join().expect("fixture should finish");

        assert_eq!(error, "S3 catalog returned a repeated continuation token");
        let requests = requests.lock().expect("fixture request log");
        assert_eq!(requests.len(), 3);
        assert!(!requests[0].contains("continuation-token="));
        assert!(requests[1].contains("continuation-token=A"));
        assert!(requests[2].contains("continuation-token=B"));
    }

    #[test]
    fn parses_list_objects_page_and_strips_the_sync_prefix() {
        let page = parse_list_objects_v2(LIST_XML, "notes/personal/").expect("list page");
        assert_eq!(page.files["daily/note.md"].size, 12);
        assert_eq!(page.next_continuation_token.as_deref(), Some("next token"));
        assert!(!page.files.contains_key("folder/"));
        assert_eq!(page.files["daily/note.md"].identity, "etag:etag-1;len:12");
    }

    #[test]
    fn rejects_unsafe_or_outside_list_keys() {
        let unsafe_xml = LIST_XML.replace("daily/note.md", "../note.md");
        assert!(parse_list_objects_v2(&unsafe_xml, "notes/personal/").is_err());
        let outside_xml = LIST_XML.replace("notes/personal/daily/note.md", "other/note.md");
        assert!(parse_list_objects_v2(&outside_xml, "notes/personal/").is_err());
    }

    #[test]
    fn omits_qingyu_and_legacy_control_objects_from_listings() {
        let xml = LIST_XML.replace(
            "</ListBucketResult>",
            r#"  <Contents><Key>notes/personal/.qingyu/sync/status.json</Key><Size>9</Size></Contents>
  <Contents><Key>notes/personal/folder/.markra-sync/manifest.json</Key><Size>11</Size></Contents>
</ListBucketResult>"#,
        );

        let page = parse_list_objects_v2(&xml, "notes/personal/").expect("list page");

        assert!(!page.files.contains_key(".qingyu/sync/status.json"));
        assert!(!page.files.contains_key("folder/.markra-sync/manifest.json"));
        assert!(page.files.contains_key("daily/note.md"));
    }

    #[test]
    fn s3_objects_reject_invalid_descendants_before_protected_filtering() {
        for protected_path in [
            ".qingyu/../outside.md",
            "folder/.markra-sync/child//invalid.md",
            ".qingyu/folder\\invalid.md",
        ] {
            let xml = LIST_XML.replace("daily/note.md", protected_path);
            assert!(
                parse_list_objects_v2(&xml, "notes/personal/").is_err(),
                "{protected_path}"
            );
        }

        for unsafe_path in [
            "ordinary/../outside.md",
            "ordinary/child//invalid.md",
            "ordinary/folder\\invalid.md",
        ] {
            let xml = LIST_XML.replace("daily/note.md", unsafe_path);
            assert!(
                parse_list_objects_v2(&xml, "notes/personal/").is_err(),
                "{unsafe_path}"
            );
        }
    }

    #[test]
    fn validates_prefix_and_fallback_identity() {
        assert_eq!(
            normalize_prefix_segments(" /notes/personal/ ").unwrap(),
            vec!["notes", "personal"]
        );
        assert!(normalize_prefix_segments("/").is_err());
        assert!(normalize_prefix_segments("notes/../other").is_err());
        assert_eq!(
            object_identity(None, Some("2026-07-16T06:00:00Z"), 12),
            "modified:2026-07-16T06:00:00Z;len:12"
        );
    }

    #[test]
    fn s3_target_and_state_identity_ignore_signing_credentials_rotation() {
        let backend = |access_key_id: &str, secret_access_key: &str| {
            S3Backend::new(S3SyncSettings {
                access_key_id: access_key_id.to_string(),
                bucket: "notes-bucket".to_string(),
                endpoint_url: "https://s3.example.test".to_string(),
                region: "test-region-1".to_string(),
                remote_path: "root/notes/Personal".to_string(),
                secret_access_key: secret_access_key.to_string(),
            })
            .expect("S3 backend")
        };
        let original = backend("first-access", "first-secret");
        let rotated = backend("second-access", "second-secret");
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
    fn addressing_changes_target_identity_but_timeout_and_tls_policy_do_not() {
        let backend = |options| {
            S3Backend::new_with_transport(
                S3SyncSettings {
                    access_key_id: "access".into(),
                    bucket: "notes-bucket".into(),
                    endpoint_url: "https://s3.example.test".into(),
                    region: "test-region-1".into(),
                    remote_path: "root/notes/Personal".into(),
                    secret_access_key: "secret".into(),
                },
                options,
            )
            .unwrap()
        };
        let original = backend(S3TransportOptions::default());
        let operational_change = backend(S3TransportOptions {
            request_timeout_seconds: 120,
            tls_verification: S3TlsVerification::Skip,
            ..S3TransportOptions::default()
        });
        let addressing_change = backend(S3TransportOptions {
            addressing_style: S3AddressingStyle::VirtualHosted,
            ..S3TransportOptions::default()
        });

        assert_eq!(
            original.target_fingerprint_source(),
            operational_change.target_fingerprint_source()
        );
        assert_ne!(
            original.target_fingerprint_source(),
            addressing_change.target_fingerprint_source()
        );
    }

    #[test]
    fn client_policy_defaults_to_verification_and_can_explicitly_skip_it() {
        let default_policy = s3_client_policy(S3TransportOptions::default());
        assert_eq!(default_policy.timeout, Duration::from_secs(60));
        assert!(!default_policy.accept_invalid_certificates);

        let custom_policy = s3_client_policy(S3TransportOptions {
            request_timeout_seconds: 299,
            tls_verification: S3TlsVerification::Skip,
            ..S3TransportOptions::default()
        });
        assert_eq!(custom_policy.timeout, Duration::from_secs(299));
        assert!(custom_policy.accept_invalid_certificates);
    }

    #[test]
    fn keeps_etag_identity_stable_across_list_and_head_date_formats() {
        assert_eq!(
            object_identity(Some("\"etag-1\""), Some("2026-07-16T06:00:00.000Z"), 12),
            object_identity(
                Some("\"etag-1\""),
                Some("Thu, 16 Jul 2026 06:00:00 GMT"),
                12
            )
        );
    }

    #[test]
    fn download_rejects_a_get_response_with_a_different_s3_identity() {
        let replacement_body = "secret replacement body";
        let list_body = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
  <IsTruncated>false</IsTruncated>
  <Contents>
    <Key>notes/personal/draft.md</Key>
    <ETag>&quot;listed-etag&quot;</ETag>
    <Size>{}</Size>
  </Contents>
</ListBucketResult>"#,
            replacement_body.len()
        );
        let responses = vec![
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{list_body}",
                list_body.len()
            ),
            format!(
                "HTTP/1.1 200 OK\r\nETag: \"listed-etag\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                replacement_body.len()
            ),
            format!(
                "HTTP/1.1 200 OK\r\nETag: \"replacement-etag\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{replacement_body}",
                replacement_body.len()
            ),
        ];
        let (endpoint_url, requests, handle) = spawn_s3_responses_fixture(responses);
        let backend = S3Backend::new(S3SyncSettings {
            access_key_id: "private-access-key".to_string(),
            bucket: "notes-bucket".to_string(),
            endpoint_url: endpoint_url.clone(),
            region: "test-region-1".to_string(),
            remote_path: "notes/personal".to_string(),
            secret_access_key: "private-secret-key".to_string(),
        })
        .expect("S3 backend");

        let error = tauri::async_runtime::block_on(async {
            let listed = backend.list_files().await.expect("S3 listing");
            backend
                .download("draft.md", &listed["draft.md"].identity)
                .await
                .expect_err("replacement between HEAD and GET must fail closed")
        });
        handle.join().expect("fixture should finish");

        assert_eq!(error.safe_code(), "s3-object-changed");
        for forbidden in [
            endpoint_url.as_str(),
            "private-access-key",
            "private-secret-key",
            "draft.md",
            replacement_body,
        ] {
            assert!(!error.contains(forbidden), "exposed {forbidden}");
        }
        let requests = requests.lock().expect("fixture request log");
        assert_eq!(requests.len(), 3);
        assert!(requests[0].starts_with("GET /notes-bucket?"));
        assert!(requests[1].starts_with("HEAD /notes-bucket/notes/personal/draft.md HTTP/1.1"));
        assert!(requests[2].starts_with("GET /notes-bucket/notes/personal/draft.md HTTP/1.1"));
    }

    #[test]
    fn connection_test_lists_at_most_one_s3_object() {
        assert_eq!(
            connection_test_query("notes/personal").expect("connection query"),
            "list-type=2&max-keys=1&prefix=notes%2Fpersonal%2F"
        );
    }

    #[test]
    fn connection_test_s3_is_signed_get_and_keeps_remote_fixture_unchanged() {
        let (endpoint_url, requests, handle) = spawn_s3_fixture(
            "HTTP/1.1 200 OK\r\nContent-Length: 62\r\nConnection: close\r\n\r\n<ListBucketResult><IsTruncated>false</IsTruncated></ListBucketResult>".to_string(),
        );
        let remote_fixture = Arc::new(Mutex::new(BTreeMap::from([
            ("notes/personal/a.md".to_string(), b"alpha".to_vec()),
            ("notes/personal/b.md".to_string(), b"beta".to_vec()),
        ])));
        let before = remote_fixture.lock().expect("remote fixture").clone();
        let backend = S3Backend::new(S3SyncSettings {
            access_key_id: "private-access-key".to_string(),
            bucket: "notes-bucket".to_string(),
            endpoint_url,
            region: "test-region-1".to_string(),
            remote_path: "notes/personal".to_string(),
            secret_access_key: "private-secret-key".to_string(),
        })
        .expect("S3 connection backend");

        let checked_target = tauri::async_runtime::block_on(backend.test_connection())
            .expect("read-only S3 probe should pass");
        handle.join().expect("fixture should finish");

        assert_eq!(checked_target, "notes/personal");
        assert_eq!(*remote_fixture.lock().expect("remote fixture"), before);
        let requests = requests.lock().expect("fixture request log");
        assert_eq!(requests.len(), 1);
        assert!(requests[0].starts_with(
            "GET /notes-bucket?list-type=2&max-keys=1&prefix=notes%2Fpersonal%2F HTTP/1.1\r\n"
        ));
        assert!(requests[0]
            .to_ascii_lowercase()
            .contains("authorization: aws4-hmac-sha256"));
        assert!(!requests[0].starts_with("HEAD "));
        assert!(!requests[0].starts_with("PUT "));
        assert!(!requests[0].starts_with("DELETE "));
    }

    #[test]
    fn connection_test_s3_error_excludes_credentials_signed_query_and_response_body() {
        let (endpoint_url, _requests, handle) = spawn_s3_fixture(
            "HTTP/1.1 403 Forbidden\r\nContent-Length: 20\r\nConnection: close\r\n\r\nsecret response body".to_string(),
        );
        let backend = S3Backend::new(S3SyncSettings {
            access_key_id: "private-access-key".to_string(),
            bucket: "notes-bucket".to_string(),
            endpoint_url: endpoint_url.clone(),
            region: "test-region-1".to_string(),
            remote_path: "notes/personal".to_string(),
            secret_access_key: "private-secret-key".to_string(),
        })
        .expect("S3 connection backend");

        let error = tauri::async_runtime::block_on(backend.test_connection())
            .expect_err("forbidden S3 probe should fail safely");
        handle.join().expect("fixture should finish");

        assert!(error.to_ascii_lowercase().contains("s3"));
        assert!(error.contains("GET"));
        assert!(error.contains("HTTP 403"));
        for forbidden in [
            "private-access-key",
            "private-secret-key",
            endpoint_url.as_str(),
            "list-type=2",
            "max-keys=1",
            "notes/personal",
            "secret response body",
            "Authorization",
        ] {
            assert!(!error.contains(forbidden), "exposed {forbidden}");
        }
    }

    #[test]
    fn connection_test_s3_rejects_redirects_without_visiting_location() {
        for (status, reason) in [(301, "Moved Permanently"), (307, "Temporary Redirect")] {
            let (redirect_target, redirected_requests, redirect_handle) =
                spawn_redirect_target_fixture();
            let location = format!("{redirect_target}/outside?token=location-secret");
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\nLocation: {location}\r\nContent-Length: 20\r\nConnection: close\r\n\r\nredirect-secret-body"
            );
            let (endpoint_url, original_requests, source_handle) = spawn_s3_fixture(response);
            let backend = S3Backend::new(S3SyncSettings {
                access_key_id: "private-access-key".to_string(),
                bucket: "notes-bucket".to_string(),
                endpoint_url: endpoint_url.clone(),
                region: "test-region-1".to_string(),
                remote_path: "notes/personal".to_string(),
                secret_access_key: "private-secret-key".to_string(),
            })
            .expect("S3 connection backend");

            let result = tauri::async_runtime::block_on(backend.test_connection());
            source_handle.join().expect("redirect source should finish");
            redirect_handle
                .join()
                .expect("redirect target should finish");

            let error = result.expect_err("S3 redirect must be a safe status error");
            assert_eq!(
                error,
                format!("project-connection-test-failed: S3 GET failed (HTTP {status}).")
            );
            let original_requests = original_requests.lock().expect("source request log");
            assert_eq!(original_requests.len(), 1);
            assert!(original_requests[0].starts_with(
                "GET /notes-bucket?list-type=2&max-keys=1&prefix=notes%2Fpersonal%2F HTTP/1.1\r\n"
            ));
            assert!(original_requests[0]
                .to_ascii_lowercase()
                .contains("authorization: aws4-hmac-sha256"));
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
                endpoint_url.as_str(),
                "location-secret",
                "redirect-secret-body",
                "private-access-key",
                "private-secret-key",
                "list-type=2",
                "max-keys=1",
            ] {
                assert!(
                    !error.contains(forbidden),
                    "HTTP {status} exposed {forbidden}"
                );
            }
        }
    }
}
