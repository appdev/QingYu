use std::fs;
use std::path::PathBuf;

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TextFile {
    pub(crate) path: String,
    pub(crate) contents: String,
}

#[tauri::command]
pub(crate) fn read_text_file(path: String) -> Result<TextFile, String> {
    let path_buf = PathBuf::from(&path);
    let contents = fs::read_to_string(&path_buf).map_err(|error| error.to_string())?;

    Ok(TextFile { path, contents })
}

pub(crate) fn write_text_file(path: String, contents: String) -> Result<(), String> {
    write_text_file_with_registry(
        crate::dejavu_sync::path_guard::native_working_tree_registry(),
        path,
        contents,
    )
}

fn write_text_file_with_registry(
    registry: &std::sync::Arc<crate::dejavu_sync::path_guard::NativeWorkingTreeRegistry>,
    path: String,
    contents: String,
) -> Result<(), String> {
    let _mutation = registry.acquire_mutation(&[PathBuf::from(&path)])?;
    fs::write(PathBuf::from(path), contents).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::write_text_file_with_registry;
    use std::sync::Arc;

    #[test]
    fn text_export_uses_an_ordinary_native_lease() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let guarded = root.join("guarded.txt");
        let other = root.join("other.txt");
        let registry =
            Arc::new(crate::dejavu_sync::path_guard::NativeWorkingTreeRegistry::default());
        let _block = registry
            .block_paths(&root, &["guarded.txt".to_string()])
            .unwrap();
        assert_eq!(
            write_text_file_with_registry(
                &registry,
                guarded.to_string_lossy().to_string(),
                "guarded".to_string()
            )
            .unwrap_err(),
            "sync-path-guarded"
        );
        assert!(!guarded.exists());
        write_text_file_with_registry(
            &registry,
            other.to_string_lossy().to_string(),
            "other".to_string(),
        )
        .unwrap();
        assert_eq!(std::fs::read_to_string(other).unwrap(), "other");
    }
}
