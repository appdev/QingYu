//! Editor windows and their save-before-close lifecycle.
use super::*;
use tauri::Emitter;
use std::sync::{atomic::{AtomicBool, Ordering}, mpsc};

struct Document {
    store: DiskLibrary,
    pending: Vec<PathBuf>,
    #[cfg(desktop)]
    _watch: external::Watch,
}

#[derive(Clone)]
struct NoteWindow {
    root: PathBuf,
    initial_note: String,
}

struct ExitRound {
    id: String,
    pending: HashSet<String>,
    result: Option<mpsc::Sender<Result<()>>>,
}

impl ExitRound {
    fn reply(&mut self, id: &str, label: &str, saved: bool) {
        if self.id != id || !self.pending.contains(label) { return; }
        if !saved {
            if let Some(send) = self.result.take() { let _ = send.send(Err("有文档尚未保存，请完成输入或处理保存错误后再退出".into())); }
        } else {
            self.pending.remove(label);
            if self.pending.is_empty() {
                if let Some(send) = self.result.take() { let _ = send.send(Ok(())); }
            }
        }
    }
}

#[derive(Default)]
pub struct Workspace {
    documents: Mutex<HashMap<String, Document>>,
    notes: Mutex<HashMap<String, NoteWindow>>,
    requests: Mutex<Vec<PathBuf>>,
    pub(super) opening: Mutex<()>,
    quitting: AtomicBool,
    exit: Mutex<Option<ExitRound>>,
}

pub(super) fn with_document<T>(app: &tauri::AppHandle, label: &str, run: impl FnOnce(&DiskLibrary) -> Result<T>) -> Result<T> {
    let state = app.state::<Workspace>();
    let documents = state.documents.lock().map_err(error)?;
    run(&documents.get(label).ok_or("文档窗口不存在")?.store)
}

impl Workspace {
    fn labels(&self) -> Result<HashSet<String>> {
        let mut labels: HashSet<_> = self.documents.lock().map_err(error)?.keys().cloned().collect();
        labels.extend(self.notes.lock().map_err(error)?.keys().cloned());
        Ok(labels)
    }

    pub(super) fn has_note_windows(&self) -> Result<bool> {
        Ok(!self.notes.lock().map_err(error)?.is_empty())
    }
}

pub(super) fn check_library_window(app: &tauri::AppHandle, label: &str, root: &Path) -> Result<bool> {
    if label == "main" { return Ok(false); }
    let state = app.state::<Workspace>();
    let notes = state.notes.lock().map_err(error)?;
    let window = notes.get(label).ok_or("请在笔记库窗口中操作")?;
    if window.root != root { return Err("笔记库位置已更改，当前窗口无法访问新目录".into()); }
    Ok(true)
}

pub(super) fn library_changed(app: &tauri::AppHandle, root: &str, origin: Option<&str>) {
    let state = app.state::<Workspace>();
    let labels: Vec<_> = state.notes.lock().map(|notes| notes.iter()
        .filter(|(_, window)| window.root == Path::new(root))
        .map(|(label, _)| label.clone()).collect()).unwrap_or_default();
    if labels.is_empty() && origin.is_some() { return; }
    for label in std::iter::once("main").chain(labels.iter().map(String::as_str)) {
        if Some(label) != origin { let _ = app.emit_to(label, "library-files-changed", root); }
    }
}

#[cfg(desktop)]
pub fn request(app: &tauri::AppHandle, paths: impl IntoIterator<Item = PathBuf>) {
    let paths: Vec<_> = paths.into_iter().filter(|path| allowed(path)).collect();
    if paths.is_empty() { let _ = show_library(app); return; }
    if let Ok(mut pending) = app.state::<Workspace>().requests.lock() {
        pending.extend(paths);
    }
    let _ = app.emit_to("main", "workspace-request", ());
}

pub fn show_library(app: &tauri::AppHandle) -> Result<()> {
    let window = app.get_webview_window("main").ok_or("笔记库窗口不存在")?;
    #[cfg(desktop)]
    window.unminimize().map_err(error)?;
    window.show().map_err(error)?; window.set_focus().map_err(error)
}

pub fn has_documents(app: &tauri::AppHandle) -> bool {
    app.state::<Workspace>().labels().is_ok_and(|labels| !labels.is_empty())
}

pub fn is_document(app: &tauri::AppHandle, label: &str) -> bool {
    let state = app.state::<Workspace>();
    state.documents.lock().is_ok_and(|docs| docs.contains_key(label)) || state.notes.lock().is_ok_and(|notes| notes.contains_key(label))
}

pub fn destroyed(app: &tauri::AppHandle, label: &str) {
    let state = app.state::<Workspace>();
    if let Ok(mut docs) = state.documents.lock() { docs.remove(label); }
    if let Ok(mut notes) = state.notes.lock() { notes.remove(label); }
    if let Ok(mut round) = state.exit.lock() {
        if let Some(round) = round.as_mut() { let id = round.id.clone(); round.reply(&id, label, false); }
    };
}

fn authorize_editor(window: &tauri::WebviewWindow) -> Result<()> {
    if window.label() == "main" || is_document(window.app_handle(), window.label()) { Ok(()) }
    else { Err("只能在编辑器中操作文档窗口".into()) }
}

#[tauri::command]
pub fn workspace_snapshot(state: State<'_, Workspace>, window: tauri::WebviewWindow) -> Result<Snapshot> {
    Ok(state.documents.lock().map_err(error)?.get(window.label()).ok_or("文档窗口不存在")?.store.snapshot())
}

#[tauri::command]
pub fn workspace_paths(state: State<'_, Workspace>, window: tauri::WebviewWindow) -> Result<Vec<String>> {
    Ok(state.documents.lock().map_err(error)?.get_mut(window.label()).ok_or("目录窗口不存在")?.pending.drain(..).map(|path| path.to_string_lossy().into()).collect())
}

#[tauri::command]
pub fn workspace_show_library(app: tauri::AppHandle, window: tauri::WebviewWindow) -> Result<()> {
    authorize_editor(&window)?; show_library(&app)
}

#[tauri::command]
pub fn workspace_finish_close(app: tauri::AppHandle, window: tauri::WebviewWindow) -> Result<()> {
    if !is_document(&app, window.label()) { return Err("文档窗口不存在".into()); }
    if app.state::<Workspace>().quitting.load(Ordering::Acquire) { return Err("应用正在退出，请稍候".into()); }
    window.destroy().map_err(error)
}

#[tauri::command]
pub fn library_hide_main(window: tauri::WebviewWindow) -> Result<()> {
    main_window(&window)?; window.hide().map_err(error)
}

fn cancel_exit(app: &tauri::AppHandle) {
    let state = app.state::<Workspace>();
    if let Ok(mut round) = state.exit.lock() { *round = None; }
    state.quitting.store(false, Ordering::Release);
    if let Ok(labels) = state.labels() {
        for label in labels { let _ = app.emit_to(&label, "workspace-exit-cancelled", ()); }
    };
}

#[tauri::command]
pub fn workspace_cancel_exit(app: tauri::AppHandle, window: tauri::WebviewWindow) -> Result<()> {
    main_window(&window)?; cancel_exit(&app); Ok(())
}

#[tauri::command]
pub fn workspace_exit_reply(state: State<'_, Workspace>, window: tauri::WebviewWindow, id: String, saved: bool) -> Result<()> {
    if let Some(round) = state.exit.lock().map_err(error)?.as_mut() { round.reply(&id, window.label(), saved); }
    Ok(())
}

#[tauri::command]
pub fn workspace_ready(state: State<'_, Workspace>, window: tauri::WebviewWindow) -> Result<Option<String>> {
    if !is_document(window.app_handle(), window.label()) { return Err("文档窗口不存在".into()); }
    Ok(state.exit.lock().map_err(error)?.as_ref().filter(|round| round.pending.contains(window.label())).map(|round| round.id.clone()))
}

#[tauri::command]
pub async fn workspace_prepare_exit(app: tauri::AppHandle, window: tauri::WebviewWindow) -> Result<()> {
    main_window(&window)?;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<Workspace>();
        let (receive, labels, request) = {
            let _creation = state.opening.lock().map_err(error)?;
            if state.quitting.swap(true, Ordering::AcqRel) { return Err("应用已经在退出".into()); }
            let labels = state.labels()?;
            if labels.is_empty() { return Ok(()); }
            let (send, receive) = mpsc::channel(); let request = id();
            *state.exit.lock().map_err(error)? = Some(ExitRound { id: request.clone(), pending: labels.clone(), result: Some(send) });
            (receive, labels, request)
        };
        for label in labels { let _ = app.emit_to(&label, "workspace-before-exit", &request); }
        let result = receive.recv_timeout(std::time::Duration::from_secs(15)).unwrap_or_else(|_| Err("文档窗口尚未响应，已取消退出以保留编辑内容".into()));
        if result.is_err() { cancel_exit(&app); }
        result
    }).await.map_err(error)?
}

pub fn exit_allowed(app: &tauri::AppHandle) -> bool {
    let state = app.state::<Workspace>();
    !has_documents(app) || state.quitting.load(Ordering::Acquire) && state.exit.lock().is_ok_and(|round| round.as_ref().is_some_and(|round| round.pending.is_empty()))
}

#[tauri::command]
pub fn workspace_requests(state: State<'_, Workspace>, window: tauri::WebviewWindow) -> Result<Vec<String>> {
    main_window(&window)?;
    Ok(state.requests.lock().map_err(error)?.drain(..).map(|p| p.to_string_lossy().into()).collect())
}

fn main_window(window: &tauri::WebviewWindow) -> Result<()> {
    if window.label() != "main" { return Err("只能在编辑器中打开外部文件".into()); }
    Ok(())
}

fn open_file(path: &Path) -> Result<DiskLibrary> {
    let path = path.canonicalize().map_err(error)?;
    if !path.is_file() || !allowed(&path) { return Err("请选择 Markdown 或文本文件".into()); }
    let root = path.parent().ok_or("文件目录无效")?.to_path_buf();
    let mut store = DiskLibrary {
        root: root.clone(), revision: 0, bytes: HashMap::new(), folder_identities: HashMap::new(),
        data: Library { version: 1, folders: vec![Folder { id: "default".into(), name: root.file_name().unwrap_or_default().to_string_lossy().into(), path: String::new() }], notes: vec![], active_id: String::new(), sync: None },
    };
    rescan(&mut store)?;
    store.data.active_id = store.data.notes.iter().find(|note| root.join(&note.path) == path).ok_or("无法读取所选文件")?.id.clone();
    Ok(store)
}

fn rescan(store: &mut DiskLibrary) -> Result<()> {
    let mut notes = vec![];
    let mut bytes = HashMap::new();
    for entry in fs::read_dir(&store.root).map_err(error)? {
        let entry = entry.map_err(error)?;
        if !allowed(&entry.path()) || !entry.file_type().map_err(error)?.is_file() { continue; }
        let filename = entry.file_name().into_string().map_err(|_| "文件名不是有效的 Unicode")?;
        if filename.starts_with('.') { continue; }
        let path = checked(&store.root, &filename)?;
        let meta = fs::metadata(&path).map_err(error)?;
        let old = store.data.notes.iter().find(|note| note.path == filename);
        let id = old.map_or_else(id, |note| note.id.clone());
        let (source, bad) = match if meta.len() > 4 * 1024 * 1024 { Err(io::Error::other("文件超过 4 MiB")) } else { fs::read(&path) } {
            Ok(raw) => { let decoded = decode(&raw); bytes.insert(id.clone(), raw); decoded }
            Err(_) => (String::new(), true),
        };
        notes.push(Note {
            id, folder_id: "default".into(), source, created_at: millis(meta.created().unwrap_or(SystemTime::now())),
            updated_at: millis(meta.modified().map_err(error)?), title: Some(display_title(&filename)),
            pinned: old.is_some_and(|note| note.pinned), trashed: false, conflict_of: None,
            path: filename.clone(), filename: Some(filename), read_only: bad || meta.permissions().readonly(),
        });
    }
    notes.sort_by(|a, b| a.path.cmp(&b.path));
    if !notes.iter().any(|note| note.id == store.data.active_id) { store.data.active_id = notes.first().map(|note| note.id.clone()).unwrap_or_default(); }
    store.data.notes = notes; store.bytes = bytes;
    Ok(())
}

fn selected_path(store: &DiskLibrary) -> Option<PathBuf> {
    store.data.notes.iter().find(|note| note.id == store.data.active_id).map(|note| store.root.join(&note.path))
}

fn document_owner(documents: &HashMap<String, Document>, path: &Path, except: &str) -> Option<String> {
    documents.iter().find(|(label, doc)| label.as_str() != except && selected_path(&doc.store).as_deref() == Some(path)).map(|(label, _)| label.clone())
}

fn directory_owner(documents: &HashMap<String, Document>, path: &Path) -> Option<String> {
    documents.iter().find(|(_, doc)| Some(doc.store.root.as_path()) == path.parent()).map(|(label, _)| label.clone())
}

#[cfg(desktop)]
fn focus(app: &tauri::AppHandle, label: &str) -> Result<()> {
    let window = app.get_webview_window(label).ok_or("文档窗口不存在")?;
    window.unminimize().map_err(error)?; window.show().map_err(error)?; window.set_focus().map_err(error)
}

#[tauri::command]
pub async fn workspace_open_note(app: tauri::AppHandle, window: tauri::WebviewWindow, note_id: String) -> Result<()> {
    if window.label() != "main" { return Err("请在笔记库中打开独立窗口".into()); }
    #[cfg(mobile)]
    { let _ = (app, note_id); Err("独立笔记窗口仅在桌面端可用".into()) }
    #[cfg(desktop)]
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<Workspace>();
        let _creation = state.opening.lock().map_err(error)?;
        if state.quitting.load(Ordering::Acquire) { return Err("应用正在退出，暂不能打开文件".into()); }
        let (root, title) = {
            let library = app.state::<FileLibrary>();
            let store = library.0.lock().map_err(error)?;
            let store = store.as_ref().ok_or("请先打开本地笔记库")?;
            let note = store.data.notes.iter().find(|note| note.id == note_id && !note.trashed).ok_or("笔记不存在")?;
            (store.root.clone(), display_title(&note.path))
        };
        let mut notes = state.notes.lock().map_err(error)?;
        if let Some((label, _)) = notes.iter().find(|(_, note)| note.root == root && note.initial_note == note_id) {
            let label = label.clone();
            drop(notes);
            return focus(&app, &label);
        }
        let label = format!("document-note-{}", id());
        notes.insert(label.clone(), NoteWindow { root, initial_note: note_id });
        drop(notes);
        let builder = tauri::WebviewWindowBuilder::new(&app, &label, tauri::WebviewUrl::App("index.html".into()))
            .title(title).inner_size(720., 620.).min_inner_size(480., 360.);
        #[cfg(target_os = "macos")]
        let builder = builder.title_bar_style(tauri::TitleBarStyle::Overlay).hidden_title(true)
            .traffic_light_position(tauri::LogicalPosition::new(18., 30.));
        if let Err(reason) = builder.build() {
            state.notes.lock().map_err(error)?.remove(&label);
            return Err(error(reason));
        }
        focus(&app, &label)
    }).await.map_err(error)?
}

fn note_snapshot(store: &DiskLibrary, note_id: &str) -> Result<Snapshot> {
    if !store.data.notes.iter().any(|note| note.id == note_id) { return Err("笔记不存在".into()); }
    let mut snapshot = store.snapshot();
    snapshot.library.active_id = note_id.into();
    Ok(snapshot)
}

#[tauri::command]
pub fn workspace_note_snapshot(app: tauri::AppHandle, window: tauri::WebviewWindow, note_id: Option<String>) -> Result<Snapshot> {
    let workspace = app.state::<Workspace>();
    let scope = workspace.notes.lock().map_err(error)?.get(window.label()).cloned().ok_or("独立笔记窗口不存在")?;
    let library = app.state::<FileLibrary>();
    let library = library.0.lock().map_err(error)?;
    let store = library.as_ref().ok_or("请先打开本地笔记库")?;
    if scope.root != store.root { return Err("笔记库位置已更改，当前窗口无法访问新目录".into()); }
    let note_id = note_id.as_deref().unwrap_or(&scope.initial_note);
    let snapshot = note_snapshot(store, note_id)?;
    drop(library);
    if let Some(note) = snapshot.library.notes.iter().find(|note| note.id == note_id) {
        let _ = window.set_title(&display_title(&note.path));
    }
    Ok(snapshot)
}

#[tauri::command]
pub async fn workspace_open(app: tauri::AppHandle, window: tauri::WebviewWindow, path: Option<String>) -> Result<Option<String>> {
    authorize_editor(&window)?;
    #[cfg(mobile)]
    { let _ = (app, path); Err("目前仅桌面端支持外部文件工作区".into()) }
    #[cfg(desktop)]
    tauri::async_runtime::spawn_blocking(move || {
        use tauri_plugin_dialog::DialogExt;
        let path = match path {
            Some(path) => PathBuf::from(path),
            None => match window.dialog().file().set_parent(&window).add_filter("Markdown / 文本", &["md", "markdown", "txt"]).blocking_pick_file() {
                Some(path) => path.into_path().map_err(|_| "请选择本地文件")?, None => return Ok(None),
            },
        };
        let path = path.canonicalize().map_err(error)?;
        if !path.is_file() || !allowed(&path) { return Err("请选择 Markdown 或文本文件".into()); }
        let state = app.state::<Workspace>();
        let _creation = state.opening.lock().map_err(error)?;
        if state.quitting.load(Ordering::Acquire) { return Err("应用正在退出，暂不能打开文件".into()); }
        let owner = {
            let docs = state.documents.lock().map_err(error)?;
            document_owner(&docs, &path, "").or_else(|| directory_owner(&docs, &path))
        };
        let owner = owner.or_else(|| app.state::<FileLibrary>().0.lock().ok().and_then(|library| library.as_ref().and_then(selected_path)).filter(|current| current == &path).map(|_| "main".to_string()));
        if let Some(label) = owner {
            let select = {
                let mut docs = state.documents.lock().map_err(error)?;
                if let Some(doc) = docs.get_mut(&label) {
                    doc.pending.push(path); true
                } else { false }
            };
            focus(&app, &label)?;
            if select { app.emit_to(&label, "workspace-select-request", ()).map_err(error)?; }
            return Ok(Some(label));
        }
        let store = open_file(&path)?;
        let title = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let label = format!("document-{}", id());
        let root = store.root.clone();
        let events = app.clone(); let target = label.clone();
        let watch = external::Watch::start(root.clone(), false, move |result| {
            if let Err(error) = result { let _ = events.emit_to(&target, "workspace-watch-error", serde_json::json!({"root": root, "error": error})); }
            else { let _ = events.emit_to(&target, "workspace-files-changed", root.to_string_lossy()); }
        })?;
        state.documents.lock().map_err(error)?.insert(label.clone(), Document { store, pending: vec![], _watch: watch });
        let mut builder = tauri::WebviewWindowBuilder::new(&app, &label, tauri::WebviewUrl::App("index.html?document=1".into()))
            .title(title).inner_size(1120., 780.).min_inner_size(360., 420.);
        if let Some(main) = app.config().app.windows.first() {
            if let (Some(width), Some(height)) = (main.min_width, main.min_height) {
                builder = builder.min_inner_size(width, height);
            }
        }
        #[cfg(target_os = "macos")]
        let builder = builder.title_bar_style(tauri::TitleBarStyle::Overlay).hidden_title(true).traffic_light_position(tauri::LogicalPosition::new(18., 30.));
        if let Err(reason) = builder.build() { state.documents.lock().map_err(error)?.remove(&label); return Err(error(reason)); }
        focus(&app, &label)?;
        Ok(Some(label))
    }).await.map_err(error)?
}

fn commit(store: &mut DiskLibrary, next: Library, revision: u64) -> Result<Snapshot> {
    validate(&next)?;
    if store.revision != revision { return Err("外部文件已更新，请重新读取后再编辑".into()); }
    if next.sync.is_some() || next.folders != store.data.folders || next.notes.len() < store.data.notes.len() || next.notes.len() > store.data.notes.len() + 1 {
        return Err("临时文件模式不支持更改目录、移动或删除文件".into());
    }
    let mut changed = vec![];
    for note in &next.notes {
        if let Some(old) = store.data.notes.iter().find(|old| old.id == note.id) {
            if note.path != old.path || note.folder_id != old.folder_id || note.trashed || note.read_only != old.read_only { return Err("外部文件路径无效".into()); }
            if note.source != old.source || note.title != old.title { changed.push((Some(old.clone()), note)); }
        } else { changed.push((None, note)); }
    }
    if store.data.notes.iter().any(|old| !next.notes.iter().any(|note| note.id == old.id)) || changed.len() > 1 { return Err("请逐个保存外部文件".into()); }
    for (old, note) in changed {
        if note.source.len() > 4 * 1024 * 1024 || note.folder_id != "default" || note.trashed { return Err("外部文件内容或目录无效".into()); }
        let title = name(note.title.as_deref().unwrap_or("无标题"))?;
        if title.is_empty() { return Err("文件名不能为空".into()); }
        if let Some(old) = old {
            if old.read_only { return Err("此文件只读，未覆盖原文".into()); }
            let path = checked(&store.root, &old.path)?;
            let raw = store.bytes.get(&old.id).ok_or("原文件不可读")?;
            if fs::read(&path).map_err(error)? != *raw { return Err("文件已在外部修改，未覆盖；请重新读取，编辑内容仍在编辑器中".into()); }
            if note.title != old.title {
                if note.source != old.source { return Err("请先保存正文再重命名".into()); }
                let ext = Path::new(&old.path).extension().and_then(|s| s.to_str()).unwrap_or("md");
                let filename = format!("{title}.{ext}");
                move_exclusive(&path, &checked(&store.root, &filename)?)?;
                // Keep the session identity through our own rename.
                store.data.notes.iter_mut().find(|n| n.id == note.id).unwrap().path = filename;
            } else { atomic(&path, note.source.as_bytes(), true)?; }
        } else {
            let filename = unique(&store.root, "", &title, "md", &mut HashSet::new(), "create")?;
            atomic(&checked(&store.root, &filename)?, note.source.as_bytes(), false)?;
            let mut created = note.clone(); created.path = filename;
            store.data.notes.push(created);
        }
    }
    for note in &mut store.data.notes { if let Some(updated) = next.notes.iter().find(|n| n.id == note.id) { note.pinned = updated.pinned; } }
    store.data.active_id = next.active_id;
    rescan(store)?;
    store.revision += 1;
    Ok(store.snapshot())
}

fn activate_path(store: &mut DiskLibrary, path: &Path, revision: u64) -> Result<Snapshot> {
    if store.revision != revision { return Err("目录内容已更新，请稍后重试".into()); }
    let path = path.canonicalize().map_err(error)?;
    if path.parent() != Some(store.root.as_path()) || !allowed(&path) { return Err("只能切换到当前目录中的笔记".into()); }
    let mut next = store.clone();
    rescan(&mut next)?;
    next.data.active_id = next.data.notes.iter().find(|note| next.root.join(&note.path) == path).ok_or("无法读取新打开的文件")?.id.clone();
    next.revision += 1;
    let snapshot = next.snapshot(); *store = next;
    Ok(snapshot)
}

#[tauri::command]
pub fn workspace_activate_path(state: State<'_, Workspace>, window: tauri::WebviewWindow, path: String, revision: u64) -> Result<Snapshot> {
    let snapshot = activate_path(&mut state.documents.lock().map_err(error)?.get_mut(window.label()).ok_or("目录窗口不存在")?.store, Path::new(&path), revision)?;
    if let Some(note) = snapshot.library.notes.iter().find(|note| note.id == snapshot.library.active_id) { let _ = window.set_title(note.filename.as_deref().unwrap_or("Markdown")); }
    Ok(snapshot)
}

#[tauri::command]
pub fn workspace_commit(state: State<'_, Workspace>, window: tauri::WebviewWindow, next: Library, revision: u64) -> Result<Snapshot> {
    let snapshot = commit(&mut state.documents.lock().map_err(error)?.get_mut(window.label()).ok_or("文档窗口不存在")?.store, next, revision)?;
    if let Some(note) = snapshot.library.notes.iter().find(|note| note.id == snapshot.library.active_id) { let _ = window.set_title(note.filename.as_deref().unwrap_or("Markdown")); }
    Ok(snapshot)
}

#[tauri::command]
pub fn workspace_select(app: tauri::AppHandle, state: State<'_, Workspace>, window: tauri::WebviewWindow, note_id: String, revision: u64) -> Result<Option<Snapshot>> {
    let mut documents = state.documents.lock().map_err(error)?;
    let store = &documents.get(window.label()).ok_or("文档窗口不存在")?.store;
    let note = store.data.notes.iter().find(|note| note.id == note_id).ok_or("笔记不存在")?;
    let owner = document_owner(&documents, &store.root.join(&note.path), window.label());
    if let Some(label) = owner {
        drop(documents);
        #[cfg(desktop)]
        focus(&app, &label)?;
        #[cfg(mobile)]
        let _ = (app, label);
        return Ok(None);
    }
    let store = &mut documents.get_mut(window.label()).unwrap().store;
    let mut next = store.data.clone(); next.active_id = note_id;
    let snapshot = commit(store, next, revision)?;
    drop(documents);
    if let Some(note) = snapshot.library.notes.iter().find(|note| note.id == snapshot.library.active_id) { let _ = window.set_title(note.filename.as_deref().unwrap_or("Markdown")); }
    Ok(Some(snapshot))
}

#[tauri::command]
pub fn workspace_reconcile(app: tauri::AppHandle, window: tauri::WebviewWindow, root: String, revision: u64, buffer: Option<external::EditorBuffer>) -> Result<external::Reconciled> {
    let state = app.state::<Workspace>();
    let mut documents = state.documents.lock().map_err(error)?;
    let store = &mut documents.get_mut(window.label()).ok_or("文档窗口不存在")?.store;
    if store.root != Path::new(&root) || store.revision != revision { return Err("外部工作区已切换".into()); }
    let mut next = store.clone(); rescan(&mut next)?;
    let mut backups = vec![];
    if let Some(buffer) = buffer {
        if let Some(old) = store.data.notes.iter().find(|n| n.id == buffer.note_id) {
            let current = next.data.notes.iter().find(|n| n.id == old.id);
            if current.is_none_or(|n| !external::same_text(&n.source, &old.source)) && !external::same_text(&buffer.source, &old.source) && !current.is_some_and(|n| external::same_text(&n.source, &buffer.source)) {
                let directory = app.path().app_data_dir().map_err(error)?.join("external-conflicts");
                fs::create_dir_all(&directory).map_err(error)?;
                let backup = directory.join(format!("{}-{}.md", now() as u64, id()));
                atomic(&backup, buffer.source.as_bytes(), false)?;
                backups.push(backup.to_string_lossy().into());
            }
        }
    }
    if next.data == store.data { return Ok(external::Reconciled { snapshot: None, backups }); }
    next.revision += 1; let snapshot = next.snapshot(); *store = next;
    Ok(external::Reconciled { snapshot: Some(snapshot), backups })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(desktop)]
    #[test]
    fn directory_owner_reuses_siblings_but_not_other_directories() {
        let first = tempfile::tempdir().unwrap(); let second = tempfile::tempdir().unwrap();
        fs::write(first.path().join("a.md"), "A").unwrap(); fs::write(first.path().join("b.md"), "B").unwrap();
        fs::write(second.path().join("c.md"), "C").unwrap();
        let make = |root: &Path, filename: &str| Document { store: open_file(&root.join(filename)).unwrap(), pending: vec![], _watch: external::Watch::start(root.to_path_buf(), false, |_| {}).unwrap() };
        let mut docs = HashMap::from([("first".into(), make(first.path(), "a.md"))]);
        assert_eq!(directory_owner(&docs, &first.path().join("b.md").canonicalize().unwrap()), Some("first".into()));
        assert_eq!(directory_owner(&docs, &second.path().join("c.md").canonicalize().unwrap()), None);
        docs.insert("second".into(), make(second.path(), "c.md"));
        assert_eq!(directory_owner(&docs, &second.path().join("c.md").canonicalize().unwrap()), Some("second".into()));
        assert_eq!(docs.len(), 2);
    }

    #[test]
    fn directory_switch_reads_new_files_without_writing_or_losing_the_old_selection_on_failure() {
        let dir = tempfile::tempdir().unwrap(); let root = dir.path();
        let a = root.join("a.md"); fs::write(&a, "saved A").unwrap();
        let mut store = open_file(&a).unwrap();
        let b = root.join("b.md"); fs::write(&b, "new B").unwrap();
        let snapshot = activate_path(&mut store, &b, 0).unwrap();
        assert_eq!(snapshot.library.notes.iter().find(|n| n.id == snapshot.library.active_id).unwrap().source, "new B");
        assert_eq!(fs::read_to_string(&a).unwrap(), "saved A"); assert!(!root.join(META).exists());
        let before = store.data.clone(); let other = tempfile::tempdir().unwrap(); fs::write(other.path().join("c.md"), "C").unwrap();
        assert!(activate_path(&mut store, &other.path().join("c.md"), 1).is_err()); assert_eq!(store.data, before);
        fs::remove_file(&b).unwrap(); assert!(activate_path(&mut store, &b, 1).is_err()); assert_eq!(store.data, before);
        assert!(activate_path(&mut store, &a, 0).is_err()); assert_eq!(store.data, before);
    }
    #[test]
    fn quit_requires_matching_acknowledgements_from_every_window() {
        let (send, receive) = mpsc::channel();
        let mut round = ExitRound { id: "quit-1".into(), pending: HashSet::from(["document-a".into(), "document-b".into()]), result: Some(send) };
        round.reply("stale", "document-a", true); round.reply("quit-1", "settings", true);
        assert_eq!(round.pending.len(), 2); assert!(receive.try_recv().is_err());
        round.reply("quit-1", "document-a", true); round.reply("quit-1", "document-a", true);
        assert_eq!(round.pending.len(), 1); assert!(receive.try_recv().is_err());
        round.reply("quit-1", "document-b", true); assert_eq!(receive.recv().unwrap(), Ok(()));
        let (send, receive) = mpsc::channel();
        let mut round = ExitRound { id: "quit-2".into(), pending: HashSet::from(["document-a".into()]), result: Some(send) };
        round.reply("quit-2", "document-a", false); assert!(receive.recv().unwrap().is_err());
        assert!(round.pending.contains("document-a"));
    }

    #[test]
    fn a_library_note_snapshot_selects_only_in_its_own_window() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("a.md"), "first note").unwrap();
        fs::write(directory.path().join("b.md"), "second note").unwrap();
        let store = DiskLibrary::open(directory.path().into(), None).unwrap();
        let active = &store.data.active_id;
        let other = store.data.notes.iter().find(|note| note.id != *active).unwrap();
        let snapshot = note_snapshot(&store, &other.id).unwrap();
        assert_eq!(snapshot.library.active_id, other.id);
        assert_eq!(snapshot.library.notes, store.data.notes);
        assert_eq!(snapshot.revision, store.revision);
        assert_eq!(note_snapshot(&store, active).unwrap().library.active_id, *active);
        assert!(note_snapshot(&store, "removed").is_err());
    }

    #[cfg(desktop)]
    #[test]
    fn windows_have_independent_stores_and_repeated_open_finds_the_current_owner() {
        let dir = tempfile::tempdir().unwrap(); let root = dir.path();
        fs::write(root.join("a.md"), "A").unwrap(); fs::write(root.join("b.md"), "B").unwrap();
        let make = |filename: &str| Document { store: open_file(&root.join(filename)).unwrap(), pending: vec![], _watch: external::Watch::start(root.to_path_buf(), false, |_| {}).unwrap() };
        let mut docs = HashMap::from([("document-a".into(), make("a.md")), ("document-b".into(), make("b.md"))]);
        let path = root.join("a.md").canonicalize().unwrap();
        assert_eq!(document_owner(&docs, &path, ""), Some("document-a".into()));
        assert_eq!(document_owner(&docs, &path, "document-a"), None);
        let first = &mut docs.get_mut("document-a").unwrap().store;
        let mut next = first.data.clone(); next.notes.iter_mut().find(|n| n.id == next.active_id).unwrap().source = "A edited".into();
        commit(first, next, 0).unwrap();
        let second = &mut docs.get_mut("document-b").unwrap().store;
        assert_eq!(second.revision, 0); assert_eq!(selected_path(second), Some(root.canonicalize().unwrap().join("b.md")));
        let mut stale = second.data.clone(); stale.notes.iter_mut().find(|n| n.filename.as_deref() == Some("a.md")).unwrap().source = "stale A edit".into();
        assert!(commit(second, stale, 0).is_err()); assert_eq!(fs::read_to_string(root.join("a.md")).unwrap(), "A edited");
        let mut next = second.data.clone(); next.notes.iter_mut().find(|n| n.id == next.active_id).unwrap().source = "B edited".into();
        commit(second, next, 0).unwrap();
        assert_eq!(fs::read_to_string(root.join("b.md")).unwrap(), "B edited"); assert!(!root.join(META).exists());
    }
    #[test]
    fn siblings_save_original_without_index_or_subdirectories() {
        let dir = tempfile::tempdir().unwrap(); let root = dir.path();
        fs::write(root.join("a.md"), b"# old\r\n").unwrap(); fs::write(root.join("b.markdown"), b"sibling").unwrap();
        fs::write(root.join(".hidden.md"), b"hidden").unwrap(); fs::create_dir(root.join("sub")).unwrap(); fs::write(root.join("sub/c.md"), b"child").unwrap();
        let mut book = open_file(&root.join("a.md")).unwrap(); assert_eq!(book.data.notes.len(), 2);
        let selected = book.data.active_id.clone(); let mut next = book.data.clone();
        next.notes.iter_mut().find(|n| n.id == selected).unwrap().source = "# changed".into();
        commit(&mut book, next, 0).unwrap(); assert_eq!(fs::read_to_string(root.join("a.md")).unwrap(), "# changed");
        assert!(!root.join(META).exists()); assert_eq!(fs::read_to_string(root.join("sub/c.md")).unwrap(), "child");
        let mut next = book.data.clone(); next.notes.iter_mut().find(|n| n.id == selected).unwrap().title = Some("renamed".into());
        commit(&mut book, next, 1).unwrap(); assert!(!root.join("a.md").exists()); assert_eq!(book.data.active_id, selected);
        assert_eq!(fs::read_to_string(root.join("renamed.md")).unwrap(), "# changed");
    }
    #[test]
    fn refuses_external_edits_deletion_path_escape_and_sync() {
        let dir = tempfile::tempdir().unwrap(); let path = dir.path().join("a.md"); fs::write(&path, "old").unwrap();
        let mut book = open_file(&path).unwrap(); let mut next = book.data.clone(); next.notes[0].source = "mine".into();
        fs::write(&path, "other").unwrap(); assert!(commit(&mut book, next.clone(), 0).is_err()); assert_eq!(fs::read_to_string(&path).unwrap(), "other");
        fs::remove_file(&path).unwrap(); assert!(commit(&mut book, next, 0).is_err()); assert!(!path.exists());
        let mut next = book.data.clone(); next.notes[0].path = "../escape.md".into(); assert!(commit(&mut book, next, 0).is_err());
        let mut next = book.data.clone(); next.sync = Some(serde_json::json!({})); assert!(commit(&mut book, next, 0).is_err());
        let mut next = book.data.clone(); next.notes.clear(); assert!(commit(&mut book, next, 0).is_err());
    }
    #[test]
    fn creation_and_rename_do_not_overwrite_siblings_and_rescan_keeps_identity() {
        let dir = tempfile::tempdir().unwrap(); let root = dir.path();
        fs::write(root.join("a.txt"), "a").unwrap(); fs::write(root.join("b.txt"), "b").unwrap();
        let mut book = open_file(&root.join("a.txt")).unwrap(); let active = book.data.active_id.clone();
        let mut next = book.data.clone(); next.notes.iter_mut().find(|n| n.id == active).unwrap().title = Some("b".into());
        assert!(commit(&mut book, next, 0).is_err()); assert_eq!(fs::read_to_string(root.join("b.txt")).unwrap(), "b");
        fs::write(root.join("new.md"), "added outside").unwrap(); rescan(&mut book).unwrap();
        assert_eq!(book.data.active_id, active); assert_eq!(book.data.notes.len(), 3);
        let mut next = book.data.clone(); let mut created = next.notes[0].clone();
        created.id = id(); created.title = Some("new".into()); created.source = "new note".into(); created.path.clear();
        next.active_id = created.id.clone(); next.notes.push(created);
        let result = commit(&mut book, next, 0).unwrap();
        assert_eq!(result.library.notes.len(), 4); assert_eq!(fs::read_to_string(root.join("new.md")).unwrap(), "added outside");
        assert_eq!(fs::read_to_string(root.join("new 1.md")).unwrap(), "new note");
        assert!(!root.join(META).exists());
    }
}
