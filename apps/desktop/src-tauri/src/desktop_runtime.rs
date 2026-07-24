use std::{ffi::OsStr, path::Path, time::Duration};

use crate::app_exit::handle_app_exit_requested;
use crate::markdown_files::MarkdownTreeLoadState;
use crate::mcp;
use crate::menu::{
    apply_native_application_menu_for_window_event, create_application_menu,
    emit_native_menu_command_payload, is_native_new_window_command, native_menu_command_from_id,
    remember_native_menu_webview_window, remember_native_menu_window_from_event,
    NativeApplicationMenuState, NativeMenuTargetState,
};
use crate::opened_files::{
    opened_markdown_paths_from_args, opened_markdown_paths_from_args_with_cwd,
    opened_markdown_paths_from_urls, queue_opened_markdown_paths, OpenedMarkdownPathsState,
};
use crate::watcher::{MarkdownFileWatcherState, MarkdownTreeWatcherState};
use crate::window_state::{remove_editor_window_restore_state, EditorWindowRestoreState};
use crate::windows::{
    apply_main_window_chrome, apply_settings_window_lifecycle, apply_webview_window_chrome,
    apply_window_event_chrome, editor_window_url_for_path, is_editor_window_label,
    spawn_blank_editor_window, spawn_editor_window, spawn_restorable_editor_window,
};
use tauri::{Emitter, EventTarget, Manager};
use tauri_plugin_window_state::StateFlags;

const STARTUP_WINDOW_NATIVE_REVEAL_FALLBACK_MS: u64 = 2400;
const DESKTOP_LOG_MAX_FILE_SIZE_BYTES: u128 = 2 * 1024 * 1024;
const DESKTOP_LOG_MAX_FILE_COUNT: usize = 5;
// tauri-plugin-log's KeepSome count applies only to archived files; the active
// log file is additional, so keep one fewer archive to cap total files.
const DESKTOP_LOG_ARCHIVED_FILE_COUNT: usize = DESKTOP_LOG_MAX_FILE_COUNT - 1;
const PRIMARY_CLOUD_NOTEBOOK_CATALOG_REQUESTED_EVENT: &str =
    "qingyu://cloud-notebook-catalog-requested";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DesktopLaunchMode {
    Normal,
    McpService,
}

fn desktop_launch_mode<I, S>(args: I) -> DesktopLaunchMode
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut args = args.into_iter();
    let Some(_executable) = args.next() else {
        return DesktopLaunchMode::Normal;
    };
    let command = args.next();
    let subcommand = args.next();
    let has_more = args.next().is_some();
    if command.as_ref().map(AsRef::as_ref) == Some(OsStr::new("mcp"))
        && subcommand.as_ref().map(AsRef::as_ref) == Some(OsStr::new("serve"))
        && !has_more
    {
        DesktopLaunchMode::McpService
    } else {
        DesktopLaunchMode::Normal
    }
}

fn should_reveal_single_instance(mode: DesktopLaunchMode) -> bool {
    mode == DesktopLaunchMode::Normal
}

fn activate_normal_ui<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    #[cfg(target_os = "macos")]
    if let Err(error) = app.set_activation_policy(tauri::ActivationPolicy::Regular) {
        eprintln!("QingYu activation policy update failed: {error}");
    }
    #[cfg(target_os = "macos")]
    if let Err(error) = app.set_dock_visibility(true) {
        eprintln!("QingYu Dock visibility update failed: {error}");
    }
    #[cfg(not(target_os = "macos"))]
    let _app = app;
}

#[cfg(test)]
fn test_desktop_launch_mode(args: &[&str]) -> &'static str {
    match desktop_launch_mode(args) {
        DesktopLaunchMode::Normal => "normal",
        DesktopLaunchMode::McpService => "mcp-service",
    }
}

#[cfg(test)]
fn test_should_reveal_single_instance(mode: &str) -> bool {
    let mode = match mode {
        "mcp-service" => DesktopLaunchMode::McpService,
        _ => DesktopLaunchMode::Normal,
    };
    should_reveal_single_instance(mode)
}

trait PrimaryCloudNotebookCatalogWindow {
    fn show_catalog_window(&self) -> Result<(), String>;
    fn focus_catalog_window(&self) -> Result<(), String>;
    fn emit_catalog_request(&self) -> Result<(), String>;
}

impl<R: tauri::Runtime> PrimaryCloudNotebookCatalogWindow for tauri::WebviewWindow<R> {
    fn show_catalog_window(&self) -> Result<(), String> {
        self.show()
            .map_err(|error| format!("primary-cloud-catalog-show-failed: {error}"))
    }

    fn focus_catalog_window(&self) -> Result<(), String> {
        self.set_focus()
            .map_err(|error| format!("primary-cloud-catalog-focus-failed: {error}"))
    }

    fn emit_catalog_request(&self) -> Result<(), String> {
        self.emit_to(
            EventTarget::webview_window(self.label()),
            PRIMARY_CLOUD_NOTEBOOK_CATALOG_REQUESTED_EVENT,
            (),
        )
        .map_err(|error| format!("primary-cloud-catalog-emit-failed: {error}"))
    }
}

fn deliver_primary_cloud_notebook_catalog_request<W: PrimaryCloudNotebookCatalogWindow>(
    window: Option<&W>,
) -> Result<(), String> {
    let window = window.ok_or_else(|| {
        "primary-cloud-catalog-main-window-missing: The primary window is unavailable.".to_string()
    })?;
    window.show_catalog_window()?;
    window.focus_catalog_window()?;
    window.emit_catalog_request()?;
    Ok(())
}

fn window_state_restore_flags() -> StateFlags {
    StateFlags::all() - StateFlags::VISIBLE - StateFlags::DECORATIONS
}

fn focus_main_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn editor_window_urls_for_opened_markdown_paths(paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .filter_map(|path| {
            let opened_path = Path::new(path);
            if opened_path.is_file() {
                return Some(editor_window_url_for_path(path));
            }

            None
        })
        .collect()
}

fn opened_paths_require_primary_notebook_switch(paths: &[String]) -> bool {
    paths.iter().any(|path| Path::new(path).is_dir())
}

fn reveal_or_open_markdown_paths<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    paths: Vec<String>,
    reveal_when_empty: bool,
) {
    if paths.is_empty() && !reveal_when_empty {
        return;
    }

    if app.get_webview_window("main").is_some() {
        queue_opened_markdown_paths(app, paths);
        focus_main_window(app);
        return;
    }

    if opened_paths_require_primary_notebook_switch(&paths) {
        queue_opened_markdown_paths(app, paths);
        spawn_restorable_editor_window(app.clone());
        focus_main_window(app);
        return;
    }

    let urls = editor_window_urls_for_opened_markdown_paths(&paths);
    if urls.is_empty() {
        spawn_restorable_editor_window(app.clone());
        return;
    }

    for url in urls {
        spawn_editor_window(app.clone(), url);
    }
}

#[tauri::command]
pub(crate) fn request_primary_notebook_switch(
    app: tauri::AppHandle,
    path: String,
) -> Result<(), String> {
    let folder = crate::markdown_files::open::resolve_markdown_folder(path)?;
    reveal_or_open_markdown_paths(&app, vec![folder], false);
    Ok(())
}

#[tauri::command]
pub(crate) fn request_primary_cloud_notebook_catalog(app: tauri::AppHandle) -> Result<(), String> {
    let window = app.get_webview_window("main");
    deliver_primary_cloud_notebook_catalog_request(window.as_ref())
}

fn show_main_window_if_hidden<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        if window.is_visible().unwrap_or(false) {
            return;
        }

        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn has_visible_editor_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> bool {
    app.webview_windows().values().any(|window| {
        is_editor_window_label(window.label()) && window.is_visible().unwrap_or(false)
    })
}

fn spawn_startup_window_reveal_fallback<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let app = app.clone();

    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(
            STARTUP_WINDOW_NATIVE_REVEAL_FALLBACK_MS,
        ));
        show_main_window_if_hidden(&app);
    });
}

pub(crate) fn run() {
    let launch_mode = desktop_launch_mode(std::env::args_os());
    let mut context = tauri::generate_context!();
    if launch_mode == DesktopLaunchMode::McpService {
        context.config_mut().app.windows.clear();
    }

    let builder = tauri::Builder::default()
        .manage(MarkdownFileWatcherState::default())
        .manage(MarkdownTreeWatcherState::default())
        .manage(MarkdownTreeLoadState::default())
        .manage(OpenedMarkdownPathsState::default())
        .manage(NativeApplicationMenuState::default())
        .manage(NativeMenuTargetState::default())
        .manage(EditorWindowRestoreState::default())
        .manage(crate::themes::ThemeActivationState::default());

    #[cfg(any(target_os = "macos", windows, target_os = "linux"))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
        let launch_mode = desktop_launch_mode(&args);
        if !should_reveal_single_instance(launch_mode) {
            return;
        }
        activate_normal_ui(app);
        reveal_or_open_markdown_paths(
            app,
            opened_markdown_paths_from_args_with_cwd(args, std::path::PathBuf::from(cwd)),
            true,
        );
    }));

    #[cfg(any(target_os = "macos", windows, target_os = "linux"))]
    let builder = builder.plugin(
        tauri_plugin_window_state::Builder::default()
            .with_state_flags(window_state_restore_flags())
            .build(),
    );

    builder
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_log::Builder::new()
                .max_file_size(DESKTOP_LOG_MAX_FILE_SIZE_BYTES)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepSome(
                    DESKTOP_LOG_ARCHIVED_FILE_COUNT,
                ))
                .build(),
        )
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(move |app| {
            if launch_mode == DesktopLaunchMode::McpService {
                #[cfg(target_os = "macos")]
                app.set_activation_policy(tauri::ActivationPolicy::Prohibited);
            } else {
                apply_main_window_chrome(app);
                spawn_startup_window_reveal_fallback(&app.handle());
                if let Some(window) = app.get_webview_window("main") {
                    remember_native_menu_webview_window(&window);
                }
                let paths = opened_markdown_paths_from_args(std::env::args());
                reveal_or_open_markdown_paths(&app.handle(), paths, false);
            }
            match mcp::initialize(&app.handle()) {
                Ok(state) => {
                    app.manage(state);
                }
                Err(error) => {
                    eprintln!("QingYu MCP initialization failed: {error}");
                }
            }
            Ok(())
        })
        .on_page_load(|webview, _| {
            apply_webview_window_chrome(webview);
        })
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::Destroyed) {
                if let Err(error) = crate::themes::release_theme_activation_for_window(
                    &window.app_handle(),
                    window.label(),
                ) {
                    eprintln!("Theme activation cleanup failed: {error}");
                }
            }
            remember_native_menu_window_from_event(window, event);
            apply_native_application_menu_for_window_event(window, event);
            apply_window_event_chrome(window, event);
            apply_settings_window_lifecycle(&window.app_handle(), window, event);
            remove_editor_window_restore_state(window, event);
        })
        .menu(create_application_menu)
        .on_menu_event(|app, event| {
            let command = event.id().as_ref();
            if is_native_new_window_command(command) {
                spawn_blank_editor_window(app.clone());
                return;
            }

            let Some(payload) = native_menu_command_from_id(app, command) else {
                return;
            };

            emit_native_menu_command_payload(app, payload);
        })
        .invoke_handler(tauri::generate_handler![
            crate::mcp::get_mcp_settings,
            crate::mcp::update_mcp_settings,
            crate::mcp::set_mcp_primary_workspace,
            crate::mcp::get_mcp_health,
            crate::mcp::list_mcp_audit_entries,
            crate::mcp::clear_mcp_audit_entries,
            crate::app_settings::read_app_settings_group,
            crate::app_settings::write_app_settings_group,
            crate::app_settings::replace_portable_app_settings,
            crate::app_settings::read_exposed_app_settings,
            crate::app_settings::patch_exposed_app_settings,
            crate::primary_workspace::read_primary_workspace_state,
            crate::primary_workspace::write_primary_workspace_state,
            crate::primary_workspace::prepare_desktop_notebook_target,
            crate::primary_workspace::discard_prepared_desktop_notebook_target,
            crate::themes::list_themes,
            crate::themes::read_theme_css,
            crate::themes::activation::prepare_theme_activation,
            crate::themes::activation::commit_theme_activation,
            crate::themes::activation::cancel_theme_activation,
            crate::themes::activation::release_theme_activation,
            crate::themes::import_theme_file,
            crate::themes::replace_theme_file,
            crate::themes::delete_theme,
            crate::themes::theme_directory_path,
            crate::markdown_files::tree::list_markdown_files_for_path,
            crate::markdown_files::tree::load_markdown_files_for_path,
            crate::markdown_files::tree::cancel_markdown_files_load,
            crate::markdown_files::search::search_markdown_files_for_path,
            crate::markdown_files::tree::create_markdown_tree_file,
            crate::markdown_files::tree::create_markdown_tree_folder,
            crate::menu::install_application_menu,
            crate::menu::show_native_app_about,
            crate::markdown_files::tree::rename_markdown_tree_file,
            crate::markdown_files::tree::move_markdown_tree_file,
            crate::markdown_files::tree::delete_markdown_tree_file,
            crate::markdown_files::open::open_markdown_file_in_new_window,
            crate::markdown_files::open::open_containing_folder,
            crate::markdown_files::open::open_markdown_attachment,
            crate::markdown_files::open::resolve_markdown_path,
            crate::markdown_files::open::resolve_markdown_folder,
            crate::markdown_files::resource::resolve_workspace_resource_root,
            crate::markdown_files::resource::trash_workspace_resources,
            crate::markdown_files::document::read_markdown_file,
            crate::text_file::read_text_file,
            crate::markdown_files::history::list_markdown_file_history,
            crate::markdown_files::history::read_markdown_file_history,
            crate::markdown_files::attachment::import_local_file,
            crate::markdown_files::image::read_local_image_file,
            crate::markdown_files::template::read_markdown_template_file,
            crate::markdown_files::template::write_markdown_template_file,
            crate::markdown_files::template::delete_markdown_template_file,
            crate::markdown_files::attachment::save_clipboard_attachment,
            crate::markdown_files::image::save_clipboard_image,
            crate::markdown_files::path::canonical_local_file_path,
            crate::clipboard::read_clipboard_text,
            crate::windows::minimize_current_window,
            crate::windows::open_blank_editor_window,
            crate::windows::open_settings_window,
            crate::windows::mark_settings_window_ready,
            crate::windows::hide_settings_window,
            crate::windows::acknowledge_settings_window_hide,
            crate::windows::cancel_settings_window_hide,
            crate::windows::complete_settings_window_hide,
            crate::windows::destroy_current_editor_window,
            crate::sync_config::sync_application,
            crate::sync_config::test_sync_connection,
            crate::sync_config::list_remote_notebooks,
            crate::workspace_membership::is_document_in_workspace,
            crate::web_http::download_web_image,
            crate::markdown_files::document::write_markdown_file,
            crate::text_file::write_text_file,
            crate::markdown_files::export::export_pdf_file,
            crate::markdown_files::export::check_pandoc_available,
            crate::markdown_files::export::detect_pandoc_path,
            crate::markdown_files::export::export_pandoc_file,
            crate::watcher::watch_markdown_file,
            crate::watcher::unwatch_markdown_file,
            crate::watcher::watch_markdown_tree,
            crate::watcher::unwatch_markdown_tree,
            request_primary_notebook_switch,
            request_primary_cloud_notebook_catalog,
            crate::opened_files::take_opened_markdown_paths,
            crate::shell_command::get_shell_command_status,
            crate::shell_command::install_shell_command,
            crate::shell_command::uninstall_shell_command,
            crate::window_state::set_editor_window_restore_state,
            crate::window_state::list_editor_window_restore_states,
            crate::fonts::list_system_font_families,
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
            crate::managed_workspace::resolve_managed_workspace_root,
            crate::managed_workspace::list_managed_workspace_names,
            crate::app_logs::open_log_folder,
        ])
        .build(context)
        .expect("error while building QingYu")
        .run(|app, event| match event {
            tauri::RunEvent::ExitRequested { code, api, .. } => {
                handle_app_exit_requested(app, code, api);
            }
            tauri::RunEvent::Exit => {
                if let Some(state) = app.try_state::<std::sync::Arc<mcp::McpState>>() {
                    let controller = state.controller.clone();
                    tauri::async_runtime::block_on(async move {
                        let _stop_result = controller.stop().await;
                    });
                }
            }
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Opened { urls } => {
                activate_normal_ui(app);
                reveal_or_open_markdown_paths(app, opened_markdown_paths_from_urls(&urls), false);
            }
            #[cfg(any(target_os = "ios", target_os = "android"))]
            tauri::RunEvent::Opened { urls } => {
                queue_opened_markdown_paths(app, opened_markdown_paths_from_urls(&urls));
            }
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { .. } => {
                activate_normal_ui(app);
                // Settings may stay visible after prewarm. Treating that as an editor would skip
                // workspace restore when the user reopens QingYu from the Dock.
                if !has_visible_editor_window(app) {
                    reveal_or_open_markdown_paths(app, Vec::new(), true);
                }
            }
            _ => {}
        });
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum CatalogDeliveryFailure {
        Show,
        Focus,
        Emit,
    }

    struct TestCatalogWindow {
        failure: Option<CatalogDeliveryFailure>,
        operations: Rc<RefCell<Vec<&'static str>>>,
    }

    impl super::PrimaryCloudNotebookCatalogWindow for TestCatalogWindow {
        fn show_catalog_window(&self) -> Result<(), String> {
            self.operations.borrow_mut().push("show");
            if self.failure == Some(CatalogDeliveryFailure::Show) {
                return Err("show failed".to_string());
            }
            Ok(())
        }

        fn focus_catalog_window(&self) -> Result<(), String> {
            self.operations.borrow_mut().push("focus");
            if self.failure == Some(CatalogDeliveryFailure::Focus) {
                return Err("focus failed".to_string());
            }
            Ok(())
        }

        fn emit_catalog_request(&self) -> Result<(), String> {
            self.operations.borrow_mut().push("emit");
            if self.failure == Some(CatalogDeliveryFailure::Emit) {
                return Err("emit failed".to_string());
            }
            Ok(())
        }
    }

    fn test_catalog_window(
        failure: Option<CatalogDeliveryFailure>,
    ) -> (TestCatalogWindow, Rc<RefCell<Vec<&'static str>>>) {
        let operations = Rc::new(RefCell::new(Vec::new()));
        (
            TestCatalogWindow {
                failure,
                operations: operations.clone(),
            },
            operations,
        )
    }

    #[test]
    fn cloud_catalog_request_shows_focuses_and_notifies_only_the_primary_window() {
        let (window, operations) = test_catalog_window(None);

        super::deliver_primary_cloud_notebook_catalog_request(Some(&window))
            .expect("catalog request should be delivered");

        assert_eq!(*operations.borrow(), ["show", "focus", "emit"]);
        assert_eq!(
            super::PRIMARY_CLOUD_NOTEBOOK_CATALOG_REQUESTED_EVENT,
            ["qingyu://cloud", "-notebook-catalog-requested"].concat()
        );
    }

    #[test]
    fn mcp_serve_selects_headless_service_mode() {
        assert_eq!(
            super::test_desktop_launch_mode(&["markra", "mcp", "serve"]),
            "mcp-service"
        );
    }

    #[test]
    fn ordinary_launch_selects_normal_mode() {
        assert_eq!(super::test_desktop_launch_mode(&["markra"]), "normal");
        assert_eq!(
            super::test_desktop_launch_mode(&["markra", "mcp", "serve", "unexpected"]),
            "normal"
        );
    }

    #[test]
    fn service_single_instance_invocation_does_not_reveal_window() {
        assert!(!super::test_should_reveal_single_instance("mcp-service"));
    }

    #[test]
    fn ordinary_single_instance_invocation_reveals_window() {
        assert!(super::test_should_reveal_single_instance("normal"));
    }

    #[test]
    fn normal_ui_promotion_restores_dock_visibility() {
        let source = include_str!("desktop_runtime.rs");
        let activation_start = source
            .find("fn activate_normal_ui")
            .expect("normal UI activation helper should exist");
        let activation_end = source[activation_start..]
            .find("\n\n#[cfg(test)]")
            .map(|offset| activation_start + offset)
            .expect("normal UI activation helper should have a boundary");
        let activation_source = &source[activation_start..activation_end];
        let regular_policy = activation_source
            .find("app.set_activation_policy(tauri::ActivationPolicy::Regular)")
            .expect("normal UI activation should restore the regular policy");
        let dock_visibility = activation_source
            .find("app.set_dock_visibility(true)")
            .expect("normal UI activation should restore Dock visibility");

        assert!(regular_policy < dock_visibility);
    }

    #[test]
    fn mcp_service_runtime_clears_startup_windows_before_build() {
        let source = include_str!("desktop_runtime.rs");
        let build = source
            .find(".build(context)")
            .expect("desktop runtime should build with a mutable context");
        let clear = source
            .find("context.config_mut().app.windows.clear();")
            .expect("MCP service mode should remove configured startup windows");

        assert!(clear < build);
        assert!(source.contains("DesktopLaunchMode::McpService"));
    }

    #[test]
    fn macos_mcp_service_cannot_become_the_active_application() {
        let source = include_str!("desktop_runtime.rs");
        let setup_start = source
            .find(".setup(move |app| {")
            .expect("desktop setup hook should exist");
        let setup_end = source[setup_start..]
            .find(".on_page_load")
            .map(|offset| setup_start + offset)
            .expect("desktop setup hook should have a boundary");
        let setup_source = &source[setup_start..setup_end];

        assert!(setup_source
            .contains("app.set_activation_policy(tauri::ActivationPolicy::Prohibited);"));
        assert!(!setup_source
            .contains("app.set_activation_policy(tauri::ActivationPolicy::Accessory);"));
    }

    #[test]
    fn cloud_catalog_event_uses_an_exact_webview_window_target() {
        let source = include_str!("desktop_runtime.rs");
        let implementation_start = source
            .find("impl<R: tauri::Runtime> PrimaryCloudNotebookCatalogWindow")
            .expect("the native catalog window implementation should exist");
        let implementation_end = source[implementation_start..]
            .find("\nfn deliver_primary_cloud_notebook_catalog_request")
            .map(|offset| implementation_start + offset)
            .expect("the native catalog window implementation should have a boundary");
        let implementation = &source[implementation_start..implementation_end];

        assert!(
            implementation.contains("self.emit_to("),
            "WebviewWindow::emit broadcasts globally in Tauri 2.11"
        );
        assert!(
            implementation.contains("EventTarget::webview_window(self.label())"),
            "the request must target exactly the selected primary webview window"
        );
        assert!(
            !implementation.contains("self.emit("),
            "broadcast delivery must not be used for the catalog request"
        );
    }

    #[test]
    fn cloud_catalog_request_returns_an_error_for_every_delivery_failure() {
        assert!(
            super::deliver_primary_cloud_notebook_catalog_request::<TestCatalogWindow>(None)
                .expect_err("missing main window must fail")
                .contains("main")
        );

        for failure in [
            CatalogDeliveryFailure::Show,
            CatalogDeliveryFailure::Focus,
            CatalogDeliveryFailure::Emit,
        ] {
            let (window, operations) = test_catalog_window(Some(failure));
            assert!(super::deliver_primary_cloud_notebook_catalog_request(Some(&window)).is_err());
            let expected = match failure {
                CatalogDeliveryFailure::Show => vec!["show"],
                CatalogDeliveryFailure::Focus => vec!["show", "focus"],
                CatalogDeliveryFailure::Emit => vec!["show", "focus", "emit"],
            };
            assert_eq!(*operations.borrow(), expected);
        }
    }

    #[test]
    fn desktop_registers_the_primary_cloud_catalog_request_command() {
        let source = include_str!("desktop_runtime.rs");
        let command = ["request_primary_cloud_notebook", "_catalog,"].concat();
        let handler = &source[source
            .find("tauri::generate_handler![")
            .expect("desktop invoke handler should exist")..];

        assert!(handler.contains(&command));
    }

    #[test]
    fn exposes_native_command_classification_from_menu_module() {
        assert!(crate::menu::is_frontend_menu_command("saveDocument"));
        assert!(crate::menu::is_frontend_menu_command("openSettings"));
        assert!(crate::menu::is_native_new_window_command("newDocument"));
    }

    #[test]
    fn bundle_declares_markdown_file_associations() {
        for (platform, source) in [
            ("macOS", include_str!("../tauri.macos.conf.json")),
            ("Windows", include_str!("../tauri.windows.conf.json")),
            ("Linux", include_str!("../tauri.linux.conf.json")),
        ] {
            let config: serde_json::Value = serde_json::from_str(source)
                .unwrap_or_else(|error| panic!("{platform} Tauri config should be valid: {error}"));
            let associations = config
                .pointer("/bundle/fileAssociations")
                .and_then(serde_json::Value::as_array)
                .unwrap_or_else(|| panic!("{platform} bundle should declare file associations"));
            let markdown_association = associations
                .iter()
                .find(|association| {
                    association
                        .pointer("/ext")
                        .and_then(serde_json::Value::as_array)
                        .is_some_and(|extensions| {
                            extensions
                                .iter()
                                .any(|extension| extension.as_str() == Some("md"))
                                && extensions
                                    .iter()
                                    .any(|extension| extension.as_str() == Some("markdown"))
                        })
                })
                .unwrap_or_else(|| {
                    panic!("Markdown extensions should be associated on {platform}")
                });

            assert_eq!(
                markdown_association
                    .pointer("/role")
                    .and_then(serde_json::Value::as_str),
                Some("Editor")
            );
        }
    }

    #[test]
    fn desktop_registers_window_state_restore_plugin() {
        let manifest = include_str!("../Cargo.toml");
        assert!(
            manifest.contains("tauri-plugin-window-state"),
            "desktop manifest should include the window state plugin"
        );

        let lib_source = include_str!("desktop_runtime.rs");
        assert!(
            lib_source.contains("tauri_plugin_window_state::Builder::default()")
                && lib_source.contains(".with_state_flags(window_state_restore_flags())"),
            "Tauri builder should register the window state restore plugin"
        );
    }

    #[test]
    fn desktop_window_state_restore_does_not_auto_show_window() {
        let flags = super::window_state_restore_flags();

        assert!(
            !flags.contains(tauri_plugin_window_state::StateFlags::VISIBLE),
            "window-state should not restore visibility before the frontend startup reveal"
        );
    }

    #[test]
    fn desktop_window_state_restore_does_not_restore_decorations() {
        let flags = super::window_state_restore_flags();

        assert!(
            !flags.contains(tauri_plugin_window_state::StateFlags::DECORATIONS),
            "window-state should not restore old native decorations over the configured window chrome"
        );
    }

    #[test]
    fn desktop_registers_native_startup_window_reveal_fallback() {
        let lib_source = include_str!("desktop_runtime.rs");
        let fallback_registration =
            ["spawn_startup_window", "_reveal_fallback(&app.handle())"].concat();

        assert!(
            lib_source.contains(&fallback_registration),
            "Tauri setup should register a native startup reveal fallback so hidden dev windows cannot stay Dock-only"
        );
    }

    #[test]
    fn cli_opened_directories_are_queued_for_the_primary_notebook_switch() {
        let root = std::env::temp_dir().join(format!(
            "markra-cli-window-fallback-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("test folder should be created");
        let markdown_file = root.join("notes.md");
        std::fs::write(&markdown_file, "# Notes").expect("markdown file should be created");

        let urls = super::editor_window_urls_for_opened_markdown_paths(&[
            root.to_string_lossy().to_string(),
            markdown_file.to_string_lossy().to_string(),
        ]);

        assert_eq!(
            urls,
            vec![crate::windows::editor_window_url_for_path(
                &markdown_file.to_string_lossy()
            )]
        );
        assert!(super::opened_paths_require_primary_notebook_switch(&[
            root.to_string_lossy().to_string(),
            markdown_file.to_string_lossy().to_string(),
        ]));
        assert!(!super::opened_paths_require_primary_notebook_switch(&[
            markdown_file.to_string_lossy().to_string(),
        ]));

        std::fs::remove_dir_all(root).expect("test folder should be removed");
    }

    #[test]
    fn desktop_reveals_initial_cli_opened_paths_natively() {
        let lib_source = include_str!("desktop_runtime.rs");

        assert!(
            lib_source.contains("reveal_or_open_markdown_paths(&app.handle(), paths, false);"),
            "initial CLI-opened paths should trigger a native window reveal instead of only being queued"
        );
    }

    #[test]
    fn macos_open_events_use_the_restore_capable_reveal_route() {
        let lib_source = include_str!("desktop_runtime.rs");
        let run_event_start = lib_source
            .find(".run(|app, event| match event")
            .expect("desktop run-event handler should exist");
        let run_event_source = &lib_source[run_event_start..];
        let macos_open_arm = run_event_source
            .find("#[cfg(target_os = \"macos\")]\n            tauri::RunEvent::Opened { urls } =>")
            .expect("macOS should own a dedicated opened-URL arm");
        let mobile_open_arm = run_event_source
            .find("#[cfg(any(target_os = \"ios\", target_os = \"android\"))]")
            .expect("mobile opened-URL arm should exist");
        let opened_source = &run_event_source[macos_open_arm..mobile_open_arm];

        assert!(opened_source.contains("reveal_or_open_markdown_paths("));
        assert!(opened_source.contains("opened_markdown_paths_from_urls(&urls)"));
        assert!(!opened_source.contains("queue_opened_markdown_paths("));
    }

    #[test]
    fn external_notebook_switch_requests_use_the_durable_reveal_route() {
        let lib_source = include_str!("desktop_runtime.rs");
        let start = lib_source
            .find("pub(crate) fn request_primary_notebook_switch")
            .expect("desktop runtime should expose a primary notebook switch command");
        let end = lib_source[start..]
            .find("fn show_main_window_if_hidden")
            .map(|offset| start + offset)
            .expect("request command should end before the next desktop helper");
        let command_source = &lib_source[start..end];

        assert!(command_source.contains("resolve_markdown_folder(path)?"));
        assert!(
            command_source.contains("reveal_or_open_markdown_paths(&app, vec![folder], false);")
        );
    }

    #[test]
    fn directory_reveal_queues_the_switch_before_spawning_the_primary_window() {
        let lib_source = include_str!("desktop_runtime.rs");
        let branch_start = lib_source
            .find("if opened_paths_require_primary_notebook_switch(&paths) {")
            .expect("directory reveal branch should exist");
        let branch_end = lib_source[branch_start..]
            .find("let urls = editor_window_urls_for_opened_markdown_paths")
            .map(|offset| branch_start + offset)
            .expect("directory reveal branch should end before file-only routing");
        let branch_source = &lib_source[branch_start..branch_end];
        let queue = branch_source
            .find("queue_opened_markdown_paths(app, paths);")
            .expect("directory switch should be durably queued");
        let spawn = branch_source
            .find("spawn_restorable_editor_window(app.clone());")
            .expect("directory switch should reveal a primary window");

        assert!(
            queue < spawn,
            "the primary renderer must not start before its directory switch is queued"
        );
    }

    #[test]
    fn empty_app_reopen_uses_restorable_editor_window() {
        let lib_source = include_str!("desktop_runtime.rs");
        let start = lib_source
            .find("fn reveal_or_open_markdown_paths")
            .expect("reveal_or_open_markdown_paths should exist");
        let end = lib_source[start..]
            .find("fn show_main_window_if_hidden")
            .map(|offset| start + offset)
            .expect("reveal_or_open_markdown_paths should end before show_main_window_if_hidden");
        let reveal_source = &lib_source[start..end];

        assert!(
            reveal_source.contains("spawn_restorable_editor_window(app.clone());"),
            "reopening QingYu without a live main window should create a restore-capable editor window"
        );
        assert!(
            !reveal_source.contains("spawn_blank_editor_window(app.clone());"),
            "empty app reopen should not use index.html?blank=1 because that skips workspace restore"
        );
    }

    #[test]
    fn desktop_handles_macos_reopen_without_visible_windows() {
        let lib_source = include_str!("desktop_runtime.rs");
        let reopen_event = ["tauri::RunEvent::", "Reopen {"].concat();
        let empty_reveal = ["reveal_or_open_markdown_paths(app, Vec::new(), ", "true);"].concat();

        assert!(
            lib_source.contains(&reopen_event),
            "macOS Dock reopen should be handled when all editor windows are closed"
        );
        assert!(
            lib_source.contains("if !has_visible_editor_window(app) {"),
            "reopen handling should only create a window when no editor window is visible"
        );
        assert!(
            lib_source.contains(&empty_reveal),
            "macOS Dock reopen should use the restore-capable empty reveal path"
        );
    }

    #[test]
    fn desktop_reopen_ignores_visible_settings_windows() {
        let lib_source = include_str!("desktop_runtime.rs");
        let generic_visible_window_guard = ["if !has", "_visible_windows {"].concat();

        assert!(
            lib_source.contains("if !has_visible_editor_window(app) {"),
            "macOS Dock reopen should restore an editor when the only visible window is Settings"
        );
        assert!(
            !lib_source.contains(&generic_visible_window_guard),
            "macOS Dock reopen should not treat visible Settings windows as visible editor windows"
        );
    }

    #[test]
    fn desktop_registers_native_about_command() {
        let lib_source = include_str!("desktop_runtime.rs");
        let command_name = ["show", "_native_app", "_about"].concat();
        let registration = format!("{command_name},");
        let handler_source = &lib_source[lib_source
            .find("tauri::generate_handler![")
            .expect("Tauri invoke handler should be registered")..];

        assert!(
            handler_source.contains(&registration),
            "Windows self-drawn app menu should be able to open the system-native About panel"
        );
    }

    #[test]
    fn desktop_registers_application_sync_commands_only() {
        let lib_source = include_str!("desktop_runtime.rs");
        let handler_start = lib_source
            .find("tauri::generate_handler![")
            .expect("Tauri invoke handler should be registered");
        let handler_source = &lib_source[handler_start
            ..lib_source[handler_start..]
                .find("])\n")
                .map(|offset| handler_start + offset)
                .expect("Tauri invoke handler should be closed")];

        for command in ["sync_application", "test_sync_connection"] {
            assert!(
                handler_source.contains(&format!("{command},")),
                "desktop invoke handler should register {command}"
            );
        }
        for forbidden in ["project_config", "sync_project_folder"] {
            assert!(!handler_source.contains(forbidden));
        }
    }

    #[test]
    fn desktop_registers_typed_app_settings_commands() {
        let lib_source = include_str!("desktop_runtime.rs");
        let handler_source = &lib_source[lib_source
            .find("tauri::generate_handler![")
            .expect("Tauri invoke handler should be registered")..];

        for command in [
            "read_app_settings_group",
            "write_app_settings_group",
            "replace_portable_app_settings",
            "read_exposed_app_settings",
            "patch_exposed_app_settings",
        ] {
            assert!(
                handler_source.contains(&format!("{command},")),
                "desktop invoke handler should register {command}"
            );
        }
    }

    #[test]
    fn desktop_registers_single_instance_plugin_before_other_plugins() {
        let manifest = include_str!("../Cargo.toml");
        assert!(
            manifest.contains("tauri-plugin-single-instance"),
            "desktop manifest should include the single instance plugin"
        );

        let lib_source = include_str!("desktop_runtime.rs");
        let single_instance_index = lib_source
            .find("tauri_plugin_single_instance::init")
            .expect("Tauri builder should register the single instance plugin");
        let store_plugin_index = lib_source
            .find("tauri_plugin_store::Builder")
            .expect("Tauri builder should register the store plugin");

        assert!(
            single_instance_index < store_plugin_index,
            "single instance plugin should be registered before other plugins"
        );
    }

    #[test]
    fn desktop_log_files_have_bounded_rotation() {
        let lib_source = include_str!("desktop_runtime.rs");
        let max_size_constant = [
            "const DESKTOP_LOG_MAX",
            "_FILE_SIZE_BYTES: u128 = 2 * 1024 * 1024;",
        ]
        .concat();
        let max_count_constant = ["const DESKTOP_LOG_MAX", "_FILE_COUNT: usize = 5;"].concat();
        let archive_count_constant = [
            "const DESKTOP_LOG_ARCHIVED",
            "_FILE_COUNT: usize = DESKTOP_LOG_MAX_FILE_COUNT - 1;",
        ]
        .concat();
        let max_file_size_call = [".max", "_file_size(DESKTOP_LOG_MAX_FILE_SIZE_BYTES)"].concat();
        let rotation_strategy_type = ["tauri_plugin_log::RotationStrategy::", "KeepSome"].concat();
        let archived_count_name = ["DESKTOP_LOG_ARCHIVED", "_FILE_COUNT"].concat();

        assert_eq!(super::DESKTOP_LOG_MAX_FILE_SIZE_BYTES, 2 * 1024 * 1024);
        assert_eq!(super::DESKTOP_LOG_MAX_FILE_COUNT, 5);
        assert_eq!(super::DESKTOP_LOG_ARCHIVED_FILE_COUNT, 4);
        assert!(
            lib_source.contains(&max_size_constant),
            "desktop file logs should use a conservative 2MB per-file limit"
        );
        assert!(
            lib_source.contains(&max_count_constant),
            "desktop file logs should cap total retained log files"
        );
        assert!(
            lib_source.contains(&archive_count_constant),
            "desktop archived log file count should reserve one slot for the active log file"
        );
        assert!(
            lib_source.contains(&max_file_size_call),
            "desktop log plugin should use the configured file size limit"
        );
        let rotation_strategy_index = lib_source
            .find(&rotation_strategy_type)
            .expect("desktop log plugin should use KeepSome rotation");
        assert!(
            lib_source[rotation_strategy_index..].contains(&archived_count_name),
            "desktop log plugin should keep only the configured number of archived files"
        );
    }
}
