# QingYu Application Sync Desktop Acceptance

Use this checklist to verify the application-level WebDAV and S3-compatible synchronization model in the desktop app. Never put a real endpoint, account, token, access key, or secret in this document, screenshots, issue text, or committed test fixtures.

Automated Rust and TypeScript suites are prerequisite evidence, not desktop-GUI evidence. Mark a manual row complete only when it is exercised on the stated desktop build and frozen commit; record unavailable tooling or environment as pending rather than inferring a pass from tests.

## Contract Under Test

- One device has exactly one current notebook directory after onboarding and one application sync configuration.
- Synchronization is disabled by default and cannot run without a valid current notebook and complete active-provider settings.
- Choosing another directory switches the current notebook; temporary external-folder editing is not supported. Opening or focusing a standalone file never retargets synchronization.
- The current notebook synchronizes ordinary files below remote `notes/<directory-name>/`; portable `settings.json` synchronizes separately below remote `app/`.
- Local AppConfig fields in `settings.json`, `sync-config.json`, `primary-workspace.json`, `mcp.json`, sync operational state, `mcp-runtime/`, themes, extensions, credentials, and device paths never synchronize.
- The child Kernel is the only normal `settings.json` writer. UI layout, open tabs, drafts, recent files, file-tree sort, and Pandoc path use Kernel AppConfig; the native host stores only canonical desktop bootstrap metadata in `primary-workspace.json`.
- Current-notebook resources use the root lowercase `assets/` directory. Standalone saved documents use adjacent `assets/` only for clipboard bytes; existing local resources remain filesystem references.
- `.qingyu/` and `.markra-sync/` stay excluded in both directions and are not read, migrated, rewritten, or deleted.

## Disposable Fixtures

Prepare one empty remote target and three isolated local fixtures. Replace placeholders only in the local test environment.

| Fixture | Purpose | Local contents |
| --- | --- | --- |
| A / notebook | Initial current notebook | `[NOTEBOOK_A]/A.md`, `[NOTEBOOK_A]/assets/a.png`, `[NOTEBOOK_A]/other.bin` |
| B / notebook | Second current notebook after switching | `[NOTEBOOK_B]/B.md`, `[NOTEBOOK_B]/outside.bin` |
| Standalone | Direct file editing without synchronization authority | `[STANDALONE_DIR]/standalone.md` |

Use a unique remote path or prefix for this run and snapshot it while empty.

## Onboarding And Current Notebook

- [ ] Start with fresh application data. Complete the existing desktop workspace-selection surface and choose `[NOTEBOOK_A]`; this pre-Kernel surface is distinct from Workspace Home.
- [ ] Confirm `[NOTEBOOK_A]` becomes the current notebook, synchronization remains off, and a ready workspace with no document shows Workspace Home without creating `Untitled.md` or an editable welcome document.
- [ ] Create and select a Markdown document, then restart QingYu. Confirm `[NOTEBOOK_A]`, the selected document, and its committed layout restore without another workspace prompt.
- [ ] Open Settings from a notebook document, a standalone document, and Workspace Home. Confirm every window shows the same current notebook and synchronization configuration.
- [ ] Switch to `[NOTEBOOK_B]` through File or Settings. Confirm the old notebook run is safely stopped before B is persisted and new runs use B's immutable root.

## AppConfig Cold Start And Workspace Home

- [ ] In workspace A, open two Markdown files, select the second, change the file-tree visibility or another layout field, quit, and relaunch. Confirm the selected file and committed layout restore from Kernel AppConfig.
- [ ] Switch to workspace B, create a different layout, then switch back to A. Confirm each stable workspace identity restores only its own tabs, active file, drafts, recent files, and sort state.
- [ ] Delete all remembered source files with no dirty draft, then relaunch. Confirm Workspace Home appears and the stale paths are pruned from committed AppConfig.
- [ ] Repeat after retaining a dirty draft for a deleted source. Confirm the recoverable draft editor appears instead of Workspace Home.
- [ ] Close the final tab. Confirm Workspace Home appears without creating a document, draft, history entry, or persisted Home flag.
- [ ] Snapshot the application configuration directory before and after the run. Confirm normal UI state changes only the Kernel-owned `settings.json`; there is no `local-state.json`, `desktop-ui-state.json`, or Tauri Plugin Store write. `primary-workspace.json` may change only when desktop workspace selection changes.

## Notebook Switching And Standalone Isolation

- [ ] Configure a complete WebDAV or S3-compatible target in Settings, enable synchronization, leave the Sync page, and run Sync now.
- [ ] Confirm only `A.md`, `assets/a.png`, and `other.bin` appear below remote `notes/NOTEBOOK_A/`.
- [ ] Switch to `[NOTEBOOK_B]`. Confirm the displayed provider, endpoint, remote root, and triggers remain unchanged while the current-notebook label changes to B.
- [ ] Run the sync shortcut and Settings Sync now. Confirm B appears only below `notes/NOTEBOOK_B/` and no new B content enters A's remote directory.
- [ ] Restart QingYu and confirm B restores. Switch back to A and confirm A resumes its own manifest and remote directory without downloading B.
- [ ] Open and save `standalone.md`. Confirm it never becomes a sync root and no sibling content appears remotely.
- [ ] If a current-notebook file is opened directly in another window, confirm its physical membership below A still makes it part of A's next scan; the window itself does not create a second authority.
- [ ] On a clean second-device app-data fixture, load the remote catalog, choose A, and confirm only `[PARENT]/NOTEBOOK_A/` is created or reused; B is not downloaded.

## Settings Save And Apply Boundaries

- [ ] Open Sync settings and change one field at a time: enabled state, provider, remote path, save trigger, interval, endpoint, account, and credential.
- [ ] After every field change, inspect ConfigRoot `sync-config.json`. Confirm the field persists immediately and atomically with a new revision beside, but independently from, Kernel AppConfig `settings.json`.
- [ ] Confirm no synchronization configuration or credential file is created anywhere below either notebook or the standalone directory.
- [ ] While the Sync page remains open, confirm automatic save and interval triggers are suspended and no final settings synchronization starts.
- [ ] Leave Sync for another settings category. Expect one final `settings-exit` apply after pending writes finish; automatic work may then resume.
- [ ] Re-enter Sync, change a field, and close or hide Settings. Expect one final apply and one editing-session close even if close is requested twice.
- [ ] Re-enter and leave without changing anything. Expect no final apply.
- [ ] Repeat a field change while offline. The field remains local, the failure is safe and retryable, and diagnostics contain no credential, endpoint query, response body, or authorization header.

## Notes And Portable Settings Scopes

- [ ] Change an allowlisted theme preference and also change UI layout, then synchronize. Confirm a validated portable `settings.json` appears below remote `app/`, while notes remain below their named `notes/<directory-name>/` directories.
- [ ] Inspect remote `app/settings.json`. Confirm it contains no UI layout, open tabs, drafts, recent files, file-tree sort, Pandoc path, `sync-config.json`, credentials, manifests, MCP policy/runtime files, themes packages, extensions, or workspace metadata.
- [ ] With a second disposable application-data fixture pointed at the same remote target, synchronize and confirm portable settings apply without replacing that device's current-notebook path or any local AppConfig field.
- [ ] Create an invalid remote settings payload. Confirm local settings remain unchanged, the payload is quarantined below local `sync-state/conflicts/`, and the run visibly fails without exposing its contents as credentials.

## Resource Matrix

- [ ] In a current-notebook document, paste clipboard image bytes. Expect `[NOTEBOOK_A]/assets/[COLLISION_SAFE_NAME]` and a document-relative Markdown path.
- [ ] In nested current-notebook notes, drop an existing local image, import an attachment, and insert a remote resource. Expect every stored copy below the root `assets/`, including while synchronization is disabled.
- [ ] Synchronize A. Confirm resources transfer through ordinary workspace sync; there is no independent image-upload destination.
- [ ] In the saved standalone document, paste clipboard bytes. Expect `[STANDALONE_DIR]/assets/[COLLISION_SAFE_NAME]` with no remote transfer.
- [ ] In the standalone document, drop or import an existing local file. Expect a filesystem reference and no copied or uploaded object.
- [ ] In the standalone document, insert a remote URL. Expect the URL to remain a reference.
- [ ] In an unsaved standalone document, paste clipboard bytes. Expect save-first behavior before an adjacent `assets/` path can be created.

## Protected Paths And Safe Status

- [ ] Create local sentinels below `[NOTEBOOK_A]/.qingyu/` and `[NOTEBOOK_A]/.markra-sync/`, then synchronize. Confirm neither directory appears remotely.
- [ ] Place equivalent sentinels on the disposable remote target. Confirm QingYu neither downloads nor deletes them.
- [ ] Confirm both directories are absent from the file tree, workspace search, and watcher-driven refreshes while ordinary files remain visible and synchronized.
- [ ] Confirm no migration is attempted and local sentinel bytes remain unchanged.
- [ ] Simulate unreachable, authorization, list, upload, and delete failures. Expect safe provider/operation/status/path context, no secrets, and no update to last-success time.
- [ ] For a successful run, expect completion time and upload/download/conflict/skip counts. Repeating an unchanged run must be stable and create no duplicate conflict.

## Deterministic Automation

Run from the repository root:

```sh
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
pnpm test
pnpm typecheck:test
pnpm build
git diff --check
```

The default suite uses local mocks. Real MinIO coverage is environment-gated; run `pnpm test:s3-sync:live` only when all documented `MARKRA_TEST_S3_*` variables point to a disposable isolated target. Run `pnpm tauri dev` for the actual desktop checks above. Desktop packaging can use `pnpm tauri build --debug` when the platform dependencies are installed. Record automated, desktop-GUI, and live-provider evidence separately; an unavailable GUI or live provider remains pending.
