//! Durable sync-manifest repository boundary.

use std::{
    collections::BTreeMap,
    fmt,
    sync::{Mutex, OnceLock},
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::scope::RemoteSyncScope;
use crate::storage::{
    CommitState, DurableFileFailure, DurableFileFailureKind, DurableFileStore, ExpectedFile,
    FileRevision, PreservePrevious, RecoveryOutcome, ReplaceRequest, StorageFileName,
};

pub const MANIFEST_VERSION: u32 = 3;
const MAX_MANIFEST_BYTES: u64 = 8 * 1024 * 1024;
const MAX_MANIFEST_ENTRIES: usize = 100_000;
const MAX_MANIFEST_PATH_BYTES: usize = 4 * 1024;
const MAX_MANIFEST_VALUE_BYTES: usize = 8 * 1024;
static SYNC_REPOSITORY_WRITER_EPOCH: OnceLock<Uuid> = OnceLock::new();

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SyncManifestEntry {
    pub local_hash: String,
    #[serde(alias = "remote_etag")]
    pub remote_identity: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SyncManifest {
    #[serde(default)]
    pub entries: BTreeMap<String, SyncManifestEntry>,
    #[serde(default)]
    pub target_fingerprint: String,
    #[serde(default)]
    pub local_identity: String,
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub full_scan_completed: bool,
    #[serde(default)]
    pub restore_generation: Option<String>,
    #[serde(default)]
    pub restore_generation_completed: bool,
    #[serde(default)]
    pub restore_local_only_paths: BTreeMap<String, String>,
}

impl Default for SyncManifest {
    fn default() -> Self {
        Self {
            entries: BTreeMap::new(),
            target_fingerprint: String::new(),
            local_identity: String::new(),
            version: MANIFEST_VERSION,
            full_scan_completed: false,
            restore_generation: None,
            restore_generation_completed: false,
            restore_local_only_paths: BTreeMap::new(),
        }
    }
}

pub struct SyncManifestRepository<'scope> {
    scope: &'scope RemoteSyncScope,
    store: DurableFileStore,
    manifest_name: StorageFileName,
    local_identity: String,
    current_revision: Mutex<Option<FileRevision>>,
}

impl<'scope> SyncManifestRepository<'scope> {
    pub fn open(scope: &'scope RemoteSyncScope) -> Result<Self, SyncManifestRepositoryError> {
        let directory = scope.open_state_root().map_err(|_| {
            SyncManifestRepositoryError::new(SyncManifestRepositoryErrorKind::Unsafe)
        })?;
        let manifest_name = StorageFileName::parse(scope.manifest_name()).map_err(|_| {
            SyncManifestRepositoryError::new(SyncManifestRepositoryErrorKind::Unsafe)
        })?;
        let store = DurableFileStore::at_retained_directory(
            directory,
            scope.state_root().to_path_buf(),
            *SYNC_REPOSITORY_WRITER_EPOCH.get_or_init(Uuid::new_v4),
        );
        let recovery = store.recover().map_err(SyncManifestRepositoryError::from)?;
        if recovery
            .iter()
            .any(|outcome| matches!(outcome, RecoveryOutcome::ManualInterventionRequired { .. }))
        {
            return Err(SyncManifestRepositoryError::new(
                SyncManifestRepositoryErrorKind::RecoveryRequired,
            ));
        }
        Ok(Self {
            scope,
            store,
            manifest_name,
            local_identity: scope.local_identity().unwrap_or_default().to_string(),
            current_revision: Mutex::new(None),
        })
    }

    pub fn load(
        &self,
        target_fingerprint: &str,
    ) -> Result<(SyncManifest, bool), SyncManifestRepositoryError> {
        self.verify_state_address()?;
        let mut current_revision = self.current_revision.lock().map_err(|_| {
            SyncManifestRepositoryError::new(SyncManifestRepositoryErrorKind::Unavailable)
        })?;
        let stored = self
            .store
            .read(&self.manifest_name, MAX_MANIFEST_BYTES)
            .map_err(SyncManifestRepositoryError::from)?;
        let manifest_exists = stored.is_some();
        let mut manifest = match stored {
            Some(stored) => {
                *current_revision = Some(stored.revision.clone());
                let manifest = serde_json::from_slice(&stored.bytes).map_err(|_| {
                    SyncManifestRepositoryError::new(SyncManifestRepositoryErrorKind::Malformed)
                })?;
                validate_manifest(&manifest)?;
                manifest
            }
            None => {
                *current_revision = None;
                SyncManifest::default()
            }
        };

        let version_matches = manifest.version == MANIFEST_VERSION;
        let target_matches = manifest.target_fingerprint == target_fingerprint;
        let local_identity_matches = manifest.local_identity == self.local_identity;
        let has_effective_baseline = manifest_exists
            && version_matches
            && target_matches
            && local_identity_matches
            && manifest.full_scan_completed;

        if !version_matches {
            manifest.entries.clear();
        }
        if manifest.target_fingerprint.is_empty() {
            manifest.target_fingerprint = target_fingerprint.to_string();
        } else if !target_matches {
            manifest.entries.clear();
            manifest.target_fingerprint = target_fingerprint.to_string();
        }
        if manifest.local_identity.is_empty() {
            manifest.local_identity.clone_from(&self.local_identity);
        } else if !local_identity_matches {
            manifest.entries.clear();
            manifest.local_identity.clone_from(&self.local_identity);
        }
        if !manifest_exists || !version_matches || !target_matches || !local_identity_matches {
            manifest.full_scan_completed = false;
        }
        manifest.version = MANIFEST_VERSION;
        Ok((manifest, has_effective_baseline))
    }

    pub fn load_current(&self) -> Result<Option<SyncManifest>, SyncManifestRepositoryError> {
        self.verify_state_address()?;
        let mut current_revision = self.current_revision.lock().map_err(|_| {
            SyncManifestRepositoryError::new(SyncManifestRepositoryErrorKind::Unavailable)
        })?;
        let stored = self
            .store
            .read(&self.manifest_name, MAX_MANIFEST_BYTES)
            .map_err(SyncManifestRepositoryError::from)?;
        match stored {
            Some(stored) => {
                *current_revision = Some(stored.revision.clone());
                let manifest = serde_json::from_slice(&stored.bytes).map_err(|_| {
                    SyncManifestRepositoryError::new(SyncManifestRepositoryErrorKind::Malformed)
                })?;
                validate_manifest(&manifest)?;
                Ok(Some(manifest))
            }
            None => {
                *current_revision = None;
                Ok(None)
            }
        }
    }

    pub fn save(&self, manifest: &SyncManifest) -> Result<(), SyncManifestRepositoryError> {
        self.verify_state_address()?;
        validate_manifest(manifest)?;
        let bytes = serde_json::to_vec_pretty(manifest).map_err(|_| {
            SyncManifestRepositoryError::new(SyncManifestRepositoryErrorKind::Malformed)
        })?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_MANIFEST_BYTES {
            return Err(SyncManifestRepositoryError::new(
                SyncManifestRepositoryErrorKind::TooLarge,
            ));
        }
        let mut current_revision = self.current_revision.lock().map_err(|_| {
            SyncManifestRepositoryError::new(SyncManifestRepositoryErrorKind::Unavailable)
        })?;
        let expected = match current_revision.as_ref() {
            Some(revision) => ExpectedFile::Revision(revision),
            None => ExpectedFile::Absent,
        };
        let outcome = self
            .store
            .replace(ReplaceRequest {
                target: &self.manifest_name,
                bytes: &bytes,
                expected,
                preserve_previous: PreservePrevious::None,
            })
            .map_err(SyncManifestRepositoryError::from)?;
        *current_revision = Some(outcome.installed_revision);
        if outcome.commit_state == CommitState::PublishedDurabilityUncertain {
            return Err(SyncManifestRepositoryError::new(
                SyncManifestRepositoryErrorKind::DurabilityUncertain,
            ));
        }
        Ok(())
    }

    fn verify_state_address(&self) -> Result<(), SyncManifestRepositoryError> {
        self.scope
            .open_state_root()
            .map(drop)
            .map_err(|_| SyncManifestRepositoryError::new(SyncManifestRepositoryErrorKind::Unsafe))
    }
}

impl fmt::Debug for SyncManifestRepository<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SyncManifestRepository(..)")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncManifestRepositoryErrorKind {
    Unsafe,
    TooLarge,
    Malformed,
    Conflict,
    DurabilityUncertain,
    RecoveryRequired,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyncManifestRepositoryError {
    kind: SyncManifestRepositoryErrorKind,
}

impl SyncManifestRepositoryError {
    const fn new(kind: SyncManifestRepositoryErrorKind) -> Self {
        Self { kind }
    }

    pub const fn kind(self) -> SyncManifestRepositoryErrorKind {
        self.kind
    }

    pub const fn safe_code(self) -> &'static str {
        match self.kind {
            SyncManifestRepositoryErrorKind::Unsafe => "sync-manifest-unsafe",
            SyncManifestRepositoryErrorKind::TooLarge => "sync-manifest-too-large",
            SyncManifestRepositoryErrorKind::Malformed => "sync-manifest-invalid",
            SyncManifestRepositoryErrorKind::Conflict => "sync-manifest-changed",
            SyncManifestRepositoryErrorKind::DurabilityUncertain => {
                "sync-manifest-durability-uncertain"
            }
            SyncManifestRepositoryErrorKind::RecoveryRequired => "sync-manifest-recovery-required",
            SyncManifestRepositoryErrorKind::Unavailable => "sync-manifest-unavailable",
        }
    }
}

impl From<DurableFileFailure> for SyncManifestRepositoryError {
    fn from(error: DurableFileFailure) -> Self {
        let kind = match error.kind() {
            DurableFileFailureKind::InvalidName | DurableFileFailureKind::UnsafeEntry => {
                SyncManifestRepositoryErrorKind::Unsafe
            }
            DurableFileFailureKind::TooLarge => SyncManifestRepositoryErrorKind::TooLarge,
            DurableFileFailureKind::RevisionConflict => SyncManifestRepositoryErrorKind::Conflict,
            DurableFileFailureKind::PublishStateUncertain
            | DurableFileFailureKind::RecoveryRequired => {
                SyncManifestRepositoryErrorKind::RecoveryRequired
            }
            DurableFileFailureKind::NotPublished | DurableFileFailureKind::Unavailable => {
                SyncManifestRepositoryErrorKind::Unavailable
            }
        };
        Self::new(kind)
    }
}

impl fmt::Display for SyncManifestRepositoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.safe_code())
    }
}

impl std::error::Error for SyncManifestRepositoryError {}

fn validate_manifest(manifest: &SyncManifest) -> Result<(), SyncManifestRepositoryError> {
    let scalar_values = [
        manifest.target_fingerprint.as_str(),
        manifest.local_identity.as_str(),
        manifest.restore_generation.as_deref().unwrap_or_default(),
    ];
    let values_are_bounded = scalar_values
        .into_iter()
        .all(|value| value.len() <= MAX_MANIFEST_VALUE_BYTES)
        && manifest.entries.len() <= MAX_MANIFEST_ENTRIES
        && manifest.restore_local_only_paths.len() <= MAX_MANIFEST_ENTRIES
        && manifest.entries.iter().all(|(path, entry)| {
            path.len() <= MAX_MANIFEST_PATH_BYTES
                && entry.local_hash.len() <= MAX_MANIFEST_VALUE_BYTES
                && entry.remote_identity.len() <= MAX_MANIFEST_VALUE_BYTES
        })
        && manifest
            .restore_local_only_paths
            .iter()
            .all(|(path, hash)| {
                path.len() <= MAX_MANIFEST_PATH_BYTES && hash.len() <= MAX_MANIFEST_VALUE_BYTES
            });
    if values_are_bounded {
        Ok(())
    } else {
        Err(SyncManifestRepositoryError::new(
            SyncManifestRepositoryErrorKind::Malformed,
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use tempfile::TempDir;

    use super::{SyncManifestEntry, SyncManifestRepository, MANIFEST_VERSION};
    use crate::sync::scope::RemoteSyncScope;

    fn temporary_root() -> TempDir {
        tempfile::Builder::new()
            .prefix("qingyu-kernel-sync-repository-")
            .tempdir_in(std::env::current_dir().expect("current directory"))
            .expect("temporary root")
    }

    fn notes_scope(
        source: &std::path::Path,
        state: &std::path::Path,
        local_identity: &str,
    ) -> RemoteSyncScope {
        RemoteSyncScope::notes(
            source,
            state,
            "manifest-v3.json",
            Some(local_identity.to_string()),
            None,
        )
        .expect("notes scope")
    }

    #[test]
    fn manifest_round_trips_and_only_complete_matching_state_is_a_baseline() {
        let temporary = temporary_root();
        let source = temporary.path().join("notes");
        let state = temporary.path().join("state");
        std::fs::create_dir(&source).expect("source root");
        let scope = notes_scope(&source, &state, "workspace-a");
        let repository = SyncManifestRepository::open(&scope).expect("repository");

        let (mut manifest, baseline) = repository.load("remote-a").expect("initial load");
        assert!(!baseline);
        assert_eq!(manifest.version, MANIFEST_VERSION);
        assert_eq!(manifest.target_fingerprint, "remote-a");
        assert_eq!(manifest.local_identity, "workspace-a");

        manifest.entries = BTreeMap::from([(
            "draft.md".to_string(),
            SyncManifestEntry {
                local_hash: "local-hash".to_string(),
                remote_identity: "remote-version".to_string(),
            },
        )]);
        manifest.full_scan_completed = true;
        repository.save(&manifest).expect("save manifest");
        drop(repository);

        let repository = SyncManifestRepository::open(&scope).expect("reopen repository");
        let (loaded, baseline) = repository.load("remote-a").expect("reload manifest");
        assert!(baseline);
        assert_eq!(loaded, manifest);
    }

    #[test]
    fn target_or_local_identity_change_invalidates_manifest_entries() {
        let temporary = temporary_root();
        let source = temporary.path().join("notes");
        let state = temporary.path().join("state");
        std::fs::create_dir(&source).expect("source root");
        let first_scope = notes_scope(&source, &state, "workspace-a");
        let repository = SyncManifestRepository::open(&first_scope).expect("repository");
        let (mut manifest, _) = repository.load("remote-a").expect("initial load");
        manifest.entries.insert(
            "draft.md".to_string(),
            SyncManifestEntry {
                local_hash: "local-hash".to_string(),
                remote_identity: "remote-version".to_string(),
            },
        );
        manifest.full_scan_completed = true;
        repository.save(&manifest).expect("save manifest");
        drop(repository);

        let changed_remote = SyncManifestRepository::open(&first_scope).expect("repository");
        let (manifest, baseline) = changed_remote.load("remote-b").expect("changed remote");
        assert!(!baseline);
        assert!(manifest.entries.is_empty());
        assert_eq!(manifest.target_fingerprint, "remote-b");
        drop(changed_remote);

        let second_scope = notes_scope(&source, &state, "workspace-b");
        let changed_local = SyncManifestRepository::open(&second_scope).expect("repository");
        let (manifest, baseline) = changed_local.load("remote-a").expect("changed local");
        assert!(!baseline);
        assert!(manifest.entries.is_empty());
        assert_eq!(manifest.local_identity, "workspace-b");
    }

    #[test]
    fn repositories_with_the_same_observed_revision_do_not_overwrite_each_other() {
        let temporary = temporary_root();
        let source = temporary.path().join("notes");
        let state = temporary.path().join("state");
        std::fs::create_dir(&source).expect("source root");
        let scope = notes_scope(&source, &state, "workspace-a");
        let first = SyncManifestRepository::open(&scope).expect("first repository");
        let second = SyncManifestRepository::open(&scope).expect("second repository");
        let (mut first_manifest, _) = first.load("remote-a").expect("first load");
        let (mut second_manifest, _) = second.load("remote-a").expect("second load");

        first_manifest.full_scan_completed = true;
        first.save(&first_manifest).expect("first save");
        second_manifest.full_scan_completed = true;
        let error = second
            .save(&second_manifest)
            .expect_err("stale repository must not overwrite the manifest");

        assert_eq!(error.safe_code(), "sync-manifest-changed");
    }

    #[cfg(unix)]
    #[test]
    fn manifest_symlink_is_rejected() {
        use std::os::unix::fs::symlink;

        let temporary = temporary_root();
        let source = temporary.path().join("notes");
        let state = temporary.path().join("state");
        std::fs::create_dir(&source).expect("source root");
        let scope = notes_scope(&source, &state, "workspace-a");
        let outside = temporary.path().join("outside.json");
        std::fs::write(&outside, b"{}").expect("outside file");
        symlink(&outside, state.join("manifest-v3.json")).expect("manifest symlink");

        let repository = SyncManifestRepository::open(&scope).expect("repository");
        let error = repository
            .load("remote-a")
            .expect_err("symlink must be rejected");
        assert_eq!(error.safe_code(), "sync-manifest-unsafe");
    }

    #[test]
    fn manifest_hard_link_is_rejected() {
        let temporary = temporary_root();
        let source = temporary.path().join("notes");
        let state = temporary.path().join("state");
        std::fs::create_dir(&source).expect("source root");
        let scope = notes_scope(&source, &state, "workspace-a");
        let outside = temporary.path().join("outside.json");
        std::fs::write(&outside, b"{}").expect("outside file");
        std::fs::hard_link(&outside, state.join("manifest-v3.json")).expect("manifest hard link");

        let repository = SyncManifestRepository::open(&scope).expect("repository");
        let error = repository
            .load("remote-a")
            .expect_err("hard link must be rejected");
        assert_eq!(error.safe_code(), "sync-manifest-unsafe");
    }

    #[test]
    fn legacy_remote_etag_and_unknown_fields_remain_read_compatible() {
        let temporary = temporary_root();
        let source = temporary.path().join("notes");
        let state = temporary.path().join("state");
        std::fs::create_dir(&source).expect("source root");
        let scope = notes_scope(&source, &state, "workspace-a");
        std::fs::write(
            state.join("manifest-v3.json"),
            br#"{
                "version": 3,
                "target_fingerprint": "remote-a",
                "local_identity": "workspace-a",
                "full_scan_completed": true,
                "future_extension": true,
                "entries": {
                    "draft.md": {
                        "local_hash": "local-hash",
                        "remote_etag": "legacy-etag"
                    }
                }
            }"#,
        )
        .expect("legacy manifest");

        let repository = SyncManifestRepository::open(&scope).expect("repository");
        let (manifest, baseline) = repository.load("remote-a").expect("legacy load");
        assert!(baseline);
        assert_eq!(manifest.entries["draft.md"].remote_identity, "legacy-etag");
    }

    #[cfg(unix)]
    #[test]
    fn replacing_state_root_after_admission_fails_closed() {
        let temporary = temporary_root();
        let source = temporary.path().join("notes");
        let state = temporary.path().join("state");
        let replacement = temporary.path().join("replacement");
        std::fs::create_dir(&source).expect("source root");
        std::fs::create_dir(&replacement).expect("replacement root");
        let scope = notes_scope(&source, &state, "workspace-a");
        let repository = SyncManifestRepository::open(&scope).expect("repository");
        std::fs::rename(&state, temporary.path().join("retained-state"))
            .expect("move admitted state");
        std::fs::rename(&replacement, &state).expect("replace state address");

        let error = repository
            .load("remote-a")
            .expect_err("replaced state address must be rejected");
        assert_eq!(error.safe_code(), "sync-manifest-unsafe");
        assert!(!state.join("manifest-v3.json").exists());
    }
}
