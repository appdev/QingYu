use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use super::scheduler::DejavuScheduler;
use super::service::{AcceptedSyncJob, DejavuSyncService, RepositoryJobError, SyncJobRequest};
use tauri::{Manager, Runtime};

#[derive(Default)]
pub(crate) struct DejavuSyncServiceOwner {
    service: OnceLock<DejavuSyncService>,
}

#[derive(Default)]
pub(crate) struct DejavuSchedulerOwner {
    scheduler: OnceLock<DejavuScheduler>,
    startup_pending: AtomicBool,
    active_root: Mutex<Option<PathBuf>>,
    native_exit_state: Arc<Mutex<NativeExitState>>,
}

#[derive(Default)]
enum NativeExitState {
    #[default]
    Idle,
    Waiting,
    BypassReady,
}

pub(crate) enum NativeExitAction {
    Allow,
    Prevent,
    Wait(Pin<Box<dyn Future<Output = Result<(), RepositoryJobError>> + Send + 'static>>),
}

pub(crate) fn handle_native_sync_exit<R: Runtime>(
    app: &tauri::AppHandle<R>,
    code: Option<i32>,
    api: tauri::ExitRequestApi,
) {
    let action = app
        .try_state::<DejavuSchedulerOwner>()
        .map_or(NativeExitAction::Allow, |owner| owner.begin_native_exit());
    match action {
        NativeExitAction::Allow => {}
        NativeExitAction::Prevent => api.prevent_exit(),
        NativeExitAction::Wait(wait) => {
            api.prevent_exit();
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = wait.await {
                    eprintln!("QingYu exit synchronization failed: {error}");
                    return;
                }
                app.exit(code.unwrap_or(0));
            });
        }
    }
}

impl DejavuSchedulerOwner {
    #[allow(dead_code)]
    pub(crate) fn install(&self, scheduler: DejavuScheduler) -> Result<(), RepositoryJobError> {
        self.scheduler
            .set(scheduler)
            .map_err(|_| RepositoryJobError::RepositoryUnavailable)
    }

    pub(crate) fn activate_root(&self, root: &Path) -> bool {
        let activated = self
            .scheduler
            .get()
            .is_some_and(|scheduler| scheduler.activate_root(root).unwrap_or(false));
        if activated {
            *self.active_root.lock().unwrap() = Some(root.to_path_buf());
            self.consume_pending_startup();
        }
        activated
    }

    pub(crate) fn deactivate_root(&self, root: &Path) -> bool {
        let deactivated = self
            .scheduler
            .get()
            .is_some_and(|scheduler| scheduler.deactivate_root(root));
        if deactivated {
            let mut active_root = self.active_root.lock().unwrap();
            if active_root.as_ref().is_some_and(|active| active == root) {
                *active_root = None;
            }
        }
        deactivated
    }

    pub(crate) fn record_file_change(&self, root: &Path, path: &Path) -> bool {
        self.scheduler
            .get()
            .is_some_and(|scheduler| scheduler.record_file_change(root, path).unwrap_or(false))
    }

    pub(crate) fn trigger_startup(&self) {
        self.startup_pending.store(true, Ordering::Release);
        self.consume_pending_startup();
    }

    fn consume_pending_startup(&self) {
        if self.active_root.lock().unwrap().is_none()
            || !self.startup_pending.swap(false, Ordering::AcqRel)
        {
            return;
        }
        let Some(scheduler) = self.scheduler.get().cloned() else {
            self.startup_pending.store(true, Ordering::Release);
            return;
        };
        tauri::async_runtime::spawn(async move {
            let _accepted = scheduler.trigger_startup().await;
        });
    }

    pub(crate) fn begin_native_exit(&self) -> NativeExitAction {
        let Some(scheduler) = self.scheduler.get().cloned() else {
            return NativeExitAction::Allow;
        };
        {
            let mut state = self.native_exit_state.lock().unwrap();
            match *state {
                NativeExitState::Idle => *state = NativeExitState::Waiting,
                NativeExitState::Waiting => return NativeExitAction::Prevent,
                NativeExitState::BypassReady => {
                    *state = NativeExitState::Idle;
                    return NativeExitAction::Allow;
                }
            }
        }
        let state = Arc::clone(&self.native_exit_state);
        NativeExitAction::Wait(Box::pin(async move {
            let result = match scheduler.trigger_exit_job().await {
                Ok(Some(accepted)) => accepted.wait_for_completion().await,
                Ok(None) => Ok(()),
                Err(error) => Err(error),
            };
            *state.lock().unwrap() = if result.is_ok() {
                NativeExitState::BypassReady
            } else {
                NativeExitState::Idle
            };
            result
        }))
    }
}

impl DejavuSyncServiceOwner {
    #[allow(dead_code)]
    pub(crate) fn install(&self, service: DejavuSyncService) -> Result<(), RepositoryJobError> {
        self.service
            .set(service)
            .map_err(|_| RepositoryJobError::RepositoryUnavailable)
    }

    #[allow(dead_code)]
    pub(crate) async fn enqueue(
        &self,
        request: SyncJobRequest,
    ) -> Result<AcceptedSyncJob, RepositoryJobError> {
        self.service
            .get()
            .ok_or(RepositoryJobError::RepositoryUnavailable)?
            .enqueue(request)
            .await
    }

    #[allow(dead_code)]
    pub(crate) async fn cancel_all_for_shutdown_or_reset(&self) {
        if let Some(service) = self.service.get() {
            service.cancel_all_for_shutdown_or_reset().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::future::Future;
    use std::path::{Path, PathBuf};
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use time::OffsetDateTime;
    use tokio::sync::{mpsc, watch};

    use super::{DejavuSchedulerOwner, NativeExitAction};
    use crate::dejavu_sync::scheduler::{
        ActiveRepositorySchedule, DejavuScheduler, DnsFlusher, RepositoryScheduleSource,
        RepositoryScheduleStore, SchedulerJobEnqueuer,
    };
    use crate::dejavu_sync::service::{AcceptedSyncJob, RepositoryJobError, SyncJobRequest};
    use crate::dejavu_sync::status::RepositorySchedule;
    use crate::sync_config::model::SyncMode;
    use crate::sync_config::status::SyncTrigger;

    type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

    struct OwnerSource(ActiveRepositorySchedule);

    impl RepositoryScheduleSource for OwnerSource {
        fn resolve_active_root(
            &self,
            root: &Path,
        ) -> Result<Option<ActiveRepositorySchedule>, RepositoryJobError> {
            Ok((self.0.notes_root == root).then(|| self.0.clone()))
        }
    }

    #[derive(Default)]
    struct OwnerStore(Mutex<HashMap<String, RepositorySchedule>>);

    impl RepositoryScheduleStore for OwnerStore {
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
            update(schedule);
            Ok(schedule.clone())
        }

        fn reserve_dns_retry(
            &self,
            _repository_id: &str,
            _now: OffsetDateTime,
            _throttle: Duration,
        ) -> Result<bool, RepositoryJobError> {
            Ok(true)
        }
    }

    struct OwnerEnqueuer(mpsc::UnboundedSender<SyncJobRequest>);

    impl SchedulerJobEnqueuer for OwnerEnqueuer {
        fn enqueue<'a>(
            &'a self,
            request: SyncJobRequest,
        ) -> BoxFuture<'a, Result<AcceptedSyncJob, RepositoryJobError>> {
            Box::pin(async move {
                self.0.send(request.clone()).unwrap();
                Ok(AcceptedSyncJob::completed_for_test(
                    "00000000-0000-4000-8000-000000000097",
                    request.repository_id,
                    request.notes_root,
                ))
            })
        }
    }

    struct OwnerFlusher;

    impl DnsFlusher for OwnerFlusher {
        fn flush(&self) {}
    }

    fn scheduler_fixture() -> (
        DejavuScheduler,
        PathBuf,
        mpsc::UnboundedReceiver<SyncJobRequest>,
    ) {
        let root = PathBuf::from("/notes/pending-startup");
        let repository_id = "00000000-0000-4000-8000-000000000040".to_owned();
        let source = Arc::new(OwnerSource(ActiveRepositorySchedule {
            notes_root: root.clone(),
            repository_id,
            mode: SyncMode::Automatic,
            interval: Duration::from_secs(30),
        }));
        let (sent, jobs) = mpsc::unbounded_channel();
        let scheduler = DejavuScheduler::new(
            source,
            Arc::new(OwnerEnqueuer(sent)),
            Arc::new(OwnerStore::default()),
            Arc::new(OwnerFlusher),
            Arc::new(|| OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap()),
        );
        (scheduler, root, jobs)
    }

    struct PendingOwnerEnqueuer(
        mpsc::UnboundedSender<(
            SyncJobRequest,
            watch::Sender<Option<Result<(), RepositoryJobError>>>,
        )>,
    );

    impl SchedulerJobEnqueuer for PendingOwnerEnqueuer {
        fn enqueue<'a>(
            &'a self,
            request: SyncJobRequest,
        ) -> BoxFuture<'a, Result<AcceptedSyncJob, RepositoryJobError>> {
            Box::pin(async move {
                let (accepted, completion) = AcceptedSyncJob::pending_for_test(
                    "00000000-0000-4000-8000-000000000096",
                    request.repository_id.clone(),
                    request.notes_root.clone(),
                );
                self.0.send((request, completion)).unwrap();
                Ok(accepted)
            })
        }
    }

    fn pending_exit_scheduler_fixture() -> (
        DejavuScheduler,
        PathBuf,
        mpsc::UnboundedReceiver<(
            SyncJobRequest,
            watch::Sender<Option<Result<(), RepositoryJobError>>>,
        )>,
    ) {
        let root = PathBuf::from("/notes/pending-exit");
        let repository_id = "00000000-0000-4000-8000-000000000042".to_owned();
        let source = Arc::new(OwnerSource(ActiveRepositorySchedule {
            notes_root: root.clone(),
            repository_id,
            mode: SyncMode::StartupExit,
            interval: Duration::from_secs(30),
        }));
        let (sent, jobs) = mpsc::unbounded_channel();
        let scheduler = DejavuScheduler::new(
            source,
            Arc::new(PendingOwnerEnqueuer(sent)),
            Arc::new(OwnerStore::default()),
            Arc::new(OwnerFlusher),
            Arc::new(|| OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap()),
        );
        (scheduler, root, jobs)
    }

    #[test]
    fn uninstalled_scheduler_owner_is_safe_for_all_native_call_sites() {
        let owner = DejavuSchedulerOwner::default();
        assert!(!owner.activate_root(Path::new("/notes/uninstalled")));
        assert!(!owner.record_file_change(
            Path::new("/notes/uninstalled"),
            Path::new("/notes/uninstalled/note.md"),
        ));
        assert!(!owner.deactivate_root(Path::new("/notes/uninstalled")));
        owner.trigger_startup();
        assert!(matches!(owner.begin_native_exit(), NativeExitAction::Allow));
    }

    #[tokio::test(start_paused = true)]
    async fn startup_before_install_is_consumed_once_by_the_first_valid_activation() {
        let owner = DejavuSchedulerOwner::default();
        owner.trigger_startup();
        let (scheduler, root, mut jobs) = scheduler_fixture();
        owner.install(scheduler).unwrap();

        assert!(owner.activate_root(&root));
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        assert_eq!(jobs.try_recv().unwrap().trigger, SyncTrigger::AppLaunch);

        assert!(owner.activate_root(&root));
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        assert!(jobs.try_recv().is_err());
    }

    #[tokio::test]
    async fn native_exit_waits_once_then_allows_one_bypassed_exit() {
        let owner = DejavuSchedulerOwner::default();
        let (scheduler, root, mut jobs) = pending_exit_scheduler_fixture();
        owner.install(scheduler).unwrap();
        assert!(owner.activate_root(&root));

        let wait = match owner.begin_native_exit() {
            NativeExitAction::Wait(wait) => wait,
            _ => panic!("first native exit should wait for the accepted sync"),
        };
        assert!(matches!(
            owner.begin_native_exit(),
            NativeExitAction::Prevent
        ));
        let mut waiting = tokio::spawn(wait);
        let (request, completion) = jobs.recv().await.unwrap();
        assert_eq!(request.trigger, SyncTrigger::SettingsExit);
        assert!(
            tokio::time::timeout(Duration::from_millis(100), &mut waiting)
                .await
                .is_err()
        );

        completion.send(Some(Ok(()))).unwrap();
        assert_eq!(waiting.await.unwrap(), Ok(()));
        assert!(matches!(owner.begin_native_exit(), NativeExitAction::Allow));
        assert!(matches!(
            owner.begin_native_exit(),
            NativeExitAction::Wait(_)
        ));
    }

    #[tokio::test]
    async fn failed_native_exit_sync_resets_the_barrier_for_a_retry() {
        let owner = DejavuSchedulerOwner::default();
        let (scheduler, root, mut jobs) = pending_exit_scheduler_fixture();
        owner.install(scheduler).unwrap();
        assert!(owner.activate_root(&root));

        let wait = match owner.begin_native_exit() {
            NativeExitAction::Wait(wait) => wait,
            _ => panic!("first native exit should wait for sync"),
        };
        let waiting = tokio::spawn(wait);
        let (_request, completion) = jobs.recv().await.unwrap();
        completion
            .send(Some(Err(RepositoryJobError::CloudUnavailable)))
            .unwrap();
        assert_eq!(
            waiting.await.unwrap(),
            Err(RepositoryJobError::CloudUnavailable)
        );
        assert!(matches!(
            owner.begin_native_exit(),
            NativeExitAction::Wait(_)
        ));
    }
}
