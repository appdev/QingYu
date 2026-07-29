pub(crate) fn common_tauri_dependency_enables_macos_private_api(manifest: &str) -> bool {
    let mut in_common_dependencies = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_common_dependencies = line == "[dependencies]";
            continue;
        }
        if !in_common_dependencies {
            continue;
        }
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        if name.trim() == "tauri" {
            return value.contains("\"macos-private-api\"");
        }
    }
    false
}

pub(crate) fn macos_private_api_validation_override(
    existing: Option<&str>,
    enabled_in_common_dependency: bool,
) -> Result<String, String> {
    let mut override_config = match existing {
        Some(value) => serde_json::from_str::<serde_json::Value>(value)
            .map_err(|error| format!("TAURI_CONFIG must be a valid JSON object: {error}"))?,
        None => serde_json::json!({}),
    };
    let root = override_config
        .as_object_mut()
        .ok_or_else(|| "TAURI_CONFIG must be a JSON object".to_string())?;
    let app = root
        .entry("app")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| "TAURI_CONFIG app must be a JSON object".to_string())?;
    app.insert(
        "macOSPrivateApi".to_string(),
        serde_json::Value::Bool(enabled_in_common_dependency),
    );
    Ok(override_config.to_string())
}

pub(crate) fn desktop_sidecar_validation_override(
    existing: Option<&str>,
    kernel_ready: bool,
) -> Result<String, String> {
    let mut override_config = match existing {
        Some(value) => serde_json::from_str::<serde_json::Value>(value)
            .map_err(|error| format!("TAURI_CONFIG must be a valid JSON object: {error}"))?,
        None => serde_json::json!({}),
    };
    let root = override_config
        .as_object_mut()
        .ok_or_else(|| "TAURI_CONFIG must be a JSON object".to_string())?;
    let bundle = root
        .entry("bundle")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| "TAURI_CONFIG bundle must be a JSON object".to_string())?;
    let binaries = bundle
        .entry("externalBin")
        .or_insert_with(|| serde_json::json!([]))
        .as_array_mut()
        .ok_or_else(|| "TAURI_CONFIG bundle.externalBin must be an array".to_string())?;
    let mcp_sidecar = serde_json::json!("binaries/qingyu-mcp");
    let kernel_sidecar = serde_json::json!("binaries/qingyu-kernel");

    if !kernel_ready {
        binaries.retain(|binary| binary != &kernel_sidecar);
    }
    if !binaries.contains(&mcp_sidecar) {
        binaries.push(mcp_sidecar);
    }
    if kernel_ready && !binaries.contains(&kernel_sidecar) {
        binaries.push(kernel_sidecar);
    }
    Ok(override_config.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        common_tauri_dependency_enables_macos_private_api, desktop_sidecar_validation_override,
        macos_private_api_validation_override,
    };

    #[test]
    fn detects_macos_private_api_only_in_the_common_dependency_table() {
        let target_scoped_only = r#"
[dependencies]
tauri = { version = "2", features = ["protocol-asset"] }

[target.'cfg(target_os = "macos")'.dependencies]
tauri = { version = "2", features = ["macos-private-api"] }
"#;
        assert!(!common_tauri_dependency_enables_macos_private_api(
            target_scoped_only
        ));

        let cli_adjusted = r#"
[dependencies]
tauri = { version = "2", features = ["macos-private-api", "protocol-asset"] }
"#;
        assert!(common_tauri_dependency_enables_macos_private_api(
            cli_adjusted
        ));
    }

    #[test]
    fn absent_tauri_config_gets_only_the_validation_override() {
        let configured = macos_private_api_validation_override(None, false)
            .expect("an absent override should use an empty object");
        let parsed: serde_json::Value =
            serde_json::from_str(&configured).expect("generated override should be valid JSON");

        assert_eq!(
            parsed,
            serde_json::json!({ "app": { "macOSPrivateApi": false } })
        );
    }

    #[test]
    fn valid_tauri_config_preserves_existing_keys() {
        let configured = macos_private_api_validation_override(
            Some(
                r#"{"bundle":{"active":false},"app":{"windows":[{"label":"main"}],"macOSPrivateApi":true}}"#,
            ),
            false,
        )
        .expect("a valid object override should be accepted");
        let parsed: serde_json::Value =
            serde_json::from_str(&configured).expect("generated override should be valid JSON");

        assert_eq!(
            parsed.pointer("/bundle/active"),
            Some(&serde_json::json!(false))
        );
        assert_eq!(
            parsed.pointer("/app/windows/0/label"),
            Some(&serde_json::json!("main"))
        );
        assert_eq!(
            parsed.pointer("/app/macOSPrivateApi"),
            Some(&serde_json::json!(false))
        );
    }

    #[test]
    fn cli_adjusted_common_dependency_keeps_private_api_enabled() {
        let configured = macos_private_api_validation_override(None, true)
            .expect("a CLI-adjusted manifest should produce an override");
        let parsed: serde_json::Value =
            serde_json::from_str(&configured).expect("generated override should be valid JSON");

        assert_eq!(
            parsed.pointer("/app/macOSPrivateApi"),
            Some(&serde_json::json!(true))
        );
    }

    #[test]
    fn malformed_tauri_config_is_rejected() {
        let error = macos_private_api_validation_override(Some("{"), false)
            .expect_err("malformed JSON must not be silently replaced");

        assert!(error.contains("valid JSON object"));
    }

    #[test]
    fn non_object_tauri_config_is_rejected() {
        for input in ["null", "[]", r#""string""#] {
            let error = macos_private_api_validation_override(Some(input), false)
                .expect_err("non-object JSON must not be silently replaced");
            assert!(error.contains("JSON object"), "{input}: {error}");
        }
    }

    #[test]
    fn non_object_app_override_is_rejected() {
        let error = macos_private_api_validation_override(Some(r#"{"app":true}"#), false)
            .expect_err("the app override must remain an object");

        assert!(error.contains("TAURI_CONFIG app must be a JSON object"));
    }

    #[test]
    fn desktop_sidecar_override_includes_kernel_only_after_real_preparation() {
        for (kernel_ready, expected) in [
            (false, serde_json::json!(["binaries/qingyu-mcp"])),
            (
                true,
                serde_json::json!(["binaries/qingyu-mcp", "binaries/qingyu-kernel"]),
            ),
        ] {
            let configured = desktop_sidecar_validation_override(None, kernel_ready)
                .expect("an absent override should use an empty object");
            let parsed: serde_json::Value =
                serde_json::from_str(&configured).expect("generated override should be valid JSON");

            assert_eq!(parsed.pointer("/bundle/externalBin"), Some(&expected));
        }
    }

    #[test]
    fn desktop_sidecar_override_preserves_other_bundle_and_root_keys() {
        let configured = desktop_sidecar_validation_override(
            Some(r#"{"bundle":{"active":false},"plugins":{"updater":{"pubkey":"value"}}}"#),
            true,
        )
        .expect("a valid object override should be accepted");
        let parsed: serde_json::Value =
            serde_json::from_str(&configured).expect("generated override should be valid JSON");

        assert_eq!(
            parsed.pointer("/bundle/active"),
            Some(&serde_json::json!(false))
        );
        assert_eq!(
            parsed.pointer("/plugins/updater/pubkey"),
            Some(&serde_json::json!("value"))
        );
        assert_eq!(
            parsed.pointer("/bundle/externalBin"),
            Some(&serde_json::json!([
                "binaries/qingyu-mcp",
                "binaries/qingyu-kernel"
            ]))
        );
    }

    #[test]
    fn desktop_sidecar_override_preserves_existing_external_binaries() {
        let configured = desktop_sidecar_validation_override(
            Some(
                r#"{"bundle":{"externalBin":["binaries/upstream-helper","binaries/qingyu-kernel"]}}"#,
            ),
            false,
        )
        .expect("an existing external binary list should be extended in place");
        let parsed: serde_json::Value =
            serde_json::from_str(&configured).expect("generated override should be valid JSON");

        assert_eq!(
            parsed.pointer("/bundle/externalBin"),
            Some(&serde_json::json!([
                "binaries/upstream-helper",
                "binaries/qingyu-mcp"
            ]))
        );
    }

    #[test]
    fn desktop_sidecar_override_does_not_duplicate_managed_binaries() {
        let configured = desktop_sidecar_validation_override(
            Some(
                r#"{"bundle":{"externalBin":["binaries/qingyu-mcp","binaries/qingyu-kernel","binaries/upstream-helper"]}}"#,
            ),
            true,
        )
        .expect("already configured managed binaries should remain singular");
        let parsed: serde_json::Value =
            serde_json::from_str(&configured).expect("generated override should be valid JSON");

        assert_eq!(
            parsed.pointer("/bundle/externalBin"),
            Some(&serde_json::json!([
                "binaries/qingyu-mcp",
                "binaries/qingyu-kernel",
                "binaries/upstream-helper"
            ]))
        );
    }

    #[test]
    fn desktop_sidecar_override_rejects_non_array_external_binaries() {
        let error = desktop_sidecar_validation_override(
            Some(r#"{"bundle":{"externalBin":"binaries/upstream-helper"}}"#),
            true,
        )
        .expect_err("an invalid external binary list must not be overwritten");

        assert!(error.contains("TAURI_CONFIG bundle.externalBin must be an array"));
    }
}
