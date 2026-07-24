# S3 Sync Test Strategy Design

## Goal

Define a repeatable verification process for S3 note-folder synchronization against a real MinIO server. The process must cover create, read, update, delete, all user-visible sync entry points, conflict handling, deletion propagation, checkpoint recovery, and immediate re-run idempotence.

Passing mocked frontend tests is not sufficient for acceptance. Manual sync, sync-after-save, and scheduled sync must each execute the production S3 request path against MinIO and produce verifiable local and remote state changes.

## Scope

- S3 note-folder sync in the desktop runtime.
- The shared `RemoteSyncBackend` engine behavior exercised through the production `S3Backend`.
- Local-to-remote and remote-to-local CRUD.
- Manual, save-after, and scheduled sync entry points.
- First sync, steady-state sync, deletion propagation, conflicts, retry, target changes, and common error cases.
- Regression tests for shared engine behavior and WebDAV dispatch where S3 changes could cause cross-provider regressions.

This strategy does not require an AWS account, an S3 SDK, a new production dependency, or destructive bucket-level operations.

## Chosen Approach

Use a layered test process with a mandatory real-MinIO acceptance lane:

1. Fast deterministic tests validate the complete synchronization decision matrix and frontend trigger rules.
2. Environment-gated Rust integration tests run the production S3 backend and common engine against MinIO.
3. Desktop black-box acceptance exercises every user-visible sync entry point against the same MinIO server.
4. Full repository tests and builds guard unrelated behavior.

The first layer identifies logic failures quickly. The MinIO lane proves request signing, addressing, XML parsing, object identity, mutation semantics, manifests, and real network behavior. The desktop lane proves that UI entry points reach that native path rather than only a mock.

## Test Environment and Isolation

The test environment is configured only through environment variables:

- `MARKRA_TEST_S3_ENDPOINT`
- `MARKRA_TEST_S3_REGION`, defaulting to `us-east-1` for the local MinIO server
- `MARKRA_TEST_S3_BUCKET`
- `MARKRA_TEST_S3_ACCESS_KEY_ID`
- `MARKRA_TEST_S3_SECRET_ACCESS_KEY`
- optional `MARKRA_TEST_S3_PREFIX_ROOT`, defaulting to `markra-sync-tests`

Credentials must never be committed, embedded in snapshots, printed in commands, or included in failure messages. Tests report only the endpoint host, bucket, run identifier, relative object key, operation, and HTTP status.

Each run creates an isolated prefix:

```text
<prefix-root>/<UTC timestamp>-<unique suffix>/<scenario>/
```

Every scenario receives a separate temporary local note directory and remote prefix. Tests never list, update, or delete the bucket root. Cleanup runs even after failure and then performs a final prefix listing. A run is not clean until the listing returns zero objects. If cleanup itself fails, the run identifier is retained in the error so an operator can remove only that isolated prefix.

Live tests are ignored or explicitly gated unless every required environment variable is present. Normal `cargo test` and `pnpm test` must remain offline and deterministic.

## Acceptance Oracle

Every live scenario is judged on all applicable state surfaces:

- local file existence and exact byte content;
- remote object existence, exact byte content, and length;
- `.markra-sync/s3-manifest.json` target fingerprint and entry state;
- returned synchronization summary counts and transferred bytes;
- conflict-copy filename and content when applicable;
- a stabilization sync followed by a no-op sync producing no additional mutations;
- no remote objects remaining below the isolated prefix after cleanup.

Content comparisons use byte hashes rather than timestamps. S3 object identity uses normalized ETag plus length when ETag is available; last-modified plus length is only the fallback when ETag is unavailable. Tests include ListObjects and HEAD returning different date formats so date formatting cannot cause a false change.

## Core State Matrix

The common engine retains deterministic in-memory coverage for every state transition. The real-MinIO suite selects representative paths from every action class.

| Baseline and current state | Expected action | Live MinIO required |
| --- | --- | --- |
| Local exists, remote absent, no manifest | Upload | Yes |
| Local absent, remote exists, no manifest | Download | Yes |
| Both exist, no manifest | Preserve local and download remote content as a local conflict copy | Yes |
| Both unchanged from manifest | Skip | Yes |
| Local changed, remote unchanged | Upload | Yes |
| Local unchanged, remote changed | Download | Yes |
| Both changed | Preserve local and download remote content as a local conflict copy | Yes |
| Local deleted, remote unchanged | Delete remote | Yes |
| Remote deleted, local unchanged | Delete local | Yes |
| Local deleted, remote changed | Download changed remote survivor | Yes |
| Remote deleted, local changed | Upload changed local survivor | Yes |

Each non-conflict live scenario ends with one immediate no-op re-run. The second summary must report zero uploads, downloads, and conflicts; direct state inspection must show that no local or remote delete occurred. Local bytes, remote bytes, and manifest entries must remain stable.

A conflict creates a new local conflict file below the synchronized folder. The next sync is allowed to upload that new file, but it must not create another conflict for the original remote revision. One additional sync must then be a complete no-op with stable local bytes, remote bytes, and manifest entries.

## Live MinIO Scenario Groups

### 1. Create and Read

- Create a root Markdown file locally, sync, list the prefix, and download the remote bytes for comparison.
- Create a nested remote object, sync into an empty second local directory, and compare local bytes.
- Cover spaces, non-ASCII characters, URL-sensitive characters, nested directories, an empty file, a binary attachment, and multiple files.
- Create enough objects to force ListObjectsV2 continuation pagination in a dedicated scenario.
- Confirm directory-marker objects are ignored and unsafe keys are rejected without writing outside the local root.

### 2. Update

- Establish a baseline, edit local content, sync, and verify remote replacement.
- Establish a baseline, replace remote content, sync, and verify local atomic replacement.
- Verify a size-preserving content change is detected.
- Verify ListObjects and HEAD date-format differences do not cause a repeated update when ETag and length are stable.

### 3. Delete

- Establish a baseline, delete the local file, sync, and verify remote deletion.
- Establish a baseline, delete the remote object, sync, and verify local deletion.
- Change the surviving side after deleting the other side and verify the changed survivor is restored rather than deleted.
- Delete nested files and verify no unrelated object under the run prefix changes.

### 4. Conflicts

- Create different local and remote content without a manifest and verify local preservation plus a local conflict file containing the remote bytes.
- Change both sides after a baseline and verify the same outcome.
- Verify the conflict filename preserves the extension and uses `remote-conflict-<UTC timestamp>`.
- Force a conflict-name collision and verify a unique name is selected without overwriting an existing conflict copy.
- Run sync again, allow the new local conflict file to upload, and verify the same remote revision does not create another conflict; the following run must be a complete no-op.

### 5. Manifest and Target Binding

- Verify the S3 manifest is separate from the WebDAV manifest.
- Change endpoint, bucket, or prefix fingerprint and verify the old baseline is cleared before planning the new target.
- Verify a malformed manifest fails safely without changing local or remote files.
- Verify `.markra-sync` and all fixed ignored directories are never uploaded.

### 6. Failure and Recovery

- Invalid access key or secret: fail before any local mutation and redact credentials.
- Missing bucket: return an actionable S3 error without creating local files.
- Unreachable endpoint: preserve local files and the last valid manifest.
- Inject a failure after at least one successful real-MinIO mutation: verify completed actions are checkpointed and retry resumes remaining work without replaying the completed mutation.
- Download real MinIO bytes into a path whose final atomic replacement is forced to fail: verify no partial destination replacement and no leftover temporary file.
- Mutate an object between list and HEAD/GET/PUT/DELETE: verify identity validation rejects the stale plan rather than overwriting concurrent changes.

Failure injection wraps the production S3 backend in test-only code. Successful operations before the injected failure still use the real MinIO server. No failure hooks are exposed in production settings or builds.

## User-Visible Sync Entry Points

All three entry points require a desktop black-box pass against real MinIO. Mocked Vitest coverage remains useful for trigger timing and disabled-state rules, but does not satisfy this gate.

### Manual Sync

1. Start the desktop app with an isolated local folder and S3 prefix.
2. Create a local marker file.
3. Trigger sync from Settings, then repeat with the native menu or shortcut.
4. Verify the marker object in MinIO, the success summary, `lastSyncAt`, and a no-op second run.

### Sync After Save

1. Enable S3 sync and sync-after-save.
2. Edit and save a note through the real editor save action.
3. Verify the saved bytes reach MinIO without a separate manual sync.
4. Disable sync-after-save, save a second change, and verify the remote object remains unchanged until another authorized sync point runs.

### Scheduled Sync

1. Configure the shortest supported nonzero interval.
2. Create or edit a local marker without invoking manual sync.
3. Wait for one interval plus a bounded grace period and verify the real MinIO mutation.
4. Verify no overlapping run starts while a sync is already in progress.
5. Disable the schedule, modify the marker, wait for the same window, and verify no remote mutation occurs.

For each entry point, logs must show the trigger type, provider `s3`, run identifier, and final counts without credentials. Remote verification must use object bytes/listing, not only a success toast.

## Trigger and Provider Regression Tests

Offline TypeScript tests continue to cover:

- S3 configuration validation and provider dispatch;
- manual Settings action and native menu/shortcut routing;
- sync-after-save only after a successful save;
- scheduled enable, disable, interval, and in-progress suppression behavior;
- WebDAV remaining on its own provider branch;
- `lastSyncAt` updating only after success.

Rust offline tests continue to cover the full action matrix, conflict naming, target fingerprint reset, path safety, checkpoint behavior, signer canonicalization, prefix normalization, ListObjects parsing, and stable ETag identity.

The live gate supplements these tests; it does not replace them.

## Execution Stages

### Stage A: Fast Change Gate

Run focused TypeScript trigger tests and focused Rust engine/S3 tests during development. This stage must not require network access.

### Stage B: Live MinIO Integration Gate

Run the environment-gated Rust suite against the configured MinIO server. Required groups are create/read, update, delete, conflict, manifest/target binding, failure/recovery, and cleanup verification.

### Stage C: Desktop Entry-Point Gate

Build or run the desktop app and complete the manual, save-after, and scheduled black-box scenarios using a fresh isolated prefix. Record the run prefix and outcome for every entry point.

### Stage D: Repository Regression Gate

Run:

```bash
pnpm test
pnpm typecheck:test
pnpm build
cd apps/desktop/src-tauri && cargo test
```

Use a debug Tauri build when the runtime bridge, packaging, or desktop acceptance setup changes.

## Reporting

A test report contains:

- commit hash and platform;
- MinIO endpoint host, bucket, and isolated run prefix, with credentials omitted;
- scenario group results and durations;
- summary counts for every sync run;
- failed relative path, operation, and status when applicable;
- cleanup result and remaining object count;
- desktop evidence for manual, save-after, and scheduled entry points;
- full regression command results.

The report must distinguish automated live integration, desktop black-box acceptance, offline unit tests, and build verification. A mocked trigger test may never be reported as a real-MinIO entry-point pass.

## Exit Criteria

S3 note sync is accepted only when:

- every core state-matrix action has deterministic engine coverage;
- every action class has at least one passing real-MinIO scenario;
- create, read, update, delete, conflicts, deletion survival, checkpoint retry, stabilization, and idempotence pass against MinIO;
- manual, sync-after-save, and scheduled entry points each cause a verified real MinIO object change through the desktop app;
- disabled triggers cause no MinIO change;
- secrets are absent from logs and reports;
- each test run cleans its isolated prefix to zero objects;
- WebDAV regression tests, full Rust tests, full workspace tests, type checking, and builds pass.
