# Settings Cloud Notebook Dialog Ownership Design

## Status

Approved for documentation on 2026-07-24. The user selected the settings-owned dialog approach.

## Problem

Choosing **Select Cloud Notebook** from Sync settings currently ends the Sync editing session, asks the primary editor window to render the cloud notebook dialog, and then explicitly hides the Settings window. This makes an action that appears local to Settings unexpectedly close Settings.

The desired behavior is:

- Settings remains visible throughout the cloud notebook flow;
- the cloud notebook dialog is rendered above and visually belongs to Settings;
- cancelling returns to the same Sync settings page;
- a failed catalog load or restore remains retryable in the dialog;
- a successful restore closes only the dialog, keeps Settings open, and refreshes the displayed workspace and Sync state.

## Selected Approach

Render the existing `RemoteNotebookDialog` inside `SettingsWindow`. Keep the primary editor window as the sole owner of notebook restoration and workspace switching.

The Settings window owns only presentation and catalog-session state:

- dialog visibility;
- loading, error, and catalog entries;
- selected catalog revision;
- retry and cancellation;
- resuming its Sync editing session after the dialog closes.

The primary editor continues to own:

- flushing active documents;
- preparing the desktop notebook target;
- bootstrap synchronization;
- committing the restored notebook as the primary workspace;
- rejecting stale or concurrent restore requests.

This preserves the existing transaction boundary while correcting the window ownership of the UI.

## Alternatives Considered

### Keep the dialog in the primary window and stop hiding Settings

This is the smallest code change, but Settings is a native child of the editor window. A React overlay in the parent editor can remain behind the child window, producing unreliable focus and stacking behavior. It does not reliably satisfy the requested interaction.

### Create a separate native catalog window

A dedicated native window could be placed above Settings, but it would add window creation, positioning, ownership, focus, and teardown behavior for a dialog that already has a reusable React implementation.

## Interaction Flow

1. The user selects **Select Cloud Notebook** in Sync settings.
2. Settings waits for pending Sync configuration writes, ends the editing session with the existing `catalog-handoff` reason, and reloads the authoritative configuration revision.
3. Settings opens `RemoteNotebookDialog` immediately in a loading state and lists remote notebooks for that revision.
4. Refresh repeats the bounded catalog request without hiding Settings.
5. Cancel closes the dialog and starts a fresh Sync editing session on the same page.
6. Restore sends a correlated request containing the selected notebook name and catalog revision to the primary editor.
7. The primary editor validates the revision and runs its existing desktop restore coordinator.
8. A correlated completion response resolves or rejects the dialog operation.
9. On success, Settings closes the dialog and starts a fresh Sync editing session using the updated workspace context. On failure, it keeps the dialog open and shows the existing safe restore error.

Only one catalog or restore request may be active for a Settings window at a time.

## Cross-Window Contract

Add a narrow request/response event contract for restoration rather than moving notebook-switching logic into Settings.

The request contains:

- a generated request identifier;
- the selected remote notebook name;
- the catalog revision.

The completion contains:

- the same request identifier;
- whether restoration succeeded.

The response does not expose provider errors, credentials, filesystem internals, or raw exception text. Settings maps failure to the existing localized safe error. Listeners are registered before a request is emitted, ignore unrelated identifiers, and are cleaned up after completion, cancellation, unmounting, or timeout.

Only the primary desktop workspace owner handles restore requests. Other editor windows and the Settings window ignore them.

## Error and Lifecycle Handling

- If ending the Sync editing session fails, Settings stays unchanged and displays the existing Sync exit failure toast.
- If authoritative configuration reload or catalog listing fails, the dialog stays open with its retry action.
- If the catalog revision becomes stale, restoration fails safely and the dialog remains retryable after refresh.
- If the primary editor is unavailable or the response times out, restoration fails without closing Settings.
- While the dialog is open, its modal overlay blocks interaction with the Settings content and contains keyboard focus.
- Closing Settings through native window controls still follows the existing hide handshake. An already-ended catalog handoff session makes that shutdown idempotent.
- Successful restoration relies on the existing primary-workspace and Sync configuration events to refresh Settings; no duplicate workspace state is introduced.

## Scope

This change covers the established desktop flow launched from Sync settings.

It does not change:

- Welcome's **Restore from cloud** flow;
- true-mobile notebook selection;
- catalog layout, copy, or styling;
- S3 or WebDAV catalog semantics;
- bootstrap synchronization or conflict behavior;
- ordinary Settings close behavior.

## Testing

Focused regression coverage will prove:

- selecting a cloud notebook opens the dialog in `SettingsWindow` and never requests `hideSettingsWindow`;
- catalog loading, refresh, and safe failure states remain available;
- cancelling keeps the Sync category visible and resumes its editing session;
- a restore request is correlated to exactly one primary-window response;
- stale, failed, and timed-out restores keep the dialog open and retryable;
- a successful restore closes only the dialog and leaves Settings mounted;
- the primary editor remains the only surface that invokes the existing notebook restore coordinator;
- existing Welcome, desktop-main, and mobile catalog flows continue to pass.

Verification will use the focused Settings, event-contract, dialog, and App tests, followed by `pnpm typecheck:test`, `pnpm test`, and `pnpm build` when the focused checks pass.
