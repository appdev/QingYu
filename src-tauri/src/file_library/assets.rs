use super::*;
use sha2::{Digest, Sha256};

pub(super) mod placeholders;
mod transaction;
pub use transaction::StagedAsset;
pub(super) use transaction::{reap_stages, AssetOperation, StageFiles};
pub(super) use transaction::stage_lock;

pub(super) const MAX_FILE: usize = 64 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    path: String,
    staged: StagedAsset,
    modified: f64,
    previous_hash: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AssetStat {
    path: String,
    size: u64,
    modified: f64,
}
#[derive(Deserialize)]
pub struct AssetRemoval {
    path: String,
    hash: String,
}
#[derive(Deserialize)]
pub struct AssetSync {
    before: Vec<AssetStat>,
    writes: Vec<Asset>,
    deletes: Vec<AssetRemoval>,
    #[serde(default)]
    placeholders: placeholders::Placeholders,
}

pub(super) fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && !path
            .rsplit('/')
            .next()
            .is_some_and(|name| name.eq_ignore_ascii_case("s3-connection.json"))
        && (!allowed(Path::new(path)) || path.split('/').any(|part| part == ".attachments"))
        && !path.contains(['\\', ':'])
        && !path.chars().any(char::is_control)
        && path.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && ![".miaoyan", ".miaoyan-conflicts", ".git"]
                    .iter()
                    .any(|reserved| part.eq_ignore_ascii_case(reserved))
        })
}
pub(super) fn trash_path(path: &str) -> bool {
    path.strip_prefix(".miaoyan/asset-trash/")
        .and_then(|p| p.split_once('/'))
        .is_some_and(|(token, rest)| Uuid::parse_str(token).is_ok() && valid_path(rest))
}

pub(super) fn visit_asset_files(
    root: &Path,
    directory: &Path,
    visit: &mut impl FnMut(String, fs::Metadata) -> Result<()>,
) -> Result<()> {
    for entry in fs::read_dir(directory).map_err(error)? {
        let entry = entry.map_err(error)?;
        let filename = entry
            .file_name()
            .into_string()
            .map_err(|_| "附件名称不是有效 UTF-8")?;
        if [".miaoyan", ".miaoyan-conflicts", ".git"]
            .iter()
            .any(|s| filename.eq_ignore_ascii_case(s))
        {
            continue;
        }
        let kind = entry.file_type().map_err(error)?;
        // Credentials stay local even if the library includes an ancestor of the app config directory.
        if kind.is_file() && filename.eq_ignore_ascii_case("s3-connection.json") {
            continue;
        }
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            visit_asset_files(root, &entry.path(), visit)?;
        } else if kind.is_file()
            && (!allowed(&entry.path())
                || entry.path().strip_prefix(root).is_ok_and(|path| {
                    path.components()
                        .any(|part| part.as_os_str() == ".attachments")
                }))
        {
            let path = entry
                .path()
                .strip_prefix(root)
                .map_err(error)?
                .to_str()
                .ok_or("附件路径不是有效 UTF-8")?
                .replace('\\', "/");
            visit(path, entry.metadata().map_err(error)?)?;
        }
    }
    Ok(())
}

fn asset_stat(path: String, meta: fs::Metadata) -> Result<AssetStat> {
    if !meta.is_file() || !valid_path(&path) {
        return Err(format!("附件路径不能跨平台同步：{path}"));
    }
    if meta.len() > MAX_FILE as u64 {
        return Err(format!("附件超过 64 MiB，未跳过或删除文件：{path}"));
    }
    let modified = millis(meta.modified().map_err(error)?);
    Ok(AssetStat {
        path,
        size: meta.len(),
        modified,
    })
}

pub(super) fn scan_assets(root: &Path) -> Result<Vec<AssetStat>> {
    let mut files = Vec::new();
    visit_asset_files(root, root, &mut |path, meta| {
        files.push(asset_stat(path, meta)?);
        Ok(())
    })?;
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let mut names = HashSet::new();
    if files
        .iter()
        .any(|file| !names.insert(file.path.to_lowercase()))
    {
        return Err("附件名称仅大小写不同，无法跨平台同步".into());
    }
    Ok(files)
}

pub(super) fn read_asset(root: &Path, expected: &AssetStat) -> Result<Vec<u8>> {
    use std::io::Read;
    if !valid_path(&expected.path) {
        return Err("附件路径无效".into());
    }
    let path = checked(root, &expected.path)?;
    if !fs::metadata(&path).map_err(error)?.is_file() {
        return Err("附件不是普通文件".into());
    }
    let file = fs::File::open(path).map_err(error)?;
    let unchanged = || -> Result<bool> {
        Ok(asset_stat(expected.path.clone(), file.metadata().map_err(error)?)? == *expected)
    };
    let mut bytes = Vec::new();
    if unchanged()? {
        (&file)
            .take(expected.size + 1)
            .read_to_end(&mut bytes)
            .map_err(error)?;
        if bytes.len() as u64 == expected.size && unchanged()? {
            return Ok(bytes);
        }
    }
    Err("同步期间本地附件发生变化，已保留新文件，请再次同步".into())
}

pub(super) fn operations(
    root: &Path,
    sync: &AssetSync,
    stage: &mut StageFiles,
    previous: &Library,
    next: &mut Library,
) -> Result<Vec<Operation>> {
    let current = scan_assets(root)?;
    if current != sync.before {
        return Err("同步期间本地附件发生变化，已保留新文件，请再次同步".into());
    }
    let before: HashMap<_, _> = current
        .iter()
        .map(|asset| (asset.path.as_str(), asset))
        .collect();
    let mut changed = HashSet::new();
    let mut staged_paths = HashSet::new();
    let read_expected = |path: &str, hash: &str| -> Result<Vec<u8>> {
        let stat = before.get(path).ok_or("附件已不存在，请再次同步")?;
        let bytes = read_asset(root, stat)?;
        if format!("{:x}", Sha256::digest(&bytes)) != hash {
            return Err("同步期间本地附件发生变化，已保留新文件，请再次同步".into());
        }
        Ok(bytes)
    };
    let mut result = Vec::new();
    for asset in &sync.writes {
        let path = &asset.path;
        if !valid_path(path)
            || !changed.insert(path.as_str())
            || !asset.modified.is_finite()
            || asset.modified < 0.0
        {
            return Err("附件路径重复或无效，未覆盖本地文件".into());
        }
        asset.staged.validate(root)?;
        if !staged_paths.insert(&asset.staged.path) {
            return Err("附件暂存文件重复，未覆盖本地文件".into());
        }
        match &asset.previous_hash {
            Some(hash) => {
                let old = read_expected(path, hash)?;
                if hash != &asset.staged.hash {
                    result.push(Operation::Asset(AssetOperation::Replace {
                        path: path.clone(),
                        before: stage.write(&old)?,
                        after: asset.staged.clone(),
                        previous_modified: before[path.as_str()].modified,
                        modified: asset.modified,
                    }));
                }
            }
            None if !before.contains_key(path.as_str()) => {
                let destination = checked(root, path)?;
                fs::create_dir_all(destination.parent().ok_or("附件目录无效")?).map_err(error)?;
                let identity = file_identity(&checked(root, &asset.staged.path)?);
                result.push(Operation::Asset(AssetOperation::Create {
                    path: path.clone(),
                    staged: asset.staged.clone(),
                    modified: asset.modified,
                    identity,
                }));
            }
            None => return Err("本地附件已存在，未覆盖文件".into()),
        }
    }
    for AssetRemoval { path, hash } in &sync.deletes {
        if !changed.insert(path.as_str()) {
            return Err("附件路径重复，未覆盖本地文件".into());
        }
        if !before.contains_key(path.as_str()) {
            return Err("附件已不存在，请再次同步".into());
        }
        let target = format!(".miaoyan/asset-trash/{}/{path}", id());
        fs::create_dir_all(checked(root, &target)?.parent().ok_or("附件回收目录无效")?)
            .map_err(error)?;
        result.push(Operation::Asset(AssetOperation::Delete {
            path: path.clone(),
            trash: target,
            hash: hash.clone(),
        }));
    }
    let mut names = HashSet::new();
    for path in before
        .keys()
        .copied()
        .filter(|path| !changed.contains(path))
        .chain(sync.writes.iter().map(|asset| asset.path.as_str()))
        .chain(sync.placeholders.keys().map(String::as_str))
    {
        if !valid_path(path) {
            return Err("附件路径无效".into());
        }
        if !names.insert(path.to_lowercase()) {
            return Err("附件名称仅大小写不同，无法跨平台同步".into());
        }
    }
    if names.iter().any(|path| {
        path.match_indices('/')
            .any(|(end, _)| names.contains(&path[..end]))
    }) {
        return Err("附件文件与目录同名，已停止同步".into());
    }
    result.extend(placeholders::operations(
        root,
        previous,
        next,
        &sync.placeholders,
    )?);
    Ok(result)
}

#[cfg(feature = "app")]
#[tauri::command]
pub async fn library_asset_placeholders(
    window: tauri::WebviewWindow,
    state: State<'_, FileLibrary>,
    root: String,
    revision: u64,
) -> Result<placeholders::Placeholders> {
    let active = state.0.lock().map_err(error)?;
    let store = active.as_ref().ok_or("请先打开笔记库")?;
    workspace::check_library_window(window.app_handle(), window.label(), &store.root)?;
    if store.root.to_string_lossy() != root || store.revision != revision {
        return Err("笔记位置或版本已变化，请重新同步".into());
    }
    placeholders::load(store)
}

#[cfg(feature = "app")]
fn update_placeholder(
    window: &tauri::WebviewWindow,
    state: &FileLibrary,
    root: &str,
    revision: u64,
    path: &str,
    hash: &str,
    bytes: Option<&[u8]>,
) -> Result<Snapshot> {
    let mut active = state.0.lock().map_err(error)?;
    let store = active.as_mut().ok_or("请先打开笔记库")?;
    workspace::check_library_window(window.app_handle(), window.label(), &store.root)?;
    if store.root.to_string_lossy() != root || store.revision != revision {
        return Err("笔记位置或版本已变化，请重新下载附件".into());
    }
    placeholders::update(store, path, hash, bytes)?;
    let snapshot = store.snapshot();
    drop(active);
    workspace::library_changed(window.app_handle(), &snapshot.root, Some(window.label()));
    Ok(snapshot)
}

#[cfg(feature = "app")]
#[tauri::command]
pub async fn library_materialize_asset(
    window: tauri::WebviewWindow,
    state: State<'_, FileLibrary>,
    root: String,
    revision: u64,
    path: String,
    hash: String,
    encoded: String,
) -> Result<Snapshot> {
    let bytes = decode_asset(encoded)?;
    update_placeholder(&window, &state, &root, revision, &path, &hash, Some(&bytes))
}

#[cfg(feature = "app")]
#[tauri::command]
pub async fn library_delete_asset_placeholder(
    window: tauri::WebviewWindow,
    state: State<'_, FileLibrary>,
    root: String,
    revision: u64,
    path: String,
    hash: String,
) -> Result<Snapshot> {
    update_placeholder(&window, &state, &root, revision, &path, &hash, None)
}

#[cfg(feature = "app")]
#[tauri::command]
pub async fn library_sync_assets(
    window: tauri::WebviewWindow,
    state: State<'_, FileLibrary>,
    root: String,
    revision: u64,
) -> Result<Vec<AssetStat>> {
    if window.label() != "main" {
        return Err("不允许的附件同步窗口".into());
    }
    let lock = state.0.lock().map_err(error)?;
    let store = lock.as_ref().ok_or("请先打开笔记库")?;
    if store.root.to_string_lossy() != root || store.revision != revision {
        return Err("笔记位置或版本已变化，请重新同步".into());
    }
    scan_assets(&store.root)
}

#[cfg(feature = "app")]
#[tauri::command]
pub async fn library_read_asset(
    window: tauri::WebviewWindow,
    state: State<'_, FileLibrary>,
    root: String,
    revision: u64,
    asset: AssetStat,
) -> Result<tauri::ipc::Response> {
    if window.label() != "main" {
        return Err("不允许的附件同步窗口".into());
    }
    let lock = state.0.lock().map_err(error)?;
    let store = lock.as_ref().ok_or("请先打开笔记库")?;
    if store.root.to_string_lossy() != root || store.revision != revision {
        return Err("笔记位置或版本已变化，请重新同步".into());
    }
    Ok(tauri::ipc::Response::new(read_asset(&store.root, &asset)?))
}

#[cfg(feature = "app")]
#[derive(Default)]
pub struct AssetStage(Mutex<Option<StageSession>>);

#[cfg(feature = "app")]
struct StageSession {
    batch: String,
    revision: u64,
    root: PathBuf,
    files: StageFiles,
    _lock: fs::File,
}

#[cfg(feature = "app")]
#[tauri::command]
pub async fn library_stage_asset(
    window: tauri::WebviewWindow,
    state: State<'_, FileLibrary>,
    staging: State<'_, AssetStage>,
    root: String,
    revision: u64,
    batch: String,
    encoded: String,
) -> Result<StagedAsset> {
    if window.label() != "main" || Uuid::parse_str(&batch).is_err() {
        return Err("附件暂存请求无效".into());
    }
    let bytes = decode_asset(encoded)?;
    let lock = state.0.lock().map_err(error)?;
    let store = lock.as_ref().ok_or("请先打开笔记库")?;
    if store.root.to_string_lossy() != root || store.revision != revision {
        return Err("笔记位置或版本已变化，请重新同步".into());
    }
    let mut active = staging.0.lock().map_err(error)?;
    if active.is_none() {
        let lock = transaction::stage_lock(&store.root)?;
        try_lock(&lock).map_err(|_| "笔记库正在暂存其他同步附件，请稍后重试")?;
        *active = Some(StageSession {
            batch: batch.clone(),
            revision,
            root: store.root.clone(),
            files: StageFiles::new(&store.root),
            _lock: lock,
        });
    }
    let session = active.as_mut().ok_or("附件暂存会话不存在")?;
    if session.batch != batch || session.root != store.root || session.revision != revision {
        return Err("笔记库正在暂存其他同步附件，请稍后重试".into());
    }
    session.files.write(&bytes)
}

#[cfg(feature = "app")]
fn decode_asset(encoded: String) -> Result<Vec<u8>> {
    use base64::Engine;
    if encoded.len() > MAX_FILE.div_ceil(3) * 4 {
        return Err("附件超过 64 MiB".into());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(error)?;
    if bytes.len() > MAX_FILE {
        return Err("附件超过 64 MiB".into());
    }
    Ok(bytes)
}

#[cfg(feature = "app")]
#[tauri::command]
pub async fn library_read_staged_asset(
    window: tauri::WebviewWindow,
    staging: State<'_, AssetStage>,
    batch: String,
    asset: StagedAsset,
) -> Result<tauri::ipc::Response> {
    if window.label() != "main" {
        return Err("不允许的附件同步窗口".into());
    }
    let active = staging.0.lock().map_err(error)?;
    let session = active
        .as_ref()
        .filter(|session| session.batch == batch)
        .ok_or("附件暂存会话已结束")?;
    Ok(tauri::ipc::Response::new(asset.read(&session.root)?))
}

#[cfg(feature = "app")]
#[tauri::command]
pub async fn library_release_asset_stage(
    window: tauri::WebviewWindow,
    staging: State<'_, AssetStage>,
    batch: String,
) -> Result<()> {
    if window.label() != "main" {
        return Err("不允许的附件同步窗口".into());
    }
    let mut active = staging.0.lock().map_err(error)?;
    if active
        .as_ref()
        .is_some_and(|session| session.batch == batch)
    {
        active.take();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn asset(stage: &mut StageFiles, path: &str, bytes: &[u8]) -> Asset {
        Asset {
            path: path.into(),
            staged: stage.write(bytes).unwrap(),
            modified: now(),
            previous_hash: None,
        }
    }
    fn commit(
        store: &mut DiskLibrary,
        before: Vec<AssetStat>,
        writes: Vec<Asset>,
        deletes: Vec<AssetRemoval>,
    ) -> Result<()> {
        store.commit_bytes(
            store.data.clone(),
            "sync",
            &HashMap::new(),
            Some(&AssetSync {
                before,
                writes,
                deletes,
                placeholders: Default::default(),
            }),
        )
    }
    #[test]
    fn recursive_assets_include_hidden_attachments_and_skip_internal_notes_and_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".attachments/nested")).unwrap();
        fs::write(dir.path().join(".attachments/nested/a.pdf"), [0, 255, 128]).unwrap();
        fs::write(dir.path().join("note.md"), "source").unwrap();
        fs::write(
            dir.path().join("s3-connection.json"),
            "synthetic private configuration",
        )
        .unwrap();
        let _store = DiskLibrary::open(dir.path().into(), None).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("/tmp", dir.path().join("outside")).unwrap();
        let files = scan_assets(dir.path()).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].size, 3);
        assert_eq!(read_asset(dir.path(), &files[0]).unwrap(), [0, 255, 128]);
        assert_eq!(files[0].path, ".attachments/nested/a.pdf");
    }
    #[test]
    fn commits_create_update_and_recoverable_delete_with_checkpoint() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = DiskLibrary::open(dir.path().into(), None).unwrap();
        let mut stage = StageFiles::new(dir.path());
        let first_bytes = [0, 255, 128];
        let second_bytes = [2, 255];
        let first = asset(&mut stage, "images/sub/a.webp", &first_bytes);
        commit(&mut store, vec![], vec![first.clone()], vec![]).unwrap();
        assert_eq!(fs::read(dir.path().join(&first.path)).unwrap(), first_bytes);
        let mut second = asset(&mut stage, &first.path, &second_bytes);
        second.previous_hash = Some(first.staged.hash);
        let before = scan_assets(dir.path()).unwrap();
        commit(&mut store, before, vec![second.clone()], vec![]).unwrap();
        assert_eq!(
            fs::read(dir.path().join(&second.path)).unwrap(),
            second_bytes
        );
        let before = scan_assets(dir.path()).unwrap();
        commit(
            &mut store,
            before,
            vec![],
            vec![AssetRemoval {
                path: second.path.clone(),
                hash: second.staged.hash,
            }],
        )
        .unwrap();
        assert!(!dir.path().join(&second.path).exists());
        let backups = fs::read_dir(dir.path().join(".miaoyan/asset-trash"))
            .unwrap()
            .collect::<io::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(backups.len(), 1);
        assert_eq!(
            fs::read(backups[0].path().join(&second.path)).unwrap(),
            second_bytes
        );
        DiskLibrary::open(dir.path().into(), None).unwrap();
        assert!(scan_assets(dir.path()).unwrap().is_empty());
    }
    #[test]
    fn refuses_external_attachment_changes_and_invalid_remote_paths_before_checkpoint() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.pdf"), [0, 255]).unwrap();
        let mut store = DiskLibrary::open(dir.path().into(), None).unwrap();
        let before = scan_assets(dir.path()).unwrap();
        let revision = store.revision;
        fs::write(dir.path().join("a.pdf"), [1, 255]).unwrap();
        modified(&dir.path().join("a.pdf"), before[0].modified).unwrap();
        assert_eq!(scan_assets(dir.path()).unwrap(), before);
        assert!(commit(
            &mut store,
            before,
            vec![],
            vec![AssetRemoval {
                path: "a.pdf".into(),
                hash: format!("{:x}", Sha256::digest([0, 255])),
            }]
        )
        .unwrap_err()
        .contains("发生变化"));
        assert_eq!(store.revision, revision);
        assert_eq!(fs::read(dir.path().join("a.pdf")).unwrap(), [1, 255]);
        let before = scan_assets(dir.path()).unwrap();
        let mut stage = StageFiles::new(dir.path());
        for path in [
            "../escape.pdf",
            ".miaoyan/profile.json",
            "a.md",
            "/outside.webp",
        ] {
            assert!(commit(
                &mut store,
                before.clone(),
                vec![asset(&mut stage, path, &[9])],
                vec![]
            )
            .is_err());
            assert_eq!(store.revision, revision);
        }
    }
    #[test]
    fn interrupted_asset_transaction_restores_original_binary() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.pdf"), [0, 255]).unwrap();
        let store = DiskLibrary::open(dir.path().into(), None).unwrap();
        let op = Operation::Write {
            path: "a.pdf".into(),
            before: vec![0, 255],
            after: vec![3, 254],
            modified: now(),
        };
        atomic(
            &dir.path().join(JOURNAL),
            &serde_json::to_vec(&Transaction {
                generation: store.revision + 1,
                operations: vec![op.clone()],
                asset_sync: true,
            })
            .unwrap(),
            false,
        )
        .unwrap();
        op.apply(dir.path()).unwrap();
        drop(store);
        DiskLibrary::open(dir.path().into(), None).unwrap();
        assert_eq!(fs::read(dir.path().join("a.pdf")).unwrap(), [0, 255]);
    }
    #[test]
    fn staged_asset_journal_restores_replacement_and_delete_without_inline_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.pdf");
        let old = [0, 255, 123, 176, 7];
        fs::write(&path, old).unwrap();
        fs::write(dir.path().join("b.pdf"), old).unwrap();
        let store = DiskLibrary::open(dir.path().into(), None).unwrap();
        let mut stage = StageFiles::new(dir.path());
        let mut replacement = asset(&mut stage, "a.pdf", &[88, 42, 87]);
        replacement.previous_hash = Some(format!("{:x}", Sha256::digest(old)));
        let previous = scan_assets(dir.path()).unwrap();
        let mut next = store.data.clone();
        let operations = operations(
            dir.path(),
            &AssetSync {
                before: previous.clone(),
                writes: vec![replacement],
                deletes: vec![AssetRemoval {
                    path: "b.pdf".into(),
                    hash: format!("{:x}", Sha256::digest(old)),
                }],
                placeholders: Default::default(),
            },
            &mut stage,
            &store.data,
            &mut next,
        )
        .unwrap();
        let journal = serde_json::to_vec(&Transaction {
            generation: store.revision + 1,
            operations: operations.clone(),
            asset_sync: true,
        })
        .unwrap();
        assert!(!journal
            .windows(b"[0,255,123,176,7]".len())
            .any(|bytes| bytes == b"[0,255,123,176,7]"));
        atomic(&dir.path().join(JOURNAL), &journal, false).unwrap();
        for operation in &operations {
            operation.apply(dir.path()).unwrap();
        }
        assert_eq!(fs::read(&path).unwrap(), [88, 42, 87]);
        assert!(!dir.path().join("b.pdf").exists());
        drop(stage);
        drop(store);
        DiskLibrary::open(dir.path().into(), None).unwrap();
        assert_eq!(fs::read(&path).unwrap(), old);
        assert_eq!(fs::read(dir.path().join("b.pdf")).unwrap(), old);
        assert_eq!(scan_assets(dir.path()).unwrap(), previous);
        assert!(!dir.path().join(JOURNAL).exists());
    }
    #[test]
    fn oversized_assets_fail_explicitly_instead_of_becoming_remote_deletions() {
        let dir = tempfile::tempdir().unwrap();
        fs::File::create(dir.path().join("large.pdf"))
            .unwrap()
            .set_len(MAX_FILE as u64 + 1)
            .unwrap();
        assert!(scan_assets(dir.path()).unwrap_err().contains("64 MiB"));
    }
    #[test]
    fn staged_commit_accepts_more_than_256_mib_of_attachments() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = DiskLibrary::open(dir.path().into(), None).unwrap();
        let mut stage = StageFiles::new(dir.path());
        let mut writes = Vec::new();
        let bytes = vec![0; 52 * 1024 * 1024];
        for i in 0..5 {
            writes.push(asset(&mut stage, &format!("{i}.pdf"), &bytes));
        }
        drop(bytes);
        commit(&mut store, vec![], writes, vec![]).unwrap();
        let before = scan_assets(dir.path()).unwrap();
        assert!(before.iter().map(|asset| asset.size).sum::<u64>() > 256 * 1024 * 1024);
        commit(&mut store, before.clone(), vec![], vec![]).unwrap();
        assert_eq!(scan_assets(dir.path()).unwrap(), before);
    }
    #[test]
    fn reopen_reaps_abandoned_stages_but_respects_the_active_sync_lock() {
        let dir = tempfile::tempdir().unwrap();
        DiskLibrary::open(dir.path().into(), None).unwrap();
        let mut stage = StageFiles::new(dir.path());
        let staged = stage.write(&[4, 2]).unwrap();
        let lock = transaction::stage_lock(dir.path()).unwrap();
        try_lock(&lock).unwrap();
        DiskLibrary::open(dir.path().into(), None).unwrap();
        assert_eq!(staged.read(dir.path()).unwrap(), &[4, 2]);
        drop(lock);
        DiskLibrary::open(dir.path().into(), None).unwrap();
        assert!(!dir.path().join(staged.path).exists());
    }
}
