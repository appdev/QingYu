use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use quick_xml::events::Event;
use quick_xml::Reader;

fn manifest_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn source(relative: impl AsRef<Path>) -> String {
    let path = manifest_root().join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
}

fn json(relative: &str) -> serde_json::Value {
    serde_json::from_str(&source(relative))
        .unwrap_or_else(|error| panic!("{relative} should be valid JSON: {error}"))
}

#[derive(Debug)]
struct XmlElement {
    name: String,
    attributes: BTreeMap<String, String>,
}

fn xml_elements(relative: &str) -> Vec<XmlElement> {
    let document = source(relative);
    let mut reader = Reader::from_str(&document);
    reader.config_mut().trim_text(true);
    let mut elements = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(element) | Event::Empty(element)) => {
                let name = String::from_utf8_lossy(element.name().as_ref()).into_owned();
                let attributes = element
                    .attributes()
                    .map(|attribute| {
                        let attribute = attribute.expect("manifest attributes should parse");
                        let key = String::from_utf8_lossy(attribute.key.as_ref()).into_owned();
                        let value = attribute
                            .decode_and_unescape_value(reader.decoder())
                            .expect("manifest attribute values should parse")
                            .into_owned();
                        (key, value)
                    })
                    .collect();
                elements.push(XmlElement { name, attributes });
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => panic!("{relative} should be valid XML: {error}"),
        }
    }

    elements
}

fn application_attribute<'a>(elements: &'a [XmlElement], name: &str) -> Option<&'a str> {
    elements
        .iter()
        .find(|element| element.name == "application")
        .and_then(|element| element.attributes.get(name))
        .map(String::as_str)
}

fn quoted_live_s3_environment_names(source: &str) -> BTreeSet<String> {
    source
        .split('"')
        .filter(|segment| segment.starts_with("MARKRA_TEST_S3_") && !segment.ends_with('*'))
        .map(str::to_string)
        .collect()
}

fn plist_string_array_after_key(document: &str, key: &str) -> BTreeSet<String> {
    let marker = format!("<key>{key}</key>");
    let key_offset = document
        .find(&marker)
        .unwrap_or_else(|| panic!("plist should contain {key}"));
    let value = &document[key_offset + marker.len()..];
    let array_start = value
        .find("<array>")
        .unwrap_or_else(|| panic!("plist key {key} should contain an array"));
    let array = &value[array_start + "<array>".len()..];
    let array_end = array
        .find("</array>")
        .unwrap_or_else(|| panic!("plist key {key} array should close"));

    array[..array_end]
        .split("<string>")
        .skip(1)
        .map(|entry| {
            entry
                .split_once("</string>")
                .map(|(value, _)| value.to_string())
                .expect("plist string should close")
        })
        .collect()
}

#[test]
fn mobile_platform_config_android_keeps_internet_and_limits_cleartext_to_loopback() {
    let main = xml_elements("gen/android/app/src/main/AndroidManifest.xml");
    let permissions = main
        .iter()
        .filter(|element| element.name == "uses-permission")
        .filter_map(|element| element.attributes.get("android:name"))
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    assert!(permissions.contains("android.permission.INTERNET"));
    assert_ne!(
        application_attribute(&main, "android:usesCleartextTraffic"),
        Some("true"),
        "the release/main manifest must not globally allow cleartext traffic"
    );
    assert_eq!(
        application_attribute(&main, "android:usesCleartextTraffic"),
        Some("false"),
        "the release/main manifest must explicitly keep cleartext disabled"
    );
    assert_eq!(
        application_attribute(&main, "android:networkSecurityConfig"),
        Some("@xml/network_security_config"),
        "the release manifest must install the narrow loopback policy"
    );

    let network_config_path = "gen/android/app/src/main/res/xml/network_security_config.xml";
    let network_config = source(network_config_path);
    let network_elements = xml_elements(network_config_path);
    assert_eq!(
        network_elements
            .iter()
            .filter(|element| element.name == "base-config")
            .filter_map(|element| element.attributes.get("cleartextTrafficPermitted"))
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["false"]
    );
    assert_eq!(
        network_elements
            .iter()
            .filter(|element| element.name == "domain-config")
            .filter_map(|element| element.attributes.get("cleartextTrafficPermitted"))
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["true"]
    );
    assert_eq!(
        network_elements
            .iter()
            .filter(|element| element.name == "domain")
            .filter_map(|element| element.attributes.get("includeSubdomains"))
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["false"]
    );
    assert!(network_config.contains(">127.0.0.1</domain>"));
    assert_eq!(network_config.matches("<domain ").count(), 1);
    for forbidden in ["*", "0.0.0.0", "localhost", "192.168.", "10.", "172."] {
        assert!(
            !network_config.contains(forbidden),
            "the release network policy contains non-loopback destination {forbidden}"
        );
    }

    let debug = xml_elements("gen/android/app/src/debug/AndroidManifest.xml");
    assert_eq!(
        application_attribute(&debug, "android:usesCleartextTraffic"),
        Some("true")
    );
    assert_eq!(
        application_attribute(&debug, "tools:replace"),
        Some("android:usesCleartextTraffic"),
        "the debug source set should explicitly override the release false value"
    );

    let gradle = source("gen/android/app/build.gradle.kts");
    assert!(
        !gradle.contains("usesCleartextTraffic"),
        "Gradle placeholders must not duplicate the source-set manifest policy"
    );
}

#[test]
fn mobile_platform_config_android_exposes_no_import_share_or_storage_surface() {
    let main = xml_elements("gen/android/app/src/main/AndroidManifest.xml");
    let debug = xml_elements("gen/android/app/src/debug/AndroidManifest.xml");
    let elements = main.iter().chain(&debug).collect::<Vec<_>>();
    let forbidden_permissions = [
        "android.permission.MANAGE_EXTERNAL_STORAGE",
        "android.permission.READ_EXTERNAL_STORAGE",
        "android.permission.WRITE_EXTERNAL_STORAGE",
    ];
    let forbidden_actions = [
        "android.intent.action.SEND",
        "android.intent.action.SEND_MULTIPLE",
        "android.intent.action.VIEW",
    ];

    for element in &elements {
        let android_name = element.attributes.get("android:name").map(String::as_str);
        assert!(
            !forbidden_permissions.contains(&android_name.unwrap_or_default()),
            "Android declares broad storage permission {android_name:?}"
        );
        assert!(
            !forbidden_actions.contains(&android_name.unwrap_or_default()),
            "Android declares unsupported import/share action {android_name:?}"
        );
        assert_ne!(
            element
                .attributes
                .get("android:mimeType")
                .map(String::as_str),
            Some("text/markdown")
        );
    }
}

#[test]
fn mobile_platform_config_android_intercepts_back_before_activity_destruction() {
    let activity = source("gen/android/app/src/main/java/dev/markra/app/MainActivity.kt");

    assert!(activity.contains("onBackPressedDispatcher.addCallback"));
    assert!(activity.contains("override fun onWebViewCreate"));
    assert!(activity.contains("evaluateJavascript"));
    assert!(activity.contains("qingyu://mobile-back-requested"));
    assert!(!activity.contains("moveTaskToBack"));
    assert!(!activity.contains("finish()"));
}

#[test]
fn mobile_platform_config_apple_allows_only_local_networking_exception() {
    let project = source("gen/apple/project.yml");
    let info_plist = source("gen/apple/qingyu_iOS/Info.plist");
    assert!(project.contains("NSLocalNetworkUsageDescription:"));
    assert!(project.contains("NSAppTransportSecurity:\n"));
    assert!(project.contains("NSAllowsLocalNetworking: true"));
    assert!(!project.contains("NSAllowsArbitraryLoads"));
    assert!(info_plist.contains("<key>NSLocalNetworkUsageDescription</key>"));
    assert!(info_plist.contains("<key>NSAppTransportSecurity</key>"));
    assert!(info_plist.contains("<key>NSAllowsLocalNetworking</key>"));
    assert!(!info_plist.contains("<key>NSAllowsArbitraryLoads</key>"));
    assert!(project.contains("CFBundleAllowMixedLocalizations: true"));
    assert!(info_plist.contains("<key>CFBundleAllowMixedLocalizations</key>\n\t<true/>"));
    assert_eq!(
        plist_string_array_after_key(&info_plist, "CFBundleLocalizations"),
        BTreeSet::from([
            "de".to_string(),
            "en".to_string(),
            "es".to_string(),
            "fr".to_string(),
            "it".to_string(),
            "ja".to_string(),
            "ko".to_string(),
            "pt-BR".to_string(),
            "ru".to_string(),
            "zh-Hans".to_string(),
            "zh-Hant".to_string(),
        ])
    );

    for forbidden in [
        "CFBundleDocumentTypes",
        "UTExportedTypeDeclarations",
        "UTImportedTypeDeclarations",
        "NSExtensionPointIdentifier",
        "com.apple.share-services",
        "public.markdown",
        "net.daringfireball.markdown",
        "DEVELOPMENT_TEAM",
        "CODE_SIGN_IDENTITY",
        "PROVISIONING_PROFILE",
    ] {
        assert!(
            !project.contains(forbidden) && !info_plist.contains(forbidden),
            "Apple project exposes forbidden metadata {forbidden}"
        );
    }
}

#[test]
fn mobile_platform_config_apple_does_not_bundle_generated_rust_archive_as_resource() {
    let project = source("gen/apple/project.yml");
    let generated_project = source("gen/apple/qingyu.xcodeproj/project.pbxproj");

    assert!(
        !project.contains("- path: Externals"),
        "generated Rust archives must not be scanned as Xcode sources"
    );
    assert!(
        !generated_project.contains("libapp.a in Resources"),
        "the Rust static library must be linked, not copied as an app resource"
    );
}

#[test]
fn mobile_platform_config_overlays_only_add_the_in_process_kernel_csp() {
    for platform in ["android", "ios"] {
        let config = json(&format!("tauri.{platform}.conf.json"));
        let object = config
            .as_object()
            .unwrap_or_else(|| panic!("tauri.{platform}.conf.json should be an object"));
        assert_eq!(
            object.keys().collect::<Vec<_>>(),
            vec!["$schema", "app"],
            "the {platform} overlay should contain only its schema and security override"
        );
        assert_eq!(
            config.pointer("/$schema"),
            Some(&serde_json::json!("https://schema.tauri.app/config/2"))
        );
        let app = config
            .pointer("/app")
            .and_then(serde_json::Value::as_object)
            .unwrap_or_else(|| panic!("the {platform} overlay should contain app security"));
        assert_eq!(app.keys().collect::<Vec<_>>(), vec!["security"]);
        let security = config
            .pointer("/app/security")
            .and_then(serde_json::Value::as_object)
            .unwrap_or_else(|| panic!("the {platform} overlay should contain security"));
        assert_eq!(security.keys().collect::<Vec<_>>(), vec!["csp"]);
        let csp = config
            .pointer("/app/security/csp")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| panic!("the {platform} overlay should contain a string CSP"));
        assert!(csp.contains("http://127.0.0.1:*"));
        assert!(csp.contains("ws://127.0.0.1:*"));
        for forbidden in [
            "connect-src http:",
            "connect-src ws:",
            "http://localhost:*",
            "ws://localhost:*",
            "192.168.",
        ] {
            assert!(
                !csp.contains(forbidden),
                "the {platform} CSP contains broad or non-loopback transport {forbidden}"
            );
        }
        for pointer in [
            "/bundle/externalBin",
            "/bundle/fileAssociations",
            "/app/macOSPrivateApi",
            "/plugins/updater/endpoints",
        ] {
            assert!(
                config.pointer(pointer).is_none(),
                "the {platform} overlay must not define {pointer}"
            );
        }
    }
}

#[test]
fn mobile_platform_config_commits_no_external_endpoint_credentials_or_signing_material() {
    let metadata = [
        source("tauri.android.conf.json"),
        source("tauri.ios.conf.json"),
        source("gen/android/app/build.gradle.kts"),
        source("gen/android/app/src/main/AndroidManifest.xml"),
        source("gen/android/app/src/debug/AndroidManifest.xml"),
        source("gen/apple/project.yml"),
        source("gen/apple/qingyu_iOS/Info.plist"),
    ]
    .join("\n");
    let lowercase = metadata.to_ascii_lowercase();
    for forbidden in [
        "markra_test_s3_",
        "access_key",
        "secret_key",
        "secret_access",
        "authorization",
        "192.168.",
        "http://localhost:",
        "ws://localhost:",
        "minio",
        ".amazonaws.com",
        "signingconfig",
        ".jks",
        ".keystore",
        ".mobileprovision",
        ".p12",
    ] {
        assert!(
            !lowercase.contains(forbidden),
            "mobile metadata contains forbidden material {forbidden}"
        );
    }
}

#[test]
fn mobile_platform_config_live_s3_environment_contract_is_exact() {
    let live_tests = source("src/remote_sync/live_tests.rs");
    assert_eq!(
        quoted_live_s3_environment_names(&live_tests),
        BTreeSet::from([
            "MARKRA_TEST_S3_ACCESS_KEY_ID".to_string(),
            "MARKRA_TEST_S3_BUCKET".to_string(),
            "MARKRA_TEST_S3_ENDPOINT".to_string(),
            "MARKRA_TEST_S3_PREFIX_ROOT".to_string(),
            "MARKRA_TEST_S3_REGION".to_string(),
            "MARKRA_TEST_S3_SECRET_ACCESS_KEY".to_string(),
        ])
    );
    assert!(live_tests.contains("remote_path: format!(\"{}/{}/{scenario}/app\""));
    for forbidden in [
        "std::fs",
        "fs::write",
        "env::set_var",
        "println!",
        "eprintln!",
        "dbg!",
        "{config:?}",
        "{self:?}",
    ] {
        assert!(
            !live_tests.contains(forbidden),
            "live S3 fixture can persist or print credential-bearing state through {forbidden}"
        );
    }
}

#[test]
fn mobile_platform_config_has_no_reset_or_workspace_switch_command() {
    let runtime = source("src/mobile_runtime.rs");
    for forbidden in [
        "reset_project_config",
        "load_project_config",
        "sync_project_folder",
        "test_project_sync_connection",
        "reset_managed_workspace",
        "switch_managed_workspace",
    ] {
        assert!(
            !runtime.contains(forbidden),
            "mobile runtime exposes forbidden command {forbidden}"
        );
    }
}

#[test]
fn mobile_platform_config_exposes_no_desktop_mcp_commands() {
    let library = source("src/lib.rs");
    assert!(library.contains("#[cfg(not(mobile))]\nmod mcp;"));
    assert!(library.contains(
        "#[cfg(all(not(mobile), any(desktop, feature = \"desktop-sidecar\")))]\npub async fn run_mcp_bridge"
    ));
    assert!(!library.contains("#[cfg(any(not(mobile), feature = \"desktop-sidecar\"))]"));
    assert!(!library.contains(
        "#[cfg(any(desktop, feature = \"desktop-sidecar\"))]\npub async fn run_mcp_bridge"
    ));
    let markdown_files = source("src/markdown_files.rs");
    assert!(markdown_files.contains(
        "#[cfg(all(not(mobile), any(desktop, feature = \"desktop-sidecar\")))]\n#[allow(dead_code)]\nmod service;"
    ));
    assert!(!markdown_files.contains("#[cfg(any(desktop, feature = \"desktop-sidecar\"))]"));
    let native_runtime = source("src/mobile_runtime.rs");
    for forbidden in [
        "get_mcp_settings",
        "update_mcp_settings",
        "set_mcp_primary_workspace",
        "get_mcp_health",
        "list_mcp_audit_entries",
        "clear_mcp_audit_entries",
        "crate::mcp::",
    ] {
        assert!(
            !native_runtime.contains(forbidden),
            "mobile native runtime exposes desktop MCP capability {forbidden}"
        );
    }

    let frontend_runtime = source("../src/runtime/mobile.ts");
    assert!(frontend_runtime.contains("policyAvailable: false"));
    assert!(frontend_runtime.contains("localServiceAvailable: false"));
    for forbidden in [
        "./tauri/mcp-policy",
        "./tauri/mcp\"",
        "setPrimaryWorkspace:",
        "getHealth:",
        "listAuditEntries:",
        "clearAuditEntries:",
    ] {
        assert!(
            !frontend_runtime.contains(forbidden),
            "mobile frontend runtime ships desktop MCP adapter {forbidden}"
        );
    }

    let mcp_module = source("src/mcp/mod.rs");
    assert!(mcp_module.contains("pub(crate) mod config;"));
    let workspace_authority = source("src/mcp/workspaces.rs");
    for forbidden in ["managed_workspace_root", "allowed_roots"] {
        assert!(
            !mcp_module.contains(forbidden) && !workspace_authority.contains(forbidden),
            "mobile-specific MCP workspace authority residue remains: {forbidden}"
        );
    }
}
