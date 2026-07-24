# QingYu MCP Dock Restoration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ensure an MCP-started QingYu process becomes a normal Dock-visible macOS application when the user later opens QingYu normally.

**Architecture:** Keep `activate_normal_ui` as the single promotion boundary. Restore both AppKit activation policy and Tauri Dock visibility before the existing window reveal/focus path, then point local Codex MCP startup at the installed app bundle rather than a generated debug bundle.

**Tech Stack:** Rust, Tauri v2, macOS AppKit/LaunchServices, Codex TOML configuration.

## Global Constraints

- MCP-only launches must remain backgrounded and must not reveal an editor window.
- Ordinary launches routed through the single-instance plugin must restore the Dock identity before showing or focusing the editor.
- Codex must use `/Applications/QingYu.app/Contents/MacOS/qingyu-mcp` without adding a PATH installation or secret.
- Preserve the root `macos-icon.icns` file.

---

### Task 1: Restore Dock visibility during normal UI promotion

**Files:**
- Modify and test: `apps/desktop/src-tauri/src/desktop_runtime.rs`

**Interfaces:**
- Consumes: `tauri::AppHandle::set_activation_policy` and `tauri::AppHandle::set_dock_visibility`.
- Produces: `activate_normal_ui` that restores both macOS foreground properties.

- [ ] **Step 1: Add the failing regression test**

Add a source-boundary test alongside the existing macOS MCP activation test. Extract the `activate_normal_ui` function body and assert that it contains these calls in order:

```rust
app.set_activation_policy(tauri::ActivationPolicy::Regular)
app.set_dock_visibility(true)
```

- [ ] **Step 2: Run the test and verify RED**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml desktop_runtime::tests::normal_ui_promotion_restores_dock_visibility -- --exact
```

Expected: failure because `activate_normal_ui` does not call `set_dock_visibility(true)`.

- [ ] **Step 3: Implement the minimum runtime change**

After the activation-policy call, add:

```rust
if let Err(error) = app.set_dock_visibility(true) {
    eprintln!("QingYu Dock visibility update failed: {error}");
}
```

Keep both calls inside the existing macOS configuration block.

- [ ] **Step 4: Run the focused tests and verify GREEN**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml desktop_runtime::tests
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
```

Expected: all desktop-runtime tests pass and formatting exits zero.

- [ ] **Step 5: Commit the runtime fix**

```bash
git add apps/desktop/src-tauri/src/desktop_runtime.rs
git commit -m "fix: restore Dock after MCP startup"
```

### Task 2: Build and validate the corrected macOS application

**Files:**
- Verify repository state only.
- Produce: `apps/desktop/src-tauri/target/release/bundle/macos/QingYu.app`.

**Interfaces:**
- Consumes: the corrected Rust runtime and existing bundled `qingyu-mcp` sidecar.
- Produces: an installable unsigned macOS app bundle.

- [ ] **Step 1: Run focused repository verification**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml desktop_runtime::tests
pnpm --filter @markra/desktop test
pnpm typecheck:test
git diff --check
```

Expected: every command exits zero.

- [ ] **Step 2: Build the macOS app bundle**

```bash
pnpm app build desktop --no-sign --bundles app
```

Expected: a `QingYu.app` bundle containing executable `markra` and sidecar `qingyu-mcp`.

### Task 3: Install, reconfigure, and perform live verification

**Files:**
- Modify outside repository: `/Users/ying/.codex/config.toml`
- Replace with backup: `/Applications/QingYu.app`

**Interfaces:**
- Consumes: the verified app bundle from Task 2.
- Produces: stable MCP startup and a normal Dock-visible QingYu process.

- [ ] **Step 1: Merge the verified branch into local `main`**

Fast-forward `main`, preserving untracked `macos-icon.icns`.

- [ ] **Step 2: Update the Codex MCP command**

Change only `mcp_servers.qingyu.command` to:

```toml
command = "/Applications/QingYu.app/Contents/MacOS/qingyu-mcp"
```

- [ ] **Step 3: Install the verified app with a recoverable backup**

Move the existing app to a timestamped backup under `/Applications`, copy the new app into place with `ditto`, and confirm both executables exist.

- [ ] **Step 4: Restart and verify ordinary launch**

Stop only the stale QingYu MCP/app processes, open `/Applications/QingYu.app`, and verify through `NSRunningApplication` that:

```text
bundleURL=/Applications/QingYu.app
activationPolicy=0
isActive=true
ownsMenuBar=true
```

- [ ] **Step 5: Verify configured MCP startup**

Launch the configured `qingyu-mcp` executable and confirm the running application remains the installed foreground instance without creating a generated-bundle process.
