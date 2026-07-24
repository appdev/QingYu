use std::fs;
use std::io::{BufReader, Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use crate::atomic_write::write_file_safer;
use crate::crypto::{decrypt, encrypt};
use crate::{CheckIndex, Chunk, File, Index, RepoError};

const OBJECT_MODE: u32 = 0o644;
const ZSTD_WINDOW_LOG: u32 = 19;

pub struct Store {
    root: PathBuf,
    key: [u8; 32],
    compressor: Mutex<zstd::bulk::Compressor<'static>>,
    decompressor: Mutex<zstd::zstd_safe::DCtx<'static>>,
}

impl Store {
    pub fn new(root: impl Into<PathBuf>, key: [u8; 32]) -> Result<Self, RepoError> {
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
            root: root.into(),
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
        if path.exists() {
            return Ok(());
        }

        let encoded = self.encode_encrypted(&chunk.data)?;
        self.write_object(&path, &encoded)
    }

    pub fn get_chunk(&self, id: &str) -> Result<Chunk, RepoError> {
        let path = self.object_path(id)?;
        let encoded = read_object(&path, id)?;
        Ok(Chunk {
            id: id.to_owned(),
            data: self.decode_encrypted(&encoded)?,
        })
    }

    pub fn put_file(&self, file: &File) -> Result<(), RepoError> {
        let path = self.object_path(&file.id)?;
        if path.exists() {
            return Ok(());
        }

        let json = serde_json::to_vec(file)?;
        let encoded = self.encode_encrypted(&json)?;
        self.write_object(&path, &encoded)
    }

    pub fn get_file(&self, id: &str) -> Result<File, RepoError> {
        let path = self.object_path(id)?;
        let encoded = read_object(&path, id)?;
        let json = self.decode_encrypted(&encoded)?;
        Ok(serde_json::from_slice(&json)?)
    }

    pub fn put_index(&self, index: &Index) -> Result<(), RepoError> {
        let path = self.index_path(&index.id)?;
        let json = serde_json::to_vec(index)?;
        let encoded = self.compress(&json)?;
        self.write_object(&path, &encoded)?;

        let seconds = index.created.div_euclid(1_000);
        let nanos = index.created.rem_euclid(1_000) as u32 * 1_000_000;
        filetime::set_file_mtime(&path, filetime::FileTime::from_unix_time(seconds, nanos))?;
        Ok(())
    }

    pub fn get_index(&self, id: &str) -> Result<Index, RepoError> {
        let path = self.index_path(id)?;
        let encoded = read_object(&path, id)?;
        let json = self.decompress(&encoded)?;
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
        let encoded = read_object(&path, id)?;
        let json = self.decompress(&encoded)?;
        Ok(serde_json::from_slice(&json)?)
    }

    fn write_object(&self, path: &Path, bytes: &[u8]) -> Result<(), RepoError> {
        let parent = path
            .parent()
            .ok_or(RepoError::InvalidData("object path must have a parent"))?;
        fs::create_dir_all(parent)?;
        write_file_safer(path, bytes, OBJECT_MODE)
    }

    fn encode_encrypted(&self, bytes: &[u8]) -> Result<Vec<u8>, RepoError> {
        let compressed = self.compress(bytes)?;
        encrypt(&compressed, &self.key)
    }

    fn decode_encrypted(&self, bytes: &[u8]) -> Result<Vec<u8>, RepoError> {
        let compressed = decrypt(bytes, &self.key)?;
        self.decompress(&compressed)
    }

    fn compress(&self, bytes: &[u8]) -> Result<Vec<u8>, RepoError> {
        self.lock_compressor()?
            .compress(bytes)
            .map_err(RepoError::Compression)
    }

    fn decompress(&self, bytes: &[u8]) -> Result<Vec<u8>, RepoError> {
        let mut context = self.lock_decompressor()?;
        let reader = BufReader::new(Cursor::new(bytes));
        let mut decoder = zstd::stream::read::Decoder::with_context(reader, &mut context);
        let mut decoded = Vec::new();
        decoder
            .read_to_end(&mut decoded)
            .map_err(RepoError::Compression)?;
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

fn read_object(path: &Path, id: &str) -> Result<Vec<u8>, RepoError> {
    fs::read(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            RepoError::NotFound(id.to_owned())
        } else {
            RepoError::Io(error)
        }
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::Store;
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
