//! Workspace ignore rules shared by document discovery, watchers, and sync.

use std::ffi::OsStr;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cap_std::fs::{Dir, Metadata};
use ignore::gitignore::{Gitignore, GitignoreBuilder};

use crate::inventory_snapshot::FileVersionStamp;
use crate::protected_paths::path_contains_qingyu_control_directory;
use crate::{
    contract::{DocumentKind, WorkspaceRelativePath},
    documents::{AllowAllDocumentIgnorePort, DocumentIgnorePort},
    settings::service::{SettingsGroup, SettingsService},
};

pub const MARKRA_IGNORE_FILE_NAME: &str = ".markraignore";
const MAX_MARKRA_IGNORE_BYTES: u64 = 1024 * 1024;
const MAX_GLOBAL_IGNORE_RULE_UNITS: usize = 50_000;

pub trait WorkspaceIgnorePort: Send + Sync {
    fn capture(
        &self,
        root_path: &Path,
        retained_root: &Dir,
    ) -> Result<WorkspaceIgnoreSnapshot, WorkspaceIgnoreError>;
}

#[derive(Clone)]
pub struct WorkspaceIgnoreSnapshot {
    matcher: Arc<dyn DocumentIgnorePort>,
}

impl WorkspaceIgnoreSnapshot {
    pub fn from_matcher(matcher: Arc<dyn DocumentIgnorePort>) -> Self {
        Self { matcher }
    }

    pub fn is_ignored(&self, path: &WorkspaceRelativePath, kind: DocumentKind) -> bool {
        self.matcher.is_ignored(path, kind)
    }
}

pub struct StaticWorkspaceIgnorePort {
    snapshot: WorkspaceIgnoreSnapshot,
}

impl StaticWorkspaceIgnorePort {
    pub fn new(matcher: Arc<dyn DocumentIgnorePort>) -> Self {
        Self {
            snapshot: WorkspaceIgnoreSnapshot::from_matcher(matcher),
        }
    }
}

impl WorkspaceIgnorePort for StaticWorkspaceIgnorePort {
    fn capture(
        &self,
        _root_path: &Path,
        _retained_root: &Dir,
    ) -> Result<WorkspaceIgnoreSnapshot, WorkspaceIgnoreError> {
        Ok(self.snapshot.clone())
    }
}

#[derive(Default)]
pub struct AllowAllWorkspaceIgnorePort;

impl WorkspaceIgnorePort for AllowAllWorkspaceIgnorePort {
    fn capture(
        &self,
        _root_path: &Path,
        _retained_root: &Dir,
    ) -> Result<WorkspaceIgnoreSnapshot, WorkspaceIgnoreError> {
        Ok(WorkspaceIgnoreSnapshot::from_matcher(Arc::new(
            AllowAllDocumentIgnorePort,
        )))
    }
}

pub struct SettingsWorkspaceIgnorePort {
    settings: Arc<SettingsService>,
}

impl SettingsWorkspaceIgnorePort {
    pub fn new(settings: Arc<SettingsService>) -> Self {
        Self { settings }
    }
}

impl WorkspaceIgnorePort for SettingsWorkspaceIgnorePort {
    fn capture(
        &self,
        root_path: &Path,
        retained_root: &Dir,
    ) -> Result<WorkspaceIgnoreSnapshot, WorkspaceIgnoreError> {
        let global_rules = self
            .settings
            .read_group(SettingsGroup::FileIgnoreSettings)
            .map_err(|_| WorkspaceIgnoreError)?
            .map(|value| {
                let object = value.as_object().ok_or(WorkspaceIgnoreError)?;
                if object.len() != 1 || !object.contains_key("rules") {
                    return Err(WorkspaceIgnoreError);
                }
                object["rules"]
                    .as_str()
                    .map(str::to_owned)
                    .ok_or(WorkspaceIgnoreError)
            })
            .transpose()?;
        let rules = MarkdownIgnoreRules::try_for_retained_root(
            root_path,
            retained_root,
            global_rules.as_deref(),
        )?;
        Ok(WorkspaceIgnoreSnapshot::from_matcher(Arc::new(
            CapturedMarkdownIgnore {
                root: root_path.to_path_buf(),
                rules,
            },
        )))
    }
}

struct CapturedMarkdownIgnore {
    root: PathBuf,
    rules: MarkdownIgnoreRules,
}

impl DocumentIgnorePort for CapturedMarkdownIgnore {
    fn is_ignored(&self, path: &WorkspaceRelativePath, kind: DocumentKind) -> bool {
        self.rules.ignores(
            &self.root.join(path.as_str()),
            kind == DocumentKind::Directory,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceIgnoreError;

impl std::fmt::Display for WorkspaceIgnoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("workspace ignore rules are unavailable")
    }
}

impl std::error::Error for WorkspaceIgnoreError {}

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
    root: PathBuf,
    matcher: Gitignore,
}

impl MarkdownIgnoreRules {
    #[cfg(test)]
    pub fn for_root(root: &Path, global_rules: Option<&str>) -> Self {
        let workspace_rules = std::fs::read_to_string(root.join(MARKRA_IGNORE_FILE_NAME)).ok();
        Self::try_from_rules(root, global_rules, workspace_rules.as_deref(), true)
            .expect("test ignore rules should be valid")
    }

    #[cfg(test)]
    pub fn built_in_only(root: &Path) -> Self {
        Self::try_from_rules(root, None, None, false)
            .expect("built-in test ignore rules should be valid")
    }

    pub fn try_for_retained_root(
        root: &Path,
        directory: &Dir,
        global_rules: Option<&str>,
    ) -> Result<Self, WorkspaceIgnoreError> {
        let workspace_rules = read_retained_workspace_rules(directory)?;
        Self::try_from_rules(root, global_rules, workspace_rules.as_deref(), true)
    }

    fn try_from_rules(
        root: &Path,
        global_rules: Option<&str>,
        workspace_rules: Option<&str>,
        include_workspace_rules: bool,
    ) -> Result<Self, WorkspaceIgnoreError> {
        let global_rules = global_rules.unwrap_or_default();
        if global_rules.len() > MAX_GLOBAL_IGNORE_RULE_UNITS {
            return Err(WorkspaceIgnoreError);
        }
        let mut builder = GitignoreBuilder::new(root);
        for line in global_rules.lines() {
            builder
                .add_line(None, line)
                .map_err(|_| WorkspaceIgnoreError)?;
        }
        if include_workspace_rules {
            let workspace_rules_path = root.join(MARKRA_IGNORE_FILE_NAME);
            for line in workspace_rules.unwrap_or_default().lines() {
                builder
                    .add_line(Some(workspace_rules_path.clone()), line)
                    .map_err(|_| WorkspaceIgnoreError)?;
            }
        }
        let matcher = builder.build().map_err(|_| WorkspaceIgnoreError)?;
        Ok(Self {
            root: root.to_path_buf(),
            matcher,
        })
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

fn read_retained_workspace_rules(directory: &Dir) -> Result<Option<String>, WorkspaceIgnoreError> {
    read_retained_workspace_rules_inner(directory, || {})
}

#[cfg(test)]
fn read_retained_workspace_rules_with_hook(
    directory: &Dir,
    after_read: impl FnOnce(),
) -> Result<Option<String>, WorkspaceIgnoreError> {
    read_retained_workspace_rules_inner(directory, after_read)
}

fn read_retained_workspace_rules_inner(
    directory: &Dir,
    after_read: impl FnOnce(),
) -> Result<Option<String>, WorkspaceIgnoreError> {
    let addressed = match directory.symlink_metadata(MARKRA_IGNORE_FILE_NAME) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(WorkspaceIgnoreError),
    };
    validate_ignore_file(&addressed)?;
    let addressed_stamp = FileVersionStamp::capture_metadata(&addressed);
    let addressed_modified = addressed.modified().map_err(|_| WorkspaceIgnoreError)?;

    let mut file = directory
        .open_with(
            MARKRA_IGNORE_FILE_NAME,
            &crate::storage::nonfollowing_read_options(),
        )
        .map_err(|_| WorkspaceIgnoreError)?;
    let retained = file.metadata().map_err(|_| WorkspaceIgnoreError)?;
    validate_ignore_file(&retained)?;
    let retained_stamp = FileVersionStamp::capture_metadata(&retained);
    let retained_modified = retained.modified().map_err(|_| WorkspaceIgnoreError)?;
    if !same_ignore_file(&addressed, &retained)
        || addressed.len() != retained.len()
        || addressed_modified != retained_modified
        || addressed_stamp != retained_stamp
    {
        return Err(WorkspaceIgnoreError);
    }
    let bytes = read_bounded_ignore_bytes(&mut file, retained.len())?;
    after_read();

    if retained_stamp.strong().is_none() {
        file.seek(SeekFrom::Start(0))
            .map_err(|_| WorkspaceIgnoreError)?;
        let verification = read_bounded_ignore_bytes(&mut file, retained.len())?;
        if verification != bytes {
            return Err(WorkspaceIgnoreError);
        }
    }

    let after = file.metadata().map_err(|_| WorkspaceIgnoreError)?;
    let named = directory
        .symlink_metadata(MARKRA_IGNORE_FILE_NAME)
        .map_err(|_| WorkspaceIgnoreError)?;
    validate_ignore_file(&after)?;
    validate_ignore_file(&named)?;
    let after_modified = after.modified().map_err(|_| WorkspaceIgnoreError)?;
    let named_modified = named.modified().map_err(|_| WorkspaceIgnoreError)?;
    if !same_ignore_file(&retained, &after)
        || !same_ignore_file(&retained, &named)
        || retained.len() != after.len()
        || retained.len() != named.len()
        || retained_modified != after_modified
        || retained_modified != named_modified
        || retained_stamp != FileVersionStamp::capture_metadata(&after)
        || retained_stamp != FileVersionStamp::capture_metadata(&named)
    {
        return Err(WorkspaceIgnoreError);
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| WorkspaceIgnoreError)
}

fn validate_ignore_file(metadata: &Metadata) -> Result<(), WorkspaceIgnoreError> {
    if trusted_ignore_file(metadata) && metadata.len() <= MAX_MARKRA_IGNORE_BYTES {
        Ok(())
    } else {
        Err(WorkspaceIgnoreError)
    }
}

fn read_bounded_ignore_bytes(
    file: &mut cap_std::fs::File,
    expected_length: u64,
) -> Result<Vec<u8>, WorkspaceIgnoreError> {
    let mut bytes = Vec::with_capacity(expected_length as usize);
    file.by_ref()
        .take(MAX_MARKRA_IGNORE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| WorkspaceIgnoreError)?;
    if bytes.len() as u64 != expected_length || bytes.len() as u64 > MAX_MARKRA_IGNORE_BYTES {
        return Err(WorkspaceIgnoreError);
    }
    Ok(bytes)
}

fn trusted_ignore_file(metadata: &cap_std::fs::Metadata) -> bool {
    metadata.is_file() && !metadata.file_type().is_symlink() && ignore_link_count(metadata) == 1
}

#[cfg(unix)]
fn ignore_link_count(metadata: &cap_std::fs::Metadata) -> u64 {
    cap_fs_ext::MetadataExt::nlink(metadata)
}

#[cfg(windows)]
fn ignore_link_count(metadata: &cap_std::fs::Metadata) -> u64 {
    use cap_std::fs::MetadataExt as _;

    metadata.number_of_links().map_or(0, u64::from)
}

#[cfg(not(any(unix, windows)))]
fn ignore_link_count(_metadata: &cap_std::fs::Metadata) -> u64 {
    1
}

#[cfg(unix)]
fn same_ignore_file(left: &cap_std::fs::Metadata, right: &cap_std::fs::Metadata) -> bool {
    cap_fs_ext::MetadataExt::dev(left) == cap_fs_ext::MetadataExt::dev(right)
        && cap_fs_ext::MetadataExt::ino(left) == cap_fs_ext::MetadataExt::ino(right)
}

#[cfg(windows)]
fn same_ignore_file(left: &cap_std::fs::Metadata, right: &cap_std::fs::Metadata) -> bool {
    use cap_std::fs::MetadataExt as _;

    matches!(
        (
            left.volume_serial_number(),
            left.file_index(),
            right.volume_serial_number(),
            right.file_index(),
        ),
        (Some(left_volume), Some(left_file), Some(right_volume), Some(right_file))
            if left_volume == right_volume && left_file == right_file
    )
}

#[cfg(not(any(unix, windows)))]
fn same_ignore_file(_left: &cap_std::fs::Metadata, _right: &cap_std::fs::Metadata) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeMap, fs, sync::Mutex};

    use crate::protected_paths::{LEGACY_SYNC_DIR, QINGYU_CONTROL_DIR};
    use crate::{
        ports::system::NoopEventSink,
        settings::storage::{SettingsStore, SettingsStoreError},
    };
    use serde_json::{json, Map, Value};

    #[derive(Default)]
    struct MemorySettingsStore {
        values: Mutex<BTreeMap<String, Value>>,
    }

    impl MemorySettingsStore {
        fn put(&self, key: &str, value: Value) {
            self.values.lock().unwrap().insert(key.to_string(), value);
        }
    }

    impl SettingsStore for MemorySettingsStore {
        fn get(&self, key: &str) -> Result<Option<Value>, SettingsStoreError> {
            Ok(self.values.lock().unwrap().get(key).cloned())
        }

        fn set(&self, key: &str, value: Value) -> Result<(), SettingsStoreError> {
            self.put(key, value);
            Ok(())
        }

        fn delete(&self, key: &str) -> Result<(), SettingsStoreError> {
            self.values.lock().unwrap().remove(key);
            Ok(())
        }

        fn save(&self) -> Result<(), SettingsStoreError> {
            Ok(())
        }

        fn replace_portable_atomically(
            &self,
            desired: &Map<String, Value>,
        ) -> Result<(), SettingsStoreError> {
            *self.values.lock().unwrap() = desired.clone().into_iter().collect();
            Ok(())
        }
    }

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
    fn settings_provider_captures_one_immutable_global_and_workspace_rule_set() {
        let root = test_root("captured-provider");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join(MARKRA_IGNORE_FILE_NAME), "workspace-old.bin\n").unwrap();
        let retained = Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap();
        let store = Arc::new(MemorySettingsStore::default());
        store.put("fileIgnoreSettings", json!({ "rules": "global-old.bin\n" }));
        let settings = Arc::new(SettingsService::new(store.clone(), Arc::new(NoopEventSink)));
        let provider = SettingsWorkspaceIgnorePort::new(settings);

        let captured = provider.capture(&root, &retained).unwrap();
        store.put("fileIgnoreSettings", json!({ "rules": "global-new.bin\n" }));
        fs::write(root.join(MARKRA_IGNORE_FILE_NAME), "workspace-new.bin\n").unwrap();

        for path in ["global-old.bin", "workspace-old.bin"] {
            assert!(captured.is_ignored(
                &WorkspaceRelativePath::parse(path).unwrap(),
                DocumentKind::File
            ));
        }
        for path in ["global-new.bin", "workspace-new.bin"] {
            assert!(!captured.is_ignored(
                &WorkspaceRelativePath::parse(path).unwrap(),
                DocumentKind::File
            ));
        }

        let next = provider.capture(&root, &retained).unwrap();
        for path in ["global-new.bin", "workspace-new.bin"] {
            assert!(next.is_ignored(
                &WorkspaceRelativePath::parse(path).unwrap(),
                DocumentKind::File
            ));
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn settings_provider_rejects_malformed_file_ignore_settings() {
        let root = test_root("malformed-provider");
        fs::create_dir_all(&root).unwrap();
        let retained = Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap();
        let store = Arc::new(MemorySettingsStore::default());
        store.put("fileIgnoreSettings", json!({ "rules": 42 }));
        let settings = Arc::new(SettingsService::new(store, Arc::new(NoopEventSink)));
        let provider = SettingsWorkspaceIgnorePort::new(settings);

        assert!(matches!(
            provider.capture(&root, &retained),
            Err(WorkspaceIgnoreError)
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn settings_provider_fails_closed_for_invalid_workspace_ignore_files() {
        let root = test_root("invalid-workspace-provider");
        fs::create_dir_all(root.join(MARKRA_IGNORE_FILE_NAME)).unwrap();
        let retained = Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap();
        let settings = Arc::new(SettingsService::new(
            Arc::new(MemorySettingsStore::default()),
            Arc::new(NoopEventSink),
        ));
        let provider = SettingsWorkspaceIgnorePort::new(settings);

        assert!(matches!(
            provider.capture(&root, &retained),
            Err(WorkspaceIgnoreError)
        ));

        fs::remove_dir(root.join(MARKRA_IGNORE_FILE_NAME)).unwrap();
        fs::write(root.join(MARKRA_IGNORE_FILE_NAME), [0xff, 0xfe]).unwrap();
        assert!(matches!(
            provider.capture(&root, &retained),
            Err(WorkspaceIgnoreError)
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn settings_provider_fails_closed_for_invalid_or_oversized_rules() {
        let root = test_root("invalid-rules-provider");
        fs::create_dir_all(&root).unwrap();
        let retained = Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap();
        let store = Arc::new(MemorySettingsStore::default());
        store.put("fileIgnoreSettings", json!({ "rules": "[z-a]\n" }));
        let settings = Arc::new(SettingsService::new(store.clone(), Arc::new(NoopEventSink)));
        let provider = SettingsWorkspaceIgnorePort::new(settings);

        assert!(matches!(
            provider.capture(&root, &retained),
            Err(WorkspaceIgnoreError)
        ));

        store.put(
            "fileIgnoreSettings",
            json!({ "rules": "a".repeat(MAX_GLOBAL_IGNORE_RULE_UNITS + 1) }),
        );
        assert!(matches!(
            provider.capture(&root, &retained),
            Err(WorkspaceIgnoreError)
        ));

        store.put("fileIgnoreSettings", json!({ "rules": "" }));
        fs::write(
            root.join(MARKRA_IGNORE_FILE_NAME),
            vec![b'a'; MAX_MARKRA_IGNORE_BYTES as usize + 1],
        )
        .unwrap();
        assert!(matches!(
            provider.capture(&root, &retained),
            Err(WorkspaceIgnoreError)
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn retained_reader_rejects_same_inode_rewrite_with_restored_length_and_mtime() {
        use std::{thread, time::Duration};

        let root = test_root("same-inode-rewrite");
        fs::create_dir_all(&root).unwrap();
        let ignore_path = root.join(MARKRA_IGNORE_FILE_NAME);
        fs::write(&ignore_path, "first\n").unwrap();
        let original_modified = fs::metadata(&ignore_path).unwrap().modified().unwrap();
        let retained = Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap();

        let result = read_retained_workspace_rules_with_hook(&retained, || {
            thread::sleep(Duration::from_millis(2));
            fs::write(&ignore_path, "other\n").unwrap();
            fs::OpenOptions::new()
                .write(true)
                .open(&ignore_path)
                .unwrap()
                .set_modified(original_modified)
                .unwrap();
        });

        assert_eq!(result, Err(WorkspaceIgnoreError));
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn retained_reader_rejects_a_fifo_without_waiting_for_a_writer() {
        use std::{process::Command, sync::mpsc, thread, time::Duration};

        let root = test_root("fifo");
        fs::create_dir_all(&root).unwrap();
        let fifo = root.join(MARKRA_IGNORE_FILE_NAME);
        assert!(Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success());
        let retained = Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap();
        let (sender, receiver) = mpsc::channel();
        let worker = thread::spawn(move || {
            sender
                .send(read_retained_workspace_rules(&retained))
                .unwrap();
        });

        let prompt_result = receiver.recv_timeout(Duration::from_millis(250));
        if prompt_result.is_err() {
            drop(fs::OpenOptions::new().write(true).open(&fifo).unwrap());
        }
        worker.join().unwrap();

        assert_eq!(prompt_result, Ok(Err(WorkspaceIgnoreError)));
        fs::remove_dir_all(root).unwrap();
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
