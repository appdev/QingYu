use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::Duration,
};

use qingyu_kernel::{
    config::KernelConfig,
    contract::{HostProfile, Rfc3339Utc},
    events::{EventPublication, EventSink, EventSinkError},
    paths::{KernelPaths, PathPolicyErrorKind},
    ports::{
        BoxSleepFuture, BoxTaskFuture, Clock, CredentialSecret, CredentialSlot, CredentialStore,
        DiagnosticRecord, DiagnosticsSink, KernelPorts, NetworkReachability, PortError,
        PortErrorKind, Sleeper, TaskSpawner,
    },
    runtime::KernelRuntime,
    services::workspace::WorkspaceService,
    workspace::{
        lock::{KernelLockErrorKind, RuntimeLockLease},
        managed::ManagedWorkspaceCollection,
        primary::{
            PrimaryWorkspaceRepositoryBinding, PrimaryWorkspaceStore, PrimaryWorkspaceStoreError,
        },
    },
};
use serde_json::Value;
use tempfile::tempdir;

#[test]
fn server_layout_is_fixed_and_has_no_root_override_input() {
    let layout = KernelPaths::server();

    assert_eq!(layout.workspace_path(), Path::new("/data/workspace"));
    assert_eq!(layout.config_path(), Path::new("/data/config"));
    assert_eq!(layout.state_path(), Path::new("/data/state"));
    assert_eq!(layout.logs_path(), Path::new("/data/logs"));
    assert_eq!(layout.cache_path(), Path::new("/tmp/qingyu"));
}

#[test]
fn desktop_activates_three_disjoint_host_validated_roots() {
    let temporary = tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    let app_data = temporary.path().join("app-data");
    let cache = temporary.path().join("cache");
    fs::create_dir_all(&workspace).unwrap();
    fs::create_dir_all(&app_data).unwrap();
    fs::create_dir_all(&cache).unwrap();

    let paths = KernelPaths::desktop(&workspace, &app_data, &cache).unwrap();

    assert_eq!(paths.profile(), HostProfile::Desktop);
    let rendered = format!("{paths:?}");
    assert!(!rendered.contains(temporary.path().to_string_lossy().as_ref()));
}

#[cfg(unix)]
#[test]
fn desktop_instance_data_capability_rejects_an_address_replacement() {
    let temporary = tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    let app_data = temporary.path().join("app-data");
    let retained = temporary.path().join("retained-app-data");
    let cache = temporary.path().join("cache");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&app_data).unwrap();
    fs::create_dir(&cache).unwrap();
    let paths = KernelPaths::desktop(&workspace, &app_data, &cache).unwrap();

    fs::rename(&app_data, &retained).unwrap();
    fs::create_dir(&app_data).unwrap();

    assert!(paths.instance_data_root().verify_held_directory().is_err());
    assert_eq!(fs::read_dir(&app_data).unwrap().count(), 0);
}

#[test]
fn desktop_rejects_equal_or_nested_roots_without_revealing_them() {
    let temporary = tempdir().unwrap();
    let workspace = temporary.path().join("workspace");
    let nested = workspace.join("state");
    let cache = temporary.path().join("cache");
    fs::create_dir_all(&nested).unwrap();
    fs::create_dir_all(&cache).unwrap();

    let error = KernelPaths::desktop(&workspace, &nested, &cache).unwrap_err();

    assert_eq!(error.kind(), PathPolicyErrorKind::OverlappingRoots);
    assert!(!format!("{error:?}").contains(temporary.path().to_string_lossy().as_ref()));
    assert!(KernelPaths::desktop(&workspace, &workspace, &cache).is_err());
}

#[test]
fn mobile_creates_one_managed_workspace_under_app_data() {
    let temporary = tempdir().unwrap();
    let app_data = temporary.path().join("app-data");
    let cache = temporary.path().join("cache");
    fs::create_dir_all(&app_data).unwrap();
    fs::create_dir_all(&cache).unwrap();

    let paths = KernelPaths::mobile(&app_data, &cache, "个人 笔记").unwrap();

    assert_eq!(paths.profile(), HostProfile::Mobile);
    assert!(app_data.join("workspaces/个人 笔记").is_dir());
}

#[test]
fn mobile_accepts_os_cache_below_persistent_app_data() {
    let temporary = tempdir().unwrap();
    let app_data = temporary.path().join("app-data");
    let cache = app_data.join("cache");
    fs::create_dir_all(&cache).unwrap();

    let paths = KernelPaths::mobile(&app_data, &cache, "personal").unwrap();

    assert_eq!(paths.profile(), HostProfile::Mobile);
    assert!(app_data.join("workspaces/personal").is_dir());
}

#[test]
fn mobile_rejects_equal_instance_data_and_cache_roots() {
    let temporary = tempdir().unwrap();
    let shared = temporary.path().join("shared");
    fs::create_dir_all(&shared).unwrap();

    let error = KernelPaths::mobile(&shared, &shared, "personal").unwrap_err();

    assert_eq!(error.kind(), PathPolicyErrorKind::OverlappingRoots);
    assert!(!shared.join("workspaces").exists());
}

#[test]
fn mobile_rejects_persistent_app_data_below_cache() {
    let temporary = tempdir().unwrap();
    let cache = temporary.path().join("cache");
    let app_data = cache.join("app-data");
    fs::create_dir_all(&app_data).unwrap();

    let error = KernelPaths::mobile(&app_data, &cache, "personal").unwrap_err();

    assert_eq!(error.kind(), PathPolicyErrorKind::OverlappingRoots);
    assert!(!app_data.join("workspaces").exists());
}

#[test]
fn mobile_rejects_cache_below_the_managed_workspace_collection() {
    let temporary = tempdir().unwrap();
    let app_data = temporary.path().join("app-data");
    let cache = app_data.join("workspaces/system-cache");
    fs::create_dir_all(&cache).unwrap();

    let error = KernelPaths::mobile(&app_data, &cache, "personal").unwrap_err();

    assert_eq!(error.kind(), PathPolicyErrorKind::OverlappingRoots);
    assert!(!app_data.join("workspaces/personal").exists());
}

#[test]
fn mobile_workspace_capability_detects_when_the_managed_address_is_rebound() {
    let temporary = tempdir().unwrap();
    let app_data = temporary.path().join("app-data");
    let cache = temporary.path().join("cache");
    let displaced = temporary.path().join("displaced-workspace");
    fs::create_dir_all(&app_data).unwrap();
    fs::create_dir_all(&cache).unwrap();

    let paths = KernelPaths::mobile(&app_data, &cache, "personal").unwrap();
    fs::rename(app_data.join("workspaces/personal"), &displaced).unwrap();
    fs::create_dir(app_data.join("workspaces/personal")).unwrap();

    let error = paths.workspace_root().verify_held_directory().unwrap_err();
    assert_eq!(error.kind(), PathPolicyErrorKind::UnsafeEntry);
}

#[test]
fn mobile_workspace_capability_detects_when_the_collection_address_is_rebound() {
    let temporary = tempdir().unwrap();
    let app_data = temporary.path().join("app-data");
    let cache = temporary.path().join("cache");
    let displaced = temporary.path().join("displaced-collection");
    fs::create_dir_all(&app_data).unwrap();
    fs::create_dir_all(&cache).unwrap();

    let paths = KernelPaths::mobile(&app_data, &cache, "personal").unwrap();
    fs::rename(app_data.join("workspaces"), &displaced).unwrap();
    fs::create_dir_all(app_data.join("workspaces/personal")).unwrap();

    let error = paths.workspace_root().verify_held_directory().unwrap_err();
    assert_eq!(error.kind(), PathPolicyErrorKind::UnsafeEntry);
}

#[test]
fn mobile_rejects_unsafe_managed_names_before_creating_the_collection() {
    for invalid in [
        "",
        ".",
        "..",
        "a/b",
        r"a\b",
        "C:notes",
        "C:/notes",
        "bad\0name",
        ".qingyu",
    ] {
        let temporary = tempdir().unwrap();
        let app_data = temporary.path().join("app-data");
        let cache = temporary.path().join("cache");
        fs::create_dir_all(&app_data).unwrap();
        fs::create_dir_all(&cache).unwrap();

        let error = KernelPaths::mobile(&app_data, &cache, invalid).unwrap_err();

        assert_eq!(error.kind(), PathPolicyErrorKind::InvalidManagedName);
        assert!(!app_data.join("workspaces").exists());
    }
}

#[cfg(unix)]
#[test]
fn desktop_accepts_a_root_alias_but_mobile_rejects_an_escaping_collection_symlink() {
    use std::os::unix::fs::symlink;

    let temporary = tempdir().unwrap();
    let canonical_workspace = temporary.path().join("canonical-workspace");
    let workspace_alias = temporary.path().join("workspace-alias");
    let app_data = temporary.path().join("app-data");
    let cache = temporary.path().join("cache");
    let outside = temporary.path().join("outside");
    fs::create_dir_all(&canonical_workspace).unwrap();
    fs::create_dir_all(&app_data).unwrap();
    fs::create_dir_all(&cache).unwrap();
    fs::create_dir_all(&outside).unwrap();
    symlink(&canonical_workspace, &workspace_alias).unwrap();

    assert!(KernelPaths::desktop(&workspace_alias, &app_data, &cache).is_ok());

    symlink(&outside, app_data.join("workspaces")).unwrap();
    let error = KernelPaths::mobile(&app_data, &cache, "personal").unwrap_err();
    assert_eq!(error.kind(), PathPolicyErrorKind::UnsafeEntry);
    assert!(!outside.join("personal").exists());
}

#[test]
fn child_process_locks_distinguish_instance_and_workspace_contention() {
    let temporary = tempdir().unwrap();
    let parent = LockRoots::create(temporary.path(), "parent");
    let second_workspace = LockRoots::create(temporary.path(), "second-workspace");
    let second_instance = LockRoots::create(temporary.path(), "second-instance");
    let parent_paths = parent.paths();
    let lease = RuntimeLockLease::acquire(&parent_paths).unwrap();

    let instance_lock = parent.app_data.join("kernel.lock");
    let before = fs::metadata(&instance_lock).unwrap();
    run_lock_probe(
        &second_workspace.workspace,
        &parent.app_data,
        &second_workspace.cache,
        "instance-locked",
        None,
    );
    let after = fs::metadata(&instance_lock).unwrap();
    assert_eq!(before.len(), 0);
    assert_eq!(after.len(), 0);

    run_lock_probe(
        &parent.workspace,
        &second_instance.app_data,
        &second_instance.cache,
        "workspace-locked-with-rollback",
        Some(&second_instance.workspace),
    );

    drop(lease);
    run_lock_probe(
        &parent.workspace,
        &parent.app_data,
        &parent.cache,
        "acquired",
        None,
    );
}

#[cfg(unix)]
#[test]
fn lock_files_reject_links_preserve_victims_and_use_private_permissions() {
    use std::os::unix::fs::{symlink, PermissionsExt as _};

    let temporary = tempdir().unwrap();
    let roots = LockRoots::create(temporary.path(), "links");
    let victim = temporary.path().join("victim");
    fs::write(&victim, b"do-not-touch").unwrap();
    symlink(&victim, roots.app_data.join("kernel.lock")).unwrap();

    let error = RuntimeLockLease::acquire(&roots.paths()).unwrap_err();
    assert_eq!(error.kind(), KernelLockErrorKind::UnsafeLockFile);
    assert_eq!(fs::read(&victim).unwrap(), b"do-not-touch");

    fs::remove_file(roots.app_data.join("kernel.lock")).unwrap();
    let lease = RuntimeLockLease::acquire(&roots.paths()).unwrap();
    assert_eq!(
        fs::metadata(roots.app_data.join("kernel.lock"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(roots.workspace.join(".qingyu/workspace.lock"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    drop(lease);

    fs::remove_file(roots.workspace.join(".qingyu/workspace.lock")).unwrap();
    fs::hard_link(&victim, roots.workspace.join(".qingyu/workspace.lock")).unwrap();
    let error = RuntimeLockLease::acquire(&roots.paths()).unwrap_err();
    assert_eq!(error.kind(), KernelLockErrorKind::UnsafeLockFile);
    assert_eq!(fs::read(&victim).unwrap(), b"do-not-touch");
}

#[cfg(unix)]
#[test]
fn active_workspace_authority_fails_closed_after_its_lock_address_is_replaced() {
    let temporary = tempdir().unwrap();
    let roots = LockRoots::create(temporary.path(), "workspace-lock-replaced");
    let runtime = KernelRuntime::activate(
        KernelConfig::generate().unwrap(),
        roots.paths(),
        KernelPorts::unavailable(),
    )
    .unwrap();
    let authority = runtime
        .active_workspace_authority()
        .expect("workspace authority");
    let lock_path = roots.workspace.join(".qingyu/workspace.lock");
    let displaced_lock = roots.workspace.join(".qingyu/displaced-workspace.lock");

    fs::rename(&lock_path, &displaced_lock).unwrap();
    fs::write(&lock_path, b"").unwrap();

    assert_eq!(
        authority.verify_held_directory().unwrap_err().kind(),
        qingyu_kernel::runtime::WorkspaceAuthorityErrorKind::UnsafeEntry
    );

    let second_instance = LockRoots::create(temporary.path(), "workspace-lock-replacement-probe");
    run_lock_probe(
        &roots.workspace,
        &second_instance.app_data,
        &second_instance.cache,
        "acquired",
        None,
    );
}

#[cfg(unix)]
#[test]
fn active_runtime_fails_closed_after_its_instance_lock_address_is_replaced() {
    let temporary = tempdir().unwrap();
    let roots = LockRoots::create(temporary.path(), "instance-lock-replaced");
    let runtime = KernelRuntime::activate(
        KernelConfig::generate().unwrap(),
        roots.paths(),
        KernelPorts::unavailable(),
    )
    .unwrap();
    let lock_path = roots.app_data.join("kernel.lock");
    let displaced_lock = roots.app_data.join("displaced-kernel.lock");

    fs::rename(&lock_path, &displaced_lock).unwrap();
    fs::write(&lock_path, b"").unwrap();

    assert_eq!(
        runtime.verify_instance_lock().unwrap_err().kind(),
        KernelLockErrorKind::UnsafeLockFile
    );

    let alternate_workspace = LockRoots::create(temporary.path(), "instance-lock-alternate");
    run_lock_probe(
        &alternate_workspace.workspace,
        &roots.app_data,
        &roots.cache,
        "acquired",
        None,
    );
}

#[cfg(windows)]
#[test]
fn windows_lock_files_reject_hard_links_without_mutating_the_victim() {
    let temporary = tempdir().unwrap();
    let roots = LockRoots::create(temporary.path(), "windows-hard-link");
    let victim = temporary.path().join("victim");
    fs::write(&victim, b"do-not-touch").unwrap();
    fs::hard_link(&victim, roots.app_data.join("kernel.lock")).unwrap();

    let error = RuntimeLockLease::acquire(&roots.paths()).unwrap_err();

    assert_eq!(error.kind(), KernelLockErrorKind::UnsafeLockFile);
    assert_eq!(fs::read(&victim).unwrap(), b"do-not-touch");
}

#[cfg(windows)]
#[test]
fn windows_lock_files_reject_file_reparse_points_without_touching_the_target() {
    use std::os::windows::fs::symlink_file;

    let temporary = tempdir().unwrap();
    let roots = LockRoots::create(temporary.path(), "windows-reparse-point");
    let victim = temporary.path().join("victim");
    fs::write(&victim, b"do-not-touch").unwrap();
    symlink_file(&victim, roots.app_data.join("kernel.lock"))
        .expect("Windows CI must permit file reparse-point fixtures");

    let error = RuntimeLockLease::acquire(&roots.paths()).unwrap_err();

    assert_eq!(error.kind(), KernelLockErrorKind::UnsafeLockFile);
    assert_eq!(fs::read(&victim).unwrap(), b"do-not-touch");
}

#[cfg(windows)]
#[test]
fn windows_held_lock_denies_delete_sharing_but_keeps_lock_contention_available() {
    let temporary = tempdir().unwrap();
    let roots = LockRoots::create(temporary.path(), "windows-share-mode");
    let lease = RuntimeLockLease::acquire(&roots.paths()).unwrap();

    assert!(fs::rename(
        roots.app_data.join("kernel.lock"),
        roots.app_data.join("kernel-renamed.lock")
    )
    .is_err());
    run_lock_probe(
        &roots.workspace,
        &roots.app_data,
        &roots.cache,
        "instance-locked",
        None,
    );

    drop(lease);
}

#[test]
fn runtime_authenticates_and_explicitly_exposes_its_native_launch_credential() {
    let temporary = tempdir().unwrap();
    let roots = LockRoots::create(temporary.path(), "runtime-credential");
    let config = KernelConfig::generate().unwrap();
    let expected = config.native_launch_credential().expose_secret().to_owned();
    let runtime =
        KernelRuntime::activate(config, roots.paths(), KernelPorts::unavailable()).unwrap();

    assert!(runtime.matches_native_launch_credential(&expected));
    assert!(!runtime.matches_native_launch_credential("wrong"));
    assert_eq!(runtime.expose_native_launch_credential(), expected);
}

#[test]
fn runtime_clones_keep_cross_process_leases_alive_until_the_last_owner_drops() {
    let temporary = tempdir().unwrap();
    let roots = LockRoots::create(temporary.path(), "runtime");
    let runtime = KernelRuntime::activate(
        KernelConfig::generate().unwrap(),
        roots.paths(),
        KernelPorts::unavailable(),
    )
    .unwrap();
    let background_owner = runtime.clone();

    drop(runtime);
    run_lock_probe(
        &roots.workspace,
        &roots.app_data,
        &roots.cache,
        "instance-locked",
        None,
    );

    drop(background_owner);
    run_lock_probe(
        &roots.workspace,
        &roots.app_data,
        &roots.cache,
        "acquired",
        None,
    );
}

#[tokio::test]
async fn runtime_spawned_deferred_task_keeps_leases_until_the_task_finishes() {
    let temporary = tempdir().unwrap();
    let roots = LockRoots::create(temporary.path(), "runtime-task");
    let task_spawner = Arc::new(DeferredTaskSpawner::default());
    let runtime = KernelRuntime::activate(
        KernelConfig::generate().unwrap(),
        roots.paths(),
        test_ports(task_spawner.clone()),
    )
    .unwrap();
    let _workspace_service = initialize_workspace(&runtime, &roots).await;

    runtime.spawn_background(Box::pin(async {})).unwrap();
    drop(runtime);

    run_lock_probe(
        &roots.workspace,
        &roots.app_data,
        &roots.cache,
        "instance-locked",
        None,
    );

    task_spawner.finish_deferred_task();
    run_lock_probe(
        &roots.workspace,
        &roots.app_data,
        &roots.cache,
        "acquired",
        None,
    );
}

#[test]
fn desktop_switch_installs_one_new_authority_while_an_old_snapshot_retains_its_lock() {
    let temporary = tempdir().unwrap();
    let initial = LockRoots::create(temporary.path(), "authority-initial");
    let next = LockRoots::create(temporary.path(), "authority-next");
    let old_probe = LockRoots::create(temporary.path(), "authority-old-probe");
    let new_probe = LockRoots::create(temporary.path(), "authority-new-probe");
    let runtime = KernelRuntime::activate(
        KernelConfig::generate().unwrap(),
        initial.paths(),
        KernelPorts::unavailable(),
    )
    .unwrap();
    let old = runtime
        .active_workspace_authority()
        .expect("workspace authority");

    let prepared = runtime
        .prepare_host_workspace_authority(&next.workspace)
        .unwrap();
    assert!(Arc::ptr_eq(
        &old,
        &runtime
            .active_workspace_authority()
            .expect("workspace authority")
    ));

    let installed = runtime.commit_host_workspace_authority(prepared).unwrap();
    assert!(Arc::ptr_eq(
        &installed,
        &runtime
            .active_workspace_authority()
            .expect("workspace authority")
    ));
    assert!(!Arc::ptr_eq(&old, &installed));

    run_lock_probe(
        &initial.workspace,
        &old_probe.app_data,
        &old_probe.cache,
        "workspace-locked-with-rollback",
        Some(&old_probe.workspace),
    );
    drop(old);
    run_lock_probe(
        &initial.workspace,
        &old_probe.app_data,
        &old_probe.cache,
        "acquired",
        None,
    );
    run_lock_probe(
        &next.workspace,
        &new_probe.app_data,
        &new_probe.cache,
        "workspace-locked-with-rollback",
        Some(&new_probe.workspace),
    );
    run_lock_probe(
        &new_probe.workspace,
        &initial.app_data,
        &new_probe.cache,
        "instance-locked",
        None,
    );
}

#[test]
fn stale_prepared_authority_is_consumed_without_replacing_the_committed_authority() {
    let temporary = tempdir().unwrap();
    let initial = LockRoots::create(temporary.path(), "stale-initial");
    let first = LockRoots::create(temporary.path(), "stale-first");
    let stale = LockRoots::create(temporary.path(), "stale-candidate");
    let stale_probe = LockRoots::create(temporary.path(), "stale-probe");
    let runtime = KernelRuntime::activate(
        KernelConfig::generate().unwrap(),
        initial.paths(),
        KernelPorts::unavailable(),
    )
    .unwrap();
    let prepared_first = runtime
        .prepare_host_workspace_authority(&first.workspace)
        .unwrap();
    let prepared_stale = runtime
        .prepare_host_workspace_authority(&stale.workspace)
        .unwrap();
    let installed = runtime
        .commit_host_workspace_authority(prepared_first)
        .unwrap();

    let error = runtime
        .commit_host_workspace_authority(prepared_stale)
        .unwrap_err();

    assert_eq!(
        error.kind(),
        qingyu_kernel::runtime::WorkspaceAuthorityErrorKind::PreparedAuthorityMismatch
    );
    assert!(Arc::ptr_eq(
        &installed,
        &runtime
            .active_workspace_authority()
            .expect("workspace authority")
    ));
    run_lock_probe(
        &stale.workspace,
        &stale_probe.app_data,
        &stale_probe.cache,
        "acquired",
        None,
    );
}

#[tokio::test]
async fn background_task_started_before_switch_retains_the_old_workspace_lock_until_completion() {
    let temporary = tempdir().unwrap();
    let initial = LockRoots::create(temporary.path(), "background-switch-initial");
    let next = LockRoots::create(temporary.path(), "background-switch-next");
    let probe = LockRoots::create(temporary.path(), "background-switch-probe");
    let task_spawner = Arc::new(DeferredTaskSpawner::default());
    let runtime = KernelRuntime::activate(
        KernelConfig::generate().unwrap(),
        initial.paths(),
        test_ports(task_spawner.clone()),
    )
    .unwrap();
    let workspace_service = initialize_workspace(&runtime, &initial).await;

    runtime.spawn_background(Box::pin(async {})).unwrap();
    let prepared = runtime
        .prepare_host_workspace_authority(&next.workspace)
        .unwrap();
    let current = workspace_service.current().unwrap();
    workspace_service
        .compare_and_set_host_workspace(&current.revision, prepared, "Next")
        .await
        .unwrap();

    run_lock_probe(
        &initial.workspace,
        &probe.app_data,
        &probe.cache,
        "workspace-locked-with-rollback",
        Some(&probe.workspace),
    );
    task_spawner.finish_deferred_task();
    run_lock_probe(
        &initial.workspace,
        &probe.app_data,
        &probe.cache,
        "acquired",
        None,
    );
}

#[test]
fn mobile_runtime_rejects_host_workspace_prepare_without_changing_authority() {
    let temporary = tempdir().unwrap();
    let app_data = temporary.path().join("mobile-authority-app-data");
    let cache = temporary.path().join("mobile-authority-cache");
    let alternate = temporary.path().join("mobile-authority-alternate");
    fs::create_dir_all(&app_data).unwrap();
    fs::create_dir_all(&cache).unwrap();
    fs::create_dir_all(&alternate).unwrap();
    let runtime = KernelRuntime::activate(
        KernelConfig::generate().unwrap(),
        KernelPaths::mobile(&app_data, &cache, "personal").unwrap(),
        KernelPorts::unavailable(),
    )
    .unwrap();
    let before = runtime
        .active_workspace_authority()
        .expect("workspace authority");

    let error = runtime
        .prepare_host_workspace_authority(&alternate)
        .unwrap_err();

    assert_eq!(
        error.kind(),
        qingyu_kernel::runtime::WorkspaceAuthorityErrorKind::UnsupportedProfile
    );
    assert!(Arc::ptr_eq(
        &before,
        &runtime
            .active_workspace_authority()
            .expect("workspace authority")
    ));
}

#[test]
fn desktop_prepare_rejects_private_root_overlap_without_changing_authority() {
    let temporary = tempdir().unwrap();
    let roots = LockRoots::create(temporary.path(), "authority-overlap");
    let runtime = KernelRuntime::activate(
        KernelConfig::generate().unwrap(),
        roots.paths(),
        KernelPorts::unavailable(),
    )
    .unwrap();
    let before = runtime
        .active_workspace_authority()
        .expect("workspace authority");

    for private_root in [&roots.app_data, &roots.cache] {
        let error = runtime
            .prepare_host_workspace_authority(private_root)
            .unwrap_err();
        assert_eq!(
            error.kind(),
            qingyu_kernel::runtime::WorkspaceAuthorityErrorKind::OverlappingRoots
        );
        assert!(!format!("{error:?}").contains(temporary.path().to_string_lossy().as_ref()));
        assert!(Arc::ptr_eq(
            &before,
            &runtime
                .active_workspace_authority()
                .expect("workspace authority")
        ));
    }
}

#[test]
fn commit_revalidates_the_prepared_address_and_leaves_current_authority_unchanged_on_rebind() {
    let temporary = tempdir().unwrap();
    let initial = LockRoots::create(temporary.path(), "commit-rebind-initial");
    let next = LockRoots::create(temporary.path(), "commit-rebind-next");
    let displaced = temporary.path().join("commit-rebind-displaced");
    let runtime = KernelRuntime::activate(
        KernelConfig::generate().unwrap(),
        initial.paths(),
        KernelPorts::unavailable(),
    )
    .unwrap();
    let before = runtime
        .active_workspace_authority()
        .expect("workspace authority");
    let prepared = runtime
        .prepare_host_workspace_authority(&next.workspace)
        .unwrap();
    fs::rename(&next.workspace, &displaced).unwrap();
    fs::create_dir(&next.workspace).unwrap();

    let error = runtime
        .commit_host_workspace_authority(prepared)
        .unwrap_err();

    assert_eq!(
        error.kind(),
        qingyu_kernel::runtime::WorkspaceAuthorityErrorKind::UnsafeEntry
    );
    assert!(Arc::ptr_eq(
        &before,
        &runtime
            .active_workspace_authority()
            .expect("workspace authority")
    ));
}

#[test]
fn prepare_rejects_a_locked_target_without_changing_current_authority() {
    let temporary = tempdir().unwrap();
    let initial = LockRoots::create(temporary.path(), "locked-target-initial");
    let target = LockRoots::create(temporary.path(), "locked-target-held");
    let target_paths = target.paths();
    let target_lease = RuntimeLockLease::acquire(&target_paths).unwrap();
    let runtime = KernelRuntime::activate(
        KernelConfig::generate().unwrap(),
        initial.paths(),
        KernelPorts::unavailable(),
    )
    .unwrap();
    let before = runtime
        .active_workspace_authority()
        .expect("workspace authority");

    let error = runtime
        .prepare_host_workspace_authority(&target.workspace)
        .unwrap_err();

    assert_eq!(
        error.kind(),
        qingyu_kernel::runtime::WorkspaceAuthorityErrorKind::WorkspaceLocked
    );
    assert!(Arc::ptr_eq(
        &before,
        &runtime
            .active_workspace_authority()
            .expect("workspace authority")
    ));
    drop(target_lease);
    assert!(runtime
        .prepare_host_workspace_authority(&target.workspace)
        .is_ok());
}

#[test]
fn prepared_authority_cannot_be_committed_by_another_runtime() {
    let temporary = tempdir().unwrap();
    let first = LockRoots::create(temporary.path(), "foreign-first");
    let second = LockRoots::create(temporary.path(), "foreign-second");
    let candidate = LockRoots::create(temporary.path(), "foreign-candidate");
    let first_runtime = KernelRuntime::activate(
        KernelConfig::generate().unwrap(),
        first.paths(),
        KernelPorts::unavailable(),
    )
    .unwrap();
    let second_runtime = KernelRuntime::activate(
        KernelConfig::generate().unwrap(),
        second.paths(),
        KernelPorts::unavailable(),
    )
    .unwrap();
    let second_before = second_runtime
        .active_workspace_authority()
        .expect("workspace authority");
    let prepared = first_runtime
        .prepare_host_workspace_authority(&candidate.workspace)
        .unwrap();

    let error = second_runtime
        .commit_host_workspace_authority(prepared)
        .unwrap_err();

    assert_eq!(
        error.kind(),
        qingyu_kernel::runtime::WorkspaceAuthorityErrorKind::PreparedAuthorityMismatch
    );
    assert!(Arc::ptr_eq(
        &second_before,
        &second_runtime
            .active_workspace_authority()
            .expect("workspace authority")
    ));
}

#[test]
fn runtime_exposes_one_shared_mutation_coordinator() {
    let temporary = tempdir().unwrap();
    let roots = LockRoots::create(temporary.path(), "shared-mutation-coordinator");
    let runtime = KernelRuntime::activate(
        KernelConfig::generate().unwrap(),
        roots.paths(),
        KernelPorts::unavailable(),
    )
    .unwrap();

    let first = runtime.mutation_coordinator().clone();
    let second = runtime.mutation_coordinator().clone();

    assert!(Arc::ptr_eq(&first, &second));
}

#[test]
#[ignore = "child-process helper"]
fn lock_probe_child() {
    let workspace = PathBuf::from(std::env::var_os("QINGYU_LOCK_WORKSPACE").unwrap());
    let app_data = PathBuf::from(std::env::var_os("QINGYU_LOCK_APP_DATA").unwrap());
    let cache = PathBuf::from(std::env::var_os("QINGYU_LOCK_CACHE").unwrap());
    let expected = std::env::var("QINGYU_LOCK_EXPECTED").unwrap();
    let paths = KernelPaths::desktop(&workspace, &app_data, &cache).unwrap();
    let result = RuntimeLockLease::acquire(&paths);

    match expected.as_str() {
        "acquired" => assert!(result.is_ok()),
        "instance-locked" => assert_eq!(
            result.unwrap_err().kind(),
            KernelLockErrorKind::InstanceLocked
        ),
        "workspace-locked-with-rollback" => {
            assert_eq!(
                result.unwrap_err().kind(),
                KernelLockErrorKind::WorkspaceLocked
            );
            let alternate = PathBuf::from(std::env::var_os("QINGYU_LOCK_ALTERNATE").unwrap());
            let alternate_paths = KernelPaths::desktop(&alternate, &app_data, &cache).unwrap();
            assert!(RuntimeLockLease::acquire(&alternate_paths).is_ok());
        }
        _ => panic!("unknown lock probe expectation"),
    }
}

struct LockRoots {
    workspace: PathBuf,
    app_data: PathBuf,
    cache: PathBuf,
}

#[derive(Default)]
struct MemoryPrimaryWorkspaceStore {
    binding: PrimaryWorkspaceRepositoryBinding,
    value: Mutex<Option<Value>>,
}

impl PrimaryWorkspaceStore for MemoryPrimaryWorkspaceStore {
    fn repository_binding(&self) -> PrimaryWorkspaceRepositoryBinding {
        self.binding.clone()
    }

    fn load(&self) -> Result<Option<Value>, PrimaryWorkspaceStoreError> {
        Ok(self.value.lock().unwrap().clone())
    }

    fn replace(&self, value: Option<Value>) -> Result<(), PrimaryWorkspaceStoreError> {
        *self.value.lock().unwrap() = value;
        Ok(())
    }

    fn save(&self) -> Result<(), PrimaryWorkspaceStoreError> {
        Ok(())
    }
}

#[derive(Default)]
struct DeferredTaskSpawner {
    task: Mutex<Option<BoxTaskFuture>>,
}

impl DeferredTaskSpawner {
    fn finish_deferred_task(&self) {
        let mut task = self.task.lock().unwrap().take().expect("deferred task");
        let mut context = Context::from_waker(std::task::Waker::noop());
        assert_eq!(task.as_mut().poll(&mut context), Poll::Ready(()));
    }
}

impl TaskSpawner for DeferredTaskSpawner {
    fn spawn(&self, task: BoxTaskFuture) -> Result<(), PortError> {
        let mut deferred = self.task.lock().unwrap();
        if deferred.is_some() {
            return Err(PortError::new(PortErrorKind::Rejected));
        }
        *deferred = Some(task);
        Ok(())
    }
}

struct TestUnavailablePort;

impl EventSink for TestUnavailablePort {
    fn publish(&self, _publication: &EventPublication) -> Result<(), EventSinkError> {
        Err(EventSinkError)
    }
}

impl Clock for TestUnavailablePort {
    fn now(&self) -> Result<Rfc3339Utc, PortError> {
        Err(PortError::unavailable())
    }
}

impl Sleeper for TestUnavailablePort {
    fn sleep(&self, _duration: Duration) -> BoxSleepFuture<'_> {
        Box::pin(async { Err(PortError::unavailable()) })
    }
}

impl CredentialStore for TestUnavailablePort {
    fn is_present(&self, _slot: CredentialSlot) -> Result<bool, PortError> {
        Err(PortError::unavailable())
    }

    fn replace(&self, _slot: CredentialSlot, _value: &CredentialSecret) -> Result<(), PortError> {
        Err(PortError::unavailable())
    }

    fn clear(&self, _slot: CredentialSlot) -> Result<(), PortError> {
        Err(PortError::unavailable())
    }
}

impl DiagnosticsSink for TestUnavailablePort {
    fn emit(&self, _record: DiagnosticRecord) -> Result<(), PortError> {
        Err(PortError::unavailable())
    }
}

impl NetworkReachability for TestUnavailablePort {
    fn is_reachable(&self) -> Result<bool, PortError> {
        Err(PortError::unavailable())
    }
}

fn test_ports(task_spawner: Arc<dyn TaskSpawner>) -> KernelPorts {
    let unavailable = Arc::new(TestUnavailablePort);
    KernelPorts::new(
        unavailable.clone(),
        unavailable.clone(),
        unavailable.clone(),
        task_spawner,
        unavailable.clone(),
        unavailable.clone(),
        unavailable,
    )
}

async fn initialize_workspace(runtime: &Arc<KernelRuntime>, roots: &LockRoots) -> WorkspaceService {
    WorkspaceService::new(
        runtime,
        Arc::new(MemoryPrimaryWorkspaceStore::default()),
        ManagedWorkspaceCollection::from_paths(&roots.paths()).unwrap(),
        Arc::new(TestUnavailablePort),
        "Initial",
    )
    .await
    .unwrap()
}

impl LockRoots {
    fn create(root: &Path, name: &str) -> Self {
        let base = root.join(name);
        let roots = Self {
            workspace: base.join("workspace"),
            app_data: base.join("app-data"),
            cache: base.join("cache"),
        };
        fs::create_dir_all(&roots.workspace).unwrap();
        fs::create_dir_all(&roots.app_data).unwrap();
        fs::create_dir_all(&roots.cache).unwrap();
        roots
    }

    fn paths(&self) -> KernelPaths {
        KernelPaths::desktop(&self.workspace, &self.app_data, &self.cache).unwrap()
    }
}

fn run_lock_probe(
    workspace: &Path,
    app_data: &Path,
    cache: &Path,
    expected: &str,
    alternate: Option<&Path>,
) {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .arg("--ignored")
        .arg("--exact")
        .arg("lock_probe_child")
        .arg("--test-threads=1")
        .env("QINGYU_LOCK_WORKSPACE", workspace)
        .env("QINGYU_LOCK_APP_DATA", app_data)
        .env("QINGYU_LOCK_CACHE", cache)
        .env("QINGYU_LOCK_EXPECTED", expected);
    if let Some(alternate) = alternate {
        command.env("QINGYU_LOCK_ALTERNATE", alternate);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "lock probe failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
