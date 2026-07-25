use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tokio::sync::{Mutex as AsyncMutex, OwnedRwLockReadGuard, RwLock};

use super::status::{
    RepositoryConflictRecord, RepositorySafeError, RepositorySyncStatus, RepositoryTransferSummary,
};
use crate::sync_config::status::{sync_status_timestamp, SyncTrigger};

const MAX_WORKING_TREE_ATTEMPTS: u8 = 3;

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
}

#[derive(Clone, Default)]
pub(crate) struct RepositorySyncResult {
    pub(crate) transfer: RepositoryTransferSummary,
    pub(crate) conflicts: Vec<RepositoryConflictRecord>,
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

#[derive(Clone)]
pub(crate) struct DejavuSyncService {
    inner: Arc<DejavuSyncServiceInner>,
}

struct DejavuSyncServiceInner {
    runner: Arc<dyn RepositoryJobRunner>,
    status_sink: Arc<dyn RepositoryStatusSink>,
    repository_locks: Mutex<HashMap<String, Arc<AsyncMutex<()>>>>,
    global_gate: Arc<RwLock<()>>,
    jobs: Mutex<HashMap<String, JobCancellationToken>>,
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
                jobs: Mutex::new(HashMap::new()),
            }),
        }
    }

    pub(crate) async fn enqueue(
        &self,
        request: SyncJobRequest,
    ) -> Result<AcceptedSyncJob, RepositoryJobError> {
        self.enqueue_inner(request, true).await
    }

    fn enqueue_inner<'a>(
        &'a self,
        request: SyncJobRequest,
        allow_follow_up: bool,
    ) -> BoxFuture<'a, Result<AcceptedSyncJob, RepositoryJobError>> {
        Box::pin(async move {
            let request = self.inner.runner.validate(request)?;
            let job_id = uuid::Uuid::new_v4().to_string();
            let accepted = AcceptedSyncJob {
                job_id: job_id.clone(),
                repository_id: request.repository_id.clone(),
                notes_root: request.notes_root.clone(),
            };
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
            self.inner
                .jobs
                .lock()
                .unwrap()
                .insert(job_id.clone(), cancellation.clone());
            let service = self.clone();
            tokio::spawn(async move {
                service
                    .run_owned_job(
                        request,
                        job_id,
                        cancellation,
                        allow_follow_up,
                        ordinary_guard,
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
        ordinary_guard: OwnedRwLockReadGuard<()>,
    ) {
        let repository_lock = self.repository_lock(&request.repository_id);
        let _repository_guard = repository_lock.lock().await;
        let mut result = Err(RepositoryJobError::Cancelled);
        let mut final_attempt = 1;
        for attempt in 1..=MAX_WORKING_TREE_ATTEMPTS {
            final_attempt = attempt;
            if cancellation.is_cancelled() {
                result = Err(RepositoryJobError::Cancelled);
                break;
            }
            if attempt > 1 {
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
            if !matches!(result, Err(RepositoryJobError::WorkingTreeChanged)) {
                break;
            }
        }
        let needs_follow_up = allow_follow_up
            && matches!(result, Err(RepositoryJobError::WorkingTreeChanged))
            && !cancellation.is_cancelled();
        let completed_at = sync_status_timestamp();
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
        let _status_result = self.inner.status_sink.publish(final_status).await;
        self.inner.jobs.lock().unwrap().remove(&job_id);
        drop(_repository_guard);
        drop(ordinary_guard);

        if needs_follow_up {
            let service = self.clone();
            tokio::spawn(async move {
                let _accepted = service.enqueue_inner(request, false).await;
            });
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
    pub(crate) fn cancel_all_for_shutdown_or_reset(&self) {
        for cancellation in self.inner.jobs.lock().unwrap().values() {
            cancellation.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::future::Future;
    use std::path::PathBuf;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tokio::sync::{mpsc, oneshot, Notify, Semaphore};

    use super::{
        AcceptedSyncJob, DejavuSyncService, RepositoryJobError, RepositoryJobRunner,
        RepositoryStatusSink, RepositorySyncResult, SyncAttemptContext, SyncJobRequest,
    };
    use crate::dejavu_sync::status::{RepositorySyncPhase, RepositorySyncStatus};
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
    async fn shutdown_or_reset_token_cancels_queued_jobs_without_caller_ownership() {
        let (started_tx, mut started_rx) = mpsc::unbounded_channel();
        let runner = Arc::new(ControlledRunner::new(started_tx));
        let sink = Arc::new(MemoryStatusSink::default());
        let service = service(Arc::clone(&runner), Arc::clone(&sink));
        let repository = "00000000-0000-4000-8000-000000000009";

        service.enqueue(request(repository)).await.unwrap();
        service.enqueue(request(repository)).await.unwrap();
        assert_eq!(receive_start(&mut started_rx).await, repository);
        service.cancel_all_for_shutdown_or_reset();
        runner.release(1);

        sink.wait_for_phase(RepositorySyncPhase::Succeeded, 1).await;
        sink.wait_for_phase(RepositorySyncPhase::Failed, 1).await;
        assert!(
            tokio::time::timeout(Duration::from_millis(100), started_rx.recv())
                .await
                .is_err()
        );
    }
}
