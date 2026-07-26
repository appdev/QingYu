use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use cap_fs_ext::DirExt;
use cap_std::fs::{Dir, Metadata};
use qingyu_dejavu::{Index, PurgeStat};
use time::{Date, Duration, Month, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset};

use super::local_state::{LocalSyncStateService, RepositoryBinding};
use super::service::{
    RepositoryJobCompletion, RepositoryJobCompletionObserver, RepositoryJobError,
};
use super::status::{RepositoryMaintenance, RepositoryStatusStore};
use crate::storage_capability::{directory_identity, open_canonical_directory_nofollow};
use crate::sync_config::status::SyncTrigger;
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

pub(crate) type LocalPurgeTaskFuture =
    Pin<Box<dyn Future<Output = Result<LocalPurgeOutcome, RepositoryJobError>> + Send + 'static>>;
pub(crate) type LocalMaintenanceOperation =
    Box<dyn FnOnce() -> LocalPurgeTaskFuture + Send + 'static>;
pub(crate) type LocalMaintenanceTimerFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

const LOCAL_MAINTENANCE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(12 * 60 * 60);

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) trait LocalPurgeTaskExecutor: Send + Sync {
    fn execute(
        &self,
        repository_id: String,
        now: OffsetDateTime,
        cancelled: Arc<AtomicBool>,
    ) -> LocalPurgeTaskFuture;
}

impl LocalPurgeTaskExecutor for LocalPurgeExecutor {
    fn execute(
        &self,
        repository_id: String,
        now: OffsetDateTime,
        cancelled: Arc<AtomicBool>,
    ) -> LocalPurgeTaskFuture {
        let executor = self.clone();
        Box::pin(async move {
            LocalPurgeExecutor::execute(&executor, repository_id, now, cancelled).await
        })
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) trait LocalMaintenanceStatusStore: Send + Sync {
    fn load_maintenance(
        &self,
        repository_id: &str,
    ) -> Result<RepositoryMaintenance, RepositoryJobError>;

    fn set_maintenance(
        &self,
        repository_id: &str,
        maintenance: RepositoryMaintenance,
    ) -> Result<RepositoryMaintenance, RepositoryJobError>;
}

impl LocalMaintenanceStatusStore for RepositoryStatusStore {
    fn load_maintenance(
        &self,
        repository_id: &str,
    ) -> Result<RepositoryMaintenance, RepositoryJobError> {
        RepositoryStatusStore::load_maintenance(self, repository_id)
    }

    fn set_maintenance(
        &self,
        repository_id: &str,
        maintenance: RepositoryMaintenance,
    ) -> Result<RepositoryMaintenance, RepositoryJobError> {
        RepositoryStatusStore::set_maintenance(self, repository_id, maintenance)
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) trait LocalMaintenanceTimer: Send + Sync {
    fn sleep(&self, duration: std::time::Duration) -> LocalMaintenanceTimerFuture;
}

pub(crate) trait LocalMaintenanceTransaction: Send + Sync {
    fn run(
        &self,
        repository_id: String,
        operation: LocalMaintenanceOperation,
    ) -> LocalPurgeTaskFuture;
}

struct ImmediateLocalMaintenanceTransaction;

impl LocalMaintenanceTransaction for ImmediateLocalMaintenanceTransaction {
    fn run(
        &self,
        _repository_id: String,
        operation: LocalMaintenanceOperation,
    ) -> LocalPurgeTaskFuture {
        Box::pin(async move { operation().await })
    }
}

struct TokioLocalMaintenanceTimer;

impl LocalMaintenanceTimer for TokioLocalMaintenanceTimer {
    fn sleep(&self, duration: std::time::Duration) -> LocalMaintenanceTimerFuture {
        Box::pin(tokio::time::sleep(duration))
    }
}

struct RunningMaintenanceAttempt {
    cancelled: Arc<AtomicBool>,
    finished: tokio::sync::watch::Sender<bool>,
}

impl RunningMaintenanceAttempt {
    fn new() -> Self {
        let (finished, _initial_receiver) = tokio::sync::watch::channel(false);
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            finished,
        }
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    fn finish(&self) {
        self.finished.send_replace(true);
    }

    async fn wait_finished(&self) {
        let mut finished = self.finished.subscribe();
        loop {
            if *finished.borrow() {
                return;
            }
            if finished.changed().await.is_err() {
                return;
            }
        }
    }
}

struct LocalMaintenanceControllerInner {
    executor: Arc<dyn LocalPurgeTaskExecutor>,
    statuses: Arc<dyn LocalMaintenanceStatusStore>,
    clock: Arc<dyn Fn() -> OffsetDateTime + Send + Sync>,
    timer: Arc<dyn LocalMaintenanceTimer>,
    transaction: Arc<dyn LocalMaintenanceTransaction>,
    state: Mutex<LocalMaintenanceControllerState>,
}

#[derive(Default)]
struct LocalMaintenanceControllerState {
    suspended: bool,
    running: HashMap<String, Arc<RunningMaintenanceAttempt>>,
    daily_housekeeping: Option<Arc<RunningMaintenanceAttempt>>,
}

struct RunningMaintenanceGuard {
    inner: Arc<LocalMaintenanceControllerInner>,
    repository_id: String,
    token: Arc<RunningMaintenanceAttempt>,
}

impl Drop for RunningMaintenanceGuard {
    fn drop(&mut self) {
        let mut state = self.inner.state.lock().unwrap();
        if state
            .running
            .get(&self.repository_id)
            .is_some_and(|current| Arc::ptr_eq(current, &self.token))
        {
            state.running.remove(&self.repository_id);
        }
        self.token.finish();
    }
}

struct DailyHousekeepingGuard {
    inner: Arc<LocalMaintenanceControllerInner>,
    token: Arc<RunningMaintenanceAttempt>,
}

impl Drop for DailyHousekeepingGuard {
    fn drop(&mut self) {
        let mut state = self.inner.state.lock().unwrap();
        if state
            .daily_housekeeping
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &self.token))
        {
            state.daily_housekeeping = None;
        }
        self.token.finish();
    }
}

#[derive(Clone)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct LocalMaintenanceController {
    inner: Arc<LocalMaintenanceControllerInner>,
}

#[cfg_attr(not(test), allow(dead_code))]
impl LocalMaintenanceController {
    pub(crate) fn new<Executor, Status, Clock>(
        executor: Arc<Executor>,
        statuses: Arc<Status>,
        clock: Clock,
    ) -> Self
    where
        Executor: LocalPurgeTaskExecutor + 'static,
        Status: LocalMaintenanceStatusStore + 'static,
        Clock: Fn() -> OffsetDateTime + Send + Sync + 'static,
    {
        Self::new_with_timer_and_transaction(
            executor,
            statuses,
            clock,
            Arc::new(TokioLocalMaintenanceTimer),
            Arc::new(ImmediateLocalMaintenanceTransaction),
        )
    }

    pub(crate) fn new_with_transaction<Executor, Status, Clock, Transaction>(
        executor: Arc<Executor>,
        statuses: Arc<Status>,
        clock: Clock,
        transaction: Arc<Transaction>,
    ) -> Self
    where
        Executor: LocalPurgeTaskExecutor + 'static,
        Status: LocalMaintenanceStatusStore + 'static,
        Clock: Fn() -> OffsetDateTime + Send + Sync + 'static,
        Transaction: LocalMaintenanceTransaction + 'static,
    {
        Self::new_with_timer_and_transaction(
            executor,
            statuses,
            clock,
            Arc::new(TokioLocalMaintenanceTimer),
            transaction,
        )
    }

    pub(crate) fn new_with_timer<Executor, Status, Clock, Timer>(
        executor: Arc<Executor>,
        statuses: Arc<Status>,
        clock: Clock,
        timer: Arc<Timer>,
    ) -> Self
    where
        Executor: LocalPurgeTaskExecutor + 'static,
        Status: LocalMaintenanceStatusStore + 'static,
        Clock: Fn() -> OffsetDateTime + Send + Sync + 'static,
        Timer: LocalMaintenanceTimer + 'static,
    {
        Self::new_with_timer_and_transaction(
            executor,
            statuses,
            clock,
            timer,
            Arc::new(ImmediateLocalMaintenanceTransaction),
        )
    }

    fn new_with_timer_and_transaction<Executor, Status, Clock, Timer, Transaction>(
        executor: Arc<Executor>,
        statuses: Arc<Status>,
        clock: Clock,
        timer: Arc<Timer>,
        transaction: Arc<Transaction>,
    ) -> Self
    where
        Executor: LocalPurgeTaskExecutor + 'static,
        Status: LocalMaintenanceStatusStore + 'static,
        Clock: Fn() -> OffsetDateTime + Send + Sync + 'static,
        Timer: LocalMaintenanceTimer + 'static,
        Transaction: LocalMaintenanceTransaction + 'static,
    {
        let executor: Arc<dyn LocalPurgeTaskExecutor> = executor;
        let statuses: Arc<dyn LocalMaintenanceStatusStore> = statuses;
        let timer: Arc<dyn LocalMaintenanceTimer> = timer;
        let transaction: Arc<dyn LocalMaintenanceTransaction> = transaction;
        Self {
            inner: Arc::new(LocalMaintenanceControllerInner {
                executor,
                statuses,
                clock: Arc::new(clock),
                timer,
                transaction,
                state: Mutex::new(LocalMaintenanceControllerState::default()),
            }),
        }
    }

    pub(crate) fn notify_sync_completion(
        &self,
        repository_id: &str,
        trigger: SyncTrigger,
        succeeded: bool,
    ) -> Result<bool, RepositoryJobError> {
        if !succeeded || trigger == SyncTrigger::SettingsExit {
            return Ok(false);
        }
        self.start_if_due(repository_id, false)
    }

    pub(crate) fn try_daily(&self, repository_id: &str) -> Result<bool, RepositoryJobError> {
        self.start_if_due(repository_id, true)
    }

    fn start_if_due(
        &self,
        repository_id: &str,
        require_first_success_marker: bool,
    ) -> Result<bool, RepositoryJobError> {
        let handle = tokio::runtime::Handle::try_current()
            .map_err(|_| RepositoryJobError::RepositoryUnavailable)?;
        let now = (self.inner.clock)();
        let mut state = self.inner.state.lock().unwrap();
        if state.suspended || state.running.contains_key(repository_id) {
            return Ok(false);
        }
        let maintenance = self.inner.statuses.load_maintenance(repository_id)?;
        match maintenance.last_local_purge_at {
            None if require_first_success_marker => return Ok(false),
            None => {}
            Some(last) => {
                let minimum_due = six_hours_after(last);
                let due_at = maintenance
                    .next_local_purge_at
                    .map_or(minimum_due, |persisted| persisted.max(minimum_due));
                if now < due_at {
                    return Ok(false);
                }
            }
        }
        let attempt = Arc::new(RunningMaintenanceAttempt::new());
        state
            .running
            .insert(repository_id.to_owned(), Arc::clone(&attempt));
        drop(state);

        let inner = Arc::clone(&self.inner);
        let repository_id = repository_id.to_owned();
        let completion_floor = maintenance
            .last_local_purge_at
            .map_or(now, |last| last.max(now));
        let running_guard = RunningMaintenanceGuard {
            inner: Arc::clone(&inner),
            repository_id: repository_id.clone(),
            token: Arc::clone(&attempt),
        };
        let executor = Arc::clone(&inner.executor);
        let execution_repository_id = repository_id.clone();
        let execution_cancelled = Arc::clone(&attempt.cancelled);
        let operation: LocalMaintenanceOperation =
            Box::new(move || executor.execute(execution_repository_id, now, execution_cancelled));
        // `run` owns the service admission boundary. Construct this future
        // before spawning so an accepted purge is registered synchronously and
        // cannot be overtaken by a repository/global reservation before the
        // detached task receives its first poll.
        let mut execution = inner.transaction.run(repository_id.clone(), operation);
        handle.spawn(async move {
            let _running_guard = running_guard;
            let mut timeout = inner.timer.sleep(LOCAL_MAINTENANCE_TIMEOUT);
            let _result = tokio::select! {
                result = &mut execution => result,
                () = &mut timeout => {
                    attempt.cancel();
                    execution.await
                }
            };
            let completed_at = (inner.clock)().max(completion_floor);
            let _status_result = inner.statuses.set_maintenance(
                &repository_id,
                RepositoryMaintenance {
                    last_local_purge_at: Some(completed_at),
                    next_local_purge_at: Some(six_hours_after(completed_at)),
                },
            );
        });
        Ok(true)
    }

    pub(crate) async fn cancel_repository_and_wait(&self, repository_id: &str) -> bool {
        let attempt = self
            .inner
            .state
            .lock()
            .unwrap()
            .running
            .get(repository_id)
            .cloned();
        let Some(attempt) = attempt else {
            return false;
        };
        attempt.cancel();
        attempt.wait_finished().await;
        true
    }

    pub(crate) async fn cancel_all_and_wait(&self) -> usize {
        let attempts = {
            let state = self.inner.state.lock().unwrap();
            let mut attempts = state.running.values().cloned().collect::<Vec<_>>();
            attempts.extend(state.daily_housekeeping.iter().cloned());
            for attempt in &attempts {
                attempt.cancel();
            }
            attempts
        };
        for attempt in &attempts {
            attempt.wait_finished().await;
        }
        attempts.len()
    }

    pub(crate) async fn suspend_all_and_wait(&self) -> usize {
        let attempts = {
            let mut state = self.inner.state.lock().unwrap();
            state.suspended = true;
            let mut attempts = state.running.values().cloned().collect::<Vec<_>>();
            attempts.extend(state.daily_housekeeping.iter().cloned());
            for attempt in &attempts {
                attempt.cancel();
            }
            attempts
        };
        for attempt in &attempts {
            attempt.wait_finished().await;
        }
        attempts.len()
    }

    pub(crate) fn resume(&self) {
        self.inner.state.lock().unwrap().suspended = false;
    }

    async fn run_daily_housekeeping_operation<T>(
        &self,
        operation: impl Future<Output = T>,
    ) -> Option<T> {
        let attempt = {
            let mut state = self.inner.state.lock().unwrap();
            if state.suspended || state.daily_housekeeping.is_some() {
                return None;
            }
            let attempt = Arc::new(RunningMaintenanceAttempt::new());
            state.daily_housekeeping = Some(Arc::clone(&attempt));
            attempt
        };
        let _guard = DailyHousekeepingGuard {
            inner: Arc::clone(&self.inner),
            token: attempt,
        };
        Some(operation.await)
    }

    #[cfg(test)]
    fn is_running(&self, repository_id: &str) -> bool {
        self.inner
            .state
            .lock()
            .unwrap()
            .running
            .contains_key(repository_id)
    }
}

impl RepositoryJobCompletionObserver for LocalMaintenanceController {
    fn observe_completion(&self, completion: RepositoryJobCompletion) {
        let _maintenance_result = self.notify_sync_completion(
            &completion.request.repository_id,
            completion.request.trigger,
            completion.result.is_ok(),
        );
    }
}

fn six_hours_after(instant: OffsetDateTime) -> OffsetDateTime {
    instant
        .checked_add(Duration::hours(6))
        .filter(|next| *next <= maximum_maintenance_instant())
        .unwrap_or_else(maximum_maintenance_instant)
}

fn maximum_maintenance_instant() -> OffsetDateTime {
    Date::from_calendar_date(9999, Month::December, 31)
        .expect("the RFC 3339 maximum date is valid")
        .with_hms_nano(23, 59, 59, 999_999_999)
        .expect("the final nanosecond of the day is valid")
        .assume_utc()
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
const DAILY_MAINTENANCE_INTERVAL: std::time::Duration =
    std::time::Duration::from_secs(24 * 60 * 60);

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

pub(crate) fn resolve_daily_repository_ids(
    app_data_path: &Path,
) -> Result<Vec<String>, RepositoryJobError> {
    let Some(state) = LocalSyncStateService::new(app_data_path)
        .load()
        .map_err(RepositoryJobError::from)?
    else {
        return Ok(Vec::new());
    };
    Ok(state
        .bindings
        .into_iter()
        .filter(|binding| binding.enabled)
        .filter(|binding| {
            uuid::Uuid::parse_str(&binding.repository_id)
                .ok()
                .is_some_and(|repository_id| repository_id.to_string() == binding.repository_id)
        })
        .filter(|binding| open_current_binding_root(&binding.notes_root).is_some())
        .map(|binding| binding.repository_id)
        .collect())
}

pub(crate) fn spawn_production_daily_maintenance(
    app_data_path: impl AsRef<Path>,
    controller: Arc<LocalMaintenanceController>,
) {
    let app_data_path = app_data_path.as_ref().to_path_buf();
    spawn_daily_maintenance_with(app_data_path, controller, spawn_on_tauri_runtime);
}

fn spawn_on_tauri_runtime(future: Pin<Box<dyn Future<Output = ()> + Send + 'static>>) {
    tauri::async_runtime::spawn(future);
}

fn spawn_daily_maintenance_with<Spawn>(
    app_data_path: PathBuf,
    controller: Arc<LocalMaintenanceController>,
    spawn: Spawn,
) where
    Spawn: FnOnce(Pin<Box<dyn Future<Output = ()> + Send + 'static>>),
{
    spawn(Box::pin(async move {
        let first_tick = tokio::time::Instant::now() + DAILY_MAINTENANCE_INTERVAL;
        let mut interval = tokio::time::interval_at(first_tick, DAILY_MAINTENANCE_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let housekeeping_app_data = app_data_path.clone();
            let housekeeping_at = OffsetDateTime::now_utc();
            let repository_ids = controller
                .run_daily_housekeeping_operation(async move {
                    tokio::task::spawn_blocking(move || {
                        let _cleanup_result =
                            clean_expired_conflict_history(&housekeeping_app_data, housekeeping_at);
                        resolve_daily_repository_ids(&housekeeping_app_data)
                    })
                    .await
                })
                .await;
            let Some(Ok(Ok(repository_ids))) = repository_ids else {
                continue;
            };
            for repository_id in repository_ids {
                let _maintenance_result = controller.try_daily(&repository_id);
            }
        }
    }));
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
    use std::collections::{HashMap, VecDeque};
    use std::fs;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use qingyu_dejavu::{Index, PurgeStat};
    use tempfile::tempdir;
    use time::{Date, Duration, Month, OffsetDateTime, UtcOffset};

    use super::{
        clean_expired_conflict_history, clean_startup_residue, os_random_index,
        resolve_daily_repository_ids, select_retained_indexes, spawn_daily_maintenance_with,
        spawn_on_tauri_runtime, LocalMaintenanceController, LocalMaintenanceStatusStore,
        LocalMaintenanceTimer, LocalMaintenanceTransaction, LocalPurgeExecutor, LocalPurgeOutcome,
        LocalPurgeRepositoryOps, LocalPurgeTaskExecutor,
    };
    use crate::dejavu_sync::local_state::{LocalSyncStateService, RepositoryBinding};
    use crate::dejavu_sync::service::RepositoryJobError;
    use crate::dejavu_sync::status::RepositoryMaintenance;
    use crate::sync_config::status::SyncTrigger;

    struct FakeMaintenanceStatusStore {
        values: Mutex<HashMap<String, RepositoryMaintenance>>,
        writes: Mutex<Vec<(String, RepositoryMaintenance)>>,
        fail_sets: AtomicBool,
    }

    #[test]
    fn tauri_daily_spawner_does_not_require_an_entered_tokio_runtime() {
        let (sent, received) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            spawn_on_tauri_runtime(Box::pin(async move {
                sent.send(()).unwrap();
            }));
        })
        .join()
        .expect("the production daily spawner must work outside Tokio context");
        received
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("the Tauri runtime should execute the spawned daily task");
    }

    impl FakeMaintenanceStatusStore {
        fn new() -> Self {
            Self {
                values: Mutex::new(HashMap::new()),
                writes: Mutex::new(Vec::new()),
                fail_sets: AtomicBool::new(false),
            }
        }
    }

    impl LocalMaintenanceStatusStore for FakeMaintenanceStatusStore {
        fn load_maintenance(
            &self,
            repository_id: &str,
        ) -> Result<RepositoryMaintenance, RepositoryJobError> {
            Ok(self
                .values
                .lock()
                .unwrap()
                .get(repository_id)
                .cloned()
                .unwrap_or_default())
        }

        fn set_maintenance(
            &self,
            repository_id: &str,
            maintenance: RepositoryMaintenance,
        ) -> Result<RepositoryMaintenance, RepositoryJobError> {
            if self.fail_sets.load(Ordering::SeqCst) {
                return Err(RepositoryJobError::StatusUnavailable);
            }
            self.values
                .lock()
                .unwrap()
                .insert(repository_id.to_owned(), maintenance.clone());
            self.writes
                .lock()
                .unwrap()
                .push((repository_id.to_owned(), maintenance.clone()));
            Ok(maintenance)
        }
    }

    struct GatedPurgeTaskExecutor {
        calls: AtomicUsize,
        started: Arc<tokio::sync::Semaphore>,
        release: Arc<tokio::sync::Semaphore>,
    }

    struct TransactionAwarePurgeTaskExecutor {
        transaction_active: Arc<AtomicBool>,
        observed_active: Arc<AtomicBool>,
    }

    impl LocalPurgeTaskExecutor for TransactionAwarePurgeTaskExecutor {
        fn execute(
            &self,
            _repository_id: String,
            _now: OffsetDateTime,
            _cancelled: Arc<AtomicBool>,
        ) -> super::LocalPurgeTaskFuture {
            let transaction_active = Arc::clone(&self.transaction_active);
            let observed_active = Arc::clone(&self.observed_active);
            Box::pin(async move {
                observed_active.store(transaction_active.load(Ordering::SeqCst), Ordering::SeqCst);
                Ok(LocalPurgeOutcome::Skipped)
            })
        }
    }

    struct RecordingMaintenanceTransaction {
        active: Arc<AtomicBool>,
        repositories: Mutex<Vec<String>>,
    }

    impl LocalMaintenanceTransaction for RecordingMaintenanceTransaction {
        fn run(
            &self,
            repository_id: String,
            operation: super::LocalMaintenanceOperation,
        ) -> super::LocalPurgeTaskFuture {
            self.repositories.lock().unwrap().push(repository_id);
            let active = Arc::clone(&self.active);
            Box::pin(async move {
                active.store(true, Ordering::SeqCst);
                let result = operation().await;
                active.store(false, Ordering::SeqCst);
                result
            })
        }
    }

    impl GatedPurgeTaskExecutor {
        fn new() -> Self {
            Self {
                calls: AtomicUsize::new(0),
                started: Arc::new(tokio::sync::Semaphore::new(0)),
                release: Arc::new(tokio::sync::Semaphore::new(0)),
            }
        }
    }

    impl LocalPurgeTaskExecutor for GatedPurgeTaskExecutor {
        fn execute(
            &self,
            _repository_id: String,
            _now: OffsetDateTime,
            _cancelled: Arc<AtomicBool>,
        ) -> super::LocalPurgeTaskFuture {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.started.add_permits(1);
            let release = Arc::clone(&self.release);
            Box::pin(async move {
                release.acquire().await.unwrap().forget();
                Ok(LocalPurgeOutcome::Skipped)
            })
        }
    }

    struct ImmediatePurgeTaskExecutor {
        outcomes: Mutex<VecDeque<Result<LocalPurgeOutcome, RepositoryJobError>>>,
    }

    struct PanickingPurgeTaskExecutor;

    impl LocalPurgeTaskExecutor for PanickingPurgeTaskExecutor {
        fn execute(
            &self,
            _repository_id: String,
            _now: OffsetDateTime,
            _cancelled: Arc<AtomicBool>,
        ) -> super::LocalPurgeTaskFuture {
            Box::pin(async move { panic!("injected local purge panic") })
        }
    }

    struct ManualMaintenanceTimer {
        requested: Mutex<Vec<std::time::Duration>>,
        fired: Arc<tokio::sync::Semaphore>,
    }

    impl ManualMaintenanceTimer {
        fn new() -> Self {
            Self {
                requested: Mutex::new(Vec::new()),
                fired: Arc::new(tokio::sync::Semaphore::new(0)),
            }
        }
    }

    impl LocalMaintenanceTimer for ManualMaintenanceTimer {
        fn sleep(&self, duration: std::time::Duration) -> super::LocalMaintenanceTimerFuture {
            self.requested.lock().unwrap().push(duration);
            let fired = Arc::clone(&self.fired);
            Box::pin(async move {
                fired.acquire().await.unwrap().forget();
            })
        }
    }

    struct CancellationAwarePurgeTaskExecutor {
        started: Arc<tokio::sync::Semaphore>,
        saw_cancel: Arc<tokio::sync::Semaphore>,
        release: Arc<tokio::sync::Semaphore>,
        cancellations: Mutex<Vec<Arc<AtomicBool>>>,
    }

    impl CancellationAwarePurgeTaskExecutor {
        fn new() -> Self {
            Self {
                started: Arc::new(tokio::sync::Semaphore::new(0)),
                saw_cancel: Arc::new(tokio::sync::Semaphore::new(0)),
                release: Arc::new(tokio::sync::Semaphore::new(0)),
                cancellations: Mutex::new(Vec::new()),
            }
        }
    }

    impl LocalPurgeTaskExecutor for CancellationAwarePurgeTaskExecutor {
        fn execute(
            &self,
            _repository_id: String,
            _now: OffsetDateTime,
            cancelled: Arc<AtomicBool>,
        ) -> super::LocalPurgeTaskFuture {
            self.cancellations
                .lock()
                .unwrap()
                .push(Arc::clone(&cancelled));
            self.started.add_permits(1);
            let saw_cancel = Arc::clone(&self.saw_cancel);
            let release = Arc::clone(&self.release);
            Box::pin(async move {
                while !cancelled.load(Ordering::Acquire) {
                    tokio::task::yield_now().await;
                }
                saw_cancel.add_permits(1);
                release.acquire().await.unwrap().forget();
                Ok(LocalPurgeOutcome::Skipped)
            })
        }
    }

    impl LocalPurgeTaskExecutor for ImmediatePurgeTaskExecutor {
        fn execute(
            &self,
            _repository_id: String,
            _now: OffsetDateTime,
            _cancelled: Arc<AtomicBool>,
        ) -> super::LocalPurgeTaskFuture {
            let outcome = self.outcomes.lock().unwrap().pop_front().unwrap();
            Box::pin(async move { outcome })
        }
    }

    async fn wait_for_status_writes(statuses: &FakeMaintenanceStatusStore, expected: usize) {
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                if statuses.writes.lock().unwrap().len() >= expected {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    async fn wait_for_idle(controller: &LocalMaintenanceController, repository_id: &str) {
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while controller.is_running(repository_id) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

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

    #[tokio::test]
    async fn first_successful_non_exit_completion_starts_detached_maintenance() {
        let executor = Arc::new(GatedPurgeTaskExecutor::new());
        let statuses = Arc::new(FakeMaintenanceStatusStore::new());
        let now = utc(2026, Month::July, 26, 12, 0);
        let controller = LocalMaintenanceController::new(
            Arc::clone(&executor),
            Arc::clone(&statuses),
            move || now,
        );
        let repository_id = "00000000-0000-4000-8000-000000000071";

        assert!(controller
            .notify_sync_completion(repository_id, SyncTrigger::Manual, true)
            .unwrap());
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            executor.started.acquire(),
        )
        .await
        .unwrap()
        .unwrap()
        .forget();
        assert_eq!(executor.calls.load(Ordering::SeqCst), 1);
        assert!(statuses.writes.lock().unwrap().is_empty());

        executor.release.add_permits(1);
        wait_for_status_writes(&statuses, 1).await;
    }

    #[tokio::test]
    async fn purge_execution_runs_inside_the_injected_repository_transaction() {
        let transaction_active = Arc::new(AtomicBool::new(false));
        let observed_active = Arc::new(AtomicBool::new(false));
        let executor = Arc::new(TransactionAwarePurgeTaskExecutor {
            transaction_active: Arc::clone(&transaction_active),
            observed_active: Arc::clone(&observed_active),
        });
        let transaction = Arc::new(RecordingMaintenanceTransaction {
            active: Arc::clone(&transaction_active),
            repositories: Mutex::new(Vec::new()),
        });
        let statuses = Arc::new(FakeMaintenanceStatusStore::new());
        let now = utc(2026, Month::July, 26, 12, 0);
        let controller = LocalMaintenanceController::new_with_transaction(
            executor,
            Arc::clone(&statuses),
            move || now,
            Arc::clone(&transaction),
        );
        let repository_id = "00000000-0000-4000-8000-000000000080";

        assert!(controller
            .notify_sync_completion(repository_id, SyncTrigger::Manual, true)
            .unwrap());
        assert_eq!(
            transaction.repositories.lock().unwrap().as_slice(),
            &[repository_id.to_owned()],
            "the repository transaction must register before the detached task can be polled"
        );
        wait_for_status_writes(&statuses, 1).await;

        assert!(observed_active.load(Ordering::SeqCst));
        assert!(!transaction_active.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn failed_and_settings_exit_syncs_do_not_start_maintenance() {
        let executor = Arc::new(GatedPurgeTaskExecutor::new());
        let statuses = Arc::new(FakeMaintenanceStatusStore::new());
        let now = utc(2026, Month::July, 26, 12, 0);
        let controller =
            LocalMaintenanceController::new(Arc::clone(&executor), statuses, move || now);
        let repository_id = "00000000-0000-4000-8000-000000000072";

        assert!(!controller
            .notify_sync_completion(repository_id, SyncTrigger::Manual, false)
            .unwrap());
        assert!(!controller
            .notify_sync_completion(repository_id, SyncTrigger::SettingsExit, true)
            .unwrap());
        tokio::task::yield_now().await;
        assert_eq!(executor.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn repeated_completion_deduplicates_and_exact_six_hours_is_due() {
        let executor = Arc::new(GatedPurgeTaskExecutor::new());
        let statuses = Arc::new(FakeMaintenanceStatusStore::new());
        let base = utc(2026, Month::July, 26, 12, 0);
        let now = Arc::new(Mutex::new(base));
        let observed_now = Arc::clone(&now);
        let controller = LocalMaintenanceController::new(
            Arc::clone(&executor),
            Arc::clone(&statuses),
            move || *observed_now.lock().unwrap(),
        );
        let repository_id = "00000000-0000-4000-8000-000000000073";

        assert!(controller
            .notify_sync_completion(repository_id, SyncTrigger::Manual, true)
            .unwrap());
        executor.started.acquire().await.unwrap().forget();
        assert!(!controller
            .notify_sync_completion(repository_id, SyncTrigger::Manual, true)
            .unwrap());
        executor.release.add_permits(1);
        wait_for_status_writes(&statuses, 1).await;
        wait_for_idle(&controller, repository_id).await;

        *now.lock().unwrap() = base + Duration::hours(6) - Duration::milliseconds(1);
        assert!(!controller
            .notify_sync_completion(repository_id, SyncTrigger::Manual, true)
            .unwrap());
        *now.lock().unwrap() = base + Duration::hours(6);
        assert!(controller
            .notify_sync_completion(repository_id, SyncTrigger::Manual, true)
            .unwrap());
        executor.started.acquire().await.unwrap().forget();
        assert_eq!(executor.calls.load(Ordering::SeqCst), 2);
        executor.release.add_permits(1);
        wait_for_status_writes(&statuses, 2).await;
    }

    #[tokio::test]
    async fn daily_is_inert_before_first_marker_and_starts_when_due_afterward() {
        let executor = Arc::new(GatedPurgeTaskExecutor::new());
        let statuses = Arc::new(FakeMaintenanceStatusStore::new());
        let base = utc(2026, Month::July, 26, 12, 0);
        let now = Arc::new(Mutex::new(base));
        let observed_now = Arc::clone(&now);
        let controller = LocalMaintenanceController::new(
            Arc::clone(&executor),
            Arc::clone(&statuses),
            move || *observed_now.lock().unwrap(),
        );
        let repository_id = "00000000-0000-4000-8000-000000000074";

        assert!(!controller.try_daily(repository_id).unwrap());
        statuses.values.lock().unwrap().insert(
            repository_id.to_owned(),
            RepositoryMaintenance {
                last_local_purge_at: Some(base),
                next_local_purge_at: Some(base + Duration::hours(6)),
            },
        );
        *now.lock().unwrap() = base + Duration::hours(6);

        assert!(controller.try_daily(repository_id).unwrap());
        executor.started.acquire().await.unwrap().forget();
        executor.release.add_permits(1);
        wait_for_status_writes(&statuses, 1).await;
    }

    #[tokio::test]
    async fn last_attempt_is_the_marker_even_if_an_orphan_next_deadline_exists() {
        let executor = Arc::new(GatedPurgeTaskExecutor::new());
        let statuses = Arc::new(FakeMaintenanceStatusStore::new());
        let now = utc(2026, Month::July, 26, 12, 0);
        let controller = LocalMaintenanceController::new(
            Arc::clone(&executor),
            Arc::clone(&statuses),
            move || now,
        );
        let daily = "00000000-0000-4000-8000-00000000007a";
        let sync = "00000000-0000-4000-8000-00000000007b";
        for repository_id in [daily, sync] {
            statuses.values.lock().unwrap().insert(
                repository_id.to_owned(),
                RepositoryMaintenance {
                    last_local_purge_at: None,
                    next_local_purge_at: Some(now + Duration::hours(6)),
                },
            );
        }

        assert!(!controller.try_daily(daily).unwrap());
        assert!(controller
            .notify_sync_completion(sync, SyncTrigger::Manual, true)
            .unwrap());
        executor.started.acquire().await.unwrap().forget();
        executor.release.add_permits(1);
        wait_for_status_writes(&statuses, 1).await;
    }

    #[tokio::test]
    async fn different_repositories_run_in_parallel() {
        let executor = Arc::new(GatedPurgeTaskExecutor::new());
        let statuses = Arc::new(FakeMaintenanceStatusStore::new());
        let now = utc(2026, Month::July, 26, 12, 0);
        let controller = LocalMaintenanceController::new(
            Arc::clone(&executor),
            Arc::clone(&statuses),
            move || now,
        );
        let first = "00000000-0000-4000-8000-000000000075";
        let second = "00000000-0000-4000-8000-000000000076";

        assert!(controller
            .notify_sync_completion(first, SyncTrigger::Manual, true)
            .unwrap());
        assert!(controller
            .notify_sync_completion(second, SyncTrigger::Manual, true)
            .unwrap());
        executor.started.acquire_many(2).await.unwrap().forget();
        assert_eq!(executor.calls.load(Ordering::SeqCst), 2);
        executor.release.add_permits(2);
        wait_for_status_writes(&statuses, 2).await;
    }

    #[tokio::test]
    async fn dropping_controller_handle_does_not_cancel_an_accepted_attempt() {
        let executor = Arc::new(GatedPurgeTaskExecutor::new());
        let statuses = Arc::new(FakeMaintenanceStatusStore::new());
        let now = utc(2026, Month::July, 26, 12, 0);
        let controller = LocalMaintenanceController::new(
            Arc::clone(&executor),
            Arc::clone(&statuses),
            move || now,
        );
        let repository_id = "00000000-0000-4000-8000-000000000077";

        assert!(controller
            .notify_sync_completion(repository_id, SyncTrigger::Manual, true)
            .unwrap());
        drop(controller);
        executor.started.acquire().await.unwrap().forget();
        executor.release.add_permits(1);
        wait_for_status_writes(&statuses, 1).await;
        assert_eq!(statuses.writes.lock().unwrap()[0].0, repository_id);
    }

    #[tokio::test]
    async fn skipped_and_failed_attempts_both_persist_completion_deadlines() {
        let executor = Arc::new(ImmediatePurgeTaskExecutor {
            outcomes: Mutex::new(VecDeque::from([
                Ok(LocalPurgeOutcome::Skipped),
                Err(RepositoryJobError::CloudUnavailable),
            ])),
        });
        let statuses = Arc::new(FakeMaintenanceStatusStore::new());
        let base = utc(2026, Month::July, 26, 12, 0);
        let now = Arc::new(Mutex::new(base));
        let observed_now = Arc::clone(&now);
        let controller =
            LocalMaintenanceController::new(executor, Arc::clone(&statuses), move || {
                *observed_now.lock().unwrap()
            });
        let skipped = "00000000-0000-4000-8000-000000000078";
        let failed = "00000000-0000-4000-8000-000000000079";

        assert!(controller
            .notify_sync_completion(skipped, SyncTrigger::Manual, true)
            .unwrap());
        wait_for_status_writes(&statuses, 1).await;
        *now.lock().unwrap() = base + Duration::minutes(1);
        assert!(controller
            .notify_sync_completion(failed, SyncTrigger::Manual, true)
            .unwrap());
        wait_for_status_writes(&statuses, 2).await;

        let writes = statuses.writes.lock().unwrap();
        assert_eq!(writes[0].0, skipped);
        assert_eq!(writes[0].1.last_local_purge_at, Some(base));
        assert_eq!(
            writes[0].1.next_local_purge_at,
            Some(base + Duration::hours(6))
        );
        assert_eq!(writes[1].0, failed);
        assert_eq!(
            writes[1].1.last_local_purge_at,
            Some(base + Duration::minutes(1))
        );
        assert_eq!(
            writes[1].1.next_local_purge_at,
            Some(base + Duration::hours(6) + Duration::minutes(1))
        );
    }

    #[tokio::test]
    async fn status_persistence_failure_is_internal_and_releases_the_running_lane() {
        let executor = Arc::new(ImmediatePurgeTaskExecutor {
            outcomes: Mutex::new(VecDeque::from([
                Ok(LocalPurgeOutcome::Skipped),
                Ok(LocalPurgeOutcome::Skipped),
            ])),
        });
        let statuses = Arc::new(FakeMaintenanceStatusStore::new());
        statuses.fail_sets.store(true, Ordering::SeqCst);
        let now = utc(2026, Month::July, 26, 12, 0);
        let controller = LocalMaintenanceController::new(executor, statuses.clone(), move || now);
        let repository_id = "00000000-0000-4000-8000-00000000007c";

        assert!(controller
            .notify_sync_completion(repository_id, SyncTrigger::Manual, true)
            .unwrap());
        wait_for_idle(&controller, repository_id).await;
        assert!(controller
            .notify_sync_completion(repository_id, SyncTrigger::Manual, true)
            .unwrap());
        wait_for_idle(&controller, repository_id).await;
        assert!(statuses.writes.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn executor_panic_cannot_leave_a_permanently_running_lane() {
        let statuses = Arc::new(FakeMaintenanceStatusStore::new());
        let now = utc(2026, Month::July, 26, 12, 0);
        let controller = LocalMaintenanceController::new(
            Arc::new(PanickingPurgeTaskExecutor),
            statuses,
            move || now,
        );
        let repository_id = "00000000-0000-4000-8000-00000000007d";

        assert!(controller
            .notify_sync_completion(repository_id, SyncTrigger::Manual, true)
            .unwrap());
        wait_for_idle(&controller, repository_id).await;
        assert!(controller
            .notify_sync_completion(repository_id, SyncTrigger::Manual, true)
            .unwrap());
        wait_for_idle(&controller, repository_id).await;
    }

    #[test]
    fn stale_running_guard_cannot_remove_a_newer_attempt_token() {
        let now = utc(2026, Month::July, 26, 12, 0);
        let controller = LocalMaintenanceController::new(
            Arc::new(ImmediatePurgeTaskExecutor {
                outcomes: Mutex::new(VecDeque::new()),
            }),
            Arc::new(FakeMaintenanceStatusStore::new()),
            move || now,
        );
        let repository_id = "00000000-0000-4000-8000-00000000007e";
        let stale = Arc::new(super::RunningMaintenanceAttempt::new());
        let current = Arc::new(super::RunningMaintenanceAttempt::new());
        controller
            .inner
            .state
            .lock()
            .unwrap()
            .running
            .insert(repository_id.to_owned(), Arc::clone(&current));
        let guard = super::RunningMaintenanceGuard {
            inner: Arc::clone(&controller.inner),
            repository_id: repository_id.to_owned(),
            token: stale,
        };

        drop(guard);

        assert!(controller
            .inner
            .state
            .lock()
            .unwrap()
            .running
            .get(repository_id)
            .is_some_and(|token| Arc::ptr_eq(token, &current)));
    }

    #[tokio::test]
    async fn completion_wait_is_sticky_when_finish_precedes_wait_registration() {
        let attempt = super::RunningMaintenanceAttempt::new();
        let wait = attempt.wait_finished();

        attempt.finish();

        tokio::time::timeout(std::time::Duration::from_secs(1), wait)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn completion_clock_rollback_cannot_shorten_the_six_hour_throttle() {
        let executor = Arc::new(GatedPurgeTaskExecutor::new());
        let statuses = Arc::new(FakeMaintenanceStatusStore::new());
        let base = utc(2026, Month::July, 26, 12, 0);
        let now = Arc::new(Mutex::new(base));
        let observed_now = Arc::clone(&now);
        let controller = LocalMaintenanceController::new(
            Arc::clone(&executor),
            Arc::clone(&statuses),
            move || *observed_now.lock().unwrap(),
        );
        let repository_id = "00000000-0000-4000-8000-00000000007f";

        assert!(controller
            .notify_sync_completion(repository_id, SyncTrigger::Manual, true)
            .unwrap());
        executor.started.acquire().await.unwrap().forget();
        *now.lock().unwrap() = base - Duration::hours(1);
        executor.release.add_permits(1);
        wait_for_status_writes(&statuses, 1).await;
        wait_for_idle(&controller, repository_id).await;
        assert_eq!(
            statuses.writes.lock().unwrap()[0].1,
            RepositoryMaintenance {
                last_local_purge_at: Some(base),
                next_local_purge_at: Some(base + Duration::hours(6)),
            }
        );

        *now.lock().unwrap() = base + Duration::hours(6) - Duration::milliseconds(1);
        assert!(!controller
            .notify_sync_completion(repository_id, SyncTrigger::Manual, true)
            .unwrap());
        *now.lock().unwrap() = base + Duration::hours(6);
        assert!(controller
            .notify_sync_completion(repository_id, SyncTrigger::Manual, true)
            .unwrap());
        executor.started.acquire().await.unwrap().forget();
        executor.release.add_permits(1);
        wait_for_status_writes(&statuses, 2).await;
    }

    #[tokio::test]
    async fn extreme_valid_timestamps_use_checked_six_hour_arithmetic() {
        let executor = Arc::new(ImmediatePurgeTaskExecutor {
            outcomes: Mutex::new(VecDeque::from([Ok(LocalPurgeOutcome::Skipped)])),
        });
        let statuses = Arc::new(FakeMaintenanceStatusStore::new());
        let maximum = super::maximum_maintenance_instant();
        let now = Arc::new(Mutex::new(
            maximum.checked_sub(Duration::nanoseconds(1)).unwrap(),
        ));
        let observed_now = Arc::clone(&now);
        let controller =
            LocalMaintenanceController::new(executor, Arc::clone(&statuses), move || {
                *observed_now.lock().unwrap()
            });
        let repository_id = "00000000-0000-4000-8000-000000000080";
        statuses.values.lock().unwrap().insert(
            repository_id.to_owned(),
            RepositoryMaintenance {
                last_local_purge_at: Some(maximum),
                next_local_purge_at: None,
            },
        );

        assert!(!controller
            .notify_sync_completion(repository_id, SyncTrigger::Manual, true)
            .unwrap());
        *now.lock().unwrap() = maximum;
        assert!(controller
            .notify_sync_completion(repository_id, SyncTrigger::Manual, true)
            .unwrap());
        wait_for_status_writes(&statuses, 1).await;
        assert_eq!(
            statuses.writes.lock().unwrap()[0].1,
            RepositoryMaintenance {
                last_local_purge_at: Some(maximum),
                next_local_purge_at: Some(maximum),
            }
        );
    }

    #[tokio::test]
    async fn twelve_hour_timeout_sets_the_executor_flag_and_waits_for_cooperative_exit() {
        let executor = Arc::new(CancellationAwarePurgeTaskExecutor::new());
        let timer = Arc::new(ManualMaintenanceTimer::new());
        let statuses = Arc::new(FakeMaintenanceStatusStore::new());
        let now = utc(2026, Month::July, 26, 12, 0);
        let controller = LocalMaintenanceController::new_with_timer(
            Arc::clone(&executor),
            Arc::clone(&statuses),
            move || now,
            Arc::clone(&timer),
        );
        let repository_id = "00000000-0000-4000-8000-000000000081";

        assert!(controller
            .notify_sync_completion(repository_id, SyncTrigger::Manual, true)
            .unwrap());
        executor.started.acquire().await.unwrap().forget();
        assert_eq!(
            timer.requested.lock().unwrap().as_slice(),
            [std::time::Duration::from_secs(12 * 60 * 60)]
        );
        timer.fired.add_permits(1);
        executor.saw_cancel.acquire().await.unwrap().forget();
        assert!(executor.cancellations.lock().unwrap()[0].load(Ordering::Acquire));
        assert!(controller.is_running(repository_id));
        assert!(statuses.writes.lock().unwrap().is_empty());

        executor.release.add_permits(1);
        wait_for_status_writes(&statuses, 1).await;
        wait_for_idle(&controller, repository_id).await;
    }

    #[tokio::test]
    async fn repository_cancellation_sets_the_flag_before_waiting_for_lane_release() {
        let executor = Arc::new(CancellationAwarePurgeTaskExecutor::new());
        let timer = Arc::new(ManualMaintenanceTimer::new());
        let statuses = Arc::new(FakeMaintenanceStatusStore::new());
        let now = utc(2026, Month::July, 26, 12, 0);
        let controller = LocalMaintenanceController::new_with_timer(
            Arc::clone(&executor),
            Arc::clone(&statuses),
            move || now,
            timer,
        );
        let repository_id = "00000000-0000-4000-8000-000000000082";
        assert!(controller
            .notify_sync_completion(repository_id, SyncTrigger::Manual, true)
            .unwrap());
        executor.started.acquire().await.unwrap().forget();

        let cancellation = tokio::spawn({
            let controller = controller.clone();
            async move { controller.cancel_repository_and_wait(repository_id).await }
        });
        executor.saw_cancel.acquire().await.unwrap().forget();
        assert!(!cancellation.is_finished());
        assert!(controller.is_running(repository_id));

        executor.release.add_permits(1);
        assert!(cancellation.await.unwrap());
        assert!(!controller.is_running(repository_id));
        wait_for_status_writes(&statuses, 1).await;
    }

    #[tokio::test]
    async fn cancel_all_marks_every_attempt_before_waiting_and_releases_all_lanes() {
        let executor = Arc::new(CancellationAwarePurgeTaskExecutor::new());
        let timer = Arc::new(ManualMaintenanceTimer::new());
        let statuses = Arc::new(FakeMaintenanceStatusStore::new());
        let now = utc(2026, Month::July, 26, 12, 0);
        let controller = LocalMaintenanceController::new_with_timer(
            Arc::clone(&executor),
            Arc::clone(&statuses),
            move || now,
            timer,
        );
        let first = "00000000-0000-4000-8000-000000000083";
        let second = "00000000-0000-4000-8000-000000000084";
        for repository_id in [first, second] {
            assert!(controller
                .notify_sync_completion(repository_id, SyncTrigger::Manual, true)
                .unwrap());
        }
        executor.started.acquire_many(2).await.unwrap().forget();

        let cancellation = tokio::spawn({
            let controller = controller.clone();
            async move { controller.cancel_all_and_wait().await }
        });
        executor.saw_cancel.acquire_many(2).await.unwrap().forget();
        assert!(!cancellation.is_finished());
        assert!(executor
            .cancellations
            .lock()
            .unwrap()
            .iter()
            .all(|cancelled| cancelled.load(Ordering::Acquire)));

        executor.release.add_permits(2);
        assert_eq!(cancellation.await.unwrap(), 2);
        assert!(!controller.is_running(first));
        assert!(!controller.is_running(second));
        wait_for_status_writes(&statuses, 2).await;
        assert!(!controller.cancel_repository_and_wait(first).await);
    }

    #[tokio::test]
    async fn suspension_linearizes_before_cancellation_and_rejects_new_attempts_until_resumed() {
        let executor = Arc::new(CancellationAwarePurgeTaskExecutor::new());
        let statuses = Arc::new(FakeMaintenanceStatusStore::new());
        let now = utc(2026, Month::July, 26, 12, 0);
        let controller = LocalMaintenanceController::new(
            Arc::clone(&executor),
            Arc::clone(&statuses),
            move || now,
        );
        let first = "00000000-0000-4000-8000-000000000085";
        let later = "00000000-0000-4000-8000-000000000086";
        assert!(controller
            .notify_sync_completion(first, SyncTrigger::Manual, true)
            .unwrap());
        executor.started.acquire().await.unwrap().forget();

        let suspension = tokio::spawn({
            let controller = controller.clone();
            async move { controller.suspend_all_and_wait().await }
        });
        executor.saw_cancel.acquire().await.unwrap().forget();
        assert!(!controller
            .notify_sync_completion(later, SyncTrigger::Manual, true)
            .unwrap());
        assert!(!controller.try_daily(later).unwrap());

        executor.release.add_permits(1);
        assert_eq!(suspension.await.unwrap(), 1);
        controller.resume();
        assert!(controller
            .notify_sync_completion(later, SyncTrigger::Manual, true)
            .unwrap());
        executor.started.acquire().await.unwrap().forget();
        let cancellation = tokio::spawn({
            let controller = controller.clone();
            async move { controller.cancel_repository_and_wait(later).await }
        });
        executor.saw_cancel.acquire().await.unwrap().forget();
        executor.release.add_permits(1);
        assert!(cancellation.await.unwrap());
        wait_for_status_writes(&statuses, 2).await;
    }

    #[tokio::test]
    async fn suspension_waits_for_tracked_daily_housekeeping_and_blocks_a_later_tick() {
        let now = utc(2026, Month::July, 26, 12, 0);
        let controller = LocalMaintenanceController::new(
            Arc::new(ImmediatePurgeTaskExecutor {
                outcomes: Mutex::new(VecDeque::new()),
            }),
            Arc::new(FakeMaintenanceStatusStore::new()),
            move || now,
        );
        let started = Arc::new(tokio::sync::Semaphore::new(0));
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let housekeeping = tokio::spawn({
            let controller = controller.clone();
            let started = Arc::clone(&started);
            let release = Arc::clone(&release);
            async move {
                controller
                    .run_daily_housekeeping_operation(async move {
                        started.add_permits(1);
                        release.acquire().await.unwrap().forget();
                    })
                    .await
            }
        });
        started.acquire().await.unwrap().forget();

        let suspension = tokio::spawn({
            let controller = controller.clone();
            async move { controller.suspend_all_and_wait().await }
        });
        tokio::task::yield_now().await;
        assert!(!suspension.is_finished());
        assert!(controller
            .run_daily_housekeeping_operation(async {})
            .await
            .is_none());

        release.add_permits(1);
        assert!(housekeeping.await.unwrap().is_some());
        assert_eq!(suspension.await.unwrap(), 1);
        controller.resume();
        assert!(controller
            .run_daily_housekeeping_operation(async {})
            .await
            .is_some());
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

    #[test]
    fn daily_repository_resolution_reloads_and_skips_disabled_or_offline_roots() {
        let root = tempdir().unwrap();
        let app_data = root.path().join("app-data");
        let first_root = root.path().join("first-notes");
        let disabled_root = root.path().join("disabled-notes");
        let offline_root = root.path().join("offline-notes");
        for notes_root in [&first_root, &disabled_root, &offline_root] {
            std::fs::create_dir(notes_root).unwrap();
        }
        let state_service = LocalSyncStateService::new(&app_data);
        let mut state = state_service.load_or_initialize(None).unwrap();
        let first = "00000000-0000-4000-8000-000000000091";
        let disabled = "00000000-0000-4000-8000-000000000092";
        let offline = "00000000-0000-4000-8000-000000000093";
        for (repository_id, notes_root) in [
            (first, &first_root),
            (disabled, &disabled_root),
            (offline, &offline_root),
        ] {
            state_service
                .add_binding(
                    &mut state,
                    RepositoryBinding {
                        repository_id: repository_id.to_owned(),
                        display_name: repository_id.to_owned(),
                        notes_root: notes_root.clone(),
                        enabled: true,
                    },
                )
                .unwrap();
        }
        state.bindings[1].enabled = false;
        state_service.save(&state).unwrap();
        std::fs::remove_dir(&offline_root).unwrap();

        assert_eq!(resolve_daily_repository_ids(&app_data).unwrap(), [first]);

        state.bindings[0].enabled = false;
        state.bindings[1].enabled = true;
        state_service.save(&state).unwrap();
        assert_eq!(resolve_daily_repository_ids(&app_data).unwrap(), [disabled]);
    }

    #[tokio::test(start_paused = true)]
    async fn production_daily_maintenance_waits_exactly_twenty_four_hours_before_first_run() {
        let root = tempdir().unwrap();
        let app_data = root.path().join("app-data");
        let notes_root = root.path().join("notes");
        std::fs::create_dir(&notes_root).unwrap();
        let repository_id = "00000000-0000-4000-8000-000000000094";
        let state_service = LocalSyncStateService::new(&app_data);
        let mut state = state_service.load_or_initialize(None).unwrap();
        state_service
            .add_binding(
                &mut state,
                RepositoryBinding {
                    repository_id: repository_id.to_owned(),
                    display_name: "Daily".to_owned(),
                    notes_root,
                    enabled: true,
                },
            )
            .unwrap();

        let executor = Arc::new(GatedPurgeTaskExecutor::new());
        let statuses = Arc::new(FakeMaintenanceStatusStore::new());
        let now = utc(2026, Month::July, 26, 12, 0);
        statuses.values.lock().unwrap().insert(
            repository_id.to_owned(),
            RepositoryMaintenance {
                last_local_purge_at: Some(now - Duration::hours(6)),
                next_local_purge_at: Some(now),
            },
        );
        let controller = Arc::new(LocalMaintenanceController::new(
            Arc::clone(&executor),
            statuses,
            move || now,
        ));
        spawn_daily_maintenance_with(app_data.clone(), controller, |future| {
            tokio::spawn(future);
        });
        tokio::task::yield_now().await;

        tokio::time::advance(std::time::Duration::from_secs(24 * 60 * 60 - 1)).await;
        tokio::task::yield_now().await;
        assert_eq!(executor.calls.load(Ordering::SeqCst), 0);

        tokio::time::advance(std::time::Duration::from_secs(1)).await;
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            executor.started.acquire(),
        )
        .await
        .unwrap()
        .unwrap()
        .forget();
        assert_eq!(executor.calls.load(Ordering::SeqCst), 1);
        executor.release.add_permits(1);
    }
}
