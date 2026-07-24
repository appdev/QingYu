# Settings Window Parent Ownership Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make QingYu's native Settings window a non-modal child of its owning editor window, preserve platform-specific close controls, follow owner movement, and shut down the Settings session safely before the owner is destroyed.

**Architecture:** Keep one application-wide Settings webview, but attach it with Tauri `WebviewWindowBuilder::parent` to the editor that creates it. Put pure ownership and geometry decisions in a small Rust module, keep Tauri orchestration in `windows.rs`, and route editor destruction through a native command so the existing Settings hide handshake finishes before an owned window disappears.

**Tech Stack:** Rust, Tauri 2.11, React 19, TypeScript 6, Vitest, Cargo test

## Global Constraints

- Settings remains a native, non-modal, application-wide singleton.
- The owning editor remains usable while Settings is visible.
- Parent binding is mandatory; never fall back to an independent Settings window.
- Prewarm is allowed only for the primary `main` editor.
- A visible singleton is focused rather than recreated under a second editor.
- A hidden singleton may be recreated under a different editor owner.
- Preserve the existing Settings sync-session hide handshake and bounded fallback.
- Preserve `MacWindowControls`, `WindowsWindowControls`, and Linux native decorations.
- Add no dependency and use `pnpm` for JavaScript workflows.
- Do not touch the unrelated untracked `macos-icon.icns` or
  `docs/superpowers/plans/2026-07-23-settings-appearance-refresh-stability.md`.

## File Map

- Create `apps/desktop/src-tauri/src/windows/settings_ownership.rs`: pure owner-selection, centering, offset, and follow-position logic with unit tests.
- Modify `apps/desktop/src-tauri/src/windows.rs`: Settings runtime ownership state, required parent binding, prewarm/open selection, movement routing, owner cleanup, and safe owner-destroy command.
- Modify `apps/desktop/src-tauri/src/desktop_runtime.rs`: register the safe editor-destroy command.
- Modify `apps/desktop/src-tauri/src/builder_boundary_tests.rs`: keep the exact desktop command surface guarded.
- Modify `apps/desktop/src/runtime/tauri/window.ts`: route editor destruction through the native coordination command.
- Modify `apps/desktop/src/runtime/tauri/window.test.ts`: verify the bridge invokes the coordination command while ordinary close remains unchanged.
- Verify, but do not redesign, `packages/app/src/components/MacWindowControls.tsx`, `packages/app/src/components/WindowsWindowControls.tsx`, and `packages/app/src/components/SettingsWindow.tsx`.

---

### Task 1: Add pure ownership and geometry decisions

**Files:**
- Create: `apps/desktop/src-tauri/src/windows/settings_ownership.rs`
- Modify: `apps/desktop/src-tauri/src/windows.rs:1-90`

**Interfaces:**
- Consumes: editor labels, visibility, `tauri::PhysicalPosition<i32>`, and `tauri::PhysicalSize<u32>`
- Produces: `ExistingSettingsAction`, `SettingsWindowOwnership`, `centered_child_position`, `relative_offset`, and `position_from_offset`

- [ ] **Step 1: Write the failing pure unit tests**

Create the module with tests first; the imports deliberately name production
items that do not exist yet:

```rust
#[cfg(test)]
mod tests {
    use super::{
        centered_child_position, existing_settings_action, position_from_offset,
        relative_offset, ExistingSettingsAction,
    };
    use tauri::{PhysicalPosition, PhysicalSize};

    #[test]
    fn visible_singleton_is_focused_even_for_another_editor() {
        assert_eq!(
            existing_settings_action(Some("main"), "markra-editor-2", true),
            ExistingSettingsAction::FocusExisting
        );
    }

    #[test]
    fn hidden_singleton_is_recreated_only_for_another_owner() {
        assert_eq!(
            existing_settings_action(Some("main"), "markra-editor-2", false),
            ExistingSettingsAction::RecreateHidden
        );
        assert_eq!(
            existing_settings_action(Some("main"), "main", false),
            ExistingSettingsAction::ReuseHidden
        );
    }

    #[test]
    fn centers_child_and_preserves_user_selected_offset() {
        let owner = PhysicalPosition::new(100, 200);
        assert_eq!(
            centered_child_position(
                owner,
                PhysicalSize::new(1400, 900),
                PhysicalSize::new(1000, 700)
            ),
            PhysicalPosition::new(300, 300)
        );

        let child = PhysicalPosition::new(360, 340);
        let offset = relative_offset(owner, child);
        assert_eq!(offset, PhysicalPosition::new(260, 140));
        assert_eq!(
            position_from_offset(PhysicalPosition::new(180, 260), offset),
            PhysicalPosition::new(440, 400)
        );
    }
}
```

Add `mod settings_ownership;` near the top of `windows.rs` so Cargo compiles the failing module.

- [ ] **Step 2: Run the focused tests and verify they fail**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml windows::settings_ownership::tests -- --nocapture
```

Expected: compilation fails because the imported ownership enum and geometry
functions have not been defined.

- [ ] **Step 3: Implement the pure decisions**

Add the production types and deterministic arithmetic above the test module:

```rust
use tauri::{PhysicalPosition, PhysicalSize};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExistingSettingsAction {
    FocusExisting,
    RecreateHidden,
    ReuseHidden,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct SettingsWindowOwnership {
    pub(crate) owner_label: Option<String>,
    pub(crate) owner_last_position: Option<PhysicalPosition<i32>>,
    pub(crate) relative_offset: Option<PhysicalPosition<i32>>,
    pub(crate) pending_owner_destroy: Option<String>,
}

fn clamp_i64(value: i64) -> i32 {
    value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

pub(crate) fn existing_settings_action(
    existing_owner: Option<&str>,
    requesting_owner: &str,
    visible: bool,
) -> ExistingSettingsAction {
    if visible {
        return ExistingSettingsAction::FocusExisting;
    }
    if existing_owner == Some(requesting_owner) {
        return ExistingSettingsAction::ReuseHidden;
    }
    ExistingSettingsAction::RecreateHidden
}

pub(crate) fn centered_child_position(
    owner_position: PhysicalPosition<i32>,
    owner_size: PhysicalSize<u32>,
    child_size: PhysicalSize<u32>,
) -> PhysicalPosition<i32> {
    let x = i64::from(owner_position.x)
        + (i64::from(owner_size.width) - i64::from(child_size.width)) / 2;
    let y = i64::from(owner_position.y)
        + (i64::from(owner_size.height) - i64::from(child_size.height)) / 2;
    PhysicalPosition::new(clamp_i64(x), clamp_i64(y))
}

pub(crate) fn relative_offset(
    owner_position: PhysicalPosition<i32>,
    child_position: PhysicalPosition<i32>,
) -> PhysicalPosition<i32> {
    PhysicalPosition::new(
        child_position.x.saturating_sub(owner_position.x),
        child_position.y.saturating_sub(owner_position.y),
    )
}

pub(crate) fn position_from_offset(
    owner_position: PhysicalPosition<i32>,
    offset: PhysicalPosition<i32>,
) -> PhysicalPosition<i32> {
    PhysicalPosition::new(
        owner_position.x.saturating_add(offset.x),
        owner_position.y.saturating_add(offset.y),
    )
}
```

- [ ] **Step 4: Run focused tests and commit**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml windows::settings_ownership::tests -- --nocapture
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
```

Expected: three focused tests pass and formatting exits 0.

Commit:

```bash
git add apps/desktop/src-tauri/src/windows.rs apps/desktop/src-tauri/src/windows/settings_ownership.rs
git commit -m "test: define settings window ownership rules"
```

---

### Task 2: Create and prewarm Settings under a required editor parent

**Files:**
- Modify: `apps/desktop/src-tauri/src/windows.rs:79-110, 1208-1430, 1458-2220`

**Interfaces:**
- Consumes: `ExistingSettingsAction`, `SettingsWindowOwnership`, the invoking `tauri::WebviewWindow`, and the existing `SettingsWindowOpenContext`
- Produces: `open_settings_window(...) -> Result<(), String>` and `prewarm_settings_window(...) -> Result<(), String>` that complete only after parent selection/build succeeds

- [ ] **Step 1: Add failing ownership, prewarm, and builder-contract tests**

Add focused tests to the existing `windows.rs` test module:

```rust
#[test]
fn settings_owner_must_be_an_editor_window() {
    assert!(is_editor_window_label("main"));
    assert!(is_editor_window_label("markra-editor-3"));
    assert!(!is_editor_window_label("markra-settings"));
    assert!(!is_editor_window_label("asset-preview"));
}

#[test]
fn only_the_primary_editor_may_prewarm_settings() {
    assert!(should_prewarm_settings_window("main"));
    assert!(!should_prewarm_settings_window("markra-editor-2"));
    assert!(!should_prewarm_settings_window("markra-settings"));
}

#[test]
fn settings_builder_requires_the_invoking_editor_as_parent() {
    let source = include_str!("windows.rs");
    let start = source.find("fn create_settings_window_blocking")
        .expect("settings builder helper should exist");
    let end = source[start..].find("pub(crate) fn spawn_settings_window")
        .map(|offset| start + offset)
        .expect("settings builder helper should end before public spawn");
    let builder = &source[start..end];

    assert!(builder.contains(".parent(&owner_window)"));
    assert!(!builder.contains("unwrap_or(builder)"));
    assert!(builder.contains("map_err(|error| error.to_string())?"));
}

#[test]
fn settings_creation_serializes_owner_selection_and_reset_clears_it() {
    let source = include_str!("windows.rs");
    assert!(source.contains("SETTINGS_WINDOW_OPERATION_LOCK"));
    assert!(source.contains("state.ownership = SettingsWindowOwnership::default()"));
}
```

- [ ] **Step 2: Run the focused tests and verify they fail**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml settings_owner_must_be_an_editor_window -- --nocapture
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml only_the_primary_editor_may_prewarm_settings -- --nocapture
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml settings_builder_requires_the_invoking_editor_as_parent -- --nocapture
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml settings_creation_serializes_owner_selection_and_reset_clears_it -- --nocapture
```

Expected: Cargo reports that `should_prewarm_settings_window` and `create_settings_window_blocking` do not exist and that the builder contract is absent.

- [ ] **Step 3: Add ownership state and required parent selection**

Extend `SettingsWindowRuntimeState` and add the prewarm guard:

```rust
#[derive(Default)]
struct SettingsWindowRuntimeState {
    creating: bool,
    hide_acknowledged: bool,
    hide_request_generation: usize,
    hide_requested: bool,
    idle_destroy_generation: usize,
    ownership: SettingsWindowOwnership,
    pending_context: Option<SettingsWindowOpenContext>,
    ready: bool,
    show_when_ready: bool,
}

fn should_prewarm_settings_window(label: &str) -> bool {
    label == MAIN_WINDOW_LABEL
}

static SETTINGS_WINDOW_OPERATION_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn settings_window_operation_lock() -> &'static Mutex<()> {
    SETTINGS_WINDOW_OPERATION_LOCK.get_or_init(|| Mutex::new(()))
}
```

In `cancel_settings_window_creation`, `reset_settings_window_runtime_state`, and
the successful idle-destroy state transition, reset ownership together with the
existing readiness fields:

```rust
state.ownership = SettingsWindowOwnership::default();
```

This prevents a destroyed hidden window from leaving a stale parent label
behind.

Change creation to accept the actual owner window and require the native parent relationship before build:

```rust
fn create_settings_window_blocking<R>(
    app: tauri::AppHandle<R>,
    owner_window: tauri::WebviewWindow<R>,
    context: SettingsWindowOpenContext,
    mode: SettingsWindowStartupMode,
) -> Result<(), String>
where
    R: tauri::Runtime,
{
    if !is_editor_window_label(owner_window.label()) {
        return Err("Settings requires an editor window owner.".to_string());
    }
    if !owner_window.is_visible().unwrap_or(false) {
        return Err("Settings requires a visible editor window owner.".to_string());
    }
    if !app_has_visible_user_window(&app, None) {
        return Err("Settings requires a visible editor window.".to_string());
    }

    let owner_label = owner_window.label().to_string();
    let show_when_ready = mode == SettingsWindowStartupMode::Open;
    if !begin_settings_window_creation(
        show_when_ready,
        show_when_ready.then(|| context.clone()),
    ) {
        return Ok(());
    }

    let result = (|| {
        let startup_preferences = settings_window_startup_preferences(&app.config().identifier);
        let (width, height) = settings_window_inner_size();
        let (min_width, min_height) = settings_window_min_inner_size();
        let builder = WebviewWindowBuilder::new(
            &app,
            SETTINGS_WINDOW_LABEL,
            WebviewUrl::App(settings_window_url(
                context.target.as_deref(),
                context.project_root.as_deref(),
                context.workspace_source_path.as_deref(),
                context.source_window_label.as_deref(),
                show_when_ready,
                &startup_preferences,
            ).into()),
        )
        .parent(&owner_window)
        .map_err(|error| error.to_string())?
        .title(settings_window_title(startup_preferences.language))
        .inner_size(width, height)
        .min_inner_size(min_width, min_height)
        .visible(settings_window_visible())
        .decorations(settings_window_decorations())
        .transparent(settings_window_transparent())
        .resizable(settings_window_resizable())
        .shadow(settings_window_shadow())
        .center();

        #[cfg(not(target_os = "macos"))]
        let builder = match crate::menu::create_settings_window_menu(&app) {
            Ok(menu) => builder.menu(menu),
            Err(error) => {
                eprintln!("failed to create settings window menu: {error}");
                builder
            }
        };

        let builder = if let Some(color) = settings_window_background_color_for_preferences(
            current_window_chrome_platform(),
            &startup_preferences,
        ) {
            builder.background_color(color)
        } else {
            builder
        };

        #[cfg(target_os = "macos")]
        let builder = builder
            .title_bar_style(settings_window_title_bar_style())
            .hidden_title(settings_window_hidden_title());

        let window = builder.build().map_err(|error| error.to_string())?;
        hide_native_macos_window_controls(&window);
        hide_native_menu_for_settings_window(&window);

        settings_window_runtime_state()
            .lock()
            .map_err(|_| "Settings window state is unavailable.".to_string())?
            .ownership
            .owner_label = Some(owner_label);

        match finish_settings_window_creation() {
            SettingsWindowCreationResult::RevealWhenReady => {
                spawn_settings_window_reveal_fallback(window.clone());
            }
            SettingsWindowCreationResult::KeepHidden => {
                schedule_settings_window_idle_destroy(window.clone());
            }
            SettingsWindowCreationResult::Canceled => {
                window.destroy().map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    })();

    if result.is_err() {
        cancel_settings_window_creation();
    }
    result
}
```

Keep the parent call as an error-producing step. Do not use `unwrap_or` or any
other fallback that would build Settings without its owner.

- [ ] **Step 4: Make open/prewarm await the off-thread result**

Use Tauri's blocking runtime so the WebView is still created away from menu/reopen callbacks while invocation failures propagate:

```rust
fn create_or_reveal_settings_window_blocking<R>(
    app: tauri::AppHandle<R>,
    owner_window: tauri::WebviewWindow<R>,
    context: SettingsWindowOpenContext,
    mode: SettingsWindowStartupMode,
) -> Result<(), String>
where
    R: tauri::Runtime,
{
    let _operation = settings_window_operation_lock()
        .lock()
        .map_err(|_| "Settings window operation is unavailable.".to_string())?;

    if let Some(existing) = app.get_webview_window(SETTINGS_WINDOW_LABEL) {
        let visible = existing.is_visible().unwrap_or(false);
        let existing_owner = settings_window_runtime_state()
            .lock()
            .ok()
            .and_then(|state| state.ownership.owner_label.clone());

        return match existing_settings_action(
            existing_owner.as_deref(),
            owner_window.label(),
            visible,
        ) {
            ExistingSettingsAction::FocusExisting | ExistingSettingsAction::ReuseHidden => {
                if mode == SettingsWindowStartupMode::Open {
                    handle_existing_settings_window(&existing, &context);
                }
                Ok(())
            }
            ExistingSettingsAction::RecreateHidden => {
                reset_settings_window_runtime_state();
                existing.destroy().map_err(|error| error.to_string())?;
                create_settings_window_blocking(app, owner_window, context, mode)
            }
        };
    }

    create_settings_window_blocking(app, owner_window, context, mode)
}

async fn spawn_settings_window_with_mode<R>(
    app: tauri::AppHandle<R>,
    owner_window: tauri::WebviewWindow<R>,
    context: SettingsWindowOpenContext,
    mode: SettingsWindowStartupMode,
) -> Result<(), String>
where
    R: tauri::Runtime,
{
    tauri::async_runtime::spawn_blocking(move || {
        create_or_reveal_settings_window_blocking(app, owner_window, context, mode)
    })
    .await
    .map_err(|error| format!("Settings window task failed: {error}"))?
}

#[tauri::command]
pub(crate) async fn open_settings_window(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    target: Option<String>,
    project_root: Option<String>,
    workspace_source_path: Option<String>,
) -> Result<(), String> {
    let context = SettingsWindowOpenContext {
        project_root,
        source_window_label: Some(window.label().to_string()),
        target: normalized_settings_window_target(target.as_deref()).map(str::to_string),
        workspace_source_path,
    };
    spawn_settings_window_with_mode(app, window, context, SettingsWindowStartupMode::Open).await
}

#[tauri::command]
pub(crate) async fn prewarm_settings_window(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<(), String> {
    if !should_prewarm_settings_window(window.label()) {
        return Ok(());
    }
    spawn_settings_window_with_mode(
        app,
        window,
        SettingsWindowOpenContext::default(),
        SettingsWindowStartupMode::Prewarm,
    ).await
}
```

- [ ] **Step 5: Run focused Rust verification and commit**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml windows::tests -- --nocapture
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
```

Expected: all `windows::tests` pass, including the existing reveal, context, and hide-handshake coverage.

Commit:

```bash
git add apps/desktop/src-tauri/src/windows.rs apps/desktop/src-tauri/src/windows/settings_ownership.rs
git commit -m "feat: parent settings to its editor window"
```

---

### Task 3: Center Settings over its owner and follow owner movement

**Files:**
- Modify: `apps/desktop/src-tauri/src/windows.rs:1000-1205, 1329-1355, 1458-2220`
- Test: `apps/desktop/src-tauri/src/windows/settings_ownership.rs`

**Interfaces:**
- Consumes: `centered_child_position`, `relative_offset`, `position_from_offset`, `SettingsWindowOwnership`
- Produces: `center_settings_window_on_owner` and `apply_settings_window_movement`

- [ ] **Step 1: Add failing movement-routing tests**

Add pure and source-boundary coverage:

```rust
#[test]
fn owner_move_uses_the_saved_relative_offset() {
    let owner = PhysicalPosition::new(500, 400);
    let offset = PhysicalPosition::new(120, 80);
    assert_eq!(
        position_from_offset(owner, offset),
        PhysicalPosition::new(620, 480)
    );
}

#[test]
fn settings_lifecycle_routes_moved_events_for_owner_and_child() {
    let source = include_str!("windows.rs");
    let start = source.find("pub(crate) fn apply_settings_window_lifecycle")
        .expect("settings lifecycle should exist");
    let end = source[start..].find("fn spawn_settings_window_reveal_fallback")
        .map(|offset| start + offset)
        .expect("lifecycle should end before reveal fallback");
    let lifecycle = &source[start..end];

    assert!(lifecycle.contains("WindowEvent::Moved(position)"));
    assert!(lifecycle.contains("apply_settings_window_movement"));
}
```

- [ ] **Step 2: Run the focused tests and verify the routing test fails**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml owner_move_uses_the_saved_relative_offset -- --nocapture
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml settings_lifecycle_routes_moved_events_for_owner_and_child -- --nocapture
```

Expected: the pure geometry test passes, while the lifecycle routing assertion fails because moved events are not handled yet.

- [ ] **Step 3: Center the new child and record its offset**

After build and before reveal, center relative to the owner:

```rust
fn center_settings_window_on_owner<R>(
    owner: &tauri::WebviewWindow<R>,
    settings: &tauri::WebviewWindow<R>,
) -> Result<(), String>
where
    R: tauri::Runtime,
{
    let owner_position = owner.outer_position().map_err(|error| error.to_string())?;
    let owner_size = owner.outer_size().map_err(|error| error.to_string())?;
    let settings_size = settings.outer_size().map_err(|error| error.to_string())?;
    let settings_position = centered_child_position(owner_position, owner_size, settings_size);
    settings.set_position(settings_position).map_err(|error| error.to_string())?;

    let mut state = settings_window_runtime_state()
        .lock()
        .map_err(|_| "Settings window state is unavailable.".to_string())?;
    state.ownership.owner_last_position = Some(owner_position);
    state.ownership.relative_offset = Some(relative_offset(owner_position, settings_position));
    Ok(())
}
```

Call this wrapper immediately after build so a geometry failure affects only the
initial position:

```rust
fn initialize_settings_window_position<R>(
    owner: &tauri::WebviewWindow<R>,
    settings: &tauri::WebviewWindow<R>,
) where
    R: tauri::Runtime,
{
    if center_settings_window_on_owner(owner, settings).is_ok() {
        return;
    }

    let _ = settings.center();
    let (Ok(owner_position), Ok(settings_position)) =
        (owner.outer_position(), settings.outer_position())
    else {
        return;
    };
    if let Ok(mut state) = settings_window_runtime_state().lock() {
        state.ownership.owner_last_position = Some(owner_position);
        state.ownership.relative_offset = Some(relative_offset(owner_position, settings_position));
    }
}
```

- [ ] **Step 4: Route child and owner move events**

Add a single movement helper and call it from `apply_settings_window_lifecycle`:

```rust
fn apply_settings_window_movement<R>(
    app: &tauri::AppHandle<R>,
    moved_label: &str,
    position: PhysicalPosition<i32>,
) where
    R: tauri::Runtime,
{
    let offset = {
        let Ok(mut state) = settings_window_runtime_state().lock() else { return; };
        let Some(owner_label) = state.ownership.owner_label.clone() else { return; };

        if moved_label == SETTINGS_WINDOW_LABEL {
            let Some(owner_position) = state.ownership.owner_last_position else { return; };
            state.ownership.relative_offset = Some(relative_offset(owner_position, position));
            return;
        }
        if moved_label != owner_label {
            return;
        }

        state.ownership.owner_last_position = Some(position);
        state.ownership.relative_offset
    };

    let Some(offset) = offset else { return; };
    let Some(settings) = app.get_webview_window(SETTINGS_WINDOW_LABEL) else { return; };
    if !settings.is_visible().unwrap_or(false) { return; }
    let _move_result = settings.set_position(position_from_offset(position, offset));
}

pub(crate) fn apply_settings_window_lifecycle<R>(
    app: &tauri::AppHandle<R>,
    window: &tauri::Window<R>,
    event: &tauri::WindowEvent,
) where
    R: tauri::Runtime,
{
    if let tauri::WindowEvent::Moved(position) = event {
        apply_settings_window_movement(app, window.label(), *position);
    }

    if is_settings_window_label(window.label()) {
        match event {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                if let Some(settings_window) = app.get_webview_window(SETTINGS_WINDOW_LABEL) {
                    request_settings_window_hide(&settings_window);
                }
            }
            tauri::WindowEvent::Destroyed => reset_settings_window_runtime_state(),
            _ => {}
        }
        return;
    }

    if matches!(event, tauri::WindowEvent::Destroyed) {
        close_settings_window_if_no_user_windows(app, window.label());
    }
}
```

Call `initialize_settings_window_position(&owner_window, &window)` immediately
after `builder.build()` and before reveal/fallback scheduling.

- [ ] **Step 5: Run focused Rust verification and commit**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml windows::settings_ownership::tests -- --nocapture
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml windows::tests -- --nocapture
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
```

Expected: ownership, geometry, lifecycle, and all pre-existing Settings tests pass.

Commit:

```bash
git add apps/desktop/src-tauri/src/windows.rs apps/desktop/src-tauri/src/windows/settings_ownership.rs
git commit -m "feat: follow settings owner movement"
```

---

### Task 4: Shut down Settings safely before destroying its owner

**Files:**
- Modify: `apps/desktop/src-tauri/src/windows.rs:1035-1185, 1422-1455`
- Modify: `apps/desktop/src-tauri/src/desktop_runtime.rs:407-416`
- Modify: `apps/desktop/src-tauri/src/builder_boundary_tests.rs:132-141`
- Modify: `apps/desktop/src/runtime/tauri/window.ts:241-249`
- Test: `apps/desktop/src/runtime/tauri/window.test.ts:67-83`

**Interfaces:**
- Consumes: the existing Settings hide request/acknowledge/cancel/complete protocol
- Produces: Tauri command `destroy_current_editor_window` and unchanged TypeScript API `destroyNativeWindow(): Promise<unknown>`

- [ ] **Step 1: Write failing Rust state and command-surface tests**

Add tests proving pending owner destruction is scoped and command registration is exact:

```rust
#[test]
fn pending_owner_destroy_is_scoped_to_the_settings_owner() {
    let mut ownership = SettingsWindowOwnership {
        owner_label: Some("main".to_string()),
        ..SettingsWindowOwnership::default()
    };

    assert!(ownership.begin_owner_destroy("main"));
    assert!(ownership.begin_owner_destroy("main"));
    assert!(!ownership.begin_owner_destroy("markra-editor-2"));
    assert_eq!(ownership.take_pending_owner_destroy(), Some("main".to_string()));
    assert_eq!(ownership.take_pending_owner_destroy(), None);
}
```

Add `destroy_current_editor_window` to `DESKTOP_COMMANDS` in `builder_boundary_tests.rs` before running the test, but do not register the production command yet.

- [ ] **Step 2: Run the focused Rust tests and verify they fail**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml pending_owner_destroy_is_scoped_to_the_settings_owner -- --nocapture
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml builder_boundary_desktop_preserves_the_complete_command_surface -- --nocapture
```

Expected: the ownership methods are missing and the boundary test reports that the runtime omitted `destroy_current_editor_window`.

- [ ] **Step 3: Implement pending-owner state transitions**

Add these methods to `SettingsWindowOwnership`:

```rust
impl SettingsWindowOwnership {
    pub(crate) fn begin_owner_destroy(&mut self, label: &str) -> bool {
        if self.owner_label.as_deref() != Some(label) {
            return false;
        }
        if self.pending_owner_destroy.as_deref() == Some(label) {
            return true;
        }
        if self.pending_owner_destroy.is_some() {
            return false;
        }
        self.pending_owner_destroy = Some(label.to_string());
        true
    }

    pub(crate) fn cancel_owner_destroy(&mut self) {
        self.pending_owner_destroy = None;
    }

    pub(crate) fn take_pending_owner_destroy(&mut self) -> Option<String> {
        self.pending_owner_destroy.take()
    }
}
```

Implement the native command in `windows.rs`:

```rust
#[tauri::command]
pub(crate) fn destroy_current_editor_window(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<(), String> {
    if !is_editor_window_label(window.label()) {
        return window.destroy().map_err(|error| error.to_string());
    }

    let owned_settings = app.get_webview_window(SETTINGS_WINDOW_LABEL)
        .filter(|settings| settings.is_visible().unwrap_or(false));
    let should_wait = if owned_settings.is_some() {
        settings_window_runtime_state()
            .lock()
            .map_err(|_| "Settings window state is unavailable.".to_string())?
            .ownership
            .begin_owner_destroy(window.label())
    } else {
        false
    };

    if should_wait {
        request_settings_window_hide(&owned_settings.expect("visible Settings should exist"));
        return Ok(());
    }

    window.destroy().map_err(|error| error.to_string())
}
```

Add `finish_pending_owner_destroy(app)` after both normal hide completion and
the unresponsive-frontend fallback:

```rust
fn finish_pending_owner_destroy<R>(app: &tauri::AppHandle<R>)
where
    R: tauri::Runtime,
{
    let pending = settings_window_runtime_state()
        .lock()
        .ok()
        .and_then(|mut state| state.ownership.take_pending_owner_destroy());
    if let Some(owner) = pending.and_then(|label| app.get_webview_window(&label)) {
        let _ = owner.destroy();
    }
}

fn finish_settings_window_hide<R>(window: &tauri::WebviewWindow<R>, generation: usize)
where
    R: tauri::Runtime,
{
    let should_hide = settings_window_runtime_state()
        .lock()
        .ok()
        .is_some_and(|mut state| complete_settings_window_hide_request(&mut state, generation));
    if should_hide {
        hide_settings_window_instance(window);
        finish_pending_owner_destroy(&window.app_handle());
    }
}

fn spawn_settings_window_hide_fallback<R>(window: tauri::WebviewWindow<R>, generation: usize)
where
    R: tauri::Runtime,
{
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(SETTINGS_WINDOW_HIDE_FALLBACK_MS));
        let should_hide = settings_window_runtime_state()
            .lock()
            .ok()
            .is_some_and(|mut state| complete_settings_window_hide_fallback(&mut state, generation));
        if should_hide {
            hide_settings_window_instance(&window);
            finish_pending_owner_destroy(&window.app_handle());
        }
    });
}
```

Clear the pending owner only when cancellation matched the active generation:

```rust
#[tauri::command]
pub(crate) fn cancel_settings_window_hide(window: tauri::WebviewWindow, generation: usize) {
    if !is_settings_window_label(window.label()) {
        return;
    }
    let Ok(mut state) = settings_window_runtime_state().lock() else {
        return;
    };
    if cancel_settings_window_hide_generation(&mut state, generation) {
        state.ownership.cancel_owner_destroy();
    }
}
```

Register `crate::windows::destroy_current_editor_window` in `desktop_runtime.rs`.

- [ ] **Step 4: Write the failing TypeScript bridge test**

Replace the old direct-destroy expectation in `window.test.ts`:

```ts
it("coordinates editor destruction through the native settings-owner command", async () => {
  mockedInvoke.mockResolvedValue(undefined);

  await destroyNativeWindow();

  expect(mockedInvoke).toHaveBeenCalledWith("destroy_current_editor_window");
  expect(mockedGetCurrentWindow).not.toHaveBeenCalled();
});
```

Run:

```bash
pnpm --filter @markra/desktop exec vitest run src/runtime/tauri/window.test.ts
```

Expected: the new test fails because `destroyNativeWindow` still calls the JavaScript window handle's `destroy()` method.

- [ ] **Step 5: Route the bridge through the native command**

Change only the desktop implementation while keeping the exported API stable:

```ts
export async function destroyNativeWindow() {
  if (!("__TAURI_INTERNALS__" in window)) {
    window.close();
    return;
  }

  await invokeNative("destroy_current_editor_window");
}
```

Run:

```bash
pnpm --filter @markra/desktop exec vitest run src/runtime/tauri/window.test.ts
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml pending_owner_destroy_is_scoped_to_the_settings_owner -- --nocapture
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml builder_boundary_desktop_preserves_the_complete_command_surface -- --nocapture
```

Expected: the focused TypeScript and Rust tests pass.

- [ ] **Step 6: Run task verification and commit**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml windows::tests -- --nocapture
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml builder_boundary_desktop_preserves_the_complete_command_surface -- --nocapture
pnpm --filter @markra/desktop exec vitest run src/runtime/tauri/window.test.ts
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
```

Expected: all commands exit 0.

Commit:

```bash
git add apps/desktop/src-tauri/src/windows.rs apps/desktop/src-tauri/src/windows/settings_ownership.rs apps/desktop/src-tauri/src/desktop_runtime.rs apps/desktop/src-tauri/src/builder_boundary_tests.rs apps/desktop/src/runtime/tauri/window.ts apps/desktop/src/runtime/tauri/window.test.ts
git commit -m "fix: close settings before destroying its owner"
```

---

### Task 5: Preserve platform controls and complete acceptance verification

**Files:**
- Verify: `packages/app/src/components/MacWindowControls.tsx`
- Verify: `packages/app/src/components/WindowsWindowControls.tsx`
- Verify: `packages/app/src/components/SettingsWindow.tsx`
- Verify: `packages/app/src/App.test.tsx`
- Verify: all files changed in Tasks 1-4

**Interfaces:**
- Consumes: completed native parent ownership, movement following, and safe owner destruction
- Produces: verified macOS/Windows/Linux chrome contract and repository acceptance evidence

- [ ] **Step 1: Run existing close-control regression tests**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/components/MacWindowControls.test.tsx src/components/WindowsWindowControls.test.tsx src/App.test.tsx
```

Expected: tests pass, including Settings rendering macOS traffic lights, Windows self-drawn controls, and Linux's close affordance.

- [ ] **Step 2: Run the complete repository gates**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
pnpm test
pnpm typecheck:test
pnpm build
git diff --check 2b35474f..HEAD
```

Expected: every command exits 0 and no whitespace error is reported.

- [ ] **Step 3: Verify the real macOS runtime**

Run:

```bash
pnpm tauri dev
```

In the running QingYu app, verify this exact sequence:

1. Open Settings from `main`; it is centered over `main` with the existing red/yellow/green controls.
2. Edit in `main` while Settings remains visible; the editor is not blocked.
3. Drag Settings to a new relative location, then move `main`; Settings preserves that offset.
4. Minimize `main`; Settings does not remain as an unrelated foreground window.
5. Close Settings with the red control; Settings hides and QingYu remains running.
6. Reopen Settings; the same owner relationship is retained.
7. Open a secondary editor, hide Settings, then open Settings from the secondary editor; the hidden instance is recreated under the secondary owner.
8. Close the owning editor with Settings visible; the Settings sync-exit handshake completes and neither window survives independently.

Expected: all eight behaviors match the confirmed design.

- [ ] **Step 4: Record final clean state**

Run:

```bash
git status --short
git log -6 --oneline
```

Expected: the user-owned `?? macos-icon.icns` and
`?? docs/superpowers/plans/2026-07-23-settings-appearance-refresh-stability.md`
remain untracked; the design, plan, and four implementation commits are
present.
