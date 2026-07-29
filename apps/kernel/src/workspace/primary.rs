//! Primary-workspace persistence boundary.

use std::fmt;

use serde_json::Value;

/// Host-owned persistence for the opaque primary-workspace state.
///
/// The `Value` is the Kernel's canonical view, not permission to create a
/// second host record. A host with an existing primary-workspace schema must
/// codec this view through that same durable key and preserve atomic rollback.
pub trait PrimaryWorkspaceStore: Send + Sync {
    fn load(&self) -> Result<Option<Value>, PrimaryWorkspaceStoreError>;
    fn replace(&self, value: Option<Value>) -> Result<(), PrimaryWorkspaceStoreError>;
    fn save(&self) -> Result<(), PrimaryWorkspaceStoreError>;
}

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
