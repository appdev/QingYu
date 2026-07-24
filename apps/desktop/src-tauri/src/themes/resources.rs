use std::{
    collections::BTreeSet,
    io::Read,
    path::{Path, PathBuf},
};

use cap_fs_ext::{DirExt, FollowSymlinks, MetadataExt, OpenOptionsExt, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use icu_casemap::CaseMapper;
use quick_xml::{events::Event, Reader};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

use super::{
    manifest::{parse_theme_manifest, ThemeManifest},
    parser::{validate_package_css, validate_svg_css, MAX_THEME_BYTES},
    ThemeDescriptor, ThemeError, ThemeErrorCode, ThemeStorageKind,
};

pub(crate) const MAX_PACKAGE_BYTES: u64 = 32 * 1024 * 1024;
pub(crate) const MAX_PACKAGE_ENTRIES: usize = 256;
const MAX_PATH_CHARS: usize = 240;
const MAX_PATH_DEPTH: usize = 16;
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_FONT_BYTES: u64 = 4 * 1024 * 1024;
const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;
const FINGERPRINT_VERSION: &[u8] = b"markra-resource-theme-v1\0";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ValidatedThemeFileKind {
    Manifest,
    Stylesheet,
    Asset,
    License,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ValidatedThemeFile {
    pub(crate) relative_path: String,
    pub(crate) bytes: Vec<u8>,
    pub(crate) kind: ValidatedThemeFileKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ValidatedThemeDirectory {
    pub(crate) descriptor: ThemeDescriptor,
    pub(crate) root: PathBuf,
    pub(crate) files: Vec<ValidatedThemeFile>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResourceKind {
    Font,
    Gif,
    Jpeg,
    Png,
    Svg,
    Webp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WalkedFileKind {
    Manifest,
    Stylesheet,
    Resource(ResourceKind),
    License,
}

impl WalkedFileKind {
    fn max_bytes(self) -> u64 {
        match self {
            Self::Manifest => MAX_MANIFEST_BYTES,
            Self::Stylesheet => MAX_THEME_BYTES as u64,
            Self::Resource(ResourceKind::Font) => MAX_FONT_BYTES,
            Self::Resource(_) => MAX_IMAGE_BYTES,
            Self::License => MAX_PACKAGE_BYTES,
        }
    }

    fn validated_kind(self) -> ValidatedThemeFileKind {
        match self {
            Self::Manifest => ValidatedThemeFileKind::Manifest,
            Self::Stylesheet => ValidatedThemeFileKind::Stylesheet,
            Self::Resource(_) => ValidatedThemeFileKind::Asset,
            Self::License => ValidatedThemeFileKind::License,
        }
    }
}

struct WalkedFile {
    parent: Dir,
    name: PathBuf,
    relative_path: String,
    kind: WalkedFileKind,
    identity: FileIdentity,
}

struct WalkedDirectory {
    relative_path: PathBuf,
    identity: FileIdentity,
}

struct WalkedPackage {
    directories: Vec<WalkedDirectory>,
    files: Vec<WalkedFile>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileIdentity {
    device: u64,
    inode: u64,
}

fn file_identity<T: MetadataExt>(metadata: &T) -> FileIdentity {
    FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ValidationOpenKind {
    FileAfterAddressedValidation,
}

fn validate_theme_directory_with_hook(
    root: &Path,
    storage_name: &str,
    before_open: &mut dyn FnMut(&str, ValidationOpenKind),
) -> Result<ValidatedThemeDirectory, ThemeError> {
    let (root_directory, root_identity) = open_root_nofollow(root)?;
    validate_open_theme_directory(
        root,
        storage_name,
        &root_directory,
        root_identity,
        before_open,
    )
}

fn validate_open_theme_directory(
    root: &Path,
    storage_name: &str,
    root_directory: &Dir,
    root_identity: FileIdentity,
    before_open: &mut dyn FnMut(&str, ValidationOpenKind),
) -> Result<ValidatedThemeDirectory, ThemeError> {
    let walked = walk_package(&root_directory)?;

    let mut actual_total = 0_u64;
    let mut files = Vec::with_capacity(walked.files.len());
    for walked_file in &walked.files {
        let bytes = read_bounded_file(walked_file, &mut actual_total, before_open)?;
        validate_file_content(walked_file.kind, &bytes)?;
        files.push(ValidatedThemeFile {
            relative_path: walked_file.relative_path.clone(),
            bytes,
            kind: walked_file.kind.validated_kind(),
        });
    }

    revalidate_directories(&root_directory, &walked.directories)?;
    revalidate_root(root, root_identity)?;

    let manifest_file = required_file(&files, "manifest.json", ThemeErrorCode::InvalidManifest)?;
    let manifest = parse_theme_manifest(&manifest_file.bytes)?;
    let stylesheet = required_file(&files, "theme.css", ThemeErrorCode::InvalidManifest)?;
    let css = validate_package_css(&stylesheet.bytes)?;
    validate_css_assets(&css.referenced_assets, &files)?;
    validate_licenses(&manifest, &files)?;

    let fingerprint = package_fingerprint(&manifest, &files)?;
    let descriptor = ThemeDescriptor {
        appearance: manifest.appearance,
        author: manifest.author.clone(),
        file_name: storage_name.to_string(),
        fingerprint,
        id: manifest.id.clone(),
        name: manifest.name.clone(),
        preview: manifest.preview.clone(),
        source: "third-party".to_string(),
        storage_kind: ThemeStorageKind::ResourceDirectory,
        version: manifest.version.clone(),
    };

    Ok(ValidatedThemeDirectory {
        descriptor,
        root: root.to_path_buf(),
        files,
    })
}

pub(crate) fn validate_theme_directory(
    root: &Path,
    storage_name: &str,
) -> Result<ValidatedThemeDirectory, ThemeError> {
    validate_theme_directory_with_hook(root, storage_name, &mut |_, _| {})
}

pub(crate) fn validate_theme_directory_from_retained(
    root: &Path,
    storage_name: &str,
    root_directory: &Dir,
) -> Result<ValidatedThemeDirectory, ThemeError> {
    let retained = root_directory
        .dir_metadata()
        .map_err(|_| unsafe_path("Theme package root metadata is unavailable."))?;
    if !retained.is_dir() {
        return Err(unsafe_path(
            "Theme package roots must remain regular directories.",
        ));
    }
    let root_identity = file_identity(&retained);
    revalidate_root(root, root_identity)?;
    validate_open_theme_directory(
        root,
        storage_name,
        root_directory,
        root_identity,
        &mut |_, _| {},
    )
}

fn open_root_nofollow(root: &Path) -> Result<(Dir, FileIdentity), ThemeError> {
    let addressed = crate::storage_capability::ambient_symlink_metadata(root).map_err(|error| {
        ThemeError::new(
            ThemeErrorCode::Io,
            format!("Could not inspect the theme package directory: {error}"),
        )
    })?;
    if addressed.file_type().is_symlink() || !addressed.file_type().is_dir() {
        return Err(unsafe_path(
            "Theme package roots must be regular directories and cannot be symbolic links.",
        ));
    }

    let directory = match (root.parent(), root.file_name()) {
        (Some(parent), Some(name)) => {
            let parent = Dir::open_ambient_dir(parent, cap_std::ambient_authority())
                .map_err(|_| unsafe_path("Theme package root parent is unavailable."))?;
            parent
                .open_dir_nofollow(name)
                .map_err(|_| unsafe_path("Theme package root changed before it could be opened."))?
        }
        _ => Dir::open_ambient_dir(root, cap_std::ambient_authority())
            .map_err(|_| unsafe_path("Theme package root is unavailable."))?,
    };
    let retained = directory
        .dir_metadata()
        .map_err(|_| unsafe_path("Theme package root metadata is unavailable."))?;
    if !retained.is_dir() || file_identity(&addressed) != file_identity(&retained) {
        return Err(unsafe_path(
            "Theme package root changed while it was being opened.",
        ));
    }
    Ok((directory, file_identity(&retained)))
}

fn walk_package(root: &Dir) -> Result<WalkedPackage, ThemeError> {
    let root = root
        .try_clone()
        .map_err(|_| unsafe_path("Theme package root could not be retained."))?;
    let mut pending = vec![(root, PathBuf::new())];
    let mut entries = 0_usize;
    let mut metadata_total = 0_u64;
    let mut normalized_paths = Vec::new();
    let mut directories = Vec::new();
    let mut files = Vec::new();

    while let Some((directory, relative_directory)) = pending.pop() {
        let children = directory.entries().map_err(|error| {
            ThemeError::new(
                ThemeErrorCode::Io,
                format!("Could not read the theme package directory: {error}"),
            )
        })?;
        for child in children {
            let child = child.map_err(|error| {
                ThemeError::new(
                    ThemeErrorCode::Io,
                    format!("Could not read a theme package entry: {error}"),
                )
            })?;
            entries = entries.checked_add(1).ok_or_else(package_too_large)?;
            if entries > MAX_PACKAGE_ENTRIES {
                return Err(package_too_large());
            }

            let name = child.file_name();
            let name = name
                .to_str()
                .ok_or_else(|| unsafe_path("Theme package paths must use valid UTF-8 names."))?;
            if name.is_empty() || name.contains(['\0', '\\', '/']) || matches!(name, "." | "..") {
                return Err(unsafe_path(
                    "Theme package paths contain an unsafe segment.",
                ));
            }

            let relative_path = relative_directory.join(name);
            let normalized_path = normalize_relative_path(&relative_path)?;
            normalized_paths.push(normalized_path.clone());

            let metadata = directory.symlink_metadata(name).map_err(|error| {
                ThemeError::new(
                    ThemeErrorCode::Io,
                    format!("Could not inspect a theme package entry: {error}"),
                )
            })?;
            let file_type = metadata.file_type();
            if file_type.is_symlink() {
                return Err(unsafe_path("Theme packages cannot contain symbolic links."));
            }
            if file_type.is_dir() {
                validate_directory_path(&normalized_path)?;
                let child_directory = directory
                    .open_dir_nofollow(name)
                    .map_err(|_| unsafe_path("Theme package directory changed before opening."))?;
                let retained = child_directory
                    .dir_metadata()
                    .map_err(|_| unsafe_path("Theme package directory metadata is unavailable."))?;
                if !retained.is_dir() || file_identity(&metadata) != file_identity(&retained) {
                    return Err(unsafe_path(
                        "Theme package directory changed while it was being opened.",
                    ));
                }
                directories.push(WalkedDirectory {
                    relative_path: relative_path.clone(),
                    identity: file_identity(&retained),
                });
                pending.push((child_directory, relative_path));
                continue;
            }
            if !file_type.is_file() {
                return Err(unsafe_path(
                    "Theme packages can contain only regular files and directories.",
                ));
            }

            let kind = classify_file(&normalized_path)?;
            if metadata.len() > kind.max_bytes() {
                return Err(package_too_large());
            }
            metadata_total = metadata_total
                .checked_add(metadata.len())
                .ok_or_else(package_too_large)?;
            if metadata_total > MAX_PACKAGE_BYTES {
                return Err(package_too_large());
            }
            let parent = directory
                .try_clone()
                .map_err(|_| unsafe_path("Theme package file parent could not be retained."))?;
            files.push(WalkedFile {
                parent,
                name: PathBuf::from(name),
                relative_path: normalized_path,
                kind,
                identity: file_identity(&metadata),
            });
        }
    }

    validate_path_aliases(&normalized_paths)?;
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(WalkedPackage { directories, files })
}

pub(crate) fn normalize_relative_path(path: &Path) -> Result<String, ThemeError> {
    let mut segments = Vec::new();
    for component in path.components() {
        let std::path::Component::Normal(segment) = component else {
            return Err(unsafe_path(
                "Theme package paths must be relative and normalized.",
            ));
        };
        let segment = segment
            .to_str()
            .ok_or_else(|| unsafe_path("Theme package paths must use valid UTF-8 names."))?;
        if segment.is_empty()
            || segment.contains(['\0', '\\', '/'])
            || matches!(segment, "." | "..")
        {
            return Err(unsafe_path(
                "Theme package paths contain an unsafe segment.",
            ));
        }
        segments.push(segment.nfc().collect::<String>());
    }
    if segments.is_empty() {
        return Err(unsafe_path("Theme package paths cannot be empty."));
    }
    if segments.len() > MAX_PATH_DEPTH {
        return Err(package_too_large());
    }
    let normalized = segments.join("/");
    if normalized.chars().count() > MAX_PATH_CHARS {
        return Err(package_too_large());
    }
    Ok(normalized)
}

fn validate_directory_path(path: &str) -> Result<(), ThemeError> {
    if path == "assets"
        || path.starts_with("assets/")
        || path == "licenses"
        || path.starts_with("licenses/")
    {
        return Ok(());
    }
    Err(unsafe_path(
        "Theme package directories must remain below assets/ or licenses/.",
    ))
}

fn classify_file(path: &str) -> Result<WalkedFileKind, ThemeError> {
    match path {
        "manifest.json" => return Ok(WalkedFileKind::Manifest),
        "theme.css" => return Ok(WalkedFileKind::Stylesheet),
        _ => {}
    }

    if let Some(name) = path.strip_prefix("assets/") {
        if name.is_empty() {
            return Err(unsafe_path("Theme package asset paths cannot be empty."));
        }
        let extension = name.rsplit_once('.').map(|(_, extension)| extension);
        let resource = match extension {
            Some("woff2") => ResourceKind::Font,
            Some("png") => ResourceKind::Png,
            Some("jpg" | "jpeg") => ResourceKind::Jpeg,
            Some("webp") => ResourceKind::Webp,
            Some("gif") => ResourceKind::Gif,
            Some("svg") => ResourceKind::Svg,
            _ => {
                return Err(unsafe_path(
                    "Theme packages contain an unsupported asset file.",
                ))
            }
        };
        return Ok(WalkedFileKind::Resource(resource));
    }

    if let Some(name) = path.strip_prefix("licenses/") {
        if matches!(name.rsplit_once('.'), Some((stem, "txt" | "md")) if !stem.is_empty()) {
            return Ok(WalkedFileKind::License);
        }
        return Err(unsafe_path(
            "Theme package licenses must use the .txt or .md extension.",
        ));
    }

    Err(unsafe_path(
        "Theme package files must be manifest.json, theme.css, assets, or licenses.",
    ))
}

pub(crate) fn validate_path_aliases(paths: &[String]) -> Result<(), ThemeError> {
    let mut exact = BTreeSet::new();
    let mut case_folded = BTreeSet::new();
    let case_mapper = CaseMapper::new();
    for path in paths {
        let normalized: String = path.nfc().collect();
        let folded = case_mapper.fold_string(&normalized).into_owned();
        if !exact.insert(normalized) || !case_folded.insert(folded) {
            return Err(unsafe_path(
                "Theme package paths contain Unicode or case-insensitive aliases.",
            ));
        }
    }
    Ok(())
}

pub(crate) fn archive_entry_limit(
    path: &str,
    is_directory: bool,
) -> Result<Option<u64>, ThemeError> {
    if is_directory {
        validate_directory_path(path)?;
        Ok(None)
    } else {
        classify_file(path).map(|kind| Some(kind.max_bytes()))
    }
}

fn nonfollowing_read_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    #[cfg(unix)]
    options.custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32);
    options
}

fn read_bounded_file(
    walked: &WalkedFile,
    actual_total: &mut u64,
    before_open: &mut dyn FnMut(&str, ValidationOpenKind),
) -> Result<Vec<u8>, ThemeError> {
    let addressed = walked
        .parent
        .symlink_metadata(&walked.name)
        .map_err(|_| unsafe_path("Theme package file changed before it could be opened."))?;
    if addressed.file_type().is_symlink()
        || !addressed.is_file()
        || file_identity(&addressed) != walked.identity
    {
        return Err(unsafe_path(
            "Theme package file changed before it could be opened safely.",
        ));
    }
    let file_limit = walked.kind.max_bytes();
    if addressed.len() > file_limit {
        return Err(package_too_large());
    }

    before_open(
        &walked.relative_path,
        ValidationOpenKind::FileAfterAddressedValidation,
    );
    let mut file = walked
        .parent
        .open_with(&walked.name, &nonfollowing_read_options())
        .map_err(|_| {
            unsafe_path("Theme package file could not be opened without following links.")
        })?;
    let retained = file
        .metadata()
        .map_err(|_| unsafe_path("Theme package file metadata is unavailable."))?;
    if !retained.is_file()
        || file_identity(&retained) != walked.identity
        || file_identity(&addressed) != file_identity(&retained)
    {
        return Err(unsafe_path(
            "Theme package file changed while it was being opened.",
        ));
    }

    let bytes = read_bounded_stream(&mut file, file_limit, actual_total)?;
    let retained_after_read = file
        .metadata()
        .map_err(|_| unsafe_path("Theme package file metadata is unavailable after reading."))?;
    if !retained_after_read.is_file()
        || file_identity(&retained_after_read) != walked.identity
        || retained_after_read.len() != bytes.len() as u64
    {
        return Err(unsafe_path(
            "Theme package file changed while it was being read.",
        ));
    }
    let addressed_after_read = walked
        .parent
        .symlink_metadata(&walked.name)
        .map_err(|_| unsafe_path("Theme package file changed after it was read."))?;
    if addressed_after_read.file_type().is_symlink()
        || !addressed_after_read.is_file()
        || file_identity(&addressed_after_read) != walked.identity
    {
        return Err(unsafe_path(
            "Theme package file path changed while it was being read.",
        ));
    }
    Ok(bytes)
}

fn open_relative_directory_nofollow(root: &Dir, path: &Path) -> Result<Dir, ThemeError> {
    let mut current = root
        .try_clone()
        .map_err(|_| unsafe_path("Theme package root could not be retained."))?;
    for component in path.components() {
        let std::path::Component::Normal(segment) = component else {
            return Err(unsafe_path(
                "Theme package directory path is no longer normalized.",
            ));
        };
        current = current
            .open_dir_nofollow(segment)
            .map_err(|_| unsafe_path("Theme package directory changed during validation."))?;
    }
    Ok(current)
}

fn revalidate_directories(root: &Dir, directories: &[WalkedDirectory]) -> Result<(), ThemeError> {
    for expected in directories {
        let retained = open_relative_directory_nofollow(root, &expected.relative_path)?;
        let metadata = retained.dir_metadata().map_err(|_| {
            unsafe_path("Theme package directory metadata is unavailable after validation.")
        })?;
        if !metadata.is_dir() || file_identity(&metadata) != expected.identity {
            return Err(unsafe_path(
                "Theme package directory changed during validation.",
            ));
        }
    }
    Ok(())
}

fn revalidate_root(root: &Path, expected: FileIdentity) -> Result<(), ThemeError> {
    let (retained, identity) = open_root_nofollow(root)?;
    if identity != expected {
        return Err(unsafe_path("Theme package root changed during validation."));
    }
    drop(retained);
    Ok(())
}

fn read_bounded_stream(
    mut reader: impl Read,
    file_limit: u64,
    actual_total: &mut u64,
) -> Result<Vec<u8>, ThemeError> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = reader.read(&mut buffer).map_err(|error| {
            ThemeError::new(
                ThemeErrorCode::Io,
                format!("Could not read a theme package file: {error}"),
            )
        })?;
        if read == 0 {
            break;
        }
        let next_file_len = (bytes.len() as u64)
            .checked_add(read as u64)
            .ok_or_else(package_too_large)?;
        let next_total = actual_total
            .checked_add(read as u64)
            .ok_or_else(package_too_large)?;
        if next_file_len > file_limit || next_total > MAX_PACKAGE_BYTES {
            return Err(package_too_large());
        }
        bytes.extend_from_slice(&buffer[..read]);
        *actual_total = next_total;
    }
    Ok(bytes)
}

fn validate_file_content(kind: WalkedFileKind, bytes: &[u8]) -> Result<(), ThemeError> {
    match kind {
        WalkedFileKind::Manifest | WalkedFileKind::Stylesheet => Ok(()),
        WalkedFileKind::License => std::str::from_utf8(bytes).map(|_| ()).map_err(|_| {
            ThemeError::new(
                ThemeErrorCode::InvalidUtf8,
                "Theme package license files must use UTF-8 encoding.",
            )
        }),
        WalkedFileKind::Resource(resource) => validate_resource(resource, bytes),
    }
}

fn validate_resource(resource: ResourceKind, bytes: &[u8]) -> Result<(), ThemeError> {
    let valid = match resource {
        ResourceKind::Font => bytes.starts_with(b"wOF2"),
        ResourceKind::Png => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        ResourceKind::Jpeg => bytes.starts_with(b"\xff\xd8\xff"),
        ResourceKind::Webp => {
            bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP"
        }
        ResourceKind::Gif => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        ResourceKind::Svg => return validate_svg(bytes),
    };
    if valid {
        Ok(())
    } else {
        Err(unsafe_resource(
            "Theme package resource contents do not match the declared file type.",
        ))
    }
}

fn validate_svg(bytes: &[u8]) -> Result<(), ThemeError> {
    let svg = std::str::from_utf8(bytes)
        .map_err(|_| unsafe_resource("Theme package SVG files must use UTF-8 XML encoding."))?;
    let mut reader = Reader::from_str(svg);
    reader.config_mut().check_comments = true;
    let mut depth = 0_usize;
    let mut root_seen = false;
    let mut root_closed = false;
    let mut style_depth = None;
    let mut style_css = String::new();

    loop {
        let event = reader.read_event().map_err(|_| {
            unsafe_resource("Theme package SVG files must contain well-formed safe XML.")
        })?;
        match event {
            Event::Start(element) => {
                if root_closed || (depth == 0 && root_seen) {
                    return Err(unsafe_resource(
                        "Theme package SVG files must contain exactly one SVG root.",
                    ));
                }
                let name = validate_svg_element(&element, reader.decoder())?;
                if depth == 0 {
                    if name != "svg" {
                        return Err(unsafe_resource(
                            "Theme package SVG files must use an svg root element.",
                        ));
                    }
                    root_seen = true;
                }
                depth = depth
                    .checked_add(1)
                    .ok_or_else(|| unsafe_resource("Theme package SVG nesting is too deep."))?;
                if depth > 256 {
                    return Err(unsafe_resource("Theme package SVG nesting is too deep."));
                }
                if name == "style" {
                    if style_depth.is_some() {
                        return Err(unsafe_resource(
                            "Theme package SVG style elements cannot be nested.",
                        ));
                    }
                    style_depth = Some(depth);
                    style_css.clear();
                } else if style_depth.is_some() {
                    return Err(unsafe_resource(
                        "Theme package SVG style elements must contain only CSS text.",
                    ));
                }
            }
            Event::Empty(element) => {
                if root_closed || (depth == 0 && root_seen) {
                    return Err(unsafe_resource(
                        "Theme package SVG files must contain exactly one SVG root.",
                    ));
                }
                let name = validate_svg_element(&element, reader.decoder())?;
                if depth == 0 {
                    if name != "svg" {
                        return Err(unsafe_resource(
                            "Theme package SVG files must use an svg root element.",
                        ));
                    }
                    root_seen = true;
                    root_closed = true;
                }
                if name == "style" || style_depth.is_some() {
                    return Err(unsafe_resource(
                        "Theme package SVG style elements must contain valid CSS text.",
                    ));
                }
            }
            Event::End(element) => {
                if depth == 0 {
                    return Err(unsafe_resource(
                        "Theme package SVG files contain an unmatched end element.",
                    ));
                }
                let name = local_xml_name(element.name().as_ref())?;
                if style_depth == Some(depth) {
                    if name != "style" {
                        return Err(unsafe_resource(
                            "Theme package SVG style markup is malformed.",
                        ));
                    }
                    validate_svg_css(&style_css).map_err(|_| {
                        unsafe_resource("Theme package SVG contains unsafe or invalid CSS.")
                    })?;
                    style_depth = None;
                    style_css.clear();
                }
                depth -= 1;
                if depth == 0 {
                    root_closed = true;
                }
            }
            Event::Text(text) => {
                let content = text
                    .decode()
                    .map_err(|_| unsafe_resource("Theme package SVG text must use valid UTF-8."))?;
                if depth == 0 && !content.trim().is_empty() {
                    return Err(unsafe_resource(
                        "Theme package SVG files cannot contain text outside the SVG root.",
                    ));
                }
                if style_depth.is_some() {
                    style_css.push_str(&content);
                }
            }
            Event::CData(text) => {
                if depth == 0 {
                    return Err(unsafe_resource(
                        "Theme package SVG files cannot contain CDATA outside the SVG root.",
                    ));
                }
                if style_depth.is_some() {
                    let content = text.decode().map_err(|_| {
                        unsafe_resource("Theme package SVG CSS must use valid UTF-8 text.")
                    })?;
                    style_css.push_str(&content);
                }
            }
            Event::GeneralRef(reference) => {
                let reference = reference.decode().map_err(|_| {
                    unsafe_resource("Theme package SVG contains an invalid entity reference.")
                })?;
                if depth == 0 {
                    return Err(unsafe_resource(
                        "Theme package SVG files cannot contain entity references outside the SVG root.",
                    ));
                }
                if !is_predefined_or_numeric_entity(&reference) {
                    return Err(unsafe_resource(
                        "Theme package SVG contains a disallowed entity reference.",
                    ));
                }
                if style_depth.is_some() {
                    style_css.push('&');
                    style_css.push_str(&reference);
                    style_css.push(';');
                }
            }
            Event::Decl(_) if depth == 0 && !root_seen => {}
            Event::Decl(_) | Event::PI(_) | Event::DocType(_) => {
                return Err(unsafe_resource(
                    "Theme package SVG files cannot contain processing instructions, doctypes, or entity declarations.",
                ));
            }
            Event::Comment(_) => {}
            Event::Eof => break,
        }
    }

    if !root_seen || !root_closed || depth != 0 || style_depth.is_some() {
        return Err(unsafe_resource(
            "Theme package SVG files must contain one complete SVG root.",
        ));
    }
    Ok(())
}

fn validate_svg_element(
    element: &quick_xml::events::BytesStart<'_>,
    decoder: quick_xml::encoding::Decoder,
) -> Result<String, ThemeError> {
    let name = local_xml_name(element.name().as_ref())?;
    if is_active_svg_element(&name) {
        return Err(unsafe_resource(
            "Theme package SVG files cannot contain active content.",
        ));
    }

    for attribute in element.attributes().with_checks(true) {
        let attribute = attribute
            .map_err(|_| unsafe_resource("Theme package SVG files contain invalid attributes."))?;
        let attribute_name = local_xml_name(attribute.key.as_ref())?;
        if attribute_name == "base" {
            return Err(unsafe_resource(
                "Theme package SVG files cannot set an XML base URI.",
            ));
        }
        if attribute_name.starts_with("on") {
            return Err(unsafe_resource(
                "Theme package SVG files cannot contain event handler attributes.",
            ));
        }
        let value = attribute
            .decode_and_unescape_value(decoder)
            .map_err(|_| unsafe_resource("Theme package SVG attributes must be safe UTF-8."))?;
        if matches!(attribute_name.as_str(), "href" | "src") {
            let value = value.trim();
            if !value.starts_with('#') || value.len() == 1 {
                return Err(unsafe_resource(
                    "Theme package SVG links and references must be same-document fragments.",
                ));
            }
        }
        validate_svg_css(&value).map_err(|_| {
            unsafe_resource("Theme package SVG attributes contain unsafe CSS URLs.")
        })?;
    }
    Ok(name)
}

fn local_xml_name(bytes: &[u8]) -> Result<String, ThemeError> {
    let name = std::str::from_utf8(bytes)
        .map_err(|_| unsafe_resource("Theme package SVG names must use valid UTF-8."))?;
    let local = name.rsplit(':').next().unwrap_or(name);
    if local.is_empty() {
        return Err(unsafe_resource(
            "Theme package SVG contains an empty XML name.",
        ));
    }
    Ok(local.to_ascii_lowercase())
}

fn is_active_svg_element(name: &str) -> bool {
    matches!(
        name,
        "script"
            | "foreignobject"
            | "iframe"
            | "object"
            | "embed"
            | "audio"
            | "video"
            | "canvas"
            | "frame"
            | "frameset"
            | "base"
            | "link"
            | "meta"
            | "handler"
            | "listener"
            | "set"
            | "animate"
            | "animatemotion"
            | "animatetransform"
            | "discard"
    )
}

fn is_predefined_or_numeric_entity(reference: &str) -> bool {
    matches!(reference, "amp" | "lt" | "gt" | "apos" | "quot")
        || reference.strip_prefix('#').is_some_and(|digits| {
            !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
        })
        || reference.strip_prefix("#x").is_some_and(|digits| {
            !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
}

fn required_file<'a>(
    files: &'a [ValidatedThemeFile],
    path: &str,
    code: ThemeErrorCode,
) -> Result<&'a ValidatedThemeFile, ThemeError> {
    files
        .iter()
        .find(|file| file.relative_path == path)
        .ok_or_else(|| ThemeError::new(code, format!("Theme package is missing {path}.")))
}

fn validate_css_assets(
    referenced_assets: &BTreeSet<String>,
    files: &[ValidatedThemeFile],
) -> Result<(), ThemeError> {
    let assets = files
        .iter()
        .filter(|file| file.kind == ValidatedThemeFileKind::Asset)
        .map(|file| file.relative_path.as_str())
        .collect::<BTreeSet<_>>();
    if referenced_assets
        .iter()
        .any(|reference| !assets.contains(reference.as_str()))
    {
        return Err(unsafe_resource(
            "Theme CSS references a package asset that does not exist.",
        ));
    }
    Ok(())
}

fn validate_licenses(
    manifest: &ThemeManifest,
    files: &[ValidatedThemeFile],
) -> Result<(), ThemeError> {
    let licenses = files
        .iter()
        .filter(|file| file.kind == ValidatedThemeFileKind::License)
        .map(|file| file.relative_path.as_str())
        .collect::<BTreeSet<_>>();
    if manifest
        .license_files
        .iter()
        .any(|path| !licenses.contains(path.as_str()))
    {
        return Err(ThemeError::new(
            ThemeErrorCode::InvalidManifest,
            "Theme manifest declares a license file that does not exist.",
        ));
    }

    let has_font = files
        .iter()
        .any(|file| file.relative_path.ends_with(".woff2"));
    if has_font
        && (manifest.license_files.is_empty()
            || !manifest
                .license_files
                .iter()
                .any(|path| licenses.contains(path.as_str())))
    {
        return Err(ThemeError::new(
            ThemeErrorCode::InvalidManifest,
            "Theme packages containing fonts must declare at least one existing UTF-8 license file.",
        ));
    }
    Ok(())
}

fn package_fingerprint(
    manifest: &ThemeManifest,
    files: &[ValidatedThemeFile],
) -> Result<String, ThemeError> {
    let canonical_manifest = serde_json::to_vec(manifest).map_err(|_| {
        ThemeError::new(
            ThemeErrorCode::InvalidManifest,
            "Theme manifest could not be serialized canonically.",
        )
    })?;
    let mut hasher = Sha256::new();
    hasher.update(FINGERPRINT_VERSION);
    hash_part(&mut hasher, &canonical_manifest);
    for file in files {
        if file.kind == ValidatedThemeFileKind::Manifest {
            continue;
        }
        hash_part(&mut hasher, file.relative_path.as_bytes());
        hash_part(&mut hasher, &file.bytes);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn hash_part(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

fn unsafe_path(message: impl Into<String>) -> ThemeError {
    ThemeError::new(ThemeErrorCode::UnsafePath, message)
}

fn unsafe_resource(message: impl Into<String>) -> ThemeError {
    ThemeError::new(ThemeErrorCode::UnsafeResource, message)
}

fn package_too_large() -> ThemeError {
    ThemeError::new(
        ThemeErrorCode::ThemeTooLarge,
        "Theme packages exceed an entry, path, file, or 32 MiB aggregate limit.",
    )
}

#[cfg(test)]
mod tests {
    use std::{
        fs::{self, File, OpenOptions},
        io::{Cursor, Write},
        path::{Path, PathBuf},
    };

    use serde_json::{json, Value};
    use tempfile::TempDir;

    use super::{
        read_bounded_stream, validate_path_aliases, validate_theme_directory,
        validate_theme_directory_with_hook, ValidatedThemeFileKind, ValidationOpenKind,
        MAX_PACKAGE_BYTES,
    };
    use crate::themes::{ThemeErrorCode, ThemeStorageKind};

    const SAFE_SVG: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"><defs><linearGradient id="g"/></defs><rect fill="url(#g)"/><use xlink:href="#g"/></svg>"##;

    struct PackageFixture {
        directory: TempDir,
    }

    impl PackageFixture {
        fn minimal() -> Self {
            let fixture = Self {
                directory: tempfile::tempdir().unwrap(),
            };
            fixture.write_manifest(approved_manifest());
            fixture.write("theme.css", b":root { --theme-accent: #e95f59; }");
            fixture
        }

        fn root(&self) -> &Path {
            self.directory.path()
        }

        fn path(&self, relative_path: &str) -> PathBuf {
            self.root().join(relative_path)
        }

        fn write(&self, relative_path: &str, bytes: &[u8]) {
            let path = self.path(relative_path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, bytes).unwrap();
        }

        fn write_manifest(&self, manifest: Value) {
            self.write(
                "manifest.json",
                serde_json::to_vec_pretty(&manifest).unwrap().as_slice(),
            );
        }

        fn write_sized(&self, relative_path: &str, prefix: &[u8], length: u64) {
            let path = self.path(relative_path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            let mut file = File::create(path).unwrap();
            file.write_all(prefix).unwrap();
            file.set_len(length).unwrap();
        }

        fn validate(&self) -> Result<super::ValidatedThemeDirectory, crate::themes::ThemeError> {
            validate_theme_directory(self.root(), "fixture-theme")
        }
    }

    fn approved_manifest() -> Value {
        json!({
            "schemaVersion": 1,
            "id": "fixture-theme",
            "name": "Fixture Theme",
            "appearance": "light",
            "entry": "theme.css",
            "author": "QingYu",
            "version": "1.0.0",
            "preview": {
                "background": "#ffffff",
                "panel": "#f6f8fa",
                "text": "#333333",
                "accent": "#e95f59"
            }
        })
    }

    fn with_license(mut manifest: Value) -> Value {
        manifest["licenseFiles"] = json!(["licenses/LICENSE.txt"]);
        manifest
    }

    fn valid_woff2(marker: u8) -> Vec<u8> {
        [b"wOF2".as_slice(), &[marker; 16]].concat()
    }

    fn add_font_bundle(fixture: &PackageFixture) {
        fixture.write_manifest(with_license(approved_manifest()));
        fixture.write("licenses/LICENSE.txt", b"Font license\n");
        for (name, marker) in [
            ("Regular", 1),
            ("Bold", 2),
            ("Italic", 3),
            ("BoldItalic", 4),
        ] {
            fixture.write(
                &format!("assets/fonts/JetBrainsMono-{name}.woff2"),
                &valid_woff2(marker),
            );
        }
    }

    #[test]
    fn validates_minimal_package_and_builds_resource_descriptor() {
        let fixture = PackageFixture::minimal();

        let validated = fixture.validate().unwrap();

        assert_eq!(validated.root, fixture.root());
        assert_eq!(validated.descriptor.id, "fixture-theme");
        assert_eq!(validated.descriptor.file_name, "fixture-theme");
        assert_eq!(
            validated.descriptor.storage_kind,
            ThemeStorageKind::ResourceDirectory
        );
        assert_eq!(validated.descriptor.fingerprint.len(), 64);
        assert_eq!(
            validated
                .files
                .iter()
                .map(|file| file.relative_path.as_str())
                .collect::<Vec<_>>(),
            ["manifest.json", "theme.css"]
        );
    }

    #[test]
    fn accepts_four_woff2_files_and_declared_utf8_license() {
        let fixture = PackageFixture::minimal();
        add_font_bundle(&fixture);

        let validated = fixture.validate().unwrap();

        assert_eq!(
            validated
                .files
                .iter()
                .filter(|file| file.relative_path.ends_with(".woff2"))
                .count(),
            4
        );
        assert!(validated.files.iter().any(|file| {
            file.relative_path == "licenses/LICENSE.txt"
                && file.kind == ValidatedThemeFileKind::License
        }));
    }

    #[test]
    fn accepts_supported_raster_signatures_and_safe_svg() {
        let fixture = PackageFixture::minimal();
        fixture.write("assets/images/image.png", b"\x89PNG\r\n\x1a\nrest");
        fixture.write("assets/images/image.jpg", b"\xff\xd8\xff\xe0rest");
        fixture.write("assets/images/image.jpeg", b"\xff\xd8\xff\xe1rest");
        fixture.write("assets/images/image.webp", b"RIFF\x04\0\0\0WEBPrest");
        fixture.write("assets/images/image87.gif", b"GIF87arest");
        fixture.write("assets/images/image89.gif", b"GIF89arest");
        fixture.write("assets/icons/safe.svg", SAFE_SVG);

        let validated = fixture.validate().unwrap();

        assert_eq!(
            validated
                .files
                .iter()
                .filter(|file| file.kind == ValidatedThemeFileKind::Asset)
                .count(),
            7
        );
    }

    #[test]
    fn rejects_css_reference_when_normalized_asset_is_absent() {
        let fixture = PackageFixture::minimal();
        fixture.write(
            "theme.css",
            b":root { background: url('./assets/icons/Cafe%CC%81.svg'); }",
        );

        assert_eq!(
            fixture.validate().unwrap_err().code,
            ThemeErrorCode::UnsafeResource
        );
    }

    #[test]
    fn keeps_unreferenced_valid_assets_installed_and_fingerprinted() {
        let fixture = PackageFixture::minimal();
        fixture.write("assets/icons/unreferenced.svg", SAFE_SVG);
        let original = fixture.validate().unwrap();

        assert!(original
            .files
            .iter()
            .any(|file| file.relative_path == "assets/icons/unreferenced.svg"));

        fixture.write(
            "assets/icons/unreferenced.svg",
            br##"<svg xmlns="http://www.w3.org/2000/svg"><circle r="4"/></svg>"##,
        );
        assert_ne!(
            fixture.validate().unwrap().descriptor.fingerprint,
            original.descriptor.fingerprint
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_roots_files_and_directories() {
        use std::os::unix::fs::symlink;

        let real_root = PackageFixture::minimal();
        let link_parent = tempfile::tempdir().unwrap();
        let root_link = link_parent.path().join("root-link");
        symlink(real_root.root(), &root_link).unwrap();
        assert_eq!(
            validate_theme_directory(&root_link, "fixture-theme")
                .unwrap_err()
                .code,
            ThemeErrorCode::UnsafePath
        );

        let file_link = PackageFixture::minimal();
        let external_file = link_parent.path().join("icon.svg");
        fs::write(&external_file, SAFE_SVG).unwrap();
        fs::create_dir_all(file_link.path("assets/icons")).unwrap();
        symlink(&external_file, file_link.path("assets/icons/icon.svg")).unwrap();
        assert_eq!(
            file_link.validate().unwrap_err().code,
            ThemeErrorCode::UnsafePath
        );

        let directory_link = PackageFixture::minimal();
        let external_directory = link_parent.path().join("icons");
        fs::create_dir(&external_directory).unwrap();
        symlink(&external_directory, directory_link.path("assets")).unwrap();
        assert_eq!(
            directory_link.validate().unwrap_err().code,
            ThemeErrorCode::UnsafePath
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_leaf_symlink_replacement_between_scan_and_open() {
        use std::os::unix::fs::symlink;

        let fixture = PackageFixture::minimal();
        let external = tempfile::tempdir().unwrap();
        let external_css = external.path().join("external.css");
        fs::write(&external_css, b":root { --external: true; }").unwrap();
        let mut replaced = false;

        let error = validate_theme_directory_with_hook(
            fixture.root(),
            "fixture-theme",
            &mut |relative_path, kind| {
                if !replaced
                    && relative_path == "theme.css"
                    && kind == ValidationOpenKind::FileAfterAddressedValidation
                {
                    fs::remove_file(fixture.path("theme.css")).unwrap();
                    symlink(&external_css, fixture.path("theme.css")).unwrap();
                    replaced = true;
                }
            },
        )
        .unwrap_err();

        assert!(replaced);
        assert_eq!(error.code, ThemeErrorCode::UnsafePath);
        assert_eq!(
            error.message,
            "Theme package file could not be opened without following links."
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_parent_replacement_during_descendant_file_open() {
        let fixture = PackageFixture::minimal();
        fixture.write("assets/icons/icon.svg", SAFE_SVG);
        let external = tempfile::tempdir().unwrap();
        fs::create_dir_all(external.path().join("icons")).unwrap();
        fs::write(external.path().join("icons/icon.svg"), SAFE_SVG).unwrap();
        let mut replaced = false;

        let error = validate_theme_directory_with_hook(
            fixture.root(),
            "fixture-theme",
            &mut |relative_path, kind| {
                if !replaced
                    && relative_path == "assets/icons/icon.svg"
                    && kind == ValidationOpenKind::FileAfterAddressedValidation
                {
                    fs::rename(fixture.path("assets"), fixture.path("assets-original")).unwrap();
                    fs::rename(external.path(), fixture.path("assets")).unwrap();
                    replaced = true;
                }
            },
        )
        .unwrap_err();

        assert!(replaced);
        assert_eq!(error.code, ThemeErrorCode::UnsafePath);
        assert_eq!(
            error.message,
            "Theme package directory changed during validation."
        );
    }

    #[test]
    fn rejects_unsupported_nested_archive_non_utf8_license_and_outside_file() {
        for (path, bytes, expected_code) in [
            (
                "assets/data.bin",
                b"data".as_slice(),
                ThemeErrorCode::UnsafePath,
            ),
            (
                "assets/nested.theme",
                b"PK\x03\x04".as_slice(),
                ThemeErrorCode::UnsafePath,
            ),
            (
                "licenses/LICENSE.txt",
                b"\xff\xfe".as_slice(),
                ThemeErrorCode::InvalidUtf8,
            ),
            (
                "README.md",
                b"outside".as_slice(),
                ThemeErrorCode::UnsafePath,
            ),
        ] {
            let fixture = PackageFixture::minimal();
            fixture.write(path, bytes);
            assert_eq!(
                fixture.validate().unwrap_err().code,
                expected_code,
                "path {path}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn rejects_non_file_non_directory_entries() {
        use std::os::unix::net::UnixListener;

        let fixture = PackageFixture::minimal();
        fs::create_dir_all(fixture.path("assets")).unwrap();
        let _listener = UnixListener::bind(fixture.path("assets/socket.svg")).unwrap();

        assert_eq!(
            fixture.validate().unwrap_err().code,
            ThemeErrorCode::UnsafePath
        );
    }

    #[test]
    fn enforces_normalized_path_depth_and_path_length_limits() {
        let deep = PackageFixture::minimal();
        let deep_path = format!("assets/{}/icon.svg", vec!["d"; 15].join("/"));
        deep.write(&deep_path, SAFE_SVG);
        assert_eq!(
            deep.validate().unwrap_err().code,
            ThemeErrorCode::ThemeTooLarge
        );

        let long = PackageFixture::minimal();
        let long_path = format!("assets/{}.svg", "a".repeat(230));
        long.write(&long_path, SAFE_SVG);
        assert_eq!(
            long.validate().unwrap_err().code,
            ThemeErrorCode::ThemeTooLarge
        );
    }

    #[test]
    fn enforces_manifest_css_font_and_image_file_limits() {
        let oversized_manifest = PackageFixture::minimal();
        oversized_manifest.write_sized("manifest.json", b"{", 64 * 1024 + 1);
        assert_eq!(
            oversized_manifest.validate().unwrap_err().code,
            ThemeErrorCode::ThemeTooLarge
        );

        let oversized_css = PackageFixture::minimal();
        oversized_css.write_sized("theme.css", b":root{}", 256 * 1024 + 1);
        assert_eq!(
            oversized_css.validate().unwrap_err().code,
            ThemeErrorCode::ThemeTooLarge
        );

        let oversized_font = PackageFixture::minimal();
        oversized_font.write_sized("assets/font.woff2", b"wOF2", 4 * 1024 * 1024 + 1);
        assert_eq!(
            oversized_font.validate().unwrap_err().code,
            ThemeErrorCode::ThemeTooLarge
        );

        let oversized_image = PackageFixture::minimal();
        oversized_image.write_sized(
            "assets/image.png",
            b"\x89PNG\r\n\x1a\n",
            8 * 1024 * 1024 + 1,
        );
        assert_eq!(
            oversized_image.validate().unwrap_err().code,
            ThemeErrorCode::ThemeTooLarge
        );
    }

    #[test]
    fn enforces_entry_count_and_aggregate_byte_limits() {
        let too_many = PackageFixture::minimal();
        for index in 0..255 {
            too_many.write(
                &format!("assets/icons/{index:03}.svg"),
                br##"<svg xmlns="http://www.w3.org/2000/svg"/>"##,
            );
        }
        assert_eq!(
            too_many.validate().unwrap_err().code,
            ThemeErrorCode::ThemeTooLarge
        );

        let too_large = PackageFixture::minimal();
        for index in 0..4 {
            too_large.write_sized(
                &format!("assets/images/{index}.gif"),
                b"GIF89a",
                8 * 1024 * 1024,
            );
        }
        assert_eq!(
            too_large.validate().unwrap_err().code,
            ThemeErrorCode::ThemeTooLarge
        );
    }

    #[test]
    fn rejects_nfc_aliases_and_case_insensitive_collisions_before_content_reads() {
        for aliases in [
            vec![
                "assets/icons/Cafe\u{301}.svg".to_string(),
                "assets/icons/Café.svg".to_string(),
            ],
            vec![
                "assets/icons/Icon.svg".to_string(),
                "assets/icons/icon.svg".to_string(),
            ],
        ] {
            assert_eq!(
                validate_path_aliases(&aliases).unwrap_err().code,
                ThemeErrorCode::UnsafePath
            );
        }
    }

    #[test]
    fn rejects_unicode_default_case_fold_collisions() {
        let aliases = vec![
            "assets/icons/Σ.svg".to_string(),
            "assets/icons/ς.svg".to_string(),
        ];

        assert_eq!(
            validate_path_aliases(&aliases).unwrap_err().code,
            ThemeErrorCode::UnsafePath
        );
    }

    #[test]
    fn rejects_post_unicode_9_default_case_fold_collisions() {
        let aliases = vec![
            "assets/icons/Ა.svg".to_string(),
            "assets/icons/ა.svg".to_string(),
        ];

        assert_eq!(
            validate_path_aliases(&aliases).unwrap_err().code,
            ThemeErrorCode::UnsafePath
        );
    }

    #[test]
    fn rejects_invalid_magic_for_every_binary_resource_type() {
        for path in [
            "assets/font.woff2",
            "assets/image.png",
            "assets/image.jpg",
            "assets/image.jpeg",
            "assets/image.webp",
            "assets/image.gif",
        ] {
            let fixture = PackageFixture::minimal();
            fixture.write(path, b"not the declared format");
            assert_eq!(
                fixture.validate().unwrap_err().code,
                ThemeErrorCode::UnsafeResource,
                "path {path}"
            );
        }
    }

    #[test]
    fn requires_every_declared_license_to_exist_and_fonts_to_declare_one() {
        let missing = PackageFixture::minimal();
        missing.write_manifest(with_license(approved_manifest()));
        assert_eq!(
            missing.validate().unwrap_err().code,
            ThemeErrorCode::InvalidManifest
        );

        let undeclared = PackageFixture::minimal();
        undeclared.write("assets/font.woff2", &valid_woff2(1));
        undeclared.write("licenses/LICENSE.txt", b"An unlisted license\n");
        assert_eq!(
            undeclared.validate().unwrap_err().code,
            ThemeErrorCode::InvalidManifest
        );
    }

    #[test]
    fn rejects_active_or_externally_referencing_svg_content() {
        let unsafe_svgs = [
            r#"<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg" onload="alert(1)"/>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><foreignObject/></svg>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><iframe/></svg>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><object/></svg>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><embed/></svg>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><a href="https://example.com"/></svg>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><use href="icon.svg#mark"/></svg>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><rect style="fill:url(https://example.com/a.svg)"/></svg>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><style>@import url(https://example.com/a.css);</style></svg>"#,
            r#"<!DOCTYPE svg><svg xmlns="http://www.w3.org/2000/svg"/>"#,
            r#"<!DOCTYPE svg [<!ENTITY x "boom">]><svg xmlns="http://www.w3.org/2000/svg">&x;</svg>"#,
            r#"<?xml-stylesheet href="https://example.com/a.css"?><svg xmlns="http://www.w3.org/2000/svg"/>"#,
        ];

        for svg in unsafe_svgs {
            let fixture = PackageFixture::minimal();
            fixture.write("assets/icons/unsafe.svg", svg.as_bytes());
            assert_eq!(
                fixture.validate().unwrap_err().code,
                ThemeErrorCode::UnsafeResource,
                "svg {svg}"
            );
        }
    }

    #[test]
    fn rejects_svg_base_uri_attributes_even_with_fragment_references() {
        let unsafe_svgs = [
            r##"<svg xmlns="http://www.w3.org/2000/svg" xml:base="https://example.com/a.svg"><g id="icon"/><use href="#icon"/></svg>"##,
            r##"<svg xmlns="http://www.w3.org/2000/svg" base="https://example.com/a.svg"><g id="icon"/><use href="#icon"/></svg>"##,
            r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:x="http://www.w3.org/XML/1998/namespace" x:base="https://example.com/a.svg"><g id="icon"/><use href="#icon"/></svg>"##,
        ];

        for svg in unsafe_svgs {
            let fixture = PackageFixture::minimal();
            fixture.write("assets/icons/unsafe.svg", svg.as_bytes());
            assert_eq!(
                fixture.validate().unwrap_err().code,
                ThemeErrorCode::UnsafeResource,
                "svg {svg}"
            );
        }
    }

    #[test]
    fn rejects_non_whitespace_outside_the_svg_root() {
        for svg in [
            r#"payload<svg xmlns="http://www.w3.org/2000/svg"/>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg"/>payload"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg"/><![CDATA[payload]]>"#,
        ] {
            let fixture = PackageFixture::minimal();
            fixture.write("assets/icons/unsafe.svg", svg.as_bytes());
            assert_eq!(
                fixture.validate().unwrap_err().code,
                ThemeErrorCode::UnsafeResource,
                "svg {svg}"
            );
        }
    }

    #[test]
    fn content_changes_across_every_package_class_change_the_fingerprint() {
        let fixture = PackageFixture::minimal();
        add_font_bundle(&fixture);
        fixture.write("assets/icons/icon.svg", SAFE_SVG);

        let mut previous = fixture.validate().unwrap().descriptor.fingerprint;

        fixture.write("theme.css", b":root { --theme-accent: #ff0000; }");
        let changed_css = fixture.validate().unwrap().descriptor.fingerprint;
        assert_ne!(changed_css, previous);
        previous = changed_css;

        fixture.write("assets/fonts/JetBrainsMono-Regular.woff2", &valid_woff2(9));
        let changed_font = fixture.validate().unwrap().descriptor.fingerprint;
        assert_ne!(changed_font, previous);
        previous = changed_font;

        fixture.write(
            "assets/icons/icon.svg",
            br##"<svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0"/></svg>"##,
        );
        let changed_icon = fixture.validate().unwrap().descriptor.fingerprint;
        assert_ne!(changed_icon, previous);
        previous = changed_icon;

        let mut manifest = with_license(approved_manifest());
        manifest["name"] = json!("Changed Fixture Theme");
        fixture.write_manifest(manifest);
        let changed_manifest = fixture.validate().unwrap().descriptor.fingerprint;
        assert_ne!(changed_manifest, previous);
        previous = changed_manifest;

        fixture.write("licenses/LICENSE.txt", b"Changed font license\n");
        assert_ne!(fixture.validate().unwrap().descriptor.fingerprint, previous);
    }

    #[test]
    fn canonical_manifest_and_sorted_paths_make_fingerprint_deterministic() {
        let first = PackageFixture::minimal();
        first.write("assets/z.svg", SAFE_SVG);
        first.write("assets/a.svg", SAFE_SVG);
        first.write("licenses/NOTICE.md", b"Notice\n");

        let second = PackageFixture::minimal();
        second.write("licenses/NOTICE.md", b"Notice\n");
        second.write("assets/a.svg", SAFE_SVG);
        second.write("assets/z.svg", SAFE_SVG);
        second.write(
            "manifest.json",
            serde_json::to_string(&approved_manifest())
                .unwrap()
                .as_bytes(),
        );

        assert_eq!(
            first.validate().unwrap().descriptor.fingerprint,
            second.validate().unwrap().descriptor.fingerprint
        );
    }

    #[test]
    fn rejects_a_file_that_already_exceeds_its_metadata_limit() {
        let fixture = PackageFixture::minimal();
        let path = fixture.path("assets/image.gif");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .unwrap();
        file.write_all(b"GIF89a").unwrap();
        file.set_len(8 * 1024 * 1024 + 1).unwrap();

        assert_eq!(
            fixture.validate().unwrap_err().code,
            ThemeErrorCode::ThemeTooLarge
        );
    }

    #[test]
    fn rejects_streamed_bytes_crossing_file_or_aggregate_limits() {
        let mut total = 0;
        assert_eq!(
            read_bounded_stream(Cursor::new(b"12345"), 4, &mut total)
                .unwrap_err()
                .code,
            ThemeErrorCode::ThemeTooLarge
        );

        let mut total = MAX_PACKAGE_BYTES;
        assert_eq!(
            read_bounded_stream(Cursor::new(b"1"), 1, &mut total)
                .unwrap_err()
                .code,
            ThemeErrorCode::ThemeTooLarge
        );
    }
}
