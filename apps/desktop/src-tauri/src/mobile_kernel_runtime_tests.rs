use std::{fs, time::Duration};

use qingyu_kernel::{
    composition::compose_fixed_mobile_kernel, config::KernelConfig, paths::KernelPaths,
};
use serde_json::json;
use tempfile::tempdir;

use crate::mobile_kernel_runtime::{
    configured_mobile_renderer_origin, validated_mobile_renderer_origin, MobileKernelRuntimeState,
};

const WEBVIEW_ORIGIN: &str = "tauri://localhost";

#[tokio::test]
async fn mobile_runtime_publishes_only_a_ready_memory_bootstrap_and_revokes_it_before_stop() {
    let temporary = tempdir().unwrap();
    let app_data = temporary.path().join("app-data");
    let cache = temporary.path().join("cache");
    fs::create_dir(&app_data).unwrap();
    fs::create_dir(&cache).unwrap();
    let launch = compose_fixed_mobile_kernel(
        KernelConfig::generate().unwrap(),
        KernelPaths::mobile(&app_data, &cache, "primary").unwrap(),
        "QingYu",
    )
    .await
    .unwrap();
    let runtime = MobileKernelRuntimeState::new(Duration::from_secs(2), WEBVIEW_ORIGIN).unwrap();

    assert_eq!(
        serde_json::to_value(runtime.read_bootstrap(WEBVIEW_ORIGIN).unwrap()).unwrap(),
        json!({
            "bootstrapVersion": 1,
            "generation": "1",
            "status": "starting"
        })
    );
    runtime.start(launch, WEBVIEW_ORIGIN).await.unwrap();
    let ready = serde_json::to_value(runtime.read_bootstrap(WEBVIEW_ORIGIN).unwrap()).unwrap();
    assert_eq!(ready["bootstrapVersion"], 1);
    assert_eq!(ready["generation"], "1");
    assert_eq!(ready["status"], "ready");
    assert!(ready["port"].as_u64().is_some_and(|port| port > 0));
    assert!(ready["instanceId"].as_str().is_some());
    assert!(ready["credential"]
        .as_str()
        .is_some_and(|value| value.len() == 43));
    assert!(runtime.read_bootstrap("https://attacker.invalid").is_err());

    runtime.stop().await.unwrap();
    let stopped = serde_json::to_value(runtime.read_bootstrap(WEBVIEW_ORIGIN).unwrap()).unwrap();
    assert_eq!(
        stopped,
        json!({
            "bootstrapVersion": 1,
            "generation": "1",
            "status": "dormant"
        })
    );
    assert!(!stopped
        .to_string()
        .contains(ready["credential"].as_str().unwrap()));
}

#[test]
fn mobile_renderer_origin_is_exact_for_dev_android_and_ios() {
    let dev = tauri::Url::parse("http://127.0.0.1:1420/index.html").unwrap();
    assert_eq!(
        configured_mobile_renderer_origin(true, Some(&dev), false).unwrap(),
        "http://127.0.0.1:1420"
    );
    assert_eq!(
        configured_mobile_renderer_origin(false, None, true).unwrap(),
        "http://tauri.localhost"
    );
    assert_eq!(
        configured_mobile_renderer_origin(false, None, false).unwrap(),
        "tauri://localhost"
    );

    let ios = tauri::Url::parse("tauri://localhost/editor").unwrap();
    assert_eq!(
        validated_mobile_renderer_origin("main", "tauri://localhost", &ios).unwrap(),
        "tauri://localhost"
    );
    assert!(validated_mobile_renderer_origin("settings", "tauri://localhost", &ios).is_err());
    assert!(validated_mobile_renderer_origin(
        "main",
        "tauri://localhost",
        &tauri::Url::parse("https://attacker.invalid").unwrap(),
    )
    .is_err());
    assert!(validated_mobile_renderer_origin(
        "main",
        "tauri://localhost",
        &tauri::Url::parse("file:///index.html").unwrap(),
    )
    .is_err());
}

#[test]
fn terminal_exit_has_one_stop_owner_and_opens_only_after_settlement() {
    let runtime = MobileKernelRuntimeState::new(Duration::from_secs(2), WEBVIEW_ORIGIN).unwrap();

    assert!(runtime.begin_terminal_exit());
    assert!(!runtime.begin_terminal_exit());
    assert!(!runtime.terminal_exit_is_ready());
    runtime.mark_terminal_exit_ready();
    assert!(runtime.terminal_exit_is_ready());
    assert!(!runtime.begin_terminal_exit());
}
