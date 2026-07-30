use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use qingyu_dejavu::{
    Device, LocalCloud, NoopWorkingTreeCoordinator, RepoOptions, RepositoryRuntimeState,
};
use qingyu_kernel::sync::dejavu_runner::{
    DejavuConflictResolution, DejavuInstanceRoots, DejavuRepositoryKey, DejavuRunError,
    DejavuRunnerInputs, DejavuS3Config, DejavuSecret, DejavuWorkspaceCapability,
    DejavuWorkspaceCapabilityError, KernelDejavuRunner,
};
use tempfile::TempDir;

const REPOSITORY_ID: &str = "323df833-764a-44b3-a534-492640c258f2";

struct TestWorkspaceCapability {
    canonical_path: PathBuf,
}

impl TestWorkspaceCapability {
    fn new(path: &Path) -> Self {
        Self {
            canonical_path: path.canonicalize().expect("canonical test workspace"),
        }
    }
}

impl DejavuWorkspaceCapability for TestWorkspaceCapability {
    fn verify_held_directory(&self) -> Result<(), DejavuWorkspaceCapabilityError> {
        let current = self
            .canonical_path
            .canonicalize()
            .map_err(|_| DejavuWorkspaceCapabilityError)?;
        if current == self.canonical_path {
            Ok(())
        } else {
            Err(DejavuWorkspaceCapabilityError)
        }
    }

    fn canonical_path(&self) -> &Path {
        &self.canonical_path
    }
}

struct FailingWorkspaceCapability {
    canonical_path: PathBuf,
    verification_count: AtomicUsize,
    fail_on: usize,
}

impl FailingWorkspaceCapability {
    fn new(path: &Path, fail_on: usize) -> Self {
        Self {
            canonical_path: path.canonicalize().expect("canonical test workspace"),
            verification_count: AtomicUsize::new(0),
            fail_on,
        }
    }
}

impl DejavuWorkspaceCapability for FailingWorkspaceCapability {
    fn verify_held_directory(&self) -> Result<(), DejavuWorkspaceCapabilityError> {
        let current = self.verification_count.fetch_add(1, Ordering::SeqCst) + 1;
        if current == self.fail_on {
            Err(DejavuWorkspaceCapabilityError)
        } else {
            Ok(())
        }
    }

    fn canonical_path(&self) -> &Path {
        &self.canonical_path
    }
}

struct RunnerFixture {
    _root: TempDir,
    workspace: PathBuf,
    runner: KernelDejavuRunner,
}

fn runner_inputs(root: &Path, name: &str) -> (PathBuf, DejavuRunnerInputs) {
    let workspace = root.join("workspace");
    fs::create_dir(&workspace).expect("workspace");
    let instance = root.join("instance");
    fs::create_dir(&instance).expect("instance root");
    let inputs = DejavuRunnerInputs {
        workspace: Arc::new(TestWorkspaceCapability::new(&workspace)),
        roots: DejavuInstanceRoots::new(
            instance.join("state"),
            instance.join("history"),
            instance.join("temp"),
        ),
        repository_id: REPOSITORY_ID.to_owned(),
        device: Device {
            id: name.to_owned(),
            name: name.to_owned(),
            os: "test".to_owned(),
        },
        options: RepoOptions::default(),
        repository_key: DejavuRepositoryKey::new([7; 32]),
        runtime: RepositoryRuntimeState::default(),
        coordinator: Arc::new(NoopWorkingTreeCoordinator),
    };
    (workspace, inputs)
}

impl RunnerFixture {
    fn new(name: &str, cloud: Arc<LocalCloud>) -> Self {
        let root = tempfile::tempdir().expect("fixture root");
        let (workspace, inputs) = runner_inputs(root.path(), name);
        let runner =
            KernelDejavuRunner::new_with_cloud(inputs, cloud).expect("valid local-cloud runner");
        Self {
            _root: root,
            workspace,
            runner,
        }
    }
}

#[test]
fn s3_constructor_builds_a_runner_without_reading_ambient_configuration() {
    let root = tempfile::tempdir().expect("fixture root");
    let (_workspace, inputs) = runner_inputs(root.path(), "s3");
    let config = DejavuS3Config {
        endpoint_url: "http://127.0.0.1:9000".to_owned(),
        region: "us-east-1".to_owned(),
        bucket: "qingyu".to_owned(),
        access_key_id: DejavuSecret::new("access"),
        secret_access_key: DejavuSecret::new("secret"),
        request_timeout: Duration::from_secs(9),
        addressing_style: qingyu_dejavu::S3AddressingStyle::Path,
        tls_verification: qingyu_dejavu::S3TlsVerification::Verify,
    };

    let runner = KernelDejavuRunner::new_s3(inputs, config).expect("valid S3 runner");

    assert_eq!(format!("{runner:?}"), "KernelDejavuRunner([REDACTED])");
}

#[test]
fn nested_instance_roots_are_rejected_before_opening_repository_state() {
    let root = tempfile::tempdir().expect("fixture root");
    let (_workspace, mut inputs) = runner_inputs(root.path(), "nested-roots");
    let instance = root.path().join("instance");
    inputs.roots = DejavuInstanceRoots::new(
        instance.join("state"),
        instance.join("state/history"),
        instance.join("temp"),
    );
    let cloud_root = tempfile::tempdir().expect("cloud root");
    let cloud = Arc::new(LocalCloud::new(cloud_root.path()).expect("local cloud"));

    let Err(error) = KernelDejavuRunner::new_with_cloud(inputs, cloud) else {
        panic!("nested instance roots must fail closed");
    };

    assert_eq!(error, DejavuRunError::InvalidConfiguration);
}

#[tokio::test]
async fn cancellation_before_open_returns_only_the_safe_cancelled_error() {
    let cloud_root = tempfile::tempdir().expect("cloud root");
    let cloud = Arc::new(LocalCloud::new(cloud_root.path()).expect("local cloud"));
    let fixture = RunnerFixture::new("cancelled", cloud);
    fs::write(fixture.workspace.join("note.md"), b"not uploaded").expect("fixture note");

    let result = fixture.runner.run(Arc::new(|| true)).await;

    assert_eq!(result, Err(DejavuRunError::Cancelled));
    assert_eq!(
        DejavuRunError::Cancelled.safe_code(),
        "dejavu-job-cancelled"
    );
}

#[tokio::test]
async fn workspace_loss_after_permit_is_not_misreported_as_cancellation() {
    let cloud_root = tempfile::tempdir().expect("cloud root");
    let cloud = Arc::new(LocalCloud::new(cloud_root.path()).expect("local cloud"));
    let remote = RunnerFixture::new("remote", Arc::clone(&cloud));
    fs::write(remote.workspace.join("remote.md"), b"remote").expect("remote note");
    remote
        .runner
        .run(Arc::new(|| false))
        .await
        .expect("seed remote repository");

    let root = tempfile::tempdir().expect("fixture root");
    let (workspace, mut inputs) = runner_inputs(root.path(), "failing-workspace");
    inputs.workspace = Arc::new(FailingWorkspaceCapability::new(&workspace, 4));
    let runner = KernelDejavuRunner::new_with_cloud(inputs, cloud).expect("runner");

    let result = runner.run(Arc::new(|| false)).await;

    assert_eq!(result, Err(DejavuRunError::WorkspaceUnavailable));
}

#[tokio::test]
async fn real_dejavu_sync_maps_transfer_and_keep_local_conflict_safely() {
    let cloud_root = tempfile::tempdir().expect("cloud root");
    let cloud = Arc::new(LocalCloud::new(cloud_root.path()).expect("local cloud"));
    let local = RunnerFixture::new("local", Arc::clone(&cloud));
    let remote = RunnerFixture::new("remote", cloud);
    fs::write(local.workspace.join("same.md"), b"local").expect("local note");
    std::thread::sleep(Duration::from_millis(1_100));
    fs::write(remote.workspace.join("same.md"), b"remote").expect("remote note");

    let uploaded = remote
        .runner
        .run(Arc::new(|| false))
        .await
        .expect("remote upload");
    let conflicted = local
        .runner
        .run(Arc::new(|| false))
        .await
        .expect("local conflict sync");

    assert!(uploaded.transfer.upload_files >= 1);
    assert!(uploaded.transfer.upload_bytes >= 6);
    assert!(conflicted.data_changed);
    assert_eq!(conflicted.conflicts.len(), 1);
    assert_eq!(conflicted.conflicts[0].repository_id, REPOSITORY_ID);
    let conflict_id =
        uuid::Uuid::parse_str(&conflicted.conflicts[0].conflict_id).expect("canonical conflict ID");
    assert_eq!(conflict_id.to_string(), conflicted.conflicts[0].conflict_id);
    assert_eq!(conflicted.conflicts[0].relative_path, "same.md");
    assert_eq!(
        conflicted.conflicts[0].resolution,
        DejavuConflictResolution::KeepLocal
    );
    assert!(conflicted.conflicts[0].occurred_at.ends_with('Z'));
    assert_eq!(
        fs::read(local.workspace.join("same.md")).expect("retained local note"),
        b"local"
    );
}

#[test]
fn s3_configuration_debug_output_redacts_every_secret_and_endpoint() {
    let config = DejavuS3Config {
        endpoint_url: "https://private.example.test".to_owned(),
        region: "private-region".to_owned(),
        bucket: "private-bucket".to_owned(),
        access_key_id: DejavuSecret::new("private-access"),
        secret_access_key: DejavuSecret::new("private-secret"),
        request_timeout: Duration::from_secs(9),
        addressing_style: qingyu_dejavu::S3AddressingStyle::Path,
        tls_verification: qingyu_dejavu::S3TlsVerification::Verify,
    };

    let debug = format!("{config:?}");

    assert!(!debug.contains("private.example"));
    assert!(!debug.contains("private-access"));
    assert!(!debug.contains("private-secret"));
    assert!(debug.contains("[REDACTED]"));
}
