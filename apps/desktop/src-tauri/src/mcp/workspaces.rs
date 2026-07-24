use std::{
    collections::{HashMap, HashSet},
    fmt,
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, RwLock,
    },
};

use cap_fs_ext::MetadataExt;
use cap_std::fs::Dir;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct AuthorizedWorkspaceConfig {
    pub(crate) workspace_id: Uuid,
    pub(crate) display_name: String,
    pub(crate) canonical_path: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileIdentity {
    device: u64,
    inode: u64,
}

impl FileIdentity {
    fn from_metadata<T: MetadataExt>(metadata: &T) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct ResolvedWorkspace {
    pub(crate) workspace_id: Uuid,
    pub(crate) workspace_generation: u64,
    pub(crate) display_name: String,
    pub(crate) canonical_path: PathBuf,
    pub(crate) root: Arc<Dir>,
    authority_generation: Arc<AtomicU64>,
    identity: FileIdentity,
}

impl ResolvedWorkspace {
    pub(crate) fn revalidate_authority(&self) -> Result<(), WorkspaceError> {
        if self.workspace_generation != self.authority_generation.load(Ordering::Acquire) {
            return Err(WorkspaceError::stale());
        }
        self.revalidate()
    }

    pub(crate) fn revalidate(&self) -> Result<(), WorkspaceError> {
        let metadata = std::fs::symlink_metadata(&self.canonical_path)
            .map_err(|_| WorkspaceError::unavailable())?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(WorkspaceError::unavailable());
        }
        let addressed = Dir::open_ambient_dir(&self.canonical_path, cap_std::ambient_authority())
            .map_err(|_| WorkspaceError::unavailable())?;
        let identity = FileIdentity::from_metadata(
            &addressed
                .dir_metadata()
                .map_err(|_| WorkspaceError::unavailable())?,
        );
        if identity != self.identity {
            return Err(WorkspaceError::unavailable());
        }
        Ok(())
    }

    fn to_config(&self) -> AuthorizedWorkspaceConfig {
        AuthorizedWorkspaceConfig {
            workspace_id: self.workspace_id,
            display_name: self.display_name.clone(),
            canonical_path: self.canonical_path.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SafeWorkspace {
    pub(crate) workspace_id: Uuid,
    pub(crate) workspace_generation: u64,
    pub(crate) display_name: String,
    pub(crate) leaf_name: String,
    pub(crate) available: bool,
}

#[derive(Clone)]
struct WorkspaceEntry {
    config: AuthorizedWorkspaceConfig,
    resolved: Option<ResolvedWorkspace>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WorkspaceError {
    pub(crate) code: &'static str,
    message: &'static str,
}

impl WorkspaceError {
    fn invalid_root() -> Self {
        Self {
            code: "workspace_not_authorizable",
            message: "The selected directory cannot be authorized for MCP.",
        }
    }

    fn overlap() -> Self {
        Self {
            code: "workspace_overlap",
            message: "Authorized MCP directories cannot overlap.",
        }
    }

    fn protected() -> Self {
        Self {
            code: "protected_path",
            message: "The selected directory is protected.",
        }
    }

    fn not_authorized() -> Self {
        Self {
            code: "workspace_not_authorized",
            message: "The MCP workspace is not authorized.",
        }
    }

    fn unavailable() -> Self {
        Self {
            code: "workspace_unavailable",
            message: "The authorized MCP workspace is unavailable.",
        }
    }

    fn primary_unavailable() -> Self {
        Self {
            code: "mcp-workspace-unavailable",
            message: "A valid primary notes workspace is required for MCP document tools.",
        }
    }

    fn stale() -> Self {
        Self {
            code: "mcp-handle-stale",
            message: "The MCP capability belongs to an older primary notes workspace.",
        }
    }

    fn state() -> Self {
        Self {
            code: "workspace_state_unavailable",
            message: "The MCP workspace registry is unavailable.",
        }
    }
}

impl fmt::Display for WorkspaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for WorkspaceError {}

pub(crate) struct WorkspaceRegistry {
    authority_gate: RwLock<()>,
    protected_roots: Vec<PathBuf>,
    stale_workspace_ids: RwLock<HashSet<Uuid>>,
    workspaces: RwLock<HashMap<Uuid, WorkspaceEntry>>,
    generation: Arc<AtomicU64>,
    #[cfg(test)]
    authority_read_hook: RwLock<Option<Arc<dyn Fn() + Send + Sync>>>,
}

impl WorkspaceRegistry {
    pub(crate) fn new(protected_roots: Vec<PathBuf>) -> Self {
        Self {
            authority_gate: RwLock::new(()),
            protected_roots: protected_roots
                .into_iter()
                .map(|path| path.canonicalize().unwrap_or(path))
                .collect(),
            stale_workspace_ids: RwLock::new(HashSet::new()),
            workspaces: RwLock::new(HashMap::new()),
            generation: Arc::new(AtomicU64::new(1)),
            #[cfg(test)]
            authority_read_hook: RwLock::new(None),
        }
    }

    pub(crate) fn for_application_data(app_config_root: &Path, app_data_root: &Path) -> Self {
        let canonical = |path: &Path| path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        Self {
            authority_gate: RwLock::new(()),
            protected_roots: vec![canonical(app_config_root), canonical(app_data_root)],
            stale_workspace_ids: RwLock::new(HashSet::new()),
            workspaces: RwLock::new(HashMap::new()),
            generation: Arc::new(AtomicU64::new(1)),
            #[cfg(test)]
            authority_read_hook: RwLock::new(None),
        }
    }

    pub(crate) fn from_configs(
        configs: Vec<AuthorizedWorkspaceConfig>,
        protected_roots: Vec<PathBuf>,
    ) -> Result<Self, WorkspaceError> {
        let registry = Self::new(protected_roots);
        for config in configs {
            registry.insert_restored(config)?;
        }
        Ok(registry)
    }

    pub(crate) fn authorize(
        &self,
        path: &Path,
        display_name: &str,
    ) -> Result<AuthorizedWorkspaceConfig, WorkspaceError> {
        let _authority = self
            .authority_gate
            .write()
            .map_err(|_| WorkspaceError::state())?;
        let display_name = display_name.trim();
        if display_name.is_empty() {
            return Err(WorkspaceError::invalid_root());
        }
        let canonical_path = canonical_authorizable_root(path)?;
        let config = AuthorizedWorkspaceConfig {
            workspace_id: Uuid::new_v4(),
            display_name: display_name.to_string(),
            canonical_path,
        };
        let workspace = open_workspace(&config, Arc::clone(&self.generation))?;
        self.insert_entry_locked(WorkspaceEntry {
            config: config.clone(),
            resolved: Some(workspace),
        })?;
        Ok(config)
    }

    pub(crate) fn activate_current(
        &self,
        path: &Path,
    ) -> Result<AuthorizedWorkspaceConfig, WorkspaceError> {
        let _authority = self
            .authority_gate
            .write()
            .map_err(|_| WorkspaceError::state())?;
        let canonical_path = canonical_authorizable_root(path)?;
        if self.is_protected_root(&canonical_path) {
            return Err(WorkspaceError::protected());
        }

        let display_name = canonical_path
            .file_name()
            .map(|name| name.to_string_lossy().trim().to_string())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "Workspace".to_string());
        let mut workspaces = self
            .workspaces
            .write()
            .map_err(|_| WorkspaceError::state())?;
        if let Some(existing) = workspaces
            .values()
            .find(|entry| entry.config.canonical_path == canonical_path)
        {
            if existing
                .resolved
                .as_ref()
                .is_some_and(|workspace| workspace.revalidate().is_ok())
            {
                return Ok(existing.config.clone());
            }
        }

        let config = AuthorizedWorkspaceConfig {
            workspace_id: Uuid::new_v4(),
            display_name,
            canonical_path,
        };
        let workspace = open_workspace(&config, Arc::clone(&self.generation))?;
        self.stale_workspace_ids
            .write()
            .map_err(|_| WorkspaceError::state())?
            .extend(workspaces.keys().copied());
        workspaces.clear();
        workspaces.insert(
            config.workspace_id,
            WorkspaceEntry {
                config: config.clone(),
                resolved: Some(workspace),
            },
        );
        self.generation.fetch_add(1, Ordering::AcqRel);
        Ok(config)
    }

    pub(crate) fn clear_current(&self) -> Result<(), WorkspaceError> {
        let _authority = self
            .authority_gate
            .write()
            .map_err(|_| WorkspaceError::state())?;
        let mut workspaces = self
            .workspaces
            .write()
            .map_err(|_| WorkspaceError::state())?;
        if !workspaces.is_empty() {
            self.stale_workspace_ids
                .write()
                .map_err(|_| WorkspaceError::state())?
                .extend(workspaces.keys().copied());
            workspaces.clear();
            self.generation.fetch_add(1, Ordering::AcqRel);
        }
        Ok(())
    }

    fn insert_restored(&self, config: AuthorizedWorkspaceConfig) -> Result<(), WorkspaceError> {
        let _authority = self
            .authority_gate
            .write()
            .map_err(|_| WorkspaceError::state())?;
        if config.display_name.trim().is_empty() || !is_normalized_absolute(&config.canonical_path)
        {
            return Err(WorkspaceError::invalid_root());
        }
        let resolved = match std::fs::symlink_metadata(&config.canonical_path) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(WorkspaceError::invalid_root());
            }
            Ok(_) => Some(open_workspace(&config, Arc::clone(&self.generation))?),
            Err(_) => None,
        };
        self.insert_entry_locked(WorkspaceEntry { config, resolved })
    }

    fn insert_entry_locked(&self, entry: WorkspaceEntry) -> Result<(), WorkspaceError> {
        if self.is_protected_root(&entry.config.canonical_path) {
            return Err(WorkspaceError::protected());
        }

        let mut workspaces = self
            .workspaces
            .write()
            .map_err(|_| WorkspaceError::state())?;
        if workspaces.contains_key(&entry.config.workspace_id)
            || workspaces.values().any(|existing| {
                same_resolved_identity(existing, &entry)
                    || paths_overlap(
                        &existing.config.canonical_path,
                        &entry.config.canonical_path,
                    )
            })
        {
            return Err(WorkspaceError::overlap());
        }
        workspaces.insert(entry.config.workspace_id, entry);
        self.generation.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }

    pub(crate) fn remove(&self, workspace_id: Uuid) -> Result<(), WorkspaceError> {
        let _authority = self
            .authority_gate
            .write()
            .map_err(|_| WorkspaceError::state())?;
        let mut workspaces = self
            .workspaces
            .write()
            .map_err(|_| WorkspaceError::state())?;
        if !workspaces.contains_key(&workspace_id) {
            return Err(WorkspaceError::not_authorized());
        }
        self.stale_workspace_ids
            .write()
            .map_err(|_| WorkspaceError::state())?
            .insert(workspace_id);
        workspaces.remove(&workspace_id);
        self.generation.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }

    pub(crate) fn resolve(&self, workspace_id: Uuid) -> Result<ResolvedWorkspace, WorkspaceError> {
        let workspaces = self
            .workspaces
            .read()
            .map_err(|_| WorkspaceError::state())?;
        if workspaces.is_empty() {
            if self
                .stale_workspace_ids
                .read()
                .map_err(|_| WorkspaceError::state())?
                .contains(&workspace_id)
            {
                return Err(WorkspaceError::stale());
            }
            return Err(WorkspaceError::primary_unavailable());
        }
        let generation = self.generation.load(Ordering::Acquire);
        let entry = workspaces.get(&workspace_id).cloned();
        drop(workspaces);
        let entry = match entry {
            Some(entry) => entry,
            None => {
                if self
                    .stale_workspace_ids
                    .read()
                    .map_err(|_| WorkspaceError::state())?
                    .contains(&workspace_id)
                {
                    return Err(WorkspaceError::stale());
                }
                return Err(WorkspaceError::not_authorized());
            }
        };
        let mut workspace = entry.resolved.ok_or_else(WorkspaceError::unavailable)?;
        workspace.workspace_generation = generation;
        workspace.revalidate_authority()?;
        Ok(workspace)
    }

    pub(crate) fn resolve_at_generation(
        &self,
        workspace_id: Uuid,
        expected_generation: u64,
    ) -> Result<ResolvedWorkspace, WorkspaceError> {
        let workspaces = self
            .workspaces
            .read()
            .map_err(|_| WorkspaceError::state())?;
        let generation = self.generation.load(Ordering::Acquire);
        if generation != expected_generation {
            return Err(WorkspaceError::stale());
        }
        let entry = workspaces
            .get(&workspace_id)
            .cloned()
            .ok_or_else(WorkspaceError::not_authorized)?;
        drop(workspaces);
        let mut workspace = entry.resolved.ok_or_else(WorkspaceError::unavailable)?;
        workspace.workspace_generation = generation;
        workspace.revalidate_authority()?;
        Ok(workspace)
    }

    pub(crate) fn require_primary_workspace(&self) -> Result<ResolvedWorkspace, WorkspaceError> {
        let workspaces = self
            .workspaces
            .read()
            .map_err(|_| WorkspaceError::state())?;
        let generation = self.generation.load(Ordering::Acquire);
        let entry = workspaces
            .values()
            .next()
            .cloned()
            .ok_or_else(WorkspaceError::primary_unavailable)?;
        drop(workspaces);
        let mut workspace = entry.resolved.ok_or_else(WorkspaceError::unavailable)?;
        workspace.workspace_generation = generation;
        workspace.revalidate_authority()?;
        Ok(workspace)
    }

    pub(crate) fn list_safe(&self) -> Vec<SafeWorkspace> {
        let Ok(workspaces) = self.workspaces.read() else {
            return Vec::new();
        };
        let generation = self.generation.load(Ordering::Acquire);
        let mut listed = workspaces
            .values()
            .map(|entry| SafeWorkspace {
                workspace_id: entry.config.workspace_id,
                workspace_generation: generation,
                display_name: entry.config.display_name.clone(),
                leaf_name: entry
                    .config
                    .canonical_path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Workspace".to_string()),
                available: entry
                    .resolved
                    .as_ref()
                    .is_some_and(|workspace| workspace.revalidate().is_ok()),
            })
            .collect::<Vec<_>>();
        listed.sort_by(|left, right| {
            left.display_name
                .cmp(&right.display_name)
                .then(left.workspace_id.cmp(&right.workspace_id))
        });
        listed
    }

    pub(crate) fn configs(&self) -> Result<Vec<AuthorizedWorkspaceConfig>, WorkspaceError> {
        let mut configs = self
            .workspaces
            .read()
            .map_err(|_| WorkspaceError::state())?
            .values()
            .map(|entry| entry.config.clone())
            .collect::<Vec<_>>();
        configs.sort_by_key(|config| config.workspace_id);
        Ok(configs)
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    pub(crate) fn with_authority<T>(
        &self,
        operation: impl FnOnce() -> T,
    ) -> Result<T, WorkspaceError> {
        let _authority = self
            .authority_gate
            .read()
            .map_err(|_| WorkspaceError::state())?;
        #[cfg(test)]
        if let Some(hook) = self
            .authority_read_hook
            .read()
            .map_err(|_| WorkspaceError::state())?
            .clone()
        {
            hook();
        }
        Ok(operation())
    }

    #[cfg(test)]
    pub(crate) fn set_authority_read_hook(&self, hook: impl Fn() + Send + Sync + 'static) {
        *self
            .authority_read_hook
            .write()
            .expect("authority read hook lock") = Some(Arc::new(hook));
    }

    fn is_protected_root(&self, candidate: &Path) -> bool {
        self.protected_roots
            .iter()
            .any(|protected| paths_overlap(candidate, protected))
    }
}

fn canonical_authorizable_root(path: &Path) -> Result<PathBuf, WorkspaceError> {
    let selected_metadata =
        std::fs::symlink_metadata(path).map_err(|_| WorkspaceError::invalid_root())?;
    if selected_metadata.file_type().is_symlink() || !selected_metadata.is_dir() {
        return Err(WorkspaceError::invalid_root());
    }
    path.canonicalize()
        .map_err(|_| WorkspaceError::invalid_root())
}

fn is_normalized_absolute(path: &Path) -> bool {
    path.is_absolute()
        && path
            .components()
            .all(|component| !matches!(component, Component::CurDir | Component::ParentDir))
}

fn open_workspace(
    config: &AuthorizedWorkspaceConfig,
    authority_generation: Arc<AtomicU64>,
) -> Result<ResolvedWorkspace, WorkspaceError> {
    if config.display_name.trim().is_empty() {
        return Err(WorkspaceError::invalid_root());
    }
    let canonical_path = canonical_authorizable_root(&config.canonical_path)?;
    if canonical_path != config.canonical_path {
        return Err(WorkspaceError::invalid_root());
    }
    let root = Dir::open_ambient_dir(&canonical_path, cap_std::ambient_authority())
        .map_err(|_| WorkspaceError::invalid_root())?;
    let identity = FileIdentity::from_metadata(
        &root
            .dir_metadata()
            .map_err(|_| WorkspaceError::invalid_root())?,
    );
    let workspace = ResolvedWorkspace {
        workspace_id: config.workspace_id,
        workspace_generation: 0,
        display_name: config.display_name.trim().to_string(),
        canonical_path,
        root: Arc::new(root),
        authority_generation,
        identity,
    };
    workspace.revalidate()?;
    Ok(workspace)
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    left == right || left.starts_with(right) || right.starts_with(left)
}

fn same_resolved_identity(left: &WorkspaceEntry, right: &WorkspaceEntry) -> bool {
    match (&left.resolved, &right.resolved) {
        (Some(left), Some(right)) => left.identity == right.identity,
        _ => false,
    }
}
