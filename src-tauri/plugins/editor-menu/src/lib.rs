use tauri::{
    plugin::{Builder, PluginHandle, TauriPlugin},
    AppHandle, Manager, Wry,
};

tauri::ios_plugin_binding!(init_plugin_editor_menu);

struct EditorMenu(PluginHandle<Wry>);

pub fn init() -> TauriPlugin<Wry> {
    Builder::new("editor-menu")
        .setup(|app, api| {
            app.manage(EditorMenu(
                api.register_ios_plugin(init_plugin_editor_menu)?,
            ));
            Ok(())
        })
        .build()
}

pub async fn show(app: &AppHandle, menu: serde_json::Value) -> Result<Option<usize>, String> {
    app.state::<EditorMenu>()
        .0
        .run_mobile_plugin_async("showTableHandleMenu", menu)
        .await
        .map_err(|error| error.to_string())
}
