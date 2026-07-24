# S3 Sync Test Report — 2026-07-16

## Build

- Date/time zone: 2026-07-16 Asia/Shanghai
- Automated live-suite commit: `5dbd14ffd4abfcb7e657a9cbac70440aaec3c88c`
- Desktop acceptance commit: `1be8338746a88b5c02c8f777216122df24a19c07`
- Final implementation commit: `8bdc7ab` (`fix(sync): serialize cross-window executions`)
- Branch: `codex/s3-note-sync`
- Platform: macOS
- App version: `1.7.0`

## MinIO Target

- Endpoint host: environment-injected local MinIO target
- Bucket: environment-injected test bucket
- Prefix root: disposable isolated test prefix
- Isolation: unique run and scenario prefixes
- Credentials recorded in this report: **No**

## Automated Live Integration

- Command: `pnpm test:s3-sync:live`
- Passed: 20
- Failed: 0
- Ignored in explicit live run: 0
- Duration: 30.83 seconds
- Cleanup remaining objects: 0 for every scenario, asserted by a final prefix listing

| Scenario group | Result | Evidence |
| --- | --- | --- |
| Harness and cleanup | Pass | Production backend uploaded, listed, downloaded, deleted, and re-listed an isolated marker |
| Create/read/update | Pass | Two local roots, both sync directions, exact bytes, and same-length changes |
| Topology, empty, binary, pagination | Pass | Encoded paths, non-ASCII, zero-byte object, binary bytes, and 1001-object listing |
| Delete and changed survivors | Pass | Both deletion directions and both delete-versus-change branches |
| Conflicts and stabilization | Pass | First-sync, changed-both, reserved-name collision, conflict upload, and final no-op |
| Manifest, target binding, ignored paths | Pass | Manifest entries/fingerprint, prefix reset, malformed JSON safety, and six ignored directories |
| Checkpoint, concurrency, atomicity | Pass | Partial checkpoint/retry, stale upload rejection, and temporary-file cleanup |
| Invalid credentials and missing bucket | Pass | Redacted errors and unchanged local/valid-bucket state |

## Defect Found During Live Testing

- Symptom: uploading an empty file returned HTTP 400 from MinIO.
- MinIO error code: `UnexpectedContent`.
- Root cause: byte-payload PUT headers omitted an explicit zero content length.
- Regression test: `s3_http::tests::signs_zero_length_put_with_explicit_content_length`.
- Fix commit: `3413df6` (`fix(s3): send content length for empty uploads`).
- Re-verification: signer tests 5/5 and the empty/binary topology live scenario passed; the final 20-test live run also passed.

## Offline Trigger and Runtime Evidence

- Trigger-related app tests: 267 passed, 0 failed.
- Desktop Tauri file bridge tests: 66 passed, 0 failed.
- App and desktop test TypeScript type checking: passed.
- Covered contracts: manual S3 menu, shortcut routing, save-after, canceled save, scheduled S3, disabled interval, in-progress suppression, provider dispatch, serialization, and no `lastSyncAt` update after failure.

## Desktop Entry Points

Status: passed against the debug desktop bundle and the real MinIO target.

| Trigger | Result | Notes |
| --- | --- | --- |
| Settings Sync now | Pass | Exact bytes for two objects; a second unchanged sync preserved both ETags and created zero conflict objects |
| File menu Sync now | Pass | File > Sync Now uploaded the changed marker with exact byte equality |
| Sync shortcut | Pass | `Cmd+Alt+R` uploaded a second changed marker with exact byte equality |
| Sync after save enabled/disabled | Pass | Enabled Save uploaded exact bytes; disabled Save left the prior remote bytes unchanged, then an authorized manual sync uploaded the pending version |
| Scheduled sync enabled/disabled | Pass | One-minute timer uploaded exact bytes; a second unchanged interval preserved ETag; interval `0` preserved the old remote ETag and bytes through a full 90-second control window |

Independent MinIO inspection used signed HEAD, GET, and ListObjectsV2 requests. Success toasts were never accepted without an object-byte comparison. The desktop prefixes were listed, deleted object-by-object, and re-listed with `KeyCount=0`.

The desktop run also backed up the existing QingYu settings before entering credentials. After the app stopped, the original settings file was restored and its SHA-256 matched the pre-test checksum. Disposable local note folders and temporary settings backups were removed.

## Defect Found During Desktop Testing

- Symptom: after switching from a large prior workspace to the disposable note folder and immediately reopening the cached settings window, Settings > Sync scanned the prior workspace at 100% CPU instead of the current folder.
- Evidence: a process sample stayed in `collect_local_sync_files` and SHA-256 while the isolated MinIO prefix remained empty.
- Root cause: the settings window loaded the workspace source only at mount; its 15-second idle cache retained the stale path across a workspace switch.
- Regression test: `useSettingsWindowState.test.ts` now changes the stored workspace after mount and requires manual sync to re-read the current source.
- Fix commit: `1be8338` (`fix(sync): refresh workspace before settings sync`).
- Re-verification: focused settings tests 11/11, app test type checking, rebuilt debug app, Settings > Sync success, and exact MinIO bytes.

## Defect Found During Final Code Review

- Symptom: the main window and cached settings window had independent in-progress flags, so simultaneous menu/shortcut/settings triggers could enter the shared sync engine concurrently.
- Risk: concurrent operations could race on the local manifest, temporary downloads, and remote objects.
- Regression test: `remote_sync::engine::tests::serializes_remote_sync_execution_across_entry_points` starts two entry points together and measures backend concurrency.
- Fix: the common remote-sync engine now holds a process-wide asynchronous mutex for each complete execution. This applies to S3, WebDAV, every UI entry point, and future `SyncProvider` implementations using the engine.
- Fix commit: `8bdc7ab` (`fix(sync): serialize cross-window executions`).
- Re-verification: the regression failed before the fix with maximum concurrency `2`, then passed with maximum concurrency `1`; all 43 remote-sync tests passed.

The separate Storage > Test connection action for S3 image upload returned an `error sending request` against this HTTP MinIO endpoint. Note-folder S3 sync uses its own Provider path and passed every automated and desktop gate above. The image-upload connection path is recorded as a separate follow-up rather than counted as a note-sync failure.

## Final Verification

- [x] Complete `docs/testing/s3-sync-desktop-acceptance.md` against the desktop app.
- [x] Run full `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml`: 257 passed, 0 failed, 20 explicit live tests ignored by the offline command.
- [x] Run `pnpm test:s3-sync:live` against the real MinIO server: 20 passed, 0 failed, 0 ignored.
- [x] Run full `pnpm test`: every workspace passed, including app 1840/1840, desktop 142/142, and web 41/41.
- [x] Run full `pnpm typecheck:test`.
- [x] Run full `pnpm build`, including desktop vendor-chunk verification.
- [x] Run `pnpm tauri build --debug`.
- [x] Run Rust formatting, workspace lint, and `git diff --check`.
- [x] Confirm desktop-prefix zero-object cleanup and restoration of the pre-test settings checksum.

Current overall result: **passed**. Automated real-MinIO integration, desktop black-box acceptance, full regressions, static checks, and debug application/DMG packaging all passed.
