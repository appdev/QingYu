use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock};

use qingyu_dejavu::RepositoryMetadata;
use serde::{Deserialize, Serialize};

use super::conflicts::{
    ConflictResolver, ConflictStore, ConflictVersions, ResolveConflictRequest, SyncConflictRecord,
};
use super::lifecycle::{
    AcceptedMaintenanceJob, RepositoryLifecycleController, RepositoryRootDeactivator,
};
use super::local_state::{LocalSyncStateService, RepositoryBinding};
use super::maintenance::LocalMaintenanceController;
use super::repository::{prepare_binding_root, RepositoryCatalogValidator};
use super::scheduler::DejavuScheduler;
use super::service::{
    AcceptedSyncJob, DejavuSyncService, RepositoryBindAdmission, RepositoryJobError, SyncJobRequest,
};
use super::status::RepositorySyncStatus;
use crate::sync_config::status::SyncTrigger;
use tauri::{Manager, Runtime};

#[derive(Default)]
pub(crate) struct DejavuSyncServiceOwner {
    service: OnceLock<DejavuSyncService>,
    binding: OnceLock<BindingController>,
    conflicts: OnceLock<Arc<ConflictStore>>,
    conflict_resolver: OnceLock<Arc<ConflictResolver>>,
    lifecycle: OnceLock<Arc<RepositoryLifecycleController>>,
    maintenance: OnceLock<Arc<LocalMaintenanceController>>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BindRepositoryRequest {
    pub(crate) notes_root: PathBuf,
    pub(crate) repository_id: String,
    pub(crate) display_name: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ConfirmedRepositoryRequest {
    pub(crate) repository_id: String,
    pub(crate) confirmed: bool,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ChangeGlobalKeyRequest {
    pub(crate) new_key: String,
    pub(crate) confirmed: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DejavuKeyState {
    pub(crate) configured: bool,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct InitializeGlobalKeyRequest {
    pub(crate) key: String,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ExportGlobalKeyRequest {
    pub(crate) confirmed: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReadConflictRequest {
    pub(crate) repository_id: String,
    pub(crate) conflict_id: String,
}

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub(crate) trait BindJobEnqueuer: Send + Sync {
    fn enqueue_bind_and_sync<'a>(
        &'a self,
        admission: RepositoryBindAdmission,
        request: SyncJobRequest,
    ) -> BoxFuture<'a, Result<AcceptedSyncJob, RepositoryJobError>>;
}

struct BindingController {
    app_data: PathBuf,
    catalog: Arc<dyn RepositoryCatalogValidator>,
    enqueuer: Arc<dyn BindJobEnqueuer>,
    service: DejavuSyncService,
}

#[derive(Clone, Default)]
pub(crate) struct DejavuSchedulerOwner {
    scheduler: Arc<OnceLock<DejavuScheduler>>,
    maintenance: Arc<OnceLock<Arc<LocalMaintenanceController>>>,
    roots: Arc<Mutex<SchedulerOwnerRoots>>,
    native_exit_state: Arc<Mutex<NativeExitState>>,
}

#[derive(Default)]
struct SchedulerOwnerRoots {
    watched_root: Option<PathBuf>,
    active_root: Option<PathBuf>,
    startup_pending: bool,
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

    pub(crate) fn install_maintenance(
        &self,
        maintenance: Arc<LocalMaintenanceController>,
    ) -> Result<(), RepositoryJobError> {
        self.maintenance
            .set(maintenance)
            .map_err(|_| RepositoryJobError::RepositoryUnavailable)
    }

    pub(crate) fn activate_root(&self, root: &Path) -> bool {
        let mut roots = self.roots.lock().unwrap();
        roots.watched_root = Some(root.to_path_buf());
        let activated = self.activate_locked_root(&mut roots, root);
        drop(roots);
        if activated {
            self.consume_pending_startup();
        }
        activated
    }

    pub(crate) fn refresh_after_bind(&self, root: &Path) -> bool {
        let mut roots = self.roots.lock().unwrap();
        if roots.watched_root.as_deref() != Some(root) {
            return false;
        }
        let activated = self.activate_locked_root(&mut roots, root);
        if activated {
            // The accepted manual bind job covers a launch that was waiting only
            // because this watched root did not have a binding yet. A launch
            // arriving after this transaction observes the active root normally.
            roots.startup_pending = false;
        }
        activated
    }

    fn observe_bind_completion(&self, accepted: AcceptedSyncJob) {
        let Some(scheduler) = self.scheduler.get().cloned() else {
            return;
        };
        tauri::async_runtime::spawn(async move {
            let result = accepted.wait_for_completion().await;
            let _recorded = scheduler.record_bind_completion(
                &accepted.notes_root,
                &accepted.repository_id,
                result,
            );
        });
    }

    fn activate_locked_root(&self, roots: &mut SchedulerOwnerRoots, root: &Path) -> bool {
        let Some(scheduler) = self.scheduler.get() else {
            roots.active_root = None;
            return false;
        };
        let activated = scheduler.activate_root(root).unwrap_or(false);
        if activated {
            roots.active_root = Some(root.to_path_buf());
            return true;
        }
        if let Some(previous) = roots.active_root.take() {
            scheduler.deactivate_root(&previous);
        }
        false
    }

    pub(crate) fn deactivate_root(&self, root: &Path) -> bool {
        let mut roots = self.roots.lock().unwrap();
        let watched = roots.watched_root.as_deref() == Some(root);
        if watched {
            roots.watched_root = None;
        }
        let active = roots.active_root.as_deref() == Some(root);
        if active {
            roots.active_root = None;
        }
        let deactivated = active
            && self
                .scheduler
                .get()
                .is_some_and(|scheduler| scheduler.deactivate_root(root));
        watched || deactivated
    }

    pub(crate) fn record_file_change(&self, root: &Path, path: &Path) -> bool {
        self.scheduler
            .get()
            .is_some_and(|scheduler| scheduler.record_file_change(root, path).unwrap_or(false))
    }

    pub(crate) fn trigger_startup(&self) {
        self.roots.lock().unwrap().startup_pending = true;
        self.consume_pending_startup();
    }

    fn consume_pending_startup(&self) {
        let Some(scheduler) = self.scheduler.get().cloned() else {
            return;
        };
        {
            let mut roots = self.roots.lock().unwrap();
            if roots.active_root.is_none() || !roots.startup_pending {
                return;
            }
            roots.startup_pending = false;
        }
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
        let maintenance = self.maintenance.get().cloned();
        NativeExitAction::Wait(Box::pin(async move {
            if let Some(maintenance) = maintenance.as_ref() {
                maintenance.suspend_all_and_wait().await;
            }
            let result = match scheduler.trigger_exit_job().await {
                Ok(Some(accepted)) => accepted.wait_for_completion().await,
                Ok(None) => Ok(()),
                Err(error) => Err(error),
            };
            if result.is_err() {
                if let Some(maintenance) = maintenance {
                    maintenance.resume();
                }
            }
            *state.lock().unwrap() = if result.is_ok() {
                NativeExitState::BypassReady
            } else {
                NativeExitState::Idle
            };
            result
        }))
    }
}

impl RepositoryRootDeactivator for DejavuSchedulerOwner {
    fn deactivate_root(&self, root: &Path) {
        DejavuSchedulerOwner::deactivate_root(self, root);
    }
}

impl DejavuSyncServiceOwner {
    #[allow(dead_code)]
    pub(crate) fn install(&self, service: DejavuSyncService) -> Result<(), RepositoryJobError> {
        self.service
            .set(service)
            .map_err(|_| RepositoryJobError::RepositoryUnavailable)
    }

    pub(crate) fn install_maintenance(
        &self,
        maintenance: Arc<LocalMaintenanceController>,
    ) -> Result<(), RepositoryJobError> {
        self.maintenance
            .set(maintenance)
            .map_err(|_| RepositoryJobError::RepositoryUnavailable)
    }

    pub(crate) fn install_lifecycle(
        &self,
        lifecycle: Arc<RepositoryLifecycleController>,
    ) -> Result<(), RepositoryJobError> {
        self.lifecycle
            .set(lifecycle)
            .map_err(|_| RepositoryJobError::RepositoryUnavailable)
    }

    pub(crate) fn install_conflicts(
        &self,
        conflicts: Arc<ConflictStore>,
    ) -> Result<(), RepositoryJobError> {
        self.conflicts
            .set(conflicts)
            .map_err(|_| RepositoryJobError::RepositoryUnavailable)
    }

    pub(crate) fn install_conflict_resolver(
        &self,
        resolver: Arc<ConflictResolver>,
    ) -> Result<(), RepositoryJobError> {
        self.conflict_resolver
            .set(resolver)
            .map_err(|_| RepositoryJobError::RepositoryUnavailable)
    }

    pub(crate) fn install_binding<Validator, Enqueuer>(
        &self,
        app_data: impl AsRef<Path>,
        catalog: Arc<Validator>,
        enqueuer: Arc<Enqueuer>,
        service: DejavuSyncService,
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
                service,
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
        validate_repository_id(&request.repository_id)?;
        let admission = controller
            .service
            .begin_repository_bind(&request.repository_id)?;
        let metadata = controller
            .catalog
            .read_repository(&request.repository_id)
            .await?;
        validate_selected_metadata(&request, &metadata)?;
        let notes_root = prepare_binding_root(&request.notes_root)?;
        let binding = RepositoryBinding {
            repository_id: metadata.repository_id.clone(),
            display_name: metadata.display_name,
            notes_root: notes_root.clone(),
            enabled: true,
        };
        admission
            .run_state(async {
                let state_service = LocalSyncStateService::new(&controller.app_data);
                let mut state = state_service
                    .load_or_initialize(None)
                    .map_err(RepositoryJobError::from)?;
                state_service
                    .bind_repository(&mut state, binding)
                    .map_err(RepositoryJobError::from)
            })
            .await?;
        controller
            .enqueuer
            .enqueue_bind_and_sync(
                admission,
                SyncJobRequest {
                    notes_root,
                    repository_id: metadata.repository_id,
                    trigger: SyncTrigger::Manual,
                },
            )
            .await
    }

    fn lifecycle(&self) -> Result<&RepositoryLifecycleController, RepositoryJobError> {
        self.lifecycle
            .get()
            .map(Arc::as_ref)
            .ok_or(RepositoryJobError::RepositoryUnavailable)
    }

    fn conflicts(&self) -> Result<&ConflictStore, RepositoryJobError> {
        self.conflicts
            .get()
            .map(Arc::as_ref)
            .ok_or(RepositoryJobError::RepositoryUnavailable)
    }

    fn service(&self) -> Result<&DejavuSyncService, RepositoryJobError> {
        self.service
            .get()
            .ok_or(RepositoryJobError::RepositoryUnavailable)
    }

    pub(crate) fn installed_service(&self) -> Result<DejavuSyncService, RepositoryJobError> {
        self.service().cloned()
    }

    pub(crate) fn key_state(&self) -> Result<DejavuKeyState, RepositoryJobError> {
        Ok(DejavuKeyState {
            configured: self.conflicts()?.key_configured()?,
        })
    }

    pub(crate) fn export_global_key(
        &self,
        request: ExportGlobalKeyRequest,
    ) -> Result<String, RepositoryJobError> {
        if !request.confirmed {
            return Err(RepositoryJobError::ConfirmationRequired);
        }
        self.conflicts()?.export_key()
    }

    pub(crate) async fn initialize_global_key(
        &self,
        request: InitializeGlobalKeyRequest,
    ) -> Result<DejavuKeyState, RepositoryJobError> {
        if request.key.trim().is_empty() {
            return Err(RepositoryJobError::InvalidBinding);
        }
        let reservation = self.service()?.reserve_global_maintenance()?;
        let app_data = self.conflicts()?.app_data().to_path_buf();
        reservation
            .run_state(async move {
                LocalSyncStateService::new(app_data)
                    .load_or_initialize(Some(&request.key))
                    .map_err(RepositoryJobError::from)
            })
            .await?;
        Ok(DejavuKeyState { configured: true })
    }

    async fn resolve_conflict(
        &self,
        request: ResolveConflictRequest,
    ) -> Result<AcceptedSyncJob, RepositoryJobError> {
        self.conflict_resolver
            .get()
            .ok_or(RepositoryJobError::RepositoryUnavailable)?
            .resolve(request)
            .await
    }

    pub(crate) fn list_conflicts(
        &self,
        repository_id: &str,
    ) -> Result<Vec<SyncConflictRecord>, RepositoryJobError> {
        self.conflicts()?.list(repository_id)
    }

    pub(crate) fn read_conflict(
        &self,
        request: ReadConflictRequest,
    ) -> Result<ConflictVersions, RepositoryJobError> {
        self.conflicts()?
            .read(&request.repository_id, &request.conflict_id)
    }

    pub(crate) fn repository_status_for_root(
        &self,
        notes_root: &Path,
    ) -> Result<Option<RepositorySyncStatus>, RepositoryJobError> {
        self.conflicts()?.status_for_root(notes_root)
    }

    pub(crate) fn rebuild_local_repository(
        &self,
        request: ConfirmedRepositoryRequest,
    ) -> Result<AcceptedMaintenanceJob, RepositoryJobError> {
        self.lifecycle()?
            .rebuild_local_repository(&request.repository_id, request.confirmed)
    }

    pub(crate) fn stop_repository_sync(
        &self,
        request: ConfirmedRepositoryRequest,
    ) -> Result<AcceptedMaintenanceJob, RepositoryJobError> {
        self.lifecycle()?
            .stop_repository_sync(&request.repository_id, request.confirmed)
    }

    pub(crate) fn change_global_key(
        &self,
        request: ChangeGlobalKeyRequest,
    ) -> Result<AcceptedMaintenanceJob, RepositoryJobError> {
        self.lifecycle()?
            .change_global_key(request.new_key, request.confirmed)
    }

    pub(crate) fn purge_remote_repository(
        &self,
        request: ConfirmedRepositoryRequest,
    ) -> Result<AcceptedMaintenanceJob, RepositoryJobError> {
        self.lifecycle()?
            .purge_remote_repository(&request.repository_id, request.confirmed)
    }

    pub(crate) fn delete_remote_repository(
        &self,
        request: ConfirmedRepositoryRequest,
    ) -> Result<AcceptedMaintenanceJob, RepositoryJobError> {
        self.lifecycle()?
            .delete_remote_repository(&request.repository_id, request.confirmed)
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
        if let Some(maintenance) = self.maintenance.get() {
            maintenance.suspend_all_and_wait().await;
        }
        if let Some(service) = self.service.get() {
            service.cancel_all_for_shutdown_or_reset().await;
        }
    }

    #[allow(dead_code)]
    pub(crate) fn resume_local_maintenance(&self) {
        if let Some(maintenance) = self.maintenance.get() {
            maintenance.resume();
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
        admission: RepositoryBindAdmission,
        request: SyncJobRequest,
    ) -> BoxFuture<'a, Result<AcceptedSyncJob, RepositoryJobError>> {
        Box::pin(async move { admission.enqueue(request).await })
    }
}

#[tauri::command]
pub(crate) async fn bind_dejavu_repository(
    owner: tauri::State<'_, DejavuSyncServiceOwner>,
    scheduler_owner: tauri::State<'_, DejavuSchedulerOwner>,
    request: BindRepositoryRequest,
) -> Result<AcceptedSyncJob, String> {
    bind_repository_and_refresh_scheduler(&owner, &scheduler_owner, request)
        .await
        .map_err(|error| error.safe_code().to_owned())
}

#[tauri::command]
pub(crate) fn rebuild_local_repository(
    owner: tauri::State<'_, DejavuSyncServiceOwner>,
    request: ConfirmedRepositoryRequest,
) -> Result<AcceptedMaintenanceJob, String> {
    owner
        .rebuild_local_repository(request)
        .map_err(|error| error.safe_code().to_owned())
}

#[tauri::command]
pub(crate) fn stop_repository_sync(
    owner: tauri::State<'_, DejavuSyncServiceOwner>,
    request: ConfirmedRepositoryRequest,
) -> Result<AcceptedMaintenanceJob, String> {
    owner
        .stop_repository_sync(request)
        .map_err(|error| error.safe_code().to_owned())
}

#[tauri::command]
pub(crate) fn change_global_key(
    owner: tauri::State<'_, DejavuSyncServiceOwner>,
    request: ChangeGlobalKeyRequest,
) -> Result<AcceptedMaintenanceJob, String> {
    owner
        .change_global_key(request)
        .map_err(|error| error.safe_code().to_owned())
}

#[tauri::command]
pub(crate) fn load_dejavu_key_state(
    owner: tauri::State<'_, DejavuSyncServiceOwner>,
) -> Result<DejavuKeyState, String> {
    owner
        .key_state()
        .map_err(|error| error.safe_code().to_owned())
}

#[tauri::command]
pub(crate) async fn initialize_dejavu_global_key(
    owner: tauri::State<'_, DejavuSyncServiceOwner>,
    request: InitializeGlobalKeyRequest,
) -> Result<DejavuKeyState, String> {
    owner
        .initialize_global_key(request)
        .await
        .map_err(|error| error.safe_code().to_owned())
}

#[tauri::command]
pub(crate) fn export_dejavu_global_key(
    owner: tauri::State<'_, DejavuSyncServiceOwner>,
    request: ExportGlobalKeyRequest,
) -> Result<String, String> {
    owner
        .export_global_key(request)
        .map_err(|error| error.safe_code().to_owned())
}

#[tauri::command]
pub(crate) fn purge_remote_repository(
    owner: tauri::State<'_, DejavuSyncServiceOwner>,
    request: ConfirmedRepositoryRequest,
) -> Result<AcceptedMaintenanceJob, String> {
    owner
        .purge_remote_repository(request)
        .map_err(|error| error.safe_code().to_owned())
}

#[tauri::command]
pub(crate) fn delete_remote_repository(
    owner: tauri::State<'_, DejavuSyncServiceOwner>,
    request: ConfirmedRepositoryRequest,
) -> Result<AcceptedMaintenanceJob, String> {
    owner
        .delete_remote_repository(request)
        .map_err(|error| error.safe_code().to_owned())
}

#[tauri::command]
pub(crate) fn list_dejavu_conflicts(
    owner: tauri::State<'_, DejavuSyncServiceOwner>,
    repository_id: String,
) -> Result<Vec<SyncConflictRecord>, String> {
    owner
        .list_conflicts(&repository_id)
        .map_err(|error| error.safe_code().to_owned())
}

#[tauri::command]
pub(crate) fn read_dejavu_conflict(
    owner: tauri::State<'_, DejavuSyncServiceOwner>,
    request: ReadConflictRequest,
) -> Result<ConflictVersions, String> {
    owner
        .read_conflict(request)
        .map_err(|error| error.safe_code().to_owned())
}

#[tauri::command]
pub(crate) fn load_dejavu_repository_status(
    owner: tauri::State<'_, DejavuSyncServiceOwner>,
    notes_root: PathBuf,
) -> Result<Option<RepositorySyncStatus>, String> {
    owner
        .repository_status_for_root(&notes_root)
        .map_err(|error| error.safe_code().to_owned())
}

#[tauri::command]
pub(crate) async fn resolve_dejavu_conflict(
    owner: tauri::State<'_, DejavuSyncServiceOwner>,
    request: ResolveConflictRequest,
) -> Result<AcceptedSyncJob, String> {
    owner
        .resolve_conflict(request)
        .await
        .map_err(|error| error.safe_code().to_owned())
}

async fn bind_repository_and_refresh_scheduler(
    owner: &DejavuSyncServiceOwner,
    scheduler_owner: &DejavuSchedulerOwner,
    request: BindRepositoryRequest,
) -> Result<AcceptedSyncJob, RepositoryJobError> {
    let accepted = owner.bind_repository(request).await?;
    if scheduler_owner.refresh_after_bind(&accepted.notes_root) {
        scheduler_owner.observe_bind_completion(accepted.clone());
    }
    Ok(accepted)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::future::Future;
    use std::path::{Path, PathBuf};
    use std::pin::Pin;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use qingyu_dejavu::RepositoryMetadata;
    use tempfile::tempdir;
    use time::OffsetDateTime;
    use tokio::sync::{mpsc, oneshot, watch, Notify, Semaphore};

    use super::{
        bind_repository_and_refresh_scheduler, BindJobEnqueuer, BindRepositoryRequest,
        DejavuSchedulerOwner, DejavuSyncServiceOwner, NativeExitAction,
    };
    use crate::dejavu_sync::local_state::{LocalSyncStateService, RepositoryBinding};
    use crate::dejavu_sync::maintenance::{
        LocalMaintenanceController, LocalMaintenanceStatusStore, LocalPurgeOutcome,
        LocalPurgeTaskExecutor, LocalPurgeTaskFuture,
    };
    use crate::dejavu_sync::repository::RepositoryCatalogValidator;
    use crate::dejavu_sync::scheduler::{
        ActiveRepositorySchedule, DejavuScheduler, DnsFlusher, RepositoryScheduleSource,
        RepositoryScheduleStore, SchedulerJobEnqueuer,
    };
    use crate::dejavu_sync::service::{
        AcceptedSyncJob, DejavuSyncService, RepositoryBindAdmission, RepositoryJobError,
        RepositoryJobRunner, RepositoryStatusSink, RepositorySyncResult, SyncAttemptContext,
        SyncJobRequest,
    };
    use crate::dejavu_sync::status::{
        RepositoryMaintenance, RepositorySchedule, RepositorySyncStatus,
    };
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

    struct ExitMaintenanceStatusStore;

    impl LocalMaintenanceStatusStore for ExitMaintenanceStatusStore {
        fn load_maintenance(
            &self,
            _repository_id: &str,
        ) -> Result<RepositoryMaintenance, RepositoryJobError> {
            Ok(RepositoryMaintenance::default())
        }

        fn set_maintenance(
            &self,
            _repository_id: &str,
            maintenance: RepositoryMaintenance,
        ) -> Result<RepositoryMaintenance, RepositoryJobError> {
            Ok(maintenance)
        }
    }

    struct ExitCancellationPurgeExecutor {
        started: Arc<Notify>,
        saw_cancel: Arc<Notify>,
        release: Arc<Semaphore>,
    }

    #[derive(Default)]
    struct ImmediateExitPurgeExecutor(AtomicUsize);

    impl LocalPurgeTaskExecutor for ImmediateExitPurgeExecutor {
        fn execute(
            &self,
            _repository_id: String,
            _now: OffsetDateTime,
            _cancelled: Arc<AtomicBool>,
        ) -> LocalPurgeTaskFuture {
            self.0.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(LocalPurgeOutcome::Skipped) })
        }
    }

    impl ExitCancellationPurgeExecutor {
        fn new() -> Self {
            Self {
                started: Arc::new(Notify::new()),
                saw_cancel: Arc::new(Notify::new()),
                release: Arc::new(Semaphore::new(0)),
            }
        }
    }

    impl LocalPurgeTaskExecutor for ExitCancellationPurgeExecutor {
        fn execute(
            &self,
            _repository_id: String,
            _now: OffsetDateTime,
            cancelled: Arc<AtomicBool>,
        ) -> LocalPurgeTaskFuture {
            self.started.notify_one();
            let saw_cancel = Arc::clone(&self.saw_cancel);
            let release = Arc::clone(&self.release);
            Box::pin(async move {
                while !cancelled.load(Ordering::Acquire) {
                    tokio::task::yield_now().await;
                }
                saw_cancel.notify_one();
                release.acquire().await.unwrap().forget();
                Ok(LocalPurgeOutcome::Skipped)
            })
        }
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

    fn test_binding_service(app_data: &Path) -> DejavuSyncService {
        DejavuSyncService::new(
            Arc::new(PersistedBindingRunner::new(app_data.to_path_buf())),
            Arc::new(NoopStatusSink),
        )
    }

    struct RecordingBindEnqueuer {
        app_data: PathBuf,
        requests: Mutex<Vec<SyncJobRequest>>,
        completions: Mutex<Vec<watch::Sender<Option<Result<(), RepositoryJobError>>>>>,
        enqueued: Notify,
    }

    impl RecordingBindEnqueuer {
        fn new(app_data: PathBuf) -> Self {
            Self {
                app_data,
                requests: Mutex::new(Vec::new()),
                completions: Mutex::new(Vec::new()),
                enqueued: Notify::new(),
            }
        }
    }

    impl BindJobEnqueuer for RecordingBindEnqueuer {
        fn enqueue_bind_and_sync<'a>(
            &'a self,
            _admission: RepositoryBindAdmission,
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
                self.enqueued.notify_waiters();
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

    struct PausingServiceBindEnqueuer {
        before_enqueue: Notify,
        release_enqueue: Notify,
        delegating_enqueue: Notify,
    }

    impl PausingServiceBindEnqueuer {
        fn new() -> Self {
            Self {
                before_enqueue: Notify::new(),
                release_enqueue: Notify::new(),
                delegating_enqueue: Notify::new(),
            }
        }
    }

    impl BindJobEnqueuer for PausingServiceBindEnqueuer {
        fn enqueue_bind_and_sync<'a>(
            &'a self,
            admission: RepositoryBindAdmission,
            request: SyncJobRequest,
        ) -> BoxFuture<'a, Result<AcceptedSyncJob, RepositoryJobError>> {
            Box::pin(async move {
                self.before_enqueue.notify_one();
                self.release_enqueue.notified().await;
                self.delegating_enqueue.notify_one();
                admission.enqueue(request).await
            })
        }
    }

    struct FailOnceBindEnqueuer {
        app_data: PathBuf,
        attempts: AtomicUsize,
    }

    impl FailOnceBindEnqueuer {
        fn new(app_data: PathBuf) -> Self {
            Self {
                app_data,
                attempts: AtomicUsize::new(0),
            }
        }
    }

    impl BindJobEnqueuer for FailOnceBindEnqueuer {
        fn enqueue_bind_and_sync<'a>(
            &'a self,
            _admission: RepositoryBindAdmission,
            request: SyncJobRequest,
        ) -> BoxFuture<'a, Result<AcceptedSyncJob, RepositoryJobError>> {
            Box::pin(async move {
                let state = LocalSyncStateService::new(&self.app_data)
                    .load()
                    .unwrap()
                    .unwrap();
                assert_eq!(state.bindings.len(), 1, "binding is durable first");
                if self.attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                    return Err(RepositoryJobError::CloudUnavailable);
                }
                Ok(AcceptedSyncJob::completed_for_test(
                    "00000000-0000-4000-8000-000000000099",
                    request.repository_id,
                    request.notes_root,
                ))
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
    struct MutableOwnerSource(Mutex<Option<ActiveRepositorySchedule>>);

    impl MutableOwnerSource {
        fn activate(&self, schedule: ActiveRepositorySchedule) {
            *self.0.lock().unwrap() = Some(schedule);
        }
    }

    impl RepositoryScheduleSource for MutableOwnerSource {
        fn resolve_active_root(
            &self,
            root: &Path,
        ) -> Result<Option<ActiveRepositorySchedule>, RepositoryJobError> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .as_ref()
                .filter(|active| active.notes_root == root)
                .cloned())
        }
    }

    struct BindingOwnerSource {
        app_data: PathBuf,
        mode: SyncMode,
    }

    impl RepositoryScheduleSource for BindingOwnerSource {
        fn resolve_active_root(
            &self,
            root: &Path,
        ) -> Result<Option<ActiveRepositorySchedule>, RepositoryJobError> {
            let state = LocalSyncStateService::new(&self.app_data)
                .load()
                .map_err(|_| RepositoryJobError::RepositoryUnavailable)?;
            Ok(state.and_then(|state| {
                state
                    .bindings
                    .into_iter()
                    .find(|binding| binding.enabled && binding.notes_root == root)
                    .map(|binding| ActiveRepositorySchedule {
                        notes_root: binding.notes_root,
                        repository_id: binding.repository_id,
                        mode: self.mode,
                        interval: Duration::from_secs(30),
                    })
            }))
        }
    }

    #[derive(Default)]
    struct OwnerStore {
        schedules: Mutex<HashMap<String, RepositorySchedule>>,
        updates: AtomicUsize,
        changed: Notify,
    }

    impl OwnerStore {
        async fn wait_for_updates(&self, expected: usize) {
            loop {
                let changed = self.changed.notified();
                if self.updates.load(Ordering::SeqCst) >= expected {
                    return;
                }
                changed.await;
            }
        }
    }

    impl RepositoryScheduleStore for OwnerStore {
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
            let schedule = schedules.entry(repository_id.to_owned()).or_default();
            update(schedule);
            let updated = schedule.clone();
            self.updates.fetch_add(1, Ordering::SeqCst);
            self.changed.notify_waiters();
            Ok(updated)
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
        assert!(owner.deactivate_root(Path::new("/notes/uninstalled")));
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
        assert_eq!(jobs.recv().await.unwrap().trigger, SyncTrigger::AppLaunch);

        assert!(owner.activate_root(&root));
        assert!(jobs.try_recv().is_err());
    }

    #[tokio::test]
    async fn accepted_bind_refreshes_only_the_current_watched_root_without_replaying_startup() {
        let temporary = tempdir().unwrap();
        let app_data = temporary.path().join("app-data");
        let root_path = temporary.path().join("bound-after-watch");
        let switched_path = temporary.path().join("switched-away");
        std::fs::create_dir(&root_path).unwrap();
        std::fs::create_dir(&switched_path).unwrap();
        let root = root_path.canonicalize().unwrap();
        let switched_root = switched_path.canonicalize().unwrap();
        let repository_id = "00000000-0000-4000-8000-000000000043";
        let source = Arc::new(BindingOwnerSource {
            app_data: app_data.clone(),
            mode: SyncMode::StartupExit,
        });
        let (sent, mut jobs) = mpsc::unbounded_channel();
        let scheduler = DejavuScheduler::new(
            source,
            Arc::new(OwnerEnqueuer(sent)),
            Arc::new(OwnerStore::default()),
            Arc::new(OwnerFlusher),
            Arc::new(|| OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap()),
        );
        let scheduler_owner = DejavuSchedulerOwner::default();
        scheduler_owner.install(scheduler).unwrap();
        let catalog = Arc::new(FakeCatalogValidator::new([repository_metadata(
            repository_id,
            "Watched journal",
        )]));
        let bind_enqueuer = Arc::new(RecordingBindEnqueuer::new(app_data.clone()));
        let service_owner = DejavuSyncServiceOwner::default();
        service_owner
            .install_binding(
                &app_data,
                catalog,
                bind_enqueuer,
                test_binding_service(&app_data),
            )
            .unwrap();

        scheduler_owner.trigger_startup();
        assert!(!scheduler_owner.activate_root(&root));

        bind_repository_and_refresh_scheduler(
            &service_owner,
            &scheduler_owner,
            bind_request(root.clone(), repository_id, "Watched journal"),
        )
        .await
        .unwrap();
        assert!(jobs.try_recv().is_err(), "manual bind covers stale startup");

        scheduler_owner.trigger_startup();
        let startup = jobs.recv().await.unwrap();
        assert_eq!(startup.repository_id, repository_id);
        assert_eq!(startup.trigger, SyncTrigger::AppLaunch);

        let exit_wait = match scheduler_owner.begin_native_exit() {
            NativeExitAction::Wait(wait) => tokio::spawn(wait),
            _ => panic!("active StartupExit binding must enqueue on exit"),
        };
        assert_eq!(
            jobs.recv().await.unwrap().trigger,
            SyncTrigger::SettingsExit
        );
        assert_eq!(exit_wait.await.unwrap(), Ok(()));

        assert!(!scheduler_owner.activate_root(&switched_root));
        assert!(!scheduler_owner.refresh_after_bind(&root));
        assert!(!scheduler_owner.record_file_change(&root, &root.join("stale.md")));
    }

    #[tokio::test]
    async fn closed_unbound_watch_is_not_reactivated_after_binding() {
        let root = PathBuf::from("/notes/closed-before-bind");
        let source = Arc::new(MutableOwnerSource::default());
        let (sent, mut jobs) = mpsc::unbounded_channel();
        let scheduler = DejavuScheduler::new(
            Arc::clone(&source),
            Arc::new(OwnerEnqueuer(sent)),
            Arc::new(OwnerStore::default()),
            Arc::new(OwnerFlusher),
            Arc::new(|| OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap()),
        );
        let owner = DejavuSchedulerOwner::default();
        owner.install(scheduler).unwrap();

        assert!(!owner.activate_root(&root));
        assert!(owner.deactivate_root(&root));
        source.activate(ActiveRepositorySchedule {
            notes_root: root.clone(),
            repository_id: "00000000-0000-4000-8000-000000000044".to_owned(),
            mode: SyncMode::StartupExit,
            interval: Duration::from_secs(30),
        });

        assert!(!owner.refresh_after_bind(&root));
        owner.trigger_startup();
        assert!(jobs.try_recv().is_err());
    }

    #[tokio::test]
    async fn disabled_binding_does_not_activate_even_while_its_root_is_watched() {
        let temporary = tempdir().unwrap();
        let app_data = temporary.path().join("app-data");
        let root_path = temporary.path().join("disabled-binding");
        std::fs::create_dir(&root_path).unwrap();
        let root = root_path.canonicalize().unwrap();
        let state_service = LocalSyncStateService::new(&app_data);
        let mut state = state_service.load_or_initialize(None).unwrap();
        state_service
            .bind_repository(
                &mut state,
                RepositoryBinding {
                    repository_id: "00000000-0000-4000-8000-000000000046".to_owned(),
                    display_name: "Disabled".to_owned(),
                    notes_root: root.clone(),
                    enabled: false,
                },
            )
            .unwrap();
        let (sent, mut jobs) = mpsc::unbounded_channel();
        let scheduler = DejavuScheduler::new(
            Arc::new(BindingOwnerSource {
                app_data,
                mode: SyncMode::StartupExit,
            }),
            Arc::new(OwnerEnqueuer(sent)),
            Arc::new(OwnerStore::default()),
            Arc::new(OwnerFlusher),
            Arc::new(|| OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap()),
        );
        let owner = DejavuSchedulerOwner::default();
        owner.install(scheduler).unwrap();

        assert!(!owner.activate_root(&root));
        assert!(!owner.refresh_after_bind(&root));
        owner.trigger_startup();
        assert!(jobs.try_recv().is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn accepted_automatic_bind_completion_polls_without_file_change_or_caller_handle() {
        let temporary = tempdir().unwrap();
        let app_data = temporary.path().join("app-data");
        let root_path = temporary.path().join("automatic-after-bind");
        std::fs::create_dir(&root_path).unwrap();
        let root = root_path.canonicalize().unwrap();
        let repository_id = "00000000-0000-4000-8000-000000000045";
        let source = Arc::new(BindingOwnerSource {
            app_data: app_data.clone(),
            mode: SyncMode::Automatic,
        });
        let (sent, mut jobs) = mpsc::unbounded_channel();
        let store = Arc::new(OwnerStore::default());
        let scheduler = DejavuScheduler::new(
            source,
            Arc::new(OwnerEnqueuer(sent)),
            Arc::clone(&store),
            Arc::new(OwnerFlusher),
            Arc::new(|| OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap()),
        );
        let scheduler_owner = DejavuSchedulerOwner::default();
        scheduler_owner.install(scheduler).unwrap();
        let catalog = Arc::new(FakeCatalogValidator::new([repository_metadata(
            repository_id,
            "Automatic journal",
        )]));
        let bind_enqueuer = Arc::new(RecordingBindEnqueuer::new(app_data.clone()));
        let service_owner = DejavuSyncServiceOwner::default();
        service_owner
            .install_binding(
                &app_data,
                catalog,
                Arc::clone(&bind_enqueuer),
                test_binding_service(&app_data),
            )
            .unwrap();

        assert!(!scheduler_owner.activate_root(&root));
        let accepted = bind_repository_and_refresh_scheduler(
            &service_owner,
            &scheduler_owner,
            bind_request(root.clone(), repository_id, "Automatic journal"),
        )
        .await
        .unwrap();
        assert!(
            jobs.try_recv().is_err(),
            "bind must not enqueue a duplicate"
        );

        let completion = bind_enqueuer.completions.lock().unwrap().pop().unwrap();
        drop(accepted);
        completion.send(Some(Ok(()))).unwrap();
        store.wait_for_updates(1).await;
        assert!(jobs.try_recv().is_err(), "completion only installs a due");
        let schedule = store.load_schedule(repository_id).unwrap();
        assert_eq!(
            schedule.next_scheduled_at,
            Some(OffsetDateTime::from_unix_timestamp(1_800_000_030).unwrap())
        );

        tokio::time::advance(Duration::from_secs(30)).await;
        let automatic = jobs.recv().await.unwrap();
        assert_eq!(automatic.trigger, SyncTrigger::Interval);
    }

    #[tokio::test(start_paused = true)]
    async fn failed_automatic_bind_completion_retries_after_five_minutes() {
        let temporary = tempdir().unwrap();
        let app_data = temporary.path().join("app-data");
        let root_path = temporary.path().join("failed-automatic-bind");
        std::fs::create_dir(&root_path).unwrap();
        let root = root_path.canonicalize().unwrap();
        let repository_id = "00000000-0000-4000-8000-000000000047";
        let source = Arc::new(BindingOwnerSource {
            app_data: app_data.clone(),
            mode: SyncMode::Automatic,
        });
        let (sent, mut jobs) = mpsc::unbounded_channel();
        let store = Arc::new(OwnerStore::default());
        let scheduler = DejavuScheduler::new(
            source,
            Arc::new(OwnerEnqueuer(sent)),
            Arc::clone(&store),
            Arc::new(OwnerFlusher),
            Arc::new(|| OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap()),
        );
        let scheduler_owner = DejavuSchedulerOwner::default();
        scheduler_owner.install(scheduler).unwrap();
        let catalog = Arc::new(FakeCatalogValidator::new([repository_metadata(
            repository_id,
            "Failed automatic journal",
        )]));
        let bind_enqueuer = Arc::new(RecordingBindEnqueuer::new(app_data.clone()));
        let service_owner = DejavuSyncServiceOwner::default();
        service_owner
            .install_binding(
                &app_data,
                catalog,
                Arc::clone(&bind_enqueuer),
                test_binding_service(&app_data),
            )
            .unwrap();

        assert!(!scheduler_owner.activate_root(&root));
        let accepted = bind_repository_and_refresh_scheduler(
            &service_owner,
            &scheduler_owner,
            bind_request(root.clone(), repository_id, "Failed automatic journal"),
        )
        .await
        .unwrap();
        assert!(jobs.try_recv().is_err());

        let completion = bind_enqueuer.completions.lock().unwrap().pop().unwrap();
        drop(accepted);
        completion
            .send(Some(Err(RepositoryJobError::CloudUnavailable)))
            .unwrap();
        store.wait_for_updates(1).await;
        let schedule = store.load_schedule(repository_id).unwrap();
        assert_eq!(schedule.automatic_failure_count, 1);
        assert_eq!(
            schedule.next_scheduled_at,
            Some(OffsetDateTime::from_unix_timestamp(1_800_000_300).unwrap())
        );
        assert!(jobs.try_recv().is_err());

        tokio::time::advance(Duration::from_secs(5 * 60)).await;
        assert_eq!(jobs.recv().await.unwrap().trigger, SyncTrigger::Interval);
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
    async fn native_exit_cancels_and_waits_for_local_maintenance_before_exit_sync() {
        let owner = DejavuSchedulerOwner::default();
        let (scheduler, root, mut jobs) = pending_exit_scheduler_fixture();
        owner.install(scheduler).unwrap();
        assert!(owner.activate_root(&root));

        let executor = Arc::new(ExitCancellationPurgeExecutor::new());
        let maintenance = Arc::new(LocalMaintenanceController::new(
            Arc::clone(&executor),
            Arc::new(ExitMaintenanceStatusStore),
            || OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap(),
        ));
        owner.install_maintenance(Arc::clone(&maintenance)).unwrap();
        let maintenance_started = executor.started.notified();
        assert!(maintenance
            .notify_sync_completion(
                "00000000-0000-4000-8000-000000000049",
                SyncTrigger::Manual,
                true,
            )
            .unwrap());
        maintenance_started.await;

        let saw_cancel = executor.saw_cancel.notified();
        let wait = match owner.begin_native_exit() {
            NativeExitAction::Wait(wait) => wait,
            _ => panic!("native exit should wait for maintenance and exit sync"),
        };
        let waiting = tokio::spawn(wait);
        tokio::time::timeout(Duration::from_secs(1), saw_cancel)
            .await
            .expect("exit must set the maintenance cancellation flag first");
        assert!(
            jobs.try_recv().is_err(),
            "exit sync must wait for purge return"
        );

        executor.release.add_permits(1);
        let (request, completion) = jobs.recv().await.unwrap();
        assert_eq!(request.trigger, SyncTrigger::SettingsExit);
        completion.send(Some(Ok(()))).unwrap();
        assert_eq!(waiting.await.unwrap(), Ok(()));
        assert!(!maintenance
            .notify_sync_completion(
                "00000000-0000-4000-8000-00000000004a",
                SyncTrigger::Manual,
                true,
            )
            .unwrap());
    }

    #[tokio::test]
    async fn failed_native_exit_sync_resets_the_barrier_for_a_retry() {
        let owner = DejavuSchedulerOwner::default();
        let (scheduler, root, mut jobs) = pending_exit_scheduler_fixture();
        owner.install(scheduler).unwrap();
        assert!(owner.activate_root(&root));
        let executor = Arc::new(ImmediateExitPurgeExecutor::default());
        let maintenance = Arc::new(LocalMaintenanceController::new(
            Arc::clone(&executor),
            Arc::new(ExitMaintenanceStatusStore),
            || OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap(),
        ));
        owner.install_maintenance(Arc::clone(&maintenance)).unwrap();

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
        assert!(maintenance
            .notify_sync_completion(
                "00000000-0000-4000-8000-00000000004b",
                SyncTrigger::Manual,
                true,
            )
            .unwrap());
        tokio::time::timeout(Duration::from_secs(1), async {
            while executor.0.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("failed exit should resume local maintenance admission");
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
            .install_binding(
                &app_data,
                Arc::clone(&catalog),
                Arc::clone(&enqueuer),
                test_binding_service(&app_data),
            )
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
            .install_binding(
                &app_data,
                catalog,
                Arc::clone(&enqueuer),
                test_binding_service(&app_data),
            )
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
            .install_binding(
                &app_data,
                Arc::clone(&catalog),
                Arc::clone(&enqueuer),
                test_binding_service(&app_data),
            )
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
    async fn global_reservation_rejects_bind_before_remote_catalog_or_local_state() {
        let temporary = tempdir().unwrap();
        let app_data = temporary.path().join("app-data");
        let notes_root = temporary.path().join("notes");
        std::fs::create_dir(&notes_root).unwrap();
        let repository_id = "00000000-0000-4000-8000-000000000062";
        let catalog = Arc::new(FakeCatalogValidator::new([repository_metadata(
            repository_id,
            "Reserved remote",
        )]));
        let service = test_binding_service(&app_data);
        let enqueuer = Arc::new(RecordingBindEnqueuer::new(app_data.clone()));
        let owner = DejavuSyncServiceOwner::default();
        owner
            .install_binding(&app_data, Arc::clone(&catalog), enqueuer, service.clone())
            .unwrap();
        let _reservation = service.reserve_global_maintenance().unwrap();

        let result = owner
            .bind_repository(bind_request(notes_root, repository_id, "Reserved remote"))
            .await;

        assert!(matches!(result, Err(RepositoryJobError::Cancelled)));
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
            .install_binding(
                &app_data,
                catalog,
                Arc::clone(&enqueuer),
                test_binding_service(&app_data),
            )
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

    #[tokio::test]
    async fn enqueue_failure_keeps_one_binding_and_exact_retry_recovers_without_duplication() {
        let temporary = tempdir().unwrap();
        let app_data = temporary.path().join("app-data");
        let notes_root = temporary.path().join("notes");
        std::fs::create_dir(&notes_root).unwrap();
        let repository_id = "00000000-0000-4000-8000-000000000060";
        let catalog = Arc::new(FakeCatalogValidator::new([repository_metadata(
            repository_id,
            "Retry remote",
        )]));
        let enqueuer = Arc::new(FailOnceBindEnqueuer::new(app_data.clone()));
        let owner = DejavuSyncServiceOwner::default();
        owner
            .install_binding(
                &app_data,
                catalog,
                Arc::clone(&enqueuer),
                test_binding_service(&app_data),
            )
            .unwrap();
        let request = bind_request(notes_root, repository_id, "Retry remote");

        let Err(first_error) = owner.bind_repository(request.clone()).await else {
            panic!("first enqueue must fail after persisting the binding");
        };
        assert_eq!(first_error, RepositoryJobError::CloudUnavailable);
        let after_failure = LocalSyncStateService::new(&app_data)
            .load()
            .unwrap()
            .unwrap();
        assert_eq!(after_failure.bindings.len(), 1);

        let accepted = owner.bind_repository(request).await.unwrap();
        assert_eq!(accepted.wait_for_completion().await, Ok(()));
        let after_retry = LocalSyncStateService::new(&app_data)
            .load()
            .unwrap()
            .unwrap();
        assert_eq!(after_retry.bindings.len(), 1);
        assert_eq!(enqueuer.attempts.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn local_state_storage_failure_uses_repository_unavailable_safe_code() {
        let temporary = tempdir().unwrap();
        let app_data = temporary.path().join("app-data");
        let notes_root = temporary.path().join("notes");
        std::fs::create_dir_all(app_data.join("local-sync.json")).unwrap();
        std::fs::create_dir(&notes_root).unwrap();
        let repository_id = "00000000-0000-4000-8000-000000000061";
        let catalog = Arc::new(FakeCatalogValidator::new([repository_metadata(
            repository_id,
            "Broken storage",
        )]));
        let enqueuer = Arc::new(RecordingBindEnqueuer::new(app_data.clone()));
        let owner = DejavuSyncServiceOwner::default();
        owner
            .install_binding(
                &app_data,
                catalog,
                enqueuer,
                test_binding_service(&app_data),
            )
            .unwrap();

        let result = owner
            .bind_repository(bind_request(notes_root, repository_id, "Broken storage"))
            .await;
        let Err(error) = result else {
            panic!("unsafe local state storage must reject binding");
        };
        assert_eq!(error.safe_code(), "dejavu-repository-unavailable");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn bind_serializes_state_writes_without_serializing_remote_catalog_reads() {
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
            .install_binding(
                &app_data,
                Arc::clone(&catalog),
                Arc::clone(&enqueuer),
                test_binding_service(&app_data),
            )
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

        assert_eq!(catalog.max_active.load(Ordering::SeqCst), 2);
        let state = LocalSyncStateService::new(&app_data)
            .load()
            .unwrap()
            .unwrap();
        assert_eq!(state.bindings.len(), 2);
        assert_eq!(enqueuer.requests.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn key_state_writer_before_bind_cannot_resurrect_the_old_repository_key() {
        let temporary = tempdir().unwrap();
        let app_data = temporary.path().join("app-data");
        let notes_root = temporary.path().join("notes");
        std::fs::create_dir(&notes_root).unwrap();
        let repository_id = "00000000-0000-4000-8000-000000000058";
        let state_service = LocalSyncStateService::new(&app_data);
        let initial = state_service.load_or_initialize(None).unwrap();
        let old_key = initial.repo_key;
        let replacement_key = STANDARD.encode([0x58_u8; 32]);
        let catalog = Arc::new(FakeCatalogValidator::new([repository_metadata(
            repository_id,
            "Key ordering",
        )]));
        let enqueuer = Arc::new(RecordingBindEnqueuer::new(app_data.clone()));
        let state_gate_service = DejavuSyncService::new(
            Arc::new(PersistedBindingRunner::new(app_data.clone())),
            Arc::new(NoopStatusSink),
        );
        let owner = Arc::new(DejavuSyncServiceOwner::default());
        owner
            .install_binding(
                &app_data,
                catalog,
                Arc::clone(&enqueuer),
                state_gate_service.clone(),
            )
            .unwrap();

        let (writer_entered_tx, writer_entered_rx) = oneshot::channel();
        let (release_writer_tx, release_writer_rx) = oneshot::channel();
        let writer = {
            let app_data = app_data.clone();
            let replacement_key = replacement_key.clone();
            let service = state_gate_service.clone();
            tokio::spawn(async move {
                service
                    .with_global_key_state_transaction(async move {
                        let state_service = LocalSyncStateService::new(app_data);
                        let mut state = state_service.load().unwrap().unwrap();
                        writer_entered_tx.send(()).unwrap();
                        release_writer_rx.await.unwrap();
                        state.repo_key = replacement_key;
                        state_service.save(&state).unwrap();
                    })
                    .await;
            })
        };
        writer_entered_rx.await.unwrap();

        let binding = {
            let owner = Arc::clone(&owner);
            let notes_root = notes_root.clone();
            tokio::spawn(async move {
                owner
                    .bind_repository(bind_request(notes_root, repository_id, "Key ordering"))
                    .await
            })
        };
        assert!(
            tokio::time::timeout(Duration::from_millis(100), enqueuer.enqueued.notified())
                .await
                .is_err(),
            "bind must wait for the service-owned state transaction"
        );
        release_writer_tx.send(()).unwrap();
        writer.await.unwrap();
        binding.await.unwrap().unwrap();

        let final_state = state_service.load().unwrap().unwrap();
        assert_ne!(final_state.repo_key, old_key);
        assert_eq!(final_state.repo_key, replacement_key);
        assert_eq!(final_state.bindings.len(), 1);
        assert_eq!(final_state.bindings[0].repository_id, repository_id);
    }

    #[tokio::test]
    async fn reset_before_enqueue_revalidates_under_global_barrier_without_lock_inversion() {
        let temporary = tempdir().unwrap();
        let app_data = temporary.path().join("app-data");
        let notes_root = temporary.path().join("notes");
        std::fs::create_dir(&notes_root).unwrap();
        let repository_id = "00000000-0000-4000-8000-000000000059";
        let state_service = LocalSyncStateService::new(&app_data);
        state_service.load_or_initialize(None).unwrap();
        let replacement_key = STANDARD.encode([0x59_u8; 32]);
        let catalog = Arc::new(FakeCatalogValidator::new([repository_metadata(
            repository_id,
            "Bind ordering",
        )]));
        let runner = Arc::new(PersistedBindingRunner::new(app_data.clone()));
        let service = DejavuSyncService::new(runner, Arc::new(NoopStatusSink));
        let enqueuer = Arc::new(PausingServiceBindEnqueuer::new());
        let owner = Arc::new(DejavuSyncServiceOwner::default());
        owner
            .install_binding(&app_data, catalog, Arc::clone(&enqueuer), service.clone())
            .unwrap();

        let binding = {
            let owner = Arc::clone(&owner);
            let notes_root = notes_root.clone();
            tokio::spawn(async move {
                owner
                    .bind_repository(bind_request(notes_root, repository_id, "Bind ordering"))
                    .await
            })
        };
        enqueuer.before_enqueue.notified().await;

        let (writer_entered_tx, writer_entered_rx) = oneshot::channel();
        let (release_writer_tx, release_writer_rx) = oneshot::channel();
        let writer = {
            let app_data = app_data.clone();
            let replacement_key = replacement_key.clone();
            let service = service.clone();
            tokio::spawn(async move {
                service
                    .with_global_key_state_transaction(async move {
                        let state_service = LocalSyncStateService::new(app_data);
                        let mut state = state_service.load().unwrap().unwrap();
                        writer_entered_tx.send(()).unwrap();
                        release_writer_rx.await.unwrap();
                        state.repo_key = replacement_key;
                        for binding in &mut state.bindings {
                            binding.enabled = false;
                        }
                        state_service.save(&state).unwrap();
                    })
                    .await;
            })
        };
        tokio::time::timeout(Duration::from_secs(2), writer_entered_rx)
            .await
            .expect("bind must release the state transaction before enqueue")
            .unwrap();

        enqueuer.release_enqueue.notify_one();
        enqueuer.delegating_enqueue.notified().await;
        release_writer_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(2), writer)
            .await
            .expect("key writer must not deadlock")
            .unwrap();
        let result = tokio::time::timeout(Duration::from_secs(2), binding)
            .await
            .expect("bind enqueue must continue after the key writer")
            .unwrap();
        let Err(error) = result else {
            panic!("reset-disabled binding must fail validation before acceptance");
        };
        assert_eq!(error, RepositoryJobError::InvalidBinding);

        let final_state = state_service.load().unwrap().unwrap();
        assert_eq!(final_state.repo_key, replacement_key);
        assert_eq!(final_state.bindings.len(), 1);
        assert_eq!(final_state.bindings[0].repository_id, repository_id);
        assert!(!final_state.bindings[0].enabled);
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
                first_service.clone(),
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
