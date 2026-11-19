use super::*;
use std::io::Read;

const STAGE_PREFIX: &str = ".miaoyan/asset-stage-";

#[derive(Clone, Serialize, Deserialize)]
pub struct StagedAsset {
    pub(super) path: String,
    pub(super) hash: String,
}

impl StagedAsset {
    pub(super) fn validate(&self, root: &Path) -> Result<()> {
        checked(root, &self.path)?;
        if self
            .path
            .strip_prefix(STAGE_PREFIX)
            .and_then(|id| Uuid::parse_str(id).ok())
            .is_none()
        {
            return Err("恢复日志中的附件暂存路径无效".into());
        }
        Ok(())
    }

    pub(in crate::file_library) fn read(&self, root: &Path) -> Result<Vec<u8>> {
        self.validate(root)?;
        let mut bytes = Vec::new();
        fs::File::open(checked(root, &self.path)?)
            .map_err(error)?
            .take(MAX_FILE as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(error)?;
        if bytes.len() > MAX_FILE || format!("{:x}", Sha256::digest(&bytes)) != self.hash {
            return Err("暂存附件内容发生变化，已停止写入".into());
        }
        Ok(bytes)
    }
}

pub(in crate::file_library) struct StageFiles {
    root: PathBuf,
    paths: Vec<PathBuf>,
}

impl StageFiles {
    pub(in crate::file_library) fn new(root: &Path) -> Self {
        Self {
            root: root.to_owned(),
            paths: Vec::new(),
        }
    }

    pub(in crate::file_library) fn write(&mut self, bytes: &[u8]) -> Result<StagedAsset> {
        let path = format!("{STAGE_PREFIX}{}", id());
        let file = checked(&self.root, &path)?;
        atomic(&file, bytes, false)?;
        self.paths.push(file);
        Ok(StagedAsset {
            path,
            hash: format!("{:x}", Sha256::digest(bytes)),
        })
    }
}

impl Drop for StageFiles {
    fn drop(&mut self) {
        // Recovery owns these files once a journal has been written.
        if !self.paths.is_empty() && !self.root.join(JOURNAL).exists() {
            for path in &self.paths {
                let _ = fs::remove_file(path);
            }
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub(in crate::file_library) enum AssetOperation {
    Create {
        path: String,
        staged: StagedAsset,
        modified: f64,
        identity: Option<[u64; 2]>,
    },
    Replace {
        path: String,
        before: StagedAsset,
        after: StagedAsset,
        previous_modified: f64,
        modified: f64,
    },
    Delete {
        path: String,
        trash: String,
        hash: String,
    },
}

fn require_hash(path: &Path, hash: &str) -> Result<()> {
    if !fs::metadata(path).map_err(error)?.is_file() {
        return Err("附件不是普通文件".into());
    }
    let mut file = fs::File::open(path)
        .map_err(error)?
        .take(MAX_FILE as u64 + 1);
    let mut digest = Sha256::new();
    let size = io::copy(&mut file, &mut digest).map_err(error)?;
    if size > MAX_FILE as u64 || format!("{:x}", digest.finalize()) != hash {
        return Err("同步期间本地附件发生变化，已保留新文件，请再次同步".into());
    }
    Ok(())
}

fn copy_staged(root: &Path, staged: &StagedAsset, to: &Path, time: f64) -> Result<()> {
    atomic(to, &staged.read(root)?, true)?;
    modified(to, time)
}

pub(in crate::file_library) fn stage_lock(root: &Path) -> Result<fs::File> {
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(checked(root, ".miaoyan/asset-stage.lock")?)
        .map_err(error)
}

pub(in crate::file_library) fn reap_stages(root: &Path) -> Result<()> {
    let lock = stage_lock(root)?;
    if try_lock(&lock).is_err() {
        return Ok(());
    }
    for entry in fs::read_dir(root.join(META)).map_err(error)? {
        let entry = entry.map_err(error)?;
        if entry
            .file_name()
            .to_str()
            .and_then(|name| name.strip_prefix("asset-stage-"))
            .and_then(|id| Uuid::parse_str(id).ok())
            .is_some()
            && entry.file_type().map_err(error)?.is_file()
        {
            fs::remove_file(entry.path()).map_err(error)?;
        }
    }
    Ok(())
}

impl AssetOperation {
    pub(in crate::file_library) fn validate(&self, root: &Path) -> Result<()> {
        let path = match self {
            Self::Create { path, staged, .. } => {
                staged.validate(root)?;
                path
            }
            Self::Replace {
                path,
                before,
                after,
                ..
            } => {
                before.validate(root)?;
                after.validate(root)?;
                if before.path == after.path {
                    return Err("恢复日志中的附件暂存文件重复".into());
                }
                path
            }
            Self::Delete { path, trash, .. } => {
                if !trash_path(trash) || trash.splitn(4, '/').nth(3) != Some(path) {
                    return Err("恢复日志中的附件回收路径无效".into());
                }
                checked(root, trash)?;
                path
            }
        };
        if !valid_path(path) {
            return Err("恢复日志中的附件路径无效".into());
        }
        checked(root, path)?;
        Ok(())
    }

    pub(in crate::file_library) fn check(&self, root: &Path) -> Result<()> {
        self.validate(root)?;
        match self {
            Self::Create { path, staged, .. } => {
                if checked(root, path)?.exists() {
                    return Err("附件已存在，未覆盖".into());
                }
                require_hash(&checked(root, &staged.path)?, &staged.hash)
            }
            Self::Replace {
                path,
                before,
                after,
                ..
            } => {
                let path = checked(root, path)?;
                if fs::metadata(&path).map_err(error)?.permissions().readonly() {
                    return Err("附件为只读状态，未覆盖".into());
                }
                require_hash(&path, &before.hash)?;
                require_hash(&checked(root, &before.path)?, &before.hash)?;
                require_hash(&checked(root, &after.path)?, &after.hash)
            }
            Self::Delete { path, hash, .. } => require_hash(&checked(root, path)?, hash),
        }
    }

    pub(in crate::file_library) fn apply(&self, root: &Path) -> Result<()> {
        match self {
            Self::Create {
                path,
                staged,
                modified: time,
                ..
            } => {
                let stage = checked(root, &staged.path)?;
                let path = checked(root, path)?;
                modified(&stage, *time)?;
                #[cfg(target_os = "android")]
                move_exclusive(&stage, &path)?;
                #[cfg(not(target_os = "android"))]
                fs::hard_link(stage, path).map_err(error)?;
                Ok(())
            }
            Self::Replace {
                path,
                after,
                modified: time,
                ..
            } => copy_staged(root, after, &checked(root, path)?, *time),
            Self::Delete { path, trash, .. } => {
                move_exclusive(&checked(root, path)?, &checked(root, trash)?)
            }
        }
    }

    pub(in crate::file_library) fn undo(&self, root: &Path) -> Result<()> {
        match self {
            Self::Create {
                path,
                staged,
                identity,
                ..
            } => {
                let path = checked(root, path)?;
                if same_file(&path, &checked(root, &staged.path)?)
                    || identity.is_some() && file_identity(&path) == *identity
                {
                    require_hash(&path, &staged.hash)?;
                    fs::remove_file(path).map_err(error)?;
                }
                Ok(())
            }
            Self::Replace {
                path,
                before,
                after,
                previous_modified,
                ..
            } => {
                let path = checked(root, path)?;
                if require_hash(&path, &before.hash).is_ok() {
                    return Ok(());
                }
                require_hash(&path, &after.hash)?;
                copy_staged(root, before, &path, *previous_modified)
            }
            Self::Delete { path, trash, hash } => {
                let path = checked(root, path)?;
                let trash = checked(root, trash)?;
                if !trash.exists() {
                    return Ok(());
                }
                require_hash(&trash, hash)?;
                if same_file(&path, &trash) {
                    fs::remove_file(trash).map_err(error)
                } else {
                    move_exclusive(&trash, &path)
                }
            }
        }
    }

    pub(in crate::file_library) fn cleanup(&self, root: &Path) -> Result<()> {
        let staged: &[_] = match self {
            Self::Create { staged, .. } => &[staged],
            Self::Replace { before, after, .. } => &[before, after],
            Self::Delete { .. } => return Ok(()),
        };
        for file in staged {
            match fs::remove_file(checked(root, &file.path)?) {
                Ok(()) => {}
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => return Err(error(err)),
            }
        }
        Ok(())
    }
}
