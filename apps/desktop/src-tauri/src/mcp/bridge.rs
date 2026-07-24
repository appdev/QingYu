use std::{
    fmt, fs, io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use rmcp::{
    model::{
        CallToolRequestParams, CallToolResult, Implementation, ListToolsResult,
        PaginatedRequestParams, ServerCapabilities, ServerInfo,
    },
    service::{NotificationContext, Peer, RequestContext, RoleClient, RoleServer, RunningService},
    transport::IntoTransport,
    ClientHandler, ErrorData, ServerHandler, ServiceExt,
};

use super::ipc::{application_data_dir, bounded_transport, LocalIpcEndpoint};

const MAXIMUM_BRIDGE_FRAME_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug)]
pub(crate) struct BridgeConfig {
    pub(crate) endpoint: LocalIpcEndpoint,
    settings_path: PathBuf,
    pub(crate) startup_timeout: Duration,
    pub(crate) initial_backoff: Duration,
    pub(crate) maximum_backoff: Duration,
}

impl BridgeConfig {
    pub(crate) fn for_app() -> Result<Self, BridgeError> {
        let endpoint = LocalIpcEndpoint::for_app().map_err(|_| BridgeError::InvalidEndpoint)?;
        let settings_path = application_data_dir()
            .map_err(|_| BridgeError::InvalidEndpoint)?
            .join("settings.json");
        Ok(Self {
            endpoint,
            settings_path,
            startup_timeout: Duration::from_secs(15),
            initial_backoff: Duration::from_millis(100),
            maximum_backoff: Duration::from_secs(1),
        })
    }

    #[cfg(test)]
    pub(crate) fn for_test(endpoint: LocalIpcEndpoint) -> Self {
        Self {
            endpoint,
            settings_path: std::env::temp_dir().join(format!(
                "qingyu-mcp-test-settings-missing-{}",
                std::process::id()
            )),
            startup_timeout: Duration::from_millis(50),
            initial_backoff: Duration::from_millis(5),
            maximum_backoff: Duration::from_millis(10),
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test_with_settings(
        endpoint: LocalIpcEndpoint,
        settings_path: PathBuf,
    ) -> Self {
        Self {
            settings_path,
            ..Self::for_test(endpoint)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BridgeError {
    InvalidEndpoint,
    McpDisabled,
    McpConfigUnavailable,
    AppLaunchFailed,
    UpstreamUnavailable,
    DownstreamUnavailable,
}

impl fmt::Display for BridgeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidEndpoint => "invalid_endpoint: QingYu MCP local IPC endpoint is invalid.",
            Self::McpDisabled => {
                "mcp_disabled: QingYu MCP is disabled. Enable MCP in QingYu Settings, then retry."
            }
            Self::McpConfigUnavailable => {
                "mcp_config_unavailable: QingYu MCP could not read its application settings."
            }
            Self::AppLaunchFailed => "app_launch_failed: QingYu could not be launched.",
            Self::UpstreamUnavailable => "upstream_unavailable: QingYu MCP is unavailable.",
            Self::DownstreamUnavailable => {
                "downstream_unavailable: The MCP stdio connection closed unexpectedly."
            }
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for BridgeError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum McpStartupPermission {
    Enabled,
    Disabled,
}

fn read_mcp_startup_permission(path: &Path) -> Result<McpStartupPermission, BridgeError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(McpStartupPermission::Disabled);
        }
        Err(_) => return Err(BridgeError::McpConfigUnavailable),
    };
    let settings: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| BridgeError::McpConfigUnavailable)?;
    let root = settings
        .as_object()
        .ok_or(BridgeError::McpConfigUnavailable)?;
    let Some(mcp) = root.get("mcp") else {
        return Ok(McpStartupPermission::Disabled);
    };
    let enabled = mcp
        .as_object()
        .and_then(|object| object.get("enabled"))
        .and_then(serde_json::Value::as_bool)
        .ok_or(BridgeError::McpConfigUnavailable)?;
    Ok(if enabled {
        McpStartupPermission::Enabled
    } else {
        McpStartupPermission::Disabled
    })
}

pub(crate) trait AppLauncher: Send + Sync {
    fn launch(&self) -> io::Result<()>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BridgePlatform {
    #[cfg(any(target_os = "macos", test))]
    MacOs,
    #[cfg(any(target_os = "windows", test))]
    Windows,
    #[cfg(any(target_os = "linux", test))]
    Linux,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AppLaunchRequest {
    executable: PathBuf,
    arguments: [&'static str; 2],
}

fn app_launch_request(
    bridge_executable: &Path,
    platform: BridgePlatform,
) -> Result<AppLaunchRequest, BridgeError> {
    let directory = bridge_executable
        .parent()
        .ok_or(BridgeError::AppLaunchFailed)?;
    let executable_name = match platform {
        #[cfg(any(target_os = "macos", test))]
        BridgePlatform::MacOs => "markra",
        #[cfg(any(target_os = "linux", test))]
        BridgePlatform::Linux => "markra",
        #[cfg(any(target_os = "windows", test))]
        BridgePlatform::Windows => "markra.exe",
    };
    Ok(AppLaunchRequest {
        executable: directory.join(executable_name),
        arguments: ["mcp", "serve"],
    })
}

fn platform_app_launch_request() -> Result<AppLaunchRequest, BridgeError> {
    let bridge_executable = std::env::current_exe().map_err(|_| BridgeError::AppLaunchFailed)?;
    #[cfg(target_os = "macos")]
    let platform = BridgePlatform::MacOs;
    #[cfg(target_os = "windows")]
    let platform = BridgePlatform::Windows;
    #[cfg(target_os = "linux")]
    let platform = BridgePlatform::Linux;
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    return Err(BridgeError::AppLaunchFailed);
    app_launch_request(&bridge_executable, platform)
}

pub(crate) struct PlatformAppLauncher {
    request: AppLaunchRequest,
}

impl PlatformAppLauncher {
    fn for_app() -> Result<Self, BridgeError> {
        Ok(Self {
            request: platform_app_launch_request()?,
        })
    }
}

#[cfg(test)]
pub(crate) fn test_app_launch_request(
    bridge_executable: &Path,
    platform: &str,
) -> (PathBuf, Vec<&'static str>) {
    let platform = match platform {
        "macos" => BridgePlatform::MacOs,
        "windows" => BridgePlatform::Windows,
        "linux" => BridgePlatform::Linux,
        _ => panic!("unsupported test platform"),
    };
    let request = app_launch_request(bridge_executable, platform).expect("test launch request");
    (request.executable, request.arguments.into())
}

impl AppLauncher for PlatformAppLauncher {
    fn launch(&self) -> io::Result<()> {
        let metadata = fs::symlink_metadata(&self.request.executable)?;
        if !metadata.file_type().is_file() {
            return Err(io::Error::other(
                "bundled QingYu executable is not a regular file",
            ));
        }
        Command::new(&self.request.executable)
            .args(self.request.arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        Ok(())
    }
}

#[derive(Clone, Default)]
struct NotificationRelay {
    downstream: Arc<Mutex<Option<Peer<RoleServer>>>>,
}

impl ClientHandler for NotificationRelay {
    async fn on_tool_list_changed(&self, _context: NotificationContext<RoleClient>) {
        let peer = self
            .downstream
            .lock()
            .ok()
            .and_then(|downstream| downstream.clone());
        if let Some(peer) = peer {
            let _notification_result = peer.notify_tool_list_changed().await;
        }
    }
}

#[derive(Clone)]
struct ForwardingHandler {
    downstream: Arc<Mutex<Option<Peer<RoleServer>>>>,
    upstream: Peer<RoleClient>,
}

impl ForwardingHandler {
    fn new(upstream: Peer<RoleClient>, relay: &NotificationRelay) -> Self {
        Self {
            downstream: relay.downstream.clone(),
            upstream,
        }
    }
}

impl ServerHandler for ForwardingHandler {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_server_info(
            Implementation::new("qingyu-mcp", env!("CARGO_PKG_VERSION"))
                .with_title("QingYu MCP stdio bridge"),
        )
    }

    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        self.upstream
            .list_tools(request)
            .await
            .map_err(|_| upstream_outcome_indeterminate())
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        self.upstream
            .call_tool(request)
            .await
            .map_err(|_| upstream_outcome_indeterminate())
    }

    async fn on_initialized(&self, context: NotificationContext<RoleServer>) {
        if let Ok(mut downstream) = self.downstream.lock() {
            *downstream = Some(context.peer);
        }
    }
}

fn upstream_outcome_indeterminate() -> ErrorData {
    ErrorData::internal_error(
        "The QingYu MCP request outcome is indeterminate; the bridge did not replay it.",
        Some(serde_json::json!({
            "code": "upstream_outcome_indeterminate",
            "retryable": false
        })),
    )
}

async fn connect_upstream(
    config: &BridgeConfig,
    relay: NotificationRelay,
) -> Result<RunningService<RoleClient, NotificationRelay>, BridgeError> {
    let stream = config
        .endpoint
        .connect()
        .await
        .map_err(|_| BridgeError::UpstreamUnavailable)?;
    relay
        .serve(bounded_transport::<RoleClient, _>(
            stream,
            MAXIMUM_BRIDGE_FRAME_BYTES,
        ))
        .await
        .map_err(|_| BridgeError::UpstreamUnavailable)
}

async fn connect_with_launch<L: AppLauncher>(
    config: &BridgeConfig,
    launcher: &L,
    relay: NotificationRelay,
) -> Result<RunningService<RoleClient, NotificationRelay>, BridgeError> {
    let started = Instant::now();
    let mut backoff = config.initial_backoff;
    let mut launched = false;
    loop {
        match connect_upstream(config, relay.clone()).await {
            Ok(service) => return Ok(service),
            Err(_) if started.elapsed() >= config.startup_timeout => {
                return Err(BridgeError::UpstreamUnavailable);
            }
            Err(_) => {
                if !launched {
                    if read_mcp_startup_permission(&config.settings_path)?
                        == McpStartupPermission::Disabled
                    {
                        return Err(BridgeError::McpDisabled);
                    }
                    launcher
                        .launch()
                        .map_err(|_| BridgeError::AppLaunchFailed)?;
                    launched = true;
                }
                let remaining = config.startup_timeout.saturating_sub(started.elapsed());
                tokio::time::sleep(backoff.min(remaining)).await;
                backoff = backoff.saturating_mul(2).min(config.maximum_backoff);
            }
        }
    }
}

pub(crate) async fn run_bridge<T, E, A, L>(
    config: BridgeConfig,
    launcher: L,
    downstream_transport: T,
) -> Result<(), BridgeError>
where
    T: IntoTransport<RoleServer, E, A>,
    E: std::error::Error + Send + Sync + 'static,
    L: AppLauncher,
{
    let relay = NotificationRelay::default();
    let upstream = connect_with_launch(&config, &launcher, relay.clone()).await?;
    let handler = ForwardingHandler::new(upstream.peer().clone(), &relay);
    let downstream = handler
        .serve(downstream_transport)
        .await
        .map_err(|_| BridgeError::DownstreamUnavailable)?;
    downstream
        .waiting()
        .await
        .map_err(|_| BridgeError::DownstreamUnavailable)?;
    upstream
        .cancel()
        .await
        .map_err(|_| BridgeError::UpstreamUnavailable)?;
    Ok(())
}

pub(crate) async fn run_bridge_for_app() -> Result<(), BridgeError> {
    run_bridge(
        BridgeConfig::for_app()?,
        PlatformAppLauncher::for_app()?,
        rmcp::transport::stdio(),
    )
    .await
}

#[cfg(test)]
pub(crate) async fn test_connect_with_launch<L: AppLauncher>(
    config: &BridgeConfig,
    launcher: &L,
) -> Result<(), BridgeError> {
    connect_with_launch(config, launcher, NotificationRelay::default())
        .await
        .map(drop)
}

#[cfg(test)]
pub(crate) fn test_indeterminate_error() -> ErrorData {
    upstream_outcome_indeterminate()
}
