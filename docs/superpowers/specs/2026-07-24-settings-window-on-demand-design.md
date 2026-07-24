# Settings Window On-Demand Creation Design

## Goal

Create the native Settings child window only after an explicit user request.
Starting or reopening QingYu must not create, preload, or reveal a Settings
window.

## Context

The parent-ownership implementation retained the previous Settings prewarm
optimization. After the primary editor finished startup, the frontend waited
600 milliseconds and invoked `prewarm_settings_window`. Native code then built
a hidden `markra-settings` webview and attached it to `main` as a real child
window.

That optimization is no longer appropriate. A parented Settings window has
native ownership and lifecycle semantics even while hidden, so creating it at
startup makes Settings part of application startup and can surface through
platform window restoration or ordering behavior.

## Chosen Approach

Remove Settings prewarming end to end:

- remove the delayed startup effect from the main React application;
- remove `prewarmSettingsWindow` from the shared runtime contract and desktop
  Tauri adapter;
- remove the `prewarm_settings_window` native command and command registration;
- remove native startup-mode branches that exist only to create an idle hidden
  Settings window.

Do not replace prewarming with a longer delay, an additional hide call, or a
platform-specific workaround. Those options still create a native child before
the user requests it and keep the startup lifecycle coupled to Settings.

## Preserved Behavior

An explicit Settings request continues to:

1. create Settings hidden with the invoking editor as its required native
   parent;
2. load the Settings frontend and stored preferences;
3. reveal and focus Settings only after the frontend reports ready, with the
   existing bounded native reveal fallback;
4. reuse the hidden singleton for the same owner;
5. retain owner movement, platform window controls, Settings hide/session
   shutdown, and safe owner-destroy behavior.

After Settings has been opened and hidden, the existing five-minute idle
destroy behavior remains. This is session reuse, not startup prewarming.

## Trade-off

The first explicit Settings open may take slightly longer because its webview
is created on demand. This is preferable to creating a native child window at
every application launch and makes startup behavior deterministic across
macOS, Windows, and Linux.

## Verification

Focused tests must prove that:

- the main application has no delayed Settings prewarm effect;
- the shared and Tauri runtime contracts expose no prewarm method;
- the desktop command surface does not register `prewarm_settings_window`;
- explicit Settings open still creates a hidden, parented window and reveals it
  only after readiness;
- the existing close, reuse, movement, and platform-control tests remain green.

Run the repository gates after focused tests:

```text
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
pnpm test
pnpm typecheck:test
pnpm build
```

Finally, launch an isolated macOS build and confirm that startup shows only the
editor, no Settings webview is created during the initial delay, and clicking
Settings creates and reveals the owned child window.
