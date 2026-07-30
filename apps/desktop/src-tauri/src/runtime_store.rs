use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    },
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_store::{Store, StoreExt};

use crate::storage_capability::{
    create_private_replaceable_file_options, nonfollowing_read_options,
    open_canonical_directory_nofollow, rename_retained_file_in_directory, sync_directory,
    unique_regular_file_identity, UniqueRegularFileIdentity,
};

const LOCAL_STATE_STORE_PATH: &str = "local-state.json";
const DESKTOP_UI_STATE_STORE_PATH: &str = "desktop-ui-state.json";
const SETTINGS_STORE_PATH: &str = "settings.json";
const DESKTOP_UI_STATE_VERSION_KEY: &str = "desktopUiStateVersion";
const DESKTOP_UI_STATE_VERSION: u64 = 1;
const STORE_UNAVAILABLE: &str = "desktop runtime store unavailable";
const STORE_UNSUPPORTED: &str = "unsupported desktop runtime store";
const STORE_KEY_UNSUPPORTED: &str = "unsupported desktop runtime store key";
const STORE_MUTATION_UNSUPPORTED: &str = "unsupported desktop runtime store mutation";
const STORE_OPTIONS_UNSUPPORTED: &str = "unsupported desktop runtime store options";
const MAX_STORE_CHANGES: usize = 16;
const MAX_STORE_CHANGE_BYTES: usize = 16 * 1024 * 1024;

const LOCAL_STATE_KEYS: &[&str] = &[
    "fileTreeSortByWorkspace",
    "pandocPath",
    "recentMarkdownFiles",
    "schemaVersion",
    "welcomeDocumentSeen",
    "workspace",
];
const SETTINGS_READ_KEYS: &[&str] = &[
    "appearanceMode",
    "customThemeCss",
    "darkCustomThemeCss",
    "darkTheme",
    "darkThemeId",
    "editorPreferences",
    "exportSettings",
    "fileIgnoreSettings",
    "language",
    "lightCustomThemeCss",
    "lightTheme",
    "lightThemeId",
    "theme",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DesktopRuntimeStorePath {
    LocalState,
    Settings,
}

impl DesktopRuntimeStorePath {
    const fn file_name(self) -> &'static str {
        match self {
            Self::LocalState => DESKTOP_UI_STATE_STORE_PATH,
            Self::Settings => SETTINGS_STORE_PATH,
        }
    }

    fn allows_read(self, key: &str) -> bool {
        match self {
            Self::LocalState => LOCAL_STATE_KEYS.contains(&key),
            Self::Settings => SETTINGS_READ_KEYS.contains(&key),
        }
    }

    fn allows_mutation(self, key: &str) -> bool {
        self == Self::LocalState && LOCAL_STATE_KEYS.contains(&key)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct DesktopRuntimeStoreLoadOptions {
    auto_save: bool,
    defaults: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase", tag = "operation")]
pub(crate) enum DesktopRuntimeStoreChange {
    Delete { key: String },
    Set { key: String, value: Value },
}

impl DesktopRuntimeStoreChange {
    fn key(&self) -> &str {
        match self {
            Self::Delete { key } | Self::Set { key, .. } => key,
        }
    }
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DesktopRuntimeStoreValue {
    exists: bool,
    value: Option<Value>,
}

#[derive(Clone)]
struct PreviousStoreValue {
    key: String,
    value: Option<Value>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RuntimeStorePersistence {
    Durable,
    PublishedWithoutDirectoryDurability,
    NotPublished,
}

trait RuntimeStoreBackend {
    fn delete(&self, key: &str);
    fn get(&self, key: &str) -> Option<Value>;
    fn persist(&self) -> RuntimeStorePersistence;
    fn set(&self, key: String, value: Value);
}

struct TauriRuntimeStoreBackend<R: Runtime> {
    app_data_root: PathBuf,
    expected_target: Option<Option<UniqueRegularFileIdentity>>,
    store: Arc<Store<R>>,
}

impl<R: Runtime> RuntimeStoreBackend for TauriRuntimeStoreBackend<R> {
    fn delete(&self, key: &str) {
        self.store.delete(key);
    }

    fn get(&self, key: &str) -> Option<Value> {
        self.store.get(key)
    }

    fn persist(&self) -> RuntimeStorePersistence {
        let values = self.store.entries().into_iter().collect::<BTreeMap<_, _>>();
        let Ok(bytes) = serde_json::to_vec(&values) else {
            return RuntimeStorePersistence::NotPublished;
        };
        if bytes.len() > MAX_STORE_CHANGE_BYTES {
            return RuntimeStorePersistence::NotPublished;
        }
        replace_ui_state_file_atomically_with_expected(
            &self.app_data_root,
            &bytes,
            self.expected_target,
        )
        .unwrap_or(RuntimeStorePersistence::NotPublished)
    }

    fn set(&self, key: String, value: Value) {
        self.store.set(key, value);
    }
}

fn replace_ui_state_file_atomically_with_expected(
    app_data_root: &Path,
    bytes: &[u8],
    expected_target: Option<Option<UniqueRegularFileIdentity>>,
) -> Result<RuntimeStorePersistence, RuntimeStorePersistence> {
    replace_ui_state_file_atomically_with_expected_and_hooks(
        app_data_root,
        bytes,
        expected_target,
        || {},
        sync_directory,
    )
}

#[cfg(test)]
fn replace_ui_state_file_atomically_with_hooks<BeforePublish, SyncDirectory>(
    app_data_root: &Path,
    bytes: &[u8],
    before_publish: BeforePublish,
    sync_after_rename: SyncDirectory,
) -> Result<RuntimeStorePersistence, RuntimeStorePersistence>
where
    BeforePublish: FnOnce(),
    SyncDirectory: FnOnce(&cap_std::fs::Dir) -> io::Result<()>,
{
    replace_ui_state_file_atomically_with_expected_and_hooks(
        app_data_root,
        bytes,
        None,
        before_publish,
        sync_after_rename,
    )
}

fn replace_ui_state_file_atomically_with_expected_and_hooks<BeforePublish, SyncDirectory>(
    app_data_root: &Path,
    bytes: &[u8],
    expected_target: Option<Option<UniqueRegularFileIdentity>>,
    before_publish: BeforePublish,
    sync_after_rename: SyncDirectory,
) -> Result<RuntimeStorePersistence, RuntimeStorePersistence>
where
    BeforePublish: FnOnce(),
    SyncDirectory: FnOnce(&cap_std::fs::Dir) -> io::Result<()>,
{
    static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
    let directory = open_canonical_directory_nofollow(app_data_root)
        .map_err(|_| RuntimeStorePersistence::NotPublished)?;
    let existing = match directory.symlink_metadata(DESKTOP_UI_STATE_STORE_PATH) {
        Ok(metadata) => Some(
            unique_regular_file_identity(&metadata).ok_or(RuntimeStorePersistence::NotPublished)?,
        ),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(_) => return Err(RuntimeStorePersistence::NotPublished),
    };
    if expected_target.is_some_and(|expected| expected != existing) {
        return Err(RuntimeStorePersistence::NotPublished);
    }
    let (staged_name, mut staged) = (0..1000)
        .find_map(|_| {
            let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let name = format!(".desktop-ui-state-{}-{sequence}.tmp", std::process::id());
            match directory.open_with(&name, &create_private_replaceable_file_options()) {
                Ok(file) => Some(Ok((name, file))),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => None,
                Err(_) => Some(Err(RuntimeStorePersistence::NotPublished)),
            }
        })
        .unwrap_or_else(|| Err(RuntimeStorePersistence::NotPublished))?;
    if staged
        .write_all(bytes)
        .and_then(|()| staged.sync_all())
        .is_err()
    {
        drop(staged);
        let _cleanup = directory.remove_file(&staged_name);
        return Err(RuntimeStorePersistence::NotPublished);
    }
    let staged_identity = match staged
        .metadata()
        .ok()
        .and_then(|metadata| unique_regular_file_identity(&metadata))
    {
        Some(identity) => identity,
        None => {
            drop(staged);
            let _cleanup = directory.remove_file(&staged_name);
            return Err(RuntimeStorePersistence::NotPublished);
        }
    };
    before_publish();
    let retained = match directory.symlink_metadata(DESKTOP_UI_STATE_STORE_PATH) {
        Ok(metadata) => match unique_regular_file_identity(&metadata) {
            Some(identity) => Some(identity),
            None => {
                drop(staged);
                let _cleanup = directory.remove_file(&staged_name);
                return Err(RuntimeStorePersistence::NotPublished);
            }
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(_) => {
            drop(staged);
            let _cleanup = directory.remove_file(&staged_name);
            return Err(RuntimeStorePersistence::NotPublished);
        }
    };
    if retained != existing {
        drop(staged);
        let _cleanup = directory.remove_file(&staged_name);
        return Err(RuntimeStorePersistence::NotPublished);
    }
    if rename_retained_file_in_directory(
        &directory,
        &staged,
        &staged_name,
        staged_identity,
        DESKTOP_UI_STATE_STORE_PATH,
        existing.is_some(),
    )
    .is_err()
    {
        drop(staged);
        let _cleanup = directory.remove_file(&staged_name);
        return Err(RuntimeStorePersistence::NotPublished);
    }
    drop(staged);
    Ok(match sync_after_rename(&directory) {
        Ok(()) => RuntimeStorePersistence::Durable,
        Err(_) => RuntimeStorePersistence::PublishedWithoutDirectoryDurability,
    })
}

fn desktop_ui_state_transaction_gate() -> &'static Mutex<()> {
    static GATE: OnceLock<Mutex<()>> = OnceLock::new();
    GATE.get_or_init(|| Mutex::new(()))
}

fn read_store_value_with_gate(
    gate: &Mutex<()>,
    read: impl FnOnce() -> Option<Value>,
) -> Result<DesktopRuntimeStoreValue, String> {
    let _transaction = gate.lock().map_err(|_| STORE_UNAVAILABLE.to_string())?;
    let value = read();
    Ok(DesktopRuntimeStoreValue {
        exists: value.is_some(),
        value,
    })
}

struct ExistingDesktopUiState {
    identity: UniqueRegularFileIdentity,
    values: BTreeMap<String, Value>,
}

fn read_existing_ui_state(app_data_root: &Path) -> Result<Option<ExistingDesktopUiState>, String> {
    let directory = open_canonical_directory_nofollow(app_data_root)
        .map_err(|_| STORE_UNAVAILABLE.to_string())?;
    let addressed = match directory.symlink_metadata(DESKTOP_UI_STATE_STORE_PATH) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(STORE_UNAVAILABLE.to_string()),
    };
    if addressed.len() > MAX_STORE_CHANGE_BYTES as u64 {
        return Err(STORE_UNAVAILABLE.to_string());
    }
    let identity =
        unique_regular_file_identity(&addressed).ok_or_else(|| STORE_UNAVAILABLE.to_string())?;
    let mut retained = directory
        .open_with(DESKTOP_UI_STATE_STORE_PATH, &nonfollowing_read_options())
        .map_err(|_| STORE_UNAVAILABLE.to_string())?;
    let retained_identity = retained
        .metadata()
        .ok()
        .and_then(|metadata| unique_regular_file_identity(&metadata))
        .ok_or_else(|| STORE_UNAVAILABLE.to_string())?;
    if retained_identity != identity {
        return Err(STORE_UNAVAILABLE.to_string());
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut retained)
        .take(MAX_STORE_CHANGE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| STORE_UNAVAILABLE.to_string())?;
    if bytes.len() > MAX_STORE_CHANGE_BYTES {
        return Err(STORE_UNAVAILABLE.to_string());
    }
    let rechecked = directory
        .symlink_metadata(DESKTOP_UI_STATE_STORE_PATH)
        .ok()
        .and_then(|metadata| unique_regular_file_identity(&metadata));
    if rechecked != Some(identity) {
        return Err(STORE_UNAVAILABLE.to_string());
    }
    let values = serde_json::from_slice::<BTreeMap<String, Value>>(&bytes)
        .map_err(|_| STORE_UNAVAILABLE.to_string())?;
    match values.get(DESKTOP_UI_STATE_VERSION_KEY) {
        None => {}
        Some(Value::Number(version)) if version.as_u64() == Some(DESKTOP_UI_STATE_VERSION) => {}
        Some(_) => return Err(STORE_UNAVAILABLE.to_string()),
    }
    Ok(Some(ExistingDesktopUiState { identity, values }))
}

fn restore_store_snapshot<R: Runtime>(store: &Store<R>, previous: &BTreeMap<String, Value>) {
    for key in store.keys() {
        if !previous.contains_key(&key) {
            store.delete(key);
        }
    }
    for (key, value) in previous {
        store.set(key.clone(), value.clone());
    }
}

pub(crate) fn install_desktop_runtime_store<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let app_data_root = app
        .path()
        .app_data_dir()
        .map_err(|_| STORE_UNAVAILABLE.to_string())?;
    std::fs::create_dir_all(&app_data_root).map_err(|_| STORE_UNAVAILABLE.to_string())?;

    let existing_ui_state = read_existing_ui_state(&app_data_root)?;
    let legacy = app
        .store_builder(LOCAL_STATE_STORE_PATH)
        .disable_auto_save()
        .build()
        .map_err(|_| STORE_UNAVAILABLE.to_string())?;
    let store = app
        .store_builder(DESKTOP_UI_STATE_STORE_PATH)
        .disable_auto_save()
        .create_new()
        .build()
        .map_err(|_| STORE_UNAVAILABLE.to_string())?;
    if let Some(existing) = &existing_ui_state {
        for (key, value) in &existing.values {
            store.set(key.clone(), value.clone());
        }
    }
    let _transaction = desktop_ui_state_transaction_gate()
        .lock()
        .map_err(|_| STORE_UNAVAILABLE.to_string())?;
    if store.get(DESKTOP_UI_STATE_VERSION_KEY) == Some(Value::from(DESKTOP_UI_STATE_VERSION)) {
        return Ok(());
    }

    let previous = store.entries().into_iter().collect::<BTreeMap<_, _>>();
    for key in LOCAL_STATE_KEYS {
        if store.get(key).is_none() {
            if let Some(value) = legacy.get(key) {
                store.set((*key).to_string(), value);
            }
        }
    }
    store.set(
        DESKTOP_UI_STATE_VERSION_KEY.to_string(),
        Value::from(DESKTOP_UI_STATE_VERSION),
    );
    let backend = TauriRuntimeStoreBackend {
        app_data_root,
        expected_target: Some(existing_ui_state.as_ref().map(|state| state.identity)),
        store: store.clone(),
    };
    match backend.persist() {
        RuntimeStorePersistence::Durable => Ok(()),
        RuntimeStorePersistence::PublishedWithoutDirectoryDurability => {
            Err(STORE_UNAVAILABLE.to_string())
        }
        RuntimeStorePersistence::NotPublished => {
            restore_store_snapshot(store.as_ref(), &previous);
            Err(STORE_UNAVAILABLE.to_string())
        }
    }
}

fn supported_store_path(path: &str) -> Result<DesktopRuntimeStorePath, String> {
    match path {
        LOCAL_STATE_STORE_PATH => Ok(DesktopRuntimeStorePath::LocalState),
        SETTINGS_STORE_PATH => Ok(DesktopRuntimeStorePath::Settings),
        _ => Err(STORE_UNSUPPORTED.to_string()),
    }
}

fn validate_load_options(options: &DesktopRuntimeStoreLoadOptions) -> Result<(), String> {
    if options.auto_save || !options.defaults.is_empty() {
        Err(STORE_OPTIONS_UNSUPPORTED.to_string())
    } else {
        Ok(())
    }
}

fn validate_read_key(path: DesktopRuntimeStorePath, key: &str) -> Result<(), String> {
    if path.allows_read(key) {
        Ok(())
    } else {
        Err(STORE_KEY_UNSUPPORTED.to_string())
    }
}

fn validate_changes(
    path: DesktopRuntimeStorePath,
    changes: &[DesktopRuntimeStoreChange],
) -> Result<(), String> {
    if changes.is_empty() || changes.len() > MAX_STORE_CHANGES {
        return Err(STORE_MUTATION_UNSUPPORTED.to_string());
    }
    let encoded =
        serde_json::to_vec(changes).map_err(|_| STORE_MUTATION_UNSUPPORTED.to_string())?;
    if encoded.len() > MAX_STORE_CHANGE_BYTES {
        return Err(STORE_MUTATION_UNSUPPORTED.to_string());
    }
    let mut keys = BTreeSet::new();
    for change in changes {
        if !path.allows_mutation(change.key()) || !keys.insert(change.key()) {
            return Err(STORE_MUTATION_UNSUPPORTED.to_string());
        }
    }
    Ok(())
}

fn runtime_store<R: Runtime>(
    app: &AppHandle<R>,
    path: DesktopRuntimeStorePath,
) -> Result<Arc<Store<R>>, String> {
    app.get_store(path.file_name())
        .ok_or_else(|| STORE_UNAVAILABLE.to_string())
}

fn apply_changes(
    store: &dyn RuntimeStoreBackend,
    changes: &[DesktopRuntimeStoreChange],
) -> Result<(), String> {
    let previous = changes
        .iter()
        .map(|change| PreviousStoreValue {
            key: change.key().to_string(),
            value: store.get(change.key()),
        })
        .collect::<Vec<_>>();
    for change in changes {
        match change {
            DesktopRuntimeStoreChange::Delete { key } => {
                store.delete(key);
            }
            DesktopRuntimeStoreChange::Set { key, value } => {
                store.set(key.clone(), value.clone());
            }
        }
    }
    match store.persist() {
        RuntimeStorePersistence::Durable => Ok(()),
        RuntimeStorePersistence::PublishedWithoutDirectoryDurability => {
            Err(STORE_UNAVAILABLE.to_string())
        }
        RuntimeStorePersistence::NotPublished => {
            for previous in previous {
                match previous.value {
                    Some(value) => store.set(previous.key, value),
                    None => store.delete(&previous.key),
                }
            }
            Err(STORE_UNAVAILABLE.to_string())
        }
    }
}

#[tauri::command]
pub(crate) fn load_desktop_runtime_store(
    app: AppHandle,
    path: String,
    options: DesktopRuntimeStoreLoadOptions,
) -> Result<(), String> {
    validate_load_options(&options)?;
    runtime_store(&app, supported_store_path(&path)?).map(|_| ())
}

#[tauri::command]
pub(crate) fn get_desktop_runtime_store_value(
    app: AppHandle,
    path: String,
    key: String,
) -> Result<DesktopRuntimeStoreValue, String> {
    let path = supported_store_path(&path)?;
    validate_read_key(path, &key)?;
    let store = runtime_store(&app, path)?;
    match path {
        DesktopRuntimeStorePath::LocalState => {
            read_store_value_with_gate(desktop_ui_state_transaction_gate(), || store.get(&key))
        }
        DesktopRuntimeStorePath::Settings => {
            let gate = crate::app_settings::app_settings_transaction_gate();
            read_store_value_with_gate(gate.as_ref(), || store.get(&key))
        }
    }
}

#[tauri::command]
pub(crate) fn commit_desktop_runtime_store_changes(
    app: AppHandle,
    path: String,
    changes: Vec<DesktopRuntimeStoreChange>,
) -> Result<(), String> {
    let path = supported_store_path(&path)?;
    validate_changes(path, &changes)?;
    let store = runtime_store(&app, path)?;
    match path {
        DesktopRuntimeStorePath::LocalState => {
            let _transaction = desktop_ui_state_transaction_gate()
                .lock()
                .map_err(|_| STORE_UNAVAILABLE.to_string())?;
            let app_data_root = app
                .path()
                .app_data_dir()
                .map_err(|_| STORE_UNAVAILABLE.to_string())?;
            apply_changes(
                &TauriRuntimeStoreBackend {
                    app_data_root,
                    expected_target: None,
                    store,
                },
                &changes,
            )
        }
        DesktopRuntimeStorePath::Settings => Err(STORE_MUTATION_UNSUPPORTED.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{mpsc, Mutex},
        time::Duration,
    };

    use super::*;

    struct MemoryStore {
        publication: RuntimeStorePersistence,
        values: Mutex<BTreeMap<String, Value>>,
    }

    impl MemoryStore {
        fn new(values: BTreeMap<String, Value>, fail_persist: bool) -> Self {
            Self {
                publication: if fail_persist {
                    RuntimeStorePersistence::NotPublished
                } else {
                    RuntimeStorePersistence::Durable
                },
                values: Mutex::new(values),
            }
        }

        fn with_publication(
            values: BTreeMap<String, Value>,
            publication: RuntimeStorePersistence,
        ) -> Self {
            Self {
                publication,
                values: Mutex::new(values),
            }
        }
    }

    impl RuntimeStoreBackend for MemoryStore {
        fn delete(&self, key: &str) {
            self.values.lock().expect("memory store lock").remove(key);
        }

        fn get(&self, key: &str) -> Option<Value> {
            self.values
                .lock()
                .expect("memory store lock")
                .get(key)
                .cloned()
        }

        fn persist(&self) -> RuntimeStorePersistence {
            self.publication
        }

        fn set(&self, key: String, value: Value) {
            self.values
                .lock()
                .expect("memory store lock")
                .insert(key, value);
        }
    }

    fn options(
        auto_save: bool,
        defaults: BTreeMap<String, Value>,
    ) -> DesktopRuntimeStoreLoadOptions {
        DesktopRuntimeStoreLoadOptions {
            auto_save,
            defaults,
        }
    }

    fn set(key: &str) -> DesktopRuntimeStoreChange {
        DesktopRuntimeStoreChange::Set {
            key: key.to_string(),
            value: Value::Bool(true),
        }
    }

    #[test]
    fn fixed_store_paths_exclude_host_private_state_and_path_syntax() {
        assert_eq!(
            supported_store_path(LOCAL_STATE_STORE_PATH),
            Ok(DesktopRuntimeStorePath::LocalState)
        );
        assert_eq!(
            supported_store_path(SETTINGS_STORE_PATH),
            Ok(DesktopRuntimeStorePath::Settings)
        );
        assert_eq!(
            DesktopRuntimeStorePath::LocalState.file_name(),
            DESKTOP_UI_STATE_STORE_PATH
        );
        assert_ne!(
            DesktopRuntimeStorePath::LocalState.file_name(),
            LOCAL_STATE_STORE_PATH
        );
        for path in [
            "../local-state.json",
            "/tmp/local-state.json",
            "desktop-ui-state.json",
            "native-host-workspaces.bin",
            "other.json",
        ] {
            assert_eq!(
                supported_store_path(path),
                Err(STORE_UNSUPPORTED.to_string())
            );
        }
    }

    #[test]
    fn fixed_store_load_rejects_plugin_expansion_options() {
        assert!(validate_load_options(&options(false, BTreeMap::new())).is_ok());
        assert_eq!(
            validate_load_options(&options(true, BTreeMap::new())),
            Err(STORE_OPTIONS_UNSUPPORTED.to_string())
        );
        assert_eq!(
            validate_load_options(&options(
                false,
                BTreeMap::from([("injected".to_string(), Value::Bool(true))]),
            )),
            Err(STORE_OPTIONS_UNSUPPORTED.to_string())
        );
    }

    #[test]
    fn key_policy_excludes_primary_mcp_and_native_host_state() {
        for key in ["primaryWorkspace", "mcp", "nativeHostWorkspaceStates"] {
            assert_eq!(
                validate_read_key(DesktopRuntimeStorePath::LocalState, key),
                Err(STORE_KEY_UNSUPPORTED.to_string())
            );
        }
        assert_eq!(
            validate_read_key(DesktopRuntimeStorePath::Settings, "mcp"),
            Err(STORE_KEY_UNSUPPORTED.to_string())
        );
        assert!(validate_read_key(DesktopRuntimeStorePath::LocalState, "workspace").is_ok());
        assert!(validate_read_key(DesktopRuntimeStorePath::Settings, "theme").is_ok());
    }

    #[test]
    fn mutation_policy_is_local_ui_only_and_bounded() {
        assert!(validate_changes(DesktopRuntimeStorePath::LocalState, &[set("workspace")]).is_ok());
        for changes in [
            vec![set("primaryWorkspace")],
            vec![set("mcp")],
            vec![set("nativeHostWorkspaceStates")],
        ] {
            assert_eq!(
                validate_changes(DesktopRuntimeStorePath::LocalState, &changes),
                Err(STORE_MUTATION_UNSUPPORTED.to_string())
            );
        }
        assert_eq!(
            validate_changes(DesktopRuntimeStorePath::Settings, &[set("theme")]),
            Err(STORE_MUTATION_UNSUPPORTED.to_string())
        );
        assert_eq!(
            validate_changes(
                DesktopRuntimeStorePath::LocalState,
                &[set("workspace"), set("workspace")],
            ),
            Err(STORE_MUTATION_UNSUPPORTED.to_string())
        );
        assert_eq!(
            validate_changes(DesktopRuntimeStorePath::LocalState, &[]),
            Err(STORE_MUTATION_UNSUPPORTED.to_string())
        );
        assert_eq!(
            validate_changes(
                DesktopRuntimeStorePath::LocalState,
                &vec![set("workspace"); MAX_STORE_CHANGES + 1],
            ),
            Err(STORE_MUTATION_UNSUPPORTED.to_string())
        );
        assert_eq!(
            validate_changes(
                DesktopRuntimeStorePath::LocalState,
                &[DesktopRuntimeStoreChange::Set {
                    key: "workspace".to_string(),
                    value: Value::String("x".repeat(MAX_STORE_CHANGE_BYTES)),
                }],
            ),
            Err(STORE_MUTATION_UNSUPPORTED.to_string())
        );
    }

    #[test]
    fn commit_applies_the_complete_change_set() {
        let store = MemoryStore::new(
            BTreeMap::from([
                ("workspace".to_string(), Value::String("old".to_string())),
                ("recentMarkdownFiles".to_string(), Value::Array(Vec::new())),
            ]),
            false,
        );
        apply_changes(
            &store,
            &[
                DesktopRuntimeStoreChange::Set {
                    key: "workspace".to_string(),
                    value: Value::String("new".to_string()),
                },
                DesktopRuntimeStoreChange::Delete {
                    key: "recentMarkdownFiles".to_string(),
                },
            ],
        )
        .expect("change set should persist");

        assert_eq!(
            store.get("workspace"),
            Some(Value::String("new".to_string()))
        );
        assert_eq!(store.get("recentMarkdownFiles"), None);
    }

    #[test]
    fn failed_commit_restores_the_complete_in_memory_snapshot() {
        let store = MemoryStore::new(
            BTreeMap::from([
                ("workspace".to_string(), Value::Null),
                ("recentMarkdownFiles".to_string(), Value::Array(Vec::new())),
            ]),
            true,
        );
        let result = apply_changes(
            &store,
            &[
                DesktopRuntimeStoreChange::Set {
                    key: "workspace".to_string(),
                    value: Value::String("new".to_string()),
                },
                DesktopRuntimeStoreChange::Delete {
                    key: "recentMarkdownFiles".to_string(),
                },
                DesktopRuntimeStoreChange::Set {
                    key: "welcomeDocumentSeen".to_string(),
                    value: Value::Bool(true),
                },
            ],
        );

        assert_eq!(result, Err(STORE_UNAVAILABLE.to_string()));
        assert_eq!(store.get("workspace"), Some(Value::Null));
        assert_eq!(
            store.get("recentMarkdownFiles"),
            Some(Value::Array(Vec::new()))
        );
        assert_eq!(store.get("welcomeDocumentSeen"), None);
    }

    #[test]
    fn published_but_not_directory_durable_commit_keeps_the_published_snapshot() {
        let store = MemoryStore::with_publication(
            BTreeMap::from([("workspace".to_string(), Value::String("old".to_string()))]),
            RuntimeStorePersistence::PublishedWithoutDirectoryDurability,
        );

        let result = apply_changes(
            &store,
            &[DesktopRuntimeStoreChange::Set {
                key: "workspace".to_string(),
                value: Value::String("new".to_string()),
            }],
        );

        assert_eq!(result, Err(STORE_UNAVAILABLE.to_string()));
        assert_eq!(
            store.get("workspace"),
            Some(Value::String("new".to_string()))
        );
    }

    #[test]
    fn atomic_ui_state_replacement_writes_complete_json() {
        let app_data = tempfile::tempdir().expect("temporary app data");
        let root = app_data.path().canonicalize().expect("canonical app data");
        let desired = br#"{"schemaVersion":2,"workspace":"/Notes"}"#;

        let publication =
            replace_ui_state_file_atomically_with_hooks(&root, desired, || {}, sync_directory)
                .expect("atomic replacement should succeed");

        assert_eq!(publication, RuntimeStorePersistence::Durable);
        assert_eq!(
            std::fs::read(root.join(DESKTOP_UI_STATE_STORE_PATH)).expect("published UI state"),
            desired
        );
    }

    #[test]
    fn atomic_ui_state_replacement_rejects_target_swap_before_publish() {
        let app_data = tempfile::tempdir().expect("temporary app data");
        let root = app_data.path().canonicalize().expect("canonical app data");
        let target = root.join(DESKTOP_UI_STATE_STORE_PATH);
        std::fs::write(&target, br#"{"workspace":"old"}"#).expect("seed UI state");

        let result = replace_ui_state_file_atomically_with_hooks(
            &root,
            br#"{"workspace":"new"}"#,
            || {
                std::fs::rename(&target, root.join("captured-ui-state"))
                    .expect("capture original target");
                std::fs::write(&target, br#"{"workspace":"attacker"}"#)
                    .expect("replace addressed target");
            },
            sync_directory,
        );

        assert_eq!(result, Err(RuntimeStorePersistence::NotPublished));
        assert_eq!(
            std::fs::read(target).expect("attacker target remains"),
            br#"{"workspace":"attacker"}"#
        );
    }

    #[test]
    fn atomic_ui_state_reports_post_publish_directory_sync_failure() {
        let app_data = tempfile::tempdir().expect("temporary app data");
        let root = app_data.path().canonicalize().expect("canonical app data");
        let desired = br#"{"workspace":"new"}"#;

        let publication = replace_ui_state_file_atomically_with_hooks(
            &root,
            desired,
            || {},
            |_| Err(std::io::Error::other("injected directory sync failure")),
        )
        .expect("rename should already be published");

        assert_eq!(
            publication,
            RuntimeStorePersistence::PublishedWithoutDirectoryDurability
        );
        assert_eq!(
            std::fs::read(root.join(DESKTOP_UI_STATE_STORE_PATH)).expect("published UI state"),
            desired
        );
    }

    #[test]
    fn atomic_ui_state_replacement_rejects_a_target_changed_since_secure_load() {
        let app_data = tempfile::tempdir().expect("temporary app data");
        let root = app_data.path().canonicalize().expect("canonical app data");
        let target = root.join(DESKTOP_UI_STATE_STORE_PATH);
        std::fs::write(&target, br#"{"workspace":"old"}"#).expect("seed UI state");
        let loaded = read_existing_ui_state(&root)
            .expect("secure state read")
            .expect("existing state");
        std::fs::rename(&target, root.join("captured-ui-state")).expect("capture loaded target");
        let newer = br#"{"workspace":"newer"}"#;
        std::fs::write(&target, newer).expect("write newer target");

        let result = replace_ui_state_file_atomically_with_expected(
            &root,
            br#"{"workspace":"stale"}"#,
            Some(Some(loaded.identity)),
        );

        assert_eq!(result, Err(RuntimeStorePersistence::NotPublished));
        assert_eq!(std::fs::read(target).expect("newer state remains"), newer);
    }

    #[test]
    fn runtime_store_reads_wait_for_the_store_transaction_gate() {
        let gate = Arc::new(Mutex::new(()));
        let held = gate.lock().expect("hold transaction gate");
        let (started_sender, started_receiver) = mpsc::channel();
        let (completed_sender, completed_receiver) = mpsc::channel();
        let reader_gate = gate.clone();
        let reader = std::thread::spawn(move || {
            started_sender.send(()).expect("reader started");
            completed_sender
                .send(read_store_value_with_gate(reader_gate.as_ref(), || {
                    Some(Value::Null)
                }))
                .expect("reader completed");
        });

        started_receiver.recv().expect("reader reached gate");
        assert!(completed_receiver
            .recv_timeout(Duration::from_millis(100))
            .is_err());
        drop(held);
        assert_eq!(
            completed_receiver
                .recv()
                .expect("read result")
                .expect("read succeeds"),
            DesktopRuntimeStoreValue {
                exists: true,
                value: Some(Value::Null)
            }
        );
        reader.join().expect("join reader");
    }

    #[test]
    fn existing_ui_state_validation_rejects_malformed_and_future_versions() {
        let app_data = tempfile::tempdir().expect("temporary app data");
        let root = app_data.path().canonicalize().expect("canonical app data");
        let target = root.join(DESKTOP_UI_STATE_STORE_PATH);

        std::fs::write(&target, b"not-json").expect("write malformed state");
        assert!(matches!(
            read_existing_ui_state(&root),
            Err(error) if error == STORE_UNAVAILABLE
        ));

        std::fs::write(
            &target,
            serde_json::to_vec(&BTreeMap::from([(
                DESKTOP_UI_STATE_VERSION_KEY.to_string(),
                Value::from(DESKTOP_UI_STATE_VERSION + 1),
            )]))
            .expect("serialize future state"),
        )
        .expect("write future state");
        assert!(matches!(
            read_existing_ui_state(&root),
            Err(error) if error == STORE_UNAVAILABLE
        ));
    }

    #[test]
    fn existing_ui_state_validation_preserves_null_and_missing_version_for_migration() {
        let app_data = tempfile::tempdir().expect("temporary app data");
        let root = app_data.path().canonicalize().expect("canonical app data");
        let target = root.join(DESKTOP_UI_STATE_STORE_PATH);
        std::fs::write(&target, br#"{"workspace":null}"#).expect("write legacy UI state");

        assert_eq!(
            read_existing_ui_state(&root)
                .expect("valid unversioned state")
                .expect("existing state")
                .values,
            BTreeMap::from([("workspace".to_string(), Value::Null)])
        );
    }
}
