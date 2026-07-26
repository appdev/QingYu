use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use cap_fs_ext::DirExt;
use cap_std::fs::{Dir, Metadata};
use qingyu_dejavu::{Index, PurgeStat};
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

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) trait LocalPurgeRepositoryOps: Send + Sync {
    fn list_local_indexes(&self, repository_id: &str) -> Result<Vec<Index>, RepositoryJobError>;

    fn purge_local(
        &self,
        repository_id: &str,
        retained_index_ids: &[String],
        cancelled: &AtomicBool,
    ) -> Result<PurgeStat, RepositoryJobError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum LocalPurgeOutcome {
    Skipped,
    Purged(PurgeStat),
}

#[derive(Clone)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct LocalPurgeExecutor {
    repository: Arc<dyn LocalPurgeRepositoryOps>,
    local_date_at: Arc<dyn Fn(OffsetDateTime) -> Option<Date> + Send + Sync>,
    select_random_index: Arc<dyn Fn(usize) -> Option<usize> + Send + Sync>,
}

#[cfg_attr(not(test), allow(dead_code))]
impl LocalPurgeExecutor {
    pub(crate) fn new<Repository, LocalDate, SelectRandom>(
        repository: Arc<Repository>,
        local_date_at: LocalDate,
        select_random_index: SelectRandom,
    ) -> Self
    where
        Repository: LocalPurgeRepositoryOps + 'static,
        LocalDate: Fn(OffsetDateTime) -> Option<Date> + Send + Sync + 'static,
        SelectRandom: Fn(usize) -> Option<usize> + Send + Sync + 'static,
    {
        let repository: Arc<dyn LocalPurgeRepositoryOps> = repository;
        Self {
            repository,
            local_date_at: Arc::new(local_date_at),
            select_random_index: Arc::new(select_random_index),
        }
    }

    pub(crate) async fn execute(
        &self,
        repository_id: String,
        now: OffsetDateTime,
        cancelled: Arc<AtomicBool>,
    ) -> Result<LocalPurgeOutcome, RepositoryJobError> {
        let repository = Arc::clone(&self.repository);
        let local_date_at = Arc::clone(&self.local_date_at);
        let select_random_index = Arc::clone(&self.select_random_index);
        tokio::task::spawn_blocking(move || {
            let indexes = repository.list_local_indexes(&repository_id)?;
            let mut selection_failed = false;
            let retained = select_retained_indexes(
                &indexes,
                now,
                |instant| local_date_at(instant),
                |upper| match select_random_index(upper) {
                    Some(selected) if selected < upper => selected,
                    _ => {
                        selection_failed = true;
                        0
                    }
                },
            );
            if selection_failed {
                return Ok(LocalPurgeOutcome::Skipped);
            }
            let Some(retained) = retained else {
                return Ok(LocalPurgeOutcome::Skipped);
            };
            repository
                .purge_local(&repository_id, &retained, cancelled.as_ref())
                .map(LocalPurgeOutcome::Purged)
        })
        .await
        .map_err(|_| RepositoryJobError::RepositoryUnavailable)?
    }
}

/// Selects an unbiased position from the exact half-open range `[0, upper)`
/// using operating-system entropy. Entropy failure conservatively disables
/// the purge attempt.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn os_random_index(upper: usize) -> Option<usize> {
    if upper == 0 {
        return None;
    }
    let upper = upper as u128;
    let acceptance_limit = u128::MAX - (u128::MAX % upper);
    loop {
        let mut entropy = [0_u8; 16];
        getrandom::fill(&mut entropy).ok()?;
        let value = u128::from_le_bytes(entropy);
        if value < acceptance_limit {
            return usize::try_from(value % upper).ok();
        }
    }
}

const INDEX_RETENTION_DAYS: i64 = 180;
const RETENTION_INDEXES_DAILY: usize = 2;

/// Selects local index IDs using the pinned SiYuan policy.
///
/// `indexes_newest_first` must keep the order returned by Dejavu's local index
/// listing. `local_date_at` resolves each instant independently so historical
/// daylight-saving offsets are preserved; returning `None` skips the purge.
/// `select_random_index` receives the exclusive upper bound used by Go's
/// `math/rand.Intn` and must return a position in that range.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn select_retained_indexes<LocalDate, SelectRandom>(
    indexes_newest_first: &[Index],
    now: OffsetDateTime,
    mut local_date_at: LocalDate,
    mut select_random_index: SelectRandom,
) -> Option<Vec<String>>
where
    LocalDate: FnMut(OffsetDateTime) -> Option<Date>,
    SelectRandom: FnMut(usize) -> usize,
{
    let now_millis = now.unix_timestamp_nanos() / 1_000_000;
    let retention_millis = i128::from(INDEX_RETENTION_DAYS) * 24 * 60 * 60 * 1_000;
    let mut grouped = HashMap::<Date, Vec<&Index>>::new();
    for index in indexes_newest_first {
        if now_millis - i128::from(index.created) > retention_millis {
            break;
        }
        let created =
            OffsetDateTime::from_unix_timestamp_nanos(i128::from(index.created) * 1_000_000)
                .ok()?;
        grouped
            .entry(local_date_at(created)?)
            .or_default()
            .push(index);
    }

    let today = local_date_at(now)?;
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

#[allow(dead_code)]
pub(crate) fn local_calendar_date_at(instant: OffsetDateTime) -> Option<Date> {
    UtcOffset::local_offset_at(instant)
        .ok()
        .map(|offset| instant.to_offset(offset).date())
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
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};

    use qingyu_dejavu::{Index, PurgeStat};
    use tempfile::tempdir;
    use time::{Date, Duration, Month, OffsetDateTime, UtcOffset};

    use super::{
        clean_expired_conflict_history, clean_startup_residue, os_random_index,
        select_retained_indexes, LocalPurgeExecutor, LocalPurgeOutcome, LocalPurgeRepositoryOps,
    };
    use crate::dejavu_sync::local_state::RepositoryBinding;
    use crate::dejavu_sync::service::RepositoryJobError;

    #[derive(Clone)]
    struct PurgeCall {
        repository_id: String,
        retained_index_ids: Vec<String>,
        same_cancellation: bool,
    }

    struct FakeLocalPurgeRepository {
        indexes: Vec<Index>,
        expected_cancellation: Arc<AtomicBool>,
        purge_stat: PurgeStat,
        list_thread: Mutex<Option<std::thread::ThreadId>>,
        purge_calls: Mutex<Vec<PurgeCall>>,
    }

    impl LocalPurgeRepositoryOps for FakeLocalPurgeRepository {
        fn list_local_indexes(
            &self,
            _repository_id: &str,
        ) -> Result<Vec<Index>, RepositoryJobError> {
            *self.list_thread.lock().unwrap() = Some(std::thread::current().id());
            Ok(self.indexes.clone())
        }

        fn purge_local(
            &self,
            repository_id: &str,
            retained_index_ids: &[String],
            cancelled: &AtomicBool,
        ) -> Result<PurgeStat, RepositoryJobError> {
            self.purge_calls.lock().unwrap().push(PurgeCall {
                repository_id: repository_id.to_owned(),
                retained_index_ids: retained_index_ids.to_vec(),
                same_cancellation: std::ptr::eq(cancelled, self.expected_cancellation.as_ref()),
            });
            Ok(self.purge_stat.clone())
        }
    }

    struct FailingLocalPurgeRepository {
        indexes: Vec<Index>,
        list_error: Option<RepositoryJobError>,
        purge_error: Option<RepositoryJobError>,
    }

    impl LocalPurgeRepositoryOps for FailingLocalPurgeRepository {
        fn list_local_indexes(
            &self,
            _repository_id: &str,
        ) -> Result<Vec<Index>, RepositoryJobError> {
            self.list_error
                .map_or_else(|| Ok(self.indexes.clone()), Err)
        }

        fn purge_local(
            &self,
            _repository_id: &str,
            _retained_index_ids: &[String],
            _cancelled: &AtomicBool,
        ) -> Result<PurgeStat, RepositoryJobError> {
            self.purge_error
                .map_or_else(|| Err(RepositoryJobError::RepositoryUnavailable), Err)
        }
    }

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

        let retained = select_retained_indexes(
            &indexes,
            now,
            |instant| Some(instant.to_offset(local_offset).date()),
            |upper| {
                upper_bounds.push(upper);
                1
            },
        )
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

        let retained = select_retained_indexes(
            &indexes,
            now,
            |instant| Some(instant.date()),
            |upper_exclusive| {
                calls += 1;
                assert_eq!(upper_exclusive, 2);
                0
            },
        )
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

        assert!(
            select_retained_indexes(&indexes, now, |instant| Some(instant.date()), |_| 0,)
                .is_none()
        );
    }

    #[test]
    fn retained_indexes_stop_at_first_expired_index_in_listing_order() {
        let now = utc(2026, Month::July, 26, 12, 0);
        let indexes = vec![
            index("today-a", now - Duration::minutes(1)),
            index("today-b", now - Duration::minutes(2)),
            index("yesterday", now - Duration::days(1)),
            index(
                "first-expired",
                now - Duration::days(180) - Duration::milliseconds(1),
            ),
            index("newer-created-after-expired", now - Duration::minutes(3)),
        ];

        let retained =
            select_retained_indexes(&indexes, now, |instant| Some(instant.date()), |_| 0).unwrap();

        assert_eq!(
            sorted(retained),
            sorted(vec![
                "today-a".to_owned(),
                "today-b".to_owned(),
                "yesterday".to_owned(),
            ])
        );
    }

    #[test]
    fn retained_indexes_resolve_each_instant_across_dst_before_grouping_local_days() {
        let now = utc(2026, Month::November, 2, 5, 30);
        let transition = utc(2026, Month::November, 1, 6, 0);
        let daylight = UtcOffset::from_hms(-4, 0, 0).unwrap();
        let standard = UtcOffset::from_hms(-5, 0, 0).unwrap();
        let indexes = vec![
            index("today", utc(2026, Month::November, 2, 5, 20)),
            index("nov-1-later", utc(2026, Month::November, 1, 5, 30)),
            index("nov-1-midnight", utc(2026, Month::November, 1, 4, 30)),
            index("oct-31-latest", utc(2026, Month::November, 1, 3, 30)),
            index("oct-31-selected", utc(2026, Month::October, 31, 23, 0)),
            index("oct-31-dropped", utc(2026, Month::October, 31, 20, 0)),
        ];

        let retained = select_retained_indexes(
            &indexes,
            now,
            |instant| {
                let offset = if instant < transition {
                    daylight
                } else {
                    standard
                };
                Some(instant.to_offset(offset).date())
            },
            |_| 1,
        )
        .unwrap();

        assert_eq!(
            sorted(retained),
            sorted(vec![
                "today".to_owned(),
                "nov-1-later".to_owned(),
                "nov-1-midnight".to_owned(),
                "oct-31-latest".to_owned(),
                "oct-31-selected".to_owned(),
            ])
        );
    }

    #[tokio::test]
    async fn local_purge_executor_runs_blocking_success_with_exact_selection_contract() {
        let now = utc(2026, Month::July, 26, 12, 0);
        let cancellation = Arc::new(AtomicBool::new(false));
        let repository = Arc::new(FakeLocalPurgeRepository {
            indexes: vec![
                index("today-a", now - Duration::minutes(1)),
                index("today-b", now - Duration::minutes(2)),
                index("old-fixed", now - Duration::days(1)),
                index(
                    "old-selected",
                    now - Duration::days(1) - Duration::minutes(1),
                ),
                index(
                    "old-dropped",
                    now - Duration::days(1) - Duration::minutes(2),
                ),
            ],
            expected_cancellation: Arc::clone(&cancellation),
            purge_stat: PurgeStat {
                objects: 3,
                indexes: 2,
                size: 128,
            },
            list_thread: Mutex::new(None),
            purge_calls: Mutex::new(Vec::new()),
        });
        let random_uppers = Arc::new(Mutex::new(Vec::new()));
        let observed_uppers = Arc::clone(&random_uppers);
        let executor = LocalPurgeExecutor::new(
            Arc::clone(&repository),
            |instant| Some(instant.date()),
            move |upper| {
                observed_uppers.lock().unwrap().push(upper);
                Some(1)
            },
        );
        let caller_thread = std::thread::current().id();

        let outcome = executor
            .execute("repo-a".to_owned(), now, Arc::clone(&cancellation))
            .await
            .unwrap();

        assert_eq!(
            outcome,
            LocalPurgeOutcome::Purged(PurgeStat {
                objects: 3,
                indexes: 2,
                size: 128,
            })
        );
        assert_eq!(*random_uppers.lock().unwrap(), vec![2]);
        assert_ne!(*repository.list_thread.lock().unwrap(), Some(caller_thread));
        let calls = repository.purge_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].repository_id, "repo-a");
        assert!(calls[0].same_cancellation);
        assert_eq!(
            sorted(calls[0].retained_index_ids.clone()),
            sorted(vec![
                "today-a".to_owned(),
                "today-b".to_owned(),
                "old-fixed".to_owned(),
                "old-selected".to_owned(),
            ])
        );
    }

    #[tokio::test]
    async fn local_purge_executor_skips_fewer_than_three_without_purge() {
        let now = utc(2026, Month::July, 26, 12, 0);
        let cancellation = Arc::new(AtomicBool::new(false));
        let repository = Arc::new(FakeLocalPurgeRepository {
            indexes: vec![
                index("today-a", now - Duration::minutes(1)),
                index("today-b", now - Duration::minutes(2)),
            ],
            expected_cancellation: Arc::clone(&cancellation),
            purge_stat: PurgeStat::default(),
            list_thread: Mutex::new(None),
            purge_calls: Mutex::new(Vec::new()),
        });
        let executor = LocalPurgeExecutor::new(
            Arc::clone(&repository),
            |instant| Some(instant.date()),
            |_| Some(0),
        );

        let outcome = executor
            .execute("repo-a".to_owned(), now, cancellation)
            .await
            .unwrap();

        assert_eq!(outcome, LocalPurgeOutcome::Skipped);
        assert!(repository.purge_calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn local_purge_executor_conservatively_skips_policy_resolution_failures() {
        let now = utc(2026, Month::July, 26, 12, 0);
        let indexes = vec![
            index("today-a", now - Duration::minutes(1)),
            index("today-b", now - Duration::minutes(2)),
            index("old-fixed", now - Duration::days(1)),
            index(
                "old-selected",
                now - Duration::days(1) - Duration::minutes(1),
            ),
            index(
                "old-dropped",
                now - Duration::days(1) - Duration::minutes(2),
            ),
        ];

        for failure_mode in 0..3 {
            let cancellation = Arc::new(AtomicBool::new(false));
            let repository = Arc::new(FakeLocalPurgeRepository {
                indexes: indexes.clone(),
                expected_cancellation: Arc::clone(&cancellation),
                purge_stat: PurgeStat::default(),
                list_thread: Mutex::new(None),
                purge_calls: Mutex::new(Vec::new()),
            });
            let executor = LocalPurgeExecutor::new(
                Arc::clone(&repository),
                move |instant| (failure_mode != 0).then_some(instant.date()),
                move |upper| match failure_mode {
                    1 => None,
                    2 => Some(upper),
                    _ => Some(0),
                },
            );

            assert_eq!(
                executor
                    .execute("repo-a".to_owned(), now, cancellation)
                    .await
                    .unwrap(),
                LocalPurgeOutcome::Skipped
            );
            assert!(repository.purge_calls.lock().unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn local_purge_executor_returns_list_and_purge_errors() {
        let now = utc(2026, Month::July, 26, 12, 0);
        let retained_indexes = vec![
            index("today-a", now - Duration::minutes(1)),
            index("today-b", now - Duration::minutes(2)),
            index("yesterday", now - Duration::days(1)),
        ];
        for (list_error, purge_error, expected) in [
            (
                Some(RepositoryJobError::ConfigUnavailable),
                None,
                RepositoryJobError::ConfigUnavailable,
            ),
            (
                None,
                Some(RepositoryJobError::RepositoryUnavailable),
                RepositoryJobError::RepositoryUnavailable,
            ),
        ] {
            let repository = Arc::new(FailingLocalPurgeRepository {
                indexes: retained_indexes.clone(),
                list_error,
                purge_error,
            });
            let executor =
                LocalPurgeExecutor::new(repository, |instant| Some(instant.date()), |_| Some(0));

            assert_eq!(
                executor
                    .execute("repo-a".to_owned(), now, Arc::new(AtomicBool::new(false)),)
                    .await,
                Err(expected)
            );
        }
    }

    #[test]
    fn os_random_index_uses_the_exact_half_open_range() {
        assert_eq!(os_random_index(0), None);
        for _ in 0..64 {
            assert_eq!(os_random_index(1), Some(0));
            assert!(os_random_index(17).is_some_and(|selected| selected < 17));
        }
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
