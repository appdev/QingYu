use std::path::PathBuf;

use tauri::{Manager, Runtime};

use super::{
    catalog::ThemeCatalog, InvalidThemeFile, ThemeCatalogSnapshot, ThemeError, ThemeErrorCode,
};

const CATALOG_VERSION: i64 = 2;

pub(crate) fn initialize_catalog<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<ThemeCatalogSnapshot, ThemeError> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|error| ThemeError::new(ThemeErrorCode::Io, error.to_string()))?
        .join("themes");
    let catalog = ThemeCatalog::at(root);
    initialize_catalog_without_legacy_settings(&catalog)
}

fn initialize_catalog_without_legacy_settings(
    catalog: &ThemeCatalog,
) -> Result<ThemeCatalogSnapshot, ThemeError> {
    let (catalog, fresh) = catalog.anchored_for_initialization()?;
    if fresh {
        catalog.persist_owned_catalog_version(0)?;
    }
    let owned_version = catalog.owned_catalog_version()?;
    let seed_diagnostics = match owned_version {
        Some(version) => initialize_catalog_files(&catalog, version)?,
        None => catalog.drake_seed_diagnostics()?,
    };
    if owned_version.is_none_or(|version| version < CATALOG_VERSION) {
        catalog.persist_owned_catalog_version(CATALOG_VERSION)?;
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

fn scan_with_diagnostics(
    catalog: &ThemeCatalog,
    seed_diagnostics: Vec<InvalidThemeFile>,
) -> Result<ThemeCatalogSnapshot, ThemeError> {
    catalog
        .scan()
        .map(|snapshot| merge_diagnostics(snapshot, seed_diagnostics))
}

fn merge_diagnostics(
    mut snapshot: ThemeCatalogSnapshot,
    seed_diagnostics: Vec<InvalidThemeFile>,
) -> ThemeCatalogSnapshot {
    for diagnostic in seed_diagnostics {
        snapshot
            .invalid_files
            .retain(|current| current.file_name != diagnostic.file_name);
        snapshot.invalid_files.push(diagnostic);
    }
    snapshot
        .invalid_files
        .sort_by(|left, right| left.file_name.cmp(&right.file_name));
    snapshot
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{
        initialize_catalog_files, initialize_catalog_without_legacy_settings, CATALOG_VERSION,
    };
    use crate::themes::{
        catalog::{ThemeCatalog, CATALOG_VERSION_MARKER_NAME},
        ThemeErrorCode,
    };

    #[test]
    fn fresh_catalog_installs_original_css_and_drake_without_frontend_builtins() {
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
        for id in ["light", "dark", "classic-light", "classic-dark"] {
            assert!(!temp.path().join("themes").join(id).exists(), "id {id}");
        }
    }

    #[test]
    fn mobile_catalog_initialization_seeds_and_scans_without_a_settings_owner() {
        let temp = tempdir().unwrap();
        let catalog = ThemeCatalog::at(temp.path().join("themes"));

        let snapshot = initialize_catalog_without_legacy_settings(&catalog).unwrap();

        assert_eq!(snapshot.themes.len(), 20);
        assert!(snapshot.themes.iter().any(|theme| theme.id == "nord"));
        assert!(snapshot
            .themes
            .iter()
            .any(|theme| theme.id == "drake-light"));
        assert!(snapshot.invalid_files.is_empty());
    }

    #[test]
    fn catalog_owned_initialization_does_not_restore_a_deleted_seed_theme() {
        let temp = tempdir().unwrap();
        let catalog = ThemeCatalog::at(temp.path().join("themes"));
        let initial = initialize_catalog_without_legacy_settings(&catalog).unwrap();
        let nord = initial
            .themes
            .into_iter()
            .find(|theme| theme.id == "nord")
            .unwrap();
        catalog.delete("nord", &nord.fingerprint).unwrap();

        let restarted = initialize_catalog_without_legacy_settings(&catalog).unwrap();

        assert!(!restarted.themes.iter().any(|theme| theme.id == "nord"));
        assert_eq!(restarted.themes.len(), 19);
    }

    #[test]
    fn catalog_owned_initialization_adopts_an_existing_catalog_without_reseeding() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("themes");
        let catalog = ThemeCatalog::at(root.clone());
        catalog
            .import_bytes(
                b"/*\n@qingyu-theme\nid: user-light\nname: User Light\nappearance: light\npreview-background: #fff\npreview-panel: #eee\npreview-text: #222\npreview-accent: #f45\n*/\n:root { --user-owned: true; }\n",
                "user-light.css",
            )
            .unwrap();

        let snapshot = initialize_catalog_without_legacy_settings(&catalog).unwrap();

        assert_eq!(snapshot.themes.len(), 1);
        assert_eq!(snapshot.themes[0].id, "user-light");
        assert!(snapshot.invalid_files.is_empty());
        assert_eq!(
            fs::read_to_string(root.join(CATALOG_VERSION_MARKER_NAME)).unwrap(),
            format!("{CATALOG_VERSION}\n")
        );
    }

    #[test]
    fn catalog_owned_initialization_adopts_a_legacy_empty_directory_without_reseeding() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("themes");
        fs::create_dir_all(&root).unwrap();
        let catalog = ThemeCatalog::at(root.clone());

        let snapshot = initialize_catalog_without_legacy_settings(&catalog).unwrap();

        assert!(snapshot.themes.is_empty());
        assert!(snapshot.invalid_files.is_empty());
        assert_eq!(
            fs::read_to_string(root.join(CATALOG_VERSION_MARKER_NAME)).unwrap(),
            format!("{CATALOG_VERSION}\n")
        );
    }

    #[test]
    fn catalog_owned_initialization_retries_an_interrupted_version_zero_seed() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("themes");
        let catalog = ThemeCatalog::at(root.clone());
        let (anchored, fresh) = catalog.anchored_for_initialization().unwrap();
        assert!(fresh);
        anchored.persist_owned_catalog_version(0).unwrap();
        anchored.seed_missing().unwrap();
        let nord = anchored.find_descriptor("nord").unwrap();
        anchored.delete("nord", &nord.fingerprint).unwrap();

        let snapshot = initialize_catalog_without_legacy_settings(&catalog).unwrap();

        assert_eq!(snapshot.themes.len(), 20);
        assert!(snapshot.themes.iter().any(|theme| theme.id == "nord"));
        assert!(snapshot
            .themes
            .iter()
            .any(|theme| theme.id == "drake-light"));
        assert_eq!(
            fs::read_to_string(root.join(CATALOG_VERSION_MARKER_NAME)).unwrap(),
            format!("{CATALOG_VERSION}\n")
        );
    }

    #[test]
    fn future_catalog_owned_version_is_preserved_without_reseeding() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("themes");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join(CATALOG_VERSION_MARKER_NAME), b"99\n").unwrap();
        let catalog = ThemeCatalog::at(root.clone());

        let snapshot = initialize_catalog_without_legacy_settings(&catalog).unwrap();

        assert!(snapshot.themes.is_empty());
        assert_eq!(
            fs::read_to_string(root.join(CATALOG_VERSION_MARKER_NAME)).unwrap(),
            "99\n"
        );
    }

    #[test]
    fn malformed_catalog_owned_version_fails_closed_without_seeding() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("themes");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join(CATALOG_VERSION_MARKER_NAME), b"not-a-version\n").unwrap();
        let catalog = ThemeCatalog::at(root.clone());

        let error = initialize_catalog_without_legacy_settings(&catalog).unwrap_err();

        assert_eq!(error.code, ThemeErrorCode::InvalidMetadata);
        assert!(!root.join("nord.css").exists());
    }

    #[cfg(unix)]
    #[test]
    fn linked_catalog_owned_version_fails_closed_without_seeding() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let root = temp.path().join("themes");
        fs::create_dir_all(&root).unwrap();
        let outside = temp.path().join("outside-version");
        fs::write(&outside, format!("{CATALOG_VERSION}\n")).unwrap();
        symlink(&outside, root.join(CATALOG_VERSION_MARKER_NAME)).unwrap();
        let catalog = ThemeCatalog::at(root.clone());

        let error = initialize_catalog_without_legacy_settings(&catalog).unwrap_err();

        assert_eq!(error.code, ThemeErrorCode::UnsafePath);
        assert!(!root.join("nord.css").exists());
    }

    #[cfg(unix)]
    #[test]
    fn hardlinked_catalog_owned_version_fails_closed_without_seeding() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("themes");
        fs::create_dir_all(&root).unwrap();
        let outside = temp.path().join("outside-version");
        fs::write(&outside, format!("{CATALOG_VERSION}\n")).unwrap();
        fs::hard_link(&outside, root.join(CATALOG_VERSION_MARKER_NAME)).unwrap();
        let catalog = ThemeCatalog::at(root.clone());

        let error = initialize_catalog_without_legacy_settings(&catalog).unwrap_err();

        assert_eq!(error.code, ThemeErrorCode::UnsafePath);
        assert!(!root.join("nord.css").exists());
    }

    #[test]
    fn version_one_adds_drake_without_restoring_deleted_css_or_overwriting_an_occupied_id() {
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
    fn current_catalog_version_does_not_reseed_a_deleted_drake_theme() {
        let temp = tempdir().unwrap();
        let catalog = ThemeCatalog::at(temp.path().join("themes"));
        assert!(initialize_catalog_files(&catalog, 0).unwrap().is_empty());
        let drake_ayu = catalog.find_descriptor("drake-ayu").unwrap();
        catalog.delete("drake-ayu", &drake_ayu.fingerprint).unwrap();

        assert!(initialize_catalog_files(&catalog, CATALOG_VERSION)
            .unwrap()
            .is_empty());
        let snapshot = catalog.scan().unwrap();

        assert!(!snapshot.themes.iter().any(|theme| theme.id == "drake-ayu"));
        assert_eq!(snapshot.themes.len(), 19);
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
}
