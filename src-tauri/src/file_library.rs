use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{self, Write},
    path::{Component, Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};
#[cfg(feature = "app")]
use tauri::{Manager, State};
use uuid::Uuid;

type Result<T> = std::result::Result<T, String>;
const META: &str = ".miaoyan";
const INDEX: &str = ".miaoyan/library.json";
const JOURNAL: &str = ".miaoyan/transaction.json";
pub(crate) mod folders;
pub(crate) mod external;
pub(crate) mod assets;
pub(crate) mod attachments;
#[cfg(feature = "app")]
pub(crate) mod workspace;
#[cfg(feature = "server")]
pub mod api;

fn try_lock(file: &fs::File) -> std::result::Result<(), fs::TryLockError> {
    #[cfg(target_os = "android")]
    {
        use std::os::fd::AsRawFd;
        // Rust 1.96's std file locks do not support Android yet.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        return Err(if error.kind() == io::ErrorKind::WouldBlock {
            fs::TryLockError::WouldBlock
        } else {
            fs::TryLockError::Error(error)
        });
    }
    #[cfg(not(target_os = "android"))]
    file.try_lock()
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Folder {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub path: String,
}
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: String,
    pub folder_id: String,
    pub source: String,
    pub created_at: f64,
    pub updated_at: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub trashed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conflict_of: Option<String>,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
}
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    pub version: u8,
    pub folders: Vec<Folder>,
    pub notes: Vec<Note>,
    pub active_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync: Option<serde_json::Value>,
}
#[derive(Serialize, Deserialize)]
struct Index {
    generation: u64,
    library: Library,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
enum Operation {
    Create {
        path: String,
        bytes: Vec<u8>,
        staged: String,
        modified: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        identity: Option<[u64; 2]>,
    },
    Move {
        from: String,
        to: String,
        bytes: Vec<u8>,
    },
    Write {
        path: String,
        before: Vec<u8>,
        after: Vec<u8>,
        modified: f64,
    },
}
#[derive(Serialize, Deserialize)]
struct Transaction {
    generation: u64,
    operations: Vec<Operation>,
    #[serde(default)]
    asset_sync: bool,
}
#[derive(Serialize)]
pub struct Snapshot {
    pub library: Library,
    pub root: String,
    pub revision: u64,
}
#[derive(Clone)]
pub struct DiskLibrary {
    root: PathBuf,
    pub data: Library,
    revision: u64,
    bytes: HashMap<String, Vec<u8>>,
    folder_identities: HashMap<String, [u64; 2]>,
}
#[cfg(feature = "app")]
#[derive(Default)]
pub struct FileLibrary(pub Mutex<Option<DiskLibrary>>, #[cfg(desktop)] Mutex<Option<external::Watch>>);
struct Imported {
    bytes: Vec<u8>,
    from: PathBuf,
}

fn now() -> f64 {
    millis(SystemTime::now())
}
fn millis(time: SystemTime) -> f64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as f64
}
fn id() -> String {
    Uuid::new_v4().to_string()
}
fn error(error: impl std::fmt::Display) -> String {
    format!("本地文件操作失败：{error}")
}
fn validate(library: &Library) -> Result<()> {
    let mut folders = HashSet::new();
    let mut notes = HashSet::new();
    if library.version != 1 || library.folders.is_empty() {
        return Err("笔记库格式无效，未覆盖原有文件".into());
    }
    for folder in &library.folders {
        if folder.id.is_empty()
            || folder.id.len() > 500
            || folder.name.trim().is_empty()
            || !folders.insert(&folder.id)
        {
            return Err("文件夹元数据无效，未覆盖原有文件".into());
        }
    }
    for note in &library.notes {
        if note.id.is_empty()
            || note.id.len() > 500
            || !notes.insert(&note.id)
            || !folders.contains(&note.folder_id)
            || [note.created_at, note.updated_at]
                .iter()
                .any(|n| !n.is_finite() || *n < 0. || *n > 8_640_000_000_000_000.)
        {
            return Err("笔记元数据无效，未覆盖原有文件".into());
        }
    }
    Ok(())
}
fn allowed(path: &Path) -> bool {
    path.extension()
        .and_then(|x| x.to_str())
        .is_some_and(|x| ["md", "markdown", "txt"].contains(&x.to_lowercase().as_str()))
}
fn name(title: &str) -> Result<String> {
    let value = title
        .trim_matches(|c: char| c.is_whitespace() && c != '\n' && c != '\r')
        .replace(':', "-")
        .replace('/', ":");
    if value.chars().any(|c| c.is_control())
        || value.starts_with('.')
        || cfg!(windows) && value.contains('\\')
    {
        return Err("文件名称不能以点开头，或包含换行、反斜杠及控制字符".into());
    }
    Ok(value)
}
fn display_title(path: &str) -> String {
    let stem = Path::new(path)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    if Uuid::parse_str(&stem).is_ok() {
        "无标题".into()
    } else {
        stem.replace(':', "/")
    }
}
fn relative(folder: &str, filename: &str) -> String {
    if folder.is_empty() {
        filename.into()
    } else {
        format!("{folder}/{filename}")
    }
}
fn checked(root: &Path, value: &str) -> Result<PathBuf> {
    if value.is_empty()
        || Path::new(value)
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("笔记文件路径无效".into());
    }
    let path = root.join(value);
    let mut current = root.to_path_buf();
    for part in Path::new(value).components() {
        current.push(part);
        if fs::symlink_metadata(&current).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return Err("请直接选择真实文件夹，不能写入符号链接".into());
        }
    }
    Ok(path)
}
fn atomic(path: &Path, bytes: &[u8], existing: bool) -> Result<()> {
    let mut temporary =
        tempfile::NamedTempFile::new_in(path.parent().ok_or("文件路径无效")?).map_err(error)?;
    temporary.write_all(bytes).map_err(error)?;
    temporary.as_file().sync_all().map_err(error)?;
    if existing {
        copy_metadata(path, temporary.path())?;
        temporary.as_file().sync_all().map_err(error)?;
        exchange(temporary.path(), path).map_err(error)?;
    } else {
        temporary.persist_noclobber(path).map_err(error)?;
    }
    Ok(())
}
fn copy_metadata(from: &Path, to: &Path) -> Result<()> {
    #[cfg(target_vendor = "apple")]
    {
        use std::os::darwin::fs::FileTimesExt;
        use std::os::unix::ffi::OsStrExt;
        let created = fs::metadata(from)
            .map_err(error)?
            .created()
            .map_err(error)?;
        let target_file = fs::File::open(to).map_err(error)?;
        unsafe extern "C" {
            fn copyfile(
                from: *const std::ffi::c_char,
                to: *const std::ffi::c_char,
                state: *mut std::ffi::c_void,
                flags: u32,
            ) -> i32;
        }
        let from = std::ffi::CString::new(from.as_os_str().as_bytes()).map_err(error)?;
        let to = std::ffi::CString::new(to.as_os_str().as_bytes()).map_err(error)?;
        // Same COPYFILE_METADATA flags as MiaoYan Note.writeReplacingExistingFile.
        if unsafe { copyfile(from.as_ptr(), to.as_ptr(), std::ptr::null_mut(), 7) } != 0 {
            return Err(error(io::Error::last_os_error()));
        }
        target_file
            .set_times(fs::FileTimes::new().set_created(created))
            .map_err(error)?;
    }
    #[cfg(not(target_vendor = "apple"))]
    fs::set_permissions(to, fs::metadata(from).map_err(error)?.permissions()).map_err(error)?;
    Ok(())
}
// Like Note.swapExistingFile: both paths must still exist at the instant of replacement.
#[cfg(unix)]
fn exchange(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let a = std::ffi::CString::new(from.as_os_str().as_bytes())?;
    let b = std::ffi::CString::new(to.as_os_str().as_bytes())?;
    #[cfg(target_vendor = "apple")]
    let result = unsafe { libc::renamex_np(a.as_ptr(), b.as_ptr(), libc::RENAME_SWAP) };
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            a.as_ptr(),
            libc::AT_FDCWD,
            b.as_ptr(),
            libc::RENAME_EXCHANGE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}
#[cfg(windows)]
fn exchange(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    let a: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let b: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    let ok = unsafe {
        windows_sys::Win32::Storage::FileSystem::ReplaceFileW(
            b.as_ptr(),
            a.as_ptr(),
            std::ptr::null(),
            0,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if ok != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}
fn modified(path: &Path, millis: f64) -> Result<()> {
    if !millis.is_finite() || millis < 0.0 || millis > 8_640_000_000_000_000.0 {
        return Err("笔记时间无效".into());
    }
    if fs::metadata(path)
        .and_then(|meta| meta.modified())
        .is_ok_and(|time| self::millis(time) == millis)
    {
        return Ok(());
    }
    #[cfg(windows)]
    let file = {
        use std::os::windows::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .access_mode(0x0100)
            .open(path)
            .map_err(error)?
    };
    #[cfg(not(windows))]
    let file = fs::File::open(path).map_err(error)?;
    file.set_times(
        fs::FileTimes::new()
            .set_modified(UNIX_EPOCH + std::time::Duration::from_millis(millis as u64)),
    )
    .map_err(error)
}
fn move_exclusive(from: &Path, to: &Path) -> Result<()> {
    if from == to {
        return Ok(());
    }
    #[cfg(target_vendor = "apple")]
    {
        use std::os::unix::ffi::OsStrExt;
        let a = std::ffi::CString::new(from.as_os_str().as_bytes()).map_err(error)?;
        let b = std::ffi::CString::new(to.as_os_str().as_bytes()).map_err(error)?;
        let result = unsafe { libc::renamex_np(a.as_ptr(), b.as_ptr(), libc::RENAME_EXCL) };
        if result == 0 {
            return Ok(());
        }
        // APFS case-only names refer to the same file; normal rename keeps the spelling.
        if from.to_string_lossy().to_lowercase() == to.to_string_lossy().to_lowercase()
            && same_file(from, to)
        {
            return fs::rename(from, to).map_err(error);
        }
        return Err(error(io::Error::last_os_error()));
    }
    #[cfg(target_os = "android")]
    {
        use std::os::unix::ffi::OsStrExt;
        let from = std::ffi::CString::new(from.as_os_str().as_bytes()).map_err(error)?;
        let to = std::ffi::CString::new(to.as_os_str().as_bytes()).map_err(error)?;
        // Android's app sandbox forbids hard links, including links within app data.
        if unsafe {
            libc::syscall(libc::SYS_renameat2, libc::AT_FDCWD, from.as_ptr(), libc::AT_FDCWD, to.as_ptr(), libc::RENAME_NOREPLACE)
        } == 0 {
            return Ok(());
        }
        return Err(error(io::Error::last_os_error()));
    }
    #[cfg(not(any(target_vendor = "apple", target_os = "android")))]
    {
        if from.to_string_lossy().to_lowercase() == to.to_string_lossy().to_lowercase()
            && same_file(from, to)
        {
            return fs::rename(from, to).map_err(error);
        }
        fs::hard_link(from, to).map_err(error)?;
        if let Err(reason) = fs::remove_file(from) {
            let _ = fs::remove_file(to);
            return Err(error(reason));
        }
        Ok(())
    }
}
fn same_file(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(a), Ok(b)) = (fs::metadata(a), fs::metadata(b)) {
            return a.dev() == b.dev() && a.ino() == b.ino();
        }
    }
    #[cfg(windows)]
    {
        return fs::canonicalize(a)
            .ok()
            .zip(fs::canonicalize(b).ok())
            .is_some_and(|(a, b)| a == b);
    }
    false
}
fn file_identity(path: &Path) -> Option<[u64; 2]> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        fs::metadata(path).ok().map(|m| [m.dev(), m.ino()])
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}
fn decode(bytes: &[u8]) -> (String, bool) {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return (text.into(), false);
    }
    if let Some((encoding, skip)) = encoding_rs::Encoding::for_bom(bytes) {
        let (text, bad) = encoding.decode_without_bom_handling(&bytes[skip..]);
        return (text.into_owned(), bad);
    }
    let mut valid = 0;
    let mut invalid = 0;
    let mut rest = bytes;
    while !rest.is_empty() {
        match std::str::from_utf8(rest) {
            Ok(text) => {
                valid += text.chars().filter(|c| !c.is_ascii()).count();
                break;
            }
            Err(e) => {
                valid += std::str::from_utf8(&rest[..e.valid_up_to()])
                    .unwrap()
                    .chars()
                    .filter(|c| !c.is_ascii())
                    .count();
                invalid += 1;
                rest = &rest
                    [e.valid_up_to() + e.error_len().unwrap_or(rest.len() - e.valid_up_to())..];
            }
        }
    }
    if valid > 0 && valid >= 4 * invalid {
        return (String::from_utf8_lossy(bytes).into_owned(), true);
    }
    let mut detector = chardetng::EncodingDetector::new();
    detector.feed(bytes, true);
    let encoding = detector.guess(None, true);
    let (text, bad) = encoding.decode_without_bom_handling(bytes);
    (text.into_owned(), bad || valid > 0 && valid >= invalid)
}
fn unique(
    root: &Path,
    folder: &str,
    title: &str,
    ext: &str,
    occupied: &mut HashSet<String>,
    intent: &str,
) -> Result<String> {
    use unicode_segmentation::UnicodeSegmentation;
    let raw = name(title)?;
    let raw = if raw.is_empty() {
        Uuid::new_v4().to_string().to_uppercase()
    } else {
        raw
    };
    let regex = regex::Regex::new(r"(.+)\s(\d)+").unwrap();
    let mut current = raw.clone();
    let mut number = 0;
    loop {
        let candidate = format!("{current}.{ext}");
        let path = relative(folder, &candidate);
        if !occupied.contains(&path.to_lowercase())
            && fs::symlink_metadata(root.join(&path)).is_err()
        {
            checked(root, &path)?;
            occupied.insert(path.to_lowercase());
            return Ok(path);
        }
        number += 1;
        if intent == "import" {
            current = if number == 1 {
                format!("{raw} Copy")
            } else {
                format!("{raw} Copy {number}")
            };
        } else {
            // MiaoYan passes String.count as an NSRange length, in UTF-16 units.
            let limit = current.graphemes(true).count();
            let mut units = 0;
            let prefix: String = current
                .chars()
                .take_while(|c| {
                    units += c.len_utf16();
                    units <= limit
                })
                .collect();
            let base = regex
                .captures(&prefix)
                .and_then(|c| c.get(1))
                .map_or(current.as_str(), |m| m.as_str());
            current = name(&format!("{base} {number}"))?;
        }
    }
}

fn duplicate_name(root: &Path, folder: &str, original: &str, ext: &str) -> String {
    let base = original
        .strip_suffix(" Copy")
        .or_else(|| original.split_once(" Copy ").map(|p| p.0))
        .unwrap_or(original);
    let mut number = 1;
    loop {
        let candidate = if number == 1 {
            format!("{base} Copy")
        } else {
            format!("{base} Copy {number}")
        };
        if !root
            .join(relative(folder, &format!("{candidate}.{ext}")))
            .exists()
        {
            return candidate;
        }
        number += 1;
    }
}

impl Operation {
    fn check(&self, root: &Path) -> Result<()> {
        match self {
            Self::Create { path, .. } => {
                if checked(root, path)?.exists() {
                    return Err("目标文件已存在，未覆盖".into());
                }
            }
            Self::Write {
                path,
                before: expected,
                ..
            }
            | Self::Move {
                from: path,
                bytes: expected,
                ..
            } => {
                if fs::read(checked(root, path)?).map_err(error)? != *expected {
                    return Err("磁盘文件已被外部修改，未覆盖，请重新读取笔记文件夹".into());
                }
                if matches!(self, Self::Write { .. })
                    && fs::metadata(checked(root, path)?)
                        .map_err(error)?
                        .permissions()
                        .readonly()
                {
                    return Err("文件为只读状态，未覆盖".into());
                }
            }
        }
        Ok(())
    }
    fn apply(&self, root: &Path) -> Result<()> {
        self.check(root)?;
        match self {
            Self::Create {
                path,
                bytes,
                staged,
                modified: time,
                ..
            } => {
                let path = checked(root, path)?;
                let stage = checked(root, staged)?;
                if fs::read(&stage).map_err(error)? != *bytes {
                    return Err("暂存笔记内容发生变化，已停止提交".into());
                }
                #[cfg(target_os = "android")]
                move_exclusive(&stage, &path)?;
                #[cfg(not(target_os = "android"))]
                fs::hard_link(stage, &path).map_err(error)?;
                modified(&path, *time)
            }
            Self::Move { from, to, bytes } => {
                let from = checked(root, from)?;
                let to = checked(root, to)?;
                if fs::read(&from).map_err(error)? != *bytes {
                    return Err("磁盘文件已被外部修改，未覆盖，请重新打开笔记库".into());
                }
                move_exclusive(&from, &to)
            }
            Self::Write {
                path,
                before,
                after,
                modified: time,
            } => {
                let path = checked(root, path)?;
                if fs::read(&path).map_err(error)? != *before {
                    return Err("磁盘文件已被外部修改，未覆盖，请重新打开笔记库".into());
                }
                atomic(&path, after, true)?;
                modified(&path, *time)
            }
        }
    }
    fn undo(&self, root: &Path) -> Result<()> {
        match self {
            Self::Create { path, staged, identity, .. } => {
                let path = checked(root, path)?;
                if path.exists()
                    && (same_file(&path, &checked(root, staged)?)
                        || identity.is_some() && file_identity(&path) == *identity)
                {
                    fs::remove_file(path).map_err(error)?;
                }
                Ok(())
            }
            Self::Move { from, to, bytes } => {
                let from = checked(root, from)?;
                let to = checked(root, to)?;
                if same_file(&from, &to) {
                    fs::rename(&to, &from).map_err(error)?;
                } else if !from.exists() && to.exists() {
                    if fs::read(&to).map_err(error)? != *bytes {
                        return Err("恢复时发现外部修改，已保留文件和事务日志".into());
                    }
                    move_exclusive(&to, &from)?;
                } else if from.exists() && to.exists() {
                    return Err("恢复路径被其他文件占用，已保留文件和事务日志".into());
                }
                Ok(())
            }
            Self::Write {
                path,
                before,
                after,
                ..
            } => {
                let path = checked(root, path)?;
                let current = fs::read(&path).map_err(error)?;
                if current == *after {
                    atomic(&path, before, true)
                } else if current == *before {
                    Ok(())
                } else {
                    Err("恢复时发现外部修改，已保留文件和事务日志".into())
                }
            }
        }
    }
    fn cleanup(&self, root: &Path) -> Result<()> {
        if let Self::Create { staged, .. } = self {
            let path = checked(root, staged)?;
            if path.exists() {
                fs::remove_file(path).map_err(error)?;
            }
        }
        Ok(())
    }
}

impl DiskLibrary {
    pub fn open(root: PathBuf, seed: Option<Library>) -> Result<Self> {
        fs::create_dir_all(&root).map_err(error)?;
        let root = fs::canonicalize(root).map_err(error)?;
        let meta = checked(&root, META)?;
        fs::create_dir_all(meta).map_err(error)?;
        let scan_lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(checked(&root, ".miaoyan/write.lock")?)
            .map_err(error)?;
        try_lock(&scan_lock)
            .map_err(|_| "笔记库正在被另一实例写入，请稍后重试".to_string())?;
        folders::recover(&root)?;
        let index_path = checked(&root, INDEX)?;
        let journal = checked(&root, JOURNAL)?;
        let mut index = if index_path.exists() {
            Some(
                serde_json::from_slice::<Index>(&fs::read(&index_path).map_err(error)?)
                    .map_err(error)?,
            )
        } else {
            None
        };
        if let Some(index) = &index {
            validate(&index.library)?;
        }
        if journal.exists() {
            let transaction: Transaction =
                serde_json::from_slice(&fs::read(&journal).map_err(error)?).map_err(error)?;
            let previous = index.as_ref().map_or(0, |index| index.generation);
            if transaction.generation == 0
                || (transaction.generation != previous
                    && transaction.generation != previous.saturating_add(1))
            {
                return Err("事务版本与笔记库不匹配，已保留原文件，请检查事务日志".into());
            }
            for operation in &transaction.operations {
                let note_path = |path: &str| -> Result<()> {
                    checked(&root, path)?;
                    if transaction.asset_sync && (assets::valid_path(path) || assets::trash_path(path)) { return Ok(()); }
                    if !allowed(Path::new(path))
                        || Path::new(path)
                            .components()
                            .any(|part| part.as_os_str().to_string_lossy().starts_with('.'))
                    {
                        return Err("恢复日志中的笔记路径无效，未修改文件".into());
                    }
                    Ok(())
                };
                match operation {
                    Operation::Create { path, staged, .. } => {
                        note_path(path)?;
                        let token = staged
                            .strip_prefix(".miaoyan/staged-")
                            .ok_or("恢复日志中的暂存路径无效")?;
                        if Uuid::parse_str(token).is_err() {
                            return Err("恢复日志中的暂存文件无效".into());
                        }
                        checked(&root, staged)?;
                    }
                    Operation::Move { from, to, .. } => {
                        note_path(from)?;
                        note_path(to)?;
                    }
                    Operation::Write { path, .. } => note_path(path)?,
                }
            }
            if index
                .as_ref()
                .is_none_or(|index| index.generation != transaction.generation)
            {
                for operation in transaction.operations.iter().rev() {
                    operation.undo(&root)?;
                }
            }
            for operation in &transaction.operations {
                operation.cleanup(&root)?;
            }
            fs::remove_file(&journal).map_err(error)?;
        }
        let empty = Library {
            version: 1,
            folders: vec![Folder {
                id: "default".into(),
                name: "备忘录".into(),
                path: String::new(),
            }],
            notes: vec![],
            active_id: String::new(),
            sync: None,
        };
        let mut store = Self {
            root,
            data: index
                .take()
                .map_or_else(|| empty.clone(), |index| index.library),
            revision: 0,
            bytes: HashMap::new(),
            folder_identities: HashMap::new(),
        };
        if index_path.exists() {
            store.revision = serde_json::from_slice::<Index>(&fs::read(index_path).map_err(error)?)
                .map_err(error)?
                .generation;
        }
        store.scan()?;
        if store.revision == 0 {
            if let Some(seed) = seed {
                let backup = checked(&store.root, ".miaoyan/localStorage-backup.json")?;
                if !backup.exists() {
                    atomic(
                        &backup,
                        &serde_json::to_vec_pretty(&seed).map_err(error)?,
                        false,
                    )?;
                }
                let mut merged = seed;
                for folder in &merged.folders {
                    let expected = if folder.id == "default" {
                        String::new()
                    } else {
                        name(&folder.name)?
                    };
                    if let Some(existing) =
                        store.data.folders.iter_mut().find(|f| f.path == expected)
                    {
                        let previous = existing.id.clone();
                        existing.id = folder.id.clone();
                        for note in &mut store.data.notes {
                            if note.folder_id == previous {
                                note.folder_id = folder.id.clone();
                            }
                        }
                    }
                }
                let known: HashSet<_> = merged.folders.iter().map(|f| f.id.clone()).collect();
                merged.folders.extend(
                    store
                        .data
                        .folders
                        .iter()
                        .filter(|f| !known.contains(&f.id))
                        .cloned(),
                );
                merged.notes.extend(store.data.notes.clone());
                drop(scan_lock);
                store.commit(merged, "migration")?;
            } else {
                store.persist_index(&store.data, 1)?;
                store.revision = 1;
            }
        } else {
            store.revision += 1;
            store.persist_index(&store.data, store.revision)?;
        }
        Ok(store)
    }
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            library: self.data.clone(),
            root: self.root.to_string_lossy().into(),
            revision: self.revision,
        }
    }
    fn persist_index(&self, library: &Library, generation: u64) -> Result<()> {
        let mut metadata = library.clone();
        for note in &mut metadata.notes {
            note.source.clear();
        }
        let bytes = serde_json::to_vec_pretty(&Index {
            generation,
            library: metadata,
        })
        .map_err(error)?;
        let path = checked(&self.root, INDEX)?;
        atomic(&path, &bytes, path.exists())
    }
    fn remember_folder_identities(&mut self) {
        let ids: HashSet<_> = self.data.folders.iter().map(|folder| &folder.id).collect();
        self.folder_identities.retain(|id, _| ids.contains(id));
        for folder in &self.data.folders {
            // Selection/pin commits may race a Finder move before the rescan.
            // Keep the registered identity until that scan reconciles its new path.
            if self.folder_identities.contains_key(&folder.id) {
                continue;
            }
            let path = self.root.join(&folder.path);
            if fs::symlink_metadata(&path)
                .is_ok_and(|meta| meta.is_dir() && !meta.file_type().is_symlink())
            {
                if let Some(identity) = file_identity(&path) {
                    self.folder_identities.insert(folder.id.clone(), identity);
                }
            }
        }
    }
    fn scan(&mut self) -> Result<()> {
        let mut entries = Vec::new();
        scan(&self.root, &mut entries, false)?;
        let root = self
            .data
            .folders
            .iter()
            .find(|folder| folder.path.is_empty())
            .cloned()
            .unwrap_or(Folder {
                id: "default".into(),
                name: "备忘录".into(),
                path: String::new(),
            });
        let mut folders = vec![root];
        let identities: HashMap<_, _> = self
            .folder_identities
            .iter()
            .map(|(id, identity)| (*identity, id))
            .collect();
        let mut directories = vec![];
        collect_directories(&self.root, &mut directories)?;
        for directory in directories {
            let path = directory
                .strip_prefix(&self.root)
                .map_err(error)?
                .to_string_lossy()
                .replace('\\', "/");
            checked(&self.root, &path)?;
            let identity = file_identity(&directory);
            let existing = identity
                .and_then(|identity| identities.get(&identity))
                .and_then(|id| self.data.folders.iter().find(|folder| &folder.id == *id))
                .or_else(|| {
                    self.data.folders.iter().find(|folder| {
                        folder.path == path && !self.folder_identities.contains_key(&folder.id)
                    })
                });
            folders.push(Folder {
                id: existing.map_or_else(id, |folder| folder.id.clone()),
                name: directory
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .replace(':', "/"),
                path,
            });
        }
        // Storage.relocateFolder preserves every descendant's identity. Rebase
        // direct owners before matching note paths, even if the old path was reused.
        let paths: HashMap<_, _> = folders
            .iter()
            .map(|folder| (&folder.id, &folder.path))
            .collect();
        let mut old: HashMap<_, _> = self
            .data
            .notes
            .iter()
            .filter_map(|note| {
                let mut note = note.clone();
                if !note.trashed {
                    let parent = paths.get(&note.folder_id)?;
                    let filename = Path::new(&note.path).file_name()?.to_str()?;
                    note.path = relative(parent, filename);
                }
                Some((note.path.clone(), note))
            })
            .collect();
        let mut notes = vec![];
        self.bytes.clear();
        for path in entries {
            let relative = path
                .strip_prefix(&self.root)
                .map_err(error)?
                .to_string_lossy()
                .replace('\\', "/");
            let parent = Path::new(&relative)
                .parent()
                .unwrap_or(Path::new(""))
                .to_string_lossy()
                .into_owned();
            let trashed = parent == "Trash";
            let folder_id = if trashed {
                old.get(&relative)
                    .map(|n| n.folder_id.clone())
                    .unwrap_or_else(|| "default".into())
            } else {
                if folders.iter().all(|f| f.path != parent) {
                    folders.push(Folder {
                        id: id(),
                        name: parent
                            .rsplit('/')
                            .next()
                            .unwrap_or("备忘录")
                            .replace(':', "/"),
                        path: parent.clone(),
                    });
                }
                folders
                    .iter()
                    .find(|f| f.path == parent)
                    .unwrap()
                    .id
                    .clone()
            };
            let meta = fs::metadata(&path).map_err(error)?;
            let mut note = old.remove(&relative).unwrap_or_else(|| Note {
                id: id(),
                folder_id: folder_id.clone(),
                source: String::new(),
                created_at: millis(meta.created().unwrap_or(SystemTime::now())),
                updated_at: now(),
                title: None,
                pinned: false,
                trashed,
                conflict_of: None,
                path: relative.clone(),
                read_only: false,
                filename: None,
            });
            match fs::read(&path) {
                Ok(bytes) => {
                    let (source, bad) = decode(&bytes);
                    note.source = source;
                    note.read_only = bad;
                    self.bytes.insert(note.id.clone(), bytes);
                }
                Err(_) => {
                    note.source.clear();
                    note.read_only = true;
                }
            }
            note.folder_id = folder_id;
            note.trashed = trashed;
            note.path = relative.clone();
            note.title = Some(display_title(&relative));
            note.filename = path.file_name().map(|n| n.to_string_lossy().into());
            note.updated_at = millis(meta.modified().map_err(error)?);
            notes.push(note);
        }
        folders.retain(|f| f.path.is_empty() || self.root.join(&f.path).is_dir());
        for note in &mut notes {
            if !folders.iter().any(|f| f.id == note.folder_id) {
                note.folder_id = folders
                    .iter()
                    .find(|f| f.path.is_empty())
                    .unwrap()
                    .id
                    .clone();
            }
        }
        if !notes.iter().any(|n| n.id == self.data.active_id) {
            self.data.active_id = notes.first().map_or(String::new(), |n| n.id.clone());
        }
        self.data.folders = folders;
        self.data.notes = notes;
        self.remember_folder_identities();
        Ok(())
    }
    pub fn commit(&mut self, next: Library, intent: &str) -> Result<()> {
        self.commit_bytes(next, intent, &HashMap::new(), None)
    }
    fn commit_bytes(
        &mut self,
        mut next: Library,
        intent: &str,
        imports: &HashMap<String, Imported>,
        assets: Option<&assets::AssetSync>,
    ) -> Result<()> {
        let lock_path = checked(&self.root, ".miaoyan/write.lock")?;
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)
            .map_err(error)?;
        try_lock(&lock)
            .map_err(|_| "另一个窗口正在写入此笔记库，请稍后重试".to_string())?;
        let index_path = checked(&self.root, INDEX)?;
        if index_path.exists() {
            let index: Index =
                serde_json::from_slice(&fs::read(index_path).map_err(error)?).map_err(error)?;
            if index.generation != self.revision {
                return Err("磁盘笔记库已被另一实例更新，请重新打开".into());
            }
        }
        if checked(&self.root, JOURNAL)?.exists() || checked(&self.root, folders::FOLDER_JOURNAL)?.exists() {
            return Err("存在未恢复的文件事务，请重新打开笔记库".into());
        }
        validate(&next)?;
        if intent == "sync" {
            for old in &self.data.notes {
                if !next.notes.iter().any(|n| n.id == old.id) {
                    let mut old = old.clone();
                    old.trashed = true;
                    next.notes.push(old);
                }
            }
        }
        let mut operations = match assets { Some(sync) if intent == "sync" => assets::operations(&self.root, sync)?, Some(_) => return Err("附件只能随同步事务提交".into()), None => vec![] };
        let mut occupied: HashSet<String> = HashSet::new();
        for folder in &mut next.folders {
            if let Some(old) = self.data.folders.iter().find(|f| f.id == folder.id) {
                folder.path = old.path.clone();
            } else {
                let directory = if intent == "sync" && !folder.path.is_empty() {
                    if folder.path.split('/').any(|part| part.is_empty() || part == "." || part == ".." || part.starts_with('.') || part.contains(['\\', ':'])) { return Err("远端文件夹路径无效".into()); }
                    folder.path.clone()
                } else { name(&folder.name)? };
                if directory.is_empty() {
                    return Err("请输入文件夹名称".into());
                }
                let path = checked(&self.root, &directory)?;
                if path.exists() && (intent != "sync" || !path.is_dir() || self.data.folders.iter().any(|f| f.path.eq_ignore_ascii_case(&directory))) {
                    return Err("已有同名文件夹或文件".into());
                }
                fs::create_dir_all(&path).map_err(error)?;
                folder.path = directory;
            }
        }
        let mut ids = HashSet::new();
        for note in &mut next.notes {
            if !ids.insert(note.id.clone()) {
                return Err("笔记 ID 重复".into());
            }
            let folder = next
                .folders
                .iter()
                .find(|f| f.id == note.folder_id)
                .ok_or("笔记文件夹不存在")?;
            let target_folder = if note.trashed { "Trash" } else { &folder.path };
            if note.trashed {
                fs::create_dir_all(checked(&self.root, "Trash")?).map_err(error)?;
            }
            let old = self.data.notes.iter().find(|n| n.id == note.id);
            if let Some(old) = old {
                let changed = note.source != old.source;
                if changed && old.read_only {
                    return Err("文件内容损坏或不可读，已阻止保存".into());
                }
                let moving = note.folder_id != old.folder_id || note.trashed != old.trashed;
                let renaming = note.title != old.title && !moving;
                if (changed || moving || renaming) && !self.bytes.contains_key(&old.id) {
                    return Err("文件不可读，未覆盖原文件".into());
                }
                let bytes = self.bytes.get(&old.id).map(Vec::as_slice).unwrap_or(&[]);
                let mut path = old.path.clone();
                if moving {
                    path = unique(
                        &self.root,
                        target_folder,
                        Path::new(&old.path)
                            .file_stem()
                            .unwrap()
                            .to_string_lossy()
                            .replace(':', "/")
                            .as_ref(),
                        Path::new(&old.path)
                            .extension()
                            .unwrap()
                            .to_str()
                            .ok_or("文件扩展名无效")?,
                        &mut occupied,
                        "create",
                    )?;
                } else if renaming {
                    let title = name(note.title.as_deref().unwrap_or(""))?;
                    if title.is_empty() {
                        return Err("文件名称不能为空".into());
                    }
                    path = relative(
                        target_folder,
                        &format!(
                            "{}.{}",
                            title,
                            Path::new(&old.path).extension().unwrap().to_string_lossy()
                        ),
                    );
                    let destination = checked(&self.root, &path)?;
                    if path != old.path
                        && destination.exists()
                        && !(path.to_lowercase() == old.path.to_lowercase()
                            && same_file(&checked(&self.root, &old.path)?, &destination))
                    {
                        return Err("该文件夹中已有同名笔记，未覆盖".into());
                    }
                }
                if path != old.path {
                    operations.push(Operation::Move {
                        from: old.path.clone(),
                        to: path.clone(),
                        bytes: bytes.to_vec(),
                    });
                }
                if changed {
                    operations.push(Operation::Write {
                        path: path.clone(),
                        before: bytes.to_vec(),
                        after: note.source.as_bytes().to_vec(),
                        modified: note.updated_at,
                    });
                }
                note.path = path;
                note.read_only = old.read_only;
            } else {
                let hint = if note.path.is_empty() {
                    note.filename.as_deref().unwrap_or("")
                } else {
                    &note.path
                };
                let mut title = if hint.is_empty() {
                    note.title.clone().unwrap_or_default()
                } else {
                    Path::new(hint)
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .replace(':', "/")
                };
                let ext = if hint.is_empty() {
                    "md"
                } else {
                    Path::new(hint)
                        .extension()
                        .and_then(|e| e.to_str())
                        .filter(|e| ["md", "markdown", "txt"].contains(&e.to_lowercase().as_str()))
                        .ok_or("不支持该文件类型")?
                };
                if let Some(source_id) = intent.strip_prefix("duplicate:") {
                    let original = self
                        .data
                        .notes
                        .iter()
                        .find(|n| n.id == source_id)
                        .ok_or("副本来源笔记不存在")?;
                    let source = Path::new(&original.path);
                    title = duplicate_name(
                        &self.root,
                        target_folder,
                        &source.file_stem().unwrap().to_string_lossy(),
                        source
                            .extension()
                            .unwrap()
                            .to_str()
                            .ok_or("文件扩展名无效")?,
                    );
                }
                note.path = unique(
                    &self.root,
                    target_folder,
                    &title,
                    ext,
                    &mut occupied,
                    intent,
                )?;
                let bytes = imports
                    .get(&note.id)
                    .map(|file| file.bytes.clone())
                    .unwrap_or_else(|| note.source.as_bytes().to_vec());
                operations.push(Operation::Create {
                    path: note.path.clone(),
                    bytes,
                    staged: format!(".miaoyan/staged-{}", id()),
                    modified: note.updated_at,
                    identity: None,
                });
                note.read_only = imports
                    .get(&note.id)
                    .is_some_and(|file| decode(&file.bytes).1);
            }
            note.title = Some(display_title(&note.path));
            note.filename = Path::new(&note.path)
                .file_name()
                .map(|n| n.to_string_lossy().into());
        }
        let mut destinations = HashSet::new();
        if next
            .notes
            .iter()
            .any(|note| !destinations.insert(note.path.to_lowercase()))
        {
            return Err("同一文件夹中有笔记名称冲突，未覆盖文件".into());
        }
        if self
            .data
            .notes
            .iter()
            .any(|old| !next.notes.iter().any(|n| n.id == old.id))
        {
            return Err("请使用废纸篓保留待移除笔记，不能直接删除本地文件".into());
        }
        if !next.notes.iter().any(|note| note.id == next.active_id) {
            next.active_id = next
                .notes
                .first()
                .map_or(String::new(), |note| note.id.clone());
        }
        let generation = self.revision + 1;
        let journal = checked(&self.root, JOURNAL)?;
        let mut transaction = Transaction {
            generation,
            operations,
            asset_sync: assets.is_some(),
        };
        for operation in &transaction.operations {
            if let Operation::Write { path, before, .. } = operation {
                if transaction.operations.iter().any(|op| matches!(op,Operation::Move {to,bytes,..} if to == path && bytes == before)) { continue; }
            }
            operation.check(&self.root)?;
        }
        for operation in &mut transaction.operations {
            if let Operation::Create {
                path,
                staged,
                bytes,
                identity,
                ..
            } = operation
            {
                let staged = checked(&self.root, staged)?;
                atomic(&staged, bytes, false)?;
                if let Some(file) = next
                    .notes
                    .iter()
                    .find(|n| n.path == *path)
                    .and_then(|n| imports.get(&n.id))
                {
                    copy_metadata(&file.from, &staged)?;
                }
                #[cfg(target_os = "android")]
                {
                    *identity = Some(file_identity(&staged).ok_or("无法读取暂存文件标识")?);
                }
                #[cfg(not(target_os = "android"))]
                let _ = identity;
            }
        }
        atomic(
            &journal,
            &serde_json::to_vec(&transaction).map_err(error)?,
            false,
        )?;
        let result = (|| {
            for operation in &transaction.operations {
                operation.apply(&self.root)?;
            }
            self.persist_index(&next, generation)
        })();
        if let Err(reason) = result {
            for operation in transaction.operations.iter().rev() {
                if let Err(recovery) = operation.undo(&self.root) {
                    return Err(format!("{reason}；{recovery}"));
                }
            }
            for operation in &transaction.operations {
                operation.cleanup(&self.root)?;
            }
            fs::remove_file(journal).map_err(error)?;
            return Err(reason);
        }
        self.data = next;
        self.revision = generation;
        self.remember_folder_identities();
        for operation in &transaction.operations {
            match operation {
                Operation::Create { path, bytes, .. }
                | Operation::Write {
                    path, after: bytes, ..
                } => {
                    if let Some(note) = self.data.notes.iter().find(|n| n.path == *path) {
                        self.bytes.insert(note.id.clone(), bytes.clone());
                    }
                }
                Operation::Move { .. } => {}
            }
            operation.cleanup(&self.root)?;
        }
        fs::remove_file(journal).map_err(error)?;
        Ok(())
    }
    pub fn import(&mut self, paths: &[PathBuf]) -> Result<usize> {
        let mut working = self.clone();
        let count = working.import_inner(paths)?;
        *self = working;
        Ok(count)
    }
    fn import_inner(&mut self, paths: &[PathBuf]) -> Result<usize> {
        let files = collect_imports(paths)?;
        if files.is_empty() {
            return Err("没有可导入的 Markdown 文件".into());
        }
        self.scan()?;
        let mut next = self.data.clone();
        let mut bytes = HashMap::new();
        let mut count = 0;
        let folder_id = next
            .folders
            .iter()
            .find(|folder| folder.path.is_empty())
            .ok_or("主文件夹不存在")?
            .id
            .clone();
        for path in files {
            let canonical = fs::canonicalize(&path).map_err(error)?;
            if let Some(note) = next
                .notes
                .iter()
                .find(|n| self.root.join(&n.path) == canonical)
            {
                next.active_id = note.id.clone();
                continue;
            }
            let raw = fs::read(&path).map_err(error)?;
            let meta = fs::metadata(&path).map_err(error)?;
            let (source, read_only) = decode(&raw);
            let new_id = id();
            next.active_id = new_id.clone();
            bytes.insert(
                new_id.clone(),
                Imported {
                    bytes: raw,
                    from: path.clone(),
                },
            );
            next.notes.push(Note {
                id: new_id,
                folder_id: folder_id.clone(),
                source,
                created_at: millis(meta.created().unwrap_or(SystemTime::now())),
                updated_at: millis(meta.modified().map_err(error)?),
                title: None,
                pinned: false,
                trashed: false,
                conflict_of: None,
                path: path
                    .file_name()
                    .ok_or("文件名无效")?
                    .to_string_lossy()
                    .into(),
                read_only,
                filename: None,
            });
            count += 1;
        }
        self.commit_bytes(next, "import", &bytes, None)?;
        Ok(count)
    }
}

fn attachment_only(path: &Path) -> Result<bool> {
    for entry in fs::read_dir(path).map_err(error)? {
        let entry = entry.map_err(error)?;
        if entry.file_type().map_err(error)?.is_file() && allowed(&entry.path()) {
            return Ok(false);
        }
    }
    Ok(true)
}
fn collect_directories(directory: &Path, result: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(directory).map_err(error)? {
        let entry = entry.map_err(error)?;
        let filename = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type().map_err(error)?.is_dir()
            && !filename.starts_with('.')
            && filename != "Trash"
        {
            if ["i", "files"].contains(&filename.as_str()) && attachment_only(&entry.path())? {
                continue;
            }
            result.push(entry.path());
            collect_directories(&entry.path(), result)?;
        }
    }
    result.sort();
    Ok(())
}
fn scan(directory: &Path, files: &mut Vec<PathBuf>, importing: bool) -> Result<()> {
    let mut entries = fs::read_dir(directory)
        .map_err(error)?
        .collect::<io::Result<Vec<_>>>()
        .map_err(error)?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let filename = entry.file_name().to_string_lossy().into_owned();
        let kind = entry.file_type().map_err(error)?;
        if filename.starts_with('.') || kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            if ["i", "files"].contains(&filename.as_str())
                && (importing || attachment_only(&entry.path())?)
            {
                continue;
            }
            if !importing && filename == "Trash" {
                for child in fs::read_dir(entry.path()).map_err(error)? {
                    let child = child.map_err(error)?;
                    if child.file_type().map_err(error)?.is_file() && allowed(&child.path()) {
                        files.push(child.path());
                    }
                }
            } else {
                scan(&entry.path(), files, importing)?;
            }
        } else if kind.is_file() && allowed(&entry.path()) {
            files.push(entry.path());
        }
    }
    Ok(())
}
pub fn collect_imports(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut files = vec![];
    for path in paths {
        if path.is_dir() {
            if !fs::symlink_metadata(path)
                .map_err(error)?
                .file_type()
                .is_symlink()
            {
                scan(path, &mut files, true)?;
            }
        } else if path.is_file() && allowed(path) {
            files.push(path.clone());
        }
    }
    let mut seen = HashSet::new();
    files.retain(|path| fs::canonicalize(path).is_ok_and(|path| seen.insert(path)));
    files.sort();
    Ok(files)
}

#[cfg(any(desktop, test))]
fn default_library_directory(documents: PathBuf) -> PathBuf {
    let current = documents.join("QingYu");
    let legacy = documents.join("MiaoYan Editor");
    if !current.exists() && legacy.is_dir() { legacy } else { current }
}

#[cfg(feature = "app")]
#[tauri::command]
pub async fn library_open(
    app: tauri::AppHandle,
    state: State<'_, FileLibrary>,
    seed: Option<Library>,
) -> Result<Snapshot> {
    let config = app
        .path()
        .app_config_dir()
        .map_err(error)?
        .join("library-location.json");
    let root = if config.exists() {
        serde_json::from_slice::<PathBuf>(&fs::read(config).map_err(error)?).map_err(error)?
    } else {
        #[cfg(desktop)]
        {
            default_library_directory(app.path().document_dir().map_err(error)?)
        }
        #[cfg(mobile)]
        {
            app.path().app_data_dir().map_err(error)?.join("notes")
        }
    };
    let opened = open_accessible_library(root, seed).await?;
    let mut lock = state.0.lock().map_err(error)?;
    *lock = Some(opened);
    Ok(lock.as_ref().unwrap().snapshot())
}

// Probe before taking the library lock or touching its index: OS permission
// prompts can leave directory opens waiting indefinitely.
#[cfg(any(feature = "app", test))]
fn check_directory_access(
    root: PathBuf,
    timeout: std::time::Duration,
    probe: impl FnOnce(&Path) -> io::Result<()> + Send + 'static,
) -> Result<()> {
    let (send, receive) = std::sync::mpsc::channel();
    let path = root.clone();
    std::thread::Builder::new().name("library-access".into()).spawn(move || {
        let _ = send.send(probe(&path));
    }).map_err(error)?;
    match receive.recv_timeout(timeout) {
        Ok(result) => result.map_err(|reason| format!("无法读取笔记目录 {}：{reason}。请检查目录访问权限，或在设置中更改笔记位置。", root.display())),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(format!("读取笔记目录超时：{}。请检查系统的文件夹访问提示，完成后使用“文件 → 重新读取笔记文件夹”，或在设置中更改笔记位置。原笔记未被覆盖。", root.display())),
        Err(reason) => Err(error(reason)),
    }
}

#[cfg(any(feature = "app", test))]
fn read_library_directory(root: &Path) -> io::Result<()> {
    let mut path = root;
    loop {
        match fs::read_dir(path) {
            Ok(_) => return Ok(()),
            Err(reason) if reason.kind() == io::ErrorKind::NotFound => {
                path = path.parent().ok_or(reason)?;
            }
            Err(reason) => return Err(reason),
        }
    }
}

#[cfg(feature = "app")]
async fn open_accessible_library(root: PathBuf, seed: Option<Library>) -> Result<DiskLibrary> {
    tauri::async_runtime::spawn_blocking(move || {
        check_directory_access(root.clone(), std::time::Duration::from_secs(10), read_library_directory)?;
        DiskLibrary::open(root, seed)
    }).await.map_err(error)?
}
#[cfg(feature = "app")]
#[tauri::command]
pub async fn library_commit(
    state: State<'_, FileLibrary>,
    next: Library,
    revision: u64,
    intent: String,
    assets: Option<assets::AssetSync>,
) -> Result<Snapshot> {
    let mut lock = state.0.lock().map_err(error)?;
    let store = lock.as_mut().ok_or("请先打开本地笔记库")?;
    if store.revision != revision {
        return Err("笔记库已发生变化，请重新打开后再操作".into());
    }
    store.commit_bytes(next, &intent, &HashMap::new(), assets.as_ref())?;
    Ok(store.snapshot())
}

#[cfg(feature = "app")]
#[tauri::command]
pub async fn library_import(
    window: tauri::WebviewWindow,
    state: State<'_, FileLibrary>,
    directory: bool,
) -> Result<Snapshot> {
    #[cfg(desktop)]
    {
        use tauri_plugin_dialog::DialogExt;
        let paths = tauri::async_runtime::spawn_blocking(move || {
            let dialog = window
                .dialog()
                .file()
                .set_parent(&window)
                .set_title("选择要导入的 Markdown 文件或文件夹");
            let selected = if directory {
                dialog.blocking_pick_folders()
            } else {
                dialog
                    .add_filter("Markdown", &["md", "markdown", "txt"])
                    .blocking_pick_files()
            };
            selected
                .unwrap_or_default()
                .into_iter()
                .map(|p| p.into_path().map_err(error))
                .collect::<Result<Vec<_>>>()
        })
        .await
        .map_err(error)??;
        let mut lock = state.0.lock().map_err(error)?;
        let store = lock.as_mut().ok_or("请先打开本地笔记库")?;
        if !paths.is_empty() {
            store.import(&paths)?;
        }
        Ok(store.snapshot())
    }
    #[cfg(mobile)]
    {
        let _ = (window, state, directory);
        Err("请先在桌面端导入本地笔记".into())
    }
}

#[cfg(feature = "app")]
#[tauri::command]
pub async fn library_choose(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    state: State<'_, FileLibrary>,
) -> Result<Snapshot> {
    #[cfg(desktop)]
    {
        use tauri_plugin_dialog::DialogExt;
        let window = app.get_webview_window("settings").filter(|settings| settings.is_focused().unwrap_or(false)).unwrap_or(window);
        let chosen = tauri::async_runtime::spawn_blocking(move || {
            window
                .dialog()
                .file()
                .set_parent(&window)
                .set_title("选择本地笔记文件夹")
                .blocking_pick_folder()
        })
        .await
        .map_err(error)?;
        if let Some(path) = chosen {
            let path = path.into_path().map_err(error)?;
            let opened = open_accessible_library(path, None).await?;
            let mut lock = state.0.lock().map_err(error)?;
            let config = app.path().app_config_dir().map_err(error)?;
            fs::create_dir_all(&config).map_err(error)?;
            let path = config.join("library-location.json");
            atomic(
                &path,
                &serde_json::to_vec(&opened.root).map_err(error)?,
                path.exists(),
            )?;
            *lock = Some(opened);
        }
        let lock = state.0.lock().map_err(error)?;
        Ok(lock.as_ref().ok_or("尚未打开本地笔记库")?.snapshot())
    }
    #[cfg(mobile)]
    {
        let _ = (window, app, state);
        Err("请先在桌面端选择本地笔记目录".into())
    }
}

#[cfg(feature = "app")]
#[tauri::command]
pub async fn library_reload(state: State<'_, FileLibrary>) -> Result<Snapshot> {
    let mut lock = state.0.lock().map_err(error)?;
    let current = lock.as_ref().ok_or("请先打开本地笔记库")?;
    let reopened = DiskLibrary::open(current.root.clone(), None)?;
    let snapshot = reopened.snapshot();
    *lock = Some(reopened);
    Ok(snapshot)
}

#[cfg(all(feature = "app", desktop))]
pub fn install_menu(app: &tauri::App) -> tauri::Result<()> {
    use tauri::{
        menu::{MenuItem, PredefinedMenuItem, Submenu},
        Emitter,
    };
    let menu = app
        .menu()
        .unwrap_or(tauri::menu::Menu::default(app.handle())?);
    let existing = menu.items()?.into_iter().find(|entry| {
        entry
            .as_submenu()
            .is_some_and(|s| s.text().is_ok_and(|t| t == "File" || t == "文件"))
    });
    let files = if let Some(entry) = existing {
        entry.as_submenu().unwrap().clone()
    } else {
        let files = Submenu::new(app, "文件", true)?;
        menu.append(&files)?;
        files
    };
    files.append(&PredefinedMenuItem::separator(app)?)?;
    for (id, text, key) in [
        ("workspace-open", "打开文件…", Some("CmdOrCtrl+O")),
        ("workspace-close", "返回笔记库", Some("CmdOrCtrl+Shift+W")),
        ("library-choose", "更改笔记库位置…", None),
        ("library-import", "导入文件…", None),
        ("library-import-folder", "导入文件夹…", None),
        (
            "library-reload",
            "重新读取笔记文件夹",
            None,
        ),
    ] {
        files.append(&MenuItem::with_id(app, id, text, true, key)?)?;
    }
    app.set_menu(menu)?;
    app.on_menu_event(|app, event| {
        if event.id().as_ref().starts_with("library-") || event.id().as_ref().starts_with("workspace-") {
            let target = if event.id().as_ref().starts_with("workspace-") {
                app.webview_windows().into_values().find(|window| window.is_focused().unwrap_or(false) && workspace::is_document(app, window.label())).map(|window| window.label().to_string()).unwrap_or_else(|| "main".into())
            } else { "main".into() };
            let _ = app.emit_to(&target, "library-command", event.id().as_ref());
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests;
