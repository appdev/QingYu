# QingYu Compact Runtime Fixes Design

Date: 2026-07-18

## Context

Real-runtime Compact acceptance found two issues after the main mobile adaptation:

1. A desktop Tauri window narrowed into Compact mode can have no active workspace root. The runtime supports project sync, but the Compact sync controller then has neither a candidate root nor a load result, so the status screen remains in its loading state forever.
2. Full-screen Compact pages visually cover the editor while deliberately keeping it mounted, but the covered editor remains exposed to keyboard focus and the accessibility tree.

These repairs must preserve the approved mobile workspace model, existing desktop behavior, editor state, and sync engine semantics.

## Sync State Without a Workspace Root

Compact sync status distinguishes three different situations:

- Runtime does not support project sync: keep the existing `Sync unavailable` state.
- Runtime supports project sync but has no candidate workspace root: show a new explicit `No workspace available` state.
- A candidate workspace root exists but its configuration has not completed loading: keep the loading state.

The no-workspace state explains that a workspace folder must be opened before sync can be configured. It exposes only Back. It must not show Configure Sync, Sync Now, reset, clear-data, folder-picker, or cloud-deletion actions.

True-mobile startup remains unchanged. The managed-workspace gate prevents the user from reaching Compact pages until a fixed root is ready, so this new state primarily makes desktop-width simulation and defensive fallback behavior finite and accurate. It must not create an extra managed root or modify the desktop workspace.

## Full-Screen Page Isolation

When the Compact navigation page is not `editor`, the editor layer stays mounted to preserve Milkdown state, selection, content, and scroll position. The layer is marked both:

- `inert`, to prevent focus, pointer, and sequential keyboard interaction;
- `aria-hidden="true"`, to remove covered editor controls from the accessibility tree.

When navigation returns to `editor`, both attributes are removed immediately. The visible full-screen page remains the only interactive and accessible page. Managed-workspace loading/error pages already omit the editor layer and require no additional treatment.

## Testing

Test-first coverage must prove:

- available sync plus a null candidate root renders the new no-workspace state, not loading;
- the no-workspace state has no Configure Sync or Sync Now action and invokes no sync/session method;
- a real candidate root with a null load result still renders loading;
- Files, Settings, Move, and Sync overlays keep the editor mounted but mark its layer inert and aria-hidden;
- returning to the editor removes both attributes and restores accessibility;
- desktop wide mode, true-mobile managed-workspace gating, sync semantics, and existing Compact navigation tests remain unchanged.

After automated verification, rebuild the isolated `QingYu Compact QA.app`, shrink its native Tauri window to Compact width, and repeat the sync and accessibility checks.

## Non-Goals

- No change to WebDAV/S3 engines, conflict handling, autosave, or sync triggers.
- No managed workspace for desktop-width simulation.
- No folder picker, workspace switcher, reset, clear-data, or cloud-delete action.
- No editor remount when opening or closing a Compact full-screen page.
- No unrelated visual redesign.
