//! Compatibility with the host-owned DejaVu repository binding state.

use std::{collections::HashSet, fmt, io::Read, path::PathBuf};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::{
    config::KernelLaunchEpoch,
    paths::{InstanceDataRoot, WorkspaceRoot},
    runtime::{ActiveInstanceAuthority, ActiveWorkspaceAuthority},
    storage::{
        nonfollowing_read_options, unique_regular_file_identity, CommitState, DurableFileStore,
        ExpectedFile, PreservePrevious, RecoveryOutcome, ReplaceRequest, StorageFileName,
    },
    sync::dejavu_runner::DejavuRepositoryKey,
};
use qingyu_dejavu::derive_key;

const LOCAL_SYNC_STATE_FILE: &str = "local-sync.json";
const LOCAL_SYNC_STATE_VERSION: u32 = 1;
const MAX_LOCAL_SYNC_STATE_BYTES: usize = 1024 * 1024;
const MAX_DEJAVU_KEY_INPUT_BYTES: usize = 1024;
const SERVER_WORKSPACE_DISPLAY_NAME: &str = "Server notes";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct LegacyLocalSyncState {
    version: u32,
    device_id: String,
    repo_key: LegacySensitiveString,
    bindings: Vec<LegacyRepositoryBinding>,
}

struct LegacySensitiveString(Zeroizing<String>);

impl LegacySensitiveString {
    fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl<'de> Deserialize<'de> for LegacySensitiveString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer).map(|value| Self(Zeroizing::new(value)))
    }
}

impl Serialize for LegacySensitiveString {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct LegacyRepositoryBinding {
    repository_id: String,
    display_name: String,
    notes_root: PathBuf,
    enabled: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FixedLocalSyncState<'a> {
    version: u32,
    device_id: &'a str,
    repo_key: &'a str,
    bindings: [FixedRepositoryBinding<'a>; 1],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FixedRepositoryBinding<'a> {
    repository_id: &'a str,
    display_name: &'a str,
    notes_root: &'a std::path::Path,
    enabled: bool,
}

pub(crate) struct DejavuLocalRepositoryBinding {
    repository_id: String,
    display_name: String,
    device_id: String,
    repository_key: DejavuRepositoryKey,
}

impl DejavuLocalRepositoryBinding {
    pub(crate) fn into_parts(self) -> (String, String, String, DejavuRepositoryKey) {
        (
            self.repository_id,
            self.display_name,
            self.device_id,
            self.repository_key,
        )
    }
}

impl fmt::Debug for DejavuLocalRepositoryBinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DejavuLocalRepositoryBinding([REDACTED])")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DejavuLocalStateError {
    InvalidState,
    Storage,
}

/// Installs the one fixed Server repository identity exactly once.
///
/// The caller must already own both retained process locks. Existing state is
/// validated but never rewritten, including valid state that has no active
/// binding for the fixed Server workspace.
pub(crate) fn initialize_server_dejavu_binding(
    instance: &ActiveInstanceAuthority,
    workspace: &ActiveWorkspaceAuthority,
    launch_epoch: &KernelLaunchEpoch,
) -> Result<(), DejavuLocalStateError> {
    initialize_fixed_dejavu_binding(
        instance,
        workspace,
        launch_epoch,
        SERVER_WORKSPACE_DISPLAY_NAME,
    )
}

pub(crate) fn initialize_native_dejavu_binding(
    instance: &ActiveInstanceAuthority,
    workspace: &ActiveWorkspaceAuthority,
    launch_epoch: &KernelLaunchEpoch,
    display_name: &str,
) -> Result<(), DejavuLocalStateError> {
    initialize_fixed_dejavu_binding(instance, workspace, launch_epoch, display_name)
}

fn initialize_fixed_dejavu_binding(
    instance: &ActiveInstanceAuthority,
    workspace: &ActiveWorkspaceAuthority,
    launch_epoch: &KernelLaunchEpoch,
    display_name: &str,
) -> Result<(), DejavuLocalStateError> {
    verify_fixed_authorities(instance, workspace)?;
    let store = DurableFileStore::at_instance_data(instance.root(), launch_epoch)
        .map_err(|_| DejavuLocalStateError::Storage)?;
    let target = StorageFileName::parse(LOCAL_SYNC_STATE_FILE)
        .map_err(|_| DejavuLocalStateError::Storage)?;
    let recovery = store
        .recover()
        .map_err(|_| DejavuLocalStateError::Storage)?;
    if recovery.iter().any(|outcome| {
        matches!(
            outcome,
            RecoveryOutcome::Committed {
                commit_state: CommitState::PublishedDurabilityUncertain,
                ..
            } | RecoveryOutcome::ManualInterventionRequired { .. }
        )
    }) {
        return Err(DejavuLocalStateError::Storage);
    }
    if store
        .read(&target, MAX_LOCAL_SYNC_STATE_BYTES as u64)
        .map_err(|_| DejavuLocalStateError::Storage)?
        .is_some()
    {
        return require_active_fixed_binding(instance, workspace);
    }

    let repository_id = uuid::Uuid::new_v4().to_string();
    let device_id = uuid::Uuid::new_v4().to_string();
    let mut repository_key = Zeroizing::new([0_u8; 32]);
    getrandom::fill(repository_key.as_mut()).map_err(|_| DejavuLocalStateError::Storage)?;
    let repository_key_base64 = Zeroizing::new(STANDARD.encode(repository_key.as_slice()));
    let state = FixedLocalSyncState {
        version: LOCAL_SYNC_STATE_VERSION,
        device_id: &device_id,
        repo_key: repository_key_base64.as_str(),
        bindings: [FixedRepositoryBinding {
            repository_id: &repository_id,
            display_name,
            notes_root: workspace.root().canonical_path(),
            enabled: true,
        }],
    };
    let mut bytes = Zeroizing::new(
        serde_json::to_vec_pretty(&state).map_err(|_| DejavuLocalStateError::Storage)?,
    );
    if bytes.len() > MAX_LOCAL_SYNC_STATE_BYTES {
        return Err(DejavuLocalStateError::Storage);
    }
    let outcome = store
        .replace_with_address_validation(
            ReplaceRequest {
                target: &target,
                bytes: bytes.as_slice(),
                expected: ExpectedFile::Absent,
                preserve_previous: PreservePrevious::None,
            },
            || verify_fixed_authorities(instance, workspace).is_ok(),
        )
        .map_err(|_| DejavuLocalStateError::Storage)?;
    bytes.fill(0);
    if outcome.commit_state == CommitState::PublishedDurabilityUncertain {
        return Err(DejavuLocalStateError::Storage);
    }
    require_active_fixed_binding(instance, workspace)
}

fn require_active_fixed_binding(
    instance: &ActiveInstanceAuthority,
    workspace: &ActiveWorkspaceAuthority,
) -> Result<(), DejavuLocalStateError> {
    verify_fixed_authorities(instance, workspace)?;
    if read_active_dejavu_binding(instance.root(), workspace.root())?.is_none() {
        return Err(DejavuLocalStateError::InvalidState);
    }
    verify_fixed_authorities(instance, workspace)
}

fn verify_fixed_authorities(
    instance: &ActiveInstanceAuthority,
    workspace: &ActiveWorkspaceAuthority,
) -> Result<(), DejavuLocalStateError> {
    instance
        .verify_held_directory()
        .map_err(|_| DejavuLocalStateError::Storage)?;
    workspace
        .verify_held_directory()
        .map_err(|_| DejavuLocalStateError::Storage)
}

pub(crate) fn read_active_dejavu_binding(
    instance_data: &InstanceDataRoot,
    workspace: &WorkspaceRoot,
) -> Result<Option<DejavuLocalRepositoryBinding>, DejavuLocalStateError> {
    instance_data
        .verify_held_directory()
        .map_err(|_| DejavuLocalStateError::Storage)?;
    workspace
        .verify_held_directory()
        .map_err(|_| DejavuLocalStateError::Storage)?;
    let directory = instance_data
        .try_clone_dir()
        .map_err(|_| DejavuLocalStateError::Storage)?;
    let addressed = match directory.symlink_metadata(LOCAL_SYNC_STATE_FILE) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(DejavuLocalStateError::Storage),
    };
    if addressed.len() > MAX_LOCAL_SYNC_STATE_BYTES as u64 {
        return Err(DejavuLocalStateError::InvalidState);
    }
    let expected =
        unique_regular_file_identity(&addressed).ok_or(DejavuLocalStateError::Storage)?;
    let mut file = directory
        .open_with(LOCAL_SYNC_STATE_FILE, &nonfollowing_read_options())
        .map_err(|_| DejavuLocalStateError::Storage)?;
    if !expected.matches_retained_regular_file(
        &file
            .metadata()
            .map_err(|_| DejavuLocalStateError::Storage)?,
        false,
    ) {
        return Err(DejavuLocalStateError::Storage);
    }
    let mut bytes = Zeroizing::new(Vec::with_capacity(addressed.len() as usize));
    Read::by_ref(&mut file)
        .take(MAX_LOCAL_SYNC_STATE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| DejavuLocalStateError::Storage)?;
    if bytes.len() > MAX_LOCAL_SYNC_STATE_BYTES
        || !expected.matches_retained_regular_file(
            &file
                .metadata()
                .map_err(|_| DejavuLocalStateError::Storage)?,
            false,
        )
        || !expected.matches_retained_regular_file(
            &directory
                .symlink_metadata(LOCAL_SYNC_STATE_FILE)
                .map_err(|_| DejavuLocalStateError::Storage)?,
            false,
        )
    {
        return Err(DejavuLocalStateError::Storage);
    }
    instance_data
        .verify_held_directory()
        .map_err(|_| DejavuLocalStateError::Storage)?;
    workspace
        .verify_held_directory()
        .map_err(|_| DejavuLocalStateError::Storage)?;

    let state: LegacyLocalSyncState =
        serde_json::from_slice(&bytes).map_err(|_| DejavuLocalStateError::InvalidState)?;
    let LegacyLocalSyncState {
        version,
        device_id,
        repo_key,
        bindings,
    } = state;
    if version != LOCAL_SYNC_STATE_VERSION {
        return Err(DejavuLocalStateError::InvalidState);
    }
    let device =
        uuid::Uuid::parse_str(&device_id).map_err(|_| DejavuLocalStateError::InvalidState)?;
    if device.get_version() != Some(uuid::Version::Random) {
        return Err(DejavuLocalStateError::InvalidState);
    }
    let decoded = Zeroizing::new(
        STANDARD
            .decode(repo_key.as_bytes())
            .map_err(|_| DejavuLocalStateError::InvalidState)?,
    );
    let mut key = Zeroizing::new([0_u8; 32]);
    if decoded.len() != key.len() {
        return Err(DejavuLocalStateError::InvalidState);
    }
    key.copy_from_slice(decoded.as_slice());
    let mut repository_ids = HashSet::new();
    let mut roots = HashSet::new();
    let mut active = None;
    for binding in bindings {
        let repository = uuid::Uuid::parse_str(&binding.repository_id)
            .map_err(|_| DejavuLocalStateError::InvalidState)?
            .to_string();
        if repository != binding.repository_id
            || !binding.notes_root.is_absolute()
            || !repository_ids.insert(repository.clone())
            || !roots.insert(binding.notes_root.clone())
        {
            return Err(DejavuLocalStateError::InvalidState);
        }
        if binding.enabled && binding.notes_root == workspace.canonical_path() {
            if active.is_some() {
                return Err(DejavuLocalStateError::InvalidState);
            }
            active = Some((repository, binding.display_name));
        }
    }
    Ok(active.map(
        |(repository_id, display_name)| DejavuLocalRepositoryBinding {
            repository_id,
            display_name,
            device_id: device.to_string(),
            repository_key: DejavuRepositoryKey::new(*key),
        },
    ))
}

pub(crate) fn bind_dejavu_repository(
    instance: &ActiveInstanceAuthority,
    workspace: &ActiveWorkspaceAuthority,
    launch_epoch: &KernelLaunchEpoch,
    repository_id: &str,
    display_name: &str,
) -> Result<(), DejavuLocalStateError> {
    verify_fixed_authorities(instance, workspace)?;
    let repository_id = canonical_repository_id(repository_id)?;
    let display_name = display_name.trim();
    if display_name.is_empty()
        || display_name.len() > 255
        || display_name.chars().any(char::is_control)
    {
        return Err(DejavuLocalStateError::InvalidState);
    }
    let (store, target, mut state, revision) = load_state_for_update(instance, launch_epoch)?;
    validate_local_state(&state)?;
    let notes_root = workspace.root().canonical_path().to_path_buf();
    state.bindings.retain(|binding| {
        binding.repository_id == repository_id || binding.notes_root != notes_root
    });
    if let Some(binding) = state
        .bindings
        .iter_mut()
        .find(|binding| binding.repository_id == repository_id)
    {
        binding.display_name = display_name.to_owned();
        binding.notes_root = notes_root;
        binding.enabled = true;
    } else {
        state.bindings.push(LegacyRepositoryBinding {
            repository_id,
            display_name: display_name.to_owned(),
            notes_root,
            enabled: true,
        });
    }
    validate_local_state(&state)?;
    save_state(&store, &target, &state, &revision, || {
        verify_fixed_authorities(instance, workspace).is_ok()
    })?;
    require_active_fixed_binding(instance, workspace)
}

pub(crate) fn dejavu_key_configured(
    instance: &ActiveInstanceAuthority,
    launch_epoch: &KernelLaunchEpoch,
) -> Result<bool, DejavuLocalStateError> {
    verify_instance_authority(instance)?;
    let (store, target) = local_state_store(instance, launch_epoch)?;
    recover_state_store(&store)?;
    let Some(stored) = store
        .read(&target, MAX_LOCAL_SYNC_STATE_BYTES as u64)
        .map_err(|_| DejavuLocalStateError::Storage)?
    else {
        return Ok(false);
    };
    let state: LegacyLocalSyncState =
        serde_json::from_slice(&stored.bytes).map_err(|_| DejavuLocalStateError::InvalidState)?;
    validate_local_state(&state)?;
    verify_instance_authority(instance)?;
    Ok(true)
}

pub(crate) fn export_dejavu_key(
    instance: &ActiveInstanceAuthority,
    launch_epoch: &KernelLaunchEpoch,
) -> Result<String, DejavuLocalStateError> {
    let (_store, _target, state, _revision) = load_state_for_update(instance, launch_epoch)?;
    validate_local_state(&state)?;
    verify_instance_authority(instance)?;
    Ok(state.repo_key.0.to_string())
}

pub(crate) fn replace_dejavu_key(
    instance: &ActiveInstanceAuthority,
    launch_epoch: &KernelLaunchEpoch,
    user_key_input: &str,
) -> Result<(), DejavuLocalStateError> {
    if user_key_input.trim().is_empty() || user_key_input.len() > MAX_DEJAVU_KEY_INPUT_BYTES {
        return Err(DejavuLocalStateError::InvalidState);
    }
    let (store, target, mut state, revision) = load_state_for_update(instance, launch_epoch)?;
    validate_local_state(&state)?;
    state.repo_key = LegacySensitiveString(Zeroizing::new(derive_repository_key(user_key_input)?));
    for binding in &mut state.bindings {
        binding.enabled = false;
    }
    validate_local_state(&state)?;
    save_state(&store, &target, &state, &revision, || {
        verify_instance_authority(instance).is_ok()
    })
}

fn canonical_repository_id(repository_id: &str) -> Result<String, DejavuLocalStateError> {
    let parsed =
        uuid::Uuid::parse_str(repository_id).map_err(|_| DejavuLocalStateError::InvalidState)?;
    if parsed.to_string() != repository_id {
        return Err(DejavuLocalStateError::InvalidState);
    }
    Ok(repository_id.to_owned())
}

fn validate_local_state(state: &LegacyLocalSyncState) -> Result<(), DejavuLocalStateError> {
    if state.version != LOCAL_SYNC_STATE_VERSION {
        return Err(DejavuLocalStateError::InvalidState);
    }
    canonical_repository_id_for_device(&state.device_id)?;
    let decoded = Zeroizing::new(
        STANDARD
            .decode(state.repo_key.as_bytes())
            .map_err(|_| DejavuLocalStateError::InvalidState)?,
    );
    if decoded.len() != 32 {
        return Err(DejavuLocalStateError::InvalidState);
    }
    let mut repositories = HashSet::new();
    let mut roots = HashSet::new();
    for binding in &state.bindings {
        canonical_repository_id(&binding.repository_id)?;
        if !binding.notes_root.is_absolute()
            || !repositories.insert(binding.repository_id.as_str())
            || !roots.insert(binding.notes_root.as_path())
        {
            return Err(DejavuLocalStateError::InvalidState);
        }
    }
    Ok(())
}

fn canonical_repository_id_for_device(device_id: &str) -> Result<(), DejavuLocalStateError> {
    let parsed =
        uuid::Uuid::parse_str(device_id).map_err(|_| DejavuLocalStateError::InvalidState)?;
    if parsed.get_version() != Some(uuid::Version::Random) {
        return Err(DejavuLocalStateError::InvalidState);
    }
    Ok(())
}

fn derive_repository_key(input: &str) -> Result<String, DejavuLocalStateError> {
    if let Ok(decoded) = STANDARD.decode(input) {
        if decoded.len() == 32 {
            return Ok(input.to_owned());
        }
    }
    let digest = format!("{:x}", Sha256::digest(input.as_bytes()));
    let key = derive_key(input, &digest[..16]).map_err(|_| DejavuLocalStateError::Storage)?;
    Ok(STANDARD.encode(key))
}

fn local_state_store(
    instance: &ActiveInstanceAuthority,
    launch_epoch: &KernelLaunchEpoch,
) -> Result<(DurableFileStore, StorageFileName), DejavuLocalStateError> {
    verify_instance_authority(instance)?;
    let store = DurableFileStore::at_instance_data(instance.root(), launch_epoch)
        .map_err(|_| DejavuLocalStateError::Storage)?;
    let target = StorageFileName::parse(LOCAL_SYNC_STATE_FILE)
        .map_err(|_| DejavuLocalStateError::Storage)?;
    Ok((store, target))
}

fn recover_state_store(store: &DurableFileStore) -> Result<(), DejavuLocalStateError> {
    let recovery = store
        .recover()
        .map_err(|_| DejavuLocalStateError::Storage)?;
    if recovery.iter().any(|outcome| {
        matches!(
            outcome,
            RecoveryOutcome::Committed {
                commit_state: CommitState::PublishedDurabilityUncertain,
                ..
            } | RecoveryOutcome::ManualInterventionRequired { .. }
        )
    }) {
        Err(DejavuLocalStateError::Storage)
    } else {
        Ok(())
    }
}

fn load_state_for_update(
    instance: &ActiveInstanceAuthority,
    launch_epoch: &KernelLaunchEpoch,
) -> Result<
    (
        DurableFileStore,
        StorageFileName,
        LegacyLocalSyncState,
        crate::storage::FileRevision,
    ),
    DejavuLocalStateError,
> {
    let (store, target) = local_state_store(instance, launch_epoch)?;
    recover_state_store(&store)?;
    let stored = store
        .read(&target, MAX_LOCAL_SYNC_STATE_BYTES as u64)
        .map_err(|_| DejavuLocalStateError::Storage)?
        .ok_or(DejavuLocalStateError::InvalidState)?;
    let state =
        serde_json::from_slice(&stored.bytes).map_err(|_| DejavuLocalStateError::InvalidState)?;
    verify_instance_authority(instance)?;
    Ok((store, target, state, stored.revision.clone()))
}

fn save_state<Validate>(
    store: &DurableFileStore,
    target: &StorageFileName,
    state: &LegacyLocalSyncState,
    revision: &crate::storage::FileRevision,
    validate_address: Validate,
) -> Result<(), DejavuLocalStateError>
where
    Validate: FnMut() -> bool,
{
    let mut bytes = Zeroizing::new(
        serde_json::to_vec_pretty(state).map_err(|_| DejavuLocalStateError::InvalidState)?,
    );
    if bytes.len() > MAX_LOCAL_SYNC_STATE_BYTES {
        return Err(DejavuLocalStateError::InvalidState);
    }
    let outcome = store
        .replace_with_address_validation(
            ReplaceRequest {
                target,
                bytes: bytes.as_slice(),
                expected: ExpectedFile::Revision(revision),
                preserve_previous: PreservePrevious::None,
            },
            validate_address,
        )
        .map_err(|_| DejavuLocalStateError::Storage)?;
    bytes.fill(0);
    if outcome.commit_state == CommitState::PublishedDurabilityUncertain {
        Err(DejavuLocalStateError::Storage)
    } else {
        Ok(())
    }
}

fn verify_instance_authority(
    instance: &ActiveInstanceAuthority,
) -> Result<(), DejavuLocalStateError> {
    instance
        .verify_held_directory()
        .map_err(|_| DejavuLocalStateError::Storage)
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use serde_json::json;
    use tempfile::tempdir;

    use crate::{
        config::KernelConfig,
        paths::{KernelPaths, ServerPathLayout},
        ports::system::system_kernel_ports,
        runtime::KernelRuntime,
        storage::{
            DurableFileFailureKind, DurableFileStore, DurableFileTestFault, ExpectedFile,
            PreservePrevious, ReplaceRequest, StorageFileName,
        },
    };

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
    fn server_initialization_recovers_a_prior_epoch_intent_before_authoritative_validation() {
        let temporary = tempdir().expect("fixture root");
        let data = temporary.path().join("data");
        let cache = temporary.path().join("cache");
        fs::create_dir(&data).expect("Server data");
        let paths = ServerPathLayout::for_test(&data, &cache)
            .activate()
            .expect("Server paths");
        let target_path = data.join("state/local-sync.json");
        let initial =
            serde_json::to_vec_pretty(&state_value(paths.workspace_root().canonical_path(), true))
                .expect("initial state");
        fs::write(&target_path, &initial).expect("initial binding");

        let prior = KernelConfig::generate().expect("prior launch config");
        let store = DurableFileStore::at_instance_data_with_test_fault(
            paths.instance_data_root(),
            prior.launch_epoch(),
            DurableFileTestFault::LeavePrepared,
        )
        .expect("prior durable store");
        let target = StorageFileName::parse("local-sync.json").expect("state target");
        let revision = store
            .read(&target, super::MAX_LOCAL_SYNC_STATE_BYTES as u64)
            .expect("read prior binding")
            .expect("prior binding")
            .revision
            .clone();
        let mut replacement = state_value(paths.workspace_root().canonical_path(), true);
        replacement["deviceId"] = json!("12c088ee-e331-4f08-bb2a-cd019947a99c");
        replacement["bindings"][0]["repositoryId"] = json!("c8059130-fe5e-4a71-94fb-f580c80a7302");
        let replacement = serde_json::to_vec_pretty(&replacement).expect("replacement state");
        let error = store
            .replace(ReplaceRequest {
                target: &target,
                bytes: &replacement,
                expected: ExpectedFile::Revision(&revision),
                preserve_previous: PreservePrevious::None,
            })
            .expect_err("simulated prior crash leaves a prepared intent");
        assert_eq!(error.kind(), DurableFileFailureKind::PublishStateUncertain);
        drop(store);
        drop(paths);
        assert_eq!(storage_artifact_count(&data.join("state")), 2);

        let current_paths = ServerPathLayout::for_test(&data, &cache)
            .activate()
            .expect("current Server paths");
        let runtime = KernelRuntime::activate(
            KernelConfig::generate().expect("current launch config"),
            current_paths,
            system_kernel_ports(),
        )
        .expect("locked Server runtime");
        let instance = runtime.active_instance_authority();
        let workspace = runtime
            .active_workspace_authority()
            .expect("locked Server workspace");

        super::initialize_server_dejavu_binding(
            instance.as_ref(),
            workspace.as_ref(),
            runtime.launch_epoch(),
        )
        .expect("recovered Server binding");

        assert_eq!(storage_artifact_count(&data.join("state")), 0);
        assert_eq!(
            fs::read(&target_path).expect("authoritative state"),
            initial
        );
        let (repository_id, _display_name, device_id, _) =
            super::read_active_dejavu_binding(runtime.instance_data_root(), workspace.root())
                .expect("valid recovered state")
                .expect("active recovered binding")
                .into_parts();
        assert_eq!(repository_id, REPOSITORY_ID);
        assert_eq!(device_id, DEVICE_ID);
    }

    fn storage_artifact_count(state: &Path) -> usize {
        fs::read_dir(state)
            .expect("state entries")
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| {
                name.starts_with(".qingyu-storage-")
                    && (name.ends_with(".intent") || name.ends_with(".stage"))
            })
            .count()
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
        let (actual_repository, actual_display_name, actual_device, key) = binding.into_parts();
        assert_eq!(actual_repository, REPOSITORY_ID);
        assert_eq!(actual_display_name, "Private notes");
        assert_eq!(actual_device, DEVICE_ID);
        assert_eq!(format!("{key:?}"), "DejavuRepositoryKey([REDACTED])");
    }

    #[test]
    fn reads_a_canonical_legacy_repository_uuid_without_rewriting_profile_bytes() {
        let temporary = tempdir().expect("fixture root");
        let (app_data, paths) = paths(&temporary);
        let mut state = state_value(paths.workspace_root().canonical_path(), true);
        state["deviceId"] = json!(DEVICE_ID.to_uppercase());
        state["bindings"][0]["repositoryId"] = json!("00000000-0000-1000-8000-000000000001");
        let original = serde_json::to_vec_pretty(&state).expect("legacy state JSON");
        let state_path = app_data.join("local-sync.json");
        fs::write(&state_path, &original).expect("legacy state file");

        let binding =
            super::read_active_dejavu_binding(paths.instance_data_root(), paths.workspace_root())
                .expect("legacy state remains compatible")
                .expect("active legacy binding");
        let (repository_id, _display_name, device_id, _key) = binding.into_parts();

        assert_eq!(repository_id, "00000000-0000-1000-8000-000000000001");
        assert_eq!(device_id, DEVICE_ID);
        assert_eq!(
            fs::read(state_path).expect("unchanged legacy state"),
            original
        );
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
                super::DejavuLocalStateError::InvalidState
            );
        }
    }

    #[test]
    fn explicit_repository_binding_preserves_legacy_device_and_key_then_key_import_disables_it() {
        const SELECTED_REPOSITORY_ID: &str = "9d941f26-28a7-46bc-a3a4-8f6c66a84583";

        let temporary = tempdir().expect("fixture root");
        let (app_data, paths) = paths(&temporary);
        let original = state_value(paths.workspace_root().canonical_path(), true);
        fs::write(
            app_data.join("local-sync.json"),
            serde_json::to_vec_pretty(&original).expect("state JSON"),
        )
        .expect("state file");
        let runtime = KernelRuntime::activate(
            KernelConfig::generate().expect("Kernel config"),
            paths,
            system_kernel_ports(),
        )
        .expect("locked runtime");
        let instance = runtime.active_instance_authority();
        let workspace = runtime
            .active_workspace_authority()
            .expect("workspace authority");

        super::bind_dejavu_repository(
            instance.as_ref(),
            workspace.as_ref(),
            runtime.launch_epoch(),
            SELECTED_REPOSITORY_ID,
            "Selected remote",
        )
        .expect("explicit binding");

        let rebound: serde_json::Value = serde_json::from_slice(
            &fs::read(app_data.join("local-sync.json")).expect("rebound state"),
        )
        .expect("rebound JSON");
        assert_eq!(rebound["deviceId"], original["deviceId"]);
        assert_eq!(rebound["repoKey"], original["repoKey"]);
        assert_eq!(rebound["bindings"].as_array().unwrap().len(), 1);
        assert_eq!(
            rebound["bindings"][0]["repositoryId"],
            SELECTED_REPOSITORY_ID
        );
        assert_eq!(
            rebound["bindings"][0]["notesRoot"],
            paths_for_assertion(&workspace)
        );
        assert_eq!(
            super::export_dejavu_key(instance.as_ref(), runtime.launch_epoch())
                .expect("exported key"),
            original["repoKey"].as_str().unwrap()
        );

        super::replace_dejavu_key(
            instance.as_ref(),
            runtime.launch_epoch(),
            "portable key phrase",
        )
        .expect("key import");
        let replaced: serde_json::Value = serde_json::from_slice(
            &fs::read(app_data.join("local-sync.json")).expect("replaced state"),
        )
        .expect("replaced JSON");
        assert_ne!(replaced["repoKey"], original["repoKey"]);
        assert_eq!(replaced["deviceId"], original["deviceId"]);
        assert_eq!(replaced["bindings"][0]["enabled"], false);
    }

    #[test]
    fn invalid_repository_selection_and_empty_key_leave_legacy_state_byte_exact() {
        let temporary = tempdir().expect("fixture root");
        let (app_data, paths) = paths(&temporary);
        let original =
            serde_json::to_vec_pretty(&state_value(paths.workspace_root().canonical_path(), true))
                .expect("state JSON");
        let state_path = app_data.join("local-sync.json");
        fs::write(&state_path, &original).expect("state file");
        let runtime = KernelRuntime::activate(
            KernelConfig::generate().expect("Kernel config"),
            paths,
            system_kernel_ports(),
        )
        .expect("locked runtime");
        let instance = runtime.active_instance_authority();
        let workspace = runtime
            .active_workspace_authority()
            .expect("workspace authority");

        for (repository_id, display_name) in [
            ("not-a-repository", "Remote"),
            ("9d941f26-28a7-46bc-a3a4-8f6c66a84583", "  \n  "),
        ] {
            assert_eq!(
                super::bind_dejavu_repository(
                    instance.as_ref(),
                    workspace.as_ref(),
                    runtime.launch_epoch(),
                    repository_id,
                    display_name,
                )
                .expect_err("invalid selection must fail closed"),
                super::DejavuLocalStateError::InvalidState
            );
            assert_eq!(fs::read(&state_path).expect("unchanged state"), original);
        }
        for invalid_key in ["   ".to_owned(), "x".repeat(1025)] {
            assert_eq!(
                super::replace_dejavu_key(instance.as_ref(), runtime.launch_epoch(), &invalid_key,)
                    .expect_err("invalid key must fail closed"),
                super::DejavuLocalStateError::InvalidState
            );
            assert_eq!(fs::read(&state_path).expect("unchanged state"), original);
        }
    }

    fn paths_for_assertion(
        workspace: &crate::runtime::ActiveWorkspaceAuthority,
    ) -> serde_json::Value {
        serde_json::to_value(workspace.root().canonical_path()).expect("workspace path JSON")
    }

    #[test]
    fn malformed_inactive_repository_id_invalidates_an_otherwise_active_state() {
        let temporary = tempdir().expect("fixture root");
        let (app_data, paths) = paths(&temporary);
        let mut state = state_value(paths.workspace_root().canonical_path(), true);
        state["bindings"]
            .as_array_mut()
            .expect("bindings array")
            .push(json!({
                "repositoryId": "not-a-repository-uuid",
                "displayName": "Malformed inactive binding",
                "notesRoot": "/tmp/inactive",
                "enabled": false
            }));
        fs::write(
            app_data.join("local-sync.json"),
            serde_json::to_vec(&state).expect("state JSON"),
        )
        .expect("state file");

        assert_eq!(
            super::read_active_dejavu_binding(paths.instance_data_root(), paths.workspace_root(),)
                .expect_err("every binding identifier must be valid"),
            super::DejavuLocalStateError::InvalidState
        );
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
                super::DejavuLocalStateError::Storage
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
            super::DejavuLocalStateError::InvalidState
        );
    }
}
