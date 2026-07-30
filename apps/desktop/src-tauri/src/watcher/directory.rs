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

const WATCH_BACKEND_RECOVERY_ATTEMPTS: usize = 3;

fn recover_supervised_watch_backend<B>(
    backend: &mut B,
    initial_error: String,
    mut rebuild: impl FnMut() -> Result<B, String>,
) -> Result<(), String> {
    let mut final_error = "watch backend rebuild was not attempted".to_string();
    for _ in 0..WATCH_BACKEND_RECOVERY_ATTEMPTS {
        match rebuild() {
            Ok(replacement) => {
                *backend = replacement;
                return Ok(());
            }
            Err(error) => final_error = error,
        }
    }
    Err(format!(
        "{initial_error}; watch backend recovery failed after {WATCH_BACKEND_RECOVERY_ATTEMPTS} attempts: {final_error}"
    ))
}

struct DirectoryWatchRoot {
    path: PathBuf,
    retained: cap_std::fs::Dir,
    identity: DirectoryIdentity,
}

#[cfg(any(target_os = "linux", test))]
struct RetainedNamedWatchDirectory {
    directory: cap_std::fs::Dir,
    identity: DirectoryIdentity,
}

#[cfg(any(target_os = "linux", test))]
impl RetainedNamedWatchDirectory {
    fn open(parent: &cap_std::fs::Dir, name: &std::ffi::OsStr) -> Result<Self, String> {
        let directory = parent
            .open_dir_nofollow(name)
            .map_err(|_| "workspace directory changed".to_string())?;
        let identity = directory_identity(&directory)
            .map_err(|_| "workspace directory changed".to_string())?;
        let retained = Self {
            directory,
            identity,
        };
        retained.verify_named(parent, name)?;
        Ok(retained)
    }

    fn directory(&self) -> &cap_std::fs::Dir {
        &self.directory
    }

    fn verify_named(
        &self,
        parent: &cap_std::fs::Dir,
        name: &std::ffi::OsStr,
    ) -> Result<(), String> {
        if directory_identity(&self.directory)
            .map_err(|_| "workspace directory changed".to_string())?
            != self.identity
        {
            return Err("workspace directory changed".to_string());
        }
        let named = parent
            .open_dir_nofollow(name)
            .map_err(|_| "workspace directory changed".to_string())?;
        if directory_identity(&named).map_err(|_| "workspace directory changed".to_string())?
            != self.identity
        {
            return Err("workspace directory changed".to_string());
        }
        Ok(())
    }
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
trait DirectoryWatchBackend {
    fn watch_directory(&mut self, path: &Path) -> Result<(), String>;
    fn unwatch_directory(&mut self, path: &Path) -> Result<(), String>;
}

#[cfg(any(target_os = "linux", test))]
impl DirectoryWatchBackend for RecommendedWatcher {
    fn watch_directory(&mut self, path: &Path) -> Result<(), String> {
        self.watch(path, RecursiveMode::NonRecursive)
            .map_err(|error| error.to_string())
    }

    fn unwatch_directory(&mut self, path: &Path) -> Result<(), String> {
        self.unwatch(path).map_err(|error| error.to_string())
    }
}

#[cfg(any(target_os = "linux", test))]
fn apply_directory_watch_diff(
    watcher: &mut impl DirectoryWatchBackend,
    watched_directories: &mut HashSet<PathBuf>,
    desired_directories: &HashSet<PathBuf>,
) -> Result<(), String> {
    let diff = directory_watch_diff(watched_directories, desired_directories);
    let mut additions = diff.add.into_iter().collect::<Vec<_>>();
    additions.sort();
    let mut added: Vec<PathBuf> = Vec::new();

    for directory in additions {
        if let Err(error) = watcher.watch_directory(&directory) {
            for added_directory in added.into_iter().rev() {
                if watcher.unwatch_directory(&added_directory).is_err() {
                    watched_directories.insert(added_directory);
                }
            }
            return Err(error);
        }
        added.push(directory);
    }
    watched_directories.extend(added);

    let mut removals = diff.remove.into_iter().collect::<Vec<_>>();
    removals.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    let mut first_error = None;
    for directory in removals {
        match watcher.unwatch_directory(&directory) {
            Ok(()) => {
                watched_directories.remove(&directory);
            }
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
    }

    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

#[cfg(any(target_os = "linux", test))]
fn reconcile_directory_watch_set_with_rebuild<B: DirectoryWatchBackend>(
    watcher: &mut B,
    watched_directories: &mut HashSet<PathBuf>,
    desired_directories: &HashSet<PathBuf>,
    rebuild: impl FnOnce(&HashSet<PathBuf>) -> Result<B, String>,
) -> Result<(), String> {
    let mutation_error =
        match apply_directory_watch_diff(watcher, watched_directories, desired_directories) {
            Ok(()) => return Ok(()),
            Err(error) => error,
        };

    // A notify backend can mutate its own path maps before returning an
    // unwatch error. Do not attempt to reason about that partial state. Build
    // a complete replacement first, then publish the backend and watch set
    // together on this coordinator thread.
    let replacement = rebuild(desired_directories).map_err(|rebuild_error| {
        format!("{mutation_error}; full watch-set rebuild failed: {rebuild_error}")
    })?;
    *watcher = replacement;
    *watched_directories = desired_directories.clone();
    Ok(())
}

#[cfg(any(target_os = "linux", test))]
fn visible_watch_directories(
    watch_root: &DirectoryWatchRoot,
    watch_rules: &mut MarkdownWatchIgnoreRules,
) -> Result<HashSet<PathBuf>, String> {
    visible_watch_directories_with_hook(watch_root, watch_rules, &mut |_| {})
}

#[cfg(any(target_os = "linux", test))]
fn visible_watch_directories_with_hook(
    watch_root: &DirectoryWatchRoot,
    watch_rules: &mut MarkdownWatchIgnoreRules,
    after_directory_open: &mut impl FnMut(&Path),
) -> Result<HashSet<PathBuf>, String> {
    fn collect(
        root: &Path,
        directory: &cap_std::fs::Dir,
        relative_directory: &Path,
        ambient_directory: &Path,
        ignore_rules: &MarkdownIgnoreRules,
        directories: &mut HashSet<PathBuf>,
        after_directory_open: &mut impl FnMut(&Path),
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
                let child = RetainedNamedWatchDirectory::open(directory, &name)?;
                after_directory_open(&relative_path);
                collect(
                    root,
                    child.directory(),
                    &relative_path,
                    &path,
                    ignore_rules,
                    directories,
                    after_directory_open,
                )?;
                child.verify_named(directory, &name)?;
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
        after_directory_open,
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

pub(super) enum DirectoryWatcherNotice {
    Event(Event),
    Recovered,
    Error(notify::Error),
}

#[derive(Debug, PartialEq, Eq)]
enum DirectoryWatcherSupervisionOutcome {
    Delivered,
    Recovered,
    Failed,
}

fn supervise_directory_backend_event<B>(
    backend: &mut B,
    result: notify::Result<Event>,
    prepare: impl FnOnce(&mut B, &Event) -> Result<(), String>,
    rebuild: impl FnMut() -> Result<B, String>,
    handler: &mut impl FnMut(DirectoryWatcherNotice),
) -> DirectoryWatcherSupervisionOutcome {
    let recovery_reason = match result {
        Ok(event) => match prepare(backend, &event) {
            Ok(()) => {
                handler(DirectoryWatcherNotice::Event(event));
                return DirectoryWatcherSupervisionOutcome::Delivered;
            }
            Err(error) => error,
        },
        Err(error) => error.to_string(),
    };

    match recover_supervised_watch_backend(backend, recovery_reason, rebuild) {
        Ok(()) => DirectoryWatcherSupervisionOutcome::Recovered,
        Err(error) => {
            handler(DirectoryWatcherNotice::Error(notify::Error::generic(
                &error,
            )));
            DirectoryWatcherSupervisionOutcome::Failed
        }
    }
}

fn recommended_watcher_for_coordinator(
    event_sender: std::sync::mpsc::Sender<CoordinatorMessage>,
) -> Result<RecommendedWatcher, String> {
    notify::recommended_watcher(move |result| {
        let _ = event_sender.send(CoordinatorMessage::BackendEvent(result));
    })
    .map_err(|error| error.to_string())
}

pub(super) struct DirectoryWatcher {
    coordinator: SupervisedDirectoryWatcher,
    ignore_rules: Arc<Mutex<MarkdownWatchIgnoreRules>>,
}

impl DirectoryWatcher {
    pub(super) fn new<F>(
        root: &Path,
        ignore_rules: Arc<Mutex<MarkdownWatchIgnoreRules>>,
        handler: F,
    ) -> Result<Self, String>
    where
        F: FnMut(DirectoryWatcherNotice) + Send + 'static,
    {
        let watch_root = Arc::new(DirectoryWatchRoot::capture(root)?);
        SupervisedDirectoryWatcher::new(Arc::clone(&watch_root), Arc::clone(&ignore_rules), handler)
            .map(|coordinator| Self {
                coordinator,
                ignore_rules,
            })
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
        self.coordinator.replace_rules(candidate)
    }
}

struct SupervisedDirectoryWatcher {
    coordinator: Option<std::thread::JoinHandle<()>>,
    sender: std::sync::mpsc::Sender<CoordinatorMessage>,
}

enum CoordinatorMessage {
    BackendEvent(notify::Result<Event>),
    ReplaceRules(
        MarkdownWatchIgnoreRules,
        std::sync::mpsc::SyncSender<Result<(), String>>,
    ),
    Shutdown,
}

impl SupervisedDirectoryWatcher {
    fn new<F>(
        watch_root: Arc<DirectoryWatchRoot>,
        ignore_rules: Arc<Mutex<MarkdownWatchIgnoreRules>>,
        handler: F,
    ) -> Result<Self, String>
    where
        F: FnMut(DirectoryWatcherNotice) + Send + 'static,
    {
        let (sender, receiver) = std::sync::mpsc::channel();
        let mut watcher = recommended_watcher_for_coordinator(sender.clone())?;
        let coordinator_rules = Arc::clone(&ignore_rules);
        let coordinator_root = Arc::clone(&watch_root);
        let coordinator_sender = sender.clone();

        #[cfg(target_os = "linux")]
        let mut watched_directories = {
            debug_assert_eq!(
                directory_watch_strategy(),
                DirectoryWatchStrategy::VisibleDirectories
            );
            let mut rules = ignore_rules
                .lock()
                .map_err(|_| "markdown ignore rules lock is poisoned".to_string())?;
            let watched_directories = visible_watch_directories(&watch_root, &mut rules)?;
            register_linux_directories(&mut watcher, &watched_directories)?;
            watched_directories
        };

        #[cfg(not(target_os = "linux"))]
        {
            debug_assert_eq!(
                directory_watch_strategy(),
                DirectoryWatchStrategy::RecursiveRoot
            );
            watcher
                .watch(&watch_root.path, RecursiveMode::Recursive)
                .map_err(|error| error.to_string())?;
        }

        let coordinator = std::thread::Builder::new()
            .name("markra-directory-watcher".to_string())
            .spawn(move || {
                #[cfg(target_os = "linux")]
                run_linux_coordinator(
                    watcher,
                    &mut watched_directories,
                    &coordinator_root,
                    &coordinator_rules,
                    handler,
                    receiver,
                    coordinator_sender,
                );

                #[cfg(not(target_os = "linux"))]
                run_recursive_coordinator(
                    watcher,
                    &coordinator_root,
                    &coordinator_rules,
                    handler,
                    receiver,
                    coordinator_sender,
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

impl Drop for SupervisedDirectoryWatcher {
    fn drop(&mut self) {
        let _ = self.sender.send(CoordinatorMessage::Shutdown);
        if let Some(coordinator) = self.coordinator.take() {
            let _ = coordinator.join();
        }
    }
}

fn recapture_watch_rules(
    ignore_rules: &Arc<Mutex<MarkdownWatchIgnoreRules>>,
) -> Result<MarkdownWatchIgnoreRules, String> {
    let (root, global_rules) = {
        let rules = ignore_rules
            .lock()
            .map_err(|_| "markdown ignore rules lock is poisoned".to_string())?;
        (rules.root.clone(), rules.global_rules.clone())
    };
    MarkdownWatchIgnoreRules::try_new(&root, global_rules.as_deref())
}

fn commit_recaptured_watch_rules(
    ignore_rules: &Arc<Mutex<MarkdownWatchIgnoreRules>>,
    candidate: MarkdownWatchIgnoreRules,
) -> Result<(), String> {
    ignore_rules
        .lock()
        .map_err(|_| "markdown ignore rules lock is poisoned".to_string())?
        .finish_reconcile(candidate, Ok(()))
}

#[cfg(not(target_os = "linux"))]
fn build_recursive_watch_backend(
    watch_root: &DirectoryWatchRoot,
    event_sender: std::sync::mpsc::Sender<CoordinatorMessage>,
) -> Result<RecommendedWatcher, String> {
    watch_root.verify_current()?;
    let mut watcher = recommended_watcher_for_coordinator(event_sender)?;
    watcher
        .watch(&watch_root.path, RecursiveMode::Recursive)
        .map_err(|error| error.to_string())?;
    watch_root.verify_current()?;
    Ok(watcher)
}

#[cfg(not(target_os = "linux"))]
fn rebuild_recursive_watch_subscription(
    watch_root: &DirectoryWatchRoot,
    ignore_rules: &Arc<Mutex<MarkdownWatchIgnoreRules>>,
    event_sender: std::sync::mpsc::Sender<CoordinatorMessage>,
) -> Result<RecommendedWatcher, String> {
    let candidate = recapture_watch_rules(ignore_rules)?;
    let watcher = build_recursive_watch_backend(watch_root, event_sender)?;
    commit_recaptured_watch_rules(ignore_rules, candidate)?;
    Ok(watcher)
}

#[cfg(not(target_os = "linux"))]
fn prepare_recursive_event(
    watch_root: &DirectoryWatchRoot,
    ignore_rules: &Arc<Mutex<MarkdownWatchIgnoreRules>>,
    event: &Event,
) -> Result<(), String> {
    watch_root.verify_current()?;
    let mut rules = ignore_rules
        .lock()
        .map_err(|_| "markdown ignore rules lock is poisoned".to_string())?;
    if let Some(candidate) = rules.stage_for_event(event)? {
        rules.finish_reconcile(candidate, Ok(()))?;
    }
    rules.current().map(|_| ())
}

#[cfg(not(target_os = "linux"))]
fn run_recursive_coordinator<F>(
    mut watcher: RecommendedWatcher,
    watch_root: &DirectoryWatchRoot,
    ignore_rules: &Arc<Mutex<MarkdownWatchIgnoreRules>>,
    mut handler: F,
    receiver: std::sync::mpsc::Receiver<CoordinatorMessage>,
    event_sender: std::sync::mpsc::Sender<CoordinatorMessage>,
) where
    F: FnMut(DirectoryWatcherNotice),
{
    while let Ok(message) = receiver.recv() {
        match message {
            CoordinatorMessage::BackendEvent(result) => {
                let outcome = supervise_directory_backend_event(
                    &mut watcher,
                    result,
                    |_, event| prepare_recursive_event(watch_root, ignore_rules, event),
                    || {
                        rebuild_recursive_watch_subscription(
                            watch_root,
                            ignore_rules,
                            event_sender.clone(),
                        )
                    },
                    &mut handler,
                );
                if outcome == DirectoryWatcherSupervisionOutcome::Recovered {
                    handler(DirectoryWatcherNotice::Recovered);
                }
            }
            CoordinatorMessage::ReplaceRules(candidate, result_sender) => {
                let result = ignore_rules
                    .lock()
                    .map_err(|_| "markdown ignore rules lock is poisoned".to_string())
                    .and_then(|mut rules| {
                        rules.finish_reconcile(candidate, watch_root.verify_current())
                    });
                let _ = result_sender.send(result);
            }
            CoordinatorMessage::Shutdown => break,
        }
    }
}

#[cfg(any(target_os = "linux", test))]
fn register_linux_directories(
    watcher: &mut RecommendedWatcher,
    directories: &HashSet<PathBuf>,
) -> Result<(), String> {
    let mut directories = directories.iter().collect::<Vec<_>>();
    directories.sort();
    for directory in directories {
        watcher
            .watch(directory, RecursiveMode::NonRecursive)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(any(target_os = "linux", test))]
fn build_linux_watch_backend(
    watch_root: &DirectoryWatchRoot,
    watched_directories: &HashSet<PathBuf>,
    event_sender: std::sync::mpsc::Sender<CoordinatorMessage>,
) -> Result<RecommendedWatcher, String> {
    watch_root.verify_current()?;
    let mut watcher = recommended_watcher_for_coordinator(event_sender)?;
    register_linux_directories(&mut watcher, watched_directories)?;
    watch_root.verify_current()?;
    Ok(watcher)
}

#[cfg(any(target_os = "linux", test))]
fn prepare_linux_event(
    watcher: &mut RecommendedWatcher,
    watched_directories: &mut HashSet<PathBuf>,
    watch_root: &DirectoryWatchRoot,
    ignore_rules: &Arc<Mutex<MarkdownWatchIgnoreRules>>,
    event: &Event,
    event_sender: &std::sync::mpsc::Sender<CoordinatorMessage>,
) -> Result<(), String> {
    watch_root.verify_current()?;
    let should_reconcile = ignore_rules
        .lock()
        .map(|rules| event_requires_reconciliation(event, &rules))
        .unwrap_or(true);
    let candidate = ignore_rules
        .lock()
        .map_err(|_| "markdown ignore rules lock is poisoned".to_string())?
        .stage_for_event(event)?;
    if let Some(mut candidate) = candidate {
        let result = reconcile_linux_directories(
            watcher,
            watched_directories,
            watch_root,
            &mut candidate,
            event_sender,
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
            watcher,
            watched_directories,
            watch_root,
            &mut rules,
            event_sender,
        ) {
            rules.invalidate();
            return Err(error);
        }
    }
    rules.current().map(|_| ())
}

#[cfg(any(target_os = "linux", test))]
fn run_linux_coordinator<F>(
    mut watcher: RecommendedWatcher,
    watched_directories: &mut HashSet<PathBuf>,
    watch_root: &DirectoryWatchRoot,
    ignore_rules: &Arc<Mutex<MarkdownWatchIgnoreRules>>,
    mut handler: F,
    receiver: std::sync::mpsc::Receiver<CoordinatorMessage>,
    event_sender: std::sync::mpsc::Sender<CoordinatorMessage>,
) where
    F: FnMut(DirectoryWatcherNotice),
{
    while let Ok(message) = receiver.recv() {
        match message {
            CoordinatorMessage::BackendEvent(result) => {
                let mut recovered_directories = None;
                let outcome = supervise_directory_backend_event(
                    &mut watcher,
                    result,
                    |watcher, event| {
                        prepare_linux_event(
                            watcher,
                            watched_directories,
                            watch_root,
                            ignore_rules,
                            event,
                            &event_sender,
                        )
                    },
                    || {
                        let mut candidate = recapture_watch_rules(ignore_rules)?;
                        let desired_directories =
                            visible_watch_directories(watch_root, &mut candidate)?;
                        let replacement = build_linux_watch_backend(
                            watch_root,
                            &desired_directories,
                            event_sender.clone(),
                        )?;
                        commit_recaptured_watch_rules(ignore_rules, candidate)?;
                        recovered_directories = Some(desired_directories);
                        Ok(replacement)
                    },
                    &mut handler,
                );
                if outcome == DirectoryWatcherSupervisionOutcome::Recovered {
                    *watched_directories = recovered_directories
                        .expect("successful Linux watcher recovery records its watch set");
                    handler(DirectoryWatcherNotice::Recovered);
                } else if outcome == DirectoryWatcherSupervisionOutcome::Failed {
                    if let Ok(mut rules) = ignore_rules.lock() {
                        rules.invalidate();
                    }
                }
            }
            CoordinatorMessage::ReplaceRules(mut candidate, result_sender) => {
                let result = reconcile_linux_directories(
                    &mut watcher,
                    watched_directories,
                    watch_root,
                    &mut candidate,
                    &event_sender,
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

#[cfg(any(target_os = "linux", test))]
fn reconcile_linux_directories(
    watcher: &mut RecommendedWatcher,
    watched_directories: &mut HashSet<PathBuf>,
    watch_root: &DirectoryWatchRoot,
    ignore_rules: &mut MarkdownWatchIgnoreRules,
    event_sender: &std::sync::mpsc::Sender<CoordinatorMessage>,
) -> Result<(), String> {
    let desired_directories = visible_watch_directories(watch_root, ignore_rules)?;
    let rebuild_sender = event_sender.clone();
    reconcile_directory_watch_set_with_rebuild(
        watcher,
        watched_directories,
        &desired_directories,
        move |desired| {
            let mut replacement = recommended_watcher_for_coordinator(rebuild_sender.clone())?;
            register_linux_directories(&mut replacement, desired)?;
            Ok(replacement)
        },
    )
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

    #[derive(Default)]
    struct FaultingDirectoryWatchBackend {
        fail_watch: HashSet<PathBuf>,
        fail_unwatch: HashSet<PathBuf>,
        watched: HashSet<PathBuf>,
    }

    #[derive(Debug, PartialEq, Eq)]
    struct ReplaceableTestWatchBackend {
        generation: usize,
    }

    #[test]
    fn bounded_recovery_replaces_a_faulted_backend_for_existing_subscriptions() {
        let mut backend = ReplaceableTestWatchBackend { generation: 1 };
        let rebuilds = std::cell::Cell::new(0);

        recover_supervised_watch_backend(&mut backend, "notify backend failed".to_string(), || {
            rebuilds.set(rebuilds.get() + 1);
            Ok(ReplaceableTestWatchBackend { generation: 2 })
        })
        .expect("the existing subscription should adopt a fresh backend");

        assert_eq!(rebuilds.get(), 1);
        assert_eq!(backend, ReplaceableTestWatchBackend { generation: 2 });
    }

    #[test]
    fn bounded_recovery_retries_without_waiting_for_a_new_subscription() {
        let mut backend = ReplaceableTestWatchBackend { generation: 1 };
        let rebuilds = std::cell::Cell::new(0);

        recover_supervised_watch_backend(&mut backend, "notify backend failed".to_string(), || {
            let attempt = rebuilds.get() + 1;
            rebuilds.set(attempt);
            if attempt < WATCH_BACKEND_RECOVERY_ATTEMPTS {
                return Err(format!("rebuild attempt {attempt} failed"));
            }
            Ok(ReplaceableTestWatchBackend { generation: 2 })
        })
        .expect("the bounded retry should recover the existing subscription");

        assert_eq!(rebuilds.get(), WATCH_BACKEND_RECOVERY_ATTEMPTS);
        assert_eq!(backend, ReplaceableTestWatchBackend { generation: 2 });
    }

    #[test]
    fn bounded_recovery_reports_the_source_and_final_rebuild_failure() {
        let mut backend = ReplaceableTestWatchBackend { generation: 1 };
        let rebuilds = std::cell::Cell::new(0);

        let error = recover_supervised_watch_backend(
            &mut backend,
            "notify backend failed".to_string(),
            || {
                let attempt = rebuilds.get() + 1;
                rebuilds.set(attempt);
                Err(format!("rebuild attempt {attempt} failed"))
            },
        )
        .expect_err("exhausted recovery must remain observable");

        assert_eq!(rebuilds.get(), WATCH_BACKEND_RECOVERY_ATTEMPTS);
        assert!(error.contains("notify backend failed"));
        assert!(error.contains("rebuild attempt 3 failed"));
        assert_eq!(backend, ReplaceableTestWatchBackend { generation: 1 });
    }

    #[test]
    fn supervisor_recovers_a_backend_error_and_delivers_the_next_event() {
        let mut backend = ReplaceableTestWatchBackend { generation: 1 };
        let mut notices = Vec::new();
        let mut handler = |notice| match notice {
            DirectoryWatcherNotice::Event(_) => notices.push("event"),
            DirectoryWatcherNotice::Recovered => notices.push("recovered"),
            DirectoryWatcherNotice::Error(_) => notices.push("error"),
        };

        let recovery = supervise_directory_backend_event(
            &mut backend,
            Err(notify::Error::generic("backend failed")),
            |_, _| Ok(()),
            || Ok(ReplaceableTestWatchBackend { generation: 2 }),
            &mut handler,
        );
        if recovery == DirectoryWatcherSupervisionOutcome::Recovered {
            handler(DirectoryWatcherNotice::Recovered);
        }
        let delivery = supervise_directory_backend_event(
            &mut backend,
            Ok(Event::new(EventKind::Any)),
            |_, _| Ok(()),
            || Ok(ReplaceableTestWatchBackend { generation: 3 }),
            &mut handler,
        );

        assert_eq!(recovery, DirectoryWatcherSupervisionOutcome::Recovered);
        assert_eq!(delivery, DirectoryWatcherSupervisionOutcome::Delivered);
        assert_eq!(backend, ReplaceableTestWatchBackend { generation: 2 });
        assert_eq!(notices, ["recovered", "event"]);
    }

    #[test]
    fn supervisor_rebuilds_after_event_preparation_fails() {
        let mut backend = ReplaceableTestWatchBackend { generation: 1 };
        let mut notices = Vec::new();

        let outcome = supervise_directory_backend_event(
            &mut backend,
            Ok(Event::new(EventKind::Any)),
            |_, _| Err("event preparation failed".to_string()),
            || Ok(ReplaceableTestWatchBackend { generation: 2 }),
            &mut |notice| match notice {
                DirectoryWatcherNotice::Event(_) => notices.push("event"),
                DirectoryWatcherNotice::Recovered => notices.push("recovered"),
                DirectoryWatcherNotice::Error(_) => notices.push("error"),
            },
        );
        if outcome == DirectoryWatcherSupervisionOutcome::Recovered {
            notices.push("recovered");
        }

        assert_eq!(outcome, DirectoryWatcherSupervisionOutcome::Recovered);
        assert_eq!(backend, ReplaceableTestWatchBackend { generation: 2 });
        assert_eq!(notices, ["recovered"]);
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn recursive_watcher_recovers_without_a_new_subscription_after_backend_error() {
        enum ObservedNotice {
            Event(Event),
            Recovered,
            Error,
        }

        let root = test_root("recursive-recovery");
        fs::create_dir_all(&root).expect("test root should be created");
        let rules = Arc::new(Mutex::new(
            MarkdownWatchIgnoreRules::try_new(&root, None).expect("watcher rules should load"),
        ));
        let (notice_sender, notice_receiver) = std::sync::mpsc::channel();
        let watcher = DirectoryWatcher::new(&root, rules, move |notice| {
            let observed = match notice {
                DirectoryWatcherNotice::Event(event) => ObservedNotice::Event(event),
                DirectoryWatcherNotice::Recovered => ObservedNotice::Recovered,
                DirectoryWatcherNotice::Error(_) => ObservedNotice::Error,
            };
            let _ = notice_sender.send(observed);
        })
        .expect("recursive watcher should start");

        watcher
            .coordinator
            .sender
            .send(CoordinatorMessage::BackendEvent(Err(
                notify::Error::generic("injected backend failure"),
            )))
            .expect("backend failure should reach the supervisor");
        assert!(matches!(
            notice_receiver
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("recovery notice should arrive"),
            ObservedNotice::Recovered
        ));

        let note = root.join("recovered.md");
        fs::write(&note, "recovered").expect("post-recovery file should be written");
        let observed_change = (0..10).any(|_| {
            matches!(
                notice_receiver.recv_timeout(std::time::Duration::from_secs(1)),
                Ok(ObservedNotice::Event(event))
                    if event.paths.iter().any(|path| path.file_name() == note.file_name())
            )
        });

        drop(watcher);
        fs::remove_dir_all(root).expect("test root should be removed");
        assert!(
            observed_change,
            "the rebuilt backend should deliver later events"
        );
    }

    #[test]
    fn linux_coordinator_recovers_without_a_new_subscription_after_backend_error() {
        enum ObservedNotice {
            Event(Event),
            Recovered,
            Error,
        }

        let root = test_root("linux-coordinator-recovery");
        fs::create_dir_all(&root).expect("test root should be created");
        let watch_root =
            Arc::new(DirectoryWatchRoot::capture(&root).expect("watch root should capture"));
        let rules = Arc::new(Mutex::new(
            MarkdownWatchIgnoreRules::try_new(&root, None).expect("watcher rules should load"),
        ));
        let watched_directories = HashSet::from([root.clone()]);
        let (coordinator_sender, coordinator_receiver) = std::sync::mpsc::channel();
        let watcher = build_linux_watch_backend(
            &watch_root,
            &watched_directories,
            coordinator_sender.clone(),
        )
        .expect("non-recursive backend should start");
        let (notice_sender, notice_receiver) = std::sync::mpsc::channel();
        let coordinator_root = Arc::clone(&watch_root);
        let coordinator_rules = Arc::clone(&rules);
        let event_sender = coordinator_sender.clone();
        let coordinator = std::thread::spawn(move || {
            let mut watched_directories = watched_directories;
            run_linux_coordinator(
                watcher,
                &mut watched_directories,
                &coordinator_root,
                &coordinator_rules,
                move |notice| {
                    let observed = match notice {
                        DirectoryWatcherNotice::Event(event) => ObservedNotice::Event(event),
                        DirectoryWatcherNotice::Recovered => ObservedNotice::Recovered,
                        DirectoryWatcherNotice::Error(_) => ObservedNotice::Error,
                    };
                    let _ = notice_sender.send(observed);
                },
                coordinator_receiver,
                event_sender,
            );
        });

        coordinator_sender
            .send(CoordinatorMessage::BackendEvent(Err(
                notify::Error::generic("injected Linux backend failure"),
            )))
            .expect("backend failure should reach the Linux coordinator");
        assert!(matches!(
            notice_receiver
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("Linux recovery notice should arrive"),
            ObservedNotice::Recovered
        ));

        let note = root.join("recovered-linux.md");
        fs::write(&note, "recovered").expect("post-recovery file should be written");
        let observed_change = (0..10).any(|_| {
            matches!(
                notice_receiver.recv_timeout(std::time::Duration::from_secs(1)),
                Ok(ObservedNotice::Event(event))
                    if event.paths.iter().any(|path| path.file_name() == note.file_name())
            )
        });

        coordinator_sender
            .send(CoordinatorMessage::Shutdown)
            .expect("Linux coordinator should stop");
        coordinator.join().expect("Linux coordinator should join");
        fs::remove_dir_all(root).expect("test root should be removed");
        assert!(
            observed_change,
            "the rebuilt Linux backend should deliver later events"
        );
    }

    impl DirectoryWatchBackend for FaultingDirectoryWatchBackend {
        fn watch_directory(&mut self, path: &Path) -> Result<(), String> {
            if self.fail_watch.remove(path) {
                return Err(format!("watch failed: {}", path.display()));
            }
            self.watched.insert(path.to_path_buf());
            Ok(())
        }

        fn unwatch_directory(&mut self, path: &Path) -> Result<(), String> {
            if self.fail_unwatch.remove(path) {
                return Err(format!("unwatch failed: {}", path.display()));
            }
            if self.watched.remove(path) {
                Ok(())
            } else {
                Err(format!("watch not found: {}", path.display()))
            }
        }
    }

    fn rebuilt_test_backend(desired: &HashSet<PathBuf>) -> FaultingDirectoryWatchBackend {
        FaultingDirectoryWatchBackend {
            watched: desired.clone(),
            ..FaultingDirectoryWatchBackend::default()
        }
    }

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
    fn rebuilds_the_complete_watch_set_after_an_addition_failure() {
        let root = PathBuf::from("/watch-root");
        let added = root.join("added");
        let desired = HashSet::from([root.clone(), added.clone()]);
        let mut watched = HashSet::from([root.clone()]);
        let mut backend = FaultingDirectoryWatchBackend {
            fail_watch: HashSet::from([added]),
            watched: watched.clone(),
            ..FaultingDirectoryWatchBackend::default()
        };
        let rebuilds = std::cell::Cell::new(0);

        reconcile_directory_watch_set_with_rebuild(
            &mut backend,
            &mut watched,
            &desired,
            |desired| {
                rebuilds.set(rebuilds.get() + 1);
                Ok(rebuilt_test_backend(desired))
            },
        )
        .expect("a fresh backend should recover the desired watch set");

        assert_eq!(rebuilds.get(), 1);
        assert_eq!(watched, desired);
        assert_eq!(backend.watched, desired);
    }

    #[test]
    fn rebuilds_after_an_addition_and_its_rollback_both_fail() {
        let root = PathBuf::from("/watch-root");
        let first = root.join("a-first");
        let second = root.join("b-second");
        let desired = HashSet::from([root.clone(), first.clone(), second.clone()]);
        let mut watched = HashSet::from([root.clone()]);
        let mut backend = FaultingDirectoryWatchBackend {
            fail_watch: HashSet::from([second]),
            fail_unwatch: HashSet::from([first]),
            watched: watched.clone(),
        };
        let rebuilds = std::cell::Cell::new(0);

        reconcile_directory_watch_set_with_rebuild(
            &mut backend,
            &mut watched,
            &desired,
            |desired| {
                rebuilds.set(rebuilds.get() + 1);
                Ok(rebuilt_test_backend(desired))
            },
        )
        .expect("a fresh backend should recover from a failed rollback");

        assert_eq!(rebuilds.get(), 1);
        assert_eq!(watched, desired);
        assert_eq!(backend.watched, desired);
    }

    #[test]
    fn rebuilds_after_an_existing_watch_cannot_be_removed() {
        let root = test_root("remove-failure");
        let removed = root.join("removed");
        fs::create_dir_all(&removed).expect("removed directory should exist");
        let desired = HashSet::from([root.clone()]);
        let mut watched = HashSet::from([root.clone(), removed.clone()]);
        let mut backend = FaultingDirectoryWatchBackend {
            fail_unwatch: HashSet::from([removed]),
            watched: watched.clone(),
            ..FaultingDirectoryWatchBackend::default()
        };
        let rebuilds = std::cell::Cell::new(0);

        reconcile_directory_watch_set_with_rebuild(
            &mut backend,
            &mut watched,
            &desired,
            |desired| {
                rebuilds.set(rebuilds.get() + 1);
                Ok(rebuilt_test_backend(desired))
            },
        )
        .expect("a fresh backend should recover from an unwatch failure");

        assert_eq!(rebuilds.get(), 1);
        assert_eq!(watched, desired);
        assert_eq!(backend.watched, desired);
        fs::remove_dir_all(root).expect("test root should be removed");
    }

    #[test]
    fn rebuilds_when_a_missing_watch_cannot_be_removed() {
        let root = test_root("missing-remove-failure");
        let missing = root.join("missing");
        let desired = HashSet::from([root.clone()]);
        let mut watched = HashSet::from([root.clone(), missing.clone()]);
        let mut backend = FaultingDirectoryWatchBackend {
            fail_unwatch: HashSet::from([missing]),
            watched: watched.clone(),
            ..FaultingDirectoryWatchBackend::default()
        };
        let rebuilds = std::cell::Cell::new(0);

        reconcile_directory_watch_set_with_rebuild(
            &mut backend,
            &mut watched,
            &desired,
            |desired| {
                rebuilds.set(rebuilds.get() + 1);
                Ok(rebuilt_test_backend(desired))
            },
        )
        .expect("a fresh backend should recover the missing watch state");

        assert_eq!(rebuilds.get(), 1);
        assert_eq!(watched, desired);
        assert_eq!(backend.watched, desired);
    }

    #[test]
    fn reports_a_full_rebuild_failure_after_watch_set_mutation_fails() {
        let root = PathBuf::from("/watch-root");
        let added = root.join("added");
        let desired = HashSet::from([root.clone(), added.clone()]);
        let mut watched = HashSet::from([root.clone()]);
        let mut backend = FaultingDirectoryWatchBackend {
            fail_watch: HashSet::from([added]),
            watched: watched.clone(),
            ..FaultingDirectoryWatchBackend::default()
        };

        let error = reconcile_directory_watch_set_with_rebuild(
            &mut backend,
            &mut watched,
            &desired,
            |_| Err("full rebuild failed".to_string()),
        )
        .expect_err("a failed full rebuild must propagate");

        assert!(error.contains("full rebuild failed"));
    }

    #[cfg(unix)]
    #[test]
    fn visible_watch_set_rejects_an_open_child_replaced_by_an_external_symlink() {
        use std::os::unix::fs::symlink;

        let root = test_root("open-child-swap");
        let outside = root.with_extension("outside");
        let displaced = root.join("captured-directory");
        fs::create_dir_all(root.join("nested")).expect("nested directory should be created");
        fs::create_dir_all(&outside).expect("outside directory should be created");
        let mut rules =
            MarkdownWatchIgnoreRules::try_new(&root, None).expect("watcher rules should load");
        let watch_root = DirectoryWatchRoot::capture(&root).expect("watch root should capture");
        let mut swapped = false;

        let result =
            visible_watch_directories_with_hook(&watch_root, &mut rules, &mut |relative_path| {
                if !swapped && relative_path == Path::new("nested") {
                    fs::rename(root.join("nested"), &displaced)
                        .expect("opened child should be displaced");
                    symlink(&outside, root.join("nested"))
                        .expect("external directory symlink should be installed");
                    swapped = true;
                }
            });

        fs::remove_file(root.join("nested")).expect("symlink should be removed");
        fs::remove_dir_all(root).expect("captured root should be removed");
        fs::remove_dir_all(outside).expect("outside root should be removed");
        assert_eq!(result, Err("workspace directory changed".to_string()));
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
