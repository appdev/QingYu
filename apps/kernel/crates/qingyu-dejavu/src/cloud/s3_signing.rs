use std::fmt;

use hmac::{Hmac, Mac};
use percent_encoding::{percent_decode_str, percent_encode, AsciiSet, NON_ALPHANUMERIC};
use reqwest::header::{
    HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, HOST, IF_NONE_MATCH,
};
use reqwest::Method;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use url::Url;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use super::CloudError;

type HmacSha256 = Hmac<Sha256>;

const AWS_PERCENT_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum S3AddressingStyle {
    #[default]
    Auto,
    Path,
    VirtualHosted,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum S3TlsVerification {
    #[default]
    Verify,
    Skip,
}

#[derive(Clone)]
pub struct S3Connection {
    pub endpoint_url: Url,
    pub region: String,
    pub bucket: String,
    pub access_key_id: String,
    secret_access_key: String,
    pub addressing_style: S3AddressingStyle,
}

impl fmt::Debug for S3Connection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("S3Connection")
            .field("endpoint_url", &"[REDACTED]")
            .field("region", &self.region)
            .field("bucket", &self.bucket)
            .field("access_key_id", &"[REDACTED]")
            .field("secret_access_key", &"[REDACTED]")
            .field("addressing_style", &self.addressing_style)
            .finish()
    }
}

impl Zeroize for S3Connection {
    fn zeroize(&mut self) {
        self.access_key_id.zeroize();
        self.secret_access_key.zeroize();
    }
}

impl Drop for S3Connection {
    fn drop(&mut self) {
        self.zeroize();
    }
}

impl ZeroizeOnDrop for S3Connection {}

impl S3Connection {
    pub fn new(
        endpoint_url: &str,
        region: &str,
        bucket: &str,
        access_key_id: &str,
        secret_access_key: &str,
        addressing_style: S3AddressingStyle,
    ) -> Result<Self, CloudError> {
        let mut endpoint_url = Url::parse(endpoint_url.trim())
            .map_err(|_| CloudError::backend("s3_invalid_endpoint"))?;
        if !matches!(endpoint_url.scheme(), "http" | "https")
            || !endpoint_url.username().is_empty()
            || endpoint_url.password().is_some()
            || endpoint_url.host_str().is_none()
        {
            return Err(CloudError::backend("s3_invalid_endpoint"));
        }
        endpoint_url.set_query(None);
        endpoint_url.set_fragment(None);
        let normalized_path = endpoint_url.path().trim_end_matches('/').to_string();
        endpoint_url.set_path(&normalized_path);

        let bucket = bucket.trim();
        if bucket.is_empty() || bucket.contains('/') || bucket.contains('\\') {
            return Err(CloudError::backend("s3_invalid_bucket"));
        }
        let access_key_id = access_key_id.trim();
        if access_key_id.is_empty() || secret_access_key.is_empty() {
            return Err(CloudError::backend("s3_invalid_credentials"));
        }
        let region = region.trim();

        Ok(Self {
            endpoint_url,
            region: if region.is_empty() { "auto" } else { region }.to_string(),
            bucket: bucket.to_string(),
            access_key_id: access_key_id.to_string(),
            secret_access_key: secret_access_key.to_string(),
            addressing_style,
        })
    }

    pub fn object_url(&self, key: &str) -> Result<Url, CloudError> {
        let mut url = self.bucket_url()?;
        append_path_segments(&mut url, key.split('/'));
        Ok(url)
    }

    pub(super) fn bucket_url(&self) -> Result<Url, CloudError> {
        let mut url = self.endpoint_url.clone();
        match self.addressing_style {
            S3AddressingStyle::Auto => {
                if endpoint_uses_virtual_hosted_bucket(&url, &self.bucket) {
                    return Ok(url);
                }
                if endpoint_requires_virtual_hosted_bucket(&url) {
                    prepend_bucket_to_host(&mut url, &self.bucket)?;
                    return Ok(url);
                }
            }
            S3AddressingStyle::Path => {
                if endpoint_uses_virtual_hosted_bucket(&url, &self.bucket) {
                    return Err(CloudError::backend("s3_invalid_addressing_style"));
                }
            }
            S3AddressingStyle::VirtualHosted => {
                if !endpoint_uses_virtual_hosted_bucket(&url, &self.bucket) {
                    prepend_bucket_to_host(&mut url, &self.bucket)?;
                }
                return Ok(url);
            }
        }

        append_path_segments(&mut url, [self.bucket.as_str()]);
        Ok(url)
    }
}

#[derive(Clone)]
pub struct S3RequestSigner {
    connection: S3Connection,
}

impl fmt::Debug for S3RequestSigner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("S3RequestSigner")
            .field("connection", &self.connection)
            .finish()
    }
}

impl S3RequestSigner {
    pub fn new(connection: S3Connection) -> Self {
        Self { connection }
    }

    pub fn sign_bytes_at(
        &self,
        method: &Method,
        url: &Url,
        payload: &[u8],
        content_type: Option<&str>,
        now: OffsetDateTime,
    ) -> Result<HeaderMap, CloudError> {
        self.sign_prehashed_at(
            method,
            url,
            &sha256_hex(payload),
            u64::try_from(payload.len())
                .map_err(|_| CloudError::backend("s3_payload_too_large"))?,
            content_type,
            now,
        )
    }

    pub(crate) fn sign_bytes_if_absent_at(
        &self,
        method: &Method,
        url: &Url,
        payload: &[u8],
        content_type: Option<&str>,
        now: OffsetDateTime,
    ) -> Result<HeaderMap, CloudError> {
        self.sign_prehashed_with_condition_at(
            method,
            url,
            &sha256_hex(payload),
            u64::try_from(payload.len())
                .map_err(|_| CloudError::backend("s3_payload_too_large"))?,
            content_type,
            true,
            now,
        )
    }

    pub fn sign_prehashed_at(
        &self,
        method: &Method,
        url: &Url,
        payload_hash: &str,
        content_length: u64,
        content_type: Option<&str>,
        now: OffsetDateTime,
    ) -> Result<HeaderMap, CloudError> {
        if payload_hash.len() != 64
            || !payload_hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err(CloudError::backend("s3_invalid_payload_hash"));
        }
        self.sign_prehashed_with_condition_at(
            method,
            url,
            payload_hash,
            content_length,
            content_type,
            false,
            now,
        )
    }

    pub(crate) fn sign_prehashed_if_absent_at(
        &self,
        method: &Method,
        url: &Url,
        payload_hash: &str,
        content_length: u64,
        content_type: Option<&str>,
        now: OffsetDateTime,
    ) -> Result<HeaderMap, CloudError> {
        self.sign_prehashed_with_condition_at(
            method,
            url,
            payload_hash,
            content_length,
            content_type,
            true,
            now,
        )
    }

    fn sign_prehashed_with_condition_at(
        &self,
        method: &Method,
        url: &Url,
        payload_hash: &str,
        content_length: u64,
        content_type: Option<&str>,
        if_absent: bool,
        now: OffsetDateTime,
    ) -> Result<HeaderMap, CloudError> {
        if payload_hash.len() != 64
            || !payload_hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err(CloudError::backend("s3_invalid_payload_hash"));
        }
        self.sign_at(
            method,
            url,
            payload_hash,
            Some(content_length),
            content_type,
            if_absent,
            now,
        )
    }

    pub fn sign_empty_at(
        &self,
        method: &Method,
        url: &Url,
        now: OffsetDateTime,
    ) -> Result<HeaderMap, CloudError> {
        self.sign_at(method, url, &sha256_hex(&[]), None, None, false, now)
    }

    fn sign_at(
        &self,
        method: &Method,
        url: &Url,
        payload_hash: &str,
        content_length: Option<u64>,
        content_type: Option<&str>,
        if_absent: bool,
        now: OffsetDateTime,
    ) -> Result<HeaderMap, CloudError> {
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
        let content_type = content_type.map(sigv4_trim_all);
        let mut signed = vec![
            ("host", host.clone()),
            ("x-amz-content-sha256", payload_hash.to_string()),
            ("x-amz-date", amz_date.clone()),
        ];
        if let Some(content_type) = &content_type {
            signed.push(("content-type", content_type.clone()));
        }
        if if_absent {
            signed.push(("if-none-match", "*".to_string()));
        }
        signed.sort_unstable_by_key(|(name, _)| *name);
        let canonical_headers = signed
            .iter()
            .map(|(name, value)| format!("{name}:{value}\n"))
            .collect::<String>();
        let signed_headers = signed
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>()
            .join(";");
        let canonical_request = format!(
            "{}\n{}\n{}\n{canonical_headers}\n{signed_headers}\n{payload_hash}",
            method.as_str(),
            canonical_uri(url),
            canonical_query(url)
        );
        let credential_scope = format!("{date}/{}/s3/aws4_request", self.connection.region);
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{amz_date}\n{credential_scope}\n{}",
            sha256_hex(canonical_request.as_bytes())
        );
        let signing_key = signing_key(
            &self.connection.secret_access_key,
            &date,
            &self.connection.region,
        )?;
        let signature = hex_lower(&hmac_sha256(&signing_key, string_to_sign.as_bytes())?);
        let authorization = Zeroizing::new(format!(
            "AWS4-HMAC-SHA256 Credential={}/{credential_scope}, SignedHeaders={signed_headers}, Signature={signature}",
            self.connection.access_key_id
        ));

        let mut headers = HeaderMap::new();
        headers.insert(HOST, header_value(&host)?);
        headers.insert("x-amz-content-sha256", header_value(payload_hash)?);
        headers.insert("x-amz-date", header_value(&amz_date)?);
        headers.insert(AUTHORIZATION, header_value(&authorization)?);
        if let Some(content_type) = content_type {
            headers.insert(CONTENT_TYPE, header_value(&content_type)?);
        }
        if if_absent {
            headers.insert(IF_NONE_MATCH, HeaderValue::from_static("*"));
        }
        if let Some(content_length) = content_length {
            headers.insert(CONTENT_LENGTH, header_value(&content_length.to_string())?);
        }
        Ok(headers)
    }
}

fn append_path_segments<'a>(url: &mut Url, segments: impl IntoIterator<Item = &'a str>) {
    let mut path = canonical_uri(url);
    if path == "/" {
        path.clear();
    } else {
        while path.ends_with('/') {
            path.pop();
        }
    }
    for segment in segments {
        path.push('/');
        path.push_str(&aws_percent_encode(segment));
    }
    url.set_path(&path);
}

fn prepend_bucket_to_host(url: &mut Url, bucket: &str) -> Result<(), CloudError> {
    let host = url
        .host_str()
        .ok_or_else(|| CloudError::backend("s3_invalid_endpoint"))?
        .to_string();
    url.set_host(Some(&format!("{bucket}.{host}")))
        .map_err(|_| CloudError::backend("s3_invalid_endpoint"))
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

fn canonical_uri(url: &Url) -> String {
    let path = if url.path().is_empty() {
        "/"
    } else {
        url.path()
    };
    path.split('/')
        .map(|segment| {
            let decoded = percent_decode_str(segment).collect::<Vec<_>>();
            percent_encode(&decoded, AWS_PERCENT_ENCODE_SET).to_string()
        })
        .collect::<Vec<_>>()
        .join("/")
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
    percent_encode(value.as_bytes(), AWS_PERCENT_ENCODE_SET).to_string()
}

fn sigv4_trim_all(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    let mut pending_space = false;
    for character in value.chars() {
        if matches!(character, ' ' | '\t') {
            pending_space = !normalized.is_empty();
        } else {
            if pending_space {
                normalized.push(' ');
                pending_space = false;
            }
            normalized.push(character);
        }
    }
    normalized
}

fn s3_host(url: &Url) -> Result<String, CloudError> {
    let host = url
        .host_str()
        .ok_or_else(|| CloudError::backend("s3_invalid_endpoint"))?;
    Ok(match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    })
}

fn signing_key(secret: &str, date: &str, region: &str) -> Result<Zeroizing<Vec<u8>>, CloudError> {
    let mut prefixed_secret = Zeroizing::new(Vec::with_capacity(4 + secret.len()));
    prefixed_secret.extend_from_slice(b"AWS4");
    prefixed_secret.extend_from_slice(secret.as_bytes());
    let date_key = hmac_sha256(&prefixed_secret, date.as_bytes())?;
    let region_key = hmac_sha256(&date_key, region.as_bytes())?;
    let service_key = hmac_sha256(&region_key, b"s3")?;
    hmac_sha256(&service_key, b"aws4_request")
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex_lower(&Sha256::digest(bytes))
}

fn hmac_sha256(key: &[u8], bytes: &[u8]) -> Result<Zeroizing<Vec<u8>>, CloudError> {
    let mut mac =
        HmacSha256::new_from_slice(key).map_err(|_| CloudError::backend("s3_signing_failed"))?;
    mac.update(bytes);
    Ok(Zeroizing::new(mac.finalize().into_bytes().to_vec()))
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

fn header_value(value: &str) -> Result<HeaderValue, CloudError> {
    HeaderValue::from_str(value).map_err(|_| CloudError::backend("s3_invalid_header"))
}

#[cfg(test)]
mod credential_lifecycle_tests {
    use reqwest::{header::AUTHORIZATION, Method};
    use time::OffsetDateTime;
    use zeroize::{Zeroize, ZeroizeOnDrop};

    use super::{signing_key, S3AddressingStyle, S3Connection, S3RequestSigner};

    fn assert_zeroizes_on_drop<T: ZeroizeOnDrop>(_value: &T) {}

    #[test]
    fn owned_s3_credentials_are_explicitly_zeroizable_and_zeroize_on_drop() {
        let mut connection = S3Connection::new(
            "https://s3.example.test/private-tenant-token",
            "us-east-1",
            "notes",
            "private-access-key",
            "private-secret-key",
            S3AddressingStyle::Path,
        )
        .expect("S3 connection");

        assert_zeroizes_on_drop(&connection);
        let debug = format!("{connection:?}");
        assert!(!debug.contains("private-access-key"));
        assert!(!debug.contains("private-secret-key"));
        assert!(!debug.contains("private-tenant-token"));

        connection.zeroize();

        assert!(connection.access_key_id.is_empty());
        assert!(connection.secret_access_key.is_empty());
    }

    #[test]
    fn every_owned_sigv4_derived_key_zeroizes_on_scope_exit() {
        let mut derived =
            signing_key("private-secret-key", "20260730", "us-east-1").expect("signing key");

        assert_zeroizes_on_drop(&derived);
        assert!(!derived.is_empty());

        derived.zeroize();

        assert!(derived.is_empty());
    }

    #[test]
    fn conditional_create_signs_if_none_match() {
        let connection = S3Connection::new(
            "https://s3.example.test",
            "us-east-1",
            "notes",
            "access-key",
            "secret-key",
            S3AddressingStyle::Path,
        )
        .expect("S3 connection");
        let url = connection.object_url("repo/metadata.json").unwrap();
        let headers = S3RequestSigner::new(connection)
            .sign_bytes_if_absent_at(
                &Method::PUT,
                &url,
                b"metadata",
                Some("application/octet-stream"),
                OffsetDateTime::from_unix_timestamp(1_784_181_600).unwrap(),
            )
            .unwrap();

        assert_eq!(headers.get("if-none-match").unwrap(), "*");
        assert!(headers
            .get(AUTHORIZATION)
            .unwrap()
            .to_str()
            .unwrap()
            .contains(
                "SignedHeaders=content-type;host;if-none-match;x-amz-content-sha256;x-amz-date"
            ));
    }
}
