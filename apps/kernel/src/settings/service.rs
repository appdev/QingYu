//! Settings service implementation boundary.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::{
    contract::{PatchSettingsRequest, Revision, SettingsSnapshotDto},
    events::EventSink,
    settings::{
        model::{
            default_editor, default_export, merge_defaults, normalize_portable_value,
            portable_revision, portable_settings_from_bytes, snapshot_from_values, storage_target,
            validate_portable_settings_bytes, validated_raw_value, PortableSettingsSnapshot,
            PORTABLE_SETTINGS_KEYS,
        },
        storage::{SettingsStore, SettingsStoreError, SettingsStoreErrorKind},
    },
};

#[derive(Clone, Debug, PartialEq)]
pub struct SettingsPublicationBatch {
    publication: crate::events::EventPublication,
    publications: Vec<SettingsPublicationEvent>,
}

impl SettingsPublicationBatch {
    pub const fn publication(&self) -> &crate::events::EventPublication {
        &self.publication
    }

    pub fn publications(&self) -> &[SettingsPublicationEvent] {
        &self.publications
    }
}

pub trait SettingsPublicationBatchSink: Send + Sync {
    fn publish(
        &self,
        batch: &SettingsPublicationBatch,
    ) -> Result<(), crate::events::EventSinkError>;
}

struct TypedEventBatchSink {
    events: Arc<dyn EventSink>,
}

impl SettingsPublicationBatchSink for TypedEventBatchSink {
    fn publish(
        &self,
        batch: &SettingsPublicationBatch,
    ) -> Result<(), crate::events::EventSinkError> {
        self.events.publish(batch.publication())
    }
}

pub struct SettingsRuntimeCoordinator {
    transaction_gate: Arc<Mutex<()>>,
    batch_sink: Arc<dyn SettingsPublicationBatchSink>,
    publications: Mutex<PublicationCoordinatorState>,
}

impl SettingsRuntimeCoordinator {
    pub fn new(events: Arc<dyn EventSink>) -> Self {
        Self::with_transaction_gate(events, Arc::new(Mutex::new(())))
    }

    pub fn with_transaction_gate(
        events: Arc<dyn EventSink>,
        transaction_gate: Arc<Mutex<()>>,
    ) -> Self {
        Self::with_batch_sink_and_transaction_gate(
            Arc::new(TypedEventBatchSink { events }),
            transaction_gate,
        )
    }

    pub fn with_batch_sink(batch_sink: Arc<dyn SettingsPublicationBatchSink>) -> Self {
        Self::with_batch_sink_and_transaction_gate(batch_sink, Arc::new(Mutex::new(())))
    }

    pub fn with_batch_sink_and_transaction_gate(
        batch_sink: Arc<dyn SettingsPublicationBatchSink>,
        transaction_gate: Arc<Mutex<()>>,
    ) -> Self {
        Self {
            transaction_gate,
            batch_sink,
            publications: Mutex::new(PublicationCoordinatorState::default()),
        }
    }

    pub fn transaction_gate(&self) -> Arc<Mutex<()>> {
        self.transaction_gate.clone()
    }

    pub fn transaction_gate_ref(&self) -> &Mutex<()> {
        self.transaction_gate.as_ref()
    }

    fn register_publication(
        &self,
        batch: SettingsPublicationBatch,
    ) -> Result<u64, SettingsServiceError> {
        let mut state = self
            .publications
            .lock()
            .map_err(|_| SettingsServiceError::unavailable())?;
        if state.recovery_required {
            return Err(SettingsServiceError::recovery_required());
        }
        let ticket = state.next_ticket;
        state.next_ticket += 1;
        state.pending.insert(
            ticket,
            PendingPublication {
                batch,
                disposition: PublicationDisposition::Awaiting,
            },
        );
        Ok(ticket)
    }

    fn mark_publication(
        &self,
        ticket: u64,
        disposition: PublicationDisposition,
    ) -> Result<(), crate::events::EventSinkError> {
        let should_drain = self.prepare_publication(ticket, disposition)?;
        if should_drain {
            self.drain_publications()
        } else {
            Ok(())
        }
    }

    fn prepare_publication(
        &self,
        ticket: u64,
        disposition: PublicationDisposition,
    ) -> Result<bool, crate::events::EventSinkError> {
        {
            let mut state = self
                .publications
                .lock()
                .map_err(|_| crate::events::EventSinkError)?;
            if state.recovery_required {
                return Err(crate::events::EventSinkError);
            }
            if ticket < state.next_publication {
                return Ok(false);
            }
            let pending = state
                .pending
                .get_mut(&ticket)
                .ok_or(crate::events::EventSinkError)?;
            pending.disposition = disposition;
            let should_drain = if state.draining
                || state
                    .pending
                    .get(&state.next_publication)
                    .is_none_or(|pending| pending.disposition == PublicationDisposition::Awaiting)
            {
                false
            } else {
                state.draining = true;
                true
            };
            Ok(should_drain)
        }
    }

    fn drain_publications(&self) -> Result<(), crate::events::EventSinkError> {
        let mut first_error = None;
        loop {
            let next = {
                let mut state = self
                    .publications
                    .lock()
                    .map_err(|_| crate::events::EventSinkError)?;
                let next_ticket = state.next_publication;
                match state.pending.get(&next_ticket) {
                    Some(pending) if pending.disposition != PublicationDisposition::Awaiting => {
                        let pending = state
                            .pending
                            .remove(&next_ticket)
                            .expect("checked pending publication exists");
                        state.next_publication += 1;
                        Some(pending)
                    }
                    _ => {
                        state.draining = false;
                        None
                    }
                }
            };
            let Some(next) = next else {
                break;
            };
            if next.disposition == PublicationDisposition::Ready
                && self.batch_sink.publish(&next.batch).is_err()
                && first_error.is_none()
            {
                first_error = Some(crate::events::EventSinkError);
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    pub(crate) fn ensure_available(&self) -> Result<(), SettingsServiceError> {
        let state = self
            .publications
            .lock()
            .map_err(|_| SettingsServiceError::unavailable())?;
        if state.recovery_required {
            Err(SettingsServiceError::recovery_required())
        } else {
            Ok(())
        }
    }

    pub(crate) fn require_recovery(&self) {
        if let Ok(mut state) = self.publications.lock() {
            state.recovery_required = true;
        }
    }
}

struct PendingPublication {
    batch: SettingsPublicationBatch,
    disposition: PublicationDisposition,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum PublicationDisposition {
    Awaiting,
    Cancelled,
    Ready,
}

struct PublicationCoordinatorState {
    draining: bool,
    next_publication: u64,
    next_ticket: u64,
    pending: BTreeMap<u64, PendingPublication>,
    recovery_required: bool,
}

impl Default for PublicationCoordinatorState {
    fn default() -> Self {
        Self {
            draining: false,
            next_publication: 1,
            next_ticket: 1,
            pending: BTreeMap::new(),
            recovery_required: false,
        }
    }
}

pub struct SettingsService {
    store: Arc<dyn SettingsStore>,
    coordinator: Arc<SettingsRuntimeCoordinator>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SettingsGroup {
    Appearance,
    CustomThemeCss,
    Language,
    EditorPreferences,
    FileIgnoreSettings,
    ExportSettings,
}

const THEME_CATALOG_SETTINGS_KEYS: &[&str] = &[
    "themeCatalogVersion",
    "theme",
    "appearanceMode",
    "lightCustomThemeCss",
    "customThemeCss",
    "darkCustomThemeCss",
    "lightThemeId",
    "darkThemeId",
];

impl SettingsService {
    pub fn new(store: Arc<dyn SettingsStore>, events: Arc<dyn EventSink>) -> Self {
        Self::with_coordinator(store, Arc::new(SettingsRuntimeCoordinator::new(events)))
    }

    /// Builds a service sharing transaction, recovery, and publication ordering state.
    /// Every service sharing one store must receive the same coordinator.
    pub fn with_coordinator(
        store: Arc<dyn SettingsStore>,
        coordinator: Arc<SettingsRuntimeCoordinator>,
    ) -> Self {
        Self { store, coordinator }
    }

    pub fn read_exposed(&self) -> Result<SettingsSnapshotDto, SettingsServiceError> {
        let _transaction = self
            .coordinator
            .transaction_gate
            .lock()
            .map_err(|_| SettingsServiceError::unavailable())?;
        self.coordinator.ensure_available()?;
        self.read_exposed_unlocked()
    }

    pub fn initialize_language_if_invalid(
        &self,
        language: &str,
    ) -> Result<bool, SettingsServiceError> {
        let language = Value::String(language.to_string());
        if !valid_single_portable_value("language", &language) {
            return Err(SettingsServiceError::invalid());
        }
        let _transaction = self
            .coordinator
            .transaction_gate
            .lock()
            .map_err(|_| SettingsServiceError::unavailable())?;
        self.coordinator.ensure_available()?;
        let previous = self.store.get("language")?;
        if previous
            .as_ref()
            .is_some_and(|value| valid_single_portable_value("language", value))
        {
            return Ok(false);
        }
        if let Err(error) = self.store.set("language", language) {
            let rollback = match previous.as_ref() {
                Some(value) => self.store.set("language", value.clone()),
                None => self.store.delete("language"),
            };
            if rollback.is_err() {
                self.coordinator.require_recovery();
                return Err(SettingsServiceError::recovery_required());
            }
            return Err(error.into());
        }
        if let Err(error) = self.store.save() {
            if error.kind() == SettingsStoreErrorKind::PublishUncertain {
                self.coordinator.require_recovery();
                return Err(SettingsServiceError::recovery_required());
            }
            let rollback = match previous {
                Some(value) => self.store.set("language", value),
                None => self.store.delete("language"),
            };
            if rollback.is_err() {
                self.coordinator.require_recovery();
                return Err(SettingsServiceError::recovery_required());
            }
            return Err(error.into());
        }
        Ok(true)
    }

    pub fn read_theme_catalog_settings(
        &self,
    ) -> Result<BTreeMap<String, Value>, SettingsServiceError> {
        let _transaction = self
            .coordinator
            .transaction_gate
            .lock()
            .map_err(|_| SettingsServiceError::unavailable())?;
        self.coordinator.ensure_available()?;
        let mut values = BTreeMap::new();
        for key in THEME_CATALOG_SETTINGS_KEYS {
            if let Some(value) = self.store.get(key)? {
                values.insert((*key).to_string(), value);
            }
        }
        Ok(values)
    }

    pub fn commit_theme_catalog_settings(
        &self,
        expected_catalog_version: i64,
        catalog_version: i64,
        appearance: Option<(&str, &str, &str)>,
    ) -> Result<bool, SettingsServiceError> {
        if expected_catalog_version < 0 || catalog_version < 0 {
            return Err(SettingsServiceError::invalid());
        }
        let mut changes = BTreeMap::from([(
            "themeCatalogVersion".to_string(),
            Value::from(catalog_version),
        )]);
        if let Some((appearance_mode, light_theme_id, dark_theme_id)) = appearance {
            let portable_values = [
                ("appearanceMode", Value::String(appearance_mode.to_string())),
                ("lightThemeId", Value::String(light_theme_id.to_string())),
                ("darkThemeId", Value::String(dark_theme_id.to_string())),
            ];
            if portable_values
                .iter()
                .any(|(key, value)| !valid_single_portable_value(key, value))
            {
                return Err(SettingsServiceError::invalid());
            }
            changes.extend(
                portable_values
                    .into_iter()
                    .map(|(key, value)| (key.to_string(), value)),
            );
        }

        let _transaction = self
            .coordinator
            .transaction_gate
            .lock()
            .map_err(|_| SettingsServiceError::unavailable())?;
        self.coordinator.ensure_available()?;
        let current_version = self
            .store
            .get("themeCatalogVersion")?
            .and_then(|value| value.as_i64())
            .unwrap_or(0)
            .max(0);
        if current_version != expected_catalog_version {
            return Ok(false);
        }
        let mut previous = BTreeMap::new();
        for key in changes.keys() {
            previous.insert(key.clone(), self.store.get(key)?);
        }
        for (key, value) in &changes {
            if let Err(error) = self.store.set(key, value.clone()) {
                if self.restore(&previous).is_err() {
                    self.coordinator.require_recovery();
                    return Err(SettingsServiceError::recovery_required());
                }
                return Err(error.into());
            }
        }
        if let Err(error) = self.store.save() {
            if error.kind() == SettingsStoreErrorKind::PublishUncertain {
                self.coordinator.require_recovery();
                return Err(SettingsServiceError::recovery_required());
            }
            if self.restore(&previous).is_err() {
                self.coordinator.require_recovery();
                return Err(SettingsServiceError::recovery_required());
            }
            return Err(error.into());
        }
        Ok(true)
    }

    pub(crate) fn read_exposed_unlocked(
        &self,
    ) -> Result<SettingsSnapshotDto, SettingsServiceError> {
        let mut values = BTreeMap::new();
        values.insert(
            "appearance.mode".to_string(),
            self.store
                .get("appearanceMode")?
                .unwrap_or_else(|| json!("system")),
        );
        values.insert(
            "appearance.lightTheme".to_string(),
            self.store
                .get("lightThemeId")?
                .unwrap_or_else(|| json!(super::model::DEFAULT_LIGHT_THEME_ID)),
        );
        values.insert(
            "appearance.darkTheme".to_string(),
            self.store
                .get("darkThemeId")?
                .unwrap_or_else(|| json!(super::model::DEFAULT_DARK_THEME_ID)),
        );
        values.insert(
            "theme.customCss.light".to_string(),
            self.store
                .get("lightCustomThemeCss")?
                .unwrap_or_else(|| json!("")),
        );
        values.insert(
            "theme.customCss.dark".to_string(),
            self.store
                .get("darkCustomThemeCss")?
                .unwrap_or_else(|| json!("")),
        );
        values.insert(
            "language".to_string(),
            self.store.get("language")?.unwrap_or_else(|| json!("en")),
        );

        let editor = merge_defaults(default_editor(), self.store.get("editorPreferences")?);
        for (field, key) in [
            ("editor.bodyFontSize", "bodyFontSize"),
            ("editor.contentWidth", "contentWidth"),
            ("editor.contentWidthPx", "contentWidthPx"),
            ("editor.fontFamily", "editorFontFamily"),
            ("editor.lineHeight", "lineHeight"),
            ("editor.paragraphSpacingPx", "paragraphSpacingPx"),
            ("editor.showWordCount", "showWordCount"),
            ("editor.wrapCodeBlocks", "wrapCodeBlocks"),
            ("editor.viewMode", "viewMode"),
        ] {
            values.insert(
                field.to_string(),
                editor
                    .get(key)
                    .cloned()
                    .ok_or(SettingsServiceError::invalid())?,
            );
        }
        let files = match self.store.get("fileIgnoreSettings")? {
            None => json!({ "rules": "" })
                .as_object()
                .cloned()
                .expect("file ignore defaults are an object"),
            Some(Value::Object(files)) => files,
            Some(_) => return Err(SettingsServiceError::unavailable()),
        };
        values.insert(
            "files.ignoreRules".to_string(),
            files
                .get("rules")
                .cloned()
                .ok_or(SettingsServiceError::unavailable())?,
        );

        let mut export = merge_defaults(default_export(), self.store.get("exportSettings")?);
        export.remove("pandocPath");
        for key in [
            "fontFamily",
            "pdfAuthor",
            "pdfFooter",
            "pdfHeader",
            "pdfHeightMm",
            "pdfMarginMm",
            "pdfMarginPreset",
            "pdfPageBreakOnH1",
            "pdfPageSize",
            "pdfWidthMm",
        ] {
            values.insert(
                format!("export.{key}"),
                export
                    .get(key)
                    .cloned()
                    .ok_or(SettingsServiceError::invalid())?,
            );
        }

        snapshot_from_values(&values).map_err(|_| SettingsServiceError::unavailable())
    }

    pub fn portable_snapshot(&self) -> Result<PortableSettingsSnapshot, SettingsServiceError> {
        let _transaction = self
            .coordinator
            .transaction_gate
            .lock()
            .map_err(|_| SettingsServiceError::unavailable())?;
        self.coordinator.ensure_available()?;
        self.portable_snapshot_unlocked()
    }

    pub fn read_group(&self, group: SettingsGroup) -> Result<Option<Value>, SettingsServiceError> {
        let _transaction = self
            .coordinator
            .transaction_gate
            .lock()
            .map_err(|_| SettingsServiceError::unavailable())?;
        self.coordinator.ensure_available()?;
        self.read_group_unlocked(group)
    }

    fn read_group_unlocked(
        &self,
        group: SettingsGroup,
    ) -> Result<Option<Value>, SettingsServiceError> {
        match group {
            SettingsGroup::Appearance => {
                let mode = self.store.get("appearanceMode")?;
                let light = self.store.get("lightThemeId")?;
                let dark = self.store.get("darkThemeId")?;
                if mode.is_none() && light.is_none() && dark.is_none() {
                    return Ok(None);
                }
                Ok(Some(json!({
                    "appearanceMode": mode.unwrap_or_else(|| json!("system")),
                    "lightTheme": light.unwrap_or_else(|| json!(super::model::DEFAULT_LIGHT_THEME_ID)),
                    "darkTheme": dark.unwrap_or_else(|| json!(super::model::DEFAULT_DARK_THEME_ID)),
                })))
            }
            SettingsGroup::CustomThemeCss => {
                let light = self.store.get("lightCustomThemeCss")?;
                let dark = self.store.get("darkCustomThemeCss")?;
                if light.is_none() && dark.is_none() {
                    return Ok(None);
                }
                Ok(Some(json!({
                    "light": light.unwrap_or_else(|| json!("")),
                    "dark": dark.unwrap_or_else(|| json!("")),
                })))
            }
            SettingsGroup::Language => self.store.get("language").map_err(Into::into),
            SettingsGroup::EditorPreferences => self
                .store
                .get("editorPreferences")
                .map(|value| value.map(|value| editor_publication_value(Some(value))))
                .map_err(Into::into),
            SettingsGroup::FileIgnoreSettings => {
                self.store.get("fileIgnoreSettings").map_err(Into::into)
            }
            SettingsGroup::ExportSettings => self
                .store
                .get("exportSettings")
                .map(|value| value.map(|value| export_publication_value(Some(value))))
                .map_err(Into::into),
        }
    }

    pub fn write_group_deferred(
        &self,
        group: SettingsGroup,
        value: Value,
    ) -> Result<(Value, DeferredSettingsPublication), SettingsServiceError> {
        let _transaction = self
            .coordinator
            .transaction_gate
            .lock()
            .map_err(|_| SettingsServiceError::unavailable())?;
        self.coordinator.ensure_available()?;
        let before = self.portable_snapshot_unlocked()?;
        let before_value = portable_settings_from_bytes(before.bytes())
            .map_err(|_| SettingsServiceError::reconcile_failed())?;
        let mut desired = before_value.clone();
        apply_settings_group(
            desired
                .as_object_mut()
                .ok_or_else(SettingsServiceError::reconcile_failed)?,
            group,
            value,
        )?;
        let desired_bytes =
            serde_json::to_vec(&desired).map_err(|_| SettingsServiceError::reconcile_failed())?;
        validate_portable_settings_bytes(&desired_bytes)
            .map_err(|_| SettingsServiceError::invalid())?;
        let deferred = self.replace_portable_value_unlocked(&desired, &before_value, |_| true)?;
        let stored = self
            .read_group_unlocked(group)?
            .ok_or_else(SettingsServiceError::reconcile_failed)?;
        Ok((stored, deferred))
    }

    fn portable_snapshot_unlocked(&self) -> Result<PortableSettingsSnapshot, SettingsServiceError> {
        let portable = self.portable_value_unlocked()?;
        let portable = portable
            .as_object()
            .expect("portable settings values are always an object");
        let bytes = if portable.is_empty() {
            None
        } else {
            let bytes = serde_json::to_vec(&Value::Object(portable.clone()))
                .map_err(|_| SettingsServiceError::unavailable())?;
            validate_portable_settings_bytes(&bytes)
                .map_err(|_| SettingsServiceError::reconcile_failed())?;
            Some(bytes)
        };
        PortableSettingsSnapshot::new(bytes).map_err(|_| SettingsServiceError::unavailable())
    }

    fn portable_value_unlocked(&self) -> Result<Value, SettingsServiceError> {
        let mut portable = Map::new();
        for key in PORTABLE_SETTINGS_KEYS {
            if let Some(value) = self.store.get(key)? {
                if is_local_only_storage_group(key, &value) {
                    continue;
                }
                let normalized = portable_publication_value(key, value);
                portable.insert(key.to_string(), normalized);
            }
        }
        Ok(Value::Object(portable))
    }

    fn portable_storage_value_unlocked(&self) -> Result<Value, SettingsServiceError> {
        let mut portable = Map::new();
        for key in PORTABLE_SETTINGS_KEYS {
            if let Some(value) = self.store.get(key)? {
                portable.insert(key.to_string(), value);
            }
        }
        Ok(Value::Object(portable))
    }

    pub fn patch_exposed(
        &self,
        request: PatchSettingsRequest,
    ) -> Result<SettingsSnapshotDto, SettingsServiceError> {
        let deferred = self.patch_exposed_transaction(request)?;
        let committed = deferred.settings().clone();
        let _publication_result = deferred.publish();
        Ok(committed)
    }

    pub fn patch_exposed_deferred(
        &self,
        request: PatchSettingsRequest,
    ) -> Result<DeferredSettingsPublication, SettingsServiceError> {
        self.patch_exposed_transaction(request)
    }

    fn patch_exposed_transaction(
        &self,
        request: PatchSettingsRequest,
    ) -> Result<DeferredSettingsPublication, SettingsServiceError> {
        let _transaction = self
            .coordinator
            .transaction_gate
            .lock()
            .map_err(|_| SettingsServiceError::unavailable())?;
        self.coordinator.ensure_available()?;
        request
            .validate()
            .map_err(|_| SettingsServiceError::invalid())?;
        let current = self.read_exposed_unlocked()?;
        if current.revision != request.expected_revision {
            return Err(SettingsServiceError::revision_conflict(current.revision));
        }
        let before_value = self.portable_value_unlocked()?;

        let mut changes = BTreeMap::<String, Value>::new();
        for entry in request.values {
            let key = entry.key;
            let raw = validated_raw_value(entry).map_err(|_| SettingsServiceError::invalid())?;
            let (store_key, nested_key) = storage_target(key);
            match nested_key {
                None => {
                    changes.insert(store_key.to_string(), raw);
                }
                Some(nested_key) => {
                    if !changes.contains_key(store_key) {
                        let current = self.store.get(store_key)?;
                        let values = match store_key {
                            "editorPreferences" => editor_storage_values(current),
                            "fileIgnoreSettings" => json!({ "rules": "" })
                                .as_object()
                                .cloned()
                                .expect("file ignore defaults are an object"),
                            "exportSettings" => export_storage_values(current),
                            _ => return Err(SettingsServiceError::invalid()),
                        };
                        changes.insert(store_key.to_string(), Value::Object(values));
                    }
                    changes
                        .get_mut(store_key)
                        .and_then(Value::as_object_mut)
                        .ok_or_else(SettingsServiceError::invalid)?
                        .insert(nested_key.to_string(), raw);
                }
            }
        }

        let mut previous = BTreeMap::new();
        for key in changes.keys() {
            previous.insert(key.clone(), self.store.get(key)?);
        }
        for (key, value) in &changes {
            if let Err(error) = self.store.set(key, value.clone()) {
                if self.restore(&previous).is_err() {
                    self.coordinator.require_recovery();
                    return Err(SettingsServiceError::recovery_required());
                }
                return Err(error.into());
            }
        }
        if let Err(error) = self.store.save() {
            if error.kind() == SettingsStoreErrorKind::PublishUncertain {
                self.coordinator.require_recovery();
                return Err(SettingsServiceError::recovery_required());
            }
            if self.restore(&previous).is_err() {
                self.coordinator.require_recovery();
                return Err(SettingsServiceError::recovery_required());
            }
            return Err(error.into());
        }

        let committed = self.read_exposed_unlocked()?;
        let publication = crate::events::EventPublication {
            resource: crate::contract::ResourceRefDto::Settings {},
            revision: committed.revision.clone(),
            event: crate::contract::DomainEvent::SettingsChanged {
                settings: committed.clone(),
            },
        };
        let after_value = self.portable_value_unlocked()?;
        let legacy_publications = legacy_change_publications(&before_value, &after_value)?;
        let ticket = self
            .coordinator
            .register_publication(SettingsPublicationBatch {
                publication,
                publications: legacy_publications,
            })?;
        Ok(DeferredSettingsPublication {
            coordinator: self.coordinator.clone(),
            settings: committed,
            settled: false,
            ticket,
        })
    }

    pub fn replace_portable_deferred(
        &self,
        bytes: Option<&[u8]>,
        expected_revision: &Revision,
    ) -> Result<DeferredSettingsPublication, SettingsServiceError> {
        self.replace_portable_deferred_with_preflight(bytes, expected_revision, || true)
    }

    pub fn replace_portable_deferred_with_preflight<Preflight>(
        &self,
        bytes: Option<&[u8]>,
        expected_revision: &Revision,
        preflight: Preflight,
    ) -> Result<DeferredSettingsPublication, SettingsServiceError>
    where
        Preflight: FnOnce() -> bool,
    {
        self.replace_portable_deferred_with_preflight_and_verify(
            bytes,
            expected_revision,
            preflight,
            |_| true,
        )
    }

    pub fn replace_portable_deferred_with_preflight_and_verify<Preflight, Verify>(
        &self,
        bytes: Option<&[u8]>,
        expected_revision: &Revision,
        preflight: Preflight,
        verify: Verify,
    ) -> Result<DeferredSettingsPublication, SettingsServiceError>
    where
        Preflight: FnOnce() -> bool,
        Verify: FnOnce(&Value) -> bool,
    {
        let desired = portable_settings_from_bytes(bytes)
            .map_err(|_| SettingsServiceError::reconcile_failed())?;
        let _transaction = self
            .coordinator
            .transaction_gate
            .lock()
            .map_err(|_| SettingsServiceError::unavailable())?;
        self.coordinator.ensure_available()?;
        let before = self.portable_snapshot_unlocked()?;
        if before.revision() != expected_revision {
            return Err(SettingsServiceError::reconcile_failed());
        }
        let before_value = portable_settings_from_bytes(before.bytes())
            .map_err(|_| SettingsServiceError::reconcile_failed())?;
        if !preflight() {
            return Err(SettingsServiceError::reconcile_failed());
        }
        self.replace_portable_value_unlocked(&desired, &before_value, verify)
    }

    fn replace_portable_value_unlocked<Verify>(
        &self,
        desired: &Value,
        before_value: &Value,
        verify: Verify,
    ) -> Result<DeferredSettingsPublication, SettingsServiceError>
    where
        Verify: FnOnce(&Value) -> bool,
    {
        let desired_object = desired
            .as_object()
            .ok_or_else(SettingsServiceError::reconcile_failed)?;
        let before_storage_value = self.portable_storage_value_unlocked()?;
        let mut storage_desired = desired_object.clone();
        preserve_local_only_settings(
            &mut storage_desired,
            self.store.get("editorPreferences")?,
            self.store.get("exportSettings")?,
        );

        // Reconciliation still needs a deferred publication ticket when the
        // portable values are unchanged, but it must not rewrite the local
        // backing file merely to publish the current canonical snapshot.
        let storage_replaced = desired != before_value;
        if storage_replaced {
            match self.store.replace_portable_atomically(&storage_desired) {
                Ok(()) => {}
                Err(error) if error.kind() == SettingsStoreErrorKind::PublishUncertain => {
                    self.coordinator.require_recovery();
                    return Err(SettingsServiceError::recovery_required());
                }
                Err(error) => return Err(error.into()),
            }
        }
        let actual = self
            .portable_snapshot_unlocked()
            .and_then(|after| {
                portable_settings_from_bytes(after.bytes())
                    .map_err(|_| SettingsServiceError::reconcile_failed())
            })
            .and_then(|actual| {
                (actual == *desired)
                    .then_some(actual)
                    .ok_or_else(SettingsServiceError::reconcile_failed)
            });
        let actual = match actual {
            Ok(actual) => actual,
            Err(_) => {
                if storage_replaced
                    && self
                        .restore_portable_storage(&before_storage_value)
                        .is_err()
                {
                    self.coordinator.require_recovery();
                    return Err(SettingsServiceError::recovery_required());
                }
                return Err(SettingsServiceError::reconcile_failed());
            }
        };
        if !verify(&actual) {
            if storage_replaced
                && self
                    .restore_portable_storage(&before_storage_value)
                    .is_err()
            {
                self.coordinator.require_recovery();
                return Err(SettingsServiceError::recovery_required());
            }
            return Err(SettingsServiceError::reconcile_failed());
        }

        let committed = self.read_exposed_unlocked()?;
        let publication = crate::events::EventPublication {
            resource: crate::contract::ResourceRefDto::Settings {},
            revision: committed.revision.clone(),
            event: crate::contract::DomainEvent::SettingsChanged {
                settings: committed.clone(),
            },
        };
        let legacy_publications = legacy_change_publications(before_value, &actual)?;
        let ticket = self
            .coordinator
            .register_publication(SettingsPublicationBatch {
                publication,
                publications: legacy_publications,
            })?;
        Ok(DeferredSettingsPublication {
            coordinator: self.coordinator.clone(),
            settings: committed,
            settled: false,
            ticket,
        })
    }

    pub fn resume_portable_publication(
        &self,
        expected_revision: &Revision,
        publications: Vec<SettingsPublicationEvent>,
    ) -> Result<DeferredSettingsPublication, SettingsServiceError> {
        let _transaction = self
            .coordinator
            .transaction_gate
            .lock()
            .map_err(|_| SettingsServiceError::unavailable())?;
        self.coordinator.ensure_available()?;
        let portable = self.portable_snapshot_unlocked()?;
        if portable.revision() != expected_revision {
            return Err(SettingsServiceError::reconcile_failed());
        }
        let settings = self.read_exposed_unlocked()?;
        let publication = crate::events::EventPublication {
            resource: crate::contract::ResourceRefDto::Settings {},
            revision: settings.revision.clone(),
            event: crate::contract::DomainEvent::SettingsChanged {
                settings: settings.clone(),
            },
        };
        let ticket = self
            .coordinator
            .register_publication(SettingsPublicationBatch {
                publication,
                publications,
            })?;
        Ok(DeferredSettingsPublication {
            coordinator: self.coordinator.clone(),
            settings,
            settled: false,
            ticket,
        })
    }

    pub fn publish_if_portable_revision(
        &self,
        mut publication: DeferredSettingsPublication,
        expected_revision: &Revision,
    ) -> Result<bool, SettingsServiceError> {
        let evaluation = (|| {
            let _transaction = self
                .coordinator
                .transaction_gate
                .lock()
                .map_err(|_| SettingsServiceError::unavailable())?;
            self.coordinator.ensure_available()?;
            let current = self.portable_snapshot_unlocked()?;
            let matches = current.revision() == expected_revision;
            let disposition = if matches {
                PublicationDisposition::Ready
            } else {
                PublicationDisposition::Cancelled
            };
            let should_drain = publication
                .coordinator
                .prepare_publication(publication.ticket, disposition)
                .map_err(|_| SettingsServiceError::unavailable())?;
            publication.settled = true;
            Ok::<_, SettingsServiceError>((matches, should_drain))
        })();
        let (matches, should_drain) = match evaluation {
            Ok(result) => result,
            Err(error) => {
                let _supersede_result = publication.supersede();
                return Err(error);
            }
        };
        if should_drain {
            publication
                .coordinator
                .drain_publications()
                .map_err(|_| SettingsServiceError::unavailable())?;
        }
        Ok(matches)
    }

    pub fn preview_merge(
        &self,
        bytes: Option<&[u8]>,
        expected_revision: &Revision,
    ) -> Result<PortableMergePreview, SettingsServiceError> {
        let desired = portable_settings_from_bytes(bytes)
            .map_err(|_| SettingsServiceError::reconcile_failed())?;
        let _transaction = self
            .coordinator
            .transaction_gate
            .lock()
            .map_err(|_| SettingsServiceError::unavailable())?;
        self.coordinator.ensure_available()?;
        let before = self.portable_snapshot_unlocked()?;
        if before.revision() != expected_revision {
            return Err(SettingsServiceError::reconcile_failed());
        }
        let before_value = portable_settings_from_bytes(before.bytes())
            .map_err(|_| SettingsServiceError::reconcile_failed())?;
        let desired_bytes = if desired.as_object().is_some_and(Map::is_empty) {
            None
        } else {
            Some(
                serde_json::to_vec(&desired)
                    .map_err(|_| SettingsServiceError::reconcile_failed())?,
            )
        };
        let applied_revision = portable_revision(desired_bytes.as_deref())
            .map_err(|_| SettingsServiceError::reconcile_failed())?;
        let publications = legacy_change_publications(&before_value, &desired)?;
        Ok(PortableMergePreview {
            applied_revision,
            publications,
        })
    }

    fn restore(
        &self,
        previous: &BTreeMap<String, Option<Value>>,
    ) -> Result<(), SettingsServiceError> {
        let mut failed = false;
        for (key, value) in previous {
            if match value {
                Some(value) => self.store.set(key, value.clone()),
                None => self.store.delete(key),
            }
            .is_err()
            {
                failed = true;
            }
        }
        if failed {
            Err(SettingsServiceError::recovery_required())
        } else {
            Ok(())
        }
    }

    fn restore_portable_storage(&self, previous: &Value) -> Result<(), SettingsServiceError> {
        let previous_object = previous
            .as_object()
            .ok_or_else(SettingsServiceError::recovery_required)?;
        self.store
            .replace_portable_atomically(previous_object)
            .map_err(|_| SettingsServiceError::recovery_required())?;
        let restored = self.portable_storage_value_unlocked()?;
        if restored != *previous {
            return Err(SettingsServiceError::recovery_required());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PortableMergePreview {
    applied_revision: Revision,
    publications: Vec<SettingsPublicationEvent>,
}

impl PortableMergePreview {
    pub const fn applied_revision(&self) -> &Revision {
        &self.applied_revision
    }

    pub fn publications(&self) -> &[SettingsPublicationEvent] {
        &self.publications
    }
}

fn apply_settings_group(
    desired: &mut Map<String, Value>,
    group: SettingsGroup,
    value: Value,
) -> Result<(), SettingsServiceError> {
    match group {
        SettingsGroup::Appearance => {
            let object =
                exact_group_object(&value, &["appearanceMode", "lightTheme", "darkTheme"])?;
            desired.insert(
                "appearanceMode".to_string(),
                object["appearanceMode"].clone(),
            );
            desired.insert("lightThemeId".to_string(), object["lightTheme"].clone());
            desired.insert("darkThemeId".to_string(), object["darkTheme"].clone());
        }
        SettingsGroup::CustomThemeCss => {
            let object = exact_group_object(&value, &["light", "dark"])?;
            desired.insert("lightCustomThemeCss".to_string(), object["light"].clone());
            desired.insert("darkCustomThemeCss".to_string(), object["dark"].clone());
        }
        SettingsGroup::Language => {
            desired.insert("language".to_string(), value);
        }
        SettingsGroup::EditorPreferences => {
            desired.insert("editorPreferences".to_string(), value);
        }
        SettingsGroup::FileIgnoreSettings => {
            desired.insert("fileIgnoreSettings".to_string(), value);
        }
        SettingsGroup::ExportSettings => {
            desired.insert("exportSettings".to_string(), value);
        }
    }
    Ok(())
}

fn exact_group_object<'a>(
    value: &'a Value,
    expected: &[&str],
) -> Result<&'a Map<String, Value>, SettingsServiceError> {
    let object = value
        .as_object()
        .ok_or_else(SettingsServiceError::invalid)?;
    let actual = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let expected = expected.iter().copied().collect::<BTreeSet<_>>();
    (actual == expected)
        .then_some(object)
        .ok_or_else(SettingsServiceError::invalid)
}

fn valid_single_portable_value(key: &str, value: &Value) -> bool {
    serde_json::to_vec(&Value::Object(Map::from_iter([(
        key.to_string(),
        value.clone(),
    )])))
    .ok()
    .is_some_and(|bytes| validate_portable_settings_bytes(&bytes).is_ok())
}

const EDITOR_STORAGE_FIELDS: &[&str] = &[
    "autoRevealActiveFile",
    "autoSaveEnabled",
    "autoSaveIntervalMinutes",
    "autoUpdateEnabled",
    "bodyFontSize",
    "clipboardImageFolder",
    "contentWidth",
    "contentWidthPx",
    "documentLinksOpen",
    "documentLinksVisible",
    "editorFontFamily",
    "extendedSyntax",
    "imageUpload",
    "lineHeight",
    "markdownShortcuts",
    "markdownTemplates",
    "openDroppedFilesInTabs",
    "paragraphSpacingPx",
    "restoreWorkspaceOnStartup",
    "sidebarLayoutMode",
    "showDocumentTabs",
    "splitVisualPanePercent",
    "tableColumnWidthMode",
    "titlebarActions",
    "typewriterModeEnabled",
    "viewMode",
    "viewModeCustomizations",
    "showLineNumbers",
    "showWordCount",
    "hideHeadingMarkersOnFocus",
    "vimModeEnabled",
    "wrapCodeBlocks",
];

const EDITOR_PUBLICATION_FIELDS: &[&str] = EDITOR_STORAGE_FIELDS;

const EDITOR_VIEW_MODE_STORAGE_FIELDS: &[&str] = &[
    "documentLinks",
    "documentTabs",
    "fileList",
    "fileTree",
    "fileTreeButton",
    "openButton",
    "outline",
    "quickCreateButton",
    "recentFolders",
    "sidebarLayout",
    "statusBar",
    "titlebarActions",
    "viewModeToggle",
    "wordCount",
];

const EDITOR_SHORTCUT_STORAGE_FIELDS: &[&str] = &[
    "bold",
    "bulletList",
    "codeBlock",
    "heading1",
    "heading2",
    "heading3",
    "image",
    "inlineCode",
    "italic",
    "link",
    "openQuickOpen",
    "orderedList",
    "paragraph",
    "quote",
    "strikethrough",
    "syncNow",
    "table",
    "toggleAllFolds",
    "toggleDocumentHistory",
    "toggleMarkdownFiles",
    "toggleReadOnlyMode",
    "toggleSourceMode",
    "toggleTypewriterMode",
    "toggleViewMode",
    "toggleVimMode",
];

const EXPORT_STORAGE_FIELDS: &[&str] = &[
    "fontFamily",
    "pandocArgs",
    "pandocPath",
    "pdfAuthor",
    "pdfFooter",
    "pdfHeader",
    "pdfHeightMm",
    "pdfMarginMm",
    "pdfMarginPreset",
    "pdfPageBreakOnH1",
    "pdfPageSize",
    "pdfWidthMm",
];

const EXPORT_PUBLICATION_FIELDS: &[&str] = &[
    "fontFamily",
    "pandocArgs",
    "pdfAuthor",
    "pdfFooter",
    "pdfHeader",
    "pdfHeightMm",
    "pdfMarginMm",
    "pdfMarginPreset",
    "pdfPageBreakOnH1",
    "pdfPageSize",
    "pdfWidthMm",
];

fn editor_storage_values(stored: Option<Value>) -> Map<String, Value> {
    let mut values = allowlisted_group(default_editor(), stored, EDITOR_STORAGE_FIELDS);
    retain_nested_fields(&mut values, "editorFontFamily", &["family", "source"]);
    retain_nested_fields(
        &mut values,
        "extendedSyntax",
        &["githubAlerts", "highlight"],
    );
    retain_nested_fields(&mut values, "imageUpload", &["fileNamePattern"]);
    retain_nested_fields(
        &mut values,
        "markdownShortcuts",
        EDITOR_SHORTCUT_STORAGE_FIELDS,
    );
    if let Some(templates) = values
        .get_mut("markdownTemplates")
        .and_then(Value::as_array_mut)
    {
        for template in templates {
            retain_value_fields(template, &["fileName", "id", "name"]);
        }
    }
    if let Some(actions) = values
        .get_mut("titlebarActions")
        .and_then(Value::as_array_mut)
    {
        for action in actions {
            retain_value_fields(action, &["id", "visible"]);
        }
    }
    retain_nested_fields(
        &mut values,
        "viewModeCustomizations",
        EDITOR_VIEW_MODE_STORAGE_FIELDS,
    );
    if !values
        .get("viewModeCustomizations")
        .and_then(Value::as_object)
        .and_then(|customizations| customizations.get("recentFolders"))
        .and_then(Value::as_str)
        .is_some_and(|visibility| matches!(visibility, "visible" | "hidden"))
    {
        if let Some(customizations) = values
            .get_mut("viewModeCustomizations")
            .and_then(Value::as_object_mut)
        {
            customizations.remove("recentFolders");
        }
    }
    let recent_folders = values
        .get("viewModeCustomizations")
        .and_then(Value::as_object)
        .and_then(|customizations| customizations.get("recentFolders"))
        .cloned();
    let portable = normalize_portable_value("editorPreferences", Value::Object(values.clone()));
    if !portable_group_is_valid("editorPreferences", portable) {
        values = default_editor();
        if let Some(recent_folders) = recent_folders {
            values
                .get_mut("viewModeCustomizations")
                .and_then(Value::as_object_mut)
                .expect("default editor customizations are an object")
                .insert("recentFolders".to_string(), recent_folders);
        }
    }
    values
}

fn export_storage_values(stored: Option<Value>) -> Map<String, Value> {
    let mut values = allowlisted_group(default_export(), stored, EXPORT_STORAGE_FIELDS);
    let pandoc_path = values
        .get("pandocPath")
        .and_then(Value::as_str)
        .map(str::to_string);
    values.remove("pandocPath");
    if !portable_group_is_valid("exportSettings", Value::Object(values.clone())) {
        values = default_export();
    }
    if let Some(pandoc_path) = pandoc_path {
        values.insert("pandocPath".to_string(), Value::String(pandoc_path));
    }
    values
}

fn preserve_local_only_settings(
    desired: &mut Map<String, Value>,
    current_editor: Option<Value>,
    current_export: Option<Value>,
) {
    let recent_folders = editor_storage_values(current_editor)
        .get("viewModeCustomizations")
        .and_then(Value::as_object)
        .and_then(|customizations| customizations.get("recentFolders"))
        .cloned();
    if let Some(recent_folders) = recent_folders {
        let editor = desired
            .entry("editorPreferences".to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if let Some(editor) = editor.as_object_mut() {
            let customizations = editor
                .entry("viewModeCustomizations".to_string())
                .or_insert_with(|| Value::Object(Map::new()));
            if let Some(customizations) = customizations.as_object_mut() {
                customizations.insert("recentFolders".to_string(), recent_folders);
            }
        }
    }

    let pandoc_path = export_storage_values(current_export)
        .get("pandocPath")
        .cloned();
    if let Some(pandoc_path) = pandoc_path {
        let export = desired
            .entry("exportSettings".to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if let Some(export) = export.as_object_mut() {
            export.insert("pandocPath".to_string(), pandoc_path);
        }
    }
}

fn is_local_only_storage_group(key: &str, value: &Value) -> bool {
    let Some(values) = value.as_object() else {
        return false;
    };
    match key {
        "editorPreferences" if values.len() == 1 => values
            .get("viewModeCustomizations")
            .and_then(Value::as_object)
            .is_some_and(|customizations| {
                customizations.len() == 1
                    && customizations
                        .get("recentFolders")
                        .and_then(Value::as_str)
                        .is_some_and(|visibility| matches!(visibility, "visible" | "hidden"))
            }),
        "exportSettings" if values.len() == 1 => {
            values.get("pandocPath").and_then(Value::as_str).is_some()
        }
        _ => false,
    }
}

fn allowlisted_group(
    mut defaults: Map<String, Value>,
    stored: Option<Value>,
    allowed: &[&str],
) -> Map<String, Value> {
    if let Some(stored) = stored.and_then(|value| value.as_object().cloned()) {
        for key in allowed {
            if let Some(value) = stored.get(*key) {
                defaults.insert((*key).to_string(), value.clone());
            }
        }
    }
    defaults
}

fn retain_nested_fields(values: &mut Map<String, Value>, key: &str, allowed: &[&str]) {
    if let Some(value) = values.get_mut(key) {
        retain_value_fields(value, allowed);
    }
}

fn retain_value_fields(value: &mut Value, allowed: &[&str]) {
    if let Some(object) = value.as_object_mut() {
        object.retain(|key, _| allowed.contains(&key.as_str()));
    }
}

/// Projects a present storage group onto the portable publication schema.
///
/// A missing top-level group stays missing because callers only invoke this
/// function for stored values. Present known object fields overlay canonical
/// defaults recursively, so newly introduced fields receive safe defaults.
/// Unknown fields are local-only and omitted without mutating storage. Present
/// known values are retained verbatim and the snapshot validator remains the
/// fail-closed authority for invalid values.
fn portable_publication_value(key: &str, stored: Value) -> Value {
    let projected = match key {
        "editorPreferences" => project_editor_publication(stored),
        "fileIgnoreSettings" => project_known_fields(json!({ "rules": "" }), stored),
        "exportSettings" => project_known_fields(Value::Object(default_export()), stored),
        _ => stored,
    };
    normalize_portable_value(key, projected)
}

fn project_editor_publication(stored: Value) -> Value {
    let stored_titlebar_actions = stored.get("titlebarActions").cloned();
    let mut projected = project_known_fields(Value::Object(default_editor()), stored);
    let Some(projected_editor) = projected.as_object_mut() else {
        return projected;
    };

    if let Some(stored_titlebar_actions) = stored_titlebar_actions {
        let defaults = default_editor()
            .remove("titlebarActions")
            .expect("default editor titlebar actions exist");
        projected_editor.insert(
            "titlebarActions".to_string(),
            project_titlebar_actions(defaults, stored_titlebar_actions),
        );
    }
    if let Some(templates) = projected_editor
        .get_mut("markdownTemplates")
        .and_then(Value::as_array_mut)
    {
        project_markdown_templates(templates);
    }
    projected
}

fn project_markdown_templates(templates: &mut [Value]) {
    let mut used_file_names = BTreeSet::new();
    for template in templates {
        retain_value_fields(template, &["fileName", "id", "name"]);
        let Some(template) = template.as_object_mut() else {
            continue;
        };
        if !template.contains_key("fileName") {
            if let Some(id) = template.get("id").and_then(Value::as_str) {
                let base_name = markdown_template_file_stem(id);
                let file_name = (0..=20)
                    .map(|index| {
                        if index == 0 {
                            format!("{base_name}.md")
                        } else {
                            format!("{base_name}-{}.md", index + 1)
                        }
                    })
                    .find(|candidate| !used_file_names.contains(candidate));
                if let Some(file_name) = file_name {
                    template.insert("fileName".to_string(), Value::String(file_name));
                }
            }
        }
        if let Some(file_name) = template
            .get("fileName")
            .and_then(Value::as_str)
            .filter(|file_name| valid_markdown_template_file_name(file_name))
        {
            used_file_names.insert(file_name.to_ascii_lowercase());
        }
    }
}

fn markdown_template_file_stem(id: &str) -> String {
    let normalized_id = id.trim().to_lowercase();
    let id = normalized_id
        .strip_suffix(".markdown")
        .or_else(|| normalized_id.strip_suffix(".md"))
        .unwrap_or(&normalized_id);
    let mut stem = String::new();
    let mut pending_separator = false;
    for character in id.chars() {
        if character.is_ascii_alphanumeric() {
            if pending_separator && !stem.is_empty() {
                stem.push('-');
            }
            stem.push(character.to_ascii_lowercase());
            pending_separator = false;
        } else {
            pending_separator = true;
        }
    }
    if stem.is_empty() {
        "template".to_string()
    } else {
        stem
    }
}

fn valid_markdown_template_file_name(file_name: &str) -> bool {
    !file_name.is_empty()
        && file_name.trim() == file_name
        && file_name != "."
        && file_name != ".."
        && file_name.to_ascii_lowercase().ends_with(".md")
        && !file_name.contains('/')
        && !file_name.contains('\\')
}

fn project_known_fields(mut defaults: Value, stored: Value) -> Value {
    match (&mut defaults, stored) {
        (Value::Object(defaults), Value::Object(stored)) => {
            for (key, default) in defaults.iter_mut() {
                if let Some(stored) = stored.get(key) {
                    *default = project_known_fields(default.clone(), stored.clone());
                }
            }
            Value::Object(defaults.clone())
        }
        (_, stored) => stored,
    }
}

fn project_titlebar_actions(defaults: Value, stored: Value) -> Value {
    let Some(defaults) = defaults.as_array() else {
        return stored;
    };
    let Some(stored_actions) = stored.as_array() else {
        return stored;
    };
    let defaults_by_id = defaults
        .iter()
        .filter_map(|action| {
            action
                .get("id")
                .and_then(Value::as_str)
                .map(|id| (id, action))
        })
        .collect::<BTreeMap<_, _>>();
    let mut seen = BTreeSet::new();
    let mut projected = Vec::new();
    for action in stored_actions {
        let Some(id) = action.get("id").and_then(Value::as_str) else {
            return stored;
        };
        let Some(default) = defaults_by_id.get(id) else {
            continue;
        };
        if !seen.insert(id) {
            return stored;
        }
        let mut action_projection = (*default).clone();
        if let Some(visible) = action.get("visible") {
            action_projection["visible"] = visible.clone();
        }
        projected.push(action_projection);
    }
    projected.extend(
        defaults
            .iter()
            .filter(|default| {
                default
                    .get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| !seen.contains(id))
            })
            .cloned(),
    );
    Value::Array(projected)
}

fn portable_group_is_valid(key: &str, value: Value) -> bool {
    serde_json::to_vec(&Value::Object(Map::from_iter([(key.to_string(), value)])))
        .ok()
        .is_some_and(|bytes| validate_portable_settings_bytes(&bytes).is_ok())
}

fn editor_publication_value(value: Option<Value>) -> Value {
    let storage = Value::Object(editor_storage_values(value));
    let publication = allowlisted_group(default_editor(), Some(storage), EDITOR_PUBLICATION_FIELDS);
    normalize_portable_value("editorPreferences", Value::Object(publication))
}

fn export_publication_value(value: Option<Value>) -> Value {
    let storage = Value::Object(export_storage_values(value));
    Value::Object(allowlisted_group(
        default_export(),
        Some(storage),
        EXPORT_PUBLICATION_FIELDS,
    ))
}

fn legacy_change_publications(
    before: &Value,
    after: &Value,
) -> Result<Vec<SettingsPublicationEvent>, SettingsServiceError> {
    let before = legacy_event_groups(before)?;
    let after = legacy_event_groups(after)?;
    let mut publications = Vec::new();
    for (index, (event, payload_key)) in [
        ("markra://theme-changed", "preferences"),
        ("markra://custom-theme-css-changed", "customThemeCss"),
        ("markra://language-changed", "language"),
        ("markra://editor-preferences-changed", "preferences"),
        ("markra://file-ignore-settings-changed", "settings"),
        ("markra://export-settings-changed", "settings"),
    ]
    .into_iter()
    .enumerate()
    {
        if before[index] != after[index] {
            publications.push(SettingsPublicationEvent::new(
                event,
                json!({ payload_key: after[index].clone() }),
            ));
        }
    }
    Ok(publications)
}

fn legacy_event_groups(value: &Value) -> Result<[Value; 6], SettingsServiceError> {
    let object = value
        .as_object()
        .ok_or_else(SettingsServiceError::reconcile_failed)?;
    Ok([
        json!({
            "appearanceMode": object.get("appearanceMode").cloned().unwrap_or_else(|| json!("system")),
            "lightTheme": object.get("lightThemeId").cloned().unwrap_or_else(|| json!(super::model::DEFAULT_LIGHT_THEME_ID)),
            "darkTheme": object.get("darkThemeId").cloned().unwrap_or_else(|| json!(super::model::DEFAULT_DARK_THEME_ID)),
        }),
        json!({
            "light": object.get("lightCustomThemeCss").cloned().unwrap_or(Value::Null),
            "dark": object.get("darkCustomThemeCss").cloned().unwrap_or(Value::Null),
        }),
        object
            .get("language")
            .cloned()
            .unwrap_or_else(|| json!("en")),
        editor_publication_value(object.get("editorPreferences").cloned()),
        object
            .get("fileIgnoreSettings")
            .cloned()
            .unwrap_or_else(|| json!({ "rules": "" })),
        export_publication_value(object.get("exportSettings").cloned()),
    ])
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SettingsPublicationEvent {
    event: String,
    payload: Value,
}

impl SettingsPublicationEvent {
    pub fn new(event: impl Into<String>, payload: Value) -> Self {
        Self {
            event: event.into(),
            payload,
        }
    }

    pub fn event_name(&self) -> &str {
        &self.event
    }

    pub const fn payload(&self) -> &Value {
        &self.payload
    }
}

pub struct DeferredSettingsPublication {
    coordinator: Arc<SettingsRuntimeCoordinator>,
    settings: SettingsSnapshotDto,
    settled: bool,
    ticket: u64,
}

impl DeferredSettingsPublication {
    /// Marks the durable batch ready and consumes the ticket so it cannot be
    /// published twice. The shared coordinator emits typed and compatibility
    /// events together in commit order.
    pub fn publish(mut self) -> Result<(), crate::events::EventSinkError> {
        let result = self
            .coordinator
            .mark_publication(self.ticket, PublicationDisposition::Ready);
        self.settled = true;
        result
    }

    pub fn cancel(mut self) -> Result<(), crate::events::EventSinkError> {
        let result = self
            .coordinator
            .mark_publication(self.ticket, PublicationDisposition::Cancelled);
        self.settled = true;
        result
    }

    pub fn supersede(self) -> Result<(), crate::events::EventSinkError> {
        self.cancel()
    }

    pub const fn settings(&self) -> &SettingsSnapshotDto {
        &self.settings
    }
}

impl Drop for DeferredSettingsPublication {
    fn drop(&mut self) {
        if !self.settled {
            self.coordinator.require_recovery();
        }
    }
}

impl fmt::Debug for DeferredSettingsPublication {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DeferredSettingsPublication")
            .field("ticket", &self.ticket)
            .field("revision", &self.settings.revision)
            .field("settled", &self.settled)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsServiceErrorKind {
    InvalidField,
    RevisionConflict,
    Unavailable,
    ReconcileFailed,
    RecoveryRequired,
}

#[derive(Clone, Eq, PartialEq)]
pub struct SettingsServiceError {
    kind: SettingsServiceErrorKind,
    current_revision: Option<Revision>,
}

impl SettingsServiceError {
    const fn invalid() -> Self {
        Self {
            kind: SettingsServiceErrorKind::InvalidField,
            current_revision: None,
        }
    }

    const fn unavailable() -> Self {
        Self {
            kind: SettingsServiceErrorKind::Unavailable,
            current_revision: None,
        }
    }

    fn revision_conflict(current_revision: Revision) -> Self {
        Self {
            kind: SettingsServiceErrorKind::RevisionConflict,
            current_revision: Some(current_revision),
        }
    }

    const fn reconcile_failed() -> Self {
        Self {
            kind: SettingsServiceErrorKind::ReconcileFailed,
            current_revision: None,
        }
    }

    const fn recovery_required() -> Self {
        Self {
            kind: SettingsServiceErrorKind::RecoveryRequired,
            current_revision: None,
        }
    }

    pub const fn kind(&self) -> SettingsServiceErrorKind {
        self.kind
    }

    pub const fn current_revision(&self) -> Option<&Revision> {
        self.current_revision.as_ref()
    }
}

impl From<SettingsStoreError> for SettingsServiceError {
    fn from(_error: SettingsStoreError) -> Self {
        Self {
            kind: SettingsServiceErrorKind::Unavailable,
            current_revision: None,
        }
    }
}

impl fmt::Debug for SettingsServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SettingsServiceError")
            .field("kind", &self.kind)
            .field("current_revision", &self.current_revision)
            .finish()
    }
}

impl fmt::Display for SettingsServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("settings operation failed")
    }
}

impl std::error::Error for SettingsServiceError {}

impl fmt::Debug for SettingsService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SettingsService(..)")
    }
}
