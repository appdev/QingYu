//! Platform-neutral Kernel service composition.

use std::{
    fmt,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;

use crate::{
    config::KernelConfig,
    contract::{
        ApiVersion, HostProfile, InstanceId, ReadyHealthResponse, ReadyStatus,
        RuntimeCapabilitiesDto, RuntimeStateDto, StartupState, SystemVersionResponse,
    },
    documents::{
        deletion::WorkspaceRecycleDeletionPort,
        history::{FileDocumentHistoryStore, FileDocumentRecoveryStore},
        service::{CapabilityAtomicInstallPort, WorkspaceDocumentService},
    },
    host::{
        mobile::{MobileKernelDrainError, MobileKernelLaunch, MobileKernelLifecycle},
        native::{NativeHostWorkspaceState, NativeHostWorkspaceStore},
    },
    ignore_rules::{SettingsWorkspaceIgnorePort, WorkspaceIgnorePort},
    paths::{open_or_create_child, KernelPaths},
    ports::system::system_kernel_ports,
    resources::WorkspaceResourceService,
    runtime::{KernelRuntime, ServiceFailure, SystemApiService},
    services::{
        sync::{
            KernelSyncTriggerDisposition, KernelSyncTriggerRejection, SyncRunSettlement,
            SyncService,
        },
        sync_scheduler::KernelSyncScheduler,
        workspace::WorkspaceService,
    },
    settings::{service::SettingsService, storage::AtomicJsonSettingsStore},
    storage::DurableFileStore,
    sync::{config::SyncConfigStore, executor::ProductionSyncExecutor},
    workspace::{
        managed::ManagedWorkspaceCollection,
        primary::{FixedPrimaryWorkspaceStore, PrimaryWorkspaceStore},
    },
};

/// Builds the complete service set currently implemented for a fixed native
/// child launch. The System service is installed last so readiness cannot be
/// published for a partially assembled runtime.
pub async fn compose_fixed_native_kernel(
    config: KernelConfig,
    paths: KernelPaths,
    workspace_state: NativeHostWorkspaceState,
) -> Result<Arc<KernelRuntime>, NativeCompositionError> {
    let (runtime, _services) =
        compose_fixed_native_kernel_services(config, paths, workspace_state).await?;
    Ok(runtime)
}

/// Fully assembled fixed native runtime and the lifecycle services that must
/// remain owned for the duration of a child process.
pub struct NativeRuntimeComposition {
    runtime: Arc<KernelRuntime>,
    lifecycle: NativeKernelLifecycle,
}

impl NativeRuntimeComposition {
    pub fn runtime(&self) -> &Arc<KernelRuntime> {
        &self.runtime
    }

    pub fn shutdown_handle(&self) -> NativeKernelLifecycle {
        self.lifecycle.clone()
    }

    pub async fn shutdown(&self) -> Result<(), NativeKernelShutdownError> {
        self.lifecycle.shutdown().await
    }
}

impl fmt::Debug for NativeRuntimeComposition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("NativeRuntimeComposition(..)")
    }
}

#[derive(Clone)]
pub struct NativeKernelLifecycle {
    completion: Arc<tokio::sync::Notify>,
    scheduler: Arc<KernelSyncScheduler>,
    state: Arc<tokio::sync::Mutex<NativeKernelLifecycleState>>,
    sync: Arc<SyncService>,
}

impl NativeKernelLifecycle {
    fn new(
        scheduler: Arc<KernelSyncScheduler>,
        sync: Arc<SyncService>,
        app_launch_settlement: Option<SyncRunSettlement>,
    ) -> Self {
        Self {
            completion: Arc::new(tokio::sync::Notify::new()),
            scheduler,
            state: Arc::new(tokio::sync::Mutex::new(NativeKernelLifecycleState {
                phase: NativeKernelLifecyclePhase::Idle {
                    app_launch_settlement,
                },
            })),
            sync,
        }
    }

    pub async fn shutdown(&self) -> Result<(), NativeKernelShutdownError> {
        let app_launch_settlement = {
            let mut state = self.state.lock().await;
            match &mut state.phase {
                NativeKernelLifecyclePhase::Idle {
                    app_launch_settlement,
                } => {
                    let settlement = app_launch_settlement.take();
                    state.phase = NativeKernelLifecyclePhase::Draining;
                    Some(settlement)
                }
                NativeKernelLifecyclePhase::Draining => None,
                NativeKernelLifecyclePhase::Drained(result) => return *result,
            }
        };
        if let Some(app_launch_settlement) = app_launch_settlement {
            let completion = self.completion.clone();
            let scheduler = self.scheduler.clone();
            let state = self.state.clone();
            let sync = self.sync.clone();
            tokio::spawn(async move {
                let result =
                    drain_native_kernel(scheduler.as_ref(), sync.as_ref(), app_launch_settlement)
                        .await;
                let mut state = state.lock().await;
                state.phase = NativeKernelLifecyclePhase::Drained(result);
                drop(state);
                completion.notify_waiters();
            });
        }

        loop {
            let completed = self.completion.notified();
            tokio::pin!(completed);
            completed.as_mut().enable();
            {
                let state = self.state.lock().await;
                if let NativeKernelLifecyclePhase::Drained(result) = &state.phase {
                    return *result;
                }
            }
            completed.await;
        }
    }
}

struct NativeKernelLifecycleState {
    phase: NativeKernelLifecyclePhase,
}

enum NativeKernelLifecyclePhase {
    Idle {
        app_launch_settlement: Option<SyncRunSettlement>,
    },
    Draining,
    Drained(Result<(), NativeKernelShutdownError>),
}

async fn drain_native_kernel(
    scheduler: &KernelSyncScheduler,
    sync: &SyncService,
    app_launch_settlement: Option<SyncRunSettlement>,
) -> Result<(), NativeKernelShutdownError> {
    sync.begin_settings_exit_quiescence()
        .map_err(|_error| NativeKernelShutdownError)?;
    scheduler.begin_quiesce();
    if let Some(settlement) = app_launch_settlement {
        settlement.wait().await;
    }
    scheduler.wait_closed().await;
    sync.wait_for_active_run_quiescence()
        .await
        .map_err(|_error| NativeKernelShutdownError)?;

    let (disposition, settlement) = scheduler.settings_exit().await.into_parts();
    match disposition {
        KernelSyncTriggerDisposition::Accepted(_) => {}
        KernelSyncTriggerDisposition::Rejected(
            KernelSyncTriggerRejection::Disabled
            | KernelSyncTriggerRejection::Incomplete
            | KernelSyncTriggerRejection::ModeDisallowed,
        ) => {}
        KernelSyncTriggerDisposition::Rejected(
            KernelSyncTriggerRejection::ActiveRun
            | KernelSyncTriggerRejection::Closing
            | KernelSyncTriggerRejection::Unavailable,
        ) => return Err(NativeKernelShutdownError),
    }
    settlement.wait().await;

    scheduler.begin_close();
    scheduler.wait_closed().await;
    sync.shutdown()
        .await
        .map_err(|_error| NativeKernelShutdownError)
}

impl fmt::Debug for NativeKernelLifecycle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("NativeKernelLifecycle(..)")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeKernelShutdownError;

impl fmt::Display for NativeKernelShutdownError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("native Kernel lifecycle could not drain")
    }
}

impl std::error::Error for NativeKernelShutdownError {}

/// Builds a fixed native runtime together with its process-owned services.
pub async fn compose_fixed_native_kernel_runtime(
    config: KernelConfig,
    paths: KernelPaths,
    workspace_state: NativeHostWorkspaceState,
) -> Result<NativeRuntimeComposition, NativeCompositionError> {
    let (runtime, services) =
        compose_fixed_native_kernel_services(config, paths, workspace_state).await?;
    let sync = services.sync;
    let scheduler = Arc::new(
        KernelSyncScheduler::start(sync.clone()).map_err(|_error| NativeCompositionError)?,
    );
    let (_app_launch_disposition, app_launch_settlement) =
        scheduler.app_launch().await.into_parts();
    Ok(NativeRuntimeComposition {
        runtime,
        lifecycle: NativeKernelLifecycle::new(scheduler, sync, Some(app_launch_settlement)),
    })
}

/// Builds the fixed managed-workspace runtime used by an in-process mobile
/// host. Mobile workspace identity is retained in private instance data while
/// the launch credential remains memory-only.
pub async fn compose_fixed_mobile_kernel(
    config: KernelConfig,
    paths: KernelPaths,
    display_name: impl Into<String>,
) -> Result<MobileKernelLaunch, NativeCompositionError> {
    if paths.profile() != HostProfile::Mobile {
        return Err(NativeCompositionError);
    }
    let workspace_state = NativeHostWorkspaceStore::at_instance_data(
        paths.instance_data_root(),
        config.launch_epoch(),
    )
    .and_then(|store| {
        store.load_or_create(paths.workspace_root().canonical_path(), display_name.into())
    })
    .map_err(|_| NativeCompositionError)?;
    let composition = compose_fixed_native_kernel_runtime(config, paths, workspace_state).await?;
    let runtime = composition.runtime;
    Ok(MobileKernelLaunch::from_composition_parts(
        runtime.clone(),
        Arc::new(MobileNativeKernelLifecycle {
            lifecycle: composition.lifecycle,
            runtime: Mutex::new(Some(runtime)),
        }),
    ))
}

struct MobileNativeKernelLifecycle {
    lifecycle: NativeKernelLifecycle,
    runtime: Mutex<Option<Arc<KernelRuntime>>>,
}

#[async_trait]
impl MobileKernelLifecycle for MobileNativeKernelLifecycle {
    async fn drain(&self) -> Result<(), MobileKernelDrainError> {
        self.lifecycle
            .shutdown()
            .await
            .map_err(|_| MobileKernelDrainError)?;
        self.runtime
            .lock()
            .map_err(|_| MobileKernelDrainError)?
            .take();
        Ok(())
    }
}

async fn compose_fixed_native_kernel_services(
    config: KernelConfig,
    paths: KernelPaths,
    workspace_state: NativeHostWorkspaceState,
) -> Result<(Arc<KernelRuntime>, InstalledFixedKernelServices), NativeCompositionError> {
    let workspace_directory = paths
        .workspace_root()
        .try_clone_dir()
        .map_err(|_| NativeCompositionError)?;
    workspace_state
        .validate_directory(&workspace_directory)
        .map_err(|_| NativeCompositionError)?;
    drop(workspace_directory);
    let workspace_state = workspace_state.into_primary_workspace();
    let display_name = workspace_state.display_name().to_owned();
    let managed =
        ManagedWorkspaceCollection::from_paths(&paths).map_err(|_| NativeCompositionError)?;
    let runtime = KernelRuntime::activate(config, paths, system_kernel_ports())
        .map_err(|_| NativeCompositionError)?;
    let services = install_fixed_kernel_services(
        &runtime,
        Arc::new(
            FixedPrimaryWorkspaceStore::new(workspace_state).map_err(|_| NativeCompositionError)?,
        ),
        managed,
        display_name,
    )
    .await
    .map_err(|_| NativeCompositionError)?;
    Ok((runtime, services))
}

/// Installs the complete fixed-workspace service set after the caller has
/// acquired the runtime's instance and workspace locks.
pub(crate) async fn install_fixed_kernel_services(
    runtime: &Arc<KernelRuntime>,
    primary_workspace: Arc<dyn PrimaryWorkspaceStore>,
    managed: ManagedWorkspaceCollection,
    display_name: impl Into<String>,
) -> Result<InstalledFixedKernelServices, FixedKernelCompositionError> {
    let settings_store = Arc::new(
        AtomicJsonSettingsStore::new(
            DurableFileStore::at_instance_data(
                runtime.instance_data_root(),
                runtime.launch_epoch(),
            )
            .map_err(|_| FixedKernelCompositionError::SettingsStore)?,
        )
        .map_err(|_| FixedKernelCompositionError::SettingsStore)?,
    );
    let workspace_service = Arc::new(
        WorkspaceService::new(
            runtime,
            primary_workspace,
            managed,
            runtime.event_broker().clone(),
            display_name,
        )
        .await
        .map_err(|_| FixedKernelCompositionError::WorkspaceService)?,
    );
    let settings_service = Arc::new(SettingsService::new(
        settings_store,
        runtime.event_broker().clone(),
    ));
    settings_service
        .migrate_schema()
        .map_err(|_| FixedKernelCompositionError::SettingsMigration)?;
    let sync_store = SyncConfigStore::new(
        DurableFileStore::at_instance_data(runtime.instance_data_root(), runtime.launch_epoch())
            .map_err(|_| FixedKernelCompositionError::SyncStore)?,
    )
    .map_err(|_| FixedKernelCompositionError::SyncStore)?;
    let _sync_initialization = sync_store
        .initialize_default_if_absent()
        .map_err(|_| FixedKernelCompositionError::SyncStore)?;
    let sync_store = Arc::new(sync_store);
    let sync_executor = Arc::new(ProductionSyncExecutor::new(
        runtime.clone(),
        settings_service.clone(),
    ));
    let sync_service = Arc::new(SyncService::new(runtime.clone(), sync_store, sync_executor));
    runtime
        .install_workspace_api_service(workspace_service)
        .map_err(|_| FixedKernelCompositionError::WorkspaceInstall)?;
    let workspace = runtime
        .active_workspace_snapshot()
        .map_err(|_| FixedKernelCompositionError::WorkspaceSnapshot)?;
    let documents_root = open_or_create_child(
        &runtime
            .instance_data_root()
            .try_clone_dir()
            .map_err(|_| FixedKernelCompositionError::DocumentStorage)?,
        "documents-v1",
    )
    .map_err(|_| FixedKernelCompositionError::DocumentStorage)?;
    let workspace_documents_root = open_or_create_child(
        &documents_root,
        &workspace.workspace().id.as_uuid().to_string(),
    )
    .map_err(|_| FixedKernelCompositionError::DocumentStorage)?;
    let history_directory = open_or_create_child(&workspace_documents_root, "history")
        .map_err(|_| FixedKernelCompositionError::DocumentStorage)?;
    let recovery_directory = open_or_create_child(&workspace_documents_root, "recovery")
        .map_err(|_| FixedKernelCompositionError::DocumentStorage)?;
    let deletion = Arc::new(
        WorkspaceRecycleDeletionPort::new(
            workspace
                .authority()
                .root()
                .try_clone_dir()
                .map_err(|_| FixedKernelCompositionError::DocumentStorage)?,
        )
        .map_err(|_| FixedKernelCompositionError::DocumentStorage)?,
    );
    let ignore: Arc<dyn WorkspaceIgnorePort> =
        Arc::new(SettingsWorkspaceIgnorePort::new(settings_service.clone()));
    let documents_service = Arc::new(
        WorkspaceDocumentService::new_with_ports(
            runtime,
            deletion.clone(),
            Arc::new(FileDocumentHistoryStore::new(history_directory)),
            Arc::new(FileDocumentRecoveryStore::new(recovery_directory)),
            Arc::new(CapabilityAtomicInstallPort),
            ignore.clone(),
        )
        .map_err(|_| FixedKernelCompositionError::DocumentService)?,
    );
    runtime
        .install_documents_api_service(documents_service)
        .map_err(|_| FixedKernelCompositionError::ServiceInstall)?;
    runtime
        .install_resources_api_service(Arc::new(WorkspaceResourceService::new(runtime, ignore)))
        .map_err(|_| FixedKernelCompositionError::ServiceInstall)?;
    runtime
        .install_settings_api_service(settings_service)
        .map_err(|_| FixedKernelCompositionError::ServiceInstall)?;
    runtime
        .install_sync_api_service(sync_service.clone())
        .map_err(|_| FixedKernelCompositionError::ServiceInstall)?;
    runtime
        .install_system_api_service(Arc::new(FixedSystemService {
            instance_id: runtime.instance_id(),
            profile: runtime.host_profile(),
        }))
        .map_err(|_| FixedKernelCompositionError::ServiceInstall)?;
    Ok(InstalledFixedKernelServices { sync: sync_service })
}

pub(crate) struct InstalledFixedKernelServices {
    pub(crate) sync: Arc<SyncService>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FixedKernelCompositionError {
    SettingsStore,
    SyncStore,
    WorkspaceService,
    SettingsMigration,
    WorkspaceInstall,
    WorkspaceSnapshot,
    DocumentStorage,
    DocumentService,
    ServiceInstall,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeCompositionError;

impl std::fmt::Display for NativeCompositionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("native Kernel composition failed")
    }
}

impl std::error::Error for NativeCompositionError {}

struct FixedSystemService {
    instance_id: InstanceId,
    profile: HostProfile,
}

#[async_trait]
impl SystemApiService for FixedSystemService {
    async fn ready(&self) -> Result<ReadyHealthResponse, ServiceFailure> {
        Ok(ReadyHealthResponse {
            status: ReadyStatus::Ready,
            api_version: ApiVersion::V1,
            instance_id: self.instance_id,
        })
    }

    async fn version(&self) -> Result<SystemVersionResponse, ServiceFailure> {
        Ok(SystemVersionResponse {
            api_version: ApiVersion::V1,
            kernel_version: env!("CARGO_PKG_VERSION").to_owned(),
            instance_id: self.instance_id,
        })
    }

    async fn runtime_state(&self) -> Result<RuntimeStateDto, ServiceFailure> {
        Ok(RuntimeStateDto {
            profile: self.profile,
            startup_state: StartupState::Ready,
            capabilities: RuntimeCapabilitiesDto {
                documents: true,
                history: true,
                resources: true,
                search: true,
                settings: true,
                sync: true,
                webdav: true,
                s3: true,
                portable_settings: true,
            },
            instance_id: self.instance_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, sync::Mutex, time::Duration};

    use async_trait::async_trait;
    use tempfile::tempdir;
    use tokio::sync::Notify;

    use super::*;
    use crate::{
        contract::{
            ErrorCode, ListDocumentsQuery, ListWorkspaceInventoryQuery, PatchSettingsRequest,
            ResourceKind, SearchQuery, SearchWorkspaceQuery, SettingEntryDto, SettingKey,
            SettingValueDto, SyncCompletionState, SyncTrigger, WorkspaceInventoryEntryDto,
            WorkspaceRelativePath,
        },
        host::native::NativeHostWorkspaceState,
        runtime::SyncApiService as _,
        services::sync::{
            KernelSyncTriggerDisposition, KernelSyncTriggerRejection, SyncExecutionError,
            SyncExecutor, SyncRunContext,
        },
        sync::config::SyncConfig,
    };

    #[derive(Default)]
    struct LifecycleBlockingExecutor {
        release: Notify,
        started: Notify,
        triggers: Mutex<Vec<SyncTrigger>>,
    }

    #[async_trait]
    impl SyncExecutor for LifecycleBlockingExecutor {
        async fn test_connection(&self, _config: SyncConfig) -> Result<(), SyncExecutionError> {
            Ok(())
        }

        async fn run(
            &self,
            _config: SyncConfig,
            context: SyncRunContext,
        ) -> Result<crate::contract::SyncSummaryDto, SyncExecutionError> {
            self.triggers.lock().unwrap().push(context.trigger());
            self.started.notify_one();
            self.release.notified().await;
            Ok(crate::contract::SyncSummaryDto::empty())
        }
    }

    fn native_fixture(
        root: &std::path::Path,
    ) -> (KernelPaths, NativeHostWorkspaceState, std::path::PathBuf) {
        let workspace = root.join("workspace");
        let app_data = root.join("app-data");
        let cache = root.join("cache");
        for path in [&workspace, &app_data, &cache] {
            fs::create_dir(path).unwrap();
        }
        let state = NativeHostWorkspaceState::for_workspace(&workspace, "Native").unwrap();
        let paths = KernelPaths::desktop(&workspace, &app_data, &cache).unwrap();
        (paths, state, app_data)
    }

    fn write_startup_exit_config(app_data: &std::path::Path) {
        fs::write(
            app_data.join("sync-config.json"),
            br#"{
  "version": 3,
  "enabled": true,
  "provider": "webdav",
  "remoteRoot": "native-notes",
  "mode": "startup-exit",
  "intervalSeconds": 30,
  "generateConflictDocument": false,
  "webdav": {
    "serverUrl": "http://127.0.0.1:9",
    "username": "native-user",
    "password": "native-password"
  },
  "s3": {
    "endpointUrl": "",
    "region": "",
    "bucket": "",
    "accessKeyId": "",
    "secretAccessKey": "",
    "requestTimeoutSeconds": 60,
    "addressingStyle": "auto",
    "tlsVerification": "verify"
  }
}
"#,
        )
        .unwrap();
    }

    fn write_automatic_config(app_data: &std::path::Path) {
        write_startup_exit_config(app_data);
        let path = app_data.join("sync-config.json");
        let mut config: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        config["mode"] = serde_json::json!("automatic");
        fs::write(path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    }

    async fn controlled_native_lifecycle(
        root: &std::path::Path,
    ) -> (
        Arc<KernelRuntime>,
        Arc<SyncService>,
        NativeKernelLifecycle,
        Arc<LifecycleBlockingExecutor>,
    ) {
        let (paths, workspace_state, app_data) = native_fixture(root);
        write_automatic_config(&app_data);
        let config = KernelConfig::generate().unwrap();
        let durable =
            DurableFileStore::at_instance_data(paths.instance_data_root(), config.launch_epoch())
                .unwrap();
        let (runtime, _installed) =
            compose_fixed_native_kernel_services(config, paths, workspace_state)
                .await
                .unwrap();
        let executor = Arc::new(LifecycleBlockingExecutor::default());
        let sync = Arc::new(SyncService::new(
            runtime.clone(),
            Arc::new(SyncConfigStore::new(durable).unwrap()),
            executor.clone(),
        ));
        let scheduler = Arc::new(KernelSyncScheduler::start(sync.clone()).unwrap());
        let lifecycle = NativeKernelLifecycle::new(scheduler, sync.clone(), None);
        (runtime, sync, lifecycle, executor)
    }

    async fn start_active_lifecycle_run(service: &SyncService, trigger: SyncTrigger) {
        if trigger == SyncTrigger::Manual {
            let revision = service.get_sync_config().await.unwrap().revision;
            service
                .trigger_sync_run(crate::contract::TriggerSyncRunRequest {
                    expected_config_revision: revision,
                })
                .await
                .unwrap();
            return;
        }
        let (disposition, _settlement) = service.trigger_kernel_sync(trigger).await.into_parts();
        assert!(
            matches!(disposition, KernelSyncTriggerDisposition::Accepted(_)),
            "active lifecycle trigger was rejected: {disposition:?}"
        );
    }

    async fn wait_for_lifecycle_trigger_rejection(
        service: &SyncService,
        expected: KernelSyncTriggerRejection,
    ) {
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let (disposition, settlement) = service
                    .trigger_kernel_sync(SyncTrigger::Save)
                    .await
                    .into_parts();
                settlement.wait().await;
                match disposition {
                    KernelSyncTriggerDisposition::Rejected(rejection) if rejection == expected => {
                        return
                    }
                    KernelSyncTriggerDisposition::Rejected(
                        KernelSyncTriggerRejection::ActiveRun,
                    ) => {
                        tokio::task::yield_now().await;
                    }
                    other => panic!("unexpected shutdown gate disposition: {other:?}"),
                }
            }
        })
        .await
        .expect("native lifecycle did not close new sync admission");
    }

    #[tokio::test]
    async fn configured_native_composition_runs_app_launch_sync_before_returning() {
        let temporary = tempdir().unwrap();
        let (paths, state, app_data) = native_fixture(temporary.path());
        write_startup_exit_config(&app_data);

        let composition =
            compose_fixed_native_kernel_runtime(KernelConfig::generate().unwrap(), paths, state)
                .await
                .unwrap();
        let status = composition
            .runtime()
            .sync_api_service()
            .unwrap()
            .get_sync_status()
            .await
            .unwrap();

        assert_eq!(status.last_trigger.as_ref(), Some(&SyncTrigger::AppLaunch));
        composition.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn native_composition_retains_the_only_scheduler_for_its_sync_service() {
        let temporary = tempdir().unwrap();
        let (paths, state, _app_data) = native_fixture(temporary.path());
        let composition =
            compose_fixed_native_kernel_runtime(KernelConfig::generate().unwrap(), paths, state)
                .await
                .unwrap();

        let second = KernelSyncScheduler::start(composition.lifecycle.sync.clone());

        assert_eq!(
            second.unwrap_err(),
            crate::services::sync_scheduler::KernelSyncSchedulerStartError
        );
        composition.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn native_shutdown_runs_settings_exit_and_waits_for_sync_settlement() {
        let temporary = tempdir().unwrap();
        let (paths, state, app_data) = native_fixture(temporary.path());
        write_startup_exit_config(&app_data);
        let composition =
            compose_fixed_native_kernel_runtime(KernelConfig::generate().unwrap(), paths, state)
                .await
                .unwrap();
        let runtime = composition.runtime().clone();

        composition.shutdown().await.unwrap();

        let status = runtime
            .sync_api_service()
            .unwrap()
            .get_sync_status()
            .await
            .unwrap();
        assert_eq!(
            status.last_trigger.as_ref(),
            Some(&SyncTrigger::SettingsExit)
        );
        assert_ne!(status.completion_state, SyncCompletionState::Attempting);
    }

    #[tokio::test]
    async fn native_shutdown_waits_for_active_manual_save_and_interval_before_settings_exit() {
        for trigger in [
            SyncTrigger::Manual,
            SyncTrigger::Save,
            SyncTrigger::Interval,
        ] {
            let temporary = tempdir().unwrap();
            let (_runtime, sync, lifecycle, executor) =
                controlled_native_lifecycle(temporary.path()).await;
            start_active_lifecycle_run(sync.as_ref(), trigger).await;
            tokio::time::timeout(Duration::from_secs(1), executor.started.notified())
                .await
                .expect("active lifecycle run did not reach its executor");

            let shutdown_lifecycle = lifecycle.clone();
            let shutdown = tokio::spawn(async move { shutdown_lifecycle.shutdown().await });
            tokio::task::yield_now().await;
            assert_eq!(executor.triggers.lock().unwrap().as_slice(), [trigger]);

            executor.release.notify_one();
            tokio::time::timeout(Duration::from_secs(1), executor.started.notified())
                .await
                .expect("SettingsExit must run after the active lifecycle run settles");
            executor.release.notify_one();
            tokio::time::timeout(Duration::from_secs(1), shutdown)
                .await
                .expect("native lifecycle shutdown did not settle")
                .unwrap()
                .unwrap();

            assert_eq!(
                executor.triggers.lock().unwrap().as_slice(),
                [trigger, SyncTrigger::SettingsExit]
            );
        }
    }

    #[tokio::test]
    async fn cancelling_first_shutdown_caller_does_not_cancel_the_shared_native_drain() {
        let temporary = tempdir().unwrap();
        let (_runtime, sync, lifecycle, executor) =
            controlled_native_lifecycle(temporary.path()).await;
        start_active_lifecycle_run(sync.as_ref(), SyncTrigger::Manual).await;
        tokio::time::timeout(Duration::from_secs(1), executor.started.notified())
            .await
            .expect("manual run did not reach its executor");

        let first_lifecycle = lifecycle.clone();
        let first = tokio::spawn(async move { first_lifecycle.shutdown().await });
        wait_for_lifecycle_trigger_rejection(sync.as_ref(), KernelSyncTriggerRejection::Closing)
            .await;
        first.abort();
        assert!(first.await.unwrap_err().is_cancelled());

        let second_lifecycle = lifecycle.clone();
        let second = tokio::spawn(async move { second_lifecycle.shutdown().await });
        executor.release.notify_one();
        tokio::time::timeout(Duration::from_secs(1), executor.started.notified())
            .await
            .expect("the shared drain lost SettingsExit after its first caller was cancelled");
        executor.release.notify_one();
        tokio::time::timeout(Duration::from_secs(1), second)
            .await
            .expect("the replacement shutdown caller did not observe shared completion")
            .unwrap()
            .unwrap();

        assert_eq!(
            executor.triggers.lock().unwrap().as_slice(),
            [SyncTrigger::Manual, SyncTrigger::SettingsExit]
        );
    }

    #[tokio::test]
    async fn production_documents_and_resources_share_live_settings_and_workspace_ignore_rules() {
        let temporary = tempdir().unwrap();
        let workspace = temporary.path().join("workspace");
        let app_data = temporary.path().join("app-data");
        let cache = temporary.path().join("cache");
        for path in [&workspace, &app_data, &cache] {
            fs::create_dir(path).unwrap();
        }
        fs::write(workspace.join("workspace-hidden.md"), "hidden").unwrap();
        fs::write(workspace.join("workspace-hidden.bin"), "hidden").unwrap();
        fs::write(workspace.join("global-hidden.md"), "hidden").unwrap();
        fs::write(workspace.join("global-hidden.bin"), "hidden").unwrap();
        fs::write(
            workspace.join(crate::ignore_rules::MARKRA_IGNORE_FILE_NAME),
            "workspace-hidden.md\nworkspace-hidden.bin\n",
        )
        .unwrap();
        let state = NativeHostWorkspaceState::for_workspace(&workspace, "Composition").unwrap();
        let paths = KernelPaths::desktop(&workspace, &app_data, &cache).unwrap();
        let runtime = compose_fixed_native_kernel(KernelConfig::generate().unwrap(), paths, state)
            .await
            .unwrap();
        let resources = runtime.resources_api_service().unwrap().clone();
        let documents = runtime.documents_api_service().unwrap().clone();
        let settings = runtime.settings_api_service().unwrap().clone();
        let inventory_query = ListWorkspaceInventoryQuery {
            cursor: None,
            limit: None,
            parent: WorkspaceRelativePath::default(),
        };

        let first_inventory = resources
            .list_workspace_inventory(inventory_query.clone())
            .await
            .unwrap();
        let global_resource = first_inventory
            .items
            .iter()
            .find_map(|entry| match entry {
                WorkspaceInventoryEntryDto::Resource { resource }
                    if resource.path.as_str() == "global-hidden.bin" =>
                {
                    Some(resource.clone())
                }
                _ => None,
            })
            .unwrap();
        assert!(first_inventory
            .items
            .iter()
            .all(|entry| entry.path().as_str() != "workspace-hidden.bin"));
        let first_documents = documents
            .list_documents(ListDocumentsQuery {
                cursor: None,
                limit: None,
                parent: WorkspaceRelativePath::default(),
            })
            .await
            .unwrap();
        assert!(first_documents
            .items
            .iter()
            .all(|entry| entry.path.as_str() != "workspace-hidden.md"));
        assert!(first_documents
            .items
            .iter()
            .any(|entry| entry.path.as_str() == "global-hidden.md"));

        let current = settings.get_settings().await.unwrap();
        settings
            .patch_settings(PatchSettingsRequest {
                expected_revision: current.revision,
                values: vec![SettingEntryDto {
                    key: SettingKey::FilesIgnoreRules,
                    value: SettingValueDto::String {
                        value: "global-hidden.md\nglobal-hidden.bin\n".to_string(),
                    },
                }],
            })
            .await
            .unwrap();

        let next_inventory = resources
            .list_workspace_inventory(inventory_query)
            .await
            .unwrap();
        assert!(next_inventory
            .items
            .iter()
            .all(|entry| entry.path().as_str() != "global-hidden.bin"));
        let next_documents = documents
            .list_documents(ListDocumentsQuery {
                cursor: None,
                limit: None,
                parent: WorkspaceRelativePath::default(),
            })
            .await
            .unwrap();
        assert!(next_documents
            .items
            .iter()
            .all(|entry| entry.path.as_str() != "global-hidden.md"));
        let error = resources
            .open_workspace_resource(global_resource.id.clone(), ResourceKind::Attachment)
            .await
            .unwrap_err();
        assert_eq!(error.code(), ErrorCode::ResourceNotFound);

        fs::remove_file(workspace.join(crate::ignore_rules::MARKRA_IGNORE_FILE_NAME)).unwrap();
        fs::create_dir(workspace.join(crate::ignore_rules::MARKRA_IGNORE_FILE_NAME)).unwrap();
        let inventory_error = resources
            .list_workspace_inventory(ListWorkspaceInventoryQuery {
                cursor: None,
                limit: None,
                parent: WorkspaceRelativePath::default(),
            })
            .await
            .unwrap_err();
        let documents_error = documents
            .list_documents(ListDocumentsQuery {
                cursor: None,
                limit: None,
                parent: WorkspaceRelativePath::default(),
            })
            .await
            .unwrap_err();
        let search_error = documents
            .search_workspace(SearchWorkspaceQuery {
                cursor: None,
                limit: None,
                query: SearchQuery::parse("hidden").unwrap(),
            })
            .await
            .unwrap_err();
        let open_error = resources
            .open_workspace_resource(global_resource.id, ResourceKind::Attachment)
            .await
            .unwrap_err();
        for error in [inventory_error, documents_error, search_error, open_error] {
            assert_eq!(error.code(), ErrorCode::WorkspaceUnavailable);
        }

        drop(runtime);
    }
}
