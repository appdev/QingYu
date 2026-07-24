use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use cap_fs_ext::{DirExt, MetadataExt};
use cap_std::fs::Dir;
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[cfg(mobile)]
use tauri::Manager;
use tauri_plugin_store::StoreExt;

const LOCAL_STATE_STORE_PATH: &str = "local-state.json";
const LOCAL_STATE_SCHEMA_VERSION_KEY: &str = "schemaVersion";
const LOCAL_STATE_SCHEMA_VERSION: u64 = 2;
const PRIMARY_WORKSPACE_KEY: &str = "primaryWorkspace";

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrimaryWorkspaceWriteInput {
    #[serde(default)]
    expected_state: Option<Value>,
    state: Value,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrimaryWorkspaceWriteResult {
    applied: bool,
    state: Value,
}

trait PrimaryWorkspaceBackend: Sync {
    fn delete(&self, key: &str);
    fn get(&self, key: &str) -> Option<Value>;
    fn save(&self) -> Result<(), String>;
    fn set(&self, key: &str, value: Value);
}

struct StorePrimaryWorkspaceBackend<R: tauri::Runtime> {
    store: Arc<tauri_plugin_store::Store<R>>,
}

impl<R: tauri::Runtime> PrimaryWorkspaceBackend for StorePrimaryWorkspaceBackend<R> {
    fn delete(&self, key: &str) {
        self.store.delete(key);
    }

    fn get(&self, key: &str) -> Option<Value> {
        self.store.get(key)
    }

    fn save(&self) -> Result<(), String> {
        self.store.save().map_err(|_| persistence_error())
    }

    fn set(&self, key: &str, value: Value) {
        self.store.set(key, value);
    }
}

struct PrimaryWorkspaceService<'a, Backend: PrimaryWorkspaceBackend + ?Sized> {
    backend: &'a Backend,
    transaction_lock: &'a Mutex<()>,
}

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
        let _transaction = self
            .transaction_lock
            .lock()
            .map_err(|_| persistence_error())?;
        operation(self.backend.get(PRIMARY_WORKSPACE_KEY))
    }

    fn restore_value(&self, key: &str, value: Option<Value>) {
        if let Some(value) = value {
            self.backend.set(key, value);
        } else {
            self.backend.delete(key);
        }
    }

    fn write(
        &self,
        input: PrimaryWorkspaceWriteInput,
    ) -> Result<PrimaryWorkspaceWriteResult, String> {
        self.write_validated(input, || Ok(()))
    }

    fn write_validated(
        &self,
        input: PrimaryWorkspaceWriteInput,
        mut validate: impl FnMut() -> Result<(), String>,
    ) -> Result<PrimaryWorkspaceWriteResult, String> {
        let _transaction = self
            .transaction_lock
            .lock()
            .map_err(|_| persistence_error())?;
        let previous_schema_version = self.backend.get(LOCAL_STATE_SCHEMA_VERSION_KEY);
        let previous_primary_workspace = self.backend.get(PRIMARY_WORKSPACE_KEY);
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
        self.backend.set(PRIMARY_WORKSPACE_KEY, input.state.clone());
        if let Err(error) = self.backend.save() {
            self.restore_value(PRIMARY_WORKSPACE_KEY, previous_primary_workspace);
            self.restore_value(LOCAL_STATE_SCHEMA_VERSION_KEY, previous_schema_version);
            return Err(error);
        }
        if let Err(error) = validate() {
            self.restore_value(PRIMARY_WORKSPACE_KEY, previous_primary_workspace);
            self.restore_value(LOCAL_STATE_SCHEMA_VERSION_KEY, previous_schema_version);
            self.backend.save().map_err(|_| persistence_error())?;
            return Err(error);
        }

        Ok(PrimaryWorkspaceWriteResult {
            applied: true,
            state: input.state,
        })
    }
}

fn transaction_lock() -> &'static Mutex<()> {
    static TRANSACTION_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    TRANSACTION_LOCK.get_or_init(|| Mutex::new(()))
}

fn persistence_error() -> String {
    "primary workspace persistence is unavailable".to_string()
}

fn notebook_target_error() -> String {
    "notebook-target-invalid: The notebook target is unavailable.".to_string()
}

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
pub(crate) struct PreparedDesktopNotebookTarget {
    lease: String,
    notes_root: String,
}

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
        let result = PrimaryWorkspaceService::new(backend, lock).write_validated(
            PrimaryWorkspaceWriteInput {
                expected_state: Some(self.expected_primary_workspace.clone()),
                state: self.desired_primary_workspace_state(),
            },
            || self.validate_current_address(),
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
        let store = app
            .store(LOCAL_STATE_STORE_PATH)
            .map_err(|_| persistence_error())?;
        let backend = StorePrimaryWorkspaceBackend { store };
        self.commit_primary_workspace_with_backend(&backend, transaction_lock())
    }
}

fn prepared_desktop_notebook_targets(
) -> &'static Mutex<HashMap<String, PreparedDesktopNotebookDirectory>> {
    static TARGETS: OnceLock<Mutex<HashMap<String, PreparedDesktopNotebookDirectory>>> =
        OnceLock::new();
    TARGETS.get_or_init(|| Mutex::new(HashMap::new()))
}

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
enum PrimaryWorkspaceKind {
    Desktop,
    #[cfg(any(mobile, test))]
    Mobile,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredPrimaryWorkspaceState {
    #[serde(default)]
    desktop_workspace_root: Option<String>,
    #[serde(default)]
    desktop_path: Option<String>,
    #[serde(default)]
    managed_name: Option<String>,
    #[serde(default)]
    onboarding_completed: bool,
    #[serde(default)]
    onboarding_requested_for_next_launch: bool,
    #[serde(default)]
    version: u64,
}

fn completed_primary_workspace_state(
    value: Option<Value>,
) -> Result<StoredPrimaryWorkspaceState, String> {
    let state = value
        .and_then(|value| serde_json::from_value::<StoredPrimaryWorkspaceState>(value).ok())
        .filter(|state| {
            let desktop_pair =
                state.desktop_workspace_root.is_some() && state.desktop_path.is_some();
            let no_desktop_identity =
                state.desktop_workspace_root.is_none() && state.desktop_path.is_none();
            let identity_is_valid = (desktop_pair && state.managed_name.is_none())
                || (no_desktop_identity && state.managed_name.is_some())
                || (no_desktop_identity && state.managed_name.is_none());
            state.version == 3
                && state.onboarding_completed
                && !state.onboarding_requested_for_next_launch
                && identity_is_valid
        })
        .ok_or_else(sync_primary_workspace_unavailable)?;
    Ok(state)
}

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

fn read_primary_workspace_value<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<Option<Value>, String> {
    let store = app
        .store(LOCAL_STATE_STORE_PATH)
        .map_err(|_| persistence_error())?;
    let backend = StorePrimaryWorkspaceBackend { store };
    PrimaryWorkspaceService::new(&backend, transaction_lock()).read()
}

pub(crate) fn with_primary_workspace_transaction<R: tauri::Runtime, T>(
    app: &tauri::AppHandle<R>,
    operation: impl FnOnce(Result<PathBuf, String>) -> Result<T, String>,
) -> Result<T, String> {
    let store = app
        .store(LOCAL_STATE_STORE_PATH)
        .map_err(|_| persistence_error())?;
    let backend = StorePrimaryWorkspaceBackend { store };
    let service = PrimaryWorkspaceService::new(&backend, transaction_lock());

    #[cfg(mobile)]
    let app_data_root = app
        .path()
        .app_data_dir()
        .map_err(|_| sync_primary_workspace_unavailable())?;

    service.with_current(|value| {
        #[cfg(mobile)]
        let authoritative = authoritative_primary_workspace_root(
            value,
            PrimaryWorkspaceKind::Mobile,
            Some(&app_data_root),
        );
        #[cfg(not(mobile))]
        let authoritative =
            authoritative_primary_workspace_root(value, PrimaryWorkspaceKind::Desktop, None);
        operation(authoritative)
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
    with_primary_workspace_transaction(app, |authoritative| authoritative)
}

#[tauri::command]
pub(crate) fn read_primary_workspace_state(app: tauri::AppHandle) -> Result<Option<Value>, String> {
    read_primary_workspace_value(&app)
}

#[tauri::command]
pub(crate) fn write_primary_workspace_state(
    app: tauri::AppHandle,
    input: PrimaryWorkspaceWriteInput,
) -> Result<PrimaryWorkspaceWriteResult, String> {
    let store = app
        .store(LOCAL_STATE_STORE_PATH)
        .map_err(|_| persistence_error())?;
    let backend = StorePrimaryWorkspaceBackend { store };
    PrimaryWorkspaceService::new(&backend, transaction_lock()).write(input)
}

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
            mpsc, Mutex,
        },
        time::Duration,
    };

    use serde_json::json;

    use super::*;

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
}
