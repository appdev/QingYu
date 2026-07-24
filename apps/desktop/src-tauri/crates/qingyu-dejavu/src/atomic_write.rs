use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::{random_hash, RepoError};

const TEMP_CREATE_ATTEMPTS: usize = 32;
const WINDOWS_RENAME_ATTEMPTS: usize = 3;

pub fn write_file_safer(path: &Path, bytes: &[u8], mode: u32) -> Result<(), RepoError> {
    let (temp_path, mut temp_file) = create_temp_file(path)?;
    let result = (|| -> Result<(), RepoError> {
        temp_file.write_all(bytes)?;
        temp_file.sync_all()?;
        drop(temp_file);
        set_mode(&temp_path, mode)?;
        rename_with_retry(&temp_path, path)?;
        Ok(())
    })();

    if result.is_err() {
        let _cleanup_result = fs::remove_file(&temp_path);
    }
    result
}

fn create_temp_file(path: &Path) -> Result<(PathBuf, File), RepoError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .ok_or(RepoError::InvalidData("destination must have a file name"))?;

    for _attempt in 0..TEMP_CREATE_ATTEMPTS {
        let random = random_hash().map_err(|_| RepoError::RandomnessUnavailable)?;
        let mut temp_name = OsString::from(file_name);
        temp_name.push(".");
        temp_name.push(random);
        temp_name.push(".tmp");
        let temp_path = parent.join(temp_name);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
        {
            Ok(file) => return Ok((temp_path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not create a unique safer-write temporary file",
    )
    .into())
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> Result<(), RepoError> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> Result<(), RepoError> {
    Ok(())
}

fn rename_with_retry(from: &Path, to: &Path) -> Result<(), RepoError> {
    for attempt in 0..WINDOWS_RENAME_ATTEMPTS {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(error)
                if cfg!(windows)
                    && retryable_windows_rename_error(&error)
                    && attempt + 1 < WINDOWS_RENAME_ATTEMPTS =>
            {
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            Err(error) => return Err(error.into()),
        }
    }
    unreachable!("rename loop returns on its final attempt")
}

#[cfg(windows)]
fn retryable_windows_rename_error(error: &io::Error) -> bool {
    use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_SHARING_VIOLATION};

    matches!(
        error.raw_os_error(),
        Some(code)
            if code == ERROR_ACCESS_DENIED as i32 || code == ERROR_SHARING_VIOLATION as i32
    )
}

#[cfg(not(windows))]
fn retryable_windows_rename_error(_error: &io::Error) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::write_file_safer;

    #[test]
    fn safer_write_replaces_destination_and_leaves_no_owned_temp() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("object");
        fs::write(&destination, b"old").unwrap();

        write_file_safer(&destination, b"new", 0o644).unwrap();

        assert_eq!(fs::read(&destination).unwrap(), b"new");
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
    }

    #[test]
    fn safer_write_failure_removes_only_its_own_temp() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("destination");
        fs::create_dir(&destination).unwrap();
        let unrelated = temp.path().join("unrelated.tmp");
        fs::write(&unrelated, b"keep").unwrap();

        assert!(write_file_safer(&destination, b"new", 0o644).is_err());

        assert!(destination.is_dir());
        assert_eq!(fs::read(&unrelated).unwrap(), b"keep");
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn safer_write_applies_requested_unix_mode() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("object");

        write_file_safer(&destination, b"data", 0o644).unwrap();

        let mode = fs::metadata(destination).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o644);
    }
}
