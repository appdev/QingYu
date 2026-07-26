use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::Path;

use cap_fs_ext::DirExt;
use cap_std::fs::{Dir, Metadata};
use qingyu_dejavu::Index;
use time::{Date, Duration, Month, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset};

use super::local_state::RepositoryBinding;
use super::service::RepositoryJobError;
use crate::storage_capability::{directory_identity, open_canonical_directory_nofollow};
use crate::sync_config::storage::open_app_data;

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct MaintenanceCleanupStat {
    pub(crate) removed_entries: usize,
}

const INDEX_RETENTION_DAYS: i64 = 180;
const RETENTION_INDEXES_DAILY: usize = 2;

/// Selects local index IDs using the pinned SiYuan policy.
///
/// `indexes_newest_first` must keep the order returned by Dejavu's local index
/// listing. `select_random_index` receives the exclusive upper bound used by
/// Go's `math/rand.Intn` and must return a position in that range.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn select_retained_indexes<F>(
    indexes_newest_first: &[Index],
    now: OffsetDateTime,
    local_offset: UtcOffset,
    mut select_random_index: F,
) -> Option<Vec<String>>
where
    F: FnMut(usize) -> usize,
{
    let now_millis = now.unix_timestamp_nanos() / 1_000_000;
    let retention_millis = i128::from(INDEX_RETENTION_DAYS) * 24 * 60 * 60 * 1_000;
    let mut grouped = HashMap::<Date, Vec<&Index>>::new();
    for index in indexes_newest_first {
        if now_millis - i128::from(index.created) > retention_millis {
            continue;
        }
        let created =
            OffsetDateTime::from_unix_timestamp_nanos(i128::from(index.created) * 1_000_000)
                .ok()?;
        grouped
            .entry(created.to_offset(local_offset).date())
            .or_default()
            .push(index);
    }

    let today = now.to_offset(local_offset).date();
    let mut retained_ids = Vec::new();
    for (date, indexes) in grouped {
        if date == today || indexes.len() <= RETENTION_INDEXES_DAILY {
            retained_ids.extend(indexes.into_iter().map(|index| index.id.clone()));
            continue;
        }

        let mut retained_positions = HashSet::from([0_usize]);
        let random_upper_exclusive = indexes.len() - 1;
        for _ in 0..RETENTION_INDEXES_DAILY * 7 {
            let selected = select_random_index(random_upper_exclusive);
            if selected < random_upper_exclusive {
                retained_positions.insert(selected);
            }
            if retained_positions.len() >= RETENTION_INDEXES_DAILY {
                break;
            }
        }
        retained_ids.extend(
            retained_positions
                .into_iter()
                .map(|position| indexes[position].id.clone()),
        );
    }

    let mut unique_ids = HashSet::new();
    retained_ids.retain(|id| unique_ids.insert(id.clone()));
    (retained_ids.len() >= 3).then_some(retained_ids)
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn clean_startup_residue(
    app_data_path: &Path,
    bindings: &[RepositoryBinding],
) -> Result<MaintenanceCleanupStat, RepositoryJobError> {
    let Some(app_data) = open_app_data(app_data_path, false)
        .map_err(|_| RepositoryJobError::RepositoryUnavailable)?
    else {
        return Ok(MaintenanceCleanupStat::default());
    };
    let mut removed_entries = clean_owned_stages(app_data.directory())?;

    if let Some(sync) = open_existing_child_directory(app_data.directory(), OsStr::new("sync"))? {
        if let Some(repositories) =
            open_existing_child_directory(&sync, OsStr::new("repositories"))?
        {
            for name in canonical_repository_directory_names(&repositories)? {
                let Some(repository) = open_existing_child_directory(&repositories, &name)? else {
                    continue;
                };
                removed_entries += clean_owned_stages(&repository)?;
                if let Some(temp) = open_existing_child_directory(&repository, OsStr::new("temp"))?
                {
                    removed_entries += clean_owned_stages(&temp)?;
                }
            }
        }
    }

    for binding in bindings {
        let Some(root) = open_current_binding_root(&binding.notes_root) else {
            continue;
        };
        let Some(qingyu) = open_existing_child_directory(&root, OsStr::new(".qingyu"))? else {
            continue;
        };
        removed_entries += clean_owned_stages(&qingyu)?;
    }

    app_data
        .revalidate()
        .map_err(|_| RepositoryJobError::RepositoryUnavailable)?;
    Ok(MaintenanceCleanupStat { removed_entries })
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn clean_expired_conflict_history(
    app_data_path: &Path,
    now_utc: OffsetDateTime,
) -> Result<MaintenanceCleanupStat, RepositoryJobError> {
    let Some(app_data) = open_app_data(app_data_path, false)
        .map_err(|_| RepositoryJobError::RepositoryUnavailable)?
    else {
        return Ok(MaintenanceCleanupStat::default());
    };
    let Some(sync) = open_existing_child_directory(app_data.directory(), OsStr::new("sync"))?
    else {
        return Ok(MaintenanceCleanupStat::default());
    };
    let Some(repositories) = open_existing_child_directory(&sync, OsStr::new("repositories"))?
    else {
        return Ok(MaintenanceCleanupStat::default());
    };
    let mut removed_entries = 0;
    let cutoff = now_utc - Duration::days(30);
    for name in canonical_repository_directory_names(&repositories)? {
        let Some(repository) = open_existing_child_directory(&repositories, &name)? else {
            continue;
        };
        let Some(history) = open_existing_child_directory(&repository, OsStr::new("history"))?
        else {
            continue;
        };
        removed_entries += clean_expired_history_directories(&history, cutoff)?;
    }
    app_data
        .revalidate()
        .map_err(|_| RepositoryJobError::RepositoryUnavailable)?;
    Ok(MaintenanceCleanupStat { removed_entries })
}

fn open_current_binding_root(path: &Path) -> Option<Dir> {
    let canonical = path.canonicalize().ok()?;
    if canonical != path {
        return None;
    }
    let retained = open_canonical_directory_nofollow(&canonical).ok()?;
    let identity = directory_identity(&retained).ok()?;
    if path.canonicalize().ok()? != canonical {
        return None;
    }
    let reopened = open_canonical_directory_nofollow(&canonical).ok()?;
    (directory_identity(&reopened).ok()? == identity).then_some(retained)
}

fn clean_expired_history_directories(
    history: &Dir,
    cutoff: OffsetDateTime,
) -> Result<usize, RepositoryJobError> {
    let mut removed = 0;
    for entry in history
        .entries()
        .map_err(|_| RepositoryJobError::RepositoryUnavailable)?
    {
        let name = entry
            .map_err(|_| RepositoryJobError::RepositoryUnavailable)?
            .file_name();
        let Some(timestamp) = parse_sync_history_timestamp(&name) else {
            continue;
        };
        if timestamp >= cutoff {
            continue;
        }
        let metadata = match history.symlink_metadata(&name) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(_) => return Err(RepositoryJobError::RepositoryUnavailable),
        };
        if !metadata.is_dir() || metadata.file_type().is_symlink() || metadata_is_reparse(&metadata)
        {
            continue;
        }
        let directory = match history.open_dir_nofollow(&name) {
            Ok(directory) => directory,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(_) => return Err(RepositoryJobError::RepositoryUnavailable),
        };
        let retained = directory
            .dir_metadata()
            .map_err(|_| RepositoryJobError::RepositoryUnavailable)?;
        if !retained.is_dir() || retained.file_type().is_symlink() || metadata_is_reparse(&retained)
        {
            continue;
        }
        directory
            .remove_open_dir_all()
            .map_err(|_| RepositoryJobError::RepositoryUnavailable)?;
        removed += 1;
    }
    Ok(removed)
}

fn parse_sync_history_timestamp(name: &OsStr) -> Option<OffsetDateTime> {
    let name = name.to_str()?;
    let bytes = name.as_bytes();
    if bytes.len() != 22
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'-'
        || &bytes[17..] != b"-sync"
    {
        return None;
    }
    for index in [0..4, 5..7, 8..10, 11..13, 13..15, 15..17] {
        if !bytes[index].iter().all(u8::is_ascii_digit) {
            return None;
        }
    }
    let year = decimal(&bytes[0..4])?;
    let month = Month::try_from(u8::try_from(decimal(&bytes[5..7])?).ok()?).ok()?;
    let day = u8::try_from(decimal(&bytes[8..10])?).ok()?;
    let hour = u8::try_from(decimal(&bytes[11..13])?).ok()?;
    let minute = u8::try_from(decimal(&bytes[13..15])?).ok()?;
    let second = u8::try_from(decimal(&bytes[15..17])?).ok()?;
    let date = Date::from_calendar_date(year, month, day).ok()?;
    let time = Time::from_hms(hour, minute, second).ok()?;
    Some(PrimitiveDateTime::new(date, time).assume_utc())
}

fn decimal(bytes: &[u8]) -> Option<i32> {
    bytes.iter().try_fold(0_i32, |value, byte| {
        value
            .checked_mul(10)?
            .checked_add(i32::from(byte.checked_sub(b'0')?))
    })
}

fn canonical_repository_directory_names(
    repositories: &Dir,
) -> Result<Vec<OsString>, RepositoryJobError> {
    let mut names = Vec::new();
    for entry in repositories
        .entries()
        .map_err(|_| RepositoryJobError::RepositoryUnavailable)?
    {
        let name = entry
            .map_err(|_| RepositoryJobError::RepositoryUnavailable)?
            .file_name();
        let Some(text) = name.to_str() else {
            continue;
        };
        let Ok(repository_id) = uuid::Uuid::parse_str(text) else {
            continue;
        };
        if repository_id.to_string() == text {
            names.push(name);
        }
    }
    Ok(names)
}

fn open_existing_child_directory(
    parent: &Dir,
    name: &OsStr,
) -> Result<Option<Dir>, RepositoryJobError> {
    let metadata = match parent.symlink_metadata(name) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(RepositoryJobError::RepositoryUnavailable),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() || metadata_is_reparse(&metadata) {
        return Ok(None);
    }
    match parent.open_dir_nofollow(name) {
        Ok(directory) => Ok(Some(directory)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(RepositoryJobError::RepositoryUnavailable),
    }
}

fn clean_owned_stages(parent: &Dir) -> Result<usize, RepositoryJobError> {
    let mut removed = 0;
    for entry in parent
        .entries()
        .map_err(|_| RepositoryJobError::RepositoryUnavailable)?
    {
        let name = entry
            .map_err(|_| RepositoryJobError::RepositoryUnavailable)?
            .file_name();
        if !qingyu_dejavu::is_owned_stage_name(&name) {
            continue;
        }
        let metadata = match parent.symlink_metadata(&name) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(_) => return Err(RepositoryJobError::RepositoryUnavailable),
        };
        let is_reparse = metadata_is_reparse(&metadata);
        if metadata.is_dir() && !metadata.file_type().is_symlink() && !is_reparse {
            continue;
        }
        if !metadata.is_file() && !metadata.file_type().is_symlink() && !is_reparse {
            continue;
        }
        match parent.remove_file_or_symlink(&name) {
            Ok(()) => removed += 1,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return Err(RepositoryJobError::RepositoryUnavailable),
        }
    }
    Ok(removed)
}

fn metadata_is_reparse(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use cap_std::fs::MetadataExt;

        metadata.file_attributes()
            & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
            != 0
    }
    #[cfg(not(windows))]
    {
        let _ = metadata;
        false
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use qingyu_dejavu::Index;
    use tempfile::tempdir;
    use time::{Date, Duration, Month, OffsetDateTime, UtcOffset};

    use super::{clean_expired_conflict_history, clean_startup_residue, select_retained_indexes};
    use crate::dejavu_sync::local_state::RepositoryBinding;

    fn owned_stage(hex: char) -> String {
        format!("stage-{}.tmp", hex.to_string().repeat(40))
    }

    fn binding(
        repository_id: &str,
        notes_root: impl Into<std::path::PathBuf>,
        enabled: bool,
    ) -> RepositoryBinding {
        RepositoryBinding {
            repository_id: repository_id.to_owned(),
            display_name: repository_id.to_owned(),
            notes_root: notes_root.into(),
            enabled,
        }
    }

    fn utc(year: i32, month: Month, day: u8, hour: u8, minute: u8) -> OffsetDateTime {
        Date::from_calendar_date(year, month, day)
            .unwrap()
            .with_hms(hour, minute, 0)
            .unwrap()
            .assume_utc()
    }

    fn index(id: &str, created: OffsetDateTime) -> Index {
        Index {
            id: id.to_owned(),
            memo: String::new(),
            created: i64::try_from(created.unix_timestamp_nanos() / 1_000_000).unwrap(),
            files: Vec::new(),
            count: 0,
            size: 0,
            system_id: String::new(),
            system_name: String::new(),
            system_os: String::new(),
            check_index_id: String::new(),
            aes_key_verify_val: String::new(),
        }
    }

    fn sorted(mut values: Vec<String>) -> Vec<String> {
        values.sort();
        values
    }

    #[test]
    fn retained_indexes_use_local_days_exact_cutoff_and_pinned_sampling_range() {
        let now = utc(2026, Month::July, 26, 0, 30);
        let local_offset = UtcOffset::from_hms(8, 0, 0).unwrap();
        let indexes = vec![
            index("today-later", utc(2026, Month::July, 26, 0, 20)),
            index("today-across-utc", utc(2026, Month::July, 25, 16, 30)),
            index("old-fixed", utc(2026, Month::July, 25, 15, 0)),
            index("old-random", utc(2026, Month::July, 25, 12, 0)),
            index("old-unselected", utc(2026, Month::July, 25, 8, 0)),
            index("old-oldest", utc(2026, Month::July, 24, 17, 0)),
            index("cutoff-inclusive", now - Duration::days(180)),
            index(
                "beyond-cutoff",
                now - Duration::days(180) - Duration::milliseconds(1),
            ),
        ];
        let mut upper_bounds = Vec::new();

        let retained = select_retained_indexes(&indexes, now, local_offset, |upper| {
            upper_bounds.push(upper);
            1
        })
        .unwrap();

        assert_eq!(upper_bounds, vec![3]);
        assert_eq!(
            sorted(retained),
            sorted(vec![
                "today-later".to_owned(),
                "today-across-utc".to_owned(),
                "old-fixed".to_owned(),
                "old-random".to_owned(),
                "cutoff-inclusive".to_owned(),
            ])
        );
    }

    #[test]
    fn retained_indexes_allow_repeated_draws_to_keep_only_one_old_day_index() {
        let now = utc(2026, Month::July, 26, 12, 0);
        let indexes = vec![
            index("today-a", now - Duration::minutes(1)),
            index("today-b", now - Duration::minutes(2)),
            index("old-fixed", utc(2026, Month::July, 25, 20, 0)),
            index("old-middle", utc(2026, Month::July, 25, 12, 0)),
            index("old-oldest", utc(2026, Month::July, 25, 4, 0)),
        ];
        let mut calls = 0;

        let retained = select_retained_indexes(&indexes, now, UtcOffset::UTC, |upper_exclusive| {
            calls += 1;
            assert_eq!(upper_exclusive, 2);
            0
        })
        .unwrap();

        assert_eq!(calls, 14);
        assert_eq!(
            sorted(retained),
            sorted(vec![
                "today-a".to_owned(),
                "today-b".to_owned(),
                "old-fixed".to_owned(),
            ])
        );
    }

    #[test]
    fn retained_indexes_skip_purge_below_three_unique_ids() {
        let now = utc(2026, Month::July, 26, 12, 0);
        let indexes = vec![
            index("today-a", now - Duration::minutes(1)),
            index("today-b", now - Duration::minutes(2)),
        ];

        assert!(select_retained_indexes(&indexes, now, UtcOffset::UTC, |_| 0).is_none());
    }

    #[test]
    fn conflict_history_cleanup_removes_only_expired_exact_utc_snapshot_directories() {
        let temporary = tempdir().unwrap();
        let app_data = temporary.path().join("app-data");
        let repositories = app_data.join("sync").join("repositories");
        let repository_id = "00000000-0000-4000-8000-00000000008a";
        let history = repositories.join(repository_id).join("history");
        let expired = history.join("2026-06-26-115959-sync");
        let exact_boundary = history.join("2026-06-26-120000-sync");
        let recent = history.join("2026-06-26-120001-sync");
        let future = history.join("2026-07-27-120000-sync");
        let invalid_date = history.join("2026-02-30-120000-sync");
        let invalid_name = history.join("2026-06-01-120000-document");
        let matching_file = history.join("2026-06-01-120000-sync");
        let invalid_repository_history = repositories
            .join("not-a-repository")
            .join("history")
            .join("2026-06-01-120000-sync");
        let noncanonical_repository_history = repositories
            .join(repository_id.replace('-', ""))
            .join("history")
            .join("2026-06-01-120000-sync");
        let document_history = app_data
            .join("markdown-history")
            .join("2026-06-01-120000-sync");
        for directory in [
            expired.join("nested"),
            exact_boundary.clone(),
            recent.clone(),
            future.clone(),
            invalid_date.clone(),
            invalid_name.clone(),
            invalid_repository_history.clone(),
            noncanonical_repository_history.clone(),
            document_history.clone(),
        ] {
            fs::create_dir_all(directory).unwrap();
        }
        fs::write(expired.join("nested/remote.md"), b"remote").unwrap();
        fs::write(&matching_file, b"ordinary file").unwrap();

        #[cfg(unix)]
        let (direct_link, direct_target, nested_target) = {
            let direct_target = temporary.path().join("direct-link-target");
            let nested_target = temporary.path().join("nested-link-target.md");
            fs::create_dir(&direct_target).unwrap();
            fs::write(&nested_target, b"outside").unwrap();
            let direct_link = history.join("2026-06-01-110000-sync");
            std::os::unix::fs::symlink(&direct_target, &direct_link).unwrap();
            std::os::unix::fs::symlink(&nested_target, expired.join("outside-link")).unwrap();
            (Some(direct_link), Some(direct_target), Some(nested_target))
        };
        #[cfg(not(unix))]
        let (direct_link, direct_target, nested_target): (
            Option<std::path::PathBuf>,
            Option<std::path::PathBuf>,
            Option<std::path::PathBuf>,
        ) = (None, None, None);

        #[cfg(unix)]
        let (repository_link, repository_link_target) = {
            let target = temporary.path().join("repository-link-target");
            let old = target.join("history/2026-06-01-100000-sync");
            fs::create_dir_all(&old).unwrap();
            let link = repositories.join("00000000-0000-4000-8000-00000000008b");
            std::os::unix::fs::symlink(&target, &link).unwrap();
            (Some(link), Some(target))
        };
        #[cfg(not(unix))]
        let (repository_link, repository_link_target): (
            Option<std::path::PathBuf>,
            Option<std::path::PathBuf>,
        ) = (None, None);

        let removed =
            clean_expired_conflict_history(&app_data, utc(2026, Month::July, 26, 12, 0)).unwrap();

        assert_eq!(removed.removed_entries, 1);
        assert!(!expired.exists());
        for retained in [
            exact_boundary,
            recent,
            future,
            invalid_date,
            invalid_name,
            invalid_repository_history,
            noncanonical_repository_history,
            document_history,
        ] {
            assert!(retained.is_dir(), "{}", retained.display());
        }
        assert_eq!(fs::read(matching_file).unwrap(), b"ordinary file");
        if let (Some(link), Some(target)) = (direct_link, direct_target) {
            assert!(fs::symlink_metadata(link).unwrap().file_type().is_symlink());
            assert!(target.is_dir());
        }
        if let Some(target) = nested_target {
            assert_eq!(fs::read(target).unwrap(), b"outside");
        }
        if let (Some(link), Some(target)) = (repository_link, repository_link_target) {
            assert!(fs::symlink_metadata(link).unwrap().file_type().is_symlink());
            assert!(target.join("history/2026-06-01-100000-sync").is_dir());
        }
    }

    #[test]
    fn startup_cleanup_removes_only_direct_owned_stage_entries_from_owned_parents() {
        let temporary = tempdir().unwrap();
        let app_data = temporary.path().join("app-data");
        let repository_id = "00000000-0000-4000-8000-00000000007a";
        let repository = app_data
            .join("sync")
            .join("repositories")
            .join(repository_id);
        let temp = repository.join("temp");
        let repo = repository.join("repo");
        let history = repository.join("history");
        let invalid_repository = app_data
            .join("sync")
            .join("repositories")
            .join("not-a-repository");
        let noncanonical_repository = app_data
            .join("sync")
            .join("repositories")
            .join(repository_id.replace('-', ""));
        let online_root = temporary.path().join("online-notes");
        let qingyu = online_root.join(".qingyu");
        let offline_root = temporary.path().join("offline-notes");
        let neighbor = temporary.path().join("neighbor");
        for directory in [
            &temp,
            &repo,
            &history,
            &invalid_repository,
            &noncanonical_repository,
            &qingyu,
            &neighbor,
        ] {
            fs::create_dir_all(directory).unwrap();
        }
        let online_root = online_root.canonicalize().unwrap();
        let qingyu = online_root.join(".qingyu");

        let app_stage = owned_stage('0');
        let repository_stage = owned_stage('1');
        let temp_stage = owned_stage('2');
        let qingyu_stage = owned_stage('3');
        let matching_directory = owned_stage('4');
        let protected_neighbor = owned_stage('5');
        fs::write(app_data.join(&app_stage), b"app").unwrap();
        fs::write(repository.join(&repository_stage), b"repository").unwrap();
        fs::write(temp.join(&temp_stage), b"temp").unwrap();
        fs::write(qingyu.join(&qingyu_stage), b"qingyu").unwrap();
        fs::create_dir(app_data.join(&matching_directory)).unwrap();
        fs::write(app_data.join("user.tmp"), b"user").unwrap();
        fs::write(repo.join(owned_stage('6')), b"repo").unwrap();
        fs::write(history.join(owned_stage('7')), b"history").unwrap();
        fs::write(online_root.join(owned_stage('8')), b"note-root").unwrap();
        fs::write(invalid_repository.join(owned_stage('9')), b"invalid").unwrap();
        fs::write(
            noncanonical_repository.join(owned_stage('a')),
            b"noncanonical",
        )
        .unwrap();
        fs::write(neighbor.join(&protected_neighbor), b"neighbor").unwrap();

        #[cfg(unix)]
        let symlink_target = {
            let target = neighbor.join("outside-target");
            fs::write(&target, b"outside").unwrap();
            std::os::unix::fs::symlink(&target, qingyu.join(owned_stage('b'))).unwrap();
            Some(target)
        };
        #[cfg(not(unix))]
        let symlink_target: Option<std::path::PathBuf> = None;

        let removed = clean_startup_residue(
            &app_data,
            &[
                binding(
                    "00000000-0000-4000-8000-00000000007c",
                    online_root.clone(),
                    true,
                ),
                binding(
                    "00000000-0000-4000-8000-000000000072",
                    offline_root.clone(),
                    true,
                ),
            ],
        )
        .unwrap();

        for removed_path in [
            app_data.join(app_stage),
            repository.join(repository_stage),
            temp.join(temp_stage),
            qingyu.join(qingyu_stage),
        ] {
            assert!(!removed_path.exists(), "{}", removed_path.display());
        }
        assert!(app_data.join(matching_directory).is_dir());
        assert_eq!(fs::read(app_data.join("user.tmp")).unwrap(), b"user");
        assert_eq!(fs::read(repo.join(owned_stage('6'))).unwrap(), b"repo");
        assert_eq!(
            fs::read(history.join(owned_stage('7'))).unwrap(),
            b"history"
        );
        assert_eq!(
            fs::read(online_root.join(owned_stage('8'))).unwrap(),
            b"note-root"
        );
        assert_eq!(
            fs::read(invalid_repository.join(owned_stage('9'))).unwrap(),
            b"invalid"
        );
        assert_eq!(
            fs::read(noncanonical_repository.join(owned_stage('a'))).unwrap(),
            b"noncanonical"
        );
        assert_eq!(
            fs::read(neighbor.join(protected_neighbor)).unwrap(),
            b"neighbor"
        );
        if let Some(target) = symlink_target {
            assert_eq!(fs::read(&target).unwrap(), b"outside");
            assert!(!qingyu.join(owned_stage('b')).exists());
        }
        let expected_removed = if cfg!(unix) { 5 } else { 4 };
        assert_eq!(removed.removed_entries, expected_removed);
        assert!(!offline_root.exists());
    }
}
