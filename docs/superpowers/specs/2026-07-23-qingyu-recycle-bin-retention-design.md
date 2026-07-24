# QingYu Recycle Bin Retention Design

## Goal

Add a simple automatic-cleanup setting for the QingYu recycle bin. Users choose a preset retention period instead of entering a number manually, including an option that never removes entries automatically.

## Scope

- The setting applies only to the private QingYu recycle bin used by the MCP document deletion policy.
- It does not control the operating system Trash.
- Manual recycle-bin emptying and document restoration are outside this change.
- Package names and application identifiers are unchanged.

## User Experience

The MCP policy section shows a `Recycle bin cleanup` row immediately below `Deletion policy` only when the deletion policy is `QingYu recycle bin`.

The row uses the existing settings select styling and exposes exactly four options:

- `Never automatically clean up`
- `After 7 days`
- `After 30 days` (default)
- `After 90 days`

The Chinese labels are `永不自动清理`, `7 天后`, `30 天后`, and `90 天后`. Changing the selection saves immediately through the existing revision-checked MCP settings flow. Selecting `Never automatically clean up` disables scheduled removal but does not prevent a future manual empty-recycle-bin action.

The control supports the existing desktop and compact settings layouts, keeps the compact touch target at least 44 pixels high, and retains visible keyboard focus and disabled states through the shared settings select.

## Configuration Model

Add `recycleBinRetentionDays` to the MCP configuration. It is represented as a number with the closed set `0 | 7 | 30 | 90`:

- `0` means never clean automatically.
- `7`, `30`, and `90` are calendar-day retention periods.
- The default is `30`.
- Missing or unsupported persisted values normalize to `30` in both Rust and TypeScript.

Keeping this value in the application-owned MCP configuration means the policy is global across MCP clients, matching the rest of the MCP permissions and policies.

## Cleanup Lifecycle

When the desktop application initializes MCP and MCP is enabled, it performs one cleanup pass without opening or focusing an application window. A background maintenance loop then checks once every 24 hours while the application process remains alive. The loop reads the latest configuration before each pass and skips cleanup when MCP is disabled or retention is `0`.

Saving a changed retention period while MCP is enabled triggers an additional immediate cleanup pass. This makes a change from 90 days to 7 days effective immediately instead of waiting for the next daily interval.

Cleanup is best-effort maintenance. A malformed or undeletable entry is skipped and must not prevent MCP startup, settings saves, or cleanup of other valid entries.

## Filesystem Safety

Cleanup operates only on direct children of the configured QingYu recycle root. An entry is eligible only when all of these checks pass:

1. Its directory name parses as a UUID.
2. The child is a real directory, not a symbolic link.
3. `metadata.json` parses as QingYu recycle metadata.
4. `deleted_at` is at or before the calculated expiry cutoff.

The cleaner never accepts a path from an MCP client and never traverses outside the application-owned recycle root. Unknown files, malformed entries, symbolic links, and entries newer than the cutoff remain untouched.

## Testing

Rust tests cover the default and normalization rules, never-clean behavior, expiry-boundary removal, preservation of recent entries, skipping malformed directories and symbolic links, and continuing after an individual deletion failure where practical.

React tests cover conditional visibility, the four exact options, the 30-day default, immediate revision-checked updates, and selection of the never-clean value. TypeScript normalization tests cover missing and unsupported values.

Focused Rust, React, typecheck, and build verification run before completion.
