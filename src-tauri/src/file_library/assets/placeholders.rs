use super::*;
use std::collections::BTreeMap;

pub(in crate::file_library) const INDEX_PATH: &str = ".miaoyan/asset-placeholders.json";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Placeholder {
    pub hash: String,
    pub size: u64,
    pub mtime: f64,
}
pub type Placeholders = BTreeMap<String, Placeholder>;

fn read(
    root: &Path,
    checkpoint: Option<&serde_json::Value>,
) -> Result<(Option<Vec<u8>>, Placeholders)> {
    let path = checked(root, INDEX_PATH)?;
    let bytes = match fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(err) if err.kind() == io::ErrorKind::NotFound => None,
        Err(err) => return Err(error(err)),
    };
    if let Some(sync) = checkpoint.filter(|value| value["version"].as_u64() == Some(2)) {
        let bytes = bytes.ok_or("附件占位索引缺失，已停止同步")?;
        let hash = format!("{:x}", Sha256::digest(&bytes));
        if sync["placeholderDigest"].as_str() != Some(&hash) {
            return Err("附件占位索引已变化，已停止同步".into());
        }
        let placeholders: Placeholders = serde_json::from_slice(&bytes).map_err(error)?;
        if placeholders.is_empty()
            || sync["placeholderCount"].as_u64() != Some(placeholders.len() as u64)
        {
            return Err("附件占位索引无效，已停止同步".into());
        }
        Ok((Some(bytes), placeholders))
    } else {
        if bytes.as_deref().is_some_and(|value| value != b"{}") {
            return Err("附件占位索引与同步记录不匹配，已停止同步".into());
        }
        Ok((bytes, Placeholders::new()))
    }
}

pub(in crate::file_library) fn load(store: &DiskLibrary) -> Result<Placeholders> {
    read(&store.root, store.data.sync.as_ref()).map(|(_, entries)| entries)
}

pub(super) fn update(
    store: &mut DiskLibrary,
    path: &str,
    hash: &str,
    bytes: Option<&[u8]>,
) -> Result<()> {
    if !valid_path(path) || checked(&store.root, path)?.exists() {
        return Err("附件已存在或路径无效，未覆盖文件".into());
    }
    let mut entries = load(store)?;
    let entry = entries
        .remove(path)
        .filter(|entry| entry.hash == hash)
        .ok_or("附件占位已变化，请重试")?;
    let _stage_lock = if bytes.is_some() {
        let lock = transaction::stage_lock(&store.root)?;
        Some(try_lock(lock).map_err(|_| "附件同步正在暂存，请稍后重试")?)
    } else {
        None
    };
    let mut staging = StageFiles::new(&store.root);
    let mut writes = Vec::new();
    if let Some(bytes) = bytes {
        if bytes.len() as u64 != entry.size || format!("{:x}", Sha256::digest(bytes)) != entry.hash
        {
            return Err("附件内容校验失败，未写入文件".into());
        }
        writes.push(Asset {
            path: path.into(),
            staged: staging.write(bytes)?,
            modified: entry.mtime,
            previous_hash: None,
        });
    }
    store.commit_bytes(
        store.data.clone(),
        "sync",
        &HashMap::new(),
        Some(&AssetSync {
            before: scan_assets(&store.root)?,
            writes,
            deletes: Vec::new(),
            placeholders: entries,
        }),
    )
}

pub(super) fn operations(
    root: &Path,
    previous: &Library,
    next: &mut Library,
    entries: &Placeholders,
) -> Result<Option<Operation>> {
    let (before, _) = read(root, previous.sync.as_ref())?;
    if entries.len() > 30_000
        || entries.values().any(|entry| {
            entry.hash.len() != 64
                || !entry
                    .hash
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                || entry.size > MAX_FILE as u64
                || !entry.mtime.is_finite()
                || !(0.0..=8_640_000_000_000_000.0).contains(&entry.mtime)
        })
    {
        return Err("附件占位索引无效，已停止同步".into());
    }
    if let Some(sync) = next.sync.as_mut() {
        let sync = sync.as_object_mut().ok_or("同步记录无效")?;
        if entries.is_empty() {
            sync.insert("version".into(), 1.into());
            sync.remove("placeholderCount");
            sync.remove("placeholderDigest");
        } else {
            let bytes = serde_json::to_vec(entries).map_err(error)?;
            sync.insert("version".into(), 2.into());
            sync.insert("placeholderCount".into(), entries.len().into());
            sync.insert(
                "placeholderDigest".into(),
                format!("{:x}", Sha256::digest(bytes)).into(),
            );
        }
    } else if !entries.is_empty() {
        return Err("附件占位缺少同步记录".into());
    }
    if before.is_none() && entries.is_empty() {
        return Ok(None);
    }
    let after = serde_json::to_vec(entries).map_err(error)?;
    Ok(match before {
        Some(before) if before == after => None,
        Some(before) => Some(Operation::Write {
            path: INDEX_PATH.into(),
            before,
            after,
            modified: now(),
        }),
        None => Some(Operation::Create {
            path: INDEX_PATH.into(),
            bytes: after,
            staged: format!(".miaoyan/staged-{}", id()),
            modified: now(),
            identity: None,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_placeholder(root: &Path) -> (DiskLibrary, String) {
        let mut store = DiskLibrary::open(root.to_owned(), None).unwrap();
        let mut library = store.data.clone();
        library.sync = Some(serde_json::json!({ "version": 1, "target": "test", "state": {} }));
        let hash = format!("{:x}", Sha256::digest([0, 255]));
        let entries = Placeholders::from([(
            "a.pdf".into(),
            Placeholder {
                hash: hash.clone(),
                size: 2,
                mtime: 10.0,
            },
        )]);
        store
            .commit_bytes(
                library,
                "sync",
                &HashMap::new(),
                Some(&AssetSync {
                    before: vec![],
                    writes: vec![],
                    deletes: vec![],
                    placeholders: entries,
                }),
            )
            .unwrap();
        (store, hash)
    }

    #[test]
    fn materializes_only_the_matching_placeholder_and_verified_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, hash) = with_placeholder(dir.path());
        assert_eq!(store.data.sync.as_ref().unwrap()["version"], 2);
        let revision = store.revision;
        assert!(update(&mut store, "a.pdf", "stale", Some(&[0, 255])).is_err());
        assert!(update(&mut store, "a.pdf", &hash, Some(&[0, 254])).is_err());
        assert_eq!(store.revision, revision);
        assert!(!dir.path().join("a.pdf").exists());
        assert_eq!(load(&store).unwrap()["a.pdf"].hash, hash);
        update(&mut store, "a.pdf", &hash, Some(&[0, 255])).unwrap();
        assert_eq!(fs::read(dir.path().join("a.pdf")).unwrap(), [0, 255]);
        assert!(load(&store).unwrap().is_empty());
        assert_eq!(store.data.sync.as_ref().unwrap()["version"], 1);
        assert!(update(&mut store, "a.pdf", &hash, Some(&[0, 255])).is_err());
    }

    #[test]
    fn keeps_folders_with_pending_attachments_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = DiskLibrary::open(dir.path().to_owned(), None).unwrap();
        store
            .folder_change("create", "default", "旅行", None)
            .unwrap();
        store
            .folder_change("create", "default", "其他", None)
            .unwrap();
        let folder = |store: &DiskLibrary, path: &str| {
            store
                .data
                .folders
                .iter()
                .find(|f| f.path == path)
                .unwrap()
                .id
                .clone()
        };
        let travel = folder(&store, "旅行");
        let mut library = store.data.clone();
        library.sync = Some(serde_json::json!({ "version": 1, "target": "test", "state": {} }));
        let hash = format!("{:x}", Sha256::digest([1]));
        let entries = Placeholders::from([(
            "旅行/.attachments/u/a.pdf".into(),
            Placeholder {
                hash: hash.clone(),
                size: 1,
                mtime: 10.0,
            },
        )]);
        store
            .commit_bytes(
                library,
                "sync",
                &HashMap::new(),
                Some(&AssetSync {
                    before: vec![],
                    writes: vec![],
                    deletes: vec![],
                    placeholders: entries,
                }),
            )
            .unwrap();
        let revision = store.revision;
        let renamed = store
            .folder_change("rename", &travel, "游记", None)
            .unwrap_err();
        assert!(renamed.contains("1 个附件尚未下载"), "{renamed}");
        let destination = dir.path().join("其他");
        assert!(store
            .folder_change("move", &travel, destination.to_str().unwrap(), None)
            .is_err());
        assert_eq!(store.revision, revision);
        assert!(dir.path().join("旅行").is_dir());
        assert!(!dir.path().join("游记").exists());
        // 其他文件夹没有占位附件，照常重命名。
        let other = folder(&store, "其他");
        store.folder_change("rename", &other, "别处", None).unwrap();
        assert!(dir.path().join("别处").is_dir());
        // 附件下载后占位消失，文件夹就能移动了。
        update(&mut store, "旅行/.attachments/u/a.pdf", &hash, Some(&[1])).unwrap();
        store
            .folder_change("rename", &travel, "游记", None)
            .unwrap();
        assert_eq!(
            fs::read(dir.path().join("游记/.attachments/u/a.pdf")).unwrap(),
            [1]
        );
    }

    #[test]
    fn refuses_corrupt_index_and_allows_explicit_placeholder_deletion() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, hash) = with_placeholder(dir.path());
        let index = dir.path().join(INDEX_PATH);
        let previous = fs::read(&index).unwrap();
        fs::write(&index, "{}").unwrap();
        assert!(load(&store).is_err());
        assert!(update(&mut store, "a.pdf", &hash, None).is_err());
        fs::write(&index, previous).unwrap();
        update(&mut store, "a.pdf", &hash, None).unwrap();
        assert!(load(&store).unwrap().is_empty());
        assert_eq!(store.data.sync.as_ref().unwrap()["target"], "test");
        assert!(!dir.path().join("a.pdf").exists());
    }
}
