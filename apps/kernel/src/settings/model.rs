//! Platform-neutral settings model boundary.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Map, Value};
use sha2::{Digest as _, Sha256};

use crate::contract::{
    FiniteNumber, FontFamilyValueDto, Nullable, Revision, SafeInteger, SettingEntryDto, SettingKey,
    SettingValueDto, SettingsSnapshotDto,
};

pub const DEFAULT_LIGHT_THEME_ID: &str = "light";
pub const DEFAULT_DARK_THEME_ID: &str = "dark";
pub const PORTABLE_SETTINGS_MAX_BYTES: usize = 16 * 1024 * 1024;

pub const PORTABLE_SETTINGS_KEYS: [&str; 9] = [
    "appearanceMode",
    "lightThemeId",
    "darkThemeId",
    "lightCustomThemeCss",
    "darkCustomThemeCss",
    "language",
    "editorPreferences",
    "fileIgnoreSettings",
    "exportSettings",
];

#[derive(Clone, Eq, PartialEq)]
pub struct PortableSettingsSnapshot {
    bytes: Option<Vec<u8>>,
    revision: Revision,
}

impl PortableSettingsSnapshot {
    pub(crate) fn new(bytes: Option<Vec<u8>>) -> Result<Self, ModelError> {
        let revision = portable_revision(bytes.as_deref())?;
        Ok(Self { bytes, revision })
    }

    pub fn bytes(&self) -> Option<&[u8]> {
        self.bytes.as_deref()
    }

    pub const fn revision(&self) -> &Revision {
        &self.revision
    }
}

impl std::fmt::Debug for PortableSettingsSnapshot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PortableSettingsSnapshot")
            .field("byte_len", &self.bytes.as_ref().map(Vec::len))
            .field("revision", &self.revision)
            .finish()
    }
}

pub(crate) fn portable_revision(bytes: Option<&[u8]>) -> Result<Revision, ModelError> {
    Revision::parse(format!(
        "sha256:{:x}",
        Sha256::digest(bytes.unwrap_or_default())
    ))
    .map_err(|_| ModelError)
}

pub(crate) fn normalize_portable_value(key: &str, mut value: Value) -> Value {
    if key == "exportSettings" {
        if let Some(settings) = value.as_object_mut() {
            settings.remove("pandocPath");
            settings
                .entry("fontFamily".to_string())
                .or_insert(Value::Null);
        }
    } else if key == "editorPreferences" {
        if let Some(customizations) = value
            .get_mut("viewModeCustomizations")
            .and_then(Value::as_object_mut)
        {
            customizations.remove("recentFolders");
        }
    }
    value
}

pub(crate) fn validated_raw_value(entry: SettingEntryDto) -> Result<Value, ModelError> {
    let key = entry.key;
    let value = match entry.value {
        SettingValueDto::Boolean { value } => Value::Bool(value),
        SettingValueDto::Integer { value } => Value::from(value.get()),
        SettingValueDto::Number { value } => json!(value.get()),
        SettingValueDto::String { value } => Value::String(value),
        SettingValueDto::NullableInteger { value } => value
            .into_option()
            .map_or(Value::Null, |value| Value::from(value.get())),
        SettingValueDto::NullableString { value } => {
            value.into_option().map_or(Value::Null, Value::String)
        }
        SettingValueDto::FontFamily { value } => match value {
            FontFamilyValueDto::Theme { family } => {
                if family.as_ref().is_some() {
                    return Err(ModelError);
                }
                json!({ "source": "theme", "family": null })
            }
            FontFamilyValueDto::System { family } => {
                json!({ "source": "system", "family": family })
            }
        },
    };
    validate_setting_value(key, &value)?;
    Ok(value)
}

fn validate_setting_value(key: SettingKey, value: &Value) -> Result<(), ModelError> {
    let valid = match key {
        SettingKey::AppearanceMode => string_in(value, &["system", "light", "dark"]),
        SettingKey::AppearanceLightTheme | SettingKey::AppearanceDarkTheme => valid_theme_id(value),
        SettingKey::ThemeCustomCssLight | SettingKey::ThemeCustomCssDark => {
            value.as_str().is_some_and(|css| utf16_len(css) <= 50_000)
        }
        SettingKey::Language => string_in(
            value,
            &[
                "en", "zh-CN", "zh-TW", "ja", "ko", "fr", "de", "es", "pt-BR", "it", "ru",
            ],
        ),
        SettingKey::EditorBodyFontSize => integer_in(value, &[14, 15, 16, 17, 18, 20]),
        SettingKey::EditorContentWidth => string_in(value, &["narrow", "default", "wide"]),
        SettingKey::EditorContentWidthPx => value.is_null() || integer_between(value, 640, 1_280),
        SettingKey::EditorFontFamily => valid_font_family(value),
        SettingKey::EditorLineHeight => value
            .as_f64()
            .is_some_and(|number| [1.5, 1.65, 1.8].contains(&number)),
        SettingKey::EditorParagraphSpacingPx => integer_between(value, 0, 32),
        SettingKey::EditorShowWordCount | SettingKey::EditorWrapCodeBlocks => value.is_boolean(),
        SettingKey::EditorViewMode => {
            string_in(value, &["full", "daily", "focus", "immersive", "custom"])
        }
        SettingKey::FilesIgnoreRules => value.as_str().is_some_and(|rules| rules.len() <= 50_000),
        SettingKey::ExportFontFamily => valid_optional_font_family(value),
        SettingKey::ExportPdfAuthor | SettingKey::ExportPdfFooter | SettingKey::ExportPdfHeader => {
            value.as_str().is_some_and(|text| utf16_len(text) <= 200)
        }
        SettingKey::ExportPdfHeightMm | SettingKey::ExportPdfWidthMm => {
            integer_between(value, 50, 2_000)
        }
        SettingKey::ExportPdfMarginMm => integer_between(value, 0, 60),
        SettingKey::ExportPdfMarginPreset => string_in(
            value,
            &["custom", "default", "narrow", "none", "normal", "wide"],
        ),
        SettingKey::ExportPdfPageBreakOnH1 => value.is_boolean(),
        SettingKey::ExportPdfPageSize => string_in(value, &["a4", "custom", "default", "letter"]),
    };
    valid.then_some(()).ok_or(ModelError)
}

pub(crate) fn storage_target(key: SettingKey) -> (&'static str, Option<&'static str>) {
    match key {
        SettingKey::AppearanceMode => ("appearanceMode", None),
        SettingKey::AppearanceLightTheme => ("lightThemeId", None),
        SettingKey::AppearanceDarkTheme => ("darkThemeId", None),
        SettingKey::ThemeCustomCssLight => ("lightCustomThemeCss", None),
        SettingKey::ThemeCustomCssDark => ("darkCustomThemeCss", None),
        SettingKey::Language => ("language", None),
        SettingKey::EditorBodyFontSize => ("editorPreferences", Some("bodyFontSize")),
        SettingKey::EditorContentWidth => ("editorPreferences", Some("contentWidth")),
        SettingKey::EditorContentWidthPx => ("editorPreferences", Some("contentWidthPx")),
        SettingKey::EditorFontFamily => ("editorPreferences", Some("editorFontFamily")),
        SettingKey::EditorLineHeight => ("editorPreferences", Some("lineHeight")),
        SettingKey::EditorParagraphSpacingPx => ("editorPreferences", Some("paragraphSpacingPx")),
        SettingKey::EditorShowWordCount => ("editorPreferences", Some("showWordCount")),
        SettingKey::EditorWrapCodeBlocks => ("editorPreferences", Some("wrapCodeBlocks")),
        SettingKey::EditorViewMode => ("editorPreferences", Some("viewMode")),
        SettingKey::FilesIgnoreRules => ("fileIgnoreSettings", Some("rules")),
        SettingKey::ExportFontFamily => ("exportSettings", Some("fontFamily")),
        SettingKey::ExportPdfAuthor => ("exportSettings", Some("pdfAuthor")),
        SettingKey::ExportPdfFooter => ("exportSettings", Some("pdfFooter")),
        SettingKey::ExportPdfHeader => ("exportSettings", Some("pdfHeader")),
        SettingKey::ExportPdfHeightMm => ("exportSettings", Some("pdfHeightMm")),
        SettingKey::ExportPdfWidthMm => ("exportSettings", Some("pdfWidthMm")),
        SettingKey::ExportPdfMarginMm => ("exportSettings", Some("pdfMarginMm")),
        SettingKey::ExportPdfMarginPreset => ("exportSettings", Some("pdfMarginPreset")),
        SettingKey::ExportPdfPageBreakOnH1 => ("exportSettings", Some("pdfPageBreakOnH1")),
        SettingKey::ExportPdfPageSize => ("exportSettings", Some("pdfPageSize")),
    }
}

fn string_in(value: &Value, allowed: &[&str]) -> bool {
    value
        .as_str()
        .is_some_and(|candidate| allowed.contains(&candidate))
}

fn integer_in(value: &Value, allowed: &[i64]) -> bool {
    value
        .as_i64()
        .is_some_and(|candidate| allowed.contains(&candidate))
}

fn integer_between(value: &Value, minimum: i64, maximum: i64) -> bool {
    value
        .as_i64()
        .is_some_and(|candidate| (minimum..=maximum).contains(&candidate))
}

fn utf16_len(value: &str) -> usize {
    value.encode_utf16().count()
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

fn valid_font_family(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    if object.len() != 2 || !object.contains_key("source") || !object.contains_key("family") {
        return false;
    }
    match object.get("source").and_then(Value::as_str) {
        Some("theme") => object.get("family").is_some_and(Value::is_null),
        Some("system") => object
            .get("family")
            .and_then(Value::as_str)
            .is_some_and(valid_font_name),
        _ => false,
    }
}

fn valid_optional_font_family(value: &Value) -> bool {
    value.is_null() || value.as_str().is_some_and(valid_font_name)
}

fn valid_font_name(family: &str) -> bool {
    !family.is_empty()
        && family.trim() == family
        && utf16_len(family) <= 160
        && !family
            .chars()
            .any(|character| character <= '\u{001f}' || character == '\u{007f}')
}

pub fn validate_portable_settings_bytes(bytes: &[u8]) -> Result<(), PortableSettingsError> {
    if bytes.len() > PORTABLE_SETTINGS_MAX_BYTES {
        return Err(PortableSettingsError);
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|_| PortableSettingsError)?;
    let object = value.as_object().ok_or(PortableSettingsError)?;
    let allowed = BTreeSet::from(PORTABLE_SETTINGS_KEYS);
    if object.keys().any(|key| !allowed.contains(key.as_str())) {
        return Err(PortableSettingsError);
    }
    for (key, value) in object {
        let valid = match key.as_str() {
            "appearanceMode" => string_in(value, &["system", "light", "dark"]),
            "lightThemeId" | "darkThemeId" => valid_theme_id(value),
            "lightCustomThemeCss" | "darkCustomThemeCss" => {
                value.as_str().is_some_and(|css| utf16_len(css) <= 50_000)
            }
            "language" => string_in(
                value,
                &[
                    "en", "zh-CN", "zh-TW", "ja", "ko", "fr", "de", "es", "pt-BR", "it", "ru",
                ],
            ),
            "editorPreferences" => valid_portable_editor_preferences(value),
            "fileIgnoreSettings" => valid_portable_file_ignore_settings(value),
            "exportSettings" => valid_portable_export_settings(value),
            _ => false,
        };
        if !valid {
            return Err(PortableSettingsError);
        }
    }
    Ok(())
}

pub fn sanitize_legacy_remote_portable_settings(
    bytes: &[u8],
) -> Result<Option<Vec<u8>>, PortableSettingsError> {
    if bytes.len() > PORTABLE_SETTINGS_MAX_BYTES {
        return Err(PortableSettingsError);
    }
    let mut value: Value = serde_json::from_slice(bytes).map_err(|_| PortableSettingsError)?;
    let object = value.as_object_mut().ok_or(PortableSettingsError)?;
    let mut changed = object.remove("mcp").is_some();
    if let Some(export) = object
        .get_mut("exportSettings")
        .and_then(Value::as_object_mut)
    {
        if !export.contains_key("fontFamily") {
            export.insert("fontFamily".to_string(), Value::Null);
            changed = true;
        }
    }
    if !changed {
        return Ok(None);
    }
    let sanitized = serde_json::to_vec(&value).map_err(|_| PortableSettingsError)?;
    validate_portable_settings_bytes(&sanitized)?;
    Ok(Some(sanitized))
}

pub fn portable_settings_from_bytes(bytes: Option<&[u8]>) -> Result<Value, PortableSettingsError> {
    let raw = match bytes {
        Some(bytes) => {
            validate_portable_settings_bytes(bytes)?;
            serde_json::from_slice::<Value>(bytes).map_err(|_| PortableSettingsError)?
        }
        None => Value::Object(Map::new()),
    };
    let raw = raw.as_object().ok_or(PortableSettingsError)?;
    let mut portable = Map::new();
    for key in PORTABLE_SETTINGS_KEYS {
        if let Some(value) = raw.get(key).cloned() {
            portable.insert(key.to_string(), normalize_portable_value(key, value));
        }
    }
    Ok(Value::Object(portable))
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
    let Some(object) = object_has_exact(value, KEYS) else {
        return false;
    };
    object.iter().all(|(key, value)| match key.as_str() {
        "autoRevealActiveFile"
        | "autoSaveEnabled"
        | "autoUpdateEnabled"
        | "documentLinksOpen"
        | "documentLinksVisible"
        | "hideHeadingMarkersOnFocus"
        | "openDroppedFilesInTabs"
        | "restoreWorkspaceOnStartup"
        | "showDocumentTabs"
        | "showLineNumbers"
        | "showWordCount"
        | "typewriterModeEnabled"
        | "vimModeEnabled"
        | "wrapCodeBlocks" => value.is_boolean(),
        "autoSaveIntervalMinutes" => integer_between(value, 1, 120),
        "bodyFontSize" => integer_in(value, &[14, 15, 16, 17, 18, 20]),
        "clipboardImageFolder" => valid_portable_relative_folder(value),
        "contentWidth" => string_in(value, &["narrow", "default", "wide"]),
        "contentWidthPx" => value.is_null() || integer_between(value, 640, 1_280),
        "editorFontFamily" => valid_font_family(value),
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

fn valid_image_upload_settings(value: &Value) -> bool {
    object_has_exact(value, &["fileNamePattern"]).is_some_and(|object| {
        object
            .get("fileNamePattern")
            .and_then(Value::as_str)
            .is_some_and(|pattern| {
                !pattern.is_empty()
                    && pattern.trim() == pattern
                    && utf16_len(pattern) <= 120
                    && !pattern.contains('/')
                    && !pattern.contains('\\')
                    && pattern != "."
                    && pattern != ".."
            })
    })
}

fn valid_markdown_shortcuts(value: &Value) -> bool {
    const SHORTCUTS: &[(&str, &str, Option<&str>)] = &[
        ("openQuickOpen", "Mod+P", None),
        ("syncNow", "Mod+Alt+R", None),
        ("toggleMarkdownFiles", "Mod+Shift+M", None),
        ("toggleDocumentHistory", "Mod+Shift+H", None),
        ("toggleSourceMode", "Mod+Alt+S", Some("Mod+Alt+V")),
        ("toggleReadOnlyMode", "Mod+Alt+L", None),
        ("toggleTypewriterMode", "Mod+Shift+Y", Some("Mod+Alt+W")),
        ("toggleVimMode", "Mod+Alt+V", None),
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
        let candidate = candidates.get(action).expect("all candidates exist");
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
        "sidebarLayout",
        "statusBar",
        "titlebarActions",
        "viewModeToggle",
        "wordCount",
    ];
    value.as_object().is_some_and(|object| {
        object
            .keys()
            .all(|key| KEYS.contains(&key.as_str()) || key == "recentFolders")
            && KEYS.iter().all(|key| {
                object
                    .get(*key)
                    .is_some_and(|visibility| string_in(visibility, &["visible", "hidden"]))
            })
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
            "fontFamily" => valid_optional_font_family(value),
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PortableSettingsError;

impl std::fmt::Display for PortableSettingsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("remote portable settings are invalid")
    }
}

impl std::error::Error for PortableSettingsError {}

pub(crate) fn default_editor() -> Map<String, Value> {
    json!({
        "autoRevealActiveFile": false,
        "autoSaveEnabled": true,
        "autoSaveIntervalMinutes": 10,
        "autoUpdateEnabled": true,
        "bodyFontSize": 16,
        "clipboardImageFolder": "assets",
        "contentWidth": "default",
        "contentWidthPx": null,
        "documentLinksOpen": true,
        "documentLinksVisible": false,
        "editorFontFamily": { "family": null, "source": "theme" },
        "extendedSyntax": {
            "githubAlerts": true,
            "highlight": true
        },
        "imageUpload": {
            "fileNamePattern": "pasted-image-{timestamp}"
        },
        "lineHeight": 1.65,
        "markdownShortcuts": {
            "bold": "Mod+B",
            "bulletList": "Mod+Shift+8",
            "codeBlock": "Mod+Alt+C",
            "heading1": "Mod+Alt+1",
            "heading2": "Mod+Alt+2",
            "heading3": "Mod+Alt+3",
            "image": "Mod+Shift+I",
            "inlineCode": "Mod+E",
            "italic": "Mod+I",
            "link": "Mod+K",
            "openQuickOpen": "Mod+P",
            "orderedList": "Mod+Shift+7",
            "paragraph": "Mod+Alt+0",
            "quote": "Mod+Shift+B",
            "strikethrough": "Mod+Shift+X",
            "syncNow": "Mod+Alt+R",
            "table": "Mod+Shift+Alt+T",
            "toggleAllFolds": "Mod+Alt+T",
            "toggleDocumentHistory": "Mod+Shift+H",
            "toggleMarkdownFiles": "Mod+Shift+M",
            "toggleReadOnlyMode": "Mod+Alt+L",
            "toggleSourceMode": "Mod+Alt+S",
            "toggleTypewriterMode": "Mod+Shift+Y",
            "toggleViewMode": "F8",
            "toggleVimMode": "Mod+Alt+V"
        },
        "markdownTemplates": [],
        "openDroppedFilesInTabs": false,
        "paragraphSpacingPx": 8,
        "restoreWorkspaceOnStartup": true,
        "sidebarLayoutMode": "stacked",
        "showDocumentTabs": true,
        "splitVisualPanePercent": 50,
        "tableColumnWidthMode": "auto",
        "titlebarActions": [
            { "id": "viewMode", "visible": true },
            { "id": "sourceMode", "visible": true },
            { "id": "history", "visible": true },
            { "id": "save", "visible": true },
            { "id": "theme", "visible": true }
        ],
        "typewriterModeEnabled": false,
        "viewModeCustomizations": {
            "documentLinks": "visible",
            "documentTabs": "visible",
            "fileList": "visible",
            "fileTree": "visible",
            "fileTreeButton": "visible",
            "openButton": "visible",
            "outline": "visible",
            "quickCreateButton": "visible",
            "sidebarLayout": "visible",
            "statusBar": "visible",
            "titlebarActions": "visible",
            "viewModeToggle": "visible",
            "wordCount": "visible"
        },
        "hideHeadingMarkersOnFocus": false,
        "showLineNumbers": false,
        "showWordCount": true,
        "vimModeEnabled": false,
        "wrapCodeBlocks": true,
        "viewMode": "daily"
    })
    .as_object()
    .cloned()
    .expect("default editor settings are an object")
}

pub(crate) fn default_export() -> Map<String, Value> {
    json!({
        "fontFamily": null,
        "pandocArgs": "",
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
    .as_object()
    .cloned()
    .expect("default export settings are an object")
}

pub(crate) fn merge_defaults(
    mut defaults: Map<String, Value>,
    stored: Option<Value>,
) -> Map<String, Value> {
    if let Some(stored) = stored.and_then(|value| value.as_object().cloned()) {
        defaults.extend(stored);
    }
    defaults
}

pub(crate) fn snapshot_from_values(
    values: &BTreeMap<String, Value>,
) -> Result<SettingsSnapshotDto, ModelError> {
    let revision_bytes = serde_json::to_vec(values).map_err(|_| ModelError)?;
    let revision =
        Revision::parse(format!("{:x}", Sha256::digest(revision_bytes))).map_err(|_| ModelError)?;

    let value = |key: &str| values.get(key).cloned().ok_or(ModelError);
    let string = |key: &str| value(key)?.as_str().map(str::to_string).ok_or(ModelError);
    let integer = |key: &str| {
        value(key)?
            .as_i64()
            .ok_or(ModelError)
            .and_then(|number| SafeInteger::new(number).map_err(|_| ModelError))
    };
    let boolean = |key: &str| value(key)?.as_bool().ok_or(ModelError);

    let content_width_px = match value("editor.contentWidthPx")? {
        Value::Null => Nullable::null(),
        number => Nullable::value(
            SafeInteger::new(number.as_i64().ok_or(ModelError)?).map_err(|_| ModelError)?,
        ),
    };
    let editor_font = match value("editor.fontFamily")? {
        Value::Object(font) if font.get("source").and_then(Value::as_str) == Some("theme") => {
            FontFamilyValueDto::Theme {
                family: Nullable::null(),
            }
        }
        Value::Object(font) if font.get("source").and_then(Value::as_str) == Some("system") => {
            FontFamilyValueDto::System {
                family: font
                    .get("family")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .ok_or(ModelError)?,
            }
        }
        _ => return Err(ModelError),
    };
    let export_font = match value("export.fontFamily")? {
        Value::Null => Nullable::null(),
        Value::String(family) => Nullable::value(family),
        _ => return Err(ModelError),
    };

    let snapshot = SettingsSnapshotDto {
        revision,
        values: vec![
            string_entry(SettingKey::AppearanceMode, string("appearance.mode")?),
            string_entry(
                SettingKey::AppearanceLightTheme,
                string("appearance.lightTheme")?,
            ),
            string_entry(
                SettingKey::AppearanceDarkTheme,
                string("appearance.darkTheme")?,
            ),
            string_entry(
                SettingKey::ThemeCustomCssLight,
                string("theme.customCss.light")?,
            ),
            string_entry(
                SettingKey::ThemeCustomCssDark,
                string("theme.customCss.dark")?,
            ),
            string_entry(SettingKey::Language, string("language")?),
            integer_entry(
                SettingKey::EditorBodyFontSize,
                integer("editor.bodyFontSize")?,
            ),
            string_entry(
                SettingKey::EditorContentWidth,
                string("editor.contentWidth")?,
            ),
            SettingEntryDto {
                key: SettingKey::EditorContentWidthPx,
                value: SettingValueDto::NullableInteger {
                    value: content_width_px,
                },
            },
            SettingEntryDto {
                key: SettingKey::EditorFontFamily,
                value: SettingValueDto::FontFamily { value: editor_font },
            },
            SettingEntryDto {
                key: SettingKey::EditorLineHeight,
                value: SettingValueDto::Number {
                    value: FiniteNumber::new(
                        value("editor.lineHeight")?.as_f64().ok_or(ModelError)?,
                    )
                    .map_err(|_| ModelError)?,
                },
            },
            integer_entry(
                SettingKey::EditorParagraphSpacingPx,
                integer("editor.paragraphSpacingPx")?,
            ),
            boolean_entry(
                SettingKey::EditorShowWordCount,
                boolean("editor.showWordCount")?,
            ),
            boolean_entry(
                SettingKey::EditorWrapCodeBlocks,
                boolean("editor.wrapCodeBlocks")?,
            ),
            string_entry(SettingKey::EditorViewMode, string("editor.viewMode")?),
            string_entry(SettingKey::FilesIgnoreRules, string("files.ignoreRules")?),
            SettingEntryDto {
                key: SettingKey::ExportFontFamily,
                value: SettingValueDto::NullableString { value: export_font },
            },
            string_entry(SettingKey::ExportPdfAuthor, string("export.pdfAuthor")?),
            string_entry(SettingKey::ExportPdfFooter, string("export.pdfFooter")?),
            string_entry(SettingKey::ExportPdfHeader, string("export.pdfHeader")?),
            integer_entry(
                SettingKey::ExportPdfHeightMm,
                integer("export.pdfHeightMm")?,
            ),
            integer_entry(SettingKey::ExportPdfWidthMm, integer("export.pdfWidthMm")?),
            integer_entry(
                SettingKey::ExportPdfMarginMm,
                integer("export.pdfMarginMm")?,
            ),
            string_entry(
                SettingKey::ExportPdfMarginPreset,
                string("export.pdfMarginPreset")?,
            ),
            boolean_entry(
                SettingKey::ExportPdfPageBreakOnH1,
                boolean("export.pdfPageBreakOnH1")?,
            ),
            string_entry(SettingKey::ExportPdfPageSize, string("export.pdfPageSize")?),
        ],
    };
    for entry in &snapshot.values {
        validated_raw_value(entry.clone())?;
    }
    Ok(snapshot)
}

fn string_entry(key: SettingKey, value: String) -> SettingEntryDto {
    SettingEntryDto {
        key,
        value: SettingValueDto::String { value },
    }
}

fn integer_entry(key: SettingKey, value: SafeInteger) -> SettingEntryDto {
    SettingEntryDto {
        key,
        value: SettingValueDto::Integer { value },
    }
}

fn boolean_entry(key: SettingKey, value: bool) -> SettingEntryDto {
    SettingEntryDto {
        key,
        value: SettingValueDto::Boolean { value },
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ModelError;
