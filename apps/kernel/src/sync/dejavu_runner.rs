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

use std::fmt;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use cap_fs_ext::DirExt;
use cap_std::fs::Dir;
use qingyu_dejavu::{
    Cloud, Device, MergeResult, Repo, RepoError, RepoOptions, RepoPaths, RepositoryRuntimeState,
    S3AddressingStyle, S3Cloud, S3Connection, S3TlsVerification, S3TransportOptions, TrafficStat,
    WorkingTreeChange, WorkingTreeCoordinator, WorkingTreePermit,
};
use time::format_description::well_known::Rfc3339;
use zeroize::Zeroizing;

use crate::storage::{
    create_private_file_options, directory_identity, nonfollowing_read_options,
    open_canonical_directory_nofollow, sync_directory, unique_regular_file_identity,
    DirectoryIdentity,
};

const SYNCIGNORE_PATH: &str = "/.qingyu/syncignore";
const SYNCIGNORE_DIRECTORY: &str = ".qingyu";
const SYNCIGNORE_FILE: &str = "syncignore";
const MAX_SYNCIGNORE_BYTES: usize = 1024 * 1024;

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

pub struct KernelDejavuRunner {
    workspace: Arc<dyn DejavuWorkspaceCapability>,
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
        let paths = prepare_repository_layout(
            inputs.instance_data,
            Arc::clone(&inputs.workspace),
            &repository_id,
        )?;
        Ok(Self {
            workspace: inputs.workspace,
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
    /// Cancellation is observed before opening, at the working-tree permit
    /// boundary, and after the core future returns. The current Dejavu API does
    /// not expose a cancellable cloud-I/O hook, so this method never detaches or
    /// abandons the in-flight `Repo::sync` future.
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
        self.paths.revalidate()?;
        let options = prepare_syncignore(self.workspace.as_ref())?;
        let repo = Repo::open_with_runtime(
            self.paths
                .repo_paths(self.workspace.canonical_path().to_path_buf()),
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
        if cancelled() {
            return Err(DejavuRunError::Cancelled);
        }
        map_result(&self.repository_id, result)
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
        let permit = self.inner.prepare(changes).await?;
        if (self.cancelled)() {
            self.inner.release(permit).await;
            return Err(RepoError::Cancelled);
        }
        if self.workspace.verify_held_directory().is_err() {
            self.inner.release(permit).await;
            return Err(RepoError::UnsafePath);
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

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DejavuRunResult {
    pub data_changed: bool,
    pub transfer: DejavuTransferSummary,
    pub conflicts: Vec<DejavuConflict>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DejavuRunError {
    InvalidConfiguration,
    WorkspaceUnavailable,
    WorkingTreeChanged,
    Cancelled,
    RepositoryUnavailable,
    CloudUnavailable,
    DnsUnavailable,
}

impl DejavuRunError {
    pub const fn safe_code(self) -> &'static str {
        match self {
            Self::InvalidConfiguration => "dejavu-config-unavailable",
            Self::WorkspaceUnavailable => "dejavu-workspace-unavailable",
            Self::WorkingTreeChanged => "dejavu-working-tree-changed",
            Self::Cancelled => "dejavu-job-cancelled",
            Self::RepositoryUnavailable => "dejavu-repository-unavailable",
            Self::CloudUnavailable => "dejavu-cloud-unavailable",
            Self::DnsUnavailable => "dejavu-dns-unavailable",
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
    if !path_is_normal_absolute(instance_path)
        || !path_is_normal_absolute(workspace_path)
        || paths_overlap(instance_path, workspace_path)
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
    if repo_error_is_dns(&error) {
        return DejavuRunError::DnsUnavailable;
    }
    match error {
        RepoError::WorkingTreeChanged => DejavuRunError::WorkingTreeChanged,
        RepoError::Cancelled => DejavuRunError::Cancelled,
        RepoError::Cloud(_) | RepoError::RemoteLockUnhealthy(_) => DejavuRunError::CloudUnavailable,
        RepoError::OperationAndUnlockFailed { operation, .. } => map_repo_error(*operation),
        RepoError::UnsafePath => DejavuRunError::WorkspaceUnavailable,
        _ => DejavuRunError::RepositoryUnavailable,
    }
}

fn repo_error_is_dns(error: &RepoError) -> bool {
    match error {
        RepoError::Cloud(error) => error.is_dns(),
        RepoError::OperationAndUnlockFailed { operation, .. } => repo_error_is_dns(operation),
        _ => false,
    }
}

fn nonnegative_u64(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

fn usize_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}
