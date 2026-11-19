//! 菜单栏里的轻羽（Windows 是通知区域，Linux 是托盘）：点开一个小面板，打字新建笔记，
//! 或者从置顶和最近的笔记里找一篇打开。对应 Pinto 的 MenuBarExtra。
//!
//! 面板只负责输入和展示，新建、打开都交给主窗口去做，笔记库只有一处在写。
//! Linux 常见桌面的托盘（AppIndicator）只弹菜单、不报告点击位置，所以在托盘菜单里放
//! 「新建或查找笔记…」，从那里打开的面板放在屏幕中间。

use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager};

const TRAY: &str = "menu-bar";
pub const PANEL: &str = "menubar";
const WIDTH: f64 = 320.;
/// 面板第一次打开时的高度；之后由页面按内容调整（menu_bar_fit）。
const HEIGHT: f64 = 360.;

#[derive(Default)]
pub struct MenuBar {
    /// 面板因失焦收起的时刻。点菜单栏图标会先让面板失焦，紧接着才收到点击，
    /// 这次点击是想关掉面板，不能又把它打开。
    hidden_at: Mutex<Option<Instant>>,
    /// 最近一次打开时图标的位置，高度变化后按它重新摆放。
    anchor: Mutex<Option<tauri::Rect>>,
}

pub fn shown(app: &tauri::AppHandle) -> bool {
    app.tray_by_id(TRAY).is_some()
}

/// 按设置显示或去掉菜单栏图标。
pub fn set_shown(app: &tauri::AppHandle, on: bool) -> Result<(), String> {
    if on == shown(app) {
        return Ok(());
    }
    if !on {
        app.remove_tray_by_id(TRAY);
        if let Some(panel) = app.get_webview_window(PANEL) {
            hide_panel(&panel);
        }
        return Ok(());
    }
    let menu = Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "menu-bar-panel", "新建或查找笔记…", true, None::<&str>)
                .map_err(|e| e.to_string())?,
            &MenuItem::with_id(app, "menu-bar-open", "打开轻羽", true, None::<&str>)
                .map_err(|e| e.to_string())?,
            &PredefinedMenuItem::separator(app).map_err(|e| e.to_string())?,
            &MenuItem::with_id(app, "menu-bar-quit", "退出轻羽", true, None::<&str>)
                .map_err(|e| e.to_string())?,
        ],
    )
    .map_err(|e| e.to_string())?;
    let builder = TrayIconBuilder::with_id(TRAY)
        .tooltip("轻羽")
        .menu(&menu)
        // 左键开面板，右键（Linux 上任意点击）出菜单。
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "menu-bar-panel" => {
                let _ = show_panel(app, None);
            }
            "menu-bar-open" => {
                let _ = open_main(app);
            }
            "menu-bar-quit" => quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                rect,
                ..
            } = event
            {
                toggle(tray.app_handle(), rect);
            }
        });
    // macOS 用黑色模板图，系统按菜单栏深浅自动着色；其他平台用应用图标。
    #[cfg(target_os = "macos")]
    let builder = builder
        .icon(
            tauri::image::Image::from_bytes(include_bytes!("../icons/tray/menu-bar-template.png"))
                .map_err(|e| e.to_string())?,
        )
        .icon_as_template(true);
    #[cfg(not(target_os = "macos"))]
    let builder = match app.default_window_icon() {
        Some(icon) => builder.icon(icon.clone()),
        None => builder,
    };
    builder.build(app).map_err(|e| e.to_string())?;
    Ok(())
}

fn toggle(app: &tauri::AppHandle, anchor: tauri::Rect) {
    if let Some(panel) = app.get_webview_window(PANEL) {
        if panel.is_visible().unwrap_or(false) {
            hide_panel(&panel);
            return;
        }
    }
    let just_hidden = app
        .state::<MenuBar>()
        .hidden_at
        .lock()
        .ok()
        .and_then(|at| *at)
        .is_some_and(|at| at.elapsed() < Duration::from_millis(300));
    if !just_hidden {
        let _ = show_panel(app, Some(anchor));
    }
}

fn show_panel(app: &tauri::AppHandle, anchor: Option<tauri::Rect>) -> Result<(), String> {
    let panel = match app.get_webview_window(PANEL) {
        Some(panel) => panel,
        None => build(app)?,
    };
    if let Ok(mut last) = app.state::<MenuBar>().anchor.lock() {
        *last = anchor;
    }
    let height = panel
        .inner_size()
        .map_err(|e| e.to_string())?
        .to_logical::<f64>(panel.scale_factor().map_err(|e| e.to_string())?)
        .height;
    place(&panel, anchor, height)?;
    panel.show().map_err(|e| e.to_string())?;
    panel.set_focus().map_err(|e| e.to_string())?;
    highlight(app, true);
    // 每次打开都让面板重新取一遍笔记，并把光标放回输入框。
    panel
        .emit_to(PANEL, "menu-bar-shown", ())
        .map_err(|e| e.to_string())
}

fn build(app: &tauri::AppHandle) -> Result<tauri::WebviewWindow, String> {
    let builder =
        tauri::WebviewWindowBuilder::new(app, PANEL, tauri::WebviewUrl::App("menubar.html".into()))
            .title("轻羽")
            .inner_size(WIDTH, HEIGHT)
            .resizable(false)
            .maximizable(false)
            .minimizable(false)
            .skip_taskbar(true)
            .always_on_top(true)
            .visible_on_all_workspaces(true)
            .visible(false);
    // macOS 保留系统窗口的圆角和阴影，只藏起标题栏和红绿灯，背景用和 Pinto 一样的 Popover 磨砂材质
    // （网页本身透明，需要 macos-private-api）；其他平台用无边框窗口加系统阴影。
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .transparent(true)
        .effects(
            tauri::window::EffectsBuilder::new()
                .effect(tauri::window::Effect::Popover)
                .state(tauri::window::EffectState::Active)
                .build(),
        );
    #[cfg(not(target_os = "macos"))]
    let builder = builder.decorations(false).shadow(true);
    let panel = builder.build().map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    hide_window_buttons(&panel)?;
    let handle = panel.clone();
    panel.on_window_event(move |event| match event {
        tauri::WindowEvent::Focused(false) => {
            if let Ok(mut at) = handle.app_handle().state::<MenuBar>().hidden_at.lock() {
                *at = Some(Instant::now());
            }
            hide_panel(&handle);
        }
        tauri::WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            hide_panel(&handle);
        }
        _ => {}
    });
    Ok(panel)
}

#[cfg(target_os = "macos")]
fn hide_window_buttons(panel: &tauri::WebviewWindow) -> Result<(), String> {
    use objc2_app_kit::{NSWindow, NSWindowButton};
    let window = panel.clone();
    panel
        .run_on_main_thread(move || {
            let Ok(raw) = window.ns_window() else { return };
            let window = unsafe { &*raw.cast::<NSWindow>() };
            for kind in [
                NSWindowButton::CloseButton,
                NSWindowButton::MiniaturizeButton,
                NSWindowButton::ZoomButton,
            ] {
                if let Some(button) = window.standardWindowButton(kind) {
                    button.setHidden(true);
                }
            }
            // 标题栏区域不能拖走面板。
            window.setMovable(false);
        })
        .map_err(|e| e.to_string())
}

/// 面板放在图标正下方（菜单栏在屏幕上方时）或正上方（任务栏在下方时），不超出可用区域。
/// 没有图标位置（Linux 托盘菜单）时放在屏幕中间。
fn place(
    panel: &tauri::WebviewWindow,
    anchor: Option<tauri::Rect>,
    height: f64,
) -> Result<(), String> {
    let Some(anchor) = anchor else {
        return panel.center().map_err(|e| e.to_string());
    };
    let scale = panel.scale_factor().map_err(|e| e.to_string())?;
    let icon = anchor.position.to_physical::<f64>(scale);
    let icon_size = anchor.size.to_physical::<f64>(scale);
    let monitor = panel
        .monitor_from_point(icon.x, icon.y)
        .ok()
        .flatten()
        .or_else(|| panel.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else {
        return panel.center().map_err(|e| e.to_string());
    };
    let area = monitor.work_area();
    let scale = monitor.scale_factor();
    let (x, y) = panel_origin(
        [icon.x, icon.y, icon_size.width, icon_size.height],
        [
            area.position.x as f64,
            area.position.y as f64,
            area.size.width as f64,
            area.size.height as f64,
        ],
        [WIDTH * scale, height * scale],
        6. * scale,
    );
    panel
        .set_position(tauri::PhysicalPosition::new(
            x.round() as i32,
            y.round() as i32,
        ))
        .map_err(|e| e.to_string())
}

/// 面板左上角的位置（都是物理像素，矩形是 [x, y, 宽, 高]）：水平对着图标中间，
/// 图标在屏幕上半部分就放在它下面，否则放在它上面，并留出 gap 不贴边。
fn panel_origin(icon: [f64; 4], area: [f64; 4], size: [f64; 2], gap: f64) -> (f64, f64) {
    let [icon_x, icon_y, icon_width, icon_height] = icon;
    let [left, top, area_width, area_height] = area;
    let [width, height] = size;
    let (right, bottom) = (left + area_width, top + area_height);
    let below = icon_y + icon_height / 2. < (top + bottom) / 2.;
    let x = (icon_x + icon_width / 2. - width / 2.)
        .clamp(left + gap, (right - width - gap).max(left + gap));
    let y = if below {
        icon_y + icon_height + gap
    } else {
        icon_y - height - gap
    };
    (
        x,
        y.clamp(top + gap, (bottom - height - gap).max(top + gap)),
    )
}

fn hide_panel(panel: &tauri::WebviewWindow) {
    let _ = panel.hide();
    highlight(panel.app_handle(), false);
}

/// 面板开着时菜单栏图标保持按下的样子，和系统菜单一样。托盘库只在鼠标按住时高亮，这里接着管。
fn highlight(app: &tauri::AppHandle, on: bool) {
    #[cfg(target_os = "macos")]
    if let Some(tray) = app.tray_by_id(TRAY) {
        let _ = tray.with_inner_tray_icon(move |inner| {
            if let (Some(item), Some(mtm)) =
                (inner.ns_status_item(), objc2::MainThreadMarker::new())
            {
                if let Some(button) = item.button(mtm) {
                    button.highlight(on);
                }
            }
        });
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, on);
}

fn open_main(app: &tauri::AppHandle) -> Result<(), String> {
    if let Some(panel) = app.get_webview_window(PANEL) {
        hide_panel(&panel);
    }
    crate::file_library::workspace::show_library(app)
}

/// 和主窗口的退出走同一条路：主窗口先保存，再真正退出。
fn quit(app: &tauri::AppHandle) {
    if let Some(panel) = app.get_webview_window(PANEL) {
        hide_panel(&panel);
    }
    let _ = app.emit_to("main", "library-before-exit", ());
}

fn panel_only(window: &tauri::WebviewWindow) -> Result<(), String> {
    if window.label() == PANEL {
        Ok(())
    } else {
        Err("只能从菜单栏面板调用".into())
    }
}

#[tauri::command]
pub fn menu_bar_hide(window: tauri::WebviewWindow) -> Result<(), String> {
    panel_only(&window)?;
    hide_panel(&window);
    Ok(())
}

/// 页面按内容告诉面板该多高，和 Pinto 一样不留空白；图标在下方（任务栏）时顶边跟着上移。
#[tauri::command]
pub fn menu_bar_fit(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    height: f64,
) -> Result<(), String> {
    panel_only(&window)?;
    if !height.is_finite() {
        return Err("面板高度无效".into());
    }
    let height = height.clamp(160., 560.).round();
    window
        .set_size(tauri::LogicalSize::new(WIDTH, height))
        .map_err(|e| e.to_string())?;
    let anchor = app
        .state::<MenuBar>()
        .anchor
        .lock()
        .ok()
        .and_then(|anchor| *anchor);
    // 屏幕中间打开的面板（Linux）保持原来的顶边，只往下伸缩。
    if anchor.is_some() {
        place(&window, anchor, height)?;
    }
    Ok(())
}

#[tauri::command]
pub fn menu_bar_open_main(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> Result<(), String> {
    panel_only(&window)?;
    open_main(&app)
}

#[tauri::command]
pub fn menu_bar_quit(app: tauri::AppHandle, window: tauri::WebviewWindow) -> Result<(), String> {
    panel_only(&window)?;
    quit(&app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::panel_origin;

    #[test]
    fn opens_under_a_menu_bar_icon_and_above_a_taskbar_icon() {
        // macOS：菜单栏在顶部，可用区域从菜单栏下面开始；面板对着图标中间，挂在图标下方。
        let mac = panel_origin(
            [2000., 0., 44., 48.],
            [0., 48., 3024., 1916.],
            [640., 720.],
            12.,
        );
        assert_eq!(mac, (1702., 60.));
        // Windows：任务栏在底部，面板出现在图标上方。
        let windows = panel_origin(
            [1700., 1040., 24., 40.],
            [0., 0., 1920., 1040.],
            [320., 360.],
            6.,
        );
        assert_eq!(windows, (1552., 674.));
    }

    #[test]
    fn stays_inside_the_screen_near_its_edges() {
        // 图标靠右边缘：面板往左收，离边缘留 gap。
        assert_eq!(
            panel_origin(
                [1900., 0., 20., 24.],
                [0., 24., 1920., 1056.],
                [320., 360.],
                6.
            )
            .0,
            1594.
        );
        // 任务栏在左侧、图标贴近顶端：不会跑出屏幕上沿。
        assert_eq!(
            panel_origin(
                [10., 0., 24., 24.],
                [0., 0., 1920., 1080.],
                [320., 360.],
                6.
            ),
            (6., 30.)
        );
    }
}
