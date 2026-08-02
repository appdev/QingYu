use std::{
    ffi::OsStr,
    io::{self, Write},
    path::{Path, PathBuf},
};

use cap_fs_ext::{FollowSymlinks, MetadataExt, OpenOptionsExt, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};

use super::types::MarkdownFile;

const UPDATE_TEMP_PREFIX: &str = ".qingyu-ui-update-";
const RENAME_TEMP_PREFIX: &str = ".qingyu-ui-rename-";
const CASE_ONLY_RENAME_RECOVERY_ERROR: &str =
    "Document rename could not be completed or rolled back safely";

pub(super) fn trusted_parent(path: &Path) -> Result<(Dir, PathBuf, std::ffi::OsString), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "Document parent is unavailable".to_string())?
        .to_path_buf();
    let name = path
        .file_name()
        .ok_or_else(|| "Document name is unavailable".to_string())?
        .to_os_string();
    let directory = Dir::open_ambient_dir(&parent, cap_std::ambient_authority())
        .map_err(|error| error.to_string())?;
    Ok((directory, parent, name))
}

fn ensure_destination_absent(directory: &Dir, name: impl AsRef<Path>) -> Result<(), String> {
    match directory.symlink_metadata(name) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err("Document destination is unsafe".to_string())
        }
        Ok(_) => Err("A document already exists at the requested destination".to_string()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn random_staging_name() -> Result<String, String> {
    let mut entropy = [0_u8; 16];
    getrandom::fill(&mut entropy).map_err(|error| error.to_string())?;
    let encoded = entropy
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(format!("{UPDATE_TEMP_PREFIX}{encoded}.tmp"))
}

fn random_rename_staging_name() -> Result<String, String> {
    let mut entropy = [0_u8; 16];
    getrandom::fill(&mut entropy).map_err(|error| error.to_string())?;
    let encoded = entropy
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(format!("{RENAME_TEMP_PREFIX}{encoded}.tmp"))
}

fn stage_document_contents_with_candidates(
    directory: &Dir,
    bytes: &[u8],
    candidates: impl IntoIterator<Item = String>,
) -> Result<String, String> {
    for name in candidates {
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = match directory.open_with(&name, &options) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.to_string()),
        };
        if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
            drop(file);
            let _cleanup_result = directory.remove_file(&name);
            return Err(error.to_string());
        }
        drop(file);
        return Ok(name);
    }
    Err("Document staging could not be created".to_string())
}

pub(super) fn stage_document_contents(directory: &Dir, bytes: &[u8]) -> Result<String, String> {
    let candidates = (0..32)
        .map(|_| random_staging_name())
        .collect::<Result<Vec<_>, _>>()?;
    stage_document_contents_with_candidates(directory, bytes, candidates)
}

fn rename_document_noreplace(
    source: &Dir,
    source_name: impl AsRef<Path>,
    destination: &Dir,
    destination_name: impl AsRef<Path>,
    _source_ambient: &Path,
    _destination_ambient: &Path,
) -> io::Result<()> {
    crate::atomic_noreplace::rename_noreplace(
        source,
        source_name.as_ref(),
        destination,
        destination_name.as_ref(),
    )
}

#[cfg(unix)]
pub(super) fn replace_document_atomic(
    directory: &Dir,
    staging_name: &str,
    target_name: &std::ffi::OsStr,
    _staging_ambient: &Path,
    _target_ambient: &Path,
) -> io::Result<()> {
    directory.rename(staging_name, directory, target_name)
}

#[cfg(windows)]
pub(super) fn replace_document_atomic(
    _directory: &Dir,
    _staging_name: &str,
    _target_name: &std::ffi::OsStr,
    staging_ambient: &Path,
    target_ambient: &Path,
) -> io::Result<()> {
    use std::{os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::Storage::FileSystem::{ReplaceFileW, REPLACEFILE_WRITE_THROUGH};

    let target = target_ambient
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let staging = staging_ambient
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let replaced = unsafe {
        ReplaceFileW(
            target.as_ptr(),
            staging.as_ptr(),
            ptr::null(),
            REPLACEFILE_WRITE_THROUGH,
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    if replaced == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(any(unix, windows)))]
pub(super) fn replace_document_atomic(
    directory: &Dir,
    staging_name: &str,
    target_name: &std::ffi::OsStr,
    _staging_ambient: &Path,
    _target_ambient: &Path,
) -> io::Result<()> {
    directory.rename(staging_name, directory, target_name)
}

#[cfg(unix)]
pub(super) fn sync_directory(directory: &Dir) -> io::Result<()> {
    rustix::fs::fsync(directory).map_err(Into::into)
}

#[cfg(not(unix))]
pub(super) fn sync_directory(_directory: &Dir) -> io::Result<()> {
    Ok(())
}

pub(super) fn write_trusted_file_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let (directory, parent, name) = trusted_parent(path)?;
    let target_exists = match directory.symlink_metadata(&name) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err("Document target is unsafe".to_string());
        }
        Ok(_) => true,
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => return Err(error.to_string()),
    };
    let staging_name = stage_document_contents(&directory, bytes)?;
    let staging_ambient = parent.join(&staging_name);
    let publish_result = if target_exists {
        replace_document_atomic(&directory, &staging_name, &name, &staging_ambient, path)
    } else {
        rename_document_noreplace(
            &directory,
            &staging_name,
            &directory,
            &name,
            &staging_ambient,
            path,
        )
    };
    if let Err(error) = publish_result {
        let _cleanup_result = directory.remove_file(&staging_name);
        return Err(error.to_string());
    }
    let _sync_result = sync_directory(&directory);
    Ok(())
}

pub(super) fn create_trusted_file_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let (directory, parent, name) = trusted_parent(path)?;
    ensure_destination_absent(&directory, &name)?;
    let staging_name = stage_document_contents(&directory, bytes)?;
    let staging_ambient = parent.join(&staging_name);
    if let Err(error) = rename_document_noreplace(
        &directory,
        &staging_name,
        &directory,
        &name,
        &staging_ambient,
        path,
    ) {
        let _cleanup_result = directory.remove_file(&staging_name);
        return Err(error.to_string());
    }
    let _sync_result = sync_directory(&directory);
    Ok(())
}

fn same_entry_identity(left: &cap_std::fs::Metadata, right: &cap_std::fs::Metadata) -> bool {
    !left.file_type().is_symlink()
        && !right.file_type().is_symlink()
        && left.is_file() == right.is_file()
        && left.is_dir() == right.is_dir()
        && left.dev() == right.dev()
        && left.ino() == right.ino()
}

fn directory_contains_exact_name(directory: &Dir, name: &OsStr) -> Result<bool, String> {
    for entry in directory.entries().map_err(|error| error.to_string())? {
        if entry.map_err(|error| error.to_string())?.file_name() == name {
            return Ok(true);
        }
    }
    Ok(false)
}

fn destination_is_case_only_source_alias(
    source_parent: &Dir,
    source_name: &OsStr,
    source_metadata: &cap_std::fs::Metadata,
    target_parent: &Dir,
    target_name: &OsStr,
    target_metadata: &cap_std::fs::Metadata,
) -> Result<bool, String> {
    if source_name == target_name
        || crate::storage_capability::directory_identity(source_parent)
            .map_err(|error| error.to_string())?
            != crate::storage_capability::directory_identity(target_parent)
                .map_err(|error| error.to_string())?
        || !same_entry_identity(source_metadata, target_metadata)
    {
        return Ok(false);
    }

    let source_name_exists = directory_contains_exact_name(source_parent, source_name)?;
    let target_name_exists = directory_contains_exact_name(source_parent, target_name)?;
    Ok(source_name_exists && !target_name_exists)
}

fn verify_entry_identity(
    directory: &Dir,
    name: &OsStr,
    expected: &cap_std::fs::Metadata,
) -> Result<(), String> {
    let actual = directory
        .symlink_metadata(name)
        .map_err(|error| error.to_string())?;
    if same_entry_identity(&actual, expected) {
        Ok(())
    } else {
        Err(CASE_ONLY_RENAME_RECOVERY_ERROR.to_string())
    }
}

fn revalidate_retained_directory(directory: &Dir, ambient_path: &Path) -> Result<(), String> {
    let expected = crate::storage_capability::directory_identity(directory)
        .map_err(|error| error.to_string())?;
    let reopened = Dir::open_ambient_dir(ambient_path, cap_std::ambient_authority())
        .map_err(|error| error.to_string())?;
    let actual = crate::storage_capability::directory_identity(&reopened)
        .map_err(|error| error.to_string())?;
    if actual == expected {
        Ok(())
    } else {
        Err("Document parent changed during rename".to_string())
    }
}

fn rollback_case_only_rename(
    directory: &Dir,
    parent_path: &Path,
    current_name: &OsStr,
    source_name: &OsStr,
    expected: &cap_std::fs::Metadata,
) -> Result<(), String> {
    verify_entry_identity(directory, current_name, expected)?;
    rename_document_noreplace(
        directory,
        current_name,
        directory,
        source_name,
        &parent_path.join(current_name),
        &parent_path.join(source_name),
    )
    .map_err(|_| CASE_ONLY_RENAME_RECOVERY_ERROR.to_string())?;
    verify_entry_identity(directory, source_name, expected)
}

fn move_case_only_path_with_candidates_and_hook<AfterStage>(
    directory: &Dir,
    parent_path: &Path,
    source_name: &OsStr,
    target_name: &OsStr,
    source_metadata: &cap_std::fs::Metadata,
    candidates: impl IntoIterator<Item = String>,
    after_stage: AfterStage,
) -> Result<(), String>
where
    AfterStage: FnOnce(),
{
    let mut staged_name = None;
    for candidate in candidates {
        match rename_document_noreplace(
            directory,
            source_name,
            directory,
            &candidate,
            &parent_path.join(source_name),
            &parent_path.join(&candidate),
        ) {
            Ok(()) => {
                staged_name = Some(candidate);
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.to_string()),
        }
    }
    let staged_name =
        staged_name.ok_or_else(|| "Document staging could not be created".to_string())?;
    let staged_os_name = OsStr::new(&staged_name);
    if verify_entry_identity(directory, staged_os_name, source_metadata).is_err() {
        return Err(CASE_ONLY_RENAME_RECOVERY_ERROR.to_string());
    }

    after_stage();
    if let Err(error) = revalidate_retained_directory(directory, parent_path) {
        rollback_case_only_rename(
            directory,
            parent_path,
            staged_os_name,
            source_name,
            source_metadata,
        )?;
        return Err(error);
    }

    if let Err(error) = rename_document_noreplace(
        directory,
        staged_os_name,
        directory,
        target_name,
        &parent_path.join(staged_os_name),
        &parent_path.join(target_name),
    ) {
        rollback_case_only_rename(
            directory,
            parent_path,
            staged_os_name,
            source_name,
            source_metadata,
        )?;
        return Err(if error.kind() == io::ErrorKind::AlreadyExists {
            "File already exists".to_string()
        } else {
            error.to_string()
        });
    }

    if verify_entry_identity(directory, target_name, source_metadata).is_err() {
        return Err(CASE_ONLY_RENAME_RECOVERY_ERROR.to_string());
    }
    if let Err(error) = revalidate_retained_directory(directory, parent_path) {
        rollback_case_only_rename(
            directory,
            parent_path,
            target_name,
            source_name,
            source_metadata,
        )?;
        return Err(error);
    }
    let _sync_result = sync_directory(directory);
    Ok(())
}

pub(super) fn move_trusted_path_noreplace(source: &Path, target: &Path) -> Result<(), String> {
    let (source_parent, source_parent_path, source_name) = trusted_parent(source)?;
    let (target_parent, _target_parent_path, target_name) = trusted_parent(target)?;
    let source_metadata = source_parent
        .symlink_metadata(&source_name)
        .map_err(|error| error.to_string())?;
    if source_metadata.file_type().is_symlink() {
        return Err("Document source is unsafe".to_string());
    }

    match target_parent.symlink_metadata(&target_name) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err("Document destination is unsafe".to_string());
        }
        Ok(metadata)
            if destination_is_case_only_source_alias(
                &source_parent,
                &source_name,
                &source_metadata,
                &target_parent,
                &target_name,
                &metadata,
            )? =>
        {
            let candidates = (0..32)
                .map(|_| random_rename_staging_name())
                .collect::<Result<Vec<_>, _>>()?;
            return move_case_only_path_with_candidates_and_hook(
                &source_parent,
                &source_parent_path,
                &source_name,
                &target_name,
                &source_metadata,
                candidates,
                || {},
            );
        }
        Ok(_) => return Err("File already exists".to_string()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }

    rename_document_noreplace(
        &source_parent,
        &source_name,
        &target_parent,
        &target_name,
        source,
        target,
    )
    .map_err(|error| {
        if error.kind() == io::ErrorKind::AlreadyExists {
            "File already exists".to_string()
        } else {
            error.to_string()
        }
    })?;
    let _source_sync_result = sync_directory(&source_parent);
    let _target_sync_result = sync_directory(&target_parent);
    Ok(())
}

pub(super) fn delete_trusted_file(path: &Path) -> Result<(), String> {
    let (parent, _parent_path, name) = trusted_parent(path)?;
    let metadata = parent
        .symlink_metadata(&name)
        .map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("Document target is unsafe".to_string());
    }
    parent
        .remove_file(&name)
        .map_err(|error| error.to_string())?;
    let _sync_result = sync_directory(&parent);
    Ok(())
}

pub(super) fn read_trusted_markdown_file(path: &Path) -> Result<MarkdownFile, String> {
    let size_bytes = std::fs::metadata(path)
        .map_err(|error| error.to_string())?
        .len();
    let contents = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    Ok(MarkdownFile {
        path: path.to_string_lossy().to_string(),
        contents,
        size_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_trusted_path_allows_a_case_only_file_name_change() {
        let root = tempfile::tempdir().expect("fixture should be created");
        let source_path = root.path().join("note.md");
        let target_path = root.path().join("Note.md");
        std::fs::write(&source_path, b"case-only rename").expect("source should be created");

        move_trusted_path_noreplace(&source_path, &target_path)
            .expect("case-only rename should succeed");

        assert_eq!(
            std::fs::read(&target_path).expect("renamed file should remain readable"),
            b"case-only rename"
        );
        assert_eq!(
            std::fs::read_dir(root.path())
                .expect("fixture directory should be readable")
                .map(|entry| entry.expect("fixture entry should be readable").file_name())
                .collect::<Vec<_>>(),
            vec![std::ffi::OsString::from("Note.md")]
        );
    }

    #[test]
    fn move_trusted_path_allows_a_filesystem_equivalent_unicode_case_change() {
        let root = tempfile::tempdir().expect("fixture should be created");
        let source_path = root.path().join("ẞ.md");
        let target_path = root.path().join("ss.md");
        std::fs::write(&source_path, b"unicode case rename").expect("source should be created");

        move_trusted_path_noreplace(&source_path, &target_path)
            .expect("filesystem-equivalent Unicode case rename should succeed");

        assert_eq!(
            std::fs::read(&target_path).expect("renamed file should remain readable"),
            b"unicode case rename"
        );
        assert_eq!(
            std::fs::read_dir(root.path())
                .expect("fixture directory should be readable")
                .map(|entry| entry.expect("fixture entry should be readable").file_name())
                .collect::<Vec<_>>(),
            vec![std::ffi::OsString::from("ss.md")]
        );
    }

    #[test]
    fn case_only_move_retries_a_staging_collision_without_clobbering() {
        let root = tempfile::tempdir().expect("fixture should be created");
        let source_name = OsStr::new("note.md");
        let target_name = OsStr::new("Note.md");
        let collision = format!("{RENAME_TEMP_PREFIX}collision.tmp");
        let unique = format!("{RENAME_TEMP_PREFIX}unique.tmp");
        std::fs::write(root.path().join(source_name), b"source").expect("source should be created");
        std::fs::write(root.path().join(&collision), b"collision")
            .expect("collision should be created");
        let directory = Dir::open_ambient_dir(root.path(), cap_std::ambient_authority())
            .expect("fixture directory should open");
        let source_metadata = directory
            .symlink_metadata(source_name)
            .expect("source metadata should be available");

        move_case_only_path_with_candidates_and_hook(
            &directory,
            root.path(),
            source_name,
            target_name,
            &source_metadata,
            [collision.clone(), unique.clone()],
            || {},
        )
        .expect("second staging name should permit the rename");

        assert_eq!(
            std::fs::read(root.path().join(target_name)).unwrap(),
            b"source"
        );
        assert_eq!(
            std::fs::read(root.path().join(collision)).unwrap(),
            b"collision"
        );
        assert!(!root.path().join(unique).exists());
    }

    #[test]
    fn case_only_move_fails_closed_when_the_destination_appears_after_staging() {
        let root = tempfile::tempdir().expect("fixture should be created");
        let source_name = OsStr::new("note.md");
        let target_name = OsStr::new("Note.md");
        let staging = format!("{RENAME_TEMP_PREFIX}rollback.tmp");
        std::fs::write(root.path().join(source_name), b"source").expect("source should be created");
        let directory = Dir::open_ambient_dir(root.path(), cap_std::ambient_authority())
            .expect("fixture directory should open");
        let source_metadata = directory
            .symlink_metadata(source_name)
            .expect("source metadata should be available");

        let error = move_case_only_path_with_candidates_and_hook(
            &directory,
            root.path(),
            source_name,
            target_name,
            &source_metadata,
            [staging.clone()],
            || {
                std::fs::write(root.path().join(target_name), b"destination")
                    .expect("destination race fixture should be created");
            },
        )
        .expect_err("a destination race must reject the rename");

        assert_eq!(error, CASE_ONLY_RENAME_RECOVERY_ERROR);
        assert_eq!(
            std::fs::read(root.path().join(target_name)).unwrap(),
            b"destination"
        );
        assert_eq!(std::fs::read(root.path().join(staging)).unwrap(), b"source");
    }

    #[test]
    fn case_only_move_rolls_back_through_the_retained_parent_when_ambient_parent_changes() {
        let root = tempfile::tempdir().expect("fixture should be created");
        let parent = root.path().join("parent");
        let saved_parent = root.path().join("saved-parent");
        let source_name = OsStr::new("note.md");
        let target_name = OsStr::new("Note.md");
        let staging = format!("{RENAME_TEMP_PREFIX}rollback.tmp");
        std::fs::create_dir(&parent).expect("parent should be created");
        std::fs::write(parent.join(source_name), b"source").expect("source should be created");
        let directory = Dir::open_ambient_dir(&parent, cap_std::ambient_authority())
            .expect("fixture directory should open");
        let source_metadata = directory
            .symlink_metadata(source_name)
            .expect("source metadata should be available");

        let error = move_case_only_path_with_candidates_and_hook(
            &directory,
            &parent,
            source_name,
            target_name,
            &source_metadata,
            [staging.clone()],
            || {
                std::fs::rename(&parent, &saved_parent).expect("ambient parent should be replaced");
                std::fs::create_dir(&parent).expect("replacement parent should be created");
            },
        )
        .expect_err("a replaced ambient parent must reject the rename");

        assert_eq!(error, "Document parent changed during rename");
        assert_eq!(
            std::fs::read(saved_parent.join(source_name)).unwrap(),
            b"source"
        );
        assert!(!saved_parent.join(staging).exists());
        assert!(std::fs::read_dir(parent)
            .expect("replacement parent should remain readable")
            .next()
            .is_none());
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn rename_markdown_tree_file_primitive_preserves_an_existing_destination() {
        let root = tempfile::tempdir().expect("fixture should be created");
        let source_path = root.path().join("A.md");
        let destination_path = root.path().join("B.md");
        let source_contents = [0_u8, 1, 2, 3, 255];
        let destination_contents = [255_u8, 3, 2, 1, 0];
        std::fs::write(&source_path, source_contents).expect("source should be created");
        std::fs::write(&destination_path, destination_contents)
            .expect("destination should be created");
        let source_directory = Dir::open_ambient_dir(root.path(), cap_std::ambient_authority())
            .expect("source directory should open");
        let destination_directory =
            Dir::open_ambient_dir(root.path(), cap_std::ambient_authority())
                .expect("destination directory should open");

        let result = rename_document_noreplace(
            &source_directory,
            "A.md",
            &destination_directory,
            "B.md",
            &source_path,
            &destination_path,
        );

        assert!(
            result.is_err(),
            "an existing destination must reject the rename"
        );
        assert_eq!(
            std::fs::read(&source_path).expect("source should remain readable"),
            source_contents
        );
        assert_eq!(
            std::fs::read(&destination_path).expect("destination should remain readable"),
            destination_contents
        );
    }

    #[cfg(not(any(unix, windows)))]
    #[test]
    fn rename_markdown_tree_file_primitive_reports_unsupported() {
        let root = tempfile::tempdir().expect("fixture should be created");
        let source_path = root.path().join("A.md");
        let destination_path = root.path().join("B.md");
        std::fs::write(&source_path, b"source").expect("source should be created");
        let source_directory = Dir::open_ambient_dir(root.path(), cap_std::ambient_authority())
            .expect("source directory should open");
        let destination_directory =
            Dir::open_ambient_dir(root.path(), cap_std::ambient_authority())
                .expect("destination directory should open");

        let error = rename_document_noreplace(
            &source_directory,
            "A.md",
            &destination_directory,
            "B.md",
            &source_path,
            &destination_path,
        )
        .expect_err("unsupported targets must fail closed");

        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
    }

    #[test]
    fn staging_retries_an_injected_collision_without_clobbering() {
        let root = tempfile::tempdir().expect("fixture should be created");
        let directory = Dir::open_ambient_dir(root.path(), cap_std::ambient_authority())
            .expect("fixture directory should open");
        let collision = format!("{UPDATE_TEMP_PREFIX}collision.tmp");
        let unique = format!("{UPDATE_TEMP_PREFIX}unique.tmp");
        std::fs::write(root.path().join(&collision), b"existing")
            .expect("collision should be created");

        let staged = stage_document_contents_with_candidates(
            &directory,
            b"replacement",
            [collision.clone(), unique.clone()],
        )
        .expect("the second candidate should be staged");

        assert_eq!(staged, unique);
        assert_eq!(
            std::fs::read(root.path().join(collision)).expect("collision should remain readable"),
            b"existing"
        );
        assert_eq!(
            std::fs::read(root.path().join(staged)).expect("staging should be readable"),
            b"replacement"
        );
    }

    #[test]
    fn generated_staging_candidates_encode_128_bits_of_randomness() {
        let candidate = random_staging_name().expect("random staging should be available");

        assert!(candidate.starts_with(UPDATE_TEMP_PREFIX));
        assert!(candidate.ends_with(".tmp"));
        assert_eq!(
            candidate.len(),
            UPDATE_TEMP_PREFIX.len() + 32 + ".tmp".len()
        );
        assert!(
            candidate[UPDATE_TEMP_PREFIX.len()..candidate.len() - ".tmp".len()]
                .chars()
                .all(|character| character.is_ascii_hexdigit())
        );
    }
}
