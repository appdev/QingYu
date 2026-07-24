use std::{
    ffi::OsString,
    io,
    path::{Path, PathBuf},
};

use cap_fs_ext::DirExt;
use cap_std::fs::Dir;

use super::resources::MAX_PACKAGE_ENTRIES;

const MAX_CLEANUP_DEPTH: usize = 16;

struct QuarantinedEntry {
    parent: Dir,
    name: OsString,
    relative: PathBuf,
    is_directory: bool,
}

pub(crate) fn remove_quarantined_directory(
    parent: &Dir,
    name: &str,
    quarantine: Dir,
) -> io::Result<()> {
    remove_quarantined_directory_inner(parent, name, quarantine, &mut |_| Ok(()))
}

pub(crate) fn remove_quarantined_directory_with_hook(
    parent: &Dir,
    name: &str,
    quarantine: Dir,
    hook: &mut dyn FnMut(&Path) -> io::Result<()>,
) -> io::Result<()> {
    remove_quarantined_directory_inner(parent, name, quarantine, hook)
}

fn remove_quarantined_directory_inner(
    parent: &Dir,
    name: &str,
    quarantine: Dir,
    hook: &mut dyn FnMut(&Path) -> io::Result<()>,
) -> io::Result<()> {
    let entries = collect_quarantined_entries(&quarantine)?;
    for entry in entries.into_iter().rev() {
        if entry.is_directory {
            entry.parent.remove_dir(&entry.name)?;
        } else {
            entry.parent.remove_file(&entry.name)?;
        }
        hook(&entry.relative)?;
    }
    // cap-std opens directories without FILE_SHARE_DELETE on Windows. Keep
    // the root guard alive while descendants are removed, then close it and
    // use only a non-recursive root removal. A replacement or newly inserted
    // child makes the final removal fail without deleting unknown contents.
    drop(quarantine);
    parent.remove_dir(name)
}

fn collect_quarantined_entries(root: &Dir) -> io::Result<Vec<QuarantinedEntry>> {
    let mut pending = vec![(root.try_clone()?, PathBuf::new(), 0_usize)];
    let mut collected = Vec::new();
    while let Some((directory, relative, depth)) = pending.pop() {
        for entry in directory.entries()? {
            let name = entry?.file_name();
            if collected.len() >= MAX_PACKAGE_ENTRIES {
                return Err(io::Error::other(
                    "theme activation quarantine exceeds cleanup entry limit",
                ));
            }
            let metadata = directory.symlink_metadata(&name)?;
            let child_relative = relative.join(&name);
            let is_directory = metadata.is_dir() && !metadata.file_type().is_symlink();
            if is_directory {
                if depth + 1 > MAX_CLEANUP_DEPTH {
                    return Err(io::Error::other(
                        "theme activation quarantine exceeds cleanup depth",
                    ));
                }
                let child = directory.open_dir_nofollow(&name)?;
                pending.push((child, child_relative.clone(), depth + 1));
            }
            collected.push(QuarantinedEntry {
                parent: directory.try_clone()?,
                name,
                relative: child_relative,
                is_directory,
            });
        }
    }
    Ok(collected)
}
