use std::{
    collections::HashSet,
    sync::atomic::{AtomicUsize, Ordering},
};

use cap_std::fs::Dir;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::{
    contract::{
        ResourceBatchId, ResourceKind, ResourceName, WorkspaceGeneration, WorkspaceId,
        WorkspaceRelativePath,
    },
    paths::open_or_create_child,
    runtime::KernelRuntime,
    storage::{
        CommitState, DurableFileFailureKind, DurableFileStore, ExpectedFile, FileRevision,
        PreservePrevious, RecoveryOutcome, ReplaceRequest, StorageFileName,
    },
};

use super::{CreateResourceBatchItem, ResourceServiceError};

const STATE_ROOT: &str = "resource-batches-v1";
const PENDING_PREFIX: &str = "resource-batch-pending-v1-";
const RECEIPT_PREFIX: &str = "resource-batch-receipt-v1-";
const RECORD_SUFFIX: &str = ".json";
const MAX_RECORD_BYTES: u64 = 128 * 1024;
const MAX_STABLE_RECORDS: usize = 100_000;
const MAX_RAW_ENTRIES: usize = MAX_STABLE_RECORDS + 3;
const MAX_PENDING_RECORDS: usize = 1;
const DIGEST_DOMAIN: &[u8] = b"qingyu-resource-batch-request-v1";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum BatchPhase {
    Preparing,
    Prepared,
    Committed,
    Aborted,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct BatchRecordItem {
    pub(super) ordinal: u8,
    pub(super) name_attempt: u32,
    pub(super) requested_name: ResourceName,
    pub(super) target_name: ResourceName,
    pub(super) target_path: WorkspaceRelativePath,
    pub(super) kind: ResourceKind,
    pub(super) media_type: String,
    pub(super) size_bytes: u64,
    pub(super) content_sha256: String,
    pub(super) stage_name: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct BatchRecord {
    schema_version: u8,
    pub(super) batch_id: ResourceBatchId,
    pub(super) request_digest: String,
    pub(super) workspace_id: WorkspaceId,
    pub(super) workspace_generation: WorkspaceGeneration,
    pub(super) document_path: WorkspaceRelativePath,
    pub(super) folder: WorkspaceRelativePath,
    pub(super) target_parent: WorkspaceRelativePath,
    pub(super) phase: BatchPhase,
    pub(super) attempt_id: uuid::Uuid,
    pub(super) items: Vec<BatchRecordItem>,
}

impl BatchRecord {
    pub(super) fn preparing(
        batch_id: ResourceBatchId,
        request_digest: String,
        workspace_id: WorkspaceId,
        workspace_generation: WorkspaceGeneration,
        document_path: WorkspaceRelativePath,
        folder: WorkspaceRelativePath,
        target_parent: WorkspaceRelativePath,
        attempt_id: uuid::Uuid,
        items: Vec<BatchRecordItem>,
    ) -> Self {
        Self {
            schema_version: 1,
            batch_id,
            request_digest,
            workspace_id,
            workspace_generation,
            document_path,
            folder,
            target_parent,
            phase: BatchPhase::Preparing,
            attempt_id,
            items,
        }
    }

    pub(super) fn validate(
        &self,
        expected_workspace: WorkspaceId,
        expected_generation: &WorkspaceGeneration,
    ) -> Result<(), ResourceServiceError> {
        let expected_parent = expected_target_parent(&self.document_path, &self.folder)?;
        let mut total_size = 0_u64;
        let mut stages = HashSet::new();
        let mut targets = HashSet::new();
        if self.schema_version != 1
            || self.workspace_id != expected_workspace
            || &self.workspace_generation != expected_generation
            || self.target_parent != expected_parent
            || self
                .target_parent
                .as_str()
                .split('/')
                .any(super::policy::protected_resource_component)
            || self.items.is_empty()
            || self.items.len() > super::MAX_RESOURCE_BATCH_ITEMS
            || self.request_digest.len() != 71
            || !self.request_digest.starts_with("sha256:")
            || !lower_hex(&self.request_digest[7..])
            || self.items.iter().enumerate().any(|(index, item)| {
                total_size = total_size.saturating_add(item.size_bytes);
                let expected_name = usize::try_from(item.name_attempt)
                    .map_err(|_| ResourceServiceError::unavailable())
                    .and_then(|attempt| {
                        unique_resource_name(item.requested_name.as_str(), attempt)
                    });
                let expected_target = expected_item_path(&self.target_parent, &item.target_name);
                usize::from(item.ordinal) != index
                    || expected_name.as_ref().ok() != Some(&item.target_name)
                    || item.content_sha256.len() != 64
                    || !lower_hex(&item.content_sha256)
                    || item.stage_name != stage_name(self.attempt_id, index)
                    || item.stage_name.contains(['/', '\\'])
                    || expected_target.as_ref().ok() != Some(&item.target_path)
                    || markdown_name(item.requested_name.as_str())
                    || markdown_name(item.target_name.as_str())
                    || super::policy::protected_resource_component(item.target_name.as_str())
                    || !valid_kind_media(item.kind, item.target_name.as_str(), &item.media_type)
                    || !stages.insert(item.stage_name.clone())
                    || !targets.insert(item.target_path.as_str().to_lowercase())
            })
            || total_size > super::MAX_RESOURCE_BODY_BYTES as u64
            || request_digest_from_record(self).as_deref() != Ok(self.request_digest.as_str())
        {
            return Err(ResourceServiceError::unavailable());
        }
        Ok(())
    }
}

pub(super) struct StoredBatchRecord {
    pub(super) record: BatchRecord,
    pub(super) revision: FileRevision,
}

pub(super) enum CreateBatchRecordError {
    NotPublished(ResourceServiceError),
    RecoveryRequired(ResourceServiceError),
}

pub(super) struct ResourceBatchStore {
    directory: Dir,
    durable: DurableFileStore,
    workspace_id: WorkspaceId,
    workspace_generation: WorkspaceGeneration,
    stable_entries: AtomicUsize,
}

impl ResourceBatchStore {
    pub(super) fn open(runtime: &KernelRuntime) -> Result<Self, ResourceServiceError> {
        Self::open_below(runtime, STATE_ROOT)
    }

    pub(super) fn open_isolated(runtime: &KernelRuntime) -> Result<Self, ResourceServiceError> {
        let root = format!("resource-batches-test-{}", uuid::Uuid::new_v4());
        Self::open_below(runtime, &root)
    }

    fn open_below(runtime: &KernelRuntime, root_name: &str) -> Result<Self, ResourceServiceError> {
        runtime
            .verify_instance_lock()
            .map_err(|_| ResourceServiceError::unavailable())?;
        let snapshot = runtime
            .active_workspace_snapshot()
            .map_err(|_| ResourceServiceError::unavailable())?;
        let instance = runtime
            .instance_data_root()
            .try_clone_dir()
            .map_err(|_| ResourceServiceError::unavailable())?;
        let state_root = open_or_create_child(&instance, root_name)
            .map_err(|_| ResourceServiceError::unavailable())?;
        let workspace_name = snapshot.workspace().id.as_uuid().to_string();
        let directory = open_or_create_child(&state_root, &workspace_name)
            .map_err(|_| ResourceServiceError::unavailable())?;
        preflight_raw_directory(&directory)?;
        let canonical_root = runtime
            .instance_data_root()
            .canonical_path()
            .join(root_name)
            .join(&workspace_name);
        let durable = DurableFileStore::at_retained_directory(
            directory
                .try_clone()
                .map_err(|_| ResourceServiceError::unavailable())?,
            canonical_root,
            runtime.launch_epoch().value(),
        );
        let outcomes = durable
            .recover()
            .map_err(|_| ResourceServiceError::unavailable())?;
        if outcomes.iter().any(|outcome| {
            matches!(
                outcome,
                RecoveryOutcome::ManualInterventionRequired { .. }
                    | RecoveryOutcome::Committed {
                        commit_state: CommitState::PublishedDurabilityUncertain,
                        ..
                    }
            )
        }) {
            return Err(ResourceServiceError::unavailable());
        }
        let stable_entries = stable_record_count(&directory)?;
        Ok(Self {
            directory,
            durable,
            workspace_id: snapshot.workspace().id,
            workspace_generation: snapshot.workspace().generation.clone(),
            stable_entries: AtomicUsize::new(stable_entries),
        })
    }

    pub(super) fn load(
        &self,
        batch_id: ResourceBatchId,
    ) -> Result<Option<StoredBatchRecord>, ResourceServiceError> {
        if let Some(receipt) = self.read_named(receipt_name(batch_id)?, batch_id)? {
            if receipt.record.phase != BatchPhase::Committed {
                return Err(ResourceServiceError::unavailable());
            }
            return Ok(Some(receipt));
        }
        let pending = self.read_named(pending_name(batch_id)?, batch_id)?;
        if pending
            .as_ref()
            .is_some_and(|stored| stored.record.phase == BatchPhase::Committed)
        {
            return Err(ResourceServiceError::unavailable());
        }
        Ok(pending)
    }

    fn read_named(
        &self,
        name: StorageFileName,
        batch_id: ResourceBatchId,
    ) -> Result<Option<StoredBatchRecord>, ResourceServiceError> {
        let Some(stored) = self
            .durable
            .read(&name, MAX_RECORD_BYTES)
            .map_err(|_| ResourceServiceError::unavailable())?
        else {
            return Ok(None);
        };
        let record: BatchRecord = serde_json::from_slice(&stored.bytes)
            .map_err(|_| ResourceServiceError::unavailable())?;
        record.validate(self.workspace_id, &self.workspace_generation)?;
        if record.batch_id != batch_id {
            return Err(ResourceServiceError::unavailable());
        }
        Ok(Some(StoredBatchRecord {
            record,
            revision: stored.revision.clone(),
        }))
    }

    pub(super) fn create(
        &self,
        record: &BatchRecord,
    ) -> Result<StoredBatchRecord, CreateBatchRecordError> {
        record
            .validate(self.workspace_id, &self.workspace_generation)
            .map_err(CreateBatchRecordError::NotPublished)?;
        let bytes = serde_json::to_vec(record).map_err(|_| {
            CreateBatchRecordError::NotPublished(ResourceServiceError::unavailable())
        })?;
        if bytes.len() as u64 > MAX_RECORD_BYTES {
            return Err(CreateBatchRecordError::NotPublished(
                ResourceServiceError::too_large(),
            ));
        }
        let target = pending_name(record.batch_id).map_err(CreateBatchRecordError::NotPublished)?;
        let outcome = self
            .durable
            .replace(ReplaceRequest {
                target: &target,
                bytes: &bytes,
                expected: ExpectedFile::Absent,
                preserve_previous: PreservePrevious::None,
            })
            .map_err(|error| {
                let resource = ResourceServiceError::unavailable();
                if create_failure_requires_recovery(error.kind()) {
                    CreateBatchRecordError::RecoveryRequired(resource)
                } else {
                    CreateBatchRecordError::NotPublished(resource)
                }
            })?;
        if outcome.commit_state == CommitState::PublishedDurabilityUncertain {
            return Err(CreateBatchRecordError::RecoveryRequired(
                ResourceServiceError::unavailable(),
            ));
        }
        self.stable_entries.fetch_add(1, Ordering::AcqRel);
        Ok(StoredBatchRecord {
            record: record.clone(),
            revision: outcome.installed_revision,
        })
    }

    pub(super) fn preflight_record(
        &self,
        record: &BatchRecord,
    ) -> Result<(), ResourceServiceError> {
        record.validate(self.workspace_id, &self.workspace_generation)?;
        let bytes = serde_json::to_vec(record).map_err(|_| ResourceServiceError::unavailable())?;
        if bytes.len() as u64 > MAX_RECORD_BYTES {
            return Err(ResourceServiceError::too_large());
        }
        Ok(())
    }

    pub(super) fn has_capacity_for_new_batch(&self) -> Result<bool, ResourceServiceError> {
        Ok(self.stable_entries.load(Ordering::Acquire) <= MAX_STABLE_RECORDS.saturating_sub(2))
    }

    pub(super) fn replace(
        &self,
        record: &BatchRecord,
        expected: &FileRevision,
    ) -> Result<StoredBatchRecord, ResourceServiceError> {
        if record.phase == BatchPhase::Committed {
            self.commit(record, expected)
        } else {
            self.write_pending(record, ExpectedFile::Revision(expected))
        }
    }

    fn write_pending(
        &self,
        record: &BatchRecord,
        expected: ExpectedFile<'_>,
    ) -> Result<StoredBatchRecord, ResourceServiceError> {
        record.validate(self.workspace_id, &self.workspace_generation)?;
        let bytes = serde_json::to_vec(record).map_err(|_| ResourceServiceError::unavailable())?;
        if bytes.len() as u64 > MAX_RECORD_BYTES {
            return Err(ResourceServiceError::unavailable());
        }
        let target = pending_name(record.batch_id)?;
        let outcome = self
            .durable
            .replace(ReplaceRequest {
                target: &target,
                bytes: &bytes,
                expected,
                preserve_previous: PreservePrevious::None,
            })
            .map_err(|_| ResourceServiceError::unavailable())?;
        if outcome.commit_state == CommitState::PublishedDurabilityUncertain {
            return Err(ResourceServiceError::unavailable());
        }
        Ok(StoredBatchRecord {
            record: record.clone(),
            revision: outcome.installed_revision,
        })
    }

    fn commit(
        &self,
        record: &BatchRecord,
        pending_revision: &FileRevision,
    ) -> Result<StoredBatchRecord, ResourceServiceError> {
        record.validate(self.workspace_id, &self.workspace_generation)?;
        let bytes = serde_json::to_vec(record).map_err(|_| ResourceServiceError::unavailable())?;
        if bytes.len() as u64 > MAX_RECORD_BYTES {
            return Err(ResourceServiceError::unavailable());
        }
        let receipt = receipt_name(record.batch_id)?;
        let stored = match self
            .durable
            .read(&receipt, MAX_RECORD_BYTES)
            .map_err(|_| ResourceServiceError::unavailable())?
        {
            Some(existing) => {
                let existing_record: BatchRecord = serde_json::from_slice(&existing.bytes)
                    .map_err(|_| ResourceServiceError::unavailable())?;
                existing_record.validate(self.workspace_id, &self.workspace_generation)?;
                if existing_record != *record {
                    return Err(ResourceServiceError::unavailable());
                }
                StoredBatchRecord {
                    record: existing_record,
                    revision: existing.revision.clone(),
                }
            }
            None => {
                let outcome = self
                    .durable
                    .replace(ReplaceRequest {
                        target: &receipt,
                        bytes: &bytes,
                        expected: ExpectedFile::Absent,
                        preserve_previous: PreservePrevious::None,
                    })
                    .map_err(|_| ResourceServiceError::unavailable())?;
                if outcome.commit_state == CommitState::PublishedDurabilityUncertain {
                    return Err(ResourceServiceError::unavailable());
                }
                self.stable_entries.fetch_add(1, Ordering::AcqRel);
                StoredBatchRecord {
                    record: record.clone(),
                    revision: outcome.installed_revision,
                }
            }
        };
        Ok(StoredBatchRecord {
            record: stored.record,
            revision: pending_revision.clone(),
        })
    }

    pub(super) fn finalize_commit(
        &self,
        batch_id: ResourceBatchId,
        pending_revision: &FileRevision,
    ) -> Result<(), ResourceServiceError> {
        self.remove_pending(batch_id, pending_revision)
    }

    fn remove_pending(
        &self,
        batch_id: ResourceBatchId,
        expected: &FileRevision,
    ) -> Result<(), ResourceServiceError> {
        let name = pending_name(batch_id)?;
        let current = self
            .durable
            .read(&name, MAX_RECORD_BYTES)
            .map_err(|_| ResourceServiceError::unavailable())?
            .ok_or_else(ResourceServiceError::unavailable)?;
        if &current.revision != expected {
            return Err(ResourceServiceError::unavailable());
        }
        self.directory
            .remove_file(name.as_str())
            .map_err(|_| ResourceServiceError::unavailable())?;
        crate::storage::sync_directory(&self.directory)
            .map_err(|_| ResourceServiceError::unavailable())?;
        self.stable_entries.fetch_sub(1, Ordering::AcqRel);
        Ok(())
    }

    pub(super) fn pending_records(&self) -> Result<Vec<StoredBatchRecord>, ResourceServiceError> {
        let mut pending_ids = Vec::new();
        let mut receipt_ids = Vec::new();
        let mut seen_names = HashSet::new();
        let mut entry_count = 0_usize;
        for entry in self
            .directory
            .entries()
            .map_err(|_| ResourceServiceError::unavailable())?
        {
            entry_count = entry_count
                .checked_add(1)
                .filter(|count| *count <= MAX_STABLE_RECORDS)
                .ok_or_else(ResourceServiceError::unavailable)?;
            let entry = entry.map_err(|_| ResourceServiceError::unavailable())?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| ResourceServiceError::unavailable())?;
            let metadata = self
                .directory
                .symlink_metadata(&name)
                .map_err(|_| ResourceServiceError::unavailable())?;
            if !trusted_record_file(&metadata) {
                return Err(ResourceServiceError::unavailable());
            }
            if name.starts_with(".qingyu-storage-") {
                return Err(ResourceServiceError::unavailable());
            }
            if !seen_names.insert(name.clone()) {
                return Err(ResourceServiceError::unavailable());
            }
            let (encoded, is_receipt) = if let Some(encoded) = name
                .strip_prefix(PENDING_PREFIX)
                .and_then(|name| name.strip_suffix(RECORD_SUFFIX))
            {
                (encoded, false)
            } else if let Some(encoded) = name
                .strip_prefix(RECEIPT_PREFIX)
                .and_then(|name| name.strip_suffix(RECORD_SUFFIX))
            {
                (encoded, true)
            } else {
                return Err(ResourceServiceError::unavailable());
            };
            let id = uuid::Uuid::parse_str(encoded)
                .map(ResourceBatchId::new)
                .map_err(|_| ResourceServiceError::unavailable())?;
            let canonical = if is_receipt {
                receipt_name(id)?
            } else {
                pending_name(id)?
            };
            if canonical.as_str() != name {
                return Err(ResourceServiceError::unavailable());
            }
            if is_receipt {
                receipt_ids.push(id);
            } else {
                if pending_ids.len() >= MAX_PENDING_RECORDS {
                    return Err(ResourceServiceError::unavailable());
                }
                pending_ids.push(id);
            }
        }
        pending_ids.sort_by_key(|id| *id.as_uuid());
        receipt_ids.sort_by_key(|id| *id.as_uuid());
        let receipts = receipt_ids.iter().copied().collect::<HashSet<_>>();
        if receipts.len() != receipt_ids.len() {
            return Err(ResourceServiceError::unavailable());
        }
        let mut records = Vec::new();
        for id in pending_ids {
            let stored = self
                .read_named(pending_name(id)?, id)?
                .ok_or_else(ResourceServiceError::unavailable)?;
            if stored.record.phase == BatchPhase::Committed {
                return Err(ResourceServiceError::unavailable());
            }
            if receipts.contains(&id) {
                let receipt = self
                    .read_named(receipt_name(id)?, id)?
                    .ok_or_else(ResourceServiceError::unavailable)?;
                let mut expected = stored.record.clone();
                expected.phase = BatchPhase::Committed;
                if receipt.record != expected {
                    return Err(ResourceServiceError::unavailable());
                }
                records.push(StoredBatchRecord {
                    record: receipt.record,
                    revision: stored.revision,
                });
                continue;
            }
            if stored.record.phase != BatchPhase::Committed {
                records.push(stored);
            }
        }
        Ok(records)
    }
}

const fn create_failure_requires_recovery(kind: DurableFileFailureKind) -> bool {
    matches!(
        kind,
        DurableFileFailureKind::PublishStateUncertain
            | DurableFileFailureKind::RecoveryRequired
            | DurableFileFailureKind::UnsafeEntry
            | DurableFileFailureKind::RevisionConflict
            | DurableFileFailureKind::Unavailable
    )
}

fn preflight_raw_directory(directory: &Dir) -> Result<(), ResourceServiceError> {
    preflight_raw_directory_with_limit(directory, MAX_RAW_ENTRIES)
}

fn preflight_raw_directory_with_limit(
    directory: &Dir,
    maximum_entries: usize,
) -> Result<(), ResourceServiceError> {
    let mut count = 0_usize;
    for entry in directory
        .entries()
        .map_err(|_| ResourceServiceError::unavailable())?
    {
        let entry = entry.map_err(|_| ResourceServiceError::unavailable())?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| ResourceServiceError::unavailable())?;
        count = count
            .checked_add(1)
            .filter(|count| *count <= maximum_entries)
            .ok_or_else(ResourceServiceError::unavailable)?;
        let metadata = directory
            .symlink_metadata(&name)
            .map_err(|_| ResourceServiceError::unavailable())?;
        if !trusted_record_file(&metadata)
            || (record_name(&name).is_err() && !valid_storage_artifact_name(&name))
        {
            return Err(ResourceServiceError::unavailable());
        }
    }
    Ok(())
}

fn stable_record_count(directory: &Dir) -> Result<usize, ResourceServiceError> {
    let mut count = 0_usize;
    for entry in directory
        .entries()
        .map_err(|_| ResourceServiceError::unavailable())?
    {
        let entry = entry.map_err(|_| ResourceServiceError::unavailable())?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| ResourceServiceError::unavailable())?;
        let metadata = directory
            .symlink_metadata(&name)
            .map_err(|_| ResourceServiceError::unavailable())?;
        if !trusted_record_file(&metadata) || record_name(&name).is_err() {
            return Err(ResourceServiceError::unavailable());
        }
        count = count
            .checked_add(1)
            .filter(|count| *count <= MAX_STABLE_RECORDS)
            .ok_or_else(ResourceServiceError::unavailable)?;
    }
    Ok(count)
}

fn record_name(name: &str) -> Result<(ResourceBatchId, bool), ResourceServiceError> {
    let (encoded, is_receipt) = if let Some(encoded) = name
        .strip_prefix(PENDING_PREFIX)
        .and_then(|name| name.strip_suffix(RECORD_SUFFIX))
    {
        (encoded, false)
    } else if let Some(encoded) = name
        .strip_prefix(RECEIPT_PREFIX)
        .and_then(|name| name.strip_suffix(RECORD_SUFFIX))
    {
        (encoded, true)
    } else {
        return Err(ResourceServiceError::unavailable());
    };
    let id = uuid::Uuid::parse_str(encoded)
        .map(ResourceBatchId::new)
        .map_err(|_| ResourceServiceError::unavailable())?;
    let canonical = if is_receipt {
        receipt_name(id)?
    } else {
        pending_name(id)?
    };
    if canonical.as_str() != name {
        return Err(ResourceServiceError::unavailable());
    }
    Ok((id, is_receipt))
}

fn valid_storage_artifact_name(name: &str) -> bool {
    let Some(value) = name.strip_prefix(".qingyu-storage-") else {
        return false;
    };
    [".stage", ".intent", ".backup"].iter().any(|suffix| {
        value
            .strip_suffix(suffix)
            .and_then(|id| uuid::Uuid::parse_str(id).ok())
            .is_some_and(|id| format!("{id}{suffix}") == value)
    })
}

fn trusted_record_file(metadata: &cap_std::fs::Metadata) -> bool {
    metadata.is_file() && !metadata.file_type().is_symlink() && record_link_count(metadata) == 1
}

#[cfg(unix)]
fn record_link_count(metadata: &cap_std::fs::Metadata) -> u64 {
    cap_fs_ext::MetadataExt::nlink(metadata)
}

#[cfg(windows)]
fn record_link_count(metadata: &cap_std::fs::Metadata) -> u64 {
    cap_fs_ext::MetadataExt::number_of_links(metadata)
}

#[cfg(not(any(unix, windows)))]
const fn record_link_count(_metadata: &cap_std::fs::Metadata) -> u64 {
    1
}

pub(super) fn request_digest(
    workspace_id: WorkspaceId,
    workspace_generation: &WorkspaceGeneration,
    document_path: &WorkspaceRelativePath,
    folder: &WorkspaceRelativePath,
    items: &[CreateResourceBatchItem],
) -> String {
    let mut digest = Sha256::new();
    digest.update(DIGEST_DOMAIN);
    push_bytes(&mut digest, workspace_id.as_uuid().as_bytes());
    push_bytes(&mut digest, workspace_generation.as_str().as_bytes());
    push_bytes(&mut digest, document_path.as_str().as_bytes());
    push_bytes(&mut digest, folder.as_str().as_bytes());
    digest.update((items.len() as u64).to_be_bytes());
    for item in items {
        push_bytes(&mut digest, item.name.as_str().as_bytes());
        digest.update([match item.kind {
            ResourceKind::Image => 1,
            ResourceKind::Attachment => 2,
        }]);
        push_bytes(&mut digest, item.media_type.as_bytes());
        digest.update((item.body.len() as u64).to_be_bytes());
        digest.update(Sha256::digest(&item.body));
    }
    format!("sha256:{}", encode_digest(digest.finalize().into()))
}

fn request_digest_from_record(record: &BatchRecord) -> Result<String, ResourceServiceError> {
    let mut digest = Sha256::new();
    digest.update(DIGEST_DOMAIN);
    push_bytes(&mut digest, record.workspace_id.as_uuid().as_bytes());
    push_bytes(&mut digest, record.workspace_generation.as_str().as_bytes());
    push_bytes(&mut digest, record.document_path.as_str().as_bytes());
    push_bytes(&mut digest, record.folder.as_str().as_bytes());
    digest.update((record.items.len() as u64).to_be_bytes());
    for item in &record.items {
        push_bytes(&mut digest, item.requested_name.as_str().as_bytes());
        digest.update([match item.kind {
            ResourceKind::Image => 1,
            ResourceKind::Attachment => 2,
        }]);
        push_bytes(&mut digest, item.media_type.as_bytes());
        digest.update(item.size_bytes.to_be_bytes());
        digest.update(decode_digest(&item.content_sha256)?);
    }
    Ok(format!(
        "sha256:{}",
        encode_digest(digest.finalize().into())
    ))
}

pub(super) fn content_digest(bytes: &[u8]) -> String {
    encode_digest(Sha256::digest(bytes).into())
}

fn push_bytes(digest: &mut Sha256, bytes: &[u8]) {
    digest.update((bytes.len() as u64).to_be_bytes());
    digest.update(bytes);
}

fn encode_digest(bytes: [u8; 32]) -> String {
    use std::fmt::Write as _;

    let mut encoded = String::with_capacity(64);
    for byte in bytes {
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

fn decode_digest(value: &str) -> Result<[u8; 32], ResourceServiceError> {
    if value.len() != 64 || !lower_hex(value) {
        return Err(ResourceServiceError::unavailable());
    }
    let mut decoded = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let high = hex_nibble(pair[0]).ok_or_else(ResourceServiceError::unavailable)?;
        let low = hex_nibble(pair[1]).ok_or_else(ResourceServiceError::unavailable)?;
        decoded[index] = (high << 4) | low;
    }
    Ok(decoded)
}

const fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn pending_name(batch_id: ResourceBatchId) -> Result<StorageFileName, ResourceServiceError> {
    StorageFileName::parse(format!(
        "{PENDING_PREFIX}{}{RECORD_SUFFIX}",
        batch_id.as_uuid()
    ))
    .map_err(|_| ResourceServiceError::unavailable())
}

fn receipt_name(batch_id: ResourceBatchId) -> Result<StorageFileName, ResourceServiceError> {
    StorageFileName::parse(format!(
        "{RECEIPT_PREFIX}{}{RECORD_SUFFIX}",
        batch_id.as_uuid()
    ))
    .map_err(|_| ResourceServiceError::unavailable())
}

pub(super) fn stage_name(attempt_id: uuid::Uuid, ordinal: usize) -> String {
    format!(".qingyu-resource-batch-{attempt_id}-{ordinal}.tmp")
}

fn expected_target_parent(
    document_path: &WorkspaceRelativePath,
    folder: &WorkspaceRelativePath,
) -> Result<WorkspaceRelativePath, ResourceServiceError> {
    let parent = document_path
        .as_str()
        .rsplit_once('/')
        .map_or("", |(parent, _)| parent);
    WorkspaceRelativePath::parse(match (parent.is_empty(), folder.as_str().is_empty()) {
        (true, true) => String::new(),
        (true, false) => folder.as_str().to_string(),
        (false, true) => parent.to_string(),
        (false, false) => format!("{parent}/{}", folder.as_str()),
    })
    .map_err(|_| ResourceServiceError::unavailable())
}

fn expected_item_path(
    parent: &WorkspaceRelativePath,
    name: &ResourceName,
) -> Result<WorkspaceRelativePath, ResourceServiceError> {
    WorkspaceRelativePath::parse(if parent.as_str().is_empty() {
        name.as_str().to_string()
    } else {
        format!("{}/{}", parent.as_str(), name.as_str())
    })
    .map_err(|_| ResourceServiceError::unavailable())
}

pub(super) fn unique_resource_name(
    requested: &str,
    attempt: usize,
) -> Result<ResourceName, ResourceServiceError> {
    if attempt >= 10_000 {
        return Err(ResourceServiceError::unavailable());
    }
    if attempt == 0 {
        return ResourceName::parse(requested).map_err(|_| ResourceServiceError::invalid_path());
    }
    let suffix = format!("-{}", attempt + 1);
    let extension_index = requested.rfind('.').filter(|index| *index > 0);
    let (mut stem, extension) = extension_index.map_or((requested.to_string(), ""), |index| {
        (requested[..index].to_string(), &requested[index..])
    });
    loop {
        if let Ok(name) = ResourceName::parse(format!("{stem}{suffix}{extension}")) {
            return Ok(name);
        }
        if stem.pop().is_none() {
            return Err(ResourceServiceError::invalid_path());
        }
    }
}

fn markdown_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".md") || lower.ends_with(".markdown")
}

fn lower_hex(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn valid_kind_media(kind: ResourceKind, name: &str, media_type: &str) -> bool {
    match kind {
        ResourceKind::Attachment => media_type == "application/octet-stream",
        ResourceKind::Image => {
            let lower = name.to_ascii_lowercase();
            matches!(
                (
                    lower.rsplit_once('.').map(|(_, extension)| extension),
                    media_type
                ),
                (Some("png"), "image/png")
                    | (Some("jpg" | "jpeg"), "image/jpeg")
                    | (Some("gif"), "image/gif")
                    | (Some("webp"), "image/webp")
                    | (Some("bmp"), "image/bmp")
                    | (Some("avif"), "image/avif")
                    | (Some("svg"), "image/svg+xml")
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use cap_std::ambient_authority;
    use tempfile::tempdir;

    use super::*;

    fn record() -> BatchRecord {
        let workspace_id = WorkspaceId::new(uuid::Uuid::from_u128(1));
        let generation = WorkspaceGeneration::parse("generation-1").unwrap();
        let attempt_id = uuid::Uuid::from_u128(3);
        let mut record = BatchRecord::preparing(
            ResourceBatchId::new(uuid::Uuid::from_u128(2)),
            format!("sha256:{}", "a".repeat(64)),
            workspace_id,
            generation,
            WorkspaceRelativePath::parse("notes/note.md").unwrap(),
            WorkspaceRelativePath::parse("assets").unwrap(),
            WorkspaceRelativePath::parse("notes/assets").unwrap(),
            attempt_id,
            vec![BatchRecordItem {
                ordinal: 0,
                name_attempt: 0,
                requested_name: ResourceName::parse("image.png").unwrap(),
                target_name: ResourceName::parse("image.png").unwrap(),
                target_path: WorkspaceRelativePath::parse("notes/assets/image.png").unwrap(),
                kind: ResourceKind::Image,
                media_type: "image/png".to_string(),
                size_bytes: 4,
                content_sha256: "b".repeat(64),
                stage_name: stage_name(attempt_id, 0),
            }],
        );
        record.request_digest = request_digest_from_record(&record).unwrap();
        record
    }

    #[test]
    fn record_validation_accepts_a_canonical_bound_record() {
        let record = record();

        record
            .validate(record.workspace_id, &record.workspace_generation)
            .unwrap();
    }

    #[test]
    fn record_validation_rejects_a_stage_path_redirect() {
        let mut record = record();
        record.items[0].stage_name =
            ".qingyu-resource-batch-../../../workspace/user.png.tmp".to_string();

        assert!(record
            .validate(record.workspace_id, &record.workspace_generation)
            .is_err());
    }

    #[test]
    fn record_validation_rejects_a_target_outside_the_recorded_parent() {
        let mut record = record();
        record.items[0].target_path = WorkspaceRelativePath::parse("other/image.png").unwrap();

        assert!(record
            .validate(record.workspace_id, &record.workspace_generation)
            .is_err());
    }

    #[test]
    fn record_validation_rejects_digest_or_collision_tampering() {
        let mut digest = record();
        digest.request_digest = format!("sha256:{}", "0".repeat(64));
        assert!(digest
            .validate(digest.workspace_id, &digest.workspace_generation)
            .is_err());

        let mut collision = record();
        collision.items[0].name_attempt = 1;
        assert!(collision
            .validate(collision.workspace_id, &collision.workspace_generation)
            .is_err());
    }

    #[test]
    fn record_validation_rejects_markdown_or_case_folded_duplicate_targets() {
        let mut markdown = record();
        markdown.items[0].requested_name = ResourceName::parse("note.md").unwrap();
        markdown.items[0].target_name = ResourceName::parse("note.md").unwrap();
        markdown.items[0].target_path =
            WorkspaceRelativePath::parse("notes/assets/note.md").unwrap();
        markdown.request_digest = request_digest_from_record(&markdown).unwrap();
        assert!(markdown
            .validate(markdown.workspace_id, &markdown.workspace_generation)
            .is_err());

        let mut duplicate = record();
        let mut second = duplicate.items[0].clone();
        second.ordinal = 1;
        second.requested_name = ResourceName::parse("IMAGE.png").unwrap();
        second.target_name = ResourceName::parse("IMAGE.png").unwrap();
        second.target_path = WorkspaceRelativePath::parse("notes/assets/IMAGE.png").unwrap();
        second.stage_name = stage_name(duplicate.attempt_id, 1);
        duplicate.items.push(second);
        duplicate.request_digest = request_digest_from_record(&duplicate).unwrap();
        assert!(duplicate
            .validate(duplicate.workspace_id, &duplicate.workspace_generation)
            .is_err());
    }

    #[test]
    fn raw_directory_limit_reserves_three_durable_store_artifacts() {
        let temporary = tempdir().unwrap();
        let receipt_id = uuid::Uuid::from_u128(0x10);
        std::fs::write(
            temporary
                .path()
                .join(format!("{RECEIPT_PREFIX}{receipt_id}{RECORD_SUFFIX}")),
            b"receipt",
        )
        .unwrap();
        let transaction = uuid::Uuid::from_u128(0x11);
        for suffix in [".stage", ".intent", ".backup"] {
            std::fs::write(
                temporary
                    .path()
                    .join(format!(".qingyu-storage-{transaction}{suffix}")),
                b"artifact",
            )
            .unwrap();
        }
        let directory = Dir::open_ambient_dir(temporary.path(), ambient_authority()).unwrap();

        preflight_raw_directory_with_limit(&directory, 4).unwrap();
        assert!(preflight_raw_directory_with_limit(&directory, 3).is_err());
        assert_eq!(MAX_RAW_ENTRIES, MAX_STABLE_RECORDS + 3);
    }

    #[test]
    fn unexpected_create_conflicts_require_recovery() {
        assert!(create_failure_requires_recovery(
            DurableFileFailureKind::RevisionConflict
        ));
        assert!(create_failure_requires_recovery(
            DurableFileFailureKind::Unavailable
        ));
        assert!(!create_failure_requires_recovery(
            DurableFileFailureKind::NotPublished
        ));
        assert!(!create_failure_requires_recovery(
            DurableFileFailureKind::TooLarge
        ));
    }
}
