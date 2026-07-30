//! Capability-addressed, platform-neutral workspace document service.

use std::{
    fmt,
    io::{self, Read as _, Seek as _, SeekFrom, Write as _},
    path::Path,
    sync::{Arc, Weak},
    time::SystemTime,
};

use async_trait::async_trait;
#[cfg(any(unix, windows))]
use cap_fs_ext::OpenOptionsExt;
use cap_fs_ext::{DirExt, FollowSymlinks, MetadataExt, OpenOptionsFollowExt};
use cap_std::fs::{Dir, File, Metadata, OpenOptions};
use sha2::{Digest as _, Sha256};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use uuid::Uuid;

use crate::{
    contract::{
        CreateDocumentRequest, CreatedDocumentDto, DeleteDocumentRequest, DocumentContentDto,
        DocumentContents, DocumentEntryDto, DocumentHistoryPageDto, DocumentId, DocumentKind,
        ErrorCode, ErrorDetails, FileDocumentKind, FileDocumentName, HistoryEntryDto,
        ListDocumentsQuery, MoveDocumentRequest, Nullable, PageCursorContext, PageQuery,
        PositiveSafeInteger, ResourceRefDto, Revision, Rfc3339Utc, SafeUnsignedInteger,
        SearchMatchDto, SearchPageDto, SearchWorkspaceQuery, SnapshotId, UpdateDocumentRequest,
        WorkspaceDto, WorkspaceReadiness, WorkspaceRelativePath,
    },
    documents::{
        history::{
            DocumentHistoryStore, DocumentRecoveryIntent, DocumentRecoveryOutcome,
            DocumentRecoveryStore, MemoryDocumentRecoveryStore,
        },
        identity::DocumentIdentityCodec,
        AllowAllDocumentIgnorePort, AtomicInstallMode, AtomicInstallPort, AtomicInstallPortError,
        AtomicInstallRequest, CapabilityMoveInstallPort, DeletionPort, DocumentDeletionTarget,
        DocumentIgnorePort, MoveInstallPort, MoveInstallPortError, MoveInstallRequest,
        PinnedInstallSource, PinnedMoveSource,
    },
    events::{EventPublication, EventSink as _},
    runtime::{ActiveWorkspaceSnapshot, DocumentsApiService, KernelRuntime, ServiceFailure},
};

const MAX_SEARCH_MATCHES: usize = 10_000;

pub struct WorkspaceDocumentService {
    runtime: Weak<KernelRuntime>,
    deletion: Arc<dyn DeletionPort>,
    history: Arc<dyn DocumentHistoryStore>,
    recovery: Arc<dyn DocumentRecoveryStore>,
    atomic_install: Arc<dyn AtomicInstallPort>,
    move_install: Arc<dyn MoveInstallPort>,
    ignore: Arc<dyn DocumentIgnorePort>,
}

#[derive(Default)]
pub struct CapabilityAtomicInstallPort;

impl AtomicInstallPort for CapabilityAtomicInstallPort {
    fn install(&self, request: AtomicInstallRequest<'_>) -> Result<(), AtomicInstallPortError> {
        capability_atomic_install(request)
    }
}

impl MoveInstallPort for CapabilityMoveInstallPort {
    fn install(&self, request: MoveInstallRequest<'_>) -> Result<(), MoveInstallPortError> {
        capability_move_install(request)
    }
}

impl WorkspaceDocumentService {
    pub fn new(
        runtime: &Arc<KernelRuntime>,
        deletion: Arc<dyn DeletionPort>,
        history: Arc<dyn DocumentHistoryStore>,
    ) -> Self {
        Self {
            runtime: Arc::downgrade(runtime),
            deletion,
            history,
            recovery: Arc::new(MemoryDocumentRecoveryStore::default()),
            atomic_install: Arc::new(CapabilityAtomicInstallPort),
            move_install: Arc::new(CapabilityMoveInstallPort),
            ignore: Arc::new(AllowAllDocumentIgnorePort),
        }
    }

    pub fn new_with_recovery(
        runtime: &Arc<KernelRuntime>,
        deletion: Arc<dyn DeletionPort>,
        history: Arc<dyn DocumentHistoryStore>,
        recovery: Arc<dyn DocumentRecoveryStore>,
    ) -> Result<Self, DocumentServiceError> {
        let service = Self {
            runtime: Arc::downgrade(runtime),
            deletion,
            history,
            recovery,
            atomic_install: Arc::new(CapabilityAtomicInstallPort),
            move_install: Arc::new(CapabilityMoveInstallPort),
            ignore: Arc::new(AllowAllDocumentIgnorePort),
        };
        service.recover()?;
        Ok(service)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_ports(
        runtime: &Arc<KernelRuntime>,
        deletion: Arc<dyn DeletionPort>,
        history: Arc<dyn DocumentHistoryStore>,
        recovery: Arc<dyn DocumentRecoveryStore>,
        atomic_install: Arc<dyn AtomicInstallPort>,
        ignore: Arc<dyn DocumentIgnorePort>,
    ) -> Result<Self, DocumentServiceError> {
        let service = Self {
            runtime: Arc::downgrade(runtime),
            deletion,
            history,
            recovery,
            atomic_install,
            move_install: Arc::new(CapabilityMoveInstallPort),
            ignore,
        };
        service.recover()?;
        Ok(service)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_mutation_ports(
        runtime: &Arc<KernelRuntime>,
        deletion: Arc<dyn DeletionPort>,
        history: Arc<dyn DocumentHistoryStore>,
        recovery: Arc<dyn DocumentRecoveryStore>,
        atomic_install: Arc<dyn AtomicInstallPort>,
        move_install: Arc<dyn MoveInstallPort>,
        ignore: Arc<dyn DocumentIgnorePort>,
    ) -> Result<Self, DocumentServiceError> {
        let service = Self {
            runtime: Arc::downgrade(runtime),
            deletion,
            history,
            recovery,
            atomic_install,
            move_install,
            ignore,
        };
        service.recover()?;
        Ok(service)
    }

    pub fn recover(&self) -> Result<Vec<DocumentRecoveryOutcome>, DocumentServiceError> {
        let context = self.context()?;
        let intents = self
            .recovery
            .pending()
            .map_err(|_| DocumentServiceError::recovery_required())?;
        let mut outcomes = Vec::with_capacity(intents.len());
        for intent in intents {
            outcomes.push(self.recover_intent(&context.root, &intent)?);
        }
        Ok(outcomes)
    }

    pub async fn list_documents(
        &self,
        query: ListDocumentsQuery,
    ) -> Result<crate::contract::DocumentPageDto, DocumentServiceError> {
        let context = self.context()?;
        let directory = open_directory(&context.root, &query.parent)?;
        let mut entries = Vec::new();
        for entry in directory
            .entries()
            .map_err(|_| DocumentServiceError::unavailable())?
        {
            let Ok(entry) = entry else {
                continue;
            };
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if protected_name(&name) {
                continue;
            }
            let Ok(metadata) = directory.symlink_metadata(entry.file_name()) else {
                continue;
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            let kind = if metadata.is_dir() {
                DocumentKind::Directory
            } else if metadata.is_file() && markdown_name(&name) && trusted_metadata(&metadata) {
                DocumentKind::File
            } else {
                continue;
            };
            let path = join_relative(&query.parent, &name)?;
            if self.ignore.is_ignored(&path, kind) {
                continue;
            }
            if let Ok(entry) = self.entry(&context, path, kind, metadata) {
                entries.push(entry);
            }
        }
        entries.sort_by(|left, right| left.path.as_str().cmp(right.path.as_str()));

        let normalized = query.parent.as_str();
        let cursor_context = PageCursorContext::new(
            "documents-list",
            normalized,
            &context.workspace().generation,
        )
        .map_err(|_| DocumentServiceError::invalid_cursor())?;
        let start = match query.cursor.as_ref() {
            Some(cursor) => {
                let last = context
                    .runtime
                    .wire_identity_key()
                    .verify_page_cursor(cursor, &cursor_context)
                    .map_err(|_| DocumentServiceError::invalid_cursor())?;
                entries.partition_point(|entry| entry.path.as_str() <= last.as_str())
            }
            None => 0,
        };
        let limit = query.limit.map_or(100, |limit| usize::from(limit.get()));
        let end = start.saturating_add(limit).min(entries.len());
        let items = entries[start..end].to_vec();
        let next_cursor = if end < entries.len() {
            let last = items
                .last()
                .ok_or_else(DocumentServiceError::invalid_cursor)?;
            Nullable::value(
                context
                    .runtime
                    .wire_identity_key()
                    .issue_page_cursor(&cursor_context, last.path.as_str())
                    .map_err(|_| DocumentServiceError::invalid_cursor())?,
            )
        } else {
            Nullable::null()
        };
        Ok(crate::contract::DocumentPageDto { items, next_cursor })
    }

    pub async fn create_document(
        &self,
        request: CreateDocumentRequest,
    ) -> Result<CreatedDocumentDto, DocumentServiceError> {
        let runtime = self.runtime()?;
        let mutation = runtime.mutation_coordinator().lock().await;
        runtime
            .verify_document_mutation_admission(&mutation)
            .map_err(|_| DocumentServiceError::unavailable())?;
        let context = self.context_with_runtime(runtime.clone())?;
        let (generation, parent, name, kind, contents) = match request {
            CreateDocumentRequest::File {
                workspace_generation,
                parent,
                name,
                contents,
            } => (
                workspace_generation,
                parent,
                name.as_str().to_string(),
                DocumentKind::File,
                Some(contents),
            ),
            CreateDocumentRequest::Directory {
                workspace_generation,
                parent,
                name,
            } => (
                workspace_generation,
                parent,
                name.as_str().to_string(),
                DocumentKind::Directory,
                None,
            ),
        };
        self.verify_generation(context.workspace(), &generation)?;
        let directory = open_directory(&context.root, &parent)?;
        ensure_absent(&directory, &name)?;
        let path = join_relative(&parent, &name)?;
        match contents.as_ref() {
            Some(contents) => self.create_atomic_recoverable(
                &directory,
                &path,
                &name,
                contents.as_str().as_bytes(),
            )?,
            None => self.create_directory_recoverable(&directory, &path, &name)?,
        }
        match contents {
            Some(_) => {
                let read = read_file(&context.root, &path)?;
                let entry = self.file_entry(&context, path, &read)?;
                let contents = DocumentContents::parse(
                    String::from_utf8(read.bytes)
                        .map_err(|_| DocumentServiceError::invalid_encoding())?,
                )
                .map_err(|_| DocumentServiceError::too_large())?;
                self.publish(&context.runtime, &entry, DocumentEvent::Created, None);
                Ok(CreatedDocumentDto::File {
                    id: entry.id,
                    path: entry.path,
                    parent: entry.parent,
                    name: FileDocumentName::parse(entry.name.as_str())
                        .map_err(|_| DocumentServiceError::invalid_name())?,
                    size_bytes: entry.size_bytes,
                    modified_at: entry.modified_at,
                    revision: entry.revision,
                    contents,
                })
            }
            None => {
                let read = read_directory(&context.root, &path)?;
                let entry =
                    self.entry_from_snapshot(&context, path, kind, &read.metadata, read.revision)?;
                self.publish(&context.runtime, &entry, DocumentEvent::Created, None);
                Ok(CreatedDocumentDto::Directory {
                    id: entry.id,
                    path: entry.path,
                    parent: entry.parent,
                    name: entry.name,
                    size_bytes: entry.size_bytes,
                    modified_at: entry.modified_at,
                    revision: entry.revision,
                })
            }
        }
    }

    pub async fn get_document(
        &self,
        document_id: DocumentId,
    ) -> Result<DocumentContentDto, DocumentServiceError> {
        let context = self.context()?;
        self.read_document(&context, &document_id)
    }

    pub async fn update_document(
        &self,
        document_id: DocumentId,
        request: UpdateDocumentRequest,
    ) -> Result<DocumentContentDto, DocumentServiceError> {
        let runtime = self.runtime()?;
        let mutation = runtime.mutation_coordinator().lock().await;
        runtime
            .verify_document_mutation_admission(&mutation)
            .map_err(|_| DocumentServiceError::unavailable())?;
        let context = self.context_with_runtime(runtime.clone())?;
        self.verify_generation(context.workspace(), &request.workspace_generation)?;
        let path = self.verify_id(&context, &document_id, DocumentKind::File)?;
        let current = read_file(&context.root, &path)?;
        if current.revision != request.expected_revision {
            return Err(DocumentServiceError::revision_conflict(current.revision));
        }
        let now = now_utc()?;
        self.history
            .preserve(&path, &current.bytes, &current.revision, &now)
            .map_err(|_| DocumentServiceError::history_unavailable())?;
        let (parent, name) = open_parent(&context.root, &path)?;
        self.replace_atomic_recoverable(
            &parent,
            &path,
            &name,
            &current,
            request.contents.as_str().as_bytes(),
        )?;
        let content = self.read_document(&context, &document_id)?;
        self.publish_content(&context.runtime, &content, DocumentEvent::Changed, None);
        Ok(content)
    }

    pub async fn move_document(
        &self,
        document_id: DocumentId,
        request: MoveDocumentRequest,
    ) -> Result<DocumentEntryDto, DocumentServiceError> {
        let runtime = self.runtime()?;
        let mutation = runtime.mutation_coordinator().lock().await;
        runtime
            .verify_document_mutation_admission(&mutation)
            .map_err(|_| DocumentServiceError::unavailable())?;
        let context = self.context_with_runtime(runtime.clone())?;
        self.verify_generation(context.workspace(), &request.workspace_generation)?;
        let (kind, source) = self.verify_any_id(&context, &document_id)?;
        let source_snapshot = match kind {
            DocumentKind::File => MoveSourceSnapshot::File(read_file(&context.root, &source)?),
            DocumentKind::Directory => {
                MoveSourceSnapshot::Directory(read_directory(&context.root, &source)?)
            }
        };
        let source_revision = source_snapshot.revision();
        if source_revision != &request.expected_revision {
            return Err(DocumentServiceError::revision_conflict(
                source_revision.clone(),
            ));
        }
        request
            .name
            .validate_kind(kind)
            .map_err(|_| DocumentServiceError::invalid_name())?;
        let (source_parent, source_name) = open_parent(&context.root, &source)?;
        let target_parent = open_directory(&context.root, &request.target_parent)?;
        ensure_absent(&target_parent, request.name.as_str())?;
        let target = join_relative(&request.target_parent, request.name.as_str())?;
        self.move_recoverable(
            &source_parent,
            &source_name,
            &source,
            &target_parent,
            request.name.as_str(),
            &target,
            kind,
            source_snapshot.pinned(),
            &request.expected_revision,
        )?;
        let metadata = target_parent
            .symlink_metadata(request.name.as_str())
            .map_err(|_| DocumentServiceError::unavailable())?;
        let entry = self.entry(&context, target, kind, metadata)?;
        self.publish(&context.runtime, &entry, DocumentEvent::Moved, Some(source));
        Ok(entry)
    }

    pub async fn delete_document(
        &self,
        document_id: DocumentId,
        request: DeleteDocumentRequest,
    ) -> Result<(), DocumentServiceError> {
        let runtime = self.runtime()?;
        let mutation = runtime.mutation_coordinator().lock().await;
        runtime
            .verify_document_mutation_admission(&mutation)
            .map_err(|_| DocumentServiceError::unavailable())?;
        let context = self.context_with_runtime(runtime.clone())?;
        self.verify_generation(context.workspace(), &request.workspace_generation)?;
        let (kind, path) = self.verify_any_id(&context, &document_id)?;
        let metadata = metadata_at(&context.root, &path)?;
        let current_revision = revision_for_metadata_and_contents(&context.root, &path, &metadata)?;
        if current_revision != request.expected_revision {
            return Err(DocumentServiceError::revision_conflict(current_revision));
        }
        if kind == DocumentKind::File {
            let current = read_file(&context.root, &path)?;
            self.history
                .preserve(&path, &current.bytes, &current.revision, &now_utc()?)
                .map_err(|_| DocumentServiceError::history_unavailable())?;
        }
        self.deletion
            .delete(
                &DocumentDeletionTarget {
                    path: path.clone(),
                    kind,
                    revision: current_revision.clone(),
                },
                request.deletion_policy,
            )
            .map_err(|_| DocumentServiceError::deletion_unavailable())?;
        if optional_revision(&context.root, &path)?.is_some() {
            return Err(DocumentServiceError::recovery_required());
        }
        let publication = EventPublication {
            resource: ResourceRefDto::Document {
                id: document_id.clone(),
            },
            revision: current_revision.clone(),
            event: crate::contract::DomainEvent::DocumentDeleted {
                document_id,
                previous_path: path,
                workspace_generation: context.workspace().generation.clone(),
                revision: current_revision,
            },
        };
        let _publication_result = context.runtime.publish(&publication);
        Ok(())
    }

    pub async fn list_document_history(
        &self,
        document_id: DocumentId,
        query: PageQuery,
    ) -> Result<DocumentHistoryPageDto, DocumentServiceError> {
        let context = self.context()?;
        let path = self.verify_id(&context, &document_id, DocumentKind::File)?;
        let mut snapshots = self
            .history
            .list(&path)
            .map_err(|_| DocumentServiceError::history_unavailable())?;
        snapshots.sort_by_key(history_identity);
        let cursor_context = PageCursorContext::new(
            "document-history",
            path.as_str(),
            &context.workspace().generation,
        )
        .map_err(|_| DocumentServiceError::invalid_cursor())?;
        let start = match query.cursor.as_ref() {
            Some(cursor) => {
                let last = context
                    .runtime
                    .wire_identity_key()
                    .verify_page_cursor(cursor, &cursor_context)
                    .map_err(|_| DocumentServiceError::invalid_cursor())?;
                snapshots.partition_point(|entry| history_identity(entry) <= last)
            }
            None => 0,
        };
        let limit = query.limit.map_or(100, |limit| usize::from(limit.get()));
        let end = start.saturating_add(limit).min(snapshots.len());
        let selected = &snapshots[start..end];
        let items = selected
            .iter()
            .map(|snapshot| {
                Ok(HistoryEntryDto {
                    snapshot_id: snapshot.snapshot_id,
                    document_id: document_id.clone(),
                    created_at: snapshot.created_at.clone(),
                    size_bytes: SafeUnsignedInteger::new(snapshot.contents.len() as u64)
                        .map_err(|_| DocumentServiceError::too_large())?,
                    revision: snapshot.revision.clone(),
                })
            })
            .collect::<Result<Vec<_>, DocumentServiceError>>()?;
        let next_cursor = if end < snapshots.len() {
            Nullable::value(
                context
                    .runtime
                    .wire_identity_key()
                    .issue_page_cursor(
                        &cursor_context,
                        history_identity(
                            selected
                                .last()
                                .ok_or_else(DocumentServiceError::invalid_cursor)?,
                        ),
                    )
                    .map_err(|_| DocumentServiceError::invalid_cursor())?,
            )
        } else {
            Nullable::null()
        };
        Ok(DocumentHistoryPageDto { items, next_cursor })
    }

    pub async fn restore_document_history(
        &self,
        document_id: DocumentId,
        snapshot_id: SnapshotId,
        request: crate::contract::RestoreDocumentHistoryRequest,
    ) -> Result<DocumentContentDto, DocumentServiceError> {
        let runtime = self.runtime()?;
        let mutation = runtime.mutation_coordinator().lock().await;
        runtime
            .verify_document_mutation_admission(&mutation)
            .map_err(|_| DocumentServiceError::unavailable())?;
        let context = self.context_with_runtime(runtime.clone())?;
        self.verify_generation(context.workspace(), &request.workspace_generation)?;
        let path = self.verify_id(&context, &document_id, DocumentKind::File)?;
        let current = read_file(&context.root, &path)?;
        if current.revision != request.expected_revision {
            return Err(DocumentServiceError::revision_conflict(current.revision));
        }
        let snapshot = self
            .history
            .get(&path, snapshot_id)
            .map_err(|_| DocumentServiceError::history_unavailable())?
            .ok_or_else(DocumentServiceError::not_found)?;
        self.history
            .preserve(&path, &current.bytes, &current.revision, &now_utc()?)
            .map_err(|_| DocumentServiceError::history_unavailable())?;
        let (parent, name) = open_parent(&context.root, &path)?;
        self.replace_atomic_recoverable(&parent, &path, &name, &current, &snapshot.contents)?;
        let content = self.read_document(&context, &document_id)?;
        self.publish_content(&context.runtime, &content, DocumentEvent::Changed, None);
        Ok(content)
    }

    pub async fn search_workspace(
        &self,
        query: SearchWorkspaceQuery,
    ) -> Result<SearchPageDto, DocumentServiceError> {
        let context = self.context()?;
        let mut matches = Vec::new();
        collect_search(
            &context.root,
            &WorkspaceRelativePath::default(),
            query.query.as_str(),
            &context,
            self,
            &mut matches,
        )?;
        matches.sort_by(|left, right| {
            left.document
                .path
                .as_str()
                .cmp(right.document.path.as_str())
                .then(left.line.get().cmp(&right.line.get()))
                .then(left.column.get().cmp(&right.column.get()))
        });
        let cursor_context = PageCursorContext::new(
            "workspace-search",
            query.query.as_str(),
            &context.workspace().generation,
        )
        .map_err(|_| DocumentServiceError::invalid_cursor())?;
        let start = match query.cursor.as_ref() {
            Some(cursor) => {
                let last = context
                    .runtime
                    .wire_identity_key()
                    .verify_page_cursor(cursor, &cursor_context)
                    .map_err(|_| DocumentServiceError::invalid_cursor())?;
                matches.partition_point(|entry| search_identity(entry) <= last)
            }
            None => 0,
        };
        let limit = query.limit.map_or(100, |limit| usize::from(limit.get()));
        let end = start.saturating_add(limit).min(matches.len());
        let items = matches[start..end].to_vec();
        let next_cursor = if end < matches.len() {
            Nullable::value(
                context
                    .runtime
                    .wire_identity_key()
                    .issue_page_cursor(
                        &cursor_context,
                        search_identity(
                            items
                                .last()
                                .ok_or_else(DocumentServiceError::invalid_cursor)?,
                        ),
                    )
                    .map_err(|_| DocumentServiceError::invalid_cursor())?,
            )
        } else {
            Nullable::null()
        };
        Ok(SearchPageDto { items, next_cursor })
    }

    fn create_atomic_recoverable(
        &self,
        directory: &Dir,
        target: &WorkspaceRelativePath,
        name: &str,
        bytes: &[u8],
    ) -> Result<(), DocumentServiceError> {
        let transaction_id = Uuid::new_v4();
        let stage_name = random_stage_name()?;
        let intended_revision = revision_for_bytes(bytes)?;
        let intent = DocumentRecoveryIntent {
            transaction_id,
            source: None,
            target: target.clone(),
            stage_name: Some(stage_name.clone()),
            kind: DocumentKind::File,
            previous_revision: None,
            intended_revision: intended_revision.clone(),
        };
        self.recovery
            .prepare(&intent)
            .map_err(|_| DocumentServiceError::recovery_required())?;
        let mut staged = match stage_named(directory, &stage_name, bytes) {
            Ok(staged) => staged,
            Err(error) => {
                let _clear_result = self.recovery.clear(transaction_id);
                return Err(error);
            }
        };
        verify_staged_identity(directory, &stage_name, &mut staged, &intended_revision)?;
        if self
            .atomic_install
            .install(AtomicInstallRequest {
                directory,
                target,
                stage_name: &stage_name,
                target_name: name,
                mode: AtomicInstallMode::CreateNoReplace,
                expected_stage: PinnedInstallSource::File(&staged.file),
                expected_target: None,
                expected_revision: None,
            })
            .is_err()
        {
            return Err(DocumentServiceError::recovery_required());
        }
        sync_dir(directory).map_err(|_| DocumentServiceError::recovery_required())?;
        verify_installed_identity(directory, name, &mut staged, &intended_revision)?;
        self.recovery
            .complete(transaction_id)
            .map_err(|_| DocumentServiceError::recovery_required())
    }

    fn create_directory_recoverable(
        &self,
        directory: &Dir,
        target: &WorkspaceRelativePath,
        name: &str,
    ) -> Result<(), DocumentServiceError> {
        let transaction_id = Uuid::new_v4();
        let stage_name = random_stage_name()?;
        let intended_revision = empty_directory_revision()?;
        let intent = DocumentRecoveryIntent {
            transaction_id,
            source: None,
            target: target.clone(),
            stage_name: Some(stage_name.clone()),
            kind: DocumentKind::Directory,
            previous_revision: None,
            intended_revision: intended_revision.clone(),
        };
        self.recovery
            .prepare(&intent)
            .map_err(|_| DocumentServiceError::recovery_required())?;
        if let Err(error) = directory.create_dir(&stage_name) {
            let _clear_result = self.recovery.clear(transaction_id);
            return Err(map_create_error(&error));
        }
        let staged = open_staged_directory(directory, &stage_name)?;
        let staged_identity = staged
            .dir_metadata()
            .map_err(|_| DocumentServiceError::recovery_required())?;
        // Creating the directory and retaining its handle are separate host
        // operations. The initial and post-publication revision checks keep a
        // changed directory fail-closed, but do not claim a strict OS CAS.
        if directory_revision_for_capability(&staged)? != intended_revision {
            return Err(DocumentServiceError::recovery_required());
        }
        self.atomic_install
            .install(AtomicInstallRequest {
                directory,
                target,
                stage_name: &stage_name,
                target_name: name,
                mode: AtomicInstallMode::CreateNoReplace,
                expected_stage: PinnedInstallSource::Directory(&staged),
                expected_target: None,
                expected_revision: None,
            })
            .map_err(|_| DocumentServiceError::recovery_required())?;
        sync_dir(directory).map_err(|_| DocumentServiceError::recovery_required())?;
        let installed_identity = directory
            .symlink_metadata(name)
            .map_err(|_| DocumentServiceError::recovery_required())?;
        if !same_file(&staged_identity, &installed_identity)
            || directory_revision_for_capability(&staged)? != intended_revision
        {
            return Err(DocumentServiceError::recovery_required());
        }
        self.recovery
            .complete(transaction_id)
            .map_err(|_| DocumentServiceError::recovery_required())
    }

    fn replace_atomic_recoverable(
        &self,
        directory: &Dir,
        target: &WorkspaceRelativePath,
        name: &str,
        expected: &ReadDocument,
        bytes: &[u8],
    ) -> Result<(), DocumentServiceError> {
        let transaction_id = Uuid::new_v4();
        let stage_name = random_stage_name()?;
        let intended_revision = revision_for_bytes(bytes)?;
        let intent = DocumentRecoveryIntent {
            transaction_id,
            source: None,
            target: target.clone(),
            stage_name: Some(stage_name.clone()),
            kind: DocumentKind::File,
            previous_revision: Some(expected.revision.clone()),
            intended_revision: intended_revision.clone(),
        };
        self.recovery
            .prepare(&intent)
            .map_err(|_| DocumentServiceError::recovery_required())?;
        let mut staged = match stage_named(directory, &stage_name, bytes) {
            Ok(staged) => staged,
            Err(error) => {
                let _clear_result = self.recovery.clear(transaction_id);
                return Err(error);
            }
        };
        verify_staged_identity(directory, &stage_name, &mut staged, &intended_revision)?;
        if self
            .atomic_install
            .install(AtomicInstallRequest {
                directory,
                target,
                stage_name: &stage_name,
                target_name: name,
                mode: AtomicInstallMode::ReplaceExisting,
                expected_stage: PinnedInstallSource::File(&staged.file),
                expected_target: Some(&expected.file),
                expected_revision: Some(&expected.revision),
            })
            .is_err()
        {
            if let Ok(current) = revision_in_directory(directory, name) {
                if current != expected.revision
                    && current != intended_revision
                    && verify_staged_identity(
                        directory,
                        &stage_name,
                        &mut staged,
                        &intended_revision,
                    )
                    .is_ok()
                {
                    drop(staged);
                    let _remove_result = directory.remove_file(&stage_name);
                    let _sync_result = sync_dir(directory);
                    let _clear_result = self.recovery.clear(transaction_id);
                    return Err(DocumentServiceError::revision_conflict(current));
                }
            }
            return Err(DocumentServiceError::recovery_required());
        }
        sync_dir(directory).map_err(|_| DocumentServiceError::recovery_required())?;
        verify_installed_identity(directory, name, &mut staged, &intended_revision)?;
        self.recovery
            .complete(transaction_id)
            .map_err(|_| DocumentServiceError::recovery_required())
    }

    #[allow(clippy::too_many_arguments)]
    fn move_recoverable(
        &self,
        source_parent: &Dir,
        source_name: &str,
        source: &WorkspaceRelativePath,
        target_parent: &Dir,
        target_name: &str,
        target: &WorkspaceRelativePath,
        kind: DocumentKind,
        expected_source: PinnedMoveSource<'_>,
        revision: &Revision,
    ) -> Result<(), DocumentServiceError> {
        let transaction_id = Uuid::new_v4();
        let intent = DocumentRecoveryIntent {
            transaction_id,
            source: Some(source.clone()),
            target: target.clone(),
            stage_name: None,
            kind,
            previous_revision: None,
            intended_revision: revision.clone(),
        };
        self.recovery
            .prepare(&intent)
            .map_err(|_| DocumentServiceError::recovery_required())?;
        match self.move_install.install(MoveInstallRequest {
            source_directory: source_parent,
            source_name,
            target_directory: target_parent,
            target_name,
            kind,
            expected_source,
            expected_revision: revision,
        }) {
            Ok(()) => {}
            Err(MoveInstallPortError::AlreadyExists) => {
                self.recovery
                    .clear(transaction_id)
                    .map_err(|_| DocumentServiceError::recovery_required())?;
                return Err(DocumentServiceError::already_exists());
            }
            Err(MoveInstallPortError::RevisionConflict(current)) => {
                self.recovery
                    .clear(transaction_id)
                    .map_err(|_| DocumentServiceError::recovery_required())?;
                return Err(DocumentServiceError::revision_conflict(current));
            }
            Err(MoveInstallPortError::UnavailableNoMutation) => {
                self.recovery
                    .clear(transaction_id)
                    .map_err(|_| DocumentServiceError::recovery_required())?;
                return Err(DocumentServiceError::unavailable());
            }
            Err(MoveInstallPortError::RecoveryRequired) => {
                let source_revision =
                    optional_revision_in_directory_for_kind(source_parent, source_name, kind)?;
                let target_revision =
                    optional_revision_in_directory_for_kind(target_parent, target_name, kind)?;
                if target_revision.is_none() {
                    if let Some(current) = source_revision {
                        if &current != revision {
                            self.recovery
                                .clear(transaction_id)
                                .map_err(|_| DocumentServiceError::recovery_required())?;
                            return Err(DocumentServiceError::revision_conflict(current));
                        }
                    }
                }
                return Err(DocumentServiceError::recovery_required());
            }
        }
        sync_dir(source_parent).map_err(|_| DocumentServiceError::recovery_required())?;
        sync_dir(target_parent).map_err(|_| DocumentServiceError::recovery_required())?;
        self.history
            .relocate(source, target, kind)
            .map_err(|_| DocumentServiceError::recovery_required())?;
        self.recovery
            .complete(transaction_id)
            .map_err(|_| DocumentServiceError::recovery_required())
    }

    fn recover_intent(
        &self,
        root: &Dir,
        intent: &DocumentRecoveryIntent,
    ) -> Result<DocumentRecoveryOutcome, DocumentServiceError> {
        intent
            .validate()
            .map_err(|_| DocumentServiceError::recovery_required())?;
        let target_revision = optional_revision(root, &intent.target)?;
        let source_revision = intent
            .source
            .as_ref()
            .map(|source| optional_revision(root, source))
            .transpose()?
            .flatten();
        let finalized = target_revision.as_ref() == Some(&intent.intended_revision)
            && intent
                .source
                .as_ref()
                .is_none_or(|_| source_revision.is_none());
        let rolled_back = if intent.source.is_some() {
            source_revision.as_ref() == Some(&intent.intended_revision) && target_revision.is_none()
        } else {
            target_revision == intent.previous_revision
        };
        let outcome = if finalized {
            DocumentRecoveryOutcome::Finalized
        } else if rolled_back {
            DocumentRecoveryOutcome::RolledBack
        } else {
            return Err(DocumentServiceError::recovery_required());
        };
        if outcome == DocumentRecoveryOutcome::Finalized {
            if let Some(source) = intent.source.as_ref() {
                self.history
                    .relocate(source, &intent.target, intent.kind)
                    .map_err(|_| DocumentServiceError::recovery_required())?;
            }
        }
        if let Some(stage_name) = intent.stage_name.as_ref() {
            let (parent, _) = open_parent(root, &intent.target)?;
            let mut removed_any = false;
            let retired_name = format!("{stage_name}.retired");
            match outcome {
                DocumentRecoveryOutcome::Finalized => {
                    if let Some(previous_revision) = intent.previous_revision.as_ref() {
                        removed_any |= remove_recovery_artifact_if_expected(
                            &parent,
                            stage_name,
                            intent.kind,
                            previous_revision,
                        )?;
                        removed_any |= remove_recovery_artifact_if_expected(
                            &parent,
                            &retired_name,
                            DocumentKind::File,
                            previous_revision,
                        )?;
                    } else {
                        ensure_recovery_artifact_absent(&parent, stage_name)?;
                        ensure_recovery_artifact_absent(&parent, &retired_name)?;
                    }
                }
                DocumentRecoveryOutcome::RolledBack => {
                    removed_any |= remove_recovery_artifact_if_expected(
                        &parent,
                        stage_name,
                        intent.kind,
                        &intent.intended_revision,
                    )?;
                    ensure_recovery_artifact_absent(&parent, &retired_name)?;
                }
            }
            if removed_any {
                sync_dir(&parent)?;
            }
        }
        self.recovery
            .clear(intent.transaction_id)
            .map_err(|_| DocumentServiceError::recovery_required())?;
        Ok(outcome)
    }

    fn runtime(&self) -> Result<Arc<KernelRuntime>, DocumentServiceError> {
        self.runtime
            .upgrade()
            .ok_or_else(DocumentServiceError::unavailable)
    }

    fn context(&self) -> Result<DocumentContext, DocumentServiceError> {
        let runtime = self.runtime()?;
        self.context_with_runtime(runtime)
    }

    fn context_with_runtime(
        &self,
        runtime: Arc<KernelRuntime>,
    ) -> Result<DocumentContext, DocumentServiceError> {
        runtime
            .verify_instance_lock()
            .map_err(|_| DocumentServiceError::unavailable())?;
        let snapshot = runtime
            .active_workspace_snapshot()
            .map_err(|_| DocumentServiceError::unavailable())?;
        if snapshot.workspace().readiness != WorkspaceReadiness::Ready {
            return Err(DocumentServiceError::unavailable());
        }
        let root = snapshot
            .authority()
            .root()
            .try_clone_dir()
            .map_err(|_| DocumentServiceError::unavailable())?;
        Ok(DocumentContext {
            runtime,
            snapshot,
            root,
        })
    }

    fn verify_generation(
        &self,
        workspace: &WorkspaceDto,
        expected: &crate::contract::WorkspaceGeneration,
    ) -> Result<(), DocumentServiceError> {
        if &workspace.generation != expected {
            return Err(DocumentServiceError::revision_conflict(
                workspace.revision.clone(),
            ));
        }
        Ok(())
    }

    fn verify_id(
        &self,
        context: &DocumentContext,
        id: &DocumentId,
        kind: DocumentKind,
    ) -> Result<WorkspaceRelativePath, DocumentServiceError> {
        DocumentIdentityCodec::new(context.runtime.wire_identity_key())
            .verify(id, context.workspace(), kind)
            .map_err(|_| DocumentServiceError::not_found())
    }

    fn verify_any_id(
        &self,
        context: &DocumentContext,
        id: &DocumentId,
    ) -> Result<(DocumentKind, WorkspaceRelativePath), DocumentServiceError> {
        match self.verify_id(context, id, DocumentKind::File) {
            Ok(path) => Ok((DocumentKind::File, path)),
            Err(_) => self
                .verify_id(context, id, DocumentKind::Directory)
                .map(|path| (DocumentKind::Directory, path)),
        }
    }

    fn entry(
        &self,
        context: &DocumentContext,
        path: WorkspaceRelativePath,
        kind: DocumentKind,
        metadata: Metadata,
    ) -> Result<DocumentEntryDto, DocumentServiceError> {
        if metadata.file_type().is_symlink() {
            return Err(DocumentServiceError::unsafe_target());
        }
        if kind == DocumentKind::File {
            let read = read_file(&context.root, &path)?;
            return self.file_entry(context, path, &read);
        }
        let snapshot = read_directory(&context.root, &path)?;
        self.entry_from_snapshot(context, path, kind, &snapshot.metadata, snapshot.revision)
    }

    fn file_entry(
        &self,
        context: &DocumentContext,
        path: WorkspaceRelativePath,
        read: &ReadDocument,
    ) -> Result<DocumentEntryDto, DocumentServiceError> {
        self.entry_from_snapshot(
            context,
            path,
            DocumentKind::File,
            &read.metadata,
            read.revision.clone(),
        )
    }

    fn entry_from_snapshot(
        &self,
        context: &DocumentContext,
        path: WorkspaceRelativePath,
        kind: DocumentKind,
        metadata: &Metadata,
        revision: Revision,
    ) -> Result<DocumentEntryDto, DocumentServiceError> {
        let name = path
            .as_str()
            .rsplit('/')
            .next()
            .ok_or_else(DocumentServiceError::invalid_name)?;
        let parent = parent_relative(&path)?;
        let name = crate::contract::DocumentName::parse(name)
            .map_err(|_| DocumentServiceError::invalid_name())?;
        name.validate_kind(kind)
            .map_err(|_| DocumentServiceError::invalid_name())?;
        Ok(DocumentEntryDto {
            id: DocumentIdentityCodec::new(context.runtime.wire_identity_key())
                .issue(context.workspace(), kind, &path)
                .map_err(|_| DocumentServiceError::unavailable())?,
            path,
            parent,
            name,
            kind,
            size_bytes: SafeUnsignedInteger::new(if kind == DocumentKind::File {
                metadata.len()
            } else {
                0
            })
            .map_err(|_| DocumentServiceError::too_large())?,
            modified_at: modified_utc(metadata)?,
            revision,
        })
    }

    fn read_document(
        &self,
        context: &DocumentContext,
        id: &DocumentId,
    ) -> Result<DocumentContentDto, DocumentServiceError> {
        let path = self.verify_id(context, id, DocumentKind::File)?;
        let read = read_file(&context.root, &path)?;
        let entry = self.file_entry(context, path, &read)?;
        let contents =
            String::from_utf8(read.bytes).map_err(|_| DocumentServiceError::invalid_encoding())?;
        Ok(DocumentContentDto {
            id: entry.id,
            path: entry.path,
            parent: entry.parent,
            name: FileDocumentName::parse(entry.name.as_str())
                .map_err(|_| DocumentServiceError::invalid_name())?,
            kind: FileDocumentKind::File,
            size_bytes: entry.size_bytes,
            modified_at: entry.modified_at,
            revision: entry.revision,
            contents: DocumentContents::parse(contents)
                .map_err(|_| DocumentServiceError::too_large())?,
        })
    }

    fn publish(
        &self,
        runtime: &KernelRuntime,
        entry: &DocumentEntryDto,
        event: DocumentEvent,
        previous_path: Option<WorkspaceRelativePath>,
    ) {
        let domain = match event {
            DocumentEvent::Created => crate::contract::DomainEvent::DocumentCreated {
                document: entry.clone(),
            },
            DocumentEvent::Changed => crate::contract::DomainEvent::DocumentChanged {
                document: entry.clone(),
            },
            DocumentEvent::Moved => crate::contract::DomainEvent::DocumentMoved {
                document: entry.clone(),
                previous_path: previous_path.expect("move publication has previous path"),
            },
        };
        let publication = EventPublication {
            resource: ResourceRefDto::Document {
                id: entry.id.clone(),
            },
            revision: entry.revision.clone(),
            event: domain,
        };
        let _publication_result = runtime.publish(&publication);
    }

    fn publish_content(
        &self,
        runtime: &KernelRuntime,
        content: &DocumentContentDto,
        event: DocumentEvent,
        previous_path: Option<WorkspaceRelativePath>,
    ) {
        let entry = DocumentEntryDto {
            id: content.id.clone(),
            path: content.path.clone(),
            parent: content.parent.clone(),
            name: crate::contract::DocumentName::parse(content.name.as_str())
                .expect("file name is a document name"),
            kind: DocumentKind::File,
            size_bytes: content.size_bytes,
            modified_at: content.modified_at.clone(),
            revision: content.revision.clone(),
        };
        self.publish(runtime, &entry, event, previous_path);
    }
}

fn ensure_recovery_artifact_absent(
    directory: &Dir,
    name: &str,
) -> Result<(), DocumentServiceError> {
    match directory.symlink_metadata(name) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        _ => Err(DocumentServiceError::recovery_required()),
    }
}

fn remove_recovery_artifact_if_expected(
    directory: &Dir,
    name: &str,
    kind: DocumentKind,
    expected_revision: &Revision,
) -> Result<bool, DocumentServiceError> {
    let named = match directory.symlink_metadata(name) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err(DocumentServiceError::recovery_required()),
    };
    if named.file_type().is_symlink() {
        return Err(DocumentServiceError::recovery_required());
    }
    match kind {
        DocumentKind::File => {
            if !trusted_metadata(&named) {
                return Err(DocumentServiceError::recovery_required());
            }
            let mut options = OpenOptions::new();
            options.read(true).follow(FollowSymlinks::No);
            let retained = directory
                .open_with(name, &options)
                .map_err(|_| DocumentServiceError::recovery_required())?;
            let retained_metadata = retained
                .metadata()
                .map_err(|_| DocumentServiceError::recovery_required())?;
            if !trusted_metadata(&retained_metadata) || !same_file(&named, &retained_metadata) {
                return Err(DocumentServiceError::recovery_required());
            }
            let actual = revision_for_retained_file(&retained)
                .map_err(|_| DocumentServiceError::recovery_required())?;
            if &actual != expected_revision {
                return Err(DocumentServiceError::recovery_required());
            }
            verify_named_retained_identity(directory, name, &retained)
                .map_err(|_| DocumentServiceError::recovery_required())?;
            directory
                .remove_file(name)
                .map_err(|_| DocumentServiceError::recovery_required())?;
        }
        DocumentKind::Directory => {
            if !named.is_dir() {
                return Err(DocumentServiceError::recovery_required());
            }
            let retained = directory
                .open_dir_nofollow(name)
                .map_err(|_| DocumentServiceError::recovery_required())?;
            let retained_metadata = retained
                .dir_metadata()
                .map_err(|_| DocumentServiceError::recovery_required())?;
            if !retained_metadata.is_dir()
                || retained_metadata.file_type().is_symlink()
                || !same_file(&named, &retained_metadata)
            {
                return Err(DocumentServiceError::recovery_required());
            }
            let actual = directory_revision_for_capability(&retained)
                .map_err(|_| DocumentServiceError::recovery_required())?;
            if &actual != expected_revision {
                return Err(DocumentServiceError::recovery_required());
            }
            let latest = directory
                .symlink_metadata(name)
                .map_err(|_| DocumentServiceError::recovery_required())?;
            if !latest.is_dir()
                || latest.file_type().is_symlink()
                || !same_file(&latest, &retained_metadata)
            {
                return Err(DocumentServiceError::recovery_required());
            }
            directory
                .remove_dir(name)
                .map_err(|_| DocumentServiceError::recovery_required())?;
        }
    }
    Ok(true)
}

struct DocumentContext {
    runtime: Arc<KernelRuntime>,
    snapshot: Arc<ActiveWorkspaceSnapshot>,
    root: Dir,
}

impl DocumentContext {
    fn workspace(&self) -> &WorkspaceDto {
        self.snapshot.workspace()
    }
}
enum DocumentEvent {
    Created,
    Changed,
    Moved,
}
struct ReadDocument {
    bytes: Vec<u8>,
    file: File,
    metadata: Metadata,
    revision: Revision,
}
struct ReadDirectory {
    directory: Dir,
    metadata: Metadata,
    revision: Revision,
}

enum MoveSourceSnapshot {
    File(ReadDocument),
    Directory(ReadDirectory),
}

impl MoveSourceSnapshot {
    fn revision(&self) -> &Revision {
        match self {
            Self::File(read) => &read.revision,
            Self::Directory(read) => &read.revision,
        }
    }

    fn pinned(&self) -> PinnedMoveSource<'_> {
        match self {
            Self::File(read) => PinnedMoveSource::File(&read.file),
            Self::Directory(read) => PinnedMoveSource::Directory(&read.directory),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DocumentServiceErrorKind {
    InvalidName,
    NotFound,
    AlreadyExists,
    TooLarge,
    InvalidEncoding,
    RevisionConflict,
    UnsafeTarget,
    Unavailable,
    HistoryUnavailable,
    DeletionUnavailable,
    InvalidCursor,
    RecoveryRequired,
}

#[derive(Clone, Eq, PartialEq)]
pub struct DocumentServiceError {
    kind: DocumentServiceErrorKind,
    current_revision: Option<Revision>,
}

impl DocumentServiceError {
    pub const fn kind(&self) -> DocumentServiceErrorKind {
        self.kind
    }
    pub fn code(&self) -> ErrorCode {
        error_code(self.kind)
    }
    pub const fn current_revision(&self) -> Option<&Revision> {
        self.current_revision.as_ref()
    }
    fn new(kind: DocumentServiceErrorKind) -> Self {
        Self {
            kind,
            current_revision: None,
        }
    }
    fn invalid_name() -> Self {
        Self::new(DocumentServiceErrorKind::InvalidName)
    }
    fn not_found() -> Self {
        Self::new(DocumentServiceErrorKind::NotFound)
    }
    fn already_exists() -> Self {
        Self::new(DocumentServiceErrorKind::AlreadyExists)
    }
    fn too_large() -> Self {
        Self::new(DocumentServiceErrorKind::TooLarge)
    }
    fn invalid_encoding() -> Self {
        Self::new(DocumentServiceErrorKind::InvalidEncoding)
    }
    fn revision_conflict(revision: Revision) -> Self {
        Self {
            kind: DocumentServiceErrorKind::RevisionConflict,
            current_revision: Some(revision),
        }
    }
    fn unsafe_target() -> Self {
        Self::new(DocumentServiceErrorKind::UnsafeTarget)
    }
    fn unavailable() -> Self {
        Self::new(DocumentServiceErrorKind::Unavailable)
    }
    fn history_unavailable() -> Self {
        Self::new(DocumentServiceErrorKind::HistoryUnavailable)
    }
    fn deletion_unavailable() -> Self {
        Self::new(DocumentServiceErrorKind::DeletionUnavailable)
    }
    fn invalid_cursor() -> Self {
        Self::new(DocumentServiceErrorKind::InvalidCursor)
    }
    fn recovery_required() -> Self {
        Self::new(DocumentServiceErrorKind::RecoveryRequired)
    }
}

impl fmt::Debug for DocumentServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DocumentServiceError")
            .field("kind", &self.kind)
            .field("current_revision", &self.current_revision)
            .finish()
    }
}
impl fmt::Display for DocumentServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("the document operation failed")
    }
}
impl std::error::Error for DocumentServiceError {}

fn error_code(kind: DocumentServiceErrorKind) -> ErrorCode {
    match kind {
        DocumentServiceErrorKind::InvalidName => ErrorCode::InvalidDocumentName,
        DocumentServiceErrorKind::NotFound => ErrorCode::DocumentNotFound,
        DocumentServiceErrorKind::AlreadyExists => ErrorCode::DocumentAlreadyExists,
        DocumentServiceErrorKind::TooLarge => ErrorCode::DocumentTooLarge,
        DocumentServiceErrorKind::InvalidEncoding => ErrorCode::DocumentInvalidEncoding,
        DocumentServiceErrorKind::RevisionConflict => ErrorCode::RevisionConflict,
        DocumentServiceErrorKind::UnsafeTarget | DocumentServiceErrorKind::InvalidCursor => {
            ErrorCode::InvalidRequest
        }
        DocumentServiceErrorKind::Unavailable
        | DocumentServiceErrorKind::HistoryUnavailable
        | DocumentServiceErrorKind::DeletionUnavailable
        | DocumentServiceErrorKind::RecoveryRequired => ErrorCode::WorkspaceUnavailable,
    }
}

fn service_failure(error: DocumentServiceError) -> ServiceFailure {
    let details =
        error
            .current_revision
            .clone()
            .map(|current_revision| ErrorDetails::RevisionConflict {
                current_revision: Some(current_revision),
            });
    ServiceFailure::new(error.code(), details).expect("document errors use compatible details")
}

#[async_trait]
impl DocumentsApiService for WorkspaceDocumentService {
    async fn list_documents(
        &self,
        query: ListDocumentsQuery,
    ) -> Result<crate::contract::DocumentPageDto, ServiceFailure> {
        WorkspaceDocumentService::list_documents(self, query)
            .await
            .map_err(service_failure)
    }
    async fn create_document(
        &self,
        request: CreateDocumentRequest,
    ) -> Result<CreatedDocumentDto, ServiceFailure> {
        WorkspaceDocumentService::create_document(self, request)
            .await
            .map_err(service_failure)
    }
    async fn get_document(
        &self,
        document_id: DocumentId,
    ) -> Result<DocumentContentDto, ServiceFailure> {
        WorkspaceDocumentService::get_document(self, document_id)
            .await
            .map_err(service_failure)
    }
    async fn update_document(
        &self,
        document_id: DocumentId,
        request: UpdateDocumentRequest,
    ) -> Result<DocumentContentDto, ServiceFailure> {
        WorkspaceDocumentService::update_document(self, document_id, request)
            .await
            .map_err(service_failure)
    }
    async fn move_document(
        &self,
        document_id: DocumentId,
        request: MoveDocumentRequest,
    ) -> Result<DocumentEntryDto, ServiceFailure> {
        WorkspaceDocumentService::move_document(self, document_id, request)
            .await
            .map_err(service_failure)
    }
    async fn delete_document(
        &self,
        document_id: DocumentId,
        request: DeleteDocumentRequest,
    ) -> Result<(), ServiceFailure> {
        WorkspaceDocumentService::delete_document(self, document_id, request)
            .await
            .map_err(service_failure)
    }
    async fn list_document_history(
        &self,
        document_id: DocumentId,
        query: PageQuery,
    ) -> Result<DocumentHistoryPageDto, ServiceFailure> {
        WorkspaceDocumentService::list_document_history(self, document_id, query)
            .await
            .map_err(service_failure)
    }
    async fn restore_document_history(
        &self,
        document_id: DocumentId,
        snapshot_id: SnapshotId,
        request: crate::contract::RestoreDocumentHistoryRequest,
    ) -> Result<DocumentContentDto, ServiceFailure> {
        WorkspaceDocumentService::restore_document_history(self, document_id, snapshot_id, request)
            .await
            .map_err(service_failure)
    }
    async fn search_workspace(
        &self,
        query: SearchWorkspaceQuery,
    ) -> Result<SearchPageDto, ServiceFailure> {
        WorkspaceDocumentService::search_workspace(self, query)
            .await
            .map_err(service_failure)
    }
}

fn open_directory(root: &Dir, path: &WorkspaceRelativePath) -> Result<Dir, DocumentServiceError> {
    let mut directory = root
        .try_clone()
        .map_err(|_| DocumentServiceError::unavailable())?;
    if path.as_str().is_empty() {
        return Ok(directory);
    }
    for component in path.as_str().split('/') {
        directory = directory
            .open_dir_nofollow(component)
            .map_err(|_| DocumentServiceError::unsafe_target())?;
    }
    Ok(directory)
}

fn open_parent(
    root: &Dir,
    path: &WorkspaceRelativePath,
) -> Result<(Dir, String), DocumentServiceError> {
    let (parent, name) = path
        .as_str()
        .rsplit_once('/')
        .map_or(("", path.as_str()), |(parent, name)| (parent, name));
    Ok((
        open_directory(
            root,
            &WorkspaceRelativePath::parse(parent)
                .map_err(|_| DocumentServiceError::unsafe_target())?,
        )?,
        name.to_string(),
    ))
}

fn parent_relative(
    path: &WorkspaceRelativePath,
) -> Result<WorkspaceRelativePath, DocumentServiceError> {
    WorkspaceRelativePath::parse(
        path.as_str()
            .rsplit_once('/')
            .map_or("", |(parent, _)| parent),
    )
    .map_err(|_| DocumentServiceError::unsafe_target())
}

fn join_relative(
    parent: &WorkspaceRelativePath,
    name: &str,
) -> Result<WorkspaceRelativePath, DocumentServiceError> {
    WorkspaceRelativePath::parse(if parent.as_str().is_empty() {
        name.to_string()
    } else {
        format!("{}/{name}", parent.as_str())
    })
    .map_err(|_| DocumentServiceError::unsafe_target())
}

fn metadata_at(root: &Dir, path: &WorkspaceRelativePath) -> Result<Metadata, DocumentServiceError> {
    let (parent, name) = open_parent(root, path)?;
    let metadata = parent.symlink_metadata(name).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            DocumentServiceError::not_found()
        } else {
            DocumentServiceError::unavailable()
        }
    })?;
    if metadata.file_type().is_symlink() {
        return Err(DocumentServiceError::unsafe_target());
    }
    Ok(metadata)
}

fn trusted_metadata(metadata: &Metadata) -> bool {
    metadata.is_file() && !metadata.file_type().is_symlink() && link_count(metadata) == 1
}

#[cfg(unix)]
fn link_count(metadata: &Metadata) -> u64 {
    MetadataExt::nlink(metadata)
}
#[cfg(windows)]
fn link_count(metadata: &Metadata) -> u64 {
    MetadataExt::nlink(metadata)
}
#[cfg(not(any(unix, windows)))]
fn link_count(_metadata: &Metadata) -> u64 {
    1
}

fn read_file(
    root: &Dir,
    path: &WorkspaceRelativePath,
) -> Result<ReadDocument, DocumentServiceError> {
    for _ in 0..4 {
        if let Some(snapshot) = try_read_stable_file(root, path)? {
            return Ok(snapshot);
        }
    }
    Err(DocumentServiceError::unavailable())
}

fn try_read_stable_file(
    root: &Dir,
    path: &WorkspaceRelativePath,
) -> Result<Option<ReadDocument>, DocumentServiceError> {
    let (parent, name) = open_parent(root, path)?;
    let metadata = parent.symlink_metadata(&name).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            DocumentServiceError::not_found()
        } else {
            DocumentServiceError::unavailable()
        }
    })?;
    if !trusted_metadata(&metadata) {
        return Err(DocumentServiceError::unsafe_target());
    }
    if metadata.len() > DocumentContents::maximum_bytes() as u64 {
        return Err(DocumentServiceError::too_large());
    }
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    #[cfg(windows)]
    options.share_mode(windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ);
    let mut file = parent
        .open_with(&name, &options)
        .map_err(|_| DocumentServiceError::unsafe_target())?;
    let before = file
        .metadata()
        .map_err(|_| DocumentServiceError::unavailable())?;
    if !trusted_metadata(&before) || !same_file(&metadata, &before) {
        return Err(DocumentServiceError::unsafe_target());
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    read_to_end_bounded(&mut file, &mut bytes, DocumentContents::maximum_bytes())
        .map_err(|_| DocumentServiceError::unavailable())?;
    if bytes.len() > DocumentContents::maximum_bytes() {
        return Err(DocumentServiceError::too_large());
    }
    let after = file
        .metadata()
        .map_err(|_| DocumentServiceError::unavailable())?;
    if !trusted_metadata(&after)
        || !same_file(&before, &after)
        || before.len() != after.len()
        || after.len() != bytes.len() as u64
        || before.modified().ok() != after.modified().ok()
    {
        return Ok(None);
    }
    let revision = Revision::parse(format!("{:x}", Sha256::digest(&bytes)))
        .map_err(|_| DocumentServiceError::unavailable())?;
    Ok(Some(ReadDocument {
        bytes,
        file,
        metadata: after,
        revision,
    }))
}

fn read_directory(
    root: &Dir,
    path: &WorkspaceRelativePath,
) -> Result<ReadDirectory, DocumentServiceError> {
    let directory = open_directory(root, path)?;
    let metadata = directory
        .dir_metadata()
        .map_err(|_| DocumentServiceError::unavailable())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(DocumentServiceError::unsafe_target());
    }
    let revision = directory_revision_for_capability(&directory)?;
    Ok(ReadDirectory {
        directory,
        metadata,
        revision,
    })
}

fn same_file(left: &Metadata, right: &Metadata) -> bool {
    MetadataExt::dev(left) == MetadataExt::dev(right)
        && MetadataExt::ino(left) == MetadataExt::ino(right)
}

fn revision_for_metadata_and_contents(
    root: &Dir,
    path: &WorkspaceRelativePath,
    metadata: &Metadata,
) -> Result<Revision, DocumentServiceError> {
    if metadata.is_file() {
        read_file(root, path).map(|read| read.revision)
    } else {
        directory_revision_at(root, path)
    }
}

fn directory_revision_at(
    root: &Dir,
    path: &WorkspaceRelativePath,
) -> Result<Revision, DocumentServiceError> {
    let directory = open_directory(root, path)?;
    directory_revision_for_capability(&directory)
}

/// Computes the same path-independent logical directory revision used by the
/// document service. Desktop capability adapters reuse this for delete parity.
pub fn directory_revision_for_capability(
    directory: &Dir,
) -> Result<Revision, DocumentServiceError> {
    let mut digest = Sha256::new();
    digest.update(b"qingyu-directory-v2\0");
    hash_directory_contents(directory, &mut digest)?;
    Revision::parse(format!("dir:{:x}", digest.finalize()))
        .map_err(|_| DocumentServiceError::unavailable())
}

fn empty_directory_revision() -> Result<Revision, DocumentServiceError> {
    let mut digest = Sha256::new();
    digest.update(b"qingyu-directory-v2\0");
    Revision::parse(format!("dir:{:x}", digest.finalize()))
        .map_err(|_| DocumentServiceError::unavailable())
}

fn hash_directory_contents(
    directory: &Dir,
    digest: &mut Sha256,
) -> Result<(), DocumentServiceError> {
    let mut names = Vec::new();
    for entry in directory
        .entries()
        .map_err(|_| DocumentServiceError::unavailable())?
    {
        let entry = entry.map_err(|_| DocumentServiceError::unavailable())?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !protected_name(&name) {
            names.push(name);
        }
    }
    names.sort();
    for name in names {
        let metadata = directory
            .symlink_metadata(&name)
            .map_err(|_| DocumentServiceError::unsafe_target())?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            digest.update(b"d");
            digest_field(digest, name.as_bytes());
            let child = directory
                .open_dir_nofollow(&name)
                .map_err(|_| DocumentServiceError::unsafe_target())?;
            hash_directory_contents(&child, digest)?;
            digest.update(b"e");
        } else if metadata.is_file() && markdown_name(&name) && trusted_metadata(&metadata) {
            if metadata.len() > DocumentContents::maximum_bytes() as u64 {
                return Err(DocumentServiceError::too_large());
            }
            digest.update(b"f");
            digest_field(digest, name.as_bytes());
            let mut options = OpenOptions::new();
            options.read(true).follow(FollowSymlinks::No);
            let mut file = directory
                .open_with(&name, &options)
                .map_err(|_| DocumentServiceError::unsafe_target())?;
            let retained = file
                .metadata()
                .map_err(|_| DocumentServiceError::unavailable())?;
            if !trusted_metadata(&retained) || !same_file(&metadata, &retained) {
                return Err(DocumentServiceError::unsafe_target());
            }
            let mut bytes = Vec::with_capacity(metadata.len() as usize);
            read_to_end_bounded(&mut file, &mut bytes, DocumentContents::maximum_bytes())
                .map_err(|_| DocumentServiceError::unavailable())?;
            if bytes.len() > DocumentContents::maximum_bytes() {
                return Err(DocumentServiceError::too_large());
            }
            let after = file
                .metadata()
                .map_err(|_| DocumentServiceError::unavailable())?;
            if !trusted_metadata(&after)
                || !same_file(&retained, &after)
                || after.len() != bytes.len() as u64
                || retained.modified().ok() != after.modified().ok()
            {
                return Err(DocumentServiceError::unsafe_target());
            }
            digest_field(digest, &bytes);
        }
    }
    Ok(())
}

fn digest_field(digest: &mut Sha256, value: &[u8]) {
    digest.update((value.len() as u64).to_be_bytes());
    digest.update(value);
}

fn modified_utc(metadata: &Metadata) -> Result<Rfc3339Utc, DocumentServiceError> {
    system_time_utc(
        metadata
            .modified()
            .map_err(|_| DocumentServiceError::unavailable())?
            .into_std(),
    )
}
fn now_utc() -> Result<Rfc3339Utc, DocumentServiceError> {
    system_time_utc(SystemTime::now())
}
fn system_time_utc(value: SystemTime) -> Result<Rfc3339Utc, DocumentServiceError> {
    let value = OffsetDateTime::from(value)
        .format(&Rfc3339)
        .map_err(|_| DocumentServiceError::unavailable())?;
    Rfc3339Utc::parse(value).map_err(|_| DocumentServiceError::unavailable())
}

fn ensure_absent(directory: &Dir, name: impl AsRef<Path>) -> Result<(), DocumentServiceError> {
    match directory.symlink_metadata(name) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(DocumentServiceError::unsafe_target())
        }
        Ok(_) => Err(DocumentServiceError::already_exists()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(DocumentServiceError::unavailable()),
    }
}
fn map_create_error(error: &io::Error) -> DocumentServiceError {
    if error.kind() == io::ErrorKind::AlreadyExists {
        DocumentServiceError::already_exists()
    } else {
        DocumentServiceError::unavailable()
    }
}
fn protected_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower == ".qingyu"
        || lower == ".markra-sync"
        || lower.starts_with(".qingyu-")
        || lower.starts_with(".markra-sync-stage-")
}
fn markdown_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".md") || lower.ends_with(".markdown")
}

fn random_stage_name() -> Result<String, DocumentServiceError> {
    let mut entropy = [0_u8; 16];
    getrandom::fill(&mut entropy).map_err(|_| DocumentServiceError::unavailable())?;
    Ok(format!(
        "{}{}.tmp",
        crate::documents::DOCUMENT_STAGE_PREFIX,
        entropy
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    ))
}
fn revision_for_bytes(bytes: &[u8]) -> Result<Revision, DocumentServiceError> {
    Revision::parse(format!("{:x}", Sha256::digest(bytes)))
        .map_err(|_| DocumentServiceError::unavailable())
}
struct StagedDocument {
    file: File,
    identity: Metadata,
}

fn stage_named(
    directory: &Dir,
    name: &str,
    bytes: &[u8],
) -> Result<StagedDocument, DocumentServiceError> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create_new(true)
        .follow(FollowSymlinks::No);
    #[cfg(windows)]
    options
        .access_mode(
            windows_sys::Win32::Foundation::GENERIC_READ
                | windows_sys::Win32::Foundation::GENERIC_WRITE
                | windows_sys::Win32::Storage::FileSystem::DELETE,
        )
        .share_mode(windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = directory
        .open_with(name, &options)
        .map_err(|_| DocumentServiceError::unavailable())?;
    if file
        .write_all(bytes)
        .and_then(|()| file.sync_all())
        .is_err()
    {
        drop(file);
        let _cleanup_result = directory.remove_file(name);
        return Err(DocumentServiceError::unavailable());
    }
    let identity = file
        .metadata()
        .map_err(|_| DocumentServiceError::unavailable())?;
    if !trusted_metadata(&identity) {
        return Err(DocumentServiceError::unsafe_target());
    }
    Ok(StagedDocument { file, identity })
}

fn open_staged_directory(directory: &Dir, name: &str) -> Result<Dir, DocumentServiceError> {
    #[cfg(windows)]
    {
        // The retained handle starts guarding the staged directory as soon as
        // the portable create/open sequence permits. DELETE is required for
        // handle-relative publication; share-read-only blocks ordinary
        // deletion or renaming of this directory entry. It does not prevent
        // child mutations, which the revision checks must detect.
        let mut options = OpenOptions::new();
        options
            .read(true)
            .access_mode(
                windows_sys::Win32::Foundation::GENERIC_READ
                    | windows_sys::Win32::Storage::FileSystem::DELETE,
            )
            .share_mode(windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ)
            .custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS)
            .follow(FollowSymlinks::No);
        let file = directory
            .open_with(name, &options)
            .map_err(|_| DocumentServiceError::recovery_required())?;
        Ok(Dir::from_std_file(file.into_std()))
    }
    #[cfg(not(windows))]
    directory
        .open_dir_nofollow(name)
        .map_err(|_| DocumentServiceError::recovery_required())
}

fn verify_staged_identity(
    directory: &Dir,
    name: &str,
    staged: &mut StagedDocument,
    expected: &Revision,
) -> Result<(), DocumentServiceError> {
    verify_named_staged_identity(directory, name, staged, expected)
        .map_err(|_| DocumentServiceError::recovery_required())
}

fn verify_installed_identity(
    directory: &Dir,
    name: &str,
    staged: &mut StagedDocument,
    expected: &Revision,
) -> Result<(), DocumentServiceError> {
    verify_named_staged_identity(directory, name, staged, expected)
        .map_err(|_| DocumentServiceError::recovery_required())
}

fn verify_named_staged_identity(
    directory: &Dir,
    name: &str,
    staged: &mut StagedDocument,
    expected: &Revision,
) -> Result<(), ()> {
    let named = directory.symlink_metadata(name).map_err(|_| ())?;
    let retained = staged.file.metadata().map_err(|_| ())?;
    if !trusted_metadata(&named)
        || !trusted_metadata(&retained)
        || !same_file(&named, &retained)
        || !same_file(&staged.identity, &retained)
    {
        return Err(());
    }
    staged.file.seek(SeekFrom::Start(0)).map_err(|_| ())?;
    let mut bytes = Vec::new();
    read_to_end_bounded(
        &mut staged.file,
        &mut bytes,
        DocumentContents::maximum_bytes(),
    )
    .map_err(|_| ())?;
    if bytes.len() > DocumentContents::maximum_bytes() {
        return Err(());
    }
    let actual = revision_for_bytes(&bytes).map_err(|_| ())?;
    (&actual == expected).then_some(()).ok_or(())
}
fn revision_in_directory(directory: &Dir, name: &str) -> Result<Revision, DocumentServiceError> {
    let metadata = directory.symlink_metadata(name).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            DocumentServiceError::not_found()
        } else {
            DocumentServiceError::unavailable()
        }
    })?;
    if !trusted_metadata(&metadata) {
        return Err(DocumentServiceError::unsafe_target());
    }
    if metadata.len() > DocumentContents::maximum_bytes() as u64 {
        return Err(DocumentServiceError::too_large());
    }
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let mut file = directory
        .open_with(name, &options)
        .map_err(|_| DocumentServiceError::unsafe_target())?;
    let retained = file
        .metadata()
        .map_err(|_| DocumentServiceError::unavailable())?;
    if !trusted_metadata(&retained) || !same_file(&metadata, &retained) {
        return Err(DocumentServiceError::unsafe_target());
    }
    let mut bytes = Vec::new();
    read_to_end_bounded(&mut file, &mut bytes, DocumentContents::maximum_bytes())
        .map_err(|_| DocumentServiceError::unavailable())?;
    if bytes.len() > DocumentContents::maximum_bytes() {
        return Err(DocumentServiceError::too_large());
    }
    revision_for_bytes(&bytes)
}

fn read_to_end_bounded(
    reader: &mut impl io::Read,
    bytes: &mut Vec<u8>,
    maximum_bytes: usize,
) -> io::Result<()> {
    reader
        .take(maximum_bytes as u64 + 1)
        .read_to_end(bytes)
        .map(|_| ())
}
fn revision_in_directory_for_kind(
    directory: &Dir,
    name: &str,
    kind: DocumentKind,
) -> Result<Revision, DocumentServiceError> {
    if kind == DocumentKind::File {
        return revision_in_directory(directory, name);
    }
    let child = directory
        .open_dir_nofollow(name)
        .map_err(|_| DocumentServiceError::not_found())?;
    directory_revision_for_capability(&child)
}
fn optional_revision(
    root: &Dir,
    path: &WorkspaceRelativePath,
) -> Result<Option<Revision>, DocumentServiceError> {
    let (parent, name) = open_parent(root, path)?;
    let metadata = match parent.symlink_metadata(&name) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(DocumentServiceError::recovery_required()),
    };
    if metadata.file_type().is_symlink() {
        return Err(DocumentServiceError::recovery_required());
    }
    if metadata.is_file() {
        revision_in_directory(&parent, &name).map(Some)
    } else if metadata.is_dir() {
        parent
            .open_dir_nofollow(&name)
            .map_err(|_| DocumentServiceError::recovery_required())
            .and_then(|directory| {
                directory_revision_for_capability(&directory)
                    .map(Some)
                    .map_err(|_| DocumentServiceError::recovery_required())
            })
    } else {
        Err(DocumentServiceError::recovery_required())
    }
}

fn optional_revision_in_directory_for_kind(
    directory: &Dir,
    name: &str,
    kind: DocumentKind,
) -> Result<Option<Revision>, DocumentServiceError> {
    match directory.symlink_metadata(name) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink()
                || (kind == DocumentKind::File && !metadata.is_file())
                || (kind == DocumentKind::Directory && !metadata.is_dir())
            {
                return Err(DocumentServiceError::recovery_required());
            }
            revision_in_directory_for_kind(directory, name, kind).map(Some)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(DocumentServiceError::recovery_required()),
    }
}

fn capability_move_install(request: MoveInstallRequest<'_>) -> Result<(), MoveInstallPortError> {
    verify_named_pinned_move_source(
        request.source_directory,
        request.source_name,
        request.kind,
        request.expected_source,
    )?;
    let before = revision_for_pinned_move_source(request.expected_source)?;
    if before != *request.expected_revision {
        return Err(MoveInstallPortError::RevisionConflict(before));
    }
    verify_named_pinned_move_source(
        request.source_directory,
        request.source_name,
        request.kind,
        request.expected_source,
    )?;
    rename_noreplace(
        request.source_directory,
        request.source_name,
        request.target_directory,
        request.target_name,
    )
    .map_err(|error| match error.kind() {
        DocumentServiceErrorKind::AlreadyExists => MoveInstallPortError::AlreadyExists,
        _ => MoveInstallPortError::UnavailableNoMutation,
    })?;

    verify_named_pinned_move_source(
        request.target_directory,
        request.target_name,
        request.kind,
        request.expected_source,
    )?;
    let after = revision_for_pinned_move_source(request.expected_source)?;
    if after != *request.expected_revision {
        verify_named_pinned_move_source(
            request.target_directory,
            request.target_name,
            request.kind,
            request.expected_source,
        )?;
        rename_noreplace(
            request.target_directory,
            request.target_name,
            request.source_directory,
            request.source_name,
        )
        .map_err(|_| MoveInstallPortError::RecoveryRequired)?;
        return Err(MoveInstallPortError::RevisionConflict(after));
    }
    verify_named_pinned_move_source(
        request.target_directory,
        request.target_name,
        request.kind,
        request.expected_source,
    )
}

fn verify_named_pinned_move_source(
    directory: &Dir,
    name: &str,
    kind: DocumentKind,
    expected: PinnedMoveSource<'_>,
) -> Result<(), MoveInstallPortError> {
    let named = directory
        .symlink_metadata(name)
        .map_err(|_| MoveInstallPortError::RecoveryRequired)?;
    let retained = match (kind, expected) {
        (DocumentKind::File, PinnedMoveSource::File(file)) => file
            .metadata()
            .map_err(|_| MoveInstallPortError::RecoveryRequired)?,
        (DocumentKind::Directory, PinnedMoveSource::Directory(directory)) => directory
            .dir_metadata()
            .map_err(|_| MoveInstallPortError::RecoveryRequired)?,
        _ => return Err(MoveInstallPortError::RecoveryRequired),
    };
    let trusted = match kind {
        DocumentKind::File => trusted_metadata(&named) && trusted_metadata(&retained),
        DocumentKind::Directory => {
            named.is_dir()
                && retained.is_dir()
                && !named.file_type().is_symlink()
                && !retained.file_type().is_symlink()
        }
    };
    if !trusted || !same_file(&named, &retained) {
        return Err(MoveInstallPortError::RecoveryRequired);
    }
    Ok(())
}

fn revision_for_pinned_move_source(
    expected: PinnedMoveSource<'_>,
) -> Result<Revision, MoveInstallPortError> {
    match expected {
        PinnedMoveSource::File(file) => {
            revision_for_retained_file(file).map_err(|_| MoveInstallPortError::RecoveryRequired)
        }
        PinnedMoveSource::Directory(directory) => directory_revision_for_capability(directory)
            .map_err(|_| MoveInstallPortError::RecoveryRequired),
    }
}

#[cfg(unix)]
fn capability_atomic_install(
    request: AtomicInstallRequest<'_>,
) -> Result<(), AtomicInstallPortError> {
    capability_atomic_install_with_retired_hook(request, || {})
}

#[cfg(unix)]
fn capability_atomic_install_with_retired_hook(
    request: AtomicInstallRequest<'_>,
    before_retired_verification: impl FnOnce(),
) -> Result<(), AtomicInstallPortError> {
    verify_named_pinned_install_source(
        request.directory,
        request.stage_name,
        request.expected_stage,
    )?;
    match request.mode {
        AtomicInstallMode::CreateNoReplace => {
            before_retired_verification();
            // Unix has no portable rename-by-source-handle operation. This
            // second identity check closes the injectable validation window,
            // while the external-mutator contract remains optimistic.
            verify_named_pinned_install_source(
                request.directory,
                request.stage_name,
                request.expected_stage,
            )?;
            rustix::fs::renameat_with(
                request.directory,
                request.stage_name,
                request.directory,
                request.target_name,
                rustix::fs::RenameFlags::NOREPLACE,
            )
            .map_err(|_| AtomicInstallPortError)
        }
        AtomicInstallMode::ReplaceExisting => {
            let (Some(expected_target), Some(expected_revision)) =
                (request.expected_target, request.expected_revision)
            else {
                return Err(AtomicInstallPortError);
            };
            verify_named_retained_identity(
                request.directory,
                request.target_name,
                expected_target,
            )?;
            if revision_for_retained_file(expected_target)? != *expected_revision {
                return Err(AtomicInstallPortError);
            }
            verify_named_retained_identity(
                request.directory,
                request.target_name,
                expected_target,
            )?;
            rustix::fs::renameat_with(
                request.directory,
                request.stage_name,
                request.directory,
                request.target_name,
                rustix::fs::RenameFlags::EXCHANGE,
            )
            .map_err(|_| AtomicInstallPortError)?;
            before_retired_verification();
            match verify_retired_target(
                request.directory,
                request.stage_name,
                expected_target,
                expected_revision,
            ) {
                Ok(()) => {}
                Err(RetiredTargetVerificationError::RevisionMismatch) => {
                    verify_named_retained_identity(
                        request.directory,
                        request.stage_name,
                        expected_target,
                    )?;
                    rustix::fs::renameat_with(
                        request.directory,
                        request.stage_name,
                        request.directory,
                        request.target_name,
                        rustix::fs::RenameFlags::EXCHANGE,
                    )
                    .map_err(|_| AtomicInstallPortError)?;
                    return Err(AtomicInstallPortError);
                }
                Err(
                    RetiredTargetVerificationError::NamedIdentityLost
                    | RetiredTargetVerificationError::RetainedReadUncertain,
                ) => return Err(AtomicInstallPortError),
            }
            verify_named_retained_identity(request.directory, request.stage_name, expected_target)?;
            request
                .directory
                .remove_file(request.stage_name)
                .map_err(|_| AtomicInstallPortError)
        }
    }
}

fn verify_retired_target(
    directory: &Dir,
    name: &str,
    expected_target: &File,
    expected_revision: &Revision,
) -> Result<(), RetiredTargetVerificationError> {
    verify_named_retained_identity(directory, name, expected_target)
        .map_err(|_| RetiredTargetVerificationError::NamedIdentityLost)?;
    let actual = revision_for_retained_file(expected_target)
        .map_err(|_| RetiredTargetVerificationError::RetainedReadUncertain)?;
    if actual != *expected_revision {
        verify_named_retained_identity(directory, name, expected_target)
            .map_err(|_| RetiredTargetVerificationError::NamedIdentityLost)?;
        return Err(RetiredTargetVerificationError::RevisionMismatch);
    }
    verify_named_retained_identity(directory, name, expected_target)
        .map_err(|_| RetiredTargetVerificationError::NamedIdentityLost)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RetiredTargetVerificationError {
    NamedIdentityLost,
    RevisionMismatch,
    RetainedReadUncertain,
}

fn verify_named_retained_identity(
    directory: &Dir,
    name: &str,
    expected_target: &File,
) -> Result<(), AtomicInstallPortError> {
    let named = directory
        .symlink_metadata(name)
        .map_err(|_| AtomicInstallPortError)?;
    let retained = expected_target
        .metadata()
        .map_err(|_| AtomicInstallPortError)?;
    if !trusted_metadata(&named) || !trusted_metadata(&retained) || !same_file(&named, &retained) {
        return Err(AtomicInstallPortError);
    }
    Ok(())
}

fn revision_for_retained_file(file: &File) -> Result<Revision, AtomicInstallPortError> {
    let mut file = file.try_clone().map_err(|_| AtomicInstallPortError)?;
    let before = file.metadata().map_err(|_| AtomicInstallPortError)?;
    if !trusted_metadata(&before) || before.len() > DocumentContents::maximum_bytes() as u64 {
        return Err(AtomicInstallPortError);
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| AtomicInstallPortError)?;
    let mut bytes = Vec::with_capacity(before.len() as usize);
    (&mut file)
        .take(DocumentContents::maximum_bytes() as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| AtomicInstallPortError)?;
    let after = file.metadata().map_err(|_| AtomicInstallPortError)?;
    if bytes.len() > DocumentContents::maximum_bytes()
        || !trusted_metadata(&after)
        || !same_file(&before, &after)
        || before.len() != after.len()
        || after.len() != bytes.len() as u64
        || before.modified().ok() != after.modified().ok()
    {
        return Err(AtomicInstallPortError);
    }
    revision_for_bytes(&bytes).map_err(|_| AtomicInstallPortError)
}

#[cfg(windows)]
fn capability_atomic_install(
    request: AtomicInstallRequest<'_>,
) -> Result<(), AtomicInstallPortError> {
    capability_atomic_install_with_prepublication_hook(request, || {})
}

#[cfg(windows)]
fn capability_atomic_install_with_prepublication_hook(
    request: AtomicInstallRequest<'_>,
    before_publication: impl FnOnce(),
) -> Result<(), AtomicInstallPortError> {
    verify_named_pinned_install_source(
        request.directory,
        request.stage_name,
        request.expected_stage,
    )?;
    match request.mode {
        AtomicInstallMode::CreateNoReplace => {
            ensure_absent(request.directory, request.target_name)
                .map_err(|_| AtomicInstallPortError)?;
            before_publication();
            match request.expected_stage {
                PinnedInstallSource::File(staged) => windows_rename_retained_entry(
                    staged,
                    request.directory,
                    request.target_name,
                    false,
                ),
                PinnedInstallSource::Directory(staged) => windows_rename_retained_entry(
                    staged,
                    request.directory,
                    request.target_name,
                    false,
                ),
            }
            .map_err(|_| AtomicInstallPortError)
        }
        AtomicInstallMode::ReplaceExisting => {
            let (PinnedInstallSource::File(staged), Some(expected_target), Some(expected_revision)) = (
                request.expected_stage,
                request.expected_target,
                request.expected_revision,
            ) else {
                return Err(AtomicInstallPortError);
            };
            verify_named_retained_identity(
                request.directory,
                request.target_name,
                expected_target,
            )?;
            if revision_for_retained_file(expected_target)? != *expected_revision {
                return Err(AtomicInstallPortError);
            }
            verify_named_retained_identity(
                request.directory,
                request.target_name,
                expected_target,
            )?;
            verify_named_pinned_install_source(
                request.directory,
                request.stage_name,
                request.expected_stage,
            )?;
            before_publication();
            windows_rename_retained_entry(staged, request.directory, request.target_name, true)
                .map_err(|_| AtomicInstallPortError)
        }
    }
}

fn verify_named_pinned_install_source(
    directory: &Dir,
    name: &str,
    expected: PinnedInstallSource<'_>,
) -> Result<(), AtomicInstallPortError> {
    let named = directory
        .symlink_metadata(name)
        .map_err(|_| AtomicInstallPortError)?;
    let retained = match expected {
        PinnedInstallSource::File(file) => file.metadata().map_err(|_| AtomicInstallPortError)?,
        PinnedInstallSource::Directory(directory) => directory
            .dir_metadata()
            .map_err(|_| AtomicInstallPortError)?,
    };
    let trusted = match expected {
        PinnedInstallSource::File(_) => trusted_metadata(&named) && trusted_metadata(&retained),
        PinnedInstallSource::Directory(_) => {
            named.is_dir()
                && retained.is_dir()
                && !named.file_type().is_symlink()
                && !retained.file_type().is_symlink()
        }
    };
    if !trusted || !same_file(&named, &retained) {
        return Err(AtomicInstallPortError);
    }
    Ok(())
}

#[cfg(windows)]
fn windows_rename_retained_entry(
    source: &impl std::os::windows::io::AsRawHandle,
    destination: &Dir,
    destination_name: &str,
    replace: bool,
) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle as _;

    use windows_sys::Win32::{
        Storage::FileSystem::{FileRenameInfoEx, SetFileInformationByHandle, FILE_RENAME_INFO},
        System::WindowsProgramming::{
            FILE_RENAME_FLAG_POSIX_SEMANTICS, FILE_RENAME_FLAG_REPLACE_IF_EXISTS,
        },
    };

    let destination_name = destination_name.encode_utf16().collect::<Vec<_>>();
    if destination_name.is_empty() || destination_name.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "document destination name is invalid",
        ));
    }
    let destination_name_bytes = destination_name
        .len()
        .checked_mul(std::mem::size_of::<u16>())
        .and_then(|length| u32::try_from(length).ok())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "document destination name is too long",
            )
        })?;
    let offset = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
    let buffer_bytes = offset
        .checked_add(destination_name_bytes as usize)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "document rename buffer is too large",
            )
        })?;
    let mut buffer = vec![0usize; buffer_bytes.div_ceil(std::mem::size_of::<usize>())];
    let rename_info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();

    let renamed = unsafe {
        (*rename_info).Anonymous.Flags = if replace {
            // POSIX semantics lets this operation retire the target while the
            // Kernel's read-only guard handle remains open. That same flag can
            // be used by an external actor, so the revision check remains an
            // optimistic conflict boundary rather than a strict OS CAS.
            FILE_RENAME_FLAG_REPLACE_IF_EXISTS | FILE_RENAME_FLAG_POSIX_SEMANTICS
        } else {
            0
        };
        (*rename_info).RootDirectory = destination.as_raw_handle();
        (*rename_info).FileNameLength = destination_name_bytes;
        std::ptr::copy_nonoverlapping(
            destination_name.as_ptr(),
            buffer.as_mut_ptr().cast::<u8>().add(offset).cast::<u16>(),
            destination_name.len(),
        );
        SetFileInformationByHandle(
            source.as_raw_handle(),
            FileRenameInfoEx,
            rename_info.cast(),
            u32::try_from(buffer_bytes).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "document rename buffer is too large",
                )
            })?,
        )
    };
    if renamed == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(any(unix, windows)))]
fn capability_atomic_install(
    _request: AtomicInstallRequest<'_>,
) -> Result<(), AtomicInstallPortError> {
    Err(AtomicInstallPortError)
}

#[cfg(unix)]
fn rename_noreplace(
    source: &Dir,
    source_name: impl AsRef<Path>,
    target: &Dir,
    target_name: impl AsRef<Path>,
) -> Result<(), DocumentServiceError> {
    rustix::fs::renameat_with(
        source,
        source_name.as_ref(),
        target,
        target_name.as_ref(),
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(|error| {
        if error == rustix::io::Errno::EXIST {
            DocumentServiceError::already_exists()
        } else {
            DocumentServiceError::unavailable()
        }
    })
}
#[cfg(windows)]
fn rename_noreplace(
    source: &Dir,
    source_name: impl AsRef<Path>,
    target: &Dir,
    target_name: impl AsRef<Path>,
) -> Result<(), DocumentServiceError> {
    ensure_absent(target, target_name.as_ref())?;
    source
        .rename(source_name, target, target_name)
        .map_err(|_| DocumentServiceError::unavailable())
}
#[cfg(not(any(unix, windows)))]
fn rename_noreplace(
    _source: &Dir,
    _source_name: impl AsRef<Path>,
    _target: &Dir,
    _target_name: impl AsRef<Path>,
) -> Result<(), DocumentServiceError> {
    Err(DocumentServiceError::unavailable())
}
#[cfg(unix)]
fn sync_dir(directory: &Dir) -> Result<(), DocumentServiceError> {
    rustix::fs::fsync(directory).map_err(|_| DocumentServiceError::unavailable())
}
#[cfg(not(unix))]
fn sync_dir(_directory: &Dir) -> Result<(), DocumentServiceError> {
    Ok(())
}

fn collect_search(
    root: &Dir,
    directory_path: &WorkspaceRelativePath,
    needle: &str,
    context: &DocumentContext,
    service: &WorkspaceDocumentService,
    matches: &mut Vec<SearchMatchDto>,
) -> Result<(), DocumentServiceError> {
    if matches.len() >= MAX_SEARCH_MATCHES {
        return Ok(());
    }
    let directory = open_directory(root, directory_path)?;
    for entry in directory
        .entries()
        .map_err(|_| DocumentServiceError::unavailable())?
    {
        if matches.len() >= MAX_SEARCH_MATCHES {
            break;
        }
        let entry = entry.map_err(|_| DocumentServiceError::unavailable())?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if protected_name(&name) {
            continue;
        }
        let Ok(metadata) = directory.symlink_metadata(entry.file_name()) else {
            continue;
        };
        if metadata.file_type().is_symlink() {
            continue;
        }
        let path = join_relative(directory_path, &name)?;
        if metadata.is_dir() {
            if service.ignore.is_ignored(&path, DocumentKind::Directory) {
                continue;
            }
            let _search_result = collect_search(root, &path, needle, context, service, matches);
        } else if metadata.is_file() && markdown_name(&name) && trusted_metadata(&metadata) {
            if service.ignore.is_ignored(&path, DocumentKind::File) {
                continue;
            }
            let Ok(read) = read_file(root, &path) else {
                continue;
            };
            let Ok(document) = service.file_entry(context, path, &read) else {
                continue;
            };
            let Ok(contents) = String::from_utf8(read.bytes) else {
                continue;
            };
            for (line_index, line) in contents.lines().enumerate() {
                for (byte_index, _) in line.match_indices(needle) {
                    if matches.len() >= MAX_SEARCH_MATCHES {
                        break;
                    }
                    matches.push(SearchMatchDto {
                        document: document.clone(),
                        line: PositiveSafeInteger::new((line_index + 1) as u64)
                            .map_err(|_| DocumentServiceError::unavailable())?,
                        column: PositiveSafeInteger::new(
                            (line[..byte_index].chars().count() + 1) as u64,
                        )
                        .map_err(|_| DocumentServiceError::unavailable())?,
                        preview: bounded_search_preview(line, byte_index, needle),
                    });
                }
            }
        }
    }
    Ok(())
}
fn search_identity(hit: &SearchMatchDto) -> String {
    format!(
        "{}\0{:020}\0{:020}",
        hit.document.path.as_str(),
        hit.line.get(),
        hit.column.get()
    )
}
fn bounded_search_preview(line: &str, match_byte: usize, needle: &str) -> String {
    const CONTEXT: usize = 80;
    let match_char = line[..match_byte].chars().count();
    let match_len = needle.chars().count();
    let chars = line.chars().collect::<Vec<_>>();
    let start = match_char.saturating_sub(CONTEXT);
    let end = match_char
        .saturating_add(match_len)
        .saturating_add(CONTEXT)
        .min(chars.len());
    let mut preview = String::new();
    if start > 0 {
        preview.push('…');
    }
    preview.extend(chars[start..end].iter());
    if end < chars.len() {
        preview.push('…');
    }
    preview
}
fn history_identity(snapshot: &crate::documents::types::HistorySnapshot) -> String {
    format!(
        "{}\0{}",
        snapshot.created_at.as_str(),
        snapshot.snapshot_id.as_uuid()
    )
}

#[cfg(all(test, unix))]
mod atomic_exchange_safety_tests {
    use super::*;

    #[test]
    fn an_unknown_retired_name_is_never_rolled_back_into_the_document_target() {
        let fixture = tempfile::tempdir().unwrap();
        let directory =
            Dir::open_ambient_dir(fixture.path(), cap_std::ambient_authority()).unwrap();
        directory.write("note.md", b"expected old").unwrap();
        directory.write("stage.tmp", b"intended new").unwrap();
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        let expected_target = directory.open_with("note.md", &options).unwrap();
        let expected_stage = directory.open_with("stage.tmp", &options).unwrap();
        let expected_revision = revision_for_bytes(b"expected old").unwrap();
        let target = WorkspaceRelativePath::parse("note.md").unwrap();

        let result = capability_atomic_install_with_retired_hook(
            AtomicInstallRequest {
                directory: &directory,
                target: &target,
                stage_name: "stage.tmp",
                target_name: "note.md",
                mode: AtomicInstallMode::ReplaceExisting,
                expected_stage: PinnedInstallSource::File(&expected_stage),
                expected_target: Some(&expected_target),
                expected_revision: Some(&expected_revision),
            },
            || {
                directory.remove_file("stage.tmp").unwrap();
                directory.write("stage.tmp", b"unknown entry").unwrap();
            },
        );

        assert!(result.is_err());
        assert_eq!(directory.read("note.md").unwrap(), b"intended new");
        assert_eq!(directory.read("stage.tmp").unwrap(), b"unknown entry");
    }

    #[test]
    fn a_named_stage_that_is_not_the_pinned_source_is_never_published() {
        let fixture = tempfile::tempdir().unwrap();
        let directory =
            Dir::open_ambient_dir(fixture.path(), cap_std::ambient_authority()).unwrap();
        directory.write("stage.tmp", b"intended new").unwrap();
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        let expected_stage = directory.open_with("stage.tmp", &options).unwrap();
        directory
            .rename("stage.tmp", &directory, "retained.tmp")
            .unwrap();
        directory.write("stage.tmp", b"attacker").unwrap();
        let target = WorkspaceRelativePath::parse("note.md").unwrap();

        let result = CapabilityAtomicInstallPort.install(AtomicInstallRequest {
            directory: &directory,
            target: &target,
            stage_name: "stage.tmp",
            target_name: "note.md",
            mode: AtomicInstallMode::CreateNoReplace,
            expected_stage: PinnedInstallSource::File(&expected_stage),
            expected_target: None,
            expected_revision: None,
        });

        assert!(result.is_err());
        assert_eq!(directory.read("stage.tmp").unwrap(), b"attacker");
        assert_eq!(directory.read("retained.tmp").unwrap(), b"intended new");
        assert!(directory.symlink_metadata("note.md").is_err());
    }

    #[test]
    fn a_directory_stage_swapped_after_validation_is_never_published() {
        let fixture = tempfile::tempdir().unwrap();
        let directory =
            Dir::open_ambient_dir(fixture.path(), cap_std::ambient_authority()).unwrap();
        directory.create_dir("stage.tmp").unwrap();
        let expected_stage = directory.open_dir_nofollow("stage.tmp").unwrap();
        let target = WorkspaceRelativePath::parse("created").unwrap();

        let result = capability_atomic_install_with_retired_hook(
            AtomicInstallRequest {
                directory: &directory,
                target: &target,
                stage_name: "stage.tmp",
                target_name: "created",
                mode: AtomicInstallMode::CreateNoReplace,
                expected_stage: PinnedInstallSource::Directory(&expected_stage),
                expected_target: None,
                expected_revision: None,
            },
            || {
                directory
                    .rename("stage.tmp", &directory, "retained.tmp")
                    .unwrap();
                directory.create_dir("stage.tmp").unwrap();
                directory
                    .open_dir_nofollow("stage.tmp")
                    .unwrap()
                    .write("attacker", b"replacement")
                    .unwrap();
            },
        );

        assert!(result.is_err());
        assert!(directory.symlink_metadata("created").is_err());
        assert!(directory
            .open_dir_nofollow("retained.tmp")
            .unwrap()
            .entries()
            .unwrap()
            .next()
            .is_none());
    }
}

#[cfg(test)]
mod bounded_read_tests {
    use std::io::{self, Read};

    use super::read_to_end_bounded;

    struct EndlessReader {
        consumed: usize,
    }

    impl Read for EndlessReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            buffer.fill(b'x');
            self.consumed += buffer.len();
            Ok(buffer.len())
        }
    }

    #[test]
    fn bounded_document_reads_never_consume_past_the_sentinel_byte() {
        let mut reader = EndlessReader { consumed: 0 };
        let mut bytes = Vec::new();

        read_to_end_bounded(&mut reader, &mut bytes, 32).unwrap();

        assert_eq!(bytes.len(), 33);
        assert_eq!(reader.consumed, 33);
    }
}

#[cfg(all(test, windows))]
mod windows_atomic_install_tests {
    use super::*;

    #[test]
    fn default_replace_uses_retained_guards_and_preserves_both_file_identities() {
        let fixture = tempfile::tempdir().unwrap();
        std::fs::write(fixture.path().join("note.md"), b"previous").unwrap();
        let directory =
            Dir::open_ambient_dir(fixture.path(), cap_std::ambient_authority()).unwrap();
        let target = WorkspaceRelativePath::parse("note.md").unwrap();
        let expected = read_file(&directory, &target).unwrap();
        let mut staged = stage_named(&directory, "stage.tmp", b"intended").unwrap();

        assert!(std::fs::OpenOptions::new()
            .write(true)
            .open(fixture.path().join("note.md"))
            .is_err());
        assert!(std::fs::remove_file(fixture.path().join("note.md")).is_err());
        assert!(std::fs::OpenOptions::new()
            .write(true)
            .open(fixture.path().join("stage.tmp"))
            .is_err());
        assert!(std::fs::rename(
            fixture.path().join("stage.tmp"),
            fixture.path().join("swapped.tmp")
        )
        .is_err());

        CapabilityAtomicInstallPort
            .install(AtomicInstallRequest {
                directory: &directory,
                target: &target,
                stage_name: "stage.tmp",
                target_name: "note.md",
                mode: AtomicInstallMode::ReplaceExisting,
                expected_stage: PinnedInstallSource::File(&staged.file),
                expected_target: Some(&expected.file),
                expected_revision: Some(&expected.revision),
            })
            .unwrap();

        let intended_revision = revision_for_bytes(b"intended").unwrap();
        verify_installed_identity(&directory, "note.md", &mut staged, &intended_revision).unwrap();
        assert_eq!(
            revision_for_retained_file(&expected.file).unwrap(),
            expected.revision
        );
    }

    #[test]
    fn invalid_handle_rename_fails_closed_without_publishing_elsewhere() {
        let fixture = tempfile::tempdir().unwrap();
        let directory =
            Dir::open_ambient_dir(fixture.path(), cap_std::ambient_authority()).unwrap();
        let mut staged = stage_named(&directory, "stage.tmp", b"intended").unwrap();

        assert!(
            windows_rename_retained_entry(&staged.file, &directory, "invalid\0target", false)
                .is_err()
        );
        verify_staged_identity(
            &directory,
            "stage.tmp",
            &mut staged,
            &revision_for_bytes(b"intended").unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn directory_create_renames_the_retained_handle_after_a_source_name_swap() {
        let fixture = tempfile::tempdir().unwrap();
        let directory =
            Dir::open_ambient_dir(fixture.path(), cap_std::ambient_authority()).unwrap();
        directory.create_dir("stage.tmp").unwrap();
        let staged = open_staged_directory(&directory, "stage.tmp").unwrap();
        let staged_identity = staged.dir_metadata().unwrap();
        let target = WorkspaceRelativePath::parse("created").unwrap();

        assert!(std::fs::rename(
            fixture.path().join("stage.tmp"),
            fixture.path().join("ordinary-swap")
        )
        .is_err());

        capability_atomic_install_with_prepublication_hook(
            AtomicInstallRequest {
                directory: &directory,
                target: &target,
                stage_name: "stage.tmp",
                target_name: "created",
                mode: AtomicInstallMode::CreateNoReplace,
                expected_stage: PinnedInstallSource::Directory(&staged),
                expected_target: None,
                expected_revision: None,
            },
            || {
                windows_rename_retained_entry(&staged, &directory, "retained.tmp", false).unwrap();
                directory.create_dir("stage.tmp").unwrap();
                directory
                    .open_dir_nofollow("stage.tmp")
                    .unwrap()
                    .write("attacker", b"replacement")
                    .unwrap();
            },
        )
        .unwrap();

        assert!(same_file(
            &staged_identity,
            &directory.symlink_metadata("created").unwrap()
        ));
        assert!(staged.entries().unwrap().next().is_none());
        assert_eq!(
            directory
                .open_dir_nofollow("stage.tmp")
                .unwrap()
                .read("attacker")
                .unwrap(),
            b"replacement"
        );
    }
}
