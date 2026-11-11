//! Temporary, metadata-free sibling view. Never replaces the configured library.
use super::*;
use tauri::Emitter;

#[derive(Default)]
pub struct Workspace {
    store: Mutex<Option<DiskLibrary>>,
    requests: Mutex<Vec<PathBuf>>,
    #[cfg(desktop)]
    watch: Mutex<Option<external::Watch>>,
}

#[cfg(desktop)]
pub fn request(app: &tauri::AppHandle, paths: impl IntoIterator<Item = PathBuf>) {
    if let Ok(mut pending) = app.state::<Workspace>().requests.lock() {
        pending.extend(paths.into_iter().filter(|path| allowed(path)));
    }
    let _ = app.emit_to("main", "workspace-request", ());
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
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

#[tauri::command]
pub async fn workspace_open(app: tauri::AppHandle, window: tauri::WebviewWindow, path: Option<String>) -> Result<Option<Snapshot>> {
    main_window(&window)?;
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
        let store = open_file(&path)?;
        let snapshot = store.snapshot();
        let root = store.root.clone();
        let events = app.clone();
        let watch = external::Watch::start(root.clone(), false, move |result| {
            if let Err(error) = result { let _ = events.emit_to("main", "workspace-watch-error", serde_json::json!({"root": root, "error": error})); }
            else { let _ = events.emit_to("main", "workspace-files-changed", root.to_string_lossy()); }
        })?;
        let state = app.state::<Workspace>();
        *state.watch.lock().map_err(error)? = Some(watch);
        *state.store.lock().map_err(error)? = Some(store);
        Ok(Some(snapshot))
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

#[tauri::command]
pub fn workspace_commit(state: State<'_, Workspace>, window: tauri::WebviewWindow, next: Library, revision: u64) -> Result<Snapshot> {
    main_window(&window)?;
    commit(state.store.lock().map_err(error)?.as_mut().ok_or("尚未打开外部文件")?, next, revision)
}

#[tauri::command]
pub fn workspace_reconcile(app: tauri::AppHandle, window: tauri::WebviewWindow, root: String, revision: u64, buffer: Option<external::EditorBuffer>) -> Result<external::Reconciled> {
    main_window(&window)?;
    let state = app.state::<Workspace>();
    let mut lock = state.store.lock().map_err(error)?;
    let store = lock.as_mut().ok_or("尚未打开外部文件")?;
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

#[tauri::command]
pub fn workspace_close(state: State<'_, Workspace>, window: tauri::WebviewWindow) -> Result<()> {
    main_window(&window)?;
    #[cfg(desktop)]
    { *state.watch.lock().map_err(error)? = None; }
    *state.store.lock().map_err(error)? = None;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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
