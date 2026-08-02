use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc, Barrier, Mutex,
    },
    thread,
    time::Duration,
};

use axum::{
    body::{to_bytes, Body},
    http::{header, Request, StatusCode},
};
use qingyu_kernel::{
    api::{build_router, TransportPolicy},
    config::KernelConfig,
    contract::{
        ApiErrorEnvelope, DomainEvent, ErrorCode, ResourceRefDto, SettingKey, SettingValueDto,
    },
    events::{EventPublication, EventSink, EventSinkError},
    paths::KernelPaths,
    ports::KernelPorts,
    runtime::{KernelRuntime, SettingsApiService},
    settings::{
        model::{sanitize_legacy_remote_portable_settings, validate_portable_settings_bytes},
        service::{
            SettingsGroup, SettingsPublicationBatch, SettingsPublicationBatchSink,
            SettingsPublicationEvent, SettingsRuntimeCoordinator, SettingsService,
            SettingsServiceErrorKind,
        },
        storage::{AtomicJsonSettingsStore, SettingsStore, SettingsStoreError},
    },
    storage::DurableFileStore,
};
use serde_json::Value;
use sha2::Digest as _;
use tempfile::tempdir;
use tower::ServiceExt as _;

const HOST: &str = "127.0.0.1:43124";
const ORIGIN: &str = "tauri://localhost";

struct MemorySettingsStore {
    values: Mutex<BTreeMap<String, Value>>,
    corrupt_replaces: AtomicUsize,
    fail_replace_on_call: AtomicUsize,
    fail_get: AtomicBool,
    fail_set_from_call: AtomicUsize,
    fail_save: AtomicBool,
    fail_delete: AtomicBool,
    invalid_replaces: AtomicUsize,
    publish_uncertain_on_replace: AtomicBool,
    publish_uncertain_on_save: AtomicBool,
    replaces: AtomicUsize,
    saves: AtomicUsize,
    set_barrier: Option<Arc<Barrier>>,
    sets: AtomicUsize,
    operations: Option<Arc<Mutex<Vec<&'static str>>>>,
}

impl MemorySettingsStore {
    fn with(values: impl IntoIterator<Item = (&'static str, Value)>) -> Self {
        Self {
            values: Mutex::new(
                values
                    .into_iter()
                    .map(|(key, value)| (key.to_string(), value))
                    .collect(),
            ),
            corrupt_replaces: AtomicUsize::new(0),
            fail_replace_on_call: AtomicUsize::new(usize::MAX),
            fail_get: AtomicBool::new(false),
            fail_set_from_call: AtomicUsize::new(usize::MAX),
            fail_save: AtomicBool::new(false),
            fail_delete: AtomicBool::new(false),
            invalid_replaces: AtomicUsize::new(0),
            publish_uncertain_on_replace: AtomicBool::new(false),
            publish_uncertain_on_save: AtomicBool::new(false),
            replaces: AtomicUsize::new(0),
            saves: AtomicUsize::new(0),
            set_barrier: None,
            sets: AtomicUsize::new(0),
            operations: None,
        }
    }

    fn with_set_barrier(
        values: impl IntoIterator<Item = (&'static str, Value)>,
        set_barrier: Arc<Barrier>,
    ) -> Self {
        let mut store = Self::with(values);
        store.set_barrier = Some(set_barrier);
        store
    }

    fn with_operations(operations: Arc<Mutex<Vec<&'static str>>>) -> Self {
        Self {
            operations: Some(operations),
            ..Self::default()
        }
    }
}

impl Default for MemorySettingsStore {
    fn default() -> Self {
        Self::with([])
    }
}

impl SettingsStore for MemorySettingsStore {
    fn get(&self, key: &str) -> Result<Option<Value>, SettingsStoreError> {
        if self.fail_get.load(Ordering::Relaxed) {
            return Err(SettingsStoreError::unavailable());
        }
        Ok(self.values.lock().unwrap().get(key).cloned())
    }

    fn set(&self, key: &str, value: Value) -> Result<(), SettingsStoreError> {
        let call = self.sets.fetch_add(1, Ordering::Relaxed) + 1;
        if call >= self.fail_set_from_call.load(Ordering::Relaxed) {
            return Err(SettingsStoreError::unavailable());
        }
        self.values.lock().unwrap().insert(key.to_string(), value);
        if let Some(operations) = &self.operations {
            operations.lock().unwrap().push("set");
        }
        if call == 1 {
            if let Some(barrier) = &self.set_barrier {
                barrier.wait();
                barrier.wait();
            }
        }
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), SettingsStoreError> {
        if self.fail_delete.load(Ordering::Relaxed) {
            return Err(SettingsStoreError::unavailable());
        }
        self.values.lock().unwrap().remove(key);
        Ok(())
    }

    fn save(&self) -> Result<(), SettingsStoreError> {
        self.saves.fetch_add(1, Ordering::Relaxed);
        if let Some(operations) = &self.operations {
            operations.lock().unwrap().push("save");
        }
        if self.publish_uncertain_on_save.load(Ordering::Relaxed) {
            Err(SettingsStoreError::publish_uncertain())
        } else if self.fail_save.load(Ordering::Relaxed) {
            Err(SettingsStoreError::unavailable())
        } else {
            Ok(())
        }
    }

    fn replace_portable_atomically(
        &self,
        desired: &serde_json::Map<String, Value>,
    ) -> Result<(), SettingsStoreError> {
        let call = self.replaces.fetch_add(1, Ordering::Relaxed) + 1;
        if self.fail_replace_on_call.load(Ordering::Relaxed) == call {
            return Err(SettingsStoreError::unavailable());
        }
        let mut values = self.values.lock().unwrap();
        for key in qingyu_kernel::settings::model::PORTABLE_SETTINGS_KEYS {
            values.remove(key);
        }
        values.extend(
            desired
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
        if self
            .corrupt_replaces
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
                remaining.checked_sub(1)
            })
            .is_ok()
        {
            values.insert("language".to_string(), serde_json::json!("de"));
        }
        if self
            .invalid_replaces
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
                remaining.checked_sub(1)
            })
            .is_ok()
        {
            values.insert("language".to_string(), serde_json::json!("xx"));
        }
        if self.publish_uncertain_on_replace.load(Ordering::Relaxed) {
            Err(SettingsStoreError::publish_uncertain())
        } else {
            Ok(())
        }
    }
}

struct RecordingEvents {
    fail: AtomicBool,
    publications: Mutex<Vec<EventPublication>>,
    operations: Option<Arc<Mutex<Vec<&'static str>>>>,
}

struct GateCheckingEvents {
    gate: Arc<Mutex<()>>,
    observed_unlocked: AtomicBool,
}

struct GateCheckingSettingsBatchSink {
    gate: Arc<Mutex<()>>,
    observed_unlocked: AtomicBool,
}

impl SettingsPublicationBatchSink for GateCheckingSettingsBatchSink {
    fn publish(&self, _batch: &SettingsPublicationBatch) -> Result<(), EventSinkError> {
        self.observed_unlocked
            .store(self.gate.try_lock().is_ok(), Ordering::Relaxed);
        Ok(())
    }
}

#[derive(Default)]
struct RecordingSettingsBatches {
    batches: Mutex<Vec<SettingsPublicationBatch>>,
}

impl SettingsPublicationBatchSink for RecordingSettingsBatches {
    fn publish(&self, batch: &SettingsPublicationBatch) -> Result<(), EventSinkError> {
        self.batches.lock().unwrap().push(batch.clone());
        Ok(())
    }
}

#[derive(Default)]
struct ReentrantSettingsBatchSink {
    batches: Mutex<Vec<SettingsPublicationBatch>>,
    in_callback: AtomicBool,
    nested_callback: AtomicBool,
    service: Mutex<Option<Arc<SettingsService>>>,
    triggered: AtomicBool,
}

impl SettingsPublicationBatchSink for ReentrantSettingsBatchSink {
    fn publish(&self, batch: &SettingsPublicationBatch) -> Result<(), EventSinkError> {
        if self.in_callback.swap(true, Ordering::Relaxed) {
            self.nested_callback.store(true, Ordering::Relaxed);
        }
        self.batches.lock().unwrap().push(batch.clone());
        if !self.triggered.swap(true, Ordering::Relaxed) {
            let service = self
                .service
                .lock()
                .unwrap()
                .take()
                .expect("reentrant service installed");
            let patch = serde_json::from_value(serde_json::json!({
                "expectedRevision": batch.publication().revision.as_str(),
                "values": [{
                    "key": "language",
                    "value": { "type": "string", "value": "fr" }
                }]
            }))
            .unwrap();
            service
                .patch_exposed(patch)
                .expect("reentrant settings patch");
        }
        self.in_callback.store(false, Ordering::Relaxed);
        Ok(())
    }
}

impl EventSink for GateCheckingEvents {
    fn publish(&self, _publication: &EventPublication) -> Result<(), EventSinkError> {
        self.observed_unlocked
            .store(self.gate.try_lock().is_ok(), Ordering::Relaxed);
        Ok(())
    }
}

impl RecordingEvents {
    fn with_operations(operations: Arc<Mutex<Vec<&'static str>>>) -> Self {
        Self {
            fail: AtomicBool::new(false),
            publications: Mutex::new(Vec::new()),
            operations: Some(operations),
        }
    }
}

impl Default for RecordingEvents {
    fn default() -> Self {
        Self {
            fail: AtomicBool::new(false),
            publications: Mutex::new(Vec::new()),
            operations: None,
        }
    }
}

impl EventSink for RecordingEvents {
    fn publish(&self, publication: &EventPublication) -> Result<(), EventSinkError> {
        self.publications.lock().unwrap().push(publication.clone());
        if let Some(operations) = &self.operations {
            operations.lock().unwrap().push("publish");
        }
        if self.fail.load(Ordering::Relaxed) {
            Err(EventSinkError)
        } else {
            Ok(())
        }
    }
}

#[test]
fn default_read_is_complete_ordered_and_safe() {
    let store = Arc::new(MemorySettingsStore::with([
        ("workspace", serde_json::json!({ "path": "/private/notes" })),
        (
            "exportSettings",
            serde_json::json!({ "pandocPath": "/private/bin/pandoc" }),
        ),
    ]));
    let service = SettingsService::new(store, Arc::new(RecordingEvents::default()));

    let snapshot = service.read_exposed().expect("read settings");

    assert_eq!(snapshot.values.len(), 26);
    assert_eq!(snapshot.values[0].key, SettingKey::AppearanceMode);
    assert_eq!(snapshot.values[3].key, SettingKey::ThemeCustomCssLight);
    assert_eq!(snapshot.values[25].key, SettingKey::ExportPdfPageSize);
    let serialized = serde_json::to_string(&snapshot).unwrap();
    assert!(!serialized.contains("/private"));
    assert!(!serialized.contains("workspace"));
    assert!(!serialized.contains("pandocPath"));
}

#[test]
fn default_read_uses_the_frontend_owned_default_theme_ids() {
    let service = SettingsService::new(
        Arc::new(MemorySettingsStore::default()),
        Arc::new(RecordingEvents::default()),
    );

    let snapshot = service.read_exposed().expect("read default settings");
    let value_for = |key| {
        &snapshot
            .values
            .iter()
            .find(|entry| entry.key == key)
            .expect("default setting exists")
            .value
    };

    assert_eq!(
        value_for(SettingKey::AppearanceLightTheme),
        &SettingValueDto::String {
            value: "light".to_string(),
        }
    );
    assert_eq!(
        value_for(SettingKey::AppearanceDarkTheme),
        &SettingValueDto::String {
            value: "dark".to_string(),
        }
    );
}

#[test]
fn read_exposed_rejects_semantically_invalid_stored_values() {
    for (key, value) in [
        ("language", serde_json::json!("xx")),
        ("lightThemeId", serde_json::json!("qingyu-reserved")),
        (
            "editorPreferences",
            serde_json::json!({ "bodyFontSize": 19 }),
        ),
        (
            "editorPreferences",
            serde_json::json!({
                "editorFontFamily": { "source": "system", "family": " Private Font " }
            }),
        ),
        ("fileIgnoreSettings", serde_json::json!({})),
    ] {
        let store = Arc::new(MemorySettingsStore::with([(key, value)]));
        let service = SettingsService::new(store, Arc::new(RecordingEvents::default()));

        let error = service
            .read_exposed()
            .expect_err("invalid stored semantics must fail closed");

        assert_eq!(error.kind(), SettingsServiceErrorKind::Unavailable);
    }
}

#[test]
fn settings_change_event_shape_uses_the_typed_contract() {
    let publication = EventPublication {
        resource: ResourceRefDto::Settings {},
        revision: qingyu_kernel::contract::Revision::parse("revision").unwrap(),
        event: DomainEvent::SettingsChanged {
            settings: serde_json::from_value(serde_json::json!({
                "revision": "revision",
                "values": []
            }))
            .unwrap(),
        },
    };

    assert!(matches!(publication.resource, ResourceRefDto::Settings {}));
}

#[test]
fn portable_snapshot_upgrades_missing_export_font_family_without_writing_back() {
    let store = Arc::new(MemorySettingsStore::with([(
        "exportSettings",
        serde_json::json!({
            "pandocArgs": "--toc",
            "pdfAuthor": "QingYu",
            "pdfFooter": "",
            "pdfHeader": "",
            "pdfHeightMm": 297,
            "pdfMarginMm": 18,
            "pdfMarginPreset": "default",
            "pdfPageBreakOnH1": false,
            "pdfPageSize": "default",
            "pdfWidthMm": 210
        }),
    )]));
    let service = SettingsService::new(store.clone(), Arc::new(RecordingEvents::default()));

    let snapshot = service.portable_snapshot().expect("portable snapshot");
    let portable: Value = serde_json::from_slice(snapshot.bytes().unwrap()).unwrap();

    assert!(portable["exportSettings"]["fontFamily"].is_null());
    assert_eq!(
        snapshot.revision().as_str(),
        format!(
            "sha256:{:x}",
            sha2::Sha256::digest(snapshot.bytes().unwrap())
        )
    );
    assert!(store.values.lock().unwrap()["exportSettings"]
        .get("fontFamily")
        .is_none());
}

#[test]
fn atomic_json_store_survives_reopen_and_preserves_non_portable_values() {
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
    store.set("language", serde_json::json!("en")).unwrap();
    store
        .set("localOnly", serde_json::json!({ "path": "/private/state" }))
        .unwrap();
    store.save().unwrap();
    drop(store);

    let durable = DurableFileStore::at_config(paths.config_root(), config.launch_epoch()).unwrap();
    let reopened = AtomicJsonSettingsStore::new(durable).unwrap();
    assert_eq!(
        reopened.get("language").unwrap(),
        Some(serde_json::json!("en"))
    );
    assert_eq!(
        reopened.get("localOnly").unwrap(),
        Some(serde_json::json!({ "path": "/private/state" }))
    );
    reopened
        .replace_portable_atomically(serde_json::json!({ "language": "fr" }).as_object().unwrap())
        .unwrap();
    drop(reopened);

    let durable = DurableFileStore::at_config(paths.config_root(), config.launch_epoch()).unwrap();
    let final_store = AtomicJsonSettingsStore::new(durable).unwrap();
    assert_eq!(
        final_store.get("language").unwrap(),
        Some(serde_json::json!("fr"))
    );
    assert_eq!(
        final_store.get("localOnly").unwrap(),
        Some(serde_json::json!({ "path": "/private/state" }))
    );
}

#[test]
fn stale_patch_reports_the_current_revision_without_writing() {
    let store = Arc::new(MemorySettingsStore::default());
    let service = SettingsService::new(store.clone(), Arc::new(RecordingEvents::default()));
    let current = service.read_exposed().unwrap();
    let patch = serde_json::from_value(serde_json::json!({
        "expectedRevision": "stale",
        "values": [{
            "key": "language",
            "value": { "type": "string", "value": "fr" }
        }]
    }))
    .unwrap();

    let error = service.patch_exposed(patch).unwrap_err();

    assert_eq!(error.kind(), SettingsServiceErrorKind::RevisionConflict);
    assert_eq!(error.current_revision(), Some(&current.revision));
    assert_eq!(store.saves.load(Ordering::Relaxed), 0);
    assert!(store.values.lock().unwrap().is_empty());
}

#[test]
fn patch_commits_once_then_publishes_one_typed_snapshot() {
    let store = Arc::new(MemorySettingsStore::default());
    let events = Arc::new(RecordingEvents::default());
    let service = SettingsService::new(store.clone(), events.clone());
    let before = service.read_exposed().unwrap();
    let patch = serde_json::from_value(serde_json::json!({
        "expectedRevision": before.revision.as_str(),
        "values": [
            {
                "key": "appearance.mode",
                "value": { "type": "string", "value": "dark" }
            },
            {
                "key": "language",
                "value": { "type": "string", "value": "zh-CN" }
            }
        ]
    }))
    .unwrap();

    let after = service.patch_exposed(patch).expect("patch settings");

    assert_ne!(after.revision, before.revision);
    assert_eq!(store.saves.load(Ordering::Relaxed), 1);
    let values = store.values.lock().unwrap();
    assert_eq!(values["appearanceMode"], serde_json::json!("dark"));
    assert_eq!(values["language"], serde_json::json!("zh-CN"));
    drop(values);
    let publications = events.publications.lock().unwrap();
    assert_eq!(publications.len(), 1);
    assert_eq!(publications[0].revision, after.revision);
    assert!(matches!(
        publications[0].resource,
        ResourceRefDto::Settings {}
    ));
    match &publications[0].event {
        DomainEvent::SettingsChanged { settings } => assert_eq!(settings, &after),
        event => panic!("unexpected event: {event:?}"),
    }
}

#[test]
fn patch_deferred_result_carries_legacy_publications_for_the_host() {
    let store = Arc::new(MemorySettingsStore::with([
        ("appearanceMode", serde_json::json!("light")),
        ("language", serde_json::json!("en")),
    ]));
    let batches = Arc::new(RecordingSettingsBatches::default());
    let coordinator = Arc::new(SettingsRuntimeCoordinator::with_batch_sink(batches.clone()));
    let service = SettingsService::with_coordinator(store, coordinator);
    let before = service.read_exposed().unwrap();
    let patch = serde_json::from_value(serde_json::json!({
        "expectedRevision": before.revision.as_str(),
        "values": [
            {
                "key": "appearance.mode",
                "value": { "type": "string", "value": "dark" }
            },
            {
                "key": "language",
                "value": { "type": "string", "value": "fr" }
            }
        ]
    }))
    .unwrap();

    let deferred = service.patch_exposed_deferred(patch).unwrap();

    assert!(batches.batches.lock().unwrap().is_empty());
    let committed = deferred.settings().clone();
    deferred.publish().unwrap();
    let published = batches.batches.lock().unwrap();
    assert_eq!(published.len(), 1);
    assert_eq!(
        published[0]
            .publications()
            .iter()
            .map(|publication| publication.event_name())
            .collect::<Vec<_>>(),
        vec!["markra://theme-changed", "markra://language-changed"]
    );
    assert_eq!(
        published[0].publications()[0].payload()["preferences"]["appearanceMode"],
        serde_json::json!("dark")
    );
    assert_eq!(
        published[0].publications()[1].payload()["language"],
        serde_json::json!("fr")
    );
    drop(published);
    assert_eq!(service.read_exposed().unwrap(), committed);
}

#[test]
fn patch_applies_all_values_then_saves_once_before_publication() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let store = Arc::new(MemorySettingsStore::with_operations(operations.clone()));
    let events = Arc::new(RecordingEvents::with_operations(operations.clone()));
    let service = SettingsService::new(store, events);
    let before = service.read_exposed().unwrap();
    let patch = serde_json::from_value(serde_json::json!({
        "expectedRevision": before.revision.as_str(),
        "values": [
            {
                "key": "appearance.mode",
                "value": { "type": "string", "value": "dark" }
            },
            {
                "key": "language",
                "value": { "type": "string", "value": "fr" }
            }
        ]
    }))
    .unwrap();

    service.patch_exposed(patch).unwrap();

    assert_eq!(
        *operations.lock().unwrap(),
        vec!["set", "set", "save", "publish"]
    );
}

#[test]
fn patch_publishes_after_releasing_the_transaction_gate() {
    let gate = Arc::new(Mutex::new(()));
    let events = Arc::new(GateCheckingEvents {
        gate: gate.clone(),
        observed_unlocked: AtomicBool::new(false),
    });
    let coordinator = Arc::new(SettingsRuntimeCoordinator::with_transaction_gate(
        events.clone(),
        gate,
    ));
    let service =
        SettingsService::with_coordinator(Arc::new(MemorySettingsStore::default()), coordinator);
    let before = service.read_exposed().unwrap();
    let patch = serde_json::from_value(serde_json::json!({
        "expectedRevision": before.revision.as_str(),
        "values": [{
            "key": "language",
            "value": { "type": "string", "value": "fr" }
        }]
    }))
    .unwrap();

    service.patch_exposed(patch).unwrap();

    assert!(events.observed_unlocked.load(Ordering::Relaxed));
}

#[test]
fn reads_cannot_observe_a_partially_applied_multi_key_patch() {
    let set_barrier = Arc::new(Barrier::new(2));
    let store = Arc::new(MemorySettingsStore::with_set_barrier(
        [
            ("appearanceMode", serde_json::json!("light")),
            ("language", serde_json::json!("en")),
        ],
        set_barrier.clone(),
    ));
    let service = Arc::new(SettingsService::new(
        store,
        Arc::new(RecordingEvents::default()),
    ));
    let before = service.read_exposed().unwrap();
    let patch = serde_json::from_value(serde_json::json!({
        "expectedRevision": before.revision.as_str(),
        "values": [
            {
                "key": "appearance.mode",
                "value": { "type": "string", "value": "dark" }
            },
            {
                "key": "language",
                "value": { "type": "string", "value": "fr" }
            }
        ]
    }))
    .unwrap();

    let patch_service = service.clone();
    let patch_thread = thread::spawn(move || patch_service.patch_exposed(patch));
    set_barrier.wait();

    let (sender, receiver) = mpsc::channel();
    let read_service = service.clone();
    let read_thread = thread::spawn(move || {
        sender.send(read_service.read_exposed()).unwrap();
    });

    let early_read = receiver.recv_timeout(Duration::from_millis(100));
    let read_was_blocked = early_read.is_err();
    set_barrier.wait();

    let committed = patch_thread.join().unwrap().unwrap();
    let observed = match early_read {
        Ok(observed) => observed,
        Err(_) => receiver.recv_timeout(Duration::from_secs(2)).unwrap(),
    }
    .unwrap();
    read_thread.join().unwrap();
    assert!(read_was_blocked);
    assert_eq!(observed, committed);
    let appearance = observed
        .values
        .iter()
        .find(|entry| entry.key == SettingKey::AppearanceMode)
        .unwrap();
    let language = observed
        .values
        .iter()
        .find(|entry| entry.key == SettingKey::Language)
        .unwrap();
    assert_eq!(
        appearance.value,
        qingyu_kernel::contract::SettingValueDto::String {
            value: "dark".to_string(),
        }
    );
    assert_eq!(
        language.value,
        qingyu_kernel::contract::SettingValueDto::String {
            value: "fr".to_string(),
        }
    );
}

#[test]
fn two_services_sharing_a_store_can_share_one_transaction_gate() {
    let set_barrier = Arc::new(Barrier::new(2));
    let store = Arc::new(MemorySettingsStore::with_set_barrier(
        [
            ("appearanceMode", serde_json::json!("light")),
            ("language", serde_json::json!("en")),
        ],
        set_barrier.clone(),
    ));
    let gate = Arc::new(Mutex::new(()));
    let coordinator = Arc::new(SettingsRuntimeCoordinator::with_transaction_gate(
        Arc::new(RecordingEvents::default()),
        gate,
    ));
    let first = Arc::new(SettingsService::with_coordinator(
        store.clone(),
        coordinator.clone(),
    ));
    let second = Arc::new(SettingsService::with_coordinator(store, coordinator));
    let before = first.read_exposed().unwrap();
    let first_patch = serde_json::from_value(serde_json::json!({
        "expectedRevision": before.revision.as_str(),
        "values": [
            {
                "key": "appearance.mode",
                "value": { "type": "string", "value": "dark" }
            },
            {
                "key": "language",
                "value": { "type": "string", "value": "fr" }
            }
        ]
    }))
    .unwrap();
    let stale_patch = serde_json::from_value(serde_json::json!({
        "expectedRevision": before.revision.as_str(),
        "values": [{
            "key": "language",
            "value": { "type": "string", "value": "de" }
        }]
    }))
    .unwrap();

    let first_thread = thread::spawn(move || first.patch_exposed(first_patch));
    set_barrier.wait();
    let (sender, receiver) = mpsc::channel();
    let second_thread = thread::spawn(move || {
        sender.send(second.patch_exposed(stale_patch)).unwrap();
    });

    assert!(receiver.recv_timeout(Duration::from_millis(100)).is_err());
    set_barrier.wait();
    first_thread.join().unwrap().unwrap();
    let stale = receiver
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap_err();
    second_thread.join().unwrap();
    assert_eq!(stale.kind(), SettingsServiceErrorKind::RevisionConflict);
}

#[test]
fn shared_coordinator_publishes_deferred_commits_in_commit_order() {
    let store = Arc::new(MemorySettingsStore::default());
    let batches = Arc::new(RecordingSettingsBatches::default());
    let coordinator = Arc::new(SettingsRuntimeCoordinator::with_batch_sink(batches.clone()));
    let first = SettingsService::with_coordinator(store.clone(), coordinator.clone());
    let second = SettingsService::with_coordinator(store.clone(), coordinator);
    let before = first.read_exposed().unwrap();
    let first_patch = serde_json::from_value(serde_json::json!({
        "expectedRevision": before.revision.as_str(),
        "values": [{
            "key": "appearance.mode",
            "value": { "type": "string", "value": "dark" }
        }]
    }))
    .unwrap();
    let first_deferred = first.patch_exposed_deferred(first_patch).unwrap();
    let second_patch = serde_json::from_value(serde_json::json!({
        "expectedRevision": first_deferred.settings().revision.as_str(),
        "values": [{
            "key": "language",
            "value": { "type": "string", "value": "fr" }
        }]
    }))
    .unwrap();
    let second_deferred = second.patch_exposed_deferred(second_patch).unwrap();
    let first_revision = first_deferred.settings().revision.clone();
    let second_revision = second_deferred.settings().revision.clone();
    second_deferred.publish().unwrap();
    assert!(batches.batches.lock().unwrap().is_empty());

    first_deferred.publish().unwrap();
    let published = batches.batches.lock().unwrap();
    assert_eq!(published.len(), 2);
    assert_eq!(published[0].publication().revision, first_revision);
    assert_eq!(published[1].publication().revision, second_revision);
    assert_eq!(
        published[0].publications()[0].event_name(),
        "markra://theme-changed"
    );
    assert_eq!(
        published[1].publications()[0].event_name(),
        "markra://language-changed"
    );
}

#[test]
fn batch_sink_can_reenter_the_same_coordinator_without_deadlock_or_nested_publication() {
    let store = Arc::new(MemorySettingsStore::default());
    let batches = Arc::new(ReentrantSettingsBatchSink::default());
    let coordinator = Arc::new(SettingsRuntimeCoordinator::with_batch_sink(batches.clone()));
    let service = Arc::new(SettingsService::with_coordinator(store, coordinator));
    *batches.service.lock().unwrap() = Some(service.clone());
    let before = service.read_exposed().unwrap();
    let patch = serde_json::from_value(serde_json::json!({
        "expectedRevision": before.revision.as_str(),
        "values": [{
            "key": "appearance.mode",
            "value": { "type": "string", "value": "dark" }
        }]
    }))
    .unwrap();

    service.patch_exposed(patch).unwrap();

    assert!(!batches.nested_callback.load(Ordering::Relaxed));
    let published = batches.batches.lock().unwrap();
    assert_eq!(published.len(), 2);
    assert_eq!(
        published[0].publications()[0].event_name(),
        "markra://theme-changed"
    );
    assert_eq!(
        published[1].publications()[0].event_name(),
        "markra://language-changed"
    );
    drop(published);
    let final_snapshot = service.read_exposed().unwrap();
    assert_eq!(
        final_snapshot.values[5].value,
        qingyu_kernel::contract::SettingValueDto::String {
            value: "fr".to_string()
        }
    );
}

#[test]
fn dropping_a_durable_deferred_commit_latches_recovery_instead_of_losing_legacy_events() {
    let store = Arc::new(MemorySettingsStore::default());
    let batches = Arc::new(RecordingSettingsBatches::default());
    let coordinator = Arc::new(SettingsRuntimeCoordinator::with_batch_sink(batches.clone()));
    let first = SettingsService::with_coordinator(store.clone(), coordinator.clone());
    let second = SettingsService::with_coordinator(store.clone(), coordinator);
    let before = first.read_exposed().unwrap();
    let first_deferred = first
        .patch_exposed_deferred(
            serde_json::from_value(serde_json::json!({
                "expectedRevision": before.revision.as_str(),
                "values": [{
                    "key": "appearance.mode",
                    "value": { "type": "string", "value": "dark" }
                }]
            }))
            .unwrap(),
        )
        .unwrap();
    let second_deferred = second
        .patch_exposed_deferred(
            serde_json::from_value(serde_json::json!({
                "expectedRevision": first_deferred.settings().revision.as_str(),
                "values": [{
                    "key": "language",
                    "value": { "type": "string", "value": "fr" }
                }]
            }))
            .unwrap(),
        )
        .unwrap();
    second_deferred.publish().unwrap();
    assert!(batches.batches.lock().unwrap().is_empty());

    drop(first_deferred);
    assert!(batches.batches.lock().unwrap().is_empty());
    let error = second.read_exposed().unwrap_err();
    assert_eq!(error.kind(), SettingsServiceErrorKind::RecoveryRequired);

    let rebuilt = SettingsService::new(store, Arc::new(RecordingEvents::default()));
    assert!(rebuilt.read_exposed().is_ok());
}

#[test]
fn cancelling_a_deferred_commit_drains_later_publications_without_emitting_it() {
    let store = Arc::new(MemorySettingsStore::default());
    let batches = Arc::new(RecordingSettingsBatches::default());
    let coordinator = Arc::new(SettingsRuntimeCoordinator::with_batch_sink(batches.clone()));
    let service = SettingsService::with_coordinator(store, coordinator);
    let before = service.read_exposed().unwrap();
    let first = service
        .patch_exposed_deferred(
            serde_json::from_value(serde_json::json!({
                "expectedRevision": before.revision.as_str(),
                "values": [{
                    "key": "appearance.mode",
                    "value": { "type": "string", "value": "dark" }
                }]
            }))
            .unwrap(),
        )
        .unwrap();
    let second = service
        .patch_exposed_deferred(
            serde_json::from_value(serde_json::json!({
                "expectedRevision": first.settings().revision.as_str(),
                "values": [{
                    "key": "language",
                    "value": { "type": "string", "value": "fr" }
                }]
            }))
            .unwrap(),
        )
        .unwrap();

    second.publish().unwrap();
    assert!(batches.batches.lock().unwrap().is_empty());
    first.cancel().unwrap();

    let published = batches.batches.lock().unwrap();
    assert_eq!(published.len(), 1);
    assert_eq!(
        published[0].publications()[0].event_name(),
        "markra://language-changed"
    );
    drop(published);
    assert!(service.read_exposed().is_ok());
}

#[test]
fn stale_conditional_publication_is_superseded_without_latching_recovery() {
    let store = Arc::new(MemorySettingsStore::default());
    let batches = Arc::new(RecordingSettingsBatches::default());
    let coordinator = Arc::new(SettingsRuntimeCoordinator::with_batch_sink(batches.clone()));
    let service = SettingsService::with_coordinator(store, coordinator);
    let before = service.portable_snapshot().unwrap();
    let deferred = service
        .replace_portable_deferred(Some(br#"{"language":"fr"}"#), before.revision())
        .unwrap();

    let published = service
        .publish_if_portable_revision(
            deferred,
            &qingyu_kernel::contract::Revision::parse("sha256:stale").unwrap(),
        )
        .unwrap();

    assert!(!published);
    assert!(batches.batches.lock().unwrap().is_empty());
    assert!(service.portable_snapshot().is_ok());
}

#[test]
fn conditional_publication_releases_the_settings_transaction_before_events() {
    let gate = Arc::new(Mutex::new(()));
    let events = Arc::new(GateCheckingEvents {
        gate: gate.clone(),
        observed_unlocked: AtomicBool::new(false),
    });
    let coordinator = Arc::new(SettingsRuntimeCoordinator::with_transaction_gate(
        events.clone(),
        gate,
    ));
    let service =
        SettingsService::with_coordinator(Arc::new(MemorySettingsStore::default()), coordinator);
    let before = service.portable_snapshot().unwrap();
    let deferred = service
        .replace_portable_deferred(Some(br#"{"language":"fr"}"#), before.revision())
        .unwrap();
    let committed = service.portable_snapshot().unwrap();

    assert!(service
        .publish_if_portable_revision(deferred, committed.revision())
        .unwrap());

    assert!(events.observed_unlocked.load(Ordering::Relaxed));
}

#[test]
fn conditional_publication_read_failure_supersedes_without_latching_recovery() {
    let store = Arc::new(MemorySettingsStore::default());
    let service = SettingsService::new(store.clone(), Arc::new(RecordingEvents::default()));
    let before = service.portable_snapshot().unwrap();
    let deferred = service
        .replace_portable_deferred(Some(br#"{"language":"fr"}"#), before.revision())
        .unwrap();
    let committed = service.portable_snapshot().unwrap();
    store.fail_get.store(true, Ordering::Relaxed);

    let error = service
        .publish_if_portable_revision(deferred, committed.revision())
        .unwrap_err();

    assert_eq!(error.kind(), SettingsServiceErrorKind::Unavailable);
    store.fail_get.store(false, Ordering::Relaxed);
    assert!(service.portable_snapshot().is_ok());
}

#[test]
fn conditional_read_failure_releases_the_gate_before_cancelling_and_draining_a_later_ticket() {
    let gate = Arc::new(Mutex::new(()));
    let batches = Arc::new(GateCheckingSettingsBatchSink {
        gate: gate.clone(),
        observed_unlocked: AtomicBool::new(false),
    });
    let coordinator = Arc::new(
        SettingsRuntimeCoordinator::with_batch_sink_and_transaction_gate(batches.clone(), gate),
    );
    let store = Arc::new(MemorySettingsStore::default());
    let service = SettingsService::with_coordinator(store.clone(), coordinator);
    let before = service.portable_snapshot().unwrap();
    let first = service
        .replace_portable_deferred(Some(br#"{"language":"fr"}"#), before.revision())
        .unwrap();
    let after_first = service.portable_snapshot().unwrap();
    let second = service
        .replace_portable_deferred(Some(br#"{"language":"de"}"#), after_first.revision())
        .unwrap();
    let after_second = service.portable_snapshot().unwrap();
    second.publish().unwrap();
    store.fail_get.store(true, Ordering::Relaxed);

    let error = service
        .publish_if_portable_revision(first, after_second.revision())
        .unwrap_err();

    assert_eq!(error.kind(), SettingsServiceErrorKind::Unavailable);
    assert!(batches.observed_unlocked.load(Ordering::Relaxed));
    store.fail_get.store(false, Ordering::Relaxed);
    assert!(service.portable_snapshot().is_ok());
}

#[test]
fn portable_preflight_failure_never_mutates_or_allocates_an_unsettled_publication() {
    let store = Arc::new(MemorySettingsStore::with([(
        "language",
        serde_json::json!("en"),
    )]));
    let service = SettingsService::new(store.clone(), Arc::new(RecordingEvents::default()));
    let before = service.portable_snapshot().unwrap();
    let preflights = AtomicUsize::new(0);

    let error = service
        .replace_portable_deferred_with_preflight(
            Some(br#"{"language":"fr"}"#),
            before.revision(),
            || {
                preflights.fetch_add(1, Ordering::Relaxed);
                false
            },
        )
        .unwrap_err();

    assert_eq!(error.kind(), SettingsServiceErrorKind::ReconcileFailed);
    assert_eq!(preflights.load(Ordering::Relaxed), 1);
    assert_eq!(store.replaces.load(Ordering::Relaxed), 0);
    assert_eq!(
        service.portable_snapshot().unwrap().revision(),
        before.revision()
    );
}

#[test]
fn publication_phase_can_be_reconstructed_after_a_process_restart() {
    let store = Arc::new(MemorySettingsStore::with([(
        "language",
        serde_json::json!("en"),
    )]));
    let first = SettingsService::new(store.clone(), Arc::new(RecordingEvents::default()));
    let before = first.portable_snapshot().unwrap();
    let desired = br#"{"language":"fr"}"#;
    let preview = first
        .preview_merge(Some(desired), before.revision())
        .unwrap();
    let committed = first
        .replace_portable_deferred(Some(desired), before.revision())
        .unwrap();
    let applied_revision = preview.applied_revision().clone();
    std::mem::forget(committed);

    let batches = Arc::new(RecordingSettingsBatches::default());
    let restarted = SettingsService::with_coordinator(
        store,
        Arc::new(SettingsRuntimeCoordinator::with_batch_sink(batches.clone())),
    );
    let resumed = restarted
        .resume_portable_publication(&applied_revision, preview.publications().to_vec())
        .unwrap();
    resumed.publish().unwrap();

    let published = batches.batches.lock().unwrap();
    assert_eq!(published.len(), 1);
    assert_eq!(
        published[0].publication().revision,
        resumed_revision(&restarted)
    );
    assert_eq!(
        published[0].publications(),
        &[SettingsPublicationEvent::new(
            "markra://language-changed",
            serde_json::json!({ "language": "fr" }),
        )]
    );
}

fn resumed_revision(service: &SettingsService) -> qingyu_kernel::contract::Revision {
    service.read_exposed().unwrap().revision
}

#[test]
fn compatibility_groups_round_trip_through_the_kernel_owner() {
    let golden = portable_golden_store();
    let store = Arc::new(MemorySettingsStore::default());
    let service = SettingsService::new(store, Arc::new(RecordingEvents::default()));
    let cases = [
        (
            SettingsGroup::Appearance,
            serde_json::json!({
                "appearanceMode": golden["appearanceMode"],
                "lightTheme": golden["lightThemeId"],
                "darkTheme": golden["darkThemeId"],
            }),
        ),
        (
            SettingsGroup::CustomThemeCss,
            serde_json::json!({
                "light": golden["lightCustomThemeCss"],
                "dark": golden["darkCustomThemeCss"],
            }),
        ),
        (SettingsGroup::Language, golden["language"].clone()),
        (
            SettingsGroup::EditorPreferences,
            golden["editorPreferences"].clone(),
        ),
        (
            SettingsGroup::FileIgnoreSettings,
            golden["fileIgnoreSettings"].clone(),
        ),
        (
            SettingsGroup::ExportSettings,
            golden["exportSettings"].clone(),
        ),
    ];

    for (group, value) in cases {
        let (stored, deferred) = service.write_group_deferred(group, value.clone()).unwrap();
        assert_eq!(stored, value);
        deferred.supersede().unwrap();
        assert_eq!(service.read_group(group).unwrap(), Some(value));
    }
}

#[test]
fn startup_language_initialization_uses_the_owner_transaction_and_preserves_valid_values() {
    let store = Arc::new(MemorySettingsStore::with([(
        "language",
        serde_json::json!("fr"),
    )]));
    let service = SettingsService::new(store.clone(), Arc::new(RecordingEvents::default()));

    assert!(!service.initialize_language_if_invalid("zh-CN").unwrap());
    assert_eq!(
        store.values.lock().unwrap()["language"],
        serde_json::json!("fr")
    );
    assert_eq!(store.saves.load(Ordering::Relaxed), 0);

    store
        .values
        .lock()
        .unwrap()
        .insert("language".to_string(), serde_json::json!("unsupported"));
    assert!(service.initialize_language_if_invalid("zh-CN").unwrap());
    assert_eq!(
        store.values.lock().unwrap()["language"],
        serde_json::json!("zh-CN")
    );
    assert_eq!(store.saves.load(Ordering::Relaxed), 1);
}

#[test]
fn theme_catalog_migration_reads_legacy_metadata_and_commits_once_with_cas() {
    let store = Arc::new(MemorySettingsStore::with([
        ("themeCatalogVersion", serde_json::json!(0)),
        ("theme", serde_json::json!("dark")),
        ("customThemeCss", serde_json::json!("legacy-css")),
        ("unrelated", serde_json::json!({ "kept": true })),
    ]));
    let service = SettingsService::new(store.clone(), Arc::new(RecordingEvents::default()));

    let snapshot = service.read_theme_catalog_settings().unwrap();
    assert_eq!(snapshot["theme"], serde_json::json!("dark"));
    assert_eq!(snapshot["customThemeCss"], serde_json::json!("legacy-css"));
    assert!(!snapshot.contains_key("unrelated"));
    assert!(service
        .commit_theme_catalog_settings(0, 4, Some(("dark", "paper", "night")))
        .unwrap());
    assert!(!service
        .commit_theme_catalog_settings(0, 4, Some(("light", "other", "other-dark")))
        .unwrap());

    let values = store.values.lock().unwrap();
    assert_eq!(values["themeCatalogVersion"], serde_json::json!(4));
    assert_eq!(values["appearanceMode"], serde_json::json!("dark"));
    assert_eq!(values["lightThemeId"], serde_json::json!("paper"));
    assert_eq!(values["darkThemeId"], serde_json::json!("night"));
    assert_eq!(values["unrelated"]["kept"], serde_json::json!(true));
    drop(values);
    assert_eq!(store.saves.load(Ordering::Relaxed), 1);
}

#[test]
fn theme_catalog_migration_save_failure_restores_every_changed_field() {
    let store = Arc::new(MemorySettingsStore::with([
        ("appearanceMode", serde_json::json!("light")),
        ("lightThemeId", serde_json::json!("old-light")),
        ("darkThemeId", serde_json::json!("old-dark")),
    ]));
    let before = store.values.lock().unwrap().clone();
    store.fail_save.store(true, Ordering::Relaxed);
    let service = SettingsService::new(store.clone(), Arc::new(RecordingEvents::default()));

    let error = service
        .commit_theme_catalog_settings(0, 4, Some(("dark", "new-light", "new-dark")))
        .unwrap_err();

    assert_eq!(error.kind(), SettingsServiceErrorKind::Unavailable);
    assert_eq!(*store.values.lock().unwrap(), before);
    store.fail_save.store(false, Ordering::Relaxed);
    assert!(service.read_exposed().is_ok());
}

#[test]
fn save_failure_restores_the_exact_prior_cache_and_publishes_nothing() {
    let store = Arc::new(MemorySettingsStore::with([
        ("appearanceMode", serde_json::json!("light")),
        ("localOnly", serde_json::json!({ "path": "/private/state" })),
    ]));
    let events = Arc::new(RecordingEvents::default());
    let service = SettingsService::new(store.clone(), events.clone());
    let before = service.read_exposed().unwrap();
    let before_values = store.values.lock().unwrap().clone();
    store.fail_save.store(true, Ordering::Relaxed);
    let patch = serde_json::from_value(serde_json::json!({
        "expectedRevision": before.revision.as_str(),
        "values": [{
            "key": "appearance.mode",
            "value": { "type": "string", "value": "dark" }
        }]
    }))
    .unwrap();

    let error = service.patch_exposed(patch).unwrap_err();

    assert_eq!(error.kind(), SettingsServiceErrorKind::Unavailable);
    assert_eq!(*store.values.lock().unwrap(), before_values);
    assert!(events.publications.lock().unwrap().is_empty());
}

#[test]
fn apply_failure_with_failed_compensation_latches_recovery() {
    let store = Arc::new(MemorySettingsStore::with([
        ("appearanceMode", serde_json::json!("light")),
        ("language", serde_json::json!("en")),
    ]));
    let service = SettingsService::new(store.clone(), Arc::new(RecordingEvents::default()));
    let before = service.read_exposed().unwrap();
    store.fail_set_from_call.store(2, Ordering::Relaxed);
    let patch = serde_json::from_value(serde_json::json!({
        "expectedRevision": before.revision.as_str(),
        "values": [
            {
                "key": "appearance.mode",
                "value": { "type": "string", "value": "dark" }
            },
            {
                "key": "language",
                "value": { "type": "string", "value": "fr" }
            }
        ]
    }))
    .unwrap();

    let error = service.patch_exposed(patch).unwrap_err();

    assert_eq!(error.kind(), SettingsServiceErrorKind::RecoveryRequired);
    assert_eq!(
        service.read_exposed().unwrap_err().kind(),
        SettingsServiceErrorKind::RecoveryRequired
    );
    assert_eq!(
        store.values.lock().unwrap()["appearanceMode"],
        serde_json::json!("dark")
    );
}

#[test]
fn save_failure_with_failed_delete_compensation_latches_recovery() {
    let store = Arc::new(MemorySettingsStore::default());
    let service = SettingsService::new(store.clone(), Arc::new(RecordingEvents::default()));
    let before = service.read_exposed().unwrap();
    store.fail_save.store(true, Ordering::Relaxed);
    store.fail_delete.store(true, Ordering::Relaxed);
    let patch = serde_json::from_value(serde_json::json!({
        "expectedRevision": before.revision.as_str(),
        "values": [{
            "key": "language",
            "value": { "type": "string", "value": "fr" }
        }]
    }))
    .unwrap();

    let error = service.patch_exposed(patch).unwrap_err();

    assert_eq!(error.kind(), SettingsServiceErrorKind::RecoveryRequired);
    assert_eq!(store.saves.load(Ordering::Relaxed), 1);
    assert_eq!(
        service.read_exposed().unwrap_err().kind(),
        SettingsServiceErrorKind::RecoveryRequired
    );
}

#[test]
fn publish_uncertain_save_latches_shared_recovery_and_blocks_a_second_save() {
    let store = Arc::new(MemorySettingsStore::with([(
        "appearanceMode",
        serde_json::json!("light"),
    )]));
    let events = Arc::new(RecordingEvents::default());
    let coordinator = Arc::new(SettingsRuntimeCoordinator::new(events.clone()));
    let first = SettingsService::with_coordinator(store.clone(), coordinator.clone());
    let second = SettingsService::with_coordinator(store.clone(), coordinator);
    let before = first.read_exposed().unwrap();
    store
        .publish_uncertain_on_save
        .store(true, Ordering::Relaxed);
    let patch = serde_json::from_value(serde_json::json!({
        "expectedRevision": before.revision.as_str(),
        "values": [{
            "key": "appearance.mode",
            "value": { "type": "string", "value": "dark" }
        }]
    }))
    .unwrap();

    let error = first.patch_exposed(patch).unwrap_err();

    assert_eq!(error.kind(), SettingsServiceErrorKind::RecoveryRequired);
    assert_eq!(store.saves.load(Ordering::Relaxed), 1);
    assert_eq!(
        store.values.lock().unwrap()["appearanceMode"],
        serde_json::json!("dark")
    );
    assert!(events.publications.lock().unwrap().is_empty());
    store
        .publish_uncertain_on_save
        .store(false, Ordering::Relaxed);
    assert_eq!(
        first.read_exposed().unwrap_err().kind(),
        SettingsServiceErrorKind::RecoveryRequired
    );
    assert_eq!(
        second.read_exposed().unwrap_err().kind(),
        SettingsServiceErrorKind::RecoveryRequired
    );
    let second_patch = serde_json::from_value(serde_json::json!({
        "expectedRevision": before.revision.as_str(),
        "values": [{
            "key": "language",
            "value": { "type": "string", "value": "fr" }
        }]
    }))
    .unwrap();
    assert_eq!(
        second.patch_exposed(second_patch).unwrap_err().kind(),
        SettingsServiceErrorKind::RecoveryRequired
    );
    assert_eq!(store.saves.load(Ordering::Relaxed), 1);

    let rebuilt = SettingsService::new(store, Arc::new(RecordingEvents::default()));
    assert!(rebuilt.read_exposed().is_ok());
}

#[test]
fn event_failure_does_not_undo_or_fail_a_durable_patch() {
    let store = Arc::new(MemorySettingsStore::default());
    let events = Arc::new(RecordingEvents::default());
    events.fail.store(true, Ordering::Relaxed);
    let service = SettingsService::new(store.clone(), events);
    let before = service.read_exposed().unwrap();
    let patch = serde_json::from_value(serde_json::json!({
        "expectedRevision": before.revision.as_str(),
        "values": [{
            "key": "language",
            "value": { "type": "string", "value": "fr" }
        }]
    }))
    .unwrap();

    let committed = service
        .patch_exposed(patch)
        .expect("durable success is authoritative");

    assert_eq!(
        store.values.lock().unwrap()["language"],
        serde_json::json!("fr")
    );
    assert_eq!(service.read_exposed().unwrap(), committed);
}

#[test]
fn key_value_variant_mismatch_is_rejected_before_writing() {
    let store = Arc::new(MemorySettingsStore::default());
    let service = SettingsService::new(store.clone(), Arc::new(RecordingEvents::default()));
    let before = service.read_exposed().unwrap();
    let patch = serde_json::from_value(serde_json::json!({
        "expectedRevision": before.revision.as_str(),
        "values": [{
            "key": "language",
            "value": { "type": "boolean", "value": true }
        }]
    }))
    .unwrap();

    let error = service.patch_exposed(patch).unwrap_err();

    assert_eq!(error.kind(), SettingsServiceErrorKind::InvalidField);
    assert_eq!(store.saves.load(Ordering::Relaxed), 0);
    assert!(store.values.lock().unwrap().is_empty());
}

#[test]
fn empty_duplicate_and_boundary_invalid_patches_never_write() {
    let invalid_values = [
        serde_json::json!([]),
        serde_json::json!([
            {
                "key": "language",
                "value": { "type": "string", "value": "fr" }
            },
            {
                "key": "language",
                "value": { "type": "string", "value": "de" }
            }
        ]),
        serde_json::json!([{
            "key": "theme.customCss.light",
            "value": { "type": "string", "value": "😀".repeat(25_001) }
        }]),
        serde_json::json!([{
            "key": "files.ignoreRules",
            "value": { "type": "string", "value": "é".repeat(25_001) }
        }]),
        serde_json::json!([{
            "key": "editor.fontFamily",
            "value": {
                "type": "font-family",
                "value": { "source": "system", "family": " Inter " }
            }
        }]),
        serde_json::json!([{
            "key": "language",
            "value": { "type": "string", "value": "not-a-language" }
        }]),
    ];

    for values in invalid_values {
        let store = Arc::new(MemorySettingsStore::default());
        let service = SettingsService::new(store.clone(), Arc::new(RecordingEvents::default()));
        let before = service.read_exposed().unwrap();
        let request = serde_json::from_value(serde_json::json!({
            "expectedRevision": before.revision.as_str(),
            "values": values
        }))
        .unwrap();

        let error = service.patch_exposed(request).unwrap_err();

        assert_eq!(error.kind(), SettingsServiceErrorKind::InvalidField);
        assert_eq!(store.saves.load(Ordering::Relaxed), 0);
        assert!(store.values.lock().unwrap().is_empty());
    }
}

fn portable_golden_store() -> Value {
    serde_json::from_str::<Value>(include_str!(
        "../../../packages/app/src/lib/settings/portable-settings.golden.json"
    ))
    .unwrap()["validStore"]
        .clone()
}

#[test]
fn portable_validation_accepts_the_cross_language_golden_and_rejects_local_fields() {
    let golden = portable_golden_store();
    validate_portable_settings_bytes(&serde_json::to_vec(&golden).unwrap()).unwrap();

    for invalid in [
        serde_json::json!({ "workspace": { "path": "/private/notes" } }),
        serde_json::json!({ "exportSettings": { "pandocPath": "/private/bin/pandoc" } }),
    ] {
        assert!(validate_portable_settings_bytes(&serde_json::to_vec(&invalid).unwrap()).is_err());
    }
}

#[test]
fn fresh_editor_and_export_patches_produce_valid_portable_snapshots() {
    let store = Arc::new(MemorySettingsStore::default());
    let service = SettingsService::new(store, Arc::new(RecordingEvents::default()));
    let before = service.read_exposed().unwrap();
    let editor_patch = serde_json::from_value(serde_json::json!({
        "expectedRevision": before.revision.as_str(),
        "values": [{
            "key": "editor.bodyFontSize",
            "value": { "type": "integer", "value": 18 }
        }]
    }))
    .unwrap();

    let after_editor = service.patch_exposed(editor_patch).unwrap();
    let editor_snapshot = service.portable_snapshot().unwrap();
    validate_portable_settings_bytes(editor_snapshot.bytes().unwrap()).unwrap();
    let export_patch = serde_json::from_value(serde_json::json!({
        "expectedRevision": after_editor.revision.as_str(),
        "values": [{
            "key": "export.pdfAuthor",
            "value": { "type": "string", "value": "QingYu" }
        }]
    }))
    .unwrap();

    service.patch_exposed(export_patch).unwrap();
    let combined = service.portable_snapshot().unwrap();
    validate_portable_settings_bytes(combined.bytes().unwrap()).unwrap();
}

#[test]
fn patch_scrubs_polluted_settings_groups_without_publishing_local_paths_or_unknown_secrets() {
    let golden = portable_golden_store();
    let mut editor = golden["editorPreferences"].clone();
    editor["unknownPath"] = serde_json::json!("/private/editor-state");
    editor["viewModeCustomizations"]["secret"] = serde_json::json!("editor-token");
    editor["viewModeCustomizations"]["recentFolders"] = serde_json::json!("hidden");
    editor["markdownTemplates"][0]["suggestedName"] = serde_json::json!("obsolete-name");
    let mut export = golden["exportSettings"].clone();
    export["pandocPath"] = serde_json::json!("/private/bin/pandoc");
    export["secret"] = serde_json::json!("export-token");
    let store = Arc::new(MemorySettingsStore::with([
        ("editorPreferences", editor),
        ("exportSettings", export),
    ]));
    let batches = Arc::new(RecordingSettingsBatches::default());
    let coordinator = Arc::new(SettingsRuntimeCoordinator::with_batch_sink(batches.clone()));
    let service = SettingsService::with_coordinator(store.clone(), coordinator);
    let before = service.read_exposed().unwrap();
    let patch = serde_json::from_value(serde_json::json!({
        "expectedRevision": before.revision.as_str(),
        "values": [
            {
                "key": "editor.bodyFontSize",
                "value": { "type": "integer", "value": 18 }
            },
            {
                "key": "export.pdfAuthor",
                "value": { "type": "string", "value": "QingYu" }
            }
        ]
    }))
    .unwrap();

    let deferred = service.patch_exposed_deferred(patch).unwrap();
    let typed = serde_json::to_string(deferred.settings()).unwrap();
    assert!(!typed.contains("/private"));
    assert!(!typed.contains("token"));
    deferred.publish().unwrap();

    let stored = store.values.lock().unwrap();
    assert!(stored["editorPreferences"].get("unknownPath").is_none());
    assert!(stored["editorPreferences"]["viewModeCustomizations"]
        .get("secret")
        .is_none());
    assert_eq!(
        stored["editorPreferences"]["markdownTemplates"][0],
        serde_json::json!({
            "fileName": "daily.md",
            "id": "daily",
            "name": "Daily"
        })
    );
    assert_eq!(
        stored["editorPreferences"]["viewModeCustomizations"]["recentFolders"],
        serde_json::json!("hidden")
    );
    assert!(stored["exportSettings"].get("secret").is_none());
    assert_eq!(
        stored["exportSettings"]["pandocPath"],
        serde_json::json!("/private/bin/pandoc")
    );
    drop(stored);

    let published = batches.batches.lock().unwrap();
    let published = published
        .iter()
        .map(|batch| {
            let typed = match &batch.publication().event {
                DomainEvent::SettingsChanged { settings } => {
                    serde_json::to_string(settings).unwrap()
                }
                event => panic!("unexpected event: {event:?}"),
            };
            format!(
                "{}{}",
                typed,
                serde_json::to_string(batch.publications()).unwrap()
            )
        })
        .collect::<String>();
    assert!(!published.contains("/private"));
    assert!(!published.contains("token"));
    assert!(!published.contains("pandocPath"));
    assert!(!published.contains("recentFolders"));
    let portable = service.portable_snapshot().unwrap();
    validate_portable_settings_bytes(portable.bytes().unwrap()).unwrap();
    let portable = String::from_utf8(portable.bytes().unwrap().to_vec()).unwrap();
    assert!(!portable.contains("/private"));
    assert!(!portable.contains("token"));
    assert!(!portable.contains("recentFolders"));
}

#[test]
fn portable_replace_preserves_valid_local_only_nested_settings() {
    let golden = portable_golden_store();
    let mut editor = golden["editorPreferences"].clone();
    editor["viewModeCustomizations"]["recentFolders"] = serde_json::json!("hidden");
    let mut export = golden["exportSettings"].clone();
    export["pandocPath"] = serde_json::json!("/private/bin/pandoc");
    let store = Arc::new(MemorySettingsStore::with([
        ("editorPreferences", editor),
        ("exportSettings", export),
    ]));
    let service = SettingsService::new(store.clone(), Arc::new(RecordingEvents::default()));
    let before = service.portable_snapshot().unwrap();
    let desired = serde_json::json!({ "language": "zh-CN" });
    let bytes = serde_json::to_vec(&desired).unwrap();

    service
        .replace_portable_deferred(Some(&bytes), before.revision())
        .unwrap()
        .cancel()
        .unwrap();

    let stored = store.values.lock().unwrap();
    assert_eq!(
        stored["editorPreferences"]["viewModeCustomizations"]["recentFolders"],
        serde_json::json!("hidden")
    );
    assert_eq!(
        stored["exportSettings"]["pandocPath"],
        serde_json::json!("/private/bin/pandoc")
    );
    drop(stored);
    assert_eq!(
        serde_json::from_slice::<Value>(service.portable_snapshot().unwrap().bytes().unwrap())
            .unwrap(),
        desired
    );
}

#[test]
fn portable_snapshot_rejects_an_invalid_local_portable_group() {
    let store = Arc::new(MemorySettingsStore::with([(
        "editorPreferences",
        serde_json::json!({ "bodyFontSize": 16 }),
    )]));
    let service = SettingsService::new(store, Arc::new(RecordingEvents::default()));

    let error = service.portable_snapshot().unwrap_err();

    assert_eq!(error.kind(), SettingsServiceErrorKind::ReconcileFailed);
}

#[test]
fn remote_portable_settings_without_font_family_are_sanitized() {
    let mut remote = portable_golden_store();
    remote["exportSettings"]
        .as_object_mut()
        .unwrap()
        .remove("fontFamily");

    let sanitized = sanitize_legacy_remote_portable_settings(&serde_json::to_vec(&remote).unwrap())
        .expect("valid remote payload")
        .expect("remote payload changed");
    let upgraded: Value = serde_json::from_slice(&sanitized).unwrap();

    assert!(upgraded["exportSettings"]["fontFamily"].is_null());
    validate_portable_settings_bytes(&sanitized).unwrap();
}

#[test]
fn failed_portable_verification_restores_the_previous_snapshot_and_local_fields() {
    let store = Arc::new(MemorySettingsStore::with([
        ("language", serde_json::json!("en")),
        (
            "editorPreferences",
            serde_json::json!({
                "viewModeCustomizations": { "recentFolders": "hidden" }
            }),
        ),
        (
            "exportSettings",
            serde_json::json!({ "pandocPath": "/private/bin/pandoc" }),
        ),
        ("localOnly", serde_json::json!({ "path": "/private/state" })),
    ]));
    store.corrupt_replaces.store(1, Ordering::Relaxed);
    let service = SettingsService::new(store.clone(), Arc::new(RecordingEvents::default()));
    let before = service.portable_snapshot().unwrap();

    let error = service
        .replace_portable_deferred(Some(br#"{"language":"fr"}"#), before.revision())
        .unwrap_err();

    assert_eq!(error.kind(), SettingsServiceErrorKind::ReconcileFailed);
    assert_eq!(store.replaces.load(Ordering::Relaxed), 2);
    let values = store.values.lock().unwrap();
    assert_eq!(values["language"], serde_json::json!("en"));
    assert_eq!(
        values["localOnly"],
        serde_json::json!({ "path": "/private/state" })
    );
    assert_eq!(
        values["editorPreferences"]["viewModeCustomizations"]["recentFolders"],
        serde_json::json!("hidden")
    );
    assert_eq!(
        values["exportSettings"]["pandocPath"],
        serde_json::json!("/private/bin/pandoc")
    );
}

#[test]
fn invalid_portable_verification_restores_the_previous_snapshot() {
    let store = Arc::new(MemorySettingsStore::with([(
        "language",
        serde_json::json!("en"),
    )]));
    store.invalid_replaces.store(1, Ordering::Relaxed);
    let service = SettingsService::new(store.clone(), Arc::new(RecordingEvents::default()));
    let before = service.portable_snapshot().unwrap();

    let error = service
        .replace_portable_deferred(Some(br#"{"language":"fr"}"#), before.revision())
        .unwrap_err();

    assert_eq!(error.kind(), SettingsServiceErrorKind::ReconcileFailed);
    assert_eq!(store.replaces.load(Ordering::Relaxed), 2);
    assert_eq!(
        store.values.lock().unwrap()["language"],
        serde_json::json!("en")
    );
}

#[test]
fn failed_portable_verification_rollback_requires_recovery() {
    let store = Arc::new(MemorySettingsStore::with([(
        "language",
        serde_json::json!("en"),
    )]));
    store.invalid_replaces.store(1, Ordering::Relaxed);
    store.fail_replace_on_call.store(2, Ordering::Relaxed);
    let service = SettingsService::new(store.clone(), Arc::new(RecordingEvents::default()));
    let before = service.portable_snapshot().unwrap();

    let error = service
        .replace_portable_deferred(Some(br#"{"language":"fr"}"#), before.revision())
        .unwrap_err();

    assert_eq!(error.kind(), SettingsServiceErrorKind::RecoveryRequired);
    assert_eq!(store.replaces.load(Ordering::Relaxed), 2);
    assert_eq!(
        store.values.lock().unwrap()["language"],
        serde_json::json!("xx")
    );
    assert_eq!(
        service.read_exposed().unwrap_err().kind(),
        SettingsServiceErrorKind::RecoveryRequired
    );
}

#[test]
fn publish_uncertain_portable_replace_latches_recovery_until_owner_rebuild() {
    let store = Arc::new(MemorySettingsStore::with([(
        "language",
        serde_json::json!("en"),
    )]));
    store
        .publish_uncertain_on_replace
        .store(true, Ordering::Relaxed);
    let events = Arc::new(RecordingEvents::default());
    let service = SettingsService::new(store.clone(), events.clone());
    let before = service.portable_snapshot().unwrap();

    let error = service
        .replace_portable_deferred(Some(br#"{"language":"fr"}"#), before.revision())
        .unwrap_err();

    assert_eq!(error.kind(), SettingsServiceErrorKind::RecoveryRequired);
    assert_eq!(store.replaces.load(Ordering::Relaxed), 1);
    assert_eq!(
        store.values.lock().unwrap()["language"],
        serde_json::json!("fr")
    );
    assert!(events.publications.lock().unwrap().is_empty());
    assert_eq!(
        service.portable_snapshot().unwrap_err().kind(),
        SettingsServiceErrorKind::RecoveryRequired
    );
    let rebuilt = SettingsService::new(store, Arc::new(RecordingEvents::default()));
    assert_eq!(
        rebuilt.read_exposed().unwrap().values[5].value,
        qingyu_kernel::contract::SettingValueDto::String {
            value: "fr".to_string()
        }
    );
}

#[test]
fn unverified_publish_uncertain_portable_replace_requires_recovery() {
    let store = Arc::new(MemorySettingsStore::with([(
        "language",
        serde_json::json!("en"),
    )]));
    store.corrupt_replaces.store(1, Ordering::Relaxed);
    store
        .publish_uncertain_on_replace
        .store(true, Ordering::Relaxed);
    let service = SettingsService::new(store.clone(), Arc::new(RecordingEvents::default()));
    let before = service.portable_snapshot().unwrap();

    let error = service
        .replace_portable_deferred(Some(br#"{"language":"fr"}"#), before.revision())
        .unwrap_err();

    assert_eq!(error.kind(), SettingsServiceErrorKind::RecoveryRequired);
    assert_eq!(store.replaces.load(Ordering::Relaxed), 1);
}

#[test]
fn portable_publication_is_deferred_and_keeps_legacy_event_order() {
    let store = Arc::new(MemorySettingsStore::with([
        ("appearanceMode", serde_json::json!("light")),
        ("language", serde_json::json!("en")),
    ]));
    let batches = Arc::new(RecordingSettingsBatches::default());
    let coordinator = Arc::new(SettingsRuntimeCoordinator::with_batch_sink(batches.clone()));
    let service = SettingsService::with_coordinator(store, coordinator);
    let before = service.portable_snapshot().unwrap();

    let deferred = service
        .replace_portable_deferred(
            Some(br#"{"appearanceMode":"dark","language":"fr"}"#),
            before.revision(),
        )
        .unwrap();

    assert!(batches.batches.lock().unwrap().is_empty());
    deferred.publish().unwrap();
    let published = batches.batches.lock().unwrap();
    assert_eq!(published.len(), 1);
    assert_eq!(
        published[0]
            .publications()
            .iter()
            .map(|event| event.event_name())
            .collect::<Vec<_>>(),
        vec!["markra://theme-changed", "markra://language-changed"]
    );
}

#[test]
fn portable_preview_is_read_only_and_reports_the_applied_revision() {
    let store = Arc::new(MemorySettingsStore::with([(
        "language",
        serde_json::json!("en"),
    )]));
    let events = Arc::new(RecordingEvents::default());
    let service = SettingsService::new(store.clone(), events.clone());
    let before = service.portable_snapshot().unwrap();

    let preview = service
        .preview_merge(Some(br#"{"language":"fr"}"#), before.revision())
        .unwrap();

    assert_eq!(
        preview.applied_revision().as_str(),
        format!("sha256:{:x}", sha2::Sha256::digest(br#"{"language":"fr"}"#))
    );
    assert_eq!(
        preview.publications()[0].event_name(),
        "markra://language-changed"
    );
    assert_eq!(
        store.values.lock().unwrap()["language"],
        serde_json::json!("en")
    );
    assert_eq!(store.replaces.load(Ordering::Relaxed), 0);
    assert!(events.publications.lock().unwrap().is_empty());
}

#[tokio::test]
async fn api_service_and_direct_read_return_the_same_snapshot() {
    let service = SettingsService::new(
        Arc::new(MemorySettingsStore::with([(
            "language",
            serde_json::json!("fr"),
        )])),
        Arc::new(RecordingEvents::default()),
    );

    let direct = service.read_exposed().unwrap();
    let api = SettingsApiService::get_settings(&service).await.unwrap();

    assert_eq!(api, direct);
}

#[tokio::test]
async fn http_get_maps_missing_file_ignore_rules_to_the_frozen_unavailable_error() {
    let root = tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let app_data = root.path().join("app-data");
    let cache = root.path().join("cache");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&app_data).unwrap();
    std::fs::create_dir(&cache).unwrap();
    let config = KernelConfig::generate().unwrap();
    let credential = config.native_launch_credential().expose_secret().to_owned();
    let runtime = KernelRuntime::activate(
        config,
        KernelPaths::desktop(&workspace, &app_data, &cache).unwrap(),
        KernelPorts::unavailable(),
    )
    .unwrap();
    runtime
        .install_settings_api_service(Arc::new(SettingsService::new(
            Arc::new(MemorySettingsStore::with([(
                "fileIgnoreSettings",
                serde_json::json!({}),
            )])),
            Arc::new(RecordingEvents::default()),
        )))
        .unwrap();
    let router = build_router(runtime, TransportPolicy::loopback(HOST, ORIGIN).unwrap());
    let request = Request::builder()
        .method("GET")
        .uri("/api/v1/settings")
        .header(header::HOST, HOST)
        .header(header::AUTHORIZATION, format!("Bearer {credential}"))
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let envelope: ApiErrorEnvelope = serde_json::from_slice(
        &to_bytes(response.into_body(), 2 * 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(envelope.code(), ErrorCode::SettingsUnavailable);
}

#[tokio::test]
async fn direct_and_http_get_patch_have_identical_dtos_and_revisions() {
    let root = tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let app_data = root.path().join("app-data");
    let cache = root.path().join("cache");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&app_data).unwrap();
    std::fs::create_dir(&cache).unwrap();
    let config = KernelConfig::generate().unwrap();
    let credential = config.native_launch_credential().expose_secret().to_owned();
    let runtime = KernelRuntime::activate(
        config,
        KernelPaths::desktop(&workspace, &app_data, &cache).unwrap(),
        KernelPorts::unavailable(),
    )
    .unwrap();
    let http_service = Arc::new(SettingsService::new(
        Arc::new(MemorySettingsStore::default()),
        Arc::new(RecordingEvents::default()),
    ));
    runtime
        .install_settings_api_service(http_service.clone())
        .unwrap();
    let router = build_router(runtime, TransportPolicy::loopback(HOST, ORIGIN).unwrap());
    let direct_service = SettingsService::new(
        Arc::new(MemorySettingsStore::default()),
        Arc::new(RecordingEvents::default()),
    );
    let direct_before = direct_service.read_exposed().unwrap();

    let get = Request::builder()
        .method("GET")
        .uri("/api/v1/settings")
        .header(header::HOST, HOST)
        .header(header::AUTHORIZATION, format!("Bearer {credential}"))
        .body(Body::empty())
        .unwrap();
    let response = router.clone().oneshot(get).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let http_before: qingyu_kernel::contract::SettingsSnapshotDto = serde_json::from_slice(
        &to_bytes(response.into_body(), 2 * 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(http_before, direct_before);

    let patch: qingyu_kernel::contract::PatchSettingsRequest =
        serde_json::from_value(serde_json::json!({
            "expectedRevision": direct_before.revision.as_str(),
            "values": [{
                "key": "language",
                "value": { "type": "string", "value": "fr" }
            }]
        }))
        .unwrap();
    let direct_after = direct_service.patch_exposed(patch.clone()).unwrap();
    let request = Request::builder()
        .method("PATCH")
        .uri("/api/v1/settings")
        .header(header::HOST, HOST)
        .header(header::AUTHORIZATION, format!("Bearer {credential}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&patch).unwrap()))
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let http_after: qingyu_kernel::contract::SettingsSnapshotDto = serde_json::from_slice(
        &to_bytes(response.into_body(), 2 * 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(http_after, direct_after);
}
