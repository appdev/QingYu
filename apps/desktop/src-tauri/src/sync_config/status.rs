use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use cap_fs_ext::DirExt;
use cap_std::fs::Dir;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{Emitter, Runtime};
use time::OffsetDateTime;

use crate::storage_capability::{
    create_private_file_options, directory_identity, nonfollowing_read_options,
    rename_in_directory, sync_directory, unique_regular_file_identity, DirectoryIdentity,
    UniqueRegularFileIdentity,
};

#[cfg(test)]
use super::model::SyncConfig;
use super::model::SyncProvider;
use super::storage::{open_app_data, AppDataDirectory};

pub(crate) const SYNC_STATUS_VERSION: u32 = 1;
const SYNC_STATUS_CHANGED_EVENT: &str = "qingyu://sync-status-changed";
const SYNC_STATUS_FILE: &str = "status.json";
const SYNC_STATUS_MAX_BYTES: u64 = 1024 * 1024;
static STATUS_WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SyncTrigger {
    AppLaunch,
    Interval,
    Manual,
    Save,
    SettingsExit,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum SyncCompletionState {
    Attempting,
    Failed,
    Succeeded,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncSummary {
    pub(crate) bytes_downloaded: u64,
    pub(crate) bytes_uploaded: u64,
    pub(crate) conflict_files: u64,
    pub(crate) downloaded_files: u64,
    pub(crate) scanned_files: u64,
    pub(crate) skipped_files: u64,
    pub(crate) uploaded_files: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncSafeError {
    #[serde(default)]
    pub(crate) category: Option<String>,
    pub(crate) code: String,
    pub(crate) http_status: Option<u16>,
    #[serde(default)]
    pub(crate) method: Option<String>,
    #[serde(default)]
    pub(crate) object_id: Option<String>,
    pub(crate) operation: String,
    pub(crate) provider: SyncProvider,
    #[serde(default)]
    pub(crate) provider_error_code: Option<String>,
    pub(crate) relative_path: Option<String>,
    #[serde(default)]
    pub(crate) request_id: Option<String>,
    #[serde(default)]
    pub(crate) run_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncStatus {
    pub(crate) completion_state: SyncCompletionState,
    pub(crate) error: Option<SyncSafeError>,
    pub(crate) last_attempt_at: String,
    pub(crate) last_successful_sync_at: Option<String>,
    pub(crate) last_trigger: SyncTrigger,
    pub(crate) notebook_name: Option<String>,
    pub(crate) notes_root: Option<String>,
    pub(crate) provider: SyncProvider,
    pub(crate) revision: Option<String>,
    pub(crate) summary: Option<SyncSummary>,
    pub(crate) version: u32,
}

impl SyncStatus {
    pub(crate) fn attempting_for_run(
        provider: SyncProvider,
        trigger: SyncTrigger,
        attempted_at: String,
        notes_root: String,
        notebook_name: String,
        revision: String,
        previous: Option<&Self>,
    ) -> Self {
        let matching_previous = previous.filter(|status| {
            status.notes_root.as_deref() == Some(notes_root.as_str())
                && status.notebook_name.as_deref() == Some(notebook_name.as_str())
                && status.revision.as_deref() == Some(revision.as_str())
        });
        Self::attempting(provider, trigger, attempted_at, matching_previous).for_run(
            notes_root,
            notebook_name,
            revision,
        )
    }

    pub(crate) fn attempting(
        provider: SyncProvider,
        trigger: SyncTrigger,
        attempted_at: String,
        previous: Option<&Self>,
    ) -> Self {
        Self {
            completion_state: SyncCompletionState::Attempting,
            error: None,
            last_attempt_at: attempted_at,
            last_successful_sync_at: previous
                .and_then(|status| status.last_successful_sync_at.clone()),
            last_trigger: trigger,
            notebook_name: None,
            notes_root: None,
            provider,
            revision: None,
            summary: None,
            version: SYNC_STATUS_VERSION,
        }
    }

    pub(crate) fn failed(mut self, error: SyncSafeError, summary: SyncSummary) -> Self {
        self.completion_state = SyncCompletionState::Failed;
        self.error = Some(error);
        self.summary = Some(summary);
        self
    }

    pub(crate) fn succeeded(mut self, completed_at: String, summary: SyncSummary) -> Self {
        self.completion_state = SyncCompletionState::Succeeded;
        self.error = None;
        self.last_successful_sync_at = Some(completed_at);
        self.summary = Some(summary);
        self
    }

    pub(crate) fn for_run(
        mut self,
        notes_root: String,
        notebook_name: String,
        revision: String,
    ) -> Self {
        self.notebook_name = Some(notebook_name);
        self.notes_root = Some(notes_root);
        self.revision = Some(revision);
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncRunResult {
    pub(crate) notebook_name: String,
    pub(crate) notes_root: String,
    pub(crate) provider: SyncProvider,
    pub(crate) revision: String,
    pub(crate) summary: SyncSummary,
    pub(crate) trigger: SyncTrigger,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SyncStatusChangedEvent {
    notebook_name: String,
    notes_root: String,
    revision: String,
    status: SyncStatus,
}

pub(crate) fn load_sync_status_at_app_data(app_data: &Path) -> Result<Option<SyncStatus>, String> {
    let Some(state) = open_state_directory(app_data, false)? else {
        return Ok(None);
    };
    let addressed = match state.directory.symlink_metadata(SYNC_STATUS_FILE) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(status_path_error())
        }
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(status_read_error()),
    };
    if addressed.len() > SYNC_STATUS_MAX_BYTES {
        return Err(status_read_error());
    }
    let addressed_identity =
        unique_regular_file_identity(&addressed).ok_or_else(status_path_error)?;
    let mut file = state
        .directory
        .open_with(SYNC_STATUS_FILE, &nonfollowing_read_options())
        .map_err(|_| status_path_error())?;
    let retained = file.metadata().map_err(|_| status_read_error())?;
    if unique_regular_file_identity(&retained) != Some(addressed_identity) {
        return Err(status_path_error());
    }
    let mut bytes = Vec::with_capacity(retained.len() as usize);
    file.read_to_end(&mut bytes)
        .map_err(|_| status_read_error())?;
    let final_metadata = file.metadata().map_err(|_| status_read_error())?;
    if !addressed_identity.matches_retained_regular_file(&final_metadata, false)
        || bytes.len() as u64 != addressed_identity.revision_parts().2
    {
        return Err(status_path_error());
    }
    state.revalidate()?;
    let rechecked = state
        .directory
        .symlink_metadata(SYNC_STATUS_FILE)
        .map_err(|_| status_path_error())?;
    if unique_regular_file_identity(&rechecked) != Some(addressed_identity) {
        return Err(status_path_error());
    }
    let status = serde_json::from_slice::<SyncStatus>(&bytes)
        .map_err(|_| "sync-status-invalid: The sync status is invalid.".to_string())?;
    if status.version != SYNC_STATUS_VERSION {
        return Err("sync-status-invalid: The sync status is invalid.".to_string());
    }
    Ok(Some(status))
}

pub(crate) fn write_sync_status_at_app_data(
    app_data: &Path,
    status: &SyncStatus,
) -> Result<(), String> {
    write_sync_status_with_observer(app_data, status, |_, _| {})
}

fn write_sync_status_with_observer<Observe>(
    app_data: &Path,
    status: &SyncStatus,
    after_write: Observe,
) -> Result<(), String>
where
    Observe: FnOnce(&Path, &str),
{
    let _guard = STATUS_WRITE_LOCK.lock().map_err(|_| status_write_error())?;
    let state = open_state_directory(app_data, true)?.ok_or_else(status_write_error)?;
    match state.directory.symlink_metadata(SYNC_STATUS_FILE) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(status_path_error())
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(status_write_error()),
    }
    let mut bytes = serde_json::to_vec_pretty(status).map_err(|_| status_write_error())?;
    bytes.push(b'\n');
    let expected_hash = Sha256::digest(&bytes);
    let (staging_name, mut staging) = create_status_staging(&state)?;
    let write_result = (|| {
        staging
            .write_all(&bytes)
            .and_then(|()| staging.sync_all())
            .map_err(|_| status_write_error())?;
        let staging_identity = staging
            .metadata()
            .ok()
            .and_then(|metadata| unique_regular_file_identity(&metadata))
            .filter(|identity| identity.revision_parts().2 == bytes.len() as u64)
            .ok_or_else(status_path_error)?;
        drop(staging);
        after_write(&state.path, &staging_name);
        verify_status_staging(&state, &staging_name, staging_identity, &expected_hash)?;
        state.revalidate()?;
        verify_status_destination(&state)?;
        rename_in_directory(&state.directory, &staging_name, SYNC_STATUS_FILE, true)
            .map_err(|_| status_write_error())?;
        verify_status_staging(&state, SYNC_STATUS_FILE, staging_identity, &expected_hash)?;
        let _directory_sync = sync_directory(&state.directory);
        Ok(())
    })();
    if write_result.is_err() {
        let _cleanup = state.directory.remove_file(&staging_name);
    }
    write_result
}

pub(crate) fn emit_sync_status_changed<R: Runtime>(
    app: &tauri::AppHandle<R>,
    notes_root: &str,
    revision: &str,
    status: &SyncStatus,
) -> Result<(), String> {
    let notebook_name = status.notebook_name.clone().ok_or_else(|| {
        "sync-status-event-failed: The sync status change could not be announced.".to_string()
    })?;
    app.emit(
        SYNC_STATUS_CHANGED_EVENT,
        SyncStatusChangedEvent {
            notebook_name,
            notes_root: notes_root.to_string(),
            revision: revision.to_string(),
            status: status.clone(),
        },
    )
    .map_err(|_| {
        "sync-status-event-failed: The sync status change could not be announced.".to_string()
    })
}

pub(crate) fn sync_status_timestamp() -> String {
    let now = OffsetDateTime::now_utc();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

struct StatusStateDirectory {
    app_data: AppDataDirectory,
    directory: Dir,
    identity: DirectoryIdentity,
    path: PathBuf,
}

impl StatusStateDirectory {
    fn revalidate(&self) -> Result<(), String> {
        self.app_data
            .revalidate()
            .map_err(|_| status_path_error())?;
        let addressed = self
            .app_data
            .directory()
            .symlink_metadata("sync-state")
            .map_err(|_| status_path_error())?;
        if addressed.file_type().is_symlink() || !addressed.is_dir() {
            return Err(status_path_error());
        }
        let reopened = self
            .app_data
            .directory()
            .open_dir_nofollow("sync-state")
            .map_err(|_| status_path_error())?;
        if directory_identity(&reopened).map_err(|_| status_path_error())? != self.identity {
            return Err(status_path_error());
        }
        Ok(())
    }
}

fn open_state_directory(
    app_data: &Path,
    create: bool,
) -> Result<Option<StatusStateDirectory>, String> {
    let Some(app_data) = open_app_data(app_data, create).map_err(|_| status_path_error())? else {
        return Ok(None);
    };
    match app_data.directory().symlink_metadata("sync-state") {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(status_path_error())
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !create => return Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if let Err(error) = app_data.directory().create_dir("sync-state") {
                if error.kind() != std::io::ErrorKind::AlreadyExists {
                    return Err(status_write_error());
                }
            }
        }
        Err(_) => return Err(status_path_error()),
    }
    let directory = app_data
        .directory()
        .open_dir_nofollow("sync-state")
        .map_err(|_| status_path_error())?;
    let identity = directory_identity(&directory).map_err(|_| status_path_error())?;
    let path = app_data.canonical_path().join("sync-state");
    let state = StatusStateDirectory {
        app_data,
        directory,
        identity,
        path,
    };
    state.revalidate()?;
    Ok(Some(state))
}

fn create_status_staging(
    state: &StatusStateDirectory,
) -> Result<(String, cap_std::fs::File), String> {
    for _ in 0..8 {
        let mut nonce = [0_u8; 16];
        getrandom::fill(&mut nonce).map_err(|_| status_write_error())?;
        let encoded = nonce
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let name = format!(".status.tmp-{encoded}");
        match state
            .directory
            .open_with(&name, &create_private_file_options())
        {
            Ok(file) => return Ok((name, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(status_write_error()),
        }
    }
    Err(status_write_error())
}

fn verify_status_staging(
    state: &StatusStateDirectory,
    name: &str,
    expected_identity: UniqueRegularFileIdentity,
    expected_hash: &[u8],
) -> Result<(), String> {
    let addressed = state
        .directory
        .symlink_metadata(name)
        .map_err(|_| status_path_error())?;
    if unique_regular_file_identity(&addressed) != Some(expected_identity) {
        return Err(status_path_error());
    }
    let mut file = state
        .directory
        .open_with(name, &nonfollowing_read_options())
        .map_err(|_| status_path_error())?;
    let retained = file.metadata().map_err(|_| status_path_error())?;
    if !expected_identity.matches_retained_regular_file(&retained, false) {
        return Err(status_path_error());
    }
    let mut bytes = Vec::with_capacity(retained.len() as usize);
    file.read_to_end(&mut bytes)
        .map_err(|_| status_path_error())?;
    let final_metadata = file.metadata().map_err(|_| status_path_error())?;
    if !expected_identity.matches_retained_regular_file(&final_metadata, false)
        || bytes.len() as u64 != expected_identity.revision_parts().2
        || Sha256::digest(&bytes).as_slice() != expected_hash
    {
        return Err(status_path_error());
    }
    Ok(())
}

fn verify_status_destination(state: &StatusStateDirectory) -> Result<(), String> {
    match state.directory.symlink_metadata(SYNC_STATUS_FILE) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(status_path_error())
        }
        Ok(metadata) if unique_regular_file_identity(&metadata).is_none() => {
            Err(status_path_error())
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(status_write_error()),
    }
}

#[cfg(test)]
fn write_sync_status_with_hook<Observe>(
    app_data: &Path,
    status: &SyncStatus,
    after_write: Observe,
) -> Result<(), String>
where
    Observe: FnOnce(&Path, &str),
{
    write_sync_status_with_observer(app_data, status, after_write)
}

fn status_path_error() -> String {
    "sync-status-path-unsafe: The sync status path is unsafe.".to_string()
}

fn status_read_error() -> String {
    "sync-status-read-failed: The sync status could not be read.".to_string()
}

fn status_write_error() -> String {
    "sync-status-write-failed: The sync status could not be written.".to_string()
}

#[cfg(test)]
pub(crate) fn status_for_failed_run(config: &SyncConfig) -> SyncStatus {
    SyncStatus {
        completion_state: SyncCompletionState::Failed,
        error: Some(SyncSafeError {
            category: None,
            code: "sync-run-failed".into(),
            http_status: None,
            method: None,
            object_id: None,
            operation: "synchronize".into(),
            provider: config.provider,
            provider_error_code: None,
            relative_path: None,
            request_id: None,
            run_id: None,
        }),
        last_attempt_at: "2026-01-01T00:00:00Z".into(),
        last_successful_sync_at: None,
        last_trigger: SyncTrigger::Manual,
        notebook_name: None,
        notes_root: None,
        provider: config.provider,
        revision: None,
        summary: None,
        version: SYNC_STATUS_VERSION,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn safe_s3_error_serializes_actionable_diagnostics_without_a_path() {
        let error = SyncSafeError {
            category: Some("http".into()),
            code: "s3-upload-http-failed".into(),
            http_status: Some(403),
            method: Some("PUT".into()),
            object_id: Some("object-a1".into()),
            operation: "upload".into(),
            provider: SyncProvider::S3,
            provider_error_code: Some("AccessDenied".into()),
            relative_path: None,
            request_id: Some("request-403".into()),
            run_id: Some("run-1".into()),
        };

        let value = serde_json::to_value(error).unwrap();

        assert_eq!(value["httpStatus"], 403);
        assert_eq!(value["providerErrorCode"], "AccessDenied");
        assert_eq!(value["requestId"], "request-403");
        assert!(value["relativePath"].is_null());
    }

    #[test]
    fn status_transitions_preserve_last_success_and_only_serialize_safe_error_fields() {
        let previous = SyncStatus::attempting(
            SyncProvider::Webdav,
            SyncTrigger::Manual,
            "2026-07-20T01:00:00Z".into(),
            None,
        )
        .succeeded("2026-07-20T01:00:01Z".into(), SyncSummary::default());
        let attempting = SyncStatus::attempting(
            SyncProvider::S3,
            SyncTrigger::Save,
            "2026-07-20T02:00:00Z".into(),
            Some(&previous),
        );
        assert_eq!(
            attempting.last_successful_sync_at.as_deref(),
            Some("2026-07-20T01:00:01Z")
        );
        let failed = attempting.failed(
            SyncSafeError {
                category: None,
                code: "remote-http-error".into(),
                http_status: Some(503),
                method: None,
                object_id: None,
                operation: "synchronize".into(),
                provider: SyncProvider::S3,
                provider_error_code: None,
                relative_path: None,
                request_id: None,
                run_id: None,
            },
            SyncSummary::default(),
        );
        let serialized = serde_json::to_string(&failed).unwrap();
        assert!(serialized.contains("remote-http-error"));
        for forbidden in ["password", "secretAccessKey", "accessKeyId", "signedUrl"] {
            assert!(!serialized.contains(forbidden));
        }
    }

    #[test]
    fn run_status_inherits_success_only_from_the_same_root_and_config_revision() {
        let previous = SyncStatus::attempting_for_run(
            SyncProvider::Webdav,
            SyncTrigger::Manual,
            "2026-07-20T01:00:00Z".into(),
            "/Notes-A".into(),
            "Notes-A".into(),
            "rev-a".into(),
            None,
        )
        .succeeded("2026-07-20T01:00:01Z".into(), SyncSummary::default());

        let same_identity = SyncStatus::attempting_for_run(
            SyncProvider::Webdav,
            SyncTrigger::Save,
            "2026-07-20T02:00:00Z".into(),
            "/Notes-A".into(),
            "Notes-A".into(),
            "rev-a".into(),
            Some(&previous),
        );
        assert_eq!(
            same_identity.last_successful_sync_at.as_deref(),
            Some("2026-07-20T01:00:01Z")
        );

        for (notes_root, notebook_name, revision) in [
            ("/Notes-B", "Notes-A", "rev-a"),
            ("/Notes-A", "Notes-B", "rev-a"),
            ("/Notes-A", "Notes-A", "rev-b"),
        ] {
            let changed_identity = SyncStatus::attempting_for_run(
                SyncProvider::Webdav,
                SyncTrigger::Save,
                "2026-07-20T02:00:00Z".into(),
                notes_root.into(),
                notebook_name.into(),
                revision.into(),
                Some(&previous),
            );
            assert_eq!(changed_identity.last_successful_sync_at, None);
        }
    }

    #[test]
    fn status_write_replaces_atomically_and_leaves_no_staging_file() {
        let app_data = tempdir().unwrap();
        let attempting = SyncStatus::attempting(
            SyncProvider::S3,
            SyncTrigger::Manual,
            "2026-07-20T03:00:00Z".into(),
            None,
        );
        write_sync_status_at_app_data(app_data.path(), &attempting).unwrap();
        let succeeded = attempting.succeeded(
            "2026-07-20T03:00:01Z".into(),
            SyncSummary {
                uploaded_files: 2,
                ..SyncSummary::default()
            },
        );

        write_sync_status_at_app_data(app_data.path(), &succeeded).unwrap();

        assert_eq!(
            load_sync_status_at_app_data(app_data.path()).unwrap(),
            Some(succeeded)
        );
        assert_eq!(
            fs::read_dir(app_data.path().join("sync-state"))
                .unwrap()
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            vec!["status.json"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn status_write_rejects_a_symlinked_state_directory() {
        use std::os::unix::fs::symlink;

        let app_data = tempdir().unwrap();
        let outside = tempdir().unwrap();
        symlink(outside.path(), app_data.path().join("sync-state")).unwrap();
        let status = SyncStatus::attempting(
            SyncProvider::Webdav,
            SyncTrigger::Manual,
            "2026-07-20T04:00:00Z".into(),
            None,
        );

        assert!(write_sync_status_at_app_data(app_data.path(), &status).is_err());
        assert!(!outside.path().join("status.json").exists());
    }

    #[cfg(unix)]
    #[test]
    fn status_write_fails_closed_when_app_data_is_exchanged_before_publish() {
        let parent = tempdir().unwrap();
        let app_data = parent.path().join("app-data");
        fs::create_dir(&app_data).unwrap();
        let displaced = parent.path().join("displaced-app-data");
        let status = SyncStatus::attempting(
            SyncProvider::Webdav,
            SyncTrigger::Manual,
            "2026-07-20T05:00:00Z".into(),
            None,
        );

        let result = write_sync_status_with_hook(&app_data, &status, |_, _| {
            fs::rename(&app_data, &displaced).unwrap();
            fs::create_dir(&app_data).unwrap();
        });

        assert!(result.is_err());
        assert!(!app_data.join("sync-state/status.json").exists());
        assert!(!displaced.join("sync-state/status.json").exists());
    }

    #[cfg(unix)]
    #[test]
    fn status_write_fails_closed_when_state_directory_is_exchanged_before_publish() {
        let app_data = tempdir().unwrap();
        let displaced = app_data.path().join("displaced-state");
        let status = SyncStatus::attempting(
            SyncProvider::Webdav,
            SyncTrigger::Manual,
            "2026-07-20T06:00:00Z".into(),
            None,
        );

        let result = write_sync_status_with_hook(app_data.path(), &status, |_, _| {
            fs::rename(app_data.path().join("sync-state"), &displaced).unwrap();
            fs::create_dir(app_data.path().join("sync-state")).unwrap();
        });

        assert!(result.is_err());
        assert!(!app_data.path().join("sync-state/status.json").exists());
        assert!(!displaced.join("status.json").exists());
    }

    #[cfg(unix)]
    #[test]
    fn status_write_rejects_staging_replacement_before_publish() {
        use std::os::unix::fs::symlink;

        let app_data = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let outside_file = outside.path().join("outside.json");
        fs::write(&outside_file, b"outside").unwrap();
        let status = SyncStatus::attempting(
            SyncProvider::S3,
            SyncTrigger::Manual,
            "2026-07-20T07:00:00Z".into(),
            None,
        );

        let result = write_sync_status_with_hook(app_data.path(), &status, |state, staging| {
            fs::remove_file(state.join(staging)).unwrap();
            symlink(&outside_file, state.join(staging)).unwrap();
        });

        assert!(result.is_err());
        assert!(!app_data.path().join("sync-state/status.json").exists());
        assert_eq!(fs::read(&outside_file).unwrap(), b"outside");
    }

    #[test]
    fn status_write_rejects_a_hardlinked_staging_replacement() {
        let app_data = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let outside_file = outside.path().join("outside.json");
        fs::write(&outside_file, b"outside").unwrap();
        let status = SyncStatus::attempting(
            SyncProvider::S3,
            SyncTrigger::Manual,
            "2026-07-20T07:00:01Z".into(),
            None,
        );

        let result = write_sync_status_with_hook(app_data.path(), &status, |state, staging| {
            fs::remove_file(state.join(staging)).unwrap();
            fs::hard_link(&outside_file, state.join(staging)).unwrap();
        });

        assert!(result.is_err());
        assert!(!app_data.path().join("sync-state/status.json").exists());
        assert_eq!(fs::read(&outside_file).unwrap(), b"outside");
    }

    #[test]
    fn status_write_rejects_a_same_length_staging_replacement() {
        let app_data = tempdir().unwrap();
        let status = SyncStatus::attempting(
            SyncProvider::S3,
            SyncTrigger::Manual,
            "2026-07-20T07:00:02Z".into(),
            None,
        );

        let result = write_sync_status_with_hook(app_data.path(), &status, |state, staging| {
            let path = state.join(staging);
            let length = fs::metadata(&path).unwrap().len() as usize;
            fs::remove_file(&path).unwrap();
            fs::write(path, vec![b'x'; length]).unwrap();
        });

        assert!(result.is_err());
        assert!(!app_data.path().join("sync-state/status.json").exists());
    }

    #[test]
    fn parallel_status_writers_publish_only_complete_documents() {
        let app_data = tempdir().unwrap();
        let app_data_path = Arc::new(app_data.path().to_path_buf());
        let writers = (0..16)
            .map(|index| {
                let app_data_path = Arc::clone(&app_data_path);
                std::thread::spawn(move || {
                    let status = SyncStatus::attempting(
                        SyncProvider::S3,
                        SyncTrigger::Interval,
                        format!("2026-07-20T08:00:{index:02}Z"),
                        None,
                    );
                    write_sync_status_at_app_data(&app_data_path, &status)
                })
            })
            .collect::<Vec<_>>();

        for writer in writers {
            writer.join().unwrap().unwrap();
        }
        assert!(load_sync_status_at_app_data(&app_data_path)
            .unwrap()
            .is_some());
        assert!(fs::read_dir(app_data_path.join("sync-state"))
            .unwrap()
            .flatten()
            .all(|entry| entry.file_name() == "status.json"));
    }
}
