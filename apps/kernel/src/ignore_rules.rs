//! Workspace ignore rules shared by document discovery, watchers, and sync.

use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use ignore::gitignore::{Gitignore, GitignoreBuilder};

use crate::protected_paths::path_contains_qingyu_control_directory;

pub const MARKRA_IGNORE_FILE_NAME: &str = ".markraignore";

fn is_builtin_ignored_directory_name(name: &OsStr) -> bool {
    name.to_str().is_some_and(|name| {
        matches!(
            name,
            ".codex" | ".git" | ".obsidian" | "build" | "dist" | "node_modules" | "target"
        )
    })
}

#[derive(Debug)]
pub struct MarkdownIgnoreRules {
    global_rules: String,
    include_workspace_rules: bool,
    root: PathBuf,
    matcher: Gitignore,
}

impl MarkdownIgnoreRules {
    pub fn for_root(root: &Path, global_rules: Option<&str>) -> Self {
        Self::build(root, global_rules, true)
    }

    pub fn built_in_only(root: &Path) -> Self {
        Self::build(root, None, false)
    }

    pub fn for_retained_root(root: &Path, directory: &Dir, global_rules: Option<&str>) -> Self {
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        let workspace_rules = directory
            .open_with(MARKRA_IGNORE_FILE_NAME, &options)
            .ok()
            .and_then(|mut file| {
                if file.metadata().ok()?.len() > 1024 * 1024 {
                    return None;
                }
                let mut rules = String::new();
                file.read_to_string(&mut rules).ok().map(|_| rules)
            });
        Self::from_rules(root, global_rules, workspace_rules.as_deref(), true)
    }

    fn build(root: &Path, global_rules: Option<&str>, include_workspace_rules: bool) -> Self {
        let workspace_rules = include_workspace_rules
            .then(|| std::fs::read_to_string(root.join(MARKRA_IGNORE_FILE_NAME)).ok())
            .flatten();
        Self::from_rules(
            root,
            global_rules,
            workspace_rules.as_deref(),
            include_workspace_rules,
        )
    }

    fn from_rules(
        root: &Path,
        global_rules: Option<&str>,
        workspace_rules: Option<&str>,
        include_workspace_rules: bool,
    ) -> Self {
        let global_rules = global_rules.unwrap_or_default().to_string();
        let mut builder = GitignoreBuilder::new(root);

        for line in global_rules.lines() {
            let _ignored_invalid_pattern = builder.add_line(None, line);
        }
        if include_workspace_rules {
            let workspace_rules_path = root.join(MARKRA_IGNORE_FILE_NAME);
            for line in workspace_rules.unwrap_or_default().lines() {
                let _ignored_invalid_pattern =
                    builder.add_line(Some(workspace_rules_path.clone()), line);
            }
        }
        let matcher = builder.build().unwrap_or_else(|_| Gitignore::empty());

        Self {
            global_rules,
            include_workspace_rules,
            root: root.to_path_buf(),
            matcher,
        }
    }

    pub fn reload(&mut self) {
        let root = self.root.clone();
        let global_rules = self.global_rules.clone();
        *self = Self::build(&root, Some(&global_rules), self.include_workspace_rules);
    }

    pub fn ignores(&self, path: &Path, is_directory: bool) -> bool {
        if path_contains_qingyu_control_directory(path) {
            return true;
        }

        let Ok(relative_path) = path.strip_prefix(&self.root) else {
            return false;
        };

        if self.is_control_file(path) {
            return true;
        }

        let directory_path = if is_directory {
            relative_path
        } else {
            relative_path.parent().unwrap_or_else(|| Path::new(""))
        };

        if directory_path
            .components()
            .any(|component| is_builtin_ignored_directory_name(component.as_os_str()))
        {
            return true;
        }

        self.matcher
            .matched_path_or_any_parents(path, is_directory)
            .is_ignore()
    }

    pub fn is_control_file(&self, path: &Path) -> bool {
        path.parent() == Some(self.root.as_path())
            && path.file_name() == Some(OsStr::new(MARKRA_IGNORE_FILE_NAME))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    use crate::protected_paths::{LEGACY_SYNC_DIR, QINGYU_CONTROL_DIR};

    fn test_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "markra-ignore-rules-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after epoch")
                .as_nanos()
        ))
    }

    #[test]
    fn applies_global_rules_before_workspace_rules() {
        let root = test_root("precedence");
        fs::create_dir_all(&root).expect("test root should be created");
        fs::write(root.join(MARKRA_IGNORE_FILE_NAME), "!keep.md\n")
            .expect("workspace rules should be written");

        let rules = MarkdownIgnoreRules::for_root(&root, Some("*.md\n"));

        assert!(!rules.ignores(&root.join("keep.md"), false));
        assert!(rules.ignores(&root.join("drop.md"), false));

        fs::remove_dir_all(root).expect("test root should be removed");
    }

    #[test]
    fn built_in_control_directories_remain_authoritative() {
        let root = test_root("builtins");
        fs::create_dir_all(&root).expect("test root should be created");
        fs::write(
            root.join(MARKRA_IGNORE_FILE_NAME),
            "!.qingyu/\n!.qingyu/config.json\n!.markra-sync/\n!.markra-sync/manifest.json\n",
        )
        .expect("workspace rules should be written");
        let rules = MarkdownIgnoreRules::for_root(
            &root,
            Some("!.qingyu/\n!.qingyu/sync/status.json\n!.markra-sync/\n"),
        );

        assert!(rules.ignores(&root.join(".qingyu/config.json"), false));
        assert!(rules.ignores(&root.join(".qingyu/sync/status.json"), false));
        assert!(rules.ignores(&root.join(".markra-sync/manifest.json"), false));

        fs::remove_dir_all(root).expect("test root should be removed");
    }

    #[test]
    fn built_in_control_directory_ascii_case_variants_remain_authoritative() {
        let root = test_root("builtins-ascii-case-variants");
        fs::create_dir_all(&root).expect("test root should be created");
        let rules = MarkdownIgnoreRules::for_root(
            &root,
            Some("!.QINGYU/\n!.QINGYU/config.json\n!.MARKRA-SYNC/\n"),
        );

        assert!(rules.ignores(&root.join(".QINGYU/config.json"), false));
        assert!(rules.ignores(&root.join(".MARKRA-SYNC/manifest.json"), false));

        fs::remove_dir_all(root).expect("test root should be removed");
    }

    #[test]
    fn control_directory_root_remains_protected_outside_relative_matching() {
        let parent = test_root("control-root");

        for control_directory in [QINGYU_CONTROL_DIR, LEGACY_SYNC_DIR] {
            let root = parent.join(control_directory);
            let rules = MarkdownIgnoreRules::for_root(&root, Some("!note.md\n"));

            assert!(rules.ignores(&root.join("note.md"), false));
        }
    }

    #[test]
    fn built_in_only_rules_keep_user_ignored_markdown_visible() {
        let root = test_root("built-in-only");
        fs::create_dir_all(root.join("drafts")).expect("draft folder should be created");
        fs::create_dir_all(root.join(".git")).expect("git folder should be created");
        fs::write(root.join(".markraignore"), "drafts/\n").expect("ignore file should be written");

        let rules = MarkdownIgnoreRules::built_in_only(&root);

        assert!(!rules.ignores(&root.join("drafts/hidden.md"), false));
        assert!(rules.ignores(&root.join(".git/readme.md"), false));
        fs::remove_dir_all(root).expect("test root should be removed");
    }

    #[test]
    fn matches_ignore_rules_case_sensitively() {
        let root = test_root("case-sensitive");
        let rules = MarkdownIgnoreRules::for_root(&root, Some("drafts/\n"));

        assert!(rules.ignores(&root.join("drafts/note.md"), false));
        assert!(!rules.ignores(&root.join("Drafts/note.md"), false));
    }
}
