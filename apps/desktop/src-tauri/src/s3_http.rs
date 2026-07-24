use std::fmt;

use hmac::{Hmac, Mac};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, HOST};
use reqwest::{Method, Url};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

use crate::sync_config::model::S3AddressingStyle;

type HmacSha256 = Hmac<Sha256>;

pub(crate) const S3_LOGICAL_EMPTY_HEADER: &str = "x-amz-meta-qingyu-logical-empty";
pub(crate) const S3_LOGICAL_EMPTY_SENTINEL: &[u8] = b"\0";

#[derive(Clone)]
pub(crate) struct S3Connection {
    pub(crate) access_key_id: String,
    pub(crate) addressing_style: S3AddressingStyle,
    pub(crate) bucket: String,
    pub(crate) endpoint_url: Url,
    pub(crate) region: String,
    pub(crate) secret_access_key: String,
}

impl fmt::Debug for S3Connection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("S3Connection")
            .field("access_key_id", &"[REDACTED]")
            .field("addressing_style", &self.addressing_style)
            .field("bucket", &self.bucket)
            .field("endpoint_url", &self.endpoint_url)
            .field("region", &self.region)
            .field("secret_access_key", &"[REDACTED]")
            .finish()
    }
}

impl S3Connection {
    #[cfg(test)]
    pub(crate) fn new(
        endpoint_url: &str,
        region: &str,
        bucket: &str,
        access_key_id: &str,
        secret_access_key: &str,
    ) -> Result<Self, String> {
        Self::new_with_addressing_style(
            endpoint_url,
            region,
            bucket,
            access_key_id,
            secret_access_key,
            S3AddressingStyle::Auto,
        )
    }

    pub(crate) fn new_with_addressing_style(
        endpoint_url: &str,
        region: &str,
        bucket: &str,
        access_key_id: &str,
        secret_access_key: &str,
        addressing_style: S3AddressingStyle,
    ) -> Result<Self, String> {
        let mut endpoint_url = Url::parse(endpoint_url.trim())
            .map_err(|_| "S3 endpoint URL is invalid".to_string())?;
        if !matches!(endpoint_url.scheme(), "http" | "https") {
            return Err("Only HTTP and HTTPS S3 endpoint URLs are supported".to_string());
        }
        if !endpoint_url.username().is_empty() || endpoint_url.password().is_some() {
            return Err("S3 endpoint URL must not contain userinfo".to_string());
        }
        if endpoint_url.host_str().is_none() {
            return Err("S3 endpoint host is required".to_string());
        }
        endpoint_url.set_query(None);
        endpoint_url.set_fragment(None);
        let normalized_path = endpoint_url.path().trim_end_matches('/').to_string();
        endpoint_url.set_path(&normalized_path);

        let bucket = bucket.trim();
        if bucket.is_empty() || bucket.contains('/') || bucket.contains('\\') {
            return Err("S3 bucket is invalid".to_string());
        }
        let region = required_trimmed(region, "S3 region")?;
        let access_key_id = required_trimmed(access_key_id, "S3 access key ID")?;
        if secret_access_key.is_empty() {
            return Err("S3 secret access key is required".to_string());
        }

        Ok(Self {
            access_key_id,
            addressing_style,
            bucket: bucket.to_string(),
            endpoint_url,
            region,
            secret_access_key: secret_access_key.to_string(),
        })
    }
}

#[derive(Clone, Copy)]
pub(crate) enum S3Payload<'a> {
    Bytes(&'a [u8]),
    Empty,
    LogicalEmpty,
}

pub(crate) fn s3_bucket_url(connection: &S3Connection) -> Result<Url, String> {
    let mut url = connection.endpoint_url.clone();
    match connection.addressing_style {
        S3AddressingStyle::Auto => {
            if endpoint_uses_virtual_hosted_bucket(&url, &connection.bucket) {
                return Ok(url);
            }
            if endpoint_requires_virtual_hosted_bucket(&url) {
                prepend_bucket_to_host(&mut url, &connection.bucket)?;
                return Ok(url);
            }
        }
        S3AddressingStyle::Path => {
            if endpoint_uses_virtual_hosted_bucket(&url, &connection.bucket) {
                return Err(
                    "Path-style S3 endpoint must not include the bucket in its host".to_string(),
                );
            }
        }
        S3AddressingStyle::VirtualHosted => {
            if !endpoint_uses_virtual_hosted_bucket(&url, &connection.bucket) {
                prepend_bucket_to_host(&mut url, &connection.bucket)?;
            }
            return Ok(url);
        }
    }

    append_path_segments(&mut url, &[connection.bucket.as_str()])?;
    Ok(url)
}

fn prepend_bucket_to_host(url: &mut Url, bucket: &str) -> Result<(), String> {
    let host = url
        .host_str()
        .ok_or_else(|| "S3 endpoint host is required".to_string())?
        .to_string();
    url.set_host(Some(&format!("{bucket}.{host}")))
        .map_err(|_| "S3 endpoint host is invalid".to_string())
}

pub(crate) fn s3_object_url(
    connection: &S3Connection,
    object_segments: &[String],
) -> Result<Url, String> {
    let mut url = s3_bucket_url(connection)?;
    let segments = object_segments
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    append_path_segments(&mut url, &segments)?;
    Ok(url)
}

pub(crate) fn signed_s3_headers(
    method: &Method,
    url: &Url,
    payload: S3Payload<'_>,
    content_type: Option<&str>,
    connection: &S3Connection,
    now: OffsetDateTime,
) -> Result<HeaderMap, String> {
    let (payload_bytes, content_length, logical_empty) = match payload {
        S3Payload::Bytes(bytes) => (bytes, Some(bytes.len()), false),
        S3Payload::Empty => (&[][..], None, false),
        S3Payload::LogicalEmpty => (S3_LOGICAL_EMPTY_SENTINEL, Some(1), true),
    };
    let payload_hash = sha256_hex(payload_bytes);
    let date = format!(
        "{:04}{:02}{:02}",
        now.year(),
        u8::from(now.month()),
        now.day()
    );
    let amz_date = format!(
        "{date}T{:02}{:02}{:02}Z",
        now.hour(),
        now.minute(),
        now.second()
    );
    let host = s3_host(url)?;
    let (canonical_headers, signed_headers) = match (content_type, logical_empty) {
        (Some(content_type), true) => (
            format!(
                "content-type:{}\nhost:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}\n{S3_LOGICAL_EMPTY_HEADER}:1\n",
                content_type.trim()
            ),
            "content-type;host;x-amz-content-sha256;x-amz-date;x-amz-meta-qingyu-logical-empty",
        ),
        (Some(content_type), false) => (
            format!(
                "content-type:{}\nhost:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}\n",
                content_type.trim()
            ),
            "content-type;host;x-amz-content-sha256;x-amz-date",
        ),
        (None, true) => (
            format!(
                "host:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}\n{S3_LOGICAL_EMPTY_HEADER}:1\n"
            ),
            "host;x-amz-content-sha256;x-amz-date;x-amz-meta-qingyu-logical-empty",
        ),
        (None, false) => (
            format!(
                "host:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}\n"
            ),
            "host;x-amz-content-sha256;x-amz-date",
        ),
    };
    let canonical_request = format!(
        "{}\n{}\n{}\n{canonical_headers}\n{signed_headers}\n{payload_hash}",
        method.as_str(),
        canonical_uri(url),
        canonical_query(url)
    );
    let credential_scope = format!("{date}/{}/s3/aws4_request", connection.region);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{credential_scope}\n{}",
        sha256_hex(canonical_request.as_bytes())
    );
    let signing_key = s3_signing_key(&connection.secret_access_key, &date, &connection.region);
    let signature = hex_lower(&hmac_sha256(&signing_key, string_to_sign.as_bytes())?);
    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={}/{credential_scope}, SignedHeaders={signed_headers}, Signature={signature}",
        connection.access_key_id
    );

    let mut headers = HeaderMap::new();
    headers.insert(HOST, header_value(&host)?);
    headers.insert("x-amz-content-sha256", header_value(&payload_hash)?);
    headers.insert("x-amz-date", header_value(&amz_date)?);
    if logical_empty {
        headers.insert(S3_LOGICAL_EMPTY_HEADER, HeaderValue::from_static("1"));
    }
    headers.insert(AUTHORIZATION, header_value(&authorization)?);
    if let Some(content_type) = content_type {
        headers.insert(CONTENT_TYPE, header_value(content_type.trim())?);
    }
    if let Some(content_length) = content_length {
        headers.insert(CONTENT_LENGTH, header_value(&content_length.to_string())?);
    }
    Ok(headers)
}

fn append_path_segments(url: &mut Url, segments: &[&str]) -> Result<(), String> {
    let mut path = url
        .path_segments_mut()
        .map_err(|_| "S3 endpoint URL cannot be used as a base URL".to_string())?;
    path.pop_if_empty();
    for segment in segments {
        path.push(segment);
    }
    Ok(())
}

fn endpoint_uses_virtual_hosted_bucket(url: &Url, bucket: &str) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    let bucket = bucket.to_ascii_lowercase();
    host == bucket || host.starts_with(&format!("{bucket}."))
}

fn endpoint_requires_virtual_hosted_bucket(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    (host.ends_with(".aliyuncs.com") && host.starts_with("oss-"))
        || (host.ends_with(".myqcloud.com") && host.starts_with("cos."))
        || host.contains(".digitaloceanspaces.com")
        || host.ends_with(".cwobject.com")
        || host == "cwobject.com"
        || host.contains(".myhuaweicloud.com")
}

fn canonical_uri(url: &Url) -> &str {
    if url.path().is_empty() {
        "/"
    } else {
        url.path()
    }
}

fn canonical_query(url: &Url) -> String {
    let mut pairs = url
        .query_pairs()
        .map(|(name, value)| (aws_percent_encode(&name), aws_percent_encode(&value)))
        .collect::<Vec<_>>();
    pairs.sort();
    pairs
        .into_iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("&")
}

fn aws_percent_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(*byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn required_trimmed(value: &str, label: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(format!("{label} is required"));
    }
    Ok(value.to_string())
}

fn s3_host(url: &Url) -> Result<String, String> {
    let host = url
        .host_str()
        .ok_or_else(|| "S3 endpoint host is required".to_string())?;
    Ok(match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    })
}

fn s3_signing_key(secret_access_key: &str, date: &str, region: &str) -> Vec<u8> {
    let date_key = hmac_sha256_unchecked(
        format!("AWS4{secret_access_key}").as_bytes(),
        date.as_bytes(),
    );
    let region_key = hmac_sha256_unchecked(&date_key, region.as_bytes());
    let service_key = hmac_sha256_unchecked(&region_key, b"s3");
    hmac_sha256_unchecked(&service_key, b"aws4_request")
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex_lower(&Sha256::digest(bytes))
}

fn hmac_sha256(key: &[u8], bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut mac = HmacSha256::new_from_slice(key).map_err(|error| error.to_string())?;
    mac.update(bytes);
    Ok(mac.finalize().into_bytes().to_vec())
}

fn hmac_sha256_unchecked(key: &[u8], bytes: &[u8]) -> Vec<u8> {
    hmac_sha256(key, bytes).unwrap_or_default()
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

fn header_value(value: &str) -> Result<HeaderValue, String> {
    HeaderValue::from_str(value).map_err(|_| "S3 signing header is invalid".to_string())
}

#[cfg(test)]
mod tests {
    use reqwest::{Method, Url};
    use time::OffsetDateTime;

    use crate::sync_config::model::S3AddressingStyle;

    use super::{
        canonical_query, s3_bucket_url, s3_object_url, signed_s3_headers, S3Connection, S3Payload,
    };

    fn connection() -> S3Connection {
        S3Connection::new(
            "https://s3.example.test",
            "us-east-1",
            "markra-notes",
            "test-key",
            "test-secret",
        )
        .expect("S3 connection")
    }

    #[test]
    fn connection_debug_output_redacts_signing_credentials() {
        let connection = S3Connection::new(
            "https://s3.example.test",
            "us-east-1",
            "notes",
            "private-access-key",
            "private-secret-key",
        )
        .unwrap();

        let output = format!("{connection:?}");

        assert!(!output.contains("private-access-key"));
        assert!(!output.contains("private-secret-key"));
        assert!(output.contains("[REDACTED]"));
    }

    #[test]
    fn rejects_s3_endpoint_url_userinfo() {
        for endpoint in [
            "https://embedded-user@s3.example.test",
            "https://:embedded-password@s3.example.test",
        ] {
            assert!(
                S3Connection::new(endpoint, "us-east-1", "notes", "key", "secret").is_err(),
                "S3 endpoint URL userinfo must be rejected"
            );
        }
    }

    #[test]
    fn signs_sorted_list_objects_query() {
        let url = Url::parse(
            "https://s3.example.test/markra-notes?prefix=notes%2Fpersonal%2F&list-type=2",
        )
        .expect("list URL");
        assert_eq!(
            canonical_query(&url),
            "list-type=2&prefix=notes%2Fpersonal%2F"
        );
        let headers = signed_s3_headers(
            &Method::GET,
            &url,
            S3Payload::Empty,
            None,
            &connection(),
            OffsetDateTime::from_unix_timestamp(1_784_181_600).expect("fixed timestamp"),
        )
        .expect("signed headers");

        assert_eq!(headers.get("x-amz-date").unwrap(), "20260716T060000Z");
        assert!(headers
            .get(reqwest::header::AUTHORIZATION)
            .unwrap()
            .to_str()
            .unwrap()
            .contains("SignedHeaders=host;x-amz-content-sha256;x-amz-date"));
    }

    #[test]
    fn builds_path_style_and_percent_encoded_object_urls() {
        assert_eq!(
            s3_bucket_url(&connection()).unwrap().as_str(),
            "https://s3.example.test/markra-notes"
        );
        assert_eq!(
            s3_object_url(
                &connection(),
                &["notes".to_string(), "空 格.md".to_string()]
            )
            .unwrap()
            .as_str(),
            "https://s3.example.test/markra-notes/notes/%E7%A9%BA%20%E6%A0%BC.md"
        );
    }

    #[test]
    fn builds_required_and_existing_virtual_hosted_urls() {
        let aliyun = S3Connection::new(
            "https://oss-cn-hangzhou.aliyuncs.com",
            "cn-hangzhou",
            "notes",
            "key",
            "secret",
        )
        .unwrap();
        assert_eq!(
            s3_bucket_url(&aliyun).unwrap().as_str(),
            "https://notes.oss-cn-hangzhou.aliyuncs.com/"
        );
        let existing = S3Connection::new(
            "https://notes.oss-cn-hangzhou.aliyuncs.com",
            "cn-hangzhou",
            "notes",
            "key",
            "secret",
        )
        .unwrap();
        assert_eq!(
            s3_bucket_url(&existing).unwrap().as_str(),
            "https://notes.oss-cn-hangzhou.aliyuncs.com/"
        );
    }

    #[test]
    fn explicit_addressing_style_overrides_endpoint_auto_detection() {
        let path = S3Connection::new_with_addressing_style(
            "https://oss-cn-hangzhou.aliyuncs.com",
            "cn-hangzhou",
            "notes",
            "key",
            "secret",
            S3AddressingStyle::Path,
        )
        .unwrap();
        assert_eq!(
            s3_bucket_url(&path).unwrap().as_str(),
            "https://oss-cn-hangzhou.aliyuncs.com/notes"
        );

        let virtual_hosted = S3Connection::new_with_addressing_style(
            "https://s3.example.test",
            "us-east-1",
            "notes",
            "key",
            "secret",
            S3AddressingStyle::VirtualHosted,
        )
        .unwrap();
        assert_eq!(
            s3_bucket_url(&virtual_hosted).unwrap().as_str(),
            "https://notes.s3.example.test/"
        );
    }

    #[test]
    fn path_style_rejects_an_already_bucket_qualified_endpoint() {
        let connection = S3Connection::new_with_addressing_style(
            "https://notes.s3.example.test",
            "us-east-1",
            "notes",
            "key",
            "secret",
            S3AddressingStyle::Path,
        )
        .unwrap();

        assert_eq!(
            s3_bucket_url(&connection).unwrap_err(),
            "Path-style S3 endpoint must not include the bucket in its host"
        );
    }

    #[test]
    fn signs_put_content_type_and_delete_without_it() {
        let url = s3_object_url(&connection(), &["note.md".to_string()]).unwrap();
        let now = OffsetDateTime::from_unix_timestamp(1_784_181_600).unwrap();
        let put = signed_s3_headers(
            &Method::PUT,
            &url,
            S3Payload::Bytes(b"hello"),
            Some("text/markdown"),
            &connection(),
            now,
        )
        .unwrap();
        assert!(put.contains_key(reqwest::header::CONTENT_TYPE));
        let delete = signed_s3_headers(
            &Method::DELETE,
            &url,
            S3Payload::Empty,
            None,
            &connection(),
            now,
        )
        .unwrap();
        assert!(!delete.contains_key(reqwest::header::CONTENT_TYPE));
    }

    #[test]
    fn signs_zero_length_put_with_explicit_content_length() {
        let url = s3_object_url(&connection(), &["empty.md".to_string()]).unwrap();
        let headers = signed_s3_headers(
            &Method::PUT,
            &url,
            S3Payload::Bytes(b""),
            Some("application/octet-stream"),
            &connection(),
            OffsetDateTime::from_unix_timestamp(1_784_181_600).unwrap(),
        )
        .unwrap();

        assert_eq!(headers.get(reqwest::header::CONTENT_LENGTH).unwrap(), "0");
    }

    #[test]
    fn signs_logical_empty_marker_and_one_byte_sentinel() {
        let url = s3_object_url(&connection(), &["empty.md".to_string()]).unwrap();
        let headers = signed_s3_headers(
            &Method::PUT,
            &url,
            S3Payload::LogicalEmpty,
            Some("application/octet-stream"),
            &connection(),
            OffsetDateTime::from_unix_timestamp(1_784_181_600).unwrap(),
        )
        .unwrap();

        assert_eq!(headers.get(reqwest::header::CONTENT_LENGTH).unwrap(), "1");
        assert_eq!(headers.get(super::S3_LOGICAL_EMPTY_HEADER).unwrap(), "1");
        assert!(headers
            .get(reqwest::header::AUTHORIZATION)
            .unwrap()
            .to_str()
            .unwrap()
            .contains("x-amz-date;x-amz-meta-qingyu-logical-empty"));
    }
}
