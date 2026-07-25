use std::collections::HashSet;
use std::error::Error;
use std::fmt;
use std::net::ToSocketAddrs;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
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
const CATALOG_ROOT_PREFIX: &str = "qingyu/repositories";

pub(crate) struct S3CatalogDirectoryListing {
    pub prefixes: Vec<String>,
    pub invalid_direct_object_count: usize,
}

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
    now: Arc<dyn Fn() -> OffsetDateTime + Send + Sync>,
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
        Self::new_with_clock(
            connection,
            options,
            repository_prefix,
            Arc::new(OffsetDateTime::now_utc),
        )
    }

    fn new_with_clock(
        connection: S3Connection,
        options: S3TransportOptions,
        repository_prefix: &str,
        now: Arc<dyn Fn() -> OffsetDateTime + Send + Sync>,
    ) -> Result<Self, CloudError> {
        validate_repository_prefix(repository_prefix)?;
        Self::new_with_validated_prefix_and_resolver(
            connection,
            options,
            repository_prefix,
            now,
            Arc::new(SystemDnsResolver),
        )
    }

    pub(crate) fn new_catalog_transport(
        connection: S3Connection,
        options: S3TransportOptions,
    ) -> Result<Self, CloudError> {
        Self::new_with_validated_prefix_and_resolver(
            connection,
            options,
            CATALOG_ROOT_PREFIX,
            Arc::new(OffsetDateTime::now_utc),
            Arc::new(SystemDnsResolver),
        )
    }

    #[cfg(test)]
    fn new_with_resolver<Resolver>(
        connection: S3Connection,
        options: S3TransportOptions,
        repository_prefix: &str,
        resolver: Arc<Resolver>,
    ) -> Result<Self, CloudError>
    where
        Resolver: Resolve + 'static,
    {
        validate_repository_prefix(repository_prefix)?;
        Self::new_with_validated_prefix_and_resolver(
            connection,
            options,
            repository_prefix,
            Arc::new(OffsetDateTime::now_utc),
            resolver,
        )
    }

    fn new_with_validated_prefix_and_resolver<Resolver>(
        connection: S3Connection,
        options: S3TransportOptions,
        repository_prefix: &str,
        now: Arc<dyn Fn() -> OffsetDateTime + Send + Sync>,
        resolver: Arc<Resolver>,
    ) -> Result<Self, CloudError>
    where
        Resolver: Resolve + 'static,
    {
        if options.request_timeout.is_zero() || !(1..=3).contains(&options.max_attempts) {
            return Err(CloudError::backend("s3_invalid_transport_options"));
        }
        let client = reqwest::Client::builder()
            .timeout(options.request_timeout)
            .redirect(reqwest::redirect::Policy::none())
            .dns_resolver(Arc::new(DnsTaggingResolver::new(resolver)))
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
            now,
        })
    }

    fn object_url(&self, key: &str) -> Result<Url, CloudError> {
        self.object_url_with_validation(key, false)
    }

    fn object_url_with_validation(
        &self,
        key: &str,
        allow_trailing_slash: bool,
    ) -> Result<Url, CloudError> {
        validate_relative(key, allow_trailing_slash)?;
        self.connection
            .object_url(&format!("{}/{key}", self.repository_prefix))
    }

    fn object_get_url(&self, key: &str) -> Result<Url, CloudError> {
        let mut url = self.object_url(key)?;
        url.query_pairs_mut()
            .append_pair("response-cache-control", "no-cache");
        Ok(url)
    }

    fn list_url(
        &self,
        prefix: &str,
        continuation: Option<&str>,
        delimiter: Option<&str>,
    ) -> Result<Url, CloudError> {
        validate_relative(prefix, true)?;
        let full_prefix = format!("{}/{}", self.repository_prefix, prefix);
        let mut url = self.connection.bucket_url()?;
        {
            let mut query = url.query_pairs_mut();
            if let Some(continuation) = continuation {
                query.append_pair("continuation-token", continuation);
            }
            if let Some(delimiter) = delimiter {
                query.append_pair("delimiter", delimiter);
            }
            query
                .append_pair("list-type", "2")
                .append_pair("max-keys", "1000")
                .append_pair("prefix", &full_prefix);
        }
        Ok(url)
    }

    pub(crate) async fn list_catalog_directories(
        &self,
    ) -> Result<S3CatalogDirectoryListing, CloudError> {
        if self.repository_prefix != CATALOG_ROOT_PREFIX {
            return Err(CloudError::UnsafeKey);
        }
        let full_prefix = format!("{}/", self.repository_prefix);
        let mut continuation: Option<String> = None;
        let mut seen_continuations = HashSet::new();
        let mut prefixes = Vec::new();
        let mut invalid_direct_object_count = 0_usize;
        loop {
            let url = self.list_url("", continuation.as_deref(), Some("/"))?;
            let response = self.send_empty_with_retry(Method::GET, &url).await?;
            let page =
                parse_list_page(response, &self.repository_prefix, &full_prefix, false).await?;
            prefixes.extend(page.common_prefixes);
            invalid_direct_object_count = invalid_direct_object_count
                .checked_add(page.objects.len())
                .ok_or_else(|| CloudError::backend("catalog_issue_count_overflow"))?;
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
        Ok(S3CatalogDirectoryListing {
            prefixes,
            invalid_direct_object_count,
        })
    }

    pub(crate) async fn list_catalog_repository_objects(
        &self,
        repository_id: &str,
    ) -> Result<Vec<CloudObject>, CloudError> {
        self.validate_catalog_repository(repository_id)?;
        self.list_objects(&format!("{repository_id}/"), true).await
    }

    pub(crate) async fn remove_catalog_repository_object(
        &self,
        repository_id: &str,
        key: &str,
    ) -> Result<(), CloudError> {
        self.validate_catalog_repository(repository_id)?;
        let repository_prefix = format!("{repository_id}/");
        if !key.starts_with(&repository_prefix) {
            return Err(CloudError::UnsafeKey);
        }
        let url = self.object_url_with_validation(key, true)?;
        self.send_empty_with_retry(Method::DELETE, &url).await?;
        Ok(())
    }

    fn validate_catalog_repository(&self, repository_id: &str) -> Result<(), CloudError> {
        if self.repository_prefix != CATALOG_ROOT_PREFIX {
            return Err(CloudError::UnsafeKey);
        }
        let parsed = uuid::Uuid::parse_str(repository_id).map_err(|_| CloudError::UnsafeKey)?;
        if parsed.to_string() != repository_id {
            return Err(CloudError::UnsafeKey);
        }
        Ok(())
    }

    async fn list_objects(
        &self,
        prefix: &str,
        allow_object_trailing_slash: bool,
    ) -> Result<Vec<CloudObject>, CloudError> {
        validate_relative(prefix, true)?;
        let full_prefix = format!("{}/{}", self.repository_prefix, prefix);
        let mut continuation: Option<String> = None;
        let mut seen_continuations = HashSet::new();
        let mut objects = Vec::new();
        loop {
            let url = self.list_url(prefix, continuation.as_deref(), None)?;
            let response = self.send_empty_with_retry(Method::GET, &url).await?;
            let page = parse_list_page(
                response,
                &self.repository_prefix,
                &full_prefix,
                allow_object_trailing_slash,
            )
            .await?;
            if !page.common_prefixes.is_empty() {
                return Err(CloudError::UnsafeKey);
            }
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

    fn signed_empty_request(
        &self,
        method: Method,
        url: Url,
    ) -> Result<reqwest::RequestBuilder, CloudError> {
        let headers = self.signer.sign_empty_at(&method, &url, (self.now)())?;
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
            (self.now)(),
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
            (self.now)(),
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

struct DnsTaggingResolver {
    inner: Arc<dyn Resolve>,
}

struct SystemDnsResolver;

impl Resolve for SystemDnsResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_owned();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                (host.as_str(), 0)
                    .to_socket_addrs()
                    .map(|addresses| Box::new(addresses) as Addrs)
                    .map_err(|error| Box::new(error) as Box<dyn Error + Send + Sync>)
            })
            .await
            .map_err(|error| Box::new(error) as Box<dyn Error + Send + Sync>)?
        })
    }
}

impl DnsTaggingResolver {
    fn new<Resolver>(inner: Arc<Resolver>) -> Self
    where
        Resolver: Resolve + 'static,
    {
        Self { inner }
    }
}

impl Resolve for DnsTaggingResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let resolving = self.inner.resolve(name);
        Box::pin(async move {
            resolving.await.map_err(|source| {
                Box::new(DnsResolutionError { source }) as Box<dyn Error + Send + Sync>
            })
        })
    }
}

#[derive(Debug)]
struct DnsResolutionError {
    source: Box<dyn Error + Send + Sync>,
}

impl fmt::Display for DnsResolutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DNS resolution failed")
    }
}

impl Error for DnsResolutionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.source.as_ref())
    }
}

#[async_trait::async_trait]
impl Cloud for S3Cloud {
    async fn get_bounded(&self, key: &str, max_bytes: u64) -> Result<Vec<u8>, CloudError> {
        let url = self.object_get_url(key)?;
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
        let url = self.object_get_url(key)?;
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
        self.list_objects(prefix, false).await
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

fn validate_repository_prefix(repository_prefix: &str) -> Result<(), CloudError> {
    validate_relative(repository_prefix, false)?;
    let mut segments = repository_prefix.split('/');
    match (
        segments.next(),
        segments.next(),
        segments.next(),
        segments.next(),
        segments.next(),
    ) {
        (Some("qingyu"), Some("repositories"), Some(repository_id), Some("repo"), None)
            if !repository_id.is_empty() =>
        {
            let parsed = uuid::Uuid::parse_str(repository_id).map_err(|_| CloudError::UnsafeKey)?;
            if parsed.to_string() != repository_id {
                return Err(CloudError::UnsafeKey);
            }
            Ok(())
        }
        _ => Err(CloudError::UnsafeKey),
    }
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
    if error_chain_contains_dns_resolution(&error) {
        CloudError::Dns
    } else {
        CloudError::Io(std::io::Error::other(error))
    }
}

fn error_chain_contains_dns_resolution(error: &(dyn Error + 'static)) -> bool {
    let mut current = Some(error);
    while let Some(error) = current {
        if error.downcast_ref::<DnsResolutionError>().is_some() {
            return true;
        }
        current = error.source();
    }
    false
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

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::fmt;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use reqwest::dns::{Name, Resolve, Resolving};
    use reqwest::header::AUTHORIZATION;
    use tokio::net::{TcpListener, TcpStream};

    use super::*;
    use crate::cloud::S3AddressingStyle;

    fn connection() -> S3Connection {
        S3Connection::new(
            "https://s3.example.test",
            "us-east-1",
            "qingyu-notes",
            "test-key",
            "test-secret",
            S3AddressingStyle::Path,
        )
        .expect("valid test S3 connection")
    }

    fn options() -> S3TransportOptions {
        S3TransportOptions {
            request_timeout: Duration::from_secs(1),
            tls_verification: S3TlsVerification::Verify,
            max_attempts: 3,
        }
    }

    #[derive(Debug)]
    struct ResolverFixtureError;

    impl fmt::Display for ResolverFixtureError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("fixture resolver failure")
        }
    }

    impl Error for ResolverFixtureError {}

    struct AlwaysFailResolver;

    impl Resolve for AlwaysFailResolver {
        fn resolve(&self, _name: Name) -> Resolving {
            Box::pin(async { Err(Box::new(ResolverFixtureError) as Box<dyn Error + Send + Sync>) })
        }
    }

    #[tokio::test]
    async fn resolver_chain_failure_is_classified_as_typed_dns_before_redaction() {
        let connection = S3Connection::new(
            "http://unresolved.example.test",
            "us-east-1",
            "qingyu-notes",
            "test-key",
            "test-secret",
            S3AddressingStyle::Path,
        )
        .unwrap();
        let cloud = S3Cloud::new_with_resolver(
            connection,
            S3TransportOptions {
                max_attempts: 1,
                ..options()
            },
            "qingyu/repositories/00000000-0000-4000-8000-000000000001/repo",
            Arc::new(AlwaysFailResolver),
        )
        .unwrap();

        assert!(matches!(
            cloud.get_bounded("refs/latest", 42).await,
            Err(CloudError::Dns)
        ));
    }

    #[tokio::test]
    async fn generic_connect_failure_is_not_misclassified_as_dns() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let connection = S3Connection::new(
            &format!("http://{address}"),
            "us-east-1",
            "qingyu-notes",
            "test-key",
            "test-secret",
            S3AddressingStyle::Path,
        )
        .unwrap();
        let cloud = S3Cloud::new(
            connection,
            S3TransportOptions {
                max_attempts: 1,
                ..options()
            },
            "qingyu/repositories/00000000-0000-4000-8000-000000000001/repo",
        )
        .unwrap();

        let error = cloud.get_bounded("refs/latest", 42).await.unwrap_err();
        assert!(!matches!(error, CloudError::Dns));
    }

    #[derive(Debug)]
    struct CapturedRequest {
        target: String,
        amz_date: String,
        authorization: String,
    }

    async fn capture_request(stream: &mut TcpStream) -> CapturedRequest {
        let mut bytes = Vec::new();
        loop {
            let mut chunk = [0_u8; 1024];
            let read = stream.read(&mut chunk).await.expect("read request head");
            assert_ne!(read, 0, "connection closed before request head completed");
            bytes.extend_from_slice(&chunk[..read]);
            assert!(
                bytes.len() <= 64 * 1024,
                "request head exceeded fixture limit"
            );
            if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }

        let head_end = bytes
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .expect("complete request head");
        let head = std::str::from_utf8(&bytes[..head_end]).expect("UTF-8 request head");
        let mut lines = head.split("\r\n");
        let target = lines
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .expect("request target")
            .to_owned();
        let mut amz_date = None;
        let mut authorization = None;
        for line in lines {
            let (name, value) = line.split_once(':').expect("valid request header");
            if name.eq_ignore_ascii_case("x-amz-date") {
                amz_date = Some(value.trim().to_owned());
            } else if name.eq_ignore_ascii_case("authorization") {
                authorization = Some(value.trim().to_owned());
            }
        }

        CapturedRequest {
            target,
            amz_date: amz_date.expect("x-amz-date header"),
            authorization: authorization.expect("authorization header"),
        }
    }

    #[tokio::test]
    async fn retry_loop_signs_each_attempt_with_a_fresh_clock() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind HTTP fixture");
        let endpoint = format!(
            "http://{}",
            listener.local_addr().expect("HTTP fixture address")
        );
        let server = tokio::spawn(async move {
            let mut captured = Vec::new();
            for (status, reason, body) in [
                (503, "Service Unavailable", &b""[..]),
                (200, "OK", &b"ok"[..]),
            ] {
                let (mut stream, _) =
                    tokio::time::timeout(Duration::from_secs(2), listener.accept())
                        .await
                        .expect("fixture timed out waiting for request")
                        .expect("accept fixture request");
                captured.push(capture_request(&mut stream).await);
                let response = format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                stream
                    .write_all(response.as_bytes())
                    .await
                    .expect("write response head");
                stream.write_all(body).await.expect("write response body");
                stream.shutdown().await.expect("close fixture response");
            }

            if let Ok(Ok((_unexpected, _))) =
                tokio::time::timeout(Duration::from_millis(150), listener.accept()).await
            {
                panic!("fixture received an unexpected third request");
            }
            captured
        });

        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_clock = Arc::clone(&calls);
        let clock = Arc::new(move || {
            let offset = calls_for_clock.fetch_add(1, Ordering::SeqCst) as i64;
            OffsetDateTime::from_unix_timestamp(1_784_181_600 + offset)
                .expect("fixed test timestamp")
        });
        let cloud = S3Cloud::new_with_clock(
            S3Connection::new(
                &endpoint,
                "us-east-1",
                "qingyu-notes",
                "test-key",
                "test-secret",
                S3AddressingStyle::Path,
            )
            .expect("valid fixture S3 connection"),
            options(),
            "qingyu/repositories/01234567-89ab-4def-8123-456789abcdef/repo",
            clock,
        )
        .expect("valid S3 cloud");
        let result =
            tokio::time::timeout(Duration::from_secs(3), cloud.get_bounded("refs/latest", 2))
                .await
                .expect("S3 GET timed out")
                .expect("retrying S3 GET");
        let captured = server.await.expect("HTTP fixture task");

        assert_eq!(result, b"ok");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(captured.len(), 2);
        let expected_target = "/qingyu-notes/qingyu/repositories/01234567-89ab-4def-8123-456789abcdef/repo/refs/latest?response-cache-control=no-cache";
        assert_eq!(captured[0].target, expected_target);
        assert_eq!(captured[1].target, expected_target);
        assert_ne!(captured[0].amz_date, captured[1].amz_date);
        assert_ne!(captured[0].authorization, captured[1].authorization);
    }

    #[test]
    fn dejavu_response_cache_query_is_part_of_the_get_signature() {
        let now = OffsetDateTime::from_unix_timestamp(1_784_181_600).expect("fixed test timestamp");
        let cloud = S3Cloud::new_with_clock(
            connection(),
            options(),
            "qingyu/repositories/01234567-89ab-4def-8123-456789abcdef/repo",
            Arc::new(move || now),
        )
        .expect("valid S3 cloud");
        let url = cloud
            .object_get_url("refs/latest")
            .expect("valid Dejavu GET URL");
        assert_eq!(url.query(), Some("response-cache-control=no-cache"));
        let with_query = cloud
            .signed_empty_request(Method::GET, url.clone())
            .expect("signed Dejavu GET")
            .build()
            .expect("build Dejavu GET");
        let mut without_query_url = url;
        without_query_url.set_query(None);
        let without_query = cloud
            .signer
            .sign_empty_at(&Method::GET, &without_query_url, now)
            .expect("sign comparison URL");

        assert_ne!(
            with_query.headers()[AUTHORIZATION],
            without_query[AUTHORIZATION]
        );
    }
}
