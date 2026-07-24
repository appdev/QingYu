use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(desktop)]
use super::types::MarkdownOpenPath;
use super::types::{MarkdownFolderEntryKind, MarkdownFolderFile};

pub(super) fn is_markdown_tree_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(extension.to_ascii_lowercase().as_str(), "md" | "markdown")
        })
}

pub(super) fn is_markdown_history_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "md" | "markdown" | "txt"
            )
        })
}

pub(super) fn is_markdown_tree_asset_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "avif" | "bmp" | "gif" | "jpg" | "jpeg" | "png" | "svg" | "webp"
            )
        })
}

pub(super) fn is_markdown_tree_attachment_file(path: &Path) -> bool {
    !is_markdown_tree_file(path) && !is_markdown_tree_asset_file(path)
}

pub(super) fn markdown_tree_file_kind(path: &Path) -> Result<MarkdownFolderEntryKind, String> {
    if is_markdown_tree_file(path) {
        return Ok(MarkdownFolderEntryKind::File);
    }

    if is_markdown_tree_asset_file(path) {
        return Ok(MarkdownFolderEntryKind::Asset);
    }

    Ok(MarkdownFolderEntryKind::Attachment)
}

pub(super) fn is_markdown_open_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "md" | "markdown" | "txt"
            )
        })
}

pub(super) fn path_to_string(path: &Path) -> String {
    let path_text = path.to_string_lossy();

    if let Some(unc_path) = path_text.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{unc_path}");
    }

    if let Some(local_path) = path_text.strip_prefix(r"\\?\") {
        return local_path.to_string();
    }

    path_text.to_string()
}

#[cfg(desktop)]
#[tauri::command]
pub(crate) fn canonical_local_file_path(path: String) -> Result<String, String> {
    let canonical_path = PathBuf::from(path)
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if !canonical_path.is_file() {
        return Err("Local resource path must be an existing file".to_string());
    }

    Ok(path_to_string(&canonical_path))
}

fn system_time_millis(time: SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| duration.as_millis().try_into().ok())
}

fn file_metadata_time_millis(
    metadata: &fs::Metadata,
    read_time: impl FnOnce(&fs::Metadata) -> std::io::Result<SystemTime>,
) -> Option<u64> {
    read_time(metadata).ok().and_then(system_time_millis)
}

#[cfg(desktop)]
pub(crate) fn markdown_open_path_for_path(path: &Path) -> Result<MarkdownOpenPath, String> {
    if path.is_dir() {
        return Ok(MarkdownOpenPath::Folder {
            path: path_to_string(path),
        });
    }

    if path.is_file() && is_markdown_open_file(path) {
        return Ok(MarkdownOpenPath::File {
            path: path_to_string(path),
        });
    }

    Err("Selected path is not a supported Markdown file or folder".to_string())
}

pub(super) fn markdown_tree_relative_path(root: &Path, path: &Path) -> Result<String, String> {
    let relative_path = path.strip_prefix(root).map_err(|error| error.to_string())?;
    let parts = relative_path
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>();

    Ok(parts.join("/"))
}

pub(super) fn markdown_relative_path(
    from_directory: &Path,
    target: &Path,
) -> Result<String, String> {
    let from_components = from_directory.components().collect::<Vec<_>>();
    let target_components = target.components().collect::<Vec<_>>();
    let common_components = from_components
        .iter()
        .zip(&target_components)
        .take_while(|(left, right)| left == right)
        .count();
    if common_components == 0 {
        return Err("Resource and Markdown document must use the same filesystem root".to_string());
    }

    let mut parts = Vec::new();
    for component in &from_components[common_components..] {
        if matches!(component, std::path::Component::Normal(_)) {
            parts.push("..".to_string());
        }
    }
    parts.extend(
        target_components[common_components..]
            .iter()
            .filter_map(|component| match component {
                std::path::Component::Normal(part) => Some(part.to_string_lossy().to_string()),
                _ => None,
            }),
    );

    if parts.is_empty() {
        return Err("Resource path must name a file".to_string());
    }
    Ok(parts.join("/"))
}

pub(super) fn markdown_folder_file(
    root: &Path,
    path: &Path,
    kind: MarkdownFolderEntryKind,
) -> Result<MarkdownFolderFile, String> {
    let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
    let modified_at = file_metadata_time_millis(&metadata, fs::Metadata::modified);
    let created_at = file_metadata_time_millis(&metadata, fs::Metadata::created).or(modified_at);

    Ok(MarkdownFolderFile {
        created_at,
        kind,
        modified_at,
        path: path_to_string(path),
        relative_path: markdown_tree_relative_path(root, path)?,
        size_bytes: if metadata.is_file() {
            Some(metadata.len())
        } else {
            None
        },
    })
}

pub(super) fn markdown_tree_root_for_path(path: &Path) -> Result<PathBuf, String> {
    if path.is_dir() {
        return Ok(path.to_path_buf());
    }

    if path.is_file() {
        return path
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| "Current Markdown folder is invalid".to_string());
    }

    if is_markdown_open_file(path) || is_markdown_tree_asset_file(path) {
        let parent = path
            .parent()
            .ok_or_else(|| "Current Markdown folder is invalid".to_string())?;
        if parent.is_dir() {
            return Ok(parent.to_path_buf());
        }
    }

    Err("Markdown folder no longer exists".to_string())
}

pub(super) fn normalize_markdown_tree_single_file_name(file_name: &str) -> Result<String, String> {
    let trimmed_name = file_name.trim();
    if trimmed_name.is_empty() {
        return Err("File name is required".to_string());
    }

    let candidate = Path::new(trimmed_name);
    if candidate.components().count() != 1
        || trimmed_name.contains('/')
        || trimmed_name.contains('\\')
    {
        return Err("File name cannot include folders".to_string());
    }

    let Some(stem) = candidate.file_stem().and_then(|stem| stem.to_str()) else {
        return Err("File name is invalid".to_string());
    };

    if stem.trim().is_empty() || matches!(trimmed_name, "." | "..") {
        return Err("File name is invalid".to_string());
    }

    Ok(trimmed_name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_to_string_omits_windows_verbatim_prefixes() {
        assert_eq!(
            path_to_string(Path::new(r"\\?\C:\vault\Daily note.md")),
            r"C:\vault\Daily note.md"
        );
        assert_eq!(
            path_to_string(Path::new(r"\\?\UNC\server\share\Daily note.md")),
            r"\\server\share\Daily note.md"
        );
    }

    #[test]
    fn creates_slash_normalized_markdown_paths_from_nested_documents() {
        assert_eq!(
            markdown_relative_path(
                Path::new("/project/notes"),
                Path::new("/project/assets/diagram.png")
            )
            .expect("project paths should be related"),
            "../assets/diagram.png"
        );
    }

    #[test]
    fn resolves_existing_local_files_to_canonical_paths() {
        let root = std::env::temp_dir().join(format!(
            "qingyu-local-reference-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock should be after epoch")
                .as_nanos()
        ));
        let file = root.join("Reference file.pdf");
        fs::create_dir_all(&root).expect("fixture should be created");
        fs::write(&file, [1, 2, 3]).expect("file should be created");

        assert_eq!(
            canonical_local_file_path(file.to_string_lossy().to_string())
                .expect("file should canonicalize"),
            path_to_string(&file.canonicalize().expect("fixture should canonicalize"))
        );
        assert!(canonical_local_file_path(root.to_string_lossy().to_string()).is_err());
        fs::remove_dir_all(root).expect("fixture should be removed");
    }
}
