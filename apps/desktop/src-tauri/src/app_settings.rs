use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    },
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use tauri::{Emitter, Manager, Runtime};
use tauri_plugin_store::StoreExt;

use crate::mcp::config::{McpConfig, McpConfigDocument};
use crate::mcp::local_settings::McpLocalSettingsService;
use crate::storage_capability::{
    create_private_file_options, open_canonical_directory_nofollow, rename_in_directory,
    sync_directory, unique_regular_file_identity,
};

const SETTINGS_STORE_PATH: &str = "settings.json";
const APPEARANCE_MODE_KEY: &str = "appearanceMode";
const LIGHT_THEME_KEY: &str = "lightThemeId";
const DARK_THEME_KEY: &str = "darkThemeId";
const LEGACY_LIGHT_THEME_KEY: &str = "lightTheme";
const LEGACY_DARK_THEME_KEY: &str = "darkTheme";
const LANGUAGE_KEY: &str = "language";
const EDITOR_PREFERENCES_KEY: &str = "editorPreferences";
const FILE_IGNORE_SETTINGS_KEY: &str = "fileIgnoreSettings";
const EXPORT_SETTINGS_KEY: &str = "exportSettings";
const PORTABLE_SETTINGS_MAX_BYTES: usize = 16 * 1024 * 1024;
const PORTABLE_SETTINGS_KEYS: [&str; 9] = [
    APPEARANCE_MODE_KEY,
    LIGHT_THEME_KEY,
    DARK_THEME_KEY,
    "lightCustomThemeCss",
    "darkCustomThemeCss",
    LANGUAGE_KEY,
    EDITOR_PREFERENCES_KEY,
    FILE_IGNORE_SETTINGS_KEY,
    EXPORT_SETTINGS_KEY,
];

const EXPOSED_FIELDS: [&str; 23] = [
    "appearance.mode",
    "appearance.lightTheme",
    "appearance.darkTheme",
    "language",
    "editor.bodyFontSize",
    "editor.contentWidth",
    "editor.contentWidthPx",
    "editor.fontFamily",
    "editor.lineHeight",
    "editor.paragraphSpacingPx",
    "editor.showWordCount",
    "editor.wrapCodeBlocks",
    "editor.viewMode",
    "files.ignoreRules",
    "export.pdfAuthor",
    "export.pdfFooter",
    "export.pdfHeader",
    "export.pdfHeightMm",
    "export.pdfMarginMm",
    "export.pdfMarginPreset",
    "export.pdfPageBreakOnH1",
    "export.pdfPageSize",
    "export.pdfWidthMm",
];

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum AppSettingsGroup {
    Appearance,
    CustomThemeCss,
    Language,
    EditorPreferences,
    FileIgnoreSettings,
    ExportSettings,
}

impl AppSettingsGroup {
    fn event(self) -> Option<(&'static str, &'static str)> {
        match self {
            Self::Appearance => Some(("markra://theme-changed", "preferences")),
            Self::CustomThemeCss => Some(("markra://custom-theme-css-changed", "customThemeCss")),
            Self::Language => Some(("markra://language-changed", "language")),
            Self::EditorPreferences => Some(("markra://editor-preferences-changed", "preferences")),
            Self::FileIgnoreSettings => Some(("markra://file-ignore-settings-changed", "settings")),
            Self::ExportSettings => Some(("markra://export-settings-changed", "settings")),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct ExposedSettingsPatch {
    pub(crate) expected_revision: String,
    pub(crate) values: BTreeMap<String, Value>,
}

#[cfg_attr(not(mobile), allow(dead_code))]
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct UpdateMcpPolicyInput {
    pub(crate) expected_revision: String,
    pub(crate) config: McpConfig,
}

#[cfg_attr(not(mobile), allow(dead_code))]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct McpPolicySnapshot {
    pub(crate) revision: String,
    pub(crate) config: McpConfig,
}

impl From<McpConfigDocument> for McpPolicySnapshot {
    fn from(document: McpConfigDocument) -> Self {
        Self {
            revision: document.revision,
            config: document.config,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExposedAppSettings {
    pub(crate) revision: String,
    pub(crate) values: BTreeMap<String, Value>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) credentials_present: BTreeMap<String, bool>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AppSettingsError {
    pub(crate) code: &'static str,
    message: &'static str,
}

impl AppSettingsError {
    fn unavailable() -> Self {
        Self {
            code: "settings_unavailable",
            message: "The QingYu settings store is unavailable.",
        }
    }

    fn invalid_group() -> Self {
        Self {
            code: "invalid_settings_group",
            message: "The settings group is invalid.",
        }
    }

    fn invalid_field() -> Self {
        Self {
            code: "invalid_settings_field",
            message: "The settings patch contains an unknown or invalid field.",
        }
    }

    fn stale() -> Self {
        Self {
            code: "settings_revision_conflict",
            message: "The settings changed after the supplied revision was read.",
        }
    }

    fn remote_invalid() -> Self {
        Self {
            code: "remote-settings-invalid",
            message: "The remote portable settings are invalid.",
        }
    }

    pub(crate) fn reconcile_failed() -> Self {
        Self {
            code: "settings-reconcile-failed",
            message: "The synchronized settings could not be reconciled safely.",
        }
    }
}

impl fmt::Display for AppSettingsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for AppSettingsError {}

pub(crate) trait SettingsBackend: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<Value>, AppSettingsError>;
    fn set(&self, key: &str, value: Value) -> Result<(), AppSettingsError>;
    fn delete(&self, key: &str) -> Result<(), AppSettingsError>;
    fn save(&self) -> Result<(), AppSettingsError>;
    fn replace_portable_atomically(
        &self,
        desired: &Map<String, Value>,
    ) -> Result<(), AppSettingsError>;
}

pub(crate) trait SettingsEventSink: Send + Sync {
    fn emit(&self, event: &str, payload: Value) -> Result<(), AppSettingsError>;
}

struct StoreSettingsBackend<R: Runtime> {
    app_data_root: PathBuf,
    store: Arc<tauri_plugin_store::Store<R>>,
}

impl<R: Runtime> SettingsBackend for StoreSettingsBackend<R> {
    fn get(&self, key: &str) -> Result<Option<Value>, AppSettingsError> {
        Ok(self.store.get(key))
    }

    fn set(&self, key: &str, value: Value) -> Result<(), AppSettingsError> {
        self.store.set(key, value);
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), AppSettingsError> {
        self.store.delete(key);
        Ok(())
    }

    fn save(&self) -> Result<(), AppSettingsError> {
        self.store
            .save()
            .map_err(|_| AppSettingsError::unavailable())
    }

    fn replace_portable_atomically(
        &self,
        desired: &Map<String, Value>,
    ) -> Result<(), AppSettingsError> {
        let mut values = self.store.entries().into_iter().collect::<BTreeMap<_, _>>();
        for key in PORTABLE_SETTINGS_KEYS {
            values.remove(key);
        }
        values.extend(
            desired
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
        let bytes = serde_json::to_vec(&values).map_err(|_| AppSettingsError::unavailable())?;
        let publication = replace_settings_file_atomically(&self.app_data_root, &bytes)?;
        complete_settings_file_replacement(publication, || {
            for key in PORTABLE_SETTINGS_KEYS {
                match desired.get(key) {
                    Some(value) => self.store.set(key, value.clone()),
                    None => {
                        self.store.delete(key);
                    }
                }
            }
        })
    }
}

struct TauriSettingsEventSink<R: Runtime> {
    app: tauri::AppHandle<R>,
}

impl<R: Runtime> SettingsEventSink for TauriSettingsEventSink<R> {
    fn emit(&self, event: &str, payload: Value) -> Result<(), AppSettingsError> {
        self.app
            .emit(event, payload)
            .map_err(|_| AppSettingsError::unavailable())
    }
}

#[derive(Clone)]
pub(crate) struct AppSettingsService {
    backend: Arc<dyn SettingsBackend>,
    events: Option<Arc<dyn SettingsEventSink>>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct SettingsPublicationEvent {
    event: String,
    payload: Value,
}

impl SettingsPublicationEvent {
    pub(crate) fn new(event: &str, payload: Value) -> Self {
        Self {
            event: event.to_string(),
            payload,
        }
    }

    pub(crate) fn event_name(&self) -> &str {
        &self.event
    }
}

#[derive(Clone)]
pub(crate) struct DeferredSettingsPublication {
    events: Option<Arc<dyn SettingsEventSink>>,
    publications: Vec<SettingsPublicationEvent>,
}

pub(crate) struct PortableSettingsSnapshot {
    bytes: Option<Vec<u8>>,
    revision: String,
}

impl PortableSettingsSnapshot {
    pub(crate) fn bytes(&self) -> Option<&[u8]> {
        self.bytes.as_deref()
    }

    pub(crate) fn revision(&self) -> &str {
        &self.revision
    }
}

impl DeferredSettingsPublication {
    pub(crate) fn publish(&self) -> Result<(), AppSettingsError> {
        let Some(events) = &self.events else {
            return Ok(());
        };
        for publication in &self.publications {
            events.emit(&publication.event, publication.payload.clone())?;
        }
        Ok(())
    }

    pub(crate) fn publications(&self) -> &[SettingsPublicationEvent] {
        &self.publications
    }
}

impl AppSettingsService {
    pub(crate) fn from_app<R: Runtime>(
        app: &tauri::AppHandle<R>,
    ) -> Result<Self, AppSettingsError> {
        Self::from_app_with_events(app, true)
    }

    fn from_app_with_events<R: Runtime>(
        app: &tauri::AppHandle<R>,
        emit_events: bool,
    ) -> Result<Self, AppSettingsError> {
        let store = app
            .store_builder(SETTINGS_STORE_PATH)
            .disable_auto_save()
            .build()
            .map_err(|_| AppSettingsError::unavailable())?;
        let app_data_root = app
            .path()
            .app_data_dir()
            .map_err(|_| AppSettingsError::unavailable())?;
        Ok(Self {
            backend: Arc::new(StoreSettingsBackend {
                app_data_root,
                store,
            }),
            events: emit_events
                .then(|| Arc::new(TauriSettingsEventSink { app: app.clone() }) as Arc<_>),
        })
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(
        backend: Arc<dyn SettingsBackend>,
        events: Option<Arc<dyn SettingsEventSink>>,
    ) -> Self {
        Self { backend, events }
    }

    #[cfg(test)]
    pub(crate) fn memory_for_test() -> Self {
        Self {
            backend: Arc::new(EmptySettingsBackend::default()),
            events: None,
        }
    }

    pub(crate) fn deferred_settings_publication(
        &self,
        publications: Vec<SettingsPublicationEvent>,
    ) -> DeferredSettingsPublication {
        DeferredSettingsPublication {
            events: self.events.clone(),
            publications,
        }
    }

    pub(crate) fn publish_deferred_if_portable_revision(
        &self,
        publication: &DeferredSettingsPublication,
        expected_portable_revision: &str,
    ) -> Result<bool, AppSettingsError> {
        let _settings_guard = app_settings_transaction_lock()
            .lock()
            .map_err(|_| AppSettingsError::unavailable())?;
        let snapshot = self.portable_store_snapshot()?;
        let bytes = portable_settings_snapshot_bytes(&snapshot)?;
        if portable_settings_revision(bytes.as_deref()) != expected_portable_revision {
            return Ok(false);
        }
        publication.publish()?;
        Ok(true)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn exposed_field_names() -> &'static [&'static str] {
        &EXPOSED_FIELDS
    }

    pub(crate) fn read_group(
        &self,
        group: AppSettingsGroup,
    ) -> Result<Option<Value>, AppSettingsError> {
        match group {
            AppSettingsGroup::Appearance => {
                let mode = self.backend.get(APPEARANCE_MODE_KEY)?;
                let light = self
                    .backend
                    .get(LIGHT_THEME_KEY)?
                    .or(self.backend.get(LEGACY_LIGHT_THEME_KEY)?);
                let dark = self
                    .backend
                    .get(DARK_THEME_KEY)?
                    .or(self.backend.get(LEGACY_DARK_THEME_KEY)?);
                if mode.is_none() && light.is_none() && dark.is_none() {
                    return Ok(None);
                }
                Ok(Some(json!({
                    "appearanceMode": mode.unwrap_or_else(|| json!("system")),
                    "lightTheme": light.unwrap_or_else(|| json!("light")),
                    "darkTheme": dark.unwrap_or_else(|| json!("dark")),
                })))
            }
            AppSettingsGroup::CustomThemeCss => {
                let light = self.backend.get("lightCustomThemeCss")?;
                let dark = self.backend.get("darkCustomThemeCss")?;
                if light.is_none() && dark.is_none() {
                    return Ok(None);
                }
                Ok(Some(json!({
                    "light": light.unwrap_or_else(|| json!("")),
                    "dark": dark.unwrap_or_else(|| json!("")),
                })))
            }
            AppSettingsGroup::Language => self.backend.get(LANGUAGE_KEY),
            AppSettingsGroup::EditorPreferences => self.backend.get(EDITOR_PREFERENCES_KEY),
            AppSettingsGroup::FileIgnoreSettings => self.backend.get(FILE_IGNORE_SETTINGS_KEY),
            AppSettingsGroup::ExportSettings => self
                .backend
                .get(EXPORT_SETTINGS_KEY)
                .map(|value| value.map(portable_export_settings)),
        }
    }

    pub(crate) fn file_ignore_rules(&self) -> Result<Option<String>, AppSettingsError> {
        let rules = self
            .read_group(AppSettingsGroup::FileIgnoreSettings)?
            .and_then(|settings| {
                settings
                    .get("rules")
                    .and_then(Value::as_str)
                    .map(normalize_file_ignore_rules)
            })
            .filter(|rules| !rules.is_empty());
        Ok(rules)
    }

    pub(crate) fn write_group(
        &self,
        group: AppSettingsGroup,
        value: Value,
    ) -> Result<Value, AppSettingsError> {
        let _guard = app_settings_transaction_lock()
            .lock()
            .map_err(|_| AppSettingsError::unavailable())?;
        let value = match group {
            AppSettingsGroup::ExportSettings => portable_export_settings(value),
            _ => value,
        };
        validate_group(group, &value)?;
        self.write_groups_atomically(&BTreeMap::from([(group, value.clone())]))?;
        self.emit_group(group, &value);
        Ok(value)
    }

    pub(crate) fn read_exposed(&self) -> Result<ExposedAppSettings, AppSettingsError> {
        let appearance = self
            .read_group(AppSettingsGroup::Appearance)?
            .unwrap_or_else(default_appearance);
        let language = self
            .read_group(AppSettingsGroup::Language)?
            .unwrap_or_else(|| json!("en"));
        let editor = merge_defaults(
            default_editor(),
            self.read_group(AppSettingsGroup::EditorPreferences)?,
        );
        let files = merge_defaults(
            json!({ "rules": "" }),
            self.read_group(AppSettingsGroup::FileIgnoreSettings)?,
        );
        let export = merge_defaults(
            default_export(),
            self.read_group(AppSettingsGroup::ExportSettings)?,
        );

        let mut values = BTreeMap::new();
        insert(
            &mut values,
            "appearance.mode",
            &appearance,
            "appearanceMode",
        );
        insert(
            &mut values,
            "appearance.lightTheme",
            &appearance,
            "lightTheme",
        );
        insert(
            &mut values,
            "appearance.darkTheme",
            &appearance,
            "darkTheme",
        );
        values.insert("language".to_string(), language);
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
            insert(&mut values, field, &editor, key);
        }
        insert(&mut values, "files.ignoreRules", &files, "rules");
        for key in [
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
            insert(&mut values, &format!("export.{key}"), &export, key);
        }
        let revision = settings_revision(&values)?;
        Ok(ExposedAppSettings {
            revision,
            values,
            credentials_present: BTreeMap::new(),
        })
    }

    pub(crate) fn patch_exposed(
        &self,
        patch: ExposedSettingsPatch,
    ) -> Result<ExposedAppSettings, AppSettingsError> {
        let _guard = app_settings_transaction_lock()
            .lock()
            .map_err(|_| AppSettingsError::unavailable())?;
        if patch.values.is_empty()
            || patch
                .values
                .keys()
                .any(|field| !EXPOSED_FIELDS.contains(&field.as_str()))
        {
            return Err(AppSettingsError::invalid_field());
        }
        for (field, value) in &patch.values {
            validate_field(field, value)?;
        }
        let current = self.read_exposed()?;
        if current.revision != patch.expected_revision {
            return Err(AppSettingsError::stale());
        }

        let mut groups = BTreeMap::new();
        for (field, value) in patch.values {
            let group = field_group(&field).ok_or_else(AppSettingsError::invalid_field)?;
            let entry = groups.entry(group).or_insert(
                self.read_group(group)?
                    .unwrap_or_else(|| default_group(group)),
            );
            apply_field(entry, &field, value)?;
        }
        for (group, value) in &groups {
            validate_group(*group, value)?;
        }
        self.write_groups_atomically(&groups)?;
        for (group, value) in &groups {
            self.emit_group(*group, value);
        }
        self.read_exposed()
    }

    fn write_groups_atomically(
        &self,
        groups: &BTreeMap<AppSettingsGroup, Value>,
    ) -> Result<(), AppSettingsError> {
        let changes = storage_changes(groups)?;
        let mut previous = BTreeMap::new();
        for key in changes.keys() {
            previous.insert(key.clone(), self.backend.get(key)?);
        }
        for (key, value) in &changes {
            if let Err(error) = self.backend.set(key, value.clone()) {
                restore_settings(self.backend.as_ref(), &previous);
                return Err(error);
            }
        }
        if let Err(error) = self.backend.save() {
            restore_settings(self.backend.as_ref(), &previous);
            return Err(error);
        }
        Ok(())
    }

    fn emit_group(&self, group: AppSettingsGroup, value: &Value) {
        let (Some(events), Some((event, payload_key))) = (&self.events, group.event()) else {
            return;
        };
        let payload = Value::Object(Map::from_iter([(payload_key.to_string(), value.clone())]));
        let _emit_result = events.emit(event, payload);
    }

    pub(crate) fn replace_portable_settings_defer_publication(
        &self,
        settings: Value,
    ) -> Result<(Value, DeferredSettingsPublication), AppSettingsError> {
        let _guard = app_settings_transaction_lock()
            .lock()
            .map_err(|_| AppSettingsError::unavailable())?;
        let object = settings
            .as_object()
            .ok_or_else(AppSettingsError::invalid_group)?;
        let expected = BTreeSet::from([
            "appearanceMode",
            "customThemeCss",
            "darkTheme",
            "editorPreferences",
            "exportSettings",
            "fileIgnoreSettings",
            "language",
            "lightTheme",
        ]);
        if object.keys().map(String::as_str).collect::<BTreeSet<_>>() != expected {
            return Err(AppSettingsError::invalid_group());
        }
        let groups = BTreeMap::from([
            (
                AppSettingsGroup::Appearance,
                json!({
                    "appearanceMode": object["appearanceMode"],
                    "lightTheme": object["lightTheme"],
                    "darkTheme": object["darkTheme"],
                }),
            ),
            (
                AppSettingsGroup::CustomThemeCss,
                object["customThemeCss"].clone(),
            ),
            (AppSettingsGroup::Language, object["language"].clone()),
            (
                AppSettingsGroup::EditorPreferences,
                object["editorPreferences"].clone(),
            ),
            (
                AppSettingsGroup::FileIgnoreSettings,
                object["fileIgnoreSettings"].clone(),
            ),
            (
                AppSettingsGroup::ExportSettings,
                portable_export_settings(object["exportSettings"].clone()),
            ),
        ]);
        for (group, value) in &groups {
            validate_group(*group, value)?;
        }
        let before = self.portable_store_snapshot()?;
        self.write_groups_atomically(&groups)?;
        let after = self.portable_store_snapshot()?;
        let publications = portable_settings_change_events_from_values(&before, &after)?
            .into_iter()
            .map(|(event, payload)| SettingsPublicationEvent::new(event, payload))
            .collect();
        Ok((
            after,
            DeferredSettingsPublication {
                events: self.events.clone(),
                publications,
            },
        ))
    }

    fn portable_store_snapshot(&self) -> Result<Value, AppSettingsError> {
        let mut object = Map::new();
        for key in PORTABLE_SETTINGS_KEYS {
            if let Some(mut value) = self.backend.get(key)? {
                if key == EXPORT_SETTINGS_KEY {
                    value = portable_export_settings(value);
                }
                object.insert(key.to_string(), value);
            }
        }
        Ok(Value::Object(object))
    }

    pub(crate) fn portable_settings_snapshot(
        &self,
    ) -> Result<PortableSettingsSnapshot, AppSettingsError> {
        let _settings_guard = app_settings_transaction_lock()
            .lock()
            .map_err(|_| AppSettingsError::unavailable())?;
        let snapshot = self.portable_store_snapshot()?;
        let bytes = if snapshot.as_object().is_some_and(Map::is_empty) {
            None
        } else {
            let bytes =
                serde_json::to_vec(&snapshot).map_err(|_| AppSettingsError::unavailable())?;
            validate_portable_settings_bytes(&bytes)?;
            Some(bytes)
        };
        let revision = portable_settings_revision(bytes.as_deref());
        Ok(PortableSettingsSnapshot { bytes, revision })
    }

    pub(crate) fn preview_portable_settings_merge(
        &self,
        bytes: Option<&[u8]>,
        expected_portable_revision: &str,
    ) -> Result<(String, Vec<SettingsPublicationEvent>), AppSettingsError> {
        if let Some(bytes) = bytes {
            validate_portable_settings_bytes(bytes)?;
        }
        let desired = portable_settings_from_bytes(bytes)?;
        let _settings_guard = app_settings_transaction_lock()
            .lock()
            .map_err(|_| AppSettingsError::unavailable())?;
        let before = self.portable_store_snapshot()?;
        let before_bytes = portable_settings_snapshot_bytes(&before)?;
        if portable_settings_revision(before_bytes.as_deref()) != expected_portable_revision {
            return Err(AppSettingsError::reconcile_failed());
        }
        let desired_bytes = portable_settings_snapshot_bytes(&desired)?;
        let applied_revision = portable_settings_revision(desired_bytes.as_deref());
        let publications = portable_settings_change_events_from_values(&before, &desired)?
            .into_iter()
            .map(|(event, payload)| SettingsPublicationEvent::new(event, payload))
            .collect();
        Ok((applied_revision, publications))
    }

    pub(crate) fn merge_portable_settings_bytes_defer_publication_with_preflight<
        Preflight,
        Verify,
    >(
        &self,
        bytes: Option<&[u8]>,
        expected_portable_revision: &str,
        preflight: Preflight,
        verify: Verify,
    ) -> Result<DeferredSettingsPublication, AppSettingsError>
    where
        Preflight: FnOnce() -> Result<(), AppSettingsError>,
        Verify: FnOnce(&Value) -> Result<(), AppSettingsError>,
    {
        if let Some(bytes) = bytes {
            validate_portable_settings_bytes(bytes)?;
        }
        let desired = portable_settings_from_bytes(bytes)?;
        let desired_object = desired
            .as_object()
            .ok_or_else(AppSettingsError::reconcile_failed)?;
        let _settings_guard = app_settings_transaction_lock()
            .lock()
            .map_err(|_| AppSettingsError::unavailable())?;
        preflight()?;
        let before = self.portable_store_snapshot()?;
        let before_bytes = portable_settings_snapshot_bytes(&before)?;
        if portable_settings_revision(before_bytes.as_deref()) != expected_portable_revision {
            return Err(AppSettingsError::reconcile_failed());
        }
        self.backend.replace_portable_atomically(desired_object)?;
        let after = self.portable_store_snapshot()?;
        if let Err(error) = verify(&after) {
            let previous = before
                .as_object()
                .ok_or_else(AppSettingsError::reconcile_failed)?;
            self.backend.replace_portable_atomically(previous)?;
            return Err(error);
        }
        let publications = portable_settings_change_events_from_values(&before, &after)?
            .into_iter()
            .map(|(event, payload)| SettingsPublicationEvent::new(event, payload))
            .collect();
        Ok(DeferredSettingsPublication {
            events: self.events.clone(),
            publications,
        })
    }
}

fn portable_settings_revision(bytes: Option<&[u8]>) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes.unwrap_or_default()))
}

fn portable_settings_snapshot_bytes(snapshot: &Value) -> Result<Option<Vec<u8>>, AppSettingsError> {
    if snapshot.as_object().is_some_and(Map::is_empty) {
        Ok(None)
    } else {
        serde_json::to_vec(snapshot)
            .map(Some)
            .map_err(|_| AppSettingsError::unavailable())
    }
}

#[cfg(test)]
#[derive(Default)]
struct EmptySettingsBackend {
    values: std::sync::Mutex<BTreeMap<String, Value>>,
}

#[cfg(test)]
impl SettingsBackend for EmptySettingsBackend {
    fn get(&self, key: &str) -> Result<Option<Value>, AppSettingsError> {
        self.values
            .lock()
            .map(|values| values.get(key).cloned())
            .map_err(|_| AppSettingsError::unavailable())
    }

    fn set(&self, key: &str, value: Value) -> Result<(), AppSettingsError> {
        self.values
            .lock()
            .map(|mut values| {
                values.insert(key.to_string(), value);
            })
            .map_err(|_| AppSettingsError::unavailable())
    }

    fn delete(&self, key: &str) -> Result<(), AppSettingsError> {
        self.values
            .lock()
            .map(|mut values| {
                values.remove(key);
            })
            .map_err(|_| AppSettingsError::unavailable())
    }

    fn save(&self) -> Result<(), AppSettingsError> {
        Ok(())
    }

    fn replace_portable_atomically(
        &self,
        desired: &Map<String, Value>,
    ) -> Result<(), AppSettingsError> {
        let mut values = self
            .values
            .lock()
            .map_err(|_| AppSettingsError::unavailable())?;
        for key in PORTABLE_SETTINGS_KEYS {
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

fn restore_settings(backend: &dyn SettingsBackend, previous: &BTreeMap<String, Option<Value>>) {
    for (key, value) in previous {
        match value {
            Some(value) => {
                let _restore_result = backend.set(key, value.clone());
            }
            None => {
                let _restore_result = backend.delete(key);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SettingsFileReplacement {
    Durable,
    PublishedWithoutDirectoryDurability,
}

fn complete_settings_file_replacement<Update>(
    publication: SettingsFileReplacement,
    update_cache: Update,
) -> Result<(), AppSettingsError>
where
    Update: FnOnce(),
{
    update_cache();
    match publication {
        SettingsFileReplacement::Durable => Ok(()),
        SettingsFileReplacement::PublishedWithoutDirectoryDurability => {
            Err(AppSettingsError::unavailable())
        }
    }
}

fn replace_settings_file_atomically(
    app_data_root: &Path,
    bytes: &[u8],
) -> Result<SettingsFileReplacement, AppSettingsError> {
    replace_settings_file_atomically_with_directory_sync(app_data_root, bytes, sync_directory)
}

fn replace_settings_file_atomically_with_directory_sync<SyncDirectory>(
    app_data_root: &Path,
    bytes: &[u8],
    sync_after_rename: SyncDirectory,
) -> Result<SettingsFileReplacement, AppSettingsError>
where
    SyncDirectory: FnOnce(&cap_std::fs::Dir) -> io::Result<()>,
{
    static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
    let directory = open_canonical_directory_nofollow(app_data_root)
        .map_err(|_| AppSettingsError::unavailable())?;
    let existing = match directory.symlink_metadata(SETTINGS_STORE_PATH) {
        Ok(metadata) => Some(
            unique_regular_file_identity(&metadata).ok_or_else(AppSettingsError::unavailable)?,
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err(AppSettingsError::unavailable()),
    };
    let (staged_name, mut staged) = (0..1000)
        .find_map(|_| {
            let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let name = format!(".settings-{}-{sequence}.tmp", std::process::id());
            match directory.open_with(&name, &create_private_file_options()) {
                Ok(file) => Some(Ok((name, file))),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => None,
                Err(_) => Some(Err(AppSettingsError::unavailable())),
            }
        })
        .unwrap_or_else(|| Err(AppSettingsError::unavailable()))?;
    if staged
        .write_all(bytes)
        .and_then(|()| staged.sync_all())
        .is_err()
    {
        drop(staged);
        let _cleanup = directory.remove_file(&staged_name);
        return Err(AppSettingsError::unavailable());
    }
    drop(staged);
    let retained = match directory.symlink_metadata(SETTINGS_STORE_PATH) {
        Ok(metadata) => match unique_regular_file_identity(&metadata) {
            Some(identity) => Some(identity),
            None => {
                let _cleanup = directory.remove_file(&staged_name);
                return Err(AppSettingsError::unavailable());
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => {
            let _cleanup = directory.remove_file(&staged_name);
            return Err(AppSettingsError::unavailable());
        }
    };
    if retained != existing {
        let _cleanup = directory.remove_file(&staged_name);
        return Err(AppSettingsError::reconcile_failed());
    }
    if rename_in_directory(
        &directory,
        &staged_name,
        SETTINGS_STORE_PATH,
        existing.is_some(),
    )
    .is_err()
    {
        let _cleanup = directory.remove_file(&staged_name);
        return Err(AppSettingsError::unavailable());
    }
    Ok(match sync_after_rename(&directory) {
        Ok(()) => SettingsFileReplacement::Durable,
        Err(_) => SettingsFileReplacement::PublishedWithoutDirectoryDurability,
    })
}

fn app_settings_transaction_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn storage_changes(
    groups: &BTreeMap<AppSettingsGroup, Value>,
) -> Result<BTreeMap<String, Value>, AppSettingsError> {
    let mut changes = BTreeMap::new();
    for (group, value) in groups {
        match group {
            AppSettingsGroup::Appearance => {
                let object = value
                    .as_object()
                    .ok_or_else(AppSettingsError::invalid_group)?;
                for (key, store_key) in [
                    ("appearanceMode", APPEARANCE_MODE_KEY),
                    ("lightTheme", LIGHT_THEME_KEY),
                    ("darkTheme", DARK_THEME_KEY),
                ] {
                    changes.insert(
                        store_key.to_string(),
                        object
                            .get(key)
                            .cloned()
                            .ok_or_else(AppSettingsError::invalid_group)?,
                    );
                }
            }
            AppSettingsGroup::CustomThemeCss => {
                let object = value
                    .as_object()
                    .ok_or_else(AppSettingsError::invalid_group)?;
                changes.insert(
                    "lightCustomThemeCss".to_string(),
                    object
                        .get("light")
                        .cloned()
                        .ok_or_else(AppSettingsError::invalid_group)?,
                );
                changes.insert(
                    "darkCustomThemeCss".to_string(),
                    object
                        .get("dark")
                        .cloned()
                        .ok_or_else(AppSettingsError::invalid_group)?,
                );
            }
            AppSettingsGroup::Language => {
                changes.insert(LANGUAGE_KEY.to_string(), value.clone());
            }
            AppSettingsGroup::EditorPreferences => {
                changes.insert(EDITOR_PREFERENCES_KEY.to_string(), value.clone());
            }
            AppSettingsGroup::FileIgnoreSettings => {
                changes.insert(FILE_IGNORE_SETTINGS_KEY.to_string(), value.clone());
            }
            AppSettingsGroup::ExportSettings => {
                changes.insert(EXPORT_SETTINGS_KEY.to_string(), value.clone());
            }
        }
    }
    Ok(changes)
}

fn validate_group(group: AppSettingsGroup, value: &Value) -> Result<(), AppSettingsError> {
    match group {
        AppSettingsGroup::Appearance => {
            let object = value
                .as_object()
                .ok_or_else(AppSettingsError::invalid_group)?;
            let keys = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
            if keys != BTreeSet::from(["appearanceMode", "lightTheme", "darkTheme"]) {
                return Err(AppSettingsError::invalid_group());
            }
            validate_field("appearance.mode", &object["appearanceMode"])?;
            validate_field("appearance.lightTheme", &object["lightTheme"])?;
            validate_field("appearance.darkTheme", &object["darkTheme"])
        }
        AppSettingsGroup::CustomThemeCss => {
            let object = value
                .as_object()
                .ok_or_else(AppSettingsError::invalid_group)?;
            let keys = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
            if keys != BTreeSet::from(["light", "dark"]) {
                return Err(AppSettingsError::invalid_group());
            }
            if ["light", "dark"].into_iter().all(|key| {
                object[key]
                    .as_str()
                    .is_some_and(|css| utf16_len(css) <= 50_000)
            }) {
                Ok(())
            } else {
                Err(AppSettingsError::invalid_group())
            }
        }
        AppSettingsGroup::Language => validate_field("language", value),
        AppSettingsGroup::EditorPreferences => {
            validate_object_exposed(value, "editor.", &[("editorFontFamily", "fontFamily")])
        }
        AppSettingsGroup::FileIgnoreSettings => {
            validate_object_exposed(value, "files.", &[("rules", "ignoreRules")])
        }
        AppSettingsGroup::ExportSettings => validate_object_exposed(value, "export.", &[]),
    }
}

fn validate_object_exposed(
    value: &Value,
    prefix: &str,
    aliases: &[(&str, &str)],
) -> Result<(), AppSettingsError> {
    let object = value
        .as_object()
        .ok_or_else(AppSettingsError::invalid_group)?;
    for (key, value) in object {
        let exposed_key = aliases
            .iter()
            .find_map(|(stored, exposed)| (*stored == key).then_some(*exposed))
            .unwrap_or(key);
        let field = format!("{prefix}{exposed_key}");
        if EXPOSED_FIELDS.contains(&field.as_str()) {
            validate_field(&field, value)?;
        }
    }
    Ok(())
}

fn validate_field(field: &str, value: &Value) -> Result<(), AppSettingsError> {
    let valid = match field {
        "appearance.mode" => string_in(value, &["system", "light", "dark"]),
        "appearance.lightTheme" | "appearance.darkTheme" => valid_theme_id(value),
        "language" => string_in(value, LANGUAGES),
        "editor.bodyFontSize" => integer_in(value, &[14, 15, 16, 17, 18, 20]),
        "editor.contentWidth" => string_in(value, &["narrow", "default", "wide"]),
        "editor.contentWidthPx" => value.is_null() || integer_between(value, 640, 1280),
        "editor.fontFamily" => valid_font_family(value),
        "editor.lineHeight" => value
            .as_f64()
            .is_some_and(|number| [1.5, 1.65, 1.8].contains(&number)),
        "editor.paragraphSpacingPx" => integer_between(value, 0, 32),
        "editor.showWordCount" | "editor.wrapCodeBlocks" => value.is_boolean(),
        "editor.viewMode" => string_in(value, &["full", "daily", "focus", "immersive", "custom"]),
        "files.ignoreRules" => value.as_str().is_some_and(|text| text.len() <= 50_000),
        "export.pdfAuthor" | "export.pdfFooter" | "export.pdfHeader" => {
            value.as_str().is_some_and(|text| text.len() <= 200)
        }
        "export.pdfHeightMm" | "export.pdfWidthMm" => integer_between(value, 50, 2000),
        "export.pdfMarginMm" => integer_between(value, 0, 60),
        "export.pdfMarginPreset" => string_in(
            value,
            &["custom", "default", "narrow", "none", "normal", "wide"],
        ),
        "export.pdfPageBreakOnH1" => value.is_boolean(),
        "export.pdfPageSize" => string_in(value, &["a4", "custom", "default", "letter"]),
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(AppSettingsError::invalid_field())
    }
}

fn field_group(field: &str) -> Option<AppSettingsGroup> {
    if field.starts_with("appearance.") {
        Some(AppSettingsGroup::Appearance)
    } else if field == "language" {
        Some(AppSettingsGroup::Language)
    } else if field.starts_with("editor.") {
        Some(AppSettingsGroup::EditorPreferences)
    } else if field.starts_with("files.") {
        Some(AppSettingsGroup::FileIgnoreSettings)
    } else if field.starts_with("export.") {
        Some(AppSettingsGroup::ExportSettings)
    } else {
        None
    }
}

fn apply_field(target: &mut Value, field: &str, value: Value) -> Result<(), AppSettingsError> {
    if field == "language" {
        *target = value;
        return Ok(());
    }
    let object = target
        .as_object_mut()
        .ok_or_else(AppSettingsError::invalid_group)?;
    let key = match field {
        "appearance.mode" => "appearanceMode",
        "appearance.lightTheme" => "lightTheme",
        "appearance.darkTheme" => "darkTheme",
        "editor.fontFamily" => "editorFontFamily",
        "files.ignoreRules" => "rules",
        _ => field
            .split_once('.')
            .map(|(_, key)| key)
            .ok_or_else(AppSettingsError::invalid_field)?,
    };
    object.insert(key.to_string(), value);
    Ok(())
}

fn default_group(group: AppSettingsGroup) -> Value {
    match group {
        AppSettingsGroup::Appearance => default_appearance(),
        AppSettingsGroup::CustomThemeCss => json!({ "light": "", "dark": "" }),
        AppSettingsGroup::Language => json!("en"),
        AppSettingsGroup::EditorPreferences => default_editor(),
        AppSettingsGroup::FileIgnoreSettings => json!({ "rules": "" }),
        AppSettingsGroup::ExportSettings => default_export(),
    }
}

fn normalize_file_ignore_rules(rules: &str) -> String {
    rules.replace("\r\n", "\n").replace('\r', "\n")
}

fn default_appearance() -> Value {
    json!({ "appearanceMode": "system", "lightTheme": "light", "darkTheme": "dark" })
}

fn default_editor() -> Value {
    json!({
        "bodyFontSize": 16,
        "contentWidth": "default",
        "contentWidthPx": null,
        "editorFontFamily": { "family": null, "source": "theme" },
        "lineHeight": 1.65,
        "paragraphSpacingPx": 8,
        "showWordCount": true,
        "wrapCodeBlocks": true,
        "viewMode": "daily"
    })
}

fn default_export() -> Value {
    json!({
        "pdfAuthor": "",
        "pdfFooter": "",
        "pdfHeader": "",
        "pdfHeightMm": 297,
        "pdfMarginMm": 18,
        "pdfMarginPreset": "default",
        "pdfPageBreakOnH1": false,
        "pdfPageSize": "default",
        "pdfWidthMm": 210
    })
}

fn portable_export_settings(mut value: Value) -> Value {
    if let Some(settings) = value.as_object_mut() {
        settings.remove("pandocPath");
    }
    value
}

fn merge_defaults(defaults: Value, stored: Option<Value>) -> Value {
    let mut defaults = defaults.as_object().cloned().unwrap_or_default();
    if let Some(stored) = stored.and_then(|value| value.as_object().cloned()) {
        defaults.extend(stored);
    }
    Value::Object(defaults)
}

fn insert(values: &mut BTreeMap<String, Value>, field: &str, object: &Value, key: &str) {
    if let Some(value) = object.get(key) {
        values.insert(field.to_string(), value.clone());
    }
}

fn settings_revision(values: &BTreeMap<String, Value>) -> Result<String, AppSettingsError> {
    let bytes = serde_json::to_vec(values).map_err(|_| AppSettingsError::unavailable())?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn string_in(value: &Value, allowed: &[&str]) -> bool {
    value
        .as_str()
        .is_some_and(|candidate| allowed.contains(&candidate))
}

fn integer_in(value: &Value, allowed: &[i64]) -> bool {
    value
        .as_i64()
        .is_some_and(|number| allowed.contains(&number))
}

fn integer_between(value: &Value, min: i64, max: i64) -> bool {
    value
        .as_i64()
        .is_some_and(|number| (min..=max).contains(&number))
}

fn valid_font_family(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    match object.get("source").and_then(Value::as_str) {
        Some("theme") => object.get("family").is_none_or(Value::is_null),
        Some("system") => object
            .get("family")
            .and_then(Value::as_str)
            .is_some_and(|family| !family.trim().is_empty() && family.len() <= 160),
        _ => false,
    }
}

fn valid_theme_id(value: &Value) -> bool {
    value.as_str().is_some_and(|theme_id| {
        let mut bytes = theme_id.bytes();
        !theme_id.starts_with("qingyu-")
            && theme_id.len() <= 64
            && bytes
                .next()
                .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
            && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    })
}

pub(crate) fn validate_portable_settings_bytes(bytes: &[u8]) -> Result<(), AppSettingsError> {
    if bytes.len() > PORTABLE_SETTINGS_MAX_BYTES {
        return Err(AppSettingsError::remote_invalid());
    }
    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| AppSettingsError::remote_invalid())?;
    let object = value
        .as_object()
        .ok_or_else(AppSettingsError::remote_invalid)?;
    let allowed = BTreeSet::from(PORTABLE_SETTINGS_KEYS);
    if object.keys().any(|key| !allowed.contains(key.as_str())) {
        return Err(AppSettingsError::remote_invalid());
    }
    for (key, value) in object {
        let valid = match key.as_str() {
            APPEARANCE_MODE_KEY => string_in(value, &["system", "light", "dark"]),
            LIGHT_THEME_KEY | DARK_THEME_KEY => valid_theme_id(value),
            "lightCustomThemeCss" | "darkCustomThemeCss" => {
                value.as_str().is_some_and(|css| utf16_len(css) <= 50_000)
            }
            LANGUAGE_KEY => string_in(value, LANGUAGES),
            EDITOR_PREFERENCES_KEY => valid_portable_editor_preferences(value),
            FILE_IGNORE_SETTINGS_KEY => valid_portable_file_ignore_settings(value),
            EXPORT_SETTINGS_KEY => valid_portable_export_settings(value),
            _ => false,
        };
        if !valid {
            return Err(AppSettingsError::remote_invalid());
        }
    }
    Ok(())
}

pub(crate) fn sanitize_legacy_remote_portable_settings(
    bytes: &[u8],
) -> Result<Option<Vec<u8>>, AppSettingsError> {
    if bytes.len() > PORTABLE_SETTINGS_MAX_BYTES {
        return Err(AppSettingsError::remote_invalid());
    }
    let mut value: Value =
        serde_json::from_slice(bytes).map_err(|_| AppSettingsError::remote_invalid())?;
    let object = value
        .as_object_mut()
        .ok_or_else(AppSettingsError::remote_invalid)?;
    if object.remove("mcp").is_none() {
        return Ok(None);
    }
    let sanitized = serde_json::to_vec(&value).map_err(|_| AppSettingsError::remote_invalid())?;
    validate_portable_settings_bytes(&sanitized)?;
    Ok(Some(sanitized))
}

fn object_has_only<'a>(value: &'a Value, allowed: &[&str]) -> Option<&'a Map<String, Value>> {
    let object = value.as_object()?;
    object
        .keys()
        .all(|key| allowed.contains(&key.as_str()))
        .then_some(object)
}

fn object_has_exact<'a>(value: &'a Value, expected: &[&str]) -> Option<&'a Map<String, Value>> {
    object_has_only(value, expected).filter(|object| {
        object.len() == expected.len() && expected.iter().all(|key| object.contains_key(*key))
    })
}

fn utf16_len(value: &str) -> usize {
    value.encode_utf16().count()
}

fn canonical_trimmed_text(value: &Value, max_utf16_len: usize, allow_empty: bool) -> bool {
    value.as_str().is_some_and(|text| {
        text.trim() == text && (allow_empty || !text.is_empty()) && utf16_len(text) <= max_utf16_len
    })
}

fn valid_portable_editor_preferences(value: &Value) -> bool {
    const KEYS: &[&str] = &[
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
        "paragraphSpacingPx",
        "restoreWorkspaceOnStartup",
        "sidebarLayoutMode",
        "showDocumentTabs",
        "splitVisualPanePercent",
        "tableColumnWidthMode",
        "titlebarActions",
        "viewMode",
        "viewModeCustomizations",
        "showLineNumbers",
        "showWordCount",
        "wrapCodeBlocks",
    ];
    let Some(object) = object_has_exact(value, KEYS) else {
        return false;
    };
    object.iter().all(|(key, value)| match key.as_str() {
        "autoRevealActiveFile"
        | "autoSaveEnabled"
        | "autoUpdateEnabled"
        | "documentLinksOpen"
        | "documentLinksVisible"
        | "restoreWorkspaceOnStartup"
        | "showDocumentTabs"
        | "showLineNumbers"
        | "showWordCount"
        | "wrapCodeBlocks" => value.is_boolean(),
        "autoSaveIntervalMinutes" => integer_between(value, 1, 120),
        "bodyFontSize" => integer_in(value, &[14, 15, 16, 17, 18, 20]),
        "clipboardImageFolder" => valid_portable_relative_folder(value),
        "contentWidth" => string_in(value, &["narrow", "default", "wide"]),
        "contentWidthPx" => value.is_null() || integer_between(value, 640, 1280),
        "editorFontFamily" => valid_strict_font_family(value),
        "extendedSyntax" => valid_boolean_object(value, &["githubAlerts", "highlight"]),
        "imageUpload" => valid_image_upload_settings(value),
        "lineHeight" => value
            .as_f64()
            .is_some_and(|number| [1.5, 1.65, 1.8].contains(&number)),
        "markdownShortcuts" => valid_markdown_shortcuts(value),
        "markdownTemplates" => valid_markdown_templates(value),
        "paragraphSpacingPx" => integer_between(value, 0, 32),
        "sidebarLayoutMode" => string_in(value, &["stacked", "tabs"]),
        "splitVisualPanePercent" => integer_between(value, 25, 75),
        "tableColumnWidthMode" => string_in(value, &["auto", "even"]),
        "titlebarActions" => valid_titlebar_actions(value),
        "viewMode" => string_in(value, &["full", "daily", "focus", "immersive", "custom"]),
        "viewModeCustomizations" => valid_view_mode_customizations(value),
        _ => false,
    })
}

fn valid_boolean_object(value: &Value, allowed: &[&str]) -> bool {
    object_has_exact(value, allowed).is_some_and(|object| object.values().all(Value::is_boolean))
}

fn valid_portable_relative_folder(value: &Value) -> bool {
    value.as_str().is_some_and(|folder| {
        if folder == "." {
            return true;
        }
        let bytes = folder.as_bytes();
        if folder.is_empty()
            || folder.trim() != folder
            || folder.starts_with('/')
            || folder.starts_with('\\')
            || folder.contains('\\')
            || folder.chars().any(char::is_control)
            || (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
        {
            return false;
        }
        folder
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != ".." && part.trim() == part)
    })
}

fn valid_strict_font_family(value: &Value) -> bool {
    object_has_exact(value, &["family", "source"]).is_some_and(|object| {
        match object.get("source").and_then(Value::as_str) {
            Some("theme") => object.get("family").is_some_and(Value::is_null),
            Some("system") => object
                .get("family")
                .and_then(Value::as_str)
                .is_some_and(|family| {
                    !family.is_empty()
                        && family.trim() == family
                        && utf16_len(family) <= 160
                        && !family
                            .chars()
                            .any(|character| character <= '\u{001f}' || character == '\u{007f}')
                }),
            _ => false,
        }
    })
}

fn valid_image_upload_settings(value: &Value) -> bool {
    object_has_exact(value, &["fileNamePattern"]).is_some_and(|object| {
        object.get("fileNamePattern").is_some_and(|pattern| {
            pattern.as_str().is_some_and(|pattern| {
                !pattern.is_empty()
                    && pattern.trim() == pattern
                    && utf16_len(pattern) <= 120
                    && !pattern.contains('/')
                    && !pattern.contains('\\')
                    && pattern != "."
                    && pattern != ".."
            })
        })
    })
}

fn valid_markdown_shortcuts(value: &Value) -> bool {
    // These defaults and normalization rules mirror packages/shared/src/keyboard-shortcuts.ts.
    // The cross-language golden test makes drift fail in both implementations.
    const SHORTCUTS: &[(&str, &str, Option<&str>)] = &[
        ("openQuickOpen", "Mod+P", None),
        ("syncNow", "Mod+Alt+R", None),
        ("toggleMarkdownFiles", "Mod+Shift+M", None),
        ("toggleDocumentHistory", "Mod+Shift+H", None),
        ("toggleSourceMode", "Mod+Alt+S", Some("Mod+Alt+V")),
        ("toggleReadOnlyMode", "Mod+Alt+L", None),
        ("toggleViewMode", "F8", None),
        ("bold", "Mod+B", None),
        ("italic", "Mod+I", None),
        ("strikethrough", "Mod+Shift+X", None),
        ("inlineCode", "Mod+E", None),
        ("paragraph", "Mod+Alt+0", None),
        ("heading1", "Mod+Alt+1", None),
        ("heading2", "Mod+Alt+2", None),
        ("heading3", "Mod+Alt+3", None),
        ("bulletList", "Mod+Shift+8", None),
        ("orderedList", "Mod+Shift+7", None),
        ("quote", "Mod+Shift+B", None),
        ("codeBlock", "Mod+Alt+C", None),
        ("link", "Mod+K", None),
        ("image", "Mod+Shift+I", None),
        ("table", "Mod+Shift+Alt+T", Some("Mod+Alt+T")),
        ("toggleAllFolds", "Mod+Alt+T", None),
    ];
    let actions = SHORTCUTS
        .iter()
        .map(|(action, _, _)| *action)
        .collect::<Vec<_>>();
    let Some(object) = object_has_exact(value, &actions) else {
        return false;
    };
    let mut candidates = BTreeMap::new();
    let mut counts = BTreeMap::<String, usize>::new();
    for (action, fallback, previous) in SHORTCUTS {
        let formatted = object
            .get(*action)
            .and_then(Value::as_str)
            .and_then(canonical_keyboard_shortcut);
        let candidate = match formatted.as_deref() {
            Some(candidate) if Some(candidate) == *previous => *fallback,
            Some(candidate) if !reserved_keyboard_shortcut(candidate) => candidate,
            _ => *fallback,
        };
        candidates.insert(*action, candidate.to_string());
        *counts.entry(candidate.to_string()).or_default() += 1;
    }

    SHORTCUTS.iter().all(|(action, fallback, _)| {
        let candidate = candidates
            .get(action)
            .expect("all shortcut candidates exist");
        let normalized = if counts.get(candidate) == Some(&1) {
            candidate.as_str()
        } else {
            *fallback
        };
        object.get(*action).and_then(Value::as_str) == Some(normalized)
    })
}

fn canonical_keyboard_shortcut(shortcut: &str) -> Option<String> {
    let mut alt = false;
    let mut key = None;
    let mut mod_key = false;
    let mut shift = false;

    for part in shortcut
        .split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        if part.eq_ignore_ascii_case("mod") || part.eq_ignore_ascii_case("cmdorctrl") {
            if mod_key {
                return None;
            }
            mod_key = true;
        } else if part.eq_ignore_ascii_case("alt") || part.eq_ignore_ascii_case("option") {
            if alt {
                return None;
            }
            alt = true;
        } else if part.eq_ignore_ascii_case("shift") {
            if shift {
                return None;
            }
            shift = true;
        } else {
            if key.is_some() {
                return None;
            }
            key = normalize_shortcut_key(part);
            key.as_ref()?;
        }
    }

    let key = key?;
    let function_key = is_function_shortcut_key(&key);
    if !mod_key && (!function_key || alt || shift) {
        return None;
    }
    let mut parts = Vec::new();
    if mod_key {
        parts.push("Mod".to_string());
    }
    if shift {
        parts.push("Shift".to_string());
    }
    if alt {
        parts.push("Alt".to_string());
    }
    parts.push(key);
    Some(parts.join("+"))
}

fn normalize_shortcut_key(key: &str) -> Option<String> {
    if key.len() == 1 {
        let byte = key.as_bytes()[0];
        if byte.is_ascii_alphabetic() {
            return Some((byte as char).to_ascii_uppercase().to_string());
        }
        if byte.is_ascii_digit()
            || matches!(
                byte,
                b'`' | b'\\' | b'[' | b']' | b',' | b'=' | b'-' | b'.' | b'\'' | b';' | b'/'
            )
        {
            return Some((byte as char).to_string());
        }
    }
    let suffix = key.get(1..)?;
    if !key.as_bytes()[0].eq_ignore_ascii_case(&b'f') || suffix.is_empty() {
        return None;
    }
    let number = suffix.parse::<u8>().ok()?;
    ((1..=12).contains(&number) && suffix == number.to_string()).then(|| format!("F{number}"))
}

fn is_function_shortcut_key(key: &str) -> bool {
    key.strip_prefix('F')
        .and_then(|number| number.parse::<u8>().ok())
        .is_some_and(|number| (1..=12).contains(&number))
}

fn reserved_keyboard_shortcut(shortcut: &str) -> bool {
    matches!(
        shortcut,
        "Mod+,"
            | "Mod+A"
            | "Mod+C"
            | "Mod+F"
            | "Mod+H"
            | "Mod+N"
            | "Mod+O"
            | "Mod+P"
            | "Mod+S"
            | "Mod+V"
            | "Mod+W"
            | "Mod+X"
            | "Mod+Y"
            | "Mod+Z"
            | "Mod+Alt+F"
            | "Mod+Alt+P"
            | "Mod+Shift+E"
            | "Mod+Shift+F"
            | "Mod+Shift+O"
            | "Mod+Shift+S"
            | "Mod+Shift+V"
            | "Mod+Shift+Z"
    )
}

fn valid_markdown_templates(value: &Value) -> bool {
    let Some(templates) = value.as_array().filter(|templates| templates.len() <= 20) else {
        return false;
    };
    let mut ids = BTreeSet::new();
    let mut file_names = BTreeSet::new();
    templates.iter().all(|template| {
        object_has_exact(template, &["fileName", "id", "name", "suggestedName"]).is_some_and(
            |object| {
                let file_name = object.get("fileName").and_then(Value::as_str);
                let id = object.get("id").and_then(Value::as_str);
                let name = object.get("name").and_then(Value::as_str);
                let suggested = object.get("suggestedName").and_then(Value::as_str);
                file_name.is_some_and(|file_name| {
                    !file_name.is_empty()
                        && file_name.trim() == file_name
                        && file_name != "."
                        && file_name != ".."
                        && file_name.to_ascii_lowercase().ends_with(".md")
                        && !file_name.contains('/')
                        && !file_name.contains('\\')
                        && file_names.insert(file_name.to_lowercase())
                }) && id.is_some_and(|id| {
                    !id.is_empty() && id.trim() == id && ids.insert(id.to_string())
                }) && name.is_some_and(|name| !name.is_empty() && name.trim() == name)
                    && suggested.is_some_and(|name| name.trim() == name)
            },
        )
    })
}

fn valid_titlebar_actions(value: &Value) -> bool {
    const IDS: &[&str] = &["viewMode", "sourceMode", "history", "save", "theme"];
    let Some(actions) = value.as_array() else {
        return false;
    };
    let mut seen = BTreeSet::new();
    actions.len() == IDS.len()
        && actions.iter().all(|action| {
            object_has_exact(action, &["id", "visible"]).is_some_and(|object| {
                let id = object.get("id").and_then(Value::as_str);
                id.is_some_and(|id| IDS.contains(&id) && seen.insert(id.to_string()))
                    && object.get("visible").is_some_and(Value::is_boolean)
            })
        })
}

fn valid_view_mode_customizations(value: &Value) -> bool {
    const KEYS: &[&str] = &[
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
    object_has_exact(value, KEYS).is_some_and(|object| {
        object
            .values()
            .all(|visibility| string_in(visibility, &["visible", "hidden"]))
    })
}

fn valid_portable_file_ignore_settings(value: &Value) -> bool {
    object_has_exact(value, &["rules"]).is_some_and(|object| {
        object
            .get("rules")
            .and_then(Value::as_str)
            .is_some_and(|rules| !rules.contains('\r') && utf16_len(rules) <= 50_000)
    })
}

fn valid_portable_export_settings(value: &Value) -> bool {
    const KEYS: &[&str] = &[
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
    let Some(object) = object_has_exact(value, KEYS) else {
        return false;
    };
    let Some(page_size) = object.get("pdfPageSize").and_then(Value::as_str) else {
        return false;
    };
    let dimensions_valid = match page_size {
        "a4" | "default" => {
            object.get("pdfHeightMm").and_then(Value::as_i64) == Some(297)
                && object.get("pdfWidthMm").and_then(Value::as_i64) == Some(210)
        }
        "letter" => {
            object.get("pdfHeightMm").and_then(Value::as_i64) == Some(279)
                && object.get("pdfWidthMm").and_then(Value::as_i64) == Some(216)
        }
        "custom" => {
            integer_between(&object["pdfHeightMm"], 50, 2_000)
                && integer_between(&object["pdfWidthMm"], 50, 2_000)
        }
        _ => false,
    };
    let Some(margin_preset) = object.get("pdfMarginPreset").and_then(Value::as_str) else {
        return false;
    };
    let margin_valid = match margin_preset {
        "custom" => integer_between(&object["pdfMarginMm"], 0, 60),
        "default" | "normal" => object.get("pdfMarginMm").and_then(Value::as_i64) == Some(18),
        "narrow" => object.get("pdfMarginMm").and_then(Value::as_i64) == Some(10),
        "none" => object.get("pdfMarginMm").and_then(Value::as_i64) == Some(0),
        "wide" => object.get("pdfMarginMm").and_then(Value::as_i64) == Some(25),
        _ => false,
    };
    dimensions_valid
        && margin_valid
        && object.iter().all(|(key, value)| match key.as_str() {
            "pandocArgs" => canonical_trimmed_text(value, 1_000, true),
            "pdfAuthor" | "pdfFooter" | "pdfHeader" => canonical_trimmed_text(value, 200, true),
            "pdfHeightMm" | "pdfWidthMm" => true,
            "pdfMarginMm" => integer_between(value, 0, 60),
            "pdfMarginPreset" => true,
            "pdfPageBreakOnH1" => value.is_boolean(),
            "pdfPageSize" => true,
            _ => false,
        })
}

#[cfg(test)]
fn portable_settings_change_events(
    before: Option<&[u8]>,
    after: Option<&[u8]>,
) -> Result<Vec<(&'static str, Value)>, AppSettingsError> {
    let before = portable_settings_from_bytes(before)?;
    let after = portable_settings_from_bytes(after)?;
    portable_settings_change_events_from_values(&before, &after)
}

fn portable_settings_change_events_from_values(
    before: &Value,
    after: &Value,
) -> Result<Vec<(&'static str, Value)>, AppSettingsError> {
    let before = portable_event_groups(before)?;
    let after = portable_event_groups(after)?;
    let mut events = Vec::new();
    for group in [
        PortableEventGroup::Appearance,
        PortableEventGroup::CustomThemeCss,
        PortableEventGroup::Language,
        PortableEventGroup::EditorPreferences,
        PortableEventGroup::FileIgnoreSettings,
        PortableEventGroup::ExportSettings,
    ] {
        let before_value = before
            .get(&group)
            .expect("all event groups are materialized");
        let after_value = after
            .get(&group)
            .expect("all event groups are materialized");
        if before_value != after_value {
            events.push(group.event(after_value.clone()));
        }
    }
    Ok(events)
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
enum PortableEventGroup {
    Appearance,
    CustomThemeCss,
    Language,
    EditorPreferences,
    FileIgnoreSettings,
    ExportSettings,
}

impl PortableEventGroup {
    fn event(self, value: Value) -> (&'static str, Value) {
        match self {
            Self::Appearance => ("markra://theme-changed", json!({ "preferences": value })),
            Self::CustomThemeCss => (
                "markra://custom-theme-css-changed",
                json!({ "customThemeCss": value }),
            ),
            Self::Language => ("markra://language-changed", json!({ "language": value })),
            Self::EditorPreferences => (
                "markra://editor-preferences-changed",
                json!({ "preferences": value }),
            ),
            Self::FileIgnoreSettings => (
                "markra://file-ignore-settings-changed",
                json!({ "settings": value }),
            ),
            Self::ExportSettings => (
                "markra://export-settings-changed",
                json!({ "settings": value }),
            ),
        }
    }
}

pub(crate) fn portable_settings_from_bytes(
    bytes: Option<&[u8]>,
) -> Result<Value, AppSettingsError> {
    let raw = match bytes {
        Some(bytes) => serde_json::from_slice::<Value>(bytes)
            .map_err(|_| AppSettingsError::reconcile_failed())?,
        None => Value::Object(Map::new()),
    };
    let raw = raw
        .as_object()
        .ok_or_else(AppSettingsError::reconcile_failed)?;
    let mut portable = Map::new();
    for key in PORTABLE_SETTINGS_KEYS {
        if let Some(mut value) = raw.get(key).cloned() {
            if key == EXPORT_SETTINGS_KEY {
                value = portable_export_settings(value);
            }
            portable.insert(key.to_string(), value);
        }
    }
    Ok(Value::Object(portable))
}

fn portable_event_groups(
    value: &Value,
) -> Result<BTreeMap<PortableEventGroup, Value>, AppSettingsError> {
    let object = value
        .as_object()
        .ok_or_else(AppSettingsError::reconcile_failed)?;
    let appearance = json!({
        "appearanceMode": object.get(APPEARANCE_MODE_KEY).cloned().unwrap_or_else(|| json!("system")),
        "lightTheme": object.get(LIGHT_THEME_KEY).cloned().unwrap_or_else(|| json!("light")),
        "darkTheme": object.get(DARK_THEME_KEY).cloned().unwrap_or_else(|| json!("dark")),
    });
    let custom_css = json!({
        "light": object.get("lightCustomThemeCss").cloned().unwrap_or(Value::Null),
        "dark": object.get("darkCustomThemeCss").cloned().unwrap_or(Value::Null),
    });
    Ok(BTreeMap::from([
        (PortableEventGroup::Appearance, appearance),
        (PortableEventGroup::CustomThemeCss, custom_css),
        (
            PortableEventGroup::Language,
            object
                .get(LANGUAGE_KEY)
                .cloned()
                .unwrap_or_else(|| json!("en")),
        ),
        (
            PortableEventGroup::EditorPreferences,
            object
                .get(EDITOR_PREFERENCES_KEY)
                .cloned()
                .unwrap_or_else(default_editor),
        ),
        (
            PortableEventGroup::FileIgnoreSettings,
            object
                .get(FILE_IGNORE_SETTINGS_KEY)
                .cloned()
                .unwrap_or_else(|| json!({ "rules": "" })),
        ),
        (
            PortableEventGroup::ExportSettings,
            object
                .get(EXPORT_SETTINGS_KEY)
                .cloned()
                .map(portable_export_settings)
                .unwrap_or_else(default_export),
        ),
    ]))
}

const LANGUAGES: &[&str] = &[
    "en", "zh-CN", "zh-TW", "ja", "ko", "fr", "de", "es", "pt-BR", "it", "ru",
];
#[tauri::command]
pub(crate) fn read_app_settings_group(
    app: tauri::AppHandle,
    group: AppSettingsGroup,
) -> Result<Option<Value>, String> {
    AppSettingsService::from_app(&app)
        .and_then(|service| service.read_group(group))
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn write_app_settings_group(
    app: tauri::AppHandle,
    group: AppSettingsGroup,
    value: Value,
) -> Result<Value, String> {
    // Existing UI hooks emit source-aware events after this command resolves. Suppress the
    // service event here so the initiating window does not replay its own change twice.
    AppSettingsService::from_app_with_events(&app, false)
        .and_then(|service| service.write_group(group, value))
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn replace_portable_app_settings(
    app: tauri::AppHandle,
    settings: Value,
) -> Result<Value, String> {
    let (stored, publication) = AppSettingsService::from_app(&app)
        .and_then(|service| service.replace_portable_settings_defer_publication(settings))
        .map_err(|error| error.to_string())?;
    publication.publish().map_err(|error| error.to_string())?;
    Ok(stored)
}

#[tauri::command]
#[cfg_attr(not(mobile), allow(dead_code))]
pub(crate) fn get_mcp_policy(app: tauri::AppHandle) -> Result<McpPolicySnapshot, String> {
    McpLocalSettingsService::from_app(&app)
        .and_then(|service| service.load_migrated())
        .map(McpPolicySnapshot::from)
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[cfg_attr(not(mobile), allow(dead_code))]
pub(crate) fn update_mcp_policy(
    app: tauri::AppHandle,
    input: UpdateMcpPolicyInput,
) -> Result<McpPolicySnapshot, String> {
    McpLocalSettingsService::from_app(&app)
        .and_then(|service| service.write(&input.expected_revision, input.config))
        .map(McpPolicySnapshot::from)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn read_exposed_app_settings(
    app: tauri::AppHandle,
) -> Result<ExposedAppSettings, String> {
    AppSettingsService::from_app(&app)
        .and_then(|service| service.read_exposed())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn patch_exposed_app_settings(
    app: tauri::AppHandle,
    patch: ExposedSettingsPatch,
) -> Result<ExposedAppSettings, String> {
    AppSettingsService::from_app(&app)
        .and_then(|service| service.patch_exposed(patch))
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    };
    use tempfile::tempdir;

    use super::*;

    #[derive(Default)]
    struct MemoryBackend {
        values: Mutex<BTreeMap<String, Value>>,
        fail_atomic_replaces: AtomicUsize,
        fail_save: Mutex<bool>,
        saves: AtomicUsize,
    }

    impl MemoryBackend {
        fn with(values: impl IntoIterator<Item = (&'static str, Value)>) -> Self {
            Self {
                values: Mutex::new(
                    values
                        .into_iter()
                        .map(|(key, value)| (key.to_string(), value))
                        .collect(),
                ),
                fail_atomic_replaces: AtomicUsize::new(0),
                fail_save: Mutex::new(false),
                saves: AtomicUsize::new(0),
            }
        }
    }

    impl SettingsBackend for MemoryBackend {
        fn get(&self, key: &str) -> Result<Option<Value>, AppSettingsError> {
            Ok(self.values.lock().expect("memory values").get(key).cloned())
        }

        fn set(&self, key: &str, value: Value) -> Result<(), AppSettingsError> {
            self.values
                .lock()
                .expect("memory values")
                .insert(key.to_string(), value);
            Ok(())
        }

        fn delete(&self, key: &str) -> Result<(), AppSettingsError> {
            self.values.lock().expect("memory values").remove(key);
            Ok(())
        }

        fn save(&self) -> Result<(), AppSettingsError> {
            self.saves.fetch_add(1, Ordering::Relaxed);
            if *self.fail_save.lock().expect("fail save") {
                Err(AppSettingsError::unavailable())
            } else {
                Ok(())
            }
        }

        fn replace_portable_atomically(
            &self,
            desired: &Map<String, Value>,
        ) -> Result<(), AppSettingsError> {
            if self
                .fail_atomic_replaces
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
                    remaining.checked_sub(1)
                })
                .is_ok()
            {
                return Err(AppSettingsError::unavailable());
            }
            let mut values = self.values.lock().expect("memory values");
            for key in PORTABLE_SETTINGS_KEYS {
                values.remove(key);
            }
            values.extend(
                desired
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone())),
            );
            self.saves.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }

    #[derive(Default)]
    struct MemoryEvents(Mutex<Vec<(String, Value)>>);

    impl SettingsEventSink for MemoryEvents {
        fn emit(&self, event: &str, payload: Value) -> Result<(), AppSettingsError> {
            self.0
                .lock()
                .expect("memory events")
                .push((event.to_string(), payload));
            Ok(())
        }
    }

    fn service_with_backend(backend: Arc<MemoryBackend>) -> AppSettingsService {
        AppSettingsService::new_for_test(backend, Some(Arc::new(MemoryEvents::default())))
    }

    fn merge_portable_for_test(
        service: &AppSettingsService,
        bytes: Option<&[u8]>,
    ) -> Result<DeferredSettingsPublication, AppSettingsError> {
        let revision = service.portable_settings_snapshot()?.revision().to_string();
        service.merge_portable_settings_bytes_defer_publication_with_preflight(
            bytes,
            &revision,
            || Ok(()),
            |_| Ok(()),
        )
    }

    #[test]
    fn registry_exposes_only_the_approved_field_names() {
        let expected = BTreeSet::from(EXPOSED_FIELDS);
        assert_eq!(
            AppSettingsService::exposed_field_names()
                .iter()
                .copied()
                .collect::<BTreeSet<_>>(),
            expected
        );
        assert_eq!(AppSettingsService::exposed_field_names().len(), 23);
        for forbidden in [
            "mcp.enabled",
            "workspace",
            "recentMarkdownFiles",
            "windowState",
            "customThemeCss",
            "export.pandocPath",
            "export.pandocArgs",
            "editor.markdownTemplates",
            "editor.markdownShortcuts",
            "editor.imageUpload",
            "updater.lastCheckedAt",
        ] {
            assert!(!AppSettingsService::exposed_field_names().contains(&forbidden));
        }
    }

    #[test]
    fn exposed_read_omits_unapproved_store_keys() {
        let backend = Arc::new(MemoryBackend::with([
            ("workspace", json!({ "path": "/private/notes" })),
            ("customThemeCss", json!("sentinel-secret")),
            (
                "exportSettings",
                json!({
                    "pandocPath": "/private/bin/pandoc",
                    "pdfAuthor": "QingYu"
                }),
            ),
        ]));
        let exposed = service_with_backend(backend)
            .read_exposed()
            .expect("read exposed settings");

        assert_eq!(exposed.values.len(), 23);
        assert!(exposed.credentials_present.is_empty());
        let serialized = serde_json::to_string(&exposed).expect("serialize exposed settings");
        assert!(!serialized.contains("secret"));
        assert!(!serialized.contains("/private/notes"));
        assert!(!serialized.contains("/private/bin/pandoc"));
    }

    #[test]
    fn file_ignore_rules_read_the_current_application_setting_once() {
        let backend = Arc::new(MemoryBackend::with([(
            FILE_IGNORE_SETTINGS_KEY,
            json!({ "rules": "drafts/\r\n*.tmp\rprivate/" }),
        )]));
        let service = service_with_backend(backend);

        assert_eq!(
            service.file_ignore_rules().unwrap().as_deref(),
            Some("drafts/\n*.tmp\nprivate/")
        );
    }

    #[test]
    fn deleting_downloaded_settings_clears_portable_store_keys_and_emits_defaults() {
        let backend = Arc::new(MemoryBackend::with([
            (LANGUAGE_KEY, json!("zh-CN")),
            (APPEARANCE_MODE_KEY, json!("dark")),
            ("localOnly", json!("preserved")),
        ]));
        let events = Arc::new(MemoryEvents::default());
        let service = AppSettingsService::new_for_test(backend.clone(), Some(events.clone()));

        merge_portable_for_test(&service, None)
            .unwrap()
            .publish()
            .unwrap();

        let values = backend.values.lock().unwrap();
        assert_eq!(values.get("localOnly"), Some(&json!("preserved")));
        assert!(!values.contains_key(LANGUAGE_KEY));
        assert!(!values.contains_key(APPEARANCE_MODE_KEY));
        drop(values);
        assert_eq!(backend.saves.load(Ordering::Relaxed), 1);
        let emitted = events.0.lock().unwrap();
        assert!(emitted.iter().any(|(event, payload)| {
            event == "markra://language-changed" && payload["language"] == json!("en")
        }));
        assert!(emitted.iter().any(|(event, payload)| {
            event == "markra://theme-changed"
                && payload["preferences"]["appearanceMode"] == json!("system")
        }));
        assert!(emitted
            .iter()
            .all(|(event, _)| event != "qingyu://settings-mcp-changed"));
    }

    #[test]
    fn failed_portable_merge_can_retry_and_emit_the_committed_value() {
        let backend = Arc::new(MemoryBackend::with([(LANGUAGE_KEY, json!("en"))]));
        backend.fail_atomic_replaces.store(1, Ordering::Relaxed);
        let events = Arc::new(MemoryEvents::default());
        let service = AppSettingsService::new_for_test(backend.clone(), Some(events.clone()));
        let downloaded = br#"{"language":"zh-CN"}"#;

        assert!(merge_portable_for_test(&service, Some(downloaded)).is_err());
        assert!(events.0.lock().unwrap().is_empty());

        merge_portable_for_test(&service, Some(downloaded))
            .unwrap()
            .publish()
            .unwrap();

        let emitted = events.0.lock().unwrap();
        assert_eq!(emitted.len(), 1);
        assert_eq!(emitted[0].0, "markra://language-changed");
        assert_eq!(emitted[0].1["language"], json!("zh-CN"));
    }

    #[test]
    fn failed_portable_merge_verification_atomically_restores_the_previous_snapshot() {
        let backend = Arc::new(MemoryBackend::with([
            (LANGUAGE_KEY, json!("en")),
            ("localOnly", json!({ "path": "/Workspace/A" })),
        ]));
        let service = service_with_backend(backend.clone());
        let revision = service.portable_settings_snapshot().unwrap().revision;

        let error = service
            .merge_portable_settings_bytes_defer_publication_with_preflight(
                Some(br#"{"language":"zh-CN"}"#),
                &revision,
                || Ok(()),
                |_| Err(AppSettingsError::reconcile_failed()),
            )
            .err()
            .expect("failed verification must roll back the atomic replacement");

        assert_eq!(error.code, "settings-reconcile-failed");
        let values = backend.values.lock().unwrap();
        assert_eq!(values.get(LANGUAGE_KEY), Some(&json!("en")));
        assert_eq!(
            values.get("localOnly"),
            Some(&json!({ "path": "/Workspace/A" }))
        );
        assert_eq!(backend.saves.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn portable_merge_events_are_derived_from_the_committed_store_snapshot() {
        let backend = Arc::new(MemoryBackend::with([(LANGUAGE_KEY, json!("en"))]));
        let events = Arc::new(MemoryEvents::default());
        let service = AppSettingsService::new_for_test(backend, Some(events.clone()));

        merge_portable_for_test(&service, Some(br#"{"language":"zh-CN"}"#))
            .unwrap()
            .publish()
            .unwrap();

        let emitted = events.0.lock().unwrap();
        assert_eq!(emitted.len(), 1);
        assert_eq!(emitted[0].1["language"], json!("zh-CN"));
    }

    #[test]
    fn post_rename_directory_sync_failure_updates_cache_before_returning_an_error() {
        let app_data = tempdir().unwrap();
        let app_data_root = app_data.path().canonicalize().unwrap();
        let settings_path = app_data_root.join(SETTINGS_STORE_PATH);
        let local_only = json!({ "path": "/Workspace/A" });
        let before = BTreeMap::from([
            (LANGUAGE_KEY.to_string(), json!("en")),
            ("workspace".to_string(), local_only.clone()),
        ]);
        fs::write(&settings_path, serde_json::to_vec(&before).unwrap()).unwrap();
        let desired = Map::from_iter([(LANGUAGE_KEY.to_string(), json!("zh-CN"))]);
        let replacement = BTreeMap::from([
            (LANGUAGE_KEY.to_string(), json!("zh-CN")),
            ("workspace".to_string(), local_only),
        ]);
        let replacement_bytes = serde_json::to_vec(&replacement).unwrap();
        let mut cache = before;

        let publication = replace_settings_file_atomically_with_directory_sync(
            &app_data_root,
            &replacement_bytes,
            |_| {
                Err(io::Error::other(
                    "injected post-rename directory sync failure",
                ))
            },
        )
        .expect("rename succeeded even though directory durability failed");
        let error = complete_settings_file_replacement(publication, || {
            for key in PORTABLE_SETTINGS_KEYS {
                cache.remove(key);
            }
            cache.extend(
                desired
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone())),
            );
        })
        .expect_err("post-rename durability failure remains observable");

        assert_eq!(error.code, "settings_unavailable");
        let disk: BTreeMap<String, Value> =
            serde_json::from_slice(&fs::read(settings_path).unwrap()).unwrap();
        assert_eq!(disk, replacement);
        assert_eq!(cache, disk);
    }

    #[test]
    fn portable_export_group_omits_the_local_pandoc_path() {
        let backend = Arc::new(MemoryBackend::with([(
            "exportSettings",
            json!({
                "pandocArgs": "--toc",
                "pandocPath": "/private/bin/pandoc",
                "pdfAuthor": "QingYu"
            }),
        )]));
        let export = service_with_backend(backend)
            .read_group(AppSettingsGroup::ExportSettings)
            .expect("read portable export settings")
            .expect("stored portable export settings");

        assert_eq!(export["pandocArgs"], json!("--toc"));
        assert_eq!(export["pdfAuthor"], json!("QingYu"));
        assert!(export.get("pandocPath").is_none());
    }

    fn portable_golden_store() -> Value {
        let mut store = serde_json::from_str::<Value>(include_str!(
            "../../../../packages/app/src/lib/settings/portable-settings.golden.json"
        ))
        .unwrap()["validStore"]
            .clone();
        store.as_object_mut().unwrap().remove("mcp");
        store
    }

    fn portable_import_payload() -> Value {
        let store = portable_golden_store();
        json!({
            "appearanceMode": store[APPEARANCE_MODE_KEY],
            "customThemeCss": {
                "dark": store["darkCustomThemeCss"],
                "light": store["lightCustomThemeCss"],
            },
            "darkTheme": store[DARK_THEME_KEY],
            "editorPreferences": store[EDITOR_PREFERENCES_KEY],
            "exportSettings": store[EXPORT_SETTINGS_KEY],
            "fileIgnoreSettings": store[FILE_IGNORE_SETTINGS_KEY],
            "language": store[LANGUAGE_KEY],
            "lightTheme": store[LIGHT_THEME_KEY],
        })
    }

    #[test]
    fn custom_theme_css_group_uses_one_typed_settings_write() {
        let backend = Arc::new(MemoryBackend::default());
        let service = service_with_backend(backend.clone());
        let value = json!({ "dark": "dark-css", "light": "light-css" });

        service
            .write_group(AppSettingsGroup::CustomThemeCss, value.clone())
            .expect("write custom theme CSS");

        assert_eq!(
            service
                .read_group(AppSettingsGroup::CustomThemeCss)
                .unwrap(),
            Some(value)
        );
        assert_eq!(backend.saves.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn imported_portable_settings_replace_all_groups_in_one_save_and_defer_all_events() {
        let backend = Arc::new(MemoryBackend::default());
        let events = Arc::new(MemoryEvents::default());
        let service = AppSettingsService::new_for_test(backend.clone(), Some(events.clone()));

        let (stored, publication) = service
            .replace_portable_settings_defer_publication(portable_import_payload())
            .expect("replace portable app settings");

        assert_eq!(stored, portable_golden_store());
        assert_eq!(backend.saves.load(Ordering::Relaxed), 1);
        assert!(events.0.lock().unwrap().is_empty());

        publication.publish().unwrap();
        let event_names = events
            .0
            .lock()
            .unwrap()
            .iter()
            .map(|(event, _)| event.clone())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            event_names,
            BTreeSet::from([
                "markra://custom-theme-css-changed".to_string(),
                "markra://editor-preferences-changed".to_string(),
                "markra://export-settings-changed".to_string(),
                "markra://file-ignore-settings-changed".to_string(),
                "markra://language-changed".to_string(),
                "markra://theme-changed".to_string(),
            ])
        );
    }

    fn portable_boundary_cases() -> Value {
        serde_json::from_str::<Value>(include_str!(
            "../../../../packages/app/src/lib/settings/portable-settings.golden.json"
        ))
        .unwrap()["boundaryCases"]
            .clone()
    }

    fn assert_remote_settings_invalid(value: &Value) {
        let error = validate_portable_settings_bytes(&serde_json::to_vec(value).unwrap())
            .expect_err("noncanonical portable settings must fail closed");
        assert_eq!(error.code, "remote-settings-invalid");
    }

    #[test]
    fn portable_settings_validator_accepts_the_complete_cross_language_golden_store() {
        validate_portable_settings_bytes(&serde_json::to_vec(&portable_golden_store()).unwrap())
            .unwrap();
    }

    #[test]
    fn portable_settings_validator_requires_complete_writer_owned_nested_objects() {
        let cases: &[&[&str]] = &[
            &["editorPreferences", "extendedSyntax", "highlight"],
            &["editorPreferences", "imageUpload", "fileNamePattern"],
            &["editorPreferences", "markdownShortcuts", "bold"],
            &["editorPreferences", "viewModeCustomizations", "outline"],
            &[
                "editorPreferences",
                "markdownTemplates",
                "0",
                "suggestedName",
            ],
            &["fileIgnoreSettings", "rules"],
            &["exportSettings", "pdfAuthor"],
        ];

        for path in cases {
            let mut store = portable_golden_store();
            let (last, parents) = path.split_last().unwrap();
            let mut target = &mut store;
            for key in parents {
                target = if let Ok(index) = key.parse::<usize>() {
                    &mut target.as_array_mut().unwrap()[index]
                } else {
                    &mut target[*key]
                };
            }
            target.as_object_mut().unwrap().remove(*last);
            assert_remote_settings_invalid(&store);
        }

        let mut incomplete_titlebar = portable_golden_store();
        incomplete_titlebar["editorPreferences"]["titlebarActions"]
            .as_array_mut()
            .unwrap()
            .pop();
        assert_remote_settings_invalid(&incomplete_titlebar);
    }

    #[test]
    fn portable_settings_validator_matches_typescript_shortcut_normalization() {
        let mutations = [
            ("bold", "mod+b"),
            ("bold", "CmdOrCtrl+B"),
            ("bold", "Mod+S"),
            ("bold", "Mod++B"),
            ("toggleViewMode", "Shift+F8"),
            ("toggleSourceMode", "Mod+Alt+V"),
        ];
        for (action, shortcut) in mutations {
            let mut store = portable_golden_store();
            store["editorPreferences"]["markdownShortcuts"][action] = json!(shortcut);
            assert_remote_settings_invalid(&store);
        }

        let mut duplicate = portable_golden_store();
        duplicate["editorPreferences"]["markdownShortcuts"]["bold"] = json!("Mod+I");
        assert_remote_settings_invalid(&duplicate);

        let mut valid_swap = portable_golden_store();
        valid_swap["editorPreferences"]["markdownShortcuts"]["bold"] = json!("Mod+I");
        valid_swap["editorPreferences"]["markdownShortcuts"]["italic"] = json!("Mod+B");
        validate_portable_settings_bytes(&serde_json::to_vec(&valid_swap).unwrap()).unwrap();
    }

    #[test]
    fn portable_settings_validator_rejects_duplicate_template_identity_and_filename() {
        let mut duplicate_id = portable_golden_store();
        duplicate_id["editorPreferences"]["markdownTemplates"] = json!([
            {"fileName":"daily.md","id":"daily","name":"Daily","suggestedName":""},
            {"fileName":"second.md","id":"daily","name":"Second","suggestedName":""}
        ]);
        assert_remote_settings_invalid(&duplicate_id);

        let mut duplicate_file_name = portable_golden_store();
        duplicate_file_name["editorPreferences"]["markdownTemplates"] = json!([
            {"fileName":"daily.md","id":"daily","name":"Daily","suggestedName":""},
            {"fileName":"DAILY.MD","id":"second","name":"Second","suggestedName":""}
        ]);
        assert_remote_settings_invalid(&duplicate_file_name);
    }

    #[test]
    fn portable_settings_validator_does_not_invent_template_string_limits() {
        let long = "文".repeat(501);
        let mut store = portable_golden_store();
        store["editorPreferences"]["markdownTemplates"] = json!([{
            "fileName": format!("{long}.md"),
            "id": long,
            "name": long,
            "suggestedName": long
        }]);

        validate_portable_settings_bytes(&serde_json::to_vec(&store).unwrap()).unwrap();
    }

    #[test]
    fn portable_settings_validator_uses_javascript_utf16_length_limits() {
        let mut within = portable_golden_store();
        within["lightCustomThemeCss"] = json!("😀".repeat(25_000));
        within["editorPreferences"]["editorFontFamily"] =
            json!({"family":"😀".repeat(80),"source":"system"});
        within["editorPreferences"]["imageUpload"]["fileNamePattern"] = json!("😀".repeat(60));
        within["fileIgnoreSettings"]["rules"] = json!("😀".repeat(25_000));
        within["exportSettings"]["pdfAuthor"] = json!("😀".repeat(100));
        validate_portable_settings_bytes(&serde_json::to_vec(&within).unwrap()).unwrap();

        for path in [
            &["lightCustomThemeCss"][..],
            &["editorPreferences", "editorFontFamily", "family"][..],
            &["editorPreferences", "imageUpload", "fileNamePattern"][..],
            &["fileIgnoreSettings", "rules"][..],
            &["exportSettings", "pdfAuthor"][..],
        ] {
            let mut over = within.clone();
            let mut target = &mut over;
            for key in path {
                target = &mut target[*key];
            }
            target.as_str().unwrap();
            *target = json!(format!("{}a", target.as_str().unwrap()));
            assert_remote_settings_invalid(&over);
        }
    }

    #[test]
    fn portable_settings_validator_requires_canonical_clipboard_folder() {
        for folder in [
            "",
            "/assets",
            "C:/assets",
            "C:\\assets",
            "assets\\screenshots",
            "assets//screenshots",
            "./assets",
            "assets/./screenshots",
            "assets/../outside",
            " assets",
            "assets/ ",
        ] {
            let mut store = portable_golden_store();
            store["editorPreferences"]["clipboardImageFolder"] = json!(folder);
            assert_remote_settings_invalid(&store);
        }

        let mut current_file = portable_golden_store();
        current_file["editorPreferences"]["clipboardImageFolder"] = json!(".");
        validate_portable_settings_bytes(&serde_json::to_vec(&current_file).unwrap()).unwrap();
    }

    #[test]
    fn portable_settings_validator_rejects_shared_clipboard_folder_boundaries() {
        for folder in portable_boundary_cases()["invalidClipboardImageFolders"]
            .as_array()
            .unwrap()
        {
            let mut store = portable_golden_store();
            store["editorPreferences"]["clipboardImageFolder"] = folder.clone();
            assert_remote_settings_invalid(&store);
        }
    }

    #[test]
    fn portable_settings_validator_rejects_shared_non_ascii_shortcut_boundaries() {
        for shortcut in portable_boundary_cases()["invalidShortcutKeys"]
            .as_array()
            .unwrap()
        {
            let mut store = portable_golden_store();
            store["editorPreferences"]["markdownShortcuts"]["bold"] = shortcut.clone();
            assert_remote_settings_invalid(&store);
        }
    }

    #[test]
    fn portable_settings_validator_requires_other_writer_normalized_values() {
        let mutations = [
            (
                &["editorPreferences", "editorFontFamily", "family"][..],
                json!(" Serif "),
            ),
            (
                &["editorPreferences", "imageUpload", "fileNamePattern"][..],
                json!(" pasted-image "),
            ),
            (&["fileIgnoreSettings", "rules"][..], json!("dist/\r\ntmp/")),
            (&["exportSettings", "pdfAuthor"][..], json!(" QingYu ")),
            (&["exportSettings", "pdfWidthMm"][..], json!(211)),
            (&["exportSettings", "pdfMarginPreset"][..], json!("narrow")),
        ];

        for (path, value) in mutations {
            let mut store = portable_golden_store();
            let mut target = &mut store;
            for key in path {
                target = &mut target[*key];
            }
            *target = value;
            assert_remote_settings_invalid(&store);
        }
    }

    #[test]
    fn portable_settings_validator_rejects_local_legacy_mcp_and_pandoc_fields() {
        for (group, key, value) in [
            (None, "theme", json!("dark")),
            (None, "customThemeCss", json!("secret")),
            (None, "mcpSettings", json!({"enabled":true})),
            (
                Some("exportSettings"),
                "pandocPath",
                json!("/private/bin/pandoc"),
            ),
        ] {
            let mut store = portable_golden_store();
            match group {
                Some(group) => store[group][key] = value,
                None => store[key] = value,
            }
            assert_remote_settings_invalid(&store);
        }
    }

    #[test]
    fn portable_settings_rejects_every_legacy_mcp_value() {
        let policy = serde_json::to_value(crate::mcp::config::McpConfig::default())
            .expect("serialize MCP policy");
        for value in [policy, json!("ignored"), Value::Null] {
            let invalid = json!({ "language": "en", "mcp": value });
            let error = validate_portable_settings_bytes(&serde_json::to_vec(&invalid).unwrap())
                .expect_err("MCP policy must stay device-local");
            assert_eq!(error.code, "remote-settings-invalid");
        }
    }

    #[test]
    fn legacy_remote_mcp_is_removed_before_strict_portable_validation() {
        let sanitized = sanitize_legacy_remote_portable_settings(
            br#"{"language":"zh-CN","mcp":"malformed-but-ignored"}"#,
        )
        .unwrap()
        .unwrap();
        let value: Value = serde_json::from_slice(&sanitized).unwrap();

        assert_eq!(value, json!({ "language": "zh-CN" }));
    }

    #[test]
    fn legacy_remote_mcp_cleanup_rejects_invalid_non_mcp_settings() {
        let error =
            sanitize_legacy_remote_portable_settings(br#"{"language":7,"mcp":{"enabled":true}}"#)
                .unwrap_err();

        assert_eq!(error.code, "remote-settings-invalid");
    }

    #[test]
    fn current_remote_settings_do_not_require_a_cleanup_write() {
        assert_eq!(
            sanitize_legacy_remote_portable_settings(br#"{"language":"zh-CN"}"#).unwrap(),
            None
        );
    }

    #[test]
    fn portable_snapshot_ignores_a_legacy_mcp_store_key() {
        let backend = Arc::new(MemoryBackend::with([
            (LANGUAGE_KEY, json!("zh-CN")),
            ("mcp", json!({ "version": 1, "enabled": true })),
        ]));
        let service = service_with_backend(backend);

        let snapshot = service.portable_settings_snapshot().unwrap();
        let value: Value = serde_json::from_slice(snapshot.bytes().unwrap()).unwrap();

        assert_eq!(value[LANGUAGE_KEY], json!("zh-CN"));
        assert!(value.get("mcp").is_none());
    }

    #[test]
    fn portable_settings_validator_rejects_unknown_local_and_invalid_fields() {
        for invalid in [
            json!({ "workspace": { "folderPath": "/private/notes" } }),
            json!({ "theme": "dark" }),
            json!({ "appearanceMode": 7 }),
            json!({ "editorPreferences": { "unknownField": true } }),
            json!({ "exportSettings": { "pandocPath": "/private/bin/pandoc" } }),
        ] {
            let error = validate_portable_settings_bytes(&serde_json::to_vec(&invalid).unwrap())
                .expect_err("invalid portable settings must fail closed");
            assert_eq!(error.code, "remote-settings-invalid");
            assert!(!error.to_string().contains("/private"));
        }
    }

    #[test]
    fn downloaded_settings_event_plan_uses_existing_groups_and_detects_deletion() {
        let before = br#"{
            "appearanceMode":"dark",
            "lightTheme":"light",
            "darkTheme":"night",
            "language":"zh-CN"
        }"#;
        let after = br#"{
            "appearanceMode":"light",
            "lightTheme":"light",
            "darkTheme":"night",
            "language":"zh-CN"
        }"#;
        let changed = portable_settings_change_events(Some(before), Some(after)).unwrap();
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].0, "markra://theme-changed");

        let deleted = portable_settings_change_events(Some(after), None).unwrap();
        assert!(deleted
            .iter()
            .any(|(event, _)| *event == "markra://theme-changed"));
        assert!(deleted
            .iter()
            .any(|(event, _)| *event == "markra://language-changed"));
    }

    #[test]
    fn patch_rejects_unknown_and_stale_values_before_writing() {
        let backend = Arc::new(MemoryBackend::default());
        let service = service_with_backend(backend.clone());
        let revision = service.read_exposed().expect("current settings").revision;

        let error = service
            .patch_exposed(ExposedSettingsPatch {
                expected_revision: revision.clone(),
                values: BTreeMap::from([("workspace.path".to_string(), json!("/tmp/escape"))]),
            })
            .expect_err("invalid patch must fail");
        assert_eq!(error.code, "invalid_settings_field");
        let stale = service
            .patch_exposed(ExposedSettingsPatch {
                expected_revision: "stale".to_string(),
                values: BTreeMap::from([("language".to_string(), json!("fr"))]),
            })
            .expect_err("stale patch must fail");
        assert_eq!(stale.code, "settings_revision_conflict");
        assert!(backend.values.lock().expect("memory values").is_empty());
    }

    #[test]
    fn appearance_accepts_dynamic_theme_ids_and_rejects_reserved_or_malformed_ids() {
        for value in [json!("nord-custom"), json!("light"), json!("dark")] {
            assert!(validate_field("appearance.lightTheme", &value).is_ok());
        }
        for value in [
            json!("-bad"),
            json!("Bad"),
            json!("qingyu-internal"),
            json!(""),
        ] {
            assert!(validate_field("appearance.darkTheme", &value).is_err());
        }
    }

    #[test]
    fn patch_commits_all_fields_once_and_restores_memory_after_save_failure() {
        let backend = Arc::new(MemoryBackend::default());
        let service = service_with_backend(backend.clone());
        let revision = service.read_exposed().expect("current settings").revision;
        let updated = service
            .patch_exposed(ExposedSettingsPatch {
                expected_revision: revision,
                values: BTreeMap::from([
                    ("appearance.mode".to_string(), json!("dark")),
                    ("language".to_string(), json!("zh-CN")),
                ]),
            })
            .expect("atomic settings patch");
        assert_eq!(updated.values["appearance.mode"], json!("dark"));
        assert_eq!(updated.values["language"], json!("zh-CN"));
        assert_eq!(backend.saves.load(Ordering::Relaxed), 1);

        let before_failure = backend.values.lock().expect("memory values").clone();
        *backend.fail_save.lock().expect("fail save") = true;
        let error = service
            .patch_exposed(ExposedSettingsPatch {
                expected_revision: updated.revision,
                values: BTreeMap::from([("language".to_string(), json!("fr"))]),
            })
            .expect_err("save failure must roll back memory");
        assert_eq!(error.code, "settings_unavailable");
        assert_eq!(
            *backend.values.lock().expect("memory values"),
            before_failure
        );
    }
}
