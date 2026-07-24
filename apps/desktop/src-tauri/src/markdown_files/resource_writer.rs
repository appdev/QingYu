use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use cap_fs_ext::{DirExt, FollowSymlinks, MetadataExt, OpenOptionsExt, OpenOptionsFollowExt};
use cap_std::fs::Dir;

use super::path::{is_markdown_open_file, markdown_relative_path};

const ASSETS_FOLDER: &str = "assets";
const STAGED_RESOURCE_NAME: &str = "content";
const QUARANTINED_RESOURCE_NAME: &str = "published";
const RESOURCE_STAGING_PREFIX: &str = ".qingyu-resource-";

pub(super) struct SavedProjectResource {
    pub(super) relative_path: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct FileIdentity {
    device: u64,
    inode: u64,
}

pub(super) fn file_identity<T: MetadataExt>(metadata: &T) -> FileIdentity {
    FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    }
}

fn requested_resource_file_name(file_name: &str) -> Result<String, String> {
    let trimmed = file_name.trim();
    if trimmed.is_empty()
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || matches!(trimmed, "." | "..")
    {
        return Err("Project resource file name is invalid".to_string());
    }
    if Path::new(trimmed).components().count() != 1 {
        return Err("Project resource file name cannot include folders".to_string());
    }
    let Some(stem) = Path::new(trimmed)
        .file_stem()
        .and_then(|stem| stem.to_str())
    else {
        return Err("Project resource file name is invalid".to_string());
    };
    if stem.trim().is_empty() || matches!(stem.trim(), "." | "..") {
        return Err("Project resource file name is invalid".to_string());
    }
    Ok(trimmed.to_string())
}

fn unique_resource_file_name(file_name: &str, attempt: usize) -> Result<String, String> {
    let requested_name = requested_resource_file_name(file_name)?;
    if attempt == 0 {
        return Ok(requested_name);
    }
    let path = Path::new(&requested_name);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "Project resource file name is invalid".to_string())?;
    let suffix = format!("-{}", attempt + 1);
    if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
        return Ok(format!("{stem}{suffix}.{extension}"));
    }
    Ok(format!("{stem}{suffix}"))
}

fn ensure_assets_folder(root: &Dir) -> Result<Dir, String> {
    match root.symlink_metadata(ASSETS_FOLDER) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err("Project assets directory cannot be a symbolic link".to_string())
        }
        Ok(metadata) if !metadata.is_dir() => {
            return Err("Project assets path must be a directory".to_string())
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if let Err(error) = root.create_dir(ASSETS_FOLDER) {
                if error.kind() != io::ErrorKind::AlreadyExists {
                    return Err(error.to_string());
                }
                let metadata = root
                    .symlink_metadata(ASSETS_FOLDER)
                    .map_err(|error| error.to_string())?;
                if metadata.file_type().is_symlink() {
                    return Err("Project assets directory cannot be a symbolic link".to_string());
                }
                if !metadata.is_dir() {
                    return Err("Project assets path must be a directory".to_string());
                }
            }
        }
        Err(error) => return Err(error.to_string()),
    }
    root.open_dir_nofollow(ASSETS_FOLDER)
        .map_err(|error| error.to_string())
}

fn ensure_resource_folder(root: &Dir, folder: &Path) -> Result<Dir, String> {
    let mut current = root.try_clone().map_err(|error| error.to_string())?;
    for component in folder.components() {
        let Component::Normal(part) = component else {
            return Err("Resource folder is invalid".to_string());
        };
        match current.symlink_metadata(part) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err("Resource folder cannot contain symbolic links".to_string())
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err("Resource folder component is not a directory".to_string())
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if let Err(error) = current.create_dir(part) {
                    if error.kind() != io::ErrorKind::AlreadyExists {
                        return Err(error.to_string());
                    }
                }
                let metadata = current
                    .symlink_metadata(part)
                    .map_err(|error| error.to_string())?;
                if metadata.file_type().is_symlink() {
                    return Err("Resource folder cannot contain symbolic links".to_string());
                }
                if !metadata.is_dir() {
                    return Err("Resource folder component is not a directory".to_string());
                }
            }
            Err(error) => return Err(error.to_string()),
        }
        current = current
            .open_dir_nofollow(part)
            .map_err(|error| error.to_string())?;
    }
    Ok(current)
}

fn open_existing_folder(root: &Dir, folder: &Path) -> Result<Dir, String> {
    let mut current = root.try_clone().map_err(|error| error.to_string())?;
    for component in folder.components() {
        let Component::Normal(part) = component else {
            return Err("Project resource folder is invalid".to_string());
        };
        current = current
            .open_dir_nofollow(part)
            .map_err(|error| error.to_string())?;
    }
    Ok(current)
}

fn nonfollowing_read_options() -> cap_std::fs::OpenOptions {
    let mut options = cap_std::fs::OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    #[cfg(unix)]
    options.custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32);
    options
}

fn verify_directory_path_identity(
    path: &Path,
    expected: FileIdentity,
    description: &str,
) -> Result<(), String> {
    let directory = Dir::open_ambient_dir(path, cap_std::ambient_authority())
        .map_err(|error| format!("{description} changed: {error}"))?;
    let actual = file_identity(
        &directory
            .dir_metadata()
            .map_err(|error| format!("{description} changed: {error}"))?,
    );
    if actual != expected {
        return Err(format!("{description} changed during resource save"));
    }
    Ok(())
}

fn root_identity_error_with_revocation(
    root: &Path,
    error: String,
    forbid_root_assets: impl FnOnce(&Path) -> Result<(), String>,
) -> String {
    match forbid_root_assets(root) {
        Ok(()) => error,
        Err(forbid_error) => {
            format!("{error}; failed to revoke changed asset root authorization: {forbid_error}")
        }
    }
}

struct ResourceStaging {
    directory: Dir,
    name: String,
}

fn random_staging_name() -> Result<String, String> {
    let mut entropy = [0_u8; 16];
    getrandom::fill(&mut entropy).map_err(|error| error.to_string())?;
    let encoded = entropy
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(format!("{RESOURCE_STAGING_PREFIX}{encoded}"))
}

fn create_resource_staging(target_folder: &Dir) -> Result<ResourceStaging, String> {
    for _ in 0..32 {
        let name = random_staging_name()?;
        match target_folder.create_dir(&name) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.to_string()),
        }
        let expected_identity = file_identity(
            &target_folder
                .symlink_metadata(&name)
                .map_err(|error| error.to_string())?,
        );
        let directory = target_folder
            .open_dir_nofollow(&name)
            .map_err(|error| error.to_string())?;
        let retained_identity = file_identity(
            &directory
                .dir_metadata()
                .map_err(|error| error.to_string())?,
        );
        if retained_identity != expected_identity {
            return Err("Resource staging directory changed during creation".to_string());
        }
        return Ok(ResourceStaging { directory, name });
    }
    Err("Could not create a resource staging directory".to_string())
}

fn cleanup_staging_after_error(staging: ResourceStaging, error: String) -> String {
    match staging.directory.remove_open_dir_all() {
        Ok(()) => error,
        Err(cleanup_error) => format!(
            "{error}; could not clean up partial resource staging '{}': {cleanup_error}",
            staging.name
        ),
    }
}

fn rollback_published_resource(
    target_folder: &Dir,
    target_name: &str,
    target_identity: FileIdentity,
    staging: ResourceStaging,
) -> Result<(), String> {
    match target_folder.rename(target_name, &staging.directory, QUARANTINED_RESOURCE_NAME) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return staging
                .directory
                .remove_open_dir_all()
                .map_err(|cleanup_error| cleanup_error.to_string());
        }
        Err(error) => {
            return Err(format!(
                "could not quarantine the published resource: {error}; staging '{}' was retained",
                staging.name
            ));
        }
    }
    let quarantined_identity = file_identity(
        &staging
            .directory
            .symlink_metadata(QUARANTINED_RESOURCE_NAME)
            .map_err(|error| error.to_string())?,
    );
    if quarantined_identity != target_identity {
        if let Err(error) =
            staging
                .directory
                .hard_link(QUARANTINED_RESOURCE_NAME, target_folder, target_name)
        {
            return Err(format!(
                "destination replacement could not be restored: {error}; staging '{}' was retained",
                staging.name
            ));
        }
    }
    staging
        .directory
        .remove_open_dir_all()
        .map_err(|error| error.to_string())
}

pub(super) fn write_unique_resource(
    root: &Path,
    markdown_directory: &Path,
    relative_folder: &Path,
    target_folder: &Dir,
    file_name: &str,
    validate_addressability: impl Fn(Option<(&str, FileIdentity)>) -> Result<(), String>,
    write_contents: impl FnOnce(&mut fs::File) -> io::Result<()>,
) -> Result<SavedProjectResource, String> {
    validate_addressability(None)?;
    let staging = create_resource_staging(target_folder)?;
    let mut options = cap_std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    let staged_file = match staging.directory.open_with(STAGED_RESOURCE_NAME, &options) {
        Ok(file) => file,
        Err(error) => return Err(cleanup_staging_after_error(staging, error.to_string())),
    };
    let target_identity = match staged_file.metadata() {
        Ok(metadata) => file_identity(&metadata),
        Err(error) => return Err(cleanup_staging_after_error(staging, error.to_string())),
    };
    if let Err(error) = validate_addressability(None) {
        drop(staged_file);
        return Err(cleanup_staging_after_error(staging, error));
    }
    let mut target = staged_file.into_std();
    if let Err(error) = write_contents(&mut target) {
        drop(target);
        return Err(cleanup_staging_after_error(staging, error.to_string()));
    }
    drop(target);

    let target_name = match (0..1000).find_map(|attempt| {
        let target_name = match unique_resource_file_name(file_name, attempt) {
            Ok(name) => name,
            Err(error) => return Some(Err(error)),
        };
        if let Err(error) = validate_addressability(None) {
            return Some(Err(error));
        }
        match staging
            .directory
            .hard_link(STAGED_RESOURCE_NAME, target_folder, &target_name)
        {
            Ok(()) => Some(Ok(target_name)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => None,
            Err(error) => Some(Err(error.to_string())),
        }
    }) {
        Some(Ok(name)) => name,
        Some(Err(error)) => return Err(cleanup_staging_after_error(staging, error)),
        None => {
            return Err(cleanup_staging_after_error(
                staging,
                "Could not create a unique project resource".to_string(),
            ))
        }
    };

    if let Err(error) = validate_addressability(Some((&target_name, target_identity))) {
        return match rollback_published_resource(
            target_folder,
            &target_name,
            target_identity,
            staging,
        ) {
            Ok(()) => Err(error),
            Err(cleanup_error) => Err(format!("{error}; resource cleanup failed: {cleanup_error}")),
        };
    }
    if let Err(error) = staging.directory.remove_open_dir_all() {
        return Err(format!(
            "Resource was published but staging cleanup failed for '{}': {error}",
            staging.name
        ));
    }
    Ok(SavedProjectResource {
        relative_path: markdown_relative_path(
            markdown_directory,
            &root.join(relative_folder).join(target_name),
        )?,
    })
}

fn existing_project_asset_reference_at(
    document_path: &Path,
    project_root: &Path,
    source_path: Option<&str>,
) -> Result<Option<SavedProjectResource>, String> {
    let Some(source_path) = source_path else {
        return Ok(None);
    };
    let assets_path = project_root.join(ASSETS_FOLDER);
    let assets_metadata = match fs::symlink_metadata(&assets_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    if assets_metadata.file_type().is_symlink() {
        return Err("Project assets directory cannot be a symbolic link".to_string());
    }
    if !assets_metadata.is_dir() {
        return Err("Project assets path must be a directory".to_string());
    }
    let canonical_assets = assets_path
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let source_path = PathBuf::from(source_path);
    let source_metadata = crate::storage_capability::ambient_symlink_metadata(&source_path)
        .map_err(|error| error.to_string())?;
    if source_metadata.file_type().is_symlink() {
        return Err("Project asset source cannot be a symbolic link".to_string());
    }
    if !source_metadata.is_file() {
        return Ok(None);
    }
    let canonical_source = source_path
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let Ok(relative_source) = canonical_source.strip_prefix(&canonical_assets) else {
        return Ok(None);
    };
    let mut current_parent = source_path.parent();
    loop {
        let Some(parent) = current_parent else {
            return Err("Project asset source is outside the assets directory".to_string());
        };
        let metadata = fs::symlink_metadata(parent).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() {
            return Err("Project asset source cannot contain a symbolic link".to_string());
        }
        let canonical_parent = parent.canonicalize().map_err(|error| error.to_string())?;
        if canonical_parent == canonical_assets {
            break;
        }
        if canonical_parent.strip_prefix(&canonical_assets).is_err() {
            return Err("Project asset source is outside the assets directory".to_string());
        }
        current_parent = parent.parent();
    }
    let source_name = relative_source
        .file_name()
        .ok_or_else(|| "Project asset source must name a file".to_string())?;
    let relative_parent = relative_source.parent().unwrap_or_else(|| Path::new(""));
    let assets_dir = Dir::open_ambient_dir(&canonical_assets, cap_std::ambient_authority())
        .map_err(|error| error.to_string())?;
    let source_parent = open_existing_folder(&assets_dir, relative_parent)
        .map_err(|error| format!("Project asset source changed: {error}"))?;
    let source = source_parent
        .open_with(source_name, &nonfollowing_read_options())
        .map_err(|error| format!("Project asset source changed: {error}"))?;
    let retained_metadata = source.metadata().map_err(|error| error.to_string())?;
    if !retained_metadata.is_file()
        || file_identity(&retained_metadata) != file_identity(&source_metadata)
    {
        return Err("Project asset source changed during authorization".to_string());
    }
    let markdown_directory = document_path
        .parent()
        .ok_or_else(|| "Current document folder is invalid".to_string())?;
    Ok(Some(SavedProjectResource {
        relative_path: markdown_relative_path(markdown_directory, &canonical_source)?,
    }))
}

fn validated_project_paths(
    document_path: &str,
    project_root_path: &str,
) -> Result<(PathBuf, PathBuf), String> {
    let document_path = PathBuf::from(document_path)
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if !document_path.is_file() || !is_markdown_open_file(&document_path) {
        return Err("Current document must be a saved Markdown file".to_string());
    }
    let project_root = PathBuf::from(project_root_path)
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if !project_root.is_dir() {
        return Err("Project root must be a directory".to_string());
    }
    document_path
        .strip_prefix(&project_root)
        .map_err(|_| "Current document must be inside the project".to_string())?;
    Ok((document_path, project_root))
}

#[cfg(desktop)]
pub(super) fn existing_project_asset_reference(
    document_path: &str,
    project_root_path: &str,
    source_path: Option<&str>,
) -> Result<Option<SavedProjectResource>, String> {
    let (document_path, project_root) = validated_project_paths(document_path, project_root_path)?;
    existing_project_asset_reference_at(&document_path, &project_root, source_path)
}

pub(super) fn save_project_resource_with_writer(
    document_path: String,
    project_root_path: String,
    file_name: String,
    source_path: Option<String>,
    allow_root_assets: impl FnOnce(&Path) -> Result<(), String>,
    forbid_root_assets: impl FnOnce(&Path) -> Result<(), String>,
    write_contents: impl FnOnce(&mut fs::File) -> io::Result<()>,
) -> Result<SavedProjectResource, String> {
    let (document_path, project_root) =
        validated_project_paths(&document_path, &project_root_path)?;
    if let Some(reference) =
        existing_project_asset_reference_at(&document_path, &project_root, source_path.as_deref())?
    {
        return Ok(reference);
    }

    let root_dir = Dir::open_ambient_dir(&project_root, cap_std::ambient_authority())
        .map_err(|error| error.to_string())?;
    let root_identity = file_identity(&root_dir.dir_metadata().map_err(|error| error.to_string())?);
    allow_root_assets(&project_root)?;
    let mut forbid_root_assets = Some(forbid_root_assets);
    if let Err(error) =
        verify_directory_path_identity(&project_root, root_identity, "Project resource root")
    {
        if let Some(forbid) = forbid_root_assets.take() {
            return Err(root_identity_error_with_revocation(
                &project_root,
                error,
                forbid,
            ));
        }
        return Err(error);
    }
    let target_folder = ensure_assets_folder(&root_dir)?;
    let target_identity = file_identity(
        &target_folder
            .dir_metadata()
            .map_err(|error| error.to_string())?,
    );
    let root_identity_changed = std::cell::Cell::new(false);
    let markdown_directory = document_path
        .parent()
        .ok_or_else(|| "Current document folder is invalid".to_string())?;
    let result = write_unique_resource(
        &project_root,
        markdown_directory,
        Path::new(ASSETS_FOLDER),
        &target_folder,
        &file_name,
        |published| {
            if let Err(error) = verify_directory_path_identity(
                &project_root,
                root_identity,
                "Project resource root",
            ) {
                root_identity_changed.set(true);
                return Err(error);
            }
            let lexical_assets = root_dir
                .open_dir_nofollow(ASSETS_FOLDER)
                .map_err(|error| format!("Project assets directory changed: {error}"))?;
            let lexical_identity = file_identity(
                &lexical_assets
                    .dir_metadata()
                    .map_err(|error| error.to_string())?,
            );
            if lexical_identity != target_identity {
                return Err("Project assets directory changed during resource save".to_string());
            }
            if let Some((target_name, expected_identity)) = published {
                let target = lexical_assets
                    .open_with(target_name, &nonfollowing_read_options())
                    .map_err(|error| format!("Published resource changed: {error}"))?;
                let metadata = target.metadata().map_err(|error| error.to_string())?;
                if !metadata.is_file() || file_identity(&metadata) != expected_identity {
                    return Err("Published resource changed during resource save".to_string());
                }
            }
            Ok(())
        },
        write_contents,
    );
    if root_identity_changed.get() {
        if let Some(forbid) = forbid_root_assets.take() {
            let error = result
                .err()
                .unwrap_or_else(|| "Project resource root changed during save".to_string());
            return Err(root_identity_error_with_revocation(
                &project_root,
                error,
                forbid,
            ));
        }
    }
    result
}

pub(super) fn save_standalone_resource_with_writer(
    document_path: String,
    folder: PathBuf,
    file_name: String,
    allow_root_assets: impl FnOnce(&Path) -> Result<(), String>,
    forbid_root_assets: impl FnOnce(&Path) -> Result<(), String>,
    write_contents: impl FnOnce(&mut fs::File) -> io::Result<()>,
) -> Result<SavedProjectResource, String> {
    let document_path = PathBuf::from(document_path)
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if !document_path.is_file() || !is_markdown_open_file(&document_path) {
        return Err("Current document must be a saved Markdown file".to_string());
    }
    let markdown_directory = document_path
        .parent()
        .ok_or_else(|| "Current document folder is invalid".to_string())?
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let root = markdown_directory.clone();
    let root_dir = Dir::open_ambient_dir(&root, cap_std::ambient_authority())
        .map_err(|error| error.to_string())?;
    let root_identity = file_identity(&root_dir.dir_metadata().map_err(|error| error.to_string())?);
    allow_root_assets(&root)?;
    let mut forbid_root_assets = Some(forbid_root_assets);
    if let Err(error) = verify_directory_path_identity(&root, root_identity, "Resource root") {
        if let Some(forbid) = forbid_root_assets.take() {
            return Err(root_identity_error_with_revocation(&root, error, forbid));
        }
        return Err(error);
    }
    let target_folder = ensure_resource_folder(&root_dir, &folder)?;
    let target_identity = file_identity(
        &target_folder
            .dir_metadata()
            .map_err(|error| error.to_string())?,
    );
    let root_identity_changed = std::cell::Cell::new(false);
    let result = write_unique_resource(
        &root,
        &markdown_directory,
        &folder,
        &target_folder,
        &file_name,
        |published| {
            if let Err(error) =
                verify_directory_path_identity(&root, root_identity, "Resource root")
            {
                root_identity_changed.set(true);
                return Err(error);
            }
            let lexical_folder = open_existing_folder(&root_dir, &folder)
                .map_err(|error| format!("Resource folder changed: {error}"))?;
            let lexical_identity = file_identity(
                &lexical_folder
                    .dir_metadata()
                    .map_err(|error| error.to_string())?,
            );
            if lexical_identity != target_identity {
                return Err("Resource folder changed during save".to_string());
            }
            if let Some((target_name, expected_identity)) = published {
                let target = lexical_folder
                    .open_with(target_name, &nonfollowing_read_options())
                    .map_err(|error| format!("Published resource changed: {error}"))?;
                let metadata = target.metadata().map_err(|error| error.to_string())?;
                if !metadata.is_file() || file_identity(&metadata) != expected_identity {
                    return Err("Published resource changed during save".to_string());
                }
            }
            Ok(())
        },
        write_contents,
    );
    if root_identity_changed.get() {
        if let Some(forbid) = forbid_root_assets.take() {
            let error = result
                .err()
                .unwrap_or_else(|| "Resource root changed during save".to_string());
            return Err(root_identity_error_with_revocation(&root, error, forbid));
        }
    }
    result
}

pub(super) fn save_project_resource_bytes(
    document_path: String,
    project_root_path: String,
    bytes: Vec<u8>,
    file_name: String,
    source_path: Option<String>,
    allow_root_assets: impl FnOnce(&Path) -> Result<(), String>,
    forbid_root_assets: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<SavedProjectResource, String> {
    save_project_resource_with_writer(
        document_path,
        project_root_path,
        file_name,
        source_path,
        allow_root_assets,
        forbid_root_assets,
        move |target| {
            if bytes.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Project resource is empty",
                ));
            }
            let mut contents = io::Cursor::new(bytes.as_slice());
            io::copy(&mut contents, target).map(|_| ())
        },
    )
}
