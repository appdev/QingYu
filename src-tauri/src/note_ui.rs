use serde::Deserialize;
use tauri::Manager;

#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn show_table_handle_menu(
    window: tauri::WebviewWindow,
    menu: serde_json::Value,
) -> Result<Option<usize>, String> {
    if window.label() != "main" {
        return Err("请在编辑器中打开表格菜单".into());
    }
    tauri_plugin_editor_menu::show(window.app_handle(), menu).await
}

#[cfg(target_os = "macos")]
#[tauri::command]
pub fn table_clipboard_has_text(window: tauri::WebviewWindow) -> Result<bool, String> {
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
    use objc2_foundation::NSArray;
    check_editor_window(&window)?;
    let types = NSArray::from_slice(&[unsafe { NSPasteboardTypeString }]);
    Ok(NSPasteboard::generalPasteboard().canReadItemWithDataConformingToTypes(&types))
}

#[cfg(target_os = "macos")]
#[tauri::command]
pub fn table_clipboard(
    window: tauri::WebviewWindow,
    text: Option<String>,
) -> Result<Option<String>, String> {
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
    use objc2_foundation::NSString;
    check_editor_window(&window)?;
    if !window.is_focused().map_err(|error| error.to_string())? {
        return Err("请在当前编辑窗口操作剪贴板".into());
    }
    let board = NSPasteboard::generalPasteboard();
    let text_type = unsafe { NSPasteboardTypeString };
    if let Some(text) = text {
        board.clearContents();
        return board
            .setString_forType(&NSString::from_str(&text), text_type)
            .then_some(None)
            .ok_or_else(|| "无法复制到剪贴板".into());
    }
    Ok(board
        .stringForType(text_type)
        .map(|value| value.to_string()))
}

#[cfg(any(target_os = "macos", test))]
fn palette_color(dark: bool, role: &str) -> Result<[f64; 4], String> {
    let palette: serde_json::Value =
        serde_json::from_str(include_str!("../../src/palette.json")).map_err(|_| "颜色配置无效")?;
    let hex = palette[if dark { "dark" } else { "light" }][role]
        .as_str()
        .and_then(|value| value.strip_prefix('#'))
        .filter(|value| matches!(value.len(), 6 | 8))
        .ok_or_else(|| format!("颜色配置无效：{role}"))?;
    let value = u32::from_str_radix(hex, 16).map_err(|_| format!("颜色配置无效：{role}"))?;
    let rgba = if hex.len() == 6 {
        (value << 8) | 255
    } else {
        value
    };
    Ok([24, 16, 8, 0].map(|shift| f64::from((rgba >> shift) & 255) / 255.))
}

#[cfg(test)]
mod palette_tests {
    use super::palette_color;

    #[test]
    fn native_colors_use_the_shared_palette_in_both_appearances() {
        assert_eq!(
            palette_color(false, "muted").unwrap(),
            [106. / 255., 106. / 255., 106. / 255., 1.]
        );
        assert_eq!(palette_color(true, "paper").unwrap(), [0., 0., 0., 1.]);
        for dark in [false, true] {
            for role in ["paper", "ink", "muted", "line"] {
                assert!(palette_color(dark, role).is_ok());
            }
            assert!(palette_color(dark, "line").unwrap()[3] < 1.);
        }
        assert!(palette_color(false, "missing").is_err());
    }
}

#[derive(Deserialize)]
pub struct NoteInfo {
    words: usize,
    modified: String,
    created: String,
    backlinks: Vec<String>,
}

#[derive(Deserialize)]
pub struct Anchor {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[tauri::command]
pub fn link_applications(
    window: tauri::WebviewWindow,
    target: String,
) -> Result<Vec<(String, String, bool)>, String> {
    check_editor_window(&window)?;
    #[cfg(target_os = "macos")]
    return mac::link_applications(&target);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = target;
        Ok(Vec::new())
    }
}

#[tauri::command]
pub async fn open_external_link(
    window: tauri::WebviewWindow,
    target: String,
    application: Option<String>,
) -> Result<(), String> {
    check_editor_window(&window)?;
    #[cfg(target_os = "macos")]
    return mac::open_external_link(&target, application.as_deref()).await;
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (target, application);
        Err("当前平台使用默认链接处理程序".into())
    }
}

fn check_editor_window(window: &tauri::WebviewWindow) -> Result<(), String> {
    if window.label() == "main"
        || crate::file_library::workspace::is_document(window.app_handle(), window.label())
    {
        Ok(())
    } else {
        Err("请在编辑器中完成此操作".into())
    }
}

#[cfg(target_os = "macos")]
pub(crate) use folder_dialog::{
    confirm_trash as confirm_folder_trash, destination as choose_folder_destination,
    sheet as show_native_sheet,
};

#[cfg(target_os = "macos")]
mod folder_dialog {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{
        NSAlert, NSAlertFirstButtonReturn, NSModalResponseOK, NSOpenPanel, NSWindow,
    };
    use objc2_foundation::{NSString, NSURL};
    use std::sync::mpsc::SyncSender;
    type Reply<T> = SyncSender<Result<T, String>>;

    pub(crate) async fn sheet<T: Send + 'static>(
        window: tauri::WebviewWindow,
        show: impl FnOnce(MainThreadMarker, &NSWindow, Reply<T>) + Send + 'static,
    ) -> Result<T, String> {
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        let parent = window.clone();
        window
            .run_on_main_thread(move || {
                let Some(main) = MainThreadMarker::new() else {
                    let _ = send.send(Err("请在主线程打开窗口".into()));
                    return;
                };
                let raw = match parent.ns_window() {
                    Ok(raw) => raw,
                    Err(reason) => {
                        let _ = send.send(Err(reason.to_string()));
                        return;
                    }
                };
                show(main, unsafe { &*raw.cast::<NSWindow>() }, send);
            })
            .map_err(|reason| reason.to_string())?;
        tauri::async_runtime::spawn_blocking(move || {
            receive.recv().map_err(|_| "窗口已关闭".to_string())?
        })
        .await
        .map_err(|reason| reason.to_string())?
    }

    pub async fn confirm_trash(window: tauri::WebviewWindow, name: String) -> Result<bool, String> {
        sheet(window, move |main, parent, send| {
            let alert = NSAlert::new(main);
            alert.setMessageText(&NSString::from_str(&format!(
                "将文件夹“{name}”及其中的所有内容移到废纸篓？"
            )));
            alert.addButtonWithTitle(&NSString::from_str("移到废纸篓"));
            alert.addButtonWithTitle(&NSString::from_str("取消"));
            let completion = block2::RcBlock::new(move |response| {
                let _ = send.send(Ok(response == NSAlertFirstButtonReturn));
            });
            alert.beginSheetModalForWindow_completionHandler(parent, Some(&completion));
        })
        .await
    }

    pub async fn destination(
        window: tauri::WebviewWindow,
        root: String,
    ) -> Result<Option<String>, String> {
        sheet(window, move |main, parent, send| {
            let panel = NSOpenPanel::openPanel(main);
            panel.setCanChooseDirectories(true);
            panel.setCanChooseFiles(false);
            panel.setAllowsMultipleSelection(false);
            panel.setPrompt(Some(&NSString::from_str("移动")));
            panel.setDirectoryURL(Some(&NSURL::fileURLWithPath_isDirectory(
                &NSString::from_str(&root),
                true,
            )));
            let selected = panel.clone();
            let completion = block2::RcBlock::new(move |response| {
                let path = if response == NSModalResponseOK {
                    selected
                        .URL()
                        .and_then(|url| url.path())
                        .map(|path| path.to_string())
                } else {
                    None
                };
                let _ = send.send(Ok(path));
            });
            panel.beginSheetModalForWindow_completionHandler(parent, &completion);
        })
        .await
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use block2::RcBlock;
    use objc2::{rc::Retained, runtime::ProtocolObject, MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::*;
    use objc2_foundation::{
        NSArray, NSError, NSFileManager, NSNotification, NSNotificationCenter, NSObjectProtocol,
        NSPoint, NSProcessInfo, NSRect, NSRectEdge, NSSize, NSString, NSURL,
    };
    use std::{cell::RefCell, ptr::NonNull};

    thread_local! {
        static MENU_OBSERVER: RefCell<Option<Retained<ProtocolObject<dyn NSObjectProtocol>>>> = const { RefCell::new(None) };
        static INFO: RefCell<Option<Retained<NSPopover>>> = const { RefCell::new(None) };
    }

    fn external_url(target: &str) -> Result<Retained<NSURL>, String> {
        let url = tauri::Url::parse(target).map_err(|_| "链接格式无效")?;
        if !matches!(url.scheme(), "http" | "https" | "mailto" | "tel") {
            return Err("不支持此类外部链接".into());
        }
        NSURL::URLWithString(&NSString::from_str(url.as_str())).ok_or_else(|| "链接格式无效".into())
    }

    #[test]
    fn editor_link_accepts_only_external_link_schemes() {
        assert!(external_url("https://example.com").is_ok());
        assert!(external_url("mailto:test@example.com").is_ok());
        assert!(external_url("file:///Applications/Safari.app").is_err());
        assert!(external_url("javascript:alert(1)").is_err());
    }

    pub fn link_applications(target: &str) -> Result<Vec<(String, String, bool)>, String> {
        let url = external_url(target)?;
        let workspace = NSWorkspace::sharedWorkspace();
        let default = workspace
            .URLForApplicationToOpenURL(&url)
            .and_then(|app| app.path());
        let manager = NSFileManager::defaultManager();
        let mut apps: Vec<_> = workspace
            .URLsForApplicationsToOpenURL(&url)
            .iter()
            .filter_map(|app| {
                let path = app.path()?;
                let is_default = default.as_deref() == Some(&*path);
                let label = manager.displayNameAtPath(&path).to_string();
                Some((path.to_string(), label, is_default))
            })
            .collect();
        apps.sort_by(|left, right| right.2.cmp(&left.2).then_with(|| left.1.cmp(&right.1)));
        Ok(apps)
    }

    pub async fn open_external_link(target: &str, application: Option<&str>) -> Result<(), String> {
        let receive = {
            let url = external_url(target)?;
            let workspace = NSWorkspace::sharedWorkspace();
            let Some(application) = application else {
                return workspace
                    .openURL(&url)
                    .then_some(())
                    .ok_or_else(|| "无法打开链接".into());
            };
            let app = workspace
                .URLsForApplicationsToOpenURL(&url)
                .iter()
                .find(|app| {
                    app.path()
                        .is_some_and(|path| path.to_string() == application)
                })
                .ok_or("此应用无法打开该链接")?;
            let (send, receive) = std::sync::mpsc::sync_channel(1);
            let completion =
                RcBlock::new(move |_: *mut NSRunningApplication, error: *mut NSError| {
                    // AppKit owns the error for the duration of this callback.
                    let result = unsafe { error.as_ref() }.map_or(Ok(()), |error| {
                        Err(error.localizedDescription().to_string())
                    });
                    let _ = send.send(result);
                });
            workspace.openURLs_withApplicationAtURL_configuration_completionHandler(
                &NSArray::from_retained_slice(&[url]),
                &app,
                &NSWorkspaceOpenConfiguration::configuration(),
                Some(&completion),
            );
            receive
        };
        tauri::async_runtime::spawn_blocking(move || {
            receive.recv().map_err(|_| "无法打开链接".to_string())?
        })
        .await
        .map_err(|error| error.to_string())?
    }

    // MiaoYan Extensions/NSMenu+.swift: use template SF Symbols so AppKit owns hover and dark-mode tint.
    fn menu_icons(menu: &NSMenu, moving: bool) {
        if callout_menu_icons(menu) {
            return;
        }
        let note_menu = menu.itemArray().iter().any(|item| {
            matches!(
                item.title().to_string().as_str(),
                "显示简介" | "版本历史…" | "恢复笔记"
            )
        });
        let moving = moving || matches!(menu.title().to_string().as_str(), "移动" | "移动到…");
        let folder_menu = menu.itemArray().iter().any(|item| {
            [
                "新建文件夹",
                "新建笔记",
                "新建子文件夹",
                "在终端中打开",
                "在 Finder 中显示",
            ]
            .contains(&item.title().to_string().as_str())
        });
        let table_menu = menu.itemArray().iter().any(|item| {
            matches!(
                item.title().to_string().as_str(),
                "在下方插入行" | "在左侧插入列"
            )
        });
        let editor_menu = table_menu
            || menu.itemArray().iter().any(|item| {
                matches!(
                    item.title().to_string().as_str(),
                    "打开链接"
                        | "打开笔记"
                        | "打开附件"
                        | "打开图片"
                        | "复制代码"
                        | "分享为图片…"
                        | "取消勾选此清单的所有任务"
                )
            });
        if !note_menu && !folder_menu && !moving && !editor_menu {
            return;
        }
        if table_menu {
            menu.setAllowsContextMenuPlugIns(false);
            if NSProcessInfo::processInfo()
                .operatingSystemVersion()
                .majorVersion
                >= 15
            {
                menu.setAutomaticallyInsertsWritingToolsItems(false);
            }
        }
        if note_menu || folder_menu {
            menu.setMinimumWidth(210.0);
        }
        for item in menu.itemArray() {
            if item.isSeparatorItem() || item.image().is_some() {
                continue;
            }
            let title = item.title().to_string();
            let symbol = match title.as_str() {
                "显示简介" => "info.circle",
                "置顶" => "pin",
                "取消置顶" => "pin.slash",
                "在新窗口打开笔记" | "在新窗口打开" => "macwindow",
                "分享…" => "square.and.arrow.up",
                "分享为图片…" => "photo",
                "制作成应用…" => "app",
                "版本历史…" => "clock.arrow.circlepath",
                "复制笔记" | "复制文件夹" => "plus.square.on.square",
                "重命名" | "重命名文件夹" => "pencil",
                "新建文件夹" | "新建子文件夹" => "folder.badge.plus",
                "新建笔记" => "square.and.pencil",
                "上移" => "arrow.up",
                "下移" => "arrow.down",
                "在 Finder 中显示" => "folder",
                "在终端中打开" => "terminal",
                "移到废纸篓" | "废纸篓" => "trash",
                "导出" => "square.and.arrow.up",
                "移动" | "移动到…" => "arrow.up.right.circle",
                "恢复笔记" => "arrow.uturn.backward",
                "打开链接" => "arrow.up.right",
                "打开笔记" => "doc.text",
                "打开附件" | "打开图片" => "eye",
                "图片尺寸" => "arrow.up.left.and.arrow.down.right",
                "打开方式" => "arrow.up.forward.app",
                "复制链接" | "复制代码" => "doc.on.doc",
                "显示为链接" => "link",
                "紧凑显示" => "capsule",
                "显示为卡片" => "rectangle.portrait.topthird.inset.filled",
                "从笔记中移除" => "trash",
                "取消勾选此清单的所有任务" => "checklist.unchecked",
                "剪切" => "scissors",
                "复制" => "document.on.document",
                "粘贴" => "document.on.clipboard",
                "在下方插入行" => "arrow.down",
                "在上方插入行" => "arrow.up",
                "在右侧插入列" => "arrow.right",
                "在左侧插入列" => "arrow.left",
                "删除行" | "删除列" | "删除表格" => "trash",
                _ if moving => "folder",
                _ => continue,
            };
            if let Some(image) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
                &NSString::from_str(symbol),
                Some(&item.title()),
            ) {
                image.setTemplate(true);
                item.setImage(Some(&image));
            }
            if matches!(title.as_str(), "移动" | "移动到…") {
                if let Some(submenu) = item.submenu() {
                    menu_icons(&submenu, true);
                }
            }
        }
    }

    fn callout_menu_icons(menu: &NSMenu) -> bool {
        let kinds = [
            ("Note", "pencil", "alert-note-rgb"),
            ("Info", "info.circle.fill", "alert-note-rgb"),
            ("Tip", "lightbulb.fill", "alert-tip-rgb"),
            ("Success", "checkmark.circle.fill", "alert-tip-rgb"),
            ("Question", "questionmark.circle.fill", "alert-warning-rgb"),
            (
                "Warning",
                "exclamationmark.triangle.fill",
                "alert-warning-rgb",
            ),
            ("Danger", "bolt.fill", "alert-caution-rgb"),
            ("Bug", "ladybug.fill", "alert-caution-rgb"),
            ("Example", "list.bullet", "alert-important-rgb"),
            ("Quote", "quote.opening", "alert-quote-rgb"),
        ];
        let items = menu.itemArray();
        if items.len() != kinds.len()
            || !items
                .iter()
                .map(|item| item.title().to_string())
                .eq(kinds.map(|kind| kind.0))
        {
            return false;
        }
        let Some(main) = MainThreadMarker::new() else {
            return false;
        };
        let appearance = NSApplication::sharedApplication(main).effectiveAppearance();
        let mode = if appearance.name().to_string().contains("Dark") {
            "dark"
        } else {
            "light"
        };
        let palette: serde_json::Value =
            serde_json::from_str(include_str!("../../src/palette.json")).unwrap_or_default();
        let supports_color = NSProcessInfo::processInfo()
            .operatingSystemVersion()
            .majorVersion
            >= 12;
        for (item, (_, symbol, role)) in items.iter().zip(kinds) {
            let Some(image) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
                &NSString::from_str(symbol),
                Some(&item.title()),
            ) else {
                continue;
            };
            let rgb: Vec<_> = palette[mode][role]
                .as_str()
                .unwrap_or_default()
                .split(',')
                .filter_map(|channel| channel.trim().parse::<u8>().ok())
                .collect();
            if let (true, [red, green, blue]) = (supports_color, rgb.as_slice()) {
                let color = NSColor::colorWithSRGBRed_green_blue_alpha(
                    f64::from(*red) / 255.,
                    f64::from(*green) / 255.,
                    f64::from(*blue) / 255.,
                    1.,
                );
                let config =
                    NSImageSymbolConfiguration::configurationWithPointSize_weight(13., unsafe {
                        NSFontWeightSemibold
                    });
                let palette = NSImageSymbolConfiguration::configurationWithPaletteColors(
                    &NSArray::from_retained_slice(&[color]),
                );
                if let Some(image) = image.imageWithSymbolConfiguration(
                    &config.configurationByApplyingConfiguration(&palette),
                ) {
                    image.setTemplate(false);
                    item.setImage(Some(&image));
                    continue;
                }
            }
            item.setImage(Some(&image));
        }
        true
    }

    pub fn install_menu_icons() {
        if NSProcessInfo::processInfo()
            .operatingSystemVersion()
            .majorVersion
            < 11
        {
            return;
        }
        MENU_OBSERVER.with_borrow_mut(|observer| {
            if observer.is_some() {
                return;
            }
            let block = RcBlock::new(|note: NonNull<NSNotification>| {
                if MainThreadMarker::new().is_none() {
                    return;
                }
                // AppKit posts this synchronously on the UI thread while constructing a menu.
                if let Some(menu) = unsafe { note.as_ref() }
                    .object()
                    .and_then(|object| object.downcast::<NSMenu>().ok())
                {
                    menu_icons(&menu, false);
                }
            });
            *observer = Some(unsafe {
                NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
                    Some(NSMenuDidAddItemNotification),
                    None,
                    None,
                    &block,
                )
            });
        });
    }

    pub fn close_info() {
        INFO.with_borrow_mut(|info| {
            if let Some(info) = info.take() {
                info.close();
            }
        });
    }

    pub fn show_info(
        window: tauri::WebviewWindow,
        info: NoteInfo,
        anchor: Anchor,
    ) -> Result<(), String> {
        let mtm = MainThreadMarker::new().ok_or("简介需要在主线程显示")?;
        let pointer = window.ns_view().map_err(|_| "笔记窗口不可用")?;
        // The window owns this NSView for the duration of the command.
        let parent = unsafe { &*pointer.cast::<NSView>() };
        close_info();
        let dark = window.theme().ok() == Some(tauri::Theme::Dark);
        let color = |[red, green, blue, alpha]: [f64; 4]| {
            NSColor::colorWithSRGBRed_green_blue_alpha(red, green, blue, alpha)
        };
        let card = color(palette_color(dark, "card")?);
        let muted = color(palette_color(dark, "muted")?);
        let ink = color(palette_color(dark, "ink")?);
        let line = color(palette_color(dark, "line")?);
        let rect =
            |x, y, width, height| NSRect::new(NSPoint::new(x, y), NSSize::new(width, height));
        let background = NSView::initWithFrame(NSView::alloc(mtm), rect(0., 0., 300., 210.));
        background.setWantsLayer(true);
        background
            .layer()
            .ok_or("简介视图不可用")?
            .setBackgroundColor(Some(&card.CGColor()));
        let content = &background;
        for (top, text, bold) in [
            (14., format!("字数：{}", info.words), false),
            (38., format!("修改：{}", info.modified), false),
            (62., format!("创建：{}", info.created), false),
            (
                100.,
                format!(
                    "反向链接：{}",
                    if info.backlinks.is_empty() {
                        "无".into()
                    } else {
                        info.backlinks.len().to_string()
                    }
                ),
                true,
            ),
        ] {
            let label = NSTextField::labelWithString(&NSString::from_str(&text), mtm);
            label.setFrame(rect(14., 210. - top - 16., 272., 16.));
            // These are immutable AppKit constants, read only on the UI thread.
            let weight = unsafe {
                if bold {
                    NSFontWeightSemibold
                } else {
                    NSFontWeightRegular
                }
            };
            label.setFont(Some(&NSFont::systemFontOfSize_weight(12., weight)));
            label.setTextColor(Some(if bold { &ink } else { &muted }));
            content.addSubview(&label);
        }
        let separator = NSView::initWithFrame(NSView::alloc(mtm), rect(14., 123., 272., 1.));
        separator.setWantsLayer(true);
        separator
            .layer()
            .ok_or("简介分隔线不可用")?
            .setBackgroundColor(Some(&line.CGColor()));
        content.addSubview(&separator);
        if !info.backlinks.is_empty() {
            let scroll =
                NSScrollView::initWithFrame(NSScrollView::alloc(mtm), rect(14., 14., 272., 82.));
            scroll.setHasVerticalScroller(true);
            scroll.setAutohidesScrollers(true);
            scroll.setDrawsBackground(false);
            let text = NSTextView::initWithFrame(
                NSTextView::alloc(mtm),
                rect(0., 0., 272., (info.backlinks.len() as f64 * 17.).max(82.)),
            );
            text.setString(&NSString::from_str(&info.backlinks.join("\n")));
            text.setEditable(false);
            text.setSelectable(true);
            text.setDrawsBackground(false);
            text.setFont(Some(&NSFont::systemFontOfSize(12.)));
            text.setTextColor(Some(&muted));
            scroll.setDocumentView(Some(&text));
            content.addSubview(&scroll);
        }
        let controller = NSViewController::new(mtm);
        controller.setView(&background);
        let popover = NSPopover::new(mtm);
        popover.setContentViewController(Some(&controller));
        popover.setContentSize(NSSize::new(300., 210.));
        popover.setBehavior(NSPopoverBehavior::Semitransient);
        let y = if parent.isFlipped() {
            anchor.y
        } else {
            parent.bounds().size.height - anchor.y - anchor.height
        };
        popover.showRelativeToRect_ofView_preferredEdge(
            rect(anchor.x, y, anchor.width, anchor.height),
            parent,
            NSRectEdge::MaxX,
        );
        if let Some(panel) = background.window() {
            let origin = panel.frame().origin;
            panel.setFrameOrigin(NSPoint::new(origin.x + 18., origin.y));
            panel.makeKeyWindow();
        }
        INFO.with_borrow_mut(|slot| *slot = Some(popover));
        Ok(())
    }
}

pub fn install_menu_icons() {
    #[cfg(target_os = "macos")]
    mac::install_menu_icons();
}

#[tauri::command]
pub fn close_note_info() {
    #[cfg(target_os = "macos")]
    mac::close_info();
}

#[tauri::command]
pub fn show_note_info(
    window: tauri::WebviewWindow,
    info: NoteInfo,
    anchor: Anchor,
) -> Result<(), String> {
    if [anchor.x, anchor.y, anchor.width, anchor.height]
        .iter()
        .any(|value| !value.is_finite() || *value < 0. || *value > 1_000_000.)
        || info.backlinks.len() > 20_000
        || info.modified.len() > 100
        || info.created.len() > 100
    {
        return Err("简介数据无效".into());
    }
    #[cfg(target_os = "macos")]
    {
        mac::show_info(window, info, anchor)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, info, anchor);
        Err("当前平台使用笔记简介浮层".into())
    }
}
