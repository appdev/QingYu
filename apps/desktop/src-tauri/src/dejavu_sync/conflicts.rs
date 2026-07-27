use std::io::Read;
use std::path::{Component, Path, PathBuf};

use cap_fs_ext::DirExt;
use qingyu_dejavu::RepositoryRelativePath;
use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use super::local_state::LocalSyncStateService;
use super::service::RepositoryJobError;
use super::status::load_repository_sync_status;
use crate::storage_capability::{
    nonfollowing_read_options, open_canonical_directory_nofollow, unique_regular_file_identity,
};

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

pub(crate) struct ConflictStore {
    app_data: PathBuf,
}

impl ConflictStore {
    pub(crate) fn new(app_data: impl AsRef<Path>) -> Self {
        Self {
            app_data: app_data.as_ref().to_path_buf(),
        }
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
    use std::path::PathBuf;
    use std::sync::Arc;

    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use tempfile::tempdir;

    use super::{ConflictStore, SyncConflictRecord, MAX_CONFLICT_TEXT_BYTES};
    use crate::dejavu_sync::local_state::{LocalSyncStateService, RepositoryBinding};
    use crate::dejavu_sync::service::{
        RepositoryJobError, RepositoryStatusSink, RepositorySyncResult, SyncJobRequest,
    };
    use crate::dejavu_sync::status::{
        RepositoryStatusEventEmitter, RepositoryStatusStore, RepositorySyncStatus,
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
    }

    #[tokio::test]
    async fn list_and_read_return_only_relative_metadata_and_bounded_text_versions() {
        let fixture = Fixture::new(b"remote text").await;
        let store = ConflictStore::new(&fixture.app_data);

        let listed = store.list(REPOSITORY_ID).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].relative_path, "document.md");
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
}
