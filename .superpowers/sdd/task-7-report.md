# Task 7 Report: Real S3 and MinIO Verification

## Outcome

Task 7 is complete at code commit `50b4542dae9222c801dc8a1b9fac4c6e902c5dc1`.

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
- A read-only live verifier checks that the configured isolated prefix root contains no objects. It never deletes outside the exact per-run repository/prefix.
- Existing protected-settings transport and read-only connection tests remain part of the same live entry point.

The real service is not the only coverage for these behaviors. The pinned Dejavu scenario fixtures cover delete and conflict semantics, the Go/Rust interop suite covers both directions of failure-before-ref-publication, and local QingYu service/status tests cover non-blocking acceptance, persistence ordering, retry, and listener removal.

## Local Endpoint

Endpoint label: `local`.

Final isolated run prefix ID: `19012b6b-51c4-4a10-83a5-bd0d681b4a44`.

- `pnpm test:s3-sync:live` with Node 24.18.0:
  - QingYu application live tests: 4 passed, 0 failed;
  - Dejavu real-S3 matrix: 1 passed, 0 failed;
  - Dejavu matrix duration: 17.65 seconds;
  - process exit: 0.
- An explicit post-run root-list check for that exact prefix passed with an empty result.
- Earlier local wrapper prefix `78bf7a5e-3f43-4be1-971f-03fdda8e1b03` was also explicitly rechecked and was empty.

## Public Endpoint Parity

Endpoint label: `public`.

- QingYu application parity prefix ID: `7fdf3291-fd5e-408d-bf9a-e25da2858eba`.
- Dejavu diagnostic run isolation ID: `d41bf10b-61b0-4b45-a6e3-6464d788867e`.
- QingYu authenticated application parity: 3 passed, 0 failed in 32.71 seconds.
- Dejavu authenticated real-S3 matrix: 1 passed, 0 failed in 131.01 seconds.
- The existing 120-second scenario timeout was not changed. The scenario completed within it; exact repository cleanup accounted for the remaining wall time.
- Explicit post-run root-list verification for the QingYu parity prefix passed with an empty result.
- Earlier public wrapper prefix `84a3da65-7eae-42ed-86d4-795d8da7177a` was also explicitly rechecked and was empty.

## Cleanup Evidence

- Dejavu creates a random canonical repository UUID and always runs `delete_repository(repository_id)` after success, failure, timeout, or panic. The test returns success only when a following catalog read returns `NotFound`.
- The QingYu background restore test applies the same catalog deletion and `NotFound` verification.
- Protected-settings scenarios delete every object listed below their exact run/scenario prefix and require a second list to be empty.
- The new isolated-root verifier performs a final read-only recursive list against the exact `MARKRA_TEST_S3_PREFIX_ROOT` used by the run.
- Local application data and note roots use `tempfile::TempDir`; they are removed when each scenario exits.
- No bucket-wide delete, broad prefix delete, or cleanup of pre-existing objects was performed.

## Failure and Fix

The first expanded local run failed at the new delete assertion. The test initially expected the uploading client to report a remove in its merge result. The pinned Dejavu fixtures define the production behavior differently: the uploader publishes its local deletion with zero merge removals, while the second client reports one removal when applying the remote deletion. The assertion was corrected to that established behavior, and the expanded matrix then passed locally and publicly. The failed run's exact repository cleanup succeeded.

No product-code defect was found during Task 7, so no production behavior was changed.

## Regression Verification

- `cargo test -p qingyu-dejavu --lib --tests -- --test-threads=1`:
  - 231 library tests passed;
  - 1 provenance test passed;
  - 56 S3 HTTP/signing/catalog tests passed;
  - 5 pinned scenario tests passed;
  - no failures.
- QingYu repository-runner focused suite: 18 passed, 1 live test ignored, 0 failed.
- Remote transport focused suite without credentials: 1 WebDAV test passed, 3 live S3 tests ignored, 0 failed.
- `@markra/scripts` tests: 7 files and 64 tests passed.
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
