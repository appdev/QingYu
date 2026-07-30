//! Read-only compatibility with the desktop DejaVu repository binding state.

#![allow(dead_code)] // Staged until ProductionSyncExecutor composes the S3 runner.

use std::{collections::HashSet, fmt, io::Read, path::PathBuf};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::Deserialize;

use crate::{
    paths::{InstanceDataRoot, WorkspaceRoot},
    storage::{nonfollowing_read_options, unique_regular_file_identity},
    sync::dejavu_runner::DejavuRepositoryKey,
};

const LOCAL_SYNC_STATE_FILE: &str = "local-sync.json";
const LOCAL_SYNC_STATE_VERSION: u32 = 1;
const MAX_LOCAL_SYNC_STATE_BYTES: usize = 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct LegacyLocalSyncState {
    version: u32,
    device_id: String,
    repo_key: String,
    bindings: Vec<LegacyRepositoryBinding>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct LegacyRepositoryBinding {
    repository_id: String,
    display_name: String,
    notes_root: PathBuf,
    enabled: bool,
}

pub(crate) struct DejavuLocalRepositoryBinding {
    repository_id: String,
    device_id: String,
    repository_key: DejavuRepositoryKey,
}

impl DejavuLocalRepositoryBinding {
    pub(crate) fn into_parts(self) -> (String, String, DejavuRepositoryKey) {
        (self.repository_id, self.device_id, self.repository_key)
    }
}

impl fmt::Debug for DejavuLocalRepositoryBinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DejavuLocalRepositoryBinding([REDACTED])")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DejavuLocalStateError;

pub(crate) fn read_active_dejavu_binding(
    instance_data: &InstanceDataRoot,
    workspace: &WorkspaceRoot,
) -> Result<Option<DejavuLocalRepositoryBinding>, DejavuLocalStateError> {
    instance_data
        .verify_held_directory()
        .map_err(|_| DejavuLocalStateError)?;
    workspace
        .verify_held_directory()
        .map_err(|_| DejavuLocalStateError)?;
    let directory = instance_data
        .try_clone_dir()
        .map_err(|_| DejavuLocalStateError)?;
    let addressed = match directory.symlink_metadata(LOCAL_SYNC_STATE_FILE) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(DejavuLocalStateError),
    };
    if addressed.len() > MAX_LOCAL_SYNC_STATE_BYTES as u64 {
        return Err(DejavuLocalStateError);
    }
    let expected = unique_regular_file_identity(&addressed).ok_or(DejavuLocalStateError)?;
    let mut file = directory
        .open_with(LOCAL_SYNC_STATE_FILE, &nonfollowing_read_options())
        .map_err(|_| DejavuLocalStateError)?;
    if !expected
        .matches_retained_regular_file(&file.metadata().map_err(|_| DejavuLocalStateError)?, false)
    {
        return Err(DejavuLocalStateError);
    }
    let mut bytes = Vec::with_capacity(addressed.len() as usize);
    Read::by_ref(&mut file)
        .take(MAX_LOCAL_SYNC_STATE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| DejavuLocalStateError)?;
    if bytes.len() > MAX_LOCAL_SYNC_STATE_BYTES
        || !expected.matches_retained_regular_file(
            &file.metadata().map_err(|_| DejavuLocalStateError)?,
            false,
        )
        || !expected.matches_retained_regular_file(
            &directory
                .symlink_metadata(LOCAL_SYNC_STATE_FILE)
                .map_err(|_| DejavuLocalStateError)?,
            false,
        )
    {
        return Err(DejavuLocalStateError);
    }
    instance_data
        .verify_held_directory()
        .map_err(|_| DejavuLocalStateError)?;
    workspace
        .verify_held_directory()
        .map_err(|_| DejavuLocalStateError)?;

    let state: LegacyLocalSyncState =
        serde_json::from_slice(&bytes).map_err(|_| DejavuLocalStateError)?;
    if state.version != LOCAL_SYNC_STATE_VERSION {
        return Err(DejavuLocalStateError);
    }
    let device = uuid::Uuid::parse_str(&state.device_id).map_err(|_| DejavuLocalStateError)?;
    if device.get_version() != Some(uuid::Version::Random) {
        return Err(DejavuLocalStateError);
    }
    let key = STANDARD
        .decode(state.repo_key.as_bytes())
        .ok()
        .and_then(|decoded| <[u8; 32]>::try_from(decoded).ok())
        .ok_or(DejavuLocalStateError)?;
    let mut repository_ids = HashSet::new();
    let mut roots = HashSet::new();
    let mut active = None;
    for binding in state.bindings {
        if binding.repository_id.is_empty()
            || !binding.notes_root.is_absolute()
            || !repository_ids.insert(binding.repository_id.clone())
            || !roots.insert(binding.notes_root.clone())
        {
            return Err(DejavuLocalStateError);
        }
        if binding.enabled && binding.notes_root == workspace.canonical_path() {
            let repository = uuid::Uuid::parse_str(&binding.repository_id)
                .map_err(|_| DejavuLocalStateError)?
                .to_string();
            if repository != binding.repository_id || active.is_some() {
                return Err(DejavuLocalStateError);
            }
            active = Some(repository);
        }
    }
    Ok(active.map(|repository_id| DejavuLocalRepositoryBinding {
        repository_id,
        device_id: device.to_string(),
        repository_key: DejavuRepositoryKey::new(key),
    }))
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use serde_json::json;
    use tempfile::tempdir;

    use crate::paths::KernelPaths;

    const REPOSITORY_ID: &str = "323df833-764a-44b3-a534-492640c258f2";
    const DEVICE_ID: &str = "eb473600-dace-4d7e-bdad-7dac05933099";

    fn state_value(workspace: &Path, enabled: bool) -> serde_json::Value {
        json!({
            "version": 1,
            "deviceId": DEVICE_ID,
            "repoKey": STANDARD.encode([7_u8; 32]),
            "bindings": [{
                "repositoryId": REPOSITORY_ID,
                "displayName": "Private notes",
                "notesRoot": workspace,
                "enabled": enabled
            }]
        })
    }

    fn paths(temporary: &tempfile::TempDir) -> (std::path::PathBuf, KernelPaths) {
        let workspace = temporary.path().join("workspace");
        let app_data = temporary.path().join("app-data");
        let cache = temporary.path().join("cache");
        fs::create_dir(&workspace).expect("workspace");
        fs::create_dir(&app_data).expect("app data");
        fs::create_dir(&cache).expect("cache");
        let paths = KernelPaths::desktop(&workspace, &app_data, &cache).expect("Kernel paths");
        (app_data, paths)
    }

    #[test]
    fn reads_the_matching_enabled_desktop_binding_without_debug_secrets() {
        let temporary = tempdir().expect("fixture root");
        let (app_data, paths) = paths(&temporary);
        fs::write(
            app_data.join("local-sync.json"),
            serde_json::to_vec_pretty(&state_value(paths.workspace_root().canonical_path(), true))
                .expect("state JSON"),
        )
        .expect("state file");

        let binding =
            super::read_active_dejavu_binding(paths.instance_data_root(), paths.workspace_root())
                .expect("compatible state")
                .expect("active binding");

        assert_eq!(
            format!("{binding:?}"),
            "DejavuLocalRepositoryBinding([REDACTED])"
        );
        let (actual_repository, actual_device, key) = binding.into_parts();
        assert_eq!(actual_repository, REPOSITORY_ID);
        assert_eq!(actual_device, DEVICE_ID);
        assert_eq!(format!("{key:?}"), "DejavuRepositoryKey([REDACTED])");
    }

    #[test]
    fn absent_disabled_and_unbound_state_do_not_activate_a_repository() {
        for scenario in ["absent", "disabled", "unbound"] {
            let temporary = tempdir().expect("fixture root");
            let (app_data, paths) = paths(&temporary);
            if scenario != "absent" {
                let root = if scenario == "unbound" {
                    let other = temporary.path().join("other");
                    fs::create_dir(&other).expect("other workspace");
                    other.canonicalize().expect("canonical other workspace")
                } else {
                    paths.workspace_root().canonical_path().to_path_buf()
                };
                fs::write(
                    app_data.join("local-sync.json"),
                    serde_json::to_vec(&state_value(&root, scenario != "disabled"))
                        .expect("state JSON"),
                )
                .expect("state file");
            }

            assert!(super::read_active_dejavu_binding(
                paths.instance_data_root(),
                paths.workspace_root(),
            )
            .expect("compatible inactive state")
            .is_none());
        }
    }

    #[test]
    fn malformed_key_and_duplicate_bindings_fail_closed() {
        for state in [
            json!({
                "version": 1,
                "deviceId": DEVICE_ID,
                "repoKey": "not-a-key",
                "bindings": []
            }),
            json!({
                "version": 1,
                "deviceId": DEVICE_ID,
                "repoKey": STANDARD.encode([7_u8; 32]),
                "bindings": [
                    {
                        "repositoryId": REPOSITORY_ID,
                        "displayName": "One",
                        "notesRoot": "/tmp/one",
                        "enabled": false
                    },
                    {
                        "repositoryId": REPOSITORY_ID,
                        "displayName": "Two",
                        "notesRoot": "/tmp/two",
                        "enabled": false
                    }
                ]
            }),
        ] {
            let temporary = tempdir().expect("fixture root");
            let (app_data, paths) = paths(&temporary);
            fs::write(
                app_data.join("local-sync.json"),
                serde_json::to_vec(&state).expect("state JSON"),
            )
            .expect("state file");

            assert_eq!(
                super::read_active_dejavu_binding(
                    paths.instance_data_root(),
                    paths.workspace_root(),
                )
                .expect_err("invalid state"),
                super::DejavuLocalStateError
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn linked_state_files_are_rejected_without_reading_the_target() {
        use std::os::unix::fs::symlink;

        for link_kind in ["symlink", "hardlink"] {
            let temporary = tempdir().expect("fixture root");
            let (app_data, paths) = paths(&temporary);
            let outside = temporary.path().join("outside-state");
            let bytes =
                serde_json::to_vec(&state_value(paths.workspace_root().canonical_path(), true))
                    .expect("state JSON");
            fs::write(&outside, &bytes).expect("outside state");
            if link_kind == "symlink" {
                symlink(&outside, app_data.join("local-sync.json")).expect("state symlink");
            } else {
                fs::hard_link(&outside, app_data.join("local-sync.json")).expect("state hard link");
            }

            assert_eq!(
                super::read_active_dejavu_binding(
                    paths.instance_data_root(),
                    paths.workspace_root(),
                )
                .expect_err("linked state"),
                super::DejavuLocalStateError
            );
            assert_eq!(fs::read(outside).expect("untouched target"), bytes);
        }
    }

    #[test]
    fn oversized_state_is_rejected_before_json_parsing() {
        let temporary = tempdir().expect("fixture root");
        let (app_data, paths) = paths(&temporary);
        fs::write(
            app_data.join("local-sync.json"),
            vec![b' '; super::MAX_LOCAL_SYNC_STATE_BYTES + 1],
        )
        .expect("oversized state");

        assert_eq!(
            super::read_active_dejavu_binding(paths.instance_data_root(), paths.workspace_root(),)
                .expect_err("oversized state"),
            super::DejavuLocalStateError
        );
    }
}
