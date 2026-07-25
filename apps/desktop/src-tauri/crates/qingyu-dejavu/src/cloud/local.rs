use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::sync::Mutex;

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use tokio::io::AsyncReadExt;

use crate::atomic_write::{create_cap_staged_file_in, stage_cap_file_in, PublishOutcome};
use crate::path_security::{
    cap_metadata_is_reparse, validate_windows_directory_components_before_canonicalize,
};
use crate::store::{absolute_lexical_root, open_absolute_dir_nofollow, open_child_directory};
use crate::RepoError;

use super::{Cloud, CloudError, CloudObject, CloudOperation, CloudUploadSource};

const OBJECT_MODE: u32 = 0o644;
const INTERNAL_NAMESPACE: &str = ".__qingyu_local_cloud";

pub struct LocalCloud {
    root: Dir,
    stage: Dir,
    failures: Mutex<[usize; 4]>,
}

impl LocalCloud {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, CloudError> {
        Self::new_with_before_final_open(root, || {})
    }

    fn new_with_before_final_open(
        root: impl AsRef<Path>,
        before_final_open: impl FnOnce(),
    ) -> Result<Self, CloudError> {
        let path = absolute_lexical_root(root.as_ref().to_path_buf()).map_err(map_repo_error)?;
        let root = match (path.parent(), path.file_name()) {
            (Some(parent), Some(final_component)) => {
                validate_windows_directory_components_before_canonicalize(parent)
                    .map_err(map_repo_error)?;
                let canonical_parent = std::fs::canonicalize(parent).map_err(CloudError::Io)?;
                let parent =
                    open_absolute_dir_nofollow(&canonical_parent).map_err(map_repo_error)?;
                before_final_open();
                open_child_directory(&parent, final_component, false).map_err(map_repo_error)?
            }
            _ => {
                validate_windows_directory_components_before_canonicalize(&path)
                    .map_err(map_repo_error)?;
                before_final_open();
                open_absolute_dir_nofollow(&path).map_err(map_repo_error)?
            }
        };
        let metadata = root.dir_metadata().map_err(CloudError::Io)?;
        if !metadata.file_type().is_dir() || cap_metadata_is_reparse(&metadata) {
            return Err(CloudError::UnsafeKey);
        }
        let stage = open_child_directory(&root, OsStr::new(INTERNAL_NAMESPACE), true)
            .map_err(map_repo_error)?;
        let stage_metadata = stage.dir_metadata().map_err(CloudError::Io)?;
        if !stage_metadata.file_type().is_dir() || cap_metadata_is_reparse(&stage_metadata) {
            return Err(CloudError::UnsafeKey);
        }
        Ok(Self {
            root,
            stage,
            failures: Mutex::new([0; 4]),
        })
    }

    pub fn fail_next(&self, operation: CloudOperation, count: usize) -> Result<(), CloudError> {
        let mut failures = self
            .failures
            .lock()
            .map_err(|_| CloudError::backend("failure_state_poisoned"))?;
        failures[operation_index(operation)] = failures[operation_index(operation)]
            .checked_add(count)
            .ok_or_else(|| CloudError::backend("failure_count_overflow"))?;
        Ok(())
    }

    fn take_failure(&self, operation: CloudOperation) -> Result<(), CloudError> {
        let mut failures = self
            .failures
            .lock()
            .map_err(|_| CloudError::backend("failure_state_poisoned"))?;
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

    fn put_with_before_publish(
        &self,
        key: &str,
        bytes: &[u8],
        overwrite: bool,
        before_publish: impl FnOnce(),
    ) -> Result<u64, CloudError> {
        let components = validate_key(key)?;
        self.take_failure(CloudOperation::Put)?;
        let parent = self.open_parent(&components, true)?;
        let destination = &components[components.len() - 1];
        validate_destination(&parent, destination)?;
        let staged = stage_cap_file_in(&self.stage, &parent, destination, bytes, OBJECT_MODE)
            .map_err(map_repo_error)?;
        before_publish();
        if overwrite {
            staged.publish_replace().map_err(map_repo_error)?;
        } else if staged.publish_no_replace().map_err(map_repo_error)?
            == PublishOutcome::AlreadyExists
        {
            return Err(CloudError::AlreadyExists);
        }
        u64::try_from(bytes.len()).map_err(|_| CloudError::backend("payload_length_overflow"))
    }
}

#[async_trait::async_trait]
impl Cloud for LocalCloud {
    async fn get_bounded(&self, key: &str, max_bytes: u64) -> Result<Vec<u8>, CloudError> {
        let components = validate_key(key)?;
        self.take_failure(CloudOperation::Get)?;
        let parent = self.open_parent(&components, false)?;
        let name = &components[components.len() - 1];
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        let file = parent
            .open_with(name, &options)
            .map_err(|error| map_object_io(&parent, name, error))?;
        let metadata = file.metadata().map_err(CloudError::Io)?;
        if !metadata.file_type().is_file() || cap_metadata_is_reparse(&metadata) {
            return Err(CloudError::UnsafeKey);
        }
        if metadata.len() > max_bytes {
            return Err(CloudError::ResponseTooLarge { limit: max_bytes });
        }
        let reader = tokio::fs::File::from_std(file.into_std());
        let retained_limit = max_bytes.saturating_add(u64::from(max_bytes != u64::MAX));
        let initial_capacity = metadata.len().min(max_bytes).min(64 * 1024) as usize;
        let mut bytes = Vec::with_capacity(initial_capacity);
        tokio::io::AsyncReadExt::read_to_end(&mut reader.take(retained_limit), &mut bytes)
            .await
            .map_err(CloudError::Io)?;
        if bytes.len() as u64 > max_bytes {
            return Err(CloudError::ResponseTooLarge { limit: max_bytes });
        }
        Ok(bytes)
    }

    async fn download_to(
        &self,
        key: &str,
        destination: &mut (dyn tokio::io::AsyncWrite + Unpin + Send),
    ) -> Result<u64, CloudError> {
        let components = validate_key(key)?;
        self.take_failure(CloudOperation::Get)?;
        let parent = self.open_parent(&components, false)?;
        let name = &components[components.len() - 1];
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        let file = parent
            .open_with(name, &options)
            .map_err(|error| map_object_io(&parent, name, error))?;
        let metadata = file.metadata().map_err(CloudError::Io)?;
        if !metadata.file_type().is_file() || cap_metadata_is_reparse(&metadata) {
            return Err(CloudError::UnsafeKey);
        }
        let mut reader = tokio::fs::File::from_std(file.into_std());
        let written = tokio::io::copy(&mut reader, destination)
            .await
            .map_err(CloudError::Io)?;
        tokio::io::AsyncWriteExt::flush(destination)
            .await
            .map_err(CloudError::Io)?;
        Ok(written)
    }

    async fn put(&self, key: &str, bytes: &[u8], overwrite: bool) -> Result<u64, CloudError> {
        self.put_with_before_publish(key, bytes, overwrite, || {})
    }

    async fn upload_from(
        &self,
        key: &str,
        source: &dyn CloudUploadSource,
        overwrite: bool,
    ) -> Result<u64, CloudError> {
        let components = validate_key(key)?;
        self.take_failure(CloudOperation::Put)?;
        let parent = self.open_parent(&components, true)?;
        let destination = &components[components.len() - 1];
        validate_destination(&parent, destination)?;
        let staged = create_cap_staged_file_in(&self.stage, &parent, destination, OBJECT_MODE)
            .map_err(map_repo_error)?;
        let expected = source.content_length();
        let mut reader = source.open()?;
        let mut target = tokio::fs::File::from_std(
            staged
                .file()
                .try_clone()
                .map_err(CloudError::Io)?
                .into_std(),
        );
        let mut actual = 0_u64;
        let mut buffer = vec![0_u8; 64 * 1024];
        while actual < expected {
            let remaining = expected - actual;
            let read_limit = usize::try_from(remaining.min(buffer.len() as u64))
                .map_err(|_| CloudError::backend("payload_length_overflow"))?;
            let read = tokio::io::AsyncReadExt::read(&mut reader, &mut buffer[..read_limit])
                .await
                .map_err(CloudError::Io)?;
            if read == 0 {
                return Err(CloudError::LengthMismatch { expected, actual });
            }
            tokio::io::AsyncWriteExt::write_all(&mut target, &buffer[..read])
                .await
                .map_err(CloudError::Io)?;
            actual = actual
                .checked_add(read as u64)
                .ok_or_else(|| CloudError::backend("payload_length_overflow"))?;
        }
        let excess = tokio::io::AsyncReadExt::read(&mut reader, &mut buffer[..1])
            .await
            .map_err(CloudError::Io)?;
        if excess != 0 {
            return Err(CloudError::LengthMismatch {
                expected,
                actual: expected.saturating_add(1),
            });
        }
        tokio::io::AsyncWriteExt::flush(&mut target)
            .await
            .map_err(CloudError::Io)?;
        target.sync_all().await.map_err(CloudError::Io)?;
        drop(target);
        if overwrite {
            staged.publish_replace().map_err(map_repo_error)?;
        } else if staged.publish_no_replace().map_err(map_repo_error)?
            == PublishOutcome::AlreadyExists
        {
            return Err(CloudError::AlreadyExists);
        }
        Ok(actual)
    }

    async fn remove(&self, key: &str) -> Result<(), CloudError> {
        let components = validate_key(key)?;
        self.take_failure(CloudOperation::Remove)?;
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
        validate_prefix(prefix)?;
        self.take_failure(CloudOperation::List)?;
        let mut all = Vec::new();
        collect_regular_objects(&self.root, "", &mut all)?;
        let mut objects = all
            .into_iter()
            .filter(|object| object.key.starts_with(prefix))
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
    if key.is_empty() || key.starts_with('/') || key.contains('\\') {
        return Err(CloudError::UnsafeKey);
    }
    let components = key
        .split('/')
        .enumerate()
        .map(|(index, component)| {
            validate_component(component, index == 0)?;
            Ok(OsString::from(component))
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

fn validate_component(component: &str, first: bool) -> Result<(), CloudError> {
    if component.is_empty()
        || component == "."
        || component == ".."
        || component.contains(':')
        || component.chars().any(char::is_control)
        || component.ends_with(['.', ' '])
        || first && component.eq_ignore_ascii_case(INTERNAL_NAMESPACE)
        || is_dos_device_name(component)
    {
        Err(CloudError::UnsafeKey)
    } else {
        Ok(())
    }
}

fn is_dos_device_name(component: &str) -> bool {
    let base = component.split('.').next().unwrap_or(component);
    let upper = base.to_ascii_uppercase();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$")
        || upper
            .strip_prefix("COM")
            .or_else(|| upper.strip_prefix("LPT"))
            .is_some_and(|number| {
                matches!(number, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            })
}

fn map_repo_error(error: RepoError) -> CloudError {
    match error {
        RepoError::Io(error) if error.kind() == std::io::ErrorKind::NotFound => {
            CloudError::NotFound
        }
        RepoError::Io(error) => CloudError::Io(error),
        RepoError::UnsafePath | RepoError::InvalidData(_) => CloudError::UnsafeKey,
        _ => CloudError::backend("repository_error"),
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
        if relative.is_empty() && name.eq_ignore_ascii_case(INTERNAL_NAMESPACE) {
            continue;
        }
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
    use std::pin::Pin;
    use std::sync::{Arc, Barrier};

    use tempfile::TempDir;
    use tokio::io::AsyncRead;

    use super::{LocalCloud, INTERNAL_NAMESPACE};
    use crate::cloud::{Cloud, CloudError, CloudObject, CloudOperation, CloudUploadSource};

    struct BytesUploadSource {
        declared_length: u64,
        bytes: Vec<u8>,
    }

    impl CloudUploadSource for BytesUploadSource {
        fn content_length(&self) -> u64 {
            self.declared_length
        }

        fn open(&self) -> Result<Pin<Box<dyn AsyncRead + Send>>, CloudError> {
            Ok(Box::pin(std::io::Cursor::new(self.bytes.clone())))
        }
    }

    #[tokio::test]
    async fn bounded_read_accepts_its_limit_and_rejects_the_next_byte_without_partial_data() {
        let temp = TempDir::new().unwrap();
        let cloud = LocalCloud::new(temp.path()).unwrap();
        cloud.put("objects/value", b"12345", true).await.unwrap();

        assert_eq!(
            cloud.get_bounded("objects/value", 5).await.unwrap(),
            b"12345"
        );
        assert!(matches!(
            cloud.get_bounded("objects/value", 4).await,
            Err(CloudError::ResponseTooLarge { limit: 4 })
        ));
    }

    #[tokio::test]
    async fn staged_transfer_streams_exact_bytes_and_rejects_short_or_long_upload_sources() {
        let temp = TempDir::new().unwrap();
        let cloud = LocalCloud::new(temp.path()).unwrap();
        let exact = BytesUploadSource {
            declared_length: 5,
            bytes: b"value".to_vec(),
        };

        assert_eq!(
            cloud
                .upload_from("objects/exact", &exact, false)
                .await
                .unwrap(),
            5
        );
        let mut downloaded = Vec::new();
        assert_eq!(
            cloud
                .download_to("objects/exact", &mut downloaded)
                .await
                .unwrap(),
            5
        );
        assert_eq!(downloaded, b"value");

        for (key, source, actual) in [
            (
                "objects/short",
                BytesUploadSource {
                    declared_length: 6,
                    bytes: b"short".to_vec(),
                },
                5,
            ),
            (
                "objects/long",
                BytesUploadSource {
                    declared_length: 4,
                    bytes: b"longer".to_vec(),
                },
                5,
            ),
        ] {
            assert!(matches!(
                cloud.upload_from(key, &source, false).await,
                Err(CloudError::LengthMismatch {
                    expected,
                    actual: observed,
                }) if expected == source.declared_length && observed == actual
            ));
            assert!(matches!(
                cloud.get_bounded(key, 16).await,
                Err(CloudError::NotFound)
            ));
        }
    }

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
            "objects/name:stream",
            "objects/CON",
            "objects/con.txt",
            "objects/aux.md",
            "objects/COM1.log",
            "objects/LPT9",
            "objects/trailing.",
            "objects/trailing ",
            ".__qingyu_local_cloud/stage-user.tmp",
            "nul\0key",
        ] {
            assert!(matches!(
                cloud.put(key, b"unsafe", true).await,
                Err(CloudError::UnsafeKey)
            ));
        }

        assert!(matches!(
            cloud.get_bounded("../escape", 16).await,
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
        assert!(matches!(
            cloud.put("refs/latest", b"ignored", false).await,
            Err(CloudError::AlreadyExists)
        ));
        assert_eq!(
            cloud.get_bounded("refs/latest", 16).await.unwrap(),
            b"first"
        );

        assert_eq!(cloud.put("refs/latest", b"second", true).await.unwrap(), 6);
        assert_eq!(
            cloud.get_bounded("refs/latest", 16).await.unwrap(),
            b"second"
        );

        cloud.remove("refs/latest").await.unwrap();
        cloud.remove("refs/latest").await.unwrap();
        assert!(matches!(
            cloud.get_bounded("refs/latest", 16).await,
            Err(CloudError::NotFound)
        ));
        assert_eq!(cloud.available_size().await.unwrap(), u64::MAX);
    }

    #[tokio::test]
    async fn list_returns_full_globally_sorted_keys_matching_the_string_prefix() {
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
                    key: "objects/aa/first".to_owned(),
                    size: 1,
                },
                CloudObject {
                    key: "objects/bb/second".to_owned(),
                    size: 2,
                },
            ]
        );
    }

    #[tokio::test]
    async fn invalid_inputs_do_not_consume_injected_failures() {
        let temp = TempDir::new().unwrap();
        let cloud = LocalCloud::new(temp.path()).unwrap();
        cloud.fail_next(CloudOperation::Put, 1).unwrap();

        assert!(matches!(
            cloud.put("objects/CON.txt", b"unsafe", true).await,
            Err(CloudError::UnsafeKey)
        ));
        assert!(matches!(
            cloud.put("objects/safe", b"value", true).await,
            Err(CloudError::Injected(CloudOperation::Put))
        ));
        assert_eq!(cloud.put("objects/safe", b"value", true).await.unwrap(), 5);

        cloud.fail_next(CloudOperation::Get, 1).unwrap();
        assert!(matches!(
            cloud.get_bounded("objects/CON", 16).await,
            Err(CloudError::UnsafeKey)
        ));
        assert!(matches!(
            cloud.get_bounded("objects/safe", 16).await,
            Err(CloudError::Injected(CloudOperation::Get))
        ));

        cloud.fail_next(CloudOperation::List, 1).unwrap();
        assert!(matches!(
            cloud.list("objects/../").await,
            Err(CloudError::UnsafeKey)
        ));
        assert!(matches!(
            cloud.list("objects/").await,
            Err(CloudError::Injected(CloudOperation::List))
        ));

        cloud.fail_next(CloudOperation::Remove, 1).unwrap();
        assert!(matches!(
            cloud.remove("objects/NUL.txt").await,
            Err(CloudError::UnsafeKey)
        ));
        assert!(matches!(
            cloud.remove("objects/safe").await,
            Err(CloudError::Injected(CloudOperation::Remove))
        ));
    }

    #[tokio::test]
    async fn crash_like_staging_is_preserved_but_excluded_from_all_lists() {
        let temp = TempDir::new().unwrap();
        let staging = temp.path().join(INTERNAL_NAMESPACE);
        fs::create_dir(&staging).unwrap();
        let crash_stage = format!("stage-{}.tmp", "0".repeat(40));
        fs::write(staging.join(&crash_stage), b"crash residue").unwrap();

        let cloud = LocalCloud::new(temp.path()).unwrap();
        assert_eq!(
            fs::read(staging.join(crash_stage)).unwrap(),
            b"crash residue"
        );
        assert!(cloud.list("").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn second_constructor_does_not_delete_an_active_stage() {
        let temp = TempDir::new().unwrap();
        let staging = temp.path().join(INTERNAL_NAMESPACE);
        let cloud = Arc::new(LocalCloud::new(temp.path()).unwrap());
        let staged = Arc::new(Barrier::new(2));
        let publish = Arc::new(Barrier::new(2));
        let writer = {
            let cloud = cloud.clone();
            let staged = staged.clone();
            let publish = publish.clone();
            std::thread::spawn(move || {
                cloud.put_with_before_publish("objects/active", b"complete", true, || {
                    staged.wait();
                    publish.wait();
                })
            })
        };

        staged.wait();
        assert_eq!(fs::read_dir(&staging).unwrap().count(), 1);
        let second = LocalCloud::new(temp.path()).unwrap();
        assert_eq!(fs::read_dir(&staging).unwrap().count(), 1);
        assert!(second.list("").await.unwrap().is_empty());

        publish.wait();
        assert_eq!(writer.join().unwrap().unwrap(), 8);
        assert_eq!(
            second.get_bounded("objects/active", 16).await.unwrap(),
            b"complete"
        );
    }

    #[tokio::test]
    async fn concurrent_list_cannot_observe_a_staged_put() {
        let temp = TempDir::new().unwrap();
        let cloud = Arc::new(LocalCloud::new(temp.path()).unwrap());
        let staged = Arc::new(Barrier::new(2));
        let publish = Arc::new(Barrier::new(2));
        let writer = {
            let cloud = cloud.clone();
            let staged = staged.clone();
            let publish = publish.clone();
            std::thread::spawn(move || {
                cloud.put_with_before_publish("objects/ready", b"complete", true, || {
                    staged.wait();
                    publish.wait();
                })
            })
        };

        staged.wait();
        assert!(cloud.list("").await.unwrap().is_empty());
        publish.wait();
        assert_eq!(writer.join().unwrap().unwrap(), 8);
        assert_eq!(
            cloud.get_bounded("objects/ready", 16).await.unwrap(),
            b"complete"
        );
    }

    #[test]
    fn cross_directory_no_clobber_publication_has_exactly_one_winner() {
        let temp = TempDir::new().unwrap();
        let cloud = Arc::new(LocalCloud::new(temp.path()).unwrap());
        let staged = Arc::new(Barrier::new(3));
        let writers = [b"first".as_slice(), b"second".as_slice()].map(|bytes| {
            let cloud = cloud.clone();
            let staged = staged.clone();
            std::thread::spawn(move || {
                cloud.put_with_before_publish("objects/race", bytes, false, || {
                    staged.wait();
                })
            })
        });

        staged.wait();
        let results = writers.map(|writer| writer.join().unwrap());
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, Err(CloudError::AlreadyExists)))
                .count(),
            1
        );
        let contents = fs::read(temp.path().join("objects/race")).unwrap();
        assert!(contents == b"first" || contents == b"second");
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
    #[test]
    fn constructor_rejects_a_final_component_swapped_after_parent_open() {
        let temp = TempDir::new().unwrap();
        let configured = temp.path().join("cloud");
        let retained = temp.path().join("retained");
        let outside = temp.path().join("outside");
        fs::create_dir(&configured).unwrap();
        fs::create_dir(&outside).unwrap();

        let result = LocalCloud::new_with_before_final_open(&configured, || {
            fs::rename(&configured, &retained).unwrap();
            std::os::unix::fs::symlink(&outside, &configured).unwrap();
        });

        assert!(matches!(result, Err(CloudError::UnsafeKey)));
        assert!(fs::read_dir(&outside).unwrap().next().is_none());
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
            cloud.get_bounded("link-object", 16).await,
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
            cloud.get_bounded("object", 16).await,
            Err(CloudError::Injected(CloudOperation::Get))
        ));
        assert_eq!(cloud.get_bounded("object", 16).await.unwrap(), b"value");

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
