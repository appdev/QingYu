use crate::markdown_files::MarkdownTreeLoadState;
use crate::mobile_back::MobileBackState;
use crate::watcher::{MarkdownFileWatcherState, MarkdownTreeWatcherState};

pub(crate) fn run() {
    tauri::Builder::default()
        .manage(MarkdownFileWatcherState::default())
        .manage(MarkdownTreeWatcherState::default())
        .manage(MarkdownTreeLoadState::default())
        .manage(MobileBackState::default())
        .manage(crate::themes::ThemeActivationState::default())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_log::Builder::new().build())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            crate::app_settings::get_mcp_policy,
            crate::app_settings::update_mcp_policy,
            crate::app_settings::read_app_settings_group,
            crate::app_settings::write_app_settings_group,
            crate::app_settings::replace_portable_app_settings,
            crate::primary_workspace::read_primary_workspace_state,
            crate::primary_workspace::write_primary_workspace_state,
            crate::themes::list_themes,
            crate::themes::read_theme_css,
            crate::themes::activation::prepare_theme_activation,
            crate::themes::activation::commit_theme_activation,
            crate::themes::activation::cancel_theme_activation,
            crate::themes::activation::release_theme_activation,
            crate::themes::delete_theme,
            crate::markdown_files::tree::list_markdown_files_for_path,
            crate::markdown_files::tree::load_markdown_files_for_path,
            crate::markdown_files::tree::cancel_markdown_files_load,
            crate::markdown_files::search::search_markdown_files_for_path,
            crate::markdown_files::tree::create_markdown_tree_file,
            crate::markdown_files::tree::create_markdown_tree_folder,
            crate::markdown_files::tree::rename_markdown_tree_file,
            crate::markdown_files::tree::move_markdown_tree_file,
            crate::markdown_files::tree::delete_markdown_tree_file,
            crate::markdown_files::document::read_markdown_file,
            crate::markdown_files::history::list_markdown_file_history,
            crate::markdown_files::history::read_markdown_file_history,
            crate::markdown_files::document::write_markdown_file,
            crate::watcher::watch_markdown_file,
            crate::watcher::unwatch_markdown_file,
            crate::watcher::watch_markdown_tree,
            crate::watcher::unwatch_markdown_tree,
            crate::sync_config::load_sync_config,
            crate::sync_config::enable_sync_config,
            crate::sync_config::patch_sync_config,
            crate::sync_config::recover_sync_config,
            crate::sync_config::reset_sync_config,
            crate::sync_config::load_sync_config_editing,
            crate::sync_config::set_sync_config_editing,
            crate::sync_config::request_sync_config_apply,
            crate::sync_config::cancel_sync_config_apply,
            crate::sync_config::load_sync_status,
            crate::sync_config::list_remote_notebooks,
            crate::sync_config::sync_application,
            crate::sync_config::test_sync_connection,
            crate::workspace_membership::is_document_in_workspace,
            crate::managed_workspace::resolve_managed_workspace_root,
            crate::managed_workspace::list_managed_workspace_names,
            crate::markdown_files::image::save_clipboard_image,
            crate::web_http::download_web_image,
            crate::mobile_back::complete_mobile_back,
        ])
        .build(tauri::generate_context!())
        .expect("error while building QingYu mobile")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested {
                code: None, api, ..
            } = event
            {
                api.prevent_exit();
                crate::mobile_back::emit_mobile_back_requested(app);
            }
        });
}
