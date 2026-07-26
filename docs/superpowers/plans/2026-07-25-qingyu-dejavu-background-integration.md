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
construct per-repo `{repo,history,temp}` paths, construct `S3Cloud`, then call
`Repo::sync` exactly once per attempt with the Tauri `WorkingTreeCoordinator`.
Do not call `Repo::index` first: the core sync state machine indexes the current
working tree while holding its own lifecycle and repository operation guards.
Retry a typed `WorkingTreeChanged` with at most three complete sync attempts;
after the third, queue one normal follow-up job instead of looping.

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
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/Cargo.lock`
- Modify: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/cloud/mod.rs`
- Modify: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/cloud/s3.rs`
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

**Implementation correction:** Tokio paused-time tests require this package to
enable Tokio's `test-util` feature. DNS classification must be produced by the
S3 transport as an explicit `CloudError` category before the application layer
redacts it; the scheduler must not infer DNS from display strings. The runtime
may manage and call an uninstalled scheduler owner in this task, but Task 5 is
still responsible for installing the real service/coordinator/scheduler graph.
Do not expose a user command, use a no-op working-tree coordinator, or cut S3
traffic over from the existing service in this task. Until the later wire
cutover expands the public trigger schema, scheduler `Exit` maps internally to
the existing `SyncTrigger::SettingsExit` value; it is not implemented through a
React settings-window hook.

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
The transport classifier must have direct typed/error-chain evidence that name
resolution failed; generic connect, timeout, TLS, HTTP, authentication, lock,
decrypt, integrity, and unsafe-path failures remain distinct.

- [ ] **Step 5: Run tests and verify GREEN**

Run scheduler, watcher, app-exit, and full Rust tests. Expected: no real test
sleeps and no interval job for inactive repositories.

- [ ] **Step 6: Commit scheduling**

```bash
git add apps/desktop/src-tauri/Cargo.toml apps/desktop/src-tauri/Cargo.lock apps/desktop/src-tauri/crates/qingyu-dejavu/src/cloud/mod.rs apps/desktop/src-tauri/crates/qingyu-dejavu/src/cloud/s3.rs apps/desktop/src-tauri/src/dejavu_sync apps/desktop/src-tauri/src/watcher.rs apps/desktop/src-tauri/src/app_exit.rs apps/desktop/src-tauri/src/desktop_runtime.rs apps/desktop/src-tauri/src/mobile_runtime.rs
git commit -m "feat(sync): schedule Dejavu background synchronization"
```

### Task 5: Coordinate only affected editor paths during remote apply

**Files:**
- Create: `apps/desktop/src-tauri/src/dejavu_sync/path_guard.rs`
- Modify: `apps/desktop/src-tauri/src/dejavu_sync/repository.rs`
- Modify: `apps/desktop/src-tauri/src/dejavu_sync/commands.rs`
- Modify: `apps/desktop/src-tauri/src/dejavu_sync.rs`
- Modify: `apps/desktop/src-tauri/src/desktop_runtime.rs`
- Modify: `apps/desktop/src-tauri/src/mobile_runtime.rs`
- Modify: `apps/desktop/src-tauri/src/builder_boundary_tests.rs`
- Create: `packages/app/src/lib/sync-path-events.ts`
- Create: `packages/app/src/lib/sync-path-events.test.ts`
- Create: `packages/app/src/hooks/useSyncPathGuard.ts`
- Create: `packages/app/src/hooks/useSyncPathGuard.test.tsx`
- Modify: `packages/app/src/hooks/useMarkdownDocument.ts`
- Modify: `packages/app/src/hooks/useMarkdownDocument.test.tsx`
- Modify: `packages/app/src/App.tsx`
- Create: `apps/desktop/src/runtime/tauri/sync-path-guard.ts`
- Modify: `apps/desktop/src/runtime/desktop.ts`
- Modify: `apps/desktop/src/runtime/mobile.ts`
- Modify: `packages/app/src/runtime/index.ts`

**Interfaces:**
- Produces: Tauri `WorkingTreeCoordinator` implementation.
- Produces: an attempt-bound coordinator factory in the product adapter; the
  Dejavu core trait remains `prepare(changes)` and receives no QingYu job/root
  context.
- Produces: request/ack/release event contract keyed by request ID.
- Produces: `guardedPaths: ReadonlySet<string>` for editor and file actions.
- Produces: the first installed internal Dejavu service graph, still without a
  product sync command or replacement of the existing S3/WebDAV route.

**Boundary correction:**

`WorkingTreeCoordinator::prepare` deliberately knows only the planned Dejavu
changes. `jobId` and `notesRoot` come from `SyncAttemptContext`, so
`DejavuRepositoryRunner` must request one coordinator per attempt from a
product-layer factory and pass that coordinator to `Repo::sync`. Do not add
Tauri window, QingYu job, or repository-root parameters to the core trait.
Static fake/no-op coordinators remain available only through test factories.

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
late acknowledgements, acknowledgement from a non-owner window, and release for
another request. The frontend stores paths per request ID and derives the
guarded set as their union so overlapping requests release independently.

- [ ] **Step 2: Run event/hook tests and verify RED**

```bash
pnpm --filter @markra/app exec vitest run src/lib/sync-path-events.test.ts src/hooks/useSyncPathGuard.test.tsx
```

- [ ] **Step 3: Implement backend prepare/release**

Create the coordinator from `SyncAttemptContext` so its event contains that
attempt's `jobId` and canonical `notesRoot`. For the root currently owned by the
primary editor, emit `qingyu://sync-path-guard-request`, wait up to 15 seconds
for the matching acknowledgement, and return a permit containing the request
ID. A root that was never editor-owned may use a product-layer no-op permit; if
ownership changes after an active attempt starts, fail coordination instead of
silently applying to the former root. On timeout, listener loss, invalid/late
ack, cancellation, or emit failure, return a safe coordination error without
applying remote changes and emit release cleanup for any published request.
Permit release emits `qingyu://sync-path-guard-release` exactly once on every
success/error path.

After this coordinator exists, install the real graph during desktop and mobile
Tauri setup in this order: Tauri coordinator factory ->
`DejavuRepositoryRunner` -> `RepositoryStatusStore`/service -> scheduler and
lifecycle -> both owners. Register the acknowledgement command on both
platforms and add builder-boundary tests. Setup must finish before the existing
startup trigger is consumed. Do not expose a new manual product command and do
not cut over `sync_application` in this task.

- [ ] **Step 4: Implement frontend flush and guarded state**

Add a path-filtered save primitive to `useMarkdownDocument`; it snapshots and
saves only dirty open tabs whose exact canonical paths are requested, using the
existing `applySavedCurrentDocument` generation/content check so an edit made
while native I/O is pending remains dirty. The request handler repeats the
snapshot/save check until every matching tab is clean at a stable generation,
then synchronously records that request's guarded paths and acknowledges. A
save failure never acknowledges. This avoids both saving unrelated tabs and
losing edits made during the flush.

Do not mutate the existing global `readOnlyMode`. Derive read-only separately
for each main/side editor from its exact path. Create, rename, move, and delete
reject only operations whose source, destination, parent, or subtree intersects
a guarded path, with a localized non-blocking “正在同步此文件” notice; unrelated
documents and folders remain usable. Release removes only that request's path
set, leaving paths guarded by another request intact.

- [ ] **Step 5: Add concurrency regression tests**

Prove an unrelated document remains editable, creatable, and deletable while
one path is guarded; only matching dirty tabs are saved before ack; an edit
during an awaited save is re-saved or leaves the request unacknowledged; a
folder mutation intersecting a guarded descendant is blocked; overlapping
request releases do not unlock early; external file replacement causes core
recheck/replan; ownership changes abort apply; and release restores the editor
after success or failure.

- [ ] **Step 6: Run tests and verify GREEN**

Run focused `useMarkdownDocument`, event/hook, Rust path-guard, repository graph,
and builder-boundary tests, followed by `pnpm typecheck:test` and full Rust
tests. Expected: no global read-only state, no no-op production coordinator, no
stale remote overwrite, and no change to the public sync command route.

- [ ] **Step 7: Commit path coordination**

```bash
git add apps/desktop/src-tauri/src/dejavu_sync apps/desktop/src-tauri/src/dejavu_sync.rs apps/desktop/src-tauri/src/desktop_runtime.rs apps/desktop/src-tauri/src/mobile_runtime.rs apps/desktop/src-tauri/src/builder_boundary_tests.rs apps/desktop/src/runtime/tauri/sync-path-guard.ts apps/desktop/src/runtime/desktop.ts apps/desktop/src/runtime/mobile.ts packages/app/src/lib/sync-path-events.ts packages/app/src/lib/sync-path-events.test.ts packages/app/src/hooks/useSyncPathGuard.ts packages/app/src/hooks/useSyncPathGuard.test.tsx packages/app/src/hooks/useMarkdownDocument.ts packages/app/src/hooks/useMarkdownDocument.test.tsx packages/app/src/App.tsx packages/app/src/runtime/index.ts
git commit -m "feat(sync): coordinate Dejavu writes by affected path"
```

### Task 6: Implement unified bind, enable, and restore acceptance

**Files:**
- Modify: `apps/desktop/src-tauri/src/dejavu_sync/commands.rs`
- Modify: `apps/desktop/src-tauri/src/dejavu_sync/repository.rs`
- Modify: `apps/desktop/src-tauri/src/dejavu_sync/local_state.rs`
- Modify: `apps/desktop/src-tauri/src/dejavu_sync/service.rs`
- Modify: `apps/desktop/src-tauri/src/dejavu_sync.rs`
- Modify: `apps/desktop/src-tauri/src/desktop_runtime.rs`
- Modify: `apps/desktop/src-tauri/src/mobile_runtime.rs`
- Modify: `apps/desktop/src-tauri/src/builder_boundary_tests.rs`
- Test: focused Rust tests in `dejavu_sync/commands.rs`,
  `dejavu_sync/local_state.rs`, `dejavu_sync/repository.rs`, and
  `dejavu_sync/service.rs`

**Interfaces:**
- Produces: command `bind_dejavu_repository` returning `AcceptedSyncJob`.
- Produces: one `BindRepositoryRequest { notes_root, repository_id, display_name }`.
- Preserves: empty, existing, and restore directories all invoke the same adapter.

**Boundary correction:**

`S3RepositoryCatalog` already exists in the `qingyu-dejavu` crate. Binding reads
and validates the selected repository's existing `metadata.json`; it does not
create, rename, or infer a remote repository. The remote display name is
authoritative and a stale request must fail instead of persisting stale metadata.

Serialize the complete local binding transaction in the installed product
owner: canonicalize and capability-check the root, ensure the empty
`/.qingyu/syncignore` when absent, atomically persist the binding, then enqueue.
An exact repository/root retry is idempotent and re-enables the binding; the
same root with another repository ID or the same repository ID with another
root is rejected. This lock also prevents simultaneous bind requests from
losing one another's `local-sync.json` update. Register the internal command on
desktop and mobile, but do not route the existing public S3/WebDAV sync command
through it until Plan 4.

- [ ] **Step 1: Add failing binding matrix tests**

Cover empty local/empty remote, nonempty local/empty remote, empty local/nonempty
remote, nonempty local/nonempty remote with independent paths, and same-path
conflict. Assert every case calls the same `bind_and_sync` function, creates an
empty `/.qingyu/syncignore` when absent, initializes local key state when
absent, records the binding before enqueue, and returns before a blocked job
completes. Also cover exact idempotent retry, both duplicate-conflict directions,
stale display metadata, and concurrent distinct bindings without a lost update.

- [ ] **Step 2: Run binding tests and verify RED**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib dejavu_sync::repository::tests::bind_
```

- [ ] **Step 3: Implement the single command path**

Validate repository metadata through `S3RepositoryCatalog`, canonicalize and
capability-check the local root, reject a root already bound to another ID,
write or idempotently re-enable the binding atomically, then enqueue. Do not
branch on whether the UI called the operation “enable” or “restore”; that text
is not part of the request. Do not create catalog metadata from this command.

- [ ] **Step 4: Add restart and listener-loss tests**

Persist a blocked accepted job, remove the simulated Settings listener, allow
the job to complete, and assert status/history are correct. Restart the service
from the same `local-sync.json` and verify it can load the binding and run a
manual sync without recreating or rereading repository metadata. Add desktop
and mobile command-registration boundary assertions.

- [ ] **Step 5: Run tests and verify GREEN**

Run binding/service tests and the Rust workspace. Expected: no command holds a
modal response open for the duration of network synchronization.

- [ ] **Step 6: Commit unified binding**

```bash
git add apps/desktop/src-tauri/src/dejavu_sync apps/desktop/src-tauri/src/dejavu_sync.rs apps/desktop/src-tauri/src/desktop_runtime.rs apps/desktop/src-tauri/src/mobile_runtime.rs apps/desktop/src-tauri/src/builder_boundary_tests.rs
git commit -m "feat(sync): unify Dejavu bind and restore jobs"
```

### Task 7: Add startup recovery, retention, reset, and manual remote maintenance

**Files:**
- Create: `apps/desktop/src-tauri/src/dejavu_sync/maintenance.rs`
- Modify: `apps/desktop/src-tauri/src/dejavu_sync.rs`
- Modify: `apps/desktop/src-tauri/src/dejavu_sync/service.rs`
- Modify: `apps/desktop/src-tauri/src/dejavu_sync/commands.rs`
- Modify: `apps/desktop/src-tauri/src/dejavu_sync/local_state.rs`
- Modify: `apps/desktop/src-tauri/src/dejavu_sync/status.rs`
- Modify: `apps/desktop/src-tauri/src/desktop_runtime.rs`
- Modify: `apps/desktop/src-tauri/src/mobile_runtime.rs`
- Modify: `apps/desktop/src-tauri/src/builder_boundary_tests.rs`
- Modify: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/repo.rs`
- Modify: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/purge.rs`

**Interfaces:**
- Produces: `rebuild_local_repository`, `stop_repository_sync`, `change_global_key`, `purge_remote_repository`, `delete_remote_repository`.
- Produces: startup cleanup and daily local maintenance.
- Produces: accepted background maintenance jobs; no long-running command keeps a
  Settings invoke pending.
- Preserves: every `local-sync.json` writer uses the service-owned
  `LocalStateTransaction`; key replacement uses
  `with_global_key_state_transaction` and no second state mutex.

- [ ] **Step 1: Write failing maintenance boundary tests**

Assert startup removes only exact `stage-[0-9a-f]{40}.tmp` files from owned,
non-recursive parents: each repository `temp/`, repository root (for
`state.json` publication), app-data root (for `local-sync.json` publication),
and the bound root's direct `.qingyu/`. Harden `Repo::open` to the same exact
stage-name predicate. Never delete an entire `temp/`, any invented `temp/repo`,
an arbitrary user `*.tmp`, `repo/`, `history/`, a note file, or a matching name
outside those exact parents.

Port SiYuan's pinned retention selection literally: discard indexes older than
180 days; group the remaining indexes by the machine's local calendar date;
retain every index from today; for each older date retain the newest index plus
up to `RetentionIndexesDaily * 7` random selections from SiYuan's same
`[0, len - 1)` range, stopping once two unique indexes are retained. This
intentionally preserves the pinned implementation's exclusion of the oldest
entry from random selection and its possibility of retaining only one after
repeated duplicate draws. Inject the random selector in tests so the production
policy remains SiYuan-compatible without flaky assertions. Skip purge when the
resulting retained-ID set has fewer than three entries. Cover first successful
non-exit sync per repository, six-hour minimum, the later daily attempt, and
12-hour cooperative cancellation.

- [ ] **Step 2: Run maintenance tests and verify RED**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib dejavu_sync::maintenance::tests
```

- [ ] **Step 3: Implement local cleanup and conflict-history retention**

Call core `Repo::purge` with calculated retained IDs. Clean sync conflict history
older than 30 days daily without touching existing document-edit history.
Record last purge and next due time per repository in the existing `state.json`
through `RepositoryStatusStore`; normal sync status writes must preserve these
maintenance fields instead of replacing them. Run the blocking purge through
`spawn_blocking` with the same cancellation flag used by the 12-hour timeout.

SiYuan calls its first purge after the first successful non-exit sync and then
allows at most one attempt per six hours; its 24-hour cron does nothing until
that first success. Apply the same rule independently per QingYu repository.

- [ ] **Step 4: Port Dejavu remote purge into the core crate**

Add `Repo::purge_cloud(cloud, cancelled)` from the pinned Go
`Repo.PurgeCloud`. The core method, not the Tauri maintenance layer, owns the
repository-format traversal: acquire `lock-sync`; list `objects/`, `indexes/`,
and `refs/`; treat every ref target as a retained index; load its encrypted
index and file metadata; compute reachable file/chunk objects; remove
unreferenced indexes, legacy check indexes, stale `indexes-v2.json` entries, and
unreferenced objects; then release the remote lock. Preserve Go's empty-list
short circuit and missing/corrupt object behavior. Add LocalCloud oracle-style
tests for reachability, shared chunks, multiple refs, stale indexes-v2,
cancellation before deletion, lock loss, and idempotent rerun. Do not reproduce
object-key, encryption, or reachability logic in `maintenance.rs`.

- [ ] **Step 5: Implement five independent lifecycle commands**

- rebuild deletes only current local `repo/`, recreates it, and indexes notes;
- stop waits for the current per-repository operation, removes the binding,
  clears persisted scheduling state, and deactivates that root; an already
  accepted sync may finish, but no new job may validate afterward;
- key change returns an accepted global maintenance job, enters
  `with_global_key_state_transaction` (global write then the shared state
  transaction), clears every old-key `repo/`, preserves notes/history and the
  device ID, disables all bindings, then writes the Base64/passphrase-derived
  key using the existing SiYuan-compatible derivation;
- remote purge returns an accepted repository maintenance job and calls only
  core `Repo::purge_cloud` while holding the ordinary global-read and
  per-repository service transaction;
- remote delete removes only the explicitly confirmed repository prefix through
  `S3RepositoryCatalog::delete_repository`. It is rejected while that repository
  still has an enabled binding, so deletion cannot silently act as Stop and the
  scheduler cannot recreate an unlisted orphan repository.

Remote purge requires confirmation and remote lock, computes reachability from
all ref-retained remote indexes exactly as pinned Dejavu does, and never runs
from the automatic scheduler. Remote delete follows pinned Dejavu's explicit
manual lifecycle but remains separate from stop in QingYu's multi-repository
model.

All state-changing commands reuse narrow service transactions with this order:
ordinary repository maintenance is `global read -> repository mutex`, adding
`LocalStateTransaction` only for a local-state mutation; key replacement is
`global write -> LocalStateTransaction`. Do not call
`cancel_all_for_shutdown_or_reset` from inside the global key transaction and
do not add another mutex around `local-sync.json`.

- [ ] **Step 6: Add crash-restart tests**

Interrupt after temporary write, object upload, index upload, and before ref
publication. Restart cleanup, index, and sync; assert convergence without WAL or
scanning every ref for an invented full restore. A key-change interruption may
leave only some local `repo/` directories cleared; restart must safely rebuild
them from their note roots after the new state is committed, and must never
touch notes or history. Remote upload/index/ref interruption remains a normal
Dejavu convergence test, not a startup-cleanup scan of remote storage.

- [ ] **Step 7: Run tests and verify GREEN**

Run maintenance/crash tests and the entire Rust workspace. Expected: each reset
operation changes only its documented target, every long operation is accepted
before it completes, and no listener/window lifetime owns the job.

- [ ] **Step 8: Commit maintenance behavior**

```bash
git add apps/desktop/src-tauri/src/dejavu_sync apps/desktop/src-tauri/src/dejavu_sync.rs apps/desktop/src-tauri/src/desktop_runtime.rs apps/desktop/src-tauri/src/mobile_runtime.rs apps/desktop/src-tauri/src/builder_boundary_tests.rs apps/desktop/src-tauri/crates/qingyu-dejavu/src/repo.rs apps/desktop/src-tauri/crates/qingyu-dejavu/src/purge.rs
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
