# QingYu Dejavu Core Port Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build an independent Rust crate that reproduces the pinned Dejavu repository format, local indexing, merge behavior, locking, history, and all 27 upstream JSON sync scenarios without S3 or Tauri dependencies.

**Architecture:** Add `qingyu-dejavu` as a workspace member below the Tauri Rust root. Keep repository semantics in small modules, expose byte-oriented cloud and working-tree coordination traits, and use a local filesystem cloud in tests so the complete state machine is testable before adding S3 or application integration.

**Tech Stack:** Rust 2021, Tokio, serde, AES-256-GCM, scrypt, zstd, SHA-1, the pinned Go Dejavu test suite, Cargo tests.

## Global Constraints

- Dejavu source baseline is exactly `siyuan-note/dejavu@8462fe30163c6e6e95ae2da832cfe76058e0e830`.
- SiYuan source baseline is exactly `siyuan-note/siyuan@eef10568384e2e7cf547adb029ae46a72e43c287`.
- Preserve Dejavu JSON field names, SHA-1 identifiers, zstd framing compatibility, AES-GCM nonce-prefix layout, Rabin chunk boundaries, ref names, merge outcomes, and write ordering.
- Port semantics into focused Rust modules; do not mirror Go package globals or goroutine layout.
- Do not add Markdown merging, `.sy` parsing, `.siyuan`, Petal, official-cloud APIs, WebDAV, or Tauri code.
- Preserve upstream AGPL and Mulan PSL v2 attribution for translated source and fixtures.
- Use `apply_patch` for repository edits and keep `apps/desktop/src-tauri/Cargo.lock` as the only Rust lockfile for this workspace.
- Every behavior change starts with a failing focused test and ends with a focused commit.

---

## Planned File Structure

```text
apps/desktop/src-tauri/
  Cargo.toml                         # package plus Cargo workspace root
  crates/qingyu-dejavu/
    Cargo.toml                       # independent internal crate
    UPSTREAM.md                      # pinned sources and license provenance
    src/
      lib.rs                         # stable public exports only
      atomic_write.rs                # safer-write primitive
      chunker.rs                     # restic-compatible Rabin boundaries
      cloud/
        mod.rs                       # byte-oriented Cloud trait
        local.rs                     # test/reference filesystem cloud
      crypto.rs                      # KDF and AES-GCM wire format
      diff.rs                        # file/index difference functions
      entity.rs                      # File, Chunk, Index, CheckIndex, stats
      error.rs                       # typed errors and source mapping
      history.rs                     # sync conflict history
      indexer.rs                     # working-tree scan and snapshot creation
      purge.rs                       # reachability-based local purge
      ref_store.rs                   # latest/latest-sync refs
      repo.rs                        # Repo construction and public operations
      store.rs                       # encrypted objects and compressed indexes
      sync.rs                        # pinned Dejavu sync state machine
      sync_lock.rs                   # remote lock protocol
      working_tree.rs                # application coordination trait
    tests/
      scenarios.rs                   # Rust JSON scenario runner
      fixtures/dejavu/cases/         # exact upstream JSON cases
      fixtures/golden/               # Go-produced format vectors
scripts/
  test-dejavu-oracle.mjs             # pinned Go baseline plus Rust runner
package.json                         # test:dejavu-oracle command
```

### Task 1: Create the Rust crate boundary and provenance record

**Files:**
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Create: `apps/desktop/src-tauri/crates/qingyu-dejavu/Cargo.toml`
- Create: `apps/desktop/src-tauri/crates/qingyu-dejavu/UPSTREAM.md`
- Create: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/lib.rs`
- Create: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/error.rs`
- Create: `apps/desktop/src-tauri/crates/qingyu-dejavu/tests/provenance.rs`
- Modify: `apps/desktop/src-tauri/Cargo.lock`

**Interfaces:**
- Produces: crate `qingyu-dejavu`, imported as `qingyu_dejavu`.
- Produces: `UPSTREAM_DEJAVU_COMMIT` and `UPSTREAM_SIYUAN_COMMIT` constants.
- Produces: `RepoError`, the error type used by all later tasks.

- [ ] **Step 1: Add the workspace member and a failing provenance test**

Add a `[workspace]` section to the existing package manifest:

```toml
[workspace]
members = [".", "crates/qingyu-dejavu"]
resolver = "2"
```

Create the crate manifest with only core dependencies:

```toml
[package]
name = "qingyu-dejavu"
version = "0.1.0"
edition = "2021"
license = "AGPL-3.0-only"
publish = false

[dependencies]
aes-gcm = "0.10.3"
async-trait = "0.1.89"
base64 = "0.22.1"
filetime = "0.2.26"
getrandom = "0.4.3"
ignore = "0.4.28"
scrypt = "0.11.0"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sha1 = "0.10.6"
thiserror = "2"
time = "0.3.44"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "sync", "time"] }
zstd = "0.13.3"

[dev-dependencies]
tempfile = "3.27.0"

[target.'cfg(windows)'.dependencies]
windows-sys = { version = "0.61.2", features = ["Win32_Foundation"] }
```

Create `tests/provenance.rs`:

```rust
use qingyu_dejavu::{UPSTREAM_DEJAVU_COMMIT, UPSTREAM_SIYUAN_COMMIT};

#[test]
fn source_baselines_are_pinned() {
    assert_eq!(
        UPSTREAM_DEJAVU_COMMIT,
        "8462fe30163c6e6e95ae2da832cfe76058e0e830"
    );
    assert_eq!(
        UPSTREAM_SIYUAN_COMMIT,
        "eef10568384e2e7cf547adb029ae46a72e43c287"
    );
}
```

- [ ] **Step 2: Run the test and verify RED**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml -p qingyu-dejavu --test provenance
```

Expected: compile failure because the two constants are not exported.

- [ ] **Step 3: Add the minimal crate exports and typed error shell**

Create `src/lib.rs`:

```rust
mod error;

pub use error::RepoError;

pub const UPSTREAM_DEJAVU_COMMIT: &str =
    "8462fe30163c6e6e95ae2da832cfe76058e0e830";
pub const UPSTREAM_SIYUAN_COMMIT: &str =
    "eef10568384e2e7cf547adb029ae46a72e43c287";
```

Create `src/error.rs`:

```rust
#[derive(Debug, thiserror::Error)]
pub enum RepoError {
    #[error("repository I/O failed")]
    Io(#[from] std::io::Error),
    #[error("repository data is invalid: {0}")]
    InvalidData(&'static str),
    #[error("repository object was not found: {0}")]
    NotFound(String),
    #[error("repository operation was cancelled")]
    Cancelled,
}
```

Write `UPSTREAM.md` with the two repository URLs, exact commits, files translated in each later task, Dejavu AGPL attribution, encryption Mulan PSL v2 attribution, and restic chunker BSD-2-Clause attribution.

- [ ] **Step 4: Run the test and verify GREEN**

Run the provenance test again. Expected: one passing test and a single updated `apps/desktop/src-tauri/Cargo.lock`.

- [ ] **Step 5: Commit the crate boundary**

```bash
git add apps/desktop/src-tauri/Cargo.toml apps/desktop/src-tauri/Cargo.lock apps/desktop/src-tauri/crates/qingyu-dejavu
git commit -m "build(sync): add Dejavu Rust crate"
```

### Task 2: Port entities, JSON names, and identifiers

**Files:**
- Create: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/entity.rs`
- Create: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/diff.rs`
- Modify: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/lib.rs`
- Test: unit tests inside `entity.rs` and `diff.rs`

**Interfaces:**
- Produces: `File`, `Chunk`, `Index`, `CheckIndex`, `CheckIndexFile`, `MergeResult`, `TrafficStat`.
- Produces: `sha1_hex`, `random_hash`, `diff_upsert_remove`, and `diff_index_files`.
- Preserves: file equality compares path plus `updated / 1000`, not content hash.

- [ ] **Step 1: Write failing entity wire-format tests**

Add tests asserting:

```rust
#[test]
fn file_id_matches_dejavu_path_plus_second_timestamp() {
    let file = File::new("/doc.txt", 6, 1_700_000_000_123);
    assert_eq!(file.id, "3b6e9cfa0638699ff9f954594602518645ec38a0");
    assert_eq!(file.sec_updated(), 1_700_000_000);
}

#[test]
fn index_json_uses_go_field_names() {
    let value = serde_json::to_value(Index::fixture()).unwrap();
    assert!(value.get("systemID").is_some());
    assert!(value.get("checkIndexID").is_some());
    assert!(value.get("aesKeyVerifyVal").is_some());
    assert!(value.get("system_id").is_none());
}
```

Add diff tests proving equal-second timestamps are equal, different-second timestamps are upserts, and path removal appears only in `removes`.

- [ ] **Step 2: Run entity tests and verify RED**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml -p qingyu-dejavu entity::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml -p qingyu-dejavu diff::tests
```

Expected: compile failure because the modules and types do not exist.

- [ ] **Step 3: Implement the exact public entities and hashing rule**

Use these field declarations and serde names:

```rust
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct File {
    pub id: String,
    pub path: String,
    pub size: i64,
    pub updated: i64,
    pub chunks: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Chunk {
    pub id: String,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Index {
    pub id: String,
    pub memo: String,
    pub created: i64,
    pub files: Vec<String>,
    pub count: usize,
    pub size: i64,
    #[serde(rename = "systemID")]
    pub system_id: String,
    #[serde(rename = "systemName")]
    pub system_name: String,
    #[serde(rename = "systemOS")]
    pub system_os: String,
    #[serde(rename = "checkIndexID")]
    pub check_index_id: String,
    #[serde(rename = "aesKeyVerifyVal")]
    pub aes_key_verify_val: String,
}
```

`File::new` must hash `path + decimal(updated / 1000)` with SHA-1. `random_hash`
must read 32 random bytes and SHA-1 those bytes. Declare every remaining entity
field from the pinned Go source explicitly; do not use `serde(flatten)` or maps.

- [ ] **Step 4: Implement deterministic diff functions**

Implement `equal_file` as path plus second-resolution timestamp equality. Return
sorted `Vec<File>` results so tests and logs are stable without changing the
set of decisions made by the Go maps.

- [ ] **Step 5: Run tests and verify GREEN**

Run the focused tests, then:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml -p qingyu-dejavu
```

Expected: all crate tests pass.

- [ ] **Step 6: Commit entities and diffs**

```bash
git add apps/desktop/src-tauri/crates/qingyu-dejavu/src
git commit -m "feat(sync): port Dejavu entities and diffs"
```

### Task 3: Port key derivation, AES-GCM, zstd, and object storage

**Files:**
- Create: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/crypto.rs`
- Create: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/atomic_write.rs`
- Create: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/store.rs`
- Create: `apps/desktop/src-tauri/crates/qingyu-dejavu/tests/fixtures/golden/README.md`
- Create: binary fixtures below `tests/fixtures/golden/`
- Modify: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/entity.rs`
- Modify: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/lib.rs`

**Interfaces:**
- Produces: `derive_key`, `encrypt`, `decrypt`, `write_file_safer`, and `Store`.
- Produces: `Store::{put,get}_{chunk,file,index,check_index}` and object path helpers.
- Preserves: encrypted objects are `12-byte nonce || AES-GCM ciphertext+tag`; indexes are zstd-compressed but unencrypted.

- [ ] **Step 1: Add failing Go-golden compatibility tests**

Add fixtures generated by the pinned Go packages for:

- scrypt password `oracle-password`, salt `oracle-salt`, expected 32-byte key;
- AES-GCM encrypted plaintext `siyuan` using that key;
- encrypted+zstd `File` JSON;
- zstd-only `Index` JSON.

The Rust tests must decrypt/decode Go fixtures, verify every entity field, write
Rust equivalents, and verify a small Go reader can decode them in Task 8.

Add the KDF assertion:

```rust
#[test]
fn kdf_is_scrypt_32768_8_1_32() {
    let key = derive_key("oracle-password", "oracle-salt").unwrap();
    assert_eq!(key, include_bytes!("../tests/fixtures/golden/kdf-key.bin"));
}
```

- [ ] **Step 2: Run crypto/store tests and verify RED**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml -p qingyu-dejavu crypto::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml -p qingyu-dejavu store::tests
```

Expected: compile failure because crypto and store modules are absent.

- [ ] **Step 3: Implement cryptographic wire compatibility**

Use `scrypt::Params::new(15, 8, 1, 32)` for `N=32768`. AES encryption must
generate a 12-byte nonce, prepend it, use no AAD, and reject ciphertext shorter
than `12 + 16` bytes before slicing. Add `Index::init_aes_key_verify_val` and
`Index::verify_aes_key` using plaintext `b"siyuan"` and standard Base64.

- [ ] **Step 4: Implement the safer-write primitive**

`write_file_safer(path, bytes, mode)` must:

1. create a random same-directory `.tmp` file with `create_new(true)`;
2. write all bytes and call `sync_all`;
3. close the file and set mode `0644` on Unix;
4. rename to the destination;
5. on Windows access-denied/in-use rename failures, retry three times with 200ms waits;
6. remove only its own temporary file after failure.

Do not call parent-directory `fsync` in this port.

- [ ] **Step 5: Implement Store paths and codecs**

Use `objects/<first-two-id-chars>/<remaining-id-chars>`, `indexes/<id>`, and
`check/indexes/<id>`. Configure zstd with checksum disabled and a 512 KiB
window. Store file metadata and chunks as zstd then AES-GCM; store indexes and
check indexes as zstd only. Reject IDs that are not 40 lowercase hex characters
before constructing paths.

- [ ] **Step 6: Run tests and verify GREEN**

Run focused crypto/store tests and the entire crate. Expected: Go fixtures decode,
wrong keys return a typed decrypt error, truncated ciphertext never panics, and
all writes leave no owned `.tmp` file after success.

- [ ] **Step 7: Commit storage compatibility**

```bash
git add apps/desktop/src-tauri/crates/qingyu-dejavu
git commit -m "feat(sync): port Dejavu encrypted object store"
```

### Task 4: Port Rabin chunking, ignores, and snapshot indexing

**Files:**
- Create: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/chunker.rs`
- Create: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/indexer.rs`
- Create: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/repo.rs`
- Modify: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/error.rs`
- Modify: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/lib.rs`
- Create: `apps/desktop/src-tauri/crates/qingyu-dejavu/tests/fixtures/golden/chunk-boundaries.json`

**Interfaces:**
- Produces: `RepoPaths`, `Device`, `RepoOptions`, `Repo::open`, and `Repo::index`.
- Produces: `RabinChunker` with fixed polynomial `0x3DA3358B4DC173`.
- Produces: `RepoError::{EmptyIndex,IndexFileChanged,RepoFatal,UnsafePath}`.

- [ ] **Step 1: Add failing chunk and index tests**

Use a deterministic Go-generated byte stream larger than 16 MiB and commit its
expected `[offset,length,sha1]` chunk list. Test exact boundaries with:

```rust
assert_eq!(RabinChunker::new(bytes).collect::<Vec<_>>(), expected_chunks);
```

Add indexing tests for a small file, a multi-chunk file, hidden entries, `.tmp`,
symlinks, non-regular files, an empty directory, and a file modified between
scan and chunk read. Assert seven bounded retries only for
`RepoError::IndexFileChanged`.

- [ ] **Step 2: Run focused tests and verify RED**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml -p qingyu-dejavu chunker::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml -p qingyu-dejavu indexer::tests
```

Expected: compile failure because chunker and indexer are absent.

- [ ] **Step 3: Translate the restic-compatible Rabin algorithm**

Use a 64-byte rolling window, 512 KiB minimum, 8 MiB maximum, `(1 << 20) - 1`
split mask, and the fixed polynomial. Preserve the upstream table-generation,
slide, pre-read, maximum-boundary, and EOF rules. Record the restic source file
and BSD-2-Clause license in `UPSTREAM.md`.

- [ ] **Step 4: Implement safe recursive indexing**

Normalize repository paths to forward-slash absolute-within-repo form such as
`/folder/doc.md`. Reject traversal and symlink targets. Apply Dejavu built-in
ignores first, then compiled `syncignore` lines. Read chunks, re-stat the file,
and compare size plus second-resolution mtime before committing its `File`
object. Create `Index` fields and refs only after every upsert succeeds.

- [ ] **Step 5: Verify `.qingyu/syncignore` mapping without core special cases**

The core accepts explicit ignore lines and an explicit protected include path.
Test that `/.qingyu/syncignore` is indexed even though hidden paths are normally
ignored, while no `.siyuan` or user-guide rule appears in the core.

- [ ] **Step 6: Run tests and verify GREEN**

Run chunker, indexer, store, and complete crate tests. Expected: every boundary
matches the Go vector and changing a file can never produce a committed partial
index.

- [ ] **Step 7: Commit indexing**

```bash
git add apps/desktop/src-tauri/crates/qingyu-dejavu
git commit -m "feat(sync): port Dejavu indexing and chunking"
```

### Task 5: Port refs, checkout, conflict history, and local purge

**Files:**
- Create: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/ref_store.rs`
- Create: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/history.rs`
- Create: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/purge.rs`
- Modify: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/repo.rs`
- Modify: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/lib.rs`
- Test: unit tests in the new modules

**Interfaces:**
- Produces: `RefStore::{latest,latest_sync,update_latest,update_latest_sync}`.
- Produces: `Repo::{checkout_file,checkout_files,remove_files,purge}`.
- Produces: `History::store_remote_conflict(timestamp, relative_path, bytes)`.

- [ ] **Step 1: Write failing safety and reachability tests**

Cover valid 40-character refs, invalid/oversized refs, interrupted checkout,
conflict history path preservation, and purge reachability. The purge fixture
must contain two retained indexes, one unreferenced index, a shared chunk, an
unreachable chunk, and corresponding check indexes. Assert only unreachable
objects and the unreferenced check index are removed.

- [ ] **Step 2: Run focused tests and verify RED**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml -p qingyu-dejavu ref_store::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml -p qingyu-dejavu history::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml -p qingyu-dejavu purge::tests
```

- [ ] **Step 3: Implement refs and checkout**

Refs contain a trimmed 40-character ID and are written with `write_file_safer`.
Checkout reconstructs ordered chunks into a same-directory temporary file,
checks the final byte count, syncs, renames, and restores the `File.updated`
mtime. Deletes operate only on validated repository-relative regular-file
paths and remove empty directories after merge.

- [ ] **Step 4: Implement conflict history and purge**

History paths are `<history>/<YYYY-MM-DD-HHMMSS>-sync/<relative-path>` and use
safe writes. Purge reads every valid ref plus caller-provided retained index ID,
walks `Index -> File -> Chunk`, then removes only unreachable indexes, check
indexes, files, and chunks. Check cancellation between collection and each
destructive loop.

- [ ] **Step 5: Run tests and verify GREEN**

Run focused modules and the whole crate. Expected: purge statistics exactly
match deleted indexes and objects, and cancelled purge preserves remaining data.

- [ ] **Step 6: Commit repository lifecycle primitives**

```bash
git add apps/desktop/src-tauri/crates/qingyu-dejavu
git commit -m "feat(sync): port Dejavu refs history and purge"
```

### Task 6: Define cloud and working-tree coordination contracts and port the remote lock

**Files:**
- Create: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/cloud/mod.rs`
- Create: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/cloud/local.rs`
- Create: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/sync_lock.rs`
- Create: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/working_tree.rs`
- Modify: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/repo.rs`
- Modify: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/lib.rs`

**Interfaces:**
- Produces: `Cloud`, `CloudObject`, `CloudError`, `LocalCloud`.
- Produces: `WorkingTreeCoordinator`, `WorkingTreeChange`, `WorkingTreePermit`, `NoopWorkingTreeCoordinator`.
- Produces: `RemoteLockGuard`, acquired by `Repo::lock_cloud`.

- [ ] **Step 1: Add failing cloud, lock, and coordination tests**

Define tests around these exact interfaces:

```rust
#[async_trait::async_trait]
pub trait Cloud: Send + Sync {
    async fn get(&self, key: &str) -> Result<Vec<u8>, CloudError>;
    async fn put(&self, key: &str, bytes: &[u8], overwrite: bool) -> Result<u64, CloudError>;
    async fn remove(&self, key: &str) -> Result<(), CloudError>;
    async fn list(&self, prefix: &str) -> Result<Vec<CloudObject>, CloudError>;
    async fn available_size(&self) -> Result<u64, CloudError>;
}

#[async_trait::async_trait]
pub trait WorkingTreeCoordinator: Send + Sync {
    async fn prepare(
        &self,
        changes: &[WorkingTreeChange],
    ) -> Result<WorkingTreePermit, RepoError>;
    async fn release(&self, permit: WorkingTreePermit);
}
```

Test three 5-second acquisition attempts with paused Tokio time, 30-second
refresh, 65-second stale takeover, same-device reacquire, and three release
attempts. Test that every acquired working-tree permit is released on success
and error.

- [ ] **Step 2: Run focused tests and verify RED**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml -p qingyu-dejavu sync_lock::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml -p qingyu-dejavu working_tree::tests
```

- [ ] **Step 3: Implement LocalCloud and the coordination no-op**

`LocalCloud` stores keys below an owned test directory, rejects traversal,
implements overwrite behavior without S3 conditions, returns sorted listings,
and supports deterministic injected failures. `NoopWorkingTreeCoordinator`
returns a permit with no token and releases it without work.

- [ ] **Step 4: Port the lock protocol exactly**

Use key `lock-sync` and JSON fields matching the pinned Go `sync_lock.go`.
Acquisition reads existing lock data, accepts absent, stale, or same-device
locks, writes the current device/time, and verifies the stored value. Refresh
runs every 30 seconds until guard drop/release. Do not add ETag, CAS, or S3
conditional headers.

- [ ] **Step 5: Run tests and verify GREEN**

Run focused async tests and the whole crate. Expected: no real sleeps, exact
attempt counts, refresh stops after release, and another device cannot take a
fresh lock.

- [ ] **Step 6: Commit contracts and lock**

```bash
git add apps/desktop/src-tauri/crates/qingyu-dejavu
git commit -m "feat(sync): port Dejavu cloud lock contract"
```

### Task 7: Port the complete sync state machine and conflict results

**Files:**
- Create: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/sync.rs`
- Modify: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/repo.rs`
- Modify: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/entity.rs`
- Modify: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/error.rs`
- Modify: `apps/desktop/src-tauri/crates/qingyu-dejavu/src/lib.rs`
- Test: unit tests in `sync.rs`

**Interfaces:**
- Produces: `Repo::sync() -> Result<(MergeResult, TrafficStat), RepoError>` and `Repo::sync_download() -> Result<(MergeResult, TrafficStat), RepoError>`.
- Produces: `MergeResult { time, upserts, removes, conflicts }`.
- Preserves: asymmetric seven-minute filtering and ordinary-file local retention.

- [ ] **Step 1: Write failing state-machine branch tests**

Using two `Repo` instances and one `LocalCloud`, cover:

- first upload and first download with empty `latest-sync`;
- independent-path merge;
- local update plus cloud remove;
- local remove plus cloud update;
- same-path create and update conflicts;
- local upsert more than seven minutes older than cloud;
- cloud-too-old filtering in the non-local-upsert branch;
- conflict history creation even when the local working file is retained;
- `.tmp` cloud upsert exclusion;
- cloud `syncignore` filtering before removes;
- working-tree change after planning returning `RepoError::WorkingTreeChanged` without stale overwrite.

- [ ] **Step 2: Run sync tests and verify RED**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml -p qingyu-dejavu sync::tests
```

- [ ] **Step 3: Port download, upload, diff, and merge ordering**

Follow pinned `sync.go` function boundaries. Download missing file metadata and
chunks before restore; upload local missing data before publishing refs; build
`localUpserts/localRemoves` from latest versus latest-sync; build cloud changes
from cloud latest versus local latest; apply `filterLocalUpserts` before the
conflict loop; save remote conflict bytes before restoring files.

Port pinned `sync_manual.go` download-only behavior into `Repo::sync_download`.
It must use the same conflict-history and ordinary-file retention rules while
never uploading the resulting local index or refs.

- [ ] **Step 4: Port merged index and ref publication**

If local and cloud changed, index the restored working tree, upload all required
objects and check index, upload the index, then publish `refs/latest` and the
S3-capable `refs/latest-<seq>-<id>` hook through cloud listing semantics. Update
local latest and latest-sync only after remote publication succeeds. If only
cloud changed, use cloud latest directly.

- [ ] **Step 5: Integrate working-tree coordination**

Stage all remote bytes first. Call `prepare` only for paths that will be written
or removed, re-stat them against the planned `File` revisions, return
`WorkingTreeChanged` when any changed, and always call `release`. The caller,
not the merge algorithm, decides when to retry this typed outcome.

- [ ] **Step 6: Run tests and verify GREEN**

Run sync tests and the whole crate. Expected: exact conflict counts, local files
remain unchanged for ordinary conflicts, and the cloud latest converges to the
merged local-retained index.

- [ ] **Step 7: Commit the state machine**

```bash
git add apps/desktop/src-tauri/crates/qingyu-dejavu
git commit -m "feat(sync): port Dejavu sync state machine"
```

### Task 8: Run the same 27 JSON scenarios in Go and Rust

**Files:**
- Create: `apps/desktop/src-tauri/crates/qingyu-dejavu/tests/scenarios.rs`
- Create: four files under `apps/desktop/src-tauri/crates/qingyu-dejavu/tests/fixtures/dejavu/cases/`
- Create: `scripts/test-dejavu-oracle.mjs`
- Modify: `package.json`
- Modify: `apps/desktop/src-tauri/crates/qingyu-dejavu/UPSTREAM.md`

**Interfaces:**
- Produces: `pnpm test:dejavu-oracle`.
- Consumes: the public `Repo`, `LocalCloud`, entity, and summary APIs from Tasks 1-7.

- [ ] **Step 1: Add exact upstream JSON fixtures and a failing Rust parser test**

Add byte-for-byte copies of:

```text
test/sync/testdata/cases/basic/config.json
test/sync/testdata/cases/edge/config.json
test/sync/testdata/cases/known-conflicts/config.json
test/sync/testdata/cases/sync-download/config.json
```

Record each SHA-256 in `UPSTREAM.md`. The Rust test must assert the four files
contain `7 + 5 + 4 + 11 = 27` scenarios before executing them.

- [ ] **Step 2: Run the Rust scenario test and verify RED**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml -p qingyu-dejavu --test scenarios
```

Expected: failure on the first unsupported scenario operation.

- [ ] **Step 3: Implement every scenario operation explicitly**

Deserialize the upstream schema with `deny_unknown_fields`. Implement `write`,
`remove`, `rename`, `index`, `sync`, `sync_download`, and `assert` operations.
Set fixture mtimes from the scenario minute clock, create one local repo per
client, use one shared `LocalCloud`, and compare exact upsert/remove/conflict
counts after each sync step.

- [ ] **Step 4: Add the pinned Go oracle script**

`scripts/test-dejavu-oracle.mjs` must:

1. use `DEJAVU_SOURCE_DIR` when set and verify its `HEAD` equals the pinned commit;
2. otherwise clone `https://github.com/siyuan-note/dejavu.git` into an OS temporary directory and checkout the exact commit;
3. compare the four fixture SHA-256 values with the committed Rust copies;
4. run `go test ./test/sync -count=1 -v` and `go test ./... -count=1` in the pinned source;
5. run the Rust `scenarios` test;
6. exit nonzero on any mismatch or test failure and remove only its own temporary clone.

Add:

```json
"test:dejavu-oracle": "node scripts/test-dejavu-oracle.mjs"
```

- [ ] **Step 5: Run oracle and Rust suites and verify GREEN**

```bash
DEJAVU_SOURCE_DIR=/Volumes/extendData/Data/IdeaProjects/upstream/dejavu pnpm test:dejavu-oracle
```

Expected: all 27 Go scenarios, all upstream Go tests, and all 27 Rust scenarios pass.

- [ ] **Step 6: Commit the shared oracle**

```bash
git add apps/desktop/src-tauri/crates/qingyu-dejavu scripts/test-dejavu-oracle.mjs package.json
git commit -m "test(sync): share Dejavu Go scenarios with Rust"
```

### Task 9: Verify the standalone core milestone

**Files:**
- Modify only files in this plan if verification reveals a scoped defect.

**Interfaces:**
- Produces: a reviewable, S3-independent core milestone for Plan 2.

- [ ] **Step 1: Run formatting and crate tests**

```bash
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml --all -- --check
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml -p qingyu-dejavu
```

Expected: zero formatting differences and all crate tests pass.

- [ ] **Step 2: Run the Go oracle**

```bash
DEJAVU_SOURCE_DIR=/Volumes/extendData/Data/IdeaProjects/upstream/dejavu pnpm test:dejavu-oracle
```

Expected: 27/27 scenarios pass in both implementations.

- [ ] **Step 3: Run the existing Rust workspace suite**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
```

Expected: existing `markra` tests and the new crate tests pass without changing product behavior.

- [ ] **Step 4: Check repository hygiene**

```bash
git diff --check
git status --short
find apps/desktop/src-tauri -name Cargo.lock -print
```

Expected: no whitespace errors, only intended changes, and only
`apps/desktop/src-tauri/Cargo.lock` is printed.

- [ ] **Step 5: Commit any verification-only correction**

If verification required a scoped correction, commit only that correction:

```bash
git add apps/desktop/src-tauri/crates/qingyu-dejavu apps/desktop/src-tauri/Cargo.lock scripts/test-dejavu-oracle.mjs package.json
git commit -m "fix(sync): close Dejavu core verification gaps"
```

If no correction was required, do not create an empty commit.

## Self-review

- The milestone has no S3, Tauri, React, WebDAV, Markdown merge, or legacy QingYu manifest dependency.
- Exact source commits, crypto layout, chunk constants, paths, refs, lock timing, seven-minute behavior, and all 27 upstream scenarios are assigned to explicit tests.
- `Cloud` and `WorkingTreeCoordinator` names and signatures are defined once and consumed unchanged by later plans.
- Every destructive path is preceded by path validation and every committed repository file uses the specified safer-write primitive.
