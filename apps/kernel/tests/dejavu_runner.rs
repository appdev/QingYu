use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use cap_std::fs::Dir;
use qingyu_dejavu::{
    Device, LocalCloud, NoopWorkingTreeCoordinator, RepositoryRelativePath, RepositoryRuntimeState,
    WorkingTreeAction, WorkingTreeChange, WorkingTreeCoordinator,
};
use qingyu_kernel::runtime::MutationCoordinator;
use qingyu_kernel::storage::{directory_identity, DirectoryIdentity};
use qingyu_kernel::sync::dejavu_runner::{
    DejavuConflictResolution, DejavuInstanceDataCapability, DejavuInstanceDataCapabilityError,
    DejavuRepositoryKey, DejavuRunError, DejavuRunnerInputs, DejavuS3Config, DejavuSecret,
    DejavuWorkspaceCapability, DejavuWorkspaceCapabilityError, KernelDejavuRunner,
    MutationWorkingTreeCoordinator,
};
use tempfile::TempDir;

const REPOSITORY_ID: &str = "323df833-764a-44b3-a534-492640c258f2";

#[tokio::test]
async fn mutation_coordinator_serializes_dejavu_and_kernel_writes() {
    let mutation = Arc::new(MutationCoordinator::new());
    let coordinator = Arc::new(MutationWorkingTreeCoordinator::new(Arc::clone(&mutation)));
    let existing_kernel_write = mutation.lock().await;
    let preparing = tokio::spawn({
        let coordinator = Arc::clone(&coordinator);
        async move {
            coordinator
                .prepare(&[WorkingTreeChange {
                    path: RepositoryRelativePath::new("note.md").expect("relative path"),
                    expected_revision: qingyu_dejavu::ExpectedRevision::Absent,
                    action: WorkingTreeAction::Write,
                }])
                .await
                .expect("working-tree permit")
        }
    });

    tokio::task::yield_now().await;
    assert!(!preparing.is_finished());
    drop(existing_kernel_write);

    let dejavu_permit = preparing.await.expect("prepare task");
    let blocked_kernel_write =
        tokio::time::timeout(Duration::from_millis(20), mutation.lock()).await;
    assert!(blocked_kernel_write.is_err());

    coordinator.release(dejavu_permit).await;
    let released_kernel_write = tokio::time::timeout(Duration::from_secs(1), mutation.lock())
        .await
        .expect("released mutation gate");
    drop(released_kernel_write);
}

struct TestWorkspaceCapability {
    canonical_path: PathBuf,
    directory: Dir,
    identity: DirectoryIdentity,
}

impl TestWorkspaceCapability {
    fn new(path: &Path) -> Self {
        let canonical_path = path.canonicalize().expect("canonical test workspace");
        let directory = Dir::open_ambient_dir(&canonical_path, cap_std::ambient_authority())
            .expect("retained test workspace");
        let identity = directory_identity(&directory).expect("workspace identity");
        Self {
            canonical_path,
            directory,
            identity,
        }
    }
}

impl DejavuWorkspaceCapability for TestWorkspaceCapability {
    fn verify_held_directory(&self) -> Result<(), DejavuWorkspaceCapabilityError> {
        let current = Dir::open_ambient_dir(&self.canonical_path, cap_std::ambient_authority())
            .map_err(|_| DejavuWorkspaceCapabilityError)?;
        if directory_identity(&self.directory).map_err(|_| DejavuWorkspaceCapabilityError)?
            == self.identity
            && directory_identity(&current).map_err(|_| DejavuWorkspaceCapabilityError)?
                == self.identity
        {
            Ok(())
        } else {
            Err(DejavuWorkspaceCapabilityError)
        }
    }

    fn try_clone_directory(&self) -> Result<Dir, DejavuWorkspaceCapabilityError> {
        self.directory
            .try_clone()
            .map_err(|_| DejavuWorkspaceCapabilityError)
    }

    fn canonical_path(&self) -> &Path {
        &self.canonical_path
    }
}

struct FailingWorkspaceCapability {
    canonical_path: PathBuf,
    directory: Dir,
    verification_count: AtomicUsize,
    fail_on: usize,
}

impl FailingWorkspaceCapability {
    fn new(path: &Path, fail_on: usize) -> Self {
        let canonical_path = path.canonicalize().expect("canonical test workspace");
        let directory = Dir::open_ambient_dir(&canonical_path, cap_std::ambient_authority())
            .expect("retained test workspace");
        Self {
            canonical_path,
            directory,
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

    fn try_clone_directory(&self) -> Result<Dir, DejavuWorkspaceCapabilityError> {
        self.directory
            .try_clone()
            .map_err(|_| DejavuWorkspaceCapabilityError)
    }

    fn canonical_path(&self) -> &Path {
        &self.canonical_path
    }
}

struct TestInstanceDataCapability {
    canonical_path: PathBuf,
    directory: Dir,
    identity: DirectoryIdentity,
}

impl TestInstanceDataCapability {
    fn new(path: &Path) -> Self {
        let canonical_path = path.canonicalize().expect("canonical instance data");
        let directory = Dir::open_ambient_dir(&canonical_path, cap_std::ambient_authority())
            .expect("retained instance data");
        let identity = directory_identity(&directory).expect("instance-data identity");
        Self {
            canonical_path,
            directory,
            identity,
        }
    }
}

impl DejavuInstanceDataCapability for TestInstanceDataCapability {
    fn verify_held_directory(&self) -> Result<(), DejavuInstanceDataCapabilityError> {
        let current = Dir::open_ambient_dir(&self.canonical_path, cap_std::ambient_authority())
            .map_err(|_| DejavuInstanceDataCapabilityError)?;
        if directory_identity(&self.directory).map_err(|_| DejavuInstanceDataCapabilityError)?
            == self.identity
            && directory_identity(&current).map_err(|_| DejavuInstanceDataCapabilityError)?
                == self.identity
        {
            Ok(())
        } else {
            Err(DejavuInstanceDataCapabilityError)
        }
    }

    fn try_clone_directory(&self) -> Result<Dir, DejavuInstanceDataCapabilityError> {
        self.directory
            .try_clone()
            .map_err(|_| DejavuInstanceDataCapabilityError)
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
        instance_data: Arc::new(TestInstanceDataCapability::new(&instance)),
        repository_id: REPOSITORY_ID.to_owned(),
        device: Device {
            id: name.to_owned(),
            name: name.to_owned(),
            os: "test".to_owned(),
        },
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
    assert_eq!(
        runner.remote_prefix(),
        "qingyu/repositories/323df833-764a-44b3-a534-492640c258f2/repo"
    );
}

#[test]
fn overlapping_workspace_and_instance_capabilities_are_rejected() {
    let root = tempfile::tempdir().expect("fixture root");
    let (workspace, mut inputs) = runner_inputs(root.path(), "overlapping-roots");
    inputs.instance_data = Arc::new(TestInstanceDataCapability::new(&workspace));
    let cloud_root = tempfile::tempdir().expect("cloud root");
    let cloud = Arc::new(LocalCloud::new(cloud_root.path()).expect("local cloud"));

    let Err(error) = KernelDejavuRunner::new_with_cloud(inputs, cloud) else {
        panic!("overlapping retained roots must fail closed");
    };

    assert_eq!(error, DejavuRunError::InvalidConfiguration);
}

#[tokio::test]
async fn workspace_syncignore_is_the_only_source_of_ignore_options() {
    let cloud_root = tempfile::tempdir().expect("cloud root");
    let cloud = Arc::new(LocalCloud::new(cloud_root.path()).expect("local cloud"));
    let source_root = tempfile::tempdir().expect("source root");
    let (source_workspace, source_inputs) = runner_inputs(source_root.path(), "source");
    fs::create_dir(source_workspace.join(".qingyu")).expect("syncignore directory");
    fs::write(
        source_workspace.join(".qingyu/syncignore"),
        b"drafts/**\n.qingyu/**\n",
    )
    .expect("syncignore");
    fs::create_dir(source_workspace.join("drafts")).expect("draft directory");
    fs::write(
        source_workspace.join("drafts/private.md"),
        b"must stay local",
    )
    .expect("ignored draft");
    fs::write(source_workspace.join("note.md"), b"must upload").expect("source note");
    let source = KernelDejavuRunner::new_with_cloud(source_inputs, cloud.clone()).expect("source");

    let uploaded = source.run(Arc::new(|| false)).await.expect("source upload");
    assert!(uploaded.transfer.upload_files >= 2);

    let target_root = tempfile::tempdir().expect("target root");
    let (target_workspace, target_inputs) = runner_inputs(target_root.path(), "target");
    fs::create_dir(target_workspace.join(".qingyu")).expect("target syncignore directory");
    fs::write(
        target_workspace.join(".qingyu/syncignore"),
        b"drafts/**\n.qingyu/**\n",
    )
    .expect("target syncignore");
    let target = KernelDejavuRunner::new_with_cloud(target_inputs, cloud).expect("target");
    target
        .run(Arc::new(|| false))
        .await
        .expect("target download");
    assert_eq!(
        fs::read(target_workspace.join("note.md")).expect("downloaded note"),
        b"must upload"
    );
    assert!(!target_workspace.join("drafts/private.md").exists());
    assert_eq!(
        fs::read(target_workspace.join(".qingyu/syncignore")).expect("protected syncignore"),
        b"drafts/**\n.qingyu/**\n"
    );
}

#[test]
fn repository_state_is_derived_into_isolated_legacy_compatible_layouts() {
    let root = tempfile::tempdir().expect("fixture root");
    let instance = root.path().join("instance");
    let first_workspace = root.path().join("first-workspace");
    let second_workspace = root.path().join("second-workspace");
    fs::create_dir(&instance).expect("instance");
    fs::create_dir(&first_workspace).expect("first workspace");
    fs::create_dir(&second_workspace).expect("second workspace");
    let instance_data = Arc::new(TestInstanceDataCapability::new(&instance));
    let first_inputs = DejavuRunnerInputs {
        workspace: Arc::new(TestWorkspaceCapability::new(&first_workspace)),
        instance_data: instance_data.clone(),
        repository_id: REPOSITORY_ID.to_owned(),
        device: Device {
            id: "first".to_owned(),
            name: "first".to_owned(),
            os: "test".to_owned(),
        },
        repository_key: DejavuRepositoryKey::new([7; 32]),
        runtime: RepositoryRuntimeState::default(),
        coordinator: Arc::new(NoopWorkingTreeCoordinator),
    };
    let cloud_root = tempfile::tempdir().expect("cloud root");
    let cloud = Arc::new(LocalCloud::new(cloud_root.path()).expect("local cloud"));
    KernelDejavuRunner::new_with_cloud(first_inputs, cloud.clone()).expect("first repository");
    let second_repository_id = "3f87eaef-ad5a-4ee1-8d2a-85b82b052d5e";
    let second_inputs = DejavuRunnerInputs {
        workspace: Arc::new(TestWorkspaceCapability::new(&second_workspace)),
        instance_data,
        repository_id: second_repository_id.to_owned(),
        device: Device {
            id: "second".to_owned(),
            name: "second".to_owned(),
            os: "test".to_owned(),
        },
        repository_key: DejavuRepositoryKey::new([7; 32]),
        runtime: RepositoryRuntimeState::default(),
        coordinator: Arc::new(NoopWorkingTreeCoordinator),
    };

    KernelDejavuRunner::new_with_cloud(second_inputs, cloud).expect("second repository");

    for repository_id in [REPOSITORY_ID, second_repository_id] {
        let repository_root = instance.join("sync/repositories").join(repository_id);
        assert!(repository_root.join("repo").is_dir());
        assert!(repository_root.join("history").is_dir());
        assert!(repository_root.join("temp").is_dir());
    }
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_syncignore_fails_closed_without_reading_its_target() {
    use std::os::unix::fs::symlink;

    let cloud_root = tempfile::tempdir().expect("cloud root");
    let cloud = Arc::new(LocalCloud::new(cloud_root.path()).expect("local cloud"));
    let root = tempfile::tempdir().expect("fixture root");
    let (workspace, inputs) = runner_inputs(root.path(), "symlink-syncignore");
    let outside = root.path().join("outside-syncignore");
    fs::write(&outside, b"keep me").expect("outside file");
    fs::create_dir(workspace.join(".qingyu")).expect("syncignore directory");
    symlink(&outside, workspace.join(".qingyu/syncignore")).expect("syncignore symlink");
    let runner = KernelDejavuRunner::new_with_cloud(inputs, cloud).expect("runner");

    let result = runner.run(Arc::new(|| false)).await;

    assert_eq!(result, Err(DejavuRunError::RepositoryUnavailable));
    assert_eq!(fs::read(outside).expect("untouched target"), b"keep me");
}

#[tokio::test]
async fn oversized_syncignore_fails_closed_before_repository_open() {
    let cloud_root = tempfile::tempdir().expect("cloud root");
    let cloud = Arc::new(LocalCloud::new(cloud_root.path()).expect("local cloud"));
    let root = tempfile::tempdir().expect("fixture root");
    let (workspace, inputs) = runner_inputs(root.path(), "oversized-syncignore");
    fs::create_dir(workspace.join(".qingyu")).expect("syncignore directory");
    fs::write(
        workspace.join(".qingyu/syncignore"),
        vec![b'x'; 1024 * 1024 + 1],
    )
    .expect("oversized syncignore");
    let runner = KernelDejavuRunner::new_with_cloud(inputs, cloud).expect("runner");

    let result = runner.run(Arc::new(|| false)).await;

    assert_eq!(result, Err(DejavuRunError::RepositoryUnavailable));
}

#[tokio::test]
async fn non_utf8_syncignore_fails_closed_before_repository_open() {
    let cloud_root = tempfile::tempdir().expect("cloud root");
    let cloud = Arc::new(LocalCloud::new(cloud_root.path()).expect("local cloud"));
    let root = tempfile::tempdir().expect("fixture root");
    let (workspace, inputs) = runner_inputs(root.path(), "non-utf8-syncignore");
    fs::create_dir(workspace.join(".qingyu")).expect("syncignore directory");
    fs::write(workspace.join(".qingyu/syncignore"), [0xff]).expect("non-UTF-8 syncignore");
    let runner = KernelDejavuRunner::new_with_cloud(inputs, cloud).expect("runner");

    let result = runner.run(Arc::new(|| false)).await;

    assert_eq!(result, Err(DejavuRunError::RepositoryUnavailable));
}

#[cfg(unix)]
#[test]
fn repository_layout_rejects_cross_repository_symlink_reuse() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().expect("fixture root");
    let (workspace, mut inputs) = runner_inputs(root.path(), "cross-repository-link");
    let instance = root.path().join("instance");
    let outside_repository = root.path().join("outside-repository");
    fs::create_dir(&outside_repository).expect("outside repository");
    let repository = instance.join("sync/repositories").join(REPOSITORY_ID);
    fs::create_dir_all(&repository).expect("repository layout");
    symlink(&outside_repository, repository.join("repo")).expect("cross repository symlink");
    inputs.workspace = Arc::new(TestWorkspaceCapability::new(&workspace));
    let cloud_root = tempfile::tempdir().expect("cloud root");
    let cloud = Arc::new(LocalCloud::new(cloud_root.path()).expect("local cloud"));

    let result = KernelDejavuRunner::new_with_cloud(inputs, cloud);

    assert!(matches!(result, Err(DejavuRunError::RepositoryUnavailable)));
}

#[tokio::test]
async fn repository_directory_replacement_after_construction_fails_closed() {
    let root = tempfile::tempdir().expect("fixture root");
    let (_workspace, inputs) = runner_inputs(root.path(), "replaced-repository");
    let cloud_root = tempfile::tempdir().expect("cloud root");
    let cloud = Arc::new(LocalCloud::new(cloud_root.path()).expect("local cloud"));
    let runner = KernelDejavuRunner::new_with_cloud(inputs, cloud).expect("runner");
    let repository = root
        .path()
        .join("instance/sync/repositories")
        .join(REPOSITORY_ID);
    let displaced = root.path().join("displaced-repository");
    fs::rename(&repository, &displaced).expect("displace repository");
    fs::create_dir(&repository).expect("replacement repository");

    let result = runner.run(Arc::new(|| false)).await;

    assert_eq!(result, Err(DejavuRunError::RepositoryUnavailable));
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
