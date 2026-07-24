# QingYu Dejavu S3 Interoperability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a Dejavu-compatible S3 cloud implementation, QingYu repository catalog, real MinIO coverage, and bidirectional Go/Rust interoperability tests on top of the completed core crate.

**Architecture:** Implement SigV4 and S3 object operations inside `qingyu-dejavu`, with one validated prefix per remote repository. Keep the outer QingYu catalog separate from the encrypted `repo/` content. Prove S3 behavior with HTTP fixtures and a real MinIO server, then prove repository-format interoperability with Go and Rust clients sharing one local-cloud directory; pinned Dejavu's S3 adapter hardcodes `repo/` and cannot address QingYu's outer prefix directly.

**Tech Stack:** Rust, reqwest, AWS Signature Version 4, S3 ListObjectsV2 XML, Tokio, MinIO, Go Dejavu at the pinned commit, Node orchestration.

## Global Constraints

- Complete `2026-07-25-qingyu-dejavu-core-port.md` first; do not duplicate its entity, crypto, store, lock, or sync implementations.
- Remote repository content lives at `qingyu/repositories/<repository-id>/repo/` and must remain readable by the pinned Go Dejavu implementation when that prefix is presented as its `repo/` root.
- `metadata.json` is outside `repo/`, contains no local path, key, credential, signature, or note content, and never participates in Dejavu merge decisions.
- Preserve the Dejavu `lock-sync` JSON and timing; do not add ETag CAS, conditional PUT, conditional DELETE, or a lease service.
- Do not use QingYu's legacy logical-empty sentinel inside the new Dejavu repository.
- Keep request diagnostics secret-free and bound transport retries; never retry deterministic authentication, authorization, validation, decrypt, or integrity failures.
- Do not modify WebDAV or route product synchronization to this backend in this milestone.
- Every behavior starts with a failing HTTP, MinIO, or interoperability test and ends with a focused commit.

---

## Planned File Structure

```text
apps/desktop/src-tauri/crates/qingyu-dejavu/
  Cargo.toml
  src/
    catalog.rs                       # outer metadata and listing
    cloud/
      mod.rs                         # existing Cloud trait plus S3 exports
      s3.rs                          # Cloud implementation
      s3_signing.rs                  # SigV4 URL and headers
      s3_xml.rs                      # bounded ListObjectsV2 parser
  src/bin/dejavu-interop.rs          # test-only Rust client CLI
  tests/
    s3_http.rs                       # deterministic HTTP fixtures
    s3_minio.rs                      # opt-in real server tests
scripts/
  dejavu-interop-go/
    go.mod
    go.sum
    main.go                          # pinned Go client CLI
  test-dejavu-interop.mjs            # mixed-client orchestrator
package.json
```

### Task 1: Move reusable SigV4 semantics into the core crate

**Files:**
- Modify: `apps/desktop/src-tauri/crates/qingyu-dejavu/Cargo.toml`
- Modify: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/cloud/mod.rs`
- Create: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/cloud/s3_signing.rs`
- Modify: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/lib.rs`
- Create: `apps/desktop/src-tauri/crates/qingyu-dejavu/tests/s3_http.rs`
- Modify: `apps/desktop/src-tauri/Cargo.lock`

**Interfaces:**
- Produces: `S3Connection`, `S3AddressingStyle`, `S3TlsVerification`, `S3RequestSigner`.
- Produces: `S3Connection::object_url(&self, key: &str) -> Result<Url, CloudError>`.
- Produces: secret-redacted `Debug` output.

- [ ] **Step 1: Add dependencies and failing signing vectors**

Add exact crate dependencies:

```toml
hmac = "0.12.1"
percent-encoding = "2.3"
quick-xml = "0.39.2"
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls"] }
sha2 = "0.10.9"
url = "2"
```

Port the existing QingYu fixed-time SigV4 tests for path and virtual-hosted
addressing, blank region resolving to `auto`, encoded object keys, sorted query
parameters, and credential redaction. Add a test that an empty body hashes the
actual zero-byte payload and does not add `x-amz-meta-qingyu-logical-empty`.

- [ ] **Step 2: Run the signing test and verify RED**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml -p qingyu-dejavu --test s3_http signing
```

Expected: compile failure because the S3 types do not exist.

- [ ] **Step 3: Implement validated connection and signing types**

Use these public enums so the Tauri adapter can map its existing configuration
without the core depending on application types:

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum S3AddressingStyle { Auto, Path, VirtualHosted }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum S3TlsVerification { Verify, Skip }

#[derive(Clone)]
pub struct S3Connection {
    pub endpoint_url: url::Url,
    pub region: String,
    pub bucket: String,
    pub access_key_id: String,
    secret_access_key: String,
    pub addressing_style: S3AddressingStyle,
}
```

Construct a fresh `x-amz-date`, payload hash, credential scope, signing key, and
Authorization header for each request attempt. Sign the real zero-byte body for
empty refs or objects.

- [ ] **Step 4: Run signing tests and verify GREEN**

Run the focused test and the whole core crate. Expected: fixed signatures match
the committed vectors and secrets never appear in `Debug` or error strings.

- [ ] **Step 5: Commit S3 signing**

```bash
git add apps/desktop/src-tauri/crates/qingyu-dejavu apps/desktop/src-tauri/Cargo.lock
git commit -m "feat(sync): add Dejavu S3 signing"
```

### Task 2: Implement the S3 Cloud operations and bounded diagnostics

**Files:**
- Create: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/cloud/s3.rs`
- Create: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/cloud/s3_xml.rs`
- Modify: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/cloud/mod.rs`
- Modify: `apps/desktop/src-tauri/crates/qingyu-dejavu/tests/s3_http.rs`

**Interfaces:**
- Produces: `S3Cloud::new(connection, options, repository_prefix)`.
- Implements: the Plan 1 `Cloud` trait with repository-relative keys.
- Produces: `S3TransportOptions { request_timeout, tls_verification, max_attempts }`.

- [ ] **Step 1: Add failing HTTP fixture tests for every Cloud operation**

Use a bounded local TCP fixture to assert exact requests for:

- GET success and 404-to-`CloudError::NotFound`;
- PUT with `Cache-Control: no-cache`, including a true zero-byte body;
- DELETE without `If-Match`;
- ListObjectsV2 pagination with `continuation-token`;
- prefix stripping from `qingyu/repositories/repo-a/repo/objects/ab/cdef`;
- malformed, oversized, truncated, and cross-prefix XML results;
- transport failure and HTTP 408/429/500/502/503/504 retry up to three attempts;
- HTTP 400/401/403 and decrypt/integrity failures without retry.

- [ ] **Step 2: Run S3 operation tests and verify RED**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml -p qingyu-dejavu --test s3_http cloud_
```

- [ ] **Step 3: Implement S3Cloud with exact key confinement**

The constructor validates repository ID and creates this prefix exactly:

```text
<remote-root>/repositories/<repository-id>/repo
```

Every `Cloud` key is a slash-separated relative repository key. Reject empty
segments, `.`, `..`, backslashes, control characters, and absolute paths before
URL construction. `put(..., overwrite)` performs ordinary S3 PUT for both
boolean values because pinned Dejavu's S3 adapter ignores `overwrite` and relies
on `lock-sync` rather than conditional writes.

- [ ] **Step 4: Implement bounded request and XML handling**

Create a fresh signature for every retry. Buffer successful GET bodies under an
explicit size limit supplied by the caller. Parse ListObjectsV2 incrementally,
cap one page at 1000 entries, require continuation progress, and sort returned
`CloudObject` values by key. Map provider request IDs and status codes into
typed diagnostics without response bodies or credentials.

- [ ] **Step 5: Run tests and verify GREEN**

Run all `s3_http` and core crate tests. Expected: exact request counts, no stale
conditional headers, true empty objects, and no request hangs after a fixture
sends fewer responses than expected.

- [ ] **Step 6: Commit the S3 cloud adapter**

```bash
git add apps/desktop/src-tauri/crates/qingyu-dejavu
git commit -m "feat(sync): implement Dejavu S3 cloud adapter"
```

### Task 3: Add the QingYu outer repository catalog

**Files:**
- Create: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/catalog.rs`
- Modify: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/lib.rs`
- Modify: `apps/desktop/src-tauri/crates/qingyu-dejavu/tests/s3_http.rs`

**Interfaces:**
- Produces: `RepositoryMetadata`, `RepositoryCatalogEntry`, `S3RepositoryCatalog`.
- Produces: `create`, `list`, `read`, `rename`, and `delete_repository` methods.
- Preserves: catalog metadata never enters `Repo`, `Store`, or sync diffs.

- [ ] **Step 1: Write failing catalog serialization and listing tests**

Use this exact metadata contract:

```rust
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepositoryMetadata {
    pub format_version: u32,
    pub repository_id: String,
    pub display_name: String,
    pub created_at: i64,
    pub updated_at: i64,
}
```

Assert `formatVersion == 1`, IDs are UUID strings, names are trimmed nonempty
user-visible values, duplicate IDs fail, malformed metadata is omitted with a
safe catalog issue, and a delete targets only one validated repository prefix.

- [ ] **Step 2: Run catalog tests and verify RED**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml -p qingyu-dejavu --test s3_http catalog_
```

- [ ] **Step 3: Implement catalog operations separately from Cloud**

List direct `repositories/<id>/metadata.json` objects, reject nested or invalid
IDs, read bounded JSON, and sort entries by display name then ID. Create writes
metadata before any repo object. Rename updates only display name and
`updatedAt`. Delete lists then deletes every object under exactly the selected
repository prefix after the caller has confirmed; the catalog API itself does
not infer confirmation.

- [ ] **Step 4: Run tests and verify GREEN**

Expected: metadata paths never appear in `Cloud::list("")`, and repository
deletion cannot escape the selected prefix.

- [ ] **Step 5: Commit the catalog**

```bash
git add apps/desktop/src-tauri/crates/qingyu-dejavu
git commit -m "feat(sync): add QingYu Dejavu repository catalog"
```

### Task 4: Prove remote lock and latest-sequence behavior over S3

**Files:**
- Modify: `apps/desktop/src-tauri/crates/qingyu-dejavu/tests/s3_http.rs`
- Modify: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/cloud/s3.rs`
- Modify: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/sync.rs`

**Interfaces:**
- Consumes: `Repo::sync`, `S3Cloud`, `RemoteLockGuard`.
- Produces: HTTP-level evidence for `lock-sync`, `refs/latest`, and `refs/latest-<seq>-<id>` ordering.

- [ ] **Step 1: Add a failing scripted HTTP sync test**

Record the complete request sequence for a one-file first sync. Assert:

1. lock GET/PUT occurs before repository mutations;
2. file and chunk objects upload before the index;
3. index uploads before `refs/latest`;
4. sequence ref uses one greater than the largest listed sequence;
5. stale sequence refs are deleted only after the new latest is visible;
6. `lock-sync` is refreshed during a long paused upload and deleted at the end;
7. no request contains `If-Match` or `If-None-Match`.

- [ ] **Step 2: Run the scripted test and verify RED**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml -p qingyu-dejavu --test s3_http full_sync_request_order
```

- [ ] **Step 3: Correct only S3 publication gaps exposed by the test**

Keep the Plan 1 state machine unchanged. Add S3 list/publish support needed by
the existing sequence-ref hook, ensure lock refresh shares the same validated
prefix, and preserve object upload ordering through awaited operations.

- [ ] **Step 4: Run HTTP and core scenarios and verify GREEN**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml -p qingyu-dejavu --test s3_http
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml -p qingyu-dejavu --test scenarios
```

- [ ] **Step 5: Commit remote-order coverage**

```bash
git add apps/desktop/src-tauri/crates/qingyu-dejavu
git commit -m "test(sync): lock Dejavu S3 publication order"
```

### Task 5: Add real MinIO Rust coverage

**Files:**
- Create: `apps/desktop/src-tauri/crates/qingyu-dejavu/tests/s3_minio.rs`
- Modify: `package.json`
- Modify: `scripts/test-s3-sync-live.mjs`

**Interfaces:**
- Produces: opt-in test target selected by `QINGYU_S3_LIVE_TESTS=1`.
- Consumes: existing MinIO environment variables without committing credentials.

- [ ] **Step 1: Add a failing opt-in live test**

The test must create a unique `qingyu/repositories/<uuid>` prefix and cover:

- catalog create/list/read;
- first upload from client A;
- download by client B;
- independent changes merged by B then observed by A;
- same-path conflict retaining B local content and storing A remote content in B history;
- lock contention between A and B;
- cleanup of only the unique test prefix in a finally-style guard.

When `QINGYU_S3_LIVE_TESTS` is absent, print one explicit skipped-test message and
return without contacting a server.

- [ ] **Step 2: Run against configured MinIO and verify RED**

```bash
QINGYU_S3_LIVE_TESTS=1 cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml -p qingyu-dejavu --test s3_minio -- --nocapture
```

Expected: initial failure at the first uncovered real-server behavior, not a
credential dump.

- [ ] **Step 3: Fix transport-only real-server incompatibilities**

Adjust addressing, XML namespace handling, region, timeouts, or zero-byte
requests only where the real request proves a mismatch. Do not change merge,
lock, or object format expectations to make the live test pass.

- [ ] **Step 4: Integrate the test into the existing live script**

Extend `pnpm test:s3-sync:live` so it runs the existing suite and this crate test
using the same environment. Preserve cleanup on failure and secret redaction.

- [ ] **Step 5: Run and verify GREEN**

Run the focused MinIO test twice to prove cleanup and prefix uniqueness, then
run `pnpm test:s3-sync:live`. Expected: all live scenarios pass on both runs.

- [ ] **Step 6: Commit live coverage**

```bash
git add apps/desktop/src-tauri/crates/qingyu-dejavu/tests/s3_minio.rs scripts/test-s3-sync-live.mjs package.json
git commit -m "test(sync): cover Dejavu Rust sync on MinIO"
```

### Task 6: Add mixed Go/Rust repository interoperability tests over one local cloud

**Files:**
- Create: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/bin/dejavu-interop.rs`
- Create: `scripts/dejavu-interop-go/go.mod`
- Create: `scripts/dejavu-interop-go/go.sum`
- Create: `scripts/dejavu-interop-go/main.go`
- Create: `scripts/test-dejavu-interop.mjs`
- Modify: `apps/desktop/src-tauri/crates/qingyu-dejavu/Cargo.toml`
- Modify: `package.json`

**Interfaces:**
- Produces: `pnpm test:dejavu-interop`.
- Produces: two test CLIs accepting the same JSON command envelope.
- Uses: one test key, device-specific IDs, local data/repo/history/temp paths, and one scenario-owned local-cloud directory.

- [ ] **Step 1: Define one explicit CLI protocol and failing orchestrator**

Both CLIs read one JSON request from stdin and write one JSON response to stdout:

```json
{
  "operation": "index-and-sync",
  "deviceId": "rust-a",
  "dataPath": "/absolute/path",
  "repoPath": "/absolute/path",
  "historyPath": "/absolute/path",
  "tempPath": "/absolute/path",
  "keyBase64": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
  "cloudRoot": "/tmp/qingyu-dejavu/cloud/scenario-a/repo",
  "failBeforeRefPublication": false
}
```

The response contains only index ID, upsert/remove/conflict counts, and safe
error code. Add an `inspect` operation only. The Node script owns the complete
temporary root and must fail before either CLI exists.

- [ ] **Step 2: Run the orchestrator and verify RED**

```bash
pnpm test:dejavu-interop
```

Expected: failure that the Rust or Go client executable is missing.

- [ ] **Step 3: Implement the Rust CLI behind an explicit feature**

Add:

```toml
[features]
interop-cli = []

[[bin]]
name = "dejavu-interop"
path = "src/bin/dejavu-interop.rs"
required-features = ["interop-cli"]
```

The CLI constructs only public crate APIs, including Plan 1's `LocalCloud`,
writes diagnostics to stderr, never prints key material, and exits nonzero after
emitting a safe JSON error.

- [ ] **Step 4: Implement the Go CLI against the pinned module**

Pin `github.com/siyuan-note/dejavu` to commit
`8462fe30163c6e6e95ae2da832cfe76058e0e830` in `go.mod`. Map the request into
`dejavu.NewRepo` and `cloud.NewLocal`, using the parent of `cloudRoot` as
`Endpoint` and its basename as `Dir`. Keep the Go module isolated below
`scripts/dejavu-interop-go` so it does not affect pnpm or Cargo dependency
resolution.

For `failBeforeRefPublication`, wrap the upstream `cloud.Cloud` by delegating
all methods and overriding `UploadObject` and `UploadBytes` only when the
repository-relative key is `refs/latest`. Return a deterministic injected error
after all preceding uploads so the recovery scenario exercises the real
publication boundary without changing Dejavu source.

- [ ] **Step 5: Implement mixed scenarios in the Node orchestrator**

Use a unique local-cloud directory for each scenario and execute:

1. Go creates, Rust downloads and changes, Go observes;
2. Rust creates, Go downloads and changes, Rust observes;
3. Go and Rust edit independent paths then converge;
4. Go and Rust edit the same path and agree on conflict count and retained bytes;
5. terminate one client after object upload but before ref publication, then let the other converge;
6. always delete only the orchestrator-owned temporary scenario root.

This command requires neither S3 credentials nor MinIO. S3 transport behavior
is covered independently by Tasks 1–5; this task isolates repository-format and
state-machine interoperability from upstream Dejavu's hardcoded S3 `repo/`
prefix.

- [ ] **Step 6: Run mixed tests and verify GREEN**

```bash
pnpm test:dejavu-interop
```

Expected: every scenario converges, both clients decode each other's objects,
and no cleanup removes another scenario's directory.

- [ ] **Step 7: Commit mixed interoperability coverage**

```bash
git add apps/desktop/src-tauri/crates/qingyu-dejavu scripts/dejavu-interop-go scripts/test-dejavu-interop.mjs package.json
git commit -m "test(sync): prove Dejavu Go Rust interoperability"
```

### Task 7: Verify the S3 interoperability milestone

**Files:**
- Modify only files in this plan if verification reveals a scoped defect.

**Interfaces:**
- Produces: an S3 backend safe to integrate into the application in Plan 3.

- [ ] **Step 1: Run formatting and all crate tests**

```bash
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml --all -- --check
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml -p qingyu-dejavu
```

- [ ] **Step 2: Run Go and mixed oracles**

```bash
DEJAVU_SOURCE_DIR=/Volumes/extendData/Data/IdeaProjects/upstream/dejavu pnpm test:dejavu-oracle
pnpm test:dejavu-interop
```

Expected: shared JSON and mixed local-cloud scenarios pass.

- [ ] **Step 3: Run live MinIO when configured**

```bash
pnpm test:s3-sync:live
```

Expected: existing live coverage and new Dejavu live coverage pass. If the
server is not configured, report the explicit skip and do not claim a live pass.

- [ ] **Step 4: Run existing Rust tests and hygiene checks**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
git diff --check
git status --short
```

Expected: product behavior remains unchanged because no Tauri route uses the new backend yet.

- [ ] **Step 5: Commit any verification-only correction**

If required:

```bash
git add apps/desktop/src-tauri/crates/qingyu-dejavu scripts package.json apps/desktop/src-tauri/Cargo.lock
git commit -m "fix(sync): close Dejavu S3 interoperability gaps"
```

Do not create an empty commit.

## Self-review

- The new S3 adapter uses the Plan 1 `Cloud` trait and leaves the Plan 1 merge algorithm untouched.
- Remote prefixes, metadata fields, retry statuses, list limits, zero-byte behavior, and lock ordering have explicit S3 tests; mixed Go/Rust repository scenarios have explicit shared-local-cloud tests.
- The new repository never writes QingYu legacy manifests or `remote-conflict` files.
- Product S3 and all WebDAV routes remain unchanged until the later integration and cutover plans.
