//! Compatibility helpers for Kernel-owned workspace ignore rules.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use cap_fs_ext::DirExt;
use cap_std::fs::Dir;

use crate::storage_capability::{
    directory_identity, open_canonical_directory_nofollow, DirectoryIdentity,
};

pub(crate) use qingyu_kernel::ignore_rules::MarkdownIgnoreRules;

pub(crate) struct RetainedNamedMarkdownDirectory {
    directory: Dir,
    identity: DirectoryIdentity,
    name: OsString,
    parent: Dir,
}

impl RetainedNamedMarkdownDirectory {
    pub(crate) fn open(parent: &Dir, name: &OsStr) -> Result<Self, String> {
        let directory = parent
            .open_dir_nofollow(name)
            .map_err(|_| directory_changed())?;
        let identity = directory_identity(&directory).map_err(|_| directory_changed())?;
        let retained = Self {
            directory,
            identity,
            name: name.to_os_string(),
            parent: parent.try_clone().map_err(|_| directory_changed())?,
        };
        retained.verify_current()?;
        Ok(retained)
    }

    pub(crate) fn directory(&self) -> &Dir {
        &self.directory
    }

    pub(crate) fn verify_current(&self) -> Result<(), String> {
        if directory_identity(&self.directory).map_err(|_| directory_changed())? != self.identity {
            return Err(directory_changed());
        }
        let named = self
            .parent
            .open_dir_nofollow(&self.name)
            .map_err(|_| directory_changed())?;
        if directory_identity(&named).map_err(|_| directory_changed())? != self.identity {
            return Err(directory_changed());
        }
        Ok(())
    }
}

pub(crate) struct RetainedMarkdownRoot {
    root: PathBuf,
    retained_root: Dir,
    identity: DirectoryIdentity,
}

impl RetainedMarkdownRoot {
    pub(crate) fn capture(root: &Path) -> Result<Self, String> {
        let root = root.to_path_buf();
        let retained_root = open_canonical_directory_nofollow(&root).map_err(|_| root_changed())?;
        let identity = directory_identity(&retained_root).map_err(|_| root_changed())?;
        let snapshot = Self {
            root,
            retained_root,
            identity,
        };
        snapshot.verify_current()?;
        Ok(snapshot)
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn retained_root(&self) -> &Dir {
        &self.retained_root
    }

    pub(crate) fn try_clone_root(&self) -> Result<Dir, String> {
        self.retained_root.try_clone().map_err(|_| root_changed())
    }

    pub(crate) fn identity(&self) -> DirectoryIdentity {
        self.identity
    }

    pub(crate) fn verify_current(&self) -> Result<(), String> {
        let current = open_canonical_directory_nofollow(&self.root).map_err(|_| root_changed())?;
        let current_identity = directory_identity(&current).map_err(|_| root_changed())?;
        if current_identity != self.identity {
            return Err(root_changed());
        }
        Ok(())
    }
}

pub(crate) struct RetainedMarkdownIgnoreSnapshot {
    root: RetainedMarkdownRoot,
    rules: MarkdownIgnoreRules,
}

impl RetainedMarkdownIgnoreSnapshot {
    pub(crate) fn capture(root: &Path, global_rules: Option<&str>) -> Result<Self, String> {
        let root = RetainedMarkdownRoot::capture(root)?;
        let rules = try_markdown_ignore_rules_for_retained_root(
            root.root(),
            root.retained_root(),
            global_rules,
        )?;
        root.verify_current()?;
        Ok(Self { root, rules })
    }

    pub(crate) fn root(&self) -> &RetainedMarkdownRoot {
        &self.root
    }

    pub(crate) fn rules(&self) -> &MarkdownIgnoreRules {
        &self.rules
    }

    pub(crate) fn verify_current(&self) -> Result<(), String> {
        self.root.verify_current()
    }
}

pub(crate) fn open_retained_ignore_root(root: &Path) -> Result<Dir, String> {
    Dir::open_ambient_dir(root, cap_std::ambient_authority())
        .map_err(|_| "workspace ignore rules are unavailable".to_string())
}

pub(crate) fn try_markdown_ignore_rules_for_retained_root(
    root: &Path,
    retained_root: &Dir,
    global_rules: Option<&str>,
) -> Result<MarkdownIgnoreRules, String> {
    MarkdownIgnoreRules::try_for_retained_root(root, retained_root, global_rules)
        .map_err(|error| error.to_string())
}

pub(crate) fn try_markdown_ignore_rules_for_root(
    root: &Path,
    global_rules: Option<&str>,
) -> Result<MarkdownIgnoreRules, String> {
    let retained_root = open_retained_ignore_root(root)?;
    try_markdown_ignore_rules_for_retained_root(root, &retained_root, global_rules)
}

fn root_changed() -> String {
    "workspace root changed".to_string()
}

fn directory_changed() -> String {
    "workspace directory changed".to_string()
}
