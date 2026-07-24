# QingYu Project-Scoped Sync Configuration Design

## Goal

Replace the application-global note-sync configuration with a project-scoped configuration stored inside each opened folder. A folder without a QingYu project configuration does not sync. A configured folder owns its provider, server, credentials, remote path, and trigger policy, so switching folders cannot accidentally reuse another folder's remote target.

The same project sync transfers Markdown, images, attachments, and other ordinary files. Images are local project assets rather than a separate WebDAV, S3, or PicGo upload workflow.

## Product Model

QingYu keeps two settings scopes:

- Application settings remain global. This includes appearance, theme, layout, editor behavior, keyboard shortcuts, and other preferences that are independent of an opened folder.
- Project settings belong to the current folder. Note sync and the provider connection are project settings and are read from the folder when it is opened.

Opening a single Markdown file is not opening a project. Single-file editing never infers a sync root from the file's parent directory and never starts note-folder sync.

## Scope

- Add a dedicated desktop project-configuration service.
- Store project sync configuration in `<project>/.qingyu/config.json`.
- Store generated sync manifests and status below `<project>/.qingyu/sync/`.
- Move WebDAV and S3 note-sync connection fields into the existing Sync settings page.
- Make the project configuration the only source of truth for note sync.
- Start one visible sync after opening a valid, enabled project.
- Persist each settings field change immediately, while deferring automatic application and synchronization until the user leaves Sync settings or closes Settings.
- Keep manual, sync-after-save, and scheduled triggers as project policies.
- Store all project credentials as plaintext in `config.json`, as explicitly chosen for this design.
- Replace independent remote image upload with local `assets/` files that participate in folder sync.
- Keep the existing two-way synchronization, deletion propagation, checkpointing, and conflict-copy semantics.

## Non-Goals

- Encrypting credentials or integrating an operating-system keychain.
- Automatically adding QingYu paths to `.gitignore` or `.git/info/exclude`.
- Migrating application-global sync or image-upload settings into a project.
- Migrating, reading, moving, or deleting `.markra-sync` manifests.
- Migrating existing remote image-upload configuration.
- Providing remote note-folder sync in the web runtime.
- Completing the broader product rebrand, such as application identifiers, package names, all UI copy, or every existing legacy-named control file.
- Synchronizing empty directories or symbolic links.
- Adding providers beyond WebDAV and S3 for note-folder sync.

## Project Layout

An initialized project uses this layout:

```text
project/
├── notes.md
├── assets/
└── .qingyu/
    ├── config.json
    └── sync/
        ├── webdav-manifest.json
        ├── s3-manifest.json
        └── status.json
```

`config.json` is user-owned configuration. Files below `.qingyu/sync/` are generated state. The application excludes the entire `.qingyu/` directory from the file tree, workspace search, file watching, AI workspace reads, local backup, and both directions of remote sync.

The legacy `.markra-sync/` directory remains excluded from all of those operations. QingYu does not read, migrate, rename, or remove it. Consequently, the first QingYu sync establishes a new baseline and follows first-sync conflict behavior.

## Configuration Schema

Version 1 uses the following shape:

```json
{
  "version": 1,
  "sync": {
    "enabled": true,
    "provider": "webdav",
    "remotePath": "notes/personal",
    "autoSyncOnSave": true,
    "intervalMinutes": 10,
    "webdav": {
      "serverUrl": "https://dav.example.com/files",
      "username": "user",
      "password": "plain-text-password"
    },
    "s3": {
      "endpointUrl": "",
      "region": "",
      "bucket": "",
      "accessKeyId": "",
      "secretAccessKey": ""
    }
  }
}
```

The configuration retains both provider branches so switching the active provider does not discard the inactive provider's fields. Only the active provider is validated for synchronization.

Common normalization rules are:

- `version` must equal `1`. A higher version is read-only and cannot sync.
- `provider` is `webdav` or `s3`.
- `remotePath` is a non-root, safe relative remote path and cannot contain parent segments.
- `intervalMinutes` is an integer from `0` through `1440`; `0` disables scheduled sync.
- WebDAV requires a valid HTTP or HTTPS server URL and a remote path. Username and password remain optional so anonymous and provider-specific WebDAV deployments continue to work.
- S3 requires endpoint, region, bucket, access key ID, secret access key, and remote path.
- Unknown fields are ignored when reading and are not intentionally generated when QingYu rewrites the file.

`assets/` is fixed and therefore is not a configuration field. The existing global image filename pattern continues to determine generated asset names. The configurable image folder, `copyExternalFilesToStorage` switch, and independent remote image provider no longer affect editor image handling and are removed from the active UI.

The service distinguishes three states:

- absent: no project configuration exists, so sync is disabled;
- readable but not sync-ready: the version and JSON structure are valid, but the enabled provider is missing required connection fields;
- sync-ready: the enabled provider has every field required for a network operation.

A readable but incomplete configuration remains editable and is never treated as permission to connect. Malformed JSON and unsupported versions are configuration errors rather than incomplete configuration.

## Generated Sync Status

`status.json` stores non-secret project status separately from user configuration. It contains a format version, provider, last attempt time, last successful sync time, completion state, and the existing transfer summary. A persisted error may contain the provider, operation, HTTP status, and relative path, but never credentials, authorization headers, or signed URLs.

Manifests retain their existing purpose: they bind file hashes and remote identities to a non-secret target fingerprint and drive upload, download, deletion, and conflict decisions. Changing provider, server, bucket, or remote path changes the fingerprint and starts a fresh baseline for that target.

## Architecture

### Native Project Configuration Service

A dedicated Tauri-side module owns project configuration. It exposes narrow commands equivalent to:

- load the configuration for a canonical project root;
- create the default configuration when sync is first enabled;
- apply a normalized field patch with an expected revision;
- validate and return an immutable sync snapshot;
- reset a damaged configuration after explicit confirmation;
- load non-secret project sync status.

The service only accesses `.qingyu/config.json` beneath the supplied canonical project root. It rejects a symbolic-link `.qingyu` directory or `config.json`, parent traversal, a non-directory root, and any resolved path outside the root.

Writes use a temporary sibling file, flush the complete JSON, and atomically replace the destination. A failed write leaves the previous complete configuration intact. Revisions prevent a stale Settings window from overwriting a newer project configuration.

### Frontend Project Configuration Adapter

The shared application layer exposes typed project-config operations through the runtime boundary. The desktop adapter invokes the native service. Remote project sync remains explicitly unavailable in the web runtime.

The main application owns the active project's applied snapshot and sync coordinator. The Settings window owns only its editing session and displayed values. Internal configuration events carry the canonical project root and revision so an event for project A cannot update project B.

### Synchronization Engine

The WebDAV and S3 backends continue using the shared remote-sync engine. The engine receives an immutable validated snapshot and a canonical folder root. It does not infer a folder from a single-file path and does not load application-global provider settings.

Local and remote filtering both treat `.qingyu` and `.markra-sync` as protected control segments. A remote object below either path is ignored and left untouched rather than downloaded or deleted.

## Project Open and Switch Flow

Opening a folder follows this order:

1. Canonicalize and install the new project root.
2. Clear the previous project's cached configuration, timers, credentials, and status from the active UI state.
3. Load `.qingyu/config.json` through the project configuration service.
4. If the file is absent, expose an unconfigured, disabled Sync page without creating anything.
5. If the file is invalid or unsupported, block all synchronization and show the exact safe validation error.
6. If it is valid but disabled, populate Settings without synchronizing.
7. If it is valid and enabled, install its immutable snapshot, configure its save and interval triggers, and visibly run one synchronization.

An in-progress sync for project A keeps A's root and immutable snapshot if the user opens project B. It may finish safely, but it cannot retarget B. Existing process-wide serialization prevents A and B from mutating remote state concurrently; B's open sync starts after the active run completes.

Opening a single file clears project-sync context. It never reads a neighboring `.qingyu/config.json`, never treats the parent directory as a sync source, and never starts a project timer.

## Settings Experience

The existing Sync settings page becomes the single UI for project connection and policy fields:

- current-project sync switch;
- WebDAV or S3 provider selector;
- provider-specific server and credential fields;
- remote folder;
- sync after save;
- interval in minutes;
- read-only connection test;
- immediate sync;
- last project sync status.

When no folder is open, the page explains that a project folder must be opened and disables project-sync controls.

For a folder without configuration, turning on sync creates `.qingyu/config.json` with `enabled: true`, WebDAV selected, empty connection fields, and disabled automatic triggers. The enabled-but-incomplete state is valid as editable configuration but cannot perform network work.

Every field change is normalized and atomically written immediately. Entering the Sync category starts an editing session with an applied snapshot, persisted revision, and dirty flag. While that session is active:

- sync-after-save and scheduled triggers are suspended;
- persisted field changes do not retarget an existing run;
- no automatic sync begins merely because the last required field became valid.

Leaving the Sync category or closing Settings ends the session. If nothing changed, no sync runs. If values changed, QingYu reloads and validates the final persisted configuration. A valid, enabled configuration becomes the applied snapshot and starts one visible sync. A disabled configuration stops timers without syncing. An invalid configuration leaves project sync blocked and shows its validation error; the previous snapshot is not allowed to resume automatic work.

The explicit Immediate Sync action is an exception to the editing suspension. It validates and applies the currently persisted values and runs them only when valid. It never silently falls back to the previous project's or previous revision's credentials.

## Sync Triggers and Notifications

Supported triggers are:

- opening a valid enabled project;
- leaving a modified Sync settings session;
- closing Settings after modifying Sync settings;
- explicit Immediate Sync;
- saving a Markdown document when `autoSyncOnSave` is enabled;
- the configured project interval.

Equivalent pending triggers for the same project are coalesced. Different project runs remain serialized. A trigger always records the project root, configuration revision, provider target fingerprint, and source before execution.

Notification policy is:

- project-open and Settings-exit syncs show running, success summary, or detailed failure;
- explicit manual sync shows running and result;
- successful save and interval syncs remain quiet;
- every failure is visible regardless of trigger;
- messages may name provider, operation, HTTP status, and relative path but never a secret.

## Image and Attachment Behavior

There is no independent remote image-upload pipeline after this change. WebDAV, S3, and PicGo image destinations, public base URLs, and image-upload paths are removed from the active UI and editor flow. Existing obsolete values may remain in the application settings file but are not read for image handling or project sync.

For an enabled sync project:

- clipboard images are written below the project-root `assets/` directory;
- dropped or imported existing images are copied into project-root `assets/`;
- images already inside `assets/` may be referenced without making a second copy;
- filename collisions use the existing unique-name behavior;
- Markdown receives a path relative to the document, including `../` segments when a nested document references the root asset directory;
- image and attachment resolution uses the canonical project root as its allowed local boundary, so nested notes can safely reference the root `assets/` directory without gaining access outside the project;
- imported attachments follow the same project-local copy principle so they can participate in folder sync.

For a folder without enabled project sync, or for a single-file editing session:

- an existing local image or attachment is referenced by its local file URL instead of being uploaded or copied;
- a clipboard image has no source path, so it is written to an `assets/` directory beside the saved Markdown document and referenced relatively;
- an unsaved document must be saved before clipboard bytes can be persisted.

All ordinary files already below the project root participate in directory sync, including Markdown, images, attachments, and other text or binary content. Empty folders and symbolic links remain unsupported. Fixed build and dependency directories continue to be ignored.

## Connection Testing

Connection tests move into Sync settings and are read-only:

- WebDAV uses a bounded `PROPFIND` against the configured collection or nearest existing parent.
- S3 uses a signed, bounded ListObjectsV2 request under the configured prefix.

Connection testing never uploads a one-pixel image or creates a remote test object. It validates only the active provider and reports safe request context without credentials.

## Failure Handling and Recovery

- Missing configuration means disabled sync, not an error.
- Incomplete configuration remains editable but cannot make a network request.
- Malformed JSON is never overwritten automatically. Settings offers Open Configuration and Reset Configuration actions.
- Reset requires confirmation and preserves the damaged file under a uniquely named local backup before writing defaults.
- A future schema version is read-only; QingYu refuses to downgrade or rewrite it.
- Removing `config.json` while a project is open disables future triggers once the service next reloads it.
- Each sync trigger obtains a fresh validated service snapshot. Settings editing suspension is the only time automatic triggers intentionally wait for an apply boundary.
- Remote and local mutations continue checkpointing the manifest after each successful action, so a later network failure resumes from completed work.
- Atomic download, identity preconditions, deletion safety, and remote conflict-copy behavior remain unchanged.

The UI prominently states that credentials are plaintext, `.qingyu/` is excluded from QingYu sync and backup, and hidden files are not automatically ignored by Git. QingYu does not place passwords, secret keys, authorization headers, or signed request material in logs, diagnostics, status, errors, or fingerprints.

## Local Backup

Local backup excludes the complete `.qingyu/` directory, including plaintext configuration and generated sync state. This matches the existing exclusion of `.markra-sync` and prevents copying credentials or stale deletion baselines into a backup target. Notes, `assets/`, attachments, and other ordinary project content remain eligible for backup.

## Legacy Behavior

No migration occurs:

- Existing global `syncSettings` are ignored by project note sync.
- Existing global WebDAV, S3, PicGo, public URL, remote image destination, and upload-path values are not imported. The non-secret image filename pattern remains a global editor preference.
- Existing `.markra-sync` manifests are ignored but left on disk.
- Existing users must enable and configure sync separately in every project.
- The first QingYu run for a project uses first-sync semantics. Local-only files upload, remote-only files download, and same-path files on both sides without a QingYu manifest preserve the local original and create a remote conflict copy.

## Testing

### Project Configuration Service

Tests cover:

- absent configuration returning a disabled unconfigured result without creating files;
- first enable creating a versioned configuration;
- normalization and round trips for both provider branches;
- immediate per-field atomic writes and revision conflicts;
- interrupted writes preserving the previous complete JSON;
- malformed JSON and future versions remaining untouched;
- confirmed reset preserving a uniquely named damaged copy;
- canonical root enforcement, traversal rejection, and symbolic-link rejection;
- validation errors and diagnostics never containing secrets.

### Application Integration

Tests cover:

- enabled project open running exactly one visible sync;
- absent, disabled, invalid, and single-file contexts making no sync request;
- A-to-B project switching clearing all paths, fields, secrets, timers, and status;
- an in-flight A run retaining A's immutable root and snapshot after B opens;
- Settings loading and writing only the canonical current project;
- every field edit persisting without starting automatic sync;
- save and interval triggers being suspended during editing;
- unchanged Settings exit doing nothing;
- modified valid Settings exit applying and syncing once;
- modified disabled or invalid Settings exit making no request;
- explicit Immediate Sync validating the current persisted revision;
- trigger coalescing and cross-window serialization;
- all notification policies and secret redaction.

### File Scope and Assets

Tests cover:

- `.qingyu` and `.markra-sync` exclusion from local scan, remote listing, file tree, search, watcher, AI workspace reads, and backup;
- remote `.qingyu` objects being ignored and left untouched;
- sync-project clipboard, dropped, and imported images landing in root `assets/` with relative Markdown paths;
- nested project notes resolving root `assets/` within the project boundary while rejecting paths outside the project;
- synchronized attachment imports becoming project-local;
- unsynchronized folders and single files referencing existing local files directly;
- unsynchronized clipboard images landing beside the Markdown document under `assets/`;
- notes, images, attachments, and other regular files retaining the full upload, download, delete, conflict, and checkpoint action matrix;
- old `.markra-sync` data never being read, moved, or removed.

Obsolete WebDAV, S3, and PicGo image-upload behavior tests are removed rather than replaced with tests that only prove deleted features are absent. Existing reusable image filename and local-file behavior tests remain where applicable.

### Verification

Implementation verification runs:

```text
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
pnpm test
pnpm typecheck:test
pnpm build
```

When the configured real MinIO environment is available, verification also runs `pnpm test:s3-sync:live`. WebDAV acceptance is run against an available real test server. Privacy and user-facing documentation are updated to describe plaintext per-project credentials, project-scoped sync, control-directory exclusions, and removal of independent remote image upload.

## Success Criteria

- Opening an unconfigured folder or a single file cannot synchronize anything.
- Two folders can use different providers, servers, accounts, and remote paths without sharing state.
- Opening a configured enabled folder starts exactly one safe, visible synchronization.
- Settings always reflects and writes the current project's file rather than global or stale workspace state.
- Editing settings cannot trigger a half-configured automatic request.
- No QingYu-controlled remote or backup operation copies `.qingyu/config.json`.
- Project images and attachments remain local, portable project files and synchronize through the same folder engine as notes.
- Existing WebDAV and S3 conflict, deletion, checkpoint, and concurrency guarantees remain intact.
