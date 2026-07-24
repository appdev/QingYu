use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use cap_fs_ext::{DirExt, FollowSymlinks, MetadataExt, OpenOptionsExt, OpenOptionsFollowExt};
use cap_std::fs::Dir;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

use super::backend::{RemoteSyncBackend, RemoteSyncError, RemoteSyncFile};
use super::scope::RemoteSyncScope;
use crate::storage_capability::{
    sync_directory, unique_regular_file_identity, UniqueRegularFileIdentity,
};
pub(super) const MANIFEST_VERSION: u32 = 3;
static REMOTE_SYNC_EXECUTION_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static SYNC_MUTATION_STAGING_SEQUENCE: AtomicUsize = AtomicUsize::new(0);
pub(super) const MAX_IMMEDIATE_RECHECK_PASSES: usize = 3;
const STAGED_SYNC_REPLACEMENT_NAME: &str = "replacement";

#[cfg(test)]
pub(crate) type AtomicReplaceTestHook =
    Box<dyn Fn(&Path) -> Result<(), String> + Send + Sync + 'static>;

#[cfg(test)]
pub(crate) type UploadValidatedTestHook =
    Box<dyn Fn(&Path) -> Result<(), String> + Send + Sync + 'static>;

#[cfg(test)]
pub(crate) type FinalMutationTestHook =
    Box<dyn Fn(&Path) -> Result<(), String> + Send + Sync + 'static>;

#[cfg(test)]
pub(crate) type QuarantineRestoreTestHook =
    Box<dyn Fn() -> Result<(), String> + Send + Sync + 'static>;

#[cfg(test)]
pub(crate) type StateStagedTestHook =
    Box<dyn Fn(&Path, &Path) -> Result<(), String> + Send + Sync + 'static>;

#[cfg(test)]
pub(crate) type SnapshotEntryTestHook =
    Box<dyn Fn(&Path) -> Result<(), String> + Send + Sync + 'static>;

#[derive(Default)]
pub(crate) struct RemoteSyncExecutionHooks {
    #[cfg(test)]
    atomic_replace: Option<AtomicReplaceTestHook>,
    #[cfg(test)]
    upload_validated: Option<UploadValidatedTestHook>,
    #[cfg(test)]
    final_replace: Option<FinalMutationTestHook>,
    #[cfg(test)]
    final_delete: Option<FinalMutationTestHook>,
    #[cfg(test)]
    quarantine_restore: Option<QuarantineRestoreTestHook>,
    #[cfg(test)]
    state_staged: Option<StateStagedTestHook>,
    #[cfg(test)]
    snapshot_entry: Option<SnapshotEntryTestHook>,
}

impl RemoteSyncExecutionHooks {
    #[cfg(test)]
    pub(crate) fn with_final_replace(hook: FinalMutationTestHook) -> Self {
        Self {
            final_replace: Some(hook),
            ..Default::default()
        }
    }

    fn run_atomic_replace(&self, path: &Path) -> Result<(), String> {
        let _ = path;
        #[cfg(test)]
        if let Some(hook) = self.atomic_replace.as_ref() {
            hook(path)?;
        }
        Ok(())
    }

    fn run_upload_validated(&self, path: &Path) -> Result<(), String> {
        let _ = path;
        #[cfg(test)]
        if let Some(hook) = self.upload_validated.as_ref() {
            hook(path)?;
        }
        Ok(())
    }

    fn run_final_replace(&self, path: &Path) -> Result<(), String> {
        let _ = path;
        #[cfg(test)]
        if let Some(hook) = self.final_replace.as_ref() {
            hook(path)?;
        }
        Ok(())
    }

    fn run_final_delete(&self, path: &Path) -> Result<(), String> {
        let _ = path;
        #[cfg(test)]
        if let Some(hook) = self.final_delete.as_ref() {
            hook(path)?;
        }
        Ok(())
    }

    fn run_quarantine_restore(&self) -> Result<(), String> {
        #[cfg(test)]
        if let Some(hook) = self.quarantine_restore.as_ref() {
            hook()?;
        }
        Ok(())
    }

    fn run_state_staged(&self, staged: &Path, target: &Path) -> Result<(), String> {
        let _ = (staged, target);
        #[cfg(test)]
        if let Some(hook) = self.state_staged.as_ref() {
            hook(staged, target)?;
        }
        Ok(())
    }

    fn run_snapshot_entry(&self, path: &Path) -> Result<(), String> {
        let _ = path;
        #[cfg(test)]
        if let Some(hook) = self.snapshot_entry.as_ref() {
            hook(path)?;
        }
        Ok(())
    }
}

#[derive(Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoteSyncSummary {
    pub(crate) bytes_downloaded: u64,
    pub(crate) bytes_uploaded: u64,
    pub(crate) conflict_files: u64,
    pub(crate) downloaded_files: u64,
    pub(crate) scanned_files: u64,
    pub(crate) skipped_files: u64,
    pub(crate) uploaded_files: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SettingsSyncOutcome {
    pub(crate) summary: RemoteSyncSummary,
    pub(crate) expected_local_hash: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
struct SyncManifestEntry {
    local_hash: String,
    #[serde(alias = "remote_etag")]
    remote_identity: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct SyncManifest {
    #[serde(default)]
    entries: BTreeMap<String, SyncManifestEntry>,
    #[serde(default)]
    target_fingerprint: String,
    #[serde(default)]
    local_identity: String,
    #[serde(default)]
    version: u32,
    #[serde(default)]
    full_scan_completed: bool,
    #[serde(default)]
    restore_generation: Option<String>,
    #[serde(default)]
    restore_generation_completed: bool,
    #[serde(default)]
    restore_local_only_paths: BTreeMap<String, String>,
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

#[derive(Debug)]
struct LocalSyncFile {
    hash: String,
    identity: FileIdentity,
    path: PathBuf,
    size: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileIdentity {
    device: u64,
    inode: u64,
}

fn file_identity<T: MetadataExt>(metadata: &T) -> FileIdentity {
    FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FileSyncAction {
    Conflict,
    DeleteLocal,
    DeleteRemote,
    Download,
    Skip,
    Upload,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RemoteSyncPhase {
    RemoteHydration,
    LocalPublication,
}

fn ordered_first_sync_actions(
    planned: BTreeMap<String, FileSyncAction>,
) -> Vec<(RemoteSyncPhase, String, FileSyncAction)> {
    let mut actions = planned
        .into_iter()
        .map(|(path, action)| {
            let phase = match action {
                FileSyncAction::Conflict | FileSyncAction::Download => {
                    RemoteSyncPhase::RemoteHydration
                }
                FileSyncAction::DeleteLocal
                | FileSyncAction::DeleteRemote
                | FileSyncAction::Skip
                | FileSyncAction::Upload => RemoteSyncPhase::LocalPublication,
            };
            (phase, path, action)
        })
        .collect::<Vec<_>>();
    actions.sort_by(|left, right| {
        first_sync_action_rank(left.2)
            .cmp(&first_sync_action_rank(right.2))
            .then_with(|| left.1.cmp(&right.1))
    });
    actions
}

fn first_sync_action_rank(action: FileSyncAction) -> u8 {
    match action {
        FileSyncAction::Conflict | FileSyncAction::Download => 0,
        FileSyncAction::Skip => 1,
        FileSyncAction::Upload => 2,
        FileSyncAction::DeleteLocal | FileSyncAction::DeleteRemote => 3,
    }
}

#[cfg(test)]
pub(crate) async fn execute_remote_sync<B: RemoteSyncBackend>(
    scope: &RemoteSyncScope,
    backend: &B,
) -> Result<RemoteSyncSummary, RemoteSyncError> {
    let _execution_guard = REMOTE_SYNC_EXECUTION_LOCK.lock().await;
    let hooks = RemoteSyncExecutionHooks::default();
    execute_remote_sync_locked(scope, backend, &hooks).await
}

#[cfg(test)]
pub(crate) async fn execute_remote_sync_pair<NotesBackend, SettingsBackend, Reconcile>(
    notes_scope: &RemoteSyncScope,
    notes_backend: &NotesBackend,
    settings_scope: &RemoteSyncScope,
    settings_backend: &SettingsBackend,
    reconcile: Reconcile,
) -> (
    Result<RemoteSyncSummary, RemoteSyncError>,
    Result<SettingsSyncOutcome, RemoteSyncError>,
)
where
    NotesBackend: RemoteSyncBackend,
    SettingsBackend: RemoteSyncBackend,
    Reconcile: FnOnce(Option<&str>) -> Result<(), String>,
{
    with_remote_sync_execution_lock(|| async {
        execute_remote_sync_pair_locked(
            notes_scope,
            notes_backend,
            settings_scope,
            settings_backend,
            reconcile,
        )
        .await
    })
    .await
}

pub(crate) async fn with_remote_sync_execution_lock<Operation, OperationFuture, Output>(
    operation: Operation,
) -> Output
where
    Operation: FnOnce() -> OperationFuture,
    OperationFuture: Future<Output = Output>,
{
    let _execution_guard = REMOTE_SYNC_EXECUTION_LOCK.lock().await;
    operation().await
}

pub(crate) async fn execute_remote_sync_pair_locked<NotesBackend, SettingsBackend, Reconcile>(
    notes_scope: &RemoteSyncScope,
    notes_backend: &NotesBackend,
    settings_scope: &RemoteSyncScope,
    settings_backend: &SettingsBackend,
    reconcile: Reconcile,
) -> (
    Result<RemoteSyncSummary, RemoteSyncError>,
    Result<SettingsSyncOutcome, RemoteSyncError>,
)
where
    NotesBackend: RemoteSyncBackend,
    SettingsBackend: RemoteSyncBackend,
    Reconcile: FnOnce(Option<&str>) -> Result<(), String>,
{
    let hooks = RemoteSyncExecutionHooks::default();
    let notes = execute_remote_sync_locked(notes_scope, notes_backend, &hooks).await;
    let mut settings =
        match execute_remote_sync_locked(settings_scope, settings_backend, &hooks).await {
            Ok(summary) => {
                let target_fingerprint =
                    sha256_hex(settings_backend.target_fingerprint_source().as_bytes());
                load_sync_manifest(settings_scope, &target_fingerprint)
                    .map(|(manifest, _)| SettingsSyncOutcome {
                        summary,
                        expected_local_hash: manifest
                            .entries
                            .get("settings.json")
                            .map(|entry| entry.local_hash.clone()),
                    })
                    .map_err(RemoteSyncError::from)
            }
            Err(error) => Err(error),
        };
    if let Ok(outcome) = &settings {
        if let Err(error) = reconcile(outcome.expected_local_hash.as_deref()) {
            settings = Err(error.into());
        }
    }
    (notes, settings)
}

#[cfg(test)]
pub(crate) async fn execute_remote_sync_with_hooks<B: RemoteSyncBackend>(
    scope: &RemoteSyncScope,
    backend: &B,
    hooks: RemoteSyncExecutionHooks,
) -> Result<RemoteSyncSummary, RemoteSyncError> {
    let _execution_guard = REMOTE_SYNC_EXECUTION_LOCK.lock().await;
    execute_remote_sync_locked(scope, backend, &hooks).await
}

async fn execute_remote_sync_locked<B: RemoteSyncBackend>(
    scope: &RemoteSyncScope,
    backend: &B,
    hooks: &RemoteSyncExecutionHooks,
) -> Result<RemoteSyncSummary, RemoteSyncError> {
    let _state_root = scope.open_state_root()?;
    cleanup_stale_state_staging(scope)?;
    let target_fingerprint = sha256_hex(backend.target_fingerprint_source().as_bytes());
    let (mut manifest, mut has_effective_baseline) =
        load_sync_manifest(scope, &target_fingerprint)?;
    if prepare_remote_first_restore(scope, &mut manifest)? {
        has_effective_baseline = false;
    }
    let mut summary = RemoteSyncSummary::default();
    for pass_index in 0..MAX_IMMEDIATE_RECHECK_PASSES {
        let local_files = match collect_local_sync_files(scope, hooks) {
            Ok(files) => files,
            Err(error)
                if is_concurrent_local_change(&error)
                    && pass_index + 1 < MAX_IMMEDIATE_RECHECK_PASSES =>
            {
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        let mut expected_local_hashes = local_hashes(&local_files);
        let mut remote_files = backend.list_files().await?;
        validate_remote_files(&remote_files)?;
        remote_files.retain(|path, _| scope.includes_relative_path(path, false));
        let timestamp = sync_timestamp();
        let paths = local_files
            .keys()
            .chain(remote_files.keys())
            .cloned()
            .collect::<BTreeSet<_>>();
        let incomplete_notes_bootstrap =
            !has_effective_baseline && !scope.remote_wins_without_baseline();
        let mut planned = BTreeMap::new();
        for relative_path in paths {
            validate_relative_path(&relative_path)?;
            let local_file = local_files.get(&relative_path);
            let remote_file = remote_files.get(&relative_path);
            let suppress_unfinished_restore_copy = scope.remote_first_restore()
                && !manifest.restore_generation_completed
                && remote_file.is_none()
                && local_file.is_some_and(|local| {
                    manifest.restore_local_only_paths.get(&relative_path) == Some(&local.hash)
                });
            let action = if suppress_unfinished_restore_copy {
                FileSyncAction::Skip
            } else if !has_effective_baseline {
                if scope.remote_wins_without_baseline()
                    && local_file.is_some()
                    && remote_file.is_some()
                {
                    FileSyncAction::Download
                } else {
                    plan_incomplete_sync(
                        local_file.map(|file| file.hash.as_str()),
                        remote_file.map(|file| file.identity.as_str()),
                        manifest.entries.get(&relative_path),
                    )
                }
            } else {
                plan_file_sync(
                    local_file.map(|file| file.hash.as_str()),
                    remote_file.map(|file| file.identity.as_str()),
                    manifest.entries.get(&relative_path),
                )
            };
            planned.insert(relative_path, action);
        }
        summary.scanned_files = summary.scanned_files.max(planned.len() as u64);

        let actions = if incomplete_notes_bootstrap {
            ordered_first_sync_actions(planned)
        } else {
            planned
                .into_iter()
                .map(|(path, action)| (RemoteSyncPhase::LocalPublication, path, action))
                .collect()
        };
        manifest.full_scan_completed = false;
        let mut recheck_required = false;
        let mut recheck_error: Option<RemoteSyncError> = None;

        for (_phase, relative_path, action) in actions {
            let local_file = local_files.get(&relative_path);
            let remote_file = remote_files.get(&relative_path);

            match action {
                FileSyncAction::Upload => {
                    let local = required_local(local_file, "upload", &relative_path)?;
                    let bytes = match read_local_upload_bytes(
                        scope,
                        &relative_path,
                        &local.hash,
                        local.identity,
                        local.size,
                        hooks,
                    ) {
                        Ok(bytes) => bytes,
                        Err(error) if is_concurrent_local_change(&error) => {
                            recheck_required = true;
                            recheck_error = Some(error.into());
                            continue;
                        }
                        Err(error) => return Err(error.into()),
                    };
                    scope.validate_upload(&bytes)?;
                    let remote_identity = match backend
                        .upload(
                            &relative_path,
                            &bytes,
                            remote_file.map(|file| file.identity.as_str()),
                        )
                        .await
                    {
                        Ok(identity) => identity,
                        Err(error) if is_stale_remote_plan(&error) => {
                            recheck_required = true;
                            recheck_error = Some(error);
                            continue;
                        }
                        Err(error) => return Err(error),
                    };
                    summary.uploaded_files += 1;
                    summary.bytes_uploaded += local.size;
                    manifest.entries.insert(
                        relative_path,
                        SyncManifestEntry {
                            local_hash: local.hash.clone(),
                            remote_identity,
                        },
                    );
                    save_sync_manifest(scope, &manifest)?;
                }
                FileSyncAction::Download => {
                    let remote = required_remote(remote_file, "download", &relative_path)?;
                    let bytes = match backend.download(&relative_path, &remote.identity).await {
                        Ok(bytes) => bytes,
                        Err(error) if is_stale_remote_plan(&error) => {
                            recheck_required = true;
                            recheck_error = Some(error);
                            continue;
                        }
                        Err(error) => return Err(error),
                    };
                    validate_remote_download_or_quarantine(
                        scope,
                        &relative_path,
                        &bytes,
                        &timestamp,
                    )?;
                    let hash = match write_download_atomically(
                        scope,
                        &relative_path,
                        &bytes,
                        local_file.map(|file| (file.hash.as_str(), file.identity)),
                        &relative_path,
                        hooks,
                    ) {
                        Ok(hash) => hash,
                        Err(error) if is_concurrent_local_change(&error) => {
                            recheck_required = true;
                            recheck_error = Some(error.into());
                            continue;
                        }
                        Err(error) => return Err(error.into()),
                    };
                    summary.downloaded_files += 1;
                    summary.bytes_downloaded += remote.size;
                    expected_local_hashes.insert(relative_path.clone(), hash.clone());
                    manifest.entries.insert(
                        relative_path,
                        SyncManifestEntry {
                            local_hash: hash,
                            remote_identity: remote.identity.clone(),
                        },
                    );
                    save_sync_manifest(scope, &manifest)?;
                }
                FileSyncAction::DeleteLocal => {
                    let local = required_local(local_file, "delete", &relative_path)?;
                    match delete_local_file(
                        scope,
                        &relative_path,
                        &local.hash,
                        local.identity,
                        hooks,
                    ) {
                        Ok(()) => {}
                        Err(error) if is_concurrent_local_change(&error) => {
                            recheck_required = true;
                            recheck_error = Some(error.into());
                            continue;
                        }
                        Err(error) => return Err(error.into()),
                    }
                    expected_local_hashes.remove(&relative_path);
                    manifest.entries.remove(&relative_path);
                    save_sync_manifest(scope, &manifest)?;
                }
                FileSyncAction::DeleteRemote => {
                    let remote = required_remote(remote_file, "delete", &relative_path)?;
                    match backend.delete(&relative_path, &remote.identity).await {
                        Ok(()) => {}
                        Err(error) if is_stale_remote_plan(&error) => {
                            recheck_required = true;
                            recheck_error = Some(error);
                            continue;
                        }
                        Err(error) => return Err(error),
                    }
                    manifest.entries.remove(&relative_path);
                    save_sync_manifest(scope, &manifest)?;
                }
                FileSyncAction::Skip => {
                    if let (Some(local), Some(remote)) = (local_file, remote_file) {
                        summary.skipped_files += 1;
                        manifest.entries.insert(
                            relative_path,
                            SyncManifestEntry {
                                local_hash: local.hash.clone(),
                                remote_identity: remote.identity.clone(),
                            },
                        );
                        save_sync_manifest(scope, &manifest)?;
                    }
                }
                FileSyncAction::Conflict => {
                    let local = required_local(local_file, "conflict", &relative_path)?;
                    let remote = required_remote(remote_file, "conflict", &relative_path)?;
                    let bytes = match backend.download(&relative_path, &remote.identity).await {
                        Ok(bytes) => bytes,
                        Err(error) if is_stale_remote_plan(&error) => {
                            recheck_required = true;
                            recheck_error = Some(error);
                            continue;
                        }
                        Err(error) => return Err(error),
                    };
                    validate_remote_download_or_quarantine(
                        scope,
                        &relative_path,
                        &bytes,
                        &timestamp,
                    )?;
                    if sha256_hex(&bytes) == local.hash {
                        summary.skipped_files += 1;
                        summary.bytes_downloaded += remote.size;
                        manifest.entries.insert(
                            relative_path,
                            SyncManifestEntry {
                                local_hash: local.hash.clone(),
                                remote_identity: remote.identity.clone(),
                            },
                        );
                        save_sync_manifest(scope, &manifest)?;
                        continue;
                    }
                    let file_name = local
                        .path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .ok_or_else(|| {
                            format!("Local sync file name is invalid: {relative_path}")
                        })?;
                    let conflict_path = write_conflict_copy(
                        scope,
                        &relative_path,
                        &remote_conflict_file_name(file_name, &timestamp),
                        &bytes,
                        hooks,
                    )?;
                    let conflict_relative_path = conflict_path
                        .strip_prefix(scope.source_root())
                        .ok()
                        .and_then(Path::to_str)
                        .map(|path| path.replace('\\', "/"))
                        .ok_or_else(|| {
                            format!("Local sync conflict path is unavailable: {relative_path}")
                        })?;
                    validate_relative_path(&conflict_relative_path)?;
                    let conflict_hash = sha256_hex(&bytes);
                    expected_local_hashes
                        .insert(conflict_relative_path.clone(), conflict_hash.clone());
                    if scope.remote_first_restore()
                        && !manifest.restore_generation_completed
                        && scope.publishes_conflicts_to_source()
                    {
                        manifest
                            .restore_local_only_paths
                            .insert(conflict_relative_path, conflict_hash);
                    }
                    summary.conflict_files += 1;
                    summary.bytes_downloaded += remote.size;
                    manifest.entries.insert(
                        relative_path,
                        SyncManifestEntry {
                            local_hash: local.hash.clone(),
                            remote_identity: remote.identity.clone(),
                        },
                    );
                    save_sync_manifest(scope, &manifest)?;
                }
            }
        }

        let current_local_files = match collect_local_sync_files(scope, hooks) {
            Ok(files) => files,
            Err(error)
                if is_concurrent_local_change(&error)
                    && pass_index + 1 < MAX_IMMEDIATE_RECHECK_PASSES =>
            {
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        let current_local_hashes = local_hashes(&current_local_files);
        if current_local_hashes != expected_local_hashes {
            recheck_required = true;
            if recheck_error.is_none() {
                let changed_path =
                    changed_local_path(&expected_local_hashes, &current_local_hashes);
                recheck_error = Some(
                    concurrent_local_change_error(changed_path.as_deref().unwrap_or("<notebook>"))
                        .into(),
                );
            }
        }
        if recheck_required {
            if pass_index + 1 == MAX_IMMEDIATE_RECHECK_PASSES {
                return Err(recheck_error.unwrap_or_else(|| {
                    RemoteSyncError::unclassified(
                        "sync-run-failed: The sync snapshot did not stabilize.",
                    )
                }));
            }
            continue;
        }
        manifest.entries.retain(|path, _| {
            current_local_files.contains_key(path) || remote_files.contains_key(path)
        });
        manifest.full_scan_completed = true;
        save_sync_manifest(scope, &manifest)?;
        break;
    }
    Ok(summary)
}

fn local_hashes(files: &BTreeMap<String, LocalSyncFile>) -> BTreeMap<String, String> {
    files
        .iter()
        .map(|(path, file)| (path.clone(), file.hash.clone()))
        .collect()
}

fn changed_local_path(
    expected: &BTreeMap<String, String>,
    current: &BTreeMap<String, String>,
) -> Option<String> {
    expected
        .keys()
        .chain(current.keys())
        .find(|path| expected.get(*path) != current.get(*path))
        .cloned()
}

fn is_concurrent_local_change(error: &str) -> bool {
    error.starts_with("Local sync file changed during sync: ")
}

fn is_stale_remote_plan(error: &RemoteSyncError) -> bool {
    error.safe_code() == "s3-object-changed"
}

fn prepare_remote_first_restore(
    scope: &RemoteSyncScope,
    manifest: &mut SyncManifest,
) -> Result<bool, String> {
    if !scope.remote_first_restore() {
        return Ok(false);
    }
    let requested_generation = scope
        .restore_generation()
        .ok_or_else(|| "Remote restore generation is unavailable".to_string())?;
    let same_generation = manifest.restore_generation.as_deref() == Some(requested_generation);
    let unfinished_generation =
        manifest.restore_generation.is_some() && !manifest.restore_generation_completed;

    if unfinished_generation {
        return Ok(false);
    }
    if same_generation {
        manifest.restore_generation_completed = false;
        manifest.restore_local_only_paths.clear();
        save_sync_manifest(scope, manifest)?;
        return Ok(false);
    }

    manifest.entries.clear();
    manifest.full_scan_completed = false;
    manifest.restore_generation = Some(requested_generation.to_string());
    manifest.restore_generation_completed = false;
    manifest.restore_local_only_paths.clear();
    save_sync_manifest(scope, manifest)?;
    Ok(true)
}

pub(crate) fn complete_remote_first_restore_locked(scope: &RemoteSyncScope) -> Result<(), String> {
    if !scope.remote_first_restore() {
        return Ok(());
    }
    let state = scope.open_state_root()?;
    let mut file = state
        .open_with(scope.manifest_name(), &nonfollowing_read_options())
        .map_err(|_| "Sync manifest path is unsafe".to_string())?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(|error| error.to_string())?;
    let mut manifest =
        serde_json::from_str::<SyncManifest>(&contents).map_err(|error| error.to_string())?;
    if manifest.restore_generation.is_none() || !manifest.full_scan_completed {
        return Err("Remote restore checkpoint is incomplete".to_string());
    }
    manifest.restore_generation_completed = true;
    manifest.restore_local_only_paths.clear();
    save_sync_manifest(scope, &manifest)
}

fn required_local<'a>(
    file: Option<&'a LocalSyncFile>,
    action: &str,
    path: &str,
) -> Result<&'a LocalSyncFile, String> {
    file.ok_or_else(|| format!("Local sync file is missing during {action}: {path}"))
}

fn required_remote<'a>(
    file: Option<&'a RemoteSyncFile>,
    action: &str,
    path: &str,
) -> Result<&'a RemoteSyncFile, String> {
    file.ok_or_else(|| format!("Remote sync file is missing during {action}: {path}"))
}

fn collect_local_sync_files(
    scope: &RemoteSyncScope,
    hooks: &RemoteSyncExecutionHooks,
) -> Result<BTreeMap<String, LocalSyncFile>, String> {
    let mut files = BTreeMap::new();
    let root = scope.open_source_root()?;
    collect_local_sync_files_in(scope, &root, "", &mut files, hooks)?;
    Ok(files)
}

fn collect_local_sync_files_in(
    scope: &RemoteSyncScope,
    directory: &Dir,
    relative_directory: &str,
    files: &mut BTreeMap<String, LocalSyncFile>,
    hooks: &RemoteSyncExecutionHooks,
) -> Result<(), String> {
    let mut entries = directory
        .entries()
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    entries.sort_by_key(cap_std::fs::DirEntry::file_name);
    for entry in entries {
        let name = entry.file_name();
        let name_text = name
            .to_str()
            .ok_or_else(|| "Remote sync source contains a non-Unicode path".to_string())?;
        let relative_path = if relative_directory.is_empty() {
            name_text.to_string()
        } else {
            format!("{relative_directory}/{name_text}")
        };
        #[cfg(test)]
        hooks.run_snapshot_entry(&scope.source_root().join(path_from_relative(&relative_path)))?;
        #[cfg(not(test))]
        hooks.run_snapshot_entry(Path::new(&relative_path))?;
        let metadata = directory.symlink_metadata(&name).map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                concurrent_local_change_error(&relative_path)
            } else {
                local_file_error("stat", &relative_path, error)
            }
        })?;
        if scope.should_cleanup_publication_temp(&name) {
            if metadata.is_file() {
                directory.remove_file(&name).map_err(|error| {
                    local_file_error("stale publication cleanup", &relative_path, error)
                })?;
            }
            continue;
        }
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            if !scope.includes_relative_path(&relative_path, true) {
                continue;
            }
            let child = directory
                .open_dir_nofollow(&name)
                .map_err(|_| unsafe_local_path_error(&relative_path))?;
            let retained = child
                .dir_metadata()
                .map_err(|_| unsafe_local_path_error(&relative_path))?;
            if file_identity(&metadata) != file_identity(&retained) {
                return Err(unsafe_local_path_error(&relative_path));
            }
            collect_local_sync_files_in(scope, &child, &relative_path, files, hooks)?;
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        if !scope.includes_relative_path(&relative_path, false) {
            continue;
        }
        let mut file = directory
            .open_with(&name, &nonfollowing_read_options())
            .map_err(|error| {
                if error.kind() == io::ErrorKind::NotFound {
                    concurrent_local_change_error(&relative_path)
                } else {
                    unsafe_local_path_error(&relative_path)
                }
            })?;
        let retained = file
            .metadata()
            .map_err(|_| unsafe_local_path_error(&relative_path))?;
        if !retained.is_file() {
            return Err(unsafe_local_path_error(&relative_path));
        }
        if file_identity(&metadata) != file_identity(&retained) {
            return Err(concurrent_local_change_error(&relative_path));
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|error| local_file_error("read", &relative_path, error))?;
        let final_addressed = directory.symlink_metadata(&name).map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                concurrent_local_change_error(&relative_path)
            } else {
                unsafe_local_path_error(&relative_path)
            }
        })?;
        if final_addressed.file_type().is_symlink() || !final_addressed.is_file() {
            return Err(unsafe_local_path_error(&relative_path));
        }
        if file_identity(&final_addressed) != file_identity(&retained)
            || retained.len() != bytes.len() as u64
        {
            return Err(concurrent_local_change_error(&relative_path));
        }
        files.insert(
            relative_path.clone(),
            LocalSyncFile {
                hash: sha256_hex(&bytes),
                identity: file_identity(&retained),
                path: scope
                    .source_root()
                    .join(relative_path.split('/').collect::<PathBuf>()),
                size: bytes.len() as u64,
            },
        );
    }
    Ok(())
}

fn cleanup_stale_state_staging(scope: &RemoteSyncScope) -> Result<(), String> {
    let root = scope.open_or_create_state_directory("staging")?;
    let entries = root
        .entries()
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    for entry in entries {
        let name = entry.file_name();
        if !scope.should_cleanup_state_staging(&name) {
            continue;
        }
        let metadata = root
            .symlink_metadata(&name)
            .map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_file() {
            root.remove_file(&name).map_err(|error| error.to_string())?;
        } else if metadata.is_dir() {
            let directory = root
                .open_dir_nofollow(&name)
                .map_err(|error| error.to_string())?;
            directory
                .remove_open_dir_all()
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
fn path_from_relative(relative_path: &str) -> PathBuf {
    relative_path.split('/').collect()
}

fn validate_remote_files(files: &BTreeMap<String, RemoteSyncFile>) -> Result<(), String> {
    for path in files.keys() {
        validate_relative_path(path)?;
    }
    Ok(())
}

pub(crate) fn validate_relative_path(path: &str) -> Result<(), String> {
    if path.is_empty()
        || path.starts_with('/')
        || path.ends_with('/')
        || path.contains(['\\', '\0'])
        || path.as_bytes().get(1).is_some_and(|second| *second == b':')
            && path.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
    {
        return Err("Remote sync file path is unsafe".to_string());
    }
    if path
        .split('/')
        .any(|segment| segment.is_empty() || matches!(segment, "." | ".."))
    {
        return Err("Remote sync file path is unsafe".to_string());
    }
    Ok(())
}

struct SafeSyncParent {
    directory: Dir,
    identity: FileIdentity,
    relative_path: PathBuf,
}

struct SyncMutationStaging {
    directory: Dir,
    path: PathBuf,
}

#[derive(Clone, Copy)]
struct VerifiedStagedFile<'a> {
    identity: UniqueRegularFileIdentity,
    hash: &'a str,
    length: u64,
}

fn unsafe_local_path_error(relative_path: &str) -> String {
    format!("Local sync file path is unsafe: {relative_path}")
}

fn concurrent_local_change_error(relative_path: &str) -> String {
    format!("Local sync file changed during sync: {relative_path}")
}

fn create_sync_mutation_staging(
    scope: &RemoteSyncScope,
    relative_path: &str,
) -> Result<SyncMutationStaging, String> {
    let staging_parent = scope.open_or_create_state_directory("staging")?;
    for _ in 0..1000 {
        let sequence = SYNC_MUTATION_STAGING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let name = scope.state_staging_name(sequence);
        match staging_parent.create_dir(&name) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(local_file_error("staging", relative_path, error)),
        }
        let addressed = staging_parent
            .symlink_metadata(&name)
            .map_err(|error| local_file_error("staging", relative_path, error))?;
        if addressed.file_type().is_symlink() || !addressed.is_dir() {
            return Err(unsafe_local_path_error(relative_path));
        }
        let directory = staging_parent
            .open_dir_nofollow(&name)
            .map_err(|_| unsafe_local_path_error(relative_path))?;
        let retained = directory
            .dir_metadata()
            .map_err(|_| unsafe_local_path_error(relative_path))?;
        if file_identity(&addressed) != file_identity(&retained) {
            return Err(unsafe_local_path_error(relative_path));
        }
        return Ok(SyncMutationStaging {
            directory,
            path: scope.staging_root().join(&name),
        });
    }
    Err(format!(
        "Local sync file staging failed: {relative_path}: no unique staging directory"
    ))
}

fn rename_sync_target_noreplace(
    source: &Dir,
    source_name: &str,
    destination: &Dir,
    destination_name: &str,
) -> io::Result<()> {
    crate::atomic_noreplace::rename_noreplace(
        source,
        Path::new(source_name),
        destination,
        Path::new(destination_name),
    )
}

fn cleanup_sync_mutation_staging(
    staging: SyncMutationStaging,
    relative_path: &str,
) -> Result<(), String> {
    staging
        .directory
        .remove_open_dir_all()
        .map_err(|error| local_file_error("staging cleanup", relative_path, error))
}

fn restore_quarantined_sync_target(
    scope: &RemoteSyncScope,
    parent: &Dir,
    file_name: &str,
    quarantine_name: String,
    relative_path: &str,
    cause: String,
    hooks: &RemoteSyncExecutionHooks,
) -> String {
    if let Err(error) = hooks.run_quarantine_restore() {
        return format!("{cause}; restore hook failed: {error}");
    }
    match rename_sync_target_noreplace(
        parent,
        &quarantine_name,
        parent,
        file_name,
    ) {
        Ok(()) => cause,
        Err(restore_error) => match retain_quarantine_in_state(
            scope,
            parent,
            &quarantine_name,
            relative_path,
        ) {
            Ok(retained) => format!(
                "{cause}; local replacement could not be restored without clobbering: {restore_error}; captured content was retained in state at '{}'",
                retained.to_string_lossy()
            ),
            Err(retain_error) => format!(
                "{cause}; local replacement could not be restored without clobbering: {restore_error}; staging '{quarantine_name}' was retained beside the source because state retention failed: {retain_error}"
            ),
        },
    }
}

fn quarantine_and_verify_sync_target(
    parent: &Dir,
    file_name: &str,
    scope: &RemoteSyncScope,
    expected_hash: &str,
    expected_identity: FileIdentity,
    relative_path: &str,
    hooks: &RemoteSyncExecutionHooks,
) -> Result<String, String> {
    let quarantine_name =
        scope.quarantine_temp_name(SYNC_MUTATION_STAGING_SEQUENCE.fetch_add(1, Ordering::Relaxed));
    if let Err(error) = parent.rename(file_name, parent, &quarantine_name) {
        return Err(local_file_error("quarantine", relative_path, error));
    }
    match ensure_safe_sync_file_identity(
        parent,
        &quarantine_name,
        Some((expected_hash, expected_identity)),
        relative_path,
    ) {
        Ok(()) => Ok(quarantine_name.clone()),
        Err(error) => Err(restore_quarantined_sync_target(
            scope,
            parent,
            file_name,
            quarantine_name,
            relative_path,
            error,
            hooks,
        )),
    }
}

fn retain_quarantine_in_state(
    scope: &RemoteSyncScope,
    parent: &Dir,
    quarantine_name: &str,
    relative_path: &str,
) -> Result<PathBuf, String> {
    let state = scope.open_or_create_state_directory("conflicts")?;
    let retained_name =
        scope.quarantine_temp_name(SYNC_MUTATION_STAGING_SEQUENCE.fetch_add(1, Ordering::Relaxed));
    let metadata = parent
        .symlink_metadata(quarantine_name)
        .map_err(|error| local_file_error("retention", relative_path, error))?;
    if metadata.file_type().is_symlink() {
        return Err(unsafe_local_path_error(relative_path));
    }
    if metadata.is_file() {
        let mut source = parent
            .open_with(quarantine_name, &nonfollowing_read_options())
            .map_err(|_| unsafe_local_path_error(relative_path))?;
        let mut bytes = Vec::new();
        source
            .read_to_end(&mut bytes)
            .map_err(|error| local_file_error("retention read", relative_path, error))?;
        let mut options = cap_std::fs::OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        let mut destination = state
            .open_with(&retained_name, &options)
            .map_err(|error| local_file_error("retention write", relative_path, error))?;
        destination
            .write_all(&bytes)
            .and_then(|()| destination.sync_all())
            .map_err(|error| local_file_error("retention write", relative_path, error))?;
        parent
            .remove_file(quarantine_name)
            .map_err(|error| local_file_error("retention cleanup", relative_path, error))?;
    } else if metadata.is_dir() {
        let source = parent
            .open_dir_nofollow(quarantine_name)
            .map_err(|_| unsafe_local_path_error(relative_path))?;
        state
            .create_dir(&retained_name)
            .map_err(|error| local_file_error("retention write", relative_path, error))?;
        let destination = state
            .open_dir_nofollow(&retained_name)
            .map_err(|_| unsafe_local_path_error(relative_path))?;
        copy_directory_nofollow(&source, &destination, relative_path)?;
        source
            .remove_open_dir_all()
            .map_err(|error| local_file_error("retention cleanup", relative_path, error))?;
    } else {
        return Err(unsafe_local_path_error(relative_path));
    }
    Ok(scope.conflict_root().join(retained_name))
}

fn copy_directory_nofollow(
    source: &Dir,
    destination: &Dir,
    relative_path: &str,
) -> Result<(), String> {
    let entries = source
        .entries()
        .map_err(|error| local_file_error("retention read", relative_path, error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| local_file_error("retention read", relative_path, error))?;
    for entry in entries {
        let name = entry.file_name();
        let metadata = source
            .symlink_metadata(&name)
            .map_err(|error| local_file_error("retention read", relative_path, error))?;
        if metadata.file_type().is_symlink() {
            return Err(unsafe_local_path_error(relative_path));
        }
        if metadata.is_dir() {
            let child_source = source
                .open_dir_nofollow(&name)
                .map_err(|_| unsafe_local_path_error(relative_path))?;
            destination
                .create_dir(&name)
                .map_err(|error| local_file_error("retention write", relative_path, error))?;
            let child_destination = destination
                .open_dir_nofollow(&name)
                .map_err(|_| unsafe_local_path_error(relative_path))?;
            copy_directory_nofollow(&child_source, &child_destination, relative_path)?;
        } else if metadata.is_file() {
            let mut source_file = source
                .open_with(&name, &nonfollowing_read_options())
                .map_err(|_| unsafe_local_path_error(relative_path))?;
            let mut bytes = Vec::new();
            source_file
                .read_to_end(&mut bytes)
                .map_err(|error| local_file_error("retention read", relative_path, error))?;
            let mut options = cap_std::fs::OpenOptions::new();
            options
                .write(true)
                .create_new(true)
                .follow(FollowSymlinks::No);
            let mut destination_file = destination
                .open_with(&name, &options)
                .map_err(|error| local_file_error("retention write", relative_path, error))?;
            destination_file
                .write_all(&bytes)
                .and_then(|()| destination_file.sync_all())
                .map_err(|error| local_file_error("retention write", relative_path, error))?;
        } else {
            return Err(unsafe_local_path_error(relative_path));
        }
    }
    Ok(())
}

fn create_local_publication_temp(
    scope: &RemoteSyncScope,
    parent: &Dir,
    state_staging: &Dir,
    state_file: VerifiedStagedFile<'_>,
    relative_path: &str,
) -> Result<(String, UniqueRegularFileIdentity), String> {
    for _ in 0..1000 {
        let sequence = SYNC_MUTATION_STAGING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let name = scope.publication_temp_name(sequence);
        let mut options = cap_std::fs::OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        let mut file = match parent.open_with(&name, &options) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(local_file_error(
                    "publication staging",
                    relative_path,
                    error,
                ))
            }
        };
        let transfer = (|| {
            let mut state = open_exact_staged_file(
                state_staging,
                STAGED_SYNC_REPLACEMENT_NAME,
                state_file.identity,
                relative_path,
            )?;
            let mut digest = Sha256::new();
            let mut copied = 0_u64;
            let mut buffer = [0_u8; 64 * 1024];
            loop {
                let read = state.read(&mut buffer).map_err(|error| {
                    local_file_error("state staging read", relative_path, error)
                })?;
                if read == 0 {
                    break;
                }
                copied = copied
                    .checked_add(read as u64)
                    .ok_or_else(|| unsafe_local_path_error(relative_path))?;
                if copied > state_file.length {
                    return Err(format!(
                        "Local sync state staging changed during sync: {relative_path}"
                    ));
                }
                digest.update(&buffer[..read]);
                file.write_all(&buffer[..read]).map_err(|error| {
                    local_file_error("publication staging", relative_path, error)
                })?;
            }
            let copied_hash = digest
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            if copied != state_file.length || copied_hash != state_file.hash {
                return Err(format!(
                    "Local sync state staging changed during sync: {relative_path}"
                ));
            }
            file.sync_all()
                .map_err(|error| local_file_error("publication staging", relative_path, error))
        })();
        if let Err(error) = transfer {
            drop(file);
            let _cleanup = parent.remove_file(&name);
            return Err(error);
        }
        drop(file);
        let identity = match capture_exact_staged_file(
            parent,
            &name,
            state_file.hash,
            state_file.length,
            relative_path,
        ) {
            Ok(identity) => identity,
            Err(error) => {
                return match cleanup_local_publication_temp(parent, &name, relative_path) {
                    Ok(()) => Err(error),
                    Err(cleanup) => Err(format!("{error}; {cleanup}")),
                };
            }
        };
        return Ok((name, identity));
    }
    Err(format!(
        "Local sync publication staging failed: {relative_path}: no unique temporary file"
    ))
}

fn open_exact_staged_file(
    parent: &Dir,
    name: &str,
    expected: UniqueRegularFileIdentity,
    relative_path: &str,
) -> Result<cap_std::fs::File, String> {
    let addressed = parent
        .symlink_metadata(name)
        .map_err(|_| unsafe_local_path_error(relative_path))?;
    if unique_regular_file_identity(&addressed) != Some(expected) {
        return Err(unsafe_local_path_error(relative_path));
    }
    let file = parent
        .open_with(name, &nonfollowing_read_options())
        .map_err(|_| unsafe_local_path_error(relative_path))?;
    let retained = file
        .metadata()
        .map_err(|_| unsafe_local_path_error(relative_path))?;
    if !expected.matches_retained_regular_file(&retained, false) {
        return Err(unsafe_local_path_error(relative_path));
    }
    Ok(file)
}

fn capture_exact_staged_file(
    parent: &Dir,
    name: &str,
    expected_hash: &str,
    expected_length: u64,
    relative_path: &str,
) -> Result<UniqueRegularFileIdentity, String> {
    let addressed = parent
        .symlink_metadata(name)
        .map_err(|_| unsafe_local_path_error(relative_path))?;
    let identity = unique_regular_file_identity(&addressed)
        .ok_or_else(|| unsafe_local_path_error(relative_path))?;
    let file = open_exact_staged_file(parent, name, identity, relative_path)?;
    verify_open_staged_file(file, expected_hash, expected_length, relative_path)?;
    Ok(identity)
}

fn verify_exact_staged_file(
    parent: &Dir,
    name: &str,
    expected_identity: UniqueRegularFileIdentity,
    expected_hash: &str,
    expected_length: u64,
    relative_path: &str,
) -> Result<(), String> {
    let file = open_exact_staged_file(parent, name, expected_identity, relative_path)?;
    verify_open_staged_file(file, expected_hash, expected_length, relative_path)
}

fn verify_open_staged_file(
    mut file: cap_std::fs::File,
    expected_hash: &str,
    expected_length: u64,
    relative_path: &str,
) -> Result<(), String> {
    let mut digest = Sha256::new();
    let mut length = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| local_file_error("staging verification", relative_path, error))?;
        if read == 0 {
            break;
        }
        length = length
            .checked_add(read as u64)
            .ok_or_else(|| unsafe_local_path_error(relative_path))?;
        if length > expected_length {
            return Err(format!(
                "Local sync staging changed during sync: {relative_path}"
            ));
        }
        digest.update(&buffer[..read]);
    }
    let hash = digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if length != expected_length || hash != expected_hash {
        return Err(format!(
            "Local sync staging changed during sync: {relative_path}"
        ));
    }
    Ok(())
}

fn cleanup_local_publication_temp(
    parent: &Dir,
    name: &str,
    relative_path: &str,
) -> Result<(), String> {
    match parent.remove_file(name) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(local_file_error(
            "publication cleanup",
            relative_path,
            error,
        )),
    }
}

fn open_safe_sync_parent(
    root: &Dir,
    relative_path: &str,
    create: bool,
) -> Result<(SafeSyncParent, String), String> {
    validate_relative_path(relative_path)?;
    let mut segments = relative_path.split('/').collect::<Vec<_>>();
    let file_name = segments
        .pop()
        .ok_or_else(|| unsafe_local_path_error(relative_path))?
        .to_string();
    let mut directory = root
        .try_clone()
        .map_err(|_| unsafe_local_path_error(relative_path))?;
    let mut parent_path = PathBuf::new();

    for segment in segments {
        parent_path.push(segment);
        let addressed = match directory.symlink_metadata(segment) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound && create => {
                if let Err(create_error) = directory.create_dir(segment) {
                    if create_error.kind() != io::ErrorKind::AlreadyExists {
                        return Err(local_file_error(
                            "folder creation",
                            relative_path,
                            create_error,
                        ));
                    }
                }
                directory
                    .symlink_metadata(segment)
                    .map_err(|_| unsafe_local_path_error(relative_path))?
            }
            Err(_) => return Err(unsafe_local_path_error(relative_path)),
        };
        if addressed.file_type().is_symlink() || !addressed.is_dir() {
            return Err(unsafe_local_path_error(relative_path));
        }
        let next = directory
            .open_dir_nofollow(segment)
            .map_err(|_| unsafe_local_path_error(relative_path))?;
        let retained = next
            .dir_metadata()
            .map_err(|_| unsafe_local_path_error(relative_path))?;
        if file_identity(&addressed) != file_identity(&retained) {
            return Err(unsafe_local_path_error(relative_path));
        }
        directory = next;
    }

    let metadata = directory
        .dir_metadata()
        .map_err(|_| unsafe_local_path_error(relative_path))?;
    Ok((
        SafeSyncParent {
            directory,
            identity: file_identity(&metadata),
            relative_path: parent_path,
        },
        file_name,
    ))
}

fn revalidate_safe_sync_parent(
    root: &Dir,
    parent: &SafeSyncParent,
    relative_path: &str,
) -> Result<(), String> {
    let mut reopened = root
        .try_clone()
        .map_err(|_| unsafe_local_path_error(relative_path))?;
    for component in parent.relative_path.components() {
        let Component::Normal(segment) = component else {
            return Err(unsafe_local_path_error(relative_path));
        };
        let addressed = reopened
            .symlink_metadata(segment)
            .map_err(|_| unsafe_local_path_error(relative_path))?;
        if addressed.file_type().is_symlink() || !addressed.is_dir() {
            return Err(unsafe_local_path_error(relative_path));
        }
        let next = reopened
            .open_dir_nofollow(segment)
            .map_err(|_| unsafe_local_path_error(relative_path))?;
        let retained = next
            .dir_metadata()
            .map_err(|_| unsafe_local_path_error(relative_path))?;
        if file_identity(&addressed) != file_identity(&retained) {
            return Err(unsafe_local_path_error(relative_path));
        }
        reopened = next;
    }
    let metadata = reopened
        .dir_metadata()
        .map_err(|_| unsafe_local_path_error(relative_path))?;
    if file_identity(&metadata) != parent.identity {
        return Err(unsafe_local_path_error(relative_path));
    }
    Ok(())
}

fn nonfollowing_read_options() -> cap_std::fs::OpenOptions {
    let mut options = cap_std::fs::OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    #[cfg(unix)]
    options.custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32);
    options
}

fn read_local_upload_bytes(
    scope: &RemoteSyncScope,
    relative_path: &str,
    expected_hash: &str,
    expected_identity: FileIdentity,
    expected_size: u64,
    hooks: &RemoteSyncExecutionHooks,
) -> Result<Vec<u8>, String> {
    let root = scope.open_source_root()?;
    let (parent, file_name) = open_safe_sync_parent(&root, relative_path, false)?;
    let addressed = parent
        .directory
        .symlink_metadata(&file_name)
        .map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                concurrent_local_change_error(relative_path)
            } else {
                unsafe_local_path_error(relative_path)
            }
        })?;
    if addressed.file_type().is_symlink() || !addressed.is_file() {
        return Err(unsafe_local_path_error(relative_path));
    }
    if file_identity(&addressed) != expected_identity {
        return Err(concurrent_local_change_error(relative_path));
    }
    let file = parent
        .directory
        .open_with(&file_name, &nonfollowing_read_options())
        .map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                concurrent_local_change_error(relative_path)
            } else {
                unsafe_local_path_error(relative_path)
            }
        })?;
    let retained = file
        .metadata()
        .map_err(|_| unsafe_local_path_error(relative_path))?;
    if !retained.is_file() {
        return Err(unsafe_local_path_error(relative_path));
    }
    if file_identity(&retained) != expected_identity {
        return Err(concurrent_local_change_error(relative_path));
    }
    #[cfg(test)]
    hooks.run_upload_validated(&scope.source_root().join(path_from_relative(relative_path)))?;
    #[cfg(not(test))]
    hooks.run_upload_validated(Path::new(relative_path))?;
    let mut bytes = Vec::new();
    file.take(expected_size.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| local_file_error("read", relative_path, error))?;
    if bytes.len() as u64 != expected_size || sha256_hex(&bytes) != expected_hash {
        return Err(concurrent_local_change_error(relative_path));
    }
    revalidate_safe_sync_parent(&root, &parent, relative_path)?;
    let final_addressed = parent
        .directory
        .symlink_metadata(&file_name)
        .map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                concurrent_local_change_error(relative_path)
            } else {
                unsafe_local_path_error(relative_path)
            }
        })?;
    if final_addressed.file_type().is_symlink() || !final_addressed.is_file() {
        return Err(unsafe_local_path_error(relative_path));
    }
    if file_identity(&final_addressed) != expected_identity {
        return Err(concurrent_local_change_error(relative_path));
    }
    Ok(bytes)
}

fn ensure_safe_sync_file_identity(
    parent: &Dir,
    file_name: &str,
    expected: Option<(&str, FileIdentity)>,
    relative_path: &str,
) -> Result<(), String> {
    let addressed = match parent.symlink_metadata(file_name) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(unsafe_local_path_error(relative_path))
        }
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(_) => return Err(unsafe_local_path_error(relative_path)),
    };
    match expected {
        None if addressed.is_none() => Ok(()),
        None => Err(concurrent_local_change_error(relative_path)),
        Some((expected_hash, expected_identity)) => {
            let Some(addressed) = addressed else {
                return Err(concurrent_local_change_error(relative_path));
            };
            if file_identity(&addressed) != expected_identity {
                return Err(concurrent_local_change_error(relative_path));
            }
            let mut file = parent
                .open_with(file_name, &nonfollowing_read_options())
                .map_err(|error| {
                    if error.kind() == io::ErrorKind::NotFound {
                        concurrent_local_change_error(relative_path)
                    } else {
                        unsafe_local_path_error(relative_path)
                    }
                })?;
            let retained = file
                .metadata()
                .map_err(|_| unsafe_local_path_error(relative_path))?;
            if !retained.is_file() {
                return Err(unsafe_local_path_error(relative_path));
            }
            if file_identity(&retained) != expected_identity {
                return Err(concurrent_local_change_error(relative_path));
            }
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)
                .map_err(|error| local_file_error("read", relative_path, error))?;
            if sha256_hex(&bytes) != expected_hash {
                return Err(concurrent_local_change_error(relative_path));
            }
            Ok(())
        }
    }
}

fn write_download_atomically(
    scope: &RemoteSyncScope,
    target_relative_path: &str,
    bytes: &[u8],
    expected: Option<(&str, FileIdentity)>,
    relative_path: &str,
    hooks: &RemoteSyncExecutionHooks,
) -> Result<String, String> {
    let download_hash = sha256_hex(bytes);
    let download_length = bytes.len() as u64;
    let source_root = scope.source_root();
    let root = scope.open_source_root()?;
    let (parent, file_name) = open_safe_sync_parent(&root, target_relative_path, true)?;
    ensure_safe_sync_file_identity(&parent.directory, &file_name, expected, relative_path)?;
    let staging = create_sync_mutation_staging(scope, relative_path)?;
    let mut options = cap_std::fs::OpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .follow(FollowSymlinks::No);
    let mut replacement = match staging
        .directory
        .open_with(STAGED_SYNC_REPLACEMENT_NAME, &options)
    {
        Ok(replacement) => replacement,
        Err(error) => {
            let write_error = local_file_error("temporary write", relative_path, error);
            return match cleanup_sync_mutation_staging(staging, relative_path) {
                Ok(()) => Err(write_error),
                Err(cleanup_error) => Err(format!("{write_error}; {cleanup_error}")),
            };
        }
    };
    if let Err(error) = replacement.write_all(bytes) {
        drop(replacement);
        let write_error = local_file_error("temporary write", relative_path, error);
        return match cleanup_sync_mutation_staging(staging, relative_path) {
            Ok(()) => Err(write_error),
            Err(cleanup_error) => Err(format!("{write_error}; {cleanup_error}")),
        };
    }
    if let Err(error) = replacement.sync_all() {
        drop(replacement);
        let write_error = local_file_error("temporary sync", relative_path, error);
        return match cleanup_sync_mutation_staging(staging, relative_path) {
            Ok(()) => Err(write_error),
            Err(cleanup_error) => Err(format!("{write_error}; {cleanup_error}")),
        };
    }
    drop(replacement);
    let state_identity = match capture_exact_staged_file(
        &staging.directory,
        STAGED_SYNC_REPLACEMENT_NAME,
        &download_hash,
        download_length,
        relative_path,
    ) {
        Ok(identity) => identity,
        Err(error) => {
            return match cleanup_sync_mutation_staging(staging, relative_path) {
                Ok(()) => Err(error),
                Err(cleanup_error) => Err(format!("{error}; {cleanup_error}")),
            }
        }
    };
    let target_path = source_root.join(target_relative_path.split('/').collect::<PathBuf>());
    if let Err(error) = hooks.run_state_staged(
        &staging.path.join(STAGED_SYNC_REPLACEMENT_NAME),
        &target_path,
    ) {
        return match cleanup_sync_mutation_staging(staging, relative_path) {
            Ok(()) => Err(error),
            Err(cleanup_error) => Err(format!("{error}; {cleanup_error}")),
        };
    }
    // Durable bytes remain in state_root. Publication always copies them into a
    // same-directory protected temporary file, so external notes volumes do not
    // depend on cross-filesystem rename semantics.
    let state_file = VerifiedStagedFile {
        identity: state_identity,
        hash: &download_hash,
        length: download_length,
    };
    let (publication_name, publication_identity) = match create_local_publication_temp(
        scope,
        &parent.directory,
        &staging.directory,
        state_file,
        relative_path,
    ) {
        Ok(publication) => publication,
        Err(error) => {
            return match cleanup_sync_mutation_staging(staging, relative_path) {
                Ok(()) => Err(error),
                Err(cleanup_error) => Err(format!("{error}; {cleanup_error}")),
            }
        }
    };
    if let Err(error) = hooks.run_atomic_replace(&target_path) {
        let publication_cleanup =
            cleanup_local_publication_temp(&parent.directory, &publication_name, relative_path);
        let state_cleanup = cleanup_sync_mutation_staging(staging, relative_path);
        return Err(combine_cleanup_errors(
            error,
            publication_cleanup,
            state_cleanup,
        ));
    }
    if let Err(error) = revalidate_safe_sync_parent(&root, &parent, relative_path) {
        let publication_cleanup =
            cleanup_local_publication_temp(&parent.directory, &publication_name, relative_path);
        let state_cleanup = cleanup_sync_mutation_staging(staging, relative_path);
        return Err(combine_cleanup_errors(
            error,
            publication_cleanup,
            state_cleanup,
        ));
    }
    if let Err(error) =
        ensure_safe_sync_file_identity(&parent.directory, &file_name, expected, relative_path)
    {
        let publication_cleanup =
            cleanup_local_publication_temp(&parent.directory, &publication_name, relative_path);
        let state_cleanup = cleanup_sync_mutation_staging(staging, relative_path);
        return Err(combine_cleanup_errors(
            error,
            publication_cleanup,
            state_cleanup,
        ));
    }
    if let Err(error) = hooks.run_final_replace(&target_path) {
        let publication_cleanup =
            cleanup_local_publication_temp(&parent.directory, &publication_name, relative_path);
        let state_cleanup = cleanup_sync_mutation_staging(staging, relative_path);
        return Err(combine_cleanup_errors(
            error,
            publication_cleanup,
            state_cleanup,
        ));
    }
    if expected.is_none() {
        if let Err(error) = verify_exact_staged_file(
            &parent.directory,
            &publication_name,
            publication_identity,
            &download_hash,
            download_length,
            relative_path,
        ) {
            let publication_cleanup =
                cleanup_local_publication_temp(&parent.directory, &publication_name, relative_path);
            let state_cleanup = cleanup_sync_mutation_staging(staging, relative_path);
            return Err(combine_cleanup_errors(
                error,
                publication_cleanup,
                state_cleanup,
            ));
        }
        if let Err(error) = rename_sync_target_noreplace(
            &parent.directory,
            &publication_name,
            &parent.directory,
            &file_name,
        ) {
            let publish_error = local_file_error("atomic publish", relative_path, error);
            let publication_cleanup =
                cleanup_local_publication_temp(&parent.directory, &publication_name, relative_path);
            let state_cleanup = cleanup_sync_mutation_staging(staging, relative_path);
            return Err(combine_cleanup_errors(
                publish_error,
                publication_cleanup,
                state_cleanup,
            ));
        }
        cleanup_sync_mutation_staging(staging, relative_path)?;
    } else {
        let Some((expected_hash, expected_identity)) = expected else {
            let error =
                format!("Local sync file identity is missing during replace: {relative_path}");
            return match cleanup_sync_mutation_staging(staging, relative_path) {
                Ok(()) => Err(error),
                Err(cleanup_error) => Err(format!("{error}; {cleanup_error}")),
            };
        };
        let quarantine_name = match quarantine_and_verify_sync_target(
            &parent.directory,
            &file_name,
            scope,
            expected_hash,
            expected_identity,
            relative_path,
            hooks,
        ) {
            Ok(name) => name,
            Err(error) => {
                let publication_cleanup = cleanup_local_publication_temp(
                    &parent.directory,
                    &publication_name,
                    relative_path,
                );
                let state_cleanup = cleanup_sync_mutation_staging(staging, relative_path);
                return Err(combine_cleanup_errors(
                    error,
                    publication_cleanup,
                    state_cleanup,
                ));
            }
        };
        if let Err(error) = verify_exact_staged_file(
            &parent.directory,
            &publication_name,
            publication_identity,
            &download_hash,
            download_length,
            relative_path,
        ) {
            let cause = restore_quarantined_sync_target(
                scope,
                &parent.directory,
                &file_name,
                quarantine_name,
                relative_path,
                error,
                hooks,
            );
            let publication_cleanup =
                cleanup_local_publication_temp(&parent.directory, &publication_name, relative_path);
            let state_cleanup = cleanup_sync_mutation_staging(staging, relative_path);
            return Err(combine_cleanup_errors(
                cause,
                publication_cleanup,
                state_cleanup,
            ));
        }
        if let Err(error) = rename_sync_target_noreplace(
            &parent.directory,
            &publication_name,
            &parent.directory,
            &file_name,
        ) {
            let cause = restore_quarantined_sync_target(
                scope,
                &parent.directory,
                &file_name,
                quarantine_name,
                relative_path,
                local_file_error("atomic publish", relative_path, error),
                hooks,
            );
            let publication_cleanup =
                cleanup_local_publication_temp(&parent.directory, &publication_name, relative_path);
            let state_cleanup = cleanup_sync_mutation_staging(staging, relative_path);
            return Err(combine_cleanup_errors(
                cause,
                publication_cleanup,
                state_cleanup,
            ));
        }
        let quarantine_cleanup = parent
            .directory
            .remove_file(&quarantine_name)
            .map_err(|error| local_file_error("quarantine cleanup", relative_path, error));
        let state_cleanup = cleanup_sync_mutation_staging(staging, relative_path);
        if let Err(error) = quarantine_cleanup {
            return Err(match state_cleanup {
                Ok(()) => error,
                Err(cleanup) => format!("{error}; {cleanup}"),
            });
        }
        state_cleanup?;
    }
    Ok(download_hash)
}

fn delete_local_file(
    scope: &RemoteSyncScope,
    relative_path: &str,
    expected_hash: &str,
    expected_identity: FileIdentity,
    hooks: &RemoteSyncExecutionHooks,
) -> Result<(), String> {
    #[cfg(test)]
    let source_root = scope.source_root();
    let root = scope.open_source_root()?;
    let (parent, file_name) = open_safe_sync_parent(&root, relative_path, false)?;
    ensure_safe_sync_file_identity(
        &parent.directory,
        &file_name,
        Some((expected_hash, expected_identity)),
        relative_path,
    )?;
    revalidate_safe_sync_parent(&root, &parent, relative_path)?;
    #[cfg(test)]
    hooks.run_final_delete(&source_root.join(path_from_relative(relative_path)))?;
    #[cfg(not(test))]
    hooks.run_final_delete(Path::new(relative_path))?;
    let quarantine_name = quarantine_and_verify_sync_target(
        &parent.directory,
        &file_name,
        scope,
        expected_hash,
        expected_identity,
        relative_path,
        hooks,
    )?;
    parent
        .directory
        .remove_file(&quarantine_name)
        .map_err(|error| local_file_error("quarantine cleanup", relative_path, error))
}

fn combine_cleanup_errors(
    cause: String,
    publication_cleanup: Result<(), String>,
    state_cleanup: Result<(), String>,
) -> String {
    [publication_cleanup.err(), state_cleanup.err()]
        .into_iter()
        .flatten()
        .fold(cause, |message, cleanup| format!("{message}; {cleanup}"))
}

fn plan_file_sync(
    local_hash: Option<&str>,
    remote_identity: Option<&str>,
    manifest: Option<&SyncManifestEntry>,
) -> FileSyncAction {
    match (local_hash, remote_identity) {
        (Some(local), None) => match manifest {
            Some(manifest) if local == manifest.local_hash => FileSyncAction::DeleteLocal,
            _ => FileSyncAction::Upload,
        },
        (None, Some(remote)) => match manifest {
            Some(manifest) if remote == manifest.remote_identity => FileSyncAction::DeleteRemote,
            _ => FileSyncAction::Download,
        },
        (None, None) => FileSyncAction::Skip,
        (Some(local), Some(remote)) => {
            let Some(manifest) = manifest else {
                return FileSyncAction::Conflict;
            };
            match (
                local != manifest.local_hash,
                remote != manifest.remote_identity,
            ) {
                (false, false) => FileSyncAction::Skip,
                (true, false) => FileSyncAction::Upload,
                (false, true) => FileSyncAction::Download,
                (true, true) => FileSyncAction::Conflict,
            }
        }
    }
}

fn plan_incomplete_sync(
    local_hash: Option<&str>,
    remote_identity: Option<&str>,
    partial: Option<&SyncManifestEntry>,
) -> FileSyncAction {
    match (local_hash, remote_identity) {
        (Some(_), None) => FileSyncAction::Upload,
        (None, Some(_)) => FileSyncAction::Download,
        (None, None) => FileSyncAction::Skip,
        (Some(local), Some(remote)) => match partial {
            Some(entry) if entry.local_hash == local && entry.remote_identity == remote => {
                FileSyncAction::Skip
            }
            _ => FileSyncAction::Conflict,
        },
    }
}

fn load_sync_manifest(
    scope: &RemoteSyncScope,
    target_fingerprint: &str,
) -> Result<(SyncManifest, bool), String> {
    let state = scope.open_state_root()?;
    let (mut manifest, manifest_exists) = match state.symlink_metadata(scope.manifest_name()) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err("Sync manifest path is unsafe".to_string())
        }
        Ok(_) => {
            let mut file = state
                .open_with(scope.manifest_name(), &nonfollowing_read_options())
                .map_err(|_| "Sync manifest path is unsafe".to_string())?;
            let mut contents = String::new();
            file.read_to_string(&mut contents)
                .map_err(|error| error.to_string())?;
            (
                serde_json::from_str(&contents).map_err(|error| error.to_string())?,
                true,
            )
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => (SyncManifest::default(), false),
        Err(error) => return Err(error.to_string()),
    };
    let version_matches = manifest.version == MANIFEST_VERSION;
    let target_matches = manifest.target_fingerprint == target_fingerprint;
    let local_identity = scope.local_identity().unwrap_or_default();
    let local_identity_matches = manifest.local_identity == local_identity;
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
    } else if manifest.target_fingerprint != target_fingerprint {
        manifest.entries.clear();
        manifest.target_fingerprint = target_fingerprint.to_string();
    }
    if manifest.local_identity.is_empty() {
        manifest.local_identity = local_identity.to_string();
    } else if manifest.local_identity != local_identity {
        manifest.entries.clear();
        manifest.local_identity = local_identity.to_string();
    }
    if !manifest_exists || !version_matches || !target_matches || !local_identity_matches {
        manifest.full_scan_completed = false;
    }
    manifest.version = MANIFEST_VERSION;
    Ok((manifest, has_effective_baseline))
}

fn save_sync_manifest(scope: &RemoteSyncScope, manifest: &SyncManifest) -> Result<(), String> {
    let state = scope.open_state_root()?;
    let contents = serde_json::to_vec_pretty(manifest).map_err(|error| error.to_string())?;
    for _ in 0..1000 {
        let sequence = SYNC_MUTATION_STAGING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let name = format!(".manifest-{}-{sequence}.tmp", std::process::id());
        let mut options = cap_std::fs::OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        let mut file = match state.open_with(&name, &options) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.to_string()),
        };
        let written = file
            .write_all(&contents)
            .and_then(|()| file.sync_all())
            .map_err(|error| error.to_string());
        drop(file);
        if let Err(error) = written {
            let _cleanup = state.remove_file(&name);
            return Err(error);
        }
        scope.open_state_root()?;
        let result = state
            .rename(&name, &state, scope.manifest_name())
            .map_err(|error| error.to_string());
        if result.is_err() {
            let _cleanup = state.remove_file(&name);
        }
        return result;
    }
    Err("Sync manifest staging name is unavailable".to_string())
}

fn remote_conflict_file_name(file_name: &str, timestamp: &str) -> String {
    if let Some((stem, extension)) = file_name.rsplit_once('.') {
        if !stem.is_empty() && !extension.is_empty() {
            return format!("{stem}.remote-conflict-{timestamp}.{extension}");
        }
    }
    format!("{file_name}.remote-conflict-{timestamp}")
}

fn validate_remote_download_or_quarantine(
    scope: &RemoteSyncScope,
    relative_path: &str,
    bytes: &[u8],
    timestamp: &str,
) -> Result<(), String> {
    if scope.validate_download(bytes).is_ok() {
        return Ok(());
    }
    let file_name = relative_path
        .rsplit_once('/')
        .map(|(_, name)| name)
        .unwrap_or(relative_path);
    let invalid_name = if file_name == "settings.json" {
        format!("settings.remote-invalid-{timestamp}.json")
    } else {
        format!("remote-invalid-{timestamp}.bin")
    };
    write_conflict_to_state(scope, relative_path, &invalid_name, bytes)
        .map_err(|_| "remote-settings-invalid: Remote settings validation failed and quarantine could not be completed.".to_string())?;
    Err("remote-settings-invalid: Remote settings are invalid and were quarantined.".to_string())
}

fn write_conflict_copy(
    scope: &RemoteSyncScope,
    source_relative_path: &str,
    conflict_file_name: &str,
    bytes: &[u8],
    hooks: &RemoteSyncExecutionHooks,
) -> Result<PathBuf, String> {
    if !scope.publishes_conflicts_to_source() {
        return write_conflict_to_state(scope, source_relative_path, conflict_file_name, bytes);
    }

    validate_relative_path(source_relative_path)?;
    let parent = source_relative_path
        .rsplit_once('/')
        .map(|(parent, _)| parent)
        .unwrap_or_default();
    let base_relative = if parent.is_empty() {
        conflict_file_name.to_string()
    } else {
        format!("{parent}/{conflict_file_name}")
    };
    validate_relative_path(&base_relative)?;
    let root = scope.open_source_root()?;

    for attempt in 1..1000 {
        let candidate = if attempt == 1 {
            base_relative.clone()
        } else {
            format!("{base_relative}-{attempt}")
        };
        let (candidate_parent, candidate_name) = open_safe_sync_parent(&root, &candidate, true)?;
        match candidate_parent.directory.symlink_metadata(&candidate_name) {
            Ok(_) => continue,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return Err(unsafe_local_path_error(source_relative_path)),
        }
        match write_download_atomically(scope, &candidate, bytes, None, source_relative_path, hooks)
        {
            Ok(_) => {
                return Ok(scope
                    .source_root()
                    .join(candidate.split('/').collect::<PathBuf>()))
            }
            Err(error)
                if error
                    == format!("Local sync file changed during sync: {source_relative_path}") =>
            {
                continue
            }
            Err(error) => return Err(error),
        }
    }
    Err(format!(
        "Local sync conflict path is unavailable: {source_relative_path}"
    ))
}

pub(super) fn preserve_remote_settings_conflict(
    scope: &RemoteSyncScope,
    bytes: Option<&[u8]>,
) -> Result<PathBuf, String> {
    preserve_remote_settings_conflict_with_syncs(scope, bytes, &sync_directory, &sync_directory)
}

#[cfg(test)]
pub(crate) fn preserve_remote_settings_conflict_with_directory_syncs<SyncChild, SyncParent>(
    scope: &RemoteSyncScope,
    bytes: Option<&[u8]>,
    sync_conflict_directory: SyncChild,
    sync_state_root: SyncParent,
) -> Result<PathBuf, String>
where
    SyncChild: Fn(&Dir) -> io::Result<()>,
    SyncParent: Fn(&Dir) -> io::Result<()>,
{
    preserve_remote_settings_conflict_with_syncs(
        scope,
        bytes,
        &sync_conflict_directory,
        &sync_state_root,
    )
}

fn preserve_remote_settings_conflict_with_syncs<SyncChild, SyncParent>(
    scope: &RemoteSyncScope,
    bytes: Option<&[u8]>,
    sync_conflict_directory: &SyncChild,
    sync_state_root: &SyncParent,
) -> Result<PathBuf, String>
where
    SyncChild: Fn(&Dir) -> io::Result<()>,
    SyncParent: Fn(&Dir) -> io::Result<()>,
{
    let timestamp = sync_timestamp();
    match bytes {
        Some(bytes) => write_conflict_to_state_with_directory_syncs(
            scope,
            "settings.json",
            &remote_conflict_file_name("settings.json", &timestamp),
            bytes,
            sync_conflict_directory,
            sync_state_root,
        ),
        None => write_conflict_to_state_with_directory_syncs(
            scope,
            "settings.json",
            &format!("settings.remote-conflict-{timestamp}.deleted"),
            &[],
            sync_conflict_directory,
            sync_state_root,
        ),
    }
}

fn write_conflict_to_state(
    scope: &RemoteSyncScope,
    source_relative_path: &str,
    conflict_file_name: &str,
    bytes: &[u8],
) -> Result<PathBuf, String> {
    write_conflict_to_state_with_directory_syncs(
        scope,
        source_relative_path,
        conflict_file_name,
        bytes,
        &sync_directory,
        &sync_directory,
    )
}

fn write_conflict_to_state_with_directory_syncs<SyncChild, SyncParent>(
    scope: &RemoteSyncScope,
    source_relative_path: &str,
    conflict_file_name: &str,
    bytes: &[u8],
    sync_conflict_directory: &SyncChild,
    sync_state_root: &SyncParent,
) -> Result<PathBuf, String>
where
    SyncChild: Fn(&Dir) -> io::Result<()>,
    SyncParent: Fn(&Dir) -> io::Result<()>,
{
    validate_relative_path(source_relative_path)?;
    let parent = source_relative_path
        .rsplit_once('/')
        .map(|(parent, _)| parent)
        .unwrap_or_default();
    let base_relative = if parent.is_empty() {
        conflict_file_name.to_string()
    } else {
        format!("{parent}/{conflict_file_name}")
    };
    validate_relative_path(&base_relative)?;
    let root = scope.open_or_create_state_directory("conflicts")?;
    let state_root = scope.open_state_root()?;
    let staging = create_sync_mutation_staging(scope, source_relative_path)?;
    let staged_write = (|| {
        let mut options = cap_std::fs::OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        let mut replacement = staging
            .directory
            .open_with(STAGED_SYNC_REPLACEMENT_NAME, &options)
            .map_err(|error| local_file_error("conflict staging", source_relative_path, error))?;
        replacement
            .write_all(bytes)
            .and_then(|()| replacement.sync_all())
            .map_err(|error| local_file_error("conflict staging", source_relative_path, error))
    })();
    if let Err(error) = staged_write {
        return match cleanup_sync_mutation_staging(staging, source_relative_path) {
            Ok(()) => Err(error),
            Err(cleanup) => Err(format!("{error}; {cleanup}")),
        };
    }

    for attempt in 1..1000 {
        let candidate = if attempt == 1 {
            base_relative.clone()
        } else {
            format!("{base_relative}-{attempt}")
        };
        let (parent, file_name) = open_safe_sync_parent(&root, &candidate, true)?;
        match rename_sync_target_noreplace(
            &staging.directory,
            STAGED_SYNC_REPLACEMENT_NAME,
            &parent.directory,
            &file_name,
        ) {
            Ok(()) => {
                if let Err(error) = sync_conflict_directory(&parent.directory) {
                    let publish =
                        local_file_error("conflict publication", source_relative_path, error);
                    return Err(cleanup_failed_conflict_publication(
                        &parent.directory,
                        &file_name,
                        staging,
                        source_relative_path,
                        sync_conflict_directory,
                        publish,
                    ));
                }
                if let Err(error) = sync_state_root(&state_root) {
                    let publish = local_file_error(
                        "conflict publication parent directory",
                        source_relative_path,
                        error,
                    );
                    return Err(cleanup_failed_conflict_publication(
                        &parent.directory,
                        &file_name,
                        staging,
                        source_relative_path,
                        sync_conflict_directory,
                        publish,
                    ));
                }
                cleanup_sync_mutation_staging(staging, source_relative_path)?;
                return Ok(scope
                    .conflict_root()
                    .join(candidate.split('/').collect::<PathBuf>()));
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                let publish = local_file_error("conflict publication", source_relative_path, error);
                return match cleanup_sync_mutation_staging(staging, source_relative_path) {
                    Ok(()) => Err(publish),
                    Err(cleanup) => Err(format!("{publish}; {cleanup}")),
                };
            }
        }
    }
    let error = format!("Local sync conflict path is unavailable: {source_relative_path}");
    match cleanup_sync_mutation_staging(staging, source_relative_path) {
        Ok(()) => Err(error),
        Err(cleanup) => Err(format!("{error}; {cleanup}")),
    }
}

fn cleanup_failed_conflict_publication<Sync>(
    parent: &Dir,
    file_name: &str,
    staging: SyncMutationStaging,
    source_relative_path: &str,
    sync_conflict_directory: &Sync,
    mut error: String,
) -> String
where
    Sync: Fn(&Dir) -> io::Result<()>,
{
    if let Err(cleanup) = parent.remove_file(file_name) {
        if cleanup.kind() != io::ErrorKind::NotFound {
            error.push_str("; ");
            error.push_str(&local_file_error(
                "conflict rollback",
                source_relative_path,
                cleanup,
            ));
        }
    }
    if let Err(cleanup) = sync_conflict_directory(parent) {
        error.push_str("; ");
        error.push_str(&local_file_error(
            "conflict rollback durability",
            source_relative_path,
            cleanup,
        ));
    }
    if let Err(cleanup) = cleanup_sync_mutation_staging(staging, source_relative_path) {
        error.push_str("; ");
        error.push_str(&cleanup);
    }
    error
}

fn local_file_error(action: &str, relative_path: &str, error: impl std::fmt::Display) -> String {
    format!("Local sync file {action} failed: {relative_path}: {error}")
}

fn sync_timestamp() -> String {
    let now = OffsetDateTime::now_utc();
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use super::{
        execute_remote_sync as execute_scoped_remote_sync,
        execute_remote_sync_with_hooks as execute_scoped_remote_sync_with_hooks, plan_file_sync,
        preserve_remote_settings_conflict_with_directory_syncs, sha256_hex, validate_relative_path,
        FileSyncAction, SyncManifestEntry,
    };
    use crate::remote_sync::backend::{
        RemoteSyncBackend, RemoteSyncDiagnostic, RemoteSyncError, RemoteSyncFile,
        SyncFailureCategory, SyncProviderOperation,
    };
    use crate::remote_sync::scope::RemoteSyncScope;

    #[cfg(unix)]
    use std::os::unix::fs::symlink;

    fn test_state_root(root: &Path) -> PathBuf {
        let file_name = root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("remote-sync-test");
        root.with_file_name(format!("{file_name}-sync-state"))
    }

    fn test_scope(root: &Path, manifest_name: &str) -> RemoteSyncScope {
        RemoteSyncScope::notes(root, test_state_root(root), manifest_name, None, None).unwrap()
    }

    #[test]
    fn deleted_remote_settings_conflict_requires_child_and_parent_directory_durability() {
        let app_data = tempfile::tempdir().unwrap();
        let app_data_root = app_data.path().canonicalize().unwrap();
        let settings_state = app_data_root.join("sync-state/settings");
        fs::create_dir_all(&settings_state).unwrap();
        let scope =
            RemoteSyncScope::portable_settings(&app_data_root, settings_state, "manifest.json")
                .unwrap();

        let child_syncs = AtomicUsize::new(0);
        let parent_syncs = AtomicUsize::new(0);
        let error = preserve_remote_settings_conflict_with_directory_syncs(
            &scope,
            None,
            |_| {
                child_syncs.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
            |_| {
                parent_syncs.fetch_add(1, Ordering::SeqCst);
                Err(io::Error::other("injected state root sync failure"))
            },
        )
        .unwrap_err();

        assert!(error.contains("conflict publication"));
        assert_eq!(child_syncs.load(Ordering::SeqCst), 2);
        assert_eq!(parent_syncs.load(Ordering::SeqCst), 1);
        assert!(!fs::read_dir(scope.conflict_root())
            .unwrap()
            .flatten()
            .any(|entry| entry.file_name().to_string_lossy().contains(".deleted")));
    }

    async fn execute_remote_sync<B: RemoteSyncBackend>(
        root: &Path,
        backend: &B,
    ) -> Result<super::RemoteSyncSummary, String> {
        let scope = test_scope(root, "fake-manifest.json");
        execute_scoped_remote_sync(&scope, backend)
            .await
            .map_err(String::from)
    }

    async fn execute_remote_sync_with_hooks<B: RemoteSyncBackend>(
        root: &Path,
        backend: &B,
        hooks: super::RemoteSyncExecutionHooks,
    ) -> Result<super::RemoteSyncSummary, String> {
        let scope = test_scope(root, "fake-manifest.json");
        execute_scoped_remote_sync_with_hooks(&scope, backend, hooks)
            .await
            .map_err(String::from)
    }

    #[test]
    fn execution_hook_bundles_keep_callbacks_isolated() {
        let first_calls = Arc::new(AtomicUsize::new(0));
        let second_calls = Arc::new(AtomicUsize::new(0));
        let first_counter = Arc::clone(&first_calls);
        let second_counter = Arc::clone(&second_calls);
        let first = super::RemoteSyncExecutionHooks {
            final_replace: Some(Box::new(move |_| {
                first_counter.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })),
            ..Default::default()
        };
        let second = super::RemoteSyncExecutionHooks {
            final_replace: Some(Box::new(move |_| {
                second_counter.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })),
            ..Default::default()
        };

        first.run_final_replace(Path::new("first.md")).unwrap();
        second.run_final_replace(Path::new("second.md")).unwrap();

        assert_eq!(first_calls.load(Ordering::SeqCst), 1);
        assert_eq!(second_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn notes_scope_keeps_manifest_and_state_outside_the_source_root() {
        tauri::async_runtime::block_on(async {
            let source = temp_root("notes-scope-source");
            let state = temp_root("notes-scope-state");
            let backend = FakeBackend::new("notes-scope-target");
            write_file(&source.join("note.md"), b"hello");
            let scope = RemoteSyncScope::notes(
                &source,
                &state,
                "notes-fake-manifest.json",
                Some("notes-root-a".to_string()),
                None,
            )
            .unwrap();

            execute_scoped_remote_sync(&scope, &backend).await.unwrap();

            assert!(state.join("notes-fake-manifest.json").is_file());
            assert!(!source.join(".qingyu").exists());
            assert!(!source.join(".markra-sync").exists());
            assert!(!source.read_dir().unwrap().flatten().any(|entry| entry
                .file_name()
                .to_string_lossy()
                .to_ascii_lowercase()
                .starts_with(".markra-sync-stage-")));

            fs::remove_dir_all(source).unwrap();
            fs::remove_dir_all(state).unwrap();
        });
    }

    #[test]
    fn notes_scope_syncs_ordinary_content_and_respects_global_and_workspace_ignores() {
        tauri::async_runtime::block_on(async {
            let source = temp_root("notes-scope-ignore-source");
            let state = temp_root("notes-scope-ignore-state");
            let backend = FakeBackend::new("notes-scope-ignore-target");
            for (path, bytes) in [
                ("note.md", b"note".as_slice()),
                ("assets/image.bin", b"binary".as_slice()),
                ("keep.tmp", b"kept by workspace override".as_slice()),
                ("drop.tmp", b"globally ignored".as_slice()),
                ("ignored/private.txt", b"workspace ignored".as_slice()),
                (
                    "global/private.txt",
                    b"globally ignored directory".as_slice(),
                ),
            ] {
                write_file(&source.join(path), bytes);
            }
            write_file(&source.join(".markraignore"), b"ignored/\n!keep.tmp\n");
            let scope = RemoteSyncScope::notes(
                &source,
                &state,
                "notes-fake-manifest.json",
                None,
                Some("*.tmp\nglobal/\n".to_string()),
            )
            .unwrap();

            execute_scoped_remote_sync(&scope, &backend).await.unwrap();

            let remote = backend.files.lock().unwrap();
            assert_eq!(remote.len(), 3);
            assert_eq!(remote.get("note.md").unwrap(), b"note");
            assert_eq!(remote.get("assets/image.bin").unwrap(), b"binary");
            assert_eq!(
                remote.get("keep.tmp").unwrap(),
                b"kept by workspace override"
            );
            assert!(!remote.contains_key(".markraignore"));
            assert!(!remote.contains_key("drop.tmp"));
            assert!(!remote.contains_key("ignored/private.txt"));
            assert!(!remote.contains_key("global/private.txt"));
            drop(remote);
            fs::remove_dir_all(source).unwrap();
            fs::remove_dir_all(state).unwrap();
        });
    }

    #[cfg(unix)]
    #[test]
    fn prepared_notes_scope_reads_ignore_rules_from_the_retained_directory() {
        tauri::async_runtime::block_on(async {
            let source = temp_root("prepared-ignore-source");
            let retained_path = source.with_file_name(format!(
                "{}-retained",
                source.file_name().unwrap().to_string_lossy()
            ));
            let state = temp_root("prepared-ignore-state");
            let backend = FakeBackend::new("prepared-ignore-target");
            write_file(&source.join("keep.md"), b"retained keep");
            write_file(&source.join("drop.md"), b"retained drop");
            write_file(&source.join(".markraignore"), b"drop.md\n!keep.md\n");
            let retained_directory =
                crate::storage_capability::open_canonical_directory_nofollow(&source).unwrap();

            fs::rename(&source, &retained_path).unwrap();
            fs::create_dir(&source).unwrap();
            write_file(&source.join(".markraignore"), b"keep.md\n!drop.md\n");
            let scope = RemoteSyncScope::notes_from_prepared_directory(
                source.clone(),
                retained_directory,
                &state,
                "notes-fake-manifest.json",
                None,
                None,
            )
            .unwrap();

            execute_scoped_remote_sync(&scope, &backend).await.unwrap();

            let remote = backend.files.lock().unwrap();
            assert_eq!(
                remote.get("keep.md").map(Vec::as_slice),
                Some(b"retained keep".as_slice())
            );
            assert!(!remote.contains_key("drop.md"));
            drop(remote);
            fs::remove_dir_all(source).unwrap();
            fs::remove_dir_all(retained_path).unwrap();
            fs::remove_dir_all(state).unwrap();
        });
    }

    #[test]
    fn portable_settings_scope_can_only_see_settings_json() {
        tauri::async_runtime::block_on(async {
            let app_data = temp_root("settings-scope-app-data");
            let state = app_data.join("sync-state");
            let backend = FakeBackend::new("settings-scope-target");
            for (path, bytes) in [
                ("settings.json", b"local-only".as_slice()),
                ("sync-config.json", b"secret".as_slice()),
                ("local-state.json", b"local".as_slice()),
                ("mcp-runtime/socket", b"runtime".as_slice()),
                ("themes/custom.css", b"theme".as_slice()),
                ("extensions/plugin.js", b"extension".as_slice()),
                ("workspace/note.md", b"note".as_slice()),
            ] {
                write_file(&app_data.join(path), bytes);
            }
            let scope = RemoteSyncScope::portable_settings(
                &app_data,
                &state,
                "settings-fake-manifest.json",
            )
            .unwrap();
            write_file(&scope.source_root().join("settings.json"), b"{}");
            write_file(
                &scope.source_root().join("portable-settings-pending.json"),
                b"local-control-state",
            );

            execute_scoped_remote_sync(&scope, &backend).await.unwrap();

            let remote = backend.files.lock().unwrap();
            assert_eq!(remote.len(), 1);
            assert_eq!(remote.get("settings.json").unwrap(), b"{}");
            assert!(!remote.values().any(|bytes| bytes == b"secret"));
            drop(remote);
            fs::remove_dir_all(app_data).unwrap();
        });
    }

    #[test]
    fn first_settings_sync_prefers_an_existing_valid_remote_file() {
        tauri::async_runtime::block_on(async {
            let app_data = temp_root("settings-first-sync-app-data");
            let state = app_data.join("sync-state");
            let backend = FakeBackend::new("settings-first-sync-target");
            backend.set("settings.json", br#"{"appearanceMode":"dark"}"#);
            let scope = RemoteSyncScope::portable_settings(
                &app_data,
                &state,
                "settings-fake-manifest.json",
            )
            .unwrap();
            write_file(
                &scope.source_root().join("settings.json"),
                br#"{"appearanceMode":"light"}"#,
            );

            let summary = execute_scoped_remote_sync(&scope, &backend).await.unwrap();

            assert_eq!(summary.downloaded_files, 1);
            assert_eq!(summary.conflict_files, 0);
            assert_eq!(
                fs::read(scope.source_root().join("settings.json")).unwrap(),
                br#"{"appearanceMode":"dark"}"#
            );
            assert!(scope
                .state_root()
                .join("settings-fake-manifest.json")
                .is_file());
            fs::remove_dir_all(app_data).unwrap();
        });
    }

    #[test]
    fn invalid_remote_settings_are_quarantined_without_publication() {
        tauri::async_runtime::block_on(async {
            let app_data = temp_root("settings-invalid-app-data");
            let state = app_data.join("sync-state");
            let local = br#"{"appearanceMode":"light"}"#;
            let backend = FakeBackend::new("settings-invalid-target");
            backend.set("settings.json", br#"{"appearanceMode":7}"#);
            let scope = RemoteSyncScope::portable_settings(
                &app_data,
                &state,
                "settings-fake-manifest.json",
            )
            .unwrap();
            write_file(&scope.source_root().join("settings.json"), local);

            let error = execute_scoped_remote_sync(&scope, &backend)
                .await
                .expect_err("invalid remote settings must fail");

            assert!(error.starts_with("remote-settings-invalid:"), "{error}");
            assert!(!error.contains("appearanceMode"), "{error}");
            assert_eq!(
                fs::read(scope.source_root().join("settings.json")).unwrap(),
                local
            );
            let conflicts = fs::read_dir(scope.state_root().join("conflicts"))
                .unwrap()
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            assert!(
                conflicts.iter().any(|name| {
                    name.starts_with("settings.remote-invalid-") && name.ends_with(".json")
                }),
                "missing invalid-settings quarantine: {conflicts:?}"
            );
            fs::remove_dir_all(app_data).unwrap();
        });
    }

    #[test]
    fn changed_settings_target_has_no_effective_baseline_and_remote_wins() {
        tauri::async_runtime::block_on(async {
            let app_data = temp_root("settings-changed-target-app-data");
            let state = app_data.join("sync-state");
            let first = FakeBackend::new("settings-target-a");
            let first_scope = RemoteSyncScope::portable_settings(
                &app_data,
                &state,
                "settings-fake-manifest.json",
            )
            .unwrap();
            write_file(
                &first_scope.source_root().join("settings.json"),
                br#"{"appearanceMode":"light"}"#,
            );
            execute_scoped_remote_sync(&first_scope, &first)
                .await
                .unwrap();

            let second = FakeBackend::new("settings-target-b");
            second.set("settings.json", br#"{"appearanceMode":"dark"}"#);
            let second_scope = RemoteSyncScope::portable_settings(
                &app_data,
                &state,
                "settings-fake-manifest.json",
            )
            .unwrap();
            let summary = execute_scoped_remote_sync(&second_scope, &second)
                .await
                .unwrap();

            assert_eq!(summary.downloaded_files, 1);
            assert_eq!(summary.conflict_files, 0);
            assert_eq!(
                fs::read(second_scope.source_root().join("settings.json")).unwrap(),
                br#"{"appearanceMode":"dark"}"#
            );
            fs::remove_dir_all(app_data).unwrap();
        });
    }

    #[test]
    fn settings_conflict_after_baseline_keeps_local_and_quarantines_remote() {
        tauri::async_runtime::block_on(async {
            let app_data = temp_root("settings-conflict-app-data");
            let state = app_data.join("sync-state");
            let backend = FakeBackend::new("settings-conflict-target");
            let first_scope = RemoteSyncScope::portable_settings(
                &app_data,
                &state,
                "settings-fake-manifest.json",
            )
            .unwrap();
            write_file(
                &first_scope.source_root().join("settings.json"),
                br#"{"appearanceMode":"light"}"#,
            );
            execute_scoped_remote_sync(&first_scope, &backend)
                .await
                .unwrap();

            let local = br#"{"appearanceMode":"dark"}"#;
            let remote = br#"{"appearanceMode":"system"}"#;
            write_file(&first_scope.source_root().join("settings.json"), local);
            backend.set("settings.json", remote);
            let second_scope = RemoteSyncScope::portable_settings(
                &app_data,
                &state,
                "settings-fake-manifest.json",
            )
            .unwrap();
            let summary = execute_scoped_remote_sync(&second_scope, &backend)
                .await
                .unwrap();

            assert_eq!(summary.conflict_files, 1);
            assert_eq!(
                fs::read(second_scope.source_root().join("settings.json")).unwrap(),
                local
            );
            assert!(fs::read_dir(second_scope.state_root().join("conflicts"))
                .unwrap()
                .flatten()
                .any(|entry| fs::read(entry.path()).ok().as_deref() == Some(remote)));
            fs::remove_dir_all(app_data).unwrap();
        });
    }

    #[test]
    fn changed_notes_root_identity_restarts_without_remote_deletion_or_settings_reset() {
        tauri::async_runtime::block_on(async {
            let first_root = temp_root("notes-identity-first");
            let second_root = temp_root("notes-identity-second");
            let app_data = temp_root("notes-identity-app-data");
            let state = app_data.join("sync-state");
            let notes_backend = FakeBackend::new("notes-identity-target");
            let settings_backend = FakeBackend::new("settings-identity-target");
            write_file(&first_root.join("first.md"), b"first");
            let first_notes_scope = RemoteSyncScope::notes(
                &first_root,
                &state,
                "notes-fake-manifest.json",
                Some(first_root.to_string_lossy().into_owned()),
                None,
            )
            .unwrap();
            let settings_scope = RemoteSyncScope::portable_settings(
                &app_data,
                &state,
                "settings-fake-manifest.json",
            )
            .unwrap();
            write_file(&settings_scope.source_root().join("settings.json"), b"{}");
            execute_scoped_remote_sync(&first_notes_scope, &notes_backend)
                .await
                .unwrap();
            execute_scoped_remote_sync(&settings_scope, &settings_backend)
                .await
                .unwrap();
            let settings_manifest_before = fs::read(
                settings_scope
                    .state_root()
                    .join("settings-fake-manifest.json"),
            )
            .unwrap();

            write_file(&second_root.join("second.md"), b"second");
            let second_notes_scope = RemoteSyncScope::notes(
                &second_root,
                &state,
                "notes-fake-manifest.json",
                Some(second_root.to_string_lossy().into_owned()),
                None,
            )
            .unwrap();
            let summary = execute_scoped_remote_sync(&second_notes_scope, &notes_backend)
                .await
                .unwrap();

            assert_eq!(summary.conflict_files, 0);
            assert_eq!(
                notes_backend.get("first.md").as_deref(),
                Some(b"first".as_slice())
            );
            assert_eq!(
                notes_backend.get("second.md").as_deref(),
                Some(b"second".as_slice())
            );
            assert_eq!(fs::read(second_root.join("first.md")).unwrap(), b"first");
            assert_eq!(
                fs::read(
                    settings_scope
                        .state_root()
                        .join("settings-fake-manifest.json")
                )
                .unwrap(),
                settings_manifest_before
            );
            fs::remove_dir_all(first_root).unwrap();
            fs::remove_dir_all(second_root).unwrap();
            fs::remove_dir_all(app_data).unwrap();
        });
    }

    #[test]
    fn notes_scope_rejects_state_inside_source() {
        let source = temp_root("notes-scope-overlap");
        let error = RemoteSyncScope::notes(
            &source,
            source.join("sync-state"),
            "notes-fake-manifest.json",
            None,
            None,
        )
        .unwrap_err();

        assert!(error.contains("state"), "{error}");
        fs::remove_dir_all(source).unwrap();
    }

    #[test]
    fn portable_settings_scope_rejects_state_outside_app_data() {
        let app_data = temp_root("portable-state-app-data");
        let outside = temp_root("portable-state-outside");

        let error =
            RemoteSyncScope::portable_settings(&app_data, &outside, "settings-fake-manifest.json")
                .unwrap_err();

        assert!(error.contains("app data"), "{error}");
        fs::remove_dir_all(app_data).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn scope_rejects_a_state_root_symlink() {
        let source = temp_root("state-symlink-source");
        let holder = temp_root("state-symlink-holder");
        let outside = temp_root("state-symlink-outside");
        let state = holder.join("state");
        symlink(&outside, &state).unwrap();

        let error = RemoteSyncScope::notes(&source, &state, "notes-fake-manifest.json", None, None)
            .unwrap_err();

        assert!(error.contains("unsafe"), "{error}");
        fs::remove_file(state).unwrap();
        fs::remove_dir_all(source).unwrap();
        fs::remove_dir_all(holder).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn scope_rejects_a_state_root_with_a_symlink_ancestor() {
        let source = temp_root("state-ancestor-source");
        let holder = temp_root("state-ancestor-holder");
        let outside = temp_root("state-ancestor-outside");
        let alias = holder.join("alias");
        symlink(&outside, &alias).unwrap();

        let error = RemoteSyncScope::notes(
            &source,
            alias.join("state"),
            "notes-fake-manifest.json",
            None,
            None,
        )
        .unwrap_err();

        assert!(error.contains("unsafe"), "{error}");
        assert!(!outside.join("state").exists());
        fs::remove_dir_all(source).unwrap();
        fs::remove_dir_all(holder).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn sync_rejects_state_root_replacement_after_scope_creation() {
        tauri::async_runtime::block_on(async {
            let source = temp_root("state-replacement-source");
            let state = temp_root("state-replacement-state");
            let retained = state.with_file_name(format!(
                "{}-retained",
                state.file_name().unwrap().to_string_lossy()
            ));
            let replacement = temp_root("state-replacement-new-directory");
            let backend = FakeBackend::new("state-replacement-target");
            backend.set("note.md", b"remote");
            let scope = RemoteSyncScope::notes(&source, &state, "notes.json", None, None).unwrap();
            fs::rename(&state, &retained).unwrap();
            fs::rename(&replacement, &state).unwrap();

            let error = execute_scoped_remote_sync(&scope, &backend)
                .await
                .expect_err("replaced state root must fail closed");

            assert!(error.contains("unsafe"), "{error}");
            assert!(!source.join("note.md").exists());
            assert_eq!(fs::read_dir(&state).unwrap().count(), 0);
            fs::remove_dir_all(source).unwrap();
            fs::remove_dir_all(state).unwrap();
            fs::remove_dir_all(retained).unwrap();
        });
    }

    #[test]
    fn two_tier_download_copies_state_staging_into_a_local_publication_temp() {
        tauri::async_runtime::block_on(async {
            let source = temp_root("cross-device-source");
            let state = temp_root("cross-device-state");
            let backend = FakeBackend::new("cross-device-target");
            backend.set("download.md", b"remote bytes");
            let scope =
                RemoteSyncScope::notes(&source, &state, "notes-fake-manifest.json", None, None)
                    .unwrap();
            let saw_state_staging = Arc::new(AtomicUsize::new(0));
            let saw_state_staging_in_hook = Arc::clone(&saw_state_staging);
            let expected_state = state.canonicalize().unwrap();
            let hooks = super::RemoteSyncExecutionHooks {
                state_staged: Some(Box::new(move |staged, target| {
                    assert!(staged.starts_with(expected_state.join("staging")));
                    assert!(staged.is_file());
                    assert_eq!(target.file_name().unwrap(), "download.md");
                    saw_state_staging_in_hook.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })),
                ..Default::default()
            };

            execute_scoped_remote_sync_with_hooks(&scope, &backend, hooks)
                .await
                .unwrap();

            assert_eq!(saw_state_staging.load(Ordering::SeqCst), 1);
            assert_eq!(
                fs::read(source.join("download.md")).unwrap(),
                b"remote bytes"
            );
            assert!(!source.read_dir().unwrap().flatten().any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .starts_with(".markra-sync-stage-")
            }));
            assert!(state
                .join("staging")
                .read_dir()
                .into_iter()
                .flatten()
                .flatten()
                .next()
                .is_none());
            let manifest = fs::read_to_string(state.join("notes-fake-manifest.json")).unwrap();
            assert!(!manifest.contains("markra-sync-stage"));
            assert!(!backend
                .files
                .lock()
                .unwrap()
                .keys()
                .any(|path| path.to_ascii_lowercase().contains("markra-sync-stage")));

            fs::remove_dir_all(source).unwrap();
            fs::remove_dir_all(state).unwrap();
        });
    }

    #[cfg(unix)]
    #[test]
    fn publication_rejects_a_symlink_replacement_before_final_rename() {
        assert_publication_replacement_is_rejected("symlink", |publication, outside| {
            fs::remove_file(publication).map_err(|error| error.to_string())?;
            symlink(outside, publication).map_err(|error| error.to_string())
        });
    }

    #[test]
    fn publication_rejects_a_hardlink_replacement_before_final_rename() {
        assert_publication_replacement_is_rejected("hardlink", |publication, outside| {
            fs::remove_file(publication).map_err(|error| error.to_string())?;
            fs::hard_link(outside, publication).map_err(|error| error.to_string())
        });
    }

    #[test]
    fn publication_rejects_same_size_different_regular_bytes_before_final_rename() {
        assert_publication_replacement_is_rejected("same-size-regular", |publication, _| {
            fs::write(publication, b"evil-new!!").map_err(|error| error.to_string())
        });
    }

    #[test]
    fn state_staging_is_revalidated_and_is_the_only_publication_source() {
        tauri::async_runtime::block_on(async {
            let source = temp_root("state-stage-trust-source");
            let state = temp_root("state-stage-trust-state");
            let backend = FakeBackend::new("state-stage-trust-target");
            backend.set("note.md", b"remote-good");
            let scope = RemoteSyncScope::notes(&source, &state, "notes.json", None, None).unwrap();
            let hooks = super::RemoteSyncExecutionHooks {
                state_staged: Some(Box::new(|staged, _| {
                    fs::write(staged, b"remote-evil").map_err(|error| error.to_string())
                })),
                ..Default::default()
            };

            let error = execute_scoped_remote_sync_with_hooks(&scope, &backend, hooks)
                .await
                .expect_err("modified durable state must not publish remembered bytes");

            assert!(
                error.contains("changed") || error.contains("unsafe"),
                "{error}"
            );
            assert!(!source.join("note.md").exists());
            assert_no_sibling_sync_mutation_artifacts(&source);
            assert!(state
                .join("staging")
                .read_dir()
                .into_iter()
                .flatten()
                .flatten()
                .next()
                .is_none());
            fs::remove_dir_all(source).unwrap();
            fs::remove_dir_all(state).unwrap();
        });
    }

    #[test]
    fn note_conflict_copies_are_written_beside_the_source_note() {
        tauri::async_runtime::block_on(async {
            let source = temp_root("state-conflict-source");
            let state = temp_root("state-conflict-state");
            let backend = FakeBackend::new("state-conflict-target");
            let scope =
                RemoteSyncScope::notes(&source, &state, "notes-fake-manifest.json", None, None)
                    .unwrap();
            write_file(&source.join("draft.md"), b"baseline");
            execute_scoped_remote_sync(&scope, &backend).await.unwrap();
            write_file(&source.join("draft.md"), b"local changed");
            backend.set("draft.md", b"remote changed");

            let summary = execute_scoped_remote_sync(&scope, &backend).await.unwrap();

            assert_eq!(summary.conflict_files, 1);
            assert_eq!(fs::read(source.join("draft.md")).unwrap(), b"local changed");
            assert!(find_conflict_file(&source).is_some());
            assert!(find_conflict_file(&state.join("conflicts")).is_none());
            fs::remove_dir_all(source).unwrap();
            fs::remove_dir_all(state).unwrap();
        });
    }

    struct FakeBackend {
        files: Mutex<BTreeMap<String, Vec<u8>>>,
        replacement_on_next_delete: Mutex<Option<Vec<u8>>>,
        replacement_on_next_download: Mutex<Option<Vec<u8>>>,
        replacement_on_next_upload: Mutex<Option<Vec<u8>>>,
        target: String,
    }

    struct RecordingBackend {
        fail_download_once: Mutex<Option<String>>,
        fail_list_once: Mutex<bool>,
        files: Mutex<BTreeMap<String, Vec<u8>>>,
        operations: Mutex<Vec<String>>,
        target: String,
    }

    #[derive(Default)]
    struct ConcurrencyBackend {
        active: AtomicUsize,
        max_active: AtomicUsize,
    }

    impl RemoteSyncBackend for ConcurrencyBackend {
        fn target_fingerprint_source(&self) -> String {
            "concurrency-target".to_string()
        }

        async fn list_files(&self) -> Result<BTreeMap<String, RemoteSyncFile>, RemoteSyncError> {
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_active.fetch_max(active, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(75)).await;
            self.active.fetch_sub(1, Ordering::SeqCst);
            Ok(BTreeMap::new())
        }

        async fn download(
            &self,
            _path: &str,
            _expected_identity: &str,
        ) -> Result<Vec<u8>, RemoteSyncError> {
            unreachable!("empty concurrency backend never downloads")
        }

        async fn upload(
            &self,
            _path: &str,
            _bytes: &[u8],
            _expected_identity: Option<&str>,
        ) -> Result<String, RemoteSyncError> {
            unreachable!("empty concurrency backend never uploads")
        }

        async fn delete(
            &self,
            _path: &str,
            _expected_identity: &str,
        ) -> Result<(), RemoteSyncError> {
            unreachable!("empty concurrency backend never deletes")
        }
    }

    impl FakeBackend {
        fn new(target: &str) -> Self {
            Self {
                files: Mutex::new(BTreeMap::new()),
                replacement_on_next_delete: Mutex::new(None),
                replacement_on_next_download: Mutex::new(None),
                replacement_on_next_upload: Mutex::new(None),
                target: target.to_string(),
            }
        }

        fn set(&self, path: &str, bytes: &[u8]) {
            self.files
                .lock()
                .unwrap()
                .insert(path.to_string(), bytes.to_vec());
        }

        fn get(&self, path: &str) -> Option<Vec<u8>> {
            self.files.lock().unwrap().get(path).cloned()
        }

        fn remove(&self, path: &str) {
            self.files.lock().unwrap().remove(path);
        }

        fn replace_on_next_upload(&self, bytes: &[u8]) {
            *self.replacement_on_next_upload.lock().unwrap() = Some(bytes.to_vec());
        }

        fn replace_on_next_download(&self, bytes: &[u8]) {
            *self.replacement_on_next_download.lock().unwrap() = Some(bytes.to_vec());
        }

        fn replace_on_next_delete(&self, bytes: &[u8]) {
            *self.replacement_on_next_delete.lock().unwrap() = Some(bytes.to_vec());
        }

        fn identity(bytes: &[u8]) -> String {
            format!("sha256:{}", sha256_hex(bytes))
        }

        fn stale_remote_change(operation: SyncProviderOperation, method: &str) -> RemoteSyncError {
            RemoteSyncError::diagnostic(RemoteSyncDiagnostic {
                category: SyncFailureCategory::Integrity,
                code: "s3-object-changed".to_string(),
                http_status: None,
                method: Some(method.to_string()),
                object_id: Some("test-object".to_string()),
                operation,
                provider_error_code: None,
                request_id: None,
                run_id: "test-run".to_string(),
                scope: "notes".to_string(),
            })
        }
    }

    impl RecordingBackend {
        fn new(target: &str) -> Self {
            Self {
                fail_download_once: Mutex::new(None),
                fail_list_once: Mutex::new(false),
                files: Mutex::new(BTreeMap::new()),
                operations: Mutex::new(Vec::new()),
                target: target.to_string(),
            }
        }

        fn set(&self, path: &str, bytes: &[u8]) {
            self.files
                .lock()
                .unwrap()
                .insert(path.to_string(), bytes.to_vec());
        }

        fn fail_download_once(&self, path: &str) {
            *self.fail_download_once.lock().unwrap() = Some(path.to_string());
        }

        fn fail_list_once(&self) {
            *self.fail_list_once.lock().unwrap() = true;
        }

        fn operations(&self) -> Vec<String> {
            self.operations.lock().unwrap().clone()
        }

        fn clear_operations(&self) {
            self.operations.lock().unwrap().clear();
        }
    }

    impl RemoteSyncBackend for FakeBackend {
        fn target_fingerprint_source(&self) -> String {
            self.target.clone()
        }

        async fn list_files(&self) -> Result<BTreeMap<String, RemoteSyncFile>, RemoteSyncError> {
            Ok(self
                .files
                .lock()
                .unwrap()
                .iter()
                .map(|(path, bytes)| {
                    (
                        path.clone(),
                        RemoteSyncFile {
                            identity: Self::identity(bytes),
                            size: bytes.len() as u64,
                        },
                    )
                })
                .collect())
        }

        async fn download(
            &self,
            path: &str,
            expected_identity: &str,
        ) -> Result<Vec<u8>, RemoteSyncError> {
            if let Some(replacement) = self.replacement_on_next_download.lock().unwrap().take() {
                self.files
                    .lock()
                    .unwrap()
                    .insert(path.to_string(), replacement);
                return Err(Self::stale_remote_change(
                    SyncProviderOperation::Download,
                    "GET",
                ));
            }
            let bytes = self
                .files
                .lock()
                .unwrap()
                .get(path)
                .cloned()
                .ok_or_else(|| "missing fake remote file".to_string())?;
            if Self::identity(&bytes) != expected_identity {
                return Err("fake remote changed".into());
            }
            Ok(bytes)
        }

        async fn upload(
            &self,
            path: &str,
            bytes: &[u8],
            expected_identity: Option<&str>,
        ) -> Result<String, RemoteSyncError> {
            let mut files = self.files.lock().unwrap();
            if let Some(replacement) = self.replacement_on_next_upload.lock().unwrap().take() {
                files.insert(path.to_string(), replacement);
                return Err(Self::stale_remote_change(
                    SyncProviderOperation::Upload,
                    "PUT",
                ));
            }
            let actual = files.get(path).map(|bytes| Self::identity(bytes));
            if actual.as_deref() != expected_identity {
                return Err("fake remote changed".into());
            }
            files.insert(path.to_string(), bytes.to_vec());
            Ok(Self::identity(bytes))
        }

        async fn delete(&self, path: &str, expected_identity: &str) -> Result<(), RemoteSyncError> {
            let mut files = self.files.lock().unwrap();
            if let Some(replacement) = self.replacement_on_next_delete.lock().unwrap().take() {
                files.insert(path.to_string(), replacement);
                return Err(Self::stale_remote_change(
                    SyncProviderOperation::Delete,
                    "DELETE",
                ));
            }
            let actual = files.get(path).map(|bytes| Self::identity(bytes));
            if actual.as_deref() != Some(expected_identity) {
                return Err("fake remote changed".into());
            }
            files.remove(path);
            Ok(())
        }
    }

    impl RemoteSyncBackend for RecordingBackend {
        fn target_fingerprint_source(&self) -> String {
            self.target.clone()
        }

        async fn list_files(&self) -> Result<BTreeMap<String, RemoteSyncFile>, RemoteSyncError> {
            let mut fail_list_once = self.fail_list_once.lock().unwrap();
            if *fail_list_once {
                *fail_list_once = false;
                return Err("recording list failed".into());
            }
            drop(fail_list_once);
            Ok(self
                .files
                .lock()
                .unwrap()
                .iter()
                .map(|(path, bytes)| {
                    (
                        path.clone(),
                        RemoteSyncFile {
                            identity: FakeBackend::identity(bytes),
                            size: bytes.len() as u64,
                        },
                    )
                })
                .collect())
        }

        async fn download(
            &self,
            path: &str,
            expected_identity: &str,
        ) -> Result<Vec<u8>, RemoteSyncError> {
            self.operations
                .lock()
                .unwrap()
                .push(format!("download:{path}"));
            let should_fail = self.fail_download_once.lock().unwrap().as_deref() == Some(path);
            if should_fail {
                *self.fail_download_once.lock().unwrap() = None;
                return Err(format!("recording download failed: {path}").into());
            }
            let bytes = self
                .files
                .lock()
                .unwrap()
                .get(path)
                .cloned()
                .ok_or_else(|| "missing recording remote file".to_string())?;
            if FakeBackend::identity(&bytes) != expected_identity {
                return Err("recording remote changed".into());
            }
            Ok(bytes)
        }

        async fn upload(
            &self,
            path: &str,
            bytes: &[u8],
            expected_identity: Option<&str>,
        ) -> Result<String, RemoteSyncError> {
            self.operations
                .lock()
                .unwrap()
                .push(format!("upload:{path}"));
            let mut files = self.files.lock().unwrap();
            let actual = files.get(path).map(|bytes| FakeBackend::identity(bytes));
            if actual.as_deref() != expected_identity {
                return Err("recording remote changed".into());
            }
            files.insert(path.to_string(), bytes.to_vec());
            Ok(FakeBackend::identity(bytes))
        }

        async fn delete(&self, path: &str, expected_identity: &str) -> Result<(), RemoteSyncError> {
            self.operations
                .lock()
                .unwrap()
                .push(format!("delete:{path}"));
            let mut files = self.files.lock().unwrap();
            let actual = files.get(path).map(|bytes| FakeBackend::identity(bytes));
            if actual.as_deref() != Some(expected_identity) {
                return Err("recording remote changed".into());
            }
            files.remove(path);
            Ok(())
        }
    }

    #[test]
    fn first_notes_sync_hydrates_every_remote_path_before_uploading_local_only_paths() {
        tauri::async_runtime::block_on(async {
            let root = temp_root("first-sync-order");
            let backend = RecordingBackend::new("first-sync-order-target");
            write_file(&root.join("a-local-only.md"), b"local only");
            write_file(&root.join("b-both-equal.md"), b"shared");
            write_file(&root.join("c-both-different.md"), b"local original");
            backend.set("b-both-equal.md", b"shared");
            backend.set("c-both-different.md", b"remote conflict");
            backend.set("z-remote-only.md", b"remote only");

            execute_remote_sync(&root, &backend).await.unwrap();

            assert_eq!(
                backend.operations(),
                [
                    "download:b-both-equal.md",
                    "download:c-both-different.md",
                    "download:z-remote-only.md",
                    "upload:a-local-only.md",
                ]
            );
            fs::remove_dir_all(root).unwrap();
        });
    }

    #[test]
    fn first_notes_sync_skips_equal_both_side_content() {
        tauri::async_runtime::block_on(async {
            let root = temp_root("first-sync-equal");
            let backend = RecordingBackend::new("first-sync-equal-target");
            write_file(&root.join("same.md"), b"same bytes");
            backend.set("same.md", b"same bytes");

            let summary = execute_remote_sync(&root, &backend).await.unwrap();

            assert_eq!(summary.skipped_files, 1);
            assert_eq!(summary.conflict_files, 0);
            assert_eq!(backend.operations(), ["download:same.md"]);
            assert!(find_conflict_file(&root).is_none());
            fs::remove_dir_all(root).unwrap();
        });
    }

    #[test]
    fn first_notes_sync_keeps_local_original_and_publishes_remote_conflict_copy() {
        tauri::async_runtime::block_on(async {
            let root = temp_root("first-sync-conflict");
            let backend = RecordingBackend::new("first-sync-conflict-target");
            write_file(&root.join("draft.md"), b"local original");
            backend.set("draft.md", b"remote conflict");

            let summary = execute_remote_sync(&root, &backend).await.unwrap();

            assert_eq!(summary.conflict_files, 1);
            assert_eq!(fs::read(root.join("draft.md")).unwrap(), b"local original");
            let conflict =
                find_conflict_file(&root).expect("remote conflict copy should be visible");
            assert_eq!(fs::read(conflict).unwrap(), b"remote conflict");
            fs::remove_dir_all(root).unwrap();
        });
    }

    #[test]
    fn interrupted_notes_bootstrap_checkpoints_without_deleting_or_early_uploading() {
        tauri::async_runtime::block_on(async {
            let root = temp_root("interrupted-first-sync");
            let backend = RecordingBackend::new("interrupted-first-sync-target");
            write_file(&root.join("a-local-only.md"), b"local only");
            backend.set("b-completed.md", b"completed remote");
            backend.set("c-redownload.md", b"redownload remote");
            backend.set("d-failure.md", b"eventual remote");
            backend.fail_download_once("d-failure.md");

            let error = execute_remote_sync(&root, &backend)
                .await
                .expect_err("bootstrap should stop at the injected remote failure");

            assert!(error.contains("recording download failed"), "{error}");
            assert_eq!(
                backend.operations(),
                [
                    "download:b-completed.md",
                    "download:c-redownload.md",
                    "download:d-failure.md",
                ]
            );
            assert!(!backend
                .files
                .lock()
                .unwrap()
                .contains_key("a-local-only.md"));
            let manifest_path = test_state_root(&root).join("fake-manifest.json");
            let partial: serde_json::Value =
                serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
            assert_eq!(partial["full_scan_completed"], false);

            fs::remove_file(root.join("c-redownload.md")).unwrap();
            backend.clear_operations();

            execute_remote_sync(&root, &backend).await.unwrap();

            assert_eq!(
                backend.operations(),
                [
                    "download:c-redownload.md",
                    "download:d-failure.md",
                    "upload:a-local-only.md",
                ]
            );
            assert_eq!(
                fs::read(root.join("c-redownload.md")).unwrap(),
                b"redownload remote"
            );
            assert_eq!(
                backend
                    .files
                    .lock()
                    .unwrap()
                    .get("c-redownload.md")
                    .unwrap(),
                b"redownload remote"
            );
            let completed: serde_json::Value =
                serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
            assert_eq!(completed["full_scan_completed"], true);
            fs::remove_dir_all(root).unwrap();
        });
    }

    #[test]
    fn retried_remote_first_restore_keeps_its_incomplete_checkpoint() {
        tauri::async_runtime::block_on(async {
            let root = temp_root("retried-remote-first-restore");
            let state = test_state_root(&root);
            let backend = RecordingBackend::new("retried-remote-first-restore-target");
            write_file(&root.join("draft.md"), b"local original");
            backend.set("draft.md", b"remote conflict");
            backend.set("z-failure.md", b"remote survivor");
            let stale_manifest = serde_json::json!({
                "entries": {
                    "draft.md": {
                        "local_hash": sha256_hex(b"stale local"),
                        "remote_identity": FakeBackend::identity(b"stale remote")
                    },
                    "deleted-before-restore.md": {
                        "local_hash": sha256_hex(b"old local"),
                        "remote_identity": FakeBackend::identity(b"old remote")
                    }
                },
                "target_fingerprint": sha256_hex(b"retried-remote-first-restore-target"),
                "local_identity": root.to_string_lossy(),
                "version": 3,
                "full_scan_completed": true
            });
            write_file(
                &state.join("fake-manifest.json"),
                &serde_json::to_vec_pretty(&stale_manifest).unwrap(),
            );
            backend.fail_download_once("z-failure.md");

            let first_scope = RemoteSyncScope::notes_from_prepared_directory(
                root.clone(),
                crate::storage_capability::open_canonical_directory_nofollow(&root).unwrap(),
                &state,
                "fake-manifest.json",
                Some(root.to_string_lossy().into_owned()),
                None,
            )
            .unwrap();
            let first_error = execute_scoped_remote_sync(&first_scope, &backend)
                .await
                .expect_err("the injected download failure must interrupt restore");
            assert!(
                first_error.contains("recording download failed"),
                "{first_error}"
            );
            let partial: serde_json::Value =
                serde_json::from_slice(&fs::read(state.join("fake-manifest.json")).unwrap())
                    .unwrap();
            assert_eq!(partial["full_scan_completed"], false);
            assert!(partial["entries"].get("draft.md").is_some());
            assert!(partial["entries"]
                .get("deleted-before-restore.md")
                .is_none());
            assert_eq!(remote_conflict_files(&root), 1);
            backend.clear_operations();

            let retry_scope = RemoteSyncScope::notes_from_prepared_directory(
                root.clone(),
                crate::storage_capability::open_canonical_directory_nofollow(&root).unwrap(),
                &state,
                "fake-manifest.json",
                Some(root.to_string_lossy().into_owned()),
                None,
            )
            .unwrap();
            execute_scoped_remote_sync(&retry_scope, &backend)
                .await
                .unwrap();

            let retry_operations = backend.operations();
            assert_eq!(
                retry_operations.first().map(String::as_str),
                Some("download:z-failure.md")
            );
            assert!(!retry_operations.iter().any(|operation| {
                operation == "download:draft.md" || operation.starts_with("delete:")
            }));
            assert_eq!(remote_conflict_files(&root), 1);
            assert_eq!(fs::read(root.join("draft.md")).unwrap(), b"local original");
            assert_eq!(
                backend.files.lock().unwrap().get("draft.md").unwrap(),
                b"remote conflict"
            );
            assert_eq!(
                backend.files.lock().unwrap().get("z-failure.md").unwrap(),
                b"remote survivor"
            );
            fs::remove_dir_all(root).unwrap();
            fs::remove_dir_all(state).unwrap();
        });
    }

    #[test]
    fn remote_first_pair_retry_after_settings_failure_does_not_repeat_completed_notes() {
        tauri::async_runtime::block_on(async {
            let root = temp_root("remote-first-settings-retry-notes");
            let app_data = temp_root("remote-first-settings-retry-app-data");
            let notes_state = app_data.join("sync-state/notes");
            let settings_state = app_data.join("sync-state/settings");
            let notes_backend = RecordingBackend::new("remote-first-settings-retry-notes-target");
            let settings_backend =
                RecordingBackend::new("remote-first-settings-retry-settings-target");
            write_file(&root.join("draft.md"), b"local original");
            notes_backend.set("draft.md", b"remote conflict");

            let settings_scope = RemoteSyncScope::portable_settings(
                &app_data,
                &settings_state,
                "settings-manifest.json",
            )
            .unwrap();
            write_file(&settings_scope.source_root().join("settings.json"), b"{}");
            settings_backend.fail_list_once();

            let first_notes_scope =
                RemoteSyncScope::notes_from_prepared_directory_with_restore_generation(
                    root.clone(),
                    crate::storage_capability::open_canonical_directory_nofollow(&root).unwrap(),
                    &notes_state,
                    "notes-manifest.json",
                    Some(root.to_string_lossy().into_owned()),
                    None,
                    "restore-generation-1".to_string(),
                )
                .unwrap();
            let (first_notes, first_settings) = super::execute_remote_sync_pair(
                &first_notes_scope,
                &notes_backend,
                &settings_scope,
                &settings_backend,
                |_| Ok(()),
            )
            .await;
            assert_eq!(first_notes.unwrap().conflict_files, 1);
            assert_eq!(first_settings.unwrap_err(), "recording list failed");
            assert_eq!(remote_conflict_files(&root), 1);
            notes_backend.set("draft.md", b"remote changed during settings retry");
            notes_backend.clear_operations();

            let retry_notes_scope =
                RemoteSyncScope::notes_from_prepared_directory_with_restore_generation(
                    root.clone(),
                    crate::storage_capability::open_canonical_directory_nofollow(&root).unwrap(),
                    &notes_state,
                    "notes-manifest.json",
                    Some(root.to_string_lossy().into_owned()),
                    None,
                    "restore-generation-2".to_string(),
                )
                .unwrap();
            let retry_settings_scope = RemoteSyncScope::portable_settings(
                &app_data,
                &settings_state,
                "settings-manifest.json",
            )
            .unwrap();
            let (retry_notes, retry_settings) = super::execute_remote_sync_pair(
                &retry_notes_scope,
                &notes_backend,
                &retry_settings_scope,
                &settings_backend,
                |_| Ok(()),
            )
            .await;

            let retry_notes = retry_notes.unwrap();
            assert!(retry_settings.is_ok());
            assert_eq!(retry_notes.conflict_files, 0);
            assert_eq!(retry_notes.uploaded_files, 0);
            assert_eq!(retry_notes.downloaded_files, 1);
            assert_eq!(
                notes_backend.operations(),
                ["download:draft.md".to_string()]
            );
            assert_eq!(
                fs::read(root.join("draft.md")).unwrap(),
                b"remote changed during settings retry"
            );
            assert_eq!(remote_conflict_files(&root), 1);
            assert!(!notes_backend
                .files
                .lock()
                .unwrap()
                .keys()
                .any(|path| path.contains("remote-conflict")));

            fs::remove_dir_all(root).unwrap();
            fs::remove_dir_all(app_data).unwrap();
        });
    }

    #[test]
    fn completed_managed_restore_with_the_same_generation_still_scans_remote_changes() {
        tauri::async_runtime::block_on(async {
            let root = temp_root("managed-restore-same-generation");
            let state = test_state_root(&root);
            let backend = RecordingBackend::new("managed-restore-same-generation-target");
            backend.set("note.md", b"remote first");

            let first_scope =
                RemoteSyncScope::notes_from_managed_bootstrap_with_restore_generation(
                    root.clone(),
                    crate::storage_capability::open_canonical_directory_nofollow(&root).unwrap(),
                    &state,
                    "fake-manifest.json",
                    Some(root.to_string_lossy().into_owned()),
                    None,
                    "managed-directory-generation".to_string(),
                )
                .unwrap();
            execute_scoped_remote_sync(&first_scope, &backend)
                .await
                .unwrap();
            super::complete_remote_first_restore_locked(&first_scope).unwrap();
            backend.set("note.md", b"remote changed while away");
            backend.clear_operations();

            let returning_scope =
                RemoteSyncScope::notes_from_managed_bootstrap_with_restore_generation(
                    root.clone(),
                    crate::storage_capability::open_canonical_directory_nofollow(&root).unwrap(),
                    &state,
                    "fake-manifest.json",
                    Some(root.to_string_lossy().into_owned()),
                    None,
                    "managed-directory-generation".to_string(),
                )
                .unwrap();
            let summary = execute_scoped_remote_sync(&returning_scope, &backend)
                .await
                .unwrap();

            assert_eq!(summary.downloaded_files, 1);
            assert_eq!(backend.operations(), ["download:note.md".to_string()]);
            assert_eq!(
                fs::read(root.join("note.md")).unwrap(),
                b"remote changed while away"
            );
            super::complete_remote_first_restore_locked(&returning_scope).unwrap();

            fs::remove_dir_all(root).unwrap();
            fs::remove_dir_all(state).unwrap();
        });
    }

    #[test]
    fn completed_remote_first_restore_allows_a_new_native_generation_to_clear_the_baseline() {
        tauri::async_runtime::block_on(async {
            let root = temp_root("remote-first-new-generation");
            let state = test_state_root(&root);
            let backend = RecordingBackend::new("remote-first-new-generation-target");
            write_file(&root.join("draft.md"), b"local original");
            write_file(&root.join("same.md"), b"same content");
            backend.set("draft.md", b"remote first");
            backend.set("same.md", b"same content");
            backend.set("remote-survivor.md", b"remote survivor");

            let first_scope =
                RemoteSyncScope::notes_from_prepared_directory_with_restore_generation(
                    root.clone(),
                    crate::storage_capability::open_canonical_directory_nofollow(&root).unwrap(),
                    &state,
                    "fake-manifest.json",
                    Some(root.to_string_lossy().into_owned()),
                    None,
                    "native-generation-1".to_string(),
                )
                .unwrap();
            execute_scoped_remote_sync(&first_scope, &backend)
                .await
                .unwrap();
            super::complete_remote_first_restore_locked(&first_scope).unwrap();
            assert_eq!(remote_conflict_files(&root), 1);
            fs::remove_file(root.join("remote-survivor.md")).unwrap();
            backend.set("draft.md", b"remote second");
            backend.clear_operations();

            let second_scope =
                RemoteSyncScope::notes_from_prepared_directory_with_restore_generation(
                    root.clone(),
                    crate::storage_capability::open_canonical_directory_nofollow(&root).unwrap(),
                    &state,
                    "fake-manifest.json",
                    Some(root.to_string_lossy().into_owned()),
                    None,
                    "native-generation-2".to_string(),
                )
                .unwrap();
            let summary = execute_scoped_remote_sync(&second_scope, &backend)
                .await
                .unwrap();

            assert_eq!(summary.conflict_files, 1);
            assert_eq!(summary.downloaded_files, 1);
            assert_eq!(
                backend.operations().first().map(String::as_str),
                Some("download:draft.md")
            );
            assert!(backend
                .operations()
                .contains(&"download:remote-survivor.md".to_string()));
            assert!(!backend
                .operations()
                .contains(&"delete:remote-survivor.md".to_string()));
            assert_eq!(
                fs::read(root.join("remote-survivor.md")).unwrap(),
                b"remote survivor"
            );
            assert_eq!(remote_conflict_files(&root), 2);
            fs::remove_dir_all(root).unwrap();
            fs::remove_dir_all(state).unwrap();
        });
    }

    #[test]
    fn remote_first_retry_after_baseline_clear_does_not_reaccept_the_stale_baseline() {
        tauri::async_runtime::block_on(async {
            let root = temp_root("remote-first-fails-after-baseline-clear");
            let state = test_state_root(&root);
            let backend = RecordingBackend::new("remote-first-fails-after-baseline-clear-target");
            backend.set("remote-survivor.md", b"remote survivor");
            let stale_manifest = serde_json::json!({
                "entries": {
                    "remote-survivor.md": {
                        "local_hash": sha256_hex(b"old local copy"),
                        "remote_identity": FakeBackend::identity(b"remote survivor")
                    }
                },
                "target_fingerprint": sha256_hex(
                    b"remote-first-fails-after-baseline-clear-target"
                ),
                "local_identity": root.to_string_lossy(),
                "version": 3,
                "full_scan_completed": true
            });
            let manifest_path = state.join("fake-manifest.json");
            write_file(
                &manifest_path,
                &serde_json::to_vec_pretty(&stale_manifest).unwrap(),
            );
            backend.fail_list_once();

            let first_scope = RemoteSyncScope::notes_from_prepared_directory(
                root.clone(),
                crate::storage_capability::open_canonical_directory_nofollow(&root).unwrap(),
                &state,
                "fake-manifest.json",
                Some(root.to_string_lossy().into_owned()),
                None,
            )
            .unwrap();
            let first_error = execute_scoped_remote_sync(&first_scope, &backend)
                .await
                .expect_err("the injected list failure must interrupt restore");
            assert_eq!(first_error, "recording list failed");
            let checkpoint: serde_json::Value =
                serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
            assert_eq!(checkpoint["full_scan_completed"], false);
            assert!(checkpoint["restore_generation"].is_string());
            assert_eq!(checkpoint["restore_generation_completed"], false);
            assert_eq!(checkpoint["entries"], serde_json::json!({}));

            let retry_scope = RemoteSyncScope::notes_from_prepared_directory(
                root.clone(),
                crate::storage_capability::open_canonical_directory_nofollow(&root).unwrap(),
                &state,
                "fake-manifest.json",
                Some(root.to_string_lossy().into_owned()),
                None,
            )
            .unwrap();
            execute_scoped_remote_sync(&retry_scope, &backend)
                .await
                .unwrap();

            assert_eq!(backend.operations(), ["download:remote-survivor.md"]);
            assert_eq!(
                fs::read(root.join("remote-survivor.md")).unwrap(),
                b"remote survivor"
            );
            assert_eq!(
                backend
                    .files
                    .lock()
                    .unwrap()
                    .get("remote-survivor.md")
                    .unwrap(),
                b"remote survivor"
            );
            fs::remove_dir_all(root).unwrap();
            fs::remove_dir_all(state).unwrap();
        });
    }

    #[test]
    fn old_manifest_is_not_deletion_authoritative() {
        tauri::async_runtime::block_on(async {
            let root = temp_root("old-manifest-bootstrap");
            let backend = RecordingBackend::new("old-manifest-bootstrap-target");
            backend.set("remote-survivor.md", b"remote survivor");
            let manifest = serde_json::json!({
                "entries": {
                    "remote-survivor.md": {
                        "local_hash": sha256_hex(b"remote survivor"),
                        "remote_identity": FakeBackend::identity(b"remote survivor")
                    }
                },
                "target_fingerprint": sha256_hex(b"old-manifest-bootstrap-target"),
                "version": 2
            });
            write_file(
                &test_state_root(&root).join("fake-manifest.json"),
                &serde_json::to_vec_pretty(&manifest).unwrap(),
            );

            execute_remote_sync(&root, &backend).await.unwrap();

            assert_eq!(
                fs::read(root.join("remote-survivor.md")).unwrap(),
                b"remote survivor"
            );
            assert_eq!(backend.operations(), ["download:remote-survivor.md"]);
            fs::remove_dir_all(root).unwrap();
        });
    }

    #[test]
    fn old_manifest_entries_do_not_bypass_first_sync_content_comparison() {
        tauri::async_runtime::block_on(async {
            let root = temp_root("old-manifest-content-comparison");
            let backend = RecordingBackend::new("old-manifest-content-comparison-target");
            write_file(&root.join("same.md"), b"same bytes");
            backend.set("same.md", b"same bytes");
            let manifest = serde_json::json!({
                "entries": {
                    "same.md": {
                        "local_hash": sha256_hex(b"same bytes"),
                        "remote_identity": FakeBackend::identity(b"same bytes")
                    }
                },
                "target_fingerprint": sha256_hex(b"old-manifest-content-comparison-target"),
                "version": 2
            });
            write_file(
                &test_state_root(&root).join("fake-manifest.json"),
                &serde_json::to_vec_pretty(&manifest).unwrap(),
            );

            let summary = execute_remote_sync(&root, &backend).await.unwrap();

            assert_eq!(summary.skipped_files, 1);
            assert_eq!(backend.operations(), ["download:same.md"]);
            fs::remove_dir_all(root).unwrap();
        });
    }

    #[test]
    fn incomplete_settings_manifest_does_not_delete_a_local_only_file() {
        tauri::async_runtime::block_on(async {
            let app_data = temp_root("incomplete-settings-local-only");
            let state = app_data.join("sync-state");
            let backend = RecordingBackend::new("incomplete-settings-local-only-target");
            let settings = br#"{"appearanceMode":"dark"}"#;
            let manifest = serde_json::json!({
                "entries": {
                    "settings.json": {
                        "local_hash": sha256_hex(settings),
                        "remote_identity": FakeBackend::identity(settings)
                    }
                },
                "target_fingerprint": sha256_hex(b"incomplete-settings-local-only-target"),
                "version": 3,
                "full_scan_completed": false
            });
            let scope = RemoteSyncScope::portable_settings(
                &app_data,
                &state,
                "settings-fake-manifest.json",
            )
            .unwrap();
            write_file(&scope.source_root().join("settings.json"), settings);
            write_file(
                &scope.state_root().join("settings-fake-manifest.json"),
                &serde_json::to_vec_pretty(&manifest).unwrap(),
            );

            execute_scoped_remote_sync(&scope, &backend).await.unwrap();

            assert_eq!(
                fs::read(scope.source_root().join("settings.json")).unwrap(),
                settings
            );
            assert_eq!(backend.operations(), ["upload:settings.json"]);
            fs::remove_dir_all(app_data).unwrap();
        });
    }

    #[test]
    fn incomplete_settings_manifest_does_not_delete_a_remote_only_file() {
        tauri::async_runtime::block_on(async {
            let app_data = temp_root("incomplete-settings-remote-only");
            let state = app_data.join("sync-state");
            let backend = RecordingBackend::new("incomplete-settings-remote-only-target");
            let settings = br#"{"appearanceMode":"dark"}"#;
            backend.set("settings.json", settings);
            let manifest = serde_json::json!({
                "entries": {
                    "settings.json": {
                        "local_hash": sha256_hex(settings),
                        "remote_identity": FakeBackend::identity(settings)
                    }
                },
                "target_fingerprint": sha256_hex(b"incomplete-settings-remote-only-target"),
                "version": 3,
                "full_scan_completed": false
            });
            let scope = RemoteSyncScope::portable_settings(
                &app_data,
                &state,
                "settings-fake-manifest.json",
            )
            .unwrap();
            write_file(
                &scope.state_root().join("settings-fake-manifest.json"),
                &serde_json::to_vec_pretty(&manifest).unwrap(),
            );

            execute_scoped_remote_sync(&scope, &backend).await.unwrap();

            assert_eq!(
                fs::read(scope.source_root().join("settings.json")).unwrap(),
                settings
            );
            assert_eq!(backend.operations(), ["download:settings.json"]);
            fs::remove_dir_all(app_data).unwrap();
        });
    }

    fn baseline() -> SyncManifestEntry {
        SyncManifestEntry {
            local_hash: "local-old".to_string(),
            remote_identity: "remote-old".to_string(),
        }
    }

    #[test]
    fn plans_upload_download_skip_and_conflict() {
        assert_eq!(
            plan_file_sync(Some("local"), None, None),
            FileSyncAction::Upload
        );
        assert_eq!(
            plan_file_sync(None, Some("remote"), None),
            FileSyncAction::Download
        );
        assert_eq!(
            plan_file_sync(Some("local-old"), Some("remote-old"), Some(&baseline())),
            FileSyncAction::Skip
        );
        assert_eq!(
            plan_file_sync(Some("local-new"), Some("remote-new"), Some(&baseline())),
            FileSyncAction::Conflict
        );
    }

    #[test]
    fn preserves_changed_survivor_and_propagates_unchanged_deletion() {
        assert_eq!(
            plan_file_sync(None, Some("remote-old"), Some(&baseline())),
            FileSyncAction::DeleteRemote
        );
        assert_eq!(
            plan_file_sync(Some("local-old"), None, Some(&baseline())),
            FileSyncAction::DeleteLocal
        );
        assert_eq!(
            plan_file_sync(None, Some("remote-new"), Some(&baseline())),
            FileSyncAction::Download
        );
        assert_eq!(
            plan_file_sync(Some("local-new"), None, Some(&baseline())),
            FileSyncAction::Upload
        );
    }

    #[test]
    fn rejects_unsafe_relative_paths() {
        for path in [
            "",
            "/note.md",
            "C:/note.md",
            "\\\\server\\share\\note.md",
            "../note.md",
            "a/../note.md",
            "a\\note.md",
            "a//b",
            "note\0.md",
        ] {
            assert!(validate_relative_path(path).is_err(), "{path}");
        }
        assert!(validate_relative_path("notes/安全.md").is_ok());
    }

    #[test]
    fn exact_file_scope_rejects_remote_traversal_before_allowlist_filtering() {
        tauri::async_runtime::block_on(async {
            let app_data = temp_root("settings-remote-traversal");
            let scope = RemoteSyncScope::portable_settings(
                &app_data,
                app_data.join("sync-state"),
                "settings-fake-manifest.json",
            )
            .unwrap();
            let backend = FakeBackend::new("settings-remote-traversal-target");
            backend.set("../sync-config.json", b"secret");

            let error = execute_scoped_remote_sync(&scope, &backend)
                .await
                .expect_err("remote traversal must fail before exact-file filtering");

            assert!(error.contains("unsafe"), "{error}");
            assert!(!app_data.join("sync-config.json").exists());
            fs::remove_dir_all(app_data).unwrap();
        });
    }

    #[cfg(unix)]
    #[test]
    fn stale_publication_cleanup_only_removes_expired_strict_engine_files() {
        tauri::async_runtime::block_on(async {
            let source = temp_root("stale-publication-source");
            let state = temp_root("stale-publication-state");
            let outside = temp_root("stale-publication-outside");
            let backend = FakeBackend::new("stale-publication-target");
            let created = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let fresh_same_prefix = source.join(format!(
                ".markra-sync-stage-publish-v1-{}-{created}-00000000000000000000000000000001-6",
                std::process::id()
            ));
            let stale = source
                .join(".markra-sync-stage-publish-v1-999999-0-00000000000000000000000000000001-7");
            let outside_file = outside.join("outside.txt");
            write_file(&fresh_same_prefix, b"user bytes");
            write_file(&stale, b"stale remote bytes");
            write_file(&outside_file, b"outside unchanged");
            let nested = source.join("nested");
            fs::create_dir_all(&nested).unwrap();
            let stale_link = nested
                .join(".markra-sync-stage-publish-v1-999999-0-00000000000000000000000000000001-8");
            symlink(&outside_file, &stale_link).unwrap();
            let scope =
                RemoteSyncScope::notes(&source, &state, "notes-fake-manifest.json", None, None)
                    .unwrap();

            execute_scoped_remote_sync(&scope, &backend).await.unwrap();

            assert!(!stale.exists());
            assert!(fresh_same_prefix.exists());
            assert!(stale_link.symlink_metadata().is_ok());
            assert_eq!(fs::read(outside_file).unwrap(), b"outside unchanged");
            assert!(backend.files.lock().unwrap().is_empty());
            fs::remove_dir_all(source).unwrap();
            fs::remove_dir_all(state).unwrap();
            fs::remove_dir_all(outside).unwrap();
        });
    }

    #[cfg(unix)]
    #[test]
    fn stale_state_cleanup_preserves_fresh_same_prefix_and_symlinks() {
        tauri::async_runtime::block_on(async {
            let source = temp_root("stale-state-source");
            let state = temp_root("stale-state-root");
            let outside = temp_root("stale-state-outside");
            let backend = FakeBackend::new("stale-state-target");
            let staging = state.join("staging");
            fs::create_dir(&staging).unwrap();
            let created = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let fresh_same_prefix = staging.join(format!(
                ".markra-sync-stage-state-v1-{}-{created}-00000000000000000000000000000001-6",
                std::process::id()
            ));
            let expired = staging
                .join(".markra-sync-stage-state-v1-999999-0-00000000000000000000000000000001-7");
            let expired_link = staging
                .join(".markra-sync-stage-state-v1-999999-0-00000000000000000000000000000001-8");
            fs::create_dir(&fresh_same_prefix).unwrap();
            fs::create_dir(&expired).unwrap();
            symlink(&outside, &expired_link).unwrap();
            let scope =
                RemoteSyncScope::notes(&source, &state, "notes-fake-manifest.json", None, None)
                    .unwrap();

            execute_scoped_remote_sync(&scope, &backend).await.unwrap();

            assert!(fresh_same_prefix.is_dir());
            assert!(!expired.exists());
            assert!(expired_link.symlink_metadata().is_ok());
            fs::remove_dir_all(source).unwrap();
            fs::remove_dir_all(state).unwrap();
            fs::remove_dir_all(outside).unwrap();
        });
    }

    #[test]
    fn rejects_a_file_as_project_sync_root() {
        let root = temp_root("file-root");
        let note = root.join("note.md");
        fs::write(&note, "# Note").unwrap();

        assert_eq!(
            RemoteSyncScope::notes(
                &note,
                test_state_root(&note),
                "fake-manifest.json",
                None,
                None,
            )
            .unwrap_err(),
            "Remote sync source must be a folder"
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn staging_names_use_distinct_strict_128_bit_lower_hex_run_nonces() {
        let source = temp_root("staging-nonce-source");
        let state = temp_root("staging-nonce-state");
        let first =
            RemoteSyncScope::notes(&source, &state, "first-manifest.json", None, None).unwrap();
        let second =
            RemoteSyncScope::notes(&source, &state, "second-manifest.json", None, None).unwrap();

        let first_nonce = strict_staging_nonce(&first.state_staging_name(7), "state", 7);
        let second_nonce = strict_staging_nonce(&second.publication_temp_name(11), "publish", 11);

        assert_ne!(first_nonce, second_nonce);
        fs::remove_dir_all(source).unwrap();
        fs::remove_dir_all(state).unwrap();
    }

    #[test]
    fn keeps_local_and_remote_control_paths_untouched() {
        tauri::async_runtime::block_on(async {
            let root = temp_root("protected-paths");
            let backend = FakeBackend::new("fake-target-protected");
            write_file(&root.join("note.md"), b"note");
            write_file(&root.join(".qingyu/local.json"), b"local-qingyu");
            write_file(
                &root.join(".markra-sync/fake-manifest.json"),
                b"{legacy-malformed",
            );
            backend.set(".qingyu/remote.json", b"remote-qingyu");
            backend.set(".markra-sync/legacy-remote.json", b"remote-legacy");

            execute_remote_sync(&root, &backend).await.unwrap();

            assert_eq!(
                fs::read(root.join(".qingyu/local.json")).unwrap(),
                b"local-qingyu"
            );
            assert_eq!(
                fs::read(root.join(".markra-sync/fake-manifest.json")).unwrap(),
                b"{legacy-malformed"
            );
            assert_eq!(
                backend
                    .files
                    .lock()
                    .unwrap()
                    .get(".qingyu/remote.json")
                    .unwrap(),
                b"remote-qingyu"
            );
            assert_eq!(
                backend
                    .files
                    .lock()
                    .unwrap()
                    .get(".markra-sync/legacy-remote.json")
                    .unwrap(),
                b"remote-legacy"
            );
            assert_eq!(
                backend.files.lock().unwrap().get("note.md").unwrap(),
                b"note"
            );
            assert!(test_state_root(&root).join("fake-manifest.json").is_file());

            fs::remove_dir_all(root).unwrap();
        });
    }

    #[test]
    fn keeps_ascii_case_variant_control_paths_out_of_project_sync() {
        tauri::async_runtime::block_on(async {
            let root = temp_root("protected-case-variant-paths");
            let backend = FakeBackend::new("fake-target-protected-case-variants");
            write_file(&root.join("visible.md"), b"visible");
            write_file(&root.join(".QINGYU/config.json"), b"private-config");
            write_file(
                &root.join(".MARKRA-SYNC/private-manifest.json"),
                b"private-manifest",
            );
            backend.set(".QiNgYu/remote.json", b"remote-private");
            backend.set(".MaRkRa-SyNc/legacy.json", b"remote-legacy");

            execute_remote_sync(&root, &backend).await.unwrap();

            let remote_files = backend.files.lock().unwrap();
            assert_eq!(remote_files.get("visible.md").unwrap(), b"visible");
            assert!(!remote_files.contains_key(".QINGYU/config.json"));
            assert!(!remote_files.contains_key(".MARKRA-SYNC/private-manifest.json"));
            assert_eq!(
                remote_files.get(".QiNgYu/remote.json").unwrap(),
                b"remote-private"
            );
            assert_eq!(
                remote_files.get(".MaRkRa-SyNc/legacy.json").unwrap(),
                b"remote-legacy"
            );
            drop(remote_files);
            assert!(!root.join(".QiNgYu/remote.json").exists());
            assert!(!root.join(".MaRkRa-SyNc/legacy.json").exists());

            fs::remove_dir_all(root).unwrap();
        });
    }

    #[test]
    fn keeps_crash_left_staging_prefixes_out_of_the_next_sync() {
        tauri::async_runtime::block_on(async {
            let root = temp_root("protected-crash-left-staging");
            let backend = FakeBackend::new("protected-crash-left-staging-target");
            write_file(&root.join("visible.md"), b"visible");
            write_file(
                &root.join(".MaRkRa-SyNc-StAgE-crash/target"),
                b"legacy-staged-secret",
            );
            write_file(
                &root.join(".qingyu/sync/.markra-sync-stage-crash/target"),
                b"protected-staged-secret",
            );

            execute_remote_sync(&root, &backend).await.unwrap();

            let remote_files = backend.files.lock().unwrap();
            assert_eq!(remote_files.get("visible.md").unwrap(), b"visible");
            assert_eq!(remote_files.len(), 1, "staging leaked: {remote_files:?}");
            drop(remote_files);

            fs::remove_dir_all(root).unwrap();
        });
    }

    #[test]
    fn executes_upload_download_conflict_and_delete_flows() {
        tauri::async_runtime::block_on(async {
            let root = temp_root("generic-engine");
            let backend = FakeBackend::new("fake-target-a");
            write_file(&root.join("draft.md"), b"local-v1");

            let uploaded = execute_remote_sync(&root, &backend).await.unwrap();
            assert_eq!(uploaded.uploaded_files, 1);
            assert!(test_state_root(&root).join("fake-manifest.json").is_file());
            assert!(!root.join(".markra-sync/fake-manifest.json").exists());
            assert_eq!(
                backend.files.lock().unwrap().get("draft.md").unwrap(),
                b"local-v1"
            );

            backend.set("draft.md", b"remote-v2");
            let downloaded = execute_remote_sync(&root, &backend).await.unwrap();
            assert_eq!(downloaded.downloaded_files, 1);
            assert_eq!(fs::read(root.join("draft.md")).unwrap(), b"remote-v2");

            write_file(&root.join("draft.md"), b"local-v3");
            backend.set("draft.md", b"remote-v3");
            let conflict = execute_remote_sync(&root, &backend).await.unwrap();
            assert_eq!(conflict.conflict_files, 1);
            assert_eq!(fs::read(root.join("draft.md")).unwrap(), b"local-v3");
            assert!(find_conflict_file(&root).is_some());

            execute_remote_sync(&root, &backend).await.unwrap();
            fs::remove_file(root.join("draft.md")).unwrap();
            execute_remote_sync(&root, &backend).await.unwrap();
            assert!(!backend.files.lock().unwrap().contains_key("draft.md"));

            write_file(&root.join("remote-delete.md"), b"same");
            execute_remote_sync(&root, &backend).await.unwrap();
            backend.remove("remote-delete.md");
            execute_remote_sync(&root, &backend).await.unwrap();
            assert!(!root.join("remote-delete.md").exists());
            assert_no_sync_mutation_artifacts(&root);

            fs::remove_dir_all(root).unwrap();
        });
    }

    #[test]
    fn replans_regular_file_edits_during_upload() {
        tauri::async_runtime::block_on(async {
            let root = temp_root("upload-concurrent-regular-edit");
            let note = root.join("note.md");
            let backend = FakeBackend::new("upload-concurrent-regular-edit-target");
            write_file(&note, b"first snapshot");

            let hook_calls = Arc::new(AtomicUsize::new(0));
            let hook_counter = Arc::clone(&hook_calls);
            let note_for_hook = note.clone();
            let hooks = super::RemoteSyncExecutionHooks {
                upload_validated: Some(Box::new(move |_| {
                    if hook_counter.fetch_add(1, Ordering::SeqCst) == 0 {
                        fs::write(&note_for_hook, b"saved while syncing")
                            .map_err(|error| error.to_string())?;
                    }
                    Ok(())
                })),
                ..Default::default()
            };

            let summary = execute_remote_sync_with_hooks(&root, &backend, hooks)
                .await
                .expect("a regular edit should be re-planned instead of failing the run");

            assert_eq!(summary.uploaded_files, 1);
            assert!(hook_calls.load(Ordering::SeqCst) >= 2);
            assert_eq!(
                backend.files.lock().unwrap().get("note.md").unwrap(),
                b"saved while syncing"
            );
            assert_eq!(fs::read(&note).unwrap(), b"saved while syncing");

            fs::remove_dir_all(root).unwrap();
        });
    }

    #[test]
    fn replans_new_files_created_during_a_pass() {
        tauri::async_runtime::block_on(async {
            let root = temp_root("upload-concurrent-create");
            let first_note = root.join("first.md");
            let second_note = root.join("second.md");
            let backend = FakeBackend::new("upload-concurrent-create-target");
            write_file(&first_note, b"first note");

            let hook_calls = Arc::new(AtomicUsize::new(0));
            let hook_counter = Arc::clone(&hook_calls);
            let second_note_for_hook = second_note.clone();
            let hooks = super::RemoteSyncExecutionHooks {
                upload_validated: Some(Box::new(move |_| {
                    if hook_counter.fetch_add(1, Ordering::SeqCst) == 0 {
                        fs::write(&second_note_for_hook, b"created while syncing")
                            .map_err(|error| error.to_string())?;
                    }
                    Ok(())
                })),
                ..Default::default()
            };

            let summary = execute_remote_sync_with_hooks(&root, &backend, hooks)
                .await
                .expect("a newly created regular file should be included by a fresh pass");

            assert_eq!(summary.uploaded_files, 2);
            assert_eq!(summary.scanned_files, 2);
            assert_eq!(backend.get("first.md").unwrap(), b"first note");
            assert_eq!(backend.get("second.md").unwrap(), b"created while syncing");
            assert_eq!(fs::read(second_note).unwrap(), b"created while syncing");

            fs::remove_dir_all(root).unwrap();
        });
    }

    #[test]
    fn stops_after_the_bounded_number_of_unstable_snapshot_passes() {
        tauri::async_runtime::block_on(async {
            let root = temp_root("upload-continuously-changing");
            let note = root.join("note.md");
            let backend = FakeBackend::new("upload-continuously-changing-target");
            write_file(&note, b"initial snapshot");

            let hook_calls = Arc::new(AtomicUsize::new(0));
            let hook_counter = Arc::clone(&hook_calls);
            let note_for_hook = note.clone();
            let hooks = super::RemoteSyncExecutionHooks {
                upload_validated: Some(Box::new(move |_| {
                    let edit = hook_counter.fetch_add(1, Ordering::SeqCst) + 1;
                    fs::write(&note_for_hook, format!("continuous edit {edit}"))
                        .map_err(|error| error.to_string())
                })),
                ..Default::default()
            };

            let error = execute_remote_sync_with_hooks(&root, &backend, hooks)
                .await
                .expect_err("a continuously changing file must not keep one run alive forever");

            assert!(error.contains("changed"), "{error}");
            assert_eq!(hook_calls.load(Ordering::SeqCst), 3);
            assert!(backend.files.lock().unwrap().is_empty());

            fs::remove_dir_all(root).unwrap();
        });
    }

    #[test]
    fn replans_when_a_file_disappears_during_the_initial_snapshot() {
        tauri::async_runtime::block_on(async {
            let root = temp_root("snapshot-concurrent-delete");
            let note = root.join("note.md");
            let backend = FakeBackend::new("snapshot-concurrent-delete-target");
            write_file(&note, b"being saved");

            let hook_calls = Arc::new(AtomicUsize::new(0));
            let hook_counter = Arc::clone(&hook_calls);
            let hooks = super::RemoteSyncExecutionHooks {
                snapshot_entry: Some(Box::new(move |path| {
                    if hook_counter.fetch_add(1, Ordering::SeqCst) == 0 {
                        fs::remove_file(path).map_err(|error| error.to_string())?;
                    }
                    Ok(())
                })),
                ..Default::default()
            };

            let summary = execute_remote_sync_with_hooks(&root, &backend, hooks)
                .await
                .expect("a disappearing regular file should trigger a fresh snapshot");

            assert_eq!(summary.scanned_files, 0);
            assert!(!note.exists());
            assert!(backend.files.lock().unwrap().is_empty());

            fs::remove_dir_all(root).unwrap();
        });
    }

    #[test]
    fn replans_a_remote_identity_change_before_upload() {
        tauri::async_runtime::block_on(async {
            let root = temp_root("upload-concurrent-remote-edit");
            let note = root.join("note.md");
            let backend = FakeBackend::new("upload-concurrent-remote-edit-target");
            write_file(&note, b"baseline");
            execute_remote_sync(&root, &backend).await.unwrap();

            write_file(&note, b"local edit");
            backend.replace_on_next_upload(b"remote edit");
            let summary = execute_remote_sync(&root, &backend)
                .await
                .expect("a stale remote upload plan should be rebuilt from fresh identities");

            assert_eq!(summary.conflict_files, 1);
            assert_eq!(fs::read(&note).unwrap(), b"local edit");
            assert_eq!(backend.get("note.md").unwrap(), b"remote edit");
            assert_eq!(
                fs::read(find_conflict_file(&root).expect("remote conflict copy")).unwrap(),
                b"remote edit"
            );

            fs::remove_dir_all(root).unwrap();
        });
    }

    #[test]
    fn replans_a_remote_identity_change_before_download() {
        tauri::async_runtime::block_on(async {
            let root = temp_root("download-concurrent-remote-edit");
            let note = root.join("note.md");
            let backend = FakeBackend::new("download-concurrent-remote-edit-target");
            write_file(&note, b"baseline");
            execute_remote_sync(&root, &backend).await.unwrap();

            backend.set("note.md", b"remote listed bytes");
            backend.replace_on_next_download(b"remote final bytes");
            let summary = execute_remote_sync(&root, &backend)
                .await
                .expect("a stale remote download plan should be rebuilt from fresh identities");

            assert_eq!(summary.downloaded_files, 1);
            assert_eq!(summary.conflict_files, 0);
            assert_eq!(fs::read(&note).unwrap(), b"remote final bytes");
            assert_eq!(backend.get("note.md").unwrap(), b"remote final bytes");

            fs::remove_dir_all(root).unwrap();
        });
    }

    #[test]
    fn replans_a_remote_identity_change_before_conflict_download() {
        tauri::async_runtime::block_on(async {
            let root = temp_root("conflict-concurrent-remote-edit");
            let note = root.join("note.md");
            let backend = FakeBackend::new("conflict-concurrent-remote-edit-target");
            write_file(&note, b"baseline");
            execute_remote_sync(&root, &backend).await.unwrap();

            write_file(&note, b"local edit");
            backend.set("note.md", b"remote listed bytes");
            backend.replace_on_next_download(b"remote final bytes");
            let summary = execute_remote_sync(&root, &backend)
                .await
                .expect("a stale conflict download should be rebuilt from fresh identities");

            assert_eq!(summary.downloaded_files, 0);
            assert_eq!(summary.conflict_files, 1);
            assert_eq!(fs::read(&note).unwrap(), b"local edit");
            assert_eq!(backend.get("note.md").unwrap(), b"remote final bytes");
            assert_eq!(
                fs::read(find_conflict_file(&root).expect("remote conflict copy")).unwrap(),
                b"remote final bytes"
            );

            fs::remove_dir_all(root).unwrap();
        });
    }

    #[test]
    fn replans_a_remote_identity_change_before_delete() {
        tauri::async_runtime::block_on(async {
            let root = temp_root("delete-concurrent-remote-edit");
            let note = root.join("note.md");
            let backend = FakeBackend::new("delete-concurrent-remote-edit-target");
            write_file(&note, b"baseline");
            execute_remote_sync(&root, &backend).await.unwrap();

            fs::remove_file(&note).unwrap();
            backend.replace_on_next_delete(b"remote survivor");
            let summary = execute_remote_sync(&root, &backend)
                .await
                .expect("a stale remote delete plan should preserve a newer remote survivor");

            assert_eq!(summary.downloaded_files, 1);
            assert_eq!(summary.conflict_files, 0);
            assert_eq!(fs::read(&note).unwrap(), b"remote survivor");
            assert_eq!(backend.get("note.md").unwrap(), b"remote survivor");

            fs::remove_dir_all(root).unwrap();
        });
    }

    #[test]
    fn ambiguous_upload_completion_converges_when_remote_bytes_match_local() {
        tauri::async_runtime::block_on(async {
            let root = temp_root("upload-ambiguous-completion");
            let note = root.join("note.md");
            let backend = FakeBackend::new("upload-ambiguous-completion-target");
            write_file(&note, b"baseline");
            execute_remote_sync(&root, &backend).await.unwrap();

            write_file(&note, b"same final bytes");
            backend.replace_on_next_upload(b"same final bytes");
            let summary = execute_remote_sync(&root, &backend)
                .await
                .expect("an ambiguous completed upload should converge without a conflict copy");

            assert_eq!(summary.conflict_files, 0);
            assert_eq!(summary.skipped_files, 1);
            assert_eq!(fs::read(&note).unwrap(), b"same final bytes");
            assert_eq!(backend.get("note.md").unwrap(), b"same final bytes");
            assert!(find_conflict_file(&root).is_none());

            fs::remove_dir_all(root).unwrap();
        });
    }

    #[test]
    fn download_publication_does_not_require_android_blocked_hard_links() {
        let source = include_str!("engine.rs");
        let publication = source
            .split_once("fn write_download_atomically(")
            .and_then(|(_, remainder)| {
                remainder
                    .split_once("fn delete_local_file(")
                    .map(|part| part.0)
            })
            .expect("download publication source should be present");

        assert!(!publication.contains(".hard_link("));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_upload_when_validated_note_becomes_a_secret_symlink() {
        tauri::async_runtime::block_on(async {
            let root = temp_root("upload-validated-symlink-race");
            let outside = temp_root("upload-validated-symlink-race-outside");
            let note = root.join("note.md");
            let outside_secret = outside.join("secret.txt");
            let backend = FakeBackend::new("upload-validated-symlink-race-target");
            write_file(&note, b"local-new");
            write_file(&outside_secret, b"outside-secret");
            backend.set("note.md", b"remote-old");
            let manifest_path = test_state_root(&root).join("fake-manifest.json");
            let manifest_bytes = format!(
                "{{\n  \"entries\": {{\n    \"note.md\": {{\n      \"local_hash\": \"{}\",\n      \"remote_identity\": \"{}\"\n    }}\n  }},\n  \"target_fingerprint\": \"{}\",\n  \"version\": 3,\n  \"full_scan_completed\": true\n}}",
                sha256_hex(b"local-old"),
                FakeBackend::identity(b"remote-old"),
                sha256_hex(b"upload-validated-symlink-race-target")
            )
            .into_bytes();
            write_file(&manifest_path, &manifest_bytes);

            let note_for_hook = note.clone();
            let secret_for_hook = outside_secret.clone();
            let hooks = super::RemoteSyncExecutionHooks {
                upload_validated: Some(Box::new(move |_| {
                    fs::remove_file(&note_for_hook).map_err(|error| error.to_string())?;
                    symlink(&secret_for_hook, &note_for_hook).map_err(|error| error.to_string())
                })),
                ..Default::default()
            };
            let result = execute_remote_sync_with_hooks(&root, &backend, hooks).await;
            let error = result.expect_err("a replaced upload source must fail closed");

            assert!(
                error.contains("unsafe") || error.contains("changed"),
                "{error}"
            );
            assert_eq!(
                backend.files.lock().unwrap().get("note.md").unwrap(),
                b"remote-old"
            );
            assert_eq!(fs::read(&manifest_path).unwrap(), manifest_bytes);
            assert!(note.symlink_metadata().unwrap().file_type().is_symlink());
            assert_eq!(fs::read(&outside_secret).unwrap(), b"outside-secret");

            fs::remove_file(note).unwrap();
            fs::remove_dir_all(root).unwrap();
            fs::remove_dir_all(outside).unwrap();
        });
    }

    #[cfg(unix)]
    #[test]
    fn rejects_remote_download_through_symlink_escape_ancestor() {
        tauri::async_runtime::block_on(async {
            let root = temp_root("download-symlink-ancestor");
            let outside = temp_root("download-symlink-ancestor-outside");
            let backend = FakeBackend::new("download-symlink-ancestor-target");
            symlink(&outside, root.join("link")).unwrap();
            backend.set("link/file.md", b"remote-private");

            let error = execute_remote_sync(&root, &backend)
                .await
                .expect_err("download through a symlink ancestor must fail");

            assert!(error.contains("unsafe"), "{error}");
            assert!(!outside.join("file.md").exists());
            assert!(!test_state_root(&root).join("fake-manifest.json").exists());
            assert!(root
                .join("link")
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink());

            fs::remove_file(root.join("link")).unwrap();
            fs::remove_dir_all(root).unwrap();
            fs::remove_dir_all(outside).unwrap();
        });
    }

    #[cfg(unix)]
    #[test]
    fn rejects_remote_download_onto_dangling_symlink_escape_target() {
        tauri::async_runtime::block_on(async {
            let root = temp_root("download-dangling-symlink-target");
            let outside = temp_root("download-dangling-symlink-target-outside");
            let outside_target = outside.join("missing.md");
            let backend = FakeBackend::new("download-dangling-symlink-target-backend");
            symlink(&outside_target, root.join("note.md")).unwrap();
            backend.set("note.md", b"remote-private");

            let error = execute_remote_sync(&root, &backend)
                .await
                .expect_err("download must not replace a dangling symlink target");

            assert!(error.contains("unsafe"), "{error}");
            assert!(!outside_target.exists());
            assert!(root
                .join("note.md")
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink());
            assert!(!test_state_root(&root).join("fake-manifest.json").exists());

            fs::remove_file(root.join("note.md")).unwrap();
            fs::remove_dir_all(root).unwrap();
            fs::remove_dir_all(outside).unwrap();
        });
    }

    #[cfg(unix)]
    #[test]
    fn rejects_local_delete_after_symlink_escape_replacement() {
        let root = temp_root("delete-symlink-replacement");
        let outside = temp_root("delete-symlink-replacement-outside");
        let outside_target = outside.join("outside.md");
        write_file(&outside_target, b"same-bytes");
        write_file(&root.join("note.md"), b"same-bytes");
        let local_files = super::collect_local_sync_files(
            &test_scope(&root, "fake-manifest.json"),
            &super::RemoteSyncExecutionHooks::default(),
        )
        .unwrap();
        let local = local_files.get("note.md").unwrap();
        let expected_hash = local.hash.clone();
        let expected_identity = local.identity;
        fs::remove_file(root.join("note.md")).unwrap();
        symlink(&outside_target, root.join("note.md")).unwrap();

        let error = super::delete_local_file(
            &test_scope(&root, "fake-manifest.json"),
            "note.md",
            &expected_hash,
            expected_identity,
            &super::RemoteSyncExecutionHooks::default(),
        )
        .expect_err("delete must reject a symlink replacement");

        assert!(error.contains("unsafe"), "{error}");
        assert_eq!(fs::read(&outside_target).unwrap(), b"same-bytes");
        assert!(root
            .join("note.md")
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink());

        fs::remove_file(root.join("note.md")).unwrap();
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn replans_regular_replacement_after_final_check_without_clobbering_it() {
        tauri::async_runtime::block_on(async {
            let root = temp_root("replace-final-check-race");
            let note = root.join("note.md");
            let backend = FakeBackend::new("replace-final-check-race-target");
            write_file(&note, b"local-old");
            backend.set("note.md", b"remote-new");
            let (manifest_path, _) = seed_fake_manifest(
                &root,
                "replace-final-check-race-target",
                b"local-old",
                b"remote-old",
            );

            let note_for_hook = note.clone();
            let hook_calls = Arc::new(AtomicUsize::new(0));
            let hook_counter = Arc::clone(&hook_calls);
            let hooks = super::RemoteSyncExecutionHooks {
                final_replace: Some(Box::new(move |_| {
                    if hook_counter.fetch_add(1, Ordering::SeqCst) == 0 {
                        fs::remove_file(&note_for_hook).map_err(|error| error.to_string())?;
                        fs::write(&note_for_hook, b"user-replacement")
                            .map_err(|error| error.to_string())?;
                    }
                    Ok(())
                })),
                ..Default::default()
            };
            let summary = execute_remote_sync_with_hooks(&root, &backend, hooks)
                .await
                .expect("a regular replacement should be re-planned safely");

            assert_eq!(summary.conflict_files, 1);
            assert_eq!(fs::read(&note).unwrap(), b"user-replacement");
            assert_eq!(
                fs::read(find_conflict_file(&root).expect("remote conflict copy")).unwrap(),
                b"remote-new"
            );
            assert_eq!(
                backend.files.lock().unwrap().get("note.md").unwrap(),
                b"remote-new"
            );
            assert!(manifest_path.exists());
            assert_no_sync_mutation_artifacts(&root);

            fs::remove_dir_all(root).unwrap();
        });
    }

    #[test]
    fn rejects_remote_create_when_name_appears_at_final_publish_and_cleans_staging() {
        tauri::async_runtime::block_on(async {
            let root = temp_root("create-final-publish-race");
            let target = root.join("blocked.md");
            let backend = FakeBackend::new("create-final-publish-race-target");
            backend.set("blocked.md", b"remote-download");

            let hooks = super::RemoteSyncExecutionHooks {
                final_replace: Some(Box::new(move |target_path| {
                    fs::create_dir(target_path).map_err(|error| error.to_string())
                })),
                ..Default::default()
            };
            let result = execute_remote_sync_with_hooks(&root, &backend, hooks).await;
            let error = result.expect_err("a final-publish create race must fail closed");

            assert!(error.contains("atomic publish"), "{error}");
            assert!(target.is_dir());
            assert_eq!(
                backend.files.lock().unwrap().get("blocked.md").unwrap(),
                b"remote-download"
            );
            assert!(!test_state_root(&root).join("fake-manifest.json").exists());
            assert_no_sync_mutation_artifacts(&root);

            fs::remove_dir_all(root).unwrap();
        });
    }

    #[cfg(unix)]
    #[test]
    fn restores_directory_replacement_after_final_check_without_exposing_staging() {
        tauri::async_runtime::block_on(async {
            let root = temp_root("replace-final-check-directory-race");
            let outside = temp_root("replace-final-check-directory-race-outside");
            let note = root.join("note.md");
            let outside_file = outside.join("outside.txt");
            let backend = FakeBackend::new("replace-final-check-directory-race-target");
            write_file(&note, b"local-old");
            write_file(&outside_file, b"outside-unchanged");
            backend.set("note.md", b"remote-new");
            let (manifest_path, manifest_bytes) = seed_fake_manifest(
                &root,
                "replace-final-check-directory-race-target",
                b"local-old",
                b"remote-old",
            );

            let note_for_hook = note.clone();
            let hooks = super::RemoteSyncExecutionHooks {
                final_replace: Some(Box::new(move |_| {
                    fs::remove_file(&note_for_hook).map_err(|error| error.to_string())?;
                    fs::create_dir(&note_for_hook).map_err(|error| error.to_string())?;
                    fs::write(note_for_hook.join("user.txt"), b"directory-replacement")
                        .map_err(|error| error.to_string())
                })),
                ..Default::default()
            };
            let result = execute_remote_sync_with_hooks(&root, &backend, hooks).await;
            let error = result.expect_err("a directory replacement race must fail closed");

            assert!(
                error.contains("unsafe") || error.contains("changed"),
                "{error}"
            );
            assert_eq!(
                fs::read(note.join("user.txt")).unwrap(),
                b"directory-replacement"
            );
            assert_eq!(
                backend.files.lock().unwrap().get("note.md").unwrap(),
                b"remote-new"
            );
            assert_eq!(fs::read(&manifest_path).unwrap(), manifest_bytes);
            assert_eq!(fs::read(&outside_file).unwrap(), b"outside-unchanged");
            assert_no_sync_mutation_artifacts(&root);

            fs::remove_dir_all(root).unwrap();
            fs::remove_dir_all(outside).unwrap();
        });
    }

    #[cfg(unix)]
    #[test]
    fn preserves_new_occupant_and_retains_captured_directory_in_protected_staging() {
        tauri::async_runtime::block_on(async {
            let root = temp_root("replace-directory-restore-occupied");
            let note = root.join("note.md");
            let backend = FakeBackend::new("replace-directory-restore-occupied-target");
            write_file(&note, b"local-old");
            backend.set("note.md", b"remote-new");
            let (manifest_path, manifest_bytes) = seed_fake_manifest(
                &root,
                "replace-directory-restore-occupied-target",
                b"local-old",
                b"remote-old",
            );

            let note_for_replace = note.clone();
            let note_for_occupant = note.clone();
            let hooks = super::RemoteSyncExecutionHooks {
                final_replace: Some(Box::new(move |_| {
                    fs::remove_file(&note_for_replace).map_err(|error| error.to_string())?;
                    fs::create_dir(&note_for_replace).map_err(|error| error.to_string())?;
                    fs::write(note_for_replace.join("user.txt"), b"captured-directory")
                        .map_err(|error| error.to_string())
                })),
                quarantine_restore: Some(Box::new(move || {
                    fs::write(&note_for_occupant, b"new-occupant")
                        .map_err(|error| error.to_string())
                })),
                ..Default::default()
            };
            let result = execute_remote_sync_with_hooks(&root, &backend, hooks).await;
            let error = result.expect_err("an occupied restore target must fail closed");

            assert!(
                error.contains("unsafe") || error.contains("changed"),
                "{error}"
            );
            assert_eq!(fs::read(&note).unwrap(), b"new-occupant");
            let staged_target = protected_staged_target(&root)
                .expect("captured directory must remain in protected staging");
            assert_eq!(
                fs::read(staged_target.join("user.txt")).unwrap(),
                b"captured-directory"
            );
            assert_eq!(
                backend.files.lock().unwrap().get("note.md").unwrap(),
                b"remote-new"
            );
            assert_eq!(fs::read(&manifest_path).unwrap(), manifest_bytes);
            assert_no_sibling_sync_mutation_artifacts(&root);

            fs::remove_dir_all(root).unwrap();
        });
    }

    #[test]
    fn preserves_regular_file_occupant_and_retains_captured_file_in_protected_staging() {
        tauri::async_runtime::block_on(async {
            let root = temp_root("replace-regular-restore-occupied");
            let note = root.join("note.md");
            let backend = FakeBackend::new("replace-regular-restore-occupied-target");
            write_file(&note, b"local-old");
            backend.set("note.md", b"remote-new");
            let (manifest_path, _) = seed_fake_manifest(
                &root,
                "replace-regular-restore-occupied-target",
                b"local-old",
                b"remote-old",
            );

            let note_for_replace = note.clone();
            let note_for_occupant = note.clone();
            let replace_calls = Arc::new(AtomicUsize::new(0));
            let replace_counter = Arc::clone(&replace_calls);
            let hooks = super::RemoteSyncExecutionHooks {
                final_replace: Some(Box::new(move |_| {
                    if replace_counter.fetch_add(1, Ordering::SeqCst) == 0 {
                        fs::remove_file(&note_for_replace).map_err(|error| error.to_string())?;
                        fs::write(&note_for_replace, b"captured-regular")
                            .map_err(|error| error.to_string())?;
                    }
                    Ok(())
                })),
                quarantine_restore: Some(Box::new(move || {
                    fs::write(&note_for_occupant, b"new-occupant")
                        .map_err(|error| error.to_string())
                })),
                ..Default::default()
            };
            let summary = execute_remote_sync_with_hooks(&root, &backend, hooks)
                .await
                .expect("a regular occupied restore should preserve both versions and re-plan");

            assert_eq!(summary.conflict_files, 1);
            assert_eq!(fs::read(&note).unwrap(), b"new-occupant");
            let staged_target = protected_staged_target(&root)
                .expect("captured regular file must remain in protected staging");
            assert_eq!(fs::read(staged_target).unwrap(), b"captured-regular");
            assert_eq!(
                backend.files.lock().unwrap().get("note.md").unwrap(),
                b"remote-new"
            );
            assert_eq!(
                fs::read(find_conflict_file(&root).expect("remote conflict copy")).unwrap(),
                b"remote-new"
            );
            assert!(manifest_path.exists());
            assert_no_sibling_sync_mutation_artifacts(&root);

            fs::remove_dir_all(root).unwrap();
        });
    }

    #[cfg(unix)]
    #[test]
    fn replans_delete_after_final_check_and_uploads_the_replacement() {
        tauri::async_runtime::block_on(async {
            let root = temp_root("delete-final-check-race");
            let note = root.join("note.md");
            let backend = FakeBackend::new("delete-final-check-race-target");
            write_file(&note, b"local-old");
            let (manifest_path, _) = seed_fake_manifest(
                &root,
                "delete-final-check-race-target",
                b"local-old",
                b"remote-old",
            );

            let note_for_hook = note.clone();
            let hook_calls = Arc::new(AtomicUsize::new(0));
            let hook_counter = Arc::clone(&hook_calls);
            let hooks = super::RemoteSyncExecutionHooks {
                final_delete: Some(Box::new(move |_| {
                    if hook_counter.fetch_add(1, Ordering::SeqCst) == 0 {
                        fs::remove_file(&note_for_hook).map_err(|error| error.to_string())?;
                        fs::write(&note_for_hook, b"user-replacement")
                            .map_err(|error| error.to_string())?;
                    }
                    Ok(())
                })),
                ..Default::default()
            };
            let summary = execute_remote_sync_with_hooks(&root, &backend, hooks)
                .await
                .expect("a regular delete replacement should be re-planned safely");

            assert_eq!(summary.uploaded_files, 1);
            assert_eq!(fs::read(&note).unwrap(), b"user-replacement");
            assert_eq!(
                backend.files.lock().unwrap().get("note.md").unwrap(),
                b"user-replacement"
            );
            assert!(manifest_path.exists());
            assert_no_sync_mutation_artifacts(&root);

            fs::remove_dir_all(root).unwrap();
        });
    }

    #[cfg(unix)]
    #[test]
    fn rejects_remote_download_when_parent_becomes_symlink_escape_during_replace() {
        tauri::async_runtime::block_on(async {
            let root = temp_root("download-parent-replace-race");
            let outside = temp_root("download-parent-replace-race-outside");
            let retained = root.join("retained-notes");
            let notes = root.join("notes");
            fs::create_dir_all(&notes).unwrap();
            let backend = FakeBackend::new("download-parent-replace-race-target");
            backend.set("notes/file.md", b"remote-private");

            let notes_for_hook = notes.clone();
            let retained_for_hook = retained.clone();
            let outside_for_hook = outside.clone();
            let hooks = super::RemoteSyncExecutionHooks {
                atomic_replace: Some(Box::new(move |_| {
                    fs::rename(&notes_for_hook, &retained_for_hook)
                        .map_err(|error| error.to_string())?;
                    symlink(&outside_for_hook, &notes_for_hook).map_err(|error| error.to_string())
                })),
                ..Default::default()
            };
            let result = execute_remote_sync_with_hooks(&root, &backend, hooks).await;
            let error = result.expect_err("parent replacement race must fail");

            assert!(error.contains("unsafe"), "{error}");
            assert!(!outside.join("file.md").exists());
            assert!(!retained.join("file.md").exists());
            assert!(!retained.join(".file.md.markra-sync-tmp").exists());
            assert!(!test_state_root(&root).join("fake-manifest.json").exists());

            fs::remove_file(notes).unwrap();
            fs::remove_dir_all(retained).unwrap();
            fs::remove_dir_all(root).unwrap();
            fs::remove_dir_all(outside).unwrap();
        });
    }

    #[test]
    fn resets_manifest_baseline_when_target_changes() {
        tauri::async_runtime::block_on(async {
            let root = temp_root("target-reset");
            let first = FakeBackend::new("target-a");
            write_file(&root.join("draft.md"), b"local");
            execute_remote_sync(&root, &first).await.unwrap();

            let second = FakeBackend::new("target-b");
            second.set("draft.md", b"other-remote");
            let result = execute_remote_sync(&root, &second).await.unwrap();
            assert_eq!(result.conflict_files, 1);
            fs::remove_dir_all(root).unwrap();
        });
    }

    #[test]
    fn serializes_remote_sync_execution_across_entry_points() {
        tauri::async_runtime::block_on(async {
            let first_root = temp_root("concurrency-first");
            let second_root = temp_root("concurrency-second");
            let backend = ConcurrencyBackend::default();

            let (first, second) = tokio::join!(
                execute_remote_sync(&first_root, &backend),
                execute_remote_sync(&second_root, &backend)
            );

            first.unwrap();
            second.unwrap();
            assert_eq!(backend.max_active.load(Ordering::SeqCst), 1);
            fs::remove_dir_all(first_root).unwrap();
            fs::remove_dir_all(second_root).unwrap();
        });
    }

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "markra-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        root.canonicalize().unwrap()
    }

    fn write_file(path: &Path, bytes: &[u8]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, bytes).unwrap();
    }

    fn strict_staging_nonce(name: &str, kind: &str, expected_sequence: usize) -> String {
        let prefix = format!(".markra-sync-stage-{kind}-");
        let mut parts = name.strip_prefix(&prefix).unwrap().split('-');
        assert_eq!(parts.next(), Some("v1"));
        parts.next().unwrap().parse::<u32>().unwrap();
        parts.next().unwrap().parse::<u64>().unwrap();
        let nonce = parts.next().unwrap();
        assert_eq!(nonce.len(), 32);
        assert!(
            nonce
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "nonce must be strict lower hex: {nonce}"
        );
        assert_eq!(
            parts.next().unwrap().parse::<usize>().unwrap(),
            expected_sequence
        );
        assert_eq!(parts.next(), None);
        nonce.to_string()
    }

    fn assert_publication_replacement_is_rejected<F>(name: &str, replace: F)
    where
        F: Fn(&Path, &Path) -> Result<(), String> + Send + Sync + 'static,
    {
        tauri::async_runtime::block_on(async move {
            let source = temp_root(&format!("publication-{name}-source"));
            let state = temp_root(&format!("publication-{name}-state"));
            let outside = temp_root(&format!("publication-{name}-outside"));
            let outside_file = outside.join("replacement.bin");
            write_file(&outside_file, b"remote-new");
            write_file(&source.join("note.md"), b"local-base");
            let backend = FakeBackend::new(&format!("publication-{name}-target"));
            let scope = RemoteSyncScope::notes(&source, &state, "notes.json", None, None).unwrap();
            execute_scoped_remote_sync(&scope, &backend).await.unwrap();
            backend.set("note.md", b"remote-new");
            let outside_for_hook = outside_file.clone();
            let hooks = super::RemoteSyncExecutionHooks {
                atomic_replace: Some(Box::new(move |target| {
                    let parent = target
                        .parent()
                        .ok_or_else(|| "missing parent".to_string())?;
                    let publication = fs::read_dir(parent)
                        .map_err(|error| error.to_string())?
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|error| error.to_string())?
                        .into_iter()
                        .find(|entry| {
                            entry
                                .file_name()
                                .to_string_lossy()
                                .to_ascii_lowercase()
                                .starts_with(".markra-sync-stage-publish-")
                        })
                        .ok_or_else(|| "publication temporary file missing".to_string())?
                        .path();
                    replace(&publication, &outside_for_hook)
                })),
                ..Default::default()
            };

            let error = execute_scoped_remote_sync_with_hooks(&scope, &backend, hooks)
                .await
                .expect_err("replaced publication temporary file must fail closed");

            assert!(
                error.contains("unsafe") || error.contains("changed"),
                "{error}"
            );
            assert_eq!(fs::read(source.join("note.md")).unwrap(), b"local-base");
            assert_eq!(fs::read(outside_file).unwrap(), b"remote-new");
            assert_no_sibling_sync_mutation_artifacts(&source);
            fs::remove_dir_all(source).unwrap();
            fs::remove_dir_all(state).unwrap();
            fs::remove_dir_all(outside).unwrap();
        });
    }

    fn seed_fake_manifest(
        root: &Path,
        target: &str,
        local_bytes: &[u8],
        remote_bytes: &[u8],
    ) -> (PathBuf, Vec<u8>) {
        let path = test_state_root(root).join("fake-manifest.json");
        let bytes = format!(
            "{{\n  \"entries\": {{\n    \"note.md\": {{\n      \"local_hash\": \"{}\",\n      \"remote_identity\": \"{}\"\n    }}\n  }},\n  \"target_fingerprint\": \"{}\",\n  \"version\": 3,\n  \"full_scan_completed\": true\n}}",
            sha256_hex(local_bytes),
            FakeBackend::identity(remote_bytes),
            sha256_hex(target.as_bytes())
        )
        .into_bytes();
        write_file(&path, &bytes);
        (path, bytes)
    }

    fn assert_no_sync_mutation_artifacts(root: &Path) {
        let mut artifacts = fs::read_dir(root)
            .unwrap()
            .flatten()
            .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
            .filter(|name| {
                let name = name.to_ascii_lowercase();
                name.contains("markra-sync-tmp") || name.contains("markra-sync-stage")
            })
            .collect::<Vec<_>>();
        artifacts.extend(
            fs::read_dir(test_state_root(root).join("staging"))
                .into_iter()
                .flatten()
                .flatten()
                .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
                .filter(|name| {
                    let name = name.to_ascii_lowercase();
                    name.contains("markra-sync-tmp") || name.contains("markra-sync-stage")
                })
                .map(|name| format!("sync-state/staging/{name}")),
        );
        assert!(artifacts.is_empty(), "left sync artifacts: {artifacts:?}");
    }

    fn assert_no_sibling_sync_mutation_artifacts(root: &Path) {
        let artifacts = fs::read_dir(root)
            .unwrap()
            .flatten()
            .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
            .filter(|name| name.to_ascii_lowercase().starts_with(".markra-sync-stage-"))
            .collect::<Vec<_>>();
        assert!(artifacts.is_empty(), "left sibling staging: {artifacts:?}");
    }

    fn protected_staged_target(root: &Path) -> Option<PathBuf> {
        fs::read_dir(test_state_root(root).join("conflicts"))
            .ok()?
            .flatten()
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name.to_ascii_lowercase().starts_with(".markra-sync-stage-")
                            && name.to_ascii_lowercase().contains("quarantine")
                    })
            })
            .map(|path| path)
    }

    fn find_conflict_file(root: &Path) -> Option<PathBuf> {
        fs::read_dir(root)
            .ok()?
            .flatten()
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.contains("remote-conflict"))
            })
    }

    fn remote_conflict_files(root: &Path) -> usize {
        fs::read_dir(root)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.contains("remote-conflict"))
            })
            .count()
    }
}
