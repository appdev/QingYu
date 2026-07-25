//! Background timing and retry policy ported from SiYuan's production sync model.
//! Rust owns concurrency and typed errors; the scheduling behavior follows the
//! pinned upstream `kernel/model/sync.go` and `kernel/model/repository.go`.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use time::OffsetDateTime;
use tokio::sync::Notify;
use tokio::time::Instant;

use super::local_state::LocalSyncStateService;
use super::service::{
    AcceptedSyncJob, DejavuSyncService, RepositoryJobCompletion, RepositoryJobError,
    RepositoryJobLifecycle, SyncJobRequest,
};
use super::status::{RepositorySchedule, RepositoryStatusStore};
use crate::protected_paths::path_contains_qingyu_control_directory;
use crate::sync_config::model::{SyncMode, SyncProvider};
use crate::sync_config::ready_snapshot_at_app_data;
use crate::sync_config::status::SyncTrigger;

const ORDINARY_FAILURE_DELAY: Duration = Duration::from_secs(5 * 60);
const FAILURE_PAUSE_DELAY: Duration = Duration::from_secs(64 * 60);
const DNS_RETRY_THROTTLE: Duration = Duration::from_secs(5 * 60);
const MINIMUM_NO_CHANGE_DELAY_MINUTES: u64 = 8;

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
type SchedulerClock = dyn Fn() -> OffsetDateTime + Send + Sync;

#[derive(Clone)]
pub(crate) struct ActiveRepositorySchedule {
    pub(crate) notes_root: PathBuf,
    pub(crate) repository_id: String,
    pub(crate) mode: SyncMode,
    pub(crate) interval: Duration,
}

pub(crate) trait RepositoryScheduleSource: Send + Sync {
    fn resolve_active_root(
        &self,
        root: &Path,
    ) -> Result<Option<ActiveRepositorySchedule>, RepositoryJobError>;
}

pub(crate) trait RepositoryScheduleStore: Send + Sync {
    fn load_schedule(&self, repository_id: &str) -> Result<RepositorySchedule, RepositoryJobError>;
    fn save_schedule(
        &self,
        repository_id: &str,
        schedule: RepositorySchedule,
    ) -> Result<(), RepositoryJobError>;
    fn reserve_dns_retry(
        &self,
        repository_id: &str,
        now: OffsetDateTime,
        throttle: Duration,
    ) -> Result<bool, RepositoryJobError>;
}

pub(crate) trait SchedulerJobEnqueuer: Send + Sync {
    fn enqueue<'a>(
        &'a self,
        request: SyncJobRequest,
    ) -> BoxFuture<'a, Result<AcceptedSyncJob, RepositoryJobError>>;
}

pub(crate) trait DnsFlusher: Send + Sync {
    fn flush(&self);
}

#[derive(Clone)]
pub(crate) struct DejavuScheduler {
    inner: Arc<SchedulerInner>,
}

struct SchedulerInner {
    source: Arc<dyn RepositoryScheduleSource>,
    enqueuer: Arc<dyn SchedulerJobEnqueuer>,
    store: Arc<dyn RepositoryScheduleStore>,
    dns_flusher: Arc<dyn DnsFlusher>,
    clock: Arc<SchedulerClock>,
    state: Mutex<SchedulerState>,
    changed: Arc<Notify>,
}

#[derive(Default)]
struct SchedulerState {
    active: Option<ActiveRepositorySchedule>,
    due: Option<Instant>,
    generation: u64,
}

impl DejavuScheduler {
    #[allow(dead_code)]
    pub(crate) fn new<Source, Enqueuer, Store, Flusher>(
        source: Arc<Source>,
        enqueuer: Arc<Enqueuer>,
        store: Arc<Store>,
        dns_flusher: Arc<Flusher>,
        clock: Arc<SchedulerClock>,
    ) -> Self
    where
        Source: RepositoryScheduleSource + 'static,
        Enqueuer: SchedulerJobEnqueuer + 'static,
        Store: RepositoryScheduleStore + 'static,
        Flusher: DnsFlusher + 'static,
    {
        let inner = Arc::new(SchedulerInner {
            source,
            enqueuer,
            store,
            dns_flusher,
            clock,
            state: Mutex::new(SchedulerState::default()),
            changed: Arc::new(Notify::new()),
        });
        tokio::spawn(run_timer(Arc::downgrade(&inner)));
        Self { inner }
    }

    pub(crate) fn activate_root(&self, root: &Path) -> Result<bool, RepositoryJobError> {
        let Some(active) = self.inner.source.resolve_active_root(root)? else {
            return Ok(false);
        };
        if active.notes_root != root || active.interval.is_zero() {
            return Err(RepositoryJobError::InvalidBinding);
        }
        let schedule = self.inner.store.load_schedule(&active.repository_id)?;
        let due = (active.mode == SyncMode::Automatic)
            .then_some(schedule.next_scheduled_at)
            .flatten()
            .map(|next| wall_time_to_deadline((self.inner.clock)(), next));
        let mut state = self.inner.state.lock().unwrap();
        state.active = Some(active);
        state.due = due;
        state.generation = state.generation.wrapping_add(1);
        drop(state);
        self.inner.changed.notify_one();
        Ok(true)
    }

    pub(crate) fn deactivate_root(&self, root: &Path) -> bool {
        let mut state = self.inner.state.lock().unwrap();
        if state
            .active
            .as_ref()
            .is_none_or(|active| active.notes_root != root)
        {
            return false;
        }
        state.active = None;
        state.due = None;
        state.generation = state.generation.wrapping_add(1);
        drop(state);
        self.inner.changed.notify_one();
        true
    }

    pub(crate) fn record_file_change(
        &self,
        event_root: &Path,
        event_path: &Path,
    ) -> Result<bool, RepositoryJobError> {
        let active = self.active_snapshot();
        let Some(active) = active else {
            return Ok(false);
        };
        if active.notes_root != event_root
            || event_path
                .strip_prefix(event_root)
                .ok()
                .is_none_or(|relative| {
                    relative.as_os_str().is_empty()
                        || path_contains_qingyu_control_directory(relative)
                })
            || active.mode != SyncMode::Automatic
        {
            return Ok(false);
        }
        let mut schedule = self.inner.store.load_schedule(&active.repository_id)?;
        schedule.same_count = 0;
        self.schedule_after(&active, schedule, active.interval)?;
        Ok(true)
    }

    pub(crate) async fn trigger_startup(&self) -> Result<bool, RepositoryJobError> {
        self.trigger_now(SyncTrigger::AppLaunch).await
    }

    pub(crate) async fn trigger_exit(&self) -> Result<bool, RepositoryJobError> {
        // The public trigger schema remains unchanged until the S3 cutover.
        self.trigger_now(SyncTrigger::SettingsExit).await
    }

    #[allow(dead_code)]
    pub(crate) async fn trigger_manual(&self) -> Result<bool, RepositoryJobError> {
        self.trigger_now(SyncTrigger::Manual).await
    }

    async fn trigger_now(&self, trigger: SyncTrigger) -> Result<bool, RepositoryJobError> {
        let Some(active) = self.active_snapshot() else {
            return Ok(false);
        };
        if !trigger_allowed(active.mode, trigger) {
            return Ok(false);
        }
        let schedule = self.inner.store.load_schedule(&active.repository_id)?;
        if trigger != SyncTrigger::Manual && schedule.automatic_failure_count >= 8 {
            if active.mode == SyncMode::Automatic {
                self.schedule_after(&active, schedule, FAILURE_PAUSE_DELAY)?;
            }
            return Ok(false);
        }
        self.enqueue_active(active, trigger).await?;
        Ok(true)
    }

    async fn enqueue_active(
        &self,
        active: ActiveRepositorySchedule,
        trigger: SyncTrigger,
    ) -> Result<(), RepositoryJobError> {
        let request = SyncJobRequest {
            notes_root: active.notes_root,
            repository_id: active.repository_id,
            trigger,
        };
        match self.inner.enqueuer.enqueue(request.clone()).await {
            Ok(_) => Ok(()),
            Err(error) => {
                let _recorded = self.record_completion(RepositoryJobCompletion {
                    request,
                    result: Err(error),
                });
                Err(error)
            }
        }
    }

    pub(crate) fn prepare_dns_retry(
        &self,
        request: &SyncJobRequest,
    ) -> Result<bool, RepositoryJobError> {
        let now = (self.inner.clock)();
        if !self
            .inner
            .store
            .reserve_dns_retry(&request.repository_id, now, DNS_RETRY_THROTTLE)?
        {
            return Ok(false);
        }
        self.inner.dns_flusher.flush();
        Ok(true)
    }

    pub(crate) fn record_completion(
        &self,
        completion: RepositoryJobCompletion,
    ) -> Result<(), RepositoryJobError> {
        let repository_id = &completion.request.repository_id;
        let mut schedule = self.inner.store.load_schedule(repository_id)?;
        let active = self
            .active_snapshot()
            .filter(|active| active.repository_id == *repository_id);
        match completion.result {
            Ok(result) => {
                schedule.automatic_failure_count = 0;
                if completion.request.trigger == SyncTrigger::Manual {
                    return self.inner.store.save_schedule(repository_id, schedule);
                }
                let should_poll = completion.request.trigger != SyncTrigger::Manual
                    && active
                        .as_ref()
                        .is_some_and(|active| active.mode == SyncMode::Automatic);
                if !should_poll {
                    schedule.next_scheduled_at = None;
                    self.inner.store.save_schedule(repository_id, schedule)?;
                    self.clear_due_if_active(repository_id);
                    return Ok(());
                }
                let active = active.expect("active automatic schedule was checked");
                let delay = if result.data_changed {
                    active.interval
                } else {
                    next_no_change_delay(&mut schedule.same_count)
                };
                self.schedule_after(&active, schedule, delay)
            }
            Err(error) => {
                if completion.request.trigger != SyncTrigger::Manual
                    && error != RepositoryJobError::Cancelled
                {
                    schedule.automatic_failure_count =
                        schedule.automatic_failure_count.saturating_add(1);
                }
                if completion.request.trigger == SyncTrigger::Manual {
                    return self.inner.store.save_schedule(repository_id, schedule);
                }
                let should_retry = error != RepositoryJobError::Cancelled
                    && completion.request.trigger != SyncTrigger::Manual
                    && active
                        .as_ref()
                        .is_some_and(|active| active.mode == SyncMode::Automatic);
                if !should_retry {
                    schedule.next_scheduled_at = None;
                    self.inner.store.save_schedule(repository_id, schedule)?;
                    self.clear_due_if_active(repository_id);
                    return Ok(());
                }
                let delay = if schedule.automatic_failure_count >= 8 {
                    FAILURE_PAUSE_DELAY
                } else {
                    ORDINARY_FAILURE_DELAY
                };
                self.schedule_after(&active.unwrap(), schedule, delay)
            }
        }
    }

    fn schedule_after(
        &self,
        active: &ActiveRepositorySchedule,
        mut schedule: RepositorySchedule,
        delay: Duration,
    ) -> Result<(), RepositoryJobError> {
        schedule.next_scheduled_at = Some((self.inner.clock)() + delay);
        self.inner
            .store
            .save_schedule(&active.repository_id, schedule)?;
        let mut state = self.inner.state.lock().unwrap();
        if state
            .active
            .as_ref()
            .is_some_and(|current| current.repository_id == active.repository_id)
        {
            state.due = Some(Instant::now() + delay);
            state.generation = state.generation.wrapping_add(1);
        }
        drop(state);
        self.inner.changed.notify_one();
        Ok(())
    }

    fn clear_due_if_active(&self, repository_id: &str) {
        let mut state = self.inner.state.lock().unwrap();
        if state
            .active
            .as_ref()
            .is_some_and(|active| active.repository_id == repository_id)
        {
            state.due = None;
            state.generation = state.generation.wrapping_add(1);
        }
        drop(state);
        self.inner.changed.notify_one();
    }

    fn active_snapshot(&self) -> Option<ActiveRepositorySchedule> {
        self.inner.state.lock().unwrap().active.clone()
    }
}

impl RepositoryJobLifecycle for DejavuScheduler {
    fn prepare_dns_retry(&self, request: &SyncJobRequest) -> Result<bool, RepositoryJobError> {
        DejavuScheduler::prepare_dns_retry(self, request)
    }

    fn record_completion(
        &self,
        completion: RepositoryJobCompletion,
    ) -> Result<(), RepositoryJobError> {
        DejavuScheduler::record_completion(self, completion)
    }
}

impl SchedulerJobEnqueuer for DejavuSyncService {
    fn enqueue<'a>(
        &'a self,
        request: SyncJobRequest,
    ) -> BoxFuture<'a, Result<AcceptedSyncJob, RepositoryJobError>> {
        Box::pin(async move { DejavuSyncService::enqueue(self, request).await })
    }
}

impl RepositoryScheduleStore for RepositoryStatusStore {
    fn load_schedule(&self, repository_id: &str) -> Result<RepositorySchedule, RepositoryJobError> {
        RepositoryStatusStore::load_schedule(self, repository_id)
    }

    fn save_schedule(
        &self,
        repository_id: &str,
        schedule: RepositorySchedule,
    ) -> Result<(), RepositoryJobError> {
        RepositoryStatusStore::save_schedule(self, repository_id, schedule)
    }

    fn reserve_dns_retry(
        &self,
        repository_id: &str,
        now: OffsetDateTime,
        throttle: Duration,
    ) -> Result<bool, RepositoryJobError> {
        RepositoryStatusStore::reserve_dns_retry(self, repository_id, now, throttle)
    }
}

#[allow(dead_code)]
async fn run_timer(weak_inner: Weak<SchedulerInner>) {
    loop {
        let Some(inner) = weak_inner.upgrade() else {
            return;
        };
        let notified = Arc::clone(&inner.changed).notified_owned();
        let timer = {
            let state = inner.state.lock().unwrap();
            state.due.map(|due| (due, state.generation))
        };
        drop(inner);
        let Some((due, generation)) = timer else {
            notified.await;
            continue;
        };
        tokio::select! {
            () = tokio::time::sleep_until(due) => {
                let Some(inner) = weak_inner.upgrade() else {
                    return;
                };
                let active = {
                    let mut state = inner.state.lock().unwrap();
                    if state.generation != generation
                        || state.due.is_none_or(|current| current > Instant::now())
                    {
                        None
                    } else {
                        state.due = None;
                        state.active.clone().map(|active| (active, state.generation))
                    }
                };
                if let Some((active, accepted_generation)) = active {
                    let mut schedule = match inner.store.load_schedule(&active.repository_id) {
                        Ok(schedule) => schedule,
                        Err(_) => continue,
                    };
                    schedule.next_scheduled_at = None;
                    if inner.store.save_schedule(&active.repository_id, schedule).is_err() {
                        continue;
                    }
                    let still_active = {
                        let state = inner.state.lock().unwrap();
                        state.generation == accepted_generation
                            && state.active.as_ref().is_some_and(|current| {
                                current.repository_id == active.repository_id
                                    && current.notes_root == active.notes_root
                                    && current.mode == SyncMode::Automatic
                            })
                    };
                    if !still_active {
                        continue;
                    }
                    let scheduler = DejavuScheduler { inner };
                    let _accepted = scheduler
                        .enqueue_active(active, SyncTrigger::Interval)
                        .await;
                }
            }
            () = notified => {}
        }
    }
}

fn trigger_allowed(mode: SyncMode, trigger: SyncTrigger) -> bool {
    match mode {
        SyncMode::Automatic => matches!(
            trigger,
            SyncTrigger::AppLaunch
                | SyncTrigger::Interval
                | SyncTrigger::Manual
                | SyncTrigger::Save
                | SyncTrigger::SettingsExit
        ),
        SyncMode::StartupExit => matches!(
            trigger,
            SyncTrigger::AppLaunch | SyncTrigger::Manual | SyncTrigger::SettingsExit
        ),
        SyncMode::FullyManual => trigger == SyncTrigger::Manual,
    }
}

fn next_no_change_delay(same_count: &mut u32) -> Duration {
    *same_count = same_count.saturating_add(1);
    if *same_count > 10 {
        *same_count = 5;
    }
    let exponential = 1_u64.checked_shl(*same_count).unwrap_or(u64::MAX);
    Duration::from_secs(
        MINIMUM_NO_CHANGE_DELAY_MINUTES
            .max(exponential)
            .saturating_mul(60),
    )
}

fn wall_time_to_deadline(now: OffsetDateTime, due: OffsetDateTime) -> Instant {
    let seconds = (due - now).whole_seconds();
    if seconds <= 0 {
        Instant::now()
    } else {
        Instant::now() + Duration::from_secs(seconds as u64)
    }
}

#[allow(dead_code)]
pub(crate) struct LocalRepositoryScheduleSource {
    app_data: PathBuf,
}

impl LocalRepositoryScheduleSource {
    #[allow(dead_code)]
    pub(crate) fn new(app_data: impl AsRef<Path>) -> Self {
        Self {
            app_data: app_data.as_ref().to_path_buf(),
        }
    }
}

impl RepositoryScheduleSource for LocalRepositoryScheduleSource {
    fn resolve_active_root(
        &self,
        root: &Path,
    ) -> Result<Option<ActiveRepositorySchedule>, RepositoryJobError> {
        let canonical = root
            .canonicalize()
            .map_err(|_| RepositoryJobError::InvalidBinding)?;
        if canonical != root {
            return Err(RepositoryJobError::InvalidBinding);
        }
        let state = LocalSyncStateService::new(&self.app_data)
            .load()
            .map_err(|_| RepositoryJobError::InvalidBinding)?
            .ok_or(RepositoryJobError::InvalidBinding)?;
        let Some(binding) = state
            .bindings
            .into_iter()
            .find(|binding| binding.enabled && binding.notes_root == canonical)
        else {
            return Ok(None);
        };
        let snapshot = ready_snapshot_at_app_data(&self.app_data, None)
            .map_err(|_| RepositoryJobError::ConfigUnavailable)?;
        if snapshot.config.provider != SyncProvider::S3 {
            return Ok(None);
        }
        Ok(Some(ActiveRepositorySchedule {
            notes_root: canonical,
            repository_id: binding.repository_id,
            mode: snapshot.config.mode,
            interval: Duration::from_secs(u64::from(snapshot.config.interval_seconds)),
        }))
    }
}

#[derive(Default)]
#[allow(dead_code)]
pub(crate) struct SystemDnsFlusher;

impl DnsFlusher for SystemDnsFlusher {
    fn flush(&self) {
        #[cfg(windows)]
        {
            let _result = std::process::Command::new("ipconfig")
                .arg("/flushdns")
                .output();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use time::OffsetDateTime;
    use tokio::sync::mpsc;

    use super::{
        ActiveRepositorySchedule, DejavuScheduler, DnsFlusher, RepositoryScheduleSource,
        RepositoryScheduleStore, SchedulerJobEnqueuer,
    };
    use crate::dejavu_sync::service::{
        AcceptedSyncJob, RepositoryJobCompletion, RepositoryJobError, RepositorySyncResult,
        SyncJobRequest,
    };
    use crate::dejavu_sync::status::RepositorySchedule;
    use crate::sync_config::model::SyncMode;
    use crate::sync_config::status::SyncTrigger;

    type BoxFuture<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

    #[derive(Default)]
    struct MemorySource {
        roots: Mutex<HashMap<PathBuf, ActiveRepositorySchedule>>,
    }

    impl MemorySource {
        fn bind(&self, root: &Path, repository_id: &str, mode: SyncMode, interval: Duration) {
            self.roots.lock().unwrap().insert(
                root.to_path_buf(),
                ActiveRepositorySchedule {
                    notes_root: root.to_path_buf(),
                    repository_id: repository_id.to_owned(),
                    mode,
                    interval,
                },
            );
        }
    }

    impl RepositoryScheduleSource for MemorySource {
        fn resolve_active_root(
            &self,
            root: &Path,
        ) -> Result<Option<ActiveRepositorySchedule>, RepositoryJobError> {
            Ok(self.roots.lock().unwrap().get(root).cloned())
        }
    }

    #[derive(Default)]
    struct MemoryStore(Mutex<HashMap<String, RepositorySchedule>>);

    impl RepositoryScheduleStore for MemoryStore {
        fn load_schedule(
            &self,
            repository_id: &str,
        ) -> Result<RepositorySchedule, RepositoryJobError> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .get(repository_id)
                .cloned()
                .unwrap_or_default())
        }

        fn save_schedule(
            &self,
            repository_id: &str,
            schedule: RepositorySchedule,
        ) -> Result<(), RepositoryJobError> {
            self.0
                .lock()
                .unwrap()
                .insert(repository_id.to_owned(), schedule);
            Ok(())
        }

        fn reserve_dns_retry(
            &self,
            repository_id: &str,
            now: OffsetDateTime,
            throttle: Duration,
        ) -> Result<bool, RepositoryJobError> {
            let mut schedules = self.0.lock().unwrap();
            let schedule = schedules.entry(repository_id.to_owned()).or_default();
            if schedule
                .last_dns_retry_at
                .is_some_and(|last| now - last < throttle)
            {
                return Ok(false);
            }
            schedule.last_dns_retry_at = Some(now);
            Ok(true)
        }
    }

    struct RecordingEnqueuer {
        sent: mpsc::UnboundedSender<SyncJobRequest>,
    }

    impl SchedulerJobEnqueuer for RecordingEnqueuer {
        fn enqueue<'a>(
            &'a self,
            request: SyncJobRequest,
        ) -> BoxFuture<'a, Result<AcceptedSyncJob, RepositoryJobError>> {
            Box::pin(async move {
                self.sent.send(request.clone()).unwrap();
                Ok(AcceptedSyncJob {
                    job_id: "00000000-0000-4000-8000-000000000099".to_owned(),
                    repository_id: request.repository_id,
                    notes_root: request.notes_root,
                })
            })
        }
    }

    #[derive(Default)]
    struct FailingEnqueuer(AtomicUsize);

    impl SchedulerJobEnqueuer for FailingEnqueuer {
        fn enqueue<'a>(
            &'a self,
            _request: SyncJobRequest,
        ) -> BoxFuture<'a, Result<AcceptedSyncJob, RepositoryJobError>> {
            Box::pin(async move {
                self.0.fetch_add(1, Ordering::SeqCst);
                Err(RepositoryJobError::CloudUnavailable)
            })
        }
    }

    #[derive(Default)]
    struct RecordingFlusher(AtomicUsize);

    impl DnsFlusher for RecordingFlusher {
        fn flush(&self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    struct TestClock(AtomicI64);

    impl TestClock {
        fn new(unix_seconds: i64) -> Self {
            Self(AtomicI64::new(unix_seconds))
        }

        fn now(&self) -> OffsetDateTime {
            OffsetDateTime::from_unix_timestamp(self.0.load(Ordering::SeqCst)).unwrap()
        }

        fn advance(&self, duration: Duration) {
            self.0
                .fetch_add(duration.as_secs() as i64, Ordering::SeqCst);
        }
    }

    struct Fixture {
        scheduler: DejavuScheduler,
        source: Arc<MemorySource>,
        store: Arc<MemoryStore>,
        flusher: Arc<RecordingFlusher>,
        clock: Arc<TestClock>,
        jobs: mpsc::UnboundedReceiver<SyncJobRequest>,
    }

    impl Fixture {
        fn new() -> Self {
            let source = Arc::new(MemorySource::default());
            let store = Arc::new(MemoryStore::default());
            let flusher = Arc::new(RecordingFlusher::default());
            let clock = Arc::new(TestClock::new(1_800_000_000));
            let (sent, jobs) = mpsc::unbounded_channel();
            let scheduler = DejavuScheduler::new(
                Arc::clone(&source),
                Arc::new(RecordingEnqueuer { sent }),
                Arc::clone(&store),
                Arc::clone(&flusher),
                {
                    let clock = Arc::clone(&clock);
                    Arc::new(move || clock.now())
                },
            );
            Self {
                scheduler,
                source,
                store,
                flusher,
                clock,
                jobs,
            }
        }

        async fn receive(&mut self) -> SyncJobRequest {
            self.jobs.recv().await.expect("scheduler should enqueue")
        }
    }

    fn repository_id(suffix: u8) -> String {
        format!("00000000-0000-4000-8000-{suffix:012}")
    }

    fn completion(
        root: &Path,
        repository_id: &str,
        trigger: SyncTrigger,
        result: Result<RepositorySyncResult, RepositoryJobError>,
    ) -> RepositoryJobCompletion {
        RepositoryJobCompletion {
            request: SyncJobRequest {
                notes_root: root.to_path_buf(),
                repository_id: repository_id.to_owned(),
                trigger,
            },
            result,
        }
    }

    #[tokio::test(start_paused = true)]
    async fn modes_accept_only_the_siyuan_trigger_matrix() {
        for (mode, startup, exit, manual) in [
            (SyncMode::Automatic, true, true, true),
            (SyncMode::StartupExit, true, true, true),
            (SyncMode::FullyManual, false, false, true),
        ] {
            let mut fixture = Fixture::new();
            let root = PathBuf::from(format!("/notes/{mode:?}"));
            let repository_id = repository_id(1);
            fixture
                .source
                .bind(&root, &repository_id, mode, Duration::from_secs(30));
            assert!(fixture.scheduler.activate_root(&root).unwrap());

            assert_eq!(fixture.scheduler.trigger_startup().await.unwrap(), startup);
            if startup {
                assert_eq!(fixture.receive().await.trigger, SyncTrigger::AppLaunch);
            }
            assert_eq!(fixture.scheduler.trigger_exit().await.unwrap(), exit);
            if exit {
                assert_eq!(fixture.receive().await.trigger, SyncTrigger::SettingsExit);
            }
            assert_eq!(fixture.scheduler.trigger_manual().await.unwrap(), manual);
            assert_eq!(fixture.receive().await.trigger, SyncTrigger::Manual);

            let file = root.join("note.md");
            assert_eq!(
                fixture.scheduler.record_file_change(&root, &file).unwrap(),
                mode == SyncMode::Automatic
            );
            if mode == SyncMode::Automatic {
                tokio::time::advance(Duration::from_secs(30)).await;
                assert_eq!(fixture.receive().await.trigger, SyncTrigger::Interval);
            }
        }
    }

    #[tokio::test(start_paused = true)]
    async fn file_change_resets_same_count_and_schedules_the_configured_interval() {
        let mut fixture = Fixture::new();
        let root = PathBuf::from("/notes/active");
        let repository_id = repository_id(2);
        fixture.source.bind(
            &root,
            &repository_id,
            SyncMode::Automatic,
            Duration::from_secs(90),
        );
        fixture.store.0.lock().unwrap().insert(
            repository_id.clone(),
            RepositorySchedule {
                same_count: 9,
                ..RepositorySchedule::default()
            },
        );
        fixture.scheduler.activate_root(&root).unwrap();

        assert!(fixture
            .scheduler
            .record_file_change(&root, &root.join("note.md"))
            .unwrap());
        assert_eq!(
            fixture
                .store
                .load_schedule(&repository_id)
                .unwrap()
                .same_count,
            0
        );
        tokio::time::advance(Duration::from_secs(89)).await;
        assert!(fixture.jobs.try_recv().is_err());
        tokio::time::advance(Duration::from_secs(1)).await;
        assert_eq!(fixture.receive().await.repository_id, repository_id);
    }

    #[tokio::test(start_paused = true)]
    async fn no_change_backoff_is_exact_and_resets_the_eleventh_count_to_five() {
        let fixture = Fixture::new();
        let root = PathBuf::from("/notes/backoff");
        let repository_id = repository_id(3);
        fixture.source.bind(
            &root,
            &repository_id,
            SyncMode::Automatic,
            Duration::from_secs(30),
        );
        fixture.scheduler.activate_root(&root).unwrap();

        let mut observed = Vec::new();
        for _ in 0..11 {
            fixture
                .scheduler
                .record_completion(completion(
                    &root,
                    &repository_id,
                    SyncTrigger::Interval,
                    Ok(RepositorySyncResult::default()),
                ))
                .unwrap();
            let schedule = fixture.store.load_schedule(&repository_id).unwrap();
            observed
                .push((schedule.next_scheduled_at.unwrap() - fixture.clock.now()).whole_minutes());
        }

        assert_eq!(observed, vec![8, 8, 8, 16, 32, 64, 128, 256, 512, 1024, 32]);
        assert_eq!(
            fixture
                .store
                .load_schedule(&repository_id)
                .unwrap()
                .same_count,
            5
        );
    }

    #[tokio::test(start_paused = true)]
    async fn ordinary_failures_wait_five_minutes_and_eighth_automatic_failure_waits_sixty_four() {
        let mut fixture = Fixture::new();
        let root = PathBuf::from("/notes/failures");
        let repository_id = repository_id(4);
        fixture.source.bind(
            &root,
            &repository_id,
            SyncMode::Automatic,
            Duration::from_secs(30),
        );
        fixture.scheduler.activate_root(&root).unwrap();

        for expected_count in 1..=8 {
            fixture
                .scheduler
                .record_completion(completion(
                    &root,
                    &repository_id,
                    SyncTrigger::Interval,
                    Err(RepositoryJobError::CloudUnavailable),
                ))
                .unwrap();
            let schedule = fixture.store.load_schedule(&repository_id).unwrap();
            let delay = (schedule.next_scheduled_at.unwrap() - fixture.clock.now()).whole_minutes();
            assert_eq!(schedule.automatic_failure_count, expected_count);
            assert_eq!(delay, if expected_count == 8 { 64 } else { 5 });
        }

        assert!(fixture.scheduler.trigger_manual().await.unwrap());
        assert_eq!(fixture.receive().await.trigger, SyncTrigger::Manual);
        fixture
            .scheduler
            .record_completion(completion(
                &root,
                &repository_id,
                SyncTrigger::Manual,
                Ok(RepositorySyncResult {
                    data_changed: true,
                    ..RepositorySyncResult::default()
                }),
            ))
            .unwrap();
        assert_eq!(
            fixture
                .store
                .load_schedule(&repository_id)
                .unwrap()
                .automatic_failure_count,
            0
        );
    }

    #[tokio::test(start_paused = true)]
    async fn the_single_timer_polls_only_the_current_active_root() {
        let mut fixture = Fixture::new();
        let old_root = PathBuf::from("/notes/old");
        let new_root = PathBuf::from("/notes/new");
        let old_id = repository_id(5);
        let new_id = repository_id(6);
        fixture.source.bind(
            &old_root,
            &old_id,
            SyncMode::Automatic,
            Duration::from_secs(30),
        );
        fixture.source.bind(
            &new_root,
            &new_id,
            SyncMode::Automatic,
            Duration::from_secs(30),
        );

        fixture.scheduler.activate_root(&old_root).unwrap();
        fixture
            .scheduler
            .record_file_change(&old_root, &old_root.join("old.md"))
            .unwrap();
        fixture.scheduler.activate_root(&new_root).unwrap();
        fixture
            .scheduler
            .record_file_change(&new_root, &new_root.join("new.md"))
            .unwrap();
        assert!(!fixture.scheduler.deactivate_root(&old_root));

        tokio::time::advance(Duration::from_secs(30)).await;
        let request = fixture.receive().await;
        assert_eq!(request.repository_id, new_id);
        assert!(fixture.jobs.try_recv().is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn an_accepted_job_finishes_after_deactivation_without_restarting_polling() {
        let mut fixture = Fixture::new();
        let root = PathBuf::from("/notes/closing");
        let repository_id = repository_id(11);
        fixture.source.bind(
            &root,
            &repository_id,
            SyncMode::Automatic,
            Duration::from_secs(30),
        );
        fixture.scheduler.activate_root(&root).unwrap();

        assert!(fixture.scheduler.trigger_startup().await.unwrap());
        let accepted = fixture.receive().await;
        assert!(fixture.scheduler.deactivate_root(&root));
        fixture
            .scheduler
            .record_completion(RepositoryJobCompletion {
                request: accepted,
                result: Ok(RepositorySyncResult::default()),
            })
            .unwrap();

        assert!(fixture
            .store
            .load_schedule(&repository_id)
            .unwrap()
            .next_scheduled_at
            .is_none());
        tokio::time::advance(Duration::from_secs(24 * 60 * 60)).await;
        assert!(fixture.jobs.try_recv().is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn activation_restores_a_persisted_due_time_only_for_automatic_mode() {
        for (mode, should_poll) in [
            (SyncMode::Automatic, true),
            (SyncMode::StartupExit, false),
            (SyncMode::FullyManual, false),
        ] {
            let mut fixture = Fixture::new();
            let root = PathBuf::from(format!("/notes/restart-{mode:?}"));
            let repository_id = repository_id(20 + mode as u8);
            fixture
                .source
                .bind(&root, &repository_id, mode, Duration::from_secs(30));
            fixture.store.0.lock().unwrap().insert(
                repository_id.clone(),
                RepositorySchedule {
                    next_scheduled_at: Some(fixture.clock.now() + Duration::from_secs(45)),
                    ..RepositorySchedule::default()
                },
            );

            fixture.scheduler.activate_root(&root).unwrap();
            tokio::time::advance(Duration::from_secs(44)).await;
            assert!(fixture.jobs.try_recv().is_err());
            tokio::time::advance(Duration::from_secs(1)).await;
            if should_poll {
                assert_eq!(fixture.receive().await.trigger, SyncTrigger::Interval);
            } else {
                assert!(fixture.jobs.try_recv().is_err());
            }
        }
    }

    #[tokio::test(start_paused = true)]
    async fn manual_completion_resets_failures_without_discarding_the_automatic_plan() {
        let fixture = Fixture::new();
        let root = PathBuf::from("/notes/manual-plan");
        let repository_id = repository_id(9);
        fixture.source.bind(
            &root,
            &repository_id,
            SyncMode::Automatic,
            Duration::from_secs(30),
        );
        let planned = fixture.clock.now() + Duration::from_secs(64 * 60);
        fixture.store.0.lock().unwrap().insert(
            repository_id.clone(),
            RepositorySchedule {
                automatic_failure_count: 8,
                next_scheduled_at: Some(planned),
                ..RepositorySchedule::default()
            },
        );
        fixture.scheduler.activate_root(&root).unwrap();

        fixture
            .scheduler
            .record_completion(completion(
                &root,
                &repository_id,
                SyncTrigger::Manual,
                Ok(RepositorySyncResult::default()),
            ))
            .unwrap();

        let schedule = fixture.store.load_schedule(&repository_id).unwrap();
        assert_eq!(schedule.automatic_failure_count, 0);
        assert_eq!(schedule.next_scheduled_at, Some(planned));
    }

    #[tokio::test(start_paused = true)]
    async fn failed_timer_enqueue_reschedules_after_five_minutes() {
        let source = Arc::new(MemorySource::default());
        let store = Arc::new(MemoryStore::default());
        let enqueuer = Arc::new(FailingEnqueuer::default());
        let clock = Arc::new(TestClock::new(1_800_000_000));
        let scheduler = DejavuScheduler::new(
            Arc::clone(&source),
            Arc::clone(&enqueuer),
            Arc::clone(&store),
            Arc::new(RecordingFlusher::default()),
            {
                let clock = Arc::clone(&clock);
                Arc::new(move || clock.now())
            },
        );
        let root = PathBuf::from("/notes/enqueue-failure");
        let repository_id = repository_id(10);
        source.bind(
            &root,
            &repository_id,
            SyncMode::Automatic,
            Duration::from_secs(30),
        );
        scheduler.activate_root(&root).unwrap();
        scheduler
            .record_file_change(&root, &root.join("note.md"))
            .unwrap();

        tokio::time::advance(Duration::from_secs(30)).await;
        for _ in 0..10 {
            tokio::task::yield_now().await;
            if enqueuer.0.load(Ordering::SeqCst) == 1 {
                break;
            }
        }

        assert_eq!(enqueuer.0.load(Ordering::SeqCst), 1);
        let schedule = store.load_schedule(&repository_id).unwrap();
        assert_eq!(schedule.automatic_failure_count, 1);
        assert_eq!(
            schedule.next_scheduled_at.unwrap() - clock.now(),
            time::Duration::minutes(5)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn stale_unbound_out_of_root_and_control_events_do_not_schedule() {
        let fixture = Fixture::new();
        let root = PathBuf::from("/notes/active");
        let repository_id = repository_id(7);
        fixture.source.bind(
            &root,
            &repository_id,
            SyncMode::Automatic,
            Duration::from_secs(30),
        );
        fixture.scheduler.activate_root(&root).unwrap();

        for (event_root, path) in [
            (
                Path::new("/notes/unbound"),
                Path::new("/notes/unbound/a.md"),
            ),
            (root.as_path(), Path::new("/notes/outside.md")),
            (
                root.as_path(),
                Path::new("/notes/active/.qingyu/state.json"),
            ),
        ] {
            assert!(!fixture
                .scheduler
                .record_file_change(event_root, path)
                .unwrap());
        }
        assert_eq!(
            fixture.store.load_schedule(&repository_id).unwrap(),
            RepositorySchedule::default()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn dns_retry_is_throttled_per_repository_for_five_minutes() {
        let fixture = Fixture::new();
        let repository_id = repository_id(8);
        let request = SyncJobRequest {
            notes_root: PathBuf::from("/notes/dns"),
            repository_id: repository_id.clone(),
            trigger: SyncTrigger::Interval,
        };

        assert!(fixture.scheduler.prepare_dns_retry(&request).unwrap());
        assert!(!fixture.scheduler.prepare_dns_retry(&request).unwrap());
        fixture.clock.advance(Duration::from_secs(5 * 60));
        assert!(fixture.scheduler.prepare_dns_retry(&request).unwrap());
        assert_eq!(fixture.flusher.0.load(Ordering::SeqCst), 2);
        assert!(fixture
            .store
            .load_schedule(&repository_id)
            .unwrap()
            .last_dns_retry_at
            .is_some());
    }
}
