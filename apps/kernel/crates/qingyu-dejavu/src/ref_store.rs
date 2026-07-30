// DejaVu - Data snapshot and sync.
// Copyright (c) 2022-present, b3log.org
// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::HashSet;
use std::io::Read;
use std::path::Path;

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};

use crate::atomic_write::stage_cap_file;
use crate::path_security::cap_metadata_is_reparse;
use crate::store::validate_id;
use crate::{Index, RepoError, Store};

const REF_MODE: u32 = 0o644;
const MAX_REF_BYTES: u64 = 42;

pub(crate) const MAX_REMOTE_REF_BYTES: u64 = MAX_REF_BYTES;

pub(crate) fn parse_remote_ref(bytes: &[u8]) -> Result<String, RepoError> {
    if bytes.len() as u64 > MAX_REMOTE_REF_BYTES {
        return Err(RepoError::InvalidData(
            "remote ref exceeds the 42-byte limit",
        ));
    }
    let id = std::str::from_utf8(bytes)
        .map_err(|_| RepoError::InvalidData("remote ref must be UTF-8"))?
        .trim();
    validate_id(id)?;
    Ok(id.to_owned())
}

pub struct RefStore<'store> {
    store: &'store Store,
}

impl<'store> RefStore<'store> {
    pub fn new(store: &'store Store) -> Self {
        Self { store }
    }

    pub fn latest(&self) -> Result<Option<Index>, RepoError> {
        let _lifecycle = self.store.try_lifecycle()?;
        let _operation = self.store.lock_operation()?;
        self.resolve_unlocked("latest")
    }

    pub fn latest_sync(&self) -> Result<Option<Index>, RepoError> {
        let _lifecycle = self.store.try_lifecycle()?;
        let _operation = self.store.lock_operation()?;
        self.resolve_unlocked("latest-sync")
    }

    pub fn update_latest(&self, index: &Index) -> Result<(), RepoError> {
        let _lifecycle = self.store.try_lifecycle()?;
        let _operation = self.store.lock_operation()?;
        self.update_unlocked("latest", index)
    }

    pub fn update_latest_sync(&self, index: &Index) -> Result<(), RepoError> {
        let _lifecycle = self.store.try_lifecycle()?;
        let _operation = self.store.lock_operation()?;
        self.update_unlocked("latest-sync", index)
    }

    #[cfg(test)]
    pub(crate) fn all_index_ids(&self) -> Result<HashSet<String>, RepoError> {
        let _lifecycle = self.store.try_lifecycle()?;
        let _operation = self.store.lock_operation()?;
        self.all_index_ids_unlocked_with_cancel_check(&mut || false)
    }

    pub(crate) fn all_index_ids_unlocked_with_cancel_check<F>(
        &self,
        is_cancelled: &mut F,
    ) -> Result<HashSet<String>, RepoError>
    where
        F: FnMut() -> bool,
    {
        check_cancelled(is_cancelled)?;
        let refs = match self.store.open_directory(Path::new("refs"), false) {
            Ok(refs) => refs,
            Err(error) if is_not_found(&error) => return Ok(HashSet::new()),
            Err(error) => return Err(error),
        };
        let mut ids = HashSet::new();
        collect_ref_ids(&refs, true, &mut ids, is_cancelled)?;
        Ok(ids)
    }

    pub(crate) fn resolve_unlocked(&self, name: &str) -> Result<Option<Index>, RepoError> {
        match self.read_unlocked(name)? {
            Some(id) => self.store.get_index_unlocked(&id).map(Some),
            None => Ok(None),
        }
    }

    pub(crate) fn update_unlocked(&self, name: &str, index: &Index) -> Result<(), RepoError> {
        validate_id(&index.id)?;
        let stored = self.store.get_index_unlocked(&index.id)?;
        if stored.id != index.id {
            return Err(RepoError::InvalidData(
                "ref target index id must match its filename",
            ));
        }
        let refs = self.store.open_directory(Path::new("refs"), true)?;
        stage_cap_file(&refs, name.as_ref(), index.id.as_bytes(), REF_MODE)?.publish_replace()
    }

    pub(crate) fn clear_unlocked(&self, name: &str) -> Result<(), RepoError> {
        let refs = self.store.open_directory(Path::new("refs"), true)?;
        stage_cap_file(&refs, name.as_ref(), b"", REF_MODE)?.publish_replace()
    }

    fn read_unlocked(&self, name: &str) -> Result<Option<String>, RepoError> {
        let refs = match self.store.open_directory(Path::new("refs"), false) {
            Ok(refs) => refs,
            Err(error) if is_not_found(&error) => return Ok(None),
            Err(error) => return Err(error),
        };
        read_ref_file(&refs, name.as_ref(), true)
    }
}

fn collect_ref_ids(
    directory: &Dir,
    root_refs_directory: bool,
    ids: &mut HashSet<String>,
    is_cancelled: &mut impl FnMut() -> bool,
) -> Result<(), RepoError> {
    check_cancelled(is_cancelled)?;
    for entry in directory.entries()? {
        check_cancelled(is_cancelled)?;
        let entry = entry?;
        let name = entry.file_name();
        let metadata = directory.symlink_metadata(&name)?;
        if cap_metadata_is_reparse(&metadata) || metadata.file_type().is_symlink() {
            return Err(RepoError::UnsafePath);
        }
        if metadata.file_type().is_dir() {
            let child = directory
                .open_dir_nofollow(&name)
                .map_err(|error| map_nofollow_error(directory, &name, error))?;
            collect_ref_ids(&child, false, ids, is_cancelled)?;
        } else if metadata.file_type().is_file() {
            let empty_is_none =
                root_refs_directory && matches!(name.to_str(), Some("latest" | "latest-sync"));
            if let Some(id) = read_ref_file(directory, &name, empty_is_none)? {
                ids.insert(id);
            }
        }
    }
    Ok(())
}

fn check_cancelled(is_cancelled: &mut impl FnMut() -> bool) -> Result<(), RepoError> {
    if is_cancelled() {
        Err(RepoError::Cancelled)
    } else {
        Ok(())
    }
}

fn read_ref_file(
    directory: &Dir,
    name: &std::ffi::OsStr,
    empty_is_none: bool,
) -> Result<Option<String>, RepoError> {
    let metadata = match directory.symlink_metadata(name) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    validate_ref_metadata(&metadata)?;
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let mut file = directory.open_with(name, &options)?;
    let opened_metadata = file.metadata()?;
    validate_ref_metadata(&opened_metadata)?;
    let mut bytes = Vec::with_capacity(opened_metadata.len() as usize);
    file.by_ref()
        .take(MAX_REF_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_REF_BYTES {
        return Err(RepoError::InvalidData("ref exceeds the 42-byte limit"));
    }
    let content = std::str::from_utf8(&bytes)
        .map_err(|_| RepoError::InvalidData("ref must be UTF-8"))?
        .trim();
    if content.is_empty() {
        return if empty_is_none {
            Ok(None)
        } else {
            Err(RepoError::InvalidData("ref id must not be empty"))
        };
    }
    validate_id(content)?;
    Ok(Some(content.to_owned()))
}

fn validate_ref_metadata(metadata: &cap_std::fs::Metadata) -> Result<(), RepoError> {
    if !metadata.file_type().is_file() || cap_metadata_is_reparse(metadata) {
        return Err(RepoError::UnsafePath);
    }
    if metadata.len() > MAX_REF_BYTES {
        return Err(RepoError::InvalidData("ref exceeds the 42-byte limit"));
    }
    Ok(())
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
    use std::collections::HashSet;
    use std::fs;
    use std::path::Path;

    use tempfile::TempDir;

    use crate::{Index, RepoError, Store};

    use super::{collect_ref_ids, RefStore};

    const INDEX_ID: &str = "1111111111111111111111111111111111111111";
    const SYNC_INDEX_ID: &str = "2222222222222222222222222222222222222222";

    fn index(id: &str) -> Index {
        Index {
            id: id.to_owned(),
            memo: "ref fixture".to_owned(),
            created: 1_700_000_000_123,
            files: Vec::new(),
            count: 0,
            size: 0,
            system_id: "device".to_owned(),
            system_name: "QingYu".to_owned(),
            system_os: "test".to_owned(),
            check_index_id: String::new(),
            aes_key_verify_val: String::new(),
        }
    }

    #[test]
    fn missing_and_empty_latest_refs_are_none() {
        let temp = TempDir::new().unwrap();
        let store = Store::new(temp.path().join("repo"), [7; 32]).unwrap();
        let refs = RefStore::new(&store);

        assert_eq!(refs.latest().unwrap(), None);
        assert_eq!(refs.latest_sync().unwrap(), None);

        fs::create_dir_all(temp.path().join("repo/refs")).unwrap();
        fs::write(temp.path().join("repo/refs/latest"), b" \r\n\t").unwrap();
        fs::write(temp.path().join("repo/refs/latest-sync"), b"\n\t ").unwrap();
        assert_eq!(refs.latest().unwrap(), None);
        assert_eq!(refs.latest_sync().unwrap(), None);

        fs::write(temp.path().join("repo/refs/latest-sync"), b"malformed").unwrap();
        assert!(matches!(refs.latest_sync(), Err(RepoError::InvalidData(_))));
    }

    #[test]
    fn refs_trim_valid_ids_and_resolve_their_indexes() {
        let temp = TempDir::new().unwrap();
        let store = Store::new(temp.path().join("repo"), [7; 32]).unwrap();
        let latest = index(INDEX_ID);
        let latest_sync = index(SYNC_INDEX_ID);
        store.put_index(&latest).unwrap();
        store.put_index(&latest_sync).unwrap();
        let refs = RefStore::new(&store);

        refs.update_latest(&latest).unwrap();
        refs.update_latest_sync(&latest_sync).unwrap();
        assert_eq!(
            fs::read(temp.path().join("repo/refs/latest")).unwrap(),
            INDEX_ID.as_bytes()
        );
        assert_eq!(refs.latest().unwrap(), Some(latest));

        fs::write(
            temp.path().join("repo/refs/latest-sync"),
            format!("{SYNC_INDEX_ID}\r\n"),
        )
        .unwrap();
        assert_eq!(refs.latest_sync().unwrap(), Some(latest_sync));
    }

    #[test]
    fn malformed_and_oversized_refs_are_rejected_instead_of_ignored() {
        let temp = TempDir::new().unwrap();
        let store = Store::new(temp.path().join("repo"), [7; 32]).unwrap();
        let refs_dir = temp.path().join("repo/refs");
        fs::create_dir_all(&refs_dir).unwrap();
        let refs = RefStore::new(&store);

        for malformed in [
            "short",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "gggggggggggggggggggggggggggggggggggggggg",
        ] {
            fs::write(refs_dir.join("latest"), malformed).unwrap();
            assert!(matches!(refs.latest(), Err(RepoError::InvalidData(_))));
        }

        fs::write(refs_dir.join("latest"), "a".repeat(43)).unwrap();
        assert!(matches!(refs.latest(), Err(RepoError::InvalidData(_))));
    }

    #[test]
    fn ref_to_missing_or_corrupt_index_is_an_error() {
        let temp = TempDir::new().unwrap();
        let store = Store::new(temp.path().join("repo"), [7; 32]).unwrap();
        let refs_dir = temp.path().join("repo/refs");
        fs::create_dir_all(&refs_dir).unwrap();
        fs::write(refs_dir.join("latest"), INDEX_ID).unwrap();

        assert!(matches!(
            RefStore::new(&store).latest(),
            Err(RepoError::NotFound(id)) if id == INDEX_ID
        ));

        fs::create_dir_all(temp.path().join("repo/indexes")).unwrap();
        fs::write(store.index_path(INDEX_ID).unwrap(), b"corrupt index").unwrap();
        assert!(RefStore::new(&store).latest().is_err());
    }

    #[test]
    fn invalid_update_does_not_replace_an_existing_ref() {
        let temp = TempDir::new().unwrap();
        let store = Store::new(temp.path().join("repo"), [7; 32]).unwrap();
        let refs = RefStore::new(&store);
        let latest = index(INDEX_ID);
        store.put_index(&latest).unwrap();
        refs.update_latest(&latest).unwrap();

        assert!(matches!(
            refs.update_latest(&index("invalid")),
            Err(RepoError::InvalidData(_))
        ));
        assert_eq!(
            fs::read(temp.path().join("repo/refs/latest")).unwrap(),
            INDEX_ID.as_bytes()
        );
    }

    #[test]
    fn atomic_ref_publication_failure_preserves_the_obstruction_and_cleans_its_temp() {
        let temp = TempDir::new().unwrap();
        let store = Store::new(temp.path().join("repo"), [7; 32]).unwrap();
        let first = index(INDEX_ID);
        let replacement = index(SYNC_INDEX_ID);
        store.put_index(&first).unwrap();
        store.put_index(&replacement).unwrap();
        let refs = RefStore::new(&store);
        refs.update_latest(&first).unwrap();
        let refs_dir = temp.path().join("repo/refs");
        let latest = refs_dir.join("latest");
        let prior = refs_dir.join("prior-latest");
        fs::hard_link(&latest, &prior).unwrap();
        fs::remove_file(&latest).unwrap();
        fs::create_dir(&latest).unwrap();
        fs::write(latest.join("sentinel"), b"do not replace").unwrap();

        assert!(refs.update_latest(&replacement).is_err());
        assert_eq!(fs::read(prior).unwrap(), INDEX_ID.as_bytes());
        assert_eq!(
            fs::read(latest.join("sentinel")).unwrap(),
            b"do not replace"
        );
        let owned_temps = fs::read_dir(&refs_dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .filter(|name| name.to_string_lossy().ends_with(".tmp"))
            .collect::<Vec<_>>();
        assert!(owned_temps.is_empty());
    }

    #[test]
    fn empty_tag_is_a_format_error_even_though_empty_latest_is_none() {
        let temp = TempDir::new().unwrap();
        let store = Store::new(temp.path().join("repo"), [7; 32]).unwrap();
        fs::create_dir_all(temp.path().join("repo/refs/tags")).unwrap();
        fs::write(temp.path().join("repo/refs/latest"), b"\n").unwrap();
        fs::write(temp.path().join("repo/refs/tags/empty"), b"\n").unwrap();
        let refs = RefStore::new(&store);

        assert_eq!(refs.latest().unwrap(), None);
        assert!(matches!(
            refs.all_index_ids(),
            Err(RepoError::InvalidData(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn recursive_ref_collection_rejects_symlinks() {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new().unwrap();
        let store = Store::new(temp.path().join("repo"), [7; 32]).unwrap();
        let tags = temp.path().join("repo/refs/tags");
        fs::create_dir_all(&tags).unwrap();
        let outside = temp.path().join("outside-ref");
        fs::write(&outside, INDEX_ID).unwrap();
        symlink(&outside, tags.join("unsafe")).unwrap();

        assert!(matches!(
            RefStore::new(&store).all_index_ids(),
            Err(RepoError::UnsafePath)
        ));
    }

    #[test]
    fn cancellation_is_checked_when_recursive_ref_collection_descends() {
        let temp = TempDir::new().unwrap();
        let store = Store::new(temp.path().join("repo"), [7; 32]).unwrap();
        fs::create_dir_all(temp.path().join("repo/refs/tags/nested")).unwrap();
        fs::write(temp.path().join("repo/refs/tags/nested/retained"), INDEX_ID).unwrap();
        let tags = store.open_directory(Path::new("refs/tags"), false).unwrap();
        let mut ids = HashSet::new();
        let mut checks = 0;

        assert!(matches!(
            collect_ref_ids(&tags, false, &mut ids, &mut || {
                checks += 1;
                checks == 3
            }),
            Err(RepoError::Cancelled)
        ));
        assert_eq!(checks, 3);
        assert!(ids.is_empty());
    }
}
