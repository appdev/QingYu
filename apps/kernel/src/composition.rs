//! Platform-neutral Kernel service composition.

use std::{fmt, sync::Arc};

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
    host::native::NativeHostWorkspaceState,
    ignore_rules::{SettingsWorkspaceIgnorePort, WorkspaceIgnorePort},
    paths::{open_or_create_child, KernelPaths},
    ports::system::system_kernel_ports,
    resources::WorkspaceResourceService,
    runtime::{KernelRuntime, ServiceFailure, SystemApiService},
    services::{
        sync::{SyncRunSettlement, SyncService},
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
    scheduler: Arc<KernelSyncScheduler>,
    state: Arc<tokio::sync::Mutex<NativeKernelLifecycleState>>,
    sync: Arc<SyncService>,
}

impl NativeKernelLifecycle {
    pub async fn shutdown(&self) -> Result<(), NativeKernelShutdownError> {
        let mut state = self.state.lock().await;
        if state.drained {
            return Ok(());
        }
        if let Some(settlement) = state.app_launch_settlement.take() {
            settlement.wait().await;
        }
        let (_disposition, settlement) = self.scheduler.settings_exit().await.into_parts();
        settlement.wait().await;
        self.scheduler.begin_close();
        let ((), sync) = tokio::join!(self.scheduler.wait_closed(), self.sync.shutdown());
        sync.map_err(|_error| NativeKernelShutdownError)?;
        state.drained = true;
        Ok(())
    }
}

struct NativeKernelLifecycleState {
    app_launch_settlement: Option<SyncRunSettlement>,
    drained: bool,
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
        lifecycle: NativeKernelLifecycle {
            scheduler,
            state: Arc::new(tokio::sync::Mutex::new(NativeKernelLifecycleState {
                app_launch_settlement: Some(app_launch_settlement),
                drained: false,
            })),
            sync,
        },
    })
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
    use std::fs;

    use tempfile::tempdir;

    use super::*;
    use crate::{
        contract::{
            ErrorCode, ListDocumentsQuery, ListWorkspaceInventoryQuery, PatchSettingsRequest,
            ResourceKind, SearchQuery, SearchWorkspaceQuery, SettingEntryDto, SettingKey,
            SettingValueDto, SyncCompletionState, SyncTrigger, WorkspaceInventoryEntryDto,
            WorkspaceRelativePath,
        },
        host::native::NativeHostWorkspaceState,
    };

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
