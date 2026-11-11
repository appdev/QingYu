use super::*;

pub(super) const FOLDER_JOURNAL: &str = ".miaoyan/folder-transaction.json";

#[derive(Serialize, Deserialize)]
struct FolderTransaction {
    generation: u64,
    from: String,
    to: String,
    identity: [u64; 2],
    creating: bool,
    trash: bool,
}

fn directory_identity(path: &Path) -> Result<[u64; 2]> {
    let metadata = fs::symlink_metadata(path).map_err(error)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("请选择真实文件夹".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok([metadata.dev(), metadata.ino()])
    }
    #[cfg(not(unix))]
    Err("当前平台尚未接入可恢复的文件夹移动".into())
}

fn folder_name(value: &str) -> Result<&str> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 240
        || value.starts_with('.')
        || value
            .chars()
            .any(|c| c.is_control() || c == '/' || c == '\\' || c == ':')
        || ["Trash", "i", "files"]
            .iter()
            .any(|name| value.eq_ignore_ascii_case(name))
        || Path::new(value).extension().is_some_and(|ext| {
            ["app", "bundle", "pkg"]
                .iter()
                .any(|x| ext.eq_ignore_ascii_case(x))
        })
    {
        return Err("文件夹名称无效，不能使用隐藏、附件、废纸篓或应用包名称".into());
    }
    Ok(value)
}

fn visible_path(root: &Path, path: &str) -> Result<PathBuf> {
    let absolute = checked(root, path)?;
    for component in Path::new(path).components() {
        folder_name(&component.as_os_str().to_string_lossy())?;
    }
    Ok(absolute)
}

fn move_directory(from: &Path, to: &Path) -> Result<()> {
    #[cfg(target_vendor = "apple")]
    {
        move_exclusive(from, to)
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::ffi::OsStrExt;
        let from = std::ffi::CString::new(from.as_os_str().as_bytes()).map_err(error)?;
        let to = std::ffi::CString::new(to.as_os_str().as_bytes()).map_err(error)?;
        if unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                from.as_ptr(),
                libc::AT_FDCWD,
                to.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        } == 0
        {
            Ok(())
        } else {
            Err(error(io::Error::last_os_error()))
        }
    }
    #[cfg(not(any(target_vendor = "apple", target_os = "linux")))]
    {
        let _ = (from, to);
        Err("当前平台尚未接入可恢复的文件夹移动".into())
    }
}

impl FolderTransaction {
    fn paths(&self, root: &Path, trash_root: Option<&Path>) -> Result<(PathBuf, PathBuf)> {
        let from = if self.creating {
            let token = self
                .from
                .strip_prefix(".miaoyan/folder-")
                .ok_or("文件夹事务暂存路径无效")?;
            Uuid::parse_str(token).map_err(error)?;
            checked(root, &self.from)?
        } else {
            visible_path(root, &self.from)?
        };
        let to = if self.trash {
            if self.creating || Path::new(&self.to).components().count() != 1 {
                return Err("废纸篓事务路径无效".into());
            }
            checked(trash_root.ok_or("系统废纸篓不可用，未移动文件")?, &self.to)?
        } else {
            visible_path(root, &self.to)?
        };
        if to.starts_with(&from) && to != from {
            return Err("不能将文件夹移入自身或其子文件夹".into());
        }
        Ok((from, to))
    }
    fn undo(&self, root: &Path, trash_root: Option<&Path>) -> Result<()> {
        let (from, to) = self.paths(root, trash_root)?;
        if directory_identity(&to).ok() == Some(self.identity) {
            if from.exists() && !same_file(&from, &to) {
                return Err("恢复路径已被占用，已保留目录和事务日志".into());
            }
            move_directory(&to, &from)?;
        } else if directory_identity(&from).ok() != Some(self.identity) {
            return Err("恢复时目录身份发生变化，已保留文件和事务日志".into());
        }
        if self.creating {
            fs::remove_dir(from).map_err(error)?;
        }
        Ok(())
    }
}

pub(super) fn recover(root: &Path) -> Result<()> {
    let journal = checked(root, FOLDER_JOURNAL)?;
    if !journal.exists() {
        return Ok(());
    }
    let transaction: FolderTransaction =
        serde_json::from_slice(&fs::read(&journal).map_err(error)?).map_err(error)?;
    let index: Index =
        serde_json::from_slice(&fs::read(checked(root, INDEX)?).map_err(error)?).map_err(error)?;
    if transaction.generation == 0
        || transaction.generation != index.generation
            && transaction.generation != index.generation.saturating_add(1)
    {
        return Err("文件夹事务版本不匹配，未修改目录".into());
    }
    let trash = if transaction.trash {
        Some(system_trash(root)?)
    } else {
        None
    };
    transaction.paths(root, trash.as_deref())?;
    if transaction.generation != index.generation {
        transaction.undo(root, trash.as_deref())?;
    }
    fs::remove_file(journal).map_err(error)
}

impl DiskLibrary {
    fn folder(&self, folder_id: &str, protected: bool) -> Result<&Folder> {
        let folder = self
            .data
            .folders
            .iter()
            .find(|folder| folder.id == folder_id)
            .ok_or("文件夹不存在")?;
        if protected && (folder.path.is_empty() || folder.id == "default") {
            return Err("不能修改笔记库根文件夹".into());
        }
        if !folder.path.is_empty() {
            directory_identity(&visible_path(&self.root, &folder.path)?)?;
        }
        Ok(folder)
    }

    pub(super) fn folder_change(
        &mut self,
        action: &str,
        folder_id: &str,
        value: &str,
        trash_root: Option<&Path>,
    ) -> Result<()> {
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(checked(&self.root, ".miaoyan/write.lock")?)
            .map_err(error)?;
        try_lock(&lock).map_err(|_| "笔记库正在被其他操作写入")?;
        let index: Index =
            serde_json::from_slice(&fs::read(checked(&self.root, INDEX)?).map_err(error)?)
                .map_err(error)?;
        if index.generation != self.revision
            || checked(&self.root, JOURNAL)?.exists()
            || checked(&self.root, FOLDER_JOURNAL)?.exists()
        {
            return Err("笔记库已变化或有未恢复事务，请重新读取笔记文件夹".into());
        }
        let mut next = self.data.clone();
        let folder = self.folder(folder_id, action != "create")?.clone();
        let (from, to, creating, trash) = match action {
            "create" => {
                let to = relative(&folder.path, folder_name(value)?);
                if checked(&self.root, &to)?.exists() {
                    return Err("已有同名文件夹或文件，未覆盖".into());
                }
                let from = format!(".miaoyan/folder-{}", id());
                fs::create_dir(checked(&self.root, &from)?).map_err(error)?;
                next.folders.push(Folder {
                    id: id(),
                    name: value.trim().into(),
                    path: to.clone(),
                });
                (from, to, true, false)
            }
            "rename" | "move" => {
                let to = if action == "rename" {
                    relative(
                        Path::new(&folder.path)
                            .parent()
                            .unwrap_or(Path::new(""))
                            .to_str()
                            .ok_or("目录名称无效")?,
                        folder_name(value)?,
                    )
                } else {
                    let chosen = Path::new(value);
                    if let Ok(inside) = chosen.strip_prefix(&self.root) {
                        if !inside.as_os_str().is_empty() {
                            checked(&self.root, inside.to_str().ok_or("目录名称无效")?)?;
                        }
                    }
                    let destination = fs::canonicalize(chosen).map_err(error)?;
                    // Symlinks and hidden/attachment/package destinations are not library folders.
                    let relative_path = destination
                        .strip_prefix(&self.root)
                        .map_err(|_| "请选择同一笔记库中的文件夹")?;
                    let destination = relative_path.to_str().ok_or("目录名称无效")?;
                    if !self.data.folders.iter().any(|f| f.path == destination) {
                        return Err("请选择同一笔记库中的笔记文件夹".into());
                    }
                    if !destination.is_empty() {
                        directory_identity(&visible_path(&self.root, destination)?)?;
                    }
                    relative(
                        destination,
                        Path::new(&folder.path)
                            .file_name()
                            .unwrap()
                            .to_str()
                            .ok_or("目录名称无效")?,
                    )
                };
                if to == folder.path {
                    return Ok(());
                }
                let target = visible_path(&self.root, &to)?;
                let source = visible_path(&self.root, &folder.path)?;
                if target.starts_with(&source)
                    || target
                        .parent()
                        .is_some_and(|parent| same_file(parent, &source))
                {
                    return Err("不能将文件夹移入自身或其子文件夹".into());
                }
                if target.exists()
                    && !(to.to_lowercase() == folder.path.to_lowercase()
                        && same_file(&source, &target))
                {
                    return Err("目标已有同名文件夹或文件，未覆盖".into());
                }
                for f in &mut next.folders {
                    if f.path == folder.path {
                        f.path = to.clone();
                        f.name = Path::new(&to).file_name().unwrap().to_string_lossy().into();
                    } else if let Some(suffix) = f.path.strip_prefix(&(folder.path.clone() + "/")) {
                        f.path = relative(&to, suffix);
                    }
                }
                for note in &mut next.notes {
                    if let Some(suffix) = note.path.strip_prefix(&(folder.path.clone() + "/")) {
                        note.path = relative(&to, suffix);
                    }
                }
                (folder.path, to, false, false)
            }
            "trash" => {
                let root = trash_root.ok_or("系统废纸篓不可用，未移动目录")?;
                directory_identity(root)?;
                let base = Path::new(&folder.path)
                    .file_name()
                    .unwrap()
                    .to_string_lossy();
                let mut to = base.to_string();
                let mut suffix = 0;
                while root.join(&to).exists() {
                    to = format!("{base}_{suffix}");
                    suffix += 1;
                }
                let removed: HashSet<_> = next
                    .folders
                    .iter()
                    .filter(|f| {
                        f.path == folder.path || f.path.starts_with(&(folder.path.clone() + "/"))
                    })
                    .map(|f| f.id.clone())
                    .collect();
                next.folders.retain(|f| !removed.contains(&f.id));
                next.notes
                    .retain(|n| !n.path.starts_with(&(folder.path.clone() + "/")));
                for note in &mut next.notes {
                    if removed.contains(&note.folder_id) {
                        note.folder_id = "default".into();
                    }
                }
                if !next.notes.iter().any(|n| n.id == next.active_id) {
                    next.active_id = next
                        .notes
                        .iter()
                        .find(|n| !n.trashed)
                        .or_else(|| next.notes.first())
                        .map_or(String::new(), |n| n.id.clone());
                }
                (folder.path, to, false, true)
            }
            _ => return Err("未知文件夹操作".into()),
        };
        let transaction = FolderTransaction {
            generation: self.revision + 1,
            identity: directory_identity(&checked(&self.root, &from)?)?,
            from,
            to,
            creating,
            trash,
        };
        let journal = checked(&self.root, FOLDER_JOURNAL)?;
        let (source, destination) = transaction.paths(&self.root, trash_root)?;
        if let Err(reason) = atomic(
            &journal,
            &serde_json::to_vec(&transaction).map_err(error)?,
            false,
        ) {
            if creating {
                fs::remove_dir(source).map_err(error)?;
            }
            return Err(reason);
        }
        let result = (|| {
            if directory_identity(&source)? != transaction.identity {
                return Err("目录已被外部替换，未移动".into());
            }
            move_directory(&source, &destination)?;
            self.persist_index(&next, transaction.generation)
        })();
        if let Err(reason) = result {
            transaction
                .undo(&self.root, trash_root)
                .map_err(|recovery| format!("{reason}；{recovery}"))?;
            fs::remove_file(journal).map_err(error)?;
            return Err(reason);
        }
        self.data = next;
        self.revision = transaction.generation;
        self.remember_folder_identities();
        self.bytes
            .retain(|id, _| self.data.notes.iter().any(|note| note.id == *id));
        fs::remove_file(journal).map_err(error)
    }
}

#[cfg(all(feature = "app", target_os = "macos"))]
fn system_trash(root: &Path) -> Result<PathBuf> {
    use objc2_foundation::{
        NSFileManager, NSSearchPathDirectory, NSSearchPathDomainMask, NSString, NSURL,
    };
    let url =
        NSURL::fileURLWithPath_isDirectory(&NSString::from_str(&root.to_string_lossy()), true);
    let trash = NSFileManager::defaultManager()
        .URLForDirectory_inDomain_appropriateForURL_create_error(
            NSSearchPathDirectory::TrashDirectory,
            NSSearchPathDomainMask::AllDomainsMask,
            Some(&url),
            false,
        )
        .map_err(error)?;
    let path = PathBuf::from(trash.path().ok_or("系统废纸篓路径不可用")?.to_string());
    directory_identity(&path)?;
    Ok(path)
}
#[cfg(not(all(feature = "app", target_os = "macos")))]
fn system_trash(_root: &Path) -> Result<PathBuf> {
    Err("当前平台尚未接入系统废纸篓，未删除文件".into())
}

#[cfg(feature = "app")]
#[tauri::command]
pub async fn library_folder_action(
    window: tauri::WebviewWindow,
    state: State<'_, FileLibrary>,
    action: String,
    folder_id: Option<String>,
    revision: u64,
    value: Option<String>,
) -> Result<Snapshot> {
    if window.label() != "main" {
        return Err("请在主窗口操作文件夹".into());
    }
    #[cfg(desktop)]
    {
        #[cfg(not(target_os = "macos"))]
        use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
        let (root, folder) = {
            let lock = state.0.lock().map_err(error)?;
            let store = lock.as_ref().ok_or("请先打开笔记库")?;
            if store.revision != revision {
                return Err("笔记库已变化，请重试".into());
            }
            let id = folder_id
                .as_deref()
                .filter(|id| *id != "trash" || action != "reveal")
                .unwrap_or("default");
            (
                store.root.clone(),
                store
                    .folder(
                        id,
                        !["create", "reveal", "terminal"].contains(&action.as_str()),
                    )?
                    .clone(),
            )
        };
        let expected_root = root.clone();
        let mut value = value.unwrap_or_default();
        if action == "move" {
            #[cfg(target_os = "macos")]
            let chosen = crate::note_ui::choose_folder_destination(
                window.clone(),
                root.to_string_lossy().into_owned(),
            )
            .await?;
            #[cfg(not(target_os = "macos"))]
            let chosen = tauri::async_runtime::spawn_blocking(move || {
                window
                    .dialog()
                    .file()
                    .set_parent(&window)
                    .set_title("移动文件夹")
                    .set_directory(&root)
                    .blocking_pick_folder()
            })
            .await
            .map_err(error)?
            .map(|path| {
                path.into_path()
                    .map_err(error)
                    .map(|path| path.to_string_lossy().into_owned())
            })
            .transpose()?;
            let Some(path) = chosen else {
                return Ok(state
                    .0
                    .lock()
                    .map_err(error)?
                    .as_ref()
                    .ok_or("笔记库未打开")?
                    .snapshot());
            };
            value = path;
        } else if action == "trash" {
            #[cfg(target_os = "macos")]
            let confirmed =
                crate::note_ui::confirm_folder_trash(window.clone(), folder.name.clone()).await?;
            #[cfg(not(target_os = "macos"))]
            let confirmed = tauri::async_runtime::spawn_blocking(move || {
                window
                    .dialog()
                    .message(format!(
                        "将文件夹“{}”及其中的所有内容移到废纸篓？",
                        folder.name
                    ))
                    .title("移到废纸篓")
                    .parent(&window)
                    .buttons(MessageDialogButtons::OkCancelCustom(
                        "移到废纸篓".into(),
                        "取消".into(),
                    ))
                    .blocking_show()
            })
            .await
            .map_err(error)?;
            if !confirmed {
                return Ok(state
                    .0
                    .lock()
                    .map_err(error)?
                    .as_ref()
                    .ok_or("笔记库未打开")?
                    .snapshot());
            }
        }
        let mut lock = state.0.lock().map_err(error)?;
        let store = lock.as_mut().ok_or("请先打开笔记库")?;
        if store.revision != revision || store.root != expected_root {
            return Err("操作期间笔记库已变化，未修改文件夹，请重试".into());
        }
        if ["reveal", "terminal"].contains(&action.as_str()) {
            let path = if folder_id.as_deref() == Some("trash") {
                if action == "terminal" {
                    return Err("不能在废纸篓中打开终端".into());
                }
                let path = checked(&store.root, "Trash")?;
                if !path.exists() {
                    fs::create_dir(&path).map_err(error)?;
                }
                path
            } else if folder.path.is_empty() {
                store.root.clone()
            } else {
                visible_path(&store.root, &folder.path)?
            };
            open_folder(&path, action == "terminal")?;
        } else {
            let trash = if action == "trash" {
                Some(system_trash(&store.root)?)
            } else {
                None
            };
            store.folder_change(
                &action,
                folder_id.as_deref().unwrap_or("default"),
                &value,
                trash.as_deref(),
            )?;
        }
        Ok(store.snapshot())
    }
    #[cfg(mobile)]
    {
        let _ = (window, state, action, folder_id, revision, value);
        Err("请在桌面端管理文件夹".into())
    }
}

#[cfg(all(feature = "app", target_os = "macos"))]
fn open_folder(path: &Path, terminal: bool) -> Result<()> {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::NSString;
    let mut process = std::process::Command::new("/usr/bin/open");
    if terminal {
        let workspace = NSWorkspace::sharedWorkspace();
        let bundle = [
            "fun.tw93.kaku",
            "dev.warp.Warp-Stable",
            "com.googlecode.iterm2",
            "net.kovidgoyal.kitty",
            "com.apple.Terminal",
        ]
        .into_iter()
        .find(|id| {
            workspace
                .URLForApplicationWithBundleIdentifier(&NSString::from_str(id))
                .is_some()
        })
        .unwrap_or("com.apple.Terminal");
        process.args(["-b", bundle]);
    } else {
        process.arg("-R");
    }
    let result = process.arg(path).status().map_err(error)?;
    if result.success() {
        Ok(())
    } else {
        Err("系统未能打开文件夹".into())
    }
}
#[cfg(all(feature = "app", not(target_os = "macos")))]
fn open_folder(_path: &Path, _terminal: bool) -> Result<()> {
    Err("当前平台尚未接入文件管理器和终端".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn folder_id(store: &DiskLibrary, path: &str) -> String {
        store
            .data
            .folders
            .iter()
            .find(|f| f.path == path)
            .unwrap()
            .id
            .clone()
    }

    #[test]
    fn folder_contract_nested_create_rename_move_preserves_identity_bytes_and_attachments() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = DiskLibrary::open(temp.path().into(), None).unwrap();
        store
            .folder_change("create", "default", "故事", None)
            .unwrap();
        let parent = folder_id(&store, "故事");
        store
            .folder_change("create", &parent, "子目录", None)
            .unwrap();
        fs::write(
            temp.path().join("故事/子目录/原文.md"),
            b"# CRLF\r\nbody\r\n",
        )
        .unwrap();
        fs::create_dir_all(temp.path().join("故事/i")).unwrap();
        fs::write(temp.path().join("故事/i/image.bin"), [0, 255, 7]).unwrap();
        fs::write(temp.path().join("故事/.untracked"), "external").unwrap();
        store = DiskLibrary::open(temp.path().into(), None).unwrap();
        let note = store.data.notes[0].clone();
        let child = folder_id(&store, "故事/子目录");
        store
            .folder_change("rename", &parent, "STORY", None)
            .unwrap();
        store
            .folder_change("rename", &parent, "story", None)
            .unwrap();
        store
            .folder_change("create", "default", "目的地", None)
            .unwrap();
        store
            .folder_change(
                "move",
                &parent,
                store.root.join("目的地").to_str().unwrap(),
                None,
            )
            .unwrap();
        let moved = store.data.notes.iter().find(|n| n.id == note.id).unwrap();
        assert_eq!(moved.path, "目的地/story/子目录/原文.md");
        assert_eq!(moved.folder_id, child);
        assert_eq!(moved.source, note.source);
        assert_eq!(moved.updated_at, note.updated_at);
        assert_eq!(
            fs::read(temp.path().join("目的地/story/i/image.bin")).unwrap(),
            [0, 255, 7]
        );
        assert_eq!(
            fs::read(temp.path().join(&moved.path)).unwrap(),
            b"# CRLF\r\nbody\r\n"
        );
        assert_eq!(
            fs::read_to_string(temp.path().join("目的地/story/.untracked")).unwrap(),
            "external"
        );
        let reopened = DiskLibrary::open(temp.path().into(), None).unwrap();
        assert_eq!(folder_id(&reopened, "目的地/story"), parent);
        assert_eq!(reopened.data.notes[0].id, note.id);
    }

    #[test]
    fn folder_contract_rejects_protected_conflicting_descendant_external_and_symlink_paths() {
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let mut store = DiskLibrary::open(temp.path().into(), None).unwrap();
        for name in ["A", "B"] {
            store
                .folder_change("create", "default", name, None)
                .unwrap();
        }
        let a = folder_id(&store, "A");
        store.folder_change("create", &a, "child", None).unwrap();
        let before = serde_json::to_vec(&store.data).unwrap();
        for (action, id, value) in [
            ("rename", "default", "root"),
            ("trash", "default", ""),
            ("rename", a.as_str(), "B"),
            ("create", a.as_str(), "../outside"),
            ("create", a.as_str(), "files"),
            (
                "move",
                a.as_str(),
                temp.path().join("A/child").to_str().unwrap(),
            ),
            ("move", a.as_str(), outside.path().to_str().unwrap()),
        ] {
            assert!(
                store.folder_change(action, id, value, None).is_err(),
                "{action}: {value}"
            );
            assert_eq!(serde_json::to_vec(&store.data).unwrap(), before);
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(outside.path(), temp.path().join("link")).unwrap();
            assert!(store
                .folder_change("move", &a, temp.path().join("link").to_str().unwrap(), None)
                .is_err());
        }
        assert!(temp.path().join("A/child").is_dir());
        assert!(temp.path().join("B").is_dir());
    }

    #[test]
    fn folder_contract_trashes_whole_directory_keeps_unrelated_and_already_trashed_notes() {
        let temp = tempfile::tempdir().unwrap();
        let trash = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("A/sub")).unwrap();
        fs::create_dir_all(temp.path().join("A/files")).unwrap();
        fs::write(temp.path().join("A/sub/n.md"), "nested").unwrap();
        fs::write(temp.path().join("A/files/image"), [1, 2, 3]).unwrap();
        fs::write(temp.path().join("root.md"), "root").unwrap();
        let mut store = DiskLibrary::open(temp.path().into(), None).unwrap();
        let a = folder_id(&store, "A");
        let nested = store
            .data
            .notes
            .iter()
            .find(|n| n.path == "A/sub/n.md")
            .unwrap()
            .clone();
        let mut next = store.data.clone();
        let mut old = nested.clone();
        old.id = id();
        old.path.clear();
        old.trashed = true;
        next.notes.push(old.clone());
        store.commit(next, "create").unwrap();
        store
            .folder_change("trash", &a, "", Some(trash.path()))
            .unwrap();
        assert!(!temp.path().join("A").exists());
        assert_eq!(
            fs::read_to_string(trash.path().join("A/sub/n.md")).unwrap(),
            "nested"
        );
        assert_eq!(
            fs::read(trash.path().join("A/files/image")).unwrap(),
            [1, 2, 3]
        );
        assert!(store.data.notes.iter().any(|n| n.path == "root.md"));
        assert!(!store.data.notes.iter().any(|n| n.id == nested.id));
        assert!(store
            .data
            .notes
            .iter()
            .any(|n| n.id == old.id && n.trashed && n.folder_id == "default"));
        assert!(store
            .data
            .notes
            .iter()
            .any(|n| n.id == store.data.active_id));
    }

    #[test]
    fn folder_contract_recovers_interrupted_move_and_leaves_external_collision_untouched() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("A")).unwrap();
        fs::write(temp.path().join("A/n.md"), "original").unwrap();
        let store = DiskLibrary::open(temp.path().into(), None).unwrap();
        let transaction = FolderTransaction {
            generation: store.revision + 1,
            from: "A".into(),
            to: "B".into(),
            identity: directory_identity(&temp.path().join("A")).unwrap(),
            creating: false,
            trash: false,
        };
        fs::write(
            temp.path().join(FOLDER_JOURNAL),
            serde_json::to_vec(&transaction).unwrap(),
        )
        .unwrap();
        move_directory(&temp.path().join("A"), &temp.path().join("B")).unwrap();
        fs::create_dir(temp.path().join("A")).unwrap();
        fs::write(temp.path().join("A/external"), "keep").unwrap();
        assert!(DiskLibrary::open(temp.path().into(), None).is_err());
        assert_eq!(
            fs::read_to_string(temp.path().join("A/external")).unwrap(),
            "keep"
        );
        assert!(temp.path().join(FOLDER_JOURNAL).exists());
        fs::remove_file(temp.path().join("A/external")).unwrap();
        fs::remove_dir(temp.path().join("A")).unwrap();
        let recovered = DiskLibrary::open(temp.path().into(), None).unwrap();
        assert_eq!(recovered.data.notes[0].source, "original");
        assert!(temp.path().join("A/n.md").is_file());
        assert!(!temp.path().join("B").exists());
    }
}
