//! Stable, capability-scoped building blocks for fail-closed inventory snapshots.
//!
//! The resource service integrates these foundations in a later change. Keeping
//! this module separate lets that integration bind cursors to strong platform
//! change tokens without teaching the wire contract about host filesystem data.

#![allow(dead_code)]

use std::{
    collections::HashMap,
    ffi::OsStr,
    fmt, io,
    num::NonZeroUsize,
    path::{Component, Path},
    sync::{Mutex, MutexGuard},
    time::{SystemTime, UNIX_EPOCH},
};

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
#[cfg(windows)]
use cap_primitives::fs::_WindowsByHandle as _;
use cap_std::fs::{Dir, File, Metadata, OpenOptions};
use serde::{Serialize, Serializer};

use crate::contract::WorkspaceRelativePath;

const INVENTORY_SNAPSHOT_FORMAT_VERSION: u8 = 2;

/// A version observation for one retained regular file.
///
/// Callers may use [`Self::Strong`] in a cursor collection snapshot. A
/// [`Self::RequiresContentHash`] observation is deliberately not sufficient:
/// the caller must hash the retained bytes and use a content digest instead.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum FileVersionStamp {
    Strong(StrongFileVersionStamp),
    RequiresContentHash(FileVersionFallback),
}

impl FileVersionStamp {
    pub(crate) fn strong(&self) -> Option<&StrongFileVersionStamp> {
        match self {
            Self::Strong(stamp) => Some(stamp),
            Self::RequiresContentHash(_) => None,
        }
    }

    pub(crate) const fn requires_content_hash() -> Self {
        Self::RequiresContentHash(FileVersionFallback::PlatformChangeTokenUnavailable)
    }

    /// Captures a nofollow metadata-only observation for inventory phase A.
    ///
    /// This never opens or reads file content. Platforms without a proven
    /// metadata change token return [`Self::RequiresContentHash`]. Callers must
    /// then hash every fallback candidate included in the collection snapshot,
    /// under one explicit byte budget, before issuing a cursor.
    #[cfg(unix)]
    pub(crate) fn capture_metadata(metadata: &Metadata) -> Self {
        Self::Strong(StrongFileVersionStamp::Unix {
            device: cap_fs_ext::MetadataExt::dev(metadata),
            inode: cap_fs_ext::MetadataExt::ino(metadata),
            link_count: cap_fs_ext::MetadataExt::nlink(metadata),
            length: metadata.len(),
            modified_seconds: cap_std::fs::MetadataExt::mtime(metadata),
            modified_nanoseconds: cap_std::fs::MetadataExt::mtime_nsec(metadata),
            changed_seconds: cap_std::fs::MetadataExt::ctime(metadata),
            changed_nanoseconds: cap_std::fs::MetadataExt::ctime_nsec(metadata),
        })
    }

    #[cfg(not(unix))]
    pub(crate) const fn capture_metadata(_metadata: &Metadata) -> Self {
        // Windows needs a full FILE_ID_INFO plus FILE_BASIC_INFO::ChangeTime
        // observation from the retained handle. Until that exact query is
        // implemented and tested on Windows, content hashing is mandatory.
        Self::requires_content_hash()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FileVersionFallback {
    PlatformChangeTokenUnavailable,
}

/// A change token that is safe to memoize for the lifetime of the process.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(tag = "platform", rename_all = "kebab-case")]
pub(crate) enum StrongFileVersionStamp {
    Unix {
        device: u64,
        inode: u64,
        #[serde(rename = "linkCount")]
        link_count: u64,
        length: u64,
        #[serde(rename = "modifiedSeconds")]
        modified_seconds: i64,
        #[serde(rename = "modifiedNanoseconds")]
        modified_nanoseconds: i64,
        #[serde(rename = "changedSeconds")]
        changed_seconds: i64,
        #[serde(rename = "changedNanoseconds")]
        changed_nanoseconds: i64,
    },
}

/// A retained nofollow file capability paired with one stable version stamp.
pub(crate) struct RetainedInventoryFile {
    file: File,
    metadata: Metadata,
    stamp: FileVersionStamp,
}

impl RetainedInventoryFile {
    pub(crate) fn open_nofollow(parent: &Dir, name: impl AsRef<Path>) -> io::Result<Self> {
        let name = name.as_ref();
        require_single_component(name)?;
        let addressed = parent.symlink_metadata(name)?;
        require_trusted_regular_file(&addressed)?;

        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        let file = parent.open_with(name, &options)?;
        let retained = file.metadata()?;
        require_trusted_regular_file(&retained)?;
        require_same_file(&addressed, &retained)?;
        let stamp = FileVersionStamp::capture_metadata(&retained);

        let latest = file.metadata()?;
        let named = parent.symlink_metadata(name)?;
        require_trusted_regular_file(&latest)?;
        require_trusted_regular_file(&named)?;
        require_same_file(&retained, &latest)?;
        require_same_file(&retained, &named)?;
        if retained.len() != latest.len()
            || retained.len() != named.len()
            || retained.modified().ok() != latest.modified().ok()
            || retained.modified().ok() != named.modified().ok()
            || FileVersionStamp::capture_metadata(&latest) != stamp
        {
            return Err(unsafe_file());
        }

        Ok(Self {
            file,
            metadata: latest,
            stamp,
        })
    }

    pub(crate) const fn stamp(&self) -> &FileVersionStamp {
        &self.stamp
    }

    pub(crate) const fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    pub(crate) const fn file(&self) -> &File {
        &self.file
    }

    pub(crate) fn file_mut(&mut self) -> &mut File {
        &mut self.file
    }

    pub(crate) fn into_file(self) -> File {
        self.file
    }
}

impl fmt::Debug for RetainedInventoryFile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RetainedInventoryFile { capability: held }")
    }
}

fn require_single_component(path: &Path) -> io::Result<()> {
    let mut components = path.components();
    if matches!(components.next(), Some(Component::Normal(name)) if name != OsStr::new("."))
        && components.next().is_none()
    {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "inventory file name must be one component",
        ))
    }
}

fn require_trusted_regular_file(metadata: &Metadata) -> io::Result<()> {
    if metadata.is_file() && !metadata.file_type().is_symlink() && link_count(metadata) == 1 {
        Ok(())
    } else {
        Err(unsafe_file())
    }
}

#[cfg(unix)]
fn link_count(metadata: &Metadata) -> u64 {
    cap_fs_ext::MetadataExt::nlink(metadata)
}

#[cfg(windows)]
fn link_count(metadata: &Metadata) -> u64 {
    metadata.number_of_links().map_or(0, u64::from)
}

#[cfg(not(any(unix, windows)))]
fn link_count(_metadata: &Metadata) -> u64 {
    1
}

#[cfg(unix)]
fn require_same_file(left: &Metadata, right: &Metadata) -> io::Result<()> {
    if cap_fs_ext::MetadataExt::dev(left) == cap_fs_ext::MetadataExt::dev(right)
        && cap_fs_ext::MetadataExt::ino(left) == cap_fs_ext::MetadataExt::ino(right)
    {
        Ok(())
    } else {
        Err(unsafe_file())
    }
}

#[cfg(windows)]
fn require_same_file(left: &Metadata, right: &Metadata) -> io::Result<()> {
    let identities = (
        left.volume_serial_number(),
        left.file_index(),
        right.volume_serial_number(),
        right.file_index(),
    );
    match identities {
        (Some(left_device), Some(left_file), Some(right_device), Some(right_file))
            if left_device == right_device && left_file == right_file =>
        {
            Ok(())
        }
        _ => Err(unsafe_file()),
    }
}

#[cfg(not(any(unix, windows)))]
fn require_same_file(_left: &Metadata, _right: &Metadata) -> io::Result<()> {
    Err(unsafe_file())
}

fn unsafe_file() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "unsafe inventory file")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum InventoryCandidateType {
    DocumentFile,
    Directory,
    ResourceFile,
}

/// Stable cursor input for one logical inventory candidate.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InventoryCandidateSnapshot {
    format_version: u8,
    logical_path: WorkspaceRelativePath,
    entry_type: InventoryCandidateType,
    version: InventoryCandidateVersion,
}

impl InventoryCandidateSnapshot {
    pub(crate) const fn logical_path(&self) -> &WorkspaceRelativePath {
        &self.logical_path
    }

    pub(crate) fn from_file_stamp(
        logical_path: WorkspaceRelativePath,
        entry_type: InventoryCandidateType,
        stamp: FileVersionStamp,
    ) -> Result<Self, ContentHashRequired> {
        match stamp {
            FileVersionStamp::Strong(stamp) => Ok(Self {
                format_version: INVENTORY_SNAPSHOT_FORMAT_VERSION,
                logical_path,
                entry_type,
                version: InventoryCandidateVersion::StrongFileStamp { stamp },
            }),
            FileVersionStamp::RequiresContentHash(_) => Err(ContentHashRequired),
        }
    }

    pub(crate) const fn from_content_digest(
        logical_path: WorkspaceRelativePath,
        entry_type: InventoryCandidateType,
        digest: ContentDigest,
        modified_at: InventoryModifiedTime,
    ) -> Self {
        Self {
            format_version: INVENTORY_SNAPSHOT_FORMAT_VERSION,
            logical_path,
            entry_type,
            version: InventoryCandidateVersion::ContentSha256 {
                digest,
                modified_at,
            },
        }
    }

    pub(crate) const fn from_tree_digest(
        logical_path: WorkspaceRelativePath,
        digest: ContentDigest,
        modified_at: InventoryModifiedTime,
    ) -> Self {
        Self {
            format_version: INVENTORY_SNAPSHOT_FORMAT_VERSION,
            logical_path,
            entry_type: InventoryCandidateType::Directory,
            version: InventoryCandidateVersion::TreeSha256 {
                digest,
                modified_at,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InventoryModifiedTime {
    seconds: i64,
    nanoseconds: u32,
}

impl InventoryModifiedTime {
    pub(crate) fn capture(metadata: &Metadata) -> io::Result<Self> {
        Self::from_system_time(metadata.modified()?.into_std())
    }

    fn from_system_time(value: SystemTime) -> io::Result<Self> {
        match value.duration_since(UNIX_EPOCH) {
            Ok(duration) => Ok(Self {
                seconds: i64::try_from(duration.as_secs()).map_err(|_| invalid_modified_time())?,
                nanoseconds: duration.subsec_nanos(),
            }),
            Err(error) => {
                let duration = error.duration();
                let magnitude =
                    i64::try_from(duration.as_secs()).map_err(|_| invalid_modified_time())?;
                if duration.subsec_nanos() == 0 {
                    Ok(Self {
                        seconds: magnitude.checked_neg().ok_or_else(invalid_modified_time)?,
                        nanoseconds: 0,
                    })
                } else {
                    Ok(Self {
                        seconds: magnitude
                            .checked_neg()
                            .and_then(|seconds| seconds.checked_sub(1))
                            .ok_or_else(invalid_modified_time)?,
                        nanoseconds: 1_000_000_000 - duration.subsec_nanos(),
                    })
                }
            }
        }
    }
}

fn invalid_modified_time() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "inventory modified time is out of range",
    )
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum InventoryCandidateVersion {
    StrongFileStamp {
        stamp: StrongFileVersionStamp,
    },
    ContentSha256 {
        digest: ContentDigest,
        #[serde(rename = "modifiedAt")]
        modified_at: InventoryModifiedTime,
    },
    TreeSha256 {
        digest: ContentDigest,
        #[serde(rename = "modifiedAt")]
        modified_at: InventoryModifiedTime,
    },
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct ContentDigest([u8; 32]);

impl ContentDigest {
    pub(crate) const fn new(value: [u8; 32]) -> Self {
        Self(value)
    }

    pub(crate) const fn into_bytes(self) -> [u8; 32] {
        self.0
    }
}

impl Serialize for ContentDigest {
    fn serialize<SerializerType>(
        &self,
        serializer: SerializerType,
    ) -> Result<SerializerType::Ok, SerializerType::Error>
    where
        SerializerType: Serializer,
    {
        let mut encoded = String::with_capacity(64);
        for byte in self.0 {
            use fmt::Write as _;
            write!(&mut encoded, "{byte:02x}").map_err(serde::ser::Error::custom)?;
        }
        serializer.serialize_str(&encoded)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ContentHashRequired;

impl fmt::Display for ContentHashRequired {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("inventory content hash required")
    }
}

impl std::error::Error for ContentHashRequired {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct InventorySnapshotLimits {
    pub(crate) maximum_nodes: u64,
    pub(crate) maximum_content_bytes: u64,
    pub(crate) maximum_metadata_bytes: u64,
    pub(crate) maximum_depth: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct InventorySnapshotBudget {
    remaining_nodes: u64,
    remaining_content_bytes: u64,
    remaining_metadata_bytes: u64,
    maximum_depth: usize,
}

impl InventorySnapshotBudget {
    pub(crate) const fn new(limits: InventorySnapshotLimits) -> Self {
        Self {
            remaining_nodes: limits.maximum_nodes,
            remaining_content_bytes: limits.maximum_content_bytes,
            remaining_metadata_bytes: limits.maximum_metadata_bytes,
            maximum_depth: limits.maximum_depth,
        }
    }

    pub(crate) fn charge_node(&mut self) -> Result<(), InventorySnapshotBudgetError> {
        self.charge_nodes(1)
    }

    pub(crate) fn charge_nodes(&mut self, nodes: u64) -> Result<(), InventorySnapshotBudgetError> {
        let Some(remaining) = self.remaining_nodes.checked_sub(nodes) else {
            return Err(InventorySnapshotBudgetError::NodeLimit);
        };
        self.remaining_nodes = remaining;
        Ok(())
    }

    pub(crate) fn charge_content_bytes(
        &mut self,
        bytes: u64,
    ) -> Result<(), InventorySnapshotBudgetError> {
        let Some(remaining) = self.remaining_content_bytes.checked_sub(bytes) else {
            return Err(InventorySnapshotBudgetError::ContentBytes);
        };
        self.remaining_content_bytes = remaining;
        Ok(())
    }

    pub(crate) fn charge_metadata_bytes(
        &mut self,
        bytes: u64,
    ) -> Result<(), InventorySnapshotBudgetError> {
        let Some(remaining) = self.remaining_metadata_bytes.checked_sub(bytes) else {
            return Err(InventorySnapshotBudgetError::MetadataBytes);
        };
        self.remaining_metadata_bytes = remaining;
        Ok(())
    }

    pub(crate) fn require_depth(&self, depth: usize) -> Result<(), InventorySnapshotBudgetError> {
        if depth <= self.maximum_depth {
            Ok(())
        } else {
            Err(InventorySnapshotBudgetError::Depth)
        }
    }

    pub(crate) const fn remaining_nodes(self) -> u64 {
        self.remaining_nodes
    }

    pub(crate) const fn remaining_content_bytes(self) -> u64 {
        self.remaining_content_bytes
    }

    pub(crate) const fn remaining_metadata_bytes(self) -> u64 {
        self.remaining_metadata_bytes
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InventorySnapshotBudgetError {
    NodeLimit,
    ContentBytes,
    MetadataBytes,
    Depth,
}

impl fmt::Display for InventorySnapshotBudgetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NodeLimit => formatter.write_str("inventory snapshot node limit exceeded"),
            Self::ContentBytes => {
                formatter.write_str("inventory snapshot content byte limit exceeded")
            }
            Self::MetadataBytes => {
                formatter.write_str("inventory snapshot metadata byte limit exceeded")
            }
            Self::Depth => formatter.write_str("inventory snapshot depth limit exceeded"),
        }
    }
}

impl std::error::Error for InventorySnapshotBudgetError {}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct ContentRevisionCacheKey {
    logical_path: WorkspaceRelativePath,
    stamp: StrongFileVersionStamp,
}

impl ContentRevisionCacheKey {
    pub(crate) const fn new(
        logical_path: WorkspaceRelativePath,
        stamp: StrongFileVersionStamp,
    ) -> Self {
        Self {
            logical_path,
            stamp,
        }
    }
}

/// A bounded, process-local LRU for content-derived inventory values.
///
/// Poisoning discards the optimization state. Correctness therefore degrades
/// to a cache miss rather than trusting entries written across an unwind.
pub(crate) struct ContentRevisionCache<Value> {
    capacity: NonZeroUsize,
    state: Mutex<ContentRevisionCacheState<Value>>,
}

struct ContentRevisionCacheState<Value> {
    clock: u64,
    entries: HashMap<ContentRevisionCacheKey, ContentRevisionCacheEntry<Value>>,
}

struct ContentRevisionCacheEntry<Value> {
    value: Value,
    last_used: u64,
}

impl<Value: Clone> ContentRevisionCache<Value> {
    pub(crate) fn new(capacity: NonZeroUsize) -> Self {
        Self {
            capacity,
            state: Mutex::new(ContentRevisionCacheState {
                clock: 0,
                entries: HashMap::new(),
            }),
        }
    }

    pub(crate) fn get(&self, key: &ContentRevisionCacheKey) -> Option<Value> {
        let mut state = self.lock_state();
        let tick = next_tick(&mut state);
        let entry = state.entries.get_mut(key)?;
        entry.last_used = tick;
        Some(entry.value.clone())
    }

    pub(crate) fn insert(&self, key: ContentRevisionCacheKey, value: Value) {
        let mut state = self.lock_state();
        let tick = next_tick(&mut state);
        if let Some(entry) = state.entries.get_mut(&key) {
            entry.value = value;
            entry.last_used = tick;
            return;
        }
        if state.entries.len() == self.capacity.get() {
            let least_recent = state
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(key, _)| key.clone());
            if let Some(least_recent) = least_recent {
                state.entries.remove(&least_recent);
            }
        }
        state.entries.insert(
            key,
            ContentRevisionCacheEntry {
                value,
                last_used: tick,
            },
        );
    }

    pub(crate) fn len(&self) -> usize {
        self.lock_state().entries.len()
    }

    fn lock_state(&self) -> MutexGuard<'_, ContentRevisionCacheState<Value>> {
        match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => {
                let mut state = poisoned.into_inner();
                state.entries.clear();
                state.clock = 0;
                self.state.clear_poison();
                state
            }
        }
    }
}

fn next_tick<Value>(state: &mut ContentRevisionCacheState<Value>) -> u64 {
    if state.clock == u64::MAX {
        state.entries.clear();
        state.clock = 0;
    }
    state.clock += 1;
    state.clock
}

#[cfg(test)]
mod tests {
    use std::{fs, num::NonZeroUsize, thread, time::Duration};

    use cap_std::{ambient_authority, fs::Dir};
    use tempfile::tempdir;

    use crate::contract::WorkspaceRelativePath;

    use super::{
        ContentDigest, ContentRevisionCache, ContentRevisionCacheKey, FileVersionStamp,
        InventoryCandidateSnapshot, InventoryCandidateType, InventoryModifiedTime,
        InventorySnapshotBudget, InventorySnapshotBudgetError, InventorySnapshotLimits,
        RetainedInventoryFile, StrongFileVersionStamp,
    };

    #[cfg(unix)]
    #[test]
    fn same_inode_rewrite_with_restored_length_and_mtime_changes_the_strong_stamp() {
        let temporary = tempdir().unwrap();
        let path = temporary.path().join("resource.bin");
        fs::write(&path, b"first").unwrap();
        let original_modified = fs::metadata(&path).unwrap().modified().unwrap();
        let directory = Dir::open_ambient_dir(temporary.path(), ambient_authority()).unwrap();
        let original = RetainedInventoryFile::open_nofollow(&directory, "resource.bin").unwrap();
        let original_stamp = original.stamp().strong().unwrap().clone();
        drop(original);

        thread::sleep(Duration::from_millis(2));
        fs::write(&path, b"other").unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(original_modified)
            .unwrap();

        let rewritten = RetainedInventoryFile::open_nofollow(&directory, "resource.bin").unwrap();
        assert_ne!(rewritten.stamp().strong(), Some(&original_stamp));
    }

    #[cfg(unix)]
    #[test]
    fn metadata_stamp_capture_does_not_require_opening_file_content() {
        use std::os::unix::fs::PermissionsExt as _;

        let temporary = tempdir().unwrap();
        let path = temporary.path().join("unreadable.bin");
        fs::write(&path, b"content that phase a must not read").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
        let directory = Dir::open_ambient_dir(temporary.path(), ambient_authority()).unwrap();
        let metadata = directory.symlink_metadata("unreadable.bin").unwrap();

        let stamp = FileVersionStamp::capture_metadata(&metadata);

        assert!(stamp.strong().is_some());
    }

    #[test]
    fn candidate_snapshot_serialization_is_byte_stable() {
        let path = WorkspaceRelativePath::parse("assets/photo.png").unwrap();
        let digest = ContentDigest::new([0xabu8; 32]);
        let snapshot = InventoryCandidateSnapshot::from_content_digest(
            path,
            InventoryCandidateType::ResourceFile,
            digest,
            InventoryModifiedTime {
                seconds: 1,
                nanoseconds: 2,
            },
        );

        assert_eq!(snapshot.logical_path().as_str(), "assets/photo.png");
        let first = serde_json::to_vec(&snapshot).unwrap();
        let second = serde_json::to_vec(&snapshot).unwrap();

        assert_eq!(first, second);
        assert_eq!(
            String::from_utf8(first).unwrap(),
            concat!(
                r#"{"formatVersion":2,"logicalPath":"assets/photo.png","entryType":"resource-file","version":{"kind":"content-sha256","digest":""#,
                "abababababababababababababababababababababababababababababababab",
                r#"","modifiedAt":{"seconds":1,"nanoseconds":2}}}"#,
            )
        );
    }

    #[test]
    fn unsupported_stamp_requires_a_content_digest_for_a_candidate_snapshot() {
        let path = WorkspaceRelativePath::parse("assets/file.bin").unwrap();
        let error = InventoryCandidateSnapshot::from_file_stamp(
            path,
            InventoryCandidateType::ResourceFile,
            FileVersionStamp::requires_content_hash(),
        )
        .unwrap_err();

        assert_eq!(error.to_string(), "inventory content hash required");
    }

    #[test]
    fn fallback_snapshots_include_modified_time_in_cursor_identity() {
        let path = WorkspaceRelativePath::parse("assets/file.bin").unwrap();
        let digest = ContentDigest::new([0x11; 32]);
        let first_modified = InventoryModifiedTime {
            seconds: 1,
            nanoseconds: 0,
        };
        let second_modified = InventoryModifiedTime {
            seconds: 2,
            nanoseconds: 0,
        };

        let first_file = InventoryCandidateSnapshot::from_content_digest(
            path.clone(),
            InventoryCandidateType::ResourceFile,
            digest,
            first_modified,
        );
        let second_file = InventoryCandidateSnapshot::from_content_digest(
            path.clone(),
            InventoryCandidateType::ResourceFile,
            digest,
            second_modified,
        );
        let first_tree =
            InventoryCandidateSnapshot::from_tree_digest(path.clone(), digest, first_modified);
        let second_tree =
            InventoryCandidateSnapshot::from_tree_digest(path, digest, second_modified);

        assert_ne!(first_file, second_file);
        assert_ne!(first_tree, second_tree);
    }

    #[test]
    fn budget_rejection_preserves_the_remaining_allowance() {
        let mut budget = InventorySnapshotBudget::new(InventorySnapshotLimits {
            maximum_nodes: 2,
            maximum_content_bytes: 8,
            maximum_metadata_bytes: 6,
            maximum_depth: 1,
        });

        budget.charge_node().unwrap();
        budget.charge_content_bytes(5).unwrap();
        budget.charge_metadata_bytes(5).unwrap();
        assert_eq!(
            budget.charge_metadata_bytes(2),
            Err(InventorySnapshotBudgetError::MetadataBytes)
        );
        assert_eq!(budget.remaining_metadata_bytes(), 1);
        assert_eq!(
            budget.charge_content_bytes(4),
            Err(InventorySnapshotBudgetError::ContentBytes)
        );
        assert_eq!(budget.remaining_content_bytes(), 3);
        budget.charge_node().unwrap();
        assert_eq!(
            budget.charge_node(),
            Err(InventorySnapshotBudgetError::NodeLimit)
        );
        assert_eq!(budget.remaining_nodes(), 0);
        assert_eq!(budget.require_depth(1), Ok(()));
        assert_eq!(
            budget.require_depth(2),
            Err(InventorySnapshotBudgetError::Depth)
        );
    }

    #[test]
    fn bounded_cache_evicts_the_least_recently_used_entry() {
        let cache = ContentRevisionCache::new(NonZeroUsize::new(2).unwrap());
        let first = cache_key("first.bin", 1);
        let second = cache_key("second.bin", 2);
        let third = cache_key("third.bin", 3);

        cache.insert(first.clone(), "first");
        cache.insert(second.clone(), "second");
        assert_eq!(cache.get(&first), Some("first"));
        cache.insert(third.clone(), "third");

        assert_eq!(cache.get(&first), Some("first"));
        assert_eq!(cache.get(&second), None);
        assert_eq!(cache.get(&third), Some("third"));
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn poisoned_cache_discards_entries_and_remains_usable() {
        let cache = std::sync::Arc::new(ContentRevisionCache::new(NonZeroUsize::new(2).unwrap()));
        let key = cache_key("poisoned.bin", 7);
        cache.insert(key.clone(), "before");
        let worker_cache = cache.clone();
        let _ = thread::spawn(move || {
            let _guard = worker_cache.state.lock().unwrap();
            panic!("poison cache lock");
        })
        .join();

        assert_eq!(cache.get(&key), None);
        cache.insert(key.clone(), "after");
        assert_eq!(cache.get(&key), Some("after"));
    }

    #[cfg(unix)]
    #[test]
    fn retained_open_rejects_a_symbolic_link_without_reading_its_target() {
        use std::os::unix::fs::symlink;

        let temporary = tempdir().unwrap();
        let victim = temporary.path().join("victim.bin");
        fs::write(&victim, b"secret").unwrap();
        symlink(&victim, temporary.path().join("linked.bin")).unwrap();
        let directory = Dir::open_ambient_dir(temporary.path(), ambient_authority()).unwrap();

        assert!(RetainedInventoryFile::open_nofollow(&directory, "linked.bin").is_err());
        assert_eq!(fs::read(&victim).unwrap(), b"secret");
    }

    fn cache_key(path: &str, marker: i64) -> ContentRevisionCacheKey {
        ContentRevisionCacheKey::new(
            WorkspaceRelativePath::parse(path).unwrap(),
            StrongFileVersionStamp::Unix {
                device: 1,
                inode: marker as u64,
                link_count: 1,
                length: marker as u64,
                modified_seconds: marker,
                modified_nanoseconds: 0,
                changed_seconds: marker,
                changed_nanoseconds: 0,
            },
        )
    }
}
