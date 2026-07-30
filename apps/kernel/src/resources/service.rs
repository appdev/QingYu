use std::{
    fmt,
    io::{self, Read as _, Seek as _, SeekFrom},
    sync::{Arc, Weak},
};

use async_trait::async_trait;
use cap_fs_ext::{DirExt, MetadataExt};
use cap_std::fs::{Dir, File, Metadata};
use sha2::{Digest as _, Sha256};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use crate::{
    contract::{
        DocumentEntryDto, DocumentKind, DocumentName, ErrorCode, ListWorkspaceInventoryQuery,
        Nullable, PageCursorContext, ResourceEntryDto, ResourceId, ResourceKind, ResourceName,
        Revision, Rfc3339Utc, SafeUnsignedInteger, WorkspaceDto, WorkspaceInventoryEntryDto,
        WorkspaceInventoryPageDto, WorkspaceReadiness, WorkspaceRelativePath,
    },
    documents::service::directory_revision_for_capability,
    ignore_rules::{WorkspaceIgnorePort, WorkspaceIgnoreSnapshot},
    inventory_snapshot::{
        ContentDigest, FileVersionStamp, InventoryCandidateSnapshot, InventoryCandidateType,
        InventoryModifiedTime, InventorySnapshotBudget, InventorySnapshotLimits,
    },
    runtime::{ActiveWorkspaceSnapshot, KernelRuntime, ResourcesApiService, ServiceFailure},
    storage::nonfollowing_read_options,
};

use super::{policy::protected_resource_component, ResourceServiceError};

const STREAM_BUFFER_BYTES: usize = 64 * 1024;
const MAGIC_BYTES: usize = 12;
const MAX_IMMEDIATE_INVENTORY_CANDIDATES: usize = 50_000;
const MAX_INVENTORY_SNAPSHOT_NODES: u64 = 100_000;
const MAX_INVENTORY_FALLBACK_BYTES: u64 = 512 * 1024 * 1024;
const MAX_INVENTORY_TREE_DEPTH: usize = 128;

#[derive(Clone)]
pub struct WorkspaceResourceService {
    runtime: Weak<KernelRuntime>,
    ignore: Arc<dyn WorkspaceIgnorePort>,
}

impl WorkspaceResourceService {
    pub fn new(runtime: &Arc<KernelRuntime>, ignore: Arc<dyn WorkspaceIgnorePort>) -> Self {
        Self {
            runtime: Arc::downgrade(runtime),
            ignore,
        }
    }

    /// Lists one directory level from the retained active workspace capability.
    /// Documents and directories remain document DTOs; binary entries use the
    /// separate resource identity and DTO contract.
    pub fn list_inventory(
        &self,
        parent: &WorkspaceRelativePath,
    ) -> Result<Vec<WorkspaceInventoryEntry>, ResourceServiceError> {
        let context = self.context()?;
        self.list_inventory_with_context(&context, parent)
    }

    pub fn list_inventory_page(
        &self,
        query: ListWorkspaceInventoryQuery,
    ) -> Result<WorkspaceInventoryPageDto, ResourceServiceError> {
        let context = self.context()?;
        let ignore = self.capture_ignore(&context)?;
        let directory = open_directory(&context.root, &query.parent)?;
        let mut budget = inventory_snapshot_budget();
        let candidates = inventory_candidates(&directory, &query.parent, &ignore, &mut budget)?;
        let snapshots = candidates
            .iter()
            .map(|candidate| &candidate.snapshot)
            .collect::<Vec<_>>();
        let cursor_context = PageCursorContext::new(
            "workspace-inventory",
            query.parent.as_str(),
            &context.workspace().generation,
            &snapshots,
        )
        .map_err(|_| ResourceServiceError::invalid_cursor())?;
        let start = match query.cursor.as_ref() {
            Some(cursor) => {
                let last = context
                    .runtime
                    .wire_identity_key()
                    .verify_page_cursor(cursor, &cursor_context)
                    .map_err(|_| ResourceServiceError::invalid_cursor())?;
                candidates.partition_point(|candidate| candidate.path.as_str() <= last.as_str())
            }
            None => 0,
        };
        let limit = query.limit.map_or(100, |limit| usize::from(limit.get()));
        let end = start.saturating_add(limit).min(candidates.len());
        let mut items = Vec::with_capacity(end.saturating_sub(start));
        for candidate in &candidates[start..end] {
            let entry = inspect_inventory_entry(
                &context,
                &ignore,
                &directory,
                &query.parent,
                &candidate.name,
            )?
            .ok_or_else(ResourceServiceError::unsafe_target)?;
            if entry.path() != &candidate.path {
                return Err(ResourceServiceError::unsafe_target());
            }
            items.push(WorkspaceInventoryEntryDto::from(entry));
        }
        if inventory_candidates(&directory, &query.parent, &ignore, &mut budget)? != candidates {
            return Err(ResourceServiceError::unsafe_target());
        }
        context
            .snapshot
            .authority()
            .verify_held_directory()
            .map_err(|_| ResourceServiceError::unavailable())?;
        let next_cursor = if end < candidates.len() {
            let last = items
                .last()
                .ok_or_else(ResourceServiceError::invalid_cursor)?;
            Nullable::value(
                context
                    .runtime
                    .wire_identity_key()
                    .issue_page_cursor(&cursor_context, last.path().as_str())
                    .map_err(|_| ResourceServiceError::invalid_cursor())?,
            )
        } else {
            Nullable::null()
        };
        Ok(WorkspaceInventoryPageDto { items, next_cursor })
    }

    fn list_inventory_with_context(
        &self,
        context: &ResourceContext,
        parent: &WorkspaceRelativePath,
    ) -> Result<Vec<WorkspaceInventoryEntry>, ResourceServiceError> {
        let ignore = self.capture_ignore(context)?;
        let directory = open_directory(&context.root, parent)?;
        let before = trusted_directory_metadata(&directory)?;
        let names = ordinary_entry_names(&directory)?;
        let mut entries = Vec::with_capacity(names.len());
        for name in &names {
            if let Some(entry) =
                inspect_inventory_entry(context, &ignore, &directory, parent, name)?
            {
                entries.push(entry);
            }
        }
        if ordinary_entry_names(&directory)? != names {
            return Err(ResourceServiceError::unsafe_target());
        }
        let after = trusted_directory_metadata(&directory)?;
        if !same_file(&before, &after) {
            return Err(ResourceServiceError::unsafe_target());
        }
        context
            .snapshot
            .authority()
            .verify_held_directory()
            .map_err(|_| ResourceServiceError::unavailable())?;
        Ok(entries)
    }

    /// Opens a signed resource identity as a retained, synchronous reader.
    /// Future transports must consume the declared length and then call
    /// [`RetainedResource::verify_complete`]; observing `Ok(0)` alone does not
    /// authenticate the completed stream.
    pub fn open_resource(
        &self,
        id: &ResourceId,
        expected_kind: ResourceKind,
    ) -> Result<RetainedResource, ResourceServiceError> {
        let context = self.context()?;
        let ignore = self.capture_ignore(&context)?;
        let path = context
            .runtime
            .wire_identity_key()
            .verify_resource_id(
                id,
                context.workspace().id,
                &context.workspace().generation,
                expected_kind,
            )
            .map_err(|_| ResourceServiceError::not_found())?;
        let (parent_path, name) = parent_and_name(&path)?;
        if protected_resource_component(&name) {
            return Err(ResourceServiceError::invalid_path());
        }
        if ignore.is_ignored(&path, DocumentKind::File) {
            return Err(ResourceServiceError::not_found());
        }
        let resource_name =
            ResourceName::parse(&name).map_err(|_| ResourceServiceError::invalid_path())?;
        let parent = open_directory(&context.root, &parent_path)?;
        let addressed = parent.symlink_metadata(&name).map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                ResourceServiceError::not_found()
            } else {
                ResourceServiceError::unavailable()
            }
        })?;
        let inspected = inspect_regular_file(&parent, &name, &addressed)?;
        if markdown_name(&name) {
            return Err(ResourceServiceError::wrong_kind());
        }
        let classification = classify_resource(&name, &inspected.magic);
        if classification.kind != expected_kind {
            return Err(ResourceServiceError::wrong_kind());
        }
        let entry = ResourceEntryDto {
            id: id.clone(),
            path,
            parent: parent_path,
            name: resource_name,
            kind: classification.kind,
            size_bytes: safe_size(inspected.metadata.len())?,
            modified_at: modified_utc(&inspected.metadata)?,
            revision: inspected.revision,
            media_type: classification.media_type.to_string(),
            previewable: classification.previewable,
        };
        let remaining = inspected.metadata.len();
        Ok(RetainedResource {
            snapshot: context.snapshot,
            parent,
            file: inspected.file,
            entry,
            expected: inspected.metadata,
            remaining,
            stream_digest: Sha256::new(),
            verified_complete: false,
        })
    }

    fn context(&self) -> Result<ResourceContext, ResourceServiceError> {
        let runtime = self
            .runtime
            .upgrade()
            .ok_or_else(ResourceServiceError::unavailable)?;
        runtime
            .verify_instance_lock()
            .map_err(|_| ResourceServiceError::unavailable())?;
        let snapshot = runtime
            .active_workspace_snapshot()
            .map_err(|_| ResourceServiceError::unavailable())?;
        if snapshot.workspace().readiness != WorkspaceReadiness::Ready {
            return Err(ResourceServiceError::unavailable());
        }
        let root = snapshot
            .authority()
            .root()
            .try_clone_dir()
            .map_err(|_| ResourceServiceError::unavailable())?;
        let root_path = snapshot.authority().root().canonical_path().to_path_buf();
        Ok(ResourceContext {
            runtime,
            snapshot,
            root,
            root_path,
        })
    }

    fn capture_ignore(
        &self,
        context: &ResourceContext,
    ) -> Result<WorkspaceIgnoreSnapshot, ResourceServiceError> {
        self.ignore
            .capture(&context.root_path, &context.root)
            .map_err(|_| ResourceServiceError::unavailable())
    }
}

impl fmt::Debug for WorkspaceResourceService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("WorkspaceResourceService { runtime: weak }")
    }
}

fn service_failure(error: ResourceServiceError) -> ServiceFailure {
    let code = match error.kind() {
        super::ResourceServiceErrorKind::InvalidCursor
        | super::ResourceServiceErrorKind::InvalidPath => ErrorCode::InvalidRequest,
        super::ResourceServiceErrorKind::NotFound | super::ResourceServiceErrorKind::WrongKind => {
            ErrorCode::ResourceNotFound
        }
        super::ResourceServiceErrorKind::UnsafeTarget
        | super::ResourceServiceErrorKind::Unavailable => ErrorCode::WorkspaceUnavailable,
    };
    ServiceFailure::new(code, None).expect("resource errors use compatible details")
}

fn unavailable_service_failure() -> ServiceFailure {
    ServiceFailure::new(ErrorCode::WorkspaceUnavailable, None)
        .expect("workspace unavailable accepts no details")
}

#[async_trait]
impl ResourcesApiService for WorkspaceResourceService {
    async fn list_workspace_inventory(
        &self,
        query: ListWorkspaceInventoryQuery,
    ) -> Result<WorkspaceInventoryPageDto, ServiceFailure> {
        let service = self.clone();
        tokio::task::spawn_blocking(move || service.list_inventory_page(query))
            .await
            .map_err(|_| unavailable_service_failure())?
            .map_err(service_failure)
    }

    async fn open_workspace_resource(
        &self,
        resource_id: ResourceId,
        expected_kind: ResourceKind,
    ) -> Result<RetainedResource, ServiceFailure> {
        let service = self.clone();
        tokio::task::spawn_blocking(move || service.open_resource(&resource_id, expected_kind))
            .await
            .map_err(|_| unavailable_service_failure())?
            .map_err(service_failure)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceInventoryEntry {
    Document(DocumentEntryDto),
    Resource(ResourceEntryDto),
}

impl WorkspaceInventoryEntry {
    pub const fn path(&self) -> &WorkspaceRelativePath {
        match self {
            Self::Document(entry) => &entry.path,
            Self::Resource(entry) => &entry.path,
        }
    }
}

impl From<WorkspaceInventoryEntry> for WorkspaceInventoryEntryDto {
    fn from(entry: WorkspaceInventoryEntry) -> Self {
        match entry {
            WorkspaceInventoryEntry::Document(document) => Self::Document { document },
            WorkspaceInventoryEntry::Resource(resource) => Self::Resource { resource },
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
struct InventoryCandidate {
    name: String,
    path: WorkspaceRelativePath,
    snapshot: InventoryCandidateSnapshot,
}

fn inventory_candidates(
    directory: &Dir,
    parent: &WorkspaceRelativePath,
    ignore: &WorkspaceIgnoreSnapshot,
    budget: &mut InventorySnapshotBudget,
) -> Result<Vec<InventoryCandidate>, ResourceServiceError> {
    let before = trusted_directory_metadata(directory)?;
    let names = ordinary_entry_names(directory)?;
    if names.len() > MAX_IMMEDIATE_INVENTORY_CANDIDATES {
        return Err(ResourceServiceError::unavailable());
    }
    let mut candidates = Vec::with_capacity(names.len());
    for name in &names {
        budget
            .charge_node()
            .map_err(|_| ResourceServiceError::unavailable())?;
        let addressed = directory
            .symlink_metadata(name)
            .map_err(|_| ResourceServiceError::unsafe_target())?;
        if addressed.file_type().is_symlink() {
            return Err(ResourceServiceError::unsafe_target());
        }
        let path = join_relative(parent, name)?;
        let kind = if addressed.is_dir() {
            DocumentKind::Directory
        } else if addressed.is_file() {
            DocumentKind::File
        } else {
            return Err(ResourceServiceError::unsafe_target());
        };
        if ignore.is_ignored(&path, kind) {
            continue;
        }
        let snapshot = if addressed.is_dir() {
            let child = directory
                .open_dir_nofollow(name)
                .map_err(|_| ResourceServiceError::unsafe_target())?;
            let retained = trusted_directory_metadata(&child)?;
            if !same_file(&addressed, &retained) {
                return Err(ResourceServiceError::unsafe_target());
            }
            let retained_modified = InventoryModifiedTime::capture(&retained)
                .map_err(|_| ResourceServiceError::unavailable())?;
            let digest = tree_snapshot_digest(&child, &path, budget)?;
            let after = trusted_directory_metadata(&child)?;
            let named = directory
                .symlink_metadata(name)
                .map_err(|_| ResourceServiceError::unsafe_target())?;
            if !named.is_dir()
                || named.file_type().is_symlink()
                || !same_file(&retained, &after)
                || !same_file(&retained, &named)
                || InventoryModifiedTime::capture(&after)
                    .map_err(|_| ResourceServiceError::unavailable())?
                    != retained_modified
                || InventoryModifiedTime::capture(&named)
                    .map_err(|_| ResourceServiceError::unavailable())?
                    != retained_modified
            {
                return Err(ResourceServiceError::unsafe_target());
            }
            InventoryCandidateSnapshot::from_tree_digest(path.clone(), digest, retained_modified)
        } else {
            inventory_file_snapshot(directory, name, path.clone(), &addressed, budget)?
        };
        candidates.push(InventoryCandidate {
            name: name.clone(),
            path,
            snapshot,
        });
    }
    if ordinary_entry_names(directory)? != names {
        return Err(ResourceServiceError::unsafe_target());
    }
    let after = trusted_directory_metadata(directory)?;
    if !same_file(&before, &after) {
        return Err(ResourceServiceError::unsafe_target());
    }
    Ok(candidates)
}

fn inventory_snapshot_budget() -> InventorySnapshotBudget {
    InventorySnapshotBudget::new(InventorySnapshotLimits {
        maximum_nodes: MAX_INVENTORY_SNAPSHOT_NODES,
        maximum_fallback_bytes: MAX_INVENTORY_FALLBACK_BYTES,
    })
}

fn inventory_file_snapshot(
    directory: &Dir,
    name: &str,
    path: WorkspaceRelativePath,
    addressed: &Metadata,
    budget: &mut InventorySnapshotBudget,
) -> Result<InventoryCandidateSnapshot, ResourceServiceError> {
    if !trusted_regular_file(addressed) {
        return Err(ResourceServiceError::unsafe_target());
    }
    let entry_type = if markdown_name(name) {
        InventoryCandidateType::DocumentFile
    } else {
        InventoryCandidateType::ResourceFile
    };
    let stamp = FileVersionStamp::capture_metadata(addressed);
    match InventoryCandidateSnapshot::from_file_stamp(path.clone(), entry_type, stamp) {
        Ok(snapshot) => Ok(snapshot),
        Err(_) => {
            let inspected = inspect_regular_file_with_budget(directory, name, addressed, budget)?;
            let modified_at = InventoryModifiedTime::capture(&inspected.metadata)
                .map_err(|_| ResourceServiceError::unavailable())?;
            Ok(InventoryCandidateSnapshot::from_content_digest(
                path,
                entry_type,
                inspected.content_digest,
                modified_at,
            ))
        }
    }
}

fn tree_snapshot_digest(
    directory: &Dir,
    path: &WorkspaceRelativePath,
    budget: &mut InventorySnapshotBudget,
) -> Result<ContentDigest, ResourceServiceError> {
    tree_snapshot_digest_at_depth(directory, path, budget, 0)
}

fn tree_snapshot_digest_at_depth(
    directory: &Dir,
    path: &WorkspaceRelativePath,
    budget: &mut InventorySnapshotBudget,
    depth: usize,
) -> Result<ContentDigest, ResourceServiceError> {
    if depth > MAX_INVENTORY_TREE_DEPTH {
        return Err(ResourceServiceError::unavailable());
    }
    let before = trusted_directory_metadata(directory)?;
    let before_stamp = FileVersionStamp::capture_metadata(&before);
    let before_modified =
        InventoryModifiedTime::capture(&before).map_err(|_| ResourceServiceError::unavailable())?;
    let names = tree_entry_names(directory)?;
    let mut manifest = Vec::with_capacity(names.len().saturating_add(1));
    if let Ok(directory_stamp) = InventoryCandidateSnapshot::from_file_stamp(
        path.clone(),
        InventoryCandidateType::Directory,
        before_stamp.clone(),
    ) {
        manifest.push(directory_stamp);
    }
    for name in &names {
        budget
            .charge_node()
            .map_err(|_| ResourceServiceError::unavailable())?;
        let addressed = directory
            .symlink_metadata(name)
            .map_err(|_| ResourceServiceError::unsafe_target())?;
        if addressed.file_type().is_symlink() {
            return Err(ResourceServiceError::unsafe_target());
        }
        if protected_tree_component(name) {
            if !addressed.is_dir() && !trusted_regular_file(&addressed) {
                return Err(ResourceServiceError::unsafe_target());
            }
            continue;
        }
        let child_path = join_relative(path, name)?;
        if addressed.is_dir() {
            let child = directory
                .open_dir_nofollow(name)
                .map_err(|_| ResourceServiceError::unsafe_target())?;
            let retained = trusted_directory_metadata(&child)?;
            if !same_file(&addressed, &retained) {
                return Err(ResourceServiceError::unsafe_target());
            }
            let child_depth = depth
                .checked_add(1)
                .ok_or_else(ResourceServiceError::unavailable)?;
            let digest = tree_snapshot_digest_at_depth(&child, &child_path, budget, child_depth)?;
            let after = trusted_directory_metadata(&child)?;
            let named = directory
                .symlink_metadata(name)
                .map_err(|_| ResourceServiceError::unsafe_target())?;
            if !named.is_dir()
                || named.file_type().is_symlink()
                || !same_file(&retained, &after)
                || !same_file(&retained, &named)
            {
                return Err(ResourceServiceError::unsafe_target());
            }
            let modified_at = InventoryModifiedTime::capture(&after)
                .map_err(|_| ResourceServiceError::unavailable())?;
            if InventoryModifiedTime::capture(&retained)
                .map_err(|_| ResourceServiceError::unavailable())?
                != modified_at
                || InventoryModifiedTime::capture(&named)
                    .map_err(|_| ResourceServiceError::unavailable())?
                    != modified_at
            {
                return Err(ResourceServiceError::unsafe_target());
            }
            manifest.push(InventoryCandidateSnapshot::from_tree_digest(
                child_path,
                digest,
                modified_at,
            ));
        } else if addressed.is_file() {
            manifest.push(inventory_file_snapshot(
                directory, name, child_path, &addressed, budget,
            )?);
        } else {
            return Err(ResourceServiceError::unsafe_target());
        }
    }
    if tree_entry_names(directory)? != names {
        return Err(ResourceServiceError::unsafe_target());
    }
    let after = trusted_directory_metadata(directory)?;
    if !same_file(&before, &after)
        || FileVersionStamp::capture_metadata(&after) != before_stamp
        || InventoryModifiedTime::capture(&after)
            .map_err(|_| ResourceServiceError::unavailable())?
            != before_modified
    {
        return Err(ResourceServiceError::unsafe_target());
    }
    let serialized =
        serde_json::to_vec(&manifest).map_err(|_| ResourceServiceError::unavailable())?;
    let mut digest = Sha256::new();
    digest.update(b"qingyu-inventory-tree-snapshot-v1\0");
    digest.update((serialized.len() as u64).to_be_bytes());
    digest.update(serialized);
    Ok(ContentDigest::new(digest.finalize().into()))
}

fn tree_entry_names(directory: &Dir) -> Result<Vec<String>, ResourceServiceError> {
    let mut names = Vec::new();
    for entry in directory
        .entries()
        .map_err(|_| ResourceServiceError::unavailable())?
    {
        let entry = entry.map_err(|_| ResourceServiceError::unavailable())?;
        let name = entry
            .file_name()
            .to_str()
            .map(str::to_owned)
            .ok_or_else(ResourceServiceError::unsafe_target)?;
        names.push(name);
        if names.len() > MAX_INVENTORY_SNAPSHOT_NODES as usize {
            return Err(ResourceServiceError::unavailable());
        }
    }
    names.sort();
    Ok(names)
}

fn protected_tree_component(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower == ".qingyu"
        || lower == ".markra-sync"
        || lower.starts_with(".qingyu-")
        || lower.starts_with(".markra-sync-stage-")
}

struct ResourceContext {
    runtime: Arc<KernelRuntime>,
    snapshot: Arc<ActiveWorkspaceSnapshot>,
    root: Dir,
    root_path: std::path::PathBuf,
}

impl ResourceContext {
    fn workspace(&self) -> &WorkspaceDto {
        self.snapshot.workspace()
    }
}

fn inspect_inventory_entry(
    context: &ResourceContext,
    ignore: &WorkspaceIgnoreSnapshot,
    directory: &Dir,
    parent: &WorkspaceRelativePath,
    name: &str,
) -> Result<Option<WorkspaceInventoryEntry>, ResourceServiceError> {
    let addressed = directory
        .symlink_metadata(name)
        .map_err(|_| ResourceServiceError::unsafe_target())?;
    if addressed.file_type().is_symlink() {
        return Err(ResourceServiceError::unsafe_target());
    }
    let path = join_relative(parent, name)?;
    let ignore_kind = if addressed.is_dir() {
        DocumentKind::Directory
    } else if addressed.is_file() {
        DocumentKind::File
    } else {
        return Err(ResourceServiceError::unsafe_target());
    };
    if ignore.is_ignored(&path, ignore_kind) {
        return Ok(None);
    }
    if addressed.is_dir() {
        let child = directory
            .open_dir_nofollow(name)
            .map_err(|_| ResourceServiceError::unsafe_target())?;
        let retained = trusted_directory_metadata(&child)?;
        if !same_file(&addressed, &retained) {
            return Err(ResourceServiceError::unsafe_target());
        }
        let revision = directory_revision_for_capability(&child)
            .map_err(|_| ResourceServiceError::unsafe_target())?;
        let after = trusted_directory_metadata(&child)?;
        let named = directory
            .symlink_metadata(name)
            .map_err(|_| ResourceServiceError::unsafe_target())?;
        if !named.is_dir()
            || named.file_type().is_symlink()
            || !same_file(&retained, &after)
            || !same_file(&retained, &named)
        {
            return Err(ResourceServiceError::unsafe_target());
        }
        let entry = document_entry(
            context,
            parent,
            path,
            name,
            DocumentKind::Directory,
            &after,
            revision,
        )?;
        return Ok(Some(WorkspaceInventoryEntry::Document(entry)));
    }
    if !addressed.is_file() {
        return Err(ResourceServiceError::unsafe_target());
    }
    let inspected = inspect_regular_file(directory, name, &addressed)?;
    if markdown_name(name) {
        let entry = document_entry(
            context,
            parent,
            path,
            name,
            DocumentKind::File,
            &inspected.metadata,
            inspected.revision,
        )?;
        Ok(Some(WorkspaceInventoryEntry::Document(entry)))
    } else {
        let classification = classify_resource(name, &inspected.magic);
        let resource_name =
            ResourceName::parse(name).map_err(|_| ResourceServiceError::invalid_path())?;
        let id = context
            .runtime
            .wire_identity_key()
            .issue_resource_id(
                context.workspace().id,
                &context.workspace().generation,
                classification.kind,
                &path,
            )
            .map_err(|_| ResourceServiceError::unavailable())?;
        Ok(Some(WorkspaceInventoryEntry::Resource(ResourceEntryDto {
            id,
            path,
            parent: parent.clone(),
            name: resource_name,
            kind: classification.kind,
            size_bytes: safe_size(inspected.metadata.len())?,
            modified_at: modified_utc(&inspected.metadata)?,
            revision: inspected.revision,
            media_type: classification.media_type.to_string(),
            previewable: classification.previewable,
        })))
    }
}

fn document_entry(
    context: &ResourceContext,
    parent: &WorkspaceRelativePath,
    path: WorkspaceRelativePath,
    name: &str,
    kind: DocumentKind,
    metadata: &Metadata,
    revision: Revision,
) -> Result<DocumentEntryDto, ResourceServiceError> {
    let id = context
        .runtime
        .wire_identity_key()
        .issue_document_id(
            context.workspace().id,
            &context.workspace().generation,
            kind,
            &path,
        )
        .map_err(|_| ResourceServiceError::unavailable())?;
    Ok(DocumentEntryDto {
        id,
        path,
        parent: parent.clone(),
        name: DocumentName::parse(name).map_err(|_| ResourceServiceError::invalid_path())?,
        kind,
        size_bytes: if kind == DocumentKind::File {
            safe_size(metadata.len())?
        } else {
            SafeUnsignedInteger::ZERO
        },
        modified_at: modified_utc(metadata)?,
        revision,
    })
}

struct InspectedFile {
    content_digest: ContentDigest,
    file: File,
    metadata: Metadata,
    revision: Revision,
    magic: [u8; MAGIC_BYTES],
}

fn inspect_regular_file(
    directory: &Dir,
    name: &str,
    addressed: &Metadata,
) -> Result<InspectedFile, ResourceServiceError> {
    inspect_regular_file_inner(directory, name, addressed, None)
}

fn inspect_regular_file_with_budget(
    directory: &Dir,
    name: &str,
    addressed: &Metadata,
    budget: &mut InventorySnapshotBudget,
) -> Result<InspectedFile, ResourceServiceError> {
    inspect_regular_file_inner(directory, name, addressed, Some(budget))
}

fn inspect_regular_file_inner(
    directory: &Dir,
    name: &str,
    addressed: &Metadata,
    fallback_budget: Option<&mut InventorySnapshotBudget>,
) -> Result<InspectedFile, ResourceServiceError> {
    if !trusted_regular_file(addressed) {
        return Err(ResourceServiceError::unsafe_target());
    }
    let mut file = directory
        .open_with(name, &nonfollowing_read_options())
        .map_err(|_| ResourceServiceError::unsafe_target())?;
    let retained = file
        .metadata()
        .map_err(|_| ResourceServiceError::unavailable())?;
    if !trusted_regular_file(&retained) || !same_file(addressed, &retained) {
        return Err(ResourceServiceError::unsafe_target());
    }
    if let Some(budget) = fallback_budget {
        budget
            .charge_fallback_bytes(retained.len())
            .map_err(|_| ResourceServiceError::unavailable())?;
    }
    let addressed_modified = addressed
        .modified()
        .map_err(|_| ResourceServiceError::unavailable())?;
    let expected_modified = retained
        .modified()
        .map_err(|_| ResourceServiceError::unavailable())?;
    let expected_stamp = FileVersionStamp::capture_metadata(&retained);
    if addressed.len() != retained.len()
        || addressed_modified != expected_modified
        || FileVersionStamp::capture_metadata(addressed) != expected_stamp
    {
        return Err(ResourceServiceError::unsafe_target());
    }
    let mut digest = Sha256::new();
    let mut total = 0_u64;
    let mut magic = [0_u8; MAGIC_BYTES];
    let mut magic_len = 0;
    let mut buffer = [0_u8; STREAM_BUFFER_BYTES];
    while total < retained.len() {
        let remaining = retained.len() - total;
        let limit = usize::try_from(remaining)
            .unwrap_or(buffer.len())
            .min(buffer.len());
        let read = file
            .read(&mut buffer[..limit])
            .map_err(|_| ResourceServiceError::unavailable())?;
        if read == 0 {
            return Err(ResourceServiceError::unsafe_target());
        }
        total = total
            .checked_add(read as u64)
            .ok_or_else(ResourceServiceError::unsafe_target)?;
        let copy = (MAGIC_BYTES - magic_len).min(read);
        magic[magic_len..magic_len + copy].copy_from_slice(&buffer[..copy]);
        magic_len += copy;
        digest.update(&buffer[..read]);
    }
    let after = file
        .metadata()
        .map_err(|_| ResourceServiceError::unavailable())?;
    let named = directory
        .symlink_metadata(name)
        .map_err(|_| ResourceServiceError::unsafe_target())?;
    let after_modified = after
        .modified()
        .map_err(|_| ResourceServiceError::unavailable())?;
    let named_modified = named
        .modified()
        .map_err(|_| ResourceServiceError::unavailable())?;
    if !trusted_regular_file(&after)
        || !trusted_regular_file(&named)
        || !same_file(&retained, &after)
        || !same_file(&retained, &named)
        || total != retained.len()
        || after.len() != retained.len()
        || named.len() != retained.len()
        || after_modified != expected_modified
        || named_modified != expected_modified
        || FileVersionStamp::capture_metadata(&after) != expected_stamp
        || FileVersionStamp::capture_metadata(&named) != expected_stamp
    {
        return Err(ResourceServiceError::unsafe_target());
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| ResourceServiceError::unavailable())?;
    let content_digest = ContentDigest::new(digest.finalize().into());
    let revision = Revision::parse(format!(
        "sha256:{}",
        content_digest
            .into_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    ))
    .map_err(|_| ResourceServiceError::unavailable())?;
    Ok(InspectedFile {
        content_digest,
        file,
        metadata: after,
        revision,
        magic,
    })
}

fn ordinary_entry_names(directory: &Dir) -> Result<Vec<String>, ResourceServiceError> {
    ordinary_entry_names_with_limit(directory, MAX_IMMEDIATE_INVENTORY_CANDIDATES)
}

fn ordinary_entry_names_with_limit(
    directory: &Dir,
    maximum_raw_entries: usize,
) -> Result<Vec<String>, ResourceServiceError> {
    let mut names = Vec::new();
    let mut raw_entries = 0_usize;
    for entry in directory
        .entries()
        .map_err(|_| ResourceServiceError::unavailable())?
    {
        let entry = entry.map_err(|_| ResourceServiceError::unavailable())?;
        raw_entries = raw_entries
            .checked_add(1)
            .ok_or_else(ResourceServiceError::unavailable)?;
        if raw_entries > maximum_raw_entries {
            return Err(ResourceServiceError::unavailable());
        }
        let name = entry
            .file_name()
            .to_str()
            .map(str::to_owned)
            .ok_or_else(ResourceServiceError::invalid_path)?;
        if protected_resource_component(&name) {
            continue;
        }
        ResourceName::parse(&name).map_err(|_| ResourceServiceError::invalid_path())?;
        names.push(name);
    }
    names.sort();
    Ok(names)
}

fn open_directory(root: &Dir, path: &WorkspaceRelativePath) -> Result<Dir, ResourceServiceError> {
    let mut directory = root
        .try_clone()
        .map_err(|_| ResourceServiceError::unavailable())?;
    for component in path
        .as_str()
        .split('/')
        .filter(|component| !component.is_empty())
    {
        if protected_resource_component(component) {
            return Err(ResourceServiceError::invalid_path());
        }
        directory = directory
            .open_dir_nofollow(component)
            .map_err(|_| ResourceServiceError::unsafe_target())?;
    }
    Ok(directory)
}

fn join_relative(
    parent: &WorkspaceRelativePath,
    name: &str,
) -> Result<WorkspaceRelativePath, ResourceServiceError> {
    WorkspaceRelativePath::parse(if parent.as_str().is_empty() {
        name.to_string()
    } else {
        format!("{}/{name}", parent.as_str())
    })
    .map_err(|_| ResourceServiceError::invalid_path())
}

fn parent_and_name(
    path: &WorkspaceRelativePath,
) -> Result<(WorkspaceRelativePath, String), ResourceServiceError> {
    let (parent, name) = path
        .as_str()
        .rsplit_once('/')
        .map_or(("", path.as_str()), |(parent, name)| (parent, name));
    if name.is_empty() {
        return Err(ResourceServiceError::invalid_path());
    }
    Ok((
        WorkspaceRelativePath::parse(parent).map_err(|_| ResourceServiceError::invalid_path())?,
        name.to_string(),
    ))
}

fn trusted_directory_metadata(directory: &Dir) -> Result<Metadata, ResourceServiceError> {
    let metadata = directory
        .dir_metadata()
        .map_err(|_| ResourceServiceError::unavailable())?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        Ok(metadata)
    } else {
        Err(ResourceServiceError::unsafe_target())
    }
}

fn trusted_regular_file(metadata: &Metadata) -> bool {
    metadata.is_file() && !metadata.file_type().is_symlink() && link_count(metadata) == 1
}

#[cfg(unix)]
fn link_count(metadata: &Metadata) -> u64 {
    MetadataExt::nlink(metadata)
}

#[cfg(windows)]
fn link_count(metadata: &Metadata) -> u64 {
    use cap_std::fs::MetadataExt as _;
    metadata.number_of_links().unwrap_or(0)
}

#[cfg(not(any(unix, windows)))]
fn link_count(_metadata: &Metadata) -> u64 {
    1
}

#[cfg(unix)]
fn same_file(left: &Metadata, right: &Metadata) -> bool {
    MetadataExt::dev(left) == MetadataExt::dev(right)
        && MetadataExt::ino(left) == MetadataExt::ino(right)
}

#[cfg(windows)]
fn same_file(left: &Metadata, right: &Metadata) -> bool {
    use cap_std::fs::MetadataExt as _;

    matches!(
        (
            left.volume_serial_number(),
            left.file_index(),
            right.volume_serial_number(),
            right.file_index(),
        ),
        (Some(left_volume), Some(left_file), Some(right_volume), Some(right_file))
            if left_volume == right_volume && left_file == right_file
    )
}

#[cfg(not(any(unix, windows)))]
fn same_file(_left: &Metadata, _right: &Metadata) -> bool {
    false
}

fn safe_size(value: u64) -> Result<SafeUnsignedInteger, ResourceServiceError> {
    SafeUnsignedInteger::new(value).map_err(|_| ResourceServiceError::unavailable())
}

fn modified_utc(metadata: &Metadata) -> Result<Rfc3339Utc, ResourceServiceError> {
    let value = metadata
        .modified()
        .map_err(|_| ResourceServiceError::unavailable())?
        .into_std();
    let value = OffsetDateTime::from(value)
        .format(&Rfc3339)
        .map_err(|_| ResourceServiceError::unavailable())?;
    Rfc3339Utc::parse(value).map_err(|_| ResourceServiceError::unavailable())
}

fn markdown_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".md") || lower.ends_with(".markdown")
}

#[derive(Clone, Copy)]
struct ResourceClassification {
    kind: ResourceKind,
    media_type: &'static str,
    previewable: bool,
}

fn classify_resource(name: &str, magic: &[u8; MAGIC_BYTES]) -> ResourceClassification {
    let lower = name.to_ascii_lowercase();
    let media_type = if lower.ends_with(".png") && magic.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if (lower.ends_with(".jpg") || lower.ends_with(".jpeg"))
        && magic.starts_with(b"\xff\xd8\xff")
    {
        Some("image/jpeg")
    } else if lower.ends_with(".gif")
        && (magic.starts_with(b"GIF87a") || magic.starts_with(b"GIF89a"))
    {
        Some("image/gif")
    } else if lower.ends_with(".webp") && magic.starts_with(b"RIFF") && &magic[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    };
    media_type.map_or(
        ResourceClassification {
            kind: ResourceKind::Attachment,
            media_type: "application/octet-stream",
            previewable: false,
        },
        |media_type| ResourceClassification {
            kind: ResourceKind::Image,
            media_type,
            previewable: true,
        },
    )
}

/// Retained resource capability for a later transport adapter.
///
/// A transport must call [`RetainedResource::verify_complete`] after it has
/// emitted the declared content length. Neither `Read::read` returning `Ok(0)`
/// nor an empty-buffer read proves stream integrity. Completion revalidates the
/// identity, metadata, workspace authority, and the SHA-256 of the exact bytes
/// emitted by this reader against the signed resource entry revision.
pub struct RetainedResource {
    snapshot: Arc<ActiveWorkspaceSnapshot>,
    parent: Dir,
    file: File,
    entry: ResourceEntryDto,
    expected: Metadata,
    remaining: u64,
    stream_digest: Sha256,
    verified_complete: bool,
}

impl RetainedResource {
    pub const fn entry(&self) -> &ResourceEntryDto {
        &self.entry
    }

    /// Verifies that the transport consumed exactly the declared content and
    /// that the emitted bytes still match the entry revision.
    ///
    /// Transport adapters must treat a failure as a failed response even when
    /// they have already read exactly `entry.size_bytes` bytes.
    pub fn verify_complete(&mut self) -> io::Result<()> {
        if self.verified_complete {
            return Ok(());
        }
        if self.remaining != 0 {
            return Err(stream_changed());
        }
        let mut sentinel = [0_u8; 1];
        if self.file.read(&mut sentinel)? != 0 {
            return Err(stream_changed());
        }
        let retained = self.file.metadata().map_err(|_| stream_changed())?;
        let named = self
            .parent
            .symlink_metadata(self.entry.name.as_str())
            .map_err(|_| stream_changed())?;
        let expected_modified = self.expected.modified().map_err(|_| stream_changed())?;
        if !trusted_regular_file(&retained)
            || !trusted_regular_file(&named)
            || !same_file(&self.expected, &retained)
            || !same_file(&self.expected, &named)
            || retained.len() != self.expected.len()
            || named.len() != self.expected.len()
            || retained.modified().ok() != Some(expected_modified)
            || named.modified().ok() != Some(expected_modified)
        {
            return Err(stream_changed());
        }
        let streamed_revision = format!("sha256:{:x}", self.stream_digest.clone().finalize());
        if streamed_revision != self.entry.revision.as_str() {
            return Err(stream_changed());
        }
        self.snapshot
            .authority()
            .verify_held_directory()
            .map_err(|_| stream_changed())?;
        self.verified_complete = true;
        Ok(())
    }
}

impl fmt::Debug for RetainedResource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RetainedResource { capability: held, entry: opaque }")
    }
}

impl io::Read for RetainedResource {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() || self.verified_complete || self.remaining == 0 {
            return Ok(0);
        }
        let limit = usize::try_from(self.remaining)
            .unwrap_or(usize::MAX)
            .min(buffer.len());
        let read = self.file.read(&mut buffer[..limit])?;
        if read == 0 {
            return Err(stream_changed());
        }
        self.stream_digest.update(&buffer[..read]);
        self.remaining -= read as u64;
        Ok(read)
    }
}

fn stream_changed() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "retained resource changed while streaming",
    )
}

#[cfg(test)]
mod tests {
    use std::fs;

    use cap_std::{ambient_authority, fs::Dir};
    use tempfile::tempdir;

    use crate::resources::ResourceServiceErrorKind;

    use super::{inventory_snapshot_budget, tree_snapshot_digest, WorkspaceRelativePath};

    #[test]
    fn tree_snapshot_rejects_excessive_nesting() {
        let temporary = tempdir().unwrap();
        let mut path = temporary.path().to_path_buf();
        for _ in 0..130 {
            path.push("d");
            fs::create_dir(&path).unwrap();
        }
        let directory = Dir::open_ambient_dir(temporary.path(), ambient_authority()).unwrap();
        let mut budget = inventory_snapshot_budget();

        let error =
            tree_snapshot_digest(&directory, &WorkspaceRelativePath::default(), &mut budget)
                .unwrap_err();

        assert_eq!(error.kind(), ResourceServiceErrorKind::Unavailable);
    }

    #[test]
    fn raw_protected_entries_are_counted_before_filtering() {
        let temporary = tempdir().unwrap();
        fs::create_dir(temporary.path().join(".qingyu-first")).unwrap();
        fs::create_dir(temporary.path().join(".qingyu-second")).unwrap();
        let directory = Dir::open_ambient_dir(temporary.path(), ambient_authority()).unwrap();

        let error = super::ordinary_entry_names_with_limit(&directory, 1).unwrap_err();

        assert_eq!(error.kind(), ResourceServiceErrorKind::Unavailable);
    }

    #[test]
    fn fallback_budget_charges_the_retained_length_before_reading() {
        use crate::inventory_snapshot::{InventorySnapshotBudget, InventorySnapshotLimits};

        let temporary = tempdir().unwrap();
        let path = temporary.path().join("growing.bin");
        fs::write(&path, b"a").unwrap();
        let directory = Dir::open_ambient_dir(temporary.path(), ambient_authority()).unwrap();
        let addressed = directory.symlink_metadata("growing.bin").unwrap();
        fs::write(path, b"grown").unwrap();
        let mut budget = InventorySnapshotBudget::new(InventorySnapshotLimits {
            maximum_nodes: 1,
            maximum_fallback_bytes: 1,
        });

        let result = super::inspect_regular_file_with_budget(
            &directory,
            "growing.bin",
            &addressed,
            &mut budget,
        );
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("grown fallback file must exceed the retained-length budget"),
        };

        assert_eq!(error.kind(), ResourceServiceErrorKind::Unavailable);
    }
}
