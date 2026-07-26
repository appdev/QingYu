use std::cell::{Cell, RefCell};
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::ambient_authority;
use cap_std::fs::{Dir, OpenOptions};
use serde::de::DeserializeOwned;
use tokio::io::AsyncRead;

use crate::atomic_write::{stage_cap_file, CapStagedFile};
use crate::cloud::{CloudError, CloudUploadSource};
use crate::crypto::{decrypt, encrypt};
use crate::path_security::{
    cap_metadata_is_reparse, std_metadata_is_reparse,
    validate_windows_directory_components_before_canonicalize,
};
use crate::{CheckIndex, Chunk, File, Index, RepoError};

const OBJECT_MODE: u32 = 0o644;
const ZSTD_WINDOW_LOG: u32 = 19;
const MAX_CHUNK_DECODED_SIZE: usize = 8 * 1024 * 1024;
const RAW_ENCODING_OVERHEAD_LIMIT: usize = 1024 * 1024;
pub(crate) const MAX_CHUNK_RAW_SIZE: usize = MAX_CHUNK_DECODED_SIZE + RAW_ENCODING_OVERHEAD_LIMIT;

type OperationGuard = Arc<Mutex<()>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RawObjectKind {
    Chunk,
    File,
    Index,
    CheckIndex,
}

static OPERATION_GUARD: OnceLock<OperationGuard> = OnceLock::new();

pub struct Store {
    root: PathBuf,
    anchor: Dir,
    relative_root: PathBuf,
    repository_dir: Mutex<Option<Dir>>,
    operation_guard: OperationGuard,
    repo_gate: crate::lifecycle::LifecycleGate,
    key: [u8; 32],
    compressor: Mutex<zstd::bulk::Compressor<'static>>,
    decompressor: Mutex<zstd::zstd_safe::DCtx<'static>>,
}

pub(crate) struct StoreUploadSource {
    file: cap_std::fs::File,
    content_length: u64,
}

impl CloudUploadSource for StoreUploadSource {
    fn content_length(&self) -> u64 {
        self.content_length
    }

    fn open(&self) -> Result<Pin<Box<dyn AsyncRead + Send>>, CloudError> {
        let mut file = self.file.try_clone().map_err(CloudError::Io)?;
        file.seek(SeekFrom::Start(0)).map_err(CloudError::Io)?;
        let metadata = file.metadata().map_err(CloudError::Io)?;
        if !metadata.file_type().is_file() || cap_metadata_is_reparse(&metadata) {
            return Err(CloudError::UnsafeKey);
        }
        if metadata.len() != self.content_length {
            return Err(CloudError::LengthMismatch {
                expected: self.content_length,
                actual: metadata.len(),
            });
        }
        Ok(Box::pin(tokio::fs::File::from_std(file.into_std())))
    }
}

impl Store {
    pub fn new(root: impl Into<PathBuf>, key: [u8; 32]) -> Result<Self, RepoError> {
        let root = absolute_lexical_root(root.into())?;
        let (anchor, relative_root) = store_anchor(&root)?;
        let operation_guard = Arc::clone(OPERATION_GUARD.get_or_init(|| Arc::new(Mutex::new(()))));
        let mut repository_dir = anchor.try_clone()?;
        for component in relative_root.components() {
            let Component::Normal(name) = component else {
                return Err(RepoError::UnsafePath);
            };
            repository_dir = open_child_directory(&repository_dir, name, true)?;
        }
        validate_store_directory(&repository_dir)?;
        let repo_gate = crate::lifecycle::LifecycleGate::for_directory(&repository_dir)?;
        let mut compressor = zstd::bulk::Compressor::new(zstd::DEFAULT_COMPRESSION_LEVEL)
            .map_err(RepoError::Compression)?;
        compressor
            .include_checksum(false)
            .map_err(RepoError::Compression)?;
        compressor
            .window_log(ZSTD_WINDOW_LOG)
            .map_err(RepoError::Compression)?;
        let decompressor = zstd::zstd_safe::DCtx::try_create().ok_or_else(|| {
            RepoError::Compression(std::io::Error::other(
                "could not create zstd decompression context",
            ))
        })?;

        Ok(Self {
            root,
            anchor,
            relative_root,
            repository_dir: Mutex::new(Some(repository_dir)),
            operation_guard,
            repo_gate,
            key,
            compressor: Mutex::new(compressor),
            decompressor: Mutex::new(decompressor),
        })
    }

    pub fn object_path(&self, id: &str) -> Result<PathBuf, RepoError> {
        validate_id(id)?;
        Ok(self.root.join("objects").join(&id[..2]).join(&id[2..]))
    }

    pub fn index_path(&self, id: &str) -> Result<PathBuf, RepoError> {
        validate_id(id)?;
        Ok(self.root.join("indexes").join(id))
    }

    pub fn check_index_path(&self, id: &str) -> Result<PathBuf, RepoError> {
        validate_id(id)?;
        Ok(self.root.join("check").join("indexes").join(id))
    }

    pub fn contains_raw(&self, kind: RawObjectKind, id: &str) -> Result<bool, RepoError> {
        let _lifecycle = self.try_lifecycle()?;
        let _operation = self.lock_operation()?;
        self.contains_raw_unlocked(kind, id)
    }

    pub(crate) fn contains_raw_unlocked(
        &self,
        kind: RawObjectKind,
        id: &str,
    ) -> Result<bool, RepoError> {
        match self.open_raw_file(kind, id) {
            Ok(file) => {
                self.validate_raw_file(kind, id, &file)?;
                Ok(true)
            }
            Err(RepoError::NotFound(_)) => Ok(false),
            Err(error) => Err(error),
        }
    }

    pub fn export_raw(&self, kind: RawObjectKind, id: &str) -> Result<Vec<u8>, RepoError> {
        let _lifecycle = self.try_lifecycle()?;
        let _operation = self.lock_operation()?;
        self.export_raw_unlocked(kind, id)
    }

    pub fn import_raw(&self, kind: RawObjectKind, id: &str, bytes: &[u8]) -> Result<(), RepoError> {
        let _lifecycle = self.try_lifecycle()?;
        let _operation = self.lock_operation()?;
        self.import_raw_unlocked(kind, id, bytes)
    }

    pub fn list_raw_ids(&self, kind: RawObjectKind) -> Result<Vec<String>, RepoError> {
        let _lifecycle = self.try_lifecycle()?;
        let _operation = self.lock_operation()?;
        let mut ids = Vec::new();
        match kind {
            RawObjectKind::Index | RawObjectKind::CheckIndex => {
                let relative = if kind == RawObjectKind::Index {
                    Path::new("indexes")
                } else {
                    Path::new("check/indexes")
                };
                let directory = match self.open_directory(relative, false) {
                    Ok(directory) => directory,
                    Err(RepoError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                        return Ok(ids);
                    }
                    Err(error) => return Err(error),
                };
                for entry in directory.entries()? {
                    let entry = entry?;
                    let name = entry.file_name();
                    let Some(id) = name.to_str() else {
                        return Err(RepoError::UnsafePath);
                    };
                    validate_id(id)?;
                    let metadata = directory.symlink_metadata(&name)?;
                    if !metadata.file_type().is_file() || cap_metadata_is_reparse(&metadata) {
                        return Err(RepoError::UnsafePath);
                    }
                    let file = self.open_raw_file(kind, id)?;
                    self.validate_raw_file(kind, id, &file)?;
                    ids.push(id.to_owned());
                }
            }
            RawObjectKind::Chunk | RawObjectKind::File => {
                let objects = match self.open_directory(Path::new("objects"), false) {
                    Ok(directory) => directory,
                    Err(RepoError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                        return Ok(ids);
                    }
                    Err(error) => return Err(error),
                };
                for prefix_entry in objects.entries()? {
                    let prefix_entry = prefix_entry?;
                    let prefix = prefix_entry.file_name();
                    let prefix_text = prefix.to_str().ok_or(RepoError::UnsafePath)?;
                    if prefix_text.len() != 2 {
                        return Err(RepoError::InvalidData(
                            "object prefix must contain two hex characters",
                        ));
                    }
                    let directory = objects
                        .open_dir_nofollow(&prefix)
                        .map_err(|error| map_nofollow_error(&objects, &prefix, error))?;
                    validate_store_directory(&directory)?;
                    for entry in directory.entries()? {
                        let entry = entry?;
                        let suffix = entry.file_name();
                        let suffix_text = suffix.to_str().ok_or(RepoError::UnsafePath)?;
                        let id = format!("{prefix_text}{suffix_text}");
                        validate_id(&id)?;
                        let metadata = directory.symlink_metadata(&suffix)?;
                        if !metadata.file_type().is_file() || cap_metadata_is_reparse(&metadata) {
                            return Err(RepoError::UnsafePath);
                        }
                        let file = self.open_raw_file(kind, &id)?;
                        match self.validate_raw_file(kind, &id, &file) {
                            Ok(()) => ids.push(id),
                            Err(requested_error) => {
                                let other_kind = match kind {
                                    RawObjectKind::Chunk => RawObjectKind::File,
                                    RawObjectKind::File => RawObjectKind::Chunk,
                                    RawObjectKind::Index | RawObjectKind::CheckIndex => {
                                        unreachable!("index kinds use their own directories")
                                    }
                                };
                                if self.validate_raw_file(other_kind, &id, &file).is_err() {
                                    return Err(requested_error);
                                }
                            }
                        }
                    }
                }
            }
        }
        ids.sort();
        Ok(ids)
    }

    pub(crate) fn export_raw_unlocked(
        &self,
        kind: RawObjectKind,
        id: &str,
    ) -> Result<Vec<u8>, RepoError> {
        let bytes = self.read_raw(kind, id, raw_limit(kind))?;
        self.validate_raw(kind, id, &bytes)?;
        Ok(bytes)
    }

    pub(crate) fn import_raw_unlocked(
        &self,
        kind: RawObjectKind,
        id: &str,
        bytes: &[u8],
    ) -> Result<(), RepoError> {
        validate_id(id)?;
        if let Some(limit) = raw_limit(kind) {
            if bytes.len() > limit {
                return Err(RepoError::DecodedSizeLimitExceeded { limit });
            }
        }
        self.validate_raw(kind, id, bytes)?;
        let path = self.raw_path(kind, id)?;
        match self.read_raw(kind, id, raw_limit(kind)) {
            Ok(existing) => {
                self.validate_raw(kind, id, &existing)?;
                return if existing == bytes {
                    Ok(())
                } else {
                    Err(RepoError::InvalidData(
                        "immutable raw object already exists with different bytes",
                    ))
                };
            }
            Err(RepoError::NotFound(_)) => {}
            Err(error) => return Err(error),
        }
        self.write_immutable_object(&path, bytes)
    }

    pub(crate) fn import_raw_staged_unlocked(
        &self,
        kind: RawObjectKind,
        id: &str,
        source: &cap_std::fs::File,
    ) -> Result<(), RepoError> {
        self.import_raw_staged_with_before_publish_unlocked(kind, id, source, || Ok(()))
    }

    fn import_raw_staged_with_before_publish_unlocked<F>(
        &self,
        kind: RawObjectKind,
        id: &str,
        source: &cap_std::fs::File,
        before_publish: F,
    ) -> Result<(), RepoError>
    where
        F: FnOnce() -> Result<(), RepoError>,
    {
        validate_id(id)?;
        let path = self.raw_path(kind, id)?;
        let relative = self.relative_store_path(&path)?;
        let destination = relative
            .file_name()
            .ok_or(RepoError::InvalidData("object path must have a file name"))?;
        let parent =
            self.open_directory(relative.parent().unwrap_or_else(|| Path::new("")), true)?;
        let staged =
            crate::atomic_write::create_cap_staged_file(&parent, destination, OBJECT_MODE)?;
        copy_cap_file(source, staged.file(), raw_limit(kind))?;
        self.validate_raw_file(kind, id, staged.file())?;
        let validated_staged = staged.file().try_clone()?;

        match self.open_raw_file(kind, id) {
            Ok(existing) => {
                self.validate_raw_file(kind, id, &existing)?;
                return if cap_files_equal(&existing, staged.file())? {
                    Ok(())
                } else {
                    Err(RepoError::InvalidData(
                        "immutable raw object already exists with different bytes",
                    ))
                };
            }
            Err(RepoError::NotFound(_)) => {}
            Err(error) => return Err(error),
        }

        before_publish()?;
        if staged.publish_no_replace()? == crate::atomic_write::PublishOutcome::AlreadyExists {
            let existing = self.open_raw_file(kind, id)?;
            self.validate_raw_file(kind, id, &existing)?;
            if !cap_files_equal(&existing, &validated_staged)? {
                return Err(RepoError::InvalidData(
                    "immutable raw object already exists with different bytes",
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn open_raw_upload_source_unlocked(
        &self,
        kind: RawObjectKind,
        id: &str,
    ) -> Result<StoreUploadSource, RepoError> {
        let file = self.open_raw_file(kind, id)?;
        self.validate_raw_file(kind, id, &file)?;
        let content_length = file.metadata()?.len();
        Ok(StoreUploadSource {
            file,
            content_length,
        })
    }

    fn validate_raw(&self, kind: RawObjectKind, id: &str, bytes: &[u8]) -> Result<(), RepoError> {
        match kind {
            RawObjectKind::Chunk => {
                let decoded = self.decode_encrypted(bytes, MAX_CHUNK_DECODED_SIZE)?;
                if crate::sha1_hex(&decoded) != id {
                    return Err(RepoError::InvalidData(
                        "chunk payload id does not match its key",
                    ));
                }
            }
            RawObjectKind::File => {
                let compressed = decrypt(bytes, &self.key)?;
                let file: File = self.deserialize_compressed_reader(Cursor::new(compressed))?;
                if file.id != id {
                    return Err(RepoError::InvalidData(
                        "file payload id does not match its key",
                    ));
                }
                if file.size < 0 || File::new(&file.path, file.size, file.updated).id != id {
                    return Err(RepoError::InvalidData(
                        "file payload identity does not match its path and timestamp",
                    ));
                }
                validate_repository_object_path(&file.path)?;
                for chunk_id in &file.chunks {
                    validate_id(chunk_id)?;
                }
            }
            RawObjectKind::Index => {
                let index: Index = self.deserialize_compressed_reader(Cursor::new(bytes))?;
                self.validate_index(id, &index)?;
            }
            RawObjectKind::CheckIndex => {
                let check_index: CheckIndex =
                    self.deserialize_compressed_reader(Cursor::new(bytes))?;
                self.validate_check_index(id, &check_index)?;
            }
        }
        Ok(())
    }

    fn validate_raw_file(
        &self,
        kind: RawObjectKind,
        id: &str,
        file: &cap_std::fs::File,
    ) -> Result<(), RepoError> {
        match kind {
            RawObjectKind::Chunk | RawObjectKind::File => {
                let bytes = read_cap_file(file, raw_limit(kind))?;
                self.validate_raw(kind, id, &bytes)
            }
            RawObjectKind::Index => {
                let index: Index = self.deserialize_compressed_reader(rewound_clone(file)?)?;
                self.validate_index(id, &index)
            }
            RawObjectKind::CheckIndex => {
                let check_index: CheckIndex =
                    self.deserialize_compressed_reader(rewound_clone(file)?)?;
                self.validate_check_index(id, &check_index)
            }
        }
    }

    fn validate_index(&self, id: &str, index: &Index) -> Result<(), RepoError> {
        if index.id != id {
            return Err(RepoError::InvalidData(
                "index payload id does not match its key",
            ));
        }
        if index.count != index.files.len() || index.size < 0 {
            return Err(RepoError::InvalidData(
                "index payload count or size is invalid",
            ));
        }
        for file_id in &index.files {
            validate_id(file_id)?;
        }
        if !index.check_index_id.is_empty() {
            validate_id(&index.check_index_id)?;
        }
        if !index.verify_aes_key(&self.key) {
            return Err(RepoError::DecryptionFailed);
        }
        Ok(())
    }

    fn validate_check_index(&self, id: &str, check_index: &CheckIndex) -> Result<(), RepoError> {
        if check_index.id != id {
            return Err(RepoError::InvalidData(
                "check index payload id does not match its key",
            ));
        }
        validate_id(&check_index.index_id)?;
        for file in &check_index.files {
            validate_id(&file.id)?;
            for chunk_id in &file.chunks {
                validate_id(chunk_id)?;
            }
        }
        Ok(())
    }

    fn raw_path(&self, kind: RawObjectKind, id: &str) -> Result<PathBuf, RepoError> {
        match kind {
            RawObjectKind::Chunk | RawObjectKind::File => self.object_path(id),
            RawObjectKind::Index => self.index_path(id),
            RawObjectKind::CheckIndex => self.check_index_path(id),
        }
    }

    fn open_raw_file(&self, kind: RawObjectKind, id: &str) -> Result<cap_std::fs::File, RepoError> {
        let path = self.raw_path(kind, id)?;
        let relative = self.relative_store_path(&path)?;
        let name = relative
            .file_name()
            .ok_or(RepoError::InvalidData("object path must have a file name"))?;
        let parent = self
            .open_directory(relative.parent().unwrap_or_else(|| Path::new("")), false)
            .map_err(|error| map_not_found(error, id))?;
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        let file = parent
            .open_with(name, &options)
            .map_err(|error| map_object_io(error, id))?;
        let metadata = file.metadata()?;
        if !metadata.file_type().is_file() || cap_metadata_is_reparse(&metadata) {
            return Err(RepoError::UnsafePath);
        }
        Ok(file)
    }

    fn read_raw(
        &self,
        kind: RawObjectKind,
        id: &str,
        limit: Option<usize>,
    ) -> Result<Vec<u8>, RepoError> {
        let file = self.open_raw_file(kind, id)?;
        read_cap_file(&file, limit)
    }

    pub fn put_chunk(&self, chunk: &Chunk) -> Result<(), RepoError> {
        let _lifecycle = self.try_lifecycle()?;
        let _operation = self.lock_operation()?;
        self.put_chunk_unlocked(chunk)
    }

    pub(crate) fn put_chunk_unlocked(&self, chunk: &Chunk) -> Result<(), RepoError> {
        let path = self.object_path(&chunk.id)?;
        let encoded = self.encode_encrypted(&chunk.data)?;
        self.write_immutable_object(&path, &encoded)
    }

    pub fn get_chunk(&self, id: &str) -> Result<Chunk, RepoError> {
        let _lifecycle = self.try_lifecycle()?;
        let _operation = self.lock_operation()?;
        self.get_chunk_unlocked(id)
    }

    pub(crate) fn get_chunk_unlocked(&self, id: &str) -> Result<Chunk, RepoError> {
        let encoded = self.export_raw_unlocked(RawObjectKind::Chunk, id)?;
        let chunk = Chunk {
            id: id.to_owned(),
            data: self.decode_encrypted(&encoded, MAX_CHUNK_DECODED_SIZE)?,
        };
        Ok(chunk)
    }

    pub fn put_file(&self, file: &File) -> Result<(), RepoError> {
        let _lifecycle = self.try_lifecycle()?;
        let _operation = self.lock_operation()?;
        self.put_file_unlocked(file)
    }

    pub(crate) fn put_file_unlocked(&self, file: &File) -> Result<(), RepoError> {
        let path = self.object_path(&file.id)?;
        let json = serde_json::to_vec(file)?;
        let encoded = self.encode_encrypted(&json)?;
        self.write_immutable_object(&path, &encoded)
    }

    pub fn get_file(&self, id: &str) -> Result<File, RepoError> {
        let _lifecycle = self.try_lifecycle()?;
        let _operation = self.lock_operation()?;
        self.get_file_unlocked(id)
    }

    pub(crate) fn get_file_unlocked(&self, id: &str) -> Result<File, RepoError> {
        let encoded = self.export_raw_unlocked(RawObjectKind::File, id)?;
        self.decode_raw_file_unlocked(id, &encoded)
    }

    pub(crate) fn decode_raw_file_unlocked(
        &self,
        id: &str,
        encoded: &[u8],
    ) -> Result<File, RepoError> {
        self.validate_raw(RawObjectKind::File, id, encoded)?;
        let compressed = decrypt(encoded, &self.key)?;
        self.deserialize_compressed_reader(Cursor::new(compressed))
    }

    pub fn put_index(&self, index: &Index) -> Result<(), RepoError> {
        let _lifecycle = self.try_lifecycle()?;
        let _operation = self.lock_operation()?;
        self.put_index_unlocked(index)
    }

    pub(crate) fn put_index_unlocked(&self, index: &Index) -> Result<(), RepoError> {
        self.put_index_with_mtime_unlocked(index, |file, mtime| {
            let standard_file = file.try_clone()?.into_std();
            filetime::set_file_handle_times(&standard_file, None, Some(mtime))
        })
    }

    #[cfg(test)]
    fn put_index_with_mtime<F>(&self, index: &Index, set_mtime: F) -> Result<(), RepoError>
    where
        F: FnOnce(&cap_std::fs::File, filetime::FileTime) -> std::io::Result<()>,
    {
        let _lifecycle = self.try_lifecycle()?;
        let _operation = self.lock_operation()?;
        self.put_index_with_mtime_unlocked(index, set_mtime)
    }

    fn put_index_with_mtime_unlocked<F>(&self, index: &Index, set_mtime: F) -> Result<(), RepoError>
    where
        F: FnOnce(&cap_std::fs::File, filetime::FileTime) -> std::io::Result<()>,
    {
        let path = self.index_path(&index.id)?;
        let json = serde_json::to_vec(index)?;
        let encoded = self.compress(&json)?;
        let staged = self.stage_object(&path, &encoded)?;
        let seconds = index.created.div_euclid(1_000);
        let nanos = index.created.rem_euclid(1_000) as u32 * 1_000_000;
        set_mtime(
            staged.file(),
            filetime::FileTime::from_unix_time(seconds, nanos),
        )?;
        staged.publish_replace()
    }

    pub fn get_index(&self, id: &str) -> Result<Index, RepoError> {
        let _lifecycle = self.try_lifecycle()?;
        let _operation = self.lock_operation()?;
        self.get_index_unlocked(id)
    }

    pub(crate) fn get_index_unlocked(&self, id: &str) -> Result<Index, RepoError> {
        let file = self.open_raw_file(RawObjectKind::Index, id)?;
        self.decode_index_reader_unlocked(id, rewound_clone(&file)?)
    }

    pub(crate) fn list_indexes_by_mtime_unlocked(&self) -> Result<Vec<Index>, RepoError> {
        let directory = match self.open_directory(Path::new("indexes"), false) {
            Ok(directory) => directory,
            Err(RepoError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Vec::new())
            }
            Err(error) => return Err(error),
        };
        let mut addressed = Vec::new();
        for entry in directory.entries()? {
            let name = entry?.file_name();
            let id = name.to_str().ok_or(RepoError::UnsafePath)?;
            if id.len() != 40 {
                continue;
            }
            validate_id(id)?;
            let metadata = directory.symlink_metadata(&name)?;
            if !metadata.file_type().is_file() || cap_metadata_is_reparse(&metadata) {
                return Err(RepoError::UnsafePath);
            }
            let file = self.open_raw_file(RawObjectKind::Index, id)?;
            let modified = file.metadata()?.modified()?;
            addressed.push((modified, id.to_owned(), file));
        }
        addressed.sort_by_key(|entry| std::cmp::Reverse(entry.0));

        let mut indexes = Vec::with_capacity(addressed.len());
        for (_modified, id, file) in addressed {
            indexes.push(self.decode_index_reader_unlocked(&id, rewound_clone(&file)?)?);
        }
        Ok(indexes)
    }

    pub(crate) fn decode_index_reader_unlocked<R: Read>(
        &self,
        id: &str,
        reader: R,
    ) -> Result<Index, RepoError> {
        validate_id(id)?;
        let index = self.deserialize_compressed_reader(reader)?;
        self.validate_index(id, &index)?;
        Ok(index)
    }

    pub fn put_check_index(&self, check_index: &CheckIndex) -> Result<(), RepoError> {
        let _lifecycle = self.try_lifecycle()?;
        let _operation = self.lock_operation()?;
        self.put_check_index_unlocked(check_index)
    }

    pub(crate) fn put_check_index_unlocked(
        &self,
        check_index: &CheckIndex,
    ) -> Result<(), RepoError> {
        let path = self.check_index_path(&check_index.id)?;
        let json = serde_json::to_vec(check_index)?;
        let encoded = self.compress(&json)?;
        self.write_object(&path, &encoded)
    }

    pub fn get_check_index(&self, id: &str) -> Result<CheckIndex, RepoError> {
        let _lifecycle = self.try_lifecycle()?;
        let _operation = self.lock_operation()?;
        self.get_check_index_unlocked(id)
    }

    pub(crate) fn get_check_index_unlocked(&self, id: &str) -> Result<CheckIndex, RepoError> {
        let file = self.open_raw_file(RawObjectKind::CheckIndex, id)?;
        let check_index = self.deserialize_compressed_reader(rewound_clone(&file)?)?;
        self.validate_check_index(id, &check_index)?;
        Ok(check_index)
    }

    fn write_object(&self, path: &Path, bytes: &[u8]) -> Result<(), RepoError> {
        self.stage_object(path, bytes)?.publish_replace()
    }

    fn write_immutable_object(&self, path: &Path, bytes: &[u8]) -> Result<(), RepoError> {
        self.stage_object(path, bytes)?.publish_no_replace()?;
        Ok(())
    }

    fn stage_object(&self, path: &Path, bytes: &[u8]) -> Result<CapStagedFile, RepoError> {
        let relative = self.relative_store_path(path)?;
        let destination = relative
            .file_name()
            .ok_or(RepoError::InvalidData("object path must have a file name"))?;
        let parent =
            self.open_directory(relative.parent().unwrap_or_else(|| Path::new("")), true)?;
        stage_cap_file(&parent, destination, bytes, OBJECT_MODE)
    }

    fn relative_store_path<'a>(&self, path: &'a Path) -> Result<&'a Path, RepoError> {
        let relative = path
            .strip_prefix(&self.root)
            .map_err(|_| RepoError::UnsafePath)?;
        validate_relative_path(relative)?;
        Ok(relative)
    }

    pub(crate) fn open_directory(&self, relative: &Path, create: bool) -> Result<Dir, RepoError> {
        let mut directory = self.open_repository_root(create)?;
        for component in relative.components() {
            let Component::Normal(name) = component else {
                return Err(RepoError::UnsafePath);
            };
            directory = open_child_directory(&directory, name, create)?;
        }
        Ok(directory)
    }

    pub(crate) fn lock_operation(&self) -> Result<MutexGuard<'_, ()>, RepoError> {
        self.operation_guard
            .lock()
            .map_err(|_| RepoError::InvalidData("repository operation lock poisoned"))
    }

    pub(crate) fn try_lifecycle(&self) -> Result<tokio::sync::OwnedMutexGuard<()>, RepoError> {
        self.repo_gate.try_acquire()
    }

    pub(crate) fn repo_gate(&self) -> &crate::lifecycle::LifecycleGate {
        &self.repo_gate
    }

    fn open_repository_root(&self, create: bool) -> Result<Dir, RepoError> {
        let mut retained = self
            .repository_dir
            .lock()
            .map_err(|_| RepoError::InvalidData("repository directory handle lock poisoned"))?;
        if let Some(directory) = retained.as_ref() {
            validate_store_directory(directory)?;
            return Ok(directory.try_clone()?);
        }

        let mut directory = self.anchor.try_clone()?;
        for component in self.relative_root.components() {
            let Component::Normal(name) = component else {
                return Err(RepoError::UnsafePath);
            };
            directory = open_child_directory(&directory, name, create)?;
        }
        validate_store_directory(&directory)?;
        *retained = Some(directory.try_clone()?);
        Ok(directory)
    }

    fn encode_encrypted(&self, bytes: &[u8]) -> Result<Vec<u8>, RepoError> {
        let compressed = self.compress(bytes)?;
        encrypt(&compressed, &self.key)
    }

    fn decode_encrypted(&self, bytes: &[u8], limit: usize) -> Result<Vec<u8>, RepoError> {
        let compressed = decrypt(bytes, &self.key)?;
        self.decompress(&compressed, limit)
    }

    pub(crate) fn compress(&self, bytes: &[u8]) -> Result<Vec<u8>, RepoError> {
        self.lock_compressor()?
            .compress(bytes)
            .map_err(RepoError::Compression)
    }

    fn decompress(&self, bytes: &[u8], limit: usize) -> Result<Vec<u8>, RepoError> {
        let content_size = zstd::zstd_safe::get_frame_content_size(bytes).map_err(|_| {
            RepoError::InvalidData("zstd frame is invalid or requires a window larger than 512 KiB")
        })?;
        if matches!(content_size, Some(size) if size > limit as u64) {
            return Err(RepoError::DecodedSizeLimitExceeded { limit });
        }

        let mut context = self.lock_decompressor()?;
        context
            .reset(zstd::zstd_safe::ResetDirective::SessionAndParameters)
            .map_err(|_| {
                RepoError::Compression(std::io::Error::other(
                    "could not reset zstd decompression context",
                ))
            })?;
        context
            .set_parameter(zstd::zstd_safe::DParameter::WindowLogMax(ZSTD_WINDOW_LOG))
            .map_err(|_| {
                RepoError::Compression(std::io::Error::other(
                    "could not set zstd decompression window limit",
                ))
            })?;
        let reader = BufReader::new(Cursor::new(bytes));
        let decoder = zstd::stream::read::Decoder::with_context(reader, &mut context);
        let read_limit = (limit as u64).saturating_add(1);
        let mut decoded = Vec::new();
        decoder
            .take(read_limit)
            .read_to_end(&mut decoded)
            .map_err(|_| {
                RepoError::InvalidData(
                    "zstd frame is invalid or requires a window larger than 512 KiB",
                )
            })?;
        if decoded.len() > limit {
            return Err(RepoError::DecodedSizeLimitExceeded { limit });
        }
        Ok(decoded)
    }

    pub(crate) fn deserialize_compressed_reader<T, R>(&self, reader: R) -> Result<T, RepoError>
    where
        T: DeserializeOwned,
        R: Read,
    {
        let mut context = self.lock_decompressor()?;
        context
            .reset(zstd::zstd_safe::ResetDirective::SessionAndParameters)
            .map_err(|_| {
                RepoError::Compression(std::io::Error::other(
                    "could not reset zstd decompression context",
                ))
            })?;
        context
            .set_parameter(zstd::zstd_safe::DParameter::WindowLogMax(ZSTD_WINDOW_LOG))
            .map_err(|_| {
                RepoError::Compression(std::io::Error::other(
                    "could not set zstd decompression window limit",
                ))
            })?;
        let source_error = Rc::new(RefCell::new(None));
        let tracked_reader = SourceErrorReader {
            inner: reader,
            source_error: Rc::clone(&source_error),
        };
        let decoder =
            zstd::stream::read::Decoder::with_context(BufReader::new(tracked_reader), &mut context);
        let decoder_error = Rc::new(Cell::new(false));
        let mut tracked_decoder = DecoderErrorReader {
            inner: decoder,
            failed: Rc::clone(&decoder_error),
        };
        let parsed = serde_json::from_reader(&mut tracked_decoder);
        let drain = std::io::copy(&mut tracked_decoder, &mut std::io::sink());
        if let Some(error) = source_error.borrow_mut().take() {
            return Err(RepoError::Io(error));
        }
        if decoder_error.get() || drain.is_err() {
            return Err(RepoError::InvalidData(
                "zstd frame is invalid or requires a window larger than 512 KiB",
            ));
        }
        parsed.map_err(RepoError::Serialization)
    }

    fn lock_compressor(
        &self,
    ) -> Result<MutexGuard<'_, zstd::bulk::Compressor<'static>>, RepoError> {
        self.compressor
            .lock()
            .map_err(|_| RepoError::InvalidData("zstd compressor lock poisoned"))
    }

    fn lock_decompressor(
        &self,
    ) -> Result<MutexGuard<'_, zstd::zstd_safe::DCtx<'static>>, RepoError> {
        self.decompressor
            .lock()
            .map_err(|_| RepoError::InvalidData("zstd decompressor lock poisoned"))
    }
}

struct SourceErrorReader<R> {
    inner: R,
    source_error: Rc<RefCell<Option<std::io::Error>>>,
}

struct DecoderErrorReader<R> {
    inner: R,
    failed: Rc<Cell<bool>>,
}

impl<R: Read> Read for DecoderErrorReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        match self.inner.read(buffer) {
            Ok(read) => Ok(read),
            Err(error) => {
                self.failed.set(true);
                Err(error)
            }
        }
    }
}

impl<R: Read> Read for SourceErrorReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        match self.inner.read(buffer) {
            Ok(read) => Ok(read),
            Err(error) => {
                let retained = match error.raw_os_error() {
                    Some(code) => std::io::Error::from_raw_os_error(code),
                    None => std::io::Error::new(error.kind(), error.to_string()),
                };
                *self.source_error.borrow_mut() = Some(retained);
                Err(error)
            }
        }
    }
}

fn raw_limit(kind: RawObjectKind) -> Option<usize> {
    match kind {
        RawObjectKind::Chunk => Some(MAX_CHUNK_RAW_SIZE),
        RawObjectKind::File | RawObjectKind::Index | RawObjectKind::CheckIndex => None,
    }
}

fn rewound_clone(file: &cap_std::fs::File) -> Result<cap_std::fs::File, RepoError> {
    let mut reader = file.try_clone()?;
    reader.seek(SeekFrom::Start(0))?;
    Ok(reader)
}

fn read_cap_file(file: &cap_std::fs::File, limit: Option<usize>) -> Result<Vec<u8>, RepoError> {
    let mut reader = rewound_clone(file)?;
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        if let Some(limit) = limit {
            let next = bytes
                .len()
                .checked_add(read)
                .ok_or(RepoError::DecodedSizeLimitExceeded { limit })?;
            if next > limit {
                return Err(RepoError::DecodedSizeLimitExceeded { limit });
            }
        }
        bytes
            .try_reserve(read)
            .map_err(|_| RepoError::InvalidData("repository object cannot fit in memory"))?;
        bytes.extend_from_slice(&buffer[..read]);
    }
    Ok(bytes)
}

fn copy_cap_file(
    source: &cap_std::fs::File,
    destination: &cap_std::fs::File,
    limit: Option<usize>,
) -> Result<u64, RepoError> {
    let mut reader = rewound_clone(source)?;
    let mut writer = destination.try_clone()?;
    writer.seek(SeekFrom::Start(0))?;
    writer.set_len(0)?;
    let mut copied = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        copied = copied
            .checked_add(read as u64)
            .ok_or(RepoError::InvalidData("repository object size overflow"))?;
        if let Some(limit) = limit {
            if copied > limit as u64 {
                return Err(RepoError::DecodedSizeLimitExceeded { limit });
            }
        }
        writer.write_all(&buffer[..read])?;
    }
    writer.sync_all()?;
    Ok(copied)
}

fn cap_files_equal(left: &cap_std::fs::File, right: &cap_std::fs::File) -> Result<bool, RepoError> {
    if left.metadata()?.len() != right.metadata()?.len() {
        return Ok(false);
    }
    let mut left = rewound_clone(left)?;
    let mut right = rewound_clone(right)?;
    let mut left_buffer = [0_u8; 64 * 1024];
    let mut right_buffer = [0_u8; 64 * 1024];
    loop {
        let left_read = left.read(&mut left_buffer)?;
        let right_read = right.read(&mut right_buffer)?;
        if left_read != right_read || left_buffer[..left_read] != right_buffer[..right_read] {
            return Ok(false);
        }
        if left_read == 0 {
            return Ok(true);
        }
    }
}

fn validate_repository_object_path(path: &str) -> Result<(), RepoError> {
    if path == "/" || !path.starts_with('/') || path.contains('\\') {
        return Err(RepoError::UnsafePath);
    }
    if path[1..]
        .split('/')
        .any(|component| component.is_empty() || component == "." || component == "..")
    {
        Err(RepoError::UnsafePath)
    } else {
        Ok(())
    }
}

pub(crate) fn validate_id(id: &str) -> Result<(), RepoError> {
    if id.len() == 40
        && id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(RepoError::InvalidData(
            "object id must be 40 lowercase hex characters",
        ))
    }
}

pub(crate) fn absolute_lexical_root(root: PathBuf) -> Result<PathBuf, RepoError> {
    if root.as_os_str().is_empty() {
        return Err(RepoError::UnsafePath);
    }
    let absolute = if root.is_absolute() {
        root
    } else {
        std::env::current_dir()?.join(root)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(RepoError::UnsafePath);
                }
            }
        }
    }
    if normalized.is_absolute() {
        Ok(normalized)
    } else {
        Err(RepoError::UnsafePath)
    }
}

pub(crate) fn store_anchor(root: &Path) -> Result<(Dir, PathBuf), RepoError> {
    validate_windows_directory_components_before_canonicalize(root)?;
    let mut existing = root.to_path_buf();
    let mut missing = Vec::new();
    loop {
        match std::fs::symlink_metadata(&existing) {
            Ok(metadata) => {
                if !metadata.file_type().is_dir() || std_metadata_is_reparse(&metadata) {
                    return Err(RepoError::UnsafePath);
                }
                let canonical_existing = std::fs::canonicalize(&existing)?;
                let anchor = open_absolute_dir_nofollow(&canonical_existing)?;
                let relative = missing.into_iter().rev().collect::<PathBuf>();
                return Ok((anchor, relative));
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                let component = existing
                    .file_name()
                    .ok_or(RepoError::UnsafePath)?
                    .to_os_string();
                missing.push(component);
                if !existing.pop() {
                    return Err(RepoError::UnsafePath);
                }
            }
            Err(error) => return Err(error.into()),
        }
    }
}

pub(crate) fn open_absolute_dir_nofollow(path: &Path) -> Result<Dir, RepoError> {
    if !path.is_absolute() {
        return Err(RepoError::UnsafePath);
    }
    let mut ambient_root = PathBuf::new();
    let mut names = Vec::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir if names.is_empty() => {
                ambient_root.push(component.as_os_str());
            }
            Component::Normal(name) => names.push(name.to_os_string()),
            Component::CurDir => {}
            Component::ParentDir | Component::Prefix(_) | Component::RootDir => {
                return Err(RepoError::UnsafePath);
            }
        }
    }
    if ambient_root.as_os_str().is_empty() {
        return Err(RepoError::UnsafePath);
    }
    let mut directory = Dir::open_ambient_dir(ambient_root, ambient_authority())?;
    validate_store_directory(&directory)?;
    for name in names {
        directory = directory
            .open_dir_nofollow(&name)
            .map_err(|error| map_nofollow_error(&directory, &name, error))?;
        validate_store_directory(&directory)?;
    }
    Ok(directory)
}

pub(crate) fn open_or_create_absolute_dir_nofollow(path: &Path) -> Result<Dir, RepoError> {
    let root = absolute_lexical_root(path.to_path_buf())?;
    let (anchor, relative_root) = store_anchor(&root)?;
    let mut directory = anchor;
    for component in relative_root.components() {
        let Component::Normal(name) = component else {
            return Err(RepoError::UnsafePath);
        };
        directory = open_child_directory(&directory, name, true)?;
    }
    validate_store_directory(&directory)?;
    Ok(directory)
}

pub(crate) fn open_child_directory(
    parent: &Dir,
    name: &std::ffi::OsStr,
    create: bool,
) -> Result<Dir, RepoError> {
    match parent.open_dir_nofollow(name) {
        Ok(directory) => {
            validate_store_directory(&directory)?;
            Ok(directory)
        }
        Err(error) if create && error.kind() == std::io::ErrorKind::NotFound => {
            match parent.create_dir(name) {
                Ok(()) => {}
                Err(create_error) if create_error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(create_error) => return Err(create_error.into()),
            }
            let directory = parent
                .open_dir_nofollow(name)
                .map_err(|open_error| map_nofollow_error(parent, name, open_error))?;
            validate_store_directory(&directory)?;
            Ok(directory)
        }
        Err(error) => Err(map_nofollow_error(parent, name, error)),
    }
}

fn map_nofollow_error(parent: &Dir, name: &std::ffi::OsStr, error: std::io::Error) -> RepoError {
    match parent.symlink_metadata(name) {
        Ok(metadata) if metadata.file_type().is_symlink() || cap_metadata_is_reparse(&metadata) => {
            RepoError::UnsafePath
        }
        _ => RepoError::Io(error),
    }
}

fn validate_store_directory(directory: &Dir) -> Result<(), RepoError> {
    let metadata = directory.dir_metadata()?;
    if !metadata.file_type().is_dir() || cap_metadata_is_reparse(&metadata) {
        Err(RepoError::UnsafePath)
    } else {
        Ok(())
    }
}

fn validate_relative_path(path: &Path) -> Result<(), RepoError> {
    if path
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
    {
        Ok(())
    } else {
        Err(RepoError::UnsafePath)
    }
}

fn map_not_found(error: RepoError, id: &str) -> RepoError {
    match error {
        RepoError::Io(io_error) if io_error.kind() == std::io::ErrorKind::NotFound => {
            RepoError::NotFound(id.to_owned())
        }
        other => other,
    }
}

fn map_object_io(error: std::io::Error, id: &str) -> RepoError {
    if error.kind() == std::io::ErrorKind::NotFound {
        RepoError::NotFound(id.to_owned())
    } else {
        RepoError::Io(error)
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::fs;
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::path::Path;
    use std::sync::{Arc, Barrier};

    use cap_std::ambient_authority;
    use cap_std::fs::Dir;
    use tokio::io::AsyncReadExt;

    use super::{Store, MAX_CHUNK_DECODED_SIZE};
    use crate::atomic_write::{stage_cap_file, write_file_safer, PublishOutcome};
    use crate::cloud::CloudUploadSource;
    use crate::{CheckIndex, CheckIndexFile, Chunk, File, Index, RepoError};

    const FILE_ID: &str = "9088f936691086d6a0c11e516cf2ec1b2cef77d6";
    const GOLDEN_FILE_ID: &str = "0123456789abcdef0123456789abcdef01234567";
    const INDEX_ID: &str = "89abcdef0123456789abcdef0123456789abcdef";
    const CHECK_INDEX_ID: &str = "fedcba9876543210fedcba9876543210fedcba98";
    const CHUNK_ID: &str = "1111111111111111111111111111111111111111";
    const SECOND_CHUNK_ID: &str = "2222222222222222222222222222222222222222";

    struct FailingReader {
        inner: std::io::Cursor<Vec<u8>>,
        fail_after: u64,
    }

    impl Read for FailingReader {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            if self.inner.position() >= self.fail_after {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::ConnectionReset,
                    "injected source read failure",
                ));
            }
            let remaining = usize::try_from(self.fail_after - self.inner.position()).unwrap();
            let read_limit = remaining.min(buffer.len());
            Read::read(&mut self.inner, &mut buffer[..read_limit])
        }
    }

    fn fixture_key() -> [u8; 32] {
        *include_bytes!("../tests/fixtures/golden/kdf-key.bin")
    }

    fn fixture_file() -> File {
        File {
            id: FILE_ID.to_owned(),
            path: "/oracle/文档.md".to_owned(),
            size: 12,
            updated: 1_700_000_000_123,
            chunks: vec![
                CHUNK_ID.to_owned(),
                "2222222222222222222222222222222222222222".to_owned(),
            ],
        }
    }

    fn fixture_index() -> Index {
        Index {
            id: INDEX_ID.to_owned(),
            memo: "Go golden oracle".to_owned(),
            created: 1_700_000_000_456,
            files: vec![FILE_ID.to_owned()],
            count: 1,
            size: 12,
            system_id: "oracle-system-id".to_owned(),
            system_name: "Oracle Device".to_owned(),
            system_os: "darwin".to_owned(),
            check_index_id: CHECK_INDEX_ID.to_owned(),
            aes_key_verify_val: "OoReOoJ1EFlKERoswkpAFJjL+pouFAtGoNLytSis7qXOCg==".to_owned(),
        }
    }

    fn fixture_check_index() -> CheckIndex {
        CheckIndex {
            id: CHECK_INDEX_ID.to_owned(),
            index_id: INDEX_ID.to_owned(),
            files: vec![CheckIndexFile {
                id: FILE_ID.to_owned(),
                chunks: vec![CHUNK_ID.to_owned()],
            }],
        }
    }

    fn write_fixture(path: &Path, bytes: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn assert_no_temp_files(path: &Path) {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let entry_path = entry.path();
            if entry_path.is_dir() {
                assert_no_temp_files(&entry_path);
            } else {
                assert!(!entry.file_name().to_string_lossy().ends_with(".tmp"));
            }
        }
    }

    fn zstd_window_size(frame: &[u8]) -> Option<u64> {
        let frame_header_descriptor = *frame.get(4)?;
        if frame_header_descriptor & 0x20 != 0 {
            return None;
        }

        let window_descriptor = *frame.get(5)?;
        let exponent = u32::from(window_descriptor >> 3) + 10;
        let base = 1_u64 << exponent;
        let add = (base / 8) * u64::from(window_descriptor & 0x07);
        Some(base + add)
    }

    fn zstd_rle_frame_with_content_size(decoded_size: usize) -> Vec<u8> {
        assert!(decoded_size <= u32::MAX as usize);
        let mut frame = vec![0x28, 0xb5, 0x2f, 0xfd, 0xa0];
        frame.extend_from_slice(&(decoded_size as u32).to_le_bytes());
        append_rle_blocks(&mut frame, decoded_size);
        frame
    }

    fn zstd_rle_frame_without_content_size(decoded_size: usize) -> Vec<u8> {
        let mut frame = vec![0x28, 0xb5, 0x2f, 0xfd, 0x00, 0x48];
        append_rle_blocks(&mut frame, decoded_size);
        frame
    }

    fn append_rle_blocks(frame: &mut Vec<u8>, decoded_size: usize) {
        let mut remaining = decoded_size;
        while remaining > 0 {
            let block_size = remaining.min(128 * 1024);
            remaining -= block_size;
            let last_block = usize::from(remaining == 0);
            let header = (block_size << 3) | (1 << 1) | last_block;
            frame.extend_from_slice(&(header as u32).to_le_bytes()[..3]);
            frame.push(b'x');
        }
    }

    #[test]
    fn paths_match_dejavu_layout_and_reject_noncanonical_ids() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::new(temp.path(), fixture_key()).unwrap();

        assert_eq!(
            store.object_path(FILE_ID).unwrap(),
            temp.path()
                .join("objects")
                .join(&FILE_ID[..2])
                .join(&FILE_ID[2..])
        );
        assert_eq!(
            store.index_path(INDEX_ID).unwrap(),
            temp.path().join("indexes").join(INDEX_ID)
        );
        assert_eq!(
            store.check_index_path(CHECK_INDEX_ID).unwrap(),
            temp.path()
                .join("check")
                .join("indexes")
                .join(CHECK_INDEX_ID)
        );

        for invalid in [
            "",
            "0123456789abcdef0123456789abcdef0123456",
            "0123456789abcdef0123456789abcdef012345678",
            "0123456789abcdef0123456789abcdef0123456A",
            "0123456789abcdef0123456789abcdef0123456/",
        ] {
            assert!(matches!(
                store.object_path(invalid),
                Err(RepoError::InvalidData(
                    "object id must be 40 lowercase hex characters"
                ))
            ));
            assert!(store.index_path(invalid).is_err());
            assert!(store.check_index_path(invalid).is_err());
        }
    }

    #[test]
    fn decodes_pinned_go_file_fixture_and_every_field() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::new(temp.path(), fixture_key()).unwrap();
        write_fixture(
            &store.object_path(GOLDEN_FILE_ID).unwrap(),
            include_bytes!("../tests/fixtures/golden/file-object.bin"),
        );

        let encoded = store
            .read_raw(
                super::RawObjectKind::File,
                GOLDEN_FILE_ID,
                super::raw_limit(super::RawObjectKind::File),
            )
            .unwrap();
        let compressed = crate::decrypt(&encoded, &fixture_key()).unwrap();
        let file: File = store
            .deserialize_compressed_reader(std::io::Cursor::new(compressed))
            .unwrap();

        assert_eq!(file.id, GOLDEN_FILE_ID);
        assert_eq!(file.path, "/oracle/文档.md");
        assert_eq!(file.size, 12);
        assert_eq!(file.updated, 1_700_000_000_123);
        assert_eq!(
            file.chunks,
            vec![
                CHUNK_ID.to_owned(),
                "2222222222222222222222222222222222222222".to_owned()
            ]
        );
    }

    #[test]
    fn decodes_pinned_go_index_fixture_and_every_field() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::new(temp.path(), fixture_key()).unwrap();
        write_fixture(
            &store.index_path(INDEX_ID).unwrap(),
            include_bytes!("../tests/fixtures/golden/index-object.bin"),
        );

        let index = store.get_index(INDEX_ID).unwrap();

        assert_eq!(index.id, INDEX_ID);
        assert_eq!(index.memo, "Go golden oracle");
        assert_eq!(index.created, 1_700_000_000_456);
        assert_eq!(index.files, vec![GOLDEN_FILE_ID.to_owned()]);
        assert_eq!(index.count, 1);
        assert_eq!(index.size, 12);
        assert_eq!(index.system_id, "oracle-system-id");
        assert_eq!(index.system_name, "Oracle Device");
        assert_eq!(index.system_os, "darwin");
        assert_eq!(index.check_index_id, CHECK_INDEX_ID);
        assert_eq!(
            index.aes_key_verify_val,
            "OoReOoJ1EFlKERoswkpAFJjL+pouFAtGoNLytSis7qXOCg=="
        );
        assert!(index.verify_aes_key(&fixture_key()));
    }

    #[test]
    fn rust_store_round_trips_all_four_object_types() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::new(temp.path(), fixture_key()).unwrap();
        let file = fixture_file();
        let chunk = Chunk {
            id: crate::sha1_hex(b"raw chunk bytes"),
            data: b"raw chunk bytes".to_vec(),
        };
        let index = fixture_index();
        let check_index = fixture_check_index();

        store.put_file(&file).unwrap();
        store.put_chunk(&chunk).unwrap();
        store.put_index(&index).unwrap();
        store.put_check_index(&check_index).unwrap();

        assert_eq!(store.get_file(FILE_ID).unwrap(), file);
        assert_eq!(store.get_chunk(&chunk.id).unwrap(), chunk);
        assert_eq!(store.get_index(INDEX_ID).unwrap(), index);
        assert_eq!(store.get_check_index(CHECK_INDEX_ID).unwrap(), check_index);
        assert_no_temp_files(temp.path());
    }

    #[test]
    fn file_and_chunk_puts_preserve_existing_immutable_objects() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::new(temp.path(), fixture_key()).unwrap();
        let file_path = store.object_path(FILE_ID).unwrap();
        let mut file = fixture_file();
        store.put_file(&file).unwrap();
        let original_file_bytes = fs::read(&file_path).unwrap();
        file.path = "/replacement.md".to_owned();
        store.put_file(&file).unwrap();

        let chunk_path = store.object_path(CHUNK_ID).unwrap();
        write_fixture(&chunk_path, b"existing object bytes");

        store
            .put_chunk(&Chunk {
                id: CHUNK_ID.to_owned(),
                data: b"replacement".to_vec(),
            })
            .unwrap();

        assert_eq!(fs::read(file_path).unwrap(), original_file_bytes);
        assert_eq!(fs::read(chunk_path).unwrap(), b"existing object bytes");
    }

    #[test]
    fn index_write_sets_created_millisecond_mtime() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::new(temp.path(), fixture_key()).unwrap();
        let index = fixture_index();

        store.put_index(&index).unwrap();

        let modified = filetime::FileTime::from_last_modification_time(
            &fs::metadata(store.index_path(INDEX_ID).unwrap()).unwrap(),
        );
        let actual_millis =
            modified.unix_seconds() * 1_000 + i64::from(modified.nanoseconds() / 1_000_000);
        assert_eq!(actual_millis, index.created);
    }

    #[test]
    fn index_mtime_failure_does_not_publish_replacement() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::new(temp.path(), fixture_key()).unwrap();
        let original = fixture_index();
        store.put_index(&original).unwrap();
        let path = store.index_path(INDEX_ID).unwrap();
        let original_bytes = fs::read(&path).unwrap();
        let mut replacement = original.clone();
        replacement.memo = "must not publish".to_owned();

        assert!(matches!(
            store.put_index_with_mtime(&replacement, |_path, _mtime| {
                Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "injected mtime failure",
                ))
            }),
            Err(RepoError::Io(_))
        ));

        assert_eq!(fs::read(&path).unwrap(), original_bytes);
        assert_eq!(store.get_index(INDEX_ID).unwrap(), original);
        assert_no_temp_files(temp.path());
    }

    #[test]
    fn immutable_store_publication_race_has_one_winner_without_overwrite() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::new(temp.path(), fixture_key()).unwrap();
        let path = store.object_path(CHUNK_ID).unwrap();
        let first = store.stage_object(&path, b"first").unwrap();
        let second = store.stage_object(&path, b"second").unwrap();
        let barrier = Arc::new(Barrier::new(2));

        let first_barrier = Arc::clone(&barrier);
        let first_thread = std::thread::spawn(move || {
            first_barrier.wait();
            (first.publish_no_replace().unwrap(), b"first".as_slice())
        });
        let second_thread = std::thread::spawn(move || {
            barrier.wait();
            (second.publish_no_replace().unwrap(), b"second".as_slice())
        });
        let first_result = first_thread.join().unwrap();
        let second_result = second_thread.join().unwrap();

        let outcomes = [first_result.0, second_result.0];
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| **outcome == PublishOutcome::Published)
                .count(),
            1
        );
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| **outcome == PublishOutcome::AlreadyExists)
                .count(),
            1
        );
        let winning_bytes = if first_result.0 == PublishOutcome::Published {
            first_result.1
        } else {
            second_result.1
        };
        assert_eq!(fs::read(path).unwrap(), winning_bytes);
        assert_no_temp_files(temp.path());
    }

    #[cfg(unix)]
    #[test]
    fn store_publication_stays_confined_to_the_opened_repository_handle() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let repository = temp.path().join("repo");
        let held_repository = temp.path().join("repo-held");
        let outside = temp.path().join("outside");
        fs::create_dir(&repository).unwrap();
        fs::create_dir(&outside).unwrap();
        let store = Store::new(&repository, fixture_key()).unwrap();
        fs::rename(&repository, &held_repository).unwrap();
        symlink(&outside, &repository).unwrap();

        let chunk_id = crate::sha1_hex(b"confined");
        store
            .put_chunk(&Chunk {
                id: chunk_id.clone(),
                data: b"confined".to_vec(),
            })
            .unwrap();

        assert_eq!(store.get_chunk(&chunk_id).unwrap().data, b"confined");
        assert!(held_repository
            .join(format!("objects/{}", &chunk_id[..2]))
            .is_dir());
        assert!(!outside.join("objects").exists());
    }

    #[test]
    fn repository_root_is_materialized_and_retained_on_open() {
        let temp = tempfile::tempdir().unwrap();
        let repository = temp.path().join("repo");
        let held_repository = temp.path().join("repo-held");
        let store = Store::new(&repository, fixture_key()).unwrap();
        assert!(repository.is_dir());

        let first_id = crate::sha1_hex(b"first");
        store
            .put_chunk(&Chunk {
                id: first_id.clone(),
                data: b"first".to_vec(),
            })
            .unwrap();
        fs::rename(&repository, &held_repository).unwrap();
        fs::create_dir(&repository).unwrap();

        let second_id = crate::sha1_hex(b"second");
        store
            .put_chunk(&Chunk {
                id: second_id.clone(),
                data: b"second".to_vec(),
            })
            .unwrap();

        assert_eq!(store.get_chunk(&first_id).unwrap().data, b"first");
        assert_eq!(store.get_chunk(&second_id).unwrap().data, b"second");
        assert!(held_repository
            .join(format!("objects/{}", &first_id[..2]))
            .is_dir());
        assert!(held_repository
            .join(format!("objects/{}", &second_id[..2]))
            .is_dir());
        assert_eq!(fs::read_dir(&repository).unwrap().count(), 0);
    }

    #[test]
    fn concurrent_first_use_converges_on_one_retained_repository_root() {
        let temp = tempfile::tempdir().unwrap();
        let repository = temp.path().join("repo");
        let store = Arc::new(Store::new(&repository, fixture_key()).unwrap());
        let barrier = Arc::new(Barrier::new(2));

        let first_store = Arc::clone(&store);
        let first_barrier = Arc::clone(&barrier);
        let first_id = crate::sha1_hex(b"first");
        let second_id = crate::sha1_hex(b"second");
        let first_thread_id = first_id.clone();
        let first = std::thread::spawn(move || {
            first_barrier.wait();
            first_store.put_chunk(&Chunk {
                id: first_thread_id,
                data: b"first".to_vec(),
            })
        });
        let second_store = Arc::clone(&store);
        let second_thread_id = second_id.clone();
        let second = std::thread::spawn(move || {
            barrier.wait();
            second_store.put_chunk(&Chunk {
                id: second_thread_id,
                data: b"second".to_vec(),
            })
        });

        let first_result = first.join().unwrap();
        let second_result = second.join().unwrap();
        assert_eq!(
            [first_result.as_ref(), second_result.as_ref()]
                .into_iter()
                .filter(|result| result.is_ok())
                .count(),
            1
        );
        assert_eq!(
            [first_result.as_ref(), second_result.as_ref()]
                .into_iter()
                .filter(|result| matches!(result, Err(RepoError::RepositoryBusy)))
                .count(),
            1
        );
        store
            .put_chunk(&Chunk {
                id: first_id.clone(),
                data: b"first".to_vec(),
            })
            .unwrap();
        store
            .put_chunk(&Chunk {
                id: second_id.clone(),
                data: b"second".to_vec(),
            })
            .unwrap();

        assert_eq!(store.get_chunk(&first_id).unwrap().data, b"first");
        assert_eq!(store.get_chunk(&second_id).unwrap().data, b"second");
    }

    #[test]
    fn chunk_decode_limit_rejects_one_byte_over() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::new(temp.path(), fixture_key()).unwrap();

        let chunk_frame = zstd_rle_frame_with_content_size(MAX_CHUNK_DECODED_SIZE + 1);
        let encrypted_chunk = crate::encrypt(&chunk_frame, &fixture_key()).unwrap();
        write_fixture(&store.object_path(CHUNK_ID).unwrap(), &encrypted_chunk);
        assert!(matches!(
            store.get_chunk(CHUNK_ID),
            Err(RepoError::DecodedSizeLimitExceeded { limit })
                if limit == MAX_CHUNK_DECODED_SIZE
        ));
    }

    #[test]
    fn content_size_missing_stream_rejects_one_byte_over_limit() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::new(temp.path(), fixture_key()).unwrap();
        let frame = zstd_rle_frame_without_content_size(1_025);
        assert_eq!(
            zstd::zstd_safe::get_frame_content_size(&frame).unwrap(),
            None
        );

        assert!(matches!(
            store.decompress(&frame, 1_024),
            Err(RepoError::DecodedSizeLimitExceeded { limit: 1_024 })
        ));
    }

    #[test]
    fn decoder_rejects_frames_requiring_a_window_over_512_kib() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::new(temp.path(), fixture_key()).unwrap();
        let frame = zstd_rle_frame_with_content_size(600_000);
        write_fixture(&store.index_path(INDEX_ID).unwrap(), &frame);

        assert!(matches!(
            store.get_index(INDEX_ID),
            Err(RepoError::InvalidData(
                "zstd frame is invalid or requires a window larger than 512 KiB"
            ))
        ));
    }

    #[test]
    fn compressed_reader_preserves_a_real_source_io_error_after_partial_input() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::new(temp.path(), fixture_key()).unwrap();
        let compressed = store
            .compress(&serde_json::to_vec(&fixture_index()).unwrap())
            .unwrap();
        let fail_after = u64::try_from((compressed.len() / 2).max(1)).unwrap();
        let reader = FailingReader {
            inner: std::io::Cursor::new(compressed),
            fail_after,
        };

        let result: Result<Index, RepoError> = store.deserialize_compressed_reader(reader);

        assert!(matches!(
            result,
            Err(RepoError::Io(error)) if error.kind() == std::io::ErrorKind::ConnectionReset
        ));
    }

    #[test]
    fn missing_objects_map_to_typed_not_found() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::new(temp.path(), fixture_key()).unwrap();

        assert!(matches!(
            store.get_chunk(CHUNK_ID),
            Err(RepoError::NotFound(id)) if id == CHUNK_ID
        ));
    }

    #[test]
    fn wrong_key_and_truncated_object_are_typed_decryption_failures() {
        let temp = tempfile::tempdir().unwrap();
        let good_store = Store::new(temp.path(), fixture_key()).unwrap();
        let path = good_store.object_path(FILE_ID).unwrap();
        write_fixture(
            &path,
            include_bytes!("../tests/fixtures/golden/file-object.bin"),
        );

        let wrong_store = Store::new(temp.path(), [0_u8; 32]).unwrap();
        assert!(matches!(
            wrong_store.get_file(FILE_ID),
            Err(RepoError::DecryptionFailed)
        ));

        fs::write(path, vec![0_u8; 27]).unwrap();
        assert!(matches!(
            good_store.get_file(FILE_ID),
            Err(RepoError::DecryptionFailed)
        ));
    }

    #[test]
    fn normal_getters_bound_raw_reads_and_validate_embedded_identity() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::new(temp.path(), fixture_key()).unwrap();
        write_fixture(
            &store.object_path(CHUNK_ID).unwrap(),
            &vec![0_u8; super::raw_limit(super::RawObjectKind::Chunk).unwrap() + 1],
        );
        assert!(matches!(
            store.get_chunk(CHUNK_ID),
            Err(RepoError::DecodedSizeLimitExceeded { .. })
        ));

        let mut mismatched = fixture_file();
        mismatched.id = SECOND_CHUNK_ID.to_owned();
        let encoded = store
            .encode_encrypted(&serde_json::to_vec(&mismatched).unwrap())
            .unwrap();
        write_fixture(&store.object_path(FILE_ID).unwrap(), &encoded);
        assert!(matches!(
            store.get_file(FILE_ID),
            Err(RepoError::InvalidData(_))
        ));
    }

    #[test]
    fn raw_import_rejects_different_valid_payloads_at_existing_file_and_index_keys() {
        let first_root = tempfile::tempdir().unwrap();
        let second_root = tempfile::tempdir().unwrap();
        let target_root = tempfile::tempdir().unwrap();
        let first = Store::new(first_root.path(), fixture_key()).unwrap();
        let second = Store::new(second_root.path(), fixture_key()).unwrap();
        let target = Store::new(target_root.path(), fixture_key()).unwrap();

        let first_file = fixture_file();
        let mut second_file = first_file.clone();
        second_file.chunks = vec![SECOND_CHUNK_ID.to_owned()];
        first.put_file(&first_file).unwrap();
        second.put_file(&second_file).unwrap();
        let first_file_raw = first
            .export_raw(super::RawObjectKind::File, FILE_ID)
            .unwrap();
        let second_file_raw = second
            .export_raw(super::RawObjectKind::File, FILE_ID)
            .unwrap();
        target
            .import_raw(super::RawObjectKind::File, FILE_ID, &first_file_raw)
            .unwrap();
        assert!(matches!(
            target.import_raw(super::RawObjectKind::File, FILE_ID, &second_file_raw),
            Err(RepoError::InvalidData(_))
        ));

        let first_index = fixture_index();
        let mut second_index = first_index.clone();
        second_index.memo = "different but valid".to_owned();
        first.put_index(&first_index).unwrap();
        second.put_index(&second_index).unwrap();
        let first_index_raw = first
            .export_raw(super::RawObjectKind::Index, INDEX_ID)
            .unwrap();
        let second_index_raw = second
            .export_raw(super::RawObjectKind::Index, INDEX_ID)
            .unwrap();
        target
            .import_raw(super::RawObjectKind::Index, INDEX_ID, &first_index_raw)
            .unwrap();
        assert!(matches!(
            target.import_raw(super::RawObjectKind::Index, INDEX_ID, &second_index_raw),
            Err(RepoError::InvalidData(_))
        ));
    }

    #[test]
    fn staged_import_race_compares_the_winner_to_the_validated_stage_not_mutated_source() {
        let first_root = tempfile::tempdir().unwrap();
        let second_root = tempfile::tempdir().unwrap();
        let target_root = tempfile::tempdir().unwrap();
        let source_root = tempfile::tempdir().unwrap();
        let first = Store::new(first_root.path(), fixture_key()).unwrap();
        let second = Store::new(second_root.path(), fixture_key()).unwrap();
        let target = Store::new(target_root.path(), fixture_key()).unwrap();
        let first_index = fixture_index();
        let mut second_index = first_index.clone();
        second_index.memo = "different but valid".to_owned();
        first.put_index(&first_index).unwrap();
        second.put_index(&second_index).unwrap();
        let first_raw = first
            .export_raw(super::RawObjectKind::Index, INDEX_ID)
            .unwrap();
        let second_raw = second
            .export_raw(super::RawObjectKind::Index, INDEX_ID)
            .unwrap();
        let source_dir = Dir::open_ambient_dir(source_root.path(), ambient_authority()).unwrap();
        let source = stage_cap_file(&source_dir, OsStr::new("source"), &first_raw, 0o600).unwrap();

        let result = target.import_raw_staged_with_before_publish_unlocked(
            super::RawObjectKind::Index,
            INDEX_ID,
            source.file(),
            || {
                target.import_raw_unlocked(super::RawObjectKind::Index, INDEX_ID, &second_raw)?;
                let mut source_writer = source.file().try_clone()?;
                source_writer.seek(SeekFrom::Start(0))?;
                source_writer.set_len(0)?;
                source_writer.write_all(&second_raw)?;
                source_writer.sync_all()?;
                Ok(())
            },
        );

        assert!(matches!(
            result,
            Err(RepoError::InvalidData(
                "immutable raw object already exists with different bytes"
            ))
        ));
        assert_eq!(
            target
                .export_raw(super::RawObjectKind::Index, INDEX_ID)
                .unwrap(),
            second_raw
        );
    }

    #[tokio::test]
    async fn upload_source_retries_keep_the_validated_file_identity_after_path_replacement() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::new(root.path(), fixture_key()).unwrap();
        let first_index = fixture_index();
        store.put_index(&first_index).unwrap();
        let first_raw = store
            .export_raw(super::RawObjectKind::Index, INDEX_ID)
            .unwrap();
        let mut second_raw = None;
        'memo: for index in 0..first_index.memo.len() {
            for replacement in b'a'..=b'z' {
                let mut memo = first_index.memo.as_bytes().to_vec();
                if memo[index] == replacement {
                    continue;
                }
                memo[index] = replacement;
                let mut candidate = first_index.clone();
                candidate.memo = String::from_utf8(memo).unwrap();
                let candidate_raw = store
                    .compress(&serde_json::to_vec(&candidate).unwrap())
                    .unwrap();
                if candidate_raw.len() == first_raw.len() && candidate_raw != first_raw {
                    second_raw = Some(candidate_raw);
                    break 'memo;
                }
            }
        }
        let second_raw = second_raw.expect("fixture must have a same-length valid replacement");
        assert_ne!(first_raw, second_raw);
        assert_eq!(first_raw.len(), second_raw.len());
        store
            .validate_raw(super::RawObjectKind::Index, INDEX_ID, &second_raw)
            .unwrap();
        let source = {
            let _operation = store.lock_operation().unwrap();
            store
                .open_raw_upload_source_unlocked(super::RawObjectKind::Index, INDEX_ID)
                .unwrap()
        };
        write_file_safer(&store.index_path(INDEX_ID).unwrap(), &second_raw, 0o644).unwrap();

        for _attempt in 0..2 {
            let mut reader = source.open().unwrap();
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).await.unwrap();
            assert_eq!(bytes, first_raw);
        }
    }

    #[test]
    fn indexes_are_zstd_only_while_data_objects_are_encrypted() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::new(temp.path(), fixture_key()).unwrap();
        let file = fixture_file();
        let index = fixture_index();
        let check_index = fixture_check_index();

        store.put_file(&file).unwrap();
        store.put_index(&index).unwrap();
        store.put_check_index(&check_index).unwrap();

        let zstd_magic = [0x28, 0xb5, 0x2f, 0xfd];
        let file_bytes = fs::read(store.object_path(FILE_ID).unwrap()).unwrap();
        let index_bytes = fs::read(store.index_path(INDEX_ID).unwrap()).unwrap();
        let check_index_bytes = fs::read(store.check_index_path(CHECK_INDEX_ID).unwrap()).unwrap();
        assert_ne!(&file_bytes[..4], zstd_magic);
        assert_eq!(&index_bytes[..4], zstd_magic);
        assert_eq!(&check_index_bytes[..4], zstd_magic);
        assert_eq!(index_bytes[4] & 0x04, 0);
        assert_eq!(check_index_bytes[4] & 0x04, 0);
    }

    #[test]
    fn large_zstd_frames_use_a_512_kib_window() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::new(temp.path(), fixture_key()).unwrap();
        let mut index = fixture_index();
        index.memo = "x".repeat(600_000);

        store.put_index(&index).unwrap();

        let frame = fs::read(store.index_path(INDEX_ID).unwrap()).unwrap();
        assert_eq!(zstd_window_size(&frame), Some(512 * 1024));
    }
}
