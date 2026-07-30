use std::fmt;

use argon2::{
    password_hash::{PasswordHash, PasswordHasher as _, PasswordVerifier as _, SaltString},
    Algorithm, Argon2, Params, Version,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::{Zeroize as _, Zeroizing};

use crate::{
    paths::ConfigRoot,
    storage::{
        CommitState, DurableFileFailureKind, DurableFileStore, ExpectedFile, PreservePrevious,
        RecoveryOutcome, ReplaceRequest, StorageFileName,
    },
};

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
}

impl ServerAuthenticationStore {
    /// Opens authentication state below the retained, non-synchronized config root.
    /// The caller must retain the matching Kernel instance lock for this store's lifetime.
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
        };
        let _state = this.read_state()?;
        this.config_root
            .verify_held_directory()
            .map_err(|_| ServerAuthenticationError)?;
        Ok(this)
    }

    pub fn status(&self) -> Result<ServerAuthenticationStatus, ServerAuthenticationError> {
        Ok(match self.read_state()? {
            None => ServerAuthenticationStatus::NeedsInitialization,
            Some(_) => ServerAuthenticationStatus::Ready,
        })
    }

    pub fn initialize_owner_password(
        &self,
        password: String,
    ) -> Result<(), OwnerPasswordInitializationError> {
        let mut password = Zeroizing::new(password);
        if !valid_owner_password(password.as_bytes()) {
            return Err(OwnerPasswordInitializationError::InvalidPassword);
        }
        self.config_root
            .verify_held_directory()
            .map_err(|_| OwnerPasswordInitializationError::StateUnavailable)?;
        match self.read_state() {
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
        let outcome = self
            .store
            .replace_with_address_validation(
                ReplaceRequest {
                    target: &self.target,
                    bytes: serialized.as_slice(),
                    expected: ExpectedFile::Absent,
                    preserve_previous: PreservePrevious::None,
                },
                || self.config_root.verify_held_directory().is_ok(),
            )
            .map_err(|error| match error.kind() {
                DurableFileFailureKind::RevisionConflict => {
                    OwnerPasswordInitializationError::AlreadyInitialized
                }
                DurableFileFailureKind::PublishStateUncertain
                | DurableFileFailureKind::RecoveryRequired => {
                    OwnerPasswordInitializationError::StateUncertain
                }
                _ => OwnerPasswordInitializationError::StateUnavailable,
            })?;
        serialized.zeroize();
        match outcome.commit_state {
            CommitState::Durable | CommitState::AtomicVisibility => Ok(()),
            CommitState::PublishedDurabilityUncertain => {
                Err(OwnerPasswordInitializationError::StateUncertain)
            }
        }
    }

    pub fn verify_owner_password(
        &self,
        candidate: &str,
    ) -> Result<OwnerPasswordVerification, ServerAuthenticationError> {
        let state = self.read_state()?.ok_or(ServerAuthenticationError)?;
        let parsed = parse_password_hash(&state.password_hash)?;
        if production_argon2()
            .verify_password(candidate.as_bytes(), &parsed)
            .is_err()
        {
            return Ok(OwnerPasswordVerification::Rejected);
        }
        Ok(OwnerPasswordVerification::Authorized {
            needs_rehash: password_hash_needs_rehash(&parsed),
        })
    }

    fn read_state(
        &self,
    ) -> Result<Option<PersistentAuthenticationState>, ServerAuthenticationError> {
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
        Ok(Some(state))
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

fn password_hash_needs_rehash(hash: &PasswordHash<'_>) -> bool {
    hash.algorithm.as_str() != "argon2id"
        || hash.version != Some(ARGON2_VERSION_NUMBER)
        || hash.params.get_decimal("m") != Some(ARGON2_MEMORY_KIB)
        || hash.params.get_decimal("t") != Some(ARGON2_ITERATIONS)
        || hash.params.get_decimal("p") != Some(ARGON2_PARALLELISM)
        || hash.hash.as_ref().map(|output| output.len()) != Some(ARGON2_OUTPUT_BYTES)
}
