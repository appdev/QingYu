#[allow(dead_code)]
#[cfg(any(desktop, feature = "desktop-sidecar"))]
pub(crate) mod audit;
#[cfg(any(desktop, feature = "desktop-sidecar"))]
pub(crate) mod bridge;
#[allow(dead_code)]
pub(crate) mod config;
#[allow(dead_code)]
#[cfg(any(desktop, feature = "desktop-sidecar"))]
pub(crate) mod confirmation;
#[cfg(any(desktop, feature = "desktop-sidecar"))]
pub(crate) mod error;
#[allow(dead_code)]
#[cfg(any(desktop, feature = "desktop-sidecar"))]
pub(crate) mod handles;
#[cfg(any(desktop, feature = "desktop-sidecar"))]
pub(crate) mod ipc;
pub(crate) mod local_settings;
#[allow(dead_code)]
#[cfg(any(desktop, feature = "desktop-sidecar"))]
pub(crate) mod server;
#[allow(dead_code)]
#[cfg(any(desktop, feature = "desktop-sidecar"))]
pub(crate) mod tools;
#[allow(dead_code)]
#[cfg(any(desktop, feature = "desktop-sidecar"))]
pub(crate) mod workspaces;

#[allow(dead_code)]
#[cfg(any(desktop, feature = "desktop-sidecar"))]
pub(crate) mod policy;
#[cfg(any(desktop, feature = "desktop-sidecar"))]
mod recycle;
#[cfg(all(test, any(desktop, feature = "desktop-sidecar")))]
mod tests;

#[cfg(any(desktop, feature = "desktop-sidecar"))]
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(any(desktop, feature = "desktop-sidecar"))]
use tauri::{Emitter, Manager};

#[cfg(any(desktop, feature = "desktop-sidecar"))]
use crate::{
    app_settings::AppSettingsService, markdown_files::DocumentService,
    remote_sync::mcp_service::SyncService,
};

#[cfg(any(desktop, feature = "desktop-sidecar"))]
use self::{
    audit::{AuditEntry, AuditSink},
    config::{McpConfig, McpConfigManager},
    confirmation::TauriConfirmationPresenter,
    handles::HandleSigner,
    ipc::LocalIpcEndpoint,
    local_settings::McpLocalSettingsService,
    policy::PolicyEngine,
    recycle::clean_expired_entries,
    server::{McpServerController, McpServerOptions, McpServerState},
    tools::{McpServices, QingYuMcpHandler},
    workspaces::{SafeWorkspace, WorkspaceRegistry},
};

#[cfg(any(desktop, feature = "desktop-sidecar"))]
pub(crate) const MCP_RUNTIME_CHANGED_EVENT: &str = "qingyu://mcp-runtime-changed";

#[cfg(any(desktop, feature = "desktop-sidecar"))]
const RECYCLE_CLEANUP_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

#[cfg(any(desktop, feature = "desktop-sidecar"))]
fn sidecar_command_for_executable(executable: &Path, executable_suffix: &str) -> Option<PathBuf> {
    Some(
        executable
            .parent()?
            .join(format!("qingyu-mcp{executable_suffix}")),
    )
}

#[cfg(any(desktop, feature = "desktop-sidecar"))]
fn current_sidecar_command() -> Option<String> {
    let executable = std::env::current_exe().ok()?;
    sidecar_command_for_executable(&executable, std::env::consts::EXE_SUFFIX)
        .map(|command| command.to_string_lossy().into_owned())
}

#[cfg(any(desktop, feature = "desktop-sidecar"))]
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct McpRuntimeChangedEvent {
    pub(crate) workspace_generation: u64,
}

#[cfg(any(desktop, feature = "desktop-sidecar"))]
fn mcp_runtime_change_event(
    previous_generation: u64,
    current_generation: u64,
) -> Option<McpRuntimeChangedEvent> {
    (previous_generation != current_generation).then_some(McpRuntimeChangedEvent {
        workspace_generation: current_generation,
    })
}

#[allow(dead_code)]
#[cfg(any(desktop, feature = "desktop-sidecar"))]
pub(crate) struct McpState {
    activation: tokio::sync::Mutex<()>,
    pub(crate) audit: Arc<AuditSink>,
    client_command: Option<String>,
    pub(crate) config: Arc<McpConfigManager>,
    pub(crate) controller: Arc<McpServerController>,
    pub(crate) policy: Arc<PolicyEngine>,
    recycle_root: PathBuf,
    pub(crate) workspaces: Arc<WorkspaceRegistry>,
}

#[cfg(any(desktop, feature = "desktop-sidecar"))]
pub(crate) fn initialize(app: &tauri::AppHandle) -> Result<Arc<McpState>, String> {
    let app_config_dir = app
        .path()
        .app_config_dir()
        .map_err(|error| error.to_string())?;
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let runtime_root = app_data_dir.join("mcp-runtime");
    std::fs::create_dir_all(&runtime_root).map_err(|error| error.to_string())?;
    let settings = Arc::new(AppSettingsService::from_app(app).map_err(|error| error.to_string())?);
    let mcp_settings = McpLocalSettingsService::from_app(app).map_err(|error| error.to_string())?;
    let config = Arc::new(McpConfigManager::load(mcp_settings).map_err(|error| error.to_string())?);
    let snapshot = config.snapshot().map_err(|error| error.to_string())?;
    let workspaces = Arc::new(WorkspaceRegistry::for_application_data(
        &app_config_dir,
        &app_data_dir,
    ));
    let workspace_activation =
        crate::primary_workspace::with_primary_workspace_transaction(app, |authoritative| {
            apply_authoritative_primary_workspace(&workspaces, authoritative)
        });
    if workspace_activation.is_err() {
        eprintln!("QingYu MCP primary workspace is unavailable.");
    }
    let signing_key = new_process_key()?;
    let signer = HandleSigner::new(signing_key);
    let recycle_root = runtime_root.join("recycle");
    let documents = Arc::new(
        DocumentService::new(signer.clone())
            .with_mutation_storage(runtime_root.join("history"), recycle_root.clone()),
    );
    let sync = Arc::new(SyncService::new(app.clone()));
    let policy = Arc::new(PolicyEngine::new(
        signer.derive_key(b"QingYu MCP operation previews v1"),
    ));
    let audit = Arc::new(AuditSink::new(&runtime_root, snapshot.config.audit.clone()));
    let services = McpServices {
        config: config.clone(),
        workspaces: workspaces.clone(),
        documents,
        settings,
        sync,
        policy: policy.clone(),
        audit: audit.clone(),
    };
    let handler = QingYuMcpHandler::new(
        services,
        Arc::new(TauriConfirmationPresenter::new(app.clone())),
    );
    let endpoint = LocalIpcEndpoint::for_app().map_err(|error| error.to_string())?;
    let controller = Arc::new(McpServerController::new(handler, endpoint));
    let state = Arc::new(McpState {
        activation: tokio::sync::Mutex::new(()),
        audit,
        client_command: current_sidecar_command(),
        config,
        controller,
        policy,
        recycle_root,
        workspaces,
    });
    spawn_recycle_cleanup(Arc::clone(&state));
    if snapshot.config.enabled {
        let startup = Arc::clone(&state);
        tauri::async_runtime::spawn(async move {
            let _startup_result = apply_server_config(&startup).await;
        });
    }
    Ok(state)
}

#[cfg(any(desktop, feature = "desktop-sidecar"))]
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct UpdateMcpSettingsInput {
    pub(crate) expected_revision: String,
    pub(crate) config: McpConfig,
}

#[cfg(any(desktop, feature = "desktop-sidecar"))]
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct McpSettingsSnapshot {
    pub(crate) revision: String,
    pub(crate) config: McpConfig,
    pub(crate) client_command: Option<String>,
    pub(crate) endpoint: String,
    pub(crate) health: server::McpServerHealth,
    pub(crate) workspace: Option<SafeWorkspace>,
}

#[cfg(any(desktop, feature = "desktop-sidecar"))]
fn settings_snapshot(state: &McpState) -> Result<McpSettingsSnapshot, String> {
    let document = state.config.snapshot().map_err(|error| error.to_string())?;
    let workspace = state
        .workspaces
        .with_authority(|| state.workspaces.list_safe().into_iter().next())
        .map_err(|error| error.to_string())?;
    Ok(McpSettingsSnapshot {
        client_command: state.client_command.clone(),
        endpoint: "local-ipc".to_string(),
        revision: document.revision,
        config: document.config,
        health: state.controller.health(),
        workspace,
    })
}

#[cfg(any(desktop, feature = "desktop-sidecar"))]
async fn apply_server_config(state: &McpState) -> Result<(), String> {
    let document = state.config.snapshot().map_err(|error| error.to_string())?;
    state
        .controller
        .restart(McpServerOptions::from_config(&document.config))
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(any(desktop, feature = "desktop-sidecar"))]
fn server_needs_apply(state: &McpState, previous: &McpConfig, current: &McpConfig) -> bool {
    previous.enabled != current.enabled
        || previous.request_limit_bytes != current.request_limit_bytes
        || current.enabled != matches!(state.controller.health().state, McpServerState::Running)
}

#[cfg(any(desktop, feature = "desktop-sidecar"))]
fn recycle_retention_for_cleanup(config: &McpConfig) -> Option<u16> {
    (config.enabled && config.recycle_bin_retention_days > 0)
        .then_some(config.recycle_bin_retention_days)
}

#[cfg(any(desktop, feature = "desktop-sidecar"))]
fn current_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(any(desktop, feature = "desktop-sidecar"))]
async fn run_recycle_cleanup(state: Arc<McpState>) {
    let retention_days = state
        .config
        .snapshot()
        .ok()
        .and_then(|document| recycle_retention_for_cleanup(&document.config));
    let Some(retention_days) = retention_days else {
        return;
    };
    let recycle_root = state.recycle_root.clone();
    let cleanup = tauri::async_runtime::spawn_blocking(move || {
        clean_expired_entries(&recycle_root, retention_days, current_time_millis())
    })
    .await;
    match cleanup {
        Ok(report) if report.failed > 0 => {
            eprintln!(
                "QingYu recycle-bin cleanup skipped {} failed entries.",
                report.failed
            );
        }
        Err(_) => eprintln!("QingYu recycle-bin cleanup worker was unavailable."),
        _ => {}
    }
}

#[cfg(any(desktop, feature = "desktop-sidecar"))]
fn spawn_recycle_cleanup(state: Arc<McpState>) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(RECYCLE_CLEANUP_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            run_recycle_cleanup(Arc::clone(&state)).await;
        }
    });
}

#[cfg(any(desktop, feature = "desktop-sidecar"))]
#[tauri::command]
pub(crate) fn get_mcp_settings(
    state: tauri::State<'_, Arc<McpState>>,
) -> Result<McpSettingsSnapshot, String> {
    settings_snapshot(&state)
}

#[cfg(any(desktop, feature = "desktop-sidecar"))]
#[tauri::command]
pub(crate) async fn update_mcp_settings(
    state: tauri::State<'_, Arc<McpState>>,
    input: UpdateMcpSettingsInput,
) -> Result<McpSettingsSnapshot, String> {
    let _activation_guard = state.activation.lock().await;
    let previous = state.config.snapshot().map_err(|error| error.to_string())?;
    let updated = state
        .config
        .update(input.config, &input.expected_revision)
        .map_err(|error| error.to_string())?;
    state
        .audit
        .update_policy(updated.config.audit.clone())
        .map_err(|error| error.to_string())?;
    state.policy.invalidate_previews();
    state.controller.notify_tools_changed().await;
    if server_needs_apply(&state, &previous.config, &updated.config) {
        apply_server_config(&state).await?;
    }
    if updated.config.enabled
        && (!previous.config.enabled
            || previous.config.recycle_bin_retention_days
                != updated.config.recycle_bin_retention_days)
    {
        let cleanup_state = Arc::clone(state.inner());
        tauri::async_runtime::spawn(async move {
            run_recycle_cleanup(cleanup_state).await;
        });
    }
    settings_snapshot(&state)
}

#[cfg(any(desktop, feature = "desktop-sidecar"))]
#[tauri::command]
pub(crate) async fn set_mcp_primary_workspace(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Arc<McpState>>,
    primary_root: Option<String>,
) -> Result<McpSettingsSnapshot, String> {
    require_mcp_primary_window(window.label())?;
    let _activation_guard = state.activation.lock().await;
    let previous_generation = state.workspaces.generation();
    let result =
        crate::primary_workspace::with_primary_workspace_transaction(&app, |authoritative| {
            apply_primary_workspace_transaction(
                &state.workspaces,
                primary_root.as_deref(),
                authoritative,
            )
        });
    let runtime_change =
        mcp_runtime_change_event(previous_generation, state.workspaces.generation());
    if let Some(runtime_change) = runtime_change {
        state.policy.invalidate_previews();
        state.controller.notify_tools_changed().await;
        let _event_result = app.emit(MCP_RUNTIME_CHANGED_EVENT, runtime_change);
    }
    result?;
    settings_snapshot(&state)
}

#[cfg(all(test, any(desktop, feature = "desktop-sidecar")))]
fn set_primary_workspace_from_window(
    window_label: &str,
    workspaces: &WorkspaceRegistry,
    requested_root: Option<&str>,
    authoritative: Result<std::path::PathBuf, String>,
) -> Result<bool, String> {
    require_mcp_primary_window(window_label)?;
    apply_primary_workspace_transaction(workspaces, requested_root, authoritative)
}

#[cfg(any(desktop, feature = "desktop-sidecar"))]
fn require_mcp_primary_window(window_label: &str) -> Result<(), String> {
    if window_label == "main" {
        Ok(())
    } else {
        Err(
            "mcp-primary-window-required: Only QingYu's main window may change MCP document authority."
                .to_string(),
        )
    }
}

#[cfg(any(desktop, feature = "desktop-sidecar"))]
fn apply_primary_workspace_transaction(
    workspaces: &WorkspaceRegistry,
    requested_root: Option<&str>,
    authoritative: Result<std::path::PathBuf, String>,
) -> Result<bool, String> {
    let previous_generation = workspaces.generation();
    let Some(requested) = requested_root
        .map(str::trim)
        .filter(|path| !path.is_empty())
    else {
        workspaces
            .clear_current()
            .map_err(|error| error.to_string())?;
        return Ok(workspaces.generation() != previous_generation);
    };

    let authoritative = match authoritative {
        Ok(root) => root,
        Err(_) => {
            workspaces
                .clear_current()
                .map_err(|error| error.to_string())?;
            return Err(
                "mcp-workspace-unavailable: The configured primary notes workspace is unavailable."
                    .to_string(),
            );
        }
    };
    let requested = crate::workspace_membership::canonical_workspace_root(requested);
    if !requested
        .as_ref()
        .is_ok_and(|requested| requested == &authoritative)
    {
        if workspaces.activate_current(&authoritative).is_err() {
            workspaces
                .clear_current()
                .map_err(|error| error.to_string())?;
        }
        return Err(
            "mcp-primary-workspace-mismatch: The requested directory is not the configured primary notes workspace."
                .to_string(),
        );
    }

    if let Err(error) = workspaces.activate_current(&authoritative) {
        workspaces
            .clear_current()
            .map_err(|clear_error| clear_error.to_string())?;
        return Err(error.to_string());
    }
    Ok(workspaces.generation() != previous_generation)
}

#[cfg(any(desktop, feature = "desktop-sidecar"))]
fn apply_authoritative_primary_workspace(
    workspaces: &WorkspaceRegistry,
    authoritative: Result<std::path::PathBuf, String>,
) -> Result<bool, String> {
    let previous_generation = workspaces.generation();
    let authoritative = match authoritative {
        Ok(root) => root,
        Err(_) => {
            workspaces
                .clear_current()
                .map_err(|error| error.to_string())?;
            return Err(
                "mcp-workspace-unavailable: The configured primary notes workspace is unavailable."
                    .to_string(),
            );
        }
    };
    if let Err(error) = workspaces.activate_current(&authoritative) {
        workspaces
            .clear_current()
            .map_err(|clear_error| clear_error.to_string())?;
        return Err(error.to_string());
    }
    Ok(workspaces.generation() != previous_generation)
}

#[cfg(all(test, any(desktop, feature = "desktop-sidecar")))]
fn test_apply_authoritative_primary_workspace(
    workspaces: &WorkspaceRegistry,
    authoritative: Result<std::path::PathBuf, String>,
) -> Result<bool, String> {
    apply_authoritative_primary_workspace(workspaces, authoritative)
}

#[cfg(all(test, any(desktop, feature = "desktop-sidecar")))]
async fn reload_current_mcp_policy(state: &McpState) -> Result<(), String> {
    let previous = state.config.snapshot().map_err(|error| error.to_string())?;
    let current = state.config.reload().map_err(|error| error.to_string())?;
    state
        .audit
        .update_policy(current.config.audit.clone())
        .map_err(|error| error.to_string())?;
    if previous.revision != current.revision {
        state.policy.invalidate_previews();
        state.controller.notify_tools_changed().await;
    }
    if server_needs_apply(&state, &previous.config, &current.config) {
        apply_server_config(&state).await?;
    }
    Ok(())
}

#[cfg(any(desktop, feature = "desktop-sidecar"))]
#[tauri::command]
pub(crate) fn get_mcp_health(state: tauri::State<'_, Arc<McpState>>) -> server::McpServerHealth {
    state.controller.health()
}

#[cfg(any(desktop, feature = "desktop-sidecar"))]
#[tauri::command]
pub(crate) fn list_mcp_audit_entries(
    state: tauri::State<'_, Arc<McpState>>,
    offset: usize,
    limit: usize,
) -> Result<Vec<AuditEntry>, String> {
    state
        .audit
        .list(offset, limit)
        .map_err(|error| error.to_string())
}

#[cfg(any(desktop, feature = "desktop-sidecar"))]
#[tauri::command]
pub(crate) fn clear_mcp_audit_entries(
    state: tauri::State<'_, Arc<McpState>>,
) -> Result<(), String> {
    state.audit.clear().map_err(|error| error.to_string())
}

#[cfg(any(desktop, feature = "desktop-sidecar"))]
pub(crate) fn new_process_key() -> Result<[u8; 32], String> {
    let mut key = [0_u8; 32];
    getrandom::fill(&mut key)
        .map_err(|_| "mcp-session-key-unavailable: QingYu could not initialize MCP.".to_string())?;
    Ok(key)
}
