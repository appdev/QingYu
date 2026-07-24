use super::{
    audit::{AuditEvent, AuditOutcome, AuditSink},
    bridge::{
        run_bridge, test_app_launch_request, test_connect_with_launch, test_indeterminate_error,
        AppLauncher, BridgeConfig, BridgeError,
    },
    config::{
        ConfirmationPolicy, DeletionPolicy, DryRunPolicy, McpConfig, McpConfigDocument,
        McpConfigManager, SyncAfterWritePolicy, SyncExecutionPolicy, ToolCapability,
    },
    confirmation::{ConfirmationOutcome, ConfirmationPresenter, ConfirmationRequest},
    handles::HandleSigner,
    ipc::LocalIpcEndpoint,
    local_settings::McpLocalSettingsService,
    policy::{OperationDescriptor, OperationRisk, PolicyEngine},
    server::{McpServerController, McpServerOptions, McpServerState, MAX_ACTIVE_SESSIONS},
    tools::{McpServices, QingYuMcpHandler},
    workspaces::{AuthorizedWorkspaceConfig, WorkspaceRegistry},
};

#[test]
fn sidecar_command_is_resolved_beside_the_application_executable() {
    let app = std::path::Path::new("/Applications/QingYu Preview.app/Contents/MacOS/QingYu");
    assert_eq!(
        super::sidecar_command_for_executable(app, ""),
        Some(std::path::PathBuf::from(
            "/Applications/QingYu Preview.app/Contents/MacOS/qingyu-mcp"
        ))
    );
}

#[test]
fn sidecar_command_uses_the_platform_executable_suffix() {
    let app = std::path::Path::new("/opt/qingyu/QingYu.exe");
    assert_eq!(
        super::sidecar_command_for_executable(app, ".exe"),
        Some(std::path::PathBuf::from("/opt/qingyu/qingyu-mcp.exe"))
    );
}

#[test]
fn resolves_macos_markra_beside_mcp_bridge() {
    let bridge = std::path::Path::new("/Applications/QingYu.app/Contents/MacOS/qingyu-mcp");
    let (executable, arguments) = test_app_launch_request(bridge, "macos");

    assert_eq!(
        executable,
        std::path::PathBuf::from("/Applications/QingYu.app/Contents/MacOS/markra")
    );
    assert_eq!(arguments, ["mcp", "serve"]);
}

#[test]
fn resolves_windows_markra_beside_mcp_bridge() {
    let bridge = std::path::Path::new("/QingYu/qingyu-mcp.exe");
    let (executable, arguments) = test_app_launch_request(bridge, "windows");

    assert_eq!(executable, std::path::PathBuf::from("/QingYu/markra.exe"));
    assert_eq!(arguments, ["mcp", "serve"]);
}

#[test]
fn resolves_linux_markra_beside_mcp_bridge() {
    let bridge = std::path::Path::new("/opt/qingyu/qingyu-mcp");
    let (executable, arguments) = test_app_launch_request(bridge, "linux");

    assert_eq!(executable, std::path::PathBuf::from("/opt/qingyu/markra"));
    assert_eq!(arguments, ["mcp", "serve"]);
}

#[test]
fn background_app_launch_does_not_inherit_mcp_stdio() {
    let source = include_str!("bridge.rs");
    let implementation_start = source
        .find("impl AppLauncher for PlatformAppLauncher")
        .expect("platform launcher implementation should exist");
    let implementation_end = source[implementation_start..]
        .find("\n#[derive(Clone, Default)]")
        .map(|offset| implementation_start + offset)
        .expect("platform launcher implementation should have a boundary");
    let implementation = &source[implementation_start..implementation_end];

    assert!(implementation.contains(".stdin(Stdio::null())"));
    assert!(implementation.contains(".stdout(Stdio::null())"));
    assert!(implementation.contains(".stderr(Stdio::null())"));
}
use crate::app_settings::AppSettingsService;
use crate::markdown_files::{
    CreateDocument, DeleteDocument, DocumentScope, DocumentService, MoveDocument, MutationOptions,
    SyncRequest, UpdateDocument,
};
use crate::remote_sync::mcp_service::{
    SyncConfigPatchInput, SyncCredentialPatchInput, SyncRunState, SyncRunner, SyncService,
};
use crate::sync_config::{
    model::{SyncConfigLoadResponse, SyncConfigPatch, SyncProvider},
    status::{SyncRunResult, SyncSummary, SyncTrigger},
    storage::{enable_at_app_data, load_from_app_data, patch_batch_at_app_data},
};
use rmcp::{ClientHandler, ServiceExt};

struct FakeConfirmationPresenter {
    outcome: ConfirmationOutcome,
}

impl ConfirmationPresenter for FakeConfirmationPresenter {
    fn present<'a>(
        &'a self,
        _request: ConfirmationRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ConfirmationOutcome> + Send + 'a>> {
        Box::pin(std::future::ready(self.outcome))
    }
}

#[test]
fn config_defaults_are_disabled_and_use_the_approved_policy() {
    let config = McpConfig::default();

    assert!(!config.enabled);
    assert_eq!(config.confirmation, ConfirmationPolicy::DestructiveOnly);
    assert_eq!(config.dry_run, DryRunPolicy::HighRisk);
    assert_eq!(config.deletion, DeletionPolicy::SystemTrash);
    assert_eq!(config.recycle_bin_retention_days, 30);
    assert_eq!(
        config.sync_after_write,
        SyncAfterWritePolicy::FollowWorkspace
    );
    assert_eq!(config.sync_execution, SyncExecutionPolicy::Background);
    assert!(config.audit.enabled);
    assert!(!config.permissions.allows(ToolCapability::DocumentsRead));
}

#[test]
fn config_revision_is_stable_and_unknown_fields_are_rejected() {
    let config = McpConfig::default();
    let first = McpConfigDocument::from_config(config.clone()).expect("valid config");
    let second = McpConfigDocument::from_config(config).expect("valid config");

    assert_eq!(first.revision, second.revision);
    assert_eq!(first.revision.len(), 64);

    let mut value = serde_json::to_value(&first.config).expect("config should serialize");
    value["unexpected"] = serde_json::json!(true);
    let bytes = serde_json::to_vec(&value).expect("json should serialize");

    assert!(McpConfigDocument::from_json(&bytes).is_err());
}

#[test]
fn config_limits_are_clamped_before_revisioning() {
    let mut config = McpConfig {
        request_limit_bytes: 0,
        response_limit_bytes: u64::MAX,
        document_limit_bytes: u64::MAX,
        requests_per_minute: 0,
        burst_requests: u32::MAX,
        concurrent_calls: usize::MAX,
        tool_timeout_secs: 0,
        recycle_bin_retention_days: 180,
        ..McpConfig::default()
    };

    config.normalize();

    assert_eq!(config.request_limit_bytes, 1);
    assert_eq!(config.response_limit_bytes, 64 * 1024 * 1024);
    assert_eq!(config.document_limit_bytes, 64 * 1024 * 1024);
    assert_eq!(config.requests_per_minute, 1);
    assert_eq!(config.burst_requests, 100);
    assert_eq!(config.concurrent_calls, 32);
    assert_eq!(config.tool_timeout_secs, 5);
    assert_eq!(config.recycle_bin_retention_days, 30);
}

#[test]
fn config_preserves_supported_recycle_bin_retention_presets() {
    for recycle_bin_retention_days in [0, 7, 30, 90] {
        let mut config = McpConfig {
            recycle_bin_retention_days,
            ..McpConfig::default()
        };

        config.normalize();

        assert_eq!(
            config.recycle_bin_retention_days,
            recycle_bin_retention_days
        );
    }
}

#[test]
fn recycle_cleanup_policy_requires_enabled_mcp_and_a_nonzero_retention() {
    let disabled = McpConfig {
        recycle_bin_retention_days: 7,
        ..McpConfig::default()
    };
    assert_eq!(super::recycle_retention_for_cleanup(&disabled), None);

    let never = McpConfig {
        enabled: true,
        recycle_bin_retention_days: 0,
        ..McpConfig::default()
    };
    assert_eq!(super::recycle_retention_for_cleanup(&never), None);

    let enabled = McpConfig {
        enabled: true,
        recycle_bin_retention_days: 7,
        ..McpConfig::default()
    };
    assert_eq!(super::recycle_retention_for_cleanup(&enabled), Some(7));
}

#[test]
fn process_scoped_identifiers_need_no_credential_store() {
    let project = tempfile::tempdir().expect("project");
    std::fs::write(project.path().join("note.md"), "session note").expect("project note");
    let registry = workspace_registry();
    let workspace = registry
        .activate_current(project.path())
        .expect("activate project");
    let first = HandleSigner::new(super::new_process_key().expect("first process key"));
    let second = HandleSigner::new(super::new_process_key().expect("second process key"));
    let document_id = first
        .issue_document(workspace.workspace_id, registry.generation(), "note.md")
        .expect("issue process-scoped document ID");

    first
        .verify_document(&document_id, &registry)
        .expect("same process key verifies");
    assert!(second.verify_document(&document_id, &registry).is_err());

    let manifest = include_str!("../../Cargo.toml");
    let module = include_str!("mod.rs");
    let desktop_runtime = include_str!("../desktop_runtime.rs");
    for removed in [
        "keyring =",
        "KeyringSecretStore",
        "ensure_signing_key",
        "copy_mcp_token",
        "rotate_mcp_token",
        "revoke_mcp_token",
    ] {
        assert!(
            !manifest.contains(removed),
            "manifest still contains {removed}"
        );
        assert!(
            !module.contains(removed),
            "MCP module still contains {removed}"
        );
        assert!(
            !desktop_runtime.contains(removed),
            "desktop runtime still contains {removed}"
        );
    }
}

#[test]
fn application_mcp_runtime_material_is_scoped_below_mcp_runtime() {
    let module = include_str!("mod.rs");

    assert!(module.contains("app_data_dir.join(\"mcp-runtime\")"));
    for legacy_root in [
        "app_data_dir.join(\"mcp-history\")",
        "app_data_dir.join(\"mcp-recycle\")",
        "AuditSink::new(&app_data_dir",
    ] {
        assert!(
            !module.contains(legacy_root),
            "MCP runtime material escapes mcp-runtime through {legacy_root}"
        );
    }
}

#[test]
fn config_manager_persists_complete_policy_without_runtime_material() {
    let settings = McpLocalSettingsService::memory_for_test();
    let manager = McpConfigManager::load(settings.clone()).expect("load app policy manager");
    let initial = manager.snapshot().expect("initial policy");
    let mut enabled = initial.config;
    enabled.enabled = true;
    let saved = manager
        .update(enabled, &initial.revision)
        .expect("persist policy through app settings");
    let loaded = McpConfigManager::load(settings)
        .expect("reload app policy manager")
        .snapshot()
        .expect("reloaded policy");
    let json = serde_json::to_string(&loaded.config).expect("config should serialize");

    assert_eq!(loaded.revision, saved.revision);
    assert!(loaded.config.enabled);
    assert!(!json.contains("bearer-token"));
    assert!(!json.contains("handle-signing-key"));
    assert!(!json.contains("\"port\""));
    assert!(!json.contains("\"workspaces\""));
}

#[test]
fn config_manager_rejects_stale_revisions_and_advances_generation() {
    let manager = McpConfigManager::memory_for_test().expect("load config manager");
    let initial = manager.snapshot().expect("initial snapshot");
    let initial_generation = manager.generation();
    let mut enabled = initial.config.clone();
    enabled.enabled = true;

    let updated = manager
        .update(enabled, &initial.revision)
        .expect("update current revision");

    assert!(updated.config.enabled);
    assert!(manager.generation() > initial_generation);
    assert!(manager
        .update(McpConfig::default(), &initial.revision)
        .is_err());
}

#[test]
fn config_manager_reloads_policy_after_the_local_store_changes() {
    let settings = McpLocalSettingsService::memory_for_test();
    let manager = McpConfigManager::load(settings.clone()).expect("load policy manager");
    let initial_generation = manager.generation();
    let mut enabled = McpConfig::default();
    enabled.enabled = true;
    let stored = settings.load_migrated().expect("stored policy");
    settings
        .write(&stored.revision, enabled)
        .expect("simulate an out-of-band local policy update");

    assert!(!manager.snapshot().unwrap().config.enabled);
    let reloaded = manager.reload().expect("reload local policy");

    assert!(reloaded.config.enabled);
    assert!(manager.generation() > initial_generation);
}

#[test]
fn application_policy_does_not_create_project_configuration() {
    let project_a = tempfile::tempdir().expect("project A");
    let project_b = tempfile::tempdir().expect("project B");
    let manager = McpConfigManager::memory_for_test().expect("application config manager");

    let initial = manager.snapshot().expect("initial application policy");
    let mut enabled = initial.config;
    enabled.enabled = true;
    manager
        .update(enabled, &initial.revision)
        .expect("save application policy");

    assert!(manager.snapshot().unwrap().config.enabled);
    assert!(!project_a.path().join(".qingyu/mcp.json").exists());
    assert!(!project_b.path().join(".qingyu/mcp.json").exists());
}

fn workspace_registry() -> WorkspaceRegistry {
    WorkspaceRegistry::new(Vec::new())
}

#[test]
fn workspace_boundary_rejects_unsafe_relative_paths() {
    let directory = tempfile::tempdir().expect("temporary workspace");
    let registry = workspace_registry();
    let workspace = registry
        .authorize(directory.path(), "Notes")
        .expect("authorize workspace");
    let signer = HandleSigner::new([7_u8; 32]);
    let rejected_names = [
        "../secret.md",
        "../../secret.md",
        "/tmp/secret.md",
        "//server/share/secret.md",
        "C:\\secret.md",
        "C:/secret.md",
        "C:secret.md",
        "\\\\server\\share\\secret.md",
        "\\\\?\\C:\\secret.md",
        "a\\..\\secret.md",
        "a/..\\secret.md",
        "a\\../secret.md",
        "a/./secret.md",
        "a//secret.md",
        "a\0secret.md",
        "%2e%2e/secret.md",
        "%2E%2E%2Fsecret.md",
        "..%2fsecret.md",
        "..%5csecret.md",
        "%252e%252e/secret.md",
        "%252e%252e%252fsecret.md",
        ".qingyu/config.md",
        ".GIT/secret.md",
        ".markra-sync/config.md",
        "node_modules/note.md",
    ];

    for name in rejected_names {
        assert!(
            signer
                .issue_document(workspace.workspace_id, registry.generation(), name)
                .is_err(),
            "unsafe path should be rejected: {name:?}"
        );
    }

    for normalized_name in ["café.md", "cafe\u{301}.md", "．．/secret.md"] {
        assert!(
            signer
                .issue_document(
                    workspace.workspace_id,
                    registry.generation(),
                    normalized_name,
                )
                .is_ok(),
            "Unicode stays a literal in-root name: {normalized_name:?}"
        );
    }
}

#[test]
fn workspace_boundary_rejects_nested_duplicate_and_protected_roots() {
    let directory = tempfile::tempdir().expect("temporary workspace parent");
    let nested = directory.path().join("nested");
    std::fs::create_dir(&nested).expect("nested workspace");
    let registry = workspace_registry();

    registry
        .authorize(directory.path(), "Parent")
        .expect("authorize parent");
    assert!(registry.authorize(directory.path(), "Duplicate").is_err());
    assert!(registry.authorize(&nested, "Nested").is_err());

    let protected_registry = WorkspaceRegistry::new(vec![directory.path().to_path_buf()]);
    assert!(protected_registry.authorize(&nested, "Protected").is_err());
}

#[test]
fn workspace_boundary_signed_handles_reject_tamper_type_and_cross_workspace_use() {
    let first = tempfile::tempdir().expect("first workspace");
    let second = tempfile::tempdir().expect("second workspace");
    std::fs::write(first.path().join("note.md"), "hello").expect("write note");
    std::fs::write(second.path().join("note.md"), "other").expect("write note");
    let registry = workspace_registry();
    let first_workspace = registry
        .authorize(first.path(), "First")
        .expect("authorize first");
    let second_workspace = registry
        .authorize(second.path(), "Second")
        .expect("authorize second");
    let signer = HandleSigner::new([9_u8; 32]);
    let document_id = signer
        .issue_document(
            first_workspace.workspace_id,
            registry.generation(),
            "note.md",
        )
        .expect("issue document ID");

    let verified = signer
        .verify_document(&document_id, &registry)
        .expect("verify document ID");
    assert_eq!(verified.workspace_id(), first_workspace.workspace_id);
    assert_eq!(verified.relative_path(), std::path::Path::new("note.md"));
    assert!(signer.verify_folder(&document_id, &registry).is_err());
    assert!(signer
        .verify_document_in_workspace(&document_id, second_workspace.workspace_id, &registry,)
        .is_err());

    let mut tampered = document_id.into_bytes();
    let final_byte = tampered.last_mut().expect("nonempty handle");
    *final_byte = if *final_byte == b'A' { b'B' } else { b'A' };
    let tampered = String::from_utf8(tampered).expect("ASCII handle");
    assert!(signer.verify_document(&tampered, &registry).is_err());
}

#[test]
fn workspace_boundary_root_folder_handle_is_typed_and_resolves() {
    let directory = tempfile::tempdir().expect("temporary workspace");
    let registry = workspace_registry();
    let workspace = registry
        .authorize(directory.path(), "Notes")
        .expect("authorize workspace");
    let signer = HandleSigner::new([11_u8; 32]);
    let folder_id = signer
        .issue_folder(workspace.workspace_id, registry.generation(), "")
        .expect("issue root folder ID");

    let verified = signer
        .verify_folder(&folder_id, &registry)
        .expect("verify root folder ID");

    assert_eq!(verified.workspace_id(), workspace.workspace_id);
    assert_eq!(verified.relative_path(), std::path::Path::new(""));
}

#[test]
fn workspace_boundary_removal_and_reauthorization_do_not_revive_old_handles() {
    let directory = tempfile::tempdir().expect("temporary workspace");
    std::fs::write(directory.path().join("note.md"), "hello").expect("write note");
    let registry = workspace_registry();
    let first_workspace = registry
        .authorize(directory.path(), "First")
        .expect("authorize workspace");
    let signer = HandleSigner::new([13_u8; 32]);
    let document_id = signer
        .issue_document(
            first_workspace.workspace_id,
            registry.generation(),
            "note.md",
        )
        .expect("issue document ID");

    registry
        .remove(first_workspace.workspace_id)
        .expect("remove workspace");
    let removed_error = match signer.verify_document(&document_id, &registry) {
        Ok(_) => panic!("removed primary handle must be stale"),
        Err(error) => error,
    };
    assert_eq!(removed_error.code, "mcp-handle-stale");

    let second_workspace = registry
        .authorize(directory.path(), "Second")
        .expect("reauthorize workspace");
    assert_ne!(first_workspace.workspace_id, second_workspace.workspace_id);
    assert!(signer.verify_document(&document_id, &registry).is_err());
}

#[test]
fn primary_workspace_switch_a_to_b_to_a_never_revives_old_handles() {
    let project_a = tempfile::tempdir().expect("project A");
    let project_b = tempfile::tempdir().expect("project B");
    std::fs::write(project_a.path().join("note.md"), "project A").expect("project A note");
    std::fs::write(project_b.path().join("note.md"), "project B").expect("project B note");
    let registry = workspace_registry();
    let signer = HandleSigner::new([14_u8; 32]);

    let workspace_a = registry
        .activate_current(project_a.path())
        .expect("activate project A");
    let project_a_document = signer
        .issue_document(workspace_a.workspace_id, registry.generation(), "note.md")
        .expect("project A handle");

    let workspace_b = registry
        .activate_current(project_b.path())
        .expect("activate project B");
    let listed = registry.list_safe();

    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].workspace_id, workspace_b.workspace_id);
    let error = match signer.verify_document(&project_a_document, &registry) {
        Ok(_) => panic!("old project handle must be invalidated"),
        Err(error) => error,
    };
    assert_eq!(error.code, "mcp-handle-stale");

    let workspace_a_again = registry
        .activate_current(project_a.path())
        .expect("activate project A again");
    assert_ne!(workspace_a_again.workspace_id, workspace_a.workspace_id);
    let error = match signer.verify_document(&project_a_document, &registry) {
        Ok(_) => panic!("A -> B -> A must not revive the old A handle"),
        Err(error) => error,
    };
    assert_eq!(error.code, "mcp-handle-stale");
}

#[test]
fn old_workspace_identifier_is_stale_after_primary_switch_or_clear() {
    let project_a = tempfile::tempdir().expect("project A");
    let project_b = tempfile::tempdir().expect("project B");
    let registry = workspace_registry();
    let workspace_a = registry
        .activate_current(project_a.path())
        .expect("install project A");

    registry
        .activate_current(project_b.path())
        .expect("switch to project B");
    let switched = registry
        .resolve(workspace_a.workspace_id)
        .err()
        .expect("old workspace identifier must fail");
    assert_eq!(switched.code, "mcp-handle-stale");

    let workspace_b = registry
        .require_primary_workspace()
        .expect("resolve project B");
    registry.clear_current().expect("clear project B");
    let cleared = registry
        .resolve(workspace_b.workspace_id)
        .err()
        .expect("cleared workspace identifier must fail");
    assert_eq!(cleared.code, "mcp-handle-stale");
}

#[test]
fn reinstalling_the_same_canonical_primary_workspace_is_idempotent() {
    let project = tempfile::tempdir().expect("project");
    std::fs::write(project.path().join("note.md"), "project note").expect("project note");
    let registry = workspace_registry();
    let signer = HandleSigner::new([15_u8; 32]);
    let workspace = registry
        .activate_current(project.path())
        .expect("install primary workspace");
    let generation = registry.generation();
    let document = signer
        .issue_document(workspace.workspace_id, registry.generation(), "note.md")
        .expect("primary document handle");

    let repeated = registry
        .activate_current(&project.path().join("."))
        .expect("repeat canonical primary workspace");

    assert_eq!(repeated.workspace_id, workspace.workspace_id);
    assert_eq!(registry.generation(), generation);
    signer
        .verify_document(&document, &registry)
        .expect("idempotent install must preserve current handles");
}

#[test]
fn runtime_workspace_event_follows_authority_changes_but_not_idempotent_reinstalls() {
    let project_a = tempfile::tempdir().expect("project A");
    let project_b = tempfile::tempdir().expect("project B");
    let authoritative_a = project_a
        .path()
        .canonicalize()
        .expect("canonical project A");
    let authoritative_b = project_b
        .path()
        .canonicalize()
        .expect("canonical project B");
    let registry = workspace_registry();
    let initial_generation = registry.generation();

    super::apply_primary_workspace_transaction(
        &registry,
        Some(authoritative_a.to_string_lossy().as_ref()),
        Ok(authoritative_a.clone()),
    )
    .expect("install project A");
    let generation_a = registry.generation();
    assert_eq!(
        super::mcp_runtime_change_event(initial_generation, generation_a)
            .expect("A installation event")
            .workspace_generation,
        generation_a
    );

    super::apply_primary_workspace_transaction(
        &registry,
        Some(authoritative_a.to_string_lossy().as_ref()),
        Ok(authoritative_a.clone()),
    )
    .expect("repeat project A");
    assert!(super::mcp_runtime_change_event(generation_a, registry.generation()).is_none());

    super::apply_primary_workspace_transaction(
        &registry,
        Some(authoritative_b.to_string_lossy().as_ref()),
        Ok(authoritative_b.clone()),
    )
    .expect("switch to project B");
    let generation_b = registry.generation();
    assert_eq!(
        super::mcp_runtime_change_event(generation_a, generation_b)
            .expect("B switch event")
            .workspace_generation,
        generation_b
    );

    super::apply_primary_workspace_transaction(&registry, None, Ok(authoritative_b))
        .expect("clear project B");
    let cleared_generation = registry.generation();
    assert_eq!(
        super::mcp_runtime_change_event(generation_b, cleared_generation)
            .expect("workspace clear event")
            .workspace_generation,
        cleared_generation
    );
    assert_eq!(
        super::MCP_RUNTIME_CHANGED_EVENT,
        "qingyu://mcp-runtime-changed"
    );
}

#[test]
fn resolved_primary_capability_rechecks_generation_before_file_access() {
    let project_a = tempfile::tempdir().expect("project A");
    let project_b = tempfile::tempdir().expect("project B");
    let registry = workspace_registry();
    let workspace_a = registry
        .activate_current(project_a.path())
        .expect("install project A");
    let resolved_a = registry
        .resolve(workspace_a.workspace_id)
        .expect("resolve project A capability");
    let service = DocumentService::new(HandleSigner::new([43_u8; 32]));
    let scope = DocumentScope::authorized(resolved_a.clone());

    registry
        .activate_current(project_b.path())
        .expect("switch to project B");

    let error = resolved_a
        .revalidate_authority()
        .expect_err("old capability must fail before filesystem access");
    assert_eq!(error.code, "mcp-handle-stale");

    let service_error = service
        .list(&scope, None, None, 10)
        .expect_err("document service must preserve the stale capability error");
    assert_eq!(service_error.code, "mcp-handle-stale");
}

#[test]
fn handle_generation_and_workspace_resolution_use_one_registry_snapshot() {
    let project_a = tempfile::tempdir().expect("project A");
    let project_b = tempfile::tempdir().expect("project B");
    let registry = workspace_registry();
    let workspace_a = registry
        .activate_current(project_a.path())
        .expect("install project A");
    let generation_a = registry.generation();

    registry
        .activate_current(project_b.path())
        .expect("switch to project B");

    let error = match registry.resolve_at_generation(workspace_a.workspace_id, generation_a) {
        Ok(_) => panic!("generation mismatch must win over workspace lookup"),
        Err(error) => error,
    };
    assert_eq!(error.code, "mcp-handle-stale");
}

#[test]
fn application_data_roots_cannot_become_mcp_document_authority() {
    let base = tempfile::tempdir().expect("application roots");
    let app_config = base.path().join("config");
    let app_data = base.path().join("data");
    let managed_workspace = app_data.join("workspace");
    let runtime = app_data.join("mcp-runtime");
    let sync_state = app_data.join("sync-state");
    for path in [&app_config, &managed_workspace, &runtime, &sync_state] {
        std::fs::create_dir_all(path).expect("application directory");
    }
    let registry = WorkspaceRegistry::for_application_data(&app_config, &app_data);

    for protected in [
        base.path(),
        &app_data,
        &managed_workspace,
        &runtime,
        &sync_state,
        &app_config,
    ] {
        let error = registry
            .activate_current(protected)
            .expect_err("application internals must stay protected");
        assert_eq!(error.code, "protected_path");
    }
}

#[test]
fn rejected_primary_request_preserves_the_authoritative_workspace() {
    let project_a = tempfile::tempdir().expect("project A");
    let project_b = tempfile::tempdir().expect("project B");
    let registry = workspace_registry();
    let workspace_a = registry
        .activate_current(project_a.path())
        .expect("install authoritative project A");

    let error = super::apply_primary_workspace_transaction(
        &registry,
        Some(project_b.path().to_string_lossy().as_ref()),
        Ok(project_a
            .path()
            .canonicalize()
            .expect("canonical project A")),
    )
    .expect_err("non-authoritative project B must be rejected");

    assert!(error.starts_with("mcp-primary-workspace-mismatch:"));
    assert_eq!(registry.list_safe().len(), 1);
    assert_eq!(
        registry.list_safe()[0].workspace_id,
        workspace_a.workspace_id
    );
}

#[test]
fn stale_primary_request_aligns_authority_to_the_current_local_state_root() {
    let project_a = tempfile::tempdir().expect("project A");
    let project_b = tempfile::tempdir().expect("project B");
    let registry = workspace_registry();
    registry
        .activate_current(project_a.path())
        .expect("install project A");

    let error = super::apply_primary_workspace_transaction(
        &registry,
        Some(project_a.path().to_string_lossy().as_ref()),
        Ok(project_b
            .path()
            .canonicalize()
            .expect("canonical project B")),
    )
    .expect_err("stale project A request must be rejected");

    assert!(error.starts_with("mcp-primary-workspace-mismatch:"));
    let listed = registry.list_safe();
    assert_eq!(listed.len(), 1);
    assert_eq!(
        registry
            .resolve(listed[0].workspace_id)
            .expect("resolved current authority")
            .canonical_path,
        project_b
            .path()
            .canonicalize()
            .expect("canonical project B")
    );
}

#[test]
fn missing_stale_primary_request_cannot_leave_the_old_authority_installed() {
    let project_a = tempfile::tempdir().expect("project A");
    let project_b = tempfile::tempdir().expect("project B");
    let missing = project_a.path().join("missing-primary");
    let registry = workspace_registry();
    registry
        .activate_current(project_a.path())
        .expect("install project A");

    let error = super::apply_primary_workspace_transaction(
        &registry,
        Some(missing.to_string_lossy().as_ref()),
        Ok(project_b
            .path()
            .canonicalize()
            .expect("canonical project B")),
    )
    .expect_err("missing stale request must be rejected");

    assert!(error.starts_with("mcp-primary-workspace-mismatch:"));
    let listed = registry.list_safe();
    assert_eq!(listed.len(), 1);
    assert_eq!(
        registry
            .resolve(listed[0].workspace_id)
            .expect("resolved current authority")
            .canonical_path,
        project_b
            .path()
            .canonicalize()
            .expect("canonical project B")
    );
}

#[test]
fn unavailable_local_state_clears_stale_primary_authority() {
    let project_a = tempfile::tempdir().expect("project A");
    let registry = workspace_registry();
    registry
        .activate_current(project_a.path())
        .expect("install project A");

    let error = super::apply_primary_workspace_transaction(
        &registry,
        Some(project_a.path().to_string_lossy().as_ref()),
        Err("sync-primary-workspace-unavailable: unavailable".to_string()),
    )
    .expect_err("unavailable fact source must fail closed");

    assert!(error.starts_with("mcp-workspace-unavailable:"));
    assert!(registry.list_safe().is_empty());
}

#[test]
fn failed_authoritative_activation_clears_the_previous_workspace() {
    let project_a = tempfile::tempdir().expect("project A");
    let protected_b = tempfile::tempdir().expect("protected project B");
    let authoritative_b = protected_b
        .path()
        .canonicalize()
        .expect("canonical protected B");
    let registry = WorkspaceRegistry::new(vec![authoritative_b.clone()]);
    registry
        .activate_current(project_a.path())
        .expect("install project A");

    let error = super::apply_primary_workspace_transaction(
        &registry,
        Some(authoritative_b.to_string_lossy().as_ref()),
        Ok(authoritative_b.clone()),
    )
    .expect_err("protected authoritative root cannot be installed");

    assert!(error.starts_with("protected_path:"));
    assert!(
        registry.list_safe().is_empty(),
        "a failed authoritative install must clear stale project A"
    );
}

#[test]
fn external_window_cannot_clear_or_replace_primary_mcp_authority() {
    let project_a = tempfile::tempdir().expect("project A");
    let project_b = tempfile::tempdir().expect("project B");
    let registry = workspace_registry();
    let workspace_a = registry
        .activate_current(project_a.path())
        .expect("install project A");
    let generation_a = registry.generation();
    let project_b_text = project_b.path().to_string_lossy().into_owned();

    for requested in [None, Some(project_b_text.as_str())] {
        let error = super::set_primary_workspace_from_window(
            "markra-external-1",
            &registry,
            requested,
            Ok(project_b
                .path()
                .canonicalize()
                .expect("canonical project B")),
        )
        .expect_err("external editor window must not mutate MCP authority");
        assert!(error.starts_with("mcp-primary-window-required:"));
        assert_eq!(registry.generation(), generation_a);
        assert_eq!(
            registry.list_safe()[0].workspace_id,
            workspace_a.workspace_id
        );
    }
}

#[test]
fn bridge_and_application_share_one_app_data_ipc_resolver() {
    let application = LocalIpcEndpoint::for_app().expect("application IPC endpoint");
    let bridge = BridgeConfig::for_app().expect("bridge IPC endpoint");

    assert_eq!(bridge.endpoint, application);
}

#[test]
fn workspace_boundary_restores_stable_ids_without_exposing_absolute_paths() {
    let directory = tempfile::tempdir().expect("temporary workspace");
    let registry = workspace_registry();
    let configured = registry
        .authorize(directory.path(), "Notes")
        .expect("authorize workspace");
    let restored = WorkspaceRegistry::from_configs(vec![configured.clone()], Vec::new())
        .expect("restore registry");
    let listed = restored.list_safe();
    let json = serde_json::to_string(&listed).expect("safe workspaces serialize");

    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].workspace_id, configured.workspace_id);
    assert!(!json.contains(&directory.path().to_string_lossy().to_string()));
}

#[test]
fn workspace_boundary_restores_unavailable_roots_without_granting_access() {
    let parent = tempfile::tempdir().expect("temporary workspace parent");
    let root = parent.path().join("offline-notes");
    std::fs::create_dir(&root).expect("workspace root");
    std::fs::write(root.join("note.md"), "offline").expect("workspace note");
    let registry = workspace_registry();
    let configured = registry
        .authorize(&root, "Offline")
        .expect("authorize workspace");
    let signer = HandleSigner::new([19_u8; 32]);
    let document_id = signer
        .issue_document(configured.workspace_id, registry.generation(), "note.md")
        .expect("issue document ID");
    drop(registry);
    std::fs::remove_dir_all(&root).expect("remove workspace root");

    let restored = WorkspaceRegistry::from_configs(vec![configured.clone()], Vec::new())
        .expect("restore unavailable registry entry");
    let listed = restored.list_safe();

    assert_eq!(listed.len(), 1);
    assert!(!listed[0].available);
    let error = match restored.resolve(configured.workspace_id) {
        Ok(_) => panic!("unavailable root must not resolve"),
        Err(error) => error,
    };
    assert_eq!(error.code, "workspace_unavailable");
    let handle_error = match signer.verify_document(&document_id, &restored) {
        Ok(_) => panic!("offline handle must fail"),
        Err(error) => error,
    };
    assert_eq!(handle_error.code, "workspace_unavailable");
}

#[test]
fn initializes_authoritative_workspace_without_window() {
    let directory = tempfile::tempdir().expect("temporary primary workspace");
    let registry = WorkspaceRegistry::new(Vec::new());

    let changed = super::test_apply_authoritative_primary_workspace(
        &registry,
        Ok(directory
            .path()
            .canonicalize()
            .expect("canonical workspace")),
    )
    .expect("activate authoritative primary workspace");

    assert!(changed);
    assert_eq!(registry.list_safe().len(), 1);
    assert!(registry.list_safe()[0].available);
}

#[test]
fn missing_authoritative_workspace_clears_existing_authority() {
    let directory = tempfile::tempdir().expect("temporary primary workspace");
    let registry = WorkspaceRegistry::new(Vec::new());
    registry
        .activate_current(directory.path())
        .expect("activate initial workspace");

    let error = super::test_apply_authoritative_primary_workspace(
        &registry,
        Err("primary unavailable".to_string()),
    )
    .expect_err("missing authority must fail closed");

    assert!(error.contains("mcp-workspace-unavailable"));
    assert!(registry.list_safe().is_empty());
}

#[test]
fn mcp_initialization_installs_workspace_before_starting_listener() {
    let source = include_str!("mod.rs");
    let initialize_start = source
        .find("pub(crate) fn initialize")
        .expect("MCP initialize function");
    let initialize_end = source[initialize_start..]
        .find("pub(crate) struct UpdateMcpSettingsInput")
        .map(|offset| initialize_start + offset)
        .expect("MCP initialize boundary");
    let initialize = &source[initialize_start..initialize_end];
    let activation = initialize
        .find("apply_authoritative_primary_workspace")
        .expect("backend-only workspace activation");
    let startup = initialize
        .find("apply_server_config")
        .expect("MCP listener startup");

    assert!(activation < startup);
    assert!(!initialize.contains("WebviewWindow"));
}

#[test]
fn workspace_boundary_rejects_a_replacement_at_the_authorized_address() {
    let parent = tempfile::tempdir().expect("temporary workspace parent");
    let root = parent.path().join("notes");
    let moved = parent.path().join("moved-notes");
    std::fs::create_dir(&root).expect("workspace root");
    let registry = workspace_registry();
    let workspace = registry
        .authorize(&root, "Notes")
        .expect("authorize workspace");

    std::fs::rename(&root, &moved).expect("move original workspace");
    std::fs::create_dir(&root).expect("replacement workspace");

    let error = match registry.resolve(workspace.workspace_id) {
        Ok(_) => panic!("replacement root must not resolve"),
        Err(error) => error,
    };
    assert_eq!(error.code, "workspace_unavailable");
}

#[cfg(unix)]
#[test]
fn workspace_boundary_rejects_symlinked_roots_parents_and_documents() {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().expect("temporary workspace parent");
    let real_root = directory.path().join("real");
    let outside = directory.path().join("outside");
    std::fs::create_dir(&real_root).expect("real root");
    std::fs::create_dir(&outside).expect("outside root");
    std::fs::write(outside.join("secret.md"), "secret").expect("outside note");
    let linked_root = directory.path().join("linked-root");
    symlink(&real_root, &linked_root).expect("root symlink");
    let registry = workspace_registry();

    assert!(registry.authorize(&linked_root, "Linked").is_err());

    let workspace = registry
        .authorize(&real_root, "Real")
        .expect("authorize real root");
    symlink(&outside, real_root.join("linked-parent")).expect("parent symlink");
    symlink(outside.join("secret.md"), real_root.join("linked.md")).expect("file symlink");
    let signer = HandleSigner::new([17_u8; 32]);
    let parent_document = signer
        .issue_document(
            workspace.workspace_id,
            registry.generation(),
            "linked-parent/secret.md",
        )
        .expect("issue parent document ID");
    let linked_document = signer
        .issue_document(workspace.workspace_id, registry.generation(), "linked.md")
        .expect("issue linked document ID");

    assert!(signer.verify_document(&parent_document, &registry).is_err());
    assert!(signer.verify_document(&linked_document, &registry).is_err());
}

fn policy_descriptor(arguments: &str, revision: Option<&str>) -> OperationDescriptor {
    OperationDescriptor {
        tool: "document_update".to_string(),
        workspace_id: Some(uuid::Uuid::nil()),
        workspace_display_name: Some("Notes".to_string()),
        target: Some("draft.md".to_string()),
        expected_revision: revision.map(str::to_string),
        risk: OperationRisk::Write,
        canonical_arguments: arguments.to_string(),
    }
}

#[test]
fn policy_matrix_matches_all_confirmation_and_dry_run_modes() {
    let confirmation_modes = [
        ConfirmationPolicy::Never,
        ConfirmationPolicy::DestructiveOnly,
        ConfirmationPolicy::AllWrites,
    ];
    let dry_run_modes = [
        DryRunPolicy::Never,
        DryRunPolicy::HighRisk,
        DryRunPolicy::AllWrites,
    ];

    for confirmation in confirmation_modes {
        for dry_run in dry_run_modes {
            let write = PolicyEngine::requirements(confirmation, dry_run, OperationRisk::Write);
            let high_risk =
                PolicyEngine::requirements(confirmation, dry_run, OperationRisk::HighRisk);
            let destructive =
                PolicyEngine::requirements(confirmation, dry_run, OperationRisk::Destructive);

            assert_eq!(
                write.confirmation_required,
                matches!(confirmation, ConfirmationPolicy::AllWrites)
            );
            assert_eq!(
                high_risk.confirmation_required,
                !matches!(confirmation, ConfirmationPolicy::Never)
            );
            assert_eq!(
                destructive.confirmation_required,
                !matches!(confirmation, ConfirmationPolicy::Never)
            );
            assert_eq!(
                write.preview_required,
                matches!(dry_run, DryRunPolicy::AllWrites)
            );
            assert_eq!(
                high_risk.preview_required,
                !matches!(dry_run, DryRunPolicy::Never)
            );
            assert_eq!(
                destructive.preview_required,
                !matches!(dry_run, DryRunPolicy::Never)
            );
        }
    }
}

#[test]
fn policy_preview_is_single_use_and_bound_to_every_security_input() {
    let engine = PolicyEngine::new([23_u8; 32]);
    let descriptor = policy_descriptor("{\"contents\":\"one\"}", Some("revision-1"));
    let preview = engine
        .preview_at(&descriptor, 7, 11, 1_000)
        .expect("issue preview");

    engine
        .consume_preview_at(&preview.token, &descriptor, 7, 11, 1_001)
        .expect("consume matching preview");
    assert!(engine
        .consume_preview_at(&preview.token, &descriptor, 7, 11, 1_002)
        .is_err());

    let changed_arguments = policy_descriptor("{\"contents\":\"two\"}", Some("revision-1"));
    let changed_revision = policy_descriptor("{\"contents\":\"one\"}", Some("revision-2"));
    for (candidate, policy_generation, workspace_generation, now) in [
        (&changed_arguments, 7, 11, 1_001),
        (&changed_revision, 7, 11, 1_001),
        (&descriptor, 8, 11, 1_001),
        (&descriptor, 7, 12, 1_001),
        (&descriptor, 7, 11, 1_301),
    ] {
        let preview = engine
            .preview_at(&descriptor, 7, 11, 1_000)
            .expect("issue bound preview");
        assert!(engine
            .consume_preview_at(
                &preview.token,
                candidate,
                policy_generation,
                workspace_generation,
                now,
            )
            .is_err());
    }

    let preview = engine
        .preview_at(&descriptor, 7, 11, 1_000)
        .expect("issue preview before invalidation");
    engine.invalidate_previews();
    assert!(engine
        .consume_preview_at(&preview.token, &descriptor, 7, 11, 1_001)
        .is_err());
}

#[test]
fn policy_permission_checks_use_the_current_global_profile() {
    let mut permissions = super::config::McpPermissions::default();
    assert!(PolicyEngine::authorize(&permissions, ToolCapability::DocumentsRead).is_err());

    permissions.documents_read = true;
    assert!(PolicyEngine::authorize(&permissions, ToolCapability::DocumentsRead).is_ok());
    assert!(PolicyEngine::authorize(&permissions, ToolCapability::DocumentsWrite).is_err());
}

#[tokio::test]
async fn policy_confirmation_maps_rejection_and_timeout_to_stable_errors() {
    let engine = PolicyEngine::new([29_u8; 32]);
    let descriptor = policy_descriptor("{}", Some("revision-1"));

    for (outcome, expected_code) in [
        (ConfirmationOutcome::Rejected, "confirmation_rejected"),
        (ConfirmationOutcome::TimedOut, "confirmation_timeout"),
    ] {
        let presenter = FakeConfirmationPresenter { outcome };
        let error = engine
            .confirm_if_required(ConfirmationPolicy::AllWrites, &descriptor, &presenter)
            .await
            .expect_err("confirmation must fail closed");
        assert_eq!(error.code, expected_code);
    }

    let presenter = FakeConfirmationPresenter {
        outcome: ConfirmationOutcome::Rejected,
    };
    assert_eq!(
        engine
            .confirm_if_required(ConfirmationPolicy::Never, &descriptor, &presenter)
            .await
            .expect("confirmation is not required"),
        ConfirmationOutcome::Allowed
    );
}

#[test]
fn policy_audit_is_bounded_and_redacts_absolute_targets() {
    let directory = tempfile::tempdir().expect("temporary audit directory");
    let sink = AuditSink::new(
        directory.path(),
        super::config::AuditPolicy {
            enabled: true,
            retention_days: 30,
            max_entries: 100,
        },
    );
    let event = AuditEvent {
        request_id: uuid::Uuid::new_v4(),
        tool: "document_read".to_string(),
        workspace_id: Some(uuid::Uuid::new_v4()),
        workspace_display_name: Some("Notes".to_string()),
        logical_target: Some("/Users/example/private".to_string()),
        dry_run: false,
        confirmation: None,
        outcome: AuditOutcome::Succeeded,
        error_code: None,
        revision_before: Some("before".to_string()),
        revision_after: Some("after".to_string()),
        sync_run_id: None,
        duration_ms: 12,
        counts: std::collections::BTreeMap::from([("documents".to_string(), 1)]),
    };

    sink.record(event).expect("record safe audit event");
    let entries = sink.list(0, 100).expect("list audit entries");
    let bytes =
        std::fs::read(directory.path().join("mcp-audit.jsonl")).expect("read audit artifact");
    let audit = String::from_utf8(bytes).expect("audit should be UTF-8");

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].logical_target.as_deref(), Some("[redacted]"));
    for sentinel in [
        "/Users/example/private",
        "secret document body",
        "Bearer abc",
        "S3SECRET",
    ] {
        assert!(!audit.contains(sentinel), "audit leaked {sentinel:?}");
    }
    for forbidden_field in ["documentBody", "absolutePath", "bearerToken", "credentials"] {
        assert!(!audit.contains(forbidden_field));
    }

    sink.update_policy(super::config::AuditPolicy {
        enabled: false,
        retention_days: 30,
        max_entries: 100,
    })
    .expect("disable audit policy at runtime");
    sink.record(AuditEvent {
        request_id: uuid::Uuid::new_v4(),
        tool: "document_update".to_string(),
        workspace_id: None,
        workspace_display_name: None,
        logical_target: Some("ignored.md".to_string()),
        dry_run: false,
        confirmation: None,
        outcome: AuditOutcome::Succeeded,
        error_code: None,
        revision_before: None,
        revision_after: None,
        sync_run_id: None,
        duration_ms: 1,
        counts: std::collections::BTreeMap::new(),
    })
    .expect("disabled audit is a successful no-op");
    assert_eq!(sink.list(0, 100).expect("list unchanged audit").len(), 1);

    sink.clear().expect("clear audit entries");
    assert!(sink.list(0, 100).expect("list cleared audit").is_empty());
}

fn document_service_fixture() -> (
    tempfile::TempDir,
    WorkspaceRegistry,
    HandleSigner,
    DocumentService,
    AuthorizedWorkspaceConfig,
) {
    let directory = tempfile::tempdir().expect("temporary document workspace");
    let registry = workspace_registry();
    let workspace = registry
        .authorize(directory.path(), "Documents")
        .expect("authorize document workspace");
    let signer = HandleSigner::new([31_u8; 32]);
    let service = DocumentService::new(signer.clone());
    (directory, registry, signer, service, workspace)
}

#[test]
fn document_read_lists_only_visible_markdown_with_opaque_paginated_ids() {
    let (directory, registry, signer, service, workspace) = document_service_fixture();
    std::fs::write(directory.path().join("a.md"), "alpha").expect("a.md");
    std::fs::write(directory.path().join("b.markdown"), "beta").expect("b.markdown");
    std::fs::write(directory.path().join("skip.txt"), "skip").expect("skip.txt");
    std::fs::create_dir(directory.path().join("docs")).expect("docs");
    std::fs::write(directory.path().join("docs/c.md"), "gamma").expect("c.md");
    std::fs::create_dir(directory.path().join(".qingyu")).expect("control directory");
    std::fs::write(directory.path().join(".qingyu/secret.md"), "secret").expect("control note");
    std::fs::create_dir_all(directory.path().join("node_modules/pkg"))
        .expect("dependency directory");
    std::fs::write(
        directory.path().join("node_modules/pkg/dependency.md"),
        "dependency",
    )
    .expect("dependency note");

    #[cfg(unix)]
    std::os::unix::fs::symlink(
        directory.path().join("docs/c.md"),
        directory.path().join("linked.md"),
    )
    .expect("linked note");

    let resolved = registry
        .resolve(workspace.workspace_id)
        .expect("resolve workspace");
    let scope = DocumentScope::authorized(resolved);
    let root_id = signer
        .issue_folder(workspace.workspace_id, registry.generation(), "")
        .expect("root folder ID");
    let root = signer
        .verify_folder(&root_id, &registry)
        .expect("verify root folder");

    let first = service
        .list(&scope, Some(&root), None, 2)
        .expect("first page");
    let repeated = service
        .list(&scope, Some(&root), None, 2)
        .expect("repeated first page");
    let second = service
        .list(&scope, Some(&root), first.next_cursor.as_deref(), 2)
        .expect("second page");

    assert_eq!(first.entries.len(), 2);
    assert_eq!(first.next_cursor, repeated.next_cursor);
    assert_eq!(second.entries.len(), 1);
    let all = first
        .entries
        .iter()
        .chain(&second.entries)
        .collect::<Vec<_>>();
    assert_eq!(
        all.iter()
            .map(|entry| entry.relative_path.as_str())
            .collect::<Vec<_>>(),
        vec!["a.md", "b.markdown", "docs"]
    );
    assert!(all.iter().all(|entry| !entry.id.contains(&entry.name)));
}

#[test]
fn document_read_searches_and_reads_with_exact_byte_revisions_and_limits() {
    let (directory, registry, signer, service, workspace) = document_service_fixture();
    std::fs::write(
        directory.path().join("note.md"),
        "first line\nSearch Needle here\n",
    )
    .expect("note");
    std::fs::write(directory.path().join("large.md"), "123456789").expect("large note");
    std::fs::create_dir(directory.path().join(".git")).expect("git control");
    std::fs::write(directory.path().join(".git/private.md"), "Search Needle")
        .expect("private note");
    let resolved = registry
        .resolve(workspace.workspace_id)
        .expect("resolve workspace");
    let scope = DocumentScope::authorized(resolved);

    let search = service
        .search(&scope, "needle", None, 100)
        .expect("search documents");
    assert_eq!(search.results.len(), 1);
    assert_eq!(search.results[0].relative_path, "note.md");
    assert_eq!(search.results[0].line_number, 2);

    let note_id = signer
        .issue_document(workspace.workspace_id, registry.generation(), "note.md")
        .expect("note ID");
    let note = signer
        .verify_document(&note_id, &registry)
        .expect("verify note");
    let first = service.read(&scope, &note, 1024).expect("read note");
    assert_eq!(first.contents, "first line\nSearch Needle here\n");
    assert_eq!(first.revision.0.len(), 64);

    std::fs::write(
        directory.path().join("note.md"),
        "first line\nSearch Needle changed\n",
    )
    .expect("change note bytes");
    let note = signer
        .verify_document(&note_id, &registry)
        .expect("verify changed note");
    let changed = service
        .read(&scope, &note, 1024)
        .expect("read changed note");
    assert_ne!(first.revision, changed.revision);

    let large_id = signer
        .issue_document(workspace.workspace_id, registry.generation(), "large.md")
        .expect("large ID");
    let large = signer
        .verify_document(&large_id, &registry)
        .expect("verify large note");
    let error = service
        .read(&scope, &large, 8)
        .expect_err("oversized document must fail");
    assert_eq!(error.code, "document_too_large");
}

#[test]
fn document_read_caps_pages_at_one_hundred_entries() {
    let (directory, registry, signer, service, workspace) = document_service_fixture();
    for index in 0..105 {
        std::fs::write(directory.path().join(format!("note-{index:03}.md")), "note")
            .expect("write paginated note");
    }
    let resolved = registry
        .resolve(workspace.workspace_id)
        .expect("resolve workspace");
    let scope = DocumentScope::authorized(resolved);
    let root_id = signer
        .issue_folder(workspace.workspace_id, registry.generation(), "")
        .expect("root folder ID");
    let root = signer
        .verify_folder(&root_id, &registry)
        .expect("verify root folder");

    let page = service
        .list(&scope, Some(&root), None, 500)
        .expect("capped page");

    assert_eq!(page.entries.len(), 100);
    assert!(page.next_cursor.is_some());
}

struct DocumentMutationFixture {
    _base: tempfile::TempDir,
    workspace_root: std::path::PathBuf,
    history_root: std::path::PathBuf,
    recycle_root: std::path::PathBuf,
    registry: WorkspaceRegistry,
    signer: HandleSigner,
    service: DocumentService,
    workspace: AuthorizedWorkspaceConfig,
}

fn document_mutation_fixture() -> DocumentMutationFixture {
    let base = tempfile::tempdir().expect("temporary mutation fixture");
    let workspace_root = base.path().join("workspace");
    let history_root = base.path().join("history");
    let recycle_root = base.path().join("recycle");
    std::fs::create_dir(&workspace_root).expect("workspace root");
    let registry = workspace_registry();
    let workspace = registry
        .authorize(&workspace_root, "Documents")
        .expect("authorize mutation workspace");
    let signer = HandleSigner::new([37_u8; 32]);
    let service = DocumentService::new(signer.clone())
        .with_mutation_storage(history_root.clone(), recycle_root.clone())
        .with_system_trash(|path| std::fs::remove_file(path).map_err(|error| error.to_string()));
    DocumentMutationFixture {
        _base: base,
        workspace_root,
        history_root,
        recycle_root,
        registry,
        signer,
        service,
        workspace,
    }
}

fn mutation_scope(fixture: &DocumentMutationFixture) -> DocumentScope {
    DocumentScope::authorized(
        fixture
            .registry
            .resolve(fixture.workspace.workspace_id)
            .expect("resolve mutation workspace"),
    )
}

fn mutation_root(fixture: &DocumentMutationFixture) -> super::handles::VerifiedFolderHandle {
    let root_id = fixture
        .signer
        .issue_folder(
            fixture.workspace.workspace_id,
            fixture.registry.generation(),
            "",
        )
        .expect("root folder ID");
    fixture
        .signer
        .verify_folder(&root_id, &fixture.registry)
        .expect("verify mutation root")
}

fn mutation_document(
    fixture: &DocumentMutationFixture,
    id: &str,
) -> super::handles::VerifiedDocumentHandle {
    fixture
        .signer
        .verify_document(id, &fixture.registry)
        .expect("verify mutation document")
}

fn sync_options(
    sync_after_write: SyncAfterWritePolicy,
    workspace_sync_enabled: bool,
) -> MutationOptions {
    MutationOptions {
        sync_after_write,
        workspace_sync_enabled,
    }
}

#[test]
fn document_mutation_creates_updates_and_moves_without_overwrite_or_stale_writes() {
    let fixture = document_mutation_fixture();
    let scope = mutation_scope(&fixture);
    let root = mutation_root(&fixture);
    let options = sync_options(SyncAfterWritePolicy::FollowWorkspace, true);

    let created = fixture
        .service
        .create(
            &scope,
            CreateDocument {
                parent: &root,
                name: "note.md",
                contents: "first",
            },
            options,
        )
        .expect("create document");
    assert_eq!(created.relative_path, "note.md");
    assert_eq!(created.sync_request, SyncRequest::Requested);
    assert_eq!(
        std::fs::read_to_string(fixture.workspace_root.join("note.md")).expect("created contents"),
        "first"
    );
    let create_error = fixture
        .service
        .create(
            &scope,
            CreateDocument {
                parent: &root,
                name: "note.md",
                contents: "must not overwrite",
            },
            options,
        )
        .expect_err("create must not overwrite");
    assert_eq!(create_error.code, "document_already_exists");

    let document = mutation_document(&fixture, &created.document_id);
    let stale_error = fixture
        .service
        .update(
            &scope,
            UpdateDocument {
                document: &document,
                contents: "second",
                expected_revision: "stale",
            },
            options,
        )
        .expect_err("stale update must fail");
    assert_eq!(stale_error.code, "revision_conflict");
    let updated = fixture
        .service
        .update(
            &scope,
            UpdateDocument {
                document: &document,
                contents: "second",
                expected_revision: &created.revision.0,
            },
            options,
        )
        .expect("revision-safe update");
    assert_ne!(updated.revision, created.revision);
    assert_eq!(
        std::fs::read_to_string(fixture.workspace_root.join("note.md")).expect("updated contents"),
        "second"
    );
    let history_snapshots = std::fs::read_dir(&fixture.history_root)
        .expect("history buckets")
        .flat_map(|bucket| {
            std::fs::read_dir(bucket.expect("history bucket").path().join("snapshots"))
                .into_iter()
                .flatten()
        })
        .map(|entry| {
            std::fs::read_to_string(entry.expect("history entry").path()).expect("history contents")
        })
        .collect::<Vec<_>>();
    assert_eq!(history_snapshots, vec!["first"]);
    assert!(std::fs::read_dir(&fixture.workspace_root)
        .expect("workspace entries")
        .all(|entry| !entry
            .expect("workspace entry")
            .file_name()
            .to_string_lossy()
            .starts_with(".qingyu-mcp-update-")));

    let stale_move = fixture
        .service
        .move_document(
            &scope,
            MoveDocument {
                document: &document,
                target_parent: &root,
                new_name: "moved.md",
                expected_revision: &created.revision.0,
            },
            options,
        )
        .expect_err("stale move must fail");
    assert_eq!(stale_move.code, "revision_conflict");
    std::fs::write(fixture.workspace_root.join("occupied.md"), "occupied")
        .expect("occupied target");
    let occupied = fixture
        .service
        .move_document(
            &scope,
            MoveDocument {
                document: &document,
                target_parent: &root,
                new_name: "occupied.md",
                expected_revision: &updated.revision.0,
            },
            options,
        )
        .expect_err("move must not overwrite");
    assert_eq!(occupied.code, "document_already_exists");
    let moved = fixture
        .service
        .move_document(
            &scope,
            MoveDocument {
                document: &document,
                target_parent: &root,
                new_name: "moved.md",
                expected_revision: &updated.revision.0,
            },
            options,
        )
        .expect("move document");
    assert_ne!(moved.document_id, updated.document_id);
    assert!(fixture
        .signer
        .verify_document(&updated.document_id, &fixture.registry)
        .is_err());
    assert!(fixture
        .signer
        .verify_document(&moved.document_id, &fixture.registry)
        .is_ok());
}

#[test]
fn document_mutation_applies_recycle_permanent_and_system_trash_deletion_modes() {
    let fixture = document_mutation_fixture();
    let scope = mutation_scope(&fixture);
    let root = mutation_root(&fixture);
    let options = sync_options(SyncAfterWritePolicy::Never, false);

    for (name, deletion) in [
        ("recycle.md", DeletionPolicy::QingYuRecycleBin),
        ("permanent.md", DeletionPolicy::Permanent),
        ("trash.md", DeletionPolicy::SystemTrash),
    ] {
        let created = fixture
            .service
            .create(
                &scope,
                CreateDocument {
                    parent: &root,
                    name,
                    contents: name,
                },
                options,
            )
            .expect("create deletion candidate");
        let document = mutation_document(&fixture, &created.document_id);
        let stale = fixture
            .service
            .delete(
                &scope,
                DeleteDocument {
                    document: &document,
                    expected_revision: "stale",
                    deletion,
                },
                options,
            )
            .expect_err("stale delete must fail");
        assert_eq!(stale.code, "revision_conflict");
        let deleted = fixture
            .service
            .delete(
                &scope,
                DeleteDocument {
                    document: &document,
                    expected_revision: &created.revision.0,
                    deletion,
                },
                options,
            )
            .expect("delete with configured mode");
        assert_eq!(deleted.revision, created.revision);
        assert_eq!(deleted.sync_request, SyncRequest::NotRequested);
        assert!(!fixture.workspace_root.join(name).exists());
    }

    let recycle_entries = std::fs::read_dir(&fixture.recycle_root)
        .expect("recycle entries")
        .collect::<Result<Vec<_>, _>>()
        .expect("read recycle entries");
    assert_eq!(recycle_entries.len(), 1);
    let recycle_entry = recycle_entries[0].path();
    assert_eq!(
        std::fs::read_to_string(recycle_entry.join("document.md")).expect("recycled contents"),
        "recycle.md"
    );
    let metadata =
        std::fs::read_to_string(recycle_entry.join("metadata.json")).expect("recycle metadata");
    assert!(metadata.contains(&fixture.workspace.workspace_id.to_string()));
    assert!(metadata.contains("recycle.md"));
}

#[test]
fn document_mutation_maps_all_sync_after_write_policies() {
    let fixture = document_mutation_fixture();
    let scope = mutation_scope(&fixture);
    let root = mutation_root(&fixture);
    let cases = [
        (
            SyncAfterWritePolicy::FollowWorkspace,
            false,
            SyncRequest::NotRequested,
        ),
        (
            SyncAfterWritePolicy::FollowWorkspace,
            true,
            SyncRequest::Requested,
        ),
        (SyncAfterWritePolicy::Always, false, SyncRequest::Requested),
        (SyncAfterWritePolicy::Never, true, SyncRequest::NotRequested),
    ];
    for (index, (policy, enabled, expected)) in cases.into_iter().enumerate() {
        let mutation = fixture
            .service
            .create(
                &scope,
                CreateDocument {
                    parent: &root,
                    name: &format!("sync-{index}.md"),
                    contents: "sync",
                },
                sync_options(policy, enabled),
            )
            .expect("create sync policy document");
        assert_eq!(mutation.sync_request, expected);
    }
}

#[cfg(unix)]
#[test]
fn document_mutation_rejects_a_symlink_swap_before_atomic_update() {
    let fixture = document_mutation_fixture();
    let scope = mutation_scope(&fixture);
    let root = mutation_root(&fixture);
    let options = sync_options(SyncAfterWritePolicy::Never, false);
    let created = fixture
        .service
        .create(
            &scope,
            CreateDocument {
                parent: &root,
                name: "race.md",
                contents: "inside",
            },
            options,
        )
        .expect("create race document");
    let document = mutation_document(&fixture, &created.document_id);
    let outside = fixture._base.path().join("outside.md");
    std::fs::write(&outside, "outside").expect("outside document");
    let target = fixture.workspace_root.join("race.md");

    let error = fixture
        .service
        .update_with_test_hook(
            &scope,
            UpdateDocument {
                document: &document,
                contents: "must not escape",
                expected_revision: &created.revision.0,
            },
            options,
            || {
                std::fs::remove_file(&target).expect("remove race target");
                std::os::unix::fs::symlink(&outside, &target).expect("swap race symlink");
            },
        )
        .expect_err("symlink swap must fail");
    assert_eq!(error.code, "path_boundary_violation");
    assert_eq!(
        std::fs::read_to_string(&outside).expect("outside remains readable"),
        "outside"
    );
}

#[derive(Default)]
struct FakeSyncRunner {
    calls: std::sync::atomic::AtomicUsize,
}

impl SyncRunner for FakeSyncRunner {
    fn run(
        &self,
        notes_root: std::path::PathBuf,
        revision: String,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<SyncRunResult, String>> + Send + 'static>,
    > {
        self.calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Box::pin(async move {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            Ok(SyncRunResult {
                notebook_name: notes_root
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                notes_root: notes_root.to_string_lossy().to_string(),
                provider: SyncProvider::Webdav,
                revision,
                summary: SyncSummary::default(),
                trigger: SyncTrigger::Manual,
            })
        })
    }
}

struct SyncToolFixture {
    _base: tempfile::TempDir,
    app_data: std::path::PathBuf,
    notes_root: std::path::PathBuf,
    runner: std::sync::Arc<FakeSyncRunner>,
    service: SyncService,
    revision: String,
}

fn sync_tools_fixture() -> SyncToolFixture {
    let base = tempfile::tempdir().expect("temporary sync workspace");
    let root = base.path().join("workspace");
    std::fs::create_dir(&root).expect("sync workspace root");
    let app_data = base.path().join("app-data");
    std::fs::create_dir(&app_data).expect("sync app data root");
    let enabled = enable_at_app_data(&app_data, None).expect("enable app sync config");
    let ready = patch_batch_at_app_data(
        &app_data,
        &enabled.document.revision,
        vec![
            SyncConfigPatch::Enabled(true),
            SyncConfigPatch::Provider(SyncProvider::Webdav),
            SyncConfigPatch::RemoteRoot("notes".to_string()),
            SyncConfigPatch::WebDavServerUrl("https://dav.example.test".to_string()),
            SyncConfigPatch::WebDavUsername("user".to_string()),
            SyncConfigPatch::WebDavPassword("secret".to_string()),
        ],
    )
    .expect("ready sync config");
    let runner = std::sync::Arc::new(FakeSyncRunner::default());
    let service = SyncService::new_for_test_with_app_data(
        runner.clone(),
        app_data.clone(),
        Some(root.clone()),
    );
    SyncToolFixture {
        _base: base,
        app_data,
        notes_root: root,
        runner,
        service,
        revision: ready.document.revision,
    }
}

#[test]
fn sync_tools_read_and_update_sanitized_config_without_losing_omitted_credentials() {
    let fixture = sync_tools_fixture();
    let initial = fixture.service.get_config().expect("sanitized sync config");
    let serialized = serde_json::to_string(&initial).expect("serialize sync config");
    assert!(initial.webdav_credentials_configured);
    assert!(!serialized.contains("secret"));
    assert!(!serialized.contains("password"));
    assert!(!serialized.contains(&fixture.notes_root.to_string_lossy().to_string()));

    let updated = fixture
        .service
        .update_config(SyncConfigPatchInput {
            expected_revision: fixture.revision.clone(),
            enabled: None,
            provider: None,
            remote_root: Some("archive".to_string()),
            auto_sync_on_save: Some(true),
            interval_minutes: None,
            webdav_server_url: None,
            s3_endpoint_url: None,
            s3_region: None,
            s3_bucket: None,
        })
        .expect("batch sync config update");
    assert_eq!(updated.remote_root, "archive");
    assert!(updated.auto_sync_on_save);
    assert_ne!(updated.revision, fixture.revision);

    let SyncConfigLoadResponse::Loaded { document } =
        load_from_app_data(&fixture.app_data).expect("load raw config")
    else {
        panic!("sync config should remain loaded");
    };
    assert_eq!(document.config.webdav.username, "user");
    assert_eq!(document.config.webdav.password, "secret");
}

#[test]
fn sync_tools_update_and_clear_credentials_without_returning_secret_values() {
    let fixture = sync_tools_fixture();
    let updated = fixture
        .service
        .update_credentials(SyncCredentialPatchInput {
            expected_revision: fixture.revision.clone(),
            webdav_username: Some("next-user".to_string()),
            webdav_password: Some("next-secret".to_string()),
            s3_access_key_id: None,
            s3_secret_access_key: None,
            clear_credentials: None,
        })
        .expect("update sync credentials");
    assert!(updated.webdav_credentials_configured);
    assert!(!serde_json::to_string(&updated)
        .expect("serialize updated credentials")
        .contains("next-secret"));

    let empty = fixture
        .service
        .update_credentials(SyncCredentialPatchInput {
            expected_revision: updated.revision.clone(),
            webdav_username: None,
            webdav_password: Some("".to_string()),
            s3_access_key_id: None,
            s3_secret_access_key: None,
            clear_credentials: None,
        })
        .expect_err("empty credential must fail");
    assert_eq!(empty.code, "invalid_sync_credentials");

    let cleared = fixture
        .service
        .update_credentials(SyncCredentialPatchInput {
            expected_revision: updated.revision,
            webdav_username: None,
            webdav_password: None,
            s3_access_key_id: None,
            s3_secret_access_key: None,
            clear_credentials: Some(true),
        })
        .expect("clear sync credentials");
    assert!(!cleared.webdav_credentials_configured);
    assert!(!cleared.s3_credentials_configured);
}

#[tokio::test]
async fn sync_tools_background_runs_return_early_coalesce_and_reach_sanitized_terminal_state() {
    let fixture = sync_tools_fixture();
    let first = fixture
        .service
        .run_background(&fixture.revision)
        .expect("start background sync");
    let second = fixture
        .service
        .run_background(&fixture.revision)
        .expect("coalesce background sync");
    assert_eq!(first.run_id, second.run_id);
    assert!(matches!(
        first.state,
        SyncRunState::Queued | SyncRunState::Running
    ));

    let completed = tokio::time::timeout(std::time::Duration::from_secs(1), async {
        loop {
            let status = fixture
                .service
                .status(first.run_id)
                .expect("read sync run status");
            if matches!(status.state, SyncRunState::Succeeded | SyncRunState::Failed) {
                break status;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("sync run should finish");
    assert_eq!(completed.state, SyncRunState::Succeeded);
    assert_eq!(
        fixture
            .runner
            .calls
            .load(std::sync::atomic::Ordering::Relaxed),
        1
    );
    let serialized = serde_json::to_string(&completed).expect("serialize run status");
    assert!(!serialized.contains(&fixture.notes_root.to_string_lossy().to_string()));
    assert!(!serialized.contains("secret"));
}

#[test]
fn sync_config_remains_application_readable_without_a_primary_workspace_but_run_fails_closed() {
    let fixture = sync_tools_fixture();
    let service = SyncService::new_for_test_with_app_data(fixture.runner, fixture.app_data, None);

    assert!(service.get_config().is_ok());
    let error = service
        .run_background(&fixture.revision)
        .expect_err("sync run requires the authoritative primary workspace");
    assert_eq!(error.code, "workspace_not_authorized");
}

#[test]
fn sync_after_write_is_enabled_only_for_the_authoritative_primary_workspace() {
    let fixture = sync_tools_fixture();
    let external = fixture._base.path().join("external");
    std::fs::create_dir(&external).expect("external workspace");

    assert!(fixture
        .service
        .sync_enabled_for_workspace(&fixture.notes_root));
    assert!(!fixture.service.sync_enabled_for_workspace(&external));
}

struct ToolRouterFixture {
    _base: tempfile::TempDir,
    config: std::sync::Arc<McpConfigManager>,
    handler: QingYuMcpHandler,
    mcp_settings: McpLocalSettingsService,
    workspace: AuthorizedWorkspaceConfig,
    workspaces: std::sync::Arc<WorkspaceRegistry>,
}

fn full_permissions() -> super::config::McpPermissions {
    super::config::McpPermissions {
        documents_read: true,
        documents_write: true,
        documents_move: true,
        documents_delete: true,
        settings_read: true,
        settings_write: true,
        sync_read: true,
        sync_write: true,
        sync_credentials_write: true,
        sync_run: true,
    }
}

fn tool_router_fixture() -> ToolRouterFixture {
    tool_router_fixture_with_services(|documents| documents, |sync| sync)
}

fn tool_router_fixture_with_documents(
    configure: impl FnOnce(DocumentService) -> DocumentService,
) -> ToolRouterFixture {
    tool_router_fixture_with_services(configure, |sync| sync)
}

fn tool_router_fixture_with_sync(
    configure: impl FnOnce(SyncService) -> SyncService,
) -> ToolRouterFixture {
    tool_router_fixture_with_services(|documents| documents, configure)
}

fn tool_router_fixture_with_services(
    configure_documents: impl FnOnce(DocumentService) -> DocumentService,
    configure_sync: impl FnOnce(SyncService) -> SyncService,
) -> ToolRouterFixture {
    let base = tempfile::tempdir().expect("temporary tool router fixture");
    let workspace_root = base.path().join("workspace");
    let audit_root = base.path().join("audit");
    std::fs::create_dir_all(&workspace_root).expect("workspace root");
    std::fs::write(workspace_root.join("note.md"), "hello").expect("fixture note");

    let workspaces = std::sync::Arc::new(workspace_registry());
    let workspace = workspaces
        .authorize(&workspace_root, "Notes")
        .expect("authorize tool workspace");
    let settings_service = AppSettingsService::memory_for_test();
    let mcp_settings = McpLocalSettingsService::memory_for_test();
    let config = std::sync::Arc::new(
        McpConfigManager::load(mcp_settings.clone()).expect("tool config manager"),
    );
    let initial = config.snapshot().expect("initial tool config");
    let enabled = McpConfig {
        enabled: true,
        permissions: full_permissions(),
        confirmation: ConfirmationPolicy::Never,
        dry_run: DryRunPolicy::Never,
        deletion: DeletionPolicy::Permanent,
        ..initial.config
    };
    config
        .update(enabled, &initial.revision)
        .expect("enable tool config");

    let signer = HandleSigner::new([47_u8; 32]);
    let documents = std::sync::Arc::new(configure_documents(DocumentService::new(signer)));
    let settings = std::sync::Arc::new(settings_service.clone());
    let runner = std::sync::Arc::new(FakeSyncRunner::default());
    let app_data = base.path().join("app-data");
    std::fs::create_dir(&app_data).expect("tool app data root");
    let enabled = enable_at_app_data(&app_data, None).expect("enable tool sync config");
    patch_batch_at_app_data(
        &app_data,
        &enabled.document.revision,
        vec![
            SyncConfigPatch::Enabled(true),
            SyncConfigPatch::RemoteRoot("notes".into()),
            SyncConfigPatch::WebDavServerUrl("https://dav.example.test".into()),
            SyncConfigPatch::WebDavUsername("user".into()),
            SyncConfigPatch::WebDavPassword("secret".into()),
        ],
    )
    .expect("ready tool sync config");
    let sync = std::sync::Arc::new(configure_sync(SyncService::new_for_test_with_app_data(
        runner,
        app_data,
        Some(workspace.canonical_path.clone()),
    )));
    let services = McpServices {
        config: config.clone(),
        workspaces: workspaces.clone(),
        documents,
        settings,
        sync,
        policy: std::sync::Arc::new(PolicyEngine::new([53_u8; 32])),
        audit: std::sync::Arc::new(AuditSink::new(
            &audit_root,
            super::config::AuditPolicy::default(),
        )),
    };
    let handler = QingYuMcpHandler::new_for_test(
        services,
        std::sync::Arc::new(FakeConfirmationPresenter {
            outcome: ConfirmationOutcome::Allowed,
        }),
    );
    ToolRouterFixture {
        _base: base,
        config,
        handler,
        mcp_settings,
        workspace,
        workspaces,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn workspace_list_and_primary_switch_do_not_reverse_transaction_and_authority_locks() {
    use std::{sync::mpsc, time::Duration};

    let simulated_primary_transaction = std::sync::Arc::new(std::sync::Mutex::new(()));
    let (resolver_called_sender, resolver_called_receiver) = mpsc::channel();
    let resolver_called_sender = std::sync::Mutex::new(Some(resolver_called_sender));
    let resolver_transaction = std::sync::Arc::clone(&simulated_primary_transaction);
    let fixture = tool_router_fixture_with_sync(move |sync| {
        sync.with_primary_root_resolver_for_test(move || {
            if let Some(sender) = resolver_called_sender
                .lock()
                .expect("resolver called sender")
                .take()
            {
                sender.send(()).expect("report primary-root lookup");
            }
            let deadline = std::time::Instant::now() + Duration::from_millis(200);
            while std::time::Instant::now() < deadline {
                if let Ok(transaction) = resolver_transaction.try_lock() {
                    drop(transaction);
                    return None;
                }
                std::thread::yield_now();
            }
            None
        })
    });
    let (lease_acquired_sender, lease_acquired_receiver) = mpsc::channel();
    let (release_lease_sender, release_lease_receiver) = mpsc::channel();
    let lease_acquired_sender = std::sync::Mutex::new(Some(lease_acquired_sender));
    let release_lease_receiver = std::sync::Mutex::new(Some(release_lease_receiver));
    fixture.workspaces.set_authority_read_hook(move || {
        if let Some(sender) = lease_acquired_sender
            .lock()
            .expect("lease acquired sender")
            .take()
        {
            sender.send(()).expect("report authority read lease");
            release_lease_receiver
                .lock()
                .expect("release lease receiver")
                .take()
                .expect("lease release channel")
                .recv()
                .expect("release authority lease");
        }
    });

    let next_workspace = fixture._base.path().join("lock-order-next");
    std::fs::create_dir(&next_workspace).expect("next workspace root");
    let switch_registry = std::sync::Arc::clone(&fixture.workspaces);
    let switch_transaction = std::sync::Arc::clone(&simulated_primary_transaction);
    let (transaction_held_sender, transaction_held_receiver) = mpsc::channel();
    let (start_switch_sender, start_switch_receiver) = mpsc::channel();
    let (switch_completed_sender, switch_completed_receiver) = mpsc::channel();
    let switch = std::thread::spawn(move || {
        let _transaction = switch_transaction.lock().expect("primary transaction");
        transaction_held_sender
            .send(())
            .expect("report primary transaction");
        start_switch_receiver
            .recv()
            .expect("start authority switch");
        let result = switch_registry.activate_current(&next_workspace);
        switch_completed_sender
            .send(result)
            .expect("report switch completion");
    });
    transaction_held_receiver
        .recv_timeout(Duration::from_secs(2))
        .expect("primary transaction held");

    let handler = fixture.handler.clone();
    let list = tokio::spawn(async move {
        handler
            .call_tool_current("workspace_list", serde_json::json!({}))
            .await
    });
    lease_acquired_receiver
        .recv_timeout(Duration::from_secs(2))
        .expect("workspace list acquired authority lease");
    start_switch_sender
        .send(())
        .expect("start workspace switch");
    release_lease_sender
        .send(())
        .expect("continue workspace list");

    let listed = tokio::time::timeout(Duration::from_secs(2), list)
        .await
        .expect("workspace list must complete")
        .expect("join workspace list")
        .expect("workspace list dispatch");
    let switched = switch_completed_receiver
        .recv_timeout(Duration::from_secs(2))
        .expect("workspace switch must complete");
    switch.join().expect("join workspace switch");

    assert_eq!(listed.is_error, Some(false));
    switched.expect("switch primary workspace");
    assert!(
        resolver_called_receiver.try_recv().is_err(),
        "workspace_list must not read local-state while holding an authority lease"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn primary_switch_waits_for_an_inflight_document_mutation_lease() {
    use std::{sync::mpsc, time::Duration};

    let (mutation_ready_sender, mutation_ready_receiver) = mpsc::channel();
    let (release_mutation_sender, release_mutation_receiver) = mpsc::channel();
    let mutation_ready_sender = std::sync::Mutex::new(Some(mutation_ready_sender));
    let release_mutation_receiver = std::sync::Mutex::new(Some(release_mutation_receiver));
    let fixture = tool_router_fixture_with_documents(|documents| {
        documents.with_before_atomic_mutation(move || {
            if let Some(sender) = mutation_ready_sender
                .lock()
                .expect("mutation ready sender")
                .take()
            {
                sender.send(()).expect("announce mutation boundary");
                release_mutation_receiver
                    .lock()
                    .expect("release mutation receiver")
                    .take()
                    .expect("mutation release channel")
                    .recv()
                    .expect("release mutation");
            }
        })
    });
    let listed = fixture
        .handler
        .call_tool_current("workspace_list", serde_json::json!({}))
        .await
        .expect("workspace list");
    let root_folder_id = structured(&listed)["workspaces"][0]["rootFolderId"]
        .as_str()
        .expect("root folder ID")
        .to_string();
    let documents = fixture
        .handler
        .call_tool_current(
            "document_list",
            serde_json::json!({
                "workspaceId": fixture.workspace.workspace_id,
                "parentFolderId": root_folder_id
            }),
        )
        .await
        .expect("document list");
    let document_id = structured(&documents)["entries"][0]["id"]
        .as_str()
        .expect("document ID")
        .to_string();
    let document = fixture
        .handler
        .call_tool_current(
            "document_read",
            serde_json::json!({ "documentId": document_id }),
        )
        .await
        .expect("document read");
    let revision = structured(&document)["revision"]["0"]
        .as_str()
        .or_else(|| structured(&document)["revision"].as_str())
        .expect("document revision")
        .to_string();

    let handler = fixture.handler.clone();
    let update_document_id = document_id.clone();
    let update = tokio::spawn(async move {
        handler
            .call_tool_current(
                "document_update",
                serde_json::json!({
                    "documentId": update_document_id,
                    "contents": "updated before the switch returns",
                    "expectedRevision": revision
                }),
            )
            .await
    });
    mutation_ready_receiver
        .recv_timeout(Duration::from_secs(2))
        .expect("mutation reached the final authority boundary");

    let next_workspace = fixture._base.path().join("next-workspace");
    std::fs::create_dir(&next_workspace).expect("next workspace root");
    let switching_registry = std::sync::Arc::clone(&fixture.workspaces);
    let (switch_completed_sender, switch_completed_receiver) = mpsc::channel();
    let switch = std::thread::spawn(move || {
        let result = switching_registry.activate_current(&next_workspace);
        switch_completed_sender
            .send(result)
            .expect("report workspace switch");
    });
    let switched_before_release = switch_completed_receiver
        .recv_timeout(Duration::from_millis(100))
        .ok();
    let switch_returned_early = switched_before_release.is_some();
    release_mutation_sender.send(()).expect("release mutation");
    let update_result = update.await.expect("join update").expect("update dispatch");
    let switch_result = switched_before_release.unwrap_or_else(|| {
        switch_completed_receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("workspace switch completes after mutation")
    });
    switch.join().expect("join workspace switch");

    assert!(
        !switch_returned_early,
        "workspace switch returned while an old-root mutation could still execute"
    );
    switch_result.expect("switch to next workspace");
    assert_eq!(update_result.is_error, Some(false));
    assert_eq!(
        std::fs::read_to_string(fixture.workspace.canonical_path.join("note.md"))
            .expect("old-root note"),
        "updated before the switch returns"
    );

    let stale = fixture
        .handler
        .call_tool_current(
            "document_update",
            serde_json::json!({
                "documentId": document_id,
                "contents": "must never reach the old root",
                "expectedRevision": structured(&update_result)["revision"]
            }),
        )
        .await
        .expect("stale update dispatch");
    assert_eq!(stale.is_error, Some(true));
    assert_eq!(structured(&stale)["code"], "mcp-handle-stale");
    assert_eq!(
        std::fs::read_to_string(fixture.workspace.canonical_path.join("note.md"))
            .expect("old-root note after stale update"),
        "updated before the switch returns"
    );
}

#[tokio::test]
async fn local_policy_reload_updates_manager_server_and_audit_policy() {
    let fixture = tool_router_fixture();
    let current = fixture.config.snapshot().expect("current policy");
    let mut synchronized = current.config.clone();
    synchronized.request_limit_bytes = 1024;
    synchronized.audit.enabled = false;
    let stored = fixture.mcp_settings.load_migrated().expect("stored policy");
    fixture
        .mcp_settings
        .write(&stored.revision, synchronized.clone())
        .expect("install local MCP policy");

    let audit_root = fixture._base.path().join("reload-audit");
    let audit = std::sync::Arc::new(AuditSink::new(&audit_root, current.config.audit.clone()));
    let controller = std::sync::Arc::new(McpServerController::new(
        fixture.handler.clone(),
        LocalIpcEndpoint::for_test(fixture._base.path().join("reload.sock")),
    ));
    let state = super::McpState {
        activation: tokio::sync::Mutex::new(()),
        audit: audit.clone(),
        client_command: Some("/Applications/QingYu.app/Contents/MacOS/qingyu-mcp".into()),
        config: fixture.config.clone(),
        controller: controller.clone(),
        policy: std::sync::Arc::new(PolicyEngine::new([91_u8; 32])),
        recycle_root: fixture._base.path().join("recycle"),
        workspaces: fixture.workspaces.clone(),
    };

    super::reload_current_mcp_policy(&state)
        .await
        .expect("apply local MCP policy");

    assert_eq!(state.config.snapshot().unwrap().config, synchronized);
    assert_eq!(controller.health().state, McpServerState::Running);
    audit
        .record(AuditEvent {
            request_id: uuid::Uuid::new_v4(),
            tool: "document_read".into(),
            workspace_id: None,
            workspace_display_name: None,
            logical_target: None,
            dry_run: false,
            confirmation: None,
            outcome: AuditOutcome::Succeeded,
            error_code: None,
            revision_before: None,
            revision_after: None,
            sync_run_id: None,
            duration_ms: 1,
            counts: std::collections::BTreeMap::new(),
        })
        .expect("disabled audit remains a successful no-op");
    assert!(!audit_root.join("mcp-audit.jsonl").exists());

    controller.stop().await.expect("stop reloaded MCP server");
}

#[test]
fn tool_router_lists_exact_dynamic_catalog_with_closed_schemas_and_annotations() {
    let fixture = tool_router_fixture();
    let tools = fixture
        .handler
        .list_tools_current()
        .expect("list MCP tools");
    let names = tools
        .iter()
        .map(|tool| tool.name.as_ref())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec![
            "workspace_list",
            "document_list",
            "document_search",
            "document_read",
            "document_create",
            "document_update",
            "document_move",
            "document_delete",
            "settings_get",
            "settings_update",
            "sync_config_get",
            "sync_config_update",
            "sync_credentials_update",
            "sync_test",
            "sync_run",
            "sync_status",
        ]
    );
    for tool in &tools {
        assert_eq!(
            tool.input_schema.get("additionalProperties"),
            Some(&serde_json::json!(false))
        );
        assert!(tool.annotations.is_some());
    }
    let sync_test = tools
        .iter()
        .find(|tool| tool.name == "sync_test")
        .expect("sync_test metadata");
    assert_eq!(
        sync_test
            .annotations
            .as_ref()
            .and_then(|annotations| annotations.open_world_hint),
        Some(true)
    );
    for name in [
        "sync_config_get",
        "sync_config_update",
        "sync_credentials_update",
        "sync_test",
        "sync_run",
        "sync_status",
    ] {
        let schema = &tools
            .iter()
            .find(|tool| tool.name == name)
            .expect("sync tool metadata")
            .input_schema;
        assert!(
            schema
                .get("properties")
                .and_then(serde_json::Value::as_object)
                .is_none_or(|properties| !properties.contains_key("workspaceId")),
            "{name} must be application-scoped"
        );
    }
}

#[tokio::test]
async fn tool_router_workspace_list_returns_a_signed_root_without_absolute_paths() {
    let fixture = tool_router_fixture();
    let result = fixture
        .handler
        .call_tool_current("workspace_list", serde_json::json!({}))
        .await
        .expect("workspace list dispatch");
    assert_eq!(result.is_error, Some(false));
    let structured = result
        .structured_content
        .expect("structured workspace list");
    assert_eq!(
        structured["workspaces"][0]["workspaceId"],
        fixture.workspace.workspace_id.to_string()
    );
    assert_eq!(
        structured["workspaces"][0]["workspaceGeneration"],
        fixture.workspaces.generation()
    );
    assert!(structured["workspaces"][0]["rootFolderId"].is_string());
    assert!(!structured.to_string().contains(
        &fixture
            .workspace
            .canonical_path
            .to_string_lossy()
            .to_string()
    ));
}

#[tokio::test]
async fn document_workspace_tools_fail_closed_without_a_primary_but_settings_remain_available() {
    let fixture = tool_router_fixture();
    fixture
        .workspaces
        .remove(fixture.workspace.workspace_id)
        .expect("clear primary workspace");

    let workspace = fixture
        .handler
        .call_tool_current("workspace_list", serde_json::json!({}))
        .await
        .expect("workspace failure should be structured");
    assert_eq!(workspace.is_error, Some(true));
    assert_eq!(structured(&workspace)["code"], "mcp-workspace-unavailable");

    let settings = fixture
        .handler
        .call_tool_current("settings_get", serde_json::json!({}))
        .await
        .expect("application settings do not depend on a workspace");
    assert_eq!(settings.is_error, Some(false));
}

#[tokio::test]
async fn tool_router_enforces_application_configured_call_rate_without_http_middleware() {
    let fixture = tool_router_fixture();
    let current = fixture.config.snapshot().expect("current MCP config");
    let mut limited = current.config;
    limited.requests_per_minute = 1;
    limited.burst_requests = 1;
    fixture
        .config
        .update(limited, &current.revision)
        .expect("set call rate limit");

    let first = fixture
        .handler
        .call_tool_current("workspace_list", serde_json::json!({}))
        .await
        .expect("first call");
    assert_eq!(first.is_error, Some(false));
    let second = fixture
        .handler
        .call_tool_current("workspace_list", serde_json::json!({}))
        .await
        .expect("rate limited call");
    assert_eq!(second.is_error, Some(true));
    assert_eq!(
        second.structured_content.expect("structured rate limit")["code"],
        "rate_limited"
    );
}

#[tokio::test]
async fn tool_router_rechecks_permissions_and_rejects_unknown_fields_as_structured_errors() {
    let fixture = tool_router_fixture();
    let current = fixture.config.snapshot().expect("current config");
    let mut revoked = current.config.clone();
    revoked.permissions.documents_read = false;
    fixture
        .config
        .update(revoked, &current.revision)
        .expect("revoke document read");

    let names = fixture
        .handler
        .list_tools_current()
        .expect("list after revocation")
        .into_iter()
        .map(|tool| tool.name.into_owned())
        .collect::<Vec<_>>();
    assert!(!names.iter().any(|name| name == "document_read"));
    let denied = fixture
        .handler
        .call_tool_current(
            "document_read",
            serde_json::json!({ "documentId": "cached-handle" }),
        )
        .await
        .expect("cached call should be a tool result");
    assert_eq!(denied.is_error, Some(true));
    assert_eq!(
        denied
            .structured_content
            .as_ref()
            .and_then(|value| value["code"].as_str()),
        Some("permission_denied")
    );

    let latest = fixture.config.snapshot().expect("revoked config");
    let mut restored = latest.config.clone();
    restored.permissions.documents_read = true;
    fixture
        .config
        .update(restored, &latest.revision)
        .expect("restore document read");
    let invalid = fixture
        .handler
        .call_tool_current(
            "document_list",
            serde_json::json!({
                "workspaceId": fixture.workspace.workspace_id,
                "unexpected": true
            }),
        )
        .await
        .expect("invalid arguments should be a tool result");
    assert_eq!(invalid.is_error, Some(true));
    let failure = invalid
        .structured_content
        .expect("structured invalid arguments");
    assert!(failure["code"].is_string());
    assert!(failure["message"].is_string());
    assert!(failure["retryable"].is_boolean());
    assert!(failure["recoveryHint"].is_string());
}

#[tokio::test]
async fn tool_router_fails_writes_closed_when_audit_storage_is_unavailable() {
    let fixture = tool_router_fixture();
    let audit_path = fixture._base.path().join("audit/mcp-audit.jsonl");
    std::fs::create_dir_all(&audit_path).expect("make audit target non-file");
    let workspaces = fixture
        .handler
        .call_tool_current("workspace_list", serde_json::json!({}))
        .await
        .expect("read-only tools may continue when audit is unavailable");
    assert_eq!(workspaces.is_error, Some(false));
    let root_folder_id = structured(&workspaces)["workspaces"][0]["rootFolderId"]
        .as_str()
        .expect("root folder ID");
    let create = fixture
        .handler
        .call_tool_current(
            "document_create",
            serde_json::json!({
                "workspaceId": fixture.workspace.workspace_id,
                "parentFolderId": root_folder_id,
                "name": "must-not-exist.md",
                "contents": "write must fail closed"
            }),
        )
        .await
        .expect("audit failure should be a structured tool result");

    assert_eq!(create.is_error, Some(true));
    assert_eq!(structured(&create)["code"], "audit_write_failed");
    assert!(!fixture
        .workspace
        .canonical_path
        .join("must-not-exist.md")
        .exists());
}

#[tokio::test]
async fn tool_router_replaces_oversized_results_with_a_bounded_error() {
    let fixture = tool_router_fixture();
    let current = fixture.config.snapshot().expect("current config");
    let mut limited = current.config;
    limited.response_limit_bytes = 64;
    fixture
        .config
        .update(limited, &current.revision)
        .expect("lower response limit");

    let result = fixture
        .handler
        .call_tool_current("workspace_list", serde_json::json!({}))
        .await
        .expect("oversized response should be a structured result");
    assert_eq!(result.is_error, Some(true));
    assert_eq!(structured(&result)["code"], "response_too_large");
}

#[tokio::test]
async fn tool_router_previews_then_commits_document_and_settings_mutations() {
    let fixture = tool_router_fixture();
    let listed = fixture
        .handler
        .call_tool_current("workspace_list", serde_json::json!({}))
        .await
        .expect("workspace list");
    let root_folder_id = listed
        .structured_content
        .as_ref()
        .expect("workspace output")["workspaces"][0]["rootFolderId"]
        .as_str()
        .expect("root folder ID")
        .to_string();
    let preview = fixture
        .handler
        .call_tool_current(
            "document_create",
            serde_json::json!({
                "workspaceId": fixture.workspace.workspace_id,
                "parentFolderId": root_folder_id,
                "name": "created.md",
                "contents": "created through MCP",
                "dryRun": true
            }),
        )
        .await
        .expect("document preview");
    let preview_token = preview.structured_content.as_ref().expect("preview output")
        ["previewToken"]
        .as_str()
        .expect("preview token")
        .to_string();
    assert!(!fixture.workspace.canonical_path.join("created.md").exists());

    let created = fixture
        .handler
        .call_tool_current(
            "document_create",
            serde_json::json!({
                "workspaceId": fixture.workspace.workspace_id,
                "parentFolderId": root_folder_id,
                "name": "created.md",
                "contents": "created through MCP",
                "previewToken": preview_token
            }),
        )
        .await
        .expect("document commit");
    assert_eq!(created.is_error, Some(false));
    let document_id = created.structured_content.as_ref().expect("created output")["documentId"]
        .as_str()
        .expect("created document ID")
        .to_string();
    let read = fixture
        .handler
        .call_tool_current(
            "document_read",
            serde_json::json!({ "documentId": document_id }),
        )
        .await
        .expect("read created document");
    assert_eq!(
        read.structured_content.as_ref().expect("read output")["contents"],
        "created through MCP"
    );

    let settings = fixture
        .handler
        .call_tool_current("settings_get", serde_json::json!({}))
        .await
        .expect("settings get");
    let settings_revision = settings
        .structured_content
        .as_ref()
        .expect("settings output")["revision"]
        .as_str()
        .expect("settings revision");
    let updated = fixture
        .handler
        .call_tool_current(
            "settings_update",
            serde_json::json!({
                "expectedRevision": settings_revision,
                "values": { "language": "zh-CN" }
            }),
        )
        .await
        .expect("settings update");
    assert_eq!(
        updated
            .structured_content
            .as_ref()
            .expect("updated settings")["values"]["language"],
        "zh-CN"
    );
}

async fn start_test_local_server(
    options: McpServerOptions,
) -> (ToolRouterFixture, McpServerController, LocalIpcEndpoint) {
    let fixture = tool_router_fixture();
    let endpoint = LocalIpcEndpoint::for_test(fixture._base.path().join("qingyu-mcp.sock"));
    let controller = McpServerController::new(fixture.handler.clone(), endpoint.clone());
    let health = controller
        .start(options)
        .await
        .expect("start MCP local IPC server");
    assert_eq!(health.endpoint.as_deref(), Some("local-ipc"));
    (fixture, controller, endpoint)
}

#[derive(Clone, Default)]
struct CountingLauncher {
    launches: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl AppLauncher for CountingLauncher {
    fn launch(&self) -> std::io::Result<()> {
        self.launches
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }
}

#[derive(Clone, Default)]
struct BridgeTestClient {
    tool_list_changes: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl ClientHandler for BridgeTestClient {
    async fn on_tool_list_changed(
        &self,
        _context: rmcp::service::NotificationContext<rmcp::RoleClient>,
    ) {
        self.tool_list_changes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

#[tokio::test]
async fn bridge_forwards_tools_calls_and_tool_list_notifications_over_stdio() {
    let (_fixture, controller, endpoint) =
        start_test_local_server(McpServerOptions::for_test()).await;
    let launcher = CountingLauncher::default();
    let launcher_count = launcher.launches.clone();
    let (client_io, bridge_io) = tokio::io::duplex(256 * 1024);
    let bridge_task = tokio::spawn(run_bridge(
        BridgeConfig::for_test(endpoint),
        launcher,
        bridge_io,
    ));
    let client_handler = BridgeTestClient::default();
    let notification_count = client_handler.tool_list_changes.clone();
    let client = client_handler
        .serve(client_io)
        .await
        .expect("initialize stdio client through bridge");

    let listed = client
        .peer()
        .list_tools(None)
        .await
        .expect("list forwarded tools");
    assert!(listed
        .tools
        .iter()
        .any(|tool| tool.name == "workspace_list"));
    let result = client
        .peer()
        .call_tool(rmcp::model::CallToolRequestParams::new("workspace_list"))
        .await
        .expect("call forwarded workspace list");
    assert_eq!(result.is_error, Some(false));
    assert_eq!(launcher_count.load(std::sync::atomic::Ordering::Relaxed), 0);

    controller.notify_tools_changed().await;
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while notification_count.load(std::sync::atomic::Ordering::Relaxed) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("forward tools/list_changed notification");

    client.cancel().await.expect("close bridge client");
    tokio::time::timeout(std::time::Duration::from_secs(1), bridge_task)
        .await
        .expect("bridge task should stop")
        .expect("bridge task should join")
        .expect("bridge should close cleanly");
    controller.stop().await.expect("stop MCP local server");
}

#[tokio::test]
async fn bridge_launches_qingyu_once_then_uses_bounded_reconnect() {
    let directory = tempfile::tempdir().expect("bridge temp directory");
    let settings_path = directory.path().join("settings.json");
    std::fs::write(&settings_path, br#"{"mcp":{"enabled":true}}"#).expect("write enabled settings");
    let launcher = CountingLauncher::default();
    let unavailable = directory.path().join("unavailable.sock");
    let error = test_connect_with_launch(
        &BridgeConfig::for_test_with_settings(
            LocalIpcEndpoint::for_test(unavailable),
            settings_path,
        ),
        &launcher,
    )
    .await
    .expect_err("unavailable upstream must remain bounded");

    assert_eq!(error, BridgeError::UpstreamUnavailable);
    assert_eq!(
        launcher.launches.load(std::sync::atomic::Ordering::Relaxed),
        1
    );
}

#[tokio::test]
async fn bridge_reports_disabled_without_launch_when_settings_are_missing() {
    let directory = tempfile::tempdir().expect("bridge temp directory");
    let launcher = CountingLauncher::default();
    let config = BridgeConfig::for_test_with_settings(
        LocalIpcEndpoint::for_test(directory.path().join("unavailable.sock")),
        directory.path().join("settings.json"),
    );

    let error = test_connect_with_launch(&config, &launcher)
        .await
        .expect_err("missing MCP settings must disable startup");

    assert!(error.to_string().contains("mcp_disabled"));
    assert!(error.to_string().contains("Settings"));
    assert_eq!(
        launcher.launches.load(std::sync::atomic::Ordering::Relaxed),
        0
    );
}

#[tokio::test]
async fn bridge_reports_disabled_without_launch_when_mcp_is_false() {
    let directory = tempfile::tempdir().expect("bridge temp directory");
    let settings_path = directory.path().join("settings.json");
    std::fs::write(&settings_path, br#"{"mcp":{"enabled":false}}"#)
        .expect("write disabled settings");
    let launcher = CountingLauncher::default();
    let config = BridgeConfig::for_test_with_settings(
        LocalIpcEndpoint::for_test(directory.path().join("unavailable.sock")),
        settings_path,
    );

    let error = test_connect_with_launch(&config, &launcher)
        .await
        .expect_err("disabled MCP settings must prevent startup");

    assert!(error.to_string().contains("mcp_disabled"));
    assert_eq!(
        launcher.launches.load(std::sync::atomic::Ordering::Relaxed),
        0
    );
}

#[tokio::test]
async fn bridge_reports_config_unavailable_without_launch_for_malformed_settings() {
    let directory = tempfile::tempdir().expect("bridge temp directory");
    let settings_path = directory.path().join("settings.json");
    std::fs::write(&settings_path, b"not-json").expect("write malformed settings");
    let launcher = CountingLauncher::default();
    let config = BridgeConfig::for_test_with_settings(
        LocalIpcEndpoint::for_test(directory.path().join("unavailable.sock")),
        settings_path,
    );

    let error = test_connect_with_launch(&config, &launcher)
        .await
        .expect_err("malformed MCP settings must fail closed");

    assert!(error.to_string().contains("mcp_config_unavailable"));
    assert_eq!(
        launcher.launches.load(std::sync::atomic::Ordering::Relaxed),
        0
    );
}

#[test]
fn bridge_transport_failures_are_indeterminate_and_never_retryable() {
    let serialized = serde_json::to_value(test_indeterminate_error())
        .expect("serialize bridge transport failure");
    assert_eq!(serialized["data"]["code"], "upstream_outcome_indeterminate");
    assert_eq!(serialized["data"]["retryable"], false);
    assert!(serialized["message"]
        .as_str()
        .expect("error message")
        .contains("did not replay"));
}

#[test]
fn bridge_startup_failures_use_stable_error_codes() {
    assert!(BridgeError::AppLaunchFailed
        .to_string()
        .contains("app_launch_failed"));
    assert!(BridgeError::UpstreamUnavailable
        .to_string()
        .contains("upstream_unavailable"));
}

async fn connect_test_local_client(
    endpoint: &LocalIpcEndpoint,
) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
    let stream = endpoint.connect().await.expect("connect local MCP stream");
    ().serve(super::ipc::bounded_transport::<rmcp::RoleClient, _>(
        stream,
        64 * 1024 * 1024,
    ))
    .await
    .expect("initialize MCP local client")
}

async fn call_client_tool(
    peer: &rmcp::service::Peer<rmcp::RoleClient>,
    name: &'static str,
    arguments: serde_json::Value,
) -> rmcp::model::CallToolResult {
    let arguments = arguments
        .as_object()
        .cloned()
        .expect("tool arguments must be an object");
    peer.call_tool(rmcp::model::CallToolRequestParams::new(name).with_arguments(arguments))
        .await
        .expect("tool request should return a protocol result")
}

fn structured(result: &rmcp::model::CallToolResult) -> &serde_json::Value {
    result
        .structured_content
        .as_ref()
        .expect("tool result should include structured content")
}

#[tokio::test]
async fn end_to_end_local_ipc_and_stdio_acceptance_revokes_every_old_capability() {
    let (fixture, controller, endpoint) =
        start_test_local_server(McpServerOptions::for_test()).await;
    let unauthorized = fixture._base.path().join("SENTINEL_ABSOLUTE_ROOT");
    std::fs::create_dir_all(&unauthorized).expect("create unauthorized workspace");
    let local_client = connect_test_local_client(&endpoint).await;
    let tools = local_client
        .peer()
        .list_tools(None)
        .await
        .expect("list enabled tools");
    assert_eq!(tools.tools.len(), 16);

    let workspaces =
        call_client_tool(local_client.peer(), "workspace_list", serde_json::json!({})).await;
    assert_eq!(workspaces.is_error, Some(false));
    let workspace = &structured(&workspaces)["workspaces"][0];
    let workspace_id = workspace["workspaceId"]
        .as_str()
        .expect("workspace ID")
        .to_string();
    let root_folder_id = workspace["rootFolderId"]
        .as_str()
        .expect("root folder ID")
        .to_string();
    let safe_workspaces = structured(&workspaces).to_string();
    assert!(!safe_workspaces.contains(&unauthorized.to_string_lossy().to_string()));
    assert!(!safe_workspaces.contains(
        &fixture
            .workspace
            .canonical_path
            .to_string_lossy()
            .to_string()
    ));

    let listed = call_client_tool(
        local_client.peer(),
        "document_list",
        serde_json::json!({
            "workspaceId": workspace_id,
            "parentFolderId": root_folder_id,
            "limit": 100
        }),
    )
    .await;
    let note_id = structured(&listed)["entries"]
        .as_array()
        .expect("document entries")
        .iter()
        .find(|entry| entry["name"] == "note.md")
        .and_then(|entry| entry["id"].as_str())
        .expect("fixture note ID")
        .to_string();
    let read = call_client_tool(
        local_client.peer(),
        "document_read",
        serde_json::json!({ "documentId": note_id }),
    )
    .await;
    assert_eq!(structured(&read)["contents"], "hello");

    let preview = call_client_tool(
        local_client.peer(),
        "document_create",
        serde_json::json!({
            "workspaceId": workspace_id,
            "parentFolderId": root_folder_id,
            "name": "preview.md",
            "contents": "preview only",
            "dryRun": true
        }),
    )
    .await;
    assert_eq!(preview.is_error, Some(false));
    assert!(structured(&preview)["previewToken"].is_string());
    assert!(!fixture.workspace.canonical_path.join("preview.md").exists());

    let created = call_client_tool(
        local_client.peer(),
        "document_create",
        serde_json::json!({
            "workspaceId": workspace_id,
            "parentFolderId": root_folder_id,
            "name": "created.md",
            "contents": "SENTINEL_DOCUMENT_BODY"
        }),
    )
    .await;
    let created_id = structured(&created)["documentId"]
        .as_str()
        .expect("created document ID")
        .to_string();
    let created_revision = structured(&created)["revision"]
        .as_str()
        .expect("created revision")
        .to_string();
    let stale = call_client_tool(
        local_client.peer(),
        "document_update",
        serde_json::json!({
            "documentId": created_id,
            "contents": "stale",
            "expectedRevision": "stale-revision"
        }),
    )
    .await;
    assert_eq!(stale.is_error, Some(true));
    assert_eq!(structured(&stale)["code"], "revision_conflict");

    let updated = call_client_tool(
        local_client.peer(),
        "document_update",
        serde_json::json!({
            "documentId": created_id,
            "contents": "updated",
            "expectedRevision": created_revision
        }),
    )
    .await;
    let updated_id = structured(&updated)["documentId"]
        .as_str()
        .expect("updated document ID")
        .to_string();
    let updated_revision = structured(&updated)["revision"]
        .as_str()
        .expect("updated revision")
        .to_string();
    let moved = call_client_tool(
        local_client.peer(),
        "document_move",
        serde_json::json!({
            "documentId": updated_id,
            "targetFolderId": root_folder_id,
            "newName": "moved.md",
            "expectedRevision": updated_revision
        }),
    )
    .await;
    let moved_id = structured(&moved)["documentId"]
        .as_str()
        .expect("moved document ID")
        .to_string();
    let moved_revision = structured(&moved)["revision"]
        .as_str()
        .expect("moved revision")
        .to_string();

    let settings =
        call_client_tool(local_client.peer(), "settings_get", serde_json::json!({})).await;
    assert_eq!(settings.is_error, Some(false));
    for secret_field in ["password", "secretAccessKey", "bearerToken"] {
        assert!(!structured(&settings).to_string().contains(secret_field));
    }
    let sync = call_client_tool(
        local_client.peer(),
        "sync_config_get",
        serde_json::json!({}),
    )
    .await;
    assert_eq!(sync.is_error, Some(false));
    let sync_revision = structured(&sync)["config"]["revision"]
        .as_str()
        .expect("sync revision")
        .to_string();
    let credentials = call_client_tool(
        local_client.peer(),
        "sync_credentials_update",
        serde_json::json!({
            "expectedRevision": sync_revision,
            "s3AccessKeyId": "sentinel-access",
            "s3SecretAccessKey": "SENTINEL_S3_SECRET"
        }),
    )
    .await;
    assert_eq!(credentials.is_error, Some(false));
    assert!(!structured(&credentials)
        .to_string()
        .contains("SENTINEL_S3_SECRET"));

    let deleted = call_client_tool(
        local_client.peer(),
        "document_delete",
        serde_json::json!({
            "documentId": moved_id,
            "expectedRevision": moved_revision
        }),
    )
    .await;
    assert_eq!(deleted.is_error, Some(false));
    assert!(!fixture.workspace.canonical_path.join("moved.md").exists());

    local_client.cancel().await.expect("close local client");

    let (stdio_client_io, bridge_io) = tokio::io::duplex(256 * 1024);
    let bridge_task = tokio::spawn(run_bridge(
        BridgeConfig::for_test(endpoint.clone()),
        CountingLauncher::default(),
        bridge_io,
    ));
    let stdio_client = BridgeTestClient::default()
        .serve(stdio_client_io)
        .await
        .expect("initialize acceptance stdio client");
    let stdio_tools = stdio_client
        .peer()
        .list_tools(None)
        .await
        .expect("list tools through stdio");
    assert!(stdio_tools
        .tools
        .iter()
        .any(|tool| tool.name == "document_read"));
    let stdio_read = call_client_tool(
        stdio_client.peer(),
        "document_read",
        serde_json::json!({ "documentId": note_id }),
    )
    .await;
    assert_eq!(structured(&stdio_read)["contents"], "hello");
    let stdio_created = call_client_tool(
        stdio_client.peer(),
        "document_create",
        serde_json::json!({
            "workspaceId": workspace_id,
            "parentFolderId": root_folder_id,
            "name": "bridge.md",
            "contents": "bridge write"
        }),
    )
    .await;
    let bridge_document_id = structured(&stdio_created)["documentId"]
        .as_str()
        .expect("bridge document ID")
        .to_string();

    fixture
        .workspaces
        .remove(fixture.workspace.workspace_id)
        .expect("remove authorized root");
    controller.notify_tools_changed().await;
    let revoked = call_client_tool(
        stdio_client.peer(),
        "document_read",
        serde_json::json!({ "documentId": bridge_document_id }),
    )
    .await;
    assert_eq!(revoked.is_error, Some(true));
    assert_eq!(structured(&revoked)["code"], "mcp-handle-stale");

    let artifact_root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/mcp-test-artifacts");
    let _remove_old_artifacts = std::fs::remove_dir_all(&artifact_root);
    std::fs::create_dir_all(&artifact_root).expect("create MCP artifact directory");
    let audit_path = fixture._base.path().join("audit/mcp-audit.jsonl");
    let audit = std::fs::read_to_string(audit_path).expect("read acceptance audit");
    std::fs::write(artifact_root.join("audit.jsonl"), &audit)
        .expect("write acceptance audit artifact");
    for sentinel in [
        "SENTINEL_BEARER",
        "SENTINEL_S3_SECRET",
        "SENTINEL_DOCUMENT_BODY",
        "SENTINEL_ABSOLUTE_ROOT",
    ] {
        assert!(
            !audit.contains(sentinel),
            "acceptance audit leaked {sentinel}"
        );
    }

    stdio_client
        .cancel()
        .await
        .expect("close acceptance client");
    tokio::time::timeout(std::time::Duration::from_secs(1), bridge_task)
        .await
        .expect("acceptance bridge should stop")
        .expect("acceptance bridge should join")
        .expect("acceptance bridge should close cleanly");
    controller.stop().await.expect("stop acceptance server");
}

#[tokio::test]
async fn local_ipc_server_needs_no_secret_and_stops_cleanly() {
    let (_fixture, controller, endpoint) =
        start_test_local_server(McpServerOptions::for_test()).await;
    let client = connect_test_local_client(&endpoint).await;

    assert_eq!(controller.active_session_count(), 1);
    assert!(client.peer().list_tools(None).await.is_ok());
    client.cancel().await.expect("close local IPC client");
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while controller.active_session_count() != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("local IPC session should close");

    controller.stop().await.expect("stop local IPC server");
    assert_eq!(controller.health().state, McpServerState::Stopped);
    assert!(endpoint.connect().await.is_err());
}

#[tokio::test]
async fn local_ipc_server_bounds_concurrent_sessions() {
    let (_fixture, controller, endpoint) =
        start_test_local_server(McpServerOptions::for_test()).await;
    let mut clients = Vec::new();
    for _ in 0..=MAX_ACTIVE_SESSIONS {
        clients.push(
            endpoint
                .connect()
                .await
                .expect("connect raw local IPC client"),
        );
    }

    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while controller.active_session_count() < MAX_ACTIVE_SESSIONS {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("server should accept up to its session limit");
    assert_eq!(controller.active_session_count(), MAX_ACTIVE_SESSIONS);

    drop(clients);
    controller
        .stop()
        .await
        .expect("stop bounded local IPC server");
}

#[tokio::test]
async fn local_ipc_server_reports_disabled_and_occupied_endpoints() {
    let fixture = tool_router_fixture();
    let endpoint = LocalIpcEndpoint::for_test(fixture._base.path().join("disabled.sock"));
    let disabled = McpServerController::new(fixture.handler.clone(), endpoint);
    let health = disabled
        .start(McpServerOptions {
            enabled: false,
            ..McpServerOptions::for_test()
        })
        .await
        .expect("disabled server is a valid state");
    assert_eq!(health.state, McpServerState::Disabled);

    let occupied_path = fixture._base.path().join("occupied.sock");
    std::fs::write(&occupied_path, "not a socket").expect("occupy endpoint path");
    let occupied = McpServerController::new(
        fixture.handler,
        LocalIpcEndpoint::for_test(occupied_path.clone()),
    );
    assert!(occupied.start(McpServerOptions::for_test()).await.is_err());
    assert_eq!(occupied.health().state, McpServerState::Error);
    assert_eq!(
        std::fs::read_to_string(occupied_path).expect("occupied file remains"),
        "not a socket"
    );
}

#[tokio::test]
async fn local_ipc_server_closes_a_connection_after_an_oversized_frame() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let options = McpServerOptions {
        request_limit_bytes: 256,
        ..McpServerOptions::for_test()
    };
    let (_fixture, controller, endpoint) = start_test_local_server(options).await;
    let mut stream = endpoint.connect().await.expect("connect oversized client");
    stream
        .write_all(format!("{}\n", "x".repeat(257)).as_bytes())
        .await
        .expect("write oversized frame");
    let mut byte = [0_u8; 1];
    let read = tokio::time::timeout(std::time::Duration::from_secs(1), stream.read(&mut byte))
        .await
        .expect("oversized connection should close")
        .expect("read closed connection");

    assert_eq!(read, 0);
    controller.stop().await.expect("stop bounded server");
}

#[test]
fn bridge_configuration_contains_no_url_or_secret() {
    let endpoint = LocalIpcEndpoint::for_test(std::path::PathBuf::from("/tmp/qingyu-test.sock"));
    let config = BridgeConfig::for_test(endpoint);
    let debug = format!("{config:?}");

    assert!(!debug.contains("http://"));
    assert!(!debug.contains("token"));
    assert!(!debug.contains("secret"));
}
