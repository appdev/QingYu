# Server Sync Lifecycle Implementation Plan

> **Execution:** Use `superpowers:executing-plans`. Execute proven-safe waves
> through `superpowers:dispatching-parallel-agents` by default; execute dependent
> or unsafe work sequentially. For L3, retain every
> `superpowers:subagent-driven-development` review and fix-loop gate.

**Risk level:** L3 — persistent state initialization, background scheduling, process shutdown, and sync durability interact.

**Goal:** Make a fresh fixed Server expose usable sync configuration, own exactly one scheduler, and drain Kernel sync work within the Server shutdown deadline.

**Architecture:** Atomically seed a disabled v3 sync config only when absent; return concrete fixed-service handles from shared composition; let the Server composition exclusively own the scheduler and shutdown lifecycle; coordinate Axum and Kernel drain under one process deadline.

**Tech stack:** Rust, Tokio, Axum, capability-addressed durable storage.

**Global constraints:**
- Write only under `apps/kernel/**`.
- Do not modify Cargo manifests, lockfiles, Web/App, or Docker files.
- Preserve the existing Unix `0600` inline credential compatibility policy.
- Follow strict Red-Green-Refactor and commit coherent layers separately.

## Execution Strategy

**Default mode:** Sequential — all tasks modify shared Kernel composition and lifecycle interfaces.

**Maximum concurrency:** One implementation agent.

**Wave plan:**
- Wave 1: Task 1 sequentially.
- Wave 2: Task 2 after Task 1.
- Wave 3: Task 3 after Task 2.
- Wave 4: Task 4 after all implementation tasks.

**Controller contract:** The implementation agent runs each focused RED test, implements only the behavior needed for GREEN, commits the layer, then runs the combined Kernel checks and a final review.

**Fallback:** If an interface requires changes outside `apps/kernel/**`, stop at the internal boundary and report the external integration requirement without modifying it.

### Task 1: Atomically initialize fresh sync configuration

**Files:**
- Modify: `apps/kernel/src/sync/config.rs`
- Modify: `apps/kernel/src/composition.rs`
- Test: `apps/kernel/src/sync/config.rs`
- Test: `apps/kernel/src/server/runtime_composition.rs`

**Requirements:** A fresh store receives one disabled default config; valid existing bytes and revision remain unchanged; corrupt or unsupported state fails closed and is never overwritten; fixed composition initializes before publishing services.

**Interfaces:**
- Produces: `SyncConfigStore::initialize_default_if_absent() -> Result<(SyncConfig, Revision), SyncConfigStoreError>` or an equivalently typed non-secret result.

**Depends on:** None.

**Write set:** The four files listed above.

**Shared mutable resources:** Kernel Cargo target only; run tests serially.

**Writable isolation:** Existing P3 integration worktree.

**Parallel-safe:** No — shared composition contract.

**Execution wave:** Wave 1.

**Integration check:** `cargo test --manifest-path apps/kernel/Cargo.toml --locked sync_config` and focused Server composition tests.

- [ ] Add store tests for absent, existing, corrupt, and unsupported states; run and observe missing-method failures.
- [ ] Add a fresh Server composition test whose real sync service returns a disabled config; run and observe `SyncConfigAbsent`.
- [ ] Implement atomic absent initialization with `ExpectedFile::Absent` and durable verification.
- [ ] Initialize in fixed composition before service installation; rerun focused tests.
- [ ] Commit the storage/composition layer.

### Task 2: Own one Server scheduler and trigger app launch

**Files:**
- Modify: `apps/kernel/src/composition.rs`
- Modify: `apps/kernel/src/server/runtime_composition.rs`
- Modify: `apps/kernel/src/services/sync_scheduler.rs` only if an ownership-safe observation seam is required.
- Test: `apps/kernel/src/server/runtime_composition.rs`
- Test: `apps/kernel/tests/sync_config_service.rs` only for scheduler behavior not covered internally.

**Requirements:** Shared composition returns the concrete SyncService; Server starts and retains exactly one scheduler after all services install; existing configured state receives one app-launch trigger; config changes continue to wake the interval scheduler; dropping or explicitly closing the Server owner ends the task.

**Interfaces:**
- Produces: `InstalledFixedKernelServices` carrying `Arc<SyncService>`.
- Produces: Server lifecycle ownership of `KernelSyncScheduler`.

**Depends on:** Task 1.

**Write set:** The files listed above.

**Shared mutable resources:** Server runtime composition API and scheduler ownership.

**Writable isolation:** Existing P3 integration worktree.

**Parallel-safe:** No — consumes Task 1 and defines Task 3 lifecycle owner.

**Execution wave:** Wave 2.

**Integration check:** Focused scheduler tests plus Server composition tests.

- [ ] Add a Server composition test proving a second scheduler cannot claim the service; run and observe that the first claim currently succeeds.
- [ ] Add an app-launch observation using a real SyncService with only the external executor replaced; run and observe no trigger.
- [ ] Return concrete handles, start the scheduler once, retain its owner, and issue app-launch.
- [ ] Re-run existing scheduler config-change/interval tests and Server restart tests.
- [ ] Commit the scheduler ownership layer.

### Task 3: Drain Kernel sync during Server shutdown

**Files:**
- Modify: `apps/kernel/src/runtime.rs`
- Modify: `apps/kernel/src/services/sync.rs`
- Modify: `apps/kernel/src/services/sync_scheduler.rs`
- Modify: `apps/kernel/src/server/runtime_composition.rs`
- Modify: `apps/kernel/src/api/auth.rs` or `apps/kernel/src/api/mod.rs` only if the lifecycle owner must travel with `ServerApiActivation`.
- Modify: `apps/kernel/src/bin/qingyu-kernel.rs`
- Test: `apps/kernel/tests/sync_config_service.rs`
- Test: `apps/kernel/src/server/runtime_composition.rs`
- Test: `apps/kernel/src/bin/qingyu-kernel.rs`

**Requirements:** Shutdown closes trigger admission, closes and awaits the scheduler, cooperatively cancels queued/running sync, awaits settlement, then releases runtime locks; Axum and Kernel drain share one 30-second deadline; timeout is distinguishable for safe diagnostics.

**Interfaces:**
- Produces: internal `SyncService` shutdown/drain operation.
- Produces: Server activation/lifecycle split permitting the binary to own an explicit shutdown handle.
- Consumes: existing `SyncCancellation`, run lifecycle notifications, and `KernelSyncScheduler::close`.

**Depends on:** Task 2.

**Write set:** The files listed above.

**Shared mutable resources:** Kernel sync lifecycle state and Server process orchestration.

**Writable isolation:** Existing P3 integration worktree.

**Parallel-safe:** No — shared runtime state machine.

**Execution wave:** Wave 3.

**Integration check:** Focused blocking-executor shutdown tests, binary shutdown tests, then all Kernel tests.

- [ ] Add a blocking-executor test proving shutdown rejects new triggers, signals cancellation, and waits for settlement; run and observe the missing shutdown API.
- [ ] Add a restart-after-drain test proving instance/workspace locks release.
- [ ] Add binary orchestration tests proving HTTP and Kernel futures both drain and one deadline bounds them.
- [ ] Implement close/cancel/wait state transitions and explicit Server lifecycle ownership.
- [ ] Re-run scheduler, sync lifecycle, Server composition, and binary tests.
- [ ] Commit the shutdown layer.

### Task 4: Combined verification and review

**Files:**
- Review only: all changed `apps/kernel/**` files.

**Requirements:** No manifest/lockfile or external package changes; no credentials in debug/events/responses; all old and new Kernel behavior passes.

**Interfaces:** None.

**Depends on:** Tasks 1–3.

**Write set:** Formatting-only changes inside already modified Kernel files if necessary.

**Shared mutable resources:** Kernel Cargo target.

**Writable isolation:** Existing P3 integration worktree.

**Parallel-safe:** No — final integrated gate.

**Execution wave:** Wave 4.

**Integration check:** `cargo test --manifest-path apps/kernel/Cargo.toml --locked --all-targets --all-features --no-fail-fast`, clippy with warnings denied, and fmt check.

- [ ] Run `cargo fmt --manifest-path apps/kernel/Cargo.toml -- --check`.
- [ ] Run all Kernel tests with all targets/features.
- [ ] Run Kernel clippy with `-D warnings`.
- [ ] Review `git diff --check`, changed paths, credential handling, and shutdown ordering.
- [ ] Report layer SHAs and exact verification output.
