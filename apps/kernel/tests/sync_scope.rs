use qingyu_kernel::sync::scope::RemoteSyncScope;
use tempfile::TempDir;

fn tempdir_without_symlink_ancestors() -> TempDir {
    tempfile::Builder::new()
        .prefix("qingyu-kernel-sync-scope-")
        .tempdir_in(std::env::current_dir().unwrap())
        .unwrap()
}

#[test]
fn notes_scope_uses_retained_authority_and_shared_ignore_policy() {
    let temporary = tempdir_without_symlink_ancestors();
    let workspace = temporary.path().join("workspace");
    let state = temporary.path().join("state");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&state).unwrap();
    std::fs::write(workspace.join(".markraignore"), "drafts/\n").unwrap();
    let scope = RemoteSyncScope::notes(
        &workspace,
        &state,
        "manifest.json",
        Some("workspace-a".to_string()),
        Some("generated/\n".to_string()),
    )
    .unwrap();

    assert!(scope.includes_relative_path("note.md", false));
    assert!(!scope.includes_relative_path("drafts/private.md", false));
    assert!(!scope.includes_relative_path("generated/private.md", false));
    assert!(!scope.includes_relative_path(".qingyu/private.json", false));
    assert!(!scope.includes_relative_path(".MARKRA-SYNC/manifest.json", false));
    assert_eq!(scope.local_identity(), Some("workspace-a"));
    assert_eq!(scope.manifest_name(), "manifest.json");
    assert_eq!(
        format!("{scope:?}"),
        "RemoteSyncScope { source: held, state: held }"
    );
    assert!(!format!("{scope:?}").contains(temporary.path().to_string_lossy().as_ref()));
}

#[test]
fn notes_scope_rejects_state_inside_the_workspace() {
    let temporary = tempdir_without_symlink_ancestors();
    let workspace = temporary.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();

    let error = RemoteSyncScope::notes(
        &workspace,
        workspace.join(".state"),
        "manifest.json",
        None,
        None,
    )
    .unwrap_err();

    assert_eq!(
        error,
        "Remote sync state root must be outside the notes source root"
    );
}

#[test]
fn portable_settings_scope_uses_the_kernel_validator() {
    let temporary = tempdir_without_symlink_ancestors();
    let app_data = temporary.path().join("app-data");
    let state = app_data.join("sync-state/settings/scope-a");
    std::fs::create_dir_all(&state).unwrap();
    let scope = RemoteSyncScope::portable_settings(&app_data, &state, "manifest.json").unwrap();

    assert!(scope.includes_relative_path("settings.json", false));
    assert!(!scope.includes_relative_path("credentials.json", false));
    assert!(scope.validate_download(br#"{}"#).is_ok());
    assert!(scope
        .validate_download(br#"{"secretAccessKey":"must-not-sync"}"#)
        .is_err());
}

#[cfg(unix)]
#[test]
fn replacing_the_state_address_with_a_symlink_fails_closed() {
    use std::os::unix::fs::symlink;

    let temporary = tempdir_without_symlink_ancestors();
    let workspace = temporary.path().join("workspace");
    let state = temporary.path().join("state");
    let outside = temporary.path().join("outside");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&state).unwrap();
    std::fs::create_dir(&outside).unwrap();
    let scope = RemoteSyncScope::notes(&workspace, &state, "manifest.json", None, None).unwrap();
    std::fs::remove_dir(&state).unwrap();
    symlink(&outside, &state).unwrap();

    assert_eq!(
        scope.open_state_root().unwrap_err(),
        "Remote sync state root is unsafe"
    );
}
