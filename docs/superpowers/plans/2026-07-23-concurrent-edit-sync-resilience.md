# Concurrent Edit Sync Resilience Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make QingYu synchronization tolerate edits during a run, queue save-triggered follow-up work, and recover transparently from transient S3 request failures.

**Architecture:** Keep the existing three-way per-file manifest and execution lock. Add bounded snapshot re-planning inside the Rust engine, a pending-save bit to the frontend shared-run coordinator, and one reusable signed S3 request retry loop whose exhausted result remains the only final error.

**Tech Stack:** Rust, Tokio, reqwest, Tauri v2, React, TypeScript, Vitest.

## Global Constraints

- Preserve the S3 object layout and manifest format.
- Preserve symlink, path traversal, durable staging, conflict-copy, and settings-journal protections.
- Retry only transient transport/HTTP failures, never credentials, authorization, validation, or integrity failures.
- Do not touch the unrelated untracked `macos-icon.icns` or settings appearance plan.
- Do not use the TypeScript `void` operator.

---

### Task 1: Re-plan concurrent filesystem changes

**Files:**
- Modify: `apps/desktop/src-tauri/src/remote_sync/engine.rs`

**Interfaces:**
- Produces: a bounded coordinator around the existing per-pass execution.
- Produces: exact concurrent-change classification that excludes unsafe paths.

- [x] Add a failing engine test whose upload-validation hook replaces a regular
  note with newer regular bytes and assert one call completes with the newer
  bytes remotely.
- [x] Run `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync::engine::tests::replans_regular_file_edits_during_upload --lib` and verify RED.
- [x] Extract one manifest planning/execution pass and return whether a fresh
  local/remote snapshot is required.
- [x] Catch only explicit concurrent regular-file and remote-identity outcomes;
  keep symlink and unsafe-path errors fatal.
- [x] Run the focused engine tests and verify GREEN.

### Task 2: Queue a save that arrives during synchronization

**Files:**
- Modify: `packages/app/src/hooks/useAppSyncCoordinator.ts`
- Modify: `packages/app/src/hooks/useAppSyncCoordinator.test.tsx`

**Interfaces:**
- Produces: `SharedRun.rerunSaveRequested: boolean`.
- Preserves: callers still receive one final shared outcome.

- [x] Add a failing test with a deferred first save, trigger a second save, and
  assert that resolving the first run starts one new native `save` request.
- [x] Run `pnpm --filter @markra/app exec vitest run src/hooks/useAppSyncCoordinator.test.tsx` and verify RED.
- [x] Consume a pending-save bit in the shared execution loop after each
  successful native result.
- [x] Verify manual callers still coalesce with an active save without adding an
  unnecessary pass.
- [x] Run the focused coordinator suite and verify GREEN.

### Task 3: Retry transient S3 request attempts

**Files:**
- Modify: `apps/desktop/src-tauri/src/remote_sync/diagnostics.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/s3_backend.rs`

**Interfaces:**
- Produces: a three-attempt signed request helper for core S3 operations.
- Produces: safe `s3-request-retrying` warning records without object paths.

- [x] Add a failing fixture test returning HEAD 404, PUT 503, PUT 200, HEAD 200;
  assert upload succeeds and the PUT is sent twice.
- [x] Add/retain the HTTP 403 test and assert only one PUT is sent.
- [x] Run the focused S3 tests and verify RED.
- [x] Implement fresh signing per attempt, transient status classification, and
  bounded backoff with test-only zero delay.
- [x] Route list, metadata, download, upload, and delete send failures through
  the helper without changing response parsing or identity verification.
- [x] Run the S3 backend suite and verify GREEN.

### Task 3.1: Close review-discovered concurrency gaps

**Files:**
- Modify: `apps/desktop/src-tauri/src/remote_sync/diagnostics.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/engine.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/s3_backend.rs`
- Modify: `packages/app/src/hooks/useAppSyncCoordinator.ts`
- Modify: focused tests beside each implementation.

- [x] Add atomic `If-None-Match` / `If-Match` conditions to every S3 PUT and
  DELETE attempt and map 409/412 to a warning-level re-plan signal.
- [x] Require upload response ETag and HEAD verification to agree.
- [x] Retry truncated/failed successful GET response bodies from a fresh signed
  request.
- [x] Retry regular-file disappearance during both initial and final snapshots;
  retain fatal symlink and non-regular-file behavior.
- [x] Bound a shared run to one trailing save and queue later saves behind it
  with their own eligibility predicate.
- [x] Add focused regression tests for each review finding and verify GREEN.

### Task 4: Verify the integrated behavior

**Files:**
- Modify only files listed above if verification finds a scoped defect.

- [x] Run `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync --lib`.
- [x] Run `pnpm --filter @markra/app exec vitest run src/hooks/useAppSyncCoordinator.test.tsx`.
- [x] Run `pnpm test`.
- [x] Run `pnpm typecheck:test`.
- [x] Run `pnpm build`.
- [x] Run `git diff --check` and inspect `git status --short` to ensure unrelated
  user files remain untouched.

### Task 5: Audit regression-test coverage

**Files:**
- Modify: `apps/desktop/src-tauri/src/remote_sync/engine.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/s3_backend.rs`
- Modify: `packages/app/src/hooks/useAppSyncCoordinator.test.tsx`

- [x] Map the pre-fix tests against each concurrent-edit and stale-plan failure
  and identify stable-snapshot assumptions.
- [x] Cover new local paths, continuously unstable snapshots, and stale remote
  upload/ordinary-download/conflict-download/delete plans.
- [x] Cover HTTP and transport-disconnect retries, ambiguous conditional
  creates, truncated download bodies, stale conditional deletes, and
  conditional headers on every retry.
- [x] Cover the failure boundary for a pending save without adding an automatic
  retry loop.
- [x] Bound S3 fixture accept/read/write operations and fail explicitly when a
  regression sends fewer requests than expected.
- [x] Temporarily remove the key engine, S3 retry, and frontend trailing-save
  protections and verify that their regression tests fail for the intended
  reason; restore the implementation and verify GREEN.

## Self-review

The design requirements map to one engine task, one scheduling task, one S3
transport task, and final integration verification. The plan keeps the existing
manifest and public API stable, contains no migration, and has explicit RED/GREEN
checks for every behavior change.
