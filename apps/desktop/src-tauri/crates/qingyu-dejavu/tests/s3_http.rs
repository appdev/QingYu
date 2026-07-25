use qingyu_dejavu::{S3AddressingStyle, S3Connection, S3RequestSigner};
use reqwest::header::{AUTHORIZATION, CONTENT_LENGTH, HOST};
use reqwest::Method;
use time::OffsetDateTime;

const FIXED_TIME: i64 = 1_784_181_600;
const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

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
