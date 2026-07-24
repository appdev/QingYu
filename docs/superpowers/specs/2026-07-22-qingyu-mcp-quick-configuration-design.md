# QingYu MCP Quick Client Configuration Design

## Goal

After MCP is enabled in the QingYu desktop app, show a ready-to-copy client configuration that starts the `qingyu-mcp` stdio sidecar bundled with the current QingYu installation.

The experience borrows the reference application's information hierarchy—connection facts, a configuration preview, and copy actions—without adopting its HTTP endpoint or token model.

## Decisions

- The quick-configuration card is desktop-only and appears only while MCP is enabled.
- QingYu continues to use `MCP client -> qingyu-mcp stdio -> private local IPC -> QingYu`.
- The configuration contains only the absolute command path of the bundled `qingyu-mcp` executable. It contains no URL, port, token, workspace path, document handle, or permission grant.
- The absolute command works without installing either `qingyu-mcp` or the optional `markra` wrapper into `PATH`.
- The Rust desktop runtime resolves the sidecar command beside the running QingYu executable. The frontend does not guess installation paths.
- The UI supports two copy formats:
  - Codex TOML: `[mcp_servers.qingyu]` with a `command` field.
  - Generic JSON: an `mcpServers.qingyu.command` object for compatible MCP hosts.
- The selected format is shown in a read-only code preview. Codex TOML is the default.
- `Copy configuration` copies only the selected configuration.
- `Copy for AI tool` copies a short installation request followed by the selected configuration. It asks the AI tool to add the server without changing the command and without adding a URL or token.
- QingYu does not directly edit Codex, Claude Desktop, or any other client's configuration file.
- Copy failure is shown inline and does not affect the running MCP service.

## Runtime Contract

`McpSettingsSnapshot` gains `clientCommand: string | null`.

On desktop, the backend derives this value from the directory of `std::env::current_exe()` plus the platform executable name `qingyu-mcp` (including `.exe` on Windows). Returning `null` is allowed if the executable path cannot be resolved; the rest of MCP settings remain usable.

Mobile policy-only snapshots always return `clientCommand: null` and never render the quick-configuration card.

## Interface

The existing MCP service section remains first. When its enabled state is true and a local service plus client command are available, a new `Client connection` section follows it and precedes permissions.

The section contains:

1. An explanation that the configuration connects an MCP client to QingYu's bundled local bridge.
2. Two compact facts: `Transport: stdio + private local IPC` and `Authentication: No token required`.
3. A format selector for `Codex` and `Generic JSON`.
4. A horizontally scrollable, selectable code preview.
5. `Copy configuration` and `Copy for AI tool` actions with localized success/error feedback.

The configuration card never claims that enabling MCP grants permissions. The existing permission switches remain the sole application authorization controls.

## Security Boundaries

- The generated command can start only the QingYu-owned bridge; it does not give the client a filesystem path to the active notebook.
- The bridge continues to reach documents only through QingYu's current-workspace capability and application policy.
- Switching QingYu's current folder continues to invalidate old handles and changes the only document scope visible through MCP.
- A copied configuration is shared across clients, matching the existing application-wide/client-agnostic policy.
- No credential store or Keychain call is added.

## Error Behavior

- MCP disabled: hide the quick-configuration section.
- A client that retained an earlier copied configuration receives `mcp_disabled` without launching QingYu.
- Enabled MCP with no running application starts only the bundled headless `mcp serve` runtime; it does not reveal or focus a window.
- Mobile or policy-only runtime: hide the section.
- Sidecar path unavailable: show a localized unavailable note rather than a broken configuration.
- Clipboard API unavailable or rejected: keep the preview visible and show a localized copy error.

## Verification

- Rust unit tests cover macOS/Linux-style and Windows-style sidecar command derivation without depending on the test runner's actual installation path.
- Frontend tests cover hidden-while-disabled behavior, Codex and JSON output, both copy actions, path escaping, mobile exclusion, and copy failure.
- Existing tests continue proving no token, port, arbitrary directory, or direct-filesystem configuration is exposed.
- Focused Rust and frontend tests run first, followed by the repository's standard test, typecheck, and build gates.
