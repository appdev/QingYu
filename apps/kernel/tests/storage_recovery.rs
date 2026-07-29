use std::fs;

use qingyu_kernel::{
    config::KernelConfig,
    paths::KernelPaths,
    storage::{
        DurableFileFailureKind, DurableFileStore, ExpectedFile, PreservePrevious, ReplaceRequest,
        StorageFileName,
    },
};
use tempfile::tempdir;

#[test]
fn durable_store_creates_reads_and_revision_checks_files() {
    let fixture = StorageFixture::new();
    let store = fixture.store();
    let target = StorageFileName::parse("settings.json").unwrap();

    let created = store
        .replace(ReplaceRequest {
            target: &target,
            bytes: br#"{"schemaVersion":1}"#,
            expected: ExpectedFile::Absent,
            preserve_previous: PreservePrevious::None,
        })
        .unwrap();
    #[cfg(unix)]
    assert_eq!(
        created.commit_state,
        qingyu_kernel::storage::CommitState::Durable
    );
    #[cfg(windows)]
    {
        assert_eq!(
            created.commit_state,
            qingyu_kernel::storage::CommitState::PublishedDurabilityUncertain
        );
        let same_launch = fixture.store();
        let recovered = same_launch.recover().unwrap();
        assert!(matches!(
            recovered.as_slice(),
            [qingyu_kernel::storage::RecoveryOutcome::Committed {
                commit_state: qingyu_kernel::storage::CommitState::PublishedDurabilityUncertain,
                ..
            }]
        ));
        let blocked = same_launch.replace(ReplaceRequest {
            target: &target,
            bytes: b"same-launch-write-must-wait",
            expected: ExpectedFile::Revision(&created.installed_revision),
            preserve_previous: PreservePrevious::None,
        });
        assert_eq!(
            blocked.unwrap_err().kind(),
            DurableFileFailureKind::RecoveryRequired
        );

        let next_launch = fixture.store_for_new_launch();
        let finalized = next_launch.recover().unwrap();
        assert!(matches!(
            finalized.as_slice(),
            [qingyu_kernel::storage::RecoveryOutcome::Committed {
                commit_state: qingyu_kernel::storage::CommitState::PublishedDurabilityUncertain,
                ..
            }]
        ));
    }
    let stored = store.read(&target, 1_024).unwrap().unwrap();
    assert_eq!(stored.bytes, br#"{"schemaVersion":1}"#);
    assert_eq!(stored.revision, created.installed_revision);

    let conflict = store.replace(ReplaceRequest {
        target: &target,
        bytes: b"replacement",
        expected: ExpectedFile::Absent,
        preserve_previous: PreservePrevious::None,
    });
    assert_eq!(
        conflict.unwrap_err().kind(),
        DurableFileFailureKind::RevisionConflict
    );
    assert_eq!(
        fs::read(fixture.app_data.join("settings.json")).unwrap(),
        stored.bytes
    );
}

#[test]
fn durable_store_preserves_unsupported_bytes_before_replacement() {
    let fixture = StorageFixture::new();
    let original = b"{\n  \"schemaVersion\": 999,\n  \"unknown\": true\n}\n";
    fs::write(fixture.app_data.join("settings.json"), original).unwrap();
    let store = fixture.store();
    let target = StorageFileName::parse("settings.json").unwrap();
    let recovery = StorageFileName::parse("settings.unsupported.json").unwrap();
    let current = store.read(&target, 1_024).unwrap().unwrap();

    let outcome = store
        .replace(ReplaceRequest {
            target: &target,
            bytes: br#"{"schemaVersion":1}"#,
            expected: ExpectedFile::Revision(&current.revision),
            preserve_previous: PreservePrevious::Required {
                recovery_name: &recovery,
            },
        })
        .unwrap();

    assert_eq!(outcome.preserved_as.as_ref(), Some(&recovery));
    assert_eq!(
        fs::read(fixture.app_data.join(recovery.as_str())).unwrap(),
        original
    );
}

#[cfg(unix)]
#[test]
fn durable_store_rejects_symlink_targets_without_touching_the_victim() {
    use std::os::unix::fs::symlink;

    let fixture = StorageFixture::new();
    let victim = fixture.root.path().join("victim");
    fs::write(&victim, b"keep").unwrap();
    symlink(&victim, fixture.app_data.join("settings.json")).unwrap();
    let store = fixture.store();
    let target = StorageFileName::parse("settings.json").unwrap();

    let error = store.read(&target, 1_024).unwrap_err();

    assert_eq!(error.kind(), DurableFileFailureKind::UnsafeEntry);
    assert_eq!(fs::read(victim).unwrap(), b"keep");
}

#[cfg(unix)]
#[test]
fn durable_store_rejects_hardlinked_targets_without_touching_the_other_link() {
    let fixture = StorageFixture::new();
    let target_path = fixture.app_data.join("settings.json");
    let other_link = fixture.root.path().join("other-settings.json");
    fs::write(&target_path, b"keep").unwrap();
    fs::hard_link(&target_path, &other_link).unwrap();
    let store = fixture.store();
    let target = StorageFileName::parse("settings.json").unwrap();

    let error = store.read(&target, 1_024).unwrap_err();

    assert_eq!(error.kind(), DurableFileFailureKind::UnsafeEntry);
    assert_eq!(fs::read(other_link).unwrap(), b"keep");
}

#[test]
fn storage_names_and_debug_output_never_expose_host_roots() {
    for invalid in [
        "",
        ".",
        "..",
        "a/b",
        r"a\b",
        "C:state",
        ".qingyu-storage-forged",
    ] {
        assert!(StorageFileName::parse(invalid).is_err());
    }

    let fixture = StorageFixture::new();
    let store = fixture.store();
    assert!(!format!("{store:?}").contains(fixture.root.path().to_string_lossy().as_ref()));
}

#[test]
fn storage_names_reject_windows_device_aliases_on_every_platform() {
    for invalid in [
        "CON",
        "con.json",
        "PRN",
        "AUX.txt",
        "NUL",
        "COM1",
        "com9.log",
        "LPT1",
        "lpt9.json",
    ] {
        assert!(
            StorageFileName::parse(invalid).is_err(),
            "accepted Windows device alias {invalid:?}"
        );
    }
}

#[test]
fn storage_debug_output_redacts_stored_and_replacement_bytes() {
    let fixture = StorageFixture::new();
    let store = fixture.store();
    let target = StorageFileName::parse("settings.json").unwrap();
    let secret = b"s3-secret-access-key-must-not-leak";

    let request = ReplaceRequest {
        target: &target,
        bytes: secret,
        expected: ExpectedFile::Absent,
        preserve_previous: PreservePrevious::None,
    };
    let raw_bytes_debug = format!("{secret:?}");
    let request_debug = format!("{request:?}");
    assert!(!request_debug.contains("s3-secret-access-key"));
    assert!(!request_debug.contains(&raw_bytes_debug));
    store.replace(request).unwrap();
    let stored = store.read(&target, 1_024).unwrap().unwrap();
    let stored_debug = format!("{stored:?}");
    assert!(!stored_debug.contains("s3-secret-access-key"));
    assert!(!stored_debug.contains(&raw_bytes_debug));
}

struct StorageFixture {
    root: tempfile::TempDir,
    workspace: std::path::PathBuf,
    app_data: std::path::PathBuf,
    cache: std::path::PathBuf,
    config: KernelConfig,
}

impl StorageFixture {
    fn new() -> Self {
        let root = tempdir().unwrap();
        let workspace = root.path().join("workspace");
        let app_data = root.path().join("app-data");
        let cache = root.path().join("cache");
        fs::create_dir(&workspace).unwrap();
        fs::create_dir(&app_data).unwrap();
        fs::create_dir(&cache).unwrap();
        Self {
            root,
            workspace,
            app_data,
            cache,
            config: KernelConfig::generate().unwrap(),
        }
    }

    fn store(&self) -> DurableFileStore {
        let paths = KernelPaths::desktop(&self.workspace, &self.app_data, &self.cache).unwrap();
        DurableFileStore::at_instance_data(paths.instance_data_root(), self.config.launch_epoch())
            .unwrap()
    }

    #[cfg(windows)]
    fn store_for_new_launch(&self) -> DurableFileStore {
        let paths = KernelPaths::desktop(&self.workspace, &self.app_data, &self.cache).unwrap();
        let config = KernelConfig::generate().unwrap();
        DurableFileStore::at_instance_data(paths.instance_data_root(), config.launch_epoch())
            .unwrap()
    }
}
