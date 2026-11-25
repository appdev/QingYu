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

/// 工具栏按钮的系统菜单。选中时返回 `{ "id": 编号 }`，取消时返回空对象。
pub async fn show_menu(app: &AppHandle, menu: serde_json::Value) -> Result<serde_json::Value, String> {
    app.state::<EditorMenu>()
        .0
        .run_mobile_plugin_async("showMenu", menu)
        .await
        .map_err(|error| error.to_string())
}

pub async fn show(app: &AppHandle, menu: serde_json::Value) -> Result<Option<usize>, String> {
    app.state::<EditorMenu>()
        .0
        .run_mobile_plugin_async("showTableHandleMenu", menu)
        .await
        .map_err(|error| error.to_string())
}
