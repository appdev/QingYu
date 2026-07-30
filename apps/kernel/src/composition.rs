//! Platform-neutral Kernel service composition.

use std::sync::Arc;

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
    services::{sync::SyncService, workspace::WorkspaceService},
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
    install_fixed_kernel_services(
        &runtime,
        Arc::new(
            FixedPrimaryWorkspaceStore::new(workspace_state).map_err(|_| NativeCompositionError)?,
        ),
        managed,
        display_name,
    )
    .await
    .map_err(|_| NativeCompositionError)?;
    Ok(runtime)
}

/// Installs the complete fixed-workspace service set after the caller has
/// acquired the runtime's instance and workspace locks.
pub(crate) async fn install_fixed_kernel_services(
    runtime: &Arc<KernelRuntime>,
    primary_workspace: Arc<dyn PrimaryWorkspaceStore>,
    managed: ManagedWorkspaceCollection,
    display_name: impl Into<String>,
) -> Result<(), FixedKernelCompositionError> {
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
    let sync_store = Arc::new(
        SyncConfigStore::new(
            DurableFileStore::at_instance_data(
                runtime.instance_data_root(),
                runtime.launch_epoch(),
            )
            .map_err(|_| FixedKernelCompositionError::SyncStore)?,
        )
        .map_err(|_| FixedKernelCompositionError::SyncStore)?,
    );
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
        .install_resources_api_service(Arc::new(WorkspaceResourceService::new(&runtime, ignore)))
        .map_err(|_| FixedKernelCompositionError::ServiceInstall)?;
    runtime
        .install_settings_api_service(settings_service)
        .map_err(|_| FixedKernelCompositionError::ServiceInstall)?;
    runtime
        .install_sync_api_service(sync_service)
        .map_err(|_| FixedKernelCompositionError::ServiceInstall)?;
    runtime
        .install_system_api_service(Arc::new(FixedSystemService {
            instance_id: runtime.instance_id(),
            profile: runtime.host_profile(),
        }))
        .map_err(|_| FixedKernelCompositionError::ServiceInstall)?;
    Ok(())
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
            SettingValueDto, WorkspaceInventoryEntryDto, WorkspaceRelativePath,
        },
        host::native::NativeHostWorkspaceState,
    };

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
