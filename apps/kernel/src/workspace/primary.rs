//! Primary-workspace persistence boundary.

use std::fmt;

use serde_json::Value;

/// Host-owned persistence for the opaque primary-workspace state.
///
/// The `Value` is the Kernel's canonical view, not permission to create a
/// second host record. A host with an existing primary-workspace schema must
/// codec this view through that same durable key. This legacy compatibility
/// interface does not itself prove failure atomicity; new host-selected
/// workspace changes must use `AtomicHostWorkspaceTransaction`.
pub trait PrimaryWorkspaceStore: Send + Sync {
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
pub trait AtomicHostWorkspaceTransaction: Send {
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
