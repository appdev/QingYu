pub(crate) mod activation;
mod activation_cleanup;
mod archive;
mod catalog;
mod manifest;
mod migration;
mod parser;
mod resources;

use serde::{Deserialize, Serialize};
use std::path::Path;

pub(crate) use activation::{release_theme_activation_for_window, ThemeActivationState};
use catalog::ThemeCatalog;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ThemeAppearance {
    Light,
    Dark,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ThemePreview {
    pub(crate) accent: String,
    pub(crate) background: String,
    pub(crate) panel: String,
    pub(crate) text: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ThemeStorageKind {
    InlineCss,
    ResourceDirectory,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ThemeDescriptor {
    pub(crate) appearance: ThemeAppearance,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) author: Option<String>,
    pub(crate) file_name: String,
    pub(crate) fingerprint: String,
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) preview: ThemePreview,
    pub(crate) source: String,
    pub(crate) storage_kind: ThemeStorageKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ParsedTheme {
    pub(crate) bytes: Vec<u8>,
    pub(crate) descriptor: ThemeDescriptor,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InvalidThemeFile {
    pub(crate) file_name: String,
    pub(crate) reason: String,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ThemeCatalogSnapshot {
    pub(crate) invalid_files: Vec<InvalidThemeFile>,
    pub(crate) themes: Vec<ThemeDescriptor>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ThemeCssPayload {
    pub(crate) css: String,
    pub(crate) fingerprint: String,
    pub(crate) id: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ThemeErrorCode {
    DuplicateTheme,
    FingerprintMismatch,
    InvalidArchive,
    InvalidCss,
    InvalidManifest,
    InvalidMetadata,
    InvalidUtf8,
    Io,
    ProtectedTheme,
    ThemeNotFound,
    ThemeTooLarge,
    UnsafePath,
    UnsafeResource,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ThemeError {
    pub(crate) code: ThemeErrorCode,
    pub(crate) message: String,
}

impl ThemeError {
    pub(crate) fn new(code: ThemeErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ThemeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

impl std::error::Error for ThemeError {}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum ThemeImportResult {
    Imported {
        theme: ThemeDescriptor,
    },
    Conflict {
        candidate: ThemeDescriptor,
        existing: ThemeDescriptor,
        source_path: String,
    },
}

fn prepared_catalog(app: &tauri::AppHandle) -> Result<ThemeCatalog, ThemeError> {
    migration::initialize_catalog(app)?;
    Ok(ThemeCatalog::at(migration::theme_directory(app)?))
}

fn read_external_theme(
    catalog: &ThemeCatalog,
    path: &str,
) -> Result<archive::PreparedThemeImport, ThemeError> {
    catalog.prepare_external(Path::new(path))
}

fn import_external_theme(
    catalog: &ThemeCatalog,
    source_path: String,
) -> Result<ThemeImportResult, ThemeError> {
    let prepared = read_external_theme(catalog, &source_path)?;
    let candidate = prepared.descriptor().clone();
    if let Some(existing) = catalog.existing_descriptor(&candidate.id)? {
        return Ok(ThemeImportResult::Conflict {
            candidate,
            existing,
            source_path,
        });
    }
    let theme = catalog.import_prepared(prepared)?;
    Ok(ThemeImportResult::Imported { theme })
}

fn replace_external_theme(
    catalog: &ThemeCatalog,
    source_path: String,
    expected_fingerprint: &str,
) -> Result<ThemeDescriptor, ThemeError> {
    let prepared = read_external_theme(catalog, &source_path)?;
    catalog.replace_prepared(prepared, expected_fingerprint)
}

#[tauri::command]
pub(crate) fn list_themes(
    app: tauri::AppHandle,
    state: tauri::State<'_, ThemeActivationState>,
) -> Result<ThemeCatalogSnapshot, ThemeError> {
    let snapshot = migration::initialize_catalog(&app)?;
    let catalog = prepared_catalog(&app)?;
    state.remember_catalog_snapshot(&catalog, &snapshot)?;
    Ok(snapshot)
}

#[tauri::command]
pub(crate) fn read_theme_css(
    app: tauri::AppHandle,
    id: String,
    expected_fingerprint: String,
) -> Result<ThemeCssPayload, ThemeError> {
    prepared_catalog(&app)?.read_css(&id, &expected_fingerprint)
}

#[tauri::command]
pub(crate) fn import_theme_file(
    app: tauri::AppHandle,
    source_path: String,
) -> Result<ThemeImportResult, ThemeError> {
    let catalog = prepared_catalog(&app)?;
    import_external_theme(&catalog, source_path)
}

#[tauri::command]
pub(crate) fn replace_theme_file(
    app: tauri::AppHandle,
    source_path: String,
    expected_fingerprint: String,
) -> Result<ThemeDescriptor, ThemeError> {
    let catalog = prepared_catalog(&app)?;
    replace_external_theme(&catalog, source_path, &expected_fingerprint)
}

#[tauri::command]
pub(crate) fn delete_theme(
    app: tauri::AppHandle,
    state: tauri::State<'_, ThemeActivationState>,
    id: String,
    expected_fingerprint: String,
) -> Result<(), ThemeError> {
    let catalog = prepared_catalog(&app)?;
    activation::delete_theme_for_app(&app, &state, &catalog, &id, &expected_fingerprint)
}

#[tauri::command]
pub(crate) fn theme_directory_path(app: tauri::AppHandle) -> Result<String, ThemeError> {
    let catalog = prepared_catalog(&app)?;
    catalog.scan()?;
    migration::theme_directory(&app).map(|path| path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Write, path::Path};

    use tempfile::tempdir;
    use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

    use super::{
        catalog::ThemeCatalog, import_external_theme, parser::parse_theme_file, ThemeImportResult,
        ThemeStorageKind,
    };

    fn css(id: &str, suffix: &str) -> Vec<u8> {
        format!(
            "/*\n@qingyu-theme\nid: {id}\nname: {id}\nappearance: light\npreview-background: #ffffff\npreview-panel: #f6f8fa\npreview-text: #1f2328\npreview-accent: #0969da\n*/\n:root {{ --suffix: {suffix}; }}\n"
        )
        .into_bytes()
    }

    fn write_package(root: &Path, id: &str, suffix: &str) {
        fs::create_dir_all(root).unwrap();
        fs::write(
            root.join("manifest.json"),
            serde_json::to_vec(&serde_json::json!({
                "schemaVersion": 1,
                "id": id,
                "name": id,
                "appearance": "dark",
                "entry": "theme.css",
                "preview": {
                    "background": "#ffffff",
                    "panel": "#f6f8fa",
                    "text": "#1f2328",
                    "accent": "#0969da"
                }
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            root.join("theme.css"),
            format!(":root {{ --suffix: {suffix}; }}\n"),
        )
        .unwrap();
    }

    fn package_archive(root: &Path, _catalog_root: &Path, id: &str) -> std::path::PathBuf {
        let source = root.join(format!("{id}-source"));
        write_package(&source, id, "resource");
        let archive = root.join(format!("{id}.theme"));
        let output = fs::File::create(&archive).unwrap();
        let mut writer = ZipWriter::new(output);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for name in ["manifest.json", "theme.css"] {
            writer.start_file(name, options).unwrap();
            writer
                .write_all(&fs::read(source.join(name)).unwrap())
                .unwrap();
        }
        writer.finish().unwrap();
        archive
    }

    #[test]
    fn command_import_conflict_returns_resource_candidate_and_legacy_existing() {
        let temp = tempdir().unwrap();
        let catalog_root = temp.path().join("themes");
        let catalog = ThemeCatalog::at(catalog_root.clone());
        let existing = catalog
            .import_bytes(&css("conflict", "legacy"), "legacy.css")
            .unwrap();
        let archive = package_archive(temp.path(), &catalog_root, "conflict");
        let source_path = archive.to_string_lossy().into_owned();

        let result = import_external_theme(&catalog, source_path.clone()).unwrap();

        let ThemeImportResult::Conflict {
            candidate,
            existing: actual_existing,
            source_path: actual_source_path,
        } = result
        else {
            panic!("expected conflict");
        };
        assert_eq!(candidate.storage_kind, ThemeStorageKind::ResourceDirectory);
        assert_eq!(actual_existing, existing);
        assert_eq!(actual_source_path, source_path);
        assert!(catalog_root.join("conflict.css").is_file());
    }

    #[test]
    fn command_import_conflict_returns_legacy_candidate_and_resource_existing() {
        let temp = tempdir().unwrap();
        let catalog_root = temp.path().join("themes");
        fs::create_dir(&catalog_root).unwrap();
        let archive = package_archive(temp.path(), &catalog_root, "conflict");
        let catalog = ThemeCatalog::at(catalog_root.clone());
        let existing = match import_external_theme(&catalog, archive.to_string_lossy().into_owned())
            .unwrap()
        {
            ThemeImportResult::Imported { theme } => theme,
            ThemeImportResult::Conflict { .. } => panic!("unexpected conflict"),
        };
        let css_path = temp.path().join("conflict.css");
        fs::write(&css_path, css("conflict", "legacy")).unwrap();
        let source_path = css_path.to_string_lossy().into_owned();

        let result = import_external_theme(&catalog, source_path.clone()).unwrap();

        let ThemeImportResult::Conflict {
            candidate,
            existing: actual_existing,
            source_path: actual_source_path,
        } = result
        else {
            panic!("expected conflict");
        };
        assert_eq!(candidate.storage_kind, ThemeStorageKind::InlineCss);
        assert_eq!(actual_existing, existing);
        assert_eq!(actual_source_path, source_path);
        assert!(catalog_root.join("conflict").is_dir());
    }

    #[test]
    fn command_replace_reopens_and_revalidates_the_source_path() {
        let temp = tempdir().unwrap();
        let catalog_root = temp.path().join("themes");
        let catalog = ThemeCatalog::at(catalog_root.clone());
        let original = catalog
            .import_bytes(&css("replace", "one"), "one.css")
            .unwrap();
        let source = temp.path().join("replace.css");
        fs::write(&source, css("replace", "two")).unwrap();
        let initially_read = parse_theme_file(&fs::read(&source).unwrap(), "replace.css").unwrap();
        fs::write(&source, css("changed-id", "three")).unwrap();

        let error = super::replace_external_theme(
            &catalog,
            source.to_string_lossy().into_owned(),
            &original.fingerprint,
        )
        .unwrap_err();

        assert_ne!(initially_read.descriptor.id, "changed-id");
        assert_eq!(error.code, super::ThemeErrorCode::ThemeNotFound);
        assert_eq!(catalog.find_descriptor("replace").unwrap(), original);
    }
}
