# QingYu Primary Notes Workspace Design

## Status

Approved for implementation on 2026-07-20. The user authorized the recommended solution to proceed without another design confirmation.

This design supersedes the project-scoped configuration described by `2026-07-16-qingyu-project-sync-config-design.md` and the project-local MCP policy described by `2026-07-20-qingyu-mcp-project-local-no-keychain-design.md`. QingYu has no released-user migration requirement: old `.qingyu/` or `.markra-sync/` data is not read, moved, rewritten, or deleted.

## Goal

Treat QingYu as a professional notes application with one primary notes workspace per device, while preserving the ability to edit standalone Markdown files and external folders as an ordinary editor.

Only the selected primary notes workspace participates in WebDAV or S3 synchronization. Application configuration belongs to the application-data directory, never to a notes folder. Portable application preferences synchronize with the notes workspace through a separate remote namespace; device paths, credentials, runtime state, manifests, and MCP runtime data remain local.

## Product Model

QingYu owns one primary notes workspace on each device:

- desktop users select an existing or new filesystem directory;
- true iOS/Android runtimes use the existing application-managed persistent `workspace/` directory;
- the main editor window represents the primary notes workspace;
- standalone files and external folders remain supported, but they are external editing contexts and never become the synchronization root;
- opening or focusing external content cannot enable, disable, retarget, or reconfigure synchronization or MCP.

The selected notes workspace contains only user content. QingYu does not write configuration, credentials, manifests, status, or runtime files into it.

## Native First-Use Experience

### Suitability Decision

The approved visual direction is appropriate for the product when implemented as application UI rather than responsive website UI:

- desktop uses a quiet identity rail and a task-oriented setup surface below the real native title bar;
- true mobile uses a single-column task flow with safe-area padding and one bottom primary action;
- runtime form factor chooses product semantics; viewport width only changes layout;
- a narrow desktop window still offers desktop directory selection and external editing actions;
- true mobile always uses the application-managed workspace and never displays a fake arbitrary-directory picker.

The identity statement is the first visual layer on both platforms:

> 明窗净几，字字轻语。

The interface follows the existing QingYu application design tokens and system font stack. It does not add decorative brand colors, gradients, marketing navigation, fake device chrome, or card-heavy landing-page structure.

### Desktop

When no primary notes directory exists and onboarding has not been completed, the main window displays the welcome surface:

- left identity rail: QingYu wordmark, the slogan, and the principles “普通 Markdown · 本地优先 · 同步由你决定”;
- main task: choose the primary notes directory;
- explanation: QingYu does not move existing files or write application configuration into the selected directory;
- external actions: open one Markdown file, open an external folder, or defer setup.

Choosing a directory validates it, stores its canonical path in `local-state.json`, marks onboarding complete, and opens it in the main window. It does not enable synchronization.

Deferring marks onboarding complete without inventing a notes directory. QingYu opens as a plain editor, synchronization and MCP document tools remain unavailable, and Settings continues to offer primary-directory selection.

Opening a standalone file or external folder from onboarding does not complete directory selection. The external content opens in an independent editor window and never replaces the main window's primary-workspace responsibility.

### True Mobile

True mobile displays the slogan first, explains the application-managed local workspace, and provides one action: “创建并开始”. There is no step counter, arbitrary folder picker, external-folder action, or meaningless skip action.

Activating the action creates or resolves the persistent app-data `workspace/`, marks onboarding complete, and opens the Compact editor. Synchronization remains disabled until configured.

### Later Launches and Recovery

- A valid desktop primary path opens automatically in the main window.
- A missing, moved, unreadable, or no-longer-directory desktop path opens a recovery version of the welcome surface. QingYu never substitutes a recent external folder.
- True mobile resolves only its app-managed workspace.
- An operating-system file-open request opens the target as external content and does not replace the primary workspace.
- Resetting onboarding from Settings shows the welcome surface on the next launch without deleting notes or configuration.

## Window and Context Responsibilities

The main window owns:

- the primary notes workspace;
- primary file tree and document restoration;
- note synchronization triggers and status;
- the MCP document workspace capability.

External editor windows own only their opened standalone file or external folder. They may use normal editor settings, but they cannot:

- change the primary notes path implicitly;
- load or display a different synchronization configuration;
- run note synchronization for their directory;
- retarget MCP when focused;
- write external-directory state into the primary-workspace restore record.

Settings is application-scoped. It always displays the configured primary notes workspace and the single application sync configuration, regardless of which editor window opened Settings.

## Application-Data Layout

The Tauri application-data directory uses this layout:

```text
QingYu app data/
├── settings.json
├── local-state.json
├── sync-config.json
├── sync-state/
│   ├── notes-webdav-manifest.json
│   ├── notes-s3-manifest.json
│   ├── settings-webdav-manifest.json
│   ├── settings-s3-manifest.json
│   ├── status.json
│   └── conflicts/
├── mcp-runtime/
├── themes/            # future, not synchronized in v1
├── extensions/        # future, not synchronized in v1
└── workspace/         # true-mobile notes workspace only
```

### `settings.json`: portable and synchronized

`settings.json` contains settings intended to follow the user between devices:

- language, appearance mode, light/dark themes, and custom theme CSS;
- editor typography, layout, view, Markdown shortcuts, templates, and other portable editor preferences;
- file ignore rules and portable export presentation preferences;
- MCP enabled state, permissions, confirmation/dry-run/deletion policy, limits, and audit policy.

Absolute executable paths such as the local Pandoc path are not portable and move to `local-state.json`.

### `local-state.json`: device-local and never synchronized

`local-state.json` contains:

- schema version;
- onboarding completion;
- the canonical desktop primary notes path, or `null` on true mobile;
- window, tab, draft, active-document, file-tree, and restore state;
- recent external files and folders;
- per-workspace UI sort state;
- device-local executable paths and comparable machine-specific preferences.

### `sync-config.json`: device-local and never synchronized

`sync-config.json` is the only source of truth for synchronization configuration. Version 1 is equivalent to:

```json
{
  "version": 1,
  "enabled": false,
  "provider": "webdav",
  "remoteRoot": "qingyu",
  "autoSyncOnSave": false,
  "intervalMinutes": 0,
  "webdav": {
    "serverUrl": "",
    "username": "",
    "password": ""
  },
  "s3": {
    "endpointUrl": "",
    "region": "",
    "bucket": "",
    "accessKeyId": "",
    "secretAccessKey": ""
  }
}
```

Credentials are intentionally plaintext. The file is private application configuration, excluded from QingYu synchronization, settings export, logs, status, diagnostics, and MCP settings reads unless the dedicated credential permission allows access.

Fields persist immediately and atomically. Entering Sync settings begins one editing session. Automatic save and interval triggers pause during the session. Leaving the category or closing Settings applies the final persisted configuration and starts synchronization only when values changed and the final state is enabled and valid.

### `sync-state/` and `mcp-runtime/`: local generated state

`sync-state/` owns manifests, safe status, durable download staging, and quarantined remote-settings conflicts. `mcp-runtime/` owns socket or named-pipe state, process-only identifier material, and local audit storage. Neither directory synchronizes.

Atomic publication has one narrow exception to the no-workspace-state rule. Application data and the notes workspace may be on different filesystems, so a validated download cannot be renamed atomically from `sync-state/` into the notes workspace. The engine therefore copies the validated bytes into a short-lived, protected publication file beside the destination, fsyncs it, and atomically renames it into place. These `.markra-sync-stage-*` files are never configuration or durable runtime state: scanners, watchers, manifests, uploads, and downloads exclude them, and every run removes its own failed or stale publication files without following symbolic links.

## Primary Workspace Selection and Switching

Changing the primary notes directory is a switch, never a move or migration:

1. validate and canonicalize the selected directory;
2. stop installing new triggers for the old workspace;
3. persist the new path atomically in `local-state.json`;
4. clear primary restore state that points outside the new root;
5. load the new root in the main window;
6. keep synchronization disabled or enabled according to the single application sync configuration;
7. if enabled and valid, run one visible synchronization after the Settings editing boundary closes.

An already-running old-root synchronization keeps its immutable old root and configuration snapshot until it finishes. It cannot retarget the new workspace. New runs remain process-serialized.

## Remote Layout and Synchronization Scope

One provider/account and one configured remote root use two namespaces:

```text
<remoteRoot>/
├── notes/**
└── app/
    └── settings.json
```

For WebDAV these are child collections. For S3 they are key prefixes. `remoteRoot` must be a safe non-root relative path with no parent segments.

Every synchronization run uses two scopes through the same conflict/checkpoint engine:

1. notes scope: local primary notes root ↔ `<remoteRoot>/notes/`;
2. application-settings scope: local app-data `settings.json` only ↔ `<remoteRoot>/app/settings.json`.

The engine accepts separate source, state, remote-prefix, include-policy, conflict-location, and optional content-validation inputs. This prevents duplicate synchronization algorithms:

- notes manifests live in `sync-state/`, not the notes root;
- settings scope includes exactly `settings.json` and cannot see `local-state.json`, `sync-config.json`, `sync-state/`, `mcp-runtime/`, themes, extensions, or the mobile workspace;
- downloaded settings must pass the current portable-settings schema before atomic publication;
- an invalid remote settings file is quarantined below `sync-state/conflicts/`, leaves current local settings unchanged, and makes the run visibly fail;
- a valid changed settings file reloads the application settings store and emits the existing settings-change events;
- a settings conflict preserves local `settings.json` and stores the remote version below `sync-state/conflicts/` for recovery.

Existing note upload, download, deletion propagation, first-sync behavior, remote conflict copies, identity checks, atomic publication, checkpointing, trigger coalescing, and process-wide serialization remain in force.

The notes manifest is also bound to the canonical primary-root identity. Switching the desktop primary directory clears the notes baseline before the first run against the new root. This prevents the old root's manifest from interpreting files absent in the new root as remote deletions. The settings manifest is independent and does not reset when the notes path changes.

## Protected Paths and Notes Contents

The primary notes workspace synchronizes all ordinary files, including Markdown, images, attachments, and other text or binary content. Empty directories and symbolic links remain unsupported.

The following segments remain ignored in both local and remote scans even though QingYu no longer creates them in a notes workspace:

- `.qingyu`;
- `.markra-sync`.

This is a security exclusion for stale plaintext configuration. Existing directories are left untouched and are not migrated or deleted.

Normal build/dependency and user-configured ignore rules continue to apply.

## Images and Attachments

There is no independent image-upload provider.

- Documents below the primary notes workspace always copy imported, dropped, pasted, and clipboard resources into the root-level lowercase `assets/` directory, whether synchronization is currently enabled or disabled.
- Markdown paths are relative to the document and may contain `../` for nested notes.
- Files already inside primary `assets/` are not copied twice.
- Standalone files and external folders reference existing local resources at their filesystem locations.
- Clipboard bytes for an external saved Markdown document go into an `assets/` directory beside that document.
- Unsaved external documents must be saved before clipboard bytes can be persisted.

Only primary-workspace resources participate in synchronization.

## Synchronization Triggers and Notifications

Supported triggers are:

- application launch after a valid enabled primary workspace is installed;
- changing the primary workspace and leaving Settings;
- leaving a modified Sync settings session;
- explicit Immediate Sync;
- saving a primary-workspace Markdown document when enabled;
- the configured interval;

External document saves never trigger synchronization. Content physically below the primary root remains part of that root's next scan even when a second window opened it directly; content outside the primary root never enters the sync scope. Each trigger captures an immutable primary root, sync-config revision, target fingerprint, and source.

Manual, launch, workspace-switch, and Settings-exit runs show progress and result. Successful save and interval runs remain quiet; every failure is visible. Notifications may contain provider, safe operation, HTTP status, and relative path, but never credentials, authorization material, or signed URLs.

## Settings Experience

Desktop Settings gains an application-level “笔记库” category containing:

- current primary notes directory;
- choose/change action;
- explicit explanation that switching does not move files;
- recovery state when the path is unavailable.

Sync settings:

- always edits `sync-config.json`;
- displays the primary notes directory as a read-only target;
- disables synchronization controls when no valid primary workspace exists;
- labels `remoteRoot` as the remote root and explains the `notes/` and `app/settings.json` namespaces;
- keeps WebDAV/S3 fields, connection test, immediate sync, save trigger, interval, and safe status;
- states that credentials are plaintext application-local data and do not synchronize.

True-mobile Storage continues to display the managed workspace read-only. Mobile Sync uses the same application sync config and backend, not a project file.

## MCP

MCP is application-level:

- its complete policy is stored under `mcp` in `settings.json` and therefore synchronizes;
- its local IPC transport, process key, identifiers, socket/pipe state, and audit files remain under `mcp-runtime/` and never synchronize;
- the enabled state applies across application launches and synchronized devices;
- the only document workspace capability is the configured primary notes workspace;
- without a valid primary workspace, MCP document tools fail closed even if MCP is enabled;
- external windows and focus changes never activate, clear, or replace the MCP workspace;
- switching the primary workspace invalidates previous workspace/document handles and installs the new capability;
- app-settings and sync tools continue to require their existing dedicated permissions.

The existing local-only no-token IPC model remains. No TCP listener, bearer token, Keychain dependency, per-project policy, or project-owned MCP file remains.

## No Migration or Compatibility

This release intentionally does not migrate or interpret:

- `<folder>/.qingyu/config.json`;
- `<folder>/.qingyu/sync/`;
- `<folder>/.qingyu/mcp.json`;
- `<folder>/.markra-sync/`;
- prior application-global image-upload configuration.

No compatibility fallback may make an opened external folder a sync root. Old control directories remain excluded so their credentials cannot be uploaded accidentally.

## Error and Recovery Behavior

- No primary workspace: sync and MCP document access remain unavailable, not erroneous.
- Invalid primary path: recovery welcome surface; no recent-folder fallback.
- Incomplete sync config: editable, but cannot connect.
- Malformed or unsupported sync config: automatic sync blocked; Settings offers reset after explicit confirmation and preserves a local damaged copy below app data.
- Invalid downloaded settings: quarantine and visible failure; current settings remain active.
- Missing remote settings: upload the local portable settings file.
- Remote-only valid settings on first sync: download and apply it.
- Primary root switch during a run: old immutable run may finish; no retargeting.
- Missing primary root during a trigger: fail closed before network mutation.

## Testing

### Local State and Onboarding

- local state persists independently from `settings.json`;
- first desktop launch, selection, defer, later selection, restart restore, missing-root recovery, and root switching;
- true-mobile managed workspace flow and absence of arbitrary directory actions;
- narrow desktop retains desktop semantics;
- external file/folder openings do not change primary root or onboarding state.

### Sync Configuration and Engine

- app-data `sync-config.json` normalization, revision checks, atomic writes, malformed reset, plaintext credential containment, and no notes-root writes;
- note manifests/status under app-data `sync-state/`;
- exact remote namespaces for WebDAV and S3;
- settings allowlist and schema validation;
- settings reload/events after valid download;
- invalid/conflicting settings quarantine;
- note and settings checkpoint, conflict, deletion, and target-change behavior;
- `.qingyu` and `.markra-sync` remain ignored in both directions.

### Application Integration

- only primary-root launch/save/interval/manual/Settings-exit triggers run;
- external windows and paths cannot run or retarget sync;
- root switch installs immutable new context;
- settings fields persist immediately but automatic sync begins only on the editing exit boundary;
- desktop and Compact settings operate on the same app config;
- primary-root assets use root `assets/`; external resources follow local-file behavior.

### MCP

- policy round-trips through `settings.json` and contains no runtime secret;
- enabled state and policy reload through settings events;
- external focus does not change authority;
- primary root change invalidates old handles;
- no root fails closed;
- no `.qingyu/mcp.json` is created;
- IPC and audit runtime data remain local.

### Verification

Run:

```text
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
pnpm test
pnpm typecheck:test
pnpm build
```

When the configured MinIO service is available, run `pnpm test:s3-sync:live`. Then run the actual desktop application with `pnpm tauri dev`, complete onboarding with a sync-disabled primary folder, verify external file/folder isolation, configure a separate S3-backed primary folder, and verify notes, `assets/`, other content, and portable settings across two local workspace fixtures. True-mobile behavior receives runtime/component tests and available Android/iOS build verification.

## Success Criteria

- The notes workspace contains no QingYu configuration or generated state.
- One device has exactly one primary notes workspace and one sync configuration.
- Files and folders outside the primary root never synchronize, and external editor contexts never retarget synchronization or MCP.
- Desktop and true-mobile onboarding use appropriate native semantics.
- The slogan “明窗净几，字字轻语。” is the first visual layer without introducing a marketing-page structure.
- WebDAV and S3 synchronize primary notes content under `notes/` and portable `settings.json` under `app/`.
- `local-state.json`, `sync-config.json`, sync state, credentials, and MCP runtime data never leave the device.
- All settings continue saving immediately; automatic synchronization waits for the appropriate Settings exit boundary.
- Existing synchronization safety, deletion, conflict, checkpoint, and parallel-test guarantees remain intact.
