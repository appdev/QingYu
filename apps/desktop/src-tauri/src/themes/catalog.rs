use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use cap_fs_ext::{DirExt, FollowSymlinks, MetadataExt, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions as CapOpenOptions};

use super::{
    archive::{prepare_external_theme, PreparedThemeImport},
    parser::parse_theme_file,
    resources::{
        validate_theme_directory, validate_theme_directory_from_retained, ValidatedThemeDirectory,
    },
    InvalidThemeFile, ParsedTheme, ThemeCatalogSnapshot, ThemeCssPayload, ThemeDescriptor,
    ThemeError, ThemeErrorCode, ThemeStorageKind,
};

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(crate) const ACTIVATION_LEASE_PARENT_NAME: &str = ".qingyu-theme-activation-leases";

const SEED_THEMES: [(&str, &[u8]); 18] = [
    (
        "github.css",
        include_bytes!("../../themes/third-party/github.css"),
    ),
    (
        "github-dark.css",
        include_bytes!("../../themes/third-party/github-dark.css"),
    ),
    (
        "one-dark.css",
        include_bytes!("../../themes/third-party/one-dark.css"),
    ),
    (
        "one-light.css",
        include_bytes!("../../themes/third-party/one-light.css"),
    ),
    (
        "one-dark-pro.css",
        include_bytes!("../../themes/third-party/one-dark-pro.css"),
    ),
    (
        "gothic.css",
        include_bytes!("../../themes/third-party/gothic.css"),
    ),
    (
        "newsprint.css",
        include_bytes!("../../themes/third-party/newsprint.css"),
    ),
    (
        "night.css",
        include_bytes!("../../themes/third-party/night.css"),
    ),
    (
        "pixyll.css",
        include_bytes!("../../themes/third-party/pixyll.css"),
    ),
    (
        "whitey.css",
        include_bytes!("../../themes/third-party/whitey.css"),
    ),
    (
        "sepia.css",
        include_bytes!("../../themes/third-party/sepia.css"),
    ),
    (
        "solarized-light.css",
        include_bytes!("../../themes/third-party/solarized-light.css"),
    ),
    (
        "solarized-dark.css",
        include_bytes!("../../themes/third-party/solarized-dark.css"),
    ),
    (
        "nord.css",
        include_bytes!("../../themes/third-party/nord.css"),
    ),
    (
        "catppuccin-latte.css",
        include_bytes!("../../themes/third-party/catppuccin-latte.css"),
    ),
    (
        "catppuccin-mocha.css",
        include_bytes!("../../themes/third-party/catppuccin-mocha.css"),
    ),
    (
        "academic.css",
        include_bytes!("../../themes/third-party/academic.css"),
    ),
    (
        "minimal.css",
        include_bytes!("../../themes/third-party/minimal.css"),
    ),
];

#[derive(Clone, Copy)]
struct EmbeddedThemePackage {
    id: &'static str,
    storage_name: &'static str,
    files: &'static [(&'static str, &'static [u8])],
}

const DRAKE_LIGHT_FILES: [(&str, &[u8]); 8] = [
    (
        "manifest.json",
        include_bytes!("../../themes/third-party/drake-light/manifest.json"),
    ),
    (
        "theme.css",
        include_bytes!("../../themes/third-party/drake-light/theme.css"),
    ),
    (
        "assets/fonts/JetBrainsMono-Bold.woff2",
        include_bytes!(
            "../../themes/third-party/drake-light/assets/fonts/JetBrainsMono-Bold.woff2"
        ),
    ),
    (
        "assets/fonts/JetBrainsMono-BoldItalic.woff2",
        include_bytes!(
            "../../themes/third-party/drake-light/assets/fonts/JetBrainsMono-BoldItalic.woff2"
        ),
    ),
    (
        "assets/fonts/JetBrainsMono-Italic.woff2",
        include_bytes!(
            "../../themes/third-party/drake-light/assets/fonts/JetBrainsMono-Italic.woff2"
        ),
    ),
    (
        "assets/fonts/JetBrainsMono-Regular.woff2",
        include_bytes!(
            "../../themes/third-party/drake-light/assets/fonts/JetBrainsMono-Regular.woff2"
        ),
    ),
    (
        "licenses/FONT-LICENSE.txt",
        include_bytes!("../../themes/third-party/drake-light/licenses/FONT-LICENSE.txt"),
    ),
    (
        "licenses/THEME-LICENSE.txt",
        include_bytes!("../../themes/third-party/drake-light/licenses/THEME-LICENSE.txt"),
    ),
];

const DRAKE_AYU_FILES: [(&str, &[u8]); 8] = [
    (
        "manifest.json",
        include_bytes!("../../themes/third-party/drake-ayu/manifest.json"),
    ),
    (
        "theme.css",
        include_bytes!("../../themes/third-party/drake-ayu/theme.css"),
    ),
    (
        "assets/fonts/JetBrainsMono-Bold.woff2",
        include_bytes!("../../themes/third-party/drake-ayu/assets/fonts/JetBrainsMono-Bold.woff2"),
    ),
    (
        "assets/fonts/JetBrainsMono-BoldItalic.woff2",
        include_bytes!(
            "../../themes/third-party/drake-ayu/assets/fonts/JetBrainsMono-BoldItalic.woff2"
        ),
    ),
    (
        "assets/fonts/JetBrainsMono-Italic.woff2",
        include_bytes!(
            "../../themes/third-party/drake-ayu/assets/fonts/JetBrainsMono-Italic.woff2"
        ),
    ),
    (
        "assets/fonts/JetBrainsMono-Regular.woff2",
        include_bytes!(
            "../../themes/third-party/drake-ayu/assets/fonts/JetBrainsMono-Regular.woff2"
        ),
    ),
    (
        "licenses/FONT-LICENSE.txt",
        include_bytes!("../../themes/third-party/drake-ayu/licenses/FONT-LICENSE.txt"),
    ),
    (
        "licenses/THEME-LICENSE.txt",
        include_bytes!("../../themes/third-party/drake-ayu/licenses/THEME-LICENSE.txt"),
    ),
];

const DRAKE_THEME_PACKAGES: &[EmbeddedThemePackage] = &[
    EmbeddedThemePackage {
        id: "drake-light",
        storage_name: "drake-light",
        files: &DRAKE_LIGHT_FILES,
    },
    EmbeddedThemePackage {
        id: "drake-ayu",
        storage_name: "drake-ayu",
        files: &DRAKE_AYU_FILES,
    },
];

pub(crate) struct ThemeCatalog {
    root: PathBuf,
}

pub(crate) enum CatalogActivationSource {
    InlineCss(String),
    ResourceDirectory(ValidatedThemeDirectory),
}

pub(crate) struct CatalogActivation {
    pub(crate) descriptor: ThemeDescriptor,
    pub(crate) source: CatalogActivationSource,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CatalogPublicationHookPoint {
    BeforePublication,
    AfterBackup,
    AfterPublication,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DeleteHookPoint {
    AfterValidation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CatalogRenameHookPoint {
    AfterRootValidation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EmbeddedSeedHookPoint {
    BeforeStagingCreate,
    AfterStagingCreate,
    AfterStagingWrite,
    BeforePublication,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CatalogFileIdentity {
    device: u64,
    inode: u64,
}

struct CatalogDirectory {
    directory: Dir,
    root: PathBuf,
    identity: CatalogFileIdentity,
}

struct EmbeddedStagingCleanup {
    parent: Dir,
    name: PathBuf,
    armed: bool,
}

impl EmbeddedStagingCleanup {
    fn new(parent: Dir, name: PathBuf) -> Self {
        Self {
            parent,
            name,
            armed: true,
        }
    }

    #[cfg(test)]
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for EmbeddedStagingCleanup {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let Ok(metadata) = self.parent.symlink_metadata(&self.name) else {
            return;
        };
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            let _cleanup = self.parent.remove_dir_all(&self.name);
        } else {
            let _cleanup = self.parent.remove_file(&self.name);
        }
    }
}

struct MaterializedEmbeddedTheme {
    validated: Option<ValidatedThemeDirectory>,
    _cleanup: EmbeddedStagingCleanup,
}

impl std::ops::Deref for CatalogDirectory {
    type Target = Dir;

    fn deref(&self) -> &Self::Target {
        &self.directory
    }
}

enum InstalledTheme {
    Legacy(ParsedTheme),
    Resource(ValidatedThemeDirectory),
}

impl InstalledTheme {
    fn descriptor(&self) -> &ThemeDescriptor {
        match self {
            Self::Legacy(theme) => &theme.descriptor,
            Self::Resource(theme) => &theme.descriptor,
        }
    }
}

impl ThemeCatalog {
    pub(crate) fn at(root: PathBuf) -> Self {
        Self { root }
    }

    pub(crate) fn scan(&self) -> Result<ThemeCatalogSnapshot, ThemeError> {
        self.ensure_root()?;
        let mut parsed = Vec::new();
        let mut invalid_files = Vec::new();
        let entries = fs::read_dir(&self.root).map_err(io_error)?;

        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    invalid_files.push(InvalidThemeFile {
                        file_name: "unknown".to_string(),
                        reason: format!("Could not inspect directory entry: {error}"),
                    });
                    continue;
                }
            };
            let file_name = entry.file_name().to_string_lossy().into_owned();
            if is_owned_catalog_entry(&file_name) || file_name == ACTIVATION_LEASE_PARENT_NAME {
                continue;
            }
            let metadata = match fs::symlink_metadata(entry.path()) {
                Ok(metadata) => metadata,
                Err(error) => {
                    invalid_files.push(InvalidThemeFile {
                        file_name,
                        reason: format!("Could not inspect theme file: {error}"),
                    });
                    continue;
                }
            };
            if metadata.file_type().is_symlink() {
                invalid_files.push(InvalidThemeFile {
                    file_name,
                    reason: "Theme entries cannot be symbolic links.".to_string(),
                });
                continue;
            }
            let parsed_theme = if metadata.file_type().is_file()
                && Path::new(&file_name).extension().and_then(OsStr::to_str) == Some("css")
            {
                match prepare_external_theme(&entry.path(), &self.root) {
                    Ok(PreparedThemeImport::LegacyCss(theme)) => Ok(theme.descriptor),
                    Ok(PreparedThemeImport::ResourcePackage(_)) => Err(ThemeError::new(
                        ThemeErrorCode::UnsafePath,
                        "Root CSS entries must remain legacy CSS themes.",
                    )),
                    Err(error) => Err(error),
                }
            } else if metadata.file_type().is_dir() {
                validate_theme_directory(&entry.path(), &file_name).map(|theme| theme.descriptor)
            } else {
                Err(ThemeError::new(
                    ThemeErrorCode::UnsafePath,
                    "Theme entries must be root CSS files or resource-theme directories.",
                ))
            };
            match parsed_theme {
                Ok(theme) => parsed.push(theme),
                Err(error) => invalid_files.push(InvalidThemeFile {
                    file_name,
                    reason: error.message,
                }),
            }
        }

        let mut by_id: BTreeMap<String, Vec<ThemeDescriptor>> = BTreeMap::new();
        for theme in parsed {
            by_id.entry(theme.id.clone()).or_default().push(theme);
        }

        let mut themes = Vec::new();
        for (id, mut matches) in by_id {
            if matches.len() > 1 {
                for theme in matches {
                    invalid_files.push(InvalidThemeFile {
                        file_name: theme.file_name,
                        reason: format!("Multiple theme entries declare the ID {id}."),
                    });
                }
                continue;
            }
            themes.push(matches.remove(0));
        }
        themes.sort_by(|left, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then_with(|| left.id.cmp(&right.id))
        });
        invalid_files.sort_by(|left, right| left.file_name.cmp(&right.file_name));

        Ok(ThemeCatalogSnapshot {
            invalid_files,
            themes,
        })
    }

    pub(crate) fn seed_missing(&self) -> Result<(), ThemeError> {
        self.ensure_root()?;
        for (file_name, bytes) in SEED_THEMES {
            let parsed = parse_theme_file(bytes, file_name)?;
            let target = self.safe_theme_path(file_name)?;
            if target.exists() {
                let current = fs::read(&target).map_err(io_error)?;
                let current = parse_theme_file(&current, file_name)?;
                if current.descriptor.id != parsed.descriptor.id {
                    return Err(ThemeError::new(
                        ThemeErrorCode::DuplicateTheme,
                        format!("The seed target {file_name} is occupied by another theme."),
                    ));
                }
                continue;
            }
            if self
                .scan()?
                .themes
                .iter()
                .any(|theme| theme.id == parsed.descriptor.id)
            {
                continue;
            }
            self.atomic_write(&target, bytes)?;
        }
        Ok(())
    }

    pub(crate) fn seed_missing_drake(&self) -> Result<Vec<InvalidThemeFile>, ThemeError> {
        self.seed_missing_drake_with_hook(&mut |_, _, _| Ok(()))
    }

    fn seed_missing_drake_with_hook(
        &self,
        hook: &mut dyn FnMut(
            EmbeddedSeedHookPoint,
            &EmbeddedThemePackage,
            &Path,
        ) -> Result<(), ThemeError>,
    ) -> Result<Vec<InvalidThemeFile>, ThemeError> {
        self.ensure_root()?;
        let mut diagnostics = Vec::new();
        for package in DRAKE_THEME_PACKAGES {
            if self.existing_descriptor(package.id)?.is_some() {
                continue;
            }

            let target =
                self.safe_storage_path(package.storage_name, ThemeStorageKind::ResourceDirectory)?;
            match fs::symlink_metadata(&target) {
                Ok(_) => {
                    diagnostics.push(occupied_embedded_target(package));
                    continue;
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(io_error(error)),
            }

            let mut materialized = None;
            for _attempt in 0..1024 {
                let staging_name = unique_owned_name("dir");
                let staging = self.root.join(&staging_name);
                if let Some(candidate) = try_materialize_embedded_theme(&staging, package, hook)? {
                    materialized = Some((staging_name, candidate));
                    break;
                }
            }
            let (staging_name, materialized) = materialized.ok_or_else(|| {
                ThemeError::new(
                    ThemeErrorCode::Io,
                    "Could not reserve a unique embedded theme staging directory.",
                )
            })?;
            let candidate = materialized.validated.as_ref().ok_or_else(|| {
                ThemeError::new(
                    ThemeErrorCode::Io,
                    "Embedded theme staging lost its validation result.",
                )
            })?;
            let publication = (|| {
                if self.existing_descriptor(package.id)?.is_some() {
                    return Ok(false);
                }
                match fs::symlink_metadata(&target) {
                    Ok(_) => {
                        diagnostics.push(occupied_embedded_target(package));
                        return Ok(false);
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(io_error(error)),
                }

                hook(EmbeddedSeedHookPoint::BeforePublication, package, &target)?;
                let directory = self.catalog_directory()?;
                if !rename_embedded_seed_noreplace(&directory, &staging_name, package.storage_name)?
                {
                    diagnostics.push(occupied_embedded_target(package));
                    return Ok(false);
                }
                sync_catalog_directory(&directory);

                let installed =
                    validate_exact_embedded_theme(&target, package.storage_name, package);
                match installed {
                    Ok(installed)
                        if same_content_descriptor(
                            &candidate.descriptor,
                            &installed.descriptor,
                        ) =>
                    {
                        Ok(true)
                    }
                    Ok(_) => {
                        let _cleanup = remove_catalog_entry(
                            &directory,
                            package.storage_name,
                            ThemeStorageKind::ResourceDirectory,
                        );
                        Err(fingerprint_mismatch())
                    }
                    Err(error) => {
                        let _cleanup = remove_catalog_entry(
                            &directory,
                            package.storage_name,
                            ThemeStorageKind::ResourceDirectory,
                        );
                        Err(error)
                    }
                }
            })();
            drop(materialized);
            publication?;
        }
        Ok(diagnostics)
    }

    pub(crate) fn drake_seed_diagnostics(&self) -> Result<Vec<InvalidThemeFile>, ThemeError> {
        self.ensure_root()?;
        let mut diagnostics = Vec::new();
        for package in DRAKE_THEME_PACKAGES {
            if self.existing_descriptor(package.id)?.is_some() {
                continue;
            }
            let target =
                self.safe_storage_path(package.storage_name, ThemeStorageKind::ResourceDirectory)?;
            match fs::symlink_metadata(target) {
                Ok(_) => diagnostics.push(occupied_embedded_target(package)),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(io_error(error)),
            }
        }
        Ok(diagnostics)
    }

    pub(crate) fn read_css(
        &self,
        id: &str,
        expected_fingerprint: &str,
    ) -> Result<ThemeCssPayload, ThemeError> {
        let descriptor = self.find_descriptor(id)?;
        if descriptor.fingerprint != expected_fingerprint {
            return Err(fingerprint_mismatch());
        }
        let InstalledTheme::Legacy(parsed) = self.load_installed(&descriptor)? else {
            return Err(ThemeError::new(
                ThemeErrorCode::UnsafeResource,
                "Resource themes require stylesheet activation.",
            ));
        };
        let css = String::from_utf8(parsed.bytes).map_err(|_| {
            ThemeError::new(ThemeErrorCode::InvalidUtf8, "Theme CSS is not valid UTF-8.")
        })?;

        Ok(ThemeCssPayload {
            css,
            fingerprint: expected_fingerprint.to_string(),
            id: id.to_string(),
        })
    }

    pub(crate) fn prepare_activation_with_hint(
        &self,
        id: &str,
        expected_fingerprint: &str,
        listed_hint: Option<&ThemeDescriptor>,
    ) -> Result<CatalogActivation, ThemeError> {
        self.prepare_activation_with_hint_inner(id, expected_fingerprint, listed_hint, &mut |_| {
            Ok(())
        })
    }

    #[cfg(test)]
    fn prepare_activation_with_hint_and_hook(
        &self,
        id: &str,
        expected_fingerprint: &str,
        listed_hint: Option<&ThemeDescriptor>,
        hook: &mut dyn FnMut(&ThemeDescriptor) -> Result<(), ThemeError>,
    ) -> Result<CatalogActivation, ThemeError> {
        self.prepare_activation_with_hint_inner(id, expected_fingerprint, listed_hint, hook)
    }

    fn prepare_activation_with_hint_inner(
        &self,
        id: &str,
        expected_fingerprint: &str,
        listed_hint: Option<&ThemeDescriptor>,
        hook: &mut dyn FnMut(&ThemeDescriptor) -> Result<(), ThemeError>,
    ) -> Result<CatalogActivation, ThemeError> {
        let descriptor = match self.find_descriptor(id) {
            Ok(descriptor) => descriptor,
            Err(error)
                if error.code == ThemeErrorCode::ThemeNotFound
                    && listed_hint.is_some_and(|hint| {
                        hint.id == id && hint.fingerprint == expected_fingerprint
                    }) =>
            {
                return Err(fingerprint_mismatch())
            }
            Err(error)
                if error.code == ThemeErrorCode::ThemeNotFound
                    && super::manifest::valid_theme_id(id)
                    && (self.root.join(id).exists()
                        || self.root.join(format!("{id}.css")).exists()) =>
            {
                return Err(fingerprint_mismatch())
            }
            Err(error) => return Err(error),
        };
        if descriptor.fingerprint != expected_fingerprint {
            return Err(fingerprint_mismatch());
        }
        hook(&descriptor)?;
        let installed = self.load_installed(&descriptor).map_err(|error| {
            if error.code == ThemeErrorCode::Io
                || error.code == ThemeErrorCode::InvalidCss
                || error.code == ThemeErrorCode::InvalidManifest
                || error.code == ThemeErrorCode::InvalidMetadata
                || error.code == ThemeErrorCode::InvalidUtf8
                || error.code == ThemeErrorCode::ThemeTooLarge
                || error.code == ThemeErrorCode::UnsafePath
                || error.code == ThemeErrorCode::UnsafeResource
            {
                fingerprint_mismatch()
            } else {
                error
            }
        })?;
        let source = match installed {
            InstalledTheme::Legacy(theme) => {
                let css = String::from_utf8(theme.bytes).map_err(|_| fingerprint_mismatch())?;
                CatalogActivationSource::InlineCss(css)
            }
            InstalledTheme::Resource(theme) => CatalogActivationSource::ResourceDirectory(theme),
        };
        Ok(CatalogActivation { descriptor, source })
    }

    pub(crate) fn validated_resource_root_with_hint(
        &self,
        id: &str,
        expected_fingerprint: &str,
        listed_hint: Option<&ThemeDescriptor>,
    ) -> Result<Option<PathBuf>, ThemeError> {
        let prepared = self.prepare_activation_with_hint(id, expected_fingerprint, listed_hint)?;
        match prepared.source {
            CatalogActivationSource::InlineCss(_) => Ok(None),
            CatalogActivationSource::ResourceDirectory(theme) => Ok(Some(theme.root)),
        }
    }

    pub(crate) fn activation_lease_parent_path(&self) -> PathBuf {
        self.root.join(ACTIVATION_LEASE_PARENT_NAME)
    }

    pub(crate) fn root_path(&self) -> &Path {
        &self.root
    }

    #[cfg(test)]
    pub(crate) fn import_bytes(
        &self,
        bytes: &[u8],
        source_file_name: &str,
    ) -> Result<ThemeDescriptor, ThemeError> {
        let parsed = parse_theme_file(bytes, source_file_name)?;
        self.import_prepared(PreparedThemeImport::LegacyCss(parsed))
    }

    pub(crate) fn prepare_external(
        &self,
        source: &Path,
    ) -> Result<PreparedThemeImport, ThemeError> {
        self.ensure_root()?;
        prepare_external_theme(source, &self.root)
    }

    pub(crate) fn import_prepared(
        &self,
        prepared: PreparedThemeImport,
    ) -> Result<ThemeDescriptor, ThemeError> {
        self.import_prepared_with_hook(prepared, &mut |_| Ok(()))
    }

    fn import_prepared_with_hook(
        &self,
        prepared: PreparedThemeImport,
        hook: &mut dyn FnMut(CatalogPublicationHookPoint) -> Result<(), ThemeError>,
    ) -> Result<ThemeDescriptor, ThemeError> {
        self.ensure_root()?;
        let candidate = prepared.descriptor().clone();
        if self.existing_descriptor(&candidate.id)?.is_some() {
            return Err(ThemeError::new(
                ThemeErrorCode::DuplicateTheme,
                "A theme with this ID already exists.",
            ));
        }
        self.with_prepared_staging(
            prepared,
            |candidate, staging_name, staging_path, publisher| {
                self.publish_new(candidate, staging_name, staging_path, hook, publisher)
            },
        )
    }

    pub(crate) fn replace_prepared(
        &self,
        prepared: PreparedThemeImport,
        expected_fingerprint: &str,
    ) -> Result<ThemeDescriptor, ThemeError> {
        self.replace_prepared_with_hook(prepared, expected_fingerprint, &mut |_| Ok(()))
    }

    fn replace_prepared_with_hook(
        &self,
        prepared: PreparedThemeImport,
        expected_fingerprint: &str,
        hook: &mut dyn FnMut(CatalogPublicationHookPoint) -> Result<(), ThemeError>,
    ) -> Result<ThemeDescriptor, ThemeError> {
        self.ensure_root()?;
        let candidate = prepared.descriptor().clone();
        let existing = self.find_descriptor(&candidate.id)?;
        if existing.fingerprint != expected_fingerprint {
            return Err(fingerprint_mismatch());
        }
        let current = self.load_installed(&existing)?;
        if current.descriptor().fingerprint != expected_fingerprint {
            return Err(fingerprint_mismatch());
        }
        self.with_prepared_staging(
            prepared,
            |candidate, staging_name, staging_path, publisher| {
                self.publish_replacement(
                    candidate,
                    &existing,
                    staging_name,
                    staging_path,
                    hook,
                    publisher,
                )
            },
        )
    }

    pub(crate) fn delete(&self, id: &str, expected_fingerprint: &str) -> Result<(), ThemeError> {
        self.delete_with_hook(id, expected_fingerprint, &mut |_| {})
    }

    fn delete_with_hook(
        &self,
        id: &str,
        expected_fingerprint: &str,
        hook: &mut dyn FnMut(DeleteHookPoint),
    ) -> Result<(), ThemeError> {
        if matches!(id, "light" | "dark") {
            return Err(ThemeError::new(
                ThemeErrorCode::ProtectedTheme,
                "Protected default themes cannot be deleted.",
            ));
        }
        let descriptor = self.find_descriptor(id)?;
        if descriptor.fingerprint != expected_fingerprint {
            return Err(fingerprint_mismatch());
        }
        let current = self.load_installed(&descriptor)?;
        if current.descriptor().fingerprint != expected_fingerprint {
            return Err(fingerprint_mismatch());
        }
        hook(DeleteHookPoint::AfterValidation);
        let directory = self.catalog_directory()?;
        let quarantine_name = reserve_backup_name(&directory, descriptor.storage_kind)?;
        let original_path = self.root.join(&descriptor.file_name);
        let quarantine_path = self.root.join(&quarantine_name);
        rename_catalog_noreplace(
            &directory,
            &descriptor.file_name,
            &quarantine_name,
            &original_path,
            &quarantine_path,
        )?;
        sync_catalog_directory(&directory);

        if let Err(error) = self.load_storage_at(&descriptor, &quarantine_path) {
            return restore_unpublished_backup(
                &directory,
                &quarantine_name,
                &descriptor.file_name,
                &quarantine_path,
                &original_path,
                if error.code == ThemeErrorCode::FingerprintMismatch {
                    error
                } else {
                    fingerprint_mismatch()
                },
            );
        }
        remove_catalog_entry(&directory, &quarantine_name, descriptor.storage_kind)?;
        sync_catalog_directory(&directory);
        Ok(())
    }

    pub(crate) fn existing_descriptor(
        &self,
        id: &str,
    ) -> Result<Option<ThemeDescriptor>, ThemeError> {
        Ok(self.scan()?.themes.into_iter().find(|theme| theme.id == id))
    }

    pub(crate) fn find_descriptor(&self, id: &str) -> Result<ThemeDescriptor, ThemeError> {
        self.existing_descriptor(id)?
            .ok_or_else(|| ThemeError::new(ThemeErrorCode::ThemeNotFound, "Theme was not found."))
    }

    fn with_prepared_staging<T>(
        &self,
        prepared: PreparedThemeImport,
        operation: impl FnOnce(
            &ThemeDescriptor,
            &str,
            &Path,
            &mut dyn FnMut(&CatalogDirectory, &str, &Path) -> Result<(), ThemeError>,
        ) -> Result<T, ThemeError>,
    ) -> Result<T, ThemeError> {
        match prepared {
            PreparedThemeImport::LegacyCss(theme) => {
                let candidate = theme.descriptor.clone();
                let (staging_name, staging_path) = self.stage_legacy(&theme)?;
                let mut publisher =
                    |directory: &CatalogDirectory, target_name: &str, target_path: &Path| {
                        rename_catalog_noreplace(
                            directory,
                            &staging_name,
                            target_name,
                            &staging_path,
                            target_path,
                        )
                    };
                let result = operation(&candidate, &staging_name, &staging_path, &mut publisher);
                let _cleanup = fs::remove_file(&staging_path);
                result
            }
            PreparedThemeImport::ResourcePackage(package) => {
                let candidate = package.validated().descriptor.clone();
                let staging_path = package.validated().root.clone();
                if staging_path.parent() != Some(self.root.as_path()) {
                    return Err(ThemeError::new(
                        ThemeErrorCode::UnsafePath,
                        "Prepared theme packages must remain below the catalog root.",
                    ));
                }
                let staging_name = staging_path
                    .file_name()
                    .and_then(OsStr::to_str)
                    .filter(|name| is_owned_staging_directory(name))
                    .ok_or_else(|| {
                        ThemeError::new(
                            ThemeErrorCode::UnsafePath,
                            "Prepared theme packages must use an owned staging directory.",
                        )
                    })?
                    .to_string();
                self.validate_staged(&candidate, &staging_path)?;
                let mut package = Some(package);
                let mut publisher =
                    |_: &CatalogDirectory, target_name: &str, _target_path: &Path| {
                        let published = package
                            .take()
                            .ok_or_else(|| {
                                ThemeError::new(
                                    ThemeErrorCode::Io,
                                    "Prepared theme package publication was attempted twice.",
                                )
                            })?
                            .publish_noreplace(target_name)?;
                        if !same_content_descriptor(&candidate, &published.descriptor) {
                            return Err(fingerprint_mismatch());
                        }
                        Ok(())
                    };
                let result = operation(&candidate, &staging_name, &staging_path, &mut publisher);
                drop(package);
                result
            }
        }
    }

    fn stage_legacy(&self, theme: &ParsedTheme) -> Result<(String, PathBuf), ThemeError> {
        let directory = self.catalog_directory()?;
        for _attempt in 0..1024 {
            let name = unique_owned_name("tmp.css");
            let path = self.root.join(&name);
            let mut options = cap_std::fs::OpenOptions::new();
            options.create_new(true).write(true);
            let mut file = match directory.open_with(&name, &options) {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(io_error(error)),
            };
            let write_result = (|| {
                file.write_all(&theme.bytes).map_err(io_error)?;
                file.sync_all().map_err(io_error)?;
                drop(file);
                self.validate_staged(&theme.descriptor, &path)
            })();
            if let Err(error) = write_result {
                let _cleanup = directory.remove_file(&name);
                return Err(error);
            }
            sync_catalog_directory(&directory);
            return Ok((name, path));
        }
        Err(ThemeError::new(
            ThemeErrorCode::Io,
            "Could not reserve a unique theme staging file.",
        ))
    }

    fn validate_staged(&self, candidate: &ThemeDescriptor, path: &Path) -> Result<(), ThemeError> {
        let current = match candidate.storage_kind {
            ThemeStorageKind::InlineCss => {
                let prepared = prepare_external_theme(path, &self.root)?;
                let PreparedThemeImport::LegacyCss(theme) = prepared else {
                    return Err(fingerprint_mismatch());
                };
                theme.descriptor
            }
            ThemeStorageKind::ResourceDirectory => {
                validate_theme_directory(path, &candidate.file_name)?.descriptor
            }
        };
        if !same_content_descriptor(candidate, &current) {
            return Err(fingerprint_mismatch());
        }
        Ok(())
    }

    fn publish_new(
        &self,
        candidate: &ThemeDescriptor,
        _staging_name: &str,
        staging_path: &Path,
        hook: &mut dyn FnMut(CatalogPublicationHookPoint) -> Result<(), ThemeError>,
        publisher: &mut dyn FnMut(&CatalogDirectory, &str, &Path) -> Result<(), ThemeError>,
    ) -> Result<ThemeDescriptor, ThemeError> {
        let target_name = canonical_storage_name(candidate);
        let target_path = self.safe_storage_path(&target_name, candidate.storage_kind)?;
        let directory = self.catalog_directory()?;
        ensure_catalog_entry_absent(&directory, &target_name)?;
        if candidate.storage_kind == ThemeStorageKind::InlineCss {
            self.validate_staged(candidate, staging_path)?;
        }
        hook(CatalogPublicationHookPoint::BeforePublication)?;
        if candidate.storage_kind == ThemeStorageKind::InlineCss {
            self.validate_staged(candidate, staging_path)?;
        }
        publisher(&directory, &target_name, &target_path)?;
        sync_catalog_directory(&directory);

        match self.fresh_published_descriptor(candidate) {
            Ok(descriptor) => Ok(descriptor),
            Err(error) => {
                let _cleanup =
                    remove_catalog_entry(&directory, &target_name, candidate.storage_kind);
                sync_catalog_directory(&directory);
                Err(error)
            }
        }
    }

    fn publish_replacement(
        &self,
        candidate: &ThemeDescriptor,
        existing: &ThemeDescriptor,
        _staging_name: &str,
        staging_path: &Path,
        hook: &mut dyn FnMut(CatalogPublicationHookPoint) -> Result<(), ThemeError>,
        publisher: &mut dyn FnMut(&CatalogDirectory, &str, &Path) -> Result<(), ThemeError>,
    ) -> Result<ThemeDescriptor, ThemeError> {
        let old_name = existing.file_name.as_str();
        self.safe_storage_path(old_name, existing.storage_kind)?;
        let target_name = canonical_storage_name(candidate);
        let target_path = self.safe_storage_path(&target_name, candidate.storage_kind)?;
        let old_path = self.root.join(old_name);
        let directory = self.catalog_directory()?;
        if target_name != old_name {
            ensure_catalog_entry_absent(&directory, &target_name)?;
        }
        let current = self.load_installed(existing)?;
        if !same_content_descriptor(existing, current.descriptor()) {
            return Err(fingerprint_mismatch());
        }
        self.validate_staged(candidate, staging_path)?;

        let backup_name = reserve_backup_name(&directory, existing.storage_kind)?;
        let backup_path = self.root.join(&backup_name);
        rename_catalog_noreplace(&directory, old_name, &backup_name, &old_path, &backup_path)?;
        sync_catalog_directory(&directory);

        if let Err(error) = self.load_storage_at(existing, &backup_path) {
            return restore_unpublished_backup(
                &directory,
                &backup_name,
                old_name,
                &backup_path,
                &old_path,
                error,
            );
        }

        if let Err(error) = hook(CatalogPublicationHookPoint::AfterBackup) {
            return restore_unpublished_backup(
                &directory,
                &backup_name,
                old_name,
                &backup_path,
                &old_path,
                error,
            );
        }
        if candidate.storage_kind == ThemeStorageKind::InlineCss {
            if let Err(error) = self.validate_staged(candidate, staging_path) {
                return restore_unpublished_backup(
                    &directory,
                    &backup_name,
                    old_name,
                    &backup_path,
                    &old_path,
                    error,
                );
            }
        }
        if let Err(error) = hook(CatalogPublicationHookPoint::BeforePublication) {
            return restore_unpublished_backup(
                &directory,
                &backup_name,
                old_name,
                &backup_path,
                &old_path,
                error,
            );
        }
        if candidate.storage_kind == ThemeStorageKind::InlineCss {
            if let Err(error) = self.validate_staged(candidate, staging_path) {
                return restore_unpublished_backup(
                    &directory,
                    &backup_name,
                    old_name,
                    &backup_path,
                    &old_path,
                    error,
                );
            }
        }
        if let Err(error) = publisher(&directory, &target_name, &target_path) {
            return restore_unpublished_backup(
                &directory,
                &backup_name,
                old_name,
                &backup_path,
                &old_path,
                error,
            );
        }
        sync_catalog_directory(&directory);

        if let Err(error) = hook(CatalogPublicationHookPoint::AfterPublication) {
            return restore_published_backup(
                &directory,
                &target_name,
                candidate.storage_kind,
                &backup_name,
                old_name,
                &target_path,
                &backup_path,
                &old_path,
                error,
            );
        }

        let installed = match self.fresh_published_descriptor(candidate) {
            Ok(installed) => installed,
            Err(error) => {
                return restore_published_backup(
                    &directory,
                    &target_name,
                    candidate.storage_kind,
                    &backup_name,
                    old_name,
                    &target_path,
                    &backup_path,
                    &old_path,
                    error,
                )
            }
        };
        let _cleanup = remove_catalog_entry(&directory, &backup_name, existing.storage_kind);
        sync_catalog_directory(&directory);
        Ok(installed)
    }

    fn fresh_published_descriptor(
        &self,
        candidate: &ThemeDescriptor,
    ) -> Result<ThemeDescriptor, ThemeError> {
        let installed = self.find_descriptor(&candidate.id)?;
        if !same_content_descriptor(candidate, &installed) {
            return Err(fingerprint_mismatch());
        }
        Ok(installed)
    }

    fn load_installed(&self, descriptor: &ThemeDescriptor) -> Result<InstalledTheme, ThemeError> {
        let path = self.safe_storage_path(&descriptor.file_name, descriptor.storage_kind)?;
        self.load_storage_at(descriptor, &path)
    }

    fn load_storage_at(
        &self,
        descriptor: &ThemeDescriptor,
        path: &Path,
    ) -> Result<InstalledTheme, ThemeError> {
        let installed = match descriptor.storage_kind {
            ThemeStorageKind::InlineCss => {
                let prepared = prepare_external_theme(&path, &self.root)?;
                let PreparedThemeImport::LegacyCss(theme) = prepared else {
                    return Err(fingerprint_mismatch());
                };
                InstalledTheme::Legacy(theme)
            }
            ThemeStorageKind::ResourceDirectory => {
                InstalledTheme::Resource(validate_theme_directory(&path, &descriptor.file_name)?)
            }
        };
        if !same_content_descriptor(descriptor, installed.descriptor()) {
            return Err(fingerprint_mismatch());
        }
        Ok(installed)
    }

    fn catalog_directory(&self) -> Result<CatalogDirectory, ThemeError> {
        self.ensure_root()?;
        let addressed =
            crate::storage_capability::ambient_symlink_metadata(&self.root).map_err(io_error)?;
        let directory =
            Dir::open_ambient_dir(&self.root, cap_std::ambient_authority()).map_err(io_error)?;
        let retained = directory.dir_metadata().map_err(io_error)?;
        let identity = catalog_file_identity(&addressed);
        if !retained.is_dir() || identity != catalog_file_identity(&retained) {
            return Err(ThemeError::new(
                ThemeErrorCode::UnsafePath,
                "The theme catalog root changed while it was being opened.",
            ));
        }
        Ok(CatalogDirectory {
            directory,
            root: self.root.clone(),
            identity,
        })
    }

    fn safe_storage_path(
        &self,
        storage_name: &str,
        storage_kind: ThemeStorageKind,
    ) -> Result<PathBuf, ThemeError> {
        let path = Path::new(storage_name);
        if path.components().count() != 1
            || storage_name.is_empty()
            || is_owned_catalog_entry(storage_name)
            || (storage_kind == ThemeStorageKind::InlineCss
                && path.extension().and_then(OsStr::to_str) != Some("css"))
        {
            return Err(ThemeError::new(
                ThemeErrorCode::UnsafePath,
                "Theme storage paths must name one safe catalog entry.",
            ));
        }
        Ok(self.root.join(path))
    }

    fn ensure_root(&self) -> Result<(), ThemeError> {
        match fs::symlink_metadata(&self.root) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
                    return Err(ThemeError::new(
                        ThemeErrorCode::UnsafePath,
                        "The theme directory must be a regular directory and cannot be a symbolic link.",
                    ));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir_all(&self.root).map_err(io_error)?;
                let metadata = fs::symlink_metadata(&self.root).map_err(io_error)?;
                if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
                    return Err(ThemeError::new(
                        ThemeErrorCode::UnsafePath,
                        "The theme directory could not be created safely.",
                    ));
                }
            }
            Err(error) => return Err(io_error(error)),
        }
        Ok(())
    }

    fn safe_theme_path(&self, file_name: &str) -> Result<PathBuf, ThemeError> {
        let path = Path::new(file_name);
        if path.components().count() != 1 || path.extension().and_then(OsStr::to_str) != Some("css")
        {
            return Err(ThemeError::new(
                ThemeErrorCode::UnsafePath,
                "Theme paths must be a single CSS file name.",
            ));
        }
        Ok(self.root.join(path))
    }

    fn atomic_write(&self, target: &Path, bytes: &[u8]) -> Result<(), ThemeError> {
        let sequence = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temp = self.root.join(format!(
            ".qingyu-theme-{}-{sequence}.tmp",
            std::process::id()
        ));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)
            .map_err(io_error)?;
        let result = (|| {
            file.write_all(bytes).map_err(io_error)?;
            file.sync_all().map_err(io_error)?;
            drop(file);
            fs::rename(&temp, target).map_err(io_error)
        })();
        if result.is_err() {
            let _cleanup = fs::remove_file(&temp);
        }
        result
    }
}

#[cfg(test)]
fn materialize_embedded_theme(
    root: &Path,
    package: &EmbeddedThemePackage,
) -> Result<ValidatedThemeDirectory, ThemeError> {
    materialize_embedded_theme_with_hook(root, package, &mut |_, _, _| Ok(()))
}

#[cfg(test)]
fn materialize_embedded_theme_with_hook(
    root: &Path,
    package: &EmbeddedThemePackage,
    hook: &mut dyn FnMut(
        EmbeddedSeedHookPoint,
        &EmbeddedThemePackage,
        &Path,
    ) -> Result<(), ThemeError>,
) -> Result<ValidatedThemeDirectory, ThemeError> {
    let Some(mut materialized) = try_materialize_embedded_theme(root, package, hook)? else {
        return Err(ThemeError::new(
            ThemeErrorCode::Io,
            "Embedded theme staging path is already occupied.",
        ));
    };
    materialized._cleanup.disarm();
    materialized.validated.take().ok_or_else(|| {
        ThemeError::new(
            ThemeErrorCode::Io,
            "Embedded theme staging lost its validation result.",
        )
    })
}

fn try_materialize_embedded_theme(
    root: &Path,
    package: &EmbeddedThemePackage,
    hook: &mut dyn FnMut(
        EmbeddedSeedHookPoint,
        &EmbeddedThemePackage,
        &Path,
    ) -> Result<(), ThemeError>,
) -> Result<Option<MaterializedEmbeddedTheme>, ThemeError> {
    let parent_path = root.parent().ok_or_else(|| {
        ThemeError::new(
            ThemeErrorCode::UnsafePath,
            "Embedded themes require a parent directory.",
        )
    })?;
    let root_name = root.file_name().ok_or_else(|| {
        ThemeError::new(
            ThemeErrorCode::UnsafePath,
            "Embedded themes require a package directory name.",
        )
    })?;
    let addressed_parent =
        crate::storage_capability::ambient_symlink_metadata(parent_path).map_err(io_error)?;
    if addressed_parent.file_type().is_symlink() || !addressed_parent.is_dir() {
        return Err(ThemeError::new(
            ThemeErrorCode::UnsafePath,
            "Embedded theme parent must be a regular directory.",
        ));
    }
    let parent =
        Dir::open_ambient_dir(parent_path, cap_std::ambient_authority()).map_err(io_error)?;
    let retained_parent = parent.dir_metadata().map_err(io_error)?;
    if !retained_parent.is_dir()
        || catalog_file_identity(&addressed_parent) != catalog_file_identity(&retained_parent)
    {
        return Err(ThemeError::new(
            ThemeErrorCode::UnsafePath,
            "Embedded theme parent changed while it was opened.",
        ));
    }
    hook(EmbeddedSeedHookPoint::BeforeStagingCreate, package, root)?;
    let cleanup_parent = parent.try_clone().map_err(io_error)?;
    match parent.create_dir(root_name) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => return Ok(None),
        Err(error) => return Err(io_error(error)),
    }
    let mut cleanup = Some(EmbeddedStagingCleanup::new(
        cleanup_parent,
        PathBuf::from(root_name),
    ));
    let result = (|| {
        hook(EmbeddedSeedHookPoint::AfterStagingCreate, package, root)?;
        let root_directory = parent.open_dir_nofollow(root_name).map_err(|_| {
            ThemeError::new(
                ThemeErrorCode::UnsafePath,
                "Embedded theme staging changed.",
            )
        })?;
        revalidate_embedded_staging(&parent, root_name, &root_directory)?;
        for (relative_path, bytes) in package.files {
            let relative_path = Path::new(relative_path);
            if relative_path.is_absolute()
                || relative_path
                    .components()
                    .any(|component| !matches!(component, std::path::Component::Normal(_)))
            {
                return Err(ThemeError::new(
                    ThemeErrorCode::UnsafePath,
                    "Embedded theme paths must contain only safe relative components.",
                ));
            }
            let file_name = relative_path.file_name().ok_or_else(|| {
                ThemeError::new(
                    ThemeErrorCode::UnsafePath,
                    "Embedded theme path has no file name.",
                )
            })?;
            let file_parent = open_or_create_embedded_directory(
                &root_directory,
                relative_path.parent().unwrap_or_else(|| Path::new("")),
            )?;
            let mut options = CapOpenOptions::new();
            options
                .create_new(true)
                .write(true)
                .follow(FollowSymlinks::No);
            let mut file = file_parent.open_with(file_name, &options).map_err(|_| {
                ThemeError::new(
                    ThemeErrorCode::UnsafePath,
                    "Embedded theme staging encountered an occupied path.",
                )
            })?;
            file.write_all(bytes).map_err(io_error)?;
            file.sync_all().map_err(io_error)?;
        }
        hook(EmbeddedSeedHookPoint::AfterStagingWrite, package, root)?;
        revalidate_embedded_staging(&parent, root_name, &root_directory)?;
        validate_exact_embedded_graph(&root_directory, package)?;
        let validated =
            validate_theme_directory_from_retained(root, package.storage_name, &root_directory)?;
        validate_exact_embedded_files(&validated, package)?;
        revalidate_embedded_staging(&parent, root_name, &root_directory)?;
        validate_exact_embedded_graph(&root_directory, package)?;
        if validated.descriptor.id != package.id {
            return Err(ThemeError::new(
                ThemeErrorCode::InvalidManifest,
                "Embedded theme ID does not match its catalog seed.",
            ));
        }
        Ok(validated)
    })();
    match result {
        Ok(validated) => Ok(Some(MaterializedEmbeddedTheme {
            validated: Some(validated),
            _cleanup: cleanup.take().ok_or_else(|| {
                ThemeError::new(
                    ThemeErrorCode::Io,
                    "Embedded theme staging cleanup was unavailable.",
                )
            })?,
        })),
        Err(error) => Err(error),
    }
}

fn revalidate_embedded_staging(
    parent: &Dir,
    root_name: &OsStr,
    root_directory: &Dir,
) -> Result<(), ThemeError> {
    let addressed = parent.symlink_metadata(root_name).map_err(|_| {
        ThemeError::new(
            ThemeErrorCode::UnsafePath,
            "Embedded theme staging path is unavailable.",
        )
    })?;
    let retained = root_directory.dir_metadata().map_err(|_| {
        ThemeError::new(
            ThemeErrorCode::UnsafePath,
            "Embedded theme staging metadata is unavailable.",
        )
    })?;
    if addressed.file_type().is_symlink()
        || !addressed.is_dir()
        || !retained.is_dir()
        || catalog_file_identity(&addressed) != catalog_file_identity(&retained)
    {
        return Err(ThemeError::new(
            ThemeErrorCode::UnsafePath,
            "Embedded theme staging changed during materialization.",
        ));
    }
    Ok(())
}

fn validate_exact_embedded_theme(
    root: &Path,
    storage_name: &str,
    package: &EmbeddedThemePackage,
) -> Result<ValidatedThemeDirectory, ThemeError> {
    let validated = validate_theme_directory(root, storage_name)?;
    validate_exact_embedded_files(&validated, package)?;
    Ok(validated)
}

fn validate_exact_embedded_files(
    validated: &ValidatedThemeDirectory,
    package: &EmbeddedThemePackage,
) -> Result<(), ThemeError> {
    if validated.files.len() != package.files.len() {
        return Err(fingerprint_mismatch());
    }
    let actual = validated
        .files
        .iter()
        .map(|file| (file.relative_path.as_str(), file.bytes.as_slice()))
        .collect::<BTreeMap<_, _>>();
    if actual.len() != package.files.len()
        || package
            .files
            .iter()
            .any(|(path, bytes)| actual.get(path).copied() != Some(*bytes))
    {
        return Err(fingerprint_mismatch());
    }
    Ok(())
}

fn validate_exact_embedded_graph(
    root: &Dir,
    package: &EmbeddedThemePackage,
) -> Result<(), ThemeError> {
    let expected_files = package
        .files
        .iter()
        .map(|(path, bytes)| ((*path).to_string(), *bytes))
        .collect::<BTreeMap<_, _>>();
    if expected_files.len() != package.files.len() {
        return Err(fingerprint_mismatch());
    }
    let mut expected_directories = BTreeSet::new();
    for path in expected_files.keys() {
        let mut parent = Path::new(path).parent();
        while let Some(directory) = parent {
            if directory.as_os_str().is_empty() {
                break;
            }
            let directory = directory.to_str().ok_or_else(|| {
                ThemeError::new(
                    ThemeErrorCode::UnsafePath,
                    "Embedded theme paths must use valid UTF-8.",
                )
            })?;
            expected_directories.insert(directory.to_string());
            parent = Path::new(directory).parent();
        }
    }

    let mut pending = vec![(root.try_clone().map_err(io_error)?, PathBuf::new())];
    let mut actual_files = BTreeSet::new();
    let mut actual_directories = BTreeSet::new();
    while let Some((directory, relative_directory)) = pending.pop() {
        for entry in directory.entries().map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let name = entry.file_name();
            let name = name.to_str().ok_or_else(|| {
                ThemeError::new(
                    ThemeErrorCode::UnsafePath,
                    "Embedded theme paths must use valid UTF-8.",
                )
            })?;
            if name.is_empty() || name.contains(['/', '\\']) || matches!(name, "." | "..") {
                return Err(ThemeError::new(
                    ThemeErrorCode::UnsafePath,
                    "Embedded theme paths contain an unsafe segment.",
                ));
            }
            let relative_path = relative_directory.join(name);
            let relative_path_text = relative_path.to_str().ok_or_else(|| {
                ThemeError::new(
                    ThemeErrorCode::UnsafePath,
                    "Embedded theme paths must use valid UTF-8.",
                )
            })?;
            let metadata = directory.symlink_metadata(name).map_err(io_error)?;
            if metadata.file_type().is_symlink() {
                return Err(ThemeError::new(
                    ThemeErrorCode::UnsafePath,
                    "Embedded theme staging cannot contain symbolic links.",
                ));
            }
            if metadata.is_dir() {
                if !expected_directories.contains(relative_path_text) {
                    return Err(fingerprint_mismatch());
                }
                let child = directory.open_dir_nofollow(name).map_err(|_| {
                    ThemeError::new(
                        ThemeErrorCode::UnsafePath,
                        "Embedded theme directory changed while it was opened.",
                    )
                })?;
                let retained = child.dir_metadata().map_err(io_error)?;
                if !retained.is_dir()
                    || catalog_file_identity(&metadata) != catalog_file_identity(&retained)
                {
                    return Err(ThemeError::new(
                        ThemeErrorCode::UnsafePath,
                        "Embedded theme directory changed while it was opened.",
                    ));
                }
                actual_directories.insert(relative_path_text.to_string());
                pending.push((child, relative_path));
                continue;
            }
            if !metadata.is_file() {
                return Err(ThemeError::new(
                    ThemeErrorCode::UnsafePath,
                    "Embedded themes can contain only regular files and directories.",
                ));
            }
            let expected = expected_files
                .get(relative_path_text)
                .ok_or_else(fingerprint_mismatch)?;
            if metadata.len() != expected.len() as u64 {
                return Err(fingerprint_mismatch());
            }
            let mut options = CapOpenOptions::new();
            options.read(true).follow(FollowSymlinks::No);
            let mut file = directory.open_with(name, &options).map_err(|_| {
                ThemeError::new(
                    ThemeErrorCode::UnsafePath,
                    "Embedded theme file changed while it was opened.",
                )
            })?;
            let retained_before = file.metadata().map_err(io_error)?;
            if catalog_file_identity(&metadata) != catalog_file_identity(&retained_before) {
                return Err(ThemeError::new(
                    ThemeErrorCode::UnsafePath,
                    "Embedded theme file changed while it was opened.",
                ));
            }
            let mut bytes = Vec::new();
            (&mut file)
                .take(expected.len() as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(io_error)?;
            if bytes.as_slice() != *expected {
                return Err(fingerprint_mismatch());
            }
            let addressed_after = directory.symlink_metadata(name).map_err(io_error)?;
            let retained_after = file.metadata().map_err(io_error)?;
            if catalog_file_identity(&addressed_after) != catalog_file_identity(&retained_before)
                || catalog_file_identity(&retained_after) != catalog_file_identity(&retained_before)
            {
                return Err(ThemeError::new(
                    ThemeErrorCode::UnsafePath,
                    "Embedded theme file changed during validation.",
                ));
            }
            actual_files.insert(relative_path_text.to_string());
        }
    }
    if actual_files != expected_files.keys().cloned().collect()
        || actual_directories != expected_directories
    {
        return Err(fingerprint_mismatch());
    }
    Ok(())
}

fn open_or_create_embedded_directory(root: &Dir, relative: &Path) -> Result<Dir, ThemeError> {
    let mut current = root.try_clone().map_err(io_error)?;
    for component in relative.components() {
        let std::path::Component::Normal(segment) = component else {
            return Err(ThemeError::new(
                ThemeErrorCode::UnsafePath,
                "Embedded theme directory paths must be normalized.",
            ));
        };
        match current.create_dir(segment) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let metadata = current.symlink_metadata(segment).map_err(io_error)?;
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(ThemeError::new(
                        ThemeErrorCode::UnsafePath,
                        "Embedded theme staging encountered an unsafe directory.",
                    ));
                }
            }
            Err(error) => return Err(io_error(error)),
        }
        current = current.open_dir_nofollow(segment).map_err(|_| {
            ThemeError::new(
                ThemeErrorCode::UnsafePath,
                "Embedded theme staging directory changed while it was opened.",
            )
        })?;
    }
    Ok(current)
}

fn occupied_embedded_target(package: &EmbeddedThemePackage) -> InvalidThemeFile {
    InvalidThemeFile {
        file_name: package.storage_name.to_string(),
        reason: format!(
            "The bundled {} destination is occupied; the existing catalog entry was preserved.",
            package.id
        ),
    }
}

fn same_content_descriptor(left: &ThemeDescriptor, right: &ThemeDescriptor) -> bool {
    left.id == right.id
        && left.fingerprint == right.fingerprint
        && left.storage_kind == right.storage_kind
}

fn canonical_storage_name(descriptor: &ThemeDescriptor) -> String {
    match descriptor.storage_kind {
        ThemeStorageKind::InlineCss => format!("{}.css", descriptor.id),
        ThemeStorageKind::ResourceDirectory => descriptor.id.clone(),
    }
}

fn is_owned_staging_directory(name: &str) -> bool {
    owned_catalog_parts(name).is_some_and(|(_, _, suffix)| suffix == "dir")
}

fn is_owned_catalog_entry(name: &str) -> bool {
    owned_catalog_parts(name).is_some()
}

fn owned_catalog_parts(name: &str) -> Option<(&str, &str, &str)> {
    let rest = name.strip_prefix(".qingyu-theme-")?;
    let (stem, suffix) = if let Some(stem) = rest.strip_suffix(".tmp.theme") {
        (stem, "tmp.theme")
    } else if let Some(stem) = rest.strip_suffix(".tmp.css") {
        (stem, "tmp.css")
    } else if let Some(stem) = rest.strip_suffix(".bak.css") {
        (stem, "bak.css")
    } else if let Some(stem) = rest.strip_suffix(".dir") {
        (stem, "dir")
    } else if let Some(stem) = rest.strip_suffix(".bak") {
        (stem, "bak")
    } else if let Some(stem) = rest.strip_suffix(".tmp") {
        (stem, "tmp")
    } else {
        return None;
    };
    let (process, counter) = stem.split_once('-')?;
    if process.is_empty()
        || counter.is_empty()
        || process.contains('-')
        || counter.contains('-')
        || !process.bytes().all(|byte| byte.is_ascii_digit())
        || !counter.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    Some((process, counter, suffix))
}

fn unique_owned_name(suffix: &str) -> String {
    let sequence = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!(".qingyu-theme-{}-{sequence}.{suffix}", std::process::id())
}

fn reserve_backup_name(
    directory: &Dir,
    storage_kind: ThemeStorageKind,
) -> Result<String, ThemeError> {
    for _attempt in 0..1024 {
        let suffix = match storage_kind {
            ThemeStorageKind::InlineCss => "bak.css",
            ThemeStorageKind::ResourceDirectory => "bak",
        };
        let name = unique_owned_name(suffix);
        match directory.symlink_metadata(&name) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(name),
            Ok(_) => continue,
            Err(error) => return Err(io_error(error)),
        }
    }
    Err(ThemeError::new(
        ThemeErrorCode::Io,
        "Could not reserve a unique theme backup entry.",
    ))
}

fn ensure_catalog_entry_absent(directory: &Dir, name: &str) -> Result<(), ThemeError> {
    match directory.symlink_metadata(name) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Ok(_) => Err(ThemeError::new(
            ThemeErrorCode::DuplicateTheme,
            "The target theme storage entry already exists.",
        )),
        Err(error) => Err(io_error(error)),
    }
}

fn remove_catalog_entry(
    directory: &Dir,
    name: &str,
    storage_kind: ThemeStorageKind,
) -> Result<(), ThemeError> {
    let metadata = directory.symlink_metadata(name).map_err(io_error)?;
    if metadata.file_type().is_symlink() {
        return Err(ThemeError::new(
            ThemeErrorCode::UnsafePath,
            "Theme storage changed before it could be removed safely.",
        ));
    }
    match storage_kind {
        ThemeStorageKind::InlineCss if metadata.is_file() => {
            directory.remove_file(name).map_err(io_error)
        }
        ThemeStorageKind::ResourceDirectory if metadata.is_dir() => {
            directory.remove_dir_all(name).map_err(io_error)
        }
        _ => Err(ThemeError::new(
            ThemeErrorCode::UnsafePath,
            "Theme storage kind changed before it could be removed safely.",
        )),
    }
}

fn restore_unpublished_backup<T>(
    directory: &CatalogDirectory,
    backup_name: &str,
    old_name: &str,
    backup_path: &Path,
    old_path: &Path,
    publication_error: ThemeError,
) -> Result<T, ThemeError> {
    let restoration =
        rename_catalog_noreplace(directory, backup_name, old_name, backup_path, old_path);
    sync_catalog_directory(directory);
    match restoration {
        Ok(()) => Err(publication_error),
        Err(restoration_error) => Err(ThemeError::new(
            ThemeErrorCode::Io,
            format!(
                "Theme publication failed ({publication_error}) and the previous storage could not be restored ({restoration_error})."
            ),
        )),
    }
}

#[allow(clippy::too_many_arguments)]
fn restore_published_backup<T>(
    directory: &CatalogDirectory,
    target_name: &str,
    target_kind: ThemeStorageKind,
    backup_name: &str,
    old_name: &str,
    target_path: &Path,
    backup_path: &Path,
    old_path: &Path,
    publication_error: ThemeError,
) -> Result<T, ThemeError> {
    let rejected_name = reserve_backup_name(directory, target_kind)?;
    let rejected_path = target_path.with_file_name(&rejected_name);
    if let Err(error) = rename_catalog_noreplace(
        directory,
        target_name,
        &rejected_name,
        target_path,
        &rejected_path,
    ) {
        return Err(ThemeError::new(
            ThemeErrorCode::Io,
            format!(
                "Theme publication failed ({publication_error}) and the rejected storage could not be isolated ({error})."
            ),
        ));
    }
    if let Err(error) =
        rename_catalog_noreplace(directory, backup_name, old_name, backup_path, old_path)
    {
        return Err(ThemeError::new(
            ThemeErrorCode::Io,
            format!(
                "Theme publication failed ({publication_error}) and the previous storage could not be restored ({error})."
            ),
        ));
    }
    let _cleanup = remove_catalog_entry(directory, &rejected_name, target_kind);
    sync_catalog_directory(directory);
    Err(publication_error)
}

fn rename_catalog_noreplace(
    directory: &CatalogDirectory,
    source_name: impl AsRef<Path>,
    target_name: impl AsRef<Path>,
    source_ambient: &Path,
    target_ambient: &Path,
) -> Result<(), ThemeError> {
    rename_catalog_noreplace_with_hook(
        directory,
        source_name,
        target_name,
        source_ambient,
        target_ambient,
        &mut |_| {},
    )
}

fn rename_catalog_noreplace_with_hook(
    directory: &CatalogDirectory,
    source_name: impl AsRef<Path>,
    target_name: impl AsRef<Path>,
    _source_ambient: &Path,
    _target_ambient: &Path,
    hook: &mut dyn FnMut(CatalogRenameHookPoint),
) -> Result<(), ThemeError> {
    let source_name = source_name.as_ref();
    let target_name = target_name.as_ref();
    revalidate_catalog_directory(directory)?;
    hook(CatalogRenameHookPoint::AfterRootValidation);
    revalidate_catalog_directory(directory)?;
    rename_catalog_noreplace_platform(&directory.directory, source_name, target_name)
        .map_err(io_error)?;
    if let Err(error) = revalidate_catalog_directory(directory) {
        return match rename_catalog_noreplace_platform(
            &directory.directory,
            target_name,
            source_name,
        ) {
            Ok(()) => Err(error),
            Err(restoration_error) => Err(ThemeError::new(
                ThemeErrorCode::Io,
                format!(
                    "The theme catalog root changed after a rename and the rename could not be rolled back ({restoration_error})."
                ),
            )),
        };
    }
    Ok(())
}

fn rename_embedded_seed_noreplace(
    directory: &CatalogDirectory,
    source_name: impl AsRef<Path>,
    target_name: impl AsRef<Path>,
) -> Result<bool, ThemeError> {
    let source_name = source_name.as_ref();
    let target_name = target_name.as_ref();
    revalidate_catalog_directory(directory)?;
    match rename_catalog_noreplace_platform(&directory.directory, source_name, target_name) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            revalidate_catalog_directory(directory)?;
            return Ok(false);
        }
        Err(error) => return Err(io_error(error)),
    }
    if let Err(error) = revalidate_catalog_directory(directory) {
        return match rename_catalog_noreplace_platform(
            &directory.directory,
            target_name,
            source_name,
        ) {
            Ok(()) => Err(error),
            Err(restoration_error) => Err(ThemeError::new(
                ThemeErrorCode::Io,
                format!(
                    "The theme catalog root changed after an embedded seed rename and the rename could not be rolled back ({restoration_error})."
                ),
            )),
        };
    }
    Ok(true)
}

fn revalidate_catalog_directory(directory: &CatalogDirectory) -> Result<(), ThemeError> {
    let retained = directory.directory.dir_metadata().map_err(io_error)?;
    let addressed =
        crate::storage_capability::ambient_symlink_metadata(&directory.root).map_err(io_error)?;
    if addressed.file_type().is_symlink()
        || !addressed.is_dir()
        || catalog_file_identity(&retained) != directory.identity
        || catalog_file_identity(&addressed) != directory.identity
    {
        return Err(ThemeError::new(
            ThemeErrorCode::UnsafePath,
            "The theme catalog root changed during a catalog operation.",
        ));
    }
    let reopened =
        Dir::open_ambient_dir(&directory.root, cap_std::ambient_authority()).map_err(io_error)?;
    let reopened = reopened.dir_metadata().map_err(io_error)?;
    if catalog_file_identity(&reopened) != directory.identity {
        return Err(ThemeError::new(
            ThemeErrorCode::UnsafePath,
            "The theme catalog root changed during a catalog operation.",
        ));
    }
    Ok(())
}

fn catalog_file_identity<T: MetadataExt>(metadata: &T) -> CatalogFileIdentity {
    CatalogFileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    }
}

fn rename_catalog_noreplace_platform(
    directory: &Dir,
    source_name: &Path,
    target_name: &Path,
) -> io::Result<()> {
    crate::atomic_noreplace::rename_noreplace(directory, source_name, directory, target_name)
}

fn sync_catalog_directory(directory: &Dir) {
    #[cfg(unix)]
    let _sync = rustix::fs::fsync(directory);
    #[cfg(not(unix))]
    let _ = directory;
}

fn io_error(error: std::io::Error) -> ThemeError {
    ThemeError::new(
        ThemeErrorCode::Io,
        format!("Theme file operation failed: {error}"),
    )
}

fn fingerprint_mismatch() -> ThemeError {
    ThemeError::new(
        ThemeErrorCode::FingerprintMismatch,
        "The theme file changed. Refresh the theme catalog and retry.",
    )
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Write, path::Path};

    use tempfile::tempdir;
    use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

    use super::{
        materialize_embedded_theme, materialize_embedded_theme_with_hook,
        rename_catalog_noreplace_with_hook, CatalogPublicationHookPoint, CatalogRenameHookPoint,
        DeleteHookPoint, EmbeddedSeedHookPoint, ThemeCatalog, ACTIVATION_LEASE_PARENT_NAME,
        DRAKE_THEME_PACKAGES,
    };
    use crate::themes::{
        archive::prepare_external_theme, manifest::parse_theme_manifest, parser::MAX_THEME_BYTES,
        resources::validate_theme_directory, ThemeAppearance, ThemeError, ThemeErrorCode,
        ThemeStorageKind,
    };
    use sha2::{Digest, Sha256};

    fn css(id: &str, name: &str, appearance: &str, suffix: &str) -> Vec<u8> {
        format!(
            "/*\n@qingyu-theme\nid: {id}\nname: {name}\nappearance: {appearance}\npreview-background: #ffffff\npreview-panel: #f6f8fa\npreview-text: #1f2328\npreview-accent: #0969da\n*/\n:root {{ --suffix: {suffix}; }}\n"
        )
        .into_bytes()
    }

    fn write_package(root: &Path, id: &str, name: &str, appearance: &str, suffix: &str) {
        fs::create_dir_all(root).unwrap();
        fs::write(
            root.join("manifest.json"),
            serde_json::to_vec(&serde_json::json!({
                "schemaVersion": 1,
                "id": id,
                "name": name,
                "appearance": appearance,
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

    fn write_package_archive(
        source_root: &Path,
        archive_path: &Path,
        _catalog_root: &Path,
        id: &str,
        name: &str,
        appearance: &str,
        suffix: &str,
    ) {
        write_package(source_root, id, name, appearance, suffix);
        let output = fs::File::create(archive_path).unwrap();
        let mut writer = ZipWriter::new(output);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for name in ["manifest.json", "theme.css"] {
            writer.start_file(name, options).unwrap();
            writer
                .write_all(&fs::read(source_root.join(name)).unwrap())
                .unwrap();
        }
        writer.finish().unwrap();
    }

    fn owned_artifacts(root: &Path) -> Vec<String> {
        let mut names = fs::read_dir(root)
            .unwrap()
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| name.starts_with(".qingyu-theme-"))
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    #[test]
    fn activation_maps_post_scan_size_growth_to_fingerprint_mismatch() {
        let temporary = tempdir().unwrap();
        let catalog_root = temporary.path().join("themes");
        fs::create_dir_all(&catalog_root).unwrap();
        let source = catalog_root.join("second-read-size.css");
        fs::write(
            &source,
            css("second-read-size", "Second Read Size", "light", "original"),
        )
        .unwrap();
        let catalog = ThemeCatalog::at(catalog_root);
        let descriptor = catalog.find_descriptor("second-read-size").unwrap();

        let error = match catalog.prepare_activation_with_hint_and_hook(
            "second-read-size",
            &descriptor.fingerprint,
            None,
            &mut |found| {
                assert_eq!(found, &descriptor);
                fs::write(&source, vec![b'A'; MAX_THEME_BYTES + 1]).unwrap();
                Ok(())
            },
        ) {
            Err(error) => error,
            Ok(_) => panic!("post-scan size growth should invalidate activation"),
        };

        assert_eq!(error.code, ThemeErrorCode::FingerprintMismatch);
    }

    #[test]
    fn activation_maps_post_scan_invalid_metadata_to_fingerprint_mismatch() {
        let temporary = tempdir().unwrap();
        let catalog_root = temporary.path().join("themes");
        fs::create_dir_all(&catalog_root).unwrap();
        let source = catalog_root.join("second-read-metadata.css");
        fs::write(
            &source,
            css(
                "second-read-metadata",
                "Second Read Metadata",
                "light",
                "original",
            ),
        )
        .unwrap();
        let catalog = ThemeCatalog::at(catalog_root);
        let descriptor = catalog.find_descriptor("second-read-metadata").unwrap();

        let error = match catalog.prepare_activation_with_hint_and_hook(
            "second-read-metadata",
            &descriptor.fingerprint,
            None,
            &mut |found| {
                assert_eq!(found, &descriptor);
                fs::write(&source, b":root { --metadata: missing; }\n").unwrap();
                Ok(())
            },
        ) {
            Err(error) => error,
            Ok(_) => panic!("post-scan metadata mutation should invalidate activation"),
        };

        assert_eq!(error.code, ThemeErrorCode::FingerprintMismatch);
    }

    #[test]
    fn bundled_drake_packages_validate_fonts_licenses_and_source_hashes() {
        const EXPECTED_FONT_HASHES: [(&str, &str); 4] = [
            (
                "assets/fonts/JetBrainsMono-Bold.woff2",
                "df3f86c04988d8f7fc516db3e95ec6b630cdc67bec91fe4297c6f8e132be1037",
            ),
            (
                "assets/fonts/JetBrainsMono-BoldItalic.woff2",
                "3aa30cac2529ca86f6b8ef479f143d924378682657510541d10d8e8b6d07120b",
            ),
            (
                "assets/fonts/JetBrainsMono-Italic.woff2",
                "9aef9fe9f1292b1cc4b1af075e4e9bc5f2adf23fef54908e58e2ebe338f33a65",
            ),
            (
                "assets/fonts/JetBrainsMono-Regular.woff2",
                "bceff0710e3a7fe5b3622265c48b6fbc055cf071df80ef5f36ffc69550296664",
            ),
        ];
        let temp = tempdir().unwrap();

        for package in DRAKE_THEME_PACKAGES {
            let root = temp.path().join(package.storage_name);
            let validated = materialize_embedded_theme(&root, package).unwrap();
            assert_eq!(validated.descriptor.id, package.id);
            assert_eq!(
                validated.descriptor.storage_kind,
                ThemeStorageKind::ResourceDirectory
            );
            assert_eq!(
                validated.descriptor.appearance,
                if package.id == "drake-light" {
                    ThemeAppearance::Light
                } else {
                    ThemeAppearance::Dark
                }
            );

            let manifest = parse_theme_manifest(
                &validated
                    .files
                    .iter()
                    .find(|file| file.relative_path == "manifest.json")
                    .unwrap()
                    .bytes,
            )
            .unwrap();
            assert_eq!(
                manifest.license_files,
                [
                    "licenses/THEME-LICENSE.txt".to_string(),
                    "licenses/FONT-LICENSE.txt".to_string(),
                ]
            );

            let css = String::from_utf8(
                validated
                    .files
                    .iter()
                    .find(|file| file.relative_path == "theme.css")
                    .unwrap()
                    .bytes
                    .clone(),
            )
            .unwrap();
            for (path, expected_hash) in EXPECTED_FONT_HASHES {
                let font = validated
                    .files
                    .iter()
                    .find(|file| file.relative_path == path)
                    .unwrap();
                assert!(font.bytes.starts_with(b"wOF2"));
                assert_eq!(format!("{:x}", Sha256::digest(&font.bytes)), expected_hash);
                assert!(css.contains(&format!("url(\"./{path}\")")));
            }
            assert_eq!(
                validated
                    .files
                    .iter()
                    .filter(|file| file.relative_path.ends_with(".woff2"))
                    .count(),
                4
            );
        }
    }

    #[test]
    fn embedded_materialization_rejects_a_same_id_replacement_before_validation() {
        let temp = tempdir().unwrap();
        let package = &DRAKE_THEME_PACKAGES[0];
        let staging = temp.path().join(".qingyu-theme-embedded-test.dir");
        let displaced = temp.path().join("displaced-original");
        let replacement = temp.path().join("same-id-replacement");
        materialize_embedded_theme(&replacement, package).unwrap();
        let replacement_css = fs::read(replacement.join("theme.css")).unwrap();
        fs::write(
            replacement.join("theme.css"),
            [
                replacement_css.as_slice(),
                b"\n/* different same-ID bytes */\n",
            ]
            .concat(),
        )
        .unwrap();
        assert_eq!(
            validate_theme_directory(&replacement, package.storage_name)
                .unwrap()
                .descriptor
                .id,
            package.id
        );

        let error = materialize_embedded_theme_with_hook(
            &staging,
            package,
            &mut |point, _, current_path| {
                if point == EmbeddedSeedHookPoint::AfterStagingWrite {
                    fs::rename(current_path, &displaced).unwrap();
                    fs::rename(&replacement, current_path).unwrap();
                }
                Ok(())
            },
        )
        .unwrap_err();

        assert!(matches!(
            error.code,
            ThemeErrorCode::UnsafePath | ThemeErrorCode::FingerprintMismatch
        ));
        assert!(!staging.exists());
        assert!(displaced.exists());
        assert!(owned_artifacts(temp.path()).is_empty());
    }

    #[test]
    fn drake_publication_collision_is_diagnostic_and_other_seed_continues() {
        let temp = tempdir().unwrap();
        let catalog = ThemeCatalog::at(temp.path().to_path_buf());
        let mut injected = false;

        let diagnostics = catalog
            .seed_missing_drake_with_hook(&mut |point, package, path| {
                if !injected
                    && point == EmbeddedSeedHookPoint::BeforePublication
                    && package.id == "drake-light"
                {
                    write_package(path, "author-race", "Author Race", "light", "occupant");
                    injected = true;
                }
                Ok(())
            })
            .unwrap();

        assert!(injected);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].file_name, "drake-light");
        assert!(diagnostics[0].reason.contains("occupied"));
        assert_eq!(
            validate_theme_directory(&temp.path().join("drake-light"), "drake-light")
                .unwrap()
                .descriptor
                .id,
            "author-race"
        );
        assert!(catalog.existing_descriptor("drake-ayu").unwrap().is_some());
        assert!(owned_artifacts(temp.path()).is_empty());
    }

    #[test]
    fn drake_staging_name_collision_retries_without_removing_the_occupant() {
        let temp = tempdir().unwrap();
        let catalog = ThemeCatalog::at(temp.path().to_path_buf());
        let mut collision = None;

        let diagnostics = catalog
            .seed_missing_drake_with_hook(&mut |point, package, path| {
                if collision.is_none()
                    && point == EmbeddedSeedHookPoint::BeforeStagingCreate
                    && package.id == "drake-light"
                {
                    fs::create_dir(path).unwrap();
                    fs::write(path.join("owner-marker"), b"preserve collision").unwrap();
                    collision = Some(path.to_path_buf());
                }
                Ok(())
            })
            .unwrap();

        assert!(diagnostics.is_empty());
        let collision = collision.unwrap();
        assert_eq!(
            fs::read(collision.join("owner-marker")).unwrap(),
            b"preserve collision"
        );
        assert_eq!(
            owned_artifacts(temp.path()),
            vec![collision
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()]
        );
        assert!(catalog
            .existing_descriptor("drake-light")
            .unwrap()
            .is_some());
        assert!(catalog.existing_descriptor("drake-ayu").unwrap().is_some());
    }

    #[test]
    fn failure_after_drake_staging_create_leaves_no_owned_residue() {
        let temp = tempdir().unwrap();
        let catalog = ThemeCatalog::at(temp.path().to_path_buf());

        let error = catalog
            .seed_missing_drake_with_hook(&mut |point, package, _| {
                if point == EmbeddedSeedHookPoint::AfterStagingCreate && package.id == "drake-light"
                {
                    return Err(ThemeError::new(
                        ThemeErrorCode::Io,
                        "injected after staging creation",
                    ));
                }
                Ok(())
            })
            .unwrap_err();

        assert_eq!(error.code, ThemeErrorCode::Io);
        assert!(owned_artifacts(temp.path()).is_empty());
    }

    fn directory_bytes(root: &Path) -> Vec<(String, Vec<u8>)> {
        fn collect(base: &Path, current: &Path, output: &mut Vec<(String, Vec<u8>)>) {
            let mut entries = fs::read_dir(current)
                .unwrap()
                .map(Result::unwrap)
                .collect::<Vec<_>>();
            entries.sort_by_key(|entry| entry.file_name());
            for entry in entries {
                let path = entry.path();
                if entry.file_type().unwrap().is_dir() {
                    collect(base, &path, output);
                } else {
                    output.push((
                        path.strip_prefix(base)
                            .unwrap()
                            .to_string_lossy()
                            .into_owned(),
                        fs::read(path).unwrap(),
                    ));
                }
            }
        }

        let mut output = Vec::new();
        collect(root, root, &mut output);
        output
    }

    #[test]
    fn scan_discovers_legacy_css_and_resource_directories_together() {
        let temp = tempdir().unwrap();
        fs::write(
            temp.path().join("legacy.css"),
            css("legacy", "Legacy", "light", "1"),
        )
        .unwrap();
        write_package(
            &temp.path().join("author-package"),
            "resource",
            "Resource",
            "dark",
            "2",
        );

        let snapshot = ThemeCatalog::at(temp.path().to_path_buf()).scan().unwrap();

        assert!(snapshot.invalid_files.is_empty());
        assert_eq!(snapshot.themes.len(), 2);
        assert_eq!(snapshot.themes[0].id, "legacy");
        assert_eq!(snapshot.themes[0].storage_kind, ThemeStorageKind::InlineCss);
        assert_eq!(snapshot.themes[1].id, "resource");
        assert_eq!(
            snapshot.themes[1].storage_kind,
            ThemeStorageKind::ResourceDirectory
        );
        assert_eq!(snapshot.themes[1].file_name, "author-package");
    }

    #[test]
    fn scan_reports_invalid_files_and_directories_without_hiding_valid_themes() {
        let temp = tempdir().unwrap();
        fs::write(
            temp.path().join("valid.css"),
            css("valid", "Valid", "light", "1"),
        )
        .unwrap();
        write_package(
            &temp.path().join("valid-package"),
            "valid-package",
            "Valid Package",
            "dark",
            "2",
        );
        fs::write(temp.path().join("invalid.css"), b"not metadata").unwrap();
        fs::create_dir(temp.path().join("invalid-package")).unwrap();
        fs::write(temp.path().join("notes.txt"), b"not a theme").unwrap();

        let snapshot = ThemeCatalog::at(temp.path().to_path_buf()).scan().unwrap();

        assert_eq!(snapshot.themes.len(), 2);
        assert_eq!(snapshot.invalid_files.len(), 3);
        assert_eq!(
            snapshot
                .invalid_files
                .iter()
                .map(|invalid| invalid.file_name.as_str())
                .collect::<Vec<_>>(),
            vec!["invalid-package", "invalid.css", "notes.txt"]
        );
    }

    #[test]
    fn scan_invalidates_duplicate_ids_across_storage_kinds() {
        let temp = tempdir().unwrap();
        fs::write(
            temp.path().join("duplicate.css"),
            css("duplicate", "Legacy Duplicate", "light", "1"),
        )
        .unwrap();
        write_package(
            &temp.path().join("duplicate-package"),
            "duplicate",
            "Resource Duplicate",
            "dark",
            "2",
        );

        let snapshot = ThemeCatalog::at(temp.path().to_path_buf()).scan().unwrap();

        assert!(snapshot.themes.is_empty());
        assert_eq!(snapshot.invalid_files.len(), 2);
        assert!(snapshot
            .invalid_files
            .iter()
            .all(|invalid| invalid.reason.contains("duplicate")));
    }

    #[test]
    fn scan_skips_only_exact_owned_staging_and_backup_names() {
        let temp = tempdir().unwrap();
        fs::create_dir(temp.path().join(".qingyu-theme-12-34.dir")).unwrap();
        fs::create_dir(temp.path().join(".qingyu-theme-12-35.bak")).unwrap();
        fs::write(temp.path().join(".qingyu-theme-12-36.tmp"), b"partial").unwrap();
        fs::write(
            temp.path().join(".qingyu-theme-12-37.tmp.theme"),
            b"partial",
        )
        .unwrap();
        fs::create_dir(temp.path().join(ACTIVATION_LEASE_PARENT_NAME)).unwrap();
        fs::create_dir(
            temp.path()
                .join(ACTIVATION_LEASE_PARENT_NAME)
                .join("author-content"),
        )
        .unwrap();
        fs::create_dir(temp.path().join(".qingyu-theme-author")).unwrap();
        fs::create_dir(temp.path().join(".qingyu-theme-12-nope.dir")).unwrap();

        let snapshot = ThemeCatalog::at(temp.path().to_path_buf()).scan().unwrap();

        assert!(snapshot.themes.is_empty());
        assert_eq!(
            snapshot
                .invalid_files
                .iter()
                .map(|invalid| invalid.file_name.as_str())
                .collect::<Vec<_>>(),
            vec![".qingyu-theme-12-nope.dir", ".qingyu-theme-author"]
        );
    }

    #[test]
    fn install_resource_package_renames_validated_staging_and_returns_fresh_descriptor() {
        let temp = tempdir().unwrap();
        let catalog_root = temp.path().join("themes");
        fs::create_dir(&catalog_root).unwrap();
        let source_root = temp.path().join("source-package");
        let source_archive = temp.path().join("source.theme");
        write_package_archive(
            &source_root,
            &source_archive,
            &catalog_root,
            "installed",
            "Installed",
            "light",
            "1",
        );
        let prepared = prepare_external_theme(&source_archive, &catalog_root).unwrap();
        let staging = match &prepared {
            crate::themes::archive::PreparedThemeImport::ResourcePackage(package) => {
                package.validated().root.clone()
            }
            _ => panic!("expected resource package"),
        };
        let catalog = ThemeCatalog::at(catalog_root.clone());

        let installed = catalog.import_prepared(prepared).unwrap();

        assert_eq!(installed.id, "installed");
        assert_eq!(installed.file_name, "installed");
        assert_eq!(installed.storage_kind, ThemeStorageKind::ResourceDirectory);
        assert!(!staging.exists());
        assert!(catalog_root.join("installed").is_dir());
        assert!(owned_artifacts(&catalog_root).is_empty());
        assert_eq!(catalog.find_descriptor("installed").unwrap(), installed);
    }

    #[test]
    fn install_revalidates_prepared_package_before_publication() {
        let temp = tempdir().unwrap();
        let catalog_root = temp.path().join("themes");
        fs::create_dir(&catalog_root).unwrap();
        let source_root = temp.path().join("source-package");
        let source_archive = temp.path().join("source.theme");
        write_package_archive(
            &source_root,
            &source_archive,
            &catalog_root,
            "changed",
            "Changed",
            "light",
            "1",
        );
        let prepared = prepare_external_theme(&source_archive, &catalog_root).unwrap();
        let staging = match &prepared {
            crate::themes::archive::PreparedThemeImport::ResourcePackage(package) => {
                package.validated().root.clone()
            }
            _ => panic!("expected resource package"),
        };
        fs::write(staging.join("theme.css"), b":root { --suffix: changed; }").unwrap();
        let catalog = ThemeCatalog::at(catalog_root.clone());

        assert_eq!(
            catalog.import_prepared(prepared).unwrap_err().code,
            ThemeErrorCode::FingerprintMismatch
        );
        assert!(!catalog_root.join("changed").exists());
        assert!(owned_artifacts(&catalog_root).is_empty());
    }

    #[test]
    fn replacement_moves_from_css_to_directory_with_fingerprint_check() {
        let temp = tempdir().unwrap();
        let catalog_root = temp.path().join("themes");
        let catalog = ThemeCatalog::at(catalog_root.clone());
        let original = catalog
            .import_bytes(&css("switch", "Switch", "light", "1"), "source.css")
            .unwrap();
        let source_root = temp.path().join("resource-source");
        let source_archive = temp.path().join("resource.theme");
        write_package_archive(
            &source_root,
            &source_archive,
            &catalog_root,
            "switch",
            "Switch",
            "dark",
            "2",
        );

        let wrong = prepare_external_theme(&source_archive, &catalog_root).unwrap();
        assert_eq!(
            catalog.replace_prepared(wrong, "wrong").unwrap_err().code,
            ThemeErrorCode::FingerprintMismatch
        );
        assert!(catalog_root.join("switch.css").is_file());

        let replacement = prepare_external_theme(&source_archive, &catalog_root).unwrap();
        let installed = catalog
            .replace_prepared(replacement, &original.fingerprint)
            .unwrap();

        assert_eq!(installed.storage_kind, ThemeStorageKind::ResourceDirectory);
        assert_eq!(installed.file_name, "switch");
        assert!(!catalog_root.join("switch.css").exists());
        assert!(catalog_root.join("switch").is_dir());
    }

    #[test]
    fn replacement_moves_from_directory_to_css_with_fingerprint_check() {
        let temp = tempdir().unwrap();
        let catalog_root = temp.path().join("themes");
        fs::create_dir(&catalog_root).unwrap();
        let source_root = temp.path().join("resource-source");
        let source_archive = temp.path().join("resource.theme");
        write_package_archive(
            &source_root,
            &source_archive,
            &catalog_root,
            "switch",
            "Switch",
            "light",
            "1",
        );
        let catalog = ThemeCatalog::at(catalog_root.clone());
        let installed = catalog
            .import_prepared(prepare_external_theme(&source_archive, &catalog_root).unwrap())
            .unwrap();
        let css_source = temp.path().join("replacement.css");
        fs::write(&css_source, css("switch", "Switch", "dark", "2")).unwrap();
        let replacement = prepare_external_theme(&css_source, &catalog_root).unwrap();

        let replaced = catalog
            .replace_prepared(replacement, &installed.fingerprint)
            .unwrap();

        assert_eq!(replaced.storage_kind, ThemeStorageKind::InlineCss);
        assert_eq!(replaced.file_name, "switch.css");
        assert!(!catalog_root.join("switch").exists());
        assert!(catalog_root.join("switch.css").is_file());
    }

    #[test]
    fn publication_failure_restores_exact_legacy_file() {
        let temp = tempdir().unwrap();
        let catalog_root = temp.path().join("themes");
        let catalog = ThemeCatalog::at(catalog_root.clone());
        let original_bytes = css("rollback", "Rollback", "light", "1");
        let original = catalog.import_bytes(&original_bytes, "source.css").unwrap();
        let source_root = temp.path().join("resource-source");
        let source_archive = temp.path().join("resource.theme");
        write_package_archive(
            &source_root,
            &source_archive,
            &catalog_root,
            "rollback",
            "Rollback",
            "dark",
            "2",
        );
        let prepared = prepare_external_theme(&source_archive, &catalog_root).unwrap();

        let error = catalog
            .replace_prepared_with_hook(prepared, &original.fingerprint, &mut |point| {
                if point == CatalogPublicationHookPoint::AfterBackup {
                    Err(ThemeError::new(
                        ThemeErrorCode::Io,
                        "injected publication failure",
                    ))
                } else {
                    Ok(())
                }
            })
            .unwrap_err();

        assert_eq!(error.code, ThemeErrorCode::Io);
        assert_eq!(
            fs::read(catalog_root.join("rollback.css")).unwrap(),
            original_bytes
        );
        assert!(!catalog_root.join("rollback").exists());
        assert!(owned_artifacts(&catalog_root).is_empty());
    }

    #[test]
    fn publication_failure_restores_exact_resource_directory() {
        let temp = tempdir().unwrap();
        let catalog_root = temp.path().join("themes");
        fs::create_dir(&catalog_root).unwrap();
        let source_root = temp.path().join("resource-source");
        let source_archive = temp.path().join("resource.theme");
        write_package_archive(
            &source_root,
            &source_archive,
            &catalog_root,
            "rollback",
            "Rollback",
            "light",
            "1",
        );
        let catalog = ThemeCatalog::at(catalog_root.clone());
        let original = catalog
            .import_prepared(prepare_external_theme(&source_archive, &catalog_root).unwrap())
            .unwrap();
        let original_bytes = directory_bytes(&catalog_root.join("rollback"));
        let css_source = temp.path().join("replacement.css");
        fs::write(&css_source, css("rollback", "Rollback", "dark", "2")).unwrap();
        let prepared = prepare_external_theme(&css_source, &catalog_root).unwrap();

        let error = catalog
            .replace_prepared_with_hook(prepared, &original.fingerprint, &mut |point| {
                if point == CatalogPublicationHookPoint::AfterBackup {
                    Err(ThemeError::new(
                        ThemeErrorCode::Io,
                        "injected publication failure",
                    ))
                } else {
                    Ok(())
                }
            })
            .unwrap_err();

        assert_eq!(error.code, ThemeErrorCode::Io);
        assert_eq!(
            directory_bytes(&catalog_root.join("rollback")),
            original_bytes
        );
        assert!(!catalog_root.join("rollback.css").exists());
        assert!(owned_artifacts(&catalog_root).is_empty());
    }

    #[test]
    fn delete_removes_legacy_files_and_resource_directories() {
        let temp = tempdir().unwrap();
        let catalog_root = temp.path().join("themes");
        let catalog = ThemeCatalog::at(catalog_root.clone());
        let legacy = catalog
            .import_bytes(&css("legacy", "Legacy", "light", "1"), "legacy.css")
            .unwrap();
        let source_root = temp.path().join("resource-source");
        let source_archive = temp.path().join("resource.theme");
        write_package_archive(
            &source_root,
            &source_archive,
            &catalog_root,
            "resource",
            "Resource",
            "dark",
            "2",
        );
        let resource = catalog
            .import_prepared(prepare_external_theme(&source_archive, &catalog_root).unwrap())
            .unwrap();

        catalog.delete("legacy", &legacy.fingerprint).unwrap();
        catalog.delete("resource", &resource.fingerprint).unwrap();

        assert!(!catalog_root.join("legacy.css").exists());
        assert!(!catalog_root.join("resource").exists());
        assert!(catalog.scan().unwrap().themes.is_empty());
    }

    #[test]
    fn refresh_never_rewrites_directly_placed_author_directories() {
        let temp = tempdir().unwrap();
        let author_root = temp.path().join("my-hand-authored-folder");
        write_package(
            &author_root,
            "author-theme",
            "Author Theme",
            "light",
            "author",
        );
        let before = directory_bytes(&author_root);
        let catalog = ThemeCatalog::at(temp.path().to_path_buf());

        let first = catalog.scan().unwrap();
        let second = catalog.scan().unwrap();

        assert_eq!(first, second);
        assert_eq!(first.themes[0].file_name, "my-hand-authored-folder");
        assert_eq!(directory_bytes(&author_root), before);
    }

    #[test]
    fn delete_never_removes_legacy_bytes_changed_after_validation() {
        let temp = tempdir().unwrap();
        let catalog_root = temp.path().join("themes");
        let catalog = ThemeCatalog::at(catalog_root.clone());
        let original = catalog
            .import_bytes(
                &css("delete-race", "Delete Race", "light", "one"),
                "source.css",
            )
            .unwrap();
        let newer = css("delete-race", "Delete Race", "light", "newer");

        let error = catalog
            .delete_with_hook("delete-race", &original.fingerprint, &mut |point| {
                assert_eq!(point, DeleteHookPoint::AfterValidation);
                fs::write(catalog_root.join("delete-race.css"), &newer).unwrap();
            })
            .unwrap_err();

        assert_eq!(error.code, ThemeErrorCode::FingerprintMismatch);
        assert_eq!(
            fs::read(catalog_root.join("delete-race.css")).unwrap(),
            newer
        );
        assert!(owned_artifacts(&catalog_root).is_empty());
    }

    #[test]
    fn delete_never_removes_resource_tree_changed_after_validation() {
        let temp = tempdir().unwrap();
        let catalog_root = temp.path().join("themes");
        fs::create_dir(&catalog_root).unwrap();
        let source_root = temp.path().join("resource-source");
        let source_archive = temp.path().join("resource.theme");
        write_package_archive(
            &source_root,
            &source_archive,
            &catalog_root,
            "delete-race",
            "Delete Race",
            "light",
            "one",
        );
        let catalog = ThemeCatalog::at(catalog_root.clone());
        let original = catalog
            .import_prepared(prepare_external_theme(&source_archive, &catalog_root).unwrap())
            .unwrap();
        let newer = b":root { --suffix: newer; }\n";

        let error = catalog
            .delete_with_hook("delete-race", &original.fingerprint, &mut |point| {
                assert_eq!(point, DeleteHookPoint::AfterValidation);
                fs::write(catalog_root.join("delete-race/theme.css"), newer).unwrap();
            })
            .unwrap_err();

        assert_eq!(error.code, ThemeErrorCode::FingerprintMismatch);
        assert_eq!(
            fs::read(catalog_root.join("delete-race/theme.css")).unwrap(),
            newer
        );
        assert!(owned_artifacts(&catalog_root).is_empty());
    }

    #[test]
    fn prepared_package_directory_substitution_is_rejected_and_cleaned() {
        let temp = tempdir().unwrap();
        let catalog_root = temp.path().join("themes");
        fs::create_dir(&catalog_root).unwrap();
        let source_root = temp.path().join("resource-source");
        let source_archive = temp.path().join("resource.theme");
        write_package_archive(
            &source_root,
            &source_archive,
            &catalog_root,
            "prepared-race",
            "Prepared Race",
            "light",
            "one",
        );
        let prepared = prepare_external_theme(&source_archive, &catalog_root).unwrap();
        let staging = match &prepared {
            crate::themes::archive::PreparedThemeImport::ResourcePackage(package) => {
                package.validated().root.clone()
            }
            _ => panic!("expected resource package"),
        };
        let displaced = catalog_root.join("displaced-prepared");
        let catalog = ThemeCatalog::at(catalog_root.clone());

        let error = catalog
            .import_prepared_with_hook(prepared, &mut |point| {
                assert_eq!(point, CatalogPublicationHookPoint::BeforePublication);
                fs::rename(&staging, &displaced).unwrap();
                fs::create_dir(&staging).unwrap();
                Ok(())
            })
            .unwrap_err();

        assert_eq!(error.code, ThemeErrorCode::UnsafePath);
        assert!(!catalog_root.join("prepared-race").exists());
        assert!(!staging.exists());
        assert!(!displaced.exists());
        assert!(owned_artifacts(&catalog_root).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn prepared_package_symlink_substitution_is_rejected_without_following_or_residue() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let catalog_root = temp.path().join("themes");
        fs::create_dir(&catalog_root).unwrap();
        let source_root = temp.path().join("resource-source");
        let source_archive = temp.path().join("resource.theme");
        write_package_archive(
            &source_root,
            &source_archive,
            &catalog_root,
            "prepared-link-race",
            "Prepared Link Race",
            "light",
            "one",
        );
        let prepared = prepare_external_theme(&source_archive, &catalog_root).unwrap();
        let staging = match &prepared {
            crate::themes::archive::PreparedThemeImport::ResourcePackage(package) => {
                package.validated().root.clone()
            }
            _ => panic!("expected resource package"),
        };
        let displaced = catalog_root.join("displaced-prepared");
        let outside = temp.path().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("keep.txt"), b"outside").unwrap();
        let catalog = ThemeCatalog::at(catalog_root.clone());

        let error = catalog
            .import_prepared_with_hook(prepared, &mut |point| {
                assert_eq!(point, CatalogPublicationHookPoint::BeforePublication);
                fs::rename(&staging, &displaced).unwrap();
                symlink(&outside, &staging).unwrap();
                Ok(())
            })
            .unwrap_err();

        assert_eq!(error.code, ThemeErrorCode::UnsafePath);
        assert_eq!(fs::read(outside.join("keep.txt")).unwrap(), b"outside");
        assert!(!catalog_root.join("prepared-link-race").exists());
        assert!(!staging.exists());
        assert!(!displaced.exists());
        assert!(owned_artifacts(&catalog_root).is_empty());
    }

    #[test]
    fn root_substitution_is_rejected_between_validation_and_catalog_rename() {
        let temp = tempdir().unwrap();
        let catalog_root = temp.path().join("themes");
        fs::create_dir(&catalog_root).unwrap();
        fs::write(catalog_root.join("source.css"), b"source").unwrap();
        let moved_root = temp.path().join("moved-themes");
        let catalog = ThemeCatalog::at(catalog_root.clone());
        let directory = catalog.catalog_directory().unwrap();

        let error = rename_catalog_noreplace_with_hook(
            &directory,
            "source.css",
            "target.css",
            &catalog_root.join("source.css"),
            &catalog_root.join("target.css"),
            &mut |point| {
                assert_eq!(point, CatalogRenameHookPoint::AfterRootValidation);
                fs::rename(&catalog_root, &moved_root).unwrap();
                fs::create_dir(&catalog_root).unwrap();
            },
        )
        .unwrap_err();

        assert_eq!(error.code, ThemeErrorCode::UnsafePath);
        assert!(moved_root.join("source.css").is_file());
        assert!(!moved_root.join("target.css").exists());
        assert!(!catalog_root.join("target.css").exists());
        fs::remove_dir(&catalog_root).unwrap();
        fs::rename(&moved_root, &catalog_root).unwrap();
    }

    #[test]
    fn after_publication_failure_restores_exact_legacy_file() {
        let temp = tempdir().unwrap();
        let catalog_root = temp.path().join("themes");
        let catalog = ThemeCatalog::at(catalog_root.clone());
        let original_bytes = css("post-rollback", "Post Rollback", "light", "one");
        let original = catalog.import_bytes(&original_bytes, "source.css").unwrap();
        let source_root = temp.path().join("resource-source");
        let source_archive = temp.path().join("resource.theme");
        write_package_archive(
            &source_root,
            &source_archive,
            &catalog_root,
            "post-rollback",
            "Post Rollback",
            "dark",
            "two",
        );
        let prepared = prepare_external_theme(&source_archive, &catalog_root).unwrap();

        let error = catalog
            .replace_prepared_with_hook(prepared, &original.fingerprint, &mut |point| match point {
                CatalogPublicationHookPoint::AfterPublication => Err(ThemeError::new(
                    ThemeErrorCode::Io,
                    "injected post-publication failure",
                )),
                _ => Ok(()),
            })
            .unwrap_err();

        assert_eq!(error.code, ThemeErrorCode::Io);
        assert_eq!(
            fs::read(catalog_root.join("post-rollback.css")).unwrap(),
            original_bytes
        );
        assert!(!catalog_root.join("post-rollback").exists());
        assert!(owned_artifacts(&catalog_root).is_empty());
    }

    #[test]
    fn after_publication_failure_restores_exact_resource_directory() {
        let temp = tempdir().unwrap();
        let catalog_root = temp.path().join("themes");
        fs::create_dir(&catalog_root).unwrap();
        let source_root = temp.path().join("resource-source");
        let source_archive = temp.path().join("resource.theme");
        write_package_archive(
            &source_root,
            &source_archive,
            &catalog_root,
            "post-rollback",
            "Post Rollback",
            "light",
            "one",
        );
        let catalog = ThemeCatalog::at(catalog_root.clone());
        let original = catalog
            .import_prepared(prepare_external_theme(&source_archive, &catalog_root).unwrap())
            .unwrap();
        let original_bytes = directory_bytes(&catalog_root.join("post-rollback"));
        let css_source = temp.path().join("replacement.css");
        fs::write(
            &css_source,
            css("post-rollback", "Post Rollback", "dark", "two"),
        )
        .unwrap();
        let prepared = prepare_external_theme(&css_source, &catalog_root).unwrap();

        let error = catalog
            .replace_prepared_with_hook(prepared, &original.fingerprint, &mut |point| match point {
                CatalogPublicationHookPoint::AfterPublication => Err(ThemeError::new(
                    ThemeErrorCode::Io,
                    "injected post-publication failure",
                )),
                _ => Ok(()),
            })
            .unwrap_err();

        assert_eq!(error.code, ThemeErrorCode::Io);
        assert_eq!(
            directory_bytes(&catalog_root.join("post-rollback")),
            original_bytes
        );
        assert!(!catalog_root.join("post-rollback.css").exists());
        assert!(owned_artifacts(&catalog_root).is_empty());
    }

    #[test]
    fn scan_invalidates_all_duplicate_ids_and_keeps_other_files() {
        let temp = tempdir().unwrap();
        fs::write(
            temp.path().join("first.css"),
            css("same", "First", "light", "1"),
        )
        .unwrap();
        fs::write(
            temp.path().join("second.css"),
            css("same", "Second", "light", "2"),
        )
        .unwrap();
        fs::write(
            temp.path().join("valid.css"),
            css("valid", "Valid", "dark", "3"),
        )
        .unwrap();

        let snapshot = ThemeCatalog::at(temp.path().to_path_buf()).scan().unwrap();

        assert_eq!(snapshot.themes.len(), 1);
        assert_eq!(snapshot.themes[0].id, "valid");
        assert_eq!(snapshot.invalid_files.len(), 2);
    }

    #[test]
    fn read_replace_and_delete_require_the_current_fingerprint() {
        let temp = tempdir().unwrap();
        let catalog = ThemeCatalog::at(temp.path().to_path_buf());
        let original = catalog
            .import_bytes(&css("safe", "Safe", "light", "1"), "source.css")
            .unwrap();

        let payload = catalog.read_css("safe", &original.fingerprint).unwrap();
        assert_eq!(payload.id, "safe");
        assert!(payload.css.contains("--suffix: 1"));

        fs::write(
            temp.path().join("safe.css"),
            css("safe", "Safe", "light", "external"),
        )
        .unwrap();
        assert_eq!(
            catalog
                .replace_prepared(
                    crate::themes::archive::PreparedThemeImport::LegacyCss(
                        crate::themes::parser::parse_theme_file(
                            &css("safe", "Safe", "light", "2"),
                            "replacement.css",
                        )
                        .unwrap(),
                    ),
                    &original.fingerprint,
                )
                .unwrap_err()
                .code,
            ThemeErrorCode::FingerprintMismatch
        );
        assert_eq!(
            catalog
                .delete("safe", &original.fingerprint)
                .unwrap_err()
                .code,
            ThemeErrorCode::FingerprintMismatch
        );
    }

    #[cfg(unix)]
    #[test]
    fn scan_rejects_symlinked_files_and_catalog_roots() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let outside = temp.path().join("outside.css");
        fs::write(&outside, css("outside", "Outside", "light", "1")).unwrap();
        let themes = temp.path().join("themes");
        fs::create_dir(&themes).unwrap();
        symlink(&outside, themes.join("linked.css")).unwrap();

        let snapshot = ThemeCatalog::at(themes.clone()).scan().unwrap();
        assert!(snapshot.themes.is_empty());
        assert_eq!(snapshot.invalid_files.len(), 1);

        let linked_root = temp.path().join("linked-root");
        symlink(&themes, &linked_root).unwrap();
        assert_eq!(
            ThemeCatalog::at(linked_root).scan().unwrap_err().code,
            ThemeErrorCode::UnsafePath
        );
    }

    #[test]
    fn protected_defaults_cannot_be_deleted() {
        let catalog = ThemeCatalog::at(tempdir().unwrap().path().to_path_buf());
        assert_eq!(
            catalog.delete("light", "default:light").unwrap_err().code,
            ThemeErrorCode::ProtectedTheme
        );
    }

    #[test]
    fn seeds_eighteen_themes_idempotently_and_allows_deleting_third_party() {
        let temp = tempdir().unwrap();
        let catalog = ThemeCatalog::at(temp.path().join("themes"));

        catalog.seed_missing().unwrap();
        assert_eq!(catalog.scan().unwrap().themes.len(), 18);

        catalog
            .delete(
                "nord",
                &catalog.find_descriptor("nord").unwrap().fingerprint,
            )
            .unwrap();
        assert_eq!(catalog.scan().unwrap().themes.len(), 17);
    }
}
