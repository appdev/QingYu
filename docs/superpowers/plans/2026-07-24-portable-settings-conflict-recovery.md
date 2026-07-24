# Portable Settings Conflict Recovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Recover a stale portable-settings reconcile journal without losing either local or remote settings and without permanently blocking synchronization.

**Architecture:** Reuse the remote-sync engine's protected state-conflict publication path. When a reconcile journal no longer matches current portable settings, archive its staged remote document and rebuild the active journal from the current local snapshot.

**Tech Stack:** Rust, Tokio, Tauri v2, serde, cap-std.

## Global Constraints

- Preserve S3 keys and sync-manifest version.
- Preserve local current settings and archive the pending remote document.
- Keep conflict copies private to sync state and use safe no-overwrite writes.
- Do not touch the unrelated untracked `macos-icon.icns`.

---

### Task 1: Recover a stale reconcile journal

**Files:**
- Modify: `apps/desktop/src-tauri/src/remote_sync/engine.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/service.rs`

**Interfaces:**
- Produces: `preserve_remote_settings_conflict(scope: &RemoteSyncScope, bytes: Option<&[u8]>) -> Result<PathBuf, String>`.
- Consumes: the validated bytes returned by `PortableSettingsJournal::staged_bytes`.

- [x] **Step 1: Change the regression test to require recovery**

  Rename the test to `concurrent_portable_writer_is_preserved_and_the_next_sync_recovers`.
  Keep the first `settings-reconcile-failed` assertion, then require the second
  call to succeed, the local setting to remain current, the remote pending bytes
  to exist under `conflicts/`, and the active journal to be absent.

- [x] **Step 2: Run the focused test and verify RED**

  Run:
  `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml concurrent_portable_writer_is_preserved_and_the_next_sync_recovers -- --nocapture`

  Expected: FAIL because the retry still returns `settings-reconcile-failed`.

- [x] **Step 3: Add the protected conflict helper**

  Add a focused wrapper in `engine.rs` that calls the existing
  `write_conflict_to_state` path with `settings.json` and a timestamped
  `settings.remote-conflict-*.json` name.

- [x] **Step 4: Rebuild stale reconcile transactions**

  In `prepare_portable_settings_sync`, archive `journal.staged_bytes()` when a
  non-applied reconcile journal conflicts with the current snapshot. Then allow
  the existing fresh-journal code to stage the current local snapshot instead
  of returning `AppSettingsError::reconcile_failed()`.

- [x] **Step 5: Run the focused test and verify GREEN**

  Run the command from Step 2. Expected: PASS with the conflict copy retained
  and no active pending journal.

- [x] **Step 6: Run repository verification**

  Run:
  - `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml`
  - `pnpm test`
  - `pnpm typecheck:test`
  - `pnpm build`
  - `git diff --check`

  Expected: every command exits 0 and `macos-icon.icns` remains untracked.

## Self-review

The plan changes one recovery branch and reuses the established conflict-write
primitive. It covers the installed-app failure exactly, includes RED/GREEN
evidence, and avoids unrelated settings or S3 transport changes.
