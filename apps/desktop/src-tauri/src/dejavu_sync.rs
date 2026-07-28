pub(crate) mod commands;
pub(crate) mod conflicts;
pub(crate) mod lifecycle;
pub(crate) mod local_state;
pub(crate) mod maintenance;
pub(crate) mod path_guard;
pub(crate) mod repository;
pub(crate) mod scheduler;
pub(crate) mod service;
pub(crate) mod status;

use std::sync::Arc;

use tauri::Manager;
use time::OffsetDateTime;

use self::commands::{DejavuSchedulerOwner, DejavuSyncServiceOwner};
use self::conflicts::ConflictStore;
use self::lifecycle::RepositoryLifecycleController;
use self::local_state::LocalSyncStateService;
use self::maintenance::{
    clean_expired_conflict_history, clean_startup_residue, local_calendar_date_at, os_random_index,
    spawn_production_daily_maintenance, LocalMaintenanceController, LocalPurgeExecutor,
};
use self::path_guard::{tauri_path_guard_factory, PathGuardCoordinatorOwner};
use self::repository::{
    DejavuLocalPurgeRepository, DejavuRepositoryMaintenance, DejavuRepositoryRunner,
    S3RepositoryCatalogValidator, WorkingTreeCoordinatorFactory,
};
use self::scheduler::{DejavuScheduler, LocalRepositoryScheduleSource, SystemDnsFlusher};
use self::service::{DejavuSyncService, RepositoryJobError};
use self::status::{RepositoryStatusStore, TauriRepositoryStatusEmitter};

pub(crate) fn install_production_graph(app: &tauri::AppHandle) -> Result<(), RepositoryJobError> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|_| RepositoryJobError::RepositoryUnavailable)?;
    let startup_bindings = LocalSyncStateService::new(&app_data)
        .load()
        .map_err(RepositoryJobError::from)?
        .map(|state| state.bindings)
        .unwrap_or_default();
    clean_startup_residue(&app_data, &startup_bindings)?;
    let _conflict_cleanup = clean_expired_conflict_history(&app_data, OffsetDateTime::now_utc());

    let path_guard_factory = tauri_path_guard_factory(app.clone());
    app.state::<PathGuardCoordinatorOwner>()
        .install(path_guard_factory.clone())?;
    let coordinator_factory: Arc<dyn WorkingTreeCoordinatorFactory> = Arc::new(path_guard_factory);
    let runner = Arc::new(DejavuRepositoryRunner::new(&app_data, coordinator_factory));
    let status_store = Arc::new(RepositoryStatusStore::new(
        &app_data,
        Arc::new(TauriRepositoryStatusEmitter::new(app.clone())),
    ));
    let service = DejavuSyncService::new(Arc::clone(&runner), Arc::clone(&status_store));
    let local_purge = Arc::new(LocalPurgeExecutor::new(
        Arc::new(DejavuLocalPurgeRepository::new(&app_data)),
        local_calendar_date_at,
        os_random_index,
    ));
    let maintenance = Arc::new(LocalMaintenanceController::new_with_transaction(
        local_purge,
        Arc::clone(&status_store),
        OffsetDateTime::now_utc,
        Arc::new(service.clone()),
    ));
    let scheduler = DejavuScheduler::new_for_tauri(
        Arc::new(LocalRepositoryScheduleSource::new(&app_data)),
        Arc::new(service.clone()),
        Arc::clone(&status_store),
        Arc::new(SystemDnsFlusher),
        Arc::new(OffsetDateTime::now_utc),
    );
    service.install_lifecycle(Arc::new(scheduler.clone()))?;
    service.install_completion_observer(Arc::clone(&maintenance))?;
    let service_owner = app.state::<DejavuSyncServiceOwner>();
    service_owner.install(service.clone())?;
    service_owner.install_maintenance(Arc::clone(&maintenance))?;
    let conflicts = Arc::new(ConflictStore::new(&app_data));
    service_owner.install_conflicts(conflicts)?;
    service_owner.install_binding(
        &app_data,
        Arc::new(S3RepositoryCatalogValidator::new(&app_data)),
        Arc::new(service.clone()),
        service.clone(),
    )?;
    let scheduler_owner = app.state::<DejavuSchedulerOwner>();
    scheduler_owner.install(scheduler)?;
    scheduler_owner.install_maintenance(Arc::clone(&maintenance))?;
    service_owner.install_lifecycle(Arc::new(RepositoryLifecycleController::new(
        &app_data,
        service,
        Arc::new(DejavuRepositoryMaintenance::new(&app_data)),
        Arc::clone(&status_store),
        Arc::new(scheduler_owner.inner().clone()),
        Arc::clone(&maintenance),
    )))?;
    spawn_production_daily_maintenance(&app_data, maintenance);
    Ok(())
}
