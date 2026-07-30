use std::{
    ffi::OsString,
    fmt, io,
    path::{Path, PathBuf},
    sync::Arc,
};

use cap_fs_ext::{DirExt, MetadataExt};
use cap_std::fs::Dir;

use crate::contract::HostProfile;

pub struct KernelPaths {
    profile: HostProfile,
    workspace: Arc<WorkspaceRoot>,
    instance_data: Arc<InstanceDataRoot>,
    cache: CacheRoot,
    config: ConfigRoot,
    _logs: PrivateDirectory,
}

impl KernelPaths {
    pub fn desktop(
        workspace: &Path,
        app_data: &Path,
        cache: &Path,
    ) -> Result<Self, PathPolicyError> {
        let workspace = OpenedDirectory::open_existing(workspace)?;
        let app_data = OpenedDirectory::open_existing(app_data)?;
        let cache = OpenedDirectory::open_existing(cache)?;
        reject_overlapping_roots([&workspace, &app_data, &cache])?;

        let config = ConfigRoot::new(app_data.try_clone_opened()?)?;
        let logs = app_data.try_clone_private()?;
        Ok(Self {
            profile: HostProfile::Desktop,
            workspace: Arc::new(WorkspaceRoot::new_host(workspace)?),
            instance_data: Arc::new(InstanceDataRoot::new(app_data)?),
            cache: CacheRoot::new(cache),
            config,
            _logs: logs,
        })
    }

    pub const fn server() -> ServerPathLayout {
        ServerPathLayout {
            source: ServerLayoutSource::Production,
        }
    }

    pub fn mobile(
        app_data: &Path,
        cache: &Path,
        managed_name: &str,
    ) -> Result<Self, PathPolicyError> {
        validate_managed_name(managed_name)?;
        let app_data = OpenedDirectory::open_existing(app_data)?;
        let cache = OpenedDirectory::open_existing(cache)?;
        reject_overlapping_roots([&app_data, &cache])?;

        let workspace_parent = app_data
            .dir
            .try_clone()
            .map_err(|_| PathPolicyError::unavailable())?;
        let collection = open_or_create_child(&app_data.dir, "workspaces")?;
        let collection_identity =
            directory_identity(&collection).map_err(|_| PathPolicyError::unavailable())?;
        let workspace_dir = open_or_create_child(&collection, managed_name)?;
        let workspace_path = app_data
            .canonical_path
            .join("workspaces")
            .join(managed_name);
        let workspace = OpenedDirectory::from_open_dir(workspace_dir, workspace_path)?;

        let config = ConfigRoot::new(app_data.try_clone_opened()?)?;
        let logs = app_data.try_clone_private()?;
        Ok(Self {
            profile: HostProfile::Mobile,
            workspace: Arc::new(WorkspaceRoot::new_managed(
                workspace,
                workspace_parent,
                collection_identity,
                managed_name,
            )),
            instance_data: Arc::new(InstanceDataRoot::new(app_data)?),
            cache: CacheRoot::new(cache),
            config,
            _logs: logs,
        })
    }

    pub const fn profile(&self) -> HostProfile {
        self.profile
    }

    pub fn workspace_root(&self) -> &WorkspaceRoot {
        self.workspace.as_ref()
    }

    pub(crate) fn workspace_root_authority(&self) -> Arc<WorkspaceRoot> {
        self.workspace.clone()
    }

    pub fn instance_data_root(&self) -> &InstanceDataRoot {
        self.instance_data.as_ref()
    }

    pub(crate) fn instance_data_root_authority(&self) -> Arc<InstanceDataRoot> {
        self.instance_data.clone()
    }

    pub const fn cache_root(&self) -> &CacheRoot {
        &self.cache
    }

    pub const fn config_root(&self) -> &ConfigRoot {
        &self.config
    }

    pub(crate) fn prepare_host_workspace_root(
        &self,
        path: &Path,
    ) -> Result<Arc<WorkspaceRoot>, PathPolicyError> {
        let candidate = OpenedDirectory::open_existing(path)?;
        self.validate_host_workspace_candidate(&candidate)?;
        let candidate = Arc::new(WorkspaceRoot::new_host(candidate)?);
        self.validate_host_workspace_root(candidate.as_ref())?;
        Ok(candidate)
    }

    pub(crate) fn validate_host_workspace_root(
        &self,
        candidate: &WorkspaceRoot,
    ) -> Result<(), PathPolicyError> {
        self.instance_data.verify_held_directory()?;
        self.cache.verify_held_directory()?;
        candidate.verify_held_directory()?;
        if roots_overlap(
            candidate.identity,
            candidate.canonical_path(),
            self.instance_data.identity,
            self.instance_data.canonical_path(),
        ) || roots_overlap(
            candidate.identity,
            candidate.canonical_path(),
            self.cache.identity,
            self.cache.canonical_path(),
        ) {
            return Err(PathPolicyError::overlapping_roots());
        }
        Ok(())
    }

    fn validate_host_workspace_candidate(
        &self,
        candidate: &OpenedDirectory,
    ) -> Result<(), PathPolicyError> {
        if roots_overlap(
            candidate.identity,
            &candidate.canonical_path,
            self.instance_data.identity,
            self.instance_data.canonical_path(),
        ) || roots_overlap(
            candidate.identity,
            &candidate.canonical_path,
            self.cache.identity,
            self.cache.canonical_path(),
        ) {
            return Err(PathPolicyError::overlapping_roots());
        }
        Ok(())
    }
}

impl fmt::Debug for KernelPaths {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("KernelPaths")
            .field("profile", &self.profile)
            .field("workspace", &"WorkspaceRoot(..)")
            .field("instance_data", &"InstanceDataRoot(..)")
            .field("cache", &"CacheRoot(..)")
            .field("config", &"ConfigRoot(..)")
            .finish()
    }
}

/// Retained workspace authority. Its absolute address is intentionally not observable.
///
/// ```compile_fail
/// use qingyu_kernel::paths::WorkspaceRoot;
/// fn expose(root: WorkspaceRoot) {
///     let WorkspaceRoot { dir, .. } = root;
///     drop(dir);
/// }
/// ```
pub struct WorkspaceRoot {
    dir: Dir,
    identity: DirectoryIdentity,
    canonical_path: PathBuf,
    address_anchor: Option<WorkspaceAddressAnchor>,
}

impl WorkspaceRoot {
    fn new_host(opened: OpenedDirectory) -> Result<Self, PathPolicyError> {
        let address_anchor = ExactDirectoryAddress::new(&opened.canonical_path)?;
        Ok(Self {
            dir: opened.dir,
            identity: opened.identity,
            canonical_path: opened.canonical_path,
            address_anchor: Some(WorkspaceAddressAnchor::Exact(address_anchor)),
        })
    }

    fn new_managed(
        opened: OpenedDirectory,
        collection_parent: Dir,
        collection_identity: DirectoryIdentity,
        workspace_name: &str,
    ) -> Self {
        Self {
            dir: opened.dir,
            identity: opened.identity,
            canonical_path: opened.canonical_path,
            address_anchor: Some(WorkspaceAddressAnchor::Managed {
                collection_parent,
                collection_identity,
                workspace_name: workspace_name.to_string(),
            }),
        }
    }

    pub fn verify_held_directory(&self) -> Result<(), PathPolicyError> {
        verify_retained_directory(&self.dir, self.identity)?;
        match &self.address_anchor {
            Some(WorkspaceAddressAnchor::Exact(anchor)) => {
                anchor.verify(self.identity)?;
            }
            Some(WorkspaceAddressAnchor::Managed {
                collection_parent,
                collection_identity,
                workspace_name,
            }) => {
                let collection = collection_parent
                    .open_dir_nofollow("workspaces")
                    .map_err(|_| PathPolicyError::unsafe_entry())?;
                let actual_collection_identity =
                    directory_identity(&collection).map_err(|_| PathPolicyError::unsafe_entry())?;
                if actual_collection_identity != *collection_identity {
                    return Err(PathPolicyError::unsafe_entry());
                }
                let addressed = collection
                    .open_dir_nofollow(workspace_name)
                    .map_err(|_| PathPolicyError::unsafe_entry())?;
                let addressed_identity =
                    directory_identity(&addressed).map_err(|_| PathPolicyError::unsafe_entry())?;
                if addressed_identity != self.identity {
                    return Err(PathPolicyError::unsafe_entry());
                }
            }
            None => {}
        }
        Ok(())
    }

    pub(crate) fn try_clone_dir(&self) -> Result<Dir, PathPolicyError> {
        self.verify_held_directory()?;
        self.dir
            .try_clone()
            .map_err(|_| PathPolicyError::unavailable())
    }

    pub(crate) fn same_identity(&self, other: &Self) -> bool {
        self.identity == other.identity
    }

    pub(crate) fn canonical_path(&self) -> &Path {
        &self.canonical_path
    }
}

enum WorkspaceAddressAnchor {
    Exact(ExactDirectoryAddress),
    Managed {
        collection_parent: Dir,
        collection_identity: DirectoryIdentity,
        workspace_name: String,
    },
}

struct ExactDirectoryAddress {
    parent: Dir,
    parent_identity: DirectoryIdentity,
    name: OsString,
}

impl ExactDirectoryAddress {
    fn new(path: &Path) -> Result<Self, PathPolicyError> {
        let parent_path = path.parent().ok_or_else(PathPolicyError::unavailable)?;
        let name = path.file_name().ok_or_else(PathPolicyError::unsafe_entry)?;
        let parent = open_canonical_directory_nofollow(parent_path)
            .map_err(|_| PathPolicyError::unavailable())?;
        let parent_identity =
            directory_identity(&parent).map_err(|_| PathPolicyError::unavailable())?;
        Ok(Self {
            parent,
            parent_identity,
            name: name.to_os_string(),
        })
    }

    fn verify(&self, expected: DirectoryIdentity) -> Result<(), PathPolicyError> {
        let parent_identity =
            directory_identity(&self.parent).map_err(|_| PathPolicyError::unsafe_entry())?;
        if parent_identity != self.parent_identity {
            return Err(PathPolicyError::unsafe_entry());
        }
        let addressed = self
            .parent
            .open_dir_nofollow(&self.name)
            .map_err(|_| PathPolicyError::unsafe_entry())?;
        let addressed_identity =
            directory_identity(&addressed).map_err(|_| PathPolicyError::unsafe_entry())?;
        if addressed_identity != expected {
            return Err(PathPolicyError::unsafe_entry());
        }
        Ok(())
    }

    fn try_clone(&self) -> Result<Self, PathPolicyError> {
        Ok(Self {
            parent: self
                .parent
                .try_clone()
                .map_err(|_| PathPolicyError::unavailable())?,
            parent_identity: self.parent_identity,
            name: self.name.clone(),
        })
    }
}

impl fmt::Debug for WorkspaceRoot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("WorkspaceRoot(..)")
    }
}

/// Retained instance-data authority. Its absolute address is intentionally not observable.
///
/// ```compile_fail
/// use qingyu_kernel::paths::InstanceDataRoot;
/// fn expose(root: InstanceDataRoot) {
///     let InstanceDataRoot { dir, .. } = root;
///     drop(dir);
/// }
/// ```
pub struct InstanceDataRoot {
    dir: Dir,
    identity: DirectoryIdentity,
    canonical_path: PathBuf,
    address_anchor: ExactDirectoryAddress,
}

impl InstanceDataRoot {
    fn new(opened: OpenedDirectory) -> Result<Self, PathPolicyError> {
        let address_anchor = ExactDirectoryAddress::new(&opened.canonical_path)?;
        Ok(Self {
            dir: opened.dir,
            identity: opened.identity,
            canonical_path: opened.canonical_path,
            address_anchor,
        })
    }

    pub fn verify_held_directory(&self) -> Result<(), PathPolicyError> {
        verify_retained_directory(&self.dir, self.identity)?;
        self.address_anchor.verify(self.identity)
    }

    pub(crate) fn try_clone_dir(&self) -> Result<Dir, PathPolicyError> {
        self.dir
            .try_clone()
            .map_err(|_| PathPolicyError::unavailable())
    }

    pub(crate) fn canonical_path(&self) -> &Path {
        &self.canonical_path
    }
}

impl fmt::Debug for InstanceDataRoot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("InstanceDataRoot(..)")
    }
}

/// Retained authority for Kernel configuration that must never be synchronized.
///
/// ```compile_fail
/// use qingyu_kernel::paths::ConfigRoot;
/// fn expose(root: ConfigRoot) {
///     let ConfigRoot { dir, .. } = root;
///     drop(dir);
/// }
/// ```
pub struct ConfigRoot {
    dir: Dir,
    identity: DirectoryIdentity,
    canonical_path: PathBuf,
    address_anchor: ExactDirectoryAddress,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) struct ConfigRootIdentity(DirectoryIdentity);

impl ConfigRoot {
    fn new(opened: OpenedDirectory) -> Result<Self, PathPolicyError> {
        let address_anchor = ExactDirectoryAddress::new(&opened.canonical_path)?;
        Ok(Self {
            dir: opened.dir,
            identity: opened.identity,
            canonical_path: opened.canonical_path,
            address_anchor,
        })
    }

    pub fn verify_held_directory(&self) -> Result<(), PathPolicyError> {
        verify_retained_directory(&self.dir, self.identity)?;
        self.address_anchor.verify(self.identity)
    }

    pub(crate) fn try_clone_dir(&self) -> Result<Dir, PathPolicyError> {
        self.verify_held_directory()?;
        self.dir
            .try_clone()
            .map_err(|_| PathPolicyError::unavailable())
    }

    pub(crate) fn try_clone_root(&self) -> Result<Self, PathPolicyError> {
        self.verify_held_directory()?;
        Ok(Self {
            dir: self
                .dir
                .try_clone()
                .map_err(|_| PathPolicyError::unavailable())?,
            identity: self.identity,
            canonical_path: self.canonical_path.clone(),
            address_anchor: self.address_anchor.try_clone()?,
        })
    }

    pub(crate) fn canonical_path(&self) -> &Path {
        &self.canonical_path
    }

    pub(crate) const fn identity(&self) -> ConfigRootIdentity {
        ConfigRootIdentity(self.identity)
    }
}

impl fmt::Debug for ConfigRoot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ConfigRoot(..)")
    }
}

/// Retained cache authority. Its absolute address is intentionally not observable.
///
/// ```compile_fail
/// use qingyu_kernel::paths::CacheRoot;
/// fn expose(root: CacheRoot) {
///     let CacheRoot { dir, .. } = root;
///     drop(dir);
/// }
/// ```
pub struct CacheRoot {
    dir: Dir,
    identity: DirectoryIdentity,
    canonical_path: PathBuf,
}

impl CacheRoot {
    fn new(opened: OpenedDirectory) -> Self {
        Self {
            dir: opened.dir,
            identity: opened.identity,
            canonical_path: opened.canonical_path,
        }
    }

    pub fn verify_held_directory(&self) -> Result<(), PathPolicyError> {
        verify_retained_directory(&self.dir, self.identity)
    }

    fn canonical_path(&self) -> &Path {
        &self.canonical_path
    }
}

impl fmt::Debug for CacheRoot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CacheRoot(..)")
    }
}

struct PrivateDirectory {
    _dir: Dir,
    _identity: DirectoryIdentity,
}

pub struct ServerPathLayout {
    source: ServerLayoutSource,
}

enum ServerLayoutSource {
    Production,
    #[cfg(test)]
    Fixture {
        data_root: PathBuf,
        cache_root: PathBuf,
    },
}

impl ServerPathLayout {
    pub fn workspace_path(&self) -> PathBuf {
        self.data_root().join("workspace")
    }

    pub fn config_path(&self) -> PathBuf {
        self.data_root().join("config")
    }

    pub fn state_path(&self) -> PathBuf {
        self.data_root().join("state")
    }

    pub fn logs_path(&self) -> PathBuf {
        self.data_root().join("logs")
    }

    pub fn cache_path(&self) -> PathBuf {
        self.cache_root().to_path_buf()
    }

    pub fn activate(self) -> Result<KernelPaths, PathPolicyError> {
        let data_root = OpenedDirectory::open_existing(self.data_root())?;
        let workspace_path = data_root.canonical_path.join("workspace");
        let config_path = data_root.canonical_path.join("config");
        let state_path = data_root.canonical_path.join("state");
        let logs_path = data_root.canonical_path.join("logs");

        let workspace = OpenedDirectory::from_open_dir(
            open_or_create_child(&data_root.dir, "workspace")?,
            workspace_path,
        )?;
        let config = OpenedDirectory::from_open_dir(
            open_or_create_child(&data_root.dir, "config")?,
            config_path,
        )?;
        let state = OpenedDirectory::from_open_dir(
            open_or_create_child(&data_root.dir, "state")?,
            state_path,
        )?;
        let logs = OpenedDirectory::from_open_dir(
            open_or_create_child(&data_root.dir, "logs")?,
            logs_path,
        )?;
        let cache = open_or_create_ambient_directory(self.cache_root())?;

        Ok(KernelPaths {
            profile: HostProfile::Server,
            workspace: Arc::new(WorkspaceRoot::new_host(workspace)?),
            instance_data: Arc::new(InstanceDataRoot::new(state)?),
            cache: CacheRoot::new(cache),
            config: ConfigRoot::new(config)?,
            _logs: PrivateDirectory::new(logs),
        })
    }

    fn data_root(&self) -> &Path {
        match &self.source {
            ServerLayoutSource::Production => Path::new("/data"),
            #[cfg(test)]
            ServerLayoutSource::Fixture { data_root, .. } => data_root,
        }
    }

    fn cache_root(&self) -> &Path {
        match &self.source {
            ServerLayoutSource::Production => Path::new("/tmp/qingyu"),
            #[cfg(test)]
            ServerLayoutSource::Fixture { cache_root, .. } => cache_root,
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test(data_root: &Path, cache_root: &Path) -> Self {
        Self {
            source: ServerLayoutSource::Fixture {
                data_root: data_root.to_path_buf(),
                cache_root: cache_root.to_path_buf(),
            },
        }
    }
}

impl fmt::Debug for ServerPathLayout {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ServerPathLayout(..)")
    }
}

impl PrivateDirectory {
    fn new(opened: OpenedDirectory) -> Self {
        Self {
            _dir: opened.dir,
            _identity: opened.identity,
        }
    }
}

struct OpenedDirectory {
    dir: Dir,
    identity: DirectoryIdentity,
    canonical_path: PathBuf,
}

impl OpenedDirectory {
    fn open_existing(path: &Path) -> Result<Self, PathPolicyError> {
        let canonical_path = path
            .canonicalize()
            .map_err(|_| PathPolicyError::unavailable())?;
        let dir = open_canonical_directory_nofollow(&canonical_path)
            .map_err(|_| PathPolicyError::unavailable())?;
        Self::from_open_dir(dir, canonical_path)
    }

    fn from_open_dir(dir: Dir, canonical_path: PathBuf) -> Result<Self, PathPolicyError> {
        let identity = directory_identity(&dir).map_err(|_| PathPolicyError::unavailable())?;
        Ok(Self {
            dir,
            identity,
            canonical_path,
        })
    }

    fn try_clone_private(&self) -> Result<PrivateDirectory, PathPolicyError> {
        let dir = self
            .dir
            .try_clone()
            .map_err(|_| PathPolicyError::unavailable())?;
        Ok(PrivateDirectory {
            _dir: dir,
            _identity: self.identity,
        })
    }

    fn try_clone_opened(&self) -> Result<Self, PathPolicyError> {
        Ok(Self {
            dir: self
                .dir
                .try_clone()
                .map_err(|_| PathPolicyError::unavailable())?,
            identity: self.identity,
            canonical_path: self.canonical_path.clone(),
        })
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct DirectoryIdentity {
    device: u64,
    inode: u64,
}

fn directory_identity(dir: &Dir) -> io::Result<DirectoryIdentity> {
    let metadata = dir.dir_metadata()?;
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

fn verify_retained_directory(
    dir: &Dir,
    expected: DirectoryIdentity,
) -> Result<(), PathPolicyError> {
    let actual = directory_identity(dir).map_err(|_| PathPolicyError::unavailable())?;
    if actual != expected {
        return Err(PathPolicyError::unsafe_entry());
    }
    Ok(())
}

fn open_canonical_directory_nofollow(path: &Path) -> io::Result<Dir> {
    let Some(parent) = path.parent() else {
        return Dir::open_ambient_dir(path, cap_std::ambient_authority());
    };
    let Some(name) = path.file_name() else {
        return Dir::open_ambient_dir(path, cap_std::ambient_authority());
    };
    let parent = Dir::open_ambient_dir(parent, cap_std::ambient_authority())?;
    parent.open_dir_nofollow(name)
}

fn open_or_create_ambient_directory(path: &Path) -> Result<OpenedDirectory, PathPolicyError> {
    let parent_path = path.parent().ok_or_else(PathPolicyError::unavailable)?;
    let name = path.file_name().ok_or_else(PathPolicyError::unavailable)?;
    let parent = OpenedDirectory::open_existing(parent_path)?;
    let name = name.to_str().ok_or_else(PathPolicyError::unsafe_entry)?;
    let dir = open_or_create_child(&parent.dir, name)?;
    let canonical_path = parent.canonical_path.join(name);
    OpenedDirectory::from_open_dir(dir, canonical_path)
}

pub(crate) fn open_or_create_child(parent: &Dir, name: &str) -> Result<Dir, PathPolicyError> {
    let mut created = false;
    match parent.symlink_metadata(name) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(PathPolicyError::unsafe_entry());
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => match parent.create_dir(name) {
            Ok(()) => created = true,
            Err(create_error) => {
                if create_error.kind() != io::ErrorKind::AlreadyExists {
                    return Err(PathPolicyError::unavailable());
                }
            }
        },
        Err(_) => return Err(PathPolicyError::unavailable()),
    }

    let metadata = parent
        .symlink_metadata(name)
        .map_err(|_| PathPolicyError::unavailable())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(PathPolicyError::unsafe_entry());
    }
    let child = parent
        .open_dir_nofollow(name)
        .map_err(|_| PathPolicyError::unsafe_entry())?;
    if created {
        crate::storage::sync_directory(parent).map_err(|_| PathPolicyError::unavailable())?;
        record_created_child_parent_sync();
    }
    Ok(child)
}

#[cfg(test)]
thread_local! {
    static CREATED_CHILD_PARENT_SYNC_COUNT: std::cell::Cell<usize> = const {
        std::cell::Cell::new(0)
    };
}

#[cfg(test)]
fn record_created_child_parent_sync() {
    CREATED_CHILD_PARENT_SYNC_COUNT.with(|count| count.set(count.get().saturating_add(1)));
}

#[cfg(not(test))]
fn record_created_child_parent_sync() {}

#[cfg(test)]
fn reset_created_child_parent_sync_count() {
    CREATED_CHILD_PARENT_SYNC_COUNT.with(|count| count.set(0));
}

#[cfg(test)]
fn created_child_parent_sync_count() -> usize {
    CREATED_CHILD_PARENT_SYNC_COUNT.with(std::cell::Cell::get)
}

fn reject_overlapping_roots<const N: usize>(
    roots: [&OpenedDirectory; N],
) -> Result<(), PathPolicyError> {
    for (index, left) in roots.iter().enumerate() {
        for right in roots.iter().skip(index + 1) {
            if roots_overlap(
                left.identity,
                &left.canonical_path,
                right.identity,
                &right.canonical_path,
            ) {
                return Err(PathPolicyError::overlapping_roots());
            }
        }
    }
    Ok(())
}

fn roots_overlap(
    left_identity: DirectoryIdentity,
    left_path: &Path,
    right_identity: DirectoryIdentity,
    right_path: &Path,
) -> bool {
    left_identity == right_identity
        || left_path.starts_with(right_path)
        || right_path.starts_with(left_path)
}

fn validate_managed_name(name: &str) -> Result<(), PathPolicyError> {
    let windows_device_stem = name
        .split_once('.')
        .map_or(name, |(stem, _)| stem)
        .to_ascii_uppercase();
    let windows_device_name = matches!(windows_device_stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (windows_device_stem.len() == 4
            && (windows_device_stem.starts_with("COM") || windows_device_stem.starts_with("LPT"))
            && matches!(windows_device_stem.as_bytes()[3], b'1'..=b'9'));
    if name.is_empty()
        || matches!(name, "." | "..")
        || name.chars().any(char::is_control)
        || name.contains(['/', '\\', ':'])
        || name.ends_with(['.', ' '])
        || name.eq_ignore_ascii_case(".qingyu")
        || name.to_ascii_lowercase().starts_with(".qingyu-ui-update-")
        || name.to_ascii_lowercase().starts_with(".qingyu-mcp-update-")
        || name.to_ascii_lowercase().starts_with(".markra-sync-stage-")
        || windows_device_name
    {
        return Err(PathPolicyError::invalid_managed_name());
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PathPolicyErrorKind {
    Unavailable,
    OverlappingRoots,
    InvalidManagedName,
    UnsafeEntry,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct PathPolicyError {
    kind: PathPolicyErrorKind,
}

impl PathPolicyError {
    pub const fn kind(self) -> PathPolicyErrorKind {
        self.kind
    }

    const fn unavailable() -> Self {
        Self {
            kind: PathPolicyErrorKind::Unavailable,
        }
    }

    const fn overlapping_roots() -> Self {
        Self {
            kind: PathPolicyErrorKind::OverlappingRoots,
        }
    }

    const fn invalid_managed_name() -> Self {
        Self {
            kind: PathPolicyErrorKind::InvalidManagedName,
        }
    }

    const fn unsafe_entry() -> Self {
        Self {
            kind: PathPolicyErrorKind::UnsafeEntry,
        }
    }
}

impl fmt::Debug for PathPolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PathPolicyError")
            .field("kind", &self.kind)
            .finish()
    }
}

impl fmt::Display for PathPolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self.kind {
            PathPolicyErrorKind::Unavailable => "a required directory is unavailable",
            PathPolicyErrorKind::OverlappingRoots => "kernel roots must not overlap",
            PathPolicyErrorKind::InvalidManagedName => "the managed workspace name is invalid",
            PathPolicyErrorKind::UnsafeEntry => "a directory entry is unsafe",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for PathPolicyError {}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use tempfile::tempdir;

    use super::{
        created_child_parent_sync_count, open_or_create_child,
        reset_created_child_parent_sync_count, KernelPaths, ServerPathLayout,
    };

    #[test]
    fn fresh_child_creation_synchronizes_its_retained_parent_once() {
        let temporary = tempdir().expect("temporary root");
        let parent =
            cap_std::fs::Dir::open_ambient_dir(temporary.path(), cap_std::ambient_authority())
                .expect("open parent");

        reset_created_child_parent_sync_count();
        let child = open_or_create_child(&parent, "created").expect("create child");
        drop(child);
        assert_eq!(created_child_parent_sync_count(), 1);

        reset_created_child_parent_sync_count();
        let existing = open_or_create_child(&parent, "created").expect("open existing child");
        drop(existing);
        assert_eq!(created_child_parent_sync_count(), 0);
    }

    #[test]
    fn fixture_server_layout_activates_without_weakening_production_server_policy() {
        let temporary = tempdir().expect("temporary root");
        let data_root = temporary.path().join("data");
        let cache_root = temporary.path().join("cache");
        fs::create_dir(&data_root).expect("data root");

        let paths = ServerPathLayout::for_test(&data_root, &cache_root)
            .activate()
            .expect("activate fixture");

        assert_eq!(paths.profile(), crate::contract::HostProfile::Server);
        for name in ["workspace", "config", "state", "logs"] {
            assert!(data_root.join(name).is_dir());
        }
        assert!(cache_root.is_dir());
        assert_eq!(
            KernelPaths::server().workspace_path(),
            Path::new("/data/workspace")
        );
    }
}
