use std::io::Read;
use std::path::{Path, PathBuf};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, Metadata, OpenOptions};

use crate::chunker::StreamingRabinChunker;
use crate::path_security::cap_metadata_is_reparse;
use crate::{random_hash, sha1_hex, Chunk, File, Index, Repo, RepoError};

#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_HIDDEN as WINDOWS_HIDDEN_ATTRIBUTE;
#[cfg(not(windows))]
const WINDOWS_HIDDEN_ATTRIBUTE: u32 = 0x2;

pub(crate) trait IndexHook: Send + Sync {
    fn after_scan(&self, attempt: usize) -> Result<(), RepoError>;
}

pub(crate) struct NoopIndexHook;

impl IndexHook for NoopIndexHook {
    fn after_scan(&self, _attempt: usize) -> Result<(), RepoError> {
        Ok(())
    }
}

struct ScannedFile {
    relative_path: PathBuf,
    repository_path: String,
    size: i64,
    updated: i64,
}

pub(crate) fn index_once(repo: &Repo, memo: &str, attempt: usize) -> Result<Index, RepoError> {
    let mut scanned = Vec::new();
    scan_directory(repo, &repo.data_dir, Path::new(""), false, &mut scanned)?;
    scanned.sort_by(|left, right| left.repository_path.cmp(&right.repository_path));
    if scanned.is_empty() {
        return Err(RepoError::EmptyIndex);
    }

    repo.index_hook.after_scan(attempt)?;

    let mut file_ids = Vec::with_capacity(scanned.len());
    let mut total_size = 0_i64;
    for scanned_file in scanned {
        let file = store_scanned_file(repo, &scanned_file)?;
        total_size = total_size
            .checked_add(file.size)
            .ok_or(RepoError::RepoFatal)?;
        file_ids.push(file.id);
    }

    let created = i64::try_from(time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000)
        .map_err(|_| RepoError::RepoFatal)?;
    let mut index = Index {
        id: random_hash().map_err(|_| RepoError::RandomnessUnavailable)?,
        memo: memo.to_owned(),
        created,
        count: file_ids.len(),
        files: file_ids,
        size: total_size,
        system_id: repo.device.id.clone(),
        system_name: repo.device.name.clone(),
        system_os: repo.device.os.clone(),
        check_index_id: String::new(),
        aes_key_verify_val: String::new(),
    };
    index.init_aes_key_verify_val(&repo.key)?;
    repo.store.put_index_unlocked(&index)?;
    Ok(index)
}

fn scan_directory(
    repo: &Repo,
    directory: &Dir,
    relative_directory: &Path,
    hidden_ancestor: bool,
    scanned: &mut Vec<ScannedFile>,
) -> Result<(), RepoError> {
    let directory_metadata = directory.dir_metadata().map_err(map_scan_io)?;
    if cap_metadata_is_reparse(&directory_metadata) {
        return Err(RepoError::UnsafePath);
    }
    if !directory_metadata.file_type().is_dir() {
        return Err(RepoError::IndexFileChanged);
    }
    let entries = directory.entries().map_err(map_scan_io)?;
    for entry in entries {
        let entry = entry.map_err(map_scan_io)?;
        let name = entry.file_name();
        let name_text = name.to_str().ok_or(RepoError::UnsafePath)?;
        let relative_path = relative_directory.join(&name);
        let repository_path = repository_path(&relative_path)?;
        let metadata = directory.symlink_metadata(&name).map_err(map_scan_io)?;
        if cap_metadata_is_reparse(&metadata) {
            return Err(RepoError::UnsafePath);
        }
        let file_type = metadata.file_type();
        let protected = repo
            .protected_include_paths
            .binary_search(&repository_path)
            .is_ok();
        let protected_descendant = has_protected_descendant(repo, &repository_path);
        let hidden = hidden_ancestor || hidden_entry(name_text, &metadata);

        if file_type.is_symlink() {
            if protected || protected_descendant {
                return Err(RepoError::UnsafePath);
            }
            continue;
        }

        if file_type.is_dir() {
            if protected {
                return Err(RepoError::UnsafePath);
            }
            let user_ignored = repo
                .ignore_matcher
                .matched_path_or_any_parents(&relative_path, true)
                .is_ignore();
            if (hidden || user_ignored) && !protected_descendant {
                continue;
            }
            let child = directory
                .open_dir_nofollow(&name)
                .map_err(|error| map_nofollow_io(directory, &name, error))?;
            validate_data_directory(&child)?;
            scan_directory(repo, &child, &relative_path, hidden, scanned)?;
            continue;
        }

        if !file_type.is_file() {
            if protected {
                return Err(RepoError::UnsafePath);
            }
            continue;
        }

        if !protected {
            if hidden || name_text.ends_with(".tmp") {
                continue;
            }
            if repo
                .ignore_matcher
                .matched_path_or_any_parents(&relative_path, false)
                .is_ignore()
            {
                continue;
            }
        }

        scanned.push(ScannedFile {
            relative_path,
            repository_path,
            size: i64::try_from(metadata.len()).map_err(|_| RepoError::RepoFatal)?,
            updated: metadata_updated(&metadata)?,
        });
    }
    Ok(())
}

fn has_protected_descendant(repo: &Repo, repository_path: &str) -> bool {
    let prefix = format!("{repository_path}/");
    repo.protected_include_paths
        .iter()
        .any(|protected| protected.starts_with(&prefix))
}

fn hidden_entry(name: &str, metadata: &Metadata) -> bool {
    #[cfg(windows)]
    let attributes = {
        use cap_std::fs::MetadataExt;
        metadata.file_attributes()
    };
    #[cfg(not(windows))]
    let attributes = {
        let _ = metadata;
        0
    };
    hidden_name_or_windows_attributes(name, attributes)
}

fn hidden_name_or_windows_attributes(name: &str, windows_attributes: u32) -> bool {
    name.starts_with('.') || windows_attributes & WINDOWS_HIDDEN_ATTRIBUTE != 0
}

fn repository_path(relative_path: &Path) -> Result<String, RepoError> {
    let mut result = String::new();
    for component in relative_path.components() {
        let std::path::Component::Normal(component) = component else {
            return Err(RepoError::UnsafePath);
        };
        let component = component.to_str().ok_or(RepoError::UnsafePath)?;
        if component.is_empty() || component == "." || component == ".." || component.contains('\\')
        {
            return Err(RepoError::UnsafePath);
        }
        result.push('/');
        result.push_str(component);
    }
    if result.is_empty() {
        Err(RepoError::UnsafePath)
    } else {
        Ok(result)
    }
}

fn store_scanned_file(repo: &Repo, scanned: &ScannedFile) -> Result<File, RepoError> {
    let mut source = open_file_nofollow(&repo.data_dir, &scanned.relative_path)?;
    let before_read = source.metadata().map_err(map_changed_io)?;
    if cap_metadata_is_reparse(&before_read) {
        return Err(RepoError::UnsafePath);
    }
    if !before_read.file_type().is_file() {
        return Err(RepoError::IndexFileChanged);
    }
    if i64::try_from(before_read.len()).map_err(|_| RepoError::RepoFatal)? != scanned.size
        || metadata_updated(&before_read)? / 1_000 != scanned.updated / 1_000
    {
        return Err(RepoError::IndexFileChanged);
    }

    let mut file = File::new(
        scanned.repository_path.clone(),
        scanned.size,
        scanned.updated,
    );
    if scanned.size == 0 {
        let mut probe = [0_u8; 1];
        if source.read(&mut probe).map_err(map_changed_io)? != 0 {
            return Err(RepoError::IndexFileChanged);
        }
        let id = sha1_hex(&[]);
        repo.store.put_chunk_unlocked(&Chunk {
            id: id.clone(),
            data: Vec::new(),
        })?;
        file.chunks.push(id);
    } else {
        let mut chunker = StreamingRabinChunker::new(&mut source);
        while let Some(data) = chunker.next_chunk().map_err(map_changed_io)? {
            let id = sha1_hex(&data);
            repo.store.put_chunk_unlocked(&Chunk {
                id: id.clone(),
                data,
            })?;
            file.chunks.push(id);
        }
    }

    let after_read = source.metadata().map_err(map_changed_io)?;
    if cap_metadata_is_reparse(&after_read) {
        return Err(RepoError::UnsafePath);
    }
    if !after_read.file_type().is_file()
        || i64::try_from(after_read.len()).map_err(|_| RepoError::RepoFatal)? != scanned.size
        || metadata_updated(&after_read)? / 1_000 != scanned.updated / 1_000
    {
        return Err(RepoError::IndexFileChanged);
    }

    repo.store.put_file_unlocked(&file)?;
    Ok(file)
}

fn open_file_nofollow(root: &Dir, path: &Path) -> Result<cap_std::fs::File, RepoError> {
    let parent = path.parent().ok_or(RepoError::UnsafePath)?;
    let name = path.file_name().ok_or(RepoError::UnsafePath)?;
    let mut directory = root.try_clone()?;
    for component in parent.components() {
        let std::path::Component::Normal(component) = component else {
            return Err(RepoError::UnsafePath);
        };
        directory = directory
            .open_dir_nofollow(component)
            .map_err(|error| map_nofollow_io(&directory, component, error))?;
        validate_data_directory(&directory)?;
    }
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let file = directory
        .open_with(name, &options)
        .map_err(|error| map_nofollow_io(&directory, name, error))?;
    if cap_metadata_is_reparse(&file.metadata()?) {
        return Err(RepoError::UnsafePath);
    }
    Ok(file)
}

fn validate_data_directory(directory: &Dir) -> Result<(), RepoError> {
    let metadata = directory.dir_metadata().map_err(map_changed_io)?;
    if cap_metadata_is_reparse(&metadata) {
        return Err(RepoError::UnsafePath);
    }
    if metadata.file_type().is_dir() {
        Ok(())
    } else {
        Err(RepoError::IndexFileChanged)
    }
}

fn metadata_updated(metadata: &Metadata) -> Result<i64, RepoError> {
    let modified = filetime::FileTime::from_system_time(metadata.modified()?.into_std());
    modified
        .unix_seconds()
        .checked_mul(1_000)
        .and_then(|millis| millis.checked_add(i64::from(modified.nanoseconds() / 1_000_000)))
        .ok_or(RepoError::RepoFatal)
}

fn map_nofollow_io(parent: &Dir, name: &std::ffi::OsStr, error: std::io::Error) -> RepoError {
    match parent.symlink_metadata(name) {
        Ok(metadata) if metadata.file_type().is_symlink() || cap_metadata_is_reparse(&metadata) => {
            RepoError::UnsafePath
        }
        _ if error.kind() == std::io::ErrorKind::NotFound => RepoError::IndexFileChanged,
        _ => RepoError::Io(error),
    }
}

fn map_scan_io(error: std::io::Error) -> RepoError {
    if error.kind() == std::io::ErrorKind::NotFound {
        RepoError::IndexFileChanged
    } else {
        RepoError::Io(error)
    }
}

fn map_changed_io(error: std::io::Error) -> RepoError {
    if error.kind() == std::io::ErrorKind::NotFound {
        RepoError::IndexFileChanged
    } else {
        RepoError::Io(error)
    }
}

#[cfg(test)]
mod tests {
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use super::{hidden_name_or_windows_attributes, IndexHook};
    use crate::{Device, Repo, RepoError, RepoOptions, RepoPaths};

    fn paths(root: &Path) -> RepoPaths {
        RepoPaths {
            data: root.join("data"),
            repo: root.join("repo"),
            history: root.join("history"),
            temp: root.join("temp"),
        }
    }

    fn device() -> Device {
        Device {
            id: "device-id".to_owned(),
            name: "QingYu Test".to_owned(),
            os: "test-os".to_owned(),
        }
    }

    fn key() -> [u8; 32] {
        [0x42; 32]
    }

    fn open_repo(root: &Path, options: RepoOptions) -> Repo {
        Repo::open(paths(root), device(), key(), options).unwrap()
    }

    fn index_file_count(repo_root: &Path) -> usize {
        fs::read_dir(repo_root.join("indexes"))
            .map(|entries| entries.filter_map(Result::ok).count())
            .unwrap_or(0)
    }

    #[test]
    fn index_persists_sorted_files_chunks_and_complete_snapshot_fields() {
        let temp = tempfile::tempdir().unwrap();
        let repo_paths = paths(temp.path());
        fs::create_dir_all(repo_paths.data.join("nested")).unwrap();
        fs::write(repo_paths.data.join("a.txt"), b"small file").unwrap();
        fs::write(
            repo_paths.data.join("nested/big.bin"),
            vec![0_u8; 2 * 1024 * 1024],
        )
        .unwrap();
        let before = time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000;
        let repo = Repo::open(repo_paths.clone(), device(), key(), RepoOptions::default()).unwrap();

        let index = repo.index("first snapshot").unwrap();

        let after = time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000;
        assert_eq!(index.memo, "first snapshot");
        assert!((before..=after).contains(&i128::from(index.created)));
        assert_eq!(index.count, 2);
        assert_eq!(index.size, 10 + 2 * 1024 * 1024);
        assert_eq!(index.system_id, "device-id");
        assert_eq!(index.system_name, "QingYu Test");
        assert_eq!(index.system_os, "test-os");
        assert!(index.check_index_id.is_empty());
        assert!(index.verify_aes_key(&key()));
        assert!(!index.aes_key_verify_val.is_empty());
        assert_eq!(index.id.len(), 40);

        let stored_index = repo.store.get_index(&index.id).unwrap();
        assert_eq!(stored_index, index);
        let files = index
            .files
            .iter()
            .map(|id| repo.store.get_file(id).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            ["/a.txt", "/nested/big.bin"]
        );
        assert_eq!(files[0].chunks.len(), 1);
        assert!(files[1].chunks.len() > 1);
        let small_chunk = repo.store.get_chunk(&files[0].chunks[0]).unwrap();
        assert_eq!(small_chunk.data, b"small file");
        assert!(!repo_paths.repo.join("refs").exists());
    }

    #[cfg(unix)]
    #[test]
    fn built_in_rules_ignore_hidden_tmp_symlink_non_regular_and_empty_entries() {
        use std::os::unix::fs::symlink;
        use std::os::unix::net::UnixListener;

        let temp = tempfile::tempdir().unwrap();
        let repo_paths = paths(temp.path());
        fs::create_dir_all(repo_paths.data.join(".hidden-dir")).unwrap();
        fs::create_dir_all(repo_paths.data.join("empty")).unwrap();
        fs::write(repo_paths.data.join("visible.md"), b"visible").unwrap();
        fs::write(repo_paths.data.join(".hidden.md"), b"hidden").unwrap();
        fs::write(repo_paths.data.join(".hidden-dir/inside.md"), b"hidden").unwrap();
        fs::write(repo_paths.data.join("scratch.tmp"), b"temporary").unwrap();
        symlink("visible.md", repo_paths.data.join("linked.md")).unwrap();
        let _socket = UnixListener::bind(repo_paths.data.join("sync.sock")).unwrap();
        let repo = open_repo(temp.path(), RepoOptions::default());

        let index = repo.index("ignore built-ins").unwrap();

        assert_eq!(index.count, 1);
        let file = repo.store.get_file(&index.files[0]).unwrap();
        assert_eq!(file.path, "/visible.md");
    }

    #[test]
    fn hidden_detection_covers_dot_names_and_the_windows_hidden_attribute() {
        assert!(hidden_name_or_windows_attributes(".hidden", 0));
        assert!(hidden_name_or_windows_attributes("visible", 0x2));
        assert!(!hidden_name_or_windows_attributes("visible", 0));
    }

    #[test]
    fn protected_include_crosses_hidden_and_user_ignored_ancestors() {
        let temp = tempfile::tempdir().unwrap();
        let repo_paths = paths(temp.path());
        fs::create_dir_all(repo_paths.data.join(".qingyu")).unwrap();
        fs::write(repo_paths.data.join(".qingyu/syncignore"), b"*.cache\n").unwrap();
        fs::write(repo_paths.data.join(".qingyu/other"), b"hidden").unwrap();
        fs::write(repo_paths.data.join("visible.md"), b"ignored by user").unwrap();
        let repo = open_repo(
            temp.path(),
            RepoOptions {
                ignore_lines: vec!["*".to_owned()],
                protected_include_paths: vec!["/.qingyu/syncignore".to_owned()],
            },
        );

        let index = repo.index("protected include").unwrap();

        assert_eq!(index.count, 1);
        let file = repo.store.get_file(&index.files[0]).unwrap();
        assert_eq!(file.path, "/.qingyu/syncignore");
    }

    #[test]
    fn empty_scan_returns_empty_index_without_publishing_an_index() {
        let temp = tempfile::tempdir().unwrap();
        let repo_paths = paths(temp.path());
        fs::create_dir_all(&repo_paths.data).unwrap();
        let repo = open_repo(temp.path(), RepoOptions::default());

        let result = repo.index("empty");

        assert!(matches!(result, Err(RepoError::EmptyIndex)));
        assert_eq!(index_file_count(&repo_paths.repo), 0);
    }

    struct AppendAfterScan {
        path: PathBuf,
        calls: AtomicUsize,
    }

    impl IndexHook for AppendAfterScan {
        fn after_scan(&self, _attempt: usize) -> Result<(), RepoError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let mut file = OpenOptions::new().append(true).open(&self.path)?;
            file.write_all(b"x")?;
            Ok(())
        }
    }

    #[test]
    fn file_changes_are_bounded_to_seven_total_attempts_without_partial_index() {
        let temp = tempfile::tempdir().unwrap();
        let repo_paths = paths(temp.path());
        fs::create_dir_all(&repo_paths.data).unwrap();
        let changed_path = repo_paths.data.join("changing.md");
        fs::write(&changed_path, b"initial").unwrap();
        let hook = Arc::new(AppendAfterScan {
            path: changed_path,
            calls: AtomicUsize::new(0),
        });
        let repo = Repo::open_with_hook(
            repo_paths.clone(),
            device(),
            key(),
            RepoOptions::default(),
            hook.clone(),
        )
        .unwrap();

        let result = repo.index("changing");

        assert!(matches!(result, Err(RepoError::IndexFileChanged)));
        assert_eq!(hook.calls.load(Ordering::SeqCst), 7);
        assert_eq!(index_file_count(&repo_paths.repo), 0);
    }

    struct FatalAfterScan {
        calls: AtomicUsize,
    }

    impl IndexHook for FatalAfterScan {
        fn after_scan(&self, _attempt: usize) -> Result<(), RepoError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err(RepoError::RepoFatal)
        }
    }

    #[test]
    fn non_file_change_errors_are_not_retried() {
        let temp = tempfile::tempdir().unwrap();
        let repo_paths = paths(temp.path());
        fs::create_dir_all(&repo_paths.data).unwrap();
        fs::write(repo_paths.data.join("file.md"), b"content").unwrap();
        let hook = Arc::new(FatalAfterScan {
            calls: AtomicUsize::new(0),
        });
        let repo = Repo::open_with_hook(
            repo_paths.clone(),
            device(),
            key(),
            RepoOptions::default(),
            hook.clone(),
        )
        .unwrap();

        let result = repo.index("fatal");

        assert!(matches!(result, Err(RepoError::RepoFatal)));
        assert_eq!(hook.calls.load(Ordering::SeqCst), 1);
        assert_eq!(index_file_count(&repo_paths.repo), 0);
    }

    #[test]
    fn open_rejects_traversal_backslashes_and_paths_without_a_leading_slash() {
        for protected in ["../escape", "/../../escape", "/folder\\file", "folder/file"] {
            let temp = tempfile::tempdir().unwrap();
            let result = Repo::open(
                paths(temp.path()),
                device(),
                key(),
                RepoOptions {
                    ignore_lines: Vec::new(),
                    protected_include_paths: vec![protected.to_owned()],
                },
            );
            assert!(matches!(result, Err(RepoError::UnsafePath)));
        }
    }

    #[cfg(unix)]
    #[test]
    fn open_rejects_symlink_roots_and_index_rejects_protected_symlinks() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let real_data = temp.path().join("real-data");
        fs::create_dir_all(&real_data).unwrap();
        let linked_data = temp.path().join("linked-data");
        symlink(&real_data, &linked_data).unwrap();
        let mut linked_paths = paths(temp.path());
        linked_paths.data = linked_data;
        assert!(matches!(
            Repo::open(linked_paths, device(), key(), RepoOptions::default()),
            Err(RepoError::UnsafePath)
        ));

        let repo_paths = paths(temp.path());
        fs::create_dir_all(repo_paths.data.join(".qingyu")).unwrap();
        fs::write(repo_paths.data.join("target"), b"target").unwrap();
        symlink("../target", repo_paths.data.join(".qingyu/syncignore")).unwrap();
        let repo = open_repo(
            temp.path(),
            RepoOptions {
                ignore_lines: Vec::new(),
                protected_include_paths: vec!["/.qingyu/syncignore".to_owned()],
            },
        );

        assert!(matches!(repo.index("unsafe"), Err(RepoError::UnsafePath)));
        assert_eq!(index_file_count(&repo_paths.repo), 0);
    }

    #[cfg(unix)]
    struct SwapAncestorForSymlink {
        directory: PathBuf,
        target: PathBuf,
        calls: AtomicUsize,
    }

    #[cfg(unix)]
    impl IndexHook for SwapAncestorForSymlink {
        fn after_scan(&self, _attempt: usize) -> Result<(), RepoError> {
            use std::os::unix::fs::symlink;

            self.calls.fetch_add(1, Ordering::SeqCst);
            fs::remove_dir_all(&self.directory)?;
            symlink(&self.target, &self.directory)?;
            Ok(())
        }
    }

    #[cfg(unix)]
    #[test]
    fn index_rejects_an_ancestor_replaced_by_a_symlink_after_scan() {
        let temp = tempfile::tempdir().unwrap();
        let repo_paths = paths(temp.path());
        let nested = repo_paths.data.join("nested");
        let outside = temp.path().join("outside");
        fs::create_dir_all(&nested).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let scanned_file = nested.join("file.md");
        let outside_file = outside.join("file.md");
        fs::write(&scanned_file, b"same bytes").unwrap();
        fs::write(&outside_file, b"same bytes").unwrap();
        let scanned_mtime =
            filetime::FileTime::from_last_modification_time(&fs::metadata(&scanned_file).unwrap());
        filetime::set_file_mtime(&outside_file, scanned_mtime).unwrap();
        let hook = Arc::new(SwapAncestorForSymlink {
            directory: nested,
            target: outside,
            calls: AtomicUsize::new(0),
        });
        let repo = Repo::open_with_hook(
            repo_paths.clone(),
            device(),
            key(),
            RepoOptions::default(),
            hook.clone(),
        )
        .unwrap();

        let result = repo.index("unsafe ancestor");

        assert!(matches!(result, Err(RepoError::UnsafePath)));
        assert_eq!(hook.calls.load(Ordering::SeqCst), 1);
        assert_eq!(index_file_count(&repo_paths.repo), 0);
    }

    #[test]
    fn open_requires_but_does_not_create_the_data_directory() {
        let temp = tempfile::tempdir().unwrap();
        let repo_paths = paths(temp.path());
        assert!(!repo_paths.data.exists());

        let result = Repo::open(repo_paths.clone(), device(), key(), RepoOptions::default());

        assert!(
            matches!(result, Err(RepoError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound)
        );
        assert!(!repo_paths.data.exists());
        assert!(!repo_paths.repo.exists());
        assert!(!repo_paths.history.exists());
        assert!(!repo_paths.temp.exists());
    }

    #[cfg(unix)]
    #[test]
    fn indexing_stays_on_the_opened_data_root_after_its_ancestor_is_replaced() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let data_parent = temp.path().join("data-parent");
        let held_parent = temp.path().join("data-parent-held");
        let outside_parent = temp.path().join("outside-parent");
        fs::create_dir_all(data_parent.join("data")).unwrap();
        fs::create_dir_all(outside_parent.join("data")).unwrap();
        fs::write(data_parent.join("data/original.md"), b"original bytes").unwrap();
        fs::write(outside_parent.join("data/outside.md"), b"outside bytes").unwrap();
        let repo_paths = RepoPaths {
            data: data_parent.join("data"),
            repo: temp.path().join("repo"),
            history: temp.path().join("history"),
            temp: temp.path().join("temp"),
        };
        let repo = Repo::open(repo_paths, device(), key(), RepoOptions::default()).unwrap();
        fs::rename(&data_parent, &held_parent).unwrap();
        symlink(&outside_parent, &data_parent).unwrap();

        let index = repo.index("held root").unwrap();
        let file = repo.store.get_file(&index.files[0]).unwrap();

        assert_eq!(index.count, 1);
        assert_eq!(file.path, "/original.md");
        assert_eq!(
            repo.store.get_chunk(&file.chunks[0]).unwrap().data,
            b"original bytes"
        );
    }

    #[cfg(unix)]
    #[test]
    fn missing_repository_root_is_retained_after_first_index_publication() {
        let temp = tempfile::tempdir().unwrap();
        let repo_paths = paths(temp.path());
        let held_repository = temp.path().join("repo-held");
        fs::create_dir_all(&repo_paths.data).unwrap();
        fs::write(repo_paths.data.join("file.md"), b"content").unwrap();
        let repo = Repo::open(repo_paths.clone(), device(), key(), RepoOptions::default()).unwrap();
        assert!(!repo_paths.repo.exists());

        let first = repo.index("first publication").unwrap();
        fs::rename(&repo_paths.repo, &held_repository).unwrap();
        fs::create_dir(&repo_paths.repo).unwrap();
        fs::write(repo_paths.data.join("second.md"), b"second content").unwrap();

        let second = repo.index("confined publication").unwrap();

        assert_eq!(repo.store.get_index(&first.id).unwrap(), first);
        assert_eq!(repo.store.get_index(&second.id).unwrap(), second);
        assert_eq!(second.count, 2);
        assert!(held_repository.join("indexes").is_dir());
        assert_eq!(fs::read_dir(&repo_paths.repo).unwrap().count(), 0);
    }

    #[test]
    fn open_rejects_a_root_beneath_a_regular_file() {
        let temp = tempfile::tempdir().unwrap();
        let blocker = temp.path().join("not-a-directory");
        fs::write(&blocker, b"file").unwrap();
        let mut repo_paths = paths(temp.path());
        repo_paths.repo = blocker.join("repo");

        let result = Repo::open(repo_paths, device(), key(), RepoOptions::default());

        match result {
            Err(RepoError::UnsafePath) => {}
            Err(error) => panic!("expected UnsafePath, got {error:?}"),
            Ok(_) => panic!("expected UnsafePath, got success"),
        }
    }
}
