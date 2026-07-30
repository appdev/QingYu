//! Fixed production composition for the single-user Server host.

use std::{fmt, sync::Arc, time::Duration};

use crate::{
    api::{ServerApiActivation, ServerApiProcess},
    composition::install_fixed_kernel_services,
    config::KernelConfig,
    paths::KernelPaths,
    ports::system::system_kernel_ports,
    runtime::KernelRuntime,
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

/// Fully assembled fixed Server runtime and its unique authentication owner.
pub struct ServerRuntimeComposition {
    runtime: Arc<KernelRuntime>,
    _authentication: Arc<ServerAuthenticationStore>,
    security: ServerAuthenticationSecurity,
}

impl ServerRuntimeComposition {
    /// Consumes the composition into the one process-wide Server API owner.
    /// A failed activation is fail-closed until the process is restarted.
    pub fn activate_api(
        self,
        environment: ServerLaunchEnvironment,
    ) -> Result<ServerApiActivation, ServerRuntimeCompositionError> {
        let process = ServerApiProcess::new(self.runtime, SERVER_BLOCKING_CAPACITY)
            .map_err(|_| ServerRuntimeCompositionError)?;
        process
            .activate(self.security, environment)
            .map_err(|_| ServerRuntimeCompositionError)
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
            .map_err(|_| ServerRuntimeCompositionError)
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
        .map_err(|_| ServerRuntimeCompositionError)?;
    let runtime = KernelRuntime::activate(config, paths, system_kernel_ports())
        .map_err(|_| ServerRuntimeCompositionError)?;
    let primary_workspace = Arc::new(
        DurableServerPrimaryWorkspaceStore::open(
            runtime.instance_data_root(),
            runtime.launch_epoch(),
        )
        .map_err(|_| ServerRuntimeCompositionError)?,
    );
    install_fixed_kernel_services(
        &runtime,
        primary_workspace,
        managed,
        SERVER_WORKSPACE_DISPLAY_NAME,
    )
    .await
    .map_err(|_| ServerRuntimeCompositionError)?;
    let authentication = Arc::new(
        ServerAuthenticationStore::open(runtime.config_root())
            .map_err(|_| ServerRuntimeCompositionError)?,
    );
    let security = production_authentication_security(Arc::clone(&authentication))?;
    Ok(ServerRuntimeComposition {
        runtime,
        _authentication: authentication,
        security,
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
    .map_err(|_| ServerRuntimeCompositionError)?;
    let initialization = RateLimitPolicy::new(
        SERVER_INITIALIZATION_MAXIMUM_FAILURES,
        SERVER_AUTH_OBSERVATION_WINDOW,
        SERVER_AUTH_LOCKOUT,
    )
    .map_err(|_| ServerRuntimeCompositionError)?;
    let rate_limiter = AuthenticationRateLimiter::with_capacity(
        login,
        initialization,
        SERVER_MAXIMUM_AUTH_CLIENTS_PER_FLOW,
        SERVER_MAXIMUM_AUTH_ATTEMPTS_IN_FLIGHT,
    )
    .map_err(|_| ServerRuntimeCompositionError)?;
    let sessions = SessionStore::new(
        SessionPolicy::with_capacity(SERVER_SESSION_LIFETIME, SERVER_MAXIMUM_ACTIVE_SESSIONS)
            .map_err(|_| ServerRuntimeCompositionError)?,
    );
    ServerAuthenticationSecurity::claim(authentication, rate_limiter, sessions)
        .map_err(|_| ServerRuntimeCompositionError)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServerRuntimeCompositionError;

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
        contract::{HostProfile, ServerAuthenticationSecret},
        paths::KernelPaths,
        server::{
            ServerAuthenticationStatus, ServerInitializationCoordinatorError,
            ServerLaunchEnvironment,
        },
    };

    use super::*;

    const INITIALIZATION_TOKEN: &str = "server-runtime-initialization-token-at-least-32-bytes";
    const OWNER_PASSWORD: &str = "correct horse battery staple";

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

        assert!(compose_fixed_server_kernel(
            KernelConfig::generate().unwrap(),
            fixture_paths(temporary.path()),
        )
        .await
        .is_err());
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

        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }
}
