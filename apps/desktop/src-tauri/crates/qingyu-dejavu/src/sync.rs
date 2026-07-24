// DejaVu - Data snapshot and sync.
// Copyright (c) 2022-present, b3log.org
// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use cap_fs_ext::DirExt;
use ignore::gitignore::GitignoreBuilder;
use time::OffsetDateTime;

use crate::path_security::cap_metadata_is_reparse;
use crate::store::{RawObjectKind, Store};
use crate::{
    random_hash, CheckIndex, CheckIndexFile, Cloud, CloudError, CloudObject, ExpectedRevision,
    File, Index, MergeResult, RefStore, RemoteLockGuard, Repo, RepoError, RepositoryRelativePath,
    TrafficStat, WorkingTreeAction, WorkingTreeChange, WorkingTreeCoordinator,
};

const SEVEN_MINUTES_MILLIS: i64 = 7 * 60 * 1_000;
const CLOUD_LATEST_KEY: &str = "refs/latest";
const CLOUD_SEQUENCE_PREFIX: &str = "refs/latest-";
const QINGYU_SYNCIGNORE_PATH: &str = "/.qingyu/syncignore";
const MAX_REMOTE_REF_BYTES: usize = 42;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SyncMode {
    Bidirectional,
    DownloadOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TransferKind {
    File,
    Chunk,
}

struct MergePlan {
    result: MergeResult,
    local_changed: bool,
    changes: Vec<WorkingTreeChange>,
}

impl Repo {
    pub async fn sync(
        &self,
        cloud: Arc<dyn Cloud>,
        coordinator: Arc<dyn WorkingTreeCoordinator>,
    ) -> Result<(MergeResult, TrafficStat), RepoError> {
        self.run_sync(cloud, coordinator, SyncMode::Bidirectional)
            .await
    }

    pub async fn sync_download(
        &self,
        cloud: Arc<dyn Cloud>,
        coordinator: Arc<dyn WorkingTreeCoordinator>,
    ) -> Result<(MergeResult, TrafficStat), RepoError> {
        self.run_sync(cloud, coordinator, SyncMode::DownloadOnly)
            .await
    }

    async fn run_sync(
        &self,
        cloud: Arc<dyn Cloud>,
        coordinator: Arc<dyn WorkingTreeCoordinator>,
        mode: SyncMode,
    ) -> Result<(MergeResult, TrafficStat), RepoError> {
        let _sync = self.sync_guard.lock().await;
        let guard = self.lock_cloud(Arc::clone(&cloud)).await?;
        let operation = self
            .run_sync_under_lock(&cloud, &guard, coordinator, mode)
            .await;
        let release = guard.release().await;
        match (operation, release) {
            (Ok(result), Ok(())) => Ok(result),
            (Ok(_), Err(unlock)) => Err(RepoError::Cloud(unlock)),
            (Err(operation), Ok(())) => Err(operation),
            (Err(operation), Err(unlock)) => Err(RepoError::OperationAndUnlockFailed {
                operation: Box::new(operation),
                unlock,
            }),
        }
    }

    async fn run_sync_under_lock(
        &self,
        cloud: &Arc<dyn Cloud>,
        guard: &RemoteLockGuard,
        coordinator: Arc<dyn WorkingTreeCoordinator>,
        mode: SyncMode,
    ) -> Result<(MergeResult, TrafficStat), RepoError> {
        // This sequence is deliberately pinned: current index, local latest,
        // cloud latest, latest-sync, then the remaining missing cloud objects.
        let current = self.index_current("[Sync] Current working tree")?;
        let _local_latest = self.latest()?;
        let mut traffic = TrafficStat::default();
        let cloud_latest = download_cloud_latest(&self.store, cloud, &mut traffic).await?;
        let latest_sync = self.latest_sync()?;

        let current_files = files_for_index(&self.store, &current)?;
        let latest_sync_files = latest_sync
            .as_ref()
            .map(|index| files_for_index(&self.store, index))
            .transpose()?
            .unwrap_or_default();

        let Some(cloud_latest) = cloud_latest else {
            let result = empty_merge_result();
            if mode == SyncMode::Bidirectional {
                let final_index =
                    publish_remote_index(&self.store, cloud, guard, current, &mut traffic).await?;
                update_local_refs(&self.store, &final_index)?;
            }
            return Ok((result, traffic));
        };

        ensure_cloud_index_contents(&self.store, cloud, &cloud_latest, &mut traffic).await?;
        let cloud_files = files_for_index(&self.store, &cloud_latest)?;
        let plan = plan_merge(
            &self.store,
            &current_files,
            &latest_sync_files,
            &cloud_files,
        )?;

        store_remote_conflicts(self, &plan.result)?;
        apply_working_tree_plan(self, coordinator, &plan).await?;

        let mut final_index = if plan.result.data_changed() && plan.local_changed {
            self.index_current("[Sync] Cloud sync merge")?
        } else if plan.result.data_changed() {
            cloud_latest.clone()
        } else {
            current
        };

        if mode == SyncMode::Bidirectional && plan.local_changed {
            final_index =
                publish_remote_index(&self.store, cloud, guard, final_index, &mut traffic).await?;
        }
        update_local_refs(&self.store, &final_index)?;
        Ok((plan.result, traffic))
    }

    fn index_current(&self, memo: &str) -> Result<Index, RepoError> {
        match self.index(memo) {
            Ok(index) => Ok(index),
            Err(RepoError::EmptyIndex) => {
                let created = now_millis()?;
                let mut index = Index {
                    id: random_hash().map_err(|_| RepoError::RandomnessUnavailable)?,
                    memo: memo.to_owned(),
                    created,
                    files: Vec::new(),
                    count: 0,
                    size: 0,
                    system_id: self.device.id.clone(),
                    system_name: self.device.name.clone(),
                    system_os: self.device.os.clone(),
                    check_index_id: String::new(),
                    aes_key_verify_val: String::new(),
                };
                index.init_aes_key_verify_val(&self.key)?;
                self.store.put_index(&index)?;
                Ok(index)
            }
            Err(error) => Err(error),
        }
    }
}

fn empty_merge_result() -> MergeResult {
    MergeResult {
        time: OffsetDateTime::now_utc(),
        upserts: Vec::new(),
        removes: Vec::new(),
        conflicts: Vec::new(),
    }
}

fn files_for_index(store: &Store, index: &Index) -> Result<Vec<File>, RepoError> {
    index.files.iter().map(|id| store.get_file(id)).collect()
}

fn plan_merge(
    store: &Store,
    current: &[File],
    latest_sync: &[File],
    cloud: &[File],
) -> Result<MergePlan, RepoError> {
    let (mut local_upserts, local_removes) = crate::diff_upsert_remove(current, latest_sync);
    let (cloud_upserts, cloud_removes) = crate::diff_upsert_remove(cloud, current);
    let cloud_syncignore = cloud_upserts
        .iter()
        .find(|file| file.path == QINGYU_SYNCIGNORE_PATH)
        .cloned();
    let cloud_by_path = by_path(&cloud_upserts);
    local_upserts.retain(|local| {
        cloud_by_path
            .get(local.path.as_str())
            .is_none_or(|remote| !local_upsert_is_too_old(local, remote))
    });
    let local_changed = !local_upserts.is_empty() || !local_removes.is_empty();

    let local_upserts_by_path = by_path(&local_upserts);
    let local_removes_by_path = by_path(&local_removes);
    let current_by_path = by_path(current);
    let mut result = empty_merge_result();

    for remote in cloud_upserts {
        if local_upserts_by_path.contains_key(remote.path.as_str()) {
            result.conflicts.push(remote);
            continue;
        }
        if local_removes_by_path.contains_key(remote.path.as_str()) {
            continue;
        }
        if remote.path.ends_with(".tmp") {
            continue;
        }
        if current_by_path
            .get(remote.path.as_str())
            .is_some_and(|local| cloud_upsert_is_too_old(local, &remote))
        {
            continue;
        }
        result.upserts.push(remote);
    }

    for remote in cloud_removes {
        if !local_upserts_by_path.contains_key(remote.path.as_str()) {
            result.removes.push(remote);
        }
    }

    if let Some(syncignore) = cloud_syncignore {
        let bytes = open_file_from_store(store, &syncignore)?;
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| RepoError::InvalidData("syncignore must be UTF-8"))?
            .replace("\r\n", "\n");
        let mut builder = GitignoreBuilder::new("");
        for line in text.split('\n') {
            builder
                .add_line(None, line)
                .map_err(|_| RepoError::InvalidData("syncignore rule is invalid"))?;
        }
        let matcher = builder
            .build()
            .map_err(|_| RepoError::InvalidData("syncignore rule is invalid"))?;
        result.removes.retain(|file| {
            file.path.strip_prefix('/').is_some_and(|path| {
                !matcher
                    .matched_path_or_any_parents(Path::new(path), false)
                    .is_ignore()
            })
        });
    }

    sort_files(&mut result.upserts);
    sort_files(&mut result.removes);
    sort_files(&mut result.conflicts);
    let changes = working_tree_changes(current, &result)?;
    Ok(MergePlan {
        result,
        local_changed,
        changes,
    })
}

fn by_path(files: &[File]) -> BTreeMap<&str, &File> {
    files
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect()
}

fn sort_files(files: &mut [File]) {
    files.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.id.cmp(&right.id))
    });
}

pub(crate) fn local_upsert_is_too_old(local: &File, cloud: &File) -> bool {
    i128::from(local.updated) < i128::from(cloud.updated) - i128::from(SEVEN_MINUTES_MILLIS)
}

pub(crate) fn cloud_upsert_is_too_old(local: &File, cloud: &File) -> bool {
    i128::from(local.updated) > i128::from(cloud.updated) + i128::from(SEVEN_MINUTES_MILLIS)
}

fn working_tree_changes(
    current: &[File],
    result: &MergeResult,
) -> Result<Vec<WorkingTreeChange>, RepoError> {
    let current_by_path = by_path(current);
    let mut changes = Vec::with_capacity(result.upserts.len() + result.removes.len());
    for file in &result.upserts {
        changes.push(WorkingTreeChange {
            path: working_tree_path(&file.path)?,
            expected_revision: expected_revision(current_by_path.get(file.path.as_str()).copied()),
            action: WorkingTreeAction::Write,
        });
    }
    for file in &result.removes {
        changes.push(WorkingTreeChange {
            path: working_tree_path(&file.path)?,
            expected_revision: expected_revision(current_by_path.get(file.path.as_str()).copied()),
            action: WorkingTreeAction::Remove,
        });
    }
    changes.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(changes)
}

fn working_tree_path(path: &str) -> Result<RepositoryRelativePath, RepoError> {
    let stripped = path.strip_prefix('/').ok_or(RepoError::UnsafePath)?;
    RepositoryRelativePath::new(stripped)
}

fn expected_revision(file: Option<&File>) -> ExpectedRevision {
    file.map_or(ExpectedRevision::Absent, |file| ExpectedRevision::File {
        id: file.id.clone(),
        size: file.size,
        updated: file.updated,
    })
}

fn store_remote_conflicts(repo: &Repo, result: &MergeResult) -> Result<(), RepoError> {
    if result.conflicts.is_empty() {
        return Ok(());
    }
    let timestamp = history_timestamp(result.time);
    for file in &result.conflicts {
        let relative = file.path.strip_prefix('/').ok_or(RepoError::UnsafePath)?;
        let bytes = open_file_from_store(&repo.store, file)?;
        repo.history
            .store_remote_conflict(&timestamp, Path::new(relative), &bytes)?;
    }
    Ok(())
}

fn history_timestamp(time: OffsetDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02}-{:02}{:02}{:02}",
        time.year(),
        u8::from(time.month()),
        time.day(),
        time.hour(),
        time.minute(),
        time.second()
    )
}

fn open_file_from_store(store: &Store, file: &File) -> Result<Vec<u8>, RepoError> {
    let mut bytes = Vec::new();
    for chunk_id in &file.chunks {
        let chunk = store.get_chunk(chunk_id)?;
        bytes.extend_from_slice(&chunk.data);
        if i64::try_from(bytes.len()).map_err(|_| RepoError::RepoFatal)? > file.size {
            return Err(RepoError::InvalidData(
                "file chunks exceed the declared file size",
            ));
        }
    }
    if i64::try_from(bytes.len()).map_err(|_| RepoError::RepoFatal)? != file.size {
        return Err(RepoError::InvalidData(
            "file chunks do not match the declared file size",
        ));
    }
    Ok(bytes)
}

async fn apply_working_tree_plan(
    repo: &Repo,
    coordinator: Arc<dyn WorkingTreeCoordinator>,
    plan: &MergePlan,
) -> Result<(), RepoError> {
    if plan.changes.is_empty() {
        return Ok(());
    }
    let permit = coordinator.prepare(&plan.changes).await?;
    let operation = (|| {
        for change in &plan.changes {
            if repo.working_tree_revision(&change.path)? != change.expected_revision {
                return Err(RepoError::WorkingTreeChanged);
            }
        }
        repo.checkout_files(&plan.result.upserts)?;
        repo.remove_files(&plan.result.removes)
    })();
    coordinator.release(permit).await;
    operation
}

impl Repo {
    fn working_tree_revision(
        &self,
        path: &RepositoryRelativePath,
    ) -> Result<ExpectedRevision, RepoError> {
        let components = path.as_str().split('/').collect::<Vec<_>>();
        let mut directory = self.data_dir.try_clone()?;
        for component in &components[..components.len() - 1] {
            directory = match directory.open_dir_nofollow(component) {
                Ok(directory) => directory,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(ExpectedRevision::Absent);
                }
                Err(error) => return Err(error.into()),
            };
            let metadata = directory.dir_metadata()?;
            if !metadata.file_type().is_dir() || cap_metadata_is_reparse(&metadata) {
                return Err(RepoError::UnsafePath);
            }
        }
        let name = components.last().ok_or(RepoError::UnsafePath)?;
        let metadata = match directory.symlink_metadata(name) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ExpectedRevision::Absent);
            }
            Err(error) => return Err(error.into()),
        };
        if !metadata.file_type().is_file() || cap_metadata_is_reparse(&metadata) {
            return Err(RepoError::UnsafePath);
        }
        let updated = metadata_updated(&metadata)?;
        let size = i64::try_from(metadata.len()).map_err(|_| RepoError::RepoFatal)?;
        let repository_path = format!("/{}", path.as_str());
        Ok(ExpectedRevision::File {
            id: File::new(repository_path, size, updated).id,
            size,
            updated,
        })
    }
}

fn metadata_updated(metadata: &cap_std::fs::Metadata) -> Result<i64, RepoError> {
    let modified = filetime::FileTime::from_system_time(metadata.modified()?.into_std());
    modified
        .unix_seconds()
        .checked_mul(1_000)
        .and_then(|millis| millis.checked_add(i64::from(modified.nanoseconds() / 1_000_000)))
        .ok_or(RepoError::RepoFatal)
}

fn update_local_refs(store: &Store, index: &Index) -> Result<(), RepoError> {
    let refs = RefStore::new(store);
    refs.update_latest(index)?;
    refs.update_latest_sync(index)
}

async fn download_cloud_latest(
    store: &Store,
    cloud: &Arc<dyn Cloud>,
    traffic: &mut TrafficStat,
) -> Result<Option<Index>, RepoError> {
    let latest_bytes = match tracked_get(cloud, CLOUD_LATEST_KEY, TransferKind::File, traffic).await
    {
        Ok(bytes) => bytes,
        Err(CloudError::NotFound) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let latest_id = parse_remote_ref(&latest_bytes)?;
    let mut latest = ensure_remote_index(store, cloud, &latest_id, traffic).await?;

    let objects = tracked_list(cloud, "refs/", traffic).await?;
    if let Some((_sequence, sequence_id)) = max_sequence_ref(&objects) {
        if sequence_id != latest_id {
            let sequence_latest = ensure_remote_index(store, cloud, &sequence_id, traffic).await?;
            if sequence_latest.created > latest.created {
                latest = sequence_latest;
            }
        }
    }
    Ok(Some(latest))
}

fn parse_remote_ref(bytes: &[u8]) -> Result<String, RepoError> {
    if bytes.len() > MAX_REMOTE_REF_BYTES {
        return Err(RepoError::InvalidData(
            "remote ref exceeds the 42-byte limit",
        ));
    }
    let id = std::str::from_utf8(bytes)
        .map_err(|_| RepoError::InvalidData("remote ref must be UTF-8"))?
        .trim();
    crate::store::validate_id(id)?;
    Ok(id.to_owned())
}

async fn ensure_remote_index(
    store: &Store,
    cloud: &Arc<dyn Cloud>,
    id: &str,
    traffic: &mut TrafficStat,
) -> Result<Index, RepoError> {
    if !store.contains_raw(RawObjectKind::Index, id)? {
        let key = format!("indexes/{id}");
        let bytes = tracked_get(cloud, &key, TransferKind::File, traffic).await?;
        store.import_raw(RawObjectKind::Index, id, &bytes)?;
    }
    store.get_index(id)
}

async fn ensure_cloud_index_contents(
    store: &Store,
    cloud: &Arc<dyn Cloud>,
    index: &Index,
    traffic: &mut TrafficStat,
) -> Result<(), RepoError> {
    if !index.check_index_id.is_empty() {
        if !store.contains_raw(RawObjectKind::CheckIndex, &index.check_index_id)? {
            let key = format!("check/indexes/{}", index.check_index_id);
            let bytes = tracked_get(cloud, &key, TransferKind::File, traffic).await?;
            store.import_raw(RawObjectKind::CheckIndex, &index.check_index_id, &bytes)?;
        }
        let check = store.get_check_index(&index.check_index_id)?;
        if check.index_id != index.id {
            return Err(RepoError::InvalidData(
                "check index target does not match cloud latest",
            ));
        }
    }

    for file_id in &index.files {
        if !store.contains_raw(RawObjectKind::File, file_id)? {
            let key = object_key(file_id)?;
            let bytes = tracked_get(cloud, &key, TransferKind::File, traffic).await?;
            store.import_raw(RawObjectKind::File, file_id, &bytes)?;
        }
    }
    let files = files_for_index(store, index)?;
    let chunk_ids = files
        .iter()
        .flat_map(|file| file.chunks.iter().cloned())
        .collect::<BTreeSet<_>>();
    for chunk_id in chunk_ids {
        if !store.contains_raw(RawObjectKind::Chunk, &chunk_id)? {
            let key = object_key(&chunk_id)?;
            let bytes = tracked_get(cloud, &key, TransferKind::Chunk, traffic).await?;
            store.import_raw(RawObjectKind::Chunk, &chunk_id, &bytes)?;
        }
    }
    Ok(())
}

async fn publish_remote_index(
    store: &Store,
    cloud: &Arc<dyn Cloud>,
    guard: &RemoteLockGuard,
    mut index: Index,
    traffic: &mut TrafficStat,
) -> Result<Index, RepoError> {
    let files = files_for_index(store, &index)?;
    let check = CheckIndex {
        id: random_hash().map_err(|_| RepoError::RandomnessUnavailable)?,
        index_id: index.id.clone(),
        files: files
            .iter()
            .map(|file| CheckIndexFile {
                id: file.id.clone(),
                chunks: file.chunks.clone(),
            })
            .collect(),
    };
    index.check_index_id = check.id.clone();
    store.put_check_index(&check)?;
    store.put_index(&index)?;

    let chunk_ids = files
        .iter()
        .flat_map(|file| file.chunks.iter().cloned())
        .collect::<BTreeSet<_>>();
    for chunk_id in chunk_ids {
        publish_raw(
            store,
            cloud,
            guard,
            RawObjectKind::Chunk,
            &chunk_id,
            TransferKind::Chunk,
            traffic,
        )
        .await?;
    }
    for file in &files {
        publish_raw(
            store,
            cloud,
            guard,
            RawObjectKind::File,
            &file.id,
            TransferKind::File,
            traffic,
        )
        .await?;
    }
    publish_raw(
        store,
        cloud,
        guard,
        RawObjectKind::CheckIndex,
        &check.id,
        TransferKind::File,
        traffic,
    )
    .await?;
    publish_raw(
        store,
        cloud,
        guard,
        RawObjectKind::Index,
        &index.id,
        TransferKind::File,
        traffic,
    )
    .await?;

    tracked_put(
        cloud,
        guard,
        CLOUD_LATEST_KEY,
        index.id.as_bytes(),
        true,
        TransferKind::File,
        traffic,
    )
    .await?;
    let existing = tracked_list(cloud, "refs/", traffic).await?;
    let next_sequence =
        max_sequence_ref(&existing).map_or(1, |(sequence, _)| sequence.saturating_add(1));
    let sequence_key = format!("{CLOUD_SEQUENCE_PREFIX}{next_sequence}-{}", index.id);
    tracked_put(
        cloud,
        guard,
        &sequence_key,
        index.id.as_bytes(),
        true,
        TransferKind::File,
        traffic,
    )
    .await?;

    for stale in existing
        .into_iter()
        .filter(|object| object.key.starts_with(CLOUD_SEQUENCE_PREFIX))
    {
        if guard.ensure_healthy().is_err() {
            break;
        }
        traffic.api_put = traffic.api_put.saturating_add(1);
        let _cleanup_result = cloud.remove(&stale.key).await;
    }
    Ok(index)
}

async fn publish_raw(
    store: &Store,
    cloud: &Arc<dyn Cloud>,
    guard: &RemoteLockGuard,
    kind: RawObjectKind,
    id: &str,
    transfer: TransferKind,
    traffic: &mut TrafficStat,
) -> Result<(), RepoError> {
    let bytes = store.export_raw(kind, id)?;
    let key = match kind {
        RawObjectKind::Chunk | RawObjectKind::File => object_key(id)?,
        RawObjectKind::Index => format!("indexes/{id}"),
        RawObjectKind::CheckIndex => format!("check/indexes/{id}"),
    };
    match tracked_put(cloud, guard, &key, &bytes, false, transfer, traffic).await {
        Ok(()) | Err(RepoError::Cloud(CloudError::AlreadyExists)) => Ok(()),
        Err(error) => Err(error),
    }
}

async fn tracked_get(
    cloud: &Arc<dyn Cloud>,
    key: &str,
    kind: TransferKind,
    traffic: &mut TrafficStat,
) -> Result<Vec<u8>, CloudError> {
    traffic.api_get = traffic.api_get.saturating_add(1);
    let bytes = cloud.get(key).await?;
    traffic.download_bytes = traffic
        .download_bytes
        .saturating_add(i64::try_from(bytes.len()).unwrap_or(i64::MAX));
    match kind {
        TransferKind::File => {
            traffic.download_file_count = traffic.download_file_count.saturating_add(1)
        }
        TransferKind::Chunk => {
            traffic.download_chunk_count = traffic.download_chunk_count.saturating_add(1)
        }
    }
    Ok(bytes)
}

async fn tracked_put(
    cloud: &Arc<dyn Cloud>,
    guard: &RemoteLockGuard,
    key: &str,
    bytes: &[u8],
    overwrite: bool,
    kind: TransferKind,
    traffic: &mut TrafficStat,
) -> Result<(), RepoError> {
    guard.ensure_healthy()?;
    traffic.api_put = traffic.api_put.saturating_add(1);
    let written = cloud.put(key, bytes, overwrite).await?;
    if written != bytes.len() as u64 {
        return Err(RepoError::InvalidData(
            "cloud put returned an invalid payload length",
        ));
    }
    traffic.upload_bytes = traffic
        .upload_bytes
        .saturating_add(i64::try_from(written).unwrap_or(i64::MAX));
    match kind {
        TransferKind::File => {
            traffic.upload_file_count = traffic.upload_file_count.saturating_add(1)
        }
        TransferKind::Chunk => {
            traffic.upload_chunk_count = traffic.upload_chunk_count.saturating_add(1)
        }
    }
    Ok(())
}

async fn tracked_list(
    cloud: &Arc<dyn Cloud>,
    prefix: &str,
    traffic: &mut TrafficStat,
) -> Result<Vec<CloudObject>, CloudError> {
    traffic.api_get = traffic.api_get.saturating_add(1);
    cloud.list(prefix).await
}

fn max_sequence_ref(objects: &[CloudObject]) -> Option<(u64, String)> {
    objects
        .iter()
        .filter_map(|object| parse_sequence_ref(&object.key))
        .max_by_key(|(sequence, _)| *sequence)
}

fn parse_sequence_ref(key: &str) -> Option<(u64, String)> {
    let rest = key.strip_prefix(CLOUD_SEQUENCE_PREFIX)?;
    let (sequence, id) = rest.split_once('-')?;
    let sequence = sequence.parse().ok()?;
    crate::store::validate_id(id).ok()?;
    Some((sequence, id.to_owned()))
}

fn object_key(id: &str) -> Result<String, RepoError> {
    crate::store::validate_id(id)?;
    Ok(format!("objects/{}/{}", &id[..2], &id[2..]))
}

fn now_millis() -> Result<i64, RepoError> {
    i64::try_from(OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000)
        .map_err(|_| RepoError::RepoFatal)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use filetime::FileTime;
    use tempfile::TempDir;
    use tokio::sync::Notify;

    use crate::{
        Cloud, CloudError, CloudObject, Device, File, LocalCloud, NoopWorkingTreeCoordinator, Repo,
        RepoError, RepoOptions, RepoPaths, Store, WorkingTreeChange, WorkingTreeCoordinator,
        WorkingTreePermit,
    };

    struct RepoFixture {
        _root: TempDir,
        data: PathBuf,
        history: PathBuf,
        repo: Repo,
    }

    fn repo_fixture(name: &str, options: RepoOptions) -> RepoFixture {
        let root = TempDir::new().unwrap();
        let data = root.path().join("data");
        let history = root.path().join("history");
        fs::create_dir_all(&data).unwrap();
        let repo = Repo::open(
            RepoPaths {
                data: data.clone(),
                repo: root.path().join("repo"),
                history: history.clone(),
                temp: root.path().join("temp"),
            },
            Device {
                id: format!("device-{name}"),
                name: name.to_owned(),
                os: "test".to_owned(),
            },
            [7; 32],
            options,
        )
        .unwrap();
        RepoFixture {
            _root: root,
            data,
            history,
            repo,
        }
    }

    fn cloud_fixture() -> (TempDir, Arc<LocalCloud>) {
        let root = TempDir::new().unwrap();
        let cloud_path = root.path().join("cloud");
        fs::create_dir_all(&cloud_path).unwrap();
        let cloud = Arc::new(LocalCloud::new(cloud_path).unwrap());
        (root, cloud)
    }

    fn coordinator() -> Arc<NoopWorkingTreeCoordinator> {
        Arc::new(NoopWorkingTreeCoordinator)
    }

    fn write_file(root: &Path, relative: &str, bytes: &[u8], updated: i64) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        filetime::set_file_mtime(
            &path,
            FileTime::from_unix_time(
                updated.div_euclid(1_000),
                updated.rem_euclid(1_000) as u32 * 1_000_000,
            ),
        )
        .unwrap();
    }

    async fn sync(repo: &Repo, cloud: Arc<dyn Cloud>) -> crate::MergeResult {
        repo.sync(cloud, coordinator()).await.unwrap().0
    }

    struct InspectCloud {
        inner: Arc<LocalCloud>,
        events: Mutex<Vec<String>>,
        fail_put: Mutex<HashMap<String, usize>>,
        fail_remove: Mutex<HashMap<String, usize>>,
        fail_refresh: AtomicBool,
        lock_puts: AtomicUsize,
        refresh_failed: Notify,
    }

    impl InspectCloud {
        fn new(inner: Arc<LocalCloud>) -> Self {
            Self {
                inner,
                events: Mutex::new(Vec::new()),
                fail_put: Mutex::new(HashMap::new()),
                fail_remove: Mutex::new(HashMap::new()),
                fail_refresh: AtomicBool::new(false),
                lock_puts: AtomicUsize::new(0),
                refresh_failed: Notify::new(),
            }
        }

        fn fail_put(&self, key: &str, count: usize) {
            self.fail_put.lock().unwrap().insert(key.to_owned(), count);
        }

        fn fail_remove(&self, key: &str, count: usize) {
            self.fail_remove
                .lock()
                .unwrap()
                .insert(key.to_owned(), count);
        }

        fn fail_next_refresh(&self) {
            self.fail_refresh.store(true, Ordering::SeqCst);
        }

        fn clear_events(&self) {
            self.events.lock().unwrap().clear();
        }

        fn events(&self) -> Vec<String> {
            self.events.lock().unwrap().clone()
        }

        fn should_fail(map: &Mutex<HashMap<String, usize>>, key: &str) -> bool {
            let mut map = map.lock().unwrap();
            let Some(remaining) = map.get_mut(key) else {
                return false;
            };
            if *remaining == 0 {
                return false;
            }
            *remaining -= 1;
            true
        }
    }

    #[async_trait::async_trait]
    impl Cloud for InspectCloud {
        async fn get(&self, key: &str) -> Result<Vec<u8>, CloudError> {
            self.events.lock().unwrap().push(format!("get:{key}"));
            self.inner.get(key).await
        }

        async fn put(&self, key: &str, bytes: &[u8], overwrite: bool) -> Result<u64, CloudError> {
            self.events.lock().unwrap().push(format!("put:{key}"));
            if key == "lock-sync" {
                let count = self.lock_puts.fetch_add(1, Ordering::SeqCst) + 1;
                if count > 1 && self.fail_refresh.swap(false, Ordering::SeqCst) {
                    self.refresh_failed.notify_waiters();
                    return Err(CloudError::Unavailable);
                }
            }
            if Self::should_fail(&self.fail_put, key) {
                return Err(CloudError::Backend {
                    code: "test_put_failure",
                    retryable: false,
                });
            }
            self.inner.put(key, bytes, overwrite).await
        }

        async fn remove(&self, key: &str) -> Result<(), CloudError> {
            self.events.lock().unwrap().push(format!("remove:{key}"));
            if Self::should_fail(&self.fail_remove, key) {
                return Err(CloudError::Backend {
                    code: "test_remove_failure",
                    retryable: false,
                });
            }
            self.inner.remove(key).await
        }

        async fn list(&self, prefix: &str) -> Result<Vec<CloudObject>, CloudError> {
            self.events.lock().unwrap().push(format!("list:{prefix}"));
            self.inner.list(prefix).await
        }

        async fn available_size(&self) -> Result<u64, CloudError> {
            self.inner.available_size().await
        }
    }

    struct MutatingCoordinator {
        path: PathBuf,
        bytes: Vec<u8>,
        releases: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl WorkingTreeCoordinator for MutatingCoordinator {
        async fn prepare(
            &self,
            _changes: &[WorkingTreeChange],
        ) -> Result<WorkingTreePermit, RepoError> {
            fs::write(&self.path, &self.bytes).unwrap();
            filetime::set_file_mtime(&self.path, FileTime::from_unix_time(1_800_000_000, 0))
                .unwrap();
            Ok(WorkingTreePermit::new(()))
        }

        async fn release(&self, _permit: WorkingTreePermit) {
            self.releases.fetch_add(1, Ordering::SeqCst);
        }
    }

    struct BlockingCoordinator {
        entered: Notify,
        proceed: Notify,
        releases: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl WorkingTreeCoordinator for BlockingCoordinator {
        async fn prepare(
            &self,
            _changes: &[WorkingTreeChange],
        ) -> Result<WorkingTreePermit, RepoError> {
            self.entered.notify_one();
            self.proceed.notified().await;
            Ok(WorkingTreePermit::new(()))
        }

        async fn release(&self, _permit: WorkingTreePermit) {
            self.releases.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn first_upload_and_first_download_start_from_empty_latest_sync() {
        let (_cloud_root, cloud) = cloud_fixture();
        let uploader = repo_fixture("upload", RepoOptions::default());
        write_file(
            &uploader.data,
            "notes/first.md",
            b"first",
            1_700_000_000_000,
        );

        let (uploaded, upload_traffic) = uploader
            .repo
            .sync(cloud.clone(), coordinator())
            .await
            .unwrap();
        assert!(uploaded.upserts.is_empty());
        assert!(uploaded.removes.is_empty());
        assert!(uploaded.conflicts.is_empty());
        assert!(cloud.get("refs/latest").await.is_ok());
        assert!(uploader.repo.latest_sync().unwrap().is_some());
        assert!(upload_traffic.api_put > 0);
        assert!(upload_traffic.upload_bytes > 0);
        let uploaded_latest = uploader.repo.latest_sync().unwrap().unwrap();
        assert!(!uploaded_latest.check_index_id.is_empty());
        assert_eq!(
            cloud
                .get(&format!("indexes/{}", uploaded_latest.id))
                .await
                .unwrap(),
            uploader
                .repo
                .store
                .export_raw(super::RawObjectKind::Index, &uploaded_latest.id)
                .unwrap()
        );
        assert_eq!(
            cloud
                .get(&format!("check/indexes/{}", uploaded_latest.check_index_id))
                .await
                .unwrap(),
            uploader
                .repo
                .store
                .export_raw(
                    super::RawObjectKind::CheckIndex,
                    &uploaded_latest.check_index_id,
                )
                .unwrap()
        );

        let downloader = repo_fixture("download", RepoOptions::default());
        let downloaded = sync(&downloader.repo, cloud.clone()).await;
        assert_eq!(
            fs::read(downloader.data.join("notes/first.md")).unwrap(),
            b"first"
        );
        assert_eq!(downloaded.upserts.len(), 1);
        assert!(downloaded.removes.is_empty());
        assert!(downloaded.conflicts.is_empty());
        assert_eq!(
            downloader.repo.latest().unwrap().unwrap().id,
            downloader.repo.latest_sync().unwrap().unwrap().id
        );
    }

    #[tokio::test]
    async fn independent_paths_merge_and_publish_the_combined_tree() {
        let (_cloud_root, cloud) = cloud_fixture();
        let first = repo_fixture("first", RepoOptions::default());
        let second = repo_fixture("second", RepoOptions::default());
        write_file(&first.data, "base.md", b"base", 1_700_000_000_000);
        sync(&first.repo, cloud.clone()).await;
        sync(&second.repo, cloud.clone()).await;

        write_file(&first.data, "cloud.md", b"cloud", 1_700_000_010_000);
        write_file(&second.data, "local.md", b"local", 1_700_000_020_000);
        sync(&first.repo, cloud.clone()).await;
        let merged = sync(&second.repo, cloud.clone()).await;

        assert_eq!(
            merged
                .upserts
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            vec!["/cloud.md"]
        );
        assert!(merged.conflicts.is_empty());
        assert_eq!(fs::read(second.data.join("cloud.md")).unwrap(), b"cloud");
        let third = repo_fixture("third", RepoOptions::default());
        sync(&third.repo, cloud).await;
        assert_eq!(fs::read(third.data.join("local.md")).unwrap(), b"local");
        assert_eq!(fs::read(third.data.join("cloud.md")).unwrap(), b"cloud");
    }

    #[tokio::test]
    async fn local_update_beats_cloud_remove_and_republishes_the_local_file() {
        let (_cloud_root, cloud) = cloud_fixture();
        let first = repo_fixture("first", RepoOptions::default());
        let second = repo_fixture("second", RepoOptions::default());
        write_file(&first.data, "doc.md", b"base", 1_700_000_000_000);
        sync(&first.repo, cloud.clone()).await;
        sync(&second.repo, cloud.clone()).await;

        fs::remove_file(first.data.join("doc.md")).unwrap();
        sync(&first.repo, cloud.clone()).await;
        write_file(&second.data, "doc.md", b"local", 1_700_000_010_000);
        let result = sync(&second.repo, cloud.clone()).await;

        assert!(result.upserts.is_empty());
        assert!(result.removes.is_empty());
        assert!(result.conflicts.is_empty());
        let third = repo_fixture("third", RepoOptions::default());
        sync(&third.repo, cloud).await;
        assert_eq!(fs::read(third.data.join("doc.md")).unwrap(), b"local");
    }

    #[tokio::test]
    async fn local_remove_beats_cloud_update_and_republishes_the_removal() {
        let (_cloud_root, cloud) = cloud_fixture();
        let first = repo_fixture("first", RepoOptions::default());
        let second = repo_fixture("second", RepoOptions::default());
        write_file(&first.data, "doc.md", b"base", 1_700_000_000_000);
        sync(&first.repo, cloud.clone()).await;
        sync(&second.repo, cloud.clone()).await;

        write_file(&first.data, "doc.md", b"cloud", 1_700_000_010_000);
        sync(&first.repo, cloud.clone()).await;
        fs::remove_file(second.data.join("doc.md")).unwrap();
        let result = sync(&second.repo, cloud.clone()).await;

        assert!(result.upserts.is_empty());
        assert!(result.removes.is_empty());
        assert!(result.conflicts.is_empty());
        assert!(!second.data.join("doc.md").exists());
        let third = repo_fixture("third", RepoOptions::default());
        sync(&third.repo, cloud).await;
        assert!(!third.data.join("doc.md").exists());
    }

    #[tokio::test]
    async fn same_path_create_conflict_keeps_local_and_stores_remote_history() {
        let (_cloud_root, cloud) = cloud_fixture();
        let first = repo_fixture("first", RepoOptions::default());
        let second = repo_fixture("second", RepoOptions::default());
        write_file(&first.data, "anchor.md", b"anchor", 1_700_000_000_000);
        sync(&first.repo, cloud.clone()).await;
        sync(&second.repo, cloud.clone()).await;

        write_file(&first.data, "same.md", b"remote", 1_700_000_020_000);
        write_file(&second.data, "same.md", b"local", 1_700_000_010_000);
        sync(&first.repo, cloud.clone()).await;
        let result = sync(&second.repo, cloud.clone()).await;

        assert_eq!(
            result
                .conflicts
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            vec!["/same.md"]
        );
        assert_eq!(fs::read(second.data.join("same.md")).unwrap(), b"local");
        let snapshots = fs::read_dir(&second.history)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(fs::read(snapshots[0].join("same.md")).unwrap(), b"remote");

        let third = repo_fixture("third", RepoOptions::default());
        sync(&third.repo, cloud).await;
        assert_eq!(fs::read(third.data.join("same.md")).unwrap(), b"local");
    }

    #[tokio::test]
    async fn same_path_update_conflict_keeps_local_and_reports_remote_file() {
        let (_cloud_root, cloud) = cloud_fixture();
        let first = repo_fixture("first", RepoOptions::default());
        let second = repo_fixture("second", RepoOptions::default());
        write_file(&first.data, "same.md", b"base", 1_700_000_000_000);
        sync(&first.repo, cloud.clone()).await;
        sync(&second.repo, cloud.clone()).await;

        write_file(&first.data, "same.md", b"remote", 1_700_000_020_000);
        write_file(&second.data, "same.md", b"local", 1_700_000_010_000);
        sync(&first.repo, cloud.clone()).await;
        let result = sync(&second.repo, cloud).await;

        assert_eq!(result.conflicts.len(), 1);
        assert_eq!(fs::read(second.data.join("same.md")).unwrap(), b"local");
    }

    #[tokio::test]
    async fn local_upsert_older_than_seven_minutes_yields_but_exact_boundary_conflicts() {
        let (_cloud_root, cloud) = cloud_fixture();
        let first = repo_fixture("first", RepoOptions::default());
        let second = repo_fixture("second", RepoOptions::default());
        write_file(&first.data, "older.md", b"base", 1_700_000_000_000);
        write_file(&first.data, "boundary.md", b"base", 1_700_000_000_000);
        sync(&first.repo, cloud.clone()).await;
        sync(&second.repo, cloud.clone()).await;

        let remote_time = 1_700_001_000_000;
        write_file(&first.data, "older.md", b"remote", remote_time);
        write_file(&first.data, "boundary.md", b"remote", remote_time);
        write_file(
            &second.data,
            "older.md",
            b"too-old-local",
            remote_time - 420_001,
        );
        write_file(
            &second.data,
            "boundary.md",
            b"boundary-local",
            remote_time - 420_000,
        );
        sync(&first.repo, cloud.clone()).await;
        let result = sync(&second.repo, cloud).await;

        assert_eq!(fs::read(second.data.join("older.md")).unwrap(), b"remote");
        assert_eq!(
            fs::read(second.data.join("boundary.md")).unwrap(),
            b"boundary-local"
        );
        assert_eq!(
            result
                .conflicts
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            vec!["/boundary.md"]
        );
    }

    #[tokio::test]
    async fn tmp_cloud_upsert_is_downloaded_to_repo_but_not_written_to_working_tree() {
        let (_cloud_root, cloud) = cloud_fixture();
        let first = repo_fixture(
            "first",
            RepoOptions {
                protected_include_paths: vec!["/remote.tmp".to_owned()],
                ..RepoOptions::default()
            },
        );
        write_file(&first.data, "anchor.md", b"anchor", 1_700_000_000_000);
        write_file(&first.data, "remote.tmp", b"temporary", 1_700_000_001_000);
        sync(&first.repo, cloud.clone()).await;

        let second = repo_fixture("second", RepoOptions::default());
        let result = sync(&second.repo, cloud).await;
        assert!(!second.data.join("remote.tmp").exists());
        assert!(!result.upserts.iter().any(|file| file.path == "/remote.tmp"));
    }

    #[tokio::test]
    async fn qingyu_cloud_syncignore_filters_removes_before_working_tree_deletion() {
        let (_cloud_root, cloud) = cloud_fixture();
        let options = RepoOptions {
            protected_include_paths: vec![super::QINGYU_SYNCIGNORE_PATH.to_owned()],
            ..RepoOptions::default()
        };
        let first = repo_fixture("first", options.clone());
        let second = repo_fixture("second", options);
        write_file(&first.data, "ignored.md", b"keep", 1_700_000_000_000);
        write_file(&first.data, "anchor.md", b"anchor", 1_700_000_000_000);
        sync(&first.repo, cloud.clone()).await;
        sync(&second.repo, cloud.clone()).await;

        fs::remove_file(first.data.join("ignored.md")).unwrap();
        write_file(
            &first.data,
            ".qingyu/syncignore",
            b"/ignored.md\n",
            1_700_000_010_000,
        );
        sync(&first.repo, cloud.clone()).await;
        let result = sync(&second.repo, cloud).await;

        assert!(second.data.join("ignored.md").exists());
        assert!(result.removes.is_empty());
        assert_eq!(
            fs::read(second.data.join(".qingyu/syncignore")).unwrap(),
            b"/ignored.md\n"
        );
    }

    #[tokio::test]
    async fn cloud_upsert_more_than_seven_minutes_older_is_ignored_without_local_upsert() {
        let (_cloud_root, cloud) = cloud_fixture();
        let first = repo_fixture("first", RepoOptions::default());
        let second = repo_fixture("second", RepoOptions::default());
        let old_time = 1_700_000_000_000;
        let new_time = old_time + 420_001;
        write_file(&first.data, "doc.md", b"old", old_time);
        sync(&first.repo, cloud.clone()).await;
        let old_index = first.repo.latest_sync().unwrap().unwrap();
        sync(&second.repo, cloud.clone()).await;

        write_file(&second.data, "doc.md", b"new", new_time);
        sync(&second.repo, cloud.clone()).await;

        for object in cloud.list("refs/latest-").await.unwrap() {
            cloud.remove(&object.key).await.unwrap();
        }
        cloud
            .put("refs/latest", old_index.id.as_bytes(), true)
            .await
            .unwrap();
        cloud
            .put(
                &format!("refs/latest-99-{}", old_index.id),
                old_index.id.as_bytes(),
                true,
            )
            .await
            .unwrap();

        let result = sync(&second.repo, cloud).await;
        assert!(result.upserts.is_empty());
        assert_eq!(fs::read(second.data.join("doc.md")).unwrap(), b"new");
    }

    #[tokio::test]
    async fn working_tree_change_after_prepare_is_typed_released_and_never_overwritten() {
        let (_cloud_root, cloud) = cloud_fixture();
        let first = repo_fixture("first", RepoOptions::default());
        let second = repo_fixture("second", RepoOptions::default());
        write_file(&first.data, "doc.md", b"base", 1_700_000_000_000);
        sync(&first.repo, cloud.clone()).await;
        sync(&second.repo, cloud.clone()).await;
        let previous_ref = second.repo.latest().unwrap().unwrap().id;

        write_file(&first.data, "doc.md", b"remote", 1_700_000_010_000);
        sync(&first.repo, cloud.clone()).await;
        let coordinator = Arc::new(MutatingCoordinator {
            path: second.data.join("doc.md"),
            bytes: b"edited-after-plan".to_vec(),
            releases: AtomicUsize::new(0),
        });
        let error = second
            .repo
            .sync(cloud, coordinator.clone())
            .await
            .unwrap_err();

        assert!(matches!(error, RepoError::WorkingTreeChanged));
        assert_eq!(coordinator.releases.load(Ordering::SeqCst), 1);
        assert_eq!(
            fs::read(second.data.join("doc.md")).unwrap(),
            b"edited-after-plan"
        );
        assert_eq!(second.repo.latest().unwrap().unwrap().id, previous_ref);
    }

    #[tokio::test]
    async fn download_only_keeps_local_conflict_updates_local_refs_and_never_publishes() {
        let (_cloud_root, inner) = cloud_fixture();
        let first = repo_fixture("first", RepoOptions::default());
        write_file(&first.data, "same.md", b"remote", 1_700_000_010_000);
        sync(&first.repo, inner.clone()).await;
        let remote_ref_before = inner.get("refs/latest").await.unwrap();

        let second = repo_fixture("second", RepoOptions::default());
        write_file(&second.data, "same.md", b"local", 1_700_000_000_000);
        write_file(&second.data, "local.md", b"local-only", 1_700_000_000_000);
        let inspected = Arc::new(InspectCloud::new(inner.clone()));
        let result = second
            .repo
            .sync_download(inspected.clone(), coordinator())
            .await
            .unwrap()
            .0;

        assert_eq!(result.conflicts.len(), 1);
        assert_eq!(fs::read(second.data.join("same.md")).unwrap(), b"local");
        assert!(second.data.join("local.md").exists());
        assert_eq!(
            second.repo.latest().unwrap().unwrap().id,
            second.repo.latest_sync().unwrap().unwrap().id
        );
        assert_eq!(inner.get("refs/latest").await.unwrap(), remote_ref_before);
        assert!(inspected.events().iter().all(|event| {
            event == "put:lock-sync"
                || event == "remove:lock-sync"
                || event.starts_with("get:")
                || event.starts_with("list:")
        }));
    }

    #[tokio::test]
    async fn remote_ref_failure_does_not_advance_either_local_ref() {
        let (_cloud_root, inner) = cloud_fixture();
        let inspected = Arc::new(InspectCloud::new(inner));
        inspected.fail_put("refs/latest", 1);
        let local = repo_fixture("local", RepoOptions::default());
        write_file(&local.data, "doc.md", b"local", 1_700_000_000_000);

        let error = local.repo.sync(inspected, coordinator()).await.unwrap_err();
        assert!(matches!(error, RepoError::Cloud(_)));
        assert!(local.repo.latest().unwrap().is_none());
        assert!(local.repo.latest_sync().unwrap().is_none());
    }

    #[tokio::test]
    async fn successful_operation_with_release_failure_returns_unlock_error_once_stopped() {
        let (_cloud_root, inner) = cloud_fixture();
        let inspected = Arc::new(InspectCloud::new(inner));
        inspected.fail_remove("lock-sync", 3);
        let local = repo_fixture("local", RepoOptions::default());
        write_file(&local.data, "doc.md", b"local", 1_700_000_000_000);

        let error = local
            .repo
            .sync(inspected.clone(), coordinator())
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            RepoError::Cloud(CloudError::UnlockFailed { .. })
        ));
        assert_eq!(
            inspected
                .events()
                .iter()
                .filter(|event| event.as_str() == "remove:lock-sync")
                .count(),
            3
        );
    }

    #[tokio::test]
    async fn operation_and_release_failure_preserve_primary_and_unlock_diagnostics() {
        let (_cloud_root, inner) = cloud_fixture();
        let inspected = Arc::new(InspectCloud::new(inner));
        inspected.fail_put("refs/latest", 1);
        inspected.fail_remove("lock-sync", 3);
        let local = repo_fixture("local", RepoOptions::default());
        write_file(&local.data, "doc.md", b"local", 1_700_000_000_000);

        let error = local.repo.sync(inspected, coordinator()).await.unwrap_err();
        let RepoError::OperationAndUnlockFailed { operation, unlock } = error else {
            panic!("expected combined operation and unlock error");
        };
        assert!(matches!(*operation, RepoError::Cloud(_)));
        assert!(matches!(unlock, CloudError::UnlockFailed { .. }));
    }

    #[tokio::test(start_paused = true)]
    async fn unhealthy_refresh_aborts_before_any_non_lock_publication() {
        let (_cloud_root, inner) = cloud_fixture();
        let first = repo_fixture("first", RepoOptions::default());
        write_file(&first.data, "remote.md", b"remote", 1_700_000_000_000);
        sync(&first.repo, inner.clone()).await;

        let RepoFixture {
            _root,
            data,
            history: _,
            repo,
        } = repo_fixture("second", RepoOptions::default());
        write_file(&data, "local.md", b"local", 1_700_000_000_000);
        let repo = Arc::new(repo);
        let inspected = Arc::new(InspectCloud::new(inner));
        inspected.fail_next_refresh();
        let coordinator = Arc::new(BlockingCoordinator {
            entered: Notify::new(),
            proceed: Notify::new(),
            releases: AtomicUsize::new(0),
        });
        let task_repo = Arc::clone(&repo);
        let task_cloud = Arc::clone(&inspected);
        let task_coordinator = Arc::clone(&coordinator);
        let task = tokio::spawn(async move { task_repo.sync(task_cloud, task_coordinator).await });

        coordinator.entered.notified().await;
        tokio::time::advance(std::time::Duration::from_secs(30)).await;
        inspected.refresh_failed.notified().await;
        coordinator.proceed.notify_one();
        let error = task.await.unwrap().unwrap_err();

        assert!(matches!(error, RepoError::RemoteLockUnhealthy(_)));
        assert_eq!(coordinator.releases.load(Ordering::SeqCst), 1);
        assert!(inspected
            .events()
            .iter()
            .filter(|event| event.starts_with("put:"))
            .all(|event| event == "put:lock-sync"));
        drop(_root);
    }

    #[test]
    fn raw_store_transfer_rejects_corruption_wrong_ids_and_wrong_keys_without_clobber() {
        let source = repo_fixture("source", RepoOptions::default());
        write_file(&source.data, "doc.md", b"source", 1_700_000_000_000);
        let index = source.repo.index("raw fixture").unwrap();
        let file_id = index.files[0].clone();
        let raw = source
            .repo
            .store
            .export_raw(super::RawObjectKind::File, &file_id)
            .unwrap();
        assert_eq!(
            source
                .repo
                .store
                .list_raw_ids(super::RawObjectKind::File)
                .unwrap(),
            vec![file_id.clone()]
        );

        let target_root = TempDir::new().unwrap();
        let target = Store::new(target_root.path().join("repo"), [7; 32]).unwrap();
        let mut corrupt = raw.clone();
        corrupt[0] ^= 0xff;
        assert!(target
            .import_raw(super::RawObjectKind::File, &file_id, &corrupt)
            .is_err());
        assert!(!target
            .contains_raw(super::RawObjectKind::File, &file_id)
            .unwrap());

        let wrong_id = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        assert!(matches!(
            target.import_raw(super::RawObjectKind::File, wrong_id, &raw),
            Err(RepoError::InvalidData(_))
        ));
        assert!(!target
            .contains_raw(super::RawObjectKind::File, wrong_id)
            .unwrap());

        let wrong_key_root = TempDir::new().unwrap();
        let wrong_key = Store::new(wrong_key_root.path().join("repo"), [8; 32]).unwrap();
        assert!(matches!(
            wrong_key.import_raw(super::RawObjectKind::File, &file_id, &raw),
            Err(RepoError::DecryptionFailed)
        ));
    }

    #[tokio::test]
    async fn corrupt_and_wrong_id_remote_indexes_fail_closed_without_advancing_refs() {
        let source = repo_fixture("source", RepoOptions::default());
        write_file(&source.data, "doc.md", b"source", 1_700_000_000_000);
        let source_index = source.repo.index("raw fixture").unwrap();
        let raw_index = source
            .repo
            .store
            .export_raw(super::RawObjectKind::Index, &source_index.id)
            .unwrap();

        for (remote_id, bytes) in [
            (source_index.id.clone(), vec![0_u8; 8]),
            (
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned(),
                raw_index.clone(),
            ),
        ] {
            let (_cloud_root, cloud) = cloud_fixture();
            cloud
                .put("refs/latest", remote_id.as_bytes(), true)
                .await
                .unwrap();
            cloud
                .put(&format!("indexes/{remote_id}"), &bytes, true)
                .await
                .unwrap();
            let local = repo_fixture("local", RepoOptions::default());

            assert!(local.repo.sync(cloud, coordinator()).await.is_err());
            assert!(local.repo.latest().unwrap().is_none());
            assert!(local.repo.latest_sync().unwrap().is_none());
            assert!(!local
                .repo
                .store
                .contains_raw(super::RawObjectKind::Index, &remote_id)
                .unwrap());
        }
    }

    #[tokio::test]
    async fn sequence_ref_is_visible_before_stale_sequence_cleanup() {
        let (_cloud_root, inner) = cloud_fixture();
        let inspected = Arc::new(InspectCloud::new(inner));
        let local = repo_fixture("local", RepoOptions::default());
        write_file(&local.data, "base.md", b"base", 1_700_000_000_000);
        sync(&local.repo, inspected.clone()).await;
        inspected.clear_events();

        write_file(&local.data, "new.md", b"new", 1_700_000_010_000);
        let (_result, traffic) = local
            .repo
            .sync(inspected.clone(), coordinator())
            .await
            .unwrap();
        let events = inspected.events();
        let latest = events
            .iter()
            .rposition(|event| event == "put:refs/latest")
            .unwrap();
        let list = events
            .iter()
            .enumerate()
            .skip(latest + 1)
            .find(|(_, event)| event.as_str() == "list:refs/")
            .map(|(index, _)| index)
            .unwrap();
        let sequence = events
            .iter()
            .enumerate()
            .skip(list + 1)
            .find(|(_, event)| event.starts_with("put:refs/latest-"))
            .map(|(index, _)| index)
            .unwrap();
        let cleanup = events
            .iter()
            .enumerate()
            .skip(sequence + 1)
            .find(|(_, event)| event.starts_with("remove:refs/latest-"))
            .map(|(index, _)| index)
            .unwrap();
        assert!(latest < list && list < sequence && sequence < cleanup);
        for prefix in ["put:objects/", "put:check/indexes/", "put:indexes/"] {
            assert!(events[..latest]
                .iter()
                .any(|event| event.starts_with(prefix)));
        }
        assert!(events[..latest]
            .iter()
            .filter(|event| event.starts_with("put:") && event.as_str() != "put:lock-sync")
            .all(|event| !event.starts_with("put:refs/")));
        let actual_state_puts = events
            .iter()
            .filter(|event| event.starts_with("put:") && event.as_str() != "put:lock-sync")
            .count()
            + events
                .iter()
                .filter(|event| event.starts_with("remove:refs/latest-"))
                .count();
        let actual_state_gets = events
            .iter()
            .filter(|event| event.starts_with("get:") && event.as_str() != "get:lock-sync")
            .count()
            + events
                .iter()
                .filter(|event| event.starts_with("list:"))
                .count();
        assert_eq!(traffic.api_put, actual_state_puts);
        assert_eq!(traffic.api_get, actual_state_gets);
        assert!(traffic.api_put > traffic.upload_file_count + traffic.upload_chunk_count);
    }

    #[test]
    fn seven_minute_filters_are_strict_and_asymmetric() {
        let cloud = File::new("/doc.md", 1, 1_700_000_500_000);
        let older_than_boundary = File::new("/doc.md", 1, cloud.updated - 420_001);
        let exact_boundary = File::new("/doc.md", 1, cloud.updated - 420_000);

        assert!(super::local_upsert_is_too_old(&older_than_boundary, &cloud));
        assert!(!super::local_upsert_is_too_old(&exact_boundary, &cloud));
        assert!(super::cloud_upsert_is_too_old(
            &File::new("/doc.md", 1, cloud.updated + 420_001),
            &cloud
        ));
        assert!(!super::cloud_upsert_is_too_old(
            &File::new("/doc.md", 1, cloud.updated + 420_000),
            &cloud
        ));
        assert!(super::working_tree_path("//absolute.md").is_err());
        assert!(super::working_tree_path("/../traversal.md").is_err());
    }
}
