use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::{random_hash, RepoError};

const TEMP_CREATE_ATTEMPTS: usize = 32;
const WINDOWS_RENAME_ATTEMPTS: usize = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PublishOutcome {
    Published,
    AlreadyExists,
}

pub(crate) struct StagedFile {
    temp_path: PathBuf,
    destination: PathBuf,
    cleanup_armed: bool,
}

pub fn write_file_safer(path: &Path, bytes: &[u8], mode: u32) -> Result<(), RepoError> {
    stage_file(path, bytes, mode)?.publish_replace()
}

pub(crate) fn stage_file(path: &Path, bytes: &[u8], mode: u32) -> Result<StagedFile, RepoError> {
    let (temp_path, mut temp_file) = create_temp_file(path)?;
    let result = (|| -> Result<(), RepoError> {
        temp_file.write_all(bytes)?;
        temp_file.sync_all()?;
        drop(temp_file);
        set_mode(&temp_path, mode)?;
        Ok(())
    })();

    match result {
        Ok(()) => Ok(StagedFile {
            temp_path,
            destination: path.to_owned(),
            cleanup_armed: true,
        }),
        Err(error) => {
            let _cleanup_result = fs::remove_file(&temp_path);
            Err(error)
        }
    }
}

impl StagedFile {
    pub(crate) fn path(&self) -> &Path {
        &self.temp_path
    }

    pub(crate) fn publish_replace(mut self) -> Result<(), RepoError> {
        replace_with_retry(&self.temp_path, &self.destination)?;
        self.cleanup_armed = false;
        Ok(())
    }

    pub(crate) fn publish_no_replace(mut self) -> Result<PublishOutcome, RepoError> {
        match fs::hard_link(&self.temp_path, &self.destination) {
            Ok(()) => {
                self.remove_temp()?;
                Ok(PublishOutcome::Published)
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let metadata = fs::symlink_metadata(&self.destination)?;
                if !metadata.file_type().is_file() {
                    return Err(RepoError::InvalidData(
                        "immutable object destination must be a regular file",
                    ));
                }
                self.remove_temp()?;
                Ok(PublishOutcome::AlreadyExists)
            }
            Err(error) => Err(error.into()),
        }
    }

    fn remove_temp(&mut self) -> Result<(), RepoError> {
        fs::remove_file(&self.temp_path)?;
        self.cleanup_armed = false;
        Ok(())
    }
}

impl Drop for StagedFile {
    fn drop(&mut self) {
        if self.cleanup_armed {
            let _cleanup_result = fs::remove_file(&self.temp_path);
        }
    }
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

fn replace_with_retry(from: &Path, to: &Path) -> Result<(), RepoError> {
    for attempt in 0..WINDOWS_RENAME_ATTEMPTS {
        match replace_file(from, to) {
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

#[cfg(not(windows))]
fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
    fs::rename(from, to)
}

#[cfg(windows)]
fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let from_wide = from
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let to_wide = to
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        MoveFileExW(
            from_wide.as_ptr(),
            to_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
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
    use std::sync::{Arc, Barrier};

    use super::{stage_file, write_file_safer, PublishOutcome};

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

    #[test]
    fn staged_no_replace_preserves_an_existing_regular_file() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("object");
        fs::write(&destination, b"first").unwrap();

        let outcome = stage_file(&destination, b"second", 0o644)
            .unwrap()
            .publish_no_replace()
            .unwrap();

        assert_eq!(outcome, PublishOutcome::AlreadyExists);
        assert_eq!(fs::read(&destination).unwrap(), b"first");
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
    }

    #[test]
    fn staged_no_replace_race_has_exactly_one_winner() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("object");
        let first = stage_file(&destination, b"first", 0o644).unwrap();
        let second = stage_file(&destination, b"second", 0o644).unwrap();
        let barrier = Arc::new(Barrier::new(2));

        let first_barrier = Arc::clone(&barrier);
        let first_thread = std::thread::spawn(move || {
            first_barrier.wait();
            (first.publish_no_replace().unwrap(), b"first".as_slice())
        });
        let second_thread = std::thread::spawn(move || {
            barrier.wait();
            (second.publish_no_replace().unwrap(), b"second".as_slice())
        });
        let first_result = first_thread.join().unwrap();
        let second_result = second_thread.join().unwrap();

        let outcomes = [first_result.0, second_result.0];
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| **outcome == PublishOutcome::Published)
                .count(),
            1
        );
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| **outcome == PublishOutcome::AlreadyExists)
                .count(),
            1
        );
        let winning_bytes = if first_result.0 == PublishOutcome::Published {
            first_result.1
        } else {
            second_result.1
        };
        assert_eq!(fs::read(&destination).unwrap(), winning_bytes);
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
    }

    #[test]
    fn staged_no_replace_rejects_a_directory_destination() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("object");
        fs::create_dir(&destination).unwrap();

        let error = stage_file(&destination, b"data", 0o644)
            .unwrap()
            .publish_no_replace()
            .unwrap_err();

        assert!(matches!(
            error,
            crate::RepoError::InvalidData("immutable object destination must be a regular file")
        ));
        assert!(destination.is_dir());
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn staged_no_replace_rejects_a_symlink_destination() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target");
        let destination = temp.path().join("object");
        fs::write(&target, b"target").unwrap();
        symlink(&target, &destination).unwrap();

        let error = stage_file(&destination, b"data", 0o644)
            .unwrap()
            .publish_no_replace()
            .unwrap_err();

        assert!(matches!(
            error,
            crate::RepoError::InvalidData("immutable object destination must be a regular file")
        ));
        assert_eq!(fs::read_link(&destination).unwrap(), target);
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
