#[cfg(not(mobile))]
use std::collections::BTreeMap;
use std::collections::HashMap;
#[cfg(not(mobile))]
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
#[cfg(not(mobile))]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use cap_fs_ext::{DirExt, MetadataExt};
use cap_std::fs::Dir;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::Manager;

#[cfg(not(mobile))]
use crate::storage_capability::{
    create_private_replaceable_file_options, nonfollowing_read_options,
    open_canonical_directory_nofollow, rename_retained_file_in_directory, sync_directory,
    unique_regular_file_identity, UniqueRegularFileIdentity,
};

#[cfg(not(mobile))]
const DESKTOP_PRIMARY_WORKSPACE_STORE_PATH: &str = "primary-workspace.json";
const LOCAL_STATE_SCHEMA_VERSION_KEY: &str = "schemaVersion";
const LOCAL_STATE_SCHEMA_VERSION: u64 = 3;
const PRIMARY_WORKSPACE_KEY: &str = "primaryWorkspace";
const MAX_PRIMARY_WORKSPACE_STORE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg(not(mobile))]
enum PrimaryWorkspacePersistence {
    Durable,
    PublishedWithoutDirectoryDurability,
    NotPublished,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg(not(mobile))]
pub(crate) struct PrimaryWorkspaceWriteInput {
    #[serde(default)]
    expected_state: Option<Value>,
    state: Value,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg(not(mobile))]
pub(crate) struct PrimaryWorkspaceWriteResult {
    applied: bool,
    state: Value,
}

#[cfg(not(mobile))]
trait PrimaryWorkspaceBackend: Sync {
    fn delete(&self, key: &str);
    fn get(&self, key: &str) -> Option<Value>;
    fn reload(&self) -> Result<(), String> {
        Ok(())
    }
    fn save(&self) -> Result<(), String>;
    fn save_publication(&self) -> PrimaryWorkspacePersistence {
        match self.save() {
            Ok(()) => PrimaryWorkspacePersistence::Durable,
            Err(_) => PrimaryWorkspacePersistence::NotPublished,
        }
    }
    fn restore_transaction(
        &self,
        schema_version: Option<Value>,
        primary_workspace: Option<Value>,
    ) -> Result<(), String> {
        if let Some(schema_version) = schema_version {
            self.set(LOCAL_STATE_SCHEMA_VERSION_KEY, schema_version);
        } else {
            self.delete(LOCAL_STATE_SCHEMA_VERSION_KEY);
        }
        if let Some(primary_workspace) = primary_workspace {
            self.set(PRIMARY_WORKSPACE_KEY, primary_workspace);
        } else {
            self.delete(PRIMARY_WORKSPACE_KEY);
        }
        self.save()
    }
    fn set(&self, key: &str, value: Value);
}

/// Host-private, path-free identities for child Kernel launches.
///
/// The Desktop host-owned primary-workspace record supplies the current path
/// guard. Opaque child identities remain in a separate native durable store.
#[cfg(not(mobile))]
struct NativeHostWorkspaceStatePersistence<'a, Backend: PrimaryWorkspaceBackend + ?Sized> {
    backend: &'a Backend,
    native_store: &'a qingyu_kernel::host::native::NativeHostWorkspaceStore,
    transaction_lock: &'a Mutex<()>,
}

#[cfg(not(mobile))]
impl<'a, Backend: PrimaryWorkspaceBackend + ?Sized>
    NativeHostWorkspaceStatePersistence<'a, Backend>
{
    fn new(
        backend: &'a Backend,
        native_store: &'a qingyu_kernel::host::native::NativeHostWorkspaceStore,
        transaction_lock: &'a Mutex<()>,
    ) -> Self {
        Self {
            backend,
            native_store,
            transaction_lock,
        }
    }

    fn load_or_create(
        &self,
        requested_root: &Path,
    ) -> Result<qingyu_kernel::host::native::NativeHostWorkspaceState, String> {
        let _transaction = self
            .transaction_lock
            .lock()
            .map_err(|_| persistence_error())?;
        self.backend.reload()?;
        let authoritative = authoritative_primary_workspace_root(
            self.backend.get(PRIMARY_WORKSPACE_KEY),
            PrimaryWorkspaceKind::Desktop,
            None,
        )?;
        let requested = requested_root
            .to_str()
            .ok_or_else(sync_primary_workspace_mismatch)
            .and_then(crate::workspace_membership::canonical_workspace_root)
            .map_err(|_| sync_primary_workspace_mismatch())?;
        if requested != authoritative {
            return Err(sync_primary_workspace_mismatch());
        }

        let display_name = crate::notebook_scope::notebook_name_from_root(&authoritative)
            .map_err(|_| sync_primary_workspace_unavailable())?;
        self.native_store
            .load_or_create(&authoritative, display_name)
            .map_err(|_| persistence_error())
    }
}

/// Host-private preparation for transactional Desktop runtime ownership.
///
/// Implementations retain the selected path and an atomic path-transition
/// reservation internally. The returned Kernel transaction exposes only the
/// opaque canonical workspace value at commit time.
#[cfg(not(mobile))]
pub(crate) trait TrustedDesktopWorkspacePersistence: Send + Sync {
    fn prepare_host_workspace_transaction(
        &self,
        absolute_path: &Path,
        authority_binding: qingyu_kernel::workspace::primary::PreparedWorkspaceAuthorityBinding,
    ) -> Result<
        Box<dyn qingyu_kernel::workspace::primary::AtomicHostWorkspaceTransaction>,
        qingyu_kernel::services::workspace::WorkspaceServiceError,
    >;
}

#[cfg(not(mobile))]
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CanonicalPrimaryWorkspaceDocument {
    schema_version: u64,
    primary_workspace: StoredPrimaryWorkspaceState,
}

#[cfg(not(mobile))]
struct DesktopDiskPrimaryWorkspaceBackend {
    app_data_root: PathBuf,
    expected_target: Mutex<Option<UniqueRegularFileIdentity>>,
    values: Mutex<BTreeMap<String, Value>>,
}

#[cfg(not(mobile))]
impl DesktopDiskPrimaryWorkspaceBackend {
    fn open<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<Self, String> {
        let app_data_root = app.path().app_data_dir().map_err(|_| persistence_error())?;
        std::fs::create_dir_all(&app_data_root).map_err(|_| persistence_error())?;
        Ok(Self::empty_at(app_data_root))
    }

    fn empty_at(app_data_root: PathBuf) -> Self {
        Self {
            app_data_root,
            expected_target: Mutex::new(None),
            values: Mutex::new(BTreeMap::new()),
        }
    }

    #[cfg(test)]
    fn open_at(app_data_root: PathBuf) -> Result<Self, String> {
        let backend = Self::empty_at(app_data_root);
        backend.reload()?;
        Ok(backend)
    }

    fn reload_from_disk(&self) -> Result<(), String> {
        let loaded =
            read_workspace_store_file(&self.app_data_root, DESKTOP_PRIMARY_WORKSPACE_STORE_PATH)?;
        let expected_target = match loaded.as_ref() {
            Some(state) => {
                let published = read_workspace_store_file(
                    &self.app_data_root,
                    DESKTOP_PRIMARY_WORKSPACE_STORE_PATH,
                )?
                .ok_or_else(persistence_error)?;
                if published.identity != state.identity {
                    return Err(persistence_error());
                }
                Some(published.identity)
            }
            None => None,
        };
        let values = loaded.map(|state| state.values).unwrap_or_default();
        *self
            .expected_target
            .lock()
            .map_err(|_| persistence_error())? = expected_target;
        *self.values.lock().map_err(|_| persistence_error())? = values;
        Ok(())
    }

    fn save_publication_with_sync<SyncDirectory>(
        &self,
        sync_after_rename: SyncDirectory,
    ) -> PrimaryWorkspacePersistence
    where
        SyncDirectory: FnOnce(&cap_std::fs::Dir) -> io::Result<()>,
    {
        let values = match self.values.lock() {
            Ok(values) => values.clone(),
            Err(_) => return PrimaryWorkspacePersistence::NotPublished,
        };
        let bytes = match serialize_primary_workspace_document(&values) {
            Ok(bytes) => bytes,
            Err(_) => return PrimaryWorkspacePersistence::NotPublished,
        };
        let expected = match self.expected_target.lock() {
            Ok(expected) => *expected,
            Err(_) => return PrimaryWorkspacePersistence::NotPublished,
        };
        let publication = replace_primary_workspace_file_atomically_with_hooks(
            &self.app_data_root,
            &bytes,
            Some(expected),
            || Ok(()),
            || Ok(()),
            sync_after_rename,
        );
        if publication != PrimaryWorkspacePersistence::NotPublished {
            let identity = match read_workspace_store_file(
                &self.app_data_root,
                DESKTOP_PRIMARY_WORKSPACE_STORE_PATH,
            ) {
                Ok(Some(state)) => Some(state.identity),
                Ok(None) | Err(_) => {
                    return PrimaryWorkspacePersistence::PublishedWithoutDirectoryDurability;
                }
            };
            if let Ok(mut expected_target) = self.expected_target.lock() {
                *expected_target = identity;
            } else {
                return PrimaryWorkspacePersistence::PublishedWithoutDirectoryDurability;
            }
        }
        publication
    }

    fn restore_values(
        &self,
        schema_version: Option<Value>,
        primary_workspace: Option<Value>,
    ) -> Result<(), String> {
        let mut values = self.values.lock().map_err(|_| persistence_error())?;
        if let Some(schema_version) = schema_version {
            values.insert(LOCAL_STATE_SCHEMA_VERSION_KEY.to_owned(), schema_version);
        } else {
            values.remove(LOCAL_STATE_SCHEMA_VERSION_KEY);
        }
        if let Some(primary_workspace) = primary_workspace {
            values.insert(PRIMARY_WORKSPACE_KEY.to_owned(), primary_workspace);
        } else {
            values.remove(PRIMARY_WORKSPACE_KEY);
        }
        Ok(())
    }

    fn remove_published_authority(&self) -> Result<(), String> {
        let published_identity = self
            .expected_target
            .lock()
            .map_err(|_| persistence_error())?
            .ok_or_else(persistence_error)?;
        remove_primary_workspace_file_with_identity(&self.app_data_root, published_identity)?;
        *self
            .expected_target
            .lock()
            .map_err(|_| persistence_error())? = None;
        Ok(())
    }
}

#[cfg(not(mobile))]
impl PrimaryWorkspaceBackend for DesktopDiskPrimaryWorkspaceBackend {
    fn delete(&self, key: &str) {
        if let Ok(mut values) = self.values.lock() {
            values.remove(key);
        }
    }

    fn get(&self, key: &str) -> Option<Value> {
        self.values.lock().ok()?.get(key).cloned()
    }

    fn reload(&self) -> Result<(), String> {
        self.reload_from_disk()
    }

    fn save(&self) -> Result<(), String> {
        match self.save_publication() {
            PrimaryWorkspacePersistence::Durable => Ok(()),
            PrimaryWorkspacePersistence::PublishedWithoutDirectoryDurability
            | PrimaryWorkspacePersistence::NotPublished => Err(persistence_error()),
        }
    }

    fn save_publication(&self) -> PrimaryWorkspacePersistence {
        self.save_publication_with_sync(sync_directory)
    }

    fn restore_transaction(
        &self,
        schema_version: Option<Value>,
        primary_workspace: Option<Value>,
    ) -> Result<(), String> {
        let original_was_absent = schema_version.is_none() && primary_workspace.is_none();
        self.restore_values(schema_version, primary_workspace)?;
        if original_was_absent {
            self.remove_published_authority()
        } else {
            self.save()
        }
    }

    fn set(&self, key: &str, value: Value) {
        if let Ok(mut values) = self.values.lock() {
            values.insert(key.to_string(), value);
        }
    }
}

#[cfg(not(mobile))]
struct ExistingPrimaryWorkspaceFile {
    identity: UniqueRegularFileIdentity,
    values: BTreeMap<String, Value>,
}

#[cfg(not(mobile))]
fn read_workspace_store_file(
    app_data_root: &Path,
    file_name: &str,
) -> Result<Option<ExistingPrimaryWorkspaceFile>, String> {
    let directory =
        open_canonical_directory_nofollow(app_data_root).map_err(|_| persistence_error())?;
    let addressed = match directory.symlink_metadata(file_name) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(persistence_error()),
    };
    if addressed.len() > MAX_PRIMARY_WORKSPACE_STORE_BYTES as u64 {
        return Err(persistence_error());
    }
    let identity = unique_regular_file_identity(&addressed).ok_or_else(persistence_error)?;
    let mut retained = directory
        .open_with(file_name, &nonfollowing_read_options())
        .map_err(|_| persistence_error())?;
    let retained_identity = retained
        .metadata()
        .ok()
        .and_then(|metadata| unique_regular_file_identity(&metadata))
        .ok_or_else(persistence_error)?;
    if retained_identity != identity {
        return Err(persistence_error());
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut retained)
        .take(MAX_PRIMARY_WORKSPACE_STORE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| persistence_error())?;
    if bytes.len() > MAX_PRIMARY_WORKSPACE_STORE_BYTES {
        return Err(persistence_error());
    }
    let rechecked = directory
        .symlink_metadata(file_name)
        .ok()
        .and_then(|metadata| unique_regular_file_identity(&metadata));
    if rechecked != Some(identity) {
        return Err(persistence_error());
    }
    let document = deserialize_primary_workspace_document(&bytes)?;
    let primary_workspace = serialize_primary_workspace_state(&document.primary_workspace)?;
    let values = BTreeMap::from([
        (
            LOCAL_STATE_SCHEMA_VERSION_KEY.to_owned(),
            Value::from(document.schema_version),
        ),
        (PRIMARY_WORKSPACE_KEY.to_owned(), primary_workspace),
    ]);
    Ok(Some(ExistingPrimaryWorkspaceFile { identity, values }))
}

#[cfg(not(mobile))]
fn remove_primary_workspace_file_with_identity(
    app_data_root: &Path,
    expected_identity: UniqueRegularFileIdentity,
) -> Result<(), String> {
    static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
    let directory =
        open_canonical_directory_nofollow(app_data_root).map_err(|_| persistence_error())?;
    let retained = directory
        .open_with(
            DESKTOP_PRIMARY_WORKSPACE_STORE_PATH,
            &nonfollowing_primary_workspace_removal_options(),
        )
        .map_err(|_| persistence_error())?;
    let retained_identity = retained
        .metadata()
        .ok()
        .and_then(|metadata| unique_regular_file_identity(&metadata))
        .ok_or_else(persistence_error)?;
    if retained_identity != expected_identity {
        return Err(persistence_error());
    }

    let rollback_name = (0..1000).find_map(|_| {
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let candidate = format!(
            ".primary-workspace-rollback-{}-{sequence}.tmp",
            std::process::id()
        );
        match rename_retained_file_in_directory(
            &directory,
            &retained,
            DESKTOP_PRIMARY_WORKSPACE_STORE_PATH,
            expected_identity,
            &candidate,
            false,
        ) {
            Ok(()) => Some(Ok(candidate)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => None,
            Err(_) => Some(Err(())),
        }
    });
    let Some(Ok(rollback_name)) = rollback_name else {
        return Err(persistence_error());
    };
    let rollback_identity = directory
        .symlink_metadata(&rollback_name)
        .ok()
        .and_then(|metadata| unique_regular_file_identity(&metadata));
    if rollback_identity != Some(expected_identity) {
        return Err(persistence_error());
    }
    drop(retained);
    directory
        .remove_file(&rollback_name)
        .map_err(|_| persistence_error())?;
    sync_directory(&directory).map_err(|_| persistence_error())
}

#[cfg(not(mobile))]
fn nonfollowing_primary_workspace_removal_options() -> cap_std::fs::OpenOptions {
    let mut options = nonfollowing_read_options();
    #[cfg(windows)]
    {
        use cap_fs_ext::OpenOptionsExt;

        options
            .access_mode(
                windows_sys::Win32::Foundation::GENERIC_READ
                    | windows_sys::Win32::Storage::FileSystem::DELETE,
            )
            .share_mode(windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ);
    }
    options
}

#[cfg(not(mobile))]
fn replace_primary_workspace_file_atomically_with_hooks<BeforeStage, AfterStage, SyncDirectory>(
    app_data_root: &Path,
    bytes: &[u8],
    expected_target: Option<Option<UniqueRegularFileIdentity>>,
    before_stage: BeforeStage,
    after_stage: AfterStage,
    sync_after_rename: SyncDirectory,
) -> PrimaryWorkspacePersistence
where
    BeforeStage: FnOnce() -> io::Result<()>,
    AfterStage: FnOnce() -> io::Result<()>,
    SyncDirectory: FnOnce(&cap_std::fs::Dir) -> io::Result<()>,
{
    static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
    let Ok(directory) = open_canonical_directory_nofollow(app_data_root) else {
        return PrimaryWorkspacePersistence::NotPublished;
    };
    let existing = match directory.symlink_metadata(DESKTOP_PRIMARY_WORKSPACE_STORE_PATH) {
        Ok(metadata) => match unique_regular_file_identity(&metadata) {
            Some(identity) => Some(identity),
            None => return PrimaryWorkspacePersistence::NotPublished,
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(_) => return PrimaryWorkspacePersistence::NotPublished,
    };
    if expected_target.is_some_and(|expected| expected != existing) {
        return PrimaryWorkspacePersistence::NotPublished;
    }
    if before_stage().is_err() {
        return PrimaryWorkspacePersistence::NotPublished;
    }
    let staged = (0..1000).find_map(|_| {
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let name = format!(".primary-workspace-{}-{sequence}.tmp", std::process::id());
        match directory.open_with(&name, &create_private_replaceable_file_options()) {
            Ok(file) => Some(Ok((name, file))),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => None,
            Err(_) => Some(Err(())),
        }
    });
    let Some(Ok((staged_name, mut staged))) = staged else {
        return PrimaryWorkspacePersistence::NotPublished;
    };
    if staged
        .write_all(bytes)
        .and_then(|()| staged.sync_all())
        .and_then(|()| after_stage())
        .is_err()
    {
        drop(staged);
        let _cleanup = directory.remove_file(&staged_name);
        return PrimaryWorkspacePersistence::NotPublished;
    }
    let Some(staged_identity) = staged
        .metadata()
        .ok()
        .and_then(|metadata| unique_regular_file_identity(&metadata))
    else {
        drop(staged);
        let _cleanup = directory.remove_file(&staged_name);
        return PrimaryWorkspacePersistence::NotPublished;
    };
    let retained = match directory.symlink_metadata(DESKTOP_PRIMARY_WORKSPACE_STORE_PATH) {
        Ok(metadata) => match unique_regular_file_identity(&metadata) {
            Some(identity) => Some(identity),
            None => {
                drop(staged);
                let _cleanup = directory.remove_file(&staged_name);
                return PrimaryWorkspacePersistence::NotPublished;
            }
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(_) => {
            drop(staged);
            let _cleanup = directory.remove_file(&staged_name);
            return PrimaryWorkspacePersistence::NotPublished;
        }
    };
    if retained != existing
        || rename_retained_file_in_directory(
            &directory,
            &staged,
            &staged_name,
            staged_identity,
            DESKTOP_PRIMARY_WORKSPACE_STORE_PATH,
            existing.is_some(),
        )
        .is_err()
    {
        drop(staged);
        let _cleanup = directory.remove_file(&staged_name);
        return PrimaryWorkspacePersistence::NotPublished;
    }
    drop(staged);
    match sync_after_rename(&directory) {
        Ok(()) => PrimaryWorkspacePersistence::Durable,
        Err(_) => PrimaryWorkspacePersistence::PublishedWithoutDirectoryDurability,
    }
}

#[cfg(not(mobile))]
fn with_primary_workspace_backend<R: tauri::Runtime, T>(
    app: &tauri::AppHandle<R>,
    operation: impl FnOnce(&dyn PrimaryWorkspaceBackend) -> Result<T, String>,
) -> Result<T, String> {
    let backend = DesktopDiskPrimaryWorkspaceBackend::open(app)?;
    operation(&backend)
}

// Retained for host-transaction tests and non-renderer workspace operations;
// the production Desktop child owner composes its authority directly.
#[allow(dead_code)]
#[cfg(not(mobile))]
struct TrustedPreparedWorkspace {
    authority: qingyu_kernel::runtime::PreparedWorkspaceAuthority,
    display_name: String,
    host_transaction: Box<dyn qingyu_kernel::workspace::primary::AtomicHostWorkspaceTransaction>,
}

#[derive(Clone, Eq, PartialEq)]
#[allow(dead_code)]
#[cfg(not(mobile))]
pub(crate) struct TrustedPreparedWorkspaceToken {
    token: String,
}

#[cfg(not(mobile))]
impl std::fmt::Debug for TrustedPreparedWorkspaceToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("TrustedPreparedWorkspaceToken([REDACTED])")
    }
}

#[allow(dead_code)]
#[cfg(not(mobile))]
pub(crate) struct TrustedDesktopWorkspaceAdapter {
    runtime: Arc<qingyu_kernel::runtime::KernelRuntime>,
    service: Arc<qingyu_kernel::services::workspace::WorkspaceService>,
    persistence: Arc<dyn TrustedDesktopWorkspacePersistence>,
    prepared: Mutex<HashMap<String, TrustedPreparedWorkspace>>,
}

#[allow(dead_code)]
#[cfg(not(mobile))]
impl TrustedDesktopWorkspaceAdapter {
    pub(crate) fn new(
        runtime: Arc<qingyu_kernel::runtime::KernelRuntime>,
        service: Arc<qingyu_kernel::services::workspace::WorkspaceService>,
        persistence: Arc<dyn TrustedDesktopWorkspacePersistence>,
    ) -> Self {
        Self {
            runtime,
            service,
            persistence,
            prepared: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn current(
        &self,
    ) -> Result<
        qingyu_kernel::contract::WorkspaceDto,
        qingyu_kernel::services::workspace::WorkspaceServiceError,
    > {
        self.service.current()
    }

    pub(crate) fn prepare_host_workspace(
        &self,
        absolute_path: &Path,
    ) -> Result<
        TrustedPreparedWorkspaceToken,
        qingyu_kernel::services::workspace::WorkspaceServiceError,
    > {
        self.service.current()?;
        let display_name =
            crate::notebook_scope::notebook_name_from_root(absolute_path).map_err(|_| {
                qingyu_kernel::services::workspace::WorkspaceServiceError::invalid_workspace()
            })?;
        let authority = self
            .runtime
            .prepare_host_workspace_authority(absolute_path)
            .map_err(qingyu_kernel::services::workspace::WorkspaceServiceError::from)?;
        let host_transaction = self
            .persistence
            .prepare_host_workspace_transaction(absolute_path, authority.binding())?;
        let token = format!(
            "{}:{}",
            self.runtime.instance_id().as_uuid(),
            uuid::Uuid::new_v4()
        );
        self.prepared
            .lock()
            .map_err(|_| qingyu_kernel::services::workspace::WorkspaceServiceError::unavailable())?
            .insert(
                token.clone(),
                TrustedPreparedWorkspace {
                    authority,
                    display_name,
                    host_transaction,
                },
            );
        Ok(TrustedPreparedWorkspaceToken { token })
    }

    pub(crate) async fn compare_and_set_host_workspace(
        &self,
        expected_revision: &qingyu_kernel::contract::Revision,
        prepared: TrustedPreparedWorkspaceToken,
    ) -> Result<
        qingyu_kernel::contract::WorkspaceDto,
        qingyu_kernel::services::workspace::WorkspaceServiceError,
    > {
        let prepared = self
            .prepared
            .lock()
            .map_err(|_| {
                qingyu_kernel::services::workspace::WorkspaceServiceError::unavailable()
            })?
            .remove(&prepared.token)
            .ok_or_else(
                qingyu_kernel::services::workspace::WorkspaceServiceError::prepared_authority_mismatch,
            )?;
        self.service
            .compare_and_set_host_workspace_transaction(
                expected_revision,
                prepared.authority,
                prepared.display_name,
                prepared.host_transaction,
            )
            .await
    }
}

#[cfg(not(mobile))]
struct PrimaryWorkspaceService<'a, Backend: PrimaryWorkspaceBackend + ?Sized> {
    backend: &'a Backend,
    transaction_lock: &'a Mutex<()>,
}

#[cfg(not(mobile))]
impl<'a, Backend: PrimaryWorkspaceBackend + ?Sized> PrimaryWorkspaceService<'a, Backend> {
    fn new(backend: &'a Backend, transaction_lock: &'a Mutex<()>) -> Self {
        Self {
            backend,
            transaction_lock,
        }
    }

    fn read(&self) -> Result<Option<Value>, String> {
        self.with_current(Ok)
    }

    fn with_current<T>(
        &self,
        operation: impl FnOnce(Option<Value>) -> Result<T, String>,
    ) -> Result<T, String> {
        self.with_transaction(|service| operation(service.backend.get(PRIMARY_WORKSPACE_KEY)))
    }

    fn with_transaction<T>(
        &self,
        operation: impl FnOnce(&Self) -> Result<T, String>,
    ) -> Result<T, String> {
        let _transaction = self
            .transaction_lock
            .lock()
            .map_err(|_| persistence_error())?;
        self.backend.reload()?;
        operation(self)
    }

    fn restore_value(&self, key: &str, value: Option<Value>) {
        if let Some(value) = value {
            self.backend.set(key, value);
        } else {
            self.backend.delete(key);
        }
    }

    #[cfg(test)]
    fn write(
        &self,
        input: PrimaryWorkspaceWriteInput,
    ) -> Result<PrimaryWorkspaceWriteResult, String> {
        self.write_validated(input, || Ok(()))
    }

    fn write_with_primary_root_guard(
        &self,
        input: PrimaryWorkspaceWriteInput,
        proposed_root: Option<&Path>,
        registry: &crate::dejavu_sync::path_guard::NativeWorkingTreeRegistry,
    ) -> Result<PrimaryWorkspaceWriteResult, String> {
        self.write_validated(input, || registry.validate_primary_root(proposed_root))
    }

    fn write_validated(
        &self,
        input: PrimaryWorkspaceWriteInput,
        mut validate: impl FnMut() -> Result<(), String>,
    ) -> Result<PrimaryWorkspaceWriteResult, String> {
        self.with_transaction(|service| service.write_validated_locked(input, &mut validate))
    }

    fn write_validated_locked(
        &self,
        input: PrimaryWorkspaceWriteInput,
        validate: &mut impl FnMut() -> Result<(), String>,
    ) -> Result<PrimaryWorkspaceWriteResult, String> {
        let previous_schema_version = self.backend.get(LOCAL_STATE_SCHEMA_VERSION_KEY);
        let previous_primary_workspace = self.backend.get(PRIMARY_WORKSPACE_KEY);
        if !local_state_schema_is_supported(previous_schema_version.as_ref()) {
            return Err(persistence_error());
        }
        let proposed_state = deserialize_current_primary_workspace_state(input.state)?;
        let proposed_state = serialize_primary_workspace_state(&proposed_state)?;
        let current = previous_primary_workspace.clone().unwrap_or(Value::Null);
        if input
            .expected_state
            .as_ref()
            .is_some_and(|expected| expected != &current)
        {
            return Ok(PrimaryWorkspaceWriteResult {
                applied: false,
                state: current,
            });
        }
        validate()?;

        self.backend.set(
            LOCAL_STATE_SCHEMA_VERSION_KEY,
            Value::from(LOCAL_STATE_SCHEMA_VERSION),
        );
        self.backend
            .set(PRIMARY_WORKSPACE_KEY, proposed_state.clone());
        match self.backend.save_publication() {
            PrimaryWorkspacePersistence::Durable => {}
            PrimaryWorkspacePersistence::PublishedWithoutDirectoryDurability => {
                return Err(persistence_error());
            }
            PrimaryWorkspacePersistence::NotPublished => {
                self.restore_value(PRIMARY_WORKSPACE_KEY, previous_primary_workspace);
                self.restore_value(LOCAL_STATE_SCHEMA_VERSION_KEY, previous_schema_version);
                return Err(persistence_error());
            }
        }
        if let Err(error) = validate() {
            self.backend
                .restore_transaction(previous_schema_version, previous_primary_workspace)
                .map_err(|_| persistence_error())?;
            return Err(error);
        }

        Ok(PrimaryWorkspaceWriteResult {
            applied: true,
            state: proposed_state,
        })
    }
}

#[cfg(not(mobile))]
fn local_state_schema_is_supported(value: Option<&Value>) -> bool {
    match value {
        None => true,
        Some(value) if value.as_u64() == Some(LOCAL_STATE_SCHEMA_VERSION) => true,
        Some(_) => false,
    }
}

#[cfg(not(mobile))]
pub(crate) fn primary_workspace_transaction_gate() -> &'static Mutex<()> {
    static TRANSACTION_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    TRANSACTION_LOCK.get_or_init(|| Mutex::new(()))
}

fn persistence_error() -> String {
    "primary workspace persistence is unavailable".to_string()
}

#[cfg(not(mobile))]
fn notebook_target_error() -> String {
    "notebook-target-invalid: The notebook target is unavailable.".to_string()
}

#[cfg(not(mobile))]
struct PreparedDesktopNotebookDirectory {
    directory: Dir,
    identity: crate::storage_capability::DirectoryIdentity,
    notes_root: PathBuf,
    parent: PathBuf,
    parent_directory: Dir,
    parent_identity: crate::storage_capability::DirectoryIdentity,
    target_name: String,
    expected_primary_workspace: Value,
    restore_generation: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg(not(mobile))]
pub(crate) struct PreparedDesktopNotebookTarget {
    lease: String,
    notes_root: String,
}

#[cfg(not(mobile))]
pub(crate) struct ConsumedPreparedDesktopNotebookTarget {
    pub(crate) directory: Dir,
    pub(crate) notes_root: PathBuf,
    identity: crate::storage_capability::DirectoryIdentity,
    parent: PathBuf,
    parent_directory: Dir,
    parent_identity: crate::storage_capability::DirectoryIdentity,
    target_name: String,
    expected_primary_workspace: Value,
    restore_generation: String,
}

#[cfg(not(mobile))]
impl ConsumedPreparedDesktopNotebookTarget {
    pub(crate) fn restore_generation(&self) -> &str {
        &self.restore_generation
    }

    pub(crate) fn validate_current_address(&self) -> Result<(), String> {
        let ambient_parent =
            crate::storage_capability::open_canonical_directory_nofollow(&self.parent)
                .map_err(|_| notebook_target_error())?;
        if crate::storage_capability::directory_identity(&ambient_parent)
            .map_err(|_| notebook_target_error())?
            != self.parent_identity
            || crate::storage_capability::directory_identity(&self.parent_directory)
                .map_err(|_| notebook_target_error())?
                != self.parent_identity
        {
            return Err(notebook_target_error());
        }
        let addressed = ambient_parent
            .symlink_metadata(&self.target_name)
            .map_err(|_| notebook_target_error())?;
        if addressed.file_type().is_symlink() || !addressed.is_dir() {
            return Err(notebook_target_error());
        }
        let current = ambient_parent
            .open_dir_nofollow(&self.target_name)
            .map_err(|_| notebook_target_error())?;
        let current_identity = crate::storage_capability::directory_identity(&current)
            .map_err(|_| notebook_target_error())?;
        let retained_identity = crate::storage_capability::directory_identity(&self.directory)
            .map_err(|_| notebook_target_error())?;
        if current_identity != self.identity || retained_identity != self.identity {
            return Err(notebook_target_error());
        }
        let canonical = self
            .notes_root
            .canonicalize()
            .map_err(|_| notebook_target_error())?;
        if canonical != self.notes_root
            || canonical
                .strip_prefix(&self.parent)
                .ok()
                .filter(|relative| *relative == Path::new(&self.target_name))
                .is_none()
        {
            return Err(notebook_target_error());
        }
        Ok(())
    }

    fn desired_primary_workspace_state(&self) -> Value {
        serde_json::json!({
            "desktopWorkspaceRoot": self.parent.to_string_lossy(),
            "desktopPath": self.notes_root.to_string_lossy(),
            "managedName": null,
            "onboardingCompleted": true,
            "version": 3
        })
    }

    fn commit_primary_workspace_with_backend(
        &self,
        backend: &dyn PrimaryWorkspaceBackend,
        lock: &Mutex<()>,
    ) -> Result<PrimaryWorkspaceWriteResult, String> {
        self.commit_primary_workspace_with_backend_and_registry(
            backend,
            lock,
            crate::dejavu_sync::path_guard::native_working_tree_registry(),
        )
    }

    fn commit_primary_workspace_with_backend_and_registry(
        &self,
        backend: &dyn PrimaryWorkspaceBackend,
        lock: &Mutex<()>,
        registry: &crate::dejavu_sync::path_guard::NativeWorkingTreeRegistry,
    ) -> Result<PrimaryWorkspaceWriteResult, String> {
        let result = PrimaryWorkspaceService::new(backend, lock).write_validated(
            PrimaryWorkspaceWriteInput {
                expected_state: Some(self.expected_primary_workspace.clone()),
                state: self.desired_primary_workspace_state(),
            },
            || {
                self.validate_current_address()?;
                registry.validate_primary_root(Some(&self.notes_root))
            },
        )?;
        if !result.applied {
            return Err(notebook_target_error());
        }
        Ok(result)
    }

    pub(crate) fn commit_primary_workspace<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
    ) -> Result<PrimaryWorkspaceWriteResult, String> {
        with_primary_workspace_backend(app, |backend| {
            self.commit_primary_workspace_with_backend(
                backend,
                primary_workspace_transaction_gate(),
            )
        })
    }
}

#[cfg(not(mobile))]
fn prepared_desktop_notebook_targets(
) -> &'static Mutex<HashMap<String, PreparedDesktopNotebookDirectory>> {
    static TARGETS: OnceLock<Mutex<HashMap<String, PreparedDesktopNotebookDirectory>>> =
        OnceLock::new();
    TARGETS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(not(mobile))]
fn prepared_target_lease() -> Result<String, String> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut entropy = [0_u8; 24];
    getrandom::fill(&mut entropy).map_err(|_| notebook_target_error())?;
    let mut lease = String::with_capacity(entropy.len() * 2);
    for byte in entropy {
        lease.push(HEX[(byte >> 4) as usize] as char);
        lease.push(HEX[(byte & 0x0f) as usize] as char);
    }
    Ok(lease)
}

#[cfg(not(mobile))]
fn open_desktop_notebook_target(
    parent_path: &str,
    notebook_name: &str,
    expected_primary_workspace: Value,
) -> Result<PreparedDesktopNotebookDirectory, String> {
    let target_name = crate::notebook_scope::validate_notebook_name(notebook_name)?;
    let parent = Path::new(parent_path)
        .canonicalize()
        .map_err(|_| notebook_target_error())?;
    let parent_directory = crate::storage_capability::open_canonical_directory_nofollow(&parent)
        .map_err(|_| notebook_target_error())?;
    let parent_identity = crate::storage_capability::directory_identity(&parent_directory)
        .map_err(|_| notebook_target_error())?;

    match parent_directory.symlink_metadata(&target_name) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(notebook_target_error())
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if let Err(error) = parent_directory.create_dir(&target_name) {
                if error.kind() != std::io::ErrorKind::AlreadyExists {
                    return Err(notebook_target_error());
                }
            }
        }
        Err(_) => return Err(notebook_target_error()),
    }

    let addressed = parent_directory
        .symlink_metadata(&target_name)
        .map_err(|_| notebook_target_error())?;
    if addressed.file_type().is_symlink() || !addressed.is_dir() {
        return Err(notebook_target_error());
    }
    let directory = parent_directory
        .open_dir_nofollow(&target_name)
        .map_err(|_| notebook_target_error())?;
    let retained = directory
        .dir_metadata()
        .map_err(|_| notebook_target_error())?;
    if addressed.dev() != retained.dev() || addressed.ino() != retained.ino() {
        return Err(notebook_target_error());
    }
    let identity = crate::storage_capability::directory_identity(&directory)
        .map_err(|_| notebook_target_error())?;
    let notes_root = parent.join(&target_name);
    let canonical = notes_root
        .canonicalize()
        .map_err(|_| notebook_target_error())?;
    if canonical != notes_root
        || canonical
            .strip_prefix(&parent)
            .ok()
            .filter(|relative| *relative == Path::new(&target_name))
            .is_none()
    {
        return Err(notebook_target_error());
    }

    Ok(PreparedDesktopNotebookDirectory {
        directory,
        identity,
        notes_root,
        parent,
        parent_directory,
        parent_identity,
        target_name,
        expected_primary_workspace,
        restore_generation: String::new(),
    })
}

#[cfg(test)]
pub(crate) fn prepare_desktop_notebook_target_at_path(
    parent_path: &str,
    notebook_name: &str,
) -> Result<PathBuf, String> {
    open_desktop_notebook_target(parent_path, notebook_name, Value::Null)
        .map(|target| target.notes_root)
}

#[cfg(test)]
pub(crate) fn prepare_desktop_notebook_target_lease_at_path(
    parent_path: &str,
    notebook_name: &str,
) -> Result<PreparedDesktopNotebookTarget, String> {
    prepare_desktop_notebook_target_lease_at_path_with_expected(parent_path, notebook_name, None)
}

#[cfg(not(mobile))]
fn prepare_desktop_notebook_target_lease_at_path_with_expected(
    parent_path: &str,
    notebook_name: &str,
    expected_primary_workspace: Option<Value>,
) -> Result<PreparedDesktopNotebookTarget, String> {
    let mut target = open_desktop_notebook_target(
        parent_path,
        notebook_name,
        expected_primary_workspace.unwrap_or(Value::Null),
    )?;
    let notes_root = target.notes_root.to_string_lossy().into_owned();
    let lease = prepared_target_lease()?;
    target.restore_generation = format!("{}-{lease}", target.identity.stable_token());
    prepared_desktop_notebook_targets()
        .lock()
        .map_err(|_| notebook_target_error())?
        .insert(lease.clone(), target);
    Ok(PreparedDesktopNotebookTarget { lease, notes_root })
}

#[cfg(not(mobile))]
pub(crate) fn consume_prepared_desktop_notebook_target(
    lease: &str,
) -> Result<ConsumedPreparedDesktopNotebookTarget, String> {
    let target = prepared_desktop_notebook_targets()
        .lock()
        .map_err(|_| notebook_target_error())?
        .remove(lease)
        .ok_or_else(notebook_target_error)?;
    let consumed = ConsumedPreparedDesktopNotebookTarget {
        directory: target.directory,
        notes_root: target.notes_root,
        identity: target.identity,
        parent: target.parent,
        parent_directory: target.parent_directory,
        parent_identity: target.parent_identity,
        target_name: target.target_name,
        expected_primary_workspace: target.expected_primary_workspace,
        restore_generation: target.restore_generation,
    };
    consumed.validate_current_address()?;
    Ok(consumed)
}

#[cfg(not(mobile))]
pub(crate) fn discard_prepared_desktop_notebook_target_lease(lease: &str) -> Result<(), String> {
    prepared_desktop_notebook_targets()
        .lock()
        .map_err(|_| notebook_target_error())?
        .remove(lease);
    Ok(())
}

fn sync_primary_workspace_unavailable() -> String {
    "sync-primary-workspace-unavailable: The primary workspace is unavailable.".to_string()
}

pub(crate) fn sync_primary_workspace_mismatch() -> String {
    "sync-primary-workspace-mismatch: The requested notes root is not the primary workspace."
        .to_string()
}

#[derive(Clone, Copy)]
#[cfg(not(mobile))]
enum PrimaryWorkspaceKind {
    Desktop,
    #[cfg(test)]
    Mobile,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[cfg(not(mobile))]
struct StoredPrimaryWorkspaceState {
    #[serde(deserialize_with = "deserialize_required_nullable_string")]
    desktop_workspace_root: Option<String>,
    #[serde(deserialize_with = "deserialize_required_nullable_string")]
    desktop_path: Option<String>,
    #[serde(deserialize_with = "deserialize_required_nullable_string")]
    managed_name: Option<String>,
    onboarding_completed: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    onboarding_requested_for_next_launch: bool,
    version: u64,
}

#[cfg(not(mobile))]
fn deserialize_required_nullable_string<'de, Deserializer>(
    deserializer: Deserializer,
) -> Result<Option<String>, Deserializer::Error>
where
    Deserializer: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)
}

#[cfg(not(mobile))]
fn is_false(value: &bool) -> bool {
    !*value
}

#[cfg(not(mobile))]
fn deserialize_primary_workspace_state(
    value: Value,
) -> Result<StoredPrimaryWorkspaceState, String> {
    serde_json::from_value(value).map_err(|_| persistence_error())
}

#[cfg(not(mobile))]
fn validate_primary_workspace_shape(state: &StoredPrimaryWorkspaceState) -> Result<(), String> {
    if state
        .desktop_workspace_root
        .as_ref()
        .is_some_and(|value| value.trim().is_empty())
        || state
            .desktop_path
            .as_ref()
            .is_some_and(|value| value.trim().is_empty())
    {
        return Err(persistence_error());
    }

    match (
        state.desktop_workspace_root.is_some(),
        state.desktop_path.is_some(),
        state.managed_name.as_deref(),
    ) {
        (false, false, None) | (true, true, None) => Ok(()),
        (false, false, Some(managed_name)) => {
            crate::notebook_scope::validate_notebook_name(managed_name)
                .map(|_| ())
                .map_err(|_| persistence_error())
        }
        _ => Err(persistence_error()),
    }
}

#[cfg(not(mobile))]
fn deserialize_current_primary_workspace_state(
    value: Value,
) -> Result<StoredPrimaryWorkspaceState, String> {
    let state = deserialize_primary_workspace_state(value)?;
    if state.version != LOCAL_STATE_SCHEMA_VERSION {
        return Err(persistence_error());
    }
    validate_primary_workspace_shape(&state)?;
    Ok(state)
}

#[cfg(not(mobile))]
fn serialize_primary_workspace_state(state: &StoredPrimaryWorkspaceState) -> Result<Value, String> {
    serde_json::to_value(state).map_err(|_| persistence_error())
}

#[cfg(not(mobile))]
fn primary_workspace_document_from_values(
    values: &BTreeMap<String, Value>,
) -> Result<CanonicalPrimaryWorkspaceDocument, String> {
    let schema_version = values
        .get(LOCAL_STATE_SCHEMA_VERSION_KEY)
        .and_then(Value::as_u64)
        .filter(|version| *version == LOCAL_STATE_SCHEMA_VERSION)
        .ok_or_else(persistence_error)?;
    let primary_workspace = values
        .get(PRIMARY_WORKSPACE_KEY)
        .cloned()
        .ok_or_else(persistence_error)
        .and_then(deserialize_current_primary_workspace_state)?;
    Ok(CanonicalPrimaryWorkspaceDocument {
        schema_version,
        primary_workspace,
    })
}

#[cfg(not(mobile))]
fn serialize_primary_workspace_document(
    values: &BTreeMap<String, Value>,
) -> Result<Vec<u8>, String> {
    let document = primary_workspace_document_from_values(values)?;
    let bytes = serde_json::to_vec(&document).map_err(|_| persistence_error())?;
    if bytes.len() > MAX_PRIMARY_WORKSPACE_STORE_BYTES {
        return Err(persistence_error());
    }
    Ok(bytes)
}

#[cfg(not(mobile))]
fn deserialize_primary_workspace_document(
    bytes: &[u8],
) -> Result<CanonicalPrimaryWorkspaceDocument, String> {
    let document = serde_json::from_slice::<CanonicalPrimaryWorkspaceDocument>(bytes)
        .map_err(|_| persistence_error())?;
    if document.schema_version != LOCAL_STATE_SCHEMA_VERSION
        || document.primary_workspace.version != LOCAL_STATE_SCHEMA_VERSION
    {
        return Err(persistence_error());
    }
    validate_primary_workspace_shape(&document.primary_workspace)?;
    Ok(document)
}

/// Desktop host decision at process startup.
///
/// `Unselected` is a valid state and must keep the child Kernel dormant until
/// the user selects a workspace. Persisted corruption and storage failures are
/// returned separately so they cannot be mistaken for first-run onboarding.
#[cfg(not(mobile))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum DesktopPrimaryWorkspaceResolution {
    Unselected,
    Selected(PathBuf),
}

#[cfg(not(mobile))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DesktopPrimaryWorkspaceResolutionError {
    Invalid,
    UnsupportedVersion,
    Unavailable,
}

#[cfg(not(mobile))]
impl std::fmt::Display for DesktopPrimaryWorkspaceResolutionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid => formatter.write_str("desktop primary workspace is invalid"),
            Self::UnsupportedVersion => {
                formatter.write_str("desktop primary workspace version is unsupported")
            }
            Self::Unavailable => formatter.write_str("desktop primary workspace is unavailable"),
        }
    }
}

#[cfg(not(mobile))]
impl std::error::Error for DesktopPrimaryWorkspaceResolutionError {}

#[cfg(not(mobile))]
fn validate_selected_desktop_workspace_with_hook<BeforeValidation>(
    workspace_root: &str,
    desktop_path: &str,
    before_validation: BeforeValidation,
) -> Result<PathBuf, DesktopPrimaryWorkspaceResolutionError>
where
    BeforeValidation: FnOnce(),
{
    let invalid = || DesktopPrimaryWorkspaceResolutionError::Invalid;
    let canonical_workspace = crate::workspace_membership::canonical_workspace_root(workspace_root)
        .map_err(|_| invalid())?;
    let canonical_desktop = crate::workspace_membership::canonical_workspace_root(desktop_path)
        .map_err(|_| invalid())?;
    if canonical_desktop.parent() != Some(canonical_workspace.as_path()) {
        return Err(invalid());
    }

    let target_name = canonical_desktop.file_name().ok_or_else(invalid)?;
    let retained_parent =
        crate::storage_capability::open_canonical_directory_nofollow(&canonical_workspace)
            .map_err(|_| invalid())?;
    let parent_identity =
        crate::storage_capability::directory_identity(&retained_parent).map_err(|_| invalid())?;
    let addressed = retained_parent
        .symlink_metadata(target_name)
        .map_err(|_| invalid())?;
    if addressed.file_type().is_symlink() || !addressed.is_dir() {
        return Err(invalid());
    }
    let retained_desktop = retained_parent
        .open_dir_nofollow(target_name)
        .map_err(|_| invalid())?;
    let retained_metadata = retained_desktop.dir_metadata().map_err(|_| invalid())?;
    if addressed.dev() != retained_metadata.dev() || addressed.ino() != retained_metadata.ino() {
        return Err(invalid());
    }
    let desktop_identity =
        crate::storage_capability::directory_identity(&retained_desktop).map_err(|_| invalid())?;

    before_validation();

    let current_parent =
        crate::storage_capability::open_canonical_directory_nofollow(&canonical_workspace)
            .map_err(|_| invalid())?;
    if crate::storage_capability::directory_identity(&current_parent).map_err(|_| invalid())?
        != parent_identity
        || crate::storage_capability::directory_identity(&retained_parent).map_err(|_| invalid())?
            != parent_identity
    {
        return Err(invalid());
    }
    let current_metadata = current_parent
        .symlink_metadata(target_name)
        .map_err(|_| invalid())?;
    if current_metadata.file_type().is_symlink() || !current_metadata.is_dir() {
        return Err(invalid());
    }
    let current_desktop = current_parent
        .open_dir_nofollow(target_name)
        .map_err(|_| invalid())?;
    if crate::storage_capability::directory_identity(&current_desktop).map_err(|_| invalid())?
        != desktop_identity
        || crate::storage_capability::directory_identity(&retained_desktop)
            .map_err(|_| invalid())?
            != desktop_identity
    {
        return Err(invalid());
    }

    let current_workspace = crate::workspace_membership::canonical_workspace_root(workspace_root)
        .map_err(|_| invalid())?;
    let current_desktop = crate::workspace_membership::canonical_workspace_root(desktop_path)
        .map_err(|_| invalid())?;
    if current_workspace != canonical_workspace || current_desktop != canonical_desktop {
        return Err(invalid());
    }
    Ok(canonical_desktop)
}

#[cfg(not(mobile))]
fn resolve_desktop_primary_workspace_value_with_validation_hook<BeforeValidation>(
    value: Option<Value>,
    before_validation: BeforeValidation,
) -> Result<DesktopPrimaryWorkspaceResolution, DesktopPrimaryWorkspaceResolutionError>
where
    BeforeValidation: FnOnce(),
{
    let Some(value) = value else {
        return Ok(DesktopPrimaryWorkspaceResolution::Unselected);
    };
    let version = value
        .as_object()
        .and_then(|object| object.get("version"))
        .and_then(Value::as_u64)
        .ok_or(DesktopPrimaryWorkspaceResolutionError::Invalid)?;
    if version != LOCAL_STATE_SCHEMA_VERSION {
        return Err(DesktopPrimaryWorkspaceResolutionError::UnsupportedVersion);
    }
    let state = deserialize_current_primary_workspace_state(value)
        .map_err(|_| DesktopPrimaryWorkspaceResolutionError::Invalid)?;

    match (
        state.desktop_workspace_root.as_deref(),
        state.desktop_path.as_deref(),
        state.managed_name.as_deref(),
    ) {
        (None, None, None) => Ok(DesktopPrimaryWorkspaceResolution::Unselected),
        (Some(workspace_root), Some(desktop_path), None)
            if !workspace_root.is_empty() && !desktop_path.is_empty() =>
        {
            let selected = validate_selected_desktop_workspace_with_hook(
                workspace_root,
                desktop_path,
                before_validation,
            )?;
            if !state.onboarding_completed || state.onboarding_requested_for_next_launch {
                return Ok(DesktopPrimaryWorkspaceResolution::Unselected);
            }
            Ok(DesktopPrimaryWorkspaceResolution::Selected(selected))
        }
        _ => Err(DesktopPrimaryWorkspaceResolutionError::Invalid),
    }
}

#[cfg(not(mobile))]
fn resolve_desktop_primary_workspace_value(
    value: Option<Value>,
) -> Result<DesktopPrimaryWorkspaceResolution, DesktopPrimaryWorkspaceResolutionError> {
    resolve_desktop_primary_workspace_value_with_validation_hook(value, || {})
}

#[cfg(not(mobile))]
fn resolve_desktop_primary_workspace_read(
    read: Result<(Option<Value>, Option<Value>), String>,
) -> Result<DesktopPrimaryWorkspaceResolution, DesktopPrimaryWorkspaceResolutionError> {
    let (schema_version, value) =
        read.map_err(|_| DesktopPrimaryWorkspaceResolutionError::Unavailable)?;
    if !local_state_schema_is_supported(schema_version.as_ref()) {
        return Err(DesktopPrimaryWorkspaceResolutionError::UnsupportedVersion);
    }
    resolve_desktop_primary_workspace_value(value)
}

#[cfg(not(mobile))]
#[derive(Clone, Copy)]
enum DesktopPrimaryWorkspaceSelectionMode<'a> {
    Initialize,
    RecoverInvalid,
    Switch { expected_root: &'a Path },
}

#[cfg(not(mobile))]
fn select_desktop_primary_workspace_with_backend<Backend: PrimaryWorkspaceBackend + ?Sized>(
    backend: &Backend,
    transaction_lock: &Mutex<()>,
    requested_root: &Path,
    mode: DesktopPrimaryWorkspaceSelectionMode<'_>,
) -> Result<PathBuf, String> {
    let requested_root = requested_root
        .to_str()
        .ok_or_else(desktop_primary_workspace_initialization_error)?;
    let canonical = crate::workspace_membership::canonical_workspace_root(requested_root)
        .map_err(|_| desktop_primary_workspace_initialization_error())?;
    let parent = canonical
        .parent()
        .ok_or_else(desktop_primary_workspace_initialization_error)?
        .to_path_buf();
    let parent_string = parent
        .to_str()
        .ok_or_else(desktop_primary_workspace_initialization_error)?
        .to_owned();
    let canonical_string = canonical
        .to_str()
        .ok_or_else(desktop_primary_workspace_initialization_error)?
        .to_owned();
    let state = serde_json::json!({
        "desktopWorkspaceRoot": parent_string,
        "desktopPath": canonical_string,
        "managedName": null,
        "onboardingCompleted": true,
        "version": 3
    });
    let service = PrimaryWorkspaceService::new(backend, transaction_lock);
    let result = service.with_transaction(|service| {
        let current = service.backend.get(PRIMARY_WORKSPACE_KEY);
        let current_resolution = resolve_desktop_primary_workspace_value(current.clone());
        let selection_is_allowed = match mode {
            DesktopPrimaryWorkspaceSelectionMode::Initialize => {
                current_resolution == Ok(DesktopPrimaryWorkspaceResolution::Unselected)
            }
            DesktopPrimaryWorkspaceSelectionMode::RecoverInvalid => {
                current_resolution == Err(DesktopPrimaryWorkspaceResolutionError::Invalid)
            }
            DesktopPrimaryWorkspaceSelectionMode::Switch { expected_root } => {
                current_resolution
                    == Ok(DesktopPrimaryWorkspaceResolution::Selected(
                        expected_root.to_path_buf(),
                    ))
            }
        };
        if !selection_is_allowed {
            return Err(desktop_primary_workspace_initialization_error());
        }
        service.write_validated_locked(
            PrimaryWorkspaceWriteInput {
                expected_state: Some(current.unwrap_or(Value::Null)),
                state,
            },
            &mut || {
                let validated = validate_selected_desktop_workspace_with_hook(
                    &parent_string,
                    &canonical_string,
                    || {},
                )
                .map_err(|_| desktop_primary_workspace_initialization_error())?;
                if validated != canonical {
                    return Err(desktop_primary_workspace_initialization_error());
                }
                crate::dejavu_sync::path_guard::native_working_tree_registry()
                    .validate_primary_root(Some(&validated))
            },
        )
    })?;
    if !result.applied {
        return Err(desktop_primary_workspace_initialization_error());
    }
    Ok(canonical)
}

#[cfg(not(mobile))]
fn initialize_desktop_primary_workspace_with_backend<Backend: PrimaryWorkspaceBackend + ?Sized>(
    backend: &Backend,
    transaction_lock: &Mutex<()>,
    requested_root: &Path,
) -> Result<PathBuf, String> {
    select_desktop_primary_workspace_with_backend(
        backend,
        transaction_lock,
        requested_root,
        DesktopPrimaryWorkspaceSelectionMode::Initialize,
    )
}

#[cfg(not(mobile))]
fn recover_invalid_desktop_primary_workspace_with_backend<
    Backend: PrimaryWorkspaceBackend + ?Sized,
>(
    backend: &Backend,
    transaction_lock: &Mutex<()>,
    requested_root: &Path,
) -> Result<PathBuf, String> {
    select_desktop_primary_workspace_with_backend(
        backend,
        transaction_lock,
        requested_root,
        DesktopPrimaryWorkspaceSelectionMode::RecoverInvalid,
    )
}

#[cfg(test)]
fn switch_desktop_primary_workspace_with_backend<Backend: PrimaryWorkspaceBackend + ?Sized>(
    backend: &Backend,
    transaction_lock: &Mutex<()>,
    requested_root: &Path,
) -> Result<PathBuf, String> {
    let current = resolve_desktop_primary_workspace_value(backend.get(PRIMARY_WORKSPACE_KEY))
        .map_err(|_| desktop_primary_workspace_initialization_error())?;
    let DesktopPrimaryWorkspaceResolution::Selected(expected_root) = current else {
        return Err(desktop_primary_workspace_initialization_error());
    };
    select_desktop_primary_workspace_with_backend(
        backend,
        transaction_lock,
        requested_root,
        DesktopPrimaryWorkspaceSelectionMode::Switch {
            expected_root: &expected_root,
        },
    )
}

#[cfg(not(mobile))]
pub(crate) struct PreparedDesktopPrimaryWorkspaceSwitch {
    pub(crate) current_root: PathBuf,
    pub(crate) target_root: PathBuf,
}

#[cfg(not(mobile))]
fn preflight_desktop_workspace_switch_root(
    workspace_root: &Path,
    app_data_root: &Path,
    cache_root: &Path,
    home_root: Option<&Path>,
) -> Result<(), String> {
    let canonical_home = home_root.and_then(|root| root.canonicalize().ok());
    if canonical_home.as_deref() == Some(workspace_root) {
        return Err(desktop_primary_workspace_initialization_error());
    }
    qingyu_kernel::paths::KernelPaths::desktop(workspace_root, app_data_root, cache_root)
        .map(|_| ())
        .map_err(|_| desktop_primary_workspace_initialization_error())
}

#[cfg(not(mobile))]
pub(crate) fn prepare_desktop_primary_workspace_switch<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    requested_root: &Path,
) -> Result<PreparedDesktopPrimaryWorkspaceSwitch, String> {
    let current_root = match resolve_desktop_primary_workspace(app)
        .map_err(|_| desktop_primary_workspace_initialization_error())?
    {
        DesktopPrimaryWorkspaceResolution::Selected(root) => root,
        DesktopPrimaryWorkspaceResolution::Unselected => {
            return Err(desktop_primary_workspace_initialization_error())
        }
    };
    let requested_root = requested_root
        .to_str()
        .ok_or_else(desktop_primary_workspace_initialization_error)?;
    let target_root = crate::workspace_membership::canonical_workspace_root(requested_root)
        .map_err(|_| desktop_primary_workspace_initialization_error())?;
    if target_root.parent().is_none() {
        return Err(desktop_primary_workspace_initialization_error());
    }
    let app_data_root = app
        .path()
        .app_data_dir()
        .map_err(|_| desktop_primary_workspace_initialization_error())?;
    let cache_root = app
        .path()
        .app_cache_dir()
        .map_err(|_| desktop_primary_workspace_initialization_error())?;
    std::fs::create_dir_all(&app_data_root)
        .and_then(|()| std::fs::create_dir_all(&cache_root))
        .map_err(|_| desktop_primary_workspace_initialization_error())?;
    preflight_desktop_workspace_switch_root(
        &target_root,
        &app_data_root,
        &cache_root,
        dirs::home_dir().as_deref(),
    )?;
    Ok(PreparedDesktopPrimaryWorkspaceSwitch {
        current_root,
        target_root,
    })
}

#[cfg(not(mobile))]
pub(crate) fn commit_desktop_primary_workspace_switch<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    prepared: &PreparedDesktopPrimaryWorkspaceSwitch,
) -> Result<PathBuf, String> {
    with_primary_workspace_backend(app, |backend| {
        select_desktop_primary_workspace_with_backend(
            backend,
            primary_workspace_transaction_gate(),
            &prepared.target_root,
            DesktopPrimaryWorkspaceSelectionMode::Switch {
                expected_root: &prepared.current_root,
            },
        )
    })
}

#[cfg(not(mobile))]
pub(crate) fn initialize_desktop_primary_workspace<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    requested_root: &Path,
) -> Result<PathBuf, String> {
    with_primary_workspace_backend(app, |backend| {
        initialize_desktop_primary_workspace_with_backend(
            backend,
            primary_workspace_transaction_gate(),
            requested_root,
        )
    })
}

#[cfg(not(mobile))]
pub(crate) fn recover_invalid_desktop_primary_workspace<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    requested_root: &Path,
) -> Result<PathBuf, String> {
    with_primary_workspace_backend(app, |backend| {
        recover_invalid_desktop_primary_workspace_with_backend(
            backend,
            primary_workspace_transaction_gate(),
            requested_root,
        )
    })
}

#[cfg(not(mobile))]
fn desktop_primary_workspace_initialization_error() -> String {
    "desktop primary workspace initialization is unavailable".to_owned()
}

#[cfg(not(mobile))]
fn completed_primary_workspace_state(
    value: Option<Value>,
) -> Result<StoredPrimaryWorkspaceState, String> {
    let value = value.ok_or_else(sync_primary_workspace_unavailable)?;
    let state = deserialize_current_primary_workspace_state(value)
        .map_err(|_| sync_primary_workspace_unavailable())?;
    if !state.onboarding_completed || state.onboarding_requested_for_next_launch {
        return Err(sync_primary_workspace_unavailable());
    }
    Ok(state)
}

#[cfg(not(mobile))]
fn authoritative_primary_workspace_root(
    value: Option<Value>,
    kind: PrimaryWorkspaceKind,
    app_data_root: Option<&Path>,
) -> Result<PathBuf, String> {
    #[cfg(not(any(mobile, test)))]
    let _ = app_data_root;
    let state = completed_primary_workspace_state(value)?;
    match kind {
        PrimaryWorkspaceKind::Desktop => {
            if state.managed_name.is_some() {
                return Err(sync_primary_workspace_unavailable());
            }
            let desktop_path = state
                .desktop_path
                .filter(|path| !path.is_empty())
                .ok_or_else(sync_primary_workspace_unavailable)?;
            let workspace_root = state
                .desktop_workspace_root
                .filter(|path| !path.is_empty())
                .ok_or_else(sync_primary_workspace_unavailable)?;
            let canonical_desktop =
                crate::workspace_membership::canonical_workspace_root(&desktop_path)
                    .map_err(|_| sync_primary_workspace_unavailable())?;
            let canonical_workspace =
                crate::workspace_membership::canonical_workspace_root(&workspace_root)
                    .map_err(|_| sync_primary_workspace_unavailable())?;
            if canonical_desktop.parent() != Some(canonical_workspace.as_path()) {
                return Err(sync_primary_workspace_unavailable());
            }
            Ok(canonical_desktop)
        }
        #[cfg(any(mobile, test))]
        PrimaryWorkspaceKind::Mobile => {
            let app_data_root = app_data_root.ok_or_else(sync_primary_workspace_unavailable)?;
            if state.desktop_path.is_some() || state.desktop_workspace_root.is_some() {
                return Err(sync_primary_workspace_unavailable());
            }
            let managed_name = state
                .managed_name
                .ok_or_else(sync_primary_workspace_unavailable)?;
            crate::managed_workspace::ensure_managed_workspace_path(app_data_root, &managed_name)
                .map_err(|_| sync_primary_workspace_unavailable())
        }
    }
}

#[cfg(test)]
fn validate_primary_workspace_identity(
    value: Option<Value>,
    kind: PrimaryWorkspaceKind,
    app_data_root: Option<&Path>,
    requested_root: &str,
) -> Result<PathBuf, String> {
    let authoritative = authoritative_primary_workspace_root(value, kind, app_data_root)?;
    let requested = crate::workspace_membership::canonical_workspace_root(requested_root)
        .map_err(|_| sync_primary_workspace_mismatch())?;
    if requested != authoritative {
        return Err(sync_primary_workspace_mismatch());
    }
    Ok(authoritative)
}

#[cfg(not(mobile))]
fn read_primary_workspace_value<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<Option<Value>, String> {
    with_primary_workspace_backend(app, |backend| {
        PrimaryWorkspaceService::new(backend, primary_workspace_transaction_gate()).read()
    })
}

/// Resolves the desktop startup state from an independent host-owned disk
/// snapshot while holding the native primary-workspace transaction gate.
#[cfg(not(mobile))]
#[allow(dead_code)]
pub(crate) fn resolve_desktop_primary_workspace<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<DesktopPrimaryWorkspaceResolution, DesktopPrimaryWorkspaceResolutionError> {
    let backend = DesktopDiskPrimaryWorkspaceBackend::open(app)
        .map_err(|_| DesktopPrimaryWorkspaceResolutionError::Unavailable)?;
    let _transaction = primary_workspace_transaction_gate()
        .lock()
        .map_err(|_| DesktopPrimaryWorkspaceResolutionError::Unavailable)?;
    backend
        .reload()
        .map_err(|_| DesktopPrimaryWorkspaceResolutionError::Unavailable)?;
    resolve_desktop_primary_workspace_read(Ok((
        backend.get(LOCAL_STATE_SCHEMA_VERSION_KEY),
        backend.get(PRIMARY_WORKSPACE_KEY),
    )))
}

/// Opens the host-private durable workspace-identity registry using the exact
/// instance-data path and a host-private durable-operation epoch.
#[cfg(not(mobile))]
#[allow(dead_code)]
pub(crate) fn open_native_host_workspace_store(
    paths: &qingyu_kernel::paths::KernelPaths,
    config: &qingyu_kernel::config::KernelConfig,
) -> Result<
    qingyu_kernel::host::native::NativeHostWorkspaceStore,
    DesktopPrimaryWorkspaceResolutionError,
> {
    qingyu_kernel::host::native::NativeHostWorkspaceStore::at_instance_data(
        paths.instance_data_root(),
        config.launch_epoch(),
    )
    .map_err(|_| DesktopPrimaryWorkspaceResolutionError::Unavailable)
}

/// Resolves the selected desktop workspace to its stable host-owned child
/// Kernel identity for the active Desktop runtime owner.
#[cfg(not(mobile))]
#[allow(dead_code)]
pub(crate) fn load_or_create_native_host_workspace_state<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    workspace_root: &Path,
    native_store: &qingyu_kernel::host::native::NativeHostWorkspaceStore,
) -> Result<qingyu_kernel::host::native::NativeHostWorkspaceState, String> {
    let backend = DesktopDiskPrimaryWorkspaceBackend::open(app)?;
    NativeHostWorkspaceStatePersistence::new(
        &backend,
        native_store,
        primary_workspace_transaction_gate(),
    )
    .load_or_create(workspace_root)
}

#[cfg(not(mobile))]
pub(crate) fn with_primary_workspace_transaction<R: tauri::Runtime, T>(
    app: &tauri::AppHandle<R>,
    operation: impl FnOnce(Result<PathBuf, String>) -> Result<T, String>,
) -> Result<T, String> {
    with_primary_workspace_backend(app, |backend| {
        let service = PrimaryWorkspaceService::new(backend, primary_workspace_transaction_gate());

        service.with_current(|value| {
            let authoritative =
                authoritative_primary_workspace_root(value, PrimaryWorkspaceKind::Desktop, None);
            operation(authoritative)
        })
    })
}

pub(crate) fn validate_sync_notes_root<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    requested_root: &str,
) -> Result<PathBuf, String> {
    let authoritative = resolve_sync_primary_workspace(app)?;
    let requested = crate::workspace_membership::canonical_workspace_root(requested_root)
        .map_err(|_| sync_primary_workspace_mismatch())?;
    if requested != authoritative {
        return Err(sync_primary_workspace_mismatch());
    }
    Ok(authoritative)
}

#[cfg(mobile)]
pub(crate) fn validate_bootstrap_notes_root<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    requested_root: &str,
) -> Result<PathBuf, String> {
    use tauri::Manager;

    let requested = crate::workspace_membership::canonical_workspace_root(requested_root)
        .map_err(|_| sync_primary_workspace_mismatch())?;
    let name = crate::notebook_scope::notebook_name_from_root(&requested)?;
    let app_data_root = app
        .path()
        .app_data_dir()
        .map_err(|_| sync_primary_workspace_unavailable())?;
    let managed = crate::managed_workspace::ensure_managed_workspace_path(&app_data_root, &name)
        .map_err(|_| sync_primary_workspace_mismatch())?;
    if requested != managed {
        return Err(sync_primary_workspace_mismatch());
    }
    Ok(requested)
}

pub(crate) fn resolve_sync_primary_workspace<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<PathBuf, String> {
    #[cfg(not(mobile))]
    {
        with_primary_workspace_transaction(app, |authoritative| authoritative)
    }
    #[cfg(mobile)]
    {
        let app_data_root = app
            .path()
            .app_data_dir()
            .map_err(|_| sync_primary_workspace_unavailable())?;
        crate::managed_workspace::ensure_managed_workspace_path(&app_data_root, "primary")
            .map_err(|_| sync_primary_workspace_unavailable())
    }
}

#[cfg(not(mobile))]
fn proposed_primary_workspace_root<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: &Value,
) -> Option<PathBuf> {
    let _ = app;
    authoritative_primary_workspace_root(Some(state.clone()), PrimaryWorkspaceKind::Desktop, None)
        .ok()
}

#[cfg(not(mobile))]
#[tauri::command]
pub(crate) fn read_primary_workspace_state(app: tauri::AppHandle) -> Result<Option<Value>, String> {
    read_primary_workspace_value(&app)
}

#[cfg(not(mobile))]
#[tauri::command]
pub(crate) fn write_primary_workspace_state(
    app: tauri::AppHandle,
    input: PrimaryWorkspaceWriteInput,
) -> Result<PrimaryWorkspaceWriteResult, String> {
    let proposed_root = proposed_primary_workspace_root(&app, &input.state);
    with_primary_workspace_backend(&app, |backend| {
        PrimaryWorkspaceService::new(backend, primary_workspace_transaction_gate())
            .write_with_primary_root_guard(
                input,
                proposed_root.as_deref(),
                crate::dejavu_sync::path_guard::native_working_tree_registry(),
            )
    })
}

#[cfg(not(mobile))]
#[tauri::command]
pub(crate) fn prepare_desktop_notebook_target(
    app: tauri::AppHandle,
    parent_path: String,
    notebook_name: String,
) -> Result<PreparedDesktopNotebookTarget, String> {
    let expected = read_primary_workspace_value(&app)?.unwrap_or(Value::Null);
    prepare_desktop_notebook_target_lease_at_path_with_expected(
        &parent_path,
        &notebook_name,
        Some(expected),
    )
}

#[cfg(not(mobile))]
#[tauri::command]
pub(crate) fn discard_prepared_desktop_notebook_target(lease: String) -> Result<(), String> {
    discard_prepared_desktop_notebook_target_lease(&lease)
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            mpsc, Arc, Mutex,
        },
        time::Duration,
    };

    use serde_json::json;

    use super::*;

    fn semantically_invalid_primary_workspace_states() -> [Value; 5] {
        [
            json!({
                "desktopWorkspaceRoot": "/workspace",
                "desktopPath": null,
                "managedName": null,
                "onboardingCompleted": true,
                "version": 3
            }),
            json!({
                "desktopWorkspaceRoot": null,
                "desktopPath": "/workspace/Notes",
                "managedName": null,
                "onboardingCompleted": true,
                "version": 3
            }),
            json!({
                "desktopWorkspaceRoot": "/workspace",
                "desktopPath": "/workspace/Notes",
                "managedName": "personal",
                "onboardingCompleted": true,
                "version": 3
            }),
            json!({
                "desktopWorkspaceRoot": "   ",
                "desktopPath": "/workspace/Notes",
                "managedName": null,
                "onboardingCompleted": true,
                "version": 3
            }),
            json!({
                "desktopWorkspaceRoot": null,
                "desktopPath": null,
                "managedName": ".markra-sync",
                "onboardingCompleted": true,
                "version": 3
            }),
        ]
    }

    fn assert_no_primary_workspace_temporary_files(root: &Path) {
        assert!(std::fs::read_dir(root)
            .expect("read app data")
            .all(|entry| !entry
                .expect("app data entry")
                .file_name()
                .to_string_lossy()
                .starts_with(".primary-workspace-")));
    }

    struct PublishedUnknownDiskBackend {
        inner: DesktopDiskPrimaryWorkspaceBackend,
    }

    impl PrimaryWorkspaceBackend for PublishedUnknownDiskBackend {
        fn delete(&self, key: &str) {
            self.inner.delete(key);
        }

        fn get(&self, key: &str) -> Option<Value> {
            self.inner.get(key)
        }

        fn reload(&self) -> Result<(), String> {
            self.inner.reload()
        }

        fn save(&self) -> Result<(), String> {
            Err(persistence_error())
        }

        fn save_publication(&self) -> PrimaryWorkspacePersistence {
            self.inner.save_publication_with_sync(|_| {
                Err(io::Error::other("injected directory fsync failure"))
            })
        }

        fn set(&self, key: &str, value: Value) {
            self.inner.set(key, value);
        }
    }

    #[test]
    fn desktop_primary_workspace_atomic_write_preserves_old_file_on_prewrite_failure() {
        let app_data = tempfile::tempdir().expect("temporary app data");
        let root = app_data.path().canonicalize().expect("canonical app data");
        let target = root.join(DESKTOP_PRIMARY_WORKSPACE_STORE_PATH);
        std::fs::write(&target, br#"{"primaryWorkspace":"old"}"#).expect("seed state");

        let publication = replace_primary_workspace_file_atomically_with_hooks(
            &root,
            br#"{"primaryWorkspace":"new"}"#,
            None,
            || Err(io::Error::other("injected prewrite failure")),
            || Ok(()),
            sync_directory,
        );

        assert_eq!(publication, PrimaryWorkspacePersistence::NotPublished);
        assert_eq!(
            std::fs::read(target).expect("old state remains"),
            br#"{"primaryWorkspace":"old"}"#,
        );
    }

    #[test]
    fn desktop_primary_workspace_atomic_write_preserves_old_file_on_postwrite_failure() {
        let app_data = tempfile::tempdir().expect("temporary app data");
        let root = app_data.path().canonicalize().expect("canonical app data");
        let target = root.join(DESKTOP_PRIMARY_WORKSPACE_STORE_PATH);
        std::fs::write(&target, br#"{"primaryWorkspace":"old"}"#).expect("seed state");

        let publication = replace_primary_workspace_file_atomically_with_hooks(
            &root,
            br#"{"primaryWorkspace":"new"}"#,
            None,
            || Ok(()),
            || Err(io::Error::other("injected postwrite failure")),
            sync_directory,
        );

        assert_eq!(publication, PrimaryWorkspacePersistence::NotPublished);
        assert_eq!(
            std::fs::read(target).expect("old state remains"),
            br#"{"primaryWorkspace":"old"}"#,
        );
    }

    #[test]
    fn desktop_primary_workspace_atomic_write_rejects_target_swap_before_publish() {
        let app_data = tempfile::tempdir().expect("temporary app data");
        let root = app_data.path().canonicalize().expect("canonical app data");
        let target = root.join(DESKTOP_PRIMARY_WORKSPACE_STORE_PATH);
        std::fs::write(&target, br#"{"primaryWorkspace":"old"}"#).expect("seed state");

        let publication = replace_primary_workspace_file_atomically_with_hooks(
            &root,
            br#"{"primaryWorkspace":"new"}"#,
            None,
            || Ok(()),
            || {
                std::fs::rename(&target, root.join("captured-local-state"))?;
                std::fs::write(&target, br#"{"primaryWorkspace":"replacement"}"#)
            },
            sync_directory,
        );

        assert_eq!(publication, PrimaryWorkspacePersistence::NotPublished);
        assert_eq!(
            std::fs::read(target).expect("replacement remains"),
            br#"{"primaryWorkspace":"replacement"}"#,
        );
        assert!(std::fs::read_dir(&root)
            .expect("read app data")
            .all(|entry| !entry
                .expect("directory entry")
                .file_name()
                .to_string_lossy()
                .starts_with(".primary-workspace-")));
    }

    #[test]
    fn desktop_primary_workspace_disk_reread_resolves_published_unknown_commit() {
        let app_data = tempfile::tempdir().expect("temporary app data");
        let root = app_data.path().canonicalize().expect("canonical app data");
        let desired = br#"{"schemaVersion":3,"primaryWorkspace":{"desktopWorkspaceRoot":null,"desktopPath":null,"managedName":null,"onboardingCompleted":false,"version":3}}"#;

        let publication = replace_primary_workspace_file_atomically_with_hooks(
            &root,
            desired,
            None,
            || Ok(()),
            || Ok(()),
            |_| Err(io::Error::other("injected directory fsync failure")),
        );
        let reread = read_workspace_store_file(&root, DESKTOP_PRIMARY_WORKSPACE_STORE_PATH)
            .expect("independent disk reread")
            .expect("published state");

        assert_eq!(
            publication,
            PrimaryWorkspacePersistence::PublishedWithoutDirectoryDurability,
        );
        assert_eq!(
            reread.values.get(PRIMARY_WORKSPACE_KEY),
            Some(&serde_json::json!({
                "desktopWorkspaceRoot": null,
                "desktopPath": null,
                "managedName": null,
                "onboardingCompleted": false,
                "version": 3
            })),
        );
    }

    #[test]
    fn desktop_primary_workspace_disk_reader_rejects_missing_outer_schema_version() {
        let app_data = tempfile::tempdir().expect("temporary app data");
        let root = app_data.path().canonicalize().expect("canonical app data");
        std::fs::write(
            root.join(DESKTOP_PRIMARY_WORKSPACE_STORE_PATH),
            br#"{"primaryWorkspace":null}"#,
        )
        .expect("seed authority without schema version");

        assert!(DesktopDiskPrimaryWorkspaceBackend::open_at(root).is_err());
    }

    #[test]
    fn desktop_primary_workspace_disk_reader_rejects_old_outer_schema_versions() {
        for version in [0, 1, 2] {
            let app_data = tempfile::tempdir().expect("temporary app data");
            let root = app_data.path().canonicalize().expect("canonical app data");
            std::fs::write(
                root.join(DESKTOP_PRIMARY_WORKSPACE_STORE_PATH),
                serde_json::to_vec(&json!({
                    "schemaVersion": version,
                    "primaryWorkspace": null
                }))
                .expect("old authority JSON"),
            )
            .expect("seed old authority");

            assert!(
                DesktopDiskPrimaryWorkspaceBackend::open_at(root).is_err(),
                "outer schema version {version} must fail closed"
            );
        }
    }

    #[test]
    fn desktop_primary_workspace_disk_reader_rejects_unknown_outer_fields() {
        let app_data = tempfile::tempdir().expect("temporary app data");
        let root = app_data.path().canonicalize().expect("canonical app data");
        std::fs::write(
            root.join(DESKTOP_PRIMARY_WORKSPACE_STORE_PATH),
            br#"{"schemaVersion":3,"primaryWorkspace":null,"futureAuthority":true}"#,
        )
        .expect("seed authority with unknown field");

        assert!(DesktopDiskPrimaryWorkspaceBackend::open_at(root).is_err());
    }

    #[test]
    fn desktop_primary_workspace_disk_reader_rejects_unknown_inner_fields() {
        let app_data = tempfile::tempdir().expect("temporary app data");
        let root = app_data.path().canonicalize().expect("canonical app data");
        std::fs::write(
            root.join(DESKTOP_PRIMARY_WORKSPACE_STORE_PATH),
            br#"{"schemaVersion":3,"primaryWorkspace":{"desktopWorkspaceRoot":null,"desktopPath":null,"managedName":null,"onboardingCompleted":false,"version":3,"futureAuthority":true}}"#,
        )
        .expect("seed authority with unknown inner field");

        assert!(DesktopDiskPrimaryWorkspaceBackend::open_at(root).is_err());
    }

    #[test]
    fn desktop_primary_workspace_disk_reader_rejects_semantic_inner_shape_without_mutation() {
        for state in semantically_invalid_primary_workspace_states() {
            let app_data = tempfile::tempdir().expect("temporary app data");
            let root = app_data.path().canonicalize().expect("canonical app data");
            let target = root.join(DESKTOP_PRIMARY_WORKSPACE_STORE_PATH);
            let bytes = serde_json::to_vec(&json!({
                "schemaVersion": LOCAL_STATE_SCHEMA_VERSION,
                "primaryWorkspace": state
            }))
            .expect("invalid semantic authority JSON");
            std::fs::write(&target, &bytes).expect("seed invalid semantic authority");

            assert!(DesktopDiskPrimaryWorkspaceBackend::open_at(root).is_err());
            assert_eq!(
                std::fs::read(target).expect("invalid authority remains unchanged"),
                bytes
            );
        }
    }

    #[test]
    fn post_validation_failure_restores_an_absent_disk_authority() {
        let app_data = tempfile::tempdir().expect("temporary app data");
        let root = app_data.path().canonicalize().expect("canonical app data");
        let target = root.join(DESKTOP_PRIMARY_WORKSPACE_STORE_PATH);
        let backend = DesktopDiskPrimaryWorkspaceBackend::open_at(root.clone())
            .expect("open absent authority");
        let transaction = Mutex::new(());
        let validations = AtomicUsize::new(0);

        let error = PrimaryWorkspaceService::new(&backend, &transaction)
            .write_validated(write_input("/workspace/Notes"), || {
                if validations.fetch_add(1, Ordering::Relaxed) == 0 {
                    Ok(())
                } else {
                    Err("injected-post-validation-failure".to_owned())
                }
            })
            .expect_err("post-validation failure must roll back publication");

        assert_eq!(error, "injected-post-validation-failure");
        assert!(!target.exists());
        assert_eq!(backend.get(LOCAL_STATE_SCHEMA_VERSION_KEY), None);
        assert_eq!(backend.get(PRIMARY_WORKSPACE_KEY), None);
        assert_no_primary_workspace_temporary_files(&root);
    }

    #[test]
    fn post_validation_failure_restores_the_original_disk_authority() {
        let app_data = tempfile::tempdir().expect("temporary app data");
        let root = app_data.path().canonicalize().expect("canonical app data");
        let target = root.join(DESKTOP_PRIMARY_WORKSPACE_STORE_PATH);
        let backend = DesktopDiskPrimaryWorkspaceBackend::open_at(root.clone())
            .expect("open absent authority");
        let transaction = Mutex::new(());
        let original = PrimaryWorkspaceService::new(&backend, &transaction)
            .write(write_input("/workspace/A"))
            .expect("publish original authority")
            .state;
        let original_bytes = std::fs::read(&target).expect("read original authority");
        let validations = AtomicUsize::new(0);

        let error = PrimaryWorkspaceService::new(&backend, &transaction)
            .write_validated(write_input("/workspace/B"), || {
                if validations.fetch_add(1, Ordering::Relaxed) == 0 {
                    Ok(())
                } else {
                    Err("injected-post-validation-failure".to_owned())
                }
            })
            .expect_err("post-validation failure must restore original authority");

        assert_eq!(error, "injected-post-validation-failure");
        assert_eq!(backend.get(PRIMARY_WORKSPACE_KEY), Some(original));
        assert_eq!(
            std::fs::read(target).expect("read restored authority"),
            original_bytes
        );
        assert_no_primary_workspace_temporary_files(&root);
    }

    #[test]
    fn desktop_switch_commit_unknown_publishes_b_for_independent_settlement() {
        let app_data = tempfile::tempdir().expect("temporary app data");
        let root = app_data.path().canonicalize().expect("canonical app data");
        let workspace = root.join("workspaces");
        let first = workspace.join("A");
        let second = workspace.join("B");
        std::fs::create_dir_all(&first).expect("workspace A");
        std::fs::create_dir_all(&second).expect("workspace B");
        let first = first.canonicalize().expect("canonical A");
        let second = second.canonicalize().expect("canonical B");
        let transaction = Mutex::new(());
        let initial = DesktopDiskPrimaryWorkspaceBackend::open_at(root.clone())
            .expect("open initial authority");
        assert_eq!(
            initialize_desktop_primary_workspace_with_backend(&initial, &transaction, &first)
                .expect("initialize A"),
            first,
        );
        let faulting = PublishedUnknownDiskBackend {
            inner: DesktopDiskPrimaryWorkspaceBackend::open_at(root.clone())
                .expect("open A for switch"),
        };

        assert!(select_desktop_primary_workspace_with_backend(
            &faulting,
            &transaction,
            &second,
            DesktopPrimaryWorkspaceSelectionMode::Switch {
                expected_root: &first,
            },
        )
        .is_err());
        let independent = DesktopDiskPrimaryWorkspaceBackend::open_at(root)
            .expect("independent post-error disk snapshot");
        assert_eq!(
            resolve_desktop_primary_workspace_read(Ok((
                independent.get(LOCAL_STATE_SCHEMA_VERSION_KEY),
                independent.get(PRIMARY_WORKSPACE_KEY),
            ))),
            Ok(DesktopPrimaryWorkspaceResolution::Selected(second)),
            "the rename published B even though directory durability was commit-unknown"
        );
    }

    #[test]
    fn desktop_primary_workspace_ignores_local_state_without_a_canonical_authority_file() {
        let app_data = tempfile::tempdir().expect("temporary app data");
        let root = app_data.path().canonicalize().expect("canonical app data");
        let legacy_path = root.join("local-state.json");
        std::fs::write(&legacy_path, br#"{"primaryWorkspace":{"version":3}}"#)
            .expect("seed forbidden local state");

        let backend = DesktopDiskPrimaryWorkspaceBackend::open_at(root.clone())
            .expect("open canonical authority");

        assert_eq!(backend.get(PRIMARY_WORKSPACE_KEY), None);
        assert!(!root.join(DESKTOP_PRIMARY_WORKSPACE_STORE_PATH).exists());
        assert_eq!(
            std::fs::read(&legacy_path).expect("local state remains untouched"),
            br#"{"primaryWorkspace":{"version":3}}"#,
        );
    }

    #[test]
    fn desktop_disk_reader_reloads_authority_after_waiting_for_the_transaction_gate() {
        let app_data = tempfile::tempdir().expect("temporary app data");
        let root = app_data.path().canonicalize().expect("canonical app data");
        let workspace = root.join("workspaces");
        let first = workspace.join("A");
        let second = workspace.join("B");
        std::fs::create_dir_all(&first).expect("workspace A");
        std::fs::create_dir_all(&second).expect("workspace B");
        let first = first.canonicalize().expect("canonical A");
        let second = second.canonicalize().expect("canonical B");
        let values = |selected: &Path| {
            BTreeMap::from([
                (
                    LOCAL_STATE_SCHEMA_VERSION_KEY.to_owned(),
                    Value::from(LOCAL_STATE_SCHEMA_VERSION),
                ),
                (
                    PRIMARY_WORKSPACE_KEY.to_owned(),
                    completed_v3_desktop_state(
                        selected.parent().expect("workspace parent"),
                        selected,
                    ),
                ),
            ])
        };
        let publish = |selected: &Path, expected| {
            replace_primary_workspace_file_atomically_with_hooks(
                &root,
                &serde_json::to_vec(&values(selected)).expect("workspace authority JSON"),
                expected,
                || Ok(()),
                || Ok(()),
                sync_directory,
            )
        };
        assert_eq!(publish(&first, None), PrimaryWorkspacePersistence::Durable);

        let transaction = Arc::new(Mutex::new(()));
        let held = transaction.lock().expect("hold transaction gate");
        let (opened_sender, opened_receiver) = mpsc::channel();
        let root_for_reader = root.clone();
        let transaction_for_reader = Arc::clone(&transaction);

        let read = std::thread::spawn(move || {
            let backend = DesktopDiskPrimaryWorkspaceBackend::open_at(root_for_reader)
                .expect("open disk backend before gate acquisition");
            opened_sender.send(()).expect("backend opened");
            PrimaryWorkspaceService::new(&backend, transaction_for_reader.as_ref()).read()
        });
        opened_receiver
            .recv()
            .expect("reader captured its pre-gate state");

        let existing = read_workspace_store_file(&root, DESKTOP_PRIMARY_WORKSPACE_STORE_PATH)
            .expect("read A authority")
            .expect("A authority");
        assert_eq!(
            publish(&second, Some(Some(existing.identity))),
            PrimaryWorkspacePersistence::Durable,
        );
        drop(held);

        let state = read
            .join()
            .expect("join disk reader")
            .expect("read latest gated authority");
        assert_eq!(
            resolve_desktop_primary_workspace_value(state),
            Ok(DesktopPrimaryWorkspaceResolution::Selected(second)),
            "the reader must reload B after it acquires the gate instead of returning cached A"
        );
    }

    #[test]
    fn desktop_workspace_switch_preflight_rejects_home_and_runtime_root_overlap() {
        let roots = tempfile::tempdir().expect("temporary roots");
        let home = roots.path().join("home");
        let workspace = roots.path().join("workspace");
        let app_data = roots.path().join("app-data");
        let cache = roots.path().join("cache");
        for root in [&home, &workspace, &app_data, &cache] {
            std::fs::create_dir(root).expect("create root");
        }
        let home = home.canonicalize().expect("canonical home");
        let workspace = workspace.canonicalize().expect("canonical workspace");
        let app_data = app_data.canonicalize().expect("canonical app data");
        let cache = cache.canonicalize().expect("canonical cache");

        assert!(preflight_desktop_workspace_switch_root(
            &workspace,
            &app_data,
            &cache,
            Some(&home),
        )
        .is_ok());
        assert!(
            preflight_desktop_workspace_switch_root(&home, &app_data, &cache, Some(&home),)
                .is_err()
        );

        let overlapping_app_data = workspace.join("app-data");
        std::fs::create_dir(&overlapping_app_data).expect("overlapping app data");
        assert!(preflight_desktop_workspace_switch_root(
            &workspace,
            &overlapping_app_data,
            &cache,
            Some(&home),
        )
        .is_err());
    }

    fn prepared_target_count() -> usize {
        super::prepared_desktop_notebook_targets()
            .lock()
            .unwrap()
            .len()
    }

    fn prepared_target_test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[derive(Default)]
    struct MemoryBackend {
        fail_save: AtomicBool,
        saves: AtomicUsize,
        values: Mutex<BTreeMap<String, Value>>,
    }

    impl MemoryBackend {
        fn with(values: impl IntoIterator<Item = (&'static str, Value)>) -> Self {
            Self {
                fail_save: AtomicBool::new(false),
                saves: AtomicUsize::new(0),
                values: Mutex::new(
                    values
                        .into_iter()
                        .map(|(key, value)| (key.to_string(), value))
                        .collect(),
                ),
            }
        }

        fn value(&self, key: &str) -> Option<Value> {
            self.values.lock().expect("memory values").get(key).cloned()
        }
    }

    impl PrimaryWorkspaceBackend for MemoryBackend {
        fn delete(&self, key: &str) {
            self.values.lock().expect("memory values").remove(key);
        }

        fn get(&self, key: &str) -> Option<Value> {
            self.value(key)
        }

        fn save(&self) -> Result<(), String> {
            self.saves.fetch_add(1, Ordering::Relaxed);
            if self.fail_save.load(Ordering::Relaxed) {
                Err(persistence_error())
            } else {
                Ok(())
            }
        }

        fn set(&self, key: &str, value: Value) {
            self.values
                .lock()
                .expect("memory values")
                .insert(key.to_string(), value);
        }
    }

    struct AtomicDesktopHostRecord {
        value: Arc<Mutex<Value>>,
        binding: qingyu_kernel::workspace::primary::PrimaryWorkspaceRepositoryBinding,
    }

    impl AtomicDesktopHostRecord {
        fn raw(&self) -> Value {
            self.value.lock().expect("desktop host record").clone()
        }
    }

    impl qingyu_kernel::workspace::primary::PrimaryWorkspaceStore for AtomicDesktopHostRecord {
        fn repository_binding(
            &self,
        ) -> qingyu_kernel::workspace::primary::PrimaryWorkspaceRepositoryBinding {
            self.binding.clone()
        }

        fn load(
            &self,
        ) -> Result<Option<Value>, qingyu_kernel::workspace::primary::PrimaryWorkspaceStoreError>
        {
            Ok(self
                .value
                .lock()
                .expect("desktop host record")
                .get("kernelWorkspace")
                .cloned())
        }

        fn replace(
            &self,
            value: Option<Value>,
        ) -> Result<(), qingyu_kernel::workspace::primary::PrimaryWorkspaceStoreError> {
            let mut record = self.value.lock().expect("desktop host record");
            let object = record.as_object_mut().ok_or_else(
                qingyu_kernel::workspace::primary::PrimaryWorkspaceStoreError::unavailable,
            )?;
            if let Some(value) = value {
                object.insert("kernelWorkspace".to_string(), value);
            } else {
                object.remove("kernelWorkspace");
            }
            Ok(())
        }

        fn save(
            &self,
        ) -> Result<(), qingyu_kernel::workspace::primary::PrimaryWorkspaceStoreError> {
            Ok(())
        }
    }

    struct AtomicDesktopHostTransaction {
        record: Arc<Mutex<Value>>,
        binding: qingyu_kernel::workspace::primary::PrimaryWorkspaceRepositoryBinding,
        authority_binding: qingyu_kernel::workspace::primary::PreparedWorkspaceAuthorityBinding,
        expected_record: Value,
        target: PathBuf,
    }

    impl qingyu_kernel::workspace::primary::AtomicHostWorkspaceTransaction
        for AtomicDesktopHostTransaction
    {
        fn repository_binding(
            &self,
        ) -> qingyu_kernel::workspace::primary::PrimaryWorkspaceRepositoryBinding {
            self.binding.clone()
        }

        fn authority_binding(
            &self,
        ) -> qingyu_kernel::workspace::primary::PreparedWorkspaceAuthorityBinding {
            self.authority_binding.clone()
        }

        fn compare_and_commit(
            self: Box<Self>,
            expected_kernel_value: Option<&Value>,
            next_kernel_value: Value,
        ) -> Result<(), qingyu_kernel::workspace::primary::AtomicHostWorkspaceCommitError> {
            let mut record = self.record.lock().expect("desktop host record");
            if *record != self.expected_record
                || record.get("kernelWorkspace") != expected_kernel_value
            {
                return Err(
                    qingyu_kernel::workspace::primary::AtomicHostWorkspaceCommitError::conflict(),
                );
            }
            let object = record.as_object_mut().ok_or_else(
                qingyu_kernel::workspace::primary::AtomicHostWorkspaceCommitError::no_commit,
            )?;
            object.insert(
                "desktopPath".to_string(),
                Value::String(self.target.to_string_lossy().into_owned()),
            );
            object.insert("kernelWorkspace".to_string(), next_kernel_value);
            Ok(())
        }
    }

    impl TrustedDesktopWorkspacePersistence for AtomicDesktopHostRecord {
        fn prepare_host_workspace_transaction(
            &self,
            absolute_path: &Path,
            authority_binding: qingyu_kernel::workspace::primary::PreparedWorkspaceAuthorityBinding,
        ) -> Result<
            Box<dyn qingyu_kernel::workspace::primary::AtomicHostWorkspaceTransaction>,
            qingyu_kernel::services::workspace::WorkspaceServiceError,
        > {
            let expected_record = self.value.lock().expect("desktop host record").clone();
            Ok(Box::new(AtomicDesktopHostTransaction {
                record: self.value.clone(),
                binding: self.binding.clone(),
                authority_binding,
                expected_record,
                target: absolute_path.to_path_buf(),
            }))
        }
    }

    struct BlockingBackend {
        inner: MemoryBackend,
        release_first_save: Mutex<Option<mpsc::Receiver<()>>>,
        started_first_save: Mutex<Option<mpsc::Sender<()>>>,
    }

    impl BlockingBackend {
        fn new() -> (Self, mpsc::Receiver<()>, mpsc::Sender<()>) {
            let (started_sender, started_receiver) = mpsc::channel();
            let (release_sender, release_receiver) = mpsc::channel();
            (
                Self {
                    inner: MemoryBackend::default(),
                    release_first_save: Mutex::new(Some(release_receiver)),
                    started_first_save: Mutex::new(Some(started_sender)),
                },
                started_receiver,
                release_sender,
            )
        }
    }

    impl PrimaryWorkspaceBackend for BlockingBackend {
        fn delete(&self, key: &str) {
            self.inner.delete(key);
        }

        fn get(&self, key: &str) -> Option<Value> {
            self.inner.get(key)
        }

        fn save(&self) -> Result<(), String> {
            if let Some(started) = self
                .started_first_save
                .lock()
                .expect("started sender")
                .take()
            {
                started.send(()).expect("announce first save");
                self.release_first_save
                    .lock()
                    .expect("release receiver")
                    .take()
                    .expect("first save receiver")
                    .recv()
                    .expect("release first save");
            }
            self.inner.save()
        }

        fn set(&self, key: &str, value: Value) {
            self.inner.set(key, value);
        }
    }

    fn write_input(path: &str) -> PrimaryWorkspaceWriteInput {
        let workspace_root = Path::new(path).parent().and_then(Path::to_str);
        PrimaryWorkspaceWriteInput {
            expected_state: None,
            state: json!({
                "desktopWorkspaceRoot": workspace_root,
                "desktopPath": path,
                "managedName": null,
                "onboardingCompleted": true,
                "version": 3
            }),
        }
    }

    fn completed_state(desktop_path: Option<&str>) -> Value {
        let workspace_root = desktop_path
            .and_then(|path| Path::new(path).parent())
            .and_then(Path::to_str);
        json!({
            "desktopWorkspaceRoot": workspace_root,
            "desktopPath": desktop_path,
            "managedName": null,
            "onboardingCompleted": true,
            "version": 3
        })
    }

    fn completed_mobile_state(managed_name: &str) -> Value {
        json!({
            "desktopWorkspaceRoot": null,
            "desktopPath": null,
            "managedName": managed_name,
            "onboardingCompleted": true,
            "version": 3
        })
    }

    fn completed_v3_desktop_state(workspace_root: &Path, desktop_path: &Path) -> Value {
        json!({
            "desktopWorkspaceRoot": workspace_root,
            "desktopPath": desktop_path,
            "managedName": null,
            "onboardingCompleted": true,
            "version": 3
        })
    }

    #[test]
    fn desktop_host_resolution_distinguishes_unselected_from_selected() {
        let temporary = tempfile::tempdir().expect("desktop host workspace roots");
        let workspace_root = temporary.path().join("Workspace");
        let notebook = workspace_root.join("Notes");
        std::fs::create_dir_all(&notebook).expect("selected notebook");

        assert_eq!(
            resolve_desktop_primary_workspace_value(None),
            Ok(DesktopPrimaryWorkspaceResolution::Unselected)
        );
        assert_eq!(
            resolve_desktop_primary_workspace_value(Some(json!({
                "desktopWorkspaceRoot": null,
                "desktopPath": null,
                "managedName": null,
                "onboardingCompleted": false,
                "version": 3
            }))),
            Ok(DesktopPrimaryWorkspaceResolution::Unselected)
        );
        assert_eq!(
            resolve_desktop_primary_workspace_value(Some(completed_v3_desktop_state(
                &workspace_root,
                &notebook,
            ))),
            Ok(DesktopPrimaryWorkspaceResolution::Selected(
                notebook.canonicalize().expect("canonical notebook")
            ))
        );
    }

    #[test]
    fn desktop_host_resolution_rejects_malformed_current_version_records() {
        let invalid_records = [
            Value::String("not-a-workspace-record".to_string()),
            json!({
                "version": 3
            }),
            json!({
                "desktopWorkspaceRoot": "/tmp/Workspace",
                "desktopPath": null,
                "managedName": null,
                "onboardingCompleted": true,
                "version": 3
            }),
        ];

        for record in invalid_records {
            assert_eq!(
                resolve_desktop_primary_workspace_value(Some(record)),
                Err(DesktopPrimaryWorkspaceResolutionError::Invalid)
            );
        }
    }

    #[test]
    fn desktop_host_resolution_preserves_unsupported_workspace_versions() {
        for version in [1, 2, 4, u64::MAX] {
            let record = json!({
                "desktopWorkspaceRoot": null,
                "desktopPath": null,
                "managedName": null,
                "onboardingCompleted": false,
                "version": version
            });
            assert_eq!(
                resolve_desktop_primary_workspace_value(Some(record)),
                Err(DesktopPrimaryWorkspaceResolutionError::UnsupportedVersion)
            );
        }
    }

    #[test]
    fn invalid_recovery_never_overwrites_an_unsupported_workspace_version() {
        let temporary = tempfile::tempdir().unwrap();
        let selected = temporary.path().join("Workspace").join("Recovered");
        std::fs::create_dir_all(&selected).unwrap();
        let future_record = json!({
            "desktopWorkspaceRoot": "/future/workspace",
            "desktopPath": "/future/workspace/Notes",
            "managedName": null,
            "onboardingCompleted": true,
            "futureField": { "mustRemain": true },
            "version": 4
        });
        let backend = MemoryBackend::with([(PRIMARY_WORKSPACE_KEY, future_record.clone())]);
        let lock = Mutex::new(());

        assert!(
            recover_invalid_desktop_primary_workspace_with_backend(&backend, &lock, &selected)
                .is_err()
        );
        assert_eq!(backend.value(PRIMARY_WORKSPACE_KEY), Some(future_record));
    }

    #[test]
    fn desktop_host_resolution_rejects_a_future_outer_local_state_schema() {
        assert_eq!(
            resolve_desktop_primary_workspace_read(Ok((
                Some(Value::from(LOCAL_STATE_SCHEMA_VERSION + 1)),
                None,
            ))),
            Err(DesktopPrimaryWorkspaceResolutionError::UnsupportedVersion)
        );
        assert_eq!(
            resolve_desktop_primary_workspace_read(Ok((
                Some(Value::String("future".to_owned())),
                None,
            ))),
            Err(DesktopPrimaryWorkspaceResolutionError::UnsupportedVersion)
        );
    }

    #[test]
    fn desktop_initialization_never_downgrades_a_future_outer_local_state_schema() {
        let temporary = tempfile::tempdir().unwrap();
        let selected = temporary.path().join("Workspace").join("Recovered");
        std::fs::create_dir_all(&selected).unwrap();
        let future_schema = Value::from(LOCAL_STATE_SCHEMA_VERSION + 1);
        let future_record = json!({
            "futureField": { "mustRemain": true },
            "version": 4
        });
        let backend = MemoryBackend::with([
            (LOCAL_STATE_SCHEMA_VERSION_KEY, future_schema.clone()),
            (PRIMARY_WORKSPACE_KEY, future_record.clone()),
        ]);
        let lock = Mutex::new(());

        assert!(
            initialize_desktop_primary_workspace_with_backend(&backend, &lock, &selected).is_err()
        );
        assert_eq!(
            backend.value(LOCAL_STATE_SCHEMA_VERSION_KEY),
            Some(future_schema)
        );
        assert_eq!(backend.value(PRIMARY_WORKSPACE_KEY), Some(future_record));
    }

    #[test]
    fn primary_workspace_writes_fail_closed_on_a_future_outer_schema() {
        let future_schema = Value::from(LOCAL_STATE_SCHEMA_VERSION + 1);
        let current = json!({
            "desktopWorkspaceRoot": null,
            "desktopPath": null,
            "managedName": null,
            "onboardingCompleted": false,
            "version": 3
        });
        let backend = MemoryBackend::with([
            (LOCAL_STATE_SCHEMA_VERSION_KEY, future_schema.clone()),
            (PRIMARY_WORKSPACE_KEY, current.clone()),
        ]);
        let lock = Mutex::new(());

        assert!(PrimaryWorkspaceService::new(&backend, &lock)
            .write(PrimaryWorkspaceWriteInput {
                expected_state: Some(current.clone()),
                state: json!({
                    "desktopWorkspaceRoot": "/replacement",
                    "desktopPath": "/replacement/Notes",
                    "managedName": null,
                    "onboardingCompleted": true,
                    "version": 3
                }),
            })
            .is_err());
        assert_eq!(
            backend.value(LOCAL_STATE_SCHEMA_VERSION_KEY),
            Some(future_schema)
        );
        assert_eq!(backend.value(PRIMARY_WORKSPACE_KEY), Some(current));
    }

    #[test]
    fn primary_workspace_writes_reject_unknown_inner_fields_without_persisting() {
        let backend = MemoryBackend::default();
        let lock = Mutex::new(());

        assert!(PrimaryWorkspaceService::new(&backend, &lock)
            .write(PrimaryWorkspaceWriteInput {
                expected_state: None,
                state: json!({
                    "desktopWorkspaceRoot": null,
                    "desktopPath": null,
                    "managedName": null,
                    "onboardingCompleted": false,
                    "version": 3,
                    "futureAuthority": true
                }),
            })
            .is_err());
        assert_eq!(backend.value(LOCAL_STATE_SCHEMA_VERSION_KEY), None);
        assert_eq!(backend.value(PRIMARY_WORKSPACE_KEY), None);
    }

    #[test]
    fn primary_workspace_writes_reject_semantic_inner_shape_without_persisting() {
        let current = json!({
            "desktopWorkspaceRoot": null,
            "desktopPath": null,
            "managedName": null,
            "onboardingCompleted": false,
            "version": 3
        });

        for state in semantically_invalid_primary_workspace_states() {
            let backend = MemoryBackend::with([
                (
                    LOCAL_STATE_SCHEMA_VERSION_KEY,
                    Value::from(LOCAL_STATE_SCHEMA_VERSION),
                ),
                (PRIMARY_WORKSPACE_KEY, current.clone()),
            ]);
            let lock = Mutex::new(());

            assert!(PrimaryWorkspaceService::new(&backend, &lock)
                .write(PrimaryWorkspaceWriteInput {
                    expected_state: Some(current.clone()),
                    state,
                })
                .is_err());
            assert_eq!(
                backend.value(LOCAL_STATE_SCHEMA_VERSION_KEY),
                Some(Value::from(LOCAL_STATE_SCHEMA_VERSION))
            );
            assert_eq!(backend.value(PRIMARY_WORKSPACE_KEY), Some(current.clone()));
            assert_eq!(backend.saves.load(Ordering::Relaxed), 0);
        }
    }

    #[test]
    fn primary_workspace_writes_reject_malformed_inner_payloads_without_persisting() {
        let malformed = [
            Value::String("not-a-workspace-record".to_owned()),
            json!({
                "desktopWorkspaceRoot": null,
                "desktopPath": null,
                "managedName": null,
                "onboardingCompleted": "yes",
                "version": 3
            }),
        ];

        for state in malformed {
            let backend = MemoryBackend::default();
            let lock = Mutex::new(());
            assert!(PrimaryWorkspaceService::new(&backend, &lock)
                .write(PrimaryWorkspaceWriteInput {
                    expected_state: None,
                    state,
                })
                .is_err());
            assert_eq!(backend.value(LOCAL_STATE_SCHEMA_VERSION_KEY), None);
            assert_eq!(backend.value(PRIMARY_WORKSPACE_KEY), None);
        }
    }

    #[test]
    fn primary_workspace_writes_serialize_the_typed_canonical_inner_payload() {
        let backend = MemoryBackend::default();
        let lock = Mutex::new(());
        let result = PrimaryWorkspaceService::new(&backend, &lock)
            .write(PrimaryWorkspaceWriteInput {
                expected_state: None,
                state: json!({
                    "desktopWorkspaceRoot": null,
                    "desktopPath": null,
                    "managedName": null,
                    "onboardingCompleted": false,
                    "onboardingRequestedForNextLaunch": false,
                    "version": 3
                }),
            })
            .expect("canonical typed write");
        let expected = json!({
            "desktopWorkspaceRoot": null,
            "desktopPath": null,
            "managedName": null,
            "onboardingCompleted": false,
            "version": 3
        });

        assert_eq!(result.state, expected);
        assert_eq!(backend.value(PRIMARY_WORKSPACE_KEY), Some(expected));
    }

    #[test]
    fn desktop_host_resolution_rejects_a_missing_selected_path() {
        let temporary = tempfile::tempdir().expect("desktop host workspace roots");
        let workspace_root = temporary.path().join("Workspace");
        let notebook = workspace_root.join("Notes");
        std::fs::create_dir_all(&workspace_root).expect("workspace root");
        let selected = completed_v3_desktop_state(&workspace_root, &notebook);
        let mut reset_for_next_launch = selected.clone();
        reset_for_next_launch["onboardingRequestedForNextLaunch"] = Value::Bool(true);

        for state in [selected, reset_for_next_launch] {
            assert_eq!(
                resolve_desktop_primary_workspace_value(Some(state)),
                Err(DesktopPrimaryWorkspaceResolutionError::Invalid)
            );
        }
    }

    #[test]
    fn desktop_host_resolution_rejects_same_address_rebinding() {
        let temporary = tempfile::tempdir().expect("desktop host workspace roots");
        let workspace_root = temporary.path().join("Workspace");
        let notebook = workspace_root.join("Notes");
        let displaced = workspace_root.join("Notes displaced");
        std::fs::create_dir_all(&notebook).expect("selected notebook");
        let state = completed_v3_desktop_state(&workspace_root, &notebook);

        let result =
            resolve_desktop_primary_workspace_value_with_validation_hook(Some(state), || {
                std::fs::rename(&notebook, &displaced).expect("displace selected notebook");
                std::fs::create_dir(&notebook).expect("replace selected notebook");
            });

        assert_eq!(result, Err(DesktopPrimaryWorkspaceResolutionError::Invalid));
    }

    #[test]
    fn desktop_host_resolution_reports_store_reads_as_unavailable() {
        assert_eq!(
            resolve_desktop_primary_workspace_read(Err(persistence_error())),
            Err(DesktopPrimaryWorkspaceResolutionError::Unavailable)
        );
    }

    #[test]
    fn desktop_startup_initialization_is_one_time_and_cannot_switch_roots() {
        let temporary = tempfile::tempdir().unwrap();
        let workspace = temporary.path().join("Workspace");
        let first = workspace.join("First");
        let second = workspace.join("Second");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        let backend = MemoryBackend::default();
        let lock = Mutex::new(());

        let selected =
            initialize_desktop_primary_workspace_with_backend(&backend, &lock, &first).unwrap();
        assert_eq!(selected, first.canonicalize().unwrap());
        assert_eq!(
            resolve_desktop_primary_workspace_value(backend.value(PRIMARY_WORKSPACE_KEY)),
            Ok(DesktopPrimaryWorkspaceResolution::Selected(selected))
        );

        assert!(
            initialize_desktop_primary_workspace_with_backend(&backend, &lock, &second).is_err()
        );
        assert_eq!(
            resolve_desktop_primary_workspace_value(backend.value(PRIMARY_WORKSPACE_KEY)),
            Ok(DesktopPrimaryWorkspaceResolution::Selected(
                first.canonicalize().unwrap()
            ))
        );
    }

    #[test]
    fn desktop_runtime_switches_a_b_a_through_atomic_host_persistence() {
        let temporary = tempfile::tempdir().unwrap();
        let workspace = temporary.path().join("Workspace");
        let first = workspace.join("First");
        let second = workspace.join("Second");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        let backend = MemoryBackend::default();
        let lock = Mutex::new(());

        initialize_desktop_primary_workspace_with_backend(&backend, &lock, &first).unwrap();
        let selected_second =
            switch_desktop_primary_workspace_with_backend(&backend, &lock, &second).unwrap();
        let selected_first =
            switch_desktop_primary_workspace_with_backend(&backend, &lock, &first).unwrap();

        assert_eq!(selected_second, second.canonicalize().unwrap());
        assert_eq!(selected_first, first.canonicalize().unwrap());
        assert_eq!(
            resolve_desktop_primary_workspace_value(backend.value(PRIMARY_WORKSPACE_KEY)),
            Ok(DesktopPrimaryWorkspaceResolution::Selected(selected_first))
        );
    }

    #[test]
    fn desktop_runtime_switch_rollback_and_compare_and_swap_preserve_the_authoritative_root() {
        let temporary = tempfile::tempdir().unwrap();
        let workspace = temporary.path().join("Workspace");
        let first = workspace.join("First");
        let second = workspace.join("Second");
        let third = workspace.join("Third");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        std::fs::create_dir_all(&third).unwrap();
        let backend = MemoryBackend::default();
        let lock = Mutex::new(());

        let canonical_first =
            initialize_desktop_primary_workspace_with_backend(&backend, &lock, &first).unwrap();
        backend.fail_save.store(true, Ordering::Relaxed);
        assert!(switch_desktop_primary_workspace_with_backend(&backend, &lock, &second).is_err());
        assert_eq!(
            resolve_desktop_primary_workspace_value(backend.value(PRIMARY_WORKSPACE_KEY)),
            Ok(DesktopPrimaryWorkspaceResolution::Selected(
                canonical_first.clone()
            ))
        );

        backend.fail_save.store(false, Ordering::Relaxed);
        let canonical_second =
            switch_desktop_primary_workspace_with_backend(&backend, &lock, &second).unwrap();
        assert!(select_desktop_primary_workspace_with_backend(
            &backend,
            &lock,
            &third,
            DesktopPrimaryWorkspaceSelectionMode::Switch {
                expected_root: &canonical_first,
            },
        )
        .is_err());
        assert_eq!(
            resolve_desktop_primary_workspace_value(backend.value(PRIMARY_WORKSPACE_KEY)),
            Ok(DesktopPrimaryWorkspaceResolution::Selected(
                canonical_second
            ))
        );
    }

    #[test]
    fn desktop_startup_recovery_replaces_only_an_explicitly_invalid_record() {
        let temporary = tempfile::tempdir().unwrap();
        let workspace = temporary.path().join("Workspace");
        let selected = workspace.join("Recovered");
        std::fs::create_dir_all(&selected).unwrap();
        let backend = MemoryBackend::with([(
            PRIMARY_WORKSPACE_KEY,
            serde_json::json!({
                "desktopWorkspaceRoot": workspace,
                "desktopPath": 42,
                "managedName": null,
                "onboardingCompleted": true,
                "version": 3
            }),
        )]);
        let lock = Mutex::new(());

        let recovered =
            recover_invalid_desktop_primary_workspace_with_backend(&backend, &lock, &selected)
                .unwrap();
        assert_eq!(recovered, selected.canonicalize().unwrap());
        assert_eq!(
            resolve_desktop_primary_workspace_value(backend.value(PRIMARY_WORKSPACE_KEY)),
            Ok(DesktopPrimaryWorkspaceResolution::Selected(recovered))
        );

        assert!(
            recover_invalid_desktop_primary_workspace_with_backend(&backend, &lock, &selected,)
                .is_err()
        );
    }

    fn native_host_workspace_store(
        workspace: &Path,
        app_data: &Path,
        cache: &Path,
    ) -> qingyu_kernel::host::native::NativeHostWorkspaceStore {
        let paths = qingyu_kernel::paths::KernelPaths::desktop(workspace, app_data, cache)
            .expect("native host state paths");
        let config = qingyu_kernel::config::KernelConfig::generate()
            .expect("native host state launch epoch");
        open_native_host_workspace_store(&paths, &config).expect("native host state store")
    }

    #[test]
    fn native_host_workspace_state_survives_restart_and_a_b_a_switches() {
        let temporary = tempfile::tempdir().expect("native host workspace roots");
        let parent = temporary.path().join("workspaces");
        let workspace_a = parent.join("Workspace A");
        let workspace_b = parent.join("Workspace B");
        let app_data = temporary.path().join("app-data");
        let cache = temporary.path().join("cache");
        std::fs::create_dir_all(&workspace_a).expect("workspace A");
        std::fs::create_dir(&workspace_b).expect("workspace B");
        std::fs::create_dir(&app_data).expect("app data");
        std::fs::create_dir(&cache).expect("cache");
        let backend = MemoryBackend::with([
            (
                LOCAL_STATE_SCHEMA_VERSION_KEY,
                Value::from(LOCAL_STATE_SCHEMA_VERSION),
            ),
            (PRIMARY_WORKSPACE_KEY, completed_state(workspace_a.to_str())),
        ]);
        let transaction_lock = Mutex::new(());
        let native_store = native_host_workspace_store(&workspace_a, &app_data, &cache);

        let first =
            NativeHostWorkspaceStatePersistence::new(&backend, &native_store, &transaction_lock)
                .load_or_create(&workspace_a)
                .expect("persist workspace A");
        let restarted =
            NativeHostWorkspaceStatePersistence::new(&backend, &native_store, &transaction_lock)
                .load_or_create(&workspace_a)
                .expect("reuse workspace A after child restart");
        assert_eq!(first, restarted);

        PrimaryWorkspaceService::new(&backend, &transaction_lock)
            .write(write_input(workspace_b.to_str().expect("workspace B path")))
            .expect("switch to workspace B");
        let second =
            NativeHostWorkspaceStatePersistence::new(&backend, &native_store, &transaction_lock)
                .load_or_create(&workspace_b)
                .expect("persist workspace B");
        assert_ne!(first, second);

        let mut obsolete_embedded_state =
            write_input(workspace_a.to_str().expect("workspace A path"));
        obsolete_embedded_state.state["nativeHostWorkspaceStates"] = json!({
            "schemaVersion": 1,
            "states": []
        });
        assert!(PrimaryWorkspaceService::new(&backend, &transaction_lock)
            .write(obsolete_embedded_state)
            .is_err());
        PrimaryWorkspaceService::new(&backend, &transaction_lock)
            .write(write_input(workspace_a.to_str().expect("workspace A path")))
            .expect("return to workspace A through host-owned state");
        let returned =
            NativeHostWorkspaceStatePersistence::new(&backend, &native_store, &transaction_lock)
                .load_or_create(&workspace_a)
                .expect("reuse workspace A after A-B-A switch");

        assert_eq!(first, returned);
        assert_eq!(
            format!("{returned:?}"),
            "NativeHostWorkspaceState([REDACTED])"
        );
        assert_eq!(backend.value("nativeHostWorkspaceStates"), None);
    }

    #[test]
    fn desktop_restore_target_uses_one_exact_validated_child() {
        let temporary = tempfile::tempdir().unwrap();
        let parent = temporary.path().join("parent");
        std::fs::create_dir(&parent).unwrap();

        let prepared = super::prepare_desktop_notebook_target_at_path(
            parent.to_str().unwrap(),
            "  个人 笔记  ",
        )
        .unwrap();

        assert_eq!(
            prepared,
            parent.join("  个人 笔记  ").canonicalize().unwrap()
        );
        assert!(prepared.is_dir());
        assert_eq!(
            super::prepare_desktop_notebook_target_at_path(
                parent.to_str().unwrap(),
                "  个人 笔记  ",
            )
            .unwrap(),
            prepared
        );
        for invalid in ["", ".", "..", "nested/name", r"nested\name", ".qingyu"] {
            assert!(super::prepare_desktop_notebook_target_at_path(
                parent.to_str().unwrap(),
                invalid,
            )
            .is_err());
        }
    }

    #[cfg(unix)]
    #[test]
    fn desktop_restore_target_rejects_symlink_and_non_directory_children() {
        use std::os::unix::fs::symlink;

        let temporary = tempfile::tempdir().unwrap();
        let parent = temporary.path().join("parent");
        let outside = temporary.path().join("outside");
        std::fs::create_dir(&parent).unwrap();
        std::fs::create_dir(&outside).unwrap();
        symlink(&outside, parent.join("linked")).unwrap();
        std::fs::write(parent.join("file"), b"not a directory").unwrap();

        assert!(
            super::prepare_desktop_notebook_target_at_path(parent.to_str().unwrap(), "linked",)
                .is_err()
        );
        assert!(
            super::prepare_desktop_notebook_target_at_path(parent.to_str().unwrap(), "file",)
                .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn prepared_desktop_restore_target_rejects_replacement_before_any_sync_action() {
        use std::os::unix::fs::symlink;

        let _guard = prepared_target_test_lock().lock().unwrap();
        let temporary = tempfile::tempdir().unwrap();
        let parent = temporary.path().join("parent");
        let outside_parent = temporary.path().join("outside");
        let outside_target = outside_parent.join("Cloud Notes");
        std::fs::create_dir(&parent).unwrap();
        std::fs::create_dir(&outside_parent).unwrap();
        std::fs::create_dir(&outside_target).unwrap();
        let prepared = super::prepare_desktop_notebook_target_lease_at_path(
            parent.to_str().unwrap(),
            "Cloud Notes",
        )
        .unwrap();
        let displaced = parent.join("displaced");
        std::fs::rename(&prepared.notes_root, &displaced).unwrap();
        symlink(&outside_target, &prepared.notes_root).unwrap();
        let sync_actions = std::sync::atomic::AtomicUsize::new(0);

        let consumed = super::consume_prepared_desktop_notebook_target(&prepared.lease).map(|_| {
            sync_actions.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });

        assert!(consumed.is_err());
        assert_eq!(sync_actions.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[test]
    fn prepared_desktop_restore_target_discard_restores_registry_baseline() {
        let _guard = prepared_target_test_lock().lock().unwrap();
        let temporary = tempfile::tempdir().unwrap();
        let parent = temporary.path().join("parent");
        std::fs::create_dir(&parent).unwrap();
        let baseline = prepared_target_count();
        let prepared = super::prepare_desktop_notebook_target_lease_at_path(
            parent.to_str().unwrap(),
            "Cloud Notes",
        )
        .unwrap();
        assert_eq!(prepared_target_count(), baseline + 1);

        super::discard_prepared_desktop_notebook_target_lease(&prepared.lease).unwrap();

        assert_eq!(prepared_target_count(), baseline);
        super::discard_prepared_desktop_notebook_target_lease(&prepared.lease).unwrap();
        assert_eq!(prepared_target_count(), baseline);
        assert!(super::consume_prepared_desktop_notebook_target(&prepared.lease).is_err());
    }

    #[test]
    fn prepared_desktop_restore_target_is_single_use_after_consume() {
        let _guard = prepared_target_test_lock().lock().unwrap();
        let temporary = tempfile::tempdir().unwrap();
        let parent = temporary.path().join("parent");
        std::fs::create_dir(&parent).unwrap();
        let baseline = prepared_target_count();
        let prepared = super::prepare_desktop_notebook_target_lease_at_path(
            parent.to_str().unwrap(),
            "Cloud Notes",
        )
        .unwrap();

        let consumed = super::consume_prepared_desktop_notebook_target(&prepared.lease).unwrap();

        assert_eq!(consumed.notes_root, PathBuf::from(prepared.notes_root));
        assert_eq!(prepared_target_count(), baseline);
        super::discard_prepared_desktop_notebook_target_lease(&prepared.lease).unwrap();
        assert_eq!(prepared_target_count(), baseline);
        assert!(super::consume_prepared_desktop_notebook_target(&prepared.lease).is_err());
    }

    #[test]
    fn consumed_desktop_restore_target_rejects_same_name_replacement_before_publish() {
        let _guard = prepared_target_test_lock().lock().unwrap();
        let temporary = tempfile::tempdir().unwrap();
        let parent = temporary.path().join("parent");
        std::fs::create_dir(&parent).unwrap();
        let prepared = super::prepare_desktop_notebook_target_lease_at_path(
            parent.to_str().unwrap(),
            "Cloud Notes",
        )
        .unwrap();
        let consumed = super::consume_prepared_desktop_notebook_target(&prepared.lease).unwrap();

        let displaced = parent.join("displaced");
        std::fs::rename(&prepared.notes_root, &displaced).unwrap();
        std::fs::create_dir(&prepared.notes_root).unwrap();

        assert!(consumed.validate_current_address().is_err());
        assert!(displaced.is_dir());
        assert!(std::path::Path::new(&prepared.notes_root).is_dir());
    }

    #[test]
    fn native_prepared_commit_fails_closed_when_the_addressed_child_was_replaced() {
        let _guard = prepared_target_test_lock().lock().unwrap();
        let temporary = tempfile::tempdir().unwrap();
        let parent = temporary.path().join("Workspace");
        let old_root = parent.join("A");
        std::fs::create_dir_all(&old_root).unwrap();
        let previous = completed_v3_desktop_state(&parent, &old_root);
        let prepared = super::prepare_desktop_notebook_target_lease_at_path_with_expected(
            parent.to_str().unwrap(),
            "B",
            Some(previous.clone()),
        )
        .unwrap();
        let consumed = super::consume_prepared_desktop_notebook_target(&prepared.lease).unwrap();
        std::fs::rename(parent.join("B"), parent.join("B-replaced")).unwrap();
        std::fs::create_dir(parent.join("B")).unwrap();
        let backend = MemoryBackend::with([(PRIMARY_WORKSPACE_KEY, previous.clone())]);
        let lock = Mutex::new(());

        let error = consumed
            .commit_primary_workspace_with_backend(&backend, &lock)
            .expect_err("a replaced final child must not be committed");

        assert_eq!(error, notebook_target_error());
        assert_eq!(backend.value(PRIMARY_WORKSPACE_KEY), Some(previous));
    }

    #[test]
    fn native_prepared_commit_publishes_the_validated_child_exactly_once() {
        let _guard = prepared_target_test_lock().lock().unwrap();
        let temporary = tempfile::tempdir().unwrap();
        let parent = temporary.path().join("Workspace");
        let old_root = parent.join("A");
        std::fs::create_dir_all(&old_root).unwrap();
        let previous = completed_v3_desktop_state(&parent, &old_root);
        let prepared = super::prepare_desktop_notebook_target_lease_at_path_with_expected(
            parent.to_str().unwrap(),
            "B",
            Some(previous.clone()),
        )
        .unwrap();
        let consumed = super::consume_prepared_desktop_notebook_target(&prepared.lease).unwrap();
        let backend = MemoryBackend::with([(PRIMARY_WORKSPACE_KEY, previous)]);
        let lock = Mutex::new(());

        let result = consumed
            .commit_primary_workspace_with_backend(&backend, &lock)
            .unwrap();

        assert!(result.applied);
        assert_eq!(backend.saves.load(Ordering::Relaxed), 1);
        assert_eq!(
            backend.value(PRIMARY_WORKSPACE_KEY),
            Some(completed_v3_desktop_state(
                &parent.canonicalize().unwrap(),
                &parent.join("B").canonicalize().unwrap(),
            ))
        );
    }

    #[test]
    fn native_prepared_commit_rejects_activating_a_root_held_by_an_inactive_sync_permit() {
        let _guard = prepared_target_test_lock().lock().unwrap();
        let temporary = tempfile::tempdir().unwrap();
        let parent = temporary.path().join("Workspace");
        let old_root = parent.join("A");
        std::fs::create_dir_all(&old_root).unwrap();
        let previous = completed_v3_desktop_state(&parent, &old_root);
        let prepared = super::prepare_desktop_notebook_target_lease_at_path_with_expected(
            parent.to_str().unwrap(),
            "B",
            Some(previous.clone()),
        )
        .unwrap();
        let consumed = super::consume_prepared_desktop_notebook_target(&prepared.lease).unwrap();
        let backend = MemoryBackend::with([(PRIMARY_WORKSPACE_KEY, previous.clone())]);
        let lock = Mutex::new(());
        let registry = std::sync::Arc::new(
            crate::dejavu_sync::path_guard::NativeWorkingTreeRegistry::default(),
        );
        let _ownership = registry.lease_ownership(consumed.notes_root.clone(), false);

        let error = consumed
            .commit_primary_workspace_with_backend_and_registry(&backend, &lock, &registry)
            .expect_err("an inactive sync permit must keep its root inactive");

        assert_eq!(error, "sync-path-guarded");
        assert_eq!(backend.value(PRIMARY_WORKSPACE_KEY), Some(previous));
    }

    #[test]
    fn concurrent_prepared_desktop_restore_consumers_cannot_replay_a_lease() {
        let _guard = prepared_target_test_lock().lock().unwrap();
        let temporary = tempfile::tempdir().unwrap();
        let parent = temporary.path().join("parent");
        std::fs::create_dir(&parent).unwrap();
        let baseline = prepared_target_count();
        let prepared = super::prepare_desktop_notebook_target_lease_at_path(
            parent.to_str().unwrap(),
            "Cloud Notes",
        )
        .unwrap();
        let start = std::sync::Arc::new(std::sync::Barrier::new(3));
        let consumers = (0..2)
            .map(|_| {
                let lease = prepared.lease.clone();
                let start = start.clone();
                std::thread::spawn(move || {
                    start.wait();
                    super::consume_prepared_desktop_notebook_target(&lease).is_ok()
                })
            })
            .collect::<Vec<_>>();
        start.wait();
        let successes = consumers
            .into_iter()
            .map(|consumer| consumer.join().unwrap())
            .filter(|succeeded| *succeeded)
            .count();

        assert_eq!(successes, 1);
        assert_eq!(prepared_target_count(), baseline);
    }

    #[test]
    fn desktop_sync_accepts_only_the_configured_primary_workspace() {
        let temporary = tempfile::tempdir().unwrap();
        let primary = temporary.path().join("primary");
        let external = temporary.path().join("external");
        std::fs::create_dir(&primary).unwrap();
        std::fs::create_dir(&external).unwrap();
        let state = completed_state(primary.to_str());

        let accepted = validate_primary_workspace_identity(
            Some(state.clone()),
            PrimaryWorkspaceKind::Desktop,
            None,
            primary.to_str().unwrap(),
        )
        .unwrap();
        assert_eq!(accepted, primary.canonicalize().unwrap());
        assert_eq!(
            validate_primary_workspace_identity(
                Some(state),
                PrimaryWorkspaceKind::Desktop,
                None,
                external.to_str().unwrap(),
            )
            .unwrap_err(),
            "sync-primary-workspace-mismatch: The requested notes root is not the primary workspace."
        );
    }

    #[test]
    fn version_3_desktop_authority_requires_an_exact_direct_workspace_child() {
        let temporary = tempfile::tempdir().unwrap();
        let workspace = temporary.path().join("Workspace");
        let notebook = workspace.join("Notes");
        let nested = notebook.join("Nested");
        let outside = temporary.path().join("Outside");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir(&outside).unwrap();

        let accepted = authoritative_primary_workspace_root(
            Some(completed_v3_desktop_state(&workspace, &notebook)),
            PrimaryWorkspaceKind::Desktop,
            None,
        )
        .unwrap();
        assert_eq!(accepted, notebook.canonicalize().unwrap());

        for invalid_notebook in [&nested, &outside] {
            assert_eq!(
                authoritative_primary_workspace_root(
                    Some(completed_v3_desktop_state(&workspace, invalid_notebook)),
                    PrimaryWorkspaceKind::Desktop,
                    None,
                )
                .unwrap_err(),
                sync_primary_workspace_unavailable()
            );
        }
    }

    #[test]
    fn version_3_contract_rejects_version_2_one_sided_and_mixed_desktop_identity() {
        let temporary = tempfile::tempdir().unwrap();
        let workspace = temporary.path().join("Workspace");
        let notebook = workspace.join("Notes");
        std::fs::create_dir_all(&notebook).unwrap();
        let unavailable = sync_primary_workspace_unavailable();
        let invalid_states = [
            json!({
                "desktopWorkspaceRoot": workspace,
                "desktopPath": notebook,
                "managedName": null,
                "onboardingCompleted": true,
                "version": 2
            }),
            json!({
                "desktopWorkspaceRoot": workspace,
                "desktopPath": null,
                "managedName": null,
                "onboardingCompleted": true,
                "version": 3
            }),
            json!({
                "desktopWorkspaceRoot": null,
                "desktopPath": notebook,
                "managedName": null,
                "onboardingCompleted": true,
                "version": 3
            }),
            json!({
                "desktopWorkspaceRoot": workspace,
                "desktopPath": notebook,
                "managedName": "personal",
                "onboardingCompleted": true,
                "version": 3
            }),
        ];

        for state in invalid_states {
            assert_eq!(
                authoritative_primary_workspace_root(
                    Some(state),
                    PrimaryWorkspaceKind::Desktop,
                    None,
                )
                .unwrap_err(),
                unavailable
            );
        }
    }

    #[test]
    fn desktop_sync_preserves_a_canonical_root_with_a_trailing_space() {
        let temporary = tempfile::tempdir().unwrap();
        let primary = temporary.path().join("Notes ");
        std::fs::create_dir(&primary).unwrap();

        let accepted = validate_primary_workspace_identity(
            Some(completed_state(primary.to_str())),
            PrimaryWorkspaceKind::Desktop,
            None,
            primary.to_str().unwrap(),
        )
        .unwrap();

        assert_eq!(accepted, primary.canonicalize().unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn desktop_sync_accepts_a_canonical_alias_of_the_primary_workspace() {
        use std::os::unix::fs::symlink;

        let temporary = tempfile::tempdir().unwrap();
        let primary = temporary.path().join("primary");
        let alias = temporary.path().join("primary-alias");
        std::fs::create_dir(&primary).unwrap();
        symlink(&primary, &alias).unwrap();

        let accepted = validate_primary_workspace_identity(
            Some(completed_state(primary.to_str())),
            PrimaryWorkspaceKind::Desktop,
            None,
            alias.to_str().unwrap(),
        )
        .unwrap();

        assert_eq!(accepted, primary.canonicalize().unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn desktop_sync_canonicalizes_stored_workspace_and_notebook_aliases() {
        use std::os::unix::fs::symlink;

        let temporary = tempfile::tempdir().unwrap();
        let workspace = temporary.path().join("Workspace");
        let notebook = workspace.join("Notes");
        let workspace_alias = temporary.path().join("Workspace Alias");
        std::fs::create_dir_all(&notebook).unwrap();
        symlink(&workspace, &workspace_alias).unwrap();
        let notebook_alias = workspace_alias.join("Notes");

        let accepted = authoritative_primary_workspace_root(
            Some(completed_v3_desktop_state(
                &workspace_alias,
                &notebook_alias,
            )),
            PrimaryWorkspaceKind::Desktop,
            None,
        )
        .unwrap();

        assert_eq!(accepted, notebook.canonicalize().unwrap());
    }

    #[test]
    fn desktop_sync_rejects_missing_incomplete_or_reset_primary_state() {
        let temporary = tempfile::tempdir().unwrap();
        let primary = temporary.path().join("primary");
        std::fs::create_dir(&primary).unwrap();
        let unavailable =
            "sync-primary-workspace-unavailable: The primary workspace is unavailable.";
        let states = [
            None,
            Some(completed_state(None)),
            Some(json!({
                "desktopWorkspaceRoot": primary.parent(),
                "desktopPath": primary,
                "managedName": null,
                "onboardingCompleted": false,
                "version": 3
            })),
            Some(json!({
                "desktopWorkspaceRoot": primary.parent(),
                "desktopPath": primary,
                "managedName": null,
                "onboardingCompleted": true,
                "onboardingRequestedForNextLaunch": true,
                "version": 3
            })),
        ];

        for state in states {
            assert_eq!(
                validate_primary_workspace_identity(
                    state,
                    PrimaryWorkspaceKind::Desktop,
                    None,
                    primary.to_str().unwrap(),
                )
                .unwrap_err(),
                unavailable
            );
        }
    }

    #[test]
    fn mobile_sync_accepts_only_the_completed_managed_workspace() {
        let temporary = tempfile::tempdir().unwrap();
        let app_data = temporary.path().join("app-data");
        let external = temporary.path().join("external");
        std::fs::create_dir(&external).unwrap();
        let managed = app_data.join("workspaces/personal");
        let state = completed_mobile_state("personal");

        let accepted = validate_primary_workspace_identity(
            Some(state.clone()),
            PrimaryWorkspaceKind::Mobile,
            Some(&app_data),
            managed.to_str().unwrap(),
        )
        .unwrap();
        assert_eq!(accepted, managed.canonicalize().unwrap());
        assert_eq!(
            validate_primary_workspace_identity(
                Some(state),
                PrimaryWorkspaceKind::Mobile,
                Some(&app_data),
                external.to_str().unwrap(),
            )
            .unwrap_err(),
            "sync-primary-workspace-mismatch: The requested notes root is not the primary workspace."
        );
    }

    #[test]
    fn notebook_scope_contract_rejects_version_1_and_mixed_identities() {
        let temporary = tempfile::tempdir().unwrap();
        let desktop = temporary.path().join("desktop");
        let app_data = temporary.path().join("app-data");
        std::fs::create_dir(&desktop).unwrap();
        let unavailable = sync_primary_workspace_unavailable();
        let states = [
            json!({
                "desktopWorkspaceRoot": desktop.parent(),
                "desktopPath": desktop,
                "managedName": null,
                "onboardingCompleted": true,
                "version": 1
            }),
            json!({
                "desktopWorkspaceRoot": desktop.parent(),
                "desktopPath": desktop,
                "managedName": "personal",
                "onboardingCompleted": true,
                "version": 3
            }),
        ];

        for state in states {
            assert_eq!(
                authoritative_primary_workspace_root(
                    Some(state.clone()),
                    PrimaryWorkspaceKind::Desktop,
                    None,
                )
                .unwrap_err(),
                unavailable
            );
            assert_eq!(
                authoritative_primary_workspace_root(
                    Some(state),
                    PrimaryWorkspaceKind::Mobile,
                    Some(&app_data),
                )
                .unwrap_err(),
                unavailable
            );
        }
    }

    #[test]
    fn canonical_write_rejects_stale_flags_even_when_the_desktop_path_matches() {
        let expected = json!({
            "desktopWorkspaceRoot": "/alias",
            "desktopPath": "/alias/Notes-A",
            "managedName": null,
            "onboardingCompleted": true,
            "version": 3
        });
        let current = json!({
            "desktopWorkspaceRoot": "/alias",
            "desktopPath": "/alias/Notes-A",
            "managedName": null,
            "onboardingCompleted": true,
            "onboardingRequestedForNextLaunch": true,
            "version": 3
        });
        let backend = MemoryBackend::with([(PRIMARY_WORKSPACE_KEY, current.clone())]);
        let transaction_lock = Mutex::new(());
        let service = PrimaryWorkspaceService::new(&backend, &transaction_lock);

        let result = service
            .write(PrimaryWorkspaceWriteInput {
                expected_state: Some(expected),
                state: json!({
                    "desktopWorkspaceRoot": "/canonical",
                    "desktopPath": "/canonical/Notes-A",
                    "managedName": null,
                    "onboardingCompleted": true,
                    "version": 3
                }),
            })
            .expect("canonical compare-and-set result");

        assert!(!result.applied);
        assert_eq!(result.state, current);
        assert_eq!(backend.value(PRIMARY_WORKSPACE_KEY), Some(current));
    }

    #[test]
    fn guarded_primary_workspace_write_is_rolled_back_if_a_lease_appears_during_save() {
        let temporary = tempfile::tempdir().unwrap();
        let workspace = temporary.path().join("Workspace");
        let old_root = workspace.join("A");
        let next_root = workspace.join("B");
        std::fs::create_dir_all(&old_root).unwrap();
        std::fs::create_dir(&next_root).unwrap();
        let (backend, save_started, release_save) = BlockingBackend::new();
        let backend = std::sync::Arc::new(backend);
        let transaction_lock = std::sync::Arc::new(Mutex::new(()));
        let registry = std::sync::Arc::new(
            crate::dejavu_sync::path_guard::NativeWorkingTreeRegistry::default(),
        );
        let writer = {
            let backend = backend.clone();
            let transaction_lock = transaction_lock.clone();
            let registry = registry.clone();
            let next_root = next_root.clone();
            std::thread::spawn(move || {
                PrimaryWorkspaceService::new(backend.as_ref(), transaction_lock.as_ref())
                    .write_with_primary_root_guard(
                        write_input(next_root.to_str().unwrap()),
                        Some(&next_root),
                        &registry,
                    )
            })
        };
        save_started.recv().unwrap();
        let _ownership = registry.lease_ownership(old_root, true);
        release_save.send(()).unwrap();

        assert_eq!(writer.join().unwrap().unwrap_err(), "sync-path-guarded");
        assert_eq!(backend.inner.value(PRIMARY_WORKSPACE_KEY), None);
    }

    #[test]
    fn failed_save_restores_the_previous_memory_values_before_a_later_write() {
        let backend = MemoryBackend::with([(
            PRIMARY_WORKSPACE_KEY,
            json!({ "desktopPath": "/Notes-A", "onboardingCompleted": true, "version": 1 }),
        )]);
        let transaction_lock = Mutex::new(());
        let service = PrimaryWorkspaceService::new(&backend, &transaction_lock);
        backend.fail_save.store(true, Ordering::Relaxed);

        assert!(service.write(write_input("/Notes-B")).is_err());
        assert_eq!(
            backend.value(PRIMARY_WORKSPACE_KEY),
            Some(json!({ "desktopPath": "/Notes-A", "onboardingCompleted": true, "version": 1 }))
        );
        assert_eq!(backend.value(LOCAL_STATE_SCHEMA_VERSION_KEY), None);

        backend.fail_save.store(false, Ordering::Relaxed);
        assert_eq!(
            service
                .write(write_input("/Notes-C"))
                .expect("later write")
                .state,
            write_input("/Notes-C").state
        );
        assert_eq!(
            backend.value(PRIMARY_WORKSPACE_KEY),
            Some(write_input("/Notes-C").state)
        );
    }

    #[test]
    fn serializes_complete_writes_so_the_later_intent_persists_last() {
        let (backend, first_save_started, release_first_save) = BlockingBackend::new();
        let transaction_lock = Mutex::new(());
        let service = PrimaryWorkspaceService::new(&backend, &transaction_lock);
        let (second_attempted_sender, second_attempted_receiver) = mpsc::channel();
        let (second_completed_sender, second_completed_receiver) = mpsc::channel();

        std::thread::scope(|scope| {
            let first_service = &service;
            let first = scope.spawn(move || first_service.write(write_input("/Notes-A")));
            first_save_started.recv().expect("first save started");

            let second_service = &service;
            let second = scope.spawn(move || {
                second_attempted_sender.send(()).expect("second attempted");
                let result = second_service.write(write_input("/Notes-B"));
                second_completed_sender.send(()).expect("second completed");
                result
            });
            second_attempted_receiver
                .recv()
                .expect("second write attempted");
            assert!(second_completed_receiver
                .recv_timeout(Duration::from_millis(50))
                .is_err());
            assert_eq!(
                backend.get(PRIMARY_WORKSPACE_KEY),
                Some(write_input("/Notes-A").state)
            );

            release_first_save.send(()).expect("release first save");
            assert!(first.join().expect("first thread").is_ok());
            assert!(second.join().expect("second thread").is_ok());
        });

        assert_eq!(
            service.read().expect("read latest state"),
            Some(write_input("/Notes-B").state)
        );
    }

    #[test]
    fn authority_install_reads_and_applies_inside_the_local_state_transaction() {
        let temporary = tempfile::tempdir().expect("temporary workspace roots");
        let primary_a = temporary.path().join("Notes-A");
        let primary_b = temporary.path().join("Notes-B");
        std::fs::create_dir(&primary_a).expect("primary A");
        std::fs::create_dir(&primary_b).expect("primary B");
        let (backend, first_save_started, release_first_save) = BlockingBackend::new();
        backend.set(PRIMARY_WORKSPACE_KEY, completed_state(primary_a.to_str()));
        let transaction_lock = Mutex::new(());
        let service = PrimaryWorkspaceService::new(&backend, &transaction_lock);
        let registry = crate::mcp::workspaces::WorkspaceRegistry::new(Vec::new());
        registry
            .activate_current(&primary_a)
            .expect("initial MCP authority A");
        let (install_attempted_sender, install_attempted_receiver) = mpsc::channel();
        let (install_completed_sender, install_completed_receiver) = mpsc::channel();

        std::thread::scope(|scope| {
            let writer = scope.spawn(|| service.write(write_input(primary_b.to_str().unwrap())));
            first_save_started.recv().expect("B save started");

            let installer = scope.spawn(|| {
                install_attempted_sender
                    .send(())
                    .expect("install attempted");
                let result = service.with_current(|current| {
                    match validate_primary_workspace_identity(
                        current,
                        PrimaryWorkspaceKind::Desktop,
                        None,
                        primary_a.to_str().unwrap(),
                    ) {
                        Ok(root) => registry
                            .activate_current(&root)
                            .map(|_| ())
                            .map_err(|error| error.to_string()),
                        Err(error) => {
                            registry
                                .clear_current()
                                .map_err(|clear_error| clear_error.to_string())?;
                            Err(error)
                        }
                    }
                });
                install_completed_sender
                    .send(())
                    .expect("install completed");
                result
            });
            install_attempted_receiver
                .recv()
                .expect("installer attempted transaction");
            assert!(install_completed_receiver
                .recv_timeout(Duration::from_millis(50))
                .is_err());

            release_first_save.send(()).expect("persist B");
            assert!(writer.join().expect("join B writer").is_ok());
            let install_error = installer
                .join()
                .expect("join authority installer")
                .expect_err("stale A request must fail closed");
            assert_eq!(install_error, sync_primary_workspace_mismatch());
        });

        assert_eq!(
            service.read().expect("latest local-state"),
            Some(completed_state(primary_b.to_str()))
        );
        assert!(
            registry.list_safe().is_empty(),
            "authority must not install stale A after local-state persisted B"
        );
    }

    #[tokio::test]
    async fn trusted_kernel_adapter_is_instance_scoped_and_matches_direct_and_api_results() {
        async fn fixture(
            root: &Path,
        ) -> (
            Arc<qingyu_kernel::runtime::KernelRuntime>,
            Arc<qingyu_kernel::services::workspace::WorkspaceService>,
            Arc<AtomicDesktopHostRecord>,
        ) {
            let workspace = root.join("workspace");
            let app_data = root.join("app-data");
            let cache = root.join("cache");
            for path in [&workspace, &app_data, &cache] {
                std::fs::create_dir_all(path).expect("kernel adapter fixture directory");
            }
            let paths = qingyu_kernel::paths::KernelPaths::desktop(&workspace, &app_data, &cache)
                .expect("kernel adapter paths");
            let managed =
                qingyu_kernel::workspace::managed::ManagedWorkspaceCollection::from_paths(&paths)
                    .expect("managed collection");
            let runtime = qingyu_kernel::runtime::KernelRuntime::activate(
                qingyu_kernel::config::KernelConfig::generate().expect("kernel config"),
                paths,
                qingyu_kernel::ports::KernelPorts::unavailable(),
            )
            .expect("kernel runtime");
            let store = Arc::new(AtomicDesktopHostRecord {
                value: Arc::new(Mutex::new(json!({ "desktopPath": workspace }))),
                binding: qingyu_kernel::workspace::primary::PrimaryWorkspaceRepositoryBinding::new(
                ),
            });
            let service = Arc::new(
                qingyu_kernel::services::workspace::WorkspaceService::new(
                    &runtime,
                    store.clone(),
                    managed,
                    runtime.event_broker().clone(),
                    "Initial Workspace",
                )
                .await
                .expect("workspace service"),
            );
            (runtime, service, store)
        }

        let temporary = tempfile::tempdir().expect("trusted adapter roots");
        let first_root = temporary.path().join("first");
        let second_root = temporary.path().join("second");
        let target = temporary.path().join("Target Workspace");
        std::fs::create_dir(&target).expect("target workspace");
        let (first_runtime, first_service, first_store) = fixture(&first_root).await;
        let (second_runtime, second_service, second_store) = fixture(&second_root).await;
        let first_adapter = TrustedDesktopWorkspaceAdapter::new(
            first_runtime,
            first_service.clone(),
            first_store.clone(),
        );
        let second_adapter = TrustedDesktopWorkspaceAdapter::new(
            second_runtime,
            second_service.clone(),
            second_store,
        );
        let direct_before = first_service.current().expect("direct current");
        let api_before =
            qingyu_kernel::runtime::WorkspaceApiService::get_workspace(first_service.as_ref())
                .await
                .expect("API current");
        let adapter_before = first_adapter.current().expect("adapter current");
        let prepared = first_adapter
            .prepare_host_workspace(&target)
            .expect("trusted prepare");
        let prepared_debug = format!("{prepared:?}");

        let foreign_error = second_adapter
            .compare_and_set_host_workspace(
                &second_service.current().expect("second current").revision,
                prepared.clone(),
            )
            .await
            .expect_err("prepared token must be instance scoped");

        assert_eq!(direct_before, api_before);
        assert_eq!(direct_before, adapter_before);
        assert_eq!(prepared_debug, "TrustedPreparedWorkspaceToken([REDACTED])");
        assert!(!prepared_debug.contains(target.to_string_lossy().as_ref()));
        assert_eq!(
            foreign_error.kind(),
            qingyu_kernel::services::workspace::WorkspaceServiceErrorKind::PreparedAuthorityMismatch
        );

        let committed = first_adapter
            .compare_and_set_host_workspace(&direct_before.revision, prepared.clone())
            .await
            .expect("trusted CAS");
        let api_after =
            qingyu_kernel::runtime::WorkspaceApiService::get_workspace(first_service.as_ref())
                .await
                .expect("API committed current");

        assert_eq!(
            committed,
            first_service.current().expect("direct committed current")
        );
        assert_eq!(committed, api_after);
        assert_eq!(
            committed,
            first_adapter.current().expect("adapter committed current")
        );
        assert_eq!(committed.display_name, "Target Workspace");
        let host_record = first_store.raw();
        assert_eq!(host_record.get("desktopPath"), Some(&json!(target)));
        assert_eq!(
            host_record.get("kernelWorkspace"),
            qingyu_kernel::workspace::primary::PrimaryWorkspaceStore::load(first_store.as_ref())
                .expect("same host record")
                .as_ref()
        );
        assert!(!serde_json::to_string(&committed)
            .expect("workspace wire JSON")
            .contains(target.to_string_lossy().as_ref()));
        assert!(first_adapter
            .compare_and_set_host_workspace(&committed.revision, prepared)
            .await
            .is_err());
    }
}
