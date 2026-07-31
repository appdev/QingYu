use std::{
    fmt,
    io::{self, Read as _, Seek as _, SeekFrom, Write as _},
    mem,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex, Weak,
    },
};

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine as _};
#[cfg(any(unix, windows))]
use cap_fs_ext::OpenOptionsExt;
use cap_fs_ext::{DirExt, FollowSymlinks, MetadataExt, OpenOptionsFollowExt};
use cap_std::fs::OpenOptions;
use cap_std::fs::{Dir, File, Metadata};
use quick_xml::{
    events::{BytesEnd, BytesStart, BytesText, Event},
    Reader, Writer,
};
use serde::{ser::SerializeSeq as _, Serialize, Serializer};
use sha2::{Digest as _, Sha256};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use crate::{
    contract::{
        CreateWorkspaceResourceBatchRequest, CreateWorkspaceResourceBatchResponse,
        CreateWorkspaceResourceQuery, DocumentEntryDto, DocumentKind, DocumentName, ErrorCode,
        ListWorkspaceInventoryQuery, Nullable, PageCursorContext, ResourceBatchId,
        ResourceEntryDto, ResourceId, ResourceKind, ResourceName, Revision, Rfc3339Utc,
        SafeUnsignedInteger, WorkspaceDto, WorkspaceInventoryEntryDto, WorkspaceInventoryPageDto,
        WorkspaceReadiness, WorkspaceRelativePath,
    },
    documents::service::directory_revision_for_capability_with_inventory_budget,
    documents::service::CapabilityAtomicInstallPort,
    documents::{
        identity::DocumentIdentityCodec, AtomicInstallMode, AtomicInstallPort,
        AtomicInstallRequest, PinnedInstallSource,
    },
    ignore_rules::{WorkspaceIgnorePort, WorkspaceIgnoreSnapshot},
    inventory_snapshot::{
        ContentDigest, FileVersionStamp, InventoryCandidateSnapshot, InventoryCandidateType,
        InventoryModifiedTime, InventorySnapshotBudget, InventorySnapshotLimits,
    },
    runtime::{
        ActiveWorkspaceSnapshot, KernelRuntime, MutationPermit, ResourcesApiService, ServiceFailure,
    },
    storage::nonfollowing_read_options,
};

use super::{
    policy::protected_resource_component,
    transaction::{
        content_digest, request_digest, stage_name, unique_resource_name, BatchPhase, BatchRecord,
        BatchRecordItem, CreateBatchRecordError, ResourceBatchStore, StoredBatchRecord,
    },
    ResourceServiceError,
};

const STREAM_BUFFER_BYTES: usize = 64 * 1024;
const MAX_IMAGE_HEADER_BYTES: usize = 64 * 1024;
const MAX_SVG_BYTES: usize = 4 * 1024 * 1024;
const MAX_IMMEDIATE_INVENTORY_CANDIDATES: usize = 50_000;
const MAX_INVENTORY_PAGE_DIRECTORY_SCANS: u64 = 4;
const MAX_INVENTORY_SNAPSHOT_NODES: u64 =
    MAX_IMMEDIATE_INVENTORY_CANDIDATES as u64 * MAX_INVENTORY_PAGE_DIRECTORY_SCANS;
const MAX_INVENTORY_CONTENT_BYTES: u64 = 512 * 1024 * 1024;
const MAX_INVENTORY_METADATA_BYTES: u64 = 64 * 1024 * 1024;
const MAX_INVENTORY_TREE_DEPTH: usize = 128;
const MAX_CONCURRENT_INVENTORY_SCANS: usize = 2;
const MAX_INVENTORY_REVISION_BYTES: usize = 71;
const MAX_INVENTORY_TIMESTAMP_BYTES: usize = 64;
const MAX_INVENTORY_MEDIA_TYPE_BYTES: usize = "application/octet-stream".len();
pub const MAX_RESOURCE_BODY_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_RESOURCE_BATCH_ITEMS: usize = 32;

#[derive(Clone, Debug)]
pub struct CreateResourceBatchItem {
    pub name: ResourceName,
    pub kind: ResourceKind,
    pub media_type: String,
    pub body: Vec<u8>,
}

impl CreateResourceBatchItem {
    pub fn image(name: ResourceName, media_type: impl Into<String>, body: Vec<u8>) -> Self {
        Self {
            name,
            kind: ResourceKind::Image,
            media_type: media_type.into(),
            body,
        }
    }
}

struct StagedBatchItem {
    file: File,
    record: BatchRecordItem,
}

enum BatchStageError {
    Operation(ResourceServiceError),
    CleanupUncertain,
}

struct ResourceRecoveryGuard<'runtime, 'mutation> {
    runtime: &'runtime KernelRuntime,
    snapshot: &'runtime Arc<ActiveWorkspaceSnapshot>,
    mutation: &'runtime MutationPermit<'mutation>,
    armed: bool,
}

impl<'runtime, 'mutation> ResourceRecoveryGuard<'runtime, 'mutation> {
    fn arm(
        context: &'runtime ResourceContext,
        mutation: &'runtime MutationPermit<'mutation>,
    ) -> Self {
        Self {
            runtime: context.runtime.as_ref(),
            snapshot: &context.snapshot,
            mutation,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ResourceRecoveryGuard<'_, '_> {
    fn drop(&mut self) {
        if self.armed {
            let _closed = self
                .runtime
                .enter_resource_recovery(self.snapshot, self.mutation);
        }
    }
}

const MAX_UNIQUE_RESOURCE_NAMES: usize = 10_000;

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
    atomic_install: Arc<dyn AtomicInstallPort>,
    batch_store: Arc<Mutex<ResourceBatchStore>>,
    recovery_complete: Arc<AtomicBool>,
}

impl WorkspaceResourceService {
    pub fn new(runtime: &Arc<KernelRuntime>, ignore: Arc<dyn WorkspaceIgnorePort>) -> Self {
        Self::open(runtime, ignore).expect("resource batch store opens for an admitted runtime")
    }

    pub(crate) fn open(
        runtime: &Arc<KernelRuntime>,
        ignore: Arc<dyn WorkspaceIgnorePort>,
    ) -> Result<Self, ResourceServiceError> {
        Ok(Self {
            runtime: Arc::downgrade(runtime),
            ignore,
            inventory_scans: Arc::new(InventoryScanGate::default()),
            atomic_install: Arc::new(CapabilityAtomicInstallPort),
            batch_store: Arc::new(Mutex::new(ResourceBatchStore::open(runtime)?)),
            recovery_complete: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn new_with_atomic_install(
        runtime: &Arc<KernelRuntime>,
        ignore: Arc<dyn WorkspaceIgnorePort>,
        atomic_install: Arc<dyn AtomicInstallPort>,
    ) -> Self {
        Self {
            runtime: Arc::downgrade(runtime),
            ignore,
            inventory_scans: Arc::new(InventoryScanGate::default()),
            atomic_install,
            batch_store: Arc::new(Mutex::new(
                ResourceBatchStore::open_isolated(runtime)
                    .expect("isolated resource batch store opens"),
            )),
            recovery_complete: Arc::new(AtomicBool::new(false)),
        }
    }

    #[doc(hidden)]
    pub fn open_with_atomic_install(
        runtime: &Arc<KernelRuntime>,
        ignore: Arc<dyn WorkspaceIgnorePort>,
        atomic_install: Arc<dyn AtomicInstallPort>,
    ) -> Result<Self, ResourceServiceError> {
        Ok(Self {
            runtime: Arc::downgrade(runtime),
            ignore,
            inventory_scans: Arc::new(InventoryScanGate::default()),
            atomic_install,
            batch_store: Arc::new(Mutex::new(ResourceBatchStore::open(runtime)?)),
            recovery_complete: Arc::new(AtomicBool::new(false)),
        })
    }

    pub async fn create_resource(
        &self,
        document_id: &crate::contract::DocumentId,
        query: CreateWorkspaceResourceQuery,
        media_type: &str,
        body: &[u8],
    ) -> Result<ResourceEntryDto, ResourceServiceError> {
        if body.len() > MAX_RESOURCE_BODY_BYTES {
            return Err(ResourceServiceError::too_large());
        }
        let body = validate_resource_payload(query.kind, query.name.as_str(), media_type, body)?;
        let runtime = self
            .runtime
            .upgrade()
            .ok_or_else(ResourceServiceError::unavailable)?;
        let mutation = runtime.mutation_coordinator().lock().await;
        runtime
            .verify_document_mutation_admission(&mutation)
            .map_err(|_| ResourceServiceError::unavailable())?;
        let context = self.context_with_runtime(runtime.clone())?;
        if context.workspace().generation != query.workspace_generation {
            return Err(ResourceServiceError::stale_workspace());
        }
        let document_path = DocumentIdentityCodec::new(context.runtime.wire_identity_key())
            .verify(document_id, context.workspace(), DocumentKind::File)
            .map_err(|_| ResourceServiceError::not_found())?;
        self.verify_document_target(&context, &document_path)?;
        let ignore = self.capture_ignore(&context)?;
        if ignore.is_ignored(&document_path, DocumentKind::File) {
            return Err(ResourceServiceError::not_found());
        }
        let document_parent = parent_and_name(&document_path)?.0;
        let target_parent = join_paths(&document_parent, &query.folder)?;
        validate_resource_parent(&target_parent, &ignore)?;
        let (directory, created_directories) =
            create_resource_parent(&context.root, &target_parent)?;
        let result = self.install_unique_resource(
            &context,
            &directory,
            &target_parent,
            &query.name,
            query.kind,
            &body,
            &ignore,
        );
        if result.is_err() {
            rollback_created_directories(&context.root, &created_directories);
        }
        result
    }

    pub async fn create_resource_batch(
        &self,
        batch_id: ResourceBatchId,
        document_id: &crate::contract::DocumentId,
        workspace_generation: crate::contract::WorkspaceGeneration,
        folder: WorkspaceRelativePath,
        items: Vec<CreateResourceBatchItem>,
    ) -> Result<Vec<ResourceEntryDto>, ResourceServiceError> {
        if items.is_empty() || items.len() > MAX_RESOURCE_BATCH_ITEMS {
            return Err(ResourceServiceError::invalid_path());
        }
        let total_bytes = items.iter().try_fold(0_usize, |total, item| {
            if item.body.len() > MAX_RESOURCE_BODY_BYTES {
                return Err(ResourceServiceError::too_large());
            }
            total
                .checked_add(item.body.len())
                .filter(|total| *total <= MAX_RESOURCE_BODY_BYTES)
                .ok_or_else(ResourceServiceError::too_large)
        })?;
        debug_assert!(total_bytes <= MAX_RESOURCE_BODY_BYTES);
        let mut normalized = Vec::with_capacity(items.len());
        for item in items {
            let body = validate_resource_payload(
                item.kind,
                item.name.as_str(),
                &item.media_type,
                &item.body,
            )?;
            normalized.push(CreateResourceBatchItem { body, ..item });
        }

        let runtime = self
            .runtime
            .upgrade()
            .ok_or_else(ResourceServiceError::unavailable)?;
        let mutation = runtime.mutation_coordinator().lock().await;
        runtime
            .verify_resource_batch_mutation_admission(&mutation)
            .map_err(|_| ResourceServiceError::unavailable())?;
        let context = self.context_with_runtime(runtime.clone())?;
        if context.workspace().generation != workspace_generation {
            return Err(ResourceServiceError::stale_workspace());
        }
        let document_path = DocumentIdentityCodec::new(context.runtime.wire_identity_key())
            .verify(document_id, context.workspace(), DocumentKind::File)
            .map_err(|_| ResourceServiceError::not_found())?;
        self.verify_document_target(&context, &document_path)?;
        let ignore = self.capture_ignore(&context)?;
        if ignore.is_ignored(&document_path, DocumentKind::File) {
            return Err(ResourceServiceError::not_found());
        }
        let document_parent = parent_and_name(&document_path)?.0;
        let target_parent = join_paths(&document_parent, &folder)?;
        validate_resource_parent(&target_parent, &ignore)?;
        let _publication = context
            .runtime
            .workspace_publication_gate()
            .write()
            .map_err(|_| ResourceServiceError::unavailable())?;
        let mut store = self
            .batch_store
            .lock()
            .map_err(|_| ResourceServiceError::unavailable())?;
        self.recover_pending_locked(&context, &mut store, &mutation)?;
        let digest = request_digest(
            context.workspace().id,
            &context.workspace().generation,
            &document_path,
            &folder,
            &normalized,
        );
        let existing = match store.load(batch_id) {
            Ok(existing) => existing,
            Err(_) => {
                let _closed = context
                    .runtime
                    .enter_resource_recovery(&context.snapshot, &mutation);
                return Err(ResourceServiceError::unavailable());
            }
        };
        if let Some(stored) = existing {
            if stored.record.request_digest != digest {
                return Err(ResourceServiceError::conflict());
            }
            match stored.record.phase {
                BatchPhase::Committed => {
                    return self.resources_from_record_or_close(&context, &stored.record, &mutation)
                }
                BatchPhase::Preparing | BatchPhase::Prepared => {
                    self.recover_record(&context, &mut store, stored, &mutation)?;
                    let committed = match store.load(batch_id) {
                        Ok(Some(committed)) => committed,
                        _ => {
                            let _closed = context
                                .runtime
                                .enter_resource_recovery(&context.snapshot, &mutation);
                            return Err(ResourceServiceError::unavailable());
                        }
                    };
                    if committed.record.phase == BatchPhase::Committed {
                        return self.resources_from_record_or_close(
                            &context,
                            &committed.record,
                            &mutation,
                        );
                    }
                }
                BatchPhase::Aborted => {}
            }
        }
        self.install_resource_batch(
            &context,
            &mut store,
            &mutation,
            batch_id,
            digest,
            document_path,
            folder,
            &target_parent,
            normalized,
            &ignore,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn install_resource_batch(
        &self,
        context: &ResourceContext,
        store: &mut ResourceBatchStore,
        mutation: &MutationPermit<'_>,
        batch_id: ResourceBatchId,
        digest: String,
        document_path: WorkspaceRelativePath,
        folder: WorkspaceRelativePath,
        parent: &WorkspaceRelativePath,
        items: Vec<CreateResourceBatchItem>,
        ignore: &WorkspaceIgnoreSnapshot,
    ) -> Result<Vec<ResourceEntryDto>, ResourceServiceError> {
        let attempt_id = uuid::Uuid::new_v4();
        let existing_directory = open_record_directory(&context.root, parent)?;
        let mut reserved = std::collections::HashSet::new();
        let mut planned = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            let mut selected = None;
            for attempt in 0..MAX_UNIQUE_RESOURCE_NAMES {
                let candidate = unique_resource_name(item.name.as_str(), attempt)?;
                let reservation = candidate.as_str().to_lowercase();
                if reserved.contains(&reservation) {
                    continue;
                }
                if let Some(directory) = existing_directory.as_ref() {
                    match directory.symlink_metadata(candidate.as_str()) {
                        Ok(_) => continue,
                        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                        Err(_) => return Err(ResourceServiceError::unavailable()),
                    }
                }
                let path = join_relative(parent, candidate.as_str())?;
                if ignore.is_ignored(&path, DocumentKind::File) {
                    return Err(ResourceServiceError::invalid_path());
                }
                reserved.insert(reservation);
                selected = Some((attempt, candidate, path));
                break;
            }
            let Some((attempt, target_name, path)) = selected else {
                return Err(ResourceServiceError::unavailable());
            };
            planned.push(BatchRecordItem {
                ordinal: u8::try_from(index).map_err(|_| ResourceServiceError::unavailable())?,
                name_attempt: u32::try_from(attempt)
                    .map_err(|_| ResourceServiceError::unavailable())?,
                requested_name: item.name.clone(),
                target_name,
                target_path: path,
                kind: item.kind,
                media_type: item.media_type.clone(),
                size_bytes: item.body.len() as u64,
                content_sha256: content_digest(&item.body),
                stage_name: stage_name(attempt_id, index),
            });
        }
        let mut record = BatchRecord::preparing(
            batch_id,
            digest,
            context.workspace().id,
            context.workspace().generation.clone(),
            document_path,
            folder,
            parent.clone(),
            attempt_id,
            planned,
        );
        store.preflight_record(&record)?;
        let mut stored = match store.load(batch_id) {
            Err(_) => {
                let _closed = context
                    .runtime
                    .enter_resource_recovery(&context.snapshot, mutation);
                return Err(ResourceServiceError::unavailable());
            }
            Ok(Some(previous)) if previous.record.phase == BatchPhase::Aborted => {
                match store.replace(&record, &previous.revision) {
                    Ok(stored) => stored,
                    Err(_) => {
                        let _closed = context
                            .runtime
                            .enter_resource_recovery(&context.snapshot, mutation);
                        return Err(ResourceServiceError::unavailable());
                    }
                }
            }
            Ok(Some(_)) => return Err(ResourceServiceError::conflict()),
            Ok(None) if !store.has_capacity_for_new_batch()? => {
                return Err(ResourceServiceError::unavailable())
            }
            Ok(None) => match store.create(&record) {
                Ok(stored) => stored,
                Err(CreateBatchRecordError::RecoveryRequired(error)) => {
                    let _closed = context
                        .runtime
                        .enter_resource_recovery(&context.snapshot, mutation);
                    return Err(error);
                }
                Err(CreateBatchRecordError::NotPublished(error)) => return Err(error),
            },
        };
        let mut recovery = ResourceRecoveryGuard::arm(context, mutation);
        let (directory, created_directories) = match create_resource_parent(&context.root, parent) {
            Ok(created) => created,
            Err(error) => return Err(error),
        };
        for item in &record.items {
            match directory.symlink_metadata(item.target_name.as_str()) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Ok(_) => {
                    drop(directory);
                    rollback_created_directories(&context.root, &created_directories);
                    record.phase = BatchPhase::Aborted;
                    match store.replace(&record, &stored.revision) {
                        Ok(aborted) => {
                            store.finalize_commit(batch_id, &aborted.revision)?;
                            recovery.disarm();
                        }
                        Err(_) => return Err(ResourceServiceError::unavailable()),
                    }
                    return Err(ResourceServiceError::conflict());
                }
                Err(_) => return Err(ResourceServiceError::unavailable()),
            }
        }

        let mut staged: Vec<StagedBatchItem> = Vec::with_capacity(items.len());
        for (item, planned) in items.into_iter().zip(record.items.iter().cloned()) {
            let file = match stage_batch_resource(&directory, &planned.stage_name, &item.body) {
                Ok(file) => file,
                Err(BatchStageError::CleanupUncertain) => {
                    let _closed = context
                        .runtime
                        .enter_resource_recovery(&context.snapshot, mutation);
                    return Err(ResourceServiceError::unavailable());
                }
                Err(BatchStageError::Operation(error)) => {
                    if self.cleanup_staged_batch(&directory, &staged).is_err() {
                        let _closed = context
                            .runtime
                            .enter_resource_recovery(&context.snapshot, mutation);
                        return Err(ResourceServiceError::unavailable());
                    }
                    drop(staged);
                    drop(directory);
                    rollback_created_directories(&context.root, &created_directories);
                    record.phase = BatchPhase::Aborted;
                    match store.replace(&record, &stored.revision) {
                        Ok(aborted) => {
                            store.finalize_commit(batch_id, &aborted.revision)?;
                            recovery.disarm();
                        }
                        Err(_) => return Err(ResourceServiceError::unavailable()),
                    }
                    return Err(error);
                }
            };
            staged.push(StagedBatchItem {
                file,
                record: planned,
            });
        }
        if crate::storage::sync_directory(&directory).is_err() {
            if self.cleanup_staged_batch(&directory, &staged).is_err() {
                let _closed = context
                    .runtime
                    .enter_resource_recovery(&context.snapshot, mutation);
                return Err(ResourceServiceError::unavailable());
            }
            drop(staged);
            drop(directory);
            rollback_created_directories(&context.root, &created_directories);
            record.phase = BatchPhase::Aborted;
            match store.replace(&record, &stored.revision) {
                Ok(aborted) => {
                    store.finalize_commit(batch_id, &aborted.revision)?;
                    recovery.disarm();
                }
                Err(_) => return Err(ResourceServiceError::unavailable()),
            }
            return Err(ResourceServiceError::unavailable());
        }
        record.phase = BatchPhase::Prepared;
        stored = store.replace(&record, &stored.revision)?;

        for item in &staged {
            let install = self.atomic_install.install(AtomicInstallRequest {
                directory: &directory,
                target: &item.record.target_path,
                stage_name: &item.record.stage_name,
                target_name: item.record.target_name.as_str(),
                mode: AtomicInstallMode::CreateNoReplace,
                expected_stage: PinnedInstallSource::File(&item.file),
                expected_target: None,
                expected_revision: None,
            });
            if install.is_err() {
                if !self.record_item_is_published(&directory, &item.record)? {
                    let _closed = context
                        .runtime
                        .enter_resource_recovery(&context.snapshot, mutation);
                    return Err(ResourceServiceError::unavailable());
                }
            }
        }
        if crate::storage::sync_directory(&directory).is_err() {
            let _closed = context
                .runtime
                .enter_resource_recovery(&context.snapshot, mutation);
            return Err(ResourceServiceError::unavailable());
        }
        record.phase = BatchPhase::Committed;
        stored = store.replace(&record, &stored.revision)?;
        let resources = self.resources_from_record(context, &record)?;
        store.finalize_commit(batch_id, &stored.revision)?;
        recovery.disarm();
        Ok(resources)
    }

    #[doc(hidden)]
    pub async fn recover_pending(&self) -> Result<(), ResourceServiceError> {
        let runtime = self
            .runtime
            .upgrade()
            .ok_or_else(ResourceServiceError::unavailable)?;
        let mutation = runtime.mutation_coordinator().lock().await;
        runtime
            .verify_resource_batch_mutation_admission(&mutation)
            .map_err(|_| ResourceServiceError::unavailable())?;
        let context = self.context_with_runtime(runtime.clone())?;
        let _publication = context
            .runtime
            .workspace_publication_gate()
            .write()
            .map_err(|_| ResourceServiceError::unavailable())?;
        let mut store = self
            .batch_store
            .lock()
            .map_err(|_| ResourceServiceError::unavailable())?;
        self.recover_pending_locked(&context, &mut store, &mutation)
    }

    fn recover_pending_locked(
        &self,
        context: &ResourceContext,
        store: &mut ResourceBatchStore,
        mutation: &MutationPermit<'_>,
    ) -> Result<(), ResourceServiceError> {
        if self.recovery_complete.load(Ordering::Acquire) {
            return Ok(());
        }
        let records = match store.pending_records() {
            Ok(records) => records,
            Err(error) => {
                let _closed = context
                    .runtime
                    .enter_resource_recovery(&context.snapshot, mutation);
                return Err(error);
            }
        };
        for record in records {
            if let Err(error) = self.recover_record(context, store, record, mutation) {
                let _closed = context
                    .runtime
                    .enter_resource_recovery(&context.snapshot, mutation);
                return Err(error);
            }
        }
        self.recovery_complete.store(true, Ordering::Release);
        Ok(())
    }

    fn recover_record(
        &self,
        context: &ResourceContext,
        store: &mut ResourceBatchStore,
        mut stored: StoredBatchRecord,
        mutation: &MutationPermit<'_>,
    ) -> Result<(), ResourceServiceError> {
        let mut recovery = ResourceRecoveryGuard::arm(context, mutation);
        let result = (|| {
            if stored.record.workspace_id != context.workspace().id
                || stored.record.workspace_generation != context.workspace().generation
            {
                return Err(ResourceServiceError::unavailable());
            }
            let directory = open_record_directory(&context.root, &stored.record.target_parent)?;
            match stored.record.phase {
                BatchPhase::Preparing => {
                    if let Some(directory) = directory.as_ref() {
                        for item in &stored.record.items {
                            match directory.symlink_metadata(item.target_name.as_str()) {
                                Ok(_) => return Err(ResourceServiceError::unsafe_target()),
                                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                                Err(_) => return Err(ResourceServiceError::unavailable()),
                            }
                            self.remove_record_stage_if_present(directory, item)?;
                        }
                        crate::storage::sync_directory(directory)
                            .map_err(|_| ResourceServiceError::unavailable())?;
                    }
                    stored.record.phase = BatchPhase::Aborted;
                    stored = store.replace(&stored.record, &stored.revision)?;
                    store.finalize_commit(stored.record.batch_id, &stored.revision)?;
                    Ok(())
                }
                BatchPhase::Prepared => {
                    let directory = directory.ok_or_else(ResourceServiceError::unavailable)?;
                    for item in &stored.record.items {
                        let stage = self.record_file(&directory, &item.stage_name, item)?;
                        let target =
                            self.record_file(&directory, item.target_name.as_str(), item)?;
                        match (stage, target) {
                            (Some(stage), None) => {
                                let install = self.atomic_install.install(AtomicInstallRequest {
                                    directory: &directory,
                                    target: &item.target_path,
                                    stage_name: &item.stage_name,
                                    target_name: item.target_name.as_str(),
                                    mode: AtomicInstallMode::CreateNoReplace,
                                    expected_stage: PinnedInstallSource::File(&stage.file),
                                    expected_target: None,
                                    expected_revision: None,
                                });
                                if install.is_err()
                                    && !self.record_item_is_published(&directory, item)?
                                {
                                    return Err(ResourceServiceError::unavailable());
                                }
                            }
                            (None, Some(_target)) => {}
                            (None, None) | (Some(_), Some(_)) => {
                                return Err(ResourceServiceError::unsafe_target());
                            }
                        }
                    }
                    crate::storage::sync_directory(&directory)
                        .map_err(|_| ResourceServiceError::unavailable())?;
                    stored.record.phase = BatchPhase::Committed;
                    stored = store.replace(&stored.record, &stored.revision)?;
                    self.resources_from_record(context, &stored.record)?;
                    store.finalize_commit(stored.record.batch_id, &stored.revision)?;
                    Ok(())
                }
                BatchPhase::Committed => {
                    self.resources_from_record(context, &stored.record)?;
                    store.finalize_commit(stored.record.batch_id, &stored.revision)
                }
                BatchPhase::Aborted => {
                    if let Some(directory) = directory.as_ref() {
                        for item in &stored.record.items {
                            if self
                                .record_file(directory, &item.stage_name, item)?
                                .is_some()
                            {
                                return Err(ResourceServiceError::unsafe_target());
                            }
                        }
                    }
                    store.finalize_commit(stored.record.batch_id, &stored.revision)
                }
            }
        })();
        if result.is_ok() {
            recovery.disarm();
        }
        result
    }

    fn cleanup_staged_batch(
        &self,
        directory: &Dir,
        staged: &[StagedBatchItem],
    ) -> Result<(), ResourceServiceError> {
        for item in staged {
            let addressed = directory
                .symlink_metadata(&item.record.stage_name)
                .map_err(|_| ResourceServiceError::unavailable())?;
            let retained = item
                .file
                .metadata()
                .map_err(|_| ResourceServiceError::unavailable())?;
            if !trusted_regular_file(&addressed)
                || !trusted_regular_file(&retained)
                || !same_file(&addressed, &retained)
            {
                return Err(ResourceServiceError::unsafe_target());
            }
            directory
                .remove_file(&item.record.stage_name)
                .map_err(|_| ResourceServiceError::unavailable())?;
        }
        crate::storage::sync_directory(directory)
            .map_err(|_| ResourceServiceError::unavailable())?;
        Ok(())
    }

    fn remove_record_stage_if_present(
        &self,
        directory: &Dir,
        item: &BatchRecordItem,
    ) -> Result<(), ResourceServiceError> {
        match self.record_file(directory, &item.stage_name, item)? {
            Some(_) => directory
                .remove_file(&item.stage_name)
                .map_err(|_| ResourceServiceError::unavailable()),
            None => Ok(()),
        }
    }

    fn record_item_is_published(
        &self,
        directory: &Dir,
        item: &BatchRecordItem,
    ) -> Result<bool, ResourceServiceError> {
        let stage = self.record_file(directory, &item.stage_name, item)?;
        let target = self.record_file(directory, item.target_name.as_str(), item)?;
        Ok(stage.is_none() && target.is_some())
    }

    fn record_file(
        &self,
        directory: &Dir,
        name: &str,
        item: &BatchRecordItem,
    ) -> Result<Option<InspectedFile>, ResourceServiceError> {
        let addressed = match directory.symlink_metadata(name) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(ResourceServiceError::unavailable()),
        };
        let inspected = inspect_regular_file(directory, name, &addressed)?;
        if inspected.metadata.len() != item.size_bytes
            || encoded_content_digest(inspected.content_digest) != item.content_sha256
        {
            return Err(ResourceServiceError::unsafe_target());
        }
        Ok(Some(inspected))
    }

    fn resources_from_record(
        &self,
        context: &ResourceContext,
        record: &BatchRecord,
    ) -> Result<Vec<ResourceEntryDto>, ResourceServiceError> {
        if record.phase != BatchPhase::Committed
            || record.workspace_id != context.workspace().id
            || record.workspace_generation != context.workspace().generation
        {
            return Err(ResourceServiceError::unavailable());
        }
        let directory = open_directory(&context.root, &record.target_parent)?;
        record
            .items
            .iter()
            .map(|item| {
                let inspected = self
                    .record_file(&directory, item.target_name.as_str(), item)?
                    .ok_or_else(ResourceServiceError::unavailable)?;
                match directory.symlink_metadata(&item.stage_name) {
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Ok(_) => return Err(ResourceServiceError::unsafe_target()),
                    Err(_) => return Err(ResourceServiceError::unavailable()),
                }
                let classification = classify_resource(
                    item.target_name.as_str(),
                    &inspected.header,
                    inspected.metadata.len(),
                    inspected.validation_body.as_deref(),
                );
                if classification.kind != item.kind || classification.media_type != item.media_type
                {
                    return Err(ResourceServiceError::unsafe_target());
                }
                let id = context
                    .runtime
                    .wire_identity_key()
                    .issue_resource_id(
                        context.workspace().id,
                        &context.workspace().generation,
                        item.kind,
                        &item.target_path,
                    )
                    .map_err(|_| ResourceServiceError::unavailable())?;
                Ok(ResourceEntryDto {
                    id,
                    path: item.target_path.clone(),
                    parent: record.target_parent.clone(),
                    name: item.target_name.clone(),
                    kind: item.kind,
                    size_bytes: safe_size(inspected.metadata.len())?,
                    modified_at: modified_utc(&inspected.metadata)?,
                    revision: inspected.revision,
                    media_type: classification.media_type.to_string(),
                    previewable: classification.previewable,
                })
            })
            .collect()
    }

    fn resources_from_record_or_close(
        &self,
        context: &ResourceContext,
        record: &BatchRecord,
        mutation: &MutationPermit<'_>,
    ) -> Result<Vec<ResourceEntryDto>, ResourceServiceError> {
        self.resources_from_record(context, record)
            .map_err(|error| {
                let _closed = context
                    .runtime
                    .enter_resource_recovery(&context.snapshot, mutation);
                error
            })
    }

    /// Lists one directory level from the retained active workspace capability.
    /// Documents and directories remain document DTOs; binary entries use the
    /// separate resource identity and DTO contract.
    pub fn list_inventory(
        &self,
        parent: &WorkspaceRelativePath,
    ) -> Result<Vec<WorkspaceInventoryEntry>, ResourceServiceError> {
        let runtime = self
            .runtime
            .upgrade()
            .ok_or_else(ResourceServiceError::unavailable)?;
        let _publication = runtime
            .workspace_publication_gate()
            .read()
            .map_err(|_| ResourceServiceError::unavailable())?;
        let _permit = self.inventory_scans.try_acquire()?;
        let context = self.context_with_runtime(runtime.clone())?;
        let mut budget = inventory_snapshot_budget();
        self.list_inventory_with_context(&context, parent, &mut budget)
    }

    pub fn list_inventory_page(
        &self,
        query: ListWorkspaceInventoryQuery,
    ) -> Result<WorkspaceInventoryPageDto, ResourceServiceError> {
        let runtime = self
            .runtime
            .upgrade()
            .ok_or_else(ResourceServiceError::unavailable)?;
        let _publication = runtime
            .workspace_publication_gate()
            .read()
            .map_err(|_| ResourceServiceError::unavailable())?;
        let _permit = self.inventory_scans.try_acquire()?;
        let context = self.context_with_runtime(runtime.clone())?;
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
                    &query.parent,
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
            parent,
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
        let runtime = self
            .runtime
            .upgrade()
            .ok_or_else(ResourceServiceError::unavailable)?;
        let _publication = runtime
            .workspace_publication_gate()
            .read()
            .map_err(|_| ResourceServiceError::unavailable())?;
        let context = self.context_with_runtime(runtime.clone())?;
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
        let classification = classify_resource(
            &name,
            &inspected.header,
            inspected.metadata.len(),
            inspected.validation_body.as_deref(),
        );
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

    #[cfg(test)]
    fn context(&self) -> Result<ResourceContext, ResourceServiceError> {
        let runtime = self
            .runtime
            .upgrade()
            .ok_or_else(ResourceServiceError::unavailable)?;
        self.context_with_runtime(runtime)
    }

    fn context_with_runtime(
        &self,
        runtime: Arc<KernelRuntime>,
    ) -> Result<ResourceContext, ResourceServiceError> {
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

    fn verify_document_target(
        &self,
        context: &ResourceContext,
        path: &WorkspaceRelativePath,
    ) -> Result<(), ResourceServiceError> {
        let (parent_path, name) = parent_and_name(path)?;
        if !markdown_name(&name) {
            return Err(ResourceServiceError::not_found());
        }
        let parent = open_directory(&context.root, &parent_path)?;
        let addressed = parent.symlink_metadata(&name).map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                ResourceServiceError::not_found()
            } else {
                ResourceServiceError::unavailable()
            }
        })?;
        let _inspected = inspect_regular_file(&parent, &name, &addressed)?;
        Ok(())
    }

    fn install_unique_resource(
        &self,
        context: &ResourceContext,
        directory: &Dir,
        parent: &WorkspaceRelativePath,
        requested_name: &ResourceName,
        kind: ResourceKind,
        body: &[u8],
        ignore: &WorkspaceIgnoreSnapshot,
    ) -> Result<ResourceEntryDto, ResourceServiceError> {
        let stage_name = random_resource_stage_name()?;
        let staged = stage_resource(directory, &stage_name, body)?;
        let staged_metadata = match staged.metadata() {
            Ok(metadata) => metadata,
            Err(_) => {
                cleanup_staged_resource(directory, &stage_name);
                return Err(ResourceServiceError::unavailable());
            }
        };
        for attempt in 0..MAX_UNIQUE_RESOURCE_NAMES {
            let name = match unique_resource_name(requested_name.as_str(), attempt) {
                Ok(name) => name,
                Err(error) => {
                    cleanup_staged_resource(directory, &stage_name);
                    return Err(error);
                }
            };
            if protected_resource_component(name.as_str()) || markdown_name(name.as_str()) {
                cleanup_staged_resource(directory, &stage_name);
                return Err(ResourceServiceError::invalid_path());
            }
            match directory.symlink_metadata(name.as_str()) {
                Ok(_) => continue,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(_) => {
                    cleanup_staged_resource(directory, &stage_name);
                    return Err(ResourceServiceError::unavailable());
                }
            }
            let path = match join_relative(parent, name.as_str()) {
                Ok(path) => path,
                Err(error) => {
                    cleanup_staged_resource(directory, &stage_name);
                    return Err(error);
                }
            };
            if ignore.is_ignored(&path, DocumentKind::File) {
                cleanup_staged_resource(directory, &stage_name);
                return Err(ResourceServiceError::invalid_path());
            }
            let install = self.atomic_install.install(AtomicInstallRequest {
                directory,
                target: &path,
                stage_name: &stage_name,
                target_name: name.as_str(),
                mode: AtomicInstallMode::CreateNoReplace,
                expected_stage: PinnedInstallSource::File(&staged),
                expected_target: None,
                expected_revision: None,
            });
            if install.is_err() {
                let stage_still_exists = directory.symlink_metadata(&stage_name).is_ok();
                let installed = directory.symlink_metadata(name.as_str()).ok();
                if stage_still_exists && installed.is_some() {
                    continue;
                }
                let settled = !stage_still_exists
                    && installed
                        .as_ref()
                        .is_some_and(|metadata| same_file(&staged_metadata, metadata));
                if !settled {
                    cleanup_staged_resource(directory, &stage_name);
                    return Err(ResourceServiceError::unavailable());
                }
            }
            let result = (|| {
                crate::storage::sync_directory(directory)
                    .map_err(|_| ResourceServiceError::unavailable())?;
                let addressed = directory
                    .symlink_metadata(name.as_str())
                    .map_err(|_| ResourceServiceError::unavailable())?;
                let inspected = inspect_regular_file(directory, name.as_str(), &addressed)?;
                if !same_file(&staged_metadata, &inspected.metadata) {
                    return Err(ResourceServiceError::unsafe_target());
                }
                let classification = classify_resource(
                    name.as_str(),
                    &inspected.header,
                    inspected.metadata.len(),
                    inspected.validation_body.as_deref(),
                );
                if classification.kind != kind {
                    return Err(ResourceServiceError::unsafe_target());
                }
                context
                    .snapshot
                    .authority()
                    .verify_held_directory()
                    .map_err(|_| ResourceServiceError::unavailable())?;
                let id = context
                    .runtime
                    .wire_identity_key()
                    .issue_resource_id(
                        context.workspace().id,
                        &context.workspace().generation,
                        kind,
                        &path,
                    )
                    .map_err(|_| ResourceServiceError::unavailable())?;
                Ok(ResourceEntryDto {
                    id,
                    path,
                    parent: parent.clone(),
                    name: name.clone(),
                    kind,
                    size_bytes: safe_size(inspected.metadata.len())?,
                    modified_at: modified_utc(&inspected.metadata)?,
                    revision: inspected.revision,
                    media_type: classification.media_type.to_string(),
                    previewable: classification.previewable,
                })
            })();
            if result.is_err() {
                cleanup_installed_resource(directory, name.as_str(), &staged_metadata);
            }
            return result;
        }
        cleanup_staged_resource(directory, &stage_name);
        Err(ResourceServiceError::unavailable())
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

fn validate_resource_payload(
    kind: ResourceKind,
    name: &str,
    media_type: &str,
    body: &[u8],
) -> Result<Vec<u8>, ResourceServiceError> {
    if protected_resource_component(name) || markdown_name(name) {
        return Err(ResourceServiceError::invalid_path());
    }
    let normalized = if name.to_ascii_lowercase().ends_with(".svg") {
        normalize_static_svg(body)?
    } else {
        body.to_vec()
    };
    let classification = classify_resource(
        name,
        &normalized,
        normalized.len() as u64,
        full_validation_limit(name).map(|_| normalized.as_slice()),
    );
    let valid = match kind {
        ResourceKind::Image => {
            classification.kind == ResourceKind::Image && media_type == classification.media_type
        }
        ResourceKind::Attachment => {
            classification.kind == ResourceKind::Attachment
                && media_type == "application/octet-stream"
        }
    };
    if !valid {
        return Err(ResourceServiceError::invalid_media_type());
    }
    Ok(normalized)
}

fn join_paths(
    parent: &WorkspaceRelativePath,
    child: &WorkspaceRelativePath,
) -> Result<WorkspaceRelativePath, ResourceServiceError> {
    if child.as_str().is_empty() {
        return Ok(parent.clone());
    }
    WorkspaceRelativePath::parse(if parent.as_str().is_empty() {
        child.as_str().to_string()
    } else {
        format!("{}/{}", parent.as_str(), child.as_str())
    })
    .map_err(|_| ResourceServiceError::invalid_path())
}

fn validate_resource_parent(
    parent: &WorkspaceRelativePath,
    ignore: &WorkspaceIgnoreSnapshot,
) -> Result<(), ResourceServiceError> {
    let mut current = WorkspaceRelativePath::default();
    for component in parent
        .as_str()
        .split('/')
        .filter(|component| !component.is_empty())
    {
        if protected_resource_component(component) {
            return Err(ResourceServiceError::invalid_path());
        }
        ResourceName::parse(component).map_err(|_| ResourceServiceError::invalid_path())?;
        current = join_relative(&current, component)?;
        if ignore.is_ignored(&current, DocumentKind::Directory) {
            return Err(ResourceServiceError::invalid_path());
        }
    }
    Ok(())
}

fn create_resource_parent(
    root: &Dir,
    parent: &WorkspaceRelativePath,
) -> Result<(Dir, Vec<WorkspaceRelativePath>), ResourceServiceError> {
    let mut directory = root
        .try_clone()
        .map_err(|_| ResourceServiceError::unavailable())?;
    let mut current = WorkspaceRelativePath::default();
    let mut created = Vec::new();
    for component in parent
        .as_str()
        .split('/')
        .filter(|component| !component.is_empty())
    {
        current = join_relative(&current, component)?;
        let addressed = match directory.symlink_metadata(component) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if let Err(error) = directory.create_dir(component) {
                    rollback_created_directories(root, &created);
                    return Err(if error.kind() == io::ErrorKind::AlreadyExists {
                        ResourceServiceError::unsafe_target()
                    } else {
                        ResourceServiceError::unavailable()
                    });
                }
                if crate::storage::sync_directory(&directory).is_err() {
                    rollback_created_directories(root, &created);
                    return Err(ResourceServiceError::unavailable());
                }
                created.push(current.clone());
                match directory.symlink_metadata(component) {
                    Ok(metadata) => metadata,
                    Err(_) => {
                        rollback_created_directories(root, &created);
                        return Err(ResourceServiceError::unavailable());
                    }
                }
            }
            Err(_) => {
                rollback_created_directories(root, &created);
                return Err(ResourceServiceError::unavailable());
            }
        };
        if !addressed.is_dir() || addressed.file_type().is_symlink() {
            rollback_created_directories(root, &created);
            return Err(ResourceServiceError::unsafe_target());
        }
        let child = match directory.open_dir_nofollow(component) {
            Ok(child) => child,
            Err(_) => {
                rollback_created_directories(root, &created);
                return Err(ResourceServiceError::unsafe_target());
            }
        };
        let retained = match trusted_directory_metadata(&child) {
            Ok(metadata) => metadata,
            Err(error) => {
                rollback_created_directories(root, &created);
                return Err(error);
            }
        };
        if !same_file(&addressed, &retained) {
            rollback_created_directories(root, &created);
            return Err(ResourceServiceError::unsafe_target());
        }
        directory = child;
    }
    Ok((directory, created))
}

fn rollback_created_directories(root: &Dir, created: &[WorkspaceRelativePath]) {
    for path in created.iter().rev() {
        let Ok((parent, name)) = parent_and_name(path) else {
            continue;
        };
        let Ok(directory) = open_directory(root, &parent) else {
            continue;
        };
        if directory.remove_dir(&name).is_ok() {
            let _sync_result = crate::storage::sync_directory(&directory);
        }
    }
}

fn random_resource_stage_name() -> Result<String, ResourceServiceError> {
    let mut entropy = [0_u8; 16];
    getrandom::fill(&mut entropy).map_err(|_| ResourceServiceError::unavailable())?;
    Ok(format!(
        ".qingyu-resource-stage-{}.tmp",
        entropy
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    ))
}

fn stage_resource(directory: &Dir, name: &str, body: &[u8]) -> Result<File, ResourceServiceError> {
    stage_resource_file(directory, name, body).map_err(|error| match error {
        BatchStageError::Operation(error) => error,
        BatchStageError::CleanupUncertain => ResourceServiceError::unavailable(),
    })
}

fn stage_batch_resource(directory: &Dir, name: &str, body: &[u8]) -> Result<File, BatchStageError> {
    stage_resource_file(directory, name, body)
}

fn stage_resource_file(directory: &Dir, name: &str, body: &[u8]) -> Result<File, BatchStageError> {
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
    let mut file = directory.open_with(name, &options).map_err(|error| {
        if error.kind() == io::ErrorKind::AlreadyExists {
            BatchStageError::CleanupUncertain
        } else {
            BatchStageError::Operation(ResourceServiceError::unavailable())
        }
    })?;
    if file.write_all(body).and_then(|()| file.sync_all()).is_err() {
        cleanup_staged_file_strict(directory, name, &file)?;
        return Err(BatchStageError::Operation(
            ResourceServiceError::unavailable(),
        ));
    }
    let metadata = match file.metadata() {
        Ok(metadata) => metadata,
        Err(_) => {
            cleanup_staged_file_strict(directory, name, &file)?;
            return Err(BatchStageError::Operation(
                ResourceServiceError::unavailable(),
            ));
        }
    };
    if !trusted_regular_file(&metadata) || metadata.len() != body.len() as u64 {
        cleanup_staged_file_strict(directory, name, &file)?;
        return Err(BatchStageError::Operation(
            ResourceServiceError::unsafe_target(),
        ));
    }
    Ok(file)
}

fn cleanup_staged_file_strict(
    directory: &Dir,
    name: &str,
    retained: &File,
) -> Result<(), BatchStageError> {
    let addressed = directory
        .symlink_metadata(name)
        .map_err(|_| BatchStageError::CleanupUncertain)?;
    let retained = retained
        .metadata()
        .map_err(|_| BatchStageError::CleanupUncertain)?;
    if !trusted_regular_file(&addressed)
        || !trusted_regular_file(&retained)
        || !same_file(&addressed, &retained)
    {
        return Err(BatchStageError::CleanupUncertain);
    }
    directory
        .remove_file(name)
        .map_err(|_| BatchStageError::CleanupUncertain)?;
    crate::storage::sync_directory(directory).map_err(|_| BatchStageError::CleanupUncertain)
}

fn cleanup_staged_resource(directory: &Dir, name: &str) {
    let _remove_result = directory.remove_file(name);
    let _sync_result = crate::storage::sync_directory(directory);
}

fn cleanup_installed_resource(directory: &Dir, name: &str, expected: &Metadata) {
    let Ok(addressed) = directory.symlink_metadata(name) else {
        return;
    };
    if !same_file(&addressed, expected) {
        return;
    }
    let _remove_result = directory.remove_file(name);
    let _sync_result = crate::storage::sync_directory(directory);
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
        | super::ResourceServiceErrorKind::InvalidMediaType
        | super::ResourceServiceErrorKind::InvalidPath => ErrorCode::InvalidRequest,
        super::ResourceServiceErrorKind::Conflict => ErrorCode::RevisionConflict,
        super::ResourceServiceErrorKind::StaleWorkspace => ErrorCode::RevisionConflict,
        super::ResourceServiceErrorKind::TooLarge => ErrorCode::ResourceTooLarge,
        super::ResourceServiceErrorKind::NotFound | super::ResourceServiceErrorKind::WrongKind => {
            ErrorCode::ResourceNotFound
        }
        super::ResourceServiceErrorKind::UnsafeTarget
        | super::ResourceServiceErrorKind::Unavailable => ErrorCode::WorkspaceUnavailable,
    };
    ServiceFailure::new(code, None).expect("resource errors use compatible details")
}

fn create_service_failure(error: ResourceServiceError) -> ServiceFailure {
    let code = match error.kind() {
        super::ResourceServiceErrorKind::NotFound | super::ResourceServiceErrorKind::WrongKind => {
            ErrorCode::DocumentNotFound
        }
        super::ResourceServiceErrorKind::InvalidCursor
        | super::ResourceServiceErrorKind::InvalidMediaType
        | super::ResourceServiceErrorKind::InvalidPath => ErrorCode::InvalidRequest,
        super::ResourceServiceErrorKind::Conflict => ErrorCode::RevisionConflict,
        super::ResourceServiceErrorKind::StaleWorkspace => ErrorCode::RevisionConflict,
        super::ResourceServiceErrorKind::TooLarge => ErrorCode::ResourceTooLarge,
        super::ResourceServiceErrorKind::UnsafeTarget
        | super::ResourceServiceErrorKind::Unavailable => ErrorCode::WorkspaceUnavailable,
    };
    ServiceFailure::new(code, None).expect("resource create errors use compatible details")
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

    async fn create_workspace_resource(
        &self,
        document_id: crate::contract::DocumentId,
        query: CreateWorkspaceResourceQuery,
        media_type: String,
        body: Vec<u8>,
    ) -> Result<ResourceEntryDto, ServiceFailure> {
        self.create_resource(&document_id, query, &media_type, &body)
            .await
            .map_err(create_service_failure)
    }

    async fn create_workspace_resource_batch(
        &self,
        document_id: crate::contract::DocumentId,
        request: CreateWorkspaceResourceBatchRequest,
    ) -> Result<CreateWorkspaceResourceBatchResponse, ServiceFailure> {
        let mut items = Vec::with_capacity(request.items.len());
        for item in request.items {
            let body = STANDARD
                .decode(item.body_base64)
                .map_err(|_| create_service_failure(ResourceServiceError::invalid_media_type()))?;
            items.push(CreateResourceBatchItem {
                name: item.name,
                kind: item.kind,
                media_type: item.media_type,
                body,
            });
        }
        let resources = self
            .create_resource_batch(
                request.batch_id,
                &document_id,
                request.workspace_generation,
                request.folder,
                items,
            )
            .await
            .map_err(create_service_failure)?;
        Ok(CreateWorkspaceResourceBatchResponse {
            batch_id: request.batch_id,
            resources,
        })
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
    parent: &WorkspaceRelativePath,
    names: &[String],
    output_kind: InventoryOutputKind,
    budget: &mut InventorySnapshotBudget,
) -> Result<(), ResourceServiceError> {
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
        let allocations = [
            context.runtime.wire_identity_key().document_id_allocation(
                context.workspace().id,
                &context.workspace().generation,
                DocumentKind::File,
                &path,
            ),
            context.runtime.wire_identity_key().document_id_allocation(
                context.workspace().id,
                &context.workspace().generation,
                DocumentKind::Directory,
                &path,
            ),
            context.runtime.wire_identity_key().resource_id_allocation(
                context.workspace().id,
                &context.workspace().generation,
                ResourceKind::Attachment,
                &path,
            ),
            context.runtime.wire_identity_key().resource_id_allocation(
                context.workspace().id,
                &context.workspace().generation,
                ResourceKind::Image,
                &path,
            ),
        ];
        let mut maximum_token_bytes = 0_usize;
        let mut maximum_identity_transient_bytes = 0_usize;
        for allocation in allocations {
            let allocation = allocation.map_err(|_| ResourceServiceError::unavailable())?;
            maximum_token_bytes = maximum_token_bytes.max(allocation.token_bytes());
            maximum_identity_transient_bytes =
                maximum_identity_transient_bytes.max(allocation.transient_bytes());
        }
        let retained = path_bytes
            .checked_add(parent.as_str().len())
            .and_then(|bytes| bytes.checked_add(name.len()))
            .and_then(|bytes| bytes.checked_add(MAX_INVENTORY_REVISION_BYTES))
            .and_then(|bytes| bytes.checked_add(MAX_INVENTORY_TIMESTAMP_BYTES))
            .and_then(|bytes| bytes.checked_add(maximum_token_bytes))
            .and_then(|bytes| bytes.checked_add(MAX_INVENTORY_MEDIA_TYPE_BYTES))
            .ok_or_else(ResourceServiceError::unavailable)?;
        retained_total = retained_total
            .checked_add(retained)
            .ok_or_else(ResourceServiceError::unavailable)?;
        maximum_retained = maximum_retained.max(retained);
        maximum_transient = maximum_transient
            .max(maximum_identity_transient_bytes)
            .max(name.len());
    }
    let (vector_elements, retained_bytes) = match output_kind {
        InventoryOutputKind::Direct => (names.len(), retained_total),
        InventoryOutputKind::Page { maximum } => {
            let elements = names.len().min(maximum);
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
        let document_revision = document_revision_from_digest(inspected.content_digest)?;
        let entry = document_entry(
            context,
            parent,
            path,
            name,
            DocumentKind::File,
            &inspected.metadata,
            document_revision,
        )?;
        Ok(Some(WorkspaceInventoryEntry::Document(entry)))
    } else {
        let classification = classify_resource(
            name,
            &inspected.header,
            inspected.metadata.len(),
            inspected.validation_body.as_deref(),
        );
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
    header: Vec<u8>,
    validation_body: Option<Vec<u8>>,
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
    let mut header = Vec::with_capacity(
        usize::try_from(retained.len())
            .unwrap_or(MAX_IMAGE_HEADER_BYTES)
            .min(MAX_IMAGE_HEADER_BYTES),
    );
    let validation_limit = full_validation_limit(name);
    let mut validation_body = validation_limit
        .map(|limit| Vec::with_capacity(usize::try_from(retained.len()).unwrap_or(0).min(limit)));
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
        let header_copy = (MAX_IMAGE_HEADER_BYTES - header.len()).min(read);
        header.extend_from_slice(&buffer[..header_copy]);
        digest.update(&buffer[..read]);
        if validation_body
            .as_ref()
            .is_some_and(|body| body.len().saturating_add(read) > validation_limit.unwrap_or(0))
        {
            validation_body = None;
        } else if let Some(body) = validation_body.as_mut() {
            body.extend_from_slice(&buffer[..read]);
        }
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
    let revision = Revision::parse(format!("sha256:{}", encoded_content_digest(content_digest)))
        .map_err(|_| ResourceServiceError::unavailable())?;
    Ok(InspectedFile {
        content_digest,
        file,
        metadata: after,
        revision,
        header,
        validation_body: validation_body.filter(|bytes| !bytes.is_empty() || retained.len() == 0),
    })
}

fn document_revision_from_digest(
    content_digest: ContentDigest,
) -> Result<Revision, ResourceServiceError> {
    Revision::parse(encoded_content_digest(content_digest))
        .map_err(|_| ResourceServiceError::unavailable())
}

fn encoded_content_digest(content_digest: ContentDigest) -> String {
    use fmt::Write as _;

    let mut encoded = String::with_capacity(64);
    for byte in content_digest.into_bytes() {
        write!(&mut encoded, "{byte:02x}").expect("writing to a String cannot fail");
    }
    encoded
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

/// Re-opens a transaction-recorded parent while distinguishing a genuinely
/// absent component from every other lookup failure. Recovery may treat the
/// former as an unpublished `Preparing` transaction, but must never turn a
/// permission error, symlink substitution, or I/O fault into a safe absence.
fn open_record_directory(
    root: &Dir,
    path: &WorkspaceRelativePath,
) -> Result<Option<Dir>, ResourceServiceError> {
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
        let addressed = match directory.symlink_metadata(component) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(ResourceServiceError::unavailable()),
        };
        if !addressed.is_dir() || addressed.file_type().is_symlink() {
            return Err(ResourceServiceError::unsafe_target());
        }
        let child = directory
            .open_dir_nofollow(component)
            .map_err(|_| ResourceServiceError::unsafe_target())?;
        let retained = trusted_directory_metadata(&child)?;
        if !same_file(&addressed, &retained) {
            return Err(ResourceServiceError::unsafe_target());
        }
        directory = child;
    }
    Ok(Some(directory))
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

fn full_validation_limit(name: &str) -> Option<usize> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".svg") {
        Some(MAX_SVG_BYTES)
    } else if [".avif", ".bmp", ".gif", ".jpeg", ".jpg", ".png", ".webp"]
        .iter()
        .any(|extension| lower.ends_with(extension))
    {
        Some(MAX_RESOURCE_BODY_BYTES)
    } else {
        None
    }
}

#[derive(Clone, Copy)]
struct ResourceClassification {
    kind: ResourceKind,
    media_type: &'static str,
    previewable: bool,
}

fn classify_resource(
    name: &str,
    _header: &[u8],
    total_len: u64,
    full_body: Option<&[u8]>,
) -> ResourceClassification {
    let lower = name.to_ascii_lowercase();
    let media_type = if lower.ends_with(".png") && full_body.is_some_and(valid_png) {
        Some("image/png")
    } else if (lower.ends_with(".jpg") || lower.ends_with(".jpeg"))
        && full_body.is_some_and(valid_jpeg)
    {
        Some("image/jpeg")
    } else if lower.ends_with(".gif") && full_body.is_some_and(valid_gif) {
        Some("image/gif")
    } else if lower.ends_with(".webp") && full_body.is_some_and(valid_webp) {
        Some("image/webp")
    } else if lower.ends_with(".bmp") && full_body.is_some_and(|body| valid_bmp(body, total_len)) {
        Some("image/bmp")
    } else if lower.ends_with(".avif") && full_body.is_some_and(valid_avif) {
        Some("image/avif")
    } else if lower.ends_with(".svg")
        && full_body.is_some_and(|body| normalize_static_svg(body).is_ok())
    {
        Some("image/svg+xml")
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

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0_u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

fn valid_png(bytes: &[u8]) -> bool {
    if bytes.get(..8) != Some(b"\x89PNG\r\n\x1a\n") {
        return false;
    }
    let mut offset = 8_usize;
    let mut first = true;
    let mut saw_data = false;
    while offset.checked_add(12).is_some_and(|end| end <= bytes.len()) {
        let Some(length) = read_be_u32(bytes, offset).map(|value| value as usize) else {
            return false;
        };
        let Some(end) = offset
            .checked_add(12)
            .and_then(|base| base.checked_add(length))
        else {
            return false;
        };
        if end > bytes.len() {
            return false;
        }
        let kind = &bytes[offset + 4..offset + 8];
        let data_end = offset + 8 + length;
        let Some(expected_crc) = read_be_u32(bytes, data_end) else {
            return false;
        };
        if crc32(&bytes[offset + 4..data_end]) != expected_crc {
            return false;
        }
        if first {
            if kind != b"IHDR" || length != 13 {
                return false;
            }
            let width = read_be_u32(bytes, offset + 8).unwrap_or(0);
            let height = read_be_u32(bytes, offset + 12).unwrap_or(0);
            if !valid_image_dimensions(width, height) {
                return false;
            }
            first = false;
        } else if kind == b"IDAT" {
            saw_data = true;
        } else if kind == b"IEND" {
            return length == 0 && saw_data && end == bytes.len();
        }
        offset = end;
    }
    false
}

fn valid_jpeg(bytes: &[u8]) -> bool {
    if bytes.get(..2) != Some(b"\xff\xd8") || bytes.len() < 12 {
        return false;
    }
    let mut offset = 2_usize;
    let mut saw_frame = false;
    let mut saw_scan = false;
    while offset < bytes.len() {
        if bytes.get(offset) != Some(&0xff) {
            return false;
        }
        while bytes.get(offset) == Some(&0xff) {
            offset += 1;
        }
        let Some(marker) = bytes.get(offset).copied() else {
            return false;
        };
        offset += 1;
        if marker == 0xd9 {
            return saw_frame && saw_scan && offset == bytes.len();
        }
        if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        let Some(length) = bytes
            .get(offset..offset + 2)
            .map(|value| u16::from_be_bytes([value[0], value[1]]) as usize)
        else {
            return false;
        };
        if length < 2
            || offset
                .checked_add(length)
                .is_none_or(|end| end > bytes.len())
        {
            return false;
        }
        if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
            let height = bytes
                .get(offset + 3..offset + 5)
                .map(|value| u16::from_be_bytes([value[0], value[1]]))
                .unwrap_or(0);
            let width = bytes
                .get(offset + 5..offset + 7)
                .map(|value| u16::from_be_bytes([value[0], value[1]]))
                .unwrap_or(0);
            if length < 8 || !valid_image_dimensions(u32::from(width), u32::from(height)) {
                return false;
            }
            saw_frame = true;
        }
        offset += length;
        if marker == 0xda {
            saw_scan = true;
            loop {
                let Some(byte) = bytes.get(offset).copied() else {
                    return false;
                };
                offset += 1;
                if byte != 0xff {
                    continue;
                }
                let Some(next) = bytes.get(offset).copied() else {
                    return false;
                };
                if next == 0x00 || (0xd0..=0xd7).contains(&next) {
                    offset += 1;
                    continue;
                }
                offset -= 1;
                break;
            }
        }
    }
    false
}

fn skip_gif_sub_blocks(bytes: &[u8], mut offset: usize) -> Option<usize> {
    loop {
        let length = usize::from(*bytes.get(offset)?);
        offset += 1;
        if length == 0 {
            return Some(offset);
        }
        offset = offset.checked_add(length)?;
        if offset > bytes.len() {
            return None;
        }
    }
}

fn valid_gif(bytes: &[u8]) -> bool {
    if bytes.len() < 14
        || !(bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"))
        || !valid_image_dimensions(
            u32::from(u16::from_le_bytes([bytes[6], bytes[7]])),
            u32::from(u16::from_le_bytes([bytes[8], bytes[9]])),
        )
    {
        return false;
    }
    let packed = bytes[10];
    let mut offset = 13_usize;
    if packed & 0x80 != 0 {
        offset = match offset.checked_add(3_usize << (usize::from(packed & 0x07) + 1)) {
            Some(value) if value <= bytes.len() => value,
            _ => return false,
        };
    }
    let mut saw_image = false;
    while let Some(kind) = bytes.get(offset).copied() {
        offset += 1;
        match kind {
            0x3b => return saw_image && offset == bytes.len(),
            0x21 => {
                if bytes.get(offset).is_none() {
                    return false;
                }
                offset += 1;
                let Some(next) = skip_gif_sub_blocks(bytes, offset) else {
                    return false;
                };
                offset = next;
            }
            0x2c => {
                let Some(descriptor) = bytes.get(offset..offset + 9) else {
                    return false;
                };
                if !valid_image_dimensions(
                    u32::from(u16::from_le_bytes([descriptor[4], descriptor[5]])),
                    u32::from(u16::from_le_bytes([descriptor[6], descriptor[7]])),
                ) {
                    return false;
                }
                offset += 9;
                if descriptor[8] & 0x80 != 0 {
                    offset = match offset
                        .checked_add(3_usize << (usize::from(descriptor[8] & 0x07) + 1))
                    {
                        Some(value) if value <= bytes.len() => value,
                        _ => return false,
                    };
                }
                if bytes.get(offset).is_none() {
                    return false;
                }
                offset += 1;
                let Some(next) = skip_gif_sub_blocks(bytes, offset) else {
                    return false;
                };
                offset = next;
                saw_image = true;
            }
            _ => return false,
        }
    }
    false
}

fn valid_webp(bytes: &[u8]) -> bool {
    if bytes.len() < 20
        || bytes.get(..4) != Some(b"RIFF")
        || bytes.get(8..12) != Some(b"WEBP")
        || read_le_u32(bytes, 4).map(|size| u64::from(size) + 8) != Some(bytes.len() as u64)
    {
        return false;
    }
    let mut offset = 12_usize;
    let mut saw_image = false;
    while offset < bytes.len() {
        let Some(kind) = bytes.get(offset..offset + 4) else {
            return false;
        };
        let Some(length) = read_le_u32(bytes, offset + 4).map(|value| value as usize) else {
            return false;
        };
        let Some(end) = offset
            .checked_add(8)
            .and_then(|value| value.checked_add(length))
        else {
            return false;
        };
        if end > bytes.len() {
            return false;
        }
        let payload = &bytes[offset + 8..end];
        saw_image |= match kind {
            b"VP8 " if payload.len() >= 10 && payload.get(3..6) == Some(b"\x9d\x01\x2a") => {
                let width = u32::from(u16::from_le_bytes([payload[6], payload[7]]) & 0x3fff);
                let height = u32::from(u16::from_le_bytes([payload[8], payload[9]]) & 0x3fff);
                valid_image_dimensions(width, height)
            }
            b"VP8L" if payload.len() >= 5 && payload[0] == 0x2f => {
                let packed = u32::from_le_bytes([payload[1], payload[2], payload[3], payload[4]]);
                let width = (packed & 0x3fff) + 1;
                let height = ((packed >> 14) & 0x3fff) + 1;
                valid_image_dimensions(width, height)
            }
            _ => false,
        };
        offset = end + (length & 1);
    }
    saw_image && offset == bytes.len()
}

fn valid_image_dimensions(width: u32, height: u32) -> bool {
    (1..=16_384).contains(&width)
        && (1..=16_384).contains(&height)
        && width
            .checked_mul(height)
            .is_some_and(|pixels| pixels <= 67_108_864)
}

fn read_be_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn read_le_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn valid_avif(bytes: &[u8]) -> bool {
    let mut offset = 0_usize;
    let mut static_brand = false;
    let mut metadata = false;
    let mut media_data = false;
    while offset < bytes.len() {
        let Some((kind, payload, next)) = iso_box(bytes, offset, bytes.len()) else {
            return false;
        };
        match kind {
            b"ftyp" if offset == 0 && payload.len() >= 8 => {
                let brands = payload.chunks_exact(4).collect::<Vec<_>>();
                static_brand = brands.iter().any(|brand| *brand == b"avif")
                    && !brands.iter().any(|brand| *brand == b"avis");
            }
            b"meta" => metadata = valid_avif_meta(payload),
            b"mdat" => media_data = !payload.is_empty(),
            _ => {}
        }
        offset = next;
    }
    offset == bytes.len() && static_brand && metadata && media_data
}

fn iso_box(bytes: &[u8], offset: usize, end: usize) -> Option<(&[u8], &[u8], usize)> {
    let size = read_be_u32(bytes, offset)? as usize;
    if size < 8 || size == 1 {
        return None;
    }
    let next = offset.checked_add(size)?;
    if next > end {
        return None;
    }
    Some((
        bytes.get(offset + 4..offset + 8)?,
        bytes.get(offset + 8..next)?,
        next,
    ))
}

fn valid_avif_meta(payload: &[u8]) -> bool {
    if payload.len() < 4 {
        return false;
    }
    let mut offset = 4_usize;
    let mut primary = false;
    let mut locations = false;
    let mut information = false;
    let mut dimensions = false;
    while offset < payload.len() {
        let Some((kind, child, next)) = iso_box(payload, offset, payload.len()) else {
            return false;
        };
        match kind {
            b"pitm" => primary = child.len() >= 6,
            b"iloc" => locations = child.len() >= 8,
            b"iinf" => information = child.len() >= 6,
            b"iprp" => dimensions = avif_property_dimensions(child),
            _ => {}
        }
        offset = next;
    }
    offset == payload.len() && primary && locations && information && dimensions
}

fn avif_property_dimensions(payload: &[u8]) -> bool {
    let mut offset = 0_usize;
    while offset < payload.len() {
        let Some((kind, child, next)) = iso_box(payload, offset, payload.len()) else {
            return false;
        };
        if kind == b"ipco" {
            let mut property_offset = 0_usize;
            while property_offset < child.len() {
                let Some((property, value, property_next)) =
                    iso_box(child, property_offset, child.len())
                else {
                    return false;
                };
                if property == b"ispe" && value.len() == 12 {
                    let width = read_be_u32(value, 4).unwrap_or(0);
                    let height = read_be_u32(value, 8).unwrap_or(0);
                    if valid_image_dimensions(width, height) {
                        return true;
                    }
                }
                property_offset = property_next;
            }
        }
        offset = next;
    }
    false
}

fn valid_bmp(bytes: &[u8], total_len: u64) -> bool {
    if bytes.len() < 54 || bytes.get(..2) != Some(b"BM") {
        return false;
    }
    let Some(declared_size) = read_le_u32(bytes, 2).map(|value| value as usize) else {
        return false;
    };
    let Some(pixel_offset) = read_le_u32(bytes, 10).map(|value| value as usize) else {
        return false;
    };
    let Some(dib_size) = read_le_u32(bytes, 14).map(|value| value as usize) else {
        return false;
    };
    let width = read_le_u32(bytes, 18)
        .map(|value| value as i32)
        .unwrap_or(0);
    let signed_height = read_le_u32(bytes, 22)
        .map(|value| value as i32)
        .unwrap_or(0);
    let Some(height) = signed_height.checked_abs().map(|value| value as u32) else {
        return false;
    };
    let planes = bytes
        .get(26..28)
        .map(|value| u16::from_le_bytes([value[0], value[1]]))
        .unwrap_or(0);
    let bits_per_pixel = bytes
        .get(28..30)
        .map(|value| u16::from_le_bytes([value[0], value[1]]))
        .unwrap_or(0);
    let compression = read_le_u32(bytes, 30).unwrap_or(u32::MAX);
    let required_pixel_bytes = u32::try_from(width)
        .ok()
        .and_then(|width| width.checked_mul(u32::from(bits_per_pixel)))
        .and_then(|row_bits| row_bits.checked_add(31))
        .map(|row_bits| (row_bits / 32) * 4)
        .and_then(|stride| stride.checked_mul(height))
        .and_then(|bytes| pixel_offset.checked_add(bytes as usize));
    declared_size as u64 == total_len
        && pixel_offset >= 14 + dib_size
        && pixel_offset < bytes.len()
        && matches!(dib_size, 40 | 52 | 56 | 64 | 108 | 124)
        && u32::try_from(width)
            .ok()
            .is_some_and(|width| valid_image_dimensions(width, height))
        && planes == 1
        && matches!(bits_per_pixel, 1 | 4 | 8 | 16 | 24 | 32)
        && compression == 0
        && required_pixel_bytes.is_some_and(|end| end <= bytes.len())
}

fn safe_svg_fragment(value: &str) -> bool {
    let Some(id) = value.strip_prefix('#') else {
        return false;
    };
    !id.is_empty()
        && id.len() <= 256
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

fn safe_svg_paint(value: &str) -> bool {
    let value = value.trim();
    if let Some(fragment) = value
        .strip_prefix("url(")
        .and_then(|rest| rest.strip_suffix(')'))
    {
        return safe_svg_fragment(fragment.trim());
    }
    !value.is_empty()
        && value.len() <= 128
        && !value.contains('\\')
        && !value.chars().any(char::is_control)
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'#' | b'(' | b')' | b',' | b'.' | b'%' | b' ' | b'-' | b'+'
                )
        })
}

fn safe_svg_numbers(value: &str, maximum: f64, expected: Option<usize>) -> bool {
    let normalized = value.replace(',', " ");
    let values = normalized
        .split_ascii_whitespace()
        .map(str::parse::<f64>)
        .collect::<Result<Vec<_>, _>>();
    values.is_ok_and(|values| {
        expected.is_none_or(|expected| values.len() == expected)
            && !values.is_empty()
            && values
                .iter()
                .all(|value| value.is_finite() && value.abs() <= maximum)
    })
}

fn safe_svg_path(value: &str) -> bool {
    if value.len() > 64 * 1024
        || value
            .bytes()
            .filter(|byte| byte.is_ascii_alphabetic())
            .count()
            > 4096
        || value
            .bytes()
            .any(|byte| byte.is_ascii_alphabetic() && !b"MmZzLlHhVvCcSsQqTtAa".contains(&byte))
    {
        return false;
    }
    let numeric = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphabetic() || character == ',' {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    safe_svg_numbers(&numeric, 1_000_000.0, None)
}

fn normalize_static_svg(body: &[u8]) -> Result<Vec<u8>, ResourceServiceError> {
    const ELEMENTS: &[&str] = &[
        "svg",
        "g",
        "defs",
        "path",
        "rect",
        "circle",
        "ellipse",
        "line",
        "polyline",
        "polygon",
        "linearGradient",
        "radialGradient",
        "stop",
        "title",
        "desc",
        "text",
        "tspan",
    ];
    const ATTRIBUTES: &[&str] = &[
        "xmlns",
        "xmlns:xlink",
        "viewBox",
        "width",
        "height",
        "x",
        "y",
        "x1",
        "y1",
        "x2",
        "y2",
        "cx",
        "cy",
        "r",
        "rx",
        "ry",
        "d",
        "points",
        "fill",
        "fill-opacity",
        "fill-rule",
        "stroke",
        "stroke-width",
        "stroke-linecap",
        "stroke-linejoin",
        "stroke-miterlimit",
        "stroke-opacity",
        "opacity",
        "id",
        "preserveAspectRatio",
        "version",
        "role",
        "aria-label",
        "aria-hidden",
        "dx",
        "dy",
        "font-family",
        "font-size",
        "font-weight",
        "text-anchor",
        "dominant-baseline",
        "offset",
        "stop-color",
        "stop-opacity",
        "gradientUnits",
        "spreadMethod",
    ];
    if body.len() > MAX_SVG_BYTES || std::str::from_utf8(body).is_err() {
        return Err(ResourceServiceError::invalid_media_type());
    }
    let mut reader = Reader::from_reader(body.strip_prefix(b"\xef\xbb\xbf").unwrap_or(body));
    reader.config_mut().trim_text(false);
    let mut writer = Writer::new(Vec::with_capacity(body.len()));
    let mut stack: Vec<String> = Vec::new();
    let mut ids = std::collections::HashSet::new();
    let mut references = Vec::new();
    let mut root_seen = false;
    let mut nodes = 0_usize;
    let mut text_bytes = 0_usize;

    loop {
        let event = reader
            .read_event()
            .map_err(|_| ResourceServiceError::invalid_media_type())?;
        let empty_element = matches!(&event, Event::Empty(_));
        match event {
            Event::Decl(_) | Event::Comment(_) => {}
            Event::DocType(_) | Event::PI(_) | Event::CData(_) | Event::GeneralRef(_) => {
                return Err(ResourceServiceError::invalid_media_type());
            }
            Event::Start(start) | Event::Empty(start) => {
                let empty = empty_element;
                nodes = nodes.saturating_add(1);
                if nodes > 10_000 || stack.len() >= 64 {
                    return Err(ResourceServiceError::invalid_media_type());
                }
                let name = std::str::from_utf8(start.name().as_ref())
                    .map_err(|_| ResourceServiceError::invalid_media_type())?
                    .to_string();
                if name.contains(':') || !ELEMENTS.contains(&name.as_str()) {
                    return Err(ResourceServiceError::invalid_media_type());
                }
                if stack.is_empty() {
                    if root_seen || name != "svg" {
                        return Err(ResourceServiceError::invalid_media_type());
                    }
                    root_seen = true;
                }
                let mut attributes = std::collections::BTreeMap::new();
                for attribute in start.attributes().with_checks(true) {
                    let attribute =
                        attribute.map_err(|_| ResourceServiceError::invalid_media_type())?;
                    let key = std::str::from_utf8(attribute.key.as_ref())
                        .map_err(|_| ResourceServiceError::invalid_media_type())?
                        .to_string();
                    if attributes.len() >= 64
                        || key.to_ascii_lowercase().starts_with("on")
                        || !ATTRIBUTES.contains(&key.as_str())
                    {
                        return Err(ResourceServiceError::invalid_media_type());
                    }
                    let value = attribute
                        .decode_and_unescape_value(reader.decoder())
                        .map_err(|_| ResourceServiceError::invalid_media_type())?
                        .into_owned();
                    if value.len()
                        > if key == "d" || key == "points" {
                            1024 * 1024
                        } else {
                            4096
                        }
                        || value.chars().any(char::is_control)
                    {
                        return Err(ResourceServiceError::invalid_media_type());
                    }
                    match key.as_str() {
                        "xmlns" if name == "svg" && value == "http://www.w3.org/2000/svg" => {}
                        "xmlns:xlink"
                            if name == "svg" && value == "http://www.w3.org/1999/xlink" => {}
                        "fill" | "stroke" | "stop-color" if safe_svg_paint(&value) => {
                            if let Some(fragment) = value
                                .trim()
                                .strip_prefix("url(")
                                .and_then(|rest| rest.strip_suffix(')'))
                            {
                                references.push(fragment.trim()[1..].to_string());
                            }
                        }
                        "viewBox" if safe_svg_numbers(&value, 1_000_000.0, Some(4)) => {}
                        "width" | "height" if safe_svg_numbers(&value, 16_384.0, Some(1)) => {}
                        "x" | "y" | "x1" | "y1" | "x2" | "y2" | "cx" | "cy" | "r" | "rx" | "ry"
                        | "dx" | "dy" | "stroke-width" | "stroke-miterlimit" | "font-size"
                        | "offset"
                            if safe_svg_numbers(&value, 1_000_000.0, None) => {}
                        "d" if safe_svg_path(&value) => {}
                        "points"
                            if value.len() <= 64 * 1024
                                && safe_svg_numbers(&value, 1_000_000.0, None) => {}
                        "viewBox" | "width" | "height" | "x" | "y" | "x1" | "y1" | "x2" | "y2"
                        | "cx" | "cy" | "r" | "rx" | "ry" | "dx" | "dy" | "stroke-width"
                        | "stroke-miterlimit" | "font-size" | "offset" | "d" | "points" => {
                            return Err(ResourceServiceError::invalid_media_type())
                        }
                        "xmlns" | "xmlns:xlink" => {
                            return Err(ResourceServiceError::invalid_media_type());
                        }
                        _ if value.to_ascii_lowercase().contains("url(")
                            || value.to_ascii_lowercase().contains("javascript:")
                            || value.to_ascii_lowercase().contains("data:") =>
                        {
                            return Err(ResourceServiceError::invalid_media_type());
                        }
                        _ => {}
                    }
                    if key == "id"
                        && (!safe_svg_fragment(&format!("#{value}")) || !ids.insert(value.clone()))
                    {
                        return Err(ResourceServiceError::invalid_media_type());
                    }
                    attributes.insert(key, value);
                }
                if name == "svg"
                    && attributes.get("xmlns").map(String::as_str)
                        != Some("http://www.w3.org/2000/svg")
                {
                    return Err(ResourceServiceError::invalid_media_type());
                }
                let mut output = BytesStart::new(name.as_str());
                for (key, value) in &attributes {
                    output.push_attribute((key.as_str(), value.as_str()));
                }
                writer
                    .write_event(if empty {
                        Event::Empty(output)
                    } else {
                        Event::Start(output)
                    })
                    .map_err(|_| ResourceServiceError::invalid_media_type())?;
                if !empty {
                    stack.push(name);
                }
            }
            Event::End(end) => {
                let name = std::str::from_utf8(end.name().as_ref())
                    .map_err(|_| ResourceServiceError::invalid_media_type())?
                    .to_string();
                if stack.pop().as_deref() != Some(name.as_str()) {
                    return Err(ResourceServiceError::invalid_media_type());
                }
                writer
                    .write_event(Event::End(BytesEnd::new(name.as_str())))
                    .map_err(|_| ResourceServiceError::invalid_media_type())?;
            }
            Event::Text(text) => {
                let value = text
                    .decode()
                    .map_err(|_| ResourceServiceError::invalid_media_type())?;
                text_bytes = text_bytes.saturating_add(value.len());
                if text_bytes > 1024 * 1024
                    || (!value.trim().is_empty()
                        && !stack.last().is_some_and(|name| {
                            matches!(name.as_str(), "title" | "desc" | "text" | "tspan")
                        }))
                {
                    return Err(ResourceServiceError::invalid_media_type());
                }
                writer
                    .write_event(Event::Text(BytesText::new(&value)))
                    .map_err(|_| ResourceServiceError::invalid_media_type())?;
            }
            Event::Eof => break,
        }
    }
    if !root_seen
        || !stack.is_empty()
        || references.iter().any(|reference| !ids.contains(reference))
    {
        return Err(ResourceServiceError::invalid_media_type());
    }
    Ok(writer.into_inner())
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
    async fn every_name_reserves_all_possible_inventory_output_kinds() {
        use crate::inventory_snapshot::{InventorySnapshotBudget, InventorySnapshotLimits};

        let fixture = InventoryFixture::new().await;
        let context = fixture.service.context().unwrap();
        let mut budget = InventorySnapshotBudget::new(InventorySnapshotLimits {
            maximum_nodes: 10,
            maximum_content_bytes: 1024,
            maximum_metadata_bytes: 10_000,
            maximum_depth: 10,
        });
        let before = budget.remaining_metadata_bytes();

        super::reserve_inventory_output(
            &context,
            &WorkspaceRelativePath::default(),
            &["payload.bin".to_string()],
            super::InventoryOutputKind::Direct,
            &mut budget,
        )
        .unwrap();

        let vector_and_transient =
            u64::try_from(std::mem::size_of::<super::WorkspaceInventoryEntry>())
                .unwrap()
                .checked_add(u64::try_from(super::MAX_INVENTORY_TIMESTAMP_BYTES).unwrap())
                .unwrap();
        assert!(before - budget.remaining_metadata_bytes() > vector_and_transient);
    }

    #[test]
    fn paged_candidate_limit_fits_all_reserved_directory_scans() {
        let required = u64::try_from(super::MAX_IMMEDIATE_INVENTORY_CANDIDATES)
            .unwrap()
            .checked_mul(super::MAX_INVENTORY_PAGE_DIRECTORY_SCANS)
            .unwrap();

        assert!(super::MAX_INVENTORY_SNAPSHOT_NODES >= required);
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
