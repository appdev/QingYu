use std::io::{BufReader, Cursor, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::ambient_authority;
use cap_std::fs::{Dir, OpenOptions};

use crate::atomic_write::{stage_cap_file, CapStagedFile};
use crate::crypto::{decrypt, encrypt};
use crate::{CheckIndex, Chunk, File, Index, RepoError};

const OBJECT_MODE: u32 = 0o644;
const ZSTD_WINDOW_LOG: u32 = 19;
const MAX_CHUNK_DECODED_SIZE: usize = 8 * 1024 * 1024;
const MAX_FILE_DECODED_SIZE: usize = 64 * 1024 * 1024;
const MAX_INDEX_DECODED_SIZE: usize = 512 * 1024 * 1024;
const MAX_CHECK_INDEX_DECODED_SIZE: usize = 512 * 1024 * 1024;

pub struct Store {
    root: PathBuf,
    anchor: Dir,
    relative_root: PathBuf,
    key: [u8; 32],
    compressor: Mutex<zstd::bulk::Compressor<'static>>,
    decompressor: Mutex<zstd::zstd_safe::DCtx<'static>>,
}

impl Store {
    pub fn new(root: impl Into<PathBuf>, key: [u8; 32]) -> Result<Self, RepoError> {
        let root = absolute_lexical_root(root.into())?;
        let (anchor, relative_root) = store_anchor(&root)?;
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

    pub fn put_chunk(&self, chunk: &Chunk) -> Result<(), RepoError> {
        let path = self.object_path(&chunk.id)?;
        let encoded = self.encode_encrypted(&chunk.data)?;
        self.write_immutable_object(&path, &encoded)
    }

    pub fn get_chunk(&self, id: &str) -> Result<Chunk, RepoError> {
        let path = self.object_path(id)?;
        let encoded = self.read_object(&path, id)?;
        Ok(Chunk {
            id: id.to_owned(),
            data: self.decode_encrypted(&encoded, MAX_CHUNK_DECODED_SIZE)?,
        })
    }

    pub fn put_file(&self, file: &File) -> Result<(), RepoError> {
        let path = self.object_path(&file.id)?;
        let json = serde_json::to_vec(file)?;
        let encoded = self.encode_encrypted(&json)?;
        self.write_immutable_object(&path, &encoded)
    }

    pub fn get_file(&self, id: &str) -> Result<File, RepoError> {
        let path = self.object_path(id)?;
        let encoded = self.read_object(&path, id)?;
        let json = self.decode_encrypted(&encoded, MAX_FILE_DECODED_SIZE)?;
        Ok(serde_json::from_slice(&json)?)
    }

    pub fn put_index(&self, index: &Index) -> Result<(), RepoError> {
        self.put_index_with_mtime(index, |file, mtime| {
            let standard_file = file.try_clone()?.into_std();
            filetime::set_file_handle_times(&standard_file, None, Some(mtime))
        })
    }

    fn put_index_with_mtime<F>(&self, index: &Index, set_mtime: F) -> Result<(), RepoError>
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
        let path = self.index_path(id)?;
        let encoded = self.read_object(&path, id)?;
        let json = self.decompress(&encoded, MAX_INDEX_DECODED_SIZE)?;
        Ok(serde_json::from_slice(&json)?)
    }

    pub fn put_check_index(&self, check_index: &CheckIndex) -> Result<(), RepoError> {
        let path = self.check_index_path(&check_index.id)?;
        let json = serde_json::to_vec(check_index)?;
        let encoded = self.compress(&json)?;
        self.write_object(&path, &encoded)
    }

    pub fn get_check_index(&self, id: &str) -> Result<CheckIndex, RepoError> {
        let path = self.check_index_path(id)?;
        let encoded = self.read_object(&path, id)?;
        let json = self.decompress(&encoded, MAX_CHECK_INDEX_DECODED_SIZE)?;
        Ok(serde_json::from_slice(&json)?)
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

    fn read_object(&self, path: &Path, id: &str) -> Result<Vec<u8>, RepoError> {
        let relative = self.relative_store_path(path)?;
        let name = relative
            .file_name()
            .ok_or(RepoError::InvalidData("object path must have a file name"))?;
        let parent = self
            .open_directory(relative.parent().unwrap_or_else(|| Path::new("")), false)
            .map_err(|error| map_not_found(error, id))?;
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        let mut file = parent
            .open_with(name, &options)
            .map_err(|error| map_object_io(error, id))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    fn relative_store_path<'a>(&self, path: &'a Path) -> Result<&'a Path, RepoError> {
        let relative = path
            .strip_prefix(&self.root)
            .map_err(|_| RepoError::UnsafePath)?;
        validate_relative_path(relative)?;
        Ok(relative)
    }

    fn open_directory(&self, relative: &Path, create: bool) -> Result<Dir, RepoError> {
        let mut directory = self.anchor.try_clone()?;
        for component in self.relative_root.components().chain(relative.components()) {
            let Component::Normal(name) = component else {
                return Err(RepoError::UnsafePath);
            };
            directory = open_child_directory(&directory, name, create)?;
        }
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

    fn compress(&self, bytes: &[u8]) -> Result<Vec<u8>, RepoError> {
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

fn validate_id(id: &str) -> Result<(), RepoError> {
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

fn absolute_lexical_root(root: PathBuf) -> Result<PathBuf, RepoError> {
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

fn store_anchor(root: &Path) -> Result<(Dir, PathBuf), RepoError> {
    let mut existing = root.to_path_buf();
    let mut missing = Vec::new();
    loop {
        match std::fs::symlink_metadata(&existing) {
            Ok(metadata) => {
                if !metadata.file_type().is_dir() || unsafe_store_root_metadata(&metadata) {
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

fn unsafe_store_root_metadata(metadata: &std::fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        false
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
    for name in names {
        directory = directory
            .open_dir_nofollow(&name)
            .map_err(|error| map_nofollow_error(&directory, &name, error))?;
    }
    Ok(directory)
}

fn open_child_directory(
    parent: &Dir,
    name: &std::ffi::OsStr,
    create: bool,
) -> Result<Dir, RepoError> {
    match parent.open_dir_nofollow(name) {
        Ok(directory) => Ok(directory),
        Err(error) if create && error.kind() == std::io::ErrorKind::NotFound => {
            match parent.create_dir(name) {
                Ok(()) => {}
                Err(create_error) if create_error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(create_error) => return Err(create_error.into()),
            }
            parent
                .open_dir_nofollow(name)
                .map_err(|open_error| map_nofollow_error(parent, name, open_error))
        }
        Err(error) => Err(map_nofollow_error(parent, name, error)),
    }
}

fn map_nofollow_error(parent: &Dir, name: &std::ffi::OsStr, error: std::io::Error) -> RepoError {
    match parent.symlink_metadata(name) {
        Ok(metadata) if metadata.file_type().is_symlink() => RepoError::UnsafePath,
        _ => RepoError::Io(error),
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
    use std::fs;
    use std::path::Path;
    use std::sync::{Arc, Barrier};

    use super::{
        Store, MAX_CHECK_INDEX_DECODED_SIZE, MAX_CHUNK_DECODED_SIZE, MAX_FILE_DECODED_SIZE,
        MAX_INDEX_DECODED_SIZE,
    };
    use crate::atomic_write::PublishOutcome;
    use crate::{CheckIndex, CheckIndexFile, Chunk, File, Index, RepoError};

    const FILE_ID: &str = "0123456789abcdef0123456789abcdef01234567";
    const INDEX_ID: &str = "89abcdef0123456789abcdef0123456789abcdef";
    const CHECK_INDEX_ID: &str = "fedcba9876543210fedcba9876543210fedcba98";
    const CHUNK_ID: &str = "1111111111111111111111111111111111111111";

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
                .join("01")
                .join("23456789abcdef0123456789abcdef01234567")
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
            &store.object_path(FILE_ID).unwrap(),
            include_bytes!("../tests/fixtures/golden/file-object.bin"),
        );

        let file = store.get_file(FILE_ID).unwrap();

        assert_eq!(file.id, FILE_ID);
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
        assert_eq!(index.files, vec![FILE_ID.to_owned()]);
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
            id: CHUNK_ID.to_owned(),
            data: b"raw chunk bytes".to_vec(),
        };
        let index = fixture_index();
        let check_index = fixture_check_index();

        store.put_file(&file).unwrap();
        store.put_chunk(&chunk).unwrap();
        store.put_index(&index).unwrap();
        store.put_check_index(&check_index).unwrap();

        assert_eq!(store.get_file(FILE_ID).unwrap(), file);
        assert_eq!(store.get_chunk(CHUNK_ID).unwrap(), chunk);
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

        store
            .put_chunk(&Chunk {
                id: CHUNK_ID.to_owned(),
                data: b"confined".to_vec(),
            })
            .unwrap();

        assert_eq!(store.get_chunk(CHUNK_ID).unwrap().data, b"confined");
        assert!(held_repository.join("objects/11").is_dir());
        assert!(!outside.join("objects").exists());
    }

    #[test]
    fn object_type_decode_limits_reject_one_byte_over() {
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

        let file_frame = zstd_rle_frame_with_content_size(MAX_FILE_DECODED_SIZE + 1);
        let encrypted_file = crate::encrypt(&file_frame, &fixture_key()).unwrap();
        write_fixture(&store.object_path(FILE_ID).unwrap(), &encrypted_file);
        assert!(matches!(
            store.get_file(FILE_ID),
            Err(RepoError::DecodedSizeLimitExceeded { limit })
                if limit == MAX_FILE_DECODED_SIZE
        ));

        let index_frame = zstd_rle_frame_with_content_size(MAX_INDEX_DECODED_SIZE + 1);
        write_fixture(&store.index_path(INDEX_ID).unwrap(), &index_frame);
        assert!(matches!(
            store.get_index(INDEX_ID),
            Err(RepoError::DecodedSizeLimitExceeded { limit })
                if limit == MAX_INDEX_DECODED_SIZE
        ));

        write_fixture(
            &store.check_index_path(CHECK_INDEX_ID).unwrap(),
            &index_frame,
        );
        assert!(matches!(
            store.get_check_index(CHECK_INDEX_ID),
            Err(RepoError::DecodedSizeLimitExceeded { limit })
                if limit == MAX_CHECK_INDEX_DECODED_SIZE
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
