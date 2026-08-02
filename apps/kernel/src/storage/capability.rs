//! Cross-platform retained filesystem capability helpers.

use std::{io, path::Path};

#[cfg(any(unix, windows))]
use cap_fs_ext::OpenOptionsExt;
use cap_fs_ext::{DirExt, FollowSymlinks, MetadataExt, OpenOptionsFollowExt};
use cap_std::fs::{Dir, File, Metadata, OpenOptions};

use super::atomic_file::CommitState;

pub fn nonfollowing_read_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    #[cfg(unix)]
    options.custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32);
    options
}

pub fn create_private_file_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .follow(FollowSymlinks::No);
    #[cfg(unix)]
    options.mode(0o600);
    options
}

pub fn create_private_replaceable_file_options() -> OpenOptions {
    let mut options = create_private_file_options();
    options.read(true);
    #[cfg(windows)]
    options
        .access_mode(
            windows_sys::Win32::Foundation::GENERIC_READ
                | windows_sys::Win32::Foundation::GENERIC_WRITE
                | windows_sys::Win32::Storage::FileSystem::DELETE,
        )
        .share_mode(windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ);
    options
}

pub fn sync_directory_commit_state(directory: &Dir) -> io::Result<CommitState> {
    #[cfg(unix)]
    {
        let fsyncable = rustix::fs::openat(
            directory,
            ".",
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::CLOEXEC
                | rustix::fs::OFlags::NOFOLLOW,
            rustix::fs::Mode::empty(),
        )?;
        rustix::fs::fsync(&fsyncable)?;
        Ok(CommitState::Durable)
    }
    #[cfg(windows)]
    {
        let _ = directory;
        Ok(CommitState::AtomicVisibility)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = directory;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "directory synchronization is unsupported on this platform",
        ))
    }
}

pub fn sync_directory(directory: &Dir) -> io::Result<()> {
    sync_directory_commit_state(directory).map(drop)
}

#[cfg(test)]
mod directory_sync_tests {
    use super::*;
    use crate::storage::CommitState;

    #[cfg(unix)]
    #[test]
    fn unix_directory_sync_reports_durable() {
        let temporary = tempfile::tempdir().unwrap();
        let directory =
            Dir::open_ambient_dir(temporary.path(), cap_std::ambient_authority()).unwrap();

        assert_eq!(
            sync_directory_commit_state(&directory).unwrap(),
            CommitState::Durable,
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_directory_sync_reports_atomic_visibility() {
        let temporary = tempfile::tempdir().unwrap();
        let directory =
            Dir::open_ambient_dir(temporary.path(), cap_std::ambient_authority()).unwrap();

        assert_eq!(
            sync_directory_commit_state(&directory).unwrap(),
            CommitState::AtomicVisibility,
        );
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn retained_directory_sync_wrapper_succeeds_on_supported_targets() {
        let temporary = tempfile::tempdir().unwrap();
        let directory =
            Dir::open_ambient_dir(temporary.path(), cap_std::ambient_authority()).unwrap();

        #[cfg(target_os = "linux")]
        let flags = rustix::fs::fcntl_getfl(&directory).unwrap();

        #[cfg(target_os = "linux")]
        assert!(flags.contains(rustix::fs::OFlags::PATH));
        assert!(sync_directory(&directory).is_ok());
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DirectoryIdentity {
    device: u64,
    inode: u64,
}

impl DirectoryIdentity {
    pub fn stable_token(self) -> String {
        format!("{:016x}{:016x}", self.device, self.inode)
    }
}

pub fn directory_identity(directory: &Dir) -> io::Result<DirectoryIdentity> {
    let metadata = directory.dir_metadata()?;
    Ok(DirectoryIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

pub fn open_canonical_directory_nofollow(path: &Path) -> io::Result<Dir> {
    let Some(parent) = path.parent() else {
        return Dir::open_ambient_dir(path, cap_std::ambient_authority());
    };
    let Some(name) = path.file_name() else {
        return Dir::open_ambient_dir(path, cap_std::ambient_authority());
    };
    let parent = Dir::open_ambient_dir(parent, cap_std::ambient_authority())?;
    parent.open_dir_nofollow(name)
}

pub fn ambient_symlink_metadata(path: &Path) -> io::Result<Metadata> {
    let Some(name) = path.file_name() else {
        return Dir::open_ambient_dir(path, cap_std::ambient_authority())?.dir_metadata();
    };
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    Dir::open_ambient_dir(parent, cap_std::ambient_authority())?.symlink_metadata(name)
}

#[cfg(unix)]
pub fn rename_in_directory(
    directory: &Dir,
    source_name: &str,
    destination_name: &str,
    replace: bool,
) -> io::Result<()> {
    if replace {
        directory.rename(source_name, directory, destination_name)
    } else {
        rustix::fs::renameat_with(
            directory,
            source_name,
            directory,
            destination_name,
            rustix::fs::RenameFlags::NOREPLACE,
        )
        .map_err(Into::into)
    }
}

fn verify_retained_named_regular_file(
    directory: &Dir,
    retained: &File,
    source_name: &str,
    expected: UniqueRegularFileIdentity,
) -> io::Result<()> {
    let retained_metadata = retained.metadata()?;
    let addressed_metadata = directory.symlink_metadata(source_name)?;
    if expected.matches_retained_regular_file(&retained_metadata, false)
        && expected.matches_retained_regular_file(&addressed_metadata, false)
    {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "retained storage source changed",
        ))
    }
}

#[cfg(unix)]
pub fn rename_retained_file_in_directory(
    directory: &Dir,
    retained: &File,
    source_name: &str,
    expected: UniqueRegularFileIdentity,
    destination_name: &str,
    replace: bool,
) -> io::Result<()> {
    verify_retained_named_regular_file(directory, retained, source_name, expected)?;
    // POSIX has no portable rename-by-handle operation. Verify that the retained
    // file and the directory-relative name still identify the staged inode, then
    // perform the rename immediately through the retained directory capability.
    rename_in_directory(directory, source_name, destination_name, replace)
}

#[cfg(windows)]
pub fn rename_retained_file_in_directory(
    directory: &Dir,
    retained: &File,
    source_name: &str,
    expected: UniqueRegularFileIdentity,
    destination_name: &str,
    replace: bool,
) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;

    use windows_sys::Win32::Storage::FileSystem::{
        FileRenameInfo, SetFileInformationByHandle, FILE_RENAME_INFO,
    };

    verify_retained_named_regular_file(directory, retained, source_name, expected)?;
    let destination_name = destination_name.encode_utf16().collect::<Vec<_>>();
    if destination_name.is_empty() || destination_name.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "storage destination name is invalid",
        ));
    }
    let destination_name_bytes = destination_name
        .len()
        .checked_mul(std::mem::size_of::<u16>())
        .and_then(|length| u32::try_from(length).ok())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "storage destination name is too long",
            )
        })?;
    let file_name_offset = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
    let buffer_bytes = std::mem::size_of::<FILE_RENAME_INFO>()
        .checked_add(destination_name_bytes as usize)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "storage destination name is too long",
            )
        })?;
    let buffer_words = buffer_bytes.div_ceil(std::mem::size_of::<usize>());
    let mut buffer = vec![0usize; buffer_words];
    let rename_info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();

    // SAFETY: The aligned buffer contains a complete FILE_RENAME_INFO and UTF-16
    // name. Publication operates on the retained staged handle, so a later
    // source-name swap cannot redirect it to another file.
    let renamed = unsafe {
        (*rename_info).Anonymous.ReplaceIfExists = replace;
        (*rename_info).RootDirectory = directory.as_raw_handle();
        (*rename_info).FileNameLength = destination_name_bytes;
        std::ptr::copy_nonoverlapping(
            destination_name.as_ptr(),
            buffer
                .as_mut_ptr()
                .cast::<u8>()
                .add(file_name_offset)
                .cast::<u16>(),
            destination_name.len(),
        );
        SetFileInformationByHandle(
            retained.as_raw_handle(),
            FileRenameInfo,
            rename_info.cast(),
            u32::try_from(buffer_bytes).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "storage rename buffer is too large",
                )
            })?,
        )
    };
    if renamed == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(any(unix, windows)))]
pub fn rename_retained_file_in_directory(
    directory: &Dir,
    retained: &File,
    source_name: &str,
    expected: UniqueRegularFileIdentity,
    destination_name: &str,
    replace: bool,
) -> io::Result<()> {
    verify_retained_named_regular_file(directory, retained, source_name, expected)?;
    rename_in_directory(directory, source_name, destination_name, replace)
}

#[cfg(windows)]
pub fn rename_in_directory(
    directory: &Dir,
    source_name: &str,
    destination_name: &str,
    replace: bool,
) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;

    use windows_sys::Win32::Storage::FileSystem::{
        FileRenameInfo, SetFileInformationByHandle, DELETE, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_RENAME_INFO,
    };

    let destination_name = destination_name.encode_utf16().collect::<Vec<_>>();
    if destination_name.is_empty() || destination_name.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "storage destination name is invalid",
        ));
    }
    let destination_name_bytes = destination_name
        .len()
        .checked_mul(std::mem::size_of::<u16>())
        .and_then(|length| u32::try_from(length).ok())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "storage destination name is too long",
            )
        })?;

    let mut options = OpenOptions::new();
    options
        .access_mode(DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .follow(FollowSymlinks::No);
    let source = directory.open_with(source_name, &options)?;

    let file_name_offset = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
    let buffer_bytes = std::mem::size_of::<FILE_RENAME_INFO>()
        .checked_add(destination_name_bytes as usize)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "storage destination name is too long",
            )
        })?;
    let buffer_words = buffer_bytes.div_ceil(std::mem::size_of::<usize>());
    let mut buffer = vec![0usize; buffer_words];
    let rename_info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();

    // SAFETY: The buffer is pointer-aligned and large enough for FILE_RENAME_INFO
    // plus the UTF-16 name. Source and retained directory handles stay valid for
    // the call, so a concurrent ambient-path swap cannot redirect publication.
    let renamed = unsafe {
        (*rename_info).Anonymous.ReplaceIfExists = replace;
        (*rename_info).RootDirectory = directory.as_raw_handle();
        (*rename_info).FileNameLength = destination_name_bytes;
        std::ptr::copy_nonoverlapping(
            destination_name.as_ptr(),
            buffer
                .as_mut_ptr()
                .cast::<u8>()
                .add(file_name_offset)
                .cast::<u16>(),
            destination_name.len(),
        );
        SetFileInformationByHandle(
            source.as_raw_handle(),
            FileRenameInfo,
            rename_info.cast(),
            u32::try_from(buffer_bytes).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "storage rename buffer is too large",
                )
            })?,
        )
    };
    if renamed == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(any(unix, windows)))]
pub fn rename_in_directory(
    directory: &Dir,
    source_name: &str,
    destination_name: &str,
    replace: bool,
) -> io::Result<()> {
    if replace {
        directory.rename(source_name, directory, destination_name)
    } else {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "atomic no-overwrite rename is unsupported",
        ))
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct UniqueRegularFileIdentity {
    device: u64,
    inode: u64,
    length: u64,
}

impl UniqueRegularFileIdentity {
    pub fn revision_parts(self) -> (u64, u64, u64) {
        (self.device, self.inode, self.length)
    }

    pub fn matches_retained_regular_file(self, metadata: &Metadata, allow_unlinked: bool) -> bool {
        let link_count_is_safe = metadata.nlink() == 1 || (allow_unlinked && metadata.nlink() == 0);
        metadata.is_file()
            && link_count_is_safe
            && metadata.dev() == self.device
            && metadata.ino() == self.inode
            && metadata.len() == self.length
    }
}

pub fn unique_regular_file_identity(metadata: &Metadata) -> Option<UniqueRegularFileIdentity> {
    if !metadata.is_file() || metadata.nlink() != 1 {
        return None;
    }
    Some(UniqueRegularFileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
        length: metadata.len(),
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use cap_fs_ext::MetadataExt;
    use cap_std::fs::{Dir, OpenOptions};
    use tempfile::tempdir;

    use super::ambient_symlink_metadata;

    #[test]
    fn ambient_metadata_has_the_same_identity_as_a_retained_file() {
        let temporary = tempdir().expect("temporary directory");
        let path = temporary.path().join("note.md");
        fs::write(&path, b"QingYu").expect("write fixture");

        let addressed = ambient_symlink_metadata(&path).expect("addressed metadata");
        let parent = Dir::open_ambient_dir(temporary.path(), cap_std::ambient_authority())
            .expect("open temporary directory");
        let mut options = OpenOptions::new();
        options.read(true);
        let retained = parent
            .open_with("note.md", &options)
            .expect("open retained file")
            .metadata()
            .expect("retained metadata");

        assert_eq!(
            (addressed.dev(), addressed.ino()),
            (retained.dev(), retained.ino())
        );
    }
}
