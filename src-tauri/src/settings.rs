use tauri::{Emitter, Manager};

#[tauri::command]
pub async fn open_settings(app: tauri::AppHandle, section: Option<String>) -> Result<(), String> {
    #[cfg(mobile)]
    {
        let _ = (app, section);
        return Err("请使用应用内设置".into());
    }
    #[cfg(desktop)]
    tauri::async_runtime::spawn_blocking(move || {
        // Menu accelerators and the toolbar can arrive together.
        static CREATION: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = CREATION.lock().map_err(|_| "无法打开设置")?;
        let select_section = section.is_some();
        let section = match section.as_deref() {
            Some("editor") => "editor",
            Some("typography") => "typography",
            Some("sync") => "sync",
            _ => "general",
        };
        if let Some(window) = app.get_webview_window("settings") {
            window.show().map_err(|e| e.to_string())?;
            window.set_focus().map_err(|e| e.to_string())?;
            if select_section {
                window
                    .emit("settings-section", section)
                    .map_err(|e| e.to_string())?;
            }
            return Ok(());
        }
        let builder = tauri::WebviewWindowBuilder::new(
            &app,
            "settings",
            tauri::WebviewUrl::App(format!("settings.html?section={section}").into()),
        )
        .title("设置")
        .inner_size(568., 560.)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .center();
        #[cfg(target_os = "macos")]
        let builder = builder
            .title_bar_style(tauri::TitleBarStyle::Overlay)
            .hidden_title(true)
            .traffic_light_position(tauri::LogicalPosition::new(16., 20.));
        let window = builder.build().map_err(|e| e.to_string())?;
        {
            let handle = window.clone();
            window.on_window_event(move |event| {
                if matches!(event, tauri::WindowEvent::Focused(false) | tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed) {
                    let app = handle.app_handle().clone();
                    tauri::async_runtime::spawn_blocking(move || crate::activation_shortcut::cancel_recording(&app));
                }
                #[cfg(target_os = "macos")]
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = handle.hide();
                }
            });
        }
        window.set_focus().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn apply_preferences(
    app: tauri::AppHandle,
    appearance: String,
    always_on_top: bool,
) -> Result<(), String> {
    let theme = match appearance.as_str() {
        "system" => None,
        "light" => Some(tauri::Theme::Light),
        "dark" => Some(tauri::Theme::Dark),
        _ => return Err("外观设置无效".into()),
    };
    #[cfg(desktop)]
    for name in ["main", "settings"] {
        if let Some(window) = app.get_webview_window(name) {
            window.set_theme(theme).map_err(|e| e.to_string())?;
            window
                .set_always_on_top(always_on_top)
                .map_err(|e| e.to_string())?;
        }
    }
    #[cfg(mobile)]
    {
        let _ = (app, theme, always_on_top);
    }
    Ok(())
}

#[tauri::command]
pub async fn system_fonts() -> Result<Vec<String>, String> {
    #[cfg(mobile)]
    return Ok(vec![]);
    #[cfg(desktop)]
    tauri::async_runtime::spawn_blocking(|| {
        let mut database = fontdb::Database::new();
        database.load_system_fonts();
        let mut names: Vec<_> = database
            .faces()
            .flat_map(|face| face.families.iter().map(|family| family.0.clone()))
            .collect();
        names.sort();
        names.dedup();
        names
    })
    .await
    .map_err(|_| "无法读取系统字体".into())
}

#[cfg(desktop)]
pub fn install_menu(app: &tauri::App) -> tauri::Result<()> {
    use tauri::{menu::{Menu, MenuItem}, Emitter};
    let menu = Menu::default(app.handle())?;
    let item = MenuItem::with_id(app, "settings", "设置…", true, Some("CmdOrCtrl+,"))?;
    #[cfg(target_os = "macos")]
    if let Some(submenu) = menu.items()?.first().and_then(|item| item.as_submenu()) {
        submenu.insert(&item, 1)?;
        // Tauri's default app menu ends in native Quit (terminate:), which bypasses async exit work.
        if let Some(quit) = submenu.items()?.last().and_then(|item| item.as_predefined_menuitem()) {
            let replacement = MenuItem::with_id(app, "library-quit", quit.text()?, true, Some("CmdOrCtrl+Q"))?;
            submenu.remove(quit)?;
            submenu.append(&replacement)?;
        }
    }
    #[cfg(not(target_os = "macos"))]
    for entry in menu.items()? {
        if let Some(submenu) = entry.as_submenu() {
            if submenu.text()? == "Edit" {
                submenu.append(&item)?;
                break;
            }
        }
    }
    app.set_menu(menu)?;
    app.on_menu_event(|app, event| {
        if event.id().as_ref() == "library-quit" {
            let _ = app.emit_to("main", "library-before-exit", ());
        }
        if event.id().as_ref() == "settings" {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let _ = open_settings(app, None).await;
            });
        }
    });
    Ok(())
}
