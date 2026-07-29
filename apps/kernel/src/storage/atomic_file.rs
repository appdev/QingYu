use std::{
    fmt,
    io::{self, Read as _, Seek as _, SeekFrom, Write as _},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[cfg(any(unix, windows))]
use cap_fs_ext::OpenOptionsExt;
use cap_fs_ext::{FollowSymlinks, MetadataExt, OpenOptionsFollowExt};
use cap_std::fs::{Dir, File, OpenOptions};
use serde::{de::Error as _, Deserialize, Deserializer, Serialize};
use sha2::{Digest as _, Sha256};
use uuid::Uuid;
use zeroize::{Zeroize as _, Zeroizing};

use crate::{config::KernelLaunchEpoch, paths::InstanceDataRoot};

const MAX_INTERNAL_FILE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_INTENT_BYTES: u64 = 64 * 1024;
const TRANSACTION_PREFIX: &str = ".qingyu-storage-";

/// Capability-addressed storage for Kernel-owned files.
///
/// `DurableFileStore` serializes calls made through this instance. The host
/// must also hold the matching instance/workspace process locks for the whole
/// lifetime of the store. Revision checks protect cooperating Kernel writers;
/// they are not an operating-system compare-and-swap and cannot prevent an
/// uncooperative process from modifying a file between validation and rename.
pub struct DurableFileStore {
    directory: Dir,
    canonical_root: PathBuf,
    writer_epoch: Uuid,
    transaction_gate: Mutex<()>,
    faults: Arc<dyn FaultInjector>,
}

impl DurableFileStore {
    /// Opens Kernel-owned storage for one runtime launch.
    ///
    /// Every store rebuilt by the same `KernelRuntime` must receive that
    /// runtime's original launch epoch. A newly generated epoch is reserved
    /// for a newly launched Kernel process and is what authorizes recovery to
    /// finalize a prior launch's durability-uncertain publication.
    pub fn at_instance_data(
        root: &InstanceDataRoot,
        launch_epoch: &KernelLaunchEpoch,
    ) -> Result<Self, DurableFileFailure> {
        let directory = root
            .try_clone_dir()
            .map_err(|_| DurableFileFailure::unavailable())?;
        Ok(Self::new(
            directory,
            root.canonical_path().to_path_buf(),
            launch_epoch.value(),
            Arc::new(NoFaults),
        ))
    }

    #[cfg(test)]
    pub(crate) fn at_instance_data_with_test_fault(
        root: &InstanceDataRoot,
        launch_epoch: &KernelLaunchEpoch,
        fault: DurableFileTestFault,
    ) -> Result<Self, DurableFileFailure> {
        let directory = root
            .try_clone_dir()
            .map_err(|_| DurableFileFailure::unavailable())?;
        Ok(Self::new(
            directory,
            root.canonical_path().to_path_buf(),
            launch_epoch.value(),
            Arc::new(DurableFileTestFaultInjector {
                point: fault.point(),
                fired: std::sync::atomic::AtomicBool::new(false),
            }),
        ))
    }

    fn new(
        directory: Dir,
        canonical_root: PathBuf,
        writer_epoch: Uuid,
        faults: Arc<dyn FaultInjector>,
    ) -> Self {
        Self {
            directory,
            canonical_root,
            writer_epoch,
            transaction_gate: Mutex::new(()),
            faults,
        }
    }

    pub fn read(
        &self,
        target: &StorageFileName,
        max_bytes: u64,
    ) -> Result<Option<StoredFile>, DurableFileFailure> {
        self.read_named(target.as_str(), max_bytes)
    }

    pub fn replace(
        &self,
        request: ReplaceRequest<'_>,
    ) -> Result<ReplaceOutcome, DurableFileFailure> {
        let _transaction = self
            .transaction_gate
            .lock()
            .map_err(|_| DurableFileFailure::recovery_required(None))?;
        self.replace_locked(request)
    }

    pub fn recover(&self) -> Result<Vec<RecoveryOutcome>, DurableFileFailure> {
        let _transaction = self
            .transaction_gate
            .lock()
            .map_err(|_| DurableFileFailure::recovery_required(None))?;
        let intents = self.read_intents()?;
        let mut outcomes = Vec::with_capacity(intents.len());
        for (intent_name, intent) in intents {
            outcomes.push(self.recover_intent(&intent_name, intent)?);
        }
        outcomes.extend(self.recover_orphan_stages()?);
        Ok(outcomes)
    }

    fn replace_locked(
        &self,
        request: ReplaceRequest<'_>,
    ) -> Result<ReplaceOutcome, DurableFileFailure> {
        if u64::try_from(request.bytes.len()).unwrap_or(u64::MAX) > MAX_INTERNAL_FILE_BYTES {
            return Err(DurableFileFailure::too_large());
        }
        if self.has_pending_intent_for(request.target)? {
            return Err(DurableFileFailure::recovery_required(None));
        }

        let previous = self.read_named(request.target.as_str(), MAX_INTERNAL_FILE_BYTES)?;
        verify_expected(request.expected, previous.as_ref())?;
        let previous_guard = previous
            .as_ref()
            .map(|stored| self.open_revision_guard(request.target.as_str(), &stored.revision))
            .transpose()?;

        let transaction = RecoveryTransactionId(Uuid::new_v4());
        let stage_name = transaction.stage_name();
        let intent_name = transaction.intent_name();
        let transient_backup: Option<String> = None;

        let staged_file = match self.write_new_file(&stage_name, request.bytes) {
            Ok(file) => file,
            Err(error) => {
                let _ = self.remove_regular_if_present(&stage_name);
                return Err(error);
            }
        };
        let intended_revision = FileRevision::digest(request.bytes);
        if self
            .read_retained_named(&stage_name, &staged_file, MAX_INTERNAL_FILE_BYTES)
            .is_err()
        {
            drop(staged_file);
            let _ = self.remove_regular_if_present(&stage_name);
            return Err(DurableFileFailure::not_published(Some(transaction)));
        }

        let current = match self.read_named(request.target.as_str(), MAX_INTERNAL_FILE_BYTES) {
            Ok(current) => current,
            Err(error) => {
                drop(staged_file);
                let _ = self.remove_regular_if_present(&stage_name);
                return Err(error);
            }
        };
        if verify_expected(request.expected, current.as_ref()).is_err() {
            drop(staged_file);
            self.remove_regular_if_present(&stage_name)?;
            return Err(DurableFileFailure::revision_conflict());
        }
        if current.as_ref().map(|stored| &stored.revision)
            != previous.as_ref().map(|stored| &stored.revision)
        {
            drop(staged_file);
            self.remove_regular_if_present(&stage_name)?;
            return Err(DurableFileFailure::revision_conflict());
        }

        let preserved_as = match (request.preserve_previous, previous.as_ref()) {
            (PreservePrevious::Required { recovery_name }, Some(previous)) => {
                if let Err(error) = self.write_new_file(recovery_name.as_str(), &previous.bytes) {
                    drop(staged_file);
                    let _ = self.remove_regular_if_present(&stage_name);
                    return Err(error);
                }
                if self.sync_parent_directory().is_err() {
                    drop(staged_file);
                    self.remove_regular_if_present(&stage_name)?;
                    return Err(DurableFileFailure::not_published(Some(transaction)));
                }
                Some(recovery_name.clone())
            }
            _ => None,
        };

        let intent = RecoveryIntent {
            schema_version: 2,
            writer_epoch: self.writer_epoch,
            transaction,
            target: request.target.clone(),
            stage_name: stage_name.clone(),
            transient_backup: transient_backup.clone(),
            previous_revision: previous.as_ref().map(|stored| stored.revision.clone()),
            intended_revision: intended_revision.clone(),
        };
        if let Err(error) = self.write_intent(&intent_name, &intent) {
            drop(staged_file);
            let _ = self.cleanup_unpublished(&intent_name, &stage_name);
            return Err(error);
        }
        let intent_sync = if self.faults.fail_at(FaultPoint::IntentSyncFailure) {
            Err(DurableFileFailure::unavailable())
        } else {
            self.sync_parent_directory()
        };
        if intent_sync.is_err() {
            drop(staged_file);
            self.cleanup_unpublished(&intent_name, &stage_name)?;
            return Err(DurableFileFailure::not_published(Some(transaction)));
        }

        if self.faults.fail_at(FaultPoint::LeavePrepared) {
            return Err(DurableFileFailure::publish_uncertain(transaction));
        }
        if self.faults.fail_at(FaultPoint::BeforePublish) {
            drop(staged_file);
            self.cleanup_unpublished(&intent_name, &stage_name)?;
            return Err(DurableFileFailure::not_published(Some(transaction)));
        }

        self.faults.before_publish_validation(
            &self.canonical_root,
            &stage_name,
            request.target.as_str(),
        );
        let stage_before_publish = self
            .read_retained_named(&stage_name, &staged_file, MAX_INTERNAL_FILE_BYTES)
            .map_err(|_| DurableFileFailure::publish_uncertain(transaction))?;
        if stage_before_publish.revision != intended_revision {
            drop(staged_file);
            self.cleanup_unpublished(&intent_name, &stage_name)?;
            return Err(DurableFileFailure::not_published(Some(transaction)));
        }
        let target_before_publish = self
            .read_named(request.target.as_str(), MAX_INTERNAL_FILE_BYTES)
            .map_err(|_| DurableFileFailure::publish_uncertain(transaction))?;
        if self
            .verify_revision_guard(
                request.target.as_str(),
                previous_guard.as_ref(),
                previous.as_ref(),
            )
            .is_err()
        {
            drop(staged_file);
            self.cleanup_unpublished(&intent_name, &stage_name)?;
            return Err(DurableFileFailure::revision_conflict());
        }
        if !revisions_match(target_before_publish.as_ref(), previous.as_ref()) {
            drop(staged_file);
            self.cleanup_unpublished(&intent_name, &stage_name)?;
            return Err(DurableFileFailure::revision_conflict());
        }
        #[cfg(test)]
        self.faults.after_final_publish_validation(
            &self.canonical_root,
            &stage_name,
            request.target.as_str(),
        );
        let stage_at_publish = self
            .read_retained_named(&stage_name, &staged_file, MAX_INTERNAL_FILE_BYTES)
            .map_err(|_| DurableFileFailure::publish_uncertain(transaction))?;
        if stage_at_publish.revision != intended_revision {
            return Err(DurableFileFailure::publish_uncertain(transaction));
        }
        let target_at_publish = self
            .read_named(request.target.as_str(), MAX_INTERNAL_FILE_BYTES)
            .map_err(|_| DurableFileFailure::publish_uncertain(transaction))?;
        if self
            .verify_revision_guard(
                request.target.as_str(),
                previous_guard.as_ref(),
                previous.as_ref(),
            )
            .is_err()
        {
            drop(staged_file);
            self.cleanup_unpublished(&intent_name, &stage_name)?;
            return Err(DurableFileFailure::revision_conflict());
        }
        if !revisions_match(target_at_publish.as_ref(), previous.as_ref()) {
            drop(staged_file);
            self.cleanup_unpublished(&intent_name, &stage_name)?;
            return Err(DurableFileFailure::revision_conflict());
        }

        let mut publish_result = publish_atomic(
            &self.directory,
            &staged_file,
            &stage_name,
            request.target.as_str(),
            previous.is_some(),
        );
        if self.faults.fail_at(FaultPoint::AfterPublishReportsFailure) {
            publish_result = Err(io::Error::other("injected publish result"));
        }
        let target_after = self
            .read_named(request.target.as_str(), MAX_INTERNAL_FILE_BYTES)
            .map_err(|_| DurableFileFailure::publish_uncertain(transaction))?;
        let target_is_intended = target_after
            .as_ref()
            .is_some_and(|stored| stored.revision == intended_revision);
        if target_is_intended
            && self
                .read_retained_named(
                    request.target.as_str(),
                    &staged_file,
                    MAX_INTERNAL_FILE_BYTES,
                )
                .is_err()
        {
            return Err(DurableFileFailure::publish_uncertain(transaction));
        }
        if !target_is_intended {
            let stage_after = self
                .read_retained_named(&stage_name, &staged_file, MAX_INTERNAL_FILE_BYTES)
                .map_err(|_| DurableFileFailure::publish_uncertain(transaction))?;
            let target_is_previous = revisions_match(target_after.as_ref(), previous.as_ref());
            let stage_is_intended = stage_after.revision == intended_revision;
            if publish_result.is_err() && target_is_previous && stage_is_intended {
                drop(staged_file);
                self.cleanup_unpublished(&intent_name, &stage_name)?;
                return Err(DurableFileFailure::not_published(Some(transaction)));
            }
            return Err(DurableFileFailure::publish_uncertain(transaction));
        }

        let parent_sync = self.sync_commit_parent();
        let mut commit_state = match parent_sync {
            Ok(state) => state.into_commit_state(),
            Err(_) => CommitState::PublishedDurabilityUncertain,
        };

        if parent_sync.is_ok()
            && (self.faults.fail_at(FaultPoint::FinalizeFailure)
                || self.finalize_committed(&intent_name, &intent).is_err())
        {
            commit_state = CommitState::PublishedDurabilityUncertain;
        }

        Ok(ReplaceOutcome {
            installed_revision: intended_revision,
            commit_state,
            preserved_as,
        })
    }

    fn read_named(
        &self,
        name: &str,
        max_bytes: u64,
    ) -> Result<Option<StoredFile>, DurableFileFailure> {
        let addressed = match self.directory.symlink_metadata(name) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(DurableFileFailure::unavailable()),
        };
        let addressed = regular_file_identity(&addressed)?;
        if addressed.length > max_bytes {
            return Err(DurableFileFailure::too_large());
        }

        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        #[cfg(unix)]
        options.custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32);
        let mut file = self
            .directory
            .open_with(name, &options)
            .map_err(|_| DurableFileFailure::unsafe_entry())?;
        let retained = regular_file_identity(
            &file
                .metadata()
                .map_err(|_| DurableFileFailure::unavailable())?,
        )?;
        if retained != addressed {
            return Err(DurableFileFailure::unsafe_entry());
        }

        let mut bytes = Zeroizing::new(Vec::with_capacity(
            usize::try_from(addressed.length).unwrap_or(0),
        ));
        std::io::Read::by_ref(&mut file)
            .take(max_bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|_| DurableFileFailure::unavailable())?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > max_bytes {
            return Err(DurableFileFailure::too_large());
        }

        let retained_after = regular_file_identity(
            &file
                .metadata()
                .map_err(|_| DurableFileFailure::unavailable())?,
        )?;
        let addressed_after = regular_file_identity(
            &self
                .directory
                .symlink_metadata(name)
                .map_err(|_| DurableFileFailure::unsafe_entry())?,
        )?;
        if retained_after != retained || addressed_after != retained {
            return Err(DurableFileFailure::unsafe_entry());
        }

        let revision = FileRevision::digest(&bytes);
        Ok(Some(StoredFile {
            bytes: std::mem::take(&mut *bytes),
            revision,
        }))
    }

    fn write_new_file(&self, name: &str, bytes: &[u8]) -> Result<File, DurableFileFailure> {
        let mut options = OpenOptions::new();
        options
            .read(true)
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        #[cfg(unix)]
        options.mode(0o600);
        #[cfg(windows)]
        options
            .access_mode(
                windows_sys::Win32::Foundation::GENERIC_READ
                    | windows_sys::Win32::Foundation::GENERIC_WRITE
                    | windows_sys::Win32::Storage::FileSystem::DELETE,
            )
            .share_mode(windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ);
        let mut file = self
            .directory
            .open_with(name, &options)
            .map_err(|_| DurableFileFailure::not_published(None))?;

        #[cfg(test)]
        let injected_write_failure = self.faults.fail_new_file_write(name);
        #[cfg(not(test))]
        let injected_write_failure = false;
        let write_result = if injected_write_failure {
            file.write_all(&bytes[..bytes.len().min(1)])
                .and_then(|()| Err(io::Error::other("injected new-file write failure")))
        } else {
            file.write_all(bytes).and_then(|()| file.sync_all())
        };
        if write_result.is_err() {
            drop(file);
            self.cleanup_failed_new_file(name)?;
            return Err(DurableFileFailure::not_published(None));
        }
        let stored =
            self.read_retained_named(name, &file, u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        let verified = stored
            .as_ref()
            .is_ok_and(|stored| stored.revision == FileRevision::digest(bytes));
        if !verified {
            drop(file);
            self.cleanup_failed_new_file(name)?;
            return Err(DurableFileFailure::not_published(None));
        }
        Ok(file)
    }

    fn read_retained_named(
        &self,
        name: &str,
        retained: &File,
        max_bytes: u64,
    ) -> Result<StoredFile, DurableFileFailure> {
        let addressed = regular_file_identity(
            &self
                .directory
                .symlink_metadata(name)
                .map_err(|_| DurableFileFailure::unsafe_entry())?,
        )?;
        let before = regular_file_identity(
            &retained
                .metadata()
                .map_err(|_| DurableFileFailure::unavailable())?,
        )?;
        if addressed != before || before.length > max_bytes {
            return Err(DurableFileFailure::unsafe_entry());
        }
        let mut reader = retained
            .try_clone()
            .map_err(|_| DurableFileFailure::unavailable())?;
        reader
            .seek(SeekFrom::Start(0))
            .map_err(|_| DurableFileFailure::unavailable())?;
        let mut bytes = Zeroizing::new(Vec::with_capacity(
            usize::try_from(before.length).unwrap_or(0),
        ));
        std::io::Read::by_ref(&mut reader)
            .take(max_bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|_| DurableFileFailure::unavailable())?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > max_bytes {
            return Err(DurableFileFailure::too_large());
        }
        let retained_after = regular_file_identity(
            &retained
                .metadata()
                .map_err(|_| DurableFileFailure::unavailable())?,
        )?;
        let addressed_after = regular_file_identity(
            &self
                .directory
                .symlink_metadata(name)
                .map_err(|_| DurableFileFailure::unsafe_entry())?,
        )?;
        if retained_after != before
            || addressed_after != before
            || before.length != bytes.len() as u64
        {
            return Err(DurableFileFailure::unsafe_entry());
        }
        Ok(StoredFile {
            revision: FileRevision::digest(&bytes),
            bytes: std::mem::take(&mut *bytes),
        })
    }

    fn open_revision_guard(
        &self,
        name: &str,
        expected: &FileRevision,
    ) -> Result<File, DurableFileFailure> {
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        #[cfg(windows)]
        options.share_mode(windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ);
        let file = self
            .directory
            .open_with(name, &options)
            .map_err(|_| DurableFileFailure::revision_conflict())?;
        let stored = self
            .read_retained_named(name, &file, MAX_INTERNAL_FILE_BYTES)
            .map_err(|_| DurableFileFailure::revision_conflict())?;
        if &stored.revision != expected {
            return Err(DurableFileFailure::revision_conflict());
        }
        Ok(file)
    }

    fn verify_revision_guard(
        &self,
        name: &str,
        guard: Option<&File>,
        expected: Option<&StoredFile>,
    ) -> Result<(), DurableFileFailure> {
        match (guard, expected) {
            (Some(guard), Some(expected)) => {
                let stored = self
                    .read_retained_named(name, guard, MAX_INTERNAL_FILE_BYTES)
                    .map_err(|_| DurableFileFailure::revision_conflict())?;
                if stored.revision == expected.revision {
                    Ok(())
                } else {
                    Err(DurableFileFailure::revision_conflict())
                }
            }
            (None, None) => Ok(()),
            _ => Err(DurableFileFailure::revision_conflict()),
        }
    }

    #[cfg(windows)]
    fn open_recovery_source_guard(
        &self,
        name: &str,
        expected: &FileRevision,
    ) -> Result<File, DurableFileFailure> {
        let mut options = OpenOptions::new();
        options
            .read(true)
            .access_mode(
                windows_sys::Win32::Foundation::GENERIC_READ
                    | windows_sys::Win32::Storage::FileSystem::DELETE,
            )
            .share_mode(windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ)
            .follow(FollowSymlinks::No);
        let file = self
            .directory
            .open_with(name, &options)
            .map_err(|_| DurableFileFailure::recovery_required(None))?;
        let stored = self
            .read_retained_named(name, &file, MAX_INTERNAL_FILE_BYTES)
            .map_err(|_| DurableFileFailure::recovery_required(None))?;
        if &stored.revision != expected {
            return Err(DurableFileFailure::recovery_required(None));
        }
        Ok(file)
    }

    fn cleanup_failed_new_file(&self, name: &str) -> Result<(), DurableFileFailure> {
        self.remove_regular_if_present(name)?;
        self.sync_parent_directory().map(|_| ())
    }

    fn write_intent(&self, name: &str, intent: &RecoveryIntent) -> Result<(), DurableFileFailure> {
        let bytes =
            serde_json::to_vec(intent).map_err(|_| DurableFileFailure::not_published(None))?;
        self.write_new_file(name, &bytes).map(drop)
    }

    fn sync_parent_directory(&self) -> Result<ParentSyncState, DurableFileFailure> {
        sync_directory(&self.directory).map_err(|_| DurableFileFailure::unavailable())
    }

    fn sync_commit_parent(&self) -> Result<ParentSyncState, DurableFileFailure> {
        if self.faults.fail_at(FaultPoint::ParentSyncFailure) {
            return Err(DurableFileFailure::unavailable());
        }
        #[cfg(test)]
        if self.faults.fail_at(FaultPoint::ParentSyncUncertain) {
            return Ok(ParentSyncState::PlatformUncertain);
        }
        self.sync_parent_directory()
    }

    fn sync_recovery_parent(&self) -> Result<ParentSyncState, DurableFileFailure> {
        #[cfg(test)]
        if self.faults.fail_at(FaultPoint::RecoverySyncFailure) {
            return Err(DurableFileFailure::unavailable());
        }
        #[cfg(test)]
        if self.faults.fail_at(FaultPoint::RecoverySyncUncertain) {
            return Ok(ParentSyncState::PlatformUncertain);
        }
        self.sync_parent_directory()
    }

    fn cleanup_unpublished(
        &self,
        intent_name: &str,
        stage_name: &str,
    ) -> Result<(), DurableFileFailure> {
        self.remove_regular_if_present(stage_name)?;
        self.remove_regular_if_present(intent_name)?;
        self.sync_parent_directory().map(|_| ())
    }

    fn finalize_committed(
        &self,
        intent_name: &str,
        intent: &RecoveryIntent,
    ) -> Result<(), DurableFileFailure> {
        self.cleanup_committed_backup(intent)?;
        self.remove_regular_if_present(intent_name)?;
        self.sync_parent_directory().map(|_| ())
    }

    fn cleanup_committed_backup(&self, intent: &RecoveryIntent) -> Result<(), DurableFileFailure> {
        if let Some(backup) = intent.transient_backup.as_deref() {
            let expected = intent
                .previous_revision
                .as_ref()
                .ok_or_else(|| DurableFileFailure::recovery_required(Some(intent.transaction)))?;
            let stored = self.read_named(backup, MAX_INTERNAL_FILE_BYTES)?;
            if stored
                .as_ref()
                .is_some_and(|stored| stored.revision != *expected)
            {
                return Err(DurableFileFailure::recovery_required(Some(
                    intent.transaction,
                )));
            }
            self.remove_regular_if_present(backup)?;
        }
        Ok(())
    }

    fn remove_regular_if_present(&self, name: &str) -> Result<(), DurableFileFailure> {
        match self.directory.symlink_metadata(name) {
            Ok(metadata) => {
                regular_file_identity(&metadata)?;
                self.directory
                    .remove_file(name)
                    .map_err(|_| DurableFileFailure::recovery_required(None))?;
                Ok(())
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(DurableFileFailure::recovery_required(None)),
        }
    }

    fn read_intents(&self) -> Result<Vec<(String, RecoveryIntent)>, DurableFileFailure> {
        let mut intents = Vec::new();
        let entries = self
            .directory
            .entries()
            .map_err(|_| DurableFileFailure::recovery_required(None))?;
        for entry in entries {
            let entry = entry.map_err(|_| DurableFileFailure::recovery_required(None))?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if !name.starts_with(TRANSACTION_PREFIX) || !name.ends_with(".intent") {
                continue;
            }
            let stored = self
                .read_named(name, MAX_INTENT_BYTES)?
                .ok_or_else(|| DurableFileFailure::recovery_required(None))?;
            let intent: RecoveryIntent = serde_json::from_slice(&stored.bytes)
                .map_err(|_| DurableFileFailure::recovery_required(None))?;
            if intent.schema_version != 2
                || intent.intent_name() != name
                || intent.stage_name != intent.transaction.stage_name()
                || !transient_backup_is_valid(&intent)
            {
                return Err(DurableFileFailure::recovery_required(Some(
                    intent.transaction,
                )));
            }
            intents.push((name.to_string(), intent));
        }
        intents.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(intents)
    }

    fn has_pending_intent_for(&self, target: &StorageFileName) -> Result<bool, DurableFileFailure> {
        Ok(self
            .read_intents()?
            .iter()
            .any(|(_, intent)| &intent.target == target))
    }

    fn recover_orphan_stages(&self) -> Result<Vec<RecoveryOutcome>, DurableFileFailure> {
        let active_stages = self
            .read_intents()?
            .into_iter()
            .map(|(_, intent)| intent.stage_name)
            .collect::<std::collections::HashSet<_>>();
        let entries = self
            .directory
            .entries()
            .map_err(|_| DurableFileFailure::recovery_required(None))?;
        let mut orphans = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|_| DurableFileFailure::recovery_required(None))?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let Some(transaction) = transaction_from_artifact_name(name, ".stage") else {
                continue;
            };
            if active_stages.contains(name) {
                continue;
            }
            let metadata = self
                .directory
                .symlink_metadata(name)
                .map_err(|_| DurableFileFailure::recovery_required(Some(transaction)))?;
            if regular_file_identity(&metadata).is_err() {
                orphans.push(RecoveryOutcome::ManualInterventionRequired { transaction });
                continue;
            }
            self.remove_regular_if_present(name)?;
            orphans.push(RecoveryOutcome::DiscardedUnpublished { transaction });
        }
        if !orphans.is_empty() {
            let _ = self.sync_parent_directory()?;
        }
        Ok(orphans)
    }

    fn recover_intent(
        &self,
        intent_name: &str,
        intent: RecoveryIntent,
    ) -> Result<RecoveryOutcome, DurableFileFailure> {
        let target = self.read_named(intent.target.as_str(), MAX_INTERNAL_FILE_BYTES)?;
        let stage = self.read_named(&intent.stage_name, MAX_INTERNAL_FILE_BYTES)?;
        if target
            .as_ref()
            .is_some_and(|stored| stored.revision == intent.intended_revision)
        {
            if intent.writer_epoch == self.writer_epoch {
                return Ok(RecoveryOutcome::Committed {
                    revision: intent.intended_revision,
                    commit_state: CommitState::PublishedDurabilityUncertain,
                });
            }
            let parent_sync = self.sync_recovery_parent();
            let commit_state = parent_sync
                .map(ParentSyncState::into_commit_state)
                .unwrap_or(CommitState::PublishedDurabilityUncertain);
            // A returned uncertainty is still a successful observation of the
            // published target. Explicit recovery is the operation that may
            // consume that state. A real sync error must retain every artifact
            // so a later recovery attempt can make the decision again.
            if parent_sync.is_err() {
                return Ok(RecoveryOutcome::Committed {
                    revision: intent.intended_revision,
                    commit_state,
                });
            }
            if stage
                .as_ref()
                .is_some_and(|stored| stored.revision != intent.intended_revision)
            {
                return Ok(RecoveryOutcome::ManualInterventionRequired {
                    transaction: intent.transaction,
                });
            }
            if self.remove_regular_if_present(&intent.stage_name).is_err() {
                return Ok(RecoveryOutcome::ManualInterventionRequired {
                    transaction: intent.transaction,
                });
            }
            if self.cleanup_committed_backup(&intent).is_err() {
                return Ok(RecoveryOutcome::ManualInterventionRequired {
                    transaction: intent.transaction,
                });
            }
            if self.remove_regular_if_present(intent_name).is_err() {
                return Ok(RecoveryOutcome::ManualInterventionRequired {
                    transaction: intent.transaction,
                });
            }
            if self.sync_parent_directory().is_err() {
                return Ok(RecoveryOutcome::Committed {
                    revision: intent.intended_revision,
                    commit_state: CommitState::PublishedDurabilityUncertain,
                });
            }
            return Ok(RecoveryOutcome::Committed {
                revision: intent.intended_revision,
                commit_state,
            });
        }

        let target_is_previous = match (&target, &intent.previous_revision) {
            (None, None) => true,
            (Some(stored), Some(previous)) => stored.revision == *previous,
            _ => false,
        };
        let stage_is_intended = stage
            .as_ref()
            .is_some_and(|stored| stored.revision == intent.intended_revision);
        if target_is_previous && stage_is_intended {
            self.remove_regular_if_present(&intent.stage_name)?;
            self.remove_regular_if_present(intent_name)?;
            let _ = self.sync_parent_directory()?;
            return Ok(RecoveryOutcome::DiscardedUnpublished {
                transaction: intent.transaction,
            });
        }

        if target.is_none() {
            if let (Some(backup), Some(previous)) = (
                intent.transient_backup.as_deref(),
                intent.previous_revision.as_ref(),
            ) {
                let stored_backup = self.read_named(backup, MAX_INTERNAL_FILE_BYTES)?;
                if stored_backup
                    .as_ref()
                    .is_some_and(|stored| stored.revision == *previous)
                {
                    #[cfg(windows)]
                    {
                        let backup_guard = self.open_recovery_source_guard(backup, previous)?;
                        windows_rename_retained_file(
                            &backup_guard,
                            &self.directory,
                            intent.target.as_str(),
                            false,
                        )
                        .map_err(|_| {
                            DurableFileFailure::recovery_required(Some(intent.transaction))
                        })?;
                    }
                    #[cfg(not(windows))]
                    rename_no_replace(&self.directory, backup, intent.target.as_str()).map_err(
                        |_| DurableFileFailure::recovery_required(Some(intent.transaction)),
                    )?;
                    let restored =
                        self.read_named(intent.target.as_str(), MAX_INTERNAL_FILE_BYTES)?;
                    if !restored
                        .as_ref()
                        .is_some_and(|stored| stored.revision == *previous)
                    {
                        return Ok(RecoveryOutcome::ManualInterventionRequired {
                            transaction: intent.transaction,
                        });
                    }
                    if self.sync_recovery_parent().is_err() {
                        return Ok(RecoveryOutcome::ManualInterventionRequired {
                            transaction: intent.transaction,
                        });
                    }
                    self.remove_regular_if_present(intent_name)?;
                    self.sync_parent_directory()?;
                    return Ok(RecoveryOutcome::RolledBack {
                        revision: Some(previous.clone()),
                    });
                }
            }
        }

        Ok(RecoveryOutcome::ManualInterventionRequired {
            transaction: intent.transaction,
        })
    }
}

impl fmt::Debug for DurableFileStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DurableFileStore(..)")
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct StorageFileName(String);

impl StorageFileName {
    pub fn parse(value: impl Into<String>) -> Result<Self, DurableFileFailure> {
        let value = value.into();
        let lower = value.to_ascii_lowercase();
        if value.is_empty()
            || matches!(value.as_str(), "." | "..")
            || value.chars().any(char::is_control)
            || value.contains(['/', '\\', ':'])
            || value.ends_with(['.', ' '])
            || lower.starts_with(TRANSACTION_PREFIX)
            || is_windows_device_alias(&value)
        {
            return Err(DurableFileFailure::invalid_name());
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn is_windows_device_alias(value: &str) -> bool {
    let stem = value
        .split('.')
        .next()
        .unwrap_or(value)
        .trim_end_matches(['.', ' '])
        .to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].into_iter().any(|prefix| {
            stem.strip_prefix(prefix).is_some_and(|number| {
                matches!(
                    number,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
        })
}

impl<'de> Deserialize<'de> for StorageFileName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(|_| D::Error::custom("invalid storage file name"))
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct FileRevision(String);

impl FileRevision {
    fn digest(bytes: &[u8]) -> Self {
        let digest = Sha256::digest(bytes);
        let mut value = String::with_capacity(71);
        value.push_str("sha256:");
        for byte in digest {
            use std::fmt::Write as _;
            let _ = write!(value, "{byte:02x}");
        }
        Self(value)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for FileRevision {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let bytes = value.as_bytes();
        let valid = bytes.len() == 71
            && bytes.starts_with(b"sha256:")
            && bytes[7..]
                .iter()
                .copied()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'));
        if !valid {
            return Err(D::Error::custom("invalid file revision"));
        }
        Ok(Self(value))
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct StoredFile {
    pub bytes: Vec<u8>,
    pub revision: FileRevision,
}

impl fmt::Debug for StoredFile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StoredFile")
            .field("byte_len", &self.bytes.len())
            .field("revision", &self.revision)
            .finish()
    }
}

impl Drop for StoredFile {
    fn drop(&mut self) {
        clear_sensitive_bytes(&mut self.bytes);
    }
}

fn clear_sensitive_bytes(bytes: &mut Vec<u8>) {
    bytes.zeroize();
}

#[derive(Clone, Copy, Debug)]
pub enum ExpectedFile<'a> {
    Absent,
    Revision(&'a FileRevision),
}

#[derive(Clone, Copy, Debug)]
pub enum PreservePrevious<'a> {
    None,
    Required { recovery_name: &'a StorageFileName },
}

#[derive(Clone, Copy)]
pub struct ReplaceRequest<'a> {
    pub target: &'a StorageFileName,
    pub bytes: &'a [u8],
    pub expected: ExpectedFile<'a>,
    pub preserve_previous: PreservePrevious<'a>,
}

impl fmt::Debug for ReplaceRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReplaceRequest")
            .field("target", &self.target)
            .field("byte_len", &self.bytes.len())
            .field("expected", &self.expected)
            .field("preserve_previous", &self.preserve_previous)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommitState {
    Durable,
    /// The new file is atomically visible and its contents were synchronized,
    /// but this platform cannot prove Unix-style parent-directory durability.
    /// Publication is complete and does not require recovery.
    AtomicVisibility,
    /// The target is published, but the platform could not prove directory
    /// durability because a commit or finalization operation failed. Do not
    /// replay the write; call [`DurableFileStore::recover`] before attempting
    /// another mutation in this workspace.
    PublishedDurabilityUncertain,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplaceOutcome {
    pub installed_revision: FileRevision,
    pub commit_state: CommitState,
    pub preserved_as: Option<StorageFileName>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Deserialize, Serialize)]
#[serde(transparent)]
pub struct RecoveryTransactionId(Uuid);

impl RecoveryTransactionId {
    fn stage_name(self) -> String {
        format!("{TRANSACTION_PREFIX}{}.stage", self.0)
    }

    fn intent_name(self) -> String {
        format!("{TRANSACTION_PREFIX}{}.intent", self.0)
    }

    #[cfg(windows)]
    fn backup_name(self) -> String {
        format!("{TRANSACTION_PREFIX}{}.backup", self.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecoveryOutcome {
    Committed {
        revision: FileRevision,
        commit_state: CommitState,
    },
    RolledBack {
        revision: Option<FileRevision>,
    },
    DiscardedUnpublished {
        transaction: RecoveryTransactionId,
    },
    ManualInterventionRequired {
        transaction: RecoveryTransactionId,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DurableFileFailureKind {
    InvalidName,
    UnsafeEntry,
    TooLarge,
    RevisionConflict,
    NotPublished,
    PublishStateUncertain,
    RecoveryRequired,
    Unavailable,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct DurableFileFailure {
    kind: DurableFileFailureKind,
    transaction: Option<RecoveryTransactionId>,
}

impl DurableFileFailure {
    pub const fn kind(self) -> DurableFileFailureKind {
        self.kind
    }

    pub const fn transaction(self) -> Option<RecoveryTransactionId> {
        self.transaction
    }

    const fn new(kind: DurableFileFailureKind, transaction: Option<RecoveryTransactionId>) -> Self {
        Self { kind, transaction }
    }

    const fn invalid_name() -> Self {
        Self::new(DurableFileFailureKind::InvalidName, None)
    }

    const fn unsafe_entry() -> Self {
        Self::new(DurableFileFailureKind::UnsafeEntry, None)
    }

    const fn too_large() -> Self {
        Self::new(DurableFileFailureKind::TooLarge, None)
    }

    const fn revision_conflict() -> Self {
        Self::new(DurableFileFailureKind::RevisionConflict, None)
    }

    const fn not_published(transaction: Option<RecoveryTransactionId>) -> Self {
        Self::new(DurableFileFailureKind::NotPublished, transaction)
    }

    const fn publish_uncertain(transaction: RecoveryTransactionId) -> Self {
        Self::new(
            DurableFileFailureKind::PublishStateUncertain,
            Some(transaction),
        )
    }

    const fn recovery_required(transaction: Option<RecoveryTransactionId>) -> Self {
        Self::new(DurableFileFailureKind::RecoveryRequired, transaction)
    }

    const fn unavailable() -> Self {
        Self::new(DurableFileFailureKind::Unavailable, None)
    }
}

impl fmt::Debug for DurableFileFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DurableFileFailure")
            .field("kind", &self.kind)
            .field("transaction", &self.transaction)
            .finish()
    }
}

impl fmt::Display for DurableFileFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self.kind {
            DurableFileFailureKind::InvalidName => "the storage file name is invalid",
            DurableFileFailureKind::UnsafeEntry => "a storage entry is unsafe",
            DurableFileFailureKind::TooLarge => "the stored file exceeds its size limit",
            DurableFileFailureKind::RevisionConflict => "the stored file revision changed",
            DurableFileFailureKind::NotPublished => "the replacement was not published",
            DurableFileFailureKind::PublishStateUncertain => {
                "the replacement publish state requires recovery"
            }
            DurableFileFailureKind::RecoveryRequired => "storage recovery is required",
            DurableFileFailureKind::Unavailable => "durable storage is unavailable",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for DurableFileFailure {}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RecoveryIntent {
    schema_version: u8,
    writer_epoch: Uuid,
    transaction: RecoveryTransactionId,
    target: StorageFileName,
    stage_name: String,
    transient_backup: Option<String>,
    previous_revision: Option<FileRevision>,
    intended_revision: FileRevision,
}

impl RecoveryIntent {
    fn intent_name(&self) -> String {
        self.transaction.intent_name()
    }
}

fn transient_backup_is_valid(intent: &RecoveryIntent) -> bool {
    #[cfg(windows)]
    {
        intent.transient_backup.is_none()
            || intent.transient_backup.as_deref()
                == intent
                    .previous_revision
                    .as_ref()
                    .map(|_| intent.transaction.backup_name())
                    .as_deref()
    }
    #[cfg(not(windows))]
    {
        intent.transient_backup.is_none()
    }
}

fn transaction_from_artifact_name(name: &str, suffix: &str) -> Option<RecoveryTransactionId> {
    let encoded = name
        .strip_prefix(TRANSACTION_PREFIX)?
        .strip_suffix(suffix)?;
    let transaction = RecoveryTransactionId(Uuid::parse_str(encoded).ok()?);
    let expected = match suffix {
        ".stage" => transaction.stage_name(),
        ".intent" => transaction.intent_name(),
        #[cfg(windows)]
        ".backup" => transaction.backup_name(),
        _ => return None,
    };
    (expected == name).then_some(transaction)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ParentSyncState {
    #[cfg(any(test, unix))]
    #[cfg_attr(all(test, not(unix)), allow(dead_code))]
    Durable,
    #[cfg(any(test, not(unix)))]
    PlatformUncertain,
}

impl ParentSyncState {
    const fn into_commit_state(self) -> CommitState {
        match self {
            #[cfg(any(test, unix))]
            Self::Durable => CommitState::Durable,
            #[cfg(any(test, not(unix)))]
            Self::PlatformUncertain => CommitState::AtomicVisibility,
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct RegularFileIdentity {
    device: u64,
    inode: u64,
    length: u64,
}

fn regular_file_identity(
    metadata: &impl StorageMetadata,
) -> Result<RegularFileIdentity, DurableFileFailure> {
    if !metadata.is_file() || metadata.is_symlink() || metadata.nlink() != 1 {
        return Err(DurableFileFailure::unsafe_entry());
    }
    Ok(RegularFileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
        length: metadata.len(),
    })
}

trait StorageMetadata {
    fn is_file(&self) -> bool;
    fn is_symlink(&self) -> bool;
    fn len(&self) -> u64;
    fn dev(&self) -> u64;
    fn ino(&self) -> u64;
    fn nlink(&self) -> u64;
}

macro_rules! impl_storage_metadata {
    ($type:ty) => {
        impl StorageMetadata for $type {
            fn is_file(&self) -> bool {
                self.is_file()
            }

            fn is_symlink(&self) -> bool {
                self.file_type().is_symlink()
            }

            fn len(&self) -> u64 {
                self.len()
            }

            fn dev(&self) -> u64 {
                MetadataExt::dev(self)
            }

            fn ino(&self) -> u64 {
                MetadataExt::ino(self)
            }

            fn nlink(&self) -> u64 {
                MetadataExt::nlink(self)
            }
        }
    };
}

impl_storage_metadata!(cap_std::fs::Metadata);

fn verify_expected(
    expected: ExpectedFile<'_>,
    actual: Option<&StoredFile>,
) -> Result<(), DurableFileFailure> {
    let matches = match (expected, actual) {
        (ExpectedFile::Absent, None) => true,
        (ExpectedFile::Revision(expected), Some(actual)) => expected == &actual.revision,
        _ => false,
    };
    if matches {
        Ok(())
    } else {
        Err(DurableFileFailure::revision_conflict())
    }
}

fn revisions_match(left: Option<&StoredFile>, right: Option<&StoredFile>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => left.revision == right.revision,
        _ => false,
    }
}

trait FaultInjector: Send + Sync {
    fn fail_at(&self, point: FaultPoint) -> bool;

    fn before_publish_validation(&self, _root: &Path, _stage: &str, _target: &str) {}

    #[cfg(test)]
    fn after_final_publish_validation(&self, _root: &Path, _stage: &str, _target: &str) {}

    #[cfg(test)]
    fn fail_new_file_write(&self, _name: &str) -> bool {
        false
    }
}

struct NoFaults;

impl FaultInjector for NoFaults {
    fn fail_at(&self, _point: FaultPoint) -> bool {
        false
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum FaultPoint {
    BeforePublish,
    LeavePrepared,
    AfterPublishReportsFailure,
    ParentSyncFailure,
    IntentSyncFailure,
    FinalizeFailure,
    #[cfg(test)]
    ParentSyncUncertain,
    #[cfg(test)]
    RecoverySyncFailure,
    #[cfg(test)]
    RecoverySyncUncertain,
    #[cfg(test)]
    RecoveryWriteFailure,
    #[cfg(test)]
    MutateStageAfterValidation,
    #[cfg(test)]
    MutateTargetAfterValidation,
    #[cfg(test)]
    MutateTargetAfterFinalValidation,
    #[cfg(test)]
    SwapStageAfterFinalValidation,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DurableFileTestFault {
    AfterPublishReportsFailure,
    FinalizeFailure,
    LeavePrepared,
    ParentSyncFailure,
    PlatformDirectorySyncUncertain,
}

#[cfg(test)]
impl DurableFileTestFault {
    const fn point(self) -> FaultPoint {
        match self {
            Self::AfterPublishReportsFailure => FaultPoint::AfterPublishReportsFailure,
            Self::FinalizeFailure => FaultPoint::FinalizeFailure,
            Self::LeavePrepared => FaultPoint::LeavePrepared,
            Self::ParentSyncFailure => FaultPoint::ParentSyncFailure,
            Self::PlatformDirectorySyncUncertain => FaultPoint::ParentSyncUncertain,
        }
    }
}

#[cfg(test)]
struct DurableFileTestFaultInjector {
    point: FaultPoint,
    fired: std::sync::atomic::AtomicBool,
}

#[cfg(test)]
impl FaultInjector for DurableFileTestFaultInjector {
    fn fail_at(&self, point: FaultPoint) -> bool {
        use std::sync::atomic::Ordering;

        point == self.point && !self.fired.swap(true, Ordering::SeqCst)
    }
}

#[cfg(unix)]
fn sync_directory(directory: &Dir) -> io::Result<ParentSyncState> {
    directory.try_clone()?.into_std_file().sync_all()?;
    Ok(ParentSyncState::Durable)
}

#[cfg(windows)]
fn sync_directory(_directory: &Dir) -> io::Result<ParentSyncState> {
    Ok(ParentSyncState::PlatformUncertain)
}

#[cfg(not(any(unix, windows)))]
fn sync_directory(_directory: &Dir) -> io::Result<ParentSyncState> {
    Ok(ParentSyncState::PlatformUncertain)
}

#[cfg(unix)]
fn publish_atomic(
    directory: &Dir,
    _stage_file: &File,
    stage: &str,
    target: &str,
    target_exists: bool,
) -> io::Result<()> {
    if target_exists {
        directory.rename(stage, directory, target)
    } else {
        rustix::fs::renameat_with(
            directory,
            stage,
            directory,
            target,
            rustix::fs::RenameFlags::NOREPLACE,
        )
        .map_err(Into::into)
    }
}

#[cfg(windows)]
fn publish_atomic(
    directory: &Dir,
    stage_file: &File,
    _stage: &str,
    target: &str,
    target_exists: bool,
) -> io::Result<()> {
    windows_rename_retained_file(stage_file, directory, target, target_exists)
}

#[cfg(not(any(unix, windows)))]
fn publish_atomic(
    directory: &Dir,
    _stage_file: &File,
    stage: &str,
    target: &str,
    target_exists: bool,
) -> io::Result<()> {
    if target_exists {
        directory.rename(stage, directory, target)
    } else {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "atomic create publication is unsupported",
        ))
    }
}

#[cfg(unix)]
fn rename_no_replace(directory: &Dir, source: &str, target: &str) -> io::Result<()> {
    rustix::fs::renameat_with(
        directory,
        source,
        directory,
        target,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(Into::into)
}

#[cfg(not(any(unix, windows)))]
fn rename_no_replace(_directory: &Dir, _source: &str, _target: &str) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "atomic recovery is unsupported",
    ))
}

#[cfg(windows)]
fn windows_rename_retained_file(
    source: &File,
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
            "storage destination name is invalid",
        ));
    }
    let destination_name_bytes = destination_name
        .len()
        .checked_mul(std::mem::size_of::<u16>())
        .and_then(|length| u32::try_from(length).ok())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "storage destination name is too long",
            )
        })?;
    let offset = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
    let buffer_bytes = offset
        .checked_add(destination_name_bytes as usize)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "storage rename buffer is too large",
            )
        })?;
    let mut buffer = vec![0usize; buffer_bytes.div_ceil(std::mem::size_of::<usize>())];
    let rename_info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    let renamed = unsafe {
        (*rename_info).Anonymous.Flags = if replace {
            // The retained target guard denies ordinary writers and deletes.
            // POSIX replacement retires it atomically, but an external actor
            // can use the same flag, so revision checks remain optimistic.
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
                    "storage rename buffer is too large",
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn sensitive_byte_helper_overwrites_every_retained_byte() {
        let mut bytes = b"inline-credential-material".to_vec();
        let original_len = bytes.len();

        clear_sensitive_bytes(&mut bytes);

        assert!(bytes.is_empty());
        assert!(bytes.spare_capacity_mut()[..original_len]
            .iter()
            .all(|byte| unsafe { byte.assume_init() } == 0));
    }

    #[test]
    fn prepublish_failure_leaves_no_visible_target_or_recovery_transaction() {
        let temporary = tempdir().expect("temporary root");
        let store = test_store(temporary.path(), FaultPoint::BeforePublish);
        let target = StorageFileName::parse("state.json").expect("target");

        let error = store
            .replace(ReplaceRequest {
                target: &target,
                bytes: b"new",
                expected: ExpectedFile::Absent,
                preserve_previous: PreservePrevious::None,
            })
            .expect_err("publish should stop before rename");

        assert_eq!(error.kind(), DurableFileFailureKind::NotPublished);
        assert!(store.read(&target, 16).expect("read target").is_none());
        assert!(store.recover().expect("recovery scan").is_empty());
    }

    #[test]
    fn an_error_reported_after_rename_is_classified_as_committed() {
        let temporary = tempdir().expect("temporary root");
        let store = test_store(temporary.path(), FaultPoint::AfterPublishReportsFailure);
        let target = StorageFileName::parse("state.json").expect("target");

        let outcome = store
            .replace(ReplaceRequest {
                target: &target,
                bytes: b"new",
                expected: ExpectedFile::Absent,
                preserve_previous: PreservePrevious::None,
            })
            .expect("observed target decides the outcome");

        assert_eq!(outcome.installed_revision, FileRevision::digest(b"new"));
        assert_eq!(
            store
                .read(&target, 16)
                .expect("read")
                .expect("target")
                .bytes,
            b"new"
        );
    }

    #[test]
    fn interrupted_prepublication_is_discarded_without_replaying_the_write() {
        let temporary = tempdir().expect("temporary root");
        let target = StorageFileName::parse("state.json").expect("target");
        let store = test_store(temporary.path(), FaultPoint::LeavePrepared);
        let error = store
            .replace(ReplaceRequest {
                target: &target,
                bytes: b"new",
                expected: ExpectedFile::Absent,
                preserve_previous: PreservePrevious::None,
            })
            .expect_err("simulated crash leaves an intent");
        assert_eq!(error.kind(), DurableFileFailureKind::PublishStateUncertain);
        drop(store);

        let recovered = no_fault_store(temporary.path())
            .recover()
            .expect("recover prepared transaction");
        assert!(matches!(
            recovered.as_slice(),
            [RecoveryOutcome::DiscardedUnpublished { .. }]
        ));
        assert!(!temporary.path().join(target.as_str()).exists());
    }

    #[test]
    fn parent_sync_failure_is_a_published_success_and_recovery_confirms_it() {
        let temporary = tempdir().expect("temporary root");
        let target = StorageFileName::parse("state.json").expect("target");
        let store = test_store(temporary.path(), FaultPoint::ParentSyncFailure);
        let outcome = store
            .replace(ReplaceRequest {
                target: &target,
                bytes: b"new",
                expected: ExpectedFile::Absent,
                preserve_previous: PreservePrevious::None,
            })
            .expect("published mutations do not become ordinary failures");
        assert_eq!(
            outcome.commit_state,
            CommitState::PublishedDurabilityUncertain
        );
        drop(store);

        let recovered = no_fault_store(temporary.path())
            .recover()
            .expect("recover published transaction");
        assert!(matches!(
            recovered.as_slice(),
            [RecoveryOutcome::Committed { .. }]
        ));
        assert_eq!(
            std::fs::read(temporary.path().join("state.json")).unwrap(),
            b"new"
        );
    }

    #[test]
    fn intent_sync_failure_cleans_prepublication_material_without_publishing() {
        let temporary = tempdir().expect("temporary root");
        let target = StorageFileName::parse("state.json").expect("target");
        let store = test_store(temporary.path(), FaultPoint::IntentSyncFailure);

        let error = store
            .replace(ReplaceRequest {
                target: &target,
                bytes: b"new",
                expected: ExpectedFile::Absent,
                preserve_previous: PreservePrevious::None,
            })
            .expect_err("intent durability fails before publication");

        assert_eq!(error.kind(), DurableFileFailureKind::NotPublished);
        assert!(!temporary.path().join(target.as_str()).exists());
        assert!(no_fault_store(temporary.path())
            .recover()
            .expect("nothing pending")
            .is_empty());
    }

    #[test]
    fn cleanup_failure_after_publication_stays_a_success_with_uncertain_durability() {
        let temporary = tempdir().expect("temporary root");
        let target = StorageFileName::parse("state.json").expect("target");
        let store = test_store(temporary.path(), FaultPoint::FinalizeFailure);

        let outcome = store
            .replace(ReplaceRequest {
                target: &target,
                bytes: b"new",
                expected: ExpectedFile::Absent,
                preserve_previous: PreservePrevious::None,
            })
            .expect("published mutation must not become an ordinary error");

        assert_eq!(
            outcome.commit_state,
            CommitState::PublishedDurabilityUncertain
        );
        assert_eq!(
            std::fs::read(temporary.path().join("state.json")).unwrap(),
            b"new"
        );
    }

    #[test]
    fn platform_without_directory_sync_reports_atomic_visibility_and_allows_the_next_write() {
        let temporary = tempdir().expect("temporary root");
        let target = StorageFileName::parse("state.json").expect("target");
        let store = test_store(temporary.path(), FaultPoint::ParentSyncUncertain);

        let outcome = store
            .replace(ReplaceRequest {
                target: &target,
                bytes: b"new",
                expected: ExpectedFile::Absent,
                preserve_previous: PreservePrevious::None,
            })
            .expect("publication is visible even when durability is uncertain");

        assert_eq!(outcome.commit_state, CommitState::AtomicVisibility);
        assert_eq!(artifact_count(temporary.path(), ".intent"), 0);
        store
            .replace(ReplaceRequest {
                target: &target,
                bytes: b"newer",
                expected: ExpectedFile::Revision(&outcome.installed_revision),
                preserve_previous: PreservePrevious::None,
            })
            .expect("platform-level directory uncertainty must not latch recovery");
    }

    #[test]
    fn recovery_sync_failure_keeps_every_recovery_artifact() {
        let temporary = tempdir().expect("temporary root");
        let target = StorageFileName::parse("state.json").expect("target");
        let store = test_store(temporary.path(), FaultPoint::ParentSyncFailure);
        let outcome = store
            .replace(ReplaceRequest {
                target: &target,
                bytes: b"new",
                expected: ExpectedFile::Absent,
                preserve_previous: PreservePrevious::None,
            })
            .expect("published state is uncertain");
        assert_eq!(
            outcome.commit_state,
            CommitState::PublishedDurabilityUncertain
        );
        drop(store);

        let recovering = test_store(temporary.path(), FaultPoint::RecoverySyncFailure);
        let recovered = recovering.recover().expect("recovery remains inspectable");

        assert!(matches!(
            recovered.as_slice(),
            [RecoveryOutcome::Committed {
                commit_state: CommitState::PublishedDurabilityUncertain,
                ..
            }]
        ));
        assert_eq!(artifact_count(temporary.path(), ".intent"), 1);
    }

    #[test]
    fn rebuilding_a_store_after_atomic_visibility_has_no_latched_recovery() {
        let temporary = tempdir().expect("temporary root");
        let target = StorageFileName::parse("state.json").expect("target");
        let launch_epoch = Uuid::new_v4();
        let store = test_store_with_epoch(
            temporary.path(),
            FaultPoint::ParentSyncUncertain,
            launch_epoch,
        );
        let outcome = store
            .replace(ReplaceRequest {
                target: &target,
                bytes: b"new",
                expected: ExpectedFile::Absent,
                preserve_previous: PreservePrevious::None,
            })
            .expect("atomically visible publication");
        assert_eq!(outcome.commit_state, CommitState::AtomicVisibility);
        drop(store);

        let recovering = no_fault_store(temporary.path());
        let recovered = recovering.recover().expect("same-launch recovery");

        assert!(recovered.is_empty());
        assert_eq!(artifact_count(temporary.path(), ".intent"), 0);
        recovering
            .replace(ReplaceRequest {
                target: &target,
                bytes: b"newer",
                expected: ExpectedFile::Revision(&outcome.installed_revision),
                preserve_previous: PreservePrevious::None,
            })
            .expect("reconstruction must not latch platform-level uncertainty");
    }

    #[cfg(windows)]
    #[test]
    fn windows_atomic_visibility_replacement_does_not_latch_the_store() {
        let temporary = tempdir().expect("temporary root");
        let target = StorageFileName::parse("state.json").expect("target");
        let store = no_fault_store(temporary.path());
        let initial = store
            .replace(ReplaceRequest {
                target: &target,
                bytes: b"previous",
                expected: ExpectedFile::Absent,
                preserve_previous: PreservePrevious::None,
            })
            .expect("initial state");
        let replaced = store
            .replace(ReplaceRequest {
                target: &target,
                bytes: b"intended",
                expected: ExpectedFile::Revision(&initial.installed_revision),
                preserve_previous: PreservePrevious::None,
            })
            .expect("handle-relative replacement");

        assert_eq!(replaced.commit_state, CommitState::AtomicVisibility);
        assert_eq!(artifact_count(temporary.path(), ".intent"), 0);
        assert_eq!(
            std::fs::read(temporary.path().join(target.as_str())).unwrap(),
            b"intended"
        );
        store
            .replace(ReplaceRequest {
                target: &target,
                bytes: b"newer",
                expected: ExpectedFile::Revision(&replaced.installed_revision),
                preserve_previous: PreservePrevious::None,
            })
            .expect("normal Windows publication must not block the next write");
    }

    #[cfg(windows)]
    #[test]
    fn windows_guards_block_ordinary_mutators_but_posix_handle_replace_preserves_identities() {
        let temporary = tempdir().expect("temporary root");
        let store = no_fault_store(temporary.path());
        std::fs::write(temporary.path().join("state.json"), b"previous").unwrap();
        let previous_revision = FileRevision::digest(b"previous");
        let target_guard = store
            .open_revision_guard("state.json", &previous_revision)
            .expect("retained target guard");
        assert!(std::fs::OpenOptions::new()
            .write(true)
            .open(temporary.path().join("state.json"))
            .is_err());
        assert!(std::fs::remove_file(temporary.path().join("state.json")).is_err());

        let staged = store
            .write_new_file("stage.tmp", b"intended")
            .expect("retained stage");
        assert!(std::fs::OpenOptions::new()
            .write(true)
            .open(temporary.path().join("stage.tmp"))
            .is_err());
        assert!(std::fs::rename(
            temporary.path().join("stage.tmp"),
            temporary.path().join("swapped.tmp")
        )
        .is_err());

        windows_rename_retained_file(&staged, &store.directory, "state.json", true)
            .expect("POSIX handle replacement");

        assert_eq!(
            store
                .read_retained_named("state.json", &staged, 64)
                .unwrap()
                .bytes,
            b"intended"
        );
        let mut old_reader = target_guard.try_clone().unwrap();
        old_reader.seek(SeekFrom::Start(0)).unwrap();
        let mut old_bytes = Vec::new();
        old_reader.read_to_end(&mut old_bytes).unwrap();
        assert_eq!(old_bytes, b"previous");
    }

    #[cfg(windows)]
    #[test]
    fn windows_invalid_handle_rename_fails_closed_without_an_ambient_fallback() {
        let temporary = tempdir().expect("temporary root");
        let store = no_fault_store(temporary.path());
        let staged = store
            .write_new_file("stage.tmp", b"intended")
            .expect("retained stage");

        assert!(
            windows_rename_retained_file(&staged, &store.directory, "invalid\0target", false)
                .is_err()
        );
        assert_eq!(
            store
                .read_retained_named("stage.tmp", &staged, 64)
                .unwrap()
                .bytes,
            b"intended"
        );
        assert!(!temporary.path().join("invalid").exists());
    }

    #[test]
    fn committed_backup_cleanup_is_idempotent_but_rejects_wrong_remaining_bytes() {
        let temporary = tempdir().expect("temporary root");
        let store = no_fault_store(temporary.path());
        let transaction = RecoveryTransactionId(Uuid::new_v4());
        let previous_revision = FileRevision::digest(b"previous");
        let intent = RecoveryIntent {
            schema_version: 2,
            writer_epoch: store.writer_epoch,
            transaction,
            target: StorageFileName::parse("state.json").expect("target"),
            stage_name: transaction.stage_name(),
            transient_backup: Some("previous.backup".to_string()),
            previous_revision: Some(previous_revision),
            intended_revision: FileRevision::digest(b"intended"),
        };

        store
            .cleanup_committed_backup(&intent)
            .expect("a missing backup is an already-completed cleanup step");

        std::fs::write(temporary.path().join("previous.backup"), b"wrong")
            .expect("wrong backup fixture");
        assert!(store.cleanup_committed_backup(&intent).is_err());
        assert_eq!(
            std::fs::read(temporary.path().join("previous.backup")).unwrap(),
            b"wrong"
        );
    }

    #[test]
    fn failed_recovery_copy_does_not_leave_a_partial_destination() {
        let temporary = tempdir().expect("temporary root");
        std::fs::write(temporary.path().join("state.json"), b"unsupported")
            .expect("unsupported state");
        let target = StorageFileName::parse("state.json").expect("target");
        let recovery = StorageFileName::parse("state.unsupported.json").expect("recovery");
        let store = test_store(temporary.path(), FaultPoint::RecoveryWriteFailure);
        let current = store.read(&target, 64).expect("read").expect("target");

        let error = store
            .replace(ReplaceRequest {
                target: &target,
                bytes: b"new",
                expected: ExpectedFile::Revision(&current.revision),
                preserve_previous: PreservePrevious::Required {
                    recovery_name: &recovery,
                },
            })
            .expect_err("partial recovery writes must abort publication");

        assert_eq!(error.kind(), DurableFileFailureKind::NotPublished);
        assert!(!temporary.path().join(recovery.as_str()).exists());
        assert_eq!(
            std::fs::read(temporary.path().join(target.as_str())).unwrap(),
            b"unsupported"
        );
    }

    #[test]
    fn oversized_replacement_is_rejected_before_creating_artifacts() {
        let temporary = tempdir().expect("temporary root");
        let target = StorageFileName::parse("state.json").expect("target");
        let store = no_fault_store(temporary.path());
        let oversized = vec![0_u8; MAX_INTERNAL_FILE_BYTES as usize + 1];

        let error = store
            .replace(ReplaceRequest {
                target: &target,
                bytes: &oversized,
                expected: ExpectedFile::Absent,
                preserve_previous: PreservePrevious::None,
            })
            .expect_err("oversized input must fail before staging");

        assert_eq!(error.kind(), DurableFileFailureKind::TooLarge);
        assert_eq!(std::fs::read_dir(temporary.path()).unwrap().count(), 0);
    }

    #[test]
    fn orphan_stage_from_a_preintent_crash_is_discarded_without_replay() {
        let temporary = tempdir().expect("temporary root");
        let transaction = RecoveryTransactionId(Uuid::new_v4());
        std::fs::write(temporary.path().join(transaction.stage_name()), b"new")
            .expect("orphan stage");

        let outcomes = no_fault_store(temporary.path())
            .recover()
            .expect("recover orphan stage");

        assert!(matches!(
            outcomes.as_slice(),
            [RecoveryOutcome::DiscardedUnpublished { transaction: recovered }]
                if *recovered == transaction
        ));
        assert!(!temporary.path().join(transaction.stage_name()).exists());
    }

    #[test]
    fn same_inode_stage_rewrite_is_detected_before_publication() {
        let temporary = tempdir().expect("temporary root");
        let target = StorageFileName::parse("state.json").expect("target");
        let store = test_store(temporary.path(), FaultPoint::MutateStageAfterValidation);

        let error = store
            .replace(ReplaceRequest {
                target: &target,
                bytes: b"new",
                expected: ExpectedFile::Absent,
                preserve_previous: PreservePrevious::None,
            })
            .expect_err("rewritten stage must not publish");

        assert_eq!(error.kind(), DurableFileFailureKind::NotPublished);
        assert!(!temporary.path().join(target.as_str()).exists());
    }

    #[test]
    fn target_change_after_staging_is_a_conflict_and_is_not_overwritten() {
        let temporary = tempdir().expect("temporary root");
        let target = StorageFileName::parse("state.json").expect("target");
        let store = test_store(temporary.path(), FaultPoint::MutateTargetAfterValidation);

        let error = store
            .replace(ReplaceRequest {
                target: &target,
                bytes: b"new",
                expected: ExpectedFile::Absent,
                preserve_previous: PreservePrevious::None,
            })
            .expect_err("concurrent target creation must win");

        assert_eq!(error.kind(), DurableFileFailureKind::RevisionConflict);
        assert_eq!(
            std::fs::read(temporary.path().join(target.as_str())).unwrap(),
            b"other"
        );
    }

    #[test]
    fn target_change_after_final_validation_is_not_overwritten() {
        let temporary = tempdir().expect("temporary root");
        let target = StorageFileName::parse("state.json").expect("target");
        let store = test_store(
            temporary.path(),
            FaultPoint::MutateTargetAfterFinalValidation,
        );

        let error = store
            .replace(ReplaceRequest {
                target: &target,
                bytes: b"new",
                expected: ExpectedFile::Absent,
                preserve_previous: PreservePrevious::None,
            })
            .expect_err("the last cooperative race check must preserve the winner");

        assert_eq!(error.kind(), DurableFileFailureKind::RevisionConflict);
        assert_eq!(
            std::fs::read(temporary.path().join(target.as_str())).unwrap(),
            b"other"
        );
    }

    #[test]
    fn stage_name_swap_after_final_validation_never_publishes_the_replacement_entry() {
        let temporary = tempdir().expect("temporary root");
        let target = StorageFileName::parse("state.json").expect("target");
        let store = test_store(temporary.path(), FaultPoint::SwapStageAfterFinalValidation);

        let error = store
            .replace(ReplaceRequest {
                target: &target,
                bytes: b"intended",
                expected: ExpectedFile::Absent,
                preserve_previous: PreservePrevious::None,
            })
            .expect_err("a swapped stage name must fail closed");

        assert_eq!(error.kind(), DurableFileFailureKind::PublishStateUncertain);
        assert!(!temporary.path().join(target.as_str()).exists());
        assert_eq!(
            std::fs::read(temporary.path().join("swapped-stage")).unwrap(),
            b"intended"
        );
    }

    #[test]
    fn tampered_intent_cannot_redirect_recovery_to_an_unrelated_file() {
        let temporary = tempdir().expect("temporary root");
        let transaction = RecoveryTransactionId(Uuid::new_v4());
        let victim = temporary.path().join("victim");
        std::fs::write(&victim, b"keep").expect("victim");
        let intent = serde_json::json!({
            "schemaVersion": 2,
            "writerEpoch": Uuid::new_v4(),
            "transaction": transaction,
            "target": "state.json",
            "stageName": "victim",
            "transientBackup": null,
            "previousRevision": null,
            "intendedRevision": FileRevision::digest(b"new")
        });
        std::fs::write(
            temporary.path().join(transaction.intent_name()),
            serde_json::to_vec(&intent).unwrap(),
        )
        .expect("tampered intent");

        let error = no_fault_store(temporary.path())
            .recover()
            .expect_err("tampered transaction must require manual recovery");

        assert_eq!(error.kind(), DurableFileFailureKind::RecoveryRequired);
        assert_eq!(std::fs::read(victim).unwrap(), b"keep");
    }

    #[cfg(windows)]
    #[test]
    fn windows_root_swap_during_rollback_restores_only_the_retained_root() {
        let parent = tempdir().expect("temporary parent");
        let root = parent.path().join("state");
        std::fs::create_dir(&root).expect("storage root");
        let store = no_fault_store(&root);
        let transaction = RecoveryTransactionId(Uuid::new_v4());
        let target = StorageFileName::parse("state.json").expect("target");
        let previous_revision = FileRevision::digest(b"previous");
        let intended_revision = FileRevision::digest(b"intended");
        store
            .write_new_file(&transaction.stage_name(), b"intended")
            .expect("stage");
        store
            .write_new_file(&transaction.backup_name(), b"previous")
            .expect("backup");
        let intent = RecoveryIntent {
            schema_version: 2,
            writer_epoch: store.writer_epoch,
            transaction,
            target: target.clone(),
            stage_name: transaction.stage_name(),
            transient_backup: Some(transaction.backup_name()),
            previous_revision: Some(previous_revision),
            intended_revision,
        };
        store
            .write_intent(&transaction.intent_name(), &intent)
            .expect("intent");

        let retained_root = parent.path().join("retained-state");
        std::fs::rename(&root, &retained_root).expect("move retained root");
        std::fs::create_dir(&root).expect("replacement root");
        std::fs::write(root.join(transaction.backup_name()), b"attacker")
            .expect("attacker artifact");

        let outcomes = store.recover().expect("safe recovery result");

        assert!(matches!(
            outcomes.as_slice(),
            [RecoveryOutcome::RolledBack { revision: Some(found) }]
                if *found == previous_revision
        ));
        assert!(!root.join(target.as_str()).exists());
        assert_eq!(
            std::fs::read(root.join(transaction.backup_name())).unwrap(),
            b"attacker"
        );
        assert_eq!(
            std::fs::read(retained_root.join(target.as_str())).unwrap(),
            b"previous"
        );
        assert!(!retained_root.join(transaction.backup_name()).exists());
    }

    fn test_store(root: &Path, point: FaultPoint) -> DurableFileStore {
        test_store_with_epoch(root, point, Uuid::new_v4())
    }

    fn test_store_with_epoch(
        root: &Path,
        point: FaultPoint,
        writer_epoch: Uuid,
    ) -> DurableFileStore {
        let directory =
            Dir::open_ambient_dir(root, cap_std::ambient_authority()).expect("open temporary root");
        DurableFileStore::new(
            directory,
            root.to_path_buf(),
            writer_epoch,
            Arc::new(OneShotFault {
                point,
                fired: AtomicBool::new(false),
            }),
        )
    }

    fn no_fault_store(root: &Path) -> DurableFileStore {
        let directory =
            Dir::open_ambient_dir(root, cap_std::ambient_authority()).expect("open temporary root");
        DurableFileStore::new(
            directory,
            root.to_path_buf(),
            Uuid::new_v4(),
            Arc::new(NoFaults),
        )
    }

    struct OneShotFault {
        point: FaultPoint,
        fired: AtomicBool,
    }

    impl FaultInjector for OneShotFault {
        fn fail_at(&self, point: FaultPoint) -> bool {
            point == self.point && !self.fired.swap(true, Ordering::SeqCst)
        }

        fn before_publish_validation(&self, root: &Path, stage: &str, target: &str) {
            if self.fired.swap(true, Ordering::SeqCst) {
                return;
            }
            match self.point {
                FaultPoint::MutateStageAfterValidation => {
                    std::fs::write(root.join(stage), b"bad").expect("rewrite stage");
                }
                FaultPoint::MutateTargetAfterValidation => {
                    std::fs::write(root.join(target), b"other").expect("create target");
                }
                _ => {
                    self.fired.store(false, Ordering::SeqCst);
                }
            }
        }

        fn after_final_publish_validation(&self, root: &Path, stage: &str, target: &str) {
            if self.fired.swap(true, Ordering::SeqCst) {
                return;
            }
            match self.point {
                FaultPoint::MutateTargetAfterFinalValidation => {
                    std::fs::write(root.join(target), b"other")
                        .expect("replace target after check");
                }
                FaultPoint::SwapStageAfterFinalValidation => {
                    std::fs::rename(root.join(stage), root.join("swapped-stage"))
                        .expect("swap retained stage name");
                    std::fs::write(root.join(stage), b"attacker")
                        .expect("install replacement stage entry");
                }
                _ => self.fired.store(false, Ordering::SeqCst),
            }
        }

        fn fail_new_file_write(&self, name: &str) -> bool {
            self.point == FaultPoint::RecoveryWriteFailure
                && !name.starts_with(TRANSACTION_PREFIX)
                && !self.fired.swap(true, Ordering::SeqCst)
        }
    }

    fn artifact_count(root: &Path, suffix: &str) -> usize {
        std::fs::read_dir(root)
            .expect("read storage root")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(suffix))
            .count()
    }
}
