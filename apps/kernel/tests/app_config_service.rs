use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

use qingyu_kernel::{
    app_config::{AppConfigService, AppConfigServiceError, AppConfigServiceErrorKind},
    config::KernelConfig,
    contract::{
        AppConfigSnapshotDto, DomainEvent, FileTreeSortKey, PatchAppConfigStateRequest,
        ResourceRefDto, StoredWorkspaceWindowStateDto, WorkspaceGeneration, WorkspaceId,
    },
    events::{EventPublication, EventSink, EventSinkError},
    paths::KernelPaths,
    settings::{
        service::{SettingsRuntimeCoordinator, SettingsService},
        storage::{AtomicJsonSettingsStore, SettingsStore, SettingsStoreError},
    },
    storage::DurableFileStore,
};
use serde_json::{json, Map, Value};
use tempfile::{tempdir, TempDir};
use uuid::Uuid;

const UNAVAILABLE: u8 = 1;
const PUBLISH_UNCERTAIN: u8 = 2;

#[derive(Default)]
struct MemorySettingsStore {
    values: Mutex<BTreeMap<String, Value>>,
    fail_get: AtomicBool,
    gets: AtomicUsize,
    save_failure: AtomicU8,
    saves: AtomicUsize,
}

impl MemorySettingsStore {
    fn snapshot(&self) -> BTreeMap<String, Value> {
        self.values.lock().unwrap().clone()
    }

    fn saves(&self) -> usize {
        self.saves.load(Ordering::Relaxed)
    }

    fn fail_next_save(&self, failure: u8) {
        self.save_failure.store(failure, Ordering::Relaxed);
    }

    fn fail_reads(&self) {
        self.fail_get.store(true, Ordering::Relaxed);
    }

    fn gets(&self) -> usize {
        self.gets.load(Ordering::Relaxed)
    }
}

impl SettingsStore for MemorySettingsStore {
    fn get(&self, key: &str) -> Result<Option<Value>, SettingsStoreError> {
        self.gets.fetch_add(1, Ordering::Relaxed);
        if self.fail_get.load(Ordering::Relaxed) {
            return Err(SettingsStoreError::unavailable());
        }
        Ok(self.values.lock().unwrap().get(key).cloned())
    }

    fn set(&self, key: &str, value: Value) -> Result<(), SettingsStoreError> {
        self.values.lock().unwrap().insert(key.to_string(), value);
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), SettingsStoreError> {
        self.values.lock().unwrap().remove(key);
        Ok(())
    }

    fn save(&self) -> Result<(), SettingsStoreError> {
        self.saves.fetch_add(1, Ordering::Relaxed);
        match self.save_failure.swap(0, Ordering::Relaxed) {
            UNAVAILABLE => Err(SettingsStoreError::unavailable()),
            PUBLISH_UNCERTAIN => Err(SettingsStoreError::publish_uncertain()),
            _ => Ok(()),
        }
    }

    fn replace_portable_atomically(
        &self,
        desired: &Map<String, Value>,
    ) -> Result<(), SettingsStoreError> {
        let mut values = self.values.lock().unwrap();
        for key in qingyu_kernel::settings::model::PORTABLE_SETTINGS_KEYS {
            values.remove(key);
        }
        values.extend(
            desired
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
        Ok(())
    }
}

#[derive(Default)]
struct RecordingEvents {
    publications: Mutex<Vec<EventPublication>>,
}

impl RecordingEvents {
    fn snapshot(&self) -> Vec<EventPublication> {
        self.publications.lock().unwrap().clone()
    }
}

impl EventSink for RecordingEvents {
    fn publish(&self, publication: &EventPublication) -> Result<(), EventSinkError> {
        self.publications.lock().unwrap().push(publication.clone());
        Ok(())
    }
}

struct Fixture {
    app_config: AppConfigService,
    generation: WorkspaceGeneration,
    store: Arc<MemorySettingsStore>,
    settings: Arc<SettingsService>,
    events: Arc<RecordingEvents>,
}

impl Fixture {
    fn new(workspace_seed: u128, generation: &str) -> Self {
        Self::with_store(
            Arc::new(MemorySettingsStore::default()),
            workspace_seed,
            generation,
        )
    }

    fn with_store(store: Arc<MemorySettingsStore>, workspace_seed: u128, generation: &str) -> Self {
        let events = Arc::new(RecordingEvents::default());
        let coordinator = Arc::new(SettingsRuntimeCoordinator::new(events.clone()));
        let settings = Arc::new(SettingsService::with_coordinator(
            store.clone(),
            coordinator.clone(),
        ));
        let generation = WorkspaceGeneration::parse(generation).unwrap();
        let app_config = AppConfigService::new(
            store.clone(),
            settings.clone(),
            coordinator,
            WorkspaceId::new(Uuid::from_u128(workspace_seed)),
            generation.clone(),
            events.clone(),
        );
        Self {
            app_config,
            generation,
            store,
            settings,
            events,
        }
    }

    fn request(&self, generation: &str, operations: Value) -> PatchAppConfigStateRequest {
        serde_json::from_value(json!({
            "workspaceGeneration": generation,
            "operations": operations,
        }))
        .unwrap()
    }

    fn patch(&self, operations: Value) -> Result<AppConfigSnapshotDto, AppConfigServiceError> {
        self.app_config
            .patch_state(self.request(self.generation.as_str(), operations))
    }
}

fn patch_window(path: &str) -> Value {
    json!([{
        "type": "patch-ui-layout",
        "windowLabel": "main",
        "patch": { "filePath": path }
    }])
}

#[test]
fn absent_app_config_returns_defaults_for_the_active_workspace() {
    let fixture = Fixture::new(1, "generation-a");
    let before = fixture.store.snapshot();
    let snapshot = fixture.app_config.read().unwrap();

    assert_eq!(snapshot.app_config_version, 1);
    assert_eq!(snapshot.workspace.id.as_uuid(), &Uuid::from_u128(1));
    assert_eq!(snapshot.workspace.generation.as_str(), "generation-a");
    assert!(snapshot.local_state.ui_layout.window_states.is_empty());
    assert!(snapshot.local_state.ui_layout.open_windows.is_empty());
    assert!(snapshot.local_state.recent_markdown_files.is_empty());
    assert_eq!(
        snapshot.local_state.file_tree_sort.key,
        FileTreeSortKey::Name
    );
    assert_eq!(fixture.store.snapshot(), before);
    assert_eq!(fixture.store.saves(), 0);
}

#[test]
fn semantic_operations_merge_without_replacing_unmentioned_fields() {
    let fixture = Fixture::new(1, "generation-a");
    fixture
        .patch(json!([{
            "type": "patch-ui-layout",
            "windowLabel": "main",
            "patch": { "fileTreeOpen": true }
        }]))
        .unwrap();
    fixture.patch(patch_window("notes/a.md")).unwrap();

    let main = fixture
        .app_config
        .read()
        .unwrap()
        .local_state
        .ui_layout
        .window_states
        .get("main")
        .unwrap()
        .clone();
    assert!(main.file_tree_open);
    assert_eq!(main.file_path.as_ref().unwrap().as_str(), "notes/a.md");
}

#[test]
fn stale_workspace_generation_rejects_without_mutating_the_store() {
    let fixture = Fixture::new(1, "generation-a");
    let before = fixture.store.snapshot();

    let error = fixture
        .app_config
        .patch_state(fixture.request(
            "generation-old",
            json!([{ "type": "set-pandoc-path", "path": "/opt/pandoc" }]),
        ))
        .unwrap_err();

    assert_eq!(
        error.kind(),
        AppConfigServiceErrorKind::StaleWorkspaceGeneration
    );
    assert_eq!(fixture.store.snapshot(), before);
    assert_eq!(fixture.store.saves(), 0);
    assert!(fixture.events.snapshot().is_empty());
}

#[test]
fn app_config_is_partitioned_by_canonical_workspace_id() {
    let store = Arc::new(MemorySettingsStore::default());
    let first = Fixture::with_store(store.clone(), 1, "generation-a");
    first.patch(patch_window("notes/a.md")).unwrap();
    let second = Fixture::with_store(store, 2, "generation-b");

    assert_eq!(
        first
            .app_config
            .read()
            .unwrap()
            .local_state
            .ui_layout
            .window_states["main"]
            .file_path
            .as_ref()
            .unwrap()
            .as_str(),
        "notes/a.md"
    );
    assert!(second
        .app_config
        .read()
        .unwrap()
        .local_state
        .ui_layout
        .window_states
        .is_empty());
    assert!(first.store.snapshot()["uiLayout"]
        .as_object()
        .unwrap()
        .contains_key(&Uuid::from_u128(1).to_string()));
}

fn physical_store() -> (TempDir, KernelConfig, AtomicJsonSettingsStore) {
    let root = tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let app_data = root.path().join("app-data");
    let cache = root.path().join("cache");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&app_data).unwrap();
    std::fs::create_dir(&cache).unwrap();
    let paths = KernelPaths::desktop(&workspace, &app_data, &cache).unwrap();
    let config = KernelConfig::generate().unwrap();
    let durable = DurableFileStore::at_config(paths.config_root(), config.launch_epoch()).unwrap();
    let store = AtomicJsonSettingsStore::new(durable).unwrap();
    (root, config, store)
}

fn reopen_physical_store(root: &TempDir, config: &KernelConfig) -> AtomicJsonSettingsStore {
    let paths = KernelPaths::desktop(
        &root.path().join("workspace"),
        &root.path().join("app-data"),
        &root.path().join("cache"),
    )
    .unwrap();
    let durable = DurableFileStore::at_config(paths.config_root(), config.launch_epoch()).unwrap();
    AtomicJsonSettingsStore::new(durable).unwrap()
}

#[test]
fn document_size_limit_is_enforced_on_save_and_portable_replace_across_reopen() {
    const MIB: usize = 1024 * 1024;

    let (root, config, store) = physical_store();
    store
        .set("oversizedLocal", json!("x".repeat(65 * MIB)))
        .unwrap();
    assert!(store.save().is_err());
    assert!(!root.path().join("app-data/settings.json").exists());
    drop(store);
    drop(reopen_physical_store(&root, &config));

    let (root, config, store) = physical_store();
    store
        .set("largeLocal", json!("x".repeat(63 * MIB)))
        .unwrap();
    store.save().unwrap();
    let path = root.path().join("app-data/settings.json");
    let before = std::fs::read(&path).unwrap();
    let replacement = json!({ "language": "y".repeat(2 * MIB) });
    assert!(store
        .replace_portable_atomically(replacement.as_object().unwrap())
        .is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    drop(store);
    let reopened = reopen_physical_store(&root, &config);
    assert_eq!(
        reopened.get("largeLocal").unwrap(),
        Some(json!("x".repeat(63 * MIB)))
    );
    assert_eq!(reopened.get("language").unwrap(), None);
}

#[test]
fn absent_physical_document_reads_defaults_without_creating_settings_json() {
    let (root, _config, store) = physical_store();
    let store = Arc::new(store);
    let events = Arc::new(RecordingEvents::default());
    let coordinator = Arc::new(SettingsRuntimeCoordinator::new(events.clone()));
    let settings = Arc::new(SettingsService::with_coordinator(
        store.clone(),
        coordinator.clone(),
    ));
    let app_config = AppConfigService::new(
        store,
        settings,
        coordinator,
        WorkspaceId::new(Uuid::from_u128(1)),
        WorkspaceGeneration::parse("generation-a").unwrap(),
        events,
    );

    assert_eq!(app_config.read().unwrap().app_config_version, 1);
    assert!(!root.path().join("app-data/settings.json").exists());
}

#[test]
fn present_document_requires_exact_app_config_version() {
    for document in [
        json!({}),
        json!({ "appConfigVersion": "1" }),
        json!({ "appConfigVersion": 0 }),
        json!({ "appConfigVersion": 2 }),
    ] {
        let root = tempdir().unwrap();
        let workspace = root.path().join("workspace");
        let app_data = root.path().join("app-data");
        let cache = root.path().join("cache");
        std::fs::create_dir(&workspace).unwrap();
        std::fs::create_dir(&app_data).unwrap();
        std::fs::create_dir(&cache).unwrap();
        std::fs::write(
            app_data.join("settings.json"),
            serde_json::to_vec(&document).unwrap(),
        )
        .unwrap();
        let paths = KernelPaths::desktop(&workspace, &app_data, &cache).unwrap();
        let config = KernelConfig::generate().unwrap();
        let durable =
            DurableFileStore::at_config(paths.config_root(), config.launch_epoch()).unwrap();
        assert!(AtomicJsonSettingsStore::new(durable).is_err(), "{document}");
    }

    let root = tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let app_data = root.path().join("app-data");
    let cache = root.path().join("cache");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&app_data).unwrap();
    std::fs::create_dir(&cache).unwrap();
    std::fs::write(app_data.join("settings.json"), br#"{"appConfigVersion":1}"#).unwrap();
    let paths = KernelPaths::desktop(&workspace, &app_data, &cache).unwrap();
    let config = KernelConfig::generate().unwrap();
    let durable = DurableFileStore::at_config(paths.config_root(), config.launch_epoch()).unwrap();
    assert!(AtomicJsonSettingsStore::new(durable).is_ok());
}

#[test]
fn invalid_ui_layout_defaults_without_hiding_settings_and_is_replaced_on_state_commit() {
    let fixture = Fixture::new(1, "generation-a");
    let workspace_key = Uuid::from_u128(1).to_string();
    fixture.store.values.lock().unwrap().extend([
        ("language".to_string(), json!("fr")),
        (
            "uiLayout".to_string(),
            json!({
                workspace_key.clone(): {
                    "schemaVersion": 1,
                    "windowStates": {
                        " main ": {
                            "activeDraftId": null,
                            "draftTabs": [],
                            "fileTreeAssetsVisible": true,
                            "filePath": "notes/a.md",
                            "fileTreeOpen": false,
                            "folderName": null,
                            "folderPath": null,
                            "openFilePaths": [],
                            "sideBySideGroup": null
                        }
                    },
                    "openWindows": []
                }
            }),
        ),
    ]);

    let snapshot = fixture.app_config.read().unwrap();
    assert!(snapshot.local_state.ui_layout.window_states.is_empty());
    assert!(serde_json::to_value(&snapshot.settings)
        .unwrap()
        .to_string()
        .contains("fr"));

    fixture
        .patch(json!([{ "type": "set-pandoc-path", "path": "/opt/pandoc" }]))
        .unwrap();
    let stored = fixture.store.snapshot();
    assert_eq!(stored["language"], "fr");
    assert!(stored["uiLayout"][&workspace_key]["windowStates"]
        .as_object()
        .unwrap()
        .is_empty());
}

#[test]
fn noncanonical_persisted_open_window_label_defaults_the_active_layout() {
    let fixture = Fixture::new(1, "generation-a");
    let workspace_key = Uuid::from_u128(1).to_string();
    fixture.store.values.lock().unwrap().insert(
        "uiLayout".to_string(),
        json!({
            workspace_key: {
                "schemaVersion": 1,
                "windowStates": {},
                "openWindows": [{
                    "filePath": "notes/a.md",
                    "label": " auxiliary ",
                    "openFilePaths": ["notes/a.md"]
                }]
            }
        }),
    );

    let layout = fixture.app_config.read().unwrap().local_state.ui_layout;
    assert!(layout.window_states.is_empty());
    assert!(layout.open_windows.is_empty());
}

#[test]
fn first_settings_or_state_commit_writes_app_config_version() {
    let (settings_root, _config, settings_store) = physical_store();
    let settings = SettingsService::new(
        Arc::new(settings_store),
        Arc::new(RecordingEvents::default()),
    );
    settings.initialize_language_if_invalid("fr").unwrap();
    let document: Value = serde_json::from_slice(
        &std::fs::read(settings_root.path().join("app-data/settings.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(document["appConfigVersion"], 1);

    let (state_root, _config, state_store) = physical_store();
    let store = Arc::new(state_store);
    let events = Arc::new(RecordingEvents::default());
    let coordinator = Arc::new(SettingsRuntimeCoordinator::new(events.clone()));
    let settings = Arc::new(SettingsService::with_coordinator(
        store.clone(),
        coordinator.clone(),
    ));
    let app_config = AppConfigService::new(
        store,
        settings,
        coordinator,
        WorkspaceId::new(Uuid::from_u128(1)),
        WorkspaceGeneration::parse("generation-a").unwrap(),
        events,
    );
    app_config
        .patch_state(
            serde_json::from_value(json!({
                "workspaceGeneration": "generation-a",
                "operations": patch_window("notes/a.md")
            }))
            .unwrap(),
        )
        .unwrap();
    let document: Value = serde_json::from_slice(
        &std::fs::read(state_root.path().join("app-data/settings.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(document["appConfigVersion"], 1);
}

#[test]
fn invalid_document_paths_are_rejected_before_any_store_mutation() {
    let fixture = Fixture::new(1, "generation-a");
    let before = fixture.store.snapshot();
    for path in [
        "/private/a.md",
        "C:/private/a.md",
        "notes/../a.md",
        "notes/a\n.md",
        "notes/a.txt",
    ] {
        let request = serde_json::from_value::<PatchAppConfigStateRequest>(json!({
            "workspaceGeneration": "generation-a",
            "operations": patch_window(path),
        }));
        if let Ok(request) = request {
            let error = fixture.app_config.patch_state(request).unwrap_err();
            assert_eq!(
                error.kind(),
                AppConfigServiceErrorKind::InvalidAppConfigState
            );
        }
        assert_eq!(fixture.store.snapshot(), before, "{path}");
        assert_eq!(fixture.store.saves(), 0, "{path}");
    }
}

#[test]
fn intrinsically_invalid_requests_are_rejected_before_any_store_read() {
    fn assert_preflight_rejects_without_reads(operations: Value) {
        let fixture = Fixture::new(1, "generation-a");
        fixture.store.fail_reads();
        let error = fixture.patch(operations).unwrap_err();
        assert_eq!(
            error.kind(),
            AppConfigServiceErrorKind::InvalidAppConfigState
        );
        assert_eq!(fixture.store.gets(), 0);
        assert_eq!(fixture.store.saves(), 0);
        assert!(fixture.events.snapshot().is_empty());
    }

    assert_preflight_rejects_without_reads(patch_window("notes/not-markdown.txt"));
    assert_preflight_rejects_without_reads(json!([{
        "type": "set-pandoc-path",
        "path": "x".repeat(501)
    }]));

    let chunk = "x".repeat(12 * 1024 * 1024 + 1);
    assert_preflight_rejects_without_reads(json!([{
        "type": "patch-ui-layout",
        "windowLabel": "main",
        "patch": { "draftTabs": [
            { "content": chunk, "id": "1", "name": "1.md", "path": null },
            { "content": chunk, "id": "2", "name": "2.md", "path": null },
            { "content": chunk, "id": "3", "name": "3.md", "path": null },
            { "content": chunk, "id": "4", "name": "4.md", "path": null }
        ] }
    }]));
}

#[test]
fn draft_content_respects_per_draft_and_aggregate_byte_limits() {
    let fixture = Fixture::new(1, "generation-a");
    let exact = "é".repeat(8 * 1024 * 1024);
    fixture
        .patch(json!([{
            "type": "patch-ui-layout",
            "windowLabel": "main",
            "patch": { "draftTabs": [{
                "content": exact,
                "id": "exact",
                "name": "Exact.md",
                "path": null
            }] }
        }]))
        .unwrap();

    let over_one = "é".repeat(8 * 1024 * 1024 + 1);
    assert!(serde_json::from_value::<PatchAppConfigStateRequest>(json!({
        "workspaceGeneration": "generation-a",
        "operations": [{
            "type": "patch-ui-layout",
            "windowLabel": "main",
            "patch": { "draftTabs": [{
                "content": over_one,
                "id": "too-large",
                "name": "Too large.md",
                "path": null
            }] }
        }]
    }))
    .is_err());

    let chunk = "x".repeat(12 * 1024 * 1024 + 1);
    let error = fixture
        .patch(json!([{
            "type": "patch-ui-layout",
            "windowLabel": "main",
            "patch": { "draftTabs": [
                { "content": chunk, "id": "1", "name": "1.md", "path": null },
                { "content": chunk, "id": "2", "name": "2.md", "path": null },
                { "content": chunk, "id": "3", "name": "3.md", "path": null },
                { "content": chunk, "id": "4", "name": "4.md", "path": null }
            ] }
        }]))
        .unwrap_err();
    assert_eq!(
        error.kind(),
        AppConfigServiceErrorKind::InvalidAppConfigState
    );
}

#[test]
fn recent_files_are_deduplicated_and_capped_at_ten() {
    let fixture = Fixture::new(1, "generation-a");
    for index in 0..11 {
        fixture
            .patch(json!([{
                "type": "remember-recent-file",
                "file": {
                    "name": format!("{index}.md"),
                    "path": format!("notes/{index}.md")
                }
            }]))
            .unwrap();
    }
    fixture
        .patch(json!([{
            "type": "remember-recent-file",
            "file": { "name": "renamed.md", "path": "notes/5.md" }
        }]))
        .unwrap();

    let recent = fixture
        .app_config
        .read()
        .unwrap()
        .local_state
        .recent_markdown_files;
    assert_eq!(recent.len(), 10);
    assert_eq!(recent[0].path.as_str(), "notes/5.md");
    assert_eq!(recent[0].name, "renamed.md");
    assert_eq!(
        recent
            .iter()
            .filter(|file| file.path.as_str() == "notes/5.md")
            .count(),
        1
    );
}

#[test]
fn window_patches_are_independent_and_open_windows_is_workspace_level() {
    let fixture = Fixture::new(1, "generation-a");
    fixture.patch(patch_window("notes/main.md")).unwrap();
    fixture
        .patch(json!([{
            "type": "patch-ui-layout",
            "windowLabel": "secondary",
            "patch": {
                "filePath": "notes/secondary.md",
                "openWindows": [{
                    "filePath": "notes/secondary.md",
                    "label": "secondary",
                    "openFilePaths": ["notes/secondary.md"]
                }]
            }
        }]))
        .unwrap();

    let layout = fixture.app_config.read().unwrap().local_state.ui_layout;
    assert_eq!(layout.window_states.len(), 2);
    assert_eq!(
        layout.window_states["main"]
            .file_path
            .as_ref()
            .unwrap()
            .as_str(),
        "notes/main.md"
    );
    assert_eq!(
        layout.window_states["secondary"]
            .file_path
            .as_ref()
            .unwrap()
            .as_str(),
        "notes/secondary.md"
    );
    assert_eq!(layout.open_windows.len(), 1);
    assert_eq!(layout.open_windows[0].label.as_str(), "secondary");
}

#[test]
fn pandoc_path_is_trimmed_bounded_and_nullable() {
    let fixture = Fixture::new(1, "generation-a");
    let snapshot = fixture
        .patch(json!([{ "type": "set-pandoc-path", "path": "  /opt/pandoc  " }]))
        .unwrap();
    assert_eq!(
        snapshot.local_state.pandoc_path.as_ref().unwrap(),
        "/opt/pandoc"
    );

    let snapshot = fixture
        .patch(json!([{ "type": "set-pandoc-path", "path": null }]))
        .unwrap();
    assert!(snapshot.local_state.pandoc_path.as_ref().is_none());

    for path in ["x".repeat(501), "bad\npath".to_string()] {
        let error = fixture
            .patch(json!([{ "type": "set-pandoc-path", "path": path }]))
            .unwrap_err();
        assert_eq!(
            error.kind(),
            AppConfigServiceErrorKind::InvalidAppConfigState
        );
    }
}

#[test]
fn control_characters_are_rejected_before_trimming_app_config_inputs() {
    assert!(serde_json::from_value::<PatchAppConfigStateRequest>(json!({
        "workspaceGeneration": "generation-a",
        "operations": [{
            "type": "patch-ui-layout",
            "windowLabel": "\tmain\t",
            "patch": { "fileTreeOpen": true }
        }]
    }))
    .is_err());

    let fixture = Fixture::new(1, "generation-a");
    for operations in [
        json!([{ "type": "set-pandoc-path", "path": "\n/opt/pandoc\n" }]),
        json!([{
            "type": "patch-ui-layout",
            "windowLabel": "main",
            "patch": { "draftTabs": [{
                "content": "draft",
                "id": "\tdraft-id\t",
                "name": "Draft.md",
                "path": null
            }] }
        }]),
        json!([{
            "type": "patch-ui-layout",
            "windowLabel": "main",
            "patch": { "draftTabs": [{
                "content": "draft",
                "id": "draft-id",
                "name": "\nDraft.md\n",
                "path": null
            }] }
        }]),
    ] {
        let before = fixture.store.snapshot();
        let error = fixture.patch(operations).unwrap_err();
        assert_eq!(
            error.kind(),
            AppConfigServiceErrorKind::InvalidAppConfigState
        );
        assert_eq!(fixture.store.snapshot(), before);
    }
}

#[test]
fn local_revision_changes_only_for_local_state() {
    let fixture = Fixture::new(1, "generation-a");
    let before = fixture.app_config.read().unwrap().local_state.revision;
    fixture
        .settings
        .initialize_language_if_invalid("fr")
        .unwrap();
    let after_setting = fixture.app_config.read().unwrap().local_state.revision;
    assert_eq!(after_setting, before);

    let after_state = fixture
        .patch(json!([{ "type": "set-pandoc-path", "path": "/opt/pandoc" }]))
        .unwrap()
        .local_state
        .revision;
    assert_ne!(after_state, before);
}

#[test]
fn portable_replacement_preserves_app_config_local_keys_and_version() {
    let (root, _config, store) = physical_store();
    store
        .set("uiLayout", json!({ "workspace": { "draft": "secret" } }))
        .unwrap();
    store
        .set(
            "recentMarkdownFilesByWorkspace",
            json!({ "workspace": [{ "path": "a.md" }] }),
        )
        .unwrap();
    store
        .set(
            "fileTreeSortByWorkspace",
            json!({ "workspace": { "key": "name", "direction": "ascending" } }),
        )
        .unwrap();
    store.set("pandocPath", json!("/opt/pandoc")).unwrap();
    store.save().unwrap();
    store
        .replace_portable_atomically(json!({ "language": "fr" }).as_object().unwrap())
        .unwrap();

    let document: Value =
        serde_json::from_slice(&std::fs::read(root.path().join("app-data/settings.json")).unwrap())
            .unwrap();
    assert_eq!(document["appConfigVersion"], 1);
    assert_eq!(
        document["uiLayout"],
        json!({ "workspace": { "draft": "secret" } })
    );
    assert_eq!(
        document["recentMarkdownFilesByWorkspace"],
        json!({ "workspace": [{ "path": "a.md" }] })
    );
    assert_eq!(
        document["fileTreeSortByWorkspace"],
        json!({ "workspace": { "key": "name", "direction": "ascending" } })
    );
    assert_eq!(document["pandocPath"], "/opt/pandoc");
}

#[test]
fn app_config_event_is_redacted() {
    let fixture = Fixture::new(1, "generation-a");
    let snapshot = fixture
        .patch(json!([{
            "type": "patch-ui-layout",
            "windowLabel": "main",
            "patch": { "draftTabs": [{
                "content": "draft-secret",
                "id": "draft-id",
                "name": "Secret.md",
                "path": "notes/secret.md"
            }] }
        }, {
            "type": "set-pandoc-path",
            "path": "/secret/pandoc"
        }]))
        .unwrap();
    let publications = fixture.events.snapshot();
    assert_eq!(publications.len(), 1);
    let publication = &publications[0];
    assert_eq!(publication.revision, snapshot.local_state.revision);
    assert_eq!(
        serde_json::to_value(&publication.resource).unwrap(),
        json!({
            "kind": "app-config",
            "workspaceId": Uuid::from_u128(1),
            "workspaceGeneration": "generation-a"
        })
    );
    assert_eq!(
        serde_json::to_value(&publication.event).unwrap(),
        json!({
            "type": "app-config-state-changed",
            "workspaceId": Uuid::from_u128(1),
            "workspaceGeneration": "generation-a",
            "revision": snapshot.local_state.revision
        })
    );
    assert!(matches!(
        publication.resource,
        ResourceRefDto::AppConfig { .. }
    ));
    assert!(matches!(
        publication.event,
        DomainEvent::AppConfigStateChanged { .. }
    ));
    let wire = serde_json::to_string(&publication.event).unwrap();
    for secret in [
        "draft-secret",
        "notes/secret.md",
        "/secret/pandoc",
        "uiLayout",
    ] {
        assert!(!wire.contains(secret));
    }
}

#[test]
fn app_config_debug_output_redacts_draft_and_pandoc_values() {
    let fixture = Fixture::new(1, "generation-a");
    let request = fixture.request(
        "generation-a",
        json!([{
            "type": "patch-ui-layout",
            "windowLabel": "main",
            "patch": { "draftTabs": [{
                "content": "debug-draft-secret",
                "id": "debug-id-secret",
                "name": "Debug secret.md",
                "path": "notes/debug-secret.md"
            }] }
        }, {
            "type": "set-pandoc-path",
            "path": "/debug/pandoc-secret"
        }]),
    );
    let request_debug = format!("{request:?}");
    for secret in [
        "debug-draft-secret",
        "debug-id-secret",
        "Debug secret.md",
        "notes/debug-secret.md",
        "/debug/pandoc-secret",
    ] {
        assert!(!request_debug.contains(secret));
    }

    let snapshot = fixture.app_config.patch_state(request).unwrap();
    let snapshot_debug = format!("{snapshot:?}");
    for secret in [
        "debug-draft-secret",
        "debug-id-secret",
        "Debug secret.md",
        "notes/debug-secret.md",
        "/debug/pandoc-secret",
    ] {
        assert!(!snapshot_debug.contains(secret));
    }
}

#[test]
fn nested_window_state_debug_redacts_active_draft_identity() {
    let state: StoredWorkspaceWindowStateDto = serde_json::from_value(json!({
        "activeDraftId": "nested-active-secret",
        "draftTabs": [{
            "content": "nested-content-secret",
            "id": "nested-active-secret",
            "name": "Nested secret.md",
            "path": null
        }],
        "fileTreeAssetsVisible": true,
        "filePath": null,
        "fileTreeOpen": false,
        "folderName": null,
        "folderPath": null,
        "openFilePaths": [],
        "sideBySideGroup": null
    }))
    .unwrap();

    let debug = format!("{state:?}");
    for secret in [
        "nested-active-secret",
        "nested-content-secret",
        "Nested secret.md",
    ] {
        assert!(!debug.contains(secret));
    }
}

#[test]
fn certain_save_failure_rolls_back_and_uncertain_save_requires_recovery() {
    let fixture = Fixture::new(1, "generation-a");
    let before = fixture.store.snapshot();
    fixture.store.fail_next_save(UNAVAILABLE);
    let error = fixture.patch(patch_window("notes/a.md")).unwrap_err();
    assert_eq!(error.kind(), AppConfigServiceErrorKind::Unavailable);
    assert_eq!(fixture.store.snapshot(), before);
    assert!(fixture.events.snapshot().is_empty());

    fixture.store.fail_next_save(PUBLISH_UNCERTAIN);
    let error = fixture.patch(patch_window("notes/b.md")).unwrap_err();
    assert_eq!(error.kind(), AppConfigServiceErrorKind::RecoveryRequired);
    assert_eq!(
        fixture.app_config.read().unwrap_err().kind(),
        AppConfigServiceErrorKind::RecoveryRequired
    );
    assert!(fixture.events.snapshot().is_empty());
}

#[test]
fn mutation_contract_is_strict_and_window_labels_are_normalized() {
    assert!(serde_json::from_value::<PatchAppConfigStateRequest>(json!({
        "workspaceGeneration": "generation-a",
        "operations": []
    }))
    .unwrap()
    .validate()
    .is_err());
    assert!(serde_json::from_value::<PatchAppConfigStateRequest>(json!({
        "workspaceGeneration": "generation-a",
        "operations": [
            { "type": "set-pandoc-path", "path": "/opt/first" },
            { "type": "set-pandoc-path", "path": "/opt/second" }
        ]
    }))
    .unwrap()
    .validate()
    .is_err());
    assert!(serde_json::from_value::<PatchAppConfigStateRequest>(json!({
        "workspaceGeneration": "generation-a",
        "operations": [{
            "type": "clear-recent-files",
            "unknown": true
        }]
    }))
    .is_err());

    let request: PatchAppConfigStateRequest = serde_json::from_value(json!({
        "workspaceGeneration": "generation-a",
        "operations": [{
            "type": "patch-ui-layout",
            "windowLabel": "  main  ",
            "patch": { "fileTreeOpen": true }
        }]
    }))
    .unwrap();
    assert_eq!(
        serde_json::to_value(request).unwrap()["operations"][0]["windowLabel"],
        "main"
    );
    let sort_request: PatchAppConfigStateRequest = serde_json::from_value(json!({
        "workspaceGeneration": "generation-a",
        "operations": [{
            "type": "set-file-tree-sort",
            "sort": { "key": "createdAt", "direction": "descending" }
        }]
    }))
    .unwrap();
    assert_eq!(
        serde_json::to_value(sort_request).unwrap()["operations"][0]["sort"]["key"],
        "createdAt"
    );
    for invalid in ["\u{0}".to_string(), "x".repeat(129)] {
        assert!(serde_json::from_value::<PatchAppConfigStateRequest>(json!({
            "workspaceGeneration": "generation-a",
            "operations": [{
                "type": "patch-ui-layout",
                "windowLabel": invalid,
                "patch": { "fileTreeOpen": true }
            }]
        }))
        .is_err());
    }
}
