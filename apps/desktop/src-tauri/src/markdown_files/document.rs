use std::path::PathBuf;

use super::asset::allow_asset_directory;
use super::history::{
    markdown_history_root, write_markdown_file_with_history_root,
    write_markdown_file_with_optional_history_root,
};
use super::trusted_file::read_trusted_markdown_file;
use super::types::MarkdownFile;

#[tauri::command]
pub(crate) fn read_markdown_file(
    app: tauri::AppHandle,
    path: String,
) -> Result<MarkdownFile, String> {
    let path_buf = PathBuf::from(&path);
    let file = read_trusted_markdown_file(&path_buf)?;
    if let Some(parent) = path_buf.parent() {
        allow_asset_directory(&app, parent)?;
    }
    Ok(file)
}

#[tauri::command]
pub(crate) fn write_markdown_file(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    path: String,
    contents: String,
    skip_history_snapshot: Option<bool>,
    history_cursor_id: Option<String>,
    sync_path_guard_request_id: Option<String>,
) -> Result<(), String> {
    let _mutation = acquire_document_write_guard(
        crate::dejavu_sync::path_guard::native_working_tree_registry(),
        &path,
        window.label(),
        sync_path_guard_request_id.as_deref(),
    )?;
    match markdown_history_root(&app) {
        Ok(history_root) if skip_history_snapshot.unwrap_or(false) => {
            write_markdown_file_with_optional_history_root(
                Some(&history_root),
                path,
                contents,
                true,
                history_cursor_id,
            )
        }
        Ok(history_root) => write_markdown_file_with_history_root(&history_root, path, contents),
        Err(_) => write_markdown_file_with_optional_history_root(None, path, contents, false, None),
    }
}

#[cfg(desktop)]
#[tauri::command]
pub(crate) fn write_markdown_export_file(path: String, contents: String) -> Result<(), String> {
    write_markdown_export_file_with_registry(
        crate::dejavu_sync::path_guard::native_working_tree_registry(),
        path,
        contents,
    )
}

#[cfg(desktop)]
fn write_markdown_export_file_with_registry(
    registry: &std::sync::Arc<crate::dejavu_sync::path_guard::NativeWorkingTreeRegistry>,
    path: String,
    contents: String,
) -> Result<(), String> {
    let _mutation = registry.acquire_mutation(&[PathBuf::from(&path)])?;
    write_markdown_file_with_optional_history_root(None, path, contents, false, None)
}

fn acquire_document_write_guard(
    registry: &std::sync::Arc<crate::dejavu_sync::path_guard::NativeWorkingTreeRegistry>,
    path: &str,
    window_label: &str,
    request_id: Option<&str>,
) -> Result<crate::dejavu_sync::path_guard::NativeMutationLease, String> {
    let paths = [PathBuf::from(path)];
    match request_id {
        Some(request_id) => registry.acquire_authorized_mutation(&paths, window_label, request_id),
        None => registry.acquire_mutation(&paths),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tempfile::tempdir;

    use super::{acquire_document_write_guard, write_markdown_export_file_with_registry};
    use crate::dejavu_sync::path_guard::NativeWorkingTreeRegistry;

    #[test]
    fn save_as_rejects_an_existing_native_guarded_destination_before_writing() {
        let directory = tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let destination = root.join("existing.md");
        std::fs::write(&destination, "old").unwrap();
        let registry = Arc::new(NativeWorkingTreeRegistry::default());
        let _block = registry
            .block_paths(&root, &["existing.md".to_string()])
            .unwrap();

        assert_eq!(
            acquire_document_write_guard(&registry, destination.to_str().unwrap(), "main", None,)
                .unwrap_err(),
            "sync-path-guarded"
        );
        assert_eq!(std::fs::read_to_string(destination).unwrap(), "old");
    }

    #[test]
    fn primary_request_can_flush_its_exact_guarded_document() {
        let directory = tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let destination = root.join("dirty.md");
        std::fs::write(&destination, "dirty").unwrap();
        let registry = Arc::new(NativeWorkingTreeRegistry::default());
        let request_id = "e728a5d6-31ed-490d-bb8a-8f15cb550e74";
        let _block = registry
            .block_paths_for_request(&root, &["dirty.md".to_string()], "main", request_id)
            .unwrap();

        assert!(acquire_document_write_guard(
            &registry,
            destination.to_str().unwrap(),
            "main",
            Some(request_id),
        )
        .is_ok());
    }

    #[test]
    fn html_export_uses_an_ordinary_native_lease() {
        let directory = tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let guarded = root.join("guarded.html");
        let other = root.join("other.html");
        let registry = Arc::new(NativeWorkingTreeRegistry::default());
        let _block = registry
            .block_paths(&root, &["guarded.html".to_string()])
            .unwrap();
        assert_eq!(
            write_markdown_export_file_with_registry(
                &registry,
                guarded.to_string_lossy().to_string(),
                "guarded".to_string()
            )
            .unwrap_err(),
            "sync-path-guarded"
        );
        assert!(!guarded.exists());
        write_markdown_export_file_with_registry(
            &registry,
            other.to_string_lossy().to_string(),
            "other".to_string(),
        )
        .unwrap();
        assert_eq!(std::fs::read_to_string(other).unwrap(), "other");
    }
}
