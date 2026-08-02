//! Kernel-owned adapter for one Dejavu repository synchronization attempt.
//!
//! The runner receives retained workspace and instance-data authorities,
//! credentials, and platform-neutral collaborators explicitly. Repository
//! state paths and ignore options are derived inside the Kernel; it never reads
//! desktop state or a process-global configuration source.
//!
//! Conflict-document generation and zeroization inside the Dejavu core remain
//! separate activation gates. This adapter reports conflicts and zeroizes its
//! owned credential/key inputs, but does not claim those downstream guarantees.

use std::ffi::OsStr;
use std::fmt;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use cap_fs_ext::DirExt;
use cap_std::fs::Dir;
use qingyu_dejavu::{
    write_cap_file_no_replace_safer, Cloud, CloudError, Device, ExpectedRevision, MergeResult,
    Repo, RepoDirectoryCapabilities, RepoError, RepoOptions, RepoPaths, RepositoryRelativePath,
    RepositoryRuntimeState, S3AddressingStyle, S3Cloud, S3Connection, S3TlsVerification,
    S3TransportOptions, TrafficStat, WorkingTreeAction, WorkingTreeChange, WorkingTreeCoordinator,
    WorkingTreePermit,
};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use zeroize::Zeroizing;

use crate::runtime::MutationCoordinator;
use crate::storage::{
    create_private_file_options, directory_identity, nonfollowing_read_options,
    open_canonical_directory_nofollow, sync_directory, unique_regular_file_identity,
    DirectoryIdentity,
};

const SYNCIGNORE_PATH: &str = "/.qingyu/syncignore";
const SYNCIGNORE_DIRECTORY: &str = ".qingyu";
const SYNCIGNORE_FILE: &str = "syncignore";
const MAX_SYNCIGNORE_BYTES: usize = 1024 * 1024;
const MAX_CONFLICT_DOCUMENT_BYTES: u64 = 16 * 1024 * 1024;

/// Retained authority for the single active workspace.
///
/// A platform adapter must keep the directory handle and workspace lock alive
/// for at least as long as the runner. Returning a path alone is not authority;
/// the runner calls `verify_held_directory` before and after the Dejavu attempt.
pub trait DejavuWorkspaceCapability: Send + Sync {
    fn verify_held_directory(&self) -> Result<(), DejavuWorkspaceCapabilityError>;
    fn try_clone_directory(&self) -> Result<Dir, DejavuWorkspaceCapabilityError>;
    fn canonical_path(&self) -> &Path;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DejavuWorkspaceCapabilityError;

impl fmt::Display for DejavuWorkspaceCapabilityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("the retained workspace capability is unavailable")
    }
}

impl std::error::Error for DejavuWorkspaceCapabilityError {}

impl DejavuWorkspaceCapability for crate::paths::WorkspaceRoot {
    fn verify_held_directory(&self) -> Result<(), DejavuWorkspaceCapabilityError> {
        crate::paths::WorkspaceRoot::verify_held_directory(self)
            .map_err(|_| DejavuWorkspaceCapabilityError)
    }

    fn try_clone_directory(&self) -> Result<Dir, DejavuWorkspaceCapabilityError> {
        self.try_clone_dir()
            .map_err(|_| DejavuWorkspaceCapabilityError)
    }

    fn canonical_path(&self) -> &Path {
        crate::paths::WorkspaceRoot::canonical_path(self)
    }
}

impl DejavuWorkspaceCapability for crate::runtime::ActiveWorkspaceAuthority {
    fn verify_held_directory(&self) -> Result<(), DejavuWorkspaceCapabilityError> {
        crate::runtime::ActiveWorkspaceAuthority::verify_held_directory(self)
            .map_err(|_| DejavuWorkspaceCapabilityError)
    }

    fn try_clone_directory(&self) -> Result<Dir, DejavuWorkspaceCapabilityError> {
        self.root()
            .try_clone_dir()
            .map_err(|_| DejavuWorkspaceCapabilityError)
    }

    fn canonical_path(&self) -> &Path {
        self.root().canonical_path()
    }
}

/// Retained authority for Kernel instance data. Repository state is always
/// derived beneath this capability and can never be selected by the caller.
pub trait DejavuInstanceDataCapability: Send + Sync {
    fn verify_held_directory(&self) -> Result<(), DejavuInstanceDataCapabilityError>;
    fn try_clone_directory(&self) -> Result<Dir, DejavuInstanceDataCapabilityError>;
    fn canonical_path(&self) -> &Path;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DejavuInstanceDataCapabilityError;

impl fmt::Display for DejavuInstanceDataCapabilityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("the retained instance-data capability is unavailable")
    }
}

impl std::error::Error for DejavuInstanceDataCapabilityError {}

impl DejavuInstanceDataCapability for crate::paths::InstanceDataRoot {
    fn verify_held_directory(&self) -> Result<(), DejavuInstanceDataCapabilityError> {
        crate::paths::InstanceDataRoot::verify_held_directory(self)
            .map_err(|_| DejavuInstanceDataCapabilityError)
    }

    fn try_clone_directory(&self) -> Result<Dir, DejavuInstanceDataCapabilityError> {
        self.try_clone_dir()
            .map_err(|_| DejavuInstanceDataCapabilityError)
    }

    fn canonical_path(&self) -> &Path {
        crate::paths::InstanceDataRoot::canonical_path(self)
    }
}

impl DejavuInstanceDataCapability for crate::runtime::ActiveInstanceAuthority {
    fn verify_held_directory(&self) -> Result<(), DejavuInstanceDataCapabilityError> {
        crate::runtime::ActiveInstanceAuthority::verify_held_directory(self)
            .map_err(|_| DejavuInstanceDataCapabilityError)
    }

    fn try_clone_directory(&self) -> Result<Dir, DejavuInstanceDataCapabilityError> {
        self.root()
            .try_clone_dir()
            .map_err(|_| DejavuInstanceDataCapabilityError)
    }

    fn canonical_path(&self) -> &Path {
        self.root().canonical_path()
    }
}

/// String secret whose owned buffer is zeroized on drop.
pub struct DejavuSecret(Zeroizing<String>);

impl DejavuSecret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(Zeroizing::new(value.into()))
    }

    fn expose_secret(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Debug for DejavuSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DejavuSecret([REDACTED])")
    }
}

/// The 256-bit Dejavu repository encryption key.
pub struct DejavuRepositoryKey(Zeroizing<[u8; 32]>);

impl DejavuRepositoryKey {
    pub fn new(value: [u8; 32]) -> Self {
        Self(Zeroizing::new(value))
    }

    fn copy_for_repository(&self) -> [u8; 32] {
        *self.0
    }
}

impl fmt::Debug for DejavuRepositoryKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DejavuRepositoryKey([REDACTED])")
    }
}

pub struct DejavuS3Config {
    pub endpoint_url: String,
    pub region: String,
    pub bucket: String,
    pub access_key_id: DejavuSecret,
    pub secret_access_key: DejavuSecret,
    pub request_timeout: Duration,
    pub addressing_style: S3AddressingStyle,
    pub tls_verification: S3TlsVerification,
}

impl fmt::Debug for DejavuS3Config {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DejavuS3Config")
            .field("endpoint_url", &"[REDACTED]")
            .field("region", &self.region)
            .field("bucket", &self.bucket)
            .field("access_key_id", &"[REDACTED]")
            .field("secret_access_key", &"[REDACTED]")
            .field("request_timeout", &self.request_timeout)
            .field("addressing_style", &self.addressing_style)
            .field("tls_verification", &self.tls_verification)
            .finish()
    }
}

/// Explicit, platform-neutral inputs for a repository runner.
pub struct DejavuRunnerInputs {
    pub workspace: Arc<dyn DejavuWorkspaceCapability>,
    pub instance_data: Arc<dyn DejavuInstanceDataCapability>,
    pub repository_id: String,
    pub device: Device,
    pub repository_key: DejavuRepositoryKey,
    pub runtime: RepositoryRuntimeState,
    pub coordinator: Arc<dyn WorkingTreeCoordinator>,
}

/// Adapts DejaVu's owned working-tree permit to the Kernel-wide mutation gate.
///
/// DejaVu remains responsible for revalidating every planned file revision
/// after this permit is acquired and before its first filesystem mutation.
#[derive(Clone)]
pub struct MutationWorkingTreeCoordinator {
    mutation: Arc<MutationCoordinator>,
}

impl MutationWorkingTreeCoordinator {
    pub const fn new(mutation: Arc<MutationCoordinator>) -> Self {
        Self { mutation }
    }
}

#[async_trait::async_trait]
impl WorkingTreeCoordinator for MutationWorkingTreeCoordinator {
    async fn prepare(
        &self,
        _changes: &[WorkingTreeChange],
    ) -> Result<WorkingTreePermit, RepoError> {
        Ok(WorkingTreePermit::new(self.mutation.lock_owned().await))
    }

    async fn release(&self, permit: WorkingTreePermit) {
        drop(permit);
    }
}

pub struct KernelDejavuRunner {
    workspace: Arc<dyn DejavuWorkspaceCapability>,
    instance_data: Arc<dyn DejavuInstanceDataCapability>,
    paths: PreparedRepositoryPaths,
    repository_id: String,
    device: Device,
    repository_key: DejavuRepositoryKey,
    runtime: RepositoryRuntimeState,
    coordinator: Arc<dyn WorkingTreeCoordinator>,
    cloud: Arc<dyn Cloud>,
    remote_prefix: String,
}

impl KernelDejavuRunner {
    /// Creates the production S3-backed runner without consulting ambient
    /// configuration. The repository prefix is derived from the canonical ID.
    pub fn new_s3(
        inputs: DejavuRunnerInputs,
        config: DejavuS3Config,
    ) -> Result<Self, DejavuRunError> {
        let repository_id = canonical_repository_id(&inputs.repository_id)?;
        let connection = S3Connection::new(
            &config.endpoint_url,
            &config.region,
            &config.bucket,
            config.access_key_id.expose_secret(),
            config.secret_access_key.expose_secret(),
            config.addressing_style,
        )
        .map_err(|_| DejavuRunError::InvalidConfiguration)?;
        let options = S3TransportOptions {
            request_timeout: config.request_timeout,
            tls_verification: config.tls_verification,
            max_attempts: S3TransportOptions::default().max_attempts,
        };
        let prefix = repository_remote_prefix(&repository_id);
        let cloud = S3Cloud::new(connection, options, &prefix)
            .map_err(|_| DejavuRunError::InvalidConfiguration)?;
        Self::new_with_cloud_and_prefix(inputs, Arc::new(cloud), prefix)
    }

    /// Creates a runner around a caller-owned Dejavu cloud. This is useful for
    /// deterministic local clouds and future non-S3 transports.
    pub fn new_with_cloud(
        inputs: DejavuRunnerInputs,
        cloud: Arc<dyn Cloud>,
    ) -> Result<Self, DejavuRunError> {
        let repository_id = canonical_repository_id(&inputs.repository_id)?;
        let remote_prefix = repository_remote_prefix(&repository_id);
        Self::new_with_cloud_and_prefix(inputs, cloud, remote_prefix)
    }

    fn new_with_cloud_and_prefix(
        inputs: DejavuRunnerInputs,
        cloud: Arc<dyn Cloud>,
        remote_prefix: String,
    ) -> Result<Self, DejavuRunError> {
        let repository_id = canonical_repository_id(&inputs.repository_id)?;
        let instance_data = inputs.instance_data;
        let paths = prepare_repository_layout(
            Arc::clone(&instance_data),
            Arc::clone(&inputs.workspace),
            &repository_id,
        )?;
        Ok(Self {
            workspace: inputs.workspace,
            instance_data,
            paths,
            repository_id,
            device: inputs.device,
            repository_key: inputs.repository_key,
            runtime: inputs.runtime,
            coordinator: inputs.coordinator,
            cloud,
            remote_prefix,
        })
    }

    /// Exact repository-specific cloud namespace used by the S3 adapter.
    pub fn remote_prefix(&self) -> &str {
        &self.remote_prefix
    }

    /// Runs exactly one bidirectional Dejavu attempt.
    ///
    /// Cancellation is observed before opening and at the working-tree permit
    /// boundary. Once the core future has completed its writes, that committed
    /// result wins over a late cancellation request. The current Dejavu API
    /// does not expose a cancellable cloud-I/O hook, so this method never
    /// detaches or abandons the in-flight `Repo::sync` future.
    pub async fn run(
        &self,
        cancelled: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<DejavuRunResult, DejavuRunError> {
        if cancelled() {
            return Err(DejavuRunError::Cancelled);
        }
        self.workspace
            .verify_held_directory()
            .map_err(|_| DejavuRunError::WorkspaceUnavailable)?;
        self.instance_data
            .verify_held_directory()
            .map_err(|_| DejavuRunError::RepositoryUnavailable)?;
        self.paths.revalidate()?;
        let options = prepare_syncignore(self.workspace.as_ref())?;
        let data_path = self.workspace.canonical_path().to_path_buf();
        let data_directory = validated_workspace_directory(self.workspace.as_ref())?;
        let capabilities = self.paths.directory_capabilities(data_directory)?;
        let repo = Repo::open_with_capabilities_and_runtime(
            self.paths.repo_paths(data_path),
            capabilities,
            self.device.clone(),
            self.repository_key.copy_for_repository(),
            options,
            &self.runtime,
        )
        .map_err(map_repo_error)?;
        self.paths.revalidate()?;
        if cancelled() {
            return Err(DejavuRunError::Cancelled);
        }
        let coordinator: Arc<dyn WorkingTreeCoordinator> = Arc::new(CancellationAwareCoordinator {
            inner: Arc::clone(&self.coordinator),
            workspace: Arc::clone(&self.workspace),
            instance_data: Arc::clone(&self.instance_data),
            cancelled: Arc::clone(&cancelled),
        });
        let result = repo
            .sync(Arc::clone(&self.cloud), coordinator)
            .await
            .map_err(map_repo_error)?;
        self.paths.revalidate()?;
        self.workspace
            .verify_held_directory()
            .map_err(|_| DejavuRunError::WorkspaceUnavailable)?;
        self.instance_data
            .verify_held_directory()
            .map_err(|_| DejavuRunError::RepositoryUnavailable)?;
        let scanned_files = repo
            .latest()
            .map_err(map_repo_error)?
            .map(|index| usize_u64(index.count))
            .unwrap_or(0);
        map_result(&self.repository_id, scanned_files, result)
    }
}

impl fmt::Debug for KernelDejavuRunner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("KernelDejavuRunner([REDACTED])")
    }
}

struct CancellationAwareCoordinator {
    inner: Arc<dyn WorkingTreeCoordinator>,
    workspace: Arc<dyn DejavuWorkspaceCapability>,
    instance_data: Arc<dyn DejavuInstanceDataCapability>,
    cancelled: Arc<dyn Fn() -> bool + Send + Sync>,
}

#[async_trait::async_trait]
impl WorkingTreeCoordinator for CancellationAwareCoordinator {
    async fn prepare(&self, changes: &[WorkingTreeChange]) -> Result<WorkingTreePermit, RepoError> {
        if (self.cancelled)() {
            return Err(RepoError::Cancelled);
        }
        self.workspace
            .verify_held_directory()
            .map_err(|_| RepoError::UnsafePath)?;
        self.instance_data
            .verify_held_directory()
            .map_err(|_| RepoError::InvalidData("instance capability unavailable"))?;
        let permit = self.inner.prepare(changes).await?;
        if (self.cancelled)() {
            self.inner.release(permit).await;
            return Err(RepoError::Cancelled);
        }
        if self.workspace.verify_held_directory().is_err() {
            self.inner.release(permit).await;
            return Err(RepoError::UnsafePath);
        }
        if self.instance_data.verify_held_directory().is_err() {
            self.inner.release(permit).await;
            return Err(RepoError::InvalidData("instance capability unavailable"));
        }
        Ok(permit)
    }

    async fn release(&self, permit: WorkingTreePermit) {
        self.inner.release(permit).await;
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DejavuTransferSummary {
    pub download_bytes: u64,
    pub download_chunks: u64,
    pub download_files: u64,
    pub upload_bytes: u64,
    pub upload_chunks: u64,
    pub upload_files: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DejavuConflictResolution {
    KeepLocal,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DejavuConflict {
    pub conflict_id: String,
    pub repository_id: String,
    pub relative_path: String,
    pub occurred_at: String,
    pub resolution: DejavuConflictResolution,
}

/// Best-effort compatibility for the existing conflict-document option.
///
/// A completed repository synchronization is never changed into a failure only
/// because its optional copy cannot be materialized. Every attempted copy still
/// enters the Kernel-wide mutation gate and writes with no-replace semantics.
pub(crate) async fn create_conflict_documents(
    workspace: Arc<dyn DejavuWorkspaceCapability>,
    instance_data: Arc<dyn DejavuInstanceDataCapability>,
    conflicts: &[DejavuConflict],
    coordinator: Arc<dyn WorkingTreeCoordinator>,
) -> Result<(), DejavuRunError> {
    workspace
        .verify_held_directory()
        .map_err(|_| DejavuRunError::WorkspaceUnavailable)?;
    instance_data
        .verify_held_directory()
        .map_err(|_| DejavuRunError::RepositoryUnavailable)?;
    for conflict in conflicts {
        let _copy = create_conflict_document(
            Arc::clone(&workspace),
            Arc::clone(&instance_data),
            conflict,
            Arc::clone(&coordinator),
        )
        .await;
    }
    workspace
        .verify_held_directory()
        .map_err(|_| DejavuRunError::WorkspaceUnavailable)?;
    instance_data
        .verify_held_directory()
        .map_err(|_| DejavuRunError::RepositoryUnavailable)
}

async fn create_conflict_document(
    workspace: Arc<dyn DejavuWorkspaceCapability>,
    instance_data: Arc<dyn DejavuInstanceDataCapability>,
    conflict: &DejavuConflict,
    coordinator: Arc<dyn WorkingTreeCoordinator>,
) -> Result<(), DejavuRunError> {
    let source = RepositoryRelativePath::new(conflict.relative_path.clone())
        .map_err(|_| DejavuRunError::RepositoryUnavailable)?;
    let source_path = PathBuf::from(source.as_str());
    if source_path
        .extension()
        .and_then(OsStr::to_str)
        .is_none_or(|extension| !extension.eq_ignore_ascii_case("md"))
    {
        return Ok(());
    }
    let occurred = OffsetDateTime::parse(&conflict.occurred_at, &Rfc3339)
        .map_err(|_| DejavuRunError::RepositoryUnavailable)?;
    let paths = prepare_repository_layout(
        instance_data,
        Arc::clone(&workspace),
        &canonical_repository_id(&conflict.repository_id)?,
    )?;
    let snapshot = format!(
        "{:04}-{:02}-{:02}-{:02}{:02}{:02}-sync",
        occurred.year(),
        u8::from(occurred.month()),
        occurred.day(),
        occurred.hour(),
        occurred.minute(),
        occurred.second()
    );
    let history_snapshot = paths
        .history
        .directory
        .open_dir_nofollow(&snapshot)
        .map_err(|_| DejavuRunError::RepositoryUnavailable)?;
    let remote_bytes = read_bounded_relative_file(history_snapshot, &source_path)?;
    paths.revalidate()?;

    let workspace_root = validated_workspace_directory(workspace.as_ref())?;
    let root_identity =
        directory_identity(&workspace_root).map_err(|_| DejavuRunError::WorkspaceUnavailable)?;
    let stem = source_path
        .file_stem()
        .and_then(OsStr::to_str)
        .ok_or(DejavuRunError::RepositoryUnavailable)?;
    let name_prefix = format!(
        "{stem}-Conflicted-{:04}{:02}{:02}-{:02}{:02}{:02}",
        occurred.year(),
        u8::from(occurred.month()),
        occurred.day(),
        occurred.hour(),
        occurred.minute(),
        occurred.second()
    );
    let parent = source_path.parent().unwrap_or_else(|| Path::new(""));
    for ordinal in 1..=10_000_u32 {
        let suffix = if ordinal == 1 {
            String::new()
        } else {
            format!("-{ordinal}")
        };
        let destination = parent.join(format!("{name_prefix}{suffix}.md"));
        if relative_entry_exists(&workspace_root, &destination)? {
            continue;
        }
        let relative = destination
            .to_str()
            .ok_or(DejavuRunError::RepositoryUnavailable)?
            .replace('\\', "/");
        let change = WorkingTreeChange {
            path: RepositoryRelativePath::new(relative)
                .map_err(|_| DejavuRunError::RepositoryUnavailable)?,
            expected_revision: ExpectedRevision::Absent,
            action: WorkingTreeAction::Write,
        };
        let permit = coordinator
            .prepare(std::slice::from_ref(&change))
            .await
            .map_err(map_repo_error)?;
        let written = (|| {
            workspace
                .verify_held_directory()
                .map_err(|_| DejavuRunError::WorkspaceUnavailable)?;
            if directory_identity(&workspace_root)
                .map_err(|_| DejavuRunError::WorkspaceUnavailable)?
                != root_identity
            {
                return Err(DejavuRunError::WorkspaceUnavailable);
            }
            write_relative_file_no_replace(&workspace_root, &destination, &remote_bytes)
        })();
        coordinator.release(permit).await;
        match written? {
            true => return Ok(()),
            false => continue,
        }
    }
    Err(DejavuRunError::WorkingTreeChanged)
}

fn read_bounded_relative_file(root: Dir, relative: &Path) -> Result<Vec<u8>, DejavuRunError> {
    let (directory, name) = open_relative_parent(root, relative, false)?;
    let name = name.ok_or(DejavuRunError::RepositoryUnavailable)?;
    let addressed = directory
        .symlink_metadata(&name)
        .map_err(|_| DejavuRunError::RepositoryUnavailable)?;
    if addressed.len() > MAX_CONFLICT_DOCUMENT_BYTES {
        return Err(DejavuRunError::RepositoryUnavailable);
    }
    let identity =
        unique_regular_file_identity(&addressed).ok_or(DejavuRunError::RepositoryUnavailable)?;
    let mut file = directory
        .open_with(&name, &nonfollowing_read_options())
        .map_err(|_| DejavuRunError::RepositoryUnavailable)?;
    if !identity.matches_retained_regular_file(
        &file
            .metadata()
            .map_err(|_| DejavuRunError::RepositoryUnavailable)?,
        false,
    ) {
        return Err(DejavuRunError::RepositoryUnavailable);
    }
    let mut bytes = Vec::with_capacity(usize::try_from(addressed.len()).unwrap_or(0));
    Read::by_ref(&mut file)
        .take(MAX_CONFLICT_DOCUMENT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| DejavuRunError::RepositoryUnavailable)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_CONFLICT_DOCUMENT_BYTES
        || !identity.matches_retained_regular_file(
            &file
                .metadata()
                .map_err(|_| DejavuRunError::RepositoryUnavailable)?,
            false,
        )
    {
        return Err(DejavuRunError::RepositoryUnavailable);
    }
    Ok(bytes)
}

fn relative_entry_exists(root: &Dir, relative: &Path) -> Result<bool, DejavuRunError> {
    let root = root
        .try_clone()
        .map_err(|_| DejavuRunError::WorkspaceUnavailable)?;
    let (directory, name) = open_relative_parent(root, relative, false)?;
    let Some(name) = name else {
        return Ok(false);
    };
    match directory.symlink_metadata(name) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(DejavuRunError::RepositoryUnavailable),
    }
}

fn write_relative_file_no_replace(
    root: &Dir,
    relative: &Path,
    bytes: &[u8],
) -> Result<bool, DejavuRunError> {
    let root = root
        .try_clone()
        .map_err(|_| DejavuRunError::WorkspaceUnavailable)?;
    let (directory, name) = open_relative_parent(root, relative, true)?;
    let name = name.ok_or(DejavuRunError::RepositoryUnavailable)?;
    write_cap_file_no_replace_safer(&directory, &name, bytes, 0o600)
        .map_err(|_| DejavuRunError::RepositoryUnavailable)
}

fn open_relative_parent(
    root: Dir,
    relative: &Path,
    create: bool,
) -> Result<(Dir, Option<std::ffi::OsString>), DejavuRunError> {
    let mut components = relative.components().peekable();
    let mut directory = root;
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            return Err(DejavuRunError::RepositoryUnavailable);
        };
        if components.peek().is_none() {
            return Ok((directory, Some(name.to_os_string())));
        }
        directory = match directory.open_dir_nofollow(name) {
            Ok(child) => child,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && create => {
                match directory.create_dir(name) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(_) => return Err(DejavuRunError::RepositoryUnavailable),
                }
                directory
                    .open_dir_nofollow(name)
                    .map_err(|_| DejavuRunError::RepositoryUnavailable)?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok((directory, None));
            }
            Err(_) => return Err(DejavuRunError::RepositoryUnavailable),
        };
    }
    Ok((directory, None))
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DejavuRunResult {
    pub data_changed: bool,
    pub scanned_files: u64,
    pub transfer: DejavuTransferSummary,
    pub conflicts: Vec<DejavuConflict>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DejavuRunError {
    InvalidConfiguration,
    WorkspaceUnavailable,
    PortableNameRequired { component: String },
    WorkingTreeChanged,
    Cancelled,
    RepositoryUnavailable,
    CloudUnavailable,
    DnsUnavailable,
    AuthenticationFailed,
    PermissionDenied,
    RateLimited,
    QuotaExceeded,
    IntegrityFailure,
    ClockSkew,
    RemoteConflict,
}

impl DejavuRunError {
    pub const fn safe_code(&self) -> &'static str {
        match self {
            Self::InvalidConfiguration => "dejavu-config-unavailable",
            Self::WorkspaceUnavailable => "dejavu-workspace-unavailable",
            Self::PortableNameRequired { .. } => "portable-name-required",
            Self::WorkingTreeChanged => "dejavu-working-tree-changed",
            Self::Cancelled => "dejavu-job-cancelled",
            Self::RepositoryUnavailable => "dejavu-repository-unavailable",
            Self::CloudUnavailable => "dejavu-cloud-unavailable",
            Self::DnsUnavailable => "dejavu-dns-unavailable",
            Self::AuthenticationFailed => "dejavu-authentication-failed",
            Self::PermissionDenied => "dejavu-permission-denied",
            Self::RateLimited => "dejavu-rate-limited",
            Self::QuotaExceeded => "dejavu-quota-exceeded",
            Self::IntegrityFailure => "dejavu-integrity-failed",
            Self::ClockSkew => "dejavu-clock-skew",
            Self::RemoteConflict => "dejavu-remote-conflict",
        }
    }
}

impl fmt::Display for DejavuRunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.safe_code())
    }
}

impl std::error::Error for DejavuRunError {}

fn canonical_repository_id(repository_id: &str) -> Result<String, DejavuRunError> {
    let canonical = uuid::Uuid::parse_str(repository_id)
        .map_err(|_| DejavuRunError::InvalidConfiguration)?
        .to_string();
    if canonical == repository_id {
        Ok(canonical)
    } else {
        Err(DejavuRunError::InvalidConfiguration)
    }
}

fn repository_remote_prefix(repository_id: &str) -> String {
    format!("qingyu/repositories/{repository_id}/repo")
}

struct PreparedRepositoryPaths {
    instance_data: Arc<dyn DejavuInstanceDataCapability>,
    sync: RetainedNamedDirectory,
    repositories: RetainedNamedDirectory,
    repository: RetainedNamedDirectory,
    repo: RetainedNamedDirectory,
    history: RetainedNamedDirectory,
    temp: RetainedNamedDirectory,
}

impl PreparedRepositoryPaths {
    fn repo_paths(&self, data: PathBuf) -> RepoPaths {
        RepoPaths {
            data,
            repo: self.repo.path.clone(),
            history: self.history.path.clone(),
            temp: self.temp.path.clone(),
        }
    }

    fn revalidate(&self) -> Result<(), DejavuRunError> {
        self.instance_data
            .verify_held_directory()
            .map_err(|_| DejavuRunError::RepositoryUnavailable)?;
        self.sync.revalidate()?;
        self.repositories.revalidate()?;
        self.repository.revalidate()?;
        self.repo.revalidate()?;
        self.history.revalidate()?;
        self.temp.revalidate()
    }

    fn directory_capabilities(
        &self,
        data: Dir,
    ) -> Result<RepoDirectoryCapabilities, DejavuRunError> {
        Ok(RepoDirectoryCapabilities::new(
            data,
            self.repo.try_clone_directory()?,
            self.history.try_clone_directory()?,
            self.temp.try_clone_directory()?,
        ))
    }
}

struct RetainedNamedDirectory {
    parent: Dir,
    name: String,
    directory: Dir,
    identity: DirectoryIdentity,
    path: PathBuf,
}

impl RetainedNamedDirectory {
    fn new(
        parent: &Dir,
        name: &str,
        directory: Dir,
        path: PathBuf,
    ) -> Result<Self, DejavuRunError> {
        let identity =
            directory_identity(&directory).map_err(|_| DejavuRunError::RepositoryUnavailable)?;
        Ok(Self {
            parent: parent
                .try_clone()
                .map_err(|_| DejavuRunError::RepositoryUnavailable)?,
            name: name.to_owned(),
            directory,
            identity,
            path,
        })
    }

    fn revalidate(&self) -> Result<(), DejavuRunError> {
        if directory_identity(&self.directory).map_err(|_| DejavuRunError::RepositoryUnavailable)?
            != self.identity
        {
            return Err(DejavuRunError::RepositoryUnavailable);
        }
        let addressed = self
            .parent
            .open_dir_nofollow(&self.name)
            .map_err(|_| DejavuRunError::RepositoryUnavailable)?;
        if directory_identity(&addressed).map_err(|_| DejavuRunError::RepositoryUnavailable)?
            != self.identity
        {
            return Err(DejavuRunError::RepositoryUnavailable);
        }
        Ok(())
    }

    fn try_clone_directory(&self) -> Result<Dir, DejavuRunError> {
        self.directory
            .try_clone()
            .map_err(|_| DejavuRunError::RepositoryUnavailable)
    }
}

fn prepare_repository_layout(
    instance_data: Arc<dyn DejavuInstanceDataCapability>,
    workspace: Arc<dyn DejavuWorkspaceCapability>,
    repository_id: &str,
) -> Result<PreparedRepositoryPaths, DejavuRunError> {
    let instance_directory = validated_instance_directory(instance_data.as_ref())?;
    let workspace_directory = validated_workspace_directory(workspace.as_ref())?;
    let instance_path = instance_data.canonical_path();
    let workspace_path = workspace.canonical_path();
    let repository_storage_path = instance_path.join("sync");
    if !path_is_normal_absolute(instance_path)
        || !path_is_normal_absolute(workspace_path)
        || paths_overlap(&repository_storage_path, workspace_path)
        || directory_identity(&instance_directory)
            .map_err(|_| DejavuRunError::InvalidConfiguration)?
            == directory_identity(&workspace_directory)
                .map_err(|_| DejavuRunError::InvalidConfiguration)?
    {
        return Err(DejavuRunError::InvalidConfiguration);
    }

    let sync_directory = open_or_create_directory(&instance_directory, "sync")?;
    let sync = RetainedNamedDirectory::new(
        &instance_directory,
        "sync",
        sync_directory,
        instance_path.join("sync"),
    )?;
    let repositories_directory = open_or_create_directory(&sync.directory, "repositories")?;
    let repositories = RetainedNamedDirectory::new(
        &sync.directory,
        "repositories",
        repositories_directory,
        sync.path.join("repositories"),
    )?;
    let repository_directory = open_or_create_directory(&repositories.directory, repository_id)?;
    let repository = RetainedNamedDirectory::new(
        &repositories.directory,
        repository_id,
        repository_directory,
        repositories.path.join(repository_id),
    )?;
    let repo_directory = open_or_create_directory(&repository.directory, "repo")?;
    let repo = RetainedNamedDirectory::new(
        &repository.directory,
        "repo",
        repo_directory,
        repository.path.join("repo"),
    )?;
    let history_directory = open_or_create_directory(&repository.directory, "history")?;
    let history = RetainedNamedDirectory::new(
        &repository.directory,
        "history",
        history_directory,
        repository.path.join("history"),
    )?;
    let temp_directory = open_or_create_directory(&repository.directory, "temp")?;
    let temp = RetainedNamedDirectory::new(
        &repository.directory,
        "temp",
        temp_directory,
        repository.path.join("temp"),
    )?;
    let paths = PreparedRepositoryPaths {
        instance_data,
        sync,
        repositories,
        repository,
        repo,
        history,
        temp,
    };
    paths.revalidate()?;
    Ok(paths)
}

fn validated_workspace_directory(
    capability: &dyn DejavuWorkspaceCapability,
) -> Result<Dir, DejavuRunError> {
    capability
        .verify_held_directory()
        .map_err(|_| DejavuRunError::WorkspaceUnavailable)?;
    let retained = capability
        .try_clone_directory()
        .map_err(|_| DejavuRunError::WorkspaceUnavailable)?;
    validate_capability_address(&retained, capability.canonical_path())
        .map_err(|_| DejavuRunError::WorkspaceUnavailable)?;
    Ok(retained)
}

fn validated_instance_directory(
    capability: &dyn DejavuInstanceDataCapability,
) -> Result<Dir, DejavuRunError> {
    capability
        .verify_held_directory()
        .map_err(|_| DejavuRunError::RepositoryUnavailable)?;
    let retained = capability
        .try_clone_directory()
        .map_err(|_| DejavuRunError::RepositoryUnavailable)?;
    validate_capability_address(&retained, capability.canonical_path())
        .map_err(|_| DejavuRunError::RepositoryUnavailable)?;
    Ok(retained)
}

fn validate_capability_address(directory: &Dir, path: &Path) -> Result<(), DejavuRunError> {
    if !path_is_normal_absolute(path) {
        return Err(DejavuRunError::InvalidConfiguration);
    }
    let addressed = open_canonical_directory_nofollow(path)
        .map_err(|_| DejavuRunError::RepositoryUnavailable)?;
    if directory_identity(directory).map_err(|_| DejavuRunError::RepositoryUnavailable)?
        != directory_identity(&addressed).map_err(|_| DejavuRunError::RepositoryUnavailable)?
    {
        return Err(DejavuRunError::RepositoryUnavailable);
    }
    Ok(())
}

fn open_or_create_directory(parent: &Dir, name: &str) -> Result<Dir, DejavuRunError> {
    crate::paths::open_or_create_child(parent, name)
        .map_err(|_| DejavuRunError::RepositoryUnavailable)
}

fn prepare_syncignore(
    workspace: &dyn DejavuWorkspaceCapability,
) -> Result<RepoOptions, DejavuRunError> {
    let root = validated_workspace_directory(workspace)?;
    let root_identity =
        directory_identity(&root).map_err(|_| DejavuRunError::WorkspaceUnavailable)?;
    let qingyu = open_or_create_directory(&root, SYNCIGNORE_DIRECTORY)?;
    let qingyu_identity =
        directory_identity(&qingyu).map_err(|_| DejavuRunError::RepositoryUnavailable)?;
    match qingyu.symlink_metadata(SYNCIGNORE_FILE) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(DejavuRunError::RepositoryUnavailable);
        }
        Ok(metadata) => {
            unique_regular_file_identity(&metadata).ok_or(DejavuRunError::RepositoryUnavailable)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match qingyu.open_with(SYNCIGNORE_FILE, &create_private_file_options()) {
                Ok(file) => file
                    .sync_all()
                    .map_err(|_| DejavuRunError::RepositoryUnavailable)?,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return Err(DejavuRunError::RepositoryUnavailable),
            }
        }
        Err(_) => return Err(DejavuRunError::RepositoryUnavailable),
    }
    let _directory_sync = sync_directory(&qingyu);
    let ignore_lines = read_syncignore(&qingyu)?;
    let addressed_qingyu = root
        .open_dir_nofollow(SYNCIGNORE_DIRECTORY)
        .map_err(|_| DejavuRunError::RepositoryUnavailable)?;
    if directory_identity(&root).map_err(|_| DejavuRunError::WorkspaceUnavailable)? != root_identity
        || directory_identity(&addressed_qingyu)
            .map_err(|_| DejavuRunError::RepositoryUnavailable)?
            != qingyu_identity
    {
        return Err(DejavuRunError::RepositoryUnavailable);
    }
    workspace
        .verify_held_directory()
        .map_err(|_| DejavuRunError::WorkspaceUnavailable)?;
    Ok(RepoOptions {
        ignore_lines,
        protected_include_paths: vec![SYNCIGNORE_PATH.to_owned()],
    })
}

fn read_syncignore(directory: &Dir) -> Result<Vec<String>, DejavuRunError> {
    let addressed = directory
        .symlink_metadata(SYNCIGNORE_FILE)
        .map_err(|_| DejavuRunError::RepositoryUnavailable)?;
    if addressed.len() > MAX_SYNCIGNORE_BYTES as u64 {
        return Err(DejavuRunError::RepositoryUnavailable);
    }
    let identity =
        unique_regular_file_identity(&addressed).ok_or(DejavuRunError::RepositoryUnavailable)?;
    let mut file = directory
        .open_with(SYNCIGNORE_FILE, &nonfollowing_read_options())
        .map_err(|_| DejavuRunError::RepositoryUnavailable)?;
    let retained = file
        .metadata()
        .map_err(|_| DejavuRunError::RepositoryUnavailable)?;
    if !identity.matches_retained_regular_file(&retained, false) {
        return Err(DejavuRunError::RepositoryUnavailable);
    }
    let mut bytes = Vec::with_capacity(retained.len() as usize);
    Read::by_ref(&mut file)
        .take(MAX_SYNCIGNORE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| DejavuRunError::RepositoryUnavailable)?;
    if bytes.len() > MAX_SYNCIGNORE_BYTES {
        return Err(DejavuRunError::RepositoryUnavailable);
    }
    let final_metadata = file
        .metadata()
        .map_err(|_| DejavuRunError::RepositoryUnavailable)?;
    if !identity.matches_retained_regular_file(&final_metadata, false) {
        return Err(DejavuRunError::RepositoryUnavailable);
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| DejavuRunError::RepositoryUnavailable)?;
    Ok(text.lines().map(str::to_owned).collect())
}

fn paths_overlap(first: &Path, second: &Path) -> bool {
    first.starts_with(second) || second.starts_with(first)
}

fn path_is_normal_absolute(path: &Path) -> bool {
    path.is_absolute()
        && path
            .components()
            .all(|component| !matches!(component, Component::CurDir | Component::ParentDir))
}

fn map_result(
    repository_id: &str,
    scanned_files: u64,
    (merge, traffic): (MergeResult, TrafficStat),
) -> Result<DejavuRunResult, DejavuRunError> {
    let data_changed = merge.data_changed();
    let occurred_at = merge
        .time
        .format(&Rfc3339)
        .map_err(|_| DejavuRunError::RepositoryUnavailable)?;
    let conflicts = merge
        .conflicts
        .into_iter()
        .filter(|file| file.path != SYNCIGNORE_PATH)
        .map(|file| {
            let relative_path = file
                .path
                .strip_prefix('/')
                .ok_or(DejavuRunError::RepositoryUnavailable)?
                .to_owned();
            qingyu_dejavu::RepositoryRelativePath::new(relative_path.clone())
                .map_err(|_| DejavuRunError::RepositoryUnavailable)?;
            Ok(DejavuConflict {
                conflict_id: uuid::Uuid::new_v4().to_string(),
                repository_id: repository_id.to_owned(),
                relative_path,
                occurred_at: occurred_at.clone(),
                resolution: DejavuConflictResolution::KeepLocal,
            })
        })
        .collect::<Result<Vec<_>, DejavuRunError>>()?;
    Ok(DejavuRunResult {
        data_changed,
        scanned_files,
        transfer: DejavuTransferSummary {
            download_bytes: nonnegative_u64(traffic.download_bytes),
            download_chunks: usize_u64(traffic.download_chunk_count),
            download_files: usize_u64(traffic.download_file_count),
            upload_bytes: nonnegative_u64(traffic.upload_bytes),
            upload_chunks: usize_u64(traffic.upload_chunk_count),
            upload_files: usize_u64(traffic.upload_file_count),
        },
        conflicts,
    })
}

fn map_repo_error(error: RepoError) -> DejavuRunError {
    match error {
        RepoError::WorkingTreeChanged => DejavuRunError::WorkingTreeChanged,
        RepoError::Cancelled => DejavuRunError::Cancelled,
        RepoError::Cloud(error) => map_cloud_error(&error),
        RepoError::RemoteLockUnhealthy(error) => map_cloud_code(error.code()),
        RepoError::OperationAndUnlockFailed { operation, .. } => map_repo_error(*operation),
        RepoError::FileIdentityCollision => DejavuRunError::IntegrityFailure,
        RepoError::DecryptionFailed => DejavuRunError::AuthenticationFailed,
        RepoError::UnsafePath => DejavuRunError::WorkspaceUnavailable,
        RepoError::PortableNameRequired { component } => {
            DejavuRunError::PortableNameRequired { component }
        }
        _ => DejavuRunError::RepositoryUnavailable,
    }
}

fn map_cloud_error(error: &CloudError) -> DejavuRunError {
    match error {
        CloudError::LockFailed { source } | CloudError::UnlockFailed { source } => {
            map_cloud_error(source)
        }
        CloudError::ResponseTooLarge { .. } | CloudError::LengthMismatch { .. } => {
            DejavuRunError::IntegrityFailure
        }
        CloudError::Locked | CloudError::AlreadyExists => DejavuRunError::RemoteConflict,
        _ => map_cloud_code(error.code()),
    }
}

fn map_cloud_code(code: &str) -> DejavuRunError {
    match code {
        "auth" => DejavuRunError::AuthenticationFailed,
        "forbidden" => DejavuRunError::PermissionDenied,
        "rate_limited" => DejavuRunError::RateLimited,
        "quota_exceeded" => DejavuRunError::QuotaExceeded,
        "clock_skew" => DejavuRunError::ClockSkew,
        "dns" => DejavuRunError::DnsUnavailable,
        "locked" | "already_exists" => DejavuRunError::RemoteConflict,
        "response_too_large" | "length_mismatch" => DejavuRunError::IntegrityFailure,
        _ => DejavuRunError::CloudUnavailable,
    }
}

fn nonnegative_u64(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

fn usize_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tempfile::tempdir;

    use super::{
        create_conflict_documents, map_repo_error, DejavuConflict, DejavuConflictResolution,
        DejavuInstanceDataCapability, DejavuWorkspaceCapability, MutationWorkingTreeCoordinator,
    };
    use crate::{paths::KernelPaths, runtime::MutationCoordinator};

    const REPOSITORY_ID: &str = "323df833-764a-44b3-a534-492640c258f2";

    #[test]
    fn cloud_error_classes_remain_typed_at_the_kernel_boundary() {
        use qingyu_dejavu::{CloudError, RepoError};

        let cases = [
            (
                CloudError::Auth,
                super::DejavuRunError::AuthenticationFailed,
            ),
            (
                CloudError::Forbidden,
                super::DejavuRunError::PermissionDenied,
            ),
            (CloudError::RateLimited, super::DejavuRunError::RateLimited),
            (
                CloudError::QuotaExceeded,
                super::DejavuRunError::QuotaExceeded,
            ),
            (
                CloudError::ResponseTooLarge { limit: 8 },
                super::DejavuRunError::IntegrityFailure,
            ),
            (CloudError::Dns, super::DejavuRunError::DnsUnavailable),
            (CloudError::Locked, super::DejavuRunError::RemoteConflict),
        ];
        for (cloud, expected) in cases {
            assert_eq!(map_repo_error(RepoError::Cloud(cloud)), expected);
        }
        assert_eq!(
            map_repo_error(RepoError::FileIdentityCollision),
            super::DejavuRunError::IntegrityFailure,
        );
        assert_eq!(
            map_repo_error(RepoError::DecryptionFailed),
            super::DejavuRunError::AuthenticationFailed,
        );
    }

    #[test]
    fn portable_name_errors_retain_the_exact_component() {
        use qingyu_dejavu::RepoError;

        assert_eq!(
            map_repo_error(RepoError::PortableNameRequired {
                component: r"bad\name.md".to_owned(),
            }),
            super::DejavuRunError::PortableNameRequired {
                component: r"bad\name.md".to_owned(),
            }
        );
    }

    #[tokio::test]
    async fn conflict_documents_preserve_collisions_and_use_the_remote_history_copy() {
        let temporary = tempdir().expect("temporary roots");
        let workspace_path = temporary.path().join("Workspace");
        let app_data = temporary.path().join("app-data");
        let cache = temporary.path().join("cache");
        std::fs::create_dir(&workspace_path).expect("workspace");
        std::fs::create_dir(&app_data).expect("app data");
        std::fs::create_dir(&cache).expect("cache");
        std::fs::create_dir(workspace_path.join("notes")).expect("notes directory");
        std::fs::write(
            workspace_path.join("notes/report-Conflicted-20260730-000000.md"),
            b"user collision",
        )
        .expect("collision");
        let history = app_data
            .join("sync/repositories")
            .join(REPOSITORY_ID)
            .join("history/2026-07-30-000000-sync/notes");
        std::fs::create_dir_all(&history).expect("history layout");
        std::fs::write(history.join("report.md"), b"remote conflict text").expect("remote history");
        let paths = KernelPaths::desktop(&workspace_path, &app_data, &cache).expect("paths");
        let workspace: Arc<dyn DejavuWorkspaceCapability> = paths.workspace_root_authority();
        let instance_data: Arc<dyn DejavuInstanceDataCapability> =
            paths.instance_data_root_authority();
        let coordinator = Arc::new(MutationWorkingTreeCoordinator::new(Arc::new(
            MutationCoordinator::new(),
        )));

        create_conflict_documents(
            workspace,
            instance_data,
            &[DejavuConflict {
                conflict_id: "4e8d1180-bd21-4d5b-bcbf-f977032a02e3".to_owned(),
                repository_id: REPOSITORY_ID.to_owned(),
                relative_path: "notes/report.md".to_owned(),
                occurred_at: "2026-07-30T00:00:00Z".to_owned(),
                resolution: DejavuConflictResolution::KeepLocal,
            }],
            coordinator,
        )
        .await
        .expect("best-effort conflict pass");

        assert_eq!(
            std::fs::read(workspace_path.join("notes/report-Conflicted-20260730-000000.md"))
                .expect("original collision"),
            b"user collision"
        );
        assert_eq!(
            std::fs::read(workspace_path.join("notes/report-Conflicted-20260730-000000-2.md"))
                .expect("generated conflict document"),
            b"remote conflict text"
        );
    }
}
