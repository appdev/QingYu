//! Document history boundary.

use std::{
    collections::HashMap,
    io::{self, Read as _, Write as _},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
};

#[cfg(any(unix, windows))]
use cap_fs_ext::OpenOptionsExt;
use cap_fs_ext::{FollowSymlinks, MetadataExt, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::contract::{DocumentKind, Revision, Rfc3339Utc, SnapshotId, WorkspaceRelativePath};

use super::types::HistorySnapshot;

pub trait DocumentHistoryStore: Send + Sync {
    fn preserve(
        &self,
        path: &WorkspaceRelativePath,
        contents: &[u8],
        revision: &Revision,
        created_at: &Rfc3339Utc,
    ) -> Result<SnapshotId, DocumentHistoryError>;
    fn list(
        &self,
        path: &WorkspaceRelativePath,
    ) -> Result<Vec<HistorySnapshot>, DocumentHistoryError>;
    fn get(
        &self,
        path: &WorkspaceRelativePath,
        snapshot_id: SnapshotId,
    ) -> Result<Option<HistorySnapshot>, DocumentHistoryError>;
    fn relocate(
        &self,
        source: &WorkspaceRelativePath,
        target: &WorkspaceRelativePath,
        kind: DocumentKind,
    ) -> Result<(), DocumentHistoryError>;
}

#[derive(Default)]
pub struct MemoryDocumentHistoryStore {
    snapshots: Mutex<HashMap<String, Vec<HistorySnapshot>>>,
}

impl DocumentHistoryStore for MemoryDocumentHistoryStore {
    fn preserve(
        &self,
        path: &WorkspaceRelativePath,
        contents: &[u8],
        revision: &Revision,
        created_at: &Rfc3339Utc,
    ) -> Result<SnapshotId, DocumentHistoryError> {
        let snapshot_id = SnapshotId::new(Uuid::new_v4());
        let snapshot = HistorySnapshot {
            snapshot_id,
            document_path: path.clone(),
            created_at: created_at.clone(),
            contents: contents.to_vec(),
            revision: revision.clone(),
        };
        self.snapshots
            .lock()
            .map_err(|_| DocumentHistoryError)?
            .entry(path.as_str().to_string())
            .or_default()
            .push(snapshot);
        Ok(snapshot_id)
    }

    fn list(
        &self,
        path: &WorkspaceRelativePath,
    ) -> Result<Vec<HistorySnapshot>, DocumentHistoryError> {
        Ok(self
            .snapshots
            .lock()
            .map_err(|_| DocumentHistoryError)?
            .get(path.as_str())
            .cloned()
            .unwrap_or_default())
    }

    fn get(
        &self,
        path: &WorkspaceRelativePath,
        snapshot_id: SnapshotId,
    ) -> Result<Option<HistorySnapshot>, DocumentHistoryError> {
        Ok(self
            .list(path)?
            .into_iter()
            .find(|snapshot| snapshot.snapshot_id == snapshot_id))
    }

    fn relocate(
        &self,
        source: &WorkspaceRelativePath,
        target: &WorkspaceRelativePath,
        kind: DocumentKind,
    ) -> Result<(), DocumentHistoryError> {
        let mut snapshots = self.snapshots.lock().map_err(|_| DocumentHistoryError)?;
        let relocations = snapshots
            .keys()
            .filter_map(|candidate| {
                relocated_history_path(source, target, candidate, kind)
                    .map(|relocated| relocated.map(|target| (candidate.clone(), target)))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut relocated = snapshots.clone();
        let mut moves = Vec::with_capacity(relocations.len());
        for (source_key, target_key) in &relocations {
            if source_key == target_key {
                continue;
            }
            let Some(mut moved) = snapshots.get(source_key).cloned() else {
                continue;
            };
            let target_path =
                WorkspaceRelativePath::parse(target_key).map_err(|_| DocumentHistoryError)?;
            for snapshot in &mut moved {
                snapshot.document_path = target_path.clone();
            }
            moves.push((source_key.clone(), target_key.clone(), moved));
        }
        for (source_key, _, _) in &moves {
            relocated.remove(source_key);
        }
        for (_, target_key, moved) in moves {
            let target_history = relocated.entry(target_key).or_default();
            for snapshot in moved {
                if let Some(existing) = target_history
                    .iter()
                    .find(|existing| existing.snapshot_id == snapshot.snapshot_id)
                {
                    if existing != &snapshot {
                        return Err(DocumentHistoryError);
                    }
                } else {
                    target_history.push(snapshot);
                }
            }
        }
        *snapshots = relocated;
        Ok(())
    }
}

fn relocated_history_path(
    source: &WorkspaceRelativePath,
    target: &WorkspaceRelativePath,
    candidate: &str,
    kind: DocumentKind,
) -> Option<Result<String, DocumentHistoryError>> {
    if candidate == source.as_str() {
        return Some(Ok(target.as_str().to_string()));
    }
    if kind != DocumentKind::Directory {
        return None;
    }
    let suffix = candidate.strip_prefix(source.as_str())?.strip_prefix('/')?;
    let relocated = if target.as_str().is_empty() {
        suffix.to_string()
    } else {
        format!("{}/{suffix}", target.as_str())
    };
    Some(
        WorkspaceRelativePath::parse(&relocated)
            .map(|path| path.as_str().to_string())
            .map_err(|_| DocumentHistoryError),
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DocumentHistoryError;

impl std::fmt::Display for DocumentHistoryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("document history is unavailable")
    }
}

impl std::error::Error for DocumentHistoryError {}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DocumentRecoveryIntent {
    pub transaction_id: Uuid,
    pub source: Option<WorkspaceRelativePath>,
    pub target: WorkspaceRelativePath,
    pub stage_name: Option<String>,
    #[serde(default = "default_recovery_document_kind")]
    pub kind: DocumentKind,
    pub previous_revision: Option<Revision>,
    pub intended_revision: Revision,
}

fn default_recovery_document_kind() -> DocumentKind {
    DocumentKind::File
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DocumentRecoveryOutcome {
    Finalized,
    RolledBack,
}

pub trait DocumentRecoveryStore: Send + Sync {
    fn prepare(&self, intent: &DocumentRecoveryIntent) -> Result<(), DocumentRecoveryError>;
    fn pending(&self) -> Result<Vec<DocumentRecoveryIntent>, DocumentRecoveryError>;
    fn complete(&self, transaction_id: Uuid) -> Result<(), DocumentRecoveryError>;
    fn clear(&self, transaction_id: Uuid) -> Result<(), DocumentRecoveryError>;
}

#[derive(Default)]
pub struct MemoryDocumentRecoveryStore {
    intents: Mutex<Vec<DocumentRecoveryIntent>>,
    fail_next_completion: AtomicBool,
}

impl MemoryDocumentRecoveryStore {
    pub fn fail_next_completion(&self) {
        self.fail_next_completion.store(true, Ordering::SeqCst);
    }

    pub fn intent_count(&self) -> usize {
        self.intents.lock().map_or(0, |intents| intents.len())
    }
}

impl DocumentRecoveryStore for MemoryDocumentRecoveryStore {
    fn prepare(&self, intent: &DocumentRecoveryIntent) -> Result<(), DocumentRecoveryError> {
        let mut intents = self.intents.lock().map_err(|_| DocumentRecoveryError)?;
        if intents
            .iter()
            .any(|candidate| candidate.transaction_id == intent.transaction_id)
        {
            return Err(DocumentRecoveryError);
        }
        intents.push(intent.clone());
        Ok(())
    }

    fn pending(&self) -> Result<Vec<DocumentRecoveryIntent>, DocumentRecoveryError> {
        self.intents
            .lock()
            .map(|intents| intents.clone())
            .map_err(|_| DocumentRecoveryError)
    }

    fn complete(&self, transaction_id: Uuid) -> Result<(), DocumentRecoveryError> {
        if self.fail_next_completion.swap(false, Ordering::SeqCst) {
            return Err(DocumentRecoveryError);
        }
        self.clear(transaction_id)
    }

    fn clear(&self, transaction_id: Uuid) -> Result<(), DocumentRecoveryError> {
        self.intents
            .lock()
            .map_err(|_| DocumentRecoveryError)?
            .retain(|intent| intent.transaction_id != transaction_id);
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DocumentRecoveryError;

impl std::fmt::Display for DocumentRecoveryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("document mutation recovery is unavailable")
    }
}

impl std::error::Error for DocumentRecoveryError {}

impl DocumentRecoveryIntent {
    pub(crate) fn validate(&self) -> Result<(), DocumentRecoveryError> {
        match (&self.source, &self.stage_name) {
            (Some(_), None) => Ok(()),
            (None, Some(stage_name)) if valid_document_stage_name(stage_name) => Ok(()),
            _ => Err(DocumentRecoveryError),
        }
    }
}

fn valid_document_stage_name(name: &str) -> bool {
    let Some(entropy) = name
        .strip_prefix(super::DOCUMENT_STAGE_PREFIX)
        .and_then(|name| name.strip_suffix(".tmp"))
    else {
        return false;
    };
    entropy.len() == 32
        && entropy
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

const RECOVERY_JOURNAL_PREFIX: &str = "document-recovery-v1-";
const RECOVERY_JOURNAL_STAGE_PREFIX: &str = ".document-recovery-v1-";
const MAX_RECOVERY_JOURNAL_BYTES: u64 = 1024 * 1024;

/// Capability-addressed persistent journal implementation. Composition owns
/// the directory capability; neither the Kernel API nor document IDs can
/// observe its absolute address.
pub struct FileDocumentRecoveryStore {
    directory: Dir,
    transaction: Mutex<()>,
}

impl FileDocumentRecoveryStore {
    pub fn new(directory: Dir) -> Self {
        Self {
            directory,
            transaction: Mutex::new(()),
        }
    }

    fn read_intent(&self, name: &str) -> Result<DocumentRecoveryIntent, DocumentRecoveryError> {
        let intent = self.read_intent_contents(name)?;
        if name != Self::journal_name(intent.transaction_id) {
            return Err(DocumentRecoveryError);
        }
        Ok(intent)
    }

    fn read_intent_contents(
        &self,
        name: &str,
    ) -> Result<DocumentRecoveryIntent, DocumentRecoveryError> {
        let metadata = self
            .directory
            .symlink_metadata(name)
            .map_err(|_| DocumentRecoveryError)?;
        if !trusted_recovery_file(&metadata) || metadata.len() > MAX_RECOVERY_JOURNAL_BYTES {
            return Err(DocumentRecoveryError);
        }
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        let mut file = self
            .directory
            .open_with(name, &options)
            .map_err(|_| DocumentRecoveryError)?;
        let before = file.metadata().map_err(|_| DocumentRecoveryError)?;
        if !trusted_recovery_file(&before)
            || !same_recovery_file(&metadata, &before)
            || before.len() > MAX_RECOVERY_JOURNAL_BYTES
        {
            return Err(DocumentRecoveryError);
        }
        let mut bytes = Vec::with_capacity(before.len() as usize);
        (&mut file)
            .take(MAX_RECOVERY_JOURNAL_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| DocumentRecoveryError)?;
        let after = file.metadata().map_err(|_| DocumentRecoveryError)?;
        let latest = self
            .directory
            .symlink_metadata(name)
            .map_err(|_| DocumentRecoveryError)?;
        if bytes.len() as u64 > MAX_RECOVERY_JOURNAL_BYTES
            || !trusted_recovery_file(&after)
            || !trusted_recovery_file(&latest)
            || !same_recovery_file(&before, &after)
            || !same_recovery_file(&after, &latest)
            || before.len() != after.len()
            || after.len() != bytes.len() as u64
            || before.modified().ok() != after.modified().ok()
        {
            return Err(DocumentRecoveryError);
        }
        let intent: DocumentRecoveryIntent =
            serde_json::from_slice(&bytes).map_err(|_| DocumentRecoveryError)?;
        intent.validate()?;
        Ok(intent)
    }

    fn read_staged_intent(
        &self,
        name: &str,
    ) -> Result<DocumentRecoveryIntent, DocumentRecoveryError> {
        let transaction = name
            .strip_prefix(RECOVERY_JOURNAL_STAGE_PREFIX)
            .and_then(|value| value.strip_suffix(".tmp"))
            .and_then(|value| Uuid::parse_str(value).ok())
            .ok_or(DocumentRecoveryError)?;
        let intent = self.read_intent_contents(name)?;
        (intent.transaction_id == transaction)
            .then_some(intent)
            .ok_or(DocumentRecoveryError)
    }

    fn journal_name(transaction_id: Uuid) -> String {
        format!("{RECOVERY_JOURNAL_PREFIX}{transaction_id}.json")
    }

    fn write_locked(&self, intent: &DocumentRecoveryIntent) -> Result<(), DocumentRecoveryError> {
        intent.validate()?;
        let journal_name = Self::journal_name(intent.transaction_id);
        match self.directory.symlink_metadata(&journal_name) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                return (self.read_intent(&journal_name)? == *intent)
                    .then_some(())
                    .ok_or(DocumentRecoveryError);
            }
            Ok(_) => return Err(DocumentRecoveryError),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return Err(DocumentRecoveryError),
        }
        let bytes = serde_json::to_vec(intent).map_err(|_| DocumentRecoveryError)?;
        let stage_name = format!(
            "{RECOVERY_JOURNAL_STAGE_PREFIX}{}.tmp",
            intent.transaction_id
        );
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = self
            .directory
            .open_with(&stage_name, &options)
            .map_err(|_| DocumentRecoveryError)?;
        if file
            .write_all(&bytes)
            .and_then(|()| file.sync_all())
            .is_err()
        {
            drop(file);
            let _cleanup_result = self.directory.remove_file(&stage_name);
            return Err(DocumentRecoveryError);
        }
        drop(file);
        if rename_recovery_noreplace(&self.directory, &stage_name, &journal_name).is_err() {
            let _cleanup_result = self.directory.remove_file(&stage_name);
            return Err(DocumentRecoveryError);
        }
        sync_recovery_directory(&self.directory)
    }
}

#[cfg(unix)]
fn rename_recovery_noreplace(
    directory: &Dir,
    source: &str,
    target: &str,
) -> Result<(), DocumentRecoveryError> {
    rustix::fs::renameat_with(
        directory,
        source,
        directory,
        target,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(|_| DocumentRecoveryError)
}

#[cfg(windows)]
fn rename_recovery_noreplace(
    directory: &Dir,
    source: &str,
    target: &str,
) -> Result<(), DocumentRecoveryError> {
    use std::os::windows::io::AsRawHandle;

    use windows_sys::Win32::Storage::FileSystem::{
        FileRenameInfo, SetFileInformationByHandle, DELETE, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_RENAME_INFO,
    };

    let target = target.encode_utf16().collect::<Vec<_>>();
    if target.is_empty() || target.contains(&0) {
        return Err(DocumentRecoveryError);
    }
    let target_bytes = target
        .len()
        .checked_mul(std::mem::size_of::<u16>())
        .and_then(|length| u32::try_from(length).ok())
        .ok_or(DocumentRecoveryError)?;
    let mut options = OpenOptions::new();
    options
        .access_mode(DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .follow(FollowSymlinks::No);
    let source = directory
        .open_with(source, &options)
        .map_err(|_| DocumentRecoveryError)?;
    let offset = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
    let buffer_bytes = std::mem::size_of::<FILE_RENAME_INFO>()
        .checked_add(target_bytes as usize)
        .ok_or(DocumentRecoveryError)?;
    let mut buffer = vec![0usize; buffer_bytes.div_ceil(std::mem::size_of::<usize>())];
    let rename_info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    let renamed = unsafe {
        (*rename_info).Anonymous.ReplaceIfExists = false;
        (*rename_info).RootDirectory = directory.as_raw_handle();
        (*rename_info).FileNameLength = target_bytes;
        std::ptr::copy_nonoverlapping(
            target.as_ptr(),
            buffer.as_mut_ptr().cast::<u8>().add(offset).cast::<u16>(),
            target.len(),
        );
        SetFileInformationByHandle(
            source.as_raw_handle(),
            FileRenameInfo,
            rename_info.cast(),
            u32::try_from(buffer_bytes).map_err(|_| DocumentRecoveryError)?,
        )
    };
    (renamed != 0).then_some(()).ok_or(DocumentRecoveryError)
}

#[cfg(not(any(unix, windows)))]
fn rename_recovery_noreplace(
    _directory: &Dir,
    _source: &str,
    _target: &str,
) -> Result<(), DocumentRecoveryError> {
    Err(DocumentRecoveryError)
}

impl DocumentRecoveryStore for FileDocumentRecoveryStore {
    fn prepare(&self, intent: &DocumentRecoveryIntent) -> Result<(), DocumentRecoveryError> {
        let _transaction = self.transaction.lock().map_err(|_| DocumentRecoveryError)?;
        self.write_locked(intent)
    }

    fn pending(&self) -> Result<Vec<DocumentRecoveryIntent>, DocumentRecoveryError> {
        let _transaction = self.transaction.lock().map_err(|_| DocumentRecoveryError)?;
        let mut intents: Vec<DocumentRecoveryIntent> = Vec::new();
        let entries = self
            .directory
            .entries()
            .map_err(|_| DocumentRecoveryError)?;
        for entry in entries {
            let entry = entry.map_err(|_| DocumentRecoveryError)?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if name.starts_with(RECOVERY_JOURNAL_STAGE_PREFIX) && name.ends_with(".tmp") {
                let intent = self.read_staged_intent(&name)?;
                let journal_name = Self::journal_name(intent.transaction_id);
                match self.directory.symlink_metadata(&journal_name) {
                    Ok(_) => {
                        if self.read_intent(&journal_name)? != intent {
                            return Err(DocumentRecoveryError);
                        }
                        self.directory
                            .remove_file(&name)
                            .map_err(|_| DocumentRecoveryError)?;
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        rename_recovery_noreplace(&self.directory, &name, &journal_name)?;
                    }
                    Err(_) => return Err(DocumentRecoveryError),
                }
                if !intents
                    .iter()
                    .any(|candidate| candidate.transaction_id == intent.transaction_id)
                {
                    intents.push(intent);
                }
            } else if name.starts_with(RECOVERY_JOURNAL_PREFIX) && name.ends_with(".json") {
                let intent = self.read_intent(&name)?;
                if !intents
                    .iter()
                    .any(|candidate| candidate.transaction_id == intent.transaction_id)
                {
                    intents.push(intent);
                }
            }
        }
        intents.sort_by_key(|intent| intent.transaction_id);
        sync_recovery_directory(&self.directory)?;
        Ok(intents)
    }

    fn complete(&self, transaction_id: Uuid) -> Result<(), DocumentRecoveryError> {
        self.clear(transaction_id)
    }

    fn clear(&self, transaction_id: Uuid) -> Result<(), DocumentRecoveryError> {
        let _transaction = self.transaction.lock().map_err(|_| DocumentRecoveryError)?;
        let name = Self::journal_name(transaction_id);
        match self.directory.remove_file(&name) {
            Ok(()) => sync_recovery_directory(&self.directory),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(DocumentRecoveryError),
        }
    }
}

fn trusted_recovery_file(metadata: &cap_std::fs::Metadata) -> bool {
    metadata.is_file() && !metadata.file_type().is_symlink() && recovery_link_count(metadata) == 1
}

fn same_recovery_file(left: &cap_std::fs::Metadata, right: &cap_std::fs::Metadata) -> bool {
    MetadataExt::dev(left) == MetadataExt::dev(right)
        && MetadataExt::ino(left) == MetadataExt::ino(right)
}

#[cfg(unix)]
fn recovery_link_count(metadata: &cap_std::fs::Metadata) -> u64 {
    MetadataExt::nlink(metadata)
}

#[cfg(windows)]
fn recovery_link_count(metadata: &cap_std::fs::Metadata) -> u64 {
    use cap_std::fs::MetadataExt as _;
    metadata.number_of_links().unwrap_or(0)
}

#[cfg(not(any(unix, windows)))]
fn recovery_link_count(_metadata: &cap_std::fs::Metadata) -> u64 {
    1
}

#[cfg(unix)]
fn sync_recovery_directory(directory: &Dir) -> Result<(), DocumentRecoveryError> {
    rustix::fs::fsync(directory).map_err(|_| DocumentRecoveryError)
}

#[cfg(not(unix))]
fn sync_recovery_directory(_directory: &Dir) -> Result<(), DocumentRecoveryError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_relocation_unions_nonconflicting_target_history() {
        let store = MemoryDocumentHistoryStore::default();
        let source = WorkspaceRelativePath::parse("source.md").unwrap();
        let target = WorkspaceRelativePath::parse("target.md").unwrap();
        let created_at = Rfc3339Utc::parse("2026-07-29T00:00:00Z").unwrap();
        let source_id = store
            .preserve(
                &source,
                b"source",
                &Revision::parse("source-revision").unwrap(),
                &created_at,
            )
            .unwrap();
        let target_id = store
            .preserve(
                &target,
                b"target",
                &Revision::parse("target-revision").unwrap(),
                &created_at,
            )
            .unwrap();

        store
            .relocate(&source, &target, DocumentKind::File)
            .unwrap();
        store
            .relocate(&source, &target, DocumentKind::File)
            .unwrap();

        assert!(store.list(&source).unwrap().is_empty());
        let relocated = store.list(&target).unwrap();
        assert_eq!(relocated.len(), 2);
        assert!(relocated
            .iter()
            .any(|snapshot| snapshot.snapshot_id == source_id));
        assert!(relocated
            .iter()
            .any(|snapshot| snapshot.snapshot_id == target_id));
        assert!(relocated
            .iter()
            .all(|snapshot| snapshot.document_path == target));
    }
}
