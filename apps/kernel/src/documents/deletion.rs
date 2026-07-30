//! Capability-addressed workspace recycle-bin deletion.

use std::{
    collections::HashSet,
    io::{self, Read as _, Write as _},
    path::{Component, Path},
    sync::Mutex,
};

#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(any(unix, windows))]
use cap_fs_ext::OpenOptionsExt;
use cap_fs_ext::{DirExt, FollowSymlinks, MetadataExt, OpenOptionsFollowExt};
use cap_std::fs::{Dir, File, OpenOptions};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use uuid::Uuid;

use crate::{
    contract::{DeletionPolicy, DocumentKind, Revision, WorkspaceRelativePath},
    documents::{
        CapabilityMoveInstallPort, DeletionPort, DeletionPortError, DocumentDeletionTarget,
        MoveInstallPort as _, MoveInstallPortError, MoveInstallRequest, PinnedMoveSource,
    },
};

const PRIVATE_DIRECTORY: &str = ".qingyu";
const RECYCLE_DIRECTORY: &str = "recycle-bin-v1";
const MANIFEST_STAGE_PREFIX: &str = ".delete-manifest-";
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct DeletionManifest {
    transaction_id: Uuid,
    source: WorkspaceRelativePath,
    kind: DocumentKind,
    revision: Revision,
    policy: DeletionPolicy,
    payload_name: String,
}

impl DeletionManifest {
    fn new(target: &DocumentDeletionTarget, policy: DeletionPolicy) -> Self {
        let transaction_id = Uuid::new_v4();
        Self {
            transaction_id,
            source: target.path.clone(),
            kind: target.kind,
            revision: target.revision.clone(),
            policy,
            payload_name: payload_name(transaction_id, target.kind),
        }
    }

    fn validate(&self) -> Result<(), DeletionPortError> {
        if self.source.as_str().is_empty()
            || self.payload_name != payload_name(self.transaction_id, self.kind)
        {
            return Err(DeletionPortError);
        }
        Ok(())
    }

    fn manifest_name(&self) -> String {
        format!("{}.json", self.transaction_id)
    }
}

/// Kernel-owned deletion store. Recoverable entries stay in the workspace's
/// protected `.qingyu/recycle-bin-v1` directory, so moving a document never
/// crosses a filesystem or requires a platform trash API.
pub struct WorkspaceRecycleDeletionPort {
    workspace: Dir,
    recycle: Dir,
    transaction: Mutex<()>,
    #[cfg(test)]
    fail_next_post_move_sync: AtomicBool,
}

impl WorkspaceRecycleDeletionPort {
    pub fn new(workspace: Dir) -> Result<Self, DeletionPortError> {
        let private = open_or_create_child(&workspace, PRIVATE_DIRECTORY)?;
        let recycle = open_or_create_child(&private, RECYCLE_DIRECTORY)?;
        let store = Self {
            workspace,
            recycle,
            transaction: Mutex::new(()),
            #[cfg(test)]
            fail_next_post_move_sync: AtomicBool::new(false),
        };
        store.recover()?;
        Ok(store)
    }

    #[cfg(test)]
    fn fail_next_post_move_sync(&self) {
        self.fail_next_post_move_sync.store(true, Ordering::SeqCst);
    }

    fn recover(&self) -> Result<(), DeletionPortError> {
        let mut manifests = Vec::new();
        let mut observed = HashSet::new();
        for entry in self.recycle.entries().map_err(|_| DeletionPortError)? {
            let entry = entry.map_err(|_| DeletionPortError)?;
            let name = entry
                .file_name()
                .to_str()
                .map(str::to_owned)
                .ok_or(DeletionPortError)?;
            if name.starts_with(MANIFEST_STAGE_PREFIX) && name.ends_with(".tmp") {
                let metadata = self
                    .recycle
                    .symlink_metadata(&name)
                    .map_err(|_| DeletionPortError)?;
                if !trusted_private_file(&metadata) {
                    return Err(DeletionPortError);
                }
                self.recycle
                    .remove_file(&name)
                    .map_err(|_| DeletionPortError)?;
                continue;
            }
            if name.ends_with(".json") {
                let manifest = self.read_manifest(&name)?;
                if name != manifest.manifest_name() {
                    return Err(DeletionPortError);
                }
                observed.insert(name);
                observed.insert(manifest.payload_name.clone());
                manifests.push(manifest);
            }
        }
        for entry in self.recycle.entries().map_err(|_| DeletionPortError)? {
            let entry = entry.map_err(|_| DeletionPortError)?;
            let name = entry
                .file_name()
                .to_str()
                .map(str::to_owned)
                .ok_or(DeletionPortError)?;
            if !observed.contains(&name) {
                return Err(DeletionPortError);
            }
        }
        for manifest in manifests {
            self.recover_manifest(&manifest)?;
        }
        sync_directory(&self.recycle)
    }

    fn recover_manifest(&self, manifest: &DeletionManifest) -> Result<(), DeletionPortError> {
        let source_exists = self.source_exists(&manifest.source, manifest.kind)?;
        let payload_exists = self.payload_exists(&manifest.payload_name, manifest.kind)?;
        match (source_exists, payload_exists, manifest.policy) {
            (true, false, _) => self.remove_manifest(manifest),
            (false, true, DeletionPolicy::Recoverable) => Ok(()),
            (false, true, DeletionPolicy::Permanent) => self.cleanup_committed(manifest),
            (false, false, DeletionPolicy::Permanent) => self.remove_manifest(manifest),
            _ => Err(DeletionPortError),
        }
    }

    fn source_exists(
        &self,
        path: &WorkspaceRelativePath,
        kind: DocumentKind,
    ) -> Result<bool, DeletionPortError> {
        entry_at_path_exists(&self.workspace, path, kind)
    }

    fn payload_exists(&self, name: &str, kind: DocumentKind) -> Result<bool, DeletionPortError> {
        entry_exists(&self.recycle, name, kind)
    }

    fn read_manifest(&self, name: &str) -> Result<DeletionManifest, DeletionPortError> {
        let named = self
            .recycle
            .symlink_metadata(name)
            .map_err(|_| DeletionPortError)?;
        if !trusted_private_file(&named) || named.len() > MAX_MANIFEST_BYTES {
            return Err(DeletionPortError);
        }
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        let mut file = self
            .recycle
            .open_with(name, &options)
            .map_err(|_| DeletionPortError)?;
        let before = file.metadata().map_err(|_| DeletionPortError)?;
        if !trusted_private_file(&before)
            || !same_file(&named, &before)
            || before.len() > MAX_MANIFEST_BYTES
        {
            return Err(DeletionPortError);
        }
        let mut bytes = Vec::with_capacity(before.len() as usize);
        (&mut file)
            .take(MAX_MANIFEST_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| DeletionPortError)?;
        let after = file.metadata().map_err(|_| DeletionPortError)?;
        let latest = self
            .recycle
            .symlink_metadata(name)
            .map_err(|_| DeletionPortError)?;
        if bytes.len() as u64 > MAX_MANIFEST_BYTES
            || !trusted_private_file(&after)
            || !trusted_private_file(&latest)
            || !same_file(&before, &after)
            || !same_file(&after, &latest)
            || before.len() != after.len()
            || after.len() != bytes.len() as u64
            || before.modified().ok() != after.modified().ok()
        {
            return Err(DeletionPortError);
        }
        let manifest: DeletionManifest =
            serde_json::from_slice(&bytes).map_err(|_| DeletionPortError)?;
        manifest.validate()?;
        Ok(manifest)
    }

    fn write_manifest(&self, manifest: &DeletionManifest) -> Result<(), DeletionPortError> {
        manifest.validate()?;
        let name = manifest.manifest_name();
        ensure_absent(&self.recycle, &name)?;
        ensure_absent(&self.recycle, &manifest.payload_name)?;
        let bytes = serde_json::to_vec(manifest).map_err(|_| DeletionPortError)?;
        if bytes.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(DeletionPortError);
        }
        let stage_name = format!("{MANIFEST_STAGE_PREFIX}{}.tmp", manifest.transaction_id);
        let mut options = OpenOptions::new();
        options
            .read(true)
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = self
            .recycle
            .open_with(&stage_name, &options)
            .map_err(|_| DeletionPortError)?;
        if file
            .write_all(&bytes)
            .and_then(|()| file.sync_all())
            .is_err()
        {
            drop(file);
            let _cleanup = self.recycle.remove_file(&stage_name);
            return Err(DeletionPortError);
        }
        let revision = Revision::parse(format!("{:x}", Sha256::digest(&bytes)))
            .map_err(|_| DeletionPortError)?;
        let publication = CapabilityMoveInstallPort.install(MoveInstallRequest {
            source_directory: &self.recycle,
            source_name: &stage_name,
            target_directory: &self.recycle,
            target_name: &name,
            kind: DocumentKind::File,
            expected_source: PinnedMoveSource::File(&file),
            expected_revision: &revision,
        });
        if publication.is_err() {
            drop(file);
            let _cleanup = self.recycle.remove_file(&stage_name);
            return Err(DeletionPortError);
        }
        drop(file);
        sync_directory(&self.recycle)?;
        (self.read_manifest(&name)? == *manifest)
            .then_some(())
            .ok_or(DeletionPortError)
    }

    fn remove_manifest(&self, manifest: &DeletionManifest) -> Result<(), DeletionPortError> {
        match self.recycle.remove_file(manifest.manifest_name()) {
            Ok(()) => sync_directory(&self.recycle),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(DeletionPortError),
        }
    }

    fn cleanup_committed(&self, manifest: &DeletionManifest) -> Result<(), DeletionPortError> {
        match manifest.kind {
            DocumentKind::File => match self.recycle.remove_file(&manifest.payload_name) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(_) => return Err(DeletionPortError),
            },
            DocumentKind::Directory => match self.recycle.remove_dir_all(&manifest.payload_name) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(_) => return Err(DeletionPortError),
            },
        }
        sync_directory(&self.recycle)?;
        self.remove_manifest(manifest)
    }

    fn move_to_recycle(
        &self,
        target: &DocumentDeletionTarget,
        manifest: &DeletionManifest,
    ) -> Result<(), DeletionPortError> {
        let (source_parent, source_name) = open_parent(&self.workspace, &target.path)?;
        let pinned = PinnedSource::open(&source_parent, &source_name, target.kind)?;
        match CapabilityMoveInstallPort.install(MoveInstallRequest {
            source_directory: &source_parent,
            source_name: &source_name,
            target_directory: &self.recycle,
            target_name: &manifest.payload_name,
            kind: target.kind,
            expected_source: pinned.as_move_source(),
            expected_revision: &target.revision,
        }) {
            Ok(()) => {}
            Err(
                MoveInstallPortError::AlreadyExists
                | MoveInstallPortError::RevisionConflict(_)
                | MoveInstallPortError::UnavailableNoMutation,
            ) => {
                self.remove_manifest(manifest)?;
                return Err(DeletionPortError);
            }
            Err(MoveInstallPortError::RecoveryRequired) => {
                let source_exists = entry_exists(&source_parent, &source_name, target.kind)?;
                let payload_exists =
                    entry_exists(&self.recycle, &manifest.payload_name, target.kind)?;
                if source_exists || !payload_exists {
                    return Err(DeletionPortError);
                }
            }
        }
        #[cfg(test)]
        if self.fail_next_post_move_sync.swap(false, Ordering::SeqCst) {
            return Err(DeletionPortError);
        }
        sync_directory(&source_parent)?;
        sync_directory(&self.recycle)?;
        Ok(())
    }
}

impl DeletionPort for WorkspaceRecycleDeletionPort {
    fn delete(
        &self,
        target: &DocumentDeletionTarget,
        policy: DeletionPolicy,
    ) -> Result<(), DeletionPortError> {
        let _transaction = self.transaction.lock().map_err(|_| DeletionPortError)?;
        let manifest = DeletionManifest::new(target, policy);
        self.write_manifest(&manifest)?;
        self.move_to_recycle(target, &manifest)?;
        if policy == DeletionPolicy::Permanent {
            // The workspace mutation is already committed. A cleanup failure
            // leaves a private tombstone for the next startup instead of
            // misreporting the delete as an uncommitted failure.
            let _cleanup = self.cleanup_committed(&manifest);
        }
        Ok(())
    }
}

enum PinnedSource {
    File(File),
    Directory(Dir),
}

impl PinnedSource {
    fn open(directory: &Dir, name: &str, kind: DocumentKind) -> Result<Self, DeletionPortError> {
        let named = directory
            .symlink_metadata(name)
            .map_err(|_| DeletionPortError)?;
        if named.file_type().is_symlink() {
            return Err(DeletionPortError);
        }
        match kind {
            DocumentKind::File if named.is_file() => {
                let mut options = OpenOptions::new();
                options.read(true).follow(FollowSymlinks::No);
                directory
                    .open_with(name, &options)
                    .map(Self::File)
                    .map_err(|_| DeletionPortError)
            }
            DocumentKind::Directory if named.is_dir() => directory
                .open_dir_nofollow(name)
                .map(Self::Directory)
                .map_err(|_| DeletionPortError),
            _ => Err(DeletionPortError),
        }
    }

    fn as_move_source(&self) -> PinnedMoveSource<'_> {
        match self {
            Self::File(file) => PinnedMoveSource::File(file),
            Self::Directory(directory) => PinnedMoveSource::Directory(directory),
        }
    }
}

fn open_or_create_child(parent: &Dir, name: &str) -> Result<Dir, DeletionPortError> {
    match parent.symlink_metadata(name) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(DeletionPortError);
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if let Err(create_error) = parent.create_dir(name) {
                if create_error.kind() != io::ErrorKind::AlreadyExists {
                    return Err(DeletionPortError);
                }
            }
        }
        Err(_) => return Err(DeletionPortError),
    }
    let metadata = parent
        .symlink_metadata(name)
        .map_err(|_| DeletionPortError)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(DeletionPortError);
    }
    parent
        .open_dir_nofollow(name)
        .map_err(|_| DeletionPortError)
}

fn open_parent(
    root: &Dir,
    path: &WorkspaceRelativePath,
) -> Result<(Dir, String), DeletionPortError> {
    if path.as_str().is_empty() {
        return Err(DeletionPortError);
    }
    let path = Path::new(path.as_str());
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .map(str::to_owned)
        .ok_or(DeletionPortError)?;
    let mut parent = root.try_clone().map_err(|_| DeletionPortError)?;
    for component in path.parent().unwrap_or_else(|| Path::new("")).components() {
        let Component::Normal(segment) = component else {
            return Err(DeletionPortError);
        };
        parent = parent
            .open_dir_nofollow(segment)
            .map_err(|_| DeletionPortError)?;
    }
    Ok((parent, name))
}

fn entry_at_path_exists(
    root: &Dir,
    path: &WorkspaceRelativePath,
    kind: DocumentKind,
) -> Result<bool, DeletionPortError> {
    if path.as_str().is_empty() {
        return Err(DeletionPortError);
    }
    let path = Path::new(path.as_str());
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or(DeletionPortError)?;
    let mut parent = root.try_clone().map_err(|_| DeletionPortError)?;
    for component in path.parent().unwrap_or_else(|| Path::new("")).components() {
        let Component::Normal(segment) = component else {
            return Err(DeletionPortError);
        };
        parent = match parent.open_dir_nofollow(segment) {
            Ok(directory) => directory,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(_) => return Err(DeletionPortError),
        };
    }
    entry_exists(&parent, name, kind)
}

fn entry_exists(
    directory: &Dir,
    name: &str,
    kind: DocumentKind,
) -> Result<bool, DeletionPortError> {
    match directory.symlink_metadata(name) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink()
                || (kind == DocumentKind::File && !metadata.is_file())
                || (kind == DocumentKind::Directory && !metadata.is_dir())
            {
                return Err(DeletionPortError);
            }
            Ok(true)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(DeletionPortError),
    }
}

fn ensure_absent(directory: &Dir, name: &str) -> Result<(), DeletionPortError> {
    match directory.symlink_metadata(name) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        _ => Err(DeletionPortError),
    }
}

fn payload_name(transaction_id: Uuid, kind: DocumentKind) -> String {
    let suffix = match kind {
        DocumentKind::File => "file",
        DocumentKind::Directory => "directory",
    };
    format!("{transaction_id}.{suffix}")
}

fn trusted_private_file(metadata: &cap_std::fs::Metadata) -> bool {
    metadata.is_file() && !metadata.file_type().is_symlink() && private_link_count(metadata) == 1
}

fn same_file(left: &cap_std::fs::Metadata, right: &cap_std::fs::Metadata) -> bool {
    MetadataExt::dev(left) == MetadataExt::dev(right)
        && MetadataExt::ino(left) == MetadataExt::ino(right)
}

#[cfg(unix)]
fn private_link_count(metadata: &cap_std::fs::Metadata) -> u64 {
    MetadataExt::nlink(metadata)
}

#[cfg(windows)]
fn private_link_count(metadata: &cap_std::fs::Metadata) -> u64 {
    use cap_std::fs::MetadataExt as _;
    metadata.number_of_links().unwrap_or(0)
}

#[cfg(not(any(unix, windows)))]
fn private_link_count(_metadata: &cap_std::fs::Metadata) -> u64 {
    1
}

#[cfg(unix)]
fn sync_directory(directory: &Dir) -> Result<(), DeletionPortError> {
    crate::storage::sync_directory(directory).map_err(|_| DeletionPortError)
}

#[cfg(windows)]
fn sync_directory(directory: &Dir) -> Result<(), DeletionPortError> {
    use std::os::windows::io::AsRawHandle as _;

    use windows_sys::Win32::{
        Foundation::GENERIC_WRITE,
        Storage::FileSystem::{FlushFileBuffers, FILE_FLAG_BACKUP_SEMANTICS},
    };

    // cap-std retains directory handles with read access. Reopen the same
    // directory relative to that capability because FlushFileBuffers requires
    // a write-capable handle, and directories require BACKUP_SEMANTICS.
    let mut options = OpenOptions::new();
    options
        .access_mode(GENERIC_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .follow(FollowSymlinks::No);
    let writable = directory
        .open_with(".", &options)
        .map_err(|_| DeletionPortError)?;
    let flushed = unsafe { FlushFileBuffers(writable.as_raw_handle()) };
    (flushed != 0).then_some(()).ok_or(DeletionPortError)
}

#[cfg(not(any(unix, windows)))]
fn sync_directory(_directory: &Dir) -> Result<(), DeletionPortError> {
    Err(DeletionPortError)
}

#[cfg(test)]
mod tests {
    use sha2::{Digest as _, Sha256};

    use super::*;

    fn fixture() -> (tempfile::TempDir, WorkspaceRecycleDeletionPort) {
        let root = tempfile::tempdir().unwrap();
        let workspace = Dir::open_ambient_dir(root.path(), cap_std::ambient_authority()).unwrap();
        let store = WorkspaceRecycleDeletionPort::new(workspace).unwrap();
        (root, store)
    }

    fn target(path: &str, contents: &[u8]) -> DocumentDeletionTarget {
        DocumentDeletionTarget {
            path: WorkspaceRelativePath::parse(path).unwrap(),
            kind: DocumentKind::File,
            revision: Revision::parse(format!("{:x}", Sha256::digest(contents))).unwrap(),
        }
    }

    #[test]
    fn recoverable_delete_moves_the_payload_into_the_private_recycle_bin() {
        let (root, store) = fixture();
        std::fs::write(root.path().join("note.md"), b"contents").unwrap();

        store
            .delete(&target("note.md", b"contents"), DeletionPolicy::Recoverable)
            .unwrap();

        assert!(!root.path().join("note.md").exists());
        assert_eq!(
            std::fs::read_dir(root.path().join(".qingyu/recycle-bin-v1"))
                .unwrap()
                .count(),
            2
        );
    }

    #[test]
    fn permanent_delete_removes_the_private_tombstone() {
        let (root, store) = fixture();
        std::fs::write(root.path().join("note.md"), b"contents").unwrap();

        store
            .delete(&target("note.md", b"contents"), DeletionPolicy::Permanent)
            .unwrap();

        assert!(!root.path().join("note.md").exists());
        assert_eq!(
            std::fs::read_dir(root.path().join(".qingyu/recycle-bin-v1"))
                .unwrap()
                .count(),
            0
        );
    }

    #[test]
    fn reconstruction_finishes_a_committed_permanent_tombstone() {
        let (root, store) = fixture();
        std::fs::write(root.path().join("note.md"), b"contents").unwrap();
        let target = target("note.md", b"contents");
        let manifest = DeletionManifest::new(&target, DeletionPolicy::Permanent);
        store.write_manifest(&manifest).unwrap();
        store.move_to_recycle(&target, &manifest).unwrap();
        drop(store);

        let workspace = Dir::open_ambient_dir(root.path(), cap_std::ambient_authority()).unwrap();
        WorkspaceRecycleDeletionPort::new(workspace).unwrap();

        assert!(!root.path().join("note.md").exists());
        assert_eq!(
            std::fs::read_dir(root.path().join(".qingyu/recycle-bin-v1"))
                .unwrap()
                .count(),
            0
        );
    }

    #[test]
    fn reconstruction_discards_a_prepublication_manifest() {
        let (root, store) = fixture();
        std::fs::write(root.path().join("note.md"), b"contents").unwrap();
        let target = target("note.md", b"contents");
        let manifest = DeletionManifest::new(&target, DeletionPolicy::Recoverable);
        store.write_manifest(&manifest).unwrap();
        drop(store);

        let workspace = Dir::open_ambient_dir(root.path(), cap_std::ambient_authority()).unwrap();
        WorkspaceRecycleDeletionPort::new(workspace).unwrap();

        assert_eq!(
            std::fs::read(root.path().join("note.md")).unwrap(),
            b"contents"
        );
        assert_eq!(
            std::fs::read_dir(root.path().join(".qingyu/recycle-bin-v1"))
                .unwrap()
                .count(),
            0
        );
    }

    #[test]
    fn revision_mismatch_leaves_the_source_unchanged() {
        let (root, store) = fixture();
        std::fs::write(root.path().join("note.md"), b"replacement").unwrap();

        assert!(store
            .delete(&target("note.md", b"expected"), DeletionPolicy::Permanent)
            .is_err());

        assert_eq!(
            std::fs::read(root.path().join("note.md")).unwrap(),
            b"replacement"
        );
        assert_eq!(
            std::fs::read_dir(root.path().join(".qingyu/recycle-bin-v1"))
                .unwrap()
                .count(),
            0
        );
    }

    #[test]
    fn post_move_sync_failure_never_reports_delete_success_and_remains_recoverable() {
        let (root, store) = fixture();
        std::fs::write(root.path().join("note.md"), b"contents").unwrap();
        store.fail_next_post_move_sync();

        assert!(store
            .delete(&target("note.md", b"contents"), DeletionPolicy::Recoverable,)
            .is_err());
        assert!(!root.path().join("note.md").exists());
        drop(store);

        let workspace = Dir::open_ambient_dir(root.path(), cap_std::ambient_authority()).unwrap();
        WorkspaceRecycleDeletionPort::new(workspace).unwrap();
        assert_eq!(
            std::fs::read_dir(root.path().join(".qingyu/recycle-bin-v1"))
                .unwrap()
                .count(),
            2
        );
    }

    #[test]
    fn recoverable_entries_survive_store_reconstruction() {
        let (root, store) = fixture();
        std::fs::write(root.path().join("note.md"), b"contents").unwrap();
        store
            .delete(&target("note.md", b"contents"), DeletionPolicy::Recoverable)
            .unwrap();
        drop(store);

        let workspace = Dir::open_ambient_dir(root.path(), cap_std::ambient_authority()).unwrap();
        WorkspaceRecycleDeletionPort::new(workspace).unwrap();

        assert_eq!(
            std::fs::read_dir(root.path().join(".qingyu/recycle-bin-v1"))
                .unwrap()
                .count(),
            2
        );
    }

    #[test]
    fn recoverable_entry_survives_when_its_original_parent_is_later_removed() {
        let (root, store) = fixture();
        std::fs::create_dir(root.path().join("folder")).unwrap();
        std::fs::write(root.path().join("folder/note.md"), b"contents").unwrap();
        store
            .delete(
                &target("folder/note.md", b"contents"),
                DeletionPolicy::Recoverable,
            )
            .unwrap();
        std::fs::remove_dir(root.path().join("folder")).unwrap();
        drop(store);

        let workspace = Dir::open_ambient_dir(root.path(), cap_std::ambient_authority()).unwrap();
        WorkspaceRecycleDeletionPort::new(workspace).unwrap();

        assert_eq!(
            std::fs::read_dir(root.path().join(".qingyu/recycle-bin-v1"))
                .unwrap()
                .count(),
            2
        );
    }

    #[cfg(unix)]
    #[test]
    fn private_directory_symlink_is_rejected_without_touching_its_target() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), root.path().join(".qingyu")).unwrap();
        let workspace = Dir::open_ambient_dir(root.path(), cap_std::ambient_authority()).unwrap();

        assert!(WorkspaceRecycleDeletionPort::new(workspace).is_err());
        assert!(std::fs::read_dir(outside.path()).unwrap().next().is_none());
    }
}
