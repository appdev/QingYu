//! Document identity boundary.

use std::fmt;

use crate::contract::{
    DocumentId, DocumentKind, InvalidWireIdentity, WireIdentityKey, WorkspaceDto,
    WorkspaceRelativePath,
};

/// Issues and verifies opaque document identifiers without exposing filesystem
/// addresses to callers. The workspace generation is part of the signed
/// identity, so switching workspaces invalidates every previously issued ID.
pub struct DocumentIdentityCodec<'a> {
    key: &'a WireIdentityKey,
}

impl<'a> DocumentIdentityCodec<'a> {
    pub const fn new(key: &'a WireIdentityKey) -> Self {
        Self { key }
    }

    pub fn issue(
        &self,
        workspace: &WorkspaceDto,
        kind: DocumentKind,
        relative_path: &WorkspaceRelativePath,
    ) -> Result<DocumentId, DocumentIdentityError> {
        self.key
            .issue_document_id(workspace.id, &workspace.generation, kind, relative_path)
            .map_err(DocumentIdentityError::from)
    }

    pub fn verify(
        &self,
        document_id: &DocumentId,
        workspace: &WorkspaceDto,
        expected_kind: DocumentKind,
    ) -> Result<WorkspaceRelativePath, DocumentIdentityError> {
        match self.key.verify_document_id(
            document_id,
            workspace.id,
            &workspace.generation,
            expected_kind,
        ) {
            Ok(path) => Ok(path),
            Err(_) => {
                let alternate_kind = match expected_kind {
                    DocumentKind::File => DocumentKind::Directory,
                    DocumentKind::Directory => DocumentKind::File,
                };
                if self
                    .key
                    .verify_document_id(
                        document_id,
                        workspace.id,
                        &workspace.generation,
                        alternate_kind,
                    )
                    .is_ok()
                {
                    Err(DocumentIdentityError::wrong_kind())
                } else {
                    Err(DocumentIdentityError::invalid_or_stale())
                }
            }
        }
    }
}

impl fmt::Debug for DocumentIdentityCodec<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DocumentIdentityCodec(..)")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DocumentIdentityErrorKind {
    InvalidOrStaleIdentity,
    WrongKind,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct DocumentIdentityError {
    kind: DocumentIdentityErrorKind,
}

impl DocumentIdentityError {
    const fn invalid_or_stale() -> Self {
        Self {
            kind: DocumentIdentityErrorKind::InvalidOrStaleIdentity,
        }
    }

    const fn wrong_kind() -> Self {
        Self {
            kind: DocumentIdentityErrorKind::WrongKind,
        }
    }

    pub const fn kind(self) -> DocumentIdentityErrorKind {
        self.kind
    }
}

impl From<InvalidWireIdentity> for DocumentIdentityError {
    fn from(_error: InvalidWireIdentity) -> Self {
        Self::invalid_or_stale()
    }
}

impl fmt::Debug for DocumentIdentityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DocumentIdentityError")
            .field("kind", &self.kind)
            .finish()
    }
}

impl fmt::Display for DocumentIdentityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            DocumentIdentityErrorKind::InvalidOrStaleIdentity => {
                formatter.write_str("the document identity is invalid or stale")
            }
            DocumentIdentityErrorKind::WrongKind => {
                formatter.write_str("the document identity kind does not match the operation")
            }
        }
    }
}

impl std::error::Error for DocumentIdentityError {}
