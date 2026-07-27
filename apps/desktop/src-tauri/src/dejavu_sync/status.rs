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

use super::conflicts::{conflict_history_exists, SyncConflictRecord};
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

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RepositoryMaintenance {
    #[serde(with = "time::serde::rfc3339::option")]
    pub(crate) last_local_purge_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub(crate) next_local_purge_at: Option<OffsetDateTime>,
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
    #[serde(default)]
    pub(crate) maintenance: RepositoryMaintenance,
    pub(crate) error: Option<RepositorySafeError>,
    pub(crate) transfer: RepositoryTransferSummary,
    pub(crate) conflicts: Vec<SyncConflictRecord>,
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
            maintenance: RepositoryMaintenance::default(),
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
            maintenance: RepositoryMaintenance::default(),
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
            maintenance: RepositoryMaintenance::default(),
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
            status.maintenance = previous.maintenance;
            if status.last_successful_sync_at.is_none() {
                status.last_successful_sync_at = previous.last_successful_sync_at;
            }
            if status.phase == RepositorySyncPhase::Attempting {
                status.transfer = previous.transfer;
                status.conflicts = previous.conflicts;
            } else if status.conflicts.is_empty() && status.phase == RepositorySyncPhase::Failed {
                status.conflicts = previous.conflicts;
            } else if status.phase == RepositorySyncPhase::Succeeded {
                let mut unresolved = previous
                    .conflicts
                    .into_iter()
                    .filter(|conflict| {
                        conflict.resolution.is_none()
                            && conflict_history_exists(&self.app_data, conflict)
                    })
                    .collect::<Vec<_>>();
                unresolved.append(&mut status.conflicts);
                status.conflicts = unresolved;
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

    pub(crate) fn load_maintenance(
        &self,
        repository_id: &str,
    ) -> Result<RepositoryMaintenance, RepositoryJobError> {
        Ok(load_repository_sync_status(&self.app_data, repository_id)?
            .map(|status| status.maintenance)
            .unwrap_or_default())
    }

    pub(crate) fn set_maintenance(
        &self,
        repository_id: &str,
        maintenance: RepositoryMaintenance,
    ) -> Result<RepositoryMaintenance, RepositoryJobError> {
        validate_repository_id(repository_id)?;
        let _write = self.write_lock.lock().unwrap();
        let mut status = load_repository_sync_status(&self.app_data, repository_id)?
            .ok_or(RepositoryJobError::StatusUnavailable)?;
        if status.maintenance == maintenance {
            return Ok(maintenance);
        }
        status.maintenance = maintenance.clone();
        self.persist_status(&status)?;
        // Exact replacement is committed once the atomic file write succeeds;
        // event delivery remains advisory and must not turn it into a retry.
        let _notification_result = self.emitter.emit(&status);
        Ok(maintenance)
    }

    pub(crate) fn update_schedule(
        &self,
        repository_id: &str,
        update: &mut dyn FnMut(&mut RepositorySchedule) -> bool,
    ) -> Result<RepositorySchedule, RepositoryJobError> {
        validate_repository_id(repository_id)?;
        let _write = self.write_lock.lock().unwrap();
        let mut status = load_repository_sync_status(&self.app_data, repository_id)?
            .ok_or(RepositoryJobError::StatusUnavailable)?;
        if !update(&mut status.schedule) {
            return Ok(status.schedule);
        }
        let schedule = status.schedule.clone();
        self.persist_status(&status)?;
        // The schedule mutation is committed once the atomic file replacement
        // succeeds. Notification failure must not make callers retry a
        // non-idempotent mutation such as incrementing failure/same counters.
        let _notification_result = self.emitter.emit(&status);
        Ok(schedule)
    }

    pub(crate) fn reserve_dns_retry(
        &self,
        repository_id: &str,
        now: OffsetDateTime,
        throttle: std::time::Duration,
    ) -> Result<bool, RepositoryJobError> {
        let mut permitted = false;
        let mut reserve = |schedule: &mut RepositorySchedule| {
            if schedule
                .last_dns_retry_at
                .is_some_and(|last| now - last < throttle)
            {
                return false;
            }
            schedule.last_dns_retry_at = Some(now);
            permitted = true;
            true
        };
        self.update_schedule(repository_id, &mut reserve)?;
        Ok(permitted)
    }

    pub(crate) fn clear_sync_schedule(
        &self,
        repository_id: &str,
    ) -> Result<RepositorySchedule, RepositoryJobError> {
        validate_repository_id(repository_id)?;
        if load_repository_sync_status(&self.app_data, repository_id)?.is_none() {
            return Ok(RepositorySchedule::default());
        }
        let mut clear = |schedule: &mut RepositorySchedule| {
            if *schedule == RepositorySchedule::default() {
                return false;
            }
            *schedule = RepositorySchedule::default();
            true
        };
        self.update_schedule(repository_id, &mut clear)
    }

    fn persist_then_emit(&self, status: RepositorySyncStatus) -> Result<(), RepositoryJobError> {
        self.persist_status(&status)?;
        self.emitter.emit(&status)
    }

    fn persist_status(&self, status: &RepositorySyncStatus) -> Result<(), RepositoryJobError> {
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

        Ok(())
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
        if uuid::Uuid::parse_str(&conflict.conflict_id)
            .ok()
            .is_none_or(|conflict_id| conflict_id.to_string() != conflict.conflict_id)
            || conflict.repository_id != status.repository_id
        {
            return Err(RepositoryJobError::StatusUnavailable);
        }
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
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tempfile::tempdir;

    use super::{
        load_repository_sync_status, RepositoryMaintenance, RepositorySafeError,
        RepositorySchedule, RepositoryStatusEventEmitter, RepositoryStatusStore,
        RepositorySyncPhase, RepositorySyncStatus, RepositoryTransferSummary,
    };
    use crate::dejavu_sync::conflicts::SyncConflictRecord;
    use crate::dejavu_sync::service::{
        RepositoryJobError, RepositoryStatusSink, RepositorySyncResult, SyncJobRequest,
    };
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

    struct FailingEmitter;

    impl RepositoryStatusEventEmitter for FailingEmitter {
        fn emit(&self, _status: &RepositorySyncStatus) -> Result<(), RepositoryJobError> {
            Err(RepositoryJobError::StatusUnavailable)
        }
    }

    #[derive(Default)]
    struct CountingFailingEmitter {
        calls: AtomicUsize,
    }

    impl RepositoryStatusEventEmitter for CountingFailingEmitter {
        fn emit(&self, _status: &RepositorySyncStatus) -> Result<(), RepositoryJobError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err(RepositoryJobError::StatusUnavailable)
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

        let mut replace_schedule = |current: &mut super::RepositorySchedule| {
            *current = schedule.clone();
            true
        };
        store
            .update_schedule(repository_id, &mut replace_schedule)
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

    #[test]
    fn old_status_without_maintenance_loads_with_the_default_section() {
        let app_data = tempdir().unwrap();
        let repository_id = "00000000-0000-4000-8000-000000000061";
        let repository = app_data
            .path()
            .join(format!("sync/repositories/{repository_id}"));
        std::fs::create_dir_all(&repository).unwrap();
        let legacy = serde_json::json!({
            "version": 1,
            "repositoryId": repository_id,
            "phase": "succeeded",
            "trigger": "manual",
            "jobId": "00000000-0000-4000-8000-000000000091",
            "attempt": 1,
            "lastAttemptAt": "2026-07-25T08:00:00Z",
            "lastSuccessfulSyncAt": "2026-07-25T08:00:00Z",
            "sameCount": 0,
            "automaticFailureCount": 0,
            "lastDnsRetryAt": null,
            "nextScheduledAt": null,
            "error": null,
            "transfer": {
                "downloadBytes": 0,
                "downloadChunks": 0,
                "downloadFiles": 0,
                "uploadBytes": 0,
                "uploadChunks": 0,
                "uploadFiles": 0
            },
            "conflicts": []
        });
        std::fs::write(
            repository.join("state.json"),
            serde_json::to_vec_pretty(&legacy).unwrap(),
        )
        .unwrap();

        let loaded = load_repository_sync_status(app_data.path(), repository_id)
            .unwrap()
            .unwrap();

        assert_eq!(loaded.version, 1);
        assert_eq!(loaded.maintenance, RepositoryMaintenance::default());
    }

    #[tokio::test]
    async fn maintenance_survives_attempting_succeeded_failed_and_schedule_writes() {
        let app_data = tempdir().unwrap();
        let repository_id = "00000000-0000-4000-8000-000000000062";
        let emitter = Arc::new(InspectingEmitter::new(app_data.path().to_path_buf()));
        let store = RepositoryStatusStore::new(app_data.path(), emitter);
        let last = time::OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();
        let maintenance = RepositoryMaintenance {
            last_local_purge_at: Some(last),
            next_local_purge_at: Some(last + Duration::from_secs(6 * 60 * 60)),
        };
        let request = SyncJobRequest {
            notes_root: PathBuf::from("/notes/journal"),
            repository_id: repository_id.to_owned(),
            trigger: SyncTrigger::Manual,
        };
        let mut initial = attempting(repository_id);
        initial.maintenance = maintenance.clone();
        store.publish(initial).await.unwrap();
        let persisted_json = serde_json::from_slice::<serde_json::Value>(
            &std::fs::read(
                app_data
                    .path()
                    .join(format!("sync/repositories/{repository_id}/state.json")),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(persisted_json["version"], 1);
        assert_eq!(
            persisted_json["maintenance"]["lastLocalPurgeAt"],
            "2027-01-15T08:00:00Z"
        );
        assert_eq!(
            persisted_json["maintenance"]["nextLocalPurgeAt"],
            "2027-01-15T14:00:00Z"
        );

        let next_attempting = attempting(repository_id);
        assert_eq!(
            next_attempting.maintenance,
            RepositoryMaintenance::default()
        );
        store.publish(next_attempting).await.unwrap();
        assert_eq!(
            load_repository_sync_status(app_data.path(), repository_id)
                .unwrap()
                .unwrap()
                .maintenance,
            maintenance
        );
        let succeeded = RepositorySyncStatus::succeeded(
            &request,
            "00000000-0000-4000-8000-000000000092".to_owned(),
            1,
            "2026-07-25T08:00:01Z".to_owned(),
            RepositorySyncResult::default(),
        );
        assert_eq!(succeeded.maintenance, RepositoryMaintenance::default());
        store.publish(succeeded).await.unwrap();
        assert_eq!(
            load_repository_sync_status(app_data.path(), repository_id)
                .unwrap()
                .unwrap()
                .maintenance,
            maintenance
        );
        let failed = RepositorySyncStatus::failed(
            &request,
            "00000000-0000-4000-8000-000000000093".to_owned(),
            1,
            "2026-07-25T08:00:02Z".to_owned(),
            RepositorySafeError {
                code: "failure".to_owned(),
                operation: "repository-sync".to_owned(),
            },
        );
        assert_eq!(failed.maintenance, RepositoryMaintenance::default());
        store.publish(failed).await.unwrap();
        let mut schedule_update = |schedule: &mut super::RepositorySchedule| {
            schedule.same_count = 2;
            true
        };
        store
            .update_schedule(repository_id, &mut schedule_update)
            .unwrap();
        assert_eq!(
            load_repository_sync_status(app_data.path(), repository_id)
                .unwrap()
                .unwrap()
                .maintenance,
            maintenance
        );
    }

    #[tokio::test]
    async fn maintenance_replacement_commits_once_despite_emitter_failure() {
        let app_data = tempdir().unwrap();
        let repository_id = "00000000-0000-4000-8000-000000000063";
        let emitter = Arc::new(CountingFailingEmitter::default());
        let store = RepositoryStatusStore::new(app_data.path(), Arc::clone(&emitter));
        assert_eq!(
            store.publish(attempting(repository_id)).await,
            Err(RepositoryJobError::StatusUnavailable)
        );
        let last = time::OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();
        let maintenance = RepositoryMaintenance {
            last_local_purge_at: Some(last),
            next_local_purge_at: Some(last + Duration::from_secs(6 * 60 * 60)),
        };

        assert_eq!(
            store.set_maintenance(repository_id, maintenance.clone()),
            Ok(maintenance.clone())
        );
        assert_eq!(
            store.load_maintenance(repository_id),
            Ok(maintenance.clone())
        );
        assert_eq!(emitter.calls.load(Ordering::SeqCst), 2);

        assert_eq!(
            store.set_maintenance(repository_id, maintenance.clone()),
            Ok(maintenance)
        );
        assert_eq!(emitter.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn schedule_clear_is_isolated_and_idempotent() {
        let app_data = tempdir().unwrap();
        let repository_id = "00000000-0000-4000-8000-000000000064";
        let emitter = Arc::new(InspectingEmitter::new(app_data.path().to_path_buf()));
        let store = RepositoryStatusStore::new(app_data.path(), Arc::clone(&emitter));
        let now = time::OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();
        let mut status = attempting(repository_id);
        status.phase = RepositorySyncPhase::Failed;
        status.last_successful_sync_at = Some("2026-07-25T07:00:00Z".to_owned());
        status.error = Some(RepositorySafeError {
            code: "cloud-failure".to_owned(),
            operation: "repository-sync".to_owned(),
        });
        status.transfer = RepositoryTransferSummary {
            download_bytes: 11,
            download_chunks: 12,
            download_files: 13,
            upload_bytes: 21,
            upload_chunks: 22,
            upload_files: 23,
        };
        status.conflicts = vec![SyncConflictRecord {
            conflict_id: "00000000-0000-4000-8000-000000000095".to_owned(),
            repository_id: repository_id.to_owned(),
            relative_path: "notes/conflict.md".to_owned(),
            occurred_at: "2026-07-25T08:00:00Z".to_owned(),
            resolution: None,
        }];
        status.schedule = RepositorySchedule {
            same_count: 4,
            automatic_failure_count: 7,
            last_dns_retry_at: Some(now),
            next_scheduled_at: Some(now + Duration::from_secs(300)),
        };
        status.maintenance = RepositoryMaintenance {
            last_local_purge_at: Some(now - Duration::from_secs(60)),
            next_local_purge_at: Some(now + Duration::from_secs(6 * 60 * 60)),
        };
        store.publish(status).await.unwrap();
        let before = load_repository_sync_status(app_data.path(), repository_id)
            .unwrap()
            .unwrap();

        assert_eq!(
            store.clear_sync_schedule(repository_id),
            Ok(RepositorySchedule::default())
        );
        let after = load_repository_sync_status(app_data.path(), repository_id)
            .unwrap()
            .unwrap();
        let mut expected = before;
        expected.schedule = RepositorySchedule::default();
        assert_eq!(after, expected);

        let emitted_after_first_clear = emitter.emitted.lock().unwrap().len();
        assert_eq!(
            store.clear_sync_schedule(repository_id),
            Ok(RepositorySchedule::default())
        );
        assert_eq!(
            emitter.emitted.lock().unwrap().len(),
            emitted_after_first_clear
        );
    }

    #[test]
    fn clearing_an_absent_sync_schedule_is_idempotent() {
        let app_data = tempdir().unwrap();
        let store = RepositoryStatusStore::new(
            app_data.path(),
            Arc::new(InspectingEmitter::new(app_data.path().to_path_buf())),
        );

        assert_eq!(
            store.clear_sync_schedule("00000000-0000-4000-8000-000000000093"),
            Ok(RepositorySchedule::default())
        );
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

    #[tokio::test]
    async fn committed_schedule_updates_are_not_reapplied_when_event_emission_fails() {
        let app_data = tempdir().unwrap();
        let repository_id = "00000000-0000-4000-8000-000000000043";
        let store = RepositoryStatusStore::new(app_data.path(), Arc::new(FailingEmitter));
        assert_eq!(
            store.publish(attempting(repository_id)).await,
            Err(RepositoryJobError::StatusUnavailable)
        );

        let mut update_result = Err(RepositoryJobError::StatusUnavailable);
        for _attempt in 1..=3 {
            let mut increment_failure_count = |schedule: &mut super::RepositorySchedule| {
                schedule.automatic_failure_count += 1;
                true
            };
            update_result = store.update_schedule(repository_id, &mut increment_failure_count);
            if update_result.is_ok() {
                break;
            }
        }
        update_result.expect("a durable schedule write is committed despite notification failure");
        assert_eq!(
            store
                .load_schedule(repository_id)
                .unwrap()
                .automatic_failure_count,
            1
        );

        let now = time::OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();
        let throttle = Duration::from_secs(300);
        assert!(store
            .reserve_dns_retry(repository_id, now, throttle)
            .unwrap());
        assert!(!store
            .reserve_dns_retry(repository_id, now + Duration::from_secs(1), throttle)
            .unwrap());
    }
}
