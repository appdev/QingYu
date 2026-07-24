# Rust Test State Isolation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the default parallel Rust test suite deterministic by giving each remote-sync execution its own test hooks and each registry semantics test its own editing registry fixture.

**Architecture:** Keep the production `REMOTE_SYNC_EXECUTION_LOCK` and `PROJECT_SYNC_EDITING_REGISTRY`. Split the sync entry point into a lock-owning wrapper and a shared locked core that receives an execution hook context; callback fields and the hook-aware entry point exist only in tests. Extract registry snapshot logic and add a test-only stack-owned fixture that calls the same transition helpers as production.

**Tech Stack:** Rust, Tokio, Tauri v2, Cargo unit tests, pnpm workspace verification.

## Global Constraints

- Do not change production synchronization concurrency, WebDAV/S3 behavior, manifests, conflict policy, or protected paths.
- Do not change the lifetime or public semantics of the production editing registry or global counter.
- Do not add a serial-test dependency or force the Rust suite to one thread.
- Keep test hooks unavailable and non-configurable in production builds.
- Preserve the user-owned untracked `bg.png` file.

## File Map

- Modify `apps/desktop/src-tauri/src/remote_sync/engine.rs`: replace process-global hook slots with a per-execution hook context and migrate race-window tests.
- Modify `apps/desktop/src-tauri/src/project_config/editing.rs`: extract shared snapshot logic and expose a test-only isolated registry fixture.
- Modify `apps/desktop/src-tauri/src/project_config.rs`: run the canonical-root/session semantics test against the isolated fixture.

---

### Task 1: Per-Execution Remote-Sync Test Hooks

**Files:**
- Modify: `apps/desktop/src-tauri/src/remote_sync/engine.rs:1-1038`
- Test: `apps/desktop/src-tauri/src/remote_sync/engine.rs:1558-1990`

**Interfaces:**
- Consumes: existing `REMOTE_SYNC_EXECUTION_LOCK`, `RemoteSyncBackend`, and mutation helper call graph.
- Produces: `RemoteSyncExecutionHooks`, `execute_remote_sync_locked<B>(source_path, backend, hooks)`, and `cfg(test) execute_remote_sync_with_hooks<B>(source_path, backend, hooks)`.

- [ ] **Step 1: Add a failing hook-isolation unit test**

Change the existing test import to `use std::sync::{Arc, Mutex};`, then add this test inside `remote_sync::engine::tests` before defining `RemoteSyncExecutionHooks`:

```rust
#[test]
fn execution_hook_bundles_keep_callbacks_isolated() {
    let first_calls = Arc::new(AtomicUsize::new(0));
    let second_calls = Arc::new(AtomicUsize::new(0));
    let first_counter = Arc::clone(&first_calls);
    let second_counter = Arc::clone(&second_calls);
    let first = super::RemoteSyncExecutionHooks {
        final_replace: Some(Box::new(move |_| {
            first_counter.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })),
        ..Default::default()
    };
    let second = super::RemoteSyncExecutionHooks {
        final_replace: Some(Box::new(move |_| {
            second_counter.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })),
        ..Default::default()
    };

    first.run_final_replace(Path::new("first.md")).unwrap();
    second.run_final_replace(Path::new("second.md")).unwrap();

    assert_eq!(first_calls.load(Ordering::SeqCst), 1);
    assert_eq!(second_calls.load(Ordering::SeqCst), 1);
}
```

- [ ] **Step 2: Run the focused test and verify RED**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml execution_hook_bundles_keep_callbacks_isolated --no-fail-fast
```

Expected: compilation fails because `RemoteSyncExecutionHooks` is not defined. This is the missing isolated hook API; the earlier default-parallel run is the behavioral red evidence for the global-hook bug.

- [ ] **Step 3: Replace global hook slots with an execution context**

Remove the `Mutex` and `OnceLock` test import, the five `*_TEST_HOOK` statics, and the five `set_*_test_hook` functions. Keep the existing callback aliases and add:

```rust
#[derive(Default)]
struct RemoteSyncExecutionHooks {
    #[cfg(test)]
    atomic_replace: Option<AtomicReplaceTestHook>,
    #[cfg(test)]
    upload_validated: Option<UploadValidatedTestHook>,
    #[cfg(test)]
    final_replace: Option<FinalMutationTestHook>,
    #[cfg(test)]
    final_delete: Option<FinalMutationTestHook>,
    #[cfg(test)]
    quarantine_restore: Option<QuarantineRestoreTestHook>,
}

impl RemoteSyncExecutionHooks {
    fn run_atomic_replace(&self, path: &Path) -> Result<(), String> {
        #[cfg(test)]
        if let Some(hook) = self.atomic_replace.as_ref() {
            hook(path)?;
        }
        Ok(())
    }

    fn run_upload_validated(&self, path: &Path) -> Result<(), String> {
        #[cfg(test)]
        if let Some(hook) = self.upload_validated.as_ref() {
            hook(path)?;
        }
        Ok(())
    }

    fn run_final_replace(&self, path: &Path) -> Result<(), String> {
        #[cfg(test)]
        if let Some(hook) = self.final_replace.as_ref() {
            hook(path)?;
        }
        Ok(())
    }

    fn run_final_delete(&self, path: &Path) -> Result<(), String> {
        #[cfg(test)]
        if let Some(hook) = self.final_delete.as_ref() {
            hook(path)?;
        }
        Ok(())
    }

    fn run_quarantine_restore(&self) -> Result<(), String> {
        #[cfg(test)]
        if let Some(hook) = self.quarantine_restore.as_ref() {
            hook()?;
        }
        Ok(())
    }
}
```

Place `let _ = path;` at the start of every path-taking hook method so non-test builds remain warning-free.

- [ ] **Step 4: Split the sync entry point and thread hooks through mutation helpers**

Rename the current `execute_remote_sync` implementation to `execute_remote_sync_locked`, add `hooks: &RemoteSyncExecutionHooks`, remove only its first lock-acquisition line, and otherwise retain its current body while passing `hooks` into the mutation helpers named below. Insert these two lock-owning wrappers before the renamed implementation:

```rust
pub(crate) async fn execute_remote_sync<B: RemoteSyncBackend>(
    source_path: &Path,
    backend: &B,
) -> Result<RemoteSyncSummary, String> {
    let _execution_guard = REMOTE_SYNC_EXECUTION_LOCK.lock().await;
    let hooks = RemoteSyncExecutionHooks::default();
    execute_remote_sync_locked(source_path, backend, &hooks).await
}

#[cfg(test)]
async fn execute_remote_sync_with_hooks<B: RemoteSyncBackend>(
    source_path: &Path,
    backend: &B,
    hooks: RemoteSyncExecutionHooks,
) -> Result<RemoteSyncSummary, String> {
    let _execution_guard = REMOTE_SYNC_EXECUTION_LOCK.lock().await;
    execute_remote_sync_locked(source_path, backend, &hooks).await
}
```

Add `hooks: &RemoteSyncExecutionHooks` to `read_local_upload_bytes`, `write_download_atomically`, `delete_local_file`, `quarantine_and_verify_sync_target`, and `restore_quarantined_sync_target`. Replace each global-slot lookup with the matching `run_*` call. Pass `hooks` from the locked execution loop through every call, including both conflict/download writes and both quarantine restoration paths. Update the direct `delete_local_file` unit test to pass `&RemoteSyncExecutionHooks::default()`.

- [ ] **Step 5: Migrate all eight race-window tests to owned bundles**

For each test, construct the bundle immediately before its engine call and replace setter/call/clear triples with one call:

```rust
let hooks = super::RemoteSyncExecutionHooks {
    final_replace: Some(Box::new(move |_| {
        fs::remove_file(&note_for_hook).map_err(|error| error.to_string())?;
        fs::write(&note_for_hook, b"user-replacement").map_err(|error| error.to_string())
    })),
    ..Default::default()
};
let result = super::execute_remote_sync_with_hooks(&root, &backend, hooks).await;
```

Apply the matching field to these tests:

- `rejects_upload_when_validated_note_becomes_a_secret_symlink`: `upload_validated`.
- `rejects_replace_after_final_check_without_clobbering_replacement`: `final_replace`.
- `rejects_remote_create_when_name_appears_at_final_publish_and_cleans_staging`: `final_replace`.
- `restores_directory_replacement_after_final_check_without_exposing_staging`: `final_replace`.
- `preserves_new_occupant_and_retains_captured_directory_in_protected_staging`: `final_replace` and `quarantine_restore` in one bundle.
- `preserves_regular_file_occupant_and_retains_captured_file_in_protected_staging`: `final_replace` and `quarantine_restore` in one bundle.
- `rejects_delete_after_final_check_without_deleting_replacement`: `final_delete`.
- `rejects_remote_download_when_parent_becomes_symlink_escape_during_replace`: `atomic_replace`.

Retain every existing filesystem, manifest, backend, staging, and error assertion.

- [ ] **Step 6: Format and verify GREEN for the engine**

Run:

```bash
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync::engine::tests::
```

Expected: all 22 engine tests pass under the default scheduler, including the new bundle-isolation test.

- [ ] **Step 7: Commit the hook isolation**

```bash
git add apps/desktop/src-tauri/src/remote_sync/engine.rs
git commit -m "test(sync): isolate engine race hooks"
```

---

### Task 2: Isolated Editing Registry Fixture

**Files:**
- Modify: `apps/desktop/src-tauri/src/project_config/editing.rs:80-185`
- Modify: `apps/desktop/src-tauri/src/project_config.rs:16-20,383-444`
- Test: `apps/desktop/src-tauri/src/project_config.rs:383-444`

**Interfaces:**
- Consumes: `ProjectSyncEditingRegistry`, `canonical_editing_root`, and `apply_editing_transition`.
- Produces: `cfg(test) ProjectSyncEditingTestRegistry::{load,set}` and shared `editing_snapshot`.

- [ ] **Step 1: Rewrite the semantics test against the missing fixture**

Import `ProjectSyncEditingTestRegistry`, create `let mut registry = ProjectSyncEditingTestRegistry::default();`, and replace the three calls to `set_project_sync_editing_state` plus the one call to `load_project_sync_editing_state` in `editing_registry_uses_canonical_root_and_only_matching_inactive_clears`:

```rust
let active = registry
    .set(alias.to_str().unwrap(), true, "settings-active", Some("rev-1"))
    .unwrap();
let mismatched = registry
    .set(root.to_str().unwrap(), false, "settings-stale", Some("rev-2"))
    .unwrap();
let loaded = registry.load(alias.to_str().unwrap()).unwrap();
let inactive = registry
    .set(root.to_str().unwrap(), false, "settings-active", Some("rev-2"))
    .unwrap();
```

Keep the exact `loaded.counter == mismatched.counter` assertion and every session/root/state assertion.

- [ ] **Step 2: Run the focused test and verify RED**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml editing_registry_uses_canonical_root_and_only_matching_inactive_clears --no-fail-fast
```

Expected: compilation fails because `ProjectSyncEditingTestRegistry` is not defined.

- [ ] **Step 3: Extract snapshot logic and implement the fixture**

Add the shared pure snapshot helper:

```rust
fn editing_snapshot(
    registry: &ProjectSyncEditingRegistry,
    root: &Path,
) -> ProjectSyncEditingSnapshot {
    ProjectSyncEditingSnapshot {
        counter: registry.counter,
        pending_apply: registry
            .applies
            .get(root)
            .map(|entry| entry.public.clone()),
        state: registry.states.get(root).cloned(),
    }
}
```

Use it from `load_project_sync_editing_state` and the final return of `apply_editing_transition`. Then add:

```rust
#[cfg(test)]
#[derive(Default)]
pub(crate) struct ProjectSyncEditingTestRegistry {
    registry: ProjectSyncEditingRegistry,
}

#[cfg(test)]
impl ProjectSyncEditingTestRegistry {
    pub(crate) fn load(
        &self,
        root_path: &str,
    ) -> Result<ProjectSyncEditingSnapshot, String> {
        let root = canonical_editing_root(root_path)?;
        Ok(editing_snapshot(&self.registry, &root))
    }

    pub(crate) fn set(
        &mut self,
        root_path: &str,
        active: bool,
        session_id: &str,
        revision: Option<&str>,
    ) -> Result<ProjectSyncEditingSnapshot, String> {
        let root = canonical_editing_root(root_path)?;
        let canonical_root = root.to_string_lossy().to_string();
        apply_editing_transition(
            &mut self.registry,
            &root,
            &canonical_root,
            active,
            session_id,
            revision,
        )
    }
}
```

Do not change `PROJECT_SYNC_EDITING_REGISTRY`, `advance_counter`, or any production wrapper signature.

- [ ] **Step 4: Format and verify GREEN for project configuration**

Run:

```bash
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml project_config::tests::
```

Expected: every project-config test passes under the default scheduler and the isolated fixture retains exact counter assertions.

- [ ] **Step 5: Commit the registry isolation**

```bash
git add apps/desktop/src-tauri/src/project_config.rs apps/desktop/src-tauri/src/project_config/editing.rs
git commit -m "test(sync): isolate editing registry fixture"
```

---

### Task 3: Parallel Stability and Workspace Regression Gate

**Files:**
- Verify only; no additional source file is expected.

**Interfaces:**
- Consumes: completed Task 1 and Task 2 commits.
- Produces: fresh evidence that default parallel execution is stable without a single-thread workaround.

- [ ] **Step 1: Check formatting and diff hygiene**

Run:

```bash
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
git diff --check
git status --short
```

Expected: formatting and diff checks exit zero; status shows only the user-owned `?? bg.png` if no other unrelated changes exist.

- [ ] **Step 2: Repeat the full default-parallel Rust suite ten times**

Run:

```bash
for run in {1..10}; do
  echo "parallel Rust run ${run}/10"
  cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml || exit 1
done
```

Expected for every run: `385 passed; 0 failed; 21 ignored`, always with zero failures. Do not pass `--test-threads=1`.

- [ ] **Step 3: Run workspace JavaScript and type-test regressions**

Run:

```bash
pnpm test
pnpm typecheck:test
```

Expected: both commands exit zero.

- [ ] **Step 4: Review final scope**

Run:

```bash
git show --stat --oneline HEAD~2..HEAD
git diff HEAD~2 -- apps/desktop/src-tauri/src/remote_sync/engine.rs apps/desktop/src-tauri/src/project_config.rs apps/desktop/src-tauri/src/project_config/editing.rs
```

Confirm that the diff removes global test hook storage, adds per-execution bundles, isolates only the registry semantics fixture, and contains no production sync-policy change.
