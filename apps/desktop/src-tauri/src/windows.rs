use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex, OnceLock,
    },
    time::Duration,
};

use crate::{
    language::{resolve_startup_language, AppLanguage},
    menu::remember_native_menu_webview_window,
    menu_labels,
};

#[cfg(target_os = "macos")]
use std::ops::Deref;

#[cfg(target_os = "macos")]
use dispatch2::{DispatchQueue, DispatchTime};
#[cfg(target_os = "macos")]
use objc2::Message;
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSWindow, NSWindowStyleMask};
use serde_json::{Map, Value};
#[cfg(target_os = "macos")]
use tauri::TitleBarStyle;
use tauri::{
    utils::config::Color, Emitter, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder,
};

mod settings_ownership;

use settings_ownership::{
    centered_child_position, existing_settings_action, position_from_offset, relative_offset,
    ExistingSettingsAction, SettingsWindowOwnership,
};

const BLANK_EDITOR_WINDOW_LABEL_PREFIX: &str = "markra-editor-";
const BLANK_EDITOR_WINDOW_URL: &str = "index.html?blank=1";
const MAIN_WINDOW_LABEL: &str = "main";
const RESTORABLE_EDITOR_WINDOW_URL: &str = "index.html";
#[cfg(test)]
pub(crate) const MINIMIZE_CURRENT_WINDOW_COMMAND: &str = "minimize_current_window";
#[cfg(test)]
pub(crate) const OPEN_BLANK_EDITOR_WINDOW_COMMAND: &str = "open_blank_editor_window";
#[cfg(test)]
pub(crate) const OPEN_SETTINGS_WINDOW_COMMAND: &str = "open_settings_window";
const SETTINGS_WINDOW_LABEL: &str = "markra-settings";
const SETTINGS_WINDOW_URL: &str = "index.html?settings=1";
const SETTINGS_WINDOW_TARGET_EVENT: &str = "markra://settings-window-target";
const SETTINGS_WINDOW_HIDE_REQUESTED_EVENT: &str = "qingyu://settings-hide-requested";
const SETTINGS_PROJECT_CONTEXT_PARAM: &str = "settingsProjectContext";
const SETTINGS_PROJECT_ROOT_PARAM: &str = "settingsProjectRoot";
const SETTINGS_WORKSPACE_CONTEXT_PARAM: &str = "settingsWorkspaceContext";
const SETTINGS_WORKSPACE_SOURCE_PATH_PARAM: &str = "settingsWorkspaceSourcePath";
const SETTINGS_SOURCE_WINDOW_LABEL_PARAM: &str = "settingsSourceWindowLabel";
const SETTINGS_WINDOW_TARGET_EXPORT_PANDOC_PATH: &str = "exportPandocPath";
const SETTINGS_WINDOW_TARGET_SYNC: &str = "sync";
const SETTINGS_STORE_PATH: &str = "settings.json";
const SETTINGS_STARTUP_LANGUAGE_PARAM: &str = "startupLanguage";
const SETTINGS_STARTUP_APPEARANCE_MODE_PARAM: &str = "startupAppearanceMode";
const SETTINGS_STARTUP_LIGHT_THEME_PARAM: &str = "startupLightTheme";
const SETTINGS_STARTUP_DARK_THEME_PARAM: &str = "startupDarkTheme";
const SETTINGS_LEGACY_THEME_KEY: &str = "theme";
const SETTINGS_APPEARANCE_MODE_KEY: &str = "appearanceMode";
const SETTINGS_LIGHT_THEME_KEY: &str = "lightThemeId";
const SETTINGS_DARK_THEME_KEY: &str = "darkThemeId";
const SETTINGS_LEGACY_LIGHT_THEME_KEY: &str = "lightTheme";
const SETTINGS_LEGACY_DARK_THEME_KEY: &str = "darkTheme";
const SETTINGS_WINDOW_NATIVE_REVEAL_FALLBACK_MS: u64 = 1_800;
const SETTINGS_WINDOW_HIDE_FALLBACK_MS: u64 = 1_200;
const SETTINGS_WINDOW_IDLE_DESTROY_MS: u64 = 5 * 60 * 1000;
const SETTINGS_WINDOW_WIDTH: f64 = 1040.0;
const SETTINGS_WINDOW_HEIGHT: f64 = 720.0;
const SETTINGS_WINDOW_MIN_WIDTH: f64 = 860.0;
const SETTINGS_WINDOW_MIN_HEIGHT: f64 = 600.0;
const SETTINGS_WINDOW_RESIZABLE: bool = true;
const SETTINGS_WINDOW_SHADOW: bool = true;
#[cfg(target_os = "macos")]
const SETTINGS_WINDOW_HIDDEN_TITLE: bool = true;
#[cfg(target_os = "macos")]
const MACOS_FULLSCREEN_MINIMIZE_DELAY_MS: u64 = 700;

static NEXT_EDITOR_WINDOW_ID: AtomicUsize = AtomicUsize::new(1);

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct SettingsWindowOpenContext {
    project_root: Option<String>,
    source_window_label: Option<String>,
    target: Option<String>,
    workspace_source_path: Option<String>,
}

#[derive(Default)]
struct SettingsWindowRuntimeState {
    creating: bool,
    hide_acknowledged: bool,
    hide_request_generation: usize,
    hide_requested: bool,
    idle_destroy_generation: usize,
    ownership: SettingsWindowOwnership,
    pending_context: Option<SettingsWindowOpenContext>,
    ready: bool,
    show_when_ready: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SettingsWindowCreationResult {
    Canceled,
    RevealWhenReady,
}

static SETTINGS_WINDOW_RUNTIME_STATE: OnceLock<Mutex<SettingsWindowRuntimeState>> = OnceLock::new();
static SETTINGS_WINDOW_OPERATION_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn settings_window_runtime_state() -> &'static Mutex<SettingsWindowRuntimeState> {
    SETTINGS_WINDOW_RUNTIME_STATE.get_or_init(|| Mutex::new(SettingsWindowRuntimeState::default()))
}

fn settings_window_operation_lock() -> &'static Mutex<()> {
    SETTINGS_WINDOW_OPERATION_LOCK.get_or_init(|| Mutex::new(()))
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SettingsWindowStartupPreferences {
    language: AppLanguage,
    appearance_mode: String,
    light_theme: String,
    dark_theme: String,
}

impl SettingsWindowStartupPreferences {
    fn default_for_language(language: AppLanguage) -> Self {
        Self {
            language,
            appearance_mode: "system".to_string(),
            light_theme: "light".to_string(),
            dark_theme: "dark".to_string(),
        }
    }
}

impl Default for SettingsWindowStartupPreferences {
    fn default() -> Self {
        Self::default_for_language(AppLanguage::En)
    }
}

const APP_APPEARANCE_MODE_OPTIONS: &[&str] = &["system", "light", "dark"];
const LIGHT_EDITOR_THEME_OPTIONS: &[&str] = &[
    "light",
    "github",
    "one-light",
    "gothic",
    "newsprint",
    "pixyll",
    "whitey",
    "sepia",
    "solarized-light",
    "catppuccin-latte",
    "academic",
    "minimal",
    "custom",
];
const DARK_EDITOR_THEME_OPTIONS: &[&str] = &[
    "dark",
    "github-dark",
    "one-dark",
    "one-dark-pro",
    "night",
    "solarized-dark",
    "nord",
    "catppuccin-mocha",
    "custom",
];

fn current_window_chrome_platform() -> &'static str {
    std::env::consts::OS
}

fn transparent_window_chrome_for_platform(platform: &str) -> bool {
    platform == "macos"
}

fn transparent_window_background_color_for_platform(platform: &str) -> Option<Color> {
    if transparent_window_chrome_for_platform(platform) {
        return Some(Color(255, 255, 255, 0));
    }

    None
}

fn next_blank_editor_window_label() -> String {
    let id = NEXT_EDITOR_WINDOW_ID.fetch_add(1, Ordering::Relaxed);
    format!("{BLANK_EDITOR_WINDOW_LABEL_PREFIX}{id}")
}

fn is_blank_editor_window_label(label: &str) -> bool {
    label.starts_with(BLANK_EDITOR_WINDOW_LABEL_PREFIX)
}

pub(crate) fn is_editor_window_label(label: &str) -> bool {
    label == MAIN_WINDOW_LABEL || is_blank_editor_window_label(label)
}

pub(crate) fn is_settings_window_label(label: &str) -> bool {
    label == SETTINGS_WINDOW_LABEL
}

fn should_hide_native_menu_for_window_label_on_platform(platform: &str, label: &str) -> bool {
    if is_settings_window_label(label) {
        return true;
    }

    platform == "windows" && is_editor_window_label(label)
}

fn should_hide_native_menu_for_window_label(label: &str) -> bool {
    should_hide_native_menu_for_window_label_on_platform(current_window_chrome_platform(), label)
}

fn editor_window_decorations_for_platform(platform: &str) -> bool {
    platform != "windows"
}

pub(crate) fn hide_native_menu_for_settings_window<R>(window: &tauri::WebviewWindow<R>)
where
    R: tauri::Runtime,
{
    if should_hide_native_menu_for_window_label(window.label()) {
        let _ = window.hide_menu();
    }
}

pub(crate) fn hide_native_menu_for_settings_window_in_app<R>(app: &tauri::AppHandle<R>)
where
    R: tauri::Runtime,
{
    if let Some(window) = app.get_webview_window(SETTINGS_WINDOW_LABEL) {
        hide_native_menu_for_settings_window(&window);
    }
}

fn encode_url_query_component(value: &str) -> String {
    let mut encoded = String::new();

    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }

    encoded
}

#[cfg(target_os = "macos")]
fn hide_native_macos_window_controls<R>(window: &tauri::WebviewWindow<R>)
where
    R: tauri::Runtime,
{
    let Ok(ns_window) = window.ns_window() else {
        return;
    };
    schedule_hide_native_macos_window_controls(ns_window);
}

#[cfg(target_os = "macos")]
struct MainThreadSafe<T>(T);

#[cfg(target_os = "macos")]
unsafe impl<T> Send for MainThreadSafe<T> {}

#[cfg(target_os = "macos")]
impl<T> Deref for MainThreadSafe<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(not(target_os = "macos"))]
fn hide_native_macos_window_controls<R>(_window: &tauri::WebviewWindow<R>)
where
    R: tauri::Runtime,
{
}

#[cfg(target_os = "macos")]
fn hide_native_macos_window_controls_for_window<R>(window: &tauri::Window<R>)
where
    R: tauri::Runtime,
{
    let Ok(ns_window) = window.ns_window() else {
        return;
    };
    schedule_hide_native_macos_window_controls(ns_window);
}

#[cfg(target_os = "macos")]
fn schedule_hide_native_macos_window_controls(ns_window: *mut std::ffi::c_void) {
    if ns_window.is_null() {
        return;
    }

    let ns_window = ns_window as usize;

    dispatch2::run_on_main(move |_| {
        let ns_window = ns_window as *mut std::ffi::c_void;
        hide_native_macos_standard_buttons(ns_window);
    });
}

#[cfg(target_os = "macos")]
fn hide_native_macos_standard_buttons(ns_window: *mut std::ffi::c_void) {
    use objc2_app_kit::{NSWindow, NSWindowButton};

    let window = unsafe { &*ns_window.cast::<NSWindow>() };

    for button in [
        NSWindowButton::CloseButton,
        NSWindowButton::MiniaturizeButton,
        NSWindowButton::ZoomButton,
    ] {
        if let Some(button) = window.standardWindowButton(button) {
            button.setHidden(true);
        }
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn apply_webview_window_chrome<R>(webview: &tauri::Webview<R>)
where
    R: tauri::Runtime,
{
    let Ok(ns_window) = webview.window().ns_window() else {
        return;
    };
    schedule_hide_native_macos_window_controls(ns_window);
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn apply_webview_window_chrome<R>(_webview: &tauri::Webview<R>)
where
    R: tauri::Runtime,
{
}

#[cfg(target_os = "macos")]
pub(crate) fn apply_window_event_chrome<R>(window: &tauri::Window<R>, event: &tauri::WindowEvent)
where
    R: tauri::Runtime,
{
    match event {
        tauri::WindowEvent::Focused(true)
        | tauri::WindowEvent::Resized(_)
        | tauri::WindowEvent::ScaleFactorChanged { .. } => {
            hide_native_macos_window_controls_for_window(window);
        }
        _ => {}
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn apply_window_event_chrome<R>(_window: &tauri::Window<R>, _event: &tauri::WindowEvent)
where
    R: tauri::Runtime,
{
}

#[cfg(target_os = "macos")]
pub(crate) fn apply_main_window_chrome<R>(app: &tauri::App<R>)
where
    R: tauri::Runtime,
{
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        hide_native_macos_window_controls(&window);
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn apply_main_window_chrome<R>(app: &tauri::App<R>)
where
    R: tauri::Runtime,
{
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        hide_native_menu_for_settings_window(&window);
    }
}

pub(crate) fn editor_window_url_for_path(path: &str) -> String {
    format!("index.html?path={}", encode_url_query_component(path))
}

fn normalized_settings_window_target(target: Option<&str>) -> Option<&'static str> {
    match target {
        Some(SETTINGS_WINDOW_TARGET_EXPORT_PANDOC_PATH) => {
            Some(SETTINGS_WINDOW_TARGET_EXPORT_PANDOC_PATH)
        }
        Some(SETTINGS_WINDOW_TARGET_SYNC) => Some(SETTINGS_WINDOW_TARGET_SYNC),
        _ => None,
    }
}

fn append_url_query_param(url: &mut String, key: &str, value: &str) {
    url.push('&');
    url.push_str(key);
    url.push('=');
    url.push_str(&encode_url_query_component(value));
}

fn settings_store_path(identifier: &str) -> Option<PathBuf> {
    dirs::data_dir().map(|data_dir| data_dir.join(identifier).join(SETTINGS_STORE_PATH))
}

fn read_settings_object(path: &Path) -> Option<Map<String, Value>> {
    fs::read_to_string(path)
        .ok()
        .and_then(|contents| serde_json::from_str::<Value>(&contents).ok())
        .and_then(|settings| settings.as_object().cloned())
}

fn stored_settings_string<'a>(settings: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    settings.get(key).and_then(Value::as_str)
}

fn is_app_appearance_mode(value: &str) -> bool {
    APP_APPEARANCE_MODE_OPTIONS.contains(&value)
}

fn is_light_editor_theme(value: &str) -> bool {
    LIGHT_EDITOR_THEME_OPTIONS.contains(&value)
}

fn is_dark_editor_theme(value: &str) -> bool {
    DARK_EDITOR_THEME_OPTIONS.contains(&value)
}

fn is_theme_id(value: &str) -> bool {
    let mut bytes = value.bytes();
    !value.starts_with("qingyu-")
        && value.len() <= 64
        && bytes
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn legacy_theme_preferences(
    language: AppLanguage,
    theme: Option<&str>,
) -> SettingsWindowStartupPreferences {
    let mut preferences = SettingsWindowStartupPreferences::default_for_language(language);
    let Some(theme) = theme else {
        return preferences;
    };

    if theme == "system" {
        return preferences;
    }

    if is_dark_editor_theme(theme) {
        preferences.appearance_mode = "dark".to_string();
        preferences.dark_theme = theme.to_string();
        return preferences;
    }

    if is_light_editor_theme(theme) {
        preferences.appearance_mode = "light".to_string();
        preferences.light_theme = theme.to_string();
    }

    preferences
}

fn settings_window_startup_preferences(identifier: &str) -> SettingsWindowStartupPreferences {
    let language = resolve_startup_language(identifier);
    let Some(settings_path) = settings_store_path(identifier) else {
        return SettingsWindowStartupPreferences::default_for_language(language);
    };
    let Some(settings) = read_settings_object(&settings_path) else {
        return SettingsWindowStartupPreferences::default_for_language(language);
    };

    let mut preferences = legacy_theme_preferences(
        language,
        stored_settings_string(&settings, SETTINGS_LEGACY_THEME_KEY),
    );

    if let Some(appearance_mode) = stored_settings_string(&settings, SETTINGS_APPEARANCE_MODE_KEY)
        .filter(|value| is_app_appearance_mode(value))
    {
        preferences.appearance_mode = appearance_mode.to_string();
    }

    if let Some(light_theme) = stored_settings_string(&settings, SETTINGS_LIGHT_THEME_KEY)
        .or_else(|| stored_settings_string(&settings, SETTINGS_LEGACY_LIGHT_THEME_KEY))
        .filter(|value| is_theme_id(value))
    {
        preferences.light_theme = light_theme.to_string();
    }

    if let Some(dark_theme) = stored_settings_string(&settings, SETTINGS_DARK_THEME_KEY)
        .or_else(|| stored_settings_string(&settings, SETTINGS_LEGACY_DARK_THEME_KEY))
        .filter(|value| is_theme_id(value))
    {
        preferences.dark_theme = dark_theme.to_string();
    }

    preferences
}

fn settings_window_url(
    target: Option<&str>,
    project_root: Option<&str>,
    workspace_source_path: Option<&str>,
    source_window_label: Option<&str>,
    project_context: bool,
    startup_preferences: &SettingsWindowStartupPreferences,
) -> String {
    let mut url = SETTINGS_WINDOW_URL.to_string();

    append_url_query_param(
        &mut url,
        SETTINGS_STARTUP_LANGUAGE_PARAM,
        startup_preferences.language.as_code(),
    );
    append_url_query_param(
        &mut url,
        SETTINGS_STARTUP_APPEARANCE_MODE_PARAM,
        &startup_preferences.appearance_mode,
    );
    append_url_query_param(
        &mut url,
        SETTINGS_STARTUP_LIGHT_THEME_PARAM,
        &startup_preferences.light_theme,
    );
    append_url_query_param(
        &mut url,
        SETTINGS_STARTUP_DARK_THEME_PARAM,
        &startup_preferences.dark_theme,
    );

    if let Some(target) = normalized_settings_window_target(target) {
        append_url_query_param(&mut url, "settingsTarget", target);
    }
    if project_context {
        append_url_query_param(&mut url, SETTINGS_PROJECT_CONTEXT_PARAM, "1");
        if let Some(project_root) = project_root {
            append_url_query_param(&mut url, SETTINGS_PROJECT_ROOT_PARAM, project_root);
        }
        append_url_query_param(&mut url, SETTINGS_WORKSPACE_CONTEXT_PARAM, "1");
        if let Some(workspace_source_path) = workspace_source_path {
            append_url_query_param(
                &mut url,
                SETTINGS_WORKSPACE_SOURCE_PATH_PARAM,
                workspace_source_path,
            );
        }
        if let Some(source_window_label) = source_window_label {
            append_url_query_param(
                &mut url,
                SETTINGS_SOURCE_WINDOW_LABEL_PARAM,
                source_window_label,
            );
        }
    }

    url
}

fn spawn_editor_window_with_label<R>(app: tauri::AppHandle<R>, label: String, url: String)
where
    R: tauri::Runtime,
{
    // Create editor windows off the menu/reopen event thread to avoid WebView2 deadlocks on Windows.
    std::thread::spawn(move || {
        let builder = WebviewWindowBuilder::new(&app, label, WebviewUrl::App(url.into()))
            .title("")
            .inner_size(1360.0, 800.0)
            .min_inner_size(360.0, 320.0)
            .decorations(editor_window_decorations())
            .transparent(editor_window_transparent())
            .shadow(true)
            .center();

        #[cfg(target_os = "macos")]
        let builder = builder
            .title_bar_style(TitleBarStyle::Overlay)
            .hidden_title(true);

        match builder.build() {
            Ok(window) => {
                remember_native_menu_webview_window(&window);
                hide_native_macos_window_controls(&window);
                hide_native_menu_for_settings_window(&window);
            }
            Err(error) => {
                eprintln!("failed to create blank editor window: {error}");
            }
        }
    });
}

pub(crate) fn spawn_editor_window<R>(app: tauri::AppHandle<R>, url: String)
where
    R: tauri::Runtime,
{
    let label = next_blank_editor_window_label();
    debug_assert!(is_blank_editor_window_label(&label));
    spawn_editor_window_with_label(app, label, url);
}

pub(crate) fn spawn_restorable_editor_window<R>(app: tauri::AppHandle<R>)
where
    R: tauri::Runtime,
{
    // ?blank=1 deliberately opts out of workspace restore. App/Dock reopens need index.html
    // so the frontend can replay the saved tabs instead of starting an empty document.
    spawn_editor_window_with_label(
        app,
        MAIN_WINDOW_LABEL.to_string(),
        RESTORABLE_EDITOR_WINDOW_URL.to_string(),
    );
}

fn editor_window_transparent() -> bool {
    transparent_window_chrome_for_platform(current_window_chrome_platform())
}

fn editor_window_decorations() -> bool {
    editor_window_decorations_for_platform(current_window_chrome_platform())
}

#[cfg(target_os = "macos")]
fn miniaturize_macos_window(window: &NSWindow) {
    NSWindow::miniaturize(window, Some(window));
}

#[cfg(target_os = "macos")]
fn schedule_macos_window_minimize(window: &NSWindow) {
    let ns_window = MainThreadSafe(window.retain());
    let delay = DispatchTime::try_from(Duration::from_millis(MACOS_FULLSCREEN_MINIMIZE_DELAY_MS))
        .unwrap_or(DispatchTime::NOW);

    let _ = DispatchQueue::main().after(delay, move || {
        miniaturize_macos_window(&ns_window);
    });
}

#[cfg(target_os = "macos")]
fn minimize_macos_window(ns_window: *mut std::ffi::c_void) {
    if ns_window.is_null() {
        return;
    }

    let ns_window = ns_window as usize;
    dispatch2::run_on_main(move |_| {
        let ns_window = ns_window as *mut std::ffi::c_void;
        let window = unsafe { &*ns_window.cast::<NSWindow>() };

        if window.styleMask().contains(NSWindowStyleMask::FullScreen) {
            let retained_window = window.retain();
            window.toggleFullScreen(None);
            schedule_macos_window_minimize(&retained_window);
            return;
        }

        miniaturize_macos_window(window);
    });
}

#[cfg(target_os = "macos")]
fn minimize_window<R>(window: &tauri::Window<R>) -> tauri::Result<()>
where
    R: tauri::Runtime,
{
    let Ok(ns_window) = window.ns_window() else {
        return window.minimize();
    };

    minimize_macos_window(ns_window);
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn minimize_window<R>(window: &tauri::Window<R>) -> tauri::Result<()>
where
    R: tauri::Runtime,
{
    window.minimize()
}

#[tauri::command]
pub(crate) fn minimize_current_window(window: tauri::Window) -> Result<(), String> {
    minimize_window(&window).map_err(|error| error.to_string())
}

pub(crate) fn spawn_blank_editor_window<R>(app: tauri::AppHandle<R>)
where
    R: tauri::Runtime,
{
    spawn_editor_window(app, BLANK_EDITOR_WINDOW_URL.to_string());
}

#[tauri::command]
pub(crate) fn open_blank_editor_window(app: tauri::AppHandle) {
    spawn_blank_editor_window(app);
}

fn settings_window_transparent() -> bool {
    transparent_window_chrome_for_platform(current_window_chrome_platform())
}

fn settings_window_decorations() -> bool {
    editor_window_decorations()
}

fn settings_window_inner_size() -> (f64, f64) {
    (SETTINGS_WINDOW_WIDTH, SETTINGS_WINDOW_HEIGHT)
}

fn settings_window_min_inner_size() -> (f64, f64) {
    (SETTINGS_WINDOW_MIN_WIDTH, SETTINGS_WINDOW_MIN_HEIGHT)
}

fn settings_window_resizable() -> bool {
    SETTINGS_WINDOW_RESIZABLE
}

fn settings_window_shadow() -> bool {
    SETTINGS_WINDOW_SHADOW
}

fn settings_window_visible() -> bool {
    false
}

fn settings_window_resolved_appearance(
    startup_preferences: &SettingsWindowStartupPreferences,
) -> &str {
    if startup_preferences.appearance_mode == "light" {
        return "light";
    }

    "dark"
}

fn settings_window_background_color_for_preferences(
    platform: &str,
    startup_preferences: &SettingsWindowStartupPreferences,
) -> Option<Color> {
    if let Some(color) = transparent_window_background_color_for_platform(platform) {
        return Some(color);
    }

    if settings_window_resolved_appearance(startup_preferences) == "light" {
        return Some(Color(255, 255, 255, 255));
    }

    Some(Color(30, 30, 30, 255))
}

fn settings_window_title(language: AppLanguage) -> String {
    menu_labels::for_language(language)
        .settings
        .trim_end_matches('.')
        .to_string()
}

fn next_settings_window_idle_destroy_generation(state: &mut SettingsWindowRuntimeState) -> usize {
    state.idle_destroy_generation = state.idle_destroy_generation.wrapping_add(1);
    state.idle_destroy_generation
}

fn cancel_settings_window_hide_request(state: &mut SettingsWindowRuntimeState) {
    state.hide_acknowledged = false;
    state.hide_requested = false;
    state.hide_request_generation = state.hide_request_generation.wrapping_add(1);
}

fn begin_settings_window_hide_request(state: &mut SettingsWindowRuntimeState) -> Option<usize> {
    if state.hide_requested {
        return None;
    }

    state.hide_requested = true;
    state.hide_acknowledged = false;
    state.hide_request_generation = state.hide_request_generation.wrapping_add(1);
    Some(state.hide_request_generation)
}

fn acknowledge_settings_window_hide_generation(
    state: &mut SettingsWindowRuntimeState,
    generation: usize,
) -> bool {
    if !state.hide_requested || state.hide_request_generation != generation {
        return false;
    }

    state.hide_acknowledged = true;
    true
}

fn complete_settings_window_hide_request(
    state: &mut SettingsWindowRuntimeState,
    generation: usize,
) -> bool {
    if !state.hide_requested || state.hide_request_generation != generation {
        return false;
    }

    cancel_settings_window_hide_request(state);
    true
}

fn cancel_settings_window_hide_generation(
    state: &mut SettingsWindowRuntimeState,
    generation: usize,
) -> bool {
    if !state.hide_requested || state.hide_request_generation != generation {
        return false;
    }

    cancel_settings_window_hide_request(state);
    true
}

fn complete_settings_window_hide_fallback(
    state: &mut SettingsWindowRuntimeState,
    generation: usize,
) -> bool {
    if state.hide_acknowledged {
        return false;
    }
    complete_settings_window_hide_request(state, generation)
}

fn begin_settings_window_creation(
    show_when_ready: bool,
    pending_context: Option<SettingsWindowOpenContext>,
) -> bool {
    let Ok(mut state) = settings_window_runtime_state().lock() else {
        return true;
    };

    if state.creating {
        if show_when_ready {
            state.show_when_ready = true;
            state.pending_context = pending_context;
            next_settings_window_idle_destroy_generation(&mut state);
        }
        return false;
    }

    state.creating = true;
    cancel_settings_window_hide_request(&mut state);
    state.ready = false;
    state.show_when_ready = show_when_ready;
    state.pending_context = pending_context;
    next_settings_window_idle_destroy_generation(&mut state);
    true
}

fn settings_window_creation_result(
    creating: bool,
    show_when_ready: bool,
) -> SettingsWindowCreationResult {
    if creating && show_when_ready {
        return SettingsWindowCreationResult::RevealWhenReady;
    }

    SettingsWindowCreationResult::Canceled
}

fn finish_settings_window_creation() -> SettingsWindowCreationResult {
    let Ok(mut state) = settings_window_runtime_state().lock() else {
        return SettingsWindowCreationResult::Canceled;
    };

    let result = settings_window_creation_result(state.creating, state.show_when_ready);
    if result == SettingsWindowCreationResult::Canceled {
        return result;
    }

    state.creating = false;
    result
}

fn cancel_settings_window_creation() {
    let Ok(mut state) = settings_window_runtime_state().lock() else {
        return;
    };

    state.creating = false;
    cancel_settings_window_hide_request(&mut state);
    state.ready = false;
    state.show_when_ready = false;
    state.ownership = SettingsWindowOwnership::default();
    state.pending_context = None;
    next_settings_window_idle_destroy_generation(&mut state);
}

fn reset_settings_window_runtime_state() {
    let Ok(mut state) = settings_window_runtime_state().lock() else {
        return;
    };

    state.creating = false;
    cancel_settings_window_hide_request(&mut state);
    state.ready = false;
    state.show_when_ready = false;
    state.ownership = SettingsWindowOwnership::default();
    state.pending_context = None;
    next_settings_window_idle_destroy_generation(&mut state);
}

fn request_settings_window_show_when_ready(context: &SettingsWindowOpenContext) -> bool {
    let Ok(mut state) = settings_window_runtime_state().lock() else {
        return true;
    };

    next_settings_window_idle_destroy_generation(&mut state);
    cancel_settings_window_hide_request(&mut state);
    if state.ready {
        return true;
    }

    state.show_when_ready = true;
    state.pending_context = Some(context.clone());
    false
}

fn mark_settings_window_runtime_ready() -> Option<SettingsWindowOpenContext> {
    let Ok(mut state) = settings_window_runtime_state().lock() else {
        return None;
    };

    state.creating = false;
    state.ready = true;
    if !state.show_when_ready {
        return None;
    }

    state.show_when_ready = false;
    state.pending_context.take()
}

fn settings_window_should_reveal_from_fallback() -> bool {
    let Ok(state) = settings_window_runtime_state().lock() else {
        return true;
    };

    state.show_when_ready
}

fn schedule_settings_window_idle_destroy<R>(window: tauri::WebviewWindow<R>)
where
    R: tauri::Runtime,
{
    let generation = {
        let Ok(mut state) = settings_window_runtime_state().lock() else {
            return;
        };

        state.show_when_ready = false;
        state.pending_context = None;
        next_settings_window_idle_destroy_generation(&mut state)
    };

    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(SETTINGS_WINDOW_IDLE_DESTROY_MS));
        let should_destroy = {
            let Ok(mut state) = settings_window_runtime_state().lock() else {
                return;
            };

            if state.idle_destroy_generation != generation {
                return;
            }

            state.creating = false;
            state.ready = false;
            state.show_when_ready = false;
            state.ownership = SettingsWindowOwnership::default();
            state.pending_context = None;
            true
        };

        if should_destroy && !window.is_visible().unwrap_or(false) {
            let _ = window.destroy();
        }
    });
}

#[cfg(target_os = "macos")]
fn settings_window_title_bar_style() -> TitleBarStyle {
    TitleBarStyle::Overlay
}

#[cfg(target_os = "macos")]
fn settings_window_hidden_title() -> bool {
    SETTINGS_WINDOW_HIDDEN_TITLE
}

fn show_settings_window<R>(window: &tauri::WebviewWindow<R>)
where
    R: tauri::Runtime,
{
    let Ok(mut state) = settings_window_runtime_state().lock() else {
        let _ = window.show();
        let _ = window.set_focus();
        return;
    };
    state.show_when_ready = false;
    state.pending_context = None;
    cancel_settings_window_hide_request(&mut state);
    next_settings_window_idle_destroy_generation(&mut state);
    drop(state);

    let _ = window.show();
    let _ = window.set_focus();
}

fn emit_settings_window_target<R>(
    window: &tauri::WebviewWindow<R>,
    context: &SettingsWindowOpenContext,
) where
    R: tauri::Runtime,
{
    let _ = window.emit(
        SETTINGS_WINDOW_TARGET_EVENT,
        SettingsWindowTargetPayload {
            project_root: context.project_root.clone(),
            source_window_label: context.source_window_label.clone(),
            target: context.target.clone(),
            workspace_source_path: context.workspace_source_path.clone(),
        },
    );
}

fn reveal_settings_window_without_consuming_pending_request<R>(window: &tauri::WebviewWindow<R>)
where
    R: tauri::Runtime,
{
    if window.is_visible().unwrap_or(false) {
        return;
    }

    let _ = window.show();
    let _ = window.set_focus();
}

fn hide_settings_window_instance<R>(window: &tauri::WebviewWindow<R>)
where
    R: tauri::Runtime,
{
    let _ = window.hide();
    schedule_settings_window_idle_destroy(window.clone());
}

fn finish_pending_owner_destroy<R>(app: &tauri::AppHandle<R>)
where
    R: tauri::Runtime,
{
    let pending = settings_window_runtime_state()
        .lock()
        .ok()
        .and_then(|mut state| state.ownership.take_pending_owner_destroy());
    if let Some(owner) = pending.and_then(|label| app.get_webview_window(&label)) {
        let _ = owner.destroy();
    }
}

fn spawn_settings_window_hide_fallback<R>(window: tauri::WebviewWindow<R>, generation: usize)
where
    R: tauri::Runtime,
{
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(SETTINGS_WINDOW_HIDE_FALLBACK_MS));
        let should_hide = {
            let Ok(mut state) = settings_window_runtime_state().lock() else {
                return;
            };
            complete_settings_window_hide_fallback(&mut state, generation)
        };

        if should_hide {
            hide_settings_window_instance(&window);
            finish_pending_owner_destroy(window.app_handle());
        }
    });
}

fn request_settings_window_hide<R>(window: &tauri::WebviewWindow<R>)
where
    R: tauri::Runtime,
{
    let generation = {
        let Ok(mut state) = settings_window_runtime_state().lock() else {
            return;
        };
        let Some(generation) = begin_settings_window_hide_request(&mut state) else {
            return;
        };
        generation
    };

    let _ = window.emit(
        SETTINGS_WINDOW_HIDE_REQUESTED_EVENT,
        SettingsWindowHideRequestedPayload { generation },
    );
    spawn_settings_window_hide_fallback(window.clone(), generation);
}

fn finish_settings_window_hide<R>(window: &tauri::WebviewWindow<R>, generation: usize)
where
    R: tauri::Runtime,
{
    let should_hide = {
        let Ok(mut state) = settings_window_runtime_state().lock() else {
            return;
        };
        complete_settings_window_hide_request(&mut state, generation)
    };

    if should_hide {
        hide_settings_window_instance(window);
        finish_pending_owner_destroy(window.app_handle());
    }
}

fn should_close_settings_auxiliary_window(has_visible_user_window: bool) -> bool {
    !has_visible_user_window
}

fn is_visible_user_window(label: &str, excluded_label: Option<&str>, visible: bool) -> bool {
    Some(label) != excluded_label && !is_settings_window_label(label) && visible
}

fn app_has_visible_user_window<R>(app: &tauri::AppHandle<R>, excluded_label: Option<&str>) -> bool
where
    R: tauri::Runtime,
{
    app.webview_windows().values().any(|window| {
        is_visible_user_window(
            window.label(),
            excluded_label,
            window.is_visible().unwrap_or(false),
        )
    })
}

fn center_settings_window_on_owner<R>(
    owner: &tauri::WebviewWindow<R>,
    settings: &tauri::WebviewWindow<R>,
) -> Result<(), String>
where
    R: tauri::Runtime,
{
    let owner_position = owner.outer_position().map_err(|error| error.to_string())?;
    let owner_size = owner.outer_size().map_err(|error| error.to_string())?;
    let settings_size = settings.outer_size().map_err(|error| error.to_string())?;
    let settings_position = centered_child_position(owner_position, owner_size, settings_size);
    settings
        .set_position(settings_position)
        .map_err(|error| error.to_string())?;

    let mut state = settings_window_runtime_state()
        .lock()
        .map_err(|_| "Settings window state is unavailable.".to_string())?;
    state.ownership.owner_last_position = Some(owner_position);
    state.ownership.relative_offset = Some(relative_offset(owner_position, settings_position));
    Ok(())
}

fn initialize_settings_window_position<R>(
    owner: &tauri::WebviewWindow<R>,
    settings: &tauri::WebviewWindow<R>,
) where
    R: tauri::Runtime,
{
    if center_settings_window_on_owner(owner, settings).is_ok() {
        return;
    }

    let _ = settings.center();
    let (Ok(owner_position), Ok(settings_position)) =
        (owner.outer_position(), settings.outer_position())
    else {
        return;
    };
    if let Ok(mut state) = settings_window_runtime_state().lock() {
        state.ownership.owner_last_position = Some(owner_position);
        state.ownership.relative_offset = Some(relative_offset(owner_position, settings_position));
    }
}

fn apply_settings_window_movement<R>(
    app: &tauri::AppHandle<R>,
    moved_label: &str,
    position: PhysicalPosition<i32>,
) where
    R: tauri::Runtime,
{
    let offset = {
        let Ok(mut state) = settings_window_runtime_state().lock() else {
            return;
        };
        let Some(owner_label) = state.ownership.owner_label.clone() else {
            return;
        };

        if moved_label == SETTINGS_WINDOW_LABEL {
            let Some(owner_position) = state.ownership.owner_last_position else {
                return;
            };
            state.ownership.relative_offset = Some(relative_offset(owner_position, position));
            return;
        }
        if moved_label != owner_label {
            return;
        }

        state.ownership.owner_last_position = Some(position);
        state.ownership.relative_offset
    };

    let Some(offset) = offset else {
        return;
    };
    let Some(settings) = app.get_webview_window(SETTINGS_WINDOW_LABEL) else {
        return;
    };
    if !settings.is_visible().unwrap_or(false) {
        return;
    }
    let _move_result = settings.set_position(position_from_offset(position, offset));
}

fn close_settings_window_if_no_user_windows<R>(
    app: &tauri::AppHandle<R>,
    destroyed_window_label: &str,
) where
    R: tauri::Runtime,
{
    let windows = app.webview_windows();
    let has_visible_user_window = windows.values().any(|window| {
        is_visible_user_window(
            window.label(),
            Some(destroyed_window_label),
            window.is_visible().unwrap_or(false),
        )
    });
    if !should_close_settings_auxiliary_window(has_visible_user_window) {
        return;
    }

    reset_settings_window_runtime_state();
    if let Some(settings_window) = windows.get(SETTINGS_WINDOW_LABEL) {
        let _ = settings_window.destroy();
    }
}

pub(crate) fn apply_settings_window_lifecycle<R>(
    app: &tauri::AppHandle<R>,
    window: &tauri::Window<R>,
    event: &tauri::WindowEvent,
) where
    R: tauri::Runtime,
{
    if let tauri::WindowEvent::Moved(position) = event {
        apply_settings_window_movement(app, window.label(), *position);
    }

    if is_settings_window_label(window.label()) {
        match event {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                if let Some(settings_window) = app.get_webview_window(SETTINGS_WINDOW_LABEL) {
                    request_settings_window_hide(&settings_window);
                }
            }
            tauri::WindowEvent::Destroyed => reset_settings_window_runtime_state(),
            _ => {}
        }
        return;
    }

    if matches!(event, tauri::WindowEvent::Destroyed) {
        close_settings_window_if_no_user_windows(app, window.label());
    }
}

fn spawn_settings_window_reveal_fallback<R>(window: tauri::WebviewWindow<R>)
where
    R: tauri::Runtime,
{
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(
            SETTINGS_WINDOW_NATIVE_REVEAL_FALLBACK_MS,
        ));
        if !settings_window_should_reveal_from_fallback() {
            return;
        }
        reveal_settings_window_without_consuming_pending_request(&window);
    });
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsWindowTargetPayload {
    project_root: Option<String>,
    source_window_label: Option<String>,
    target: Option<String>,
    workspace_source_path: Option<String>,
}

#[derive(Clone, Copy, serde::Serialize)]
struct SettingsWindowHideRequestedPayload {
    generation: usize,
}

fn handle_existing_settings_window<R>(
    window: &tauri::WebviewWindow<R>,
    context: &SettingsWindowOpenContext,
) where
    R: tauri::Runtime,
{
    hide_native_menu_for_settings_window(window);
    if request_settings_window_show_when_ready(context) {
        emit_settings_window_target(window, context);
        show_settings_window(window);
    } else {
        spawn_settings_window_reveal_fallback(window.clone());
    }
}

fn create_settings_window_blocking<R>(
    app: tauri::AppHandle<R>,
    owner_window: tauri::WebviewWindow<R>,
    context: SettingsWindowOpenContext,
) -> Result<(), String>
where
    R: tauri::Runtime,
{
    if !is_editor_window_label(owner_window.label()) {
        return Err("Settings requires an editor window owner.".to_string());
    }
    if !owner_window.is_visible().unwrap_or(false) {
        return Err("Settings requires a visible editor window owner.".to_string());
    }
    if !app_has_visible_user_window(&app, None) {
        return Err("Settings requires a visible editor window.".to_string());
    }

    let owner_label = owner_window.label().to_string();
    if !begin_settings_window_creation(true, Some(context.clone())) {
        return Ok(());
    }

    let result = (|| {
        let startup_preferences = settings_window_startup_preferences(&app.config().identifier);
        let (width, height) = settings_window_inner_size();
        let (min_width, min_height) = settings_window_min_inner_size();
        let builder = WebviewWindowBuilder::new(
            &app,
            SETTINGS_WINDOW_LABEL,
            WebviewUrl::App(
                settings_window_url(
                    context.target.as_deref(),
                    context.project_root.as_deref(),
                    context.workspace_source_path.as_deref(),
                    context.source_window_label.as_deref(),
                    true,
                    &startup_preferences,
                )
                .into(),
            ),
        )
        .parent(&owner_window)
        .map_err(|error| error.to_string())?
        .title(settings_window_title(startup_preferences.language))
        .inner_size(width, height)
        .min_inner_size(min_width, min_height)
        .visible(settings_window_visible())
        .decorations(settings_window_decorations())
        .transparent(settings_window_transparent())
        .resizable(settings_window_resizable())
        .shadow(settings_window_shadow())
        .center();

        #[cfg(not(target_os = "macos"))]
        let builder = match crate::menu::create_settings_window_menu(&app) {
            Ok(menu) => builder.menu(menu),
            Err(error) => {
                eprintln!("failed to create settings window menu: {error}");
                builder
            }
        };

        let builder = if let Some(color) = settings_window_background_color_for_preferences(
            current_window_chrome_platform(),
            &startup_preferences,
        ) {
            builder.background_color(color)
        } else {
            builder
        };

        #[cfg(target_os = "macos")]
        let builder = builder
            .title_bar_style(settings_window_title_bar_style())
            .hidden_title(settings_window_hidden_title());

        let window = builder.build().map_err(|error| error.to_string())?;
        hide_native_macos_window_controls(&window);
        hide_native_menu_for_settings_window(&window);

        settings_window_runtime_state()
            .lock()
            .map_err(|_| "Settings window state is unavailable.".to_string())?
            .ownership
            .owner_label = Some(owner_label);
        initialize_settings_window_position(&owner_window, &window);

        match finish_settings_window_creation() {
            SettingsWindowCreationResult::RevealWhenReady => {
                spawn_settings_window_reveal_fallback(window.clone());
            }
            SettingsWindowCreationResult::Canceled => {
                window.destroy().map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    })();

    if result.is_err() {
        cancel_settings_window_creation();
    }
    result
}

fn create_or_reveal_settings_window_blocking<R>(
    app: tauri::AppHandle<R>,
    owner_window: tauri::WebviewWindow<R>,
    context: SettingsWindowOpenContext,
) -> Result<(), String>
where
    R: tauri::Runtime,
{
    let _operation = settings_window_operation_lock()
        .lock()
        .map_err(|_| "Settings window operation is unavailable.".to_string())?;

    if !is_editor_window_label(owner_window.label()) {
        return Err("Settings requires an editor window owner.".to_string());
    }
    if !owner_window.is_visible().unwrap_or(false) {
        return Err("Settings requires a visible editor window owner.".to_string());
    }

    if let Some(existing) = app.get_webview_window(SETTINGS_WINDOW_LABEL) {
        if !app_has_visible_user_window(&app, None) {
            reset_settings_window_runtime_state();
            existing.destroy().map_err(|error| error.to_string())?;
            return Err("Settings requires a visible editor window.".to_string());
        }

        let visible = existing.is_visible().unwrap_or(false);
        let existing_owner = settings_window_runtime_state()
            .lock()
            .ok()
            .and_then(|state| state.ownership.owner_label.clone());

        return match existing_settings_action(
            existing_owner.as_deref(),
            owner_window.label(),
            visible,
        ) {
            ExistingSettingsAction::FocusExisting | ExistingSettingsAction::ReuseHidden => {
                handle_existing_settings_window(&existing, &context);
                Ok(())
            }
            ExistingSettingsAction::RecreateHidden => {
                reset_settings_window_runtime_state();
                existing.destroy().map_err(|error| error.to_string())?;
                create_settings_window_blocking(app, owner_window, context)
            }
        };
    }

    create_settings_window_blocking(app, owner_window, context)
}

async fn spawn_settings_window_task<R>(
    app: tauri::AppHandle<R>,
    owner_window: tauri::WebviewWindow<R>,
    context: SettingsWindowOpenContext,
) -> Result<(), String>
where
    R: tauri::Runtime,
{
    tauri::async_runtime::spawn_blocking(move || {
        create_or_reveal_settings_window_blocking(app, owner_window, context)
    })
    .await
    .map_err(|error| format!("Settings window task failed: {error}"))?
}

pub(crate) fn spawn_settings_window(
    app: tauri::AppHandle,
    owner_window: tauri::WebviewWindow,
    context: SettingsWindowOpenContext,
) -> impl std::future::Future<Output = Result<(), String>> {
    spawn_settings_window_task(app, owner_window, context)
}

#[tauri::command]
pub(crate) async fn open_settings_window(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    target: Option<String>,
    project_root: Option<String>,
    workspace_source_path: Option<String>,
) -> Result<(), String> {
    let context = SettingsWindowOpenContext {
        project_root,
        source_window_label: Some(window.label().to_string()),
        target: normalized_settings_window_target(target.as_deref()).map(str::to_string),
        workspace_source_path,
    };
    spawn_settings_window(app, window, context).await
}

#[tauri::command]
pub(crate) fn mark_settings_window_ready(window: tauri::WebviewWindow) {
    if !is_settings_window_label(window.label()) {
        return;
    }

    let Some(context) = mark_settings_window_runtime_ready() else {
        return;
    };

    emit_settings_window_target(&window, &context);
    show_settings_window(&window);
}

#[tauri::command]
pub(crate) fn hide_settings_window(window: tauri::WebviewWindow) {
    if is_settings_window_label(window.label()) {
        request_settings_window_hide(&window);
    }
}

#[tauri::command]
pub(crate) fn acknowledge_settings_window_hide(window: tauri::WebviewWindow, generation: usize) {
    if !is_settings_window_label(window.label()) {
        return;
    }
    let Ok(mut state) = settings_window_runtime_state().lock() else {
        return;
    };
    acknowledge_settings_window_hide_generation(&mut state, generation);
}

#[tauri::command]
pub(crate) fn cancel_settings_window_hide(window: tauri::WebviewWindow, generation: usize) {
    if !is_settings_window_label(window.label()) {
        return;
    }
    let Ok(mut state) = settings_window_runtime_state().lock() else {
        return;
    };
    if cancel_settings_window_hide_generation(&mut state, generation) {
        state.ownership.cancel_owner_destroy();
    }
}

#[tauri::command]
pub(crate) fn complete_settings_window_hide(window: tauri::WebviewWindow, generation: usize) {
    if is_settings_window_label(window.label()) {
        finish_settings_window_hide(&window, generation);
    }
}

#[tauri::command]
pub(crate) fn destroy_current_editor_window(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<(), String> {
    if !is_editor_window_label(window.label()) {
        return window.destroy().map_err(|error| error.to_string());
    }

    let owned_settings = app
        .get_webview_window(SETTINGS_WINDOW_LABEL)
        .filter(|settings| settings.is_visible().unwrap_or(false));
    let should_wait = if owned_settings.is_some() {
        settings_window_runtime_state()
            .lock()
            .map_err(|_| "Settings window state is unavailable.".to_string())?
            .ownership
            .begin_owner_destroy(window.label())
    } else {
        false
    };

    if should_wait {
        request_settings_window_hide(&owned_settings.expect("visible Settings should exist"));
        return Ok(());
    }

    window.destroy().map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_owner_must_be_an_editor_window() {
        assert!(is_editor_window_label("main"));
        assert!(is_editor_window_label("markra-editor-3"));
        assert!(!is_editor_window_label("markra-settings"));
        assert!(!is_editor_window_label("asset-preview"));
    }

    #[test]
    fn settings_builder_requires_the_invoking_editor_as_parent() {
        let source = include_str!("windows.rs");
        let start = source
            .find("fn create_settings_window_blocking")
            .expect("settings builder helper should exist");
        let end = source[start..]
            .find("pub(crate) fn spawn_settings_window")
            .map(|offset| start + offset)
            .expect("settings builder helper should end before public spawn");
        let builder = &source[start..end];

        assert!(builder.contains(".parent(&owner_window)"));
        assert!(!builder.contains("unwrap_or(builder)"));
        assert!(builder.contains("map_err(|error| error.to_string())?"));
    }

    #[test]
    fn settings_creation_serializes_owner_selection_and_reset_clears_it() {
        let source = include_str!("windows.rs");
        assert!(source.contains("SETTINGS_WINDOW_OPERATION_LOCK"));
        assert!(source.contains("state.ownership = SettingsWindowOwnership::default()"));
    }

    #[test]
    fn settings_lifecycle_routes_moved_events_for_owner_and_child() {
        let source = include_str!("windows.rs");
        let start = source
            .find("pub(crate) fn apply_settings_window_lifecycle")
            .expect("settings lifecycle should exist");
        let end = source[start..]
            .find("fn spawn_settings_window_reveal_fallback")
            .map(|offset| start + offset)
            .expect("lifecycle should end before reveal fallback");
        let lifecycle = &source[start..end];

        assert!(lifecycle.contains("WindowEvent::Moved(position)"));
        assert!(lifecycle.contains("apply_settings_window_movement"));
    }

    #[test]
    fn settings_window_startup_accepts_dynamic_theme_ids() {
        assert!(is_theme_id("ocean-night"));
        assert!(is_theme_id("light"));
        assert!(!is_theme_id("-ocean"));
        assert!(!is_theme_id("Ocean"));
        assert!(!is_theme_id("qingyu-internal"));
    }

    #[test]
    fn settings_window_matches_editor_window_chrome() {
        assert_eq!(settings_window_transparent(), editor_window_transparent());
        assert_eq!(settings_window_decorations(), editor_window_decorations());

        #[cfg(target_os = "macos")]
        {
            assert!(matches!(
                settings_window_title_bar_style(),
                TitleBarStyle::Overlay
            ));
            assert!(settings_window_hidden_title());
        }
    }

    #[test]
    fn secondary_window_transparency_is_enabled_only_on_macos() {
        assert!(transparent_window_chrome_for_platform("macos"));
        assert!(!transparent_window_chrome_for_platform("windows"));
        assert!(!transparent_window_chrome_for_platform("linux"));
    }

    #[test]
    fn editor_window_transparency_matches_current_platform_strategy() {
        #[cfg(target_os = "macos")]
        assert!(editor_window_transparent());

        #[cfg(not(target_os = "macos"))]
        assert!(!editor_window_transparent());
    }

    #[test]
    fn settings_window_transparency_matches_current_platform_strategy() {
        #[cfg(target_os = "macos")]
        assert!(settings_window_transparent());

        #[cfg(not(target_os = "macos"))]
        assert!(!settings_window_transparent());
    }

    #[test]
    fn macos_windows_preserve_native_rounded_frame() {
        #[cfg(target_os = "windows")]
        {
            assert!(!editor_window_decorations());
            assert!(!settings_window_decorations());
        }

        #[cfg(not(target_os = "windows"))]
        {
            assert!(editor_window_decorations());
            assert!(settings_window_decorations());
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_settings_windows_are_self_drawn() {
        assert!(!editor_window_decorations());
        assert!(!settings_window_decorations());
    }

    #[test]
    fn macos_main_window_config_preserves_native_rounded_frame() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.macos.conf.json"))
                .expect("macOS Tauri config should be valid JSON");
        let decorations = config
            .pointer("/app/windows/0/decorations")
            .and_then(serde_json::Value::as_bool);
        let title_bar_style = config
            .pointer("/app/windows/0/titleBarStyle")
            .and_then(serde_json::Value::as_str);
        let hidden_title = config
            .pointer("/app/windows/0/hiddenTitle")
            .and_then(serde_json::Value::as_bool);

        assert_eq!(decorations, Some(true));
        assert_eq!(title_bar_style, Some("Overlay"));
        assert_eq!(hidden_title, Some(true));
    }

    #[test]
    fn base_main_window_config_is_linux_safe() {
        let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json"))
            .expect("base Tauri config should be valid JSON");
        let window = config
            .pointer("/app/windows/0")
            .expect("base config should declare a main window");
        let decorations = window
            .pointer("/decorations")
            .and_then(serde_json::Value::as_bool);
        let transparent = window
            .pointer("/transparent")
            .and_then(serde_json::Value::as_bool);
        let visible = window
            .pointer("/visible")
            .and_then(serde_json::Value::as_bool);

        assert_eq!(decorations, Some(true));
        assert_eq!(transparent, Some(false));
        assert_eq!(visible, Some(true));
        assert!(window.pointer("/titleBarStyle").is_none());
        assert!(window.pointer("/hiddenTitle").is_none());
    }

    #[test]
    fn editor_windows_keep_current_default_size_with_small_resize_floor() {
        let config_paths = [
            "tauri.conf.json",
            "tauri.macos.conf.json",
            "tauri.windows.conf.json",
            "tauri.linux.conf.json",
        ];

        for config_path in config_paths {
            let config_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(config_path);
            let config: serde_json::Value = serde_json::from_str(
                &std::fs::read_to_string(&config_path).expect("Tauri config should exist"),
            )
            .expect("Tauri config should be valid JSON");
            let window = config
                .pointer("/app/windows/0")
                .expect("Tauri config should declare a main window");

            assert_eq!(
                window.pointer("/width").and_then(serde_json::Value::as_i64),
                Some(1360)
            );
            assert_eq!(
                window
                    .pointer("/height")
                    .and_then(serde_json::Value::as_i64),
                Some(800)
            );
            assert_eq!(
                window
                    .pointer("/minWidth")
                    .and_then(serde_json::Value::as_i64),
                Some(360)
            );
            assert_eq!(
                window
                    .pointer("/minHeight")
                    .and_then(serde_json::Value::as_i64),
                Some(320)
            );
        }
    }

    #[test]
    fn secondary_editor_windows_keep_current_default_size_with_small_resize_floor() {
        let windows_source = include_str!("windows.rs");
        let start = windows_source
            .find("fn spawn_editor_window_with_label")
            .expect("spawn_editor_window_with_label should exist");
        let end = windows_source
            .find("fn editor_window_transparent")
            .expect("spawn_editor_window_with_label should end before editor_window_transparent");
        let spawn_editor_window_source = &windows_source[start..end];

        assert!(spawn_editor_window_source.contains(".inner_size(1360.0, 800.0)"));
        assert!(spawn_editor_window_source.contains(".min_inner_size(360.0, 320.0)"));
    }

    #[test]
    fn main_capability_allows_self_drawn_window_controls() {
        let capability: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/main.json"))
                .expect("main capability should be valid JSON");
        let permissions = capability
            .pointer("/permissions")
            .and_then(serde_json::Value::as_array)
            .expect("main capability should declare permissions");

        for permission in [
            "core:window:allow-close",
            "core:window:allow-destroy",
            "core:window:allow-is-visible",
            "core:window:allow-minimize",
            "core:window:allow-set-fullscreen",
            "core:window:allow-toggle-maximize",
        ] {
            assert!(
                permissions
                    .iter()
                    .any(|value| value.as_str() == Some(permission)),
                "missing permission {permission}"
            );
        }
    }

    #[test]
    fn windows_main_window_config_disables_transparency() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.windows.conf.json"))
                .expect("windows Tauri config should be valid JSON");
        let transparent = config
            .pointer("/app/windows/0/transparent")
            .and_then(serde_json::Value::as_bool);

        assert_eq!(transparent, Some(false));
    }

    #[test]
    fn linux_main_window_config_disables_transparency() {
        let config_path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.linux.conf.json");
        let config: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(config_path).expect("Linux Tauri config should exist"),
        )
        .expect("Linux Tauri config should be valid JSON");
        let transparent = config
            .pointer("/app/windows/0/transparent")
            .and_then(serde_json::Value::as_bool);
        let visible = config
            .pointer("/app/windows/0/visible")
            .and_then(serde_json::Value::as_bool);

        assert_eq!(transparent, Some(false));
        assert_eq!(visible, Some(true));
    }

    #[test]
    fn settings_window_uses_roomier_default_size() {
        assert_eq!(settings_window_inner_size(), (1040.0, 720.0));
        assert_eq!(settings_window_min_inner_size(), (860.0, 600.0));
        assert!(settings_window_resizable());
    }

    #[test]
    fn settings_window_starts_hidden_until_frontend_reveal() {
        assert!(!settings_window_visible());
    }

    #[test]
    fn settings_window_registers_native_reveal_fallback() {
        let windows_source = include_str!("windows.rs");

        assert_eq!(SETTINGS_WINDOW_NATIVE_REVEAL_FALLBACK_MS, 1_800);
        assert!(windows_source.contains("spawn_settings_window_reveal_fallback(window.clone())"));
        assert!(windows_source
            .contains("reveal_settings_window_without_consuming_pending_request(&window)"));
    }

    #[test]
    fn settings_window_reveal_fallback_preserves_pending_request_context() {
        let windows_source = include_str!("windows.rs");
        let fallback_start = windows_source
            .find("fn spawn_settings_window_reveal_fallback")
            .expect("settings reveal fallback should exist");
        let fallback_end = windows_source[fallback_start..]
            .find("#[derive(Clone, serde::Serialize)]")
            .map(|offset| fallback_start + offset)
            .expect("settings reveal fallback should end before payload declarations");
        let fallback_source = &windows_source[fallback_start..fallback_end];

        assert!(fallback_source
            .contains("reveal_settings_window_without_consuming_pending_request(&window)"));
        assert!(!fallback_source.contains("show_settings_window_if_hidden(&window)"));

        let reveal_start = windows_source
            .find("fn reveal_settings_window_without_consuming_pending_request")
            .expect("fallback should use a dedicated non-consuming reveal path");
        let reveal_end = windows_source[reveal_start..]
            .find("fn hide_settings_window_instance")
            .map(|offset| reveal_start + offset)
            .expect("dedicated reveal should end before hide helpers");
        let reveal_source = &windows_source[reveal_start..reveal_end];

        assert!(!reveal_source.contains("settings_window_runtime_state"));
        assert!(!reveal_source.contains("show_settings_window(window)"));
        assert!(reveal_source.contains("window.show()"));
        assert!(reveal_source.contains("window.set_focus()"));
    }

    #[test]
    fn settings_hide_paths_request_handshake_before_hiding() {
        let windows_source = include_str!("windows.rs");
        let lifecycle_start = windows_source
            .find("pub(crate) fn apply_settings_window_lifecycle")
            .expect("settings lifecycle should exist");
        let lifecycle_end = windows_source[lifecycle_start..]
            .find("fn spawn_settings_window_reveal_fallback")
            .map(|offset| lifecycle_start + offset)
            .expect("settings lifecycle should end before reveal fallback");
        let lifecycle_source = &windows_source[lifecycle_start..lifecycle_end];
        let command_start = windows_source
            .find("pub(crate) fn hide_settings_window")
            .expect("frontend settings hide command should exist");
        let command_end = windows_source[command_start..]
            .find("#[cfg(test)]")
            .map(|offset| command_start + offset)
            .expect("settings hide command should end before tests");
        let command_source = &windows_source[command_start..command_end];

        assert!(windows_source.contains("qingyu://settings-hide-requested"));
        assert!(lifecycle_source.contains("WindowEvent::CloseRequested"));
        assert!(lifecycle_source.contains("prevent_close"));
        assert!(lifecycle_source.contains("request_settings_window_hide"));
        assert!(!lifecycle_source.contains("hide_settings_window_instance"));
        assert!(command_source.contains("request_settings_window_hide"));
        assert!(command_source.contains("acknowledge_settings_window_hide"));
        assert!(command_source.contains("cancel_settings_window_hide"));
        assert!(command_source.contains("complete_settings_window_hide"));
    }

    #[test]
    fn settings_hide_handshake_is_idempotent_and_has_a_bounded_fallback() {
        let mut state = SettingsWindowRuntimeState::default();

        let generation = begin_settings_window_hide_request(&mut state)
            .expect("first request should start a handshake");
        assert!(begin_settings_window_hide_request(&mut state).is_none());
        assert!(complete_settings_window_hide_request(
            &mut state, generation
        ));
        assert!(!complete_settings_window_hide_request(
            &mut state, generation
        ));
        assert!(!complete_settings_window_hide_fallback(
            &mut state, generation
        ));

        let fallback_generation = begin_settings_window_hide_request(&mut state)
            .expect("a later request should start a new handshake");
        assert!(complete_settings_window_hide_fallback(
            &mut state,
            fallback_generation
        ));
        assert!(!complete_settings_window_hide_fallback(
            &mut state,
            fallback_generation
        ));
        assert!(SETTINGS_WINDOW_HIDE_FALLBACK_MS > 0);
        assert!(SETTINGS_WINDOW_HIDE_FALLBACK_MS <= 2_000);
    }

    #[test]
    fn settings_hide_completion_rejects_a_stale_generation_after_reopen() {
        let mut state = SettingsWindowRuntimeState::default();

        let first_generation = begin_settings_window_hide_request(&mut state)
            .expect("first hide should start a handshake");
        cancel_settings_window_hide_request(&mut state);
        let second_generation = begin_settings_window_hide_request(&mut state)
            .expect("hide after reopen should start a new handshake");

        assert_ne!(first_generation, second_generation);
        assert!(!complete_settings_window_hide_request(
            &mut state,
            first_generation
        ));
        assert!(state.hide_requested);
        assert!(complete_settings_window_hide_request(
            &mut state,
            second_generation
        ));
    }

    #[test]
    fn settings_hide_failure_cancels_fallback_and_later_retry_can_hide() {
        let mut state = SettingsWindowRuntimeState::default();

        let failed_generation = begin_settings_window_hide_request(&mut state)
            .expect("failed close should start a handshake");
        assert!(cancel_settings_window_hide_generation(
            &mut state,
            failed_generation
        ));
        assert!(!complete_settings_window_hide_fallback(
            &mut state,
            failed_generation
        ));

        let retry_generation = begin_settings_window_hide_request(&mut state)
            .expect("later close retry should start a fresh handshake");
        assert_ne!(retry_generation, failed_generation);
        assert!(complete_settings_window_hide_request(
            &mut state,
            retry_generation
        ));

        let unresponsive_generation = begin_settings_window_hide_request(&mut state)
            .expect("unresponsive frontend should start a handshake");
        assert!(complete_settings_window_hide_fallback(
            &mut state,
            unresponsive_generation
        ));
    }

    #[test]
    fn settings_hide_ack_disarms_initial_fallback_without_completing_request() {
        let mut state = SettingsWindowRuntimeState::default();

        let generation = begin_settings_window_hide_request(&mut state)
            .expect("responsive frontend should receive a hide generation");
        assert!(acknowledge_settings_window_hide_generation(
            &mut state, generation
        ));
        assert!(state.hide_requested);
        assert!(!complete_settings_window_hide_fallback(
            &mut state, generation
        ));
        assert!(complete_settings_window_hide_request(
            &mut state, generation
        ));

        let unresponsive_generation = begin_settings_window_hide_request(&mut state)
            .expect("later request should get a fresh generation");
        assert!(complete_settings_window_hide_fallback(
            &mut state,
            unresponsive_generation
        ));
    }

    #[test]
    fn settings_hide_request_payload_carries_its_generation() {
        let payload = serde_json::to_value(SettingsWindowHideRequestedPayload { generation: 37 })
            .expect("hide request payload should serialize");

        assert_eq!(payload.get("generation").and_then(Value::as_u64), Some(37));
    }

    #[test]
    fn internal_settings_cleanup_destroys_instead_of_reentering_the_hide_handshake() {
        let windows_source = include_str!("windows.rs");
        let idle_start = windows_source
            .find("fn schedule_settings_window_idle_destroy")
            .expect("idle destroy scheduler should exist");
        let idle_end = windows_source[idle_start..]
            .find("#[cfg(target_os = \"macos\")]")
            .map(|offset| idle_start + offset)
            .expect("idle destroy scheduler should end before platform helpers");
        let idle_source = &windows_source[idle_start..idle_end];
        let last_window_start = windows_source
            .find("fn close_settings_window_if_no_user_windows")
            .expect("last-window cleanup should exist");
        let last_window_end = windows_source[last_window_start..]
            .find("pub(crate) fn apply_settings_window_lifecycle")
            .map(|offset| last_window_start + offset)
            .expect("last-window cleanup should end before lifecycle routing");
        let last_window_source = &windows_source[last_window_start..last_window_end];

        assert!(idle_source.contains("window.destroy()"));
        assert!(!idle_source.contains("window.close()"));
        assert!(!idle_source.contains("request_settings_window_hide"));
        assert!(last_window_source.contains("settings_window.destroy()"));
        assert!(!last_window_source.contains("settings_window.close()"));
        assert!(!last_window_source.contains("request_settings_window_hide"));
        assert!(!last_window_source.contains("schedule_settings_window_idle_destroy"));
    }

    #[test]
    fn settings_window_lifecycle_closes_auxiliary_window_without_user_windows() {
        assert!(should_close_settings_auxiliary_window(false));
        assert!(!should_close_settings_auxiliary_window(true));
    }

    #[test]
    fn settings_window_creation_result_cancels_reset_creation() {
        assert_eq!(
            settings_window_creation_result(false, true),
            SettingsWindowCreationResult::Canceled
        );
        assert_eq!(
            settings_window_creation_result(true, false),
            SettingsWindowCreationResult::Canceled
        );
        assert_eq!(
            settings_window_creation_result(true, true),
            SettingsWindowCreationResult::RevealWhenReady
        );
    }

    #[test]
    fn settings_window_user_window_detection_excludes_settings_and_destroyed_window() {
        assert!(is_visible_user_window("main", None, true));
        assert!(!is_visible_user_window("main", Some("main"), true));
        assert!(!is_visible_user_window(SETTINGS_WINDOW_LABEL, None, true));
        assert!(!is_visible_user_window("main", None, false));
    }

    #[test]
    fn localizes_settings_window_native_title_from_startup_language() {
        assert_eq!(settings_window_title(AppLanguage::En), "Settings");
        assert_eq!(settings_window_title(AppLanguage::ZhCn), "设置");
    }

    #[test]
    fn windows_editor_windows_hide_native_menu() {
        assert!(should_hide_native_menu_for_window_label_on_platform(
            "windows",
            SETTINGS_WINDOW_LABEL
        ));
        assert!(should_hide_native_menu_for_window_label_on_platform(
            "macos",
            SETTINGS_WINDOW_LABEL
        ));
        assert!(should_hide_native_menu_for_window_label_on_platform(
            "windows",
            MAIN_WINDOW_LABEL
        ));
        assert!(should_hide_native_menu_for_window_label_on_platform(
            "windows",
            "markra-editor-1"
        ));
        assert!(!should_hide_native_menu_for_window_label_on_platform(
            "macos",
            MAIN_WINDOW_LABEL
        ));
    }

    #[test]
    fn windows_editor_windows_are_self_drawn() {
        assert!(!editor_window_decorations_for_platform("windows"));
        assert!(editor_window_decorations_for_platform("macos"));
        assert!(editor_window_decorations_for_platform("linux"));
    }

    #[test]
    fn secondary_window_background_matches_transparency_strategy() {
        assert_eq!(
            transparent_window_background_color_for_platform("macos"),
            Some(Color(255, 255, 255, 0))
        );
        assert_eq!(
            transparent_window_background_color_for_platform("windows"),
            None
        );
        assert_eq!(
            transparent_window_background_color_for_platform("linux"),
            None
        );
    }

    #[test]
    fn settings_window_background_matches_current_platform_strategy() {
        assert!(settings_window_shadow());
        let startup_preferences = SettingsWindowStartupPreferences::default();

        assert_eq!(
            settings_window_background_color_for_preferences("macos", &startup_preferences),
            Some(Color(255, 255, 255, 0))
        );
        assert_eq!(
            settings_window_background_color_for_preferences("windows", &startup_preferences),
            Some(Color(30, 30, 30, 255))
        );

        let light_startup_preferences = SettingsWindowStartupPreferences {
            language: AppLanguage::En,
            appearance_mode: "light".to_string(),
            light_theme: "light".to_string(),
            dark_theme: "dark".to_string(),
        };

        assert_eq!(
            settings_window_background_color_for_preferences("windows", &light_startup_preferences),
            Some(Color(255, 255, 255, 255))
        );
    }

    #[test]
    fn creates_unique_blank_editor_window_labels() {
        let first = next_blank_editor_window_label();
        let second = next_blank_editor_window_label();

        assert_ne!(first, second);
        assert!(is_blank_editor_window_label(&first));
        assert!(is_blank_editor_window_label(&second));
        assert!(!is_blank_editor_window_label("main"));
    }

    #[test]
    fn editor_window_labels_exclude_settings_window() {
        assert!(is_editor_window_label("main"));
        assert!(is_editor_window_label("markra-editor-1"));
        assert!(!is_editor_window_label("markra-settings"));
    }

    #[test]
    fn secondary_editor_windows_become_native_menu_targets_when_created() {
        let source = include_str!("windows.rs");
        let start = source
            .find("fn spawn_editor_window_with_label")
            .expect("spawn_editor_window_with_label should exist");
        let end = source[start..]
            .find("fn editor_window_transparent")
            .map(|offset| start + offset)
            .expect("spawn_editor_window_with_label should end before editor_window_transparent");
        let spawn_editor_window_source = &source[start..end];

        assert!(
            spawn_editor_window_source.contains("remember_native_menu_webview_window(&window);"),
            "secondary editor windows should become native menu targets as soon as they are created"
        );
    }

    #[test]
    fn exposes_window_command_names_for_js_menus() {
        assert_eq!(MINIMIZE_CURRENT_WINDOW_COMMAND, "minimize_current_window");
        assert_eq!(OPEN_BLANK_EDITOR_WINDOW_COMMAND, "open_blank_editor_window");
        assert_eq!(OPEN_SETTINGS_WINDOW_COMMAND, "open_settings_window");
    }

    #[test]
    fn targets_export_pandoc_settings_from_window_url() {
        let startup_preferences = SettingsWindowStartupPreferences {
            language: AppLanguage::ZhCn,
            appearance_mode: "dark".to_string(),
            light_theme: "sepia".to_string(),
            dark_theme: "night".to_string(),
        };

        assert_eq!(
            settings_window_url(
                Some("exportPandocPath"),
                None,
                None,
                None,
                false,
                &startup_preferences,
            ),
            "index.html?settings=1&startupLanguage=zh-CN&startupAppearanceMode=dark&startupLightTheme=sepia&startupDarkTheme=night&settingsTarget=exportPandocPath"
        );
    }

    #[test]
    fn targets_sync_settings_from_window_url() {
        let startup_preferences = SettingsWindowStartupPreferences::default();

        assert_eq!(
            settings_window_url(Some("sync"), None, None, None, false, &startup_preferences),
            "index.html?settings=1&startupLanguage=en&startupAppearanceMode=system&startupLightTheme=light&startupDarkTheme=dark&settingsTarget=sync"
        );
    }

    #[test]
    fn settings_window_url_uses_default_startup_preferences() {
        assert_eq!(
            settings_window_url(
                None,
                None,
                None,
                None,
                false,
                &SettingsWindowStartupPreferences::default(),
            ),
            "index.html?settings=1&startupLanguage=en&startupAppearanceMode=system&startupLightTheme=light&startupDarkTheme=dark"
        );
    }

    #[test]
    fn settings_window_url_carries_the_invoking_project_context() {
        assert_eq!(
            settings_window_url(
                Some("sync"),
                Some("/mock files/project-a"),
                Some("/mock files/project-a/notes/standalone.md"),
                Some("markra-editor-2"),
                true,
                &SettingsWindowStartupPreferences::default(),
            ),
            "index.html?settings=1&startupLanguage=en&startupAppearanceMode=system&startupLightTheme=light&startupDarkTheme=dark&settingsTarget=sync&settingsProjectContext=1&settingsProjectRoot=%2Fmock%20files%2Fproject-a&settingsWorkspaceContext=1&settingsWorkspaceSourcePath=%2Fmock%20files%2Fproject-a%2Fnotes%2Fstandalone.md&settingsSourceWindowLabel=markra-editor-2"
        );
        assert_eq!(
            settings_window_url(
                Some("sync"),
                None,
                Some("/notes/standalone.md"),
                Some("main"),
                true,
                &SettingsWindowStartupPreferences::default(),
            ),
            "index.html?settings=1&startupLanguage=en&startupAppearanceMode=system&startupLightTheme=light&startupDarkTheme=dark&settingsTarget=sync&settingsProjectContext=1&settingsWorkspaceContext=1&settingsWorkspaceSourcePath=%2Fnotes%2Fstandalone.md&settingsSourceWindowLabel=main"
        );
    }

    #[test]
    fn settings_window_target_payload_serializes_one_atomic_workspace_context() {
        let payload = serde_json::to_value(SettingsWindowTargetPayload {
            project_root: None,
            source_window_label: Some("markra-editor-2".to_string()),
            target: Some("sync".to_string()),
            workspace_source_path: Some("/notes/standalone.md".to_string()),
        })
        .expect("settings context payload should serialize");

        assert_eq!(payload.get("projectRoot"), Some(&Value::Null));
        assert_eq!(
            payload.get("sourceWindowLabel").and_then(Value::as_str),
            Some("markra-editor-2")
        );
        assert_eq!(
            payload.get("workspaceSourcePath").and_then(Value::as_str),
            Some("/notes/standalone.md")
        );
    }

    #[test]
    fn concurrent_settings_open_takes_the_newest_pending_workspace_context() {
        reset_settings_window_runtime_state();
        let first = SettingsWindowOpenContext {
            project_root: Some("/projects/first".to_string()),
            source_window_label: Some("main".to_string()),
            target: Some("sync".to_string()),
            workspace_source_path: Some("/projects/first".to_string()),
        };
        let newest = SettingsWindowOpenContext {
            project_root: None,
            source_window_label: Some("markra-editor-2".to_string()),
            target: None,
            workspace_source_path: Some("/notes/standalone.md".to_string()),
        };

        assert!(begin_settings_window_creation(true, Some(first)));
        assert!(!begin_settings_window_creation(true, Some(newest.clone())));
        assert_eq!(mark_settings_window_runtime_ready(), Some(newest));

        reset_settings_window_runtime_state();
    }

    #[test]
    fn canceled_and_reset_settings_creation_clear_pending_workspace_context() {
        reset_settings_window_runtime_state();
        let pending = SettingsWindowOpenContext {
            project_root: None,
            source_window_label: Some("markra-editor-3".to_string()),
            target: None,
            workspace_source_path: Some("/notes/standalone.md".to_string()),
        };

        assert!(begin_settings_window_creation(true, Some(pending.clone())));
        cancel_settings_window_creation();
        assert!(settings_window_runtime_state()
            .lock()
            .expect("settings runtime state should lock")
            .pending_context
            .is_none());

        assert!(begin_settings_window_creation(true, Some(pending)));
        reset_settings_window_runtime_state();
        assert!(settings_window_runtime_state()
            .lock()
            .expect("settings runtime state should lock")
            .pending_context
            .is_none());
    }

    #[test]
    fn legacy_theme_preferences_preserve_old_theme_settings() {
        assert_eq!(
            legacy_theme_preferences(AppLanguage::En, Some("night")),
            SettingsWindowStartupPreferences {
                language: AppLanguage::En,
                appearance_mode: "dark".to_string(),
                light_theme: "light".to_string(),
                dark_theme: "night".to_string(),
            }
        );
        assert_eq!(
            legacy_theme_preferences(AppLanguage::En, Some("sepia")),
            SettingsWindowStartupPreferences {
                language: AppLanguage::En,
                appearance_mode: "light".to_string(),
                light_theme: "sepia".to_string(),
                dark_theme: "dark".to_string(),
            }
        );
    }

    #[test]
    fn encodes_open_file_window_urls() {
        assert_eq!(
            editor_window_url_for_path("/mock files/read me.md"),
            "index.html?path=%2Fmock%20files%2Fread%20me.md"
        );
        assert_eq!(
            editor_window_url_for_path("/mock/中文.md"),
            "index.html?path=%2Fmock%2F%E4%B8%AD%E6%96%87.md"
        );
    }
}
