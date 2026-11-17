use super::*;
use sha2::{Digest, Sha256};

const WINDOW_MS: u64 = 5 * 60 * 1000;
const MAX_LISTED: usize = 500;
pub(super) const RESTORE: &str = "restore-history";

fn directory(note_id: &str) -> String {
    format!("{META}/versions/{:x}", Sha256::digest(note_id.as_bytes()))
}

fn valid_version(version: &str) -> bool {
    let Some((time, sequence)) = version.strip_suffix(".md").and_then(|stem| stem.split_once('-')) else { return false };
    time.len() == 20 && time.bytes().all(|byte| byte.is_ascii_digit())
        && (sequence == "edit" || sequence.len() == 20 && sequence.bytes().all(|byte| byte.is_ascii_digit()))
}

pub(super) fn capture(root: &Path, note: &Note, intent: &str, generation: u64, time: u64) -> Result<Option<Operation>> {
    let dir = directory(&note.id);
    let version = if matches!(intent, RESTORE | "sync") {
        format!("{time:020}-{generation:020}.md")
    } else {
        format!("{:020}-edit.md", time / WINDOW_MS * WINDOW_MS)
    };
    let path = relative(&dir, &version);
    if checked(root, &path)?.exists() && intent != RESTORE { return Ok(None); }
    fs::create_dir_all(checked(root, &dir)?).map_err(error)?;
    Ok(Some(Operation::Create {
        path,
        bytes: note.source.as_bytes().to_vec(),
        staged: format!("{META}/staged-{}", id()),
        modified: note.updated_at,
        identity: None,
    }))
}

#[cfg(any(feature = "app", test))]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteVersion {
    id: String,
    updated_at: f64,
}

pub(super) fn archived_bytes(root: &Path) -> Result<u64> {
    let folders = match fs::read_dir(checked(root, ".miaoyan/versions")?) {
        Ok(entries) => entries,
        Err(reason) if reason.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(reason) => return Err(error(reason)),
    };
    let mut bytes = 0;
    for folder in folders {
        let folder = folder.map_err(error)?;
        if !folder.file_type().map_err(error)?.is_dir() || !folder.file_name().to_str().is_some_and(|name| name.len() == 64 && name.bytes().all(|byte| byte.is_ascii_hexdigit())) { continue; }
        for version in fs::read_dir(folder.path()).map_err(error)? {
            let version = version.map_err(error)?;
            if version.file_type().map_err(error)?.is_file() && version.file_name().to_str().is_some_and(valid_version) {
                bytes += version.metadata().map_err(error)?.len();
            }
        }
    }
    Ok(bytes)
}

#[cfg(any(feature = "app", test))]
impl DiskLibrary {
    fn history_note(&self, note_id: &str) -> Result<&Note> {
        self.data.notes.iter().find(|note| note.id == note_id && !note.trashed).ok_or("笔记不存在或已移到废纸篓".into())
    }

    fn note_versions(&self, note_id: &str) -> Result<Vec<NoteVersion>> {
        self.history_note(note_id)?;
        let dir = checked(&self.root, &directory(note_id))?;
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(reason) if reason.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(reason) => return Err(error(reason)),
        };
        let mut versions = Vec::new();
        for entry in entries {
            let entry = entry.map_err(error)?;
            let id = entry.file_name().to_string_lossy().into_owned();
            if !valid_version(&id) || !entry.file_type().map_err(error)?.is_file() { continue; }
            let updated_at = millis(entry.metadata().and_then(|metadata| metadata.modified()).map_err(error)?);
            versions.push(NoteVersion { id, updated_at });
        }
        versions.sort_by(|a, b| b.updated_at.total_cmp(&a.updated_at).then_with(|| b.id.cmp(&a.id)));
        versions.truncate(MAX_LISTED);
        Ok(versions)
    }

    fn note_version_source(&self, note_id: &str, version: &str) -> Result<String> {
        self.history_note(note_id)?;
        if !valid_version(version) { return Err("历史版本无效".into()); }
        fs::read_to_string(checked(&self.root, &relative(&directory(note_id), version))?).map_err(error)
    }
}

#[cfg(feature = "app")]
#[tauri::command]
pub fn library_note_versions(window: tauri::WebviewWindow, state: State<'_, FileLibrary>, note_id: String) -> Result<Vec<NoteVersion>> {
    let lock = state.0.lock().map_err(error)?;
    let store = lock.as_ref().ok_or("请先打开笔记库")?;
    workspace::check_library_window(window.app_handle(), window.label(), &store.root)?;
    store.note_versions(&note_id)
}

#[cfg(feature = "app")]
#[tauri::command]
pub fn library_note_version(window: tauri::WebviewWindow, state: State<'_, FileLibrary>, note_id: String, version: String) -> Result<String> {
    let lock = state.0.lock().map_err(error)?;
    let store = lock.as_ref().ok_or("请先打开笔记库")?;
    workspace::check_library_window(window.app_handle(), window.label(), &store.root)?;
    store.note_version_source(&note_id, &version)
}

#[cfg(feature = "app")]
#[tauri::command]
pub async fn library_restore_note_version(
    window: tauri::WebviewWindow, state: State<'_, FileLibrary>, root: String,
    note_id: String, version: String, expected_source: String,
) -> Result<Snapshot> {
    let mut lock = state.0.lock().map_err(error)?;
    let store = lock.as_mut().ok_or("请先打开笔记库")?;
    workspace::check_library_window(window.app_handle(), window.label(), &store.root)?;
    let source = store.note_version_source(&note_id, &version)?;
    store.save_note_body(&root, &note_id, &expected_source, source, RESTORE)?;
    let snapshot = store.snapshot();
    drop(lock);
    workspace::library_changed(window.app_handle(), &snapshot.root, Some(window.label()));
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library() -> (tempfile::TempDir, DiskLibrary, Note) {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("笔记.md"), "# 初稿\r\n\r\n保留中文。\r\n").unwrap();
        let store = DiskLibrary::open(temp.path().into(), None).unwrap();
        let note = store.data.notes[0].clone();
        (temp, store, note)
    }

    #[test]
    fn versions_persist_across_reopen_and_restore_without_losing_the_current_version() {
        let (temp, mut store, note) = library();
        let root = store.snapshot().root;
        assert!(store.note_versions(&note.id).unwrap().is_empty());
        let latest = "# 修改后\n\n含代码与中文。\n";
        store.save_note_body(&root, &note.id, &note.source, latest.into(), "edit").unwrap();
        store = DiskLibrary::open(temp.path().into(), None).unwrap();
        let versions = store.note_versions(&note.id).unwrap();
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].updated_at, note.updated_at);
        assert_eq!(archived_bytes(temp.path()).unwrap(), note.source.len() as u64);
        let saved = store.note_version_source(&note.id, &versions[0].id).unwrap();
        assert_eq!(saved, note.source);
        assert!(store.save_note_body(&root, &note.id, "并发已改", saved.clone(), RESTORE).is_err());
        assert_eq!(store.note_versions(&note.id).unwrap().len(), 1);
        store.save_note_body(&root, &note.id, latest, saved, RESTORE).unwrap();
        assert_eq!(fs::read_to_string(temp.path().join(note.path)).unwrap(), note.source);
        let versions = store.note_versions(&note.id).unwrap();
        assert_eq!(versions.len(), 2);
        assert_eq!(store.note_version_source(&note.id, &versions[0].id).unwrap(), latest);
        assert!(store.note_version_source(&note.id, "../笔记.md").is_err());
        assert_eq!(store.data.notes.len(), 1);
        assert_eq!(archived_bytes(temp.path()).unwrap(), (note.source.len() + latest.len()) as u64);
    }

    #[test]
    fn failed_saves_leave_no_phantom_version_and_edits_share_one_file_per_window() {
        let (temp, mut store, note) = library();
        let root = store.snapshot().root;
        let time = WINDOW_MS * 2 + 1;
        let disk = temp.path().join(&note.path);
        fs::write(&disk, "外部写入").unwrap();
        assert!(store.save_note_body(&root, &note.id, &note.source, "本地编辑".into(), "edit").is_err());
        assert!(store.note_versions(&note.id).unwrap().is_empty());
        assert_eq!(fs::read_to_string(disk).unwrap(), "外部写入");

        let Operation::Create { path, bytes, .. } = capture(temp.path(), &note, "edit", 1, time).unwrap().unwrap() else { panic!("expected staged version") };
        fs::write(temp.path().join(path), bytes).unwrap();
        assert!(capture(temp.path(), &note, "edit", 2, time + 1).unwrap().is_none());
        assert!(capture(temp.path(), &note, "edit", 3, time + WINDOW_MS).unwrap().is_some());
        assert!(capture(temp.path(), &note, RESTORE, 4, time + 1).unwrap().is_some());
    }
}
