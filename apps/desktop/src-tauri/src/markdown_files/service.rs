use std::{
    fmt,
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use cap_fs_ext::{DirExt, FollowSymlinks, MetadataExt, OpenOptionsExt, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[cfg(windows)]
use qingyu_kernel::documents::service::CapabilityAtomicInstallPort;
#[cfg(not(windows))]
use qingyu_kernel::documents::{AtomicInstallMode, PinnedInstallSource};
use qingyu_kernel::{
    contract::{DeletionPolicy as KernelDeletionPolicy, DocumentKind, DocumentName},
    documents::{
        AtomicInstallPort, AtomicInstallPortError, AtomicInstallRequest, DeletionPort,
        DeletionPortError, DocumentDeletionTarget, DocumentIgnorePort,
    },
    runtime::{DocumentsApiService, ServiceFailure},
};

use crate::mcp::{
    config::{DeletionPolicy, SyncAfterWritePolicy},
    handles::{HandleSigner, VerifiedDocumentHandle, VerifiedFolderHandle},
    workspaces::{ResolvedWorkspace, WorkspaceError},
};

use super::{
    history::snapshot_markdown_file_history_contents,
    ignore_rules::{try_markdown_ignore_rules_for_root, MarkdownIgnoreRules},
    path::is_markdown_tree_file,
    search::{markdown_search_line, markdown_search_ranges, markdown_search_snippet},
    types::MarkdownFile,
};

const DOCUMENT_PAGE_LIMIT: usize = 100;
const DEFAULT_DOCUMENT_LIMIT_BYTES: u64 = 8 * 1024 * 1024;
const MAX_SEARCH_RESULTS: usize = 10_000;
const CURSOR_VERSION: u8 = 1;
const UPDATE_TEMP_PREFIX: &str = ".qingyu-mcp-update-";

type SystemTrash = dyn Fn(&Path) -> Result<(), String> + Send + Sync;
const MAX_KERNEL_DOCUMENT_BYTES: usize = 16 * 1024 * 1024;
#[cfg(test)]
type BeforeAtomicDocumentMutation = dyn Fn() + Send + Sync;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct KernelTauriDocumentError {
    code: qingyu_kernel::contract::ErrorCode,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<qingyu_kernel::contract::ErrorDetails>,
}

impl KernelTauriDocumentError {
    pub(crate) const fn code(&self) -> qingyu_kernel::contract::ErrorCode {
        self.code
    }
}

impl From<ServiceFailure> for KernelTauriDocumentError {
    fn from(error: ServiceFailure) -> Self {
        Self {
            code: error.code(),
            details: error.details().cloned(),
        }
    }
}

/// Thin uncomposed compatibility facade for future Tauri command ownership.
/// It deliberately returns the exact Kernel DTOs and safe error codes.
pub(crate) struct KernelDocumentsTauriFacade {
    service: Arc<dyn DocumentsApiService>,
}

impl KernelDocumentsTauriFacade {
    pub(crate) fn new(service: Arc<dyn DocumentsApiService>) -> Self {
        Self { service }
    }

    pub(crate) async fn list(
        &self,
        query: qingyu_kernel::contract::ListDocumentsQuery,
    ) -> Result<qingyu_kernel::contract::DocumentPageDto, KernelTauriDocumentError> {
        self.service.list_documents(query).await.map_err(Into::into)
    }

    pub(crate) async fn create(
        &self,
        request: qingyu_kernel::contract::CreateDocumentRequest,
    ) -> Result<qingyu_kernel::contract::CreatedDocumentDto, KernelTauriDocumentError> {
        self.service
            .create_document(request)
            .await
            .map_err(Into::into)
    }

    pub(crate) async fn read(
        &self,
        document_id: qingyu_kernel::contract::DocumentId,
    ) -> Result<qingyu_kernel::contract::DocumentContentDto, KernelTauriDocumentError> {
        self.service
            .get_document(document_id)
            .await
            .map_err(Into::into)
    }

    pub(crate) async fn write(
        &self,
        document_id: qingyu_kernel::contract::DocumentId,
        request: qingyu_kernel::contract::UpdateDocumentRequest,
    ) -> Result<qingyu_kernel::contract::DocumentContentDto, KernelTauriDocumentError> {
        self.service
            .update_document(document_id, request)
            .await
            .map_err(Into::into)
    }

    pub(crate) async fn history(
        &self,
        document_id: qingyu_kernel::contract::DocumentId,
        query: qingyu_kernel::contract::PageQuery,
    ) -> Result<qingyu_kernel::contract::DocumentHistoryPageDto, KernelTauriDocumentError> {
        self.service
            .list_document_history(document_id, query)
            .await
            .map_err(Into::into)
    }

    pub(crate) async fn restore(
        &self,
        document_id: qingyu_kernel::contract::DocumentId,
        snapshot_id: qingyu_kernel::contract::SnapshotId,
        request: qingyu_kernel::contract::RestoreDocumentHistoryRequest,
    ) -> Result<qingyu_kernel::contract::DocumentContentDto, KernelTauriDocumentError> {
        self.service
            .restore_document_history(document_id, snapshot_id, request)
            .await
            .map_err(Into::into)
    }

    pub(crate) async fn search(
        &self,
        query: qingyu_kernel::contract::SearchWorkspaceQuery,
    ) -> Result<qingyu_kernel::contract::SearchPageDto, KernelTauriDocumentError> {
        self.service
            .search_workspace(query)
            .await
            .map_err(Into::into)
    }
}

/// Uncomposed host adapter for Kernel atomic installs. Its Windows branch
/// delegates to the Kernel's retained-handle implementation after verifying
/// that the supplied parent belongs to this workspace root.
pub(crate) struct KernelDocumentAtomicInstallAdapter {
    workspace_root: PathBuf,
    retained_root: Dir,
    root_identity: cap_std::fs::Metadata,
}

impl KernelDocumentAtomicInstallAdapter {
    pub(crate) fn new(workspace_root: &Path) -> Result<Self, String> {
        let workspace_root = workspace_root
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let retained_root = Dir::open_ambient_dir(&workspace_root, cap_std::ambient_authority())
            .map_err(|error| error.to_string())?;
        let root_identity = retained_root
            .dir_metadata()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            workspace_root,
            retained_root,
            root_identity,
        })
    }

    fn verified_parent(
        &self,
        target: &qingyu_kernel::contract::WorkspaceRelativePath,
        supplied: &Dir,
    ) -> Result<(Dir, PathBuf), AtomicInstallPortError> {
        let current = Dir::open_ambient_dir(&self.workspace_root, cap_std::ambient_authority())
            .and_then(|directory| directory.dir_metadata())
            .map_err(|_| AtomicInstallPortError)?;
        if MetadataExt::dev(&self.root_identity) != MetadataExt::dev(&current)
            || MetadataExt::ino(&self.root_identity) != MetadataExt::ino(&current)
        {
            return Err(AtomicInstallPortError);
        }
        let relative = Path::new(target.as_str());
        let parent_relative = relative.parent().unwrap_or_else(|| Path::new(""));
        let mut retained = self
            .retained_root
            .try_clone()
            .map_err(|_| AtomicInstallPortError)?;
        for component in parent_relative.components() {
            let Component::Normal(segment) = component else {
                return Err(AtomicInstallPortError);
            };
            retained = retained
                .open_dir_nofollow(segment)
                .map_err(|_| AtomicInstallPortError)?;
        }
        let expected = retained
            .dir_metadata()
            .map_err(|_| AtomicInstallPortError)?;
        let actual = supplied
            .dir_metadata()
            .map_err(|_| AtomicInstallPortError)?;
        if MetadataExt::dev(&expected) != MetadataExt::dev(&actual)
            || MetadataExt::ino(&expected) != MetadataExt::ino(&actual)
        {
            return Err(AtomicInstallPortError);
        }
        Ok((retained, self.workspace_root.join(parent_relative)))
    }
}

impl AtomicInstallPort for KernelDocumentAtomicInstallAdapter {
    fn install(&self, request: AtomicInstallRequest<'_>) -> Result<(), AtomicInstallPortError> {
        let (parent, ambient_parent) = self.verified_parent(request.target, request.directory)?;
        #[cfg(windows)]
        {
            let _ = ambient_parent;
            return CapabilityAtomicInstallPort.install(AtomicInstallRequest {
                directory: &parent,
                target: request.target,
                stage_name: request.stage_name,
                target_name: request.target_name,
                mode: request.mode,
                expected_stage: request.expected_stage,
                expected_target: request.expected_target,
                expected_revision: request.expected_revision,
            });
        }
        #[cfg(not(windows))]
        {
            // These legacy helpers publish by name. Confirm that name still
            // resolves to the retained stage and that its logical contents
            // can be read stably before any target mutation.
            verify_kernel_named_pinned_install_source(
                &parent,
                request.stage_name,
                request.expected_stage,
            )
            .map_err(|_| AtomicInstallPortError)?;
            let target_ambient = ambient_parent.join(request.target_name);
            let stage_ambient = ambient_parent.join(request.stage_name);
            match request.mode {
                AtomicInstallMode::CreateNoReplace => rename_document_noreplace(
                    &parent,
                    request.stage_name,
                    &parent,
                    request.target_name,
                    &stage_ambient,
                    &target_ambient,
                ),
                AtomicInstallMode::ReplaceExisting => replace_kernel_document_compare_exchange(
                    &parent,
                    &request,
                    &stage_ambient,
                    &target_ambient,
                ),
            }
            .map_err(|_| AtomicInstallPortError)
        }
    }
}

pub(crate) struct KernelDocumentIgnoreAdapter {
    workspace_root: PathBuf,
    rules: MarkdownIgnoreRules,
}

impl KernelDocumentIgnoreAdapter {
    pub(crate) fn new(
        workspace_root: &Path,
        retained_root: &Dir,
        global_rules: Option<&str>,
    ) -> Result<Self, String> {
        let workspace_root = workspace_root
            .canonicalize()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            rules: MarkdownIgnoreRules::try_for_retained_root(
                &workspace_root,
                retained_root,
                global_rules,
            )
            .map_err(|error| error.to_string())?,
            workspace_root,
        })
    }
}

impl DocumentIgnorePort for KernelDocumentIgnoreAdapter {
    fn is_ignored(
        &self,
        path: &qingyu_kernel::contract::WorkspaceRelativePath,
        kind: DocumentKind,
    ) -> bool {
        self.rules.ignores(
            &self.workspace_root.join(path.as_str()),
            kind == DocumentKind::Directory,
        )
    }
}

/// Uncomposed Phase 1 adapter seam. It captures the trusted desktop workspace
/// root once; Kernel callers can provide only validated relative identities.
pub(crate) struct KernelDocumentDeletionAdapter {
    workspace_root: PathBuf,
    retained_root: Dir,
    root_identity: cap_std::fs::Metadata,
    system_trash: Arc<SystemTrash>,
    #[cfg(test)]
    before_delete: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl KernelDocumentDeletionAdapter {
    pub(crate) fn new(
        workspace_root: &Path,
        system_trash: Arc<SystemTrash>,
    ) -> Result<Self, String> {
        let workspace_root = workspace_root
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let retained_root = Dir::open_ambient_dir(&workspace_root, cap_std::ambient_authority())
            .map_err(|error| error.to_string())?;
        let root_identity = retained_root
            .dir_metadata()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            retained_root,
            root_identity,
            workspace_root,
            system_trash,
            #[cfg(test)]
            before_delete: None,
        })
    }

    #[cfg(test)]
    fn with_before_delete(mut self, hook: Arc<dyn Fn() + Send + Sync>) -> Self {
        self.before_delete = Some(hook);
        self
    }

    fn verify_root(&self) -> Result<(), DeletionPortError> {
        let current = Dir::open_ambient_dir(&self.workspace_root, cap_std::ambient_authority())
            .and_then(|directory| directory.dir_metadata())
            .map_err(|_| DeletionPortError)?;
        (MetadataExt::dev(&self.root_identity) == MetadataExt::dev(&current)
            && MetadataExt::ino(&self.root_identity) == MetadataExt::ino(&current))
        .then_some(())
        .ok_or(DeletionPortError)
    }

    fn parent_and_name(
        &self,
        target: &DocumentDeletionTarget,
    ) -> Result<(Dir, String), DeletionPortError> {
        if target.path.as_str().is_empty() {
            return Err(DeletionPortError);
        }
        let path = Path::new(target.path.as_str());
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or(DeletionPortError)?;
        let mut parent = self
            .retained_root
            .try_clone()
            .map_err(|_| DeletionPortError)?;
        for component in path.parent().unwrap_or_else(|| Path::new("")).components() {
            let Component::Normal(segment) = component else {
                return Err(DeletionPortError);
            };
            parent = parent
                .open_dir_nofollow(segment)
                .map_err(|_| DeletionPortError)?;
        }
        Ok((parent, name.to_string()))
    }

    fn verify_target(
        &self,
        target: &DocumentDeletionTarget,
        parent: &Dir,
        name: &str,
    ) -> Result<(), DeletionPortError> {
        let metadata = parent
            .symlink_metadata(name)
            .map_err(|_| DeletionPortError)?;
        if metadata.file_type().is_symlink()
            || (target.kind == DocumentKind::File && !metadata.is_file())
            || (target.kind == DocumentKind::Directory && !metadata.is_dir())
        {
            return Err(DeletionPortError);
        }
        let actual = if metadata.is_file() {
            if !kernel_trusted_file_metadata(&metadata)
                || metadata.len() > MAX_KERNEL_DOCUMENT_BYTES as u64
            {
                return Err(DeletionPortError);
            }
            let mut options = OpenOptions::new();
            options.read(true).follow(FollowSymlinks::No);
            let mut file = parent
                .open_with(name, &options)
                .map_err(|_| DeletionPortError)?;
            let before = file.metadata().map_err(|_| DeletionPortError)?;
            if !kernel_trusted_file_metadata(&before)
                || MetadataExt::dev(&metadata) != MetadataExt::dev(&before)
                || MetadataExt::ino(&metadata) != MetadataExt::ino(&before)
                || before.len() != metadata.len()
            {
                return Err(DeletionPortError);
            }
            let mut bytes = Vec::with_capacity(before.len() as usize);
            (&mut file)
                .take(MAX_KERNEL_DOCUMENT_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| DeletionPortError)?;
            let after = file.metadata().map_err(|_| DeletionPortError)?;
            let latest = parent
                .symlink_metadata(name)
                .map_err(|_| DeletionPortError)?;
            if bytes.len() > MAX_KERNEL_DOCUMENT_BYTES
                || !kernel_trusted_file_metadata(&after)
                || !kernel_trusted_file_metadata(&latest)
                || MetadataExt::dev(&before) != MetadataExt::dev(&after)
                || MetadataExt::ino(&before) != MetadataExt::ino(&after)
                || MetadataExt::dev(&after) != MetadataExt::dev(&latest)
                || MetadataExt::ino(&after) != MetadataExt::ino(&latest)
                || before.len() != after.len()
                || after.len() != bytes.len() as u64
                || before.modified().ok() != after.modified().ok()
            {
                return Err(DeletionPortError);
            }
            format!("{:x}", Sha256::digest(bytes))
        } else {
            let retained = parent
                .open_dir_nofollow(name)
                .map_err(|_| DeletionPortError)?;
            let retained_metadata = retained.dir_metadata().map_err(|_| DeletionPortError)?;
            if MetadataExt::dev(&metadata) != MetadataExt::dev(&retained_metadata)
                || MetadataExt::ino(&metadata) != MetadataExt::ino(&retained_metadata)
            {
                return Err(DeletionPortError);
            }
            qingyu_kernel::documents::service::directory_revision_for_capability(&retained)
                .map_err(|_| DeletionPortError)?
                .as_str()
                .to_string()
        };
        (actual == target.revision.as_str())
            .then_some(())
            .ok_or(DeletionPortError)
    }
}

impl DeletionPort for KernelDocumentDeletionAdapter {
    fn delete(
        &self,
        target: &DocumentDeletionTarget,
        policy: KernelDeletionPolicy,
    ) -> Result<(), DeletionPortError> {
        self.verify_root()?;
        let (parent, name) = self.parent_and_name(target)?;
        self.verify_target(target, &parent, &name)?;
        #[cfg(test)]
        if let Some(hook) = self.before_delete.as_ref() {
            hook();
        }
        self.verify_target(target, &parent, &name)?;
        let quarantine = format!(".qingyu-delete-{}.tmp", Uuid::new_v4());
        let original_ambient = self.workspace_root.join(target.path.as_str());
        let quarantine_ambient = self.workspace_root.join(&quarantine);
        rename_document_noreplace(
            &parent,
            &name,
            &self.retained_root,
            &quarantine,
            &original_ambient,
            &quarantine_ambient,
        )
        .map_err(|_| DeletionPortError)?;
        if self
            .verify_target(target, &self.retained_root, &quarantine)
            .is_err()
        {
            let _root_sync = sync_directory(&self.retained_root);
            let _parent_sync = sync_directory(&parent);
            return Err(DeletionPortError);
        }
        let result = match policy {
            KernelDeletionPolicy::Recoverable => {
                self.verify_root()?;
                let trash_result = (self.system_trash)(&quarantine_ambient);
                let root_is_still_current = self.verify_root().is_ok();
                let quarantine_is_absent = matches!(
                    self.retained_root.symlink_metadata(&quarantine),
                    Err(error) if error.kind() == io::ErrorKind::NotFound
                );
                if trash_result.is_ok() && root_is_still_current && quarantine_is_absent {
                    Ok(())
                } else {
                    Err(DeletionPortError)
                }
            }
            KernelDeletionPolicy::Permanent if target.kind == DocumentKind::File => self
                .retained_root
                .remove_file(&quarantine)
                .map_err(|_| DeletionPortError),
            KernelDeletionPolicy::Permanent => self
                .retained_root
                .remove_dir_all(&quarantine)
                .map_err(|_| DeletionPortError),
        };
        if result.is_err()
            && self
                .verify_target(target, &self.retained_root, &quarantine)
                .is_ok()
        {
            let _rollback = rename_document_noreplace(
                &self.retained_root,
                &quarantine,
                &parent,
                &name,
                &quarantine_ambient,
                &original_ambient,
            );
        }
        sync_directory(&self.retained_root).map_err(|_| DeletionPortError)?;
        sync_directory(&parent).map_err(|_| DeletionPortError)?;
        result
    }
}

#[derive(Clone)]
pub(crate) enum DocumentScope {
    Authorized {
        workspace: ResolvedWorkspace,
        global_ignore_rules: Option<String>,
    },
    TrustedUi {
        root: PathBuf,
        global_ignore_rules: Option<String>,
    },
}

impl DocumentScope {
    pub(crate) fn authorized(workspace: ResolvedWorkspace) -> Self {
        Self::Authorized {
            workspace,
            global_ignore_rules: None,
        }
    }

    pub(crate) fn with_global_ignore_rules(self, rules: Option<String>) -> Self {
        match self {
            Self::Authorized { workspace, .. } => Self::Authorized {
                workspace,
                global_ignore_rules: rules,
            },
            Self::TrustedUi { root, .. } => Self::TrustedUi {
                root,
                global_ignore_rules: rules,
            },
        }
    }

    fn authorized_workspace(&self) -> Result<&ResolvedWorkspace, DocumentServiceError> {
        match self {
            Self::Authorized { workspace, .. } => Ok(workspace),
            Self::TrustedUi { .. } => Err(DocumentServiceError::scope()),
        }
    }

    fn ignore_rules(&self) -> Result<MarkdownIgnoreRules, DocumentServiceError> {
        match self {
            Self::Authorized {
                workspace,
                global_ignore_rules,
            } => MarkdownIgnoreRules::try_for_retained_root(
                &workspace.canonical_path,
                workspace.root.as_ref(),
                global_ignore_rules.as_deref(),
            )
            .map_err(|_| DocumentServiceError::workspace_unavailable()),
            Self::TrustedUi {
                root,
                global_ignore_rules,
            } => try_markdown_ignore_rules_for_root(root, global_ignore_rules.as_deref())
                .map_err(|_| DocumentServiceError::workspace_unavailable()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DocumentEntryKind {
    Document,
    Folder,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DocumentEntry {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) relative_path: String,
    pub(crate) kind: DocumentEntryKind,
    pub(crate) size_bytes: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DocumentPage {
    pub(crate) entries: Vec<DocumentEntry>,
    pub(crate) next_cursor: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DocumentSearchHit {
    pub(crate) document_id: String,
    pub(crate) relative_path: String,
    pub(crate) line_number: usize,
    pub(crate) column_number: usize,
    pub(crate) snippet: String,
    pub(crate) matched_from: usize,
    pub(crate) matched_to: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DocumentSearchPage {
    pub(crate) results: Vec<DocumentSearchHit>,
    pub(crate) next_cursor: Option<String>,
    pub(crate) searched_document_count: usize,
    pub(crate) unreadable_document_count: usize,
    pub(crate) truncated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct DocumentRevision(pub(crate) String);

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DocumentSnapshot {
    pub(crate) document_id: String,
    pub(crate) relative_path: String,
    pub(crate) contents: String,
    pub(crate) size_bytes: u64,
    pub(crate) revision: DocumentRevision,
}

#[derive(Clone, Copy)]
pub(crate) struct CreateDocument<'a> {
    pub(crate) parent: &'a VerifiedFolderHandle,
    pub(crate) name: &'a str,
    pub(crate) contents: &'a str,
}

#[derive(Clone, Copy)]
pub(crate) struct UpdateDocument<'a> {
    pub(crate) document: &'a VerifiedDocumentHandle,
    pub(crate) contents: &'a str,
    pub(crate) expected_revision: &'a str,
}

#[derive(Clone, Copy)]
pub(crate) struct MoveDocument<'a> {
    pub(crate) document: &'a VerifiedDocumentHandle,
    pub(crate) target_parent: &'a VerifiedFolderHandle,
    pub(crate) new_name: &'a str,
    pub(crate) expected_revision: &'a str,
}

#[derive(Clone, Copy)]
pub(crate) struct DeleteDocument<'a> {
    pub(crate) document: &'a VerifiedDocumentHandle,
    pub(crate) expected_revision: &'a str,
    pub(crate) deletion: DeletionPolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SyncRequest {
    Requested,
    NotRequested,
}

#[derive(Clone, Copy)]
pub(crate) struct MutationOptions {
    pub(crate) sync_after_write: SyncAfterWritePolicy,
    pub(crate) workspace_sync_enabled: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DocumentMutation {
    pub(crate) document_id: String,
    pub(crate) relative_path: String,
    pub(crate) revision: DocumentRevision,
    pub(crate) sync_request: SyncRequest,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RecycleMetadata<'a> {
    workspace_id: Uuid,
    relative_path: &'a str,
    deleted_at: u64,
    revision: &'a str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DocumentServiceError {
    pub(crate) code: &'static str,
    message: &'static str,
}

impl DocumentServiceError {
    fn scope() -> Self {
        Self {
            code: "workspace_not_authorized",
            message: "The document scope is not authorized for MCP.",
        }
    }

    fn invalid_cursor() -> Self {
        Self {
            code: "invalid_cursor",
            message: "The document page cursor is invalid or stale.",
        }
    }

    fn unavailable() -> Self {
        Self {
            code: "document_not_found",
            message: "The document is unavailable.",
        }
    }

    fn workspace_unavailable() -> Self {
        Self {
            code: "workspace_unavailable",
            message: "The authorized workspace is currently unavailable.",
        }
    }

    fn primary_workspace_unavailable() -> Self {
        Self {
            code: "mcp-workspace-unavailable",
            message: "A valid primary notes workspace is required for MCP document tools.",
        }
    }

    fn stale_handle() -> Self {
        Self {
            code: "mcp-handle-stale",
            message: "The MCP object identifier belongs to an older primary workspace.",
        }
    }

    fn boundary() -> Self {
        Self {
            code: "path_boundary_violation",
            message: "The document path is outside its authorized workspace.",
        }
    }

    fn too_large() -> Self {
        Self {
            code: "document_too_large",
            message: "The document exceeds the configured MCP size limit.",
        }
    }

    fn invalid_encoding() -> Self {
        Self {
            code: "document_invalid_encoding",
            message: "The Markdown document is not valid UTF-8.",
        }
    }

    fn invalid_query() -> Self {
        Self {
            code: "invalid_query",
            message: "The document search query is invalid.",
        }
    }

    fn invalid_name() -> Self {
        Self {
            code: "invalid_document_name",
            message: "The document name must be one safe Markdown filename.",
        }
    }

    fn already_exists() -> Self {
        Self {
            code: "document_already_exists",
            message: "A document already exists at the requested destination.",
        }
    }

    fn revision_conflict() -> Self {
        Self {
            code: "revision_conflict",
            message: "The document changed after the supplied revision was read.",
        }
    }

    fn mutation_failed() -> Self {
        Self {
            code: "document_mutation_failed",
            message: "The document mutation could not be completed safely.",
        }
    }

    fn history_failed() -> Self {
        Self {
            code: "document_history_failed",
            message: "The current document could not be preserved in history.",
        }
    }

    fn recycle_unavailable() -> Self {
        Self {
            code: "recycle_bin_unavailable",
            message: "The QingYu recycle bin is unavailable.",
        }
    }
}

impl fmt::Display for DocumentServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for DocumentServiceError {}

#[derive(Clone)]
pub(crate) struct DocumentService {
    signer: HandleSigner,
    cursor_key: [u8; 32],
    search_document_limit_bytes: u64,
    history_root: Option<PathBuf>,
    recycle_root: Option<PathBuf>,
    #[cfg(test)]
    before_atomic_mutation: Option<Arc<BeforeAtomicDocumentMutation>>,
}

impl DocumentService {
    pub(crate) fn new(signer: HandleSigner) -> Self {
        let cursor_key = signer.derive_key(b"QingYu MCP document cursors v1");
        Self {
            signer,
            cursor_key,
            search_document_limit_bytes: DEFAULT_DOCUMENT_LIMIT_BYTES,
            history_root: None,
            recycle_root: None,
            #[cfg(test)]
            before_atomic_mutation: None,
        }
    }

    pub(crate) fn root_folder_id(
        &self,
        workspace: &ResolvedWorkspace,
    ) -> Result<String, DocumentServiceError> {
        self.signer
            .issue_folder(workspace.workspace_id, workspace.workspace_generation, "")
            .map_err(map_handle_error)
    }

    pub(crate) fn verify_folder(
        &self,
        folder_id: &str,
        registry: &crate::mcp::workspaces::WorkspaceRegistry,
    ) -> Result<VerifiedFolderHandle, DocumentServiceError> {
        self.signer
            .verify_folder(folder_id, registry)
            .map_err(map_handle_error)
    }

    pub(crate) fn verify_document(
        &self,
        document_id: &str,
        registry: &crate::mcp::workspaces::WorkspaceRegistry,
    ) -> Result<VerifiedDocumentHandle, DocumentServiceError> {
        self.signer
            .verify_document(document_id, registry)
            .map_err(map_handle_error)
    }

    pub(crate) fn with_mutation_storage(
        mut self,
        history_root: PathBuf,
        recycle_root: PathBuf,
    ) -> Self {
        self.history_root = Some(history_root);
        self.recycle_root = Some(recycle_root);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_before_atomic_mutation(
        mut self,
        hook: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        self.before_atomic_mutation = Some(Arc::new(hook));
        self
    }

    pub(crate) fn create(
        &self,
        scope: &DocumentScope,
        input: CreateDocument<'_>,
        options: MutationOptions,
    ) -> Result<DocumentMutation, DocumentServiceError> {
        let workspace = mutation_workspace(scope, input.parent.workspace_id())?;
        let name = validate_document_name(input.name)?;
        validate_mutation_size(input.contents.as_bytes(), self.search_document_limit_bytes)?;
        workspace
            .revalidate_authority()
            .map_err(map_workspace_error)?;
        let parent = input.parent.open_dir().map_err(map_handle_error)?;
        ensure_destination_absent(&parent, name)?;
        let staging_name = stage_document_contents(&parent, input.contents.as_bytes())?;
        let target_relative = input.parent.relative_path().join(name);
        let source_ambient = workspace
            .canonical_path
            .join(input.parent.relative_path())
            .join(&staging_name);
        let target_ambient = workspace.canonical_path.join(&target_relative);
        if let Err(error) = workspace.revalidate_authority() {
            let _cleanup_result = parent.remove_file(&staging_name);
            return Err(map_workspace_error(error));
        }
        if let Err(error) = rename_document_noreplace(
            &parent,
            &staging_name,
            &parent,
            name,
            &source_ambient,
            &target_ambient,
        ) {
            let _cleanup_result = parent.remove_file(&staging_name);
            return Err(map_noreplace_error(error));
        }
        let _sync_result = sync_directory(&parent);
        self.mutation_result(
            workspace,
            &target_relative,
            revision_for_bytes(input.contents.as_bytes()),
            options,
        )
    }

    pub(crate) fn update(
        &self,
        scope: &DocumentScope,
        input: UpdateDocument<'_>,
        options: MutationOptions,
    ) -> Result<DocumentMutation, DocumentServiceError> {
        self.update_inner(scope, input, options, || {})
    }

    #[cfg(test)]
    pub(crate) fn update_with_test_hook(
        &self,
        scope: &DocumentScope,
        input: UpdateDocument<'_>,
        options: MutationOptions,
        hook: impl FnOnce(),
    ) -> Result<DocumentMutation, DocumentServiceError> {
        self.update_inner(scope, input, options, hook)
    }

    fn update_inner(
        &self,
        scope: &DocumentScope,
        input: UpdateDocument<'_>,
        options: MutationOptions,
        hook: impl FnOnce(),
    ) -> Result<DocumentMutation, DocumentServiceError> {
        let workspace = mutation_workspace(scope, input.document.workspace_id())?;
        validate_mutation_size(input.contents.as_bytes(), self.search_document_limit_bytes)?;
        let current = read_document_bytes(input.document, self.search_document_limit_bytes)?;
        validate_expected_revision(&current, input.expected_revision)?;
        let parent = input.document.open_parent_dir().map_err(map_handle_error)?;
        let staging_name = stage_document_contents(&parent, input.contents.as_bytes())?;
        hook();

        let latest = match read_document_bytes(input.document, self.search_document_limit_bytes) {
            Ok(bytes) => bytes,
            Err(error) => {
                let _cleanup_result = parent.remove_file(&staging_name);
                return Err(error);
            }
        };
        if let Err(error) = validate_expected_revision(&latest, input.expected_revision) {
            let _cleanup_result = parent.remove_file(&staging_name);
            return Err(error);
        }
        let latest_contents = match String::from_utf8(latest) {
            Ok(contents) => contents,
            Err(_) => {
                let _cleanup_result = parent.remove_file(&staging_name);
                return Err(DocumentServiceError::invalid_encoding());
            }
        };
        let relative_path = input.document.relative_path();
        let ambient_path = workspace.canonical_path.join(relative_path);
        if let Some(history_root) = &self.history_root {
            if snapshot_markdown_file_history_contents(
                history_root,
                &ambient_path,
                &latest_contents,
                input.contents,
            )
            .is_err()
            {
                let _cleanup_result = parent.remove_file(&staging_name);
                return Err(DocumentServiceError::history_failed());
            }
        }
        let final_bytes =
            match read_document_bytes(input.document, self.search_document_limit_bytes) {
                Ok(bytes) => bytes,
                Err(error) => {
                    let _cleanup_result = parent.remove_file(&staging_name);
                    return Err(error);
                }
            };
        if let Err(error) = validate_expected_revision(&final_bytes, input.expected_revision) {
            let _cleanup_result = parent.remove_file(&staging_name);
            return Err(error);
        }
        if let Err(error) = workspace.revalidate_authority() {
            let _cleanup_result = parent.remove_file(&staging_name);
            return Err(map_workspace_error(error));
        }
        #[cfg(test)]
        if let Some(hook) = &self.before_atomic_mutation {
            hook();
        }
        let file_name = relative_path
            .file_name()
            .ok_or_else(DocumentServiceError::boundary)?;
        let staging_ambient = workspace
            .canonical_path
            .join(relative_path.parent().unwrap_or_else(|| Path::new("")))
            .join(&staging_name);
        if replace_document_atomic(
            &parent,
            &staging_name,
            file_name,
            &staging_ambient,
            &ambient_path,
        )
        .is_err()
        {
            let _cleanup_result = parent.remove_file(&staging_name);
            return Err(DocumentServiceError::mutation_failed());
        }
        let _sync_result = sync_directory(&parent);
        self.mutation_result(
            workspace,
            relative_path,
            revision_for_bytes(input.contents.as_bytes()),
            options,
        )
    }

    pub(crate) fn move_document(
        &self,
        scope: &DocumentScope,
        input: MoveDocument<'_>,
        options: MutationOptions,
    ) -> Result<DocumentMutation, DocumentServiceError> {
        let workspace = mutation_workspace(scope, input.document.workspace_id())?;
        if input.target_parent.workspace_id() != workspace.workspace_id {
            return Err(DocumentServiceError::scope());
        }
        let new_name = validate_document_name(input.new_name)?;
        let current = read_document_bytes(input.document, self.search_document_limit_bytes)?;
        let revision = validate_expected_revision(&current, input.expected_revision)?;
        let source_relative = input.document.relative_path();
        let target_relative = input.target_parent.relative_path().join(new_name);
        if source_relative == target_relative {
            return self.mutation_result(workspace, source_relative, revision, options);
        }
        let source_parent = input.document.open_parent_dir().map_err(map_handle_error)?;
        let target_parent = input.target_parent.open_dir().map_err(map_handle_error)?;
        ensure_destination_absent(&target_parent, new_name)?;
        let latest = read_document_bytes(input.document, self.search_document_limit_bytes)?;
        let revision = validate_expected_revision(&latest, input.expected_revision)?;
        workspace
            .revalidate_authority()
            .map_err(map_workspace_error)?;
        let source_name = source_relative
            .file_name()
            .ok_or_else(DocumentServiceError::boundary)?;
        let source_ambient = workspace.canonical_path.join(source_relative);
        let target_ambient = workspace.canonical_path.join(&target_relative);
        rename_document_noreplace(
            &source_parent,
            source_name,
            &target_parent,
            new_name,
            &source_ambient,
            &target_ambient,
        )
        .map_err(map_noreplace_error)?;
        let _source_sync_result = sync_directory(&source_parent);
        let _target_sync_result = sync_directory(&target_parent);
        self.mutation_result(workspace, &target_relative, revision, options)
    }

    pub(crate) fn delete(
        &self,
        scope: &DocumentScope,
        input: DeleteDocument<'_>,
        options: MutationOptions,
    ) -> Result<DocumentMutation, DocumentServiceError> {
        let workspace = mutation_workspace(scope, input.document.workspace_id())?;
        let current = read_document_bytes(input.document, self.search_document_limit_bytes)?;
        let revision = validate_expected_revision(&current, input.expected_revision)?;
        let relative_path = input.document.relative_path();
        let relative_text = slash_path(relative_path)?;
        let document_id = self
            .signer
            .issue_document(
                workspace.workspace_id,
                workspace.workspace_generation,
                &relative_text,
            )
            .map_err(map_handle_error)?;
        let parent = input.document.open_parent_dir().map_err(map_handle_error)?;
        let file_name = relative_path
            .file_name()
            .ok_or_else(DocumentServiceError::boundary)?;
        workspace
            .revalidate_authority()
            .map_err(map_workspace_error)?;
        match input.deletion {
            DeletionPolicy::Recoverable => {
                let recycle_root = self
                    .recycle_root
                    .as_deref()
                    .ok_or_else(DocumentServiceError::recycle_unavailable)?;
                copy_to_recycle_bin(
                    recycle_root,
                    workspace.workspace_id,
                    &relative_text,
                    &revision,
                    &current,
                )?;
                let latest = read_document_bytes(input.document, self.search_document_limit_bytes)?;
                validate_expected_revision(&latest, input.expected_revision)?;
                parent
                    .remove_file(file_name)
                    .map_err(|_| DocumentServiceError::mutation_failed())?;
            }
            DeletionPolicy::Permanent => {
                let latest = read_document_bytes(input.document, self.search_document_limit_bytes)?;
                validate_expected_revision(&latest, input.expected_revision)?;
                parent
                    .remove_file(file_name)
                    .map_err(|_| DocumentServiceError::mutation_failed())?;
            }
        }
        let _sync_result = sync_directory(&parent);
        Ok(DocumentMutation {
            document_id,
            relative_path: relative_text,
            revision,
            sync_request: sync_request(options),
        })
    }

    fn mutation_result(
        &self,
        workspace: &ResolvedWorkspace,
        relative_path: &Path,
        revision: DocumentRevision,
        options: MutationOptions,
    ) -> Result<DocumentMutation, DocumentServiceError> {
        let relative_path = slash_path(relative_path)?;
        let document_id = self
            .signer
            .issue_document(
                workspace.workspace_id,
                workspace.workspace_generation,
                &relative_path,
            )
            .map_err(map_handle_error)?;
        Ok(DocumentMutation {
            document_id,
            relative_path,
            revision,
            sync_request: sync_request(options),
        })
    }

    pub(crate) fn list(
        &self,
        scope: &DocumentScope,
        parent: Option<&VerifiedFolderHandle>,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<DocumentPage, DocumentServiceError> {
        let workspace = scope.authorized_workspace()?;
        workspace
            .revalidate_authority()
            .map_err(map_workspace_error)?;
        let (directory, parent_path) = match parent {
            Some(parent) if parent.workspace_id() == workspace.workspace_id => (
                parent.open_dir().map_err(map_handle_error)?,
                parent.relative_path().to_path_buf(),
            ),
            Some(_) => return Err(DocumentServiceError::scope()),
            None => (
                workspace
                    .root
                    .try_clone()
                    .map_err(|_| DocumentServiceError::unavailable())?,
                PathBuf::new(),
            ),
        };
        let ignore_rules = scope.ignore_rules()?;
        let mut entries =
            self.list_directory(workspace, &directory, &parent_path, &ignore_rules)?;
        entries.sort_by(|left, right| {
            left.relative_path
                .to_lowercase()
                .cmp(&right.relative_path.to_lowercase())
                .then(left.relative_path.cmp(&right.relative_path))
        });
        let collection_digest = collection_digest(&entries)?;
        let scope_digest = format!(
            "list:{}:{}",
            workspace.workspace_id,
            slash_path(&parent_path)?
        );
        let offset = self.cursor_offset(cursor, "list", &scope_digest, &collection_digest)?;
        let page_limit = limit.clamp(1, DOCUMENT_PAGE_LIMIT);
        let page_entries = entries
            .iter()
            .skip(offset)
            .take(page_limit)
            .cloned()
            .collect::<Vec<_>>();
        let next_offset = offset.saturating_add(page_entries.len());
        let next_cursor = if next_offset < entries.len() {
            Some(self.issue_cursor("list", &scope_digest, &collection_digest, next_offset)?)
        } else {
            None
        };
        Ok(DocumentPage {
            entries: page_entries,
            next_cursor,
        })
    }

    pub(crate) fn read(
        &self,
        scope: &DocumentScope,
        document: &VerifiedDocumentHandle,
        max_bytes: u64,
    ) -> Result<DocumentSnapshot, DocumentServiceError> {
        let workspace = scope.authorized_workspace()?;
        if document.workspace_id() != workspace.workspace_id {
            return Err(DocumentServiceError::scope());
        }
        let mut file = document.open_file().map_err(map_handle_error)?;
        let metadata = file
            .metadata()
            .map_err(|_| DocumentServiceError::unavailable())?;
        if metadata.len() > max_bytes {
            return Err(DocumentServiceError::too_large());
        }
        let read_limit = max_bytes.saturating_add(1);
        let mut bytes = Vec::new();
        Read::by_ref(&mut file)
            .take(read_limit)
            .read_to_end(&mut bytes)
            .map_err(|_| DocumentServiceError::unavailable())?;
        if bytes.len() as u64 > max_bytes {
            return Err(DocumentServiceError::too_large());
        }
        let revision = revision_for_bytes(&bytes);
        let contents =
            String::from_utf8(bytes).map_err(|_| DocumentServiceError::invalid_encoding())?;
        let relative_path = slash_path(document.relative_path())?;
        let document_id = self
            .signer
            .issue_document(
                workspace.workspace_id,
                workspace.workspace_generation,
                &relative_path,
            )
            .map_err(map_handle_error)?;
        Ok(DocumentSnapshot {
            document_id,
            relative_path,
            size_bytes: contents.len() as u64,
            contents,
            revision,
        })
    }

    pub(crate) fn search(
        &self,
        scope: &DocumentScope,
        query: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<DocumentSearchPage, DocumentServiceError> {
        let query = query.trim();
        if query.is_empty() || query.len() > 1024 {
            return Err(DocumentServiceError::invalid_query());
        }
        let workspace = scope.authorized_workspace()?;
        workspace
            .revalidate_authority()
            .map_err(map_workspace_error)?;
        let ignore_rules = scope.ignore_rules()?;
        let root = workspace
            .root
            .try_clone()
            .map_err(|_| DocumentServiceError::unavailable())?;
        let mut files = Vec::new();
        collect_markdown_files(workspace, &root, Path::new(""), &ignore_rules, &mut files)?;
        files.sort();

        let searched_document_count = files.len();
        let mut unreadable_document_count = 0;
        let mut results = Vec::new();
        let mut truncated = false;
        for relative_path in files {
            let (contents, unreadable) = match read_search_document(
                &workspace.root,
                &relative_path,
                self.search_document_limit_bytes,
            ) {
                Ok(contents) => (contents, false),
                Err(_) => (String::new(), true),
            };
            if unreadable {
                unreadable_document_count += 1;
                continue;
            }
            let ranges = markdown_search_ranges(&contents, None, query, false, None);
            let relative_text = slash_path(&relative_path)?;
            let document_id = self
                .signer
                .issue_document(
                    workspace.workspace_id,
                    workspace.workspace_generation,
                    &relative_text,
                )
                .map_err(map_handle_error)?;
            for range in ranges {
                if results.len() >= MAX_SEARCH_RESULTS {
                    truncated = true;
                    break;
                }
                let (line_number, column_number, line_text) =
                    markdown_search_line(&contents, &range);
                let match_length = contents[range.from..range.to].chars().count();
                results.push(DocumentSearchHit {
                    document_id: document_id.clone(),
                    relative_path: relative_text.clone(),
                    line_number,
                    column_number,
                    snippet: markdown_search_snippet(&line_text, column_number, match_length),
                    matched_from: range.from,
                    matched_to: range.to,
                });
            }
            if truncated {
                break;
            }
        }
        let collection_digest = collection_digest(&results)?;
        let scope_digest = format!("search:{}:{}", workspace.workspace_id, query);
        let offset = self.cursor_offset(cursor, "search", &scope_digest, &collection_digest)?;
        let page_limit = limit.clamp(1, DOCUMENT_PAGE_LIMIT);
        let page_results = results
            .iter()
            .skip(offset)
            .take(page_limit)
            .cloned()
            .collect::<Vec<_>>();
        let next_offset = offset.saturating_add(page_results.len());
        let next_cursor = if next_offset < results.len() {
            Some(self.issue_cursor("search", &scope_digest, &collection_digest, next_offset)?)
        } else {
            None
        };
        Ok(DocumentSearchPage {
            results: page_results,
            next_cursor,
            searched_document_count,
            unreadable_document_count,
            truncated,
        })
    }

    fn list_directory(
        &self,
        workspace: &ResolvedWorkspace,
        directory: &Dir,
        parent_path: &Path,
        ignore_rules: &MarkdownIgnoreRules,
    ) -> Result<Vec<DocumentEntry>, DocumentServiceError> {
        let mut entries = Vec::new();
        for entry in directory
            .entries()
            .map_err(|_| DocumentServiceError::unavailable())?
        {
            let entry = entry.map_err(|_| DocumentServiceError::unavailable())?;
            let name = entry.file_name();
            let Some(name_text) = name.to_str() else {
                continue;
            };
            let relative_path = parent_path.join(name_text);
            let ambient_path = workspace.canonical_path.join(&relative_path);
            let file_type = entry
                .file_type()
                .map_err(|_| DocumentServiceError::unavailable())?;
            if file_type.is_symlink() {
                continue;
            }
            let relative_text = slash_path(&relative_path)?;
            if file_type.is_dir() {
                if ignore_rules.ignores(&ambient_path, true) {
                    continue;
                }
                entries.push(DocumentEntry {
                    id: self
                        .signer
                        .issue_folder(
                            workspace.workspace_id,
                            workspace.workspace_generation,
                            &relative_text,
                        )
                        .map_err(map_handle_error)?,
                    name: name_text.to_string(),
                    relative_path: relative_text,
                    kind: DocumentEntryKind::Folder,
                    size_bytes: None,
                });
                continue;
            }
            if !file_type.is_file()
                || ignore_rules.ignores(&ambient_path, false)
                || !is_markdown_tree_file(&relative_path)
            {
                continue;
            }
            let metadata = directory
                .symlink_metadata(&name)
                .map_err(|_| DocumentServiceError::unavailable())?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                continue;
            }
            entries.push(DocumentEntry {
                id: self
                    .signer
                    .issue_document(
                        workspace.workspace_id,
                        workspace.workspace_generation,
                        &relative_text,
                    )
                    .map_err(map_handle_error)?,
                name: name_text.to_string(),
                relative_path: relative_text,
                kind: DocumentEntryKind::Document,
                size_bytes: Some(metadata.len()),
            });
        }
        Ok(entries)
    }

    fn cursor_offset(
        &self,
        cursor: Option<&str>,
        kind: &str,
        scope_digest: &str,
        collection_digest: &str,
    ) -> Result<usize, DocumentServiceError> {
        let Some(cursor) = cursor else {
            return Ok(0);
        };
        let payload = self.decode_cursor(cursor)?;
        if payload.version != CURSOR_VERSION
            || payload.kind != kind
            || payload.scope_digest != digest_text(scope_digest)
            || payload.collection_digest != collection_digest
        {
            return Err(DocumentServiceError::invalid_cursor());
        }
        Ok(payload.offset)
    }

    fn issue_cursor(
        &self,
        kind: &str,
        scope_digest: &str,
        collection_digest: &str,
        offset: usize,
    ) -> Result<String, DocumentServiceError> {
        let payload = CursorPayload {
            version: CURSOR_VERSION,
            kind: kind.to_string(),
            scope_digest: digest_text(scope_digest),
            collection_digest: collection_digest.to_string(),
            offset,
        };
        let bytes =
            serde_json::to_vec(&payload).map_err(|_| DocumentServiceError::invalid_cursor())?;
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.cursor_key)
            .map_err(|_| DocumentServiceError::invalid_cursor())?;
        mac.update(&bytes);
        let signature = mac.finalize().into_bytes();
        Ok(format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(bytes),
            URL_SAFE_NO_PAD.encode(signature)
        ))
    }

    fn decode_cursor(&self, cursor: &str) -> Result<CursorPayload, DocumentServiceError> {
        let mut parts = cursor.split('.');
        let payload = parts
            .next()
            .ok_or_else(DocumentServiceError::invalid_cursor)?;
        let signature = parts
            .next()
            .ok_or_else(DocumentServiceError::invalid_cursor)?;
        if parts.next().is_some() {
            return Err(DocumentServiceError::invalid_cursor());
        }
        let payload = URL_SAFE_NO_PAD
            .decode(payload)
            .map_err(|_| DocumentServiceError::invalid_cursor())?;
        let signature = URL_SAFE_NO_PAD
            .decode(signature)
            .map_err(|_| DocumentServiceError::invalid_cursor())?;
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.cursor_key)
            .map_err(|_| DocumentServiceError::invalid_cursor())?;
        mac.update(&payload);
        mac.verify_slice(&signature)
            .map_err(|_| DocumentServiceError::invalid_cursor())?;
        serde_json::from_slice(&payload).map_err(|_| DocumentServiceError::invalid_cursor())
    }
}

fn mutation_workspace(
    scope: &DocumentScope,
    workspace_id: Uuid,
) -> Result<&ResolvedWorkspace, DocumentServiceError> {
    let workspace = scope.authorized_workspace()?;
    if workspace.workspace_id != workspace_id {
        return Err(DocumentServiceError::scope());
    }
    workspace
        .revalidate_authority()
        .map_err(map_workspace_error)?;
    Ok(workspace)
}

fn validate_document_name(name: &str) -> Result<&str, DocumentServiceError> {
    DocumentName::parse_file(name.to_string()).map_err(|_| DocumentServiceError::invalid_name())?;
    Ok(name)
}

fn validate_mutation_size(bytes: &[u8], max_bytes: u64) -> Result<(), DocumentServiceError> {
    if bytes.len() as u64 > max_bytes {
        return Err(DocumentServiceError::too_large());
    }
    Ok(())
}

fn read_document_bytes(
    document: &VerifiedDocumentHandle,
    max_bytes: u64,
) -> Result<Vec<u8>, DocumentServiceError> {
    let mut file = document.open_file().map_err(map_handle_error)?;
    let metadata = file
        .metadata()
        .map_err(|_| DocumentServiceError::unavailable())?;
    if metadata.len() > max_bytes {
        return Err(DocumentServiceError::too_large());
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| DocumentServiceError::unavailable())?;
    validate_mutation_size(&bytes, max_bytes)?;
    Ok(bytes)
}

fn validate_expected_revision(
    bytes: &[u8],
    expected_revision: &str,
) -> Result<DocumentRevision, DocumentServiceError> {
    let revision = revision_for_bytes(bytes);
    if revision.0 != expected_revision {
        return Err(DocumentServiceError::revision_conflict());
    }
    Ok(revision)
}

fn ensure_destination_absent(
    directory: &Dir,
    name: impl AsRef<Path>,
) -> Result<(), DocumentServiceError> {
    match directory.symlink_metadata(name) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(DocumentServiceError::boundary()),
        Ok(_) => Err(DocumentServiceError::already_exists()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(DocumentServiceError::mutation_failed()),
    }
}

fn stage_document_contents(directory: &Dir, bytes: &[u8]) -> Result<String, DocumentServiceError> {
    for _ in 0..8 {
        let name = format!("{UPDATE_TEMP_PREFIX}{}.tmp", Uuid::new_v4());
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = match directory.open_with(&name, &options) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(DocumentServiceError::mutation_failed()),
        };
        if file
            .write_all(bytes)
            .and_then(|()| file.sync_all())
            .is_err()
        {
            drop(file);
            let _cleanup_result = directory.remove_file(&name);
            return Err(DocumentServiceError::mutation_failed());
        }
        drop(file);
        return Ok(name);
    }
    Err(DocumentServiceError::mutation_failed())
}

fn map_noreplace_error(error: io::Error) -> DocumentServiceError {
    if error.kind() == io::ErrorKind::AlreadyExists {
        DocumentServiceError::already_exists()
    } else {
        DocumentServiceError::mutation_failed()
    }
}

#[cfg(unix)]
fn rename_document_noreplace(
    source: &Dir,
    source_name: impl AsRef<Path>,
    destination: &Dir,
    destination_name: impl AsRef<Path>,
    _source_ambient: &Path,
    _destination_ambient: &Path,
) -> io::Result<()> {
    rustix::fs::renameat_with(
        source,
        source_name.as_ref(),
        destination,
        destination_name.as_ref(),
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(Into::into)
}

#[cfg(windows)]
fn rename_document_noreplace(
    source: &Dir,
    source_name: impl AsRef<Path>,
    destination: &Dir,
    destination_name: impl AsRef<Path>,
    _source_ambient: &Path,
    _destination_ambient: &Path,
) -> io::Result<()> {
    rename_document_capability_noreplace(source, source_name, destination, destination_name)
}

fn rename_document_capability_noreplace(
    source: &Dir,
    source_name: impl AsRef<Path>,
    destination: &Dir,
    destination_name: impl AsRef<Path>,
) -> io::Result<()> {
    crate::atomic_noreplace::rename_noreplace(
        source,
        source_name.as_ref(),
        destination,
        destination_name.as_ref(),
    )
}

#[cfg(not(any(unix, windows)))]
fn rename_document_noreplace(
    _source: &Dir,
    _source_name: impl AsRef<Path>,
    _destination: &Dir,
    _destination_name: impl AsRef<Path>,
    _source_ambient: &Path,
    _destination_ambient: &Path,
) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "atomic no-overwrite rename is unsupported",
    ))
}

#[cfg(unix)]
fn verify_kernel_named_pinned_install_source(
    directory: &Dir,
    name: &str,
    expected: PinnedInstallSource<'_>,
) -> io::Result<()> {
    verify_kernel_named_pinned_install_identity(directory, name, expected)?;
    match expected {
        PinnedInstallSource::File(file) => {
            kernel_revision_for_retained_file(file)?;
        }
        PinnedInstallSource::Directory(directory) => {
            qingyu_kernel::documents::service::directory_revision_for_capability(directory)
                .map_err(|_| kernel_atomic_install_error())?;
        }
    }
    verify_kernel_named_pinned_install_identity(directory, name, expected)
}

#[cfg(unix)]
fn verify_kernel_named_pinned_install_identity(
    directory: &Dir,
    name: &str,
    expected: PinnedInstallSource<'_>,
) -> io::Result<()> {
    let named = directory.symlink_metadata(name)?;
    let retained = match expected {
        PinnedInstallSource::File(file) => file.metadata()?,
        PinnedInstallSource::Directory(directory) => directory.dir_metadata()?,
    };
    let trusted = match expected {
        PinnedInstallSource::File(_) => {
            kernel_trusted_file_metadata(&named) && kernel_trusted_file_metadata(&retained)
        }
        PinnedInstallSource::Directory(_) => {
            named.is_dir()
                && retained.is_dir()
                && !named.file_type().is_symlink()
                && !retained.file_type().is_symlink()
        }
    };
    if !trusted
        || MetadataExt::dev(&named) != MetadataExt::dev(&retained)
        || MetadataExt::ino(&named) != MetadataExt::ino(&retained)
    {
        return Err(kernel_atomic_install_error());
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn verify_kernel_named_pinned_install_source(
    _directory: &Dir,
    _name: &str,
    _expected: PinnedInstallSource<'_>,
) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "kernel staged-source verification is unsupported",
    ))
}

#[cfg(unix)]
fn replace_kernel_document_compare_exchange(
    directory: &Dir,
    request: &AtomicInstallRequest<'_>,
    staging_ambient: &Path,
    target_ambient: &Path,
) -> io::Result<()> {
    replace_kernel_document_compare_exchange_with_hook(
        directory,
        request,
        staging_ambient,
        target_ambient,
        || {},
    )
}

#[cfg(unix)]
fn replace_kernel_document_compare_exchange_with_hook(
    directory: &Dir,
    request: &AtomicInstallRequest<'_>,
    _staging_ambient: &Path,
    _target_ambient: &Path,
    before_retired_verification: impl FnOnce(),
) -> io::Result<()> {
    let (Some(expected_target), Some(expected_revision)) =
        (request.expected_target, request.expected_revision)
    else {
        return Err(kernel_atomic_install_error());
    };
    verify_kernel_named_retained_identity(directory, request.target_name, expected_target)?;
    if kernel_revision_for_retained_file(expected_target)? != *expected_revision {
        return Err(kernel_atomic_install_error());
    }
    verify_kernel_named_retained_identity(directory, request.target_name, expected_target)?;
    rustix::fs::renameat_with(
        directory,
        request.stage_name,
        directory,
        request.target_name,
        rustix::fs::RenameFlags::EXCHANGE,
    )?;
    before_retired_verification();
    match verify_kernel_retired_target(
        directory,
        request.stage_name,
        expected_target,
        expected_revision,
    ) {
        Ok(()) => {}
        Err(KernelRetiredTargetVerificationError::RevisionMismatch) => {
            verify_kernel_named_retained_identity(directory, request.stage_name, expected_target)?;
            rustix::fs::renameat_with(
                directory,
                request.stage_name,
                directory,
                request.target_name,
                rustix::fs::RenameFlags::EXCHANGE,
            )?;
            return Err(kernel_atomic_install_error());
        }
        Err(
            KernelRetiredTargetVerificationError::NamedIdentityLost
            | KernelRetiredTargetVerificationError::RetainedReadUncertain,
        ) => return Err(kernel_atomic_install_error()),
    }
    verify_kernel_named_retained_identity(directory, request.stage_name, expected_target)?;
    directory.remove_file(request.stage_name)
}

#[cfg(not(any(unix, windows)))]
fn replace_kernel_document_compare_exchange(
    _directory: &Dir,
    _request: &AtomicInstallRequest<'_>,
    _staging_ambient: &Path,
    _target_ambient: &Path,
) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "kernel compare-and-exchange replacement is unsupported",
    ))
}

#[cfg(unix)]
fn verify_kernel_retired_target(
    directory: &Dir,
    name: &str,
    expected_target: &cap_std::fs::File,
    expected_revision: &qingyu_kernel::contract::Revision,
) -> Result<(), KernelRetiredTargetVerificationError> {
    verify_kernel_named_retained_identity(directory, name, expected_target)
        .map_err(|_| KernelRetiredTargetVerificationError::NamedIdentityLost)?;
    let actual = kernel_revision_for_retained_file(expected_target)
        .map_err(|_| KernelRetiredTargetVerificationError::RetainedReadUncertain)?;
    if actual != *expected_revision {
        verify_kernel_named_retained_identity(directory, name, expected_target)
            .map_err(|_| KernelRetiredTargetVerificationError::NamedIdentityLost)?;
        return Err(KernelRetiredTargetVerificationError::RevisionMismatch);
    }
    verify_kernel_named_retained_identity(directory, name, expected_target)
        .map_err(|_| KernelRetiredTargetVerificationError::NamedIdentityLost)
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KernelRetiredTargetVerificationError {
    NamedIdentityLost,
    RevisionMismatch,
    RetainedReadUncertain,
}

#[cfg(unix)]
fn verify_kernel_named_retained_identity(
    directory: &Dir,
    name: &str,
    expected_target: &cap_std::fs::File,
) -> io::Result<()> {
    let named = directory.symlink_metadata(name)?;
    let retained = expected_target.metadata()?;
    if !kernel_trusted_file_metadata(&named)
        || !kernel_trusted_file_metadata(&retained)
        || MetadataExt::dev(&named) != MetadataExt::dev(&retained)
        || MetadataExt::ino(&named) != MetadataExt::ino(&retained)
    {
        return Err(kernel_atomic_install_error());
    }
    Ok(())
}

#[cfg(unix)]
fn kernel_revision_for_retained_file(
    expected_target: &cap_std::fs::File,
) -> io::Result<qingyu_kernel::contract::Revision> {
    let mut file = expected_target.try_clone()?;
    let before = file.metadata()?;
    if !kernel_trusted_file_metadata(&before) || before.len() > MAX_KERNEL_DOCUMENT_BYTES as u64 {
        return Err(kernel_atomic_install_error());
    }
    file.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::with_capacity(before.len() as usize);
    (&mut file)
        .take(MAX_KERNEL_DOCUMENT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    let after = file.metadata()?;
    if bytes.len() > MAX_KERNEL_DOCUMENT_BYTES
        || !kernel_trusted_file_metadata(&after)
        || MetadataExt::dev(&before) != MetadataExt::dev(&after)
        || MetadataExt::ino(&before) != MetadataExt::ino(&after)
        || before.len() != after.len()
        || after.len() != bytes.len() as u64
        || before.modified().ok() != after.modified().ok()
    {
        return Err(kernel_atomic_install_error());
    }
    qingyu_kernel::contract::Revision::parse(format!("{:x}", Sha256::digest(bytes)))
        .map_err(|_| kernel_atomic_install_error())
}

fn kernel_trusted_file_metadata(metadata: &cap_std::fs::Metadata) -> bool {
    metadata.is_file() && !metadata.file_type().is_symlink() && kernel_link_count(metadata) == 1
}

#[cfg(unix)]
fn kernel_link_count(metadata: &cap_std::fs::Metadata) -> u64 {
    MetadataExt::nlink(metadata)
}

#[cfg(windows)]
fn kernel_link_count(metadata: &cap_std::fs::Metadata) -> u64 {
    MetadataExt::nlink(metadata)
}

#[cfg(not(any(unix, windows)))]
fn kernel_link_count(_metadata: &cap_std::fs::Metadata) -> u64 {
    1
}

#[cfg(not(windows))]
fn kernel_atomic_install_error() -> io::Error {
    io::Error::other("kernel atomic install target changed")
}

#[cfg(unix)]
fn replace_document_atomic(
    directory: &Dir,
    staging_name: &str,
    target_name: &std::ffi::OsStr,
    _staging_ambient: &Path,
    _target_ambient: &Path,
) -> io::Result<()> {
    directory.rename(staging_name, directory, target_name)
}

#[cfg(windows)]
fn replace_document_atomic(
    _directory: &Dir,
    _staging_name: &str,
    _target_name: &std::ffi::OsStr,
    staging_ambient: &Path,
    target_ambient: &Path,
) -> io::Result<()> {
    use std::{os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::Storage::FileSystem::{ReplaceFileW, REPLACEFILE_WRITE_THROUGH};

    let target = target_ambient
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let staging = staging_ambient
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let replaced = unsafe {
        ReplaceFileW(
            target.as_ptr(),
            staging.as_ptr(),
            ptr::null(),
            REPLACEFILE_WRITE_THROUGH,
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    if replaced == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(any(unix, windows)))]
fn replace_document_atomic(
    directory: &Dir,
    staging_name: &str,
    target_name: &std::ffi::OsStr,
    _staging_ambient: &Path,
    _target_ambient: &Path,
) -> io::Result<()> {
    directory.rename(staging_name, directory, target_name)
}

#[cfg(unix)]
fn sync_directory(directory: &Dir) -> io::Result<()> {
    rustix::fs::fsync(directory).map_err(Into::into)
}

#[cfg(not(unix))]
fn sync_directory(_directory: &Dir) -> io::Result<()> {
    Ok(())
}

fn copy_to_recycle_bin(
    recycle_root: &Path,
    workspace_id: Uuid,
    relative_path: &str,
    revision: &DocumentRevision,
    bytes: &[u8],
) -> Result<(), DocumentServiceError> {
    std::fs::create_dir_all(recycle_root)
        .map_err(|_| DocumentServiceError::recycle_unavailable())?;
    let entry = recycle_root.join(Uuid::new_v4().to_string());
    std::fs::create_dir(&entry).map_err(|_| DocumentServiceError::recycle_unavailable())?;
    let result = (|| {
        let document_path = entry.join("document.md");
        let mut document = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&document_path)
            .map_err(|_| DocumentServiceError::recycle_unavailable())?;
        document
            .write_all(bytes)
            .and_then(|()| document.sync_all())
            .map_err(|_| DocumentServiceError::recycle_unavailable())?;
        let metadata = RecycleMetadata {
            workspace_id,
            relative_path,
            deleted_at: current_time_millis(),
            revision: &revision.0,
        };
        let metadata_bytes = serde_json::to_vec_pretty(&metadata)
            .map_err(|_| DocumentServiceError::recycle_unavailable())?;
        let mut metadata_file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(entry.join("metadata.json"))
            .map_err(|_| DocumentServiceError::recycle_unavailable())?;
        metadata_file
            .write_all(&metadata_bytes)
            .and_then(|()| metadata_file.sync_all())
            .map_err(|_| DocumentServiceError::recycle_unavailable())?;
        if std::fs::read(document_path).map_err(|_| DocumentServiceError::recycle_unavailable())?
            != bytes
        {
            return Err(DocumentServiceError::recycle_unavailable());
        }
        let _entry_sync_result = std::fs::File::open(&entry).and_then(|file| file.sync_all());
        Ok(())
    })();
    if result.is_err() {
        let _cleanup_result = std::fs::remove_dir_all(&entry);
    }
    result
}

fn current_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn sync_request(options: MutationOptions) -> SyncRequest {
    let requested = match options.sync_after_write {
        SyncAfterWritePolicy::FollowWorkspace => options.workspace_sync_enabled,
        SyncAfterWritePolicy::Always => true,
        SyncAfterWritePolicy::Never => false,
    };
    if requested {
        SyncRequest::Requested
    } else {
        SyncRequest::NotRequested
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct CursorPayload {
    version: u8,
    kind: String,
    scope_digest: String,
    collection_digest: String,
    offset: usize,
}

fn collect_markdown_files(
    workspace: &ResolvedWorkspace,
    directory: &Dir,
    parent_path: &Path,
    ignore_rules: &MarkdownIgnoreRules,
    files: &mut Vec<PathBuf>,
) -> Result<(), DocumentServiceError> {
    let mut entries = directory
        .entries()
        .map_err(|_| DocumentServiceError::unavailable())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| DocumentServiceError::unavailable())?;
    entries.sort_by(|left, right| {
        left.file_name()
            .to_string_lossy()
            .to_lowercase()
            .cmp(&right.file_name().to_string_lossy().to_lowercase())
    });
    for entry in entries {
        let name = entry.file_name();
        let Some(name_text) = name.to_str() else {
            continue;
        };
        let relative_path = parent_path.join(name_text);
        let ambient_path = workspace.canonical_path.join(&relative_path);
        let file_type = entry
            .file_type()
            .map_err(|_| DocumentServiceError::unavailable())?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            if ignore_rules.ignores(&ambient_path, true) {
                continue;
            }
            let child = directory
                .open_dir_nofollow(&name)
                .map_err(|_| DocumentServiceError::boundary())?;
            collect_markdown_files(workspace, &child, &relative_path, ignore_rules, files)?;
            continue;
        }
        if file_type.is_file()
            && !ignore_rules.ignores(&ambient_path, false)
            && is_markdown_tree_file(&relative_path)
        {
            let metadata = directory
                .symlink_metadata(&name)
                .map_err(|_| DocumentServiceError::unavailable())?;
            if !metadata.file_type().is_symlink() && metadata.is_file() {
                files.push(relative_path);
            }
        }
    }
    Ok(())
}

fn read_search_document(
    root: &Dir,
    relative_path: &Path,
    max_bytes: u64,
) -> Result<String, DocumentServiceError> {
    let parent = relative_path.parent().unwrap_or_else(|| Path::new(""));
    let file_name = relative_path
        .file_name()
        .ok_or_else(DocumentServiceError::unavailable)?;
    let mut directory = root
        .try_clone()
        .map_err(|_| DocumentServiceError::unavailable())?;
    for component in parent.components() {
        let Component::Normal(segment) = component else {
            return Err(DocumentServiceError::boundary());
        };
        directory = directory
            .open_dir_nofollow(segment)
            .map_err(|_| DocumentServiceError::boundary())?;
    }
    let metadata = directory
        .symlink_metadata(file_name)
        .map_err(|_| DocumentServiceError::unavailable())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > max_bytes {
        return Err(DocumentServiceError::too_large());
    }
    use cap_fs_ext::{FollowSymlinks, OpenOptionsExt, OpenOptionsFollowExt};
    let mut options = cap_std::fs::OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    #[cfg(unix)]
    options.custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32);
    let mut file = directory
        .open_with(file_name, &options)
        .map_err(|_| DocumentServiceError::unavailable())?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| DocumentServiceError::unavailable())?;
    if bytes.len() as u64 > max_bytes {
        return Err(DocumentServiceError::too_large());
    }
    String::from_utf8(bytes).map_err(|_| DocumentServiceError::invalid_encoding())
}

fn revision_for_bytes(bytes: &[u8]) -> DocumentRevision {
    let mut digest = Sha256::new();
    digest.update(bytes);
    digest.update((bytes.len() as u64).to_le_bytes());
    DocumentRevision(format!("{:x}", digest.finalize()))
}

fn slash_path(path: &Path) -> Result<String, DocumentServiceError> {
    let mut parts = Vec::new();
    for component in path.components() {
        let Component::Normal(segment) = component else {
            return Err(DocumentServiceError::boundary());
        };
        parts.push(
            segment
                .to_str()
                .ok_or_else(DocumentServiceError::boundary)?,
        );
    }
    Ok(parts.join("/"))
}

fn map_handle_error(error: crate::mcp::handles::HandleError) -> DocumentServiceError {
    match error.code {
        "workspace_not_authorized" => DocumentServiceError::scope(),
        "workspace_unavailable" => DocumentServiceError::workspace_unavailable(),
        "mcp-workspace-unavailable" => DocumentServiceError::primary_workspace_unavailable(),
        "mcp-handle-stale" => DocumentServiceError::stale_handle(),
        "document_not_found" => DocumentServiceError::unavailable(),
        _ => DocumentServiceError::boundary(),
    }
}

fn map_workspace_error(error: WorkspaceError) -> DocumentServiceError {
    match error.code {
        "workspace_not_authorized" => DocumentServiceError::scope(),
        "workspace_unavailable" => DocumentServiceError::workspace_unavailable(),
        "mcp-workspace-unavailable" => DocumentServiceError::primary_workspace_unavailable(),
        "mcp-handle-stale" => DocumentServiceError::stale_handle(),
        _ => DocumentServiceError::boundary(),
    }
}

fn digest_text(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn collection_digest<T: Serialize>(values: &[T]) -> Result<String, DocumentServiceError> {
    let bytes = serde_json::to_vec(values).map_err(|_| DocumentServiceError::invalid_cursor())?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn trusted_parent(path: &Path) -> Result<(Dir, PathBuf, std::ffi::OsString), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "Document parent is unavailable".to_string())?
        .to_path_buf();
    let name = path
        .file_name()
        .ok_or_else(|| "Document name is unavailable".to_string())?
        .to_os_string();
    let directory = Dir::open_ambient_dir(&parent, cap_std::ambient_authority())
        .map_err(|error| error.to_string())?;
    Ok((directory, parent, name))
}

pub(super) fn write_trusted_file_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let (directory, parent, name) = trusted_parent(path)?;
    let target_exists = match directory.symlink_metadata(&name) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err("Document target is unsafe".to_string());
        }
        Ok(_) => true,
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => return Err(error.to_string()),
    };
    let staging_name =
        stage_document_contents(&directory, bytes).map_err(|error| error.to_string())?;
    let staging_ambient = parent.join(&staging_name);
    let publish_result = if target_exists {
        replace_document_atomic(&directory, &staging_name, &name, &staging_ambient, path)
    } else {
        rename_document_noreplace(
            &directory,
            &staging_name,
            &directory,
            &name,
            &staging_ambient,
            path,
        )
    };
    if let Err(error) = publish_result {
        let _cleanup_result = directory.remove_file(&staging_name);
        return Err(error.to_string());
    }
    let _sync_result = sync_directory(&directory);
    Ok(())
}

pub(super) fn create_trusted_file_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let (directory, parent, name) = trusted_parent(path)?;
    ensure_destination_absent(&directory, &name).map_err(|error| error.to_string())?;
    let staging_name =
        stage_document_contents(&directory, bytes).map_err(|error| error.to_string())?;
    let staging_ambient = parent.join(&staging_name);
    if let Err(error) = rename_document_noreplace(
        &directory,
        &staging_name,
        &directory,
        &name,
        &staging_ambient,
        path,
    ) {
        let _cleanup_result = directory.remove_file(&staging_name);
        return Err(error.to_string());
    }
    let _sync_result = sync_directory(&directory);
    Ok(())
}

pub(super) fn move_trusted_path_noreplace(source: &Path, target: &Path) -> Result<(), String> {
    let (source_parent, _source_parent_path, source_name) = trusted_parent(source)?;
    let (target_parent, _target_parent_path, target_name) = trusted_parent(target)?;
    let source_metadata = source_parent
        .symlink_metadata(&source_name)
        .map_err(|error| error.to_string())?;
    if source_metadata.file_type().is_symlink() {
        return Err("Document source is unsafe".to_string());
    }
    ensure_destination_absent(&target_parent, &target_name).map_err(|error| error.to_string())?;
    rename_document_noreplace(
        &source_parent,
        &source_name,
        &target_parent,
        &target_name,
        source,
        target,
    )
    .map_err(|error| error.to_string())?;
    let _source_sync_result = sync_directory(&source_parent);
    let _target_sync_result = sync_directory(&target_parent);
    Ok(())
}

pub(super) fn delete_trusted_file(path: &Path) -> Result<(), String> {
    let (parent, _parent_path, name) = trusted_parent(path)?;
    let metadata = parent
        .symlink_metadata(&name)
        .map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("Document target is unsafe".to_string());
    }
    parent
        .remove_file(&name)
        .map_err(|error| error.to_string())?;
    let _sync_result = sync_directory(&parent);
    Ok(())
}

pub(super) fn read_trusted_markdown_file(path: &Path) -> Result<MarkdownFile, String> {
    let size_bytes = std::fs::metadata(path)
        .map_err(|error| error.to_string())?
        .len();
    let contents = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    Ok(MarkdownFile {
        path: path.to_string_lossy().to_string(),
        contents,
        size_bytes,
    })
}

#[cfg(test)]
mod kernel_deletion_adapter_tests {
    use std::sync::{Arc, Mutex};

    #[cfg(windows)]
    use cap_fs_ext::OpenOptionsExt as _;
    use cap_fs_ext::{DirExt as _, FollowSymlinks, OpenOptionsFollowExt as _};
    use cap_std::fs::{Dir, File, OpenOptions};
    use qingyu_kernel::{
        config::KernelConfig,
        contract::{
            CreateDocumentRequest, DeletionPolicy, DocumentContents, DocumentKind,
            FileDocumentName, ListDocumentsQuery, PageQuery, RestoreDocumentHistoryRequest,
            Revision, SearchQuery, SearchWorkspaceQuery, UpdateDocumentRequest,
            WorkspaceGeneration, WorkspaceRelativePath,
        },
        documents::{
            history::MemoryDocumentRecoveryStore, service::WorkspaceDocumentService,
            AtomicInstallMode, AtomicInstallPort, AtomicInstallRequest, DeletionPort,
            DocumentDeletionTarget, DocumentIgnorePort, PinnedInstallSource,
        },
        ignore_rules::StaticWorkspaceIgnorePort,
        paths::KernelPaths,
        ports::KernelPorts,
        runtime::{DocumentsApiService, KernelRuntime},
        services::workspace::WorkspaceService,
        workspace::{
            managed::ManagedWorkspaceCollection,
            primary::{
                PrimaryWorkspaceRepositoryBinding, PrimaryWorkspaceStore,
                PrimaryWorkspaceStoreError,
            },
        },
    };
    use serde_json::Value;
    use sha2::{Digest as _, Sha256};

    #[cfg(unix)]
    use super::replace_kernel_document_compare_exchange_with_hook;
    use super::{
        rename_document_capability_noreplace, KernelDocumentAtomicInstallAdapter,
        KernelDocumentDeletionAdapter, KernelDocumentIgnoreAdapter, KernelDocumentsTauriFacade,
    };
    use crate::markdown_files::history::KernelDocumentHistoryAdapter;

    #[derive(Default)]
    struct MemoryWorkspaceStore {
        binding: PrimaryWorkspaceRepositoryBinding,
        value: Mutex<Option<Value>>,
    }

    impl PrimaryWorkspaceStore for MemoryWorkspaceStore {
        fn repository_binding(&self) -> PrimaryWorkspaceRepositoryBinding {
            self.binding.clone()
        }

        fn load(&self) -> Result<Option<Value>, PrimaryWorkspaceStoreError> {
            Ok(self.value.lock().unwrap().clone())
        }

        fn replace(&self, value: Option<Value>) -> Result<(), PrimaryWorkspaceStoreError> {
            *self.value.lock().unwrap() = value;
            Ok(())
        }

        fn save(&self) -> Result<(), PrimaryWorkspaceStoreError> {
            Ok(())
        }
    }

    fn target(path: &str, contents: &str) -> DocumentDeletionTarget {
        DocumentDeletionTarget {
            path: WorkspaceRelativePath::parse(path).unwrap(),
            kind: DocumentKind::File,
            revision: Revision::parse(format!("{:x}", Sha256::digest(contents.as_bytes())))
                .unwrap(),
        }
    }

    fn open_pinned_stage(directory: &Dir, name: &str) -> File {
        let mut options = OpenOptions::new();
        options.read(true).write(true).follow(FollowSymlinks::No);
        #[cfg(windows)]
        options
            .access_mode(
                windows_sys::Win32::Foundation::GENERIC_READ
                    | windows_sys::Win32::Foundation::GENERIC_WRITE
                    | windows_sys::Win32::Storage::FileSystem::DELETE,
            )
            .share_mode(windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ);
        directory.open_with(name, &options).unwrap()
    }

    #[test]
    fn adapter_keeps_absolute_workspace_authority_outside_the_kernel_port() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("workspace");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("recover.md"), "recover").unwrap();
        std::fs::write(root.join("permanent.md"), "permanent").unwrap();
        let trashed = Arc::new(Mutex::new(Vec::new()));
        let seen = trashed.clone();
        let adapter = KernelDocumentDeletionAdapter::new(
            &root,
            Arc::new(move |path| {
                seen.lock().unwrap().push(path.to_path_buf());
                std::fs::remove_file(path).map_err(|error| error.to_string())
            }),
        )
        .unwrap();

        adapter
            .delete(
                &target("recover.md", "recover"),
                DeletionPolicy::Recoverable,
            )
            .unwrap();
        adapter
            .delete(
                &target("permanent.md", "permanent"),
                DeletionPolicy::Permanent,
            )
            .unwrap();

        let trashed = trashed.lock().unwrap();
        assert_eq!(trashed.len(), 1);
        assert_eq!(
            trashed[0].parent(),
            Some(root.canonicalize().unwrap().as_path())
        );
        assert!(trashed[0]
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with(".qingyu-delete-"));
        assert!(!root.join("recover.md").exists());
        assert!(!root.join("permanent.md").exists());
        assert!(WorkspaceRelativePath::parse("../outside.md").is_err());
    }

    #[test]
    fn adapter_rejects_replaced_workspace_root() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("workspace");
        let moved = fixture.path().join("moved");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("note.md"), "original").unwrap();
        let adapter = KernelDocumentDeletionAdapter::new(&root, Arc::new(|_| Ok(()))).unwrap();
        std::fs::rename(&root, &moved).unwrap();
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("note.md"), "replacement").unwrap();

        assert!(adapter
            .delete(&target("note.md", "original"), DeletionPolicy::Permanent)
            .is_err());
        assert_eq!(
            std::fs::read_to_string(root.join("note.md")).unwrap(),
            "replacement"
        );
        assert_eq!(
            std::fs::read_to_string(moved.join("note.md")).unwrap(),
            "original"
        );
    }

    #[test]
    fn adapter_rejects_replaced_target_revision() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("workspace");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("note.md"), "original").unwrap();
        let adapter = KernelDocumentDeletionAdapter::new(&root, Arc::new(|_| Ok(()))).unwrap();
        let expected = target("note.md", "original");
        std::fs::write(root.join("note.md"), "replacement").unwrap();

        assert!(adapter
            .delete(&expected, DeletionPolicy::Permanent)
            .is_err());
        assert_eq!(
            std::fs::read_to_string(root.join("note.md")).unwrap(),
            "replacement"
        );
    }

    #[test]
    fn deletion_adapter_uses_the_kernel_directory_revision_contract() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("workspace");
        std::fs::create_dir_all(root.join("folder/nested")).unwrap();
        std::fs::write(root.join("folder/nested/note.md"), "contents").unwrap();
        let retained =
            Dir::open_ambient_dir(root.join("folder"), cap_std::ambient_authority()).unwrap();
        let revision =
            qingyu_kernel::documents::service::directory_revision_for_capability(&retained)
                .unwrap();
        let adapter = KernelDocumentDeletionAdapter::new(&root, Arc::new(|_| Ok(()))).unwrap();
        adapter
            .delete(
                &DocumentDeletionTarget {
                    path: WorkspaceRelativePath::parse("folder").unwrap(),
                    kind: DocumentKind::Directory,
                    revision,
                },
                DeletionPolicy::Permanent,
            )
            .unwrap();
        assert!(!root.join("folder").exists());
    }

    #[test]
    fn adapter_quarantines_then_revalidates_a_target_replaced_during_delete() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("workspace");
        let saved = root.join("saved.md");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("note.md"), "original").unwrap();
        let hook_root = root.clone();
        let hook_saved = saved.clone();
        let adapter = KernelDocumentDeletionAdapter::new(&root, Arc::new(|_| Ok(())))
            .unwrap()
            .with_before_delete(Arc::new(move || {
                std::fs::rename(hook_root.join("note.md"), &hook_saved).unwrap();
                std::fs::write(hook_root.join("note.md"), "replacement").unwrap();
            }));

        assert!(adapter
            .delete(&target("note.md", "original"), DeletionPolicy::Permanent)
            .is_err());
        assert_eq!(
            std::fs::read_to_string(root.join("note.md")).unwrap(),
            "replacement"
        );
        assert_eq!(std::fs::read_to_string(saved).unwrap(), "original");
    }

    #[test]
    fn recoverable_delete_is_not_redirected_by_a_nested_parent_replacement() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("workspace");
        std::fs::create_dir_all(root.join("folder")).unwrap();
        std::fs::write(root.join("folder/note.md"), "original").unwrap();
        let trash_root = root.clone();
        let adapter = KernelDocumentDeletionAdapter::new(
            &root,
            Arc::new(move |path| {
                let quarantine_name = path.file_name().unwrap().to_owned();
                std::fs::rename(trash_root.join("folder"), trash_root.join("saved"))
                    .map_err(|error| error.to_string())?;
                std::fs::create_dir(trash_root.join("folder"))
                    .map_err(|error| error.to_string())?;
                std::fs::write(trash_root.join("folder").join(quarantine_name), "decoy")
                    .map_err(|error| error.to_string())?;
                std::fs::remove_file(path).map_err(|error| error.to_string())
            }),
        )
        .unwrap();

        adapter
            .delete(
                &DocumentDeletionTarget {
                    path: WorkspaceRelativePath::parse("folder/note.md").unwrap(),
                    kind: DocumentKind::File,
                    revision: target("note.md", "original").revision,
                },
                DeletionPolicy::Recoverable,
            )
            .unwrap();

        let saved_entries = std::fs::read_dir(root.join("saved"))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(
            saved_entries.is_empty(),
            "the original must not survive under the renamed parent"
        );
        let decoys = std::fs::read_dir(root.join("folder"))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(decoys.len(), 1);
        assert_eq!(std::fs::read_to_string(decoys[0].path()).unwrap(), "decoy");
    }

    #[test]
    fn recoverable_delete_never_rolls_an_unknown_quarantine_name_into_the_document() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("workspace");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("note.md"), "original").unwrap();
        let adapter = KernelDocumentDeletionAdapter::new(
            &root,
            Arc::new(|path| {
                std::fs::remove_file(path).map_err(|error| error.to_string())?;
                std::fs::write(path, "unknown entry").map_err(|error| error.to_string())?;
                Err("trash failed after replacement".to_string())
            }),
        )
        .unwrap();

        assert!(adapter
            .delete(&target("note.md", "original"), DeletionPolicy::Recoverable)
            .is_err());
        assert!(!root.join("note.md").exists());
        let quarantine = std::fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap())
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".qingyu-delete-")
            })
            .expect("the unknown entry must be retained for manual recovery");
        assert_eq!(
            std::fs::read_to_string(quarantine.path()).unwrap(),
            "unknown entry"
        );
    }

    #[test]
    fn capability_rename_moves_the_retained_entry_after_ambient_parent_replacement() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("workspace");
        std::fs::create_dir_all(root.join("folder")).unwrap();
        std::fs::write(root.join("folder/note.md"), "original").unwrap();
        let retained_root = Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap();
        let retained_parent = retained_root.open_dir_nofollow("folder").unwrap();
        std::fs::rename(root.join("folder"), root.join("saved")).unwrap();
        std::fs::create_dir(root.join("folder")).unwrap();
        std::fs::write(root.join("folder/note.md"), "replacement").unwrap();

        rename_document_capability_noreplace(
            &retained_parent,
            "note.md",
            &retained_root,
            "quarantine.tmp",
        )
        .unwrap();

        assert_eq!(
            std::fs::read_to_string(root.join("folder/note.md")).unwrap(),
            "replacement"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("quarantine.tmp")).unwrap(),
            "original"
        );
        assert!(!root.join("saved/note.md").exists());
    }

    #[test]
    fn atomic_install_adapter_replaces_existing_and_creates_without_overwrite() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("workspace");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("note.md"), "before").unwrap();
        std::fs::write(root.join("stage.tmp"), "after").unwrap();
        let directory = Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap();
        let adapter = KernelDocumentAtomicInstallAdapter::new(&root).unwrap();
        let mut expected_options = OpenOptions::new();
        expected_options.read(true).follow(FollowSymlinks::No);
        #[cfg(windows)]
        expected_options.share_mode(windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ);
        let expected_target = directory.open_with("note.md", &expected_options).unwrap();
        let expected_stage = open_pinned_stage(&directory, "stage.tmp");
        let expected_revision =
            Revision::parse(format!("{:x}", Sha256::digest(b"before"))).unwrap();
        adapter
            .install(AtomicInstallRequest {
                directory: &directory,
                target: &WorkspaceRelativePath::parse("note.md").unwrap(),
                stage_name: "stage.tmp",
                target_name: "note.md",
                mode: AtomicInstallMode::ReplaceExisting,
                expected_stage: PinnedInstallSource::File(&expected_stage),
                expected_target: Some(&expected_target),
                expected_revision: Some(&expected_revision),
            })
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("note.md")).unwrap(),
            "after"
        );

        std::fs::write(root.join("create.tmp"), "created").unwrap();
        let create_stage = open_pinned_stage(&directory, "create.tmp");
        adapter
            .install(AtomicInstallRequest {
                directory: &directory,
                target: &WorkspaceRelativePath::parse("created.md").unwrap(),
                stage_name: "create.tmp",
                target_name: "created.md",
                mode: AtomicInstallMode::CreateNoReplace,
                expected_stage: PinnedInstallSource::File(&create_stage),
                expected_target: None,
                expected_revision: None,
            })
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("created.md")).unwrap(),
            "created"
        );
    }

    #[cfg(unix)]
    #[test]
    fn atomic_install_adapter_never_creates_from_a_swapped_file_stage() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("workspace");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("stage.tmp"), "intended").unwrap();
        let directory = Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap();
        let adapter = KernelDocumentAtomicInstallAdapter::new(&root).unwrap();
        let expected_stage = open_pinned_stage(&directory, "stage.tmp");
        std::fs::rename(root.join("stage.tmp"), root.join("retained.tmp")).unwrap();
        std::fs::write(root.join("stage.tmp"), "attacker").unwrap();

        let result = adapter.install(AtomicInstallRequest {
            directory: &directory,
            target: &WorkspaceRelativePath::parse("created.md").unwrap(),
            stage_name: "stage.tmp",
            target_name: "created.md",
            mode: AtomicInstallMode::CreateNoReplace,
            expected_stage: PinnedInstallSource::File(&expected_stage),
            expected_target: None,
            expected_revision: None,
        });

        assert!(result.is_err());
        assert!(!root.join("created.md").exists());
        assert_eq!(
            std::fs::read_to_string(root.join("retained.tmp")).unwrap(),
            "intended"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("stage.tmp")).unwrap(),
            "attacker"
        );
    }

    #[cfg(unix)]
    #[test]
    fn atomic_install_adapter_never_replaces_from_a_swapped_file_stage() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("workspace");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("note.md"), "before").unwrap();
        std::fs::write(root.join("stage.tmp"), "intended").unwrap();
        let directory = Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap();
        let adapter = KernelDocumentAtomicInstallAdapter::new(&root).unwrap();
        let mut expected_options = OpenOptions::new();
        expected_options.read(true).follow(FollowSymlinks::No);
        let expected_target = directory.open_with("note.md", &expected_options).unwrap();
        let expected_stage = open_pinned_stage(&directory, "stage.tmp");
        let expected_revision =
            Revision::parse(format!("{:x}", Sha256::digest(b"before"))).unwrap();
        std::fs::rename(root.join("stage.tmp"), root.join("retained.tmp")).unwrap();
        std::fs::write(root.join("stage.tmp"), "attacker").unwrap();

        let result = adapter.install(AtomicInstallRequest {
            directory: &directory,
            target: &WorkspaceRelativePath::parse("note.md").unwrap(),
            stage_name: "stage.tmp",
            target_name: "note.md",
            mode: AtomicInstallMode::ReplaceExisting,
            expected_stage: PinnedInstallSource::File(&expected_stage),
            expected_target: Some(&expected_target),
            expected_revision: Some(&expected_revision),
        });

        assert!(result.is_err());
        assert_eq!(
            std::fs::read_to_string(root.join("note.md")).unwrap(),
            "before"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("retained.tmp")).unwrap(),
            "intended"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("stage.tmp")).unwrap(),
            "attacker"
        );
    }

    #[cfg(unix)]
    #[test]
    fn atomic_install_adapter_never_creates_from_a_swapped_directory_stage() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("workspace");
        std::fs::create_dir_all(root.join("stage.tmp")).unwrap();
        std::fs::write(root.join("stage.tmp/intended.md"), "intended").unwrap();
        let directory = Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap();
        let adapter = KernelDocumentAtomicInstallAdapter::new(&root).unwrap();
        let expected_stage = directory.open_dir_nofollow("stage.tmp").unwrap();
        std::fs::rename(root.join("stage.tmp"), root.join("retained.tmp")).unwrap();
        std::fs::create_dir(root.join("stage.tmp")).unwrap();
        std::fs::write(root.join("stage.tmp/attacker.md"), "attacker").unwrap();

        let result = adapter.install(AtomicInstallRequest {
            directory: &directory,
            target: &WorkspaceRelativePath::parse("created").unwrap(),
            stage_name: "stage.tmp",
            target_name: "created",
            mode: AtomicInstallMode::CreateNoReplace,
            expected_stage: PinnedInstallSource::Directory(&expected_stage),
            expected_target: None,
            expected_revision: None,
        });

        assert!(result.is_err());
        assert!(!root.join("created").exists());
        assert_eq!(
            std::fs::read_to_string(root.join("retained.tmp/intended.md")).unwrap(),
            "intended"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("stage.tmp/attacker.md")).unwrap(),
            "attacker"
        );
    }

    #[cfg(unix)]
    #[test]
    fn atomic_install_never_rolls_an_unknown_retired_name_into_the_target() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("workspace");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("note.md"), "before").unwrap();
        std::fs::write(root.join("stage.tmp"), "after").unwrap();
        let directory = Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap();
        let mut expected_options = OpenOptions::new();
        expected_options.read(true).follow(FollowSymlinks::No);
        let expected_target = directory.open_with("note.md", &expected_options).unwrap();
        let expected_stage = open_pinned_stage(&directory, "stage.tmp");
        let expected_revision =
            Revision::parse(format!("{:x}", Sha256::digest(b"before"))).unwrap();
        let request = AtomicInstallRequest {
            directory: &directory,
            target: &WorkspaceRelativePath::parse("note.md").unwrap(),
            stage_name: "stage.tmp",
            target_name: "note.md",
            mode: AtomicInstallMode::ReplaceExisting,
            expected_stage: PinnedInstallSource::File(&expected_stage),
            expected_target: Some(&expected_target),
            expected_revision: Some(&expected_revision),
        };

        let result = replace_kernel_document_compare_exchange_with_hook(
            &directory,
            &request,
            &root.join("stage.tmp"),
            &root.join("note.md"),
            || {
                directory.remove_file("stage.tmp").unwrap();
                directory.write("stage.tmp", b"unknown entry").unwrap();
            },
        );

        assert!(result.is_err());
        assert_eq!(
            std::fs::read_to_string(root.join("note.md")).unwrap(),
            "after"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("stage.tmp")).unwrap(),
            "unknown entry"
        );
    }

    #[test]
    fn atomic_install_adapter_rejects_a_replaced_workspace_root() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("workspace");
        let moved = fixture.path().join("moved");
        std::fs::create_dir(&root).unwrap();
        let adapter = KernelDocumentAtomicInstallAdapter::new(&root).unwrap();
        std::fs::rename(&root, &moved).unwrap();
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("stage.tmp"), "replacement").unwrap();
        let replacement = Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap();
        let replacement_stage = open_pinned_stage(&replacement, "stage.tmp");

        assert!(adapter
            .install(AtomicInstallRequest {
                directory: &replacement,
                target: &WorkspaceRelativePath::parse("note.md").unwrap(),
                stage_name: "stage.tmp",
                target_name: "note.md",
                mode: AtomicInstallMode::CreateNoReplace,
                expected_stage: PinnedInstallSource::File(&replacement_stage),
                expected_target: None,
                expected_revision: None,
            })
            .is_err());
        assert_eq!(
            std::fs::read_to_string(root.join("stage.tmp")).unwrap(),
            "replacement"
        );
    }

    #[test]
    fn ignore_adapter_combines_workspace_global_and_protected_rules() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("workspace");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join(".markraignore"), "workspace-hidden.md\n").unwrap();
        let retained = Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap();
        let adapter =
            KernelDocumentIgnoreAdapter::new(&root, &retained, Some("global-hidden.md\n")).unwrap();

        for path in [
            "workspace-hidden.md",
            "global-hidden.md",
            ".QINGYU/hidden.md",
            ".MARKRA-SYNC/hidden.md",
        ] {
            assert!(adapter.is_ignored(
                &WorkspaceRelativePath::parse(path).unwrap(),
                DocumentKind::File
            ));
        }
        assert!(!adapter.is_ignored(
            &WorkspaceRelativePath::parse("visible.md").unwrap(),
            DocumentKind::File
        ));
    }

    #[tokio::test]
    async fn tauri_facade_preserves_direct_dtos_errors_revisions_history_search_and_events() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("workspace");
        let app_data = fixture.path().join("app-data");
        let cache = fixture.path().join("cache");
        for path in [&root, &app_data, &cache] {
            std::fs::create_dir(path).unwrap();
        }
        std::fs::write(root.join(".markraignore"), "workspace-hidden.md\n").unwrap();
        let paths = KernelPaths::desktop(&root, &app_data, &cache).unwrap();
        let managed = ManagedWorkspaceCollection::from_paths(&paths).unwrap();
        let runtime = KernelRuntime::activate(
            KernelConfig::generate().unwrap(),
            paths,
            KernelPorts::unavailable(),
        )
        .unwrap();
        let workspace = Arc::new(
            WorkspaceService::new(
                &runtime,
                Arc::new(MemoryWorkspaceStore::default()),
                managed,
                runtime.event_broker().clone(),
                "Tauri parity",
            )
            .await
            .unwrap(),
        );
        let retained = Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap();
        let service = Arc::new(
            WorkspaceDocumentService::new_with_ports(
                &runtime,
                Arc::new(
                    KernelDocumentDeletionAdapter::new(
                        &root,
                        Arc::new(|path| {
                            std::fs::remove_file(path).map_err(|error| error.to_string())
                        }),
                    )
                    .unwrap(),
                ),
                Arc::new(KernelDocumentHistoryAdapter::new(&app_data.join("history")).unwrap()),
                Arc::new(MemoryDocumentRecoveryStore::default()),
                Arc::new(KernelDocumentAtomicInstallAdapter::new(&root).unwrap()),
                Arc::new(StaticWorkspaceIgnorePort::new(Arc::new(
                    KernelDocumentIgnoreAdapter::new(&root, &retained, Some("global-hidden.md\n"))
                        .unwrap(),
                ))),
            )
            .unwrap(),
        );
        let facade = KernelDocumentsTauriFacade::new(service.clone());
        let generation = workspace.current().unwrap().generation;
        let created = facade
            .create(CreateDocumentRequest::File {
                workspace_generation: generation.clone(),
                parent: WorkspaceRelativePath::default(),
                name: FileDocumentName::parse("note.md").unwrap(),
                contents: DocumentContents::parse("needle first").unwrap(),
            })
            .await
            .unwrap();
        let (id, revision) = match created {
            qingyu_kernel::contract::CreatedDocumentDto::File { id, revision, .. } => {
                (id, revision)
            }
            _ => panic!("file expected"),
        };
        std::fs::write(root.join("workspace-hidden.md"), "needle hidden").unwrap();
        std::fs::write(root.join("global-hidden.md"), "needle hidden").unwrap();
        std::fs::create_dir_all(root.join(".MARKRA-SYNC")).unwrap();
        std::fs::write(root.join(".MARKRA-SYNC/hidden.md"), "needle hidden").unwrap();
        let list_query = ListDocumentsQuery {
            cursor: None,
            limit: None,
            parent: WorkspaceRelativePath::default(),
        };
        let tauri_list = facade.list(list_query.clone()).await.unwrap();
        let direct_list = DocumentsApiService::list_documents(service.as_ref(), list_query)
            .await
            .unwrap();
        assert_eq!(tauri_list, direct_list);
        assert_eq!(tauri_list.items.len(), 1);
        assert_eq!(tauri_list.items[0].path.as_str(), "note.md");
        assert_eq!(
            facade.read(id.clone()).await.unwrap(),
            DocumentsApiService::get_document(service.as_ref(), id.clone())
                .await
                .unwrap()
        );
        let search_query = SearchWorkspaceQuery {
            cursor: None,
            limit: None,
            query: SearchQuery::parse("needle").unwrap(),
        };
        let tauri_search = facade.search(search_query.clone()).await.unwrap();
        let direct_search = DocumentsApiService::search_workspace(service.as_ref(), search_query)
            .await
            .unwrap();
        assert_eq!(tauri_search, direct_search);
        assert_eq!(tauri_search.items.len(), 1);
        assert_eq!(tauri_search.items[0].document.path.as_str(), "note.md");

        let mut events = runtime.event_broker().subscribe();
        let updated = facade
            .write(
                id.clone(),
                UpdateDocumentRequest {
                    workspace_generation: generation.clone(),
                    expected_revision: revision,
                    contents: DocumentContents::parse("second").unwrap(),
                },
            )
            .await
            .unwrap();
        let publication = events.recv().await.unwrap();
        assert_eq!(publication.revision, updated.revision);
        match publication.event {
            qingyu_kernel::contract::DomainEvent::DocumentChanged { document } => {
                assert_eq!(document.revision, updated.revision);
            }
            _ => panic!("document changed event expected"),
        }
        assert_eq!(
            updated,
            DocumentsApiService::get_document(service.as_ref(), id.clone())
                .await
                .unwrap()
        );

        let page_query = PageQuery {
            cursor: None,
            limit: None,
        };
        let history = facade
            .history(id.clone(), page_query.clone())
            .await
            .unwrap();
        assert_eq!(
            history,
            DocumentsApiService::list_document_history(service.as_ref(), id.clone(), page_query)
                .await
                .unwrap()
        );
        let restored = facade
            .restore(
                id.clone(),
                history.items[0].snapshot_id,
                RestoreDocumentHistoryRequest {
                    workspace_generation: generation,
                    expected_revision: updated.revision,
                },
            )
            .await
            .unwrap();
        assert_eq!(restored.contents.as_str(), "needle first");

        let stale = runtime
            .wire_identity_key()
            .issue_document_id(
                workspace.current().unwrap().id,
                &WorkspaceGeneration::parse("stale-generation").unwrap(),
                DocumentKind::File,
                &WorkspaceRelativePath::parse("note.md").unwrap(),
            )
            .unwrap();
        let direct_error = DocumentsApiService::get_document(service.as_ref(), stale.clone())
            .await
            .unwrap_err();
        let tauri_error = facade.read(stale).await.unwrap_err();
        assert_eq!(tauri_error.code(), direct_error.code());
        assert_eq!(tauri_error.details.as_ref(), direct_error.details());
    }
}
