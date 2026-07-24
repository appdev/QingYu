# QingYu Project-Local MCP Without Keychain Design

## Goal

Make QingYu MCP a project-scoped application API that needs no persistent MCP credential and never asks macOS Keychain for access.

The only filesystem authority exposed through QingYu MCP is the folder currently active in QingYu. MCP clients still call QingYu services; they do not receive a general local-filesystem capability.

## Decisions

- MCP policy is stored in `<current-project>/.qingyu/mcp.json`.
- The active project root is runtime state supplied by the focused QingYu editor window. It is never read from `mcp.json` and cannot be expanded by editing that file.
- At most one workspace is visible through MCP: the current active project.
- Switching the active project replaces the workspace capability and invalidates every previously issued workspace, folder, document, cursor, and preview identifier.
- Identifiers are process-scoped. Restarting QingYu invalidates them; clients recover by calling `workspace_list`, `document_list`, or `document_search` again.
- MCP permissions remain one project policy shared by every client. There are no per-client ACLs.
- The normal stdio bridge uses an OS-local IPC channel without a Bearer Token: a mode-`0600` Unix-domain socket on macOS/Linux and a current-user named pipe on Windows.
- There is no default loopback HTTP listener, configurable port, MCP token, signing-key credential, or Keychain dependency.

## Threat Model

QingYu guarantees that calls made through its MCP endpoint stay inside the current active project and pass the configured permission, confirmation, dry-run, revision, protected-path, and audit checks.

QingYu does not attempt to constrain other tools available to the same AI process. A client that also has a shell or native filesystem tool may have authority unrelated to MCP. Removing MCP authentication intentionally treats processes running as the same operating-system user as locally trusted.

The editable project configuration is policy, not the security root. Values in `.qingyu/mcp.json` may enable tools or choose confirmation behavior for that project, but the file has no field that can select another directory, socket, command, or executable.

## Runtime Flow

```text
Codex or another MCP host
        |
        | stdio MCP
        v
qingyu-mcp bridge
        |
        | private local IPC, no token
        v
QingYu Tauri process
        |
        +-- current-project capability
        +-- project-local MCP policy
        +-- DocumentService
        +-- AppSettingsService
        +-- SyncService
```

The editor frontend reports its project root when the root changes and again when its window receives focus. The Rust backend canonicalizes the directory, rejects symlinks and protected roots, opens a capability-scoped directory handle, loads `.qingyu/mcp.json`, invalidates the prior session authority, and starts or stops the private IPC listener according to that project's `enabled` setting.

Opening a standalone Markdown file or a blank window clears that window's MCP project context. Focusing such a window leaves MCP with no document workspace and stops the listener.

## Project Configuration

`<project>/.qingyu/mcp.json` contains only non-secret project policy:

```json
{
  "version": 1,
  "enabled": false,
  "permissions": {
    "documentsRead": false,
    "documentsWrite": false,
    "documentsMove": false,
    "documentsDelete": false,
    "settingsRead": false,
    "settingsWrite": false,
    "syncRead": false,
    "syncWrite": false,
    "syncCredentialsWrite": false,
    "syncRun": false
  },
  "confirmation": "destructive-only",
  "dryRun": "high-risk",
  "deletion": "system-trash",
  "syncAfterWrite": "follow-workspace",
  "syncExecution": "background",
  "documentLimitBytes": 8388608,
  "requestLimitBytes": 8388608,
  "responseLimitBytes": 8388608,
  "requestsPerMinute": 120,
  "burstRequests": 20,
  "concurrentCalls": 8,
  "toolTimeoutSecs": 60,
  "audit": {
    "enabled": true,
    "retentionDays": 30,
    "maxEntries": 10000
  }
}
```

The configuration deliberately contains no absolute path, workspace list, workspace ID, port, token, socket path, or signing key. Atomic no-follow storage rejects a symlinked `.qingyu` directory or `mcp.json` file. The entire `.qingyu` control directory remains excluded from document tools and QingYu sync.

When a project has no MCP configuration, its effective policy is disabled defaults. Saving from the MCP settings page creates `.qingyu/mcp.json` for that project.

## Transport

`qingyu-mcp` remains the only MCP process configured in clients. It speaks standard MCP over stdin/stdout and forwards messages to the running QingYu app.

On macOS and Linux, QingYu listens on a stable socket below its application-data directory. The socket file is created with owner-only permissions, stale socket files are removed only after verifying they are sockets, and non-socket occupants cause a visible startup error. The bridge may accept a `QINGYU_MCP_SOCKET` override for testing or unusual installations; normal clients need no environment variables.

On Windows, the same message stream uses a named pipe whose security descriptor is scoped to the current user. Neither transport exposes a TCP port.

The bridge first attempts to connect. If QingYu is unavailable, it launches QingYu once and retries with bounded exponential backoff. It never replays an indeterminate tool mutation.

Request framing remains newline-delimited JSON-RPC through the Rust MCP SDK. The server enforces the configured maximum line size before JSON allocation, then applies rate and concurrent-call limits inside the shared handler so every local transport receives the same policy.

## Session Identifiers

QingYu generates one random 256-bit process key at MCP initialization. It is held only in memory and is used to authenticate document/folder identifiers, pagination cursors, and operation preview tokens. It is not an access credential, is never returned to a client, and is never written to disk or Keychain.

This keeps existing typed, tamper-resistant handles while intentionally making them process-scoped. A client does not place handles in its MCP configuration. After reconnect or restart it simply lists the active workspace and obtains current identifiers.

## Current-Project Boundary

The backend, not `.qingyu/mcp.json`, creates the sole active workspace entry. Activation:

1. canonicalizes the reported directory and rejects a symlink or non-directory;
2. rejects QingYu app data/config roots and other protected roots;
3. opens and retains a `cap_std::fs::Dir` capability;
4. records filesystem device/inode identity and revalidates it for every operation;
5. replaces the previous registry entry atomically;
6. invalidates all process-scoped identifiers and operation previews;
7. loads only the new root's `.qingyu/mcp.json`;
8. restarts or stops the local IPC listener according to the new policy.

Tool calls never accept an absolute project path. Existing no-follow traversal, protected-segment filtering, Markdown-only behavior, revision checks, atomic writes, history, trash policy, confirmations, and sync integration remain mandatory.

## Settings UI and Client Configuration

The MCP settings page operates on the invoking editor window's current project. It shows that project, global-for-the-project permissions, policy, limits, health, and audit entries.

The page removes:

- authorized-directory add/remove controls;
- TCP port controls and endpoint copy;
- Bearer Token copy/rotate/revoke controls;
- copy that claims authenticated loopback HTTP.

The client configuration becomes stable and secret-free:

```toml
[mcp_servers.qingyu]
command = "/Applications/QingYu.app/Contents/MacOS/qingyu-mcp"
```

No token or handle changes are written into Codex configuration after a QingYu restart.

## Failure Behavior

- No active project: MCP is stopped and settings report that a folder must be opened.
- Project MCP disabled: local listener is stopped.
- Unsafe project configuration path: reject the project policy and leave MCP stopped.
- Occupied or unsafe socket path: report `mcp_bind_failed` without deleting the occupant.
- Project switch during a call: the old capability/identifier fails closed; mutations keep their existing identity and revision rechecks.
- Oversized request: reject the frame before dispatch.
- Rate or concurrency exhaustion: return the existing structured `rate_limited` failure.

## Verification

- A process-start test proves initialization uses an in-memory key and has no credential-store dependency.
- Config storage tests prove only `.qingyu/mcp.json` is written, contains no path/token/key/port, and rejects symlink escapes.
- Workspace tests prove activation exposes exactly one current root and switching invalidates old workspace and document handles.
- Transport tests prove the bridge connects without token environment variables, launches QingYu once when absent, rejects oversized frames, and leaves no TCP listener.
- Frontend tests prove the current root is reported on change/focus and token/workspace/port controls are absent.
- Dependency and source scans prove `keyring`, `KeyringSecretStore`, Bearer-token commands, and loopback MCP endpoint code are gone.
- Full Rust, JavaScript, typecheck, production build, debug bundle, and live Codex MCP calls complete before integration to `main`.
