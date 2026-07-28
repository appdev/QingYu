# Task 7 Report: Real S3 and MinIO Verification

## Outcome

Task 7 and review-fix round 1 are complete at code commit `d9cc9f08b99ec3469bc1dace14fab5ecd5effd79`.

The local and public test endpoints both passed authenticated S3 transport, QingYu application-service restore/status, protected-settings transport, and the expanded Dejavu sync matrix. Every scenario used a generated repository UUID or an isolated run prefix, and all exact repository/prefix cleanup checks passed. No credential or endpoint configuration was added to source control.

## Test Boundaries Added

- The Dejavu real-S3 matrix now explicitly covers:
  - initial upload;
  - second-device download;
  - independent edits from two devices;
  - create and delete propagation;
  - same-path conflict with local-wins working tree and remote history retention;
  - a one-shot failure immediately before `refs/latest` publication;
  - verification that failed publication does not advance `refs/latest`;
  - retry from the same local repository and convergence on the second device;
  - remote-lock contention and release.
- A QingYu application-level ignored live test now uses the production `DejavuRepositoryRunner`, `DejavuSyncService`, S3 cloud factory, and `RepositoryStatusStore` to upload from one application client and restore into an empty second client.
- The application-level test verifies the accepted job's `attempting` to `succeeded` event sequence, persisted job ID, terminal transfer counters, restored bytes, and exact catalog cleanup.
- A uniquely named read-only live verifier checks that the configured isolated prefix root contains no objects. The wrapper runs it in a separate Cargo child after the ordinary application live group, so test enumeration order cannot make the check run early. It never deletes outside the exact per-run repository/prefix.
- Existing protected-settings transport and read-only connection tests remain part of the same live entry point.
- Protected-settings and connection scenarios now catch a panic from the scenario future, always run exact-prefix cleanup and verification, then resume the original panic payload. When both an ordinary scenario and cleanup fail, the original scenario error remains primary.

The real service is not the only coverage for these behaviors. The pinned Dejavu scenario fixtures cover delete and conflict semantics, the Go/Rust interop suite covers both directions of failure-before-ref-publication, and local QingYu service/status tests cover non-blocking acceptance, persistence ordering, retry, and listener removal.

## Local Endpoint

Endpoint label: `local`.

Final review-round isolated run prefix ID: `9e7c210b-e857-4849-8d9d-d7d4de9cade9`.

- `pnpm test:s3-sync:live` with Node 24.18.0:
  - QingYu application scenario group: 3 passed, 0 failed;
  - independent final isolated-root verifier: 1 passed, 0 failed;
  - Dejavu real-S3 matrix: 1 passed, 0 failed;
  - Dejavu matrix duration: 17.63 seconds;
  - process exit: 0.
- The wrapper itself ran the independent root-list process after the application group, and it passed with an empty result.
- The previous final local prefix `19012b6b-51c4-4a10-83a5-bd0d681b4a44` had already passed an explicit empty-root recheck.
- Earlier local wrapper prefix `78bf7a5e-3f43-4be1-971f-03fdda8e1b03` was also explicitly rechecked and was empty.

## Public Endpoint Parity

Endpoint label: `public`.

- QingYu review-round application parity prefix ID: `240ea0c0-ae14-437e-b342-50d1724f5ae0`.
- Dejavu diagnostic run isolation ID: `d41bf10b-61b0-4b45-a6e3-6464d788867e`.
- QingYu authenticated application parity: 3 passed, 0 failed in 34.57 seconds.
- Independent final isolated-root verifier: 1 passed, 0 failed in 0.92 seconds.
- Dejavu authenticated real-S3 matrix: 1 passed, 0 failed in 131.01 seconds.
- The existing 120-second scenario timeout was not changed. The scenario completed within it; exact repository cleanup accounted for the remaining wall time.
- The uniquely named post-group root-list process verified the review-round parity prefix was empty.
- The previous public parity prefix `7fdf3291-fd5e-408d-bf9a-e25da2858eba` had already passed an explicit empty-root recheck.
- Earlier public wrapper prefix `84a3da65-7eae-42ed-86d4-795d8da7177a` was also explicitly rechecked and was empty.

## Cleanup Evidence

- Dejavu creates a random canonical repository UUID and always runs `delete_repository(repository_id)` after success, failure, timeout, or panic. The test returns success only when a following catalog read returns `NotFound`.
- The QingYu background restore test applies the same catalog deletion and `NotFound` verification.
- Protected-settings scenarios delete every object listed below their exact run/scenario prefix and require a second list to be empty. Success, ordinary error, and panic all enter the same cleanup path.
- The isolated-root verifier performs a read-only recursive list against the exact `MARKRA_TEST_S3_PREFIX_ROOT` used by the run. Its test name no longer matches the ordinary application group filter, and the wrapper invokes it in its own child process afterward.
- Local application data and note roots use `tempfile::TempDir`; they are removed when each scenario exits.
- No bucket-wide delete, broad prefix delete, or cleanup of pre-existing objects was performed.

## Failure and Fix

The first expanded local run failed at the new delete assertion. The test initially expected the uploading client to report a remove in its merge result. The pinned Dejavu fixtures define the production behavior differently: the uploader publishes its local deletion with zero merge removals, while the second client reports one removal when applying the remote deletion. The assertion was corrected to that established behavior, and the expanded matrix then passed locally and publicly. The failed run's exact repository cleanup succeeded.

No product-code defect was found during Task 7, so no production behavior was changed.

## Review Round 1 Disposition

Two Important review findings were accepted and fixed.

1. The protected-settings and connection test futures previously awaited their scenario before entering `finish_s3_scenario`; a panic could therefore skip cleanup. A generic `run_s3_scenario_with_cleanup` boundary now catches the scenario future, awaits cleanup unconditionally, preserves ordinary scenario-error precedence, and resumes the same panic after cleanup.
2. The isolated-root verifier previously shared the `live_minio_s3_` filter with the ordinary application group, so enumeration order could run it before other scenarios. It now has the unique filter `verify_live_s3_isolated_prefix_root_is_empty`, and `test-s3-sync-live.mjs` uses three ordered Cargo children: application scenarios, isolated-root verifier, then Dejavu. Application and Dejavu scenario failures remain ahead of verifier failures in the final exit code.

TDD evidence:

- Wrapper RED: 6 failures, including only two child invocations and incorrect verifier/Dejavu error precedence.
- Wrapper GREEN: 7 files and 66 tests passed.
- Rust RED: compile failure `E0425` because `run_s3_scenario_with_cleanup` did not exist.
- Rust GREEN: the recording fixture proved cleanup ran exactly once before the original sentinel panic was restored; a second test proved the original scenario error remains first when cleanup also fails.

## Regression Verification

- `cargo test -p qingyu-dejavu --lib --tests -- --test-threads=1`:
  - 231 library tests passed;
  - 1 provenance test passed;
  - 56 S3 HTTP/signing/catalog tests passed;
  - 5 pinned scenario tests passed;
  - no failures.
- QingYu repository-runner focused suite: 18 passed, 1 live test ignored, 0 failed.
- Remote transport focused suite without credentials: 3 tests passed, 3 live S3 tests ignored, 0 failed.
- `@markra/scripts` tests: 7 files and 66 tests passed.
- `pnpm typecheck:test`: all participating workspace packages passed.
- `cargo fmt`: passed.
- `git diff --check`: passed.
- Credential literal audit: zero matches in the diff and zero matches in tracked files.

## Security and Scope

- Credentials were parsed only inside the test process from the user-supplied temporary file.
- Credentials were not copied into source files, reports, environment files, or commits.
- Retained test evidence and this report use only `local` and `public` endpoint labels.
- `.serena/` was not modified, staged, or committed.
- No push, Task 8 UI operation, or merge into `main` was performed.

## Remaining Work

Task 8 must still run the actual Tauri application through Computer Use and verify interactive responsiveness while background restore is active. Task 7 proves the real S3 transport and production service/status seams, but it does not replace that UI/runtime test.
