use std::collections::HashSet;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use qingyu_dejavu::{derive_key, write_file_safer};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::storage_capability::{nonfollowing_read_options, unique_regular_file_identity};
use crate::sync_config::storage::open_app_data;

// Task 3 wires these staged internal API items into the background sync runtime.
#[cfg_attr(not(test), allow(dead_code))]
const LOCAL_SYNC_STATE_VERSION: u32 = 1;
#[cfg_attr(not(test), allow(dead_code))]
const LOCAL_SYNC_STATE_FILE: &str = "dejavu-sync-local-state.json";
#[cfg_attr(not(test), allow(dead_code))]
const MAX_LOCAL_SYNC_STATE_BYTES: usize = 1024 * 1024;

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LocalSyncState {
    pub(crate) version: u32,
    pub(crate) device_id: String,
    pub(crate) repo_key: String,
    pub(crate) bindings: Vec<RepositoryBinding>,
}

impl fmt::Debug for LocalSyncState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalSyncState")
            .field("version", &self.version)
            .field("device_id", &self.device_id)
            .field("repo_key", &"[REDACTED]")
            .field("bindings", &self.bindings)
            .finish()
    }
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RepositoryBinding {
    pub(crate) repository_id: String,
    pub(crate) display_name: String,
    pub(crate) notes_root: PathBuf,
    pub(crate) enabled: bool,
}

impl fmt::Debug for RepositoryBinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RepositoryBinding")
            .field("repository_id", &self.repository_id)
            .field("display_name", &self.display_name)
            .field("notes_root", &self.notes_root)
            .field("enabled", &self.enabled)
            .finish()
    }
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LocalSyncStateError {
    code: &'static str,
    message: &'static str,
}

impl LocalSyncStateError {
    fn new(code: &'static str, message: &'static str) -> Self {
        Self { code, message }
    }

    fn unsafe_path() -> Self {
        Self::new(
            "local-sync-state-unsafe-path",
            "The local sync state path is unsafe.",
        )
    }

    fn read_failed() -> Self {
        Self::new(
            "local-sync-state-read-failed",
            "The local sync state could not be read.",
        )
    }

    fn malformed() -> Self {
        Self::new(
            "local-sync-state-malformed",
            "The local sync state is malformed.",
        )
    }

    fn too_large() -> Self {
        Self::new(
            "local-sync-state-too-large",
            "The local sync state exceeds the supported size limit.",
        )
    }

    fn invalid_binding() -> Self {
        Self::new(
            "local-sync-state-invalid-binding",
            "The repository binding is invalid.",
        )
    }

    fn duplicate_repository() -> Self {
        Self::new(
            "local-sync-state-duplicate-repository",
            "The repository is already bound.",
        )
    }

    fn duplicate_root() -> Self {
        Self::new(
            "local-sync-state-duplicate-root",
            "The note root is already bound.",
        )
    }

    fn random_failed() -> Self {
        Self::new(
            "local-sync-state-random-failed",
            "The local sync key could not be generated.",
        )
    }

    fn derive_failed() -> Self {
        Self::new(
            "local-sync-state-key-derivation-failed",
            "The local sync key could not be derived.",
        )
    }

    fn write_failed() -> Self {
        Self::new(
            "local-sync-state-write-failed",
            "The local sync state could not be written.",
        )
    }
}

impl fmt::Display for LocalSyncStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for LocalSyncStateError {}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Debug)]
pub(crate) struct LocalSyncStateService {
    app_data: PathBuf,
}

#[cfg_attr(not(test), allow(dead_code))]
impl LocalSyncStateService {
    pub(crate) fn new(app_data: impl AsRef<Path>) -> Self {
        Self {
            app_data: app_data.as_ref().to_path_buf(),
        }
    }

    pub(crate) fn load_or_initialize(
        &self,
        user_key_input: Option<&str>,
    ) -> Result<LocalSyncState, LocalSyncStateError> {
        if let Some(state) = self.load()? {
            return Ok(state);
        }

        let state = LocalSyncState {
            version: LOCAL_SYNC_STATE_VERSION,
            device_id: uuid::Uuid::new_v4().to_string(),
            repo_key: repository_key(user_key_input)?,
            bindings: Vec::new(),
        };
        self.save(&state)?;
        Ok(state)
    }

    pub(crate) fn load(&self) -> Result<Option<LocalSyncState>, LocalSyncStateError> {
        let Some(app_data) =
            open_app_data(&self.app_data, false).map_err(|_| LocalSyncStateError::unsafe_path())?
        else {
            return Ok(None);
        };
        app_data
            .revalidate()
            .map_err(|_| LocalSyncStateError::unsafe_path())?;
        let addressed = match app_data.directory().symlink_metadata(LOCAL_SYNC_STATE_FILE) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                return Err(LocalSyncStateError::unsafe_path());
            }
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(LocalSyncStateError::read_failed()),
        };
        if addressed.len() > MAX_LOCAL_SYNC_STATE_BYTES as u64 {
            return Err(LocalSyncStateError::too_large());
        }
        let addressed_identity = unique_regular_file_identity(&addressed)
            .ok_or_else(LocalSyncStateError::unsafe_path)?;
        let mut file = app_data
            .directory()
            .open_with(LOCAL_SYNC_STATE_FILE, &nonfollowing_read_options())
            .map_err(|_| LocalSyncStateError::unsafe_path())?;
        let retained = file
            .metadata()
            .map_err(|_| LocalSyncStateError::read_failed())?;
        if unique_regular_file_identity(&retained) != Some(addressed_identity) {
            return Err(LocalSyncStateError::unsafe_path());
        }
        let mut bytes = Vec::with_capacity(retained.len() as usize);
        Read::by_ref(&mut file)
            .take(MAX_LOCAL_SYNC_STATE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| LocalSyncStateError::read_failed())?;
        if bytes.len() > MAX_LOCAL_SYNC_STATE_BYTES {
            return Err(LocalSyncStateError::too_large());
        }
        let final_metadata = file
            .metadata()
            .map_err(|_| LocalSyncStateError::read_failed())?;
        if unique_regular_file_identity(&final_metadata) != Some(addressed_identity) {
            return Err(LocalSyncStateError::unsafe_path());
        }
        app_data
            .revalidate()
            .map_err(|_| LocalSyncStateError::unsafe_path())?;

        let state = serde_json::from_slice::<LocalSyncState>(&bytes)
            .map_err(|_| LocalSyncStateError::malformed())?;
        validate_state(&state)?;
        Ok(Some(state))
    }

    pub(crate) fn save(&self, state: &LocalSyncState) -> Result<(), LocalSyncStateError> {
        self.save_with_writer(state, |path, bytes, mode| {
            write_file_safer(path, bytes, mode).map_err(|_| LocalSyncStateError::write_failed())
        })
    }

    pub(crate) fn add_binding(
        &self,
        state: &mut LocalSyncState,
        mut binding: RepositoryBinding,
    ) -> Result<(), LocalSyncStateError> {
        let metadata = std::fs::symlink_metadata(&binding.notes_root)
            .map_err(|_| LocalSyncStateError::invalid_binding())?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(LocalSyncStateError::invalid_binding());
        }
        binding.notes_root = binding
            .notes_root
            .canonicalize()
            .map_err(|_| LocalSyncStateError::invalid_binding())?;
        if state
            .bindings
            .iter()
            .any(|existing| existing.repository_id == binding.repository_id)
        {
            return Err(LocalSyncStateError::duplicate_repository());
        }
        if state
            .bindings
            .iter()
            .any(|existing| existing.notes_root == binding.notes_root)
        {
            return Err(LocalSyncStateError::duplicate_root());
        }
        let mut updated = state.clone();
        updated.bindings.push(binding);
        self.save(&updated)?;
        *state = updated;
        Ok(())
    }

    fn save_with_writer<Write>(
        &self,
        state: &LocalSyncState,
        writer: Write,
    ) -> Result<(), LocalSyncStateError>
    where
        Write: FnOnce(&Path, &[u8], u32) -> Result<(), LocalSyncStateError>,
    {
        validate_state(state)?;
        let mut bytes =
            serde_json::to_vec_pretty(state).map_err(|_| LocalSyncStateError::write_failed())?;
        bytes.push(b'\n');
        if bytes.len() > MAX_LOCAL_SYNC_STATE_BYTES {
            return Err(LocalSyncStateError::too_large());
        }

        let app_data = open_app_data(&self.app_data, true)
            .map_err(|_| LocalSyncStateError::unsafe_path())?
            .ok_or_else(LocalSyncStateError::unsafe_path)?;
        match app_data.directory().symlink_metadata(LOCAL_SYNC_STATE_FILE) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                return Err(LocalSyncStateError::unsafe_path());
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(LocalSyncStateError::write_failed()),
        }
        app_data
            .revalidate()
            .map_err(|_| LocalSyncStateError::unsafe_path())?;
        writer(
            &app_data.canonical_path().join(LOCAL_SYNC_STATE_FILE),
            &bytes,
            0o600,
        )?;
        app_data
            .revalidate()
            .map_err(|_| LocalSyncStateError::unsafe_path())?;
        Ok(())
    }
}

#[cfg_attr(not(test), allow(dead_code))]
fn repository_key(user_key_input: Option<&str>) -> Result<String, LocalSyncStateError> {
    if let Some(input) = user_key_input {
        if let Ok(decoded) = STANDARD.decode(input) {
            if decoded.len() == 32 {
                return Ok(input.to_string());
            }
        }
        let digest = format!("{:x}", Sha256::digest(input.as_bytes()));
        let derived =
            derive_key(input, &digest[..16]).map_err(|_| LocalSyncStateError::derive_failed())?;
        return Ok(STANDARD.encode(derived));
    }

    let mut generated = [0_u8; 32];
    getrandom::fill(&mut generated).map_err(|_| LocalSyncStateError::random_failed())?;
    Ok(STANDARD.encode(generated))
}

#[cfg_attr(not(test), allow(dead_code))]
fn validate_state(state: &LocalSyncState) -> Result<(), LocalSyncStateError> {
    if state.version != LOCAL_SYNC_STATE_VERSION {
        return Err(LocalSyncStateError::malformed());
    }
    let device_id =
        uuid::Uuid::parse_str(&state.device_id).map_err(|_| LocalSyncStateError::malformed())?;
    if device_id.get_version() != Some(uuid::Version::Random) {
        return Err(LocalSyncStateError::malformed());
    }
    if STANDARD
        .decode(&state.repo_key)
        .ok()
        .filter(|key| key.len() == 32)
        .is_none()
    {
        return Err(LocalSyncStateError::malformed());
    }

    let mut repositories = HashSet::new();
    let mut roots = HashSet::new();
    for binding in &state.bindings {
        if !binding.notes_root.is_absolute() {
            return Err(LocalSyncStateError::invalid_binding());
        }
        if !repositories.insert(binding.repository_id.as_str()) {
            return Err(LocalSyncStateError::duplicate_repository());
        }
        if !roots.insert(binding.notes_root.as_path()) {
            return Err(LocalSyncStateError::duplicate_root());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use qingyu_dejavu::derive_key;
    use tempfile::tempdir;
    use uuid::{Uuid, Version};

    use super::{
        LocalSyncStateError, LocalSyncStateService, RepositoryBinding, LOCAL_SYNC_STATE_FILE,
        MAX_LOCAL_SYNC_STATE_BYTES,
    };

    #[test]
    fn absent_state_initializes_versioned_local_identity_and_pretty_json() {
        let temporary = tempdir().expect("temporary app data");
        let service = LocalSyncStateService::new(temporary.path());
        let imported = STANDARD.encode([3_u8; 32]);

        let state = service
            .load_or_initialize(Some(&imported))
            .expect("absent state should initialize");

        assert_eq!(state.version, 1);
        assert_eq!(
            Uuid::parse_str(&state.device_id)
                .expect("device id should be a UUID")
                .get_version(),
            Some(Version::Random)
        );
        assert!(state.bindings.is_empty());
        let stored = fs::read_to_string(temporary.path().join(LOCAL_SYNC_STATE_FILE))
            .expect("state should be persisted");
        assert!(stored.ends_with('\n'));
        assert!(stored.contains("\n  \"deviceId\":"));
        assert!(stored.contains("\n  \"repoKey\":"));
        assert!(!stored.contains("device_id"));
    }

    #[test]
    fn exact_32_byte_standard_base64_input_is_imported_without_derivation() {
        let temporary = tempdir().expect("temporary app data");
        let service = LocalSyncStateService::new(temporary.path());
        let imported = STANDARD.encode([0x5a_u8; 32]);

        let state = service
            .load_or_initialize(Some(&imported))
            .expect("base64 key should import");

        assert_eq!(state.repo_key, imported);
        assert_eq!(STANDARD.decode(&state.repo_key).unwrap(), [0x5a_u8; 32]);
    }

    #[test]
    fn non_key_input_derives_from_the_sha256_prefix_salt() {
        let temporary = tempdir().expect("temporary app data");
        let service = LocalSyncStateService::new(temporary.path());
        let expected = STANDARD.encode(
            derive_key("correct horse battery staple", "c4bbcb1fbec99d65")
                .expect("fixture derivation"),
        );

        let state = service
            .load_or_initialize(Some("correct horse battery staple"))
            .expect("passphrase should derive");

        assert_eq!(state.repo_key, expected);
    }

    #[test]
    fn missing_key_input_generates_independent_random_32_byte_keys() {
        let first_root = tempdir().expect("first app data");
        let second_root = tempdir().expect("second app data");

        let first = LocalSyncStateService::new(first_root.path())
            .load_or_initialize(None)
            .expect("first random state");
        let second = LocalSyncStateService::new(second_root.path())
            .load_or_initialize(None)
            .expect("second random state");

        assert_eq!(STANDARD.decode(&first.repo_key).unwrap().len(), 32);
        assert_eq!(STANDARD.decode(&second.repo_key).unwrap().len(), 32);
        assert_ne!(first.repo_key, second.repo_key);
        assert_ne!(first.device_id, second.device_id);
    }

    #[test]
    fn adding_bindings_rejects_duplicate_repository_ids_and_note_roots() {
        let temporary = tempdir().expect("temporary app data");
        let notes_a = temporary.path().join("notes-a");
        let notes_b = temporary.path().join("notes-b");
        fs::create_dir(&notes_a).unwrap();
        fs::create_dir(&notes_b).unwrap();
        let service = LocalSyncStateService::new(temporary.path().join("app-data"));
        let mut state = service.load_or_initialize(None).unwrap();

        service
            .add_binding(
                &mut state,
                RepositoryBinding {
                    repository_id: "repo-a".to_string(),
                    display_name: "Notes A".to_string(),
                    notes_root: notes_a.clone(),
                    enabled: true,
                },
            )
            .unwrap();

        let duplicate_id = service.add_binding(
            &mut state,
            RepositoryBinding {
                repository_id: "repo-a".to_string(),
                display_name: "Notes B".to_string(),
                notes_root: notes_b,
                enabled: true,
            },
        );
        assert!(duplicate_id.is_err());

        let duplicate_root = service.add_binding(
            &mut state,
            RepositoryBinding {
                repository_id: "repo-b".to_string(),
                display_name: "Notes A Again".to_string(),
                notes_root: notes_a.canonicalize().unwrap(),
                enabled: true,
            },
        );
        assert!(duplicate_root.is_err());
        assert_eq!(state.bindings.len(), 1);
        assert_eq!(
            state.bindings[0].notes_root,
            notes_a.canonicalize().unwrap()
        );
    }

    #[test]
    fn malformed_or_unknown_state_is_rejected_without_echoing_the_key() {
        let temporary = tempdir().expect("temporary app data");
        let secret = STANDARD.encode([0x7b_u8; 32]);
        let path = temporary.path().join(LOCAL_SYNC_STATE_FILE);
        fs::write(
            &path,
            format!(
                "{{\"version\":1,\"deviceId\":\"{}\",\"repoKey\":\"{secret}\",\"bindings\":[],\"unexpected\":true}}",
                Uuid::new_v4()
            ),
        )
        .unwrap();

        let error = LocalSyncStateService::new(temporary.path())
            .load_or_initialize(None)
            .expect_err("unknown fields must be rejected");

        assert!(!format!("{error:?}").contains(&secret));
        assert!(!error.to_string().contains(&secret));
    }

    #[test]
    fn oversized_state_is_rejected_without_reading_it_as_json() {
        let temporary = tempdir().expect("temporary app data");
        let path = temporary.path().join(LOCAL_SYNC_STATE_FILE);
        fs::write(&path, vec![b'x'; MAX_LOCAL_SYNC_STATE_BYTES + 1]).unwrap();

        let error = LocalSyncStateService::new(temporary.path())
            .load_or_initialize(None)
            .expect_err("oversized state must be rejected");

        assert!(error.to_string().contains("too-large"));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_and_non_file_state_destinations_are_rejected() {
        use std::os::unix::fs::symlink;

        let symlink_root = tempdir().expect("symlink app data");
        let outside = symlink_root.path().join("outside.json");
        fs::write(&outside, b"outside").unwrap();
        symlink(&outside, symlink_root.path().join(LOCAL_SYNC_STATE_FILE)).unwrap();
        assert!(LocalSyncStateService::new(symlink_root.path())
            .load_or_initialize(None)
            .is_err());
        assert_eq!(fs::read(&outside).unwrap(), b"outside");

        let directory_root = tempdir().expect("directory app data");
        fs::create_dir(directory_root.path().join(LOCAL_SYNC_STATE_FILE)).unwrap();
        assert!(LocalSyncStateService::new(directory_root.path())
            .load_or_initialize(None)
            .is_err());
    }

    #[cfg(unix)]
    #[test]
    fn saving_replaces_the_prior_state_atomically() {
        use std::os::unix::fs::MetadataExt;

        let temporary = tempdir().expect("temporary app data");
        let service = LocalSyncStateService::new(temporary.path());
        let mut state = service.load_or_initialize(None).unwrap();
        let path = temporary.path().join(LOCAL_SYNC_STATE_FILE);
        let prior_inode = fs::metadata(&path).unwrap().ino();
        state.device_id = Uuid::new_v4().to_string();

        service.save(&state).expect("replacement should succeed");

        assert_ne!(fs::metadata(&path).unwrap().ino(), prior_inode);
        assert_eq!(
            LocalSyncStateService::new(temporary.path())
                .load_or_initialize(None)
                .unwrap()
                .device_id,
            state.device_id
        );
        assert_eq!(fs::read_dir(temporary.path()).unwrap().count(), 1);
    }

    #[test]
    fn interrupted_save_preserves_the_prior_valid_state() {
        let temporary = tempdir().expect("temporary app data");
        let service = LocalSyncStateService::new(temporary.path());
        let mut state = service.load_or_initialize(None).unwrap();
        let path = temporary.path().join(LOCAL_SYNC_STATE_FILE);
        let before = fs::read(&path).unwrap();
        state.device_id = Uuid::new_v4().to_string();

        let result = service.save_with_writer(&state, |destination, bytes, _mode| {
            fs::write(destination.with_extension("interrupted"), &bytes[..8]).unwrap();
            Err(LocalSyncStateError::write_failed())
        });

        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(
            LocalSyncStateService::new(temporary.path())
                .load_or_initialize(None)
                .unwrap()
                .repo_key,
            state.repo_key
        );
    }

    #[test]
    fn debug_output_always_redacts_the_repository_key() {
        let temporary = tempdir().expect("temporary app data");
        let state = LocalSyncStateService::new(temporary.path())
            .load_or_initialize(None)
            .unwrap();

        let debug = format!("{state:?}");
        assert!(!debug.contains(&state.repo_key));
        assert!(debug.contains("[REDACTED]"));
    }
}
