use std::{
    fmt,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock, Weak,
    },
};

use argon2::{
    password_hash::{PasswordHash, PasswordHasher as _, PasswordVerifier as _, SaltString},
    Algorithm, Argon2, Params, Version,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::{Zeroize as _, Zeroizing};

use crate::{
    paths::{ConfigRoot, ConfigRootIdentity},
    storage::{
        CommitState, DurableFileFailureKind, DurableFileStore, ExpectedFile, FileRevision,
        PreservePrevious, RecoveryOutcome, ReplaceRequest, StorageFileName,
    },
};

use super::secret::ServerAuthenticationSecret;

const AUTHENTICATION_FILE: &str = "owner-auth-v1.json";
const AUTHENTICATION_SCHEMA_VERSION: u32 = 1;
const MAXIMUM_AUTHENTICATION_FILE_BYTES: u64 = 16 * 1024;
const MINIMUM_OWNER_PASSWORD_BYTES: usize = 12;
const MAXIMUM_OWNER_PASSWORD_BYTES: usize = 1024;
const ARGON2_MEMORY_KIB: u32 = 19_456;
const ARGON2_ITERATIONS: u32 = 2;
const ARGON2_PARALLELISM: u32 = 1;
const ARGON2_OUTPUT_BYTES: usize = 32;
const ARGON2_VERSION_NUMBER: u32 = 19;

type ClaimedSecurityOwnerRoots = Vec<(ConfigRootIdentity, PathBuf, Weak<()>)>;

static CLAIMED_SECURITY_OWNER_ROOTS: OnceLock<Mutex<ClaimedSecurityOwnerRoots>> = OnceLock::new();

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServerAuthenticationStatus {
    NeedsInitialization,
    Ready,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OwnerPasswordVerification {
    Authorized { needs_rehash: bool },
    Rejected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OwnerPasswordRehash {
    Unchanged,
    Updated,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OwnerPasswordUpdateError {
    InvalidCurrentPassword,
    InvalidNewPassword,
    StateUnavailable,
    StateUncertain,
}

pub(super) struct PreparedOwnerPasswordChange {
    expected_revision: FileRevision,
    serialized: Zeroizing<Vec<u8>>,
}

impl Drop for PreparedOwnerPasswordChange {
    fn drop(&mut self) {
        self.serialized.zeroize();
    }
}

impl fmt::Display for OwnerPasswordUpdateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidCurrentPassword => "current owner password is not authorized",
            Self::InvalidNewPassword => "new owner password is invalid",
            Self::StateUnavailable => "server authentication state is unavailable",
            Self::StateUncertain => "server authentication publication is uncertain",
        })
    }
}

impl std::error::Error for OwnerPasswordUpdateError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OwnerPasswordInitializationError {
    InvalidPassword,
    AlreadyInitialized,
    StateUnavailable,
    StateUncertain,
}

impl fmt::Display for OwnerPasswordInitializationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidPassword => "owner password is invalid",
            Self::AlreadyInitialized => "server owner is already initialized",
            Self::StateUnavailable => "server authentication state is unavailable",
            Self::StateUncertain => "server authentication publication is uncertain",
        })
    }
}

impl std::error::Error for OwnerPasswordInitializationError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServerAuthenticationError;

impl fmt::Display for ServerAuthenticationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("server authentication state is unavailable")
    }
}

impl std::error::Error for ServerAuthenticationError {}

pub struct ServerAuthenticationStore {
    config_root: ConfigRoot,
    store: DurableFileStore,
    target: StorageFileName,
    state_uncertain: AtomicBool,
    security_owner_claimed: AtomicBool,
    #[cfg(test)]
    password_update_hook: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    #[cfg(test)]
    password_commit_hook: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    #[cfg(test)]
    password_prepare_entry_hook: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    #[cfg(test)]
    password_verification_hook: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

impl ServerAuthenticationStore {
    /// Opens authentication state below the retained, non-synchronized config root.
    /// The caller must retain the matching Kernel instance lock for this store's lifetime.
    /// Construct exactly one store per server process launch. A publication uncertainty
    /// latches that store closed; reconstruction is reserved for process-level recovery.
    pub fn open(config_root: &ConfigRoot) -> Result<Self, ServerAuthenticationError> {
        config_root
            .verify_held_directory()
            .map_err(|_| ServerAuthenticationError)?;
        let config_root = config_root
            .try_clone_root()
            .map_err(|_| ServerAuthenticationError)?;
        let store = DurableFileStore::at_retained_directory(
            config_root
                .try_clone_dir()
                .map_err(|_| ServerAuthenticationError)?,
            config_root.canonical_path().to_path_buf(),
            Uuid::new_v4(),
        );
        Self::finish_open(config_root, store)
    }

    pub(super) fn matches_config_root(&self, candidate: &ConfigRoot) -> bool {
        self.config_root.identity() == candidate.identity()
            && self.config_root.verify_held_directory().is_ok()
            && candidate.verify_held_directory().is_ok()
    }

    #[cfg(test)]
    pub(super) fn open_with_test_fault(
        config_root: &ConfigRoot,
        fault: crate::storage::DurableFileTestFault,
    ) -> Result<Self, ServerAuthenticationError> {
        config_root
            .verify_held_directory()
            .map_err(|_| ServerAuthenticationError)?;
        let config_root = config_root
            .try_clone_root()
            .map_err(|_| ServerAuthenticationError)?;
        let store = DurableFileStore::at_retained_directory_with_test_fault(
            config_root
                .try_clone_dir()
                .map_err(|_| ServerAuthenticationError)?,
            config_root.canonical_path().to_path_buf(),
            Uuid::new_v4(),
            fault,
        );
        Self::finish_open(config_root, store)
    }

    fn finish_open(
        config_root: ConfigRoot,
        store: DurableFileStore,
    ) -> Result<Self, ServerAuthenticationError> {
        let recovery = store.recover().map_err(|_| ServerAuthenticationError)?;
        if recovery
            .iter()
            .any(|outcome| matches!(outcome, RecoveryOutcome::ManualInterventionRequired { .. }))
        {
            return Err(ServerAuthenticationError);
        }
        let this = Self {
            config_root,
            store,
            target: StorageFileName::parse(AUTHENTICATION_FILE)
                .map_err(|_| ServerAuthenticationError)?,
            state_uncertain: AtomicBool::new(false),
            security_owner_claimed: AtomicBool::new(false),
            #[cfg(test)]
            password_update_hook: Mutex::new(None),
            #[cfg(test)]
            password_commit_hook: Mutex::new(None),
            #[cfg(test)]
            password_prepare_entry_hook: Mutex::new(None),
            #[cfg(test)]
            password_verification_hook: Mutex::new(None),
        };
        let _state = this.read_snapshot()?;
        this.config_root
            .verify_held_directory()
            .map_err(|_| ServerAuthenticationError)?;
        Ok(this)
    }

    pub(super) fn claim_security_owner(&self) -> Result<Arc<()>, ServerAuthenticationError> {
        self.security_owner_claimed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_already_claimed| ServerAuthenticationError)?;
        let claim = self.claim_unowned_security_root();
        if claim.is_err() {
            self.security_owner_claimed.store(false, Ordering::Release);
        }
        claim
    }

    fn claim_unowned_security_root(&self) -> Result<Arc<()>, ServerAuthenticationError> {
        let mut claimed_roots = CLAIMED_SECURITY_OWNER_ROOTS
            .get_or_init(|| Mutex::new(Vec::new()))
            .lock()
            .map_err(|_poisoned| ServerAuthenticationError)?;
        let root_path = self.config_root.canonical_path();
        let root_identity = self.config_root.identity();
        claimed_roots.retain(|(_identity, _path, owner)| owner.strong_count() != 0);
        if claimed_roots
            .iter()
            .any(|(identity, path, _owner)| *identity == root_identity || path == root_path)
        {
            return Err(ServerAuthenticationError);
        }
        let owner = Arc::new(());
        claimed_roots.push((
            root_identity,
            root_path.to_path_buf(),
            Arc::downgrade(&owner),
        ));
        Ok(owner)
    }

    pub(super) fn is_state_uncertain(&self) -> bool {
        self.state_uncertain.load(Ordering::Acquire)
    }

    #[cfg(test)]
    pub(super) fn set_password_update_test_hook(&self, hook: Arc<dyn Fn() + Send + Sync>) {
        *self.password_update_hook.lock().unwrap() = Some(hook);
    }

    #[cfg(test)]
    pub(super) fn set_password_commit_test_hook(&self, hook: Arc<dyn Fn() + Send + Sync>) {
        *self.password_commit_hook.lock().unwrap() = Some(hook);
    }

    #[cfg(test)]
    pub(super) fn set_password_prepare_entry_test_hook(&self, hook: Arc<dyn Fn() + Send + Sync>) {
        *self.password_prepare_entry_hook.lock().unwrap() = Some(hook);
    }

    #[cfg(test)]
    pub(super) fn set_password_verification_test_hook(&self, hook: Arc<dyn Fn() + Send + Sync>) {
        *self.password_verification_hook.lock().unwrap() = Some(hook);
    }

    pub fn status(&self) -> Result<ServerAuthenticationStatus, ServerAuthenticationError> {
        Ok(match self.read_snapshot()? {
            None => ServerAuthenticationStatus::NeedsInitialization,
            Some(_) => ServerAuthenticationStatus::Ready,
        })
    }

    /// Inspects the retained owner state after an initialization publication
    /// became uncertain without clearing the process-local authentication
    /// latch. This is intentionally narrower than [`Self::status`]: it only
    /// lets the initialization coordinator keep persistent state authoritative
    /// when deciding whether its one-time gate is spent.
    pub(super) fn reconcile_uncertain_initialization(
        &self,
    ) -> Result<ServerAuthenticationStatus, ServerAuthenticationError> {
        if !self.state_uncertain.load(Ordering::Acquire) {
            return Err(ServerAuthenticationError);
        }
        Ok(match self.read_snapshot_unchecked()? {
            None => ServerAuthenticationStatus::NeedsInitialization,
            Some(_) => ServerAuthenticationStatus::Ready,
        })
    }

    pub fn initialize_owner_password(
        &self,
        password: impl Into<ServerAuthenticationSecret>,
    ) -> Result<(), OwnerPasswordInitializationError> {
        let mut password = password.into();
        if self.state_uncertain.load(Ordering::Acquire) {
            return Err(OwnerPasswordInitializationError::StateUncertain);
        }
        if !valid_owner_password(password.as_bytes()) {
            return Err(OwnerPasswordInitializationError::InvalidPassword);
        }
        self.config_root
            .verify_held_directory()
            .map_err(|_| OwnerPasswordInitializationError::StateUnavailable)?;
        match self.read_snapshot() {
            Ok(None) => {}
            Ok(Some(_)) => return Err(OwnerPasswordInitializationError::AlreadyInitialized),
            Err(_) => return Err(OwnerPasswordInitializationError::StateUnavailable),
        }

        let state = PersistentAuthenticationState {
            schema_version: AUTHENTICATION_SCHEMA_VERSION,
            password_hash: hash_owner_password(password.as_bytes())
                .map_err(|_| OwnerPasswordInitializationError::StateUnavailable)?,
        };
        password.zeroize();
        let mut serialized = Zeroizing::new(
            serde_json::to_vec(&state)
                .map_err(|_| OwnerPasswordInitializationError::StateUnavailable)?,
        );
        let outcome = match self.store.replace_with_address_validation(
            ReplaceRequest {
                target: &self.target,
                bytes: serialized.as_slice(),
                expected: ExpectedFile::Absent,
                preserve_previous: PreservePrevious::None,
            },
            || self.config_root.verify_held_directory().is_ok(),
        ) {
            Ok(outcome) => outcome,
            Err(error) => {
                return Err(match error.kind() {
                    DurableFileFailureKind::RevisionConflict => {
                        OwnerPasswordInitializationError::AlreadyInitialized
                    }
                    DurableFileFailureKind::PublishStateUncertain
                    | DurableFileFailureKind::RecoveryRequired => {
                        self.latch_uncertain_and_reload();
                        OwnerPasswordInitializationError::StateUncertain
                    }
                    _ => OwnerPasswordInitializationError::StateUnavailable,
                })
            }
        };
        serialized.zeroize();
        match outcome.commit_state {
            CommitState::Durable | CommitState::AtomicVisibility => Ok(()),
            CommitState::PublishedDurabilityUncertain => {
                self.latch_uncertain_and_reload();
                Err(OwnerPasswordInitializationError::StateUncertain)
            }
        }
    }

    pub fn verify_owner_password(
        &self,
        candidate: &str,
    ) -> Result<OwnerPasswordVerification, ServerAuthenticationError> {
        let snapshot = self.read_snapshot()?.ok_or(ServerAuthenticationError)?;
        let verification = verify_password_against_state(candidate.as_bytes(), &snapshot.state);
        #[cfg(test)]
        if let Some(hook) = self.password_verification_hook.lock().unwrap().clone() {
            hook();
        }
        verification
    }

    /// Replaces an accepted legacy PHC with the current Argon2 policy.
    ///
    /// The password is verified again against the revision used by the CAS so
    /// callers cannot publish a rehash based on a stale authentication read.
    pub fn rehash_owner_password(
        &self,
        password: impl Into<ServerAuthenticationSecret>,
    ) -> Result<OwnerPasswordRehash, OwnerPasswordUpdateError> {
        let password = password.into();
        self.ensure_update_available()?;
        let snapshot = self
            .read_snapshot()
            .map_err(|_| OwnerPasswordUpdateError::StateUnavailable)?
            .ok_or(OwnerPasswordUpdateError::StateUnavailable)?;
        match verify_password_against_state(password.as_bytes(), &snapshot.state)
            .map_err(|_| OwnerPasswordUpdateError::StateUnavailable)?
        {
            OwnerPasswordVerification::Rejected => {
                return Err(OwnerPasswordUpdateError::InvalidCurrentPassword);
            }
            OwnerPasswordVerification::Authorized {
                needs_rehash: false,
            } => return Ok(OwnerPasswordRehash::Unchanged),
            OwnerPasswordVerification::Authorized { needs_rehash: true } => {}
        }
        self.replace_password_hash(&snapshot.revision, password.as_bytes())?;
        Ok(OwnerPasswordRehash::Updated)
    }

    /// Atomically replaces the fixed owner's password after verifying the
    /// current password against the same persistent revision. Any uncertain
    /// publication latches every read and mutation closed until reconstruction.
    pub fn change_owner_password(
        &self,
        current_password: impl Into<ServerAuthenticationSecret>,
        new_password: impl Into<ServerAuthenticationSecret>,
    ) -> Result<(), OwnerPasswordUpdateError> {
        let prepared = self.prepare_owner_password_change(current_password, new_password)?;
        self.commit_prepared_owner_password_change(prepared)
    }

    pub(super) fn prepare_owner_password_change(
        &self,
        current_password: impl Into<ServerAuthenticationSecret>,
        new_password: impl Into<ServerAuthenticationSecret>,
    ) -> Result<PreparedOwnerPasswordChange, OwnerPasswordUpdateError> {
        #[cfg(test)]
        if let Some(hook) = self.password_prepare_entry_hook.lock().unwrap().clone() {
            hook();
        }
        let current_password = current_password.into();
        let new_password = new_password.into();
        self.ensure_update_available()?;
        let snapshot = self
            .read_snapshot()
            .map_err(|_| OwnerPasswordUpdateError::StateUnavailable)?
            .ok_or(OwnerPasswordUpdateError::StateUnavailable)?;
        match verify_password_against_state(current_password.as_bytes(), &snapshot.state)
            .map_err(|_| OwnerPasswordUpdateError::StateUnavailable)?
        {
            OwnerPasswordVerification::Rejected => {
                return Err(OwnerPasswordUpdateError::InvalidCurrentPassword);
            }
            OwnerPasswordVerification::Authorized { .. } => {}
        }
        if !valid_owner_password(new_password.as_bytes()) {
            return Err(OwnerPasswordUpdateError::InvalidNewPassword);
        }
        let prepared = self.prepare_password_hash(snapshot.revision, new_password.as_bytes())?;
        #[cfg(test)]
        if let Some(hook) = self.password_update_hook.lock().unwrap().clone() {
            hook();
        }
        Ok(prepared)
    }

    pub(super) fn commit_prepared_owner_password_change(
        &self,
        prepared: PreparedOwnerPasswordChange,
    ) -> Result<(), OwnerPasswordUpdateError> {
        self.ensure_update_available()?;
        self.publish_prepared_password_hash(&prepared)
    }

    fn replace_password_hash(
        &self,
        expected_revision: &FileRevision,
        password: &[u8],
    ) -> Result<(), OwnerPasswordUpdateError> {
        let prepared = self.prepare_password_hash(expected_revision.clone(), password)?;
        self.publish_prepared_password_hash(&prepared)
    }

    fn prepare_password_hash(
        &self,
        expected_revision: FileRevision,
        password: &[u8],
    ) -> Result<PreparedOwnerPasswordChange, OwnerPasswordUpdateError> {
        let state = PersistentAuthenticationState {
            schema_version: AUTHENTICATION_SCHEMA_VERSION,
            password_hash: hash_owner_password(password)
                .map_err(|_| OwnerPasswordUpdateError::StateUnavailable)?,
        };
        let serialized = Zeroizing::new(
            serde_json::to_vec(&state).map_err(|_| OwnerPasswordUpdateError::StateUnavailable)?,
        );
        Ok(PreparedOwnerPasswordChange {
            expected_revision,
            serialized,
        })
    }

    fn publish_prepared_password_hash(
        &self,
        prepared: &PreparedOwnerPasswordChange,
    ) -> Result<(), OwnerPasswordUpdateError> {
        self.config_root
            .verify_held_directory()
            .map_err(|_| OwnerPasswordUpdateError::StateUnavailable)?;
        #[cfg(test)]
        if let Some(hook) = self.password_commit_hook.lock().unwrap().clone() {
            hook();
        }
        let outcome = match self.store.replace_with_address_validation(
            ReplaceRequest {
                target: &self.target,
                bytes: prepared.serialized.as_slice(),
                expected: ExpectedFile::Revision(&prepared.expected_revision),
                preserve_previous: PreservePrevious::None,
            },
            || self.config_root.verify_held_directory().is_ok(),
        ) {
            Ok(outcome) => outcome,
            Err(error) => return Err(self.reconcile_update_failure(error.kind())),
        };
        match outcome.commit_state {
            CommitState::Durable | CommitState::AtomicVisibility => {
                if self.config_root.verify_held_directory().is_err() {
                    self.latch_uncertain_and_reload();
                    return Err(OwnerPasswordUpdateError::StateUncertain);
                }
                Ok(())
            }
            CommitState::PublishedDurabilityUncertain => {
                Err(self.reconcile_update_failure(DurableFileFailureKind::PublishStateUncertain))
            }
        }
    }

    fn reconcile_update_failure(
        &self,
        failure: DurableFileFailureKind,
    ) -> OwnerPasswordUpdateError {
        let uncertain = matches!(
            failure,
            DurableFileFailureKind::RevisionConflict
                | DurableFileFailureKind::PublishStateUncertain
                | DurableFileFailureKind::RecoveryRequired
        );
        if !uncertain {
            return OwnerPasswordUpdateError::StateUnavailable;
        }
        self.latch_uncertain_and_reload();
        OwnerPasswordUpdateError::StateUncertain
    }

    fn read_snapshot(
        &self,
    ) -> Result<Option<PersistentAuthenticationSnapshot>, ServerAuthenticationError> {
        if self.state_uncertain.load(Ordering::Acquire) {
            return Err(ServerAuthenticationError);
        }
        let snapshot = self.read_snapshot_unchecked()?;
        if self.state_uncertain.load(Ordering::Acquire) {
            return Err(ServerAuthenticationError);
        }
        Ok(snapshot)
    }

    fn read_snapshot_unchecked(
        &self,
    ) -> Result<Option<PersistentAuthenticationSnapshot>, ServerAuthenticationError> {
        self.config_root
            .verify_held_directory()
            .map_err(|_| ServerAuthenticationError)?;
        let Some(stored) = self
            .store
            .read(&self.target, MAXIMUM_AUTHENTICATION_FILE_BYTES)
            .map_err(|_| ServerAuthenticationError)?
        else {
            self.config_root
                .verify_held_directory()
                .map_err(|_| ServerAuthenticationError)?;
            return Ok(None);
        };
        let state: PersistentAuthenticationState =
            serde_json::from_slice(&stored.bytes).map_err(|_| ServerAuthenticationError)?;
        self.config_root
            .verify_held_directory()
            .map_err(|_| ServerAuthenticationError)?;
        if state.schema_version != AUTHENTICATION_SCHEMA_VERSION {
            return Err(ServerAuthenticationError);
        }
        let _parsed = parse_password_hash(&state.password_hash)?;
        Ok(Some(PersistentAuthenticationSnapshot {
            state,
            revision: stored.revision.clone(),
        }))
    }

    fn ensure_update_available(&self) -> Result<(), OwnerPasswordUpdateError> {
        if self.state_uncertain.load(Ordering::Acquire) {
            Err(OwnerPasswordUpdateError::StateUncertain)
        } else {
            Ok(())
        }
    }

    fn latch_uncertain_and_reload(&self) {
        self.state_uncertain.store(true, Ordering::Release);
        let _reconciled_state = self.read_snapshot_unchecked();
    }
}

impl fmt::Debug for ServerAuthenticationStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ServerAuthenticationStore(..)")
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PersistentAuthenticationState {
    schema_version: u32,
    password_hash: String,
}

impl Drop for PersistentAuthenticationState {
    fn drop(&mut self) {
        self.password_hash.zeroize();
    }
}

struct PersistentAuthenticationSnapshot {
    state: PersistentAuthenticationState,
    revision: FileRevision,
}

fn valid_owner_password(password: &[u8]) -> bool {
    (MINIMUM_OWNER_PASSWORD_BYTES..=MAXIMUM_OWNER_PASSWORD_BYTES).contains(&password.len())
        && password.iter().any(|byte| !byte.is_ascii_whitespace())
}

fn production_argon2() -> Argon2<'static> {
    let params = Params::new(
        ARGON2_MEMORY_KIB,
        ARGON2_ITERATIONS,
        ARGON2_PARALLELISM,
        Some(ARGON2_OUTPUT_BYTES),
    )
    .expect("the fixed Argon2id policy must be valid");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

fn hash_owner_password(password: &[u8]) -> Result<String, ServerAuthenticationError> {
    let mut salt = [0_u8; 16];
    getrandom::fill(&mut salt).map_err(|_| ServerAuthenticationError)?;
    let encoded_salt = SaltString::encode_b64(&salt).map_err(|_| ServerAuthenticationError)?;
    salt.zeroize();
    production_argon2()
        .hash_password(password, &encoded_salt)
        .map(|hash| hash.to_string())
        .map_err(|_| ServerAuthenticationError)
}

fn parse_password_hash(value: &str) -> Result<PasswordHash<'_>, ServerAuthenticationError> {
    PasswordHash::new(value).map_err(|_| ServerAuthenticationError)
}

fn verify_password_against_state(
    candidate: &[u8],
    state: &PersistentAuthenticationState,
) -> Result<OwnerPasswordVerification, ServerAuthenticationError> {
    let parsed = parse_password_hash(&state.password_hash)?;
    if production_argon2()
        .verify_password(candidate, &parsed)
        .is_err()
    {
        return Ok(OwnerPasswordVerification::Rejected);
    }
    Ok(OwnerPasswordVerification::Authorized {
        needs_rehash: password_hash_needs_rehash(&parsed),
    })
}

fn password_hash_needs_rehash(hash: &PasswordHash<'_>) -> bool {
    hash.algorithm.as_str() != "argon2id"
        || hash.version != Some(ARGON2_VERSION_NUMBER)
        || hash.params.get_decimal("m") != Some(ARGON2_MEMORY_KIB)
        || hash.params.get_decimal("t") != Some(ARGON2_ITERATIONS)
        || hash.params.get_decimal("p") != Some(ARGON2_PARALLELISM)
        || hash.hash.as_ref().map(|output| output.len()) != Some(ARGON2_OUTPUT_BYTES)
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use static_assertions::assert_not_impl_any;
    use tempfile::tempdir;

    use super::*;
    use crate::{paths::KernelPaths, storage::DurableFileTestFault};

    const OWNER_PASSWORD: &str = "correct horse battery staple";
    const NEW_PASSWORD: &str = "new owner password material";

    assert_not_impl_any!(PreparedOwnerPasswordChange: Clone, Copy, fmt::Debug, Serialize);

    fn fixture_paths(root: &Path) -> KernelPaths {
        let workspace = root.join("workspace");
        let config = root.join("config");
        let cache = root.join("cache");
        fs::create_dir_all(&workspace).unwrap();
        fs::create_dir_all(&config).unwrap();
        fs::create_dir_all(&cache).unwrap();
        KernelPaths::desktop(&workspace, &config, &cache).unwrap()
    }

    #[test]
    fn stale_revision_is_reloaded_but_the_update_still_fails_closed() {
        let temporary = tempdir().unwrap();
        let paths = fixture_paths(temporary.path());
        let store = ServerAuthenticationStore::open(paths.config_root()).unwrap();
        store
            .initialize_owner_password(OWNER_PASSWORD.to_owned())
            .unwrap();
        let stale = store.read_snapshot().unwrap().unwrap();
        store
            .change_owner_password(OWNER_PASSWORD.to_owned(), NEW_PASSWORD.to_owned())
            .unwrap();

        assert_eq!(
            store
                .replace_password_hash(&stale.revision, b"third owner password material",)
                .unwrap_err(),
            OwnerPasswordUpdateError::StateUncertain
        );
        assert_eq!(
            store.verify_owner_password(NEW_PASSWORD),
            Err(ServerAuthenticationError)
        );
        let reopened = ServerAuthenticationStore::open(paths.config_root()).unwrap();
        assert_eq!(
            reopened.verify_owner_password(NEW_PASSWORD).unwrap(),
            OwnerPasswordVerification::Authorized {
                needs_rehash: false
            }
        );
        assert_eq!(
            reopened
                .verify_owner_password("third owner password material")
                .unwrap(),
            OwnerPasswordVerification::Rejected
        );
    }

    #[test]
    fn durability_uncertainty_reloads_visible_state_and_never_reports_success() {
        let temporary = tempdir().unwrap();
        let paths = fixture_paths(temporary.path());
        ServerAuthenticationStore::open(paths.config_root())
            .unwrap()
            .initialize_owner_password(OWNER_PASSWORD.to_owned())
            .unwrap();
        let store = ServerAuthenticationStore::open_with_test_fault(
            paths.config_root(),
            DurableFileTestFault::ParentSyncFailure,
        )
        .unwrap();

        assert_eq!(
            store
                .change_owner_password(OWNER_PASSWORD.to_owned(), NEW_PASSWORD.to_owned())
                .unwrap_err(),
            OwnerPasswordUpdateError::StateUncertain
        );
        assert_eq!(
            store.verify_owner_password(NEW_PASSWORD),
            Err(ServerAuthenticationError)
        );
        assert_eq!(
            store
                .change_owner_password(
                    NEW_PASSWORD.to_owned(),
                    "third owner password material".to_owned(),
                )
                .unwrap_err(),
            OwnerPasswordUpdateError::StateUncertain
        );
        drop(store);
        let reopened = ServerAuthenticationStore::open(paths.config_root()).unwrap();
        assert_eq!(
            reopened.verify_owner_password(NEW_PASSWORD).unwrap(),
            OwnerPasswordVerification::Authorized {
                needs_rehash: false
            }
        );
    }

    #[test]
    fn initialization_uncertainty_latches_authentication_until_recovery() {
        let temporary = tempdir().unwrap();
        let paths = fixture_paths(temporary.path());
        let store = ServerAuthenticationStore::open_with_test_fault(
            paths.config_root(),
            DurableFileTestFault::ParentSyncFailure,
        )
        .unwrap();

        assert_eq!(
            store
                .initialize_owner_password(OWNER_PASSWORD.to_owned())
                .unwrap_err(),
            OwnerPasswordInitializationError::StateUncertain
        );
        assert_eq!(store.status(), Err(ServerAuthenticationError));
        drop(store);

        let reopened = ServerAuthenticationStore::open(paths.config_root()).unwrap();
        assert_eq!(
            reopened.status().unwrap(),
            ServerAuthenticationStatus::Ready
        );
        assert_eq!(
            reopened.verify_owner_password(OWNER_PASSWORD).unwrap(),
            OwnerPasswordVerification::Authorized {
                needs_rehash: false
            }
        );
    }
}
