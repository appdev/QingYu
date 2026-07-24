use std::fmt;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use cap_std::fs::{Dir, File};
use sha2::{Digest, Sha256};

use crate::storage_capability::{
    create_private_file_options, directory_identity, nonfollowing_read_options,
    open_canonical_directory_nofollow, rename_in_directory, sync_directory,
    unique_regular_file_identity, DirectoryIdentity, UniqueRegularFileIdentity,
};

use super::model::{
    SyncConfig, SyncConfigDocument, SyncConfigLoadIssue, SyncConfigLoadResponse, SyncConfigPatch,
    SYNC_CONFIG_VERSION,
};

const SYNC_CONFIG_FILE: &str = "sync-config.json";
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
const MAX_RECOVERABLE_DAMAGED_BYTES: u64 = 16 * 1024 * 1024;
static CONFIG_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static CONFIG_WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SyncConfigStorageError {
    pub(crate) code: &'static str,
    message: &'static str,
}

impl SyncConfigStorageError {
    fn new(code: &'static str, message: &'static str) -> Self {
        Self { code, message }
    }

    fn app_data_unavailable() -> Self {
        Self::new(
            "app-data-unavailable",
            "The application data directory is unavailable.",
        )
    }

    fn unsafe_path() -> Self {
        Self::new(
            "unsafe-sync-config-path",
            "The sync configuration path is unsafe.",
        )
    }

    fn read_failed() -> Self {
        Self::new(
            "sync-config-read-failed",
            "The sync configuration could not be read.",
        )
    }

    fn write_failed() -> Self {
        Self::new(
            "sync-config-write-failed",
            "The sync configuration could not be written.",
        )
    }

    fn replace_failed() -> Self {
        Self::new(
            "atomic-replace-failed",
            "The sync configuration could not be replaced atomically.",
        )
    }

    fn revision_conflict() -> Self {
        Self::new(
            "revision-conflict",
            "The sync configuration changed before this update.",
        )
    }

    fn not_editable() -> Self {
        Self::new(
            "sync-config-not-editable",
            "The sync configuration must be reset or recovered before editing.",
        )
    }

    fn not_recoverable() -> Self {
        Self::new(
            "sync-config-not-recoverable",
            "Only malformed or unsupported sync configurations can be recovered.",
        )
    }

    fn invalid_draft() -> Self {
        Self::new(
            "sync-config-invalid-draft",
            "Submit a complete supported sync configuration.",
        )
    }

    fn reset_not_confirmed() -> Self {
        Self::new(
            "sync-config-reset-not-confirmed",
            "Resetting the sync configuration requires confirmation.",
        )
    }

    fn too_large() -> Self {
        Self::new(
            "sync-config-too-large",
            "The sync configuration exceeds the supported size limit.",
        )
    }

    fn recovery_state_uncertain() -> Self {
        Self::new(
            "sync-config-recovery-state-uncertain",
            "The original sync configuration remains under a damaged recovery name.",
        )
    }
}

impl fmt::Display for SyncConfigStorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for SyncConfigStorageError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SyncConfigDurability {
    Durable,
    ParentSyncUncertain,
}

pub(crate) struct SyncConfigWriteOutcome {
    pub(crate) document: SyncConfigDocument,
    pub(crate) durability: SyncConfigDurability,
}

#[cfg(any(windows, test))]
pub(crate) fn config_path(app_data: &Path) -> PathBuf {
    app_data.join(SYNC_CONFIG_FILE)
}

pub(crate) struct AppDataDirectory {
    canonical_path: PathBuf,
    directory: Dir,
    identity: DirectoryIdentity,
}

impl AppDataDirectory {
    pub(crate) fn canonical_path(&self) -> &Path {
        &self.canonical_path
    }

    pub(crate) fn directory(&self) -> &Dir {
        &self.directory
    }

    pub(crate) fn revalidate(&self) -> Result<(), SyncConfigStorageError> {
        let reopened = open_canonical_directory_nofollow(&self.canonical_path)
            .map_err(|_| SyncConfigStorageError::unsafe_path())?;
        if directory_identity(&reopened).map_err(|_| SyncConfigStorageError::unsafe_path())?
            != self.identity
        {
            return Err(SyncConfigStorageError::unsafe_path());
        }
        Ok(())
    }
}

pub(crate) fn open_app_data(
    app_data: &Path,
    create: bool,
) -> Result<Option<AppDataDirectory>, SyncConfigStorageError> {
    match fs::symlink_metadata(app_data) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(SyncConfigStorageError::unsafe_path());
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && create => {
            fs::create_dir_all(app_data)
                .map_err(|_| SyncConfigStorageError::app_data_unavailable())?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(None);
        }
        Err(_) => return Err(SyncConfigStorageError::app_data_unavailable()),
    }
    let metadata = fs::symlink_metadata(app_data)
        .map_err(|_| SyncConfigStorageError::app_data_unavailable())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(SyncConfigStorageError::unsafe_path());
    }
    let canonical_path = app_data
        .canonicalize()
        .map_err(|_| SyncConfigStorageError::app_data_unavailable())?;
    let directory = open_canonical_directory_nofollow(&canonical_path)
        .map_err(|_| SyncConfigStorageError::unsafe_path())?;
    let identity =
        directory_identity(&directory).map_err(|_| SyncConfigStorageError::unsafe_path())?;
    let retained = AppDataDirectory {
        canonical_path,
        directory,
        identity,
    };
    retained.revalidate()?;
    Ok(Some(retained))
}

enum ConfigSource {
    Bytes {
        bytes: Vec<u8>,
        revision: String,
    },
    Oversized {
        file: File,
        identity: UniqueRegularFileIdentity,
        length: u64,
        revision: String,
    },
    OversizedTooLarge {
        _file: File,
        identity: UniqueRegularFileIdentity,
        _length: u64,
        revision: String,
    },
}

impl ConfigSource {
    fn revision(&self) -> &str {
        match self {
            Self::Bytes { revision, .. }
            | Self::Oversized { revision, .. }
            | Self::OversizedTooLarge { revision, .. } => revision,
        }
    }

    fn bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Bytes { bytes, .. } => Some(bytes),
            Self::Oversized { .. } | Self::OversizedTooLarge { .. } => None,
        }
    }

    fn is_too_large(&self) -> bool {
        matches!(self, Self::OversizedTooLarge { .. })
    }
}

fn open_unique_regular_file(
    directory: &Dir,
    name: &str,
) -> Result<Option<(File, UniqueRegularFileIdentity)>, SyncConfigStorageError> {
    let addressed = match directory.symlink_metadata(name) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(SyncConfigStorageError::unsafe_path())
        }
        Ok(metadata) => unique_regular_file_identity(&metadata)
            .ok_or_else(SyncConfigStorageError::unsafe_path)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(SyncConfigStorageError::read_failed()),
    };
    let file = directory
        .open_with(name, &nonfollowing_read_options())
        .map_err(|_| SyncConfigStorageError::unsafe_path())?;
    let retained = file
        .metadata()
        .map_err(|_| SyncConfigStorageError::read_failed())?;
    let retained =
        unique_regular_file_identity(&retained).ok_or_else(SyncConfigStorageError::unsafe_path)?;
    if retained != addressed {
        return Err(SyncConfigStorageError::unsafe_path());
    }
    Ok(Some((file, retained)))
}

fn hash_file_with_observer<Observe>(
    file: &mut File,
    expected_length: u64,
    observe: &mut Observe,
) -> Result<(String, u64), SyncConfigStorageError>
where
    Observe: FnMut(usize),
{
    file.seek(SeekFrom::Start(0))
        .map_err(|_| SyncConfigStorageError::read_failed())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut length = 0_u64;
    let mut limited = Read::by_ref(file).take(expected_length.saturating_add(1));
    loop {
        let read = limited
            .read(&mut buffer)
            .map_err(|_| SyncConfigStorageError::read_failed())?;
        if read == 0 {
            break;
        }
        observe(read);
        hasher.update(&buffer[..read]);
        length = length
            .checked_add(read as u64)
            .ok_or_else(SyncConfigStorageError::read_failed)?;
    }
    Ok((format!("{:x}", hasher.finalize()), length))
}

fn metadata_revision(
    file: &File,
    identity: UniqueRegularFileIdentity,
) -> Result<String, SyncConfigStorageError> {
    let metadata = file
        .metadata()
        .map_err(|_| SyncConfigStorageError::read_failed())?;
    let modified = metadata
        .modified()
        .map_err(|_| SyncConfigStorageError::read_failed())?
        .into_std();
    let (before_epoch, seconds, nanos) = match modified.duration_since(UNIX_EPOCH) {
        Ok(duration) => (false, duration.as_secs(), duration.subsec_nanos()),
        Err(error) => {
            let duration = error.duration();
            (true, duration.as_secs(), duration.subsec_nanos())
        }
    };
    let (device, inode, length) = identity.revision_parts();
    let mut hasher = Sha256::new();
    hasher.update(b"qingyu-sync-config-oversized-metadata-v1\0");
    hasher.update([u8::from(before_epoch)]);
    for value in [device, inode, length, seconds, u64::from(nanos)] {
        hasher.update(value.to_le_bytes());
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn read_config_source(
    app_data: &AppDataDirectory,
) -> Result<Option<ConfigSource>, SyncConfigStorageError> {
    read_config_source_with_observer(app_data, |_| {})
}

fn read_config_source_with_observer<Observe>(
    app_data: &AppDataDirectory,
    mut observe: Observe,
) -> Result<Option<ConfigSource>, SyncConfigStorageError>
where
    Observe: FnMut(usize),
{
    app_data.revalidate()?;
    let Some((mut file, identity)) =
        open_unique_regular_file(&app_data.directory, SYNC_CONFIG_FILE)?
    else {
        return Ok(None);
    };
    let retained = file
        .metadata()
        .map_err(|_| SyncConfigStorageError::read_failed())?;
    if retained.len() <= MAX_CONFIG_BYTES {
        let mut bytes = Vec::with_capacity(retained.len() as usize);
        Read::by_ref(&mut file)
            .take(MAX_CONFIG_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| SyncConfigStorageError::read_failed())?;
        observe(bytes.len());
        let final_metadata = file
            .metadata()
            .map_err(|_| SyncConfigStorageError::read_failed())?;
        let final_identity = unique_regular_file_identity(&final_metadata)
            .ok_or_else(SyncConfigStorageError::unsafe_path)?;
        if final_identity != identity {
            return Err(SyncConfigStorageError::unsafe_path());
        }
        if bytes.len() <= MAX_CONFIG_BYTES as usize {
            let current_revision = revision(&bytes);
            app_data.revalidate()?;
            return Ok(Some(ConfigSource::Bytes {
                bytes,
                revision: current_revision,
            }));
        }
    }
    if retained.len() > MAX_RECOVERABLE_DAMAGED_BYTES {
        let current_revision = metadata_revision(&file, identity)?;
        app_data.revalidate()?;
        return Ok(Some(ConfigSource::OversizedTooLarge {
            _file: file,
            identity,
            _length: retained.len(),
            revision: current_revision,
        }));
    }
    let (current_revision, length) =
        hash_file_with_observer(&mut file, retained.len(), &mut observe)?;
    let final_metadata = file
        .metadata()
        .map_err(|_| SyncConfigStorageError::read_failed())?;
    let final_identity = unique_regular_file_identity(&final_metadata)
        .ok_or_else(SyncConfigStorageError::unsafe_path)?;
    if final_identity != identity || length != final_metadata.len() {
        return Err(SyncConfigStorageError::unsafe_path());
    }
    app_data.revalidate()?;
    Ok(Some(ConfigSource::Oversized {
        file,
        identity,
        length,
        revision: current_revision,
    }))
}

fn revision(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn document(config: SyncConfig, revision: String) -> SyncConfigDocument {
    let configured = config.configured();
    let readiness = config.readiness();
    let issues = config.issues();
    SyncConfigDocument {
        config,
        configured,
        issues,
        readiness,
        revision,
    }
}

fn classify(bytes: &[u8]) -> SyncConfigLoadResponse {
    let revision = revision(bytes);
    let value = match serde_json::from_slice::<serde_json::Value>(bytes) {
        Ok(value) => value,
        Err(_) => {
            return SyncConfigLoadResponse::Malformed {
                issue: SyncConfigLoadIssue::malformed(),
                revision,
            };
        }
    };
    let Some(version) = value.get("version").and_then(serde_json::Value::as_u64) else {
        return SyncConfigLoadResponse::Malformed {
            issue: SyncConfigLoadIssue::malformed(),
            revision,
        };
    };
    if version != SYNC_CONFIG_VERSION as u64 {
        return SyncConfigLoadResponse::Unsupported {
            issue: SyncConfigLoadIssue::unsupported(),
            revision,
            version,
        };
    }
    let mut config = match serde_json::from_value::<SyncConfig>(value) {
        Ok(config) => config,
        Err(_) => {
            return SyncConfigLoadResponse::Malformed {
                issue: SyncConfigLoadIssue::malformed(),
                revision,
            };
        }
    };
    config.normalize();
    SyncConfigLoadResponse::Loaded {
        document: document(config, revision),
    }
}

pub(crate) fn load_from_app_data(
    app_data: &Path,
) -> Result<SyncConfigLoadResponse, SyncConfigStorageError> {
    let Some(directory) = open_app_data(app_data, false)? else {
        return Ok(SyncConfigLoadResponse::Absent { revision: None });
    };
    let Some(source) = read_config_source(&directory)? else {
        return Ok(SyncConfigLoadResponse::Absent { revision: None });
    };
    Ok(match &source {
        ConfigSource::Bytes { bytes, .. } => classify(bytes),
        ConfigSource::OversizedTooLarge { .. } => SyncConfigLoadResponse::Malformed {
            issue: SyncConfigLoadIssue::oversized_too_large(),
            revision: source.revision().to_string(),
        },
        ConfigSource::Oversized { .. } => SyncConfigLoadResponse::Malformed {
            issue: SyncConfigLoadIssue::malformed(),
            revision: source.revision().to_string(),
        },
    })
}

fn serialized_config(config: &SyncConfig) -> Result<Vec<u8>, SyncConfigStorageError> {
    let mut bytes =
        serde_json::to_vec_pretty(config).map_err(|_| SyncConfigStorageError::write_failed())?;
    bytes.push(b'\n');
    if bytes.len() > MAX_CONFIG_BYTES as usize {
        return Err(SyncConfigStorageError::too_large());
    }
    Ok(bytes)
}

fn expected_revision_matches(
    source: Option<&ConfigSource>,
    expected_revision: Option<&str>,
) -> Result<(), SyncConfigStorageError> {
    if source.map(ConfigSource::revision) == expected_revision {
        Ok(())
    } else {
        Err(SyncConfigStorageError::revision_conflict())
    }
}

fn unique_temp_name() -> String {
    let sequence = CONFIG_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!(".sync-config.tmp-{}-{sequence}", std::process::id())
}

fn replace_staged_file(
    app_data: &AppDataDirectory,
    staged_name: &str,
) -> Result<(), SyncConfigStorageError> {
    rename_in_directory(&app_data.directory, staged_name, SYNC_CONFIG_FILE, true)
        .map_err(|_| SyncConfigStorageError::replace_failed())
}

fn exact_file_identity(file: &File) -> Result<UniqueRegularFileIdentity, SyncConfigStorageError> {
    let metadata = file
        .metadata()
        .map_err(|_| SyncConfigStorageError::read_failed())?;
    unique_regular_file_identity(&metadata).ok_or_else(SyncConfigStorageError::unsafe_path)
}

fn verify_name_identity(
    directory: &Dir,
    name: &str,
    expected: UniqueRegularFileIdentity,
) -> Result<(), SyncConfigStorageError> {
    let Some((_, actual)) = open_unique_regular_file(directory, name)? else {
        return Err(SyncConfigStorageError::unsafe_path());
    };
    if actual != expected {
        return Err(SyncConfigStorageError::unsafe_path());
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum AtomicWriteBehavior {
    Replace,
    #[cfg(test)]
    FailBeforePublish,
}

fn rollback_published_config(
    app_data: &AppDataDirectory,
    current: &mut Option<ConfigSource>,
    published_identity: UniqueRegularFileIdentity,
) -> Result<(), SyncConfigStorageError> {
    verify_name_identity(&app_data.directory, SYNC_CONFIG_FILE, published_identity)
        .map_err(|_| SyncConfigStorageError::recovery_state_uncertain())?;
    app_data
        .directory
        .remove_file(SYNC_CONFIG_FILE)
        .map_err(|_| SyncConfigStorageError::recovery_state_uncertain())?;
    let Some(source) = current.as_mut() else {
        return sync_directory(&app_data.directory)
            .map_err(|_| SyncConfigStorageError::recovery_state_uncertain());
    };
    if source.is_too_large() {
        return Err(SyncConfigStorageError::recovery_state_uncertain());
    }

    let restoration_name = unique_temp_name();
    let restoration = (|| {
        let mut file = app_data
            .directory
            .open_with(&restoration_name, &create_private_file_options())
            .map_err(|_| SyncConfigStorageError::recovery_state_uncertain())?;
        copy_source_to_for_rollback(source, &mut file)
            .map_err(|_| SyncConfigStorageError::recovery_state_uncertain())?;
        file.sync_all()
            .map_err(|_| SyncConfigStorageError::recovery_state_uncertain())?;
        let created = exact_file_identity(&file)
            .map_err(|_| SyncConfigStorageError::recovery_state_uncertain())?;
        drop(file);
        verify_name_identity(&app_data.directory, &restoration_name, created)
            .map_err(|_| SyncConfigStorageError::recovery_state_uncertain())?;
        rename_in_directory(
            &app_data.directory,
            &restoration_name,
            SYNC_CONFIG_FILE,
            false,
        )
        .map_err(|_| SyncConfigStorageError::recovery_state_uncertain())?;
        verify_name_identity(&app_data.directory, SYNC_CONFIG_FILE, created)
            .map_err(|_| SyncConfigStorageError::recovery_state_uncertain())?;
        sync_directory(&app_data.directory)
            .map_err(|_| SyncConfigStorageError::recovery_state_uncertain())
    })();
    if restoration.is_err() {
        let _cleanup = app_data.directory.remove_file(&restoration_name);
    }
    restoration
}

fn atomic_write_in_directory_with_hooks<BeforeRevalidate, BeforePublish>(
    app_data: &AppDataDirectory,
    bytes: &[u8],
    expected_revision: Option<&str>,
    before_revalidate: BeforeRevalidate,
    before_publish: BeforePublish,
    behavior: AtomicWriteBehavior,
) -> Result<SyncConfigDurability, SyncConfigStorageError>
where
    BeforeRevalidate: FnOnce(),
    BeforePublish: FnOnce(),
{
    let staged_name = unique_temp_name();
    let write_result = (|| -> Result<UniqueRegularFileIdentity, SyncConfigStorageError> {
        app_data.revalidate()?;
        let mut file = app_data
            .directory
            .open_with(&staged_name, &create_private_file_options())
            .map_err(|_| SyncConfigStorageError::write_failed())?;
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|_| SyncConfigStorageError::write_failed())?;
        exact_file_identity(&file)
    })();
    let created_identity = match write_result {
        Ok(identity) => identity,
        Err(error) => {
            let _cleanup = app_data.directory.remove_file(&staged_name);
            return Err(error);
        }
    };

    before_revalidate();
    if let Err(error) = app_data.revalidate() {
        let _ = app_data.directory.remove_file(&staged_name);
        return Err(error);
    }
    let mut current = match read_config_source(app_data) {
        Ok(current) => current,
        Err(error) => {
            let _ = app_data.directory.remove_file(&staged_name);
            return Err(error);
        }
    };
    if let Err(error) = expected_revision_matches(current.as_ref(), expected_revision) {
        let _ = app_data.directory.remove_file(&staged_name);
        return Err(error);
    }
    if let Err(error) = verify_name_identity(&app_data.directory, &staged_name, created_identity) {
        let _cleanup = app_data.directory.remove_file(&staged_name);
        return Err(error);
    }
    if let Err(error) = app_data.revalidate() {
        let _cleanup = app_data.directory.remove_file(&staged_name);
        return Err(error);
    }
    before_publish();
    if let Err(error) = verify_name_identity(&app_data.directory, &staged_name, created_identity) {
        let _cleanup = app_data.directory.remove_file(&staged_name);
        return Err(error);
    }
    #[cfg(test)]
    if matches!(behavior, AtomicWriteBehavior::FailBeforePublish) {
        let _cleanup = app_data.directory.remove_file(&staged_name);
        return Err(SyncConfigStorageError::replace_failed());
    }
    let _ = behavior;
    if let Err(error) = replace_staged_file(app_data, &staged_name) {
        let _cleanup = app_data.directory.remove_file(&staged_name);
        return Err(error);
    }
    if verify_name_identity(&app_data.directory, SYNC_CONFIG_FILE, created_identity).is_err() {
        return Err(SyncConfigStorageError::recovery_state_uncertain());
    }
    if let Err(identity_error) = app_data.revalidate() {
        if rollback_published_config(app_data, &mut current, created_identity).is_err() {
            return Err(SyncConfigStorageError::recovery_state_uncertain());
        }
        return Err(identity_error);
    }
    Ok(match sync_directory(&app_data.directory) {
        Ok(()) => SyncConfigDurability::Durable,
        Err(_) => SyncConfigDurability::ParentSyncUncertain,
    })
}

#[cfg(test)]
fn atomic_write_with_hook<BeforeRevalidate>(
    app_data: &Path,
    bytes: &[u8],
    expected_revision: Option<&str>,
    before_revalidate: BeforeRevalidate,
) -> Result<SyncConfigDurability, SyncConfigStorageError>
where
    BeforeRevalidate: FnOnce(),
{
    let directory =
        open_app_data(app_data, false)?.ok_or_else(SyncConfigStorageError::app_data_unavailable)?;
    atomic_write_in_directory_with_hooks(
        &directory,
        bytes,
        expected_revision,
        before_revalidate,
        || {},
        AtomicWriteBehavior::Replace,
    )
}

#[cfg(test)]
fn atomic_write_with_hooks<BeforeRevalidate, BeforePublish>(
    app_data: &Path,
    bytes: &[u8],
    expected_revision: Option<&str>,
    before_revalidate: BeforeRevalidate,
    before_publish: BeforePublish,
) -> Result<SyncConfigDurability, SyncConfigStorageError>
where
    BeforeRevalidate: FnOnce(),
    BeforePublish: FnOnce(),
{
    let directory =
        open_app_data(app_data, false)?.ok_or_else(SyncConfigStorageError::app_data_unavailable)?;
    atomic_write_in_directory_with_hooks(
        &directory,
        bytes,
        expected_revision,
        before_revalidate,
        before_publish,
        AtomicWriteBehavior::Replace,
    )
}

fn install(
    app_data: &AppDataDirectory,
    config: SyncConfig,
    expected_revision: Option<&str>,
) -> Result<SyncConfigWriteOutcome, SyncConfigStorageError> {
    install_with_behavior(
        app_data,
        config,
        expected_revision,
        AtomicWriteBehavior::Replace,
    )
}

fn install_with_behavior(
    app_data: &AppDataDirectory,
    mut config: SyncConfig,
    expected_revision: Option<&str>,
    behavior: AtomicWriteBehavior,
) -> Result<SyncConfigWriteOutcome, SyncConfigStorageError> {
    config.normalize();
    let bytes = serialized_config(&config)?;
    let durability = atomic_write_in_directory_with_hooks(
        app_data,
        &bytes,
        expected_revision,
        || {},
        || {},
        behavior,
    )?;
    Ok(SyncConfigWriteOutcome {
        document: document(config, revision(&bytes)),
        durability,
    })
}

pub(crate) fn enable_at_app_data(
    app_data: &Path,
    expected_revision: Option<&str>,
) -> Result<SyncConfigWriteOutcome, SyncConfigStorageError> {
    let _guard = CONFIG_WRITE_LOCK
        .lock()
        .map_err(|_| SyncConfigStorageError::write_failed())?;
    let directory =
        open_app_data(app_data, true)?.ok_or_else(SyncConfigStorageError::app_data_unavailable)?;
    let current = read_config_source(&directory)?;
    expected_revision_matches(current.as_ref(), expected_revision)?;
    if current.is_some() {
        return Err(SyncConfigStorageError::revision_conflict());
    }
    install(&directory, SyncConfig::default(), None)
}

pub(crate) fn patch_at_app_data(
    app_data: &Path,
    expected_revision: &str,
    patch: SyncConfigPatch,
) -> Result<SyncConfigWriteOutcome, SyncConfigStorageError> {
    patch_batch_at_app_data(app_data, expected_revision, vec![patch])
}

pub(crate) fn patch_batch_at_app_data(
    app_data: &Path,
    expected_revision: &str,
    patches: Vec<SyncConfigPatch>,
) -> Result<SyncConfigWriteOutcome, SyncConfigStorageError> {
    if patches.is_empty() {
        return Err(SyncConfigStorageError::invalid_draft());
    }
    let _guard = CONFIG_WRITE_LOCK
        .lock()
        .map_err(|_| SyncConfigStorageError::write_failed())?;
    let directory =
        open_app_data(app_data, false)?.ok_or_else(SyncConfigStorageError::not_editable)?;
    let source =
        read_config_source(&directory)?.ok_or_else(SyncConfigStorageError::not_editable)?;
    expected_revision_matches(Some(&source), Some(expected_revision))?;
    let Some(bytes) = source.bytes() else {
        return Err(SyncConfigStorageError::not_editable());
    };
    let SyncConfigLoadResponse::Loaded { document } = classify(bytes) else {
        return Err(SyncConfigStorageError::not_editable());
    };
    let mut config = document.config;
    for patch in patches {
        config.apply_patch(patch);
    }
    install(&directory, config, Some(expected_revision))
}

fn damaged_name() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    let sequence = CONFIG_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("sync-config.damaged-{timestamp}-{sequence}.json")
}

fn copy_source_to(
    source: &mut ConfigSource,
    target: &mut File,
) -> Result<(), SyncConfigStorageError> {
    copy_source_to_with_policy(source, target, |_| {}, false)
}

fn copy_source_to_for_rollback(
    source: &mut ConfigSource,
    target: &mut File,
) -> Result<(), SyncConfigStorageError> {
    copy_source_to_with_policy(source, target, |_| {}, true)
}

#[cfg(test)]
fn copy_source_to_with_observer<Observe>(
    source: &mut ConfigSource,
    target: &mut File,
    observe: Observe,
) -> Result<(), SyncConfigStorageError>
where
    Observe: FnMut(usize),
{
    copy_source_to_with_policy(source, target, observe, false)
}

fn copy_source_to_with_policy<Observe>(
    source: &mut ConfigSource,
    target: &mut File,
    mut observe: Observe,
    allow_unlinked_source: bool,
) -> Result<(), SyncConfigStorageError>
where
    Observe: FnMut(usize),
{
    if source.is_too_large() {
        return Err(SyncConfigStorageError::too_large());
    }
    let expected_revision = source.revision().to_string();
    let expected_length = match source {
        ConfigSource::Bytes { bytes, .. } => bytes.len() as u64,
        ConfigSource::Oversized { length, .. } => *length,
        ConfigSource::OversizedTooLarge { .. } => unreachable!("checked above"),
    };
    let mut hasher = Sha256::new();
    let mut length = 0_u64;
    match source {
        ConfigSource::Bytes { bytes, .. } => {
            target
                .write_all(bytes)
                .map_err(|_| SyncConfigStorageError::write_failed())?;
            hasher.update(bytes.as_slice());
            length = bytes.len() as u64;
        }
        ConfigSource::Oversized { file, .. } => {
            file.seek(SeekFrom::Start(0))
                .map_err(|_| SyncConfigStorageError::read_failed())?;
            let mut buffer = [0_u8; 64 * 1024];
            let mut limited = Read::by_ref(file).take(expected_length.saturating_add(1));
            loop {
                let read = limited
                    .read(&mut buffer)
                    .map_err(|_| SyncConfigStorageError::read_failed())?;
                if read == 0 {
                    break;
                }
                observe(read);
                target
                    .write_all(&buffer[..read])
                    .map_err(|_| SyncConfigStorageError::write_failed())?;
                hasher.update(&buffer[..read]);
                length = length
                    .checked_add(read as u64)
                    .ok_or_else(SyncConfigStorageError::read_failed)?;
            }
        }
        ConfigSource::OversizedTooLarge { .. } => unreachable!("checked above"),
    }
    let copied_revision = format!("{:x}", hasher.finalize());
    if let ConfigSource::Oversized { file, identity, .. } = source {
        let metadata = file
            .metadata()
            .map_err(|_| SyncConfigStorageError::read_failed())?;
        if !identity.matches_retained_regular_file(&metadata, allow_unlinked_source) {
            return Err(SyncConfigStorageError::unsafe_path());
        }
    }
    if copied_revision != expected_revision || length != expected_length {
        return Err(SyncConfigStorageError::unsafe_path());
    }
    Ok(())
}

enum DamagedPreservation {
    Copied,
    Moved {
        name: String,
        identity: UniqueRegularFileIdentity,
        revision: String,
    },
}

fn remove_created_damaged(app_data: &AppDataDirectory, name: &str) {
    let _cleanup = app_data.directory.remove_file(name);
    let _sync = sync_directory(&app_data.directory);
}

fn create_damaged_file(
    app_data: &AppDataDirectory,
) -> Result<(String, File), SyncConfigStorageError> {
    for _ in 0..1_000 {
        app_data.revalidate()?;
        let name = damaged_name();
        match app_data
            .directory
            .open_with(&name, &create_private_file_options())
        {
            Ok(file) => return Ok((name, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let metadata = app_data
                    .directory
                    .symlink_metadata(&name)
                    .map_err(|_| SyncConfigStorageError::unsafe_path())?;
                if metadata.file_type().is_symlink()
                    || unique_regular_file_identity(&metadata).is_none()
                {
                    return Err(SyncConfigStorageError::unsafe_path());
                }
            }
            Err(_) => return Err(SyncConfigStorageError::write_failed()),
        }
    }
    Err(SyncConfigStorageError::write_failed())
}

fn preserve_damaged_copy_with_hooks<AfterWrite, SyncDirectory>(
    app_data: &AppDataDirectory,
    source: &mut ConfigSource,
    after_write: AfterWrite,
    sync_directory_hook: SyncDirectory,
) -> Result<DamagedPreservation, SyncConfigStorageError>
where
    AfterWrite: FnOnce(&str),
    SyncDirectory: FnOnce(&Dir) -> std::io::Result<()>,
{
    if source.is_too_large() {
        return Err(SyncConfigStorageError::too_large());
    }
    let (name, mut file) = create_damaged_file(app_data)?;
    let created = match copy_source_to(source, &mut file)
        .and_then(|()| {
            file.sync_all()
                .map_err(|_| SyncConfigStorageError::write_failed())
        })
        .and_then(|()| exact_file_identity(&file))
    {
        Ok(identity) => identity,
        Err(error) => {
            drop(file);
            remove_created_damaged(app_data, &name);
            return Err(error);
        }
    };
    drop(file);
    after_write(&name);
    let finish = verify_name_identity(&app_data.directory, &name, created)
        .and_then(|()| app_data.revalidate())
        .and_then(|()| {
            sync_directory_hook(&app_data.directory)
                .map_err(|_| SyncConfigStorageError::write_failed())
        });
    if let Err(error) = finish {
        remove_created_damaged(app_data, &name);
        return Err(error);
    }
    Ok(DamagedPreservation::Copied)
}

fn preserve_damaged_copy(
    app_data: &AppDataDirectory,
    source: &mut ConfigSource,
) -> Result<DamagedPreservation, SyncConfigStorageError> {
    preserve_damaged_copy_with_hooks(app_data, source, |_| {}, sync_directory)
}

fn verify_large_source_at_name(
    app_data: &AppDataDirectory,
    name: &str,
    expected_identity: UniqueRegularFileIdentity,
    expected_revision: &str,
) -> Result<(), SyncConfigStorageError> {
    let Some((file, identity)) = open_unique_regular_file(&app_data.directory, name)? else {
        return Err(SyncConfigStorageError::unsafe_path());
    };
    if identity != expected_identity || metadata_revision(&file, identity)? != expected_revision {
        return Err(SyncConfigStorageError::unsafe_path());
    }
    Ok(())
}

fn rollback_moved_damaged(
    app_data: &AppDataDirectory,
    name: &str,
    identity: UniqueRegularFileIdentity,
    revision: &str,
) -> Result<(), SyncConfigStorageError> {
    verify_large_source_at_name(app_data, name, identity, revision)
        .map_err(|_| SyncConfigStorageError::recovery_state_uncertain())?;
    match open_unique_regular_file(&app_data.directory, SYNC_CONFIG_FILE) {
        Ok(None) => {}
        Ok(Some(_)) | Err(_) => return Err(SyncConfigStorageError::recovery_state_uncertain()),
    }
    rename_in_directory(&app_data.directory, name, SYNC_CONFIG_FILE, false)
        .map_err(|_| SyncConfigStorageError::recovery_state_uncertain())?;
    verify_large_source_at_name(app_data, SYNC_CONFIG_FILE, identity, revision)
        .map_err(|_| SyncConfigStorageError::recovery_state_uncertain())?;
    sync_directory(&app_data.directory)
        .map_err(|_| SyncConfigStorageError::recovery_state_uncertain())
}

fn preserve_damaged_by_rename(
    app_data: &AppDataDirectory,
    source: &ConfigSource,
) -> Result<DamagedPreservation, SyncConfigStorageError> {
    let ConfigSource::OversizedTooLarge {
        identity, revision, ..
    } = source
    else {
        return Err(SyncConfigStorageError::write_failed());
    };
    let identity = *identity;
    let revision = revision.clone();
    app_data.revalidate()?;
    verify_large_source_at_name(app_data, SYNC_CONFIG_FILE, identity, &revision)?;

    for _ in 0..1_000 {
        let name = damaged_name();
        match rename_in_directory(&app_data.directory, SYNC_CONFIG_FILE, &name, false) {
            Ok(()) => {
                let finish = verify_large_source_at_name(app_data, &name, identity, &revision)
                    .and_then(|()| app_data.revalidate())
                    .and_then(|()| {
                        sync_directory(&app_data.directory)
                            .map_err(|_| SyncConfigStorageError::write_failed())
                    });
                if let Err(error) = finish {
                    if rollback_moved_damaged(app_data, &name, identity, &revision).is_err() {
                        return Err(SyncConfigStorageError::recovery_state_uncertain());
                    }
                    return Err(error);
                }
                return Ok(DamagedPreservation::Moved {
                    name,
                    identity,
                    revision,
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let metadata = app_data
                    .directory
                    .symlink_metadata(&name)
                    .map_err(|_| SyncConfigStorageError::unsafe_path())?;
                if metadata.file_type().is_symlink()
                    || unique_regular_file_identity(&metadata).is_none()
                {
                    return Err(SyncConfigStorageError::unsafe_path());
                }
            }
            Err(_) => return Err(SyncConfigStorageError::write_failed()),
        }
    }
    Err(SyncConfigStorageError::write_failed())
}

fn preserve_damaged(
    app_data: &AppDataDirectory,
    source: &mut ConfigSource,
) -> Result<DamagedPreservation, SyncConfigStorageError> {
    if source.is_too_large() {
        preserve_damaged_by_rename(app_data, source)
    } else {
        preserve_damaged_copy(app_data, source)
    }
}

fn install_after_preserving_damaged(
    app_data: &AppDataDirectory,
    config: SyncConfig,
    original_revision: &str,
    preservation: DamagedPreservation,
    behavior: AtomicWriteBehavior,
) -> Result<SyncConfigWriteOutcome, SyncConfigStorageError> {
    let expected_revision = match &preservation {
        DamagedPreservation::Copied => Some(original_revision),
        DamagedPreservation::Moved { .. } => None,
    };
    let result = install_with_behavior(app_data, config, expected_revision, behavior);
    if result.is_err() {
        if let DamagedPreservation::Moved {
            name,
            identity,
            revision,
        } = preservation
        {
            rollback_moved_damaged(app_data, &name, identity, &revision)?;
        }
    }
    result
}

pub(crate) fn recover_at_app_data(
    app_data: &Path,
    expected_revision: &str,
    mut config: SyncConfig,
) -> Result<SyncConfigWriteOutcome, SyncConfigStorageError> {
    let _guard = CONFIG_WRITE_LOCK
        .lock()
        .map_err(|_| SyncConfigStorageError::write_failed())?;
    let directory =
        open_app_data(app_data, false)?.ok_or_else(SyncConfigStorageError::not_recoverable)?;
    let mut source =
        read_config_source(&directory)?.ok_or_else(SyncConfigStorageError::not_recoverable)?;
    expected_revision_matches(Some(&source), Some(expected_revision))?;
    if source
        .bytes()
        .is_some_and(|bytes| matches!(classify(bytes), SyncConfigLoadResponse::Loaded { .. }))
    {
        return Err(SyncConfigStorageError::not_recoverable());
    }
    if config.version != SYNC_CONFIG_VERSION {
        return Err(SyncConfigStorageError::invalid_draft());
    }
    config.normalize();
    let draft = serialized_config(&config)?;
    if !matches!(classify(&draft), SyncConfigLoadResponse::Loaded { .. }) {
        return Err(SyncConfigStorageError::invalid_draft());
    }
    let original_revision = source.revision().to_string();
    let preservation = preserve_damaged(&directory, &mut source)?;
    install_after_preserving_damaged(
        &directory,
        config,
        &original_revision,
        preservation,
        AtomicWriteBehavior::Replace,
    )
}

pub(crate) fn reset_at_app_data(
    app_data: &Path,
    confirmed: bool,
    expected_revision: Option<&str>,
) -> Result<SyncConfigWriteOutcome, SyncConfigStorageError> {
    reset_at_app_data_with_behavior(
        app_data,
        confirmed,
        expected_revision,
        AtomicWriteBehavior::Replace,
    )
}

fn reset_at_app_data_with_behavior(
    app_data: &Path,
    confirmed: bool,
    expected_revision: Option<&str>,
    behavior: AtomicWriteBehavior,
) -> Result<SyncConfigWriteOutcome, SyncConfigStorageError> {
    if !confirmed {
        return Err(SyncConfigStorageError::reset_not_confirmed());
    }
    let _guard = CONFIG_WRITE_LOCK
        .lock()
        .map_err(|_| SyncConfigStorageError::write_failed())?;
    let directory =
        open_app_data(app_data, true)?.ok_or_else(SyncConfigStorageError::app_data_unavailable)?;
    let mut current = read_config_source(&directory)?;
    expected_revision_matches(current.as_ref(), expected_revision)?;
    if let Some(source) = current.as_mut() {
        let invalid = source
            .bytes()
            .is_none_or(|bytes| !matches!(classify(bytes), SyncConfigLoadResponse::Loaded { .. }));
        if invalid {
            let original_revision = source.revision().to_string();
            let preservation = preserve_damaged(&directory, source)?;
            return install_after_preserving_damaged(
                &directory,
                SyncConfig::default(),
                &original_revision,
                preservation,
                behavior,
            );
        }
    }
    install_with_behavior(
        &directory,
        SyncConfig::default(),
        expected_revision,
        behavior,
    )
}

#[cfg(test)]
fn reset_at_app_data_with_install_fault(
    app_data: &Path,
    confirmed: bool,
    expected_revision: Option<&str>,
) -> Result<SyncConfigWriteOutcome, SyncConfigStorageError> {
    reset_at_app_data_with_behavior(
        app_data,
        confirmed,
        expected_revision,
        AtomicWriteBehavior::FailBeforePublish,
    )
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use tempfile::tempdir;

    #[test]
    fn recovery_preserves_invalid_bytes_and_installs_submitted_config() {
        let app_data = tempdir().unwrap();
        let invalid = b"{invalid-exact-bytes";
        fs::write(config_path(app_data.path()), invalid).unwrap();
        let expected = revision(invalid);
        let mut config = SyncConfig::default();
        config.remote_root = "team".into();

        let outcome = recover_at_app_data(app_data.path(), &expected, config).unwrap();

        assert_eq!(outcome.document.config.remote_root, "team");
        assert!(fs::read_dir(app_data.path()).unwrap().any(|entry| {
            let path = entry.unwrap().path();
            path.file_name().is_some_and(|name| {
                name.to_string_lossy().starts_with("sync-config.damaged-")
                    && fs::read(&path).unwrap() == invalid
            })
        }));
    }

    #[test]
    fn storage_errors_do_not_echo_secret_patch_values() {
        let app_data = tempdir().unwrap();
        enable_at_app_data(app_data.path(), None).unwrap();
        let secret = "must-not-appear";
        let error = patch_at_app_data(
            app_data.path(),
            "stale",
            SyncConfigPatch::S3SecretAccessKey(secret.into()),
        )
        .err()
        .unwrap();
        assert!(!error.to_string().contains(secret));
    }

    #[test]
    fn creating_sync_config_keeps_synchronization_disabled_by_default() {
        let app_data = tempdir().unwrap();

        let created = enable_at_app_data(app_data.path(), None).unwrap();

        assert!(!created.document.config.enabled);
        assert!(!created.document.configured);
        assert!(matches!(
            created.document.readiness,
            super::super::model::SyncConfigReadiness::Disabled
        ));
        assert_eq!(
            serde_json::to_value(&created.document).unwrap()["configured"],
            false
        );
    }

    #[test]
    fn disabled_config_load_explicitly_distinguishes_complete_and_partial_documents() {
        let mut complete = SyncConfig::default();
        complete.provider = super::super::model::SyncProvider::Webdav;
        complete.remote_root = "qingyu".into();
        complete.webdav.server_url = "https://dav.example.test".into();

        let SyncConfigLoadResponse::Loaded { document: complete } =
            classify(&serde_json::to_vec(&complete).unwrap())
        else {
            panic!("complete disabled config should load");
        };
        assert!(complete.configured);
        assert!(matches!(
            complete.readiness,
            super::super::model::SyncConfigReadiness::Disabled
        ));
        assert!(complete.issues.is_empty());

        let mut partial = SyncConfig::default();
        partial.remote_root = "qingyu".into();
        let SyncConfigLoadResponse::Loaded { document: partial } =
            classify(&serde_json::to_vec(&partial).unwrap())
        else {
            panic!("partial disabled config should load");
        };
        assert!(!partial.configured);
        assert!(matches!(
            partial.readiness,
            super::super::model::SyncConfigReadiness::Disabled
        ));
        assert!(partial.issues.is_empty());
    }

    #[test]
    fn malformed_config_blocks_patch_and_unconfirmed_reset() {
        let app_data = tempdir().unwrap();
        let invalid = b"{invalid";
        fs::write(config_path(app_data.path()), invalid).unwrap();
        let current = revision(invalid);

        assert_eq!(
            patch_at_app_data(app_data.path(), &current, SyncConfigPatch::Enabled(true),)
                .err()
                .unwrap()
                .code,
            "sync-config-not-editable"
        );
        assert_eq!(
            reset_at_app_data(app_data.path(), false, Some(&current))
                .err()
                .unwrap()
                .code,
            "sync-config-reset-not-confirmed"
        );
        assert_eq!(fs::read(config_path(app_data.path())).unwrap(), invalid);
    }

    #[test]
    fn atomic_writes_leave_no_staging_files() {
        let app_data = tempdir().unwrap();
        let enabled = enable_at_app_data(app_data.path(), None).unwrap();
        patch_at_app_data(
            app_data.path(),
            &enabled.document.revision,
            SyncConfigPatch::RemoteRoot("qingyu".into()),
        )
        .unwrap();

        let names = fs::read_dir(app_data.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["sync-config.json"]);
    }

    #[cfg(unix)]
    #[test]
    fn persisted_config_is_private_to_the_current_user() {
        use std::os::unix::fs::PermissionsExt;

        let app_data = tempdir().unwrap();
        enable_at_app_data(app_data.path(), None).unwrap();
        let mode = fs::metadata(config_path(app_data.path()))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn revalidation_read_failure_removes_the_staged_file() {
        let app_data = tempdir().unwrap();
        let installed = enable_at_app_data(app_data.path(), None).unwrap();
        let outside = tempdir().unwrap();
        let outside_file = outside.path().join("outside.json");
        fs::write(&outside_file, b"outside-secret").unwrap();
        let bytes = serialized_config(&SyncConfig::default()).unwrap();

        let error = atomic_write_with_hook(
            app_data.path(),
            &bytes,
            Some(&installed.document.revision),
            || {
                fs::remove_file(config_path(app_data.path())).unwrap();
                std::os::unix::fs::symlink(&outside_file, config_path(app_data.path())).unwrap();
            },
        )
        .err()
        .unwrap();

        assert_eq!(error.code, "unsafe-sync-config-path");
        assert_eq!(fs::read(outside_file).unwrap(), b"outside-secret");
        assert!(!fs::read_dir(app_data.path()).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".sync-config.tmp-")
        }));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_config_file() {
        let app_data = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let outside_file = outside.path().join("secret.json");
        fs::write(&outside_file, b"outside").unwrap();
        std::os::unix::fs::symlink(&outside_file, config_path(app_data.path())).unwrap();
        let error = load_from_app_data(app_data.path()).err().unwrap();
        assert_eq!(error.code, "unsafe-sync-config-path");
        assert_eq!(fs::read(outside_file).unwrap(), b"outside");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_hardlinked_config_file_without_changing_the_other_link() {
        let app_data = tempdir().unwrap();
        enable_at_app_data(app_data.path(), None).unwrap();
        let outside = tempdir().unwrap();
        let outside_file = outside.path().join("linked-config.json");
        fs::hard_link(config_path(app_data.path()), &outside_file).unwrap();
        let original = fs::read(&outside_file).unwrap();

        let error = load_from_app_data(app_data.path()).err().unwrap();

        assert_eq!(error.code, "unsafe-sync-config-path");
        assert_eq!(fs::read(outside_file).unwrap(), original);
    }

    #[cfg(unix)]
    #[test]
    fn app_data_identity_change_aborts_publish_and_cleans_the_retained_directory() {
        let parent = tempdir().unwrap();
        let app_data = parent.path().join("app-data");
        fs::create_dir(&app_data).unwrap();
        let installed = enable_at_app_data(&app_data, None).unwrap();
        let original = fs::read(config_path(&app_data)).unwrap();
        let moved = parent.path().join("moved-app-data");
        let replacement_bytes = serialized_config(&SyncConfig::default()).unwrap();

        let error = atomic_write_with_hook(
            &app_data,
            &replacement_bytes,
            Some(&installed.document.revision),
            || {
                fs::rename(&app_data, &moved).unwrap();
                fs::create_dir(&app_data).unwrap();
            },
        )
        .err()
        .unwrap();

        assert_eq!(error.code, "unsafe-sync-config-path");
        assert_eq!(fs::read(config_path(&moved)).unwrap(), original);
        assert!(!config_path(&app_data).exists());
        assert!(!fs::read_dir(&moved).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".sync-config.tmp-")
        }));
    }

    #[test]
    fn oversized_patch_is_rejected_before_staging_and_preserves_active_config() {
        let app_data = tempdir().unwrap();
        let installed = enable_at_app_data(app_data.path(), None).unwrap();
        let original = fs::read(config_path(app_data.path())).unwrap();

        let error = patch_at_app_data(
            app_data.path(),
            &installed.document.revision,
            SyncConfigPatch::WebDavPassword("x".repeat(MAX_CONFIG_BYTES as usize)),
        )
        .err()
        .unwrap();

        assert_eq!(error.code, "sync-config-too-large");
        assert_eq!(fs::read(config_path(app_data.path())).unwrap(), original);
        assert!(!fs::read_dir(app_data.path()).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".sync-config.tmp-")
        }));
    }

    #[test]
    fn oversized_recovery_draft_is_rejected_without_backup_or_staging() {
        let app_data = tempdir().unwrap();
        let invalid = b"{invalid";
        fs::write(config_path(app_data.path()), invalid).unwrap();
        let expected = revision(invalid);
        let mut draft = SyncConfig::default();
        draft.webdav.password = "x".repeat(MAX_CONFIG_BYTES as usize);

        let error = recover_at_app_data(app_data.path(), &expected, draft)
            .err()
            .unwrap();

        assert_eq!(error.code, "sync-config-too-large");
        assert_eq!(fs::read(config_path(app_data.path())).unwrap(), invalid);
        let names = fs::read_dir(app_data.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(names, vec![std::ffi::OsString::from(SYNC_CONFIG_FILE)]);
    }

    #[test]
    fn confirmed_reset_streams_an_external_oversized_config_to_damaged_backup() {
        let app_data = tempdir().unwrap();
        let oversized = vec![b'x'; MAX_CONFIG_BYTES as usize + 1];
        fs::write(config_path(app_data.path()), &oversized).unwrap();
        let SyncConfigLoadResponse::Malformed {
            revision: expected, ..
        } = load_from_app_data(app_data.path()).unwrap()
        else {
            panic!("an external oversized config should remain resettable");
        };
        assert_eq!(expected, revision(&oversized));

        let outcome = reset_at_app_data(app_data.path(), true, Some(&expected)).unwrap();

        assert_eq!(outcome.document.config.version, SYNC_CONFIG_VERSION);
        assert!(fs::read_dir(app_data.path()).unwrap().any(|entry| {
            let path = entry.unwrap().path();
            path.file_name().is_some_and(|name| {
                name.to_string_lossy().starts_with("sync-config.damaged-")
                    && fs::read(&path).unwrap() == oversized
            })
        }));
    }

    #[test]
    fn staging_same_size_inode_replacement_is_rejected_before_publish() {
        let app_data = tempdir().unwrap();
        let installed = enable_at_app_data(app_data.path(), None).unwrap();
        let original = fs::read(config_path(app_data.path())).unwrap();
        let bytes = serialized_config(&SyncConfig::default()).unwrap();

        let error = atomic_write_with_hooks(
            app_data.path(),
            &bytes,
            Some(&installed.document.revision),
            || {
                let staging = fs::read_dir(app_data.path())
                    .unwrap()
                    .map(|entry| entry.unwrap().path())
                    .find(|path| {
                        path.file_name().is_some_and(|name| {
                            name.to_string_lossy().starts_with(".sync-config.tmp-")
                        })
                    })
                    .unwrap();
                let length = fs::metadata(&staging).unwrap().len() as usize;
                fs::remove_file(&staging).unwrap();
                fs::write(staging, vec![b'x'; length]).unwrap();
            },
            || {},
        )
        .err()
        .unwrap();

        assert_eq!(error.code, "unsafe-sync-config-path");
        assert_eq!(fs::read(config_path(app_data.path())).unwrap(), original);
        assert!(!fs::read_dir(app_data.path()).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".sync-config.tmp-")
        }));
    }

    #[test]
    fn damaged_same_size_inode_replacement_is_rejected_and_cleaned() {
        let app_data = tempdir().unwrap();
        let invalid = b"{invalid-damaged";
        fs::write(config_path(app_data.path()), invalid).unwrap();
        let directory = open_app_data(app_data.path(), false).unwrap().unwrap();
        let mut source = read_config_source(&directory).unwrap().unwrap();

        let error = preserve_damaged_copy_with_hooks(
            &directory,
            &mut source,
            |name| {
                let path = app_data.path().join(name);
                let length = fs::metadata(&path).unwrap().len() as usize;
                fs::remove_file(&path).unwrap();
                fs::write(path, vec![b'x'; length]).unwrap();
            },
            |_| Ok(()),
        )
        .err()
        .unwrap();

        assert_eq!(error.code, "unsafe-sync-config-path");
        assert_eq!(fs::read(config_path(app_data.path())).unwrap(), invalid);
        assert!(!fs::read_dir(app_data.path()).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("sync-config.damaged-")
        }));
    }

    #[test]
    fn damaged_directory_sync_failure_cleans_the_created_copy() {
        let app_data = tempdir().unwrap();
        fs::write(config_path(app_data.path()), b"{invalid").unwrap();
        let directory = open_app_data(app_data.path(), false).unwrap().unwrap();
        let mut source = read_config_source(&directory).unwrap().unwrap();

        let error = preserve_damaged_copy_with_hooks(
            &directory,
            &mut source,
            |_| {},
            |_| Err(std::io::Error::other("injected directory sync failure")),
        )
        .err()
        .unwrap();

        assert_eq!(error.code, "sync-config-write-failed");
        assert!(!fs::read_dir(app_data.path()).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("sync-config.damaged-")
        }));
    }

    #[test]
    fn growing_oversized_source_hash_is_bounded_to_the_original_length() {
        let app_data = tempdir().unwrap();
        let active = config_path(app_data.path());
        let original_length = MAX_CONFIG_BYTES + 1;
        fs::File::create(&active)
            .unwrap()
            .set_len(original_length)
            .unwrap();
        let directory = open_app_data(app_data.path(), false).unwrap().unwrap();
        let observed_reads = Cell::new(0_u64);
        let grew = Cell::new(false);

        let error = read_config_source_with_observer(&directory, |read| {
            observed_reads.set(observed_reads.get() + read as u64);
            if !grew.replace(true) {
                fs::OpenOptions::new()
                    .write(true)
                    .open(&active)
                    .unwrap()
                    .set_len(MAX_RECOVERABLE_DAMAGED_BYTES * 4)
                    .unwrap();
            }
        })
        .err()
        .unwrap();

        assert_eq!(error.code, "unsafe-sync-config-path");
        assert!(observed_reads.get() <= original_length + 1);
    }

    #[test]
    fn growing_oversized_source_copy_is_bounded_to_the_original_length() {
        let app_data = tempdir().unwrap();
        let active = config_path(app_data.path());
        let original_length = MAX_CONFIG_BYTES + 1;
        fs::File::create(&active)
            .unwrap()
            .set_len(original_length)
            .unwrap();
        let directory = open_app_data(app_data.path(), false).unwrap().unwrap();
        let mut source = read_config_source(&directory).unwrap().unwrap();
        let mut target = directory
            .directory
            .open_with("bounded-copy", &create_private_file_options())
            .unwrap();
        let observed_reads = Cell::new(0_u64);
        let grew = Cell::new(false);

        let error = copy_source_to_with_observer(&mut source, &mut target, |read| {
            observed_reads.set(observed_reads.get() + read as u64);
            if !grew.replace(true) {
                fs::OpenOptions::new()
                    .write(true)
                    .open(&active)
                    .unwrap()
                    .set_len(MAX_RECOVERABLE_DAMAGED_BYTES * 4)
                    .unwrap();
            }
        })
        .err()
        .unwrap();

        assert_eq!(error.code, "unsafe-sync-config-path");
        assert!(observed_reads.get() <= original_length + 1);
        assert!(target.metadata().unwrap().len() <= original_length + 1);
    }

    #[cfg(unix)]
    #[test]
    fn rollback_copy_accepts_the_exact_retained_source_after_publish_unlinks_it() {
        let app_data = tempdir().unwrap();
        let original = vec![b'x'; MAX_CONFIG_BYTES as usize + 1];
        fs::write(config_path(app_data.path()), &original).unwrap();
        let directory = open_app_data(app_data.path(), false).unwrap().unwrap();
        let mut source = read_config_source(&directory).unwrap().unwrap();
        directory.directory.remove_file(SYNC_CONFIG_FILE).unwrap();
        let mut target = directory
            .directory
            .open_with("rollback-copy", &create_private_file_options())
            .unwrap();

        copy_source_to_for_rollback(&mut source, &mut target).unwrap();
        drop(target);
        let restored = fs::read(app_data.path().join("rollback-copy")).unwrap();

        assert_eq!(restored, original);
    }

    #[test]
    fn above_hard_cap_sparse_load_uses_metadata_revision_without_content_reads() {
        let app_data = tempdir().unwrap();
        fs::File::create(config_path(app_data.path()))
            .unwrap()
            .set_len(MAX_RECOVERABLE_DAMAGED_BYTES + 1)
            .unwrap();
        let directory = open_app_data(app_data.path(), false).unwrap().unwrap();
        let observed_reads = Cell::new(0_u64);

        let source = read_config_source_with_observer(&directory, |read| {
            observed_reads.set(observed_reads.get() + read as u64);
        })
        .unwrap()
        .unwrap();

        assert!(matches!(source, ConfigSource::OversizedTooLarge { .. }));
        assert_eq!(observed_reads.get(), 0);
        let SyncConfigLoadResponse::Malformed { issue, .. } =
            load_from_app_data(app_data.path()).unwrap()
        else {
            panic!("above-hard-cap config should be safely classified");
        };
        assert_eq!(issue.code, "sync-config-oversized-too-large");
    }

    #[cfg(unix)]
    fn assert_sparse_damaged_was_renamed_without_copy(
        app_data: &Path,
        original_inode: u64,
        original_length: u64,
    ) {
        let damaged = fs::read_dir(app_data)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("sync-config.damaged-"))
            })
            .unwrap();
        let metadata = fs::metadata(damaged).unwrap();
        assert_eq!(cap_fs_ext::MetadataExt::ino(&metadata), original_inode);
        assert_eq!(metadata.len(), original_length);
    }

    #[cfg(unix)]
    #[test]
    fn above_hard_cap_reset_and_recover_rename_exact_sparse_file_to_damaged() {
        for recover in [false, true] {
            let app_data = tempdir().unwrap();
            let active = config_path(app_data.path());
            fs::File::create(&active)
                .unwrap()
                .set_len(MAX_RECOVERABLE_DAMAGED_BYTES + 1)
                .unwrap();
            let original = fs::metadata(&active).unwrap();
            let original_inode = cap_fs_ext::MetadataExt::ino(&original);
            let SyncConfigLoadResponse::Malformed { revision, .. } =
                load_from_app_data(app_data.path()).unwrap()
            else {
                panic!("sparse config should be malformed");
            };

            if recover {
                recover_at_app_data(app_data.path(), &revision, SyncConfig::default()).unwrap();
            } else {
                reset_at_app_data(app_data.path(), true, Some(&revision)).unwrap();
            }

            assert_sparse_damaged_was_renamed_without_copy(
                app_data.path(),
                original_inode,
                original.len(),
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn above_hard_cap_install_failure_rolls_damaged_rename_back_to_active() {
        let app_data = tempdir().unwrap();
        let active = config_path(app_data.path());
        fs::File::create(&active)
            .unwrap()
            .set_len(MAX_RECOVERABLE_DAMAGED_BYTES + 1)
            .unwrap();
        let original = fs::metadata(&active).unwrap();
        let original_inode = cap_fs_ext::MetadataExt::ino(&original);
        let SyncConfigLoadResponse::Malformed { revision, .. } =
            load_from_app_data(app_data.path()).unwrap()
        else {
            panic!("sparse config should be malformed");
        };

        let error = reset_at_app_data_with_install_fault(app_data.path(), true, Some(&revision))
            .err()
            .unwrap();

        assert_eq!(error.code, "atomic-replace-failed");
        let restored = fs::metadata(&active).unwrap();
        assert_eq!(cap_fs_ext::MetadataExt::ino(&restored), original_inode);
        assert_eq!(restored.len(), original.len());
        assert!(!fs::read_dir(app_data.path()).unwrap().any(|entry| {
            let name = entry.unwrap().file_name().to_string_lossy().into_owned();
            name.starts_with("sync-config.damaged-") || name.starts_with(".sync-config.tmp-")
        }));
    }

    #[cfg(unix)]
    #[test]
    fn directory_swap_after_final_revalidate_never_writes_replacement_directory() {
        let parent = tempdir().unwrap();
        let app_data = parent.path().join("app-data");
        fs::create_dir(&app_data).unwrap();
        let installed = enable_at_app_data(&app_data, None).unwrap();
        let original = fs::read(config_path(&app_data)).unwrap();
        let replacement_bytes = serialized_config(&SyncConfig::default()).unwrap();
        let moved = parent.path().join("moved-app-data");
        let replacement_target = config_path(&app_data);
        let attacker_bytes = b"attacker-owned-target";

        let error = atomic_write_with_hooks(
            &app_data,
            &replacement_bytes,
            Some(&installed.document.revision),
            || {},
            || {
                fs::rename(&app_data, &moved).unwrap();
                fs::create_dir(&app_data).unwrap();
                fs::write(&replacement_target, attacker_bytes).unwrap();
            },
        )
        .err()
        .unwrap();

        assert_eq!(error.code, "unsafe-sync-config-path");
        assert_eq!(fs::read(&replacement_target).unwrap(), attacker_bytes);
        assert_eq!(fs::read(config_path(&moved)).unwrap(), original);
        assert!(!fs::read_dir(&moved).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".sync-config.tmp-")
        }));
    }

    #[test]
    fn batch_patch_is_one_revisioned_application_config_write() {
        let app_data = tempdir().unwrap();
        let enabled = enable_at_app_data(app_data.path(), None).unwrap();

        let updated = patch_batch_at_app_data(
            app_data.path(),
            &enabled.document.revision,
            vec![
                SyncConfigPatch::RemoteRoot("archive".into()),
                SyncConfigPatch::AutoSyncOnSave(true),
                SyncConfigPatch::WebDavUsername("user".into()),
                SyncConfigPatch::WebDavPassword("secret".into()),
            ],
        )
        .unwrap();

        assert_ne!(updated.document.revision, enabled.document.revision);
        assert_eq!(updated.document.config.remote_root, "archive");
        assert!(updated.document.config.auto_sync_on_save);
        assert_eq!(updated.document.config.webdav.username, "user");
        assert_eq!(updated.document.config.webdav.password, "secret");
    }
}
