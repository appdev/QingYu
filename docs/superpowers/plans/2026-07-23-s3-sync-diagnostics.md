# QingYu S3 Sync Diagnostics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `QingYu.log`, persisted sync status, and the in-app runtime log identify safe S3 failure details instead of collapsing normal provider failures to `sync-run-failed`.

**Architecture:** Introduce one typed `RemoteSyncError` boundary shared by provider adapters and the sync engine. The S3 adapter constructs allowlisted diagnostics at the HTTP boundary, logs one structured failure record, and passes the same safe metadata through the service into status and frontend logging; successful runs emit compact lifecycle summaries.

**Tech Stack:** Rust 2021, Tauri v2, `tauri-plugin-log`, reqwest 0.12, quick-xml 0.39, serde, sha2, React, TypeScript, Vitest.

## Global Constraints

- Keep `QingYu.log` as the only authoritative rotating desktop diagnostic file; do not create a separate S3 log.
- Default logging records run lifecycle summaries and failures only; `QINGYU_SYNC_DIAGNOSTICS=1` enables sanitized successful-request `DEBUG` records.
- Never log credentials, authorization or signature values, complete endpoints, buckets, regions, remote roots, raw object keys, local paths, headers, or bodies.
- Preserve existing `HEAD 404` and delete `404` success semantics.
- Logging and provider-error parsing are best effort and must never change a synchronization result.
- Use the existing `tauri_plugin_log::log` re-export; do not add a new logging dependency.
- Preserve all unrelated dirty-worktree changes and stage only files belonging to the task being committed.
- For any file that was already modified before this plan began, inspect the original diff first and use `git add -p`; never stage a whole overlapping file without reviewing every hunk.
- Use `pnpm` for JavaScript workflows and do not add another package-manager lockfile.

---

## File Structure

- Create `apps/desktop/src-tauri/src/remote_sync/diagnostics.rs`: typed provider diagnostics, run IDs, privacy-safe object IDs, bounded S3 error parsing, structured native log records, and diagnostic-mode detection.
- Modify `apps/desktop/src-tauri/src/remote_sync/backend.rs`: replace backend `String` errors with `RemoteSyncError` while retaining an unclassified conversion for WebDAV and local engine failures.
- Modify `apps/desktop/src-tauri/src/remote_sync/engine.rs`: propagate `RemoteSyncError` through the engine and convert existing local strings at the boundary.
- Modify `apps/desktop/src-tauri/src/remote_sync.rs`: register the diagnostics module, adapt WebDAV to the typed backend error, and route S3 connection-test failures through sanitized diagnostics.
- Modify `apps/desktop/src-tauri/src/remote_sync/s3_backend.rs`: instrument S3 GET/HEAD/PUT/DELETE boundaries and return typed diagnostics.
- Modify `apps/desktop/src-tauri/src/remote_sync/catalog.rs`: attach a catalog diagnostic context to S3 notebook listing.
- Modify `apps/desktop/src-tauri/src/remote_sync/service.rs`: create one run ID, attach note/settings contexts, emit lifecycle records, and preserve typed provider failures.
- Modify `apps/desktop/src-tauri/src/remote_sync/live_tests.rs`: update test backend signatures to the typed error.
- Modify `apps/desktop/src-tauri/src/sync_config/status.rs`: persist safe diagnostic fields in `SyncSafeError` and cover serialization/redaction.
- Modify `packages/app/src/lib/sync-config.ts`: mirror the expanded safe error schema.
- Modify `packages/app/src/hooks/useAppSyncCoordinator.ts`: show the richer safe failure description without parsing raw provider prose.
- Modify `apps/desktop/src/runtime/tauri/sync-config/shared.ts`: write a compact successful synchronization summary through `appLogger` so it reaches both runtime and native file sinks.
- Modify focused Rust and TypeScript tests beside the files above.

---

### Task 1: Add the Typed Remote Sync Error Boundary

**Files:**
- Modify: `apps/desktop/src-tauri/src/remote_sync/backend.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/engine.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/service.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/live_tests.rs`

**Interfaces:**
- Produces: `RemoteSyncDiagnostic`, `RemoteSyncError`, `SyncFailureCategory`, and `SyncProviderOperation` in `remote_sync::backend`.
- Produces: `RemoteSyncError::unclassified(message)`, `RemoteSyncError::diagnostic(details)`, `RemoteSyncError::details()`, and `RemoteSyncError::safe_code()`.
- Consumes: existing `RemoteSyncBackend` implementations and engine result paths.

- [ ] **Step 1: Write failing backend type tests**

Add tests in `backend.rs` proving an unclassified error keeps its display text while a diagnostic error exposes only stable safe metadata:

```rust
#[test]
fn typed_remote_error_preserves_safe_diagnostics_without_parsing_display_text() {
    let diagnostic = RemoteSyncDiagnostic {
        category: SyncFailureCategory::Http,
        code: "s3-upload-http-failed".into(),
        http_status: Some(403),
        method: Some("PUT".into()),
        object_id: Some("object-a1".into()),
        operation: SyncProviderOperation::Upload,
        provider_error_code: Some("AccessDenied".into()),
        request_id: Some("request-1".into()),
        run_id: "run-1".into(),
        scope: "notes".into(),
    };
    let error = RemoteSyncError::diagnostic(diagnostic.clone());

    assert_eq!(error.safe_code(), "s3-upload-http-failed");
    assert_eq!(error.details(), Some(&diagnostic));
    assert_eq!(error.to_string(), "s3-upload-http-failed: S3 upload failed.");
}

#[test]
fn unclassified_remote_error_retains_existing_local_message() {
    let error = RemoteSyncError::unclassified("manifest-write-failed: unavailable");
    assert_eq!(error.safe_code(), "manifest-write-failed");
    assert!(error.details().is_none());
}
```

- [ ] **Step 2: Run the focused Rust test and verify it fails**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync::backend::tests::typed_remote_error -- --nocapture
```

Expected: compilation fails because the diagnostic types and constructors do not exist.

- [ ] **Step 3: Add the minimal typed error model**

Add these public-in-crate shapes to `backend.rs`:

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SyncFailureCategory {
    Http,
    Integrity,
    Local,
    Transport,
}

impl SyncFailureCategory {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Integrity => "integrity",
            Self::Local => "local",
            Self::Transport => "transport",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SyncProviderOperation {
    Catalog,
    Delete,
    Download,
    List,
    Metadata,
    Upload,
}

impl SyncProviderOperation {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Catalog => "catalog",
            Self::Delete => "delete",
            Self::Download => "download",
            Self::List => "list",
            Self::Metadata => "metadata",
            Self::Upload => "upload",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RemoteSyncDiagnostic {
    pub(crate) category: SyncFailureCategory,
    pub(crate) code: String,
    pub(crate) http_status: Option<u16>,
    pub(crate) method: Option<String>,
    pub(crate) object_id: Option<String>,
    pub(crate) operation: SyncProviderOperation,
    pub(crate) provider_error_code: Option<String>,
    pub(crate) request_id: Option<String>,
    pub(crate) run_id: String,
    pub(crate) scope: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RemoteSyncError {
    diagnostic: Option<RemoteSyncDiagnostic>,
    message: String,
}
```

Implement `Display`, `std::error::Error`, `From<String>`, and `From<&str>`. `RemoteSyncError::diagnostic` builds the message only from the stable code and operation:

```rust
let message = format!(
    "{}: S3 {} failed.",
    diagnostic.code,
    diagnostic.operation.as_str()
);
```

- [ ] **Step 4: Change the backend and engine result types**

Change `RemoteSyncBackend` methods to return `RemoteSyncError`:

```rust
async fn list_files(&self) -> Result<BTreeMap<String, RemoteSyncFile>, RemoteSyncError>;
async fn download(&self, path: &str, expected_identity: &str) -> Result<Vec<u8>, RemoteSyncError>;
async fn upload(
    &self,
    path: &str,
    bytes: &[u8],
    expected_identity: Option<&str>,
) -> Result<String, RemoteSyncError>;
async fn delete(&self, path: &str, expected_identity: &str) -> Result<(), RemoteSyncError>;
```

Change the engine public and locked result types from `String` to `RemoteSyncError`. Keep local helpers returning `String`; existing `?` expressions convert through `From<String>`. Update every test backend listed in File Structure so explicit failures use `.into()`:

```rust
return Err("recording list failed".into());
```

Update WebDAV backend methods by mapping their existing `String` bodies through `RemoteSyncError::from` without changing WebDAV messages or behavior.

- [ ] **Step 5: Run the backend and engine tests**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync::backend -- --nocapture
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync::engine -- --nocapture
```

Expected: both test selections pass with no behavior change.

- [ ] **Step 6: Commit the typed boundary**

```bash
git add apps/desktop/src-tauri/src/remote_sync/backend.rs apps/desktop/src-tauri/src/remote_sync/engine.rs apps/desktop/src-tauri/src/remote_sync.rs apps/desktop/src-tauri/src/remote_sync/service.rs apps/desktop/src-tauri/src/remote_sync/live_tests.rs
git commit -m "refactor(sync): preserve typed provider failures"
```

---

### Task 2: Build Privacy-Safe S3 Diagnostic Helpers

**Files:**
- Create: `apps/desktop/src-tauri/src/remote_sync/diagnostics.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync.rs`
- Test: `apps/desktop/src-tauri/src/remote_sync/diagnostics.rs`

**Interfaces:**
- Consumes: Task 1 `RemoteSyncDiagnostic`, `RemoteSyncError`, `SyncFailureCategory`, and `SyncProviderOperation`.
- Produces: `SyncDiagnosticContext::new(run_id, scope)`, `create_sync_run_id()`, `s3_object_id(context, relative_path)`, `s3_transport_failure(...)`, `s3_http_failure(...)`, `s3_integrity_failure(...)`, `record_sync_started(...)`, `record_sync_succeeded(...)`, and `record_sync_failed(...)`.

- [ ] **Step 1: Write failing sanitizer and parser tests**

Create `diagnostics.rs` with a test module first. Cover safe provider metadata, oversized values, opaque object IDs, and the diagnostic environment switch:

```rust
#[test]
fn s3_metadata_keeps_only_allowlisted_bounded_values() {
    assert_eq!(safe_s3_error_code("AccessDenied"), Some("AccessDenied".into()));
    assert_eq!(safe_s3_error_code("bad code /secret"), None);
    assert_eq!(safe_request_id("request-123"), Some("request-123".into()));
    assert_eq!(safe_request_id(&"x".repeat(257)), None);
}

#[test]
fn object_id_never_contains_the_relative_path() {
    let context = SyncDiagnosticContext::new("run-1", "notes");
    let object_id = s3_object_id(&context, "private/面试.md");
    assert!(!object_id.contains("private"));
    assert!(!object_id.contains("面试"));
    assert_eq!(object_id.len(), 16);
}

#[test]
fn diagnostic_switch_requires_exact_enabled_value() {
    assert!(!diagnostics_enabled_from_value(None));
    assert!(!diagnostics_enabled_from_value(Some("true")));
    assert!(diagnostics_enabled_from_value(Some("1")));
}
```

Add an XML parser test using a 403 body containing secrets in `Message`, `Resource`, and `HostId`; assert only `AccessDenied` is returned.

- [ ] **Step 2: Run the diagnostics test and verify it fails**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync::diagnostics -- --nocapture
```

Expected: compilation fails because the helper functions do not exist.

- [ ] **Step 3: Implement context, run IDs, and object IDs**

Use `OnceLock`, `AtomicU64`, `SystemTime`, and the existing `sha2` dependency:

```rust
#[derive(Clone, Debug)]
pub(crate) struct SyncDiagnosticContext {
    run_id: String,
    scope: String,
}

pub(crate) fn create_sync_run_id() -> String {
    static PROCESS_STARTED_MS: OnceLock<u128> = OnceLock::new();
    static RUN_SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let now_ms = unix_timestamp_ms();
    let process_ms = *PROCESS_STARTED_MS.get_or_init(|| now_ms);
    let sequence = RUN_SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1;
    format!("sync-{process_ms}-{now_ms}-{sequence}")
}

pub(crate) fn s3_object_id(context: &SyncDiagnosticContext, relative_path: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(context.run_id.as_bytes());
    digest.update([0]);
    digest.update(context.scope.as_bytes());
    digest.update([0]);
    digest.update(relative_path.as_bytes());
    format!("{:x}", digest.finalize())[..16].to_string()
}
```

- [ ] **Step 4: Implement bounded metadata parsing**

Define `MAX_S3_ERROR_BODY_BYTES = 64 * 1024`, `MAX_S3_ERROR_CODE_BYTES = 80`, and `MAX_REQUEST_ID_BYTES = 256`. Parse only the first XML `<Code>` text with quick-xml. Reject a body once incremental reads exceed 64 KiB. Do not retain `Message`, `Resource`, `HostId`, or unknown elements.

Use a separate pure helper:

```rust
fn parse_s3_error_code(bytes: &[u8]) -> Option<String> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut inside_code = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(start)) => inside_code = start.name().as_ref() == b"Code",
            Ok(Event::Text(text)) if inside_code => {
                return safe_s3_error_code(&text.decode().ok()?);
            }
            Ok(Event::Eof) | Err(_) => return None,
            _ => {}
        }
    }
}
```

- [ ] **Step 5: Implement allowlisted structured logging**

Serialize a private record containing only the approved fields and write it with the plugin re-export:

```rust
tauri_plugin_log::log::error!(
    target: "qingyu::sync",
    "S3 request failed {}",
    serde_json::to_string(&record).unwrap_or_else(|_| "{}".into())
);
```

Serialization failure logs `{}` and never changes the returned provider error. Detailed successful request logging is gated by a `OnceLock<bool>` initialized from `QINGYU_SYNC_DIAGNOSTICS`.

- [ ] **Step 6: Run diagnostics tests**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync::diagnostics -- --nocapture
```

Expected: all diagnostics tests pass.

- [ ] **Step 7: Commit the helper module**

```bash
git add apps/desktop/src-tauri/src/remote_sync.rs apps/desktop/src-tauri/src/remote_sync/diagnostics.rs
git commit -m "feat(sync): add safe S3 diagnostics"
```

---

### Task 3: Instrument S3 HTTP Operations

**Files:**
- Modify: `apps/desktop/src-tauri/src/remote_sync/s3_backend.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/catalog.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync.rs`
- Test: `apps/desktop/src-tauri/src/remote_sync/s3_backend.rs`

**Interfaces:**
- Consumes: Task 2 `SyncDiagnosticContext` and failure constructors.
- Produces: `S3Backend::with_diagnostic_context(context) -> Self` and typed diagnostics for list, catalog, metadata, download, upload, and delete.

- [ ] **Step 1: Write a failing `PUT 403` fixture test**

Extend the existing TCP S3 fixture so it returns `x-amz-request-id` and a bounded XML error:

```rust
#[test]
fn upload_403_returns_typed_access_denied_without_object_path() {
    let body = "<Error><Code>AccessDenied</Code><Message>private</Message></Error>";
    let response = format!(
        "HTTP/1.1 403 Forbidden\r\nx-amz-request-id: request-403\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let (endpoint_url, _, handle) = spawn_s3_responses_fixture(vec![
        not_found_response(),
        response,
    ]);
    let backend = s3_backend(endpoint_url)
        .with_diagnostic_context(SyncDiagnosticContext::new("run-403", "notes"));

    let error = tauri::async_runtime::block_on(backend.upload("private/面试.md", b"body", None))
        .expect_err("upload must fail");
    handle.join().unwrap();

    let diagnostic = error.details().expect("typed S3 failure");
    assert_eq!(diagnostic.code, "s3-upload-http-failed");
    assert_eq!(diagnostic.http_status, Some(403));
    assert_eq!(diagnostic.provider_error_code.as_deref(), Some("AccessDenied"));
    assert_eq!(diagnostic.request_id.as_deref(), Some("request-403"));
    assert!(!error.to_string().contains("面试"));
}
```

- [ ] **Step 2: Run the focused test and verify it fails**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml upload_403_returns_typed_access_denied -- --nocapture
```

Expected: compilation fails because the backend has no diagnostic context and still returns prose strings.

- [ ] **Step 3: Add diagnostic context to `S3Backend`**

Add `diagnostic_context: Option<SyncDiagnosticContext>` initialized to `None`, plus:

```rust
pub(crate) fn with_diagnostic_context(mut self, context: SyncDiagnosticContext) -> Self {
    self.diagnostic_context = Some(context);
    self
}
```

Connection-test and catalog constructors attach a `catalog` context with a new run ID. Application sync service constructors attach `notes` and `settings` contexts supplied by Task 4.

- [ ] **Step 4: Replace transport and status prose at each request boundary**

For every request, capture `Instant::now()` immediately before `.send()`. Replace raw error formatting with the Task 2 constructors. The upload pattern is:

```rust
let started_at = Instant::now();
let response = self.client.put(url).headers(headers).body(bytes.to_vec()).send().await
    .map_err(|error| s3_transport_failure(
        self.diagnostic_context.as_ref(),
        SyncProviderOperation::Upload,
        "PUT",
        relative_path,
        &error,
        started_at.elapsed(),
    ))?;
if !response.status().is_success() {
    return Err(s3_http_failure(
        self.diagnostic_context.as_ref(),
        SyncProviderOperation::Upload,
        "PUT",
        relative_path,
        response,
        started_at.elapsed(),
    ).await);
}
```

Use the equivalent operation/method pair for list, catalog, metadata, download, and delete. Continue treating metadata 404 as `Ok(None)` and delete 404 as `Ok(())` before constructing a failure.

- [ ] **Step 5: Type integrity failures**

Replace identity mismatch and post-upload missing-object prose with `s3_integrity_failure`, using codes `s3-object-changed` and `s3-upload-verification-failed`. Do not include the relative path in display text.

- [ ] **Step 6: Sanitize the connection-test error contract**

Change connection-test failures so they never include the remote prefix. Preserve safe status and provider code when present:

```text
sync-connection-test-failed: S3 GET failed (HTTP 403, AccessDenied, request request-403).
```

Transport failures remain:

```text
sync-connection-test-failed: S3 GET request failed.
```

- [ ] **Step 7: Add remaining focused tests**

Cover list 500, transport refusal, oversized XML, unsafe request ID, metadata 404 success, delete 404 success, upload verification failure, and detailed-mode redaction. Each test asserts endpoint, bucket, access key, secret, prefix, and raw path sentinels are absent from `RemoteSyncError::to_string()` and `RemoteSyncDiagnostic` string fields.

- [ ] **Step 8: Run S3 and catalog tests**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync::s3_backend -- --nocapture
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync::catalog -- --nocapture
```

Expected: all S3 backend and catalog tests pass.

- [ ] **Step 9: Commit S3 instrumentation**

```bash
git add apps/desktop/src-tauri/src/remote_sync.rs apps/desktop/src-tauri/src/remote_sync/catalog.rs apps/desktop/src-tauri/src/remote_sync/s3_backend.rs
git commit -m "feat(sync): log actionable S3 failures"
```

---

### Task 4: Preserve Diagnostics Through Service and Status

**Files:**
- Modify: `apps/desktop/src-tauri/src/remote_sync/service.rs`
- Modify: `apps/desktop/src-tauri/src/sync_config/status.rs`
- Test: `apps/desktop/src-tauri/src/remote_sync/service.rs`
- Test: `apps/desktop/src-tauri/src/sync_config/status.rs`

**Interfaces:**
- Consumes: Tasks 1-3 typed backend error and diagnostic context.
- Produces: expanded `SyncSafeError` fields and one lifecycle log record at run start and terminal completion.

- [ ] **Step 1: Write failing status serialization test**

Add a failed status with full safe metadata and assert camelCase serialization:

```rust
let error = SyncSafeError {
    category: Some("http".into()),
    code: "s3-upload-http-failed".into(),
    http_status: Some(403),
    method: Some("PUT".into()),
    object_id: Some("object-a1".into()),
    operation: "upload".into(),
    provider: SyncProvider::S3,
    provider_error_code: Some("AccessDenied".into()),
    relative_path: None,
    request_id: Some("request-403".into()),
    run_id: Some("run-1".into()),
};
let value = serde_json::to_value(error).unwrap();
assert_eq!(value["httpStatus"], 403);
assert_eq!(value["providerErrorCode"], "AccessDenied");
assert_eq!(value["requestId"], "request-403");
assert!(value["relativePath"].is_null());
```

- [ ] **Step 2: Run status tests and verify they fail**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml sync_config::status -- --nocapture
```

Expected: compilation fails because the new safe fields do not exist.

- [ ] **Step 3: Expand `SyncSafeError` safely**

Add optional `category`, `method`, `object_id`, `provider_error_code`, `request_id`, and `run_id` fields with `#[serde(default)]`. Keep `relative_path` present for wire compatibility but set it to `None` for all new S3 diagnostics. Keep sync status version `1` because the fields are optional and existing readers already deserialize by field name.

- [ ] **Step 4: Preserve provider diagnostics in `SyncRunError`**

Add `diagnostic: Option<RemoteSyncDiagnostic>` to `SyncRunError`. Replace `scoped_sync_pair_result` string handling with:

```rust
fn remote_run_error(
    error: RemoteSyncError,
    revision: &str,
    trigger: SyncTrigger,
    partial_summary: SyncSummary,
) -> SyncRunError {
    let code = error.safe_code().to_string();
    SyncRunError {
        code,
        diagnostic: error.details().cloned(),
        partial_summary,
        revision: revision.to_string(),
        trigger,
    }
}
```

Local `run_error` sets `diagnostic: None` and retains the existing safe-code fallback.

- [ ] **Step 5: Create one run ID and attach note/settings contexts**

At the start of `run_application_sync_with_source`, create one run ID and start time. Pass clones into S3 backends:

```rust
let run_id = create_sync_run_id();
record_sync_started(&run_id, provider, trigger, source.retains_source_directory());

let notes_backend = S3Backend::new_at_validated_prefix(notes_settings)?
    .with_diagnostic_context(SyncDiagnosticContext::new(&run_id, "notes"));
let settings_backend = S3Backend::new_at_validated_prefix(settings_settings)?
    .with_diagnostic_context(SyncDiagnosticContext::new(&run_id, "settings"));
```

Thread `run_id` through the inner service functions rather than creating one per scope.

- [ ] **Step 6: Persist the typed safe error and log run completion**

Convert `RemoteSyncDiagnostic` into `SyncSafeError` field-for-field. Unknown failures keep `http_status: None`, `operation: "synchronize"`, and the existing generic code. On success log the combined summary; on failure log the code, category, partial summary, and duration. Do not serialize provider configuration or paths.

- [ ] **Step 7: Add service propagation and redaction tests**

Use a fake backend returning a diagnostic `PUT 403` error. Assert:

- `SyncRunError::to_string()` contains `s3-upload-http-failed` but no path;
- failed status has operation `upload`, method `PUT`, status 403, provider code, request ID, run ID, and `relativePath: null`;
- partial summary is preserved;
- a plain fake error still becomes `sync-run-failed` only when its prefix is not a safe code.

- [ ] **Step 8: Run service and status tests**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync::service -- --nocapture
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml sync_config::status -- --nocapture
```

Expected: all selected tests pass.

- [ ] **Step 9: Commit propagation and status**

```bash
git add apps/desktop/src-tauri/src/remote_sync/service.rs apps/desktop/src-tauri/src/sync_config/status.rs
git commit -m "feat(sync): preserve S3 failure diagnostics"
```

---

### Task 5: Surface Safe Diagnostics in Runtime and Copied Logs

**Files:**
- Modify: `packages/app/src/lib/sync-config.ts`
- Modify: `packages/app/src/hooks/useAppSyncCoordinator.ts`
- Modify: `packages/app/src/hooks/useAppSyncCoordinator.test.tsx`
- Modify: `apps/desktop/src/runtime/tauri/sync-config/shared.ts`
- Modify: `apps/desktop/src/runtime/tauri/sync-config.test.ts`
- Modify: `packages/app/src/lib/runtime-log.test.ts`

**Interfaces:**
- Consumes: Task 4 expanded `SyncSafeError` JSON.
- Produces: a compact `Application synchronization completed` `appLogger` event and a richer safe failure description.

- [ ] **Step 1: Write failing TypeScript schema and logging tests**

Extend test fixtures with:

```ts
const s3Failure: SyncSafeError = {
  category: "http",
  code: "s3-upload-http-failed",
  httpStatus: 403,
  method: "PUT",
  objectId: "object-a1",
  operation: "upload",
  provider: "s3",
  providerErrorCode: "AccessDenied",
  relativePath: null,
  requestId: "request-403",
  runId: "run-1"
};
```

Assert the failure description is:

```text
s3-upload-http-failed · upload · PUT · HTTP 403 · AccessDenied · request request-403
```

In `sync-config.test.ts`, mock a successful result and assert `appLogger.info` receives only the summary fields, provider, trigger, and revision—not notesRoot or notebookName.

- [ ] **Step 2: Run focused TypeScript tests and verify they fail**

Run:

```bash
pnpm test -- packages/app/src/hooks/useAppSyncCoordinator.test.tsx apps/desktop/src/runtime/tauri/sync-config.test.ts packages/app/src/lib/runtime-log.test.ts
```

Expected: type or assertion failures because the new fields and summary log do not exist.

- [ ] **Step 3: Expand the TypeScript safe error type**

Use exact nullable fields matching Rust:

```ts
export type SyncSafeError = {
  category: "http" | "integrity" | "local" | "transport" | null;
  code: string;
  httpStatus: number | null;
  method: string | null;
  objectId: string | null;
  operation: string;
  provider: SyncProvider;
  providerErrorCode: string | null;
  relativePath: string | null;
  requestId: string | null;
  runId: string | null;
};
```

Update fallback errors to set all new fields to `null`.

- [ ] **Step 4: Render a safe enriched failure description**

Change `safeErrorDescription` to join only the allowlisted fields in this order: code, operation, method, HTTP status, provider error code, request ID. Do not include object ID by default in toast text; it remains available in copied logs and status.

- [ ] **Step 5: Log successful summaries through `appLogger`**

After `invokeNative<SyncRunResult>` resolves in `sync-config/shared.ts`, call:

```ts
appLogger.info("sync", "Application synchronization completed", {
  bytesDownloaded: result.summary.bytesDownloaded,
  bytesUploaded: result.summary.bytesUploaded,
  conflictFiles: result.summary.conflictFiles,
  downloadedFiles: result.summary.downloadedFiles,
  provider: result.provider,
  revision: result.revision,
  scannedFiles: result.summary.scannedFiles,
  skippedFiles: result.summary.skippedFiles,
  trigger: result.trigger,
  uploadedFiles: result.summary.uploadedFiles
});
```

Do not include `notesRoot` or `notebookName` in the log details.

- [ ] **Step 6: Prove copied logs stay redacted**

Extend `runtime-log.test.ts` with sentinel endpoint, bucket, credential, absolute path, Unicode filename, and authorization values. Assert formatted copied logs contain the safe S3 code/status/request ID and none of the sentinels.

- [ ] **Step 7: Run focused frontend tests**

Run:

```bash
pnpm test -- packages/app/src/hooks/useAppSyncCoordinator.test.tsx apps/desktop/src/runtime/tauri/sync-config.test.ts packages/app/src/lib/runtime-log.test.ts
```

Expected: all selected tests pass.

- [ ] **Step 8: Commit frontend surfacing**

```bash
git add packages/app/src/lib/sync-config.ts apps/desktop/src/runtime/tauri/sync-config/shared.ts apps/desktop/src/runtime/tauri/sync-config.test.ts packages/app/src/lib/runtime-log.test.ts
git add -p packages/app/src/hooks/useAppSyncCoordinator.ts packages/app/src/hooks/useAppSyncCoordinator.test.tsx
git diff --cached --check
git diff --cached
git commit -m "feat(sync): surface safe S3 diagnostics"
```

---

### Task 6: Run Full Verification and Runtime Acceptance

**Files:**
- Modify only if verification exposes a task-scoped defect in files already listed above.

**Interfaces:**
- Consumes: all previous tasks.
- Produces: verified repository state and runtime evidence.

- [ ] **Step 1: Run formatting and focused lint-like gates**

Run:

```bash
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
pnpm typecheck:test
```

Expected: both commands exit 0.

- [ ] **Step 2: Run the full Rust test suite**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
```

Expected: all Rust tests pass under default parallel execution.

- [ ] **Step 3: Run frontend tests and production build**

Run:

```bash
pnpm test
pnpm build
```

Expected: tests and build exit 0.

- [ ] **Step 4: Run live S3 coverage when the configured server is available**

Run:

```bash
pnpm test:s3-sync:live
```

Expected: live S3 synchronization tests pass. If credentials or the live server are unavailable, record the exact prerequisite failure and continue with the deterministic fixture evidence.

- [ ] **Step 5: Verify the packaged runtime log**

Launch the real desktop app with `pnpm tauri dev`, configure the existing safe test S3 target, and perform:

1. a successful Test Connection;
2. a successful no-change synchronization;
3. a file save that succeeds with write permission;
4. a file save under a policy that denies `PutObject`.

Inspect `QingYu.log` and the Settings log. The denied write must show `s3-upload-http-failed`, `PUT`, `403`, `AccessDenied`, and the server request ID. Search both exports for access key, secret, endpoint, bucket, remote root, local root, and filename sentinels; every search must return no match.

- [ ] **Step 6: Correlate the server trace**

Run MinIO trace during the denied upload and match its request ID and UTC timestamp to `QingYu.log`. Do not capture verbose headers or bodies.

- [ ] **Step 7: Commit any verification-only task fix**

If verification required a scoped fix, stage only those task files and commit:

```bash
git commit -m "fix(sync): finalize S3 diagnostic logging"
```

If verification required no code change, do not create an empty commit.
