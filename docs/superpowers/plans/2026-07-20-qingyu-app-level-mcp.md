# QingYu Application-Level MCP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Store MCP policy in portable application settings and bind all document capability to the single primary notes workspace rather than the focused directory.

**Architecture:** `settings.json` becomes the only persisted source for MCP policy; app-data `mcp-runtime/` owns IPC and audit state. The native manager receives primary-workspace changes from application state, never from window focus. Frontend settings read and write the same app-level policy on desktop and mobile, while document tools fail closed when no valid primary workspace exists.

**Tech Stack:** Rust, Tauri v2, serde/serde_json, Tauri Store, Unix socket/Windows named pipe IPC, React 19, TypeScript, Vitest.

## Global Constraints

- The complete portable MCP policy is stored under `mcp` in `settings.json`.
- MCP policy includes enabled state, permissions, confirmation, dry-run/deletion policy, limits, and audit policy.
- No MCP policy or runtime file is written into a notes or external folder.
- IPC transport, process-only identifiers, socket/pipe state, and audit entries live below app-data `mcp-runtime/` and never synchronize.
- External window focus never activates, clears, or replaces the MCP workspace.
- The only document workspace capability is the valid primary notes workspace.
- Without a valid primary workspace, document tools fail closed even when MCP is enabled.
- Switching the primary workspace invalidates prior workspace/document handles.
- Settings and sync tools keep their dedicated permissions.
- Preserve the local-only no-token IPC model; do not add TCP, bearer tokens, or Keychain dependencies.
- Do not read, migrate, rewrite, or delete `.qingyu/mcp.json`.
- Do not add dependencies.

---

## File Structure

- `apps/desktop/src-tauri/src/app_settings.rs`: validates and revision-writes portable MCP policy under the `mcp` key.
- `apps/desktop/src-tauri/src/mcp/config.rs`: owns MCP defaults/normalization only; it no longer owns filesystem persistence.
- `apps/desktop/src-tauri/src/mcp/bridge.rs`: installs one application policy and one primary-workspace capability.
- `apps/desktop/src-tauri/src/mcp/workspaces.rs`: manages handles tied to a generation of the primary workspace.
- `packages/app/src/lib/mcp.ts`: mirrors portable MCP policy and snapshots.
- `packages/app/src/components/settings/McpSettings.tsx`: edits app policy even when no document workspace exists, while clearly disabling document capability.
- `packages/app/src/App.tsx`: notifies MCP only when the primary workspace changes.

### Task 1: Persist MCP Policy Through `settings.json`

**Files:**
- Modify: `apps/desktop/src-tauri/src/app_settings.rs`
- Modify: `apps/desktop/src-tauri/src/mcp/config.rs`
- Modify: `apps/desktop/src-tauri/src/mcp/tests.rs`
- Modify: `packages/app/src/runtime/index.ts`
- Modify: `packages/app/src/lib/mcp.ts`
- Modify: `packages/app/src/lib/settings/app-settings.ts`
- Modify: `packages/app/src/lib/settings/app-settings.test.ts`
- Modify: `packages/app/src/lib/settings/settings-events.ts`
- Modify: `packages/app/src/lib/settings/settings-events.test.ts`

**Interfaces:**
- Consumes: portable `settings.json` store and settings synchronization from the application-sync plan.
- Produces: app-settings group `mcp` and event `qingyu://settings-mcp-changed`.
- Produces: `load_mcp_policy(app_data_root) -> AppSettingsGroupSnapshot<McpConfig>` and revisioned `write_mcp_policy`.
- Removes filesystem persistence APIs from `mcp/config.rs`.

- [ ] **Step 1: Write failing persistence and portability tests**

```rust
#[test]
fn mcp_policy_round_trips_through_settings_json_only() {
    let app_data = tempdir().unwrap();
    let notes = tempdir().unwrap();
    let updated = sample_mcp_config_enabled();
    write_mcp_settings_group(app_data.path(), None, &updated).unwrap();

    assert_eq!(read_mcp_settings_group(app_data.path()).unwrap().value, updated);
    assert!(!notes.path().join(".qingyu/mcp.json").exists());
    assert!(!app_data.path().join("mcp.json").exists());
}

#[test]
fn portable_settings_validation_accepts_mcp_policy_but_no_runtime_state() {
    let value = portable_settings_with_mcp(sample_mcp_config_enabled());
    assert!(validate_portable_settings(&value).is_ok());
    assert!(value.pointer("/mcp/processKey").is_none());
    assert!(value.pointer("/mcp/socketPath").is_none());
}
```

- [ ] **Step 2: Run MCP and app-settings tests to verify red state**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::`

Expected: FAIL because MCP still loads `.qingyu/mcp.json` from an activated project.

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml app_settings::`

Expected: FAIL because portable app settings do not include an MCP group.

- [ ] **Step 3: Make app settings the only policy repository**

```rust
pub(crate) const MCP_SETTINGS_GROUP: &str = "mcp";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct McpSettingsGroup {
    #[serde(flatten)]
    pub config: McpConfig,
}

pub(crate) fn read_mcp_settings_group(
    app_data_root: &Path,
) -> Result<AppSettingsGroupSnapshot<McpConfig>, AppSettingsError> {
    read_validated_group(app_data_root, MCP_SETTINGS_GROUP, McpConfig::default, normalize_mcp_config)
}
```

Reuse the existing `settings.json` revision/write lock and atomic store path rather than introducing an MCP-specific writer. Include `mcp` in the portable settings schema and settings synchronization allowlist. `update_mcp_settings` checks the settings-group revision, writes the group, reloads the manager policy, and emits `qingyu://settings-mcp-changed`. Remove root parameters and `.qingyu/mcp.json` load/save/recovery logic from `mcp/config.rs`.

- [ ] **Step 4: Run Rust and frontend portability tests**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::`

Expected: PASS for defaults, normalization, revision conflict, and absence of project files.

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml app_settings::`

Expected: PASS for MCP group read/write and settings change events.

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync::settings_scope`

Expected: PASS for settings download reload with MCP policy.

Run: `pnpm --filter @markra/app exec vitest run src/lib/settings/app-settings.test.ts src/lib/settings/settings-events.test.ts --environment jsdom --globals`

Expected: PASS with `mcp` included in portable settings and no runtime material in serialized output.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src-tauri/src/app_settings.rs apps/desktop/src-tauri/src/mcp/config.rs apps/desktop/src-tauri/src/mcp/tests.rs packages/app/src/runtime/index.ts packages/app/src/lib/mcp.ts packages/app/src/lib/settings/app-settings.ts packages/app/src/lib/settings/app-settings.test.ts packages/app/src/lib/settings/settings-events.ts packages/app/src/lib/settings/settings-events.test.ts
git commit -m "refactor: persist MCP policy in app settings"
```

### Task 2: Bind Native MCP Authority to the Primary Workspace

**Files:**
- Modify: `apps/desktop/src-tauri/src/mcp/bridge.rs`
- Modify: `apps/desktop/src-tauri/src/mcp/workspaces.rs`
- Modify: `apps/desktop/src-tauri/src/mcp/handles.rs`
- Modify: `apps/desktop/src-tauri/src/mcp/server.rs`
- Modify: `apps/desktop/src-tauri/src/mcp/tools/document.rs`
- Modify: `apps/desktop/src-tauri/src/mcp/tools/workspace.rs`
- Modify: `apps/desktop/src-tauri/src/mcp/tests.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src/runtime/tauri/mcp.ts`
- Modify: `apps/desktop/src/runtime/index.test.ts`

**Interfaces:**
- Consumes: primary workspace path from `local-state.json` and MCP policy from Task 1.
- Produces: Tauri command `set_mcp_primary_workspace({ primaryRoot: string | null })`.
- Produces: runtime `mcp.setPrimaryWorkspace(input: { primaryRoot: string | null }): Promise<McpSettingsSnapshot>`.
- Guarantees: every document/workspace handle carries a workspace generation and becomes invalid after a root change.

- [ ] **Step 1: Write failing authority lifecycle tests**

```rust
#[tokio::test]
async fn external_focus_cannot_change_mcp_workspace() {
    let fixture = mcp_fixture_with_primary("/Notes").await;
    fixture.focus_external_window("/External").await;
    assert_eq!(fixture.snapshot().workspace.unwrap().root_path, "/Notes");
}

#[tokio::test]
async fn changing_primary_workspace_invalidates_old_handles() {
    let fixture = mcp_fixture_with_primary("/Notes-A").await;
    let handle = fixture.open_document_handle("a.md").await;
    fixture.set_primary_workspace(Some("/Notes-B")).await;
    let error = fixture.read_handle(handle).await.unwrap_err();
    assert_eq!(error.code, "mcp-handle-stale");
}

#[tokio::test]
async fn enabled_mcp_without_primary_workspace_fails_document_tools_closed() {
    let fixture = mcp_fixture_enabled_without_primary().await;
    let error = fixture.list_workspace().await.unwrap_err();
    assert_eq!(error.code, "mcp-workspace-unavailable");
    assert!(fixture.settings_read().await.is_ok());
}
```

- [ ] **Step 2: Run MCP authority tests to verify red state**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tests`

Expected: FAIL because `activate_project` still reloads project config and focus changes authority.

- [ ] **Step 3: Replace project activation with a generation-bound primary capability**

```rust
pub(crate) struct PrimaryWorkspaceCapability {
    pub canonical_root: PathBuf,
    pub display_name: String,
    pub generation: u64,
}

impl McpBridge {
    pub(crate) async fn set_primary_workspace(
        &self,
        primary_root: Option<PathBuf>,
    ) -> Result<McpSettingsSnapshot, McpError> {
        let capability = match primary_root {
            Some(path) => Some(self.workspaces.canonicalize_primary(path)?),
            None => None,
        };
        self.workspaces.replace_primary(capability).await;
        self.handles.invalidate_all().await;
        Ok(self.snapshot().await)
    }
}
```

Start MCP service according to the application policy independently from workspace availability. Document tools call `require_primary_workspace()` and compare handle generation before filesystem access. Settings/sync tools do not call that guard. Allocate socket/pipe, identifiers, and audit paths below `app_data/mcp-runtime`; preserve the current local-only no-token transport behavior.

- [ ] **Step 4: Run native MCP and runtime adapter tests**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::`

Expected: PASS for primary install/remove/switch, stale handles, containment, external focus independence, policy reload, transport, and permission behavior.

Run: `pnpm --filter @markra/desktop exec vitest run src/runtime/index.test.ts --environment jsdom --globals`

Expected: PASS with `setPrimaryWorkspace` and no `activateProject` runtime method.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src-tauri/src/mcp/bridge.rs apps/desktop/src-tauri/src/mcp/workspaces.rs apps/desktop/src-tauri/src/mcp/handles.rs apps/desktop/src-tauri/src/mcp/server.rs apps/desktop/src-tauri/src/mcp/tools/document.rs apps/desktop/src-tauri/src/mcp/tools/workspace.rs apps/desktop/src-tauri/src/mcp/tests.rs apps/desktop/src-tauri/src/lib.rs apps/desktop/src/runtime/tauri/mcp.ts apps/desktop/src/runtime/index.test.ts
git commit -m "refactor: bind MCP to the primary workspace"
```

### Task 3: Make MCP Settings Application-Scoped on Every Surface

**Files:**
- Modify: `packages/app/src/App.tsx`
- Modify: `packages/app/src/App.test.tsx`
- Modify: `packages/app/src/components/settings/McpSettings.tsx`
- Modify: `packages/app/src/components/settings/McpSettings.test.tsx`
- Modify: `packages/app/src/hooks/useMcpSettings.ts`
- Modify: `packages/app/src/hooks/useMcpSettings.test.tsx`
- Modify: `packages/app/src/components/compact/CompactSettingsDetail.tsx`
- Modify: `packages/app/src/components/compact/CompactSettingsDetail.test.tsx`
- Modify: `packages/shared/src/i18n/locales/en.ts`
- Modify: `packages/shared/src/i18n/locales/zh-CN.ts`

**Interfaces:**
- Consumes: `primaryWorkspace.root` and `mcp.setPrimaryWorkspace` from Task 2.
- Produces: one application-level MCP settings UI shared by desktop and Compact settings.
- Guarantees: focus events contain no MCP authority side effect.

- [ ] **Step 1: Write failing frontend authority and settings tests**

```tsx
it("updates MCP only when the primary workspace changes", async () => {
  seedPrimaryWorkspace("/Notes-A", true);
  render(<App />);
  await waitFor(() => expect(mockSetPrimaryWorkspace).toHaveBeenLastCalledWith({ primaryRoot: "/Notes-A" }));
  const callCount = mockSetPrimaryWorkspace.mock.calls.length;
  window.dispatchEvent(new Event("focus"));
  expect(mockSetPrimaryWorkspace).toHaveBeenCalledTimes(callCount);
  switchPrimaryWorkspace("/Notes-B");
  await waitFor(() => expect(mockSetPrimaryWorkspace).toHaveBeenLastCalledWith({ primaryRoot: "/Notes-B" }));
});

it("allows policy edits without a workspace but labels document tools unavailable", async () => {
  render(<McpSettings runtime={runtimeWithoutWorkspace()} />);
  expect(screen.getByText(/尚未选择主笔记目录/u)).toBeVisible();
  await user.click(screen.getByRole("button", { name: /启用 MCP/u }));
  expect(mockUpdateSettings).toHaveBeenCalled();
  expect(screen.getByText(/文档工具不可用/u)).toBeVisible();
});
```

- [ ] **Step 2: Run frontend tests and verify focus/project assumptions fail**

Run: `pnpm --filter @markra/app exec vitest run src/App.test.tsx src/components/settings/McpSettings.test.tsx src/hooks/useMcpSettings.test.tsx src/components/compact/CompactSettingsDetail.test.tsx --environment jsdom --globals`

Expected: FAIL because App installs a focus listener and MCP settings disables every policy edit when no project is active.

- [ ] **Step 3: Install primary-only authority and independent policy editing**

```tsx
useEffect(() => {
  getAppRuntime().mcp.setPrimaryWorkspace({
    primaryRoot: primaryWorkspace.status === "ready" ? primaryWorkspace.root : null
  }).catch(() => {});
}, [primaryWorkspace.root, primaryWorkspace.status]);
```

Delete the window-focus MCP effect. In `McpSettings`, derive `documentWorkspaceAvailable` only for document-capability messaging and document-specific controls; do not use it to block service policy, permissions, limits, audit policy, or enabled state. Display the canonical primary workspace from the snapshot and explain that external windows do not alter it. Desktop and Compact settings use the same `AppMcpRuntime`.

- [ ] **Step 4: Run frontend and cross-subsystem verification**

Run: `pnpm --filter @markra/app exec vitest run src/App.test.tsx src/components/settings/McpSettings.test.tsx src/hooks/useMcpSettings.test.tsx src/components/compact/CompactSettingsDetail.test.tsx --environment jsdom --globals`

Expected: PASS.

Run: `pnpm test`

Expected: PASS.

Run: `pnpm typecheck:test`

Expected: PASS.

Run: `pnpm build`

Expected: PASS.

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml`

Expected: PASS, including default-parallel Rust execution.

- [ ] **Step 5: Commit**

```bash
git add packages/app/src/App.tsx packages/app/src/App.test.tsx packages/app/src/components/settings/McpSettings.tsx packages/app/src/components/settings/McpSettings.test.tsx packages/app/src/hooks/useMcpSettings.ts packages/app/src/hooks/useMcpSettings.test.tsx packages/app/src/components/compact/CompactSettingsDetail.tsx packages/app/src/components/compact/CompactSettingsDetail.test.tsx packages/shared/src/i18n/locales/en.ts packages/shared/src/i18n/locales/zh-CN.ts
git commit -m "feat: make MCP settings application scoped"
```

### Task 4: Remove Obsolete Project MCP Paths and Verify the Real App

**Files:**
- Delete: project-MCP persistence fixtures under `apps/desktop/src-tauri/src/mcp/` that exist only for `.qingyu/mcp.json`.
- Modify: `apps/desktop/src-tauri/src/builder_boundary_tests.rs`
- Modify: `apps/desktop/src-tauri/src/mobile_platform_config_tests.rs`
- Modify: `packages/app/src/test/app-harness.tsx`
- Modify: `docs/superpowers/specs/2026-07-20-qingyu-primary-notes-workspace-design.md` only if implementation names differ while behavior remains identical.

**Interfaces:**
- Consumes all earlier tasks.
- Produces no compatibility layer; final tree has no production reference to project activation or project-owned MCP config.

- [ ] **Step 1: Add boundary assertions before deleting residue**

```rust
#[test]
fn production_sources_do_not_register_project_scoped_mcp_commands() {
    let source = runtime_source();
    for forbidden in ["activate_mcp_project", ".qingyu/mcp.json", "load_project_mcp_config"] {
        assert!(!source.contains(forbidden), "obsolete MCP boundary remains: {forbidden}");
    }
    assert!(source.contains("set_mcp_primary_workspace"));
}
```

- [ ] **Step 2: Run the boundary tests and record each remaining obsolete reference**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml builder_boundary_tests`

Expected: FAIL with exact remaining project-scoped MCP symbols in desktop registration.

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mobile_platform_config_tests`

Expected: FAIL with exact remaining project-scoped MCP symbols in mobile boundaries.

- [ ] **Step 3: Remove only obsolete persistence/activation code and fixtures**

Delete the paths identified by the boundary test when their only responsibility is `.qingyu/mcp.json` persistence or focus-driven project activation. Preserve policy normalization, permissions, confirmations, handles, tools, IPC, audit behavior, and existing security tests. Update app harness defaults to `setPrimaryWorkspace` and app-level settings snapshots.

- [ ] **Step 4: Run repository gates**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml`

Expected: PASS with default parallelism.

Run: `pnpm test`

Expected: PASS.

Run: `pnpm typecheck:test`

Expected: PASS.

Run: `pnpm build`

Expected: PASS.

- [ ] **Step 5: Exercise the real desktop application**

Run: `pnpm tauri dev`

Expected manual evidence:

1. A fresh app-data fixture shows the approved desktop welcome surface.
2. Selecting primary folder A installs A in the main window and MCP snapshot.
3. Opening external folder B creates another editor window; focusing B leaves MCP on A.
4. Switching primary folder to C through Settings invalidates an A document handle and installs C.
5. With no primary path, application MCP policy remains editable while document tools fail closed.
6. No `.qingyu/mcp.json` is created in A, B, or C.

- [ ] **Step 6: Commit**

```bash
git add -A apps/desktop/src-tauri/src/mcp apps/desktop/src-tauri/src/builder_boundary_tests.rs apps/desktop/src-tauri/src/mobile_platform_config_tests.rs packages/app/src/test/app-harness.tsx docs/superpowers/specs/2026-07-20-qingyu-primary-notes-workspace-design.md
git commit -m "test: verify application-level MCP boundaries"
```
