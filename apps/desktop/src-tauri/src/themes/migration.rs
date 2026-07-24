use std::path::PathBuf;

use serde_json::{json, Value};
use tauri::{Manager, Runtime};
use tauri_plugin_store::StoreExt;

use super::{
    archive::PreparedThemeImport, catalog::ThemeCatalog, parser::parse_theme_file,
    InvalidThemeFile, ThemeAppearance, ThemeCatalogSnapshot, ThemeDescriptor, ThemeError,
    ThemeErrorCode,
};

const SETTINGS_STORE_PATH: &str = "settings.json";
const CATALOG_VERSION_KEY: &str = "themeCatalogVersion";
const CATALOG_VERSION: i64 = 2;
const LIGHT_THEME_ID_KEY: &str = "lightThemeId";
const DARK_THEME_ID_KEY: &str = "darkThemeId";

pub(crate) fn initialize_catalog<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<ThemeCatalogSnapshot, ThemeError> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|error| ThemeError::new(ThemeErrorCode::Io, error.to_string()))?
        .join("themes");
    let catalog = ThemeCatalog::at(root);
    let store = app.store(SETTINGS_STORE_PATH).map_err(|_| {
        ThemeError::new(
            ThemeErrorCode::Io,
            "The QingYu settings store is unavailable.",
        )
    })?;
    let stored_catalog_version = store
        .get(CATALOG_VERSION_KEY)
        .and_then(|value| value.as_i64())
        .unwrap_or(0)
        .max(0);
    let seed_diagnostics = initialize_catalog_files(&catalog, stored_catalog_version)?;
    if stored_catalog_version >= CATALOG_VERSION {
        return scan_with_diagnostics(&catalog, seed_diagnostics);
    }
    if !should_migrate_legacy_preferences(stored_catalog_version) {
        store.set(CATALOG_VERSION_KEY, json!(CATALOG_VERSION));
        if store.save().is_err() {
            store.set(CATALOG_VERSION_KEY, json!(stored_catalog_version));
            return Err(ThemeError::new(
                ThemeErrorCode::Io,
                "Theme catalog settings could not be saved.",
            ));
        }
        return scan_with_diagnostics(&catalog, seed_diagnostics);
    }

    let legacy_theme = store.get("theme").and_then(json_string);
    let mut appearance_mode = store
        .get("appearanceMode")
        .and_then(json_string)
        .filter(|value| matches!(value.as_str(), "system" | "light" | "dark"))
        .unwrap_or_else(|| appearance_from_legacy(legacy_theme.as_deref()).to_string());
    let mut light_theme_id = store
        .get("lightTheme")
        .and_then(json_string)
        .unwrap_or_else(|| legacy_light_theme(legacy_theme.as_deref()));
    let mut dark_theme_id = store
        .get("darkTheme")
        .and_then(json_string)
        .unwrap_or_else(|| legacy_dark_theme(legacy_theme.as_deref()));

    if light_theme_id == "custom" {
        let css = store
            .get("lightCustomThemeCss")
            .or_else(|| store.get("customThemeCss"))
            .and_then(json_string)
            .unwrap_or_default();
        light_theme_id = migrate_custom_theme(&catalog, ThemeAppearance::Light, &css)?.id;
    }
    if dark_theme_id == "custom" {
        let css = store
            .get("darkCustomThemeCss")
            .or_else(|| store.get("customThemeCss"))
            .and_then(json_string)
            .unwrap_or_default();
        dark_theme_id = migrate_custom_theme(&catalog, ThemeAppearance::Dark, &css)?.id;
    }

    let snapshot = catalog.scan()?;
    if !snapshot
        .themes
        .iter()
        .any(|theme| theme.id == light_theme_id)
        && light_theme_id != "light"
    {
        light_theme_id = "light".to_string();
    }
    if !snapshot
        .themes
        .iter()
        .any(|theme| theme.id == dark_theme_id)
        && dark_theme_id != "dark"
    {
        dark_theme_id = "dark".to_string();
    }
    if !matches!(appearance_mode.as_str(), "system" | "light" | "dark") {
        appearance_mode = "system".to_string();
    }

    store.set("appearanceMode", json!(appearance_mode));
    store.set(LIGHT_THEME_ID_KEY, json!(light_theme_id));
    store.set(DARK_THEME_ID_KEY, json!(dark_theme_id));
    store.set(CATALOG_VERSION_KEY, json!(CATALOG_VERSION));
    if store.save().is_err() {
        store.delete(CATALOG_VERSION_KEY);
        return Err(ThemeError::new(
            ThemeErrorCode::Io,
            "Theme catalog settings could not be saved.",
        ));
    }

    scan_with_diagnostics(&catalog, seed_diagnostics)
}

pub(crate) fn theme_directory<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<PathBuf, ThemeError> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("themes"))
        .map_err(|error| ThemeError::new(ThemeErrorCode::Io, error.to_string()))
}

fn initialize_catalog_files(
    catalog: &ThemeCatalog,
    catalog_version: i64,
) -> Result<Vec<InvalidThemeFile>, ThemeError> {
    match catalog_version {
        i64::MIN..=0 => {
            catalog.seed_missing()?;
            catalog.seed_missing_drake()
        }
        1 => catalog.seed_missing_drake(),
        _ => catalog.drake_seed_diagnostics(),
    }
}

fn should_migrate_legacy_preferences(catalog_version: i64) -> bool {
    catalog_version <= 0
}

fn scan_with_diagnostics(
    catalog: &ThemeCatalog,
    seed_diagnostics: Vec<InvalidThemeFile>,
) -> Result<ThemeCatalogSnapshot, ThemeError> {
    let mut snapshot = catalog.scan()?;
    for diagnostic in seed_diagnostics {
        snapshot
            .invalid_files
            .retain(|current| current.file_name != diagnostic.file_name);
        snapshot.invalid_files.push(diagnostic);
    }
    snapshot
        .invalid_files
        .sort_by(|left, right| left.file_name.cmp(&right.file_name));
    Ok(snapshot)
}

fn migrate_custom_theme(
    catalog: &ThemeCatalog,
    appearance: ThemeAppearance,
    css: &str,
) -> Result<ThemeDescriptor, ThemeError> {
    let base_id = match appearance {
        ThemeAppearance::Light => "migrated-custom-light",
        ThemeAppearance::Dark => "migrated-custom-dark",
    };
    let existing = catalog.scan()?;
    if let Some(theme) = existing
        .themes
        .into_iter()
        .find(|theme| theme.id == base_id)
    {
        return Ok(theme);
    }
    let (name, background, panel, text, accent) = match appearance {
        ThemeAppearance::Light => (
            "Migrated Custom Light",
            "#ffffff",
            "#f6f8fa",
            "#1f2328",
            "#0969da",
        ),
        ThemeAppearance::Dark => (
            "Migrated Custom Dark",
            "#1e1e1e",
            "#252526",
            "#e0e0e0",
            "#f4f4f5",
        ),
    };
    let appearance_value = match appearance {
        ThemeAppearance::Light => "light",
        ThemeAppearance::Dark => "dark",
    };
    let rewritten = rewrite_legacy_custom_selectors(css, base_id);
    let body = if rewritten.trim().is_empty() {
        format!(":root {{ --bg-primary: {background}; --bg-secondary: {panel}; --text-primary: {text}; --accent: {accent}; }}")
    } else {
        rewritten
    };
    let bytes = format!(
        "/*\n@qingyu-theme\nid: {base_id}\nname: {name}\nappearance: {appearance_value}\npreview-background: {background}\npreview-panel: {panel}\npreview-text: {text}\npreview-accent: {accent}\n*/\n\n{body}\n"
    )
    .into_bytes();
    let parsed = parse_theme_file(&bytes, "migrated.css")?;
    catalog.import_prepared(PreparedThemeImport::LegacyCss(parsed))
}

fn rewrite_legacy_custom_selectors(css: &str, id: &str) -> String {
    css.replace("data-theme=\"custom\"", &format!("data-theme=\"{id}\""))
        .replace("data-theme='custom'", &format!("data-theme='{id}'"))
        .replace(
            "data-editor-theme=\"custom\"",
            &format!("data-editor-theme=\"{id}\""),
        )
        .replace(
            "data-editor-theme='custom'",
            &format!("data-editor-theme='{id}'"),
        )
}

fn json_string(value: Value) -> Option<String> {
    value.as_str().map(str::to_string)
}

fn appearance_from_legacy(theme: Option<&str>) -> &'static str {
    match theme {
        Some("system") | None => "system",
        Some(theme) if is_dark_seed(theme) => "dark",
        Some(_) => "light",
    }
}

fn legacy_light_theme(theme: Option<&str>) -> String {
    match theme {
        Some(theme) if !is_dark_seed(theme) && theme != "system" => theme.to_string(),
        _ => "light".to_string(),
    }
}

fn legacy_dark_theme(theme: Option<&str>) -> String {
    match theme {
        Some(theme) if is_dark_seed(theme) => theme.to_string(),
        _ => "dark".to_string(),
    }
}

fn is_dark_seed(id: &str) -> bool {
    matches!(
        id,
        "dark"
            | "github-dark"
            | "one-dark"
            | "one-dark-pro"
            | "night"
            | "solarized-dark"
            | "nord"
            | "catppuccin-mocha"
    )
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{
        initialize_catalog_files, rewrite_legacy_custom_selectors,
        should_migrate_legacy_preferences, CATALOG_VERSION,
    };
    use crate::themes::catalog::ThemeCatalog;

    #[test]
    fn fresh_catalog_installs_original_css_and_both_drake_packages() {
        let temp = tempdir().unwrap();
        let catalog = ThemeCatalog::at(temp.path().join("themes"));

        assert!(initialize_catalog_files(&catalog, 0).unwrap().is_empty());
        let snapshot = catalog.scan().unwrap();

        assert_eq!(CATALOG_VERSION, 2);
        assert_eq!(snapshot.themes.len(), 20);
        assert!(snapshot
            .themes
            .iter()
            .any(|theme| theme.id == "drake-light"));
        assert!(snapshot.themes.iter().any(|theme| theme.id == "drake-ayu"));
        assert!(temp.path().join("themes/drake-light").is_dir());
        assert!(temp.path().join("themes/drake-ayu").is_dir());
    }

    #[test]
    fn version_one_seeds_only_missing_drake_ids_and_version_two_is_idempotent() {
        let temp = tempdir().unwrap();
        let catalog = ThemeCatalog::at(temp.path().join("themes"));
        catalog.seed_missing().unwrap();
        let nord = catalog
            .scan()
            .unwrap()
            .themes
            .into_iter()
            .find(|theme| theme.id == "nord")
            .unwrap();
        catalog.delete("nord", &nord.fingerprint).unwrap();
        catalog
            .import_bytes(
                b"/*\n@qingyu-theme\nid: drake-light\nname: User Drake\nappearance: light\npreview-background: #fff\npreview-panel: #eee\npreview-text: #222\npreview-accent: #f45\n*/\n:root { --user-owned: true; }\n",
                "user-drake.css",
            )
            .unwrap();

        assert!(initialize_catalog_files(&catalog, 1).unwrap().is_empty());
        let after_v1 = catalog.scan().unwrap();

        assert!(!should_migrate_legacy_preferences(1));
        assert!(!after_v1.themes.iter().any(|theme| theme.id == "nord"));
        assert!(after_v1
            .themes
            .iter()
            .any(|theme| { theme.id == "drake-light" && theme.file_name == "drake-light.css" }));
        assert!(after_v1.themes.iter().any(|theme| theme.id == "drake-ayu"));
        assert!(!temp.path().join("themes/drake-light").exists());

        assert!(initialize_catalog_files(&catalog, 2).unwrap().is_empty());
        assert_eq!(catalog.scan().unwrap(), after_v1);
    }

    #[test]
    fn occupied_drake_destination_is_diagnostic_and_does_not_block_other_seed() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("themes");
        fs::create_dir_all(root.join("drake-light")).unwrap();
        fs::write(
            root.join("drake-light/manifest.json"),
            br##"{"schemaVersion":1,"id":"author-light","name":"Author Light","appearance":"light","entry":"theme.css","preview":{"background":"#fff","panel":"#eee","text":"#222","accent":"#f45"}}"##,
        )
        .unwrap();
        fs::write(
            root.join("drake-light/theme.css"),
            b":root { --author: true; }\n",
        )
        .unwrap();
        let catalog = ThemeCatalog::at(root.clone());

        let diagnostics = initialize_catalog_files(&catalog, 1).unwrap();
        let snapshot = catalog.scan().unwrap();

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].file_name, "drake-light");
        assert!(diagnostics[0].reason.contains("occupied"));
        assert!(snapshot
            .themes
            .iter()
            .any(|theme| theme.id == "author-light"));
        assert!(snapshot.themes.iter().any(|theme| theme.id == "drake-ayu"));
        assert!(!snapshot
            .themes
            .iter()
            .any(|theme| theme.id == "drake-light"));

        let repeated_diagnostics = initialize_catalog_files(&catalog, 2).unwrap();
        assert_eq!(repeated_diagnostics.len(), 1);
        assert_eq!(repeated_diagnostics[0].file_name, "drake-light");
    }

    #[test]
    fn migration_rewrites_only_legacy_theme_selector_values() {
        let css = ":root[data-theme=\"custom\"] .markdown-paper[data-editor-theme='custom'] { --name: custom; }";
        assert_eq!(
            rewrite_legacy_custom_selectors(css, "migrated-custom-light"),
            ":root[data-theme=\"migrated-custom-light\"] .markdown-paper[data-editor-theme='migrated-custom-light'] { --name: custom; }"
        );
    }
}
