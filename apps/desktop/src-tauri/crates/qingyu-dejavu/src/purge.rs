// DejaVu - Data snapshot and sync.
// Copyright (c) 2022-present, b3log.org
// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use cap_fs_ext::DirExt;
use cap_std::fs::Dir;

use crate::path_security::cap_metadata_is_reparse;
use crate::ref_store::{parse_remote_ref, MAX_REMOTE_REF_BYTES};
use crate::store::{validate_id, RawObjectKind};
use crate::{Cloud, CloudError, File, RefStore, RemoteLockGuard, Repo, RepoError, Store};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PurgeStat {
    pub objects: usize,
    pub indexes: usize,
    pub size: i64,
}

#[derive(Default, serde::Deserialize, serde::Serialize)]
struct CloudIndexes {
    indexes: Option<Vec<CloudIndex>>,
}

#[derive(Default, serde::Deserialize, serde::Serialize)]
#[serde(default)]
struct CloudIndex {
    id: String,
    #[serde(rename = "systemID")]
    system_id: String,
    #[serde(rename = "systemName")]
    system_name: String,
    #[serde(rename = "systemOS")]
    system_os: String,
}

impl Repo {
    pub async fn purge_cloud(
        &self,
        cloud: Arc<dyn Cloud>,
        cancelled: &AtomicBool,
    ) -> Result<PurgeStat, RepoError> {
        let _lifecycle = self.acquire_lifecycle().await;
        let guard = self.lock_cloud(Arc::clone(&cloud)).await?;
        let operation =
            purge_cloud_under_lock(self, &cloud, &guard, || cancelled.load(Ordering::Relaxed))
                .await;
        let release = guard.release().await;
        match (operation, release) {
            (Ok(stat), Ok(())) => Ok(stat),
            (Ok(_), Err(unlock)) => Err(RepoError::Cloud(unlock)),
            (Err(operation), Ok(())) => Err(operation),
            (Err(operation), Err(unlock)) => Err(RepoError::OperationAndUnlockFailed {
                operation: Box::new(operation),
                unlock,
            }),
        }
    }
}

async fn purge_cloud_under_lock<F>(
    repo: &Repo,
    cloud: &Arc<dyn Cloud>,
    guard: &RemoteLockGuard,
    mut is_cancelled: F,
) -> Result<PurgeStat, RepoError>
where
    F: FnMut() -> bool,
{
    check_remote_operation(guard, &mut is_cancelled)?;
    let object_entries = cloud.list("objects/").await?;
    let mut object_sizes = BTreeMap::new();
    for object in object_entries {
        check_remote_operation(guard, &mut is_cancelled)?;
        if let Some(id) = object_id_from_key(&object.key) {
            object_sizes.insert(id, object.size);
        }
    }

    check_remote_operation(guard, &mut is_cancelled)?;
    let index_entries = cloud.list("indexes/").await?;
    let mut index_ids = BTreeSet::new();
    for object in index_entries {
        check_remote_operation(guard, &mut is_cancelled)?;
        if let Some(id) = flat_id_from_key(&object.key, "indexes/") {
            index_ids.insert(id);
        }
    }

    check_remote_operation(guard, &mut is_cancelled)?;
    if index_ids.is_empty() || object_sizes.is_empty() {
        return Ok(PurgeStat::default());
    }

    check_remote_operation(guard, &mut is_cancelled)?;
    let refs = cloud.list("refs/").await?;
    let mut referenced_index_ids = BTreeSet::new();
    for reference in refs {
        check_remote_operation(guard, &mut is_cancelled)?;
        let bytes = cloud
            .get_bounded(&reference.key, MAX_REMOTE_REF_BYTES)
            .await?;
        check_remote_operation(guard, &mut is_cancelled)?;
        referenced_index_ids.insert(parse_remote_ref(&bytes)?);
    }

    let unreferenced_index_ids = index_ids
        .difference(&referenced_index_ids)
        .cloned()
        .collect::<Vec<_>>();
    let mut referenced_file_ids = BTreeSet::new();
    let mut referenced_object_ids = BTreeSet::new();
    for index_id in &referenced_index_ids {
        check_remote_operation(guard, &mut is_cancelled)?;
        let index = download_cloud_index(repo, cloud, index_id).await;
        let Ok(index) = index else {
            // Dejavu retains a ref target whose index is missing or corrupt, but it cannot
            // contribute file reachability when the index payload cannot be decoded.
            continue;
        };
        for file_id in index.files {
            check_remote_operation(guard, &mut is_cancelled)?;
            referenced_file_ids.insert(file_id.clone());
            referenced_object_ids.insert(file_id);
        }
    }

    for file_id in referenced_file_ids {
        check_remote_operation(guard, &mut is_cancelled)?;
        let local_file = {
            let _operation = repo.store.lock_operation()?;
            repo.store.get_file_unlocked(&file_id)
        };
        let file = match local_file {
            Ok(file) => file,
            Err(_) => download_cloud_file(repo, cloud, &file_id).await?,
        };
        for chunk_id in file.chunks {
            check_remote_operation(guard, &mut is_cancelled)?;
            referenced_object_ids.insert(chunk_id);
        }
    }

    let mut unreferenced_objects = Vec::new();
    let mut stat = PurgeStat {
        objects: 0,
        indexes: unreferenced_index_ids.len(),
        size: 0,
    };
    for (id, size) in object_sizes {
        check_remote_operation(guard, &mut is_cancelled)?;
        if !referenced_object_ids.contains(&id) {
            stat.objects += 1;
            stat.size = stat
                .size
                .checked_add(
                    i64::try_from(size)
                        .map_err(|_| RepoError::InvalidData("object size exceeds i64"))?,
                )
                .ok_or(RepoError::InvalidData("purge byte count overflow"))?;
            unreferenced_objects.push(id);
        }
    }

    check_remote_operation(guard, &mut is_cancelled)?;
    let check_indexes = cloud.list("check/indexes/").await.unwrap_or_default();

    // No remote mutation may begin after cancellation or lock loss has been observed.
    check_remote_operation(guard, &mut is_cancelled)?;
    for object in check_indexes {
        check_remote_operation(guard, &mut is_cancelled)?;
        cloud.remove(&object.key).await?;
    }
    for id in &unreferenced_index_ids {
        check_remote_operation(guard, &mut is_cancelled)?;
        cloud.remove(&format!("indexes/{id}")).await?;
    }
    let indexes_v2 =
        prepare_cloud_indexes_v2(repo, cloud, guard, &referenced_index_ids, &mut is_cancelled)
            .await?;
    check_remote_operation(guard, &mut is_cancelled)?;
    publish_cloud_indexes_v2(cloud, guard, indexes_v2, &mut is_cancelled).await?;
    for id in unreferenced_objects {
        check_remote_operation(guard, &mut is_cancelled)?;
        cloud.remove(&object_key(&id)?).await?;
    }
    Ok(stat)
}

async fn download_cloud_index(
    repo: &Repo,
    cloud: &Arc<dyn Cloud>,
    id: &str,
) -> Result<crate::Index, RepoError> {
    validate_id(id)?;
    let (staged, _written) = repo
        .stage_cloud_download(cloud, &format!("indexes/{id}"))
        .await?;
    let _operation = repo.store.lock_operation()?;
    repo.store
        .decode_index_reader_unlocked(id, staged.reader()?)
}

async fn download_cloud_file(
    repo: &Repo,
    cloud: &Arc<dyn Cloud>,
    id: &str,
) -> Result<File, RepoError> {
    repo.download_raw_to_store(cloud, &object_key(id)?, RawObjectKind::File, id)
        .await?;
    let _operation = repo.store.lock_operation()?;
    repo.store.get_file_unlocked(id)
}

async fn prepare_cloud_indexes_v2<F>(
    repo: &Repo,
    cloud: &Arc<dyn Cloud>,
    guard: &RemoteLockGuard,
    referenced_index_ids: &BTreeSet<String>,
    is_cancelled: &mut F,
) -> Result<Option<Vec<u8>>, RepoError>
where
    F: FnMut() -> bool,
{
    check_remote_operation(guard, is_cancelled)?;
    let staged = match repo.stage_cloud_download(cloud, "indexes-v2.json").await {
        Ok((staged, _written)) => staged,
        Err(RepoError::Cloud(CloudError::NotFound)) => return Ok(None),
        Err(error) => return Err(error),
    };
    check_remote_operation(guard, is_cancelled)?;
    let mut indexes = match repo
        .store
        .deserialize_compressed_reader::<CloudIndexes, _>(staged.reader()?)
    {
        Ok(indexes) => indexes,
        // The pinned Go decoder logs malformed JSON and leaves this value at its default.
        Err(RepoError::Serialization(_)) => CloudIndexes::default(),
        Err(error) => return Err(error),
    };
    check_remote_operation(guard, is_cancelled)?;
    let retained = indexes
        .indexes
        .take()
        .unwrap_or_default()
        .into_iter()
        .filter(|index| referenced_index_ids.contains(&index.id))
        .collect::<Vec<_>>();
    indexes.indexes = if retained.is_empty() {
        None
    } else {
        Some(retained)
    };
    let json = serde_json::to_vec_pretty(&indexes)?;
    let encoded = repo.store.compress(&json)?;
    check_remote_operation(guard, is_cancelled)?;
    Ok(Some(encoded))
}

async fn publish_cloud_indexes_v2<F>(
    cloud: &Arc<dyn Cloud>,
    guard: &RemoteLockGuard,
    encoded: Option<Vec<u8>>,
    is_cancelled: &mut F,
) -> Result<(), RepoError>
where
    F: FnMut() -> bool,
{
    let Some(encoded) = encoded else {
        return Ok(());
    };
    check_remote_operation(guard, is_cancelled)?;
    let written = cloud.put("indexes-v2.json", &encoded, true).await?;
    if written != encoded.len() as u64 {
        return Err(RepoError::Cloud(CloudError::LengthMismatch {
            expected: encoded.len() as u64,
            actual: written,
        }));
    }
    Ok(())
}

fn check_remote_operation<F>(guard: &RemoteLockGuard, is_cancelled: &mut F) -> Result<(), RepoError>
where
    F: FnMut() -> bool,
{
    check_cancelled(is_cancelled)?;
    guard.ensure_healthy()?;
    Ok(())
}

fn object_key(id: &str) -> Result<String, RepoError> {
    validate_id(id)?;
    Ok(format!("objects/{}/{}", &id[..2], &id[2..]))
}

fn object_id_from_key(key: &str) -> Option<String> {
    let relative = key.strip_prefix("objects/")?;
    let (prefix, suffix) = relative.split_once('/')?;
    if suffix.contains('/') || !is_lower_hex(prefix, 2) || !is_lower_hex(suffix, 38) {
        return None;
    }
    Some(format!("{prefix}{suffix}"))
}

fn flat_id_from_key(key: &str, prefix: &str) -> Option<String> {
    let id = key.strip_prefix(prefix)?;
    if validate_id(id).is_ok() {
        Some(id.to_owned())
    } else {
        None
    }
}

pub(crate) fn purge_store_with_cancel_check<F>(
    store: &Store,
    retained_index_ids: &[String],
    is_cancelled: F,
) -> Result<PurgeStat, RepoError>
where
    F: FnMut() -> bool,
{
    purge_store_with_cancel_check_and_hook(store, retained_index_ids, is_cancelled, || Ok(()))
}

pub(crate) fn purge_store_with_cancel_check_and_hook<F, H>(
    store: &Store,
    retained_index_ids: &[String],
    mut is_cancelled: F,
    mut before_delete: H,
) -> Result<PurgeStat, RepoError>
where
    F: FnMut() -> bool,
    H: FnMut() -> Result<(), RepoError>,
{
    check_cancelled(&mut is_cancelled)?;
    let object_ids = collect_object_ids(store, &mut is_cancelled)?;
    check_cancelled(&mut is_cancelled)?;

    let index_ids = collect_flat_ids(store, Path::new("indexes"), &mut is_cancelled)?;
    check_cancelled(&mut is_cancelled)?;

    let mut referenced_index_ids =
        RefStore::new(store).all_index_ids_unlocked_with_cancel_check(&mut is_cancelled)?;
    for id in retained_index_ids {
        validate_id(id)?;
        referenced_index_ids.insert(id.clone());
    }
    check_cancelled(&mut is_cancelled)?;

    let mut referenced_object_ids = HashSet::new();
    let mut ordered_references = referenced_index_ids.iter().collect::<Vec<_>>();
    ordered_references.sort();
    for index_id in ordered_references {
        check_cancelled(&mut is_cancelled)?;
        let index = store.get_index_unlocked(index_id)?;
        for file_id in index.files {
            check_cancelled(&mut is_cancelled)?;
            validate_id(&file_id)?;
            referenced_object_ids.insert(file_id.clone());
            let file = store.get_file_unlocked(&file_id)?;
            for chunk_id in file.chunks {
                check_cancelled(&mut is_cancelled)?;
                validate_id(&chunk_id)?;
                referenced_object_ids.insert(chunk_id);
            }
        }
    }
    check_cancelled(&mut is_cancelled)?;

    let mut unreferenced_index_ids = index_ids
        .difference(&referenced_index_ids)
        .cloned()
        .collect::<Vec<_>>();
    unreferenced_index_ids.sort();
    let mut unreferenced_object_ids = object_ids
        .difference(&referenced_object_ids)
        .cloned()
        .collect::<Vec<_>>();
    unreferenced_object_ids.sort();
    let unreferenced_indexes = unreferenced_index_ids
        .iter()
        .cloned()
        .collect::<HashSet<_>>();

    let mut check_index_ids =
        collect_flat_ids(store, Path::new("check/indexes"), &mut is_cancelled)?
            .into_iter()
            .collect::<Vec<_>>();
    check_index_ids.sort();
    let mut removable_check_index_ids = Vec::new();
    for id in check_index_ids {
        check_cancelled(&mut is_cancelled)?;
        let check_index = store.get_check_index_unlocked(&id)?;
        if check_index.id != id {
            return Err(RepoError::InvalidData(
                "check index payload id must match its filename",
            ));
        }
        validate_id(&check_index.index_id)?;
        if unreferenced_indexes.contains(&check_index.index_id) {
            removable_check_index_ids.push(id);
        }
    }

    let mut stat = PurgeStat::default();
    check_cancelled(&mut is_cancelled)?;
    before_delete()?;
    check_cancelled(&mut is_cancelled)?;
    for id in &unreferenced_index_ids {
        check_cancelled(&mut is_cancelled)?;
        remove_flat_file(store, Path::new("indexes"), id)?;
        stat.indexes += 1;
    }

    check_cancelled(&mut is_cancelled)?;
    for id in removable_check_index_ids {
        check_cancelled(&mut is_cancelled)?;
        remove_flat_file(store, Path::new("check/indexes"), &id)?;
    }

    check_cancelled(&mut is_cancelled)?;
    for id in unreferenced_object_ids {
        check_cancelled(&mut is_cancelled)?;
        let size = remove_object(store, &id)?;
        stat.objects += 1;
        stat.size = stat
            .size
            .checked_add(size)
            .ok_or(RepoError::InvalidData("purge byte count overflow"))?;
    }
    Ok(stat)
}

fn collect_object_ids<F>(store: &Store, is_cancelled: &mut F) -> Result<HashSet<String>, RepoError>
where
    F: FnMut() -> bool,
{
    check_cancelled(is_cancelled)?;
    let objects = match store.open_directory(Path::new("objects"), false) {
        Ok(objects) => objects,
        Err(error) if is_not_found(&error) => return Ok(HashSet::new()),
        Err(error) => return Err(error),
    };
    let mut ids = HashSet::new();
    for entry in objects.entries()? {
        check_cancelled(is_cancelled)?;
        let entry = entry?;
        let prefix = entry.file_name();
        let metadata = objects.symlink_metadata(&prefix)?;
        if cap_metadata_is_reparse(&metadata) || metadata.file_type().is_symlink() {
            return Err(RepoError::UnsafePath);
        }
        let Some(prefix) = prefix.to_str() else {
            continue;
        };
        if !metadata.file_type().is_dir() || !is_lower_hex(prefix, 2) {
            continue;
        }
        let directory = objects
            .open_dir_nofollow(prefix)
            .map_err(|error| map_nofollow_error(&objects, prefix.as_ref(), error))?;
        for object in directory.entries()? {
            check_cancelled(is_cancelled)?;
            let object = object?;
            let name = object.file_name();
            let metadata = directory.symlink_metadata(&name)?;
            if cap_metadata_is_reparse(&metadata) || metadata.file_type().is_symlink() {
                return Err(RepoError::UnsafePath);
            }
            if !metadata.file_type().is_file() {
                continue;
            }
            let Some(name) = name.to_str() else {
                continue;
            };
            let id = format!("{prefix}{name}");
            if validate_id(&id).is_ok() {
                ids.insert(id);
            }
        }
    }
    Ok(ids)
}

fn collect_flat_ids<F>(
    store: &Store,
    relative: &Path,
    is_cancelled: &mut F,
) -> Result<HashSet<String>, RepoError>
where
    F: FnMut() -> bool,
{
    check_cancelled(is_cancelled)?;
    let directory = match store.open_directory(relative, false) {
        Ok(directory) => directory,
        Err(error) if is_not_found(&error) => return Ok(HashSet::new()),
        Err(error) => return Err(error),
    };
    let mut ids = HashSet::new();
    for entry in directory.entries()? {
        check_cancelled(is_cancelled)?;
        let entry = entry?;
        let name = entry.file_name();
        let metadata = directory.symlink_metadata(&name)?;
        if cap_metadata_is_reparse(&metadata) || metadata.file_type().is_symlink() {
            return Err(RepoError::UnsafePath);
        }
        if !metadata.file_type().is_file() {
            continue;
        }
        if let Some(id) = name.to_str().filter(|id| validate_id(id).is_ok()) {
            ids.insert(id.to_owned());
        }
    }
    Ok(ids)
}

fn remove_flat_file(store: &Store, relative: &Path, id: &str) -> Result<(), RepoError> {
    validate_id(id)?;
    let directory = store.open_directory(relative, false)?;
    validate_regular_file(&directory, id.as_ref())?;
    directory.remove_file(id)?;
    Ok(())
}

fn remove_object(store: &Store, id: &str) -> Result<i64, RepoError> {
    validate_id(id)?;
    let directory_path = Path::new("objects").join(&id[..2]);
    let directory = store.open_directory(&directory_path, false)?;
    let name = &id[2..];
    let metadata = validate_regular_file(&directory, name.as_ref())?;
    let size = i64::try_from(metadata.len())
        .map_err(|_| RepoError::InvalidData("object size exceeds i64"))?;
    directory.remove_file(name)?;
    Ok(size)
}

fn validate_regular_file(
    directory: &Dir,
    name: &std::ffi::OsStr,
) -> Result<cap_std::fs::Metadata, RepoError> {
    let metadata = directory.symlink_metadata(name)?;
    if !metadata.file_type().is_file() || cap_metadata_is_reparse(&metadata) {
        return Err(RepoError::UnsafePath);
    }
    Ok(metadata)
}

fn check_cancelled<F>(is_cancelled: &mut F) -> Result<(), RepoError>
where
    F: FnMut() -> bool,
{
    if is_cancelled() {
        Err(RepoError::Cancelled)
    } else {
        Ok(())
    }
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn map_nofollow_error(parent: &Dir, name: &std::ffi::OsStr, error: std::io::Error) -> RepoError {
    match parent.symlink_metadata(name) {
        Ok(metadata) if metadata.file_type().is_symlink() || cap_metadata_is_reparse(&metadata) => {
            RepoError::UnsafePath
        }
        _ => RepoError::Io(error),
    }
}

fn is_not_found(error: &RepoError) -> bool {
    matches!(error, RepoError::Io(error) if error.kind() == std::io::ErrorKind::NotFound)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::sync::Arc;
    use std::time::Duration;

    use tempfile::TempDir;

    use crate::{
        CheckIndex, CheckIndexFile, Chunk, Cloud, CloudError, CloudObject, CloudOperation,
        CloudUploadSource, Device, File, Index, LocalCloud, RawObjectKind, RefStore, Repo,
        RepoError, RepoOptions, RepoPaths, Store,
    };

    use super::{collect_flat_ids, collect_object_ids, purge_store_with_cancel_check};

    const RETAINED_INDEX_REF: &str = "1111111111111111111111111111111111111111";
    const RETAINED_INDEX_CALLER: &str = "2222222222222222222222222222222222222222";
    const UNREFERENCED_INDEX: &str = "3333333333333333333333333333333333333333";
    const RETAINED_CHECK_REF: &str = "4444444444444444444444444444444444444444";
    const RETAINED_CHECK_CALLER: &str = "5555555555555555555555555555555555555555";
    const UNREFERENCED_CHECK: &str = "6666666666666666666666666666666666666666";
    const RETAINED_FILE_REF: &str = "aa1f5cfc4d153cccacac523c2bf38a5028428830";
    const RETAINED_FILE_CALLER: &str = "9bb9f38cbfb569132c9e39441dad0b6368583c88";
    const UNREFERENCED_FILE: &str = "baa00a51cc31646e8dc08d909d3439c0231cf9bd";
    const SHARED_CHUNK: &str = "d18aac96b905b4b3c839891b7a91c9414149514c";
    const RETAINED_CHUNK_REF: &str = "b1fd00c4e51aee02e1cbb17219649c97d2cd3b78";
    const RETAINED_CHUNK_CALLER: &str = "a6f0959a2c1aebdd1c10a2d03d2f9a453f6ab2e9";
    const UNREACHABLE_CHUNK: &str = "408e9edfd6912aae297c39da08db127ea40a4bfc";
    const MISMATCH_CHECK_ID: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
    const CONCURRENT_INDEX: &str = "ffffffffffffffffffffffffffffffffffffffff";
    const CONCURRENT_CHUNK: &str = "c18f39b2beac0a5693d47fcea130cd8acf595258";

    struct PurgeFixture {
        _temp: TempDir,
        repo: Repo,
        unreachable_encoded_size: i64,
    }

    struct CloudPurgeFixture {
        local: PurgeFixture,
        _cloud_temp: TempDir,
        cloud: Arc<LocalCloud>,
    }

    #[derive(Clone, Copy, Default)]
    struct CloudHooks {
        cancel_before_delete: bool,
        cancel_after_first_repository_remove: bool,
        fail_check_index_list: bool,
        fail_refresh_during_object_list: bool,
        fail_unlock_after_index_list: bool,
        fail_unlock_after_ref_list: bool,
        remove_first_ref_after_list: bool,
    }

    struct HookCloud {
        inner: Arc<LocalCloud>,
        cancelled: Arc<AtomicBool>,
        hooks: CloudHooks,
    }

    impl HookCloud {
        fn new(inner: Arc<LocalCloud>, cancelled: Arc<AtomicBool>, hooks: CloudHooks) -> Self {
            Self {
                inner,
                cancelled,
                hooks,
            }
        }
    }

    #[async_trait::async_trait]
    impl Cloud for HookCloud {
        async fn get_bounded(&self, key: &str, max_bytes: u64) -> Result<Vec<u8>, CloudError> {
            self.inner.get_bounded(key, max_bytes).await
        }

        async fn download_to(
            &self,
            key: &str,
            destination: &mut (dyn tokio::io::AsyncWrite + Unpin + Send),
        ) -> Result<u64, CloudError> {
            self.inner.download_to(key, destination).await
        }

        async fn put(&self, key: &str, bytes: &[u8], overwrite: bool) -> Result<u64, CloudError> {
            self.inner.put(key, bytes, overwrite).await
        }

        async fn upload_from(
            &self,
            key: &str,
            source: &dyn CloudUploadSource,
            overwrite: bool,
        ) -> Result<u64, CloudError> {
            self.inner.upload_from(key, source, overwrite).await
        }

        async fn remove(&self, key: &str) -> Result<(), CloudError> {
            self.inner.remove(key).await?;
            if self.hooks.cancel_after_first_repository_remove && key != "lock-sync" {
                self.cancelled.store(true, Ordering::SeqCst);
            }
            Ok(())
        }

        async fn list(&self, prefix: &str) -> Result<Vec<CloudObject>, CloudError> {
            if self.hooks.fail_check_index_list && prefix == "check/indexes/" {
                return Err(CloudError::Injected(CloudOperation::List));
            }
            let objects = self.inner.list(prefix).await?;
            if self.hooks.fail_refresh_during_object_list && prefix == "objects/" {
                tokio::task::yield_now().await;
                self.inner.fail_next(CloudOperation::Put, 1)?;
                tokio::time::advance(Duration::from_secs(30)).await;
                tokio::task::yield_now().await;
                tokio::task::yield_now().await;
            }
            if self.hooks.remove_first_ref_after_list && prefix == "refs/" {
                if let Some(reference) = objects.first() {
                    self.inner.remove(&reference.key).await?;
                }
            }
            if self.hooks.fail_unlock_after_ref_list && prefix == "refs/" {
                self.inner.fail_next(CloudOperation::Remove, 3)?;
            }
            if self.hooks.fail_unlock_after_index_list && prefix == "indexes/" {
                self.inner.fail_next(CloudOperation::Remove, 3)?;
            }
            if self.hooks.cancel_before_delete && prefix == "check/indexes/" {
                self.cancelled.store(true, Ordering::SeqCst);
            }
            Ok(objects)
        }

        async fn available_size(&self) -> Result<u64, CloudError> {
            self.inner.available_size().await
        }
    }

    fn index(id: &str, file_id: &str, check_index_id: &str) -> Index {
        Index {
            id: id.to_owned(),
            memo: "purge fixture".to_owned(),
            created: 1_700_000_000_123,
            files: vec![file_id.to_owned()],
            count: 1,
            size: 1,
            system_id: "device".to_owned(),
            system_name: "QingYu".to_owned(),
            system_os: "test".to_owned(),
            check_index_id: check_index_id.to_owned(),
            aes_key_verify_val: String::new(),
        }
    }

    fn file(id: &str, path: &str, chunks: &[&str]) -> File {
        File {
            id: id.to_owned(),
            path: path.to_owned(),
            size: 1,
            updated: 1_700_000_000_123,
            chunks: chunks.iter().map(|id| (*id).to_owned()).collect(),
        }
    }

    fn check_index(id: &str, index_id: &str, file_id: &str, chunks: &[&str]) -> CheckIndex {
        CheckIndex {
            id: id.to_owned(),
            index_id: index_id.to_owned(),
            files: vec![CheckIndexFile {
                id: file_id.to_owned(),
                chunks: chunks.iter().map(|id| (*id).to_owned()).collect(),
            }],
        }
    }

    fn fixture() -> PurgeFixture {
        let temp = TempDir::new().unwrap();
        let paths = RepoPaths {
            data: temp.path().join("data"),
            repo: temp.path().join("repo"),
            history: temp.path().join("history"),
            temp: temp.path().join("temp"),
        };
        fs::create_dir_all(&paths.data).unwrap();
        let repo = Repo::open(
            paths,
            Device {
                id: "device".to_owned(),
                name: "QingYu".to_owned(),
                os: "test".to_owned(),
            },
            [9; 32],
            RepoOptions::default(),
        )
        .unwrap();

        for (id, data) in [
            (SHARED_CHUNK, b"shared".as_slice()),
            (RETAINED_CHUNK_REF, b"retained-ref".as_slice()),
            (RETAINED_CHUNK_CALLER, b"retained-caller".as_slice()),
            (UNREACHABLE_CHUNK, b"unreachable".as_slice()),
        ] {
            repo.store
                .put_chunk(&Chunk {
                    id: id.to_owned(),
                    data: data.to_vec(),
                })
                .unwrap();
        }

        let retained_ref_file = file(
            RETAINED_FILE_REF,
            "/retained-ref.md",
            &[SHARED_CHUNK, RETAINED_CHUNK_REF],
        );
        let retained_caller_file = file(
            RETAINED_FILE_CALLER,
            "/retained-caller.md",
            &[SHARED_CHUNK, RETAINED_CHUNK_CALLER],
        );
        let unreferenced_file = file(
            UNREFERENCED_FILE,
            "/unreferenced.md",
            &[SHARED_CHUNK, UNREACHABLE_CHUNK],
        );
        for file in [
            &retained_ref_file,
            &retained_caller_file,
            &unreferenced_file,
        ] {
            repo.store.put_file(file).unwrap();
        }

        let retained_ref_index = index(RETAINED_INDEX_REF, RETAINED_FILE_REF, RETAINED_CHECK_REF);
        let retained_caller_index = index(
            RETAINED_INDEX_CALLER,
            RETAINED_FILE_CALLER,
            RETAINED_CHECK_CALLER,
        );
        let unreferenced_index = index(UNREFERENCED_INDEX, UNREFERENCED_FILE, UNREFERENCED_CHECK);
        for index in [
            &retained_ref_index,
            &retained_caller_index,
            &unreferenced_index,
        ] {
            repo.store.put_index(index).unwrap();
        }
        for check in [
            check_index(
                RETAINED_CHECK_REF,
                RETAINED_INDEX_REF,
                RETAINED_FILE_REF,
                &[SHARED_CHUNK, RETAINED_CHUNK_REF],
            ),
            check_index(
                RETAINED_CHECK_CALLER,
                RETAINED_INDEX_CALLER,
                RETAINED_FILE_CALLER,
                &[SHARED_CHUNK, RETAINED_CHUNK_CALLER],
            ),
            check_index(
                UNREFERENCED_CHECK,
                UNREFERENCED_INDEX,
                UNREFERENCED_FILE,
                &[SHARED_CHUNK, UNREACHABLE_CHUNK],
            ),
        ] {
            repo.store.put_check_index(&check).unwrap();
        }
        RefStore::new(&repo.store)
            .update_latest(&retained_ref_index)
            .unwrap();

        let unreachable_encoded_size = [UNREFERENCED_FILE, UNREACHABLE_CHUNK]
            .into_iter()
            .map(|id| {
                fs::metadata(repo.store.object_path(id).unwrap())
                    .unwrap()
                    .len() as i64
            })
            .sum();

        PurgeFixture {
            _temp: temp,
            repo,
            unreachable_encoded_size,
        }
    }

    fn remote_object_key(id: &str) -> String {
        format!("objects/{}/{}", &id[..2], &id[2..])
    }

    async fn put_remote_raw(
        fixture: &PurgeFixture,
        cloud: &LocalCloud,
        kind: RawObjectKind,
        id: &str,
    ) {
        let key = match kind {
            RawObjectKind::Chunk | RawObjectKind::File => remote_object_key(id),
            RawObjectKind::Index => format!("indexes/{id}"),
            RawObjectKind::CheckIndex => format!("check/indexes/{id}"),
        };
        let bytes = fixture.repo.store.export_raw(kind, id).unwrap();
        cloud.put(&key, &bytes, false).await.unwrap();
    }

    async fn cloud_fixture() -> CloudPurgeFixture {
        let local = fixture();
        let cloud_temp = TempDir::new().unwrap();
        let cloud = Arc::new(LocalCloud::new(cloud_temp.path()).unwrap());
        for id in [
            SHARED_CHUNK,
            RETAINED_CHUNK_REF,
            RETAINED_CHUNK_CALLER,
            UNREACHABLE_CHUNK,
        ] {
            put_remote_raw(&local, &cloud, RawObjectKind::Chunk, id).await;
        }
        for id in [RETAINED_FILE_REF, RETAINED_FILE_CALLER, UNREFERENCED_FILE] {
            put_remote_raw(&local, &cloud, RawObjectKind::File, id).await;
        }
        for id in [
            RETAINED_INDEX_REF,
            RETAINED_INDEX_CALLER,
            UNREFERENCED_INDEX,
        ] {
            put_remote_raw(&local, &cloud, RawObjectKind::Index, id).await;
        }
        for id in [
            RETAINED_CHECK_REF,
            RETAINED_CHECK_CALLER,
            UNREFERENCED_CHECK,
        ] {
            put_remote_raw(&local, &cloud, RawObjectKind::CheckIndex, id).await;
        }
        cloud
            .put("refs/latest", RETAINED_INDEX_REF.as_bytes(), true)
            .await
            .unwrap();
        cloud
            .put("refs/tag-retained", RETAINED_INDEX_CALLER.as_bytes(), true)
            .await
            .unwrap();
        let indexes_v2 = serde_json::to_vec_pretty(&serde_json::json!({
            "indexes": [
                {
                    "id": RETAINED_INDEX_REF,
                    "systemID": "device-a",
                    "systemName": "QingYu A",
                    "systemOS": "test"
                },
                {
                    "id": UNREFERENCED_INDEX,
                    "systemID": "stale",
                    "systemName": "Stale",
                    "systemOS": "test"
                },
                {
                    "id": RETAINED_INDEX_CALLER,
                    "systemID": "device-b",
                    "systemName": "QingYu B",
                    "systemOS": "test"
                }
            ]
        }))
        .unwrap();
        let indexes_v2 = local.repo.store.compress(&indexes_v2).unwrap();
        cloud
            .put("indexes-v2.json", &indexes_v2, true)
            .await
            .unwrap();
        CloudPurgeFixture {
            local,
            _cloud_temp: cloud_temp,
            cloud,
        }
    }

    fn assert_exists(repo: &Repo, id: &str) {
        assert!(repo.store.object_path(id).unwrap().is_file(), "{id}");
    }

    fn assert_unreferenced_candidates_untouched(repo: &Repo) {
        assert!(repo.store.index_path(UNREFERENCED_INDEX).unwrap().is_file());
        assert!(repo
            .store
            .check_index_path(UNREFERENCED_CHECK)
            .unwrap()
            .is_file());
        assert_exists(repo, UNREFERENCED_FILE);
        assert_exists(repo, UNREACHABLE_CHUNK);
    }

    async fn assert_remote_unreferenced_candidates_untouched(cloud: &LocalCloud) {
        assert_eq!(
            cloud
                .list(&format!("indexes/{UNREFERENCED_INDEX}"))
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            cloud
                .list(&format!("check/indexes/{UNREFERENCED_CHECK}"))
                .await
                .unwrap()
                .len(),
            1
        );
        for id in [UNREFERENCED_FILE, UNREACHABLE_CHUNK] {
            assert_eq!(cloud.list(&remote_object_key(id)).await.unwrap().len(), 1);
        }
    }

    async fn assert_remote_purge_stopped_at_indexes_v2(
        cloud: &LocalCloud,
        original_indexes_v2: &[u8],
    ) {
        assert!(cloud
            .list(&format!("indexes/{UNREFERENCED_INDEX}"))
            .await
            .unwrap()
            .is_empty());
        assert!(cloud.list("check/indexes/").await.unwrap().is_empty());
        for id in [UNREFERENCED_FILE, UNREACHABLE_CHUNK] {
            assert_eq!(cloud.list(&remote_object_key(id)).await.unwrap().len(), 1);
        }
        assert_eq!(
            cloud
                .get_bounded("indexes-v2.json", u64::MAX)
                .await
                .unwrap(),
            original_indexes_v2
        );
    }

    fn oversized_zstd_window_frame(decoded_size: usize) -> Vec<u8> {
        assert!(decoded_size <= u32::MAX as usize);
        let mut frame = vec![0x28, 0xb5, 0x2f, 0xfd, 0xa0];
        frame.extend_from_slice(&(decoded_size as u32).to_le_bytes());
        let mut remaining = decoded_size;
        while remaining > 0 {
            let block_size = remaining.min(128 * 1024);
            remaining -= block_size;
            let last_block = usize::from(remaining == 0);
            let header = (block_size << 3) | (1 << 1) | last_block;
            frame.extend_from_slice(&(header as u32).to_le_bytes()[..3]);
            frame.push(b'x');
        }
        frame
    }

    #[tokio::test]
    async fn purge_cloud_preserves_all_ref_reachability_and_is_idempotent() {
        let fixture = cloud_fixture().await;
        let cloud: Arc<dyn Cloud> = fixture.cloud.clone();
        let cancelled = AtomicBool::new(false);

        let stat = fixture
            .local
            .repo
            .purge_cloud(cloud.clone(), &cancelled)
            .await
            .unwrap();

        assert_eq!(stat.indexes, 1);
        assert_eq!(stat.objects, 2);
        assert_eq!(stat.size, fixture.local.unreachable_encoded_size);
        assert!(fixture.cloud.list("lock-sync").await.unwrap().is_empty());
        assert!(
            fixture
                .cloud
                .list(&format!("indexes/{RETAINED_INDEX_REF}"))
                .await
                .unwrap()
                .len()
                == 1
        );
        assert!(
            fixture
                .cloud
                .list(&format!("indexes/{RETAINED_INDEX_CALLER}"))
                .await
                .unwrap()
                .len()
                == 1
        );
        assert!(fixture
            .cloud
            .list(&format!("indexes/{UNREFERENCED_INDEX}"))
            .await
            .unwrap()
            .is_empty());
        assert!(fixture
            .cloud
            .list("check/indexes/")
            .await
            .unwrap()
            .is_empty());
        for id in [
            RETAINED_FILE_REF,
            RETAINED_FILE_CALLER,
            SHARED_CHUNK,
            RETAINED_CHUNK_REF,
            RETAINED_CHUNK_CALLER,
        ] {
            assert_eq!(
                fixture
                    .cloud
                    .list(&remote_object_key(id))
                    .await
                    .unwrap()
                    .len(),
                1,
                "{id}"
            );
        }
        for id in [UNREFERENCED_FILE, UNREACHABLE_CHUNK] {
            assert!(
                fixture
                    .cloud
                    .list(&remote_object_key(id))
                    .await
                    .unwrap()
                    .is_empty(),
                "{id}"
            );
        }
        let indexes_v2 = fixture
            .cloud
            .get_bounded("indexes-v2.json", 1024 * 1024)
            .await
            .unwrap();
        let indexes_v2 = zstd::stream::decode_all(indexes_v2.as_slice()).unwrap();
        let indexes_v2: serde_json::Value = serde_json::from_slice(&indexes_v2).unwrap();
        assert_eq!(
            indexes_v2["indexes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|index| index["id"].as_str().unwrap())
                .collect::<Vec<_>>(),
            [RETAINED_INDEX_REF, RETAINED_INDEX_CALLER]
        );

        assert_eq!(
            fixture
                .local
                .repo
                .purge_cloud(cloud, &cancelled)
                .await
                .unwrap(),
            super::PurgeStat::default()
        );
    }

    #[tokio::test]
    async fn purge_cloud_malformed_indexes_v2_rewrites_null_after_go_decoder_discards_prefix() {
        let fixture = cloud_fixture().await;
        let truncated = format!(
            r#"{{"indexes":[{{"id":"{RETAINED_INDEX_REF}","systemID":"a","systemName":"A","systemOS":"test"}},{{"id":"#
        );
        let encoded = fixture
            .local
            .repo
            .store
            .compress(truncated.as_bytes())
            .unwrap();
        fixture
            .cloud
            .put("indexes-v2.json", &encoded, true)
            .await
            .unwrap();
        let cloud: Arc<dyn Cloud> = fixture.cloud.clone();
        let cancelled = AtomicBool::new(false);

        fixture
            .local
            .repo
            .purge_cloud(cloud, &cancelled)
            .await
            .unwrap();

        let encoded = fixture
            .cloud
            .get_bounded("indexes-v2.json", u64::MAX)
            .await
            .unwrap();
        let decoded = zstd::stream::decode_all(encoded.as_slice()).unwrap();
        let indexes: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
        assert_eq!(indexes["indexes"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn purge_cloud_indexes_v2_rejects_oversized_window_before_object_deletes() {
        let fixture = cloud_fixture().await;
        let encoded = oversized_zstd_window_frame(600_000);
        fixture
            .cloud
            .put("indexes-v2.json", &encoded, true)
            .await
            .unwrap();
        let cloud: Arc<dyn Cloud> = fixture.cloud.clone();
        let cancelled = AtomicBool::new(false);

        let error = fixture
            .local
            .repo
            .purge_cloud(cloud, &cancelled)
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            RepoError::InvalidData(
                "zstd frame is invalid or requires a window larger than 512 KiB"
            )
        ));
        assert_remote_purge_stopped_at_indexes_v2(&fixture.cloud, &encoded).await;
    }

    #[tokio::test]
    async fn purge_cloud_indexes_v2_rejects_truncated_zstd_before_object_deletes() {
        let fixture = cloud_fixture().await;
        let json = serde_json::to_vec(&serde_json::json!({
            "indexes": [{
                "id": RETAINED_INDEX_REF,
                "systemID": "device-a",
                "systemName": "QingYu A",
                "systemOS": "test"
            }]
        }))
        .unwrap();
        let mut encoded = fixture.local.repo.store.compress(&json).unwrap();
        encoded.pop().unwrap();
        fixture
            .cloud
            .put("indexes-v2.json", &encoded, true)
            .await
            .unwrap();
        let cloud: Arc<dyn Cloud> = fixture.cloud.clone();
        let cancelled = AtomicBool::new(false);

        fixture
            .local
            .repo
            .purge_cloud(cloud, &cancelled)
            .await
            .unwrap_err();

        assert_remote_purge_stopped_at_indexes_v2(&fixture.cloud, &encoded).await;
    }

    #[tokio::test]
    async fn purge_cloud_rejects_invalid_refs_before_any_delete() {
        let invalid_refs = [
            ("empty", Vec::new()),
            ("non-utf8", vec![0xff; 40]),
            ("39-bytes", vec![b'a'; 39]),
            ("uppercase", vec![b'A'; 40]),
            ("non-hex", vec![b'g'; 40]),
            ("over-42-bytes", vec![b'a'; 43]),
        ];

        for (name, invalid_ref) in invalid_refs {
            let fixture = cloud_fixture().await;
            fixture
                .cloud
                .put("refs/latest", &invalid_ref, true)
                .await
                .unwrap();
            let cloud: Arc<dyn Cloud> = fixture.cloud.clone();
            let cancelled = AtomicBool::new(false);

            fixture
                .local
                .repo
                .purge_cloud(cloud, &cancelled)
                .await
                .unwrap_err();

            assert_remote_unreferenced_candidates_untouched(&fixture.cloud).await;
            assert!(
                fixture.cloud.list("lock-sync").await.unwrap().is_empty(),
                "{name}"
            );
        }
    }

    #[tokio::test]
    async fn purge_cloud_decodes_remote_ref_index_without_reading_corrupt_local_copy() {
        let fixture = cloud_fixture().await;
        let local_index_path = fixture
            .local
            .repo
            .store
            .index_path(RETAINED_INDEX_REF)
            .unwrap();
        let local_bytes = b"corrupt-local-index";
        fs::write(&local_index_path, local_bytes).unwrap();
        let cloud: Arc<dyn Cloud> = fixture.cloud.clone();
        let cancelled = AtomicBool::new(false);

        fixture
            .local
            .repo
            .purge_cloud(cloud, &cancelled)
            .await
            .unwrap();

        assert_eq!(fs::read(local_index_path).unwrap(), local_bytes);
        for id in [RETAINED_FILE_REF, RETAINED_CHUNK_REF] {
            assert_eq!(
                fixture
                    .cloud
                    .list(&remote_object_key(id))
                    .await
                    .unwrap()
                    .len(),
                1,
                "{id}"
            );
        }
    }

    #[tokio::test]
    async fn purge_cloud_decodes_remote_ref_index_without_replacing_different_valid_local_raw() {
        let fixture = cloud_fixture().await;
        let local_index_path = fixture
            .local
            .repo
            .store
            .index_path(RETAINED_INDEX_REF)
            .unwrap();
        let different = index(RETAINED_INDEX_REF, UNREFERENCED_FILE, RETAINED_CHECK_REF);
        let local_bytes = fixture
            .local
            .repo
            .store
            .compress(serde_json::to_vec(&different).unwrap().as_slice())
            .unwrap();
        fs::write(&local_index_path, &local_bytes).unwrap();
        let cloud: Arc<dyn Cloud> = fixture.cloud.clone();
        let cancelled = AtomicBool::new(false);

        fixture
            .local
            .repo
            .purge_cloud(cloud, &cancelled)
            .await
            .unwrap();

        assert_eq!(fs::read(local_index_path).unwrap(), local_bytes);
        for id in [RETAINED_FILE_REF, RETAINED_CHUNK_REF] {
            assert_eq!(
                fixture
                    .cloud
                    .list(&remote_object_key(id))
                    .await
                    .unwrap()
                    .len(),
                1,
                "{id}"
            );
        }
    }

    #[tokio::test]
    async fn purge_cloud_missing_or_corrupt_referenced_index_is_retained_but_protects_no_objects() {
        for corrupt in [false, true] {
            let fixture = cloud_fixture().await;
            let index_key = format!("indexes/{RETAINED_INDEX_REF}");
            if corrupt {
                fixture
                    .cloud
                    .put(&index_key, b"not-a-zstd-index", true)
                    .await
                    .unwrap();
            } else {
                fixture.cloud.remove(&index_key).await.unwrap();
            }
            let cloud: Arc<dyn Cloud> = fixture.cloud.clone();
            let cancelled = AtomicBool::new(false);

            let stat = fixture
                .local
                .repo
                .purge_cloud(cloud, &cancelled)
                .await
                .unwrap();

            assert_eq!(stat.indexes, 1);
            assert!(fixture.cloud.list(&index_key).await.unwrap().len() == usize::from(corrupt));
            for id in [RETAINED_FILE_REF, RETAINED_CHUNK_REF] {
                assert!(fixture
                    .cloud
                    .list(&remote_object_key(id))
                    .await
                    .unwrap()
                    .is_empty());
            }
            for id in [RETAINED_FILE_CALLER, SHARED_CHUNK, RETAINED_CHUNK_CALLER] {
                assert_eq!(
                    fixture
                        .cloud
                        .list(&remote_object_key(id))
                        .await
                        .unwrap()
                        .len(),
                    1
                );
            }
        }
    }

    #[tokio::test]
    async fn purge_cloud_missing_or_corrupt_referenced_file_aborts_before_deletion() {
        for corrupt in [false, true] {
            let fixture = cloud_fixture().await;
            fs::remove_file(
                fixture
                    .local
                    .repo
                    .store
                    .object_path(RETAINED_FILE_REF)
                    .unwrap(),
            )
            .unwrap();
            let file_key = remote_object_key(RETAINED_FILE_REF);
            if corrupt {
                fixture
                    .cloud
                    .put(&file_key, b"not-an-encrypted-file", true)
                    .await
                    .unwrap();
            } else {
                fixture.cloud.remove(&file_key).await.unwrap();
            }
            let cloud: Arc<dyn Cloud> = fixture.cloud.clone();
            let cancelled = AtomicBool::new(false);

            let error = fixture
                .local
                .repo
                .purge_cloud(cloud, &cancelled)
                .await
                .unwrap_err();

            if corrupt {
                assert!(!matches!(error, RepoError::Cloud(CloudError::NotFound)));
            } else {
                assert!(matches!(error, RepoError::Cloud(CloudError::NotFound)));
            }
            assert_remote_unreferenced_candidates_untouched(&fixture.cloud).await;
        }
    }

    #[tokio::test]
    async fn purge_cloud_imports_missing_referenced_file_metadata_and_preserves_its_chunks() {
        let fixture = cloud_fixture().await;
        let local_path = fixture
            .local
            .repo
            .store
            .object_path(RETAINED_FILE_REF)
            .unwrap();
        fs::remove_file(&local_path).unwrap();
        let remote_raw = fixture
            .cloud
            .get_bounded(&remote_object_key(RETAINED_FILE_REF), u64::MAX)
            .await
            .unwrap();
        let cloud: Arc<dyn Cloud> = fixture.cloud.clone();
        let cancelled = AtomicBool::new(false);

        fixture
            .local
            .repo
            .purge_cloud(cloud, &cancelled)
            .await
            .unwrap();

        assert_eq!(
            fixture
                .local
                .repo
                .store
                .export_raw(RawObjectKind::File, RETAINED_FILE_REF)
                .unwrap(),
            remote_raw
        );
        assert_eq!(
            fixture
                .local
                .repo
                .store
                .get_file(RETAINED_FILE_REF)
                .unwrap()
                .chunks,
            [SHARED_CHUNK, RETAINED_CHUNK_REF]
        );
        for id in [SHARED_CHUNK, RETAINED_CHUNK_REF] {
            assert_eq!(
                fixture
                    .cloud
                    .list(&remote_object_key(id))
                    .await
                    .unwrap()
                    .len(),
                1
            );
        }
    }

    #[tokio::test]
    async fn purge_cloud_ref_disappearing_after_list_aborts_before_deletion() {
        let fixture = cloud_fixture().await;
        let cancelled = Arc::new(AtomicBool::new(false));
        let cloud: Arc<dyn Cloud> = Arc::new(HookCloud::new(
            fixture.cloud.clone(),
            cancelled.clone(),
            CloudHooks {
                remove_first_ref_after_list: true,
                ..CloudHooks::default()
            },
        ));

        let error = fixture
            .local
            .repo
            .purge_cloud(cloud, cancelled.as_ref())
            .await
            .unwrap_err();

        assert!(matches!(error, RepoError::Cloud(CloudError::NotFound)));
        assert_remote_unreferenced_candidates_untouched(&fixture.cloud).await;
    }

    #[tokio::test]
    async fn purge_cloud_cancellation_before_first_delete_preserves_every_candidate() {
        let fixture = cloud_fixture().await;
        let cancelled = Arc::new(AtomicBool::new(false));
        let cloud: Arc<dyn Cloud> = Arc::new(HookCloud::new(
            fixture.cloud.clone(),
            cancelled.clone(),
            CloudHooks {
                cancel_before_delete: true,
                ..CloudHooks::default()
            },
        ));

        let error = fixture
            .local
            .repo
            .purge_cloud(cloud, cancelled.as_ref())
            .await
            .unwrap_err();

        assert!(matches!(error, RepoError::Cancelled));
        assert_remote_unreferenced_candidates_untouched(&fixture.cloud).await;
        assert!(fixture.cloud.list("lock-sync").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn purge_cloud_cancellation_during_deletion_stops_following_destructive_phases() {
        let fixture = cloud_fixture().await;
        let cancelled = Arc::new(AtomicBool::new(false));
        let cloud: Arc<dyn Cloud> = Arc::new(HookCloud::new(
            fixture.cloud.clone(),
            cancelled.clone(),
            CloudHooks {
                cancel_after_first_repository_remove: true,
                ..CloudHooks::default()
            },
        ));

        let error = fixture
            .local
            .repo
            .purge_cloud(cloud, cancelled.as_ref())
            .await
            .unwrap_err();

        assert!(matches!(error, RepoError::Cancelled));
        assert_eq!(fixture.cloud.list("check/indexes/").await.unwrap().len(), 2);
        assert_remote_unreferenced_candidates_untouched(&fixture.cloud).await;
        assert!(fixture.cloud.list("lock-sync").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn purge_cloud_ignores_legacy_check_index_list_failure_like_go() {
        let fixture = cloud_fixture().await;
        let cancelled = Arc::new(AtomicBool::new(false));
        let cloud: Arc<dyn Cloud> = Arc::new(HookCloud::new(
            fixture.cloud.clone(),
            cancelled.clone(),
            CloudHooks {
                fail_check_index_list: true,
                ..CloudHooks::default()
            },
        ));

        let stat = fixture
            .local
            .repo
            .purge_cloud(cloud, cancelled.as_ref())
            .await
            .unwrap();

        assert_eq!(stat.indexes, 1);
        assert_eq!(stat.objects, 2);
        assert_eq!(fixture.cloud.list("check/indexes/").await.unwrap().len(), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn purge_cloud_lock_refresh_loss_aborts_before_deletion() {
        let fixture = cloud_fixture().await;
        let cancelled = Arc::new(AtomicBool::new(false));
        let cloud: Arc<dyn Cloud> = Arc::new(HookCloud::new(
            fixture.cloud.clone(),
            cancelled.clone(),
            CloudHooks {
                fail_refresh_during_object_list: true,
                ..CloudHooks::default()
            },
        ));

        let error = fixture
            .local
            .repo
            .purge_cloud(cloud, cancelled.as_ref())
            .await
            .unwrap_err();

        assert!(matches!(error, RepoError::RemoteLockUnhealthy(_)));
        assert_remote_unreferenced_candidates_untouched(&fixture.cloud).await;
        assert!(fixture.cloud.list("lock-sync").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn purge_cloud_empty_object_or_index_set_short_circuits_before_refs_and_cleanup() {
        for remove_objects in [false, true] {
            let fixture = cloud_fixture().await;
            if remove_objects {
                for object in fixture.cloud.list("objects/").await.unwrap() {
                    fixture.cloud.remove(&object.key).await.unwrap();
                }
            } else {
                for index in fixture.cloud.list("indexes/").await.unwrap() {
                    fixture.cloud.remove(&index.key).await.unwrap();
                }
            }
            let indexes_v2_before = fixture
                .cloud
                .get_bounded("indexes-v2.json", u64::MAX)
                .await
                .unwrap();
            let cloud: Arc<dyn Cloud> = fixture.cloud.clone();
            let cancelled = AtomicBool::new(false);

            let stat = fixture
                .local
                .repo
                .purge_cloud(cloud, &cancelled)
                .await
                .unwrap();

            assert_eq!(stat, super::PurgeStat::default());
            assert_eq!(fixture.cloud.list("check/indexes/").await.unwrap().len(), 3);
            assert_eq!(
                fixture
                    .cloud
                    .get_bounded("indexes-v2.json", u64::MAX)
                    .await
                    .unwrap(),
                indexes_v2_before
            );
        }
    }

    #[tokio::test]
    async fn purge_cloud_reports_unlock_failure_after_successful_short_circuit() {
        let fixture = cloud_fixture().await;
        for index in fixture.cloud.list("indexes/").await.unwrap() {
            fixture.cloud.remove(&index.key).await.unwrap();
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        let cloud: Arc<dyn Cloud> = Arc::new(HookCloud::new(
            fixture.cloud.clone(),
            cancelled.clone(),
            CloudHooks {
                fail_unlock_after_index_list: true,
                ..CloudHooks::default()
            },
        ));

        let error = fixture
            .local
            .repo
            .purge_cloud(cloud, cancelled.as_ref())
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            RepoError::Cloud(CloudError::UnlockFailed { .. })
        ));
    }

    #[tokio::test]
    async fn purge_cloud_preserves_operation_and_unlock_failures_together() {
        let fixture = cloud_fixture().await;
        let cancelled = Arc::new(AtomicBool::new(false));
        let cloud: Arc<dyn Cloud> = Arc::new(HookCloud::new(
            fixture.cloud.clone(),
            cancelled.clone(),
            CloudHooks {
                remove_first_ref_after_list: true,
                fail_unlock_after_ref_list: true,
                ..CloudHooks::default()
            },
        ));

        let error = fixture
            .local
            .repo
            .purge_cloud(cloud, cancelled.as_ref())
            .await
            .unwrap_err();

        let RepoError::OperationAndUnlockFailed { operation, unlock } = error else {
            panic!("expected combined operation and unlock error");
        };
        assert!(matches!(*operation, RepoError::Cloud(CloudError::NotFound)));
        assert!(matches!(unlock, CloudError::UnlockFailed { .. }));
    }

    #[test]
    fn purge_preserves_reachable_indexes_files_and_shared_chunks() {
        let fixture = fixture();
        let cancelled = AtomicBool::new(false);

        let stat = fixture
            .repo
            .purge(&[RETAINED_INDEX_CALLER.to_owned()], &cancelled)
            .unwrap();

        assert_eq!(stat.indexes, 1);
        assert_eq!(stat.objects, 2);
        assert_eq!(stat.size, fixture.unreachable_encoded_size);
        assert!(fixture
            .repo
            .store
            .index_path(RETAINED_INDEX_REF)
            .unwrap()
            .is_file());
        assert!(fixture
            .repo
            .store
            .index_path(RETAINED_INDEX_CALLER)
            .unwrap()
            .is_file());
        assert!(!fixture
            .repo
            .store
            .index_path(UNREFERENCED_INDEX)
            .unwrap()
            .exists());
        for id in [
            RETAINED_FILE_REF,
            RETAINED_FILE_CALLER,
            SHARED_CHUNK,
            RETAINED_CHUNK_REF,
            RETAINED_CHUNK_CALLER,
        ] {
            assert_exists(&fixture.repo, id);
        }
        for id in [UNREFERENCED_FILE, UNREACHABLE_CHUNK] {
            assert!(
                !fixture.repo.store.object_path(id).unwrap().exists(),
                "{id}"
            );
        }
        for id in [RETAINED_CHECK_REF, RETAINED_CHECK_CALLER] {
            assert!(fixture.repo.store.check_index_path(id).unwrap().is_file());
        }
        assert!(!fixture
            .repo
            .store
            .check_index_path(UNREFERENCED_CHECK)
            .unwrap()
            .exists());
    }

    #[test]
    fn cancellation_after_index_deletion_stops_later_destructive_loops() {
        let fixture = fixture();
        let unreferenced_index_path = fixture.repo.store.index_path(UNREFERENCED_INDEX).unwrap();

        let result = purge_store_with_cancel_check(&fixture.repo.store, &[], || {
            !unreferenced_index_path.exists()
        });

        assert!(matches!(result, Err(RepoError::Cancelled)));
        assert!(!unreferenced_index_path.exists());
        assert!(fixture
            .repo
            .store
            .check_index_path(UNREFERENCED_CHECK)
            .unwrap()
            .is_file());
        assert_exists(&fixture.repo, UNREFERENCED_FILE);
        assert_exists(&fixture.repo, UNREACHABLE_CHUNK);
    }

    #[test]
    fn already_cancelled_purge_preserves_every_object() {
        let fixture = fixture();
        let cancelled = AtomicBool::new(true);

        assert!(matches!(
            fixture.repo.purge(&[], &cancelled),
            Err(RepoError::Cancelled)
        ));
        assert!(fixture
            .repo
            .store
            .index_path(UNREFERENCED_INDEX)
            .unwrap()
            .is_file());
        assert!(fixture
            .repo
            .store
            .check_index_path(UNREFERENCED_CHECK)
            .unwrap()
            .is_file());
        assert_exists(&fixture.repo, UNREFERENCED_FILE);
        assert_exists(&fixture.repo, UNREACHABLE_CHUNK);
    }

    #[test]
    fn cancellation_is_observed_inside_object_index_and_recursive_ref_collection() {
        let fixture = fixture();
        fs::create_dir_all(fixture._temp.path().join("repo/refs/tags/nested")).unwrap();
        fs::write(
            fixture._temp.path().join("repo/refs/tags/nested/retained"),
            RETAINED_INDEX_REF,
        )
        .unwrap();

        let mut object_checks = 0;
        assert!(matches!(
            collect_object_ids(&fixture.repo.store, &mut || {
                object_checks += 1;
                object_checks == 3
            }),
            Err(RepoError::Cancelled)
        ));
        assert_eq!(object_checks, 3);

        let mut index_checks = 0;
        assert!(matches!(
            collect_flat_ids(&fixture.repo.store, Path::new("indexes"), &mut || {
                index_checks += 1;
                index_checks == 3
            },),
            Err(RepoError::Cancelled)
        ));
        assert_eq!(index_checks, 3);

        let mut ref_checks = 0;
        assert!(matches!(
            RefStore::new(&fixture.repo.store).all_index_ids_unlocked_with_cancel_check(
                &mut || {
                    ref_checks += 1;
                    ref_checks == 3
                }
            ),
            Err(RepoError::Cancelled)
        ));
        assert_eq!(ref_checks, 3);
        assert_unreferenced_candidates_untouched(&fixture.repo);
    }

    #[test]
    fn missing_retained_index_aborts_before_any_deletion() {
        let fixture = fixture();
        fs::remove_file(fixture.repo.store.index_path(RETAINED_INDEX_REF).unwrap()).unwrap();

        assert!(fixture
            .repo
            .purge(&[RETAINED_INDEX_CALLER.to_owned()], &AtomicBool::new(false))
            .is_err());
        assert_unreferenced_candidates_untouched(&fixture.repo);
    }

    #[test]
    fn corrupt_retained_index_aborts_before_any_deletion() {
        let fixture = fixture();
        fs::write(
            fixture.repo.store.index_path(RETAINED_INDEX_REF).unwrap(),
            b"not a zstd index",
        )
        .unwrap();

        assert!(fixture
            .repo
            .purge(&[RETAINED_INDEX_CALLER.to_owned()], &AtomicBool::new(false))
            .is_err());
        assert_unreferenced_candidates_untouched(&fixture.repo);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_retained_index_aborts_before_any_deletion() {
        use std::os::unix::fs::symlink;

        let fixture = fixture();
        let path = fixture.repo.store.index_path(RETAINED_INDEX_REF).unwrap();
        fs::remove_file(&path).unwrap();
        symlink(
            fixture
                .repo
                .store
                .index_path(RETAINED_INDEX_CALLER)
                .unwrap(),
            path,
        )
        .unwrap();

        assert!(matches!(
            fixture
                .repo
                .purge(&[RETAINED_INDEX_CALLER.to_owned()], &AtomicBool::new(false)),
            Err(RepoError::UnsafePath)
        ));
        assert_unreferenced_candidates_untouched(&fixture.repo);
    }

    #[test]
    fn missing_retained_file_aborts_before_any_deletion() {
        let fixture = fixture();
        fs::remove_file(fixture.repo.store.object_path(RETAINED_FILE_REF).unwrap()).unwrap();

        assert!(fixture
            .repo
            .purge(&[RETAINED_INDEX_CALLER.to_owned()], &AtomicBool::new(false))
            .is_err());
        assert_unreferenced_candidates_untouched(&fixture.repo);
    }

    #[test]
    fn corrupt_retained_file_aborts_before_any_deletion() {
        let fixture = fixture();
        fs::write(
            fixture.repo.store.object_path(RETAINED_FILE_REF).unwrap(),
            b"not an encrypted file object",
        )
        .unwrap();

        assert!(fixture
            .repo
            .purge(&[RETAINED_INDEX_CALLER.to_owned()], &AtomicBool::new(false))
            .is_err());
        assert_unreferenced_candidates_untouched(&fixture.repo);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_retained_file_aborts_before_any_deletion() {
        use std::os::unix::fs::symlink;

        let fixture = fixture();
        let path = fixture.repo.store.object_path(RETAINED_FILE_REF).unwrap();
        fs::remove_file(&path).unwrap();
        symlink(
            fixture
                .repo
                .store
                .object_path(RETAINED_FILE_CALLER)
                .unwrap(),
            path,
        )
        .unwrap();

        assert!(matches!(
            fixture
                .repo
                .purge(&[RETAINED_INDEX_CALLER.to_owned()], &AtomicBool::new(false)),
            Err(RepoError::UnsafePath)
        ));
        assert_unreferenced_candidates_untouched(&fixture.repo);
    }

    #[test]
    fn corrupt_check_index_aborts_before_any_deletion() {
        let fixture = fixture();
        fs::write(
            fixture
                .repo
                .store
                .check_index_path(UNREFERENCED_CHECK)
                .unwrap(),
            b"not a zstd check index",
        )
        .unwrap();

        assert!(fixture
            .repo
            .purge(&[RETAINED_INDEX_CALLER.to_owned()], &AtomicBool::new(false))
            .is_err());
        assert_unreferenced_candidates_untouched(&fixture.repo);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_check_index_aborts_before_any_deletion() {
        use std::os::unix::fs::symlink;

        let fixture = fixture();
        let path = fixture
            .repo
            .store
            .check_index_path(UNREFERENCED_CHECK)
            .unwrap();
        fs::remove_file(&path).unwrap();
        symlink(
            fixture
                .repo
                .store
                .check_index_path(RETAINED_CHECK_REF)
                .unwrap(),
            path,
        )
        .unwrap();

        assert!(matches!(
            fixture
                .repo
                .purge(&[RETAINED_INDEX_CALLER.to_owned()], &AtomicBool::new(false)),
            Err(RepoError::UnsafePath)
        ));
        assert_unreferenced_candidates_untouched(&fixture.repo);
    }

    #[test]
    fn check_index_payload_id_must_match_its_filename() {
        let fixture = fixture();
        let mismatch = check_index(
            MISMATCH_CHECK_ID,
            UNREFERENCED_INDEX,
            UNREFERENCED_FILE,
            &[UNREACHABLE_CHUNK],
        );
        fixture.repo.store.put_check_index(&mismatch).unwrap();
        fs::rename(
            fixture
                .repo
                .store
                .check_index_path(MISMATCH_CHECK_ID)
                .unwrap(),
            fixture
                .repo
                .store
                .check_index_path(UNREFERENCED_CHECK)
                .unwrap(),
        )
        .unwrap();

        assert!(matches!(
            fixture
                .repo
                .purge(&[RETAINED_INDEX_CALLER.to_owned()], &AtomicBool::new(false)),
            Err(RepoError::InvalidData(_))
        ));
        assert_unreferenced_candidates_untouched(&fixture.repo);
    }

    #[test]
    fn purge_delete_phase_excludes_cross_open_ref_and_index_publication() {
        let fixture = fixture();
        let second_repo = Repo::open(
            RepoPaths {
                data: fixture._temp.path().join("data"),
                repo: fixture._temp.path().join("repo"),
                history: fixture._temp.path().join("history-2"),
                temp: fixture._temp.path().join("temp-2"),
            },
            Device {
                id: "second-device".to_owned(),
                name: "QingYu".to_owned(),
                os: "test".to_owned(),
            },
            [9; 32],
            RepoOptions::default(),
        )
        .unwrap();
        let ref_target = index(UNREFERENCED_INDEX, UNREFERENCED_FILE, UNREFERENCED_CHECK);
        let mut concurrent_index = index(CONCURRENT_INDEX, UNREFERENCED_FILE, "");
        concurrent_index.files.clear();
        concurrent_index.count = 0;
        concurrent_index.size = 0;
        let cancelled = AtomicBool::new(false);
        let (before_delete_tx, before_delete_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (ref_started_tx, ref_started_rx) = mpsc::channel();
        let (ref_result_tx, ref_result_rx) = mpsc::channel();
        let (index_started_tx, index_started_rx) = mpsc::channel();
        let (index_result_tx, index_result_rx) = mpsc::channel();

        std::thread::scope(|scope| {
            let purge_repo = &fixture.repo;
            let cancelled = &cancelled;
            let purge_thread = scope.spawn(move || {
                purge_repo.purge_with_before_delete_hook(
                    &[RETAINED_INDEX_CALLER.to_owned()],
                    cancelled,
                    || {
                        before_delete_tx
                            .send(())
                            .map_err(|_| RepoError::RepoFatal)?;
                        release_rx.recv().map_err(|_| RepoError::RepoFatal)?;
                        Ok(())
                    },
                )
            });
            before_delete_rx.recv().unwrap();

            let ref_repo = &second_repo;
            scope.spawn(move || {
                ref_started_tx.send(()).unwrap();
                ref_result_tx
                    .send(RefStore::new(&ref_repo.store).update_latest(&ref_target))
                    .unwrap();
            });
            let index_repo = &second_repo;
            scope.spawn(move || {
                index_started_tx.send(()).unwrap();
                index_result_tx
                    .send(index_repo.store.put_index(&concurrent_index))
                    .unwrap();
            });
            ref_started_rx.recv().unwrap();
            index_started_rx.recv().unwrap();

            let premature_ref = ref_result_rx.recv_timeout(Duration::from_millis(100)).ok();
            let premature_index = index_result_rx
                .recv_timeout(Duration::from_millis(100))
                .ok();
            let ref_was_premature = premature_ref.is_some();
            let index_was_premature = premature_index.is_some();
            release_tx.send(()).unwrap();
            let purge_result = purge_thread.join().unwrap();
            let ref_result = premature_ref.unwrap_or_else(|| ref_result_rx.recv().unwrap());
            let index_result = premature_index.unwrap_or_else(|| index_result_rx.recv().unwrap());

            assert!(ref_was_premature, "ref publication did not fail fast");
            assert!(index_was_premature, "index publication did not fail fast");
            purge_result.unwrap();
            assert!(matches!(ref_result, Err(RepoError::RepositoryBusy)));
            assert!(matches!(index_result, Err(RepoError::RepositoryBusy)));
        });

        assert!(!fixture
            .repo
            .store
            .index_path(UNREFERENCED_INDEX)
            .unwrap()
            .exists());
        assert!(!fixture
            .repo
            .store
            .index_path(CONCURRENT_INDEX)
            .unwrap()
            .is_file());
        assert_eq!(
            RefStore::new(&fixture.repo.store)
                .latest()
                .unwrap()
                .unwrap()
                .id,
            RETAINED_INDEX_REF
        );
    }

    #[test]
    fn purge_guard_survives_repository_materialize_rename_and_reopen() {
        let fixture = fixture();
        let original_repo_root = fixture._temp.path().join("repo");
        let moved_repo_root = fixture._temp.path().join("repo-moved");
        assert!(original_repo_root.is_dir());
        fs::rename(&original_repo_root, &moved_repo_root).unwrap();

        let reopened_repo = Repo::open(
            RepoPaths {
                data: fixture._temp.path().join("data"),
                repo: moved_repo_root.clone(),
                history: fixture._temp.path().join("history-moved"),
                temp: fixture._temp.path().join("temp-moved"),
            },
            Device {
                id: "moved-device".to_owned(),
                name: "QingYu".to_owned(),
                os: "test".to_owned(),
            },
            [9; 32],
            RepoOptions::default(),
        )
        .unwrap();
        let reopened_store = Store::new(&moved_repo_root, [9; 32]).unwrap();
        let ref_target = index(UNREFERENCED_INDEX, UNREFERENCED_FILE, UNREFERENCED_CHECK);
        let mut concurrent_index = index(CONCURRENT_INDEX, UNREFERENCED_FILE, "");
        concurrent_index.files.clear();
        concurrent_index.count = 0;
        concurrent_index.size = 0;
        let concurrent_chunk = Chunk {
            id: CONCURRENT_CHUNK.to_owned(),
            data: b"published after purge".to_vec(),
        };
        let cancelled = AtomicBool::new(false);
        let (before_delete_tx, before_delete_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (ref_started_tx, ref_started_rx) = mpsc::channel();
        let (ref_result_tx, ref_result_rx) = mpsc::channel();
        let (index_started_tx, index_started_rx) = mpsc::channel();
        let (index_result_tx, index_result_rx) = mpsc::channel();
        let (chunk_started_tx, chunk_started_rx) = mpsc::channel();
        let (chunk_result_tx, chunk_result_rx) = mpsc::channel();

        std::thread::scope(|scope| {
            let purge_repo = &fixture.repo;
            let cancelled = &cancelled;
            let purge_thread = scope.spawn(move || {
                purge_repo.purge_with_before_delete_hook(
                    &[RETAINED_INDEX_CALLER.to_owned()],
                    cancelled,
                    || {
                        before_delete_tx
                            .send(())
                            .map_err(|_| RepoError::RepoFatal)?;
                        release_rx.recv().map_err(|_| RepoError::RepoFatal)?;
                        Ok(())
                    },
                )
            });
            before_delete_rx.recv().unwrap();

            let ref_repo = &reopened_repo;
            scope.spawn(move || {
                ref_started_tx.send(()).unwrap();
                ref_result_tx
                    .send(RefStore::new(&ref_repo.store).update_latest(&ref_target))
                    .unwrap();
            });
            let index_store = &reopened_store;
            scope.spawn(move || {
                index_started_tx.send(()).unwrap();
                index_result_tx
                    .send(index_store.put_index(&concurrent_index))
                    .unwrap();
            });
            let chunk_store = &reopened_store;
            scope.spawn(move || {
                chunk_started_tx.send(()).unwrap();
                chunk_result_tx
                    .send(chunk_store.put_chunk(&concurrent_chunk))
                    .unwrap();
            });
            ref_started_rx.recv().unwrap();
            index_started_rx.recv().unwrap();
            chunk_started_rx.recv().unwrap();

            let premature_ref = ref_result_rx.recv_timeout(Duration::from_millis(100)).ok();
            let premature_index = index_result_rx
                .recv_timeout(Duration::from_millis(100))
                .ok();
            let premature_chunk = chunk_result_rx
                .recv_timeout(Duration::from_millis(100))
                .ok();
            let ref_was_premature = premature_ref.is_some();
            let index_was_premature = premature_index.is_some();
            let chunk_was_premature = premature_chunk.is_some();
            release_tx.send(()).unwrap();
            let purge_result = purge_thread.join().unwrap();
            let ref_result = premature_ref.unwrap_or_else(|| ref_result_rx.recv().unwrap());
            let index_result = premature_index.unwrap_or_else(|| index_result_rx.recv().unwrap());
            let chunk_result = premature_chunk.unwrap_or_else(|| chunk_result_rx.recv().unwrap());

            assert!(ref_was_premature, "renamed-path ref did not fail fast");
            assert!(
                index_was_premature && chunk_was_premature,
                "same-repository publications must both fail before the std guard"
            );
            purge_result.unwrap();
            assert!(matches!(ref_result, Err(RepoError::RepositoryBusy)));
            assert_eq!(
                [&index_result, &chunk_result]
                    .into_iter()
                    .filter(|result| matches!(result, Err(RepoError::RepositoryBusy)))
                    .count(),
                2
            );
        });

        let mut retry_index = index(CONCURRENT_INDEX, UNREFERENCED_FILE, "");
        retry_index.files.clear();
        retry_index.count = 0;
        retry_index.size = 0;
        reopened_store.put_index(&retry_index).unwrap();
        reopened_store
            .put_chunk(&Chunk {
                id: CONCURRENT_CHUNK.to_owned(),
                data: b"published after purge".to_vec(),
            })
            .unwrap();

        assert!(!reopened_store
            .index_path(UNREFERENCED_INDEX)
            .unwrap()
            .exists());
        assert!(reopened_store
            .index_path(CONCURRENT_INDEX)
            .unwrap()
            .is_file());
        assert_eq!(
            reopened_store.get_chunk(CONCURRENT_CHUNK).unwrap().data,
            b"published after purge"
        );
        assert_eq!(
            RefStore::new(&reopened_store).latest().unwrap().unwrap().id,
            RETAINED_INDEX_REF
        );
    }
}
