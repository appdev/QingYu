//! Kernel-owned adapter for one Dejavu repository synchronization attempt.
//!
//! The runner receives every authority, path, credential, and platform-neutral
//! collaborator explicitly. It never reads desktop state or a process-global
//! configuration source.

use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use qingyu_dejavu::{
    Cloud, Device, MergeResult, Repo, RepoError, RepoOptions, RepoPaths, RepositoryRuntimeState,
    S3AddressingStyle, S3Cloud, S3Connection, S3TlsVerification, S3TransportOptions, TrafficStat,
    WorkingTreeChange, WorkingTreeCoordinator, WorkingTreePermit,
};
use time::format_description::well_known::Rfc3339;
use zeroize::Zeroizing;

const SYNCIGNORE_PATH: &str = "/.qingyu/syncignore";

/// Retained authority for the single active workspace.
///
/// A platform adapter must keep the directory handle and workspace lock alive
/// for at least as long as the runner. Returning a path alone is not authority;
/// the runner calls `verify_held_directory` before and after the Dejavu attempt.
pub trait DejavuWorkspaceCapability: Send + Sync {
    fn verify_held_directory(&self) -> Result<(), DejavuWorkspaceCapabilityError>;
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

/// Three instance-private roots used by Dejavu for encrypted state, conflict
/// history, and staged downloads. They must remain outside the workspace.
pub struct DejavuInstanceRoots {
    state: PathBuf,
    history: PathBuf,
    temp: PathBuf,
}

impl DejavuInstanceRoots {
    pub fn new(state: PathBuf, history: PathBuf, temp: PathBuf) -> Self {
        Self {
            state,
            history,
            temp,
        }
    }
}

impl fmt::Debug for DejavuInstanceRoots {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DejavuInstanceRoots([REDACTED])")
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
    pub roots: DejavuInstanceRoots,
    pub repository_id: String,
    pub device: Device,
    pub options: RepoOptions,
    pub repository_key: DejavuRepositoryKey,
    pub runtime: RepositoryRuntimeState,
    pub coordinator: Arc<dyn WorkingTreeCoordinator>,
}

pub struct KernelDejavuRunner {
    workspace: Arc<dyn DejavuWorkspaceCapability>,
    paths: RepoPaths,
    repository_id: String,
    device: Device,
    options: RepoOptions,
    repository_key: DejavuRepositoryKey,
    runtime: RepositoryRuntimeState,
    coordinator: Arc<dyn WorkingTreeCoordinator>,
    cloud: Arc<dyn Cloud>,
}

impl KernelDejavuRunner {
    /// Creates the production S3-backed runner without consulting ambient
    /// configuration. The repository prefix is derived from the canonical ID.
    pub fn new_s3(
        inputs: DejavuRunnerInputs,
        config: DejavuS3Config,
    ) -> Result<Self, DejavuRunError> {
        validate_repository_id(&inputs.repository_id)?;
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
        let prefix = format!("qingyu/repositories/{}/repo", inputs.repository_id);
        let cloud = S3Cloud::new(connection, options, &prefix)
            .map_err(|_| DejavuRunError::InvalidConfiguration)?;
        Self::new_with_cloud(inputs, Arc::new(cloud))
    }

    /// Creates a runner around a caller-owned Dejavu cloud. This is useful for
    /// deterministic local clouds and future non-S3 transports.
    pub fn new_with_cloud(
        inputs: DejavuRunnerInputs,
        cloud: Arc<dyn Cloud>,
    ) -> Result<Self, DejavuRunError> {
        validate_repository_id(&inputs.repository_id)?;
        inputs
            .workspace
            .verify_held_directory()
            .map_err(|_| DejavuRunError::WorkspaceUnavailable)?;
        let workspace = inputs.workspace.canonical_path().to_path_buf();
        validate_layout(&workspace, &inputs.roots)?;
        let DejavuInstanceRoots {
            state,
            history,
            temp,
        } = inputs.roots;
        Ok(Self {
            workspace: inputs.workspace,
            paths: RepoPaths {
                data: workspace,
                repo: state,
                history,
                temp,
            },
            repository_id: inputs.repository_id,
            device: inputs.device,
            options: inputs.options,
            repository_key: inputs.repository_key,
            runtime: inputs.runtime,
            coordinator: inputs.coordinator,
            cloud,
        })
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
        let repo = Repo::open_with_runtime(
            self.paths.clone(),
            self.device.clone(),
            self.repository_key.copy_for_repository(),
            self.options.clone(),
            &self.runtime,
        )
        .map_err(map_repo_error)?;
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

fn validate_repository_id(repository_id: &str) -> Result<(), DejavuRunError> {
    let canonical = uuid::Uuid::parse_str(repository_id)
        .map_err(|_| DejavuRunError::InvalidConfiguration)?
        .to_string();
    if canonical == repository_id {
        Ok(())
    } else {
        Err(DejavuRunError::InvalidConfiguration)
    }
}

fn validate_layout(workspace: &Path, roots: &DejavuInstanceRoots) -> Result<(), DejavuRunError> {
    let paths = [&roots.state, &roots.history, &roots.temp];
    if !path_is_normal_absolute(workspace)
        || paths.iter().any(|path| !path_is_normal_absolute(path))
        || paths_overlap(paths[0], paths[1])
        || paths_overlap(paths[0], paths[2])
        || paths_overlap(paths[1], paths[2])
        || paths
            .iter()
            .any(|path| path.starts_with(workspace) || workspace.starts_with(path))
    {
        return Err(DejavuRunError::InvalidConfiguration);
    }
    Ok(())
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
