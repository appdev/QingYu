#[cfg(desktop)]
use tauri::menu::MenuItemKind;
use tauri::{Emitter, Manager};

#[cfg(desktop)]
#[derive(Clone)]
pub struct NativeMenuLabels(Vec<(MenuItemKind<tauri::Wry>, String)>);

#[cfg(desktop)]
impl NativeMenuLabels {
    pub fn capture(menu: &tauri::menu::Menu<tauri::Wry>) -> tauri::Result<Self> {
        fn collect(
            items: Vec<MenuItemKind<tauri::Wry>>,
            labels: &mut Vec<(MenuItemKind<tauri::Wry>, String)>,
        ) -> tauri::Result<()> {
            for item in items {
                let label = match &item {
                    MenuItemKind::Submenu(submenu) => {
                        collect(submenu.items()?, labels)?;
                        submenu.text()?
                    }
                    MenuItemKind::MenuItem(item) => item.text()?,
                    MenuItemKind::Predefined(item) => item.text()?,
                    _ => continue,
                };
                if !label.is_empty() {
                    labels.push((item, label));
                }
            }
            Ok(())
        }
        let mut labels = Vec::new();
        collect(menu.items()?, &mut labels)?;
        Ok(Self(labels))
    }

    pub fn apply(&self, labels: &std::collections::HashMap<String, String>) -> tauri::Result<()> {
        for (entry, source) in &self.0 {
            let Some(label) = labels.get(source) else {
                continue;
            };
            if label.is_empty() || label.len() > 512 || label.chars().any(char::is_control) {
                continue;
            }
            let label = label.replace('&', "&&");
            match entry {
                MenuItemKind::Submenu(item) => item.set_text(&label)?,
                MenuItemKind::MenuItem(item) => item.set_text(&label)?,
                MenuItemKind::Predefined(item) => item.set_text(&label)?,
                _ => {}
            }
        }
        Ok(())
    }
}

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
            Some("account") => "account",
            Some("shortcuts") => "shortcuts",
            Some("ai") => "ai",
            Some("today") => "today",
            Some("storage") => "storage",
            Some("help") => "help",
            Some("personal") => "personal",
            Some("signIn") => "signIn",
            Some("appearance") => "appearance",
            Some("sync") => "sync",
            _ => "general",
        };
        let (window, created) = crate::feature_window::FeatureWindow {
            label: "settings",
            title: "设置",
            route: format!("settings.html?section={section}"),
            size: (760., 660.),
            chrome: crate::feature_window::WindowChrome::Fixed,
        }
        .open(&app, |window, created| {
            if created {
                let handle = window.clone();
                window.on_window_event(move |event| {
                    if matches!(
                        event,
                        tauri::WindowEvent::Focused(false)
                            | tauri::WindowEvent::CloseRequested { .. }
                            | tauri::WindowEvent::Destroyed
                    ) {
                        let app = handle.app_handle().clone();
                        tauri::async_runtime::spawn_blocking(move || {
                            crate::activation_shortcut::cancel_recording(&app)
                        });
                    }
                    #[cfg(target_os = "macos")]
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = handle.hide();
                    }
                });
            }
            #[cfg(target_os = "macos")]
            place_settings_on_main_display(window, created)?;
            Ok(())
        })?;
        if !created && select_section {
            window
                .emit("settings-section", section)
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(target_os = "macos")]
fn place_settings_on_main_display(
    window: &tauri::WebviewWindow,
    first_open: bool,
) -> Result<(), String> {
    use objc2_app_kit::NSWindow;
    use objc2_foundation::NSPoint;

    let Some(main) = window.app_handle().get_webview_window("main") else {
        return Ok(());
    };
    let settings = window.clone();
    let (ready, positioned) = std::sync::mpsc::sync_channel(1);
    window
        .run_on_main_thread(move || {
            if let (Ok(main), Ok(settings)) = (main.ns_window(), settings.ns_window()) {
                // AppKit uses logical screen coordinates, including across displays with different scales.
                let (main, settings) =
                    unsafe { (&*main.cast::<NSWindow>(), &*settings.cast::<NSWindow>()) };
                if let Some(screen) = main.screen() {
                    let already_on_display = settings
                        .screen()
                        .is_some_and(|current| current.frame() == screen.frame());
                    if first_open || !already_on_display {
                        let area = screen.visibleFrame();
                        let size = settings.frame().size;
                        settings.setFrameOrigin(NSPoint::new(
                            area.origin.x + (area.size.width - size.width).max(0.) / 2.,
                            area.origin.y + (area.size.height - size.height).max(0.) / 2.,
                        ));
                    }
                }
            }
            let _ = ready.send(());
        })
        .map_err(|e| e.to_string())?;
    positioned.recv().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn apply_preferences(
    app: tauri::AppHandle,
    appearance: String,
    always_on_top: bool,
    note_text_size: Option<u16>,
    menu_bar: Option<bool>,
    menu_labels: Option<std::collections::HashMap<String, String>>,
) -> Result<(), String> {
    let theme = match appearance.as_str() {
        "system" => None,
        "light" => Some(tauri::Theme::Light),
        "dark" => Some(tauri::Theme::Dark),
        _ => return Err("外观设置无效".into()),
    };
    #[cfg(desktop)]
    for window in app.webview_windows().into_values() {
        window.set_theme(theme).map_err(|e| e.to_string())?;
        // 菜单栏面板始终浮在最上面，不跟「窗口置顶」走。
        if window.label() == crate::menu_bar::PANEL {
            continue;
        }
        window
            .set_always_on_top(always_on_top)
            .map_err(|e| e.to_string())?;
    }
    // 托盘不可用时（比如 Linux 桌面没有托盘）不影响其他设置生效。
    #[cfg(desktop)]
    if let Some(on) = menu_bar {
        if let Err(reason) = crate::menu_bar::set_shown(&app, on) {
            eprintln!("菜单栏图标未能更新：{reason}");
        }
    }
    #[cfg(desktop)]
    if let Some(labels) = menu_labels {
        app.state::<NativeMenuLabels>()
            .apply(&labels)
            .map_err(|error| error.to_string())?;
        crate::menu_bar::apply_labels(&app, &labels)?;
        if let (Some(main), Some(name)) = (app.get_webview_window("main"), labels.get("轻羽")) {
            main.set_title(name).map_err(|error| error.to_string())?;
        }
        if let (Some(window), Some(title)) =
            (app.get_webview_window("settings"), labels.get("设置…"))
        {
            window
                .set_title(title.trim_end_matches(['…', '.']))
                .map_err(|error| error.to_string())?;
        }
    }
    #[cfg(mobile)]
    {
        let _ = (app, theme, always_on_top, menu_bar, menu_labels);
    }
    #[cfg(target_os = "macos")]
    if let (Some(size), Some(menu)) = (note_text_size, app.menu()) {
        for entry in menu.items().map_err(|e| e.to_string())? {
            let Some(submenu) = entry.as_submenu() else {
                continue;
            };
            for (id, enabled) in [
                ("note-text-bigger", size < 24),
                ("note-text-smaller", size > 13),
                ("note-text-actual", size != 13),
            ] {
                if let Some(item) = submenu
                    .get(id)
                    .and_then(|entry| entry.as_menuitem().cloned())
                {
                    item.set_enabled(enabled).map_err(|e| e.to_string())?;
                }
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = note_text_size;
    Ok(())
}

#[cfg(target_os = "macos")]
const FORMAT_GROUPS: &[&[(&str, &str, Option<&str>)]] = &[
    &[
        ("format-title", "标题", Some("Cmd+Shift+T")),
        ("format-heading", "二级标题", Some("Cmd+Shift+H")),
        ("format-subheading", "三级标题", Some("Cmd+Shift+J")),
        ("format-body", "正文", Some("Cmd+Shift+B")),
    ],
    &[
        ("format-bold", "粗体", Some("Cmd+B")),
        ("format-italic", "斜体", Some("Cmd+I")),
        ("format-underline", "下划线", Some("Cmd+U")),
        ("format-strike", "删除线", Some("Cmd+Shift+X")),
        ("format-highlight", "高亮", Some("Cmd+Shift+E")),
        ("format-code", "行内等宽", Some("Cmd+Shift+M")),
    ],
    &[
        ("format-task", "任务清单", Some("Cmd+Shift+L")),
        ("format-tick", "标为已完成", Some("Cmd+Shift+U")),
        ("format-untick", "取消勾选全部任务", Some("Cmd+Alt+Shift+L")),
        ("format-bullet", "项目符号列表", Some("Cmd+Shift+7")),
        ("format-list", "短划线列表", Some("Cmd+Shift+8")),
        ("format-ordered", "编号列表", Some("Cmd+Shift+9")),
        ("format-quote", "块引用", Some("Cmd+'")),
        ("format-move-up", "上移项目", Some("Cmd+Ctrl+Up")),
        ("format-move-down", "下移项目", Some("Cmd+Ctrl+Down")),
        ("format-codeblock", "代码块", None),
        ("format-divider", "分隔线", None),
        ("format-callout", "标注", None),
    ],
    &[
        ("format-table", "表格", Some("Cmd+Alt+T")),
        ("format-date", "日期", Some("Cmd+Shift+D")),
        ("format-link", "链接", None),
    ],
    &[
        ("format-typography", "排版优化", Some("Cmd+Alt+Shift+F")),
    ],
];

#[cfg(target_os = "macos")]
#[derive(serde::Deserialize)]
pub struct FormatMenuState {
    editable: bool,
    ticked: Option<bool>,
    untick: bool,
    moves: [bool; 2],
}

#[cfg(target_os = "macos")]
#[tauri::command]
pub fn editor_menu_state(
    window: tauri::WebviewWindow,
    undo: bool,
    redo: bool,
    sections: bool,
    format: FormatMenuState,
    tick_label: Option<String>,
) -> Result<(), String> {
    if !window.is_focused().map_err(|e| e.to_string())? {
        return Ok(());
    }
    let Some(menu) = window.app_handle().menu() else {
        return Ok(());
    };
    if let Some(find) = menu
        .get("editor-edit")
        .and_then(|item| item.as_submenu()?.get("editor-find"))
        .and_then(|item| item.as_submenu().cloned())
    {
        let enabled = window.label() != "settings";
        if find.is_enabled().map_err(|e| e.to_string())? != enabled {
            find.set_enabled(enabled).map_err(|e| e.to_string())?;
        }
    }
    for entry in menu.items().map_err(|e| e.to_string())? {
        let Some(submenu) = entry.as_submenu() else {
            continue;
        };
        for (id, enabled) in [
            ("editor-undo", undo),
            ("editor-redo", redo),
            ("note-sections-collapse", sections),
            ("note-sections-expand", sections),
        ] {
            if let Some(item) = submenu.get(id).and_then(|item| item.as_menuitem().cloned()) {
                if item.is_enabled().map_err(|e| e.to_string())? != enabled {
                    item.set_enabled(enabled).map_err(|e| e.to_string())?;
                }
            }
        }
    }
    if let Some(submenu) = menu
        .get("editor-format")
        .and_then(|item| item.as_submenu().cloned())
    {
        for item in submenu.items().map_err(|e| e.to_string())? {
            let Some(item) = item.as_menuitem() else {
                continue;
            };
            let enabled = format.editable
                && match item.id().as_ref() {
                    "format-tick" => format.ticked.is_some(),
                    "format-untick" => format.untick,
                    "format-move-up" => format.moves[0],
                    "format-move-down" => format.moves[1],
                    _ => true,
                };
            if item.is_enabled().map_err(|e| e.to_string())? != enabled {
                item.set_enabled(enabled).map_err(|e| e.to_string())?;
            }
            if item.id().as_ref() == "format-tick" {
                let title = tick_label
                    .as_deref()
                    .filter(|text| {
                        !text.is_empty() && text.len() <= 512 && !text.chars().any(char::is_control)
                    })
                    .unwrap_or(if format.ticked == Some(true) {
                        "标为未完成"
                    } else {
                        "标为已完成"
                    });
                if item.text().map_err(|e| e.to_string())? != title {
                    item.set_text(title).map_err(|e| e.to_string())?;
                }
            }
        }
    }
    Ok(())
}

#[cfg(desktop)]
pub fn install_menu(app: &tauri::App) -> tauri::Result<()> {
    use tauri::{
        menu::{Menu, MenuItem},
        Emitter,
    };
    let menu = Menu::default(app.handle())?;
    let item = MenuItem::with_id(app, "settings", "设置…", true, Some("CmdOrCtrl+,"))?;
    #[cfg(target_os = "macos")]
    if let Some(submenu) = menu.items()?.first().and_then(|item| item.as_submenu()) {
        submenu.insert(&item, 1)?;
        // Tauri's default app menu ends in native Quit (terminate:), which bypasses async exit work.
        if let Some(quit) = submenu
            .items()?
            .last()
            .and_then(|item| item.as_predefined_menuitem())
        {
            let replacement =
                MenuItem::with_id(app, "library-quit", "退出轻羽", true, Some("CmdOrCtrl+Q"))?;
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
    // AppKit's predefined About and Hide labels can include the app name.
    let app_name_suffix = format!(" {}", app.package_info().name.trim());
    for (_index, entry) in menu.items()?.into_iter().enumerate() {
        let Some(submenu) = entry.as_submenu() else {
            continue;
        };
        #[cfg(target_os = "macos")]
        let replacement = if submenu.text()? == "Edit" {
            let edit = tauri::menu::Submenu::with_id(app, "editor-edit", "Edit", true)?;
            edit.append(&MenuItem::with_id(
                app,
                "editor-undo",
                "撤销",
                false,
                Some("Cmd+Z"),
            )?)?;
            edit.append(&MenuItem::with_id(
                app,
                "editor-redo",
                "重做",
                false,
                Some("Cmd+Shift+Z"),
            )?)?;
            for item in submenu.items()?.into_iter().skip(2) {
                edit.append(&item)?;
            }
            let find = tauri::menu::Submenu::with_id(app, "editor-find", "查找", true)?;
            for (id, label, key) in [
                ("note-find-open", "查找…", "Cmd+F"),
                ("note-find-next", "查找下一个", "Cmd+G"),
                ("note-find-previous", "查找上一个", "Cmd+Shift+G"),
            ] {
                find.append(&MenuItem::with_id(app, id, label, true, Some(key))?)?;
            }
            edit.append(&tauri::menu::PredefinedMenuItem::separator(app)?)?;
            edit.append(&find)?;
            menu.remove(submenu)?;
            menu.insert(&edit, _index)?;
            Some(edit)
        } else {
            None
        };
        #[cfg(target_os = "macos")]
        let submenu = replacement.as_ref().unwrap_or(submenu);
        let title = match submenu.text()?.as_str() {
            "File" => "文件",
            "Edit" => "编辑",
            "View" => "显示",
            "Window" => "窗口",
            "Help" => "帮助",
            _ => "",
        };
        #[cfg(target_os = "macos")]
        if title == "显示" {
            submenu.append(&tauri::menu::PredefinedMenuItem::separator(app)?)?;
            submenu.append(&MenuItem::with_id(
                app,
                "note-sections-collapse",
                "收起所有章节",
                false,
                Some("Cmd+Alt+Shift+Left"),
            )?)?;
            submenu.append(&MenuItem::with_id(
                app,
                "note-sections-expand",
                "展开所有章节",
                false,
                Some("Cmd+Alt+Shift+Right"),
            )?)?;
            submenu.append(&tauri::menu::PredefinedMenuItem::separator(app)?)?;
            for (id, label, key, enabled) in [
                ("note-text-bigger", "放大正文", "CmdOrCtrl+Equal", true),
                ("note-text-smaller", "缩小正文", "CmdOrCtrl+Minus", false),
                ("note-text-actual", "恢复默认字号", "CmdOrCtrl+0", false),
            ] {
                submenu.append(&MenuItem::with_id(app, id, label, enabled, Some(key))?)?;
            }
        }
        if !title.is_empty() {
            submenu.set_text(title)?;
        }
        for entry in submenu.items()? {
            let Some(item) = entry.as_predefined_menuitem() else {
                continue;
            };
            let source = item.text()?.replace('&', "");
            let title = match source.strip_suffix(&app_name_suffix).unwrap_or(&source) {
                "About" => "关于轻羽",
                "Services" => "服务",
                "Hide" => "隐藏轻羽",
                "Hide Others" => "隐藏其他应用",
                "Close" | "Close Window" => "关闭窗口",
                "Exit" | "Quit" => "退出轻羽",
                "Undo" => "撤销",
                "Redo" => "重做",
                "Cut" => "剪切",
                "Copy" => "复制",
                "Paste" => "粘贴",
                "Select All" => "全选",
                "Toggle Full Screen" => "切换全屏",
                "Minimize" => "最小化",
                "Maximize" | "Zoom" => "缩放窗口",
                _ => continue,
            };
            item.set_text(title)?;
        }
    }
    #[cfg(target_os = "macos")]
    {
        let format = tauri::menu::Submenu::with_id(app, "editor-format", "格式", true)?;
        for (index, group) in FORMAT_GROUPS.iter().enumerate() {
            if index > 0 {
                format.append(&tauri::menu::PredefinedMenuItem::separator(app)?)?;
            }
            for &(id, title, shortcut) in *group {
                format.append(&MenuItem::with_id(app, id, title, false, shortcut)?)?;
            }
        }
        let index = menu
            .items()?
            .iter()
            .position(|entry| {
                entry
                    .as_submenu()
                    .and_then(|submenu| submenu.text().ok())
                    .is_some_and(|title| title == "显示")
            })
            .unwrap_or(2);
        menu.insert(&format, index)?;
    }
    app.set_menu(menu)?;
    app.on_menu_event(|app, event| {
        #[cfg(target_os = "macos")]
        if let Some(action @ ("undo" | "redo")) = event.id().as_ref().strip_prefix("editor-") {
            if let Some(window) = app
                .webview_windows()
                .into_values()
                .find(|window| window.is_focused().unwrap_or(false))
            {
                let _ = app.emit_to(window.label(), "editor-history-menu", action);
            }
            return;
        }
        #[cfg(target_os = "macos")]
        if let Some(action) = event.id().as_ref().strip_prefix("note-text-") {
            let _ = app.emit_to("main", "note-text-size", action);
        }
        #[cfg(target_os = "macos")]
        if let Some(action) = event.id().as_ref().strip_prefix("note-sections-") {
            if let Some(window) = app
                .webview_windows()
                .into_values()
                .find(|window| window.is_focused().unwrap_or(false))
            {
                let _ = app.emit_to(window.label(), "note-sections", action);
            }
        }
        #[cfg(target_os = "macos")]
        if let Some(action) = event.id().as_ref().strip_prefix("note-find-") {
            if let Some(window) = app
                .webview_windows()
                .into_values()
                .find(|window| window.is_focused().unwrap_or(false))
            {
                let _ = app.emit_to(window.label(), "note-find", action);
            }
        }
        #[cfg(target_os = "macos")]
        if let Some(action) = event.id().as_ref().strip_prefix("format-") {
            if let Some(window) = app
                .webview_windows()
                .into_values()
                .find(|window| window.is_focused().unwrap_or(false))
            {
                let _ = app.emit_to(window.label(), "note-format", action);
            }
        }
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
