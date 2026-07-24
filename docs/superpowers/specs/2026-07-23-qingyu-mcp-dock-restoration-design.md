# QingYu MCP Dock Restoration Design

## Problem

QingYu can be launched first as `markra mcp serve`, which deliberately uses the
macOS prohibited activation policy and has no startup window. A later ordinary
launch is routed to that process by the single-instance plugin. The current
promotion path restores `ActivationPolicy::Regular` and shows the editor, but it
does not explicitly transform the process back into a Dock-visible foreground
application.

The local Codex MCP configuration also points at a generated
`target/debug/bundle` application. That bundle can disappear during later builds
while its process remains alive, leaving LaunchServices attached to a stale app
path instead of the installed `/Applications/QingYu.app`.

## Options

1. Restart the current process only. This restores the icon temporarily but the
   MCP-first launch sequence can reproduce the bug.
2. Restore Dock visibility only. This fixes the runtime transition but leaves
   Codex tied to an unstable generated bundle.
3. Restore Dock visibility and move Codex to the installed MCP executable. This
   fixes both the runtime defect and the local stale-path trigger.

Use option 3.

## Runtime Change

`activate_normal_ui` remains the single boundary for promoting a headless MCP
service into the normal application. On macOS it will:

1. set the activation policy to `Regular`;
2. set Dock visibility to `true`;
3. allow the existing reveal/focus path to show the editor window.

Each native operation remains best-effort and logs its own error. MCP service
invocations continue to stay fully backgrounded because they return before this
promotion boundary.

## Local Configuration Change

Update only the `mcp_servers.qingyu.command` value in the user's Codex config to:

```text
/Applications/QingYu.app/Contents/MacOS/qingyu-mcp
```

No token, PATH installation, or per-client secret is introduced.

## Verification

- Add a regression test proving the normal-UI promotion contains both the
  regular activation policy and Dock visibility restoration in that order.
- Run the focused desktop-runtime Rust tests and formatting checks.
- Build a macOS app bundle containing the fix and install it over the existing
  `/Applications/QingYu.app` with a recoverable backup.
- Stop the stale generated-bundle MCP process, open the installed app normally,
  and verify through AppKit that it is `Regular`, active, owns the menu bar, and
  resolves to `/Applications/QingYu.app`.
- Start the configured QingYu MCP executable and verify it can reach the same
  installed application without changing the foreground identity.
