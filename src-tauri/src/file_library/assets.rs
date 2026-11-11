use super::*;

const MAX_FILE: usize = 64 * 1024 * 1024;
const MAX_TOTAL: usize = 256 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Asset {
    path: String,
    bytes: Vec<u8>,
    modified: f64,
}
#[derive(Deserialize)]
pub struct AssetSync {
    before: Vec<Asset>,
    after: Vec<Asset>,
}

pub(super) fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && !path
            .rsplit('/')
            .next()
            .is_some_and(|name| name.eq_ignore_ascii_case("s3-connection.json"))
        && !allowed(Path::new(path))
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

fn scan_assets(root: &Path) -> Result<Vec<Asset>> {
    fn visit(
        root: &Path,
        directory: &Path,
        output: &mut Vec<Asset>,
        total: &mut usize,
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
                visit(root, &entry.path(), output, total)?;
            } else if kind.is_file() && !allowed(&entry.path()) {
                let path = entry
                    .path()
                    .strip_prefix(root)
                    .map_err(error)?
                    .to_str()
                    .ok_or("附件路径不是有效 UTF-8")?
                    .replace('\\', "/");
                if !valid_path(&path) {
                    return Err(format!("附件路径不能跨平台同步：{path}"));
                }
                let meta = entry.metadata().map_err(error)?;
                if meta.len() > MAX_FILE as u64 {
                    return Err(format!("附件超过 64 MiB，未跳过或删除文件：{path}"));
                }
                // Bound the actual read too: another process may grow the file after metadata().
                use std::io::Read;
                let mut bytes = Vec::new();
                fs::File::open(checked(root, &path)?)
                    .map_err(error)?
                    .take((MAX_FILE + 1) as u64)
                    .read_to_end(&mut bytes)
                    .map_err(error)?;
                if bytes.len() > MAX_FILE {
                    return Err(format!("附件超过 64 MiB：{path}"));
                }
                *total = total.checked_add(bytes.len()).ok_or("附件总大小超出限制")?;
                if *total > MAX_TOTAL {
                    return Err("本次附件快照超过 256 MiB，已停止同步并保留本地文件".into());
                }
                output.push(Asset {
                    path,
                    bytes,
                    modified: meta.modified().map(millis).unwrap_or(0.0),
                });
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    visit(root, root, &mut files, &mut 0)?;
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

pub(super) fn operations(root: &Path, sync: &AssetSync) -> Result<Vec<Operation>> {
    let current = scan_assets(root)?;
    let to_map = |files: &[Asset]| -> Result<HashMap<String, Asset>> {
        let mut result = HashMap::new();
        let mut names = HashSet::new();
        let mut total = 0usize;
        for asset in files {
            if !valid_path(&asset.path)
                || !names.insert(asset.path.to_lowercase())
                || !asset.modified.is_finite()
                || asset.modified < 0.0
            {
                return Err("附件路径重复或无效，未覆盖本地文件".into());
            }
            total = total
                .checked_add(asset.bytes.len())
                .ok_or("附件总大小超出限制")?;
            if asset.bytes.len() > MAX_FILE || total > MAX_TOTAL {
                return Err("附件快照超出大小限制".into());
            }
            checked(root, &asset.path)?;
            result.insert(asset.path.clone(), asset.clone());
        }
        Ok(result)
    };
    let before = to_map(&sync.before)?;
    let after = to_map(&sync.after)?;
    if current.len() != before.len()
        || current.iter().any(|asset| {
            before
                .get(&asset.path)
                .is_none_or(|old| old.bytes != asset.bytes)
        })
    {
        return Err("同步期间本地附件发生变化，已保留新文件，请再次同步".into());
    }
    let mut result = Vec::new();
    for (path, asset) in &after {
        match before.get(path) {
            Some(old) if old.bytes != asset.bytes => result.push(Operation::Write {
                path: path.clone(),
                before: old.bytes.clone(),
                after: asset.bytes.clone(),
                modified: asset.modified,
            }),
            None => {
                let destination = checked(root, path)?;
                fs::create_dir_all(destination.parent().ok_or("附件目录无效")?).map_err(error)?;
                result.push(Operation::Create {
                    path: path.clone(),
                    bytes: asset.bytes.clone(),
                    staged: format!(".miaoyan/staged-{}", id()),
                    modified: asset.modified,
                    identity: None,
                });
            }
            _ => {}
        }
    }
    for (path, old) in before {
        if !after.contains_key(&path) {
            let target = format!(".miaoyan/asset-trash/{}/{path}", id());
            fs::create_dir_all(checked(root, &target)?.parent().ok_or("附件回收目录无效")?)
                .map_err(error)?;
            result.push(Operation::Move {
                from: path,
                to: target,
                bytes: old.bytes,
            });
        }
    }
    Ok(result)
}

#[tauri::command]
pub async fn library_sync_assets(
    window: tauri::WebviewWindow,
    state: State<'_, FileLibrary>,
    root: String,
    revision: u64,
) -> Result<Vec<Asset>> {
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

#[cfg(test)]
mod tests {
    use super::*;
    fn asset(path: &str, bytes: &[u8]) -> Asset {
        Asset {
            path: path.into(),
            bytes: bytes.to_vec(),
            modified: now(),
        }
    }
    fn commit(store: &mut DiskLibrary, before: Vec<Asset>, after: Vec<Asset>) -> Result<()> {
        store.commit_bytes(
            store.data.clone(),
            "sync",
            &HashMap::new(),
            Some(&AssetSync { before, after }),
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
        assert_eq!(files[0].bytes, [0, 255, 128]);
        assert_eq!(files[0].path, ".attachments/nested/a.pdf");
    }
    #[test]
    fn commits_create_update_and_recoverable_delete_with_checkpoint() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = DiskLibrary::open(dir.path().into(), None).unwrap();
        let first = asset("images/sub/a.webp", &[0, 255, 128]);
        commit(&mut store, vec![], vec![first.clone()]).unwrap();
        assert_eq!(fs::read(dir.path().join(&first.path)).unwrap(), first.bytes);
        let second = asset(&first.path, &[2, 255]);
        commit(&mut store, vec![first], vec![second.clone()]).unwrap();
        assert_eq!(
            fs::read(dir.path().join(&second.path)).unwrap(),
            second.bytes
        );
        commit(&mut store, vec![second.clone()], vec![]).unwrap();
        assert!(!dir.path().join(&second.path).exists());
        let backups = fs::read_dir(dir.path().join(".miaoyan/asset-trash"))
            .unwrap()
            .collect::<io::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(backups.len(), 1);
        assert_eq!(
            fs::read(backups[0].path().join(&second.path)).unwrap(),
            second.bytes
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
        assert!(commit(&mut store, before, vec![])
            .unwrap_err()
            .contains("发生变化"));
        assert_eq!(store.revision, revision);
        assert_eq!(fs::read(dir.path().join("a.pdf")).unwrap(), [1, 255]);
        let before = scan_assets(dir.path()).unwrap();
        for path in [
            "../escape.pdf",
            ".miaoyan/profile.json",
            "a.md",
            "/outside.webp",
        ] {
            assert!(commit(&mut store, before.clone(), vec![asset(path, &[9])]).is_err());
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
    fn oversized_assets_fail_explicitly_instead_of_becoming_remote_deletions() {
        let dir = tempfile::tempdir().unwrap();
        fs::File::create(dir.path().join("large.pdf"))
            .unwrap()
            .set_len(MAX_FILE as u64 + 1)
            .unwrap();
        assert!(scan_assets(dir.path()).unwrap_err().contains("64 MiB"));
    }
}
