use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use qingyu_dejavu::RepositoryMetadata;
use serde::Deserialize;

use super::local_state::{LocalSyncStateService, RepositoryBinding};
use super::repository::{prepare_binding_root, RepositoryCatalogValidator};
use super::scheduler::DejavuScheduler;
use super::service::{AcceptedSyncJob, DejavuSyncService, RepositoryJobError, SyncJobRequest};
use crate::sync_config::status::SyncTrigger;
use tauri::{Manager, Runtime};

#[derive(Default)]
pub(crate) struct DejavuSyncServiceOwner {
    service: OnceLock<DejavuSyncService>,
    binding: OnceLock<BindingController>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BindRepositoryRequest {
    pub(crate) notes_root: PathBuf,
    pub(crate) repository_id: String,
    pub(crate) display_name: String,
}

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub(crate) trait BindJobEnqueuer: Send + Sync {
    fn enqueue_bind_and_sync<'a>(
        &'a self,
        request: SyncJobRequest,
    ) -> BoxFuture<'a, Result<AcceptedSyncJob, RepositoryJobError>>;
}

struct BindingController {
    app_data: PathBuf,
    catalog: Arc<dyn RepositoryCatalogValidator>,
    enqueuer: Arc<dyn BindJobEnqueuer>,
    transaction: tokio::sync::Mutex<()>,
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

    pub(crate) fn install_binding<Validator, Enqueuer>(
        &self,
        app_data: impl AsRef<Path>,
        catalog: Arc<Validator>,
        enqueuer: Arc<Enqueuer>,
    ) -> Result<(), RepositoryJobError>
    where
        Validator: RepositoryCatalogValidator + 'static,
        Enqueuer: BindJobEnqueuer + 'static,
    {
        let catalog: Arc<dyn RepositoryCatalogValidator> = catalog;
        let enqueuer: Arc<dyn BindJobEnqueuer> = enqueuer;
        self.binding
            .set(BindingController {
                app_data: app_data.as_ref().to_path_buf(),
                catalog,
                enqueuer,
                transaction: tokio::sync::Mutex::new(()),
            })
            .map_err(|_| RepositoryJobError::RepositoryUnavailable)
    }

    pub(crate) async fn bind_repository(
        &self,
        request: BindRepositoryRequest,
    ) -> Result<AcceptedSyncJob, RepositoryJobError> {
        let controller = self
            .binding
            .get()
            .ok_or(RepositoryJobError::RepositoryUnavailable)?;
        let _transaction = controller.transaction.lock().await;
        validate_repository_id(&request.repository_id)?;
        let metadata = controller
            .catalog
            .read_repository(&request.repository_id)
            .await?;
        validate_selected_metadata(&request, &metadata)?;
        let notes_root = prepare_binding_root(&request.notes_root)?;
        let state_service = LocalSyncStateService::new(&controller.app_data);
        let mut state = state_service
            .load_or_initialize(None)
            .map_err(|_| RepositoryJobError::InvalidBinding)?;
        state_service
            .bind_repository(
                &mut state,
                RepositoryBinding {
                    repository_id: metadata.repository_id.clone(),
                    display_name: metadata.display_name,
                    notes_root: notes_root.clone(),
                    enabled: true,
                },
            )
            .map_err(|_| RepositoryJobError::InvalidBinding)?;
        controller
            .enqueuer
            .enqueue_bind_and_sync(SyncJobRequest {
                notes_root,
                repository_id: metadata.repository_id,
                trigger: SyncTrigger::Manual,
            })
            .await
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

fn validate_selected_metadata(
    request: &BindRepositoryRequest,
    metadata: &RepositoryMetadata,
) -> Result<(), RepositoryJobError> {
    if metadata.repository_id != request.repository_id
        || metadata.display_name != request.display_name
    {
        return Err(RepositoryJobError::InvalidBinding);
    }
    Ok(())
}

fn validate_repository_id(repository_id: &str) -> Result<(), RepositoryJobError> {
    let parsed =
        uuid::Uuid::parse_str(repository_id).map_err(|_| RepositoryJobError::InvalidBinding)?;
    if parsed.to_string() != repository_id {
        return Err(RepositoryJobError::InvalidBinding);
    }
    Ok(())
}

impl BindJobEnqueuer for DejavuSyncService {
    fn enqueue_bind_and_sync<'a>(
        &'a self,
        request: SyncJobRequest,
    ) -> BoxFuture<'a, Result<AcceptedSyncJob, RepositoryJobError>> {
        Box::pin(async move { self.enqueue(request).await })
    }
}

#[tauri::command]
pub(crate) async fn bind_dejavu_repository(
    owner: tauri::State<'_, DejavuSyncServiceOwner>,
    request: BindRepositoryRequest,
) -> Result<AcceptedSyncJob, String> {
    owner
        .bind_repository(request)
        .await
        .map_err(|error| error.safe_code().to_owned())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::future::Future;
    use std::path::{Path, PathBuf};
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use qingyu_dejavu::RepositoryMetadata;
    use tempfile::tempdir;
    use time::OffsetDateTime;
    use tokio::sync::{mpsc, watch};

    use super::{
        BindJobEnqueuer, BindRepositoryRequest, DejavuSchedulerOwner, DejavuSyncServiceOwner,
        NativeExitAction,
    };
    use crate::dejavu_sync::local_state::LocalSyncStateService;
    use crate::dejavu_sync::repository::RepositoryCatalogValidator;
    use crate::dejavu_sync::scheduler::{
        ActiveRepositorySchedule, DejavuScheduler, DnsFlusher, RepositoryScheduleSource,
        RepositoryScheduleStore, SchedulerJobEnqueuer,
    };
    use crate::dejavu_sync::service::{
        AcceptedSyncJob, DejavuSyncService, RepositoryJobError, RepositoryJobRunner,
        RepositoryStatusSink, RepositorySyncResult, SyncAttemptContext, SyncJobRequest,
    };
    use crate::dejavu_sync::status::{RepositorySchedule, RepositorySyncStatus};
    use crate::sync_config::model::SyncMode;
    use crate::sync_config::status::SyncTrigger;

    type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

    struct FakeCatalogValidator {
        metadata: HashMap<String, RepositoryMetadata>,
        calls: Mutex<Vec<String>>,
        active: AtomicUsize,
        max_active: AtomicUsize,
        delay: Duration,
    }

    impl FakeCatalogValidator {
        fn new(metadata: impl IntoIterator<Item = RepositoryMetadata>) -> Self {
            Self {
                metadata: metadata
                    .into_iter()
                    .map(|metadata| (metadata.repository_id.clone(), metadata))
                    .collect(),
                calls: Mutex::new(Vec::new()),
                active: AtomicUsize::new(0),
                max_active: AtomicUsize::new(0),
                delay: Duration::ZERO,
            }
        }

        fn with_delay(mut self, delay: Duration) -> Self {
            self.delay = delay;
            self
        }
    }

    impl RepositoryCatalogValidator for FakeCatalogValidator {
        fn read_repository<'a>(
            &'a self,
            repository_id: &'a str,
        ) -> BoxFuture<'a, Result<RepositoryMetadata, RepositoryJobError>> {
            Box::pin(async move {
                self.calls.lock().unwrap().push(repository_id.to_owned());
                let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
                self.max_active.fetch_max(active, Ordering::SeqCst);
                if !self.delay.is_zero() {
                    tokio::time::sleep(self.delay).await;
                }
                self.active.fetch_sub(1, Ordering::SeqCst);
                self.metadata
                    .get(repository_id)
                    .cloned()
                    .ok_or(RepositoryJobError::CloudUnavailable)
            })
        }
    }

    struct PersistedBindingRunner {
        app_data: PathBuf,
        attempts: AtomicUsize,
    }

    impl PersistedBindingRunner {
        fn new(app_data: PathBuf) -> Self {
            Self {
                app_data,
                attempts: AtomicUsize::new(0),
            }
        }
    }

    impl RepositoryJobRunner for PersistedBindingRunner {
        fn validate(&self, request: SyncJobRequest) -> Result<SyncJobRequest, RepositoryJobError> {
            let state = LocalSyncStateService::new(&self.app_data)
                .load()
                .map_err(|_| RepositoryJobError::InvalidBinding)?
                .ok_or(RepositoryJobError::InvalidBinding)?;
            if state.bindings.iter().any(|binding| {
                binding.enabled
                    && binding.repository_id == request.repository_id
                    && binding.notes_root == request.notes_root
            }) {
                Ok(request)
            } else {
                Err(RepositoryJobError::InvalidBinding)
            }
        }

        fn run_attempt<'a>(
            &'a self,
            context: SyncAttemptContext,
        ) -> BoxFuture<'a, Result<RepositorySyncResult, RepositoryJobError>> {
            Box::pin(async move {
                self.validate(context.request)?;
                self.attempts.fetch_add(1, Ordering::SeqCst);
                Ok(RepositorySyncResult::default())
            })
        }
    }

    struct NoopStatusSink;

    impl RepositoryStatusSink for NoopStatusSink {
        fn publish<'a>(
            &'a self,
            _status: RepositorySyncStatus,
        ) -> BoxFuture<'a, Result<(), RepositoryJobError>> {
            Box::pin(async { Ok(()) })
        }
    }

    struct RecordingBindEnqueuer {
        app_data: PathBuf,
        requests: Mutex<Vec<SyncJobRequest>>,
        completions: Mutex<Vec<watch::Sender<Option<Result<(), RepositoryJobError>>>>>,
    }

    impl RecordingBindEnqueuer {
        fn new(app_data: PathBuf) -> Self {
            Self {
                app_data,
                requests: Mutex::new(Vec::new()),
                completions: Mutex::new(Vec::new()),
            }
        }
    }

    impl BindJobEnqueuer for RecordingBindEnqueuer {
        fn enqueue_bind_and_sync<'a>(
            &'a self,
            request: SyncJobRequest,
        ) -> BoxFuture<'a, Result<AcceptedSyncJob, RepositoryJobError>> {
            Box::pin(async move {
                let state = LocalSyncStateService::new(&self.app_data)
                    .load()
                    .map_err(|_| RepositoryJobError::InvalidBinding)?
                    .ok_or(RepositoryJobError::InvalidBinding)?;
                if !state.bindings.iter().any(|binding| {
                    binding.enabled
                        && binding.repository_id == request.repository_id
                        && binding.notes_root == request.notes_root
                }) || !request.notes_root.join(".qingyu/syncignore").is_file()
                {
                    return Err(RepositoryJobError::InvalidBinding);
                }
                self.requests.lock().unwrap().push(request.clone());
                let (accepted, completion) = AcceptedSyncJob::pending_for_test(
                    "00000000-0000-4000-8000-000000000095",
                    request.repository_id,
                    request.notes_root,
                );
                self.completions.lock().unwrap().push(completion);
                Ok(accepted)
            })
        }
    }

    fn repository_metadata(repository_id: &str, display_name: &str) -> RepositoryMetadata {
        RepositoryMetadata {
            format_version: 1,
            repository_id: repository_id.to_owned(),
            display_name: display_name.to_owned(),
            created_at: 1_800_000_000,
            updated_at: 1_800_000_000,
        }
    }

    fn bind_request(
        notes_root: PathBuf,
        repository_id: &str,
        display_name: &str,
    ) -> BindRepositoryRequest {
        BindRepositoryRequest {
            notes_root,
            repository_id: repository_id.to_owned(),
            display_name: display_name.to_owned(),
        }
    }

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

    #[tokio::test]
    async fn bind_commits_remote_metadata_and_syncignore_before_returning_a_pending_job() {
        let temporary = tempdir().unwrap();
        let app_data = temporary.path().join("app-data");
        let notes_root = temporary.path().join("notes");
        std::fs::create_dir(&notes_root).unwrap();
        let repository_id = "00000000-0000-4000-8000-000000000051";
        let catalog = Arc::new(FakeCatalogValidator::new([repository_metadata(
            repository_id,
            "Remote journal",
        )]));
        let enqueuer = Arc::new(RecordingBindEnqueuer::new(app_data.clone()));
        let owner = DejavuSyncServiceOwner::default();
        owner
            .install_binding(&app_data, Arc::clone(&catalog), Arc::clone(&enqueuer))
            .unwrap();

        let accepted = tokio::time::timeout(
            Duration::from_millis(250),
            owner.bind_repository(bind_request(
                notes_root.clone(),
                repository_id,
                "Remote journal",
            )),
        )
        .await
        .expect("bind should return after enqueue acceptance")
        .expect("valid existing metadata should bind");

        assert_eq!(accepted.repository_id, repository_id);
        assert!(
            tokio::time::timeout(Duration::from_millis(50), accepted.wait_for_completion())
                .await
                .is_err()
        );
        assert_eq!(
            std::fs::read(notes_root.join(".qingyu/syncignore")).unwrap(),
            b""
        );
        let state = LocalSyncStateService::new(&app_data)
            .load()
            .unwrap()
            .unwrap();
        assert_eq!(state.bindings.len(), 1);
        assert_eq!(state.bindings[0].repository_id, repository_id);
        assert_eq!(state.bindings[0].display_name, "Remote journal");
        assert!(state.bindings[0].enabled);
        assert_eq!(enqueuer.requests.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn bind_rejects_stale_display_metadata_without_local_side_effects() {
        let temporary = tempdir().unwrap();
        let app_data = temporary.path().join("app-data");
        let notes_root = temporary.path().join("notes");
        std::fs::create_dir(&notes_root).unwrap();
        let repository_id = "00000000-0000-4000-8000-000000000052";
        let catalog = Arc::new(FakeCatalogValidator::new([repository_metadata(
            repository_id,
            "Renamed remotely",
        )]));
        let enqueuer = Arc::new(RecordingBindEnqueuer::new(app_data.clone()));
        let owner = DejavuSyncServiceOwner::default();
        owner
            .install_binding(&app_data, catalog, Arc::clone(&enqueuer))
            .unwrap();

        let result = owner
            .bind_repository(bind_request(
                notes_root.clone(),
                repository_id,
                "Stale list name",
            ))
            .await;
        let Err(error) = result else {
            panic!("stale display metadata must be rejected");
        };

        assert_eq!(error, RepositoryJobError::InvalidBinding);
        assert!(!app_data.join("local-sync.json").exists());
        assert!(!notes_root.join(".qingyu").exists());
        assert!(enqueuer.requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn bind_rejects_a_noncanonical_repository_id_before_reading_remote_metadata() {
        let temporary = tempdir().unwrap();
        let app_data = temporary.path().join("app-data");
        let notes_root = temporary.path().join("notes");
        std::fs::create_dir(&notes_root).unwrap();
        let catalog = Arc::new(FakeCatalogValidator::new([]));
        let enqueuer = Arc::new(RecordingBindEnqueuer::new(app_data.clone()));
        let owner = DejavuSyncServiceOwner::default();
        owner
            .install_binding(&app_data, Arc::clone(&catalog), Arc::clone(&enqueuer))
            .unwrap();

        assert!(matches!(
            owner
                .bind_repository(bind_request(notes_root, "NOT-A-UUID", "Remote"))
                .await,
            Err(RepositoryJobError::InvalidBinding)
        ));
        assert!(catalog.calls.lock().unwrap().is_empty());
        assert!(!app_data.join("local-sync.json").exists());
    }

    #[tokio::test]
    async fn bind_exact_retry_reenables_but_both_reassignment_directions_are_rejected() {
        let temporary = tempdir().unwrap();
        let app_data = temporary.path().join("app-data");
        let notes_a = temporary.path().join("notes-a");
        let notes_b = temporary.path().join("notes-b");
        std::fs::create_dir(&notes_a).unwrap();
        std::fs::create_dir(&notes_b).unwrap();
        let repository_a = "00000000-0000-4000-8000-000000000053";
        let repository_b = "00000000-0000-4000-8000-000000000054";
        let catalog = Arc::new(FakeCatalogValidator::new([
            repository_metadata(repository_a, "Remote A"),
            repository_metadata(repository_b, "Remote B"),
        ]));
        let enqueuer = Arc::new(RecordingBindEnqueuer::new(app_data.clone()));
        let owner = DejavuSyncServiceOwner::default();
        owner
            .install_binding(&app_data, catalog, Arc::clone(&enqueuer))
            .unwrap();
        owner
            .bind_repository(bind_request(notes_a.clone(), repository_a, "Remote A"))
            .await
            .unwrap();
        let state_service = LocalSyncStateService::new(&app_data);
        let mut disabled = state_service.load().unwrap().unwrap();
        disabled.bindings[0].enabled = false;
        state_service.save(&disabled).unwrap();

        owner
            .bind_repository(bind_request(notes_a.clone(), repository_a, "Remote A"))
            .await
            .expect("exact retry should re-enable and enqueue");
        assert!(matches!(
            owner
                .bind_repository(bind_request(notes_b, repository_a, "Remote A"))
                .await,
            Err(RepositoryJobError::InvalidBinding)
        ));
        assert!(matches!(
            owner
                .bind_repository(bind_request(notes_a, repository_b, "Remote B"))
                .await,
            Err(RepositoryJobError::InvalidBinding)
        ));

        let state = state_service.load().unwrap().unwrap();
        assert_eq!(state.bindings.len(), 1);
        assert!(state.bindings[0].enabled);
        assert_eq!(enqueuer.requests.lock().unwrap().len(), 2);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn bind_serializes_distinct_transactions_without_losing_local_state_updates() {
        let temporary = tempdir().unwrap();
        let app_data = temporary.path().join("app-data");
        let notes_a = temporary.path().join("notes-a");
        let notes_b = temporary.path().join("notes-b");
        std::fs::create_dir(&notes_a).unwrap();
        std::fs::create_dir(&notes_b).unwrap();
        let repository_a = "00000000-0000-4000-8000-000000000055";
        let repository_b = "00000000-0000-4000-8000-000000000056";
        let catalog = Arc::new(
            FakeCatalogValidator::new([
                repository_metadata(repository_a, "Remote A"),
                repository_metadata(repository_b, "Remote B"),
            ])
            .with_delay(Duration::from_millis(50)),
        );
        let enqueuer = Arc::new(RecordingBindEnqueuer::new(app_data.clone()));
        let owner = Arc::new(DejavuSyncServiceOwner::default());
        owner
            .install_binding(&app_data, Arc::clone(&catalog), Arc::clone(&enqueuer))
            .unwrap();

        let first = {
            let owner = Arc::clone(&owner);
            tokio::spawn(async move {
                owner
                    .bind_repository(bind_request(notes_a, repository_a, "Remote A"))
                    .await
            })
        };
        let second = {
            let owner = Arc::clone(&owner);
            tokio::spawn(async move {
                owner
                    .bind_repository(bind_request(notes_b, repository_b, "Remote B"))
                    .await
            })
        };
        first.await.unwrap().unwrap();
        second.await.unwrap().unwrap();

        assert_eq!(catalog.max_active.load(Ordering::SeqCst), 1);
        let state = LocalSyncStateService::new(&app_data)
            .load()
            .unwrap()
            .unwrap();
        assert_eq!(state.bindings.len(), 2);
        assert_eq!(enqueuer.requests.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn restarted_manual_sync_uses_the_persisted_binding_without_catalog_access() {
        let temporary = tempdir().unwrap();
        let app_data = temporary.path().join("app-data");
        let notes_root = temporary.path().join("notes");
        std::fs::create_dir(&notes_root).unwrap();
        let repository_id = "00000000-0000-4000-8000-000000000057";
        let catalog = Arc::new(FakeCatalogValidator::new([repository_metadata(
            repository_id,
            "Restarted",
        )]));
        let runner = Arc::new(PersistedBindingRunner::new(app_data.clone()));
        let first_service = DejavuSyncService::new(Arc::clone(&runner), Arc::new(NoopStatusSink));
        let owner = DejavuSyncServiceOwner::default();
        owner
            .install_binding(
                &app_data,
                Arc::clone(&catalog),
                Arc::new(first_service.clone()),
            )
            .unwrap();

        let accepted = owner
            .bind_repository(bind_request(notes_root, repository_id, "Restarted"))
            .await
            .unwrap();
        assert_eq!(accepted.wait_for_completion().await, Ok(()));
        drop(owner);
        drop(first_service);

        let persisted = LocalSyncStateService::new(&app_data)
            .load()
            .unwrap()
            .unwrap()
            .bindings
            .into_iter()
            .find(|binding| binding.repository_id == repository_id)
            .unwrap();
        let restarted = DejavuSyncService::new(Arc::clone(&runner), Arc::new(NoopStatusSink));
        let accepted = restarted
            .enqueue(SyncJobRequest {
                notes_root: persisted.notes_root,
                repository_id: persisted.repository_id,
                trigger: SyncTrigger::Manual,
            })
            .await
            .unwrap();
        assert_eq!(accepted.wait_for_completion().await, Ok(()));

        assert_eq!(runner.attempts.load(Ordering::SeqCst), 2);
        assert_eq!(catalog.calls.lock().unwrap().as_slice(), [repository_id]);
    }
}
