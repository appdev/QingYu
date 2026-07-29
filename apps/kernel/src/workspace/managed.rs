//! Managed-workspace ownership boundary.

use std::{fmt, io, path::Path};

use cap_fs_ext::{DirExt, MetadataExt};
use cap_std::fs::Dir;

use crate::paths::KernelPaths;

pub struct ManagedWorkspaceCollection {
    instance_root: Dir,
    instance_identity: DirectoryIdentity,
    address_parent: Dir,
    address_name: std::ffi::OsString,
}

impl ManagedWorkspaceCollection {
    pub fn from_paths(paths: &KernelPaths) -> Result<Self, ManagedWorkspaceError> {
        paths
            .instance_data_root()
            .verify_held_directory()
            .map_err(|_| ManagedWorkspaceError::unavailable())?;
        let instance_root = paths
            .instance_data_root()
            .try_clone_dir()
            .map_err(|_| ManagedWorkspaceError::unavailable())?;
        let instance_identity =
            directory_identity(&instance_root).map_err(|_| ManagedWorkspaceError::unavailable())?;
        let address = paths.instance_data_root().canonical_path();
        let parent_path = address
            .parent()
            .ok_or_else(ManagedWorkspaceError::unavailable)?;
        let address_name = address
            .file_name()
            .ok_or_else(ManagedWorkspaceError::unavailable)?
            .to_os_string();
        let address_parent = Dir::open_ambient_dir(parent_path, cap_std::ambient_authority())
            .map_err(|_| ManagedWorkspaceError::unavailable())?;
        Ok(Self {
            instance_root,
            instance_identity,
            address_parent,
            address_name,
        })
    }

    pub fn create(&self, name: &str) -> Result<String, ManagedWorkspaceError> {
        validate_managed_name(name)?;
        let collection = self
            .open_collection(true)?
            .ok_or_else(ManagedWorkspaceError::unavailable)?;
        match collection.symlink_metadata(name) {
            Ok(metadata) => validate_directory_entry(&collection, name, &metadata)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if let Err(create_error) = collection.create_dir(name) {
                    if create_error.kind() != io::ErrorKind::AlreadyExists {
                        return Err(ManagedWorkspaceError::unavailable());
                    }
                }
                let metadata = collection
                    .symlink_metadata(name)
                    .map_err(|_| ManagedWorkspaceError::unavailable())?;
                validate_directory_entry(&collection, name, &metadata)?;
            }
            Err(_) => return Err(ManagedWorkspaceError::unavailable()),
        }
        Ok(name.to_string())
    }

    pub fn list(&self) -> Result<Vec<String>, ManagedWorkspaceError> {
        let Some(collection) = self.open_collection(false)? else {
            return Ok(Vec::new());
        };
        let entries = collection
            .entries()
            .map_err(|_| ManagedWorkspaceError::unavailable())?;
        let mut names = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|_| ManagedWorkspaceError::unavailable())?;
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if validate_managed_name(&name).is_err() {
                continue;
            }
            let metadata = collection
                .symlink_metadata(&name)
                .map_err(|_| ManagedWorkspaceError::unavailable())?;
            if validate_directory_entry(&collection, &name, &metadata).is_ok() {
                names.push(name);
            }
        }
        names.sort();
        Ok(names)
    }

    fn open_collection(&self, create: bool) -> Result<Option<Dir>, ManagedWorkspaceError> {
        self.verify_address()?;
        match self.instance_root.symlink_metadata("workspaces") {
            Ok(metadata) => {
                validate_directory_entry(&self.instance_root, "workspaces", &metadata)?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound && !create => return Ok(None),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if let Err(create_error) = self.instance_root.create_dir("workspaces") {
                    if create_error.kind() != io::ErrorKind::AlreadyExists {
                        return Err(ManagedWorkspaceError::unavailable());
                    }
                }
                let metadata = self
                    .instance_root
                    .symlink_metadata("workspaces")
                    .map_err(|_| ManagedWorkspaceError::unavailable())?;
                validate_directory_entry(&self.instance_root, "workspaces", &metadata)?;
            }
            Err(_) => return Err(ManagedWorkspaceError::unavailable()),
        }
        self.instance_root
            .open_dir_nofollow("workspaces")
            .map(Some)
            .map_err(|_| ManagedWorkspaceError::unsafe_entry())
    }

    fn verify_address(&self) -> Result<(), ManagedWorkspaceError> {
        let retained_identity = directory_identity(&self.instance_root)
            .map_err(|_| ManagedWorkspaceError::unavailable())?;
        let addressed = self
            .address_parent
            .open_dir_nofollow(&self.address_name)
            .map_err(|_| ManagedWorkspaceError::unsafe_entry())?;
        let addressed_identity =
            directory_identity(&addressed).map_err(|_| ManagedWorkspaceError::unsafe_entry())?;
        if retained_identity != self.instance_identity
            || addressed_identity != self.instance_identity
        {
            return Err(ManagedWorkspaceError::unsafe_entry());
        }
        Ok(())
    }
}

fn validate_directory_entry(
    parent: &Dir,
    name: impl AsRef<Path>,
    addressed: &cap_std::fs::Metadata,
) -> Result<(), ManagedWorkspaceError> {
    if addressed.file_type().is_symlink() || !addressed.is_dir() {
        return Err(ManagedWorkspaceError::unsafe_entry());
    }
    let retained = parent
        .open_dir_nofollow(name)
        .map_err(|_| ManagedWorkspaceError::unsafe_entry())?;
    let retained = retained
        .dir_metadata()
        .map_err(|_| ManagedWorkspaceError::unsafe_entry())?;
    if addressed.dev() != retained.dev() || addressed.ino() != retained.ino() {
        return Err(ManagedWorkspaceError::unsafe_entry());
    }
    Ok(())
}

fn validate_managed_name(name: &str) -> Result<(), ManagedWorkspaceError> {
    let windows_device_stem = name
        .split_once('.')
        .map_or(name, |(stem, _)| stem)
        .to_ascii_uppercase();
    let windows_device_name = matches!(windows_device_stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (windows_device_stem.len() == 4
            && (windows_device_stem.starts_with("COM") || windows_device_stem.starts_with("LPT"))
            && matches!(windows_device_stem.as_bytes()[3], b'1'..=b'9'));
    let lowercase = name.to_ascii_lowercase();
    if name.is_empty()
        || matches!(name, "." | "..")
        || name.chars().any(char::is_control)
        || name.contains(['/', '\\', ':'])
        || name.ends_with(['.', ' '])
        || name.eq_ignore_ascii_case(".qingyu")
        || lowercase.starts_with(".qingyu-ui-update-")
        || lowercase.starts_with(".qingyu-mcp-update-")
        || lowercase.starts_with(".markra-sync-stage-")
        || windows_device_name
    {
        return Err(ManagedWorkspaceError::invalid_name());
    }
    Ok(())
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct DirectoryIdentity {
    device: u64,
    inode: u64,
}

fn directory_identity(directory: &Dir) -> io::Result<DirectoryIdentity> {
    let metadata = directory.dir_metadata()?;
    if !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "not a directory",
        ));
    }
    Ok(DirectoryIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManagedWorkspaceErrorKind {
    Unavailable,
    InvalidName,
    UnsafeEntry,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct ManagedWorkspaceError {
    kind: ManagedWorkspaceErrorKind,
}

impl ManagedWorkspaceError {
    const fn unavailable() -> Self {
        Self {
            kind: ManagedWorkspaceErrorKind::Unavailable,
        }
    }

    const fn invalid_name() -> Self {
        Self {
            kind: ManagedWorkspaceErrorKind::InvalidName,
        }
    }

    const fn unsafe_entry() -> Self {
        Self {
            kind: ManagedWorkspaceErrorKind::UnsafeEntry,
        }
    }

    pub const fn kind(self) -> ManagedWorkspaceErrorKind {
        self.kind
    }
}

impl fmt::Debug for ManagedWorkspaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ManagedWorkspaceError")
            .field("kind", &self.kind)
            .finish()
    }
}

impl fmt::Display for ManagedWorkspaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("managed workspace storage is unavailable")
    }
}

impl std::error::Error for ManagedWorkspaceError {}
