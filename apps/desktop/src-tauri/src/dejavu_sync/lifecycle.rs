use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use serde::Serialize;
use tokio::sync::watch;

use super::local_state::LocalSyncStateService;
use super::maintenance::LocalMaintenanceController;
use super::repository::DejavuRepositoryMaintenance;
use super::service::{DejavuSyncService, RepositoryJobError};
use super::status::RepositoryStatusStore;

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum RepositoryLifecycleOperation {
    RebuildLocalRepository,
    StopRepositorySync,
    ChangeGlobalKey,
    PurgeRemoteRepository,
    DeleteRemoteRepository,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AcceptedMaintenanceJob {
    pub(crate) job_id: String,
    pub(crate) operation: RepositoryLifecycleOperation,
    pub(crate) repository_id: Option<String>,
    #[serde(skip)]
    completion: watch::Receiver<Option<Result<(), RepositoryJobError>>>,
}

impl AcceptedMaintenanceJob {
    fn pending(
        operation: RepositoryLifecycleOperation,
        repository_id: Option<String>,
    ) -> (Self, watch::Sender<Option<Result<(), RepositoryJobError>>>) {
        let (completion_tx, completion) = watch::channel(None);
        (
            Self {
                job_id: uuid::Uuid::new_v4().to_string(),
                operation,
                repository_id,
                completion,
            },
            completion_tx,
        )
    }

    #[cfg_attr(not(test), allow(dead_code))]
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
}

pub(crate) trait RepositoryLifecycleOps: Send + Sync {
    fn rebuild_local_repository(&self, repository_id: &str) -> Result<(), RepositoryJobError>;

    fn clear_local_repository(&self, repository_id: &str) -> Result<(), RepositoryJobError>;

    fn purge_remote_repository<'a>(
        &'a self,
        repository_id: &'a str,
        cancelled: &'a AtomicBool,
    ) -> BoxFuture<'a, Result<(), RepositoryJobError>>;

    fn delete_remote_repository<'a>(
        &'a self,
        repository_id: &'a str,
    ) -> BoxFuture<'a, Result<(), RepositoryJobError>>;
}

impl RepositoryLifecycleOps for DejavuRepositoryMaintenance {
    fn rebuild_local_repository(&self, repository_id: &str) -> Result<(), RepositoryJobError> {
        DejavuRepositoryMaintenance::rebuild_local_repository(self, repository_id)
    }

    fn clear_local_repository(&self, repository_id: &str) -> Result<(), RepositoryJobError> {
        DejavuRepositoryMaintenance::clear_local_repository(self, repository_id)
    }

    fn purge_remote_repository<'a>(
        &'a self,
        repository_id: &'a str,
        cancelled: &'a AtomicBool,
    ) -> BoxFuture<'a, Result<(), RepositoryJobError>> {
        Box::pin(async move {
            DejavuRepositoryMaintenance::purge_remote_repository(self, repository_id, cancelled)
                .await
                .map(drop)
        })
    }

    fn delete_remote_repository<'a>(
        &'a self,
        repository_id: &'a str,
    ) -> BoxFuture<'a, Result<(), RepositoryJobError>> {
        Box::pin(async move {
            DejavuRepositoryMaintenance::delete_remote_repository(self, repository_id).await
        })
    }
}

pub(crate) trait RepositoryScheduleReset: Send + Sync {
    fn clear_sync_schedule(&self, repository_id: &str) -> Result<(), RepositoryJobError>;
}

impl RepositoryScheduleReset for RepositoryStatusStore {
    fn clear_sync_schedule(&self, repository_id: &str) -> Result<(), RepositoryJobError> {
        RepositoryStatusStore::clear_sync_schedule(self, repository_id).map(drop)
    }
}

pub(crate) trait RepositoryRootDeactivator: Send + Sync {
    fn deactivate_root(&self, root: &Path);
}

pub(crate) trait LocalMaintenanceSuspender: Send + Sync {
    fn suspend_all_and_wait(&self) -> BoxFuture<'_, ()>;
    fn resume(&self);
}

impl LocalMaintenanceSuspender for LocalMaintenanceController {
    fn suspend_all_and_wait(&self) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            LocalMaintenanceController::suspend_all_and_wait(self).await;
        })
    }

    fn resume(&self) {
        LocalMaintenanceController::resume(self);
    }
}

pub(crate) struct RepositoryLifecycleController {
    app_data: PathBuf,
    service: DejavuSyncService,
    operations: Arc<dyn RepositoryLifecycleOps>,
    schedules: Arc<dyn RepositoryScheduleReset>,
    roots: Arc<dyn RepositoryRootDeactivator>,
    local_maintenance: Arc<dyn LocalMaintenanceSuspender>,
}

impl RepositoryLifecycleController {
    pub(crate) fn new<Operations, Schedules, Roots, LocalMaintenance>(
        app_data: impl AsRef<Path>,
        service: DejavuSyncService,
        operations: Arc<Operations>,
        schedules: Arc<Schedules>,
        roots: Arc<Roots>,
        local_maintenance: Arc<LocalMaintenance>,
    ) -> Self
    where
        Operations: RepositoryLifecycleOps + 'static,
        Schedules: RepositoryScheduleReset + 'static,
        Roots: RepositoryRootDeactivator + 'static,
        LocalMaintenance: LocalMaintenanceSuspender + 'static,
    {
        Self {
            app_data: app_data.as_ref().to_path_buf(),
            service,
            operations,
            schedules,
            roots,
            local_maintenance,
        }
    }

    pub(crate) fn rebuild_local_repository(
        &self,
        repository_id: &str,
        confirmed: bool,
    ) -> Result<AcceptedMaintenanceJob, RepositoryJobError> {
        require_confirmation(confirmed)?;
        let repository_id = canonical_repository_id(repository_id)?;
        let reservation = self
            .service
            .reserve_repository_maintenance(&repository_id)?;
        let operations = Arc::clone(&self.operations);
        let background_repository_id = repository_id.clone();
        Ok(spawn_accepted(
            RepositoryLifecycleOperation::RebuildLocalRepository,
            Some(repository_id),
            async move {
                reservation
                    .run(async move {
                        tokio::task::spawn_blocking(move || {
                            operations.rebuild_local_repository(&background_repository_id)
                        })
                        .await
                        .map_err(|_| RepositoryJobError::RepositoryUnavailable)?
                    })
                    .await
            },
        ))
    }

    pub(crate) fn stop_repository_sync(
        &self,
        repository_id: &str,
        confirmed: bool,
    ) -> Result<AcceptedMaintenanceJob, RepositoryJobError> {
        require_confirmation(confirmed)?;
        let repository_id = canonical_repository_id(repository_id)?;
        let reservation = self
            .service
            .reserve_repository_maintenance(&repository_id)?;
        let app_data = self.app_data.clone();
        let schedules = Arc::clone(&self.schedules);
        let roots = Arc::clone(&self.roots);
        let background_repository_id = repository_id.clone();
        Ok(spawn_accepted(
            RepositoryLifecycleOperation::StopRepositorySync,
            Some(repository_id),
            async move {
                let root = reservation
                    .run_state(async move {
                        let state_service = LocalSyncStateService::new(app_data);
                        let mut state = state_service
                            .load()
                            .map_err(RepositoryJobError::from)?
                            .ok_or(RepositoryJobError::InvalidBinding)?;
                        let root = state
                            .bindings
                            .iter()
                            .find(|binding| binding.repository_id == background_repository_id)
                            .map(|binding| binding.notes_root.clone())
                            .ok_or(RepositoryJobError::InvalidBinding)?;
                        schedules.clear_sync_schedule(&background_repository_id)?;
                        state_service
                            .remove_repository_binding(&mut state, &background_repository_id)
                            .map_err(RepositoryJobError::from)?;
                        Ok::<_, RepositoryJobError>(root)
                    })
                    .await?;
                roots.deactivate_root(&root);
                Ok(())
            },
        ))
    }

    pub(crate) fn change_global_key(
        &self,
        user_key_input: String,
        confirmed: bool,
    ) -> Result<AcceptedMaintenanceJob, RepositoryJobError> {
        require_confirmation(confirmed)?;
        let reservation = self.service.reserve_global_maintenance()?;
        let app_data = self.app_data.clone();
        let operations = Arc::clone(&self.operations);
        let schedules = Arc::clone(&self.schedules);
        let roots = Arc::clone(&self.roots);
        let local_maintenance = Arc::clone(&self.local_maintenance);
        Ok(spawn_accepted(
            RepositoryLifecycleOperation::ChangeGlobalKey,
            None,
            async move {
                local_maintenance.suspend_all_and_wait().await;
                let result = reservation
                    .run_state(async move {
                        let state_service = LocalSyncStateService::new(&app_data);
                        let mut state = state_service
                            .load()
                            .map_err(RepositoryJobError::from)?
                            .ok_or(RepositoryJobError::InvalidBinding)?;
                        let repository_ids = state
                            .bindings
                            .iter()
                            .map(|binding| binding.repository_id.clone())
                            .collect::<Vec<_>>();
                        let note_roots = state
                            .bindings
                            .iter()
                            .map(|binding| binding.notes_root.clone())
                            .collect::<Vec<_>>();
                        for repository_id in &repository_ids {
                            schedules.clear_sync_schedule(repository_id)?;
                        }
                        tokio::task::spawn_blocking(move || {
                            for repository_id in repository_ids {
                                operations.clear_local_repository(&repository_id)?;
                            }
                            Ok::<_, RepositoryJobError>(())
                        })
                        .await
                        .map_err(|_| RepositoryJobError::RepositoryUnavailable)??;
                        state_service
                            .replace_repository_key(&mut state, &user_key_input)
                            .map_err(RepositoryJobError::from)?;
                        Ok::<_, RepositoryJobError>(note_roots)
                    })
                    .await;
                local_maintenance.resume();
                let note_roots = result?;
                for root in note_roots {
                    roots.deactivate_root(&root);
                }
                Ok(())
            },
        ))
    }

    pub(crate) fn purge_remote_repository(
        &self,
        repository_id: &str,
        confirmed: bool,
    ) -> Result<AcceptedMaintenanceJob, RepositoryJobError> {
        require_confirmation(confirmed)?;
        let repository_id = canonical_repository_id(repository_id)?;
        let reservation = self
            .service
            .reserve_repository_maintenance(&repository_id)?;
        let operations = Arc::clone(&self.operations);
        let background_repository_id = repository_id.clone();
        Ok(spawn_accepted(
            RepositoryLifecycleOperation::PurgeRemoteRepository,
            Some(repository_id),
            async move {
                reservation
                    .run(async move {
                        operations
                            .purge_remote_repository(
                                &background_repository_id,
                                &AtomicBool::new(false),
                            )
                            .await
                    })
                    .await
            },
        ))
    }

    pub(crate) fn delete_remote_repository(
        &self,
        repository_id: &str,
        confirmed: bool,
    ) -> Result<AcceptedMaintenanceJob, RepositoryJobError> {
        require_confirmation(confirmed)?;
        let repository_id = canonical_repository_id(repository_id)?;
        let reservation = self
            .service
            .reserve_repository_maintenance(&repository_id)?;
        let app_data = self.app_data.clone();
        let operations = Arc::clone(&self.operations);
        let background_repository_id = repository_id.clone();
        Ok(spawn_accepted(
            RepositoryLifecycleOperation::DeleteRemoteRepository,
            Some(repository_id),
            async move {
                reservation
                    .run(async move {
                        let enabled = LocalSyncStateService::new(app_data)
                            .load()
                            .map_err(RepositoryJobError::from)?
                            .is_some_and(|state| {
                                state.bindings.iter().any(|binding| {
                                    binding.repository_id == background_repository_id
                                        && binding.enabled
                                })
                            });
                        if enabled {
                            return Err(RepositoryJobError::InvalidBinding);
                        }
                        operations
                            .delete_remote_repository(&background_repository_id)
                            .await
                    })
                    .await
            },
        ))
    }
}

fn spawn_accepted(
    operation: RepositoryLifecycleOperation,
    repository_id: Option<String>,
    future: impl Future<Output = Result<(), RepositoryJobError>> + Send + 'static,
) -> AcceptedMaintenanceJob {
    let (accepted, completion) = AcceptedMaintenanceJob::pending(operation, repository_id);
    tokio::spawn(async move {
        let _completion_result = completion.send(Some(future.await));
    });
    accepted
}

fn require_confirmation(confirmed: bool) -> Result<(), RepositoryJobError> {
    if confirmed {
        Ok(())
    } else {
        Err(RepositoryJobError::ConfirmationRequired)
    }
}

fn canonical_repository_id(repository_id: &str) -> Result<String, RepositoryJobError> {
    let parsed =
        uuid::Uuid::parse_str(repository_id).map_err(|_| RepositoryJobError::InvalidBinding)?;
    let canonical = parsed.to_string();
    if repository_id != canonical {
        return Err(RepositoryJobError::InvalidBinding);
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use tempfile::tempdir;
    use tokio::sync::Notify;

    use super::{
        BoxFuture, LocalMaintenanceSuspender, RepositoryLifecycleController,
        RepositoryLifecycleOperation, RepositoryLifecycleOps, RepositoryRootDeactivator,
        RepositoryScheduleReset,
    };
    use crate::dejavu_sync::local_state::{LocalSyncStateService, RepositoryBinding};
    use crate::dejavu_sync::service::{
        DejavuSyncService, RepositoryJobError, RepositoryJobRunner, RepositoryStatusSink,
        RepositorySyncResult, SyncAttemptContext, SyncJobRequest,
    };
    use crate::dejavu_sync::status::RepositorySyncStatus;

    const REPOSITORY_A: &str = "00000000-0000-4000-8000-0000000000a1";
    const REPOSITORY_B: &str = "00000000-0000-4000-8000-0000000000b2";

    struct NoopRunner;

    impl RepositoryJobRunner for NoopRunner {
        fn validate(&self, request: SyncJobRequest) -> Result<SyncJobRequest, RepositoryJobError> {
            Ok(request)
        }

        fn run_attempt<'a>(
            &'a self,
            _context: SyncAttemptContext,
        ) -> BoxFuture<'a, Result<RepositorySyncResult, RepositoryJobError>> {
            Box::pin(async { Ok(RepositorySyncResult::default()) })
        }
    }

    struct NoopStatus;

    impl RepositoryStatusSink for NoopStatus {
        fn publish<'a>(
            &'a self,
            _status: RepositorySyncStatus,
        ) -> BoxFuture<'a, Result<(), RepositoryJobError>> {
            Box::pin(async { Ok(()) })
        }
    }

    #[derive(Default)]
    struct FakeOperations {
        rebuilt: Mutex<Vec<String>>,
        cleared: Mutex<Vec<String>>,
        purged: Mutex<Vec<String>>,
        deleted: Mutex<Vec<String>>,
        fail_clear: AtomicBool,
        purge_started: AtomicBool,
        purge_release: Notify,
    }

    impl RepositoryLifecycleOps for FakeOperations {
        fn rebuild_local_repository(&self, repository_id: &str) -> Result<(), RepositoryJobError> {
            self.rebuilt.lock().unwrap().push(repository_id.to_owned());
            Ok(())
        }

        fn clear_local_repository(&self, repository_id: &str) -> Result<(), RepositoryJobError> {
            self.cleared.lock().unwrap().push(repository_id.to_owned());
            if self.fail_clear.load(Ordering::Acquire) {
                return Err(RepositoryJobError::RepositoryUnavailable);
            }
            Ok(())
        }

        fn purge_remote_repository<'a>(
            &'a self,
            repository_id: &'a str,
            cancelled: &'a AtomicBool,
        ) -> BoxFuture<'a, Result<(), RepositoryJobError>> {
            Box::pin(async move {
                assert!(!cancelled.load(Ordering::Acquire));
                self.purged.lock().unwrap().push(repository_id.to_owned());
                self.purge_started.store(true, Ordering::Release);
                self.purge_release.notified().await;
                Ok(())
            })
        }

        fn delete_remote_repository<'a>(
            &'a self,
            repository_id: &'a str,
        ) -> BoxFuture<'a, Result<(), RepositoryJobError>> {
            Box::pin(async move {
                self.deleted.lock().unwrap().push(repository_id.to_owned());
                Ok(())
            })
        }
    }

    #[derive(Default)]
    struct FakeSchedules {
        cleared: Mutex<Vec<String>>,
    }

    impl RepositoryScheduleReset for FakeSchedules {
        fn clear_sync_schedule(&self, repository_id: &str) -> Result<(), RepositoryJobError> {
            self.cleared.lock().unwrap().push(repository_id.to_owned());
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeRoots {
        deactivated: Mutex<Vec<PathBuf>>,
    }

    impl RepositoryRootDeactivator for FakeRoots {
        fn deactivate_root(&self, root: &Path) {
            self.deactivated.lock().unwrap().push(root.to_path_buf());
        }
    }

    #[derive(Default)]
    struct FakeLocalMaintenance {
        suspended: AtomicUsize,
        resumed: AtomicUsize,
    }

    impl LocalMaintenanceSuspender for FakeLocalMaintenance {
        fn suspend_all_and_wait(&self) -> BoxFuture<'_, ()> {
            Box::pin(async move {
                self.suspended.fetch_add(1, Ordering::AcqRel);
            })
        }

        fn resume(&self) {
            self.resumed.fetch_add(1, Ordering::AcqRel);
        }
    }

    struct Fixture {
        _temporary: tempfile::TempDir,
        app_data: PathBuf,
        root_a: PathBuf,
        root_b: PathBuf,
        service: DejavuSyncService,
        operations: Arc<FakeOperations>,
        schedules: Arc<FakeSchedules>,
        roots: Arc<FakeRoots>,
        local_maintenance: Arc<FakeLocalMaintenance>,
    }

    impl Fixture {
        fn new() -> Self {
            let temporary = tempdir().unwrap();
            let app_data = temporary.path().join("app-data");
            let root_a = temporary.path().join("notes-a");
            let root_b = temporary.path().join("notes-b");
            std::fs::create_dir_all(&app_data).unwrap();
            std::fs::create_dir_all(&root_a).unwrap();
            std::fs::create_dir_all(&root_b).unwrap();
            let root_a = root_a.canonicalize().unwrap();
            let root_b = root_b.canonicalize().unwrap();
            let state_service = LocalSyncStateService::new(&app_data);
            let mut state = state_service
                .load_or_initialize(Some(&STANDARD.encode([7_u8; 32])))
                .unwrap();
            state_service
                .bind_repository(
                    &mut state,
                    RepositoryBinding {
                        repository_id: REPOSITORY_A.to_owned(),
                        display_name: "A".to_owned(),
                        notes_root: root_a.clone(),
                        enabled: true,
                    },
                )
                .unwrap();
            state_service
                .bind_repository(
                    &mut state,
                    RepositoryBinding {
                        repository_id: REPOSITORY_B.to_owned(),
                        display_name: "B".to_owned(),
                        notes_root: root_b.clone(),
                        enabled: true,
                    },
                )
                .unwrap();
            Self {
                _temporary: temporary,
                app_data,
                root_a,
                root_b,
                service: DejavuSyncService::new(Arc::new(NoopRunner), Arc::new(NoopStatus)),
                operations: Arc::new(FakeOperations::default()),
                schedules: Arc::new(FakeSchedules::default()),
                roots: Arc::new(FakeRoots::default()),
                local_maintenance: Arc::new(FakeLocalMaintenance::default()),
            }
        }

        fn controller(&self) -> RepositoryLifecycleController {
            RepositoryLifecycleController::new(
                &self.app_data,
                self.service.clone(),
                Arc::clone(&self.operations),
                Arc::clone(&self.schedules),
                Arc::clone(&self.roots),
                Arc::clone(&self.local_maintenance),
            )
        }
    }

    #[tokio::test]
    async fn rebuild_returns_an_accepted_background_job_with_a_stable_public_shape() {
        let fixture = Fixture::new();
        let accepted = fixture
            .controller()
            .rebuild_local_repository(REPOSITORY_A, true)
            .unwrap();
        let serialized = serde_json::to_value(&accepted).unwrap();

        assert_eq!(
            accepted.operation,
            RepositoryLifecycleOperation::RebuildLocalRepository
        );
        assert_eq!(accepted.repository_id.as_deref(), Some(REPOSITORY_A));
        assert_eq!(serialized["operation"], "rebuild-local-repository");
        assert_eq!(serialized["repositoryId"], REPOSITORY_A);
        assert!(uuid::Uuid::parse_str(serialized["jobId"].as_str().unwrap()).is_ok());
        assert_eq!(accepted.wait_for_completion().await, Ok(()));
        assert_eq!(
            fixture.operations.rebuilt.lock().unwrap().as_slice(),
            [REPOSITORY_A]
        );
    }

    #[tokio::test]
    async fn stop_waits_in_the_repository_lane_then_removes_only_that_binding_and_schedule() {
        let fixture = Fixture::new();
        let in_flight = fixture.service.begin_repository_bind(REPOSITORY_A).unwrap();
        let accepted = fixture
            .controller()
            .stop_repository_sync(REPOSITORY_A, true)
            .unwrap();

        assert!(
            tokio::time::timeout(Duration::from_millis(50), accepted.wait_for_completion())
                .await
                .is_err(),
            "stop must wait for the already accepted repository operation"
        );
        assert_eq!(
            LocalSyncStateService::new(&fixture.app_data)
                .load()
                .unwrap()
                .unwrap()
                .bindings
                .len(),
            2
        );
        drop(in_flight);

        assert_eq!(accepted.wait_for_completion().await, Ok(()));
        let state = LocalSyncStateService::new(&fixture.app_data)
            .load()
            .unwrap()
            .unwrap();
        assert_eq!(state.bindings.len(), 1);
        assert_eq!(state.bindings[0].repository_id, REPOSITORY_B);
        assert_eq!(
            fixture.schedules.cleared.lock().unwrap().as_slice(),
            [REPOSITORY_A]
        );
        assert_eq!(
            fixture.roots.deactivated.lock().unwrap().as_slice(),
            [fixture.root_a]
        );
        assert!(fixture.root_b.is_dir());
    }

    #[tokio::test]
    async fn key_change_clears_all_old_key_repositories_then_preserves_identity_and_disables_bindings(
    ) {
        let fixture = Fixture::new();
        let before = LocalSyncStateService::new(&fixture.app_data)
            .load()
            .unwrap()
            .unwrap();
        let accepted = fixture
            .controller()
            .change_global_key("replacement passphrase".to_owned(), true)
            .unwrap();

        assert_eq!(accepted.wait_for_completion().await, Ok(()));
        let after = LocalSyncStateService::new(&fixture.app_data)
            .load()
            .unwrap()
            .unwrap();
        assert_eq!(after.device_id, before.device_id);
        assert_ne!(after.repo_key, before.repo_key);
        assert_eq!(
            after
                .bindings
                .iter()
                .map(|binding| (&binding.repository_id, &binding.notes_root, binding.enabled))
                .collect::<Vec<_>>(),
            vec![
                (&REPOSITORY_A.to_owned(), &fixture.root_a, false),
                (&REPOSITORY_B.to_owned(), &fixture.root_b, false),
            ]
        );
        assert_eq!(
            fixture.operations.cleared.lock().unwrap().as_slice(),
            [REPOSITORY_A, REPOSITORY_B]
        );
        assert_eq!(
            fixture.schedules.cleared.lock().unwrap().as_slice(),
            [REPOSITORY_A, REPOSITORY_B]
        );
        assert_eq!(
            fixture.roots.deactivated.lock().unwrap().as_slice(),
            [fixture.root_a, fixture.root_b]
        );
        assert_eq!(
            fixture.local_maintenance.suspended.load(Ordering::Acquire),
            1
        );
        assert_eq!(fixture.local_maintenance.resumed.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn failed_key_change_preserves_old_state_and_resumes_local_maintenance() {
        let fixture = Fixture::new();
        let before = LocalSyncStateService::new(&fixture.app_data)
            .load()
            .unwrap()
            .unwrap();
        fixture.operations.fail_clear.store(true, Ordering::Release);
        let accepted = fixture
            .controller()
            .change_global_key("replacement passphrase".to_owned(), true)
            .unwrap();

        assert_eq!(
            accepted.wait_for_completion().await,
            Err(RepositoryJobError::RepositoryUnavailable)
        );
        let after = LocalSyncStateService::new(&fixture.app_data)
            .load()
            .unwrap()
            .unwrap();
        assert_eq!(after.device_id, before.device_id);
        assert_eq!(after.repo_key, before.repo_key);
        assert!(after.bindings.iter().all(|binding| binding.enabled));
        assert_eq!(
            fixture.local_maintenance.suspended.load(Ordering::Acquire),
            1
        );
        assert_eq!(fixture.local_maintenance.resumed.load(Ordering::Acquire), 1);
        assert!(fixture.service.begin_repository_bind(REPOSITORY_A).is_ok());
    }

    #[tokio::test]
    async fn remote_purge_is_accepted_before_completion_and_seals_the_repository_lane() {
        let fixture = Fixture::new();
        let controller = fixture.controller();
        let accepted = controller
            .purge_remote_repository(REPOSITORY_A, true)
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while !fixture.operations.purge_started.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

        assert_eq!(
            fixture.service.begin_repository_bind(REPOSITORY_A).err(),
            Some(RepositoryJobError::Cancelled)
        );
        fixture.operations.purge_release.notify_one();
        assert_eq!(accepted.wait_for_completion().await, Ok(()));
        assert!(fixture.service.begin_repository_bind(REPOSITORY_A).is_ok());
    }

    #[tokio::test]
    async fn dropping_the_accepted_maintenance_listener_does_not_cancel_the_background_job() {
        let fixture = Fixture::new();
        let accepted = fixture
            .controller()
            .purge_remote_repository(REPOSITORY_A, true)
            .unwrap();
        drop(accepted);
        tokio::time::timeout(Duration::from_secs(2), async {
            while !fixture.operations.purge_started.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

        assert_eq!(
            fixture.operations.purged.lock().unwrap().as_slice(),
            [REPOSITORY_A]
        );
        fixture.operations.purge_release.notify_one();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if fixture.service.begin_repository_bind(REPOSITORY_A).is_ok() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn remote_delete_rejects_an_enabled_binding_but_allows_a_disabled_one() {
        let fixture = Fixture::new();
        let controller = fixture.controller();
        let rejected = controller
            .delete_remote_repository(REPOSITORY_A, true)
            .unwrap();
        assert_eq!(
            rejected.wait_for_completion().await,
            Err(RepositoryJobError::InvalidBinding)
        );
        assert!(fixture.operations.deleted.lock().unwrap().is_empty());

        let state_service = LocalSyncStateService::new(&fixture.app_data);
        let mut state = state_service.load().unwrap().unwrap();
        state_service
            .replace_repository_key(&mut state, "replacement")
            .unwrap();
        let accepted = controller
            .delete_remote_repository(REPOSITORY_A, true)
            .unwrap();
        assert_eq!(accepted.wait_for_completion().await, Ok(()));
        assert_eq!(
            fixture.operations.deleted.lock().unwrap().as_slice(),
            [REPOSITORY_A]
        );
    }

    #[test]
    fn every_destructive_lifecycle_operation_requires_confirmation_before_reservation() {
        let fixture = Fixture::new();
        let controller = fixture.controller();

        assert_eq!(
            controller
                .rebuild_local_repository(REPOSITORY_A, false)
                .err(),
            Some(RepositoryJobError::ConfirmationRequired)
        );
        assert_eq!(
            controller.stop_repository_sync(REPOSITORY_A, false).err(),
            Some(RepositoryJobError::ConfirmationRequired)
        );
        assert_eq!(
            controller
                .change_global_key("secret".to_owned(), false)
                .err(),
            Some(RepositoryJobError::ConfirmationRequired)
        );
        assert_eq!(
            controller
                .purge_remote_repository(REPOSITORY_A, false)
                .err(),
            Some(RepositoryJobError::ConfirmationRequired)
        );
        assert_eq!(
            controller
                .delete_remote_repository(REPOSITORY_A, false)
                .err(),
            Some(RepositoryJobError::ConfirmationRequired)
        );
        assert!(fixture.service.begin_repository_bind(REPOSITORY_A).is_ok());
    }
}
