//! Note-relative resources: one boundary for native previews, opening and the Web API.
use super::*;
pub(super) const MAX_FILE: u64 = assets::MAX_FILE as u64;

pub(super) fn resolve(store: &DiskLibrary, id: &str, value: &str) -> Result<PathBuf> {
    let relative = relative_path(store, id, value)?;
    let path = checked(&store.root, &relative)?;
    let meta = fs::metadata(&path).map_err(|_| "附件不存在，可能尚未完成同步")?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        return Err("附件不可读取或超过 64 MiB".into());
    }
    Ok(path)
}

pub(super) fn relative_path(store: &DiskLibrary, id: &str, value: &str) -> Result<String> {
    let note = store
        .data
        .notes
        .iter()
        .find(|note| note.id == id)
        .ok_or("笔记不存在")?;
    if value.len() > 4096 || value.chars().any(char::is_control) || value.contains(['\\', ':']) {
        return Err("附件路径无效".into());
    }
    // MiaoYan's historical /i/ and /files/ spellings are relative to the note folder.
    let value = if value.starts_with("/i/") || value.starts_with("/files/") {
        &value[1..]
    } else {
        value
    };
    if value.starts_with('/') {
        return Err("附件必须位于笔记库内".into());
    }
    let mut parts: Vec<_> = Path::new(&note.path)
        .parent()
        .unwrap_or(Path::new(""))
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    for part in value.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop().ok_or("附件路径超出笔记库")?;
            }
            part => parts.push(part.to_string()),
        }
    }
    let relative = parts.join("/");
    let archive = note
        .path
        .strip_prefix("Trash/.folders/")
        .and_then(|path| path.split_once('/'))
        .map(|(folder, _)| format!("Trash/.folders/{folder}/"));
    let in_own_archive = note.trashed && archive.is_some_and(|path| relative.starts_with(&path));
    if !assets::valid_path(&relative)
        || parts.iter().enumerate().any(|(index, part)| {
            part.starts_with('.')
                && part != ".attachments"
                && !(index == 1 && part == ".folders" && in_own_archive)
        })
    {
        return Err("附件路径属于笔记库内部文件".into());
    }
    Ok(relative)
}

pub(super) fn read(path: &Path) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(error)?
        .take(MAX_FILE + 1)
        .read_to_end(&mut bytes)
        .map_err(error)?;
    if bytes.len() as u64 > MAX_FILE {
        return Err("附件超过 64 MiB".into());
    }
    Ok(bytes)
}

pub(super) fn mime(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|part| part.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "m4a" => "audio/mp4",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "opus" => "audio/ogg",
        "aac" => "audio/aac",
        "flac" => "audio/flac",
        "webm" => "video/webm",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "txt" => "text/plain",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        _ => "application/octet-stream",
    }
}

#[derive(Deserialize)]
pub(crate) struct Upload {
    pub name: String,
    pub bytes: Vec<u8>,
}
#[derive(Serialize)]
pub(crate) struct ImportedAttachment {
    pub name: String,
    pub path: String,
    pub image: bool,
}

pub(super) fn import(store: &DiskLibrary, id: &str, upload: Upload) -> Result<ImportedAttachment> {
    let note = store
        .data
        .notes
        .iter()
        .find(|note| note.id == id)
        .ok_or("笔记不存在")?;
    if note.read_only || note.trashed {
        return Err("此笔记不可添加附件".into());
    }
    let name = upload.name.trim();
    if name.is_empty()
        || name.len() > 240
        || name.starts_with('.')
        || name.chars().any(char::is_control)
        || name.contains(['/', '\\', ':'])
        || name.eq_ignore_ascii_case("s3-connection.json")
        || upload.bytes.len() as u64 > MAX_FILE
    {
        return Err("附件名称无效或超过 64 MiB".into());
    }
    let directory = Path::new(&note.path).parent().unwrap_or(Path::new(""));
    let relative = directory
        .join(".attachments")
        .join(Uuid::new_v4().to_string())
        .join(name);
    let relative = relative.to_str().ok_or("附件路径无效")?.replace('\\', "/");
    if !assets::valid_path(&relative) {
        return Err("附件路径无效".into());
    }
    let path = checked(&store.root, &relative)?;
    fs::create_dir_all(path.parent().ok_or("附件目录无效")?).map_err(error)?;
    // A fresh UUID directory belongs to this import; never replace an existing user file.
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(error)?;
    if let Err(err) = file.write_all(&upload.bytes).and_then(|_| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(&path);
        return Err(error(err));
    }
    let relative = path
        .strip_prefix(store.root.join(directory))
        .map_err(error)?
        .to_str()
        .ok_or("附件路径无效")?
        .replace('\\', "/");
    Ok(ImportedAttachment {
        name: name.into(),
        path: relative,
        image: mime(&path).starts_with("image/"),
    })
}

#[cfg(feature = "app")]
fn with_store<T>(
    app: &tauri::AppHandle,
    label: &str,
    run: impl FnOnce(&DiskLibrary) -> Result<T>,
) -> Result<T> {
    use tauri::Manager;
    if !label.starts_with("document-note-") && label != "main" {
        return workspace::with_document(app, label, run);
    }
    let state = app.state::<FileLibrary>();
    let lock = state.0.lock().map_err(error)?;
    let store = lock.as_ref().ok_or("请先打开笔记库")?;
    workspace::check_library_window(app, label, &store.root)?;
    run(store)
}

#[cfg(feature = "app")]
pub fn response(
    app: &tauri::AppHandle,
    label: &str,
    request: tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    let result: Result<Option<(&str, Vec<u8>)>> = (|| {
        if request.method() != tauri::http::Method::GET {
            return Err("不支持的资源请求".into());
        }
        let url = tauri::Url::parse(&request.uri().to_string()).map_err(error)?;
        let id = url.path().trim_start_matches('/');
        let value = url
            .query_pairs()
            .find(|(key, _)| key == "path")
            .ok_or("附件路径缺失")?
            .1
            .into_owned();
        with_store(app, label, |store| match resolve(store, id, &value) {
            Ok(path) => Ok(Some((mime(&path), read(&path)?))),
            Err(reason)
                if store
                    .data
                    .sync
                    .as_ref()
                    .is_some_and(|sync| sync["version"].as_u64() == Some(2)) =>
            {
                let relative = relative_path(store, id, &value)?;
                if !checked(&store.root, &relative)?.exists()
                    && assets::placeholders::load(store)?.contains_key(&relative)
                {
                    Ok(None)
                } else {
                    Err(reason)
                }
            }
            Err(reason) => Err(reason),
        })
    })();
    match result {
        Ok(Some((mime, bytes))) => tauri::http::Response::builder()
            .header("Content-Type", mime)
            .header("Cache-Control", "no-store")
            .header("X-Content-Type-Options", "nosniff")
            .header(
                "Content-Security-Policy",
                "sandbox; default-src 'none'; style-src 'unsafe-inline'",
            )
            .body(bytes)
            .unwrap(),
        Ok(None) => tauri::http::Response::builder()
            .status(404)
            .header("X-QingYu-Placeholder", "1")
            .header("Cache-Control", "no-store")
            .body(Vec::new())
            .unwrap(),
        Err(_) => tauri::http::Response::builder()
            .status(404)
            .body(Vec::new())
            .unwrap(),
    }
}

#[cfg(feature = "app")]
#[tauri::command]
pub async fn attachment_import(
    window: tauri::WebviewWindow,
    id: String,
    upload: Upload,
) -> Result<ImportedAttachment> {
    with_store(window.app_handle(), window.label(), |store| {
        import(store, &id, upload)
    })
}

#[cfg(all(feature = "app", any(target_os = "macos", target_os = "ios")))]
#[tauri::command]
pub async fn memo_pick_attachments(
    window: tauri::WebviewWindow,
    images: bool,
    remaining: u64,
) -> Result<tauri::ipc::Response> {
    use tauri_plugin_dialog::{DialogExt, PickerMode};
    if window.label() != "main" {
        return Err("请在主窗口中选择附件".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let dialog = window.dialog().file();
        #[cfg(target_os = "macos")]
        let dialog = dialog.set_parent(&window);
        let dialog = if images {
            dialog.set_picker_mode(PickerMode::Image).add_filter(
                "Images",
                &[
                    "png", "jpg", "jpeg", "gif", "webp", "avif", "svg", "bmp", "ico",
                ],
            )
        } else {
            dialog
        };
        let mut response = vec![0; 4];
        let mut files = Vec::new();
        let limit = remaining.min(4 * MAX_FILE);
        for selected in dialog.blocking_pick_files().unwrap_or_default() {
            let path = selected.into_path().map_err(error)?;
            let info = fs::metadata(&path).map_err(error)?;
            if !info.is_file() || info.len() > MAX_FILE {
                return Err("附件不可读取或超过 64 MiB".into());
            }
            let used = (response.len() - 4) as u64;
            if info.len() > limit - used {
                return Err("附件总大小超过上限".into());
            }
            let bytes = read(&path)?;
            if bytes.len() as u64 > limit - used {
                return Err("附件总大小超过上限".into());
            }
            let name = path
                .file_name()
                .ok_or("无法读取附件名称")?
                .to_string_lossy();
            files.push(
                serde_json::json!({ "name": name, "type": mime(&path), "size": bytes.len() }),
            );
            response.extend(bytes);
        }
        // Record the start of the file index so the selected bytes can be returned without JSON encoding.
        let index = response.len() as u32;
        response[..4].copy_from_slice(&index.to_le_bytes());
        serde_json::to_writer(&mut response, &files).map_err(error)?;
        Ok(tauri::ipc::Response::new(response))
    })
    .await
    .map_err(error)?
}

#[cfg(feature = "app")]
#[tauri::command]
pub async fn note_picture_asset(
    window: tauri::WebviewWindow,
    note_id: String,
    path: String,
) -> Result<tauri::ipc::Response> {
    with_store(window.app_handle(), window.label(), |store| {
        let file = resolve(store, &note_id, &path)?;
        if !matches!(
            mime(&file),
            "image/png" | "image/jpeg" | "image/webp" | "image/gif" | "image/bmp"
        ) {
            return Err("无法将此附件作为图片导出".into());
        }
        Ok(tauri::ipc::Response::new(read(&file)?))
    })
}

#[cfg(feature = "app")]
#[tauri::command]
pub async fn note_audio_asset(
    window: tauri::WebviewWindow,
    note_id: String,
    path: String,
) -> Result<tauri::ipc::Response> {
    with_store(window.app_handle(), window.label(), |store| {
        let file = resolve(store, &note_id, &path)?;
        let kind = mime(&file);
        if !kind.starts_with("audio/") && kind != "video/webm" {
            return Err("此附件不是受支持的音频".into());
        }
        Ok(tauri::ipc::Response::new(read(&file)?))
    })
}

pub(super) fn can_open(path: &Path) -> bool {
    ![
        "exe",
        "com",
        "msi",
        "bat",
        "cmd",
        "ps1",
        "sh",
        "command",
        "desktop",
        "app",
        "scr",
        "vbs",
        "js",
        "jar",
        "apk",
        "url",
        "webloc",
        "lnk",
        "scf",
        "hta",
        "reg",
        "py",
        "pyw",
        "rb",
        "pl",
        "php",
        "scpt",
        "applescript",
        "workflow",
    ]
    .contains(
        &path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
    )
}

#[cfg(feature = "app")]
#[tauri::command]
pub async fn attachment_open(window: tauri::WebviewWindow, id: String, path: String) -> Result<()> {
    use tauri::Manager;
    let file = with_store(window.app_handle(), window.label(), |store| {
        resolve(store, &id, &path)
    })?;
    if !can_open(&file) {
        return Err("不能从笔记直接运行可执行附件".into());
    }
    #[cfg(mobile)]
    {
        let cache = window
            .app_handle()
            .path()
            .app_cache_dir()
            .map_err(error)?
            .join("attachments")
            .join(Uuid::new_v4().to_string());
        fs::create_dir_all(&cache).map_err(error)?;
        let copy = cache.join(file.file_name().ok_or("附件名称无效")?);
        fs::copy(&file, &copy).map_err(error)?;
        tauri_plugin_attachments::open(window.app_handle().clone(), copy, mime(&file)).await
    }
    #[cfg(desktop)]
    {
        let status = {
            #[cfg(target_os = "macos")]
            {
                std::process::Command::new("/usr/bin/open")
                    .arg("--")
                    .arg(&file)
                    .status()
            }
            #[cfg(target_os = "windows")]
            {
                std::process::Command::new("explorer.exe")
                    .arg(&file)
                    .status()
            }
            #[cfg(target_os = "linux")]
            {
                std::process::Command::new("xdg-open").arg(&file).status()
            }
        }
        .map_err(|_| "无法打开附件，请安装支持此文件类型的应用")?;
        if status.success() {
            Ok(())
        } else {
            Err("系统未能打开此附件".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn imports_unique_resources_and_resolves_only_library_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("note.md"), "# note").unwrap();
        let store = DiskLibrary::open(dir.path().into(), None).unwrap();
        let id = &store.data.notes[0].id;
        for (name, image) in [
            ("中文 图片.png", true),
            ("报告.pdf", false),
            ("说明.txt", false),
        ] {
            let first = import(
                &store,
                id,
                Upload {
                    name: name.into(),
                    bytes: vec![0, 255, 128],
                },
            )
            .unwrap();
            let second = import(
                &store,
                id,
                Upload {
                    name: name.into(),
                    bytes: vec![1],
                },
            )
            .unwrap();
            assert_ne!(first.path, second.path);
            assert_eq!(first.image, image);
            assert_eq!(
                read(&resolve(&store, id, &first.path).unwrap()).unwrap(),
                [0, 255, 128]
            );
        }
        for value in [
            "../outside.pdf",
            "/etc/passwd",
            ".miaoyan/index.json",
            "note.md",
            "file:///etc/passwd",
        ] {
            assert!(resolve(&store, id, value).is_err(), "{value}");
        }
        assert_eq!(assets::scan_assets(&store.root).unwrap().len(), 6);
        for name in [
            "../evil.pdf",
            "s3-connection.json",
            "bad\\name.pdf",
            ".hidden",
        ] {
            assert!(import(
                &store,
                id,
                Upload {
                    name: name.into(),
                    bytes: vec![]
                }
            )
            .is_err());
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/tmp", dir.path().join("link")).unwrap();
            assert!(resolve(&store, id, "link/test.pdf").is_err());
        }
        for name in ["launch.command", "script.py", "auto.scpt", "shortcut.lnk"] {
            assert!(!can_open(Path::new(name)));
        }
        assert!(can_open(Path::new("report.pdf")));
    }
}
