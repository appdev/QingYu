use std::fmt;
use std::future::Future;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use cap_fs_ext::DirExt;
use qingyu_dejavu::{
    Cloud, Device, Repo, RepoError, RepoOptions, RepoPaths, S3AddressingStyle, S3Cloud,
    S3Connection, S3TlsVerification, S3TransportOptions, WorkingTreeCoordinator,
};

use super::local_state::LocalSyncStateService;
use super::service::{
    RepositoryJobError, RepositoryJobRunner, RepositorySyncResult, SyncAttemptContext,
    SyncJobRequest,
};
use super::status::{RepositoryConflictRecord, RepositoryTransferSummary};
use crate::storage_capability::{
    create_private_file_options, directory_identity, nonfollowing_read_options,
    open_canonical_directory_nofollow, sync_directory, unique_regular_file_identity,
    DirectoryIdentity,
};
use crate::sync_config::model::{
    S3AddressingStyle as ConfigAddressingStyle, S3TlsVerification as ConfigTlsVerification,
    SyncTarget,
};
use crate::sync_config::ready_snapshot_at_app_data;
use crate::sync_config::storage::open_app_data;

const QINGYU_SYNCIGNORE_DIRECTORY: &str = ".qingyu";
const QINGYU_SYNCIGNORE_FILE: &str = "syncignore";
const QINGYU_SYNCIGNORE_PROTECTED_PATH: &str = "/.qingyu/syncignore";
const MAX_SYNCIGNORE_BYTES: usize = 1024 * 1024;

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[allow(dead_code)]
pub(crate) struct RepositoryCloudParameters {
    pub(crate) endpoint_url: String,
    pub(crate) region: String,
    pub(crate) bucket: String,
    pub(crate) access_key_id: String,
    pub(crate) secret_access_key: String,
    pub(crate) request_timeout_seconds: u32,
    pub(crate) addressing_style: String,
    pub(crate) tls_verification: String,
    pub(crate) repository_prefix: String,
}

impl fmt::Debug for RepositoryCloudParameters {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RepositoryCloudParameters")
            .field("endpoint_url", &"[REDACTED]")
            .field("region", &self.region)
            .field("bucket", &self.bucket)
            .field("access_key_id", &"[REDACTED]")
            .field("secret_access_key", &"[REDACTED]")
            .field("request_timeout_seconds", &self.request_timeout_seconds)
            .field("addressing_style", &self.addressing_style)
            .field("tls_verification", &self.tls_verification)
            .field("repository_prefix", &self.repository_prefix)
            .finish()
    }
}

pub(crate) trait RepositoryCloudFactory: Send + Sync {
    fn create(
        &self,
        parameters: RepositoryCloudParameters,
    ) -> Result<Arc<dyn Cloud>, RepositoryJobError>;
}

pub(crate) trait WorkingTreeCoordinatorFactory: Send + Sync {
    fn create(
        &self,
        context: &SyncAttemptContext,
    ) -> Result<Arc<dyn WorkingTreeCoordinator>, RepositoryJobError>;
}

#[allow(dead_code)]
struct S3RepositoryCloudFactory;

impl RepositoryCloudFactory for S3RepositoryCloudFactory {
    fn create(
        &self,
        parameters: RepositoryCloudParameters,
    ) -> Result<Arc<dyn Cloud>, RepositoryJobError> {
        let addressing_style = match parameters.addressing_style.as_str() {
            "auto" => S3AddressingStyle::Auto,
            "path" => S3AddressingStyle::Path,
            "virtual-hosted" => S3AddressingStyle::VirtualHosted,
            _ => return Err(RepositoryJobError::ConfigUnavailable),
        };
        let tls_verification = match parameters.tls_verification.as_str() {
            "verify" => S3TlsVerification::Verify,
            "skip" => S3TlsVerification::Skip,
            _ => return Err(RepositoryJobError::ConfigUnavailable),
        };
        let connection = S3Connection::new(
            &parameters.endpoint_url,
            &parameters.region,
            &parameters.bucket,
            &parameters.access_key_id,
            &parameters.secret_access_key,
            addressing_style,
        )
        .map_err(|_| RepositoryJobError::ConfigUnavailable)?;
        let options = S3TransportOptions {
            request_timeout: Duration::from_secs(u64::from(parameters.request_timeout_seconds)),
            tls_verification,
            max_attempts: S3TransportOptions::default().max_attempts,
        };
        let cloud = S3Cloud::new(connection, options, &parameters.repository_prefix)
            .map_err(|_| RepositoryJobError::CloudUnavailable)?;
        Ok(Arc::new(cloud))
    }
}

pub(crate) struct DejavuRepositoryRunner {
    app_data: PathBuf,
    coordinator_factory: Arc<dyn WorkingTreeCoordinatorFactory>,
    cloud_factory: Arc<dyn RepositoryCloudFactory>,
}

impl DejavuRepositoryRunner {
    #[allow(dead_code)]
    pub(crate) fn new(
        app_data: impl AsRef<Path>,
        coordinator_factory: Arc<dyn WorkingTreeCoordinatorFactory>,
    ) -> Self {
        Self {
            app_data: app_data.as_ref().to_path_buf(),
            coordinator_factory,
            cloud_factory: Arc::new(S3RepositoryCloudFactory),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_cloud_factory<Factory, CoordinatorFactory>(
        app_data: impl AsRef<Path>,
        coordinator_factory: Arc<CoordinatorFactory>,
        cloud_factory: Arc<Factory>,
    ) -> Self
    where
        Factory: RepositoryCloudFactory + 'static,
        CoordinatorFactory: WorkingTreeCoordinatorFactory + 'static,
    {
        let cloud_factory: Arc<dyn RepositoryCloudFactory> = cloud_factory;
        let coordinator_factory: Arc<dyn WorkingTreeCoordinatorFactory> = coordinator_factory;
        Self {
            app_data: app_data.as_ref().to_path_buf(),
            coordinator_factory,
            cloud_factory,
        }
    }

    pub(crate) async fn bind_and_sync(
        &self,
        context: SyncAttemptContext,
    ) -> Result<RepositorySyncResult, RepositoryJobError> {
        self.bind_and_sync_with_layout_observer(context, |_| {})
            .await
    }

    async fn bind_and_sync_with_layout_observer<Observe>(
        &self,
        context: SyncAttemptContext,
        after_layout_prepared: Observe,
    ) -> Result<RepositorySyncResult, RepositoryJobError>
    where
        Observe: FnOnce(&Path) + Send,
    {
        if context.attempt == 0
            || context.attempt > 3
            || uuid::Uuid::parse_str(&context.job_id)
                .ok()
                .is_none_or(|job_id| job_id.to_string() != context.job_id)
        {
            return Err(RepositoryJobError::RepositoryUnavailable);
        }
        if context.cancellation.is_cancelled() {
            return Err(RepositoryJobError::Cancelled);
        }
        let request = self.validate(context.request.clone())?;
        let coordinator = self.coordinator_factory.create(&SyncAttemptContext {
            request: request.clone(),
            ..context.clone()
        })?;
        let local_state = LocalSyncStateService::new(&self.app_data)
            .load()
            .map_err(|_| RepositoryJobError::InvalidBinding)?
            .ok_or(RepositoryJobError::InvalidBinding)?;
        let key_bytes = STANDARD
            .decode(&local_state.repo_key)
            .map_err(|_| RepositoryJobError::RepositoryUnavailable)?;
        let key: [u8; 32] = key_bytes
            .try_into()
            .map_err(|_| RepositoryJobError::RepositoryUnavailable)?;
        let snapshot = ready_snapshot_at_app_data(&self.app_data, None)
            .map_err(|_| RepositoryJobError::ConfigUnavailable)?;
        let SyncTarget::S3 {
            access_key_id,
            addressing_style,
            bucket,
            endpoint_url,
            region,
            remote_root: _,
            request_timeout_seconds,
            secret_access_key,
            tls_verification,
        } = snapshot.target
        else {
            return Err(RepositoryJobError::ConfigUnavailable);
        };

        let (canonical_notes_root, ignore_lines) = prepare_syncignore(&request.notes_root)?;
        let repository_paths = prepare_repository_layout(&self.app_data, &request.repository_id)?;
        after_layout_prepared(repository_paths.root_path());
        repository_paths.revalidate()?;
        let repository_prefix = format!("qingyu/repositories/{}/repo", request.repository_id);
        let cloud = self.cloud_factory.create(RepositoryCloudParameters {
            endpoint_url,
            region,
            bucket,
            access_key_id,
            secret_access_key,
            request_timeout_seconds,
            addressing_style: config_addressing_style(addressing_style).to_owned(),
            tls_verification: config_tls_verification(tls_verification).to_owned(),
            repository_prefix,
        })?;
        let repo = Repo::open(
            repository_paths.repo_paths(canonical_notes_root),
            Device {
                id: local_state.device_id,
                name: "QingYu".to_owned(),
                os: std::env::consts::OS.to_owned(),
            },
            key,
            RepoOptions {
                ignore_lines,
                protected_include_paths: vec![QINGYU_SYNCIGNORE_PROTECTED_PATH.to_owned()],
            },
        )
        .map_err(map_repo_error)?;
        repository_paths.revalidate()?;

        // Repo::sync owns the attempt's indexing and lifecycle/operation guards.
        let (merge, traffic) = repo
            .sync(cloud, coordinator)
            .await
            .map_err(map_repo_error)?;
        if context.cancellation.is_cancelled() {
            return Err(RepositoryJobError::Cancelled);
        }
        let occurred_at = timestamp(merge.time);
        let data_changed = merge.data_changed();
        let conflicts = merge
            .conflicts
            .into_iter()
            .map(|file| {
                let relative_path = file
                    .path
                    .strip_prefix('/')
                    .ok_or(RepositoryJobError::RepositoryUnavailable)?
                    .to_owned();
                qingyu_dejavu::RepositoryRelativePath::new(relative_path.clone())
                    .map_err(|_| RepositoryJobError::RepositoryUnavailable)?;
                Ok(RepositoryConflictRecord {
                    relative_path,
                    occurred_at: occurred_at.clone(),
                })
            })
            .collect::<Result<Vec<_>, RepositoryJobError>>()?;
        Ok(RepositorySyncResult {
            data_changed,
            transfer: RepositoryTransferSummary {
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
}

impl RepositoryJobRunner for DejavuRepositoryRunner {
    fn validate(&self, mut request: SyncJobRequest) -> Result<SyncJobRequest, RepositoryJobError> {
        request.repository_id = canonical_repository_id(&request.repository_id)?;
        request.notes_root = canonical_notes_root(&request.notes_root)?;
        let state = LocalSyncStateService::new(&self.app_data)
            .load()
            .map_err(|_| RepositoryJobError::InvalidBinding)?
            .ok_or(RepositoryJobError::InvalidBinding)?;
        let valid = state.bindings.iter().any(|binding| {
            binding.enabled
                && binding.repository_id == request.repository_id
                && binding.notes_root == request.notes_root
        });
        if !valid {
            return Err(RepositoryJobError::InvalidBinding);
        }
        Ok(request)
    }

    fn run_attempt<'a>(
        &'a self,
        context: SyncAttemptContext,
    ) -> BoxFuture<'a, Result<RepositorySyncResult, RepositoryJobError>> {
        Box::pin(async move { self.bind_and_sync(context).await })
    }
}

struct PreparedRepositoryPaths {
    repository: RetainedRepositoryDirectory,
    repo: RetainedRepositoryDirectory,
    history: RetainedRepositoryDirectory,
    temp: RetainedRepositoryDirectory,
}

struct RetainedRepositoryDirectory {
    path: PathBuf,
    directory: cap_std::fs::Dir,
    identity: DirectoryIdentity,
}

impl RetainedRepositoryDirectory {
    fn new(path: PathBuf, directory: cap_std::fs::Dir) -> Result<Self, RepositoryJobError> {
        let identity = directory_identity(&directory)
            .map_err(|_| RepositoryJobError::RepositoryUnavailable)?;
        Ok(Self {
            path,
            directory,
            identity,
        })
    }

    fn revalidate(&self) -> Result<(), RepositoryJobError> {
        if directory_identity(&self.directory)
            .map_err(|_| RepositoryJobError::RepositoryUnavailable)?
            != self.identity
        {
            return Err(RepositoryJobError::RepositoryUnavailable);
        }
        let reopened = open_canonical_directory_nofollow(&self.path)
            .map_err(|_| RepositoryJobError::RepositoryUnavailable)?;
        if directory_identity(&reopened).map_err(|_| RepositoryJobError::RepositoryUnavailable)?
            != self.identity
        {
            return Err(RepositoryJobError::RepositoryUnavailable);
        }
        Ok(())
    }
}

impl PreparedRepositoryPaths {
    fn root_path(&self) -> &Path {
        &self.repository.path
    }

    fn repo_paths(&self, data: PathBuf) -> RepoPaths {
        RepoPaths {
            data,
            repo: self.repo.path.clone(),
            history: self.history.path.clone(),
            temp: self.temp.path.clone(),
        }
    }

    fn revalidate(&self) -> Result<(), RepositoryJobError> {
        self.repository.revalidate()?;
        self.repo.revalidate()?;
        self.history.revalidate()?;
        self.temp.revalidate()
    }
}

fn canonical_repository_id(repository_id: &str) -> Result<String, RepositoryJobError> {
    let parsed =
        uuid::Uuid::parse_str(repository_id).map_err(|_| RepositoryJobError::InvalidBinding)?;
    let canonical = parsed.to_string();
    if canonical != repository_id {
        return Err(RepositoryJobError::InvalidBinding);
    }
    Ok(canonical)
}

fn canonical_notes_root(notes_root: &Path) -> Result<PathBuf, RepositoryJobError> {
    let canonical = notes_root
        .canonicalize()
        .map_err(|_| RepositoryJobError::InvalidBinding)?;
    let retained = open_canonical_directory_nofollow(&canonical)
        .map_err(|_| RepositoryJobError::InvalidBinding)?;
    let identity = directory_identity(&retained).map_err(|_| RepositoryJobError::InvalidBinding)?;
    let recanonicalized = notes_root
        .canonicalize()
        .map_err(|_| RepositoryJobError::InvalidBinding)?;
    let reopened = open_canonical_directory_nofollow(&canonical)
        .map_err(|_| RepositoryJobError::InvalidBinding)?;
    if recanonicalized != canonical
        || directory_identity(&reopened).map_err(|_| RepositoryJobError::InvalidBinding)?
            != identity
    {
        return Err(RepositoryJobError::InvalidBinding);
    }
    Ok(canonical)
}

fn prepare_syncignore(notes_root: &Path) -> Result<(PathBuf, Vec<String>), RepositoryJobError> {
    let canonical = canonical_notes_root(notes_root)?;
    let root = open_canonical_directory_nofollow(&canonical)
        .map_err(|_| RepositoryJobError::RepositoryUnavailable)?;
    let root_identity =
        directory_identity(&root).map_err(|_| RepositoryJobError::RepositoryUnavailable)?;
    let qingyu = open_or_create_directory(&root, QINGYU_SYNCIGNORE_DIRECTORY)?;
    match qingyu.symlink_metadata(QINGYU_SYNCIGNORE_FILE) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(RepositoryJobError::RepositoryUnavailable)
        }
        Ok(metadata) => {
            unique_regular_file_identity(&metadata)
                .ok_or(RepositoryJobError::RepositoryUnavailable)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match qingyu.open_with(QINGYU_SYNCIGNORE_FILE, &create_private_file_options()) {
                Ok(file) => file
                    .sync_all()
                    .map_err(|_| RepositoryJobError::RepositoryUnavailable)?,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return Err(RepositoryJobError::RepositoryUnavailable),
            }
        }
        Err(_) => return Err(RepositoryJobError::RepositoryUnavailable),
    }
    let _directory_sync = sync_directory(&qingyu);
    let ignore_lines = read_syncignore(&qingyu)?;
    let reopened = open_canonical_directory_nofollow(&canonical)
        .map_err(|_| RepositoryJobError::RepositoryUnavailable)?;
    if directory_identity(&reopened).map_err(|_| RepositoryJobError::RepositoryUnavailable)?
        != root_identity
    {
        return Err(RepositoryJobError::RepositoryUnavailable);
    }
    Ok((canonical, ignore_lines))
}

fn read_syncignore(directory: &cap_std::fs::Dir) -> Result<Vec<String>, RepositoryJobError> {
    let addressed = directory
        .symlink_metadata(QINGYU_SYNCIGNORE_FILE)
        .map_err(|_| RepositoryJobError::RepositoryUnavailable)?;
    if addressed.len() > MAX_SYNCIGNORE_BYTES as u64 {
        return Err(RepositoryJobError::RepositoryUnavailable);
    }
    let identity = unique_regular_file_identity(&addressed)
        .ok_or(RepositoryJobError::RepositoryUnavailable)?;
    let mut file = directory
        .open_with(QINGYU_SYNCIGNORE_FILE, &nonfollowing_read_options())
        .map_err(|_| RepositoryJobError::RepositoryUnavailable)?;
    let retained = file
        .metadata()
        .map_err(|_| RepositoryJobError::RepositoryUnavailable)?;
    if !identity.matches_retained_regular_file(&retained, false) {
        return Err(RepositoryJobError::RepositoryUnavailable);
    }
    let mut bytes = Vec::with_capacity(retained.len() as usize);
    Read::by_ref(&mut file)
        .take(MAX_SYNCIGNORE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| RepositoryJobError::RepositoryUnavailable)?;
    if bytes.len() > MAX_SYNCIGNORE_BYTES {
        return Err(RepositoryJobError::RepositoryUnavailable);
    }
    let final_metadata = file
        .metadata()
        .map_err(|_| RepositoryJobError::RepositoryUnavailable)?;
    if !identity.matches_retained_regular_file(&final_metadata, false) {
        return Err(RepositoryJobError::RepositoryUnavailable);
    }
    let text =
        std::str::from_utf8(&bytes).map_err(|_| RepositoryJobError::RepositoryUnavailable)?;
    Ok(text.lines().map(str::to_owned).collect())
}

fn prepare_repository_layout(
    app_data_path: &Path,
    repository_id: &str,
) -> Result<PreparedRepositoryPaths, RepositoryJobError> {
    let repository_id = canonical_repository_id(repository_id)?;
    let app_data = open_app_data(app_data_path, true)
        .map_err(|_| RepositoryJobError::RepositoryUnavailable)?
        .ok_or(RepositoryJobError::RepositoryUnavailable)?;
    let sync = open_or_create_directory(app_data.directory(), "sync")?;
    let repositories = open_or_create_directory(&sync, "repositories")?;
    let repository = open_or_create_directory(&repositories, &repository_id)?;
    let repo = open_or_create_directory(&repository, "repo")?;
    let history = open_or_create_directory(&repository, "history")?;
    let temp = open_or_create_directory(&repository, "temp")?;
    app_data
        .revalidate()
        .map_err(|_| RepositoryJobError::RepositoryUnavailable)?;
    let root = app_data
        .canonical_path()
        .join("sync")
        .join("repositories")
        .join(repository_id);
    let prepared = PreparedRepositoryPaths {
        repository: RetainedRepositoryDirectory::new(root.clone(), repository)?,
        repo: RetainedRepositoryDirectory::new(root.join("repo"), repo)?,
        history: RetainedRepositoryDirectory::new(root.join("history"), history)?,
        temp: RetainedRepositoryDirectory::new(root.join("temp"), temp)?,
    };
    prepared.revalidate()?;
    Ok(prepared)
}

fn open_or_create_directory(
    parent: &cap_std::fs::Dir,
    name: &str,
) -> Result<cap_std::fs::Dir, RepositoryJobError> {
    match parent.symlink_metadata(name) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(RepositoryJobError::RepositoryUnavailable)
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match parent.create_dir(name) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return Err(RepositoryJobError::RepositoryUnavailable),
            }
        }
        Err(_) => return Err(RepositoryJobError::RepositoryUnavailable),
    }
    let metadata = parent
        .symlink_metadata(name)
        .map_err(|_| RepositoryJobError::RepositoryUnavailable)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(RepositoryJobError::RepositoryUnavailable);
    }
    parent
        .open_dir_nofollow(name)
        .map_err(|_| RepositoryJobError::RepositoryUnavailable)
}

fn config_addressing_style(style: ConfigAddressingStyle) -> &'static str {
    match style {
        ConfigAddressingStyle::Auto => "auto",
        ConfigAddressingStyle::Path => "path",
        ConfigAddressingStyle::VirtualHosted => "virtual-hosted",
    }
}

fn config_tls_verification(verification: ConfigTlsVerification) -> &'static str {
    match verification {
        ConfigTlsVerification::Verify => "verify",
        ConfigTlsVerification::Skip => "skip",
    }
}

fn map_repo_error(error: RepoError) -> RepositoryJobError {
    if repo_error_is_dns(&error) {
        return RepositoryJobError::DnsUnavailable;
    }
    match error {
        RepoError::WorkingTreeChanged => RepositoryJobError::WorkingTreeChanged,
        RepoError::Cancelled => RepositoryJobError::Cancelled,
        RepoError::Cloud(_) | RepoError::RemoteLockUnhealthy(_) => {
            RepositoryJobError::CloudUnavailable
        }
        RepoError::OperationAndUnlockFailed { operation, .. } => map_repo_error(*operation),
        _ => RepositoryJobError::RepositoryUnavailable,
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

fn timestamp(value: time::OffsetDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        value.year(),
        u8::from(value.month()),
        value.day(),
        value.hour(),
        value.minute(),
        value.second()
    )
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::path::{Path, PathBuf};
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};

    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use qingyu_dejavu::{
        Cloud, CloudError, LocalCloud, RepoError, WorkingTreeChange, WorkingTreeCoordinator,
        WorkingTreePermit,
    };
    use tempfile::{tempdir, TempDir};

    use super::{
        map_repo_error, DejavuRepositoryRunner, RepositoryCloudFactory, RepositoryCloudParameters,
        WorkingTreeCoordinatorFactory,
    };
    use crate::dejavu_sync::service::{
        JobCancellationToken, RepositoryJobError, RepositoryJobRunner, SyncAttemptContext,
        SyncJobRequest,
    };
    use crate::sync_config::status::SyncTrigger;

    #[test]
    fn only_typed_cloud_dns_errors_reach_the_scheduler_dns_category() {
        assert_eq!(
            map_repo_error(RepoError::Cloud(CloudError::Dns)),
            RepositoryJobError::DnsUnavailable
        );
        for error in [
            RepoError::Cloud(CloudError::Unavailable),
            RepoError::Cloud(CloudError::Auth),
            RepoError::Cloud(CloudError::Forbidden),
            RepoError::Cloud(CloudError::UnsafeKey),
            RepoError::UnsafePath,
        ] {
            assert_ne!(map_repo_error(error), RepositoryJobError::DnsUnavailable);
        }
        assert_eq!(
            map_repo_error(RepoError::OperationAndUnlockFailed {
                operation: Box::new(RepoError::Cloud(CloudError::Dns)),
                unlock: CloudError::Forbidden,
            }),
            RepositoryJobError::DnsUnavailable
        );
    }

    #[test]
    fn dns_during_unlock_never_reclassifies_the_primary_operation() {
        for (operation, expected) in [
            (
                RepoError::UnsafePath,
                RepositoryJobError::RepositoryUnavailable,
            ),
            (
                RepoError::DecryptionFailed,
                RepositoryJobError::RepositoryUnavailable,
            ),
            (
                RepoError::InvalidData("fixture integrity failure"),
                RepositoryJobError::RepositoryUnavailable,
            ),
            (
                RepoError::Cloud(CloudError::Auth),
                RepositoryJobError::CloudUnavailable,
            ),
            (
                RepoError::Cloud(CloudError::Forbidden),
                RepositoryJobError::CloudUnavailable,
            ),
            (
                RepoError::Cloud(CloudError::Locked),
                RepositoryJobError::CloudUnavailable,
            ),
        ] {
            assert_eq!(
                map_repo_error(RepoError::OperationAndUnlockFailed {
                    operation: Box::new(operation),
                    unlock: CloudError::Dns,
                }),
                expected
            );
        }
    }

    struct FakeCoordinator;

    struct FakeCoordinatorFactory;

    impl WorkingTreeCoordinatorFactory for FakeCoordinatorFactory {
        fn create(
            &self,
            _context: &SyncAttemptContext,
        ) -> Result<Arc<dyn WorkingTreeCoordinator>, RepositoryJobError> {
            Ok(Arc::new(FakeCoordinator))
        }
    }

    impl WorkingTreeCoordinator for FakeCoordinator {
        fn prepare<'life0, 'life1, 'async_trait>(
            &'life0 self,
            _changes: &'life1 [WorkingTreeChange],
        ) -> Pin<
            Box<
                dyn Future<Output = Result<WorkingTreePermit, qingyu_dejavu::RepoError>>
                    + Send
                    + 'async_trait,
            >,
        >
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async { Ok(WorkingTreePermit::new(())) })
        }

        fn release<'life0, 'async_trait>(
            &'life0 self,
            _permit: WorkingTreePermit,
        ) -> Pin<Box<dyn Future<Output = ()> + Send + 'async_trait>>
        where
            'life0: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async {})
        }
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct SafeObservedCloudParameters {
        endpoint_url: String,
        region: String,
        bucket: String,
        request_timeout_seconds: u32,
        addressing_style: String,
        tls_verification: String,
        repository_prefix: String,
    }

    struct LocalCloudFactory {
        cloud_root: PathBuf,
        observed: Mutex<Vec<SafeObservedCloudParameters>>,
    }

    impl LocalCloudFactory {
        fn new(cloud_root: PathBuf) -> Self {
            Self {
                cloud_root,
                observed: Mutex::new(Vec::new()),
            }
        }
    }

    impl RepositoryCloudFactory for LocalCloudFactory {
        fn create(
            &self,
            parameters: RepositoryCloudParameters,
        ) -> Result<Arc<dyn Cloud>, RepositoryJobError> {
            self.observed
                .lock()
                .unwrap()
                .push(SafeObservedCloudParameters {
                    endpoint_url: parameters.endpoint_url,
                    region: parameters.region,
                    bucket: parameters.bucket,
                    request_timeout_seconds: parameters.request_timeout_seconds,
                    addressing_style: parameters.addressing_style.to_owned(),
                    tls_verification: parameters.tls_verification.to_owned(),
                    repository_prefix: parameters.repository_prefix,
                });
            Ok(Arc::new(
                LocalCloud::new(&self.cloud_root).expect("local cloud fixture"),
            ))
        }
    }

    struct Fixture {
        _root: TempDir,
        app_data: PathBuf,
        notes_root: PathBuf,
        cloud_root: PathBuf,
        repository_id: String,
    }

    impl Fixture {
        fn new() -> Self {
            let root = tempdir().unwrap();
            let app_data = root.path().join("app-data");
            let notes_root = root.path().join("notes");
            let cloud_root = root.path().join("cloud");
            std::fs::create_dir(&app_data).unwrap();
            std::fs::create_dir(&notes_root).unwrap();
            std::fs::create_dir(&cloud_root).unwrap();
            std::fs::write(notes_root.join("journal.md"), b"first note\n").unwrap();
            let repository_id = "00000000-0000-4000-8000-000000000020".to_owned();
            let canonical_notes_root = notes_root.canonicalize().unwrap();
            std::fs::write(
                app_data.join("sync-config.json"),
                serde_json::to_vec_pretty(&serde_json::json!({
                    "version": 3,
                    "enabled": true,
                    "provider": "s3",
                    "remoteRoot": "ignored-by-dejavu-layout",
                    "mode": "automatic",
                    "intervalSeconds": 30,
                    "webdav": {
                        "serverUrl": "",
                        "username": "",
                        "password": ""
                    },
                    "s3": {
                        "endpointUrl": "https://objects.example.test/base",
                        "region": "",
                        "bucket": "qingyu-notes",
                        "accessKeyId": "private-access-key",
                        "secretAccessKey": "private-secret-key",
                        "requestTimeoutSeconds": 47,
                        "addressingStyle": "path",
                        "tlsVerification": "skip"
                    }
                }))
                .unwrap(),
            )
            .unwrap();
            std::fs::write(
                app_data.join("local-sync.json"),
                serde_json::to_vec_pretty(&serde_json::json!({
                    "version": 1,
                    "deviceId": "10000000-0000-4000-8000-000000000020",
                    "repoKey": STANDARD.encode([7_u8; 32]),
                    "bindings": [{
                        "repositoryId": repository_id,
                        "displayName": "Journal",
                        "notesRoot": canonical_notes_root,
                        "enabled": true
                    }]
                }))
                .unwrap(),
            )
            .unwrap();
            Self {
                _root: root,
                app_data,
                notes_root,
                cloud_root,
                repository_id,
            }
        }

        fn request(&self) -> SyncJobRequest {
            SyncJobRequest {
                notes_root: self.notes_root.clone(),
                repository_id: self.repository_id.clone(),
                trigger: SyncTrigger::Manual,
            }
        }
    }

    fn runner(fixture: &Fixture) -> (DejavuRepositoryRunner, Arc<LocalCloudFactory>) {
        let factory = Arc::new(LocalCloudFactory::new(fixture.cloud_root.clone()));
        (
            DejavuRepositoryRunner::with_cloud_factory(
                &fixture.app_data,
                Arc::new(FakeCoordinatorFactory),
                Arc::clone(&factory),
            ),
            factory,
        )
    }

    fn index_count(root: &Path) -> usize {
        std::fs::read_dir(root)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
            .count()
    }

    #[tokio::test]
    async fn bind_and_sync_uses_exact_layout_config_semantics_and_one_core_sync_index() {
        let fixture = Fixture::new();
        let (runner, factory) = runner(&fixture);
        let request = runner.validate(fixture.request()).unwrap();

        let result = runner
            .run_attempt(SyncAttemptContext {
                request,
                job_id: "20000000-0000-4000-8000-000000000020".to_owned(),
                attempt: 1,
                cancellation: JobCancellationToken::new(),
            })
            .await
            .unwrap();

        assert_eq!(
            std::fs::read(fixture.notes_root.join(".qingyu/syncignore")).unwrap(),
            b""
        );
        let repository_root = fixture
            .app_data
            .join(format!("sync/repositories/{}", fixture.repository_id));
        assert!(repository_root.join("repo").is_dir());
        assert!(repository_root.join("history").is_dir());
        assert!(repository_root.join("temp").is_dir());
        assert_eq!(index_count(&repository_root.join("repo/indexes")), 1);
        assert!(fixture.cloud_root.join("refs/latest").is_file());
        assert!(result.conflicts.is_empty());
        assert!(result.transfer.upload_files >= 2);
        assert_eq!(
            factory.observed.lock().unwrap().as_slice(),
            &[SafeObservedCloudParameters {
                endpoint_url: "https://objects.example.test/base".to_owned(),
                region: String::new(),
                bucket: "qingyu-notes".to_owned(),
                request_timeout_seconds: 47,
                addressing_style: "path".to_owned(),
                tls_verification: "skip".to_owned(),
                repository_prefix: format!("qingyu/repositories/{}/repo", fixture.repository_id),
            }]
        );
    }

    #[tokio::test]
    async fn bind_and_sync_preserves_an_existing_user_syncignore_file() {
        let fixture = Fixture::new();
        std::fs::create_dir(fixture.notes_root.join(".qingyu")).unwrap();
        std::fs::write(
            fixture.notes_root.join(".qingyu/syncignore"),
            b"drafts/**\n",
        )
        .unwrap();
        let (runner, _) = runner(&fixture);
        let request = runner.validate(fixture.request()).unwrap();

        runner
            .run_attempt(SyncAttemptContext {
                request,
                job_id: "20000000-0000-4000-8000-000000000021".to_owned(),
                attempt: 1,
                cancellation: JobCancellationToken::new(),
            })
            .await
            .unwrap();

        assert_eq!(
            std::fs::read(fixture.notes_root.join(".qingyu/syncignore")).unwrap(),
            b"drafts/**\n"
        );
    }

    #[test]
    fn binding_validation_rejects_traversal_ids_before_repository_path_join() {
        let fixture = Fixture::new();
        let (runner, _) = runner(&fixture);
        let mut request = fixture.request();
        request.repository_id = "../../outside".to_owned();

        let Err(error) = runner.validate(request) else {
            panic!("traversal binding must be rejected");
        };

        assert_eq!(error, RepositoryJobError::InvalidBinding);
        assert!(!fixture.app_data.join("outside").exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn bind_and_sync_rejects_a_symlink_syncignore_without_overwriting_its_target() {
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new();
        let outside = fixture._root.path().join("outside-ignore");
        std::fs::write(&outside, b"keep me").unwrap();
        std::fs::create_dir(fixture.notes_root.join(".qingyu")).unwrap();
        symlink(&outside, fixture.notes_root.join(".qingyu/syncignore")).unwrap();
        let (runner, _) = runner(&fixture);
        let request = runner.validate(fixture.request()).unwrap();

        let Err(error) = runner
            .run_attempt(SyncAttemptContext {
                request,
                job_id: "20000000-0000-4000-8000-000000000022".to_owned(),
                attempt: 1,
                cancellation: JobCancellationToken::new(),
            })
            .await
        else {
            panic!("symlink syncignore must be rejected");
        };

        assert_eq!(error, RepositoryJobError::RepositoryUnavailable);
        assert_eq!(std::fs::read(outside).unwrap(), b"keep me");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn bind_and_sync_rejects_an_ordinary_repo_directory_replaced_before_core_open() {
        let fixture = Fixture::new();
        let (runner, _) = runner(&fixture);
        let request = runner.validate(fixture.request()).unwrap();
        let repository_root = fixture
            .app_data
            .canonicalize()
            .unwrap()
            .join(format!("sync/repositories/{}", fixture.repository_id));
        let moved_repo = repository_root.join("moved-repo");

        let result = runner
            .bind_and_sync_with_layout_observer(
                SyncAttemptContext {
                    request,
                    job_id: "20000000-0000-4000-8000-000000000023".to_owned(),
                    attempt: 1,
                    cancellation: JobCancellationToken::new(),
                },
                |prepared_root| {
                    assert_eq!(prepared_root, repository_root);
                    std::fs::rename(repository_root.join("repo"), &moved_repo).unwrap();
                    std::fs::create_dir(repository_root.join("repo")).unwrap();
                },
            )
            .await;

        let Err(error) = result else {
            panic!("ambient replacement must not be accepted by the core open");
        };
        assert_eq!(error, RepositoryJobError::RepositoryUnavailable);
        assert!(moved_repo.is_dir());
        assert!(repository_root.join("repo").is_dir());
    }

    #[test]
    fn cloud_parameter_debug_redacts_credentials_and_endpoint_details() {
        let parameters = RepositoryCloudParameters {
            endpoint_url: "https://embedded:password@private.example.test/base?token=signed"
                .to_owned(),
            region: "private-region".to_owned(),
            bucket: "private-bucket".to_owned(),
            access_key_id: "private-access-key".to_owned(),
            secret_access_key: "private-secret-key".to_owned(),
            request_timeout_seconds: 47,
            addressing_style: "path".to_owned(),
            tls_verification: "skip".to_owned(),
            repository_prefix: "qingyu/repositories/00000000-0000-4000-8000-000000000020/repo"
                .to_owned(),
        };

        let debug = format!("{parameters:?}");

        for secret in [
            "embedded",
            "password",
            "private.example.test",
            "signed",
            "private-access-key",
            "private-secret-key",
        ] {
            assert!(!debug.contains(secret));
        }
        assert!(debug.contains("[REDACTED]"));
    }
}
