//! Internal document model boundary.

use crate::contract::{Revision, Rfc3339Utc, SnapshotId, WorkspaceRelativePath};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistorySnapshot {
    pub snapshot_id: SnapshotId,
    pub document_path: WorkspaceRelativePath,
    pub created_at: Rfc3339Utc,
    pub contents: Vec<u8>,
    pub revision: Revision,
}
