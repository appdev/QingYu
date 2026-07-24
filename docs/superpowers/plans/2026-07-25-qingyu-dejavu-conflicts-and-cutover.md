# QingYu Dejavu Conflict UX and S3 Cutover Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expose non-blocking conflict awareness and resolution to users, update Sync settings for the new lifecycle, and cut S3 note-folder synchronization from the legacy manifest engine to the validated Dejavu background service without changing WebDAV or portable-settings behavior.

**Architecture:** Persist conflict records in each repository's local state and expose narrow list/read/resolve commands. Drive a current-file indicator and conflict dialog from backend status events, submit restore as an accepted background job, then route only S3 note data to Dejavu while retaining the legacy engine for WebDAV and portable application settings.

**Tech Stack:** Rust, Tauri v2, React, TypeScript, Tailwind CSS, lucide-react, Vitest, existing QingYu Settings and toast primitives.

## Global Constraints

- Complete the first three plans before starting cutover.
- A sync conflict never waits for user input: local remains active, remote is retained in 30-day local conflict history, and sync finishes.
- The user must see one non-blocking notification and a current-file conflict marker; no full-screen overlay or blocking modal may appear.
- Resolution choices are exactly keep local, use remote, and keep both. None performs automatic Markdown merge.
- S3 note data uses Dejavu after cutover and must never create legacy `remote-conflict` files or per-file manifests.
- WebDAV remains on the legacy engine. Portable `settings.json` synchronization remains on its existing protected settings scope and is not moved into the note repository.
- Settings stays open during catalog and restore flows. A successfully accepted restore closes only the restore dialog, not Settings.
- All user-facing strings are localized and TypeScript must not use the `void` operator.
- Remove legacy S3 note routes only after focused and full regression gates pass.

---

## Planned File Structure

```text
apps/desktop/src-tauri/src/
  dejavu_sync/conflicts.rs            # list/read/resolve operations
  dejavu_sync/commands.rs             # conflict and maintenance commands
  dejavu_sync/status.rs               # persisted unresolved records
  remote_sync/catalog.rs              # provider-specific catalog dispatch
  remote_sync/service.rs              # WebDAV/settings-only legacy paths
  sync_config/status.rs               # public status/conflict payloads
packages/app/src/
  components/sync/
    SyncConflictIndicator.tsx
    SyncConflictIndicator.test.tsx
    SyncConflictDialog.tsx
    SyncConflictDialog.test.tsx
  hooks/useSyncConflicts.ts
  hooks/useSyncConflicts.test.tsx
  hooks/useSettingsRemoteNotebookDialog.ts
  lib/sync-config.ts
  runtime/index.ts
  App.tsx
  components/settings/SyncSettings.tsx
  components/settings/SyncSettings.test.tsx
  components/notebooks/RemoteNotebookDialog.tsx
packages/shared/src/i18n/locales/*.ts
```

### Task 1: Persist and expose unresolved conflict records

**Files:**
- Create: `apps/desktop/src-tauri/src/dejavu_sync/conflicts.rs`
- Modify: `apps/desktop/src-tauri/src/dejavu_sync.rs`
- Modify: `apps/desktop/src-tauri/src/dejavu_sync/status.rs`
- Modify: `apps/desktop/src-tauri/src/dejavu_sync/repository.rs`
- Modify: `apps/desktop/src-tauri/src/dejavu_sync/commands.rs`
- Modify: `apps/desktop/src-tauri/src/sync_config/status.rs`

**Interfaces:**
- Produces: `SyncConflictRecord`, `ConflictVersion`, `ConflictResolution`.
- Produces: commands to list, read, and resolve one conflict by opaque ID.
- Preserves: backend paths remain private; frontend receives repository-relative paths only.

- [ ] **Step 1: Write failing conflict persistence and command tests**

Use this public record:

```rust
#[derive(Clone, Debug, serde::Deserialize, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SyncConflictRecord {
    pub(crate) conflict_id: String,
    pub(crate) repository_id: String,
    pub(crate) relative_path: String,
    pub(crate) occurred_at: String,
    pub(crate) resolution: Option<ConflictResolutionKind>,
}

#[derive(Clone, Copy, Debug, serde::Deserialize, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ConflictResolutionKind {
    KeepLocal,
    UseRemote,
    KeepBoth,
}
```

Test stable opaque IDs, restart persistence, path traversal rejection, missing
history, expired history, another repository's ID, and secret/absolute-path
redaction. An unresolved conflict must be present in the status event even after
the original job has completed.

- [ ] **Step 2: Run conflict backend tests and verify RED**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib dejavu_sync::conflicts::tests
```

- [ ] **Step 3: Create records from Dejavu MergeResult**

After a successful sync, map each `MergeResult.conflicts` path to the corresponding
history file created during that run. Generate a UUID conflict ID, append a local
record to `state.json`, and retain prior unresolved records whose history still
exists. Never create a note-folder copy.

- [ ] **Step 4: Implement list and version-read commands**

`list_dejavu_conflicts(repository_id)` returns public records. `read_dejavu_conflict`
returns current local bytes plus archived remote bytes only after validating the
binding and history path. Return UTF-8 text for files at most 2 MiB; otherwise
return byte size and `text: null` so UI can offer file-level actions without
loading arbitrary binary content.

- [ ] **Step 5: Run tests and verify GREEN**

Run conflict/status tests and the Rust workspace. Expected: a conflict survives
restart, records never affect core merge input, and expired bytes produce a safe
unavailable result instead of a panic.

- [ ] **Step 6: Commit conflict records**

```bash
git add apps/desktop/src-tauri/src/dejavu_sync apps/desktop/src-tauri/src/sync_config/status.rs
git commit -m "feat(sync): persist Dejavu conflict records"
```

### Task 2: Implement the three explicit conflict resolutions

**Files:**
- Modify: `apps/desktop/src-tauri/src/dejavu_sync/conflicts.rs`
- Modify: `apps/desktop/src-tauri/src/dejavu_sync/commands.rs`
- Modify: `apps/desktop/src-tauri/src/dejavu_sync/service.rs`
- Modify: `apps/desktop/src-tauri/src/dejavu_sync/status.rs`

**Interfaces:**
- Produces: `resolve_dejavu_conflict(ResolveConflictRequest) -> AcceptedSyncJob`.
- Consumes: the Plan 3 path guard and background queue.

- [ ] **Step 1: Write failing resolution tests**

Define:

```rust
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ResolveConflictRequest {
    pub(crate) conflict_id: String,
    pub(crate) resolution: ConflictResolution,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub(crate) enum ConflictResolution {
    KeepLocal,
    UseRemote,
    KeepBoth { destination_relative_path: String },
}
```

Test keep-local performs no file write, use-remote first archives current local
bytes then atomically replaces the note, keep-both writes only the explicitly
selected safe destination, every resolution marks the record, and use-remote/
keep-both enqueue a new sync after releasing the path guard.

- [ ] **Step 2: Run resolution tests and verify RED**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib dejavu_sync::conflicts::tests::resolve_
```

- [ ] **Step 3: Implement keep-local and use-remote**

Keep-local atomically marks the record resolved. Use-remote acquires the exact
path guard, revalidates current/history files, writes the current local bytes to
a new resolution history entry, uses core `write_file_safer` for remote bytes,
releases, marks resolved, and enqueues manual sync.

- [ ] **Step 4: Implement keep-both**

Require a destination inside the same bound root, different from the conflicted
path, absent at final recheck, and not ignored/protected. Write remote bytes
safely, mark resolved, and enqueue. Do not choose or silently alter a filename in
the backend.

- [ ] **Step 5: Run tests and verify GREEN**

Run resolution, path-guard, restart, and full Rust tests. Expected: a failed
write leaves the record unresolved and preserves both original versions.

- [ ] **Step 6: Commit resolution operations**

```bash
git add apps/desktop/src-tauri/src/dejavu_sync
git commit -m "feat(sync): resolve Dejavu conflicts explicitly"
```

### Task 3: Add frontend conflict state, one-time notice, and current-file marker

**Files:**
- Create: `packages/app/src/hooks/useSyncConflicts.ts`
- Create: `packages/app/src/hooks/useSyncConflicts.test.tsx`
- Create: `packages/app/src/components/sync/SyncConflictIndicator.tsx`
- Create: `packages/app/src/components/sync/SyncConflictIndicator.test.tsx`
- Modify: `packages/app/src/lib/sync-config.ts`
- Modify: `packages/app/src/runtime/index.ts`
- Modify: `apps/desktop/src/runtime/tauri/sync-config/shared.ts`
- Modify: `packages/app/src/App.tsx`

**Interfaces:**
- Produces: `useSyncConflicts(notesRoot)` returning unresolved records and actions.
- Produces: clickable `SyncConflictIndicator` for the exact active file.
- Consumes: persisted status and `qingyu://dejavu-sync-status-changed`.

- [ ] **Step 1: Write failing hook and marker tests**

Mirror `SyncConflictRecord` in TypeScript. Test initial status load, event update,
dedupe by conflict ID, root/path normalization, one toast per newly seen ID,
restart-loaded conflicts without repeated historical toasts, and marker presence
only when the active file has an unresolved record.

- [ ] **Step 2: Run focused tests and verify RED**

```bash
pnpm --filter @markra/app exec vitest run src/hooks/useSyncConflicts.test.tsx src/components/sync/SyncConflictIndicator.test.tsx
```

- [ ] **Step 3: Implement the hook and runtime bridge**

Expose runtime methods `listConflicts`, `readConflict`, and `resolveConflict`.
The hook loads unresolved records for the active binding, listens for the exact
root, and calls `showAppToast` with a stable `sync-conflict-<id>` key only for a
conflict first observed during the current application run.

- [ ] **Step 4: Render a non-blocking active-file indicator**

Render `SyncConflictIndicator` near the editor's existing quiet status overlay,
not above the whole application. Use `AlertTriangle` from `lucide-react`, text
“存在同步冲突”, and a button that opens the conflict dialog. It must not change
editor read-only state.

- [ ] **Step 5: Run tests and verify GREEN**

Run focused tests, App tests covering a conflicted active tab and unrelated tab,
and `pnpm typecheck:test`. Expected: no modal opens automatically.

- [ ] **Step 6: Commit conflict awareness**

```bash
git add packages/app/src/hooks/useSyncConflicts.ts packages/app/src/hooks/useSyncConflicts.test.tsx packages/app/src/components/sync packages/app/src/lib/sync-config.ts packages/app/src/runtime/index.ts apps/desktop/src/runtime/tauri/sync-config/shared.ts packages/app/src/App.tsx
git commit -m "feat(sync): show active Dejavu conflicts"
```

### Task 4: Add the conflict comparison and resolution dialog

**Files:**
- Create: `packages/app/src/components/sync/SyncConflictDialog.tsx`
- Create: `packages/app/src/components/sync/SyncConflictDialog.test.tsx`
- Modify: `packages/app/src/App.tsx`
- Modify: `packages/shared/src/i18n/locales/types.ts`
- Modify: every file under `packages/shared/src/i18n/locales/`

**Interfaces:**
- Produces: user actions “保留本地”, “使用远端”, and “保留两份”.
- Consumes: `useSyncConflicts` actions and version payload.

- [ ] **Step 1: Write failing dialog behavior tests**

Cover loading, unavailable/expired remote history, local and remote UTF-8 text,
binary/oversized metadata, cancel, resolution pending, backend failure, and each
resolution request. Assert no “merge” action exists and no resolution occurs on
dialog close.

- [ ] **Step 2: Run dialog tests and verify RED**

```bash
pnpm --filter @markra/app exec vitest run src/components/sync/SyncConflictDialog.test.tsx
```

- [ ] **Step 3: Implement the comparison surface**

Use the existing dialog shell and two read-only scrollable panes labeled local
and remote. For Markdown/text, show line-preserving plain text; do not add a diff
or merge dependency. For binary/oversized data, show file sizes and retain the
same three actions.

- [ ] **Step 4: Implement resolution interactions**

Keep-local and use-remote require confirmation in the dialog. Keep-both opens
the existing save-path picker, converts the selected path to a safe repository-
relative destination, then sends it to the backend. Disable all actions while a
request is pending. Close only after backend acceptance, and leave the main app
usable throughout.

- [ ] **Step 5: Add localized strings**

Add exact typed keys for conflict title, version labels, unavailable history,
three actions, confirmations, pending, and failure. Provide native Chinese and
English text; use the English text in other locale files until translated so
every locale satisfies the typed catalog without runtime missing keys.

- [ ] **Step 6: Run tests and verify GREEN**

Run dialog, hook, App, i18n, and typecheck tests. Expected: all three actions
dispatch exact requests and an unrelated editor remains interactive behind the
user-opened dialog after cancellation.

- [ ] **Step 7: Commit conflict resolution UI**

```bash
git add packages/app/src/components/sync packages/app/src/App.tsx packages/shared/src/i18n/locales
git commit -m "feat(sync): add Dejavu conflict resolution UI"
```

### Task 5: Update Sync settings for mode, key, repository lifecycle, and conflict status

**Files:**
- Modify: `packages/app/src/components/settings/SyncSettings.tsx`
- Modify: `packages/app/src/components/settings/SyncSettings.test.tsx`
- Modify: `packages/app/src/hooks/useSyncSettingsSession.ts`
- Modify: `packages/app/src/hooks/useSyncSettingsSession.test.tsx`
- Modify: `packages/app/src/hooks/useSettingsWindowState.ts`
- Modify: `packages/app/src/lib/sync-config.ts`
- Modify: `packages/app/src/runtime/index.ts`
- Modify: `apps/desktop/src/runtime/tauri/sync-config/shared.ts`
- Modify: `packages/shared/src/i18n/locales/types.ts`
- Modify: every file under `packages/shared/src/i18n/locales/`

**Interfaces:**
- Produces: settings controls for mode, 30-43,200-second interval, key import/export/change, rebuild, stop, remote purge, and remote delete.
- Produces: conflict path list linked to the same conflict dialog.

- [ ] **Step 1: Write failing settings tests for the new contract**

Assert mode values, interval visibility only where useful, seconds bounds,
Base64/passphrase key import, copy/export feedback without rendering the key,
separate confirmations for each lifecycle operation, manual remote purge never
starting automatically, and conflict count/path rendering.

- [ ] **Step 2: Run settings tests and verify RED**

```bash
pnpm --filter @markra/app exec vitest run src/components/settings/SyncSettings.test.tsx src/hooks/useSyncSettingsSession.test.tsx
```

- [ ] **Step 3: Implement scheduling and key controls**

Replace the removed save switch/minute field with a three-option `SettingsSelect`
and bounded seconds input. Add a key section whose masked state says configured
or absent. Import accepts Base64 or passphrase; export copies Base64 only after
an explicit user click and never stores it in React logs or error text.

- [ ] **Step 4: Implement four distinct lifecycle groups**

Use separate buttons and confirmation copy for rebuild local repository, stop
sync, change global key, and delete remote repository. Put manual remote purge
beside repository maintenance, not the ordinary automatic-sync controls. Keep
controls disabled only while their own request is pending.

- [ ] **Step 5: Add status and conflict paths**

Show background phase, trigger, last attempt/success, next scheduled time,
transfer counts, safe error, and unresolved conflict paths. Clicking a path asks
the primary window to open the same conflict dialog; it does not block Settings.

- [ ] **Step 6: Run tests and verify GREEN**

Run settings/session/i18n tests and `pnpm typecheck:test`. Expected: settings
remains interactive during a background sync and never displays key bytes.

- [ ] **Step 7: Commit updated Sync settings**

```bash
git add packages/app/src/components/settings/SyncSettings.tsx packages/app/src/components/settings/SyncSettings.test.tsx packages/app/src/hooks/useSyncSettingsSession.ts packages/app/src/hooks/useSyncSettingsSession.test.tsx packages/app/src/hooks/useSettingsWindowState.ts packages/app/src/lib/sync-config.ts packages/app/src/runtime/index.ts apps/desktop/src/runtime/tauri/sync-config/shared.ts packages/shared/src/i18n/locales
git commit -m "feat(sync): expose Dejavu repository controls"
```

### Task 6: Make cloud restore an accepted background job

**Files:**
- Modify: `packages/app/src/hooks/useSettingsRemoteNotebookDialog.ts`
- Modify: `packages/app/src/hooks/useSettingsRemoteNotebookDialog.test.tsx`
- Modify: `packages/app/src/components/notebooks/RemoteNotebookDialog.tsx`
- Modify: `packages/app/src/components/notebooks/RemoteNotebookDialog.test.tsx`
- Modify: `packages/app/src/components/SettingsWindow.tsx`
- Modify: `packages/app/src/lib/notebook-switch-events.ts`
- Modify: `packages/app/src/App.tsx`
- Modify: `apps/desktop/src-tauri/src/remote_sync/catalog.rs`
- Modify: `apps/desktop/src-tauri/src/dejavu_sync/commands.rs`

**Interfaces:**
- Produces: catalog entries containing repository ID and display name for S3.
- Changes: restore completion means background-job acceptance, not full sync completion.

- [ ] **Step 1: Write failing restore ownership tests**

Assert selecting an S3 repository sends repository ID plus selected local root,
closes the restore dialog immediately after `AcceptedSyncJob`, keeps Settings
mounted on the Sync page, and shows background status. If validation/enqueue
fails, keep the dialog open with retry. Catalog load/refresh failures remain in
the dialog.

- [ ] **Step 2: Run restore tests and verify RED**

```bash
pnpm --filter @markra/app exec vitest run src/hooks/useSettingsRemoteNotebookDialog.test.tsx src/components/notebooks/RemoteNotebookDialog.test.tsx src/App.test.tsx
```

- [ ] **Step 3: Dispatch provider-specific catalog entries**

For S3, use `S3RepositoryCatalog` and return `{ repositoryId, displayName,
available, disabledReason }`. For WebDAV, preserve existing name-based entries
and old restore behavior. Keep the public union provider-tagged so a display name
is never mistaken for a repository ID.

- [ ] **Step 4: Change S3 restore response semantics**

The primary workspace owner still validates the target and calls the backend,
but replies to Settings as soon as binding/enqueue succeeds. Settings closes only
`RemoteNotebookDialog`, reloads authoritative status, and keeps its window open.
Actual job success/failure arrives through persisted status events.

- [ ] **Step 5: Run tests and verify GREEN**

Run dialog, SettingsWindow, event-contract, and App tests. Expected: a deferred
fake S3 cloud job can remain pending after the dialog and initiating hook unmount.

- [ ] **Step 6: Commit background restore UX**

```bash
git add packages/app/src/hooks/useSettingsRemoteNotebookDialog.ts packages/app/src/hooks/useSettingsRemoteNotebookDialog.test.tsx packages/app/src/components/notebooks packages/app/src/components/SettingsWindow.tsx packages/app/src/lib/notebook-switch-events.ts packages/app/src/App.tsx apps/desktop/src-tauri/src/remote_sync/catalog.rs apps/desktop/src-tauri/src/dejavu_sync/commands.rs
git commit -m "feat(sync): run S3 restore as a background job"
```

### Task 7: Cut S3 note data over while preserving WebDAV and portable settings

**Files:**
- Modify: `apps/desktop/src-tauri/src/remote_sync/service.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/catalog.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync.rs`
- Modify: `apps/desktop/src-tauri/src/dejavu_sync/commands.rs`
- Modify: `apps/desktop/src-tauri/src/sync_config.rs`
- Modify: `apps/desktop/src/runtime/tauri/sync-config/shared.ts`
- Modify: `packages/app/src/lib/sync-config.ts`
- Modify: `packages/app/src/hooks/useAppSyncCoordinator.ts`
- Modify: `packages/app/src/hooks/useAppSyncCoordinator.test.tsx`
- Modify: focused Rust tests beside the service and catalog

**Interfaces:**
- Produces: provider-tagged sync dispatch: S3 returns accepted job; WebDAV returns completed legacy result.
- Preserves: portable settings continue through `RemoteSyncScope::for_portable_settings`.

- [ ] **Step 1: Write failing provider-routing tests**

Assert S3 note runs invoke only `DejavuSyncService::enqueue`; WebDAV invokes only
the legacy notes engine; S3 portable settings still use the existing settings
scope; no S3 note run opens or writes a legacy manifest, notes prefix, or
`remote-conflict` copy.

Define the tagged response consistently in Rust and TypeScript:

```ts
export type SyncDispatchResult =
  | { status: "accepted"; job: AcceptedSyncJob }
  | { status: "completed"; result: SyncRunResult };
```

- [ ] **Step 2: Run routing tests and verify RED**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib remote_sync::service::tests::provider_
pnpm --filter @markra/app exec vitest run src/hooks/useAppSyncCoordinator.test.tsx
```

- [ ] **Step 3: Extract the protected portable-settings run**

Refactor the legacy service so its portable-settings journal and publication can
run without the legacy notes scope. Keep all existing validation, quarantine,
revision, durability, and conflict-history tests green. Do not move settings.json
into the Dejavu note repository.

- [ ] **Step 4: Route S3 notes to Dejavu**

At the Tauri command boundary, map current S3 config plus the active binding to
`SyncJobRequest`, return `accepted`, and let status events drive UI completion.
WebDAV retains `completed`. Update the frontend coordinator so an accepted S3
request clears only its submission state; it must not synthesize success before
the backend status event.

- [ ] **Step 5: Prove legacy S3 notes are not read or migrated**

Add a fixture containing only the old S3 notes layout. The new catalog must show
no repository until valid `metadata.json` exists, and binding must not import the
old manifest. This is deliberate no-migration behavior, not a recoverable error.

- [ ] **Step 6: Run tests and verify GREEN**

Run service, catalog, portable-settings, coordinator, status, and full Rust/
Vitest suites. Expected: WebDAV and portable settings retain their prior tests,
while S3 notes produce only Dejavu objects.

- [ ] **Step 7: Commit S3 cutover**

```bash
git add apps/desktop/src-tauri/src/remote_sync apps/desktop/src-tauri/src/remote_sync.rs apps/desktop/src-tauri/src/dejavu_sync apps/desktop/src-tauri/src/sync_config.rs apps/desktop/src/runtime/tauri/sync-config/shared.ts packages/app/src/lib/sync-config.ts packages/app/src/hooks/useAppSyncCoordinator.ts packages/app/src/hooks/useAppSyncCoordinator.test.tsx
git commit -m "refactor(sync): route S3 notes through Dejavu"
```

### Task 8: Remove unreachable legacy S3 note code and run final acceptance

**Files:**
- Modify/delete only legacy S3 note code proven unreachable by Task 7 coverage.
- Modify: `apps/desktop/src-tauri/src/remote_sync/live_tests.rs`
- Modify: `scripts/test-s3-sync-live.mjs`
- Modify: affected docs and static copy if they describe the legacy format.

**Interfaces:**
- Produces: one supported S3 note engine, one retained WebDAV/portable-settings legacy engine.

- [ ] **Step 1: Add a static routing-boundary test**

Assert the production S3 note route has no reference to
`RemoteSyncScope::for_notes`, legacy note manifest names, or
`remote_conflict_file_name`. Keep those symbols only where WebDAV tests or the
legacy WebDAV route still require them.

- [ ] **Step 2: Remove only code proven unreachable**

Delete S3-specific legacy note constructors, catalog branches, tests, and live
fixtures that cannot be reached after cutover. Do not delete shared WebDAV,
portable-settings journal, diagnostics, or safe filesystem helpers.

- [ ] **Step 3: Run focused acceptance suites**

```bash
DEJAVU_SOURCE_DIR=/Volumes/extendData/Data/IdeaProjects/upstream/dejavu pnpm test:dejavu-oracle
pnpm test:dejavu-interop
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib dejavu_sync
pnpm --filter @markra/app exec vitest run src/components/sync src/components/settings/SyncSettings.test.tsx src/hooks/useSettingsRemoteNotebookDialog.test.tsx src/hooks/useSyncPathGuard.test.tsx src/hooks/useSyncConflicts.test.tsx
```

- [ ] **Step 4: Run the complete repository gates**

```bash
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml --all -- --check
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
pnpm test
pnpm typecheck:test
pnpm build
git diff --check
git status --short
```

Expected: all commands exit zero and only intended changes remain.

- [ ] **Step 5: Run real MinIO acceptance when configured**

```bash
pnpm test:s3-sync:live
```

Verify in the live report: existing local plus existing remote merges, background
restore acceptance, unrelated file edits during sync, conflict notification and
history, use-remote, keep-both, lock contention, restart convergence, and cleanup
of only test prefixes. If MinIO is unavailable, record an explicit skip and do
not report this gate as passed.

- [ ] **Step 6: Perform one real desktop smoke test**

Run `pnpm tauri dev`, open Settings > Sync, select a remote repository, start
restore, confirm the restore dialog closes while Settings remains open, edit an
unrelated document, and trigger a controlled same-file conflict from the test
client. Confirm the current-file marker opens the dialog and all three actions
work without a global blocked state.

- [ ] **Step 7: Commit final cleanup or smoke-test correction**

If code or docs changed:

```bash
git add apps/desktop/src-tauri packages/app packages/shared scripts docs package.json
git commit -m "chore(sync): finish Dejavu S3 cutover"
```

Do not create an empty commit.

## Self-review

- Backend conflict records, three resolution actions, runtime types, UI state, settings controls, restore acceptance, provider routing, and legacy cleanup use matching names and explicit tests.
- Conflict UI adds awareness and user choice without changing the Dejavu merge result or blocking synchronization.
- S3 note data has one new engine after cutover; WebDAV and portable settings keep their protected existing paths.
- Final gates include shared Go tests, mixed Go/Rust local-cloud interoperability, Rust/TypeScript repository suites, real MinIO when available, and a desktop interaction smoke test.
