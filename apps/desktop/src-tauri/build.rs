mod build_support;

fn main() {
    ensure_mcp_sidecar_slot();
    align_desktop_sidecar_manifest_check();
    align_macos_private_api_manifest_check();
    tauri_build::build();
}

fn align_desktop_sidecar_manifest_check() {
    let Ok(target) = std::env::var("TARGET") else {
        return;
    };
    if target.contains("android") || target.contains("ios") {
        return;
    }
    let suffix = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    let path = std::path::Path::new("binaries").join(format!("qingyu-kernel-{target}{suffix}"));
    let kernel_ready = kernel_sidecar_is_ready(&path, target.contains("windows"));
    if !kernel_ready && std::env::var_os("MARKRA_DESKTOP_TARGET").is_some() {
        panic!(
            "prepared Kernel sidecar is missing, empty, linked, or non-executable: {}",
            path.display()
        );
    }

    let current_config = current_tauri_config();
    let override_config =
        build_support::desktop_sidecar_validation_override(current_config.as_deref(), kernel_ready)
            .unwrap_or_else(|error| panic!("{error}"));
    std::env::set_var("TAURI_CONFIG", override_config);
}

fn kernel_sidecar_is_ready(path: &std::path::Path, _windows_target: bool) -> bool {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() == 0 {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if !_windows_target && metadata.permissions().mode() & 0o111 == 0 {
            return false;
        }
    }
    true
}

fn align_macos_private_api_manifest_check() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }

    // tauri-build 2.6 validates only the common dependency when `tauri` also
    // has a target-scoped dependency entry. Keep the real setting in the macOS
    // config and the real feature in the macOS dependency, while preventing
    // that manifest-only check from demanding the feature on mobile targets.
    let current_config = current_tauri_config();
    let manifest = std::fs::read_to_string("Cargo.toml")
        .unwrap_or_else(|error| panic!("failed to read Cargo.toml: {error}"));
    let enabled_in_common_dependency =
        build_support::common_tauri_dependency_enables_macos_private_api(&manifest);
    let override_config = build_support::macos_private_api_validation_override(
        current_config.as_deref(),
        enabled_in_common_dependency,
    )
    .unwrap_or_else(|error| panic!("{error}"));
    std::env::set_var("TAURI_CONFIG", override_config);
}

fn current_tauri_config() -> Option<String> {
    match std::env::var("TAURI_CONFIG") {
        Ok(value) => Some(value),
        Err(std::env::VarError::NotPresent) => None,
        Err(std::env::VarError::NotUnicode(_)) => {
            panic!("TAURI_CONFIG must be valid UTF-8 JSON object")
        }
    }
}

fn ensure_mcp_sidecar_slot() {
    let Ok(target) = std::env::var("TARGET") else {
        return;
    };
    if target.contains("android") || target.contains("ios") {
        return;
    }
    let suffix = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    let directory = std::path::Path::new("binaries");
    let path = directory.join(format!("qingyu-mcp-{target}{suffix}"));
    if path.exists() || std::fs::create_dir_all(directory).is_err() {
        return;
    }
    let _placeholder = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path);
}
