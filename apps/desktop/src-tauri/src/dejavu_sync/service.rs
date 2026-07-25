use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use serde::Serialize;
use tokio::sync::{watch, Mutex as AsyncMutex, OwnedRwLockReadGuard, RwLock};

use super::status::{
    RepositoryConflictRecord, RepositorySafeError, RepositorySyncStatus, RepositoryTransferSummary,
};
use crate::sync_config::status::{sync_status_timestamp, SyncTrigger};

const MAX_WORKING_TREE_ATTEMPTS: u8 = 3;
const MAX_FINALIZATION_ATTEMPTS: u8 = 3;

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Clone)]
pub(crate) struct SyncJobRequest {
    pub(crate) notes_root: PathBuf,
    pub(crate) repository_id: String,
    pub(crate) trigger: SyncTrigger,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AcceptedSyncJob {
    pub(crate) job_id: String,
    pub(crate) repository_id: String,
    pub(crate) notes_root: PathBuf,
    #[serde(skip)]
    completion: watch::Receiver<Option<Result<(), RepositoryJobError>>>,
}

impl AcceptedSyncJob {
    fn new(
        job_id: String,
        repository_id: String,
        notes_root: PathBuf,
        completion: watch::Receiver<Option<Result<(), RepositoryJobError>>>,
    ) -> Self {
        Self {
            job_id,
            repository_id,
            notes_root,
            completion,
        }
    }

    pub(crate) async fn wait_for_completion(&self) -> Result<(), RepositoryJobError> {
        let mut completion = self.completion.clone();
        loop {
            if let Some(result) = completion.borrow().clone() {
                return result;
            }
            completion
                .changed()
                .await
                .map_err(|_| RepositoryJobError::Cancelled)?;
        }
    }

    #[cfg(test)]
    pub(crate) fn completed_for_test(
        job_id: &str,
        repository_id: String,
        notes_root: PathBuf,
    ) -> Self {
        let (_completion_tx, completion) = watch::channel(Some(Ok(())));
        Self::new(job_id.to_owned(), repository_id, notes_root, completion)
    }

    #[cfg(test)]
    pub(crate) fn pending_for_test(
        job_id: &str,
        repository_id: String,
        notes_root: PathBuf,
    ) -> (Self, watch::Sender<Option<Result<(), RepositoryJobError>>>) {
        let (completion_tx, completion) = watch::channel(None);
        (
            Self::new(job_id.to_owned(), repository_id, notes_root, completion),
            completion_tx,
        )
    }
}

#[derive(Clone, Default)]
pub(crate) struct RepositorySyncResult {
    pub(crate) data_changed: bool,
    pub(crate) transfer: RepositoryTransferSummary,
    pub(crate) conflicts: Vec<RepositoryConflictRecord>,
}

#[derive(Clone)]
pub(crate) struct RepositoryJobCompletion {
    pub(crate) request: SyncJobRequest,
    pub(crate) result: Result<RepositorySyncResult, RepositoryJobError>,
}

#[derive(Clone)]
pub(crate) struct JobCancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl JobCancellationToken {
    pub(crate) fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

#[derive(Clone)]
pub(crate) struct SyncAttemptContext {
    pub(crate) request: SyncJobRequest,
    pub(crate) job_id: String,
    pub(crate) attempt: u8,
    pub(crate) cancellation: JobCancellationToken,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum RepositoryJobError {
    InvalidBinding,
    WorkingTreeChanged,
    Cancelled,
    StatusUnavailable,
    ConfigUnavailable,
    RepositoryUnavailable,
    CloudUnavailable,
    DnsUnavailable,
}

impl RepositoryJobError {
    pub(crate) fn safe_code(self) -> &'static str {
        match self {
            Self::InvalidBinding => "dejavu-invalid-binding",
            Self::WorkingTreeChanged => "dejavu-working-tree-changed",
            Self::Cancelled => "dejavu-job-cancelled",
            Self::StatusUnavailable => "dejavu-status-unavailable",
            Self::ConfigUnavailable => "dejavu-config-unavailable",
            Self::RepositoryUnavailable => "dejavu-repository-unavailable",
            Self::CloudUnavailable => "dejavu-cloud-unavailable",
            Self::DnsUnavailable => "dejavu-dns-unavailable",
        }
    }
}

impl fmt::Display for RepositoryJobError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.safe_code())
    }
}

impl std::error::Error for RepositoryJobError {}

pub(crate) trait RepositoryJobRunner: Send + Sync {
    fn validate(&self, request: SyncJobRequest) -> Result<SyncJobRequest, RepositoryJobError>;

    fn run_attempt<'a>(
        &'a self,
        context: SyncAttemptContext,
    ) -> BoxFuture<'a, Result<RepositorySyncResult, RepositoryJobError>>;
}

pub(crate) trait RepositoryStatusSink: Send + Sync {
    fn publish<'a>(
        &'a self,
        status: RepositorySyncStatus,
    ) -> BoxFuture<'a, Result<(), RepositoryJobError>>;
}

pub(crate) trait RepositoryJobLifecycle: Send + Sync {
    fn prepare_dns_retry(&self, request: &SyncJobRequest) -> Result<bool, RepositoryJobError>;

    fn record_completion(
        &self,
        completion: RepositoryJobCompletion,
    ) -> Result<(), RepositoryJobError>;
}

#[derive(Clone)]
pub(crate) struct DejavuSyncService {
    inner: Arc<DejavuSyncServiceInner>,
}

struct DejavuSyncServiceInner {
    runner: Arc<dyn RepositoryJobRunner>,
    status_sink: Arc<dyn RepositoryStatusSink>,
    repository_locks: Mutex<HashMap<String, Arc<AsyncMutex<()>>>>,
    global_gate: Arc<RwLock<()>>,
    jobs: Mutex<JobRegistry>,
    lifecycle: OnceLock<Arc<dyn RepositoryJobLifecycle>>,
}

#[derive(Default)]
struct JobRegistry {
    generation: u64,
    draining: bool,
    cancellations: HashMap<String, JobCancellationToken>,
}

impl DejavuSyncService {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn new<Runner, Sink>(runner: Arc<Runner>, status_sink: Arc<Sink>) -> Self
    where
        Runner: RepositoryJobRunner + 'static,
        Sink: RepositoryStatusSink + 'static,
    {
        let runner: Arc<dyn RepositoryJobRunner> = runner;
        let status_sink: Arc<dyn RepositoryStatusSink> = status_sink;
        Self {
            inner: Arc::new(DejavuSyncServiceInner {
                runner,
                status_sink,
                repository_locks: Mutex::new(HashMap::new()),
                global_gate: Arc::new(RwLock::new(())),
                jobs: Mutex::new(JobRegistry::default()),
                lifecycle: OnceLock::new(),
            }),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn install_lifecycle<Lifecycle>(
        &self,
        lifecycle: Arc<Lifecycle>,
    ) -> Result<(), RepositoryJobError>
    where
        Lifecycle: RepositoryJobLifecycle + 'static,
    {
        let lifecycle: Arc<dyn RepositoryJobLifecycle> = lifecycle;
        self.inner
            .lifecycle
            .set(lifecycle)
            .map_err(|_| RepositoryJobError::RepositoryUnavailable)
    }

    pub(crate) async fn enqueue(
        &self,
        request: SyncJobRequest,
    ) -> Result<AcceptedSyncJob, RepositoryJobError> {
        self.enqueue_inner(request, true, None).await
    }

    fn enqueue_inner<'a>(
        &'a self,
        request: SyncJobRequest,
        allow_follow_up: bool,
        inherited_generation: Option<u64>,
    ) -> BoxFuture<'a, Result<AcceptedSyncJob, RepositoryJobError>> {
        Box::pin(async move {
            let generation = self.accepting_generation(inherited_generation)?;
            let request = self.inner.runner.validate(request)?;
            self.require_accepting_generation(generation)?;
            let job_id = uuid::Uuid::new_v4().to_string();
            let (completion_tx, completion) = watch::channel(None);
            let accepted = AcceptedSyncJob::new(
                job_id.clone(),
                request.repository_id.clone(),
                request.notes_root.clone(),
                completion,
            );
            self.inner
                .status_sink
                .publish(RepositorySyncStatus::attempting(
                    &request,
                    job_id.clone(),
                    1,
                    sync_status_timestamp(),
                ))
                .await?;

            // Acquire the ordinary-job guard before reporting acceptance. A key writer
            // that begins after enqueue returns therefore observes every accepted job.
            let ordinary_guard = Arc::clone(&self.inner.global_gate).read_owned().await;
            let cancellation = JobCancellationToken::new();
            let registered = {
                let mut jobs = self.inner.jobs.lock().unwrap();
                if jobs.draining || jobs.generation != generation {
                    false
                } else {
                    jobs.cancellations
                        .insert(job_id.clone(), cancellation.clone());
                    true
                }
            };
            if !registered {
                drop(ordinary_guard);
                let _status_result = self
                    .inner
                    .status_sink
                    .publish(RepositorySyncStatus::failed(
                        &request,
                        job_id,
                        1,
                        sync_status_timestamp(),
                        RepositorySafeError::from(RepositoryJobError::Cancelled),
                    ))
                    .await;
                return Err(RepositoryJobError::Cancelled);
            }
            let service = self.clone();
            tokio::spawn(async move {
                service
                    .run_owned_job(
                        request,
                        job_id,
                        cancellation,
                        allow_follow_up,
                        generation,
                        ordinary_guard,
                        completion_tx,
                    )
                    .await;
            });
            Ok(accepted)
        })
    }

    async fn run_owned_job(
        &self,
        request: SyncJobRequest,
        job_id: String,
        cancellation: JobCancellationToken,
        allow_follow_up: bool,
        generation: u64,
        ordinary_guard: OwnedRwLockReadGuard<()>,
        completion_tx: watch::Sender<Option<Result<(), RepositoryJobError>>>,
    ) {
        let repository_lock = self.repository_lock(&request.repository_id);
        let _repository_guard = repository_lock.lock().await;
        let mut result = Err(RepositoryJobError::Cancelled);
        let mut final_attempt = 1;
        let mut dns_retry_performed = false;
        for attempt in 1..=MAX_WORKING_TREE_ATTEMPTS {
            final_attempt = attempt;
            if cancellation.is_cancelled() {
                result = Err(RepositoryJobError::Cancelled);
                break;
            }
            let _status_result = self
                .inner
                .status_sink
                .publish(RepositorySyncStatus::attempting(
                    &request,
                    job_id.clone(),
                    attempt,
                    sync_status_timestamp(),
                ))
                .await;
            if cancellation.is_cancelled() {
                result = Err(RepositoryJobError::Cancelled);
                break;
            }
            result = self
                .inner
                .runner
                .run_attempt(SyncAttemptContext {
                    request: request.clone(),
                    job_id: job_id.clone(),
                    attempt,
                    cancellation: cancellation.clone(),
                })
                .await;
            if matches!(result, Err(RepositoryJobError::DnsUnavailable))
                && !dns_retry_performed
                && !cancellation.is_cancelled()
                && self
                    .inner
                    .lifecycle
                    .get()
                    .is_some_and(|lifecycle| lifecycle.prepare_dns_retry(&request).unwrap_or(false))
            {
                dns_retry_performed = true;
                result = self
                    .inner
                    .runner
                    .run_attempt(SyncAttemptContext {
                        request: request.clone(),
                        job_id: job_id.clone(),
                        attempt,
                        cancellation: cancellation.clone(),
                    })
                    .await;
            }
            if !matches!(result, Err(RepositoryJobError::WorkingTreeChanged)) {
                break;
            }
        }
        let needs_follow_up = allow_follow_up
            && matches!(result, Err(RepositoryJobError::WorkingTreeChanged))
            && !cancellation.is_cancelled();
        let completed_at = sync_status_timestamp();
        let completion_result = result.clone();
        let owned_operation_result = completion_result.clone().map(|_| ());
        let final_status = match result {
            Ok(result) => RepositorySyncStatus::succeeded(
                &request,
                job_id.clone(),
                final_attempt,
                completed_at,
                result,
            ),
            Err(error) => RepositorySyncStatus::failed(
                &request,
                job_id.clone(),
                final_attempt,
                completed_at,
                RepositorySafeError::from(error),
            ),
        };
        let mut schedule_result = Ok(());
        if let Some(lifecycle) = self.inner.lifecycle.get() {
            let completion = RepositoryJobCompletion {
                request: request.clone(),
                result: completion_result,
            };
            schedule_result = Err(RepositoryJobError::StatusUnavailable);
            for _attempt in 1..=MAX_FINALIZATION_ATTEMPTS {
                schedule_result = lifecycle.record_completion(completion.clone());
                if schedule_result.is_ok() {
                    break;
                }
            }
        }
        let mut status_result = schedule_result;
        if schedule_result.is_ok() {
            status_result = Err(RepositoryJobError::StatusUnavailable);
            for _attempt in 1..=MAX_FINALIZATION_ATTEMPTS {
                status_result = self.inner.status_sink.publish(final_status.clone()).await;
                if status_result.is_ok() {
                    break;
                }
            }
        }
        self.inner
            .jobs
            .lock()
            .unwrap()
            .cancellations
            .remove(&job_id);
        drop(_repository_guard);
        drop(ordinary_guard);

        let owned_job_result = owned_operation_result
            .and(schedule_result)
            .and(status_result);
        let _completion_result = completion_tx.send(Some(owned_job_result));

        if needs_follow_up {
            let service = self.clone();
            tokio::spawn(async move {
                let _accepted = service
                    .enqueue_inner(request, false, Some(generation))
                    .await;
            });
        }
    }

    fn accepting_generation(
        &self,
        inherited_generation: Option<u64>,
    ) -> Result<u64, RepositoryJobError> {
        let jobs = self.inner.jobs.lock().unwrap();
        if jobs.draining {
            return Err(RepositoryJobError::Cancelled);
        }
        match inherited_generation {
            Some(generation) if generation == jobs.generation => Ok(generation),
            Some(_) => Err(RepositoryJobError::Cancelled),
            None => Ok(jobs.generation),
        }
    }

    fn require_accepting_generation(&self, generation: u64) -> Result<(), RepositoryJobError> {
        let jobs = self.inner.jobs.lock().unwrap();
        if jobs.draining || jobs.generation != generation {
            Err(RepositoryJobError::Cancelled)
        } else {
            Ok(())
        }
    }

    fn repository_lock(&self, repository_id: &str) -> Arc<AsyncMutex<()>> {
        Arc::clone(
            self.inner
                .repository_locks
                .lock()
                .unwrap()
                .entry(repository_id.to_owned())
                .or_insert_with(|| Arc::new(AsyncMutex::new(()))),
        )
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) async fn with_global_key_write_barrier<T>(
        &self,
        operation: impl Future<Output = T>,
    ) -> T {
        let _barrier = self.inner.global_gate.write().await;
        operation.await
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn cancel_all_for_shutdown_or_reset(&self) -> BoxFuture<'static, ()> {
        let cancellation_generation = {
            let mut jobs = self.inner.jobs.lock().unwrap();
            jobs.generation = jobs.generation.wrapping_add(1);
            jobs.draining = true;
            for cancellation in jobs.cancellations.values() {
                cancellation.cancel();
            }
            jobs.generation
        };
        let service = self.clone();
        Box::pin(async move {
            let barrier = service.inner.global_gate.write().await;
            drop(barrier);
            let mut jobs = service.inner.jobs.lock().unwrap();
            if jobs.generation == cancellation_generation {
                jobs.draining = false;
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::collections::VecDeque;
    use std::future::Future;
    use std::path::PathBuf;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tempfile::tempdir;
    use time::OffsetDateTime;
    use tokio::sync::{mpsc, oneshot, Notify, Semaphore};

    use super::{
        AcceptedSyncJob, DejavuSyncService, RepositoryJobCompletion, RepositoryJobError,
        RepositoryJobLifecycle, RepositoryJobRunner, RepositoryStatusSink, RepositorySyncResult,
        SyncAttemptContext, SyncJobRequest, MAX_FINALIZATION_ATTEMPTS,
    };
    use crate::dejavu_sync::status::{
        load_repository_sync_status, RepositoryConflictRecord, RepositoryStatusEventEmitter,
        RepositoryStatusStore, RepositorySyncPhase, RepositorySyncStatus,
        RepositoryTransferSummary,
    };
    use crate::sync_config::status::SyncTrigger;

    type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

    struct ControlledRunner {
        active_by_repository: Mutex<HashMap<String, usize>>,
        max_active_by_repository: Mutex<HashMap<String, usize>>,
        active_total: Mutex<usize>,
        max_active_total: Mutex<usize>,
        permits: Semaphore,
        started: mpsc::UnboundedSender<String>,
    }

    impl ControlledRunner {
        fn new(started: mpsc::UnboundedSender<String>) -> Self {
            Self {
                active_by_repository: Mutex::new(HashMap::new()),
                max_active_by_repository: Mutex::new(HashMap::new()),
                active_total: Mutex::new(0),
                max_active_total: Mutex::new(0),
                permits: Semaphore::new(0),
                started,
            }
        }

        fn release(&self, jobs: usize) {
            self.permits.add_permits(jobs);
        }

        fn max_for(&self, repository_id: &str) -> usize {
            self.max_active_by_repository
                .lock()
                .unwrap()
                .get(repository_id)
                .copied()
                .unwrap_or(0)
        }

        fn max_total(&self) -> usize {
            *self.max_active_total.lock().unwrap()
        }
    }

    impl RepositoryJobRunner for ControlledRunner {
        fn validate(&self, request: SyncJobRequest) -> Result<SyncJobRequest, RepositoryJobError> {
            Ok(request)
        }

        fn run_attempt<'a>(
            &'a self,
            context: SyncAttemptContext,
        ) -> BoxFuture<'a, Result<RepositorySyncResult, RepositoryJobError>> {
            Box::pin(async move {
                {
                    let mut active = self.active_by_repository.lock().unwrap();
                    let count = active
                        .entry(context.request.repository_id.clone())
                        .or_default();
                    *count += 1;
                    let mut maximum = self.max_active_by_repository.lock().unwrap();
                    let recorded = maximum
                        .entry(context.request.repository_id.clone())
                        .or_default();
                    *recorded = (*recorded).max(*count);
                }
                {
                    let mut active = self.active_total.lock().unwrap();
                    *active += 1;
                    let mut maximum = self.max_active_total.lock().unwrap();
                    *maximum = (*maximum).max(*active);
                }
                self.started
                    .send(context.request.repository_id.clone())
                    .unwrap();
                let permit = self.permits.acquire().await.unwrap();
                permit.forget();
                {
                    let mut active = self.active_by_repository.lock().unwrap();
                    *active.get_mut(&context.request.repository_id).unwrap() -= 1;
                }
                *self.active_total.lock().unwrap() -= 1;
                Ok(RepositorySyncResult::default())
            })
        }
    }

    #[derive(Default)]
    struct MemoryStatusSink {
        statuses: Mutex<Vec<RepositorySyncStatus>>,
        changed: Notify,
    }

    impl MemoryStatusSink {
        async fn wait_for_phase(&self, phase: RepositorySyncPhase, count: usize) {
            tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    let listening = self.changed.notified();
                    if self
                        .statuses
                        .lock()
                        .unwrap()
                        .iter()
                        .filter(|status| status.phase == phase)
                        .count()
                        >= count
                    {
                        return;
                    }
                    listening.await;
                }
            })
            .await
            .expect("status phase should arrive");
        }
    }

    impl RepositoryStatusSink for MemoryStatusSink {
        fn publish<'a>(
            &'a self,
            status: RepositorySyncStatus,
        ) -> BoxFuture<'a, Result<(), RepositoryJobError>> {
            Box::pin(async move {
                self.statuses.lock().unwrap().push(status);
                self.changed.notify_waiters();
                Ok(())
            })
        }
    }

    struct FlakyFinalStatusSink {
        final_failures: AtomicUsize,
        final_attempts: AtomicUsize,
    }

    impl FlakyFinalStatusSink {
        fn new(final_failures: usize) -> Self {
            Self {
                final_failures: AtomicUsize::new(final_failures),
                final_attempts: AtomicUsize::new(0),
            }
        }
    }

    impl RepositoryStatusSink for FlakyFinalStatusSink {
        fn publish<'a>(
            &'a self,
            status: RepositorySyncStatus,
        ) -> BoxFuture<'a, Result<(), RepositoryJobError>> {
            Box::pin(async move {
                if status.phase == RepositorySyncPhase::Attempting {
                    return Ok(());
                }
                self.final_attempts.fetch_add(1, Ordering::SeqCst);
                if self
                    .final_failures
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
                Ok(())
            })
        }
    }

    struct BlockingStatusSink {
        statuses: Mutex<Vec<RepositorySyncStatus>>,
        blocked_phase: RepositorySyncPhase,
        blocked_occurrence: usize,
        matching_publishes: AtomicUsize,
        entered: mpsc::UnboundedSender<()>,
        release: Semaphore,
    }

    impl BlockingStatusSink {
        fn new(
            blocked_phase: RepositorySyncPhase,
            blocked_occurrence: usize,
            entered: mpsc::UnboundedSender<()>,
        ) -> Self {
            Self {
                statuses: Mutex::new(Vec::new()),
                blocked_phase,
                blocked_occurrence,
                matching_publishes: AtomicUsize::new(0),
                entered,
                release: Semaphore::new(0),
            }
        }

        fn release(&self) {
            self.release.add_permits(1);
        }
    }

    impl RepositoryStatusSink for BlockingStatusSink {
        fn publish<'a>(
            &'a self,
            status: RepositorySyncStatus,
        ) -> BoxFuture<'a, Result<(), RepositoryJobError>> {
            Box::pin(async move {
                let should_block = status.phase == self.blocked_phase
                    && self.matching_publishes.fetch_add(1, Ordering::SeqCst) + 1
                        == self.blocked_occurrence;
                if should_block {
                    self.entered.send(()).unwrap();
                    let permit = self.release.acquire().await.unwrap();
                    permit.forget();
                }
                self.statuses.lock().unwrap().push(status);
                Ok(())
            })
        }
    }

    struct NoopStatusEmitter;

    impl RepositoryStatusEventEmitter for NoopStatusEmitter {
        fn emit(&self, _status: &RepositorySyncStatus) -> Result<(), RepositoryJobError> {
            Ok(())
        }
    }

    struct RemovableListenerEmitter {
        listening: AtomicBool,
        events: Mutex<Vec<RepositorySyncStatus>>,
    }

    impl RemovableListenerEmitter {
        fn new() -> Self {
            Self {
                listening: AtomicBool::new(true),
                events: Mutex::new(Vec::new()),
            }
        }

        fn remove_listener(&self) {
            self.listening.store(false, Ordering::SeqCst);
        }
    }

    impl RepositoryStatusEventEmitter for RemovableListenerEmitter {
        fn emit(&self, status: &RepositorySyncStatus) -> Result<(), RepositoryJobError> {
            if self.listening.load(Ordering::SeqCst) {
                self.events.lock().unwrap().push(status.clone());
            }
            Ok(())
        }
    }

    struct BlockingResultRunner {
        started: mpsc::UnboundedSender<()>,
        release: Semaphore,
        result: RepositorySyncResult,
    }

    impl BlockingResultRunner {
        fn new(started: mpsc::UnboundedSender<()>, result: RepositorySyncResult) -> Self {
            Self {
                started,
                release: Semaphore::new(0),
                result,
            }
        }

        fn release(&self) {
            self.release.add_permits(1);
        }
    }

    impl RepositoryJobRunner for BlockingResultRunner {
        fn validate(&self, request: SyncJobRequest) -> Result<SyncJobRequest, RepositoryJobError> {
            Ok(request)
        }

        fn run_attempt<'a>(
            &'a self,
            _context: SyncAttemptContext,
        ) -> BoxFuture<'a, Result<RepositorySyncResult, RepositoryJobError>> {
            Box::pin(async move {
                self.started.send(()).unwrap();
                self.release.acquire().await.unwrap().forget();
                Ok(self.result.clone())
            })
        }
    }

    struct BlockingFinalStore {
        store: Arc<RepositoryStatusStore>,
        entered: mpsc::UnboundedSender<()>,
        release: Semaphore,
    }

    impl BlockingFinalStore {
        fn release(&self) {
            self.release.add_permits(1);
        }
    }

    impl RepositoryStatusSink for BlockingFinalStore {
        fn publish<'a>(
            &'a self,
            status: RepositorySyncStatus,
        ) -> BoxFuture<'a, Result<(), RepositoryJobError>> {
            Box::pin(async move {
                if status.phase != RepositorySyncPhase::Attempting {
                    self.entered.send(()).unwrap();
                    self.release.acquire().await.unwrap().forget();
                }
                self.store.publish(status).await
            })
        }
    }

    struct StoreScheduleLifecycle {
        store: Arc<RepositoryStatusStore>,
        next_due: OffsetDateTime,
    }

    impl RepositoryJobLifecycle for StoreScheduleLifecycle {
        fn prepare_dns_retry(&self, _request: &SyncJobRequest) -> Result<bool, RepositoryJobError> {
            Ok(false)
        }

        fn record_completion(
            &self,
            completion: RepositoryJobCompletion,
        ) -> Result<(), RepositoryJobError> {
            let mut install_due =
                |schedule: &mut crate::dejavu_sync::status::RepositorySchedule| {
                    schedule.next_scheduled_at = Some(self.next_due);
                    true
                };
            self.store
                .update_schedule(&completion.request.repository_id, &mut install_due)?;
            Ok(())
        }
    }

    struct WorkingTreeChangedRunner {
        attempts: AtomicUsize,
        changed: Notify,
    }

    impl WorkingTreeChangedRunner {
        fn new() -> Self {
            Self {
                attempts: AtomicUsize::new(0),
                changed: Notify::new(),
            }
        }

        async fn wait_for_attempts(&self, expected: usize) {
            tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    let listening = self.changed.notified();
                    if self.attempts.load(Ordering::SeqCst) >= expected {
                        return;
                    }
                    listening.await;
                }
            })
            .await
            .expect("expected working-tree retry attempts should run");
        }

        async fn assert_no_additional_attempt(&self) {
            let listening = self.changed.notified();
            assert!(tokio::time::timeout(Duration::from_millis(100), listening)
                .await
                .is_err());
        }
    }

    impl RepositoryJobRunner for WorkingTreeChangedRunner {
        fn validate(&self, request: SyncJobRequest) -> Result<SyncJobRequest, RepositoryJobError> {
            Ok(request)
        }

        fn run_attempt<'a>(
            &'a self,
            _context: SyncAttemptContext,
        ) -> BoxFuture<'a, Result<RepositorySyncResult, RepositoryJobError>> {
            Box::pin(async move {
                self.attempts.fetch_add(1, Ordering::SeqCst);
                self.changed.notify_waiters();
                Err(RepositoryJobError::WorkingTreeChanged)
            })
        }
    }

    struct FollowUpDetectingRunner {
        primary_job_id: Mutex<Option<String>>,
        primary_attempts: AtomicUsize,
        primary_changed: Notify,
        other_started: mpsc::UnboundedSender<String>,
    }

    struct SequenceRunner {
        results: Mutex<VecDeque<Result<RepositorySyncResult, RepositoryJobError>>>,
        attempts: AtomicUsize,
    }

    impl SequenceRunner {
        fn new(
            results: impl IntoIterator<Item = Result<RepositorySyncResult, RepositoryJobError>>,
        ) -> Self {
            Self {
                results: Mutex::new(results.into_iter().collect()),
                attempts: AtomicUsize::new(0),
            }
        }
    }

    impl RepositoryJobRunner for SequenceRunner {
        fn validate(&self, request: SyncJobRequest) -> Result<SyncJobRequest, RepositoryJobError> {
            Ok(request)
        }

        fn run_attempt<'a>(
            &'a self,
            _context: SyncAttemptContext,
        ) -> BoxFuture<'a, Result<RepositorySyncResult, RepositoryJobError>> {
            Box::pin(async move {
                self.attempts.fetch_add(1, Ordering::SeqCst);
                self.results
                    .lock()
                    .unwrap()
                    .pop_front()
                    .expect("fixture result for every complete attempt")
            })
        }
    }

    struct RecordingLifecycle {
        allow_dns_retry: bool,
        dns_prepares: AtomicUsize,
        completion_attempts: AtomicUsize,
        completion_failures: AtomicUsize,
        completions: Mutex<Vec<RepositoryJobCompletion>>,
        changed: Notify,
    }

    impl RecordingLifecycle {
        fn new(allow_dns_retry: bool) -> Self {
            Self {
                allow_dns_retry,
                dns_prepares: AtomicUsize::new(0),
                completion_attempts: AtomicUsize::new(0),
                completion_failures: AtomicUsize::new(0),
                completions: Mutex::new(Vec::new()),
                changed: Notify::new(),
            }
        }

        fn fail_next_completion(&self) {
            self.completion_failures.fetch_add(1, Ordering::SeqCst);
        }

        async fn wait_for_completion(&self) {
            tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    let listening = self.changed.notified();
                    if !self.completions.lock().unwrap().is_empty() {
                        return;
                    }
                    listening.await;
                }
            })
            .await
            .expect("job lifecycle completion should arrive");
        }
    }

    impl RepositoryJobLifecycle for RecordingLifecycle {
        fn prepare_dns_retry(&self, _request: &SyncJobRequest) -> Result<bool, RepositoryJobError> {
            self.dns_prepares.fetch_add(1, Ordering::SeqCst);
            Ok(self.allow_dns_retry)
        }

        fn record_completion(
            &self,
            completion: RepositoryJobCompletion,
        ) -> Result<(), RepositoryJobError> {
            self.completion_attempts.fetch_add(1, Ordering::SeqCst);
            if self
                .completion_failures
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
            self.completions.lock().unwrap().push(completion);
            self.changed.notify_waiters();
            Ok(())
        }
    }

    impl FollowUpDetectingRunner {
        fn new(other_started: mpsc::UnboundedSender<String>) -> Self {
            Self {
                primary_job_id: Mutex::new(None),
                primary_attempts: AtomicUsize::new(0),
                primary_changed: Notify::new(),
                other_started,
            }
        }

        async fn wait_for_primary_attempts(&self, expected: usize) {
            tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    let listening = self.primary_changed.notified();
                    if self.primary_attempts.load(Ordering::SeqCst) >= expected {
                        return;
                    }
                    listening.await;
                }
            })
            .await
            .expect("expected primary working-tree attempts should run");
        }
    }

    impl RepositoryJobRunner for FollowUpDetectingRunner {
        fn validate(&self, request: SyncJobRequest) -> Result<SyncJobRequest, RepositoryJobError> {
            Ok(request)
        }

        fn run_attempt<'a>(
            &'a self,
            context: SyncAttemptContext,
        ) -> BoxFuture<'a, Result<RepositorySyncResult, RepositoryJobError>> {
            Box::pin(async move {
                let is_primary = {
                    let mut primary_job_id = self.primary_job_id.lock().unwrap();
                    let primary_job_id =
                        primary_job_id.get_or_insert_with(|| context.job_id.clone());
                    primary_job_id == &context.job_id
                };
                if is_primary {
                    self.primary_attempts.fetch_add(1, Ordering::SeqCst);
                    self.primary_changed.notify_waiters();
                    Err(RepositoryJobError::WorkingTreeChanged)
                } else {
                    self.other_started.send(context.job_id).unwrap();
                    Ok(RepositorySyncResult::default())
                }
            })
        }
    }

    fn request(repository_id: &str) -> SyncJobRequest {
        SyncJobRequest {
            notes_root: PathBuf::from(format!("/notes/{repository_id}")),
            repository_id: repository_id.to_owned(),
            trigger: SyncTrigger::Manual,
        }
    }

    fn service(runner: Arc<ControlledRunner>, sink: Arc<MemoryStatusSink>) -> DejavuSyncService {
        DejavuSyncService::new(runner, sink)
    }

    async fn receive_start(started: &mut mpsc::UnboundedReceiver<String>) -> String {
        tokio::time::timeout(Duration::from_secs(2), started.recv())
            .await
            .expect("job should start")
            .expect("start channel should stay open")
    }

    #[tokio::test]
    async fn enqueue_returns_acceptance_while_the_repository_run_is_blocked() {
        let (started_tx, mut started_rx) = mpsc::unbounded_channel();
        let runner = Arc::new(ControlledRunner::new(started_tx));
        let sink = Arc::new(MemoryStatusSink::default());
        let service = service(Arc::clone(&runner), Arc::clone(&sink));

        let accepted: AcceptedSyncJob = tokio::time::timeout(
            Duration::from_millis(250),
            service.enqueue(request("00000000-0000-4000-8000-000000000001")),
        )
        .await
        .expect("enqueue must not await the repository run")
        .expect("valid job should be accepted");

        assert_eq!(
            accepted.repository_id,
            "00000000-0000-4000-8000-000000000001"
        );
        assert_eq!(receive_start(&mut started_rx).await, accepted.repository_id);
        runner.release(1);
        sink.wait_for_phase(RepositorySyncPhase::Succeeded, 1).await;
    }

    #[tokio::test]
    async fn accepted_job_completion_waits_for_the_owned_job_to_finish() {
        let (started_tx, mut started_rx) = mpsc::unbounded_channel();
        let runner = Arc::new(ControlledRunner::new(started_tx));
        let sink = Arc::new(MemoryStatusSink::default());
        let service = service(Arc::clone(&runner), Arc::clone(&sink));

        let accepted = service
            .enqueue(request("00000000-0000-4000-8000-000000000041"))
            .await
            .unwrap();
        receive_start(&mut started_rx).await;
        assert!(
            tokio::time::timeout(Duration::from_millis(100), accepted.wait_for_completion(),)
                .await
                .is_err()
        );

        runner.release(1);
        assert_eq!(accepted.wait_for_completion().await, Ok(()));
        sink.wait_for_phase(RepositorySyncPhase::Succeeded, 1).await;
    }

    #[tokio::test]
    async fn dropping_the_enqueue_caller_does_not_cancel_an_accepted_job() {
        let (started_tx, mut started_rx) = mpsc::unbounded_channel();
        let runner = Arc::new(ControlledRunner::new(started_tx));
        let sink = Arc::new(MemoryStatusSink::default());
        let service = service(Arc::clone(&runner), Arc::clone(&sink));

        let accepted = service
            .enqueue(request("00000000-0000-4000-8000-000000000002"))
            .await
            .unwrap();
        drop(service);
        assert_eq!(receive_start(&mut started_rx).await, accepted.repository_id);

        runner.release(1);
        sink.wait_for_phase(RepositorySyncPhase::Succeeded, 1).await;
    }

    #[tokio::test]
    async fn removing_the_settings_listener_does_not_cancel_status_or_conflict_history() {
        let app_data = tempdir().unwrap();
        let repository_id = "00000000-0000-4000-8000-000000000042";
        let (started_tx, mut started_rx) = mpsc::unbounded_channel();
        let runner = Arc::new(BlockingResultRunner::new(
            started_tx,
            RepositorySyncResult {
                data_changed: true,
                transfer: RepositoryTransferSummary {
                    download_bytes: 7,
                    download_chunks: 1,
                    download_files: 1,
                    upload_bytes: 0,
                    upload_chunks: 0,
                    upload_files: 0,
                },
                conflicts: vec![RepositoryConflictRecord {
                    relative_path: "conflicted.md".to_owned(),
                    occurred_at: "2026-07-26T10:00:00Z".to_owned(),
                }],
            },
        ));
        let emitter = Arc::new(RemovableListenerEmitter::new());
        let store = Arc::new(RepositoryStatusStore::new(
            app_data.path(),
            Arc::clone(&emitter),
        ));
        let service = DejavuSyncService::new(Arc::clone(&runner), Arc::clone(&store));

        let accepted = service.enqueue(request(repository_id)).await.unwrap();
        started_rx.recv().await.expect("accepted job should start");
        let attempting = load_repository_sync_status(app_data.path(), repository_id)
            .unwrap()
            .unwrap();
        assert_eq!(attempting.phase, RepositorySyncPhase::Attempting);

        emitter.remove_listener();
        drop(accepted);
        drop(service);
        runner.release();

        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if load_repository_sync_status(app_data.path(), repository_id)
                    .unwrap()
                    .is_some_and(|status| status.phase == RepositorySyncPhase::Succeeded)
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("listener-free job should persist terminal status");
        let terminal = load_repository_sync_status(app_data.path(), repository_id)
            .unwrap()
            .unwrap();
        assert_eq!(terminal.phase, RepositorySyncPhase::Succeeded);
        assert_eq!(terminal.transfer.download_bytes, 7);
        assert_eq!(terminal.conflicts.len(), 1);
        assert_eq!(terminal.conflicts[0].relative_path, "conflicted.md");
        let events = emitter.events.lock().unwrap();
        assert!(!events.is_empty());
        assert!(events
            .iter()
            .all(|status| status.phase == RepositorySyncPhase::Attempting));
    }

    #[tokio::test]
    async fn accepted_jobs_for_one_repository_run_serially() {
        let (started_tx, mut started_rx) = mpsc::unbounded_channel();
        let runner = Arc::new(ControlledRunner::new(started_tx));
        let sink = Arc::new(MemoryStatusSink::default());
        let service = service(Arc::clone(&runner), Arc::clone(&sink));
        let repository = "00000000-0000-4000-8000-000000000003";

        service.enqueue(request(repository)).await.unwrap();
        service.enqueue(request(repository)).await.unwrap();
        assert_eq!(receive_start(&mut started_rx).await, repository);
        assert!(
            tokio::time::timeout(Duration::from_millis(100), started_rx.recv())
                .await
                .is_err()
        );
        assert_eq!(runner.max_for(repository), 1);

        runner.release(1);
        assert_eq!(receive_start(&mut started_rx).await, repository);
        runner.release(1);
        sink.wait_for_phase(RepositorySyncPhase::Succeeded, 2).await;
        assert_eq!(runner.max_for(repository), 1);
    }

    #[tokio::test]
    async fn queued_job_republishes_attempting_after_it_acquires_the_repository_lock() {
        let (started_tx, mut started_rx) = mpsc::unbounded_channel();
        let runner = Arc::new(ControlledRunner::new(started_tx));
        let sink = Arc::new(MemoryStatusSink::default());
        let service = service(Arc::clone(&runner), Arc::clone(&sink));
        let repository = "00000000-0000-4000-8000-000000000012";

        service.enqueue(request(repository)).await.unwrap();
        assert_eq!(receive_start(&mut started_rx).await, repository);
        let queued = service.enqueue(request(repository)).await.unwrap();

        runner.release(1);
        assert_eq!(receive_start(&mut started_rx).await, repository);
        let latest = sink.statuses.lock().unwrap().last().cloned().unwrap();
        assert_eq!(latest.phase, RepositorySyncPhase::Attempting);
        assert_eq!(latest.job_id, queued.job_id);
        assert_eq!(latest.attempt, 1);

        runner.release(1);
        sink.wait_for_phase(RepositorySyncPhase::Succeeded, 2).await;
    }

    #[tokio::test]
    async fn accepted_jobs_for_different_repositories_run_concurrently() {
        let (started_tx, mut started_rx) = mpsc::unbounded_channel();
        let runner = Arc::new(ControlledRunner::new(started_tx));
        let sink = Arc::new(MemoryStatusSink::default());
        let service = service(Arc::clone(&runner), Arc::clone(&sink));

        service
            .enqueue(request("00000000-0000-4000-8000-000000000004"))
            .await
            .unwrap();
        service
            .enqueue(request("00000000-0000-4000-8000-000000000005"))
            .await
            .unwrap();
        let first = receive_start(&mut started_rx).await;
        let second = receive_start(&mut started_rx).await;

        assert_ne!(first, second);
        assert_eq!(runner.max_total(), 2);
        runner.release(2);
        sink.wait_for_phase(RepositorySyncPhase::Succeeded, 2).await;
    }

    #[tokio::test]
    async fn global_key_write_barrier_waits_for_all_accepted_repository_jobs() {
        let (started_tx, mut started_rx) = mpsc::unbounded_channel();
        let runner = Arc::new(ControlledRunner::new(started_tx));
        let sink = Arc::new(MemoryStatusSink::default());
        let service = service(Arc::clone(&runner), Arc::clone(&sink));

        service
            .enqueue(request("00000000-0000-4000-8000-000000000006"))
            .await
            .unwrap();
        service
            .enqueue(request("00000000-0000-4000-8000-000000000007"))
            .await
            .unwrap();
        receive_start(&mut started_rx).await;
        receive_start(&mut started_rx).await;

        let (entered_tx, mut entered_rx) = oneshot::channel();
        let barrier = {
            let service = service.clone();
            tokio::spawn(async move {
                service
                    .with_global_key_write_barrier(async move {
                        entered_tx.send(()).unwrap();
                    })
                    .await;
            })
        };
        assert!(
            tokio::time::timeout(Duration::from_millis(100), &mut entered_rx)
                .await
                .is_err()
        );

        runner.release(2);
        sink.wait_for_phase(RepositorySyncPhase::Succeeded, 2).await;
        tokio::time::timeout(Duration::from_secs(2), entered_rx)
            .await
            .expect("barrier should enter after ordinary jobs finish")
            .expect("barrier should signal entry");
        barrier.await.unwrap();
    }

    #[tokio::test]
    async fn working_tree_changes_retry_three_attempts_then_queue_only_one_follow_up() {
        let runner = Arc::new(WorkingTreeChangedRunner::new());
        let sink = Arc::new(MemoryStatusSink::default());
        let service = DejavuSyncService::new(Arc::clone(&runner), Arc::clone(&sink));

        service
            .enqueue(request("00000000-0000-4000-8000-000000000008"))
            .await
            .unwrap();
        runner.wait_for_attempts(6).await;
        sink.wait_for_phase(RepositorySyncPhase::Failed, 2).await;
        runner.assert_no_additional_attempt().await;

        assert_eq!(runner.attempts.load(Ordering::SeqCst), 6);
        let statuses = sink.statuses.lock().unwrap();
        assert_eq!(
            statuses
                .iter()
                .filter(|status| status.phase == RepositorySyncPhase::Failed)
                .count(),
            2
        );
        assert_eq!(
            statuses
                .iter()
                .filter(|status| status.phase == RepositorySyncPhase::Attempting)
                .map(|status| status.attempt)
                .max(),
            Some(3)
        );
    }

    #[tokio::test]
    async fn shutdown_generation_rejects_an_enqueue_blocked_before_job_registration() {
        let (started_tx, mut started_rx) = mpsc::unbounded_channel();
        let runner = Arc::new(ControlledRunner::new(started_tx));
        let (status_entered_tx, mut status_entered_rx) = mpsc::unbounded_channel();
        let sink = Arc::new(BlockingStatusSink::new(
            RepositorySyncPhase::Attempting,
            1,
            status_entered_tx,
        ));
        let service = DejavuSyncService::new(Arc::clone(&runner), Arc::clone(&sink));
        let enqueue = {
            let service = service.clone();
            tokio::spawn(async move {
                service
                    .enqueue(request("00000000-0000-4000-8000-000000000010"))
                    .await
            })
        };
        status_entered_rx
            .recv()
            .await
            .expect("initial status publish should block");

        service.cancel_all_for_shutdown_or_reset().await;
        sink.release();

        assert!(matches!(
            enqueue.await.unwrap(),
            Err(RepositoryJobError::Cancelled)
        ));
        assert!(matches!(
            started_rx.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
    }

    #[tokio::test]
    async fn shutdown_generation_prevents_a_follow_up_derived_during_final_status_publish() {
        let (other_started_tx, mut other_started_rx) = mpsc::unbounded_channel();
        let runner = Arc::new(FollowUpDetectingRunner::new(other_started_tx));
        let (status_entered_tx, mut status_entered_rx) = mpsc::unbounded_channel();
        let sink = Arc::new(BlockingStatusSink::new(
            RepositorySyncPhase::Failed,
            1,
            status_entered_tx,
        ));
        let service = DejavuSyncService::new(Arc::clone(&runner), Arc::clone(&sink));
        let repository = "00000000-0000-4000-8000-000000000011";

        service.enqueue(request(repository)).await.unwrap();
        runner.wait_for_primary_attempts(3).await;
        status_entered_rx
            .recv()
            .await
            .expect("final failed status publish should block");

        let draining = service.cancel_all_for_shutdown_or_reset();
        sink.release();
        draining.await;

        let explicit = service.enqueue(request(repository)).await.unwrap();
        let started_job_id = tokio::time::timeout(Duration::from_secs(2), other_started_rx.recv())
            .await
            .expect("the explicit post-reset job should start")
            .expect("runner start channel should stay open");
        assert_eq!(started_job_id, explicit.job_id);
        assert_eq!(runner.primary_attempts.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn shutdown_or_reset_token_cancels_queued_jobs_without_caller_ownership() {
        let (started_tx, mut started_rx) = mpsc::unbounded_channel();
        let runner = Arc::new(ControlledRunner::new(started_tx));
        let sink = Arc::new(MemoryStatusSink::default());
        let service = service(Arc::clone(&runner), Arc::clone(&sink));
        let repository = "00000000-0000-4000-8000-000000000009";

        service.enqueue(request(repository)).await.unwrap();
        service.enqueue(request(repository)).await.unwrap();
        assert_eq!(receive_start(&mut started_rx).await, repository);
        let draining = service.cancel_all_for_shutdown_or_reset();
        runner.release(1);
        draining.await;

        sink.wait_for_phase(RepositorySyncPhase::Succeeded, 1).await;
        sink.wait_for_phase(RepositorySyncPhase::Failed, 1).await;
        assert!(
            tokio::time::timeout(Duration::from_millis(100), started_rx.recv())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn typed_dns_failure_retries_one_complete_attempt_then_reports_success() {
        let runner = Arc::new(SequenceRunner::new([
            Err(RepositoryJobError::DnsUnavailable),
            Ok(RepositorySyncResult {
                data_changed: true,
                ..RepositorySyncResult::default()
            }),
        ]));
        let sink = Arc::new(MemoryStatusSink::default());
        let lifecycle = Arc::new(RecordingLifecycle::new(true));
        let service = DejavuSyncService::new(Arc::clone(&runner), Arc::clone(&sink));
        service.install_lifecycle(Arc::clone(&lifecycle)).unwrap();

        service
            .enqueue(request("00000000-0000-4000-8000-000000000013"))
            .await
            .unwrap();
        lifecycle.wait_for_completion().await;

        assert_eq!(runner.attempts.load(Ordering::SeqCst), 2);
        assert_eq!(lifecycle.dns_prepares.load(Ordering::SeqCst), 1);
        let completions = lifecycle.completions.lock().unwrap();
        assert!(completions[0]
            .result
            .as_ref()
            .is_ok_and(|result| result.data_changed));
    }

    #[tokio::test]
    async fn dns_retry_is_separately_bounded_to_one_complete_retry() {
        let runner = Arc::new(SequenceRunner::new([
            Err(RepositoryJobError::DnsUnavailable),
            Err(RepositoryJobError::DnsUnavailable),
        ]));
        let sink = Arc::new(MemoryStatusSink::default());
        let lifecycle = Arc::new(RecordingLifecycle::new(true));
        let service = DejavuSyncService::new(Arc::clone(&runner), Arc::clone(&sink));
        service.install_lifecycle(Arc::clone(&lifecycle)).unwrap();

        service
            .enqueue(request("00000000-0000-4000-8000-000000000014"))
            .await
            .unwrap();
        lifecycle.wait_for_completion().await;

        assert_eq!(runner.attempts.load(Ordering::SeqCst), 2);
        assert_eq!(lifecycle.dns_prepares.load(Ordering::SeqCst), 1);
        assert!(matches!(
            lifecycle.completions.lock().unwrap()[0].result,
            Err(RepositoryJobError::DnsUnavailable)
        ));
    }

    #[tokio::test]
    async fn throttled_dns_failure_does_not_run_a_second_complete_attempt() {
        let runner = Arc::new(SequenceRunner::new([Err(
            RepositoryJobError::DnsUnavailable,
        )]));
        let sink = Arc::new(MemoryStatusSink::default());
        let lifecycle = Arc::new(RecordingLifecycle::new(false));
        let service = DejavuSyncService::new(Arc::clone(&runner), Arc::clone(&sink));
        service.install_lifecycle(Arc::clone(&lifecycle)).unwrap();

        service
            .enqueue(request("00000000-0000-4000-8000-000000000015"))
            .await
            .unwrap();
        lifecycle.wait_for_completion().await;

        assert_eq!(runner.attempts.load(Ordering::SeqCst), 1);
        assert_eq!(lifecycle.dns_prepares.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn final_status_and_schedule_persistence_retry_without_rerunning_sync() {
        let runner = Arc::new(SequenceRunner::new([Ok(RepositorySyncResult::default())]));
        let sink = Arc::new(FlakyFinalStatusSink::new(1));
        let lifecycle = Arc::new(RecordingLifecycle::new(false));
        lifecycle.fail_next_completion();
        let service = DejavuSyncService::new(Arc::clone(&runner), Arc::clone(&sink));
        service.install_lifecycle(Arc::clone(&lifecycle)).unwrap();

        let accepted = service
            .enqueue(request("00000000-0000-4000-8000-000000000016"))
            .await
            .unwrap();
        assert_eq!(accepted.wait_for_completion().await, Ok(()));

        assert_eq!(runner.attempts.load(Ordering::SeqCst), 1);
        assert_eq!(sink.final_attempts.load(Ordering::SeqCst), 2);
        assert_eq!(lifecycle.completion_attempts.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn recoverable_schedule_is_durable_before_terminal_status_publication() {
        let app_data = tempdir().unwrap();
        let repository_id = "00000000-0000-4000-8000-000000000044";
        let next_due = OffsetDateTime::from_unix_timestamp(1_800_003_840).unwrap();
        let store = Arc::new(RepositoryStatusStore::new(
            app_data.path(),
            Arc::new(NoopStatusEmitter),
        ));
        let (entered_tx, mut entered_rx) = mpsc::unbounded_channel();
        let sink = Arc::new(BlockingFinalStore {
            store: Arc::clone(&store),
            entered: entered_tx,
            release: Semaphore::new(0),
        });
        let lifecycle = Arc::new(StoreScheduleLifecycle {
            store: Arc::clone(&store),
            next_due,
        });
        let runner = Arc::new(SequenceRunner::new([Ok(RepositorySyncResult::default())]));
        let service = DejavuSyncService::new(Arc::clone(&runner), Arc::clone(&sink));
        service.install_lifecycle(lifecycle).unwrap();

        let accepted = service.enqueue(request(repository_id)).await.unwrap();
        entered_rx
            .recv()
            .await
            .expect("terminal publication should reach the injected crash boundary");
        let crash_boundary = load_repository_sync_status(app_data.path(), repository_id)
            .unwrap()
            .unwrap();
        assert_eq!(crash_boundary.phase, RepositorySyncPhase::Attempting);
        assert_eq!(crash_boundary.schedule.next_scheduled_at, Some(next_due));

        sink.release();
        assert_eq!(accepted.wait_for_completion().await, Ok(()));
        let terminal = load_repository_sync_status(app_data.path(), repository_id)
            .unwrap()
            .unwrap();
        assert_eq!(terminal.phase, RepositorySyncPhase::Succeeded);
        assert_eq!(terminal.schedule.next_scheduled_at, Some(next_due));
        assert_eq!(runner.attempts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn exhausted_schedule_transition_keeps_attempting_status_and_reports_failure() {
        let runner = Arc::new(SequenceRunner::new([Ok(RepositorySyncResult::default())]));
        let sink = Arc::new(MemoryStatusSink::default());
        let lifecycle = Arc::new(RecordingLifecycle::new(false));
        for _attempt in 1..=MAX_FINALIZATION_ATTEMPTS {
            lifecycle.fail_next_completion();
        }
        let service = DejavuSyncService::new(Arc::clone(&runner), Arc::clone(&sink));
        service.install_lifecycle(Arc::clone(&lifecycle)).unwrap();

        let accepted = service
            .enqueue(request("00000000-0000-4000-8000-000000000045"))
            .await
            .unwrap();
        assert_eq!(
            accepted.wait_for_completion().await,
            Err(RepositoryJobError::StatusUnavailable)
        );

        assert!(sink
            .statuses
            .lock()
            .unwrap()
            .iter()
            .all(|status| status.phase == RepositorySyncPhase::Attempting));
        assert_eq!(runner.attempts.load(Ordering::SeqCst), 1);
        assert_eq!(
            lifecycle.completion_attempts.load(Ordering::SeqCst),
            MAX_FINALIZATION_ATTEMPTS as usize
        );
    }
}
