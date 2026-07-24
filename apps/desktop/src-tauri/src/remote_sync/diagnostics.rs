use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use quick_xml::events::Event;
use quick_xml::Reader;
use reqwest::Response;
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::backend::{
    RemoteSyncDiagnostic, RemoteSyncError, SyncFailureCategory, SyncProviderOperation,
};
use crate::sync_config::model::SyncProvider;
use crate::sync_config::status::{SyncSummary, SyncTrigger};

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
    let now_ms = unix_timestamp_ms();
    let process_ms = *PROCESS_STARTED_MS.get_or_init(|| now_ms);
    let sequence = RUN_SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1;
    format!("sync-{process_ms}-{now_ms}-{sequence}")
}

fn unix_timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

pub(crate) fn s3_object_id(context: &SyncDiagnosticContext, relative_path: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(context.run_id.as_bytes());
    digest.update([0]);
    digest.update(context.scope.as_bytes());
    digest.update([0]);
    digest.update(relative_path.as_bytes());
    format!("{:x}", digest.finalize())[..16].to_string()
}

pub(crate) fn diagnostics_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        diagnostics_enabled_from_value(std::env::var("QINGYU_SYNC_DIAGNOSTICS").ok().as_deref())
    })
}

fn diagnostics_enabled_from_value(value: Option<&str>) -> bool {
    value == Some("1")
}

fn safe_s3_error_code(value: &str) -> Option<String> {
    safe_bounded_token(value, MAX_S3_ERROR_CODE_BYTES, false)
}

fn safe_request_id(value: &str) -> Option<String> {
    safe_bounded_token(value, MAX_REQUEST_ID_BYTES, true)
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
    let safe = value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'_' | b'.')
            || (allow_base64_punctuation && matches!(byte, b'+' | b'/' | b'='))
    });
    safe.then(|| value.to_string())
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
            Ok(Event::Start(start)) => {
                inside_code = start.name().as_ref() == b"Code";
            }
            Ok(Event::Text(text)) if inside_code => {
                return safe_s3_error_code(&text.decode().ok()?);
            }
            Ok(Event::End(end)) if end.name().as_ref() == b"Code" => {
                inside_code = false;
            }
            Ok(Event::Eof) | Err(_) => return None,
            _ => {}
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct S3RequestLogRecord<'a> {
    category: &'a str,
    code: &'a str,
    duration_ms: u128,
    event: &'a str,
    http_status: Option<u16>,
    method: Option<&'a str>,
    object_id: Option<&'a str>,
    operation: &'a str,
    provider: &'static str,
    provider_error_code: Option<&'a str>,
    request_id: Option<&'a str>,
    run_id: &'a str,
    scope: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct S3RetryLogRecord<'a> {
    attempt: u8,
    category: &'a str,
    code: &'a str,
    duration_ms: u128,
    event: &'static str,
    http_status: Option<u16>,
    max_attempts: u8,
    method: &'a str,
    object_id: &'a str,
    operation: &'a str,
    provider: &'static str,
    run_id: &'a str,
    scope: &'a str,
}

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

fn request_log_record<'a>(
    diagnostic: &'a RemoteSyncDiagnostic,
    duration: Duration,
    event: &'a str,
) -> S3RequestLogRecord<'a> {
    S3RequestLogRecord {
        category: diagnostic.category.as_str(),
        code: &diagnostic.code,
        duration_ms: duration.as_millis(),
        event,
        http_status: diagnostic.http_status,
        method: diagnostic.method.as_deref(),
        object_id: diagnostic.object_id.as_deref(),
        operation: diagnostic.operation.as_str(),
        provider: "s3",
        provider_error_code: diagnostic.provider_error_code.as_deref(),
        request_id: diagnostic.request_id.as_deref(),
        run_id: &diagnostic.run_id,
        scope: &diagnostic.scope,
    }
}

fn serialize_log_record(record: &S3RequestLogRecord<'_>) -> String {
    serde_json::to_string(record).unwrap_or_else(|_| "{}".to_string())
}

fn record_s3_failure(diagnostic: &RemoteSyncDiagnostic, duration: Duration) {
    let record = request_log_record(diagnostic, duration, "s3-request-failed");
    tauri_plugin_log::log::error!(
        target: "qingyu::sync",
        "S3 request failed {}",
        serialize_log_record(&record)
    );
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SyncLifecycleLogRecord<'a> {
    category: Option<&'a str>,
    code: Option<&'a str>,
    duration_ms: u128,
    event: &'a str,
    provider: &'a str,
    retained_source_directory: Option<bool>,
    run_id: &'a str,
    summary: Option<&'a SyncSummary>,
    trigger: &'a str,
}

fn provider_name(provider: SyncProvider) -> &'static str {
    match provider {
        SyncProvider::S3 => "s3",
        SyncProvider::Webdav => "webdav",
    }
}

fn trigger_name(trigger: SyncTrigger) -> &'static str {
    match trigger {
        SyncTrigger::AppLaunch => "app-launch",
        SyncTrigger::Interval => "interval",
        SyncTrigger::Manual => "manual",
        SyncTrigger::Save => "save",
        SyncTrigger::SettingsExit => "settings-exit",
    }
}

fn serialize_lifecycle_record(record: &SyncLifecycleLogRecord<'_>) -> String {
    serde_json::to_string(record).unwrap_or_else(|_| "{}".to_string())
}

pub(crate) fn record_sync_started(
    run_id: &str,
    provider: SyncProvider,
    trigger: SyncTrigger,
    retained_source_directory: bool,
) {
    let record = SyncLifecycleLogRecord {
        category: None,
        code: None,
        duration_ms: 0,
        event: "sync-started",
        provider: provider_name(provider),
        retained_source_directory: Some(retained_source_directory),
        run_id,
        summary: None,
        trigger: trigger_name(trigger),
    };
    tauri_plugin_log::log::info!(
        target: "qingyu::sync",
        "Application synchronization started {}",
        serialize_lifecycle_record(&record)
    );
}

pub(crate) fn record_sync_succeeded(
    run_id: &str,
    provider: SyncProvider,
    trigger: SyncTrigger,
    summary: &SyncSummary,
    duration: Duration,
) {
    let record = SyncLifecycleLogRecord {
        category: None,
        code: None,
        duration_ms: duration.as_millis(),
        event: "sync-succeeded",
        provider: provider_name(provider),
        retained_source_directory: None,
        run_id,
        summary: Some(summary),
        trigger: trigger_name(trigger),
    };
    tauri_plugin_log::log::info!(
        target: "qingyu::sync",
        "Application synchronization completed {}",
        serialize_lifecycle_record(&record)
    );
}

pub(crate) fn record_sync_failed(
    run_id: &str,
    provider: SyncProvider,
    trigger: SyncTrigger,
    code: &str,
    diagnostic: Option<&RemoteSyncDiagnostic>,
    summary: &SyncSummary,
    duration: Duration,
) {
    let record = SyncLifecycleLogRecord {
        category: Some(
            diagnostic
                .map(|details| details.category.as_str())
                .unwrap_or(SyncFailureCategory::Local.as_str()),
        ),
        code: Some(code),
        duration_ms: duration.as_millis(),
        event: "sync-failed",
        provider: provider_name(provider),
        retained_source_directory: None,
        run_id,
        summary: Some(summary),
        trigger: trigger_name(trigger),
    };
    tauri_plugin_log::log::error!(
        target: "qingyu::sync",
        "Application synchronization failed {}",
        serialize_lifecycle_record(&record)
    );
}

pub(crate) fn record_s3_request_succeeded(
    context: &SyncDiagnosticContext,
    method: &str,
    operation: SyncProviderOperation,
    relative_path: &str,
    status: u16,
    duration: Duration,
) {
    if !diagnostics_enabled() {
        return;
    }
    let diagnostic = diagnostic_for(
        context,
        SyncFailureCategory::Http,
        format!("s3-{}-request-succeeded", operation.as_str()),
        Some(status),
        Some(method),
        operation,
        relative_path,
        None,
        None,
    );
    let record = request_log_record(&diagnostic, duration, "s3-request-succeeded");
    tauri_plugin_log::log::debug!(
        target: "qingyu::sync",
        "S3 request succeeded {}",
        serialize_log_record(&record)
    );
}

pub(crate) fn record_s3_request_retrying(
    context: &SyncDiagnosticContext,
    method: &str,
    operation: SyncProviderOperation,
    relative_path: &str,
    status: Option<u16>,
    attempt: u8,
    max_attempts: u8,
    duration: Duration,
) {
    let category = if status.is_some() {
        SyncFailureCategory::Http
    } else {
        SyncFailureCategory::Transport
    };
    let code = format!("s3-{}-request-retrying", operation.as_str());
    let object_id = s3_object_id(context, relative_path);
    let record = S3RetryLogRecord {
        attempt,
        category: category.as_str(),
        code: &code,
        duration_ms: duration.as_millis(),
        event: "s3-request-retrying",
        http_status: status,
        max_attempts,
        method,
        object_id: &object_id,
        operation: operation.as_str(),
        provider: "s3",
        run_id: &context.run_id,
        scope: &context.scope,
    };
    let serialized = serde_json::to_string(&record).unwrap_or_else(|_| "{}".to_string());
    tauri_plugin_log::log::warn!(
        target: "qingyu::sync",
        "S3 request will be retried {serialized}"
    );
}

pub(crate) fn s3_replan_required(
    context: &SyncDiagnosticContext,
    operation: SyncProviderOperation,
    method: &str,
    relative_path: &str,
    http_status: Option<u16>,
    duration: Duration,
) -> RemoteSyncError {
    let diagnostic = diagnostic_for(
        context,
        SyncFailureCategory::Integrity,
        "s3-object-changed".to_string(),
        http_status,
        Some(method),
        operation,
        relative_path,
        None,
        None,
    );
    let record = request_log_record(&diagnostic, duration, "s3-replan-required");
    tauri_plugin_log::log::warn!(
        target: "qingyu::sync",
        "S3 object changed; synchronization will re-plan {}",
        serialize_log_record(&record)
    );
    RemoteSyncError::diagnostic(diagnostic)
}

pub(crate) fn s3_transport_failure(
    context: &SyncDiagnosticContext,
    operation: SyncProviderOperation,
    method: &str,
    relative_path: &str,
    duration: Duration,
) -> RemoteSyncError {
    let diagnostic = diagnostic_for(
        context,
        SyncFailureCategory::Transport,
        format!("s3-{}-request-failed", operation.as_str()),
        None,
        Some(method),
        operation,
        relative_path,
        None,
        None,
    );
    record_s3_failure(&diagnostic, duration);
    RemoteSyncError::diagnostic(diagnostic)
}

pub(crate) async fn s3_http_failure(
    context: &SyncDiagnosticContext,
    operation: SyncProviderOperation,
    method: &str,
    relative_path: &str,
    mut response: Response,
    duration: Duration,
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
    let diagnostic = diagnostic_for(
        context,
        SyncFailureCategory::Http,
        format!("s3-{}-http-failed", operation.as_str()),
        Some(http_status),
        Some(method),
        operation,
        relative_path,
        provider_error_code,
        request_id,
    );
    record_s3_failure(&diagnostic, duration);
    RemoteSyncError::diagnostic(diagnostic)
}

pub(crate) fn s3_integrity_failure(
    context: &SyncDiagnosticContext,
    operation: SyncProviderOperation,
    method: &str,
    relative_path: &str,
    code: &str,
    duration: Duration,
) -> RemoteSyncError {
    let diagnostic = diagnostic_for(
        context,
        SyncFailureCategory::Integrity,
        code.to_string(),
        None,
        Some(method),
        operation,
        relative_path,
        None,
        None,
    );
    record_s3_failure(&diagnostic, duration);
    RemoteSyncError::diagnostic(diagnostic)
}

#[cfg(test)]
mod tests {
    use super::{
        diagnostics_enabled_from_value, parse_s3_error_code, s3_object_id, safe_request_id,
        safe_s3_error_code, SyncDiagnosticContext,
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
    fn diagnostic_switch_requires_exact_enabled_value() {
        assert!(!diagnostics_enabled_from_value(None));
        assert!(!diagnostics_enabled_from_value(Some("true")));
        assert!(diagnostics_enabled_from_value(Some("1")));
    }

    #[test]
    fn s3_error_parser_keeps_only_the_code() {
        let body = br#"<Error>
            <Code>AccessDenied</Code>
            <Message>secret-message</Message>
            <Resource>/secret-bucket/private/file.md</Resource>
            <HostId>secret-host</HostId>
        </Error>"#;

        let code = parse_s3_error_code(body);

        assert_eq!(code.as_deref(), Some("AccessDenied"));
        assert!(!format!("{code:?}").contains("secret"));
    }

    #[test]
    fn s3_error_parser_rejects_oversized_or_unsafe_values() {
        let oversized = format!(
            "<Error><Code>AccessDenied</Code>{}</Error>",
            "x".repeat(64 * 1024)
        );
        assert_eq!(parse_s3_error_code(oversized.as_bytes()), None);
        assert_eq!(
            parse_s3_error_code(b"<Error><Code>bad code/path</Code></Error>"),
            None
        );
    }
}
