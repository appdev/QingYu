use std::collections::HashSet;
use std::fmt;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue, CACHE_CONTROL};
use reqwest::{Method, Response, Url};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio_util::io::ReaderStream;

use super::s3_xml::parse_list_page;
use super::{
    Cloud, CloudError, CloudObject, CloudUploadSource, S3Connection, S3RequestSigner,
    S3TlsVerification,
};

const CONTENT_TYPE_BINARY: &str = "application/octet-stream";
const MAX_REQUEST_ID_BYTES: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct S3TransportOptions {
    pub request_timeout: Duration,
    pub tls_verification: S3TlsVerification,
    pub max_attempts: usize,
}

impl Default for S3TransportOptions {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(30),
            tls_verification: S3TlsVerification::Verify,
            max_attempts: 3,
        }
    }
}

pub struct S3Cloud {
    connection: S3Connection,
    signer: S3RequestSigner,
    client: reqwest::Client,
    options: S3TransportOptions,
    repository_prefix: String,
}

impl fmt::Debug for S3Cloud {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("S3Cloud")
            .field("connection", &self.connection)
            .field("options", &self.options)
            .field("repository_prefix", &self.repository_prefix)
            .finish_non_exhaustive()
    }
}

impl S3Cloud {
    pub fn new(
        connection: S3Connection,
        options: S3TransportOptions,
        repository_prefix: &str,
    ) -> Result<Self, CloudError> {
        validate_relative(repository_prefix, false)?;
        if options.request_timeout.is_zero() || !(1..=3).contains(&options.max_attempts) {
            return Err(CloudError::backend("s3_invalid_transport_options"));
        }
        let client = reqwest::Client::builder()
            .timeout(options.request_timeout)
            .redirect(reqwest::redirect::Policy::none())
            .danger_accept_invalid_certs(matches!(
                options.tls_verification,
                S3TlsVerification::Skip
            ))
            .build()
            .map_err(|_| CloudError::backend("s3_client_build_failed"))?;
        let signer = S3RequestSigner::new(connection.clone());
        Ok(Self {
            connection,
            signer,
            client,
            options,
            repository_prefix: repository_prefix.to_string(),
        })
    }

    fn object_url(&self, key: &str) -> Result<Url, CloudError> {
        validate_relative(key, false)?;
        self.connection
            .object_url(&format!("{}/{key}", self.repository_prefix))
    }

    fn list_url(&self, prefix: &str, continuation: Option<&str>) -> Result<Url, CloudError> {
        validate_relative(prefix, true)?;
        let full_prefix = format!("{}/{}", self.repository_prefix, prefix);
        let mut url = self.connection.bucket_url()?;
        {
            let mut query = url.query_pairs_mut();
            if let Some(continuation) = continuation {
                query.append_pair("continuation-token", continuation);
            }
            query
                .append_pair("list-type", "2")
                .append_pair("max-keys", "1000")
                .append_pair("prefix", &full_prefix);
        }
        Ok(url)
    }

    fn signed_empty_request(
        &self,
        method: Method,
        url: Url,
    ) -> Result<reqwest::RequestBuilder, CloudError> {
        let headers = self
            .signer
            .sign_empty_at(&method, &url, OffsetDateTime::now_utc())?;
        Ok(self.client.request(method, url).headers(headers))
    }

    fn signed_bytes_request(
        &self,
        method: Method,
        url: Url,
        bytes: &[u8],
    ) -> Result<reqwest::RequestBuilder, CloudError> {
        let mut headers = self.signer.sign_bytes_at(
            &method,
            &url,
            bytes,
            Some(CONTENT_TYPE_BINARY),
            OffsetDateTime::now_utc(),
        )?;
        headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        Ok(self
            .client
            .request(method, url)
            .headers(headers)
            .body(bytes.to_vec()))
    }

    fn signed_stream_request(
        &self,
        url: Url,
        payload_hash: &str,
        content_length: u64,
        body: reqwest::Body,
    ) -> Result<reqwest::RequestBuilder, CloudError> {
        let mut headers = self.signer.sign_prehashed_at(
            &Method::PUT,
            &url,
            payload_hash,
            content_length,
            Some(CONTENT_TYPE_BINARY),
            OffsetDateTime::now_utc(),
        )?;
        headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        Ok(self
            .client
            .request(Method::PUT, url)
            .headers(headers)
            .body(body))
    }

    async fn send_empty_with_retry(
        &self,
        method: Method,
        url: &Url,
    ) -> Result<Response, CloudError> {
        for attempt in 0..self.options.max_attempts {
            let response = self
                .signed_empty_request(method.clone(), url.clone())?
                .send()
                .await;
            match response {
                Ok(response) => {
                    if response.status().is_success() {
                        return Ok(response);
                    }
                    let error = response_error(&response);
                    if error.is_retryable() && attempt + 1 < self.options.max_attempts {
                        continue;
                    }
                    return Err(error);
                }
                Err(error) => {
                    if attempt + 1 < self.options.max_attempts && retryable_send_error(&error) {
                        continue;
                    }
                    return Err(map_reqwest_error(error));
                }
            }
        }
        Err(CloudError::Unavailable)
    }

    async fn put_bytes(&self, url: &Url, bytes: &[u8]) -> Result<u64, CloudError> {
        for attempt in 0..self.options.max_attempts {
            let response = self
                .signed_bytes_request(Method::PUT, url.clone(), bytes)?
                .send()
                .await;
            match response {
                Ok(response) => {
                    if response.status().is_success() {
                        return u64::try_from(bytes.len())
                            .map_err(|_| CloudError::backend("payload_length_overflow"));
                    }
                    let error = response_error(&response);
                    if error.is_retryable() && attempt + 1 < self.options.max_attempts {
                        continue;
                    }
                    return Err(error);
                }
                Err(error) => {
                    if attempt + 1 < self.options.max_attempts && retryable_send_error(&error) {
                        continue;
                    }
                    return Err(map_reqwest_error(error));
                }
            }
        }
        Err(CloudError::Unavailable)
    }
}

#[async_trait::async_trait]
impl Cloud for S3Cloud {
    async fn get_bounded(&self, key: &str, max_bytes: u64) -> Result<Vec<u8>, CloudError> {
        let url = self.object_url(key)?;
        for attempt in 0..self.options.max_attempts {
            let response = self
                .signed_empty_request(Method::GET, url.clone())?
                .send()
                .await;
            let mut response = match response {
                Ok(response) if response.status().is_success() => response,
                Ok(response) => {
                    let error = response_error(&response);
                    if error.is_retryable() && attempt + 1 < self.options.max_attempts {
                        continue;
                    }
                    return Err(error);
                }
                Err(error)
                    if attempt + 1 < self.options.max_attempts && retryable_send_error(&error) =>
                {
                    continue;
                }
                Err(error) => return Err(map_reqwest_error(error)),
            };
            if response
                .content_length()
                .is_some_and(|length| length > max_bytes)
            {
                return Err(CloudError::ResponseTooLarge { limit: max_bytes });
            }
            let mut bytes = Vec::with_capacity(
                response
                    .content_length()
                    .unwrap_or_default()
                    .min(max_bytes)
                    .min(64 * 1024) as usize,
            );
            loop {
                match response.chunk().await {
                    Ok(Some(chunk)) => {
                        let remaining = max_bytes.saturating_sub(bytes.len() as u64);
                        if chunk.len() as u64 > remaining {
                            let retain = usize::try_from(remaining.saturating_add(1))
                                .unwrap_or(usize::MAX)
                                .min(chunk.len());
                            bytes.extend_from_slice(&chunk[..retain]);
                            return Err(CloudError::ResponseTooLarge { limit: max_bytes });
                        }
                        bytes.extend_from_slice(&chunk);
                    }
                    Ok(None) => return Ok(bytes),
                    Err(error)
                        if bytes.is_empty()
                            && attempt + 1 < self.options.max_attempts
                            && retryable_send_error(&error) =>
                    {
                        break;
                    }
                    Err(error) => return Err(map_reqwest_error(error)),
                }
            }
        }
        Err(CloudError::Unavailable)
    }

    async fn download_to(
        &self,
        key: &str,
        destination: &mut (dyn AsyncWrite + Unpin + Send),
    ) -> Result<u64, CloudError> {
        let url = self.object_url(key)?;
        for attempt in 0..self.options.max_attempts {
            let response = self
                .signed_empty_request(Method::GET, url.clone())?
                .send()
                .await;
            let mut response = match response {
                Ok(response) if response.status().is_success() => response,
                Ok(response) => {
                    let error = response_error(&response);
                    if error.is_retryable() && attempt + 1 < self.options.max_attempts {
                        continue;
                    }
                    return Err(error);
                }
                Err(error)
                    if attempt + 1 < self.options.max_attempts && retryable_send_error(&error) =>
                {
                    continue;
                }
                Err(error) => return Err(map_reqwest_error(error)),
            };
            let mut written = 0_u64;
            loop {
                match response.chunk().await {
                    Ok(Some(chunk)) => {
                        destination
                            .write_all(&chunk)
                            .await
                            .map_err(CloudError::Io)?;
                        written = written
                            .checked_add(chunk.len() as u64)
                            .ok_or_else(|| CloudError::backend("payload_length_overflow"))?;
                    }
                    Ok(None) => {
                        destination.flush().await.map_err(CloudError::Io)?;
                        return Ok(written);
                    }
                    Err(error)
                        if written == 0
                            && attempt + 1 < self.options.max_attempts
                            && retryable_send_error(&error) =>
                    {
                        break;
                    }
                    Err(error) => return Err(map_reqwest_error(error)),
                }
            }
        }
        Err(CloudError::Unavailable)
    }

    async fn put(&self, key: &str, bytes: &[u8], _overwrite: bool) -> Result<u64, CloudError> {
        let url = self.object_url(key)?;
        self.put_bytes(&url, bytes).await
    }

    async fn upload_from(
        &self,
        key: &str,
        source: &dyn CloudUploadSource,
        _overwrite: bool,
    ) -> Result<u64, CloudError> {
        let url = self.object_url(key)?;
        let expected = source.content_length();
        for attempt in 0..self.options.max_attempts {
            let payload_hash = hash_source(source).await?;
            let reader = source.open()?;
            let mismatch = Arc::new(Mutex::new(None));
            let source_error = Arc::new(Mutex::new(None));
            let checked = LengthCheckedReader::new(
                reader,
                expected,
                Arc::clone(&mismatch),
                Arc::clone(&source_error),
            );
            let body = reqwest::Body::wrap_stream(ReaderStream::new(checked));
            let response = self
                .signed_stream_request(url.clone(), &payload_hash, expected, body)?
                .send()
                .await;
            if let Some(error) = mismatch
                .lock()
                .map_err(|_| CloudError::backend("s3_upload_state_poisoned"))?
                .take()
            {
                return Err(error);
            }
            if let Some(error) = source_error
                .lock()
                .map_err(|_| CloudError::backend("s3_upload_state_poisoned"))?
                .take()
            {
                return Err(CloudError::Io(std::io::Error::new(
                    error.kind,
                    error.message,
                )));
            }
            match response {
                Ok(response) => {
                    if response.status().is_success() {
                        return Ok(expected);
                    }
                    let error = response_error(&response);
                    if error.is_retryable() && attempt + 1 < self.options.max_attempts {
                        continue;
                    }
                    return Err(error);
                }
                Err(error) => {
                    if attempt + 1 < self.options.max_attempts && retryable_send_error(&error) {
                        continue;
                    }
                    return Err(map_reqwest_error(error));
                }
            }
        }
        Err(CloudError::Unavailable)
    }

    async fn remove(&self, key: &str) -> Result<(), CloudError> {
        let url = self.object_url(key)?;
        self.send_empty_with_retry(Method::DELETE, &url).await?;
        Ok(())
    }

    async fn list(&self, prefix: &str) -> Result<Vec<CloudObject>, CloudError> {
        validate_relative(prefix, true)?;
        let full_prefix = format!("{}/{}", self.repository_prefix, prefix);
        let mut continuation: Option<String> = None;
        let mut seen_continuations = HashSet::new();
        let mut objects = Vec::new();
        loop {
            let url = self.list_url(prefix, continuation.as_deref())?;
            let response = self.send_empty_with_retry(Method::GET, &url).await?;
            let page = parse_list_page(response, &self.repository_prefix, &full_prefix).await?;
            objects.extend(page.objects);
            if !page.is_truncated {
                break;
            }
            let next = page
                .next_continuation_token
                .ok_or_else(|| CloudError::backend("s3_list_missing_continuation"))?;
            if next.is_empty() || !seen_continuations.insert(next.clone()) {
                return Err(CloudError::backend("s3_list_stalled_continuation"));
            }
            continuation = Some(next);
        }
        objects.sort_by(|left, right| left.key.cmp(&right.key));
        Ok(objects)
    }

    async fn available_size(&self) -> Result<u64, CloudError> {
        Ok(u64::MAX)
    }
}

async fn hash_source(source: &dyn CloudUploadSource) -> Result<String, CloudError> {
    let expected = source.content_length();
    let mut reader = source.open()?;
    let mut hasher = Sha256::new();
    let mut actual = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    while actual < expected {
        let remaining = expected - actual;
        let limit = buffer
            .len()
            .min(usize::try_from(remaining).unwrap_or(usize::MAX));
        let read = reader
            .read(&mut buffer[..limit])
            .await
            .map_err(CloudError::Io)?;
        if read == 0 {
            return Err(CloudError::LengthMismatch { expected, actual });
        }
        hasher.update(&buffer[..read]);
        actual = actual
            .checked_add(read as u64)
            .ok_or_else(|| CloudError::backend("payload_length_overflow"))?;
    }
    let excess = reader
        .read(&mut buffer[..1])
        .await
        .map_err(CloudError::Io)?;
    if excess != 0 {
        return Err(CloudError::LengthMismatch {
            expected,
            actual: expected.saturating_add(1),
        });
    }
    Ok(hex_lower(&hasher.finalize()))
}

struct LengthCheckedReader {
    inner: Pin<Box<dyn AsyncRead + Send>>,
    expected: u64,
    actual: u64,
    checked_end: bool,
    mismatch: Arc<Mutex<Option<CloudError>>>,
    source_error: Arc<Mutex<Option<SourceIoError>>>,
}

struct SourceIoError {
    kind: std::io::ErrorKind,
    message: String,
}

impl LengthCheckedReader {
    fn new(
        inner: Pin<Box<dyn AsyncRead + Send>>,
        expected: u64,
        mismatch: Arc<Mutex<Option<CloudError>>>,
        source_error: Arc<Mutex<Option<SourceIoError>>>,
    ) -> Self {
        Self {
            inner,
            expected,
            actual: 0,
            checked_end: false,
            mismatch,
            source_error,
        }
    }

    fn fail(&self, actual: u64) -> std::io::Error {
        if let Ok(mut mismatch) = self.mismatch.lock() {
            *mismatch = Some(CloudError::LengthMismatch {
                expected: self.expected,
                actual,
            });
        }
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "upload source length mismatch",
        )
    }

    fn record_source_error(&self, error: &std::io::Error) {
        if let Ok(mut source_error) = self.source_error.lock() {
            *source_error = Some(SourceIoError {
                kind: error.kind(),
                message: error.to_string(),
            });
        }
    }
}

impl AsyncRead for LengthCheckedReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if self.checked_end {
            return Poll::Ready(Ok(()));
        }
        if self.actual < self.expected {
            let remaining = self.expected - self.actual;
            let limit = output
                .remaining()
                .min(usize::try_from(remaining).unwrap_or(usize::MAX));
            if limit == 0 {
                return Poll::Ready(Ok(()));
            }
            let before = output.filled().len();
            let mut limited = ReadBuf::new(output.initialize_unfilled_to(limit));
            match self.inner.as_mut().poll_read(context, &mut limited) {
                Poll::Ready(Ok(())) => {
                    let read = limited.filled().len();
                    if read == 0 {
                        return Poll::Ready(Err(self.fail(self.actual)));
                    }
                    output.advance(read);
                    debug_assert_eq!(output.filled().len(), before + read);
                    self.actual += read as u64;
                    Poll::Ready(Ok(()))
                }
                Poll::Ready(Err(error)) => {
                    self.record_source_error(&error);
                    Poll::Ready(Err(error))
                }
                Poll::Pending => Poll::Pending,
            }
        } else {
            let mut byte = [0_u8; 1];
            let mut probe = ReadBuf::new(&mut byte);
            match self.inner.as_mut().poll_read(context, &mut probe) {
                Poll::Ready(Ok(())) if probe.filled().is_empty() => {
                    self.checked_end = true;
                    Poll::Ready(Ok(()))
                }
                Poll::Ready(Ok(())) => Poll::Ready(Err(self.fail(self.expected.saturating_add(1)))),
                Poll::Ready(Err(error)) => {
                    self.record_source_error(&error);
                    Poll::Ready(Err(error))
                }
                Poll::Pending => Poll::Pending,
            }
        }
    }
}

pub(super) fn validate_relative(
    value: &str,
    allow_empty_or_trailing_slash: bool,
) -> Result<(), CloudError> {
    if value.is_empty() {
        return if allow_empty_or_trailing_slash {
            Ok(())
        } else {
            Err(CloudError::UnsafeKey)
        };
    }
    if value.starts_with('/')
        || value.contains('\\')
        || value.chars().any(char::is_control)
        || (value.ends_with('/') && !allow_empty_or_trailing_slash)
    {
        return Err(CloudError::UnsafeKey);
    }
    let without_trailing = value.strip_suffix('/').unwrap_or(value);
    if without_trailing
        .split('/')
        .any(|segment| segment.is_empty() || matches!(segment, "." | ".."))
    {
        return Err(CloudError::UnsafeKey);
    }
    Ok(())
}

fn response_error(response: &Response) -> CloudError {
    let status = response.status().as_u16();
    match status {
        401 => return CloudError::Auth,
        403 => return CloudError::Forbidden,
        404 => return CloudError::NotFound,
        _ => {}
    }
    CloudError::S3Response {
        status,
        request_id: provider_request_id(response.headers()),
        retryable: matches!(status, 408 | 429 | 500 | 502 | 503 | 504),
    }
}

fn provider_request_id(headers: &HeaderMap) -> Option<String> {
    ["x-amz-request-id", "x-amz-id-2", "x-request-id"]
        .into_iter()
        .filter_map(|name| headers.get(name))
        .filter_map(|value| value.to_str().ok())
        .find(|value| {
            !value.is_empty()
                && value.len() <= MAX_REQUEST_ID_BYTES
                && value.bytes().all(|byte| byte.is_ascii_graphic())
        })
        .map(ToOwned::to_owned)
}

fn retryable_send_error(error: &reqwest::Error) -> bool {
    !error.is_builder() && !error.is_redirect() && error.status().is_none()
}

fn map_reqwest_error(error: reqwest::Error) -> CloudError {
    CloudError::Io(std::io::Error::other(error))
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}
