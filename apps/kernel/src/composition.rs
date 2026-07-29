//! Platform-neutral Kernel service composition.

use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    config::KernelConfig,
    contract::{
        ApiVersion, HostProfile, InstanceId, ReadyHealthResponse, ReadyStatus,
        RuntimeCapabilitiesDto, RuntimeStateDto, StartupState, SystemVersionResponse,
    },
    host::native::NativeHostWorkspaceState,
    paths::KernelPaths,
    ports::KernelPorts,
    runtime::{KernelRuntime, ServiceFailure, SystemApiService},
    services::workspace::WorkspaceService,
    settings::{service::SettingsService, storage::AtomicJsonSettingsStore},
    storage::DurableFileStore,
    workspace::{managed::ManagedWorkspaceCollection, primary::FixedPrimaryWorkspaceStore},
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
    let profile = paths.profile();
    let display_name = workspace_state.display_name().to_owned();
    let managed =
        ManagedWorkspaceCollection::from_paths(&paths).map_err(|_| NativeCompositionError)?;
    let runtime = KernelRuntime::activate(config, paths, KernelPorts::unavailable())
        .map_err(|_| NativeCompositionError)?;
    let settings_store = Arc::new(
        AtomicJsonSettingsStore::new(
            DurableFileStore::at_instance_data(
                runtime.instance_data_root(),
                runtime.launch_epoch(),
            )
            .map_err(|_| NativeCompositionError)?,
        )
        .map_err(|_| NativeCompositionError)?,
    );
    let workspace_service = Arc::new(
        WorkspaceService::new(
            &runtime,
            Arc::new(
                FixedPrimaryWorkspaceStore::new(workspace_state)
                    .map_err(|_| NativeCompositionError)?,
            ),
            managed,
            runtime.event_broker().clone(),
            display_name,
        )
        .await
        .map_err(|_| NativeCompositionError)?,
    );
    let settings_service = Arc::new(SettingsService::new(
        settings_store,
        runtime.event_broker().clone(),
    ));
    settings_service
        .migrate_schema()
        .map_err(|_| NativeCompositionError)?;
    runtime
        .install_workspace_api_service(workspace_service)
        .map_err(|_| NativeCompositionError)?;
    runtime
        .install_settings_api_service(settings_service)
        .map_err(|_| NativeCompositionError)?;
    runtime
        .install_system_api_service(Arc::new(NativeSystemService {
            instance_id: runtime.instance_id(),
            profile,
        }))
        .map_err(|_| NativeCompositionError)?;
    Ok(runtime)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeCompositionError;

impl std::fmt::Display for NativeCompositionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("native Kernel composition failed")
    }
}

impl std::error::Error for NativeCompositionError {}

struct NativeSystemService {
    instance_id: InstanceId,
    profile: HostProfile,
}

#[async_trait]
impl SystemApiService for NativeSystemService {
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
                documents: false,
                history: false,
                search: false,
                settings: true,
                sync: false,
                webdav: false,
                s3: false,
                portable_settings: true,
            },
            instance_id: self.instance_id,
        })
    }
}
