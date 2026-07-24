# S3 Sync Desktop Acceptance

This checklist is the black-box gate for QingYu's user-visible S3 sync entry points. It must be run with a real desktop build and a real MinIO server. Mocked frontend tests and Rust-only calls do not satisfy this gate.

## Preconditions

- Build or run the desktop app from the commit under test.
- Create disposable local notebook directories named `A` and `B`, plus one standalone Markdown file outside both directories.
- Configure the MinIO connection under Settings > Sync.
- Enable Sync, select S3, and use a new remote prefix below:

```text
markra-sync-tests/desktop-<UTC timestamp>-<short suffix>/<trigger>/
```

- Keep access keys, secret keys, authorization headers, and signed URLs out of screenshots and reports.
- Prepare an independent way to inspect MinIO object keys, bytes, length, and ETag. The MinIO Console is acceptable. A signed request is also acceptable when credentials are injected through the process environment.

For each marker, record its SHA-256 before the trigger and compare it with the downloaded MinIO object bytes. A success toast without an object-byte comparison is not a pass.

## A. Settings Manual Sync

1. Use application remote root `<run-prefix>/manual-settings` and make local directory `A` the current notebook.
2. Create `A/manual-settings.md` with unique content, record its SHA-256, and change one portable preference such as theme or layout.
3. Open Settings > Sync and click **Sync now**.
4. Require the running state followed by a success state.
5. Verify MinIO contains `<remote-root>/notes/A/manual-settings.md` with the exact bytes and hash.
6. Verify `<remote-root>/app/settings.json` exists, contains only portable application settings, and contains no credentials, device path, or sync state.
7. Record the settings object bytes and identity, then verify **Last sync** changed from its previous value.
8. Click **Sync now** again without changing either side.
9. Verify both object identities and bytes remain unchanged and no conflict copy appears.
10. Delete only the isolated remote root and verify it lists zero objects.

## B. Native Menu and Shortcut Manual Sync

1. Use remote prefix `<run-prefix>/manual-native`.
2. Create `manual-menu.md`, then invoke File > Sync Now.
3. Verify exact bytes below `<remote-root>/notes/<current-directory-name>/`, Last sync, and a stable no-op second run.
4. Change the local bytes and invoke the configured sync shortcut, whose default is `CmdOrCtrl+Alt+R`.
5. Verify the same named-notebook object now contains the changed bytes and a new object identity.
6. Delete only the isolated prefix and verify zero objects remain.

## C. Sync After Save

### Enabled

1. Use remote prefix `<run-prefix>/save-enabled`.
2. Set **Sync after save** on and **Scheduled sync** to `0`.
3. Edit `save-trigger.md` with unique bytes and use QingYu's real Save action.
4. Do not invoke a manual sync.
5. Within the normal request-completion window, verify MinIO contains the saved bytes below `notes/<current-directory-name>/` and Last sync changed.
6. Save a second unique version and verify the same object changes again.

### Disabled Control

1. Turn **Sync after save** off and keep Scheduled sync at `0`.
2. Record the current MinIO ETag and bytes.
3. Save a third unique local version.
4. Wait 15 seconds without invoking another sync point.
5. Require the MinIO ETag and bytes to remain unchanged.
6. Run an authorized manual sync and verify the third version now reaches MinIO.
7. Delete only the isolated prefix and verify zero objects remain.

## D. Scheduled Sync

### Enabled

1. Use remote prefix `<run-prefix>/schedule-enabled`.
2. Turn **Sync after save** off and set Scheduled sync to `1` minute.
3. Create or edit `schedule-trigger.md`, then avoid Save-triggered or manual sync actions.
4. Record the local content hash and start time.
5. Wait up to one interval plus a 30-second grace period.
6. Verify MinIO contains the exact bytes below `notes/<current-directory-name>/`, Last sync changed, and the mutation time is within the bounded window.
7. Leave both sides unchanged for another interval and verify the ETag and bytes remain stable.

### Disabled Control

1. Set Scheduled sync to `0` and record the current MinIO ETag and bytes.
2. Change the local marker without invoking another sync point.
3. Wait 90 seconds.
4. Require the MinIO ETag and bytes to remain unchanged.
5. Delete only the isolated prefix and verify zero objects remain.

The in-progress overlap guard and notebook-switch barrier are covered by `useAppSyncCoordinator.test.tsx` and `useNotebookSwitchCoordinator.test.tsx`. During this black-box run, any duplicate completion, repeated conflict, old-root status publication, or second object mutation for one interval is a failure and must be investigated.

## E. Notebook Switching, Selective Restore, And Standalone Files

1. Use application remote root `<run-prefix>/named-notebooks` and make local directory `A` current.
2. Create unique files in A, run Sync now, and verify they exist only below `<remote-root>/notes/A/`.
3. Switch to local directory `B`. Confirm the provider, bucket, remote root, credentials, and trigger settings remain unchanged, then synchronize B.
4. Verify B exists only below `<remote-root>/notes/B/`; A's keys and bytes remain unchanged.
5. Switch back to A, add another A-only file, and synchronize. Verify A resumes its own remote directory without downloading B or publishing into B.
6. Confirm `<remote-root>/app/settings.json` keeps the same bytes and object identity across A -> B -> A when no portable preference changed.
7. Open and save the standalone Markdown file. Invoke Sync now while it is focused and verify the current notebook remains A and no standalone file or sibling enters either remote notebook directory.
8. Start a clean second-device application-data fixture with the same provider configuration. Open the cloud restore catalog and require the shallow name list to show A and B without recursively downloading either.
9. Select A and choose a local parent. Verify only `<parent>/A/` is created or reused and hydrated; `<parent>/B/` is absent.
10. If `<parent>/A/` already contains a local-only file, record provider request order or equivalent timestamped evidence that existing remote A content is hydrated before the local-only file is published.
11. Delete only `<remote-root>` and verify a second listing returns zero objects.

## Cleanup Gate

For every trigger prefix:

1. list the prefix;
2. delete only objects returned by that prefix listing;
3. list it again;
4. record `remaining objects: 0`.

Never delete the bucket root or a shared parent prefix. A test cannot be reported as passed while its cleanup status is unknown.

## Traceability

| Requirement | Automated evidence | Desktop evidence |
| --- | --- | --- |
| Local create/upload and remote create/download | `live_minio_uploads_local_create_and_downloads_remote_create` | Manual marker upload |
| Local and remote update | `live_minio_propagates_local_and_remote_updates` | Save and shortcut changed bytes |
| Delete both directions and changed survivors | Four `live_minio_*delete*` / `*survivor*` tests | Not repeated manually |
| First-sync and changed-both conflicts | `live_minio_preserves_both_*_as_conflict` | No conflict on no-op rerun |
| Conflict collision and stabilization | `live_minio_uses_unique_conflict_name_and_does_not_repeat_conflict` | No repeated conflict |
| Pagination and object-key topology | Pagination and topology live tests | Marker path visible in MinIO |
| Named notebook catalog and A -> B -> A isolation | `live_minio_named_notebooks_catalog_restore_switch_and_cleanup_exact_root` exercises the production catalog, S3 backend, and generic sync engine | Section E, required for the application service and UI |
| Selective restore and same-name hydration | Named-notebook live scenario plus restore transaction unit tests | Section E, required |
| Portable `app/settings.json` stability | Application settings/service contract tests plus named-notebook live object checks | Sections A and E, required |
| Checkpoint, concurrency, atomic replacement | Recovery live tests | Not repeated manually |
| Invalid credentials and missing bucket | Failure live tests | UI failure may be smoke-tested without recording secrets |
| Settings manual trigger | App contract tests plus live backend/engine coverage; these do not exercise the complete application service or UI | Section A, required |
| Native menu and shortcut trigger | App and shortcut contract tests plus live backend/engine coverage; these do not exercise the complete application service or UI | Section B, required |
| Sync-after-save enabled/disabled | App contract tests plus live backend/engine coverage; these do not exercise the complete application service or UI | Section C, required |
| Scheduled sync enabled/disabled | App/hook contract tests plus live backend/engine coverage; these do not exercise the complete application service or UI | Section D, required |
| Full cleanup | Every live scenario cleanup assertion | Cleanup Gate, required |

Record results with `docs/testing/s3-sync-test-report-template.md`.
