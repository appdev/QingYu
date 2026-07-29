use std::{fmt, fs::File, io};

#[cfg(any(unix, windows))]
use cap_fs_ext::OpenOptionsExt;
use cap_fs_ext::{FollowSymlinks, MetadataExt, OpenOptionsFollowExt};
#[cfg(windows)]
use cap_std::fs::MetadataExt as WindowsMetadataExt;
use cap_std::fs::{Dir, OpenOptions};
use fs2::FileExt as _;

use crate::paths::{open_or_create_child, InstanceDataRoot, KernelPaths, WorkspaceRoot};

pub struct RuntimeLockLease {
    _workspace: WorkspaceLockLease,
    _instance: InstanceLockLease,
}

impl RuntimeLockLease {
    pub fn acquire(paths: &KernelPaths) -> Result<Self, KernelLockError> {
        let instance = InstanceLockLease::acquire(paths.instance_data_root())?;
        let workspace = WorkspaceLockLease::acquire(paths.workspace_root())?;
        Ok(Self {
            _workspace: workspace,
            _instance: instance,
        })
    }
}

impl fmt::Debug for RuntimeLockLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RuntimeLockLease { instance: held, workspace: held }")
    }
}

pub struct InstanceLockLease {
    lock: InstanceLock,
}

impl InstanceLockLease {
    pub(crate) fn acquire(root: &InstanceDataRoot) -> Result<Self, KernelLockError> {
        Ok(Self {
            lock: InstanceLock::acquire(root)?,
        })
    }

    pub(crate) fn verify_held_lock(&self) -> Result<(), KernelLockError> {
        self.lock.verify_held_lock()
    }
}

impl Drop for InstanceLockLease {
    fn drop(&mut self) {
        let _ = self.lock._file.unlock();
    }
}

impl fmt::Debug for InstanceLockLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("InstanceLockLease { held: true }")
    }
}

pub struct WorkspaceLockLease {
    lock: WorkspaceLock,
}

impl WorkspaceLockLease {
    pub(crate) fn acquire(root: &WorkspaceRoot) -> Result<Self, KernelLockError> {
        Ok(Self {
            lock: WorkspaceLock::acquire(root)?,
        })
    }

    pub(crate) fn verify_held_lock(&self) -> Result<(), KernelLockError> {
        self.lock.verify_held_lock()
    }
}

impl Drop for WorkspaceLockLease {
    fn drop(&mut self) {
        let _ = self.lock._file.unlock();
    }
}

impl fmt::Debug for WorkspaceLockLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("WorkspaceLockLease { held: true }")
    }
}

pub struct InstanceLock {
    _file: File,
    directory: Dir,
    identity: RegularLockIdentity,
}

impl InstanceLock {
    fn acquire(root: &InstanceDataRoot) -> Result<Self, KernelLockError> {
        let directory = root
            .try_clone_dir()
            .map_err(|_| KernelLockError::instance_unavailable())?;
        let (file, identity) = acquire_lock_file(
            &directory,
            "kernel.lock",
            KernelLockErrorKind::InstanceLocked,
            KernelLockErrorKind::InstanceStateUnavailable,
        )?;
        Ok(Self {
            _file: file,
            directory,
            identity,
        })
    }

    fn verify_held_lock(&self) -> Result<(), KernelLockError> {
        validate_addressed_identity(&self.directory, "kernel.lock", self.identity)
    }
}

impl fmt::Debug for InstanceLock {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("InstanceLock { held: true }")
    }
}

pub struct WorkspaceLock {
    _file: File,
    directory: Dir,
    identity: RegularLockIdentity,
}

impl WorkspaceLock {
    fn acquire(root: &WorkspaceRoot) -> Result<Self, KernelLockError> {
        let workspace = root
            .try_clone_dir()
            .map_err(|_| KernelLockError::workspace_unavailable())?;
        let control = open_or_create_child(&workspace, ".qingyu")
            .map_err(|_| KernelLockError::workspace_unavailable())?;
        let (file, identity) = acquire_lock_file(
            &control,
            "workspace.lock",
            KernelLockErrorKind::WorkspaceLocked,
            KernelLockErrorKind::WorkspaceUnavailable,
        )?;
        Ok(Self {
            _file: file,
            directory: control,
            identity,
        })
    }

    fn verify_held_lock(&self) -> Result<(), KernelLockError> {
        validate_addressed_identity(&self.directory, "workspace.lock", self.identity)
    }
}

impl fmt::Debug for WorkspaceLock {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("WorkspaceLock { held: true }")
    }
}

fn acquire_lock_file(
    directory: &Dir,
    name: &str,
    contention_kind: KernelLockErrorKind,
    unavailable_kind: KernelLockErrorKind,
) -> Result<(File, RegularLockIdentity), KernelLockError> {
    validate_addressed_lock_if_present(directory, name)?;

    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create(true)
        .follow(FollowSymlinks::No);
    #[cfg(unix)]
    options.mode(0o600);
    #[cfg(windows)]
    options.share_mode(
        windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ
            | windows_sys::Win32::Storage::FileSystem::FILE_SHARE_WRITE,
    );

    let file = directory
        .open_with(name, &options)
        .map_err(|_| KernelLockError::new(unavailable_kind))?;
    let retained = regular_lock_identity(
        &file
            .metadata()
            .map_err(|_| KernelLockError::new(unavailable_kind))?,
    )?;
    let file = file.into_std();
    validate_addressed_identity(directory, name, retained)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;

        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|_| KernelLockError::new(unavailable_kind))?;
    }

    if let Err(error) = file.try_lock_exclusive() {
        if lock_is_contended(&error) {
            return Err(KernelLockError::new(contention_kind));
        }
        return Err(KernelLockError::new(unavailable_kind));
    }

    if validate_addressed_identity(directory, name, retained).is_err() {
        let _ = file.unlock();
        return Err(KernelLockError::unsafe_file());
    }
    Ok((file, retained))
}

fn validate_addressed_lock_if_present(directory: &Dir, name: &str) -> Result<(), KernelLockError> {
    match directory.symlink_metadata(name) {
        Ok(metadata) => {
            regular_lock_identity(&metadata)?;
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(KernelLockError::unsafe_file()),
    }
}

fn validate_addressed_identity(
    directory: &Dir,
    name: &str,
    expected: RegularLockIdentity,
) -> Result<(), KernelLockError> {
    let addressed = directory
        .symlink_metadata(name)
        .map_err(|_| KernelLockError::unsafe_file())?;
    let addressed = regular_lock_identity(&addressed)?;
    if addressed != expected {
        return Err(KernelLockError::unsafe_file());
    }
    Ok(())
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct RegularLockIdentity {
    device: u64,
    inode: u64,
}

fn regular_lock_identity(
    metadata: &impl LockMetadata,
) -> Result<RegularLockIdentity, KernelLockError> {
    if !metadata.is_file()
        || metadata.is_symlink()
        || metadata.is_reparse_point()
        || metadata.nlink() != 1
        || metadata.len() != 0
    {
        return Err(KernelLockError::unsafe_file());
    }
    Ok(RegularLockIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

trait LockMetadata {
    fn is_file(&self) -> bool;
    fn is_symlink(&self) -> bool;
    fn is_reparse_point(&self) -> bool;
    fn len(&self) -> u64;
    fn dev(&self) -> u64;
    fn ino(&self) -> u64;
    fn nlink(&self) -> u64;
}

impl LockMetadata for cap_std::fs::Metadata {
    fn is_file(&self) -> bool {
        self.is_file()
    }

    fn is_symlink(&self) -> bool {
        self.file_type().is_symlink()
    }

    fn is_reparse_point(&self) -> bool {
        #[cfg(windows)]
        {
            WindowsMetadataExt::file_attributes(self)
                & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
                != 0
        }
        #[cfg(not(windows))]
        {
            false
        }
    }

    fn len(&self) -> u64 {
        self.len()
    }

    fn dev(&self) -> u64 {
        MetadataExt::dev(self)
    }

    fn ino(&self) -> u64 {
        MetadataExt::ino(self)
    }

    fn nlink(&self) -> u64 {
        MetadataExt::nlink(self)
    }
}

fn lock_is_contended(error: &io::Error) -> bool {
    let expected = fs2::lock_contended_error();
    same_os_error(error, &expected)
}

fn same_os_error(actual: &io::Error, expected: &io::Error) -> bool {
    match (actual.raw_os_error(), expected.raw_os_error()) {
        (Some(actual), Some(expected)) => actual == expected,
        (None, None) => actual.kind() == expected.kind(),
        _ => false,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KernelLockErrorKind {
    InstanceLocked,
    WorkspaceLocked,
    InstanceStateUnavailable,
    WorkspaceUnavailable,
    UnsafeLockFile,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct KernelLockError {
    kind: KernelLockErrorKind,
}

impl KernelLockError {
    pub const fn kind(self) -> KernelLockErrorKind {
        self.kind
    }

    const fn new(kind: KernelLockErrorKind) -> Self {
        Self { kind }
    }

    const fn instance_unavailable() -> Self {
        Self::new(KernelLockErrorKind::InstanceStateUnavailable)
    }

    const fn workspace_unavailable() -> Self {
        Self::new(KernelLockErrorKind::WorkspaceUnavailable)
    }

    const fn unsafe_file() -> Self {
        Self::new(KernelLockErrorKind::UnsafeLockFile)
    }
}

impl fmt::Debug for KernelLockError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("KernelLockError")
            .field("kind", &self.kind)
            .finish()
    }
}

impl fmt::Display for KernelLockError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self.kind {
            KernelLockErrorKind::InstanceLocked => "the Kernel instance is already running",
            KernelLockErrorKind::WorkspaceLocked => "the workspace is already in use",
            KernelLockErrorKind::InstanceStateUnavailable => {
                "the Kernel instance state is unavailable"
            }
            KernelLockErrorKind::WorkspaceUnavailable => "the workspace is unavailable",
            KernelLockErrorKind::UnsafeLockFile => "a Kernel lock file is unsafe",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for KernelLockError {}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::io;

    #[cfg(unix)]
    use super::same_os_error;

    #[cfg(unix)]
    #[test]
    fn contention_matching_does_not_fall_back_to_error_kind_when_raw_codes_differ() {
        let permission_denied = io::Error::from_raw_os_error(1);
        let access_denied = io::Error::from_raw_os_error(13);

        assert_eq!(permission_denied.kind(), access_denied.kind());
        assert!(!same_os_error(&permission_denied, &access_denied));
    }

    #[cfg(unix)]
    #[test]
    fn contention_matching_does_not_fall_back_when_only_one_error_has_a_raw_code() {
        let raw_permission_denied = io::Error::from_raw_os_error(13);
        let kind_only_permission_denied = io::Error::from(io::ErrorKind::PermissionDenied);

        assert_eq!(
            raw_permission_denied.kind(),
            kind_only_permission_denied.kind()
        );
        assert_eq!(kind_only_permission_denied.raw_os_error(), None);
        assert!(!same_os_error(
            &raw_permission_denied,
            &kind_only_permission_denied
        ));
    }
}
