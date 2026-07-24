# S3 Sync MinIO Tests

The live S3 sync suite runs QingYu's production `S3Backend` against a real MinIO server. The named-notebook scenario also calls the production shallow catalog and generic remote-sync engine. These tests do not exercise the complete application service, notebook-switch coordinator, settings reconciliation, or desktop UI; those remain separate runtime acceptance gates. The suite is intentionally separate from the default test suite so normal `cargo test` and `pnpm test` remain deterministic and offline.

## Configuration

Supply credentials through the process environment. Never commit them to this repository or include them in test reports.

Required variables:

- `MARKRA_TEST_S3_ENDPOINT`
- `MARKRA_TEST_S3_BUCKET`
- `MARKRA_TEST_S3_ACCESS_KEY_ID`
- `MARKRA_TEST_S3_SECRET_ACCESS_KEY`

Optional variables:

- `MARKRA_TEST_S3_REGION`, default `us-east-1`
- `MARKRA_TEST_S3_PREFIX_ROOT`, default `markra-sync-tests`

Use a shell session or secret manager that can inject these variables without saving them in a checked-in `.env` file. Test failures name missing variables but never print their values.

## Isolation

Every test process creates a unique run ID. Each scenario is restricted to:

```text
<prefix-root>/<run-id>/<scenario>/
```

The suite does not list or delete the bucket root. Cleanup lists only the scenario prefix, deletes each returned object with identity validation, then lists the same prefix again and requires zero remaining objects. Cleanup failures retain the run ID in the error so an operator can remove only the affected test prefix.

## Running

After injecting the variables, run:

```bash
pnpm test:s3-sync:live
```

The command selects only ignored tests whose names start with `live_minio_` and forces serial execution. Ordinary offline coverage remains available through:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync
```

The live CRUD group verifies:

- local create/upload followed by remote byte comparison;
- remote create/download into a second local root;
- same-length local and remote updates;
- nested, spaced, non-ASCII, and URL-sensitive object keys;
- empty files and binary attachments;
- ListObjectsV2 continuation with 1001 objects;
- manifest target/entry state and stable local, remote, and manifest state on a no-op re-run.

The live deletion group establishes a manifest baseline before verifying local-to-remote deletion, remote-to-local deletion, preservation of a changed remote survivor after local deletion, and preservation of a changed local survivor after remote deletion.

The live conflict and manifest group verifies first-sync conflicts, changed-both conflicts, collision-safe conflict names, conflict stabilization without repetition, prefix target rebinding, malformed manifest safety, and exclusion of every fixed ignored directory.

The named-notebook scenario creates `notes/A/`, `notes/B/`, and `app/settings.json` below one unique remote root. It verifies a shallow A/B catalog, restores only A into a same-name local directory, records remote hydration before local-only publication, exercises A -> B -> A with isolated manifests and object keys, keeps the settings object identity stable, and cleans then relists that exact remote root. It deliberately tests the provider, catalog, and engine boundary rather than claiming full application-service or UI coverage.

The live recovery group wraps the production backend with test-only failure controls to verify per-action checkpoints, retry without replay, concurrent remote mutation rejection, temporary-file cleanup after a forced atomic replacement failure, invalid-credential redaction, and missing-bucket local safety. Successful operations in these scenarios still use the real MinIO server.

## Reporting

A live test report may contain the commit hash, endpoint host, bucket, run ID, relative object path, operation, HTTP status, summary counts, and cleanup count. It must not contain access keys, secret keys, authorization headers, signed URLs, or signed query parameters.

Use [`s3-sync-test-report-template.md`](./s3-sync-test-report-template.md) to keep automated provider/catalog/engine evidence separate from runtime evidence. User-visible switching, selective restore, standalone-file isolation, triggers, and `app/settings.json` behavior require the separate [`s3-sync-desktop-acceptance.md`](./s3-sync-desktop-acceptance.md) checklist; an automated live pass must not be reported as a desktop application-service or UI pass.
