// DejaVu - Data snapshot and sync.
// Copyright (c) 2022-present, b3log.org
// SPDX-License-Identifier: AGPL-3.0-only

use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use cap_std::fs::Dir;

use crate::atomic_write::stage_cap_file;
use crate::store::{
    absolute_lexical_root, open_child_directory, store_anchor, validate_store_directory,
};
use crate::RepoError;

const HISTORY_MODE: u32 = 0o644;

pub struct History {
    root: PathBuf,
    anchor: Dir,
    relative_root: PathBuf,
    history_dir: Mutex<Option<Dir>>,
}

impl History {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, RepoError> {
        let root = absolute_lexical_root(root.into())?;
        let (anchor, relative_root) = store_anchor(&root)?;
        let history_dir = if relative_root.as_os_str().is_empty() {
            Some(anchor.try_clone()?)
        } else {
            None
        };
        Ok(Self {
            root,
            anchor,
            relative_root,
            history_dir: Mutex::new(history_dir),
        })
    }

    pub(crate) fn new_with_directory(
        root: impl Into<PathBuf>,
        history_dir: Dir,
    ) -> Result<Self, RepoError> {
        let root = absolute_lexical_root(root.into())?;
        validate_store_directory(&history_dir)?;
        Ok(Self {
            root,
            anchor: history_dir.try_clone()?,
            relative_root: PathBuf::new(),
            history_dir: Mutex::new(Some(history_dir)),
        })
    }

    pub fn store_remote_conflict(
        &self,
        timestamp: &str,
        relative_path: &Path,
        bytes: &[u8],
    ) -> Result<PathBuf, RepoError> {
        validate_timestamp(timestamp)?;
        let components = validate_relative_file_path(relative_path)?;
        let snapshot_name = format!("{timestamp}-sync");
        let mut directory =
            open_child_directory(&self.open_root(true)?, snapshot_name.as_ref(), true)?;
        for component in &components[..components.len() - 1] {
            directory = open_child_directory(&directory, component, true)?;
        }
        let destination = &components[components.len() - 1];
        stage_cap_file(&directory, destination, bytes, HISTORY_MODE)?.publish_replace()?;
        Ok(self.root.join(snapshot_name).join(relative_path))
    }

    fn open_root(&self, create: bool) -> Result<Dir, RepoError> {
        let mut retained = self.lock_root()?;
        if let Some(directory) = retained.as_ref() {
            return Ok(directory.try_clone()?);
        }
        let mut directory = self.anchor.try_clone()?;
        for component in self.relative_root.components() {
            let Component::Normal(name) = component else {
                return Err(RepoError::UnsafePath);
            };
            directory = open_child_directory(&directory, name, create)?;
        }
        *retained = Some(directory.try_clone()?);
        Ok(directory)
    }

    fn lock_root(&self) -> Result<MutexGuard<'_, Option<Dir>>, RepoError> {
        self.history_dir
            .lock()
            .map_err(|_| RepoError::InvalidData("history directory handle lock poisoned"))
    }
}

fn validate_timestamp(timestamp: &str) -> Result<(), RepoError> {
    let valid = timestamp.len() == 17
        && timestamp.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 4 | 7 | 10) {
                byte == b'-'
            } else {
                byte.is_ascii_digit()
            }
        });
    if valid {
        Ok(())
    } else {
        Err(RepoError::InvalidData(
            "history timestamp must use YYYY-MM-DD-HHMMSS",
        ))
    }
}

fn validate_relative_file_path(path: &Path) -> Result<Vec<std::ffi::OsString>, RepoError> {
    if path.as_os_str().is_empty() || path.is_absolute() || path.to_string_lossy().contains('\\') {
        return Err(RepoError::UnsafePath);
    }
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(name) => components.push(name.to_os_string()),
            Component::Prefix(_)
            | Component::RootDir
            | Component::CurDir
            | Component::ParentDir => return Err(RepoError::UnsafePath),
        }
    }
    if components.is_empty() {
        Err(RepoError::UnsafePath)
    } else {
        Ok(components)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::TempDir;

    use crate::RepoError;

    use super::History;

    #[test]
    fn remote_conflict_preserves_the_full_relative_path() {
        let temp = TempDir::new().unwrap();
        let history_root = temp.path().join("history");
        let history = History::new(&history_root).unwrap();

        let stored = history
            .store_remote_conflict(
                "2026-07-25-142233",
                Path::new("notes/nested/document.md"),
                b"remote version",
            )
            .unwrap();

        assert_eq!(
            stored,
            history_root.join("2026-07-25-142233-sync/notes/nested/document.md")
        );
        assert_eq!(fs::read(stored).unwrap(), b"remote version");
    }

    #[test]
    fn history_rejects_absolute_parent_and_backslash_paths() {
        let temp = TempDir::new().unwrap();
        let history = History::new(temp.path().join("history")).unwrap();

        for path in [
            Path::new("/absolute.md"),
            Path::new("../escape.md"),
            Path::new("notes/../../escape.md"),
            Path::new(r"notes\escape.md"),
        ] {
            assert!(matches!(
                history.store_remote_conflict("2026-07-25-142233", path, b"escape"),
                Err(RepoError::UnsafePath)
            ));
        }
    }

    #[test]
    fn history_rejects_invalid_timestamp_path_material() {
        let temp = TempDir::new().unwrap();
        let history = History::new(temp.path().join("history")).unwrap();

        for timestamp in [
            "2026-07-25 142233",
            "../2026-07-25-142233",
            "20260725142233",
        ] {
            assert!(matches!(
                history.store_remote_conflict(timestamp, Path::new("document.md"), b"escape"),
                Err(RepoError::InvalidData(_))
            ));
        }
    }

    #[cfg(unix)]
    #[test]
    fn history_rejects_a_symlink_ancestor_escape() {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new().unwrap();
        let outside = temp.path().join("outside");
        let history_root = temp.path().join("history");
        fs::create_dir_all(&outside).unwrap();
        fs::create_dir_all(history_root.join("2026-07-25-142233-sync")).unwrap();
        symlink(&outside, history_root.join("2026-07-25-142233-sync/notes")).unwrap();
        let history = History::new(&history_root).unwrap();

        assert!(matches!(
            history.store_remote_conflict(
                "2026-07-25-142233",
                Path::new("notes/document.md"),
                b"escape",
            ),
            Err(RepoError::UnsafePath)
        ));
        assert!(!outside.join("document.md").exists());
    }
}
