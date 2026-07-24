# QingYu Sidebar Sync Status Button Design

## Goal

Add a cloud-sync status button beside the existing Settings launcher in the desktop file-tree footer. Clicking the button must immediately request the existing application-level manual synchronization flow for the authoritative primary notebook.

## Scope

- Desktop primary notebook window only.
- Both expanded and collapsed file-tree presentations.
- Reuse the current global S3/WebDAV configuration and `useAppSyncCoordinator` manual run path.
- Do not show the button in external standalone-file windows or runtimes without application sync.
- Do not change sync configuration, sync engine, notebook switching, or external-file behavior.

## Visual Design

The control is a peer of the existing Settings icon button:

- 28 by 28 pixel ghost icon button using `lucide-react`.
- Same size, rounding, hover, and focus treatment as Settings; Sync additionally provides a localized status tooltip.
- Expanded drawer: Settings and Sync form a compact left-aligned group; the update action remains right-aligned.
- Collapsed drawer: Settings and Sync share one fixed bottom-left group rather than separate floating anchors.
- No new color system. The component consumes the existing QingYu tokens (`--text-secondary`, `--accent`, `--danger`, `--bg-hover`).

### States

| State | Icon treatment | Interaction |
| --- | --- | --- |
| Default/idle | Cloud, secondary ink | Starts manual sync |
| Hover | Existing ghost-button hover surface | Starts manual sync |
| Focus | Existing visible accent focus ring | Starts manual sync with keyboard |
| Active | One-pixel pressed translation | Starts manual sync |
| Disabled/unconfigured | Cloud-off, existing secondary ink | Still invokes the coordinator so its existing localized missing-configuration guidance is shown |
| Loading | Rotating refresh/cloud treatment | Disabled until the current run settles |
| Error | Cloud with small danger mark | Retry starts manual sync |
| Success | Cloud with small accent check | Starts a new manual sync |

The accessible label and tooltip describe the action and current state using existing translations such as “Sync now”, “Syncing…”, “Succeeded”, and “Failed”.

## Data Flow

1. `WorkspaceApp` derives a sidebar state from the current primary root, applied sync-config readiness, `appSync.running`, and scoped `appSync.status`.
2. `WorkspaceApp` passes the state and `runApplicationSyncNow` through `WorkspaceLayout` to `MarkdownFileTreeDrawer` only when the window owns the primary notebook and application sync is available.
3. `MarkdownFileTreeDrawer` renders the reusable `SidebarSyncButton` beside Settings.
4. Clicking calls the existing `appSync.run("manual")`; the coordinator continues to own request coalescing, toast feedback, file-tree refresh, status events, error recovery, and notebook-root validation.

No second synchronization implementation or local loading state is introduced.

## Failure and Concurrency Behavior

- A click with no primary notebook or incomplete configuration follows the coordinator’s existing localized error path.
- While a manual or automatic run is active, the loading state disables the button to prevent accidental repeat clicks. The coordinator remains the authoritative deduplication boundary.
- Failure leaves the button available as an immediate retry action.
- Status remains scoped to the current primary notebook and sync-config revision through the existing coordinator.

## Testing

- Component tests cover all semantic states, accessible labels, icons, disabled/loading behavior, and click handling.
- Drawer tests verify adjacency to Settings in expanded and collapsed layouts and verify that omitting the callback omits the control.
- App integration tests prove the button sends one `manual` request for the current primary notebook and is absent from external standalone windows.
- Focused tests run red before implementation, then green.
- Final verification includes `pnpm test`, `pnpm typecheck:test`, `pnpm build`, and a real desktop launch confirming placement and manual synchronization feedback.

## Hallmark Self-Review

- Philosophy: 5/5 — the control stays quiet and subordinate to the writing surface.
- Hierarchy: 5/5 — it is a peer of Settings, not a new panel or primary action.
- Execution: 4/5 — behavior reuses the existing coordinator and shared controls.
- Specificity: 5/5 — layout, states, ownership, and error behavior are explicit.
- Restraint: 5/5 — no new palette, configuration, toast system, or sync path.
- Variety: 4/5 — status is expressed through icon detail without adding text density.

The design contains no placeholders, compatibility promises, or ambiguous ownership boundaries.
