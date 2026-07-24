# QingYu MCP Design

## Goal

Add a local MCP control surface to QingYu (轻语) so an authenticated MCP client can work with documents, selected application settings, and workspace sync through QingYu's own application services.

MCP is an application API, not a general filesystem API. MCP clients never receive ambient filesystem authority, never choose arbitrary local roots, and never read or write document files outside QingYu's guarded service layer.

## Product Principles

- QingYu owns authentication, authorization, path validation, confirmation, auditing, persistence, history, and synchronization.
- MCP permissions are one global application policy shared by every MCP client, as requested. They are not separate per-client ACLs.
- Authentication remains mandatory even though authorization is global. Possession of the global Bearer token proves that a local process may connect; the current QingYu policy determines what it may do.
- Only directories explicitly authorized on QingYu's MCP settings page are visible through MCP. This authorization is independent of which workspace is currently visible in a QingYu window.
- Clients address workspaces, folders, and documents with application-issued identifiers rather than absolute paths.
- Existing document, project-configuration, and remote-sync behavior remains the source of truth. MCP does not implement a parallel file or sync engine.
- Security invariants are not configurable. User-experience and operational risk policies are configurable.

## Industry Direction and Rationale

Comparable note applications generally expose an application-owned local API and put MCP in front of that API:

- Logseq embeds a local Streamable HTTP MCP endpoint and combines token authentication with application validation, dry-run support, and undo-oriented write behavior.
- Anytype exposes a local HTTP API and uses a stdio MCP adapter for clients that only support stdio.
- Joplin MCP integrations commonly adapt the application's authenticated local Web Clipper API instead of scanning the note directory directly.
- Obsidian integrations demonstrate both the usefulness and the risk of vault-scoped REST/MCP access. A recent encoded-path traversal issue shows that string-prefix checks alone are insufficient even after authentication.

QingYu therefore uses an embedded Streamable HTTP MCP server, an optional forwarding-only stdio bridge, application-issued object handles, and capability-rooted file access. This follows the application-API pattern while avoiding a second privileged filesystem process.

## Scope

- An MCP settings page in QingYu for enabling the service, managing authorized directories, choosing the global permission profile, configuring operation policies, managing the connection token, and viewing audit events.
- A loopback-only Streamable HTTP MCP server embedded in the Tauri Rust backend.
- An optional `qingyu-mcp` stdio bridge for clients without Streamable HTTP support.
- Document discovery, search, read, create, update, rename/move, and delete within MCP-authorized directories.
- Typed reads and updates for an explicit allowlist of application settings.
- Typed reads and updates for per-workspace sync configuration, write-only credential changes, connection testing, manual sync, and sync status.
- Revision preconditions for document and configuration writes.
- Configurable confirmation, dry-run, deletion, sync-trigger, sync-wait, and audit-retention policies.
- Focused security, behavior, and end-to-end tests for both HTTP and stdio entry points.

## Non-Goals

- Giving the MCP server or stdio bridge general filesystem access.
- Exposing a tool that opens, closes, or changes the folder shown in a QingYu window.
- Allowing MCP to add or remove authorized directories, enable or disable MCP, rotate its token, change its permission profile, or weaken its security policy.
- Per-client identities, permissions, tokens, or audit partitions in the first version.
- A remote-network MCP endpoint, LAN listening, TLS termination, wildcard CORS, or cloud relay.
- MCP resources, prompts, or subscriptions in the first version; document reads use tools.
- A generic `settings_get(key)` or `settings_set(key, value)` interface.
- Renaming existing package names, Rust crates, JavaScript packages, bundle identifiers, or event names from `markra`. That migration remains a separate task.
- Mobile or web-runtime MCP support. The first version is a QingYu desktop capability.

## Naming

- Chinese product name: `轻语`.
- English product and MCP display title: `QingYu`.
- MCP server name: `qingyu`.
- Optional stdio bridge executable: `qingyu-mcp`.
- Default endpoint: `http://127.0.0.1:19618/mcp`.

Existing internal `markra` package and identifier names may remain until the separately planned package migration.

## Architecture

```text
Streamable HTTP client ─┐
                        ├─> loopback HTTP MCP server in QingYu
stdio MCP client ─> qingyu-mcp bridge ───────────────────────┘
                                      │
                                      ├─> MCP authentication and global policy
                                      ├─> DocumentService
                                      ├─> SettingsService
                                      └─> SyncService
                                               │
                                               └─> guarded local files and remote providers
```

The Tauri Rust process owns the HTTP listener, MCP sessions, authentication, policy evaluation, opaque-handle validation, audit records, and tool implementations. Tool handlers call the same native services used by Tauri commands. The React frontend is not an RPC hop and is not required to be mounted for a tool call to complete.

The service boundaries are:

- `DocumentService`: workspace tree queries, content reads, atomic writes, history integration, create, rename/move, trash/delete, protected-path filtering, and revision calculation.
- `SettingsService`: typed application settings schema, validation, persistence, revision calculation, and cross-window change notification.
- `SyncService`: typed project sync configuration, credential updates, connection testing, request coalescing, background runs, status, conflicts, and existing S3/WebDAV behavior.
- `McpService`: transport, sessions, authentication, authorization, handles, confirmation/dry-run coordination, tool mapping, rate limits, and audit logging.

Tauri commands and MCP tools must both call these services. MCP handlers must not invoke React commands or duplicate path, settings, or sync business logic.

## Transport and Lifecycle

The embedded MCP server uses the standard MCP Streamable HTTP transport at `/mcp` and binds only to `127.0.0.1`. It is disabled by default.

The MCP settings page exposes a stable configurable port with default `19618`. Enabling MCP starts the listener. If the port is occupied, QingYu reports a visible configuration error and leaves MCP disabled; it does not silently switch ports.

Disabling MCP:

1. stops accepting requests;
2. cancels pending app-confirmation requests;
3. invalidates all MCP sessions and preview tokens;
4. leaves authorized-directory and permission configuration intact for a future re-enable.

MCP sessions use cryptographically random session identifiers and a 30-minute idle timeout. A session identifier is routing state, not authentication. Every HTTP request must independently pass Bearer-token, Host, and Origin checks.

The server accepts only loopback Host values for the configured port. Requests with a browser Origin must use an explicit QingYu-approved local origin; missing Origin is allowed for non-browser MCP clients. Wildcard CORS is never emitted.

Default transport limits are:

- 8 MiB maximum request body;
- 8 MiB maximum serialized tool response;
- 120 requests per minute with a burst of 20 for the single global credential;
- 8 simultaneously executing tool calls;
- 100 results per list/search page.

The document content-size limit is configurable from 1 MiB to 64 MiB and defaults to 8 MiB. Transport and concurrency defaults may be tightened in QingYu settings but cannot be disabled.

## Authentication

On first enable, QingYu creates a random 256-bit Bearer token. The token is shown only through an explicit copy action and is never placed in ordinary application settings, logs, diagnostics, or audit entries.

The MCP Bearer token and the independent handle-signing key are stored in the operating system credential store. The signing key is not derived from the Bearer token. Token rotation:

- invalidates the previous token immediately;
- terminates all sessions and outstanding confirmations;
- invalidates all preview tokens;
- does not change object handles, authorized directories, or permissions.

The settings page supports copy, rotate, and revoke. Revocation disables MCP until a new token is generated. Authentication failures return a generic unauthorized response without revealing whether MCP is disabled, a token is absent, or a token is stale.

## Global Authorization Model

There is one application-owned permission profile shared by every authenticated client. A client cannot request, grant, or persist additional privileges.

The profile contains these independent capabilities:

- Documents: read/list/search.
- Documents: create/update.
- Documents: rename/move.
- Documents: delete.
- Application settings: read exposed fields.
- Application settings: update exposed fields.
- Sync: read configuration and status.
- Sync: update non-secret configuration.
- Sync: update credentials.
- Sync: test connection and run synchronization.

Unauthorized tools are omitted from `tools/list`. Tool handlers also re-evaluate the current permission on every call, so a permission reduction takes effect for existing sessions without reconnecting. QingYu sends `notifications/tools/list_changed` to connected clients when the exposed tool set changes.

Master enablement, token management, authorized-directory management, the permission profile, operation policy, transport limits, and security invariants are never mutable through MCP.

## Authorized Directory and Workspace Model

The user adds directories from QingYu's MCP settings page. QingYu resolves each selection to a canonical directory, rejects unsafe or duplicate roots, and stores the real path only in app-private native configuration. Absolute authorized paths are never returned to MCP clients or written into audit records.

Each authorized directory receives a random stable `workspaceId`. The identifier remains stable across restarts and across UI workspace changes. Removing a directory immediately makes its workspace and every object handle under it unusable. Re-adding the same directory creates a new `workspaceId` so stale authority does not revive accidentally.

Nested authorized roots are rejected. This avoids ambiguous ownership, permission evaluation, and audit identity. A directory cannot be authorized if it is inaccessible, is itself a symlink, resolves to a file, or is inside QingYu's application data or credential directories.

The current window's open folder does not change MCP authorization. The MCP settings page is the only way to add or remove an authorized root. There are no `workspace_open` or `workspace_close` tools.

`workspace_list` returns only safe metadata:

- `workspaceId`;
- user-chosen display name;
- leaf directory name;
- availability state;
- sync provider/configured state;
- permission-relevant feature state.

It does not return an absolute path.

## Object Handles

MCP clients use three application-issued identifier types:

- stable random `workspaceId` for an authorized root;
- signed `folderId` for a folder within that workspace;
- signed `documentId` for a Markdown document within that workspace.

A folder or document handle contains a versioned, type-tagged payload with `workspaceId` and normalized relative identity, authenticated with HMAC-SHA-256 using the independent signing key. The serialized value is URL-safe and treated as opaque by clients. Type tags prevent using a folder handle as a document handle or vice versa.

Handle validation verifies signature, version, type, current workspace authorization, relative-path syntax, protected-path rules, and the capability-root boundary. A valid signature never bypasses a current permission or filesystem-boundary check.

Rename and move operations return a new identifier. The former identifier no longer resolves after the old location disappears. Deletion invalidates the identifier. Handles remain stable across application restarts and ordinary content edits.

Clients may receive `relativePath` for display and recovery, but they cannot pass arbitrary paths to read, update, move, or delete existing objects. Creation uses `workspaceId`, `parentFolderId`, and one validated child name. Move uses an existing object ID and a target `folderId`.

## Filesystem Boundary Enforcement

All MCP document access is performed relative to a capability-scoped directory handle, preferably `cap-std`, rather than by joining strings onto an ambient absolute path.

Every operation applies all of these checks:

1. Decode and verify the signed handle before filesystem lookup.
2. Resolve the currently authorized canonical root from `workspaceId`.
3. Reject absolute POSIX paths, absolute or drive-relative Windows paths, UNC paths, NUL bytes, empty control segments, `.` and `..` segments, alternate separators, and invalid Unicode.
4. Resolve the target through the root capability without following an escape outside it.
5. Reject symlink or reparse-point targets in the first version, including symlinked parents and final entries.
6. Re-check metadata immediately before a mutation to reduce time-of-check/time-of-use exposure.
7. Apply the same protected-path and file-type filters to list and search as to reads and writes.

The implementation must test raw, percent-encoded, double-encoded, mixed-separator, Unicode-normalized, and platform-specific traversal forms. String prefix comparison is not a security boundary.

Protected entries are excluded recursively from MCP, including:

- `.qingyu`;
- `.git`;
- `.markra-sync` and other remote-sync metadata;
- QingYu history, recycle, temporary, and lock locations;
- `node_modules`, `target`, `build`, and `dist` where already excluded by workspace rules;
- non-Markdown files except image or attachment metadata that a future explicit tool may expose.

The first version manages Markdown documents only. Search and list never leak protected names, content, counts, or snippets.

## Tool Catalog

### Workspaces

- `workspace_list`: list currently authorized workspaces and safe metadata. Read-only, non-destructive, idempotent.

### Documents

- `document_list`: page through visible folders and Markdown documents below a workspace or folder. Read-only, non-destructive, idempotent.
- `document_search`: search visible document names and Markdown content with bounded result counts and snippets. Read-only, non-destructive, idempotent.
- `document_read`: return content, metadata, and current revision for one `documentId`. Read-only, non-destructive, idempotent.
- `document_create`: create one Markdown document under `parentFolderId`. Mutating, non-destructive, non-idempotent; rejects an existing target.
- `document_update`: replace the Markdown content of one `documentId` using `expectedRevision`. Mutating, non-destructive, conditionally idempotent.
- `document_move`: rename and/or move a document using `documentId`, target `folderId`, new name, and `expectedRevision`. Mutating, non-destructive, conditionally idempotent; rejects an existing target and returns the new ID.
- `document_delete`: delete one document using `documentId` and `expectedRevision`. Mutating and destructive; behavior follows the configured deletion policy.

Folder creation, folder rename/move, and recursive folder deletion are excluded from the first version. A document create may target only an existing folder ID.

### Application Settings

- `settings_get`: return the exposed settings schema, sanitized values, and settings revision. Read-only, non-destructive, idempotent.
- `settings_update`: update one or more exposed fields using `expectedRevision`. Mutating, non-destructive, conditionally idempotent.

### Sync

- `sync_config_get`: return sanitized sync configuration, credential-configured flags, sync revision, and current status for a workspace. Read-only, non-destructive, idempotent.
- `sync_config_update`: update allowlisted non-secret sync fields using `expectedRevision`. Mutating, non-destructive, conditionally idempotent.
- `sync_credentials_update`: set or explicitly clear provider credentials without ever reading them back. Mutating, sensitive, conditionally idempotent.
- `sync_test`: test the effective workspace/provider configuration without changing documents or the saved sync manifest. Read-only with external network access, non-destructive.
- `sync_run`: enqueue or execute synchronization for one workspace. Mutating, potentially destructive remotely, non-idempotent.
- `sync_status`: get a specific `runId` or the current workspace sync status. Read-only, non-destructive, idempotent.

Tool schemas use closed objects with unknown fields rejected. Every tool returns structured JSON content in addition to concise human-readable text.

## Document Semantics

`document_read` returns a revision derived from a stable digest of the exact bytes plus relevant file identity. `document_update`, `document_move`, and `document_delete` require the revision observed by the caller. If the document changed, QingYu returns `revision_conflict` with the current revision and safe metadata but does not apply the mutation.

Creation and move never overwrite an existing path, even if the client requests it. The API has no `force` flag.

Document writes use the existing safe-save path: validate current identity, write a sibling temporary file, flush it, atomically replace where supported, update history, emit normal document/watcher notifications, and trigger sync according to policy. A successful write returns the new revision, current object ID, and sync-trigger outcome.

MCP writes participate in the same application history and recovery behavior as UI writes. They must not create a second MCP-only history format.

Search is bounded and cancellable. It returns document ID, safe relative path, title, a short redacted snippet, match locations, and revision. It never follows symlinks or indexes protected directories.

## Application Settings Schema

Settings are exposed through a typed native registry. This requires moving MCP-relevant persistence and validation behind `SettingsService`; MCP must not edit Tauri Store files or frontend JSON directly.

The first-version allowlist covers non-secret user preferences in these groups when the corresponding setting exists in QingYu:

- appearance: color mode and selected light/dark theme;
- language and locale;
- editor presentation: font family, font size, line height, content width, line numbers, word wrap, and spellcheck;
- Markdown/file behavior: supported ignore-pattern preferences and safe document defaults;
- export defaults;
- non-security sync behavior that is not workspace-specific.

Each field descriptor includes type, validation constraints, mutability, and whether a restart is required. Unknown, removed, internal-only, or unsupported fields return `settings_field_not_exposed`.

The allowlist explicitly excludes:

- MCP enablement, connection URL/port, token, signing key, authorized roots, permissions, confirmations, dry-run, limits, and audit policy;
- recent files, active tabs, current folder, window state, restore state, and internal UI selections;
- updater state, diagnostics flags, internal schema versions, and migration markers;
- passwords, access keys, secret keys, authorization headers, and all other credentials;
- arbitrary Tauri Store keys or raw configuration documents.

`settings_update` is atomic across its requested fields. It validates the complete patch before persistence, requires `expectedRevision`, emits the normal cross-window settings event, and returns the new revision and normalized changed fields.

## Sync Configuration and Credentials

Sync tools operate on an authorized `workspaceId`, whether or not that workspace is currently open in a window. They call the existing project configuration and shared S3/WebDAV sync engine.

Sanitized readable fields include provider, enabled state, remote path/prefix, sync-after-save behavior, schedule, last-sync metadata, non-secret endpoint/region/bucket/server fields, and boolean flags such as `credentialsConfigured`. Passwords, secret keys, tokens, and authorization headers are never returned.

`sync_config_update` changes only typed non-secret fields and uses the existing project-config revision. Omitted fields preserve their current values. Provider-specific validation occurs before persistence.

`sync_credentials_update` requires the separate credential-write permission. It accepts provider-specific secrets as write-only input. Omitted secret fields preserve existing values. Clearing credentials requires `clearCredentials: true`; an empty string is rejected so accidental omission and intentional deletion are distinct. Tool responses contain only configured/unconfigured booleans.

`sync_test` uses the proposed or stored effective configuration but does not update last-sync time or sync manifests and does not transfer note content.

By default, `sync_run` returns `{ runId, state: "running" }` after enqueueing work. `sync_status` reports queued, running, succeeded, failed, or cancelled state and the existing sanitized sync summary. The sync coordinator coalesces duplicate requests for the same workspace and remains the only owner of manifests, conflict handling, and status.

When the policy selects wait mode, `sync_run` waits for completion up to the configured tool timeout and then returns the final result. A timeout does not cancel the background sync; the caller receives the `runId` and may poll.

## Configurable Operation Policy

The user can configure these app-owned policies:

### Write Confirmation

- `never`;
- `destructive-only` (default);
- `all-writes`.

When confirmation is required, QingYu shows an application-owned confirmation dialog containing the tool, workspace display name, safe logical target, revision, and effect. The MCP request remains pending until accept, reject, or timeout. If the application is not visible, it is activated. Rejection and timeout are explicit structured results. Client-side confirmation is not trusted as authorization.

### Dry Run

- `never`;
- `high-risk` (default);
- `all-writes`.

A required dry run returns a short-lived, single-use `previewToken` plus the normalized operation summary. The token is cryptographically bound to the tool name, canonical arguments, observed revisions, permission-policy generation, and expiry. The commit call must include it. Any argument, revision, permission, workspace authorization, or policy change invalidates the preview. Optional dry runs use the same mechanism.

High-risk operations are permanent deletion, credential changes, provider/remote-target changes, and sync runs that may propagate deletions.

### Deletion

- system trash (default);
- QingYu recycle bin;
- permanent deletion.

The QingYu recycle bin is app-private, outside the authorized workspace and sync tree, and stores only the minimum restore metadata. Cross-filesystem moves use copy, flush, verification, then source removal. Permanent deletion remains subject to destructive confirmation and dry-run policy.

### Sync Trigger After MCP Document Writes

- follow the workspace's current sync-after-save setting (default);
- always request sync;
- never request sync.

The existing sync coordinator coalesces the request. A document mutation succeeds independently of a later background sync failure, and its response reports the run ID or why no sync was queued.

### Sync Execution Mode

- background with `runId` (default);
- wait for completion.

### Audit Policy

- enabled by default;
- configurable retention duration and maximum entry count;
- explicit clear-audit action available only in QingYu UI.

## Non-Configurable Security Invariants

The following cannot be weakened by settings or MCP calls:

- loopback-only listener and mandatory Bearer authentication;
- Host/Origin validation and absence of wildcard CORS;
- application-owned authorized-directory boundary;
- signed object handles and current workspace-authorization checks;
- rejection of absolute paths, traversal, symlinks/reparse points, and protected paths;
- runtime permission check on every call;
- settings allowlist and closed tool schemas;
- revision conflict detection and no-overwrite semantics;
- token, secret, credential, absolute-path, and content redaction from diagnostics and audit;
- bounded request size, result size, concurrency, and rate limiting.

## Audit Log

Audit records are stored in QingYu's app-private data directory, not inside a workspace and not in a synced configuration file. Each record contains:

- timestamp and generated request ID;
- tool name;
- workspace ID and display name when applicable;
- safe logical document/folder name or settings field names;
- dry-run/confirmation outcome;
- success or structured error code;
- before and after revision when applicable;
- sync run ID when applicable;
- duration and result counts.

Audit records never contain document bodies or snippets, absolute paths, Bearer tokens, handle-signing keys, passwords, access keys, secret keys, authorization headers, or raw credential input. Diagnostic logging follows the same redaction rules.

## Error Model

Tool failures use a stable machine-readable code, a concise safe message, retryability, and an optional recovery hint. Initial codes include:

- `mcp_disabled`;
- `permission_denied`;
- `workspace_not_authorized`;
- `workspace_unavailable`;
- `invalid_handle`;
- `document_not_found`;
- `revision_conflict`;
- `target_already_exists`;
- `path_boundary_violation`;
- `protected_path`;
- `document_too_large`;
- `settings_field_not_exposed`;
- `confirmation_rejected`;
- `confirmation_timeout`;
- `preview_required`;
- `preview_expired`;
- `sync_not_configured`;
- `sync_in_progress`;
- `credential_write_denied`;
- `rate_limited`;
- `response_too_large`.

Errors may include safe relative display names, revisions, validation field names, and recovery actions. They never include absolute paths or secrets. Unexpected internal failures use a request ID that can be matched to redacted local diagnostics.

## Stdio Bridge

`qingyu-mcp` is a separate small Rust binary distributed with QingYu. It translates MCP stdio messages to the embedded Streamable HTTP endpoint and forwards responses/notifications. It contains no document, settings, sync, authorization, or filesystem implementation.

The bridge reads:

- `QINGYU_MCP_URL`, defaulting to `http://127.0.0.1:19618/mcp`;
- `QINGYU_MCP_TOKEN`, required unless a platform-specific secure handoff is added later.

If the endpoint is unavailable, the bridge starts the installed QingYu desktop application, waits with a bounded backoff for the endpoint, and then connects. It does not enable MCP, create a token, or change a port. If MCP is disabled or misconfigured, it returns actionable setup guidance on stderr without exposing the token.

The bridge must preserve MCP request IDs, cancellation, notifications, structured errors, and tool-list changes. It reconnects only for safe transport failures and never automatically replays a mutating call whose completion is unknown.

## Suggested Runtime Modules

The first implementation should add a focused native module tree similar to:

```text
apps/desktop/src-tauri/src/mcp/
  mod.rs
  server.rs
  auth.rs
  permissions.rs
  handles.rs
  policy.rs
  audit.rs
  error.rs
  tools/
    mod.rs
    workspace.rs
    document.rs
    settings.rs
    sync.rs
```

Reusable business services should live beside the current native domains rather than inside MCP. The exact extraction should remain small: first wrap existing behavior, then route both Tauri commands and MCP tools through the shared service. The frontend settings UI remains in `packages/app`; transport, credentials, guarded file access, and sync execution remain in Rust.

The `qingyu-mcp` binary should live in the existing Rust workspace/package arrangement if practical so distribution does not require Node.js. A mature Rust MCP SDK may be added only after verifying its current transport and protocol support; transport conformance must not be hand-waved behind an untested dependency.

## Failure and Recovery Behavior

- If an authorized directory is moved, deleted, or unmounted, it remains listed as unavailable but yields no document metadata. The user can remove and re-authorize it from QingYu.
- If a port becomes unavailable after startup, QingYu records a visible MCP health error and retries only after an explicit disable/enable or application restart.
- If secure credential storage fails, MCP enablement or token rotation fails safely without a plaintext-token fallback.
- If audit persistence fails, mutating tools fail closed while read-only tools may continue with a visible health warning. This prevents unaudited writes when auditing is enabled.
- If an app confirmation cannot be shown, the operation fails with `confirmation_timeout`; it is not silently allowed.
- If a settings or document write succeeds but its optional background sync later fails, the write remains successful and the sync run reports failure independently.
- If the stdio bridge loses the response to a mutating request, it reports an indeterminate outcome and instructs the caller to read current state before retrying.

## Testing

### Authentication and Lifecycle

- enable/disable, listener binding, stable port collision, app restart, and 30-minute session expiry;
- absent, malformed, incorrect, rotated, and revoked tokens;
- Bearer auth on every request even with a valid session ID;
- Host, Origin, CORS, body-size, response-size, concurrency, and rate-limit enforcement;
- session and preview invalidation after disable, token rotation, permission changes, or root removal.

### Authorization and Tool Discovery

- each permission independently changes `tools/list` and triggers `list_changed`;
- cached tool names cannot bypass a revoked permission;
- MCP cannot mutate MCP security settings or authorized directories;
- one global policy applies consistently to HTTP and stdio clients.

### Handle and Boundary Security

- handle signature tampering, type confusion, stale workspace IDs, cross-workspace substitution, and renamed/deleted handles;
- POSIX absolute paths, Windows drive/UNC/drive-relative paths, `..`, `.`, empty segments, alternate separators, NUL, invalid Unicode, and Unicode normalization variants;
- percent-encoded and double-encoded separators/traversal;
- symlinked roots, parent directories, files, race replacements, and platform reparse points;
- protected directories and files excluded from list, search, read, create, move, and delete;
- nested authorized roots rejected and re-authorization creating a new workspace ID.

### Document Behavior

- list/search pagination and limits;
- create conflict and move conflict without overwrite;
- revision conflicts for update, move, and delete;
- safe-save atomicity, history integration, watcher notifications, and new handle/revision results;
- request/content/response size enforcement;
- system trash, QingYu recycle, and permanent deletion policy behavior;
- confirmation and dry-run matrix, including preview-token argument/revision/policy binding;
- sync-trigger policy after a successful MCP write.

### Settings and Sync

- allowlisted settings round trip through the shared service and cross-window notifications;
- unknown/internal/security/secret settings rejected;
- atomic settings patch and revision conflicts;
- sanitized sync configuration and write-only credential behavior;
- omitted credentials preserved and explicit clearing required;
- secrets absent from responses, errors, logs, and audit;
- sync test without document transfer or manifest mutation;
- background run IDs, wait mode, status transitions, coalescing, conflict behavior, and failure recovery;
- existing WebDAV and S3 sync behavior preserved, including live S3 coverage when configured.

### End-to-End

- initialize and call representative read/write/sync tools through Streamable HTTP;
- initialize and call the same tools through `qingyu-mcp` stdio forwarding;
- bridge auto-start of an installed QingYu app;
- transport disconnect during a mutation does not cause automatic replay;
- disabling MCP or reducing permissions affects both transports immediately.

## Acceptance Criteria

- An authenticated MCP client can perform only the document, settings, and sync operations currently permitted by QingYu.
- No MCP request accepts or reveals an absolute document path.
- Only MCP-authorized directories are visible, regardless of the folder currently shown in a window.
- The stdio bridge cannot access documents or settings without the running QingYu service.
- Permission and root removal take effect for existing sessions immediately.
- Document and configuration writes detect stale revisions and never overwrite an existing target implicitly.
- Credential values are write-only and absent from all outputs and audit records.
- Traversal, encoding, symlink, protected-path, and cross-workspace tests demonstrate that the authorized root cannot be escaped.
- Existing UI document behavior, history, S3/WebDAV synchronization, and project configuration remain operational through the shared service boundary.
- The repository passes focused MCP tests plus `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml`, `pnpm test`, `pnpm typecheck:test`, and `pnpm build`; live S3 sync tests run when the configured test service is available.
