use std::{
    fmt,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, RwLock,
    },
    time::Duration,
};

use rmcp::{RoleServer, ServiceExt};
use serde::Serialize;
use tokio::{task::JoinHandle, task::JoinSet};
use tokio_util::sync::CancellationToken;

use super::{
    config::McpConfig,
    ipc::{bounded_transport, LocalIpcEndpoint},
    tools::QingYuMcpHandler,
};

pub(crate) const MAX_ACTIVE_SESSIONS: usize = 8;
#[cfg(not(test))]
const MCP_SHUTDOWN_DRAIN_TIMEOUT: Duration = Duration::from_secs(30);
#[cfg(test)]
const MCP_SHUTDOWN_DRAIN_TIMEOUT: Duration = Duration::from_millis(100);

#[derive(Clone, Debug)]
pub(crate) struct McpServerOptions {
    pub(crate) enabled: bool,
    pub(crate) request_limit_bytes: u64,
}

impl McpServerOptions {
    pub(crate) fn from_config(config: &McpConfig) -> Self {
        Self {
            enabled: config.enabled,
            request_limit_bytes: config.request_limit_bytes,
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        Self {
            enabled: true,
            request_limit_bytes: 8 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum McpServerState {
    Disabled,
    Stopped,
    Starting,
    Running,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct McpServerHealth {
    pub(crate) state: McpServerState,
    pub(crate) endpoint: Option<String>,
    pub(crate) error_code: Option<String>,
}

impl Default for McpServerHealth {
    fn default() -> Self {
        Self {
            state: McpServerState::Stopped,
            endpoint: None,
            error_code: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct McpServerError {
    pub(crate) code: &'static str,
    message: &'static str,
}

impl McpServerError {
    fn bind() -> Self {
        Self {
            code: "mcp_bind_failed",
            message: "QingYu MCP could not bind its private local IPC endpoint.",
        }
    }

    fn state() -> Self {
        Self {
            code: "mcp_server_state_unavailable",
            message: "QingYu MCP server state is unavailable.",
        }
    }

    fn shutdown() -> Self {
        Self {
            code: "mcp_server_shutdown",
            message: "QingYu MCP has entered terminal application shutdown.",
        }
    }
}

impl fmt::Display for McpServerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for McpServerError {}

struct RunningServer {
    graceful_shutdown: CancellationToken,
    immediate_stop: CancellationToken,
    join: JoinHandle<()>,
}

#[derive(Default)]
struct ServerLifecycle {
    running: Option<RunningServer>,
    shutdown: bool,
}

struct ActiveSessionGuard(Arc<AtomicUsize>);

impl Drop for ActiveSessionGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

pub(crate) struct McpServerController {
    active_sessions: Arc<AtomicUsize>,
    endpoint: LocalIpcEndpoint,
    handler: QingYuMcpHandler,
    health: Arc<RwLock<McpServerHealth>>,
    lifecycle: tokio::sync::Mutex<ServerLifecycle>,
}

impl McpServerController {
    pub(crate) fn new(handler: QingYuMcpHandler, endpoint: LocalIpcEndpoint) -> Self {
        Self {
            active_sessions: Arc::new(AtomicUsize::new(0)),
            endpoint,
            handler,
            health: Arc::new(RwLock::new(McpServerHealth::default())),
            lifecycle: tokio::sync::Mutex::new(ServerLifecycle::default()),
        }
    }

    pub(crate) async fn start(
        &self,
        options: McpServerOptions,
    ) -> Result<McpServerHealth, McpServerError> {
        let mut lifecycle = self.lifecycle.lock().await;
        if lifecycle.shutdown {
            return Err(McpServerError::shutdown());
        }
        Self::stop_running(&mut lifecycle, false).await;
        self.handler.invalidate_previews();
        if !options.enabled {
            return self.set_health(McpServerHealth {
                state: McpServerState::Disabled,
                ..McpServerHealth::default()
            });
        }
        self.set_health(McpServerHealth {
            state: McpServerState::Starting,
            ..McpServerHealth::default()
        })?;

        let mut listener = match self.endpoint.bind().await {
            Ok(listener) => listener,
            Err(_) => {
                let error = McpServerError::bind();
                let _health_result = self.set_health(McpServerHealth {
                    state: McpServerState::Error,
                    error_code: Some(error.code.to_string()),
                    ..McpServerHealth::default()
                });
                return Err(error);
            }
        };
        let graceful_shutdown = CancellationToken::new();
        let task_graceful_shutdown = graceful_shutdown.clone();
        let immediate_stop = CancellationToken::new();
        let task_immediate_stop = immediate_stop.clone();
        let handler = self.handler.clone();
        let active_sessions = Arc::clone(&self.active_sessions);
        let health_store = Arc::clone(&self.health);
        let request_limit = options.request_limit_bytes.try_into().unwrap_or(usize::MAX);
        let session_permits = Arc::new(tokio::sync::Semaphore::new(MAX_ACTIVE_SESSIONS));
        let join = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            let graceful = loop {
                tokio::select! {
                    _ = task_immediate_stop.cancelled() => break false,
                    _ = task_graceful_shutdown.cancelled() => break true,
                    accepted = listener.accept() => match accepted {
                        Ok(stream) => {
                            let Ok(permit) = Arc::clone(&session_permits).try_acquire_owned() else {
                                drop(stream);
                                continue;
                            };
                            active_sessions.fetch_add(1, Ordering::Relaxed);
                            let guard = ActiveSessionGuard(Arc::clone(&active_sessions));
                            let connection_handler = handler.clone();
                            connections.spawn(async move {
                                let _permit = permit;
                                let _guard = guard;
                                let transport =
                                    bounded_transport::<RoleServer, _>(stream, request_limit);
                                match connection_handler.serve(transport).await {
                                    Ok(service) => {
                                        let _service_result = service.waiting().await;
                                    }
                                    Err(_error) => {}
                                }
                            });
                        }
                        Err(_) => {
                            if let Ok(mut health) = health_store.write() {
                                health.state = McpServerState::Error;
                                health.error_code = Some("mcp_server_failed".to_string());
                            }
                            break false;
                        }
                    },
                    completed = connections.join_next(), if !connections.is_empty() => {
                        let _connection_result = completed;
                    }
                }
            };
            if graceful {
                let _drain_result =
                    tokio::time::timeout(MCP_SHUTDOWN_DRAIN_TIMEOUT, handler.wait_for_drain())
                        .await;
            }
            connections.abort_all();
            while connections.join_next().await.is_some() {}
        });
        lifecycle.running = Some(RunningServer {
            graceful_shutdown,
            immediate_stop,
            join,
        });
        self.set_health(McpServerHealth {
            state: McpServerState::Running,
            endpoint: Some(self.endpoint.health_label().to_string()),
            error_code: None,
        })
    }

    pub(crate) async fn stop(&self) -> Result<(), McpServerError> {
        let mut lifecycle = self.lifecycle.lock().await;
        Self::stop_running(&mut lifecycle, false).await;
        self.handler.invalidate_previews();
        self.set_health(McpServerHealth {
            state: McpServerState::Stopped,
            ..McpServerHealth::default()
        })?;
        Ok(())
    }

    pub(crate) async fn shutdown(&self) -> Result<(), McpServerError> {
        let mut lifecycle = self.lifecycle.lock().await;
        if !lifecycle.shutdown {
            lifecycle.shutdown = true;
            self.handler.begin_shutdown();
        }
        if lifecycle.running.is_some() {
            Self::stop_running(&mut lifecycle, true).await;
        } else {
            let _drain_result =
                tokio::time::timeout(MCP_SHUTDOWN_DRAIN_TIMEOUT, self.handler.wait_for_drain())
                    .await;
        }
        self.handler.invalidate_previews();
        self.set_health(McpServerHealth {
            state: McpServerState::Stopped,
            ..McpServerHealth::default()
        })?;
        Ok(())
    }

    pub(crate) async fn restart(
        &self,
        options: McpServerOptions,
    ) -> Result<McpServerHealth, McpServerError> {
        self.start(options).await
    }

    pub(crate) async fn notify_tools_changed(&self) {
        self.handler.notify_tools_changed().await;
    }

    pub(crate) fn health(&self) -> McpServerHealth {
        self.health
            .read()
            .map(|health| health.clone())
            .unwrap_or(McpServerHealth {
                state: McpServerState::Error,
                error_code: Some("mcp_server_state_unavailable".to_string()),
                ..McpServerHealth::default()
            })
    }

    #[cfg(test)]
    pub(crate) fn active_session_count(&self) -> usize {
        self.active_sessions.load(Ordering::Relaxed)
    }

    fn set_health(&self, health: McpServerHealth) -> Result<McpServerHealth, McpServerError> {
        *self.health.write().map_err(|_| McpServerError::state())? = health.clone();
        Ok(health)
    }

    async fn stop_running(lifecycle: &mut ServerLifecycle, graceful: bool) {
        let Some(running) = lifecycle.running.take() else {
            return;
        };
        if graceful {
            running.graceful_shutdown.cancel();
        } else {
            running.immediate_stop.cancel();
        }
        let _join_result = running.join.await;
    }
}
