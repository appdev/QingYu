use super::*;

#[derive(Default, Serialize)]
pub struct StorageUsage {
    notes: u64,
    files: u64,
    trash: u64,
}

pub(super) struct UsageTarget {
    root: PathBuf,
}

impl UsageTarget {
    pub(super) fn new(store: &DiskLibrary) -> Self {
        Self {
            root: store.root.clone(),
        }
    }

    pub(super) fn measure(self) -> Result<StorageUsage> {
        let Self { root } = self;
        let mut usage = StorageUsage::default();
        let mut notes = Vec::new();
        scan(&root, &mut notes, false)?;
        let deleted_notes = root.join("Trash");
        for path in notes {
            let meta = fs::symlink_metadata(&path);
            let size = match meta {
                Ok(meta) if meta.is_file() => meta.len(),
                Ok(_) => continue,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(reason) => return Err(error(reason)),
            };
            if path.parent() == Some(deleted_notes.as_path()) {
                usage.trash += size;
            } else {
                usage.notes += size;
            }
        }
        usage.notes += history::archived_bytes(&root)?;
        assets::visit_asset_files(&root, &root, &mut |path, meta| {
            if path.starts_with("Trash/") {
                usage.trash += meta.len();
            } else {
                usage.files += meta.len();
            }
            Ok(())
        })?;
        let deleted = checked(&root, ".miaoyan/asset-trash")?;
        let entries = match fs::read_dir(deleted) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(usage),
            Err(reason) => return Err(error(reason)),
        };
        for entry in entries {
            let entry = entry.map_err(error)?;
            if !entry.file_type().map_err(error)?.is_dir()
                || !entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| Uuid::parse_str(name).is_ok())
            {
                continue;
            }
            let path = entry.path();
            assets::visit_asset_files(&path, &path, &mut |_, meta| {
                usage.trash += meta.len();
                Ok(())
            })?;
        }
        Ok(usage)
    }
}

#[cfg(feature = "app")]
#[tauri::command]
pub async fn library_storage_usage(
    state: State<'_, FileLibrary>,
    window: tauri::WebviewWindow,
) -> Result<StorageUsage> {
    if !matches!(window.label(), "main" | "settings") {
        return Err("请在笔记库窗口查看存储用量".into());
    }
    let target = UsageTarget::new(
        state
            .0
            .lock()
            .map_err(error)?
            .as_ref()
            .ok_or("请先打开笔记库")?,
    );
    tauri::async_runtime::spawn_blocking(move || target.measure())
        .await
        .map_err(error)?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_actual_file_lengths_without_reading_large_attachments_or_counting_credentials() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::create_dir(root.join("Trash")).unwrap();
        fs::write(root.join("old.md"), [0xff, 0xfe, b'A', 0]).unwrap();
        fs::write(root.join("Trash/removed.md"), "删").unwrap();
        fs::write(root.join("Trash/photo.png"), [0; 5]).unwrap();
        fs::write(root.join("s3-connection.json"), [0; 12]).unwrap();
        fs::create_dir(root.join(".attachments")).unwrap();
        fs::write(root.join(".attachments/reference.txt"), "A").unwrap();
        let file = fs::File::create(root.join("large.bin")).unwrap();
        file.set_len((assets::MAX_FILE + 1) as u64).unwrap();
        let store = DiskLibrary::open(root.into(), None).unwrap();
        let deleted = root.join(".miaoyan/asset-trash").join(id());
        fs::create_dir_all(&deleted).unwrap();
        fs::write(deleted.join("old-image.png"), [0; 6]).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("large.bin", root.join("alias.bin")).unwrap();
        let result = UsageTarget::new(&store).measure().unwrap();
        assert_eq!(result.notes, 4);
        assert_eq!(result.files, (assets::MAX_FILE + 2) as u64);
        assert_eq!(result.trash, 14);
    }
}
