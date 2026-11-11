use super::*;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorBuffer {
    pub(super) note_id: String,
    pub(super) source: String,
}

#[derive(Serialize)]
pub struct Reconciled {
    pub(super) snapshot: Option<Snapshot>,
    pub(super) backups: Vec<String>,
}

fn same_library(a: &Library, b: &Library) -> bool {
    let (mut a, mut b) = (a.clone(), b.clone());
    for library in [&mut a, &mut b] {
        library.notes.sort_by(|a, b| a.id.cmp(&b.id));
        library.folders.sort_by(|a, b| a.id.cmp(&b.id));
    }
    a == b
}

pub(super) fn same_text(a: &str, b: &str) -> bool {
    a.replace("\r\n", "\n").replace('\r', "\n") == b.replace("\r\n", "\n").replace('\r', "\n")
}

impl DiskLibrary {
    pub(super) fn reconcile(&mut self, buffer: Option<EditorBuffer>) -> Result<Reconciled> {
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(checked(&self.root, ".miaoyan/write.lock")?)
            .map_err(error)?;
        try_lock(&lock).map_err(|_| "笔记库正在写入，稍后重新检查外部变化".to_string())?;
        if checked(&self.root, JOURNAL)?.exists()
            || checked(&self.root, folders::FOLDER_JOURNAL)?.exists()
        {
            return Err("文件事务尚未完成，暂未载入外部变化".into());
        }
        let index: Index =
            serde_json::from_slice(&fs::read(checked(&self.root, INDEX)?).map_err(error)?)
                .map_err(error)?;
        let mut next = self.clone();
        if index.generation != self.revision {
            next.data = index.library;
            next.revision = index.generation;
        }
        // Re-read the current filesystem, not individual event flags: atomic replacement
        // can report removal, creation and rename together (MiaoYan handleFileRemoval).
        next.scan()?;
        validate(&next.data)?;
        let mut backups = vec![];
        if let Some(buffer) = buffer.filter(|buffer| buffer.note_id == self.data.active_id) {
            if let Some(old) = self
                .data
                .notes
                .iter()
                .find(|note| note.id == buffer.note_id)
            {
                let external = next.data.notes.iter().find(|note| note.id == old.id);
                let changed = external.is_some_and(|note| !same_text(&note.source, &old.source));
                let removed_draft = external.is_none() && !same_text(&buffer.source, &old.source);
                if (changed || removed_draft)
                    && !external.is_some_and(|note| same_text(&buffer.source, &note.source))
                {
                    let directory = checked(&self.root, ".miaoyan-conflicts")?;
                    fs::create_dir_all(&directory).map_err(error)?;
                    let base = Path::new(&old.path)
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy();
                    let project = Path::new(&old.path)
                        .parent()
                        .and_then(Path::file_name)
                        .or_else(|| self.root.file_name())
                        .unwrap_or_default()
                        .to_string_lossy();
                    let extension = Path::new(&old.path)
                        .extension()
                        .unwrap_or_default()
                        .to_string_lossy();
                    let filename =
                        format!("{project}-{base}-{}-{}.{extension}", now() as u64, id());
                    let path = format!(".miaoyan-conflicts/{filename}");
                    atomic(
                        &checked(&self.root, &path)?,
                        buffer.source.as_bytes(),
                        false,
                    )?;
                    backups.push(path);
                }
            }
        }
        if same_library(&self.data, &next.data) && self.revision == next.revision {
            return Ok(Reconciled {
                snapshot: None,
                backups,
            });
        }
        next.revision += 1;
        next.persist_index(&next.data, next.revision)?;
        let snapshot = next.snapshot();
        *self = next;
        Ok(Reconciled {
            snapshot: Some(snapshot),
            backups,
        })
    }
}

#[cfg(feature = "app")]
#[tauri::command]
pub async fn library_reconcile(
    state: State<'_, FileLibrary>,
    root: String,
    revision: u64,
    buffer: Option<EditorBuffer>,
) -> Result<Reconciled> {
    let mut lock = state.0.lock().map_err(error)?;
    let store = lock.as_mut().ok_or("尚未打开本地笔记库")?;
    if store.root != Path::new(&root) {
        return Ok(Reconciled {
            snapshot: None,
            backups: vec![],
        });
    }
    if store.revision != revision {
        return Err("笔记库正在变化，稍后重新检查外部变化".into());
    }
    store.reconcile(buffer)
}

#[cfg(desktop)]
pub struct Watch {
    root: PathBuf,
    _watcher: notify::RecommendedWatcher,
}

#[cfg(desktop)]
impl Watch {
    pub(super) fn start(root: PathBuf, recursive: bool, changed: impl Fn(Result<()>) + Send + 'static) -> Result<Self> {
        use notify::{EventKind, RecursiveMode, Watcher};
        use std::{
            sync::mpsc,
            time::{Duration, Instant},
        };
        let (tx, rx) = mpsc::sync_channel(128);
        let watched = root.clone();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                let message = match event {
                    Ok(event)
                        if event.need_rescan()
                            || !matches!(event.kind, EventKind::Access(_))
                                && event.paths.iter().any(|path| relevant(&watched, path)) =>
                    {
                        Ok(())
                    }
                    Ok(_) => return,
                    Err(reason) => Err(error(reason)),
                };
                // A full queue already guarantees a rescan; keep the OS callback nonblocking.
                let _ = tx.try_send(message);
            })
            .map_err(error)?;
        watcher
            .watch(&root, if recursive { RecursiveMode::Recursive } else { RecursiveMode::NonRecursive })
            .map_err(error)?;
        std::thread::spawn(move || {
            while let Ok(mut message) = rx.recv() {
                let started = Instant::now();
                while started.elapsed() < Duration::from_secs(1) {
                    match rx.recv_timeout(Duration::from_millis(150)) {
                        Ok(next) => {
                            if next.is_err() {
                                message = next;
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => break,
                        Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                }
                changed(message);
            }
        });
        Ok(Self {
            root,
            _watcher: watcher,
        })
    }
}

fn relevant(root: &Path, path: &Path) -> bool {
    path.strip_prefix(root).is_ok_and(|relative| {
        !relative
            .components()
            .any(|part| part.as_os_str().to_string_lossy().starts_with('.'))
    })
}

#[cfg(feature = "app")]
#[tauri::command]
pub async fn library_watch(app: tauri::AppHandle, state: State<'_, FileLibrary>) -> Result<()> {
    #[cfg(desktop)]
    {
        let root = state
            .0
            .lock()
            .map_err(error)?
            .as_ref()
            .ok_or("尚未打开本地笔记库")?
            .root
            .clone();
        let mut watch = state.1.lock().map_err(error)?;
        if watch.as_ref().is_some_and(|watch| watch.root == root) {
            return Ok(());
        }
        use tauri::Emitter;
        let payload = root.to_string_lossy().into_owned();
        let emitter = app.clone();
        let path = payload.clone();
        let next = Watch::start(root, true, move |message| {
            if let Err(reason) = message {
                let _ = emitter.emit_to(
                    "main",
                    "library-watch-error",
                    serde_json::json!({ "root": path, "error": reason }),
                );
            }
            let _ = emitter.emit_to("main", "library-files-changed", &path);
        })?;
        *watch = Some(next);
        let _ = app.emit_to("main", "library-files-changed", &payload);
    }
    #[cfg(mobile)]
    {
        let _ = (app, state);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
