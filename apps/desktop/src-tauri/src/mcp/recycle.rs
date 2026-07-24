use std::{io::ErrorKind, path::Path};

use serde::Deserialize;
use uuid::Uuid;

const MILLIS_PER_DAY: u64 = 86_400_000;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct RecycleCleanupReport {
    pub(crate) removed: usize,
    pub(crate) skipped: usize,
    pub(crate) failed: usize,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecycleMetadata {
    deleted_at: u64,
}

pub(crate) fn clean_expired_entries(
    recycle_root: &Path,
    retention_days: u16,
    now_ms: u64,
) -> RecycleCleanupReport {
    let mut report = RecycleCleanupReport::default();
    if retention_days == 0 {
        return report;
    }
    let cutoff = now_ms.saturating_sub(u64::from(retention_days) * MILLIS_PER_DAY);
    let entries = match std::fs::read_dir(recycle_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return report,
        Err(_) => {
            report.failed += 1;
            return report;
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                report.failed += 1;
                continue;
            }
        };
        let path = entry.path();
        let valid_name = entry
            .file_name()
            .to_str()
            .and_then(|name| Uuid::parse_str(name).ok())
            .is_some();
        let valid_directory = std::fs::symlink_metadata(&path)
            .map(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
            .unwrap_or(false);
        if !valid_name || !valid_directory {
            report.skipped += 1;
            continue;
        }
        let metadata = std::fs::read(path.join("metadata.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<RecycleMetadata>(&bytes).ok());
        let Some(metadata) = metadata else {
            report.skipped += 1;
            continue;
        };
        if metadata.deleted_at > cutoff {
            report.skipped += 1;
            continue;
        }
        match std::fs::remove_dir_all(path) {
            Ok(()) => report.removed += 1,
            Err(_) => report.failed += 1,
        }
    }

    report
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use tempfile::TempDir;
    use uuid::Uuid;

    use super::clean_expired_entries;

    use super::MILLIS_PER_DAY;

    fn create_entry(root: &Path, deleted_at: u64) -> std::path::PathBuf {
        let entry = root.join(Uuid::new_v4().to_string());
        std::fs::create_dir_all(&entry).expect("recycle entry");
        std::fs::write(entry.join("document.md"), "recycled").expect("recycled document");
        std::fs::write(
            entry.join("metadata.json"),
            serde_json::json!({ "deletedAt": deleted_at }).to_string(),
        )
        .expect("recycle metadata");
        entry
    }

    #[test]
    fn never_retention_keeps_every_entry() {
        let root = TempDir::new().expect("recycle root");
        let entry = create_entry(root.path(), 1);

        let report = clean_expired_entries(root.path(), 0, 100 * MILLIS_PER_DAY);

        assert_eq!(report.removed, 0);
        assert!(entry.exists());
    }

    #[test]
    fn cleanup_removes_entries_at_the_cutoff_and_keeps_newer_entries() {
        let root = TempDir::new().expect("recycle root");
        let now = 100 * MILLIS_PER_DAY;
        let expired = create_entry(root.path(), now - 7 * MILLIS_PER_DAY);
        let recent = create_entry(root.path(), now - 7 * MILLIS_PER_DAY + 1);

        let report = clean_expired_entries(root.path(), 7, now);

        assert_eq!(report.removed, 1);
        assert!(!expired.exists());
        assert!(recent.exists());
    }

    #[test]
    fn cleanup_skips_unknown_and_malformed_children() {
        let root = TempDir::new().expect("recycle root");
        let malformed = root.path().join(Uuid::new_v4().to_string());
        std::fs::create_dir(&malformed).expect("malformed entry");
        std::fs::write(malformed.join("metadata.json"), "not json").expect("bad metadata");
        let unrelated = root.path().join("not-a-recycle-entry");
        std::fs::create_dir(&unrelated).expect("unrelated directory");
        std::fs::write(
            unrelated.join("metadata.json"),
            serde_json::json!({ "deletedAt": 1 }).to_string(),
        )
        .expect("unrelated metadata");
        let file = root.path().join(Uuid::new_v4().to_string());
        std::fs::write(&file, "not a directory").expect("unrelated file");

        let report = clean_expired_entries(root.path(), 7, 100 * MILLIS_PER_DAY);

        assert_eq!(report.removed, 0);
        assert_eq!(report.skipped, 3);
        assert!(malformed.exists());
        assert!(unrelated.exists());
        assert!(file.exists());
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_never_follows_a_uuid_named_symbolic_link() {
        let root = TempDir::new().expect("recycle root");
        let outside = TempDir::new().expect("outside directory");
        std::fs::write(outside.path().join("keep.md"), "keep").expect("outside document");
        let link = root.path().join(Uuid::new_v4().to_string());
        std::os::unix::fs::symlink(outside.path(), &link).expect("recycle symlink");

        let report = clean_expired_entries(root.path(), 7, 100 * MILLIS_PER_DAY);

        assert_eq!(report.removed, 0);
        assert_eq!(report.skipped, 1);
        assert!(link.exists());
        assert!(outside.path().join("keep.md").exists());
    }
}
