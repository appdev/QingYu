use std::sync::Arc;

use qingyu_kernel::mcp::{
    McpConfig, McpConfigError, McpLocalSettingsService, McpPolicyEventSink,
    MCP_POLICY_CHANGED_EVENT,
};
use tauri::{Emitter, Manager, Runtime};

struct TauriMcpPolicyEventSink<R: Runtime> {
    app: tauri::AppHandle<R>,
}

impl<R: Runtime> McpPolicyEventSink for TauriMcpPolicyEventSink<R> {
    fn emit(&self, config: &McpConfig) -> Result<(), McpConfigError> {
        self.app
            .emit(
                MCP_POLICY_CHANGED_EVENT,
                serde_json::json!({ "config": config }),
            )
            .map_err(|_| McpConfigError::write())
    }
}

pub(crate) fn mcp_local_settings_service_from_app<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<McpLocalSettingsService, McpConfigError> {
    let config_root = app
        .path()
        .app_data_dir()
        .map_err(|_| McpConfigError::read())?;
    Ok(McpLocalSettingsService::at_config_root_with_events(
        config_root,
        Arc::new(TauriMcpPolicyEventSink { app: app.clone() }),
    ))
}
