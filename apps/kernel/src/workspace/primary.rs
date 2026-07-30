//! Primary-workspace persistence boundary.

use std::{
    fmt,
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::{Uuid, Variant, Version};

use crate::{
    config::KernelLaunchEpoch,
    paths::InstanceDataRoot,
    storage::{
        CommitState, DurableFileFailureKind, DurableFileStore, ExpectedFile, FileRevision,
        PreservePrevious, RecoveryOutcome, ReplaceRequest, StorageFileName,
    },
};

const PRIMARY_WORKSPACE_SCHEMA_VERSION: u64 = 1;

/// Host-committed, path-free identity for the one active workspace.
///
/// Native hosts persist this value inside their existing primary-workspace
/// record and pass it to the Kernel during launch. It deliberately contains no
/// absolute filesystem address, launch credential, or per-process identity.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PrimaryWorkspaceState {
    schema_version: u64,
    revision_seed: String,
    display_name: String,
}

impl PrimaryWorkspaceState {
    pub(crate) fn new(display_name: impl Into<String>) -> Result<Self, PrimaryWorkspaceStateError> {
        let state = Self {
            schema_version: PRIMARY_WORKSPACE_SCHEMA_VERSION,
            revision_seed: Uuid::new_v4().to_string(),
            display_name: display_name.into(),
        };
        state.validate()?;
        Ok(state)
    }

    pub fn from_value(value: Value) -> Result<Self, PrimaryWorkspaceStateError> {
        let state: Self = serde_json::from_value(value).map_err(|_| PrimaryWorkspaceStateError)?;
        state.validate()?;
        Ok(state)
    }

    pub fn to_value(&self) -> Result<Value, PrimaryWorkspaceStateError> {
        serde_json::to_value(self).map_err(|_| PrimaryWorkspaceStateError)
    }

    pub fn validate(&self) -> Result<(), PrimaryWorkspaceStateError> {
        if self.schema_version != PRIMARY_WORKSPACE_SCHEMA_VERSION || self.revision_seed.is_empty()
        {
            return Err(PrimaryWorkspaceStateError);
        }
        validate_display_name(&self.display_name)
    }

    pub fn display_name(&self) -> &str {
        self.display_name.as_str()
    }

    pub(crate) fn validate_display_name(value: &str) -> Result<(), PrimaryWorkspaceStateError> {
        validate_display_name(value)
    }

    pub(crate) fn validate_native_host_identity(&self) -> Result<(), PrimaryWorkspaceStateError> {
        self.validate()?;
        let revision_seed =
            Uuid::parse_str(&self.revision_seed).map_err(|_| PrimaryWorkspaceStateError)?;
        if revision_seed.get_variant() != Variant::RFC4122
            || revision_seed.get_version() != Some(Version::Random)
            || revision_seed.hyphenated().to_string() != self.revision_seed
        {
            return Err(PrimaryWorkspaceStateError);
        }
        Ok(())
    }

    pub(crate) fn has_same_revision_identity(&self, candidate: &Self) -> bool {
        self.revision_seed == candidate.revision_seed
    }
}

impl fmt::Debug for PrimaryWorkspaceState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PrimaryWorkspaceState")
            .field("schema_version", &self.schema_version)
            .field("revision_seed", &"[REDACTED]")
            .field("display_name", &self.display_name)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrimaryWorkspaceStateError;

impl fmt::Display for PrimaryWorkspaceStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("the primary workspace state is invalid")
    }
}

impl std::error::Error for PrimaryWorkspaceStateError {}

fn validate_display_name(value: &str) -> Result<(), PrimaryWorkspaceStateError> {
    if value.is_empty()
        || value.chars().count() > 255
        || value.chars().any(char::is_control)
        || value.contains(['/', '\\'])
    {
        return Err(PrimaryWorkspaceStateError);
    }
    Ok(())
}

/// Read-only primary-workspace store for a fixed native launch.
///
/// The host has already committed the state before spawning the child. The
/// child may observe that exact value, but cannot create a parallel authority
/// record or switch the host-selected workspace in place.
pub struct FixedPrimaryWorkspaceStore {
    binding: PrimaryWorkspaceRepositoryBinding,
    value: Value,
}

impl FixedPrimaryWorkspaceStore {
    pub fn new(state: PrimaryWorkspaceState) -> Result<Self, PrimaryWorkspaceStateError> {
        state.validate()?;
        Ok(Self {
            binding: PrimaryWorkspaceRepositoryBinding::new(),
            value: state.to_value()?,
        })
    }
}

impl PrimaryWorkspaceStore for FixedPrimaryWorkspaceStore {
    fn repository_binding(&self) -> PrimaryWorkspaceRepositoryBinding {
        self.binding.clone()
    }

    fn load(&self) -> Result<Option<Value>, PrimaryWorkspaceStoreError> {
        Ok(Some(self.value.clone()))
    }

    fn replace(&self, value: Option<Value>) -> Result<(), PrimaryWorkspaceStoreError> {
        if value.as_ref() == Some(&self.value) {
            Ok(())
        } else {
            Err(PrimaryWorkspaceStoreError::unavailable())
        }
    }

    fn save(&self) -> Result<(), PrimaryWorkspaceStoreError> {
        Ok(())
    }
}

impl fmt::Debug for FixedPrimaryWorkspaceStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("FixedPrimaryWorkspaceStore(..)")
    }
}

const SERVER_PRIMARY_WORKSPACE_FILE: &str = "primary-workspace-v1.json";
const MAXIMUM_SERVER_PRIMARY_WORKSPACE_BYTES: u64 = 16 * 1024;

/// Kernel-owned primary-workspace state for the fixed `/data/workspace` Server host.
///
/// The durable-file boundary retains the state directory, rejects symlinks,
/// recovers interrupted publications, and atomically replaces the one canonical
/// record. A server workspace cannot be removed or switched in place.
pub struct DurableServerPrimaryWorkspaceStore {
    binding: PrimaryWorkspaceRepositoryBinding,
    durable: DurableFileStore,
    target: StorageFileName,
    state: Mutex<DurableServerPrimaryWorkspaceState>,
}

struct DurableServerPrimaryWorkspaceState {
    committed: Option<Value>,
    revision: Option<FileRevision>,
    staged: Option<Option<Value>>,
    recovery_required: bool,
}

impl DurableServerPrimaryWorkspaceStore {
    pub fn open(
        root: &InstanceDataRoot,
        launch_epoch: &KernelLaunchEpoch,
    ) -> Result<Self, PrimaryWorkspaceStoreError> {
        root.verify_held_directory()
            .map_err(|_| PrimaryWorkspaceStoreError::unavailable())?;
        let durable = DurableFileStore::at_instance_data(root, launch_epoch)
            .map_err(|_| PrimaryWorkspaceStoreError::unavailable())?;
        let recovery = durable
            .recover()
            .map_err(|_| PrimaryWorkspaceStoreError::unavailable())?;
        if recovery
            .iter()
            .any(|outcome| matches!(outcome, RecoveryOutcome::ManualInterventionRequired { .. }))
        {
            return Err(PrimaryWorkspaceStoreError::unavailable());
        }
        let target = StorageFileName::parse(SERVER_PRIMARY_WORKSPACE_FILE)
            .map_err(|_| PrimaryWorkspaceStoreError::unavailable())?;
        let stored = durable
            .read(&target, MAXIMUM_SERVER_PRIMARY_WORKSPACE_BYTES)
            .map_err(|_| PrimaryWorkspaceStoreError::unavailable())?;
        let (committed, revision) = match stored {
            Some(stored) => {
                let value = decode_server_primary_workspace(&stored.bytes)?;
                (Some(value), Some(stored.revision.clone()))
            }
            None => (None, None),
        };
        Ok(Self {
            binding: PrimaryWorkspaceRepositoryBinding::new(),
            durable,
            target,
            state: Mutex::new(DurableServerPrimaryWorkspaceState {
                committed,
                revision,
                staged: None,
                recovery_required: false,
            }),
        })
    }

    fn persist(
        &self,
        state: &mut DurableServerPrimaryWorkspaceState,
        value: &Value,
    ) -> Result<(), PrimaryWorkspaceStoreError> {
        let bytes =
            serde_json::to_vec(value).map_err(|_| PrimaryWorkspaceStoreError::unavailable())?;
        let expected = match state.revision.as_ref() {
            Some(revision) => ExpectedFile::Revision(revision),
            None => ExpectedFile::Absent,
        };
        match self.durable.replace(ReplaceRequest {
            target: &self.target,
            bytes: &bytes,
            expected,
            preserve_previous: PreservePrevious::None,
        }) {
            Ok(outcome) => {
                state.committed = Some(value.clone());
                state.revision = Some(outcome.installed_revision);
                state.staged = None;
                if outcome.commit_state == CommitState::PublishedDurabilityUncertain {
                    state.recovery_required = true;
                    Err(PrimaryWorkspaceStoreError::unavailable())
                } else {
                    Ok(())
                }
            }
            Err(error) if error.kind() == DurableFileFailureKind::NotPublished => {
                Err(PrimaryWorkspaceStoreError::unavailable())
            }
            Err(_error) => {
                state.recovery_required = true;
                state.staged = None;
                Err(PrimaryWorkspaceStoreError::unavailable())
            }
        }
    }
}

impl PrimaryWorkspaceStore for DurableServerPrimaryWorkspaceStore {
    fn repository_binding(&self) -> PrimaryWorkspaceRepositoryBinding {
        self.binding.clone()
    }

    fn load(&self) -> Result<Option<Value>, PrimaryWorkspaceStoreError> {
        let state = self
            .state
            .lock()
            .map_err(|_| PrimaryWorkspaceStoreError::unavailable())?;
        if state.recovery_required {
            return Err(PrimaryWorkspaceStoreError::unavailable());
        }
        Ok(state
            .staged
            .as_ref()
            .cloned()
            .unwrap_or_else(|| state.committed.clone()))
    }

    fn replace(&self, value: Option<Value>) -> Result<(), PrimaryWorkspaceStoreError> {
        let value = value.map(validate_server_primary_workspace).transpose()?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| PrimaryWorkspaceStoreError::unavailable())?;
        if state.recovery_required || (value.is_none() && state.committed.is_some()) {
            return Err(PrimaryWorkspaceStoreError::unavailable());
        }
        state.staged = Some(value);
        Ok(())
    }

    fn save(&self) -> Result<(), PrimaryWorkspaceStoreError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| PrimaryWorkspaceStoreError::unavailable())?;
        if state.recovery_required {
            return Err(PrimaryWorkspaceStoreError::unavailable());
        }
        let Some(staged) = state.staged.clone() else {
            return Ok(());
        };
        match staged {
            Some(value) => self.persist(&mut state, &value),
            None if state.committed.is_none() => {
                state.staged = None;
                Ok(())
            }
            None => Err(PrimaryWorkspaceStoreError::unavailable()),
        }
    }
}

impl fmt::Debug for DurableServerPrimaryWorkspaceStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DurableServerPrimaryWorkspaceStore(..)")
    }
}

fn validate_server_primary_workspace(value: Value) -> Result<Value, PrimaryWorkspaceStoreError> {
    let state = PrimaryWorkspaceState::from_value(value)
        .map_err(|_| PrimaryWorkspaceStoreError::unavailable())?;
    state
        .to_value()
        .map_err(|_| PrimaryWorkspaceStoreError::unavailable())
}

fn decode_server_primary_workspace(bytes: &[u8]) -> Result<Value, PrimaryWorkspaceStoreError> {
    let value =
        serde_json::from_slice(bytes).map_err(|_| PrimaryWorkspaceStoreError::unavailable())?;
    validate_server_primary_workspace(value)
}

/// Process-local identity for one host-owned primary-workspace repository.
///
/// Clones preserve identity, while `new` always creates a distinct binding.
/// The identity has no serializable representation and its debug form exposes
/// neither an address nor host-private repository details.
#[derive(Clone)]
pub struct PrimaryWorkspaceRepositoryBinding {
    identity: Arc<()>,
}

impl PrimaryWorkspaceRepositoryBinding {
    pub fn new() -> Self {
        Self {
            identity: Arc::new(()),
        }
    }

    pub fn matches(&self, candidate: &Self) -> bool {
        Arc::ptr_eq(&self.identity, &candidate.identity)
    }
}

impl Default for PrimaryWorkspaceRepositoryBinding {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for PrimaryWorkspaceRepositoryBinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PrimaryWorkspaceRepositoryBinding([REDACTED])")
    }
}

/// Unforgeable process-local identity minted for one prepared authority.
#[derive(Clone)]
pub struct PreparedWorkspaceAuthorityBinding {
    identity: Arc<()>,
}

impl PreparedWorkspaceAuthorityBinding {
    pub(crate) fn new() -> Self {
        Self {
            identity: Arc::new(()),
        }
    }

    pub(crate) fn matches(&self, candidate: &Self) -> bool {
        Arc::ptr_eq(&self.identity, &candidate.identity)
    }
}

impl fmt::Debug for PreparedWorkspaceAuthorityBinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PreparedWorkspaceAuthorityBinding([REDACTED])")
    }
}

/// Host-owned persistence for the opaque primary-workspace state.
///
/// The `Value` is the Kernel's canonical view, not permission to create a
/// second host record. A host with an existing primary-workspace schema must
/// codec this view through that same durable key. This legacy compatibility
/// interface does not itself prove failure atomicity; new host-selected
/// workspace changes must use `AtomicHostWorkspaceTransaction`.
pub trait PrimaryWorkspaceStore: Send + Sync {
    fn repository_binding(&self) -> PrimaryWorkspaceRepositoryBinding;
    fn load(&self) -> Result<Option<Value>, PrimaryWorkspaceStoreError>;
    fn replace(&self, value: Option<Value>) -> Result<(), PrimaryWorkspaceStoreError>;
    fn save(&self) -> Result<(), PrimaryWorkspaceStoreError>;
}

/// One host-owned, atomic update of the existing primary-workspace record.
///
/// The host implementation owns every platform-private field (including any
/// local path) and codecs the Kernel's opaque canonical value into that same
/// durable record. The operation is consumed and has exactly one durable
/// commit point. It must not create a second Kernel-owned record or implement
/// failure handling as a compensating second write.
///
/// A prepared implementation must also retain the host-private target and its
/// path-transition reservation. At the commit point it must prove that the
/// retained directory identity and lease still designate that target. A
/// failure before the durable commit is `NoCommit`; any ambiguity discovered
/// after the commit starts is `OutcomeUnknown`. Absolute paths stay inside the
/// host implementation and never enter the canonical `Value`.
///
/// The binding accessors must be side-effect-free, non-blocking identity
/// reads. `compare_and_commit` runs without the Kernel's current-workspace or
/// active-authority locks, so a host may perform read-only runtime/service
/// checks. The caller holds the workspace mutation coordinator across every
/// trait call: no method may synchronously re-enter a mutation that needs that
/// same coordinator.
pub trait AtomicHostWorkspaceTransaction: Send {
    fn repository_binding(&self) -> PrimaryWorkspaceRepositoryBinding;
    fn authority_binding(&self) -> PreparedWorkspaceAuthorityBinding;

    fn compare_and_commit(
        self: Box<Self>,
        expected_kernel_value: Option<&Value>,
        next_kernel_value: Value,
    ) -> Result<(), AtomicHostWorkspaceCommitError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AtomicHostWorkspaceCommitErrorKind {
    /// The expected host record did not match; durable state is unchanged.
    Conflict,
    /// The commit could not start or complete; durable state is unchanged.
    NoCommit,
    /// The implementation cannot determine whether the durable commit landed.
    OutcomeUnknown,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct AtomicHostWorkspaceCommitError {
    kind: AtomicHostWorkspaceCommitErrorKind,
}

impl AtomicHostWorkspaceCommitError {
    pub const fn conflict() -> Self {
        Self {
            kind: AtomicHostWorkspaceCommitErrorKind::Conflict,
        }
    }

    pub const fn no_commit() -> Self {
        Self {
            kind: AtomicHostWorkspaceCommitErrorKind::NoCommit,
        }
    }

    pub const fn outcome_unknown() -> Self {
        Self {
            kind: AtomicHostWorkspaceCommitErrorKind::OutcomeUnknown,
        }
    }

    pub const fn kind(self) -> AtomicHostWorkspaceCommitErrorKind {
        self.kind
    }
}

impl fmt::Debug for AtomicHostWorkspaceCommitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AtomicHostWorkspaceCommitError")
            .field("kind", &self.kind)
            .finish()
    }
}

impl fmt::Display for AtomicHostWorkspaceCommitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("atomic host workspace commit failed")
    }
}

impl std::error::Error for AtomicHostWorkspaceCommitError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrimaryWorkspaceStoreErrorKind {
    Unavailable,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct PrimaryWorkspaceStoreError {
    kind: PrimaryWorkspaceStoreErrorKind,
}

impl PrimaryWorkspaceStoreError {
    pub const fn unavailable() -> Self {
        Self {
            kind: PrimaryWorkspaceStoreErrorKind::Unavailable,
        }
    }

    pub const fn kind(self) -> PrimaryWorkspaceStoreErrorKind {
        self.kind
    }
}

impl fmt::Debug for PrimaryWorkspaceStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PrimaryWorkspaceStoreError")
            .field("kind", &self.kind)
            .finish()
    }
}

impl fmt::Display for PrimaryWorkspaceStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("primary workspace storage is unavailable")
    }
}

impl std::error::Error for PrimaryWorkspaceStoreError {}

#[cfg(test)]
mod durable_server_tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;
    use crate::{config::KernelConfig, paths::KernelPaths};

    fn fixture() -> (tempfile::TempDir, KernelPaths, KernelConfig) {
        let temporary = tempdir().unwrap();
        let data = temporary.path().join("data");
        let cache = temporary.path().join("cache");
        fs::create_dir(&data).unwrap();
        let paths = crate::paths::ServerPathLayout::for_test(&data, &cache)
            .activate()
            .unwrap();
        (temporary, paths, KernelConfig::generate().unwrap())
    }

    #[test]
    fn durable_server_store_persists_one_valid_workspace_state_across_restarts() {
        let (_temporary, paths, config) = fixture();
        let store = DurableServerPrimaryWorkspaceStore::open(
            paths.instance_data_root(),
            config.launch_epoch(),
        )
        .unwrap();
        let expected = PrimaryWorkspaceState::new("Notes")
            .unwrap()
            .to_value()
            .unwrap();

        assert_eq!(store.load().unwrap(), None);
        store.replace(Some(expected.clone())).unwrap();
        assert_eq!(store.load().unwrap(), Some(expected.clone()));
        store.save().unwrap();
        drop(store);

        let reopened = DurableServerPrimaryWorkspaceStore::open(
            paths.instance_data_root(),
            KernelConfig::generate().unwrap().launch_epoch(),
        )
        .unwrap();
        assert_eq!(reopened.load().unwrap(), Some(expected));
    }

    #[test]
    fn durable_server_store_rejects_invalid_or_removal_replacements() {
        let (_temporary, paths, config) = fixture();
        let store = DurableServerPrimaryWorkspaceStore::open(
            paths.instance_data_root(),
            config.launch_epoch(),
        )
        .unwrap();

        assert!(store
            .replace(Some(serde_json::json!({"unsafe": true})))
            .is_err());
        let expected = PrimaryWorkspaceState::new("Notes")
            .unwrap()
            .to_value()
            .unwrap();
        store.replace(Some(expected)).unwrap();
        store.save().unwrap();
        assert!(store.replace(None).is_err());
    }

    #[test]
    fn durable_server_store_latches_closed_on_a_conflicting_external_record() {
        let (temporary, paths, config) = fixture();
        let store = DurableServerPrimaryWorkspaceStore::open(
            paths.instance_data_root(),
            config.launch_epoch(),
        )
        .unwrap();
        let staged = PrimaryWorkspaceState::new("Notes")
            .unwrap()
            .to_value()
            .unwrap();
        let conflicting = PrimaryWorkspaceState::new("Conflicting")
            .unwrap()
            .to_value()
            .unwrap();
        store.replace(Some(staged)).unwrap();
        fs::write(
            temporary
                .path()
                .join("data/state")
                .join(SERVER_PRIMARY_WORKSPACE_FILE),
            serde_json::to_vec(&conflicting).unwrap(),
        )
        .unwrap();

        assert!(store.save().is_err());
        assert!(store.load().is_err());
    }

    #[cfg(unix)]
    #[test]
    fn durable_server_store_rejects_a_symlinked_primary_record_without_following_it() {
        use std::os::unix::fs::symlink;

        let (temporary, paths, config) = fixture();
        let outside = temporary.path().join("outside.json");
        fs::write(&outside, b"outside-must-not-be-read-or-replaced").unwrap();
        symlink(
            &outside,
            temporary
                .path()
                .join("data/state")
                .join(SERVER_PRIMARY_WORKSPACE_FILE),
        )
        .unwrap();

        assert!(DurableServerPrimaryWorkspaceStore::open(
            paths.instance_data_root(),
            config.launch_epoch(),
        )
        .is_err());
        assert_eq!(
            fs::read(&outside).unwrap(),
            b"outside-must-not-be-read-or-replaced"
        );
    }
}
