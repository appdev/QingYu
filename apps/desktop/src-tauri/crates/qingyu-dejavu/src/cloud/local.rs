use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::Path;
use std::sync::Mutex;

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};

use crate::atomic_write::stage_cap_file;
use crate::path_security::{
    cap_metadata_is_reparse, std_metadata_is_reparse,
    validate_windows_directory_components_before_canonicalize,
};
use crate::store::{absolute_lexical_root, open_absolute_dir_nofollow, open_child_directory};
use crate::RepoError;

use super::{Cloud, CloudError, CloudObject, CloudOperation};

const OBJECT_MODE: u32 = 0o644;

pub struct LocalCloud {
    root: Dir,
    failures: Mutex<[usize; 4]>,
}

impl LocalCloud {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, CloudError> {
        let root = absolute_lexical_root(root.as_ref().to_path_buf()).map_err(map_repo_error)?;
        validate_windows_directory_components_before_canonicalize(&root).map_err(map_repo_error)?;
        let metadata = std::fs::symlink_metadata(&root).map_err(CloudError::Io)?;
        if !metadata.file_type().is_dir() || std_metadata_is_reparse(&metadata) {
            return Err(CloudError::UnsafeKey);
        }
        let canonical = std::fs::canonicalize(root).map_err(CloudError::Io)?;
        let root = open_absolute_dir_nofollow(&canonical).map_err(map_repo_error)?;
        let metadata = root.dir_metadata().map_err(CloudError::Io)?;
        if !metadata.file_type().is_dir() || cap_metadata_is_reparse(&metadata) {
            return Err(CloudError::UnsafeKey);
        }
        Ok(Self {
            root,
            failures: Mutex::new([0; 4]),
        })
    }

    pub fn fail_next(&self, operation: CloudOperation, count: usize) -> Result<(), CloudError> {
        let mut failures = self.failures.lock().map_err(|_| CloudError::Backend)?;
        failures[operation_index(operation)] = failures[operation_index(operation)]
            .checked_add(count)
            .ok_or(CloudError::Backend)?;
        Ok(())
    }

    fn take_failure(&self, operation: CloudOperation) -> Result<(), CloudError> {
        let mut failures = self.failures.lock().map_err(|_| CloudError::Backend)?;
        let remaining = &mut failures[operation_index(operation)];
        if *remaining == 0 {
            return Ok(());
        }
        *remaining -= 1;
        Err(CloudError::Injected(operation))
    }

    fn open_parent(&self, components: &[OsString], create: bool) -> Result<Dir, CloudError> {
        let mut directory = self.root.try_clone().map_err(CloudError::Io)?;
        for component in &components[..components.len() - 1] {
            directory =
                open_child_directory(&directory, component, create).map_err(map_repo_error)?;
        }
        Ok(directory)
    }
}

#[async_trait::async_trait]
impl Cloud for LocalCloud {
    async fn get(&self, key: &str) -> Result<Vec<u8>, CloudError> {
        self.take_failure(CloudOperation::Get)?;
        let components = validate_key(key)?;
        let parent = self.open_parent(&components, false)?;
        let name = &components[components.len() - 1];
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        let mut file = parent
            .open_with(name, &options)
            .map_err(|error| map_object_io(&parent, name, error))?;
        let metadata = file.metadata().map_err(CloudError::Io)?;
        if !metadata.file_type().is_file() || cap_metadata_is_reparse(&metadata) {
            return Err(CloudError::UnsafeKey);
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).map_err(CloudError::Io)?;
        Ok(bytes)
    }

    async fn put(&self, key: &str, bytes: &[u8], overwrite: bool) -> Result<u64, CloudError> {
        self.take_failure(CloudOperation::Put)?;
        let components = validate_key(key)?;
        let parent = self.open_parent(&components, true)?;
        let destination = &components[components.len() - 1];
        validate_destination(&parent, destination)?;
        let staged =
            stage_cap_file(&parent, destination, bytes, OBJECT_MODE).map_err(map_repo_error)?;
        if overwrite {
            staged.publish_replace().map_err(map_repo_error)?;
        } else {
            staged.publish_no_replace().map_err(map_repo_error)?;
        }
        u64::try_from(bytes.len()).map_err(|_| CloudError::Backend)
    }

    async fn remove(&self, key: &str) -> Result<(), CloudError> {
        self.take_failure(CloudOperation::Remove)?;
        let components = validate_key(key)?;
        let parent = match self.open_parent(&components, false) {
            Ok(parent) => parent,
            Err(CloudError::NotFound) => return Ok(()),
            Err(error) => return Err(error),
        };
        let name = &components[components.len() - 1];
        let metadata = match parent.symlink_metadata(name) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(CloudError::Io(error)),
        };
        if !metadata.file_type().is_file() || cap_metadata_is_reparse(&metadata) {
            return Err(CloudError::UnsafeKey);
        }
        parent.remove_file(name).map_err(CloudError::Io)
    }

    async fn list(&self, prefix: &str) -> Result<Vec<CloudObject>, CloudError> {
        self.take_failure(CloudOperation::List)?;
        validate_prefix(prefix)?;
        let mut all = Vec::new();
        collect_regular_objects(&self.root, "", &mut all)?;
        let mut objects = all
            .into_iter()
            .filter_map(|object| {
                object.key.strip_prefix(prefix).and_then(|relative| {
                    if relative.is_empty() {
                        None
                    } else {
                        Some(CloudObject {
                            key: relative.to_owned(),
                            size: object.size,
                        })
                    }
                })
            })
            .collect::<Vec<_>>();
        objects.sort_by(|left, right| left.key.cmp(&right.key));
        Ok(objects)
    }

    async fn available_size(&self) -> Result<u64, CloudError> {
        Ok(u64::MAX)
    }
}

fn operation_index(operation: CloudOperation) -> usize {
    match operation {
        CloudOperation::Get => 0,
        CloudOperation::Put => 1,
        CloudOperation::Remove => 2,
        CloudOperation::List => 3,
    }
}

fn validate_key(key: &str) -> Result<Vec<OsString>, CloudError> {
    if key.is_empty()
        || key.starts_with('/')
        || key.contains('\\')
        || key.chars().any(|character| character.is_control())
        || has_windows_prefix(key)
    {
        return Err(CloudError::UnsafeKey);
    }
    let components = key
        .split('/')
        .map(|component| {
            if component.is_empty() || component == "." || component == ".." {
                Err(CloudError::UnsafeKey)
            } else {
                Ok(OsString::from(component))
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    if components.is_empty() {
        Err(CloudError::UnsafeKey)
    } else {
        Ok(components)
    }
}

fn validate_prefix(prefix: &str) -> Result<(), CloudError> {
    if prefix.is_empty() {
        return Ok(());
    }
    let key = prefix.strip_suffix('/').unwrap_or(prefix);
    validate_key(key).map(|_| ())
}

fn has_windows_prefix(key: &str) -> bool {
    let bytes = key.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

fn map_repo_error(error: RepoError) -> CloudError {
    match error {
        RepoError::Io(error) if error.kind() == std::io::ErrorKind::NotFound => {
            CloudError::NotFound
        }
        RepoError::Io(error) => CloudError::Io(error),
        RepoError::UnsafePath | RepoError::InvalidData(_) => CloudError::UnsafeKey,
        _ => CloudError::Backend,
    }
}

fn map_object_io(parent: &Dir, name: &OsStr, error: std::io::Error) -> CloudError {
    match parent.symlink_metadata(name) {
        Ok(metadata) if metadata.file_type().is_symlink() || cap_metadata_is_reparse(&metadata) => {
            CloudError::UnsafeKey
        }
        _ if error.kind() == std::io::ErrorKind::NotFound => CloudError::NotFound,
        _ => CloudError::Io(error),
    }
}

fn validate_destination(parent: &Dir, name: &OsStr) -> Result<(), CloudError> {
    match parent.symlink_metadata(name) {
        Ok(metadata) if metadata.file_type().is_file() && !cap_metadata_is_reparse(&metadata) => {
            Ok(())
        }
        Ok(_) => Err(CloudError::UnsafeKey),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(CloudError::Io(error)),
    }
}

fn collect_regular_objects(
    directory: &Dir,
    relative: &str,
    objects: &mut Vec<CloudObject>,
) -> Result<(), CloudError> {
    for entry in directory.entries().map_err(CloudError::Io)? {
        let entry = entry.map_err(CloudError::Io)?;
        let name = entry.file_name();
        let name = name.to_str().ok_or(CloudError::UnsafeKey)?;
        let metadata = directory.symlink_metadata(name).map_err(CloudError::Io)?;
        if metadata.file_type().is_symlink() || cap_metadata_is_reparse(&metadata) {
            continue;
        }
        let key = if relative.is_empty() {
            name.to_owned()
        } else {
            format!("{relative}/{name}")
        };
        if metadata.file_type().is_dir() {
            let child =
                open_child_directory(directory, OsStr::new(name), false).map_err(map_repo_error)?;
            collect_regular_objects(&child, &key, objects)?;
        } else if metadata.file_type().is_file() {
            objects.push(CloudObject {
                key,
                size: metadata.len(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::LocalCloud;
    use crate::cloud::{Cloud, CloudError, CloudObject, CloudOperation};

    #[tokio::test]
    async fn rejects_non_repository_keys_and_prefixes() {
        let temp = TempDir::new().unwrap();
        let cloud = LocalCloud::new(temp.path()).unwrap();

        for key in [
            "",
            "/absolute",
            ".",
            "..",
            "objects/./id",
            "objects/../id",
            "objects//id",
            "objects\\id",
            "C:/escape",
            "nul\0key",
        ] {
            assert!(matches!(
                cloud.put(key, b"unsafe", true).await,
                Err(CloudError::UnsafeKey)
            ));
        }

        assert!(matches!(
            cloud.get("../escape").await,
            Err(CloudError::UnsafeKey)
        ));
        assert!(matches!(
            cloud.remove("/absolute").await,
            Err(CloudError::UnsafeKey)
        ));
        assert!(matches!(
            cloud.list("refs/../objects/").await,
            Err(CloudError::UnsafeKey)
        ));
    }

    #[tokio::test]
    async fn put_get_remove_and_overwrite_are_no_clobber_or_atomic_as_requested() {
        let temp = TempDir::new().unwrap();
        let cloud = LocalCloud::new(temp.path()).unwrap();

        assert_eq!(cloud.put("refs/latest", b"first", false).await.unwrap(), 5);
        assert_eq!(
            cloud.put("refs/latest", b"ignored", false).await.unwrap(),
            7
        );
        assert_eq!(cloud.get("refs/latest").await.unwrap(), b"first");

        assert_eq!(cloud.put("refs/latest", b"second", true).await.unwrap(), 6);
        assert_eq!(cloud.get("refs/latest").await.unwrap(), b"second");

        cloud.remove("refs/latest").await.unwrap();
        cloud.remove("refs/latest").await.unwrap();
        assert!(matches!(
            cloud.get("refs/latest").await,
            Err(CloudError::NotFound)
        ));
        assert_eq!(cloud.available_size().await.unwrap(), u64::MAX);
    }

    #[tokio::test]
    async fn list_returns_sorted_regular_objects_relative_to_the_prefix() {
        let temp = TempDir::new().unwrap();
        let cloud = LocalCloud::new(temp.path()).unwrap();
        cloud.put("objects/bb/second", b"22", false).await.unwrap();
        cloud.put("objects/aa/first", b"1", false).await.unwrap();
        fs::create_dir_all(temp.path().join("objects/cc/directory")).unwrap();

        #[cfg(unix)]
        std::os::unix::fs::symlink(
            temp.path().join("objects/aa/first"),
            temp.path().join("objects/cc/link"),
        )
        .unwrap();

        let listed = cloud.list("objects/").await.unwrap();
        assert_eq!(
            listed,
            [
                CloudObject {
                    key: "aa/first".to_owned(),
                    size: 1,
                },
                CloudObject {
                    key: "bb/second".to_owned(),
                    size: 2,
                },
            ]
        );
    }

    #[tokio::test]
    async fn root_capability_survives_an_ambient_same_name_replacement() {
        let temp = TempDir::new().unwrap();
        let configured = temp.path().join("cloud");
        let retained = temp.path().join("retained-cloud");
        fs::create_dir(&configured).unwrap();
        let cloud = LocalCloud::new(&configured).unwrap();

        fs::rename(&configured, &retained).unwrap();
        fs::create_dir(&configured).unwrap();
        cloud.put("refs/latest", b"retained", true).await.unwrap();

        assert_eq!(fs::read(retained.join("refs/latest")).unwrap(), b"retained");
        assert!(fs::read_dir(configured).unwrap().next().is_none());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn operations_reject_symlink_ancestor_and_final_object_escapes() {
        let temp = TempDir::new().unwrap();
        let outside = temp.path().join("outside");
        let root = temp.path().join("cloud");
        fs::create_dir(&outside).unwrap();
        fs::create_dir(&root).unwrap();
        fs::write(outside.join("secret"), b"outside").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("escape")).unwrap();
        std::os::unix::fs::symlink(outside.join("secret"), root.join("link-object")).unwrap();
        let cloud = LocalCloud::new(&root).unwrap();

        assert!(matches!(
            cloud.put("escape/new", b"pwn", true).await,
            Err(CloudError::UnsafeKey)
        ));
        assert!(matches!(
            cloud.get("link-object").await,
            Err(CloudError::UnsafeKey)
        ));
        assert!(matches!(
            cloud.put("link-object", b"replacement", true).await,
            Err(CloudError::UnsafeKey)
        ));
        assert_eq!(fs::read(outside.join("secret")).unwrap(), b"outside");
    }

    #[tokio::test]
    async fn injected_failures_are_counted_per_operation_and_then_clear() {
        let temp = TempDir::new().unwrap();
        let cloud = LocalCloud::new(temp.path()).unwrap();
        cloud.put("object", b"value", true).await.unwrap();

        for operation in [
            CloudOperation::Get,
            CloudOperation::Put,
            CloudOperation::Remove,
            CloudOperation::List,
        ] {
            cloud.fail_next(operation, 1).unwrap();
        }

        assert!(matches!(
            cloud.get("object").await,
            Err(CloudError::Injected(CloudOperation::Get))
        ));
        assert_eq!(cloud.get("object").await.unwrap(), b"value");

        assert!(matches!(
            cloud.put("other", b"other", true).await,
            Err(CloudError::Injected(CloudOperation::Put))
        ));
        cloud.put("other", b"other", true).await.unwrap();

        assert!(matches!(
            cloud.list("").await,
            Err(CloudError::Injected(CloudOperation::List))
        ));
        assert_eq!(cloud.list("").await.unwrap().len(), 2);

        assert!(matches!(
            cloud.remove("other").await,
            Err(CloudError::Injected(CloudOperation::Remove))
        ));
        cloud.remove("other").await.unwrap();
    }
}
