use std::fs;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::STANDARD_NO_PAD;
use sha2::{Digest, Sha256};
use tauri::Manager;
use uuid::Uuid;

#[cfg(unix)]
use cap_fs_ext::OpenOptionsExt;
use cap_fs_ext::{DirExt, FollowSymlinks, MetadataExt, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use qingyu_kernel::{
    contract::{Revision, Rfc3339Utc, SnapshotId, WorkspaceRelativePath},
    documents::{
        history::{DocumentHistoryError, DocumentHistoryStore},
        types::HistorySnapshot,
    },
};

use super::path::is_markdown_history_file;
use super::trusted_file::write_trusted_file_atomic;
use super::types::{MarkdownFileHistoryEntry, MarkdownFileHistoryFile};

const MARKDOWN_HISTORY_RETENTION_LIMIT: usize = 30;
const MAX_KERNEL_HISTORY_CONTENT_BYTES: usize = 16 * 1024 * 1024;
const MAX_KERNEL_HISTORY_RECORD_BYTES: u64 = 24 * 1024 * 1024;
const KERNEL_HISTORY_RELOCATION_MARKER: &str = ".relocated.json";
#[allow(dead_code)]
const KERNEL_HISTORY_DIRECTORY: &str = "kernel-v1";

#[derive(Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[allow(dead_code)]
struct KernelHistoryRecord {
    snapshot_id: SnapshotId,
    document_path: WorkspaceRelativePath,
    created_at: Rfc3339Utc,
    #[serde(with = "compact_history_contents")]
    contents: Vec<u8>,
    revision: Revision,
}

#[derive(Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct KernelHistoryRelocationMarker {
    target: WorkspaceRelativePath,
    snapshot_ids: Vec<SnapshotId>,
}

mod compact_history_contents {
    use super::{MAX_KERNEL_HISTORY_CONTENT_BYTES, STANDARD_NO_PAD};
    use base64::Engine as _;
    use serde::{Deserialize as _, Deserializer, Serializer};

    pub fn serialize<S>(contents: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if contents.len() > MAX_KERNEL_HISTORY_CONTENT_BYTES {
            return Err(serde::ser::Error::custom(
                "history contents exceed the limit",
            ));
        }
        serializer.serialize_str(&STANDARD_NO_PAD.encode(contents))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoded = String::deserialize(deserializer)?;
        let contents = STANDARD_NO_PAD
            .decode(encoded)
            .map_err(serde::de::Error::custom)?;
        if contents.len() > MAX_KERNEL_HISTORY_CONTENT_BYTES {
            return Err(serde::de::Error::custom(
                "history contents exceed the limit",
            ));
        }
        Ok(contents)
    }
}

/// Persistent, capability-addressed Kernel history adapter. This remains
/// uncomposed in Phase 1; production command ownership is unchanged.
#[allow(dead_code)]
pub(crate) struct KernelDocumentHistoryAdapter {
    directory: Dir,
    transaction: Mutex<()>,
    #[cfg(test)]
    fail_after_stage: AtomicBool,
    #[cfg(test)]
    fail_relocation_before_marker: AtomicBool,
}

#[allow(dead_code)]
impl KernelDocumentHistoryAdapter {
    pub(crate) fn new(history_root: &Path) -> Result<Self, String> {
        fs::create_dir_all(history_root).map_err(|error| error.to_string())?;
        let root = Dir::open_ambient_dir(history_root, cap_std::ambient_authority())
            .map_err(|error| error.to_string())?;
        match root.symlink_metadata(KERNEL_HISTORY_DIRECTORY) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err("Kernel history directory is unsafe".to_string());
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => root
                .create_dir(KERNEL_HISTORY_DIRECTORY)
                .map_err(|error| error.to_string())?,
            Err(error) => return Err(error.to_string()),
        }
        let directory = root
            .open_dir_nofollow(KERNEL_HISTORY_DIRECTORY)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            directory,
            transaction: Mutex::new(()),
            #[cfg(test)]
            fail_after_stage: AtomicBool::new(false),
            #[cfg(test)]
            fail_relocation_before_marker: AtomicBool::new(false),
        })
    }

    #[cfg(test)]
    fn fail_next_preserve_after_stage(&self) {
        self.fail_after_stage.store(true, Ordering::SeqCst);
    }

    #[cfg(test)]
    fn fail_next_relocation_before_marker(&self) {
        self.fail_relocation_before_marker
            .store(true, Ordering::SeqCst);
    }

    fn bucket_name(path: &WorkspaceRelativePath) -> String {
        hash_hex(path.as_str())
    }

    fn bucket(
        &self,
        path: &WorkspaceRelativePath,
        create: bool,
    ) -> Result<Option<Dir>, DocumentHistoryError> {
        let name = Self::bucket_name(path);
        if create {
            match self.directory.symlink_metadata(&name) {
                Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                    return Err(DocumentHistoryError);
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => self
                    .directory
                    .create_dir(&name)
                    .map_err(|_| DocumentHistoryError)?,
                Err(_) => return Err(DocumentHistoryError),
            }
        }
        match self.directory.open_dir_nofollow(&name) {
            Ok(directory) => {
                Self::cleanup_orphan_stages(&directory)?;
                Ok(Some(directory))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(DocumentHistoryError),
        }
    }

    fn cleanup_orphan_stages(directory: &Dir) -> Result<(), DocumentHistoryError> {
        #[derive(Clone, Copy)]
        enum StageKind {
            Snapshot,
            RelocationRecord,
            Marker,
        }
        let mut mutated = false;
        for entry in directory.entries().map_err(|_| DocumentHistoryError)? {
            let entry = entry.map_err(|_| DocumentHistoryError)?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let stage = [
                (".snapshot-", StageKind::Snapshot),
                (".relocation-record-", StageKind::RelocationRecord),
                (".relocation-marker-", StageKind::Marker),
            ]
            .into_iter()
            .find_map(|(prefix, kind)| {
                name.strip_prefix(prefix)
                    .and_then(|value| value.strip_suffix(".tmp"))
                    .map(|id| (kind, id))
            });
            let Some((kind, id)) = stage else {
                continue;
            };
            let Ok(parsed) = Uuid::parse_str(id) else {
                continue;
            };
            if parsed.hyphenated().to_string() != id {
                continue;
            }
            let final_name = match kind {
                StageKind::Snapshot | StageKind::RelocationRecord => {
                    let record = Self::read_record(directory, &name)?;
                    if matches!(kind, StageKind::Snapshot)
                        && record.snapshot_id.as_uuid() != &parsed
                    {
                        return Err(DocumentHistoryError);
                    }
                    format!("{}.json", record.snapshot_id.as_uuid())
                }
                StageKind::Marker => KERNEL_HISTORY_RELOCATION_MARKER.to_string(),
            };
            match directory.symlink_metadata(&final_name) {
                Ok(_) => {
                    let same = match kind {
                        StageKind::Snapshot | StageKind::RelocationRecord => {
                            Self::read_record(directory, &name)?
                                == Self::read_record(directory, &final_name)?
                        }
                        StageKind::Marker => {
                            let staged: KernelHistoryRelocationMarker = serde_json::from_slice(
                                &read_kernel_history_bytes(directory, &name, 4096)?,
                            )
                            .map_err(|_| DocumentHistoryError)?;
                            Self::relocation_marker(directory)? == Some(staged)
                        }
                    };
                    if !same {
                        return Err(DocumentHistoryError);
                    }
                    directory
                        .remove_file(&name)
                        .map_err(|_| DocumentHistoryError)?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    match kind {
                        StageKind::Snapshot | StageKind::RelocationRecord => {
                            let _validated = Self::read_record(directory, &name)?;
                        }
                        StageKind::Marker => {
                            let _validated: KernelHistoryRelocationMarker = serde_json::from_slice(
                                &read_kernel_history_bytes(directory, &name, 4096)?,
                            )
                            .map_err(|_| DocumentHistoryError)?;
                        }
                    }
                    rename_kernel_history_noreplace(directory, &name, &final_name)?;
                }
                Err(_) => return Err(DocumentHistoryError),
            }
            mutated = true;
        }
        if mutated {
            sync_kernel_history_directory(directory)?;
        }
        Ok(())
    }

    fn relocation_marker(
        directory: &Dir,
    ) -> Result<Option<KernelHistoryRelocationMarker>, DocumentHistoryError> {
        match directory.symlink_metadata(KERNEL_HISTORY_RELOCATION_MARKER) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(DocumentHistoryError),
            Ok(metadata) => {
                if !trusted_kernel_history_file(&metadata) || metadata.len() > 4096 {
                    return Err(DocumentHistoryError);
                }
                let bytes =
                    read_kernel_history_bytes(directory, KERNEL_HISTORY_RELOCATION_MARKER, 4096)?;
                serde_json::from_slice(&bytes)
                    .map(Some)
                    .map_err(|_| DocumentHistoryError)
            }
        }
    }

    fn publish_bytes_locked(
        directory: &Dir,
        name: &str,
        stage_prefix: &str,
        bytes: &[u8],
    ) -> Result<(), DocumentHistoryError> {
        let stage_name = format!("{stage_prefix}{}.tmp", Uuid::new_v4());
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = directory
            .open_with(&stage_name, &options)
            .map_err(|_| DocumentHistoryError)?;
        if file
            .write_all(bytes)
            .and_then(|()| file.sync_all())
            .is_err()
        {
            drop(file);
            let _cleanup = directory.remove_file(&stage_name);
            return Err(DocumentHistoryError);
        }
        drop(file);
        if rename_kernel_history_noreplace(directory, &stage_name, name).is_err() {
            let _cleanup = directory.remove_file(&stage_name);
            return Err(DocumentHistoryError);
        }
        sync_kernel_history_directory(directory)
    }

    fn publish_relocated_record_locked(
        directory: &Dir,
        record: &KernelHistoryRecord,
    ) -> Result<(), DocumentHistoryError> {
        let name = format!("{}.json", record.snapshot_id.as_uuid());
        match directory.symlink_metadata(&name) {
            Ok(_) => {
                return (Self::read_record(directory, &name)? == *record)
                    .then_some(())
                    .ok_or(DocumentHistoryError)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(DocumentHistoryError),
        }
        let bytes = serde_json::to_vec(record).map_err(|_| DocumentHistoryError)?;
        if bytes.len() as u64 > MAX_KERNEL_HISTORY_RECORD_BYTES {
            return Err(DocumentHistoryError);
        }
        Self::publish_bytes_locked(directory, &name, ".relocation-record-", &bytes)
    }

    fn publish_relocation_marker_locked(
        directory: &Dir,
        target: &WorkspaceRelativePath,
        snapshot_ids: Vec<SnapshotId>,
    ) -> Result<(), DocumentHistoryError> {
        let marker = KernelHistoryRelocationMarker {
            target: target.clone(),
            snapshot_ids,
        };
        if let Some(existing) = Self::relocation_marker(directory)? {
            return (existing == marker)
                .then_some(())
                .ok_or(DocumentHistoryError);
        }
        let bytes = serde_json::to_vec(&marker).map_err(|_| DocumentHistoryError)?;
        Self::publish_bytes_locked(
            directory,
            KERNEL_HISTORY_RELOCATION_MARKER,
            ".relocation-marker-",
            &bytes,
        )
    }

    fn reset_relocated_bucket_locked(directory: &Dir) -> Result<(), DocumentHistoryError> {
        if Self::relocation_marker(directory)?.is_none() {
            return Ok(());
        }
        let mut records = Vec::new();
        for entry in directory.entries().map_err(|_| DocumentHistoryError)? {
            let entry = entry.map_err(|_| DocumentHistoryError)?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                return Err(DocumentHistoryError);
            };
            if name == KERNEL_HISTORY_RELOCATION_MARKER {
                continue;
            }
            if name.ends_with(".json") {
                let record = Self::read_record(directory, &name)?;
                if name != format!("{}.json", record.snapshot_id.as_uuid()) {
                    return Err(DocumentHistoryError);
                }
                records.push(name);
            } else {
                return Err(DocumentHistoryError);
            }
        }
        for name in records {
            directory
                .remove_file(name)
                .map_err(|_| DocumentHistoryError)?;
        }
        directory
            .remove_file(KERNEL_HISTORY_RELOCATION_MARKER)
            .map_err(|_| DocumentHistoryError)?;
        sync_kernel_history_directory(directory)
    }

    fn read_record(
        directory: &Dir,
        name: &str,
    ) -> Result<KernelHistoryRecord, DocumentHistoryError> {
        let metadata = directory
            .symlink_metadata(name)
            .map_err(|_| DocumentHistoryError)?;
        if metadata.file_type().is_symlink()
            || !trusted_kernel_history_file(&metadata)
            || metadata.len() > MAX_KERNEL_HISTORY_RECORD_BYTES
        {
            return Err(DocumentHistoryError);
        }
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        let mut file = directory
            .open_with(name, &options)
            .map_err(|_| DocumentHistoryError)?;
        let before = file.metadata().map_err(|_| DocumentHistoryError)?;
        if !trusted_kernel_history_file(&before)
            || !same_kernel_history_file(&metadata, &before)
            || before.len() > MAX_KERNEL_HISTORY_RECORD_BYTES
        {
            return Err(DocumentHistoryError);
        }
        let mut bytes = Vec::with_capacity(before.len() as usize);
        (&mut file)
            .take(MAX_KERNEL_HISTORY_RECORD_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| DocumentHistoryError)?;
        let after = file.metadata().map_err(|_| DocumentHistoryError)?;
        let latest = directory
            .symlink_metadata(name)
            .map_err(|_| DocumentHistoryError)?;
        if bytes.len() as u64 > MAX_KERNEL_HISTORY_RECORD_BYTES
            || !trusted_kernel_history_file(&after)
            || !trusted_kernel_history_file(&latest)
            || !same_kernel_history_file(&before, &after)
            || !same_kernel_history_file(&after, &latest)
            || before.len() != after.len()
            || after.len() != bytes.len() as u64
            || before.modified().ok() != after.modified().ok()
        {
            return Err(DocumentHistoryError);
        }
        serde_json::from_slice(&bytes).map_err(|_| DocumentHistoryError)
    }

    fn read_all_locked(
        &self,
        path: &WorkspaceRelativePath,
    ) -> Result<Vec<HistorySnapshot>, DocumentHistoryError> {
        let Some(directory) = self.bucket(path, false)? else {
            return Ok(Vec::new());
        };
        if Self::relocation_marker(&directory)?.is_some() {
            return Ok(Vec::new());
        }
        Self::read_bucket_snapshots(&directory, path)
    }

    fn read_bucket_snapshots(
        directory: &Dir,
        path: &WorkspaceRelativePath,
    ) -> Result<Vec<HistorySnapshot>, DocumentHistoryError> {
        let mut snapshots = Vec::new();
        for entry in directory.entries().map_err(|_| DocumentHistoryError)? {
            let entry = entry.map_err(|_| DocumentHistoryError)?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if name == KERNEL_HISTORY_RELOCATION_MARKER {
                continue;
            }
            if !name.ends_with(".json") {
                continue;
            }
            let record = Self::read_record(directory, &name)?;
            if record.document_path != *path {
                return Err(DocumentHistoryError);
            }
            snapshots.push(HistorySnapshot {
                snapshot_id: record.snapshot_id,
                document_path: record.document_path,
                created_at: record.created_at,
                contents: record.contents,
                revision: record.revision,
            });
        }
        snapshots.sort_by(|left, right| {
            left.created_at
                .as_str()
                .cmp(right.created_at.as_str())
                .then(left.snapshot_id.as_uuid().cmp(right.snapshot_id.as_uuid()))
        });
        Ok(snapshots)
    }

    fn known_history_paths_locked(
        &self,
    ) -> Result<Vec<WorkspaceRelativePath>, DocumentHistoryError> {
        let mut paths = Vec::new();
        for entry in self.directory.entries().map_err(|_| DocumentHistoryError)? {
            let entry = entry.map_err(|_| DocumentHistoryError)?;
            let Some(bucket_name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let metadata = self
                .directory
                .symlink_metadata(&bucket_name)
                .map_err(|_| DocumentHistoryError)?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(DocumentHistoryError);
            }
            let bucket = self
                .directory
                .open_dir_nofollow(&bucket_name)
                .map_err(|_| DocumentHistoryError)?;
            Self::cleanup_orphan_stages(&bucket)?;
            if Self::relocation_marker(&bucket)?.is_some() {
                continue;
            }
            let mut path = None;
            for record_entry in bucket.entries().map_err(|_| DocumentHistoryError)? {
                let record_entry = record_entry.map_err(|_| DocumentHistoryError)?;
                let Some(name) = record_entry.file_name().to_str().map(str::to_owned) else {
                    continue;
                };
                if !name.ends_with(".json") {
                    continue;
                }
                let record = Self::read_record(&bucket, &name)?;
                if Self::bucket_name(&record.document_path) != bucket_name {
                    return Err(DocumentHistoryError);
                }
                match path.as_ref() {
                    Some(existing) if existing != &record.document_path => {
                        return Err(DocumentHistoryError)
                    }
                    Some(_) => {}
                    None => path = Some(record.document_path),
                }
            }
            if let Some(path) = path {
                paths.push(path);
            }
        }
        Ok(paths)
    }

    fn relocate_one_locked(
        &self,
        source: &WorkspaceRelativePath,
        target: &WorkspaceRelativePath,
    ) -> Result<(), DocumentHistoryError> {
        let Some(source_directory) = self.bucket(source, false)? else {
            return Ok(());
        };
        if let Some(marker) = Self::relocation_marker(&source_directory)? {
            if marker.target != *target {
                return Err(DocumentHistoryError);
            }
            let source_snapshots = Self::read_bucket_snapshots(&source_directory, source)?;
            let mut source_ids = source_snapshots
                .iter()
                .map(|snapshot| snapshot.snapshot_id)
                .collect::<Vec<_>>();
            source_ids.sort_by_key(|id| *id.as_uuid());
            if source_ids != marker.snapshot_ids {
                return Err(DocumentHistoryError);
            }
            let target_snapshots = self.read_all_locked(target)?;
            return source_snapshots
                .iter()
                .all(|source_snapshot| {
                    target_snapshots.iter().any(|target_snapshot| {
                        target_snapshot.snapshot_id == source_snapshot.snapshot_id
                            && target_snapshot.created_at == source_snapshot.created_at
                            && target_snapshot.contents == source_snapshot.contents
                            && target_snapshot.revision == source_snapshot.revision
                    })
                })
                .then_some(())
                .ok_or(DocumentHistoryError);
        }
        let source_snapshots = Self::read_bucket_snapshots(&source_directory, source)?;
        if source_snapshots.is_empty() {
            return Ok(());
        }
        let target_directory = self.bucket(target, true)?.ok_or(DocumentHistoryError)?;
        Self::reset_relocated_bucket_locked(&target_directory)?;
        let mut expected = source_snapshots
            .iter()
            .map(|snapshot| HistorySnapshot {
                snapshot_id: snapshot.snapshot_id,
                document_path: target.clone(),
                created_at: snapshot.created_at.clone(),
                contents: snapshot.contents.clone(),
                revision: snapshot.revision.clone(),
            })
            .collect::<Vec<_>>();
        expected.sort_by(|left, right| {
            left.created_at
                .as_str()
                .cmp(right.created_at.as_str())
                .then(left.snapshot_id.as_uuid().cmp(right.snapshot_id.as_uuid()))
        });
        for snapshot in &expected {
            let record = KernelHistoryRecord {
                snapshot_id: snapshot.snapshot_id,
                document_path: target.clone(),
                created_at: snapshot.created_at.clone(),
                contents: snapshot.contents.clone(),
                revision: snapshot.revision.clone(),
            };
            Self::publish_relocated_record_locked(&target_directory, &record)?;
        }
        let published = self.read_all_locked(target)?;
        if !expected
            .iter()
            .all(|snapshot| published.iter().any(|candidate| candidate == snapshot))
        {
            return Err(DocumentHistoryError);
        }
        #[cfg(test)]
        if self
            .fail_relocation_before_marker
            .swap(false, Ordering::SeqCst)
        {
            return Err(DocumentHistoryError);
        }
        let mut snapshot_ids = source_snapshots
            .iter()
            .map(|snapshot| snapshot.snapshot_id)
            .collect::<Vec<_>>();
        snapshot_ids.sort_by_key(|id| *id.as_uuid());
        Self::publish_relocation_marker_locked(&source_directory, target, snapshot_ids)
    }

    fn prune_locked(&self, path: &WorkspaceRelativePath) -> Result<(), DocumentHistoryError> {
        let snapshots = self.read_all_locked(path)?;
        let remove_count = snapshots
            .len()
            .saturating_sub(MARKDOWN_HISTORY_RETENTION_LIMIT);
        if remove_count == 0 {
            return Ok(());
        }
        let directory = self.bucket(path, false)?.ok_or(DocumentHistoryError)?;
        for snapshot in snapshots.into_iter().take(remove_count) {
            directory
                .remove_file(format!("{}.json", snapshot.snapshot_id.as_uuid()))
                .map_err(|_| DocumentHistoryError)?;
        }
        sync_kernel_history_directory(&directory)
    }
}

fn read_kernel_history_bytes(
    directory: &Dir,
    name: &str,
    maximum_bytes: u64,
) -> Result<Vec<u8>, DocumentHistoryError> {
    let metadata = directory
        .symlink_metadata(name)
        .map_err(|_| DocumentHistoryError)?;
    if !trusted_kernel_history_file(&metadata) || metadata.len() > maximum_bytes {
        return Err(DocumentHistoryError);
    }
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let mut file = directory
        .open_with(name, &options)
        .map_err(|_| DocumentHistoryError)?;
    let before = file.metadata().map_err(|_| DocumentHistoryError)?;
    if !trusted_kernel_history_file(&before)
        || !same_kernel_history_file(&metadata, &before)
        || before.len() > maximum_bytes
    {
        return Err(DocumentHistoryError);
    }
    let mut bytes = Vec::with_capacity(before.len() as usize);
    (&mut file)
        .take(maximum_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| DocumentHistoryError)?;
    let after = file.metadata().map_err(|_| DocumentHistoryError)?;
    let latest = directory
        .symlink_metadata(name)
        .map_err(|_| DocumentHistoryError)?;
    if bytes.len() as u64 > maximum_bytes
        || !trusted_kernel_history_file(&after)
        || !trusted_kernel_history_file(&latest)
        || !same_kernel_history_file(&before, &after)
        || !same_kernel_history_file(&after, &latest)
        || before.len() != after.len()
        || after.len() != bytes.len() as u64
        || before.modified().ok() != after.modified().ok()
    {
        return Err(DocumentHistoryError);
    }
    Ok(bytes)
}

#[allow(dead_code)]
impl DocumentHistoryStore for KernelDocumentHistoryAdapter {
    fn preserve(
        &self,
        path: &WorkspaceRelativePath,
        contents: &[u8],
        revision: &Revision,
        created_at: &Rfc3339Utc,
    ) -> Result<SnapshotId, DocumentHistoryError> {
        if contents.len() > MAX_KERNEL_HISTORY_CONTENT_BYTES {
            return Err(DocumentHistoryError);
        }
        let _transaction = self.transaction.lock().map_err(|_| DocumentHistoryError)?;
        let directory = self.bucket(path, true)?.ok_or(DocumentHistoryError)?;
        Self::reset_relocated_bucket_locked(&directory)?;
        let snapshot_id = SnapshotId::new(Uuid::new_v4());
        let record = KernelHistoryRecord {
            snapshot_id,
            document_path: path.clone(),
            created_at: created_at.clone(),
            contents: contents.to_vec(),
            revision: revision.clone(),
        };
        let bytes = serde_json::to_vec(&record).map_err(|_| DocumentHistoryError)?;
        if bytes.len() as u64 > MAX_KERNEL_HISTORY_RECORD_BYTES {
            return Err(DocumentHistoryError);
        }
        let name = format!(
            "{snapshot_id_value}.json",
            snapshot_id_value = snapshot_id.as_uuid()
        );
        let stage_name = format!(".snapshot-{}.tmp", snapshot_id.as_uuid());
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = directory
            .open_with(&stage_name, &options)
            .map_err(|_| DocumentHistoryError)?;
        if file
            .write_all(&bytes)
            .and_then(|()| file.sync_all())
            .is_err()
        {
            drop(file);
            let _cleanup = directory.remove_file(&stage_name);
            return Err(DocumentHistoryError);
        }
        drop(file);
        #[cfg(test)]
        if self.fail_after_stage.swap(false, Ordering::SeqCst) {
            let _cleanup = directory.remove_file(&stage_name);
            return Err(DocumentHistoryError);
        }
        if rename_kernel_history_noreplace(&directory, &stage_name, &name).is_err() {
            let _cleanup = directory.remove_file(&stage_name);
            return Err(DocumentHistoryError);
        }
        sync_kernel_history_directory(&directory)?;
        self.prune_locked(path)?;
        Ok(snapshot_id)
    }

    fn list(
        &self,
        path: &WorkspaceRelativePath,
    ) -> Result<Vec<HistorySnapshot>, DocumentHistoryError> {
        let _transaction = self.transaction.lock().map_err(|_| DocumentHistoryError)?;
        self.read_all_locked(path)
    }

    fn get(
        &self,
        path: &WorkspaceRelativePath,
        snapshot_id: SnapshotId,
    ) -> Result<Option<HistorySnapshot>, DocumentHistoryError> {
        let _transaction = self.transaction.lock().map_err(|_| DocumentHistoryError)?;
        let Some(directory) = self.bucket(path, false)? else {
            return Ok(None);
        };
        if Self::relocation_marker(&directory)?.is_some() {
            return Ok(None);
        }
        let name = format!("{}.json", snapshot_id.as_uuid());
        let record = match Self::read_record(&directory, &name) {
            Ok(record) => record,
            Err(_)
                if directory
                    .symlink_metadata(&name)
                    .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
            {
                return Ok(None)
            }
            Err(error) => return Err(error),
        };
        if record.snapshot_id != snapshot_id || record.document_path != *path {
            return Err(DocumentHistoryError);
        }
        Ok(Some(HistorySnapshot {
            snapshot_id: record.snapshot_id,
            document_path: record.document_path,
            created_at: record.created_at,
            contents: record.contents,
            revision: record.revision,
        }))
    }

    fn relocate(
        &self,
        source: &WorkspaceRelativePath,
        target: &WorkspaceRelativePath,
        kind: qingyu_kernel::contract::DocumentKind,
    ) -> Result<(), DocumentHistoryError> {
        let _transaction = self.transaction.lock().map_err(|_| DocumentHistoryError)?;
        let sources = if kind == qingyu_kernel::contract::DocumentKind::File {
            vec![source.clone()]
        } else {
            self.known_history_paths_locked()?
                .into_iter()
                .filter(|candidate| {
                    candidate == source
                        || candidate
                            .as_str()
                            .strip_prefix(source.as_str())
                            .is_some_and(|suffix| suffix.starts_with('/'))
                })
                .collect()
        };
        for source_path in sources {
            let suffix = source_path
                .as_str()
                .strip_prefix(source.as_str())
                .ok_or(DocumentHistoryError)?;
            let target_value = if suffix.is_empty() {
                target.as_str().to_string()
            } else {
                format!("{}{suffix}", target.as_str())
            };
            let target_path =
                WorkspaceRelativePath::parse(target_value).map_err(|_| DocumentHistoryError)?;
            self.relocate_one_locked(&source_path, &target_path)?;
        }
        Ok(())
    }
}

fn trusted_kernel_history_file(metadata: &cap_std::fs::Metadata) -> bool {
    metadata.is_file()
        && !metadata.file_type().is_symlink()
        && kernel_history_link_count(metadata) == 1
}

fn same_kernel_history_file(left: &cap_std::fs::Metadata, right: &cap_std::fs::Metadata) -> bool {
    MetadataExt::dev(left) == MetadataExt::dev(right)
        && MetadataExt::ino(left) == MetadataExt::ino(right)
}

#[cfg(unix)]
fn kernel_history_link_count(metadata: &cap_std::fs::Metadata) -> u64 {
    MetadataExt::nlink(metadata)
}

#[cfg(windows)]
fn kernel_history_link_count(metadata: &cap_std::fs::Metadata) -> u64 {
    MetadataExt::nlink(metadata)
}

#[cfg(not(any(unix, windows)))]
fn kernel_history_link_count(_metadata: &cap_std::fs::Metadata) -> u64 {
    1
}

fn rename_kernel_history_noreplace(
    directory: &Dir,
    source: &str,
    target: &str,
) -> Result<(), DocumentHistoryError> {
    crate::atomic_noreplace::rename_noreplace(
        directory,
        Path::new(source),
        directory,
        Path::new(target),
    )
    .map_err(|_| DocumentHistoryError)
}

#[cfg(unix)]
#[allow(dead_code)]
fn sync_kernel_history_directory(directory: &Dir) -> Result<(), DocumentHistoryError> {
    rustix::fs::fsync(directory).map_err(|_| DocumentHistoryError)
}

#[cfg(not(unix))]
#[allow(dead_code)]
fn sync_kernel_history_directory(_directory: &Dir) -> Result<(), DocumentHistoryError> {
    Ok(())
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct MarkdownFileHistoryIndex {
    document_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    current_entry_id: Option<String>,
    entries: Vec<MarkdownFileHistoryEntry>,
}

fn current_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn hash_hex(input: impl AsRef<[u8]>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input);
    format!("{:x}", hasher.finalize())
}

fn canonical_history_document_path(path: &Path) -> String {
    fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string()
}

pub(super) fn markdown_history_root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("markdown-history"))
        .map_err(|error| error.to_string())
}

fn markdown_history_bucket(root: &Path, document_path: &str) -> PathBuf {
    root.join(hash_hex(document_path))
}

fn markdown_history_index_path(bucket: &Path) -> PathBuf {
    bucket.join("index.json")
}

fn markdown_history_snapshots_dir(bucket: &Path) -> PathBuf {
    bucket.join("snapshots")
}

fn markdown_history_snapshot_path(bucket: &Path, id: &str) -> PathBuf {
    markdown_history_snapshots_dir(bucket).join(format!("{id}.md"))
}

fn empty_markdown_history_index(document_path: String) -> MarkdownFileHistoryIndex {
    MarkdownFileHistoryIndex {
        document_path,
        current_entry_id: None,
        entries: Vec::new(),
    }
}

fn load_markdown_history_index(
    root: &Path,
    path: &Path,
) -> Result<(PathBuf, MarkdownFileHistoryIndex), String> {
    let document_path = canonical_history_document_path(path);
    let bucket = markdown_history_bucket(root, &document_path);
    let index_path = markdown_history_index_path(&bucket);
    if !index_path.exists() {
        return Ok((bucket, empty_markdown_history_index(document_path)));
    }

    let index = serde_json::from_str::<MarkdownFileHistoryIndex>(
        &fs::read_to_string(&index_path).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;

    Ok((bucket, index))
}

fn save_markdown_history_index(
    bucket: &Path,
    index: &MarkdownFileHistoryIndex,
) -> Result<(), String> {
    fs::create_dir_all(bucket).map_err(|error| error.to_string())?;
    fs::write(
        markdown_history_index_path(bucket),
        serde_json::to_string_pretty(index).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

fn normalize_markdown_history_entries(index: &mut MarkdownFileHistoryIndex) {
    let mut seen_ids = Vec::new();
    index.entries.retain(|entry| {
        if seen_ids.iter().any(|id| id == &entry.id) {
            return false;
        }

        seen_ids.push(entry.id.clone());
        true
    });
}

fn prune_markdown_history(bucket: &Path, index: &mut MarkdownFileHistoryIndex) {
    normalize_markdown_history_entries(index);
    let removed_entries = if index.entries.len() > MARKDOWN_HISTORY_RETENTION_LIMIT {
        index.entries.split_off(MARKDOWN_HISTORY_RETENTION_LIMIT)
    } else {
        Vec::new()
    };

    for entry in removed_entries {
        let _remove_result = fs::remove_file(markdown_history_snapshot_path(bucket, &entry.id));
    }
}

fn remove_markdown_history_snapshots(bucket: &Path, entries: &[MarkdownFileHistoryEntry]) {
    for entry in entries {
        let _remove_result = fs::remove_file(markdown_history_snapshot_path(bucket, &entry.id));
    }
}

fn truncate_markdown_history_after_current_entry(
    bucket: &Path,
    index: &mut MarkdownFileHistoryIndex,
) -> bool {
    let Some(current_entry_id) = index.current_entry_id.take() else {
        return false;
    };

    normalize_markdown_history_entries(index);
    let Some(current_index) = index
        .entries
        .iter()
        .position(|entry| entry.id == current_entry_id)
    else {
        return true;
    };
    let future_entries = index.entries.drain(..current_index).collect::<Vec<_>>();
    remove_markdown_history_snapshots(bucket, &future_entries);

    true
}

fn newest_markdown_history_contents(
    bucket: &Path,
    index: &MarkdownFileHistoryIndex,
) -> Option<String> {
    index.entries.first().and_then(|entry| {
        fs::read_to_string(markdown_history_snapshot_path(bucket, &entry.id)).ok()
    })
}

fn markdown_history_snapshot_id(bucket: &Path, created_at: u64, contents: &str) -> String {
    let content_hash = hash_hex(contents);
    let base_id = format!("{created_at}-{}", &content_hash[..12]);
    let mut id = base_id.clone();
    let mut suffix = 1;

    while markdown_history_snapshot_path(bucket, &id).exists() {
        suffix += 1;
        id = format!("{base_id}-{suffix}");
    }

    id
}

fn snapshot_markdown_file_history(
    root: &Path,
    path: &Path,
    next_contents: &str,
) -> Result<(), String> {
    if !is_markdown_history_file(path) || !path.is_file() {
        return Ok(());
    }

    let current_contents = fs::read_to_string(path).map_err(|error| error.to_string())?;
    snapshot_markdown_file_history_contents(root, path, &current_contents, next_contents)
}

pub(super) fn snapshot_markdown_file_history_contents(
    root: &Path,
    path: &Path,
    current_contents: &str,
    next_contents: &str,
) -> Result<(), String> {
    if !is_markdown_history_file(path) {
        return Ok(());
    }
    if current_contents == next_contents {
        return Ok(());
    }

    let (bucket, mut index) = load_markdown_history_index(root, path)?;
    let history_was_truncated = truncate_markdown_history_after_current_entry(&bucket, &mut index);
    if newest_markdown_history_contents(&bucket, &index).as_deref() == Some(current_contents) {
        if history_was_truncated {
            save_markdown_history_index(&bucket, &index)?;
        }
        return Ok(());
    }

    let created_at = current_time_millis();
    let id = markdown_history_snapshot_id(&bucket, created_at, current_contents);
    let snapshots_dir = markdown_history_snapshots_dir(&bucket);
    fs::create_dir_all(&snapshots_dir).map_err(|error| error.to_string())?;
    fs::write(
        markdown_history_snapshot_path(&bucket, &id),
        current_contents,
    )
    .map_err(|error| error.to_string())?;

    index.entries.insert(
        0,
        MarkdownFileHistoryEntry {
            id,
            created_at,
            size_bytes: current_contents.len() as u64,
        },
    );
    prune_markdown_history(&bucket, &mut index);
    save_markdown_history_index(&bucket, &index)
}

fn mark_markdown_history_current_entry(root: &Path, path: &Path, id: String) -> Result<(), String> {
    let (bucket, mut index) = load_markdown_history_index(root, path)?;
    if !index.entries.iter().any(|entry| entry.id == id) {
        return Err("History version was not found".to_string());
    }

    index.current_entry_id = Some(id);
    save_markdown_history_index(&bucket, &index)
}

fn list_markdown_file_history_with_root(
    root: &Path,
    path: String,
) -> Result<Vec<MarkdownFileHistoryEntry>, String> {
    let path_buf = PathBuf::from(path);
    let (_bucket, mut index) = load_markdown_history_index(root, &path_buf)?;
    normalize_markdown_history_entries(&mut index);

    Ok(index.entries)
}

fn read_markdown_file_history_with_root(
    root: &Path,
    path: String,
    id: String,
) -> Result<MarkdownFileHistoryFile, String> {
    let path_buf = PathBuf::from(path);
    let (bucket, index) = load_markdown_history_index(root, &path_buf)?;
    let entry = index
        .entries
        .iter()
        .find(|entry| entry.id == id)
        .ok_or_else(|| "History version was not found".to_string())?;
    let contents = fs::read_to_string(markdown_history_snapshot_path(&bucket, &entry.id))
        .map_err(|error| error.to_string())?;

    Ok(MarkdownFileHistoryFile {
        id: entry.id.clone(),
        contents,
    })
}

pub(super) fn write_markdown_file_with_history_root(
    root: &Path,
    path: String,
    contents: String,
) -> Result<(), String> {
    write_markdown_file_with_optional_history_root(Some(root), path, contents, false, None)
}

pub(super) fn write_markdown_file_with_optional_history_root(
    root: Option<&Path>,
    path: String,
    contents: String,
    skip_history_snapshot: bool,
    history_cursor_id: Option<String>,
) -> Result<(), String> {
    let path_buf = PathBuf::from(path);
    if let Some(root) = root.filter(|_| !skip_history_snapshot) {
        let _history_result = snapshot_markdown_file_history(root, &path_buf, &contents);
    }

    write_trusted_file_atomic(&path_buf, contents.as_bytes())?;

    if skip_history_snapshot {
        if let (Some(root), Some(history_cursor_id)) = (root, history_cursor_id) {
            mark_markdown_history_current_entry(root, &path_buf, history_cursor_id)?;
        }
    }

    Ok(())
}

#[tauri::command]
pub(crate) fn list_markdown_file_history(
    app: tauri::AppHandle,
    path: String,
) -> Result<Vec<MarkdownFileHistoryEntry>, String> {
    list_markdown_file_history_with_root(&markdown_history_root(&app)?, path)
}

#[tauri::command]
pub(crate) fn read_markdown_file_history(
    app: tauri::AppHandle,
    path: String,
    id: String,
) -> Result<MarkdownFileHistoryFile, String> {
    read_markdown_file_history_with_root(&markdown_history_root(&app)?, path, id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernel_history_adapter_persists_relative_snapshots_across_reconstruction() {
        let fixture = tempfile::tempdir().unwrap();
        let path = WorkspaceRelativePath::parse("folder/note.md").unwrap();
        let revision = Revision::parse("revision-a").unwrap();
        let created_at = Rfc3339Utc::parse("2026-07-29T00:00:00Z").unwrap();
        let first = KernelDocumentHistoryAdapter::new(fixture.path()).unwrap();
        let snapshot_id = first
            .preserve(&path, b"contents", &revision, &created_at)
            .unwrap();
        drop(first);

        let second = KernelDocumentHistoryAdapter::new(fixture.path()).unwrap();
        let snapshot = second.get(&path, snapshot_id).unwrap().unwrap();
        assert_eq!(snapshot.contents, b"contents");
        assert_eq!(snapshot.document_path, path);
        assert_eq!(snapshot.revision, revision);
        assert!(second
            .list(&WorkspaceRelativePath::parse("other.md").unwrap())
            .unwrap()
            .is_empty());
        assert!(!fixture
            .path()
            .to_string_lossy()
            .contains(snapshot.document_path.as_str()));
    }

    #[test]
    fn kernel_history_adapter_round_trips_the_maximum_legal_document_size() {
        let fixture = tempfile::tempdir().unwrap();
        let path = WorkspaceRelativePath::parse("large.md").unwrap();
        let contents = vec![b'x'; 16 * 1024 * 1024];
        let revision = Revision::parse(format!("{:x}", Sha256::digest(&contents))).unwrap();
        let created_at = Rfc3339Utc::parse("2026-07-29T00:00:00Z").unwrap();
        let first = KernelDocumentHistoryAdapter::new(fixture.path()).unwrap();

        let snapshot_id = first
            .preserve(&path, &contents, &revision, &created_at)
            .expect("the maximum legal document should remain a readable snapshot");
        drop(first);

        let second = KernelDocumentHistoryAdapter::new(fixture.path()).unwrap();
        let snapshot = second.get(&path, snapshot_id).unwrap().unwrap();
        assert_eq!(snapshot.contents.len(), contents.len());
        assert_eq!(snapshot.contents, contents);
        assert_eq!(second.list(&path).unwrap().len(), 1);
    }

    #[test]
    fn kernel_history_adapter_rejects_oversized_contents_before_publishing_a_record() {
        let fixture = tempfile::tempdir().unwrap();
        let path = WorkspaceRelativePath::parse("too-large.md").unwrap();
        let contents = vec![b'x'; MAX_KERNEL_HISTORY_CONTENT_BYTES + 1];
        let adapter = KernelDocumentHistoryAdapter::new(fixture.path()).unwrap();

        assert!(adapter
            .preserve(
                &path,
                &contents,
                &Revision::parse("oversized").unwrap(),
                &Rfc3339Utc::parse("2026-07-29T00:00:00Z").unwrap(),
            )
            .is_err());

        let kernel_root = fixture.path().join(KERNEL_HISTORY_DIRECTORY);
        assert!(std::fs::read_dir(kernel_root).unwrap().next().is_none());
    }

    #[test]
    fn kernel_history_adapter_finalizes_a_valid_orphan_snapshot_stage_on_reopen() {
        let fixture = tempfile::tempdir().unwrap();
        let path = WorkspaceRelativePath::parse("note.md").unwrap();
        let bucket = fixture
            .path()
            .join(KERNEL_HISTORY_DIRECTORY)
            .join(KernelDocumentHistoryAdapter::bucket_name(&path));
        let first = KernelDocumentHistoryAdapter::new(fixture.path()).unwrap();
        std::fs::create_dir(&bucket).unwrap();
        let snapshot_id = SnapshotId::new(Uuid::new_v4());
        let stage = bucket.join(format!(".snapshot-{}.tmp", snapshot_id.as_uuid()));
        std::fs::write(
            &stage,
            serde_json::to_vec(&KernelHistoryRecord {
                snapshot_id,
                document_path: path.clone(),
                created_at: Rfc3339Utc::parse("2026-07-29T00:00:00Z").unwrap(),
                contents: b"orphan contents".to_vec(),
                revision: Revision::parse("orphan-revision").unwrap(),
            })
            .unwrap(),
        )
        .unwrap();
        drop(first);

        let second = KernelDocumentHistoryAdapter::new(fixture.path()).unwrap();
        let history = second.list(&path).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].snapshot_id, snapshot_id);
        assert_eq!(history[0].contents, b"orphan contents");
        assert!(!stage.exists());
        assert!(bucket
            .join(format!("{}.json", snapshot_id.as_uuid()))
            .exists());
    }

    #[test]
    fn kernel_history_adapter_relocates_snapshots_across_reconstruction() {
        let fixture = tempfile::tempdir().unwrap();
        let source = WorkspaceRelativePath::parse("source.md").unwrap();
        let target = WorkspaceRelativePath::parse("folder/target.md").unwrap();
        let revision = Revision::parse("revision-a").unwrap();
        let created_at = Rfc3339Utc::parse("2026-07-29T00:00:00Z").unwrap();
        let first = KernelDocumentHistoryAdapter::new(fixture.path()).unwrap();
        let snapshot_id = first
            .preserve(&source, b"contents", &revision, &created_at)
            .unwrap();

        first
            .relocate(
                &source,
                &target,
                qingyu_kernel::contract::DocumentKind::File,
            )
            .unwrap();
        drop(first);

        let second = KernelDocumentHistoryAdapter::new(fixture.path()).unwrap();
        assert!(second.list(&source).unwrap().is_empty());
        let snapshot = second.get(&target, snapshot_id).unwrap().unwrap();
        assert_eq!(snapshot.document_path, target);
        assert_eq!(snapshot.contents, b"contents");
        assert_eq!(snapshot.revision, revision);
    }

    #[test]
    fn kernel_history_relocation_retries_an_existing_verified_target_bucket() {
        let fixture = tempfile::tempdir().unwrap();
        let source = WorkspaceRelativePath::parse("source.md").unwrap();
        let target = WorkspaceRelativePath::parse("target.md").unwrap();
        let first = KernelDocumentHistoryAdapter::new(fixture.path()).unwrap();
        first
            .preserve(
                &source,
                b"contents",
                &Revision::parse("revision-a").unwrap(),
                &Rfc3339Utc::parse("2026-07-29T00:00:00Z").unwrap(),
            )
            .unwrap();
        first.fail_next_relocation_before_marker();
        assert!(first
            .relocate(
                &source,
                &target,
                qingyu_kernel::contract::DocumentKind::File
            )
            .is_err());
        assert_eq!(first.list(&source).unwrap().len(), 1);
        assert_eq!(first.list(&target).unwrap().len(), 1);
        drop(first);

        let second = KernelDocumentHistoryAdapter::new(fixture.path()).unwrap();
        second
            .relocate(
                &source,
                &target,
                qingyu_kernel::contract::DocumentKind::File,
            )
            .unwrap();
        second
            .relocate(
                &source,
                &target,
                qingyu_kernel::contract::DocumentKind::File,
            )
            .unwrap();
        assert!(second.list(&source).unwrap().is_empty());
        assert_eq!(second.list(&target).unwrap().len(), 1);
    }

    #[test]
    fn kernel_history_relocation_unions_nonconflicting_target_history() {
        let fixture = tempfile::tempdir().unwrap();
        let source = WorkspaceRelativePath::parse("source.md").unwrap();
        let target = WorkspaceRelativePath::parse("target.md").unwrap();
        let adapter = KernelDocumentHistoryAdapter::new(fixture.path()).unwrap();
        let target_snapshot = adapter
            .preserve(
                &target,
                b"older target contents",
                &Revision::parse("target-revision").unwrap(),
                &Rfc3339Utc::parse("2026-07-28T00:00:00Z").unwrap(),
            )
            .unwrap();
        let source_snapshot = adapter
            .preserve(
                &source,
                b"source contents",
                &Revision::parse("source-revision").unwrap(),
                &Rfc3339Utc::parse("2026-07-29T00:00:00Z").unwrap(),
            )
            .unwrap();

        adapter
            .relocate(
                &source,
                &target,
                qingyu_kernel::contract::DocumentKind::File,
            )
            .unwrap();

        assert!(adapter.list(&source).unwrap().is_empty());
        let history = adapter.list(&target).unwrap();
        assert_eq!(history.len(), 2);
        assert!(history
            .iter()
            .any(|item| item.snapshot_id == target_snapshot));
        assert!(history
            .iter()
            .any(|item| item.snapshot_id == source_snapshot));
    }

    #[cfg(unix)]
    #[test]
    fn kernel_history_adapter_rejects_a_symlinked_hash_bucket() {
        use std::os::unix::fs::symlink;

        let fixture = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let path = WorkspaceRelativePath::parse("note.md").unwrap();
        let adapter = KernelDocumentHistoryAdapter::new(fixture.path()).unwrap();
        symlink(
            outside.path(),
            fixture
                .path()
                .join(KERNEL_HISTORY_DIRECTORY)
                .join(KernelDocumentHistoryAdapter::bucket_name(&path)),
        )
        .unwrap();

        assert!(adapter
            .preserve(
                &path,
                b"contents",
                &Revision::parse("revision").unwrap(),
                &Rfc3339Utc::parse("2026-07-29T00:00:00Z").unwrap(),
            )
            .is_err());
        assert!(std::fs::read_dir(outside.path()).unwrap().next().is_none());
    }

    #[test]
    fn kernel_history_adapter_cleans_a_failed_staged_snapshot() {
        let fixture = tempfile::tempdir().unwrap();
        let path = WorkspaceRelativePath::parse("note.md").unwrap();
        let adapter = KernelDocumentHistoryAdapter::new(fixture.path()).unwrap();
        adapter.fail_next_preserve_after_stage();
        assert!(adapter
            .preserve(
                &path,
                b"contents",
                &Revision::parse("revision").unwrap(),
                &Rfc3339Utc::parse("2026-07-29T00:00:00Z").unwrap(),
            )
            .is_err());
        assert!(adapter.list(&path).unwrap().is_empty());
        let bucket = fixture
            .path()
            .join(KERNEL_HISTORY_DIRECTORY)
            .join(KernelDocumentHistoryAdapter::bucket_name(&path));
        assert!(std::fs::read_dir(bucket).unwrap().next().is_none());
    }

    #[test]
    fn kernel_history_adapter_retains_the_newest_thirty_snapshots() {
        let fixture = tempfile::tempdir().unwrap();
        let path = WorkspaceRelativePath::parse("note.md").unwrap();
        let adapter = KernelDocumentHistoryAdapter::new(fixture.path()).unwrap();
        let mut snapshot_ids = Vec::new();
        for second in 0..31 {
            snapshot_ids.push(
                adapter
                    .preserve(
                        &path,
                        format!("contents-{second}").as_bytes(),
                        &Revision::parse(format!("revision-{second}")).unwrap(),
                        &Rfc3339Utc::parse(format!("2026-07-29T00:00:{second:02}Z")).unwrap(),
                    )
                    .unwrap(),
            );
        }
        let snapshots = adapter.list(&path).unwrap();
        assert_eq!(snapshots.len(), MARKDOWN_HISTORY_RETENTION_LIMIT);
        assert!(adapter.get(&path, snapshot_ids[0]).unwrap().is_none());
        assert_eq!(snapshots.first().unwrap().contents, b"contents-1".to_vec());
        assert_eq!(snapshots.last().unwrap().contents, b"contents-30".to_vec());
    }

    #[test]
    fn snapshots_existing_markdown_before_overwriting() {
        let root = std::env::temp_dir().join(format!(
            "markra-history-write-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after epoch")
                .as_nanos()
        ));
        let history_root = root.join("history");
        let note = root.join("Synthetic.md");
        fs::create_dir_all(&root).expect("test folder should be created");
        fs::write(&note, "# Initial\n\nSynthetic body").expect("markdown file should be created");

        write_markdown_file_with_history_root(
            &history_root,
            note.to_string_lossy().to_string(),
            "# Updated\n\nSynthetic body".to_string(),
        )
        .expect("markdown file should be written");

        assert_eq!(
            fs::read_to_string(&note).expect("markdown file should be readable"),
            "# Updated\n\nSynthetic body"
        );

        let entries =
            list_markdown_file_history_with_root(&history_root, note.to_string_lossy().to_string())
                .expect("history should list");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].size_bytes, 25);

        let history_file = read_markdown_file_history_with_root(
            &history_root,
            note.to_string_lossy().to_string(),
            entries[0].id.clone(),
        )
        .expect("history file should read");
        assert_eq!(history_file.contents, "# Initial\n\nSynthetic body");

        fs::remove_dir_all(root).expect("test folder should be removed");
    }

    #[test]
    fn skips_history_snapshot_when_saved_contents_match_disk() {
        let root = std::env::temp_dir().join(format!(
            "markra-history-unchanged-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after epoch")
                .as_nanos()
        ));
        let history_root = root.join("history");
        let note = root.join("Synthetic.md");
        fs::create_dir_all(&root).expect("test folder should be created");
        fs::write(&note, "# Same\n\nSynthetic body").expect("markdown file should be created");

        write_markdown_file_with_history_root(
            &history_root,
            note.to_string_lossy().to_string(),
            "# Same\n\nSynthetic body".to_string(),
        )
        .expect("markdown file should be written");

        let entries =
            list_markdown_file_history_with_root(&history_root, note.to_string_lossy().to_string())
                .expect("history should list");
        assert!(entries.is_empty());

        fs::remove_dir_all(root).expect("test folder should be removed");
    }

    #[test]
    fn skips_history_snapshot_when_requested() {
        let root = std::env::temp_dir().join(format!(
            "markra-history-skip-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after epoch")
                .as_nanos()
        ));
        let history_root = root.join("history");
        let note = root.join("Synthetic.md");
        fs::create_dir_all(&root).expect("test folder should be created");
        fs::write(&note, "# Current\n\nSynthetic body").expect("markdown file should be created");

        write_markdown_file_with_optional_history_root(
            Some(&history_root),
            note.to_string_lossy().to_string(),
            "# Earlier\n\nSynthetic body".to_string(),
            true,
            None,
        )
        .expect("markdown file should be written");

        assert_eq!(
            fs::read_to_string(&note).expect("markdown file should be readable"),
            "# Earlier\n\nSynthetic body"
        );

        let entries =
            list_markdown_file_history_with_root(&history_root, note.to_string_lossy().to_string())
                .expect("history should list");
        assert!(entries.is_empty());

        fs::remove_dir_all(root).expect("test folder should be removed");
    }

    #[test]
    fn truncates_future_history_after_saving_from_a_restored_state() {
        let root = std::env::temp_dir().join(format!(
            "markra-history-linear-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after epoch")
                .as_nanos()
        ));
        let history_root = root.join("history");
        let note = root.join("Synthetic.md");
        fs::create_dir_all(&root).expect("test folder should be created");
        fs::write(&note, "# State A\n\nSynthetic body").expect("markdown file should be created");

        write_markdown_file_with_history_root(
            &history_root,
            note.to_string_lossy().to_string(),
            "# State B\n\nSynthetic body".to_string(),
        )
        .expect("state B should be written");
        write_markdown_file_with_history_root(
            &history_root,
            note.to_string_lossy().to_string(),
            "# State C\n\nSynthetic body".to_string(),
        )
        .expect("state C should be written");
        write_markdown_file_with_history_root(
            &history_root,
            note.to_string_lossy().to_string(),
            "# State D\n\nSynthetic body".to_string(),
        )
        .expect("state D should be written");

        let initial_entries =
            list_markdown_file_history_with_root(&history_root, note.to_string_lossy().to_string())
                .expect("history should list");
        assert_eq!(initial_entries.len(), 3);
        let restored_entry = initial_entries
            .iter()
            .find(|entry| {
                read_markdown_file_history_with_root(
                    &history_root,
                    note.to_string_lossy().to_string(),
                    entry.id.clone(),
                )
                .expect("history entry should read")
                .contents
                    == "# State B\n\nSynthetic body"
            })
            .expect("state B history should exist");

        write_markdown_file_with_optional_history_root(
            Some(&history_root),
            note.to_string_lossy().to_string(),
            "# State B\n\nSynthetic body".to_string(),
            true,
            Some(restored_entry.id.clone()),
        )
        .expect("restored state should be written");

        let restored_entries =
            list_markdown_file_history_with_root(&history_root, note.to_string_lossy().to_string())
                .expect("history should list");
        assert_eq!(restored_entries.len(), 3);

        write_markdown_file_with_history_root(
            &history_root,
            note.to_string_lossy().to_string(),
            "# State E\n\nSynthetic body".to_string(),
        )
        .expect("state E should be written");

        let entries =
            list_markdown_file_history_with_root(&history_root, note.to_string_lossy().to_string())
                .expect("history should list");
        let entry_contents = entries
            .iter()
            .map(|entry| {
                read_markdown_file_history_with_root(
                    &history_root,
                    note.to_string_lossy().to_string(),
                    entry.id.clone(),
                )
                .expect("history entry should read")
                .contents
            })
            .collect::<Vec<_>>();
        assert_eq!(
            entry_contents,
            vec![
                "# State B\n\nSynthetic body".to_string(),
                "# State A\n\nSynthetic body".to_string(),
            ]
        );
        assert_eq!(
            fs::read_to_string(&note).expect("markdown file should be readable"),
            "# State E\n\nSynthetic body"
        );

        fs::remove_dir_all(root).expect("test folder should be removed");
    }

    #[test]
    fn writes_markdown_when_history_root_is_unavailable() {
        let root = std::env::temp_dir().join(format!(
            "markra-history-unavailable-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after epoch")
                .as_nanos()
        ));
        let note = root.join("Synthetic.md");
        fs::create_dir_all(&root).expect("test folder should be created");
        fs::write(&note, "# Initial\n\nSynthetic body").expect("markdown file should be created");

        write_markdown_file_with_optional_history_root(
            None,
            note.to_string_lossy().to_string(),
            "# Updated\n\nSynthetic body".to_string(),
            false,
            None,
        )
        .expect("markdown file should be written");

        assert_eq!(
            fs::read_to_string(&note).expect("markdown file should be readable"),
            "# Updated\n\nSynthetic body"
        );

        fs::remove_dir_all(root).expect("test folder should be removed");
    }
}
