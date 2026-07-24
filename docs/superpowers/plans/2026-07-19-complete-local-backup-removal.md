# QingYu Complete Local Backup Removal Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Do not delegate to subagents unless the user explicitly authorizes delegation.

**Goal:** Delete QingYu's dedicated local note-folder backup feature completely while preserving settings import/export, sync-configuration recovery copies, and project-scoped WebDAV and S3-compatible cloud sync.

**Architecture:** First remove automatic backup scheduling and the application-exit callback so ordinary document lifecycle code no longer depends on backup. Then delete the backup settings and application service, followed by the unused cross-platform runtime and native Rust command. Finish by removing backup-only localization and current product claims, rewriting ambiguous sync copy, and proving the retained sync stack still passes its full verification gates.

**Tech Stack:** pnpm workspace, React 19, TypeScript 6, Tailwind CSS, Tauri v2, Rust, Vitest.

## Global Constraints

- Delete the product-level local note-folder backup vertically; do not hide it or leave compatibility shells.
- Do not add migration, fallback parsing, deprecated types, cleanup routines, or aliases for `backupSettings`.
- Preserve settings import/export itself. It must export only retained settings.
- Preserve WebDAV and S3-compatible project sync, `SyncProvider`, the shared remote-sync engine, triggers, conflict handling, provider validation, exclusions, and MinIO coverage.
- Preserve invalid/unsupported sync-configuration safety copies in `apps/desktop/src-tauri/src/project_config/storage.rs` and their reset/recovery copy.
- Preserve the Windows API constant `FILE_FLAG_BACKUP_SEMANTICS`; it is a filesystem-open flag used by remote sync, not the deleted product feature.
- Preserve third-party backup-tool references where they explain that QingYu does not control external software.
- Preserve `CHANGELOG.md`, Git history, existing `docs/superpowers` records, and the untracked `bg.png` file.
- Use `pnpm` for JavaScript dependency and verification commands.
- Do not use the TypeScript `void` keyword or operator.
- Do not push any commit to a remote.
- Keep each commit limited to the task being completed.

---

## File and Interface Map

- `packages/app/src/App.tsx` currently composes `useBackupSettings` and `useWorkspaceBackupSync`, passes the exit callback to `useMarkdownDocument`, assigns the current workspace source path, and renders the last-backup status.
- `packages/app/src/hooks/useMarkdownDocument.ts` currently has a generic `beforeNativeAppExit` option whose only consumer is local backup; remove the option and its wait-before-exit test.
- `packages/app/src/lib/settings/app-settings.ts` is the persisted and portable settings source of truth; remove `backupSettings` without changing settings import/export as a capability.
- `packages/app/src/hooks/useSettingsWindowState.ts`, `packages/app/src/components/SettingsWindow.tsx`, and `packages/app/src/components/SettingsShell.tsx` own the Backups settings category and manual-run flow.
- `packages/app/src/runtime/index.ts` defines the cross-platform file runtime; remove the backup contract before deleting the platform implementations.
- `apps/desktop/src-tauri/src/backup.rs` is the dedicated one-way local-copy implementation and can be deleted in full.
- `apps/desktop/src-tauri/src/project_config/storage.rs` owns retained sync-configuration recovery copies and must not be changed as part of feature removal.
- `packages/shared/src/i18n/locales/*.ts` contain both removable `settings.backup.*` keys and retained sync-reset copy. Only the plaintext sync description should stop mentioning the removed local backup feature.

---

### Task 1: Remove Backup Scheduling and Application-Exit Coupling

**Files:**
- Delete: `packages/app/src/hooks/useWorkspaceBackupSync.ts`
- Delete: `packages/app/src/hooks/useWorkspaceBackupSync.test.tsx`
- Modify: `packages/app/src/App.tsx`
- Modify: `packages/app/src/App.test.tsx`
- Modify: `packages/app/src/components/QuietStatus.tsx`
- Modify: `packages/app/src/hooks/useMarkdownDocument.ts`
- Modify: `packages/app/src/hooks/useMarkdownDocument.test.tsx`
- Modify: `packages/app/src/test/app-harness.tsx`

**Interfaces:**
- Removes: `backupStatusLabel`, `beforeNativeAppExitBackup`, `runWorkspaceBackup`, and workspace backup source assignment.
- Removes: `beforeNativeAppExit?: () => unknown | Promise<unknown>` from `UseMarkdownDocumentOptions`.
- Removes: `backupLabel` from `QuietStatusProps`.
- Preserves: unsaved-change confirmation, draft persistence, editor-window restore snapshots, `exitNativeApp`, and the separately composed `useProjectSyncCoordinator`.

- [ ] **Step 1: Delete the backup coordinator and its tests**

Delete both `useWorkspaceBackupSync` files. Do not move the interval or exit scheduling into another hook.

- [ ] **Step 2: Strip backup composition from `App.tsx`**

Remove the `useBackupSettings` and `useWorkspaceBackupSync` imports and state. Remove:

```text
backupSettings
backupStatusLabel
beforeNativeAppExitBackup
setWorkspaceBackupSyncSourcePath
```

Delete the call that assigns `fileTreeSourcePath ?? document.path` as the backup source. Stop passing `beforeNativeAppExit` to `useMarkdownDocument` and `backupLabel` to `QuietStatus`. Leave `projectSync`, `runProjectSyncNow`, `syncStatusLabel`, and every cloud-sync trigger unchanged.

- [ ] **Step 3: Remove the now-unused exit callback extension point**

In `useMarkdownDocument.ts`, delete the `beforeNativeAppExit` option, destructuring, invocation, and effect dependency. The retained exit sequence must remain:

```ts
await draftPersistence;
await persistNativeEditorWindowRestoreSnapshot();
await exitNativeApp();
```

Delete the test named `waits for native app exit work before exiting the app`. Keep tests for unsaved-document decisions, draft persistence, restore snapshots, and the native exit request listener.

- [ ] **Step 4: Remove status-bar backup plumbing and update fixtures**

Delete `backupLabel` from `QuietStatus`. Update `App.test.tsx` and `app-harness.tsx` to remove backup-hook mocks, backup settings fixtures, and status assertions while retaining sync status coverage.

- [ ] **Step 5: Run focused lifecycle and application tests**

Run:

```bash
pnpm --filter @markra/app test -- App.test.tsx useMarkdownDocument.test.tsx
```

Expected: selected tests PASS; app exit no longer awaits local backup, while normal save/exit and cloud-sync composition still compile.

- [ ] **Step 6: Commit application detachment**

```bash
git add packages/app/src/App.tsx packages/app/src/App.test.tsx packages/app/src/components/QuietStatus.tsx packages/app/src/hooks/useMarkdownDocument.ts packages/app/src/hooks/useMarkdownDocument.test.tsx packages/app/src/hooks/useWorkspaceBackupSync.ts packages/app/src/hooks/useWorkspaceBackupSync.test.tsx packages/app/src/test/app-harness.tsx
git commit -m "refactor: detach local backup from app lifecycle"
```

---

### Task 2: Remove Backup Settings, Manual Execution, and Diagnostics

**Files:**
- Delete: `packages/app/src/components/settings/BackupSettings.tsx`
- Delete: `packages/app/src/components/settings/BackupSettings.test.tsx`
- Delete: `packages/app/src/hooks/useBackupSettings.ts`
- Delete: `packages/app/src/lib/backup.ts`
- Delete: `packages/app/src/lib/backup.test.ts`
- Delete: `packages/app/src/lib/settings/backup-settings.ts`
- Delete: `packages/app/src/lib/settings/backup-settings.test.ts`
- Modify: `packages/app/src/components/SettingsSections.tsx`
- Modify: `packages/app/src/components/SettingsShell.tsx`
- Modify: `packages/app/src/components/SettingsShell.test.tsx`
- Modify: `packages/app/src/components/SettingsWindow.tsx`
- Modify: `packages/app/src/components/compact/CompactSettingsHome.test.tsx`
- Modify: `packages/app/src/lib/compact-settings.test.ts`
- Modify: `packages/app/src/hooks/useSettingsWindowState.ts`
- Modify: `packages/app/src/lib/diagnostics/diagnostics-report.ts`
- Modify: `packages/app/src/lib/diagnostics/diagnostics-report.test.ts`
- Modify: `packages/app/src/lib/settings/app-settings.ts`
- Modify: `packages/app/src/lib/settings/app-settings.test.ts`
- Modify: `packages/app/src/lib/settings/settings-events.ts`
- Modify: `packages/app/src/lib/settings/settings-events.test.ts`

**Interfaces:**
- Removes: `BackupSettings`, defaults, normalizer, storage key, store accessors, change event, manual run handler, target picker, backup toast state, and diagnostics input/section.
- Produces: a `PortableStoredAppSettings` shape with no `backupSettings` field.
- Preserves: `exportStoredAppSettings`, `importStoredAppSettings`, Storage settings controls, and `settings.storage.settingsBackup*` copy.

- [ ] **Step 1: Delete backup-only modules**

Delete the component, hooks, application service, settings model, and their dedicated tests listed above.

- [ ] **Step 2: Remove backup from the settings schema and portable format**

In `app-settings.ts`, remove:

```text
BackupSettings imports and exports
backupSettingsKey
PortableStoredAppSettings.backupSettings
normalization of value.backupSettings
store.get("backupSettings")
store.set("backupSettings", ...)
getStoredBackupSettings
saveStoredBackupSettings
```

Do not add a compatibility branch for unknown `backupSettings` input. In `app-settings.test.ts`, add explicit assertions that exports and imported results do not contain `backupSettings`, and that imports never write the `backupSettings` store key. Keep the settings format version unchanged unless the implementation already requires a version change for another reason.

- [ ] **Step 3: Remove cross-window backup events**

Delete the backup event constant, payload, notifier, listener, imports, and test blocks from `settings-events.ts` and `settings-events.test.ts`. Preserve all other settings events.

- [ ] **Step 4: Remove Backups settings navigation and state**

Delete `"backup"` from `SettingsCategory`, remove the `Archive` category definition, and delete backup-specific navigation/content tests. Remove the `BackupSettings` export and render branch.

In `useSettingsWindowState.ts`, delete backup state, loading, source-path discovery, target picker, update handler, manual-run handler, toasts, import-apply broadcasting, and returned properties. In `SettingsWindow.tsx`, delete all matching destructuring and props.

Remove obsolete Compact tests that list Backup as a hidden unsupported category; do not replace them with absence-only assertions.

- [ ] **Step 5: Remove backup diagnostics**

Delete `backupSettings` from `DiagnosticsReportInput` and remove the `### Backup` section. Update its test fixture and expected report while preserving diagnostics for retained features.

- [ ] **Step 6: Run focused settings and diagnostics tests**

Run:

```bash
pnpm --filter @markra/app test -- app-settings.test.ts settings-events.test.ts SettingsShell.test.tsx diagnostics-report.test.ts CompactSettingsHome.test.tsx compact-settings.test.ts
```

Expected: selected tests PASS; settings import/export remains functional and never emits or writes `backupSettings`.

- [ ] **Step 7: Commit settings and application-service removal**

```bash
git add packages/app/src
git commit -m "refactor: remove local backup settings and service"
```

---

### Task 3: Remove Cross-Platform Backup Runtime and Native Copy Command

**Files:**
- Delete: `apps/desktop/src-tauri/src/backup.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `packages/app/src/runtime/index.ts`
- Modify: `packages/app/src/lib/tauri/file.ts`
- Modify: `packages/app/src/lib/app-logger.ts`
- Modify: `packages/app/src/lib/runtime-log.ts`
- Modify: `apps/desktop/src/runtime/index.ts`
- Modify: `apps/desktop/src/runtime/tauri/file.ts`
- Modify: `apps/desktop/src/runtime/tauri/file.test.ts`
- Modify: `apps/desktop/src/runtime/tauri/invoke.ts`
- Modify: `apps/web/src/runtime/web/file.ts`
- Modify: `apps/web/src/runtime/web/file.test.ts`

**Interfaces:**
- Removes: `BackupNativeMarkdownFolderInput`, `NativeMarkdownBackupSummary`, `backupMarkdownFolder`, `backupNativeMarkdownFolder`, `backup_markdown_folder`, and the `backup` application log area.
- Preserves: all project config, WebDAV, S3, `syncProjectFolder`, file-save, web-image, and ordinary file runtime interfaces.

- [ ] **Step 1: Remove the shared file-runtime contract**

In `packages/app/src/runtime/index.ts`, delete the backup input/summary types, `AppFileRuntime.backupMarkdownFolder`, and unsupported default. Delete the wrapper from `packages/app/src/lib/tauri/file.ts`.

- [ ] **Step 2: Remove desktop and web implementations**

Delete the desktop runtime mapping and Tauri invoke wrapper, plus the corresponding test block. Delete the web runtime unsupported placeholder and its test that expects `Local folder backups require the desktop runtime.`

- [ ] **Step 3: Remove native Rust implementation**

Delete `apps/desktop/src-tauri/src/backup.rs`. In `lib.rs`, remove:

```rust
mod backup;
use backup::backup_markdown_folder;
```

and remove `backup_markdown_folder` from `tauri::generate_handler!`. Do not change `remote_sync` or `project_config` modules.

- [ ] **Step 4: Remove backup-only logging classification**

Delete `"backup"` from `AppLogArea` and `runtimeLogAreas`. Remove the `command.includes("backup")` branch from the Tauri invoke classifier. Preserve sync, storage, file, update, and other log areas.

- [ ] **Step 5: Run platform runtime and Rust tests**

Run:

```bash
pnpm --filter @markra/desktop test -- file.test.ts
pnpm --filter @markra/web test -- file.test.ts
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
```

Expected: all selected suites PASS; no backup command is registered, and all remote-sync/project-config Rust tests still pass, including configuration recovery-copy tests.

- [ ] **Step 6: Commit runtime removal**

```bash
git add packages/app/src/runtime/index.ts packages/app/src/lib/tauri/file.ts packages/app/src/lib/app-logger.ts packages/app/src/lib/runtime-log.ts apps/desktop/src/runtime apps/desktop/src-tauri/src/backup.rs apps/desktop/src-tauri/src/lib.rs apps/web/src/runtime
git commit -m "refactor: remove local backup runtime"
```

---

### Task 4: Remove Backup Localization and Rewrite Current Product Copy

**Files:**
- Modify: `packages/shared/src/i18n/locales/types.ts`
- Modify: `packages/shared/src/i18n/locales/de.ts`
- Modify: `packages/shared/src/i18n/locales/en.ts`
- Modify: `packages/shared/src/i18n/locales/es.ts`
- Modify: `packages/shared/src/i18n/locales/fr.ts`
- Modify: `packages/shared/src/i18n/locales/it.ts`
- Modify: `packages/shared/src/i18n/locales/ja.ts`
- Modify: `packages/shared/src/i18n/locales/ko.ts`
- Modify: `packages/shared/src/i18n/locales/pt-BR.ts`
- Modify: `packages/shared/src/i18n/locales/ru.ts`
- Modify: `packages/shared/src/i18n/locales/zh-CN.ts`
- Modify: `packages/shared/src/i18n/locales/zh-TW.ts`
- Modify: `packages/app/src/components/settings/SyncSettings.test.tsx`
- Modify: `README.md`
- Modify: `README.zh-CN.md`
- Modify: `PRODUCT.md`
- Modify: `docs/privacy.md`
- Modify: `docs/testing/qingyu-project-sync-desktop-acceptance.md`

**Interfaces:**
- Removes: `settings.categories.backup`, `settings.sections.backup`, and every `settings.backup.*` key from `I18nKey` and all locale maps.
- Preserves: `settings.storage.settingsBackup*`, sync reset confirmations, and their explicit promise to retain damaged/unsupported config as a local safety copy.
- Rewrites: `settings.sync.plaintextDescription` in every locale to say QingYu sync excludes `.qingyu/config.json`, without claiming the deleted local backup feature also excludes it.

- [ ] **Step 1: Delete backup-only localization keys**

Remove the Backups category/section keys and all `settings.backup.*` keys from `types.ts` and every locale file. Do not remove Storage settings import/export keys.

- [ ] **Step 2: Rewrite only the ambiguous sync description**

In every locale, update `settings.sync.plaintextDescription` so it attributes the exclusion solely to QingYu sync. Keep `settings.sync.resetConfirm` and `settings.sync.unsupportedResetConfirm` unchanged because they describe the retained recovery-copy mechanism.

Update `SyncSettings.test.tsx` to expect the new sync-only sentence while retaining its local recovery-backup assertion.

- [ ] **Step 3: Rewrite current documentation**

Update current docs as follows:

- `README.md` and `README.zh-CN.md`: remove local backup from the feature table, workflow, practical-value copy, and the Backup/Sync/Export section; keep cloud sync and export.
- `PRODUCT.md`: remove backup from the core experience, audience, advanced-feature, and reliability lists; retain sync and export.
- `docs/privacy.md`: remove local backup settings and the dedicated Backup section; describe project sync triggers and exclusions accurately; retain the statement that third-party backup tools are outside QingYu's control.
- `docs/testing/qingyu-project-sync-desktop-acceptance.md`: remove local-backup setup and assertions while preserving `.qingyu/` and `.markra-sync/` remote-sync exclusion coverage.

Do not edit `CHANGELOG.md` or prior `docs/superpowers` records.

- [ ] **Step 4: Run localization and focused sync UI tests**

Run:

```bash
pnpm --filter @markra/shared test
pnpm --filter @markra/app test -- SyncSettings.test.tsx
```

Expected: locale parity tests PASS; sync reset copy still describes the retained safety copy, and plaintext exclusion copy describes only cloud sync.

- [ ] **Step 5: Commit localization and documentation cleanup**

```bash
git add packages/shared/src/i18n/locales packages/app/src/components/settings/SyncSettings.test.tsx README.md README.zh-CN.md PRODUCT.md docs/privacy.md docs/testing/qingyu-project-sync-desktop-acceptance.md
git commit -m "docs: remove local backup product surface"
```

---

### Task 5: Prove Complete Removal and Retained Cloud Sync

**Files:**
- Verify only; modify files solely to fix failures caused by Tasks 1–4.

- [ ] **Step 1: Scan for removed implementation symbols**

Run:

```bash
rg -n 'BackupSettings|useBackupSettings|useWorkspaceBackupSync|runMarkdownBackup|backupStatusLabel|beforeNativeAppExitBackup|backupMarkdownFolder|backupNativeMarkdownFolder|backup_markdown_folder' packages apps
rg -n 'settings\.backup\.|settings\.categories\.backup|settings\.sections\.backup|backupSettings' packages apps README.md README.zh-CN.md PRODUCT.md docs/privacy.md docs/testing
```

Expected: no matches.

- [ ] **Step 2: Classify every remaining active use of “backup”**

Run:

```bash
rg -n -i '\bbackup\b|backups|备份|備份' packages apps README.md README.zh-CN.md PRODUCT.md docs/privacy.md docs/testing --glob '!docs/superpowers/**'
```

Expected remaining categories only:

- settings import/export labels such as `settings.storage.settingsBackup*`;
- sync reset/recovery copy and `project_config/storage.rs` safety-copy implementation/tests;
- Windows `FILE_FLAG_BACKUP_SEMANTICS` used by remote sync;
- third-party backup-tool disclaimers.

Review every hit; remove any product-level local note-folder backup residue.

- [ ] **Step 3: Confirm the retained sync stack is still present**

Run:

```bash
rg -n 'useProjectSyncCoordinator|runProjectFolderSync|syncProjectFolder|SyncProvider|WebDAV|S3' packages/app/src apps/desktop/src apps/desktop/src-tauri/src
```

Expected: the project sync coordinator, frontend runtime, Tauri command, provider configuration, and remote-sync engine remain present.

- [ ] **Step 4: Run repository verification gates**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
pnpm test
pnpm typecheck:test
pnpm build
```

Expected: all commands exit 0.

- [ ] **Step 5: Run live S3 coverage when configured**

If the real MinIO test environment is already configured, run:

```bash
pnpm test:s3-sync:live
```

Expected: PASS. If it is not configured, report that fact without inventing credentials or changing repository configuration.

- [ ] **Step 6: Verify the desktop application live**

Start:

```bash
pnpm tauri dev
```

Confirm:

- the desktop application launches;
- a Markdown note opens, edits, saves, and closes normally;
- settings have no Backups category or local-copy controls;
- settings import/export remains available under Storage;
- WebDAV and S3-compatible sync settings remain available;
- manual cloud sync still reaches the retained sync path;
- closing the application does not start or await a local backup.

- [ ] **Step 7: Review final diff and repository state**

Run:

```bash
git diff --check
git status --short --branch
git log --oneline -6
```

Expected: no uncommitted task changes, `bg.png` remains untracked and untouched, local commits are not pushed, and the four implementation commits are visible after the design/plan commits.

## Completion Criteria

- The dedicated local note-folder backup feature has no active UI, settings, scheduling, runtime, native implementation, diagnostics, logging area, localization, tests, or current product claims.
- Settings import/export still works and contains no `backupSettings` field.
- Sync-configuration recovery copies remain implemented, tested, and accurately described.
- WebDAV and S3-compatible cloud sync remains fully wired and passes automated verification.
- All required verification commands pass, or any environment-only live MinIO limitation is reported precisely.
- No remote push occurs.
