#[cfg(desktop)]
mod app_exit;
#[cfg(desktop)]
mod app_logs;
mod app_settings;
mod atomic_noreplace;
#[cfg(desktop)]
mod clipboard;
#[cfg(desktop)]
mod desktop_runtime;
#[cfg(desktop)]
mod fonts;
#[cfg(desktop)]
mod language;
mod managed_workspace;
mod markdown_files;
mod mcp;
#[cfg(desktop)]
mod menu;
#[cfg(desktop)]
mod menu_labels;
#[cfg(any(mobile, test))]
mod mobile_back;
#[cfg(mobile)]
mod mobile_runtime;
mod notebook_scope;
#[cfg(desktop)]
mod opened_files;
mod primary_workspace;
mod protected_paths;
mod remote_sync;
mod s3_http;
#[cfg(desktop)]
mod shell_command;
mod storage_capability;
mod sync_config;
mod sync_validation;
#[cfg(desktop)]
mod text_file;
mod themes;
mod watcher;
mod web_http;
#[cfg(desktop)]
mod window_state;
#[cfg(desktop)]
mod windows;
mod workspace_membership;

#[cfg(test)]
mod builder_boundary_tests;
#[cfg(test)]
mod mobile_platform_config_tests;

#[cfg(any(desktop, feature = "desktop-sidecar"))]
pub async fn run_mcp_bridge() -> Result<(), impl std::error::Error> {
    mcp::bridge::run_bridge_for_app().await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(desktop)]
    desktop_runtime::run();

    #[cfg(mobile)]
    mobile_runtime::run();
}
