#[cfg(any(feature = "app", feature = "server"))]
mod link_preview;

#[cfg(all(feature = "app", target_os = "android"))]
#[no_mangle]
pub extern "system" fn Java_app_miaoyan_editor_prototype_MainActivity_initTlsVerifier<'local>(
    mut env: jni::EnvUnowned<'local>,
    _activity: jni::objects::JObject<'local>,
    context: jni::objects::JObject<'local>,
) {
    env.with_env(|env| rustls_platform_verifier::android::init_with_env(env, context))
        .resolve::<jni::errors::ThrowRuntimeExAndDefault>();
}

#[cfg(feature = "app")]
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    use tauri::{Emitter, Manager};
    let builder = tauri::Builder::default();
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
        file_library::workspace::request(
            app,
            args.into_iter()
                .skip(1)
                .filter(|arg| !arg.starts_with('-'))
                .map(|arg| std::path::Path::new(&cwd).join(arg)),
        );
    }));
    let builder = builder
        .register_uri_scheme_protocol("qingyu-asset", |context, request| {
            file_library::attachments::response(
                context.app_handle(),
                context.webview_label(),
                request,
            )
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_opener::Builder::new()
                .open_js_links_on_click(false)
                .build(),
        )
        .plugin(tauri_plugin_calendar::init())
        .plugin(tauri_plugin_attachments::init())
        .manage(sync_http::SyncHttp::default())
        .manage(client_account::ClientAccount::default())
        .manage(file_library::FileLibrary::default())
        .manage(file_library::assets::AssetStage::default())
        .manage(file_library::workspace::Workspace::default())
        .manage(ExitPermit(std::sync::atomic::AtomicBool::new(false)));
    #[cfg(target_os = "ios")]
    let builder = builder.plugin(tauri_plugin_editor_menu::init());
    #[cfg(target_os = "android")]
    let builder = builder.plugin(sync_http::local_network_plugin());
    #[cfg(desktop)]
    let builder = builder
        .manage(activation_shortcut::ActivationShortcut::default())
        .manage(menu_bar::MenuBar::default())
        .manage(file_library::api::desktop::LocalMcp::default());
    builder
        .setup(|app| {
            #[cfg(desktop)]
            {
                let _ = app.handle().plugin(activation_shortcut::plugin());
                activation_shortcut::restore(app);
            }
            note_ui::install_menu_icons();
            #[cfg(desktop)]
            settings::install_menu(app)?;
            #[cfg(desktop)]
            file_library::install_menu(app)?;
            // 主窗口在配置里是隐藏的，页面排好版后再显示；手机上没有这一步，直接显示。
            #[cfg(desktop)]
            file_library::workspace::show_when_ready(app.handle(), "main");
            #[cfg(mobile)]
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
            }
            // 没有带文件启动时不走 request：它会立即显示主窗口，抢在页面排好版之前。
            #[cfg(desktop)]
            {
                let paths: Vec<_> = std::env::args_os()
                    .skip(1)
                    .filter(|arg| !arg.to_string_lossy().starts_with('-'))
                    .map(std::path::PathBuf::from)
                    .collect();
                if !paths.is_empty() {
                    file_library::workspace::request(app.handle(), paths);
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            client_account::account_status,
            client_account::account_login,
            client_account::account_logout,
            client_account::account_redeem,
            client_account::account_redemptions,
            client_account::account_sync_config,
            client_account::account_check_sync,
            client_account::account_open_portal,
            #[cfg(target_os = "android")]
            sync_http::sync_local_network_access,
            sync_http::sync_set_target,
            sync_http::sync_request,
            sync_profile::sync_load_profile,
            sync_profile::sync_save_profile,
            note_picture::note_picture_action,
            feature_window::open_feature_window,
            note_picture::note_share_text,
            create_calendar_event,
            note_ui::show_note_info,
            note_ui::close_note_info,
            note_ui::link_applications,
            note_ui::open_external_link,
            link_preview::fetch_link_preview,
            #[cfg(target_os = "macos")]
            note_ui::table_clipboard_has_text,
            #[cfg(target_os = "macos")]
            note_ui::table_clipboard,
            #[cfg(target_os = "ios")]
            note_ui::show_table_handle_menu,
            file_library::library_open,
            #[cfg(desktop)]
            file_library::api::desktop::local_mcp,
            file_library::workspace::workspace_requests,
            file_library::workspace::workspace_open,
            file_library::workspace::workspace_open_note,
            file_library::workspace::workspace_window_ready,
            file_library::workspace::workspace_note_snapshot,
            file_library::workspace::workspace_snapshot,
            file_library::workspace::workspace_paths,
            file_library::workspace::workspace_activate_path,
            file_library::workspace::workspace_select,
            file_library::workspace::workspace_commit,
            file_library::workspace::workspace_reconcile,
            file_library::workspace::workspace_finish_close,
            file_library::workspace::workspace_show_library,
            file_library::workspace::workspace_ready,
            file_library::workspace::workspace_prepare_exit,
            file_library::workspace::workspace_exit_reply,
            file_library::workspace::workspace_cancel_exit,
            file_library::workspace::library_hide_main,
            file_library::library_commit,
            file_library::library_save_note,
            #[cfg(desktop)]
            file_library::library_reveal_note,
            file_library::history::library_note_versions,
            file_library::history::library_note_version,
            file_library::history::library_restore_note_version,
            file_library::assets::library_sync_assets,
            file_library::assets::library_asset_placeholders,
            file_library::assets::library_materialize_asset,
            file_library::assets::library_delete_asset_placeholder,
            file_library::assets::library_read_asset,
            file_library::assets::library_stage_asset,
            file_library::assets::library_read_staged_asset,
            file_library::assets::library_release_asset_stage,
            file_library::storage_usage::library_storage_usage,
            file_library::attachments::attachment_import,
            file_library::attachments::attachment_open,
            file_library::attachments::note_picture_asset,
            file_library::library_import,
            file_library::library_choose,
            file_library::library_reload,
            file_library::external::library_watch,
            file_library::external::library_reconcile,
            file_library::folders::library_folder_action,
            settings::open_settings,
            settings::apply_preferences,
            #[cfg(target_os = "macos")]
            settings::editor_menu_state,
            library_finish_exit,
            activation_shortcut_command,
            #[cfg(desktop)]
            menu_bar::menu_bar_hide,
            #[cfg(desktop)]
            menu_bar::menu_bar_fit,
            #[cfg(desktop)]
            menu_bar::menu_bar_open_main,
            #[cfg(desktop)]
            menu_bar::menu_bar_quit
        ])
        .on_window_event(|window, event| {
            #[cfg(desktop)]
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    // 还有文档窗口、或菜单栏里有轻羽时，关掉主窗口只是把它藏起来，不退出。
                    let keep = file_library::workspace::has_documents(window.app_handle())
                        || menu_bar::shown(window.app_handle());
                    let event = if keep {
                        "library-close-main"
                    } else {
                        "library-before-exit"
                    };
                    let _ = window.emit_to(window.label(), event, ());
                }
            } else if file_library::workspace::is_document(window.app_handle(), window.label()) {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.emit_to(window.label(), "workspace-before-close", ());
                } else if matches!(event, tauri::WindowEvent::Destroyed) {
                    file_library::workspace::destroyed(window.app_handle(), window.label());
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("failed to run the Markdown editor")
        .run(|app, event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Opened { ref urls } = event {
                file_library::workspace::request(
                    app,
                    urls.iter().filter_map(|url| url.to_file_path().ok()),
                );
            }
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen {
                has_visible_windows: false,
                ..
            } = event
            {
                let _ = file_library::workspace::show_library(app);
            }
            #[cfg(desktop)]
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                use tauri::Manager;
                if !app
                    .state::<ExitPermit>()
                    .0
                    .load(std::sync::atomic::Ordering::Acquire)
                {
                    api.prevent_exit();
                    let _ = app.emit_to("main", "library-before-exit", ());
                }
            }
        });
}
mod file_library;
#[cfg(any(feature = "server", all(feature = "app", desktop)))]
pub use file_library::api;
#[cfg(feature = "app")]
mod client_account;
#[cfg(feature = "app")]
mod feature_window;
#[cfg(feature = "app")]
mod note_picture;
#[cfg(feature = "app")]
mod note_ui;
#[cfg(feature = "app")]
mod settings;
#[cfg(feature = "app")]
mod sync_http;
#[cfg(feature = "app")]
mod sync_profile;

#[cfg(feature = "app")]
#[tauri::command]
async fn create_calendar_event(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    event: tauri_plugin_calendar::CalendarEvent,
) -> Result<String, String> {
    if window.label() != "main" && !file_library::workspace::is_document(&app, window.label()) {
        return Err("请在编辑器中创建日程".into());
    }
    tauri_plugin_calendar::create(app, event).await
}
#[cfg(all(feature = "app", desktop))]
mod activation_shortcut;
#[cfg(all(feature = "app", desktop))]
mod menu_bar;

#[cfg(feature = "app")]
#[tauri::command]
async fn activation_shortcut_command(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    action: String,
    shortcut: Option<String>,
) -> Result<serde_json::Value, String> {
    if !["main", "settings"].contains(&window.label()) {
        return Err("无法在此窗口设置唤起快捷键".into());
    }
    #[cfg(desktop)]
    {
        serde_json::to_value(activation_shortcut::activation_shortcut(app, action, shortcut).await?)
            .map_err(|_| "无法读取唤起快捷键".into())
    }
    #[cfg(mobile)]
    {
        let _ = (app, action, shortcut);
        Err("移动端不支持系统全局快捷键".into())
    }
}

#[cfg(feature = "app")]
#[tauri::command]
fn library_finish_exit(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    permit: tauri::State<'_, ExitPermit>,
) -> Result<(), String> {
    if window.label() != "main" || !file_library::workspace::exit_allowed(&app) {
        return Err("请先保存所有文档窗口再退出".into());
    }
    permit.0.store(true, std::sync::atomic::Ordering::Release);
    app.exit(0);
    Ok(())
}
#[cfg(feature = "app")]
struct ExitPermit(std::sync::atomic::AtomicBool);
