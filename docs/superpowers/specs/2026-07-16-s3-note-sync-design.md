# S3 Note Sync Design

## Goal

Add S3-compatible note-folder synchronization as a peer to the existing WebDAV `SyncProvider`. S3 must support the same two-way upload, download, deletion propagation, conflict preservation, manual sync, sync-after-save, and scheduled sync behavior.

## Scope

- Extend `SyncProvider` from `"webdav"` to `"webdav" | "s3"`.
- Keep remote note sync available only in the desktop runtime.
- Reuse the S3 endpoint, region, bucket, access key ID, and secret access key already configured under Storage.
- Continue storing the note sync object prefix in `SyncSettings.remotePath`; do not reuse the image upload path.
- Support the S3-compatible endpoint addressing already supported by image upload, including generic path-style endpoints and providers that require virtual-hosted-style buckets.
- Do not add the AWS SDK or another production dependency.

## Non-Goals

- Adding new S3 authentication fields such as session tokens, profiles, role assumption, or environment-based credential discovery.
- Adding remote sync to the web runtime.
- Changing which local files are included in note-folder synchronization.
- Creating placeholder objects for empty local directories.
- Adding additional sync providers beyond WebDAV and S3.

## User Experience

The Sync settings provider selector will offer WebDAV and S3. The selected provider controls which connection settings are read from Storage. The Sync page continues to own only the enable switch, provider, remote folder, save-after-sync switch, schedule, last-sync time, and manual run action.

When S3 is selected, the remote folder is an object-key prefix. For example, a remote folder of `notes/personal` maps the local file `daily/2026-07-16.md` to the S3 object key `notes/personal/daily/2026-07-16.md`.

Provider-specific validation errors will direct the user to configure the corresponding Storage connection. Manual sync, save-after-sync, scheduled sync, status display, and shortcut behavior remain unchanged.

## Architecture

The current `remote_sync.rs` implementation will become a focused module tree:

- `remote_sync/backend.rs` defines the crate-private asynchronous `RemoteSyncBackend` trait and protocol-neutral remote file types.
- `remote_sync/engine.rs` owns local scanning, manifest persistence, the synchronization action matrix, conflict naming, identity checks, checkpointing, and summary accounting.
- `remote_sync/webdav_backend.rs` owns PROPFIND, MKCOL, Basic Auth, WebDAV URL handling, and WebDAV GET, PUT, and DELETE requests.
- `remote_sync/s3_backend.rs` owns ListObjectsV2 pagination and S3 HEAD, GET, PUT, and DELETE requests.
- `remote_sync/mod.rs` owns Tauri request deserialization, provider selection, backend construction, and the single sync command entry point.
- `s3_http.rs` owns reusable S3 endpoint addressing, canonical URI/query construction, SigV4 signing, and signed request construction. Both image upload and S3 note sync use it.

The engine is generic over a concrete backend. It does not use dynamic dispatch, `async-trait`, or a new dependency. WebDAV and S3 expose the same engine-level operations while keeping protocol-specific request logic isolated.

The frontend runtime uses a discriminated `SyncNativeMarkdownFolderInput` union. Each branch contains `sourcePath` and exactly one provider configuration. The native runtime sends one provider-tagged request to the unified Tauri command.

## Backend Contract

`RemoteSyncBackend` provides operations equivalent to:

- identify the non-secret remote target for manifest binding and diagnostics;
- list all remote files below the configured remote root;
- download one remote file while validating its expected identity;
- upload one local file while validating the expected previous remote identity;
- delete one remote file while validating its expected identity.

Remote files expose a normalized relative path, a stable identity string, and a byte length. Backend implementations must reject paths that escape the configured root or cannot be represented as safe local relative paths.

S3 remote identity combines normalized ETag, content length, and last-modified time. The S3 backend performs a fresh HEAD identity check before mutating an existing object. Downloads use the expected ETag where supported. Upload responses are followed by HEAD when they do not return sufficient identity metadata.

## S3 Protocol

The S3 backend uses SigV4-authenticated HTTP requests through the existing `reqwest` and network proxy configuration.

- ListObjectsV2 uses `list-type=2`, the configured prefix, and continuation-token pagination.
- Object keys are URL encoded without changing their logical slash-separated hierarchy.
- Canonical query parameters are percent encoded and sorted before signing.
- PUT signs the actual SHA-256 payload hash.
- GET, HEAD, DELETE, and list requests sign the appropriate empty-payload hash.
- Responses with non-success HTTP status codes produce provider-, method-, and relative-path-specific diagnostics.
- List responses skip the prefix root itself and keys ending in `/`, which represent empty-directory markers.
- Returned keys must begin with the exact configured prefix boundary. Decoded relative segments may not be empty control segments, `.` or `..`.

The shared S3 HTTP module preserves current path-style and virtual-hosted-style behavior for image uploads. Existing image upload tests remain regression coverage for that extraction.

## Data Flow

1. The user selects a provider and configures the remote folder in Sync.
2. `runMarkdownSync` validates the selected provider and reads its existing Storage connection.
3. The desktop runtime serializes a provider-tagged request including network proxy settings.
4. Rust validates the local source root and provider configuration, constructs the concrete backend, and calls the generic engine.
5. The engine scans local files and loads the provider manifest.
6. The backend lists remote files and returns normalized protocol-neutral entries.
7. The engine plans and executes each action in stable relative-path order.
8. Successful mutations update the manifest checkpoint immediately.
9. The engine returns the existing unified counts for scanned, skipped, uploaded, downloaded, conflict files, and transferred bytes.
10. The frontend records `lastSyncAt` and emits the existing cross-window sync-settings update event.

## Manifest Safety and Migration

Provider state remains separate:

- `.markra-sync/webdav-manifest.json`
- `.markra-sync/s3-manifest.json`

Each manifest records a schema version, a non-secret target fingerprint, and its entries. The fingerprint covers:

- WebDAV: normalized server URL and remote path.
- S3: normalized endpoint, region, bucket, and object prefix.

When the fingerprint changes, old entries are not reused. The next run follows first-sync behavior, preventing stale state from propagating deletion into a different remote target.

The legacy WebDAV manifest format is accepted. Its entries are associated with the currently configured WebDAV target on first post-upgrade use and then saved in the versioned format. No user reconfiguration is required.

## Synchronization Semantics

The common engine uses one action matrix for both providers:

- Local only without a manifest entry: upload.
- Remote only without a manifest entry: download.
- Both sides present without a manifest entry: preserve as a conflict rather than overwrite either side.
- Local changed and remote unchanged: upload.
- Remote changed and local unchanged: download.
- Both changed: keep the local file and download the remote version beside it as `name.remote-conflict-<UTC timestamp>.ext`.
- Local deleted and remote unchanged: delete remote.
- Remote deleted and local unchanged: delete local.
- One side deleted while the remaining side changed: preserve the changed side rather than propagating deletion.

The engine continues synchronizing every regular file below the source root except files inside the existing fixed ignored directories: `.git`, `.markra-sync`, `build`, `dist`, `node_modules`, and `target`.

## Reliability and Error Handling

- Downloads write to a temporary sibling file and atomically replace the destination only after a complete successful transfer.
- Local reads, local deletes, remote downloads, remote uploads, and remote deletes verify the identity that was used when the action was planned.
- Each successful mutating action persists a manifest checkpoint so a later network failure resumes from completed work.
- A failed action stops the current sync and leaves completed actions checkpointed.
- Partial temporary downloads are removed on failure.
- Logs and errors may include provider, operation, relative path, endpoint host, bucket, and HTTP status. They must not include passwords, secret access keys, authorization headers, or signed query values.
- Configuration validation happens before remote listing. An empty remote path, the remote root itself, invalid bucket, missing credential, invalid endpoint, or unsafe object prefix fails without changing local or remote files.

## Frontend and Runtime Changes

- Normalize and persist `"s3"` as a valid `SyncProvider`.
- Add S3 to the Sync provider selector and provider-specific explanatory copy.
- Pass both WebDAV and S3 Storage settings into the sync orchestration hook, selecting only the active provider at runtime.
- Replace the WebDAV-only missing-configuration result with provider-specific validation outcomes.
- Extend the desktop runtime request types and Tauri invocation to the provider-tagged union.
- Keep the web runtime failure explicit: remote sync requires the desktop runtime.
- Keep the existing summary shape, toasts, last-sync state, manual shortcut, save trigger, and timer behavior.

## Testing

TypeScript tests will cover:

- normalization and persistence of the S3 provider;
- Sync settings selection between WebDAV and S3;
- provider-specific configuration validation;
- `runMarkdownSync` dispatching a discriminated S3 request with the existing Storage credentials and Sync remote path;
- desktop runtime serialization of S3 requests and network settings;
- manual, save-after, and scheduled sync continuing to use the common orchestration path;
- WebDAV request behavior remaining unchanged.

Rust tests will cover:

- the generic engine's complete action matrix using a deterministic in-memory fake backend;
- manifest target binding, legacy WebDAV migration, and per-action checkpointing;
- safe relative-path validation and atomic download behavior;
- S3 endpoint addressing for generic, AWS-style, and currently supported compatible providers;
- SigV4 canonical requests for PUT, GET, HEAD, DELETE, and ListObjectsV2 query strings;
- ListObjectsV2 XML parsing, prefix stripping, directory-marker skipping, and continuation pagination;
- provider-specific request/status diagnostics without secret values;
- existing WebDAV planner and protocol behavior after extraction;
- existing S3 image upload behavior after sharing `s3_http.rs`.

Verification will run focused TypeScript and Rust tests during TDD, followed by the full workspace test suite, TypeScript builds, Rust tests, and a desktop production build. A debug Tauri package build will be used when practical.
