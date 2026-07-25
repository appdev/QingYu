use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, File as CapFile, OpenOptions as CapOpenOptions};

use crate::path_security::{
    cap_metadata_is_safe_immutable_destination, std_metadata_is_safe_immutable_destination,
};
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

pub(crate) struct CapStagedFile {
    stage_parent: Dir,
    destination_parent: Dir,
    temp_name: OsString,
    destination: OsString,
    temp_file: CapFile,
    cleanup_armed: bool,
}

pub fn write_file_safer(path: &Path, bytes: &[u8], mode: u32) -> Result<(), RepoError> {
    stage_file(path, bytes, mode)?.publish_replace()
}

pub fn write_cap_file_safer(
    parent: &Dir,
    destination: &OsStr,
    bytes: &[u8],
    mode: u32,
) -> Result<(), RepoError> {
    stage_cap_file(parent, destination, bytes, mode)?.publish_replace()
}

pub(crate) fn stage_file(path: &Path, bytes: &[u8], mode: u32) -> Result<StagedFile, RepoError> {
    let (temp_path, mut temp_file) = create_temp_file(path, mode)?;
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

pub(crate) fn stage_cap_file(
    parent: &Dir,
    destination: &std::ffi::OsStr,
    bytes: &[u8],
    mode: u32,
) -> Result<CapStagedFile, RepoError> {
    stage_cap_file_in(parent, parent, destination, bytes, mode)
}

pub(crate) fn stage_cap_file_in(
    stage_parent: &Dir,
    destination_parent: &Dir,
    destination: &std::ffi::OsStr,
    bytes: &[u8],
    mode: u32,
) -> Result<CapStagedFile, RepoError> {
    let staged = create_cap_staged_file_in(stage_parent, destination_parent, destination, mode)?;
    let result = (|| -> Result<(), RepoError> {
        let mut temp_file = staged.file();
        temp_file.write_all(bytes)?;
        temp_file.sync_all()?;
        set_cap_mode(temp_file, mode)?;
        Ok(())
    })();

    match result {
        Ok(()) => Ok(staged),
        Err(error) => Err(error),
    }
}

pub(crate) fn create_cap_staged_file(
    parent: &Dir,
    destination: &std::ffi::OsStr,
    mode: u32,
) -> Result<CapStagedFile, RepoError> {
    create_cap_staged_file_in(parent, parent, destination, mode)
}

pub(crate) fn create_cap_staged_file_in(
    stage_parent: &Dir,
    destination_parent: &Dir,
    destination: &std::ffi::OsStr,
    mode: u32,
) -> Result<CapStagedFile, RepoError> {
    for _attempt in 0..TEMP_CREATE_ATTEMPTS {
        let random = random_hash().map_err(|_| RepoError::RandomnessUnavailable)?;
        let mut temp_name = OsString::from("stage-");
        temp_name.push(random);
        temp_name.push(".tmp");
        let mut options = CapOpenOptions::new();
        options
            .read(true)
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        configure_cap_temp_options(&mut options, mode);
        match stage_parent.open_with(&temp_name, &options) {
            Ok(temp_file) => {
                return Ok(CapStagedFile {
                    stage_parent: stage_parent.try_clone()?,
                    destination_parent: destination_parent.try_clone()?,
                    temp_name,
                    destination: destination.to_os_string(),
                    temp_file,
                    cleanup_armed: true,
                })
            }
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

impl StagedFile {
    pub(crate) fn publish_replace(mut self) -> Result<(), RepoError> {
        replace_with_retry(&self.temp_path, &self.destination)?;
        self.cleanup_armed = false;
        Ok(())
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn publish_no_replace(mut self) -> Result<PublishOutcome, RepoError> {
        match fs::hard_link(&self.temp_path, &self.destination) {
            Ok(()) => {
                self.remove_temp()?;
                Ok(PublishOutcome::Published)
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let metadata = fs::symlink_metadata(&self.destination)?;
                if !std_metadata_is_safe_immutable_destination(&metadata) {
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

    #[cfg_attr(not(test), allow(dead_code))]
    fn remove_temp(&mut self) -> Result<(), RepoError> {
        fs::remove_file(&self.temp_path)?;
        self.cleanup_armed = false;
        Ok(())
    }
}

impl CapStagedFile {
    pub(crate) fn file(&self) -> &CapFile {
        &self.temp_file
    }

    pub(crate) fn publish_replace(mut self) -> Result<(), RepoError> {
        replace_cap_with_retry(
            &self.stage_parent,
            &self.destination_parent,
            &self.temp_file,
            &self.temp_name,
            &self.destination,
        )?;
        self.cleanup_armed = false;
        Ok(())
    }

    pub(crate) fn publish_replace_retaining_handle(mut self) -> Result<CapFile, RepoError> {
        let published = self.temp_file.try_clone()?;
        replace_cap_with_retry(
            &self.stage_parent,
            &self.destination_parent,
            &self.temp_file,
            &self.temp_name,
            &self.destination,
        )?;
        self.cleanup_armed = false;
        Ok(published)
    }

    pub(crate) fn publish_no_replace(mut self) -> Result<PublishOutcome, RepoError> {
        match self.stage_parent.hard_link(
            &self.temp_name,
            &self.destination_parent,
            &self.destination,
        ) {
            Ok(()) => {
                self.remove_temp()?;
                Ok(PublishOutcome::Published)
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let metadata = self
                    .destination_parent
                    .symlink_metadata(&self.destination)?;
                if !cap_metadata_is_safe_immutable_destination(&metadata) {
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
        self.stage_parent.remove_file(&self.temp_name)?;
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

impl Drop for CapStagedFile {
    fn drop(&mut self) {
        if self.cleanup_armed {
            let _cleanup_result = self.stage_parent.remove_file(&self.temp_name);
        }
    }
}

#[cfg(unix)]
fn configure_cap_temp_options(options: &mut CapOpenOptions, mode: u32) {
    use cap_std::fs::OpenOptionsExt;

    options.mode(mode);
}

#[cfg(windows)]
fn configure_cap_temp_options(options: &mut CapOpenOptions, _mode: u32) {
    use cap_std::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_WRITE_ATTRIBUTES,
    };

    options.access_mode(FILE_GENERIC_READ | FILE_GENERIC_WRITE | FILE_WRITE_ATTRIBUTES | DELETE);
}

#[cfg(not(any(unix, windows)))]
fn configure_cap_temp_options(_options: &mut CapOpenOptions, _mode: u32) {}

#[cfg(unix)]
fn set_cap_mode(file: &CapFile, mode: u32) -> Result<(), RepoError> {
    use cap_std::fs::PermissionsExt;

    file.set_permissions(cap_std::fs::Permissions::from_mode(mode))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_cap_mode(_file: &CapFile, _mode: u32) -> Result<(), RepoError> {
    Ok(())
}

fn replace_cap_with_retry(
    stage_parent: &Dir,
    destination_parent: &Dir,
    temp_file: &CapFile,
    from: &std::ffi::OsStr,
    to: &std::ffi::OsStr,
) -> Result<(), RepoError> {
    for attempt in 0..WINDOWS_RENAME_ATTEMPTS {
        match replace_cap_file(stage_parent, destination_parent, temp_file, from, to) {
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
fn replace_cap_file(
    stage_parent: &Dir,
    destination_parent: &Dir,
    _temp_file: &CapFile,
    from: &std::ffi::OsStr,
    to: &std::ffi::OsStr,
) -> io::Result<()> {
    stage_parent.rename(from, destination_parent, to)
}

#[cfg(windows)]
fn replace_cap_file(
    _stage_parent: &Dir,
    destination_parent: &Dir,
    temp_file: &CapFile,
    _from: &std::ffi::OsStr,
    to: &std::ffi::OsStr,
) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FileRenameInfo, SetFileInformationByHandle, FILE_RENAME_INFO,
    };

    let wide_name = to.encode_wide().collect::<Vec<_>>();
    let header_size = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
    let buffer_size = header_size + wide_name.len() * std::mem::size_of::<u16>();
    let mut buffer = vec![0_u8; buffer_size];
    let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    unsafe {
        (*info).Anonymous.ReplaceIfExists = true;
        (*info).RootDirectory = destination_parent.as_raw_handle();
        (*info).FileNameLength = u32::try_from(wide_name.len() * 2)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "file name is too long"))?;
        std::ptr::copy_nonoverlapping(
            wide_name.as_ptr(),
            (*info).FileName.as_mut_ptr(),
            wide_name.len(),
        );
        if SetFileInformationByHandle(
            temp_file.as_raw_handle(),
            FileRenameInfo,
            buffer.as_ptr().cast(),
            u32::try_from(buffer_size).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "rename buffer is too large")
            })?,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

fn create_temp_file(path: &Path, mode: u32) -> Result<(PathBuf, File), RepoError> {
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
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        configure_std_temp_options(&mut options, mode);
        match options.open(&temp_path) {
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
fn configure_std_temp_options(options: &mut OpenOptions, mode: u32) {
    use std::os::unix::fs::OpenOptionsExt;

    options.mode(mode);
}

#[cfg(not(unix))]
fn configure_std_temp_options(_options: &mut OpenOptions, _mode: u32) {}

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
    use std::ffi::OsStr;
    use std::fs;
    use std::io::{Read, Seek, SeekFrom};
    use std::sync::{Arc, Barrier};

    use cap_std::ambient_authority;
    use cap_std::fs::Dir;

    use super::{
        create_temp_file, stage_cap_file, stage_file, write_cap_file_safer, write_file_safer,
        PublishOutcome,
    };

    #[test]
    fn capability_stage_can_be_rewound_and_read_before_publication() {
        let temp = tempfile::tempdir().unwrap();
        let parent = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
        let staged = stage_cap_file(&parent, OsStr::new("object"), b"staged bytes", 0o600).unwrap();
        let mut staged_file = staged.file();
        staged_file.seek(SeekFrom::Start(0)).unwrap();
        let mut bytes = Vec::new();
        staged_file.read_to_end(&mut bytes).unwrap();

        assert_eq!(bytes, b"staged bytes");
    }

    #[test]
    fn capability_safer_write_stays_with_the_retained_directory_after_path_replacement() {
        let temp = tempfile::tempdir().unwrap();
        let configured = temp.path().join("configured");
        let retained = temp.path().join("retained");
        fs::create_dir(&configured).unwrap();
        let parent = Dir::open_ambient_dir(&configured, ambient_authority()).unwrap();
        fs::rename(&configured, &retained).unwrap();
        fs::create_dir(&configured).unwrap();

        write_cap_file_safer(&parent, OsStr::new("local-sync.json"), b"secret", 0o600).unwrap();

        assert_eq!(
            fs::read(retained.join("local-sync.json")).unwrap(),
            b"secret"
        );
        assert!(fs::read_dir(&configured).unwrap().next().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn ambient_safer_write_temp_is_private_from_its_initial_creation() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("object");

        let (temp_path, temp_file) = create_temp_file(&destination, 0o600).unwrap();

        let mode = temp_file.metadata().unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        drop(temp_file);
        fs::remove_file(temp_path).unwrap();
    }

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
