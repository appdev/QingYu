use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[path = "../build_support.rs"]
mod build_support;

const MCP_COMMANDS: &[&str] = &[
    "get_mcp_settings",
    "update_mcp_settings",
    "set_mcp_primary_workspace",
    "get_mcp_health",
    "list_mcp_audit_entries",
    "clear_mcp_audit_entries",
];

const TYPED_SETTINGS_COMMANDS: &[&str] = &[
    "read_app_settings_group",
    "write_app_settings_group",
    "replace_portable_app_settings",
];

const MOBILE_COMMANDS: &[&str] = &[
    "get_mcp_policy",
    "update_mcp_policy",
    "read_app_settings_group",
    "write_app_settings_group",
    "replace_portable_app_settings",
    "read_primary_workspace_state",
    "write_primary_workspace_state",
    "list_themes",
    "read_theme_css",
    "prepare_theme_activation",
    "commit_theme_activation",
    "cancel_theme_activation",
    "release_theme_activation",
    "delete_theme",
    "list_markdown_files_for_path",
    "load_markdown_files_for_path",
    "cancel_markdown_files_load",
    "search_markdown_files_for_path",
    "create_markdown_tree_file",
    "create_markdown_tree_folder",
    "rename_markdown_tree_file",
    "move_markdown_tree_file",
    "delete_markdown_tree_file",
    "read_markdown_file",
    "list_markdown_file_history",
    "read_markdown_file_history",
    "write_markdown_file",
    "watch_markdown_file",
    "unwatch_markdown_file",
    "watch_markdown_tree",
    "unwatch_markdown_tree",
    "load_sync_config",
    "enable_sync_config",
    "patch_sync_config",
    "recover_sync_config",
    "reset_sync_config",
    "load_sync_config_editing",
    "set_sync_config_editing",
    "request_sync_config_apply",
    "cancel_sync_config_apply",
    "load_sync_status",
    "list_remote_notebooks",
    "sync_application",
    "test_sync_connection",
    "is_document_in_workspace",
    "list_managed_workspace_names",
    "resolve_managed_workspace_root",
    "save_clipboard_image",
    "download_web_image",
    "complete_mobile_back",
];

const DESKTOP_COMMANDS: &[&str] = &[
    "get_mcp_settings",
    "update_mcp_settings",
    "set_mcp_primary_workspace",
    "get_mcp_health",
    "list_mcp_audit_entries",
    "clear_mcp_audit_entries",
    "read_app_settings_group",
    "write_app_settings_group",
    "replace_portable_app_settings",
    "read_exposed_app_settings",
    "patch_exposed_app_settings",
    "read_primary_workspace_state",
    "write_primary_workspace_state",
    "prepare_desktop_notebook_target",
    "discard_prepared_desktop_notebook_target",
    "list_themes",
    "read_theme_css",
    "prepare_theme_activation",
    "commit_theme_activation",
    "cancel_theme_activation",
    "release_theme_activation",
    "import_theme_file",
    "replace_theme_file",
    "delete_theme",
    "theme_directory_path",
    "list_markdown_files_for_path",
    "load_markdown_files_for_path",
    "cancel_markdown_files_load",
    "search_markdown_files_for_path",
    "create_markdown_tree_file",
    "create_markdown_tree_folder",
    "install_application_menu",
    "show_native_app_about",
    "rename_markdown_tree_file",
    "move_markdown_tree_file",
    "delete_markdown_tree_file",
    "open_markdown_file_in_new_window",
    "open_containing_folder",
    "open_markdown_attachment",
    "resolve_markdown_path",
    "resolve_markdown_folder",
    "resolve_workspace_resource_root",
    "trash_workspace_resources",
    "read_markdown_file",
    "read_text_file",
    "list_markdown_file_history",
    "read_markdown_file_history",
    "import_local_file",
    "read_local_image_file",
    "read_markdown_template_file",
    "write_markdown_template_file",
    "delete_markdown_template_file",
    "save_clipboard_attachment",
    "save_clipboard_image",
    "canonical_local_file_path",
    "read_clipboard_text",
    "minimize_current_window",
    "open_blank_editor_window",
    "open_settings_window",
    "mark_settings_window_ready",
    "hide_settings_window",
    "acknowledge_settings_window_hide",
    "cancel_settings_window_hide",
    "complete_settings_window_hide",
    "destroy_current_editor_window",
    "download_web_image",
    "write_markdown_file",
    "write_text_file",
    "export_pdf_file",
    "check_pandoc_available",
    "detect_pandoc_path",
    "export_pandoc_file",
    "watch_markdown_file",
    "unwatch_markdown_file",
    "watch_markdown_tree",
    "unwatch_markdown_tree",
    "request_primary_cloud_notebook_catalog",
    "request_primary_notebook_switch",
    "take_opened_markdown_paths",
    "get_shell_command_status",
    "install_shell_command",
    "uninstall_shell_command",
    "set_editor_window_restore_state",
    "list_editor_window_restore_states",
    "list_system_font_families",
    "load_sync_config",
    "enable_sync_config",
    "patch_sync_config",
    "recover_sync_config",
    "reset_sync_config",
    "load_sync_config_editing",
    "set_sync_config_editing",
    "request_sync_config_apply",
    "cancel_sync_config_apply",
    "load_sync_status",
    "list_remote_notebooks",
    "sync_application",
    "test_sync_connection",
    "is_document_in_workspace",
    "list_managed_workspace_names",
    "resolve_managed_workspace_root",
    "open_log_folder",
];

fn manifest_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn source(relative: impl AsRef<Path>) -> String {
    let path = manifest_root().join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
}

fn production_source_tree(relative: impl AsRef<Path>) -> String {
    fn collect(path: &Path, contents: &mut String) {
        if path.is_dir() {
            for entry in std::fs::read_dir(path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
            {
                let entry = entry.expect("source tree entry should be readable");
                collect(&entry.path(), contents);
            }
            return;
        }

        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if file_name.ends_with("_tests.rs")
            || file_name == "tests.rs"
            || file_name.contains(".test.")
        {
            return;
        }
        if !matches!(
            path.extension().and_then(|extension| extension.to_str()),
            Some("rs" | "ts" | "tsx")
        ) {
            return;
        }

        contents.push_str(
            &std::fs::read_to_string(path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display())),
        );
        contents.push('\n');
    }

    let mut contents = String::new();
    collect(&manifest_root().join(relative), &mut contents);
    contents
}

fn handler_identifiers(runtime_source: &str) -> BTreeSet<String> {
    let marker = "tauri::generate_handler![";
    let start = runtime_source
        .find(marker)
        .unwrap_or_else(|| panic!("runtime should contain {marker}"))
        + marker.len();
    let end = runtime_source[start..]
        .find(']')
        .map(|offset| start + offset)
        .expect("generate_handler should have a closing bracket");

    runtime_source[start..end]
        .split(',')
        .map(str::trim)
        .filter(|identifier| !identifier.is_empty())
        .map(|identifier| {
            identifier
                .rsplit("::")
                .next()
                .expect("handler path should have an identifier")
                .to_string()
        })
        .collect()
}

fn dependency_section<'a>(manifest: &'a str, header: &str) -> &'a str {
    let start = manifest
        .find(header)
        .unwrap_or_else(|| panic!("manifest should contain {header}"));
    let body_start = start + header.len();
    let end = manifest[body_start..]
        .find("\n[")
        .map(|offset| body_start + offset)
        .unwrap_or(manifest.len());
    &manifest[body_start..end]
}

fn json(relative: &str) -> serde_json::Value {
    serde_json::from_str(&source(relative))
        .unwrap_or_else(|error| panic!("{relative} should be valid JSON: {error}"))
}

fn string_array_at<'a>(value: &'a serde_json::Value, pointer: &str) -> Vec<&'a str> {
    value
        .pointer(pointer)
        .and_then(serde_json::Value::as_array)
        .unwrap_or_else(|| panic!("{pointer} should be an array"))
        .iter()
        .map(|entry| {
            entry
                .as_str()
                .unwrap_or_else(|| panic!("{pointer} entries should be strings"))
        })
        .collect()
}

fn permission_identifiers(value: &serde_json::Value) -> Vec<&str> {
    value
        .pointer("/permissions")
        .and_then(serde_json::Value::as_array)
        .expect("permissions should be an array")
        .iter()
        .map(|permission| {
            permission.as_str().or_else(|| {
                permission
                    .get("identifier")
                    .and_then(serde_json::Value::as_str)
            })
        })
        .collect::<Option<Vec<_>>>()
        .expect("permissions should be strings or scoped permission objects")
}

#[test]
fn builder_boundary_desktop_preserves_the_complete_command_surface() {
    let runtime = source("src/desktop_runtime.rs");
    assert!(
        runtime.contains("tauri_plugin_opener::init()"),
        "desktop runtime omitted the official opener plugin"
    );
    let commands = handler_identifiers(&runtime);
    let expected = DESKTOP_COMMANDS
        .iter()
        .map(|command| (*command).to_string())
        .collect::<BTreeSet<_>>();

    assert_eq!(commands, expected, "desktop command registrations changed");
    for command in MCP_COMMANDS.iter().chain(TYPED_SETTINGS_COMMANDS) {
        assert!(commands.contains(*command), "desktop omitted {command}");
    }
}

#[test]
fn builder_boundary_production_sources_and_current_guidance_are_application_scoped() {
    let production = [
        production_source_tree("src"),
        production_source_tree("../src"),
        production_source_tree("../../../packages/app/src"),
        production_source_tree("../../../packages/shared/src"),
        production_source_tree("../../site/src"),
    ]
    .join("\n");
    for forbidden in [
        "activate_mcp_project",
        "load_project_mcp_config",
        ".qingyu/mcp.json",
        "为当前项目单独开启 MCP",
        "MCP for the current project",
    ] {
        assert!(
            !production.contains(forbidden),
            "obsolete project-scoped MCP production contract remains: {forbidden}"
        );
    }
    assert!(production.contains("set_mcp_primary_workspace"));

    let guidance = [
        source("../../../docs/qingyu-mcp.md"),
        source("../../../docs/privacy.md"),
    ]
    .join("\n");
    for forbidden in [
        ".qingyu/mcp.json",
        "最近获得焦点的轻语编辑器窗口",
        "当前项目的权限",
        "focused QingYu editor window",
        "disabled per project",
    ] {
        assert!(
            !guidance.contains(forbidden),
            "current MCP guidance retains an obsolete project boundary: {forbidden}"
        );
    }
    for required in ["settings.json", "mcp-runtime", "当前笔记目录"] {
        assert!(
            guidance.contains(required),
            "current MCP guidance omits the application contract: {required}"
        );
    }
}

#[test]
fn builder_boundary_app_harness_uses_the_final_application_mcp_contract() {
    let harness = source("../../../packages/app/src/test/app-harness.tsx");
    for required in [
        "createApplicationMcpRuntime",
        "localServiceAvailable: true",
        "policyAvailable: true",
        "setPrimaryWorkspace: vi.fn",
        "getSettings: vi.fn",
        "updateSettings: vi.fn",
    ] {
        assert!(
            harness.contains(required),
            "App harness omits final application MCP contract: {required}"
        );
    }
    assert!(!harness.contains("activateProject"));
}

#[test]
fn builder_boundary_mobile_registers_only_approved_shared_commands() {
    let commands = handler_identifiers(&source("src/mobile_runtime.rs"));
    let expected = MOBILE_COMMANDS
        .iter()
        .map(|command| (*command).to_string())
        .collect::<BTreeSet<_>>();

    assert_eq!(commands, expected, "mobile command registrations changed");
}

#[test]
fn builder_boundary_theme_activation_uses_window_identity_and_narrow_lifetimes() {
    let activation = source("src/themes/activation.rs");
    let themes = source("src/themes/mod.rs");
    let desktop = source("src/desktop_runtime.rs");
    let mobile = source("src/mobile_runtime.rs");

    assert!(
        activation.contains("window: tauri::WebviewWindow"),
        "activation commands should receive the invoking WebviewWindow"
    );
    assert!(
        activation.contains("window.label()"),
        "activation commands should derive identity from the invoking window"
    );
    for command in [
        "prepare_theme_activation",
        "commit_theme_activation",
        "cancel_theme_activation",
        "release_theme_activation",
    ] {
        let start = activation
            .find(&format!("pub(crate) fn {command}"))
            .unwrap_or_else(|| panic!("activation module should define {command}"));
        let signature_end = activation[start..]
            .find(") ->")
            .map(|offset| start + offset)
            .unwrap_or_else(|| panic!("{command} should have a complete signature"));
        assert!(
            !activation[start..signature_end].contains("window_label"),
            "{command} must not accept a caller-supplied window label"
        );
    }
    assert!(
        desktop.contains(".manage(crate::themes::ThemeActivationState::default())")
            && mobile.contains(".manage(crate::themes::ThemeActivationState::default())"),
        "desktop and mobile should each manage one activation state"
    );
    assert!(
        desktop.contains("tauri::WindowEvent::Destroyed")
            && desktop.contains("release_theme_activation_for_window"),
        "desktop window destruction should release pending and active theme roots"
    );
    assert!(
        themes.contains("activation::delete_theme_for_app"),
        "resource deletion should revoke activation references before catalog removal"
    );
    assert!(
        activation.contains("allow_directory(path, true)")
            && activation.contains("forbid_directory(path, true)"),
        "theme asset permissions should be recursive only for the validated package path"
    );
    assert!(
        !activation.contains("app_data_dir") && !activation.contains("migration::theme_directory"),
        "activation must never grant an app-data-wide or catalog-wide path"
    );
}

#[test]
fn builder_boundary_mobile_back_intercepts_only_uncoded_exit_requests() {
    let runtime = source("src/mobile_runtime.rs");
    assert!(runtime.contains("MobileBackState::default()"));
    assert!(runtime.contains("crate::mobile_back::complete_mobile_back"));
    assert!(runtime.contains("tauri::RunEvent::ExitRequested"));
    assert!(runtime.contains("code: None"));
    assert!(runtime.contains("api.prevent_exit()"));
    assert!(runtime.contains("emit_mobile_back_requested"));

    let mobile_back = source("src/mobile_back.rs");
    assert!(mobile_back.contains("pub(crate) fn complete_mobile_back"));
    assert!(mobile_back.contains("app.exit(0)"));
}

#[test]
fn builder_boundary_mobile_excludes_desktop_modules_state_plugins_and_initialization() {
    let runtime = source("src/mobile_runtime.rs");
    for forbidden in [
        "OpenedMarkdownPathsState",
        "NativeApplicationMenuState",
        "NativeMenuTargetState",
        "EditorWindowRestoreState",
        "mcp::",
        "McpState",
        "qingyu-mcp",
        "tauri_plugin_process",
        "tauri_plugin_updater",
        "tauri_plugin_single_instance",
        "tauri_plugin_window_state",
        "arboard",
        "fontdb",
        ".menu(",
        ".on_menu_event(",
        ".on_window_event(",
    ] {
        assert!(
            !runtime.contains(forbidden),
            "mobile runtime contains {forbidden}"
        );
    }

    for plugin in [
        "tauri_plugin_store",
        "tauri_plugin_dialog",
        "tauri_plugin_fs",
        "tauri_plugin_log",
        "tauri_plugin_os",
        "tauri_plugin_opener",
    ] {
        assert!(runtime.contains(plugin), "mobile runtime omitted {plugin}");
    }

    for state in [
        "MarkdownFileWatcherState",
        "MarkdownTreeWatcherState",
        "MarkdownTreeLoadState",
        "MobileBackState",
    ] {
        assert!(runtime.contains(state), "mobile runtime omitted {state}");
    }
}

#[test]
fn builder_boundary_dispatcher_cfg_gates_desktop_only_modules() {
    let lib = source("src/lib.rs");
    assert!(lib.contains("\nmod app_settings;"));
    assert!(!lib.contains("#[cfg(any(desktop, feature = \"desktop-sidecar\"))]\nmod app_settings;"));
    assert!(lib.contains("\nmod mcp;"));
    assert!(!lib.contains("#[cfg(any(desktop, feature = \"desktop-sidecar\"))]\nmod mcp;"));
    let mcp = source("src/mcp/mod.rs");
    assert!(mcp.contains("pub(crate) mod config;"));
    assert!(!mcp
        .contains("#[cfg(any(desktop, feature = \"desktop-sidecar\"))]\npub(crate) mod config;"));
    for declaration in [
        "pub(crate) mod audit;",
        "pub(crate) mod bridge;",
        "pub(crate) mod confirmation;",
        "pub(crate) mod error;",
        "pub(crate) mod handles;",
        "pub(crate) mod ipc;",
        "pub(crate) mod policy;",
        "pub(crate) mod server;",
        "pub(crate) mod tools;",
        "pub(crate) mod workspaces;",
    ] {
        let declaration_start = mcp.find(declaration).expect("MCP module declaration");
        let recent_prefix = &mcp[..declaration_start];
        let recent_prefix = &recent_prefix[recent_prefix.len().saturating_sub(180)..];
        assert!(
            recent_prefix.contains("#[cfg(any(desktop, feature = \"desktop-sidecar\"))]"),
            "{declaration} must remain desktop-only"
        );
    }
    assert!(lib.contains("#[cfg(desktop)]\nmod desktop_runtime;"));
    assert!(lib.contains("#[cfg(mobile)]\nmod mobile_runtime;"));
    assert!(lib.contains("#[cfg(desktop)]\n    desktop_runtime::run();"));
    assert!(lib.contains("#[cfg(mobile)]\n    mobile_runtime::run();"));
    assert!(
        !lib.contains("mod project_config;"),
        "the removed project-owned sync module must not remain reachable"
    );

    for module in [
        "app_exit",
        "app_logs",
        "clipboard",
        "fonts",
        "language",
        "menu",
        "menu_labels",
        "opened_files",
        "shell_command",
        "text_file",
        "window_state",
        "windows",
    ] {
        let declaration = format!("#[cfg(desktop)]\nmod {module};");
        assert!(
            lib.contains(&declaration),
            "{module} should be desktop-only"
        );
    }
}

#[test]
fn builder_boundary_local_path_and_arbitrary_attachment_commands_are_desktop_only() {
    let modules = source("src/markdown_files.rs");
    assert!(modules.contains("#[cfg(desktop)]\npub(crate) mod attachment;"));
    assert!(modules.contains("mod resource_writer;"));

    let attachment = source("src/markdown_files/attachment.rs");
    assert!(attachment.contains("use super::resource_writer::{"));
    assert!(!attachment.contains("fn save_project_resource_bytes("));
    assert!(attachment
        .contains("#[cfg(desktop)]\n#[tauri::command]\npub(crate) fn save_clipboard_attachment"));
    assert!(attachment
        .contains("#[cfg(desktop)]\n#[tauri::command]\npub(crate) async fn import_local_file"));

    let image = source("src/markdown_files/image.rs");
    assert!(image.contains("use super::resource_writer::{"));
    assert!(image.contains("save_project_resource_bytes"));
    assert!(image.contains("save_standalone_resource_with_writer"));
    assert!(!image.contains("super::attachment::save_project_resource_bytes"));
    assert!(
        image.contains("#[cfg(desktop)]\n#[tauri::command]\npub(crate) fn read_local_image_file")
    );

    let resource_writer = source("src/markdown_files/resource_writer.rs");
    for forbidden in [
        "#[tauri::command]",
        "ClipboardAttachmentFile",
        "import_local_file",
        "save_clipboard_attachment",
    ] {
        assert!(
            !resource_writer.contains(forbidden),
            "shared resource writer contains desktop attachment surface {forbidden}"
        );
    }

    let path = source("src/markdown_files/path.rs");
    assert!(path
        .contains("#[cfg(desktop)]\n#[tauri::command]\npub(crate) fn canonical_local_file_path"));
    assert!(path.contains("#[cfg(desktop)]\npub(crate) fn markdown_open_path_for_path"));

    let types = source("src/markdown_files/types.rs");
    for declaration in [
        "pub(crate) struct MarkdownTemplateFile",
        "pub(crate) enum PandocExportFormat",
        "pub(crate) struct ClipboardAttachmentFile",
        "pub(crate) struct MarkdownImageFile",
        "pub(crate) enum MarkdownOpenPath",
    ] {
        let offset = types
            .find(declaration)
            .unwrap_or_else(|| panic!("missing {declaration}"));
        assert!(
            types[..offset].ends_with("#[cfg(desktop)]\n"),
            "{declaration} should be desktop-only"
        );
    }
}

#[test]
fn builder_boundary_has_no_project_owned_sync_commands_or_modules() {
    let desktop = source("src/desktop_runtime.rs");
    let desktop = desktop
        .split("#[cfg(test)]")
        .next()
        .expect("desktop runtime production source");
    let mobile = source("src/mobile_runtime.rs");
    let lib = source("src/lib.rs");
    for forbidden in [
        "project_config",
        "sync_project_folder",
        "test_project_sync_connection",
        "load_project_sync_status",
    ] {
        assert!(!desktop.contains(forbidden), "desktop retained {forbidden}");
        assert!(!mobile.contains(forbidden), "mobile retained {forbidden}");
        assert!(!lib.contains(forbidden), "library retained {forbidden}");
    }
}

#[test]
fn builder_boundary_generated_mobile_projects_ignore_signing_material() {
    let android_ignore = source("gen/android/.gitignore");
    for pattern in ["*.jks", "*.keystore"] {
        assert!(
            android_ignore.lines().any(|line| line == pattern),
            "Android ignore rules omitted {pattern}"
        );
    }

    let apple_ignore = source("gen/apple/.gitignore");
    for pattern in [
        "*.mobileprovision",
        "*.p12",
        "*.cer",
        "*.crt",
        "*.key",
        "*.pem",
    ] {
        assert!(
            apple_ignore.lines().any(|line| line == pattern),
            "Apple ignore rules omitted {pattern}"
        );
    }

    let ignored_paths = [
        "gen/android/release.jks",
        "gen/android/upload.keystore",
        "gen/apple/AppStore.mobileprovision",
        "gen/apple/distribution.p12",
        "gen/apple/distribution.cer",
        "gen/apple/distribution.crt",
        "gen/apple/private.key",
        "gen/apple/private.pem",
    ];
    for path in ignored_paths {
        let output = std::process::Command::new("git")
            .current_dir(manifest_root())
            .arg("check-ignore")
            .arg("--no-index")
            .arg("--quiet")
            .arg(path)
            .output()
            .expect("git check-ignore should run");
        assert!(output.status.success(), "{path} should be ignored");
    }
}

#[test]
fn builder_boundary_sidecar_has_an_explicit_non_default_feature_gate() {
    let manifest = source("Cargo.toml");
    assert!(manifest.contains("[features]\ndefault = []\ndesktop-sidecar = []"));
    assert!(manifest.contains("[[bin]]\nname = \"qingyu-mcp\""));
    assert!(manifest.contains("required-features = [\"desktop-sidecar\"]"));

    let preparation = source("../../../packages/scripts/src/prepare-qingyu-mcp-sidecar.mjs");
    assert!(preparation.contains("\"--features\",\n  \"desktop-sidecar\""));
}

#[test]
fn builder_boundary_common_dependencies_exclude_desktop_and_mcp_crates() {
    let manifest = source("Cargo.toml");
    let common = dependency_section(&manifest, "[dependencies]");
    for dependency in [
        "arboard",
        "dirs",
        "fontdb",
        "futures",
        "rmcp",
        "schemars",
        "sys-locale",
        "tauri-plugin-process",
        "tauri-plugin-single-instance",
        "tauri-plugin-updater",
        "tauri-plugin-window-state",
        "tokio-util",
        "trash",
    ] {
        assert!(
            !common
                .lines()
                .any(|line| line.starts_with(&format!("{dependency} ="))),
            "{dependency} should not be a common dependency"
        );
    }
    assert!(common.contains("base64 = \"0.22.1\""));
    assert!(common.contains("tauri = { version = \"2.11.0\", features = [\"protocol-asset\"] }"));
    assert!(common.contains("tauri-plugin-fs = \"2.5.1\""));
    assert!(common.contains("tauri-plugin-opener = \"2.5.4\""));

    let desktop = dependency_section(
        &manifest,
        "[target.'cfg(any(target_os = \"macos\", target_os = \"windows\", target_os = \"linux\"))'.dependencies]",
    );
    for dependency in [
        "arboard",
        "dirs",
        "fontdb",
        "futures",
        "rmcp",
        "schemars",
        "sys-locale",
        "tauri-plugin-process",
        "tauri-plugin-single-instance",
        "tauri-plugin-updater",
        "tauri-plugin-window-state",
        "tokio-util",
        "trash",
    ] {
        assert!(
            desktop
                .lines()
                .any(|line| line.starts_with(&format!("{dependency} ="))),
            "desktop dependency table omitted {dependency}"
        );
    }

    let macos = dependency_section(
        &manifest,
        "[target.'cfg(target_os = \"macos\")'.dependencies]",
    );
    assert!(macos.contains("tauri = { version = \"2.11.0\", features = [\"macos-private-api\"] }"));
}

#[test]
fn builder_boundary_build_script_skips_mobile_sidecar_slots() {
    let build = source("build.rs");
    assert!(build.contains("target.contains(\"android\") || target.contains(\"ios\")"));
    assert!(build.contains("return;"));
}

#[test]
fn builder_boundary_macos_private_api_stays_target_scoped_despite_tauri_build_validation() {
    let manifest = source("Cargo.toml");
    let common = dependency_section(&manifest, "[dependencies]");
    assert!(!common.contains("macos-private-api"));

    let macos = dependency_section(
        &manifest,
        "[target.'cfg(target_os = \"macos\")'.dependencies]",
    );
    assert!(macos.contains("features = [\"macos-private-api\"]"));

    let build = source("build.rs");
    let workaround = build
        .find("align_macos_private_api_manifest_check();")
        .expect("build script should align tauri-build's common-dependency validation");
    let tauri_build = build
        .find("tauri_build::build();")
        .expect("build script should invoke tauri-build");
    assert!(workaround < tauri_build);
    assert!(build.contains("std::env::var(\"CARGO_CFG_TARGET_OS\").as_deref() != Ok(\"macos\")"));
    assert!(build.contains("std::env::set_var(\"TAURI_CONFIG\", override_config)"));
}

#[test]
fn builder_boundary_capabilities_are_platform_disjoint() {
    let desktop = json("capabilities/main.json");
    assert_eq!(
        string_array_at(&desktop, "/platforms"),
        vec!["linux", "macOS", "windows"]
    );

    let mobile = json("capabilities/mobile.json");
    assert_eq!(
        string_array_at(&mobile, "/platforms"),
        vec!["iOS", "android"]
    );
    let permissions = permission_identifiers(&mobile);
    for forbidden in ["menu", "process", "updater", "window-state", "shell"] {
        assert!(
            permissions
                .iter()
                .all(|permission| !permission.contains(forbidden)),
            "mobile capability contains {forbidden} permission"
        );
    }

    for (name, capability) in [("desktop", &desktop), ("mobile", &mobile)] {
        let opener = capability
            .pointer("/permissions")
            .and_then(serde_json::Value::as_array)
            .and_then(|permissions| {
                permissions.iter().find(|permission| {
                    permission
                        .pointer("/identifier")
                        .and_then(serde_json::Value::as_str)
                        == Some("opener:allow-open-url")
                })
            })
            .unwrap_or_else(|| {
                panic!("{name} opener permission should have an explicit URL scope")
            });
        assert_eq!(
            opener.pointer("/allow"),
            Some(&serde_json::json!([
                { "url": "http://*" },
                { "url": "https://*" }
            ])),
            "{name} opener scope should allow only HTTP and HTTPS"
        );
    }
}

#[test]
fn builder_boundary_sidecar_and_file_associations_are_desktop_config_only() {
    let base = json("tauri.conf.json");
    assert!(base.pointer("/bundle/externalBin").is_none());
    assert!(base.pointer("/bundle/fileAssociations").is_none());
    assert!(base.pointer("/app/macOSPrivateApi").is_none());
    assert_eq!(
        string_array_at(&base, "/app/security/capabilities"),
        vec!["main", "mobile"]
    );
    assert_eq!(
        base.pointer("/build/beforeBuildCommand"),
        Some(&serde_json::json!("pnpm --dir ../.. prepare:mobile-build"))
    );

    let package = json("../../../package.json");
    assert_eq!(
        package.pointer("/scripts/prepare:mobile-build"),
        Some(&serde_json::json!("pnpm --filter @markra/desktop build"))
    );

    for platform in ["macos", "windows", "linux"] {
        let config = json(&format!("tauri.{platform}.conf.json"));
        assert_eq!(
            string_array_at(&config, "/bundle/externalBin"),
            vec!["binaries/qingyu-mcp"]
        );
        assert!(config.pointer("/bundle/fileAssociations").is_some());
        assert_eq!(
            config.pointer("/build/beforeBuildCommand"),
            Some(&serde_json::json!("pnpm --dir ../.. prepare:desktop-build"))
        );
    }

    let macos = json("tauri.macos.conf.json");
    assert_eq!(
        macos.pointer("/app/macOSPrivateApi"),
        Some(&serde_json::json!(true))
    );
}

#[test]
fn builder_boundary_theme_quarantine_cleanup_has_platform_safe_root_deletion() {
    let activation = source("src/themes/activation.rs");
    let lease_cleanup = activation
        .split("fn remove_activation_lease_with_hooks")
        .nth(1)
        .and_then(|source| source.split("fn reserve_quarantine_name").next())
        .expect("theme activation should define lease quarantine cleanup");
    let retained_close = lease_cleanup
        .find("drop(retained);")
        .expect("the retained source handle should be closed before quarantine rename");
    let quarantine_rename = lease_cleanup
        .find("reserve_quarantine_name")
        .expect("lease cleanup should atomically rename into quarantine");
    assert!(
        retained_close < quarantine_rename,
        "Windows cannot quarantine a directory while its no-share-delete handle is open"
    );

    let cleanup = source("src/themes/activation_cleanup.rs");
    assert!(cleanup.contains("MAX_PACKAGE_ENTRIES"));
    assert!(cleanup.contains("const MAX_CLEANUP_DEPTH: usize = 16;"));
    assert!(cleanup.contains("let mut pending"));
    assert!(cleanup.contains("entries.into_iter().rev()"));
    assert!(cleanup.contains("drop(quarantine);"));
    assert!(cleanup.contains("parent.remove_dir(name)"));
    assert!(!cleanup.contains("remove_open_dir_all"));
    assert!(!cleanup.contains("remove_quarantined_descendants"));
    assert!(
        !cleanup.contains("collect::<io::Result<Vec"),
        "cleanup must not collect an unbounded directory width before enforcing limits"
    );
}
