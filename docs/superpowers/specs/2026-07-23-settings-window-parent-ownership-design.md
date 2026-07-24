# Settings Window Parent Ownership Design

## Goal

Keep Settings as a native, non-modal desktop window while giving it a real
parent relationship with the editor window that owns it. The editor remains
usable while Settings is open, but Settings must no longer behave like an
unrelated application window.

## User-Visible Behavior

- Opening Settings creates or reveals one application-wide Settings window.
- The Settings window is owned by the editor window that created it.
- Settings initially opens centered over its owner rather than centered on the
  display.
- Moving the owner moves Settings by the same delta. The user may still move
  Settings independently to choose a different relative offset.
- Minimizing, hiding, or destroying the owner also hides or destroys Settings
  according to the platform-owned-window lifecycle.
- Settings stays non-modal: the owner remains editable while Settings is open.
- Reopening Settings from the same owner focuses the existing window.
- Reopening Settings from a different editor while Settings is visible focuses
  the existing window instead of creating competing settings sessions.
- Reopening Settings from a different editor while Settings is hidden recreates
  it under the new owner.

## Chosen Approach

Use Tauri's native `WebviewWindowBuilder::parent` relationship and retain the
existing Settings webview route. This maps to a child window on macOS, an owned
window on Windows, and a transient window on Linux.

Do not replace Settings with an in-page overlay. The Settings surface is large,
scrollable, and includes platform dialogs and long-running configuration flows.
An in-page overlay would obscure the editor and would have no native window
lifecycle.

Do not implement platform-specific modal sheets. Settings is not a blocking
confirmation flow, and Tauri does not expose one uniform modal-sheet contract
across the three desktop platforms.

## Ownership Model

Extend the native Settings runtime state with the owning editor label and the
positions needed to preserve a relative offset:

- `owner_window_label`
- `owner_last_position`
- `settings_relative_offset`

The owner must be an editor window label. A Settings window must never be
created with another Settings window or an unknown auxiliary window as its
parent.

When `open_settings_window` creates Settings, it uses the invoking
`WebviewWindow` as the parent. Parent binding is required. If Tauri rejects the
relationship, the command returns an error instead of silently creating an
independent window.

Settings remains an application-wide singleton. Multiple simultaneous Settings
webviews would allow competing sync-editing sessions and duplicate application
settings writes, so one Settings window per editor is deliberately rejected.

## Prewarm Behavior

> Superseded by
> `docs/superpowers/specs/2026-07-24-settings-window-on-demand-design.md`.
> Settings is now created only after an explicit open request.

Retain Settings prewarming for the primary `main` editor only. The prewarm
command receives the invoking editor window and creates the hidden Settings
window with that window as its parent.

Secondary editor windows do not race to replace the prewarmed owner. If a
secondary editor later opens Settings while the prewarmed window is hidden, the
native layer destroys the unused hidden instance and recreates it under the
secondary editor before revealing it.

Any replacement of a hidden Settings window must occur only after the existing
hide/session shutdown protocol has completed. A visible Settings window is
never destroyed merely to change owners.

## Positioning and Movement

On creation, calculate the Settings position from the owner's outer position
and size and the Settings outer size. Center the child inside the owner and use
the display-centered position only when native geometry cannot be read.

Native parent semantics differ by platform. In particular, a Windows owned
window is not guaranteed to follow owner movement. Handle `WindowEvent::Moved`
for consistent behavior:

1. When Settings moves, recompute and store its offset from the owner.
2. When the owner moves, apply the same position delta to visible Settings.
3. Update the stored owner position after the move.
4. Ignore movement events for unrelated editor windows.

This preserves user-selected relative placement while preventing the two
windows from drifting apart when the owner moves.

## Close, Hide, and Owner Destruction

Keep the existing Settings close-request handshake:

1. prevent the native close;
2. ask the frontend to end the Settings sync session;
3. hide Settings only after the frontend acknowledges completion;
4. keep Settings visible and retryable if session shutdown fails.

Owner-driven shutdown must use the same handshake before a visible Settings
window is destroyed. If the owner is closing, temporarily prevent owner close,
request Settings shutdown, and resume the owner close only after Settings has
hidden successfully. If shutdown fails, cancel the pending owner close and keep
both windows available so the user does not lose a staged sync configuration.

When the owner has already been destroyed or the Settings window is hidden and
idle, clear the ownership state and destroy the Settings instance. Destruction
of an unrelated editor window does not affect Settings.

## Platform Window Controls

Keep the current platform-specific chrome:

- macOS continues to hide the native traffic-light buttons and renders
  `MacWindowControls`.
- Windows continues to use `WindowsWindowControls` with the existing self-drawn
  title bar.
- Linux keeps native window decorations and its existing close affordance.

The close action continues to request Settings hide rather than exiting QingYu.
Establishing native parent ownership does not require a new close-button
component and does not change the visual design shown by each platform.

## Error Handling

- Reject a parent that is missing, already destroyed, or not an editor window.
- Return parent-binding failures through the existing frontend invocation so
  the caller can log or show the failure.
- Do not fall back to an independent Settings window after a binding failure.
- If parent-relative geometry is unavailable, retain parent ownership and fall
  back only for the initial position.
- If a programmatic follow move fails, keep the stored ownership and retry on a
  later owner movement instead of destroying either window.
- Preserve the current hide fallback timer for cases where the Settings
  frontend cannot acknowledge a normal close request.

## Implementation Boundaries

The primary native changes belong in
`apps/desktop/src-tauri/src/windows.rs`. Runtime bridge signatures and focused
tests may change in:

- `apps/desktop/src/runtime/tauri/window.ts`
- `apps/desktop/src/runtime/tauri/window.test.ts`
- `packages/app/src/runtime/index.ts`
- `packages/app/src/App.tsx`
- `packages/app/src/App.test.tsx`

The Settings content hierarchy and platform control components should not be
redesigned for this work.

## Verification

Add focused coverage for:

- accepting only editor labels as Settings owners;
- creating Settings with the invoking editor as native parent;
- limiting prewarm ownership to `main`;
- calculating parent-centered initial position;
- retaining a user-selected relative offset when the owner moves;
- ignoring unrelated editor movement;
- focusing a visible singleton instead of replacing it;
- recreating a hidden singleton under a different owner;
- destroying Settings when its owner is destroyed;
- completing the Settings hide/session handshake before resuming owner close;
- preserving the existing macOS, Windows, and Linux close affordances.

Run the smallest focused Rust and Vitest targets during development, followed by
the repository acceptance gates:

```text
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
pnpm test
pnpm typecheck:test
pnpm build
```

Finally, run the real desktop app on macOS and verify that Settings remains
non-modal, follows the owner, keeps the existing traffic-light controls, hides
cleanly, and cannot outlive its owner.
