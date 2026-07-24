# S3 Sync Real-MinIO Test Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Use superpowers:test-driven-development for every test-harness or behavior change and superpowers:verification-before-completion before reporting a gate as passing.

**Goal:** Build and execute a repeatable S3 note-sync verification process in which CRUD, conflicts, recovery, and every user-visible sync entry point are accepted only after real MinIO traffic and state verification.

**Architecture:** Add environment-gated live tests as a child of the existing private `remote_sync` module so they can call the production `S3Backend` and generic engine without widening production visibility. Keep ordinary tests offline. Use random run/scenario prefixes, test-only backend wrappers for deterministic failure injection, a documented desktop black-box checklist for manual/save/scheduled triggers, and explicit cleanup verification.

**Tech Stack:** Rust 2021, Tauri async runtime, reqwest/SigV4 already in the repository, React/TypeScript, Vitest, pnpm, local MinIO.

**Design:** `docs/superpowers/specs/2026-07-16-s3-sync-test-strategy-design.md`

## Global Constraints

- Every live scenario uses the configured MinIO endpoint and the existing `markra` bucket only through an isolated random prefix.
- Never create, delete, or enumerate unrelated bucket content.
- Never commit or print access keys, secret keys, authorization headers, or signed URLs.
- Do not add an S3 SDK or a production dependency.
- Normal `cargo test` and `pnpm test` remain network-independent.
- Use `pnpm` for JavaScript and workspace commands.
- Do not use the TypeScript `void` keyword or operator.
- Keep the current S3/WebDAV production behavior unchanged unless a failing test demonstrates a real defect.
- All live test cleanup must list the scenario prefix after deletion and assert zero remaining objects.
- All live tests run serially when invoked by the repository command.

---

### Task 1: Add the environment-gated live MinIO harness

**Files:**
- Modify: `apps/desktop/src-tauri/src/remote_sync.rs`
- Create: `apps/desktop/src-tauri/src/remote_sync/live_tests.rs`
- Modify: `package.json`
- Create: `docs/testing/s3-sync-minio.md`

**Interfaces:**
- Produces: `LiveS3Config::from_env()` with redacted diagnostics.
- Produces: a unique run prefix and per-scenario `S3Backend`.
- Produces: cleanup helpers that delete and then re-list only the isolated prefix.
- Produces: `pnpm test:s3-sync:live` as the canonical serial live-test command.
- Consumes: the existing private `S3Backend`, `S3SyncSettings`, `RemoteSyncBackend`, and `execute_remote_sync` interfaces.

- [ ] **Step 1: Write a failing harness smoke test**

Add a child test module declaration:

```rust
#[cfg(test)]
mod live_tests;
```

Create an ignored test named `live_minio_harness_uploads_reads_and_cleans_isolated_object`. It must:

1. read the four required and two optional `MARKRA_TEST_S3_*` variables;
2. create a random prefix below `MARKRA_TEST_S3_PREFIX_ROOT`;
3. upload `harness/marker.md` through the production `S3Backend`;
4. list and download it through real MinIO;
5. compare exact bytes;
6. delete it and assert the final listing is empty.

The test helper returns `Result` values and defers assertions until cleanup has been attempted, so ordinary scenario failures do not skip remote cleanup.

- [ ] **Step 2: Run the test and verify RED**

Run with credentials supplied only in the shell environment:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml live_minio_harness_uploads_reads_and_cleans_isolated_object -- --ignored --nocapture --test-threads=1
```

Expected: compilation FAIL because `live_tests.rs` and its helpers are incomplete, or the new test FAILS before the production backend is wired in.

- [ ] **Step 3: Implement the minimal harness**

Implement:

- required-variable validation that names only the missing variable;
- region default `us-east-1`;
- prefix-root default `markra-sync-tests`;
- a run ID derived from UTC time plus process ID and a unique monotonic suffix;
- `backend_for(scenario)` that normalizes the isolated prefix;
- local temporary roots under `std::env::temp_dir()`;
- `cleanup_backend_prefix()` using `list_files` and identity-checked deletes;
- final empty-list verification;
- redacted failure formatting.

Do not log the environment-variable values.

- [ ] **Step 4: Add the canonical command and operator documentation**

Add to the root `package.json`:

```json
"test:s3-sync:live": "cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml live_minio_ -- --ignored --nocapture --test-threads=1"
```

Document the variable names, isolation rules, serial execution, cleanup behavior, and credential-redaction policy in `docs/testing/s3-sync-minio.md`. Use placeholders only; do not copy the live credentials into the document.

- [ ] **Step 5: Verify GREEN and offline isolation**

Run:

```bash
pnpm test:s3-sync:live
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync
```

Expected: the ignored live harness PASS against MinIO; ordinary Rust tests PASS without reading live credentials or making network calls.

- [ ] **Step 6: Commit**

```bash
git add package.json apps/desktop/src-tauri/src/remote_sync.rs apps/desktop/src-tauri/src/remote_sync/live_tests.rs docs/testing/s3-sync-minio.md
git commit -m "test(sync): add isolated MinIO harness"
```

---

### Task 2: Cover real-MinIO create, read, update, and object topology

**Files:**
- Modify: `apps/desktop/src-tauri/src/remote_sync/live_tests.rs`
- Modify: `docs/testing/s3-sync-minio.md`

**Interfaces:**
- Produces: reusable assertions for local bytes, remote bytes, summaries, manifests, and no-op re-runs.
- Consumes: the isolated backend/local-root harness from Task 1.

- [ ] **Step 1: Write failing CRUD scenario tests**

Add ignored tests with the `live_minio_` prefix:

- `live_minio_uploads_local_create_and_downloads_remote_create`
- `live_minio_propagates_local_and_remote_updates`
- `live_minio_handles_nested_unicode_reserved_empty_and_binary_files`
- `live_minio_paginates_more_than_one_thousand_objects`

The first test must use two different temporary local roots: upload from device A, then download into device B. The update test must baseline both sides, update local-to-remote, then remote-to-local. Include a size-preserving content update.

The topology test covers nested paths, spaces, non-ASCII characters, URL-sensitive characters supported by the relative-path rules, empty files, and binary bytes. The pagination test creates 1001 one-byte objects below its scenario prefix and asserts the production list loop returns all objects.

- [ ] **Step 2: Run focused tests and verify RED**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml live_minio_uploads_local_create_and_downloads_remote_create -- --ignored --nocapture --test-threads=1
```

Expected: FAIL because the shared state/manifest/idempotence assertions do not yet exist or expose an incorrect expectation.

- [ ] **Step 3: Implement assertions and complete scenarios**

Add helpers that:

- write local files with parent creation;
- seed remote files through `S3Backend::upload`;
- download remote bytes using the identity returned by `list_files`;
- load `.markra-sync/s3-manifest.json` as JSON without exposing secrets;
- assert expected uploaded/downloaded/conflict/skipped counts;
- snapshot local hashes, remote identities, and manifest entries;
- for non-conflict scenarios, run an immediate second sync and assert zero uploads, downloads, or conflicts and unchanged snapshots.

Do not use timestamps as a content oracle.

- [ ] **Step 4: Run the live group**

Run:

```bash
pnpm test:s3-sync:live
```

Expected: create/read/update/topology/pagination tests PASS and every scenario reports zero cleanup leftovers.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src-tauri/src/remote_sync/live_tests.rs docs/testing/s3-sync-minio.md
git commit -m "test(sync): cover S3 CRUD against MinIO"
```

---

### Task 3: Cover deletion propagation and changed-survivor semantics

**Files:**
- Modify: `apps/desktop/src-tauri/src/remote_sync/live_tests.rs`

**Interfaces:**
- Produces: real-MinIO coverage for both deletion directions and both deletion-versus-change branches.

- [ ] **Step 1: Write failing deletion tests**

Add:

- `live_minio_propagates_local_delete_to_remote`
- `live_minio_propagates_remote_delete_to_local`
- `live_minio_preserves_remote_change_when_local_was_deleted`
- `live_minio_preserves_local_change_when_remote_was_deleted`

Each scenario first creates a manifest baseline through `execute_remote_sync`. The test then applies exactly one delete/change combination, syncs, verifies both state surfaces, and runs the no-op assertion.

- [ ] **Step 2: Run one test and verify RED**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml live_minio_propagates_local_delete_to_remote -- --ignored --nocapture --test-threads=1
```

Expected: FAIL until the live baseline and direct deletion verification helpers are connected.

- [ ] **Step 3: Implement and verify all four branches**

Use identity-checked `S3Backend::delete` to create a remote deletion. Verify remote deletion through a new production listing and local deletion through filesystem state. For changed survivors, compare exact bytes after restoration.

Run:

```bash
pnpm test:s3-sync:live
```

Expected: all deletion scenarios PASS, unrelated objects below the scenario prefix remain unchanged, and cleanup leaves zero objects.

- [ ] **Step 4: Commit**

```bash
git add apps/desktop/src-tauri/src/remote_sync/live_tests.rs
git commit -m "test(sync): verify S3 deletion semantics"
```

---

### Task 4: Cover conflicts, target binding, and ignored paths

**Files:**
- Modify: `apps/desktop/src-tauri/src/remote_sync/live_tests.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/engine.rs`

**Interfaces:**
- Produces: deterministic conflict-file discovery and collision assertions.
- Produces: malformed-manifest and prefix-target reset coverage.

- [ ] **Step 1: Write failing conflict tests**

Add:

- `live_minio_preserves_both_first_sync_versions_as_conflict`
- `live_minio_preserves_both_changed_versions_as_conflict`
- `live_minio_uses_unique_conflict_name_and_does_not_repeat_conflict`
- `live_minio_resets_manifest_when_prefix_target_changes`
- `live_minio_rejects_malformed_manifest_without_mutation`
- `live_minio_never_uploads_fixed_ignored_directories`

Assert that the original local file remains unchanged and the local conflict file contains the exact remote bytes. Assert extension preservation, a unique suffix on collision, and a single conflict for the remote revision. The first follow-up sync may upload the newly created conflict file; the following sync must be a complete no-op with stable state.

- [ ] **Step 2: Run focused tests and verify RED**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml live_minio_preserves_both_first_sync_versions_as_conflict -- --ignored --nocapture --test-threads=1
```

Expected: FAIL until conflict-file and manifest assertions are implemented. If the failure exposes production behavior that contradicts the approved design, stop and apply systematic debugging before changing the engine.

- [ ] **Step 3: Add only the minimum reusable engine test seam if needed**

Prefer assertions against filesystem and manifest state from `live_tests.rs`. Modify `engine.rs` only if a deterministic UTC timestamp or conflict-path collision cannot be observed without a test-only seam. Keep any seam under `#[cfg(test)]` and do not change production behavior.

- [ ] **Step 4: Run live and offline regression tests**

```bash
pnpm test:s3-sync:live
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync
```

Expected: live conflict/manifest/ignored-path scenarios PASS and offline engine tests remain PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src-tauri/src/remote_sync/live_tests.rs apps/desktop/src-tauri/src/remote_sync/engine.rs
git commit -m "test(sync): verify S3 conflict and manifest behavior"
```

---

### Task 5: Cover real-MinIO failure, checkpoint, atomicity, and concurrency

**Files:**
- Modify: `apps/desktop/src-tauri/src/remote_sync/live_tests.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/engine.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/s3_backend.rs`

**Interfaces:**
- Produces: a test-only `RemoteSyncBackend` wrapper around the real `S3Backend` that fails or mutates on a selected operation.
- Consumes: stable BTreeMap action ordering and per-action manifest checkpoints.

- [ ] **Step 1: Write failing recovery and safety tests**

Add:

- `live_minio_checkpoints_completed_upload_before_injected_failure`
- `live_minio_rejects_remote_change_between_plan_and_upload`
- `live_minio_removes_temp_file_when_atomic_replace_fails`
- `live_minio_redacts_invalid_credentials`
- `live_minio_missing_bucket_does_not_mutate_local_state`

For checkpoint recovery, create two sorted local paths, allow the first real MinIO upload, inject failure on the second, inspect the manifest, then retry with the unwrapped backend. Assert the first object identity did not change and only the remaining object uploads.

For concurrent mutation, establish a baseline, change the local file, then have the wrapper replace the real remote object immediately before delegating the planned upload. The production identity check must reject the stale expected identity and preserve the concurrent remote bytes.

For atomic replacement, seed a real remote object whose destination path is an existing local directory. The download reaches MinIO, the final rename fails, and the `.markra-sync-tmp` sibling must be removed.

- [ ] **Step 2: Run focused tests and verify RED**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml live_minio_checkpoints_completed_upload_before_injected_failure -- --ignored --nocapture --test-threads=1
```

Expected: FAIL until the test-only wrapper and checkpoint inspection are implemented.

- [ ] **Step 3: Implement test-only wrappers**

Implement the wrapper in `live_tests.rs` and delegate every non-injected operation to the production `S3Backend`. Do not add failure configuration to `S3SyncSettings` or runtime requests.

Touch `engine.rs` or `s3_backend.rs` only when a failing test demonstrates an actual defect. For any production fix, first add the smallest focused offline regression test, verify RED, implement the fix, and verify GREEN before returning to the live suite.

- [ ] **Step 4: Run failure group and secret scan**

```bash
pnpm test:s3-sync:live
rg -n "MARKRA_TEST_S3_SECRET_ACCESS_KEY|authorization|x-amz-signature" apps/desktop/src-tauri/src/remote_sync docs/testing package.json
```

Expected: live failures are handled as designed; source contains variable names where necessary but no credential values, authorization header contents, or signed URLs.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src-tauri/src/remote_sync/live_tests.rs apps/desktop/src-tauri/src/remote_sync/engine.rs apps/desktop/src-tauri/src/remote_sync/s3_backend.rs
git commit -m "test(sync): exercise S3 recovery against MinIO"
```

---

### Task 6: Complete offline trigger contracts for every sync point

**Files:**
- Modify: `packages/app/src/App.test.tsx`
- Modify: `packages/app/src/hooks/useNativeBindings.test.tsx`
- Modify: `packages/app/src/lib/sync.test.ts`
- Modify: `apps/desktop/src/runtime/tauri/file.test.ts`

**Interfaces:**
- Produces: complete trigger-to-common-orchestration contract coverage.
- Consumes: existing app test harness and runtime injection points.

- [ ] **Step 1: Audit and write only missing failing trigger tests**

Keep existing coverage for S3 save-after and scheduled sync. Add missing cases for:

- manual S3 sync from Settings;
- manual S3 sync from native menu and shortcut;
- `lastSyncAt` changes only after successful real orchestration completion;
- failed save does not start sync-after-save;
- disabled sync-after-save produces no sync request;
- disabled schedule produces no sync request;
- an in-progress run suppresses overlapping scheduled/manual requests;
- WebDAV dispatch remains provider-isolated.

These are timing and routing tests, so mocks are allowed here. They do not count as MinIO acceptance.

- [ ] **Step 2: Run focused tests and verify RED**

```bash
pnpm --filter @markra/app exec vitest run src/App.test.tsx src/hooks/useNativeBindings.test.tsx src/lib/sync.test.ts
pnpm --filter @markra/desktop exec vitest run src/runtime/tauri/file.test.ts
```

Expected: at least the newly identified missing cases FAIL before minimal orchestration changes, or PASS immediately if the behavior is already covered; do not create duplicate tests solely to manufacture RED.

- [ ] **Step 3: Implement only demonstrated trigger defects**

If a test exposes a defect, change the smallest relevant production module and keep the three triggers routed through the existing common S3 sync orchestration. Do not introduce a test-only frontend path.

- [ ] **Step 4: Verify focused tests and type checking**

```bash
pnpm --filter @markra/app exec vitest run src/App.test.tsx src/hooks/useNativeBindings.test.tsx src/lib/sync.test.ts
pnpm --filter @markra/desktop exec vitest run src/runtime/tauri/file.test.ts
pnpm --filter @markra/app typecheck:test
pnpm --filter @markra/desktop typecheck:test
```

Expected: all focused trigger tests and type checks PASS.

- [ ] **Step 5: Commit**

```bash
git add packages/app/src/App.test.tsx packages/app/src/hooks/useNativeBindings.test.tsx packages/app/src/lib/sync.test.ts apps/desktop/src/runtime/tauri/file.test.ts
git commit -m "test(sync): complete S3 trigger contracts"
```

---

### Task 7: Add the real desktop entry-point acceptance procedure

**Files:**
- Create: `docs/testing/s3-sync-desktop-acceptance.md`
- Create: `docs/testing/s3-sync-test-report-template.md`
- Modify: `docs/testing/s3-sync-minio.md`

**Interfaces:**
- Produces: a repeatable black-box checklist for manual, save-after, and scheduled sync.
- Produces: an evidence template that cannot confuse mocked coverage with real-MinIO acceptance.

- [ ] **Step 1: Write the acceptance checklist**

Document one fresh isolated prefix per trigger and exact steps for:

1. Settings manual sync;
2. native menu/shortcut manual sync;
3. sync-after-save enabled and disabled controls;
4. scheduled sync enabled, in-progress suppression, and disabled controls.

For every positive trigger require:

- a unique marker file and content hash;
- actual desktop action;
- observed MinIO object key and exact bytes;
- app success summary and `lastSyncAt`;
- immediate no-op second run.

For every disabled control require a bounded wait and proof that the MinIO object identity remains unchanged.

- [ ] **Step 2: Add the report template**

Include fields for commit, app build, platform, endpoint host, bucket, isolated prefix, trigger, local hash, remote hash/ETag, summary counts, `lastSyncAt`, cleanup count, and evidence location. Explicitly prohibit credential values and state that frontend mocks do not satisfy the entry-point gate.

- [ ] **Step 3: Review the procedure against the design**

Check that all exit criteria in `docs/superpowers/specs/2026-07-16-s3-sync-test-strategy-design.md` map to either an automated test name or a desktop checklist row. Add a traceability table to the acceptance document.

- [ ] **Step 4: Commit**

```bash
git add docs/testing/s3-sync-desktop-acceptance.md docs/testing/s3-sync-test-report-template.md docs/testing/s3-sync-minio.md
git commit -m "docs(test): define S3 desktop acceptance"
```

---

### Task 8: Execute automated live MinIO verification

**Files:**
- Create: `docs/testing/results/<UTC-date>-s3-sync-minio.md`

**Interfaces:**
- Produces: a redacted, commit-specific live integration report.

- [ ] **Step 1: Confirm the bucket without exposing credentials**

Export the supplied endpoint, region, bucket, access key, and secret in the process environment. Do not place them in shell history, command arguments, `.env` files, documentation, or tool output. Confirm the harness uses a new prefix below `markra-sync-tests`.

- [ ] **Step 2: Run the full live suite**

```bash
pnpm test:s3-sync:live
```

Expected: every `live_minio_` scenario PASS serially, including final zero-object cleanup assertions.

- [ ] **Step 3: Re-run any failure through systematic debugging**

If a test fails, preserve its isolated run ID, inspect only that prefix, determine whether the failure is harness, environment, or production behavior, add a focused regression test, fix, and rerun both the focused test and full live suite. Never weaken an assertion merely to match observed behavior.

- [ ] **Step 4: Write the redacted report**

Record the commit hash, endpoint host, bucket, run prefix root, scenario results, durations, cleanup results, and commands. Do not record credential values or signed URLs.

- [ ] **Step 5: Commit the report if repository policy accepts test evidence**

```bash
git add docs/testing/results/<UTC-date>-s3-sync-minio.md
git commit -m "test(sync): record MinIO verification"
```

If test-result documents are intentionally not versioned, keep the report as a local artifact and state its absolute path in the handoff instead.

---

### Task 9: Execute all desktop sync points against real MinIO

**Files:**
- Modify: `docs/testing/results/<UTC-date>-s3-sync-minio.md` or the local report artifact chosen in Task 8.

**Interfaces:**
- Consumes: the debug/production desktop app, test connection settings, and the Task 7 checklist.
- Produces: real-MinIO evidence for every sync entry point.

- [ ] **Step 1: Build and launch the desktop app**

Run:

```bash
pnpm build
pnpm tauri build --debug
```

Launch the debug app using a disposable note folder. Configure S3 through the UI with the supplied connection and a fresh isolated remote prefix. Do not include credentials in screenshots or logs.

- [ ] **Step 2: Execute manual sync acceptance**

Run the Settings action and native menu/shortcut rows from the checklist. Verify exact MinIO bytes and no-op re-runs. Record evidence and clean only those trigger prefixes.

- [ ] **Step 3: Execute sync-after-save acceptance**

Run enabled and disabled rows. Verify that saving through the editor reaches MinIO only when enabled. Record evidence and clean only that trigger prefix.

- [ ] **Step 4: Execute scheduled sync acceptance**

Use the shortest supported interval. Verify positive scheduling, in-progress suppression, and disabled behavior within the documented bounded window. Record evidence and clean only that trigger prefix.

- [ ] **Step 5: Confirm entry-point gate**

The gate passes only if every positive trigger produced a verified MinIO object mutation and every disabled control left the object identity unchanged. A toast, frontend mock, or Rust-only call is insufficient.

---

### Task 10: Run final regressions and review the complete evidence

**Files:**
- Modify only if a verification failure requires a focused fix and regression test.

- [ ] **Step 1: Run all required verification commands**

```bash
pnpm test
pnpm typecheck:test
pnpm build
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
pnpm test:s3-sync:live
```

Expected: all commands PASS. The ordinary Rust run skips ignored live tests; the explicit live command runs them.

- [ ] **Step 2: Run repository hygiene checks**

```bash
git diff --check
git status --short
git ls-files | rg '(^|/)(node_modules|dist|target)/|\.env$'
```

Expected: no whitespace errors, no generated directories or secret environment files tracked, and only intentional changes present.

- [ ] **Step 3: Review acceptance traceability**

Confirm the report contains:

- all core state-matrix action classes;
- create/read/update/delete and both changed-survivor branches;
- first-sync and changed-both conflicts;
- manifest target reset;
- checkpoint retry, atomic replacement cleanup, and concurrent mutation rejection;
- non-conflict immediate no-op re-runs, plus conflict stabilization followed by a no-op run;
- real desktop manual, save-after, and scheduled MinIO evidence;
- disabled trigger evidence;
- zero-object cleanup for every isolated prefix;
- full regression results.

- [ ] **Step 4: Request final code review**

Use superpowers:requesting-code-review. Treat any finding about destructive prefix handling, credential exposure, false-positive MinIO acceptance, or uncleaned remote objects as blocking.

- [ ] **Step 5: Commit any final focused corrections**

Stage only the files changed by the correction and use a narrow commit message. Rerun the affected focused test, full live suite, and final regression commands before completion.
