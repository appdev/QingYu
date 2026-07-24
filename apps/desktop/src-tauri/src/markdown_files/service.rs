use std::{
    fmt,
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsExt, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::mcp::{
    config::{DeletionPolicy, SyncAfterWritePolicy},
    handles::{HandleSigner, VerifiedDocumentHandle, VerifiedFolderHandle},
    workspaces::{ResolvedWorkspace, WorkspaceError},
};

use super::{
    history::snapshot_markdown_file_history_contents,
    ignore_rules::MarkdownIgnoreRules,
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
#[cfg(test)]
type BeforeAtomicDocumentMutation = dyn Fn() + Send + Sync;

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
            } => Ok(MarkdownIgnoreRules::for_root(
                &workspace.canonical_path,
                global_ignore_rules.as_deref(),
            )),
            Self::TrustedUi {
                root,
                global_ignore_rules,
            } => Ok(MarkdownIgnoreRules::for_root(
                root,
                global_ignore_rules.as_deref(),
            )),
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
    system_trash: Arc<SystemTrash>,
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
            system_trash: Arc::new(|path| trash::delete(path).map_err(|error| error.to_string())),
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
    pub(crate) fn with_system_trash(
        mut self,
        delete: impl Fn(&Path) -> Result<(), String> + Send + Sync + 'static,
    ) -> Self {
        self.system_trash = Arc::new(delete);
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
        let ambient_path = workspace.canonical_path.join(relative_path);
        workspace
            .revalidate_authority()
            .map_err(map_workspace_error)?;
        match input.deletion {
            DeletionPolicy::SystemTrash => {
                let latest = read_document_bytes(input.document, self.search_document_limit_bytes)?;
                validate_expected_revision(&latest, input.expected_revision)?;
                (self.system_trash)(&ambient_path)
                    .map_err(|_| DocumentServiceError::mutation_failed())?;
            }
            DeletionPolicy::QingYuRecycleBin => {
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

fn mutation_workspace<'a>(
    scope: &'a DocumentScope,
    workspace_id: Uuid,
) -> Result<&'a ResolvedWorkspace, DocumentServiceError> {
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
    if name.is_empty()
        || name.len() > 255
        || name.contains(['/', '\\', '\0'])
        || name.contains(['<', '>', ':', '"', '|', '?', '*'])
        || name.ends_with(['.', ' '])
        || !Path::new(name)
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                matches!(extension.to_ascii_lowercase().as_str(), "md" | "markdown")
            })
        || Path::new(name).components().count() != 1
        || name.starts_with(UPDATE_TEMP_PREFIX)
        || is_windows_reserved_name(name)
    {
        return Err(DocumentServiceError::invalid_name());
    }
    Ok(name)
}

fn is_windows_reserved_name(name: &str) -> bool {
    let stem = name
        .split('.')
        .next()
        .unwrap_or(name)
        .trim_end_matches(['.', ' '])
        .to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || stem
            .strip_prefix("COM")
            .or_else(|| stem.strip_prefix("LPT"))
            .is_some_and(|suffix| suffix.len() == 1 && matches!(suffix.as_bytes()[0], b'1'..=b'9'))
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
    _source: &Dir,
    _source_name: impl AsRef<Path>,
    _destination: &Dir,
    _destination_name: impl AsRef<Path>,
    source_ambient: &Path,
    destination_ambient: &Path,
) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH};

    let source = source_ambient
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination_ambient
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let moved = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
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
