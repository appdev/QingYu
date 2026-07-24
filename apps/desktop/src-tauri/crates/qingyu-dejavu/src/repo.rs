use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, File as CapFile, OpenOptions};
use ignore::gitignore::{Gitignore, GitignoreBuilder};

use crate::atomic_write::create_cap_staged_file;
use crate::indexer::{self, IndexHook, NoopIndexHook};
use crate::path_security::{
    cap_metadata_is_reparse, std_metadata_is_reparse,
    validate_windows_directory_components_before_canonicalize,
};
use crate::purge::{purge_store_with_cancel_check, PurgeStat};
use crate::store::{open_absolute_dir_nofollow, open_child_directory};
use crate::{File, Index, RepoError, Store};

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
    pub(crate) device: Device,
    pub(crate) key: [u8; 32],
    pub(crate) protected_include_paths: Vec<String>,
    pub(crate) ignore_matcher: Gitignore,
    pub(crate) store: Store,
    pub(crate) index_hook: Arc<dyn IndexHook>,
}

impl Repo {
    pub fn open(
        paths: RepoPaths,
        device: Device,
        key: [u8; 32],
        options: RepoOptions,
    ) -> Result<Self, RepoError> {
        Self::open_inner(paths, device, key, options, Arc::new(NoopIndexHook))
    }

    #[cfg(test)]
    pub(crate) fn open_with_hook(
        paths: RepoPaths,
        device: Device,
        key: [u8; 32],
        options: RepoOptions,
        index_hook: Arc<dyn IndexHook>,
    ) -> Result<Self, RepoError> {
        Self::open_inner(paths, device, key, options, index_hook)
    }

    fn open_inner(
        paths: RepoPaths,
        device: Device,
        key: [u8; 32],
        options: RepoOptions,
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
        let store = Store::new(&paths.repo, key)?;

        Ok(Self {
            data_dir,
            device,
            key,
            protected_include_paths,
            ignore_matcher,
            store,
            index_hook,
        })
    }

    pub fn index(&self, memo: &str) -> Result<Index, RepoError> {
        for attempt in 0..7 {
            match indexer::index_once(self, memo, attempt) {
                Err(RepoError::IndexFileChanged) if attempt < 6 => continue,
                result => return result,
            }
        }
        Err(RepoError::RepoFatal)
    }

    pub fn checkout_file(&self, file: &File) -> Result<(), RepoError> {
        self.checkout_file_with_mtime(file, |published, mtime| {
            let standard_file = published.try_clone()?.into_std();
            filetime::set_file_handle_times(&standard_file, None, Some(mtime))
        })
    }

    fn checkout_file_with_mtime<F>(&self, file: &File, set_mtime: F) -> Result<(), RepoError>
    where
        F: FnOnce(&CapFile, filetime::FileTime) -> std::io::Result<()>,
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
            let chunk = self.store.get_chunk(chunk_id)?;
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
        staged.publish_replace()?;

        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        let published = parent
            .open_with(destination, &options)
            .map_err(|error| map_data_nofollow_error(&parent, destination, error))?;
        let metadata = published.metadata()?;
        if !metadata.file_type().is_file() || cap_metadata_is_reparse(&metadata) {
            return Err(RepoError::UnsafePath);
        }
        set_mtime(
            &published,
            filetime::FileTime::from_unix_time(seconds, nanos),
        )?;
        Ok(())
    }

    pub fn checkout_files(&self, files: &[File]) -> Result<(), RepoError> {
        for file in files {
            self.checkout_file(file)?;
        }
        Ok(())
    }

    pub fn remove_files(&self, files: &[File]) -> Result<(), RepoError> {
        let paths = files
            .iter()
            .map(|file| validate_repository_file_path(&file.path))
            .collect::<Result<Vec<_>, _>>()?;
        for components in paths {
            self.remove_file(&components)?;
        }
        Ok(())
    }

    pub fn purge(
        &self,
        retained_index_ids: &[String],
        cancelled: &AtomicBool,
    ) -> Result<PurgeStat, RepoError> {
        purge_store_with_cancel_check(&self.store, retained_index_ids, || {
            cancelled.load(Ordering::Relaxed)
        })
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
        self.prune_empty_parents(&components[..components.len() - 1])
    }

    fn prune_empty_parents(&self, directories: &[std::ffi::OsString]) -> Result<(), RepoError> {
        'depths: for depth in (1..=directories.len()).rev() {
            let mut parent = self.data_dir.try_clone()?;
            for component in &directories[..depth - 1] {
                parent = match open_child_directory(&parent, component, false) {
                    Ok(child) => child,
                    Err(error) if is_not_found(&error) => continue 'depths,
                    Err(error) => return Err(error),
                };
            }
            let name = &directories[depth - 1];
            let metadata = match parent.symlink_metadata(name) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            if !metadata.file_type().is_dir() || cap_metadata_is_reparse(&metadata) {
                return Err(RepoError::UnsafePath);
            }
            match parent.remove_dir(name) {
                Ok(()) => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::DirectoryNotEmpty | std::io::ErrorKind::NotFound
                    ) =>
                {
                    break
                }
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }
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

fn map_data_nofollow_error(
    parent: &Dir,
    name: &std::ffi::OsStr,
    error: std::io::Error,
) -> RepoError {
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

    use crate::{Chunk, File, RepoError};

    use super::{Device, Repo, RepoOptions, RepoPaths};

    fn repo_fixture() -> (TempDir, Repo) {
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
            [3; 32],
            RepoOptions::default(),
        )
        .unwrap();
        (temp, repo)
    }

    #[test]
    fn checkout_file_streams_chunks_in_order_and_restores_mtime() {
        let (temp, repo) = repo_fixture();
        let first = "1111111111111111111111111111111111111111";
        let second = "2222222222222222222222222222222222222222";
        repo.store
            .put_chunk(&Chunk {
                id: first.to_owned(),
                data: b"hello ".to_vec(),
            })
            .unwrap();
        repo.store
            .put_chunk(&Chunk {
                id: second.to_owned(),
                data: b"world".to_vec(),
            })
            .unwrap();
        let file = File {
            id: "3333333333333333333333333333333333333333".to_owned(),
            path: "/notes/document.md".to_owned(),
            size: 11,
            updated: 1_700_000_000_123,
            chunks: vec![first.to_owned(), second.to_owned()],
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
        let first = "1111111111111111111111111111111111111111";
        let missing = "ffffffffffffffffffffffffffffffffffffffff";
        repo.store
            .put_chunk(&Chunk {
                id: first.to_owned(),
                data: b"partial".to_vec(),
            })
            .unwrap();
        let target = temp.path().join("data/notes/document.md");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, b"existing version").unwrap();
        let file = File {
            id: "3333333333333333333333333333333333333333".to_owned(),
            path: "/notes/document.md".to_owned(),
            size: 14,
            updated: 1_700_000_000_123,
            chunks: vec![first.to_owned(), missing.to_owned()],
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
        let chunk_id = "1111111111111111111111111111111111111111";
        repo.store
            .put_chunk(&Chunk {
                id: chunk_id.to_owned(),
                data: b"new bytes".to_vec(),
            })
            .unwrap();
        let target = temp.path().join("data/document.md");
        fs::write(&target, b"existing version").unwrap();
        let file = File {
            id: "3333333333333333333333333333333333333333".to_owned(),
            path: "/document.md".to_owned(),
            size: 99,
            updated: 1_700_000_000_123,
            chunks: vec![chunk_id.to_owned()],
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
        let chunk_id = "1111111111111111111111111111111111111111";
        repo.store
            .put_chunk(&Chunk {
                id: chunk_id.to_owned(),
                data: b"published bytes".to_vec(),
            })
            .unwrap();
        let target = temp.path().join("data/document.md");
        fs::write(&target, b"existing version").unwrap();
        let file = File {
            id: "3333333333333333333333333333333333333333".to_owned(),
            path: "/document.md".to_owned(),
            size: 15,
            updated: 1_700_000_000_123,
            chunks: vec![chunk_id.to_owned()],
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

    #[test]
    fn remove_files_deletes_only_regular_relative_files_and_prunes_empty_parents() {
        let (temp, repo) = repo_fixture();
        let target = temp.path().join("data/notes/nested/document.md");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, b"delete me").unwrap();
        let file = File::new("/notes/nested/document.md", 9, 1_700_000_000_123);

        repo.remove_files(&[file]).unwrap();

        assert!(!target.exists());
        assert!(!temp.path().join("data/notes").exists());
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
    fn pruning_a_missing_ancestor_does_not_remove_a_same_named_root_directory() {
        let (temp, repo) = repo_fixture();
        let same_named_root = temp.path().join("data/nested");
        fs::create_dir_all(&same_named_root).unwrap();

        repo.prune_empty_parents(&["missing".into(), "nested".into()])
            .unwrap();

        assert!(same_named_root.is_dir());
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
