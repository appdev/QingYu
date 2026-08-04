use serde::{Deserialize, Serialize};
use tauri::{
    plugin::{Builder, PluginHandle, TauriPlugin},
    Manager as _, State,
};

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct PickMobileImagesResponse {
    uris: Vec<String>,
}

#[derive(Clone)]
pub(crate) struct MobileImagePickerPlugin(PluginHandle<tauri::Wry>);

pub(crate) fn init() -> TauriPlugin<tauri::Wry> {
    Builder::new("qingyu-mobile-image-picker")
        .setup(|app, api| {
            #[cfg(target_os = "android")]
            {
                let handle =
                    api.register_android_plugin("dev.markra.app", "QingyuImagePickerPlugin")?;
                app.manage(MobileImagePickerPlugin(handle));
            }
            Ok(())
        })
        .build()
}

#[tauri::command]
pub(crate) fn pick_mobile_images(
    picker: State<'_, MobileImagePickerPlugin>,
    title: Option<String>,
) -> Result<PickMobileImagesResponse, String> {
    picker
        .0
        .run_mobile_plugin("pickImages", serde_json::json!({ "title": title }))
        .map_err(|error| error.to_string())
}
