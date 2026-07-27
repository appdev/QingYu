use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use cap_fs_ext::DirExt;
use qingyu_dejavu::{
    write_cap_file_safer, ExpectedRevision, RepoError, RepositoryRelativePath, WorkingTreeAction,
    WorkingTreeChange,
};
use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use super::local_state::LocalSyncStateService;
use super::repository::WorkingTreeCoordinatorFactory;
use super::service::{
    AcceptedSyncJob, DejavuSyncService, JobCancellationToken, RepositoryJobError,
    SyncAttemptContext, SyncJobRequest,
};
use super::status::{load_repository_sync_status, RepositoryStatusStore, RepositorySyncStatus};
use crate::storage_capability::{
    nonfollowing_read_options, open_canonical_directory_nofollow, unique_regular_file_identity,
    UniqueRegularFileIdentity,
};
use crate::sync_config::status::SyncTrigger;

const MAX_CONFLICT_TEXT_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ConflictResolutionKind {
    KeepLocal,
    UseRemote,
    KeepBoth,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SyncConflictRecord {
    pub(crate) conflict_id: String,
    pub(crate) repository_id: String,
    pub(crate) relative_path: String,
    pub(crate) occurred_at: String,
    pub(crate) resolution: Option<ConflictResolutionKind>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConflictVersion {
    pub(crate) byte_size: u64,
    pub(crate) text: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConflictVersions {
    pub(crate) conflict: SyncConflictRecord,
    pub(crate) local: Option<ConflictVersion>,
    pub(crate) remote: ConflictVersion,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    tag = "kind",
    deny_unknown_fields
)]
pub(crate) enum ConflictResolution {
    KeepLocal,
    UseRemote,
    KeepBoth { destination_relative_path: String },
}

impl ConflictResolution {
    fn kind(&self) -> ConflictResolutionKind {
        match self {
            Self::KeepLocal => ConflictResolutionKind::KeepLocal,
            Self::UseRemote => ConflictResolutionKind::UseRemote,
            Self::KeepBoth { .. } => ConflictResolutionKind::KeepBoth,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ResolveConflictRequest {
    pub(crate) repository_id: String,
    pub(crate) conflict_id: String,
    pub(crate) resolution: ConflictResolution,
}

pub(crate) struct ConflictStore {
    app_data: PathBuf,
}

pub(crate) struct ConflictResolver {
    app_data: PathBuf,
    service: DejavuSyncService,
    store: Arc<ConflictStore>,
    status: Arc<RepositoryStatusStore>,
    coordinator_factory: Arc<dyn WorkingTreeCoordinatorFactory>,
}

impl ConflictStore {
    pub(crate) fn new(app_data: impl AsRef<Path>) -> Self {
        Self {
            app_data: app_data.as_ref().to_path_buf(),
        }
    }

    pub(crate) fn app_data(&self) -> &Path {
        &self.app_data
    }

    pub(crate) fn key_configured(&self) -> Result<bool, RepositoryJobError> {
        LocalSyncStateService::new(&self.app_data)
            .load()
            .map(|state| state.is_some())
            .map_err(RepositoryJobError::from)
    }

    pub(crate) fn export_key(&self) -> Result<String, RepositoryJobError> {
        LocalSyncStateService::new(&self.app_data)
            .load()
            .map_err(RepositoryJobError::from)?
            .map(|state| state.repo_key)
            .ok_or(RepositoryJobError::InvalidBinding)
    }

    pub(crate) fn list(
        &self,
        repository_id: &str,
    ) -> Result<Vec<SyncConflictRecord>, RepositoryJobError> {
        canonical_uuid(repository_id)?;
        let status = load_repository_sync_status(&self.app_data, repository_id)?;
        Ok(status
            .map(|status| {
                status
                    .conflicts
                    .into_iter()
                    .filter(|conflict| {
                        conflict.resolution.is_none()
                            && conflict_history_exists(&self.app_data, conflict)
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    pub(crate) fn read(
        &self,
        repository_id: &str,
        conflict_id: &str,
    ) -> Result<ConflictVersions, RepositoryJobError> {
        canonical_uuid(repository_id)?;
        canonical_uuid(conflict_id)?;
        let conflict = load_repository_sync_status(&self.app_data, repository_id)?
            .and_then(|status| {
                status
                    .conflicts
                    .into_iter()
                    .find(|conflict| conflict.conflict_id == conflict_id)
            })
            .filter(|conflict| {
                conflict.repository_id == repository_id && conflict.resolution.is_none()
            })
            .ok_or(RepositoryJobError::ConflictUnavailable)?;
        let state = LocalSyncStateService::new(&self.app_data)
            .load()
            .map_err(RepositoryJobError::from)?
            .ok_or(RepositoryJobError::ConflictUnavailable)?;
        let notes_root = state
            .bindings
            .iter()
            .find(|binding| binding.repository_id == repository_id)
            .map(|binding| binding.notes_root.clone())
            .ok_or(RepositoryJobError::ConflictUnavailable)?;
        let relative = validated_relative_path(&conflict.relative_path)?;
        let history_root = conflict_history_root(&self.app_data, &conflict)?;
        let remote = read_version(&history_root, &relative)?
            .ok_or(RepositoryJobError::ConflictUnavailable)?;
        let local = read_version(&notes_root, &relative)?;
        Ok(ConflictVersions {
            conflict,
            local,
            remote,
        })
    }

    pub(crate) fn status_for_root(
        &self,
        notes_root: &Path,
    ) -> Result<Option<RepositorySyncStatus>, RepositoryJobError> {
        let notes_root = notes_root
            .canonicalize()
            .map_err(|_| RepositoryJobError::InvalidBinding)?;
        let Some(state) = LocalSyncStateService::new(&self.app_data)
            .load()
            .map_err(RepositoryJobError::from)?
        else {
            return Ok(None);
        };
        let Some(binding) = state
            .bindings
            .iter()
            .find(|binding| binding.enabled && binding.notes_root == notes_root)
        else {
            return Ok(None);
        };
        load_repository_sync_status(&self.app_data, &binding.repository_id)
    }

    fn find(
        &self,
        repository_id: &str,
        conflict_id: &str,
    ) -> Result<SyncConflictRecord, RepositoryJobError> {
        load_repository_sync_status(&self.app_data, repository_id)?
            .and_then(|status| {
                status.conflicts.into_iter().find(|conflict| {
                    conflict.conflict_id == conflict_id
                        && conflict.repository_id == repository_id
                        && conflict.resolution.is_none()
                })
            })
            .filter(|conflict| conflict_history_exists(&self.app_data, conflict))
            .ok_or(RepositoryJobError::ConflictUnavailable)
    }
}

impl ConflictResolver {
    pub(crate) fn new<CoordinatorFactory>(
        app_data: impl AsRef<Path>,
        service: DejavuSyncService,
        store: Arc<ConflictStore>,
        status: Arc<RepositoryStatusStore>,
        coordinator_factory: Arc<CoordinatorFactory>,
    ) -> Self
    where
        CoordinatorFactory: WorkingTreeCoordinatorFactory + 'static,
    {
        let coordinator_factory: Arc<dyn WorkingTreeCoordinatorFactory> = coordinator_factory;
        Self {
            app_data: app_data.as_ref().to_path_buf(),
            service,
            store,
            status,
            coordinator_factory,
        }
    }

    pub(crate) async fn resolve(
        &self,
        request: ResolveConflictRequest,
    ) -> Result<AcceptedSyncJob, RepositoryJobError> {
        canonical_uuid(&request.repository_id)?;
        canonical_uuid(&request.conflict_id)?;
        let reservation = self
            .service
            .reserve_repository_maintenance(&request.repository_id)?;
        let state = LocalSyncStateService::new(&self.app_data)
            .load()
            .map_err(RepositoryJobError::from)?
            .ok_or(RepositoryJobError::ConflictUnavailable)?;
        let binding = state
            .bindings
            .iter()
            .find(|binding| binding.repository_id == request.repository_id && binding.enabled)
            .cloned()
            .ok_or(RepositoryJobError::ConflictUnavailable)?;
        let status = Arc::clone(&self.status);
        let store = Arc::clone(&self.store);
        let factory = Arc::clone(&self.coordinator_factory);
        let repository_id = request.repository_id.clone();
        let conflict_id = request.conflict_id.clone();
        let resolution_kind = request.resolution.kind();
        let job_id = uuid::Uuid::new_v4().to_string();
        let notes_root = binding.notes_root.clone();
        let cancellation = JobCancellationToken::new();
        let sync_request = SyncJobRequest {
            notes_root: notes_root.clone(),
            repository_id: repository_id.clone(),
            trigger: SyncTrigger::Manual,
        };
        let coordinator = factory.create(&SyncAttemptContext {
            request: sync_request.clone(),
            job_id,
            attempt: 1,
            cancellation,
        })?;
        let reserved_repository_id = repository_id.clone();
        let reserved_notes_root = notes_root.clone();

        reservation
            .run(async move {
                let conflict = store.find(&reserved_repository_id, &conflict_id)?;
                match request.resolution {
                    ConflictResolution::KeepLocal => {}
                    ConflictResolution::UseRemote => {
                        let source_relative = validated_relative_path(&conflict.relative_path)?;
                        let history_root = conflict_history_root(&store.app_data, &conflict)?;
                        let (remote_bytes, _) = read_all_bytes(&history_root, &source_relative)?
                            .ok_or(RepositoryJobError::ConflictUnavailable)?;
                        let before = file_identity_at(&reserved_notes_root, &source_relative)?;
                        let change = WorkingTreeChange {
                            path: RepositoryRelativePath::new(conflict.relative_path.clone())
                                .map_err(|_| RepositoryJobError::ConflictUnavailable)?,
                            expected_revision: ExpectedRevision::Absent,
                            action: WorkingTreeAction::Write,
                        };
                        let permit = coordinator
                            .prepare(std::slice::from_ref(&change))
                            .await
                            .map_err(map_working_tree_error)?;
                        let operation = (|| {
                            if file_identity_at(&reserved_notes_root, &source_relative)? != before {
                                return Err(RepositoryJobError::WorkingTreeChanged);
                            }
                            if let Some((local_bytes, _)) =
                                read_all_bytes(&reserved_notes_root, &source_relative)?
                            {
                                let archive_relative =
                                    PathBuf::from(".qingyu-local").join(&source_relative);
                                write_relative_file(
                                    &history_root,
                                    &archive_relative,
                                    &local_bytes,
                                    false,
                                )?;
                            }
                            write_relative_file(
                                &reserved_notes_root,
                                &source_relative,
                                &remote_bytes,
                                false,
                            )
                        })();
                        coordinator.release(permit).await;
                        operation?;
                    }
                    ConflictResolution::KeepBoth {
                        destination_relative_path,
                    } => {
                        let source_relative = validated_relative_path(&conflict.relative_path)?;
                        let destination = validated_relative_path(&destination_relative_path)?;
                        if destination == source_relative {
                            return Err(RepositoryJobError::ConflictUnavailable);
                        }
                        let history_root = conflict_history_root(&store.app_data, &conflict)?;
                        let (remote_bytes, _) = read_all_bytes(&history_root, &source_relative)?
                            .ok_or(RepositoryJobError::ConflictUnavailable)?;
                        let before = file_identity_at(&reserved_notes_root, &destination)?;
                        if before.is_some() {
                            return Err(RepositoryJobError::ConflictUnavailable);
                        }
                        let change = WorkingTreeChange {
                            path: RepositoryRelativePath::new(destination_relative_path)
                                .map_err(|_| RepositoryJobError::ConflictUnavailable)?,
                            expected_revision: ExpectedRevision::Absent,
                            action: WorkingTreeAction::Write,
                        };
                        let permit = coordinator
                            .prepare(std::slice::from_ref(&change))
                            .await
                            .map_err(map_working_tree_error)?;
                        let operation = (|| {
                            if file_identity_at(&reserved_notes_root, &destination)? != before {
                                return Err(RepositoryJobError::WorkingTreeChanged);
                            }
                            write_relative_file(
                                &reserved_notes_root,
                                &destination,
                                &remote_bytes,
                                true,
                            )
                        })();
                        coordinator.release(permit).await;
                        operation?;
                    }
                }
                status.resolve_conflict(&reserved_repository_id, &conflict_id, resolution_kind)?;
                Ok::<_, RepositoryJobError>(())
            })
            .await?;

        if resolution_kind == ConflictResolutionKind::KeepLocal {
            return Ok(AcceptedSyncJob::completed(repository_id, notes_root));
        }
        self.service.enqueue(sync_request).await
    }
}

pub(crate) fn conflict_history_exists(app_data: &Path, conflict: &SyncConflictRecord) -> bool {
    let Ok(relative) = validated_relative_path(&conflict.relative_path) else {
        return false;
    };
    let Ok(root) = conflict_history_root(app_data, conflict) else {
        return false;
    };
    read_version(&root, &relative).is_ok_and(|version| version.is_some())
}

fn conflict_history_root(
    app_data: &Path,
    conflict: &SyncConflictRecord,
) -> Result<PathBuf, RepositoryJobError> {
    canonical_uuid(&conflict.repository_id)?;
    let occurred = OffsetDateTime::parse(&conflict.occurred_at, &Rfc3339)
        .map_err(|_| RepositoryJobError::ConflictUnavailable)?;
    let snapshot = format!(
        "{:04}-{:02}-{:02}-{:02}{:02}{:02}-sync",
        occurred.year(),
        u8::from(occurred.month()),
        occurred.day(),
        occurred.hour(),
        occurred.minute(),
        occurred.second()
    );
    Ok(app_data
        .join("sync/repositories")
        .join(&conflict.repository_id)
        .join("history")
        .join(snapshot))
}

fn read_version(
    root: &Path,
    relative: &Path,
) -> Result<Option<ConflictVersion>, RepositoryJobError> {
    let root = match open_canonical_directory_nofollow(root) {
        Ok(root) => root,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(RepositoryJobError::ConflictUnavailable),
    };
    let mut components = relative.components().peekable();
    let mut directory = root;
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            return Err(RepositoryJobError::ConflictUnavailable);
        };
        if components.peek().is_some() {
            directory = match directory.open_dir_nofollow(name) {
                Ok(directory) => directory,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(_) => return Err(RepositoryJobError::ConflictUnavailable),
            };
            continue;
        }
        let addressed = match directory.symlink_metadata(name) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(RepositoryJobError::ConflictUnavailable),
        };
        let identity = unique_regular_file_identity(&addressed)
            .ok_or(RepositoryJobError::ConflictUnavailable)?;
        let mut file = directory
            .open_with(name, &nonfollowing_read_options())
            .map_err(|_| RepositoryJobError::ConflictUnavailable)?;
        let retained = file
            .metadata()
            .map_err(|_| RepositoryJobError::ConflictUnavailable)?;
        if !identity.matches_retained_regular_file(&retained, false) {
            return Err(RepositoryJobError::ConflictUnavailable);
        }
        let byte_size = retained.len();
        let text = if byte_size <= MAX_CONFLICT_TEXT_BYTES {
            let mut bytes = Vec::with_capacity(byte_size as usize);
            Read::by_ref(&mut file)
                .take(MAX_CONFLICT_TEXT_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| RepositoryJobError::ConflictUnavailable)?;
            if bytes.len() as u64 > MAX_CONFLICT_TEXT_BYTES {
                None
            } else {
                String::from_utf8(bytes).ok()
            }
        } else {
            None
        };
        let final_metadata = file
            .metadata()
            .map_err(|_| RepositoryJobError::ConflictUnavailable)?;
        if !identity.matches_retained_regular_file(&final_metadata, false) {
            return Err(RepositoryJobError::ConflictUnavailable);
        }
        return Ok(Some(ConflictVersion { byte_size, text }));
    }
    Err(RepositoryJobError::ConflictUnavailable)
}

fn read_all_bytes(
    root: &Path,
    relative: &Path,
) -> Result<Option<(Vec<u8>, UniqueRegularFileIdentity)>, RepositoryJobError> {
    let root = match open_canonical_directory_nofollow(root) {
        Ok(root) => root,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(RepositoryJobError::ConflictUnavailable),
    };
    let (directory, name) = open_relative_parent(root, relative, false)?;
    let Some(name) = name else {
        return Ok(None);
    };
    let addressed = match directory.symlink_metadata(&name) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(RepositoryJobError::ConflictUnavailable),
    };
    let identity =
        unique_regular_file_identity(&addressed).ok_or(RepositoryJobError::ConflictUnavailable)?;
    let mut file = directory
        .open_with(&name, &nonfollowing_read_options())
        .map_err(|_| RepositoryJobError::ConflictUnavailable)?;
    let retained = file
        .metadata()
        .map_err(|_| RepositoryJobError::ConflictUnavailable)?;
    if !identity.matches_retained_regular_file(&retained, false) {
        return Err(RepositoryJobError::ConflictUnavailable);
    }
    let mut bytes = Vec::with_capacity(usize::try_from(retained.len()).unwrap_or(0));
    file.read_to_end(&mut bytes)
        .map_err(|_| RepositoryJobError::ConflictUnavailable)?;
    let final_metadata = file
        .metadata()
        .map_err(|_| RepositoryJobError::ConflictUnavailable)?;
    if !identity.matches_retained_regular_file(&final_metadata, false) {
        return Err(RepositoryJobError::ConflictUnavailable);
    }
    Ok(Some((bytes, identity)))
}

fn file_identity_at(
    root: &Path,
    relative: &Path,
) -> Result<Option<UniqueRegularFileIdentity>, RepositoryJobError> {
    read_all_bytes(root, relative).map(|value| value.map(|(_, identity)| identity))
}

fn write_relative_file(
    root: &Path,
    relative: &Path,
    bytes: &[u8],
    require_absent: bool,
) -> Result<(), RepositoryJobError> {
    let root = open_canonical_directory_nofollow(root)
        .map_err(|_| RepositoryJobError::ConflictUnavailable)?;
    let (directory, name) = open_relative_parent(root, relative, true)?;
    let name = name.ok_or(RepositoryJobError::ConflictUnavailable)?;
    match directory.symlink_metadata(&name) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(RepositoryJobError::ConflictUnavailable)
        }
        Ok(_) if require_absent => return Err(RepositoryJobError::ConflictUnavailable),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(RepositoryJobError::ConflictUnavailable),
    }
    write_cap_file_safer(&directory, &name, bytes, 0o600)
        .map_err(|_| RepositoryJobError::ConflictUnavailable)
}

fn open_relative_parent(
    root: cap_std::fs::Dir,
    relative: &Path,
    create: bool,
) -> Result<(cap_std::fs::Dir, Option<std::ffi::OsString>), RepositoryJobError> {
    let mut components = relative.components().peekable();
    let mut directory = root;
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            return Err(RepositoryJobError::ConflictUnavailable);
        };
        if components.peek().is_none() {
            return Ok((directory, Some(name.to_os_string())));
        }
        directory = match directory.open_dir_nofollow(name) {
            Ok(directory) => directory,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && create => {
                match directory.create_dir(name) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(_) => return Err(RepositoryJobError::ConflictUnavailable),
                }
                directory
                    .open_dir_nofollow(name)
                    .map_err(|_| RepositoryJobError::ConflictUnavailable)?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok((directory, None))
            }
            Err(_) => return Err(RepositoryJobError::ConflictUnavailable),
        };
    }
    Ok((directory, None))
}

fn map_working_tree_error(error: RepoError) -> RepositoryJobError {
    match error {
        RepoError::WorkingTreeChanged => RepositoryJobError::WorkingTreeChanged,
        RepoError::Cancelled => RepositoryJobError::Cancelled,
        _ => RepositoryJobError::RepositoryUnavailable,
    }
}

fn validated_relative_path(relative_path: &str) -> Result<PathBuf, RepositoryJobError> {
    RepositoryRelativePath::new(relative_path.to_owned())
        .map(|relative| PathBuf::from(relative.as_str()))
        .map_err(|_| RepositoryJobError::ConflictUnavailable)
}

fn canonical_uuid(value: &str) -> Result<(), RepositoryJobError> {
    let parsed =
        uuid::Uuid::parse_str(value).map_err(|_| RepositoryJobError::ConflictUnavailable)?;
    if parsed.to_string() != value {
        return Err(RepositoryJobError::ConflictUnavailable);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::path::PathBuf;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use qingyu_dejavu::{WorkingTreeChange, WorkingTreeCoordinator, WorkingTreePermit};
    use tempfile::tempdir;

    use super::{
        ConflictResolution, ConflictResolutionKind, ConflictResolver, ConflictStore,
        ResolveConflictRequest, SyncConflictRecord, MAX_CONFLICT_TEXT_BYTES,
    };
    use crate::dejavu_sync::local_state::{LocalSyncStateService, RepositoryBinding};
    use crate::dejavu_sync::repository::WorkingTreeCoordinatorFactory;
    use crate::dejavu_sync::service::{
        DejavuSyncService, RepositoryJobError, RepositoryJobRunner, RepositoryStatusSink,
        RepositorySyncResult, SyncAttemptContext, SyncJobRequest,
    };
    use crate::dejavu_sync::status::{
        load_repository_sync_status, RepositoryStatusEventEmitter, RepositoryStatusStore,
        RepositorySyncStatus,
    };
    use crate::sync_config::status::SyncTrigger;

    const REPOSITORY_ID: &str = "00000000-0000-4000-8000-0000000000c1";
    const OTHER_REPOSITORY_ID: &str = "00000000-0000-4000-8000-0000000000c2";
    const CONFLICT_ID: &str = "00000000-0000-4000-8000-0000000000c3";
    const OCCURRED_AT: &str = "2026-07-25T14:22:33Z";

    struct NoopEmitter;

    impl RepositoryStatusEventEmitter for NoopEmitter {
        fn emit(&self, _status: &RepositorySyncStatus) -> Result<(), RepositoryJobError> {
            Ok(())
        }
    }

    struct NoopRunner;

    impl RepositoryJobRunner for NoopRunner {
        fn validate(&self, request: SyncJobRequest) -> Result<SyncJobRequest, RepositoryJobError> {
            Ok(request)
        }

        fn run_attempt<'a>(
            &'a self,
            _context: SyncAttemptContext,
        ) -> Pin<
            Box<dyn Future<Output = Result<RepositorySyncResult, RepositoryJobError>> + Send + 'a>,
        > {
            Box::pin(async { Ok(RepositorySyncResult::default()) })
        }
    }

    #[derive(Default)]
    struct RecordingCoordinator {
        prepared: Mutex<Vec<Vec<String>>>,
        releases: AtomicUsize,
    }

    impl WorkingTreeCoordinator for RecordingCoordinator {
        fn prepare<'life0, 'life1, 'async_trait>(
            &'life0 self,
            changes: &'life1 [WorkingTreeChange],
        ) -> Pin<
            Box<
                dyn Future<Output = Result<WorkingTreePermit, qingyu_dejavu::RepoError>>
                    + Send
                    + 'async_trait,
            >,
        >
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async move {
                self.prepared.lock().unwrap().push(
                    changes
                        .iter()
                        .map(|change| change.path.as_str().to_owned())
                        .collect(),
                );
                Ok(WorkingTreePermit::new(()))
            })
        }

        fn release<'life0, 'async_trait>(
            &'life0 self,
            _permit: WorkingTreePermit,
        ) -> Pin<Box<dyn Future<Output = ()> + Send + 'async_trait>>
        where
            'life0: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async move {
                self.releases.fetch_add(1, Ordering::SeqCst);
            })
        }
    }

    struct RecordingCoordinatorFactory {
        coordinator: Arc<RecordingCoordinator>,
    }

    impl WorkingTreeCoordinatorFactory for RecordingCoordinatorFactory {
        fn create(
            &self,
            _context: &SyncAttemptContext,
        ) -> Result<Arc<dyn WorkingTreeCoordinator>, RepositoryJobError> {
            Ok(self.coordinator.clone())
        }
    }

    struct Fixture {
        _temporary: tempfile::TempDir,
        app_data: PathBuf,
        notes_root: PathBuf,
        history_file: PathBuf,
    }

    impl Fixture {
        async fn new(remote_bytes: &[u8]) -> Self {
            let temporary = tempdir().unwrap();
            let app_data = temporary.path().join("app-data");
            let notes_root = temporary.path().join("notes");
            std::fs::create_dir_all(&app_data).unwrap();
            std::fs::create_dir_all(&notes_root).unwrap();
            std::fs::write(notes_root.join("document.md"), b"local text").unwrap();
            let notes_root = notes_root.canonicalize().unwrap();
            let state_service = LocalSyncStateService::new(&app_data);
            let mut state = state_service
                .load_or_initialize(Some(&STANDARD.encode([3_u8; 32])))
                .unwrap();
            state_service
                .bind_repository(
                    &mut state,
                    RepositoryBinding {
                        repository_id: REPOSITORY_ID.to_owned(),
                        display_name: "Notes".to_owned(),
                        notes_root: notes_root.clone(),
                        enabled: true,
                    },
                )
                .unwrap();
            let history_file = app_data
                .join("sync/repositories")
                .join(REPOSITORY_ID)
                .join("history/2026-07-25-142233-sync/document.md");
            std::fs::create_dir_all(history_file.parent().unwrap()).unwrap();
            std::fs::write(&history_file, remote_bytes).unwrap();
            let request = SyncJobRequest {
                notes_root: notes_root.clone(),
                repository_id: REPOSITORY_ID.to_owned(),
                trigger: SyncTrigger::Manual,
            };
            let status = RepositorySyncStatus::succeeded(
                &request,
                "00000000-0000-4000-8000-0000000000c4".to_owned(),
                1,
                "2026-07-25T14:22:34Z".to_owned(),
                RepositorySyncResult {
                    data_changed: true,
                    transfer: Default::default(),
                    conflicts: vec![SyncConflictRecord {
                        conflict_id: CONFLICT_ID.to_owned(),
                        repository_id: REPOSITORY_ID.to_owned(),
                        relative_path: "document.md".to_owned(),
                        occurred_at: OCCURRED_AT.to_owned(),
                        resolution: None,
                    }],
                },
            );
            RepositoryStatusStore::new(&app_data, Arc::new(NoopEmitter))
                .publish(status)
                .await
                .unwrap();
            Self {
                _temporary: temporary,
                app_data,
                notes_root,
                history_file,
            }
        }

        fn resolver(&self) -> (ConflictResolver, Arc<RecordingCoordinator>) {
            let status = Arc::new(RepositoryStatusStore::new(
                &self.app_data,
                Arc::new(NoopEmitter),
            ));
            let service = DejavuSyncService::new(Arc::new(NoopRunner), Arc::clone(&status));
            let coordinator = Arc::new(RecordingCoordinator::default());
            let resolver = ConflictResolver::new(
                &self.app_data,
                service,
                Arc::new(ConflictStore::new(&self.app_data)),
                status,
                Arc::new(RecordingCoordinatorFactory {
                    coordinator: Arc::clone(&coordinator),
                }),
            );
            (resolver, coordinator)
        }

        fn resolution(&self) -> Option<ConflictResolutionKind> {
            load_repository_sync_status(&self.app_data, REPOSITORY_ID)
                .unwrap()
                .unwrap()
                .conflicts
                .into_iter()
                .find(|conflict| conflict.conflict_id == CONFLICT_ID)
                .unwrap()
                .resolution
        }
    }

    #[tokio::test]
    async fn list_and_read_return_only_relative_metadata_and_bounded_text_versions() {
        let fixture = Fixture::new(b"remote text").await;
        let store = ConflictStore::new(&fixture.app_data);

        assert!(store.key_configured().unwrap());
        assert_eq!(store.export_key().unwrap(), STANDARD.encode([3_u8; 32]));
        let listed = store.list(REPOSITORY_ID).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].relative_path, "document.md");
        assert_eq!(
            store
                .status_for_root(&fixture.notes_root)
                .unwrap()
                .unwrap()
                .repository_id,
            REPOSITORY_ID
        );
        let versions = store.read(REPOSITORY_ID, CONFLICT_ID).unwrap();
        assert_eq!(
            versions.local.as_ref().unwrap().text.as_deref(),
            Some("local text")
        );
        assert_eq!(versions.remote.text.as_deref(), Some("remote text"));
        let json = serde_json::to_string(&versions).unwrap();
        assert!(!json.contains(fixture.app_data.to_string_lossy().as_ref()));
        assert!(!json.contains(fixture.notes_root.to_string_lossy().as_ref()));
    }

    #[tokio::test]
    async fn missing_history_and_another_repository_are_safely_unavailable() {
        let fixture = Fixture::new(b"remote text").await;
        std::fs::remove_file(&fixture.history_file).unwrap();
        let store = ConflictStore::new(&fixture.app_data);

        assert!(store.list(REPOSITORY_ID).unwrap().is_empty());
        assert_eq!(
            store.read(REPOSITORY_ID, CONFLICT_ID),
            Err(RepositoryJobError::ConflictUnavailable)
        );
        assert_eq!(
            store.read(OTHER_REPOSITORY_ID, CONFLICT_ID),
            Err(RepositoryJobError::ConflictUnavailable)
        );
    }

    #[tokio::test]
    async fn oversized_and_binary_versions_report_size_without_loading_text() {
        let oversized = vec![b'x'; MAX_CONFLICT_TEXT_BYTES as usize + 1];
        let fixture = Fixture::new(&oversized).await;
        std::fs::write(fixture.notes_root.join("document.md"), [0xff, 0xfe]).unwrap();

        let versions = ConflictStore::new(&fixture.app_data)
            .read(REPOSITORY_ID, CONFLICT_ID)
            .unwrap();
        assert_eq!(versions.remote.byte_size, MAX_CONFLICT_TEXT_BYTES + 1);
        assert_eq!(versions.remote.text, None);
        assert_eq!(versions.local.unwrap().text, None);
    }

    #[tokio::test]
    async fn keep_local_marks_the_conflict_without_touching_the_working_tree() {
        let fixture = Fixture::new(b"remote text").await;
        let (resolver, coordinator) = fixture.resolver();

        let accepted = resolver
            .resolve(ResolveConflictRequest {
                repository_id: REPOSITORY_ID.to_owned(),
                conflict_id: CONFLICT_ID.to_owned(),
                resolution: ConflictResolution::KeepLocal,
            })
            .await
            .unwrap();
        accepted.wait_for_completion().await.unwrap();

        assert_eq!(
            std::fs::read(fixture.notes_root.join("document.md")).unwrap(),
            b"local text"
        );
        assert_eq!(
            fixture.resolution(),
            Some(ConflictResolutionKind::KeepLocal)
        );
        assert!(coordinator.prepared.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn use_remote_archives_local_then_replaces_and_enqueues_sync() {
        let fixture = Fixture::new(b"remote text").await;
        let (resolver, coordinator) = fixture.resolver();

        let accepted = resolver
            .resolve(ResolveConflictRequest {
                repository_id: REPOSITORY_ID.to_owned(),
                conflict_id: CONFLICT_ID.to_owned(),
                resolution: ConflictResolution::UseRemote,
            })
            .await
            .unwrap();
        accepted.wait_for_completion().await.unwrap();

        assert_eq!(
            std::fs::read(fixture.notes_root.join("document.md")).unwrap(),
            b"remote text"
        );
        assert_eq!(
            std::fs::read(
                fixture
                    .history_file
                    .parent()
                    .unwrap()
                    .join(".qingyu-local/document.md")
            )
            .unwrap(),
            b"local text"
        );
        assert_eq!(
            fixture.resolution(),
            Some(ConflictResolutionKind::UseRemote)
        );
        assert_eq!(
            coordinator.prepared.lock().unwrap().as_slice(),
            &[vec!["document.md".to_owned()]]
        );
        assert_eq!(coordinator.releases.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn keep_both_writes_remote_to_an_absent_destination_and_rejects_traversal() {
        let fixture = Fixture::new(b"remote text").await;
        let (resolver, coordinator) = fixture.resolver();

        let traversal = resolver
            .resolve(ResolveConflictRequest {
                repository_id: REPOSITORY_ID.to_owned(),
                conflict_id: CONFLICT_ID.to_owned(),
                resolution: ConflictResolution::KeepBoth {
                    destination_relative_path: "../escaped.md".to_owned(),
                },
            })
            .await;
        assert!(matches!(
            traversal,
            Err(RepositoryJobError::ConflictUnavailable)
        ));
        assert_eq!(fixture.resolution(), None);

        let accepted = resolver
            .resolve(ResolveConflictRequest {
                repository_id: REPOSITORY_ID.to_owned(),
                conflict_id: CONFLICT_ID.to_owned(),
                resolution: ConflictResolution::KeepBoth {
                    destination_relative_path: "Copies/document.remote.md".to_owned(),
                },
            })
            .await
            .unwrap();
        accepted.wait_for_completion().await.unwrap();

        assert_eq!(
            std::fs::read(fixture.notes_root.join("document.md")).unwrap(),
            b"local text"
        );
        assert_eq!(
            std::fs::read(fixture.notes_root.join("Copies/document.remote.md")).unwrap(),
            b"remote text"
        );
        assert!(!fixture
            .notes_root
            .parent()
            .unwrap()
            .join("escaped.md")
            .exists());
        assert_eq!(fixture.resolution(), Some(ConflictResolutionKind::KeepBoth));
        assert_eq!(
            coordinator.prepared.lock().unwrap().as_slice(),
            &[vec!["Copies/document.remote.md".to_owned()]]
        );
        assert_eq!(coordinator.releases.load(Ordering::SeqCst), 1);
    }
}
