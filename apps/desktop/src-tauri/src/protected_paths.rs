use std::ffi::OsStr;
use std::path::Path;

pub(crate) const QINGYU_CONTROL_DIR: &str = ".qingyu";
pub(crate) const LEGACY_SYNC_DIR: &str = ".markra-sync";
pub(crate) const SYNC_MUTATION_STAGING_PREFIX: &str = ".markra-sync-stage-";

fn is_reserved_control_directory_name(name: &str) -> bool {
    name.eq_ignore_ascii_case(QINGYU_CONTROL_DIR)
        || name.eq_ignore_ascii_case(LEGACY_SYNC_DIR)
        || name
            .get(..SYNC_MUTATION_STAGING_PREFIX.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(SYNC_MUTATION_STAGING_PREFIX))
}

pub(crate) fn is_qingyu_control_directory_name(name: &OsStr) -> bool {
    name.to_str()
        .is_some_and(is_reserved_control_directory_name)
}

pub(crate) fn path_contains_qingyu_control_directory(path: &Path) -> bool {
    path.components()
        .any(|component| is_qingyu_control_directory_name(component.as_os_str()))
}

pub(crate) fn is_protected_sync_relative_path(path: &str) -> bool {
    path.split('/').any(is_reserved_control_directory_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protects_control_and_mutation_staging_names_case_insensitively() {
        for name in [
            ".qingyu",
            ".QINGYU",
            ".markra-sync",
            ".MARKRA-SYNC",
            ".markra-sync-stage-123",
            ".MaRkRa-SyNc-StAgE-publish-123",
        ] {
            assert!(is_qingyu_control_directory_name(OsStr::new(name)), "{name}");
            assert!(
                is_protected_sync_relative_path(&format!("notes/{name}/private.json")),
                "{name}"
            );
        }
    }

    #[test]
    fn does_not_overmatch_similar_user_names() {
        for name in [".qingyu-notes", ".markra-sync-notes", ".markra-sync-stage"] {
            assert!(
                !is_qingyu_control_directory_name(OsStr::new(name)),
                "{name}"
            );
        }
    }
}
