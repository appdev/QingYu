use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use crate::app_exit::handle_app_exit_requested;
use crate::markdown_files::MarkdownTreeLoadState;
use crate::mcp;
use crate::menu::{
    apply_native_application_menu_for_window_event, create_application_menu,
    emit_native_menu_command_payload, is_native_new_window_command, native_menu_command_from_id,
    remember_native_menu_webview_window, remember_native_menu_window_from_event,
    NativeApplicationMenuState, NativeMenuTargetState,
};
use crate::opened_files::{
    opened_markdown_paths_from_args, opened_markdown_paths_from_args_with_cwd,
    opened_markdown_paths_from_urls, queue_opened_markdown_paths, OpenedMarkdownPathsState,
};
use crate::watcher::{MarkdownFileWatcherState, MarkdownTreeWatcherState};
use crate::window_state::{remove_editor_window_restore_state, EditorWindowRestoreState};
use crate::windows::{
    apply_main_window_chrome, apply_settings_window_lifecycle, apply_webview_window_chrome,
    apply_window_event_chrome, editor_window_url_for_path, is_editor_window_label,
    spawn_blank_editor_window, spawn_editor_window,
};
use tauri::Manager;
use tauri_plugin_window_state::StateFlags;

const STARTUP_WINDOW_NATIVE_REVEAL_FALLBACK_MS: u64 = 2400;
const DESKTOP_LOG_MAX_FILE_SIZE_BYTES: u128 = 2 * 1024 * 1024;
const DESKTOP_LOG_MAX_FILE_COUNT: usize = 5;
// tauri-plugin-log's KeepSome count applies only to archived files; the active
// log file is additional, so keep one fewer archive to cap total files.
const DESKTOP_LOG_ARCHIVED_FILE_COUNT: usize = DESKTOP_LOG_MAX_FILE_COUNT - 1;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DesktopLaunchMode {
    Normal,
    McpService,
}

#[derive(Clone)]
struct DesktopUiPromotionRequest {
    paths: Vec<String>,
    reveal_when_empty: bool,
}

#[derive(Default)]
struct DesktopUiPromotionInner {
    creation_in_flight: bool,
    pending: Vec<DesktopUiPromotionRequest>,
    setup_ready: bool,
}

#[derive(Default)]
struct DesktopUiPromotionState {
    inner: Mutex<DesktopUiPromotionInner>,
}

impl DesktopUiPromotionState {
    fn submit(&self, request: DesktopUiPromotionRequest) -> Option<DesktopUiPromotionRequest> {
        let mut inner = match self.inner.lock() {
            Ok(inner) => inner,
            Err(poisoned) => poisoned.into_inner(),
        };
        if !inner.setup_ready {
            inner.pending.push(request);
            return None;
        }
        Some(request)
    }

    fn mark_setup_ready(&self) -> Vec<DesktopUiPromotionRequest> {
        let mut inner = match self.inner.lock() {
            Ok(inner) => inner,
            Err(poisoned) => poisoned.into_inner(),
        };
        inner.setup_ready = true;
        std::mem::take(&mut inner.pending)
    }

    fn begin_creation(&self, request: DesktopUiPromotionRequest) -> bool {
        let mut inner = match self.inner.lock() {
            Ok(inner) => inner,
            Err(poisoned) => poisoned.into_inner(),
        };
        if inner.creation_in_flight {
            inner.pending.push(request);
            return false;
        }
        inner.creation_in_flight = true;
        true
    }

    fn finish_creation(&self) -> Vec<DesktopUiPromotionRequest> {
        let mut inner = match self.inner.lock() {
            Ok(inner) => inner,
            Err(poisoned) => poisoned.into_inner(),
        };
        inner.creation_in_flight = false;
        std::mem::take(&mut inner.pending)
    }
}

fn desktop_launch_mode<I, S>(args: I) -> DesktopLaunchMode
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut args = args.into_iter();
    let Some(_executable) = args.next() else {
        return DesktopLaunchMode::Normal;
    };
    let command = args.next();
    let subcommand = args.next();
    let has_more = args.next().is_some();
    if command.as_ref().map(AsRef::as_ref) == Some(OsStr::new("mcp"))
        && subcommand.as_ref().map(AsRef::as_ref) == Some(OsStr::new("serve"))
        && !has_more
    {
        DesktopLaunchMode::McpService
    } else {
        DesktopLaunchMode::Normal
    }
}

fn should_reveal_single_instance(mode: DesktopLaunchMode) -> bool {
    mode == DesktopLaunchMode::Normal
}

fn guarded_desktop_invoke_handler<R, Handler>(
    handler: Handler,
) -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static
where
    R: tauri::Runtime,
    Handler: Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static,
{
    move |invoke| {
        if !crate::writer_authority::normal_desktop_command_is_allowed(invoke.message.command()) {
            invoke.resolver.reject("native-command-unavailable");
            true
        } else {
            handler(invoke)
        }
    }
}

fn desktop_renderer_origin(url: &tauri::Url) -> Result<String, String> {
    if !url.username().is_empty() || url.password().is_some() {
        return Err("desktop renderer origin is unavailable".to_owned());
    }
    match url.scheme() {
        "http" | "https" => {
            let origin = url.origin().ascii_serialization();
            (origin != "null")
                .then_some(origin)
                .ok_or_else(|| "desktop renderer origin is unavailable".to_owned())
        }
        "tauri" if !url.authority().is_empty() => Ok(format!("tauri://{}", url.authority())),
        _ => Err("desktop renderer origin is unavailable".to_owned()),
    }
}

fn configured_desktop_renderer_origin(
    development: bool,
    dev_url: Option<&tauri::Url>,
    frontend_dist_url: Option<&tauri::Url>,
    windows: bool,
    use_https_scheme: bool,
) -> Result<String, String> {
    if development {
        return dev_url
            .ok_or_else(|| "desktop renderer origin is unavailable".to_owned())
            .and_then(desktop_renderer_origin);
    }
    if let Some(frontend_dist_url) = frontend_dist_url {
        return desktop_renderer_origin(frontend_dist_url);
    }
    if windows {
        let scheme = if use_https_scheme { "https" } else { "http" };
        return Ok(format!("{scheme}://tauri.localhost"));
    }
    Ok("tauri://localhost".to_owned())
}

fn configured_main_renderer_origin(app: &tauri::AppHandle) -> Result<String, String> {
    let main = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == "main")
        .ok_or_else(|| "desktop renderer origin is unavailable".to_owned())?;
    let frontend_dist_url = match app.config().build.frontend_dist.as_ref() {
        Some(tauri::utils::config::FrontendDist::Url(url)) => Some(url),
        _ => None,
    };
    configured_desktop_renderer_origin(
        cfg!(dev),
        app.config().build.dev_url.as_ref(),
        frontend_dist_url,
        cfg!(windows),
        main.use_https_scheme,
    )
}

fn validated_desktop_renderer_origin(
    caller_label: &str,
    configured_origin: &str,
    caller_url: &tauri::Url,
) -> Result<String, String> {
    if caller_label != "main" || desktop_renderer_origin(caller_url)? != configured_origin {
        return Err("desktop renderer origin is unavailable".to_owned());
    }
    Ok(configured_origin.to_owned())
}

fn main_renderer_origin(window: &tauri::WebviewWindow) -> Result<String, String> {
    let configured = configured_main_renderer_origin(&window.app_handle())?;
    let url = window
        .url()
        .map_err(|_| "desktop renderer origin is unavailable".to_owned())?;
    validated_desktop_renderer_origin(window.label(), &configured, &url)
}

#[tauri::command]
fn read_desktop_kernel_startup_state(
    runtime: tauri::State<'_, Arc<crate::desktop_kernel_runtime::DesktopKernelRuntimeState>>,
) -> Result<crate::desktop_kernel_runtime::DesktopKernelStartupSnapshot, String> {
    runtime.snapshot()
}

fn recover_published_desktop_workspace_initialization(
    requested_path: &Path,
    resolution: Result<
        crate::primary_workspace::DesktopPrimaryWorkspaceResolution,
        crate::primary_workspace::DesktopPrimaryWorkspaceResolutionError,
    >,
) -> Result<PathBuf, String> {
    let requested = requested_path
        .to_str()
        .ok_or_else(|| "desktop primary workspace initialization failed".to_owned())?;
    let requested = crate::workspace_membership::canonical_workspace_root(requested)
        .map_err(|_| "desktop primary workspace initialization failed".to_owned())?;
    match resolution {
        Ok(crate::primary_workspace::DesktopPrimaryWorkspaceResolution::Selected(root))
            if root == requested =>
        {
            Ok(root)
        }
        _ => Err("desktop primary workspace initialization failed".to_owned()),
    }
}

#[tauri::command]
async fn initialize_desktop_kernel_workspace(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    runtime: tauri::State<'_, Arc<crate::desktop_kernel_runtime::DesktopKernelRuntimeState>>,
    path: String,
) -> Result<(), String> {
    let origin = main_renderer_origin(&window)?;
    let runtime = Arc::clone(runtime.inner());
    let recover_invalid = runtime.is_invalid()?;
    let persistence_app = app.clone();
    let requested_path = PathBuf::from(path);
    let persisted_request = requested_path.clone();
    let persisted = tauri::async_runtime::spawn_blocking(move || {
        if recover_invalid {
            crate::primary_workspace::recover_invalid_desktop_primary_workspace(
                &persistence_app,
                &persisted_request,
            )
        } else {
            crate::primary_workspace::initialize_desktop_primary_workspace(
                &persistence_app,
                &persisted_request,
            )
        }
    })
    .await
    .map_err(|_| "desktop primary workspace initialization failed".to_owned())?;
    let workspace_root = match persisted {
        Ok(root) => root,
        Err(_) => {
            let resolution_app = app.clone();
            let resolution = tauri::async_runtime::spawn_blocking(move || {
                crate::primary_workspace::resolve_desktop_primary_workspace(&resolution_app)
            })
            .await
            .map_err(|_| "desktop primary workspace initialization failed".to_owned())?;
            recover_published_desktop_workspace_initialization(&requested_path, resolution)?
        }
    };
    runtime.start_selected(&app, workspace_root, origin)
}

#[tauri::command]
async fn switch_desktop_kernel_workspace(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    runtime: tauri::State<'_, Arc<crate::desktop_kernel_runtime::DesktopKernelRuntimeState>>,
    path: String,
) -> Result<(), String> {
    let origin = main_renderer_origin(&window)?;
    let runtime = Arc::clone(runtime.inner());
    let persistence_app = app.clone();
    let requested_path = PathBuf::from(path);
    let prepared = tauri::async_runtime::spawn_blocking(move || {
        crate::primary_workspace::prepare_desktop_primary_workspace_switch(
            &persistence_app,
            &requested_path,
        )
    })
    .await
    .map_err(|_| "desktop primary workspace switch failed".to_owned())??;
    let Some(attempt) = runtime
        .begin_workspace_switch(
            &app,
            &prepared.current_root,
            prepared.target_root.clone(),
            origin,
        )
        .await?
    else {
        return Ok(());
    };

    let target_root = prepared.target_root.clone();
    let commit_app = app.clone();
    let committed = tauri::async_runtime::spawn_blocking(move || {
        crate::primary_workspace::commit_desktop_primary_workspace_switch(&commit_app, &prepared)
    })
    .await
    .map_err(|_| "desktop primary workspace switch failed".to_owned())
    .and_then(|result| result);
    if committed.as_ref() == Ok(&target_root) {
        return runtime.complete_workspace_switch(&app, attempt);
    }

    let resolution_app = app.clone();
    let authoritative_root = tauri::async_runtime::spawn_blocking(move || {
        crate::primary_workspace::resolve_desktop_primary_workspace(&resolution_app)
    })
    .await
    .ok()
    .and_then(Result::ok)
    .and_then(|resolution| match resolution {
        crate::primary_workspace::DesktopPrimaryWorkspaceResolution::Selected(root) => Some(root),
        crate::primary_workspace::DesktopPrimaryWorkspaceResolution::Unselected => None,
    });
    runtime.reconcile_failed_workspace_switch(&app, attempt, authoritative_root);
    Err("desktop primary workspace switch failed".to_owned())
}

#[tauri::command]
async fn retry_desktop_kernel_workspace(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    runtime: tauri::State<'_, Arc<crate::desktop_kernel_runtime::DesktopKernelRuntimeState>>,
) -> Result<(), String> {
    let caller_origin = main_renderer_origin(&window)?;
    let runtime = Arc::clone(runtime.inner());
    if runtime.reserve_resolution_retry(&app)? {
        resolve_and_start_desktop_kernel(app, caller_origin, runtime).await
    } else {
        runtime.retry_selected(&app)
    }
}

async fn resolve_and_start_desktop_kernel(
    app: tauri::AppHandle,
    renderer_origin: String,
    runtime: Arc<crate::desktop_kernel_runtime::DesktopKernelRuntimeState>,
) -> Result<(), String> {
    let persistence_app = app.clone();
    let resolution = tauri::async_runtime::spawn_blocking(move || {
        crate::primary_workspace::resolve_desktop_primary_workspace(&persistence_app)
    })
    .await
    .unwrap_or(Err(
        crate::primary_workspace::DesktopPrimaryWorkspaceResolutionError::Unavailable,
    ));
    match resolution {
        Ok(crate::primary_workspace::DesktopPrimaryWorkspaceResolution::Unselected) => runtime
            .complete_initial_resolution(
                &app,
                crate::desktop_kernel_runtime::DesktopKernelStartupStatus::Unselected,
            ),
        Ok(crate::primary_workspace::DesktopPrimaryWorkspaceResolution::Selected(
            workspace_root,
        )) => runtime.start_resolved(&app, workspace_root, renderer_origin),
        Err(crate::primary_workspace::DesktopPrimaryWorkspaceResolutionError::Invalid) => runtime
            .complete_initial_resolution(
                &app,
                crate::desktop_kernel_runtime::DesktopKernelStartupStatus::Invalid,
            ),
        Err(
            crate::primary_workspace::DesktopPrimaryWorkspaceResolutionError::UnsupportedVersion,
        ) => runtime.complete_initial_resolution(
            &app,
            crate::desktop_kernel_runtime::DesktopKernelStartupStatus::UnsupportedVersion,
        ),
        Err(crate::primary_workspace::DesktopPrimaryWorkspaceResolutionError::Unavailable) => {
            runtime.complete_initial_resolution(
                &app,
                crate::desktop_kernel_runtime::DesktopKernelStartupStatus::Unavailable,
            )
        }
    }
}

fn activate_normal_ui<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let activation_error = app
            .set_activation_policy(tauri::ActivationPolicy::Regular)
            .err()
            .map(|error| format!("QingYu activation policy update failed: {error}"));
        let dock_error = app
            .set_dock_visibility(true)
            .err()
            .map(|error| format!("QingYu Dock visibility update failed: {error}"));
        match (activation_error, dock_error) {
            (None, None) => Ok(()),
            (Some(error), None) | (None, Some(error)) => Err(error),
            (Some(activation), Some(dock)) => Err(format!("{activation}; {dock}")),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _app = app;
        Ok(())
    }
}

#[cfg(test)]
fn test_desktop_launch_mode(args: &[&str]) -> &'static str {
    match desktop_launch_mode(args) {
        DesktopLaunchMode::Normal => "normal",
        DesktopLaunchMode::McpService => "mcp-service",
    }
}

#[cfg(test)]
fn test_should_reveal_single_instance(mode: &str) -> bool {
    let mode = match mode {
        "mcp-service" => DesktopLaunchMode::McpService,
        _ => DesktopLaunchMode::Normal,
    };
    should_reveal_single_instance(mode)
}

fn window_state_restore_flags() -> StateFlags {
    StateFlags::all() - StateFlags::VISIBLE - StateFlags::DECORATIONS
}

fn focus_main_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn configured_main_window<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<tauri::utils::config::WindowConfig, String> {
    let mut windows = app
        .config()
        .app
        .windows
        .iter()
        .filter(|window| window.label == "main");
    let main = windows
        .next()
        .cloned()
        .ok_or_else(|| "configured main window is unavailable".to_owned())?;
    if windows.next().is_some() {
        return Err("configured main window is unavailable".to_owned());
    }
    Ok(main)
}

fn spawn_configured_main_window<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    request: DesktopUiPromotionRequest,
) {
    let Some(state) = app.try_state::<DesktopUiPromotionState>() else {
        return;
    };
    if !state.begin_creation(request.clone()) {
        return;
    }
    if !request.paths.is_empty() {
        queue_opened_markdown_paths(&app, request.paths);
    }
    let config = match configured_main_window(&app) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("QingYu main window configuration failed: {error}");
            for request in state.finish_creation() {
                if !request.paths.is_empty() {
                    queue_opened_markdown_paths(&app, request.paths);
                }
            }
            return;
        }
    };
    std::thread::spawn(move || {
        let built = tauri::WebviewWindowBuilder::from_config(&app, &config)
            .and_then(tauri::WebviewWindowBuilder::build);
        let pending = app
            .try_state::<DesktopUiPromotionState>()
            .map(|state| state.finish_creation())
            .unwrap_or_default();
        match built {
            Ok(window) => {
                remember_native_menu_webview_window(&window);
                apply_webview_window_chrome(window.as_ref());
                for request in pending {
                    if !request.paths.is_empty() {
                        queue_opened_markdown_paths(&app, request.paths);
                    }
                }
                let _show_result = window.show();
                let _focus_result = window.set_focus();
            }
            Err(error) => {
                for request in pending {
                    if !request.paths.is_empty() {
                        queue_opened_markdown_paths(&app, request.paths);
                    }
                }
                eprintln!("QingYu main window creation failed: {error}");
            }
        }
    });
}

fn editor_window_urls_for_opened_markdown_paths(paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .filter_map(|path| {
            let opened_path = Path::new(path);
            if opened_path.is_file() {
                return Some(editor_window_url_for_path(path));
            }

            None
        })
        .collect()
}

fn opened_paths_require_primary_notebook_switch(paths: &[String]) -> bool {
    paths.iter().any(|path| Path::new(path).is_dir())
}

fn reveal_or_open_markdown_paths<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    paths: Vec<String>,
    reveal_when_empty: bool,
) {
    if paths.is_empty() && !reveal_when_empty {
        return;
    }

    if app.get_webview_window("main").is_some() {
        queue_opened_markdown_paths(app, paths);
        focus_main_window(app);
        return;
    }

    if opened_paths_require_primary_notebook_switch(&paths) {
        queue_opened_markdown_paths(app, paths);
        spawn_configured_main_window(
            app.clone(),
            DesktopUiPromotionRequest {
                paths: Vec::new(),
                reveal_when_empty: true,
            },
        );
        return;
    }

    let urls = editor_window_urls_for_opened_markdown_paths(&paths);
    if urls.is_empty() {
        spawn_configured_main_window(
            app.clone(),
            DesktopUiPromotionRequest {
                paths: Vec::new(),
                reveal_when_empty: true,
            },
        );
        return;
    }

    for url in urls {
        spawn_editor_window(app.clone(), url);
    }
}

fn promote_normal_ui<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    paths: Vec<String>,
    reveal_when_empty: bool,
) {
    if let Err(error) = activate_normal_ui(app) {
        eprintln!("{error}");
    }
    let _ = submit_normal_ui_promotion(app, paths, reveal_when_empty);
}

fn submit_normal_ui_promotion<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    paths: Vec<String>,
    reveal_when_empty: bool,
) -> Result<(), String> {
    let Some(state) = app.try_state::<DesktopUiPromotionState>() else {
        return Err("QingYu UI promotion state is unavailable".to_string());
    };
    let request = DesktopUiPromotionRequest {
        paths,
        reveal_when_empty,
    };
    if let Some(request) = state.submit(request) {
        reveal_or_open_markdown_paths(app, request.paths, request.reveal_when_empty);
    }
    Ok(())
}

pub(crate) fn promote_normal_ui_for_confirmation<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<(), String> {
    activate_normal_ui(app)?;
    submit_normal_ui_promotion(app, Vec::new(), true)
}

fn mark_ui_promotion_setup_ready<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let Some(state) = app.try_state::<DesktopUiPromotionState>() else {
        return;
    };
    for request in state.mark_setup_ready() {
        promote_normal_ui(app, request.paths, request.reveal_when_empty);
    }
}

#[tauri::command]
pub(crate) fn request_primary_notebook_switch(
    app: tauri::AppHandle,
    path: String,
) -> Result<(), String> {
    let folder = crate::markdown_files::open::resolve_markdown_folder(path)?;
    promote_normal_ui(&app, vec![folder], false);
    Ok(())
}

fn show_main_window_if_hidden<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        if window.is_visible().unwrap_or(false) {
            return;
        }

        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn has_visible_editor_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> bool {
    app.webview_windows().values().any(|window| {
        is_editor_window_label(window.label()) && window.is_visible().unwrap_or(false)
    })
}

fn spawn_startup_window_reveal_fallback<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let app = app.clone();

    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(
            STARTUP_WINDOW_NATIVE_REVEAL_FALLBACK_MS,
        ));
        show_main_window_if_hidden(&app);
    });
}

pub(crate) fn run() {
    let launch_mode = desktop_launch_mode(std::env::args_os());
    let mut context = tauri::generate_context!();
    if launch_mode == DesktopLaunchMode::McpService {
        let mut main_windows = context
            .config_mut()
            .app
            .windows
            .iter_mut()
            .filter(|window| window.label == "main");
        let main = main_windows
            .next()
            .expect("MCP service requires one configured main window");
        assert!(
            main_windows.next().is_none(),
            "MCP service requires one configured main window"
        );
        main.create = false;
    }

    let builder = tauri::Builder::default()
        .manage(MarkdownFileWatcherState::default())
        .manage(MarkdownTreeWatcherState::default())
        .manage(MarkdownTreeLoadState::default())
        .manage(OpenedMarkdownPathsState::default())
        .manage(NativeApplicationMenuState::default())
        .manage(NativeMenuTargetState::default())
        .manage(EditorWindowRestoreState::default())
        .manage(DesktopUiPromotionState::default())
        .manage(crate::themes::ThemeActivationState::default());

    #[cfg(any(target_os = "macos", windows, target_os = "linux"))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
        let launch_mode = desktop_launch_mode(&args);
        if !should_reveal_single_instance(launch_mode) {
            return;
        }
        promote_normal_ui(
            app,
            opened_markdown_paths_from_args_with_cwd(args, std::path::PathBuf::from(cwd)),
            true,
        );
    }));

    #[cfg(any(target_os = "macos", windows, target_os = "linux"))]
    let builder = builder.plugin(
        tauri_plugin_window_state::Builder::default()
            .with_state_flags(window_state_restore_flags())
            .build(),
    );

    builder
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_log::Builder::new()
                .max_file_size(DESKTOP_LOG_MAX_FILE_SIZE_BYTES)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepSome(
                    DESKTOP_LOG_ARCHIVED_FILE_COUNT,
                ))
                .build(),
        )
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(move |app| {
            let bootstrap = crate::kernel_bootstrap::NativeKernelBootstrapOwner::new();
            app.manage(bootstrap.clone());
            crate::runtime_store::install_desktop_runtime_store(&app.handle())
                .map_err(std::io::Error::other)?;
            let startup_language =
                crate::language::resolve_startup_language(&app.config().identifier);
            let settings_owner = crate::app_settings::KernelSettingsOwner::install(&app.handle())
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            settings_owner
                .initialize_startup_language(startup_language.as_code())
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            app.manage(settings_owner);
            if let Err(error) = crate::themes::initialize_catalog_before_kernel(&app.handle()) {
                eprintln!("QingYu theme catalog initialization failed: {error}");
            }
            let runtime = Arc::new(
                crate::desktop_kernel_runtime::DesktopKernelRuntimeState::new(
                    bootstrap,
                    crate::desktop_kernel_runtime::DesktopKernelStartupStatus::Resolving,
                ),
            );
            app.manage(runtime.clone());
            let mcp_state = mcp::initialize(&app.handle()).map_err(std::io::Error::other)?;
            app.manage(mcp_state);
            let renderer_origin =
                configured_main_renderer_origin(&app.handle()).map_err(std::io::Error::other)?;
            let startup_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) =
                    resolve_and_start_desktop_kernel(startup_app, renderer_origin, runtime).await
                {
                    eprintln!("QingYu Kernel host initialization failed: {error}");
                }
            });
            if launch_mode == DesktopLaunchMode::McpService {
                #[cfg(target_os = "macos")]
                app.set_activation_policy(tauri::ActivationPolicy::Prohibited);
            } else {
                apply_main_window_chrome(app);
                spawn_startup_window_reveal_fallback(&app.handle());
                if let Some(window) = app.get_webview_window("main") {
                    remember_native_menu_webview_window(&window);
                }
            }
            mark_ui_promotion_setup_ready(&app.handle());
            if launch_mode == DesktopLaunchMode::Normal {
                let paths = opened_markdown_paths_from_args(std::env::args());
                promote_normal_ui(&app.handle(), paths, false);
            }
            Ok(())
        })
        .on_page_load(|webview, _| {
            apply_webview_window_chrome(webview);
        })
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::Destroyed) {
                if let Err(error) = crate::themes::release_theme_activation_for_window(
                    &window.app_handle(),
                    window.label(),
                ) {
                    eprintln!("Theme activation cleanup failed: {error}");
                }
            }
            remember_native_menu_window_from_event(window, event);
            apply_native_application_menu_for_window_event(window, event);
            apply_window_event_chrome(window, event);
            apply_settings_window_lifecycle(&window.app_handle(), window, event);
            remove_editor_window_restore_state(window, event);
        })
        .menu(create_application_menu)
        .on_menu_event(|app, event| {
            let command = event.id().as_ref();
            if is_native_new_window_command(command) {
                spawn_blank_editor_window(app.clone());
                return;
            }

            let Some(payload) = native_menu_command_from_id(app, command) else {
                return;
            };

            emit_native_menu_command_payload(app, payload);
        })
        .invoke_handler(guarded_desktop_invoke_handler::<tauri::Wry, _>(
            tauri::generate_handler![
                crate::kernel_bootstrap::read_native_kernel_bootstrap,
                read_desktop_kernel_startup_state,
                initialize_desktop_kernel_workspace,
                switch_desktop_kernel_workspace,
                retry_desktop_kernel_workspace,
                crate::mcp::get_mcp_settings,
                crate::mcp::update_mcp_settings,
                crate::mcp::get_mcp_health,
                crate::mcp::list_mcp_audit_entries,
                crate::mcp::clear_mcp_audit_entries,
                crate::app_settings::read_app_settings_group,
                crate::app_settings::write_app_settings_group,
                crate::app_settings::replace_portable_app_settings,
                crate::app_settings::read_exposed_app_settings,
                crate::app_settings::patch_exposed_app_settings,
                crate::runtime_store::load_desktop_runtime_store,
                crate::runtime_store::get_desktop_runtime_store_value,
                crate::runtime_store::commit_desktop_runtime_store_changes,
                crate::primary_workspace::read_primary_workspace_state,
                crate::primary_workspace::write_primary_workspace_state,
                crate::primary_workspace::prepare_desktop_notebook_target,
                crate::primary_workspace::discard_prepared_desktop_notebook_target,
                crate::themes::list_themes,
                crate::themes::read_theme_css,
                crate::themes::activation::prepare_theme_activation,
                crate::themes::activation::commit_theme_activation,
                crate::themes::activation::cancel_theme_activation,
                crate::themes::activation::release_theme_activation,
                crate::themes::import_theme_file,
                crate::themes::replace_theme_file,
                crate::themes::delete_theme,
                crate::themes::theme_directory_path,
                crate::markdown_files::tree::list_markdown_files_for_path,
                crate::markdown_files::tree::list_markdown_reference_files_for_path,
                crate::markdown_files::tree::load_markdown_files_for_path,
                crate::markdown_files::tree::cancel_markdown_files_load,
                crate::markdown_files::search::search_markdown_files_for_path,
                crate::markdown_files::tree::create_markdown_tree_file,
                crate::markdown_files::tree::create_markdown_tree_folder,
                crate::menu::install_application_menu,
                crate::menu::show_native_app_about,
                crate::markdown_files::tree::rename_markdown_tree_file,
                crate::markdown_files::tree::move_markdown_tree_file,
                crate::markdown_files::tree::delete_markdown_tree_file,
                crate::markdown_files::asset_cleanup::trash_markdown_assets,
                crate::markdown_files::open::open_markdown_file_in_new_window,
                crate::markdown_files::open::open_markdown_folder_in_new_window,
                crate::markdown_files::open::open_containing_folder,
                crate::markdown_files::open::open_markdown_attachment,
                crate::markdown_files::open::resolve_markdown_path,
                crate::markdown_files::open::resolve_markdown_folder,
                crate::markdown_files::resource::resolve_workspace_resource_root,
                crate::markdown_files::resource::trash_workspace_resources,
                crate::markdown_files::document::read_markdown_file,
                crate::markdown_files::standalone::read_standalone_document,
                crate::markdown_files::standalone::write_standalone_document_cas,
                crate::text_file::read_text_file,
                crate::markdown_files::history::list_markdown_file_history,
                crate::markdown_files::history::read_markdown_file_history,
                crate::markdown_files::attachment::import_local_file,
                crate::markdown_files::image::read_local_image_file,
                crate::markdown_files::template::read_markdown_template_file,
                crate::markdown_files::template::write_markdown_template_file,
                crate::markdown_files::template::delete_markdown_template_file,
                crate::markdown_files::attachment::save_clipboard_attachment,
                crate::markdown_files::image::save_clipboard_image,
                crate::markdown_files::path::canonical_local_file_path,
                crate::clipboard::read_clipboard_text,
                crate::windows::minimize_current_window,
                crate::windows::open_blank_editor_window,
                crate::windows::open_settings_window,
                crate::windows::mark_settings_window_ready,
                crate::windows::hide_settings_window,
                crate::windows::acknowledge_settings_window_hide,
                crate::windows::cancel_settings_window_hide,
                crate::windows::complete_settings_window_hide,
                crate::windows::destroy_current_editor_window,
                crate::workspace_membership::is_document_in_workspace,
                crate::web_http::download_web_image,
                crate::markdown_files::document::write_markdown_file,
                crate::markdown_files::document::write_markdown_export_file,
                crate::text_file::write_text_file,
                crate::markdown_files::export::export_pdf_file,
                crate::markdown_files::export::check_pandoc_available,
                crate::markdown_files::export::detect_pandoc_path,
                crate::markdown_files::export::export_pandoc_file,
                crate::watcher::watch_markdown_file,
                crate::watcher::unwatch_markdown_file,
                crate::watcher::watch_markdown_tree,
                crate::watcher::unwatch_markdown_tree,
                request_primary_notebook_switch,
                crate::opened_files::take_opened_markdown_paths,
                crate::shell_command::get_shell_command_status,
                crate::shell_command::install_shell_command,
                crate::shell_command::uninstall_shell_command,
                crate::window_state::set_editor_window_restore_state,
                crate::window_state::list_editor_window_restore_states,
                crate::fonts::list_system_font_families,
                crate::sync_config::load_sync_config_editing,
                crate::sync_config::set_sync_config_editing,
                crate::sync_config::request_sync_config_apply,
                crate::sync_config::cancel_sync_config_apply,
                crate::sync_config::settle_kernel_sync_config_apply,
                crate::managed_workspace::resolve_managed_workspace_root,
                crate::managed_workspace::list_managed_workspace_names,
                crate::app_logs::open_log_folder,
            ],
        ))
        .build(context)
        .expect("error while building QingYu")
        .run(move |app, event| match event {
            tauri::RunEvent::ExitRequested { code, api, .. } => {
                handle_app_exit_requested(app, code, api);
            }
            tauri::RunEvent::Exit => {
                if let Some(state) = app.try_state::<std::sync::Arc<mcp::McpState>>() {
                    tauri::async_runtime::block_on(async move {
                        let _shutdown_result = state.shutdown().await;
                    });
                }
                if let Some(runtime) =
                    app.try_state::<Arc<crate::desktop_kernel_runtime::DesktopKernelRuntimeState>>()
                {
                    if let Some(owner) = runtime.take_owner_for_shutdown() {
                        tauri::async_runtime::block_on(async move {
                            if let Err(error) = owner.stop().await {
                                eprintln!("QingYu Kernel graceful shutdown failed: {error:?}");
                            }
                        });
                    }
                }
            }
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Opened { urls } => {
                promote_normal_ui(app, opened_markdown_paths_from_urls(&urls), false);
            }
            #[cfg(any(target_os = "ios", target_os = "android"))]
            tauri::RunEvent::Opened { urls } => {
                queue_opened_markdown_paths(app, opened_markdown_paths_from_urls(&urls));
            }
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { .. } => {
                // Settings may stay visible after prewarm. Treating that as an editor would skip
                // workspace restore when the user reopens QingYu from the Dock.
                if !has_visible_editor_window(app) {
                    promote_normal_ui(app, Vec::new(), true);
                }
            }
            _ => {}
        });
}

#[cfg(test)]
mod tests {
    use crate::primary_workspace::{
        DesktopPrimaryWorkspaceResolution, DesktopPrimaryWorkspaceResolutionError,
    };

    #[test]
    fn mcp_serve_selects_headless_service_mode() {
        assert_eq!(
            super::test_desktop_launch_mode(&["qingyu", "mcp", "serve"]),
            "mcp-service"
        );
    }

    #[test]
    fn ordinary_launch_selects_normal_mode() {
        assert_eq!(super::test_desktop_launch_mode(&["qingyu"]), "normal");
        assert_eq!(
            super::test_desktop_launch_mode(&["qingyu", "mcp", "serve", "unexpected"]),
            "normal"
        );
    }

    #[test]
    fn published_unknown_initialization_accepts_only_the_requested_authoritative_root() {
        let roots = tempfile::tempdir().expect("temporary workspace roots");
        let requested = roots.path().join("requested");
        let other = roots.path().join("other");
        std::fs::create_dir(&requested).expect("requested workspace");
        std::fs::create_dir(&other).expect("other workspace");
        let requested = requested.canonicalize().expect("canonical requested root");
        let other = other.canonicalize().expect("canonical other root");

        assert_eq!(
            super::recover_published_desktop_workspace_initialization(
                &requested,
                Ok(DesktopPrimaryWorkspaceResolution::Selected(
                    requested.clone()
                )),
            )
            .expect("matching published target"),
            requested,
        );
        assert!(super::recover_published_desktop_workspace_initialization(
            &requested,
            Ok(DesktopPrimaryWorkspaceResolution::Selected(other)),
        )
        .is_err());
        assert!(super::recover_published_desktop_workspace_initialization(
            &requested,
            Ok(DesktopPrimaryWorkspaceResolution::Unselected),
        )
        .is_err());
        assert!(super::recover_published_desktop_workspace_initialization(
            &requested,
            Err(DesktopPrimaryWorkspaceResolutionError::Unavailable),
        )
        .is_err());
    }

    #[test]
    fn renderer_origin_keeps_exact_scheme_and_authority_only() {
        assert_eq!(
            super::desktop_renderer_origin(
                &tauri::Url::parse("http://127.0.0.1:1420/editor?draft=1#selection").unwrap()
            )
            .unwrap(),
            "http://127.0.0.1:1420"
        );
        assert_eq!(
            super::desktop_renderer_origin(
                &tauri::Url::parse("tauri://localhost/settings").unwrap()
            )
            .unwrap(),
            "tauri://localhost"
        );
        assert_eq!(
            super::desktop_renderer_origin(
                &tauri::Url::parse("http://tauri.localhost/index.html").unwrap()
            )
            .unwrap(),
            "http://tauri.localhost"
        );
        assert_eq!(
            super::desktop_renderer_origin(&tauri::Url::parse("http://[::1]:1420/editor").unwrap())
                .unwrap(),
            "http://[::1]:1420"
        );
    }

    #[test]
    fn renderer_origin_requires_the_main_caller_at_the_configured_origin() {
        let configured = "http://127.0.0.1:1420";
        assert_eq!(
            super::validated_desktop_renderer_origin(
                "main",
                configured,
                &tauri::Url::parse("http://127.0.0.1:1420/editor?draft=1").unwrap(),
            )
            .unwrap(),
            configured
        );
        assert!(super::validated_desktop_renderer_origin(
            "settings",
            configured,
            &tauri::Url::parse("http://127.0.0.1:1420/settings").unwrap(),
        )
        .is_err());
        assert!(super::validated_desktop_renderer_origin(
            "main",
            configured,
            &tauri::Url::parse("https://attacker.example/").unwrap(),
        )
        .is_err());
    }

    #[test]
    fn configured_renderer_origin_is_platform_and_build_mode_exact() {
        let dev = tauri::Url::parse("http://127.0.0.1:1420/app").unwrap();
        let external = tauri::Url::parse("https://desktop.example/app").unwrap();
        assert_eq!(
            super::configured_desktop_renderer_origin(true, Some(&dev), None, false, false)
                .unwrap(),
            "http://127.0.0.1:1420"
        );
        assert_eq!(
            super::configured_desktop_renderer_origin(false, None, Some(&external), false, false,)
                .unwrap(),
            "https://desktop.example"
        );
        assert_eq!(
            super::configured_desktop_renderer_origin(false, None, None, false, false).unwrap(),
            "tauri://localhost"
        );
        assert_eq!(
            super::configured_desktop_renderer_origin(false, None, None, true, false).unwrap(),
            "http://tauri.localhost"
        );
        assert_eq!(
            super::configured_desktop_renderer_origin(false, None, None, true, true).unwrap(),
            "https://tauri.localhost"
        );
    }

    #[test]
    fn renderer_origin_rejects_opaque_hostless_and_credentialed_urls() {
        assert!(
            super::desktop_renderer_origin(&tauri::Url::parse("file:///index.html").unwrap())
                .is_err()
        );
        assert!(super::desktop_renderer_origin(
            &tauri::Url::parse("http://user@127.0.0.1:1420/").unwrap()
        )
        .is_err());
    }

    #[test]
    fn service_single_instance_invocation_does_not_reveal_window() {
        assert!(!super::test_should_reveal_single_instance("mcp-service"));
    }

    #[test]
    fn ordinary_single_instance_invocation_reveals_window() {
        assert!(super::test_should_reveal_single_instance("normal"));
    }

    #[test]
    fn normal_ui_requests_queue_until_setup_and_deduplicate_window_creation() {
        let state = super::DesktopUiPromotionState::default();
        let first = super::DesktopUiPromotionRequest {
            paths: vec!["first.md".to_owned()],
            reveal_when_empty: true,
        };
        assert!(state.submit(first.clone()).is_none());
        assert_eq!(state.mark_setup_ready().len(), 1);

        assert!(state.begin_creation(first));
        assert!(!state.begin_creation(super::DesktopUiPromotionRequest {
            paths: vec!["second.md".to_owned()],
            reveal_when_empty: true,
        }));
        let pending = state.finish_creation();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].paths, ["second.md"]);
    }

    #[test]
    fn poisoned_ui_promotion_state_does_not_drop_a_ready_request() {
        let state = std::sync::Arc::new(super::DesktopUiPromotionState::default());
        assert!(state.mark_setup_ready().is_empty());
        let poison = std::sync::Arc::clone(&state);
        let _ = std::thread::spawn(move || {
            let _guard = poison.inner.lock().expect("promotion state lock");
            panic!("poison promotion state for recovery coverage");
        })
        .join();

        let request = super::DesktopUiPromotionRequest {
            paths: vec!["retained.md".to_owned()],
            reveal_when_empty: false,
        };
        let submitted = state
            .submit(request)
            .expect("a poisoned state must recover instead of dropping the request");

        assert_eq!(submitted.paths, ["retained.md"]);
    }

    #[test]
    fn main_window_configuration_failure_preserves_concurrent_pending_paths() {
        let source = include_str!("desktop_runtime.rs");
        let start = source
            .find(
                "Err(error) => {\n            eprintln!(\"QingYu main window configuration failed",
            )
            .expect("configuration failure branch");
        let end = source[start..]
            .find("\n            return;")
            .map(|offset| start + offset)
            .expect("configuration failure branch end");
        let branch = &source[start..end];

        assert!(branch.contains("for request in state.finish_creation()"));
        assert!(branch.contains("queue_opened_markdown_paths(&app, request.paths)"));
    }

    #[test]
    fn dynamic_main_uses_the_preserved_config_and_focuses_only_after_build() {
        let source = include_str!("desktop_runtime.rs");
        let start = source
            .find("fn spawn_configured_main_window")
            .expect("configured main creation helper");
        let end = source[start..]
            .find("fn editor_window_urls_for_opened_markdown_paths")
            .map(|offset| start + offset)
            .expect("configured main helper boundary");
        let helper = &source[start..end];
        let from_config = helper
            .find("WebviewWindowBuilder::from_config")
            .expect("dynamic main should use its preserved WindowConfig");
        let build = helper
            .find("WebviewWindowBuilder::build")
            .expect("window build");
        let show = helper.find("window.show()").expect("window show");
        let focus = helper.find("window.set_focus()").expect("window focus");

        assert!(from_config < build);
        assert!(build < show);
        assert!(show < focus);
    }

    #[test]
    fn normal_ui_promotion_restores_dock_visibility() {
        let source = include_str!("desktop_runtime.rs");
        let activation_start = source
            .find("fn activate_normal_ui")
            .expect("normal UI activation helper should exist");
        let activation_end = source[activation_start..]
            .find("\n\n#[cfg(test)]")
            .map(|offset| activation_start + offset)
            .expect("normal UI activation helper should have a boundary");
        let activation_source = &source[activation_start..activation_end];
        let regular_policy = activation_source
            .find(".set_activation_policy(tauri::ActivationPolicy::Regular)")
            .expect("normal UI activation should restore the regular policy");
        let dock_visibility = activation_source
            .find(".set_dock_visibility(true)")
            .expect("normal UI activation should restore Dock visibility");

        assert!(regular_policy < dock_visibility);
    }

    #[test]
    fn mcp_service_runtime_preserves_main_config_but_disables_initial_creation() {
        let source = include_str!("desktop_runtime.rs");
        let build = source
            .find(".build(context)")
            .expect("desktop runtime should build with a mutable context");
        let disable = source
            .find("main.create = false;")
            .expect("MCP service mode should retain main config without creating it");
        let destructive_clear = ["context.config_mut().app.windows", ".clear();"].concat();

        assert!(disable < build);
        assert!(!source.contains(&destructive_clear));
        assert!(source.contains("DesktopLaunchMode::McpService"));
    }

    #[test]
    fn mcp_service_runtime_does_not_depend_on_renderer_ui_state() {
        let source = include_str!("desktop_runtime.rs");
        let setup_start = source
            .find(".setup(move |app| {")
            .expect("desktop setup hook should exist");
        let settings = source[setup_start..]
            .find("crate::app_settings::KernelSettingsOwner::install")
            .map(|offset| setup_start + offset)
            .expect("Kernel settings owner should delimit runtime-store setup");
        let setup_prefix = &source[setup_start..settings];

        assert!(!setup_prefix.contains("if launch_mode == DesktopLaunchMode::Normal"));
        assert!(setup_prefix.contains("install_desktop_runtime_store"));
    }

    #[test]
    fn every_desktop_launch_installs_one_kernel_runtime_before_mcp() {
        let source = include_str!("desktop_runtime.rs");
        let setup_start = source
            .find(".setup(move |app| {")
            .expect("desktop setup hook should exist");
        let setup_end = source[setup_start..]
            .find(".on_page_load")
            .map(|offset| setup_start + offset)
            .expect("desktop setup hook should have a boundary");
        let setup = &source[setup_start..setup_end];
        let kernel = setup
            .find("app.manage(runtime.clone());")
            .expect("Desktop Kernel runtime must be managed for every launch mode");
        let mcp = setup
            .find("mcp::initialize(&app.handle())")
            .expect("MCP must attach to the managed child Kernel runtime");

        assert!(kernel < mcp);
        assert!(!setup.contains("install_production_graph"));
    }

    #[test]
    fn macos_mcp_service_cannot_become_the_active_application() {
        let source = include_str!("desktop_runtime.rs");
        let setup_start = source
            .find(".setup(move |app| {")
            .expect("desktop setup hook should exist");
        let setup_end = source[setup_start..]
            .find(".on_page_load")
            .map(|offset| setup_start + offset)
            .expect("desktop setup hook should have a boundary");
        let setup_source = &source[setup_start..setup_end];

        assert!(setup_source
            .contains("app.set_activation_policy(tauri::ActivationPolicy::Prohibited);"));
        assert!(!setup_source
            .contains("app.set_activation_policy(tauri::ActivationPolicy::Accessory);"));
        let prohibited = setup_source
            .find("app.set_activation_policy(tauri::ActivationPolicy::Prohibited);")
            .expect("headless activation policy");
        let ready = setup_source
            .find("mark_ui_promotion_setup_ready")
            .expect("UI promotion inbox ready edge");
        assert!(prohibited < ready);
    }

    #[test]
    fn exposes_native_command_classification_from_menu_module() {
        assert!(crate::menu::is_frontend_menu_command("saveDocument"));
        assert!(crate::menu::is_frontend_menu_command("openSettings"));
        assert!(crate::menu::is_native_new_window_command("newDocument"));
    }

    #[test]
    fn bundle_declares_markdown_file_associations() {
        for (platform, source) in [
            ("macOS", include_str!("../tauri.macos.conf.json")),
            ("Windows", include_str!("../tauri.windows.conf.json")),
            ("Linux", include_str!("../tauri.linux.conf.json")),
        ] {
            let config: serde_json::Value = serde_json::from_str(source)
                .unwrap_or_else(|error| panic!("{platform} Tauri config should be valid: {error}"));
            let associations = config
                .pointer("/bundle/fileAssociations")
                .and_then(serde_json::Value::as_array)
                .unwrap_or_else(|| panic!("{platform} bundle should declare file associations"));
            let markdown_association = associations
                .iter()
                .find(|association| {
                    association
                        .pointer("/ext")
                        .and_then(serde_json::Value::as_array)
                        .is_some_and(|extensions| {
                            extensions
                                .iter()
                                .any(|extension| extension.as_str() == Some("md"))
                                && extensions
                                    .iter()
                                    .any(|extension| extension.as_str() == Some("markdown"))
                        })
                })
                .unwrap_or_else(|| {
                    panic!("Markdown extensions should be associated on {platform}")
                });

            assert_eq!(
                markdown_association
                    .pointer("/role")
                    .and_then(serde_json::Value::as_str),
                Some("Editor")
            );
        }
    }

    #[test]
    fn desktop_registers_window_state_restore_plugin() {
        let manifest = include_str!("../Cargo.toml");
        assert!(
            manifest.contains("tauri-plugin-window-state"),
            "desktop manifest should include the window state plugin"
        );

        let lib_source = include_str!("desktop_runtime.rs");
        assert!(
            lib_source.contains("tauri_plugin_window_state::Builder::default()")
                && lib_source.contains(".with_state_flags(window_state_restore_flags())"),
            "Tauri builder should register the window state restore plugin"
        );
    }

    #[test]
    fn desktop_window_state_restore_does_not_auto_show_window() {
        let flags = super::window_state_restore_flags();

        assert!(
            !flags.contains(tauri_plugin_window_state::StateFlags::VISIBLE),
            "window-state should not restore visibility before the frontend startup reveal"
        );
    }

    #[test]
    fn desktop_window_state_restore_does_not_restore_decorations() {
        let flags = super::window_state_restore_flags();

        assert!(
            !flags.contains(tauri_plugin_window_state::StateFlags::DECORATIONS),
            "window-state should not restore old native decorations over the configured window chrome"
        );
    }

    #[test]
    fn desktop_registers_native_startup_window_reveal_fallback() {
        let lib_source = include_str!("desktop_runtime.rs");
        let fallback_registration =
            ["spawn_startup_window", "_reveal_fallback(&app.handle())"].concat();

        assert!(
            lib_source.contains(&fallback_registration),
            "Tauri setup should register a native startup reveal fallback so hidden dev windows cannot stay Dock-only"
        );
    }

    #[test]
    fn cli_opened_directories_are_queued_for_the_primary_notebook_switch() {
        let root = std::env::temp_dir().join(format!(
            "qingyu-cli-window-fallback-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("test folder should be created");
        let markdown_file = root.join("notes.md");
        std::fs::write(&markdown_file, "# Notes").expect("markdown file should be created");

        let urls = super::editor_window_urls_for_opened_markdown_paths(&[
            root.to_string_lossy().to_string(),
            markdown_file.to_string_lossy().to_string(),
        ]);

        assert_eq!(
            urls,
            vec![crate::windows::editor_window_url_for_path(
                &markdown_file.to_string_lossy()
            )]
        );
        assert!(super::opened_paths_require_primary_notebook_switch(&[
            root.to_string_lossy().to_string(),
            markdown_file.to_string_lossy().to_string(),
        ]));
        assert!(!super::opened_paths_require_primary_notebook_switch(&[
            markdown_file.to_string_lossy().to_string(),
        ]));

        std::fs::remove_dir_all(root).expect("test folder should be removed");
    }

    #[test]
    fn desktop_reveals_initial_cli_opened_paths_natively() {
        let lib_source = include_str!("desktop_runtime.rs");

        assert!(
            lib_source.contains("promote_normal_ui(&app.handle(), paths, false);"),
            "initial CLI-opened paths should trigger a native window reveal instead of only being queued"
        );
    }

    #[test]
    fn macos_open_events_use_the_restore_capable_reveal_route() {
        let lib_source = include_str!("desktop_runtime.rs");
        let run_event_start = lib_source
            .find(".run(move |app, event| match event")
            .expect("desktop run-event handler should exist");
        let run_event_source = &lib_source[run_event_start..];
        let macos_open_arm = run_event_source
            .find("#[cfg(target_os = \"macos\")]\n            tauri::RunEvent::Opened { urls } =>")
            .expect("macOS should own a dedicated opened-URL arm");
        let mobile_open_arm = run_event_source
            .find("#[cfg(any(target_os = \"ios\", target_os = \"android\"))]")
            .expect("mobile opened-URL arm should exist");
        let opened_source = &run_event_source[macos_open_arm..mobile_open_arm];

        assert!(opened_source.contains("promote_normal_ui("));
        assert!(opened_source.contains("opened_markdown_paths_from_urls(&urls)"));
        assert!(!opened_source.contains("queue_opened_markdown_paths("));
    }

    #[test]
    fn external_notebook_switch_requests_use_the_durable_reveal_route() {
        let lib_source = include_str!("desktop_runtime.rs");
        let start = lib_source
            .find("pub(crate) fn request_primary_notebook_switch")
            .expect("desktop runtime should expose a primary notebook switch command");
        let end = lib_source[start..]
            .find("fn show_main_window_if_hidden")
            .map(|offset| start + offset)
            .expect("request command should end before the next desktop helper");
        let command_source = &lib_source[start..end];

        assert!(command_source.contains("resolve_markdown_folder(path)?"));
        assert!(command_source.contains("promote_normal_ui(&app, vec![folder], false);"));
    }

    #[test]
    fn directory_reveal_queues_the_switch_before_spawning_the_primary_window() {
        let lib_source = include_str!("desktop_runtime.rs");
        let branch_start = lib_source
            .find("if opened_paths_require_primary_notebook_switch(&paths) {")
            .expect("directory reveal branch should exist");
        let branch_end = lib_source[branch_start..]
            .find("let urls = editor_window_urls_for_opened_markdown_paths")
            .map(|offset| branch_start + offset)
            .expect("directory reveal branch should end before file-only routing");
        let branch_source = &lib_source[branch_start..branch_end];
        let queue = branch_source
            .find("queue_opened_markdown_paths(app, paths);")
            .expect("directory switch should be durably queued");
        let spawn = branch_source
            .find("spawn_configured_main_window(")
            .expect("directory switch should reveal a primary window");

        assert!(
            queue < spawn,
            "the primary renderer must not start before its directory switch is queued"
        );
    }

    #[test]
    fn empty_app_reopen_uses_restorable_editor_window() {
        let lib_source = include_str!("desktop_runtime.rs");
        let start = lib_source
            .find("fn reveal_or_open_markdown_paths")
            .expect("reveal_or_open_markdown_paths should exist");
        let end = lib_source[start..]
            .find("fn promote_normal_ui")
            .map(|offset| start + offset)
            .expect("reveal_or_open_markdown_paths should have a promotion boundary");
        let reveal_source = &lib_source[start..end];

        assert!(
            reveal_source.contains("spawn_configured_main_window("),
            "reopening QingYu without a live main window should create a restore-capable editor window"
        );
        assert!(
            !reveal_source.contains("spawn_blank_editor_window(app.clone());"),
            "empty app reopen should not use index.html?blank=1 because that skips workspace restore"
        );
    }

    #[test]
    fn desktop_handles_macos_reopen_without_visible_windows() {
        let lib_source = include_str!("desktop_runtime.rs");
        let reopen_event = ["tauri::RunEvent::", "Reopen {"].concat();
        let empty_reveal = ["promote_normal_ui(app, Vec::new(), ", "true);"].concat();

        assert!(
            lib_source.contains(&reopen_event),
            "macOS Dock reopen should be handled when all editor windows are closed"
        );
        assert!(
            lib_source.contains("if !has_visible_editor_window(app) {"),
            "reopen handling should only create a window when no editor window is visible"
        );
        assert!(
            lib_source.contains(&empty_reveal),
            "macOS Dock reopen should use the restore-capable empty reveal path"
        );
    }

    #[test]
    fn desktop_reopen_ignores_visible_settings_windows() {
        let lib_source = include_str!("desktop_runtime.rs");
        let generic_visible_window_guard = ["if !has", "_visible_windows {"].concat();

        assert!(
            lib_source.contains("if !has_visible_editor_window(app) {"),
            "macOS Dock reopen should restore an editor when the only visible window is Settings"
        );
        assert!(
            !lib_source.contains(&generic_visible_window_guard),
            "macOS Dock reopen should not treat visible Settings windows as visible editor windows"
        );
    }

    #[test]
    fn desktop_registers_native_about_command() {
        let lib_source = include_str!("desktop_runtime.rs");
        let command_name = ["show", "_native_app", "_about"].concat();
        let registration = format!("{command_name},");
        let handler_source = &lib_source[lib_source
            .find("tauri::generate_handler![")
            .expect("Tauri invoke handler should be registered")..];

        assert!(
            handler_source.contains(&registration),
            "Windows self-drawn app menu should be able to open the system-native About panel"
        );
    }

    #[test]
    fn desktop_registers_only_kernel_sync_host_coordination_commands() {
        let lib_source = include_str!("desktop_runtime.rs");
        let handler_start = lib_source
            .find("tauri::generate_handler![")
            .expect("Tauri invoke handler should be registered");
        let handler_source = &lib_source[handler_start
            ..lib_source[handler_start..]
                .find("],\n        ))")
                .map(|offset| handler_start + offset)
                .expect("Tauri invoke handler should be closed")];

        for command in [
            "load_sync_config_editing",
            "set_sync_config_editing",
            "request_sync_config_apply",
            "cancel_sync_config_apply",
            "settle_kernel_sync_config_apply",
        ] {
            assert!(
                handler_source.contains(&format!("{command},")),
                "desktop invoke handler should register {command}"
            );
        }
        for forbidden in [
            "sync_application",
            "test_sync_connection",
            "bind_dejavu_repository",
            "acknowledge_path_guard",
        ] {
            assert!(!handler_source.contains(forbidden));
        }
    }

    #[test]
    fn desktop_registers_typed_app_settings_commands() {
        let lib_source = include_str!("desktop_runtime.rs");
        let handler_source = &lib_source[lib_source
            .find("tauri::generate_handler![")
            .expect("Tauri invoke handler should be registered")..];

        for command in [
            "read_app_settings_group",
            "write_app_settings_group",
            "replace_portable_app_settings",
            "read_exposed_app_settings",
            "patch_exposed_app_settings",
        ] {
            assert!(
                handler_source.contains(&format!("{command},")),
                "desktop invoke handler should register {command}"
            );
        }
    }

    #[test]
    fn desktop_registers_single_instance_plugin_before_other_plugins() {
        let manifest = include_str!("../Cargo.toml");
        assert!(
            manifest.contains("tauri-plugin-single-instance"),
            "desktop manifest should include the single instance plugin"
        );

        let lib_source = include_str!("desktop_runtime.rs");
        let single_instance_index = lib_source
            .find("tauri_plugin_single_instance::init")
            .expect("Tauri builder should register the single instance plugin");
        let store_plugin_index = lib_source
            .find("tauri_plugin_store::Builder")
            .expect("Tauri builder should register the store plugin");

        assert!(
            single_instance_index < store_plugin_index,
            "single instance plugin should be registered before other plugins"
        );
    }

    #[test]
    fn desktop_log_files_have_bounded_rotation() {
        let lib_source = include_str!("desktop_runtime.rs");
        let max_size_constant = [
            "const DESKTOP_LOG_MAX",
            "_FILE_SIZE_BYTES: u128 = 2 * 1024 * 1024;",
        ]
        .concat();
        let max_count_constant = ["const DESKTOP_LOG_MAX", "_FILE_COUNT: usize = 5;"].concat();
        let archive_count_constant = [
            "const DESKTOP_LOG_ARCHIVED",
            "_FILE_COUNT: usize = DESKTOP_LOG_MAX_FILE_COUNT - 1;",
        ]
        .concat();
        let max_file_size_call = [".max", "_file_size(DESKTOP_LOG_MAX_FILE_SIZE_BYTES)"].concat();
        let rotation_strategy_type = ["tauri_plugin_log::RotationStrategy::", "KeepSome"].concat();
        let archived_count_name = ["DESKTOP_LOG_ARCHIVED", "_FILE_COUNT"].concat();

        assert_eq!(super::DESKTOP_LOG_MAX_FILE_SIZE_BYTES, 2 * 1024 * 1024);
        assert_eq!(super::DESKTOP_LOG_MAX_FILE_COUNT, 5);
        assert_eq!(super::DESKTOP_LOG_ARCHIVED_FILE_COUNT, 4);
        assert!(
            lib_source.contains(&max_size_constant),
            "desktop file logs should use a conservative 2MB per-file limit"
        );
        assert!(
            lib_source.contains(&max_count_constant),
            "desktop file logs should cap total retained log files"
        );
        assert!(
            lib_source.contains(&archive_count_constant),
            "desktop archived log file count should reserve one slot for the active log file"
        );
        assert!(
            lib_source.contains(&max_file_size_call),
            "desktop log plugin should use the configured file size limit"
        );
        let rotation_strategy_index = lib_source
            .find(&rotation_strategy_type)
            .expect("desktop log plugin should use KeepSome rotation");
        assert!(
            lib_source[rotation_strategy_index..].contains(&archived_count_name),
            "desktop log plugin should keep only the configured number of archived files"
        );
    }
}
