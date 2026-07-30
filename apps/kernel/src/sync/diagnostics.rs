//! Redacted sync diagnostics shared by provider implementations.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use quick_xml::events::Event;
use quick_xml::Reader;
use reqwest::Response;
use sha2::{Digest, Sha256};

use super::backend::{
    RemoteSyncDiagnostic, RemoteSyncError, SyncFailureCategory, SyncProviderOperation,
};

const MAX_REQUEST_ID_BYTES: usize = 256;
const MAX_S3_ERROR_BODY_BYTES: usize = 64 * 1024;
const MAX_S3_ERROR_CODE_BYTES: usize = 80;

#[derive(Clone, Debug)]
pub(crate) struct SyncDiagnosticContext {
    run_id: String,
    scope: String,
}

impl SyncDiagnosticContext {
    pub(crate) fn new(run_id: impl Into<String>, scope: impl Into<String>) -> Self {
        Self {
            run_id: run_id.into(),
            scope: scope.into(),
        }
    }
}

pub(crate) fn create_sync_run_id() -> String {
    static PROCESS_STARTED_MS: OnceLock<u128> = OnceLock::new();
    static RUN_SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let process_ms = *PROCESS_STARTED_MS.get_or_init(|| now_ms);
    let sequence = RUN_SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1;
    format!("sync-{process_ms}-{now_ms}-{sequence}")
}

fn s3_object_id(context: &SyncDiagnosticContext, relative_path: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(context.run_id.as_bytes());
    digest.update([0]);
    digest.update(context.scope.as_bytes());
    digest.update([0]);
    digest.update(relative_path.as_bytes());
    format!("{:x}", digest.finalize())[..16].to_string()
}

fn safe_bounded_token(
    value: &str,
    max_bytes: usize,
    allow_base64_punctuation: bool,
) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > max_bytes || !value.is_ascii() {
        return None;
    }
    value
        .bytes()
        .all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'-' | b'_' | b'.')
                || (allow_base64_punctuation && matches!(byte, b'+' | b'/' | b'='))
        })
        .then(|| value.to_string())
}

fn safe_s3_error_code(value: &str) -> Option<String> {
    safe_bounded_token(value, MAX_S3_ERROR_CODE_BYTES, false)
}

fn safe_request_id(value: &str) -> Option<String> {
    safe_bounded_token(value, MAX_REQUEST_ID_BYTES, true)
}

fn parse_s3_error_code(bytes: &[u8]) -> Option<String> {
    if bytes.len() > MAX_S3_ERROR_BODY_BYTES {
        return None;
    }
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut inside_code = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(start)) => inside_code = start.name().as_ref() == b"Code",
            Ok(Event::Text(text)) if inside_code => {
                return safe_s3_error_code(&text.decode().ok()?);
            }
            Ok(Event::End(end)) if end.name().as_ref() == b"Code" => inside_code = false,
            Ok(Event::Eof) | Err(_) => return None,
            _ => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn diagnostic_for(
    context: &SyncDiagnosticContext,
    category: SyncFailureCategory,
    code: String,
    http_status: Option<u16>,
    method: Option<&str>,
    operation: SyncProviderOperation,
    relative_path: &str,
    provider_error_code: Option<String>,
    request_id: Option<String>,
) -> RemoteSyncDiagnostic {
    RemoteSyncDiagnostic {
        category,
        code,
        http_status,
        method: method.map(str::to_string),
        object_id: Some(s3_object_id(context, relative_path)),
        operation,
        provider_error_code,
        request_id,
        run_id: context.run_id.clone(),
        scope: context.scope.clone(),
    }
}

pub(crate) fn record_s3_request_succeeded(
    _context: &SyncDiagnosticContext,
    _method: &str,
    _operation: SyncProviderOperation,
    _relative_path: &str,
    _status: u16,
    _duration: Duration,
) {
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn record_s3_request_retrying(
    _context: &SyncDiagnosticContext,
    _method: &str,
    _operation: SyncProviderOperation,
    _relative_path: &str,
    _status: Option<u16>,
    _attempt: u8,
    _max_attempts: u8,
    _duration: Duration,
) {
}

pub(crate) fn s3_replan_required(
    context: &SyncDiagnosticContext,
    operation: SyncProviderOperation,
    method: &str,
    relative_path: &str,
    http_status: Option<u16>,
    _duration: Duration,
) -> RemoteSyncError {
    RemoteSyncError::diagnostic(diagnostic_for(
        context,
        SyncFailureCategory::Integrity,
        "s3-object-changed".to_string(),
        http_status,
        Some(method),
        operation,
        relative_path,
        None,
        None,
    ))
}

pub(crate) fn s3_transport_failure(
    context: &SyncDiagnosticContext,
    operation: SyncProviderOperation,
    method: &str,
    relative_path: &str,
    _duration: Duration,
) -> RemoteSyncError {
    RemoteSyncError::diagnostic(diagnostic_for(
        context,
        SyncFailureCategory::Transport,
        format!("s3-{}-request-failed", operation.as_str()),
        None,
        Some(method),
        operation,
        relative_path,
        None,
        None,
    ))
}

pub(crate) async fn s3_http_failure(
    context: &SyncDiagnosticContext,
    operation: SyncProviderOperation,
    method: &str,
    relative_path: &str,
    mut response: Response,
    _duration: Duration,
) -> RemoteSyncError {
    let http_status = response.status().as_u16();
    let request_id = response
        .headers()
        .get("x-amz-request-id")
        .or_else(|| response.headers().get("x-amz-requestid"))
        .and_then(|value| value.to_str().ok())
        .and_then(safe_request_id);
    let mut body = Vec::new();
    let mut oversized = false;
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) if body.len() + chunk.len() <= MAX_S3_ERROR_BODY_BYTES => {
                body.extend_from_slice(&chunk);
            }
            Ok(Some(_)) => {
                oversized = true;
                break;
            }
            Ok(None) | Err(_) => break,
        }
    }
    let provider_error_code = (!oversized).then(|| parse_s3_error_code(&body)).flatten();
    RemoteSyncError::diagnostic(diagnostic_for(
        context,
        SyncFailureCategory::Http,
        format!("s3-{}-http-failed", operation.as_str()),
        Some(http_status),
        Some(method),
        operation,
        relative_path,
        provider_error_code,
        request_id,
    ))
}

pub(crate) fn s3_integrity_failure(
    context: &SyncDiagnosticContext,
    operation: SyncProviderOperation,
    method: &str,
    relative_path: &str,
    code: &str,
    _duration: Duration,
) -> RemoteSyncError {
    RemoteSyncError::diagnostic(diagnostic_for(
        context,
        SyncFailureCategory::Integrity,
        code.to_string(),
        None,
        Some(method),
        operation,
        relative_path,
        None,
        None,
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        parse_s3_error_code, s3_object_id, safe_request_id, safe_s3_error_code,
        SyncDiagnosticContext, MAX_S3_ERROR_BODY_BYTES,
    };

    #[test]
    fn s3_metadata_keeps_only_allowlisted_bounded_values() {
        assert_eq!(
            safe_s3_error_code("AccessDenied"),
            Some("AccessDenied".into())
        );
        assert_eq!(safe_s3_error_code("bad code /secret"), None);
        assert_eq!(safe_request_id("request-123"), Some("request-123".into()));
        assert_eq!(safe_request_id(&"x".repeat(257)), None);
    }

    #[test]
    fn object_id_never_contains_the_relative_path() {
        let context = SyncDiagnosticContext::new("run-1", "notes");
        let object_id = s3_object_id(&context, "private/面试.md");
        assert!(!object_id.contains("private"));
        assert!(!object_id.contains("面试"));
        assert_eq!(object_id.len(), 16);
    }

    #[test]
    fn s3_error_parser_keeps_only_the_code() {
        let body = br#"<Error><Code>AccessDenied</Code><Message>secret</Message></Error>"#;
        assert_eq!(parse_s3_error_code(body), Some("AccessDenied".into()));
        assert_eq!(
            parse_s3_error_code(b"<Error><Code>bad code</Code></Error>"),
            None
        );
        assert_eq!(
            parse_s3_error_code(&vec![b'x'; MAX_S3_ERROR_BODY_BYTES + 1]),
            None
        );
    }
}
