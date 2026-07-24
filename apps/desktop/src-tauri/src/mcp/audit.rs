use std::{
    collections::BTreeMap,
    fmt,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use super::{config::AuditPolicy, confirmation::ConfirmationOutcome};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AuditOutcome {
    Succeeded,
    Failed,
    Previewed,
}

#[derive(Clone, Debug)]
pub(crate) struct AuditEvent {
    pub(crate) request_id: Uuid,
    pub(crate) tool: String,
    pub(crate) workspace_id: Option<Uuid>,
    pub(crate) workspace_display_name: Option<String>,
    pub(crate) logical_target: Option<String>,
    pub(crate) dry_run: bool,
    pub(crate) confirmation: Option<ConfirmationOutcome>,
    pub(crate) outcome: AuditOutcome,
    pub(crate) error_code: Option<String>,
    pub(crate) revision_before: Option<String>,
    pub(crate) revision_after: Option<String>,
    pub(crate) sync_run_id: Option<Uuid>,
    pub(crate) duration_ms: u64,
    pub(crate) counts: BTreeMap<String, u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct AuditEntry {
    pub(crate) request_id: Uuid,
    pub(crate) timestamp_ms: i64,
    pub(crate) tool: String,
    pub(crate) workspace_id: Option<Uuid>,
    pub(crate) workspace_display_name: Option<String>,
    pub(crate) logical_target: Option<String>,
    pub(crate) dry_run: bool,
    pub(crate) confirmation: Option<ConfirmationOutcome>,
    pub(crate) outcome: AuditOutcome,
    pub(crate) error_code: Option<String>,
    pub(crate) revision_before: Option<String>,
    pub(crate) revision_after: Option<String>,
    pub(crate) sync_run_id: Option<Uuid>,
    pub(crate) duration_ms: u64,
    pub(crate) counts: BTreeMap<String, u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AuditError {
    pub(crate) code: &'static str,
    message: &'static str,
}

impl AuditError {
    fn unavailable() -> Self {
        Self {
            code: "audit_write_failed",
            message: "The MCP audit record could not be persisted.",
        }
    }

    fn malformed() -> Self {
        Self {
            code: "audit_read_failed",
            message: "The MCP audit log could not be read.",
        }
    }
}

impl fmt::Display for AuditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for AuditError {}

pub(crate) struct AuditSink {
    path: PathBuf,
    policy: Mutex<AuditPolicy>,
    operation_lock: Mutex<()>,
}

impl AuditSink {
    pub(crate) fn new(app_data_dir: &Path, mut policy: AuditPolicy) -> Self {
        policy.retention_days = policy.retention_days.clamp(1, 365);
        policy.max_entries = policy.max_entries.clamp(100, 100_000);
        Self {
            path: app_data_dir.join("mcp-audit.jsonl"),
            policy: Mutex::new(policy),
            operation_lock: Mutex::new(()),
        }
    }

    pub(crate) fn update_policy(&self, mut policy: AuditPolicy) -> Result<(), AuditError> {
        policy.retention_days = policy.retention_days.clamp(1, 365);
        policy.max_entries = policy.max_entries.clamp(100, 100_000);
        *self.policy.lock().map_err(|_| AuditError::unavailable())? = policy;
        Ok(())
    }

    pub(crate) fn record(&self, event: AuditEvent) -> Result<(), AuditError> {
        let policy = self
            .policy
            .lock()
            .map_err(|_| AuditError::unavailable())?
            .clone();
        if !policy.enabled {
            return Ok(());
        }
        let _guard = self
            .operation_lock
            .lock()
            .map_err(|_| AuditError::unavailable())?;
        let now = current_timestamp_ms();
        let mut entries = read_entries(&self.path)?;
        let oldest = now.saturating_sub(i64::from(policy.retention_days) * 86_400_000);
        entries.retain(|entry| entry.timestamp_ms >= oldest);
        entries.push(sanitize_event(event, now));
        if entries.len() > policy.max_entries {
            let remove_count = entries.len() - policy.max_entries;
            entries.drain(..remove_count);
        }
        write_entries(&self.path, &entries)
    }

    pub(crate) fn preflight(&self) -> Result<(), AuditError> {
        let policy = self
            .policy
            .lock()
            .map_err(|_| AuditError::unavailable())?
            .clone();
        if !policy.enabled {
            return Ok(());
        }
        let _guard = self
            .operation_lock
            .lock()
            .map_err(|_| AuditError::unavailable())?;
        let entries = read_entries(&self.path)?;
        write_entries(&self.path, &entries)
    }

    pub(crate) fn list(&self, offset: usize, limit: usize) -> Result<Vec<AuditEntry>, AuditError> {
        let _guard = self
            .operation_lock
            .lock()
            .map_err(|_| AuditError::malformed())?;
        let entries = read_entries(&self.path)?;
        Ok(entries
            .into_iter()
            .rev()
            .skip(offset)
            .take(limit.clamp(1, 100))
            .collect())
    }

    pub(crate) fn clear(&self) -> Result<(), AuditError> {
        let _guard = self
            .operation_lock
            .lock()
            .map_err(|_| AuditError::unavailable())?;
        match fs::symlink_metadata(&self.path) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                Err(AuditError::unavailable())
            }
            Ok(_) => fs::remove_file(&self.path).map_err(|_| AuditError::unavailable()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(AuditError::unavailable()),
        }
    }
}

fn sanitize_event(event: AuditEvent, timestamp_ms: i64) -> AuditEntry {
    AuditEntry {
        request_id: event.request_id,
        timestamp_ms,
        tool: sanitize_text(&event.tool, false),
        workspace_id: event.workspace_id,
        workspace_display_name: event
            .workspace_display_name
            .map(|value| sanitize_text(&value, false)),
        logical_target: event
            .logical_target
            .map(|value| sanitize_text(&value, true)),
        dry_run: event.dry_run,
        confirmation: event.confirmation,
        outcome: event.outcome,
        error_code: event.error_code.map(|value| sanitize_text(&value, false)),
        revision_before: event
            .revision_before
            .map(|value| sanitize_text(&value, false)),
        revision_after: event
            .revision_after
            .map(|value| sanitize_text(&value, false)),
        sync_run_id: event.sync_run_id,
        duration_ms: event.duration_ms,
        counts: event
            .counts
            .into_iter()
            .take(32)
            .map(|(key, value)| (sanitize_text(&key, false), value))
            .collect(),
    }
}

fn sanitize_text(value: &str, redact_absolute: bool) -> String {
    if redact_absolute && looks_absolute(value) {
        return "[redacted]".to_string();
    }
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(512)
        .collect()
}

fn looks_absolute(value: &str) -> bool {
    let bytes = value.as_bytes();
    Path::new(value).is_absolute()
        || value.starts_with("\\\\")
        || (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
}

fn current_timestamp_ms() -> i64 {
    (OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000)
        .try_into()
        .unwrap_or(i64::MAX)
}

fn read_entries(path: &Path) -> Result<Vec<AuditEntry>, AuditError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(AuditError::malformed());
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(AuditError::malformed()),
    }
    let contents = fs::read_to_string(path).map_err(|_| AuditError::malformed())?;
    contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).map_err(|_| AuditError::malformed()))
        .collect()
}

fn write_entries(path: &Path, entries: &[AuditEntry]) -> Result<(), AuditError> {
    let parent = path.parent().ok_or_else(AuditError::unavailable)?;
    fs::create_dir_all(parent).map_err(|_| AuditError::unavailable())?;
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(AuditError::unavailable());
        }
    }
    let staging = parent.join(format!(".mcp-audit.{}.tmp", Uuid::new_v4()));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&staging)
            .map_err(|_| AuditError::unavailable())?;
        for entry in entries {
            serde_json::to_writer(&mut file, entry).map_err(|_| AuditError::unavailable())?;
            file.write_all(b"\n")
                .map_err(|_| AuditError::unavailable())?;
        }
        file.sync_all().map_err(|_| AuditError::unavailable())?;
        replace_audit_file(&staging, path, path.exists())?;
        #[cfg(unix)]
        if let Ok(directory) = fs::File::open(parent) {
            let _ = directory.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(staging);
    }
    result
}

#[cfg(not(windows))]
fn replace_audit_file(
    staging: &Path,
    target: &Path,
    _target_exists: bool,
) -> Result<(), AuditError> {
    fs::rename(staging, target).map_err(|_| AuditError::unavailable())
}

#[cfg(windows)]
fn replace_audit_file(
    staging: &Path,
    target: &Path,
    target_exists: bool,
) -> Result<(), AuditError> {
    if !target_exists {
        return fs::rename(staging, target).map_err(|_| AuditError::unavailable());
    }
    use std::{os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::Storage::FileSystem::{ReplaceFileW, REPLACEFILE_WRITE_THROUGH};

    let target = target
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let staging = staging
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let replaced = unsafe {
        ReplaceFileW(
            target.as_ptr(),
            staging.as_ptr(),
            ptr::null(),
            REPLACEFILE_WRITE_THROUGH,
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    if replaced == 0 {
        return Err(AuditError::unavailable());
    }
    Ok(())
}
