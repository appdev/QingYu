# Settings Window On-Demand Creation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prevent QingYu startup from creating a Settings window and create the native owned window only after an explicit Settings request.

**Architecture:** Remove the delayed React startup invocation and delete the prewarm command from the shared runtime, desktop adapter, and Rust command surface. Keep the explicit-open path unchanged: it builds Settings hidden under the invoking editor, then reveals it after the Settings frontend reports ready.

**Tech Stack:** React 19, TypeScript 6, Vitest, Rust, Tauri 2.11, Cargo test

## Global Constraints

- Starting or reopening QingYu must not create, preload, or reveal Settings.
- Explicit Settings open must retain mandatory native parent ownership.
- Explicit Settings open must remain hidden until frontend readiness or the bounded reveal fallback.
- Hidden Settings reuse and five-minute idle destruction after a user close remain supported.
- Owner movement, native close coordination, and platform-specific controls remain unchanged.
- Add no dependency and use `pnpm` for JavaScript workflows.
- Preserve unrelated main-worktree sync changes and `macos-icon.icns`.

---

### Task 1: Stop the main application from prewarming Settings

**Files:**
- Modify: `packages/app/src/App.test.tsx:66,5328-5338`
- Modify: `packages/app/src/App.tsx:142,296,1528-1538`

**Interfaces:**
- Consumes: `appFeatures.settingsWindow` and the existing explicit `openSettingsWindow(...)` actions
- Produces: a startup lifecycle with no `prewarmSettingsWindow()` invocation

- [ ] **Step 1: Replace the startup-prewarm test with a failing no-prewarm regression**

Keep the existing `mockedPrewarmSettingsWindow` import for the RED run and replace the test with:

```tsx
it("does not create Settings during workspace startup", async () => {
  mockedConsumeWelcomeDocumentState.mockResolvedValue(false);

  renderApp();

  expect(await screen.findByRole("heading", { name: "Untitled.md" })).toBeInTheDocument();
  await waitFor(() => expect(mockedShowNativeWindow).toHaveBeenCalledTimes(1));
  await act(async () => {
    await new Promise((resolve) => window.setTimeout(resolve, 750));
  });
  expect(mockedPrewarmSettingsWindow).not.toHaveBeenCalled();
});
```

- [ ] **Step 2: Run the focused test and verify RED**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/App.test.tsx -t "does not create Settings during workspace startup"
```

Expected: FAIL because startup invokes `prewarmSettingsWindow` once after 600 milliseconds.

- [ ] **Step 3: Remove the startup effect**

In `App.tsx`, remove the `prewarmSettingsWindow` import, the
`settingsWindowPrewarmDelayMs` constant, and the `useEffect` that schedules the
prewarm call. Do not change any explicit `openSettingsWindow(...)` call.

- [ ] **Step 4: Run the focused test and verify GREEN**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/App.test.tsx -t "does not create Settings during workspace startup"
```

Expected: PASS with one test and zero failures.

- [ ] **Step 5: Commit the startup behavior change**

```bash
git add packages/app/src/App.tsx packages/app/src/App.test.tsx
git commit -m "fix: create settings only on demand"
```

### Task 2: Delete the prewarm API and native startup mode

**Files:**
- Modify: `apps/desktop/src-tauri/src/builder_boundary_tests.rs:135`
- Modify: `apps/desktop/src-tauri/src/desktop_runtime.rs:410`
- Modify: `apps/desktop/src-tauri/src/windows.rs:112,213-215,849-891,1354-1581,1677-1681,2188-2205`
- Modify: `apps/desktop/src/runtime/desktop.ts:235`
- Modify: `apps/desktop/src/runtime/tauri/window.ts:63-65`
- Modify: `apps/desktop/src/runtime/tauri/window.test.ts:22,296-302`
- Modify: `packages/app/src/runtime/index.ts:405,808`
- Modify: `packages/app/src/lib/tauri/window.ts:38-40`
- Modify: `packages/app/src/test/app-harness.tsx:68,244,755,1114,1247`
- Modify: `packages/app/src/App.test.tsx:66`

**Interfaces:**
- Consumes: `open_settings_window`, `mark_settings_window_ready`, and `SettingsWindowOpenContext`
- Produces: an `AppWindowRuntime` and Tauri command handler with no prewarm entry, while `open_settings_window(...) -> Result<(), String>` retains the explicit owned-window path

- [ ] **Step 1: Make the native command boundary test fail**

Remove `"prewarm_settings_window",` from `DESKTOP_COMMANDS` in
`builder_boundary_tests.rs` without changing production registration.

- [ ] **Step 2: Run the exact boundary test and verify RED**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml builder_boundary_tests::builder_boundary_desktop_preserves_the_complete_command_surface -- --exact
```

Expected: FAIL with `desktop command registrations changed` because production still registers `prewarm_settings_window`.

- [ ] **Step 3: Remove the cross-layer prewarm contract**

Delete:

- `prewarmSettingsWindow` from `AppWindowRuntime`, its default implementation,
  `packages/app/src/lib/tauri/window.ts`, desktop runtime wiring, and the Tauri
  adapter;
- the Tauri adapter unit test and app-harness mock/export/reset/default;
- `prewarm_settings_window` from the Rust invoke handler;
- the now-unused `mockedPrewarmSettingsWindow` import in `App.test.tsx`.

Do not alter `openSettingsWindow`, `markSettingsWindowReady`, or hide-handshake
methods.

- [ ] **Step 4: Collapse native creation to the explicit-open path**

In `windows.rs`:

- remove `should_prewarm_settings_window`;
- remove `SettingsWindowStartupMode` and the `mode` parameters;
- always call `begin_settings_window_creation(true, Some(context.clone()))`;
- always call `handle_existing_settings_window(...)` for a reusable existing
  Settings window;
- remove `SettingsWindowCreationResult::KeepHidden` and its creation match arm;
- keep `schedule_settings_window_idle_destroy(...)` because normal user hides
  still use it;
- remove tests that exist only for primary-window prewarming and `KeepHidden`.

The explicit asynchronous wrapper remains:

```rust
pub(crate) fn spawn_settings_window(
    app: tauri::AppHandle,
    owner_window: tauri::WebviewWindow,
    context: SettingsWindowOpenContext,
) -> impl std::future::Future<Output = Result<(), String>> {
    spawn_settings_window_blocking(app, owner_window, context)
}
```

- [ ] **Step 5: Run focused Rust, bridge, and application tests**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml builder_boundary_tests::builder_boundary_desktop_preserves_the_complete_command_surface -- --exact
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml windows:: -- --test-threads=1
pnpm --filter @markra/desktop exec vitest run src/runtime/tauri/window.test.ts
pnpm --filter @markra/app exec vitest run src/App.test.tsx
```

Expected: all selected tests pass with zero failures.

- [ ] **Step 6: Verify no production prewarm references remain**

Run:

```bash
rg -n "prewarm_settings_window|prewarmSettingsWindow|SettingsWindowStartupMode|settingsWindowPrewarmDelayMs" apps packages
```

Expected: no output and exit status 1.

- [ ] **Step 7: Commit the API removal**

```bash
git add apps/desktop/src-tauri/src/builder_boundary_tests.rs apps/desktop/src-tauri/src/desktop_runtime.rs apps/desktop/src-tauri/src/windows.rs apps/desktop/src/runtime/desktop.ts apps/desktop/src/runtime/tauri/window.ts apps/desktop/src/runtime/tauri/window.test.ts packages/app/src/runtime/index.ts packages/app/src/lib/tauri/window.ts packages/app/src/test/app-harness.tsx packages/app/src/App.test.tsx
git commit -m "refactor: remove settings window prewarm"
```

### Task 3: Run acceptance and startup verification

**Files:**
- Verify only: all files changed by Tasks 1-2

**Interfaces:**
- Consumes: the final on-demand Settings implementation
- Produces: repository and macOS runtime evidence for startup and explicit open

- [ ] **Step 1: Run formatting and repository gates sequentially**

```bash
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml -- --test-threads=1
pnpm test
pnpm typecheck:test
pnpm build
git diff --check main...HEAD
```

Expected: every command exits 0.

- [ ] **Step 2: Build and launch an isolated macOS debug app**

```bash
pnpm tauri build --debug --config '{"identifier":"dev.markra.settings-ondemand-qa"}'
```

Expected: the debug application bundle is created successfully.

- [ ] **Step 3: Verify the user-visible lifecycle**

Launch the isolated bundle and verify:

1. startup exposes only the editor after more than 600 milliseconds;
2. startup logs contain no `prewarm_settings_window` command;
3. clicking Settings creates and reveals the owned Settings child;
4. closing Settings hides it without closing the editor;
5. reopening Settings works and owner close still completes the hide handshake.

- [ ] **Step 4: Commit documentation updates if needed**

```bash
git add docs/superpowers/specs/2026-07-23-settings-window-parent-ownership-design.md docs/superpowers/plans/2026-07-24-settings-window-on-demand.md
git commit -m "docs: plan on-demand settings creation"
```
