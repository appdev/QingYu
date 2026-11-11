#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    use tauri::Emitter;
    let builder = tauri::Builder::default();
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
        file_library::workspace::request(app, args.into_iter().skip(1).filter(|arg| !arg.starts_with('-')).map(|arg| std::path::Path::new(&cwd).join(arg)));
    }));
    let builder = builder
        .plugin(tauri_plugin_dialog::init())
        .manage(sync_http::SyncHttp::default())
        .manage(file_library::FileLibrary::default())
        .manage(file_library::workspace::Workspace::default())
        .manage(ExitPermit(std::sync::atomic::AtomicBool::new(false)));
    #[cfg(desktop)]
    let builder = builder.manage(activation_shortcut::ActivationShortcut::default());
    builder.setup(|app| {
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
            #[cfg(desktop)]
            file_library::workspace::request(app.handle(), std::env::args_os().skip(1).filter(|arg| !arg.to_string_lossy().starts_with('-')).map(std::path::PathBuf::from));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            sync_http::sync_set_target,
            sync_http::sync_request,
            sync_profile::sync_load_profile,
            sync_profile::sync_save_profile,
            export_markdown,
            note_ui::show_note_info,
            note_ui::close_note_info,
            note_ui::prompt_folder_name,
            file_library::library_open,
            file_library::workspace::workspace_requests,
            file_library::workspace::workspace_open,
            file_library::workspace::workspace_commit,
            file_library::workspace::workspace_reconcile,
            file_library::workspace::workspace_close,
            file_library::library_commit,
            file_library::assets::library_sync_assets,
            file_library::library_import,
            file_library::library_choose,
            file_library::library_reload,
            file_library::external::library_watch,
            file_library::external::library_reconcile,
            file_library::folders::library_folder_action,
            settings::open_settings,
            settings::apply_preferences,
            settings::system_fonts,
            library_finish_exit,
            activation_shortcut_command
        ])
        .on_window_event(|window, event| {
            #[cfg(desktop)]
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.emit("library-before-exit", ());
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("failed to run the Markdown editor")
        .run(|app, event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Opened { ref urls } = event {
                file_library::workspace::request(app, urls.iter().filter_map(|url| url.to_file_path().ok()));
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
mod note_ui;
mod settings;
mod sync_http;
mod sync_profile;
#[cfg(desktop)]
mod activation_shortcut;

#[tauri::command]
async fn activation_shortcut_command(app: tauri::AppHandle, window: tauri::WebviewWindow, action: String, shortcut: Option<String>) -> Result<serde_json::Value, String> {
    if !["main", "settings"].contains(&window.label()) { return Err("无法在此窗口设置唤起快捷键".into()); }
    #[cfg(desktop)]
    { serde_json::to_value(activation_shortcut::activation_shortcut(app, action, shortcut).await?).map_err(|_| "无法读取唤起快捷键".into()) }
    #[cfg(mobile)]
    { let _ = (app, action, shortcut); Err("移动端不支持系统全局快捷键".into()) }
}

#[tauri::command]
fn library_finish_exit(app: tauri::AppHandle, permit: tauri::State<'_, ExitPermit>) {
    permit.0.store(true, std::sync::atomic::Ordering::Release);
    app.exit(0);
}
struct ExitPermit(std::sync::atomic::AtomicBool);

#[tauri::command]
async fn export_markdown(
    window: tauri::WebviewWindow,
    filename: String,
    source: String,
) -> Result<bool, String> {
    #[cfg(desktop)]
    use tauri_plugin_dialog::DialogExt;
    if filename.is_empty()
        || filename
            .chars()
            .any(|c| c.is_control() || c == '/' || c == '\\')
    {
        return Err("导出文件名称无效".into());
    }
    #[cfg(mobile)]
    {
        let _ = (window, source);
        return Err("请在桌面端导出 Markdown".into());
    }
    #[cfg(desktop)]
    tauri::async_runtime::spawn_blocking(move || {
        let Some(path) = window
            .dialog()
            .file()
            .set_parent(&window)
            .set_file_name(filename)
            .add_filter("Markdown", &["md"])
            .blocking_save_file()
        else {
            return Ok(false);
        };
        let path = path.into_path().map_err(|_| "请选择本地文件位置")?;
        std::fs::write(path, source.as_bytes())
            .map_err(|_| "导出失败，请检查文件权限和剩余空间")?;
        Ok(true)
    })
    .await
    .map_err(|_| "导出未能完成".to_string())?
}
