//! Kernel-owned document operations.

use cap_std::fs::Dir;

use crate::contract::{DeletionPolicy, DocumentKind, Revision, WorkspaceRelativePath};

pub mod history;
pub mod identity;
pub mod search;
pub mod service;
pub mod types;

pub(crate) const DOCUMENT_STAGE_PREFIX: &str = ".qingyu-kernel-update-";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentDeletionTarget {
    pub path: WorkspaceRelativePath,
    pub kind: DocumentKind,
    pub revision: Revision,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AtomicInstallMode {
    CreateNoReplace,
    ReplaceExisting,
}

#[derive(Clone, Copy)]
pub enum PinnedInstallSource<'a> {
    File(&'a cap_std::fs::File),
    Directory(&'a Dir),
}

pub struct AtomicInstallRequest<'a> {
    pub directory: &'a Dir,
    pub target: &'a WorkspaceRelativePath,
    pub stage_name: &'a str,
    pub target_name: &'a str,
    pub mode: AtomicInstallMode,
    pub expected_stage: PinnedInstallSource<'a>,
    pub expected_target: Option<&'a cap_std::fs::File>,
    pub expected_revision: Option<&'a Revision>,
}

/// Host-overridable atomic installation seam.
///
/// The source capability is retained from staging through publication. Default
/// platform adapters use it as the mutation authority where the OS supports a
/// handle-relative rename. Revision checks protect cooperating Kernel writers;
/// this is not a strict operating-system compare-and-swap against arbitrary
/// external mutators.
pub trait AtomicInstallPort: Send + Sync {
    fn install(&self, request: AtomicInstallRequest<'_>) -> Result<(), AtomicInstallPortError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AtomicInstallPortError;

#[derive(Clone, Copy)]
pub enum PinnedMoveSource<'a> {
    File(&'a cap_std::fs::File),
    Directory(&'a Dir),
}

pub struct MoveInstallRequest<'a> {
    pub source_directory: &'a Dir,
    pub source_name: &'a str,
    pub target_directory: &'a Dir,
    pub target_name: &'a str,
    pub kind: DocumentKind,
    pub expected_source: PinnedMoveSource<'a>,
    pub expected_revision: &'a Revision,
}

pub trait MoveInstallPort: Send + Sync {
    fn install(&self, request: MoveInstallRequest<'_>) -> Result<(), MoveInstallPortError>;
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CapabilityMoveInstallPort;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MoveInstallPortError {
    AlreadyExists,
    RevisionConflict(Revision),
    UnavailableNoMutation,
    RecoveryRequired,
}

pub trait DocumentIgnorePort: Send + Sync {
    fn is_ignored(&self, path: &WorkspaceRelativePath, kind: DocumentKind) -> bool;
}

#[derive(Default)]
pub struct AllowAllDocumentIgnorePort;

impl DocumentIgnorePort for AllowAllDocumentIgnorePort {
    fn is_ignored(&self, _path: &WorkspaceRelativePath, _kind: DocumentKind) -> bool {
        false
    }
}

/// Host deletion capability. Callers can supply only a validated logical
/// workspace target; absolute filesystem addresses never cross this boundary.
pub trait DeletionPort: Send + Sync {
    fn delete(
        &self,
        target: &DocumentDeletionTarget,
        policy: DeletionPolicy,
    ) -> Result<(), DeletionPortError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeletionPortError;

impl std::fmt::Display for DeletionPortError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("the document deletion capability is unavailable")
    }
}

impl std::error::Error for DeletionPortError {}
