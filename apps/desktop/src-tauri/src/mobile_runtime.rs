use std::sync::Arc;

use tauri::Manager as _;

use crate::{mobile_back::MobileBackState, mobile_kernel_runtime::MobileKernelRuntimeState};

pub(crate) fn run() {
    tauri::Builder::default()
        .manage(MobileBackState::default())
        .manage(crate::themes::ThemeActivationState::default())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_log::Builder::new().build())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_opener::init())
        .setup(crate::mobile_kernel_runtime::install_mobile_kernel_runtime)
        .invoke_handler(tauri::generate_handler![
            crate::mobile_kernel_runtime::read_mobile_kernel_bootstrap,
            crate::themes::list_themes,
            crate::themes::read_theme_css,
            crate::themes::activation::prepare_theme_activation,
            crate::themes::activation::commit_theme_activation,
            crate::themes::activation::cancel_theme_activation,
            crate::themes::activation::release_theme_activation,
            crate::themes::delete_theme,
            crate::mobile_back::complete_mobile_back,
        ])
        .build(tauri::generate_context!())
        .expect("error while building QingYu mobile")
        .run(|app, event| match event {
            tauri::RunEvent::ExitRequested {
                code: None, api, ..
            } => {
                api.prevent_exit();
                crate::mobile_back::emit_mobile_back_requested(app);
            }
            tauri::RunEvent::ExitRequested {
                code: Some(code),
                api,
                ..
            } => request_mobile_kernel_exit(app, code, &api),
            _ => {}
        });
}

fn request_mobile_kernel_exit<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    code: i32,
    api: &tauri::ExitRequestApi,
) {
    let Some(runtime) = app.try_state::<Arc<MobileKernelRuntimeState>>() else {
        return;
    };
    if runtime.terminal_exit_is_ready() {
        return;
    }
    api.prevent_exit();
    if !runtime.begin_terminal_exit() {
        return;
    }
    let runtime = runtime.inner().clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _settled = runtime.stop().await;
        runtime.mark_terminal_exit_ready();
        app.exit(code);
    });
}
