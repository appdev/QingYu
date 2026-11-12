use tauri::plugin::{Builder, TauriPlugin};
#[cfg(any(target_os = "android", target_os = "ios"))]
use tauri::Manager;
#[cfg(any(target_os = "android", target_os = "ios"))]
struct AttachmentHandle(tauri::plugin::PluginHandle<tauri::Wry>);
#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_attachments);

pub fn init() -> TauriPlugin<tauri::Wry> {
    Builder::new("attachments")
        .setup(|_app, _api| {
            #[cfg(target_os = "android")]
            _app.manage(AttachmentHandle(_api.register_android_plugin(
                "app.miaoyan.attachments",
                "AttachmentsPlugin",
            )?));
            #[cfg(target_os = "ios")]
            _app.manage(AttachmentHandle(
                _api.register_ios_plugin(init_plugin_attachments)?,
            ));
            Ok(())
        })
        .build()
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub async fn open(
    app: tauri::AppHandle,
    path: std::path::PathBuf,
    mime: &'static str,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<AttachmentHandle>()
            .0
            .run_mobile_plugin::<serde_json::Value>(
                "openFile",
                serde_json::json!({ "path": path, "mime": mime }),
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|_| "无法打开附件".to_string())?
}
