# QingYu Complete Local Backup Removal Design

**Date:** 2026-07-19

## Goal

Remove QingYu's dedicated local note-folder backup feature completely while preserving project-scoped WebDAV and S3-compatible cloud sync. The product should retain simple note recording and optional cloud synchronization without carrying a second, application-specific local-copy system.

The removal is vertical: user interface, settings, scheduling, runtime interfaces, native implementation, tests, localization, diagnostics, and current documentation are deleted together. The result must not hide local backup behind a flag or leave inactive compatibility shells.

## Product Boundary

### Remove completely

- The Backups settings category and its manual-run, target-folder, exit, interval, status, and last-run controls.
- The `BackupSettings` type, defaults, normalization, persisted `backupSettings` field, change event, and settings hook.
- The local backup coordinator and its manual, interval, and application-exit triggers.
- The shared application runtime contract for backing up a Markdown folder.
- The desktop TypeScript bridge and Tauri command for local folder backup.
- The native Rust local-copy module and its command registration.
- The web runtime's unsupported local-backup placeholder.
- Backup-only diagnostics fields, status labels, toasts, localization keys, exports, fixtures, and tests.
- Current product, privacy, and acceptance documentation that presents local backup as a supported QingYu capability.

### Preserve

- Project-scoped WebDAV and S3-compatible bidirectional cloud sync.
- The shared remote-sync engine, provider configuration, triggers, conflict handling, exclusion rules, and live MinIO coverage.
- Settings import and export as a user-facing capability.
- Internal safety copies created when invalid or unsupported sync configuration must be reset or recovered.
- Ordinary file saving, autosave, history, export, search, and Markdown editing.
- Historical references in `CHANGELOG.md`, Git history, and existing `docs/superpowers` plans and specifications.
- The untracked `bg.png` file.

## Compatibility Policy

The product has no users, and compatibility was explicitly excluded. The implementation therefore removes the `backupSettings` field from the active settings model and portable settings format without adding migration aliases, deprecated types, fallback parsing, or cleanup routines.

Unknown fields in manually supplied settings JSON do not become supported settings and are not emitted by a later export. Existing prerelease development data is not migrated or deleted from local application stores.

## Architecture

### Application composition and exit flow

`App.tsx` no longer loads backup settings or constructs `useWorkspaceBackupSync`. Opening a workspace does not assign a backup source path, schedule a copy, or derive a backup status label.

The document close and native application-exit path stops invoking `beforeNativeAppExitBackup`. Removing that callback must not alter unsaved-document checks, normal file saving, or the separately wired project-sync coordinator.

Cloud sync remains independent. `useProjectSyncCoordinator` continues to own project-open, interval, save, manual, and settings-exit synchronization according to the current project configuration and editing barriers.

### Settings and portable settings

Delete the Backups category from settings navigation and category types. Delete `BackupSettings`, its component, its state and handlers in the settings window, and the cross-window backup settings event.

Settings import and export remain available. Their portable data structure contains only retained settings and no longer reads, normalizes, persists, broadcasts, or exports `backupSettings`.

Any copy that currently groups cloud sync and local backup together must be rewritten to describe only the retained behavior. In particular, descriptions of `.qingyu/` and `.markra-sync/` exclusions must not imply that QingYu still provides a local backup feature.

### Runtime boundaries

Remove `backupMarkdownFolder` from the shared file-runtime contract and unsupported runtime defaults. Delete the corresponding desktop and web implementations, frontend bridge exports, response/input types, and tests.

Remove the `backup_markdown_folder` Tauri command, command registration, Rust module, local-copy traversal, exclusion logic, and Rust tests. Do not reuse or move this implementation into the sync engine; remote sync already has its own semantics and ownership.

The remote-sync modules and their runtime interfaces remain intact. Mechanical edits are allowed only where removal of a shared backup type or copy requires compilation fixes.

### Diagnostics, localization, and documentation

Diagnostics no longer accept backup settings or emit a Backup section. Cloud-sync diagnostics, if any, remain unchanged.

Delete backup-only keys from every locale and from the localization key union. Preserve uses of words such as "backup" when they refer specifically to settings import/export, third-party backup tools, or internal sync-configuration recovery copies. Rewrite ambiguous retained text so its meaning is explicit.

Update the current README files, product description, privacy documentation, and sync acceptance checklist. Historical engineering records remain unchanged.

## Data Flow After Removal

1. QingYu starts and loads only retained appearance, editor, storage, export, network, workspace, and other supported settings.
2. Opening a note or project initializes normal file handling and, when configured, the project cloud-sync coordinator. No local backup settings or scheduler are initialized.
3. Saving writes the Markdown document through the existing file runtime. Configured cloud sync may run through its existing save trigger.
4. Closing the document or application follows existing unsaved-change and save handling without starting or awaiting a local folder copy.
5. Manual and scheduled cloud sync continue through the configured WebDAV or S3-compatible provider.
6. Settings export writes only retained preferences; settings import applies only retained preferences.
7. If sync configuration is damaged or unsupported, the existing recovery flow may retain an internal safety copy before resetting it.

## Error Handling

Delete local-backup success, failure, running, missing-source, and missing-target notifications together with the feature. No replacement error or unsupported-feature message is added.

Cloud-sync errors, conflict copies, provider validation, and sync-configuration recovery retain their existing behavior. File-saving and application-exit error handling must not be broadened or refactored beyond the changes required to remove the backup callback.

## Testing and Acceptance

### Test maintenance

- Delete tests that only cover the removed local backup settings, scheduling, runtime bridge, native copy routine, or unsupported web placeholder.
- Update settings import/export, settings-window, application, diagnostics, runtime, and test-harness fixtures whose interfaces lose backup fields.
- Preserve all WebDAV, S3, project-sync coordinator, remote-sync engine, conflict, and configuration-recovery tests.
- Do not add absence-only unit tests for deleted functionality; use repository scans to prove vertical removal.

### Static acceptance

- Active source contains no `BackupSettings`, `useBackupSettings`, `useWorkspaceBackupSync`, `runMarkdownBackup`, `backupMarkdownFolder`, `backup_markdown_folder`, or Backups settings category.
- The active settings schema, portable settings format, diagnostics, runtime contracts, and localization key union contain no dedicated local-backup fields.
- The Rust local-backup module and command registration do not exist.
- Current product documentation does not advertise local backup as a QingYu capability.
- Every remaining active-code use of "backup" refers to settings import/export, an internal recovery safety copy, a third-party backup tool, or an approved historical record.
- WebDAV and S3-compatible sync settings and implementation remain present.

### Automated verification

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
pnpm test
pnpm typecheck:test
pnpm build
```

If the configured real MinIO test server is available, also run:

```bash
pnpm test:s3-sync:live
```

Run focused repository scans for removed backup symbols, settings keys, localization keys, runtime commands, and current product claims. Review every remaining hit and classify it against the preserved meanings above.

### Live verification

Start the desktop application with `pnpm tauri dev` and confirm:

- the application launches and Markdown notes can be opened, edited, saved, and closed;
- settings contain no Backups category or local-copy controls;
- settings import and export remain available;
- WebDAV and S3-compatible cloud-sync settings remain available;
- manual cloud sync still reaches the existing sync path;
- closing the application no longer starts or awaits a local backup.

## Out of Scope

- Removing or weakening WebDAV or S3-compatible cloud sync.
- Removing internal sync-configuration safety copies or recovery handling.
- Removing settings import/export because its user-facing label may contain the word "backup".
- Migrating or deleting prerelease backup settings from local development installations.
- Rewriting `CHANGELOG.md`, Git history, or prior Superpowers plans and specifications.
- Introducing a replacement backup system, versioned snapshot system, or cloud provider.
- Unrelated refactors.
- Pushing commits to any remote.
