# QingYu Project-Local MCP Without Keychain Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace QingYu's Keychain-backed loopback MCP endpoint and persistent workspace whitelist with secret-free local IPC, project-local policy, and a hard current-project runtime boundary.

**Architecture:** `qingyu-mcp` continues to expose MCP over stdio but forwards to QingYu through an owner-only Unix socket or current-user Windows named pipe. The focused editor window activates exactly one canonical project capability; QingYu loads `<project>/.qingyu/mcp.json`, while an in-memory process key makes every handle and preview expire at restart without requiring persistent secrets.

**Tech Stack:** Rust, Tauri v2, rmcp async read/write transport, Tokio Unix sockets/Windows named pipes, cap-std, React, TypeScript, Vitest, pnpm.

## Global Constraints

- Use `pnpm` for JavaScript and frontend workflows.
- The only MCP document root is the folder currently active in QingYu.
- MCP permissions are one project policy shared by all clients, never per-client ACLs.
- `.qingyu/mcp.json` contains no path, port, token, socket path, signing key, or workspace identifier.
- The default MCP path uses no TCP listener and no operating-system credential store.
- Old workspace, folder, document, cursor, and preview identifiers may expire after project switch or QingYu restart.
- Keep package identifiers unchanged; outward naming remains QingYu/轻语.
- Preserve existing no-follow path checks, protected paths, revisions, confirmations, audit, history, trash, and sync behavior.

---

### Task 1: Make MCP configuration project-local

**Files:**
- Modify: `apps/desktop/src-tauri/src/mcp/config.rs`
- Modify: `apps/desktop/src-tauri/src/mcp/workspaces.rs`
- Modify: `apps/desktop/src-tauri/src/mcp/tests.rs`

**Interfaces:**
- Produces: `McpConfigStore::for_project(project_root: &Path)` targeting `.qingyu/mcp.json`.
- Produces: `McpConfigManager::inactive()` and `activate_project(project_root: &Path)`.
- Produces: `WorkspaceRegistry::activate_current(path: &Path) -> AuthorizedWorkspaceConfig` and `clear_current()`.
- Removes: serialized `port` and `workspaces` fields from `McpConfig`.

- [ ] **Step 1: Write failing project-storage and current-root tests**

Add tests that load disabled defaults without a project, activate two temporary roots, persist different settings in each `.qingyu/mcp.json`, and prove no configuration contains either absolute root. Add symlink tests for `.qingyu` and `mcp.json`. Add a workspace test that issues a document handle in project A, switches to project B, and observes `workspace_not_authorized` for A.

- [ ] **Step 2: Run the focused tests and verify RED**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tests::project_local_config -- --nocapture
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tests::current_workspace -- --nocapture
```

Expected: FAIL because the current store is application-global and the registry retains multiple authorized roots.

- [ ] **Step 3: Implement the project-local store and activation state**

Use these public contracts:

```rust
impl McpConfigStore {
    pub(crate) fn for_project(project_root: &Path) -> Result<Self, McpConfigError>;
}

impl McpConfigManager {
    pub(crate) fn inactive() -> Result<Self, McpConfigError>;
    pub(crate) fn activate_project(
        &self,
        project_root: &Path,
    ) -> Result<McpConfigDocument, McpConfigError>;
    pub(crate) fn clear_project(&self) -> Result<McpConfigDocument, McpConfigError>;
}

impl WorkspaceRegistry {
    pub(crate) fn activate_current(
        &self,
        path: &Path,
    ) -> Result<AuthorizedWorkspaceConfig, WorkspaceError>;
    pub(crate) fn clear_current(&self) -> Result<(), WorkspaceError>;
}
```

The store must open the canonical project root, create/open `.qingyu` without following symlinks, stage `mcp.json` as a sibling, sync it, and atomically replace it. `McpConfig` serialization must omit path/port/workspace fields by removing them from the model rather than silently ignoring them.

- [ ] **Step 4: Run the focused tests and verify GREEN**

Run the two commands from Step 2. Expected: PASS.

- [ ] **Step 5: Commit project-local configuration**

```bash
git add apps/desktop/src-tauri/src/mcp/config.rs apps/desktop/src-tauri/src/mcp/workspaces.rs apps/desktop/src-tauri/src/mcp/tests.rs
git commit -m "refactor: scope MCP policy to the active project"
```

---

### Task 2: Remove Keychain-backed MCP secrets

**Files:**
- Delete: `apps/desktop/src-tauri/src/mcp/secrets.rs`
- Modify: `apps/desktop/src-tauri/src/mcp/mod.rs`
- Modify: `apps/desktop/src-tauri/src/mcp/tests.rs`
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/src/builder_boundary_tests.rs`

**Interfaces:**
- Produces: `new_process_key() -> Result<[u8; 32], String>`.
- Removes: `KeyringSecretStore`, `McpSecretStore`, token Tauri commands, and the `keyring` dependency.
- Preserves: existing HMAC handle, cursor, and preview validation using only the process key.

- [ ] **Step 1: Write a failing initialization-boundary test**

Assert the desktop source and dependency manifest contain none of `KeyringSecretStore`, `ensure_signing_key`, `copy_mcp_token`, `rotate_mcp_token`, `revoke_mcp_token`, or `keyring`, and assert two generated process keys produce handles that cannot be verified across instances.

- [ ] **Step 2: Run the test and verify RED**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tests::process_scoped_identifiers -- --nocapture
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml builder_boundary -- --nocapture
```

Expected: FAIL because startup calls Keychain and token commands/dependency still exist.

- [ ] **Step 3: Replace persistent secrets with one process key**

Generate the key exactly once during `mcp::initialize`:

```rust
fn new_process_key() -> Result<[u8; 32], String> {
    let mut key = [0_u8; 32];
    getrandom::fill(&mut key)
        .map_err(|_| "mcp-session-key-unavailable: QingYu could not initialize MCP.".to_string())?;
    Ok(key)
}
```

Construct `HandleSigner` from that key and derive the preview/cursor keys as today. Remove every credential-store read and token command. Delete `secrets.rs`, remove `keyring`, and update desktop-only dependency boundary assertions.

- [ ] **Step 4: Run the focused tests and verify GREEN**

Run the commands from Step 2. Expected: PASS and no Keychain-linked crate in the desktop dependency tree.

- [ ] **Step 5: Commit the secret removal**

```bash
git add apps/desktop/src-tauri apps/desktop/src-tauri/Cargo.toml pnpm-lock.yaml
git commit -m "refactor: make MCP identifiers process scoped"
```

---

### Task 3: Replace loopback HTTP with private local IPC

**Files:**
- Create: `apps/desktop/src-tauri/src/mcp/ipc.rs`
- Rewrite: `apps/desktop/src-tauri/src/mcp/server.rs`
- Rewrite: `apps/desktop/src-tauri/src/mcp/bridge.rs`
- Modify: `apps/desktop/src-tauri/src/mcp/mod.rs`
- Modify: `apps/desktop/src-tauri/src/mcp/tools/mod.rs`
- Modify: `apps/desktop/src-tauri/src/mcp/tests.rs`
- Modify: `apps/desktop/src-tauri/Cargo.toml`

**Interfaces:**
- Produces: `LocalIpcEndpoint::default_for_qingyu()` and `display_name()`.
- Produces: `limited_transport::<Role, _>(stream, max_line_bytes)` using rmcp `JsonRpcMessageCodec`.
- Changes: `McpServerController::start(options)` and `restart(options)` no longer accept a token.
- Changes: `BridgeConfig` contains a local endpoint and retry timings, not URL/token.

- [ ] **Step 1: Write failing no-token bridge and private-listener tests**

Add tests that remove `QINGYU_MCP_TOKEN`/`QINGYU_MCP_URL`, build `BridgeConfig::from_env`, start a controller on a temporary local endpoint, list tools through the bridge, stop the controller, and prove the endpoint disappears. Add a stale-socket test and an oversized JSON-RPC line test. Keep the existing one-launch bounded-backoff and indeterminate-mutation behavior tests.

- [ ] **Step 2: Run the transport tests and verify RED**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tests::local_ipc -- --nocapture
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tests::bridge_ -- --nocapture
```

Expected: FAIL because the bridge requires token/URL variables and the server binds TCP.

- [ ] **Step 3: Implement bounded stream transport and platform endpoints**

Enable rmcp `transport-async-rw`; remove its streamable-HTTP features. Build role-specific read/write halves with:

```rust
JsonRpcMessageCodec::<RxJsonRpcMessage<Role>>::new_with_max_length(max_line_bytes)
JsonRpcMessageCodec::<TxJsonRpcMessage<Role>>::new_with_max_length(max_line_bytes)
```

On Unix bind `<data-dir>/dev.markra.app/qingyu-mcp.sock`, reject non-socket occupants, remove only verified stale sockets, and set mode `0600`. On Windows use `\\.\pipe\qingyu-mcp-dev.markra.app` with a current-user security descriptor. Accept each connection into `QingYuMcpHandler::serve` and cancel every connection when stopping or changing project.

Move request-rate and concurrent-call gating into `QingYuMcpHandler::call_tool_current` so it applies to every IPC transport. Keep maximum request framing at the server connection.

- [ ] **Step 4: Run the focused tests and verify GREEN**

Run the commands from Step 2. Expected: PASS, with no TCP socket or Authorization header involved.

- [ ] **Step 5: Commit private IPC**

```bash
git add apps/desktop/src-tauri/src/mcp apps/desktop/src-tauri/Cargo.toml Cargo.lock
git commit -m "refactor: connect QingYu MCP over private local IPC"
```

---

### Task 4: Bind the focused QingYu project to MCP

**Files:**
- Modify: `packages/app/src/runtime/index.ts`
- Modify: `apps/desktop/src/runtime/tauri/mcp.ts`
- Modify: `apps/desktop/src/runtime/desktop.ts`
- Modify: `packages/app/src/App.tsx`
- Modify: `packages/app/src/App.test.tsx`
- Modify: `apps/desktop/src-tauri/src/mcp/mod.rs`
- Modify: `apps/desktop/src-tauri/src/desktop_runtime.rs`
- Modify: `apps/desktop/src-tauri/src/builder_boundary_tests.rs`

**Interfaces:**
- Produces: `AppMcpRuntime.activateProject({ projectRoot: string | null })`.
- Produces: Tauri command `activate_mcp_project(project_root: Option<String>)`.
- Removes: authorize/remove workspace and token runtime methods/commands.

- [ ] **Step 1: Write failing frontend activation tests**

Render `App` with the desktop runtime, change `fileTree.projectRoot` from project A to B to `null`, dispatch a window `focus` event, and assert `activateProject` receives the current root on each boundary change/focus. Assert a stale root is never resent after a newer root becomes active.

- [ ] **Step 2: Run the frontend test and verify RED**

Run:

```bash
pnpm --filter @markra/app test -- App.test.tsx
```

Expected: FAIL because `AppMcpRuntime.activateProject` does not exist.

- [ ] **Step 3: Implement activation wiring**

Add the runtime contract:

```ts
activateProject: (input: { projectRoot: string | null }) => Promise<McpSettingsSnapshot>;
```

In `App.tsx`, report the current canonical project candidate whenever `fileTree.projectRoot` changes and from a stable `focus` listener. The native command activates/clears the registry and config, invalidates handles/previews, updates audit policy, emits `tools/list_changed`, and restarts/stops the IPC listener.

- [ ] **Step 4: Run frontend and Rust boundary tests**

Run:

```bash
pnpm --filter @markra/app test -- App.test.tsx
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml builder_boundary -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit active-project wiring**

```bash
git add packages/app/src/runtime/index.ts packages/app/src/App.tsx packages/app/src/App.test.tsx apps/desktop/src/runtime apps/desktop/src-tauri/src
git commit -m "feat: bind MCP to the focused QingYu project"
```

---

### Task 5: Simplify settings and documentation

**Files:**
- Modify: `packages/app/src/lib/mcp.ts`
- Modify: `packages/app/src/hooks/useMcpSettings.ts`
- Modify: `packages/app/src/components/settings/McpSettings.tsx`
- Modify: `packages/app/src/components/settings/McpSettings.test.tsx`
- Modify: `packages/app/src/components/SettingsWindow.tsx`
- Modify: `packages/shared/src/i18n/locales/*.ts`
- Modify: `packages/shared/src/i18n/index.test.ts`
- Rewrite: `docs/qingyu-mcp.md`
- Modify: `docs/privacy.md`

**Interfaces:**
- `McpConfig` removes `port` and `workspaces`.
- `McpSettingsSnapshot` removes `tokenConfigured`, exposes current safe workspace metadata, and identifies transport as local IPC.
- The MCP settings page accepts the current `projectRoot` context and has no token, port, or directory-authorizer actions.

- [ ] **Step 1: Write failing settings-surface tests**

Assert the page shows the current project and project-local policy, does not render Bearer Token copy/rotate/revoke controls, does not render port input, and does not offer add/remove authorized directory actions. Assert the default TypeScript config has no `port` or `workspaces` keys.

- [ ] **Step 2: Run UI and i18n tests and verify RED**

Run:

```bash
pnpm --filter @markra/app test -- McpSettings.test.tsx
pnpm --filter @markra/shared test -- i18n/index.test.ts
```

Expected: FAIL on the obsolete controls and copy.

- [ ] **Step 3: Implement the project-local settings surface**

Remove obsolete runtime actions and UI sections. Keep permissions, confirmation, dry-run, deletion, sync, request/response/rate/concurrency/timeout limits, and audit controls. Update every locale's visible summary and enablement description so it describes a private same-user channel and current-project scope without claiming token authentication.

Rewrite `docs/qingyu-mcp.md` with secret-free Codex configuration, reconnect behavior, `.qingyu/mcp.json`, and the hard current-project boundary. Update privacy documentation to state that MCP policy is project-local and contains no MCP credential.

- [ ] **Step 4: Run UI, i18n, and static scans**

Run:

```bash
pnpm --filter @markra/app test -- McpSettings.test.tsx
pnpm --filter @markra/shared test -- i18n/index.test.ts
rg -n "Bearer|QINGYU_MCP_TOKEN|127\.0\.0\.1:19618|copy_mcp_token|authorize_mcp_workspace" apps packages docs --glob '!docs/superpowers/**'
```

Expected: tests PASS; scan returns no active MCP implementation or current-user documentation references.

- [ ] **Step 5: Commit UI and documentation**

```bash
git add packages/app packages/shared apps/desktop/src/runtime docs
git commit -m "docs: describe project-local secret-free MCP"
```

---

### Task 6: Verify live Codex integration and merge

**Files:**
- Modify outside repo only for test: `~/.codex/config.toml`
- Remove obsolete test-only app config after verification: `~/Library/Application Support/dev.markra.app/mcp.json`

**Interfaces:**
- Codex launches `qingyu-mcp` with no token or URL environment variables.
- A temporary QingYu project enables MCP through `.qingyu/mcp.json` and is the only visible workspace.

- [ ] **Step 1: Run complete repository verification**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
pnpm test
pnpm typecheck:test
pnpm build
pnpm tauri build --debug
```

Expected: all checks PASS; live S3 cases remain ignored only when `MARKRA_TEST_S3_*` is unavailable.

- [ ] **Step 2: Run source and dependency verification**

Run:

```bash
rg -n "KeyringSecretStore|ensure_signing_key|QINGYU_MCP_TOKEN|Authorization.*Bearer|127\.0\.0\.1:19618" apps packages docs --glob '!docs/superpowers/**'
cargo tree --manifest-path apps/desktop/src-tauri/Cargo.toml -i keyring
lsof -nP -iTCP:19618 -sTCP:LISTEN
```

Expected: no active-code/docs matches, no `keyring` dependency, and no MCP TCP listener.

- [ ] **Step 3: Configure and exercise current Codex**

Point the existing global `qingyu` MCP entry at the newly built `qingyu-mcp` binary and remove token/URL environment values. Launch the debug QingYu app on an isolated project whose `.qingyu/mcp.json` enables read/write test permissions. Through Codex call `workspace_list`, `document_list`, `document_read`, `document_create`, `document_update`, and the configured deletion flow. Switch QingYu to a second project and prove the first project's identifiers fail while only the second project is listed.

- [ ] **Step 4: Verify restart recovery and Keychain absence**

Capture a document identifier, restart QingYu, confirm that identifier is rejected, then relist and complete a read with the new identifier. Monitor macOS processes/logs during two launches and confirm no SecurityAgent prompt or QingYu Keychain access occurs.

- [ ] **Step 5: Review and integrate**

Review `git diff --check`, the complete diff, and all verification evidence. Merge `codex/qingyu-mcp-no-keychain` into local `main` without disturbing the primary checkout's pre-existing README/package/script changes. Do not push unless explicitly requested.
