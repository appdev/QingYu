use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[cfg(any(target_os = "linux", test))]
use cap_fs_ext::DirExt;
#[cfg(any(target_os = "linux", test))]
use notify::event::{CreateKind, ModifyKind, RemoveKind};
#[cfg(any(target_os = "linux", test))]
use notify::EventKind;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
#[cfg(any(target_os = "linux", test))]
use std::collections::HashSet;

use super::MarkdownWatchIgnoreRules;
#[cfg(any(target_os = "linux", test))]
use crate::markdown_files::MarkdownIgnoreRules;
#[cfg(any(target_os = "linux", test))]
use crate::protected_paths::path_contains_qingyu_control_directory;
use crate::storage_capability::{
    directory_identity, open_canonical_directory_nofollow, DirectoryIdentity,
};

struct DirectoryWatchRoot {
    path: PathBuf,
    retained: cap_std::fs::Dir,
    identity: DirectoryIdentity,
}

impl DirectoryWatchRoot {
    fn capture(path: &Path) -> Result<Self, String> {
        let retained = open_canonical_directory_nofollow(path)
            .map_err(|_| "workspace root changed".to_string())?;
        let identity =
            directory_identity(&retained).map_err(|_| "workspace root changed".to_string())?;
        let root = Self {
            path: path.to_path_buf(),
            retained,
            identity,
        };
        root.verify_current()?;
        Ok(root)
    }

    fn verify_current(&self) -> Result<(), String> {
        if directory_identity(&self.retained).map_err(|_| "workspace root changed".to_string())?
            != self.identity
        {
            return Err("workspace root changed".to_string());
        }
        let current = open_canonical_directory_nofollow(&self.path)
            .and_then(|directory| directory_identity(&directory))
            .map_err(|_| "workspace root changed".to_string())?;
        if current != self.identity {
            return Err("workspace root changed".to_string());
        }
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
enum DirectoryWatchStrategy {
    #[cfg(not(target_os = "linux"))]
    RecursiveRoot,
    #[cfg(target_os = "linux")]
    VisibleDirectories,
}

#[cfg(target_os = "linux")]
fn directory_watch_strategy() -> DirectoryWatchStrategy {
    DirectoryWatchStrategy::VisibleDirectories
}

#[cfg(not(target_os = "linux"))]
fn directory_watch_strategy() -> DirectoryWatchStrategy {
    DirectoryWatchStrategy::RecursiveRoot
}

#[cfg(any(target_os = "linux", test))]
#[derive(Debug, PartialEq, Eq)]
struct DirectoryWatchDiff {
    add: HashSet<PathBuf>,
    remove: HashSet<PathBuf>,
}

#[cfg(any(target_os = "linux", test))]
fn visible_watch_directories(
    watch_root: &DirectoryWatchRoot,
    watch_rules: &mut MarkdownWatchIgnoreRules,
) -> Result<HashSet<PathBuf>, String> {
    fn collect(
        root: &Path,
        directory: &cap_std::fs::Dir,
        relative_directory: &Path,
        ambient_directory: &Path,
        ignore_rules: &MarkdownIgnoreRules,
        directories: &mut HashSet<PathBuf>,
    ) -> Result<(), String> {
        if path_contains_qingyu_control_directory(ambient_directory) {
            return Ok(());
        }

        directories.insert(ambient_directory.to_path_buf());
        for entry in directory.entries().map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let name = entry.file_name();
            let relative_path = relative_directory.join(&name);
            let path = root.join(&relative_path);
            if entry
                .file_type()
                .map_err(|error| error.to_string())?
                .is_dir()
                && !ignore_rules.ignores(&path, true)
            {
                let child = directory
                    .open_dir_nofollow(&name)
                    .map_err(|_| "workspace directory changed".to_string())?;
                collect(
                    root,
                    &child,
                    &relative_path,
                    &path,
                    ignore_rules,
                    directories,
                )?;
            }
        }
        Ok(())
    }

    watch_root.verify_current()?;
    watch_rules.verify_root()?;
    let retained_root = watch_root
        .retained
        .try_clone()
        .map_err(|_| "workspace root changed".to_string())?;
    let mut directories = HashSet::new();
    collect(
        &watch_root.path,
        &retained_root,
        Path::new(""),
        &watch_root.path,
        watch_rules.current()?,
        &mut directories,
    )?;
    watch_rules.verify_root()?;
    watch_root.verify_current()?;
    Ok(directories)
}

#[cfg(any(target_os = "linux", test))]
fn directory_watch_diff(
    current: &HashSet<PathBuf>,
    desired: &HashSet<PathBuf>,
) -> DirectoryWatchDiff {
    DirectoryWatchDiff {
        add: desired.difference(current).cloned().collect(),
        remove: current.difference(desired).cloned().collect(),
    }
}

#[cfg(any(target_os = "linux", test))]
fn event_requires_reconciliation(event: &Event, ignore_rules: &MarkdownWatchIgnoreRules) -> bool {
    event.need_rescan()
        || matches!(
            event.kind,
            EventKind::Any
                | EventKind::Create(CreateKind::Any | CreateKind::Folder)
                | EventKind::Modify(ModifyKind::Name(_))
                | EventKind::Remove(RemoveKind::Any | RemoveKind::Folder)
        )
        || event
            .paths
            .iter()
            .any(|path| ignore_rules.is_control_file(path))
}

pub(super) struct DirectoryWatcher {
    #[cfg(target_os = "linux")]
    coordinator: LinuxDirectoryWatcher,
    #[cfg(not(target_os = "linux"))]
    _watcher: RecommendedWatcher,
    ignore_rules: Arc<Mutex<MarkdownWatchIgnoreRules>>,
    watch_root: Arc<DirectoryWatchRoot>,
}

impl DirectoryWatcher {
    pub(super) fn new<F>(
        root: &Path,
        ignore_rules: Arc<Mutex<MarkdownWatchIgnoreRules>>,
        handler: F,
    ) -> Result<Self, String>
    where
        F: FnMut(notify::Result<Event>) + Send + 'static,
    {
        let watch_root = Arc::new(DirectoryWatchRoot::capture(root)?);
        #[cfg(target_os = "linux")]
        {
            debug_assert_eq!(
                directory_watch_strategy(),
                DirectoryWatchStrategy::VisibleDirectories
            );
            return LinuxDirectoryWatcher::new(
                Arc::clone(&watch_root),
                Arc::clone(&ignore_rules),
                handler,
            )
            .map(|coordinator| Self {
                coordinator,
                ignore_rules,
                watch_root,
            });
        }

        #[cfg(not(target_os = "linux"))]
        {
            debug_assert_eq!(
                directory_watch_strategy(),
                DirectoryWatchStrategy::RecursiveRoot
            );
            let callback_rules = Arc::clone(&ignore_rules);
            let callback_root = Arc::clone(&watch_root);
            let mut handler = handler;
            let mut watcher = notify::recommended_watcher(move |result| match result {
                Err(error) => {
                    if let Ok(mut rules) = callback_rules.lock() {
                        rules.invalidate();
                    }
                    handler(Err(error));
                }
                Ok(event) => {
                    let preparation = callback_root.verify_current().and_then(|()| {
                        callback_rules
                            .lock()
                            .map_err(|_| "markdown ignore rules lock is poisoned".to_string())
                            .and_then(|mut rules| {
                                if let Some(candidate) = rules.stage_for_event(&event)? {
                                    rules.finish_reconcile(candidate, Ok(()))?;
                                }
                                rules.current().map(|_| ())
                            })
                    });
                    match preparation {
                        Ok(()) => handler(Ok(event)),
                        Err(error) => {
                            if let Ok(mut rules) = callback_rules.lock() {
                                rules.invalidate();
                            }
                            handler(Err(notify::Error::generic(&error)));
                        }
                    }
                }
            })
            .map_err(|error| error.to_string())?;
            watcher
                .watch(root, RecursiveMode::Recursive)
                .map_err(|error| error.to_string())?;
            Ok(Self {
                _watcher: watcher,
                ignore_rules,
                watch_root,
            })
        }
    }

    pub(super) fn replace_rules(
        &self,
        root: &Path,
        global_rules: Option<&str>,
    ) -> Result<(), String> {
        let candidate = match MarkdownWatchIgnoreRules::try_new(root, global_rules) {
            Ok(candidate) => candidate,
            Err(error) => {
                if let Ok(mut rules) = self.ignore_rules.lock() {
                    rules.invalidate();
                }
                return Err(error);
            }
        };
        #[cfg(target_os = "linux")]
        {
            return self.coordinator.replace_rules(candidate);
        }

        #[cfg(not(target_os = "linux"))]
        {
            let mut rules = self
                .ignore_rules
                .lock()
                .map_err(|_| "markdown ignore rules lock is poisoned".to_string())?;
            let result = self.watch_root.verify_current();
            rules.finish_reconcile(candidate, result)
        }
    }
}

#[cfg(target_os = "linux")]
struct LinuxDirectoryWatcher {
    coordinator: Option<std::thread::JoinHandle<()>>,
    sender: std::sync::mpsc::Sender<CoordinatorMessage>,
}

#[cfg(target_os = "linux")]
enum CoordinatorMessage {
    BackendEvent(notify::Result<Event>),
    ReplaceRules(
        MarkdownWatchIgnoreRules,
        std::sync::mpsc::SyncSender<Result<(), String>>,
    ),
    Shutdown,
}

#[cfg(target_os = "linux")]
impl LinuxDirectoryWatcher {
    fn new<F>(
        watch_root: Arc<DirectoryWatchRoot>,
        ignore_rules: Arc<Mutex<MarkdownWatchIgnoreRules>>,
        handler: F,
    ) -> Result<Self, String>
    where
        F: FnMut(notify::Result<Event>) + Send + 'static,
    {
        let (sender, receiver) = std::sync::mpsc::channel();
        let event_sender = sender.clone();
        let mut watcher = notify::recommended_watcher(move |result| {
            let _ = event_sender.send(CoordinatorMessage::BackendEvent(result));
        })
        .map_err(|error| error.to_string())?;
        let mut watched_directories = {
            let mut rules = ignore_rules
                .lock()
                .map_err(|_| "markdown ignore rules lock is poisoned".to_string())?;
            visible_watch_directories(&watch_root, &mut rules)?
        };
        let mut initial_directories = watched_directories.iter().collect::<Vec<_>>();
        initial_directories.sort();
        for directory in initial_directories {
            watcher
                .watch(directory, RecursiveMode::NonRecursive)
                .map_err(|error| error.to_string())?;
        }

        let coordinator_rules = Arc::clone(&ignore_rules);
        let coordinator_root = Arc::clone(&watch_root);
        let coordinator = std::thread::Builder::new()
            .name("markra-directory-watcher".to_string())
            .spawn(move || {
                run_linux_coordinator(
                    watcher,
                    &mut watched_directories,
                    &coordinator_root,
                    &coordinator_rules,
                    handler,
                    receiver,
                );
            })
            .map_err(|error| error.to_string())?;

        Ok(Self {
            coordinator: Some(coordinator),
            sender,
        })
    }

    fn replace_rules(&self, candidate: MarkdownWatchIgnoreRules) -> Result<(), String> {
        let (result_sender, result_receiver) = std::sync::mpsc::sync_channel(1);
        self.sender
            .send(CoordinatorMessage::ReplaceRules(candidate, result_sender))
            .map_err(|_| "markdown directory watcher has stopped".to_string())?;
        result_receiver
            .recv()
            .map_err(|_| "markdown directory watcher has stopped".to_string())?
    }
}

#[cfg(target_os = "linux")]
impl Drop for LinuxDirectoryWatcher {
    fn drop(&mut self) {
        let _ = self.sender.send(CoordinatorMessage::Shutdown);
        if let Some(coordinator) = self.coordinator.take() {
            let _ = coordinator.join();
        }
    }
}

#[cfg(target_os = "linux")]
fn run_linux_coordinator<F>(
    mut watcher: RecommendedWatcher,
    watched_directories: &mut HashSet<PathBuf>,
    watch_root: &DirectoryWatchRoot,
    ignore_rules: &Arc<Mutex<MarkdownWatchIgnoreRules>>,
    mut handler: F,
    receiver: std::sync::mpsc::Receiver<CoordinatorMessage>,
) where
    F: FnMut(notify::Result<Event>),
{
    while let Ok(message) = receiver.recv() {
        match message {
            CoordinatorMessage::BackendEvent(result) => {
                let event = match result {
                    Ok(event) => event,
                    Err(error) => {
                        if let Ok(mut rules) = ignore_rules.lock() {
                            rules.invalidate();
                        }
                        handler(Err(error));
                        continue;
                    }
                };
                let preparation = (|| -> Result<(), String> {
                    watch_root.verify_current()?;
                    let should_reconcile = ignore_rules
                        .lock()
                        .map(|rules| event_requires_reconciliation(&event, &rules))
                        .unwrap_or(true);
                    let candidate = ignore_rules
                        .lock()
                        .map_err(|_| "markdown ignore rules lock is poisoned".to_string())?
                        .stage_for_event(&event)?;
                    if let Some(mut candidate) = candidate {
                        let result = reconcile_linux_directories(
                            &mut watcher,
                            watched_directories,
                            watch_root,
                            &mut candidate,
                        );
                        return ignore_rules
                            .lock()
                            .map_err(|_| "markdown ignore rules lock is poisoned".to_string())?
                            .finish_reconcile(candidate, result);
                    }
                    let mut rules = ignore_rules
                        .lock()
                        .map_err(|_| "markdown ignore rules lock is poisoned".to_string())?;
                    if should_reconcile {
                        if let Err(error) = reconcile_linux_directories(
                            &mut watcher,
                            watched_directories,
                            watch_root,
                            &mut rules,
                        ) {
                            rules.invalidate();
                            return Err(error);
                        }
                    }
                    rules.current().map(|_| ())
                })();
                match preparation {
                    Ok(()) => handler(Ok(event)),
                    Err(error) => {
                        if let Ok(mut rules) = ignore_rules.lock() {
                            rules.invalidate();
                        }
                        handler(Err(notify::Error::generic(&error)));
                    }
                }
            }
            CoordinatorMessage::ReplaceRules(mut candidate, result_sender) => {
                let result = reconcile_linux_directories(
                    &mut watcher,
                    watched_directories,
                    watch_root,
                    &mut candidate,
                );
                let result = ignore_rules
                    .lock()
                    .map_err(|_| "markdown ignore rules lock is poisoned".to_string())
                    .and_then(|mut rules| rules.finish_reconcile(candidate, result));
                let _ = result_sender.send(result);
            }
            CoordinatorMessage::Shutdown => break,
        }
    }
}

#[cfg(target_os = "linux")]
fn reconcile_linux_directories(
    watcher: &mut RecommendedWatcher,
    watched_directories: &mut HashSet<PathBuf>,
    watch_root: &DirectoryWatchRoot,
    ignore_rules: &mut MarkdownWatchIgnoreRules,
) -> Result<(), String> {
    let desired_directories = visible_watch_directories(watch_root, ignore_rules)?;
    let diff = directory_watch_diff(watched_directories, &desired_directories);
    let mut additions = diff.add.into_iter().collect::<Vec<_>>();
    additions.sort();
    let mut added: Vec<PathBuf> = Vec::new();

    // Add the full desired set before pruning stale watches. A failed addition
    // rolls back only this attempt, preserving the last known-good coverage.
    // notify invokes callbacks on its backend thread. All watch mutations stay on
    // this coordinator thread to avoid re-entering inotify and deadlocking it.
    for directory in additions {
        if let Err(error) = watcher.watch(&directory, RecursiveMode::NonRecursive) {
            for added_directory in added.into_iter().rev() {
                if watcher.unwatch(&added_directory).is_err() {
                    watched_directories.insert(added_directory);
                }
            }
            return Err(error.to_string());
        }
        added.push(directory);
    }
    watched_directories.extend(added);

    let mut removals = diff.remove.into_iter().collect::<Vec<_>>();
    removals.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    let mut first_error = None;
    for directory in removals {
        if !directory.exists() {
            let _ = watcher.unwatch(&directory);
            watched_directories.remove(&directory);
            continue;
        }

        match watcher.unwatch(&directory) {
            Ok(()) => {
                watched_directories.remove(&directory);
            }
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(error.to_string());
                }
            }
        }
    }

    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{CreateKind, DataChange, ModifyKind};
    use notify::{Event, EventKind};
    use std::collections::HashSet;
    use std::fs;
    use std::path::PathBuf;

    use crate::protected_paths::{LEGACY_SYNC_DIR, QINGYU_CONTROL_DIR};

    fn test_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "markra-directory-watcher-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after epoch")
                .as_nanos()
        ))
    }

    #[test]
    fn collects_only_visible_directories() {
        let root = test_root("visible");
        fs::create_dir_all(root.join("docs/generated"))
            .expect("generated directory should be created");
        fs::create_dir_all(root.join("node_modules/pkg"))
            .expect("dependency directory should be created");
        fs::create_dir_all(root.join(".qingyu/sync"))
            .expect("QingYu sync directory should be created");
        fs::create_dir_all(root.join(".markra-sync/objects"))
            .expect("legacy sync directory should be created");
        fs::write(root.join(".markraignore"), "!.qingyu/\n!.markra-sync/\n")
            .expect("workspace rules should be written");
        let mut rules = MarkdownWatchIgnoreRules::try_new(
            &root,
            Some("docs/generated/\n!.qingyu/\n!.markra-sync/\n"),
        )
        .expect("watcher rules should load");

        let watch_root = DirectoryWatchRoot::capture(&root).expect("watch root should capture");
        let directories = visible_watch_directories(&watch_root, &mut rules)
            .expect("visible directories should be collected");

        assert!(directories.contains(&root));
        assert!(directories.contains(&root.join("docs")));
        assert!(!directories.contains(&root.join("docs/generated")));
        assert!(!directories.contains(&root.join("node_modules")));
        assert!(!directories.contains(&root.join(".qingyu")));
        assert!(!directories.contains(&root.join(".qingyu/sync")));
        assert!(!directories.contains(&root.join(".markra-sync")));

        fs::remove_dir_all(root).expect("test root should be removed");
    }

    #[test]
    fn does_not_register_a_protected_control_directory_as_the_watch_root() {
        let parent = test_root("protected-root");

        for control_directory in [QINGYU_CONTROL_DIR, LEGACY_SYNC_DIR] {
            let root = parent.join(control_directory);
            fs::create_dir_all(root.join("nested"))
                .expect("protected watch root should be created");
            let mut rules =
                MarkdownWatchIgnoreRules::try_new(&root, None).expect("watcher rules should load");

            let watch_root = DirectoryWatchRoot::capture(&root).expect("watch root should capture");
            let directories = visible_watch_directories(&watch_root, &mut rules)
                .expect("visible directories should be collected");

            assert!(directories.is_empty());
        }

        fs::remove_dir_all(parent).expect("test parent should be removed");
    }

    #[test]
    fn calculates_directory_registration_changes() {
        let root = PathBuf::from("/mock-workspace");
        let current = HashSet::from([root.clone(), root.join("docs"), root.join("drafts")]);
        let desired = HashSet::from([root.clone(), root.join("docs"), root.join("notes")]);

        let diff = directory_watch_diff(&current, &desired);

        assert_eq!(diff.add, HashSet::from([root.join("notes")]));
        assert_eq!(diff.remove, HashSet::from([root.join("drafts")]));
    }

    #[test]
    fn recalculates_registrations_after_rules_change() {
        let root = test_root("rule-change");
        fs::create_dir_all(root.join("drafts")).expect("drafts directory should be created");
        fs::create_dir_all(root.join("notes")).expect("notes directory should be created");
        let mut initial_rules = MarkdownWatchIgnoreRules::try_new(&root, Some("drafts/\n"))
            .expect("initial watcher rules should load");
        let mut next_rules = MarkdownWatchIgnoreRules::try_new(&root, Some("notes/\n"))
            .expect("next watcher rules should load");
        let watch_root = DirectoryWatchRoot::capture(&root).expect("watch root should capture");
        let current = visible_watch_directories(&watch_root, &mut initial_rules)
            .expect("initial directories should be collected");
        let desired = visible_watch_directories(&watch_root, &mut next_rules)
            .expect("next directories should be collected");

        let diff = directory_watch_diff(&current, &desired);

        assert_eq!(diff.add, HashSet::from([root.join("drafts")]));
        assert_eq!(diff.remove, HashSet::from([root.join("notes")]));

        fs::remove_dir_all(root).expect("test root should be removed");
    }

    #[test]
    fn selects_the_native_directory_watch_strategy() {
        #[cfg(target_os = "linux")]
        assert_eq!(
            directory_watch_strategy(),
            DirectoryWatchStrategy::VisibleDirectories
        );

        #[cfg(not(target_os = "linux"))]
        assert_eq!(
            directory_watch_strategy(),
            DirectoryWatchStrategy::RecursiveRoot
        );
    }

    #[test]
    fn reconciles_for_directory_and_control_file_events_only() {
        let root = test_root("reconciliation-event");
        fs::create_dir_all(&root).expect("test root should be created");
        let rules = MarkdownWatchIgnoreRules::try_new(&root, None)
            .expect("watcher rules should load strictly");
        let directory_event =
            Event::new(EventKind::Create(CreateKind::Folder)).add_path(root.join("notes"));
        let control_event = Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Content)))
            .add_path(root.join(".markraignore"));
        let file_event = Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Content)))
            .add_path(root.join("note.md"));

        assert!(event_requires_reconciliation(&directory_event, &rules));
        assert!(event_requires_reconciliation(&control_event, &rules));
        assert!(!event_requires_reconciliation(&file_event, &rules));

        fs::remove_dir_all(root).expect("test root should be removed");
    }
}
