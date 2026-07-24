# QingYu MCP Headless CLI Service Design

## Goal

Make QingYu MCP usable through the executables bundled inside the application without installing a command into `PATH`, while ensuring that a configured MCP client never opens or focuses QingYu when the user has disabled MCP.

When MCP is enabled and QingYu is not running, the bundled bridge may start a QingYu-owned background service. That service must not create an editor window, restore window state, activate the application, or interrupt the user's current work.

## User-visible Contract

- The client configuration continues to contain the absolute path of the bundled `qingyu-mcp` executable. Installing the optional `markra` shell command is not required for MCP.
- If MCP is disabled or has never been enabled, starting `qingyu-mcp` exits without launching QingYu and reports `mcp_disabled` with a message that tells the user to enable MCP in QingYu Settings.
- If MCP is enabled and QingYu's local IPC endpoint is already available, the bridge connects without launching another process.
- If MCP is enabled but the endpoint is unavailable, the bridge starts the QingYu executable beside itself in headless MCP-service mode and retries the bounded local connection.
- Headless service startup produces no editor window, Dock activation, taskbar activation, focus change, or startup-window restoration.
- A later explicit user launch reuses the existing QingYu process and opens the normal editor window.
- Disabling MCP stops the local listener. It does not delete Codex, Claude, or other client configuration files.

## Existing CLI Boundaries

QingYu already has two command-line surfaces:

- the optional `markra` shell wrapper, which forwards arguments to the main QingYu executable;
- the bundled `qingyu-mcp` stdio bridge used by MCP clients.

MCP must not depend on the optional wrapper or on `PATH`. The bridge resolves the sibling QingYu executable from its own absolute executable path and launches that target directly. The main executable recognizes an internal MCP service command equivalent to `mcp serve`; the optional shell wrapper forwards it naturally but is not part of the client connection path.

## Disabled-State Preflight

The bridge performs a read-only preflight only after the first local IPC connection attempt fails. It reads QingYu's application-local `settings.json` from the same application-data directory used by the desktop runtime and inspects the `mcp.enabled` value.

The result is one of three states:

- `enabled`: background service startup is permitted;
- `disabled`: the `mcp` group is missing or `enabled` is false;
- `unavailable`: the settings file or MCP group is malformed or cannot be read safely.

`disabled` returns `mcp_disabled`; `unavailable` returns `mcp_config_unavailable`. Neither state launches QingYu. Error output is stable, actionable, written to stderr, and paired with a non-zero exit code so MCP hosts can surface the startup failure.

The preflight does not read a workspace path from client input, does not grant permissions, and does not expose application settings to the MCP client.

## Headless Service Startup

The bridge replaces macOS `open -a QingYu` and platform PATH lookup with a direct launch of the current `markra` application executable packaged beside `qingyu-mcp` (`markra.exe` on Windows). The technical executable name remains unchanged until the separately planned package rename. It passes the internal MCP service command and no client-controlled arguments.

The desktop runtime detects service mode before building the Tauri application:

- it removes the configured startup window from the runtime context;
- it skips main-window chrome, startup reveal fallback, opened-file delivery, settings prewarm, and focus behavior;
- on macOS it uses a background/accessory activation policy;
- it initializes the application settings, MCP controller, document service, sync service, audit service, and current primary-workspace authority;
- it activates the MCP workspace directly from the authoritative application-local primary-workspace state rather than waiting for the React main window.

The single-instance callback treats an internal service invocation as non-visual. A later ordinary invocation switches back to the normal activation policy and creates or restores the editor window.

## Process and Lifetime Behavior

The headless QingYu process owns the private IPC listener and remains available across multiple MCP calls and clients. Each `qingyu-mcp` stdio bridge remains scoped to its MCP host session, but all bridges reuse the same application service.

If MCP is disabled while a UI window exists, the listener stops and the UI remains open. If MCP is disabled while QingYu is running only as a headless service, the service stops accepting clients and exits after active MCP sessions close. An explicit normal application launch always retains normal desktop lifecycle behavior.

## Security Boundaries

- `qingyu-mcp` remains a transport bridge and does not gain direct note-file authority.
- Document, settings, and sync operations continue to execute inside the QingYu application service and pass its permission, confirmation, dry-run, revision, protected-path, and audit checks.
- Only the application-local enabled state authorizes background startup.
- The current primary notebook remains the sole MCP document scope.
- No token, Keychain item, TCP port, arbitrary command, client-provided executable, or client-provided workspace path is introduced.
- Client configuration remains application-wide and client-agnostic; disabling MCP does not mutate third-party configuration.

## Error Behavior

- MCP disabled or absent: `mcp_disabled`, no launch.
- Settings malformed or unreadable: `mcp_config_unavailable`, no launch.
- Bundled QingYu executable missing or unsafe: `app_launch_failed`, no PATH fallback.
- Enabled service fails to publish IPC within the existing bounded timeout: `upstream_unavailable`.
- An in-flight mutating request whose upstream connection is lost remains indeterminate and is never replayed automatically.

## Verification

Automated tests cover:

- absolute sibling executable resolution on macOS/Linux and Windows;
- no PATH lookup or `open -a` launch path;
- missing or disabled MCP settings returning `mcp_disabled` without invoking the launcher;
- malformed settings returning `mcp_config_unavailable` without invoking the launcher;
- enabled settings launching headless service once and using bounded reconnect;
- an already-running listener causing zero launches;
- service-mode runtime construction without a startup window or focus path;
- authoritative primary-workspace activation without a webview;
- a later normal single-instance invocation opening the editor;
- multiple bridge sessions reusing one service.

Live verification uses the bundled debug application and the current Codex MCP configuration:

1. Disable MCP, start the configured bridge, confirm the explicit error, no QingYu process launch, no window, and unchanged foreground application.
2. Enable MCP, close QingYu, invoke MCP from Codex, confirm a background service starts with no window or focus change.
3. Call workspace and document read tools, then make repeated calls and confirm the service process is reused.
4. Open QingYu explicitly and confirm the normal editor appears from the existing process.
5. Disable MCP and confirm the listener stops and later bridge startup returns `mcp_disabled` without reopening QingYu.
6. Run focused Rust and frontend tests, followed by the repository test, typecheck, build, and debug-bundle gates in an isolated clean worktree.
