// DejaVu - Data snapshot and sync.
// Copyright (c) 2022-present, b3log.org
// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::HashSet;
use std::path::Path;

use cap_fs_ext::DirExt;
use cap_std::fs::Dir;

use crate::path_security::cap_metadata_is_reparse;
use crate::store::validate_id;
use crate::{RefStore, RepoError, Store};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PurgeStat {
    pub objects: usize,
    pub indexes: usize,
    pub size: i64,
}

pub(crate) fn purge_store_with_cancel_check<F>(
    store: &Store,
    retained_index_ids: &[String],
    mut is_cancelled: F,
) -> Result<PurgeStat, RepoError>
where
    F: FnMut() -> bool,
{
    check_cancelled(&mut is_cancelled)?;
    let Some(object_ids) = collect_object_ids(store)? else {
        return Ok(PurgeStat::default());
    };
    check_cancelled(&mut is_cancelled)?;

    let index_ids = collect_flat_ids(store, Path::new("indexes"))?;
    check_cancelled(&mut is_cancelled)?;

    let mut referenced_index_ids = RefStore::new(store).all_index_ids()?;
    for id in retained_index_ids {
        validate_id(id)?;
        referenced_index_ids.insert(id.clone());
    }
    check_cancelled(&mut is_cancelled)?;

    let mut referenced_object_ids = HashSet::new();
    let mut ordered_references = referenced_index_ids.iter().collect::<Vec<_>>();
    ordered_references.sort();
    for index_id in ordered_references {
        check_cancelled(&mut is_cancelled)?;
        let index = match store.get_index(index_id) {
            Ok(index) => index,
            Err(RepoError::UnsafePath) => return Err(RepoError::UnsafePath),
            Err(_) => continue,
        };
        for file_id in index.files {
            validate_id(&file_id)?;
            referenced_object_ids.insert(file_id.clone());
            let file = match store.get_file(&file_id) {
                Ok(file) => file,
                Err(RepoError::UnsafePath) => return Err(RepoError::UnsafePath),
                Err(_) => continue,
            };
            for chunk_id in file.chunks {
                validate_id(&chunk_id)?;
                referenced_object_ids.insert(chunk_id);
            }
        }
    }
    check_cancelled(&mut is_cancelled)?;

    let mut unreferenced_index_ids = index_ids
        .difference(&referenced_index_ids)
        .cloned()
        .collect::<Vec<_>>();
    unreferenced_index_ids.sort();
    let mut unreferenced_object_ids = object_ids
        .difference(&referenced_object_ids)
        .cloned()
        .collect::<Vec<_>>();
    unreferenced_object_ids.sort();
    let unreferenced_indexes = unreferenced_index_ids
        .iter()
        .cloned()
        .collect::<HashSet<_>>();

    let mut stat = PurgeStat::default();
    check_cancelled(&mut is_cancelled)?;
    for id in &unreferenced_index_ids {
        check_cancelled(&mut is_cancelled)?;
        remove_flat_file(store, Path::new("indexes"), id)?;
        stat.indexes += 1;
    }

    check_cancelled(&mut is_cancelled)?;
    let mut check_index_ids = collect_flat_ids(store, Path::new("check/indexes"))?
        .into_iter()
        .collect::<Vec<_>>();
    check_index_ids.sort();
    for id in check_index_ids {
        check_cancelled(&mut is_cancelled)?;
        let check_index = match store.get_check_index(&id) {
            Ok(check_index) => check_index,
            Err(_) => continue,
        };
        if unreferenced_indexes.contains(&check_index.index_id) {
            remove_flat_file(store, Path::new("check/indexes"), &id)?;
        }
    }

    check_cancelled(&mut is_cancelled)?;
    for id in unreferenced_object_ids {
        check_cancelled(&mut is_cancelled)?;
        let size = remove_object(store, &id)?;
        stat.objects += 1;
        stat.size = stat
            .size
            .checked_add(size)
            .ok_or(RepoError::InvalidData("purge byte count overflow"))?;
    }
    Ok(stat)
}

fn collect_object_ids(store: &Store) -> Result<Option<HashSet<String>>, RepoError> {
    let objects = match store.open_directory(Path::new("objects"), false) {
        Ok(objects) => objects,
        Err(error) if is_not_found(&error) => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut ids = HashSet::new();
    for entry in objects.entries()? {
        let entry = entry?;
        let prefix = entry.file_name();
        let metadata = objects.symlink_metadata(&prefix)?;
        if cap_metadata_is_reparse(&metadata) || metadata.file_type().is_symlink() {
            return Err(RepoError::UnsafePath);
        }
        let Some(prefix) = prefix.to_str() else {
            continue;
        };
        if !metadata.file_type().is_dir() || !is_lower_hex(prefix, 2) {
            continue;
        }
        let directory = objects
            .open_dir_nofollow(prefix)
            .map_err(|error| map_nofollow_error(&objects, prefix.as_ref(), error))?;
        for object in directory.entries()? {
            let object = object?;
            let name = object.file_name();
            let metadata = directory.symlink_metadata(&name)?;
            if cap_metadata_is_reparse(&metadata) || metadata.file_type().is_symlink() {
                return Err(RepoError::UnsafePath);
            }
            if !metadata.file_type().is_file() {
                continue;
            }
            let Some(name) = name.to_str() else {
                continue;
            };
            let id = format!("{prefix}{name}");
            if validate_id(&id).is_ok() {
                ids.insert(id);
            }
        }
    }
    Ok(Some(ids))
}

fn collect_flat_ids(store: &Store, relative: &Path) -> Result<HashSet<String>, RepoError> {
    let directory = match store.open_directory(relative, false) {
        Ok(directory) => directory,
        Err(error) if is_not_found(&error) => return Ok(HashSet::new()),
        Err(error) => return Err(error),
    };
    let mut ids = HashSet::new();
    for entry in directory.entries()? {
        let entry = entry?;
        let name = entry.file_name();
        let metadata = directory.symlink_metadata(&name)?;
        if cap_metadata_is_reparse(&metadata) || metadata.file_type().is_symlink() {
            return Err(RepoError::UnsafePath);
        }
        if !metadata.file_type().is_file() {
            continue;
        }
        if let Some(id) = name.to_str().filter(|id| validate_id(id).is_ok()) {
            ids.insert(id.to_owned());
        }
    }
    Ok(ids)
}

fn remove_flat_file(store: &Store, relative: &Path, id: &str) -> Result<(), RepoError> {
    validate_id(id)?;
    let directory = store.open_directory(relative, false)?;
    validate_regular_file(&directory, id.as_ref())?;
    directory.remove_file(id)?;
    Ok(())
}

fn remove_object(store: &Store, id: &str) -> Result<i64, RepoError> {
    validate_id(id)?;
    let directory_path = Path::new("objects").join(&id[..2]);
    let directory = store.open_directory(&directory_path, false)?;
    let name = &id[2..];
    let metadata = validate_regular_file(&directory, name.as_ref())?;
    let size = i64::try_from(metadata.len())
        .map_err(|_| RepoError::InvalidData("object size exceeds i64"))?;
    directory.remove_file(name)?;
    Ok(size)
}

fn validate_regular_file(
    directory: &Dir,
    name: &std::ffi::OsStr,
) -> Result<cap_std::fs::Metadata, RepoError> {
    let metadata = directory.symlink_metadata(name)?;
    if !metadata.file_type().is_file() || cap_metadata_is_reparse(&metadata) {
        return Err(RepoError::UnsafePath);
    }
    Ok(metadata)
}

fn check_cancelled<F>(is_cancelled: &mut F) -> Result<(), RepoError>
where
    F: FnMut() -> bool,
{
    if is_cancelled() {
        Err(RepoError::Cancelled)
    } else {
        Ok(())
    }
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn map_nofollow_error(parent: &Dir, name: &std::ffi::OsStr, error: std::io::Error) -> RepoError {
    match parent.symlink_metadata(name) {
        Ok(metadata) if metadata.file_type().is_symlink() || cap_metadata_is_reparse(&metadata) => {
            RepoError::UnsafePath
        }
        _ => RepoError::Io(error),
    }
}

fn is_not_found(error: &RepoError) -> bool {
    matches!(error, RepoError::Io(error) if error.kind() == std::io::ErrorKind::NotFound)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::AtomicBool;

    use tempfile::TempDir;

    use crate::{
        CheckIndex, CheckIndexFile, Chunk, Device, File, Index, RefStore, Repo, RepoError,
        RepoOptions, RepoPaths,
    };

    use super::purge_store_with_cancel_check;

    const RETAINED_INDEX_REF: &str = "1111111111111111111111111111111111111111";
    const RETAINED_INDEX_CALLER: &str = "2222222222222222222222222222222222222222";
    const UNREFERENCED_INDEX: &str = "3333333333333333333333333333333333333333";
    const RETAINED_CHECK_REF: &str = "4444444444444444444444444444444444444444";
    const RETAINED_CHECK_CALLER: &str = "5555555555555555555555555555555555555555";
    const UNREFERENCED_CHECK: &str = "6666666666666666666666666666666666666666";
    const RETAINED_FILE_REF: &str = "7777777777777777777777777777777777777777";
    const RETAINED_FILE_CALLER: &str = "8888888888888888888888888888888888888888";
    const UNREFERENCED_FILE: &str = "9999999999999999999999999999999999999999";
    const SHARED_CHUNK: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const RETAINED_CHUNK_REF: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const RETAINED_CHUNK_CALLER: &str = "cccccccccccccccccccccccccccccccccccccccc";
    const UNREACHABLE_CHUNK: &str = "dddddddddddddddddddddddddddddddddddddddd";

    struct PurgeFixture {
        _temp: TempDir,
        repo: Repo,
        unreachable_encoded_size: i64,
    }

    fn index(id: &str, file_id: &str, check_index_id: &str) -> Index {
        Index {
            id: id.to_owned(),
            memo: "purge fixture".to_owned(),
            created: 1_700_000_000_123,
            files: vec![file_id.to_owned()],
            count: 1,
            size: 1,
            system_id: "device".to_owned(),
            system_name: "QingYu".to_owned(),
            system_os: "test".to_owned(),
            check_index_id: check_index_id.to_owned(),
            aes_key_verify_val: String::new(),
        }
    }

    fn file(id: &str, path: &str, chunks: &[&str]) -> File {
        File {
            id: id.to_owned(),
            path: path.to_owned(),
            size: 1,
            updated: 1_700_000_000_123,
            chunks: chunks.iter().map(|id| (*id).to_owned()).collect(),
        }
    }

    fn check_index(id: &str, index_id: &str, file_id: &str, chunks: &[&str]) -> CheckIndex {
        CheckIndex {
            id: id.to_owned(),
            index_id: index_id.to_owned(),
            files: vec![CheckIndexFile {
                id: file_id.to_owned(),
                chunks: chunks.iter().map(|id| (*id).to_owned()).collect(),
            }],
        }
    }

    fn fixture() -> PurgeFixture {
        let temp = TempDir::new().unwrap();
        let paths = RepoPaths {
            data: temp.path().join("data"),
            repo: temp.path().join("repo"),
            history: temp.path().join("history"),
            temp: temp.path().join("temp"),
        };
        fs::create_dir_all(&paths.data).unwrap();
        let repo = Repo::open(
            paths,
            Device {
                id: "device".to_owned(),
                name: "QingYu".to_owned(),
                os: "test".to_owned(),
            },
            [9; 32],
            RepoOptions::default(),
        )
        .unwrap();

        for (id, data) in [
            (SHARED_CHUNK, b"shared".as_slice()),
            (RETAINED_CHUNK_REF, b"retained-ref".as_slice()),
            (RETAINED_CHUNK_CALLER, b"retained-caller".as_slice()),
            (UNREACHABLE_CHUNK, b"unreachable".as_slice()),
        ] {
            repo.store
                .put_chunk(&Chunk {
                    id: id.to_owned(),
                    data: data.to_vec(),
                })
                .unwrap();
        }

        let retained_ref_file = file(
            RETAINED_FILE_REF,
            "/retained-ref.md",
            &[SHARED_CHUNK, RETAINED_CHUNK_REF],
        );
        let retained_caller_file = file(
            RETAINED_FILE_CALLER,
            "/retained-caller.md",
            &[SHARED_CHUNK, RETAINED_CHUNK_CALLER],
        );
        let unreferenced_file = file(
            UNREFERENCED_FILE,
            "/unreferenced.md",
            &[SHARED_CHUNK, UNREACHABLE_CHUNK],
        );
        for file in [
            &retained_ref_file,
            &retained_caller_file,
            &unreferenced_file,
        ] {
            repo.store.put_file(file).unwrap();
        }

        let retained_ref_index = index(RETAINED_INDEX_REF, RETAINED_FILE_REF, RETAINED_CHECK_REF);
        let retained_caller_index = index(
            RETAINED_INDEX_CALLER,
            RETAINED_FILE_CALLER,
            RETAINED_CHECK_CALLER,
        );
        let unreferenced_index = index(UNREFERENCED_INDEX, UNREFERENCED_FILE, UNREFERENCED_CHECK);
        for index in [
            &retained_ref_index,
            &retained_caller_index,
            &unreferenced_index,
        ] {
            repo.store.put_index(index).unwrap();
        }
        for check in [
            check_index(
                RETAINED_CHECK_REF,
                RETAINED_INDEX_REF,
                RETAINED_FILE_REF,
                &[SHARED_CHUNK, RETAINED_CHUNK_REF],
            ),
            check_index(
                RETAINED_CHECK_CALLER,
                RETAINED_INDEX_CALLER,
                RETAINED_FILE_CALLER,
                &[SHARED_CHUNK, RETAINED_CHUNK_CALLER],
            ),
            check_index(
                UNREFERENCED_CHECK,
                UNREFERENCED_INDEX,
                UNREFERENCED_FILE,
                &[SHARED_CHUNK, UNREACHABLE_CHUNK],
            ),
        ] {
            repo.store.put_check_index(&check).unwrap();
        }
        RefStore::new(&repo.store)
            .update_latest(&retained_ref_index)
            .unwrap();

        let unreachable_encoded_size = [UNREFERENCED_FILE, UNREACHABLE_CHUNK]
            .into_iter()
            .map(|id| {
                fs::metadata(repo.store.object_path(id).unwrap())
                    .unwrap()
                    .len() as i64
            })
            .sum();

        PurgeFixture {
            _temp: temp,
            repo,
            unreachable_encoded_size,
        }
    }

    fn assert_exists(repo: &Repo, id: &str) {
        assert!(repo.store.object_path(id).unwrap().is_file(), "{id}");
    }

    #[test]
    fn purge_preserves_reachable_indexes_files_and_shared_chunks() {
        let fixture = fixture();
        let cancelled = AtomicBool::new(false);

        let stat = fixture
            .repo
            .purge(&[RETAINED_INDEX_CALLER.to_owned()], &cancelled)
            .unwrap();

        assert_eq!(stat.indexes, 1);
        assert_eq!(stat.objects, 2);
        assert_eq!(stat.size, fixture.unreachable_encoded_size);
        assert!(fixture
            .repo
            .store
            .index_path(RETAINED_INDEX_REF)
            .unwrap()
            .is_file());
        assert!(fixture
            .repo
            .store
            .index_path(RETAINED_INDEX_CALLER)
            .unwrap()
            .is_file());
        assert!(!fixture
            .repo
            .store
            .index_path(UNREFERENCED_INDEX)
            .unwrap()
            .exists());
        for id in [
            RETAINED_FILE_REF,
            RETAINED_FILE_CALLER,
            SHARED_CHUNK,
            RETAINED_CHUNK_REF,
            RETAINED_CHUNK_CALLER,
        ] {
            assert_exists(&fixture.repo, id);
        }
        for id in [UNREFERENCED_FILE, UNREACHABLE_CHUNK] {
            assert!(
                !fixture.repo.store.object_path(id).unwrap().exists(),
                "{id}"
            );
        }
        for id in [RETAINED_CHECK_REF, RETAINED_CHECK_CALLER] {
            assert!(fixture.repo.store.check_index_path(id).unwrap().is_file());
        }
        assert!(!fixture
            .repo
            .store
            .check_index_path(UNREFERENCED_CHECK)
            .unwrap()
            .exists());
    }

    #[test]
    fn cancellation_after_index_deletion_stops_later_destructive_loops() {
        let fixture = fixture();
        let unreferenced_index_path = fixture.repo.store.index_path(UNREFERENCED_INDEX).unwrap();

        let result = purge_store_with_cancel_check(&fixture.repo.store, &[], || {
            !unreferenced_index_path.exists()
        });

        assert!(matches!(result, Err(RepoError::Cancelled)));
        assert!(!unreferenced_index_path.exists());
        assert!(fixture
            .repo
            .store
            .check_index_path(UNREFERENCED_CHECK)
            .unwrap()
            .is_file());
        assert_exists(&fixture.repo, UNREFERENCED_FILE);
        assert_exists(&fixture.repo, UNREACHABLE_CHUNK);
    }

    #[test]
    fn already_cancelled_purge_preserves_every_object() {
        let fixture = fixture();
        let cancelled = AtomicBool::new(true);

        assert!(matches!(
            fixture.repo.purge(&[], &cancelled),
            Err(RepoError::Cancelled)
        ));
        assert!(fixture
            .repo
            .store
            .index_path(UNREFERENCED_INDEX)
            .unwrap()
            .is_file());
        assert!(fixture
            .repo
            .store
            .check_index_path(UNREFERENCED_CHECK)
            .unwrap()
            .is_file());
        assert_exists(&fixture.repo, UNREFERENCED_FILE);
        assert_exists(&fixture.repo, UNREACHABLE_CHUNK);
    }
}
