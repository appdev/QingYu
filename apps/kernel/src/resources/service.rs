use std::{
    fmt,
    io::{self, Read as _, Seek as _, SeekFrom},
    mem,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Weak,
    },
};

use async_trait::async_trait;
use cap_fs_ext::{DirExt, MetadataExt};
use cap_std::fs::{Dir, File, Metadata};
use serde::{ser::SerializeSeq as _, Serialize, Serializer};
use sha2::{Digest as _, Sha256};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use crate::{
    contract::{
        DocumentEntryDto, DocumentKind, DocumentName, ErrorCode, ListWorkspaceInventoryQuery,
        Nullable, PageCursorContext, ResourceEntryDto, ResourceId, ResourceKind, ResourceName,
        Revision, Rfc3339Utc, SafeUnsignedInteger, WorkspaceDto, WorkspaceInventoryEntryDto,
        WorkspaceInventoryPageDto, WorkspaceReadiness, WorkspaceRelativePath,
    },
    documents::service::directory_revision_for_capability_with_inventory_budget,
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
const MAX_INVENTORY_CONTENT_BYTES: u64 = 512 * 1024 * 1024;
const MAX_INVENTORY_METADATA_BYTES: u64 = 64 * 1024 * 1024;
const MAX_INVENTORY_TREE_DEPTH: usize = 128;
const MAX_CONCURRENT_INVENTORY_SCANS: usize = 2;
const MAX_INVENTORY_REVISION_BYTES: usize = 71;
const MAX_INVENTORY_TIMESTAMP_BYTES: usize = 64;
const MAX_INVENTORY_MEDIA_TYPE_BYTES: usize = "application/octet-stream".len();
const MAX_INVENTORY_ID_KIND_VARIATION_BYTES: usize = 16;

#[cfg(test)]
thread_local! {
    static TEST_INVENTORY_LIMITS: std::cell::Cell<Option<InventorySnapshotLimits>> = const {
        std::cell::Cell::new(None)
    };
    static TEST_INVENTORY_CONTENT_READS: std::cell::Cell<usize> = const {
        std::cell::Cell::new(0)
    };
    static TEST_FORCE_CONTENT_HASH: std::cell::Cell<bool> = const {
        std::cell::Cell::new(false)
    };
}

#[derive(Clone)]
pub struct WorkspaceResourceService {
    runtime: Weak<KernelRuntime>,
    ignore: Arc<dyn WorkspaceIgnorePort>,
    inventory_scans: Arc<InventoryScanGate>,
}

impl WorkspaceResourceService {
    pub fn new(runtime: &Arc<KernelRuntime>, ignore: Arc<dyn WorkspaceIgnorePort>) -> Self {
        Self {
            runtime: Arc::downgrade(runtime),
            ignore,
            inventory_scans: Arc::new(InventoryScanGate::default()),
        }
    }

    /// Lists one directory level from the retained active workspace capability.
    /// Documents and directories remain document DTOs; binary entries use the
    /// separate resource identity and DTO contract.
    pub fn list_inventory(
        &self,
        parent: &WorkspaceRelativePath,
    ) -> Result<Vec<WorkspaceInventoryEntry>, ResourceServiceError> {
        let _permit = self.inventory_scans.try_acquire()?;
        let context = self.context()?;
        let mut budget = inventory_snapshot_budget();
        self.list_inventory_with_context(&context, parent, &mut budget)
    }

    pub fn list_inventory_page(
        &self,
        query: ListWorkspaceInventoryQuery,
    ) -> Result<WorkspaceInventoryPageDto, ResourceServiceError> {
        let _permit = self.inventory_scans.try_acquire()?;
        let context = self.context()?;
        let ignore = self.capture_ignore(&context)?;
        let directory = open_directory(&context.root, &query.parent)?;
        let mut budget = inventory_snapshot_budget();
        let limit = query.limit.map_or(100, |limit| usize::from(limit.get()));
        let (candidates, verification_names) = inventory_candidates(
            &directory,
            &query.parent,
            &ignore,
            &mut budget,
            |names, budget| {
                reserve_inventory_output(
                    &context,
                    &directory,
                    &query.parent,
                    &ignore,
                    names,
                    InventoryOutputKind::Page { maximum: limit },
                    budget,
                )
            },
            |path, name, kind| preflight_inventory_identity(&context, path, name, kind),
        )?;
        let snapshots = InventoryCandidateSnapshots(&candidates);
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
                candidates
                    .partition_point(|candidate| candidate.logical_path().as_str() <= last.as_str())
            }
            None => 0,
        };
        let end = start.saturating_add(limit).min(candidates.len());
        let mut items = Vec::with_capacity(end.saturating_sub(start));
        for candidate in &candidates[start..end] {
            let entry = inspect_inventory_entry(
                &context,
                &ignore,
                &directory,
                &query.parent,
                &candidate.name,
                &mut budget,
            )?
            .ok_or_else(ResourceServiceError::unsafe_target)?;
            if entry.path() != candidate.logical_path() {
                return Err(ResourceServiceError::unsafe_target());
            }
            items.push(WorkspaceInventoryEntryDto::from(entry));
        }
        verify_inventory_candidates_unchanged(
            &directory,
            &query.parent,
            &ignore,
            &mut budget,
            &candidates,
            verification_names,
        )?;
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
        budget: &mut InventorySnapshotBudget,
    ) -> Result<Vec<WorkspaceInventoryEntry>, ResourceServiceError> {
        let ignore = self.capture_ignore(context)?;
        let directory = open_directory(&context.root, parent)?;
        let before = trusted_directory_metadata(&directory)?;
        let scanned_names = ordinary_entry_names(&directory, budget, 1)?;
        let names_digest = entry_names_digest(&scanned_names.names)?;
        reserve_inventory_output(
            context,
            &directory,
            parent,
            &ignore,
            &scanned_names.names,
            InventoryOutputKind::Direct,
            budget,
        )?;
        let mut entries = Vec::with_capacity(scanned_names.names.len());
        for name in scanned_names.names {
            if let Some(entry) =
                inspect_inventory_entry(context, &ignore, &directory, parent, &name, budget)?
            {
                entries.push(entry);
            }
        }
        if entry_names_digest(&ordinary_entry_names_prepaid(
            &directory,
            scanned_names.rescan,
        )?)? != names_digest
        {
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

#[derive(Default)]
struct InventoryScanGate {
    active: AtomicUsize,
}

impl InventoryScanGate {
    fn try_acquire(self: &Arc<Self>) -> Result<InventoryScanPermit, ResourceServiceError> {
        self.active
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
                (active < MAX_CONCURRENT_INVENTORY_SCANS).then_some(active + 1)
            })
            .map_err(|_| ResourceServiceError::unavailable())?;
        Ok(InventoryScanPermit { gate: self.clone() })
    }
}

struct InventoryScanPermit {
    gate: Arc<InventoryScanGate>,
}

impl Drop for InventoryScanPermit {
    fn drop(&mut self) {
        let previous = self.gate.active.fetch_sub(1, Ordering::Release);
        debug_assert!(previous > 0, "inventory scan permit underflow");
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
    snapshot: InventoryCandidateSnapshot,
}

impl InventoryCandidate {
    fn logical_path(&self) -> &WorkspaceRelativePath {
        self.snapshot.logical_path()
    }
}

struct InventoryCandidateSnapshots<'a>(&'a [InventoryCandidate]);

impl Serialize for InventoryCandidateSnapshots<'_> {
    fn serialize<SerializerType>(
        &self,
        serializer: SerializerType,
    ) -> Result<SerializerType::Ok, SerializerType::Error>
    where
        SerializerType: Serializer,
    {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for candidate in self.0 {
            sequence.serialize_element(&candidate.snapshot)?;
        }
        sequence.end()
    }
}

fn inventory_candidates<Prepare, Validate>(
    directory: &Dir,
    parent: &WorkspaceRelativePath,
    ignore: &WorkspaceIgnoreSnapshot,
    budget: &mut InventorySnapshotBudget,
    prepare: Prepare,
    validate: Validate,
) -> Result<(Vec<InventoryCandidate>, OrdinaryEntryNamesReservation), ResourceServiceError>
where
    Prepare: FnOnce(&[String], &mut InventorySnapshotBudget) -> Result<(), ResourceServiceError>,
    Validate: Fn(&WorkspaceRelativePath, &str, DocumentKind) -> Result<(), ResourceServiceError>,
{
    scan_inventory_candidates(
        InventoryCandidateScan {
            directory,
            parent,
            ignore,
            budget,
            names_reservation: None,
        },
        prepare,
        validate,
        |maximum_candidates, budget| {
            charge_vec_metadata::<InventoryCandidate>(budget, maximum_candidates)?;
            Ok(Vec::with_capacity(maximum_candidates))
        },
        |candidates, candidate| {
            candidates.push(candidate);
            Ok(())
        },
    )
}

fn verify_inventory_candidates_unchanged(
    directory: &Dir,
    parent: &WorkspaceRelativePath,
    ignore: &WorkspaceIgnoreSnapshot,
    budget: &mut InventorySnapshotBudget,
    expected: &[InventoryCandidate],
    names_reservation: OrdinaryEntryNamesReservation,
) -> Result<(), ResourceServiceError> {
    let (matched, _reservation) = scan_inventory_candidates(
        InventoryCandidateScan {
            directory,
            parent,
            ignore,
            budget,
            names_reservation: Some(names_reservation),
        },
        |_names, _budget| Ok(()),
        |_path, _name, _kind| Ok(()),
        |_maximum_candidates, _budget| Ok(0_usize),
        |index, candidate| {
            if expected.get(*index) != Some(&candidate) {
                return Err(ResourceServiceError::unsafe_target());
            }
            *index = index
                .checked_add(1)
                .ok_or_else(ResourceServiceError::unavailable)?;
            Ok(())
        },
    )?;
    if matched != expected.len() {
        return Err(ResourceServiceError::unsafe_target());
    }
    Ok(())
}

struct InventoryCandidateScan<'a> {
    directory: &'a Dir,
    parent: &'a WorkspaceRelativePath,
    ignore: &'a WorkspaceIgnoreSnapshot,
    budget: &'a mut InventorySnapshotBudget,
    names_reservation: Option<OrdinaryEntryNamesReservation>,
}

fn scan_inventory_candidates<State, Prepare, Validate, Initialize, Accept>(
    scan: InventoryCandidateScan<'_>,
    prepare: Prepare,
    validate: Validate,
    initialize: Initialize,
    mut accept: Accept,
) -> Result<(State, OrdinaryEntryNamesReservation), ResourceServiceError>
where
    Prepare: FnOnce(&[String], &mut InventorySnapshotBudget) -> Result<(), ResourceServiceError>,
    Validate: Fn(&WorkspaceRelativePath, &str, DocumentKind) -> Result<(), ResourceServiceError>,
    Initialize: FnOnce(usize, &mut InventorySnapshotBudget) -> Result<State, ResourceServiceError>,
    Accept: FnMut(&mut State, InventoryCandidate) -> Result<(), ResourceServiceError>,
{
    let InventoryCandidateScan {
        directory,
        parent,
        ignore,
        budget,
        names_reservation,
    } = scan;
    let before = trusted_directory_metadata(directory)?;
    let fresh_names = names_reservation.is_none();
    let scanned_names = if let Some(rescan) = names_reservation {
        OrdinaryEntryNames {
            names: ordinary_entry_names_prepaid(directory, rescan)?,
            rescan,
        }
    } else {
        ordinary_entry_names(directory, budget, 3)?
    };
    if scanned_names.names.len() > MAX_IMMEDIATE_INVENTORY_CANDIDATES {
        return Err(ResourceServiceError::unavailable());
    }
    let names_digest = entry_names_digest(&scanned_names.names)?;
    prepare(&scanned_names.names, budget)?;
    if fresh_names {
        let path_bytes = scanned_names.names.iter().try_fold(0_u64, |total, name| {
            let bytes = parent
                .as_str()
                .len()
                .checked_add(usize::from(!parent.as_str().is_empty()))
                .and_then(|bytes| bytes.checked_add(name.len()))
                .and_then(|bytes| u64::try_from(bytes).ok())
                .ok_or_else(ResourceServiceError::unavailable)?;
            total
                .checked_add(bytes)
                .ok_or_else(ResourceServiceError::unavailable)
        })?;
        budget
            .charge_metadata_bytes(
                path_bytes
                    .checked_mul(2)
                    .ok_or_else(ResourceServiceError::unavailable)?,
            )
            .map_err(|_| ResourceServiceError::unavailable())?;
    }
    let mut state = initialize(scanned_names.names.len(), budget)?;
    for name in scanned_names.names {
        let path = join_relative(parent, &name)?;
        let addressed = directory
            .symlink_metadata(&name)
            .map_err(|_| ResourceServiceError::unsafe_target())?;
        if addressed.file_type().is_symlink() {
            return Err(ResourceServiceError::unsafe_target());
        }
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
        validate(&path, &name, kind)?;
        let snapshot = if addressed.is_dir() {
            let child = directory
                .open_dir_nofollow(&name)
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
                .symlink_metadata(&name)
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
            InventoryCandidateSnapshot::from_tree_digest(path, digest, retained_modified)
        } else {
            inventory_file_snapshot(directory, &name, path, &addressed, budget)?
        };
        accept(&mut state, InventoryCandidate { name, snapshot })?;
    }
    if entry_names_digest(&ordinary_entry_names_prepaid(
        directory,
        scanned_names.rescan,
    )?)? != names_digest
    {
        return Err(ResourceServiceError::unsafe_target());
    }
    let after = trusted_directory_metadata(directory)?;
    if !same_file(&before, &after) {
        return Err(ResourceServiceError::unsafe_target());
    }
    Ok((state, scanned_names.rescan))
}

fn inventory_snapshot_budget() -> InventorySnapshotBudget {
    #[cfg(test)]
    if let Some(limits) = TEST_INVENTORY_LIMITS.get() {
        return InventorySnapshotBudget::new(limits);
    }
    InventorySnapshotBudget::new(InventorySnapshotLimits {
        maximum_nodes: MAX_INVENTORY_SNAPSHOT_NODES,
        maximum_content_bytes: MAX_INVENTORY_CONTENT_BYTES,
        maximum_metadata_bytes: MAX_INVENTORY_METADATA_BYTES,
        maximum_depth: MAX_INVENTORY_TREE_DEPTH,
    })
}

fn charge_vec_metadata<Element>(
    budget: &mut InventorySnapshotBudget,
    elements: usize,
) -> Result<(), ResourceServiceError> {
    let bytes = mem::size_of::<Element>()
        .checked_mul(elements)
        .and_then(|bytes| u64::try_from(bytes).ok())
        .ok_or_else(ResourceServiceError::unavailable)?;
    budget
        .charge_metadata_bytes(bytes)
        .map_err(|_| ResourceServiceError::unavailable())
}

fn string_metadata_bytes(bytes: usize) -> Result<u64, ResourceServiceError> {
    mem::size_of::<String>()
        .checked_add(bytes)
        .and_then(|bytes| u64::try_from(bytes).ok())
        .ok_or_else(ResourceServiceError::unavailable)
}

#[derive(Clone, Copy)]
enum InventoryOutputKind {
    Direct,
    Page { maximum: usize },
}

fn reserve_inventory_output(
    context: &ResourceContext,
    directory: &Dir,
    parent: &WorkspaceRelativePath,
    ignore: &WorkspaceIgnoreSnapshot,
    names: &[String],
    output_kind: InventoryOutputKind,
    budget: &mut InventorySnapshotBudget,
) -> Result<(), ResourceServiceError> {
    let mut included = 0_usize;
    let mut retained_total = 0_usize;
    let mut maximum_retained = 0_usize;
    let mut maximum_transient = MAX_INVENTORY_TIMESTAMP_BYTES;
    for name in names {
        let path_bytes = parent
            .as_str()
            .len()
            .checked_add(usize::from(!parent.as_str().is_empty()))
            .and_then(|bytes| bytes.checked_add(name.len()))
            .ok_or_else(ResourceServiceError::unavailable)?;
        if u64::try_from(path_bytes).map_err(|_| ResourceServiceError::unavailable())?
            > budget.remaining_metadata_bytes()
        {
            return Err(ResourceServiceError::unavailable());
        }
        let path = join_relative(parent, name)?;
        maximum_transient = maximum_transient.max(path_bytes);
        let addressed = directory
            .symlink_metadata(name)
            .map_err(|_| ResourceServiceError::unsafe_target())?;
        if addressed.file_type().is_symlink() {
            return Err(ResourceServiceError::unsafe_target());
        }
        let document_kind = if addressed.is_dir() {
            Some(DocumentKind::Directory)
        } else if addressed.is_file() && markdown_name(name) {
            Some(DocumentKind::File)
        } else if addressed.is_file() {
            None
        } else {
            return Err(ResourceServiceError::unsafe_target());
        };
        let ignore_kind = document_kind.unwrap_or(DocumentKind::File);
        if ignore.is_ignored(&path, ignore_kind) {
            continue;
        }
        let allocation = if let Some(kind) = document_kind {
            context.runtime.wire_identity_key().document_id_allocation(
                context.workspace().id,
                &context.workspace().generation,
                kind,
                &path,
            )
        } else {
            context.runtime.wire_identity_key().resource_id_allocation(
                context.workspace().id,
                &context.workspace().generation,
                ResourceKind::Attachment,
                &path,
            )
        }
        .map_err(|_| ResourceServiceError::unavailable())?;
        let retained = path_bytes
            .checked_add(parent.as_str().len())
            .and_then(|bytes| bytes.checked_add(name.len()))
            .and_then(|bytes| bytes.checked_add(MAX_INVENTORY_REVISION_BYTES))
            .and_then(|bytes| bytes.checked_add(MAX_INVENTORY_TIMESTAMP_BYTES))
            .and_then(|bytes| bytes.checked_add(allocation.token_bytes()))
            .and_then(|bytes| bytes.checked_add(MAX_INVENTORY_ID_KIND_VARIATION_BYTES))
            .and_then(|bytes| {
                bytes.checked_add(if document_kind.is_none() {
                    MAX_INVENTORY_MEDIA_TYPE_BYTES
                } else {
                    0
                })
            })
            .ok_or_else(ResourceServiceError::unavailable)?;
        retained_total = retained_total
            .checked_add(retained)
            .ok_or_else(ResourceServiceError::unavailable)?;
        maximum_retained = maximum_retained.max(retained);
        maximum_transient = maximum_transient
            .max(
                allocation
                    .transient_bytes()
                    .checked_add(MAX_INVENTORY_ID_KIND_VARIATION_BYTES)
                    .ok_or_else(ResourceServiceError::unavailable)?,
            )
            .max(name.len());
        included = included
            .checked_add(1)
            .ok_or_else(ResourceServiceError::unavailable)?;
    }
    let (vector_elements, retained_bytes) = match output_kind {
        InventoryOutputKind::Direct => (names.len(), retained_total),
        InventoryOutputKind::Page { maximum } => {
            let elements = included.min(maximum);
            let bytes = maximum_retained
                .checked_mul(elements)
                .ok_or_else(ResourceServiceError::unavailable)?;
            (elements, bytes)
        }
    };
    let vector_bytes = match output_kind {
        InventoryOutputKind::Direct => mem::size_of::<WorkspaceInventoryEntry>(),
        InventoryOutputKind::Page { .. } => mem::size_of::<WorkspaceInventoryEntryDto>(),
    }
    .checked_mul(vector_elements)
    .ok_or_else(ResourceServiceError::unavailable)?;
    let bytes = vector_bytes
        .checked_add(retained_bytes)
        .and_then(|bytes| bytes.checked_add(maximum_transient))
        .and_then(|bytes| u64::try_from(bytes).ok())
        .ok_or_else(ResourceServiceError::unavailable)?;
    budget
        .charge_metadata_bytes(bytes)
        .map_err(|_| ResourceServiceError::unavailable())
}

fn entry_names_digest(names: &[String]) -> Result<ContentDigest, ResourceServiceError> {
    let mut digest = Sha256::new();
    digest.update(b"qingyu-inventory-entry-names-v1\0");
    digest.update(
        u64::try_from(names.len())
            .map_err(|_| ResourceServiceError::unavailable())?
            .to_be_bytes(),
    );
    for name in names {
        digest.update(
            u64::try_from(name.len())
                .map_err(|_| ResourceServiceError::unavailable())?
                .to_be_bytes(),
        );
        digest.update(name.as_bytes());
    }
    Ok(ContentDigest::new(digest.finalize().into()))
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
    #[cfg(test)]
    let stamp = if TEST_FORCE_CONTENT_HASH.get() {
        FileVersionStamp::requires_content_hash()
    } else {
        FileVersionStamp::capture_metadata(addressed)
    };
    #[cfg(not(test))]
    let stamp = FileVersionStamp::capture_metadata(addressed);
    if stamp.strong().is_some() {
        return InventoryCandidateSnapshot::from_file_stamp(path, entry_type, stamp)
            .map_err(|_| ResourceServiceError::unavailable());
    }
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
    budget
        .require_depth(depth)
        .map_err(|_| ResourceServiceError::unavailable())?;
    let before = trusted_directory_metadata(directory)?;
    let before_stamp = FileVersionStamp::capture_metadata(&before);
    let before_modified =
        InventoryModifiedTime::capture(&before).map_err(|_| ResourceServiceError::unavailable())?;
    let scanned_names = tree_entry_names(directory, budget)?;
    let names_digest = entry_names_digest(&scanned_names.names)?;
    let manifest_capacity = scanned_names
        .names
        .len()
        .checked_add(1)
        .ok_or_else(ResourceServiceError::unavailable)?;
    charge_vec_metadata::<InventoryCandidateSnapshot>(budget, manifest_capacity)?;
    let mut manifest = Vec::with_capacity(manifest_capacity);
    budget
        .charge_metadata_bytes(
            u64::try_from(path.as_str().len()).map_err(|_| ResourceServiceError::unavailable())?,
        )
        .map_err(|_| ResourceServiceError::unavailable())?;
    if let Ok(directory_stamp) = InventoryCandidateSnapshot::from_file_stamp(
        path.clone(),
        InventoryCandidateType::Directory,
        before_stamp.clone(),
    ) {
        manifest.push(directory_stamp);
    }
    let child_path_bytes = scanned_names.names.iter().try_fold(0_u64, |total, name| {
        if protected_tree_component(name) {
            return Ok(total);
        }
        let bytes = path
            .as_str()
            .len()
            .checked_add(usize::from(!path.as_str().is_empty()))
            .and_then(|bytes| bytes.checked_add(name.len()))
            .and_then(|bytes| u64::try_from(bytes).ok())
            .ok_or_else(ResourceServiceError::unavailable)?;
        total
            .checked_add(bytes)
            .ok_or_else(ResourceServiceError::unavailable)
    })?;
    budget
        .charge_metadata_bytes(child_path_bytes)
        .map_err(|_| ResourceServiceError::unavailable())?;
    for name in scanned_names.names {
        let addressed = directory
            .symlink_metadata(&name)
            .map_err(|_| ResourceServiceError::unsafe_target())?;
        if addressed.file_type().is_symlink() {
            return Err(ResourceServiceError::unsafe_target());
        }
        if protected_tree_component(&name) {
            if !addressed.is_dir() && !trusted_regular_file(&addressed) {
                return Err(ResourceServiceError::unsafe_target());
            }
            continue;
        }
        let child_path = join_relative(path, &name)?;
        if addressed.is_dir() {
            let child = directory
                .open_dir_nofollow(&name)
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
                .symlink_metadata(&name)
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
                directory, &name, child_path, &addressed, budget,
            )?);
        } else {
            return Err(ResourceServiceError::unsafe_target());
        }
    }
    if entry_names_digest(&tree_entry_names_prepaid(directory, scanned_names.rescan)?)?
        != names_digest
    {
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
    tree_manifest_digest(&manifest)
}

fn tree_manifest_digest(
    manifest: &[InventoryCandidateSnapshot],
) -> Result<ContentDigest, ResourceServiceError> {
    let mut length_writer = JsonLengthWriter::default();
    serde_json::to_writer(&mut length_writer, manifest)
        .map_err(|_| ResourceServiceError::unavailable())?;
    let mut digest = Sha256::new();
    digest.update(b"qingyu-inventory-tree-snapshot-v1\0");
    digest.update(length_writer.length.to_be_bytes());
    serde_json::to_writer(Sha256Writer(&mut digest), manifest)
        .map_err(|_| ResourceServiceError::unavailable())?;
    Ok(ContentDigest::new(digest.finalize().into()))
}

#[derive(Default)]
struct JsonLengthWriter {
    length: u64,
}

impl io::Write for JsonLengthWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.length = self
            .length
            .checked_add(u64::try_from(bytes.len()).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "JSON length exceeds u64")
            })?)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "JSON length exceeds u64"))?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct Sha256Writer<'a>(&'a mut Sha256);

impl io::Write for Sha256Writer<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct TreeEntryNames {
    names: Vec<String>,
    rescan: TreeEntryNamesReservation,
}

#[derive(Clone, Copy)]
struct TreeEntryNamesReservation {
    maximum_entries: usize,
    metadata_bytes: u64,
}

fn tree_entry_names(
    directory: &Dir,
    budget: &mut InventorySnapshotBudget,
) -> Result<TreeEntryNames, ResourceServiceError> {
    let mut names = Vec::new();
    let mut metadata_bytes = 0_u64;
    for entry in directory
        .entries()
        .map_err(|_| ResourceServiceError::unavailable())?
    {
        let entry = entry.map_err(|_| ResourceServiceError::unavailable())?;
        budget
            .charge_node()
            .map_err(|_| ResourceServiceError::unavailable())?;
        let file_name = entry.file_name();
        let name = file_name
            .to_str()
            .ok_or_else(ResourceServiceError::unsafe_target)?;
        // The directory entry owns the raw OsString while `names` retains a
        // second UTF-8 allocation for sorting and traversal.
        for _ in 0..2 {
            let bytes = string_metadata_bytes(name.len())?;
            budget
                .charge_metadata_bytes(bytes)
                .map_err(|_| ResourceServiceError::unavailable())?;
            metadata_bytes = metadata_bytes
                .checked_add(bytes)
                .ok_or_else(ResourceServiceError::unavailable)?;
        }
        names.push(name.to_owned());
        if names.len() > MAX_INVENTORY_SNAPSHOT_NODES as usize {
            return Err(ResourceServiceError::unavailable());
        }
    }
    names.sort();
    budget
        .charge_nodes(u64::try_from(names.len()).map_err(|_| ResourceServiceError::unavailable())?)
        .map_err(|_| ResourceServiceError::unavailable())?;
    budget
        .charge_metadata_bytes(metadata_bytes)
        .map_err(|_| ResourceServiceError::unavailable())?;
    Ok(TreeEntryNames {
        rescan: TreeEntryNamesReservation {
            maximum_entries: names.len(),
            metadata_bytes,
        },
        names,
    })
}

fn tree_entry_names_prepaid(
    directory: &Dir,
    reservation: TreeEntryNamesReservation,
) -> Result<Vec<String>, ResourceServiceError> {
    let mut names = Vec::new();
    let mut remaining_metadata_bytes = reservation.metadata_bytes;
    for entry in directory
        .entries()
        .map_err(|_| ResourceServiceError::unavailable())?
    {
        let entry = entry.map_err(|_| ResourceServiceError::unavailable())?;
        if names.len() >= reservation.maximum_entries {
            return Err(ResourceServiceError::unsafe_target());
        }
        let file_name = entry.file_name();
        let name = file_name
            .to_str()
            .ok_or_else(ResourceServiceError::unsafe_target)?;
        consume_prepaid_metadata(&mut remaining_metadata_bytes, name.len())?;
        consume_prepaid_metadata(&mut remaining_metadata_bytes, name.len())?;
        names.push(name.to_owned());
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
    budget: &mut InventorySnapshotBudget,
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
    preflight_inventory_identity(context, &path, name, ignore_kind)?;
    if addressed.is_dir() {
        let child = directory
            .open_dir_nofollow(name)
            .map_err(|_| ResourceServiceError::unsafe_target())?;
        let retained = trusted_directory_metadata(&child)?;
        if !same_file(&addressed, &retained) {
            return Err(ResourceServiceError::unsafe_target());
        }
        let revision = directory_revision_for_capability_with_inventory_budget(&child, budget)
            .map_err(|error| {
                if error.kind() == crate::documents::service::DocumentServiceErrorKind::Unavailable
                {
                    ResourceServiceError::unavailable()
                } else {
                    ResourceServiceError::unsafe_target()
                }
            })?;
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
    let inspected = inspect_regular_file_with_budget(directory, name, &addressed, budget)?;
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

fn preflight_inventory_identity(
    context: &ResourceContext,
    path: &WorkspaceRelativePath,
    name: &str,
    kind: DocumentKind,
) -> Result<(), ResourceServiceError> {
    let allocation = if kind == DocumentKind::Directory || markdown_name(name) {
        context.runtime.wire_identity_key().document_id_allocation(
            context.workspace().id,
            &context.workspace().generation,
            kind,
            path,
        )
    } else {
        context.runtime.wire_identity_key().resource_id_allocation(
            context.workspace().id,
            &context.workspace().generation,
            ResourceKind::Attachment,
            path,
        )
    };
    allocation
        .map(|_allocation| ())
        .map_err(|_| ResourceServiceError::unavailable())
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
    inventory_budget: Option<&mut InventorySnapshotBudget>,
) -> Result<InspectedFile, ResourceServiceError> {
    #[cfg(test)]
    if inventory_budget.is_some() {
        TEST_INVENTORY_CONTENT_READS.set(TEST_INVENTORY_CONTENT_READS.get().saturating_add(1));
    }
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
    if let Some(budget) = inventory_budget {
        budget
            .charge_content_bytes(retained.len())
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

struct OrdinaryEntryNames {
    names: Vec<String>,
    rescan: OrdinaryEntryNamesReservation,
}

#[derive(Clone, Copy, Debug)]
struct OrdinaryEntryNamesReservation {
    maximum_raw_entries: usize,
    metadata_bytes: u64,
}

fn ordinary_entry_names(
    directory: &Dir,
    budget: &mut InventorySnapshotBudget,
    prepaid_scans: usize,
) -> Result<OrdinaryEntryNames, ResourceServiceError> {
    ordinary_entry_names_and_reserve(
        directory,
        MAX_IMMEDIATE_INVENTORY_CANDIDATES,
        budget,
        prepaid_scans,
    )
}

#[cfg(test)]
fn ordinary_entry_names_with_limit(
    directory: &Dir,
    maximum_raw_entries: usize,
    budget: &mut InventorySnapshotBudget,
) -> Result<Vec<String>, ResourceServiceError> {
    Ok(ordinary_entry_names_scan(directory, maximum_raw_entries, budget, 0)?.names)
}

fn ordinary_entry_names_and_reserve(
    directory: &Dir,
    maximum_raw_entries: usize,
    budget: &mut InventorySnapshotBudget,
    prepaid_scans: usize,
) -> Result<OrdinaryEntryNames, ResourceServiceError> {
    ordinary_entry_names_scan(directory, maximum_raw_entries, budget, prepaid_scans)
}

fn ordinary_entry_names_scan(
    directory: &Dir,
    maximum_raw_entries: usize,
    budget: &mut InventorySnapshotBudget,
    prepaid_scans: usize,
) -> Result<OrdinaryEntryNames, ResourceServiceError> {
    let mut names = Vec::new();
    let mut raw_entries = 0_usize;
    let mut metadata_bytes = 0_u64;
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
        budget
            .charge_node()
            .map_err(|_| ResourceServiceError::unavailable())?;
        let file_name = entry.file_name();
        let name = file_name
            .to_str()
            .ok_or_else(ResourceServiceError::invalid_path)?;
        // Account for the raw OsString before filtering. Protected-name
        // fanout must not bypass the request metadata budget.
        let raw_name_bytes = string_metadata_bytes(name.len())?;
        budget
            .charge_metadata_bytes(raw_name_bytes)
            .map_err(|_| ResourceServiceError::unavailable())?;
        metadata_bytes = metadata_bytes
            .checked_add(raw_name_bytes)
            .ok_or_else(ResourceServiceError::unavailable)?;
        if protected_resource_component(name) {
            continue;
        }
        // ResourceName validation and the retained sortable name each own a
        // temporary String allocation in this request.
        let parsed_name_bytes = string_metadata_bytes(name.len())?;
        budget
            .charge_metadata_bytes(parsed_name_bytes)
            .map_err(|_| ResourceServiceError::unavailable())?;
        metadata_bytes = metadata_bytes
            .checked_add(parsed_name_bytes)
            .ok_or_else(ResourceServiceError::unavailable)?;
        ResourceName::parse(name).map_err(|_| ResourceServiceError::invalid_path())?;
        let retained_name_bytes = string_metadata_bytes(name.len())?;
        budget
            .charge_metadata_bytes(retained_name_bytes)
            .map_err(|_| ResourceServiceError::unavailable())?;
        metadata_bytes = metadata_bytes
            .checked_add(retained_name_bytes)
            .ok_or_else(ResourceServiceError::unavailable)?;
        names.push(name.to_owned());
    }
    names.sort();
    if prepaid_scans != 0 {
        let prepaid_scans =
            u64::try_from(prepaid_scans).map_err(|_| ResourceServiceError::unavailable())?;
        budget
            .charge_nodes(
                u64::try_from(raw_entries)
                    .map_err(|_| ResourceServiceError::unavailable())?
                    .checked_mul(prepaid_scans)
                    .ok_or_else(ResourceServiceError::unavailable)?,
            )
            .map_err(|_| ResourceServiceError::unavailable())?;
        budget
            .charge_metadata_bytes(
                metadata_bytes
                    .checked_mul(prepaid_scans)
                    .ok_or_else(ResourceServiceError::unavailable)?,
            )
            .map_err(|_| ResourceServiceError::unavailable())?;
    }
    Ok(OrdinaryEntryNames {
        names,
        rescan: OrdinaryEntryNamesReservation {
            maximum_raw_entries: raw_entries,
            metadata_bytes,
        },
    })
}

fn ordinary_entry_names_prepaid(
    directory: &Dir,
    reservation: OrdinaryEntryNamesReservation,
) -> Result<Vec<String>, ResourceServiceError> {
    let mut names = Vec::new();
    let mut raw_entries = 0_usize;
    let mut remaining_metadata_bytes = reservation.metadata_bytes;
    for entry in directory
        .entries()
        .map_err(|_| ResourceServiceError::unavailable())?
    {
        let entry = entry.map_err(|_| ResourceServiceError::unavailable())?;
        raw_entries = raw_entries
            .checked_add(1)
            .ok_or_else(ResourceServiceError::unavailable)?;
        if raw_entries > reservation.maximum_raw_entries {
            return Err(ResourceServiceError::unsafe_target());
        }
        let file_name = entry.file_name();
        let name = file_name
            .to_str()
            .ok_or_else(ResourceServiceError::invalid_path)?;
        consume_prepaid_metadata(&mut remaining_metadata_bytes, name.len())?;
        if protected_resource_component(name) {
            continue;
        }
        consume_prepaid_metadata(&mut remaining_metadata_bytes, name.len())?;
        ResourceName::parse(name).map_err(|_| ResourceServiceError::invalid_path())?;
        consume_prepaid_metadata(&mut remaining_metadata_bytes, name.len())?;
        names.push(name.to_owned());
    }
    names.sort();
    Ok(names)
}

fn consume_prepaid_metadata(
    remaining: &mut u64,
    string_bytes: usize,
) -> Result<(), ResourceServiceError> {
    *remaining = remaining
        .checked_sub(string_metadata_bytes(string_bytes)?)
        .ok_or_else(ResourceServiceError::unsafe_target)?;
    Ok(())
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
    use std::{
        fs,
        path::PathBuf,
        sync::{Arc, Mutex},
    };

    use cap_std::{ambient_authority, fs::Dir};
    use sha2::{Digest as _, Sha256};
    use tempfile::{tempdir, TempDir};

    use crate::{
        config::KernelConfig,
        contract::{ListWorkspaceInventoryQuery, PageLimit},
        documents::AllowAllDocumentIgnorePort,
        ignore_rules::{WorkspaceIgnoreError, WorkspaceIgnorePort, WorkspaceIgnoreSnapshot},
        inventory_snapshot::{
            ContentDigest, FileVersionStamp, InventoryCandidateSnapshot, InventoryCandidateType,
            StrongFileVersionStamp,
        },
        paths::KernelPaths,
        ports::KernelPorts,
        resources::ResourceServiceErrorKind,
        runtime::KernelRuntime,
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

    use super::{inventory_snapshot_budget, tree_snapshot_digest, WorkspaceRelativePath};

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

    struct AllowAllWorkspaceIgnorePort;

    impl WorkspaceIgnorePort for AllowAllWorkspaceIgnorePort {
        fn capture(
            &self,
            _root_path: &std::path::Path,
            _retained_root: &Dir,
        ) -> Result<WorkspaceIgnoreSnapshot, WorkspaceIgnoreError> {
            Ok(WorkspaceIgnoreSnapshot::from_matcher(Arc::new(
                AllowAllDocumentIgnorePort,
            )))
        }
    }

    struct InventoryFixture {
        _temporary: TempDir,
        _runtime: Arc<KernelRuntime>,
        _workspace: Arc<WorkspaceService>,
        service: super::WorkspaceResourceService,
        root: PathBuf,
    }

    impl InventoryFixture {
        async fn new() -> Self {
            let temporary = tempdir().unwrap();
            let root = temporary.path().join("workspace");
            let app_data = temporary.path().join("app-data");
            let cache = temporary.path().join("cache");
            for path in [&root, &app_data, &cache] {
                fs::create_dir(path).unwrap();
            }
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
                    "Resources budget",
                )
                .await
                .unwrap(),
            );
            let service = super::WorkspaceResourceService::new(
                &runtime,
                Arc::new(AllowAllWorkspaceIgnorePort),
            );
            Self {
                _temporary: temporary,
                _runtime: runtime,
                _workspace: workspace,
                service,
                root,
            }
        }

        fn deep_parent_with_files(&self) -> WorkspaceRelativePath {
            let mut physical = self.root.clone();
            let mut components = Vec::new();
            for index in 0..24 {
                let component = format!("segment-{index:02}");
                physical.push(&component);
                fs::create_dir(&physical).unwrap();
                components.push(component);
            }
            for index in 0..8 {
                fs::write(
                    physical.join(format!("payload-{index:02}.bin")),
                    vec![index as u8; 4 * 1024],
                )
                .unwrap();
            }
            WorkspaceRelativePath::parse(components.join("/")).unwrap()
        }
    }

    fn with_inventory_test_limits<ResultType>(
        limits: crate::inventory_snapshot::InventorySnapshotLimits,
        force_content_hash: bool,
        operation: impl FnOnce() -> ResultType,
    ) -> (ResultType, usize) {
        super::TEST_INVENTORY_LIMITS.set(Some(limits));
        super::TEST_INVENTORY_CONTENT_READS.set(0);
        super::TEST_FORCE_CONTENT_HASH.set(force_content_hash);
        let result = operation();
        let reads = super::TEST_INVENTORY_CONTENT_READS.get();
        super::TEST_FORCE_CONTENT_HASH.set(false);
        super::TEST_INVENTORY_LIMITS.set(None);
        (result, reads)
    }

    #[tokio::test]
    async fn direct_output_budget_is_rejected_before_inventory_content_reads() {
        use crate::inventory_snapshot::InventorySnapshotLimits;

        let fixture = InventoryFixture::new().await;
        let parent = fixture.deep_parent_with_files();
        let (result, reads) = with_inventory_test_limits(
            InventorySnapshotLimits {
                maximum_nodes: 10_000,
                maximum_content_bytes: 1024 * 1024,
                maximum_metadata_bytes: 6_000,
                maximum_depth: 128,
            },
            false,
            || fixture.service.list_inventory(&parent),
        );

        assert_eq!(
            result.unwrap_err().kind(),
            ResourceServiceErrorKind::Unavailable
        );
        assert_eq!(reads, 0);
    }

    #[tokio::test]
    async fn paged_output_budget_is_rejected_before_inventory_content_reads() {
        use crate::inventory_snapshot::InventorySnapshotLimits;

        let fixture = InventoryFixture::new().await;
        let parent = fixture.deep_parent_with_files();
        let (result, reads) = with_inventory_test_limits(
            InventorySnapshotLimits {
                maximum_nodes: 10_000,
                maximum_content_bytes: 1024 * 1024,
                maximum_metadata_bytes: 9_000,
                maximum_depth: 128,
            },
            false,
            || {
                fixture
                    .service
                    .list_inventory_page(ListWorkspaceInventoryQuery {
                        cursor: None,
                        limit: Some(PageLimit::new(4).unwrap()),
                        parent,
                    })
            },
        );

        assert_eq!(
            result.unwrap_err().kind(),
            ResourceServiceErrorKind::Unavailable
        );
        assert_eq!(reads, 0);
    }

    #[test]
    fn tree_snapshot_digest_preserves_the_legacy_json_manifest_hash_input() {
        let temporary = tempdir().unwrap();
        let directory = Dir::open_ambient_dir(temporary.path(), ambient_authority()).unwrap();
        let path = WorkspaceRelativePath::parse("stable").unwrap();
        let metadata = super::trusted_directory_metadata(&directory).unwrap();
        let mut legacy_manifest = Vec::new();
        if let Ok(directory_snapshot) = InventoryCandidateSnapshot::from_file_stamp(
            path.clone(),
            InventoryCandidateType::Directory,
            FileVersionStamp::capture_metadata(&metadata),
        ) {
            legacy_manifest.push(directory_snapshot);
        }
        let legacy_json = serde_json::to_vec(&legacy_manifest).unwrap();
        let mut legacy_hash = Sha256::new();
        legacy_hash.update(b"qingyu-inventory-tree-snapshot-v1\0");
        legacy_hash.update((legacy_json.len() as u64).to_be_bytes());
        legacy_hash.update(legacy_json);
        let expected = ContentDigest::new(legacy_hash.finalize().into());
        let mut budget = inventory_snapshot_budget();

        let actual = tree_snapshot_digest(&directory, &path, &mut budget).unwrap();

        assert_eq!(actual, expected);
    }

    #[test]
    fn candidate_snapshot_sequence_keeps_the_cursor_json_array_bytes() {
        let snapshot = InventoryCandidateSnapshot::from_file_stamp(
            WorkspaceRelativePath::parse("assets/photo.png").unwrap(),
            InventoryCandidateType::ResourceFile,
            FileVersionStamp::Strong(StrongFileVersionStamp::Unix {
                device: 1,
                inode: 2,
                link_count: 1,
                length: 3,
                modified_seconds: 4,
                modified_nanoseconds: 5,
                changed_seconds: 6,
                changed_nanoseconds: 7,
            }),
        )
        .unwrap();
        let candidates = vec![super::InventoryCandidate {
            name: "photo.png".to_string(),
            snapshot,
        }];

        let serialized =
            serde_json::to_string(&super::InventoryCandidateSnapshots(&candidates)).unwrap();

        assert_eq!(
            serialized,
            r#"[{"formatVersion":2,"logicalPath":"assets/photo.png","entryType":"resource-file","version":{"kind":"strong-file-stamp","stamp":{"platform":"unix","device":1,"inode":2,"linkCount":1,"length":3,"modifiedSeconds":4,"modifiedNanoseconds":5,"changedSeconds":6,"changedNanoseconds":7}}}]"#
        );
    }

    #[test]
    fn high_fanout_metadata_is_rejected_before_candidate_content_reads() {
        use crate::inventory_snapshot::{InventorySnapshotBudget, InventorySnapshotLimits};

        let temporary = tempdir().unwrap();
        for index in 0..16 {
            fs::write(
                temporary.path().join(format!("payload-{index:02}.bin")),
                vec![index as u8; 4 * 1024],
            )
            .unwrap();
        }
        let directory = Dir::open_ambient_dir(temporary.path(), ambient_authority()).unwrap();
        let parent = WorkspaceRelativePath::default();
        let ignore = WorkspaceIgnoreSnapshot::from_matcher(Arc::new(AllowAllDocumentIgnorePort));
        let mut budget = InventorySnapshotBudget::new(InventorySnapshotLimits {
            maximum_nodes: 100,
            maximum_content_bytes: 1024 * 1024,
            maximum_metadata_bytes: 4_096,
            maximum_depth: 100,
        });
        let content_bytes_before = budget.remaining_content_bytes();
        super::TEST_INVENTORY_CONTENT_READS.set(0);
        super::TEST_FORCE_CONTENT_HASH.set(true);

        let error = super::inventory_candidates(
            &directory,
            &parent,
            &ignore,
            &mut budget,
            |_names, _budget| Ok(()),
            |_path, _name, _kind| Ok(()),
        )
        .unwrap_err();
        let reads = super::TEST_INVENTORY_CONTENT_READS.get();
        super::TEST_FORCE_CONTENT_HASH.set(false);

        assert_eq!(error.kind(), ResourceServiceErrorKind::Unavailable);
        assert_eq!(budget.remaining_content_bytes(), content_bytes_before);
        assert_eq!(reads, 0);
    }

    #[test]
    fn tree_rescan_metadata_is_reserved_before_fallback_content_reads() {
        use crate::inventory_snapshot::{InventorySnapshotBudget, InventorySnapshotLimits};

        let temporary = tempdir().unwrap();
        for index in 0..16 {
            fs::write(
                temporary.path().join(format!("payload-{index:02}.bin")),
                vec![index as u8; 4 * 1024],
            )
            .unwrap();
        }
        let directory = Dir::open_ambient_dir(temporary.path(), ambient_authority()).unwrap();
        let mut budget = InventorySnapshotBudget::new(InventorySnapshotLimits {
            maximum_nodes: 100,
            maximum_content_bytes: 1024 * 1024,
            maximum_metadata_bytes: 4_096,
            maximum_depth: 100,
        });
        let content_bytes_before = budget.remaining_content_bytes();
        super::TEST_INVENTORY_CONTENT_READS.set(0);
        super::TEST_FORCE_CONTENT_HASH.set(true);

        let error =
            super::tree_snapshot_digest(&directory, &WorkspaceRelativePath::default(), &mut budget)
                .unwrap_err();
        let reads = super::TEST_INVENTORY_CONTENT_READS.get();
        super::TEST_FORCE_CONTENT_HASH.set(false);

        assert_eq!(error.kind(), ResourceServiceErrorKind::Unavailable);
        assert_eq!(budget.remaining_content_bytes(), content_bytes_before);
        assert_eq!(reads, 0);
    }

    #[test]
    fn unchanged_rescan_does_not_reserve_a_second_candidate_vector() {
        use crate::inventory_snapshot::{InventorySnapshotBudget, InventorySnapshotLimits};

        let temporary = tempdir().unwrap();
        for index in 0..16 {
            fs::write(
                temporary.path().join(format!("payload-{index:02}.bin")),
                vec![index as u8; 4 * 1024],
            )
            .unwrap();
        }
        let directory = Dir::open_ambient_dir(temporary.path(), ambient_authority()).unwrap();
        let parent = WorkspaceRelativePath::default();
        let ignore = WorkspaceIgnoreSnapshot::from_matcher(Arc::new(AllowAllDocumentIgnorePort));
        let mut initial_budget = InventorySnapshotBudget::new(InventorySnapshotLimits {
            maximum_nodes: 100,
            maximum_content_bytes: 1024 * 1024,
            maximum_metadata_bytes: 1024 * 1024,
            maximum_depth: 100,
        });
        let (candidates, verification_names) = super::inventory_candidates(
            &directory,
            &parent,
            &ignore,
            &mut initial_budget,
            |_names, _budget| Ok(()),
            |_path, _name, _kind| Ok(()),
        )
        .unwrap();
        let mut rescan_budget = InventorySnapshotBudget::new(InventorySnapshotLimits {
            maximum_nodes: 100,
            maximum_content_bytes: 1024 * 1024,
            maximum_metadata_bytes: 4_096,
            maximum_depth: 100,
        });

        super::verify_inventory_candidates_unchanged(
            &directory,
            &parent,
            &ignore,
            &mut rescan_budget,
            &candidates,
            verification_names,
        )
        .unwrap();
    }

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
        use crate::inventory_snapshot::{InventorySnapshotBudget, InventorySnapshotLimits};

        let temporary = tempdir().unwrap();
        fs::create_dir(temporary.path().join(".qingyu-first")).unwrap();
        fs::create_dir(temporary.path().join(".qingyu-second")).unwrap();
        let directory = Dir::open_ambient_dir(temporary.path(), ambient_authority()).unwrap();
        let mut budget = InventorySnapshotBudget::new(InventorySnapshotLimits {
            maximum_nodes: 10,
            maximum_content_bytes: 10,
            maximum_metadata_bytes: 1_024,
            maximum_depth: 10,
        });

        let error = super::ordinary_entry_names_with_limit(&directory, 1, &mut budget).unwrap_err();

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
            maximum_content_bytes: 1,
            maximum_metadata_bytes: 1_024,
            maximum_depth: 1,
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

    #[test]
    fn inventory_content_reader_enforces_a_small_injected_budget() {
        use crate::inventory_snapshot::{InventorySnapshotBudget, InventorySnapshotLimits};

        let temporary = tempdir().unwrap();
        fs::write(temporary.path().join("bounded.bin"), b"two").unwrap();
        let directory = Dir::open_ambient_dir(temporary.path(), ambient_authority()).unwrap();
        let addressed = directory.symlink_metadata("bounded.bin").unwrap();
        let mut budget = InventorySnapshotBudget::new(InventorySnapshotLimits {
            maximum_nodes: 1,
            maximum_content_bytes: 2,
            maximum_metadata_bytes: 1_024,
            maximum_depth: 1,
        });

        let result = super::inspect_regular_file_with_budget(
            &directory,
            "bounded.bin",
            &addressed,
            &mut budget,
        );
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("inventory content read must not exceed its injected byte budget"),
        };

        assert_eq!(error.kind(), ResourceServiceErrorKind::Unavailable);
    }

    #[test]
    fn directory_revision_charges_nested_content_to_the_inventory_budget() {
        use crate::{
            documents::service::directory_revision_for_capability_with_inventory_budget,
            inventory_snapshot::{InventorySnapshotBudget, InventorySnapshotLimits},
        };

        let temporary = tempdir().unwrap();
        fs::write(temporary.path().join("nested.bin"), b"nested").unwrap();
        let directory = Dir::open_ambient_dir(temporary.path(), ambient_authority()).unwrap();
        let mut budget = InventorySnapshotBudget::new(InventorySnapshotLimits {
            maximum_nodes: 10,
            maximum_content_bytes: 1,
            maximum_metadata_bytes: 1_024,
            maximum_depth: 10,
        });

        let error =
            directory_revision_for_capability_with_inventory_budget(&directory, &mut budget)
                .unwrap_err();

        assert_eq!(
            error.kind(),
            crate::documents::service::DocumentServiceErrorKind::Unavailable
        );
    }

    #[test]
    fn directory_revision_enforces_the_inventory_depth_budget() {
        use crate::{
            documents::service::directory_revision_for_capability_with_inventory_budget,
            inventory_snapshot::{InventorySnapshotBudget, InventorySnapshotLimits},
        };

        let temporary = tempdir().unwrap();
        fs::create_dir(temporary.path().join("nested")).unwrap();
        let directory = Dir::open_ambient_dir(temporary.path(), ambient_authority()).unwrap();
        let mut budget = InventorySnapshotBudget::new(InventorySnapshotLimits {
            maximum_nodes: 10,
            maximum_content_bytes: 10,
            maximum_metadata_bytes: 1_024,
            maximum_depth: 0,
        });

        let error =
            directory_revision_for_capability_with_inventory_budget(&directory, &mut budget)
                .unwrap_err();

        assert_eq!(
            error.kind(),
            crate::documents::service::DocumentServiceErrorKind::Unavailable
        );
    }
}
