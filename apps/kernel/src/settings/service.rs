//! Settings service implementation boundary.

use std::{
    collections::BTreeMap,
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
        let should_drain = {
            let mut state = self
                .publications
                .lock()
                .map_err(|_| crate::events::EventSinkError)?;
            if state.recovery_required {
                return Err(crate::events::EventSinkError);
            }
            if ticket < state.next_publication {
                return Ok(());
            }
            let pending = state
                .pending
                .get_mut(&ticket)
                .ok_or(crate::events::EventSinkError)?;
            pending.disposition = disposition;
            if state.draining
                || state
                    .pending
                    .get(&state.next_publication)
                    .is_none_or(|pending| pending.disposition == PublicationDisposition::Awaiting)
            {
                false
            } else {
                state.draining = true;
                true
            }
        };
        if !should_drain {
            return Ok(());
        }

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

    fn ensure_available(&self) -> Result<(), SettingsServiceError> {
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

    fn require_recovery(&self) {
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

    fn read_exposed_unlocked(&self) -> Result<SettingsSnapshotDto, SettingsServiceError> {
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
                .or(self.store.get("lightTheme")?)
                .unwrap_or_else(|| json!(super::model::DEFAULT_LIGHT_THEME_ID)),
        );
        values.insert(
            "appearance.darkTheme".to_string(),
            self.store
                .get("darkThemeId")?
                .or(self.store.get("darkTheme")?)
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
                portable.insert(key.to_string(), normalize_portable_value(key, value));
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
        let desired = portable_settings_from_bytes(bytes)
            .map_err(|_| SettingsServiceError::reconcile_failed())?;
        let desired_object = desired
            .as_object()
            .ok_or_else(SettingsServiceError::reconcile_failed)?;
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

        match self.store.replace_portable_atomically(desired_object) {
            Ok(()) => {}
            Err(error) if error.kind() == SettingsStoreErrorKind::PublishUncertain => {
                self.coordinator.require_recovery();
                return Err(SettingsServiceError::recovery_required());
            }
            Err(error) => return Err(error.into()),
        }
        let actual = self
            .portable_snapshot_unlocked()
            .and_then(|after| {
                portable_settings_from_bytes(after.bytes())
                    .map_err(|_| SettingsServiceError::reconcile_failed())
            })
            .and_then(|actual| {
                (actual == desired)
                    .then_some(actual)
                    .ok_or_else(SettingsServiceError::reconcile_failed)
            });
        let actual = match actual {
            Ok(actual) => actual,
            Err(_) => {
                if self.restore_portable(&before_value).is_err() {
                    self.coordinator.require_recovery();
                    return Err(SettingsServiceError::recovery_required());
                }
                return Err(SettingsServiceError::reconcile_failed());
            }
        };

        let committed = self.read_exposed_unlocked()?;
        let publication = crate::events::EventPublication {
            resource: crate::contract::ResourceRefDto::Settings {},
            revision: committed.revision.clone(),
            event: crate::contract::DomainEvent::SettingsChanged {
                settings: committed.clone(),
            },
        };
        let legacy_publications = legacy_change_publications(&before_value, &actual)?;
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

    fn restore_portable(&self, previous: &Value) -> Result<(), SettingsServiceError> {
        let previous_object = previous
            .as_object()
            .ok_or_else(SettingsServiceError::recovery_required)?;
        self.store
            .replace_portable_atomically(previous_object)
            .map_err(|_| SettingsServiceError::recovery_required())?;
        let restored = self.portable_snapshot_unlocked().and_then(|snapshot| {
            portable_settings_from_bytes(snapshot.bytes())
                .map_err(|_| SettingsServiceError::recovery_required())
        })?;
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
            retain_value_fields(template, &["fileName", "id", "name", "suggestedName"]);
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
