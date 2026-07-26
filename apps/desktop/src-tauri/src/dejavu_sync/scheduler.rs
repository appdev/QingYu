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
const PERSISTENCE_RETRY_DELAY: Duration = Duration::from_secs(5 * 60);
const MINIMUM_NO_CHANGE_DELAY_MINUTES: u64 = 8;

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
type SchedulerTaskFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
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
    fn update_schedule(
        &self,
        repository_id: &str,
        update: &mut dyn FnMut(&mut RepositorySchedule) -> bool,
    ) -> Result<RepositorySchedule, RepositoryJobError>;
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
    due: Option<ScheduledDue>,
    due_claim: Option<DueClaim>,
    generation: u64,
}

#[derive(Clone, Copy, PartialEq)]
struct ScheduledDue {
    deadline: Instant,
    expected_persisted: Option<OffsetDateTime>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct DueClaim {
    generation: u64,
    expected_persisted: Option<OffsetDateTime>,
}

enum PauseDecision {
    NotPaused,
    BlockedUntil(OffsetDateTime),
    Eligible(Option<OffsetDateTime>),
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
        Self::new_with_task_spawner(source, enqueuer, store, dns_flusher, clock, |future| {
            tokio::spawn(future);
        })
    }

    pub(crate) fn new_for_tauri<Source, Enqueuer, Store, Flusher>(
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
        Self::new_with_task_spawner(source, enqueuer, store, dns_flusher, clock, |future| {
            tauri::async_runtime::spawn(future);
        })
    }

    fn new_with_task_spawner<Source, Enqueuer, Store, Flusher, Spawn>(
        source: Arc<Source>,
        enqueuer: Arc<Enqueuer>,
        store: Arc<Store>,
        dns_flusher: Arc<Flusher>,
        clock: Arc<SchedulerClock>,
        spawn: Spawn,
    ) -> Self
    where
        Source: RepositoryScheduleSource + 'static,
        Enqueuer: SchedulerJobEnqueuer + 'static,
        Store: RepositoryScheduleStore + 'static,
        Flusher: DnsFlusher + 'static,
        Spawn: FnOnce(SchedulerTaskFuture),
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
        spawn(Box::pin(run_timer(Arc::downgrade(&inner))));
        Self { inner }
    }

    pub(crate) fn activate_root(&self, root: &Path) -> Result<bool, RepositoryJobError> {
        let Some(active) = self.inner.source.resolve_active_root(root)? else {
            return Ok(false);
        };
        if active.notes_root != root || active.interval.is_zero() {
            return Err(RepositoryJobError::InvalidBinding);
        }
        let due = if active.mode == SyncMode::Automatic {
            match self.inner.store.load_schedule(&active.repository_id) {
                Ok(schedule) => schedule.next_scheduled_at.map(|next| ScheduledDue {
                    deadline: wall_time_to_deadline((self.inner.clock)(), next),
                    expected_persisted: Some(next),
                }),
                Err(_) => Some(ScheduledDue {
                    deadline: Instant::now() + PERSISTENCE_RETRY_DELAY,
                    expected_persisted: None,
                }),
            }
        } else {
            None
        };
        let mut state = self.inner.state.lock().unwrap();
        state.active = Some(active);
        state.due = due;
        state.due_claim = None;
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
        state.due_claim = None;
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
        let next_scheduled_at = (self.inner.clock)() + active.interval;
        let mut update = |schedule: &mut RepositorySchedule| {
            schedule.same_count = 0;
            schedule.next_scheduled_at = Some(next_scheduled_at);
            true
        };
        if let Err(error) = self
            .inner
            .store
            .update_schedule(&active.repository_id, &mut update)
        {
            self.install_memory_retry(&active.repository_id, None);
            return Err(error);
        }
        self.install_due_if_active(&active, active.interval, next_scheduled_at);
        Ok(true)
    }

    pub(crate) async fn trigger_startup(&self) -> Result<bool, RepositoryJobError> {
        self.trigger_now(SyncTrigger::AppLaunch).await
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) async fn trigger_exit(&self) -> Result<bool, RepositoryJobError> {
        // The public trigger schema remains unchanged until the S3 cutover.
        self.trigger_now(SyncTrigger::SettingsExit).await
    }

    pub(crate) async fn trigger_exit_job(
        &self,
    ) -> Result<Option<AcceptedSyncJob>, RepositoryJobError> {
        // The exit barrier needs the internal completion handle while the public
        // trigger schema remains unchanged until the S3 cutover.
        self.trigger_job(SyncTrigger::SettingsExit).await
    }

    #[allow(dead_code)]
    pub(crate) async fn trigger_manual(&self) -> Result<bool, RepositoryJobError> {
        self.trigger_now(SyncTrigger::Manual).await
    }

    async fn trigger_now(&self, trigger: SyncTrigger) -> Result<bool, RepositoryJobError> {
        Ok(self.trigger_job(trigger).await?.is_some())
    }

    async fn trigger_job(
        &self,
        trigger: SyncTrigger,
    ) -> Result<Option<AcceptedSyncJob>, RepositoryJobError> {
        let Some(active) = self.active_snapshot() else {
            return Ok(None);
        };
        if !trigger_allowed(active.mode, trigger) {
            return Ok(None);
        }
        let now = (self.inner.clock)();
        let mut pause = PauseDecision::NotPaused;
        let mut update = |schedule: &mut RepositorySchedule| {
            if trigger == SyncTrigger::Manual || schedule.automatic_failure_count < 8 {
                return false;
            }
            pause = match schedule.next_scheduled_at {
                Some(deadline) if deadline > now => PauseDecision::BlockedUntil(deadline),
                deadline => PauseDecision::Eligible(deadline),
            };
            false
        };
        if let Err(error) = self
            .inner
            .store
            .update_schedule(&active.repository_id, &mut update)
        {
            self.install_memory_retry(&active.repository_id, None);
            return Err(error);
        }
        if let PauseDecision::BlockedUntil(deadline) = pause {
            if active.mode == SyncMode::Automatic {
                self.install_due_if_active(&active, wall_time_delay(now, deadline), deadline);
            }
            return Ok(None);
        }
        let claim = match (pause, trigger) {
            (PauseDecision::Eligible(expected_persisted), SyncTrigger::AppLaunch) => {
                let Some(claim) = self.claim_due_for_trigger(&active, expected_persisted) else {
                    return Ok(None);
                };
                Some(claim)
            }
            (PauseDecision::NotPaused, SyncTrigger::AppLaunch) => {
                match self.claim_expired_automatic_due_for_startup(&active) {
                    Ok(claim) => claim,
                    Err(()) => return Ok(None),
                }
            }
            (PauseDecision::Eligible(_), _)
            | (PauseDecision::NotPaused, _)
            | (PauseDecision::BlockedUntil(_), _) => None,
        };
        match self.enqueue_active(active.clone(), trigger).await {
            Ok(accepted) => {
                if let Some(claim) = claim {
                    let _consumed = self.consume_due_after_acceptance(&active, claim);
                }
                Ok(Some(accepted))
            }
            Err(error) => Err(error),
        }
    }

    async fn enqueue_active(
        &self,
        active: ActiveRepositorySchedule,
        trigger: SyncTrigger,
    ) -> Result<AcceptedSyncJob, RepositoryJobError> {
        let request = SyncJobRequest {
            notes_root: active.notes_root,
            repository_id: active.repository_id,
            trigger,
        };
        match self.inner.enqueuer.enqueue(request.clone()).await {
            Ok(accepted) => Ok(accepted),
            Err(error) => {
                let recorded = self.record_completion(RepositoryJobCompletion {
                    request,
                    result: Err(error),
                });
                match recorded {
                    Ok(()) => Err(error),
                    Err(persistence_error) => Err(persistence_error),
                }
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

    pub(crate) fn record_bind_completion(
        &self,
        notes_root: &Path,
        repository_id: &str,
        result: Result<(), RepositoryJobError>,
    ) -> Result<bool, RepositoryJobError> {
        let Some(active) = self.active_snapshot().filter(|active| {
            active.mode == SyncMode::Automatic
                && active.repository_id == repository_id
                && active.notes_root == notes_root
        }) else {
            return Ok(false);
        };
        if result == Err(RepositoryJobError::Cancelled) {
            return Ok(false);
        }
        let now = (self.inner.clock)();
        let mut delay = active.interval;
        let mut next_scheduled_at = now + delay;
        let mut update = |schedule: &mut RepositorySchedule| {
            match result {
                Ok(()) => {
                    schedule.automatic_failure_count = 0;
                    schedule.same_count = 0;
                }
                Err(_) => {
                    schedule.automatic_failure_count =
                        schedule.automatic_failure_count.saturating_add(1);
                    delay = if schedule.automatic_failure_count >= 8 {
                        FAILURE_PAUSE_DELAY
                    } else {
                        ORDINARY_FAILURE_DELAY
                    };
                    next_scheduled_at = now + delay;
                }
            }
            schedule.next_scheduled_at = Some(next_scheduled_at);
            true
        };
        if let Err(error) = self.inner.store.update_schedule(repository_id, &mut update) {
            self.install_memory_retry(repository_id, None);
            return Err(error);
        }
        self.install_due_if_active(&active, delay, next_scheduled_at);
        Ok(true)
    }

    pub(crate) fn record_completion(
        &self,
        completion: RepositoryJobCompletion,
    ) -> Result<(), RepositoryJobError> {
        let repository_id = &completion.request.repository_id;
        let active = self
            .active_snapshot()
            .filter(|active| active.repository_id == *repository_id);
        let now = (self.inner.clock)();
        let mut next_due = None;
        let mut update = |schedule: &mut RepositorySchedule| {
            match &completion.result {
                Ok(result) => {
                    schedule.automatic_failure_count = 0;
                    if completion.request.trigger == SyncTrigger::Manual {
                        return true;
                    }
                    let should_poll = active
                        .as_ref()
                        .is_some_and(|active| active.mode == SyncMode::Automatic);
                    if !should_poll {
                        schedule.next_scheduled_at = None;
                        return true;
                    }
                    let active = active
                        .as_ref()
                        .expect("active automatic schedule was checked");
                    let delay = if result.data_changed {
                        active.interval
                    } else {
                        next_no_change_delay(&mut schedule.same_count)
                    };
                    let next_scheduled_at = now + delay;
                    schedule.next_scheduled_at = Some(next_scheduled_at);
                    next_due = Some((delay, next_scheduled_at));
                }
                Err(error) => {
                    if completion.request.trigger != SyncTrigger::Manual
                        && *error != RepositoryJobError::Cancelled
                    {
                        schedule.automatic_failure_count =
                            schedule.automatic_failure_count.saturating_add(1);
                    }
                    if completion.request.trigger == SyncTrigger::Manual {
                        return true;
                    }
                    let active_mode = active.as_ref().map(|active| active.mode);
                    let should_retry = *error != RepositoryJobError::Cancelled
                        && (active_mode == Some(SyncMode::Automatic)
                            || (schedule.automatic_failure_count >= 8
                                && active_mode == Some(SyncMode::StartupExit)));
                    if !should_retry {
                        schedule.next_scheduled_at = None;
                        return true;
                    }
                    let delay = if schedule.automatic_failure_count >= 8 {
                        FAILURE_PAUSE_DELAY
                    } else {
                        ORDINARY_FAILURE_DELAY
                    };
                    let next_scheduled_at = now + delay;
                    schedule.next_scheduled_at = Some(next_scheduled_at);
                    next_due = Some((delay, next_scheduled_at));
                }
            }
            true
        };
        if let Err(error) = self.inner.store.update_schedule(repository_id, &mut update) {
            self.install_memory_retry(repository_id, None);
            return Err(error);
        }
        if let (Some(active), Some((delay, next_scheduled_at))) = (active, next_due) {
            if active.mode == SyncMode::Automatic {
                self.install_due_if_active(&active, delay, next_scheduled_at);
            } else {
                self.clear_due_if_active(repository_id);
            }
        } else {
            self.clear_due_if_active(repository_id);
        }
        Ok(())
    }

    fn install_due_if_active(
        &self,
        active: &ActiveRepositorySchedule,
        delay: Duration,
        next_scheduled_at: OffsetDateTime,
    ) {
        let mut state = self.inner.state.lock().unwrap();
        if state
            .active
            .as_ref()
            .is_some_and(|current| current.repository_id == active.repository_id)
        {
            state.due = Some(ScheduledDue {
                deadline: Instant::now() + delay,
                expected_persisted: Some(next_scheduled_at),
            });
            state.due_claim = None;
            state.generation = state.generation.wrapping_add(1);
        }
        drop(state);
        self.inner.changed.notify_one();
    }

    fn install_memory_retry(
        &self,
        repository_id: &str,
        expected_persisted: Option<OffsetDateTime>,
    ) {
        let mut state = self.inner.state.lock().unwrap();
        if state.active.as_ref().is_some_and(|active| {
            active.repository_id == repository_id && active.mode == SyncMode::Automatic
        }) {
            state.due = Some(ScheduledDue {
                deadline: Instant::now() + PERSISTENCE_RETRY_DELAY,
                expected_persisted,
            });
            state.due_claim = None;
            state.generation = state.generation.wrapping_add(1);
        }
        drop(state);
        self.inner.changed.notify_one();
    }

    fn consume_due_after_acceptance(
        &self,
        active: &ActiveRepositorySchedule,
        claim: DueClaim,
    ) -> Result<(), RepositoryJobError> {
        let Some(expected_persisted) = claim.expected_persisted else {
            self.park_due_claim(active, claim);
            return Ok(());
        };
        let mut consumed = false;
        let mut consume = |schedule: &mut RepositorySchedule| {
            if schedule.next_scheduled_at != Some(expected_persisted) {
                return false;
            }
            schedule.next_scheduled_at = None;
            consumed = true;
            true
        };
        let schedule = match self
            .inner
            .store
            .update_schedule(&active.repository_id, &mut consume)
        {
            Ok(schedule) => schedule,
            Err(error) => {
                self.park_due_claim(active, claim);
                return Err(error);
            }
        };
        if consumed {
            self.park_due_claim(active, claim);
        } else {
            let due = schedule.next_scheduled_at.map(|next| ScheduledDue {
                deadline: wall_time_to_deadline((self.inner.clock)(), next),
                expected_persisted: Some(next),
            });
            self.finish_due_claim(active, claim, due);
        }
        Ok(())
    }

    fn claim_due_for_trigger(
        &self,
        active: &ActiveRepositorySchedule,
        expected_persisted: Option<OffsetDateTime>,
    ) -> Option<DueClaim> {
        let mut state = self.inner.state.lock().unwrap();
        if state.due_claim.is_some()
            || state.active.as_ref().is_none_or(|current| {
                current.repository_id != active.repository_id
                    || current.notes_root != active.notes_root
            })
            || state.due.is_some_and(|due| {
                due.expected_persisted.is_some() && due.expected_persisted != expected_persisted
            })
        {
            return None;
        }
        let claim = DueClaim {
            generation: state.generation,
            expected_persisted,
        };
        state.due = None;
        state.due_claim = Some(claim);
        Some(claim)
    }

    fn claim_expired_automatic_due_for_startup(
        &self,
        active: &ActiveRepositorySchedule,
    ) -> Result<Option<DueClaim>, ()> {
        let mut state = self.inner.state.lock().unwrap();
        if state.active.as_ref().is_none_or(|current| {
            current.repository_id != active.repository_id
                || current.notes_root != active.notes_root
                || current.mode != SyncMode::Automatic
        }) {
            return Ok(None);
        }
        if state.due_claim.is_some() {
            return Err(());
        }
        let Some(due) = state.due.filter(|due| due.deadline <= Instant::now()) else {
            return Ok(None);
        };
        let claim = DueClaim {
            generation: state.generation,
            expected_persisted: due.expected_persisted,
        };
        state.due = None;
        state.due_claim = Some(claim);
        Ok(Some(claim))
    }

    fn claim_timer_due(
        &self,
        active: &ActiveRepositorySchedule,
        generation: u64,
        due: ScheduledDue,
    ) -> Option<DueClaim> {
        let mut state = self.inner.state.lock().unwrap();
        if state.generation != generation
            || state.due != Some(due)
            || state.due_claim.is_some()
            || state.active.as_ref().is_none_or(|current| {
                current.repository_id != active.repository_id
                    || current.notes_root != active.notes_root
                    || current.mode != SyncMode::Automatic
            })
        {
            return None;
        }
        let claim = DueClaim {
            generation,
            expected_persisted: due.expected_persisted,
        };
        state.due = None;
        state.due_claim = Some(claim);
        Some(claim)
    }

    fn finish_due_claim(
        &self,
        active: &ActiveRepositorySchedule,
        claim: DueClaim,
        due: Option<ScheduledDue>,
    ) {
        let mut state = self.inner.state.lock().unwrap();
        if state.generation == claim.generation
            && state.due_claim == Some(claim)
            && state.active.as_ref().is_some_and(|current| {
                current.repository_id == active.repository_id
                    && current.notes_root == active.notes_root
            })
        {
            state.due = due;
            state.due_claim = None;
            state.generation = state.generation.wrapping_add(1);
        }
        drop(state);
        self.inner.changed.notify_one();
    }

    fn park_due_claim(&self, active: &ActiveRepositorySchedule, claim: DueClaim) {
        let mut state = self.inner.state.lock().unwrap();
        if state.generation == claim.generation
            && state.due_claim == Some(claim)
            && state.active.as_ref().is_some_and(|current| {
                current.repository_id == active.repository_id
                    && current.notes_root == active.notes_root
            })
        {
            state.due = None;
        }
        drop(state);
        self.inner.changed.notify_one();
    }

    fn clear_due_if_active(&self, repository_id: &str) {
        let mut state = self.inner.state.lock().unwrap();
        if state
            .active
            .as_ref()
            .is_some_and(|active| active.repository_id == repository_id)
        {
            state.due = None;
            state.due_claim = None;
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

    fn update_schedule(
        &self,
        repository_id: &str,
        update: &mut dyn FnMut(&mut RepositorySchedule) -> bool,
    ) -> Result<RepositorySchedule, RepositoryJobError> {
        RepositoryStatusStore::update_schedule(self, repository_id, update)
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
            () = tokio::time::sleep_until(due.deadline) => {
                let Some(inner) = weak_inner.upgrade() else {
                    return;
                };
                let active = {
                    let state = inner.state.lock().unwrap();
                    if state.generation != generation
                        || state
                            .due
                            .is_none_or(|current| current.deadline > Instant::now())
                    {
                        None
                    } else {
                        state.active.clone()
                    }
                };
                if let Some(active) = active {
                    let still_active = {
                        let state = inner.state.lock().unwrap();
                        state.generation == generation
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
                    if let Some(claim) = scheduler.claim_timer_due(&active, generation, due) {
                        if scheduler
                            .enqueue_active(active.clone(), SyncTrigger::Interval)
                            .await
                            .is_ok()
                        {
                            let active = scheduler
                                .active_snapshot()
                                .filter(|current| current.repository_id == active.repository_id);
                            if let Some(active) = active {
                                let _consumed =
                                    scheduler.consume_due_after_acceptance(&active, claim);
                            }
                        }
                    }
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

fn wall_time_delay(now: OffsetDateTime, due: OffsetDateTime) -> Duration {
    let seconds = (due - now).whole_seconds();
    Duration::from_secs(seconds.max(0) as u64)
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
            .map_err(RepositoryJobError::from)?
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
    use std::sync::mpsc as std_mpsc;
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

    #[test]
    fn tauri_scheduler_spawner_does_not_require_an_entered_tokio_runtime() {
        std::thread::spawn(|| {
            let (sent, _received) = mpsc::unbounded_channel();
            let scheduler = DejavuScheduler::new_for_tauri(
                Arc::new(MemorySource::default()),
                Arc::new(RecordingEnqueuer { sent }),
                Arc::new(MemoryStore::default()),
                Arc::new(RecordingFlusher::default()),
                Arc::new(|| OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap()),
            );
            drop(scheduler);
        })
        .join()
        .expect("the production scheduler spawner must work outside Tokio context");
    }

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

        fn update_schedule(
            &self,
            repository_id: &str,
            update: &mut dyn FnMut(&mut RepositorySchedule) -> bool,
        ) -> Result<RepositorySchedule, RepositoryJobError> {
            let mut schedules = self.0.lock().unwrap();
            let schedule = schedules.entry(repository_id.to_owned()).or_default();
            let changed = update(schedule);
            let updated = schedule.clone();
            if !changed {
                return Ok(updated);
            }
            Ok(updated)
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

    #[derive(Default)]
    struct StaleSnapshotStore {
        schedules: Mutex<HashMap<String, RepositorySchedule>>,
        next_update_gate: Mutex<Option<(std_mpsc::Sender<()>, std_mpsc::Receiver<()>)>>,
    }

    impl StaleSnapshotStore {
        fn gate_next_update(&self) -> (std_mpsc::Receiver<()>, std_mpsc::Sender<()>) {
            let (entered_tx, entered_rx) = std_mpsc::channel();
            let (release_tx, release_rx) = std_mpsc::channel();
            *self.next_update_gate.lock().unwrap() = Some((entered_tx, release_rx));
            (entered_rx, release_tx)
        }
    }

    impl RepositoryScheduleStore for StaleSnapshotStore {
        fn load_schedule(
            &self,
            repository_id: &str,
        ) -> Result<RepositorySchedule, RepositoryJobError> {
            Ok(self
                .schedules
                .lock()
                .unwrap()
                .get(repository_id)
                .cloned()
                .unwrap_or_default())
        }

        fn update_schedule(
            &self,
            repository_id: &str,
            update: &mut dyn FnMut(&mut RepositorySchedule) -> bool,
        ) -> Result<RepositorySchedule, RepositoryJobError> {
            let mut schedules = self.schedules.lock().unwrap();
            if let Some((entered, release)) = self.next_update_gate.lock().unwrap().take() {
                entered.send(()).unwrap();
                release.recv().unwrap();
            }
            let schedule = schedules.entry(repository_id.to_owned()).or_default();
            update(schedule);
            Ok(schedule.clone())
        }

        fn reserve_dns_retry(
            &self,
            repository_id: &str,
            now: OffsetDateTime,
            throttle: Duration,
        ) -> Result<bool, RepositoryJobError> {
            let mut schedules = self.schedules.lock().unwrap();
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
                Ok(AcceptedSyncJob::completed_for_test(
                    "00000000-0000-4000-8000-000000000099",
                    request.repository_id,
                    request.notes_root,
                ))
            })
        }
    }

    struct BlockingAcceptanceEnqueuer {
        sent: mpsc::UnboundedSender<SyncJobRequest>,
        release: Arc<tokio::sync::Semaphore>,
    }

    impl SchedulerJobEnqueuer for BlockingAcceptanceEnqueuer {
        fn enqueue<'a>(
            &'a self,
            request: SyncJobRequest,
        ) -> BoxFuture<'a, Result<AcceptedSyncJob, RepositoryJobError>> {
            Box::pin(async move {
                self.sent.send(request.clone()).unwrap();
                self.release.acquire().await.unwrap().forget();
                Ok(AcceptedSyncJob::completed_for_test(
                    "00000000-0000-4000-8000-000000000098",
                    request.repository_id,
                    request.notes_root,
                ))
            })
        }
    }

    #[derive(Default)]
    struct FailingMemoryStore {
        inner: MemoryStore,
        fail_updates: AtomicUsize,
        update_attempts: AtomicUsize,
        changed: tokio::sync::Notify,
    }

    impl FailingMemoryStore {
        fn fail_next_update(&self) {
            self.fail_updates.fetch_add(1, Ordering::SeqCst);
        }

        async fn wait_for_update_attempts(&self, expected: usize) {
            loop {
                let changed = self.changed.notified();
                if self.update_attempts.load(Ordering::SeqCst) >= expected {
                    return;
                }
                changed.await;
            }
        }
    }

    impl RepositoryScheduleStore for FailingMemoryStore {
        fn load_schedule(
            &self,
            repository_id: &str,
        ) -> Result<RepositorySchedule, RepositoryJobError> {
            self.inner.load_schedule(repository_id)
        }

        fn update_schedule(
            &self,
            repository_id: &str,
            update: &mut dyn FnMut(&mut RepositorySchedule) -> bool,
        ) -> Result<RepositorySchedule, RepositoryJobError> {
            self.update_attempts.fetch_add(1, Ordering::SeqCst);
            self.changed.notify_waiters();
            if self
                .fail_updates
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                    if remaining > 0 {
                        Some(remaining - 1)
                    } else {
                        None
                    }
                })
                .is_ok()
            {
                return Err(RepositoryJobError::StatusUnavailable);
            }
            self.inner.update_schedule(repository_id, update)
        }

        fn reserve_dns_retry(
            &self,
            repository_id: &str,
            now: OffsetDateTime,
            throttle: Duration,
        ) -> Result<bool, RepositoryJobError> {
            self.inner.reserve_dns_retry(repository_id, now, throttle)
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

    #[tokio::test]
    async fn file_change_rmw_cannot_erase_a_concurrent_dns_reservation() {
        let source = Arc::new(MemorySource::default());
        let store = Arc::new(StaleSnapshotStore::default());
        let flusher = Arc::new(RecordingFlusher::default());
        let clock = Arc::new(TestClock::new(1_800_000_000));
        let (sent, _jobs) = mpsc::unbounded_channel();
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
        let root = PathBuf::from("/notes/file-dns-race");
        let repository_id = repository_id(30);
        source.bind(
            &root,
            &repository_id,
            SyncMode::Automatic,
            Duration::from_secs(30),
        );
        scheduler.activate_root(&root).unwrap();
        let (loaded, release) = store.gate_next_update();
        let file_change = {
            let scheduler = scheduler.clone();
            let root = root.clone();
            std::thread::spawn(move || scheduler.record_file_change(&root, &root.join("note.md")))
        };
        loaded.recv().unwrap();

        let request = SyncJobRequest {
            notes_root: root,
            repository_id: repository_id.clone(),
            trigger: SyncTrigger::Interval,
        };
        let (dns_started, dns_entered) = std_mpsc::channel();
        let dns_retry = {
            let scheduler = scheduler.clone();
            std::thread::spawn(move || {
                dns_started.send(()).unwrap();
                scheduler.prepare_dns_retry(&request)
            })
        };
        dns_entered.recv().unwrap();
        release.send(()).unwrap();
        assert!(file_change.join().unwrap().unwrap());
        assert!(dns_retry.join().unwrap().unwrap());

        assert_eq!(
            store
                .load_schedule(&repository_id)
                .unwrap()
                .last_dns_retry_at,
            Some(clock.now())
        );
    }

    #[tokio::test]
    async fn completion_rmw_cannot_erase_a_concurrent_dns_reservation() {
        let source = Arc::new(MemorySource::default());
        let store = Arc::new(StaleSnapshotStore::default());
        let clock = Arc::new(TestClock::new(1_800_000_000));
        let (sent, _jobs) = mpsc::unbounded_channel();
        let scheduler = DejavuScheduler::new(
            Arc::clone(&source),
            Arc::new(RecordingEnqueuer { sent }),
            Arc::clone(&store),
            Arc::new(RecordingFlusher::default()),
            {
                let clock = Arc::clone(&clock);
                Arc::new(move || clock.now())
            },
        );
        let root = PathBuf::from("/notes/completion-dns-race");
        let repository_id = repository_id(31);
        source.bind(
            &root,
            &repository_id,
            SyncMode::Automatic,
            Duration::from_secs(30),
        );
        scheduler.activate_root(&root).unwrap();
        let (loaded, release) = store.gate_next_update();
        let completed = {
            let scheduler = scheduler.clone();
            let root = root.clone();
            let repository_id = repository_id.clone();
            std::thread::spawn(move || {
                scheduler.record_completion(completion(
                    &root,
                    &repository_id,
                    SyncTrigger::Interval,
                    Ok(RepositorySyncResult::default()),
                ))
            })
        };
        loaded.recv().unwrap();

        let request = SyncJobRequest {
            notes_root: root,
            repository_id: repository_id.clone(),
            trigger: SyncTrigger::Interval,
        };
        let (dns_started, dns_entered) = std_mpsc::channel();
        let dns_retry = {
            let scheduler = scheduler.clone();
            std::thread::spawn(move || {
                dns_started.send(()).unwrap();
                scheduler.prepare_dns_retry(&request)
            })
        };
        dns_entered.recv().unwrap();
        release.send(()).unwrap();
        completed.join().unwrap().unwrap();
        assert!(dns_retry.join().unwrap().unwrap());

        assert_eq!(
            store
                .load_schedule(&repository_id)
                .unwrap()
                .last_dns_retry_at,
            Some(clock.now())
        );
    }

    #[tokio::test]
    async fn timer_consumption_rmw_cannot_erase_a_concurrent_dns_reservation() {
        let source = Arc::new(MemorySource::default());
        let store = Arc::new(StaleSnapshotStore::default());
        let clock = Arc::new(TestClock::new(1_800_000_000));
        let (sent, _jobs) = mpsc::unbounded_channel();
        let scheduler = DejavuScheduler::new(
            Arc::clone(&source),
            Arc::new(RecordingEnqueuer { sent }),
            Arc::clone(&store),
            Arc::new(RecordingFlusher::default()),
            {
                let clock = Arc::clone(&clock);
                Arc::new(move || clock.now())
            },
        );
        let root = PathBuf::from("/notes/timer-dns-race");
        let repository_id = repository_id(32);
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
        let (active, generation, due) = {
            let state = scheduler.inner.state.lock().unwrap();
            (
                state.active.clone().unwrap(),
                state.generation,
                state.due.unwrap(),
            )
        };
        let claim = scheduler.claim_timer_due(&active, generation, due).unwrap();
        let (loaded, release) = store.gate_next_update();
        let timer_consumption = {
            let scheduler = scheduler.clone();
            std::thread::spawn(move || scheduler.consume_due_after_acceptance(&active, claim))
        };
        loaded.recv().unwrap();

        let request = SyncJobRequest {
            notes_root: root,
            repository_id: repository_id.clone(),
            trigger: SyncTrigger::Interval,
        };
        let (dns_started, dns_entered) = std_mpsc::channel();
        let dns_retry = {
            let scheduler = scheduler.clone();
            std::thread::spawn(move || {
                dns_started.send(()).unwrap();
                scheduler.prepare_dns_retry(&request)
            })
        };
        dns_entered.recv().unwrap();
        release.send(()).unwrap();
        timer_consumption.join().unwrap().unwrap();
        assert!(dns_retry.join().unwrap().unwrap());

        let schedule = store.load_schedule(&repository_id).unwrap();
        assert_eq!(schedule.last_dns_retry_at, Some(clock.now()));
        assert_eq!(schedule.next_scheduled_at, None);
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
    async fn eighth_failure_persists_pause_deadline_for_automatic_and_startup_exit() {
        for (mode, timer_expected) in [(SyncMode::Automatic, true), (SyncMode::StartupExit, false)]
        {
            let fixture = Fixture::new();
            let root = PathBuf::from(format!("/notes/eighth-failure-{mode:?}"));
            let repository_id = repository_id(40 + mode as u8);
            fixture
                .source
                .bind(&root, &repository_id, mode, Duration::from_secs(30));
            fixture.store.0.lock().unwrap().insert(
                repository_id.clone(),
                RepositorySchedule {
                    automatic_failure_count: 7,
                    ..RepositorySchedule::default()
                },
            );
            fixture.scheduler.activate_root(&root).unwrap();

            fixture
                .scheduler
                .record_completion(completion(
                    &root,
                    &repository_id,
                    SyncTrigger::SettingsExit,
                    Err(RepositoryJobError::CloudUnavailable),
                ))
                .unwrap();

            let schedule = fixture.store.load_schedule(&repository_id).unwrap();
            assert_eq!(schedule.automatic_failure_count, 8);
            assert_eq!(
                schedule.next_scheduled_at,
                Some(fixture.clock.now() + Duration::from_secs(64 * 60))
            );
            assert_eq!(
                fixture.scheduler.inner.state.lock().unwrap().due.is_some(),
                timer_expected
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn persisted_pause_deadline_survives_restart_and_allows_one_attempt_at_expiry() {
        for mode in [SyncMode::Automatic, SyncMode::StartupExit] {
            let mut fixture = Fixture::new();
            let root = PathBuf::from(format!("/notes/pause-deadline-{mode:?}"));
            let repository_id = repository_id(50 + mode as u8);
            fixture
                .source
                .bind(&root, &repository_id, mode, Duration::from_secs(30));
            let pause_until = fixture.clock.now() + Duration::from_secs(64 * 60);
            fixture.store.0.lock().unwrap().insert(
                repository_id.clone(),
                RepositorySchedule {
                    automatic_failure_count: 8,
                    next_scheduled_at: Some(pause_until),
                    ..RepositorySchedule::default()
                },
            );
            fixture.scheduler.activate_root(&root).unwrap();
            fixture.clock.advance(Duration::from_secs(60));

            assert!(!fixture.scheduler.trigger_startup().await.unwrap());
            assert_eq!(
                fixture
                    .store
                    .load_schedule(&repository_id)
                    .unwrap()
                    .next_scheduled_at,
                Some(pause_until)
            );
            assert!(fixture.scheduler.deactivate_root(&root));
            assert!(fixture.scheduler.activate_root(&root).unwrap());
            assert!(!fixture.scheduler.trigger_exit().await.unwrap());
            assert_eq!(
                fixture
                    .store
                    .load_schedule(&repository_id)
                    .unwrap()
                    .next_scheduled_at,
                Some(pause_until)
            );

            fixture.clock.advance(Duration::from_secs(63 * 60));
            assert!(fixture.scheduler.trigger_startup().await.unwrap());
            assert_eq!(fixture.receive().await.trigger, SyncTrigger::AppLaunch);
            fixture.clock.advance(Duration::from_secs(1));
            assert!(!fixture.scheduler.trigger_startup().await.unwrap());
            assert!(fixture.jobs.try_recv().is_err());
        }
    }

    #[tokio::test(start_paused = true)]
    async fn automatic_expired_due_and_pending_startup_enqueue_exactly_one_job() {
        let mut fixture = Fixture::new();
        let root = PathBuf::from("/notes/expired-due-startup");
        let repository_id = repository_id(60);
        fixture.source.bind(
            &root,
            &repository_id,
            SyncMode::Automatic,
            Duration::from_secs(30),
        );
        fixture.store.0.lock().unwrap().insert(
            repository_id.clone(),
            RepositorySchedule {
                automatic_failure_count: 8,
                next_scheduled_at: Some(fixture.clock.now()),
                ..RepositorySchedule::default()
            },
        );
        fixture.scheduler.activate_root(&root).unwrap();

        assert!(fixture.scheduler.trigger_startup().await.unwrap());
        assert_eq!(fixture.receive().await.trigger, SyncTrigger::AppLaunch);
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        assert!(fixture.jobs.try_recv().is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn automatic_normal_expired_due_and_pending_startup_enqueue_exactly_one_job() {
        let mut fixture = Fixture::new();
        let root = PathBuf::from("/notes/normal-expired-due-startup");
        let repository_id = repository_id(61);
        fixture.source.bind(
            &root,
            &repository_id,
            SyncMode::Automatic,
            Duration::from_secs(30),
        );
        fixture.store.0.lock().unwrap().insert(
            repository_id.clone(),
            RepositorySchedule {
                next_scheduled_at: Some(fixture.clock.now()),
                ..RepositorySchedule::default()
            },
        );
        fixture.scheduler.activate_root(&root).unwrap();

        assert!(fixture.scheduler.trigger_startup().await.unwrap());
        assert_eq!(fixture.receive().await.trigger, SyncTrigger::AppLaunch);
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        assert!(fixture.jobs.try_recv().is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn settings_exit_enqueues_its_barrier_job_while_an_interval_due_is_claimed() {
        let mut fixture = Fixture::new();
        let root = PathBuf::from("/notes/claimed-interval-exit");
        let repository_id = repository_id(62);
        fixture.source.bind(
            &root,
            &repository_id,
            SyncMode::Automatic,
            Duration::from_secs(30),
        );
        fixture.store.0.lock().unwrap().insert(
            repository_id.clone(),
            RepositorySchedule {
                next_scheduled_at: Some(fixture.clock.now()),
                ..RepositorySchedule::default()
            },
        );
        fixture.scheduler.activate_root(&root).unwrap();
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }

        assert_eq!(fixture.receive().await.trigger, SyncTrigger::Interval);
        assert!(fixture
            .scheduler
            .inner
            .state
            .lock()
            .unwrap()
            .due_claim
            .is_some());
        assert!(fixture.scheduler.trigger_exit().await.unwrap());
        assert_eq!(fixture.receive().await.trigger, SyncTrigger::SettingsExit);
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
    async fn successful_automatic_bind_completion_polls_at_interval_without_file_change() {
        let mut fixture = Fixture::new();
        let root = PathBuf::from("/notes/new-automatic-bind");
        let repository_id = repository_id(41);
        fixture.source.bind(
            &root,
            &repository_id,
            SyncMode::Automatic,
            Duration::from_secs(30),
        );
        fixture.scheduler.activate_root(&root).unwrap();

        assert!(fixture
            .scheduler
            .record_bind_completion(&root, &repository_id, Ok(()))
            .unwrap());
        assert!(fixture.jobs.try_recv().is_err());
        let schedule = fixture.store.load_schedule(&repository_id).unwrap();
        assert_eq!(schedule.automatic_failure_count, 0);
        assert_eq!(
            schedule.next_scheduled_at,
            Some(fixture.clock.now() + Duration::from_secs(30))
        );

        fixture.clock.advance(Duration::from_secs(30));
        tokio::time::advance(Duration::from_secs(30)).await;
        assert_eq!(fixture.receive().await.trigger, SyncTrigger::Interval);
    }

    #[tokio::test(start_paused = true)]
    async fn failed_automatic_bind_completion_counts_failure_and_retries_after_five_minutes() {
        let mut fixture = Fixture::new();
        let root = PathBuf::from("/notes/failed-automatic-bind");
        let repository_id = repository_id(42);
        fixture.source.bind(
            &root,
            &repository_id,
            SyncMode::Automatic,
            Duration::from_secs(30),
        );
        fixture.scheduler.activate_root(&root).unwrap();

        assert!(fixture
            .scheduler
            .record_bind_completion(
                &root,
                &repository_id,
                Err(RepositoryJobError::CloudUnavailable),
            )
            .unwrap());
        let schedule = fixture.store.load_schedule(&repository_id).unwrap();
        assert_eq!(schedule.automatic_failure_count, 1);
        assert_eq!(
            schedule.next_scheduled_at,
            Some(fixture.clock.now() + Duration::from_secs(5 * 60))
        );

        fixture.clock.advance(Duration::from_secs(5 * 60));
        tokio::time::advance(Duration::from_secs(5 * 60)).await;
        assert_eq!(fixture.receive().await.trigger, SyncTrigger::Interval);
    }

    #[tokio::test]
    async fn bind_completion_does_not_schedule_startup_exit_or_stale_roots() {
        let fixture = Fixture::new();
        let startup_exit_root = PathBuf::from("/notes/startup-exit-bind");
        let startup_exit_repository = repository_id(43);
        fixture.source.bind(
            &startup_exit_root,
            &startup_exit_repository,
            SyncMode::StartupExit,
            Duration::from_secs(30),
        );
        fixture.scheduler.activate_root(&startup_exit_root).unwrap();
        assert!(!fixture
            .scheduler
            .record_bind_completion(&startup_exit_root, &startup_exit_repository, Ok(()))
            .unwrap());
        assert_eq!(
            fixture
                .store
                .load_schedule(&startup_exit_repository)
                .unwrap()
                .next_scheduled_at,
            None
        );

        let closed_root = PathBuf::from("/notes/closed-automatic-bind");
        let closed_repository = repository_id(44);
        fixture.source.bind(
            &closed_root,
            &closed_repository,
            SyncMode::Automatic,
            Duration::from_secs(30),
        );
        fixture.scheduler.activate_root(&closed_root).unwrap();
        assert!(fixture.scheduler.deactivate_root(&closed_root));
        assert!(!fixture
            .scheduler
            .record_bind_completion(&closed_root, &closed_repository, Ok(()))
            .unwrap());

        let current_root = PathBuf::from("/notes/current-automatic-bind");
        let current_repository = repository_id(45);
        fixture.source.bind(
            &current_root,
            &current_repository,
            SyncMode::Automatic,
            Duration::from_secs(30),
        );
        fixture.scheduler.activate_root(&current_root).unwrap();
        assert!(!fixture
            .scheduler
            .record_bind_completion(&closed_root, &closed_repository, Ok(()))
            .unwrap());
        assert_eq!(
            fixture
                .store
                .load_schedule(&closed_repository)
                .unwrap()
                .next_scheduled_at,
            None
        );
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
    async fn timer_keeps_the_persisted_due_until_enqueue_is_accepted() {
        let source = Arc::new(MemorySource::default());
        let store = Arc::new(MemoryStore::default());
        let clock = Arc::new(TestClock::new(1_800_000_000));
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let (sent, mut jobs) = mpsc::unbounded_channel();
        let scheduler = DejavuScheduler::new(
            Arc::clone(&source),
            Arc::new(BlockingAcceptanceEnqueuer {
                sent,
                release: Arc::clone(&release),
            }),
            Arc::clone(&store),
            Arc::new(RecordingFlusher::default()),
            {
                let clock = Arc::clone(&clock);
                Arc::new(move || clock.now())
            },
        );
        let root = PathBuf::from("/notes/acceptance-boundary");
        let repository_id = repository_id(32);
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
        let original_due = store
            .load_schedule(&repository_id)
            .unwrap()
            .next_scheduled_at;

        tokio::time::advance(Duration::from_secs(30)).await;
        clock.advance(Duration::from_secs(30));
        assert_eq!(jobs.recv().await.unwrap().trigger, SyncTrigger::Interval);

        assert_eq!(
            store
                .load_schedule(&repository_id)
                .unwrap()
                .next_scheduled_at,
            original_due
        );
        scheduler
            .record_file_change(&root, &root.join("newer.md"))
            .unwrap();
        let replacement_due = store
            .load_schedule(&repository_id)
            .unwrap()
            .next_scheduled_at;
        release.add_permits(1);
        tokio::task::yield_now().await;

        assert_eq!(
            store
                .load_schedule(&repository_id)
                .unwrap()
                .next_scheduled_at,
            replacement_due
        );
    }

    #[tokio::test(start_paused = true)]
    async fn accepted_timer_due_clear_failure_parks_without_a_second_sync_or_busy_loop() {
        let source = Arc::new(MemorySource::default());
        let store = Arc::new(FailingMemoryStore::default());
        let clock = Arc::new(TestClock::new(1_800_000_000));
        let (sent, mut jobs) = mpsc::unbounded_channel();
        let scheduler = DejavuScheduler::new(
            Arc::clone(&source),
            Arc::new(RecordingEnqueuer { sent }),
            Arc::clone(&store),
            Arc::new(RecordingFlusher::default()),
            {
                let clock = Arc::clone(&clock);
                Arc::new(move || clock.now())
            },
        );
        let root = PathBuf::from("/notes/timer-persist-failure");
        let repository_id = repository_id(33);
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
        let original_due = store
            .load_schedule(&repository_id)
            .unwrap()
            .next_scheduled_at;
        let before_timer = store.update_attempts.load(Ordering::SeqCst);
        store.fail_next_update();

        tokio::time::advance(Duration::from_secs(30)).await;
        store.wait_for_update_attempts(before_timer + 1).await;
        assert_eq!(jobs.try_recv().unwrap().trigger, SyncTrigger::Interval);
        tokio::time::advance(Duration::from_secs(5 * 60)).await;
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        assert!(jobs.try_recv().is_err());
        tokio::time::advance(Duration::from_secs(60 * 60)).await;
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        assert!(jobs.try_recv().is_err());
        assert_eq!(
            store.update_attempts.load(Ordering::SeqCst),
            before_timer + 1
        );
        assert_eq!(
            store
                .load_schedule(&repository_id)
                .unwrap()
                .next_scheduled_at,
            original_due
        );
    }

    #[tokio::test(start_paused = true)]
    async fn completion_persistence_failure_reinstalls_an_in_memory_retry() {
        let source = Arc::new(MemorySource::default());
        let store = Arc::new(FailingMemoryStore::default());
        let clock = Arc::new(TestClock::new(1_800_000_000));
        let (sent, mut jobs) = mpsc::unbounded_channel();
        let scheduler = DejavuScheduler::new(
            Arc::clone(&source),
            Arc::new(RecordingEnqueuer { sent }),
            Arc::clone(&store),
            Arc::new(RecordingFlusher::default()),
            {
                let clock = Arc::clone(&clock);
                Arc::new(move || clock.now())
            },
        );
        let root = PathBuf::from("/notes/completion-persist-failure");
        let repository_id = repository_id(34);
        source.bind(
            &root,
            &repository_id,
            SyncMode::Automatic,
            Duration::from_secs(30),
        );
        scheduler.activate_root(&root).unwrap();
        store.fail_next_update();

        assert_eq!(
            scheduler.record_completion(completion(
                &root,
                &repository_id,
                SyncTrigger::Interval,
                Ok(RepositorySyncResult::default()),
            )),
            Err(RepositoryJobError::StatusUnavailable)
        );
        tokio::time::advance(Duration::from_secs(5 * 60)).await;
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }

        assert_eq!(jobs.try_recv().unwrap().trigger, SyncTrigger::Interval);
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
