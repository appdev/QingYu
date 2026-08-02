//! Fixed production composition for the single-user Server host.

use std::{fmt, sync::Arc, time::Duration};

use crate::{
    api::{ServerApiActivation, ServerApiProcess},
    composition::{install_fixed_kernel_services, FixedKernelCompositionError},
    config::KernelConfig,
    paths::KernelPaths,
    ports::system::system_kernel_ports,
    runtime::KernelRuntime,
    services::{sync::SyncService, sync_scheduler::KernelSyncScheduler},
    sync::local_state::initialize_server_dejavu_binding,
    workspace::{managed::ManagedWorkspaceCollection, primary::DurableServerPrimaryWorkspaceStore},
};

use super::{
    AuthenticationRateLimiter, RateLimitPolicy, ServerAuthenticationSecurity,
    ServerAuthenticationStore, ServerLaunchEnvironment, SessionPolicy, SessionStore,
};

const SERVER_WORKSPACE_DISPLAY_NAME: &str = "Notes";
const SERVER_BLOCKING_CAPACITY: usize = 4;
const SERVER_MAXIMUM_AUTH_CLIENTS_PER_FLOW: usize = 64;
const SERVER_MAXIMUM_AUTH_ATTEMPTS_IN_FLIGHT: usize = 4;
const SERVER_LOGIN_MAXIMUM_FAILURES: u32 = 5;
const SERVER_INITIALIZATION_MAXIMUM_FAILURES: u32 = 3;
const SERVER_AUTH_OBSERVATION_WINDOW: Duration = Duration::from_secs(5 * 60);
const SERVER_AUTH_LOCKOUT: Duration = Duration::from_secs(15 * 60);
const SERVER_SESSION_LIFETIME: Duration = Duration::from_secs(12 * 60 * 60);
const SERVER_MAXIMUM_ACTIVE_SESSIONS: usize = 8;

#[derive(Clone)]
pub struct ServerKernelLifecycle {
    scheduler: Arc<KernelSyncScheduler>,
    sync: Arc<SyncService>,
}

impl ServerKernelLifecycle {
    pub async fn shutdown(&self) -> Result<(), ServerKernelShutdownError> {
        self.scheduler.begin_close();
        let ((), sync) = tokio::join!(self.scheduler.wait_closed(), self.sync.shutdown());
        sync.map_err(|_error| ServerKernelShutdownError)
    }

    #[cfg(test)]
    fn scheduler(&self) -> &Arc<KernelSyncScheduler> {
        &self.scheduler
    }
}

impl fmt::Debug for ServerKernelLifecycle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ServerKernelLifecycle(..)")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServerKernelShutdownError;

impl fmt::Display for ServerKernelShutdownError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Server Kernel lifecycle could not drain")
    }
}

impl std::error::Error for ServerKernelShutdownError {}

/// Fully assembled fixed Server runtime and its unique authentication owner.
pub struct ServerRuntimeComposition {
    runtime: Arc<KernelRuntime>,
    _authentication: Arc<ServerAuthenticationStore>,
    security: ServerAuthenticationSecurity,
    lifecycle: ServerKernelLifecycle,
}

impl ServerRuntimeComposition {
    /// Consumes the composition into the one process-wide Server API owner.
    /// A failed activation is fail-closed until the process is restarted.
    pub fn activate_api(
        self,
        environment: ServerLaunchEnvironment,
    ) -> Result<ServerApiActivation, ServerRuntimeCompositionError> {
        let process = ServerApiProcess::new(self.runtime, SERVER_BLOCKING_CAPACITY)
            .map_err(|_| ServerRuntimeCompositionError::ApiActivation)?;
        process
            .activate(self.security, environment)
            .map(|activation| activation.with_kernel_lifecycle(self.lifecycle))
            .map_err(|_| ServerRuntimeCompositionError::ApiActivation)
    }

    pub fn shutdown_handle(&self) -> ServerKernelLifecycle {
        self.lifecycle.clone()
    }

    #[cfg(test)]
    fn runtime(&self) -> &Arc<KernelRuntime> {
        &self.runtime
    }

    #[cfg(test)]
    fn authentication_status(
        &self,
    ) -> Result<super::ServerAuthenticationStatus, ServerRuntimeCompositionError> {
        self._authentication
            .status()
            .map_err(|_| ServerRuntimeCompositionError::AuthenticationStore)
    }

    #[cfg(test)]
    fn initialization_coordinator(
        &self,
        environment: ServerLaunchEnvironment,
    ) -> Result<super::ServerInitializationCoordinator, super::ServerInitializationCoordinatorError>
    {
        environment.into_initialization_owner(&self.security)
    }

    #[cfg(test)]
    fn authentication_coordinator(&self) -> super::ServerAuthenticationCoordinator {
        self.security.authentication_coordinator()
    }

    #[cfg(test)]
    fn scheduler(&self) -> &Arc<KernelSyncScheduler> {
        self.lifecycle.scheduler()
    }
}

impl fmt::Debug for ServerRuntimeComposition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ServerRuntimeComposition(..)")
    }
}

/// Activates the fixed Server layout and installs all currently supported
/// services before constructing the one production authentication owner.
pub async fn compose_fixed_server_kernel(
    config: KernelConfig,
    paths: KernelPaths,
) -> Result<ServerRuntimeComposition, ServerRuntimeCompositionError> {
    let managed = ManagedWorkspaceCollection::from_paths(&paths)
        .map_err(|_| ServerRuntimeCompositionError::ManagedPaths)?;
    let runtime = KernelRuntime::activate(config, paths, system_kernel_ports())
        .map_err(|_| ServerRuntimeCompositionError::RuntimeActivation)?;
    let workspace_authority = runtime
        .active_workspace_authority()
        .map_err(|_| ServerRuntimeCompositionError::DejavuBinding)?;
    let instance_authority = runtime.active_instance_authority();
    initialize_server_dejavu_binding(
        instance_authority.as_ref(),
        workspace_authority.as_ref(),
        runtime.launch_epoch(),
    )
    .map_err(|_| ServerRuntimeCompositionError::DejavuBinding)?;
    let primary_workspace = Arc::new(
        DurableServerPrimaryWorkspaceStore::open(
            runtime.instance_data_root(),
            runtime.launch_epoch(),
        )
        .map_err(|_| ServerRuntimeCompositionError::PrimaryWorkspaceStore)?,
    );
    let services = install_fixed_kernel_services(
        &runtime,
        primary_workspace,
        managed,
        SERVER_WORKSPACE_DISPLAY_NAME,
    )
    .await
    .map_err(ServerRuntimeCompositionError::from_fixed_services)?;
    let authentication = Arc::new(
        ServerAuthenticationStore::open(runtime.config_root())
            .map_err(|_| ServerRuntimeCompositionError::AuthenticationStore)?,
    );
    let security = production_authentication_security(Arc::clone(&authentication))?;
    let sync = services.sync;
    let scheduler = Arc::new(
        KernelSyncScheduler::start(sync.clone())
            .map_err(|_error| ServerRuntimeCompositionError::SyncScheduler)?,
    );
    let _app_launch = scheduler.app_launch().await;
    Ok(ServerRuntimeComposition {
        runtime,
        _authentication: authentication,
        security,
        lifecycle: ServerKernelLifecycle { scheduler, sync },
    })
}

fn production_authentication_security(
    authentication: Arc<ServerAuthenticationStore>,
) -> Result<ServerAuthenticationSecurity, ServerRuntimeCompositionError> {
    let login = RateLimitPolicy::new(
        SERVER_LOGIN_MAXIMUM_FAILURES,
        SERVER_AUTH_OBSERVATION_WINDOW,
        SERVER_AUTH_LOCKOUT,
    )
    .map_err(|_| ServerRuntimeCompositionError::AuthenticationSecurity)?;
    let initialization = RateLimitPolicy::new(
        SERVER_INITIALIZATION_MAXIMUM_FAILURES,
        SERVER_AUTH_OBSERVATION_WINDOW,
        SERVER_AUTH_LOCKOUT,
    )
    .map_err(|_| ServerRuntimeCompositionError::AuthenticationSecurity)?;
    let rate_limiter = AuthenticationRateLimiter::with_capacity(
        login,
        initialization,
        SERVER_MAXIMUM_AUTH_CLIENTS_PER_FLOW,
        SERVER_MAXIMUM_AUTH_ATTEMPTS_IN_FLIGHT,
    )
    .map_err(|_| ServerRuntimeCompositionError::AuthenticationSecurity)?;
    let sessions = SessionStore::new(
        SessionPolicy::with_capacity(SERVER_SESSION_LIFETIME, SERVER_MAXIMUM_ACTIVE_SESSIONS)
            .map_err(|_| ServerRuntimeCompositionError::AuthenticationSecurity)?,
    );
    ServerAuthenticationSecurity::claim(authentication, rate_limiter, sessions)
        .map_err(|_| ServerRuntimeCompositionError::AuthenticationSecurity)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServerRuntimeCompositionError {
    ManagedPaths,
    RuntimeActivation,
    DejavuBinding,
    PrimaryWorkspaceStore,
    FixedSettingsStore,
    FixedSyncStore,
    FixedWorkspaceService,
    FixedWorkspaceInstall,
    FixedWorkspaceSnapshot,
    FixedDocumentStorage,
    FixedDocumentService,
    FixedResourceRecovery,
    FixedServiceInstall,
    AuthenticationStore,
    AuthenticationSecurity,
    SyncScheduler,
    ApiActivation,
}

impl ServerRuntimeCompositionError {
    const fn from_fixed_services(error: FixedKernelCompositionError) -> Self {
        match error {
            FixedKernelCompositionError::SettingsStore => Self::FixedSettingsStore,
            FixedKernelCompositionError::SyncStore => Self::FixedSyncStore,
            FixedKernelCompositionError::WorkspaceService => Self::FixedWorkspaceService,
            FixedKernelCompositionError::WorkspaceInstall => Self::FixedWorkspaceInstall,
            FixedKernelCompositionError::WorkspaceSnapshot => Self::FixedWorkspaceSnapshot,
            FixedKernelCompositionError::DocumentStorage => Self::FixedDocumentStorage,
            FixedKernelCompositionError::DocumentService => Self::FixedDocumentService,
            FixedKernelCompositionError::ResourceRecovery => Self::FixedResourceRecovery,
            FixedKernelCompositionError::ServiceInstall => Self::FixedServiceInstall,
        }
    }

    pub const fn diagnostic_code(self) -> &'static str {
        match self {
            Self::ManagedPaths => "QK-SRV-COMPOSE-MANAGED-PATHS",
            Self::RuntimeActivation => "QK-SRV-COMPOSE-RUNTIME-ACTIVATE",
            Self::DejavuBinding => "QK-SRV-COMPOSE-DEJAVU-BINDING",
            Self::PrimaryWorkspaceStore => "QK-SRV-COMPOSE-PRIMARY-WORKSPACE",
            Self::FixedSettingsStore => "QK-SRV-COMPOSE-FIXED-SETTINGS-STORE",
            Self::FixedSyncStore => "QK-SRV-COMPOSE-FIXED-SYNC-STORE",
            Self::FixedWorkspaceService => "QK-SRV-COMPOSE-FIXED-WORKSPACE",
            Self::FixedWorkspaceInstall => "QK-SRV-COMPOSE-FIXED-WORKSPACE-INSTALL",
            Self::FixedWorkspaceSnapshot => "QK-SRV-COMPOSE-FIXED-WORKSPACE-SNAPSHOT",
            Self::FixedDocumentStorage => "QK-SRV-COMPOSE-FIXED-DOCUMENT-STORAGE",
            Self::FixedDocumentService => "QK-SRV-COMPOSE-FIXED-DOCUMENT-SERVICE",
            Self::FixedResourceRecovery => "QK-SRV-COMPOSE-FIXED-RESOURCE-RECOVERY",
            Self::FixedServiceInstall => "QK-SRV-COMPOSE-FIXED-SERVICE-INSTALL",
            Self::AuthenticationStore => "QK-SRV-COMPOSE-AUTH-STORE",
            Self::AuthenticationSecurity => "QK-SRV-COMPOSE-AUTH-SECURITY",
            Self::SyncScheduler => "QK-SRV-COMPOSE-SYNC-SCHEDULER",
            Self::ApiActivation => "QK-SRV-AUTH-API",
        }
    }
}

impl fmt::Display for ServerRuntimeCompositionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("fixed Server runtime composition failed")
    }
}

impl std::error::Error for ServerRuntimeCompositionError {}

#[cfg(test)]
mod tests {
    use std::{fs, time::Duration};

    use tempfile::tempdir;

    use crate::{
        api::{build_server_router, TransportPolicy},
        config::KernelConfig,
        contract::{
            HostProfile, PatchSettingsRequest, PatchSyncConfigRequest, ServerAuthenticationSecret,
            SettingEntryDto, SettingKey, SettingValueDto, SyncConfigChangesDto, SyncTrigger,
        },
        paths::KernelPaths,
        server::{
            ServerAuthenticationStatus, ServerInitializationCoordinatorError,
            ServerLaunchEnvironment,
        },
        sync::local_state::read_active_dejavu_binding,
    };

    use super::*;

    const INITIALIZATION_TOKEN: &str = "server-runtime-initialization-token-at-least-32-bytes";
    const OWNER_PASSWORD: &str = "Correct-Horse-Battery-Staple!7";

    fn fixture_paths(root: &std::path::Path) -> KernelPaths {
        let data = root.join("data");
        let cache = root.join("cache");
        fs::create_dir_all(&data).unwrap();
        crate::paths::ServerPathLayout::for_test(&data, &cache)
            .activate()
            .unwrap()
    }

    fn launch_environment(token: Option<&str>) -> ServerLaunchEnvironment {
        ServerLaunchEnvironment::from_lookup(|name| match token {
            Some(token) if name == crate::server::SERVER_INITIALIZATION_TOKEN_ENV => {
                Some(std::ffi::OsString::from(token))
            }
            Some(_) | None => None,
        })
        .unwrap()
    }

    #[tokio::test]
    async fn composition_installs_complete_server_services_after_lock_acquisition() {
        let temporary = tempdir().unwrap();
        let paths = fixture_paths(temporary.path());
        let composition = compose_fixed_server_kernel(KernelConfig::generate().unwrap(), paths)
            .await
            .unwrap();

        assert_eq!(composition.runtime().host_profile(), HostProfile::Server);
        let state = composition
            .runtime()
            .system_api_service()
            .unwrap()
            .runtime_state()
            .await
            .unwrap();
        assert!(state.capabilities.documents);
        assert!(state.capabilities.history);
        assert!(state.capabilities.resources);
        assert!(state.capabilities.search);
        assert!(state.capabilities.settings);
        assert!(state.capabilities.sync);
        assert!(state.capabilities.webdav);
        assert!(state.capabilities.s3);
        assert_eq!(
            composition.authentication_status().unwrap(),
            ServerAuthenticationStatus::NeedsInitialization
        );
    }

    #[tokio::test]
    async fn fixed_server_composition_writes_configuration_only_to_config_root() {
        let temporary = tempdir().unwrap();
        let paths = fixture_paths(temporary.path());
        let legacy = temporary.path().join("data/state/settings.json");
        fs::write(&legacy, br#"{"language":"ja"}"#).unwrap();

        let composition = compose_fixed_server_kernel(KernelConfig::generate().unwrap(), paths)
            .await
            .unwrap();
        let settings = composition.runtime().settings_api_service().unwrap();
        let before = settings.get_settings().await.unwrap();
        assert_eq!(
            &before
                .values
                .iter()
                .find(|entry| entry.key == SettingKey::Language)
                .unwrap()
                .value,
            &SettingValueDto::String {
                value: "en".to_owned(),
            },
        );

        settings
            .patch_settings(PatchSettingsRequest {
                expected_revision: before.revision,
                values: vec![SettingEntryDto {
                    key: SettingKey::Language,
                    value: SettingValueDto::String {
                        value: "zh-CN".to_owned(),
                    },
                }],
            })
            .await
            .unwrap();

        assert!(temporary.path().join("data/config/settings.json").is_file());
        assert!(temporary
            .path()
            .join("data/config/sync-config.json")
            .is_file());
        assert_eq!(fs::read(&legacy).unwrap(), br#"{"language":"ja"}"#);
        assert!(!temporary
            .path()
            .join("data/state/sync-config.json")
            .exists());
    }

    #[tokio::test]
    async fn fresh_server_owns_one_private_stable_dejavu_binding_before_ready() {
        let temporary = tempdir().unwrap();
        let paths = fixture_paths(temporary.path());
        let first = compose_fixed_server_kernel(KernelConfig::generate().unwrap(), paths)
            .await
            .unwrap();
        let target = temporary.path().join("data/state/local-sync.json");
        let first_bytes = fs::read(&target).expect("fresh Server DejaVu binding state");
        assert!(first_bytes.len() <= 1024 * 1024);
        let first_workspace = first.runtime().active_workspace_snapshot().unwrap();
        let (repository_id, _display_name, device_id, repository_key) = read_active_dejavu_binding(
            first.runtime().instance_data_root(),
            first_workspace.authority().root(),
        )
        .expect("valid Server binding state")
        .expect("enabled fixed-workspace binding")
        .into_parts();
        for identifier in [&repository_id, &device_id] {
            let parsed = uuid::Uuid::parse_str(identifier).expect("canonical UUID");
            assert_eq!(parsed.get_version(), Some(uuid::Version::Random));
            assert_eq!(parsed.to_string(), *identifier);
        }
        assert_eq!(
            format!("{repository_key:?}"),
            "DejavuRepositoryKey([REDACTED])"
        );
        let json: serde_json::Value = serde_json::from_slice(&first_bytes).unwrap();
        assert_eq!(
            base64::Engine::decode(
                &base64::engine::general_purpose::STANDARD,
                json["repoKey"].as_str().unwrap(),
            )
            .unwrap()
            .len(),
            32
        );
        assert_eq!(
            json["bindings"][0]["notesRoot"],
            temporary
                .path()
                .join("data/workspace")
                .canonicalize()
                .unwrap()
                .to_str()
                .unwrap()
        );
        assert_eq!(json["bindings"][0]["enabled"], true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                fs::metadata(&target).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        drop(first_workspace);
        drop(first);

        let restarted = compose_fixed_server_kernel(
            KernelConfig::generate().unwrap(),
            fixture_paths(temporary.path()),
        )
        .await
        .unwrap();
        assert_eq!(fs::read(target).unwrap(), first_bytes);
        let restarted_workspace = restarted.runtime().active_workspace_snapshot().unwrap();
        let restarted_binding = read_active_dejavu_binding(
            restarted.runtime().instance_data_root(),
            restarted_workspace.authority().root(),
        )
        .unwrap()
        .unwrap();
        let (restarted_repository, _display_name, restarted_device, _) =
            restarted_binding.into_parts();
        assert_eq!(restarted_repository, repository_id);
        assert_eq!(restarted_device, device_id);
    }

    #[tokio::test]
    async fn server_fails_closed_without_replacing_an_existing_inactive_dejavu_state() {
        let temporary = tempdir().unwrap();
        let paths = fixture_paths(temporary.path());
        let target = temporary.path().join("data/state/local-sync.json");
        let existing = serde_json::to_vec(&serde_json::json!({
            "version": 1,
            "deviceId": "eb473600-dace-4d7e-bdad-7dac05933099",
            "repoKey": base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                [7_u8; 32],
            ),
            "bindings": [{
                "repositoryId": "323df833-764a-44b3-a534-492640c258f2",
                "displayName": "Server notes",
                "notesRoot": temporary.path().join("data/workspace"),
                "enabled": false,
            }],
        }))
        .unwrap();
        fs::write(&target, &existing).unwrap();

        let error = compose_fixed_server_kernel(KernelConfig::generate().unwrap(), paths)
            .await
            .unwrap_err();

        assert_eq!(error, ServerRuntimeCompositionError::DejavuBinding);
        assert_eq!(error.diagnostic_code(), "QK-SRV-COMPOSE-DEJAVU-BINDING");
        assert_eq!(fs::read(target).unwrap(), existing);
        assert!(!error.to_string().contains("eb473600"));
    }

    #[tokio::test]
    async fn server_never_replaces_invalid_oversized_or_nonregular_dejavu_state() {
        for scenario in ["unknown-field", "oversized", "directory"] {
            let temporary = tempdir().unwrap();
            let paths = fixture_paths(temporary.path());
            let target = temporary.path().join("data/state/local-sync.json");
            let expected = match scenario {
                "unknown-field" => {
                    let bytes = serde_json::to_vec(&serde_json::json!({
                        "version": 1,
                        "deviceId": "eb473600-dace-4d7e-bdad-7dac05933099",
                        "repoKey": base64::Engine::encode(
                            &base64::engine::general_purpose::STANDARD,
                            [7_u8; 32],
                        ),
                        "bindings": [{
                            "repositoryId": "323df833-764a-44b3-a534-492640c258f2",
                            "displayName": "Server notes",
                            "notesRoot": temporary.path().join("data/workspace"),
                            "enabled": true,
                        }],
                        "private-invalid-marker": true,
                    }))
                    .unwrap();
                    fs::write(&target, &bytes).unwrap();
                    bytes
                }
                "oversized" => {
                    let bytes = vec![b'X'; 1024 * 1024 + 1];
                    fs::write(&target, &bytes).unwrap();
                    bytes
                }
                "directory" => {
                    fs::create_dir(&target).unwrap();
                    fs::write(target.join("private-marker"), b"untouched").unwrap();
                    Vec::new()
                }
                _ => unreachable!(),
            };

            let error = compose_fixed_server_kernel(KernelConfig::generate().unwrap(), paths)
                .await
                .unwrap_err();

            assert_eq!(error, ServerRuntimeCompositionError::DejavuBinding);
            assert_eq!(error.diagnostic_code(), "QK-SRV-COMPOSE-DEJAVU-BINDING");
            if scenario == "directory" {
                assert_eq!(
                    fs::read(target.join("private-marker")).unwrap(),
                    b"untouched"
                );
            } else {
                assert_eq!(fs::read(target).unwrap(), expected);
            }
            assert!(!error.to_string().contains("private-invalid-marker"));
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn server_never_follows_or_replaces_linked_dejavu_state() {
        use std::os::unix::fs::symlink;

        for link_kind in ["symlink", "hardlink"] {
            let temporary = tempdir().unwrap();
            let paths = fixture_paths(temporary.path());
            let target = temporary.path().join("data/state/local-sync.json");
            let outside = temporary.path().join("outside-private-state");
            let bytes = b"private-linked-state-marker";
            fs::write(&outside, bytes).unwrap();
            if link_kind == "symlink" {
                symlink(&outside, &target).unwrap();
            } else {
                fs::hard_link(&outside, &target).unwrap();
            }

            let error = compose_fixed_server_kernel(KernelConfig::generate().unwrap(), paths)
                .await
                .unwrap_err();

            assert_eq!(error, ServerRuntimeCompositionError::DejavuBinding);
            assert_eq!(fs::read(&outside).unwrap(), bytes);
            assert_eq!(fs::read(&target).unwrap(), bytes);
            if link_kind == "symlink" {
                assert!(fs::symlink_metadata(&target)
                    .unwrap()
                    .file_type()
                    .is_symlink());
            } else {
                use std::os::unix::fs::MetadataExt as _;
                assert_eq!(fs::metadata(&target).unwrap().nlink(), 2);
            }
            assert!(!error.to_string().contains("private-linked-state-marker"));
        }
    }

    #[tokio::test]
    async fn fresh_server_initializes_one_disabled_sync_configuration_that_can_be_patched() {
        let temporary = tempdir().unwrap();
        let paths = fixture_paths(temporary.path());

        let composition = compose_fixed_server_kernel(KernelConfig::generate().unwrap(), paths)
            .await
            .unwrap();

        let sync = composition.runtime().sync_api_service().unwrap();
        let config = sync.get_sync_config().await.unwrap();
        assert!(!config.enabled);
        assert_eq!(
            config.readiness,
            crate::contract::SyncConfigReadiness::Disabled
        );
        assert!(temporary
            .path()
            .join("data/config/sync-config.json")
            .is_file());

        let patched = sync
            .patch_sync_config(PatchSyncConfigRequest {
                expected_revision: config.revision,
                changes: SyncConfigChangesDto {
                    remote_root: Some("server-notes".to_owned()),
                    ..SyncConfigChangesDto::default()
                },
            })
            .await
            .unwrap();
        assert_eq!(patched.remote_root, "server-notes");
    }

    #[tokio::test]
    async fn server_composition_preserves_an_existing_valid_sync_configuration_byte_for_byte() {
        let temporary = tempdir().unwrap();
        let paths = fixture_paths(temporary.path());
        let existing = br#"{
  "version": 3,
  "enabled": false,
  "provider": "s3",
  "remoteRoot": "existing-root",
  "mode": "automatic",
  "intervalSeconds": 30,
  "generateConflictDocument": false,
  "webdav": {"serverUrl": "", "username": "", "password": ""},
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
"#;
        let target = temporary.path().join("data/config/sync-config.json");
        fs::write(&target, existing).unwrap();

        let composition = compose_fixed_server_kernel(KernelConfig::generate().unwrap(), paths)
            .await
            .unwrap();

        let sync = composition.runtime().sync_api_service().unwrap();
        let config = sync.get_sync_config().await.unwrap();
        assert_eq!(config.remote_root, "existing-root");
        assert_eq!(fs::read(target).unwrap(), existing);
    }

    #[tokio::test]
    async fn configured_server_starts_one_app_launch_sync_before_reporting_ready() {
        let temporary = tempdir().unwrap();
        let paths = fixture_paths(temporary.path());
        fs::write(
            temporary.path().join("data/config/sync-config.json"),
            br#"{
  "version": 3,
  "enabled": true,
  "provider": "webdav",
  "remoteRoot": "server-notes",
  "mode": "startup-exit",
  "intervalSeconds": 30,
  "generateConflictDocument": false,
  "webdav": {
    "serverUrl": "http://127.0.0.1:9",
    "username": "server-user",
    "password": "server-password"
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

        let composition = compose_fixed_server_kernel(KernelConfig::generate().unwrap(), paths)
            .await
            .unwrap();
        let sync = composition.runtime().sync_api_service().unwrap();

        let status = sync.get_sync_status().await.unwrap();
        assert_eq!(status.last_trigger.as_ref(), Some(&SyncTrigger::AppLaunch));
    }

    #[tokio::test]
    async fn server_composition_owns_the_only_sync_scheduler_for_its_service() {
        let temporary = tempdir().unwrap();
        let paths = fixture_paths(temporary.path());
        let composition = compose_fixed_server_kernel(KernelConfig::generate().unwrap(), paths)
            .await
            .unwrap();

        let second = KernelSyncScheduler::start(composition.scheduler().service().clone());

        assert_eq!(
            second.unwrap_err(),
            crate::services::sync_scheduler::KernelSyncSchedulerStartError
        );
    }

    #[tokio::test]
    async fn server_composition_preserves_and_reports_a_corrupt_sync_configuration() {
        let temporary = tempdir().unwrap();
        let paths = fixture_paths(temporary.path());
        let corrupt = b"private-corrupt-sync-config-marker";
        let target = temporary.path().join("data/config/sync-config.json");
        fs::write(&target, corrupt).unwrap();

        let composition = compose_fixed_server_kernel(KernelConfig::generate().unwrap(), paths)
            .await
            .unwrap();

        let error = composition
            .runtime()
            .sync_api_service()
            .unwrap()
            .get_sync_config()
            .await
            .unwrap_err();
        assert_eq!(error.code(), crate::contract::ErrorCode::SyncConfigInvalid);
        assert_eq!(fs::read(target).unwrap(), corrupt);
    }

    #[tokio::test]
    async fn server_composition_preserves_and_reports_an_unsupported_sync_configuration() {
        let temporary = tempdir().unwrap();
        let paths = fixture_paths(temporary.path());
        let unsupported = br#"{"version":4,"private":"unsupported-sync-marker"}"#;
        let target = temporary.path().join("data/config/sync-config.json");
        fs::write(&target, unsupported).unwrap();

        let composition = compose_fixed_server_kernel(KernelConfig::generate().unwrap(), paths)
            .await
            .unwrap();

        let error = composition
            .runtime()
            .sync_api_service()
            .unwrap()
            .get_sync_config()
            .await
            .unwrap_err();
        assert_eq!(error.code(), crate::contract::ErrorCode::SyncConfigInvalid);
        assert_eq!(fs::read(target).unwrap(), unsupported);
    }

    #[tokio::test]
    async fn uninitialized_server_requires_token_but_initialized_restart_does_not() {
        let missing_token = tempdir().unwrap();
        let first = compose_fixed_server_kernel(
            KernelConfig::generate().unwrap(),
            fixture_paths(missing_token.path()),
        )
        .await
        .unwrap();
        assert_eq!(
            first
                .initialization_coordinator(launch_environment(None))
                .unwrap_err(),
            ServerInitializationCoordinatorError::MissingInitializationToken
        );
        drop(first);

        let temporary = tempdir().unwrap();
        let first = compose_fixed_server_kernel(
            KernelConfig::generate().unwrap(),
            fixture_paths(temporary.path()),
        )
        .await
        .unwrap();
        let mut initialization = first
            .initialization_coordinator(launch_environment(Some(INITIALIZATION_TOKEN)))
            .unwrap();
        initialization
            .initialize(
                7,
                Duration::ZERO,
                INITIALIZATION_TOKEN,
                ServerAuthenticationSecret::from(OWNER_PASSWORD.to_owned()),
            )
            .unwrap();
        drop(initialization);
        drop(first);

        let restarted = compose_fixed_server_kernel(
            KernelConfig::generate().unwrap(),
            fixture_paths(temporary.path()),
        )
        .await
        .unwrap();
        assert_eq!(
            restarted.authentication_status().unwrap(),
            ServerAuthenticationStatus::Ready
        );
        restarted.activate_api(launch_environment(None)).unwrap();
    }

    #[tokio::test]
    async fn workspace_identity_and_authentication_survive_restart() {
        let temporary = tempdir().unwrap();
        let first = compose_fixed_server_kernel(
            KernelConfig::generate().unwrap(),
            fixture_paths(temporary.path()),
        )
        .await
        .unwrap();
        let first_workspace = first.runtime().active_workspace_snapshot().unwrap();
        let first_id = first_workspace.workspace().id;
        drop(first_workspace);
        let mut initialization = first
            .initialization_coordinator(launch_environment(Some(INITIALIZATION_TOKEN)))
            .unwrap();
        initialization
            .initialize(
                7,
                Duration::ZERO,
                INITIALIZATION_TOKEN,
                ServerAuthenticationSecret::from(OWNER_PASSWORD.to_owned()),
            )
            .unwrap();
        drop(initialization);
        drop(first);

        let second = compose_fixed_server_kernel(
            KernelConfig::generate().unwrap(),
            fixture_paths(temporary.path()),
        )
        .await
        .unwrap();
        assert_eq!(
            second
                .runtime()
                .active_workspace_snapshot()
                .unwrap()
                .workspace()
                .id,
            first_id
        );
        assert!(second
            .authentication_coordinator()
            .login(
                7,
                Duration::from_secs(1),
                ServerAuthenticationSecret::from(OWNER_PASSWORD.to_owned()),
            )
            .is_ok());
    }

    #[tokio::test]
    async fn a_second_server_runtime_cannot_share_the_same_locked_roots() {
        let temporary = tempdir().unwrap();
        let _first = compose_fixed_server_kernel(
            KernelConfig::generate().unwrap(),
            fixture_paths(temporary.path()),
        )
        .await
        .unwrap();

        assert_eq!(
            compose_fixed_server_kernel(
                KernelConfig::generate().unwrap(),
                fixture_paths(temporary.path()),
            )
            .await
            .unwrap_err(),
            ServerRuntimeCompositionError::RuntimeActivation
        );
    }

    #[tokio::test]
    async fn drained_server_lifecycle_releases_fixed_roots_for_restart() {
        let temporary = tempdir().unwrap();
        let first = compose_fixed_server_kernel(
            KernelConfig::generate().unwrap(),
            fixture_paths(temporary.path()),
        )
        .await
        .unwrap();
        let shutdown = first.shutdown_handle();
        let concurrent_shutdown = shutdown.clone();

        let (first_drain, second_drain) =
            tokio::join!(shutdown.shutdown(), concurrent_shutdown.shutdown());
        first_drain.unwrap();
        second_drain.unwrap();
        shutdown.shutdown().await.unwrap();
        drop(concurrent_shutdown);
        drop(shutdown);
        drop(first);

        let restarted = compose_fixed_server_kernel(
            KernelConfig::generate().unwrap(),
            fixture_paths(temporary.path()),
        )
        .await
        .expect("a drained and dropped Server lifecycle must release its fixed roots");
        restarted.shutdown_handle().shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn malformed_primary_workspace_state_reports_only_its_composition_stage() {
        let temporary = tempdir().unwrap();
        let paths = fixture_paths(temporary.path());
        fs::write(
            temporary
                .path()
                .join("data/state/primary-workspace-v1.json"),
            br#"{"private":"primary-workspace-marker"}"#,
        )
        .unwrap();

        let error = compose_fixed_server_kernel(KernelConfig::generate().unwrap(), paths)
            .await
            .unwrap_err();

        assert_eq!(error, ServerRuntimeCompositionError::PrimaryWorkspaceStore);
        assert_eq!(error.diagnostic_code(), "QK-SRV-COMPOSE-PRIMARY-WORKSPACE");
        assert!(!error.to_string().contains("primary-workspace-marker"));
    }

    #[tokio::test]
    async fn malformed_settings_reports_only_the_fixed_services_stage() {
        let temporary = tempdir().unwrap();
        let paths = fixture_paths(temporary.path());
        fs::write(
            temporary.path().join("data/config/settings.json"),
            b"private-invalid-fixed-services-marker",
        )
        .unwrap();

        let error = compose_fixed_server_kernel(KernelConfig::generate().unwrap(), paths)
            .await
            .unwrap_err();

        assert_eq!(error, ServerRuntimeCompositionError::FixedSettingsStore);
        assert_eq!(
            error.diagnostic_code(),
            "QK-SRV-COMPOSE-FIXED-SETTINGS-STORE"
        );
        assert!(!error.to_string().contains("fixed-services-marker"));
    }

    #[tokio::test]
    async fn malformed_authentication_state_reports_only_the_auth_store_stage() {
        let temporary = tempdir().unwrap();
        let paths = fixture_paths(temporary.path());
        fs::write(
            temporary.path().join("data/config/owner-auth-v1.json"),
            br#"{"private":"authentication-store-marker"}"#,
        )
        .unwrap();

        let error = compose_fixed_server_kernel(KernelConfig::generate().unwrap(), paths)
            .await
            .unwrap_err();

        assert_eq!(error, ServerRuntimeCompositionError::AuthenticationStore);
        assert_eq!(error.diagnostic_code(), "QK-SRV-COMPOSE-AUTH-STORE");
        assert!(!error.to_string().contains("authentication-store-marker"));
    }

    #[tokio::test]
    async fn an_existing_authentication_owner_reports_only_the_security_stage() {
        let temporary = tempdir().unwrap();
        let paths = fixture_paths(temporary.path());
        let authentication =
            Arc::new(ServerAuthenticationStore::open(paths.config_root()).unwrap());
        let _existing_owner = production_authentication_security(authentication).unwrap();

        let error = compose_fixed_server_kernel(KernelConfig::generate().unwrap(), paths)
            .await
            .unwrap_err();

        assert_eq!(error, ServerRuntimeCompositionError::AuthenticationSecurity);
        assert_eq!(error.diagnostic_code(), "QK-SRV-COMPOSE-AUTH-SECURITY");
    }

    #[test]
    fn every_composition_stage_has_a_stable_safe_diagnostic_code() {
        for (error, expected_code) in [
            (
                ServerRuntimeCompositionError::ManagedPaths,
                "QK-SRV-COMPOSE-MANAGED-PATHS",
            ),
            (
                ServerRuntimeCompositionError::RuntimeActivation,
                "QK-SRV-COMPOSE-RUNTIME-ACTIVATE",
            ),
            (
                ServerRuntimeCompositionError::DejavuBinding,
                "QK-SRV-COMPOSE-DEJAVU-BINDING",
            ),
            (
                ServerRuntimeCompositionError::PrimaryWorkspaceStore,
                "QK-SRV-COMPOSE-PRIMARY-WORKSPACE",
            ),
            (
                ServerRuntimeCompositionError::FixedSettingsStore,
                "QK-SRV-COMPOSE-FIXED-SETTINGS-STORE",
            ),
            (
                ServerRuntimeCompositionError::FixedSyncStore,
                "QK-SRV-COMPOSE-FIXED-SYNC-STORE",
            ),
            (
                ServerRuntimeCompositionError::FixedWorkspaceService,
                "QK-SRV-COMPOSE-FIXED-WORKSPACE",
            ),
            (
                ServerRuntimeCompositionError::FixedWorkspaceInstall,
                "QK-SRV-COMPOSE-FIXED-WORKSPACE-INSTALL",
            ),
            (
                ServerRuntimeCompositionError::FixedWorkspaceSnapshot,
                "QK-SRV-COMPOSE-FIXED-WORKSPACE-SNAPSHOT",
            ),
            (
                ServerRuntimeCompositionError::FixedDocumentStorage,
                "QK-SRV-COMPOSE-FIXED-DOCUMENT-STORAGE",
            ),
            (
                ServerRuntimeCompositionError::FixedDocumentService,
                "QK-SRV-COMPOSE-FIXED-DOCUMENT-SERVICE",
            ),
            (
                ServerRuntimeCompositionError::FixedServiceInstall,
                "QK-SRV-COMPOSE-FIXED-SERVICE-INSTALL",
            ),
            (
                ServerRuntimeCompositionError::AuthenticationStore,
                "QK-SRV-COMPOSE-AUTH-STORE",
            ),
            (
                ServerRuntimeCompositionError::AuthenticationSecurity,
                "QK-SRV-COMPOSE-AUTH-SECURITY",
            ),
            (
                ServerRuntimeCompositionError::SyncScheduler,
                "QK-SRV-COMPOSE-SYNC-SCHEDULER",
            ),
            (
                ServerRuntimeCompositionError::ApiActivation,
                "QK-SRV-AUTH-API",
            ),
        ] {
            assert_eq!(error.diagnostic_code(), expected_code);
            assert_eq!(error.to_string(), "fixed Server runtime composition failed");
        }
    }

    #[tokio::test]
    async fn real_tcp_server_serves_health_initialization_and_login_behind_tls_origin() {
        let temporary = tempdir().unwrap();
        let composition = compose_fixed_server_kernel(
            KernelConfig::generate().unwrap(),
            fixture_paths(temporary.path()),
        )
        .await
        .unwrap();
        let activation = composition
            .activate_api(launch_environment(Some(INITIALIZATION_TOKEN)))
            .unwrap();
        let policy =
            TransportPolicy::same_origin("notes.example.com", "https://notes.example.com").unwrap();
        let router = build_server_router(activation, policy);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .await
        });
        let client = reqwest::Client::new();
        let endpoint = |path: &str| format!("http://{address}{path}");

        let live = client
            .get(endpoint("/api/v1/health/live"))
            .header(reqwest::header::HOST, "notes.example.com")
            .send()
            .await
            .unwrap();
        assert_eq!(live.status(), reqwest::StatusCode::OK);

        let initialized = client
            .post(endpoint("/api/v1/auth/initialize"))
            .header(reqwest::header::HOST, "notes.example.com")
            .header(reqwest::header::ORIGIN, "https://notes.example.com")
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(
                serde_json::json!({
                    "initializationToken": INITIALIZATION_TOKEN,
                    "password": OWNER_PASSWORD,
                })
                .to_string(),
            )
            .send()
            .await
            .unwrap();
        assert_eq!(initialized.status(), reqwest::StatusCode::CREATED);
        assert_eq!(
            initialized
                .headers()
                .get_all(reqwest::header::SET_COOKIE)
                .iter()
                .count(),
            2
        );

        let logged_in = client
            .post(endpoint("/api/v1/auth/session"))
            .header(reqwest::header::HOST, "notes.example.com")
            .header(reqwest::header::ORIGIN, "https://notes.example.com")
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(serde_json::json!({"password": OWNER_PASSWORD}).to_string())
            .send()
            .await
            .unwrap();
        assert_eq!(logged_in.status(), reqwest::StatusCode::CREATED);
        assert_eq!(
            logged_in
                .headers()
                .get_all(reqwest::header::SET_COOKIE)
                .iter()
                .count(),
            2
        );
        let session_cookie = logged_in
            .headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .find(|value| value.starts_with("__Host-qingyu_session="))
            .and_then(|value| value.split(';').next())
            .unwrap();
        let ready = client
            .get(endpoint("/api/v1/health/ready"))
            .header(reqwest::header::HOST, "notes.example.com")
            .header(reqwest::header::ORIGIN, "https://notes.example.com")
            .header(reqwest::header::COOKIE, session_cookie)
            .send()
            .await
            .unwrap();
        assert_eq!(ready.status(), reqwest::StatusCode::OK);

        let sync_config = client
            .get(endpoint("/api/v1/sync/config"))
            .header(reqwest::header::HOST, "notes.example.com")
            .header(reqwest::header::ORIGIN, "https://notes.example.com")
            .header(reqwest::header::COOKIE, session_cookie)
            .send()
            .await
            .unwrap();
        assert_eq!(sync_config.status(), reqwest::StatusCode::OK);
        let sync_config: serde_json::Value =
            serde_json::from_slice(&sync_config.bytes().await.unwrap()).unwrap();
        let revision = sync_config["revision"]
            .as_str()
            .expect("fixed server sync configuration revision");
        let catalog = client
            .get(endpoint(&format!(
                "/api/v1/sync/repositories?expectedRevision={revision}"
            )))
            .header(reqwest::header::HOST, "notes.example.com")
            .header(reqwest::header::ORIGIN, "https://notes.example.com")
            .header(reqwest::header::COOKIE, session_cookie)
            .send()
            .await
            .unwrap();
        assert_eq!(catalog.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
        let catalog: serde_json::Value =
            serde_json::from_slice(&catalog.bytes().await.unwrap()).unwrap();
        assert_eq!(catalog["code"], "sync_not_ready");

        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }
}
