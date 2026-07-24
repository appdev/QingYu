use std::path::{Path, PathBuf};

fn unavailable_root() -> String {
    "workspace-membership-unavailable: The workspace identity is unavailable.".to_string()
}

fn unavailable_document() -> String {
    "workspace-document-membership-unavailable: The saved document identity is unavailable."
        .to_string()
}

pub(crate) fn canonical_workspace_root(root_path: &str) -> Result<PathBuf, String> {
    let root = Path::new(root_path)
        .canonicalize()
        .map_err(|_| unavailable_root())?;
    if !root.metadata().map_err(|_| unavailable_root())?.is_dir() {
        return Err(unavailable_root());
    }
    Ok(root)
}

pub(crate) fn is_document_in_root(root_path: &str, document_path: &str) -> Result<bool, String> {
    let root = canonical_workspace_root(root_path)?;
    let document = Path::new(document_path)
        .canonicalize()
        .map_err(|_| unavailable_document())?;
    let metadata = document.metadata().map_err(|_| unavailable_document())?;
    Ok(metadata.is_file() && document != root && document.starts_with(&root))
}

#[tauri::command]
pub(crate) fn is_document_in_workspace(
    root_path: String,
    document_path: String,
) -> Result<bool, String> {
    is_document_in_root(&root_path, &document_path)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::is_document_in_root;

    #[test]
    fn membership_accepts_only_regular_documents_inside_the_canonical_root() {
        let root = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let inside = root.path().join("inside.md");
        let external = outside.path().join("outside.md");
        fs::write(&inside, b"inside").unwrap();
        fs::write(&external, b"outside").unwrap();
        fs::create_dir(root.path().join("folder")).unwrap();

        assert!(
            is_document_in_root(root.path().to_str().unwrap(), inside.to_str().unwrap()).unwrap()
        );
        assert!(
            !is_document_in_root(root.path().to_str().unwrap(), external.to_str().unwrap())
                .unwrap()
        );
        assert!(!is_document_in_root(
            root.path().to_str().unwrap(),
            root.path().join("folder").to_str().unwrap()
        )
        .unwrap());
    }

    #[test]
    fn membership_rejects_traversal_and_unavailable_paths_with_safe_errors() {
        let root = tempdir().unwrap();
        let error = is_document_in_root(
            root.path().to_str().unwrap(),
            root.path().join("../missing-secret.md").to_str().unwrap(),
        )
        .unwrap_err();

        assert_eq!(
            error,
            "workspace-document-membership-unavailable: The saved document identity is unavailable."
        );
        assert!(!error.contains("missing-secret"));
    }

    #[cfg(unix)]
    #[test]
    fn membership_rejects_a_symlink_that_escapes_the_workspace() {
        use std::os::unix::fs::symlink;

        let root = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let external = outside.path().join("outside.md");
        let link = root.path().join("escaped.md");
        fs::write(&external, b"outside").unwrap();
        symlink(&external, &link).unwrap();

        assert!(
            !is_document_in_root(root.path().to_str().unwrap(), link.to_str().unwrap()).unwrap()
        );
    }
}
