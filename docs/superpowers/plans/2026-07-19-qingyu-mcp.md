# QingYu MCP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a disabled-by-default, loopback-only QingYu MCP service that exposes globally authorized document, settings, and sync operations through QingYu-owned services, plus a forwarding-only `qingyu-mcp` stdio bridge.

**Architecture:** The Tauri Rust process owns MCP Streamable HTTP transport, authentication, global authorization, signed handles, guarded workspace capabilities, policy enforcement, auditing, and tool routing. Existing UI commands and MCP tools share document/settings/sync service facades; clients never receive absolute document paths. A separate Rust stdio binary connects to the loopback endpoint and forwards protocol calls without any business or filesystem logic.

**Tech Stack:** Rust 2021, Tauri 2.11, `rmcp` 2.2.0, Tokio, Axum/Tower, `cap-std`/`cap-fs-ext`, HMAC-SHA-256, OS keyring, React 19, TypeScript 6, Tailwind CSS, Vitest, pnpm.

## Global Constraints

- Chinese product name is `轻语`; English product and MCP display title are `QingYu`; server name is `qingyu`; bridge executable is `qingyu-mcp`.
- Keep current `markra` package, crate, event, and bundle identifiers. Package/identifier renaming is out of scope.
- Bind only `127.0.0.1`; expose Streamable HTTP only at `/mcp`; default port is exactly `19618`.
- MCP is disabled by default. Enabling it requires a token in the OS credential store; no plaintext fallback is allowed.
- One global permission policy applies to all authenticated clients. Do not add per-client permissions, per-client tokens, or client-specific roots.
- Only directories explicitly authorized in QingYu's MCP settings are visible. Current window/workspace state must not affect MCP authorization.
- Do not add `workspace_open`, `workspace_close`, arbitrary-path tools, MCP resources, or MCP prompts.
- MCP clients never receive or submit absolute document paths. Existing objects use signed `documentId`/`folderId`; creation uses `workspaceId + parentFolderId + name`.
- Reject symlinks/reparse points, absolute paths, traversal, protected paths, unknown schema fields, stale revisions, and implicit overwrite.
- Never return or log Bearer tokens, signing keys, sync secrets, document bodies in audit records, or absolute workspace paths in MCP output.
- Use `pnpm` for JavaScript workflows; keep `pnpm-lock.yaml`; do not introduce another JavaScript lockfile.
- Do not use the TypeScript `void` keyword or operator.
- Keep the existing S3/WebDAV sync coordinator, conflict behavior, project revision checks, history, watcher notifications, and local product capabilities.
- Execute the plan in an isolated worktree created with `superpowers:using-git-worktrees`; preserve the primary checkout's untracked `bg.png`.

---

## File and Responsibility Map

### Native MCP control plane

- Create `apps/desktop/src-tauri/src/mcp/mod.rs`: shared `McpState`, Tauri command facade, lifecycle entry points.
- Create `apps/desktop/src-tauri/src/mcp/error.rs`: stable error codes and safe MCP tool results.
- Create `apps/desktop/src-tauri/src/mcp/config.rs`: versioned app-private config, revisions, global permissions/policies, authorized-root records.
- Create `apps/desktop/src-tauri/src/mcp/secrets.rs`: Bearer/signing-key storage abstraction and keyring implementation.
- Create `apps/desktop/src-tauri/src/mcp/workspaces.rs`: live authorized workspace registry and `cap-std` roots.
- Create `apps/desktop/src-tauri/src/mcp/handles.rs`: signed folder/document handles.
- Create `apps/desktop/src-tauri/src/mcp/policy.rs`: permission checks, confirmation policy, preview tokens, limits.
- Create `apps/desktop/src-tauri/src/mcp/confirmation.rs`: app-owned confirmation presenter backed by the native dialog plugin.
- Create `apps/desktop/src-tauri/src/mcp/audit.rs`: redacted bounded audit store.
- Create `apps/desktop/src-tauri/src/mcp/server.rs`: authenticated loopback Axum + `rmcp` service lifecycle.
- Create `apps/desktop/src-tauri/src/mcp/tools/mod.rs`: dynamic `ServerHandler`, tool definitions, closed-schema dispatch.
- Create `apps/desktop/src-tauri/src/mcp/tools/workspace.rs`: `workspace_list`.
- Create `apps/desktop/src-tauri/src/mcp/tools/document.rs`: document tools.
- Create `apps/desktop/src-tauri/src/mcp/tools/settings.rs`: typed application-settings tools.
- Create `apps/desktop/src-tauri/src/mcp/tools/sync.rs`: sanitized sync tools and background run IDs.
- Create `apps/desktop/src-tauri/src/mcp/tests.rs`: native unit/integration coverage with in-memory secrets and temporary roots.

### Shared application services

- Create `apps/desktop/src-tauri/src/markdown_files/service.rs`: shared document read/list/search/mutation facade with UI-path and MCP-capability resolvers.
- Modify `apps/desktop/src-tauri/src/markdown_files.rs`, `document.rs`, `tree.rs`, `search.rs`, `history.rs`, and `ignore_rules.rs`: route reusable behavior through the service and make protected-path rules consistent.
- Create `apps/desktop/src-tauri/src/app_settings.rs`: native group persistence plus MCP field registry and revisioned patches.
- Modify `apps/desktop/src-tauri/src/project_config/storage.rs` and `project_config.rs`: add one-write batch patching and sanitized service calls.
- Create `apps/desktop/src-tauri/src/remote_sync/service.rs`: shared sync facade and sanitized background run registry.
- Modify `apps/desktop/src-tauri/src/remote_sync.rs` and `remote_sync/coordinator.rs`: route Tauri/MCP calls through the shared facade without duplicating the engine.

### Tauri/frontend integration

- Modify `apps/desktop/src-tauri/src/lib.rs`: manage `McpState`, start/stop it, and register control-plane commands.
- Create `apps/desktop/src/runtime/tauri/mcp.ts`: typed Tauri invokes.
- Modify `apps/desktop/src/runtime/index.ts`: provide the desktop MCP runtime.
- Modify `packages/app/src/runtime/index.ts`: define `AppMcpRuntime` and native settings-group methods, plus safe browser defaults.
- Create `packages/app/src/lib/mcp.ts`: frontend configuration, policy, permission, audit, and health types.
- Create `packages/app/src/hooks/useMcpSettings.ts`: revision-aware MCP settings orchestration.
- Create `packages/app/src/components/settings/McpSettings.tsx` and `.test.tsx`: MCP control page.
- Modify `packages/app/src/components/SettingsShell.tsx`, `SettingsWindow.tsx`, and `hooks/useSettingsWindowState.ts`: add the MCP category.
- Modify `packages/app/src/lib/settings/app-settings.ts` and focused tests: route MCP-exposed setting groups through the native shared service on desktop.
- Modify every `packages/shared/src/i18n/locales/*.ts` and `i18n/index.test.ts`: add MCP settings copy.

### Bridge, packaging, and documentation

- Create `apps/desktop/src-tauri/src/bin/qingyu-mcp.rs`: forwarding-only stdio bridge.
- Create `apps/desktop/src-tauri/src/mcp/bridge.rs`: upstream client/downstream proxy implementation reusable by bridge tests.
- Create `packages/scripts/src/prepare-qingyu-mcp-sidecar.mjs`: build/copy the target-suffixed sidecar for Tauri packaging.
- Modify `package.json`, `apps/desktop/src-tauri/tauri.conf.json`, and `.gitignore`: build and bundle the sidecar without committing binaries.
- Create `docs/qingyu-mcp.md`: end-user HTTP and stdio configuration examples and security behavior.
- Modify `README.md` and `README.zh-CN.md`: link the MCP guide from their existing documentation sections.

---

### Task 1: Pin the official MCP SDK and establish the protocol boundary

**Files:**
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/Cargo.lock`
- Create: `apps/desktop/src-tauri/src/mcp/mod.rs`
- Create: `apps/desktop/src-tauri/src/mcp/error.rs`
- Create: `apps/desktop/src-tauri/src/mcp/tools/mod.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`

**Interfaces:**
- Produces: initial `QingYuMcpHandler::for_test()`, `ServerHandler` metadata `{ name: "qingyu", title: "QingYu" }`, and `McpToolFailure`.
- Consumes: no earlier task.

- [ ] **Step 1: Add a failing server metadata test**

In `mcp/tools/mod.rs`, add the test before the implementation:

```rust
#[cfg(test)]
mod tests {
    use rmcp::ServerHandler;

    #[test]
    fn server_identity_uses_qingyu_without_resources_or_prompts() {
        let handler = super::QingYuMcpHandler::for_test();
        let info = handler.get_info();

        assert_eq!(info.server_info.name, "qingyu");
        assert_eq!(info.server_info.title.as_deref(), Some("QingYu"));
        assert!(info.capabilities.tools.is_some());
        assert!(info.capabilities.resources.is_none());
        assert!(info.capabilities.prompts.is_none());
    }
}
```

- [ ] **Step 2: Run the focused test and observe the missing module/type failure**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tools::tests::server_identity_uses_qingyu_without_resources_or_prompts -- --exact`

Expected: FAIL because `mcp`, `QingYuMcpHandler`, and its metadata do not exist.

- [ ] **Step 3: Add exact dependency versions and the minimal handler**

Add these dependencies and features:

```toml
axum = "0.8.9"
base64 = "0.22.1"
cap-fs-ext = "4.0.2"
cap-std = "4.0.2"
getrandom = "0.4.3"
hmac = "0.13.0"
keyring = "4.1.5"
rmcp = { version = "2.2.0", default-features = false, features = ["base64", "client", "macros", "server", "transport-io", "transport-streamable-http-client-reqwest", "transport-streamable-http-server"] }
sha2 = "0.11.0"
subtle = "2.6.1"
tokio = { version = "1", features = ["io-std", "macros", "net", "process", "rt-multi-thread", "sync", "time"] }
tokio-util = "0.7.18"
tower = { version = "0.5.3", features = ["limit", "util"] }
trash = "5.2.6"
uuid = { version = "1.24.0", features = ["serde", "v4"] }

[dev-dependencies]
tempfile = "3.27.0"
```

Define the safe error envelope in `mcp/error.rs`:

```rust
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct McpToolFailure {
    pub(crate) code: &'static str,
    pub(crate) message: String,
    pub(crate) retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) recovery_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) current_revision: Option<String>,
}
```

Define `QingYuMcpHandler` with a tools-only `ServerInfo`; leave its initial `list_tools` empty and `call_tool` method-not-found. Declare `mod mcp;` in `lib.rs`.

- [ ] **Step 4: Run the focused test and compile all native tests**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tools::tests::server_identity_uses_qingyu_without_resources_or_prompts -- --exact`

Expected: PASS.

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --no-run`

Expected: PASS with `rmcp 2.2.0` in the resolved dependency graph.

- [ ] **Step 5: Commit the protocol foundation**

```bash
git add apps/desktop/src-tauri/Cargo.toml apps/desktop/src-tauri/Cargo.lock apps/desktop/src-tauri/src/lib.rs apps/desktop/src-tauri/src/mcp
git commit -m "feat: add QingYu MCP protocol foundation"
```

---

### Task 2: Persist global MCP configuration and protect connection secrets

**Files:**
- Create: `apps/desktop/src-tauri/src/mcp/config.rs`
- Create: `apps/desktop/src-tauri/src/mcp/secrets.rs`
- Modify: `apps/desktop/src-tauri/src/mcp/mod.rs`
- Create: `apps/desktop/src-tauri/src/mcp/tests.rs`

**Interfaces:**
- Produces: `McpConfig`, `McpConfigDocument`, `McpConfigStore`, `McpConfigManager`, `McpSecretStore`, `KeyringSecretStore`, and `MemorySecretStore`.
- Produces: `McpPermissions::allows(ToolCapability)` and all approved policy enums.
- Consumes: `McpToolFailure` from Task 1.

- [ ] **Step 1: Write failing default/config-revision/secret tests**

Add tests that assert:

```rust
#[test]
fn default_config_is_disabled_and_uses_the_approved_policy() {
    let config = McpConfig::default();
    assert!(!config.enabled);
    assert_eq!(config.port, 19_618);
    assert_eq!(config.confirmation, ConfirmationPolicy::DestructiveOnly);
    assert_eq!(config.dry_run, DryRunPolicy::HighRisk);
    assert_eq!(config.deletion, DeletionPolicy::SystemTrash);
    assert_eq!(config.sync_after_write, SyncAfterWritePolicy::FollowWorkspace);
    assert_eq!(config.sync_execution, SyncExecutionPolicy::Background);
    assert!(config.audit.enabled);
}

#[test]
fn memory_secrets_keep_bearer_and_signing_keys_independent() {
    let secrets = MemorySecretStore::default();
    let bearer = secrets.ensure_bearer_token().unwrap();
    let signing = secrets.ensure_signing_key().unwrap();
    assert_eq!(bearer.len(), 43);
    assert_eq!(signing.len(), 32);
    assert_ne!(bearer.as_bytes(), signing.as_slice());
    assert_eq!(secrets.ensure_bearer_token().unwrap(), bearer);
}
```

Also verify canonical serialization yields a stable SHA-256 revision and unknown JSON fields fail to load.

- [ ] **Step 2: Run focused tests and verify failure**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tests::config -- --nocapture`

Expected: FAIL because the config and secret stores do not exist.

- [ ] **Step 3: Implement versioned configuration and secret abstractions**

Use these exact model shapes:

```rust
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct McpConfig {
    pub(crate) version: u32,
    pub(crate) enabled: bool,
    pub(crate) port: u16,
    pub(crate) permissions: McpPermissions,
    pub(crate) workspaces: Vec<AuthorizedWorkspaceConfig>,
    pub(crate) confirmation: ConfirmationPolicy,
    pub(crate) dry_run: DryRunPolicy,
    pub(crate) deletion: DeletionPolicy,
    pub(crate) sync_after_write: SyncAfterWritePolicy,
    pub(crate) sync_execution: SyncExecutionPolicy,
    pub(crate) document_limit_bytes: u64,
    pub(crate) request_limit_bytes: u64,
    pub(crate) response_limit_bytes: u64,
    pub(crate) requests_per_minute: u32,
    pub(crate) burst_requests: u32,
    pub(crate) concurrent_calls: usize,
    pub(crate) tool_timeout_secs: u64,
    pub(crate) audit: AuditPolicy,
}

#[derive(Clone, Debug, Default, serde::Deserialize, serde::Serialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct McpPermissions {
    pub(crate) documents_read: bool,
    pub(crate) documents_write: bool,
    pub(crate) documents_move: bool,
    pub(crate) documents_delete: bool,
    pub(crate) settings_read: bool,
    pub(crate) settings_write: bool,
    pub(crate) sync_read: bool,
    pub(crate) sync_write: bool,
    pub(crate) sync_credentials_write: bool,
    pub(crate) sync_run: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ToolCapability {
    DocumentsRead,
    DocumentsWrite,
    DocumentsMove,
    DocumentsDelete,
    SettingsRead,
    SettingsWrite,
    SyncRead,
    SyncWrite,
    SyncCredentialsWrite,
    SyncRun,
}
```

Set request, response, and document limits to 8 MiB by default and clamp each to `1..=64 MiB`; set rate to 120/minute with burst 20, concurrency 8, and tool timeout 60 seconds. Clamp rate to `1..=600`, burst to `1..=100`, concurrency to `1..=32`, and timeout to `5..=600` seconds. `AuditPolicy` defaults to enabled, 30 retention days, and 10,000 entries, clamped to 1–365 days and 100–100,000 entries. Reject nested/duplicate workspace roots during writes. Persist `mcp.json` under `app_config_dir` with a sibling temporary file, flush, atomic replace, and directory sync where supported. Store service `QingYu MCP` entries `bearer-token` and `handle-signing-key` through `keyring::Entry`; return an error instead of writing either secret to disk. Wrap the current document plus a monotonically increasing in-memory generation in `McpConfigManager`; policy/root/security changes increment that generation.

- [ ] **Step 4: Verify config and secret behavior**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tests:: -- --nocapture`

Expected: PASS, including token rotation/revocation and no plaintext token in serialized config.

- [ ] **Step 5: Commit configuration and secrets**

```bash
git add apps/desktop/src-tauri/src/mcp
git commit -m "feat: persist MCP policy and secure secrets"
```

---

### Task 3: Authorize capability-rooted workspaces and issue signed object handles

**Files:**
- Create: `apps/desktop/src-tauri/src/mcp/workspaces.rs`
- Create: `apps/desktop/src-tauri/src/mcp/handles.rs`
- Modify: `apps/desktop/src-tauri/src/mcp/config.rs`
- Modify: `apps/desktop/src-tauri/src/mcp/tests.rs`

**Interfaces:**
- Produces: `WorkspaceRegistry::authorize`, `remove`, `list_safe`, `resolve`.
- Produces: `HandleSigner::issue_document`, `issue_folder`, and `verify`.
- Produces: distinct `VerifiedDocumentHandle` and `VerifiedFolderHandle` values; verification never returns an untyped relative path.
- Produces: `ResolvedWorkspace { id, display_name, canonical_path, root: Arc<cap_std::fs::Dir> }`; the canonical path is native-only.
- Consumes: `AuthorizedWorkspaceConfig`, signing key, and config revision from Task 2.

- [ ] **Step 1: Write the traversal, symlink, nesting, and tamper tests first**

Cover this table with explicit test cases:

```rust
let rejected_names = [
    "../secret.md", "/tmp/secret.md", "C:\\secret.md", "C:secret.md",
    "\\\\server\\share\\secret.md", "a\\..\\secret.md", "a/./secret.md",
    "a//secret.md", "a\0secret.md", "%2e%2e/secret.md", "%252e%252e/secret.md",
];
```

Tests must also create a symlinked root, symlinked parent, symlinked file, nested authorized root, HMAC-bit-flipped handle, folder-as-document handle, and handle copied to another `workspaceId`. On Windows, add a reparse-point equivalent behind `#[cfg(windows)]`.

- [ ] **Step 2: Run the boundary tests and verify failure**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tests::workspace_boundary -- --nocapture`

Expected: FAIL because no registry/handle validator exists.

- [ ] **Step 3: Implement the registry and handles with no ambient child-path joins**

Use a random UUID for every authorization and recreate it after removal/re-add. Reject roots that are symlinks, files, inaccessible, nested, duplicates, or below QingYu app config/data/credential directories. Open accepted roots once with `cap_std::fs::Dir::open_ambient_dir` and retain that capability.

Use this signed payload:

```rust
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct HandlePayload {
    version: u8,
    kind: HandleKind,
    workspace_id: uuid::Uuid,
    relative_path: String,
}
```

Serialize as `base64url_no_pad(payload_json).base64url_no_pad(hmac_sha256(payload_json))`. Validate HMAC with `Mac::verify_slice`, then revalidate workspace authorization, handle type, normalized relative segments, protected paths, and nofollow metadata. Decode a handle exactly once; percent signs remain ordinary filename characters and never trigger a second decode.

- [ ] **Step 4: Run all boundary tests**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tests:: -- --nocapture`

Expected: PASS on the host platform; platform-gated cases compile on their targets.

- [ ] **Step 5: Commit the workspace security boundary**

```bash
git add apps/desktop/src-tauri/src/mcp
git commit -m "feat: guard MCP workspaces with signed handles"
```

---

### Task 4: Enforce confirmations, cryptographic dry runs, safe errors, and redacted audit

**Files:**
- Create: `apps/desktop/src-tauri/src/mcp/policy.rs`
- Create: `apps/desktop/src-tauri/src/mcp/confirmation.rs`
- Create: `apps/desktop/src-tauri/src/mcp/audit.rs`
- Modify: `apps/desktop/src-tauri/src/mcp/error.rs`
- Modify: `apps/desktop/src-tauri/src/mcp/tests.rs`

**Interfaces:**
- Produces: `PolicyEngine::authorize`, `preview`, `consume_preview`, `confirm_if_required`.
- Produces: `OperationDescriptor { tool, workspace_id, target, expected_revision, risk, canonical_arguments }`.
- Produces: `AuditSink::record(AuditEvent)` and paginated sanitized reads.
- Consumes: global config generation, signing key, and `McpPermissions`.

- [ ] **Step 1: Write policy matrix and redaction tests**

Assert all 3×3 confirmation/dry-run modes for ordinary writes and high-risk writes. Assert a preview token fails after changing one argument, revision, permission generation, workspace generation, expiry, or after reuse. Serialize audit entries and verify none of these sentinels appear: `/Users/example/private`, `secret document body`, `Bearer abc`, `S3SECRET`.

- [ ] **Step 2: Run the focused policy tests and observe failure**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tests::policy -- --nocapture`

Expected: FAIL because policy and audit services do not exist.

- [ ] **Step 3: Implement the policy and audit contracts**

Define risks exactly as:

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OperationRisk {
    ReadOnly,
    Write,
    HighRisk,
    Destructive,
}
```

Preview tokens contain an HMAC-authenticated payload with tool, SHA-256 of canonical arguments, expected revision, policy generation, workspace generation, issued-at, expiry-at, and a random nonce. Set expiry to 5 minutes, store consumed nonces in a bounded in-memory cache, and invalidate the cache on disable/token rotation/policy/root changes.

Implement `ConfirmationPresenter` as a trait. Before presenting, the production presenter shows and focuses an existing QingYu editor/settings window or creates the normal restore-capable editor window, then uses `tauri_plugin_dialog::DialogExt::message`, custom Allow/Cancel buttons, a oneshot channel, and `tokio::time::timeout(Duration::from_secs(120), receiver)`. Tests use a deterministic fake presenter.

Audit entries contain only request ID, timestamp, tool, workspace ID/display name, logical target, dry-run/confirmation result, outcome/error code, revisions, sync run ID, duration, and counts. Persist JSON Lines in app-private data, rotate at the configured entry/age limits, and fail mutating operations closed if enabled audit persistence fails.

- [ ] **Step 4: Run policy/audit tests and inspect serialized fixtures**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tests:: -- --nocapture`

Expected: PASS; no sentinel secret/path/content appears.

- [ ] **Step 5: Commit policy enforcement**

```bash
git add apps/desktop/src-tauri/src/mcp
git commit -m "feat: enforce MCP operation policy and audit"
```

---

### Task 5: Build the shared guarded document read/list/search service

**Files:**
- Create: `apps/desktop/src-tauri/src/markdown_files/service.rs`
- Modify: `apps/desktop/src-tauri/src/markdown_files.rs`
- Modify: `apps/desktop/src-tauri/src/markdown_files/document.rs`
- Modify: `apps/desktop/src-tauri/src/markdown_files/tree.rs`
- Modify: `apps/desktop/src-tauri/src/markdown_files/search.rs`
- Modify: `apps/desktop/src-tauri/src/markdown_files/ignore_rules.rs`
- Modify: `apps/desktop/src-tauri/src/mcp/workspaces.rs`
- Modify: `apps/desktop/src-tauri/src/mcp/tests.rs`

**Interfaces:**
- Produces: `DocumentService::list`, `search`, and `read` with `DocumentScope::Authorized` and `DocumentScope::TrustedUi`.
- Produces: `DocumentRevision`, `DocumentEntry`, `DocumentPage`, `DocumentSearchPage`, `DocumentSnapshot`.
- Consumes: `ResolvedWorkspace` and verified handles from Task 3.

- [ ] **Step 1: Write service tests for visibility, pagination, content limits, and revisions**

Build a temporary tree containing Markdown files, `.qingyu`, `.git`, `.markra-sync`, `node_modules`, a non-Markdown file, and symlinks. Assert list/search/read expose only Markdown files, use opaque IDs, cap pages at 100, return stable cursors, reject content over the configured limit, and change revisions when exact bytes change.

- [ ] **Step 2: Run the document read tests and verify failure**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tests::document_read -- --nocapture`

Expected: FAIL because the shared service is absent.

- [ ] **Step 3: Implement focused document types and capability traversal**

Use these service signatures:

```rust
pub(crate) struct DocumentService;

impl DocumentService {
    pub(crate) fn list(
        &self,
        scope: &DocumentScope,
        parent: Option<&VerifiedFolderHandle>,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<DocumentPage, DocumentServiceError>;

    pub(crate) fn read(
        &self,
        scope: &DocumentScope,
        document: &VerifiedDocumentHandle,
        max_bytes: u64,
    ) -> Result<DocumentSnapshot, DocumentServiceError>;

    pub(crate) fn search(
        &self,
        scope: &DocumentScope,
        query: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<DocumentSearchPage, DocumentServiceError>;
}
```

Compute revision as SHA-256 over exact bytes plus length. Use nofollow directory traversal from the retained `cap_std::fs::Dir`. Share `MarkdownIgnoreRules` and add `.qingyu` plus every sync/history/temp/recycle control name. List/search must never first collect protected entries and filter them afterward.

Keep Tauri UI compatibility by adapting existing list/search/read commands to call `DocumentService` with `TrustedUi` scope; preserve their serialized response shapes and asset-scope behavior.

- [ ] **Step 4: Run focused and existing file tests**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tests::document_read -- --nocapture`

Expected: PASS.

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml markdown_files:: -- --nocapture`

Expected: PASS; existing UI command tests remain green.

- [ ] **Step 5: Commit shared read behavior**

```bash
git add apps/desktop/src-tauri/src/markdown_files.rs apps/desktop/src-tauri/src/markdown_files apps/desktop/src-tauri/src/mcp
git commit -m "feat: share guarded document read services"
```

---

### Task 6: Add revision-safe document mutations, deletion modes, history, and sync triggers

**Files:**
- Modify: `apps/desktop/src-tauri/src/markdown_files/service.rs`
- Modify: `apps/desktop/src-tauri/src/markdown_files/document.rs`
- Modify: `apps/desktop/src-tauri/src/markdown_files/tree.rs`
- Modify: `apps/desktop/src-tauri/src/markdown_files/history.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync.rs`
- Modify: `apps/desktop/src-tauri/src/mcp/tests.rs`

**Interfaces:**
- Produces: `DocumentService::create`, `update`, `move_document`, `delete`.
- Produces: `DocumentMutation { document_id, relative_path, revision, sync_request }`.
- Consumes: policy decision from Task 4 and document scope from Task 5.

- [ ] **Step 1: Write mutation tests before changing production code**

Cover create existing target, stale update, stale move, move existing target, stale delete, atomic replacement, history snapshot, handle replacement after move, symlink race hook, system trash, QingYu recycle copy/flush/verify/remove, permanent deletion, and all three sync-after-write policies.

- [ ] **Step 2: Run the mutation tests and verify failure**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tests::document_mutation -- --nocapture`

Expected: FAIL because mutation methods are absent.

- [ ] **Step 3: Implement no-overwrite, expected-revision mutations**

Use these inputs:

```rust
pub(crate) struct CreateDocument<'a> {
    pub(crate) parent: &'a VerifiedFolderHandle,
    pub(crate) name: &'a str,
    pub(crate) contents: &'a str,
}

pub(crate) struct UpdateDocument<'a> {
    pub(crate) document: &'a VerifiedDocumentHandle,
    pub(crate) contents: &'a str,
    pub(crate) expected_revision: &'a str,
}

pub(crate) struct MoveDocument<'a> {
    pub(crate) document: &'a VerifiedDocumentHandle,
    pub(crate) target_parent: &'a VerifiedFolderHandle,
    pub(crate) new_name: &'a str,
    pub(crate) expected_revision: &'a str,
}
```

Validate one child filename ending in `.md`/`.markdown`; reject separators and control names. Revalidate parent/final metadata immediately before mutation. Create and move use platform no-replace primitives. Update writes a sibling temporary file, flushes, snapshots existing history, atomically replaces, and returns a new revision. Move returns a newly signed document ID.

System trash uses `trash::delete` only after capability and nofollow revalidation. QingYu recycle writes to app-private storage with `{ workspaceId, relativePath, deletedAt, revision }`, verifies the copied bytes, then removes the source. Permanent delete unlinks only the verified final file. Existing Tauri write/tree commands call the shared mutation primitives and retain their public shapes.

After success, emit normal watcher/history events and request sync through the existing coordinator according to `follow-workspace`, `always`, or `never`. The document result is successful even if a queued background sync later fails.

- [ ] **Step 4: Run mutation, history, watcher, and sync-coalescing tests**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tests::document_mutation -- --nocapture`

Expected: PASS.

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml markdown_files::history -- --nocapture`

Expected: PASS.

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml watcher:: -- --nocapture`

Expected: PASS.

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync::coordinator -- --nocapture`

Expected: PASS with no implicit overwrite and one coalesced sync request.

- [ ] **Step 5: Commit document mutation support**

```bash
git add apps/desktop/src-tauri/src/markdown_files apps/desktop/src-tauri/src/remote_sync.rs apps/desktop/src-tauri/src/mcp
git commit -m "feat: add safe MCP document mutations"
```

---

### Task 7: Add a typed native application-settings service and migrate exposed groups

**Files:**
- Create: `apps/desktop/src-tauri/src/app_settings.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `packages/app/src/runtime/index.ts`
- Modify: `apps/desktop/src/runtime/index.ts`
- Create: `apps/desktop/src/runtime/tauri/settings.ts`
- Modify: `packages/app/src/lib/settings/app-settings.ts`
- Modify: focused tests under `packages/app/src/lib/settings/*.test.ts`

**Interfaces:**
- Produces: native `AppSettingsService::read_exposed`, `patch_exposed`, `read_group`, `write_group`.
- Produces: Tauri commands `read_app_settings_group`, `write_app_settings_group`, `read_exposed_app_settings`, `patch_exposed_app_settings`.
- Consumes: existing `settings.json` through `tauri_plugin_store::StoreExt`.

- [ ] **Step 1: Write Rust registry tests and TypeScript runtime tests**

The registry must expose only these exact field names:

```text
appearance.mode
appearance.lightTheme
appearance.darkTheme
language
editor.bodyFontSize
editor.contentWidth
editor.contentWidthPx
editor.fontFamily
editor.lineHeight
editor.paragraphSpacingPx
editor.showWordCount
editor.wrapCodeBlocks
editor.viewMode
files.ignoreRules
export.pdfAuthor
export.pdfFooter
export.pdfHeader
export.pdfHeightMm
export.pdfMarginMm
export.pdfMarginPreset
export.pdfPageBreakOnH1
export.pdfPageSize
export.pdfWidthMm
```

Assert rejection of MCP/security keys, workspace/recent/window state, custom CSS, Pandoc executable/arguments, template/shortcut/image-upload structures, updater state, and unknown fields.

In TypeScript, assert the appearance, language, editor, file-ignore, and export getters/savers call the new native group runtime when available while browser tests continue using the memory store.

- [ ] **Step 2: Run Rust and TypeScript focused tests and observe failure**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml app_settings:: -- --nocapture`

Expected: FAIL because `app_settings` does not exist.

Run: `pnpm test -- packages/app/src/lib/settings/app-settings.test.ts apps/desktop/src/runtime/index.test.ts`

Expected: FAIL because group methods are not part of `AppSettingsRuntime`.

- [ ] **Step 3: Implement native group persistence and field-level patches**

Define closed groups:

```rust
#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum AppSettingsGroup {
    Appearance,
    Language,
    EditorPreferences,
    FileIgnoreSettings,
    ExportSettings,
}
```

Map them to the existing keys `appearanceMode/lightTheme/darkTheme`, `language`, `editorPreferences`, `fileIgnoreSettings`, and `exportSettings`. Validate complete patches before touching the store; compute revision from canonical exposed JSON; reject stale `expectedRevision`; update requested fields in memory; save once; restore old values if save fails; emit the existing settings-change events after persistence.

Extend `AppSettingsRuntime` with `readGroup` and `writeGroup`. Desktop calls the typed Tauri commands. The browser runtime maps the same group enum onto its in-memory store. Migrate only the five exposed groups: appearance, language, editor, file-ignore, export. Leave backup, workspace, recent files, custom CSS, templates, and other excluded data on their current paths.

- [ ] **Step 4: Run settings tests and type checking**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml app_settings:: -- --nocapture`

Expected: PASS.

Run: `pnpm test -- packages/app/src/lib/settings apps/desktop/src/runtime/index.test.ts`

Expected: PASS.

Run: `pnpm typecheck:test`

Expected: PASS without TypeScript `void` usage.

- [ ] **Step 5: Commit the shared settings service**

```bash
git add apps/desktop/src-tauri/src/app_settings.rs apps/desktop/src-tauri/src/lib.rs apps/desktop/src/runtime packages/app/src/runtime packages/app/src/lib/settings
git commit -m "feat: share typed application settings service"
```

---

### Task 8: Add sanitized sync configuration and background-run services

**Files:**
- Modify: `apps/desktop/src-tauri/src/project_config/model.rs`
- Modify: `apps/desktop/src-tauri/src/project_config/storage.rs`
- Modify: `apps/desktop/src-tauri/src/project_config.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/coordinator.rs`
- Create: `apps/desktop/src-tauri/src/remote_sync/service.rs`
- Create: `apps/desktop/src-tauri/src/mcp/tools/sync.rs`
- Modify: `apps/desktop/src-tauri/src/mcp/tests.rs`

**Interfaces:**
- Produces: `patch_project_config_batch_at_root` and one-write revision semantics.
- Produces: shared `SyncService::get_config`, `update_config`, `update_credentials`, `test`, `run`, `status`.
- Produces: `SyncRunRegistry` with generated `runId` and queued/running/succeeded/failed/cancelled states.
- Consumes: authorized workspace root and existing sync engine/coordinator.

- [ ] **Step 1: Write sanitized-config, credential, batch, and run-state tests**

Assert readable configuration contains `credentialsConfigured` booleans but none of `password` or `secretAccessKey`. Assert omitted secrets remain unchanged, empty secret strings fail, `clearCredentials: true` clears, and a two-field batch either writes both fields in one revision or writes neither. Assert background mode returns before completion and `sync_status(runId)` reaches the final state; duplicate workspace/revision runs coalesce.

- [ ] **Step 2: Run focused sync tests and verify failure**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tests::sync_tools -- --nocapture`

Expected: FAIL because batch patches and MCP run registry are absent.

- [ ] **Step 3: Implement the sanitized facade over existing config/sync code**

Use distinct input types:

```rust
#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct SyncConfigPatchInput {
    pub(crate) workspace_id: uuid::Uuid,
    pub(crate) expected_revision: String,
    pub(crate) enabled: Option<bool>,
    pub(crate) provider: Option<ProjectSyncProvider>,
    pub(crate) remote_path: Option<String>,
    pub(crate) auto_sync_on_save: Option<bool>,
    pub(crate) interval_minutes: Option<u32>,
    pub(crate) webdav_server_url: Option<String>,
    pub(crate) s3_endpoint_url: Option<String>,
    pub(crate) s3_region: Option<String>,
    pub(crate) s3_bucket: Option<String>,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct SyncCredentialPatchInput {
    pub(crate) workspace_id: uuid::Uuid,
    pub(crate) expected_revision: String,
    pub(crate) webdav_username: Option<String>,
    pub(crate) webdav_password: Option<String>,
    pub(crate) s3_access_key_id: Option<String>,
    pub(crate) s3_secret_access_key: Option<String>,
    pub(crate) clear_credentials: Option<bool>,
}
```

Convert validated fields into `Vec<ProjectConfigPatch>`, apply to one cloned config, validate readiness/issues, and persist once using the original expected revision. Wrap existing `ready_snapshot`, `test_connection`, `coalesced_project_sync`, and status persistence instead of copying protocol/provider logic.

Define `SyncService` in `remote_sync/service.rs`; both the existing Tauri command facade and MCP tool adapter call it. Generate a UUID run ID before spawning. Store only sanitized status/results. In wait mode use the same spawned run and wait up to the configured tool timeout; timeout returns the run ID without cancelling.

- [ ] **Step 4: Run project-config, S3/WebDAV, and MCP sync tests**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml project_config:: -- --nocapture`

Expected: PASS.

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync:: -- --nocapture`

Expected: PASS.

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tests::sync_tools -- --nocapture`

Expected: PASS with existing provider behavior unchanged.

- [ ] **Step 5: Commit sync service integration**

```bash
git add apps/desktop/src-tauri/src/project_config.rs apps/desktop/src-tauri/src/project_config apps/desktop/src-tauri/src/remote_sync.rs apps/desktop/src-tauri/src/remote_sync apps/desktop/src-tauri/src/mcp
git commit -m "feat: expose sanitized MCP sync services"
```

---

### Task 9: Implement the dynamic tool catalog and closed-schema dispatch

**Files:**
- Modify: `apps/desktop/src-tauri/src/mcp/tools/mod.rs`
- Create: `apps/desktop/src-tauri/src/mcp/tools/workspace.rs`
- Create: `apps/desktop/src-tauri/src/mcp/tools/document.rs`
- Create: `apps/desktop/src-tauri/src/mcp/tools/settings.rs`
- Modify: `apps/desktop/src-tauri/src/mcp/tools/sync.rs`
- Modify: `apps/desktop/src-tauri/src/mcp/mod.rs`
- Modify: `apps/desktop/src-tauri/src/mcp/tests.rs`

**Interfaces:**
- Produces all 16 approved tool names and MCP annotations.
- Produces: `McpServices { config, workspaces, documents, settings, sync, policy, audit }` held by `QingYuMcpHandler::new` as cloneable `Arc` services.
- Consumes document/settings/sync services, `PolicyEngine`, `AuditSink`, and current global config.

- [ ] **Step 1: Write tool-list, permission-recheck, schema, and result tests**

Assert this exact catalog under full permissions:

```text
workspace_list
document_list
document_search
document_read
document_create
document_update
document_move
document_delete
settings_get
settings_update
sync_config_get
sync_config_update
sync_credentials_update
sync_test
sync_run
sync_status
```

The complete count is 16. `workspace_list` returns a signed `rootFolderId` for each available workspace so root-level listing and creation never require a path. Assert each permission removes its associated tools, a cached tool call fails after permission revocation, unknown input fields produce a tool execution error, and errors contain structured `{ code, message, retryable, recoveryHint }` without protocol-level failure except unknown tool/malformed envelope.

- [ ] **Step 2: Run tool router tests and verify failure**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tests::tool_router -- --nocapture`

Expected: FAIL because the catalog and dispatch are incomplete.

- [ ] **Step 3: Implement manual dynamic `ServerHandler` methods**

Implement `list_tools` by reading current config on every call and constructing only permitted `rmcp::model::Tool` values. Derive input/output schemas with `rmcp::schemars`; all input structs use `deny_unknown_fields`. Set annotations exactly:

```rust
#[derive(Clone)]
pub(crate) struct McpServices {
    pub(crate) config: std::sync::Arc<McpConfigManager>,
    pub(crate) workspaces: std::sync::Arc<WorkspaceRegistry>,
    pub(crate) documents: std::sync::Arc<DocumentService>,
    pub(crate) settings: std::sync::Arc<AppSettingsService>,
    pub(crate) sync: std::sync::Arc<SyncService>,
    pub(crate) policy: std::sync::Arc<PolicyEngine>,
    pub(crate) audit: std::sync::Arc<AuditSink>,
}
```

- read/list/search/status/get/test: `readOnlyHint=true`, `destructiveHint=false`, `idempotentHint=true`;
- create: read-only false, destructive false, idempotent false;
- update/move/settings/config/credentials: read-only false, destructive false, idempotent true only with the same revision/preview token;
- delete and sync run: read-only false, destructive true, idempotent false;
- only sync tools that contact a provider set `openWorldHint=true`; all other tools set false.

Every mutating input contains `dryRun: Option<bool>` and `previewToken: Option<String>`. Route through permission recheck → handle/root validation → revision check → required preview → app confirmation → operation → audit. Return `CallToolResult` with concise text plus `structured_content`. Map user-correctable failures to `Ok(CallToolResult::error(...))`. Implement the design's stable codes: `mcp_disabled`, `permission_denied`, `workspace_not_authorized`, `workspace_unavailable`, `invalid_handle`, `document_not_found`, `revision_conflict`, `target_already_exists`, `path_boundary_violation`, `protected_path`, `document_too_large`, `settings_field_not_exposed`, `confirmation_rejected`, `confirmation_timeout`, `preview_required`, `preview_expired`, `sync_not_configured`, `sync_in_progress`, `credential_write_denied`, `rate_limited`, and `response_too_large`.

- [ ] **Step 4: Run all router/service tests**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tests:: -- --nocapture`

Expected: PASS; `tools/list` changes immediately after in-memory config changes.

- [ ] **Step 5: Commit the tool surface**

```bash
git add apps/desktop/src-tauri/src/mcp
git commit -m "feat: expose QingYu MCP tools"
```

---

### Task 10: Embed the authenticated loopback Streamable HTTP server

**Files:**
- Create: `apps/desktop/src-tauri/src/mcp/server.rs`
- Modify: `apps/desktop/src-tauri/src/mcp/mod.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/mcp/tests.rs`

**Interfaces:**
- Produces: `McpServerController::start`, `stop`, `restart`, `health`.
- Consumes: `QingYuMcpHandler`, keyring token, global config, `rmcp::StreamableHttpService`.

- [ ] **Step 1: Write HTTP auth, limits, lifecycle, and session tests**

Start on an ephemeral test port while production defaults remain `19618`. Test missing/wrong/correct/rotated/revoked tokens, session ID without auth, invalid Host, invalid Origin returning 403, request-size 413, burst 429, concurrency limit, occupied port, disabled service, and cancellation of active sessions/previews on stop. Configure a short test idle duration and assert expiry; production uses 30 minutes.

- [ ] **Step 2: Run HTTP tests and verify failure**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tests::http_server -- --nocapture`

Expected: FAIL because no listener/controller exists.

- [ ] **Step 3: Implement server lifecycle and middleware**

Build `LocalSessionManager` with:

```rust
SessionConfig {
    keep_alive: Some(Duration::from_secs(30 * 60)),
    ..SessionConfig::default()
}
```

Build `StreamableHttpServerConfig` with stateful mode, cancellation token, allowed hosts `127.0.0.1:{port}` and `localhost:{port}`, and allowed origins `tauri://localhost`, `http://tauri.localhost`, and `https://tauri.localhost`. Mount exactly `/mcp` on an Axum router bound to `SocketAddr::from(([127, 0, 0, 1], port))`.

Place custom middleware before the MCP service to:

1. load the in-memory token digest;
2. require `Authorization: Bearer ...` on every method/request;
3. compare fixed digests with `subtle::ConstantTimeEq`;
4. enforce an 8 MiB default body limit;
5. enforce the configured token bucket and `ConcurrencyLimitLayer`;
6. reject any non-`/mcp` path;
7. avoid all CORS wildcard headers.

The tool-result builder serializes structured output before returning and converts results over 8 MiB to `response_too_large`. Start from Tauri `setup` only when persisted config is enabled and the keyring token exists. A bind failure updates visible health without changing to another port. Stop cancels the `CancellationToken` and waits for the listener task.

- [ ] **Step 4: Run HTTP tests and native regression tests**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tests::http_server -- --nocapture`

Expected: PASS.

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml`

Expected: PASS.

- [ ] **Step 5: Commit the embedded server**

```bash
git add apps/desktop/src-tauri/src/mcp apps/desktop/src-tauri/src/lib.rs
git commit -m "feat: embed authenticated QingYu MCP server"
```

---

### Task 11: Add Tauri MCP control commands and the settings page

**Files:**
- Modify: `apps/desktop/src-tauri/src/mcp/mod.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Create: `apps/desktop/src/runtime/tauri/mcp.ts`
- Modify: `apps/desktop/src/runtime/index.ts`
- Modify: `packages/app/src/runtime/index.ts`
- Create: `packages/app/src/lib/mcp.ts`
- Create: `packages/app/src/hooks/useMcpSettings.ts`
- Create: `packages/app/src/components/settings/McpSettings.tsx`
- Create: `packages/app/src/components/settings/McpSettings.test.tsx`
- Modify: `packages/app/src/components/SettingsShell.tsx`
- Modify: `packages/app/src/components/SettingsWindow.tsx`
- Modify: `packages/app/src/hooks/useSettingsWindowState.ts`

**Interfaces:**
- Produces `AppMcpRuntime` methods for config, roots, token, health, and audit.
- Consumes `McpState` and revisioned config from Tasks 2/10.

- [ ] **Step 1: Write runtime mapping and settings component tests**

Assert the desktop runtime invokes these exact commands:

```text
get_mcp_settings
update_mcp_settings
authorize_mcp_workspace
remove_mcp_workspace
copy_mcp_token
rotate_mcp_token
revoke_mcp_token
get_mcp_health
list_mcp_audit_entries
clear_mcp_audit_entries
```

Render the page and assert: disabled default, endpoint `127.0.0.1:19618`, add/remove roots, every global permission, confirmation/dry-run/deletion/sync/audit controls, request/response/document/rate/concurrency/timeout limits, copy/rotate/revoke token actions, revision conflict reload, health error, and absence of any “open folder through MCP” action.

- [ ] **Step 2: Run focused frontend tests and verify failure**

Run: `pnpm test -- packages/app/src/components/settings/McpSettings.test.tsx apps/desktop/src/runtime/index.test.ts`

Expected: FAIL because the runtime/category/page do not exist.

- [ ] **Step 3: Implement commands, runtime types, and revision-aware UI**

Use one `McpSettingsSnapshot` containing config revision, UI-visible authorized absolute paths, safe endpoint, `tokenConfigured`, and health. Only these Tauri commands may return absolute authorized paths; MCP tools must use `list_safe` and never serialize them.

`update_mcp_settings` accepts a closed typed patch and `expectedRevision`, persists, then starts/stops/restarts the controller as required. Permission/root/policy changes increment the generation, invalidate previews, and broadcast `notifications/tools/list_changed`; prune peers that reject notification. Token rotate/revoke terminates sessions.

The add button calls the existing native folder picker, then `authorizeMcpWorkspace`; selecting a directory does not open it in an editor window. All destructive UI actions use local confirmation. The browser/default runtime reports MCP unavailable and renders no category outside desktop.

- [ ] **Step 4: Run UI, runtime, and type tests**

Run: `pnpm test -- packages/app/src/components/settings/McpSettings.test.tsx packages/app/src/components/SettingsShell.test.tsx apps/desktop/src/runtime/index.test.ts`

Expected: PASS.

Run: `pnpm typecheck:test`

Expected: PASS.

- [ ] **Step 5: Commit the MCP settings surface**

```bash
git add apps/desktop/src-tauri/src/lib.rs apps/desktop/src-tauri/src/mcp apps/desktop/src/runtime packages/app/src
git commit -m "feat: add QingYu MCP settings controls"
```

---

### Task 12: Add localized copy and audit inspection

**Files:**
- Modify: every `packages/shared/src/i18n/locales/*.ts`
- Modify: `packages/shared/src/i18n/index.test.ts`
- Modify: `packages/app/src/components/settings/McpSettings.tsx`
- Modify: `packages/app/src/components/settings/McpSettings.test.tsx`

**Interfaces:**
- Produces all `settings.mcp.*` copy and a paginated, redacted audit table.
- Consumes audit runtime from Task 11.

- [ ] **Step 1: Add failing locale parity and redaction UI tests**

Add required-key assertions for category, summary, enable/port/endpoint, token actions, authorized directories, ten permission labels, five policy groups, health states, audit headings, empty state, clear action, revision conflict, and save/start errors. Render an audit entry and assert the component never accepts body/path/token/credential fields in its TypeScript type.

- [ ] **Step 2: Run locale/UI tests and verify failure**

Run: `pnpm test -- packages/shared/src/i18n/index.test.ts packages/app/src/components/settings/McpSettings.test.tsx`

Expected: FAIL with missing MCP keys.

- [ ] **Step 3: Add exact translations and the bounded audit table**

Write natural Simplified/Traditional Chinese and English copy. For German, Spanish, French, Italian, Japanese, Korean, Portuguese-Brazil, and Russian, use reviewed translations; if a reviewer cannot validate a translation, use the exact English value rather than an ambiguous machine-generated security instruction. Keep the terms `MCP`, `QingYu`, `Bearer Token`, `WebDAV`, and `S3` unchanged.

Render only the typed audit fields: time, tool, workspace display name, logical target, outcome/error code, revisions, run ID, and duration. Page at 100 entries. The clear action exists only here and requires local confirmation.

- [ ] **Step 4: Run all locale and settings tests**

Run: `pnpm test -- packages/shared/src/i18n packages/app/src/components/settings`

Expected: PASS with equal key coverage across all locales.

- [ ] **Step 5: Commit copy and audit UI**

```bash
git add packages/shared/src/i18n packages/app/src/components/settings
git commit -m "feat: localize MCP settings and audit view"
```

---

### Task 13: Build the forwarding-only `qingyu-mcp` stdio bridge and package it

**Files:**
- Create: `apps/desktop/src-tauri/src/mcp/bridge.rs`
- Create: `apps/desktop/src-tauri/src/bin/qingyu-mcp.rs`
- Modify: `apps/desktop/src-tauri/src/mcp/mod.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Create: `packages/scripts/src/prepare-qingyu-mcp-sidecar.mjs`
- Modify: `package.json`
- Modify: `apps/desktop/src-tauri/tauri.conf.json`
- Modify: `.gitignore`

**Interfaces:**
- Produces: `run_bridge(BridgeConfig, AppLauncher)` and the `qingyu-mcp` binary.
- Consumes: upstream Streamable HTTP MCP endpoint and token only.

- [ ] **Step 1: Write bridge forwarding, notification, launch, and no-replay tests**

Start the real in-process HTTP test server, run bridge logic over Tokio duplex stdio, initialize through it, list tools, call `workspace_list`, and observe `tools/list_changed`. Use a fake launcher to assert one app launch followed by bounded reconnect. Simulate disconnect after sending a mutating call and assert the bridge reports indeterminate outcome without replay.

- [ ] **Step 2: Run bridge tests and verify failure**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tests::bridge -- --nocapture`

Expected: FAIL because bridge/proxy code is absent.

- [ ] **Step 3: Implement protocol forwarding without business services**

Read `QINGYU_MCP_URL` with default `http://127.0.0.1:19618/mcp` and require `QINGYU_MCP_TOKEN`. Connect an `rmcp` Streamable HTTP client with the Authorization header. Expose a stdio `ServerHandler` whose `list_tools` and `call_tool` forward to the upstream peer. Forward tool-list notifications from the upstream `ClientHandler` to the downstream peer. Write logs only to stderr. Export one narrow `markra_lib::run_mcp_bridge_from_env()` entry point from `lib.rs`; `src/bin/qingyu-mcp.rs` contains only the Tokio main function, calls that entry point, prints a safe error to stderr, and exits non-zero on failure.

On initial connection failure, launch:

- macOS: `open -a QingYu`;
- Windows: sibling installed `QingYu.exe`;
- Linux: sibling installed `qingyu` executable.

Wait at most 15 seconds using 100 ms exponential backoff capped at 1 second. Do not enable MCP, rotate a token, change the port, read files, or replay a request after an unknown transport outcome.

- [ ] **Step 4: Add reproducible sidecar preparation and bundle metadata**

The Node script resolves the Rust target triple with `rustc -vV`, runs:

```text
cargo build --manifest-path apps/desktop/src-tauri/Cargo.toml --bin qingyu-mcp --release
```

Then copies the executable to `apps/desktop/src-tauri/binaries/qingyu-mcp-$TARGET_TRIPLE` (with `.exe` on Windows). Add the generated `binaries/qingyu-mcp-*` files to `.gitignore`, set Tauri `bundle.externalBin` to `binaries/qingyu-mcp`, and make one root `prepare:desktop-build` script run the existing frontend build followed by sidecar preparation. Point `beforeBuildCommand` at that one script.

- [ ] **Step 5: Verify bridge tests and sidecar build**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tests::bridge -- --nocapture`

Expected: PASS.

Run: `cargo build --manifest-path apps/desktop/src-tauri/Cargo.toml --bin qingyu-mcp`

Expected: PASS and produce only the forwarding binary.

Run: `pnpm prepare:qingyu-mcp-sidecar`

Expected: PASS and create one ignored target-suffixed sidecar.

- [ ] **Step 6: Commit bridge and packaging sources**

```bash
git add .gitignore package.json apps/desktop/src-tauri/Cargo.toml apps/desktop/src-tauri/Cargo.lock apps/desktop/src-tauri/tauri.conf.json apps/desktop/src-tauri/src/bin apps/desktop/src-tauri/src/lib.rs apps/desktop/src-tauri/src/mcp/bridge.rs packages/scripts/src/prepare-qingyu-mcp-sidecar.mjs
git commit -m "feat: add QingYu MCP stdio bridge"
```

---

### Task 14: Add end-to-end conformance, adversarial security, and secret-leak coverage

**Files:**
- Modify: `apps/desktop/src-tauri/src/mcp/tests.rs`

**Interfaces:**
- Produces: automated HTTP and stdio acceptance evidence.
- Consumes: complete service from Tasks 1–13.

- [ ] **Step 1: Write an end-to-end acceptance fixture**

The fixture must:

1. create two temporary workspaces and authorize only one;
2. initialize with `rmcp` over Streamable HTTP;
3. verify the permission-filtered tool list;
4. list/read/create/dry-run/update/move/delete one document;
5. force a revision conflict;
6. test sanitized settings and sync reads;
7. rotate token and prove the old session cannot continue;
8. repeat representative list/read/write calls through `qingyu-mcp` stdio;
9. remove the root and prove old IDs fail immediately.

- [ ] **Step 2: Add the complete adversarial table**

Include raw/encoded/double-encoded traversal, slash/backslash mixtures, POSIX/Windows/UNC/drive-relative absolute forms, NUL, invalid/normalized Unicode, symlink parents/finals, replacement races, protected dirs, cross-workspace handles, type-confused handles, oversized documents, oversized request/results, missing/wrong auth, bad Host/Origin, rate/concurrency limits, stale previews, rejected/timed-out confirmation, credential omission/clear, audit failure, and bridge disconnect/no-replay.

- [ ] **Step 3: Run the new E2E tests and fix only defects within this design**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tests::end_to_end -- --nocapture`

Expected: PASS. For any failure, add the smallest focused regression test beside the owning module before changing implementation.

- [ ] **Step 4: Scan artifacts and logs for secret/path sentinels**

Run the tests with known sentinel tokens, credentials, absolute paths, and bodies, then scan only generated test audit/log output:

```bash
rg -n "SENTINEL_BEARER|SENTINEL_S3_SECRET|SENTINEL_DOCUMENT_BODY|SENTINEL_ABSOLUTE_ROOT" apps/desktop/src-tauri/target/mcp-test-artifacts
```

Expected: no matches. The test harness must create `target/mcp-test-artifacts` itself and remove stale contents before writing new evidence.

- [ ] **Step 5: Commit acceptance coverage**

```bash
git add apps/desktop/src-tauri/src/mcp/tests.rs
git commit -m "test: cover QingYu MCP end to end"
```

---

### Task 15: Document client setup and run final verification

**Files:**
- Create: `docs/qingyu-mcp.md`
- Modify: `README.md`
- Modify: `README.zh-CN.md`

**Interfaces:**
- Produces: user-facing setup examples and final verification record.
- Consumes: final endpoint, token, and bridge behavior.

- [ ] **Step 1: Write the guide with exact HTTP and stdio examples**

Include:

```json
{
  "mcpServers": {
    "qingyu": {
      "type": "streamable-http",
      "url": "http://127.0.0.1:19618/mcp",
      "headers": {
        "Authorization": "Bearer <token copied from QingYu>"
      }
    }
  }
}
```

```json
{
  "mcpServers": {
    "qingyu": {
      "command": "/absolute/path/to/qingyu-mcp",
      "env": {
        "QINGYU_MCP_URL": "http://127.0.0.1:19618/mcp",
        "QINGYU_MCP_TOKEN": "<token copied from QingYu>"
      }
    }
  }
}
```

Explain disabled default, global permissions, directory authorization, token rotation/revocation, no open-folder tool, opaque IDs, revision conflicts, write confirmation/dry-run/deletion choices, write-only sync credentials, background run IDs, and audit redaction. State that anyone holding the global token shares the same permissions.

- [ ] **Step 2: Run formatting and diff hygiene checks**

Run: `cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check`

Expected: PASS.

Run: `git diff --check`

Expected: PASS.

- [ ] **Step 3: Run the full repository verification gate**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml`

Expected: PASS.

Run: `pnpm test`

Expected: PASS.

Run: `pnpm typecheck:test`

Expected: PASS.

Run: `pnpm build`

Expected: PASS.

Run: `pnpm test:s3-sync:live`

Expected: PASS when the configured real MinIO/S3 test server is available; otherwise record the explicit environment/configuration reason it was skipped.

- [ ] **Step 4: Build the bridge and desktop package**

Run: `cargo build --manifest-path apps/desktop/src-tauri/Cargo.toml --bin qingyu-mcp --release`

Expected: PASS.

Run: `pnpm tauri build --debug`

Expected: PASS when platform signing/packaging prerequisites are available, and the artifact contains the target-suffixed `qingyu-mcp` sidecar. If packaging prerequisites are unavailable, run `pnpm prepare:qingyu-mcp-sidecar` and `pnpm build` and record the exact packaging-only blocker.

- [ ] **Step 5: Inspect final scope and commit documentation**

Run: `git status --short`

Expected: only intentional MCP/doc changes; the primary checkout's `bg.png` remains untracked and untouched.

Run: `rg -n "workspace_open|workspace_close|0\.0\.0\.0|absolutePath|perClient" apps/desktop/src-tauri/src/mcp packages/app/src/components/settings/McpSettings.tsx docs/qingyu-mcp.md`

Expected: no tool/schema/listener/per-client implementation; explanatory negations in documentation are acceptable after manual review.

```bash
git add docs/qingyu-mcp.md
git add README.md
git add README.zh-CN.md
git commit -m "docs: explain QingYu MCP setup"
```
