use std::ffi::OsStr;
use std::fmt;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use cap_fs_ext::{DirExt, MetadataExt};
use cap_std::fs::Dir;

use crate::ignore_rules::MarkdownIgnoreRules;
use crate::protected_paths::{is_protected_sync_relative_path, SYNC_MUTATION_STAGING_PREFIX};

pub(crate) const PORTABLE_SETTINGS_FILE: &str = "settings.json";
const STAGING_NAME_VERSION: &str = "v1";
const STAGING_EXPIRY_SECONDS: u64 = 24 * 60 * 60;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RemoteSyncIncludePolicy {
    Notes,
    ExactFile(&'static str),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RemoteContentValidator {
    None,
    PortableSettings,
}

pub struct RemoteSyncScope {
    source_directory: Dir,
    source_identity: DirectoryIdentity,
    source_root: PathBuf,
    state_root: PathBuf,
    state_directory: Dir,
    state_identity: StateDirectoryIdentity,
    manifest_name: String,
    conflict_root: PathBuf,
    staging_root: PathBuf,
    include: RemoteSyncIncludePolicy,
    notes_ignore_rules: Option<MarkdownIgnoreRules>,
    validator: RemoteContentValidator,
    local_identity: Option<String>,
    remote_first_restore: bool,
    restore_generation: Option<String>,
    run_nonce: String,
}

impl fmt::Debug for RemoteSyncScope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RemoteSyncScope { source: held, state: held }")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DirectoryIdentity {
    device: u64,
    inode: u64,
}

impl DirectoryIdentity {
    fn stable_token(self) -> String {
        format!("{:016x}{:016x}", self.device, self.inode)
    }
}

fn directory_identity(directory: &Dir) -> io::Result<DirectoryIdentity> {
    let metadata = directory.dir_metadata()?;
    Ok(DirectoryIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StateDirectoryIdentity {
    device: u64,
    inode: u64,
}

fn state_directory_identity<T: MetadataExt>(metadata: &T) -> StateDirectoryIdentity {
    StateDirectoryIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    }
}

impl RemoteSyncScope {
    pub fn notes(
        source_root: impl AsRef<Path>,
        state_root: impl AsRef<Path>,
        manifest_name: impl Into<String>,
        local_identity: Option<String>,
        global_ignore_rules: Option<String>,
    ) -> Result<Self, String> {
        let (source_root, source_directory, source_identity) =
            secure_source_root(source_root.as_ref())?;
        Self::notes_with_directory(
            source_root,
            source_directory,
            source_identity,
            state_root,
            manifest_name,
            local_identity,
            global_ignore_rules,
            false,
            None,
        )
    }

    pub fn notes_from_prepared_directory(
        source_root: PathBuf,
        source_directory: Dir,
        state_root: impl AsRef<Path>,
        manifest_name: impl Into<String>,
        local_identity: Option<String>,
        global_ignore_rules: Option<String>,
    ) -> Result<Self, String> {
        let restore_generation = directory_identity(&source_directory)
            .map_err(|_| "Remote sync source must be a folder".to_string())?
            .stable_token();
        Self::notes_from_prepared_directory_with_restore_generation(
            source_root,
            source_directory,
            state_root,
            manifest_name,
            local_identity,
            global_ignore_rules,
            restore_generation,
        )
    }

    pub fn notes_from_prepared_directory_with_restore_generation(
        source_root: PathBuf,
        source_directory: Dir,
        state_root: impl AsRef<Path>,
        manifest_name: impl Into<String>,
        local_identity: Option<String>,
        global_ignore_rules: Option<String>,
        restore_generation: String,
    ) -> Result<Self, String> {
        Self::notes_from_remote_first_directory(
            source_root,
            source_directory,
            state_root,
            manifest_name,
            local_identity,
            global_ignore_rules,
            restore_generation,
        )
    }

    pub fn notes_from_managed_bootstrap(
        source_root: PathBuf,
        source_directory: Dir,
        state_root: impl AsRef<Path>,
        manifest_name: impl Into<String>,
        local_identity: Option<String>,
        global_ignore_rules: Option<String>,
    ) -> Result<Self, String> {
        let restore_generation = directory_identity(&source_directory)
            .map_err(|_| "Remote sync source must be a folder".to_string())?
            .stable_token();
        Self::notes_from_managed_bootstrap_with_restore_generation(
            source_root,
            source_directory,
            state_root,
            manifest_name,
            local_identity,
            global_ignore_rules,
            restore_generation,
        )
    }

    pub fn notes_from_managed_bootstrap_with_restore_generation(
        source_root: PathBuf,
        source_directory: Dir,
        state_root: impl AsRef<Path>,
        manifest_name: impl Into<String>,
        local_identity: Option<String>,
        global_ignore_rules: Option<String>,
        restore_generation: String,
    ) -> Result<Self, String> {
        Self::notes_from_remote_first_directory(
            source_root,
            source_directory,
            state_root,
            manifest_name,
            local_identity,
            global_ignore_rules,
            restore_generation,
        )
    }

    fn notes_from_remote_first_directory(
        source_root: PathBuf,
        source_directory: Dir,
        state_root: impl AsRef<Path>,
        manifest_name: impl Into<String>,
        local_identity: Option<String>,
        global_ignore_rules: Option<String>,
        restore_generation: String,
    ) -> Result<Self, String> {
        if restore_generation.is_empty() || restore_generation.len() > 512 {
            return Err("Remote restore generation is invalid".to_string());
        }
        let source_identity = directory_identity(&source_directory)
            .map_err(|_| "Remote sync source must be a folder".to_string())?;
        Self::notes_with_directory(
            source_root,
            source_directory,
            source_identity,
            state_root,
            manifest_name,
            local_identity,
            global_ignore_rules,
            true,
            Some(restore_generation),
        )
    }

    #[allow(clippy::too_many_arguments)] // Constructor keeps retained capability parts explicit.
    fn notes_with_directory(
        source_root: PathBuf,
        source_directory: Dir,
        source_identity: DirectoryIdentity,
        state_root: impl AsRef<Path>,
        manifest_name: impl Into<String>,
        local_identity: Option<String>,
        global_ignore_rules: Option<String>,
        remote_first_restore: bool,
        restore_generation: Option<String>,
    ) -> Result<Self, String> {
        let requested_state = state_root.as_ref();
        if requested_state.starts_with(&source_root) {
            return Err("Remote sync state root must be outside the notes source root".into());
        }
        let (state_root, state_directory, state_identity) = secure_state_root(requested_state)?;
        if state_root.starts_with(&source_root) {
            return Err("Remote sync state root must be outside the notes source root".into());
        }
        let notes_ignore_rules = Some(
            MarkdownIgnoreRules::try_for_retained_root(
                &source_root,
                &source_directory,
                global_ignore_rules.as_deref(),
            )
            .map_err(|_| "Remote sync ignore rules are unavailable".to_string())?,
        );
        Self::new(
            source_directory,
            source_identity,
            source_root,
            state_root,
            state_directory,
            state_identity,
            manifest_name.into(),
            RemoteSyncIncludePolicy::Notes,
            notes_ignore_rules,
            RemoteContentValidator::None,
            local_identity,
            remote_first_restore,
            restore_generation,
        )
    }

    pub fn portable_settings(
        app_data_root: impl AsRef<Path>,
        state_root: impl AsRef<Path>,
        manifest_name: impl Into<String>,
    ) -> Result<Self, String> {
        let app_data_root = app_data_root
            .as_ref()
            .canonicalize()
            .map_err(|_| "Portable settings app data is unavailable".to_string())?;
        let requested_state = state_root.as_ref();
        if requested_state == app_data_root || !requested_state.starts_with(&app_data_root) {
            return Err("Portable settings state root must be inside app data".into());
        }
        let (source_root, _, _) = secure_state_root(requested_state)?;
        if source_root == app_data_root || !source_root.starts_with(&app_data_root) {
            return Err("Portable settings state root must be inside app data".into());
        }
        let (source_root, source_directory, source_identity) = secure_source_root(&source_root)?;
        let (state_root, state_directory, state_identity) =
            secure_state_root(&source_root.join("engine"))?;
        Self::new(
            source_directory,
            source_identity,
            source_root,
            state_root,
            state_directory,
            state_identity,
            manifest_name.into(),
            RemoteSyncIncludePolicy::ExactFile(PORTABLE_SETTINGS_FILE),
            None,
            RemoteContentValidator::PortableSettings,
            None,
            false,
            None,
        )
    }

    pub(crate) fn portable_settings_from_prepared_directory(
        app_data_root: PathBuf,
        app_data_directory: Dir,
        state_relative: impl AsRef<Path>,
        manifest_name: impl Into<String>,
    ) -> Result<Self, String> {
        if !app_data_root.is_absolute() {
            return Err("Portable settings app data is unavailable".to_string());
        }
        let state_relative = state_relative.as_ref();
        let source_directory =
            open_or_create_relative_directory_nofollow(&app_data_directory, state_relative)?;
        let source_identity = directory_identity(&source_directory)
            .map_err(|_| "Portable settings state root is unavailable".to_string())?;
        let source_root = app_data_root.join(state_relative);
        let state_directory = open_or_create_child_directory_nofollow(&source_directory, "engine")?;
        let state_identity = state_directory
            .dir_metadata()
            .map(|metadata| state_directory_identity(&metadata))
            .map_err(|_| unsafe_state_root_error())?;
        let state_root = source_root.join("engine");
        Self::new(
            source_directory,
            source_identity,
            source_root,
            state_root,
            state_directory,
            state_identity,
            manifest_name.into(),
            RemoteSyncIncludePolicy::ExactFile(PORTABLE_SETTINGS_FILE),
            None,
            RemoteContentValidator::PortableSettings,
            None,
            false,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)] // Scope invariants are assembled in one audited boundary.
    fn new(
        source_directory: Dir,
        source_identity: DirectoryIdentity,
        source_root: PathBuf,
        state_root: PathBuf,
        state_directory: Dir,
        state_identity: StateDirectoryIdentity,
        manifest_name: String,
        include: RemoteSyncIncludePolicy,
        notes_ignore_rules: Option<MarkdownIgnoreRules>,
        validator: RemoteContentValidator,
        local_identity: Option<String>,
        remote_first_restore: bool,
        restore_generation: Option<String>,
    ) -> Result<Self, String> {
        validate_state_component(&manifest_name)?;
        Ok(Self {
            source_directory,
            source_identity,
            source_root,
            conflict_root: state_root.join("conflicts"),
            staging_root: state_root.join("staging"),
            state_root,
            state_directory,
            state_identity,
            manifest_name,
            include,
            notes_ignore_rules,
            validator,
            local_identity,
            remote_first_restore,
            restore_generation,
            run_nonce: random_run_nonce()?,
        })
    }

    pub fn source_root(&self) -> &Path {
        &self.source_root
    }

    pub fn open_source_root(&self) -> Result<Dir, String> {
        let directory = self
            .source_directory
            .try_clone()
            .map_err(|_| "Remote sync source is unavailable".to_string())?;
        if directory_identity(&directory)
            .map_err(|_| "Remote sync source is unavailable".to_string())?
            != self.source_identity
        {
            return Err("Remote sync source is unavailable".to_string());
        }
        Ok(directory)
    }

    pub fn open_state_root(&self) -> Result<Dir, String> {
        let retained = self
            .state_directory
            .try_clone()
            .map_err(|_| unsafe_state_root_error())?;
        let retained_identity = retained
            .dir_metadata()
            .map(|metadata| state_directory_identity(&metadata))
            .map_err(|_| unsafe_state_root_error())?;
        if retained_identity != self.state_identity {
            return Err(unsafe_state_root_error());
        }
        let directory = open_existing_directory_nofollow(&self.state_root)?;
        let identity = directory
            .dir_metadata()
            .map(|metadata| state_directory_identity(&metadata))
            .map_err(|_| unsafe_state_root_error())?;
        if identity != self.state_identity {
            return Err(unsafe_state_root_error());
        }
        Ok(retained)
    }

    pub fn open_or_create_state_directory(&self, name: &str) -> Result<Dir, String> {
        validate_state_component(name)?;
        let root = self.open_state_root()?;
        match root.symlink_metadata(name) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(unsafe_state_root_error())
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if let Err(error) = root.create_dir(name) {
                    if error.kind() != io::ErrorKind::AlreadyExists {
                        return Err(unsafe_state_root_error());
                    }
                } else {
                    sync_created_directory_parent(&root)?;
                }
            }
            Err(_) => return Err(unsafe_state_root_error()),
        }
        open_child_directory_nofollow(&root, OsStr::new(name))
    }

    pub fn manifest_name(&self) -> &str {
        &self.manifest_name
    }

    #[doc(hidden)]
    pub fn state_root(&self) -> &Path {
        &self.state_root
    }

    pub fn conflict_root(&self) -> &Path {
        &self.conflict_root
    }

    pub fn staging_root(&self) -> &Path {
        &self.staging_root
    }

    pub fn local_identity(&self) -> Option<&str> {
        self.local_identity.as_deref()
    }

    pub fn remote_first_restore(&self) -> bool {
        self.remote_first_restore
    }

    pub fn restore_generation(&self) -> Option<&str> {
        self.restore_generation.as_deref()
    }

    pub fn includes_relative_path(&self, relative_path: &str, is_directory: bool) -> bool {
        if is_protected_sync_relative_path(relative_path) {
            return false;
        }
        match &self.include {
            RemoteSyncIncludePolicy::Notes => !self
                .notes_ignore_rules
                .as_ref()
                .expect("notes scopes always have ignore rules")
                .ignores(&self.source_root.join(relative_path), is_directory),
            RemoteSyncIncludePolicy::ExactFile(file) => !is_directory && relative_path == *file,
        }
    }

    pub fn validate_download(&self, bytes: &[u8]) -> Result<(), String> {
        match self.validator {
            RemoteContentValidator::None => Ok(()),
            RemoteContentValidator::PortableSettings => {
                crate::settings::model::validate_portable_settings_bytes(bytes)
                    .map_err(|error| error.to_string())
            }
        }
    }

    pub fn validate_upload(&self, bytes: &[u8]) -> Result<(), String> {
        self.validate_download(bytes)
    }

    pub fn remote_wins_without_baseline(&self) -> bool {
        matches!(
            self.include,
            RemoteSyncIncludePolicy::ExactFile(PORTABLE_SETTINGS_FILE)
        )
    }

    pub fn publishes_conflicts_to_source(&self) -> bool {
        matches!(self.include, RemoteSyncIncludePolicy::Notes)
    }

    pub fn state_staging_name(&self, sequence: usize) -> String {
        staging_name("state", &self.run_nonce, sequence)
    }

    pub fn publication_temp_name(&self, sequence: usize) -> String {
        staging_name("publish", &self.run_nonce, sequence)
    }

    pub fn quarantine_temp_name(&self, sequence: usize) -> String {
        format!(
            "{SYNC_MUTATION_STAGING_PREFIX}quarantine-{}-{sequence}",
            std::process::id()
        )
    }

    pub fn should_cleanup_state_staging(&self, name: &OsStr) -> bool {
        should_cleanup_staging_kind(name, "state", &self.run_nonce)
    }

    pub fn should_cleanup_publication_temp(&self, name: &OsStr) -> bool {
        should_cleanup_staging_kind(name, "publish", &self.run_nonce)
    }
}

fn staging_name(kind: &str, run_nonce: &str, sequence: usize) -> String {
    let created = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!(
        "{SYNC_MUTATION_STAGING_PREFIX}{kind}-{STAGING_NAME_VERSION}-{}-{created}-{run_nonce}-{sequence}",
        std::process::id(),
    )
}

fn random_run_nonce() -> Result<String, String> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut entropy = [0_u8; 16];
    getrandom::fill(&mut entropy)
        .map_err(|_| "Remote sync staging nonce is unavailable".to_string())?;
    let mut nonce = String::with_capacity(entropy.len() * 2);
    for byte in entropy {
        nonce.push(HEX[(byte >> 4) as usize] as char);
        nonce.push(HEX[(byte & 0x0f) as usize] as char);
    }
    Ok(nonce)
}

fn should_cleanup_staging_kind(name: &OsStr, kind: &str, run_nonce: &str) -> bool {
    let prefix = format!("{SYNC_MUTATION_STAGING_PREFIX}{kind}-{STAGING_NAME_VERSION}-");
    let Some(remainder) = name.to_str().and_then(|name| name.strip_prefix(&prefix)) else {
        return false;
    };
    let mut parts = remainder.split('-');
    let Some(_pid) = parts.next().and_then(|value| value.parse::<u32>().ok()) else {
        return false;
    };
    let Some(created) = parts.next().and_then(|value| value.parse::<u64>().ok()) else {
        return false;
    };
    let Some(nonce) = parts.next().filter(|value| {
        value.len() == 32
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }) else {
        return false;
    };
    if parts
        .next()
        .and_then(|value| value.parse::<usize>().ok())
        .is_none()
        || parts.next().is_some()
    {
        return false;
    }
    if nonce == run_nonce {
        return true;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    now.saturating_sub(created) >= STAGING_EXPIRY_SECONDS && created <= now
}

fn secure_source_root(path: &Path) -> Result<(PathBuf, Dir, DirectoryIdentity), String> {
    let canonical = path
        .canonicalize()
        .map_err(|_| "Remote sync source must be a folder".to_string())?;
    let directory = open_canonical_directory_nofollow(&canonical)
        .map_err(|_| "Remote sync source must be a folder".to_string())?;
    let identity = directory_identity(&directory)
        .map_err(|_| "Remote sync source must be a folder".to_string())?;
    Ok((canonical, directory, identity))
}

fn open_canonical_directory_nofollow(path: &Path) -> io::Result<Dir> {
    let Some(parent) = path.parent() else {
        return Dir::open_ambient_dir(path, cap_std::ambient_authority());
    };
    let Some(name) = path.file_name() else {
        return Dir::open_ambient_dir(path, cap_std::ambient_authority());
    };
    let parent = Dir::open_ambient_dir(parent, cap_std::ambient_authority())?;
    parent.open_dir_nofollow(name)
}

fn unsafe_state_root_error() -> String {
    "Remote sync state root is unsafe".into()
}

fn secure_state_root(path: &Path) -> Result<(PathBuf, Dir, StateDirectoryIdentity), String> {
    if !path.is_absolute() {
        return Err(unsafe_state_root_error());
    }
    let directory = open_or_create_directory_nofollow(path)?;
    let canonical = path.canonicalize().map_err(|_| unsafe_state_root_error())?;
    let identity = directory
        .dir_metadata()
        .map(|metadata| state_directory_identity(&metadata))
        .map_err(|_| unsafe_state_root_error())?;
    Ok((canonical, directory, identity))
}

fn open_or_create_relative_directory_nofollow(root: &Dir, relative: &Path) -> Result<Dir, String> {
    let mut directory = root.try_clone().map_err(|_| unsafe_state_root_error())?;
    let mut saw_component = false;
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(unsafe_state_root_error());
        };
        saw_component = true;
        directory = open_or_create_child_directory_nofollow(&directory, name)?;
    }
    if !saw_component {
        return Err(unsafe_state_root_error());
    }
    Ok(directory)
}

fn open_or_create_child_directory_nofollow(
    parent: &Dir,
    name: impl AsRef<OsStr>,
) -> Result<Dir, String> {
    let name = name.as_ref();
    match parent.symlink_metadata(name) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(unsafe_state_root_error());
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if let Err(error) = parent.create_dir(name) {
                if error.kind() != io::ErrorKind::AlreadyExists {
                    return Err(unsafe_state_root_error());
                }
            } else {
                sync_created_directory_parent(parent)?;
            }
        }
        Err(_) => return Err(unsafe_state_root_error()),
    }
    open_child_directory_nofollow(parent, name)
}

fn open_or_create_directory_nofollow(path: &Path) -> Result<Dir, String> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err(unsafe_state_root_error())
        }
        Ok(_) => open_existing_directory_nofollow(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let parent = path.parent().ok_or_else(unsafe_state_root_error)?;
            let name = path.file_name().ok_or_else(unsafe_state_root_error)?;
            let parent = open_or_create_directory_nofollow(parent)?;
            if let Err(error) = parent.create_dir(name) {
                if error.kind() != io::ErrorKind::AlreadyExists {
                    return Err(unsafe_state_root_error());
                }
            } else {
                sync_created_directory_parent(&parent)?;
            }
            open_child_directory_nofollow(&parent, name)
        }
        Err(_) => Err(unsafe_state_root_error()),
    }
}

#[cfg(unix)]
fn sync_created_directory_parent(parent: &Dir) -> Result<(), String> {
    crate::storage::sync_directory(parent).map_err(|_| unsafe_state_root_error())
}

#[cfg(not(unix))]
fn sync_created_directory_parent(_parent: &Dir) -> Result<(), String> {
    Ok(())
}

fn open_existing_directory_nofollow(path: &Path) -> Result<Dir, String> {
    let Some(name) = path.file_name() else {
        return Dir::open_ambient_dir(path, cap_std::ambient_authority())
            .map_err(|_| unsafe_state_root_error());
    };
    let parent_path = path.parent().ok_or_else(unsafe_state_root_error)?;
    let parent = open_existing_directory_nofollow(parent_path)?;
    open_child_directory_nofollow(&parent, name)
}

fn open_child_directory_nofollow(parent: &Dir, name: &OsStr) -> Result<Dir, String> {
    let addressed = parent
        .symlink_metadata(name)
        .map_err(|_| unsafe_state_root_error())?;
    if addressed.file_type().is_symlink() || !addressed.is_dir() {
        return Err(unsafe_state_root_error());
    }
    let child = parent
        .open_dir_nofollow(name)
        .map_err(|_| unsafe_state_root_error())?;
    let retained = child
        .dir_metadata()
        .map_err(|_| unsafe_state_root_error())?;
    if state_directory_identity(&addressed) != state_directory_identity(&retained) {
        return Err(unsafe_state_root_error());
    }
    Ok(child)
}

fn validate_state_component(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.contains(['/', '\\', '\0'])
        || Path::new(value)
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("Remote sync state file name is unsafe".into());
    }
    Ok(())
}

#[cfg(test)]
mod prepared_directory_tests {
    use std::fs;

    use cap_std::fs::Dir;
    use tempfile::tempdir;

    use super::RemoteSyncScope;

    #[test]
    fn prepared_portable_settings_never_creates_state_in_a_replacement_app_data_root() {
        let temporary = tempdir().expect("temporary root");
        let app_data = temporary.path().join("app-data");
        let retained = temporary.path().join("retained-app-data");
        fs::create_dir(&app_data).expect("app data");
        let directory = Dir::open_ambient_dir(&app_data, cap_std::ambient_authority())
            .expect("retained app data");
        fs::rename(&app_data, &retained).expect("retain original app data address");
        fs::create_dir(&app_data).expect("replacement app data");

        let scope = RemoteSyncScope::portable_settings_from_prepared_directory(
            app_data.clone(),
            directory,
            "sync-state/settings/provider",
            "manifest.json",
        )
        .expect("capability-based settings scope");

        assert!(retained
            .join("sync-state/settings/provider/engine")
            .is_dir());
        assert_eq!(
            fs::read_dir(&app_data).expect("replacement root").count(),
            0
        );
        scope
            .open_source_root()
            .expect("retained source remains open");
        assert!(scope.open_state_root().is_err());
    }
}
