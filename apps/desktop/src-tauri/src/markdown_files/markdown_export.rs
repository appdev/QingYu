use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::hash::Hash;
use std::io::{self, Seek, SeekFrom, Write};
use std::ops::Range;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use cap_fs_ext::DirExt;
use cap_std::fs::Dir;

use super::path::{is_markdown_tree_file, markdown_tree_root_for_path, path_to_string};
use super::resource_writer::{file_identity, FileIdentity};

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MarkdownExportReference {
    from: usize,
    href: String,
    // The parser decodes Markdown escapes in `href`; this keeps the exact source slice for tamper-safe range validation.
    raw_href: String,
    #[serde(default)]
    resource_path: Option<String>,
    to: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct MarkdownExportSnapshotResource {
    body_base64: String,
    name: String,
    path: String,
}

fn strip_markdown_resource_suffix(href: &str) -> (&str, &str) {
    let suffix_start = [href.find('?'), href.find('#')]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(href.len());

    (&href[..suffix_start], &href[suffix_start..])
}

fn percent_decode_markdown_resource_path(path: &str) -> Result<String, String> {
    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err("Markdown resource path has invalid percent encoding".to_string());
            }

            let hex = std::str::from_utf8(&bytes[index + 1..index + 3])
                .map_err(|_| "Markdown resource path has invalid percent encoding".to_string())?;
            let byte = u8::from_str_radix(hex, 16)
                .map_err(|_| "Markdown resource path has invalid percent encoding".to_string())?;
            decoded.push(byte);
            index += 3;
            continue;
        }

        decoded.push(bytes[index]);
        index += 1;
    }

    String::from_utf8(decoded)
        .map_err(|_| "Markdown resource path has invalid UTF-8 encoding".to_string())
}

fn is_windows_absolute_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\')
}

fn markdown_resource_url_scheme(path: &str) -> Option<&str> {
    let colon = path.find(':')?;
    let scheme = &path[..colon];
    if scheme.is_empty()
        || !scheme.as_bytes()[0].is_ascii_alphabetic()
        || !scheme
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
    {
        return None;
    }

    Some(scheme)
}

fn file_url_markdown_resource_path(href: &str) -> Result<PathBuf, String> {
    let normalized = href.to_ascii_lowercase();
    if !normalized.starts_with("file://") {
        return Err("Markdown resource file URL is invalid".to_string());
    }

    let encoded_path = &href["file://".len()..];
    if encoded_path.is_empty() {
        return Err("Markdown resource path is empty".to_string());
    }

    let decoded_path = percent_decode_markdown_resource_path(encoded_path)?;
    let path_text = if cfg!(windows)
        && decoded_path.starts_with('/')
        && decoded_path.as_bytes().get(2) == Some(&b':')
    {
        decoded_path[1..].to_string()
    } else if cfg!(windows) && !decoded_path.starts_with('/') {
        format!("//{decoded_path}")
    } else {
        decoded_path
    };
    let path = PathBuf::from(path_text);
    if !path.is_absolute() {
        return Err("Markdown resource file URL must be absolute".to_string());
    }

    Ok(path)
}

fn resolve_markdown_export_resource_path(
    root: &Path,
    document_path: &Path,
    href: &str,
) -> Result<PathBuf, String> {
    let (path_text, _) = strip_markdown_resource_suffix(href.trim());
    if path_text.is_empty() {
        return Err("Markdown resource path is empty".to_string());
    }

    let normalized = path_text.to_ascii_lowercase();
    let (candidate, restrict_to_root) = if normalized.starts_with("file:") {
        (file_url_markdown_resource_path(path_text)?, false)
    } else if is_windows_absolute_path(path_text) {
        (
            PathBuf::from(percent_decode_markdown_resource_path(path_text)?),
            false,
        )
    } else {
        if markdown_resource_url_scheme(path_text).is_some() || path_text.starts_with("//") {
            return Err("Only local Markdown resources can be exported".to_string());
        }

        let decoded_path = percent_decode_markdown_resource_path(path_text)?.replace('\\', "/");
        let candidate = if decoded_path.starts_with('/') {
            root.join(decoded_path.trim_start_matches('/'))
        } else {
            document_path
                .parent()
                .ok_or_else(|| "Current document folder is invalid".to_string())?
                .join(decoded_path)
        };
        (candidate, true)
    };
    let canonical_path = candidate
        .canonicalize()
        .map_err(|error| format!("Could not read Markdown resource \"{href}\": {error}"))?;

    if restrict_to_root {
        canonical_path.strip_prefix(root).map_err(|_| {
            format!("Markdown resource \"{href}\" is outside the current Markdown folder")
        })?;
    }
    if !canonical_path.is_file() || is_markdown_tree_file(&canonical_path) {
        return Err(format!(
            "Markdown resource \"{href}\" is not a supported local file"
        ));
    }

    Ok(canonical_path)
}

fn utf16_offset_to_byte_index(text: &str, offset: usize) -> Option<usize> {
    // Frontend offsets are JavaScript UTF-16 indices, while Rust string edits require UTF-8 byte boundaries.
    let mut utf16_offset = 0;
    for (byte_index, character) in text.char_indices() {
        if utf16_offset == offset {
            return Some(byte_index);
        }

        utf16_offset += character.len_utf16();
        if utf16_offset > offset {
            return None;
        }
    }

    (utf16_offset == offset).then_some(text.len())
}

fn markdown_export_reference_byte_range(
    markdown: &str,
    reference: &MarkdownExportReference,
) -> Result<Range<usize>, String> {
    if reference.from > reference.to {
        return Err("Markdown export resource range is invalid".to_string());
    }

    let from = utf16_offset_to_byte_index(markdown, reference.from)
        .ok_or_else(|| "Markdown export resource range is invalid".to_string())?;
    let to = utf16_offset_to_byte_index(markdown, reference.to)
        .ok_or_else(|| "Markdown export resource range is invalid".to_string())?;
    if markdown.get(from..to) != Some(reference.raw_href.as_str()) {
        return Err("Markdown export resource range does not match its href".to_string());
    }

    Ok(from..to)
}

fn encode_markdown_relative_path(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let mut encoded = String::new();

    for byte in normalized.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                encoded.push(*byte as char)
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }

    encoded
}

fn collected_markdown_resource_path(
    target_document_path: &Path,
    relative_path: &str,
) -> Result<PathBuf, String> {
    let relative = Path::new(relative_path);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
    {
        return Err("Collected Markdown resource path is invalid".to_string());
    }

    Ok(target_document_path
        .parent()
        .ok_or_else(|| "Markdown export folder is invalid".to_string())?
        .join(relative))
}

fn markdown_export_names(suggested_name: &str) -> Result<(String, String), String> {
    let file_name = suggested_name.trim();
    let candidate = Path::new(file_name);
    if file_name.is_empty()
        || file_name.contains('/')
        || file_name.contains('\\')
        || candidate.components().count() != 1
        || !is_markdown_tree_file(candidate)
    {
        return Err("Markdown export file name is invalid".to_string());
    }

    let folder_name = candidate
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Markdown export folder name is invalid".to_string())?;

    Ok((file_name.to_string(), folder_name.to_string()))
}

struct RetainedMarkdownExportFolder {
    directory: Dir,
    identity: crate::storage_capability::DirectoryIdentity,
    name: OsString,
    parent: Dir,
    parent_identity: crate::storage_capability::DirectoryIdentity,
    parent_path: PathBuf,
    path: PathBuf,
}

impl RetainedMarkdownExportFolder {
    fn verify_addressability(&self) -> Result<(), String> {
        if crate::storage_capability::directory_identity(&self.parent)
            .map_err(|error| error.to_string())?
            != self.parent_identity
            || crate::storage_capability::directory_identity(&self.directory)
                .map_err(|error| error.to_string())?
                != self.identity
        {
            return Err("Markdown export folder changed during export".to_string());
        }
        let current_parent =
            crate::storage_capability::open_canonical_directory_nofollow(&self.parent_path)
                .map_err(|error| format!("Markdown export parent changed: {error}"))?;
        if crate::storage_capability::directory_identity(&current_parent)
            .map_err(|error| error.to_string())?
            != self.parent_identity
        {
            return Err("Markdown export parent changed during export".to_string());
        }
        let current = current_parent
            .open_dir_nofollow(&self.name)
            .map_err(|error| format!("Markdown export folder changed: {error}"))?;
        if crate::storage_capability::directory_identity(&current)
            .map_err(|error| error.to_string())?
            != self.identity
        {
            return Err("Markdown export folder changed during export".to_string());
        }
        Ok(())
    }

    fn verify_target(
        &self,
        file_name: &str,
        retained: &fs::File,
        expected: FileIdentity,
    ) -> Result<(), String> {
        let retained_metadata = retained.metadata().map_err(|error| error.to_string())?;
        if !retained_metadata.is_file() || file_identity(&retained_metadata) != expected {
            return Err("Markdown export target changed during export".to_string());
        }
        let current = self
            .directory
            .open_with(
                file_name,
                &crate::storage_capability::nonfollowing_read_options(),
            )
            .map_err(|error| format!("Markdown export target changed: {error}"))?;
        let current_metadata = current.metadata().map_err(|error| error.to_string())?;
        if !current_metadata.is_file() || file_identity(&current_metadata) != expected {
            return Err("Markdown export target changed during export".to_string());
        }
        Ok(())
    }

    fn cleanup_after_error(self, error: String) -> String {
        match self.directory.remove_open_dir_all() {
            Ok(()) => error,
            Err(cleanup_error) => {
                format!("{error}; Markdown export cleanup failed: {cleanup_error}")
            }
        }
    }
}

fn create_unique_markdown_export_folder(
    parent_path: &Path,
    folder_name: &str,
) -> Result<RetainedMarkdownExportFolder, String> {
    let parent = crate::storage_capability::open_canonical_directory_nofollow(parent_path)
        .map_err(|error| error.to_string())?;
    let parent_identity = crate::storage_capability::directory_identity(&parent)
        .map_err(|error| error.to_string())?;
    for attempt in 0..1000 {
        let candidate_name = if attempt == 0 {
            folder_name.to_string()
        } else {
            format!("{folder_name}-{}", attempt + 1)
        };
        match parent.create_dir(&candidate_name) {
            Ok(()) => {
                let directory = parent
                    .open_dir_nofollow(&candidate_name)
                    .map_err(|error| error.to_string())?;
                let identity = crate::storage_capability::directory_identity(&directory)
                    .map_err(|error| error.to_string())?;
                let folder = RetainedMarkdownExportFolder {
                    directory,
                    identity,
                    name: OsString::from(&candidate_name),
                    parent,
                    parent_identity,
                    parent_path: parent_path.to_path_buf(),
                    path: parent_path.join(candidate_name),
                };
                if let Err(error) = folder.verify_addressability() {
                    return Err(folder.cleanup_after_error(error));
                }
                return Ok(folder);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.to_string()),
        }
    }

    Err("Could not create a unique Markdown export folder".to_string())
}

fn normalized_markdown_export_resource_folder(folder: &str) -> Result<PathBuf, String> {
    let normalized = folder.trim().replace('\\', "/");
    if normalized == "." {
        return Ok(PathBuf::new());
    }
    let candidate = Path::new(&normalized);
    if normalized.is_empty() || candidate.is_absolute() {
        return Err("Markdown export resource folder must be relative".to_string());
    }
    let mut relative = PathBuf::new();
    for component in candidate.components() {
        match component {
            Component::Normal(part) => relative.push(part),
            Component::CurDir => {}
            _ => return Err("Markdown export resource folder is invalid".to_string()),
        }
    }
    if relative.as_os_str().is_empty() {
        return Err("Markdown export resource folder is invalid".to_string());
    }
    Ok(relative)
}

fn ensure_markdown_export_resource_folder(root: &Dir, folder: &Path) -> Result<Dir, String> {
    let mut current = root.try_clone().map_err(|error| error.to_string())?;
    for component in folder.components() {
        let Component::Normal(name) = component else {
            return Err("Markdown export resource folder is invalid".to_string());
        };
        match current.symlink_metadata(name) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err("Markdown export resource folder cannot be a symbolic link".to_string())
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err("Markdown export resource folder must be a directory".to_string())
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if let Err(error) = current.create_dir(name) {
                    if error.kind() != io::ErrorKind::AlreadyExists {
                        return Err(error.to_string());
                    }
                }
            }
            Err(error) => return Err(error.to_string()),
        }
        current = current
            .open_dir_nofollow(name)
            .map_err(|error| error.to_string())?;
    }
    Ok(current)
}

fn open_existing_markdown_export_resource_folder(root: &Dir, folder: &Path) -> Result<Dir, String> {
    let mut current = root.try_clone().map_err(|error| error.to_string())?;
    for component in folder.components() {
        let Component::Normal(name) = component else {
            return Err("Markdown export resource folder is invalid".to_string());
        };
        current = current
            .open_dir_nofollow(name)
            .map_err(|error| format!("Markdown export resource folder changed: {error}"))?;
    }
    Ok(current)
}

fn unique_markdown_export_resource_name(file_name: &str, attempt: usize) -> Result<String, String> {
    let candidate = Path::new(file_name);
    if file_name.trim().is_empty()
        || file_name.contains('/')
        || file_name.contains('\\')
        || candidate.components().count() != 1
    {
        return Err("Markdown export resource file name is invalid".to_string());
    }
    if attempt == 0 {
        return Ok(file_name.to_string());
    }
    let stem = candidate
        .file_stem()
        .and_then(OsStr::to_str)
        .filter(|stem| !stem.is_empty())
        .ok_or_else(|| "Markdown export resource file name is invalid".to_string())?;
    let suffix = format!("-{}", attempt + 1);
    Ok(match candidate.extension().and_then(OsStr::to_str) {
        Some(extension) => format!("{stem}{suffix}.{extension}"),
        None => format!("{stem}{suffix}"),
    })
}

struct RetainedMarkdownExportResource {
    file: fs::File,
    file_identity: FileIdentity,
    file_name: String,
    folder: Dir,
    folder_identity: crate::storage_capability::DirectoryIdentity,
    relative_folder: PathBuf,
}

struct MarkdownExportImportedResource {
    relative_path: String,
    retained: Option<RetainedMarkdownExportResource>,
}

impl MarkdownExportImportedResource {
    #[cfg(test)]
    fn unverified(relative_path: String) -> Self {
        Self {
            relative_path,
            retained: None,
        }
    }

    fn verify(&self, export_directory: &Dir) -> Result<(), String> {
        let Some(retained) = self.retained.as_ref() else {
            return Ok(());
        };
        if crate::storage_capability::directory_identity(&retained.folder)
            .map_err(|error| error.to_string())?
            != retained.folder_identity
        {
            return Err("Markdown export resource folder changed during export".to_string());
        }
        let current_folder = open_existing_markdown_export_resource_folder(
            export_directory,
            &retained.relative_folder,
        )?;
        if crate::storage_capability::directory_identity(&current_folder)
            .map_err(|error| error.to_string())?
            != retained.folder_identity
        {
            return Err("Markdown export resource folder changed during export".to_string());
        }
        let retained_metadata = retained
            .file
            .metadata()
            .map_err(|error| error.to_string())?;
        if !retained_metadata.is_file()
            || file_identity(&retained_metadata) != retained.file_identity
        {
            return Err("Markdown export resource changed during export".to_string());
        }
        let current = current_folder
            .open_with(
                &retained.file_name,
                &crate::storage_capability::nonfollowing_read_options(),
            )
            .map_err(|error| format!("Markdown export resource changed: {error}"))?;
        let current_metadata = current.metadata().map_err(|error| error.to_string())?;
        if !current_metadata.is_file() || file_identity(&current_metadata) != retained.file_identity
        {
            return Err("Markdown export resource changed during export".to_string());
        }
        Ok(())
    }
}

fn write_markdown_export_resource(
    source_name: &str,
    export_directory: &Dir,
    folder: &str,
    write: impl FnOnce(&mut fs::File) -> Result<(), String>,
) -> Result<MarkdownExportImportedResource, String> {
    let relative_folder = normalized_markdown_export_resource_folder(folder)?;
    let target_folder = ensure_markdown_export_resource_folder(export_directory, &relative_folder)?;
    let target_folder_identity = crate::storage_capability::directory_identity(&target_folder)
        .map_err(|error| error.to_string())?;
    let (target_name, target) = (0..1000)
        .find_map(|attempt| {
            let target_name = match unique_markdown_export_resource_name(source_name, attempt) {
                Ok(name) => name,
                Err(error) => return Some(Err(error)),
            };
            match target_folder.open_with(
                &target_name,
                &crate::storage_capability::create_private_file_options(),
            ) {
                Ok(target) => Some(Ok((target_name, target))),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => None,
                Err(error) => Some(Err(error.to_string())),
            }
        })
        .transpose()?
        .ok_or_else(|| "Could not create a unique Markdown export resource".to_string())?;
    let target_metadata = target.metadata().map_err(|error| error.to_string())?;
    if !target_metadata.is_file() {
        return Err("Markdown export resource target must be a regular file".to_string());
    }
    let target_identity = file_identity(&target_metadata);
    let mut target = target.into_std();
    write(&mut target)?;
    target.sync_all().map_err(|error| error.to_string())?;

    Ok(MarkdownExportImportedResource {
        relative_path: path_to_string(&relative_folder.join(&target_name)),
        retained: Some(RetainedMarkdownExportResource {
            file: target,
            file_identity: target_identity,
            file_name: target_name,
            folder: target_folder,
            folder_identity: target_folder_identity,
            relative_folder,
        }),
    })
}

fn import_markdown_export_resource(
    source_path: &Path,
    export_directory: &Dir,
    folder: &str,
) -> Result<MarkdownExportImportedResource, String> {
    let source_name = source_path
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| "Markdown export resource file name is invalid".to_string())?;
    let source_parent_path = source_path
        .parent()
        .ok_or_else(|| "Markdown export resource folder is invalid".to_string())?;
    let source_parent =
        crate::storage_capability::open_canonical_directory_nofollow(source_parent_path)
            .map_err(|error| error.to_string())?;
    let expected_metadata = source_parent
        .symlink_metadata(source_name)
        .map_err(|error| error.to_string())?;
    if !expected_metadata.is_file() || expected_metadata.file_type().is_symlink() {
        return Err("Markdown export resource is not a regular file".to_string());
    }
    let source = source_parent
        .open_with(
            source_name,
            &crate::storage_capability::nonfollowing_read_options(),
        )
        .map_err(|error| error.to_string())?;
    let source_metadata = source.metadata().map_err(|error| error.to_string())?;
    if !source_metadata.is_file()
        || file_identity(&source_metadata) != file_identity(&expected_metadata)
    {
        return Err("Markdown export resource changed during export".to_string());
    }
    let mut source = source.into_std();
    write_markdown_export_resource(source_name, export_directory, folder, |target| {
        io::copy(&mut source, target).map_err(|error| error.to_string())?;
        Ok(())
    })
}

fn import_markdown_export_snapshot_resource(
    resource: &[u8],
    source_name: &str,
    export_directory: &Dir,
    folder: &str,
) -> Result<MarkdownExportImportedResource, String> {
    write_markdown_export_resource(source_name, export_directory, folder, |target| {
        target
            .write_all(resource)
            .map_err(|error| error.to_string())
    })
}

fn open_markdown_export_target(
    export_folder: &RetainedMarkdownExportFolder,
    file_name: &str,
) -> Result<(fs::File, FileIdentity), String> {
    let target = export_folder
        .directory
        .open_with(
            file_name,
            &crate::storage_capability::create_private_file_options(),
        )
        .map_err(|error| error.to_string())?;
    let metadata = target.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Err("Markdown export target must be a regular file".to_string());
    }
    let identity = file_identity(&metadata);
    Ok((target.into_std(), identity))
}

fn export_markdown_bundle_with_importer<ResourceKey>(
    parent_path: String,
    suggested_name: String,
    markdown: String,
    folder: String,
    mut validated_references: Vec<(MarkdownExportReference, Range<usize>, ResourceKey)>,
    mut import: impl FnMut(
        &ResourceKey,
        &Path,
        &str,
        &Dir,
    ) -> Result<MarkdownExportImportedResource, String>,
) -> Result<PathBuf, String>
where
    ResourceKey: Clone + Eq + Hash,
{
    let export_parent = PathBuf::from(parent_path)
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if !export_parent.is_dir() {
        return Err("Markdown export parent must be a folder".to_string());
    }
    let (file_name, folder_name) = markdown_export_names(&suggested_name)?;

    validated_references.sort_by_key(|(_, range, _)| range.start);
    if validated_references
        .windows(2)
        .any(|window| window[0].1.end > window[1].1.start)
    {
        return Err("Markdown export resource ranges overlap".to_string());
    }

    let export_folder = create_unique_markdown_export_folder(&export_parent, &folder_name)?;
    let target_path = export_folder.path.join(&file_name);
    let result = (|| {
        let (mut target, target_identity) =
            open_markdown_export_target(&export_folder, &file_name)?;
        target
            .write_all(markdown.as_bytes())
            .map_err(|error| error.to_string())?;

        let mut collected_by_source = HashMap::<ResourceKey, MarkdownExportImportedResource>::new();

        for (_, _, source_path) in &validated_references {
            if collected_by_source.contains_key(source_path) {
                continue;
            }

            let relative_path =
                import(source_path, &target_path, &folder, &export_folder.directory)?;
            collected_markdown_resource_path(&target_path, &relative_path.relative_path)?;
            collected_by_source.insert(source_path.clone(), relative_path);
        }

        let mut exported_markdown = markdown.clone();
        for (reference, range, source_path) in validated_references.iter().rev() {
            let relative_path = collected_by_source
                .get(source_path)
                .ok_or_else(|| "Markdown resource was not collected".to_string())?;
            let (_, suffix) = strip_markdown_resource_suffix(&reference.href);
            let replacement = format!(
                "{}{suffix}",
                encode_markdown_relative_path(&relative_path.relative_path)
            );
            exported_markdown.replace_range(range.clone(), &replacement);
        }

        target.set_len(0).map_err(|error| error.to_string())?;
        target
            .seek(SeekFrom::Start(0))
            .map_err(|error| error.to_string())?;
        target
            .write_all(exported_markdown.as_bytes())
            .map_err(|error| error.to_string())?;
        target.sync_all().map_err(|error| error.to_string())?;
        for resource in collected_by_source.values() {
            resource.verify(&export_folder.directory)?;
        }
        export_folder.verify_target(&file_name, &target, target_identity)?;
        export_folder.verify_addressability()?;

        Ok(())
    })();

    if let Err(error) = result {
        return Err(export_folder.cleanup_after_error(error));
    }

    Ok(target_path)
}

fn export_markdown_file_with_importer(
    parent_path: String,
    suggested_name: String,
    markdown: String,
    document_path: String,
    root_path: Option<String>,
    folder: String,
    references: Vec<MarkdownExportReference>,
    import: impl FnMut(&PathBuf, &Path, &str, &Dir) -> Result<MarkdownExportImportedResource, String>,
) -> Result<PathBuf, String> {
    let source_document_path = PathBuf::from(document_path)
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if !source_document_path.is_file() || !is_markdown_tree_file(&source_document_path) {
        return Err("Current document must be a saved Markdown file".to_string());
    }
    let root_source = root_path
        .as_deref()
        .map(Path::new)
        .unwrap_or(source_document_path.as_path());
    let root = markdown_tree_root_for_path(root_source)?
        .canonicalize()
        .map_err(|error| error.to_string())?;
    source_document_path
        .strip_prefix(&root)
        .map_err(|_| "Current document is outside the Markdown folder".to_string())?;
    let validated_references = references
        .into_iter()
        .map(|reference| {
            let range = markdown_export_reference_byte_range(&markdown, &reference)?;
            let source_path = resolve_markdown_export_resource_path(
                &root,
                &source_document_path,
                &reference.href,
            )?;
            Ok((reference, range, source_path))
        })
        .collect::<Result<Vec<_>, String>>()?;
    export_markdown_bundle_with_importer(
        parent_path,
        suggested_name,
        markdown,
        folder,
        validated_references,
        import,
    )
}

fn export_markdown_file_with_importer_in_registry(
    registry: &Arc<crate::dejavu_sync::path_guard::NativeWorkingTreeRegistry>,
    parent_path: String,
    suggested_name: String,
    markdown: String,
    document_path: String,
    root_path: Option<String>,
    folder: String,
    references: Vec<MarkdownExportReference>,
    mut import: impl FnMut(&Path, &Path, &str, &Dir) -> Result<MarkdownExportImportedResource, String>,
) -> Result<PathBuf, String> {
    let mut guarded_paths = vec![PathBuf::from(&parent_path), PathBuf::from(&document_path)];
    if let Some(root_path) = root_path.as_deref() {
        guarded_paths.push(PathBuf::from(root_path));
    }
    let _mutation = registry.acquire_mutation(&guarded_paths)?;

    export_markdown_file_with_importer(
        parent_path,
        suggested_name,
        markdown,
        document_path,
        root_path,
        folder,
        references,
        |source_path: &PathBuf, target_path, folder, export_directory| {
            import(source_path.as_path(), target_path, folder, export_directory)
        },
    )
}

fn validate_markdown_export_snapshot_resource_path(
    resource: &MarkdownExportSnapshotResource,
) -> Result<(), String> {
    let path = Path::new(&resource.path);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        || path.file_name().and_then(OsStr::to_str) != Some(resource.name.as_str())
        || is_markdown_tree_file(path)
    {
        return Err("Markdown export snapshot resource path is invalid".to_string());
    }
    Ok(())
}

fn export_markdown_snapshot(
    parent_path: String,
    suggested_name: String,
    markdown: String,
    folder: String,
    references: Vec<MarkdownExportReference>,
    resources: Vec<MarkdownExportSnapshotResource>,
) -> Result<PathBuf, String> {
    const MAX_SNAPSHOT_BYTES: usize = 512 * 1024 * 1024;
    let mut decoded_by_path = HashMap::<String, (String, Vec<u8>)>::new();
    let mut total_bytes = 0_usize;
    for resource in resources {
        validate_markdown_export_snapshot_resource_path(&resource)?;
        let body = STANDARD
            .decode(&resource.body_base64)
            .map_err(|_| "Markdown export snapshot resource body is invalid".to_string())?;
        if STANDARD.encode(&body) != resource.body_base64 {
            return Err("Markdown export snapshot resource body is invalid".to_string());
        }
        total_bytes = total_bytes
            .checked_add(body.len())
            .filter(|total| *total <= MAX_SNAPSHOT_BYTES)
            .ok_or_else(|| "Markdown export snapshot is too large".to_string())?;
        if decoded_by_path
            .insert(resource.path, (resource.name, body))
            .is_some()
        {
            return Err("Markdown export snapshot resource path is duplicated".to_string());
        }
    }

    let mut referenced_paths = std::collections::HashSet::new();
    let validated_references = references
        .into_iter()
        .map(|reference| {
            let range = markdown_export_reference_byte_range(&markdown, &reference)?;
            let resource_path = reference.resource_path.clone().ok_or_else(|| {
                "Markdown export snapshot reference is missing its resource path".to_string()
            })?;
            if !decoded_by_path.contains_key(&resource_path) {
                return Err("Markdown export snapshot resource is missing".to_string());
            }
            referenced_paths.insert(resource_path.clone());
            Ok((reference, range, resource_path))
        })
        .collect::<Result<Vec<_>, String>>()?;
    if referenced_paths.len() != decoded_by_path.len() {
        return Err("Markdown export snapshot contains an unreferenced resource".to_string());
    }

    export_markdown_bundle_with_importer(
        parent_path,
        suggested_name,
        markdown,
        folder,
        validated_references,
        |resource_path, _target_path, target_folder, export_directory| {
            let (source_name, body) = decoded_by_path
                .get(resource_path)
                .ok_or_else(|| "Markdown export snapshot resource is missing".to_string())?;
            import_markdown_export_snapshot_resource(
                body,
                source_name,
                export_directory,
                target_folder,
            )
        },
    )
}

#[tauri::command]
pub(crate) async fn export_markdown_file(
    parent_path: String,
    suggested_name: String,
    markdown: String,
    document_path: Option<String>,
    root_path: Option<String>,
    folder: String,
    references: Vec<MarkdownExportReference>,
    resources: Option<Vec<MarkdownExportSnapshotResource>>,
) -> Result<String, String> {
    if let Some(resources) = resources {
        if document_path.is_some() || root_path.is_some() {
            return Err("Markdown export snapshot cannot use host source paths".to_string());
        }
        return tauri::async_runtime::spawn_blocking(move || {
            export_markdown_snapshot(
                parent_path,
                suggested_name,
                markdown,
                folder,
                references,
                resources,
            )
            .map(|path| path_to_string(&path))
        })
        .await
        .map_err(|error| format!("Markdown export task failed: {error}"))?;
    }
    let document_path = document_path
        .ok_or_else(|| "Current document must be a saved Markdown file".to_string())?;
    let registry = Arc::clone(crate::dejavu_sync::path_guard::native_working_tree_registry());
    tauri::async_runtime::spawn_blocking(move || {
        export_markdown_file_with_importer_in_registry(
            &registry,
            parent_path,
            suggested_name,
            markdown,
            document_path,
            root_path,
            folder,
            references,
            |source_path, _target_document_path, target_folder, export_directory| {
                import_markdown_export_resource(source_path, export_directory, target_folder)
            },
        )
        .map(|path| path_to_string(&path))
    })
    .await
    .map_err(|error| format!("Markdown export task failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_an_export_parent_blocked_by_sync_before_creating_the_bundle() {
        let root = tempfile::tempdir().expect("temporary root should be created");
        let root_path = root
            .path()
            .canonicalize()
            .expect("temporary root should resolve");
        let export_parent = root_path.join("exports");
        fs::create_dir(&export_parent).expect("export parent should be created");
        let registry = std::sync::Arc::new(
            crate::dejavu_sync::path_guard::NativeWorkingTreeRegistry::default(),
        );
        let _block = registry
            .block_paths(&root_path, &["exports".to_string()])
            .expect("export parent should be guarded");

        let result = export_markdown_file_with_importer_in_registry(
            &registry,
            export_parent.to_string_lossy().to_string(),
            "draft.md".to_string(),
            String::new(),
            root_path.join("missing.md").to_string_lossy().to_string(),
            Some(root_path.to_string_lossy().to_string()),
            "assets".to_string(),
            Vec::new(),
            |_, _, _, _| panic!("resource importer must not run for a guarded export"),
        );

        assert_eq!(
            result.err().as_deref(),
            Some(crate::dejavu_sync::path_guard::SYNC_PATH_GUARDED_ERROR),
        );
        assert!(!export_parent.join("draft").exists());
    }

    #[test]
    fn rejects_a_source_document_blocked_by_sync_before_creating_the_bundle() {
        let root = tempfile::tempdir().expect("temporary root should be created");
        let root_path = root
            .path()
            .canonicalize()
            .expect("temporary root should resolve");
        let vault = root_path.join("vault");
        let note = vault.join("draft.md");
        let export_parent = root_path.join("exports");
        fs::create_dir(&vault).expect("vault should be created");
        fs::create_dir(&export_parent).expect("export parent should be created");
        fs::write(&note, "# Draft").expect("source note should be created");
        let registry = std::sync::Arc::new(
            crate::dejavu_sync::path_guard::NativeWorkingTreeRegistry::default(),
        );
        let _block = registry
            .block_paths(&vault, &["draft.md".to_string()])
            .expect("source document should be guarded");

        let result = export_markdown_file_with_importer_in_registry(
            &registry,
            export_parent.to_string_lossy().to_string(),
            "draft.md".to_string(),
            "# Draft".to_string(),
            note.to_string_lossy().to_string(),
            Some(vault.to_string_lossy().to_string()),
            "assets".to_string(),
            Vec::new(),
            |_, _, _, _| panic!("resource importer must not run for a guarded source"),
        );

        assert_eq!(
            result.err().as_deref(),
            Some(crate::dejavu_sync::path_guard::SYNC_PATH_GUARDED_ERROR),
        );
        assert!(!export_parent.join("draft").exists());
    }

    fn markdown_export_reference(markdown: &str, href: &str) -> MarkdownExportReference {
        markdown_export_reference_with_raw_href(markdown, href, href)
    }

    fn markdown_export_reference_with_raw_href(
        markdown: &str,
        href: &str,
        raw_href: &str,
    ) -> MarkdownExportReference {
        let byte_from = markdown
            .find(raw_href)
            .expect("synthetic href should be present in markdown");
        let from = markdown[..byte_from].encode_utf16().count();
        let to = from + raw_href.encode_utf16().count();

        MarkdownExportReference {
            from,
            href: href.to_string(),
            raw_href: raw_href.to_string(),
            resource_path: None,
            to,
        }
    }

    #[test]
    fn exports_a_self_contained_markdown_snapshot_without_host_source_paths() {
        let root = tempfile::tempdir().expect("temporary root should be created");
        let export_parent = root.path().join("exports");
        fs::create_dir(&export_parent).expect("export parent should be created");
        let markdown = "![Chart](assets/chart.png)";
        let mut reference = markdown_export_reference(markdown, "assets/chart.png");
        reference.resource_path = Some("notes/assets/chart.png".to_string());

        let exported = export_markdown_snapshot(
            export_parent.to_string_lossy().to_string(),
            "draft.md".to_string(),
            markdown.to_string(),
            "assets".to_string(),
            vec![reference],
            vec![MarkdownExportSnapshotResource {
                body_base64: STANDARD.encode(b"synthetic-image"),
                name: "chart.png".to_string(),
                path: "notes/assets/chart.png".to_string(),
            }],
        )
        .expect("snapshot should be exported");

        assert_eq!(
            fs::read_to_string(&exported).expect("exported Markdown should be readable"),
            markdown
        );
        assert_eq!(
            fs::read(export_parent.join("draft/assets/chart.png"))
                .expect("exported resource should be readable"),
            b"synthetic-image"
        );
    }

    #[test]
    fn exports_markdown_with_deduplicated_local_resources_and_rewritten_destinations() {
        let root = std::env::temp_dir().join(format!(
            "markra-markdown-export-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after epoch")
                .as_nanos()
        ));
        let note = root.join("vault/notes/draft.md");
        let image = root.join("vault/notes/assets/chart image.png");
        let attachment = root.join("vault/files/reference.pdf");
        let escaped_attachment = root.join("vault/files/reference(final).pdf");
        let export_parent = root.join("exports");
        let occupied_bundle = export_parent.join("draft");
        let target = export_parent.join("draft-2/draft.md");
        let markdown = [
            "# 中文草稿",
            "",
            "![Chart](<assets/chart image.png>)",
            "![Chart again](<assets/chart image.png>)",
            "[Reference](../files/reference.pdf?raw=1#page=2)",
            "[Escaped](../files/reference\\(final\\).pdf)",
        ]
        .join("\n");
        let references = vec![
            markdown_export_reference(&markdown, "assets/chart image.png"),
            MarkdownExportReference {
                from: markdown[..markdown
                    .rfind("assets/chart image.png")
                    .expect("second href should exist")]
                    .encode_utf16()
                    .count(),
                href: "assets/chart image.png".to_string(),
                raw_href: "assets/chart image.png".to_string(),
                resource_path: None,
                to: markdown[..markdown
                    .rfind("assets/chart image.png")
                    .expect("second href should exist")]
                    .encode_utf16()
                    .count()
                    + "assets/chart image.png".encode_utf16().count(),
            },
            markdown_export_reference(&markdown, "../files/reference.pdf?raw=1#page=2"),
            markdown_export_reference_with_raw_href(
                &markdown,
                "../files/reference(final).pdf",
                "../files/reference\\(final\\).pdf",
            ),
        ];
        let mut imported_sources = Vec::new();

        fs::create_dir_all(image.parent().expect("image should have a parent"))
            .expect("image folder should be created");
        fs::create_dir_all(
            attachment
                .parent()
                .expect("attachment should have a parent"),
        )
        .expect("attachment folder should be created");
        fs::create_dir_all(&occupied_bundle).expect("occupied bundle should be created");
        fs::write(occupied_bundle.join("keep.txt"), b"keep")
            .expect("occupied bundle should remain untouched");
        fs::write(&note, &markdown).expect("source note should be written");
        fs::write(&image, b"synthetic-image").expect("image should be written");
        fs::write(&attachment, b"synthetic-pdf").expect("attachment should be written");
        fs::write(&escaped_attachment, b"synthetic-escaped-pdf")
            .expect("escaped attachment should be written");

        let exported_path = export_markdown_file_with_importer(
            export_parent.to_string_lossy().to_string(),
            "draft.md".to_string(),
            markdown,
            note.to_string_lossy().to_string(),
            Some(root.join("vault").to_string_lossy().to_string()),
            "assets".to_string(),
            references,
            |source_path, target_document_path, folder, _export_directory| {
                imported_sources.push(source_path.to_path_buf());
                let suffix = imported_sources.len() + 1;
                let file_name = source_path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .expect("synthetic file name should be UTF-8");
                let renamed = if file_name == "chart image.png" {
                    format!("chart image-{suffix}.png")
                } else {
                    file_name.to_string()
                };
                let destination = target_document_path
                    .parent()
                    .expect("target should have a parent")
                    .join(folder)
                    .join(&renamed);
                fs::create_dir_all(destination.parent().expect("resource should have a parent"))
                    .map_err(|error| error.to_string())?;
                fs::copy(source_path, &destination).map_err(|error| error.to_string())?;
                Ok(MarkdownExportImportedResource::unverified(format!(
                    "{folder}/{renamed}"
                )))
            },
        )
        .expect("markdown bundle should be exported");

        assert_eq!(
            exported_path,
            target.canonicalize().expect("target should canonicalize")
        );
        assert_eq!(
            fs::read(occupied_bundle.join("keep.txt")).expect("occupied bundle should be readable"),
            b"keep"
        );
        assert_eq!(
            imported_sources,
            vec![
                image.canonicalize().expect("image should canonicalize"),
                attachment
                    .canonicalize()
                    .expect("attachment should canonicalize"),
                escaped_attachment
                    .canonicalize()
                    .expect("escaped attachment should canonicalize"),
            ]
        );
        assert_eq!(
            fs::read_to_string(&target).expect("exported markdown should be readable"),
            [
                "# 中文草稿",
                "",
                "![Chart](<assets/chart%20image-2.png>)",
                "![Chart again](<assets/chart%20image-2.png>)",
                "[Reference](assets/reference.pdf?raw=1#page=2)",
                "[Escaped](assets/reference%28final%29.pdf)",
            ]
            .join("\n")
        );

        fs::remove_dir_all(root).expect("test tree should be removed");
    }

    #[test]
    fn removes_the_new_markdown_export_folder_after_failure() {
        let root = std::env::temp_dir().join(format!(
            "markra-markdown-export-rollback-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after epoch")
                .as_nanos()
        ));
        let note = root.join("vault/draft.md");
        let first = root.join("vault/assets/first.png");
        let second = root.join("vault/assets/second.png");
        let export_parent = root.join("exports");
        let bundle = export_parent.join("draft");
        let markdown = "![First](assets/first.png)\n![Second](assets/second.png)";
        let references = vec![
            markdown_export_reference(markdown, "assets/first.png"),
            markdown_export_reference(markdown, "assets/second.png"),
        ];
        let mut import_count = 0;

        fs::create_dir_all(first.parent().expect("asset should have a parent"))
            .expect("asset folder should be created");
        fs::create_dir_all(&export_parent).expect("export parent should be created");
        fs::write(&note, markdown).expect("source note should be written");
        fs::write(&first, b"first").expect("first resource should be written");
        fs::write(&second, b"second").expect("second resource should be written");

        let result = export_markdown_file_with_importer(
            export_parent.to_string_lossy().to_string(),
            "draft.md".to_string(),
            markdown.to_string(),
            note.to_string_lossy().to_string(),
            Some(root.join("vault").to_string_lossy().to_string()),
            "assets".to_string(),
            references,
            |source_path, target_document_path, folder, _export_directory| {
                import_count += 1;
                if import_count == 2 {
                    return Err("Synthetic import failure".to_string());
                }

                let destination = target_document_path
                    .parent()
                    .expect("target should have a parent")
                    .join(folder)
                    .join(
                        source_path
                            .file_name()
                            .expect("source should have a file name"),
                    );
                fs::create_dir_all(destination.parent().expect("resource should have a parent"))
                    .map_err(|error| error.to_string())?;
                fs::copy(source_path, &destination).map_err(|error| error.to_string())?;
                Ok(MarkdownExportImportedResource::unverified(format!(
                    "{folder}/{}",
                    source_path
                        .file_name()
                        .and_then(|value| value.to_str())
                        .expect("synthetic file name should be UTF-8")
                )))
            },
        );

        assert_eq!(result, Err("Synthetic import failure".to_string()));
        assert!(!bundle.exists());
        assert!(export_parent.exists());

        fs::remove_dir_all(root).expect("test tree should be removed");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_an_export_folder_swap_without_writing_through_the_replacement() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().expect("temporary root should be created");
        let vault = root.path().join("vault");
        let note = vault.join("draft.md");
        let image = vault.join("image.png");
        let export_parent = root.path().join("exports");
        let export_folder = export_parent.join("draft");
        let moved_export_folder = export_parent.join("draft-original");
        let outside = root.path().join("outside");
        let outside_target = outside.join("draft.md");
        let markdown = "![Image](image.png)";
        fs::create_dir(&vault).expect("vault should be created");
        fs::create_dir(&export_parent).expect("export parent should be created");
        fs::create_dir(&outside).expect("outside folder should be created");
        fs::write(&note, markdown).expect("source note should be created");
        fs::write(&image, b"image").expect("source image should be created");
        fs::write(&outside_target, b"outside-sentinel")
            .expect("outside sentinel should be created");

        let result = export_markdown_file_with_importer(
            export_parent.to_string_lossy().to_string(),
            "draft.md".to_string(),
            markdown.to_string(),
            note.to_string_lossy().to_string(),
            Some(vault.to_string_lossy().to_string()),
            "assets".to_string(),
            vec![markdown_export_reference(markdown, "image.png")],
            |_, _, _, _| {
                fs::rename(&export_folder, &moved_export_folder)
                    .map_err(|error| error.to_string())?;
                symlink(&outside, &export_folder).map_err(|error| error.to_string())?;
                Ok(MarkdownExportImportedResource::unverified(
                    "assets/image.png".to_string(),
                ))
            },
        );

        assert!(result.is_err());
        assert_eq!(
            fs::read(&outside_target).expect("outside sentinel should remain readable"),
            b"outside-sentinel"
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_a_markdown_target_swap_without_writing_through_the_symlink() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().expect("temporary root should be created");
        let vault = root.path().join("vault");
        let note = vault.join("draft.md");
        let image = vault.join("image.png");
        let export_parent = root.path().join("exports");
        let export_target = export_parent.join("draft/draft.md");
        let outside_target = root.path().join("outside.md");
        let markdown = "![Image](image.png)";
        fs::create_dir(&vault).expect("vault should be created");
        fs::create_dir(&export_parent).expect("export parent should be created");
        fs::write(&note, markdown).expect("source note should be created");
        fs::write(&image, b"image").expect("source image should be created");
        fs::write(&outside_target, b"outside-sentinel")
            .expect("outside sentinel should be created");

        let result = export_markdown_file_with_importer(
            export_parent.to_string_lossy().to_string(),
            "draft.md".to_string(),
            markdown.to_string(),
            note.to_string_lossy().to_string(),
            Some(vault.to_string_lossy().to_string()),
            "assets".to_string(),
            vec![markdown_export_reference(markdown, "image.png")],
            |_, _, _, _| {
                fs::remove_file(&export_target).map_err(|error| error.to_string())?;
                symlink(&outside_target, &export_target).map_err(|error| error.to_string())?;
                Ok(MarkdownExportImportedResource::unverified(
                    "assets/image.png".to_string(),
                ))
            },
        );

        assert!(result.is_err());
        assert_eq!(
            fs::read(&outside_target).expect("outside sentinel should remain readable"),
            b"outside-sentinel"
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_a_collected_resource_swap_before_publication() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().expect("temporary root should be created");
        let vault = root.path().join("vault");
        let note = vault.join("draft.md");
        let image = vault.join("image.png");
        let export_parent = root.path().join("exports");
        let exported_resource = export_parent.join("draft/assets/image.png");
        let moved_resource = export_parent.join("draft/assets/image-original.png");
        let outside_target = root.path().join("outside.png");
        let markdown = "![Image](image.png)";
        fs::create_dir(&vault).expect("vault should be created");
        fs::create_dir(&export_parent).expect("export parent should be created");
        fs::write(&note, markdown).expect("source note should be created");
        fs::write(&image, b"image").expect("source image should be created");
        fs::write(&outside_target, b"outside-sentinel")
            .expect("outside sentinel should be created");

        let result = export_markdown_file_with_importer(
            export_parent.to_string_lossy().to_string(),
            "draft.md".to_string(),
            markdown.to_string(),
            note.to_string_lossy().to_string(),
            Some(vault.to_string_lossy().to_string()),
            "assets".to_string(),
            vec![markdown_export_reference(markdown, "image.png")],
            |source_path, _, folder, export_directory| {
                let imported =
                    import_markdown_export_resource(source_path, export_directory, folder)?;
                fs::rename(&exported_resource, &moved_resource)
                    .map_err(|error| error.to_string())?;
                symlink(&outside_target, &exported_resource).map_err(|error| error.to_string())?;
                Ok(imported)
            },
        );

        assert!(result.is_err());
        assert_eq!(
            fs::read(&outside_target).expect("outside sentinel should remain readable"),
            b"outside-sentinel"
        );
    }

    #[test]
    fn rejects_unsafe_markdown_export_names() {
        assert!(markdown_export_names("../draft.md").is_err());
        assert!(markdown_export_names("nested/draft.md").is_err());
        assert!(markdown_export_names("draft.txt").is_err());
        assert!(normalized_markdown_export_resource_folder("../outside").is_err());
        assert_eq!(
            normalized_markdown_export_resource_folder("."),
            Ok(PathBuf::new())
        );
        assert_eq!(
            markdown_export_names("中文草稿.md"),
            Ok(("中文草稿.md".to_string(), "中文草稿".to_string()))
        );
    }

    #[test]
    fn exports_markdown_without_resources_to_a_standalone_folder() {
        let root = std::env::temp_dir().join(format!(
            "markra-markdown-export-empty-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after epoch")
                .as_nanos()
        ));
        let note = root.join("vault/draft.md");
        let export_parent = root.join("exports");

        fs::create_dir_all(note.parent().expect("note should have a parent"))
            .expect("vault should be created");
        fs::create_dir_all(&export_parent).expect("export parent should be created");
        fs::write(&note, "# Standalone").expect("source note should be written");

        let exported_path = export_markdown_file_with_importer(
            export_parent.to_string_lossy().to_string(),
            "draft.md".to_string(),
            "# Standalone".to_string(),
            note.to_string_lossy().to_string(),
            Some(root.join("vault").to_string_lossy().to_string()),
            "assets".to_string(),
            Vec::new(),
            |_source_path, _target_document_path, _folder, _export_directory| {
                panic!("resource importer should not run without references")
            },
        )
        .expect("resource-free markdown should be exported");

        assert_eq!(
            fs::read_to_string(&exported_path).expect("exported markdown should be readable"),
            "# Standalone"
        );
        assert!(!exported_path
            .parent()
            .expect("export should have a parent")
            .join("assets")
            .exists());

        fs::remove_dir_all(root).expect("test tree should be removed");
    }

    #[test]
    fn validates_missing_resources_before_creating_the_export_folder() {
        let root = std::env::temp_dir().join(format!(
            "markra-markdown-export-missing-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after epoch")
                .as_nanos()
        ));
        let markdown = "![Missing](assets/missing.png)";
        let note = root.join("vault/draft.md");
        let export_parent = root.join("exports");

        fs::create_dir_all(note.parent().expect("note should have a parent"))
            .expect("vault should be created");
        fs::create_dir_all(&export_parent).expect("export parent should be created");
        fs::write(&note, markdown).expect("source note should be written");

        let result = export_markdown_file_with_importer(
            export_parent.to_string_lossy().to_string(),
            "draft.md".to_string(),
            markdown.to_string(),
            note.to_string_lossy().to_string(),
            Some(root.join("vault").to_string_lossy().to_string()),
            "assets".to_string(),
            vec![markdown_export_reference(markdown, "assets/missing.png")],
            |_source_path, _target_document_path, _folder, _export_directory| {
                panic!("resource importer should not run after validation fails")
            },
        );

        assert!(result
            .expect_err("missing resource should reject export")
            .contains("Could not read Markdown resource"));
        assert!(!export_parent.join("draft").exists());

        fs::remove_dir_all(root).expect("test tree should be removed");
    }
}
