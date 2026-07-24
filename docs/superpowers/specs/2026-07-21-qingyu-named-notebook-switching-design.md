# QingYu Named Notebook Switching Design

## Status

Approved for implementation on 2026-07-21. The user approved the recommended mobile model and authorized subsequent recommended decisions without additional confirmation pauses.

This design supersedes the external-folder and single-mobile-`workspace/` portions of `2026-07-20-qingyu-primary-notes-workspace-design.md`. The application-settings and global synchronization configuration boundaries from that design remain in force.

QingYu is unreleased for this feature boundary. There is no migration or compatibility reader for the superseded external-folder model, the old mobile `workspace/` layout, or note files stored directly below the old remote `notes/` prefix.

## Goal

Make QingYu a focused notes application with exactly one current notebook directory at a time:

- opening a directory always means switching the current notebook;
- temporary external directories are not supported;
- standalone external Markdown files remain supported and never participate in notebook synchronization;
- WebDAV or S3 configuration is global application configuration;
- switching notebooks changes only the note-content scope of the global synchronization policy;
- each notebook maps to a remote child directory using its directory name alone;
- a new device lists remote notebook directories and synchronizes only the one explicitly selected by the user.

## Product Boundaries

QingYu has one current notebook per device and one global synchronization configuration per application installation.

The global synchronization configuration owns:

- enabled state;
- provider selection;
- WebDAV server and credentials;
- S3 endpoint, region, bucket, and credentials;
- the configured remote root;
- automatic-save, interval, conflict, and trigger policies.

Switching the current notebook never creates, clones, replaces, or edits this configuration. It changes only the local notes root and the directory-name child below the global remote `notes/` namespace.

Application-settings synchronization remains global and independent. Switching notebooks does not stop, reset, or retarget `<remoteRoot>/app/settings.json`.

## Alternatives Considered

### Directory name as identity — selected

The local notebook directory basename is the remote notebook directory name. This is understandable in S3 and WebDAV tools and requires no registry or metadata.

The user accepts responsibility for choosing distinct names. Selecting two unrelated local directories with the same basename intentionally targets the same remote directory and may merge their contents.

### Separate local-to-remote binding — rejected

Persisting an arbitrary remote name for every local directory would allow different local and remote names, but it would reintroduce per-notebook configuration and a binding-management interface.

### Stable UUID and remote registry — rejected

A UUID registry would prevent name collisions and survive renames, but adds remote metadata, discovery state, reconciliation, and recovery behavior that the product does not need.

## Directory Identity

Desktop notebook identity is the basename of the canonical selected filesystem directory.

```text
local:  /Users/ada/Documents/weizhi-note
name:   weizhi-note
remote: <remoteRoot>/notes/weizhi-note/
```

True mobile notebook identity is the selected child name below the application-managed workspace collection.

```text
QingYu app data/
└── workspaces/
    ├── work-notes/
    ├── personal/
    └── weizhi-note/
```

Directory names remain the logical remote names. Provider adapters perform the required S3 key and WebDAV URL encoding without changing the displayed name.

QingYu validates one safe path segment. It rejects empty names, `.` and `..`, names containing a platform separator, and protected names such as `.qingyu` and `.markra-sync`. A remote name that cannot be created on the current operating system remains visible in the catalog but is disabled with a clear explanation.

There is no UUID, hidden notebook configuration, remote registry, or notebook metadata file.

Renaming a notebook root changes its identity. The renamed directory targets a new remote child on the next successful selection. QingYu does not rename or delete the former remote directory.

## Remote Layout

One global provider target uses this layout:

```text
<remoteRoot>/
├── app/
│   └── settings.json
└── notes/
    ├── personal/**
    ├── weizhi-note/**
    └── work-notes/**
```

The notes scope for one run is:

```text
local current notebook <-> <remoteRoot>/notes/<current-directory-name>/
```

Only the current notebook scope is scanned, watched, downloaded, uploaded, or deleted. Other remote notebook directories are touched only by the read-only catalog operation until the user selects one.

Files stored directly below `<remoteRoot>/notes/` are outside the new model. QingYu does not migrate, infer, or synchronize them.

## Remote Notebook Catalog

The global sync service exposes a read-only catalog operation after provider configuration is valid:

- S3 lists common prefixes one level below `<remoteRoot>/notes/`;
- WebDAV lists child collections with a depth-one request;
- the catalog returns decoded directory names only;
- the catalog does not recursively list files, calculate sizes, infer modification dates, or download content;
- protected and malformed children are excluded or returned disabled with a safe reason;
- credentials, signed URLs, object identities, and raw provider errors are not exposed to the UI.

Listing the catalog does not enable synchronization and does not change the current notebook.

The configured remote root is the application's namespace inside the S3 bucket or WebDAV
location; it is not a notebook name. Before the first user-initiated notebook sync from
settings, QingYu lists the children below `<remoteRoot>/notes/` before any upload:

- when the catalog contains notebook directories, the user selects exactly one;
- the current same-name directory remains selectable and starts a remote-first merge in place;
- when the catalog is empty, QingYu immediately selects the current local notebook as the
  default and the first normal sync creates `<remoteRoot>/notes/<current-directory-name>/`;
- a catalog error is never treated as an empty catalog and never triggers an upload.

## Desktop Experience

### First use

The welcome surface offers:

- choose a local notebook directory;
- restore from cloud;
- open a standalone Markdown file;
- defer setup.

It no longer offers an external-folder action.

“Restore from cloud” opens the application-scoped sync setup. After the connection is valid, the user selects exactly one remote notebook directory and then selects a local parent directory. QingYu creates or reuses `<parent>/<remote-name>/`.

### Established use

Every folder entry point uses the label and semantics “Switch Notebook Directory”:

- File menu;
- command palette and shortcuts;
- Notes Directory settings;
- operating-system folder-open requests.

No code path opens an independent external-folder editor window.

Standalone file entry points remain unchanged. A standalone file opens outside notebook ownership, cannot retarget synchronization or MCP, and does not replace the next-launch notebook.

## True Mobile Experience

True mobile owns `app-data/workspaces/` rather than a single fixed `app-data/workspace/`.

- creating a local notebook asks for a notebook name and creates `workspaces/<name>/`;
- restoring a remote notebook creates or reuses `workspaces/<remote-name>/`;
- switching happens through an in-app notebook selector, not an arbitrary filesystem picker;
- only the selected managed child is mounted, watched, and synchronized;
- switching back to a managed child resumes its corresponding same-name remote directory;
- standalone external files remain subject to platform capabilities but never become notebooks.

The current managed notebook name is device-local state. It is not part of portable `settings.json`.

## Local State

`local-state.json` remains the device-local source of truth.

The primary-workspace record distinguishes desktop and true-mobile roots without storing provider configuration:

```json
{
  "version": 2,
  "desktopPath": "/Users/ada/Documents/weizhi-note",
  "managedName": null,
  "onboardingCompleted": true
}
```

or on true mobile:

```json
{
  "version": 2,
  "desktopPath": null,
  "managedName": "weizhi-note",
  "onboardingCompleted": true
}
```

No old version is migrated. Invalid or unsupported state returns to onboarding.

Recent standalone files may remain local history. Recent notebook directories may remain device-local switch history, but only successful primary-notebook transactions can add them and selecting one performs a full notebook switch. External-folder window state is removed from active product behavior.

## Local Notebook Switching Transaction

All folder entry points call one switch coordinator.

1. Validate and canonicalize the requested local directory or managed child.
2. Derive and validate the directory name.
3. Flush the active document and durable editor state.
4. Block new note-sync triggers for the old notebook.
5. Request cancellation and wait for the old note run to reach a safe boundary. Atomic publication is never interrupted halfway.
6. If safe stopping fails, keep the old notebook current and report the failure.
7. Persist the new primary-workspace state atomically.
8. Unmount the old file tree, watchers, restore state, and note-sync trigger source.
9. Mount the new root and install its file tree, watchers, and trigger source.
10. If global sync is enabled and valid, start one visible synchronization for the new same-name remote scope.

Application-settings synchronization is not cancelled by this transaction.

A failure before primary state publication leaves the old notebook active. A provider or network failure after a successful local switch does not roll the switch back: QingYu remains a fully usable local editor and reports note synchronization as pending or failed.

## Restore from Cloud Transaction

Restoring differs from switching to an already-owned local directory because the remote content is the source of the initial local notebook.

Desktop flow:

1. Load the remote notebook catalog.
2. Select one remote directory.
3. Select a local parent directory.
4. Create or reuse `<parent>/<remote-name>/`.
5. Run bootstrap synchronization for that remote directory only.
6. After bootstrap succeeds, atomically make the target the current notebook.

True-mobile flow is identical except the target is `app-data/workspaces/<remote-name>/` and there is no parent picker.

If bootstrap fails, QingYu preserves successfully downloaded files and the target directory for retry but does not make the incomplete target current. A retry reuses the same target and resumes through the normal checkpoint mechanism.

## Bootstrap Synchronization

Bootstrap synchronization is remote-first without silently overwriting either side.

1. List the selected remote directory only.
2. Download remote-only paths into validated staging and publish them atomically.
3. Treat equal local and remote content as unchanged.
4. For a path that differs on both sides without a shared baseline, preserve both through the existing conflict-copy mechanism.
5. After remote hydration and conflict preservation, upload local-only paths.
6. Publish a complete manifest and checkpoint for the bound local and remote scope.

This sequence implements “synchronize existing cloud files first” while retaining ordinary two-way behavior after the initial run.

If the remote same-name directory does not exist when switching to an existing local notebook, QingYu creates it through the first upload. If it exists, bootstrap behavior applies before local-only files upload.

## Sync State Isolation

Note manifests, checkpoints, conflicts, and coalescing state must never leak between notebook directories.

Each local note scope uses an internal local-only scope key derived from:

- provider target fingerprint without credentials;
- configured global remote root;
- remote notebook directory name;
- canonical local root identity.

The key is hashed only to produce a filesystem-safe cache directory under `sync-state/notes/`. It is not a notebook ID, is not displayed, and is never uploaded.

```text
sync-state/
├── notes/
│   ├── <scope-hash-A>/
│   └── <scope-hash-B>/
└── settings/
    └── <global-target-hash>/
```

Switching back to the same canonical local directory and same global target resumes its former manifest. Selecting a different local directory with the same basename receives a separate local baseline even though the user intentionally targets the same remote directory.

Every trigger captures an immutable local root, notebook name, provider configuration revision, and target fingerprint. Completion from an obsolete notebook generation cannot replace the current notebook status.

## External File Behavior

Standalone files remain supported on desktop and where the mobile platform permits.

- opening a file does not change `primaryWorkspace`;
- saving it never triggers note synchronization;
- assets for a saved standalone Markdown file use the adjacent lowercase `assets/` directory;
- closing the standalone file returns focus without changing the current notebook;
- the next normal launch restores the current notebook, not the standalone file as a notebook.

## MCP and Operating-System Requests

MCP remains application-scoped.

- document tools remain rooted in the current notebook;
- the current MCP surface has no arbitrary folder-open capability, and this change does not add one;
- MCP cannot create an external-folder editing context or select a remote notebook;
- existing read/write/switch permission and confirmation policies remain authoritative;
- remote catalog and restore operations are not added to MCP in this change.

An operating-system folder-open event also calls the switch coordinator. A file-open event retains standalone-file semantics.

## Errors and User Feedback

- Catalog failure leaves the current notebook unchanged and creates no local directory.
- Invalid remote names remain visible but disabled when safe to display.
- A target child that already exists is reused; the UI states that same-name local and remote contents will reconcile.
- A local validation, save, or safe-stop failure aborts switching and leaves the old notebook current.
- A sync failure after local switching leaves the new notebook usable and exposes retry.
- A bootstrap failure preserves partial validated downloads and exposes retry without publishing primary ownership.
- Same-name merges use existing conflict copies rather than silent replacement.
- Remote directories are never automatically renamed or deleted when a local notebook is renamed, removed, or switched away from.
- Notifications and logs contain safe directory names and relative paths but never credentials or signed URLs.

## Implementation Boundaries

The implementation should extend existing services rather than introduce a second synchronization engine.

- `usePrimaryWorkspace` owns current desktop path or managed mobile name.
- one switch coordinator owns every directory-switch entry point;
- one notebook-scope resolver derives local root, directory name, remote prefix, and local state key;
- provider adapters expose a shallow remote-directory catalog operation;
- the existing remote-sync engine receives bootstrap or steady-state mode plus the resolved scope;
- the existing application-settings scope remains independent;
- UI components consume catalog and switch services without provider-specific logic.

External-folder-only callbacks, labels, tests, restore state, and window-routing branches are deleted rather than retained as hidden compatibility code. Recent-directory UI may remain only after being redefined as notebook-switch history owned by the switch coordinator.

## Testing

### Unit tests

- directory-name validation and provider-safe encoding;
- remote prefix derivation from Unicode and spaced names;
- S3 common-prefix and WebDAV child-collection catalog parsing;
- catalog operations remain shallow and read-only;
- mobile managed-root resolution below `workspaces/<name>/`;
- local sync-state keys isolate canonical roots with the same basename;
- bootstrap ordering downloads before uploads;
- bootstrap conflicts preserve both sides;
- obsolete switch generations cannot publish current status.

### Application tests

- onboarding has local choose, cloud restore, standalone file, and no external-folder action;
- File and Settings surfaces say “Switch Notebook Directory”;
- A to B switching changes current root but not global sync configuration;
- only B synchronizes after switching, while application settings continue globally;
- switching back to A resumes A's remote directory and local manifest;
- standalone files do not switch or synchronize;
- cloud catalog selection creates only the chosen local directory;
- a failed bootstrap does not publish the target as current;
- mobile creates and switches managed notebook children.

### Native and concurrency tests

- safe-stop acknowledgment gates primary state publication;
- an old note run cannot retarget or overwrite new-notebook status;
- provider/network failure after publication does not roll back a local switch;
- app-settings sync continues across a note switch;
- unsupported remote directory names cannot escape local managed roots.

### Live and runtime verification

- real MinIO contains at least two remote notebook prefixes and confirms only the selected one downloads;
- switch A to B to A and verify isolated remote content and manifests;
- verify a same-name existing remote is pulled before local-only upload;
- verify settings remain at `<remoteRoot>/app/settings.json` throughout switching;
- run the full Rust, frontend test, typecheck, and build gates;
- run the desktop app with clean data, local A, local B, a standalone file, restart restoration, and a remote restore;
- build the true-mobile target and verify managed `workspaces/<name>/` routing.

## Explicit Non-Goals

- stable notebook IDs or UUIDs;
- a remote notebook registry;
- arbitrary local-to-remote naming;
- automatic collision prevention;
- automatic remote rename or deletion;
- synchronizing every remote notebook to a new device;
- temporary external-folder editing;
- migration of old external-folder state, old mobile `workspace/`, or old root-level remote notes;
- per-notebook provider credentials or sync policies.
