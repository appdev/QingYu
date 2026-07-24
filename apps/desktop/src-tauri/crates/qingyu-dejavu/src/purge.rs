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
    is_cancelled: F,
) -> Result<PurgeStat, RepoError>
where
    F: FnMut() -> bool,
{
    purge_store_with_cancel_check_and_hook(store, retained_index_ids, is_cancelled, || Ok(()))
}

pub(crate) fn purge_store_with_cancel_check_and_hook<F, H>(
    store: &Store,
    retained_index_ids: &[String],
    mut is_cancelled: F,
    mut before_delete: H,
) -> Result<PurgeStat, RepoError>
where
    F: FnMut() -> bool,
    H: FnMut() -> Result<(), RepoError>,
{
    check_cancelled(&mut is_cancelled)?;
    let object_ids = collect_object_ids(store, &mut is_cancelled)?;
    check_cancelled(&mut is_cancelled)?;

    let index_ids = collect_flat_ids(store, Path::new("indexes"), &mut is_cancelled)?;
    check_cancelled(&mut is_cancelled)?;

    let mut referenced_index_ids =
        RefStore::new(store).all_index_ids_unlocked_with_cancel_check(&mut is_cancelled)?;
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
        let index = store.get_index_unlocked(index_id)?;
        for file_id in index.files {
            check_cancelled(&mut is_cancelled)?;
            validate_id(&file_id)?;
            referenced_object_ids.insert(file_id.clone());
            let file = store.get_file_unlocked(&file_id)?;
            for chunk_id in file.chunks {
                check_cancelled(&mut is_cancelled)?;
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

    let mut check_index_ids =
        collect_flat_ids(store, Path::new("check/indexes"), &mut is_cancelled)?
            .into_iter()
            .collect::<Vec<_>>();
    check_index_ids.sort();
    let mut removable_check_index_ids = Vec::new();
    for id in check_index_ids {
        check_cancelled(&mut is_cancelled)?;
        let check_index = store.get_check_index_unlocked(&id)?;
        if check_index.id != id {
            return Err(RepoError::InvalidData(
                "check index payload id must match its filename",
            ));
        }
        validate_id(&check_index.index_id)?;
        if unreferenced_indexes.contains(&check_index.index_id) {
            removable_check_index_ids.push(id);
        }
    }

    let mut stat = PurgeStat::default();
    check_cancelled(&mut is_cancelled)?;
    before_delete()?;
    check_cancelled(&mut is_cancelled)?;
    for id in &unreferenced_index_ids {
        check_cancelled(&mut is_cancelled)?;
        remove_flat_file(store, Path::new("indexes"), id)?;
        stat.indexes += 1;
    }

    check_cancelled(&mut is_cancelled)?;
    for id in removable_check_index_ids {
        check_cancelled(&mut is_cancelled)?;
        remove_flat_file(store, Path::new("check/indexes"), &id)?;
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

fn collect_object_ids<F>(store: &Store, is_cancelled: &mut F) -> Result<HashSet<String>, RepoError>
where
    F: FnMut() -> bool,
{
    check_cancelled(is_cancelled)?;
    let objects = match store.open_directory(Path::new("objects"), false) {
        Ok(objects) => objects,
        Err(error) if is_not_found(&error) => return Ok(HashSet::new()),
        Err(error) => return Err(error),
    };
    let mut ids = HashSet::new();
    for entry in objects.entries()? {
        check_cancelled(is_cancelled)?;
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
            check_cancelled(is_cancelled)?;
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
    Ok(ids)
}

fn collect_flat_ids<F>(
    store: &Store,
    relative: &Path,
    is_cancelled: &mut F,
) -> Result<HashSet<String>, RepoError>
where
    F: FnMut() -> bool,
{
    check_cancelled(is_cancelled)?;
    let directory = match store.open_directory(relative, false) {
        Ok(directory) => directory,
        Err(error) if is_not_found(&error) => return Ok(HashSet::new()),
        Err(error) => return Err(error),
    };
    let mut ids = HashSet::new();
    for entry in directory.entries()? {
        check_cancelled(is_cancelled)?;
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
    use std::path::Path;
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc;
    use std::time::Duration;

    use tempfile::TempDir;

    use crate::{
        CheckIndex, CheckIndexFile, Chunk, Device, File, Index, RefStore, Repo, RepoError,
        RepoOptions, RepoPaths,
    };

    use super::{collect_flat_ids, collect_object_ids, purge_store_with_cancel_check};

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
    const MISMATCH_CHECK_ID: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
    const CONCURRENT_INDEX: &str = "ffffffffffffffffffffffffffffffffffffffff";

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

    fn assert_unreferenced_candidates_untouched(repo: &Repo) {
        assert!(repo.store.index_path(UNREFERENCED_INDEX).unwrap().is_file());
        assert!(repo
            .store
            .check_index_path(UNREFERENCED_CHECK)
            .unwrap()
            .is_file());
        assert_exists(repo, UNREFERENCED_FILE);
        assert_exists(repo, UNREACHABLE_CHUNK);
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

    #[test]
    fn cancellation_is_observed_inside_object_index_and_recursive_ref_collection() {
        let fixture = fixture();
        fs::create_dir_all(fixture._temp.path().join("repo/refs/tags/nested")).unwrap();
        fs::write(
            fixture._temp.path().join("repo/refs/tags/nested/retained"),
            RETAINED_INDEX_REF,
        )
        .unwrap();

        let mut object_checks = 0;
        assert!(matches!(
            collect_object_ids(&fixture.repo.store, &mut || {
                object_checks += 1;
                object_checks == 3
            }),
            Err(RepoError::Cancelled)
        ));
        assert_eq!(object_checks, 3);

        let mut index_checks = 0;
        assert!(matches!(
            collect_flat_ids(&fixture.repo.store, Path::new("indexes"), &mut || {
                index_checks += 1;
                index_checks == 3
            },),
            Err(RepoError::Cancelled)
        ));
        assert_eq!(index_checks, 3);

        let mut ref_checks = 0;
        assert!(matches!(
            RefStore::new(&fixture.repo.store).all_index_ids_unlocked_with_cancel_check(
                &mut || {
                    ref_checks += 1;
                    ref_checks == 3
                }
            ),
            Err(RepoError::Cancelled)
        ));
        assert_eq!(ref_checks, 3);
        assert_unreferenced_candidates_untouched(&fixture.repo);
    }

    #[test]
    fn missing_retained_index_aborts_before_any_deletion() {
        let fixture = fixture();
        fs::remove_file(fixture.repo.store.index_path(RETAINED_INDEX_REF).unwrap()).unwrap();

        assert!(fixture
            .repo
            .purge(&[RETAINED_INDEX_CALLER.to_owned()], &AtomicBool::new(false))
            .is_err());
        assert_unreferenced_candidates_untouched(&fixture.repo);
    }

    #[test]
    fn corrupt_retained_index_aborts_before_any_deletion() {
        let fixture = fixture();
        fs::write(
            fixture.repo.store.index_path(RETAINED_INDEX_REF).unwrap(),
            b"not a zstd index",
        )
        .unwrap();

        assert!(fixture
            .repo
            .purge(&[RETAINED_INDEX_CALLER.to_owned()], &AtomicBool::new(false))
            .is_err());
        assert_unreferenced_candidates_untouched(&fixture.repo);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_retained_index_aborts_before_any_deletion() {
        use std::os::unix::fs::symlink;

        let fixture = fixture();
        let path = fixture.repo.store.index_path(RETAINED_INDEX_REF).unwrap();
        fs::remove_file(&path).unwrap();
        symlink(
            fixture
                .repo
                .store
                .index_path(RETAINED_INDEX_CALLER)
                .unwrap(),
            path,
        )
        .unwrap();

        assert!(matches!(
            fixture
                .repo
                .purge(&[RETAINED_INDEX_CALLER.to_owned()], &AtomicBool::new(false)),
            Err(RepoError::UnsafePath)
        ));
        assert_unreferenced_candidates_untouched(&fixture.repo);
    }

    #[test]
    fn missing_retained_file_aborts_before_any_deletion() {
        let fixture = fixture();
        fs::remove_file(fixture.repo.store.object_path(RETAINED_FILE_REF).unwrap()).unwrap();

        assert!(fixture
            .repo
            .purge(&[RETAINED_INDEX_CALLER.to_owned()], &AtomicBool::new(false))
            .is_err());
        assert_unreferenced_candidates_untouched(&fixture.repo);
    }

    #[test]
    fn corrupt_retained_file_aborts_before_any_deletion() {
        let fixture = fixture();
        fs::write(
            fixture.repo.store.object_path(RETAINED_FILE_REF).unwrap(),
            b"not an encrypted file object",
        )
        .unwrap();

        assert!(fixture
            .repo
            .purge(&[RETAINED_INDEX_CALLER.to_owned()], &AtomicBool::new(false))
            .is_err());
        assert_unreferenced_candidates_untouched(&fixture.repo);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_retained_file_aborts_before_any_deletion() {
        use std::os::unix::fs::symlink;

        let fixture = fixture();
        let path = fixture.repo.store.object_path(RETAINED_FILE_REF).unwrap();
        fs::remove_file(&path).unwrap();
        symlink(
            fixture
                .repo
                .store
                .object_path(RETAINED_FILE_CALLER)
                .unwrap(),
            path,
        )
        .unwrap();

        assert!(matches!(
            fixture
                .repo
                .purge(&[RETAINED_INDEX_CALLER.to_owned()], &AtomicBool::new(false)),
            Err(RepoError::UnsafePath)
        ));
        assert_unreferenced_candidates_untouched(&fixture.repo);
    }

    #[test]
    fn corrupt_check_index_aborts_before_any_deletion() {
        let fixture = fixture();
        fs::write(
            fixture
                .repo
                .store
                .check_index_path(UNREFERENCED_CHECK)
                .unwrap(),
            b"not a zstd check index",
        )
        .unwrap();

        assert!(fixture
            .repo
            .purge(&[RETAINED_INDEX_CALLER.to_owned()], &AtomicBool::new(false))
            .is_err());
        assert_unreferenced_candidates_untouched(&fixture.repo);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_check_index_aborts_before_any_deletion() {
        use std::os::unix::fs::symlink;

        let fixture = fixture();
        let path = fixture
            .repo
            .store
            .check_index_path(UNREFERENCED_CHECK)
            .unwrap();
        fs::remove_file(&path).unwrap();
        symlink(
            fixture
                .repo
                .store
                .check_index_path(RETAINED_CHECK_REF)
                .unwrap(),
            path,
        )
        .unwrap();

        assert!(matches!(
            fixture
                .repo
                .purge(&[RETAINED_INDEX_CALLER.to_owned()], &AtomicBool::new(false)),
            Err(RepoError::UnsafePath)
        ));
        assert_unreferenced_candidates_untouched(&fixture.repo);
    }

    #[test]
    fn check_index_payload_id_must_match_its_filename() {
        let fixture = fixture();
        let mismatch = check_index(
            MISMATCH_CHECK_ID,
            UNREFERENCED_INDEX,
            UNREFERENCED_FILE,
            &[UNREACHABLE_CHUNK],
        );
        fixture.repo.store.put_check_index(&mismatch).unwrap();
        fs::rename(
            fixture
                .repo
                .store
                .check_index_path(MISMATCH_CHECK_ID)
                .unwrap(),
            fixture
                .repo
                .store
                .check_index_path(UNREFERENCED_CHECK)
                .unwrap(),
        )
        .unwrap();

        assert!(matches!(
            fixture
                .repo
                .purge(&[RETAINED_INDEX_CALLER.to_owned()], &AtomicBool::new(false)),
            Err(RepoError::InvalidData(_))
        ));
        assert_unreferenced_candidates_untouched(&fixture.repo);
    }

    #[test]
    fn purge_delete_phase_excludes_cross_open_ref_and_index_publication() {
        let fixture = fixture();
        let second_repo = Repo::open(
            RepoPaths {
                data: fixture._temp.path().join("data"),
                repo: fixture._temp.path().join("repo"),
                history: fixture._temp.path().join("history-2"),
                temp: fixture._temp.path().join("temp-2"),
            },
            Device {
                id: "second-device".to_owned(),
                name: "QingYu".to_owned(),
                os: "test".to_owned(),
            },
            [9; 32],
            RepoOptions::default(),
        )
        .unwrap();
        let ref_target = index(UNREFERENCED_INDEX, UNREFERENCED_FILE, UNREFERENCED_CHECK);
        let mut concurrent_index = index(CONCURRENT_INDEX, UNREFERENCED_FILE, "");
        concurrent_index.files.clear();
        concurrent_index.count = 0;
        concurrent_index.size = 0;
        let cancelled = AtomicBool::new(false);
        let (before_delete_tx, before_delete_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (ref_started_tx, ref_started_rx) = mpsc::channel();
        let (ref_result_tx, ref_result_rx) = mpsc::channel();
        let (index_started_tx, index_started_rx) = mpsc::channel();
        let (index_result_tx, index_result_rx) = mpsc::channel();

        std::thread::scope(|scope| {
            let purge_repo = &fixture.repo;
            let cancelled = &cancelled;
            let purge_thread = scope.spawn(move || {
                purge_repo.purge_with_before_delete_hook(
                    &[RETAINED_INDEX_CALLER.to_owned()],
                    &cancelled,
                    || {
                        before_delete_tx
                            .send(())
                            .map_err(|_| RepoError::RepoFatal)?;
                        release_rx.recv().map_err(|_| RepoError::RepoFatal)?;
                        Ok(())
                    },
                )
            });
            before_delete_rx.recv().unwrap();

            let ref_repo = &second_repo;
            scope.spawn(move || {
                ref_started_tx.send(()).unwrap();
                ref_result_tx
                    .send(RefStore::new(&ref_repo.store).update_latest(&ref_target))
                    .unwrap();
            });
            let index_repo = &second_repo;
            scope.spawn(move || {
                index_started_tx.send(()).unwrap();
                index_result_tx
                    .send(index_repo.store.put_index(&concurrent_index))
                    .unwrap();
            });
            ref_started_rx.recv().unwrap();
            index_started_rx.recv().unwrap();

            let premature_ref = ref_result_rx.recv_timeout(Duration::from_millis(100)).ok();
            let premature_index = index_result_rx
                .recv_timeout(Duration::from_millis(100))
                .ok();
            let ref_was_premature = premature_ref.is_some();
            let index_was_premature = premature_index.is_some();
            release_tx.send(()).unwrap();
            let purge_result = purge_thread.join().unwrap();
            let ref_result = premature_ref.unwrap_or_else(|| ref_result_rx.recv().unwrap());
            let index_result = premature_index.unwrap_or_else(|| index_result_rx.recv().unwrap());

            assert!(!ref_was_premature, "ref publication bypassed purge guard");
            assert!(
                !index_was_premature,
                "index publication bypassed purge guard"
            );
            purge_result.unwrap();
            assert!(matches!(
                ref_result,
                Err(RepoError::NotFound(id)) if id == UNREFERENCED_INDEX
            ));
            index_result.unwrap();
        });

        assert!(!fixture
            .repo
            .store
            .index_path(UNREFERENCED_INDEX)
            .unwrap()
            .exists());
        assert!(fixture
            .repo
            .store
            .index_path(CONCURRENT_INDEX)
            .unwrap()
            .is_file());
        assert_eq!(
            RefStore::new(&fixture.repo.store)
                .latest()
                .unwrap()
                .unwrap()
                .id,
            RETAINED_INDEX_REF
        );
    }
}
