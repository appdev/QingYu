// DejaVu - Data snapshot and sync.
// Copyright (c) 2022-present, b3log.org
// SPDX-License-Identifier: AGPL-3.0-only

use std::env;
use std::fs;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use filetime::FileTime;
use futures_util::FutureExt;
use qingyu_dejavu::{
    Cloud, CloudError, CloudObject, CloudOperation, CloudUploadSource, Device, MergeResult,
    NoopWorkingTreeCoordinator, Repo, RepoError, RepoOptions, RepoPaths, S3AddressingStyle,
    S3Cloud, S3Connection, S3RepositoryCatalog, S3TlsVerification, S3TransportOptions,
};
use tempfile::TempDir;
use uuid::Uuid;

const LIVE_TEST_FLAG: &str = "QINGYU_S3_LIVE_TESTS";
const SCENARIO_TIMEOUT: Duration = Duration::from_secs(120);
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(60);
const LOCK_CONTENTION_TIMEOUT: Duration = Duration::from_secs(20);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const TEST_KEY: [u8; 32] = [0x51; 32];
const MAX_LATEST_REF_BYTES: u64 = 1024 * 1024;

struct FailBeforeLatestCloud {
    inner: Arc<dyn Cloud>,
    failed: AtomicBool,
}

impl FailBeforeLatestCloud {
    fn new(inner: Arc<dyn Cloud>) -> Self {
        Self {
            inner,
            failed: AtomicBool::new(false),
        }
    }

    fn reject_latest_once(&self, key: &str) -> Result<(), CloudError> {
        if key == "refs/latest" && !self.failed.swap(true, Ordering::SeqCst) {
            return Err(CloudError::Injected(CloudOperation::Put));
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl Cloud for FailBeforeLatestCloud {
    async fn get_bounded(&self, key: &str, max_bytes: u64) -> Result<Vec<u8>, CloudError> {
        self.inner.get_bounded(key, max_bytes).await
    }

    async fn download_to(
        &self,
        key: &str,
        destination: &mut (dyn tokio::io::AsyncWrite + Unpin + Send),
    ) -> Result<u64, CloudError> {
        self.inner.download_to(key, destination).await
    }

    async fn put(&self, key: &str, bytes: &[u8], overwrite: bool) -> Result<u64, CloudError> {
        self.reject_latest_once(key)?;
        self.inner.put(key, bytes, overwrite).await
    }

    async fn upload_from(
        &self,
        key: &str,
        source: &dyn CloudUploadSource,
        overwrite: bool,
    ) -> Result<u64, CloudError> {
        self.reject_latest_once(key)?;
        self.inner.upload_from(key, source, overwrite).await
    }

    async fn remove(&self, key: &str) -> Result<(), CloudError> {
        self.inner.remove(key).await
    }

    async fn list(&self, prefix: &str) -> Result<Vec<CloudObject>, CloudError> {
        self.inner.list(prefix).await
    }

    async fn available_size(&self) -> Result<u64, CloudError> {
        self.inner.available_size().await
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dejavu_s3_sync_round_trips_through_real_minio() -> Result<(), LiveFailure> {
    if env::var(LIVE_TEST_FLAG).as_deref() != Ok("1") {
        println!("SKIPPED qingyu-dejavu real MinIO test: set {LIVE_TEST_FLAG}=1 to enable");
        return Ok(());
    }

    let config = LiveConfig::from_env()?;
    let repository_id = random_repository_id()?;
    let catalog = S3RepositoryCatalog::new(config.connection.clone(), config.options)
        .map_err(|error| LiveFailure::cloud("catalog_open", &error))?;

    let body = AssertUnwindSafe(async {
        tokio::time::timeout(
            SCENARIO_TIMEOUT,
            run_live_scenario(&config, &catalog, &repository_id),
        )
        .await
        .map_err(|_| LiveFailure::new("scenario", "timeout"))?
    })
    .catch_unwind()
    .await;
    let cleanup = tokio::time::timeout(
        CLEANUP_TIMEOUT,
        cleanup_exact_repository(&catalog, &repository_id),
    )
    .await
    .map_err(|_| LiveFailure::new("cleanup", "timeout"))
    .and_then(|result| result);

    match body {
        Ok(body) => match (body, cleanup) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) => Err(error),
            (Ok(()), Err(error)) => Err(error),
            (Err(error), Err(cleanup_error)) => Err(error.with_cleanup(&cleanup_error)),
        },
        Err(panic) => {
            if let Err(cleanup_error) = cleanup {
                eprintln!(
                    "qingyu-dejavu live MinIO panic cleanup failed with safe code {}",
                    cleanup_error.code
                );
            }
            std::panic::resume_unwind(panic)
        }
    }
}

#[derive(Debug)]
struct LiveFailure {
    stage: &'static str,
    code: &'static str,
    cleanup_code: Option<&'static str>,
}

impl LiveFailure {
    const fn new(stage: &'static str, code: &'static str) -> Self {
        Self {
            stage,
            code,
            cleanup_code: None,
        }
    }

    fn cloud(stage: &'static str, error: &CloudError) -> Self {
        Self::new(stage, error.code())
    }

    fn repo(stage: &'static str, error: &RepoError) -> Self {
        let code = match error {
            RepoError::Cloud(error) => error.code(),
            RepoError::Io(_) => "io",
            RepoError::Serialization(_) => "serialization",
            RepoError::Compression(_) => "compression",
            RepoError::DecodedSizeLimitExceeded { .. } => "decoded_size_limit",
            RepoError::KeyDerivationFailed => "key_derivation",
            RepoError::EncryptionFailed => "encryption",
            RepoError::DecryptionFailed => "decryption",
            RepoError::RandomnessUnavailable => "randomness",
            RepoError::InvalidData(_) => "invalid_data",
            RepoError::NotFound(_) => "not_found",
            RepoError::Cancelled => "cancelled",
            RepoError::RepositoryBusy => "repository_busy",
            RepoError::EmptyIndex => "empty_index",
            RepoError::IndexFileChanged => "index_file_changed",
            RepoError::WorkingTreeChanged => "working_tree_changed",
            RepoError::RemoteLockUnhealthy(error) => error.code(),
            RepoError::OperationAndUnlockFailed { .. } => "operation_and_unlock_failed",
            RepoError::RepoFatal => "repo_fatal",
            RepoError::UnsafePath => "unsafe_path",
        };
        Self::new(stage, code)
    }

    fn with_cleanup(mut self, cleanup: &Self) -> Self {
        self.cleanup_code = Some(cleanup.code);
        self
    }
}

impl std::fmt::Display for LiveFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "live MinIO stage {} failed with safe code {}",
            self.stage, self.code
        )?;
        if let Some(cleanup_code) = self.cleanup_code {
            write!(
                formatter,
                "; exact-prefix cleanup also failed with safe code {cleanup_code}"
            )?;
        }
        Ok(())
    }
}

impl std::error::Error for LiveFailure {}

struct LiveConfig {
    connection: S3Connection,
    options: S3TransportOptions,
}

impl LiveConfig {
    fn from_env() -> Result<Self, LiveFailure> {
        let endpoint = required_environment("MARKRA_TEST_S3_ENDPOINT")?;
        let bucket = required_environment("MARKRA_TEST_S3_BUCKET")?;
        let access_key_id = required_environment("MARKRA_TEST_S3_ACCESS_KEY_ID")?;
        let secret_access_key = required_environment("MARKRA_TEST_S3_SECRET_ACCESS_KEY")?;
        let region = optional_environment("MARKRA_TEST_S3_REGION")
            .unwrap_or_else(|| "us-east-1".to_string());
        let connection = S3Connection::new(
            &endpoint,
            &region,
            &bucket,
            &access_key_id,
            &secret_access_key,
            S3AddressingStyle::Auto,
        )
        .map_err(|error| LiveFailure::cloud("connection", &error))?;
        Ok(Self {
            connection,
            options: S3TransportOptions {
                request_timeout: REQUEST_TIMEOUT,
                tls_verification: S3TlsVerification::Verify,
                max_attempts: 3,
            },
        })
    }

    fn cloud(&self, repository_id: &str) -> Result<Arc<dyn Cloud>, LiveFailure> {
        let prefix = format!("qingyu/repositories/{repository_id}/repo");
        let cloud = S3Cloud::new(self.connection.clone(), self.options, &prefix)
            .map_err(|error| LiveFailure::cloud("cloud_open", &error))?;
        Ok(Arc::new(cloud))
    }
}

struct ClientFixture {
    _root: TempDir,
    data: PathBuf,
    history: PathBuf,
    repo: Repo,
}

impl ClientFixture {
    fn open(device_id: &str) -> Result<Self, LiveFailure> {
        let root = TempDir::new().map_err(|_| LiveFailure::new("local_setup", "io"))?;
        let data = root.path().join("data");
        let history = root.path().join("history");
        fs::create_dir_all(&data).map_err(|_| LiveFailure::new("local_setup", "io"))?;
        let repo = Repo::open(
            RepoPaths {
                data: data.clone(),
                repo: root.path().join("repo"),
                history: history.clone(),
                temp: root.path().join("temp"),
            },
            Device {
                id: device_id.to_string(),
                name: device_id.to_string(),
                os: "live-minio-test".to_string(),
            },
            TEST_KEY,
            RepoOptions::default(),
        )
        .map_err(|error| LiveFailure::repo("repo_open", &error))?;
        Ok(Self {
            _root: root,
            data,
            history,
            repo,
        })
    }
}

async fn run_live_scenario(
    config: &LiveConfig,
    catalog: &S3RepositoryCatalog,
    repository_id: &str,
) -> Result<(), LiveFailure> {
    let timestamp = unix_millis()?;
    let metadata = catalog
        .create(repository_id, "QingYu Dejavu Rust MinIO", timestamp)
        .await
        .map_err(|error| LiveFailure::cloud("catalog_create", &error))?;
    require(
        "catalog_create",
        metadata.repository_id == repository_id && metadata.format_version == 1,
    )?;

    let listed = catalog
        .list()
        .await
        .map_err(|error| LiveFailure::cloud("catalog_list", &error))?;
    require(
        "catalog_list",
        listed
            .entries
            .iter()
            .any(|entry| entry.repository_id == repository_id),
    )?;
    let read = catalog
        .read(repository_id)
        .await
        .map_err(|error| LiveFailure::cloud("catalog_read", &error))?;
    require("catalog_read", read == metadata)?;

    let cloud = config.cloud(repository_id)?;
    let client_a = ClientFixture::open("qingyu-live-a")?;
    let client_b = ClientFixture::open("qingyu-live-b")?;
    let base_seconds = unix_seconds()?
        .checked_sub(300)
        .ok_or_else(|| LiveFailure::new("clock", "out_of_range"))?;

    write_file_at(
        &client_a.data,
        "base.md",
        b"first upload from A",
        base_seconds,
    )?;
    write_file_at(&client_a.data, "same.md", b"shared baseline", base_seconds)?;
    let first_upload = sync_repo("a_first_upload", &client_a.repo, Arc::clone(&cloud)).await?;
    require(
        "a_first_upload",
        first_upload.conflicts.is_empty() && client_a.repo.latest_sync().is_ok_and(|v| v.is_some()),
    )?;

    let first_download = sync_repo("b_first_download", &client_b.repo, Arc::clone(&cloud)).await?;
    require(
        "b_first_download",
        first_download.conflicts.is_empty()
            && read_file(&client_b.data, "base.md")? == b"first upload from A"
            && read_file(&client_b.data, "same.md")? == b"shared baseline",
    )?;

    write_file_at(
        &client_a.data,
        "from-a.md",
        b"independent A",
        base_seconds + 10,
    )?;
    write_file_at(
        &client_b.data,
        "from-b.md",
        b"independent B",
        base_seconds + 20,
    )?;
    sync_repo("a_independent_upload", &client_a.repo, Arc::clone(&cloud)).await?;
    let b_merge = sync_repo("b_independent_merge", &client_b.repo, Arc::clone(&cloud)).await?;
    require(
        "b_independent_merge",
        b_merge.conflicts.is_empty()
            && merge_contains_path(&b_merge, "/from-a.md")
            && read_file(&client_b.data, "from-a.md")? == b"independent A"
            && read_file(&client_b.data, "from-b.md")? == b"independent B",
    )?;
    let a_observe = sync_repo("a_observes_b", &client_a.repo, Arc::clone(&cloud)).await?;
    require(
        "a_observes_b",
        a_observe.conflicts.is_empty()
            && merge_contains_path(&a_observe, "/from-b.md")
            && read_file(&client_a.data, "from-b.md")? == b"independent B",
    )?;

    fs::remove_file(client_a.data.join("from-a.md"))
        .map_err(|_| LiveFailure::new("a_delete_local", "io"))?;
    let a_delete = sync_repo("a_delete_upload", &client_a.repo, Arc::clone(&cloud)).await?;
    require(
        "a_delete_upload",
        a_delete.conflicts.is_empty() && !client_a.data.join("from-a.md").exists(),
    )?;
    let b_delete = sync_repo("b_delete_download", &client_b.repo, Arc::clone(&cloud)).await?;
    require(
        "b_delete_download",
        b_delete.conflicts.is_empty()
            && merge_removes_path(&b_delete, "/from-a.md")
            && !client_b.data.join("from-a.md").exists(),
    )?;

    write_file_at(
        &client_a.data,
        "retry.md",
        b"survives interrupted ref publication",
        base_seconds + 25,
    )?;
    let latest_before = cloud
        .get_bounded("refs/latest", MAX_LATEST_REF_BYTES)
        .await
        .map_err(|error| LiveFailure::cloud("retry_latest_before", &error))?;
    let fail_before_latest: Arc<dyn Cloud> =
        Arc::new(FailBeforeLatestCloud::new(Arc::clone(&cloud)));
    let failed_publication = sync_repo(
        "a_interrupted_ref_publication",
        &client_a.repo,
        fail_before_latest,
    )
    .await;
    require(
        "a_interrupted_ref_publication",
        failed_publication.is_err_and(|error| error.code == "injected"),
    )?;
    let latest_after_failure = cloud
        .get_bounded("refs/latest", MAX_LATEST_REF_BYTES)
        .await
        .map_err(|error| LiveFailure::cloud("retry_latest_after_failure", &error))?;
    require(
        "a_interrupted_ref_publication",
        latest_after_failure == latest_before,
    )?;
    let retry = sync_repo(
        "a_ref_publication_retry",
        &client_a.repo,
        Arc::clone(&cloud),
    )
    .await?;
    require(
        "a_ref_publication_retry",
        retry.conflicts.is_empty() && client_a.repo.latest_sync().is_ok_and(|v| v.is_some()),
    )?;
    let b_retry = sync_repo(
        "b_observes_publication_retry",
        &client_b.repo,
        Arc::clone(&cloud),
    )
    .await?;
    require(
        "b_observes_publication_retry",
        b_retry.conflicts.is_empty()
            && read_file(&client_b.data, "retry.md")? == b"survives interrupted ref publication",
    )?;

    write_file_at(
        &client_b.data,
        "same.md",
        b"B local conflict version",
        base_seconds + 30,
    )?;
    write_file_at(
        &client_a.data,
        "same.md",
        b"A remote conflict version",
        base_seconds + 40,
    )?;
    sync_repo("a_conflict_upload", &client_a.repo, Arc::clone(&cloud)).await?;
    let conflict = sync_repo("b_conflict_merge", &client_b.repo, Arc::clone(&cloud)).await?;
    require(
        "b_conflict_merge",
        conflict.conflicts.len() == 1
            && conflict.conflicts[0].path == "/same.md"
            && read_file(&client_b.data, "same.md")? == b"B local conflict version",
    )?;
    let history_versions = history_versions(&client_b.history, "same.md")?;
    require(
        "b_conflict_history",
        history_versions == [b"A remote conflict version".to_vec()],
    )?;
    let conflict_convergence =
        sync_repo("a_conflict_convergence", &client_a.repo, Arc::clone(&cloud)).await?;
    require(
        "a_conflict_convergence",
        conflict_convergence.conflicts.is_empty()
            && read_file(&client_a.data, "same.md")? == b"B local conflict version",
    )?;

    exercise_lock_contention(&client_a.repo, &client_b.repo, Arc::clone(&cloud)).await?;
    let final_metadata = catalog
        .read(repository_id)
        .await
        .map_err(|error| LiveFailure::cloud("catalog_final_read", &error))?;
    require("catalog_final_read", final_metadata == metadata)
}

async fn sync_repo(
    stage: &'static str,
    repo: &Repo,
    cloud: Arc<dyn Cloud>,
) -> Result<MergeResult, LiveFailure> {
    repo.sync(cloud, Arc::new(NoopWorkingTreeCoordinator))
        .await
        .map(|(result, _traffic)| result)
        .map_err(|error| LiveFailure::repo(stage, &error))
}

async fn exercise_lock_contention(
    client_a: &Repo,
    client_b: &Repo,
    cloud: Arc<dyn Cloud>,
) -> Result<(), LiveFailure> {
    let guard_a = client_a
        .lock_cloud(Arc::clone(&cloud))
        .await
        .map_err(|error| LiveFailure::cloud("lock_a", &error))?;
    let contention = tokio::time::timeout(
        LOCK_CONTENTION_TIMEOUT,
        client_b.lock_cloud(Arc::clone(&cloud)),
    )
    .await;

    let contention_result = match contention {
        Ok(Err(CloudError::Locked)) => Ok(()),
        Ok(Err(error)) => Err(LiveFailure::cloud("lock_contention", &error)),
        Ok(Ok(guard_b)) => {
            let _release = guard_b.release().await;
            Err(LiveFailure::new(
                "lock_contention",
                "unexpected_lock_acquisition",
            ))
        }
        Err(_) => Err(LiveFailure::new("lock_contention", "timeout")),
    };
    let release_a = guard_a
        .release()
        .await
        .map_err(|error| LiveFailure::cloud("unlock_a", &error));
    contention_result?;
    release_a?;

    let guard_b = client_b
        .lock_cloud(cloud)
        .await
        .map_err(|error| LiveFailure::cloud("lock_b_after_release", &error))?;
    guard_b
        .release()
        .await
        .map_err(|error| LiveFailure::cloud("unlock_b", &error))
}

async fn cleanup_exact_repository(
    catalog: &S3RepositoryCatalog,
    repository_id: &str,
) -> Result<(), LiveFailure> {
    catalog
        .delete_repository(repository_id)
        .await
        .map_err(|error| LiveFailure::cloud("cleanup_delete", &error))?;
    match catalog.read(repository_id).await {
        Err(CloudError::NotFound) => Ok(()),
        Ok(_) => Err(LiveFailure::new("cleanup_verify", "metadata_remained")),
        Err(error) => Err(LiveFailure::cloud("cleanup_verify", &error)),
    }
}

fn required_environment(name: &'static str) -> Result<String, LiveFailure> {
    optional_environment(name).ok_or_else(|| LiveFailure::new(name, "missing_environment"))
}

fn optional_environment(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn random_repository_id() -> Result<String, LiveFailure> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| LiveFailure::new("repository_id", "randomness"))?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(Uuid::from_bytes(bytes).to_string())
}

fn unix_seconds() -> Result<i64, LiveFailure> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| LiveFailure::new("clock", "before_unix_epoch"))?
        .as_secs();
    i64::try_from(seconds).map_err(|_| LiveFailure::new("clock", "out_of_range"))
}

fn unix_millis() -> Result<i64, LiveFailure> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| LiveFailure::new("clock", "before_unix_epoch"))?
        .as_millis();
    i64::try_from(millis).map_err(|_| LiveFailure::new("clock", "out_of_range"))
}

fn write_file_at(
    root: &Path,
    relative_path: &str,
    bytes: &[u8],
    updated_seconds: i64,
) -> Result<(), LiveFailure> {
    let path = root.join(relative_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|_| LiveFailure::new("local_write", "io"))?;
    }
    fs::write(&path, bytes).map_err(|_| LiveFailure::new("local_write", "io"))?;
    filetime::set_file_mtime(&path, FileTime::from_unix_time(updated_seconds, 0))
        .map_err(|_| LiveFailure::new("local_mtime", "io"))
}

fn read_file(root: &Path, relative_path: &str) -> Result<Vec<u8>, LiveFailure> {
    fs::read(root.join(relative_path)).map_err(|_| LiveFailure::new("local_read", "io"))
}

fn history_versions(history_root: &Path, relative_path: &str) -> Result<Vec<Vec<u8>>, LiveFailure> {
    let entries = fs::read_dir(history_root).map_err(|_| LiveFailure::new("history", "io"))?;
    let mut versions = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_| LiveFailure::new("history", "io"))?;
        let candidate = entry.path().join(relative_path);
        if candidate.is_file() {
            versions.push(fs::read(candidate).map_err(|_| LiveFailure::new("history", "io"))?);
        }
    }
    Ok(versions)
}

fn merge_contains_path(result: &MergeResult, path: &str) -> bool {
    result.upserts.iter().any(|file| file.path == path)
}

fn merge_removes_path(result: &MergeResult, path: &str) -> bool {
    result.removes.iter().any(|file| file.path == path)
}

fn require(stage: &'static str, condition: bool) -> Result<(), LiveFailure> {
    if condition {
        Ok(())
    } else {
        Err(LiveFailure::new(stage, "assertion_failed"))
    }
}
