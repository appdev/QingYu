# Settings Appearance Refresh Stability Design

## Status

Approach A was approved on 2026-07-23. This written specification is awaiting
final review before implementation planning.

## Problem

Opening the Appearance category mounts `AppearanceSettings`, whose mount effect
refreshes the theme catalog. The refresh temporarily marks the catalog as
loading, which makes `useAppTheme().ready` false. `SettingsWindow` currently
uses that live readiness value as a render gate and returns `null`, unmounting
the Appearance category. When the refresh completes, the category mounts again
and starts another refresh. The resulting loop alternates the complete Settings
UI with a blank window and repeatedly calls `list_themes` and
`mark_settings_window_ready`.

The initial readiness gate is useful: a newly created Settings window should
not be revealed before its language and theme are ready. The defect is that the
same gate remains reversible after the window has already reached that initial
ready state.

## Goals

- Keep the Settings window visible after it has completed its first successful
  language and theme initialization.
- Preserve the automatic catalog refresh when the Appearance category mounts.
- Preserve manual refresh, import, replacement, deletion, third-party theme
  activation, and startup theme protection.
- Stop repeated `list_themes` and `mark_settings_window_ready` calls.
- Add regression coverage for the real category-switching path.

## Non-goals

- Do not redesign the Appearance settings UI or theme catalog.
- Do not change theme selection, approval, activation, or persistence rules.
- Do not remove the initial Settings startup readiness gate.
- Do not alter workspace-window startup behavior.
- Do not change the native Settings window lifecycle or visibility commands.

## Alternatives Considered

### Latch Settings startup readiness — selected

Treat language/theme readiness as a one-way startup gate inside
`SettingsWindow`. Before the first ready render, the window may return `null`.
After both dependencies have been ready once, later catalog refreshes or theme
reactivations cannot blank the entire Settings surface.

This preserves the intended automatic refresh and matches the semantic role of
`mark_settings_window_ready`: it describes completion of startup, not every
later transient refresh.

### Remove the Appearance mount refresh

Deleting the mount effect would stop the loop with a smaller diff, but a
prewarmed Settings window would no longer discover themes changed externally
until the user pressed Refresh. This changes existing behavior and is rejected.

### Move refresh orchestration into SettingsWindow

The parent could track category transitions and refresh only once per entry.
That would prevent the infinite loop, but the current reversible render gate
could still produce a blank frame during the refresh. It also moves catalog
behavior into a component that should only coordinate the Settings shell.

## State Model

`SettingsWindow` will derive the current readiness condition from
`appLanguage.ready && appTheme.ready`, then latch successful startup readiness
for the lifetime of that Settings React tree.

- Before the latch is set, the existing `null` render remains valid.
- The first render where both inputs are ready sets the latch.
- Once set, the latch never returns to false during that mount.
- The native ready notification depends on the latched value, so it is not
  reissued for later theme refreshes.
- Closing and destroying the Settings webview naturally resets the latch on the
  next independent mount.

The latch is local to the Settings surface. `useAppTheme.ready` remains an
accurate live signal for other consumers and does not need a global semantic
change.

## Testing

Add an application integration test for an independent Settings route:

1. render the Settings route and wait for initial readiness;
2. switch from General to Appearance;
3. hold the Appearance-triggered catalog refresh pending;
4. assert that the Settings shell and Appearance heading remain mounted;
5. resolve the refresh and assert that the catalog request count stabilizes;
6. assert that the native ready notification is not repeated.

The test must fail against the current implementation because the Settings
window disappears while the refresh is pending. It must pass after the readiness
latch is introduced.

Run the focused application test first, followed by:

- the Appearance and theme-hook Vitest suites;
- `pnpm typecheck:test`;
- `pnpm build`;
- `git diff --check`.

Finally, rebuild and run the real desktop application. Switch repeatedly
between General and Appearance, verify that no blank frames occur, and confirm
that the runtime log contains one bounded catalog refresh rather than a command
storm.

## Implementation Boundaries

Expected changes are limited to:

- `packages/app/src/components/SettingsWindow.tsx` for the one-way startup
  readiness latch;
- `packages/app/src/App.test.tsx` for the category-switching regression test.

No native Rust, theme catalog, theme activation, localization, or styling
changes are expected.
