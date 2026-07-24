# Application Sync Task 2 Report

## Outcome

The remote-sync engine now receives an explicit `RemoteSyncScope`. The scope is
the single owner of the canonical local source, local state, manifest path,
conflict root, durable staging root, include policy, content validator, local
identity, and protected temporary names.

- Notes scopes scan all allowed ordinary files, apply global and
  `.markraignore` rules, and reject state roots below the notes source.
- Portable-settings scopes expose exactly `settings.json`; credentials, local
  state, sync state, MCP runtime data, themes, extensions, and workspace data
  are excluded.
- Manifests and conflicts are stored below scope state, never in the notes
  source.
- Every backend listing path is validated before scope filtering or local path
  joining. Absolute, drive, UNC/backslash, parent, empty-segment, and NUL paths
  fail closed. Local traversal and publication retain the existing no-follow
  capability checks.
- Download bytes are first durably staged in state. Publication then uses a
  protected same-directory temporary file and atomic no-replace rename, so a
  notes root on another volume does not rely on cross-filesystem rename.
- The durable state file is now the only publication source. Both state and
  publication files are reopened no-follow and checked by exact device, inode,
  single-link count, length, and SHA-256 immediately before their sensitive
  use. Symlink, hard-link, replacement, and same-length content mutations fail
  closed and clean both staging tiers.
- Failed restoration never clobbers a new occupant. Captured regular files and
  directories are retained below state conflicts when possible; if retention
  fails, the protected source-side quarantine is kept and reported.

## Review remediation

- WebDAV and S3 validate every listed remote path before protected-path or
  include-policy filtering. Invalid descendants such as
  `.qingyu/../outside` now fail the listing rather than disappearing behind an
  ignore rule.
- State roots are absolute, canonical, outside the notes source (or a strict
  app-data descendant for portable settings), opened one component at a time
  without following symlinks, and bound to their captured device/inode.
  Staging, conflicts, and manifest I/O use retained state-directory
  capabilities and revalidate the root identity before later operations.
- Production project-sync state is no longer derived from the environment
  temporary directory. `AppHandle` supplies app data, and the canonical notes
  root SHA-256 selects
  `app_data/sync-state/notes/<canonical-root-sha256>`. Manifest, staging,
  conflict, and legacy status data remain below that app-owned state root;
  the notes source receives none of them. The legacy status loader resolves
  the same app-data path.
- Crash cleanup recognizes only strict versioned names containing PID,
  creation time, a per-scope random 128-bit run nonce, and sequence. Exact
  current-run entries or expired entries may be removed; fresh entries from a
  different run, malformed same-prefix files, and symlinks are preserved.
- The run nonce no longer depends on desktop-only `uuid`. Every target uses
  the common `getrandom` dependency to produce 16 random bytes encoded as
  exactly 32 lowercase hexadecimal characters. The iOS library target now
  compiles the scope implementation.
- MCP persisted-status reads now resolve the authorized canonical notes root,
  derive its app-data hash state root, and load status only from there. A stale
  legacy status below the notes directory is never returned. Production uses
  `AppHandle` for app data; tests inject an isolated app-data root without
  adding a second production resolution path.

## Shared protected paths

`protected_paths.rs` now owns `.qingyu`, `.markra-sync`, the
`.markra-sync-stage-` prefix, and all related case-insensitive predicates.
Remote sync, Markdown ignore rules, and both watcher implementations import
that shared owner directly. `project_config` temporarily re-exports only the
control-directory constant still needed by its own storage/status modules.

## Duplication audit

- `manifest_file_name` was removed from `RemoteSyncBackend`; zero references
  remain. Manifest names are injected only through `RemoteSyncScope`.
- There is one `execute_remote_sync_locked` algorithm and one
  `plan_file_sync` action matrix for WebDAV, S3, notes, and settings.
- The old `sync_source_root`, `unique_conflict_path`, and engine-local protected
  path constants/predicates were removed; zero references remain.
- Ignore rules are constructed once per notes scope rather than once per
  scanned path.
- Staging-name generation and stale-name recognition are both scope methods;
  there is no second engine-local prefix definition.

## Verification

- `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync:: -q`
  passed: 85 passed, 0 failed, 21 environment-gated live tests ignored.
- The focused persisted-status regression passed: 1 passed, 0 failed. MCP
  sync-tool tests passed 3/3, and the local IPC/stdio end-to-end MCP acceptance
  test passed 1/1.
- `cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml --target
  aarch64-apple-ios --lib` passed. Before the nonce remediation, the same check
  failed with `E0433` because `scope.rs` referenced target-gated `uuid`.
- Default-parallel `cargo test --manifest-path
  apps/desktop/src-tauri/Cargo.toml -q` passed three consecutive final runs:
  each run was 543 passed, 0 failed, 21 ignored.
- `pnpm test` passed across the workspace, including 2,041 app tests, 213
  desktop tests, and 45 web tests.
- `pnpm typecheck:test` passed across every participating workspace package.
- `pnpm build` passed; desktop vendor-chunk verification imported all 12
  expected chunks.
- Targeted `rustfmt --edition 2021 --check` for all modified Rust files passed,
  and `git diff --check` passed.

The iOS check regenerated `gen/schemas/acl-manifests.json`. That derived
difference was removed after verification and is not included in the commit.

The live MinIO tests were not run because no `MARKRA_TEST_S3_*` environment was
used for this task. Their fixtures were updated to keep state outside the notes
root and to clean both source and sibling state roots.

## Follow-on boundary

Full portable-settings schema validation and store reload/event publication
remain Task 3 work. The production notes entry point already constructs its
stable app-data state root and provider-specific manifest name.
