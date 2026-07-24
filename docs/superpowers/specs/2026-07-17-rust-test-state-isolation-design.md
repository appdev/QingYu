# Rust Test State Isolation Design

## Goal

Make the default parallel Rust test suite deterministic by isolating remote-sync test hooks and project-sync editing registry fixtures. The change must not alter production synchronization locking, file mutation behavior, registry lifetime, or the public meaning of the global editing counter.

## Root Cause

Remote-sync race tests currently install closures into process-wide hook slots before calling `execute_remote_sync` and clear them after the call returns. The production execution lock serializes synchronization work, but it does not cover hook installation. Parallel tests can therefore overwrite or clear another test's hook while that test is waiting for or holding the execution lock. A hook can then mutate the wrong test directory, while the intended test executes without its hook.

The editing registry is intentionally process-wide in production. One test reads the global counter, performs a second global read, and expects exact equality. Parallel tests for other project roots can legitimately advance that same counter between the two operations, making the assertion nondeterministic even though the root-specific state remains correct.

## Chosen Approach

Use per-execution hook injection and an isolated registry fixture.

Remote-sync tests will construct an owned test-hook bundle for one `execute_remote_sync` call. The production entry point continues to acquire the existing execution lock and runs with no hooks. A test-only entry point acquires the same lock and executes the same synchronization core with its own hook bundle. Hook callbacks are read only from that bundle, so one parallel test cannot install, replace, clear, or consume another test's callbacks.

The editing transition and snapshot logic will remain shared, but the canonical-root and session-matching test will operate on a fresh in-memory registry instance. Production commands continue to use the existing process-wide registry. Integration tests that intentionally exercise production global coordination may continue to use the global registry while avoiding assertions that assume no unrelated test can advance its global counter.

## Remote-Sync Structure

Split the synchronization entry into a locking wrapper and a private locked implementation:

- `execute_remote_sync` acquires the existing `REMOTE_SYNC_EXECUTION_LOCK` and invokes the locked implementation with the normal no-op hook context.
- A `cfg(test)` helper accepts an owned `RemoteSyncTestHooks`, acquires the same execution lock, and invokes the locked implementation with those hooks.
- The hook context is passed only to mutation helpers that expose a tested race window: upload validation, atomic replacement, final replacement, final deletion, and quarantine restoration.
- Existing test-only global hook slots and setter functions are removed.

The internal hook context exists in every build so helper signatures stay uniform, but callback fields, construction with callbacks, and the test entry point are all `cfg(test)`. In production it is an empty no-op value with no externally configurable failure-injection surface. The shared synchronization algorithm, backend calls, manifests, summaries, and safety checks remain unchanged.

## Editing Registry Structure

Keep `PROJECT_SYNC_EDITING_REGISTRY` as the production source of truth. Extract small internal helpers that load a snapshot and apply a transition against a caller-provided `ProjectSyncEditingRegistry` while the caller owns the lock or isolated instance.

Production functions obtain the existing global mutex guard and call those helpers. The registry semantics test creates `ProjectSyncEditingRegistry::default()`, canonicalizes its temporary root, and calls the same helpers directly. This preserves exact counter assertions inside a fixture no other test can mutate.

Tests that verify command-level broadcasting or cross-window coordination continue to use the production wrappers because global behavior is part of what they exercise. They must use unique temporary roots and compare root-specific state rather than assuming exclusive ownership of the process-wide counter.

## Error Handling and Cleanup

Per-execution hook ownership removes the need for setter/clear cleanup pairs. Returning an error or unwinding a test drops the hook bundle with the execution future, leaving no global callback behind for later tests.

The isolated registry fixture is stack-owned and is dropped at the end of its test. Temporary filesystem roots retain the existing explicit cleanup behavior. No credentials, endpoints, or user files are introduced into fixtures or diagnostics.

## Test-Driven Implementation

Use the already reproduced default-parallel failures as the red phase. The existing race-window tests fail with changing filesystem errors or unexpectedly successful summaries when another test replaces their hook, and the registry test fails when another root advances the shared counter. Both groups pass when the suite is forced to one thread.

1. Keep the existing remote-sync race-window assertions while moving each callback into the hook bundle passed to that test's execution. Add a focused unit test showing two independently constructed bundles retain their own callbacks.
2. Rewrite the editing registry semantics test against a fresh registry instance while retaining its exact counter assertions. It demonstrates that loading a snapshot does not advance that fixture's counter and that only the matching session can clear the active state.

Focused tests are run after each change. The final gate runs the default parallel Rust suite repeatedly, without `--test-threads=1`, to detect schedule-dependent pollution.

## Verification

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync::engine::tests::
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml project_config::tests::
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
```

Repeat the full default parallel Rust command at least ten times. Every run must report zero failures. Then run:

```bash
pnpm test
pnpm typecheck:test
```

A single-thread Rust pass remains a diagnostic comparison, not an acceptance substitute. Live MinIO tests are outside this isolation fix because the failure is in offline test state; no S3 credentials or network access are required.

## Non-Goals

- Changing production synchronization concurrency or allowing simultaneous engine executions.
- Changing WebDAV or S3 behavior, manifests, conflict policy, or protected paths.
- Changing the lifetime or public semantics of the production editing registry.
- Adding a serial-test dependency or forcing the whole Rust suite to one thread.
- Weakening exact counter assertions when an isolated fixture can make them deterministic.
