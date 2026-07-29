use std::io::{Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use cap_std::fs::{Dir, File as CapFile};
use ignore::gitignore::{Gitignore, GitignoreBuilder};

use crate::atomic_write::{create_cap_staged_file, is_owned_stage_name, CapStagedFile};
use crate::cloud::{Cloud, CloudError};
use crate::indexer::{self, IndexHook, NoopIndexHook};
use crate::path_security::{
    cap_metadata_is_reparse, std_metadata_is_reparse,
    validate_windows_directory_components_before_canonicalize,
};
use crate::purge::{purge_store_with_cancel_check, PurgeStat};
use crate::store::{
    open_absolute_dir_nofollow, open_child_directory, open_or_create_absolute_dir_nofollow,
};
use crate::sync_lock::{acquire_remote_lock, RemoteLockGuard};
use crate::{File, History, Index, RefStore, RepoError, Store};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepoPaths {
    pub data: PathBuf,
    pub repo: PathBuf,
    pub history: PathBuf,
    pub temp: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub os: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RepoOptions {
    pub ignore_lines: Vec<String>,
    pub protected_include_paths: Vec<String>,
}

pub struct Repo {
    pub(crate) data_dir: Dir,
    pub(crate) data_gate: crate::lifecycle::LifecycleGate,
    pub(crate) device: Device,
    pub(crate) key: [u8; 32],
    pub(crate) protected_include_paths: Vec<String>,
    pub(crate) ignore_matcher: Gitignore,
    pub(crate) store: Store,
    pub(crate) temp_dir: Dir,
    pub(crate) temp_gate: crate::lifecycle::LifecycleGate,
    pub(crate) history: History,
    pub(crate) index_hook: Arc<dyn IndexHook>,
}

impl Repo {
    pub fn open(
        paths: RepoPaths,
        device: Device,
        key: [u8; 32],
        options: RepoOptions,
    ) -> Result<Self, RepoError> {
        let runtime = crate::RepositoryRuntimeState::default();
        Self::open_with_runtime(paths, device, key, options, &runtime)
    }

    pub fn open_with_runtime(
        paths: RepoPaths,
        device: Device,
        key: [u8; 32],
        options: RepoOptions,
        runtime: &crate::RepositoryRuntimeState,
    ) -> Result<Self, RepoError> {
        Self::open_inner(
            paths,
            device,
            key,
            options,
            runtime,
            Arc::new(NoopIndexHook),
        )
    }

    #[cfg(test)]
    pub(crate) fn open_with_hook(
        paths: RepoPaths,
        device: Device,
        key: [u8; 32],
        options: RepoOptions,
        index_hook: Arc<dyn IndexHook>,
    ) -> Result<Self, RepoError> {
        let runtime = crate::RepositoryRuntimeState::default();
        Self::open_inner(paths, device, key, options, &runtime, index_hook)
    }

    fn open_inner(
        paths: RepoPaths,
        device: Device,
        key: [u8; 32],
        options: RepoOptions,
        runtime: &crate::RepositoryRuntimeState,
        index_hook: Arc<dyn IndexHook>,
    ) -> Result<Self, RepoError> {
        let paths = RepoPaths {
            data: normalize_root(&paths.data)?,
            repo: normalize_root(&paths.repo)?,
            history: normalize_root(&paths.history)?,
            temp: normalize_root(&paths.temp)?,
        };
        validate_root_has_no_symlinks(&paths.data)?;
        validate_root_has_no_symlinks(&paths.repo)?;
        validate_root_has_no_symlinks(&paths.history)?;
        validate_root_has_no_symlinks(&paths.temp)?;
        let mut protected_include_paths = options
            .protected_include_paths
            .iter()
            .map(|path| normalize_protected_path(path))
            .collect::<Result<Vec<_>, _>>()?;
        protected_include_paths.sort();
        protected_include_paths.dedup();

        let mut ignore_builder = GitignoreBuilder::new(&paths.data);
        for line in &options.ignore_lines {
            ignore_builder
                .add_line(None, line)
                .map_err(|_| RepoError::RepoFatal)?;
        }
        let ignore_matcher = ignore_builder.build().map_err(|_| RepoError::RepoFatal)?;
        let data_dir = open_absolute_dir_nofollow(&paths.data)?;
        let data_metadata = data_dir.dir_metadata()?;
        if !data_metadata.file_type().is_dir() || cap_metadata_is_reparse(&data_metadata) {
            return Err(RepoError::UnsafePath);
        }
        let data_gate = crate::lifecycle::LifecycleGate::for_directory(&data_dir, runtime)?;
        let store = Store::new_with_runtime(&paths.repo, key, runtime)?;
        let history = History::new(&paths.history)?;
        let temp_dir = open_or_create_absolute_dir_nofollow(&paths.temp)?;
        let temp_gate = crate::lifecycle::LifecycleGate::for_directory(&temp_dir, runtime)?;
        match temp_gate.try_acquire() {
            Ok(_cleanup_guard) => cleanup_abandoned_downloads(&temp_dir)?,
            Err(RepoError::RepositoryBusy) => {}
            Err(error) => return Err(error),
        }

        Ok(Self {
            data_dir,
            data_gate,
            device,
            key,
            protected_include_paths,
            ignore_matcher,
            store,
            temp_dir,
            temp_gate,
            history,
            index_hook,
        })
    }

    pub(crate) fn create_staged_download(&self) -> Result<RepoStagedDownload, RepoError> {
        let temp_guard = self.temp_gate.try_acquire()?;
        Ok(RepoStagedDownload {
            staged: create_cap_staged_file(
                &self.temp_dir,
                std::ffi::OsStr::new("download.tmp"),
                0o600,
            )?,
            _temp_guard: temp_guard,
        })
    }

    pub(crate) async fn stage_cloud_download(
        &self,
        cloud: &Arc<dyn Cloud>,
        key: &str,
    ) -> Result<(RepoStagedDownload, u64), RepoError> {
        let staged = self.create_staged_download()?;
        let mut writer = staged.writer()?;
        let written = cloud.download_to(key, &mut writer).await?;
        tokio::io::AsyncWriteExt::flush(&mut writer).await?;
        writer.sync_all().await?;
        drop(writer);
        if staged.file().metadata()?.len() != written {
            return Err(RepoError::InvalidData(
                "cloud download returned an invalid payload length",
            ));
        }
        Ok((staged, written))
    }

    pub(crate) async fn download_raw_to_store(
        &self,
        cloud: &Arc<dyn Cloud>,
        key: &str,
        kind: crate::RawObjectKind,
        id: &str,
    ) -> Result<u64, RepoError> {
        let (staged, written) = self.stage_cloud_download(cloud, key).await?;
        let _operation = self.store.lock_operation()?;
        self.store
            .import_raw_staged_unlocked(kind, id, staged.file())?;
        Ok(written)
    }

    pub fn index(&self, memo: &str) -> Result<Index, RepoError> {
        let _lifecycle = self.try_lifecycle()?;
        let _operation = self.store.lock_operation()?;
        self.index_unlocked(memo, None)
    }

    pub(crate) fn index_unlocked(
        &self,
        memo: &str,
        previous: Option<&Index>,
    ) -> Result<Index, RepoError> {
        for attempt in 0..7 {
            match indexer::index_once(self, memo, attempt, previous) {
                Err(RepoError::IndexFileChanged) if attempt < 6 => continue,
                result => return result,
            }
        }
        Err(RepoError::RepoFatal)
    }

    pub async fn lock_cloud(&self, cloud: Arc<dyn Cloud>) -> Result<RemoteLockGuard, CloudError> {
        acquire_remote_lock(cloud, self.device.id.clone()).await
    }

    pub fn latest(&self) -> Result<Option<Index>, RepoError> {
        RefStore::new(&self.store).latest()
    }

    pub fn latest_sync(&self) -> Result<Option<Index>, RepoError> {
        RefStore::new(&self.store).latest_sync()
    }

    pub fn list_local_indexes(&self) -> Result<Vec<Index>, RepoError> {
        let _lifecycle = self.try_lifecycle()?;
        let _operation = self.store.lock_operation()?;
        self.store.list_indexes_by_mtime_unlocked()
    }

    pub fn checkout_file(&self, file: &File) -> Result<(), RepoError> {
        let _lifecycle = self.try_lifecycle()?;
        self.checkout_file_unlocked(file)
    }

    pub(crate) fn checkout_file_unlocked(&self, file: &File) -> Result<(), RepoError> {
        let _operation = self.store.lock_operation()?;
        self.checkout_file_with_hooks(
            file,
            || Ok(()),
            |published, mtime| {
                let standard_file = published.try_clone()?.into_std();
                filetime::set_file_handle_times(&standard_file, None, Some(mtime))
            },
        )
    }

    #[cfg(test)]
    fn checkout_file_with_mtime<F>(&self, file: &File, set_mtime: F) -> Result<(), RepoError>
    where
        F: FnOnce(&CapFile, filetime::FileTime) -> std::io::Result<()>,
    {
        let _lifecycle = self.try_lifecycle()?;
        let _operation = self.store.lock_operation()?;
        self.checkout_file_with_hooks(file, || Ok(()), set_mtime)
    }

    fn checkout_file_with_hooks<F, G>(
        &self,
        file: &File,
        after_publish: F,
        set_mtime: G,
    ) -> Result<(), RepoError>
    where
        F: FnOnce() -> std::io::Result<()>,
        G: FnOnce(&CapFile, filetime::FileTime) -> std::io::Result<()>,
    {
        let components = validate_repository_file_path(&file.path)?;
        if file.size < 0 {
            return Err(RepoError::InvalidData("file size must not be negative"));
        }
        let parent = self.open_data_parent(&components, true)?;
        let destination = &components[components.len() - 1];
        validate_replace_destination(&parent, destination)?;
        let staged = create_cap_staged_file(&parent, destination, 0o600)?;
        let mut written = 0_i64;
        for chunk_id in &file.chunks {
            let chunk = self.store.get_chunk_unlocked(chunk_id)?;
            let chunk_size = i64::try_from(chunk.data.len())
                .map_err(|_| RepoError::InvalidData("chunk size exceeds i64"))?;
            written = written
                .checked_add(chunk_size)
                .ok_or(RepoError::InvalidData("checkout size overflow"))?;
            if written > file.size {
                return Err(RepoError::InvalidData(
                    "checkout chunks exceed the declared file size",
                ));
            }
            let mut temp_file = staged.file();
            temp_file.write_all(&chunk.data)?;
        }
        if written != file.size {
            return Err(RepoError::InvalidData(
                "checkout chunks do not match the declared file size",
            ));
        }
        staged.file().sync_all()?;
        let seconds = file.updated.div_euclid(1_000);
        let nanos = file.updated.rem_euclid(1_000) as u32 * 1_000_000;
        let published = staged.publish_replace_retaining_handle()?;
        after_publish()?;
        set_mtime(
            &published,
            filetime::FileTime::from_unix_time(seconds, nanos),
        )?;
        Ok(())
    }

    pub fn checkout_files(&self, files: &[File]) -> Result<(), RepoError> {
        let _lifecycle = self.try_lifecycle()?;
        let _operation = self.store.lock_operation()?;
        for file in files {
            self.checkout_file_with_hooks(
                file,
                || Ok(()),
                |published, mtime| {
                    let standard_file = published.try_clone()?.into_std();
                    filetime::set_file_handle_times(&standard_file, None, Some(mtime))
                },
            )?;
        }
        Ok(())
    }

    pub fn remove_files(&self, files: &[File]) -> Result<(), RepoError> {
        let _lifecycle = self.try_lifecycle()?;
        self.remove_files_unlocked(files)
    }

    pub(crate) fn remove_files_unlocked(&self, files: &[File]) -> Result<(), RepoError> {
        let _operation = self.store.lock_operation()?;
        for file in files {
            let components = validate_repository_file_path(&file.path)?;
            self.remove_file(&components)?;
        }
        Ok(())
    }

    pub fn purge(
        &self,
        retained_index_ids: &[String],
        cancelled: &AtomicBool,
    ) -> Result<PurgeStat, RepoError> {
        let _lifecycle = self.try_lifecycle()?;
        let _operation = self.store.lock_operation()?;
        purge_store_with_cancel_check(&self.store, retained_index_ids, || {
            cancelled.load(Ordering::Relaxed)
        })
    }

    #[cfg(test)]
    pub(crate) fn purge_with_before_delete_hook<F>(
        &self,
        retained_index_ids: &[String],
        cancelled: &AtomicBool,
        before_delete: F,
    ) -> Result<PurgeStat, RepoError>
    where
        F: FnMut() -> Result<(), RepoError>,
    {
        let _lifecycle = self.try_lifecycle()?;
        let _operation = self.store.lock_operation()?;
        crate::purge::purge_store_with_cancel_check_and_hook(
            &self.store,
            retained_index_ids,
            || cancelled.load(Ordering::Relaxed),
            before_delete,
        )
    }

    fn open_data_parent(
        &self,
        components: &[std::ffi::OsString],
        create: bool,
    ) -> Result<Dir, RepoError> {
        let mut directory = self.data_dir.try_clone()?;
        for component in &components[..components.len() - 1] {
            directory = open_child_directory(&directory, component, create)?;
        }
        Ok(directory)
    }

    pub(crate) async fn acquire_lifecycle(&self) -> crate::lifecycle::LifecyclePermits {
        crate::lifecycle::LifecycleGate::acquire_pair(self.store.repo_gate(), &self.data_gate).await
    }

    pub(crate) fn try_lifecycle(&self) -> Result<crate::lifecycle::LifecyclePermits, RepoError> {
        crate::lifecycle::LifecycleGate::try_acquire_pair(self.store.repo_gate(), &self.data_gate)
    }

    fn remove_file(&self, components: &[std::ffi::OsString]) -> Result<(), RepoError> {
        let parent = match self.open_data_parent(components, false) {
            Ok(parent) => parent,
            Err(error) if is_not_found(&error) => return Ok(()),
            Err(error) => return Err(error),
        };
        let name = &components[components.len() - 1];
        let metadata = match parent.symlink_metadata(name) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        if !metadata.file_type().is_file() || cap_metadata_is_reparse(&metadata) {
            return Err(RepoError::UnsafePath);
        }
        parent.remove_file(name)?;
        Ok(())
    }
}

pub(crate) struct RepoStagedDownload {
    staged: CapStagedFile,
    _temp_guard: tokio::sync::OwnedMutexGuard<()>,
}

impl RepoStagedDownload {
    pub(crate) fn writer(&self) -> Result<tokio::fs::File, RepoError> {
        Ok(tokio::fs::File::from_std(
            self.staged.file().try_clone()?.into_std(),
        ))
    }

    pub(crate) fn file(&self) -> &CapFile {
        self.staged.file()
    }

    pub(crate) fn reader(&self) -> Result<CapFile, RepoError> {
        let mut reader = self.staged.file().try_clone()?;
        reader.seek(SeekFrom::Start(0))?;
        Ok(reader)
    }
}

fn cleanup_abandoned_downloads(temp_dir: &Dir) -> Result<(), RepoError> {
    for entry in temp_dir.entries()? {
        let entry = entry?;
        let name = entry.file_name();
        if !is_owned_stage_name(&name) {
            continue;
        }
        let metadata = temp_dir.symlink_metadata(&name)?;
        if metadata.file_type().is_file()
            || metadata.file_type().is_symlink()
            || cap_metadata_is_reparse(&metadata)
        {
            temp_dir.remove_file(&name)?;
        }
    }
    Ok(())
}

fn validate_repository_file_path(path: &str) -> Result<Vec<std::ffi::OsString>, RepoError> {
    if path == "/" || !path.starts_with('/') || path.contains('\\') {
        return Err(RepoError::UnsafePath);
    }
    let mut components = Vec::new();
    for component in path[1..].split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return Err(RepoError::UnsafePath);
        }
        components.push(std::ffi::OsString::from(component));
    }
    if components.is_empty() {
        Err(RepoError::UnsafePath)
    } else {
        Ok(components)
    }
}

fn validate_replace_destination(parent: &Dir, name: &std::ffi::OsStr) -> Result<(), RepoError> {
    match parent.symlink_metadata(name) {
        Ok(metadata) if metadata.file_type().is_file() && !cap_metadata_is_reparse(&metadata) => {
            Ok(())
        }
        Ok(_) => Err(RepoError::UnsafePath),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn is_not_found(error: &RepoError) -> bool {
    matches!(error, RepoError::Io(error) if error.kind() == std::io::ErrorKind::NotFound)
}

fn normalize_root(path: &Path) -> Result<PathBuf, RepoError> {
    if path.as_os_str().is_empty() {
        return Err(RepoError::UnsafePath);
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(RepoError::UnsafePath);
                }
            }
        }
    }
    if !normalized.is_absolute() {
        return Err(RepoError::UnsafePath);
    }
    validate_windows_directory_components_before_canonicalize(&normalized)?;

    match std::fs::symlink_metadata(&normalized) {
        Ok(metadata) if unsafe_link_metadata(&metadata) => {
            return Err(RepoError::UnsafePath);
        }
        Ok(_) => {}
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) => {}
        Err(error) => return Err(RepoError::Io(error)),
    }

    let mut existing = normalized.clone();
    let mut missing = Vec::new();
    let canonical_existing = loop {
        match std::fs::symlink_metadata(&existing) {
            Ok(metadata) => {
                if !metadata.is_dir() {
                    return Err(RepoError::UnsafePath);
                }
                break std::fs::canonicalize(&existing)?;
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                let component = existing
                    .file_name()
                    .ok_or(RepoError::UnsafePath)?
                    .to_os_string();
                missing.push(component);
                if !existing.pop() {
                    return Err(RepoError::UnsafePath);
                }
            }
            Err(error) => return Err(RepoError::Io(error)),
        }
    };
    let mut canonical = canonical_existing;
    for component in missing.into_iter().rev() {
        canonical.push(component);
    }
    Ok(canonical)
}

fn normalize_protected_path(path: &str) -> Result<String, RepoError> {
    if path == "/" || !path.starts_with('/') || path.contains('\\') {
        return Err(RepoError::UnsafePath);
    }
    let mut normalized = String::new();
    for component in path[1..].split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return Err(RepoError::UnsafePath);
        }
        normalized.push('/');
        normalized.push_str(component);
    }
    Ok(normalized)
}

pub(crate) fn validate_root_has_no_symlinks(path: &Path) -> Result<(), RepoError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if unsafe_link_metadata(&metadata) => Err(RepoError::UnsafePath),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(RepoError::Io(error)),
    }
}

fn unsafe_link_metadata(metadata: &std::fs::Metadata) -> bool {
    std_metadata_is_reparse(metadata)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use filetime::FileTime;
    use tempfile::TempDir;
    use tokio::io::AsyncWriteExt;

    use crate::{Chunk, File, Index, RepoError, RepositoryRuntimeState};

    use super::{Device, Repo, RepoOptions, RepoPaths};

    fn repo_fixture_with_runtime() -> (TempDir, RepositoryRuntimeState, Repo) {
        let temp = TempDir::new().unwrap();
        let runtime = RepositoryRuntimeState::default();
        let paths = RepoPaths {
            data: temp.path().join("data"),
            repo: temp.path().join("repo"),
            history: temp.path().join("history"),
            temp: temp.path().join("temp"),
        };
        fs::create_dir_all(&paths.data).unwrap();
        let repo = Repo::open_with_runtime(
            paths,
            Device {
                id: "device".to_owned(),
                name: "QingYu".to_owned(),
                os: "test".to_owned(),
            },
            [3; 32],
            RepoOptions::default(),
            &runtime,
        )
        .unwrap();
        (temp, runtime, repo)
    }

    fn repo_fixture() -> (TempDir, Repo) {
        let (temp, _runtime, repo) = repo_fixture_with_runtime();
        (temp, repo)
    }

    #[tokio::test]
    async fn dropping_a_staged_download_removes_its_partial_task_owned_file() {
        let (temp, repo) = repo_fixture();
        let staged = repo.create_staged_download().unwrap();
        let mut writer = staged.writer().unwrap();
        writer.write_all(b"partial").await.unwrap();
        writer.flush().await.unwrap();
        drop(writer);
        assert_eq!(fs::read_dir(temp.path().join("temp")).unwrap().count(), 1);

        drop(staged);

        assert_eq!(fs::read_dir(temp.path().join("temp")).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn opening_another_repo_does_not_clean_an_active_download_stage() {
        let (temp, runtime, first) = repo_fixture_with_runtime();
        let staged = first.create_staged_download().unwrap();
        let mut writer = staged.writer().unwrap();
        writer.write_all(b"active").await.unwrap();
        writer.flush().await.unwrap();
        drop(writer);
        let second_data = temp.path().join("data-second");
        fs::create_dir_all(&second_data).unwrap();

        let second = Repo::open_with_runtime(
            RepoPaths {
                data: second_data,
                repo: temp.path().join("repo-second"),
                history: temp.path().join("history-second"),
                temp: temp.path().join("temp"),
            },
            Device {
                id: "second".to_owned(),
                name: "QingYu".to_owned(),
                os: "test".to_owned(),
            },
            [3; 32],
            RepoOptions::default(),
            &runtime,
        )
        .unwrap();

        assert_eq!(fs::read_dir(temp.path().join("temp")).unwrap().count(), 1);
        drop(second);
        drop(staged);
        assert_eq!(fs::read_dir(temp.path().join("temp")).unwrap().count(), 0);
    }

    #[test]
    fn opening_a_repo_cleans_only_abandoned_stages_inside_its_own_temp_root() {
        let root = TempDir::new().unwrap();
        let own_temp = root.path().join("own-temp");
        let other_temp = root.path().join("other-temp");
        fs::create_dir_all(&own_temp).unwrap();
        fs::create_dir_all(&other_temp).unwrap();
        let owned = format!("stage-{}.tmp", "0".repeat(40));
        fs::write(own_temp.join(&owned), b"owned").unwrap();
        let retained_files = [
            "stage-abandoned.tmp".to_owned(),
            format!("stage-{}.tmp", "0".repeat(39)),
            format!("stage-{}.tmp", "0".repeat(41)),
            format!("stage-{}.tmp", "A".repeat(40)),
            format!("stage-{}g.tmp", "0".repeat(39)),
            "user.tmp".to_owned(),
        ];
        for name in &retained_files {
            fs::write(own_temp.join(name), name.as_bytes()).unwrap();
        }
        let matching_directory = format!("stage-{}.tmp", "1".repeat(40));
        fs::create_dir(own_temp.join(&matching_directory)).unwrap();
        fs::write(other_temp.join(&owned), b"other").unwrap();
        let data = root.path().join("data");
        fs::create_dir_all(&data).unwrap();

        let repo = Repo::open(
            RepoPaths {
                data,
                repo: root.path().join("repo"),
                history: root.path().join("history"),
                temp: own_temp.clone(),
            },
            Device {
                id: "device".to_owned(),
                name: "QingYu".to_owned(),
                os: "test".to_owned(),
            },
            [3; 32],
            RepoOptions::default(),
        )
        .unwrap();

        assert!(!own_temp.join(&owned).exists());
        for name in retained_files {
            assert_eq!(fs::read(own_temp.join(&name)).unwrap(), name.as_bytes());
        }
        assert!(own_temp.join(matching_directory).is_dir());
        assert_eq!(fs::read(other_temp.join(owned)).unwrap(), b"other");
        drop(repo);
    }

    #[tokio::test]
    async fn lifecycle_gate_follows_data_directory_identity_across_rename_and_reopen() {
        let (temp, runtime, first) = repo_fixture_with_runtime();
        fs::write(temp.path().join("data/document.md"), b"document").unwrap();
        let moved_data = temp.path().join("data-moved");
        fs::rename(temp.path().join("data"), &moved_data).unwrap();
        let second = Repo::open_with_runtime(
            RepoPaths {
                data: moved_data,
                repo: temp.path().join("repo-second"),
                history: temp.path().join("history-second"),
                temp: temp.path().join("temp-second"),
            },
            Device {
                id: "second".to_owned(),
                name: "QingYu".to_owned(),
                os: "test".to_owned(),
            },
            [3; 32],
            RepoOptions::default(),
            &runtime,
        )
        .unwrap();
        let held = first.acquire_lifecycle().await;

        assert!(matches!(
            second.index("busy"),
            Err(RepoError::RepositoryBusy)
        ));
        drop(held);
        assert!(second.index("available").is_ok());
    }

    #[tokio::test]
    async fn repository_runtime_state_scopes_repo_lifecycle_gates() {
        let temp = TempDir::new().unwrap();
        let paths = RepoPaths {
            data: temp.path().join("data"),
            repo: temp.path().join("repo"),
            history: temp.path().join("history"),
            temp: temp.path().join("temp"),
        };
        fs::create_dir_all(&paths.data).unwrap();
        fs::write(paths.data.join("document.md"), b"document").unwrap();
        let shared_runtime = RepositoryRuntimeState::default();
        let isolated_runtime = RepositoryRuntimeState::default();
        let device = Device {
            id: "device".to_owned(),
            name: "QingYu".to_owned(),
            os: "test".to_owned(),
        };
        let first = Repo::open_with_runtime(
            paths.clone(),
            device.clone(),
            [3; 32],
            RepoOptions::default(),
            &shared_runtime,
        )
        .unwrap();
        let shared = Repo::open_with_runtime(
            paths.clone(),
            device.clone(),
            [3; 32],
            RepoOptions::default(),
            &shared_runtime,
        )
        .unwrap();
        let isolated = Repo::open_with_runtime(
            paths,
            device,
            [3; 32],
            RepoOptions::default(),
            &isolated_runtime,
        )
        .unwrap();
        let held = first.acquire_lifecycle().await;

        assert!(matches!(
            shared.index("shared runtime"),
            Err(RepoError::RepositoryBusy)
        ));
        assert!(isolated.index("isolated runtime").is_ok());

        drop(held);
        assert!(shared.index("shared runtime released").is_ok());
    }

    fn put_chunk(repo: &Repo, data: &[u8]) -> String {
        let id = crate::sha1_hex(data);
        repo.store
            .put_chunk(&Chunk {
                id: id.clone(),
                data: data.to_vec(),
            })
            .unwrap();
        id
    }

    fn empty_index(repo: &Repo, id: &str, created: i64) -> Index {
        let mut index = Index {
            id: id.to_owned(),
            memo: String::new(),
            created,
            files: Vec::new(),
            count: 0,
            size: 0,
            system_id: String::new(),
            system_name: String::new(),
            system_os: String::new(),
            check_index_id: String::new(),
            aes_key_verify_val: String::new(),
        };
        index.init_aes_key_verify_val(&repo.key).unwrap();
        index
    }

    #[test]
    fn local_indexes_are_decoded_in_filesystem_mtime_order_not_created_order() {
        let (_temp, repo) = repo_fixture();
        let older_created = empty_index(&repo, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", 1_000);
        let newer_created = empty_index(&repo, "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", 2_000);
        repo.store.put_index(&older_created).unwrap();
        repo.store.put_index(&newer_created).unwrap();
        filetime::set_file_mtime(
            repo.store.index_path(&older_created.id).unwrap(),
            FileTime::from_unix_time(2_000, 0),
        )
        .unwrap();
        filetime::set_file_mtime(
            repo.store.index_path(&newer_created.id).unwrap(),
            FileTime::from_unix_time(1_000, 0),
        )
        .unwrap();

        let indexes = repo.list_local_indexes().unwrap();

        assert_eq!(indexes, vec![older_created, newer_created]);
    }

    #[test]
    fn local_index_listing_reports_a_corrupt_addressed_index() {
        let (temp, repo) = repo_fixture();
        let corrupt_id = "cccccccccccccccccccccccccccccccccccccccc";
        let indexes = temp.path().join("repo/indexes");
        fs::create_dir_all(&indexes).unwrap();
        fs::write(indexes.join(corrupt_id), b"corrupt addressed index").unwrap();

        assert!(repo.list_local_indexes().is_err());
    }

    #[test]
    fn local_index_listing_rejects_a_non_regular_addressed_entry() {
        let (temp, repo) = repo_fixture();
        let directory_id = "dddddddddddddddddddddddddddddddddddddddd";
        fs::create_dir_all(temp.path().join("repo/indexes").join(directory_id)).unwrap();

        assert!(matches!(
            repo.list_local_indexes(),
            Err(RepoError::UnsafePath)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn local_index_listing_does_not_follow_an_addressed_symlink() {
        use std::os::unix::fs::symlink;

        let (temp, repo) = repo_fixture();
        let symlink_id = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
        let target = temp.path().join("outside-index");
        fs::write(&target, b"outside").unwrap();
        let indexes = temp.path().join("repo/indexes");
        fs::create_dir_all(&indexes).unwrap();
        symlink(&target, indexes.join(symlink_id)).unwrap();

        assert!(matches!(
            repo.list_local_indexes(),
            Err(RepoError::UnsafePath)
        ));
        assert_eq!(fs::read(target).unwrap(), b"outside");
    }

    #[test]
    fn checkout_file_streams_chunks_in_order_and_restores_mtime() {
        let (temp, repo) = repo_fixture();
        let first = put_chunk(&repo, b"hello ");
        let second = put_chunk(&repo, b"world");
        let file = File {
            id: "3333333333333333333333333333333333333333".to_owned(),
            path: "/notes/document.md".to_owned(),
            size: 11,
            updated: 1_700_000_000_123,
            chunks: vec![first, second],
        };

        repo.checkout_file(&file).unwrap();

        let target = temp.path().join("data/notes/document.md");
        assert_eq!(fs::read(&target).unwrap(), b"hello world");
        let updated = FileTime::from_last_modification_time(&fs::metadata(target).unwrap());
        assert_eq!(updated.unix_seconds(), 1_700_000_000);
        assert_eq!(updated.nanoseconds(), 123_000_000);
    }

    #[test]
    fn interrupted_checkout_preserves_the_existing_target_and_removes_temp() {
        let (temp, repo) = repo_fixture();
        let first = put_chunk(&repo, b"partial");
        let missing = "ffffffffffffffffffffffffffffffffffffffff";
        let target = temp.path().join("data/notes/document.md");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, b"existing version").unwrap();
        let file = File {
            id: "3333333333333333333333333333333333333333".to_owned(),
            path: "/notes/document.md".to_owned(),
            size: 14,
            updated: 1_700_000_000_123,
            chunks: vec![first, missing.to_owned()],
        };

        assert!(matches!(
            repo.checkout_file(&file),
            Err(RepoError::NotFound(id)) if id == missing
        ));
        assert_eq!(fs::read(&target).unwrap(), b"existing version");
        let names = fs::read_dir(target.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(names, [target.file_name().unwrap()]);
    }

    #[test]
    fn checkout_size_mismatch_preserves_the_existing_target() {
        let (temp, repo) = repo_fixture();
        let chunk_id = put_chunk(&repo, b"new bytes");
        let target = temp.path().join("data/document.md");
        fs::write(&target, b"existing version").unwrap();
        let file = File {
            id: "3333333333333333333333333333333333333333".to_owned(),
            path: "/document.md".to_owned(),
            size: 99,
            updated: 1_700_000_000_123,
            chunks: vec![chunk_id],
        };

        assert!(matches!(
            repo.checkout_file(&file),
            Err(RepoError::InvalidData(_))
        ));
        assert_eq!(fs::read(target).unwrap(), b"existing version");
    }

    #[test]
    fn checkout_publishes_before_restoring_mtime() {
        let (temp, repo) = repo_fixture();
        let chunk_id = put_chunk(&repo, b"published bytes");
        let target = temp.path().join("data/document.md");
        fs::write(&target, b"existing version").unwrap();
        let file = File {
            id: "3333333333333333333333333333333333333333".to_owned(),
            path: "/document.md".to_owned(),
            size: 15,
            updated: 1_700_000_000_123,
            chunks: vec![chunk_id],
        };

        let result = repo.checkout_file_with_mtime(&file, |_file, _mtime| {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "injected mtime failure",
            ))
        });

        assert!(matches!(
            result,
            Err(RepoError::Io(error)) if error.kind() == std::io::ErrorKind::PermissionDenied
        ));
        assert_eq!(fs::read(target).unwrap(), b"published bytes");
    }

    #[cfg(unix)]
    #[test]
    fn checkout_restores_mtime_on_the_published_inode_not_a_same_name_replacement() {
        let (temp, repo) = repo_fixture();
        let chunk_id = put_chunk(&repo, b"published bytes");
        let target = temp.path().join("data/document.md");
        fs::write(&target, b"existing version").unwrap();
        let published_link = temp.path().join("data/published-inode.md");
        let replacement = temp.path().join("data/replacement.md");
        let replacement_time = FileTime::from_unix_time(1_600_000_000, 0);
        let file = File {
            id: "3333333333333333333333333333333333333333".to_owned(),
            path: "/document.md".to_owned(),
            size: 15,
            updated: 1_700_000_000_123,
            chunks: vec![chunk_id],
        };

        repo.checkout_file_with_hooks(
            &file,
            || {
                fs::hard_link(&target, &published_link)?;
                fs::write(&replacement, b"concurrent replacement")?;
                filetime::set_file_mtime(&replacement, replacement_time)?;
                fs::rename(&replacement, &target)
            },
            |published, mtime| {
                let standard_file = published.try_clone()?.into_std();
                filetime::set_file_handle_times(&standard_file, None, Some(mtime))
            },
        )
        .unwrap();

        assert_eq!(fs::read(&target).unwrap(), b"concurrent replacement");
        assert_eq!(fs::read(&published_link).unwrap(), b"published bytes");
        let target_mtime = FileTime::from_last_modification_time(&fs::metadata(&target).unwrap());
        assert_eq!(target_mtime.unix_seconds(), replacement_time.unix_seconds());
        let published_mtime =
            FileTime::from_last_modification_time(&fs::metadata(&published_link).unwrap());
        assert_eq!(published_mtime.unix_seconds(), 1_700_000_000);
        assert_eq!(published_mtime.nanoseconds(), 123_000_000);
    }

    #[test]
    fn remove_files_deletes_only_regular_relative_files_without_pruning_parents() {
        let (temp, repo) = repo_fixture();
        let target = temp.path().join("data/notes/nested/document.md");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, b"delete me").unwrap();
        let file = File::new("/notes/nested/document.md", 9, 1_700_000_000_123);

        repo.remove_files(&[file]).unwrap();

        assert!(!target.exists());
        assert!(temp.path().join("data/notes/nested").is_dir());
        assert!(temp.path().join("data/notes").is_dir());
        assert!(temp.path().join("data").is_dir());
    }

    #[test]
    fn remove_files_rejects_a_directory_target() {
        let (temp, repo) = repo_fixture();
        fs::create_dir_all(temp.path().join("data/notes")).unwrap();
        let file = File::new("/notes", 0, 1_700_000_000_123);

        assert!(matches!(
            repo.remove_files(&[file]),
            Err(RepoError::UnsafePath)
        ));
        assert!(temp.path().join("data/notes").is_dir());
    }

    #[test]
    fn checkout_files_keeps_earlier_success_when_a_later_path_is_invalid() {
        let (temp, repo) = repo_fixture();
        let chunk_id = put_chunk(&repo, b"first");
        let first = File {
            id: "2222222222222222222222222222222222222222".to_owned(),
            path: "/first.md".to_owned(),
            size: 5,
            updated: 1_700_000_000_123,
            chunks: vec![chunk_id],
        };
        let later_invalid = File::new("relative.md", 0, 1_700_000_000_123);

        assert!(matches!(
            repo.checkout_files(&[first, later_invalid]),
            Err(RepoError::UnsafePath)
        ));
        assert_eq!(
            fs::read(temp.path().join("data/first.md")).unwrap(),
            b"first"
        );
    }

    #[test]
    fn remove_files_keeps_earlier_success_when_a_later_path_is_invalid() {
        let (temp, repo) = repo_fixture();
        let first_target = temp.path().join("data/first.md");
        fs::write(&first_target, b"first").unwrap();
        let first = File::new("/first.md", 5, 1_700_000_000_123);
        let later_invalid = File::new("relative.md", 0, 1_700_000_000_123);

        assert!(matches!(
            repo.remove_files(&[first, later_invalid]),
            Err(RepoError::UnsafePath)
        ));

        assert!(!first_target.exists());
    }

    #[cfg(unix)]
    #[test]
    fn checkout_and_remove_reject_symlink_ancestor_escapes() {
        use std::os::unix::fs::symlink;

        let (temp, repo) = repo_fixture();
        let outside = temp.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("document.md"), b"outside").unwrap();
        symlink(&outside, temp.path().join("data/notes")).unwrap();
        let file = File::new("/notes/document.md", 7, 1_700_000_000_123);

        assert!(matches!(
            repo.checkout_file(&file),
            Err(RepoError::UnsafePath)
        ));
        assert!(matches!(
            repo.remove_files(&[file]),
            Err(RepoError::UnsafePath)
        ));
        assert_eq!(fs::read(outside.join("document.md")).unwrap(), b"outside");
    }
}
