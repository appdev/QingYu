use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use cap_std::fs::Dir;
use notify::event::{CreateKind, RemoveKind};
use notify::{Event, EventKind};
use qingyu_kernel::ignore_rules::MARKRA_IGNORE_FILE_NAME;
use tauri::{Emitter, Manager};

use crate::dejavu_sync::commands::DejavuSchedulerOwner;
use crate::markdown_files::MarkdownIgnoreRules;
use crate::protected_paths::path_contains_qingyu_control_directory;

mod directory;

use directory::DirectoryWatcher;

const MARKDOWN_FILE_CHANGED_EVENT: &str = "markra://file-changed";
const MARKDOWN_TREE_CHANGED_EVENT: &str = "markra://tree-changed";

struct ActiveMarkdownWatcher {
    ignore_rules: Arc<Mutex<MarkdownWatchIgnoreRules>>,
    subscriber_count: usize,
    watch_root: PathBuf,
    watcher: DirectoryWatcher,
}

struct MarkdownWatchIgnoreRules {
    root: PathBuf,
    retained_root: Dir,
    global_rules: Option<String>,
    current: Option<MarkdownIgnoreRules>,
}

impl MarkdownWatchIgnoreRules {
    fn try_new(root: &Path, global_rules: Option<&str>) -> Result<Self, String> {
        let retained_root = Dir::open_ambient_dir(root, cap_std::ambient_authority())
            .map_err(|_| "workspace ignore rules are unavailable".to_string())?;
        let current =
            MarkdownIgnoreRules::try_for_retained_root(root, &retained_root, global_rules)
                .map_err(|error| error.to_string())?;

        Ok(Self {
            root: root.to_path_buf(),
            retained_root,
            global_rules: global_rules.map(str::to_string),
            current: Some(current),
        })
    }

    fn current(&self) -> Result<&MarkdownIgnoreRules, String> {
        self.current
            .as_ref()
            .ok_or_else(|| "workspace ignore rules are unavailable".to_string())
    }

    fn is_control_file(&self, path: &Path) -> bool {
        path.parent() == Some(self.root.as_path())
            && path.file_name() == Some(std::ffi::OsStr::new(MARKRA_IGNORE_FILE_NAME))
    }

    fn reload_for_event(&mut self, event: &Event) -> Result<(), String> {
        if !event
            .paths
            .iter()
            .any(|event_path| self.is_control_file(event_path))
        {
            return self.current().map(|_| ());
        }

        match MarkdownIgnoreRules::try_for_retained_root(
            &self.root,
            &self.retained_root,
            self.global_rules.as_deref(),
        ) {
            Ok(next) => {
                self.current = Some(next);
                Ok(())
            }
            Err(error) => {
                self.current = None;
                Err(error.to_string())
            }
        }
    }

    fn replace(&mut self, root: &Path, global_rules: Option<&str>) -> Result<(), String> {
        match Self::try_new(root, global_rules) {
            Ok(next) => {
                *self = next;
                Ok(())
            }
            Err(error) => {
                self.current = None;
                Err(error)
            }
        }
    }
}

#[derive(Default)]
pub(crate) struct MarkdownFileWatcherState(Mutex<HashMap<PathBuf, ActiveMarkdownWatcher>>);

#[derive(Default)]
pub(crate) struct MarkdownTreeWatcherState(Mutex<HashMap<PathBuf, ActiveMarkdownWatcher>>);

#[derive(Clone, serde::Serialize)]
struct MarkdownFileChanged {
    path: String,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct MarkdownTreeChanged {
    path: String,
    root_path: String,
}

fn is_target_file_event(event: &Event, watched_path: &Path) -> bool {
    if !matches!(
        event.kind,
        EventKind::Any | EventKind::Create(_) | EventKind::Modify(_)
    ) {
        return false;
    }

    let Some(watched_file_name) = watched_path.file_name() else {
        return false;
    };

    event.paths.iter().any(|event_path| {
        event_path == watched_path
            || (event_path.parent() == watched_path.parent()
                && event_path.file_name() == Some(watched_file_name))
    })
}

fn markdown_ignore_root(watched_path: &Path, candidate_root: Option<&Path>) -> PathBuf {
    let watched_parent = watched_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    candidate_root
        .filter(|root| watched_path.starts_with(root))
        .map(Path::to_path_buf)
        .unwrap_or(watched_parent)
}

fn is_markdown_tree_path(_path: &Path) -> bool {
    true
}

fn should_suppress_markdown_watch(path: &Path) -> bool {
    path_contains_qingyu_control_directory(path)
}

fn markdown_event_path_is_directory(event: &Event, path: &Path) -> bool {
    matches!(
        event.kind,
        EventKind::Create(CreateKind::Folder) | EventKind::Remove(RemoveKind::Folder)
    ) || path.is_dir()
}

fn markdown_tree_event_path<'a>(
    event: &'a Event,
    root: &Path,
    ignore_rules: &MarkdownIgnoreRules,
) -> Option<&'a Path> {
    if !matches!(
        event.kind,
        EventKind::Any | EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    ) {
        return None;
    }

    event.paths.iter().map(PathBuf::as_path).find(|event_path| {
        let Ok(relative_path) = event_path.strip_prefix(root) else {
            return false;
        };

        if relative_path.as_os_str().is_empty() {
            return false;
        }

        // The control file stays hidden from the tree, but its event must trigger
        // a refresh so subsequent traversal uses the updated rules.
        ignore_rules.is_control_file(event_path)
            || (!ignore_rules.ignores(
                event_path,
                markdown_event_path_is_directory(event, event_path),
            ) && is_markdown_tree_path(event_path))
    })
}

fn remove_path_entry<T>(entries: &mut HashMap<PathBuf, T>, path: &Path) -> Option<T> {
    entries.remove(path)
}

fn release_active_watcher_subscription(subscriber_count: &mut usize) -> bool {
    if *subscriber_count > 1 {
        *subscriber_count -= 1;
        return false;
    }

    true
}

fn has_active_watcher_subscription(
    watcher_state: &Mutex<HashMap<PathBuf, ActiveMarkdownWatcher>>,
    path: &Path,
    ignore_root: &Path,
    global_ignore_rules: Option<&str>,
) -> Result<bool, String> {
    let mut active_watchers = watcher_state
        .lock()
        .map_err(|_| "markdown watcher state lock is poisoned".to_string())?;

    if let Some(watcher) = active_watchers.get_mut(path) {
        let mut ignore_rules = watcher
            .ignore_rules
            .lock()
            .map_err(|_| "markdown ignore rules lock is poisoned".to_string())?;
        // React may subscribe with new settings before the previous async unwatch
        // reaches Rust, so refresh a shared watcher's matcher during subscription.
        ignore_rules.replace(ignore_root, global_ignore_rules)?;
        // Linux reconciliation reads this matcher on its coordinator thread.
        drop(ignore_rules);
        watcher.watcher.reconcile()?;
        watcher.subscriber_count += 1;
        return Ok(true);
    }

    Ok(false)
}

fn remember_active_watcher(
    watcher_state: &Mutex<HashMap<PathBuf, ActiveMarkdownWatcher>>,
    path: PathBuf,
    watch_root: PathBuf,
    watcher: DirectoryWatcher,
    ignore_rules: Arc<Mutex<MarkdownWatchIgnoreRules>>,
) -> Result<(), String> {
    let mut active_watchers = watcher_state
        .lock()
        .map_err(|_| "markdown watcher state lock is poisoned".to_string())?;

    if let Some(existing_watcher) = active_watchers.get_mut(&path) {
        let mut existing_ignore_rules = existing_watcher
            .ignore_rules
            .lock()
            .map_err(|_| "markdown ignore rules lock is poisoned".to_string())?;
        let mut next_ignore_rules = ignore_rules
            .lock()
            .map_err(|_| "markdown ignore rules lock is poisoned".to_string())?;
        std::mem::swap(&mut *existing_ignore_rules, &mut *next_ignore_rules);
        // Release both matcher locks before waiting for Linux reconciliation.
        drop(existing_ignore_rules);
        drop(next_ignore_rules);
        existing_watcher.watcher.reconcile()?;
        existing_watcher.subscriber_count += 1;
        return Ok(());
    }

    active_watchers.insert(
        path.clone(),
        ActiveMarkdownWatcher {
            ignore_rules,
            subscriber_count: 1,
            watch_root,
            watcher,
        },
    );

    Ok(())
}

fn release_active_watcher(
    watcher_state: &Mutex<HashMap<PathBuf, ActiveMarkdownWatcher>>,
    path: &Path,
) -> Result<Option<PathBuf>, String> {
    let mut active_watchers = watcher_state
        .lock()
        .map_err(|_| "markdown watcher state lock is poisoned".to_string())?;

    if let Some(watcher) = active_watchers.get_mut(path) {
        if !release_active_watcher_subscription(&mut watcher.subscriber_count) {
            return Ok(None);
        }
    }

    Ok(remove_path_entry(&mut active_watchers, path).map(|watcher| watcher.watch_root))
}

#[tauri::command]
pub(crate) fn watch_markdown_file(
    app: tauri::AppHandle,
    watcher_state: tauri::State<'_, MarkdownFileWatcherState>,
    path: String,
    global_ignore_rules: Option<String>,
    ignore_root_path: Option<String>,
) -> Result<(), String> {
    let watched_path = PathBuf::from(&path);
    if should_suppress_markdown_watch(&watched_path) {
        return Ok(());
    }

    let watch_root = watched_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    // The OS subscription stays scoped to the file's parent, while ignore matching
    // may need workspace-relative paths. Reject unrelated roots before using them.
    let ignore_root =
        markdown_ignore_root(&watched_path, ignore_root_path.as_deref().map(Path::new));
    if has_active_watcher_subscription(
        &watcher_state.0,
        &watched_path,
        &ignore_root,
        global_ignore_rules.as_deref(),
    )? {
        return Ok(());
    }
    let emitted_path = watched_path.to_string_lossy().to_string();
    let emitted_root = watch_root.to_string_lossy().to_string();
    let callback_path = watched_path.clone();
    let callback_root = watch_root.clone();
    let ignore_rules = Arc::new(Mutex::new(MarkdownWatchIgnoreRules::try_new(
        &ignore_root,
        global_ignore_rules.as_deref(),
    )?));
    let callback_ignore_rules = Arc::clone(&ignore_rules);

    // Watch the parent tree so atomic saves and adjacent pasted assets are still visible.
    let watcher = DirectoryWatcher::new(
        &watch_root,
        Arc::clone(&ignore_rules),
        move |result: notify::Result<Event>| {
            let Ok(event) = result else {
                return;
            };

            let Ok(mut ignore_rules) = callback_ignore_rules.lock() else {
                return;
            };
            if ignore_rules.reload_for_event(&event).is_err() {
                return;
            }
            let Ok(current_ignore_rules) = ignore_rules.current() else {
                return;
            };

            if is_target_file_event(&event, &callback_path) {
                let _ = app.emit(
                    MARKDOWN_FILE_CHANGED_EVENT,
                    MarkdownFileChanged {
                        path: emitted_path.clone(),
                    },
                );
            }

            if let Some(event_path) =
                markdown_tree_event_path(&event, &callback_root, current_ignore_rules)
            {
                let _ = app.emit(
                    MARKDOWN_TREE_CHANGED_EVENT,
                    MarkdownTreeChanged {
                        path: event_path.to_string_lossy().to_string(),
                        root_path: emitted_root.clone(),
                    },
                );
            }
        },
    )?;

    remember_active_watcher(
        &watcher_state.0,
        watched_path,
        watch_root,
        watcher,
        ignore_rules,
    )
}

#[tauri::command]
pub(crate) fn unwatch_markdown_file(
    watcher_state: tauri::State<'_, MarkdownFileWatcherState>,
    path: String,
) -> Result<(), String> {
    let watched_path = PathBuf::from(path);
    release_active_watcher(&watcher_state.0, &watched_path).map(|_| ())
}

#[tauri::command]
pub(crate) fn watch_markdown_tree(
    app: tauri::AppHandle,
    watcher_state: tauri::State<'_, MarkdownTreeWatcherState>,
    scheduler_owner: tauri::State<'_, DejavuSchedulerOwner>,
    root_path: String,
    global_ignore_rules: Option<String>,
) -> Result<(), String> {
    let source_path = PathBuf::from(&root_path);
    if should_suppress_markdown_watch(&source_path) {
        return Ok(());
    }

    let watch_root = if source_path.is_dir() {
        source_path.clone()
    } else {
        source_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    };
    if has_active_watcher_subscription(
        &watcher_state.0,
        &source_path,
        &watch_root,
        global_ignore_rules.as_deref(),
    )? {
        // A new React subscription can arrive before the stale subscription's
        // asynchronous unwatch. The newest subscription owns the active root.
        scheduler_owner.activate_root(&watch_root);
        return Ok(());
    }
    let emitted_root = watch_root.to_string_lossy().to_string();
    let callback_root = watch_root.clone();
    let ignore_rules = Arc::new(Mutex::new(MarkdownWatchIgnoreRules::try_new(
        &callback_root,
        global_ignore_rules.as_deref(),
    )?));
    let callback_ignore_rules = Arc::clone(&ignore_rules);

    let callback_app = app.clone();
    let watcher = DirectoryWatcher::new(
        &watch_root,
        Arc::clone(&ignore_rules),
        move |result: notify::Result<Event>| {
            let Ok(event) = result else {
                return;
            };

            let Ok(mut ignore_rules) = callback_ignore_rules.lock() else {
                return;
            };
            if ignore_rules.reload_for_event(&event).is_err() {
                return;
            }
            let Ok(current_ignore_rules) = ignore_rules.current() else {
                return;
            };
            if let Some(event_path) =
                markdown_tree_event_path(&event, &callback_root, current_ignore_rules)
            {
                scheduler_owner_for(&callback_app).record_file_change(&callback_root, event_path);
                let _ = callback_app.emit(
                    MARKDOWN_TREE_CHANGED_EVENT,
                    MarkdownTreeChanged {
                        path: event_path.to_string_lossy().to_string(),
                        root_path: emitted_root.clone(),
                    },
                );
            }
        },
    )?;

    remember_active_watcher(
        &watcher_state.0,
        source_path,
        watch_root.clone(),
        watcher,
        ignore_rules,
    )?;
    scheduler_owner.activate_root(&watch_root);
    Ok(())
}

#[tauri::command]
pub(crate) fn unwatch_markdown_tree(
    watcher_state: tauri::State<'_, MarkdownTreeWatcherState>,
    scheduler_owner: tauri::State<'_, DejavuSchedulerOwner>,
    root_path: String,
) -> Result<(), String> {
    let watched_path = PathBuf::from(root_path);
    if let Some(root) = release_active_watcher(&watcher_state.0, &watched_path)? {
        scheduler_owner.deactivate_root(&root);
    }
    Ok(())
}

fn scheduler_owner_for<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> tauri::State<'_, DejavuSchedulerOwner> {
    app.state::<DejavuSchedulerOwner>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{CreateKind, DataChange, ModifyKind, RemoveKind};
    use std::collections::HashMap;

    use crate::protected_paths::{LEGACY_SYNC_DIR, QINGYU_CONTROL_DIR};

    fn test_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "markra-watcher-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after epoch")
                .as_nanos()
        ))
    }

    fn strict_test_rules(root: &Path, global_rules: Option<&str>) -> MarkdownIgnoreRules {
        let retained_root = Dir::open_ambient_dir(root, cap_std::ambient_authority())
            .expect("test root should open");
        MarkdownIgnoreRules::try_for_retained_root(root, &retained_root, global_rules)
            .expect("test ignore rules should be valid")
    }

    fn test_markdown_tree_event_path<'a>(event: &'a Event, root: &Path) -> Option<&'a Path> {
        let ignore_rules = strict_test_rules(root, None);
        markdown_tree_event_path(event, root, &ignore_rules)
    }

    #[test]
    fn uses_workspace_root_for_nested_watched_files() {
        let watched_path = Path::new("/mock-workspace/docs/note.md");

        assert_eq!(
            markdown_ignore_root(watched_path, Some(Path::new("/mock-workspace"))),
            PathBuf::from("/mock-workspace")
        );
    }

    #[test]
    fn rejects_ignore_roots_that_do_not_contain_the_watched_file() {
        let watched_path = Path::new("/mock-workspace/docs/note.md");

        assert_eq!(
            markdown_ignore_root(watched_path, Some(Path::new("/other-workspace"))),
            PathBuf::from("/mock-workspace/docs")
        );
    }

    #[test]
    fn parent_ignore_roots_still_protect_control_directory_file_watches() {
        let root = test_root("protected-file-watch");
        for control_directory in [QINGYU_CONTROL_DIR, LEGACY_SYNC_DIR] {
            let watched_path = root.join(control_directory).join("note.md");
            let ignore_root = markdown_ignore_root(&watched_path, None);
            std::fs::create_dir_all(&ignore_root).expect("ignore root should be created");
            let rules = strict_test_rules(&ignore_root, Some("!note.md\n"));

            assert_eq!(ignore_root, watched_path.parent().unwrap());
            assert!(should_suppress_markdown_watch(&watched_path));
            assert!(rules.ignores(&watched_path, false));
        }
        std::fs::remove_dir_all(root).expect("test root should be removed");
    }

    #[test]
    fn matches_target_file_modifications_in_the_watched_directory() {
        let watched_path = PathBuf::from("/mock-files/readme.md");
        let event = Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Content)))
            .add_path(PathBuf::from("/mock-files/readme.md"));

        assert!(is_target_file_event(&event, &watched_path));
    }

    #[test]
    fn matches_target_file_recreation_from_atomic_saves() {
        let watched_path = PathBuf::from("/mock-files/readme.md");
        let event = Event::new(EventKind::Create(CreateKind::File))
            .add_path(PathBuf::from("/mock-files/readme.md"));

        assert!(is_target_file_event(&event, &watched_path));
    }

    #[test]
    fn ignores_other_files_in_the_same_directory() {
        let watched_path = PathBuf::from("/mock-files/readme.md");
        let event = Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Content)))
            .add_path(PathBuf::from("/mock-files/other.md"));

        assert!(!is_target_file_event(&event, &watched_path));
    }

    #[test]
    fn matches_nested_markdown_tree_asset_creations() {
        let root = test_root("nested-asset");
        std::fs::create_dir_all(&root).expect("test root should be created");
        let event = Event::new(EventKind::Create(CreateKind::File))
            .add_path(root.join("assets/pasted-image.png"));

        assert!(test_markdown_tree_event_path(&event, &root).is_some());
        std::fs::remove_dir_all(root).expect("test root should be removed");
    }

    #[test]
    fn ignores_dependency_folder_tree_events() {
        let root = test_root("dependency-event");
        std::fs::create_dir_all(&root).expect("test root should be created");
        let event = Event::new(EventKind::Create(CreateKind::File))
            .add_path(root.join("node_modules/pkg/readme.md"));

        assert!(test_markdown_tree_event_path(&event, &root).is_none());
        std::fs::remove_dir_all(root).expect("test root should be removed");
    }

    #[test]
    fn ignores_tool_metadata_folder_tree_events() {
        let root = test_root("metadata-event");
        std::fs::create_dir_all(&root).expect("test root should be created");
        for path in [
            root.join(".obsidian/plugins/mock-plugin/data.json"),
            root.join(".qingyu/config.json"),
            root.join(".qingyu/sync/status.json"),
            root.join(".markra-sync/manifest.json"),
        ] {
            let event = Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Content)))
                .add_path(path.clone());

            assert!(
                test_markdown_tree_event_path(&event, &root).is_none(),
                "tool metadata event should remain hidden: {}",
                path.display()
            );
        }
        std::fs::remove_dir_all(root).expect("test root should be removed");
    }

    #[test]
    fn ignores_control_directory_tree_events_when_rules_negate_them() {
        let root = std::env::temp_dir().join(format!(
            "markra-control-directory-watcher-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("test root should be created");
        std::fs::write(
            root.join(".markraignore"),
            "!.qingyu/\n!.qingyu/config.json\n!.markra-sync/\n",
        )
        .expect("workspace rules should be written");
        let rules = strict_test_rules(
            &root,
            Some("!.qingyu/\n!.qingyu/sync/status.json\n!.markra-sync/\n"),
        );

        for path in [
            root.join(".qingyu/config.json"),
            root.join(".qingyu/sync/status.json"),
            root.join(".markra-sync/manifest.json"),
        ] {
            let event = Event::new(EventKind::Create(CreateKind::File)).add_path(path);
            assert!(markdown_tree_event_path(&event, &root, &rules).is_none());
        }

        std::fs::remove_dir_all(root).expect("test root should be removed");
    }

    #[test]
    fn ignores_removed_root_control_directory_events_without_disk_metadata() {
        let root = test_root("removed-control-event");
        std::fs::create_dir_all(&root).expect("test root should be created");

        for control_directory in [QINGYU_CONTROL_DIR, LEGACY_SYNC_DIR] {
            for kind in [EventKind::Remove(RemoveKind::Folder), EventKind::Any] {
                let event = Event::new(kind).add_path(root.join(control_directory));

                assert!(test_markdown_tree_event_path(&event, &root).is_none());
            }
        }
        std::fs::remove_dir_all(root).expect("test root should be removed");
    }

    #[test]
    fn classifies_folder_events_without_relying_on_disk_metadata() {
        let root = test_root("folder-event");
        std::fs::create_dir_all(&root).expect("test root should be created");
        let rules = strict_test_rules(&root, Some("generated/\n"));

        for kind in [
            EventKind::Create(CreateKind::Folder),
            EventKind::Remove(RemoveKind::Folder),
        ] {
            let event = Event::new(kind).add_path(root.join("generated"));

            assert!(markdown_tree_event_path(&event, &root, &rules).is_none());
        }
        std::fs::remove_dir_all(root).expect("test root should be removed");
    }

    #[test]
    fn uses_root_markraignore_for_tree_events() {
        let root = std::env::temp_dir().join(format!(
            "markra-ignore-watcher-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after epoch")
                .as_nanos()
        ));
        let generated = root.join("generated");

        std::fs::create_dir_all(&generated).expect("generated folder should be created");
        let mut ignore_rules = MarkdownWatchIgnoreRules::try_new(&root, None)
            .expect("initial watcher rules should load strictly");
        std::fs::write(root.join(".markraignore"), "generated/\n")
            .expect("ignore rules should be created");
        let control_event =
            Event::new(EventKind::Create(CreateKind::File)).add_path(root.join(".markraignore"));
        ignore_rules
            .reload_for_event(&control_event)
            .expect("updated rules should load strictly");
        let event =
            Event::new(EventKind::Create(CreateKind::File)).add_path(generated.join("hidden.md"));

        assert!(markdown_tree_event_path(
            &event,
            &root,
            ignore_rules
                .current()
                .expect("rules should remain available")
        )
        .is_none());

        std::fs::remove_dir_all(root).expect("test tree should be removed");
    }

    #[test]
    fn preserves_global_rules_when_root_markraignore_reloads() {
        let root = std::env::temp_dir().join(format!(
            "markra-global-ignore-watcher-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after epoch")
                .as_nanos()
        ));
        let generated = root.join("generated");

        std::fs::create_dir_all(&generated).expect("generated folder should be created");
        let mut ignore_rules = MarkdownWatchIgnoreRules::try_new(&root, Some("generated/\n"))
            .expect("initial watcher rules should load strictly");
        std::fs::write(root.join(".markraignore"), "").expect("workspace rules should be created");
        let control_event = Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Content)))
            .add_path(root.join(".markraignore"));
        ignore_rules
            .reload_for_event(&control_event)
            .expect("updated rules should load strictly");
        let event =
            Event::new(EventKind::Create(CreateKind::File)).add_path(generated.join("hidden.md"));

        assert!(markdown_tree_event_path(
            &event,
            &root,
            ignore_rules
                .current()
                .expect("rules should remain available")
        )
        .is_none());

        std::fs::remove_dir_all(root).expect("test tree should be removed");
    }

    #[test]
    fn invalid_root_markraignore_disables_event_matching_until_strict_reload_succeeds() {
        let root = std::env::temp_dir().join(format!(
            "markra-invalid-ignore-watcher-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after epoch")
                .as_nanos()
        ));
        let generated = root.join("generated");

        std::fs::create_dir_all(&generated).expect("generated folder should be created");
        std::fs::write(root.join(".markraignore"), "generated/\n")
            .expect("initial ignore rules should be valid");
        let mut ignore_rules = MarkdownWatchIgnoreRules::try_new(&root, None)
            .expect("initial watcher rules should load strictly");
        let control_event = Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Content)))
            .add_path(root.join(".markraignore"));
        let generated_event =
            Event::new(EventKind::Create(CreateKind::File)).add_path(generated.join("hidden.md"));

        std::fs::write(root.join(".markraignore"), [0xff, 0xfe])
            .expect("invalid UTF-8 fixture should be written");
        assert_eq!(
            ignore_rules.reload_for_event(&control_event),
            Err("workspace ignore rules are unavailable".to_string())
        );
        assert!(ignore_rules.current().is_err());
        assert!(ignore_rules.reload_for_event(&generated_event).is_err());

        std::fs::write(root.join(".markraignore"), "generated/\n")
            .expect("valid ignore rules should be restored");
        ignore_rules
            .reload_for_event(&control_event)
            .expect("a later valid control event should restore matching");
        assert!(ignore_rules
            .current()
            .expect("rules should be available after recovery")
            .ignores(&generated.join("hidden.md"), false));

        std::fs::remove_dir_all(root).expect("test tree should be removed");
    }

    #[test]
    fn emits_root_markraignore_tree_events() {
        let root = test_root("control-event");
        std::fs::create_dir_all(&root).expect("test root should be created");
        let event = Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Content)))
            .add_path(root.join(".markraignore"));

        assert_eq!(
            test_markdown_tree_event_path(&event, &root),
            Some(root.join(".markraignore").as_path())
        );
        std::fs::remove_dir_all(root).expect("test root should be removed");
    }

    #[test]
    fn removing_an_active_watcher_keeps_other_paths() {
        let mut entries = HashMap::from([
            (PathBuf::from("/mock-files/first.md"), "first"),
            (PathBuf::from("/mock-files/second.md"), "second"),
        ]);

        remove_path_entry(&mut entries, Path::new("/mock-files/first.md"));

        assert_eq!(entries.len(), 1);
        assert!(entries.contains_key(Path::new("/mock-files/second.md")));
    }

    #[test]
    fn shared_active_watchers_release_only_after_last_subscription() {
        let mut subscriber_count = 2;

        assert!(!release_active_watcher_subscription(&mut subscriber_count));
        assert_eq!(subscriber_count, 1);
        assert!(release_active_watcher_subscription(&mut subscriber_count));
    }
}
