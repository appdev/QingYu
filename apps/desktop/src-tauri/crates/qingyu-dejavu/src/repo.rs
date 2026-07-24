use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use cap_std::fs::Dir;
use ignore::gitignore::{Gitignore, GitignoreBuilder};

use crate::indexer::{self, IndexHook, NoopIndexHook};
use crate::path_security::{cap_metadata_is_reparse, std_metadata_is_reparse};
use crate::store::open_absolute_dir_nofollow;
use crate::{Index, RepoError, Store};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepoPaths {
    pub data: PathBuf,
    pub repo: PathBuf,
    pub history: PathBuf,
    pub temp: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub os: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RepoOptions {
    pub ignore_lines: Vec<String>,
    pub protected_include_paths: Vec<String>,
}

pub struct Repo {
    pub(crate) data_dir: Dir,
    pub(crate) device: Device,
    pub(crate) key: [u8; 32],
    pub(crate) protected_include_paths: Vec<String>,
    pub(crate) ignore_matcher: Gitignore,
    pub(crate) store: Store,
    pub(crate) index_hook: Arc<dyn IndexHook>,
}

impl Repo {
    pub fn open(
        paths: RepoPaths,
        device: Device,
        key: [u8; 32],
        options: RepoOptions,
    ) -> Result<Self, RepoError> {
        Self::open_inner(paths, device, key, options, Arc::new(NoopIndexHook))
    }

    #[cfg(test)]
    pub(crate) fn open_with_hook(
        paths: RepoPaths,
        device: Device,
        key: [u8; 32],
        options: RepoOptions,
        index_hook: Arc<dyn IndexHook>,
    ) -> Result<Self, RepoError> {
        Self::open_inner(paths, device, key, options, index_hook)
    }

    fn open_inner(
        paths: RepoPaths,
        device: Device,
        key: [u8; 32],
        options: RepoOptions,
        index_hook: Arc<dyn IndexHook>,
    ) -> Result<Self, RepoError> {
        let paths = RepoPaths {
            data: normalize_root(&paths.data)?,
            repo: normalize_root(&paths.repo)?,
            history: normalize_root(&paths.history)?,
            temp: normalize_root(&paths.temp)?,
        };
        validate_root_has_no_symlinks(&paths.data)?;
        validate_root_has_no_symlinks(&paths.repo)?;
        validate_root_has_no_symlinks(&paths.history)?;
        validate_root_has_no_symlinks(&paths.temp)?;
        let mut protected_include_paths = options
            .protected_include_paths
            .iter()
            .map(|path| normalize_protected_path(path))
            .collect::<Result<Vec<_>, _>>()?;
        protected_include_paths.sort();
        protected_include_paths.dedup();

        let mut ignore_builder = GitignoreBuilder::new(&paths.data);
        for line in &options.ignore_lines {
            ignore_builder
                .add_line(None, line)
                .map_err(|_| RepoError::RepoFatal)?;
        }
        let ignore_matcher = ignore_builder.build().map_err(|_| RepoError::RepoFatal)?;
        let data_dir = open_absolute_dir_nofollow(&paths.data)?;
        let data_metadata = data_dir.dir_metadata()?;
        if !data_metadata.file_type().is_dir() || cap_metadata_is_reparse(&data_metadata) {
            return Err(RepoError::UnsafePath);
        }
        let store = Store::new(&paths.repo, key)?;

        Ok(Self {
            data_dir,
            device,
            key,
            protected_include_paths,
            ignore_matcher,
            store,
            index_hook,
        })
    }

    pub fn index(&self, memo: &str) -> Result<Index, RepoError> {
        for attempt in 0..7 {
            match indexer::index_once(self, memo, attempt) {
                Err(RepoError::IndexFileChanged) if attempt < 6 => continue,
                result => return result,
            }
        }
        Err(RepoError::RepoFatal)
    }
}

fn normalize_root(path: &Path) -> Result<PathBuf, RepoError> {
    if path.as_os_str().is_empty() {
        return Err(RepoError::UnsafePath);
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(RepoError::UnsafePath);
                }
            }
        }
    }
    if !normalized.is_absolute() {
        return Err(RepoError::UnsafePath);
    }

    match std::fs::symlink_metadata(&normalized) {
        Ok(metadata) if unsafe_link_metadata(&metadata) => {
            return Err(RepoError::UnsafePath);
        }
        Ok(_) => {}
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) => {}
        Err(error) => return Err(RepoError::Io(error)),
    }

    let mut existing = normalized.clone();
    let mut missing = Vec::new();
    let canonical_existing = loop {
        match std::fs::symlink_metadata(&existing) {
            Ok(metadata) => {
                if !metadata.is_dir() {
                    return Err(RepoError::UnsafePath);
                }
                break std::fs::canonicalize(&existing)?;
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                let component = existing
                    .file_name()
                    .ok_or(RepoError::UnsafePath)?
                    .to_os_string();
                missing.push(component);
                if !existing.pop() {
                    return Err(RepoError::UnsafePath);
                }
            }
            Err(error) => return Err(RepoError::Io(error)),
        }
    };
    let mut canonical = canonical_existing;
    for component in missing.into_iter().rev() {
        canonical.push(component);
    }
    Ok(canonical)
}

fn normalize_protected_path(path: &str) -> Result<String, RepoError> {
    if path == "/" || !path.starts_with('/') || path.contains('\\') {
        return Err(RepoError::UnsafePath);
    }
    let mut normalized = String::new();
    for component in path[1..].split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return Err(RepoError::UnsafePath);
        }
        normalized.push('/');
        normalized.push_str(component);
    }
    Ok(normalized)
}

pub(crate) fn validate_root_has_no_symlinks(path: &Path) -> Result<(), RepoError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if unsafe_link_metadata(&metadata) => Err(RepoError::UnsafePath),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(RepoError::Io(error)),
    }
}

fn unsafe_link_metadata(metadata: &std::fs::Metadata) -> bool {
    std_metadata_is_reparse(metadata)
}
