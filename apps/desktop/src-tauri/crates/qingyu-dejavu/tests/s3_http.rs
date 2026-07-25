use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use qingyu_dejavu::{
    CatalogIssueKind, Cloud, CloudError, CloudUploadSource, RepositoryCatalogEntry,
    RepositoryMetadata, S3AddressingStyle, S3Cloud, S3Connection, S3RepositoryCatalog,
    S3RequestSigner, S3TlsVerification, S3TransportOptions,
};
use reqwest::header::{AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, HOST};
use reqwest::Method;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use tokio::io::{AsyncRead, ReadBuf};
use tokio::net::TcpListener;

const FIXED_TIME: i64 = 1_784_181_600;
const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const REPOSITORY_PREFIX: &str = "qingyu/repositories/repo-a/repo";
const REF_GET_TARGET: &str =
    "/qingyu-notes/qingyu/repositories/repo-a/repo/refs/latest?response-cache-control=no-cache";
const OBJECT_GET_TARGET: &str =
    "/qingyu-notes/qingyu/repositories/repo-a/repo/objects/ab/cdef?response-cache-control=no-cache";

fn connection(addressing_style: S3AddressingStyle) -> S3Connection {
    S3Connection::new(
        "https://s3.example.test",
        "us-east-1",
        "qingyu-notes",
        "test-key",
        "test-secret",
        addressing_style,
    )
    .expect("valid S3 connection")
}

fn fixed_time() -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(FIXED_TIME).expect("fixed timestamp")
}

#[test]
fn signing_path_style_matches_the_fixed_sigv4_vector() {
    let connection = connection(S3AddressingStyle::Path);
    let url = connection
        .object_url("repo/objects/ab/hello world")
        .expect("encoded object URL");
    let headers = S3RequestSigner::new(connection)
        .sign_bytes_at(
            &Method::PUT,
            &url,
            b"hello",
            Some("application/octet-stream"),
            fixed_time(),
        )
        .expect("signed headers");

    assert_eq!(
        url.as_str(),
        "https://s3.example.test/qingyu-notes/repo/objects/ab/hello%20world"
    );
    assert_eq!(headers.get(HOST).unwrap(), "s3.example.test");
    assert_eq!(headers.get(CONTENT_LENGTH).unwrap(), "5");
    assert_eq!(headers.get("x-amz-date").unwrap(), "20260716T060000Z");
    assert_eq!(
        headers.get(AUTHORIZATION).unwrap(),
        "AWS4-HMAC-SHA256 Credential=test-key/20260716/us-east-1/s3/aws4_request, SignedHeaders=content-type;host;x-amz-content-sha256;x-amz-date, Signature=be6fac64e75d77fe31b8e44331b8888c10702880ec79c20ff0aced048651d246"
    );
}

#[test]
fn signing_virtual_hosted_style_matches_the_fixed_sigv4_vector() {
    let connection = connection(S3AddressingStyle::VirtualHosted);
    let mut url = connection
        .object_url("repo/refs/latest")
        .expect("virtual-hosted object URL");
    url.set_query(Some("prefix=repo/refs/&list-type=2"));
    let headers = S3RequestSigner::new(connection)
        .sign_empty_at(&Method::GET, &url, fixed_time())
        .expect("signed headers");

    assert_eq!(
        url.as_str(),
        "https://qingyu-notes.s3.example.test/repo/refs/latest?prefix=repo/refs/&list-type=2"
    );
    assert_eq!(headers.get(HOST).unwrap(), "qingyu-notes.s3.example.test");
    assert_eq!(
        headers.get(AUTHORIZATION).unwrap(),
        "AWS4-HMAC-SHA256 Credential=test-key/20260716/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature=f4f609f5ef8b644309971634ea6e3375d2228959f7725a5763e579ea9e75a72c"
    );
}

#[test]
fn signing_blank_region_uses_auto_and_query_parameters_are_sorted() {
    let connection = S3Connection::new(
        "https://s3.example.test",
        "  ",
        "qingyu-notes",
        "test-key",
        "test-secret",
        S3AddressingStyle::Path,
    )
    .expect("blank region should use auto");
    let mut url = connection.object_url("repo").expect("object URL");
    url.set_query(Some("prefix=repo/refs/&list-type=2"));
    let headers = S3RequestSigner::new(connection.clone())
        .sign_empty_at(&Method::GET, &url, fixed_time())
        .expect("signed headers");

    assert_eq!(connection.region, "auto");
    assert_eq!(
        headers.get(AUTHORIZATION).unwrap(),
        "AWS4-HMAC-SHA256 Credential=test-key/20260716/auto/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature=f61983609a4d84139aacbde6ba07d725c2edcc8eb3a5409d7a7bbfe05ee5be1e"
    );
}

#[test]
fn signing_encodes_each_object_key_segment_once() {
    let connection = connection(S3AddressingStyle::Path);

    let url = connection
        .object_url("repo/空 格/%2F?#.md")
        .expect("encoded object URL");

    assert_eq!(
        url.as_str(),
        "https://s3.example.test/qingyu-notes/repo/%E7%A9%BA%20%E6%A0%BC/%252F%3F%23.md"
    );
}

#[test]
fn signing_aws_path_encodes_reserved_punctuation() {
    let connection = connection(S3AddressingStyle::Path);
    let url = connection
        .object_url("repo/a+b!c")
        .expect("AWS-encoded object URL");
    let headers = S3RequestSigner::new(connection)
        .sign_bytes_at(
            &Method::PUT,
            &url,
            b"hello",
            Some("application/octet-stream"),
            fixed_time(),
        )
        .expect("signed headers");

    assert_eq!(
        url.as_str(),
        "https://s3.example.test/qingyu-notes/repo/a%2Bb%21c"
    );
    assert_eq!(
        headers.get(AUTHORIZATION).unwrap(),
        "AWS4-HMAC-SHA256 Credential=test-key/20260716/us-east-1/s3/aws4_request, SignedHeaders=content-type;host;x-amz-content-sha256;x-amz-date, Signature=0c829a323266cae4782fa167966ad4445215c742bba3d96a4e6593fa7b5d9dc3"
    );
}

#[test]
fn signing_content_type_uses_sigv4_trim_all_for_header_and_signature() {
    let connection = connection(S3AddressingStyle::Path);
    let url = connection
        .object_url("repo/refs/latest")
        .expect("object URL");
    let headers = S3RequestSigner::new(connection)
        .sign_bytes_at(
            &Method::PUT,
            &url,
            b"hello",
            Some("\ttext/plain;  \tcharset=utf-8 \t"),
            fixed_time(),
        )
        .expect("signed headers");

    assert_eq!(
        headers.get(CONTENT_TYPE).unwrap(),
        "text/plain; charset=utf-8"
    );
    assert_eq!(
        headers.get(AUTHORIZATION).unwrap(),
        "AWS4-HMAC-SHA256 Credential=test-key/20260716/us-east-1/s3/aws4_request, SignedHeaders=content-type;host;x-amz-content-sha256;x-amz-date, Signature=e5853974cd16ad831d191e4a842b35efaa6b75746592355e2921eb12f0b9e9ac"
    );
}

#[test]
fn signing_debug_output_redacts_both_credentials() {
    let connection = S3Connection::new(
        "https://s3.example.test",
        "us-east-1",
        "qingyu-notes",
        "private-access-key",
        "private-secret-key",
        S3AddressingStyle::Auto,
    )
    .expect("valid S3 connection");
    let connection_debug = format!("{connection:?}");
    let signer_debug = format!("{:?}", S3RequestSigner::new(connection));

    for output in [connection_debug, signer_debug] {
        assert!(!output.contains("private-access-key"));
        assert!(!output.contains("private-secret-key"));
        assert!(output.contains("[REDACTED]"));
    }
}

#[test]
fn signing_empty_body_hashes_zero_bytes_without_a_logical_empty_marker() {
    let connection = connection(S3AddressingStyle::Path);
    let url = connection
        .object_url("repo/refs/latest")
        .expect("object URL");
    let headers = S3RequestSigner::new(connection)
        .sign_bytes_at(
            &Method::PUT,
            &url,
            b"",
            Some("application/octet-stream"),
            fixed_time(),
        )
        .expect("signed headers");

    assert_eq!(headers.get(CONTENT_LENGTH).unwrap(), "0");
    assert_eq!(headers.get("x-amz-content-sha256").unwrap(), EMPTY_SHA256);
    assert!(!headers.contains_key("x-amz-meta-qingyu-logical-empty"));
    assert!(!headers
        .get(AUTHORIZATION)
        .unwrap()
        .to_str()
        .unwrap()
        .contains("x-amz-meta-qingyu-logical-empty"));
}

#[test]
fn signing_prehashed_payload_supports_u64_content_lengths_without_payload_bytes() {
    let connection = connection(S3AddressingStyle::Path);
    let url = connection
        .object_url("repo/objects/ab/large")
        .expect("object URL");
    let content_length: u64 = u64::from(u32::MAX) + 17;
    let headers = S3RequestSigner::new(connection)
        .sign_prehashed_at(
            &Method::PUT,
            &url,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
            content_length,
            Some("application/octet-stream"),
            fixed_time(),
        )
        .expect("prehashed payload should sign without payload bytes");

    assert_eq!(headers.get(CONTENT_LENGTH).unwrap(), "4294967312");
    assert_eq!(
        headers.get(AUTHORIZATION).unwrap(),
        "AWS4-HMAC-SHA256 Credential=test-key/20260716/us-east-1/s3/aws4_request, SignedHeaders=content-type;host;x-amz-content-sha256;x-amz-date, Signature=f773531a68895081557ad21abe37e041319a1ef629a2309548094e1458c1a55f"
    );
}

#[test]
fn signing_prehashed_payload_rejects_non_sha256_lower_hex() {
    let connection = connection(S3AddressingStyle::Path);
    let url = connection
        .object_url("repo/objects/ab/large")
        .expect("object URL");
    let signer = S3RequestSigner::new(connection);
    let invalid_hashes = [
        "abc".to_string(),
        "a".repeat(63),
        "a".repeat(65),
        "A".repeat(64),
        "g".repeat(64),
    ];

    for invalid_hash in invalid_hashes {
        let error = signer
            .sign_prehashed_at(&Method::PUT, &url, &invalid_hash, 1, None, fixed_time())
            .expect_err("invalid SHA-256 text must be rejected");
        assert_eq!(error.code(), "s3_invalid_payload_hash");
    }
}

#[derive(Clone, Debug)]
struct ExpectedRequest {
    method: &'static str,
    target: &'static str,
    required_headers: Vec<(&'static str, &'static str)>,
    absent_headers: Vec<&'static str>,
    body: Vec<u8>,
    payload_sha256: String,
    body_read: FixtureBodyRead,
    response: FixtureResponse,
}

#[derive(Clone, Copy, Debug)]
enum FixtureBodyRead {
    Full,
    Prefix(usize),
    ConnectionOnly,
}

#[derive(Clone, Debug)]
enum FixtureResponse {
    Http {
        status: u16,
        headers: Vec<(&'static str, &'static str)>,
        body: Vec<u8>,
        declared_length: Option<usize>,
    },
    Disconnect,
}

impl FixtureResponse {
    fn ok(body: impl AsRef<[u8]>) -> Self {
        Self::Http {
            status: 200,
            headers: vec![],
            body: body.as_ref().to_vec(),
            declared_length: None,
        }
    }

    fn status(status: u16) -> Self {
        Self::Http {
            status,
            headers: vec![],
            body: vec![],
            declared_length: None,
        }
    }
}

struct HttpFixture {
    endpoint: String,
    observed: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl HttpFixture {
    async fn start(expected: Vec<ExpectedRequest>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind HTTP fixture");
        let endpoint = format!("http://{}", listener.local_addr().expect("fixture address"));
        let observed = Arc::new(AtomicUsize::new(0));
        let observed_for_task = Arc::clone(&observed);
        let task = tokio::spawn(async move {
            for expectation in expected {
                let (mut stream, _) =
                    tokio::time::timeout(Duration::from_secs(2), listener.accept())
                        .await
                        .expect("fixture timed out waiting for expected request")
                        .expect("accept fixture request");
                observed_for_task.fetch_add(1, Ordering::SeqCst);
                let (request_head, body) = read_request(&mut stream, expectation.body_read).await;
                if !matches!(expectation.body_read, FixtureBodyRead::ConnectionOnly) {
                    assert_request(&request_head, &body, &expectation);
                }
                match expectation.response {
                    FixtureResponse::Disconnect => {}
                    FixtureResponse::Http {
                        status,
                        headers,
                        body,
                        declared_length,
                    } => {
                        let reason = match status {
                            200 => "OK",
                            204 => "No Content",
                            307 => "Temporary Redirect",
                            400 => "Bad Request",
                            401 => "Unauthorized",
                            403 => "Forbidden",
                            404 => "Not Found",
                            408 => "Request Timeout",
                            418 => "I'm a teapot",
                            429 => "Too Many Requests",
                            500 => "Internal Server Error",
                            502 => "Bad Gateway",
                            503 => "Service Unavailable",
                            504 => "Gateway Timeout",
                            _ => "Fixture",
                        };
                        let declared_length = declared_length.unwrap_or(body.len());
                        let mut response = format!(
                            "HTTP/1.1 {status} {reason}\r\nContent-Length: {declared_length}\r\nConnection: close\r\n"
                        );
                        for (name, value) in headers {
                            response.push_str(name);
                            response.push_str(": ");
                            response.push_str(value);
                            response.push_str("\r\n");
                        }
                        response.push_str("\r\n");
                        tokio::io::AsyncWriteExt::write_all(&mut stream, response.as_bytes())
                            .await
                            .expect("write fixture response head");
                        tokio::io::AsyncWriteExt::write_all(&mut stream, &body)
                            .await
                            .expect("write fixture response body");
                        tokio::io::AsyncWriteExt::shutdown(&mut stream)
                            .await
                            .expect("close fixture response");
                    }
                }
            }

            if let Ok(Ok((_unexpected, _))) =
                tokio::time::timeout(Duration::from_millis(150), listener.accept()).await
            {
                observed_for_task.fetch_add(1, Ordering::SeqCst);
                panic!("fixture received an unexpected retry");
            }
        });
        Self {
            endpoint,
            observed,
            task,
        }
    }

    async fn finish(self, expected_requests: usize) {
        self.task.await.expect("HTTP fixture task failed");
        assert_eq!(
            self.observed.load(Ordering::SeqCst),
            expected_requests,
            "unexpected HTTP request count"
        );
    }
}

async fn read_request(
    stream: &mut tokio::net::TcpStream,
    body_read: FixtureBodyRead,
) -> (String, Vec<u8>) {
    if matches!(body_read, FixtureBodyRead::ConnectionOnly) {
        let mut byte = [0_u8; 1];
        let _ = tokio::io::AsyncReadExt::read(stream, &mut byte)
            .await
            .expect("observe source-error connection close");
        return (String::new(), Vec::new());
    }
    let mut bytes = Vec::new();
    let mut scratch = [0_u8; 4096];
    let header_end = loop {
        let read = tokio::io::AsyncReadExt::read(stream, &mut scratch)
            .await
            .expect("read fixture request");
        assert_ne!(read, 0, "request ended before headers");
        bytes.extend_from_slice(&scratch[..read]);
        if let Some(offset) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break offset + 4;
        }
        assert!(bytes.len() <= 64 * 1024, "request headers were oversized");
    };
    let head = String::from_utf8(bytes[..header_end].to_vec()).expect("UTF-8 request headers");
    let headers = parse_request_headers(&head);
    let content_length = headers
        .get("content-length")
        .map(|value| value.parse::<usize>().expect("numeric content-length"))
        .unwrap_or(0);
    let body_target = match body_read {
        FixtureBodyRead::Full | FixtureBodyRead::ConnectionOnly => content_length,
        FixtureBodyRead::Prefix(limit) => content_length.min(limit),
    };
    while bytes.len() - header_end < body_target {
        let read = tokio::io::AsyncReadExt::read(stream, &mut scratch)
            .await
            .expect("read fixture request body");
        assert_ne!(read, 0, "request body ended early");
        bytes.extend_from_slice(&scratch[..read]);
    }
    let retained = (bytes.len() - header_end).min(body_target);
    (head, bytes[header_end..header_end + retained].to_vec())
}

fn parse_request_headers(head: &str) -> BTreeMap<String, String> {
    head.split("\r\n")
        .skip(1)
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_string()))
        .collect()
}

fn assert_request(head: &str, body: &[u8], expected: &ExpectedRequest) {
    let request_line = head.split("\r\n").next().expect("request line");
    assert_eq!(
        request_line,
        format!("{} {} HTTP/1.1", expected.method, expected.target)
    );
    let headers = parse_request_headers(head);
    assert_eq!(
        headers.get("x-amz-content-sha256"),
        Some(&expected.payload_sha256),
        "wrong signed payload hash"
    );
    for (name, value) in &expected.required_headers {
        let actual = headers
            .get(&name.to_ascii_lowercase())
            .unwrap_or_else(|| panic!("missing required header {name}"));
        if *value == "<aws4>" {
            assert!(actual.starts_with("AWS4-HMAC-SHA256 "));
        } else {
            assert_eq!(actual, value, "wrong {name} header");
        }
    }
    for name in &expected.absent_headers {
        assert!(
            !headers.contains_key(&name.to_ascii_lowercase()),
            "header {name} must be absent"
        );
    }
    assert_eq!(body, expected.body);
}

fn expected_request(
    method: &'static str,
    target: &'static str,
    body: impl AsRef<[u8]>,
    response: FixtureResponse,
) -> ExpectedRequest {
    let body = body.as_ref().to_vec();
    ExpectedRequest {
        method,
        target,
        required_headers: vec![("authorization", "<aws4>")],
        absent_headers: vec![],
        payload_sha256: sha256_hex(&body),
        body,
        body_read: FixtureBodyRead::Full,
        response,
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn cloud(endpoint: &str, max_attempts: usize) -> S3Cloud {
    let connection = S3Connection::new(
        endpoint,
        "us-east-1",
        "qingyu-notes",
        "test-key",
        "test-secret",
        S3AddressingStyle::Path,
    )
    .expect("valid fixture connection");
    S3Cloud::new(
        connection,
        S3TransportOptions {
            request_timeout: Duration::from_secs(2),
            tls_verification: S3TlsVerification::Verify,
            max_attempts,
        },
        REPOSITORY_PREFIX,
    )
    .expect("valid fixture S3 cloud")
}

fn catalog(endpoint: &str, max_attempts: usize) -> S3RepositoryCatalog {
    let connection = S3Connection::new(
        endpoint,
        "us-east-1",
        "qingyu-notes",
        "test-key",
        "test-secret",
        S3AddressingStyle::Path,
    )
    .expect("valid fixture connection");
    S3RepositoryCatalog::new(
        connection,
        S3TransportOptions {
            request_timeout: Duration::from_secs(2),
            tls_verification: S3TlsVerification::Verify,
            max_attempts,
        },
    )
    .expect("valid fixture S3 catalog")
}

fn leak(value: String) -> &'static str {
    Box::leak(value.into_boxed_str())
}

struct MemoryReader {
    bytes: Vec<u8>,
    offset: usize,
}

impl AsyncRead for MemoryReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let remaining = &self.bytes[self.offset..];
        let count = remaining.len().min(buffer.remaining());
        buffer.put_slice(&remaining[..count]);
        self.offset += count;
        Poll::Ready(Ok(()))
    }
}

struct CountingSource {
    bytes: Vec<u8>,
    content_length: u64,
    opens: AtomicUsize,
}

struct BodyErrorSource {
    opens: AtomicUsize,
}

impl CloudUploadSource for BodyErrorSource {
    fn content_length(&self) -> u64 {
        7
    }

    fn open(&self) -> Result<Pin<Box<dyn AsyncRead + Send>>, CloudError> {
        let open = self.opens.fetch_add(1, Ordering::SeqCst);
        if open == 0 {
            Ok(Box::pin(MemoryReader {
                bytes: b"payload".to_vec(),
                offset: 0,
            }))
        } else {
            Ok(Box::pin(FailingReader {
                bytes: b"abc".to_vec(),
                offset: 0,
                failed: false,
            }))
        }
    }
}

struct FailingReader {
    bytes: Vec<u8>,
    offset: usize,
    failed: bool,
}

impl AsyncRead for FailingReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if self.offset < self.bytes.len() {
            let count = (self.bytes.len() - self.offset).min(buffer.remaining());
            let end = self.offset + count;
            buffer.put_slice(&self.bytes[self.offset..end]);
            self.offset = end;
            return Poll::Ready(Ok(()));
        }
        if !self.failed {
            self.failed = true;
            return Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "fixture upload source failed",
            )));
        }
        Poll::Ready(Ok(()))
    }
}

impl CountingSource {
    fn new(bytes: impl Into<Vec<u8>>, content_length: u64) -> Self {
        Self {
            bytes: bytes.into(),
            content_length,
            opens: AtomicUsize::new(0),
        }
    }
}

impl CloudUploadSource for CountingSource {
    fn content_length(&self) -> u64 {
        self.content_length
    }

    fn open(&self) -> Result<Pin<Box<dyn AsyncRead + Send>>, CloudError> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        Ok(Box::pin(MemoryReader {
            bytes: self.bytes.clone(),
            offset: 0,
        }))
    }
}

async fn assert_completes<T>(future: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(3), future)
        .await
        .expect("operation must not hang waiting for fixture requests")
}

#[tokio::test]
async fn cloud_put_uses_plain_no_cache_put_for_both_overwrite_values_and_true_empty_body() {
    let target = "/qingyu-notes/qingyu/repositories/repo-a/repo/refs/latest";
    let mut first = expected_request("PUT", target, b"hello", FixtureResponse::ok(vec![]));
    first
        .required_headers
        .extend([("cache-control", "no-cache"), ("content-length", "5")]);
    first.absent_headers.push("if-none-match");
    let mut second = expected_request("PUT", target, vec![], FixtureResponse::ok(vec![]));
    second.required_headers.extend([
        ("cache-control", "no-cache"),
        ("content-length", "0"),
        ("x-amz-content-sha256", EMPTY_SHA256),
    ]);
    second.absent_headers.push("if-none-match");
    let fixture = HttpFixture::start(vec![first, second]).await;
    let s3 = cloud(&fixture.endpoint, 1);

    assert_eq!(s3.put("refs/latest", b"hello", false).await.unwrap(), 5);
    assert_eq!(s3.put("refs/latest", b"", true).await.unwrap(), 0);
    fixture.finish(2).await;
}

#[tokio::test]
async fn cloud_get_is_bounded_at_max_plus_one_and_maps_404_to_not_found() {
    let target = REF_GET_TARGET;
    let fixture = HttpFixture::start(vec![
        expected_request("GET", target, vec![], FixtureResponse::ok(b"hello")),
        expected_request("GET", target, vec![], FixtureResponse::ok(b"hello")),
        expected_request("GET", target, vec![], FixtureResponse::status(404)),
    ])
    .await;
    let s3 = cloud(&fixture.endpoint, 1);

    assert_eq!(s3.get_bounded("refs/latest", 5).await.unwrap(), b"hello");
    assert!(matches!(
        s3.get_bounded("refs/latest", 4).await,
        Err(CloudError::ResponseTooLarge { limit: 4 })
    ));
    assert!(matches!(
        s3.get_bounded("refs/latest", 42).await,
        Err(CloudError::NotFound)
    ));
    fixture.finish(3).await;
}

#[tokio::test]
async fn cloud_get_retries_a_body_failure_before_retaining_the_first_byte() {
    let target = REF_GET_TARGET;
    let fixture = HttpFixture::start(vec![
        expected_request(
            "GET",
            target,
            vec![],
            FixtureResponse::Http {
                status: 200,
                headers: vec![],
                body: vec![],
                declared_length: Some(2),
            },
        ),
        expected_request("GET", target, vec![], FixtureResponse::ok(b"ok")),
    ])
    .await;
    let s3 = cloud(&fixture.endpoint, 3);

    assert_eq!(s3.get_bounded("refs/latest", 2).await.unwrap(), b"ok");
    fixture.finish(2).await;
}

#[tokio::test]
async fn cloud_download_streams_once_and_never_appends_a_retry_after_partial_body() {
    let target = OBJECT_GET_TARGET;
    let fixture = HttpFixture::start(vec![
        expected_request("GET", target, vec![], FixtureResponse::ok(b"abcdef")),
        expected_request(
            "GET",
            target,
            vec![],
            FixtureResponse::Http {
                status: 200,
                headers: vec![],
                body: b"abc".to_vec(),
                declared_length: Some(6),
            },
        ),
    ])
    .await;
    let s3 = cloud(&fixture.endpoint, 3);
    let mut complete = Vec::new();
    let mut truncated = Vec::new();

    assert_eq!(
        s3.download_to("objects/ab/cdef", &mut complete)
            .await
            .unwrap(),
        6
    );
    assert_eq!(complete, b"abcdef");
    let error = assert_completes(s3.download_to("objects/ab/cdef", &mut truncated))
        .await
        .expect_err("truncated response must fail");
    assert!(matches!(
        error,
        CloudError::Io(_) | CloudError::Backend { .. }
    ));
    assert_eq!(truncated, b"abc");
    fixture.finish(2).await;
}

#[tokio::test]
async fn cloud_download_counts_header_retries_and_zero_byte_body_failure_in_one_attempt_budget() {
    let target = OBJECT_GET_TARGET;
    let fixture = HttpFixture::start(vec![
        expected_request("GET", target, vec![], FixtureResponse::status(503)),
        expected_request("GET", target, vec![], FixtureResponse::status(503)),
        expected_request(
            "GET",
            target,
            vec![],
            FixtureResponse::Http {
                status: 200,
                headers: vec![],
                body: vec![],
                declared_length: Some(5),
            },
        ),
    ])
    .await;
    let s3 = cloud(&fixture.endpoint, 3);
    let mut destination = Vec::new();

    assert!(
        assert_completes(s3.download_to("objects/ab/cdef", &mut destination))
            .await
            .is_err()
    );
    assert!(destination.is_empty());
    fixture.finish(3).await;
}

#[tokio::test]
async fn cloud_upload_prehashes_and_reopens_the_source_for_every_retry() {
    let target = "/qingyu-notes/qingyu/repositories/repo-a/repo/objects/ab/cdef";
    let first = expected_request("PUT", target, b"payload", FixtureResponse::status(503));
    let second = expected_request("PUT", target, b"payload", FixtureResponse::ok(vec![]));
    let fixture = HttpFixture::start(vec![first, second]).await;
    let s3 = cloud(&fixture.endpoint, 3);
    let source = CountingSource::new(b"payload", 7);

    assert_eq!(
        s3.upload_from("objects/ab/cdef", &source, false)
            .await
            .unwrap(),
        7
    );
    assert_eq!(
        source.opens.load(Ordering::SeqCst),
        4,
        "each attempt must open once for prehash and once for its body"
    );
    fixture.finish(2).await;
}

#[tokio::test]
async fn cloud_upload_reopens_and_retries_after_the_server_drops_a_partial_put() {
    let target = "/qingyu-notes/qingyu/repositories/repo-a/repo/objects/ab/cdef";
    let payload = vec![b'x'; 256 * 1024];
    let mut first = expected_request("PUT", target, &payload[..1024], FixtureResponse::Disconnect);
    first.body_read = FixtureBodyRead::Prefix(1024);
    first.payload_sha256 = sha256_hex(&payload);
    first.required_headers.push(("content-length", "262144"));
    let second = expected_request("PUT", target, &payload, FixtureResponse::ok(vec![]));
    let fixture = HttpFixture::start(vec![first, second]).await;
    let s3 = cloud(&fixture.endpoint, 3);
    let source = CountingSource::new(payload.clone(), payload.len() as u64);

    assert_eq!(
        assert_completes(s3.upload_from("objects/ab/cdef", &source, true))
            .await
            .unwrap(),
        payload.len() as u64
    );
    assert_eq!(source.opens.load(Ordering::SeqCst), 4);
    fixture.finish(2).await;
}

#[tokio::test]
async fn cloud_upload_restores_a_source_io_error_and_does_not_retry_it() {
    let target = "/qingyu-notes/qingyu/repositories/repo-a/repo/objects/ab/cdef";
    let mut request = expected_request("PUT", target, b"", FixtureResponse::Disconnect);
    request.body_read = FixtureBodyRead::ConnectionOnly;
    let fixture = HttpFixture::start(vec![request]).await;
    let s3 = cloud(&fixture.endpoint, 3);
    let source = BodyErrorSource {
        opens: AtomicUsize::new(0),
    };

    let error = assert_completes(s3.upload_from("objects/ab/cdef", &source, true))
        .await
        .expect_err("source I/O error must fail the upload");
    assert!(matches!(
        error,
        CloudError::Io(ref source_error)
            if source_error.kind() == std::io::ErrorKind::PermissionDenied
    ));
    assert_eq!(source.opens.load(Ordering::SeqCst), 2);
    fixture.finish(1).await;
}

#[tokio::test]
async fn cloud_upload_rejects_short_and_long_sources_before_network_io_without_retry() {
    let fixture = HttpFixture::start(vec![]).await;
    let s3 = cloud(&fixture.endpoint, 3);
    let short = CountingSource::new(b"short", 6);
    let long = CountingSource::new(b"longer", 5);

    assert!(matches!(
        s3.upload_from("objects/ab/cdef", &short, true).await,
        Err(CloudError::LengthMismatch {
            expected: 6,
            actual: 5
        })
    ));
    assert!(matches!(
        s3.upload_from("objects/ab/cdef", &long, true).await,
        Err(CloudError::LengthMismatch {
            expected: 5,
            actual: 6
        })
    ));
    assert_eq!(short.opens.load(Ordering::SeqCst), 1);
    assert_eq!(long.opens.load(Ordering::SeqCst), 1);
    fixture.finish(0).await;
}

#[tokio::test]
async fn cloud_delete_is_unconditional_and_has_no_if_match_header() {
    let target = "/qingyu-notes/qingyu/repositories/repo-a/repo/refs/latest";
    let mut request = expected_request("DELETE", target, vec![], FixtureResponse::status(204));
    request.absent_headers.push("if-match");
    let fixture = HttpFixture::start(vec![request]).await;
    let s3 = cloud(&fixture.endpoint, 1);

    s3.remove("refs/latest").await.unwrap();
    fixture.finish(1).await;
}

#[tokio::test]
async fn cloud_list_paginates_strips_only_the_repository_prefix_and_sorts_keys() {
    let first_target = "/qingyu-notes?list-type=2&max-keys=1000&prefix=qingyu%2Frepositories%2Frepo-a%2Frepo%2Fobjects%2F";
    let second_target = "/qingyu-notes?continuation-token=next%2Btoken&list-type=2&max-keys=1000&prefix=qingyu%2Frepositories%2Frepo-a%2Frepo%2Fobjects%2F";
    let first_xml = br#"<ListBucketResult><CommonPrefixes/><CustomSibling/><IsTruncated>true</IsTruncated><NextContinuationToken>next+token</NextContinuationToken><Contents><Key>qingyu/repositories/repo-a/repo/objects/zz/item</Key><Size>9</Size></Contents></ListBucketResult>"#;
    let second_xml = br#"<ListBucketResult><IsTruncated>false</IsTruncated><Contents><Key>qingyu/repositories/repo-a/repo/objects/ab/cdef</Key><Size>4</Size></Contents></ListBucketResult>"#;
    let fixture = HttpFixture::start(vec![
        expected_request("GET", first_target, vec![], FixtureResponse::ok(first_xml)),
        expected_request(
            "GET",
            second_target,
            vec![],
            FixtureResponse::ok(second_xml),
        ),
    ])
    .await;
    let s3 = cloud(&fixture.endpoint, 1);

    let objects = s3.list("objects/").await.unwrap();
    assert_eq!(
        objects,
        vec![
            qingyu_dejavu::CloudObject {
                key: "objects/ab/cdef".to_string(),
                size: 4,
            },
            qingyu_dejavu::CloudObject {
                key: "objects/zz/item".to_string(),
                size: 9,
            },
        ]
    );
    fixture.finish(2).await;
}

#[tokio::test]
async fn cloud_list_rejects_malformed_truncated_cross_prefix_and_stalled_pagination() {
    let target = "/qingyu-notes?list-type=2&max-keys=1000&prefix=qingyu%2Frepositories%2Frepo-a%2Frepo%2Fobjects%2F";
    let cases: Vec<&[u8]> = vec![
        br#"<ListBucketResult><Contents><Key>x</Key>"#,
        br#"<ListBucketResult><IsTruncated>true</IsTruncated></ListBucketResult>"#,
        br#"<ListBucketResult><IsTruncated>false</IsTruncated><Contents><Key>qingyu/repositories/repo-b/repo/objects/ab/cdef</Key><Size>1</Size></Contents></ListBucketResult>"#,
        br#"<ListBucketResult><IsTruncated>fal<Unexpected/>se</IsTruncated></ListBucketResult>"#,
        br#"<ListBucketResult><IsTruncated>false</IsTruncated></ListBucketResult><ListBucketResult><IsTruncated>false</IsTruncated></ListBucketResult>"#,
        br#"<ListBucketResult><IsTruncated>false</IsTruncated></ListBucketResult>trailing"#,
        br#"<ListBucketResult><Wrapper><IsTruncated>false</IsTruncated></Wrapper></ListBucketResult>"#,
        br#"<ListBucketResult><IsTruncated>false</IsTruncated><Contents/></ListBucketResult>"#,
        br#"<ListBucketResult><IsTruncated>false</IsTruncated><NextContinuationToken/></ListBucketResult>"#,
    ];
    for (index, xml) in cases.into_iter().enumerate() {
        let fixture = HttpFixture::start(vec![expected_request(
            "GET",
            target,
            vec![],
            FixtureResponse::ok(xml),
        )])
        .await;
        let s3 = cloud(&fixture.endpoint, 1);
        let error = s3.list("objects/").await.expect_err("case must fail");
        assert!(
            matches!(error, CloudError::Backend { .. } | CloudError::UnsafeKey),
            "case {index} returned {error:?}"
        );
        fixture.finish(1).await;
    }
}

#[tokio::test]
async fn cloud_list_rejects_a_continuation_token_that_does_not_advance() {
    let first_target = "/qingyu-notes?list-type=2&max-keys=1000&prefix=qingyu%2Frepositories%2Frepo-a%2Frepo%2Fobjects%2F";
    let second_target = "/qingyu-notes?continuation-token=same&list-type=2&max-keys=1000&prefix=qingyu%2Frepositories%2Frepo-a%2Frepo%2Fobjects%2F";
    let xml = br#"<ListBucketResult><IsTruncated>true</IsTruncated><NextContinuationToken>same</NextContinuationToken></ListBucketResult>"#;
    let fixture = HttpFixture::start(vec![
        expected_request("GET", first_target, vec![], FixtureResponse::ok(xml)),
        expected_request("GET", second_target, vec![], FixtureResponse::ok(xml)),
    ])
    .await;
    let s3 = cloud(&fixture.endpoint, 1);

    assert!(matches!(
        s3.list("objects/").await,
        Err(CloudError::Backend {
            code: "s3_list_stalled_continuation",
            retryable: false
        })
    ));
    fixture.finish(2).await;
}

#[tokio::test]
async fn cloud_list_rejects_a_multi_token_continuation_cycle() {
    let first_target = "/qingyu-notes?list-type=2&max-keys=1000&prefix=qingyu%2Frepositories%2Frepo-a%2Frepo%2Fobjects%2F";
    let second_target = "/qingyu-notes?continuation-token=A&list-type=2&max-keys=1000&prefix=qingyu%2Frepositories%2Frepo-a%2Frepo%2Fobjects%2F";
    let third_target = "/qingyu-notes?continuation-token=B&list-type=2&max-keys=1000&prefix=qingyu%2Frepositories%2Frepo-a%2Frepo%2Fobjects%2F";
    let page_a = br#"<ListBucketResult><IsTruncated>true</IsTruncated><NextContinuationToken>A</NextContinuationToken></ListBucketResult>"#;
    let page_b = br#"<ListBucketResult><IsTruncated>true</IsTruncated><NextContinuationToken>B</NextContinuationToken></ListBucketResult>"#;
    let fixture = HttpFixture::start(vec![
        expected_request("GET", first_target, vec![], FixtureResponse::ok(page_a)),
        expected_request("GET", second_target, vec![], FixtureResponse::ok(page_b)),
        expected_request("GET", third_target, vec![], FixtureResponse::ok(page_a)),
    ])
    .await;
    let s3 = cloud(&fixture.endpoint, 1);

    assert!(matches!(
        s3.list("objects/").await,
        Err(CloudError::Backend {
            code: "s3_list_stalled_continuation",
            retryable: false
        })
    ));
    fixture.finish(3).await;
}

#[tokio::test]
async fn cloud_list_rejects_oversized_xml_fields_and_more_than_one_thousand_contents() {
    let target = "/qingyu-notes?list-type=2&max-keys=1000&prefix=qingyu%2Frepositories%2Frepo-a%2Frepo%2Fobjects%2F";
    let oversized_key = format!(
        "<ListBucketResult><IsTruncated>false</IsTruncated><Contents><Key>{}</Key><Size>1</Size></Contents></ListBucketResult>",
        "x".repeat(70 * 1024)
    );
    let mut too_many = String::from("<ListBucketResult><IsTruncated>false</IsTruncated>");
    for index in 0..=1000 {
        too_many.push_str(&format!(
            "<Contents><Key>{REPOSITORY_PREFIX}/objects/{index}</Key><Size>1</Size></Contents>"
        ));
    }
    too_many.push_str("</ListBucketResult>");
    let oversized_document = format!(
        "<ListBucketResult><Name>{}</Name><IsTruncated>false</IsTruncated></ListBucketResult>",
        "x".repeat(8 * 1024 * 1024)
    );
    for xml in [
        oversized_key.into_bytes(),
        too_many.into_bytes(),
        oversized_document.into_bytes(),
    ] {
        let fixture = HttpFixture::start(vec![expected_request(
            "GET",
            target,
            vec![],
            FixtureResponse::ok(xml),
        )])
        .await;
        let s3 = cloud(&fixture.endpoint, 1);
        assert!(s3.list("objects/").await.is_err());
        fixture.finish(1).await;
    }
}

#[tokio::test]
async fn cloud_retries_only_transient_http_statuses_and_re_signs_each_attempt() {
    let target = REF_GET_TARGET;
    for status in [408, 429, 500, 502, 503, 504] {
        let fixture = HttpFixture::start(vec![
            expected_request("GET", target, vec![], FixtureResponse::status(status)),
            expected_request("GET", target, vec![], FixtureResponse::status(status)),
            expected_request("GET", target, vec![], FixtureResponse::ok(b"ok")),
        ])
        .await;
        let s3 = cloud(&fixture.endpoint, 3);
        assert_eq!(s3.get_bounded("refs/latest", 2).await.unwrap(), b"ok");
        fixture.finish(3).await;
    }
}

#[tokio::test]
async fn cloud_retries_connection_failure_but_not_400_401_403_or_redirects() {
    let target = REF_GET_TARGET;
    let fixture = HttpFixture::start(vec![
        expected_request("GET", target, vec![], FixtureResponse::Disconnect),
        expected_request("GET", target, vec![], FixtureResponse::ok(b"ok")),
    ])
    .await;
    let s3 = cloud(&fixture.endpoint, 3);
    assert_eq!(s3.get_bounded("refs/latest", 2).await.unwrap(), b"ok");
    fixture.finish(2).await;

    for status in [400, 401, 403, 307] {
        let mut response = FixtureResponse::status(status);
        if status == 307 {
            response = FixtureResponse::Http {
                status,
                headers: vec![("Location", "http://127.0.0.1:1/credential-leak")],
                body: vec![],
                declared_length: None,
            };
        }
        let fixture =
            HttpFixture::start(vec![expected_request("GET", target, vec![], response)]).await;
        let s3 = cloud(&fixture.endpoint, 3);
        let error = s3.get_bounded("refs/latest", 2).await.unwrap_err();
        match status {
            401 => assert!(matches!(error, CloudError::Auth)),
            403 => assert!(matches!(error, CloudError::Forbidden)),
            400 | 307 => assert!(matches!(
                error,
                CloudError::S3Response {
                    status: actual,
                    retryable: false,
                    ..
                } if actual == status
            )),
            _ => unreachable!(),
        }
        fixture.finish(1).await;
    }
}

#[tokio::test]
async fn cloud_provider_diagnostics_bound_request_ids_without_response_bodies() {
    let target = REF_GET_TARGET;
    let secret_body = b"do-not-retain-this-provider-body";
    let long_request_id = "x".repeat(300);
    let leaked: &'static str = Box::leak(long_request_id.into_boxed_str());
    let fixture = HttpFixture::start(vec![
        expected_request(
            "GET",
            target,
            vec![],
            FixtureResponse::Http {
                status: 418,
                headers: vec![("x-amz-request-id", leaked)],
                body: secret_body.to_vec(),
                declared_length: None,
            },
        ),
        expected_request(
            "GET",
            target,
            vec![],
            FixtureResponse::Http {
                status: 418,
                headers: vec![("x-amz-request-id", "request-123")],
                body: secret_body.to_vec(),
                declared_length: None,
            },
        ),
    ])
    .await;
    let s3 = cloud(&fixture.endpoint, 1);

    let error = s3.get_bounded("refs/latest", 1024).await.unwrap_err();
    let debug = format!("{error:?}");
    assert!(matches!(
        error,
        CloudError::S3Response {
            status: 418,
            request_id: None,
            ..
        }
    ));
    assert!(!debug.contains("do-not-retain-this-provider-body"));
    assert!(!debug.contains("test-key"));
    assert!(!debug.contains("test-secret"));
    assert!(matches!(
        s3.get_bounded("refs/latest", 1024).await,
        Err(CloudError::S3Response {
            status: 418,
            request_id: Some(request_id),
            ..
        }) if request_id == "request-123"
    ));
    fixture.finish(2).await;
}

#[tokio::test]
async fn cloud_rejects_unsafe_repository_prefixes_keys_and_list_prefixes_before_network_io() {
    let connection = S3Connection::new(
        "http://127.0.0.1:1",
        "us-east-1",
        "qingyu-notes",
        "test-key",
        "test-secret",
        S3AddressingStyle::Path,
    )
    .unwrap();
    let options = S3TransportOptions {
        request_timeout: Duration::from_millis(100),
        tls_verification: S3TlsVerification::Verify,
        max_attempts: 1,
    };
    for prefix in [
        "",
        "/qingyu/repo",
        "qingyu//repo",
        "qingyu/../repo",
        "qingyu\\repo",
        "qingyu/repositories",
        "qingyu/repositories/repo-a",
        "catalog",
        "qingyu/repositories//repo",
        "qingyu/repositories/repo-a/repo/extra",
        "qingyu/extra/repositories/repo-a/repo",
    ] {
        assert!(S3Cloud::new(connection.clone(), options, prefix).is_err());
    }
    let s3 = S3Cloud::new(connection, options, REPOSITORY_PREFIX).unwrap();
    for key in [
        "",
        "/refs/latest",
        "refs//latest",
        "refs/./latest",
        "refs/../latest",
        "refs\\latest",
        "refs/\u{7f}latest",
    ] {
        assert!(matches!(
            s3.get_bounded(key, 42).await,
            Err(CloudError::UnsafeKey)
        ));
    }
    for prefix in ["/objects", "objects//", "objects/../", "objects\\"] {
        assert!(matches!(s3.list(prefix).await, Err(CloudError::UnsafeKey)));
    }
}

#[tokio::test]
async fn cloud_s3_capacity_is_explicitly_unknown() {
    let s3 = cloud("http://127.0.0.1:1", 1);
    assert_eq!(s3.available_size().await.unwrap(), u64::MAX);
}

#[test]
fn cloud_transport_options_reject_zero_and_more_than_three_attempts() {
    let connection = connection(S3AddressingStyle::Path);
    for max_attempts in [0, 4] {
        assert!(S3Cloud::new(
            connection.clone(),
            S3TransportOptions {
                request_timeout: Duration::from_secs(1),
                tls_verification: S3TlsVerification::Verify,
                max_attempts,
            },
            REPOSITORY_PREFIX,
        )
        .is_err());
    }
    assert!(S3Cloud::new(
        connection,
        S3TransportOptions {
            request_timeout: Duration::ZERO,
            tls_verification: S3TlsVerification::Verify,
            max_attempts: 1,
        },
        REPOSITORY_PREFIX,
    )
    .is_err());
}

const CATALOG_ID_A: &str = "00000000-0000-4000-8000-000000000001";
const CATALOG_ID_B: &str = "00000000-0000-4000-8000-000000000002";
const CATALOG_ID_C: &str = "00000000-0000-4000-8000-000000000003";
const CATALOG_ID_D: &str = "00000000-0000-4000-8000-000000000004";
const CATALOG_ID_E: &str = "00000000-0000-4000-8000-000000000005";
const CATALOG_ID_F: &str = "00000000-0000-4000-8000-000000000006";
const CATALOG_ID_G: &str = "00000000-0000-4000-8000-000000000007";
const CATALOG_ID_H: &str = "00000000-0000-4000-8000-000000000008";
const CATALOG_ID_I: &str = "00000000-0000-4000-8000-000000000009";
const CATALOG_LIST_TARGET: &str =
    "/qingyu-notes?delimiter=%2F&list-type=2&max-keys=1000&prefix=qingyu%2Frepositories%2F";
const CATALOG_LIST_NEXT_TARGET: &str = "/qingyu-notes?continuation-token=next%2Bpage&delimiter=%2F&list-type=2&max-keys=1000&prefix=qingyu%2Frepositories%2F";

fn catalog_metadata_target(repository_id: &str) -> &'static str {
    leak(format!(
        "/qingyu-notes/qingyu/repositories/{repository_id}/metadata.json?response-cache-control=no-cache"
    ))
}

fn catalog_object_target(repository_id: &str, suffix: &str) -> &'static str {
    leak(format!(
        "/qingyu-notes/qingyu/repositories/{repository_id}/{suffix}"
    ))
}

fn catalog_json(
    repository_id: &str,
    display_name: &str,
    created_at: i64,
    updated_at: i64,
) -> Vec<u8> {
    format!(
        "{{\"formatVersion\":1,\"repositoryId\":\"{repository_id}\",\"displayName\":\"{display_name}\",\"createdAt\":{created_at},\"updatedAt\":{updated_at}}}"
    )
    .into_bytes()
}

#[test]
fn catalog_metadata_serde_contract_is_exact_and_denies_unknown_fields() {
    let metadata = RepositoryMetadata {
        format_version: 1,
        repository_id: CATALOG_ID_A.to_string(),
        display_name: "Notes".to_string(),
        created_at: 10,
        updated_at: 20,
    };

    assert_eq!(
        serde_json::to_string(&metadata).unwrap(),
        format!(
            "{{\"formatVersion\":1,\"repositoryId\":\"{CATALOG_ID_A}\",\"displayName\":\"Notes\",\"createdAt\":10,\"updatedAt\":20}}"
        )
    );
    assert!(serde_json::from_str::<RepositoryMetadata>(&format!(
        "{{\"formatVersion\":1,\"repositoryId\":\"{CATALOG_ID_A}\",\"displayName\":\"Notes\",\"createdAt\":10,\"updatedAt\":20,\"localPath\":\"/secret\"}}"
    ))
    .is_err());
}

#[tokio::test]
async fn catalog_create_rejects_noncanonical_ids_and_invalid_names_before_network_io() {
    let fixture = HttpFixture::start(vec![]).await;
    let catalog = catalog(&fixture.endpoint, 1);

    for invalid_id in [
        "00000000-0000-4000-8000-00000000000A",
        "{00000000-0000-4000-8000-000000000001}",
        "00000000000040008000000000000001",
        "../00000000-0000-4000-8000-000000000001",
        "00000000-0000-4000-8000-000000000001/extra",
    ] {
        assert_eq!(
            catalog
                .create(invalid_id, "Notes", 10)
                .await
                .unwrap_err()
                .code(),
            "catalog_invalid_repository_id"
        );
    }
    for invalid_name in ["", "   ", "bad\nname", "bad\u{7f}name"] {
        assert_eq!(
            catalog
                .create(CATALOG_ID_A, invalid_name, 10)
                .await
                .unwrap_err()
                .code(),
            "catalog_invalid_display_name"
        );
    }
    fixture.finish(0).await;
}

#[tokio::test]
async fn catalog_list_uses_delimited_direct_prefixes_sorts_and_reports_safe_typed_issues() {
    let invalid_upper = "00000000-0000-4000-8000-00000000000A";
    let first_list_xml = format!(
        "<ListBucketResult><IsTruncated>true</IsTruncated><NextContinuationToken>next+page</NextContinuationToken>\
         <CommonPrefixes><Prefix>qingyu/repositories/{CATALOG_ID_A}/</Prefix></CommonPrefixes>\
         <CommonPrefixes><Prefix>qingyu/repositories/{CATALOG_ID_B}/</Prefix></CommonPrefixes>\
         <CommonPrefixes><Prefix>qingyu/repositories/{CATALOG_ID_C}/</Prefix></CommonPrefixes>\
         <CommonPrefixes><Prefix>qingyu/repositories/{CATALOG_ID_D}/</Prefix></CommonPrefixes>\
         </ListBucketResult>"
    );
    let second_list_xml = format!(
        "<ListBucketResult><IsTruncated>false</IsTruncated>\
         <CommonPrefixes><Prefix>qingyu/repositories/{CATALOG_ID_E}/</Prefix></CommonPrefixes>\
         <CommonPrefixes><Prefix>qingyu/repositories/{CATALOG_ID_F}/</Prefix></CommonPrefixes>\
         <CommonPrefixes><Prefix>qingyu/repositories/{CATALOG_ID_G}/</Prefix></CommonPrefixes>\
         <CommonPrefixes><Prefix>qingyu/repositories/{CATALOG_ID_H}/</Prefix></CommonPrefixes>\
         <CommonPrefixes><Prefix>qingyu/repositories/{CATALOG_ID_I}/</Prefix></CommonPrefixes>\
         <CommonPrefixes><Prefix>qingyu/repositories/{invalid_upper}/</Prefix></CommonPrefixes>\
         <CommonPrefixes><Prefix>qingyu/repositories/{CATALOG_ID_A}/nested/</Prefix></CommonPrefixes>\
         <Contents><Key>qingyu/repositories/junk</Key><Size>4</Size></Contents>\
         </ListBucketResult>"
    );
    let valid_a = catalog_json(CATALOG_ID_A, "Alpha", 1, 2);
    let valid_b = catalog_json(CATALOG_ID_B, "Alpha", 3, 4);
    let valid_c = catalog_json(CATALOG_ID_C, "Beta", 5, 6);
    let malformed = b"{not-json".to_vec();
    let unknown = format!(
        "{{\"formatVersion\":1,\"repositoryId\":\"{CATALOG_ID_E}\",\"displayName\":\"Unknown\",\"createdAt\":1,\"updatedAt\":1,\"extra\":true}}"
    )
    .into_bytes();
    let invalid_stored_name = catalog_json(CATALOG_ID_F, " Not Trimmed ", 1, 1);
    let mismatch = catalog_json(CATALOG_ID_A, "Mismatch", 1, 1);
    let mut oversized = catalog_json(CATALOG_ID_H, "Oversized", 1, 1);
    oversized.resize(64 * 1024 + 1, b' ');
    let fixture = HttpFixture::start(vec![
        expected_request(
            "GET",
            CATALOG_LIST_TARGET,
            vec![],
            FixtureResponse::ok(first_list_xml),
        ),
        expected_request(
            "GET",
            CATALOG_LIST_NEXT_TARGET,
            vec![],
            FixtureResponse::ok(second_list_xml),
        ),
        expected_request(
            "GET",
            catalog_metadata_target(CATALOG_ID_A),
            vec![],
            FixtureResponse::ok(valid_a),
        ),
        expected_request(
            "GET",
            catalog_metadata_target(CATALOG_ID_B),
            vec![],
            FixtureResponse::ok(valid_b),
        ),
        expected_request(
            "GET",
            catalog_metadata_target(CATALOG_ID_C),
            vec![],
            FixtureResponse::ok(valid_c),
        ),
        expected_request(
            "GET",
            catalog_metadata_target(CATALOG_ID_D),
            vec![],
            FixtureResponse::ok(malformed),
        ),
        expected_request(
            "GET",
            catalog_metadata_target(CATALOG_ID_E),
            vec![],
            FixtureResponse::ok(unknown),
        ),
        expected_request(
            "GET",
            catalog_metadata_target(CATALOG_ID_F),
            vec![],
            FixtureResponse::ok(invalid_stored_name),
        ),
        expected_request(
            "GET",
            catalog_metadata_target(CATALOG_ID_G),
            vec![],
            FixtureResponse::ok(mismatch),
        ),
        expected_request(
            "GET",
            catalog_metadata_target(CATALOG_ID_H),
            vec![],
            FixtureResponse::ok(oversized),
        ),
        expected_request(
            "GET",
            catalog_metadata_target(CATALOG_ID_I),
            vec![],
            FixtureResponse::status(404),
        ),
    ])
    .await;
    let catalog = catalog(&fixture.endpoint, 1);

    let result = catalog.list().await.unwrap();
    assert_eq!(
        result.entries,
        vec![
            RepositoryCatalogEntry {
                repository_id: CATALOG_ID_A.to_string(),
                display_name: "Alpha".to_string(),
                created_at: 1,
                updated_at: 2,
            },
            RepositoryCatalogEntry {
                repository_id: CATALOG_ID_B.to_string(),
                display_name: "Alpha".to_string(),
                created_at: 3,
                updated_at: 4,
            },
            RepositoryCatalogEntry {
                repository_id: CATALOG_ID_C.to_string(),
                display_name: "Beta".to_string(),
                created_at: 5,
                updated_at: 6,
            },
        ]
    );
    assert_eq!(
        result
            .issues
            .iter()
            .map(|issue| (issue.repository_id.as_deref(), issue.kind))
            .collect::<Vec<_>>(),
        vec![
            (Some(CATALOG_ID_D), CatalogIssueKind::MalformedMetadata),
            (Some(CATALOG_ID_E), CatalogIssueKind::MalformedMetadata),
            (Some(CATALOG_ID_F), CatalogIssueKind::InvalidMetadata),
            (Some(CATALOG_ID_G), CatalogIssueKind::RepositoryIdMismatch),
            (Some(CATALOG_ID_H), CatalogIssueKind::MetadataTooLarge),
            (Some(CATALOG_ID_I), CatalogIssueKind::MissingMetadata),
            (None, CatalogIssueKind::InvalidRepositoryPrefix),
            (None, CatalogIssueKind::InvalidRepositoryPrefix),
            (None, CatalogIssueKind::InvalidRepositoryPrefix),
        ]
    );
    fixture.finish(11).await;
}

#[tokio::test]
async fn catalog_list_fails_closed_on_duplicate_canonical_repository_ids() {
    let xml = format!(
        "<ListBucketResult><IsTruncated>false</IsTruncated>\
         <CommonPrefixes><Prefix>qingyu/repositories/{CATALOG_ID_A}/</Prefix></CommonPrefixes>\
         <CommonPrefixes><Prefix>qingyu/repositories/{CATALOG_ID_A}/</Prefix></CommonPrefixes>\
         </ListBucketResult>"
    );
    let fixture = HttpFixture::start(vec![expected_request(
        "GET",
        CATALOG_LIST_TARGET,
        vec![],
        FixtureResponse::ok(xml),
    )])
    .await;
    let catalog = catalog(&fixture.endpoint, 1);

    assert_eq!(
        catalog.list().await.unwrap_err().code(),
        "catalog_duplicate_repository_id"
    );
    fixture.finish(1).await;
}

#[tokio::test]
async fn catalog_list_rejects_duplicate_or_missing_prefix_fields() {
    let duplicate = format!(
        "<ListBucketResult><IsTruncated>false</IsTruncated><CommonPrefixes>\
         <Prefix>qingyu/repositories/{CATALOG_ID_A}/</Prefix>\
         <Prefix>qingyu/repositories/{CATALOG_ID_B}/</Prefix>\
         </CommonPrefixes></ListBucketResult>"
    );
    let missing =
        "<ListBucketResult><IsTruncated>false</IsTruncated><CommonPrefixes><Other/></CommonPrefixes></ListBucketResult>";
    for xml in [duplicate.as_bytes(), missing.as_bytes()] {
        let fixture = HttpFixture::start(vec![expected_request(
            "GET",
            CATALOG_LIST_TARGET,
            vec![],
            FixtureResponse::ok(xml),
        )])
        .await;
        let catalog = catalog(&fixture.endpoint, 1);
        assert!(matches!(
            catalog.list().await,
            Err(CloudError::Backend { .. })
        ));
        fixture.finish(1).await;
    }
}

#[tokio::test]
async fn catalog_read_accepts_exactly_64_kib_and_rejects_the_next_byte() {
    let mut exact = catalog_json(CATALOG_ID_A, "Notes", 1, 1);
    exact.resize(64 * 1024, b' ');
    let mut oversized = catalog_json(CATALOG_ID_A, "Notes", 1, 1);
    oversized.resize(64 * 1024 + 1, b' ');
    let target = catalog_metadata_target(CATALOG_ID_A);
    let fixture = HttpFixture::start(vec![
        expected_request("GET", target, vec![], FixtureResponse::ok(exact)),
        expected_request("GET", target, vec![], FixtureResponse::ok(oversized)),
    ])
    .await;
    let catalog = catalog(&fixture.endpoint, 1);

    assert_eq!(
        catalog.read(CATALOG_ID_A).await.unwrap().display_name,
        "Notes"
    );
    assert!(matches!(
        catalog.read(CATALOG_ID_A).await,
        Err(CloudError::ResponseTooLarge { limit: 65_536 })
    ));
    fixture.finish(2).await;
}

#[tokio::test]
async fn catalog_read_rejects_unsupported_version_and_noncanonical_stored_id() {
    let version_two = format!(
        "{{\"formatVersion\":2,\"repositoryId\":\"{CATALOG_ID_A}\",\"displayName\":\"Notes\",\"createdAt\":1,\"updatedAt\":1}}"
    );
    let uppercase_id =
        b"{\"formatVersion\":1,\"repositoryId\":\"00000000-0000-4000-8000-00000000000A\",\"displayName\":\"Notes\",\"createdAt\":1,\"updatedAt\":1}";
    let target = catalog_metadata_target(CATALOG_ID_A);
    let fixture = HttpFixture::start(vec![
        expected_request("GET", target, vec![], FixtureResponse::ok(version_two)),
        expected_request("GET", target, vec![], FixtureResponse::ok(uppercase_id)),
    ])
    .await;
    let catalog = catalog(&fixture.endpoint, 1);

    for _ in 0..2 {
        assert_eq!(
            catalog.read(CATALOG_ID_A).await.unwrap_err().code(),
            "catalog_invalid_metadata"
        );
    }
    fixture.finish(2).await;
}

#[tokio::test]
async fn catalog_create_checks_for_a_duplicate_before_writing_trimmed_metadata() {
    let metadata = catalog_json(CATALOG_ID_A, "Notes", 10, 10);
    let get_target = catalog_metadata_target(CATALOG_ID_A);
    let put_target = catalog_object_target(CATALOG_ID_A, "metadata.json");
    let mut put = expected_request("PUT", put_target, &metadata, FixtureResponse::status(200));
    put.required_headers.extend([
        ("content-type", "application/octet-stream"),
        ("cache-control", "no-cache"),
    ]);
    put.absent_headers.extend(["if-match", "if-none-match"]);
    let fixture = HttpFixture::start(vec![
        expected_request("GET", get_target, vec![], FixtureResponse::status(404)),
        put,
    ])
    .await;
    let catalog = catalog(&fixture.endpoint, 1);

    assert_eq!(
        catalog.create(CATALOG_ID_A, "  Notes  ", 10).await.unwrap(),
        RepositoryMetadata {
            format_version: 1,
            repository_id: CATALOG_ID_A.to_string(),
            display_name: "Notes".to_string(),
            created_at: 10,
            updated_at: 10,
        }
    );
    fixture.finish(2).await;
}

#[tokio::test]
async fn catalog_create_does_not_overwrite_an_existing_repository_id() {
    let fixture = HttpFixture::start(vec![expected_request(
        "GET",
        catalog_metadata_target(CATALOG_ID_A),
        vec![],
        FixtureResponse::ok(catalog_json(CATALOG_ID_A, "Existing", 1, 1)),
    )])
    .await;
    let catalog = catalog(&fixture.endpoint, 1);

    assert!(matches!(
        catalog.create(CATALOG_ID_A, "New", 2).await,
        Err(CloudError::AlreadyExists)
    ));
    fixture.finish(1).await;
}

#[tokio::test]
async fn catalog_create_rejects_escaped_metadata_over_64_kib_before_network_io() {
    let fixture = HttpFixture::start(vec![]).await;
    let catalog = catalog(&fixture.endpoint, 1);
    let escaped_name = "\"".repeat(40 * 1024);

    assert_eq!(
        catalog
            .create(CATALOG_ID_A, &escaped_name, 1)
            .await
            .unwrap_err()
            .code(),
        "catalog_metadata_too_large"
    );
    fixture.finish(0).await;
}

#[tokio::test]
async fn catalog_rename_changes_only_trimmed_name_and_updated_at() {
    let before = catalog_json(CATALOG_ID_A, "Before", 10, 20);
    let after = catalog_json(CATALOG_ID_A, "After", 10, 30);
    let mut put = expected_request(
        "PUT",
        catalog_object_target(CATALOG_ID_A, "metadata.json"),
        &after,
        FixtureResponse::status(200),
    );
    put.absent_headers.extend(["if-match", "if-none-match"]);
    let fixture = HttpFixture::start(vec![
        expected_request(
            "GET",
            catalog_metadata_target(CATALOG_ID_A),
            vec![],
            FixtureResponse::ok(before),
        ),
        put,
    ])
    .await;
    let catalog = catalog(&fixture.endpoint, 1);

    assert_eq!(
        catalog.rename(CATALOG_ID_A, "  After ", 30).await.unwrap(),
        RepositoryMetadata {
            format_version: 1,
            repository_id: CATALOG_ID_A.to_string(),
            display_name: "After".to_string(),
            created_at: 10,
            updated_at: 30,
        }
    );
    fixture.finish(2).await;
}

#[tokio::test]
async fn catalog_rename_does_not_put_escaped_metadata_over_64_kib() {
    let fixture = HttpFixture::start(vec![expected_request(
        "GET",
        catalog_metadata_target(CATALOG_ID_A),
        vec![],
        FixtureResponse::ok(catalog_json(CATALOG_ID_A, "Before", 10, 20)),
    )])
    .await;
    let catalog = catalog(&fixture.endpoint, 1);
    let escaped_name = "\"".repeat(40 * 1024);

    assert_eq!(
        catalog
            .rename(CATALOG_ID_A, &escaped_name, 30)
            .await
            .unwrap_err()
            .code(),
        "catalog_metadata_too_large"
    );
    fixture.finish(1).await;
}

#[tokio::test]
async fn catalog_delete_paginates_and_deletes_metadata_last() {
    let first_target = leak(format!(
        "/qingyu-notes?list-type=2&max-keys=1000&prefix=qingyu%2Frepositories%2F{}%2F",
        CATALOG_ID_A
    ));
    let second_target = leak(format!(
        "/qingyu-notes?continuation-token=next&list-type=2&max-keys=1000&prefix=qingyu%2Frepositories%2F{}%2F",
        CATALOG_ID_A
    ));
    let first_page = format!(
        "<ListBucketResult><IsTruncated>true</IsTruncated><NextContinuationToken>next</NextContinuationToken>\
         <Contents><Key>qingyu/repositories/{CATALOG_ID_A}/metadata.json</Key><Size>10</Size></Contents>\
         <Contents><Key>qingyu/repositories/{CATALOG_ID_A}/repo/objects/aa/one</Key><Size>11</Size></Contents>\
         </ListBucketResult>"
    );
    let second_page = format!(
        "<ListBucketResult><IsTruncated>false</IsTruncated>\
         <Contents><Key>qingyu/repositories/{CATALOG_ID_A}/repo/refs/latest</Key><Size>40</Size></Contents>\
         </ListBucketResult>"
    );
    let mut delete_object = expected_request(
        "DELETE",
        catalog_object_target(CATALOG_ID_A, "repo/objects/aa/one"),
        vec![],
        FixtureResponse::status(204),
    );
    delete_object
        .absent_headers
        .extend(["if-match", "if-none-match"]);
    let mut delete_ref = expected_request(
        "DELETE",
        catalog_object_target(CATALOG_ID_A, "repo/refs/latest"),
        vec![],
        FixtureResponse::status(204),
    );
    delete_ref
        .absent_headers
        .extend(["if-match", "if-none-match"]);
    let mut delete_metadata = expected_request(
        "DELETE",
        catalog_object_target(CATALOG_ID_A, "metadata.json"),
        vec![],
        FixtureResponse::status(204),
    );
    delete_metadata
        .absent_headers
        .extend(["if-match", "if-none-match"]);
    let fixture = HttpFixture::start(vec![
        expected_request("GET", first_target, vec![], FixtureResponse::ok(first_page)),
        expected_request(
            "GET",
            second_target,
            vec![],
            FixtureResponse::ok(second_page),
        ),
        delete_object,
        delete_ref,
        delete_metadata,
    ])
    .await;
    let catalog = catalog(&fixture.endpoint, 1);

    catalog.delete_repository(CATALOG_ID_A).await.unwrap();
    fixture.finish(5).await;
}

#[tokio::test]
async fn catalog_delete_rejects_cross_prefix_listing_before_any_delete() {
    let target = leak(format!(
        "/qingyu-notes?list-type=2&max-keys=1000&prefix=qingyu%2Frepositories%2F{}%2F",
        CATALOG_ID_A
    ));
    let page = format!(
        "<ListBucketResult><IsTruncated>false</IsTruncated>\
         <Contents><Key>qingyu/repositories/{CATALOG_ID_A}/repo/refs/latest</Key><Size>40</Size></Contents>\
         <Contents><Key>qingyu/repositories/{CATALOG_ID_B}/metadata.json</Key><Size>10</Size></Contents>\
         </ListBucketResult>"
    );
    let fixture = HttpFixture::start(vec![expected_request(
        "GET",
        target,
        vec![],
        FixtureResponse::ok(page),
    )])
    .await;
    let catalog = catalog(&fixture.endpoint, 1);

    assert!(matches!(
        catalog.delete_repository(CATALOG_ID_A).await,
        Err(CloudError::UnsafeKey)
    ));
    fixture.finish(1).await;
}

#[tokio::test]
async fn catalog_delete_removes_listed_objects_when_metadata_is_missing() {
    let target = leak(format!(
        "/qingyu-notes?list-type=2&max-keys=1000&prefix=qingyu%2Frepositories%2F{}%2F",
        CATALOG_ID_A
    ));
    let page = format!(
        "<ListBucketResult><IsTruncated>false</IsTruncated>\
         <Contents><Key>qingyu/repositories/{CATALOG_ID_A}/repo/refs/latest</Key><Size>40</Size></Contents>\
         </ListBucketResult>"
    );
    let fixture = HttpFixture::start(vec![
        expected_request("GET", target, vec![], FixtureResponse::ok(page)),
        expected_request(
            "DELETE",
            catalog_object_target(CATALOG_ID_A, "repo/refs/latest"),
            vec![],
            FixtureResponse::status(204),
        ),
    ])
    .await;
    let catalog = catalog(&fixture.endpoint, 1);

    catalog.delete_repository(CATALOG_ID_A).await.unwrap();
    fixture.finish(2).await;
}

#[tokio::test]
async fn catalog_list_propagates_auth_and_provider_failures() {
    for status in [403, 500] {
        let fixture = HttpFixture::start(vec![expected_request(
            "GET",
            CATALOG_LIST_TARGET,
            vec![],
            FixtureResponse::status(status),
        )])
        .await;
        let catalog = catalog(&fixture.endpoint, 1);
        let error = catalog.list().await.unwrap_err();
        if status == 403 {
            assert!(matches!(error, CloudError::Forbidden));
        } else {
            assert!(matches!(
                error,
                CloudError::S3Response {
                    status: 500,
                    retryable: true,
                    ..
                }
            ));
        }
        fixture.finish(1).await;
    }
}

#[tokio::test]
async fn catalog_read_reuses_retry_and_redirect_boundaries() {
    let target = catalog_metadata_target(CATALOG_ID_A);
    let fixture = HttpFixture::start(vec![
        expected_request("GET", target, vec![], FixtureResponse::status(503)),
        expected_request(
            "GET",
            target,
            vec![],
            FixtureResponse::ok(catalog_json(CATALOG_ID_A, "Notes", 1, 1)),
        ),
    ])
    .await;
    let retrying_catalog = catalog(&fixture.endpoint, 2);
    assert_eq!(
        retrying_catalog
            .read(CATALOG_ID_A)
            .await
            .unwrap()
            .display_name,
        "Notes"
    );
    fixture.finish(2).await;

    let redirect = FixtureResponse::Http {
        status: 307,
        headers: vec![("Location", "http://127.0.0.1:1/credential-leak")],
        body: vec![],
        declared_length: None,
    };
    let fixture = HttpFixture::start(vec![expected_request("GET", target, vec![], redirect)]).await;
    let redirect_catalog = catalog(&fixture.endpoint, 3);
    assert!(matches!(
        redirect_catalog.read(CATALOG_ID_A).await,
        Err(CloudError::S3Response {
            status: 307,
            retryable: false,
            ..
        })
    ));
    fixture.finish(1).await;
}

#[tokio::test]
async fn repository_cloud_list_cannot_expose_outer_catalog_metadata() {
    let target =
        "/qingyu-notes?list-type=2&max-keys=1000&prefix=qingyu%2Frepositories%2Frepo-a%2Frepo%2F";
    let xml = format!(
        "<ListBucketResult><IsTruncated>false</IsTruncated>\
         <Contents><Key>qingyu/repositories/{CATALOG_ID_A}/metadata.json</Key><Size>10</Size></Contents>\
         </ListBucketResult>"
    );
    let fixture = HttpFixture::start(vec![expected_request(
        "GET",
        target,
        vec![],
        FixtureResponse::ok(xml),
    )])
    .await;
    let cloud = cloud(&fixture.endpoint, 1);

    assert!(matches!(cloud.list("").await, Err(CloudError::UnsafeKey)));
    fixture.finish(1).await;
}

#[tokio::test]
async fn repository_cloud_list_rejects_nonempty_common_prefixes_without_a_delimiter() {
    let target =
        "/qingyu-notes?list-type=2&max-keys=1000&prefix=qingyu%2Frepositories%2Frepo-a%2Frepo%2F";
    let xml = format!(
        "<ListBucketResult><IsTruncated>false</IsTruncated><CommonPrefixes>\
         <Prefix>{REPOSITORY_PREFIX}/objects/</Prefix>\
         </CommonPrefixes></ListBucketResult>"
    );
    let fixture = HttpFixture::start(vec![expected_request(
        "GET",
        target,
        vec![],
        FixtureResponse::ok(xml),
    )])
    .await;
    let cloud = cloud(&fixture.endpoint, 1);

    assert!(matches!(cloud.list("").await, Err(CloudError::UnsafeKey)));
    fixture.finish(1).await;
}
