#[cfg(windows)]
const WINDOWS_REPARSE_POINT_ATTRIBUTE: u32 =
    windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
#[cfg(all(not(windows), test))]
const WINDOWS_REPARSE_POINT_ATTRIBUTE: u32 = 0x400;

#[cfg(any(test, windows))]
pub(crate) const fn windows_attributes_are_reparse(attributes: u32) -> bool {
    attributes & WINDOWS_REPARSE_POINT_ATTRIBUTE != 0
}

pub(crate) const fn immutable_destination_is_safe(is_file: bool, is_reparse: bool) -> bool {
    is_file && !is_reparse
}

pub(crate) fn cap_metadata_is_reparse(metadata: &cap_std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use cap_std::fs::MetadataExt;

        windows_attributes_are_reparse(metadata.file_attributes())
    }
    #[cfg(not(windows))]
    {
        let _ = metadata;
        false
    }
}

pub(crate) fn std_metadata_is_reparse(metadata: &std::fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;

        windows_attributes_are_reparse(metadata.file_attributes())
    }
    #[cfg(not(windows))]
    {
        false
    }
}

pub(crate) fn cap_metadata_is_safe_immutable_destination(metadata: &cap_std::fs::Metadata) -> bool {
    immutable_destination_is_safe(
        metadata.file_type().is_file(),
        cap_metadata_is_reparse(metadata),
    )
}

pub(crate) fn std_metadata_is_safe_immutable_destination(metadata: &std::fs::Metadata) -> bool {
    immutable_destination_is_safe(
        metadata.file_type().is_file(),
        std_metadata_is_reparse(metadata),
    )
}

#[cfg(any(test, windows))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DirectoryComponentStatus {
    ExistingDirectory { reparse: bool },
    Missing,
    NonDirectory,
}

#[cfg(any(test, windows))]
pub(crate) fn audit_absolute_directory_components_with<F>(
    path: &std::path::Path,
    mut inspect: F,
) -> Result<(), crate::RepoError>
where
    F: FnMut(&std::path::Path) -> Result<DirectoryComponentStatus, crate::RepoError>,
{
    use std::path::{Component, PathBuf};

    if !path.is_absolute() {
        return Err(crate::RepoError::UnsafePath);
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) => {
                if !current.as_os_str().is_empty() {
                    return Err(crate::RepoError::UnsafePath);
                }
                current.push(component.as_os_str());
                continue;
            }
            Component::RootDir | Component::Normal(_) => current.push(component.as_os_str()),
            Component::CurDir => continue,
            Component::ParentDir => return Err(crate::RepoError::UnsafePath),
        }

        match inspect(&current)? {
            DirectoryComponentStatus::ExistingDirectory { reparse: false } => {}
            DirectoryComponentStatus::Missing => return Ok(()),
            DirectoryComponentStatus::ExistingDirectory { reparse: true }
            | DirectoryComponentStatus::NonDirectory => return Err(crate::RepoError::UnsafePath),
        }
    }
    Ok(())
}

pub(crate) fn validate_windows_directory_components_before_canonicalize(
    path: &std::path::Path,
) -> Result<(), crate::RepoError> {
    #[cfg(windows)]
    {
        audit_absolute_directory_components_with(path, |component| match std::fs::symlink_metadata(
            component,
        ) {
            Ok(metadata) if !metadata.file_type().is_dir() => {
                Ok(DirectoryComponentStatus::NonDirectory)
            }
            Ok(metadata) => Ok(DirectoryComponentStatus::ExistingDirectory {
                reparse: std_metadata_is_reparse(&metadata),
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(DirectoryComponentStatus::Missing)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotADirectory => {
                Ok(DirectoryComponentStatus::NonDirectory)
            }
            Err(error) => Err(crate::RepoError::Io(error)),
        })
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};

    use super::{
        audit_absolute_directory_components_with, immutable_destination_is_safe,
        windows_attributes_are_reparse, DirectoryComponentStatus,
    };
    use crate::RepoError;

    #[test]
    fn windows_reparse_attribute_detection_is_independent_of_reparse_kind() {
        assert!(windows_attributes_are_reparse(0x400));
        assert!(windows_attributes_are_reparse(0x402));
        assert!(!windows_attributes_are_reparse(0));
        assert!(!windows_attributes_are_reparse(0x2));
    }

    #[test]
    fn immutable_destination_rejects_a_regular_typed_reparse_object() {
        assert!(immutable_destination_is_safe(true, false));
        assert!(!immutable_destination_is_safe(true, true));
        assert!(!immutable_destination_is_safe(false, false));
    }

    #[test]
    fn pre_canonicalize_audit_rejects_an_earlier_reparse_component() {
        let visited = RefCell::new(Vec::new());

        let result = audit_absolute_directory_components_with(
            Path::new("/safe/junction/root"),
            |component| {
                visited.borrow_mut().push(component.to_path_buf());
                Ok(if component == Path::new("/safe/junction") {
                    DirectoryComponentStatus::ExistingDirectory { reparse: true }
                } else {
                    DirectoryComponentStatus::ExistingDirectory { reparse: false }
                })
            },
        );

        assert!(matches!(result, Err(RepoError::UnsafePath)));
        assert_eq!(
            visited.into_inner(),
            [
                PathBuf::from("/"),
                PathBuf::from("/safe"),
                PathBuf::from("/safe/junction")
            ]
        );
    }

    #[test]
    fn pre_canonicalize_audit_stops_at_missing_and_rejects_non_directory_components() {
        let missing_result = audit_absolute_directory_components_with(
            Path::new("/safe/missing/root"),
            |component| {
                Ok(if component == Path::new("/safe/missing") {
                    DirectoryComponentStatus::Missing
                } else {
                    DirectoryComponentStatus::ExistingDirectory { reparse: false }
                })
            },
        );
        assert!(missing_result.is_ok());

        let non_directory_result =
            audit_absolute_directory_components_with(Path::new("/safe/file/root"), |component| {
                Ok(if component == Path::new("/safe/file") {
                    DirectoryComponentStatus::NonDirectory
                } else {
                    DirectoryComponentStatus::ExistingDirectory { reparse: false }
                })
            });
        assert!(matches!(non_directory_result, Err(RepoError::UnsafePath)));
    }
}
