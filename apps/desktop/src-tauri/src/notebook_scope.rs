use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};

use crate::{
    protected_paths::is_qingyu_control_directory_name,
    remote_sync::{sync_state_key, ValidRemoteRoot},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NotebookSyncScope {
    pub(crate) canonical_root: PathBuf,
    pub(crate) name: String,
    pub(crate) remote_prefix: String,
    pub(crate) state_root: PathBuf,
}

pub(crate) fn notebook_state_key(
    target_fingerprint_source: &str,
    remote_root: &str,
    notebook_name: &str,
    canonical_local_root: &Path,
) -> String {
    sync_state_key(
        "notes",
        &[
            target_fingerprint_source.as_bytes(),
            remote_root.as_bytes(),
            notebook_name.as_bytes(),
            canonical_local_root.as_os_str().as_encoded_bytes(),
        ],
    )
}

pub(crate) fn resolve_notebook_sync_scope(
    target_fingerprint_source: &str,
    remote_root: &ValidRemoteRoot,
    local_root: &Path,
    sync_state_root: &Path,
) -> Result<NotebookSyncScope, String> {
    let canonical_root = local_root
        .canonicalize()
        .map_err(|_| "notes-root-unavailable: The notes root is unavailable.".to_string())?;
    resolve_notebook_sync_scope_from_canonical(
        target_fingerprint_source,
        remote_root,
        canonical_root,
        sync_state_root,
    )
}

pub(crate) fn resolve_notebook_sync_scope_from_canonical(
    target_fingerprint_source: &str,
    remote_root: &ValidRemoteRoot,
    canonical_root: PathBuf,
    sync_state_root: &Path,
) -> Result<NotebookSyncScope, String> {
    let name = notebook_name_from_root(&canonical_root)?;
    let remote_prefix = notes_remote_prefix(remote_root, &name)?;
    let key = notebook_state_key(
        target_fingerprint_source,
        remote_root.as_str(),
        &name,
        &canonical_root,
    );
    Ok(NotebookSyncScope {
        canonical_root,
        name,
        remote_prefix,
        state_root: sync_state_root.join("notes").join(key),
    })
}

#[allow(dead_code)]
pub(crate) fn validate_notebook_name(name: &str) -> Result<String, String> {
    if name.is_empty()
        || matches!(name, "." | "..")
        || name.contains(['/', '\\', '\0'])
        || is_qingyu_control_directory_name(OsStr::new(name))
    {
        return Err("notebook-name-invalid: The notebook name is invalid.".to_string());
    }

    Ok(name.to_string())
}

#[allow(dead_code)]
pub(crate) fn notebook_name_from_root(root: &Path) -> Result<String, String> {
    let name = root
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| "notebook-name-invalid: The notebook name is invalid.".to_string())?;
    validate_notebook_name(name)
}

#[allow(dead_code)]
pub(crate) fn notes_remote_prefix(root: &ValidRemoteRoot, name: &str) -> Result<String, String> {
    let name = validate_notebook_name(name)?;
    Ok(format!("{}/{}", root.notes_prefix(), name))
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use crate::remote_sync::ValidRemoteRoot;
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn notebook_name_validation_rejects_unsafe_or_protected_segments() {
        for invalid in [
            "",
            ".",
            "..",
            "team/notes",
            r"team\notes",
            ".qingyu",
            ".markra-sync",
        ] {
            assert!(
                validate_notebook_name(invalid).is_err(),
                "accepted {invalid:?}"
            );
        }
    }

    #[test]
    fn notebook_name_validation_preserves_unicode_and_ordinary_spaces() {
        let name = "  个人 笔记  ";

        assert_eq!(validate_notebook_name(name).unwrap(), name);
        assert_eq!(
            notebook_name_from_root(Path::new("/notes/  个人 笔记  ")).unwrap(),
            name
        );
    }

    #[test]
    fn notebook_name_validation_rejects_case_variants_and_internal_staging_names() {
        for name in [".QINGYU", ".MARKRA-SYNC", ".markra-sync-stage-123"] {
            assert!(validate_notebook_name(name).is_err(), "accepted {name:?}");
            assert!(
                notebook_name_from_root(Path::new("/notes").join(name).as_path()).is_err(),
                "derived protected notebook name {name:?}"
            );
        }
    }

    #[test]
    fn notebook_remote_prefix_keeps_the_validated_logical_name_exact() {
        let root = ValidRemoteRoot::parse("qingyu/team").unwrap();

        assert_eq!(
            notes_remote_prefix(&root, "A").unwrap(),
            "qingyu/team/notes/A"
        );
        assert_eq!(
            notes_remote_prefix(&root, "  个人 笔记  ").unwrap(),
            "qingyu/team/notes/  个人 笔记  "
        );
    }

    #[test]
    fn notebook_state_key_is_stable_for_one_tuple_and_isolates_roots_and_targets() {
        let directory = tempdir().unwrap();
        let first_parent = directory.path().join("first");
        let second_parent = directory.path().join("second");
        let first = first_parent.join("A");
        let second = second_parent.join("A");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();
        let first = first.canonicalize().unwrap();
        let second = second.canonicalize().unwrap();

        let original = notebook_state_key("s3|target-a", "root", "A", &first);
        assert_eq!(
            notebook_state_key("s3|target-a", "root", "A", &first),
            original,
            "switching back to the exact same tuple must resume its state"
        );
        assert_ne!(
            notebook_state_key("s3|target-a", "root", "A", &second),
            original,
            "equal basenames at different canonical roots need separate state"
        );
        assert_ne!(
            notebook_state_key("s3|target-b", "root", "A", &first),
            original,
            "different provider targets need separate state"
        );
    }

    #[test]
    fn resolved_notebook_scope_uses_the_exact_name_and_hashed_notes_state_directory() {
        let directory = tempdir().unwrap();
        let notebook = directory.path().join("  个人 笔记  ");
        fs::create_dir(&notebook).unwrap();
        let canonical = notebook.canonicalize().unwrap();
        let state = directory.path().join("sync-state");
        let remote_root = ValidRemoteRoot::parse("root").unwrap();

        let scope = resolve_notebook_sync_scope(
            "webdav|https://dav.example.test/root/notes/%20%20%E4%B8%AA%E4%BA%BA%20%E7%AC%94%E8%AE%B0%20%20/",
            &remote_root,
            &canonical,
            &state,
        )
        .unwrap();

        assert_eq!(scope.canonical_root, canonical);
        assert_eq!(scope.name, "  个人 笔记  ");
        assert_eq!(scope.remote_prefix, "root/notes/  个人 笔记  ");
        assert_eq!(
            scope.state_root.parent(),
            Some(state.join("notes").as_path())
        );
        assert_eq!(scope.state_root.file_name().unwrap().len(), 64);
    }
}
