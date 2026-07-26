pub(crate) mod commands;
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
use self::path_guard::{tauri_path_guard_factory, PathGuardCoordinatorOwner};
use self::repository::{
    DejavuRepositoryRunner, S3RepositoryCatalogValidator, WorkingTreeCoordinatorFactory,
};
use self::scheduler::{DejavuScheduler, LocalRepositoryScheduleSource, SystemDnsFlusher};
use self::service::{DejavuSyncService, RepositoryJobError};
use self::status::{RepositoryStatusStore, TauriRepositoryStatusEmitter};

pub(crate) fn install_production_graph(app: &tauri::AppHandle) -> Result<(), RepositoryJobError> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|_| RepositoryJobError::RepositoryUnavailable)?;
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
    let scheduler = DejavuScheduler::new(
        Arc::new(LocalRepositoryScheduleSource::new(&app_data)),
        Arc::new(service.clone()),
        Arc::clone(&status_store),
        Arc::new(SystemDnsFlusher),
        Arc::new(OffsetDateTime::now_utc),
    );
    service.install_lifecycle(Arc::new(scheduler.clone()))?;
    let service_owner = app.state::<DejavuSyncServiceOwner>();
    service_owner.install(service.clone())?;
    let state_transaction = service.local_state_transaction();
    service_owner.install_binding(
        &app_data,
        Arc::new(S3RepositoryCatalogValidator::new(&app_data)),
        Arc::new(service),
        state_transaction,
    )?;
    app.state::<DejavuSchedulerOwner>().install(scheduler)?;
    Ok(())
}
