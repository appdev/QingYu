use serde::Deserialize;

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
pub async fn prompt_folder_name(window: tauri::WebviewWindow) -> Result<Option<String>, String> {
    if window.label() != "main" {
        return Err("请在主窗口创建文件夹".into());
    }
    #[cfg(target_os = "macos")]
    {
        folder_dialog::name(window).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        Err("当前平台使用应用内创建窗口".into())
    }
}

#[cfg(target_os = "macos")]
pub(crate) use folder_dialog::{
    confirm_trash as confirm_folder_trash, destination as choose_folder_destination,
};

#[cfg(target_os = "macos")]
mod folder_dialog {
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{
        NSAlert, NSAlertFirstButtonReturn, NSModalResponseOK, NSOpenPanel, NSTextField, NSWindow,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString, NSURL};
    use std::sync::mpsc::SyncSender;
    type Reply<T> = SyncSender<Result<T, String>>;

    async fn sheet<T: Send + 'static>(
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
            receive.recv().map_err(|_| "文件夹窗口已关闭".to_string())?
        })
        .await
        .map_err(|reason| reason.to_string())?
    }

    pub async fn name(window: tauri::WebviewWindow) -> Result<Option<String>, String> {
        sheet(window, |main, parent, send| {
            let alert = NSAlert::new(main);
            alert.setMessageText(&NSString::from_str("新建文件夹"));
            alert.addButtonWithTitle(&NSString::from_str("添加"));
            alert.addButtonWithTitle(&NSString::from_str("取消"));
            let field = NSTextField::initWithFrame(
                NSTextField::alloc(main),
                NSRect::new(NSPoint::new(0., 0.), NSSize::new(320., 26.)),
            );
            field.setPlaceholderString(Some(&NSString::from_str("文件夹名称")));
            alert.setAccessoryView(Some(&field));
            alert.window().setInitialFirstResponder(Some(&field));
            let completion = block2::RcBlock::new(move |response| {
                let _ = send
                    .send(Ok((response == NSAlertFirstButtonReturn)
                        .then(|| field.stringValue().to_string())));
            });
            alert.beginSheetModalForWindow_completionHandler(parent, Some(&completion));
        })
        .await
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
        NSNotification, NSNotificationCenter, NSObjectProtocol, NSPoint, NSProcessInfo, NSRect,
        NSRectEdge, NSSize, NSString,
    };
    use std::{cell::RefCell, ptr::NonNull};

    thread_local! {
        static MENU_OBSERVER: RefCell<Option<Retained<ProtocolObject<dyn NSObjectProtocol>>>> = const { RefCell::new(None) };
        static INFO: RefCell<Option<Retained<NSPopover>>> = const { RefCell::new(None) };
    }

    // MiaoYan Extensions/NSMenu+.swift: use template SF Symbols so AppKit owns hover and dark-mode tint.
    fn menu_icons(menu: &NSMenu, moving: bool) {
        let note_menu = menu
            .itemArray()
            .iter()
            .any(|item| item.title().to_string() == "显示简介");
        let moving = moving || menu.title().to_string() == "移动";
        let folder_menu = menu.itemArray().iter().any(|item| {
            [
                "新建文件夹",
                "新建子文件夹",
                "在终端中打开",
                "在 Finder 中显示",
            ]
            .contains(&item.title().to_string().as_str())
        });
        if !note_menu && !folder_menu && !moving {
            return;
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
                "置顶" | "取消置顶" => "flag",
                "制作副本" => "plus.square.on.square",
                "重命名" | "重命名文件夹" => "pencil",
                "新建文件夹" | "新建子文件夹" => "folder.badge.plus",
                "在 Finder 中显示" => "folder",
                "在终端中打开" => "terminal",
                "移到废纸篓" | "废纸篓" => "trash",
                "导出" => "square.and.arrow.up",
                "移动" => "arrow.up.right.circle",
                "恢复笔记" => "arrow.uturn.backward",
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
            if title == "移动" {
                if let Some(submenu) = item.submenu() {
                    menu_icons(&submenu, true);
                }
            }
        }
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
        let color = |rgb: u32| {
            NSColor::colorWithSRGBRed_green_blue_alpha(
                f64::from((rgb >> 16) & 255) / 255.,
                f64::from((rgb >> 8) & 255) / 255.,
                f64::from(rgb & 255) / 255.,
                1.,
            )
        };
        let paper = color(if dark { 0x1e1d1b } else { 0xfdfcf9 });
        let muted = color(if dark { 0x8e8b83 } else { 0x7a7872 });
        let ink = color(if dark { 0xf2f0ea } else { 0x363433 });
        let line = color(if dark { 0x3a3936 } else { 0xe8e6dc });
        let rect =
            |x, y, width, height| NSRect::new(NSPoint::new(x, y), NSSize::new(width, height));
        let background = NSView::initWithFrame(NSView::alloc(mtm), rect(0., 0., 300., 210.));
        background.setWantsLayer(true);
        background
            .layer()
            .ok_or("简介视图不可用")?
            .setBackgroundColor(Some(&paper.CGColor()));
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
