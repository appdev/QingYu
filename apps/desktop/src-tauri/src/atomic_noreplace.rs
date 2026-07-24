use std::{ffi::OsStr, io, path::Component, path::Path};

use cap_std::fs::Dir;

pub(crate) fn rename_noreplace(
    source: &Dir,
    source_name: &Path,
    destination: &Dir,
    destination_name: &Path,
) -> io::Result<()> {
    let source_name = single_component(source_name)?;
    let destination_name = single_component(destination_name)?;
    rename_noreplace_platform(source, source_name, destination, destination_name)
}

fn single_component(path: &Path) -> io::Result<&OsStr> {
    let mut components = path.components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(name)), None) if !name.is_empty() => Ok(name),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "atomic no-replace rename names must be one path component",
        )),
    }
}

#[cfg(unix)]
fn rename_noreplace_platform(
    source: &Dir,
    source_name: &OsStr,
    destination: &Dir,
    destination_name: &OsStr,
) -> io::Result<()> {
    rustix::fs::renameat_with(
        source,
        source_name,
        destination,
        destination_name,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(Into::into)
}

#[cfg(windows)]
fn rename_noreplace_platform(
    source: &Dir,
    source_name: &OsStr,
    destination: &Dir,
    destination_name: &OsStr,
) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::AsRawHandle;

    use cap_fs_ext::{FollowSymlinks, OpenOptionsExt, OpenOptionsFollowExt};
    use windows_sys::Win32::Storage::FileSystem::{
        FileRenameInfo, SetFileInformationByHandle, DELETE, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_RENAME_INFO,
    };

    let destination_name = destination_name.encode_wide().collect::<Vec<_>>();
    if destination_name.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "atomic no-replace destination name contains a null character",
        ));
    }
    let destination_name_bytes = destination_name
        .len()
        .checked_mul(std::mem::size_of::<u16>())
        .and_then(|length| u32::try_from(length).ok())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "atomic no-replace destination name is too long",
            )
        })?;

    let mut options = cap_std::fs::OpenOptions::new();
    options
        .access_mode(DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .follow(FollowSymlinks::No);
    let source_file = source.open_with(Path::new(source_name), &options)?;

    let file_name_offset = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
    let buffer_bytes = std::mem::size_of::<FILE_RENAME_INFO>()
        .checked_add(destination_name_bytes as usize)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "atomic no-replace rename buffer is too large",
            )
        })?;
    let buffer_words = buffer_bytes.div_ceil(std::mem::size_of::<usize>());
    let mut buffer = vec![0usize; buffer_words];
    let rename_info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();

    // SAFETY: The usize buffer is pointer-aligned and has room for the fixed
    // FILE_RENAME_INFO fields plus the exact UTF-16 component bytes. Both
    // capability handles remain alive for the call. ReplaceIfExists=false
    // binds the no-clobber condition to the same operation as the rename.
    let renamed = unsafe {
        (*rename_info).Anonymous.ReplaceIfExists = false;
        (*rename_info).RootDirectory = destination.as_raw_handle();
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
            source_file.as_raw_handle(),
            FileRenameInfo,
            rename_info.cast(),
            u32::try_from(buffer_bytes).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "atomic no-replace rename buffer is too large",
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
fn rename_noreplace_platform(
    _source: &Dir,
    _source_name: &OsStr,
    _destination: &Dir,
    _destination_name: &OsStr,
) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "atomic no-replace rename is unsupported on this platform",
    ))
}

#[cfg(test)]
mod tests {
    use std::{fs, io, path::Path};

    use cap_std::fs::Dir;
    use tempfile::tempdir;

    use super::rename_noreplace;

    #[test]
    fn rename_noreplace_preserves_an_existing_destination() {
        let temp = tempdir().expect("temp directory");
        fs::write(temp.path().join("source"), b"candidate").expect("source file");
        fs::write(temp.path().join("target"), b"installed").expect("target file");
        let directory = Dir::open_ambient_dir(temp.path(), cap_std::ambient_authority())
            .expect("open temp directory");

        let error = rename_noreplace(
            &directory,
            Path::new("source"),
            &directory,
            Path::new("target"),
        )
        .expect_err("an existing target must never be replaced");

        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(temp.path().join("source")).unwrap(), b"candidate");
        assert_eq!(fs::read(temp.path().join("target")).unwrap(), b"installed");
    }

    #[test]
    fn rename_noreplace_accepts_only_single_path_components() {
        let temp = tempdir().expect("temp directory");
        fs::create_dir(temp.path().join("nested")).expect("nested directory");
        fs::write(temp.path().join("source"), b"candidate").expect("source file");
        let directory = Dir::open_ambient_dir(temp.path(), cap_std::ambient_authority())
            .expect("open temp directory");

        for (source, target) in [
            ("nested/source", "target"),
            ("source", "nested/target"),
            ("source", "../target"),
            ("source", ""),
        ] {
            let error =
                rename_noreplace(&directory, Path::new(source), &directory, Path::new(target))
                    .expect_err("non-component names must be rejected");
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        }

        assert_eq!(fs::read(temp.path().join("source")).unwrap(), b"candidate");
        assert!(!temp.path().join("target").exists());
    }
}
