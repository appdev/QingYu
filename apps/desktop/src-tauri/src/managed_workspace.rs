use std::path::Path;

#[cfg(any(mobile, test))]
use std::path::PathBuf;

use tauri::Manager;

#[cfg(any(mobile, test))]
fn managed_workspace_path(app_data_root: &Path, name: &str) -> Result<PathBuf, String> {
    let name = crate::notebook_scope::validate_notebook_name(name)?;
    Ok(app_data_root.join("workspaces").join(name))
}

#[cfg(any(mobile, test))]
pub(crate) fn ensure_managed_workspace_path(
    app_data_root: &Path,
    name: &str,
) -> Result<PathBuf, String> {
    let name = crate::notebook_scope::validate_notebook_name(name)?;
    std::fs::create_dir_all(app_data_root).map_err(|error| error.to_string())?;
    let canonical_app_data = app_data_root
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let collection_root = app_data_root.join("workspaces");

    match std::fs::symlink_metadata(&collection_root) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err("Managed workspace is outside persistent app data".to_string());
        }
        Ok(metadata) if !metadata.is_dir() => {
            return Err("Managed workspace collection is not a directory".to_string());
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir(&collection_root).map_err(|error| error.to_string())?;
        }
        Err(error) => return Err(error.to_string()),
    }

    let canonical_collection = collection_root
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if canonical_collection
        .strip_prefix(&canonical_app_data)
        .ok()
        .filter(|relative| *relative == Path::new("workspaces"))
        .is_none()
    {
        return Err("Managed workspace is outside persistent app data".to_string());
    }

    let workspace_root = managed_workspace_path(app_data_root, &name)?;

    match std::fs::symlink_metadata(&workspace_root) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err("Managed workspace is outside persistent app data".to_string());
        }
        Ok(metadata) if !metadata.is_dir() => {
            return Err("Managed workspace path is not a directory".to_string());
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir(&workspace_root).map_err(|error| error.to_string())?;
        }
        Err(error) => return Err(error.to_string()),
    }

    let canonical_workspace = workspace_root
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if canonical_workspace
        .strip_prefix(&canonical_app_data)
        .ok()
        .filter(|relative| *relative == Path::new("workspaces").join(&name))
        .is_none()
    {
        return Err("Managed workspace is outside persistent app data".to_string());
    }

    Ok(canonical_workspace)
}

pub(crate) fn list_managed_workspace_names_at(app_data_root: &Path) -> Result<Vec<String>, String> {
    let collection_root = app_data_root.join("workspaces");
    let collection_metadata = match std::fs::symlink_metadata(&collection_root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.to_string()),
    };
    if collection_metadata.file_type().is_symlink() {
        return Err("Managed workspace is outside persistent app data".to_string());
    }
    if !collection_metadata.is_dir() {
        return Err("Managed workspace collection is not a directory".to_string());
    }

    let mut names = Vec::new();
    for entry in std::fs::read_dir(&collection_root).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let metadata =
            std::fs::symlink_metadata(entry.path()).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if crate::notebook_scope::validate_notebook_name(&name).is_ok() {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

#[tauri::command]
pub(crate) fn list_managed_workspace_names<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<Vec<String>, String> {
    let app_data_root = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    list_managed_workspace_names_at(&app_data_root)
}

#[tauri::command]
pub(crate) fn resolve_managed_workspace_root<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    name: String,
) -> Result<Option<String>, String> {
    #[cfg(mobile)]
    {
        let app_data_root = app
            .path()
            .app_data_dir()
            .map_err(|error| error.to_string())?;
        let workspace_root = ensure_managed_workspace_path(&app_data_root, &name)?;

        Ok(Some(workspace_root.to_string_lossy().to_string()))
    }

    #[cfg(not(mobile))]
    {
        let _ = (app, name);
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn managed_workspace_is_a_child_of_persistent_app_data() {
        let root = PathBuf::from("/app-data");
        assert_eq!(
            managed_workspace_path(&root, "personal").unwrap(),
            root.join("workspaces/personal")
        );
    }

    #[test]
    fn managed_workspace_is_created_persistently_and_resolves_stably() {
        let temporary = tempdir().expect("temporary app data parent should be created");
        let app_data_root = temporary.path().join("app-data");
        let expected = app_data_root.join("workspaces/personal");

        let first = ensure_managed_workspace_path(&app_data_root, "personal")
            .expect("managed workspace should be created");
        let expected = expected
            .canonicalize()
            .expect("managed workspace should canonicalize");
        let marker = first.join("retained.md");
        std::fs::write(&marker, "# Retained").expect("workspace marker should be written");
        let second = ensure_managed_workspace_path(&app_data_root, "personal")
            .expect("managed workspace should resolve after relaunch");

        assert_eq!(first, expected);
        assert_eq!(second, expected);
        assert_eq!(
            std::fs::read_to_string(marker).expect("workspace marker should persist"),
            "# Retained"
        );
    }

    #[cfg(unix)]
    #[test]
    fn managed_workspace_returns_the_canonical_root_for_an_aliased_app_data_path() {
        use std::os::unix::fs::symlink;

        let temporary = tempdir().expect("temporary app data parent should be created");
        let canonical_app_data = temporary.path().join("canonical-app-data");
        let aliased_app_data = temporary.path().join("aliased-app-data");
        std::fs::create_dir(&canonical_app_data).expect("canonical app data should be created");
        symlink(&canonical_app_data, &aliased_app_data).expect("app data alias should be created");

        let workspace = ensure_managed_workspace_path(&aliased_app_data, "personal")
            .expect("managed workspace should resolve through the alias");

        assert_eq!(
            workspace,
            canonical_app_data
                .join("workspaces/personal")
                .canonicalize()
                .expect("managed workspace should canonicalize")
        );
    }

    #[cfg(unix)]
    #[test]
    fn managed_workspace_rejects_a_collection_symlink_that_escapes_app_data() {
        use std::os::unix::fs::symlink;

        let temporary = tempdir().expect("temporary root should be created");
        let app_data_root = temporary.path().join("app-data");
        let outside = temporary.path().join("outside");
        std::fs::create_dir_all(&app_data_root).expect("app data root should be created");
        std::fs::create_dir_all(&outside).expect("outside root should be created");
        symlink(&outside, app_data_root.join("workspaces"))
            .expect("escaping workspaces symlink should be created");

        let error = ensure_managed_workspace_path(&app_data_root, "personal")
            .expect_err("escaping workspaces symlink must be rejected");

        assert!(error.contains("outside persistent app data"));
        assert!(
            std::fs::read_dir(&outside)
                .expect("outside root should remain readable")
                .next()
                .is_none(),
            "rejecting the workspace must not write through the symlink"
        );
    }

    #[cfg(unix)]
    #[test]
    fn managed_workspace_rejects_a_child_symlink_without_writing_through_it() {
        use std::os::unix::fs::symlink;

        let temporary = tempdir().expect("temporary root should be created");
        let app_data_root = temporary.path().join("app-data");
        let outside = temporary.path().join("outside");
        std::fs::create_dir_all(app_data_root.join("workspaces"))
            .expect("workspaces collection should be created");
        std::fs::create_dir_all(&outside).expect("outside root should be created");
        symlink(&outside, app_data_root.join("workspaces/personal"))
            .expect("escaping child symlink should be created");

        let error = ensure_managed_workspace_path(&app_data_root, "personal")
            .expect_err("escaping child symlink must be rejected");

        assert!(error.contains("outside persistent app data"));
        assert!(
            std::fs::read_dir(&outside)
                .expect("outside root should remain readable")
                .next()
                .is_none(),
            "rejecting the child must not write through the symlink"
        );
    }

    #[test]
    fn managed_workspace_rejects_non_directory_collection_and_child_entries() {
        let temporary = tempdir().expect("temporary root should be created");
        let app_data_root = temporary.path().join("app-data");
        std::fs::create_dir_all(&app_data_root).expect("app data root should be created");
        std::fs::write(app_data_root.join("workspaces"), "not a directory")
            .expect("collection file should be written");
        assert!(ensure_managed_workspace_path(&app_data_root, "personal").is_err());

        std::fs::remove_file(app_data_root.join("workspaces"))
            .expect("collection file should be removed");
        std::fs::create_dir(app_data_root.join("workspaces"))
            .expect("collection directory should be created");
        std::fs::write(app_data_root.join("workspaces/personal"), "not a directory")
            .expect("child file should be written");
        assert!(ensure_managed_workspace_path(&app_data_root, "personal").is_err());
    }

    #[test]
    fn managed_workspace_names_are_shallow_exact_sorted_directories_only() {
        let temporary = tempdir().expect("temporary root should be created");
        let app_data_root = temporary.path().join("app-data");
        let collection_root = app_data_root.join("workspaces");
        std::fs::create_dir_all(collection_root.join("beta/nested"))
            .expect("nested workspace content should be created");
        std::fs::create_dir(collection_root.join("Alpha"))
            .expect("uppercase workspace should be created");
        std::fs::create_dir(collection_root.join("随笔"))
            .expect("unicode workspace should be created");
        std::fs::create_dir(collection_root.join(".qingyu"))
            .expect("protected directory should be created for filtering");
        std::fs::create_dir(collection_root.join(".markra-sync"))
            .expect("legacy protected directory should be created for filtering");
        std::fs::write(
            collection_root.join("ordinary-file.md"),
            "# not a workspace",
        )
        .expect("ordinary file should be created");

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;

            let outside = temporary.path().join("outside");
            std::fs::create_dir(&outside).expect("outside directory should be created");
            symlink(&outside, collection_root.join("linked"))
                .expect("workspace symlink should be created");
        }

        assert_eq!(
            list_managed_workspace_names_at(&app_data_root)
                .expect("managed workspace names should be listed"),
            vec!["Alpha".to_string(), "beta".to_string(), "随笔".to_string()]
        );
    }

    #[test]
    fn listing_managed_workspace_names_does_not_create_a_missing_collection() {
        let temporary = tempdir().expect("temporary root should be created");
        let app_data_root = temporary.path().join("app-data");

        assert_eq!(
            list_managed_workspace_names_at(&app_data_root)
                .expect("a missing collection should be an empty list"),
            Vec::<String>::new()
        );
        assert!(
            !app_data_root.join("workspaces").exists(),
            "read-only discovery must not create the managed workspace collection"
        );
    }
}
