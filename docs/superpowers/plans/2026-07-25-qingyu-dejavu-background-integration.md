# QingYu Dejavu Background Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Integrate the validated Dejavu Rust and S3 crates into QingYu as a window-independent, per-repository background service with local keys, unified bind/restore behavior, SiYuan-derived scheduling, path-level edit coordination, cleanup, and reset operations.

**Architecture:** Keep the existing WebDAV engine active while introducing `dejavu_sync` as a new S3-only application service. Store global key/device identity and directory bindings in local non-synced JSON, queue jobs in Tauri-managed state, use one mutex per repository, and bridge only affected paths to the frontend during the short apply phase.

**Tech Stack:** Rust, Tokio, Tauri v2 managed state/events/commands, notify watcher, React hooks, TypeScript, Vitest, the `qingyu-dejavu` crate from Plans 1-2.

## Global Constraints

- Complete the core-port and S3-interoperability plans first; product code must depend on those public APIs instead of duplicating them.
- One note root maps to one repository ID; repository tasks are serialized per repository and independent repositories may run concurrently.
- The repository key and device ID live in `<app-data>/local-sync.json`, never in Keychain, the note directory, logs, events, or S3.
- `bind_and_sync(local_notes_root, remote_repository_id)` is the only core workflow for enable and restore.
- Accepted background jobs outlive settings and React component listeners; no sync task is owned by a window.
- Only paths about to receive a remote write/delete enter a short guarded state; unrelated documents remain fully usable.
- Preserve WebDAV behavior and code paths in this milestone. Do not route S3 UI to the new service until Plan 4's cutover task.
- Do not add automatic remote purge, Markdown merge, a global read-only overlay, WAL, or network-step resume.
- Do not use the TypeScript `void` operator.

---

## Planned File Structure

```text
apps/desktop/src-tauri/src/
  dejavu_sync.rs                     # module exports and Tauri registration
  dejavu_sync/
    commands.rs                      # narrow command boundary
    local_state.rs                   # local-sync.json
    path_guard.rs                    # Tauri WorkingTreeCoordinator
    repository.rs                    # bind_and_sync adapter
    scheduler.rs                     # modes, backoff, active-root scheduling
    service.rs                       # job registry and per-repo locks
    status.rs                        # per-repo state.json and events
    maintenance.rs                   # startup cleanup, purge, reset operations
  sync_config/model.rs               # version 3 mode/interval contract
  sync_config/status.rs              # background phases and conflict records
  watcher.rs                         # active-root change scheduling
  app_exit.rs                        # exit trigger
  desktop_runtime.rs                 # managed service startup
packages/app/src/
  hooks/useSyncPathGuard.ts          # flush/guard/release handshake
  hooks/useSyncPathGuard.test.tsx
  lib/sync-config.ts                 # matching TypeScript schema
  lib/sync-path-events.ts            # typed event contract
  lib/sync-path-events.test.ts
  App.tsx                            # install guard and per-path read-only state
```

### Task 1: Add local key, device, and repository binding storage

**Files:**
- Create: `apps/desktop/src-tauri/src/dejavu_sync.rs`
- Create: `apps/desktop/src-tauri/src/dejavu_sync/local_state.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/Cargo.lock`

**Interfaces:**
- Produces: `LocalSyncStateService` and versioned `LocalSyncState`.
- Produces: `RepositoryBinding` keyed by repository ID with one canonical note root.
- Consumes: `qingyu_dejavu::{derive_key, write_file_safer}`.

- [ ] **Step 1: Add the path dependency and failing local-state tests**

Add:

```toml
qingyu-dejavu = { path = "crates/qingyu-dejavu" }
```

Test this exact local-only schema:

```rust
#[derive(Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LocalSyncState {
    pub(crate) version: u32,
    pub(crate) device_id: String,
    pub(crate) repo_key: String,
    pub(crate) bindings: Vec<RepositoryBinding>,
}

#[derive(Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RepositoryBinding {
    pub(crate) repository_id: String,
    pub(crate) display_name: String,
    pub(crate) notes_root: PathBuf,
    pub(crate) enabled: bool,
}
```

Cover absent-file initialization, 32-byte Base64 import, passphrase derivation,
random key generation, duplicate repository/root rejection, malformed/oversized
state, symlink/non-file destination rejection, and atomic replacement.

- [ ] **Step 2: Run tests and verify RED**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib dejavu_sync::local_state::tests
```

Expected: compile failure because the service is absent.

- [ ] **Step 3: Implement key and state behavior**

Use `version: 1`, a UUID v4 device ID, and standard Base64. For a user key input:

1. if standard Base64 decodes to exactly 32 bytes, use it directly;
2. otherwise compute lowercase SHA-256 hex of the passphrase;
3. take its first 16 ASCII characters as salt;
4. call core `derive_key(passphrase, salt)`.

Write pretty JSON plus newline with the existing safe app-data directory
capability checks. Redact `repo_key` from every `Debug` implementation.

- [ ] **Step 4: Run tests and verify GREEN**

Run local-state tests and the full Rust workspace. Expected: no key appears in
failure text, and interrupted writes preserve the prior valid file.

- [ ] **Step 5: Commit local identity storage**

```bash
git add apps/desktop/src-tauri/Cargo.toml apps/desktop/src-tauri/Cargo.lock apps/desktop/src-tauri/src/dejavu_sync.rs apps/desktop/src-tauri/src/dejavu_sync apps/desktop/src-tauri/src/lib.rs
git commit -m "feat(sync): store Dejavu local identity and bindings"
```

### Task 2: Introduce the version 3 scheduling contract without migrating remote data

**Files:**
- Modify: `apps/desktop/src-tauri/src/sync_config/model.rs`
- Modify: `apps/desktop/src-tauri/src/sync_config/storage.rs`
- Modify: `apps/desktop/src-tauri/src/sync_validation.rs`
- Modify: focused Rust tests in those files
- Modify: `packages/app/src/lib/sync-config.ts`
- Modify: `packages/app/src/lib/sync-config.test.ts`
- Modify: `packages/shared/src/i18n/locales/types.ts`

**Interfaces:**
- Produces: `SyncMode::{Automatic,StartupExit,FullyManual}`.
- Produces: `SyncConfig.version == 3`, `mode`, and `interval_seconds`.
- Preserves: S3/WebDAV provider credentials and connection validation fields.

- [ ] **Step 1: Write failing Rust and TypeScript schema tests**

Replace `autoSyncOnSave` and `intervalMinutes` with:

```rust
#[derive(Clone, Copy, Debug, serde::Deserialize, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SyncMode {
    Automatic,
    StartupExit,
    FullyManual,
}
```

and `interval_seconds: u32`. Assert default automatic/30 seconds, inclusive
range 30 through 43,200 seconds, and version 2 loads as unsupported rather than
being heuristically migrated. Mirror exact names in TypeScript:

```ts
export type SyncMode = "automatic" | "startup-exit" | "fully-manual";
```

- [ ] **Step 2: Run focused schema tests and verify RED**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib sync_config
pnpm --filter @markra/app exec vitest run src/lib/sync-config.test.ts
```

- [ ] **Step 3: Implement version 3 validation and patches**

Set `SYNC_CONFIG_VERSION = 3`. Add patch fields `mode` and `intervalSeconds`;
remove old fields from Rust and TypeScript unions. Preserve provider readiness:
blank S3 region still resolves at runtime, WebDAV still requires its existing
fields, and changing scheduling fields does not alter stored credentials.

- [ ] **Step 4: Update existing coordinator eligibility tests**

Until S3 cutover, map WebDAV triggers as follows: automatic keeps save/interval/
launch/exit/manual behavior, startup-exit keeps launch/exit/manual, and fully
manual keeps manual only. Replace tests that assert the removed boolean/minute
fields with the new mode/seconds contract.

- [ ] **Step 5: Run tests and verify GREEN**

Run Rust sync-config tests, TypeScript sync-config/coordinator tests, and
`pnpm typecheck:test`. Expected: version 2 is safely unsupported and WebDAV
still has an eligible execution path in every supported mode.

- [ ] **Step 6: Commit the scheduling schema**

```bash
git add apps/desktop/src-tauri/src/sync_config apps/desktop/src-tauri/src/sync_validation.rs packages/app/src/lib packages/app/src/hooks/useAppSyncCoordinator.ts packages/app/src/hooks/useAppSyncCoordinator.test.tsx packages/shared/src/i18n/locales/types.ts
git commit -m "feat(sync): define SiYuan-style sync modes"
```

### Task 3: Build the window-independent per-repository job service

**Files:**
- Create: `apps/desktop/src-tauri/src/dejavu_sync/service.rs`
- Create: `apps/desktop/src-tauri/src/dejavu_sync/repository.rs`
- Create: `apps/desktop/src-tauri/src/dejavu_sync/status.rs`
- Create: `apps/desktop/src-tauri/src/dejavu_sync/commands.rs`
- Modify: `apps/desktop/src-tauri/src/dejavu_sync.rs`
- Modify: `apps/desktop/src-tauri/src/desktop_runtime.rs`
- Modify: `apps/desktop/src-tauri/src/mobile_runtime.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`

**Interfaces:**
- Produces: managed `DejavuSyncService`.
- Produces: `enqueue(SyncJobRequest) -> AcceptedSyncJob` and repository status events.
- Produces: `bind_and_sync(local_notes_root, remote_repository_id)` adapter.

- [ ] **Step 1: Add failing service ownership and lock tests**

Use these contracts:

```rust
pub(crate) struct SyncJobRequest {
    pub(crate) notes_root: PathBuf,
    pub(crate) repository_id: String,
    pub(crate) trigger: SyncTrigger,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AcceptedSyncJob {
    pub(crate) job_id: String,
    pub(crate) repository_id: String,
    pub(crate) notes_root: PathBuf,
}
```

Test that enqueue returns before a blocked cloud request completes, dropping the
caller does not cancel the job, two requests for one repository serialize, two
repositories run concurrently, and a global key-change barrier waits for both.

- [ ] **Step 2: Run service tests and verify RED**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib dejavu_sync::service::tests
```

- [ ] **Step 3: Implement the managed job registry**

Store per-repository `Arc<tokio::sync::Mutex<()>>`, job cancellation tokens only
for application shutdown/reset, and one global `RwLock` whose read guard covers
ordinary jobs and write guard covers key change. `enqueue` validates binding,
creates attempting status, spawns an owned Tokio task, and returns acceptance.

- [ ] **Step 4: Implement the repository adapter**

`bind_and_sync` must canonicalize the note root, ensure an empty
`/.qingyu/syncignore` exists through a safe write, load the global key and device,
construct per-repo `{repo,history,temp}` paths, construct `S3Cloud`, call
`Repo::index("[Sync] Cloud sync", true)`, then `Repo::sync`. Retry a typed
`WorkingTreeChanged` at most three immediate re-index passes; after the third,
queue one normal follow-up job instead of looping.

- [ ] **Step 5: Persist per-repository status independent of listeners**

Write `<repo-root>/state.json` with version, phase, trigger, job ID, attempt and
success timestamps, next scheduled time, safe error, transfer summary, and
conflict records. Write before emitting `qingyu://dejavu-sync-status-changed`.
The event contains the same public data and no secrets.

- [ ] **Step 6: Run tests and verify GREEN**

Run service/status tests and the full Rust workspace. Expected: tests can remove
the fake window listener while a job continues to completion and persisted
status remains loadable.

- [ ] **Step 7: Commit the background service**

```bash
git add apps/desktop/src-tauri/src/dejavu_sync.rs apps/desktop/src-tauri/src/dejavu_sync apps/desktop/src-tauri/src/desktop_runtime.rs apps/desktop/src-tauri/src/mobile_runtime.rs apps/desktop/src-tauri/src/lib.rs
git commit -m "feat(sync): add Dejavu background job service"
```

### Task 4: Port SiYuan scheduling, backoff, DNS retry, and active-root behavior

**Files:**
- Create: `apps/desktop/src-tauri/src/dejavu_sync/scheduler.rs`
- Modify: `apps/desktop/src-tauri/src/dejavu_sync/service.rs`
- Modify: `apps/desktop/src-tauri/src/dejavu_sync/status.rs`
- Modify: `apps/desktop/src-tauri/src/watcher.rs`
- Modify: `apps/desktop/src-tauri/src/app_exit.rs`
- Modify: `apps/desktop/src-tauri/src/desktop_runtime.rs`
- Modify: `apps/desktop/src-tauri/src/mobile_runtime.rs`

**Interfaces:**
- Produces: `RepositorySchedule`, `record_file_change`, `activate_root`, `deactivate_root`, `trigger_startup`, `trigger_exit`.
- Consumes: `SyncMode` and `DejavuSyncService::enqueue`.

- [ ] **Step 1: Write paused-time scheduler tests**

Assert:

- automatic accepts file-change/startup/exit/manual;
- startup-exit accepts startup/exit/manual;
- fully-manual accepts manual only;
- file change resets `same_count` to zero and schedules configured interval;
- failures schedule 5 minutes;
- failure count 8 schedules 64 minutes for automatic work while manual bypasses;
- success resets failure count;
- inactive roots do not poll, while an accepted first restore finishes;
- only the current active root receives interval jobs.

For no-change results, assert the exact SiYuan sequence in minutes for successive
counts: `8, 8, 8, 16, 32, 64, 128, 256, 512, 1024, 32`; count 11 is reset to 5
before computing its delay.

- [ ] **Step 2: Run scheduler tests and verify RED**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib dejavu_sync::scheduler::tests
```

- [ ] **Step 3: Implement scheduling and active-root wiring**

Maintain schedule state per repository in `state.json`. Use one Tokio timer task
that wakes for the nearest active due time; never scan closed roots. Feed native
watcher changes into `record_file_change` only after mapping the path to the
active binding. Startup and exit call explicit service methods rather than
depending on mounted React hooks.

- [ ] **Step 4: Implement error and DNS policy**

Classify DNS resolution errors as a typed transport category. Retry the complete
sync once immediately when the last DNS refresh attempt was at least five
minutes ago. Windows may run the existing flush command; other platforms record
a no-op flush and still retry once. Do not retry auth, forbidden, rate-limit
beyond transport policy, decrypt, lock, integrity, or unsafe-path errors as DNS.

- [ ] **Step 5: Run tests and verify GREEN**

Run scheduler, watcher, app-exit, and full Rust tests. Expected: no real test
sleeps and no interval job for inactive repositories.

- [ ] **Step 6: Commit scheduling**

```bash
git add apps/desktop/src-tauri/src/dejavu_sync apps/desktop/src-tauri/src/watcher.rs apps/desktop/src-tauri/src/app_exit.rs apps/desktop/src-tauri/src/desktop_runtime.rs apps/desktop/src-tauri/src/mobile_runtime.rs
git commit -m "feat(sync): schedule Dejavu background synchronization"
```

### Task 5: Coordinate only affected editor paths during remote apply

**Files:**
- Create: `apps/desktop/src-tauri/src/dejavu_sync/path_guard.rs`
- Modify: `apps/desktop/src-tauri/src/dejavu_sync/repository.rs`
- Modify: `apps/desktop/src-tauri/src/dejavu_sync/commands.rs`
- Create: `packages/app/src/lib/sync-path-events.ts`
- Create: `packages/app/src/lib/sync-path-events.test.ts`
- Create: `packages/app/src/hooks/useSyncPathGuard.ts`
- Create: `packages/app/src/hooks/useSyncPathGuard.test.tsx`
- Modify: `packages/app/src/App.tsx`
- Modify: `apps/desktop/src/runtime/tauri/sync-config/shared.ts`
- Modify: `packages/app/src/runtime/index.ts`

**Interfaces:**
- Produces: Tauri `WorkingTreeCoordinator` implementation.
- Produces: request/ack/release event contract keyed by request ID.
- Produces: `guardedPaths: ReadonlySet<string>` for editor and file actions.

- [ ] **Step 1: Define and test the event contract**

Use these payloads:

```ts
export type SyncPathGuardRequest = {
  requestId: string;
  jobId: string;
  notesRoot: string;
  relativePaths: string[];
};

export type SyncPathGuardRelease = {
  requestId: string;
  notesRoot: string;
  relativePaths: string[];
};
```

Add runtime `acknowledgePathGuard({ requestId, notesRoot })`. Tests must reject
duplicate IDs, roots that do not match the primary workspace, traversal paths,
late acknowledgements, and release for another request.

- [ ] **Step 2: Run event/hook tests and verify RED**

```bash
pnpm --filter @markra/app exec vitest run src/lib/sync-path-events.test.ts src/hooks/useSyncPathGuard.test.tsx
```

- [ ] **Step 3: Implement backend prepare/release**

For the active root, emit `qingyu://sync-path-guard-request`, wait up to 15
seconds for the matching acknowledgement, and return a permit containing the
request ID. Inactive roots use a no-op permit because no editor owns those
paths. On timeout, return a safe coordination error without applying remote
changes. Release emits `qingyu://sync-path-guard-release` on every result path.

- [ ] **Step 4: Implement frontend flush and guarded state**

On a valid request, find open tabs under the exact root, save dirty matching
documents, then acknowledge. Add relative paths to `guardedPaths` only after
saves succeed. The active editor is read-only only when its exact path is in the
set. Create, rename, move, and delete actions reject only a guarded target path
with a non-blocking “正在同步此文件” notice. Release removes exactly that request's
paths.

- [ ] **Step 5: Add concurrency regression tests**

Prove an unrelated document remains editable and deletable while one path is
guarded, dirty content is saved before ack, a save during guard is not lost,
external file replacement causes core recheck/replan, and release restores the
editor after success or failure.

- [ ] **Step 6: Run tests and verify GREEN**

Run focused Vitest, Rust path-guard tests, `pnpm typecheck:test`, and full Rust
tests. Expected: no global read-only state and no stale remote overwrite.

- [ ] **Step 7: Commit path coordination**

```bash
git add apps/desktop/src-tauri/src/dejavu_sync packages/app/src/lib/sync-path-events.ts packages/app/src/lib/sync-path-events.test.ts packages/app/src/hooks/useSyncPathGuard.ts packages/app/src/hooks/useSyncPathGuard.test.tsx packages/app/src/App.tsx apps/desktop/src/runtime/tauri/sync-config/shared.ts packages/app/src/runtime/index.ts
git commit -m "feat(sync): coordinate Dejavu writes by affected path"
```

### Task 6: Implement unified bind, enable, and restore acceptance

**Files:**
- Modify: `apps/desktop/src-tauri/src/dejavu_sync/commands.rs`
- Modify: `apps/desktop/src-tauri/src/dejavu_sync/repository.rs`
- Modify: `apps/desktop/src-tauri/src/dejavu_sync/local_state.rs`
- Modify: `apps/desktop/src-tauri/src/dejavu_sync/service.rs`
- Test: focused Rust integration tests in `dejavu_sync/repository.rs`

**Interfaces:**
- Produces: command `bind_dejavu_repository` returning `AcceptedSyncJob`.
- Produces: one `BindRepositoryRequest { notes_root, repository_id, display_name }`.
- Preserves: empty, existing, and restore directories all invoke the same adapter.

- [ ] **Step 1: Add failing binding matrix tests**

Cover empty local/empty remote, nonempty local/empty remote, empty local/nonempty
remote, nonempty local/nonempty remote with independent paths, and same-path
conflict. Assert every case calls the same `bind_and_sync` function, creates an
empty `/.qingyu/syncignore` when absent, records the binding before enqueue, and
returns before a blocked job completes.

- [ ] **Step 2: Run binding tests and verify RED**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib dejavu_sync::repository::tests::bind_
```

- [ ] **Step 3: Implement the single command path**

Validate repository metadata through `S3RepositoryCatalog`, canonicalize and
capability-check the local root, reject a root already bound to another ID,
write the binding atomically, then enqueue. Do not branch on whether the UI
called the operation “enable” or “restore”; that text is not part of the request.

- [ ] **Step 4: Add restart and listener-loss tests**

Persist a blocked accepted job, remove the simulated Settings listener, allow
the job to complete, and assert status/history are correct. Restart the service
from the same `local-sync.json` and verify it can load the binding and run a
manual sync without recreating repository metadata.

- [ ] **Step 5: Run tests and verify GREEN**

Run binding/service tests and the Rust workspace. Expected: no command holds a
modal response open for the duration of network synchronization.

- [ ] **Step 6: Commit unified binding**

```bash
git add apps/desktop/src-tauri/src/dejavu_sync
git commit -m "feat(sync): unify Dejavu bind and restore jobs"
```

### Task 7: Add startup recovery, retention, reset, and manual remote maintenance

**Files:**
- Create: `apps/desktop/src-tauri/src/dejavu_sync/maintenance.rs`
- Modify: `apps/desktop/src-tauri/src/dejavu_sync/service.rs`
- Modify: `apps/desktop/src-tauri/src/dejavu_sync/commands.rs`
- Modify: `apps/desktop/src-tauri/src/dejavu_sync/local_state.rs`
- Modify: `apps/desktop/src-tauri/src/desktop_runtime.rs`
- Modify: `apps/desktop/src-tauri/src/mobile_runtime.rs`

**Interfaces:**
- Produces: `rebuild_local_repository`, `stop_repository_sync`, `change_global_key`, `purge_remote_repository`, `delete_remote_repository`.
- Produces: startup cleanup and daily local maintenance.

- [ ] **Step 1: Write failing maintenance boundary tests**

Assert startup deletes only each repository's `temp/repo` and recognized owned
`.qingyu` staging names, never arbitrary user `*.tmp`, `repo`, `history`, or note
files. Add retention tests for 180 days, all indexes today, two per older day,
no purge under three retained indexes, first-success async purge, six-hour
minimum, daily scheduling, and 12-hour cancellation.

- [ ] **Step 2: Run maintenance tests and verify RED**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib dejavu_sync::maintenance::tests
```

- [ ] **Step 3: Implement local cleanup and conflict-history retention**

Call core `Repo::purge` with calculated retained IDs. Clean sync conflict history
older than 30 days daily without touching existing document-edit history.
Record last purge and next due time per repository in `state.json`.

- [ ] **Step 4: Implement four independent lifecycle commands**

- rebuild deletes only current local `repo/`, recreates it, and indexes notes;
- stop removes enabled binding/schedule only;
- key change takes global write lock, clears every old-key `repo/`, preserves notes/history, disables bindings, then writes the new key;
- remote delete removes only the explicitly confirmed repository prefix.

Remote purge requires confirmation and remote lock, computes reachability from
all retained remote indexes, and never runs from the automatic scheduler.

- [ ] **Step 5: Add crash-restart tests**

Interrupt after temporary write, object upload, index upload, and before ref
publication. Restart cleanup, index, and sync; assert convergence without WAL or
scanning every ref for an invented full restore.

- [ ] **Step 6: Run tests and verify GREEN**

Run maintenance/crash tests and the entire Rust workspace. Expected: each reset
operation changes only its documented target.

- [ ] **Step 7: Commit maintenance behavior**

```bash
git add apps/desktop/src-tauri/src/dejavu_sync apps/desktop/src-tauri/src/desktop_runtime.rs apps/desktop/src-tauri/src/mobile_runtime.rs
git commit -m "feat(sync): add Dejavu repository maintenance"
```

### Task 8: Verify the background-integration milestone

**Files:**
- Modify only files in this plan if verification reveals a scoped defect.

**Interfaces:**
- Produces: an internal S3 background service ready for Plan 4 UI cutover.

- [ ] **Step 1: Run focused Rust and frontend tests**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib dejavu_sync
pnpm --filter @markra/app exec vitest run src/lib/sync-config.test.ts src/lib/sync-path-events.test.ts src/hooks/useSyncPathGuard.test.tsx src/hooks/useAppSyncCoordinator.test.tsx
```

- [ ] **Step 2: Run core and interoperability oracles**

```bash
DEJAVU_SOURCE_DIR=/Volumes/extendData/Data/IdeaProjects/upstream/dejavu pnpm test:dejavu-oracle
pnpm test:dejavu-interop
```

- [ ] **Step 3: Run repository gates**

```bash
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml --all -- --check
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
pnpm test
pnpm typecheck:test
pnpm build
git diff --check
git status --short
```

Expected: background service and path coordination pass, while product S3 still
uses the old route and WebDAV remains unchanged.

- [ ] **Step 4: Commit any verification-only correction**

If required:

```bash
git add apps/desktop/src-tauri packages/app packages/shared package.json
git commit -m "fix(sync): close Dejavu background integration gaps"
```

Do not create an empty commit.

## Self-review

- Local state, config version, service requests, status fields, scheduling modes, no-change delays, retry counts, path events, and maintenance operations have explicit types and tests.
- The accepted-job boundary makes settings/window lifetime irrelevant to synchronization lifetime.
- S3 is integrated behind an internal service, but user-facing S3 routing is intentionally deferred to Plan 4 so this milestone remains independently reviewable.
- WebDAV retains its old remote engine and receives only the new trigger eligibility model.
