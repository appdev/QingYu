# QingYu Established Cloud Notebook Switching Design

## Status

Approved for implementation on 2026-07-22. The user required an established notebook to be able to switch to another cloud notebook and retained the standing authorization to use the recommended design without additional confirmation pauses.

This design extends `2026-07-21-qingyu-named-notebook-switching-design.md`. All existing application-level synchronization, same-name remote identity, remote-first bootstrap, standalone-file, and current-notebook transaction boundaries remain in force. A read-only architecture review tightened the recommended model from an implicit parent to an explicit device-local Workspace root.

## Problem

After desktop notebook `A` becomes current, the synchronization settings can display and synchronize `A`, but they do not expose the existing remote notebook catalog. The desktop catalog dialog is mounted only during onboarding. A user therefore cannot select remote notebook `B` without returning to a first-use surface.

The desktop product needs to store both the Workspace collection and the exact current notebook:

```text
desktopWorkspaceRoot = /Users/ada/Workspace
desktopPath          = /Users/ada/Workspace/A
```

The current notebook must be a distinct direct child of the canonical Workspace collection; the filesystem root itself can never be both Workspace and notebook:

```text
current notebook = /Users/ada/Workspace/A
Workspace        = /Users/ada/Workspace
selected remote  = B
local target     = /Users/ada/Workspace/B
remote target    = <remoteRoot>/notes/B/
```

## Alternatives Considered

### Persist a separate Workspace root — selected

Persist `desktopWorkspaceRoot` beside `desktopPath`. This directly represents the user's `Workspace/A`, `Workspace/B` model, gives recovery a stable authority, and removes the repeated parent picker after initial setup.

Choosing an existing local notebook derives and atomically persists its canonical parent as the new Workspace root. Choosing `/Other/C` therefore changes the local state to Workspace `/Other` with current notebook `C`.

### Derive Workspace only when restoring — rejected

Calculating `parent(A)` only at restore time changes fewer files, but cannot validate a fixed Workspace during reload, recovery, or concurrent local switches. It also leaves future settings surfaces without an authoritative Workspace path.

### Ask for a parent on every restore — rejected

This preserves maximum placement flexibility but makes routine A/B switching repetitive and fails to express the Workspace collection the user expects.

## Desktop Experience

When synchronization configuration is complete and a current desktop notebook exists, the Synchronization settings show a `Restore Notebook from Cloud` action using the existing localized catalog wording.

Selecting the action:

1. waits for pending field saves and ends the synchronization editing session;
2. asks a credential-free native command to focus the primary window and emit a catalog request only to it;
3. hides the settings window through its existing close handshake;
4. lists only the shallow cloud notebook names through the existing catalog API;
5. lets the user select one notebook, such as `B`;
6. creates or reuses the target below the persisted canonical Workspace root;
7. bootstraps only `B` with the existing remote-first synchronization transaction;
8. publishes `B` as current only after bootstrap succeeds.

The catalog action is disabled while configuration changes are unsaved, a network action is running, the provider configuration is incomplete, or no current desktop notebook is available. Its eligibility is based on `configured`, not readiness: disabled global synchronization remains valid for catalog listing and restore as long as the provider configuration itself is complete.

The same remote dialog remains available on first use. First-use restore asks for a Workspace directory once and persists it with the restored current notebook.

## Placement and State

Established restore never appends a duplicate child below the selected notebook:

```text
/Workspace/A + remote B -> /Workspace/B
```

It never produces:

```text
/Workspace/A/B
/Workspace/B/B
```

If `/Workspace/B` does not exist, the existing secure desktop-target preparation creates it. If it exists as a real directory, QingYu reuses it. A file, symlink, replaced directory, invalid name, or unavailable parent fails closed.

`local-state.json` uses primary-workspace version 3 and stores the Workspace root with the exact current desktop path. Legal state shapes are a configured desktop pair, a configured mobile name, or an all-null onboarding/deferred state. A single desktop path without its Workspace root, a single Workspace root without its current path, or mixed desktop/mobile identity is invalid. Version 2 is unsupported and returns to onboarding; the feature is unreleased and no compatibility reader or migration is added. No remote binding, credential, or notebook registry is added.

After a successful switch it stores:

```json
{
  "version": 3,
  "desktopWorkspaceRoot": "/Users/ada/Workspace",
  "desktopPath": "/Users/ada/Workspace/B",
  "managedName": null,
  "onboardingCompleted": true
}
```

The global S3/WebDAV configuration is unchanged. The note scope changes from `notes/A/` to `notes/B/`; portable application settings remain at `app/settings.json`.

## Ownership and Events

The independent settings window may request the catalog but does not perform a notebook switch. It first completes the existing sync-settings editing session so bootstrap cannot overlap an editing lease. A single TypeScript event-contract module owns the event name and listener. A desktop native command then focuses the primary window and emits a request containing no path, notebook name, provider data, revision, or credentials. If the primary window cannot be found, shown, focused, or notified, the command fails and the settings window remains visible.

Only the primary workspace window listens for the request and opens the catalog. The existing generation guards continue to reject catalog responses from an obsolete synchronization revision or a closed dialog.

The primary window remains the only owner of:

- active-document flushing;
- old-notebook synchronization barriers;
- prepared desktop target leases;
- bootstrap synchronization;
- primary-state publication;
- file-tree, watcher, MCP, and recent-notebook updates.

## Transaction and Failure Behavior

Selecting remote `B` while `A` is current uses the existing restore transaction:

1. flush dirty documents in `A`;
2. block new note-sync triggers and drain `A` to an atomic boundary;
3. securely prepare `/Workspace/B`;
4. run remote-first bootstrap for `notes/B/` only;
5. preserve conflicts and validated partial downloads through existing rules;
6. publish `B` as current only after successful bootstrap;
7. release the switch barrier and start normal current-notebook behavior.

The transaction remains mutually exclusive with a concurrent local or remote switch. A competing request fails safely and remains retryable rather than creating a second queue or merging remote-name and local-path semantics.

If catalog loading fails, the current notebook remains `A`. If target preparation or bootstrap fails, `A` remains current and any safe partial `B` directory remains available for retry. Provider errors never change the global configuration or retarget `A`.

## Mobile and Standalone Files

True mobile already exposes local and remote notebooks through the managed notebook dialog and continues to use `app-data/workspaces/<name>/`. This change does not add a desktop filesystem picker to mobile.

Standalone Markdown files remain unsynchronized and cannot open the cloud notebook catalog or affect the persisted Workspace collection.

## Testing

Focused tests must prove:

- Synchronization settings expose the cloud-notebook action only for a complete configuration and current desktop notebook.
- Version-3 desktop state accepts only a canonical Workspace plus one direct current-notebook child; version 2 returns to onboarding.
- The independent settings window ends its editing session, emits one credential-free native request, and then enters the normal hide handshake.
- An established primary window opens the existing catalog outside onboarding and ignores stale or closed responses.
- Restoring `B` from current `/Workspace/A` prepares `/Workspace/B` without opening a parent picker.
- First-use restore still opens the parent picker.
- Existing `/Workspace/B` is reused; target preparation and bootstrap failure leave `A` current.
- Global sync configuration and portable settings scope remain unchanged across A to B.
- Mobile notebook management and standalone-file isolation remain unchanged.

Final verification includes the focused Vitest and Rust suites, `pnpm test`, `pnpm typecheck:test`, `pnpm build`, the default-parallel Rust suite, and a real desktop runtime pass that switches from synchronized A to cloud B under one Workspace collection.

## Non-Goals

- Synchronizing the device-local Workspace collection root.
- Downloading every cloud notebook.
- Renaming, deleting, moving, or deduplicating cloud notebooks.
- Preventing collisions between unrelated same-name local directories.
- Adding cloud notebook operations to MCP.
- Changing mobile managed-workspace storage.
