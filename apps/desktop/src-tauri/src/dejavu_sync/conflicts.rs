use std::ffi::OsStr;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use cap_fs_ext::DirExt;
use qingyu_dejavu::{
    write_cap_file_no_replace_safer, ExpectedRevision, RepoError, RepositoryRelativePath,
    WorkingTreeAction, WorkingTreeChange, WorkingTreeCoordinator,
};
use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use super::local_state::LocalSyncStateService;
use super::service::RepositoryJobError;
use super::status::{load_repository_sync_status, RepositorySyncStatus};
use crate::storage_capability::{
    directory_identity, nonfollowing_read_options, open_canonical_directory_nofollow,
    unique_regular_file_identity, DirectoryIdentity, UniqueRegularFileIdentity,
};

const MAX_CONFLICT_TEXT_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ConflictResolutionKind {
    KeepLocal,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ConflictCopyStatus {
    #[default]
    NotRequested,
    Generated,
    Skipped,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ConflictCopyOutcome {
    pub(crate) status: ConflictCopyStatus,
    pub(crate) relative_path: Option<String>,
    pub(crate) error: Option<String>,
}

impl ConflictCopyOutcome {
    fn generated(relative_path: String) -> Self {
        Self {
            status: ConflictCopyStatus::Generated,
            relative_path: Some(relative_path),
            error: None,
        }
    }

    fn skipped() -> Self {
        Self {
            status: ConflictCopyStatus::Skipped,
            relative_path: None,
            error: None,
        }
    }

    fn failed(error: RepositoryJobError) -> Self {
        Self {
            status: ConflictCopyStatus::Failed,
            relative_path: None,
            error: Some(error.safe_code().to_owned()),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SyncConflictRecord {
    pub(crate) conflict_id: String,
    pub(crate) repository_id: String,
    pub(crate) relative_path: String,
    pub(crate) occurred_at: String,
    pub(crate) resolution: Option<ConflictResolutionKind>,
    #[serde(default)]
    pub(crate) copy_status: ConflictCopyStatus,
    #[serde(default)]
    pub(crate) copy_path: Option<String>,
    #[serde(default)]
    pub(crate) copy_error: Option<String>,
}

impl SyncConflictRecord {
    pub(crate) fn apply_copy_outcome(&mut self, outcome: ConflictCopyOutcome) {
        self.copy_status = outcome.status;
        self.copy_path = outcome.relative_path;
        self.copy_error = outcome.error;
    }
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

pub(crate) struct ConflictStore {
    app_data: PathBuf,
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

    pub(crate) fn list_history(
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
                    .filter(|conflict| conflict_history_exists(&self.app_data, conflict))
                    .collect()
            })
            .unwrap_or_default())
    }

    pub(crate) fn read_history(
        &self,
        notes_root: &Path,
        repository_id: &str,
        conflict_id: &str,
    ) -> Result<ConflictVersions, RepositoryJobError> {
        canonical_uuid(repository_id)?;
        canonical_uuid(conflict_id)?;
        let notes_root = notes_root
            .canonicalize()
            .map_err(|_| RepositoryJobError::ConflictUnavailable)?;
        let conflict = load_repository_sync_status(&self.app_data, repository_id)?
            .and_then(|status| {
                status
                    .conflicts
                    .into_iter()
                    .find(|conflict| conflict.conflict_id == conflict_id)
            })
            .filter(|conflict| conflict.repository_id == repository_id)
            .ok_or(RepositoryJobError::ConflictUnavailable)?;
        let state = LocalSyncStateService::new(&self.app_data)
            .load()
            .map_err(RepositoryJobError::from)?
            .ok_or(RepositoryJobError::ConflictUnavailable)?;
        let binding_matches_request = state.bindings.iter().any(|binding| {
            binding.enabled
                && binding.repository_id == repository_id
                && binding.notes_root == notes_root
        });
        if !binding_matches_request {
            return Err(RepositoryJobError::ConflictUnavailable);
        }
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
        let mut status = load_repository_sync_status(&self.app_data, &binding.repository_id)?;
        if let Some(status) = &mut status {
            status
                .conflicts
                .retain(|conflict| conflict_history_exists(&self.app_data, conflict));
        }
        Ok(status)
    }
}

pub(crate) async fn create_conflict_document(
    app_data: &Path,
    notes_root: &Path,
    conflict: &SyncConflictRecord,
    coordinator: Arc<dyn WorkingTreeCoordinator>,
) -> ConflictCopyOutcome {
    let source_relative = match validated_relative_path(&conflict.relative_path) {
        Ok(relative) => relative,
        Err(error) => return ConflictCopyOutcome::failed(error),
    };
    if source_relative
        .extension()
        .and_then(OsStr::to_str)
        .is_none_or(|extension| !extension.eq_ignore_ascii_case("md"))
    {
        return ConflictCopyOutcome::skipped();
    }
    match try_create_conflict_document(
        app_data,
        notes_root,
        conflict,
        &source_relative,
        coordinator,
    )
    .await
    {
        Ok(relative_path) => ConflictCopyOutcome::generated(relative_path),
        Err(error) => ConflictCopyOutcome::failed(error),
    }
}

async fn try_create_conflict_document(
    app_data: &Path,
    notes_root: &Path,
    conflict: &SyncConflictRecord,
    source_relative: &Path,
    coordinator: Arc<dyn WorkingTreeCoordinator>,
) -> Result<String, RepositoryJobError> {
    let occurred = OffsetDateTime::parse(&conflict.occurred_at, &Rfc3339)
        .map_err(|_| RepositoryJobError::ConflictUnavailable)?;
    let history_root = conflict_history_root(app_data, conflict)?;
    let (remote_bytes, _) = read_all_bytes(&history_root, &source_relative)?
        .ok_or(RepositoryJobError::ConflictUnavailable)?;
    let stem = source_relative
        .file_stem()
        .and_then(OsStr::to_str)
        .ok_or(RepositoryJobError::ConflictUnavailable)?;
    let name_prefix = format!(
        "{stem}-Conflicted-{:04}{:02}{:02}-{:02}{:02}{:02}",
        occurred.year(),
        u8::from(occurred.month()),
        occurred.day(),
        occurred.hour(),
        occurred.minute(),
        occurred.second()
    );
    let parent = source_relative.parent().unwrap_or_else(|| Path::new(""));
    let notes_root_directory = open_canonical_directory_nofollow(notes_root)
        .map_err(|_| RepositoryJobError::ConflictUnavailable)?;
    let notes_root_identity = directory_identity(&notes_root_directory)
        .map_err(|_| RepositoryJobError::ConflictUnavailable)?;
    let mut ordinal = 1_u32;
    loop {
        let suffix = if ordinal == 1 {
            String::new()
        } else {
            format!("-{ordinal}")
        };
        let destination = parent.join(format!("{name_prefix}{suffix}.md"));
        if file_identity_in_root(&notes_root_directory, &destination)?.is_some() {
            ordinal = next_conflict_document_ordinal(ordinal)?;
            continue;
        }

        let change = WorkingTreeChange {
            path: RepositoryRelativePath::new(
                destination
                    .to_str()
                    .ok_or(RepositoryJobError::ConflictUnavailable)?
                    .replace('\\', "/"),
            )
            .map_err(|_| RepositoryJobError::ConflictUnavailable)?,
            expected_revision: ExpectedRevision::Absent,
            action: WorkingTreeAction::Write,
        };
        let permit = coordinator
            .prepare(std::slice::from_ref(&change))
            .await
            .map_err(map_working_tree_error)?;
        let operation = (|| {
            revalidate_notes_root(notes_root, notes_root_identity)?;
            write_relative_file_no_replace(&notes_root_directory, &destination, &remote_bytes)
        })();
        coordinator.release(permit).await;
        match operation {
            Ok(true) => {
                let destination_string = destination
                    .to_str()
                    .ok_or(RepositoryJobError::ConflictUnavailable)?
                    .replace('\\', "/");
                return Ok(destination_string);
            }
            Ok(false) => ordinal = next_conflict_document_ordinal(ordinal)?,
            Err(error) => return Err(error),
        }
    }
}

fn next_conflict_document_ordinal(ordinal: u32) -> Result<u32, RepositoryJobError> {
    ordinal
        .checked_add(1)
        .filter(|next| *next <= 10_000)
        .ok_or(RepositoryJobError::WorkingTreeChanged)
}

fn revalidate_notes_root(
    notes_root: &Path,
    retained_identity: DirectoryIdentity,
) -> Result<(), RepositoryJobError> {
    let current = open_canonical_directory_nofollow(notes_root)
        .map_err(|_| RepositoryJobError::WorkingTreeChanged)?;
    let current_identity =
        directory_identity(&current).map_err(|_| RepositoryJobError::WorkingTreeChanged)?;
    if current_identity != retained_identity {
        return Err(RepositoryJobError::WorkingTreeChanged);
    }
    Ok(())
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

fn file_identity_in_root(
    root: &cap_std::fs::Dir,
    relative: &Path,
) -> Result<Option<UniqueRegularFileIdentity>, RepositoryJobError> {
    let root = root
        .try_clone()
        .map_err(|_| RepositoryJobError::ConflictUnavailable)?;
    let (directory, name) = open_relative_parent(root, relative, false)?;
    let Some(name) = name else {
        return Ok(None);
    };
    let addressed = match directory.symlink_metadata(&name) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(RepositoryJobError::ConflictUnavailable),
    };
    unique_regular_file_identity(&addressed)
        .map(Some)
        .ok_or(RepositoryJobError::ConflictUnavailable)
}

fn write_relative_file_no_replace(
    root: &cap_std::fs::Dir,
    relative: &Path,
    bytes: &[u8],
) -> Result<bool, RepositoryJobError> {
    let root = root
        .try_clone()
        .map_err(|_| RepositoryJobError::ConflictUnavailable)?;
    let (directory, name) = open_relative_parent(root, relative, true)?;
    let name = name.ok_or(RepositoryJobError::ConflictUnavailable)?;
    write_cap_file_no_replace_safer(&directory, &name, bytes, 0o600)
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
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;

    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use tempfile::tempdir;

    use super::{
        create_conflict_document, ConflictCopyStatus, ConflictResolutionKind, ConflictStore,
        SyncConflictRecord, MAX_CONFLICT_TEXT_BYTES,
    };
    use crate::dejavu_sync::local_state::{LocalSyncStateService, RepositoryBinding};
    use crate::dejavu_sync::service::{
        RepositoryJobError, RepositoryStatusSink, RepositorySyncResult, SyncJobRequest,
    };
    use crate::dejavu_sync::status::{
        RepositoryStatusEventEmitter, RepositoryStatusStore, RepositorySyncStatus,
    };
    use crate::sync_config::status::SyncTrigger;
    use qingyu_dejavu::{RepoError, WorkingTreeChange, WorkingTreeCoordinator, WorkingTreePermit};

    const REPOSITORY_ID: &str = "00000000-0000-4000-8000-0000000000c1";
    const OTHER_REPOSITORY_ID: &str = "00000000-0000-4000-8000-0000000000c2";
    const CONFLICT_ID: &str = "00000000-0000-4000-8000-0000000000c3";
    const OCCURRED_AT: &str = "2026-07-25T14:22:33Z";

    struct NoopEmitter;

    struct AllowCoordinator;

    struct CollisionOnPrepareCoordinator {
        notes_root: PathBuf,
        prepares: AtomicUsize,
        releases: AtomicUsize,
    }

    struct ReplaceRootOnPrepareCoordinator {
        notes_root: PathBuf,
        replaced_root: PathBuf,
        replaced: AtomicBool,
        releases: AtomicUsize,
    }

    impl WorkingTreeCoordinator for AllowCoordinator {
        fn prepare<'life0, 'life1, 'async_trait>(
            &'life0 self,
            _changes: &'life1 [WorkingTreeChange],
        ) -> Pin<Box<dyn Future<Output = Result<WorkingTreePermit, RepoError>> + Send + 'async_trait>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async { Ok(WorkingTreePermit::new(())) })
        }

        fn release<'life0, 'async_trait>(
            &'life0 self,
            _permit: WorkingTreePermit,
        ) -> Pin<Box<dyn Future<Output = ()> + Send + 'async_trait>>
        where
            'life0: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async {})
        }
    }

    impl WorkingTreeCoordinator for CollisionOnPrepareCoordinator {
        fn prepare<'life0, 'life1, 'async_trait>(
            &'life0 self,
            changes: &'life1 [WorkingTreeChange],
        ) -> Pin<Box<dyn Future<Output = Result<WorkingTreePermit, RepoError>> + Send + 'async_trait>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            Self: 'async_trait,
        {
            let first = self.prepares.fetch_add(1, Ordering::SeqCst) == 0;
            let notes_root = self.notes_root.clone();
            let relative_path = changes[0].path.as_str().to_owned();
            Box::pin(async move {
                if first {
                    let path = notes_root.join(relative_path);
                    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                    std::fs::write(path, b"concurrent user file").unwrap();
                }
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
            self.releases.fetch_add(1, Ordering::SeqCst);
            Box::pin(async {})
        }
    }

    impl WorkingTreeCoordinator for ReplaceRootOnPrepareCoordinator {
        fn prepare<'life0, 'life1, 'async_trait>(
            &'life0 self,
            _changes: &'life1 [WorkingTreeChange],
        ) -> Pin<Box<dyn Future<Output = Result<WorkingTreePermit, RepoError>> + Send + 'async_trait>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            Self: 'async_trait,
        {
            let replace = !self.replaced.swap(true, Ordering::SeqCst);
            let notes_root = self.notes_root.clone();
            let replaced_root = self.replaced_root.clone();
            Box::pin(async move {
                if replace {
                    std::fs::rename(&notes_root, &replaced_root).unwrap();
                    std::fs::create_dir(&notes_root).unwrap();
                }
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
            self.releases.fetch_add(1, Ordering::SeqCst);
            Box::pin(async {})
        }
    }

    impl RepositoryStatusEventEmitter for NoopEmitter {
        fn emit(&self, _status: &RepositorySyncStatus) -> Result<(), RepositoryJobError> {
            Ok(())
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
                        resolution: Some(ConflictResolutionKind::KeepLocal),
                        ..Default::default()
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
    }

    #[tokio::test]
    async fn list_and_read_return_completed_relative_metadata_and_bounded_text_versions() {
        let fixture = Fixture::new(b"remote text").await;
        let store = ConflictStore::new(&fixture.app_data);

        assert!(store.key_configured().unwrap());
        assert_eq!(store.export_key().unwrap(), STANDARD.encode([3_u8; 32]));
        let listed = store.list_history(REPOSITORY_ID).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].relative_path, "document.md");
        assert_eq!(
            listed[0].resolution,
            Some(ConflictResolutionKind::KeepLocal)
        );
        assert_eq!(
            store
                .status_for_root(&fixture.notes_root)
                .unwrap()
                .unwrap()
                .repository_id,
            REPOSITORY_ID
        );
        let versions = store
            .read_history(&fixture.notes_root, REPOSITORY_ID, CONFLICT_ID)
            .unwrap();
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

        assert!(store.list_history(REPOSITORY_ID).unwrap().is_empty());
        assert_eq!(
            store.read_history(&fixture.notes_root, REPOSITORY_ID, CONFLICT_ID),
            Err(RepositoryJobError::ConflictUnavailable)
        );
        assert_eq!(
            store.read_history(&fixture.notes_root, OTHER_REPOSITORY_ID, CONFLICT_ID),
            Err(RepositoryJobError::ConflictUnavailable)
        );
        let another_root = fixture._temporary.path().join("another-notes-root");
        std::fs::create_dir(&another_root).unwrap();
        assert_eq!(
            store.read_history(&another_root, REPOSITORY_ID, CONFLICT_ID),
            Err(RepositoryJobError::ConflictUnavailable)
        );
    }

    #[tokio::test]
    async fn oversized_and_binary_versions_report_size_without_loading_text() {
        let oversized = vec![b'x'; MAX_CONFLICT_TEXT_BYTES as usize + 1];
        let fixture = Fixture::new(&oversized).await;
        std::fs::write(fixture.notes_root.join("document.md"), [0xff, 0xfe]).unwrap();

        let versions = ConflictStore::new(&fixture.app_data)
            .read_history(&fixture.notes_root, REPOSITORY_ID, CONFLICT_ID)
            .unwrap();
        assert_eq!(versions.remote.byte_size, MAX_CONFLICT_TEXT_BYTES + 1);
        assert_eq!(versions.remote.text, None);
        assert_eq!(versions.local.unwrap().text, None);
    }

    #[tokio::test]
    async fn conflict_document_retries_a_suffix_without_overwriting_a_racing_user_file() {
        let fixture = Fixture::new(b"remote text").await;
        let coordinator = Arc::new(CollisionOnPrepareCoordinator {
            notes_root: fixture.notes_root.clone(),
            prepares: AtomicUsize::new(0),
            releases: AtomicUsize::new(0),
        });
        let conflict = ConflictStore::new(&fixture.app_data)
            .list_history(REPOSITORY_ID)
            .unwrap()
            .remove(0);

        let outcome = create_conflict_document(
            &fixture.app_data,
            &fixture.notes_root,
            &conflict,
            coordinator.clone(),
        )
        .await;
        assert_eq!(outcome.status, ConflictCopyStatus::Generated);

        let base = fixture
            .notes_root
            .join("document-Conflicted-20260725-142233.md");
        let suffixed = fixture
            .notes_root
            .join("document-Conflicted-20260725-142233-2.md");
        assert_eq!(std::fs::read(base).unwrap(), b"concurrent user file");
        assert_eq!(std::fs::read(suffixed).unwrap(), b"remote text");
        assert_eq!(coordinator.prepares.load(Ordering::SeqCst), 2);
        assert_eq!(coordinator.releases.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn conflict_document_rejects_a_notes_root_replaced_while_waiting_for_the_permit() {
        let fixture = Fixture::new(b"remote text").await;
        let replaced_root = fixture.notes_root.with_file_name("notes-replaced");
        let coordinator = Arc::new(ReplaceRootOnPrepareCoordinator {
            notes_root: fixture.notes_root.clone(),
            replaced_root: replaced_root.clone(),
            replaced: AtomicBool::new(false),
            releases: AtomicUsize::new(0),
        });
        let conflict = ConflictStore::new(&fixture.app_data)
            .list_history(REPOSITORY_ID)
            .unwrap()
            .remove(0);

        let outcome = create_conflict_document(
            &fixture.app_data,
            &fixture.notes_root,
            &conflict,
            coordinator.clone(),
        )
        .await;

        assert_eq!(outcome.status, ConflictCopyStatus::Failed);
        assert_eq!(
            outcome.error.as_deref(),
            Some(RepositoryJobError::WorkingTreeChanged.safe_code())
        );
        assert!(std::fs::read_dir(&fixture.notes_root)
            .unwrap()
            .next()
            .is_none());
        assert_eq!(
            std::fs::read(replaced_root.join("document.md")).unwrap(),
            b"local text"
        );
        assert_eq!(coordinator.releases.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn markdown_conflict_document_reports_generated_relative_path() {
        let fixture = Fixture::new(b"remote text").await;
        let conflict = ConflictStore::new(&fixture.app_data)
            .list_history(REPOSITORY_ID)
            .unwrap()
            .remove(0);

        let outcome = create_conflict_document(
            &fixture.app_data,
            &fixture.notes_root,
            &conflict,
            Arc::new(AllowCoordinator),
        )
        .await;

        assert_eq!(outcome.status, ConflictCopyStatus::Generated);
        assert_eq!(
            outcome.relative_path.as_deref(),
            Some("document-Conflicted-20260725-142233.md")
        );
        assert_eq!(outcome.error, None);
        assert_eq!(
            std::fs::read(
                fixture
                    .notes_root
                    .join("document-Conflicted-20260725-142233.md")
            )
            .unwrap(),
            b"remote text"
        );
    }

    #[tokio::test]
    async fn non_markdown_conflict_document_reports_skipped_without_writing() {
        let fixture = Fixture::new(b"remote text").await;
        let mut conflict = ConflictStore::new(&fixture.app_data)
            .list_history(REPOSITORY_ID)
            .unwrap()
            .remove(0);
        conflict.relative_path = "document.txt".to_owned();
        let history_txt = fixture.history_file.with_file_name("document.txt");
        std::fs::write(&history_txt, b"remote text").unwrap();

        let outcome = create_conflict_document(
            &fixture.app_data,
            &fixture.notes_root,
            &conflict,
            Arc::new(AllowCoordinator),
        )
        .await;

        assert_eq!(outcome.status, ConflictCopyStatus::Skipped);
        assert_eq!(outcome.relative_path, None);
        assert_eq!(outcome.error, None);
        assert!(!fixture
            .notes_root
            .join("document-Conflicted-20260725-142233.txt")
            .exists());
    }

    #[tokio::test]
    async fn conflict_record_serialization_defaults_copy_status_for_old_state() {
        let json = serde_json::json!({
            "conflictId": CONFLICT_ID,
            "repositoryId": REPOSITORY_ID,
            "relativePath": "document.md",
            "occurredAt": OCCURRED_AT,
            "resolution": "keep-local"
        });

        let record: SyncConflictRecord = serde_json::from_value(json).unwrap();

        assert_eq!(record.copy_status, ConflictCopyStatus::NotRequested);
        assert_eq!(record.copy_path, None);
        assert_eq!(record.copy_error, None);
    }
}
