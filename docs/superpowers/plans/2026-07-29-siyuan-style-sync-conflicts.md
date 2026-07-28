# SiYuan-Style Sync Conflicts Implementation Plan

> **Execution:** Use `superpowers:executing-plans` for inline execution. Use
> `superpowers:subagent-driven-development` only for L3 work or when the user
> explicitly requests subagent execution.

**Risk level:** L2 — the Dejavu merge algorithm already implements local-wins and remote-history retention; this change removes an overbuilt manual-resolution product layer and adds the SiYuan conflict-document option.

**Goal:** Make QingYu synchronization resolve same-path conflicts automatically like SiYuan: keep the local working copy, publish it normally, preserve the remote version in history, notify without blocking, and optionally create a timestamped conflict document.

**Architecture:** Leave `qingyu-dejavu` repository format and merge planning unchanged. Convert application conflict records to completed `keep-local` history records at the repository-runner boundary, retain their remote history for the existing retention period, and expose that history as a read-only advanced surface. Remove the active-document three-way resolution workflow. Store the optional conflict-document preference in the local unsynchronized sync configuration; when enabled, create a safe sibling Markdown copy from the already-preserved remote history without failing the synchronization if copy generation cannot complete.

**Tech stack:** Rust, Tauri v2, React, TypeScript, CodeMirror 6, Vitest, pnpm, qingyu-dejavu.

**Global constraints:**
- Dejavu remains authoritative for merge planning, local-wins publication, and remote history bytes.
- SiYuan remains authoritative for product behavior: automatic local-wins, data-history preservation, non-blocking notice, optional conflict document disabled by default.
- No mandatory modal, unresolved-conflict queue, or per-file choice in the sync path.
- Existing repository history safety, retention cleanup, path validation, atomic writes, working-tree coordination, and background execution must remain intact.
- Do not add compatibility work for existing QingYu users; the product is still pre-release.
- Do not commit credentials, endpoint configuration, `.serena/`, generated bundles, or temporary QA data.

### Task 1: Make conflict completion automatic at the repository boundary

**Files:**
- Modify: `apps/desktop/src-tauri/src/dejavu_sync/repository.rs`
- Modify: `apps/desktop/src-tauri/src/dejavu_sync/status.rs`
- Modify: `apps/desktop/src-tauri/src/dejavu_sync/conflicts.rs`
- Test: adjacent Rust test modules

**Behavior and boundary:** A bidirectional same-path conflict completes successfully with the local working file and published local version unchanged. The remote version remains under repository `history`. Persisted records are immediately marked `keep-local`, are never counted as unresolved, and remain readable until normal history retention removes them. Do not change `qingyu-dejavu/src/sync.rs` merge semantics.

**Interfaces:** Retain `SyncConflictRecord` as history metadata. Replace unresolved-only listing/reading with read-only history listing/reading; remove manual mutation from the product command graph after all callers are migrated.

- [x] Add focused failing tests proving a fresh conflict result is completed `keep-local`, remains readable from history, and produces zero unresolved records; run `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml dejavu_sync::repository -- --nocapture` and confirm the old `resolution: None` behavior fails.
- [x] Implement automatic completion without altering Dejavu merge planning; run repository, status, and conflict module tests.
- [x] Add retention and missing-history tests proving stale history disappears safely without reviving a manual-resolution state.
- [x] Self-review that local/remote bytes and published refs match SiYuan behavior.

### Task 2: Add the optional SiYuan conflict-document preference

**Files:**
- Modify: `apps/desktop/src-tauri/src/sync_config/model.rs`
- Modify: `apps/desktop/src-tauri/src/sync_config.rs`
- Modify: `apps/desktop/src-tauri/src/dejavu_sync/repository.rs`
- Modify: `packages/app/src/lib/sync-config.ts`
- Modify: `packages/app/src/components/settings/SyncSettings.tsx`
- Modify: locale resources containing `settings.sync.*`
- Test: focused Rust sync-config/repository tests and `SyncSettings.test.tsx`

**Behavior and boundary:** Add local unsynchronized `generateConflictDocument`, default `false`. When enabled, each Markdown conflict creates a safe sibling named `<stem>-Conflicted-YYYYMMDD-HHmmss.md` from the preserved remote bytes. Non-Markdown conflicts remain history-only. Copy collisions use a deterministic numeric suffix. Copy failure does not fail or roll back a completed sync; history remains the source of recovery.

**Interfaces:** `SyncConfig.generate_conflict_document: bool`, serialized as `generateConflictDocument`; `SyncConfigPatch::GenerateConflictDocument(bool)`; matching TypeScript field and patch.

- [x] Add config RED tests for absent/default/patch/round-trip behavior and a settings RED test for an off-by-default switch.
- [x] Add repository RED tests for disabled, enabled Markdown, non-Markdown, collision, coordinator rejection, and copy-write failure behavior.
- [x] Implement the smallest config and post-merge copy path using existing safe relative-path and working-tree coordination helpers.
- [x] Run `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml sync_config` plus focused repository tests and `pnpm --filter @markra/app test -- SyncSettings.test.tsx`.

### Task 3: Replace the manual conflict workflow with notice and read-only history

**Files:**
- Modify: `packages/app/src/hooks/useSyncConflicts.ts`
- Modify: `packages/app/src/hooks/useSyncConflicts.test.tsx`
- Modify: `packages/app/src/components/settings/SyncSettings.tsx`
- Delete: `packages/app/src/components/sync/SyncConflictDialog.tsx`
- Create: `packages/app/src/components/sync/SyncConflictHistoryDialog.tsx`
- Modify: `packages/app/src/App.tsx`
- Modify: `packages/app/src/lib/sync-config.ts`
- Modify: `apps/desktop/src/runtime/tauri/sync-config/shared.ts`
- Modify: Tauri command registration and boundary tests under `apps/desktop/src-tauri/src`
- Test: adjacent hook/component/App/runtime tests

**Behavior and boundary:** A new conflict emits one non-blocking notice that says the local version was retained and the remote version was saved in sync history. The settings status shows the conflict count and paths as history, not unresolved work. Users may explicitly open a read-only local/remote preview from history. Remove the titlebar conflict indicator, three resolution buttons, destination-path input, and `resolve_dejavu_conflict` command from the product graph.

**Interfaces:** Replace mutation-oriented `list/read/resolve` runtime calls with read-only `listDejavuConflictHistory` and `readDejavuConflictHistory`. Remove `resolveDejavuConflict` and its native command.

- [x] Add UI RED tests proving no three-way actions or active-document conflict button exists, while a new event produces one notice and a read-only history preview contains both actual versions.
- [x] Implement notice/history state with root-generation protection; do not replay notices for persisted old history after relaunch.
- [x] Remove obsolete resolution components, runtime types, commands, locale keys, and tests; retain only history/notice copy.
- [x] Run focused hook/component/App/runtime tests, then `pnpm --filter @markra/app test` and `pnpm --filter @markra/desktop test`.

### Task 4: Verify production behavior and complete the interrupted integration gate

**Files:**
- Update: `.superpowers/sdd/task-8-report.md`
- Update: `docs/superpowers/audits/2026-07-28-dejavu-main-integration-gap-audit.md` if conflict behavior evidence changes

**Behavior and boundary:** Real S3 conflict synchronization remains non-blocking, automatically retains the local file, publishes local bytes, preserves exact remote bytes in history, shows only a notice, and optionally creates a conflict document. Previously completed bootstrap, relocation, restore, delete propagation, retry, and relaunch scenarios remain green.

**Interfaces:** None beyond Tasks 1–3.

- [x] Run a real isolated conflict with the option off; verify local/cloud convergence, remote history bytes, zero unresolved work, notice text, and no modal.
- [x] Enable the option and repeat with a new conflict; verify the timestamped remote copy and history retention.
- [x] Run `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml`, `pnpm test`, `pnpm typecheck:test`, `pnpm build`, `pnpm brand:verify`, and `pnpm test:dejavu-oracle:unit` on Node 24.
- [x] Rerun the pinned Go oracle/interop and local/public live S3 gates because Rust sync integration changed; prove exact-prefix cleanup.
- [ ] Perform one combined final diff review covering automatic semantics, privacy, path safety, failure recovery, removed manual surfaces, and repository cleanliness.
