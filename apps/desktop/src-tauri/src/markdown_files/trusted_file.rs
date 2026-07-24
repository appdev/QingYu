use std::{
    io::{self, Write},
    path::{Path, PathBuf},
};

use cap_fs_ext::{FollowSymlinks, OpenOptionsExt, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};

use super::types::MarkdownFile;

const UPDATE_TEMP_PREFIX: &str = ".qingyu-ui-update-";

fn trusted_parent(path: &Path) -> Result<(Dir, PathBuf, std::ffi::OsString), String> {
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

fn stage_document_contents(directory: &Dir, bytes: &[u8]) -> Result<String, String> {
    let candidates = (0..32)
        .map(|_| random_staging_name())
        .collect::<Result<Vec<_>, _>>()?;
    stage_document_contents_with_candidates(directory, bytes, candidates)
}

#[cfg(unix)]
fn rename_document_noreplace(
    source: &Dir,
    source_name: impl AsRef<Path>,
    destination: &Dir,
    destination_name: impl AsRef<Path>,
    _source_ambient: &Path,
    _destination_ambient: &Path,
) -> io::Result<()> {
    rustix::fs::renameat_with(
        source,
        source_name.as_ref(),
        destination,
        destination_name.as_ref(),
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(Into::into)
}

#[cfg(windows)]
fn rename_document_noreplace(
    _source: &Dir,
    _source_name: impl AsRef<Path>,
    _destination: &Dir,
    _destination_name: impl AsRef<Path>,
    source_ambient: &Path,
    destination_ambient: &Path,
) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH};

    let source = source_ambient
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination_ambient
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let moved = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(any(unix, windows)))]
fn rename_document_noreplace(
    _source: &Dir,
    _source_name: impl AsRef<Path>,
    _destination: &Dir,
    _destination_name: impl AsRef<Path>,
    _source_ambient: &Path,
    _destination_ambient: &Path,
) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "atomic no-overwrite rename is unsupported",
    ))
}

#[cfg(unix)]
fn replace_document_atomic(
    directory: &Dir,
    staging_name: &str,
    target_name: &std::ffi::OsStr,
    _staging_ambient: &Path,
    _target_ambient: &Path,
) -> io::Result<()> {
    directory.rename(staging_name, directory, target_name)
}

#[cfg(windows)]
fn replace_document_atomic(
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
fn replace_document_atomic(
    directory: &Dir,
    staging_name: &str,
    target_name: &std::ffi::OsStr,
    _staging_ambient: &Path,
    _target_ambient: &Path,
) -> io::Result<()> {
    directory.rename(staging_name, directory, target_name)
}

#[cfg(unix)]
fn sync_directory(directory: &Dir) -> io::Result<()> {
    rustix::fs::fsync(directory).map_err(Into::into)
}

#[cfg(not(unix))]
fn sync_directory(_directory: &Dir) -> io::Result<()> {
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

pub(super) fn move_trusted_path_noreplace(source: &Path, target: &Path) -> Result<(), String> {
    let (source_parent, _source_parent_path, source_name) = trusted_parent(source)?;
    let (target_parent, _target_parent_path, target_name) = trusted_parent(target)?;
    let source_metadata = source_parent
        .symlink_metadata(&source_name)
        .map_err(|error| error.to_string())?;
    if source_metadata.file_type().is_symlink() {
        return Err("Document source is unsafe".to_string());
    }
    ensure_destination_absent(&target_parent, &target_name)?;
    rename_document_noreplace(
        &source_parent,
        &source_name,
        &target_parent,
        &target_name,
        source,
        target,
    )
    .map_err(|error| error.to_string())?;
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
