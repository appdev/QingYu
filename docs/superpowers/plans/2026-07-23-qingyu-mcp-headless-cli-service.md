# QingYu MCP Headless CLI Service Implementation Plan

> **Required subskill:** Use `superpowers:executing-plans` to implement this plan task by task. Use `superpowers:test-driven-development` for every behavior change and `superpowers:verification-before-completion` before reporting success.

**Goal:** Make the bundled MCP CLI work without a `PATH` installation, prevent disabled MCP configurations from launching QingYu, and start an enabled MCP runtime as a windowless background service.

**Architecture:** Keep `qingyu-mcp` as a thin stdio-to-local-IPC bridge. On a failed IPC connection it reads only the application-local `mcp.enabled` value, then either returns a stable error or launches the sibling QingYu executable directly with `mcp serve`. The desktop executable recognizes that launch mode before Tauri creates windows and initializes MCP authority from persisted application state without waiting for React.

**Tech Stack:** Rust, Tauri v2, serde_json, local Unix-domain socket / Windows named pipe, Vitest, pnpm.

---

## Task 1: Gate bridge startup with the persisted MCP enabled state

**Files:**

- Modify: `apps/desktop/src-tauri/src/mcp/ipc.rs`
- Modify: `apps/desktop/src-tauri/src/mcp/bridge.rs`
- Modify: `apps/desktop/src-tauri/src/mcp/tests.rs`

### Step 1: Add failing settings-preflight tests

Add focused bridge tests using a temporary application-data directory and a counting `AppLauncher`:

```rust
#[test]
fn bridge_reports_disabled_without_launch_when_settings_are_missing() {
    // No settings.json, unreachable endpoint.
    // Expect BridgeError::McpDisabled and launch_count == 0.
}

#[test]
fn bridge_reports_disabled_without_launch_when_mcp_is_false() {
    // { "mcp": { "enabled": false } }
}

#[test]
fn bridge_reports_config_unavailable_without_launch_for_malformed_settings() {
    // Invalid JSON.
}

#[test]
fn bridge_launches_once_when_mcp_is_enabled_and_endpoint_is_unavailable() {
    // { "mcp": { "enabled": true } }, short retry policy.
}
```

Also assert the user-visible strings contain the stable codes `mcp_disabled` and `mcp_config_unavailable` plus an actionable Settings instruction.

### Step 2: Run the new tests and confirm they fail

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tests::bridge_reports_ -- --nocapture
```

Expected: failures because the bridge currently launches immediately after an IPC miss and has no settings preflight errors.

### Step 3: Expose the authoritative application-data path

In `mcp/ipc.rs`, centralize the data directory so the bridge and endpoint resolution cannot drift:

```rust
pub(crate) const APP_IDENTIFIER: &str = "dev.markra.app";

pub(crate) fn application_data_dir() -> Result<PathBuf, LocalIpcError> {
    dirs::data_dir()
        .map(|path| path.join(APP_IDENTIFIER))
        .ok_or(LocalIpcError::RuntimeDirectoryUnavailable)
}
```

Build the socket/runtime path from this helper.

### Step 4: Implement a fail-closed launch preflight

Extend `BridgeConfig` with a `settings_path`. Production configuration uses `<application_data_dir>/settings.json`; tests inject a temporary path.

Add private parsing with exactly three outcomes:

```rust
enum McpStartupPermission {
    Enabled,
    Disabled,
}

fn read_mcp_startup_permission(path: &Path) -> Result<McpStartupPermission, BridgeError>;
```

Rules:

- missing settings file or missing `mcp` group => `Disabled`;
- `mcp.enabled == false` => `Disabled`;
- `mcp.enabled == true` => `Enabled`;
- unreadable file, invalid JSON, malformed MCP group, or non-boolean enabled value => `McpConfigUnavailable`.

Add `BridgeError::McpDisabled` and `BridgeError::McpConfigUnavailable` display text with stable codes. Call the preflight only after the first IPC connection fails and immediately before any launcher invocation. A running service must remain usable without rereading settings on every request.

### Step 5: Run focused bridge tests

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tests::bridge_ -- --nocapture
```

Expected: disabled/config tests pass, existing-listener test records zero launches, enabled/unavailable test records one launch.

### Step 6: Commit

```bash
git add apps/desktop/src-tauri/src/mcp/ipc.rs apps/desktop/src-tauri/src/mcp/bridge.rs apps/desktop/src-tauri/src/mcp/tests.rs
git commit -m "fix: gate MCP background startup"
```

## Task 2: Resolve and launch the bundled QingYu executable without PATH

**Files:**

- Modify: `apps/desktop/src-tauri/src/mcp/bridge.rs`
- Modify: `apps/desktop/src-tauri/src/mcp/tests.rs`
- Modify: `apps/desktop/src-tauri/src/bin/qingyu-mcp.rs`

### Step 1: Add failing executable-resolution and command tests

Cover each packaging layout with pure path tests:

```rust
#[test]
fn resolves_macos_markra_beside_mcp_bridge() {
    // .../QingYu.app/Contents/MacOS/qingyu-mcp -> .../MacOS/markra
}

#[test]
fn resolves_windows_markra_beside_mcp_bridge() {
    // C:\\...\\qingyu-mcp.exe -> C:\\...\\markra.exe
}

#[test]
fn resolves_linux_markra_beside_mcp_bridge() {
    // /opt/qingyu/qingyu-mcp -> /opt/qingyu/markra
}
```

Assert that the launch request contains the exact absolute executable and arguments `mcp`, `serve`, with no `open`, `-a`, or PATH-resolved command.

### Step 2: Run and confirm failure

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tests::resolves_ -- --nocapture
```

Expected: failures because the current launcher uses `open -a QingYu` on macOS and command names elsewhere.

### Step 3: Replace platform shell launch with sibling resolution

Introduce a pure resolver and a launch request value:

```rust
struct AppLaunchRequest {
    executable: PathBuf,
    arguments: [&'static str; 2],
}

fn sibling_app_launch_request(
    bridge_executable: &Path,
    platform: BridgePlatform,
) -> Result<AppLaunchRequest, BridgeError>;
```

Resolve only the current packaged sibling (`markra` or `markra.exe`; the technical name changes only with the later package rename). Reject a missing/non-file target with `app_launch_failed`; never fall back to `open`, a bundle name, shell evaluation, or PATH lookup.

Production startup gets `std::env::current_exe()`, builds the request once, and calls `std::process::Command::new(absolute_path).args(["mcp", "serve"]).spawn()`.

### Step 4: Preserve stable CLI error output

Keep the bridge error on stderr with a non-zero exit. If useful for testing, factor the exit mapping into a small function, but do not print settings contents or paths containing note data.

### Step 5: Run focused tests

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tests::resolves_ -- --nocapture
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tests::bridge_ -- --nocapture
```

Expected: all resolution, launch-count, and stable-error tests pass.

### Step 6: Commit

```bash
git add apps/desktop/src-tauri/src/mcp/bridge.rs apps/desktop/src-tauri/src/mcp/tests.rs apps/desktop/src-tauri/src/bin/qingyu-mcp.rs
git commit -m "fix: launch bundled QingYu without PATH"
```

## Task 3: Add a windowless `mcp serve` desktop launch mode

**Files:**

- Modify: `apps/desktop/src-tauri/src/desktop_runtime.rs`
- Modify: `apps/desktop/src-tauri/src/builder_boundary_tests.rs`
- Modify: `apps/desktop/src-tauri/src/mcp/tests.rs`

### Step 1: Add failing launch-mode behavior tests

Add pure tests for argument parsing and single-instance decisions:

```rust
#[test]
fn mcp_serve_selects_headless_service_mode() {}

#[test]
fn ordinary_launch_selects_normal_mode() {}

#[test]
fn service_single_instance_invocation_does_not_reveal_window() {}

#[test]
fn ordinary_single_instance_invocation_reveals_window() {}
```

Add a builder boundary assertion that the service branch clears configured startup windows before `.build(...)` and does not call the startup reveal path.

### Step 2: Run and confirm failure

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml desktop_runtime -- --nocapture
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml builder_boundary_tests -- --nocapture
```

Expected: new parser/window-boundary tests fail because all launches currently create or reveal the main window.

### Step 3: Parse the internal service command before Tauri construction

Add:

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DesktopLaunchMode {
    Normal,
    McpService,
}

fn desktop_launch_mode<I, S>(args: I) -> DesktopLaunchMode
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>;
```

Recognize only the exact `mcp serve` argument sequence. This command accepts no workspace, executable, or file path from the MCP client.

### Step 4: Build service mode without a startup window

Before `Builder::build`, make the generated Tauri context mutable. For `McpService`:

- clear `context.config_mut().app.windows`;
- skip window chrome and startup reveal fallback;
- skip opened-file delivery and UI settings prewarm;
- on macOS, set `ActivationPolicy::Prohibited` so the MCP-only runtime cannot become the active application;
- still initialize settings and backend services needed by MCP.

Keep the normal path unchanged.

### Step 5: Make single-instance and reopen behavior mode-aware

In the single-instance callback:

- ignore `mcp serve` invocations visually;
- for ordinary invocations, switch macOS activation policy back to `Regular`, then create/restore/reveal the editor window.

Apply the same `Regular` transition to an explicit Dock reopen. Ensure only user-originated normal launches show and focus a window.

### Step 6: Run focused runtime tests

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml desktop_runtime -- --nocapture
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml builder_boundary_tests -- --nocapture
```

Expected: parser, no-startup-window boundary, silent service invocation, and ordinary reveal tests pass.

### Step 7: Commit

```bash
git add apps/desktop/src-tauri/src/desktop_runtime.rs apps/desktop/src-tauri/src/builder_boundary_tests.rs apps/desktop/src-tauri/src/mcp/tests.rs
git commit -m "feat: add headless MCP service mode"
```

## Task 4: Initialize MCP workspace authority without a webview

**Files:**

- Modify: `apps/desktop/src-tauri/src/mcp/mod.rs`
- Modify: `apps/desktop/src-tauri/src/mcp/tests.rs`
- Modify: `apps/desktop/src-tauri/src/primary_workspace.rs`

### Step 1: Add a failing backend-only activation test

Create a test app state with a persisted authoritative primary workspace and no main window. Initialize MCP, then assert the workspace registry is active for exactly that root.

Also test the no-workspace state remains fail-closed and does not invent a root.

### Step 2: Run and confirm failure

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tests::initializes_authoritative_workspace_without_window -- --nocapture
```

Expected: failure because activation currently depends on the React `setPrimaryWorkspace` call after the main webview loads.

### Step 3: Factor authoritative root resolution for backend startup

Reuse `primary_workspace::with_primary_workspace_transaction` rather than duplicating settings or trusting any client path. Add a small helper that:

- resolves the persisted authoritative primary root;
- activates the MCP workspace registry when valid;
- clears/leaves inactive authority when no primary workspace exists;
- records a safe diagnostic without exposing the path to the bridge on failure.

Call it during `mcp::initialize` before the listener becomes usable. Keep the existing frontend command as a consistency check for later workspace changes.

### Step 4: Handle headless disable lifecycle

Expose enough controller state for the desktop runtime to know when the listener is disabled. In service-only mode, exit only after MCP is disabled and active sessions have drained. Normal UI mode must never exit merely because MCP was disabled.

Cover the lifecycle decision with a pure unit test even if the session drain itself remains owned by the existing controller.

### Step 5: Run focused MCP tests

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tests::initializes_authoritative_workspace_without_window -- --nocapture
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp::tests -- --nocapture
```

Expected: backend-only activation, fail-closed no-root behavior, and existing document-scope tests pass.

### Step 6: Commit

```bash
git add apps/desktop/src-tauri/src/mcp/mod.rs apps/desktop/src-tauri/src/mcp/tests.rs apps/desktop/src-tauri/src/primary_workspace.rs
git commit -m "fix: initialize MCP authority headlessly"
```

## Task 5: Lock the absolute-path client configuration contract

**Files:**

- Modify: `packages/app/src/lib/mcp-client-config.test.ts`
- Modify if needed: `packages/app/src/lib/mcp-client-config.ts`
- Modify: `docs/qingyu-mcp.md`
- Modify: `docs/superpowers/specs/2026-07-21-qingyu-mcp-quick-configuration-design.md`

### Step 1: Add or strengthen client-config tests

Assert Codex and generic JSON output uses the exact absolute bundled `qingyu-mcp` command, contains no `markra` wrapper dependency, and requires no token or PATH setup.

Run:

```bash
pnpm exec vitest run packages/app/src/lib/mcp-client-config.test.ts
```

Expected before any needed implementation: either a failure that exposes drift or a passing regression test confirming the existing configuration generator already satisfies the requirement.

### Step 2: Make only necessary generator changes

If the regression test already passes, do not rewrite the generator. If it fails, preserve its existing platform-specific escaping while ensuring the backend-provided absolute bridge path is the command value.

### Step 3: Update operator documentation

Document:

- copied configuration already contains an absolute bundled executable path;
- no PATH installation and no optional `markra` shell command is needed;
- `mcp_disabled` means the client remains configured but MCP is off in QingYu;
- enabled, closed QingYu starts only the headless service;
- `mcp_config_unavailable` and `app_launch_failed` troubleshooting;
- `mcp serve` is an internal service command and should not receive user paths.

### Step 4: Run the focused frontend test

Run:

```bash
pnpm exec vitest run packages/app/src/lib/mcp-client-config.test.ts
```

Expected: pass.

### Step 5: Commit

```bash
git add packages/app/src/lib/mcp-client-config.test.ts packages/app/src/lib/mcp-client-config.ts docs/qingyu-mcp.md docs/superpowers/specs/2026-07-21-qingyu-mcp-quick-configuration-design.md
git commit -m "docs: clarify bundled MCP CLI startup"
```

## Task 6: Build and verify the actual MCP lifecycle

**Files:**

- Modify only if tests expose a defect: files from Tasks 1-5
- Do not commit: generated bundles, `target/`, local Codex config, local application settings

### Step 1: Run the clean-worktree automated gates

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
pnpm test
pnpm typecheck:test
pnpm build
```

Expected: all pass. If a gate fails, classify whether it is caused by this branch before modifying code.

### Step 2: Produce a debug bundle

Run the repository-supported debug package command:

```bash
pnpm tauri build --debug
```

Confirm both packaged executables exist inside the same application bundle and neither is tracked by Git:

```bash
test -x apps/desktop/src-tauri/target/debug/bundle/macos/QingYu.app/Contents/MacOS/QingYu
test -x apps/desktop/src-tauri/target/debug/bundle/macos/QingYu.app/Contents/MacOS/qingyu-mcp
git status --short
```

### Step 3: Verify disabled behavior with an absolute bridge path

Back up the local application settings before changing them. Set MCP disabled through the application-supported setting path. Stop QingYu, invoke the bundled bridge by absolute path with a minimal MCP initialize exchange, and verify:

- stderr contains `mcp_disabled`;
- exit is non-zero;
- no QingYu process starts;
- no QingYu window appears;
- the current foreground application is unchanged.

Restore/retain user settings according to the test sequence.

### Step 4: Verify enabled headless startup and process reuse

Enable MCP, stop QingYu, then invoke the configured absolute bridge through Codex or a protocol fixture. Verify:

- exactly one QingYu service process starts with `mcp serve`;
- no editor window or focus change occurs;
- the private IPC endpoint appears;
- a second bridge session reuses the same process PID.

### Step 5: Exercise real MCP tools

Using the current Codex MCP configuration, call at minimum:

- MCP health/capability discovery;
- current workspace/status or document listing;
- one read-only document operation scoped to the currently open QingYu folder.

Do not mutate or delete user documents merely to prove connectivity. Record tool results and confirm no path outside the authoritative folder is exposed.

### Step 6: Verify explicit app launch and later disable

While the service process is running, explicitly launch QingYu and confirm the normal editor window appears from the existing process. Then disable MCP and confirm the listener stops. Invoke the still-configured bridge again and verify `mcp_disabled` without reopening QingYu.

### Step 7: Final diff and hygiene review

Run:

```bash
git diff --check
git status --short
git log --oneline --decorate -8
```

Expected: only intended source/docs changes are committed; no app bundle, target output, local settings, or Codex configuration is staged.

### Step 8: Integrate after verification

Review the isolated branch diff, merge it into local `main` without discarding the primary checkout's unrelated dirty files, and rerun the smallest affected checks if the merge base changed. Do not push unless explicitly requested.
