use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use cap_fs_ext::{DirExt, FollowSymlinks, MetadataExt, OpenOptionsExt, OpenOptionsFollowExt};
use cap_std::fs::Dir;

use super::asset::allow_asset_directory;
use super::path::{markdown_tree_root_for_path, path_to_string};

type SystemTrash = dyn Fn(&Path) -> Result<(), String> + Send + Sync;
type BeforeTrash = dyn Fn() -> Result<(), String> + Send + Sync;

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrashWorkspaceResourceInput {
    modified_at: u64,
    relative_path: String,
    size_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum TrashWorkspaceResourceStatus {
    Failed,
    Trashed,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrashWorkspaceResourceResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    relative_path: String,
    status: TrashWorkspaceResourceStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileIdentity {
    device: u64,
    inode: u64,
}

fn file_identity<T: MetadataExt>(metadata: &T) -> FileIdentity {
    FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    }
}

fn system_time_millis(time: SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| duration.as_millis().try_into().ok())
}

#[cfg(test)]
fn modified_time_millis(metadata: &std::fs::Metadata) -> Option<u64> {
    metadata.modified().ok().and_then(system_time_millis)
}

fn retained_modified_time_millis(metadata: &cap_std::fs::Metadata) -> Option<u64> {
    metadata
        .modified()
        .ok()
        .and_then(|time| system_time_millis(time.into_std()))
}

fn normalized_resource_relative_path(relative_path: &str) -> Result<PathBuf, String> {
    let normalized = relative_path.trim().replace('\\', "/");
    if normalized.is_empty()
        || normalized.starts_with('/')
        || normalized.starts_with("//")
        || normalized
            .as_bytes()
            .get(1)
            .is_some_and(|separator| *separator == b':')
    {
        return Err("Workspace resource path must be relative".to_string());
    }

    let mut path = PathBuf::new();
    let mut has_assets_parent = false;
    let components = Path::new(&normalized).components().collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate() {
        match component {
            Component::Normal(part) => {
                if index + 1 < components.len() {
                    let text = part
                        .to_str()
                        .ok_or_else(|| "Workspace resource path must be valid UTF-8".to_string())?;
                    let is_assets = if cfg!(windows) {
                        text.eq_ignore_ascii_case("assets")
                    } else {
                        text == "assets"
                    };
                    has_assets_parent |= is_assets;
                }
                path.push(part);
            }
            Component::CurDir => {}
            _ => return Err("Workspace resource path cannot leave the workspace".to_string()),
        }
    }

    if path.file_name().is_none() || !has_assets_parent {
        return Err("Workspace resource must be a file below an assets directory".to_string());
    }

    Ok(path)
}

fn nonfollowing_read_options() -> cap_std::fs::OpenOptions {
    let mut options = cap_std::fs::OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    #[cfg(unix)]
    options.custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32);
    options
}

fn open_resource_parent(root: &Dir, relative_path: &Path) -> Result<(Dir, PathBuf), String> {
    let file_name = relative_path
        .file_name()
        .ok_or_else(|| "Workspace resource path must name a file".to_string())?
        .to_os_string();
    let mut current = root.try_clone().map_err(|error| error.to_string())?;
    if let Some(parent) = relative_path.parent() {
        for component in parent.components() {
            let Component::Normal(part) = component else {
                return Err("Workspace resource path cannot leave the workspace".to_string());
            };
            current = current
                .open_dir_nofollow(part)
                .map_err(|error| format!("Workspace resource parent is unavailable: {error}"))?;
        }
    }

    Ok((current, PathBuf::from(file_name)))
}

struct VerifiedResource {
    identity: FileIdentity,
    modified_at: u64,
    relative_path: PathBuf,
    size_bytes: u64,
}

fn verify_resource(
    root: &Dir,
    input: &TrashWorkspaceResourceInput,
) -> Result<VerifiedResource, String> {
    let relative_path = normalized_resource_relative_path(&input.relative_path)?;
    let (parent, file_name) = open_resource_parent(root, &relative_path)?;
    let addressed = parent
        .symlink_metadata(&file_name)
        .map_err(|error| format!("Workspace resource is unavailable: {error}"))?;
    if addressed.file_type().is_symlink() || !addressed.is_file() {
        return Err("Workspace resource must be a regular non-symlink file".to_string());
    }

    let retained = parent
        .open_with(&file_name, &nonfollowing_read_options())
        .map_err(|error| format!("Workspace resource could not be retained: {error}"))?;
    let retained_metadata = retained.metadata().map_err(|error| error.to_string())?;
    let modified_at = retained_modified_time_millis(&retained_metadata)
        .ok_or_else(|| "Workspace resource modified time is unavailable".to_string())?;
    if !retained_metadata.is_file()
        || file_identity(&addressed) != file_identity(&retained_metadata)
        || retained_metadata.len() != input.size_bytes
        || modified_at != input.modified_at
    {
        return Err("Workspace resource changed since it was scanned".to_string());
    }

    Ok(VerifiedResource {
        identity: file_identity(&retained_metadata),
        modified_at,
        relative_path,
        size_bytes: retained_metadata.len(),
    })
}

fn verify_root_identity(root_path: &Path, expected: FileIdentity) -> Result<(), String> {
    let addressed = Dir::open_ambient_dir(root_path, cap_std::ambient_authority())
        .map_err(|error| format!("Workspace root changed: {error}"))?;
    let identity = file_identity(
        &addressed
            .dir_metadata()
            .map_err(|error| format!("Workspace root changed: {error}"))?,
    );
    if identity != expected {
        return Err("Workspace root changed during resource deletion".to_string());
    }
    Ok(())
}

fn verify_resource_identity(root: &Dir, resource: &VerifiedResource) -> Result<(), String> {
    let (parent, file_name) = open_resource_parent(root, &resource.relative_path)?;
    let retained = parent
        .open_with(&file_name, &nonfollowing_read_options())
        .map_err(|error| format!("Workspace resource changed: {error}"))?;
    let metadata = retained.metadata().map_err(|error| error.to_string())?;
    let modified_at = retained_modified_time_millis(&metadata)
        .ok_or_else(|| "Workspace resource modified time is unavailable".to_string())?;
    if !metadata.is_file()
        || file_identity(&metadata) != resource.identity
        || metadata.len() != resource.size_bytes
        || modified_at != resource.modified_at
    {
        return Err("Workspace resource changed before it reached the trash".to_string());
    }
    Ok(())
}

struct WorkspaceResourceService {
    before_trash: Arc<BeforeTrash>,
    system_trash: Arc<SystemTrash>,
}

impl Default for WorkspaceResourceService {
    fn default() -> Self {
        Self {
            before_trash: Arc::new(|| Ok(())),
            system_trash: Arc::new(|path| trash::delete(path).map_err(|error| error.to_string())),
        }
    }
}

impl WorkspaceResourceService {
    #[cfg(test)]
    fn with_system_trash(
        system_trash: impl Fn(&Path) -> Result<(), String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            before_trash: Arc::new(|| Ok(())),
            system_trash: Arc::new(system_trash),
        }
    }

    #[cfg(test)]
    fn with_before_trash(
        mut self,
        before_trash: impl Fn() -> Result<(), String> + Send + Sync + 'static,
    ) -> Self {
        self.before_trash = Arc::new(before_trash);
        self
    }

    fn trash_one(
        &self,
        canonical_root: &Path,
        root: &Dir,
        root_identity: FileIdentity,
        input: &TrashWorkspaceResourceInput,
    ) -> Result<(), String> {
        let verified = verify_resource(root, input)?;
        (self.before_trash)()?;
        verify_root_identity(canonical_root, root_identity)?;
        verify_resource_identity(root, &verified)?;
        (self.system_trash)(&canonical_root.join(&verified.relative_path))
    }

    fn trash(
        &self,
        root_path: String,
        resources: Vec<TrashWorkspaceResourceInput>,
    ) -> Vec<TrashWorkspaceResourceResult> {
        let canonical_root = match PathBuf::from(root_path).canonicalize() {
            Ok(path) if path.is_dir() => path,
            Ok(_) => {
                return resources
                    .into_iter()
                    .map(|input| TrashWorkspaceResourceResult {
                        error: Some("Workspace resource root must be a directory".to_string()),
                        relative_path: input.relative_path,
                        status: TrashWorkspaceResourceStatus::Failed,
                    })
                    .collect()
            }
            Err(error) => {
                return resources
                    .into_iter()
                    .map(|input| TrashWorkspaceResourceResult {
                        error: Some(error.to_string()),
                        relative_path: input.relative_path,
                        status: TrashWorkspaceResourceStatus::Failed,
                    })
                    .collect()
            }
        };
        let root = match Dir::open_ambient_dir(&canonical_root, cap_std::ambient_authority()) {
            Ok(root) => root,
            Err(error) => {
                return resources
                    .into_iter()
                    .map(|input| TrashWorkspaceResourceResult {
                        error: Some(error.to_string()),
                        relative_path: input.relative_path,
                        status: TrashWorkspaceResourceStatus::Failed,
                    })
                    .collect()
            }
        };
        let root_identity = match root.dir_metadata() {
            Ok(metadata) => file_identity(&metadata),
            Err(error) => {
                return resources
                    .into_iter()
                    .map(|input| TrashWorkspaceResourceResult {
                        error: Some(error.to_string()),
                        relative_path: input.relative_path,
                        status: TrashWorkspaceResourceStatus::Failed,
                    })
                    .collect()
            }
        };

        resources
            .into_iter()
            .map(
                |input| match self.trash_one(&canonical_root, &root, root_identity, &input) {
                    Ok(()) => TrashWorkspaceResourceResult {
                        error: None,
                        relative_path: input.relative_path,
                        status: TrashWorkspaceResourceStatus::Trashed,
                    },
                    Err(error) => TrashWorkspaceResourceResult {
                        error: Some(error),
                        relative_path: input.relative_path,
                        status: TrashWorkspaceResourceStatus::Failed,
                    },
                },
            )
            .collect()
    }
}

fn resolve_workspace_resource_root_with_scope(
    source_path: String,
    allow_root: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<String, String> {
    let source_path = PathBuf::from(source_path);
    let root = markdown_tree_root_for_path(&source_path)?
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if !root.is_dir() {
        return Err("Workspace resource root must be a directory".to_string());
    }
    allow_root(&root)?;
    Ok(path_to_string(&root))
}

#[tauri::command]
pub(crate) fn resolve_workspace_resource_root(
    app: tauri::AppHandle,
    source_path: String,
) -> Result<String, String> {
    resolve_workspace_resource_root_with_scope(source_path, |root| {
        allow_asset_directory(&app, root)
    })
}

#[tauri::command]
pub(crate) fn trash_workspace_resources(
    root_path: String,
    resources: Vec<TrashWorkspaceResourceInput>,
) -> Vec<TrashWorkspaceResourceResult> {
    WorkspaceResourceService::default().trash(root_path, resources)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    fn resource_input(root: &Path, relative_path: &str) -> TrashWorkspaceResourceInput {
        let metadata =
            fs::metadata(root.join(relative_path)).expect("fixture metadata should exist");
        TrashWorkspaceResourceInput {
            modified_at: modified_time_millis(&metadata)
                .expect("fixture should have modified time"),
            relative_path: relative_path.to_string(),
            size_bytes: metadata.len(),
        }
    }

    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let temporary = tempfile::tempdir().expect("temporary workspace should be created");
        let root = temporary.path().join("vault");
        fs::create_dir_all(root.join("assets")).expect("root assets should be created");
        fs::create_dir_all(root.join("docs/assets")).expect("nested assets should be created");
        fs::write(root.join("assets/cover.png"), b"cover").expect("cover should be written");
        fs::write(root.join("docs/assets/manual.pdf"), b"manual")
            .expect("manual should be written");
        fs::write(root.join("outside.txt"), b"outside").expect("outside file should be written");
        (temporary, root)
    }

    #[test]
    fn resolves_a_markdown_file_source_to_its_canonical_parent() {
        let (_temporary, root) = fixture();
        let markdown = root.join("note.md");
        fs::write(&markdown, "# Note").expect("note should be written");

        let resolved = resolve_workspace_resource_root_with_scope(
            markdown.to_string_lossy().to_string(),
            |_| Ok(()),
        )
        .expect("workspace root should resolve");

        assert_eq!(
            PathBuf::from(resolved),
            root.canonicalize().expect("root should canonicalize")
        );
    }

    #[test]
    fn trashes_valid_root_and_nested_assets() {
        let (_temporary, root) = fixture();
        let trashed = Arc::new(Mutex::new(Vec::new()));
        let recorded = trashed.clone();
        let service = WorkspaceResourceService::with_system_trash(move |path| {
            recorded
                .lock()
                .expect("trash recorder should lock")
                .push(path.to_path_buf());
            Ok(())
        });

        let results = service.trash(
            root.to_string_lossy().to_string(),
            vec![
                resource_input(&root, "assets/cover.png"),
                resource_input(&root, "docs/assets/manual.pdf"),
            ],
        );

        assert_eq!(
            results
                .iter()
                .map(|result| result.status)
                .collect::<Vec<_>>(),
            vec![
                TrashWorkspaceResourceStatus::Trashed,
                TrashWorkspaceResourceStatus::Trashed,
            ]
        );
        assert_eq!(trashed.lock().expect("trash recorder should lock").len(), 2);
        assert!(root.join("assets/cover.png").exists());
    }

    #[test]
    fn rejects_unsafe_non_asset_and_non_file_targets_independently() {
        let (_temporary, root) = fixture();
        let service = WorkspaceResourceService::with_system_trash(|_| Ok(()));
        let valid = resource_input(&root, "assets/cover.png");
        let results = service.trash(
            root.to_string_lossy().to_string(),
            vec![
                TrashWorkspaceResourceInput {
                    relative_path: "../outside.txt".to_string(),
                    ..valid.clone()
                },
                TrashWorkspaceResourceInput {
                    relative_path: root.join("assets/cover.png").to_string_lossy().to_string(),
                    ..valid.clone()
                },
                resource_input(&root, "outside.txt"),
                TrashWorkspaceResourceInput {
                    relative_path: "assets".to_string(),
                    size_bytes: 0,
                    modified_at: 0,
                },
                TrashWorkspaceResourceInput {
                    relative_path: "assets/missing.png".to_string(),
                    ..valid.clone()
                },
                valid,
            ],
        );

        assert_eq!(
            results
                .iter()
                .filter(|result| result.status == TrashWorkspaceResourceStatus::Failed)
                .count(),
            5
        );
        assert_eq!(
            results.last().map(|result| result.status),
            Some(TrashWorkspaceResourceStatus::Trashed)
        );
    }

    #[test]
    fn rejects_changed_size_and_modified_time() {
        let (_temporary, root) = fixture();
        let service = WorkspaceResourceService::with_system_trash(|_| Ok(()));
        let expected = resource_input(&root, "assets/cover.png");

        let results = service.trash(
            root.to_string_lossy().to_string(),
            vec![
                TrashWorkspaceResourceInput {
                    size_bytes: expected.size_bytes + 1,
                    ..expected.clone()
                },
                TrashWorkspaceResourceInput {
                    modified_at: expected.modified_at.saturating_sub(1),
                    ..expected
                },
            ],
        );

        assert!(results
            .iter()
            .all(|result| result.status == TrashWorkspaceResourceStatus::Failed));
    }

    #[test]
    fn rejects_a_destination_replaced_before_the_final_identity_check() {
        let (_temporary, root) = fixture();
        let target = root.join("assets/cover.png");
        let captured = root.join("assets/captured.png");
        let before_target = target.clone();
        let before_captured = captured.clone();
        let trashed = Arc::new(Mutex::new(Vec::new()));
        let recorded = trashed.clone();
        let service = WorkspaceResourceService::with_system_trash(move |path| {
            recorded
                .lock()
                .expect("trash recorder should lock")
                .push(path.to_path_buf());
            Ok(())
        })
        .with_before_trash(move || {
            fs::rename(&before_target, &before_captured).map_err(|error| error.to_string())?;
            fs::write(&before_target, b"cover").map_err(|error| error.to_string())
        });

        let result = service.trash(
            root.to_string_lossy().to_string(),
            vec![resource_input(&root, "assets/cover.png")],
        );

        assert_eq!(result[0].status, TrashWorkspaceResourceStatus::Failed);
        assert!(trashed
            .lock()
            .expect("trash recorder should lock")
            .is_empty());
        assert!(captured.exists());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_parent_and_leaf_targets() {
        use std::os::unix::fs::symlink;

        let (_temporary, root) = fixture();
        let real = root.join("real-assets");
        fs::create_dir_all(&real).expect("real assets should be created");
        fs::write(real.join("linked.png"), b"linked").expect("linked file should be written");
        symlink(&real, root.join("linked-assets")).expect("parent symlink should be created");
        symlink(real.join("linked.png"), root.join("assets/linked.png"))
            .expect("leaf symlink should be created");
        let metadata = fs::metadata(real.join("linked.png")).expect("real metadata should exist");
        let service = WorkspaceResourceService::with_system_trash(|_| Ok(()));

        let results = service.trash(
            root.to_string_lossy().to_string(),
            vec![
                TrashWorkspaceResourceInput {
                    modified_at: modified_time_millis(&metadata)
                        .expect("modified time should exist"),
                    relative_path: "linked-assets/assets/linked.png".to_string(),
                    size_bytes: metadata.len(),
                },
                TrashWorkspaceResourceInput {
                    modified_at: modified_time_millis(&metadata)
                        .expect("modified time should exist"),
                    relative_path: "assets/linked.png".to_string(),
                    size_bytes: metadata.len(),
                },
            ],
        );

        assert!(results
            .iter()
            .all(|result| result.status == TrashWorkspaceResourceStatus::Failed));
    }
}
