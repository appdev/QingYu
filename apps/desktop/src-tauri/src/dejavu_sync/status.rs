#![cfg_attr(not(test), allow(dead_code))]

use std::ffi::OsStr;
use std::future::Future;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use cap_fs_ext::DirExt;
use qingyu_dejavu::{write_cap_file_safer, RepositoryRelativePath};
use serde::{Deserialize, Serialize};
use tauri::Emitter;
use time::OffsetDateTime;

use super::service::{
    RepositoryJobError, RepositoryStatusSink, RepositorySyncResult, SyncJobRequest,
};
use crate::storage_capability::{nonfollowing_read_options, unique_regular_file_identity};
use crate::sync_config::status::SyncTrigger;
use crate::sync_config::storage::{open_app_data, AppDataDirectory};

pub(crate) const REPOSITORY_SYNC_STATUS_VERSION: u32 = 1;
#[allow(dead_code)]
pub(crate) const REPOSITORY_SYNC_STATUS_CHANGED_EVENT: &str = "qingyu://dejavu-sync-status-changed";
const REPOSITORY_STATUS_FILE: &str = "state.json";
const MAX_REPOSITORY_STATUS_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RepositorySyncPhase {
    Attempting,
    Succeeded,
    Failed,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RepositoryTransferSummary {
    pub(crate) download_bytes: u64,
    pub(crate) download_chunks: u64,
    pub(crate) download_files: u64,
    pub(crate) upload_bytes: u64,
    pub(crate) upload_chunks: u64,
    pub(crate) upload_files: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RepositoryConflictRecord {
    pub(crate) relative_path: String,
    pub(crate) occurred_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RepositorySafeError {
    pub(crate) code: String,
    pub(crate) operation: String,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RepositorySchedule {
    pub(crate) same_count: u32,
    pub(crate) automatic_failure_count: u32,
    #[serde(with = "time::serde::rfc3339::option")]
    pub(crate) last_dns_retry_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub(crate) next_scheduled_at: Option<OffsetDateTime>,
}

impl From<RepositoryJobError> for RepositorySafeError {
    fn from(error: RepositoryJobError) -> Self {
        Self {
            code: error.safe_code().to_owned(),
            operation: "repository-sync".to_owned(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RepositorySyncStatus {
    pub(crate) version: u32,
    pub(crate) repository_id: String,
    pub(crate) phase: RepositorySyncPhase,
    pub(crate) trigger: SyncTrigger,
    pub(crate) job_id: String,
    pub(crate) attempt: u8,
    pub(crate) last_attempt_at: String,
    pub(crate) last_successful_sync_at: Option<String>,
    #[serde(flatten)]
    pub(crate) schedule: RepositorySchedule,
    pub(crate) error: Option<RepositorySafeError>,
    pub(crate) transfer: RepositoryTransferSummary,
    pub(crate) conflicts: Vec<RepositoryConflictRecord>,
}

impl RepositorySyncStatus {
    pub(crate) fn attempting(
        request: &SyncJobRequest,
        job_id: String,
        attempt: u8,
        attempted_at: String,
    ) -> Self {
        Self {
            version: REPOSITORY_SYNC_STATUS_VERSION,
            repository_id: request.repository_id.clone(),
            phase: RepositorySyncPhase::Attempting,
            trigger: request.trigger,
            job_id,
            attempt,
            last_attempt_at: attempted_at,
            last_successful_sync_at: None,
            schedule: RepositorySchedule::default(),
            error: None,
            transfer: RepositoryTransferSummary::default(),
            conflicts: Vec::new(),
        }
    }

    pub(crate) fn succeeded(
        request: &SyncJobRequest,
        job_id: String,
        attempt: u8,
        completed_at: String,
        result: RepositorySyncResult,
    ) -> Self {
        Self {
            version: REPOSITORY_SYNC_STATUS_VERSION,
            repository_id: request.repository_id.clone(),
            phase: RepositorySyncPhase::Succeeded,
            trigger: request.trigger,
            job_id,
            attempt,
            last_attempt_at: completed_at.clone(),
            last_successful_sync_at: Some(completed_at),
            schedule: RepositorySchedule::default(),
            error: None,
            transfer: result.transfer,
            conflicts: result.conflicts,
        }
    }

    pub(crate) fn failed(
        request: &SyncJobRequest,
        job_id: String,
        attempt: u8,
        completed_at: String,
        error: RepositorySafeError,
    ) -> Self {
        Self {
            version: REPOSITORY_SYNC_STATUS_VERSION,
            repository_id: request.repository_id.clone(),
            phase: RepositorySyncPhase::Failed,
            trigger: request.trigger,
            job_id,
            attempt,
            last_attempt_at: completed_at,
            last_successful_sync_at: None,
            schedule: RepositorySchedule::default(),
            error: Some(error),
            transfer: RepositoryTransferSummary::default(),
            conflicts: Vec::new(),
        }
    }
}

pub(crate) trait RepositoryStatusEventEmitter: Send + Sync {
    fn emit(&self, status: &RepositorySyncStatus) -> Result<(), RepositoryJobError>;
}

#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct TauriRepositoryStatusEmitter {
    app: tauri::AppHandle,
}

impl TauriRepositoryStatusEmitter {
    #[allow(dead_code)]
    pub(crate) fn new(app: tauri::AppHandle) -> Self {
        Self { app }
    }
}

impl RepositoryStatusEventEmitter for TauriRepositoryStatusEmitter {
    fn emit(&self, status: &RepositorySyncStatus) -> Result<(), RepositoryJobError> {
        Emitter::emit(
            &self.app,
            REPOSITORY_SYNC_STATUS_CHANGED_EVENT,
            status.clone(),
        )
        .map_err(|_| RepositoryJobError::StatusUnavailable)
    }
}

pub(crate) struct RepositoryStatusStore {
    app_data: PathBuf,
    emitter: Arc<dyn RepositoryStatusEventEmitter>,
    write_lock: Mutex<()>,
}

impl RepositoryStatusStore {
    pub(crate) fn new<Emit>(app_data: impl AsRef<Path>, emitter: Arc<Emit>) -> Self
    where
        Emit: RepositoryStatusEventEmitter + 'static,
    {
        let emitter: Arc<dyn RepositoryStatusEventEmitter> = emitter;
        Self {
            app_data: app_data.as_ref().to_path_buf(),
            emitter,
            write_lock: Mutex::new(()),
        }
    }

    fn write_then_emit(&self, mut status: RepositorySyncStatus) -> Result<(), RepositoryJobError> {
        validate_status(&status)?;
        let _write = self.write_lock.lock().unwrap();
        if let Some(previous) = load_repository_sync_status(&self.app_data, &status.repository_id)?
        {
            status.schedule = previous.schedule;
            if status.last_successful_sync_at.is_none() {
                status.last_successful_sync_at = previous.last_successful_sync_at;
            }
            if status.phase == RepositorySyncPhase::Attempting {
                status.transfer = previous.transfer;
                status.conflicts = previous.conflicts;
            } else if status.conflicts.is_empty() && status.phase == RepositorySyncPhase::Failed {
                status.conflicts = previous.conflicts;
            }
        }
        self.persist_then_emit(status)
    }

    pub(crate) fn load_schedule(
        &self,
        repository_id: &str,
    ) -> Result<RepositorySchedule, RepositoryJobError> {
        Ok(load_repository_sync_status(&self.app_data, repository_id)?
            .map(|status| status.schedule)
            .unwrap_or_default())
    }

    pub(crate) fn save_schedule(
        &self,
        repository_id: &str,
        schedule: RepositorySchedule,
    ) -> Result<(), RepositoryJobError> {
        validate_repository_id(repository_id)?;
        let _write = self.write_lock.lock().unwrap();
        let mut status = load_repository_sync_status(&self.app_data, repository_id)?
            .ok_or(RepositoryJobError::StatusUnavailable)?;
        status.schedule = schedule;
        self.persist_then_emit(status)
    }

    pub(crate) fn reserve_dns_retry(
        &self,
        repository_id: &str,
        now: OffsetDateTime,
        throttle: std::time::Duration,
    ) -> Result<bool, RepositoryJobError> {
        validate_repository_id(repository_id)?;
        let _write = self.write_lock.lock().unwrap();
        let mut status = load_repository_sync_status(&self.app_data, repository_id)?
            .ok_or(RepositoryJobError::StatusUnavailable)?;
        if status
            .schedule
            .last_dns_retry_at
            .is_some_and(|last| now - last < throttle)
        {
            return Ok(false);
        }
        status.schedule.last_dns_retry_at = Some(now);
        self.persist_then_emit(status)?;
        Ok(true)
    }

    fn persist_then_emit(&self, status: RepositorySyncStatus) -> Result<(), RepositoryJobError> {
        validate_status(&status)?;
        let mut bytes = serde_json::to_vec_pretty(&status)
            .map_err(|_| RepositoryJobError::StatusUnavailable)?;
        bytes.push(b'\n');
        if bytes.len() > MAX_REPOSITORY_STATUS_BYTES {
            return Err(RepositoryJobError::StatusUnavailable);
        }

        let app_data = open_app_data(&self.app_data, true)
            .map_err(|_| RepositoryJobError::StatusUnavailable)?
            .ok_or(RepositoryJobError::StatusUnavailable)?;
        let repository = open_repository_status_directory(&app_data, &status.repository_id, true)?
            .ok_or(RepositoryJobError::StatusUnavailable)?;
        match repository.symlink_metadata(REPOSITORY_STATUS_FILE) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                return Err(RepositoryJobError::StatusUnavailable)
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(RepositoryJobError::StatusUnavailable),
        }
        app_data
            .revalidate()
            .map_err(|_| RepositoryJobError::StatusUnavailable)?;
        write_cap_file_safer(
            &repository,
            OsStr::new(REPOSITORY_STATUS_FILE),
            &bytes,
            0o600,
        )
        .map_err(|_| RepositoryJobError::StatusUnavailable)?;
        app_data
            .revalidate()
            .map_err(|_| RepositoryJobError::StatusUnavailable)?;

        // Persistence is deliberately complete before this notification.
        self.emitter.emit(&status)
    }
}

impl RepositoryStatusSink for RepositoryStatusStore {
    fn publish<'a>(
        &'a self,
        status: RepositorySyncStatus,
    ) -> Pin<Box<dyn Future<Output = Result<(), RepositoryJobError>> + Send + 'a>> {
        Box::pin(async move { self.write_then_emit(status) })
    }
}

pub(crate) fn load_repository_sync_status(
    app_data_path: &Path,
    repository_id: &str,
) -> Result<Option<RepositorySyncStatus>, RepositoryJobError> {
    validate_repository_id(repository_id)?;
    let Some(app_data) =
        open_app_data(app_data_path, false).map_err(|_| RepositoryJobError::StatusUnavailable)?
    else {
        return Ok(None);
    };
    let Some(repository) = open_repository_status_directory(&app_data, repository_id, false)?
    else {
        return Ok(None);
    };
    let addressed = match repository.symlink_metadata(REPOSITORY_STATUS_FILE) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(RepositoryJobError::StatusUnavailable)
        }
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(RepositoryJobError::StatusUnavailable),
    };
    if addressed.len() > MAX_REPOSITORY_STATUS_BYTES as u64 {
        return Err(RepositoryJobError::StatusUnavailable);
    }
    let identity =
        unique_regular_file_identity(&addressed).ok_or(RepositoryJobError::StatusUnavailable)?;
    let mut file = repository
        .open_with(REPOSITORY_STATUS_FILE, &nonfollowing_read_options())
        .map_err(|_| RepositoryJobError::StatusUnavailable)?;
    let retained = file
        .metadata()
        .map_err(|_| RepositoryJobError::StatusUnavailable)?;
    if !identity.matches_retained_regular_file(&retained, false) {
        return Err(RepositoryJobError::StatusUnavailable);
    }
    let mut bytes = Vec::with_capacity(retained.len() as usize);
    Read::by_ref(&mut file)
        .take(MAX_REPOSITORY_STATUS_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| RepositoryJobError::StatusUnavailable)?;
    if bytes.len() > MAX_REPOSITORY_STATUS_BYTES {
        return Err(RepositoryJobError::StatusUnavailable);
    }
    let final_metadata = file
        .metadata()
        .map_err(|_| RepositoryJobError::StatusUnavailable)?;
    if !identity.matches_retained_regular_file(&final_metadata, false) {
        return Err(RepositoryJobError::StatusUnavailable);
    }
    app_data
        .revalidate()
        .map_err(|_| RepositoryJobError::StatusUnavailable)?;
    let status = serde_json::from_slice::<RepositorySyncStatus>(&bytes)
        .map_err(|_| RepositoryJobError::StatusUnavailable)?;
    validate_status(&status)?;
    if status.repository_id != repository_id {
        return Err(RepositoryJobError::StatusUnavailable);
    }
    Ok(Some(status))
}

fn validate_repository_id(repository_id: &str) -> Result<(), RepositoryJobError> {
    let parsed =
        uuid::Uuid::parse_str(repository_id).map_err(|_| RepositoryJobError::InvalidBinding)?;
    if parsed.to_string() != repository_id {
        return Err(RepositoryJobError::InvalidBinding);
    }
    Ok(())
}

fn validate_status(status: &RepositorySyncStatus) -> Result<(), RepositoryJobError> {
    validate_repository_id(&status.repository_id)?;
    if status.version != REPOSITORY_SYNC_STATUS_VERSION
        || status.attempt == 0
        || status.attempt > 3
        || uuid::Uuid::parse_str(&status.job_id)
            .ok()
            .is_none_or(|job_id| job_id.to_string() != status.job_id)
        || status.last_attempt_at.is_empty()
    {
        return Err(RepositoryJobError::StatusUnavailable);
    }
    for conflict in &status.conflicts {
        RepositoryRelativePath::new(conflict.relative_path.clone())
            .map_err(|_| RepositoryJobError::StatusUnavailable)?;
        if conflict.occurred_at.is_empty() {
            return Err(RepositoryJobError::StatusUnavailable);
        }
    }
    Ok(())
}

fn open_repository_status_directory(
    app_data: &AppDataDirectory,
    repository_id: &str,
    create: bool,
) -> Result<Option<cap_std::fs::Dir>, RepositoryJobError> {
    validate_repository_id(repository_id)?;
    let Some(sync) = open_child_directory(app_data.directory(), "sync", create)? else {
        return Ok(None);
    };
    let Some(repositories) = open_child_directory(&sync, "repositories", create)? else {
        return Ok(None);
    };
    open_child_directory(&repositories, repository_id, create)
}

fn open_child_directory(
    parent: &cap_std::fs::Dir,
    name: &str,
    create: bool,
) -> Result<Option<cap_std::fs::Dir>, RepositoryJobError> {
    match parent.symlink_metadata(name) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(RepositoryJobError::StatusUnavailable)
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && create => {
            match parent.create_dir(name) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return Err(RepositoryJobError::StatusUnavailable),
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(RepositoryJobError::StatusUnavailable),
    }
    let metadata = parent
        .symlink_metadata(name)
        .map_err(|_| RepositoryJobError::StatusUnavailable)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(RepositoryJobError::StatusUnavailable);
    }
    parent
        .open_dir_nofollow(name)
        .map(Some)
        .map_err(|_| RepositoryJobError::StatusUnavailable)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tempfile::tempdir;

    use super::{
        load_repository_sync_status, RepositoryStatusEventEmitter, RepositoryStatusStore,
        RepositorySyncPhase, RepositorySyncStatus,
    };
    use crate::dejavu_sync::service::{RepositoryJobError, RepositoryStatusSink, SyncJobRequest};
    use crate::sync_config::status::SyncTrigger;

    struct InspectingEmitter {
        app_data: PathBuf,
        listening: AtomicBool,
        emitted: Mutex<Vec<RepositorySyncStatus>>,
    }

    impl InspectingEmitter {
        fn new(app_data: PathBuf) -> Self {
            Self {
                app_data,
                listening: AtomicBool::new(true),
                emitted: Mutex::new(Vec::new()),
            }
        }
    }

    impl RepositoryStatusEventEmitter for InspectingEmitter {
        fn emit(&self, status: &RepositorySyncStatus) -> Result<(), RepositoryJobError> {
            let persisted = load_repository_sync_status(&self.app_data, &status.repository_id)?
                .expect("state must exist before event emission");
            assert_eq!(&persisted, status);
            if self.listening.load(Ordering::SeqCst) {
                self.emitted.lock().unwrap().push(status.clone());
            }
            Ok(())
        }
    }

    fn attempting(repository_id: &str) -> RepositorySyncStatus {
        RepositorySyncStatus::attempting(
            &SyncJobRequest {
                notes_root: PathBuf::from("/notes/journal"),
                repository_id: repository_id.to_owned(),
                trigger: SyncTrigger::Manual,
            },
            "00000000-0000-4000-8000-000000000099".to_owned(),
            1,
            "2026-07-25T08:00:00Z".to_owned(),
        )
    }

    #[tokio::test]
    async fn status_is_atomically_persisted_before_the_same_public_payload_is_emitted() {
        let app_data = tempdir().unwrap();
        let repository_id = "00000000-0000-4000-8000-000000000010";
        let emitter = Arc::new(InspectingEmitter::new(app_data.path().to_path_buf()));
        let store = RepositoryStatusStore::new(app_data.path(), Arc::clone(&emitter));
        let status = attempting(repository_id);

        store.publish(status.clone()).await.unwrap();

        assert_eq!(
            emitter.emitted.lock().unwrap().as_slice(),
            std::slice::from_ref(&status)
        );
        assert_eq!(
            load_repository_sync_status(app_data.path(), repository_id).unwrap(),
            Some(status)
        );
        assert!(app_data
            .path()
            .join(format!("sync/repositories/{repository_id}/state.json"))
            .is_file());
    }

    #[tokio::test]
    async fn listener_drop_does_not_remove_or_cancel_persisted_repository_status() {
        let app_data = tempdir().unwrap();
        let repository_id = "00000000-0000-4000-8000-000000000011";
        let emitter = Arc::new(InspectingEmitter::new(app_data.path().to_path_buf()));
        emitter.listening.store(false, Ordering::SeqCst);
        let store = RepositoryStatusStore::new(app_data.path(), Arc::clone(&emitter));
        let mut status = attempting(repository_id);
        status.phase = RepositorySyncPhase::Succeeded;
        status.last_successful_sync_at = Some("2026-07-25T08:00:01Z".to_owned());

        store.publish(status.clone()).await.unwrap();

        assert!(emitter.emitted.lock().unwrap().is_empty());
        assert_eq!(
            load_repository_sync_status(app_data.path(), repository_id).unwrap(),
            Some(status)
        );
    }

    #[tokio::test]
    async fn persisted_and_emitted_status_omit_the_machine_absolute_notes_root() {
        let app_data = tempdir().unwrap();
        let repository_id = "00000000-0000-4000-8000-000000000013";
        let sentinel = app_data.path().join("machine-private-notes-root");
        let emitter = Arc::new(InspectingEmitter::new(app_data.path().to_path_buf()));
        let store = RepositoryStatusStore::new(app_data.path(), Arc::clone(&emitter));
        let status = RepositorySyncStatus::attempting(
            &SyncJobRequest {
                notes_root: sentinel.clone(),
                repository_id: repository_id.to_owned(),
                trigger: SyncTrigger::Manual,
            },
            "00000000-0000-4000-8000-000000000098".to_owned(),
            1,
            "2026-07-25T08:00:00Z".to_owned(),
        );

        store.publish(status).await.unwrap();

        let persisted = std::fs::read_to_string(
            app_data
                .path()
                .join(format!("sync/repositories/{repository_id}/state.json")),
        )
        .unwrap();
        let persisted_json = serde_json::from_str::<serde_json::Value>(&persisted).unwrap();
        let emitted_json =
            serde_json::to_value(emitter.emitted.lock().unwrap().last().unwrap()).unwrap();
        let sentinel = sentinel.to_string_lossy();
        assert!(persisted_json.get("notesRoot").is_none());
        assert!(emitted_json.get("notesRoot").is_none());
        assert!(!persisted.contains(sentinel.as_ref()));
        assert!(!emitted_json.to_string().contains(sentinel.as_ref()));
    }

    #[tokio::test]
    async fn repository_status_rejects_non_uuid_binding_ids_before_path_join() {
        let parent = tempdir().unwrap();
        let app_data = parent.path().join("app-data");
        std::fs::create_dir(&app_data).unwrap();
        let emitter = Arc::new(InspectingEmitter::new(app_data.clone()));
        let store = RepositoryStatusStore::new(&app_data, emitter);
        let status = attempting("../../outside");

        let error = store.publish(status).await.unwrap_err();

        assert_eq!(error, RepositoryJobError::InvalidBinding);
        assert!(!parent.path().join("outside/state.json").exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn repository_status_rejects_a_symlinked_repository_directory() {
        use std::os::unix::fs::symlink;

        let app_data = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let repository_id = "00000000-0000-4000-8000-000000000012";
        let repositories = app_data.path().join("sync/repositories");
        std::fs::create_dir_all(&repositories).unwrap();
        symlink(outside.path(), repositories.join(repository_id)).unwrap();
        let emitter = Arc::new(InspectingEmitter::new(app_data.path().to_path_buf()));
        let store = RepositoryStatusStore::new(app_data.path(), emitter);

        let error = store.publish(attempting(repository_id)).await.unwrap_err();

        assert_eq!(error, RepositoryJobError::StatusUnavailable);
        assert!(!outside.path().join("state.json").exists());
    }

    #[tokio::test]
    async fn schedule_fields_are_persisted_before_emit_and_survive_sync_status_updates() {
        let app_data = tempdir().unwrap();
        let repository_id = "00000000-0000-4000-8000-000000000016";
        let emitter = Arc::new(InspectingEmitter::new(app_data.path().to_path_buf()));
        let store = RepositoryStatusStore::new(app_data.path(), Arc::clone(&emitter));
        store.publish(attempting(repository_id)).await.unwrap();
        let now = time::OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();
        let schedule = super::RepositorySchedule {
            same_count: 4,
            automatic_failure_count: 7,
            last_dns_retry_at: Some(now),
            next_scheduled_at: Some(now + Duration::from_secs(300)),
        };

        store
            .save_schedule(repository_id, schedule.clone())
            .unwrap();
        let mut succeeded = attempting(repository_id);
        succeeded.phase = RepositorySyncPhase::Succeeded;
        succeeded.last_successful_sync_at = Some("2026-07-25T08:00:01Z".to_owned());
        store.publish(succeeded).await.unwrap();

        let persisted = load_repository_sync_status(app_data.path(), repository_id)
            .unwrap()
            .unwrap();
        assert_eq!(persisted.schedule, schedule);
        let json = std::fs::read_to_string(
            app_data
                .path()
                .join(format!("sync/repositories/{repository_id}/state.json")),
        )
        .unwrap();
        for field in [
            "\"sameCount\"",
            "\"automaticFailureCount\"",
            "\"lastDnsRetryAt\"",
            "\"nextScheduledAt\"",
        ] {
            assert!(
                json.contains(field),
                "missing persisted schedule field {field}"
            );
        }
        assert!(!json.contains("notesRoot"));
    }

    #[tokio::test]
    async fn dns_retry_timestamp_reservation_is_atomic_and_allows_the_five_minute_boundary() {
        let app_data = tempdir().unwrap();
        let repository_id = "00000000-0000-4000-8000-000000000017";
        let emitter = Arc::new(InspectingEmitter::new(app_data.path().to_path_buf()));
        let store = RepositoryStatusStore::new(app_data.path(), Arc::clone(&emitter));
        store.publish(attempting(repository_id)).await.unwrap();
        let now = time::OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();
        let throttle = Duration::from_secs(300);

        assert!(store
            .reserve_dns_retry(repository_id, now, throttle)
            .unwrap());
        assert!(!store
            .reserve_dns_retry(repository_id, now + Duration::from_secs(299), throttle)
            .unwrap());
        assert!(store
            .reserve_dns_retry(repository_id, now + Duration::from_secs(300), throttle)
            .unwrap());
        assert_eq!(
            store
                .load_schedule(repository_id)
                .unwrap()
                .last_dns_retry_at,
            Some(now + Duration::from_secs(300))
        );
    }
}
