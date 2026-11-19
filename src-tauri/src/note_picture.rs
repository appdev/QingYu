use serde::Deserialize;
use tauri::Manager;

#[derive(Deserialize)]
pub struct PictureAnchor {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[tauri::command]
pub async fn note_share_text(
    window: tauri::WebviewWindow,
    title: String,
    text: String,
    anchor: PictureAnchor,
) -> Result<(), String> {
    if window.label() != "main"
        && !crate::file_library::workspace::is_document(window.app_handle(), window.label())
    {
        return Err("请在编辑器中分享笔记".into());
    }
    if title.len() > 4096 || text.len() > 16 << 20 {
        return Err("笔记过长，无法发送".into());
    }
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        let _ = anchor;
        return tauri_plugin_attachments::share_text(window.app_handle().clone(), title, text)
            .await;
    }
    #[cfg(target_os = "macos")]
    {
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        let owner = window.clone();
        window
            .run_on_main_thread(move || {
                let _ = send.send(mac::share_text(&owner, &text, anchor));
            })
            .map_err(|e| e.to_string())?;
        return tauri::async_runtime::spawn_blocking(move || {
            receive.recv().map_err(|_| "无法打开系统分享".to_string())?
        })
        .await
        .map_err(|e| e.to_string())?;
    }
    #[cfg(not(any(target_os = "macos", target_os = "android", target_os = "ios")))]
    {
        let _ = (text, anchor);
        Err("当前平台不支持系统分享，请复制为纯文本".into())
    }
}

#[tauri::command]
pub async fn note_picture_action(
    window: tauri::WebviewWindow,
    action: String,
    filename: String,
    png: Vec<u8>,
    anchor: PictureAnchor,
) -> Result<bool, String> {
    if window.label() != "main"
        && !crate::file_library::workspace::is_document(window.app_handle(), window.label())
        && !crate::feature_window::is_picture_window(window.label())
    {
        return Err("请在编辑器中导出图片".into());
    }
    if filename.len() > 320
        || !filename.ends_with(".png")
        || filename
            .chars()
            .any(|c| c.is_control() || c == '/' || c == '\\')
        || png.len() > 32 << 20
        || !png.starts_with(b"\x89PNG\r\n\x1a\n")
    {
        return Err("无法识别这张 PNG 图片".into());
    }
    #[cfg(mobile)]
    if action == "share" {
        let app = window.app_handle().clone();
        let cache = app
            .path()
            .app_cache_dir()
            .map_err(|e| e.to_string())?
            .join("attachments/pictures");
        let (temp, path) = tauri::async_runtime::spawn_blocking(move || {
            use std::fs;
            fs::create_dir_all(&cache)?;
            for entry in fs::read_dir(&cache)?.flatten() {
                let stale = entry
                    .metadata()
                    .and_then(|meta| meta.modified())
                    .ok()
                    .and_then(|modified| modified.elapsed().ok())
                    .is_some_and(|age| age.as_secs() > 86_400);
                if stale {
                    let _ = fs::remove_dir_all(entry.path());
                }
            }
            let temp = tempfile::tempdir_in(cache)?;
            let path = temp.path().join(filename);
            fs::write(&path, png)?;
            Ok::<_, std::io::Error>((temp, path))
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|_| "无法准备待分享图片")?;
        tauri_plugin_attachments::share_picture(app, path).await?;
        // The recipient may read the picture after the chooser returns.
        let _ = temp.keep();
        return Ok(true);
    }
    if action == "save" {
        #[cfg(target_os = "macos")]
        let path = mac::destination(window, filename).await?;
        #[cfg(all(desktop, not(target_os = "macos")))]
        let path = tauri::async_runtime::spawn_blocking(move || {
            use tauri_plugin_dialog::DialogExt;
            window
                .dialog()
                .file()
                .set_parent(&window)
                .set_file_name(filename)
                .add_filter("PNG", &["png"])
                .blocking_save_file()
                .map(|path| {
                    path.into_path()
                        .map_err(|_| "请选择本地文件位置".to_string())
                })
                .transpose()
        })
        .await
        .map_err(|_| "未能打开保存窗口".to_string())??;
        #[cfg(desktop)]
        {
            let Some(path) = path else {
                return Ok(false);
            };
            return tauri::async_runtime::spawn_blocking(move || {
                std::fs::write(path, png)
                    .map_err(|_| "无法保存图片，请检查位置和剩余空间".to_string())?;
                Ok(true)
            })
            .await
            .map_err(|_| "未能保存图片".to_string())?;
        }
    }
    #[cfg(target_os = "macos")]
    if matches!(action.as_str(), "copy" | "share") {
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        let owner = window.clone();
        window
            .run_on_main_thread(move || {
                let _ = send.send(mac::present(&owner, &action, png, anchor));
            })
            .map_err(|e| e.to_string())?;
        return tauri::async_runtime::spawn_blocking(move || {
            receive.recv().map_err(|_| "无法处理图片".to_string())?
        })
        .await
        .map_err(|e| e.to_string())?;
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (anchor.x, anchor.y, anchor.width, anchor.height);
    Err("当前平台不支持此图片操作".into())
}

#[cfg(target_os = "macos")]
mod mac {
    use super::PictureAnchor;
    use objc2::{rc::Retained, AnyThread, MainThreadMarker};
    use objc2_app_kit::{
        NSImage, NSModalResponseOK, NSPasteboard, NSPasteboardTypePNG, NSSavePanel,
        NSSharingServicePicker, NSView,
    };
    use objc2_foundation::{NSArray, NSData, NSPoint, NSRect, NSRectEdge, NSSize, NSString};
    use std::cell::RefCell;
    use std::path::PathBuf;

    thread_local! {
        static PICKER: RefCell<Option<Retained<NSSharingServicePicker>>> = const { RefCell::new(None) };
    }

    pub async fn destination(
        window: tauri::WebviewWindow,
        filename: String,
    ) -> Result<Option<PathBuf>, String> {
        crate::note_ui::show_native_sheet(window, move |main, parent, send| {
            let panel = NSSavePanel::savePanel(main);
            // Use the file type identifier for compatibility with macOS 10.13–10.15.
            #[allow(deprecated)]
            panel.setAllowedFileTypes(Some(&NSArray::from_slice(&[&*NSString::from_str(
                "public.png",
            )])));
            panel.setNameFieldStringValue(&NSString::from_str(&filename));
            panel.setCanCreateDirectories(true);
            let selected = panel.clone();
            let completion = block2::RcBlock::new(move |response| {
                let path = if response == NSModalResponseOK {
                    selected
                        .URL()
                        .and_then(|url| url.to_file_path())
                        .map(Some)
                        .ok_or_else(|| "请选择本地文件位置".to_string())
                } else {
                    Ok(None)
                };
                let _ = send.send(path);
            });
            panel.beginSheetModalForWindow_completionHandler(parent, &completion);
        })
        .await
    }

    pub fn present(
        window: &tauri::WebviewWindow,
        action: &str,
        png: Vec<u8>,
        anchor: PictureAnchor,
    ) -> Result<bool, String> {
        let data = NSData::with_bytes(&png);
        if action == "copy" {
            let pasteboard = NSPasteboard::generalPasteboard();
            pasteboard.clearContents();
            return pasteboard
                .setData_forType(Some(&data), unsafe { NSPasteboardTypePNG })
                .then_some(true)
                .ok_or_else(|| "无法复制图片".into());
        }
        let image = NSImage::initWithData(NSImage::alloc(), &data).ok_or("无法读取 PNG")?;
        let item = Retained::into_super(Retained::into_super(image));
        let items = NSArray::from_retained_slice(&[item]);
        show_picker(window, &items, anchor)?;
        Ok(true)
    }

    pub fn share_text(
        window: &tauri::WebviewWindow,
        text: &str,
        anchor: PictureAnchor,
    ) -> Result<(), String> {
        let item = Retained::into_super(Retained::into_super(NSString::from_str(text)));
        show_picker(window, &NSArray::from_retained_slice(&[item]), anchor)
    }

    fn show_picker(
        window: &tauri::WebviewWindow,
        items: &NSArray<objc2::runtime::AnyObject>,
        anchor: PictureAnchor,
    ) -> Result<(), String> {
        let view = window.ns_view().map_err(|_| "编辑窗口不可用")?;
        if MainThreadMarker::new().is_none()
            || [anchor.x, anchor.y, anchor.width, anchor.height]
                .iter()
                .any(|x| !x.is_finite() || *x < 0. || *x > 10000.)
        {
            return Err("无法定位分享按钮".into());
        }
        // The window owns the view while this main-thread callback runs.
        let view = unsafe { &*view.cast::<NSView>() };
        // NSImage and NSString implement NSPasteboardWriting, as required by AppKit.
        let picker = unsafe {
            NSSharingServicePicker::initWithItems(NSSharingServicePicker::alloc(), items)
        };
        let y = if view.isFlipped() {
            anchor.y
        } else {
            view.bounds().size.height - anchor.y - anchor.height
        };
        picker.showRelativeToRect_ofView_preferredEdge(
            NSRect::new(
                NSPoint::new(anchor.x, y),
                NSSize::new(anchor.width, anchor.height),
            ),
            view,
            NSRectEdge::MaxY,
        );
        PICKER.with_borrow_mut(|slot| *slot = Some(picker));
        Ok(())
    }
}
