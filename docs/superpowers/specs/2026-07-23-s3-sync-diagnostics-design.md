# QingYu S3 Sync Diagnostics Design

## Status

Approved for implementation on 2026-07-23. The user approved the recommended structured-logging approach and authorized subsequent recommended decisions without additional confirmation pauses.

## Problem

QingYu currently writes frontend application events and native library output into the rotating desktop log file, but S3 synchronization failures lose the provider-level reason before they reach the application logger.

The observed failure sequence demonstrates the gap:

- the signed S3 connection test completed successfully;
- several no-change synchronization runs completed successfully;
- creating and saving new Markdown files caused subsequent synchronization runs to fail;
- the log retained only `sync-run-failed: Application synchronization did not complete.`;
- the log did not identify the failed S3 operation, HTTP status, provider error code, or request ID.

The S3 backend currently formats detailed errors such as `S3 sync upload failed: PUT ...: HTTP 403`. The service then accepts only lowercase slug-like prefixes as safe error codes. Because the S3 message starts with uppercase prose, the service replaces it with `sync-run-failed`. The persisted status repeats that generic code.

This prevents a user or developer from distinguishing upload authorization failures, endpoint and region errors, transport failures, remote-object races, and server-side failures from a single exported QingYu log.

## Goals

- Keep `QingYu.log` as the authoritative rotating diagnostic file.
- Record a concise synchronization start and completion summary by default.
- Record structured, sanitized details at the S3 request boundary whenever an operation fails.
- Preserve safe S3 failure information through `sync_application`, persisted sync status, the in-app runtime log, and copied logs.
- Allow detailed successful S3 request logging only in an explicit diagnostic mode.
- Correlate QingYu failures with MinIO or AWS-compatible server traces through request IDs and timestamps.
- Ensure logging never changes synchronization behavior or turns a successful operation into a failure.

## Non-goals

- Do not create a separate S3 log file.
- Do not persist raw HTTP requests, response bodies, credentials, signatures, endpoints, bucket names, local absolute paths, or object keys.
- Do not make the in-app runtime log a complete mirror of native TRACE output.
- Do not enable verbose reqwest or hyper wire logging in normal releases.
- Do not add remote telemetry, log upload, or automatic external reporting.
- Do not redesign synchronization, retry policy, conflict resolution, or provider configuration.

## Alternatives Considered

### Log only at `sync_application` — rejected

This is the smallest change, but the provider error has already been flattened at that boundary. It cannot reliably recover the S3 operation, response metadata, or transport category.

### Structured logging at the S3 adapter boundary — selected

The S3 adapter is the last boundary that simultaneously knows the logical operation, HTTP method, response status, provider headers, elapsed time, and safe object identity. It can create a structured diagnostic before returning a stable error to the service.

### Enable full reqwest and hyper tracing — rejected as the default

Wire logs are noisy and may expose URLs, headers, object names, and signature material. They also make ordinary exported logs harder to inspect. Low-level traces may remain available during development, but they are not the product diagnostic contract.

## Log Destinations

QingYu retains two deliberately different views of logging.

### Authoritative rotating file

The desktop `tauri-plugin-log` target remains the authoritative file sink. Frontend `appLogger` events and native Rust log records continue to land in the same `QingYu.log` file. Existing rotation remains unchanged: one active file plus retained archives under the current size and file-count limits.

### In-app runtime log

The Settings > Logs panel remains a compact application-level view stored in local browser state. It displays synchronization lifecycle summaries and safe S3 failures, but it does not ingest all reqwest, hyper, runtime, or per-request success traces.

The Copy Logs action therefore includes the information required for normal S3 failure triage. Open Log Folder remains the path to the complete native diagnostic file.

## Synchronization Correlation

Every application synchronization run receives a diagnostic `runId` before provider work starts. The identifier contains no provider, path, credential, or user data. It uses the process start timestamp, current Unix timestamp in milliseconds, and a process-local atomic sequence; this is unique inside one log-producing process without adding a dependency. The same identifier is attached to:

- synchronization start and completion records;
- note-scope and settings-scope S3 failures;
- the returned safe error;
- persisted sync status when a run fails.

## Default Lifecycle Records

Normal logging emits one start record and one terminal record per run.

Start fields:

- `runId`;
- `provider`;
- `trigger`;
- `bootstrap` when applicable.

Successful completion fields:

- `runId`;
- `durationMs`;
- `scannedFiles`;
- `uploadedFiles`;
- `downloadedFiles`;
- `deletedFiles`;
- `skippedFiles`;
- `conflicts` when present;
- byte totals already available in the synchronization summary.

Failed completion fields:

- `runId`;
- `durationMs`;
- stable safe error code;
- error category;
- partial summary.

Lifecycle records use the existing `sync` log area. Start and successful completion use `INFO`; a terminal failure uses `ERROR`.

## S3 Request Diagnostics

The S3 adapter classifies operations using a stable vocabulary:

| Operation | HTTP method | Purpose |
| --- | --- | --- |
| `list` | `GET` | List objects for a synchronized prefix |
| `catalog` | `GET` | List notebook prefixes or test the target |
| `metadata` | `HEAD` | Read or verify object identity |
| `download` | `GET` | Download an object |
| `upload` | `PUT` | Upload an object |
| `delete` | `DELETE` | Delete an object |

Every failed request emits exactly one S3 request-failure record containing only applicable fields:

- `runId`;
- `scope`: `notes`, `settings`, or `catalog`;
- `operation`;
- `method`;
- `category`;
- `httpStatus`;
- `s3ErrorCode`;
- `requestId`;
- `objectId`;
- `durationMs`.

`objectId` is an opaque, diagnostic-only identifier derived from the run context, scope, and validated relative path. It must not contain the original path and must not be usable as an object key. It only needs to correlate records within the same run; the server request ID is the cross-system correlation key.

The S3 adapter does not log a request-start entry in the default mode. This keeps ordinary successful synchronization compact.

## Failure Categories and Safe Codes

Provider failures use four categories.

### Transport

Transport failures are classified without retaining the raw URL-bearing reqwest message:

- `connect`;
- `timeout`;
- `tls` when reliably identifiable;
- `request` for another send failure;
- `response-body` for failure while reading an expected response.

Stable examples include:

- `s3-list-transport-failed`;
- `s3-upload-transport-failed`;
- `s3-download-body-failed`.

### HTTP

Non-success responses preserve the HTTP status and, when safely parseable, the S3 XML error `Code` and `x-amz-request-id` response header.

Stable examples include:

- `s3-list-http-failed`;
- `s3-metadata-http-failed`;
- `s3-upload-http-failed`;
- `s3-delete-http-failed`.

Existing operation-specific semantics remain unchanged. For example, a metadata `HEAD 404` means the object is absent and is not logged as a failure, and a delete `404` remains successful.

### Integrity

Remote consistency failures preserve a stable safe code:

- `s3-object-changed` when the expected identity no longer matches;
- `s3-upload-verification-failed` when a completed upload cannot be verified;
- a stable parse code for malformed list responses.

### Local

Local file, manifest, settings-publication, state-directory, and status-persistence failures remain synchronization errors rather than S3 HTTP errors. They receive their own stable lowercase codes and lifecycle failure records without an S3 request record.

## Bounded S3 Error Parsing

For a non-success S3 response, QingYu may parse only the provider error code from a bounded XML error document.

- Read the response incrementally with a hard maximum of 64 KiB.
- Stop and omit `s3ErrorCode` if the body exceeds the limit or cannot be parsed safely.
- Accept only a short provider code containing ASCII letters, digits, hyphens, underscores, or periods.
- Never log the raw XML, message text, resource path, host ID, headers, or body.
- Accept `x-amz-request-id` only when it is a bounded printable ASCII value.
- A parsing failure must not replace the original HTTP failure or alter retry behavior.

The diagnostic parser is isolated from request execution so it can be unit tested independently.

## Error Propagation and Sync Status

The provider returns a typed internal diagnostic error rather than prose that must later be reparsed. The error retains:

- stable public code;
- category;
- operation and method;
- optional HTTP status;
- optional provider error code;
- optional request ID;
- optional opaque object ID;
- a safe user-facing summary.

`run_application_sync` converts this structure into `SyncRunError` without calling the generic fallback for known provider errors. The failure status stores the same safe fields. Raw paths and provider configuration remain absent.

The Tauri command error includes the stable code and safe summary, allowing the existing frontend command logger to place useful failure information in both the Settings log panel and `QingYu.log`. The status response provides structured fields for UI presentation without parsing prose.

Unknown internal errors continue to fail closed as `sync-run-failed`. The generic fallback is retained for genuinely unclassified errors, not normal S3 failures.

## Detailed Diagnostic Mode

The first implementation does not add a new end-user settings toggle. Detailed successful S3 request records are enabled only when the native process starts with `QINGYU_SYNC_DIAGNOSTICS=1`. Any other value, an absent variable, or a non-Unicode value leaves detailed mode disabled. The process reads this switch once and does not expose it through application settings.

When enabled, each completed request emits one native `DEBUG` record containing:

- `runId`;
- `scope`;
- `operation` and method;
- HTTP status;
- request ID;
- opaque object ID;
- duration.

Detailed mode follows the same redaction rules. There is no mode that logs secrets, headers, raw URLs, bucket names, object keys, or bodies.

## Privacy and Redaction Contract

The following values are forbidden in every new synchronization diagnostic:

- S3 access key and secret access key;
- Authorization, credential, signature, security-token, cookie, and proxy-auth values;
- complete endpoint URLs;
- bucket and region configuration values;
- remote-root and raw object keys;
- local absolute paths and raw relative file paths;
- request or response bodies;
- complete synchronization configuration or command arguments.

The following values are allowed:

- provider name;
- operation and HTTP method;
- numeric HTTP status;
- bounded S3 error code;
- bounded request ID;
- `notes`, `settings`, or `catalog` scope;
- opaque object ID;
- timestamps, durations, counters, and byte totals;
- stable QingYu error codes.

Rust-side construction enforces this allowlist before the record reaches the logging plugin. The shared TypeScript diagnostic sanitizer remains a second defense for records routed through `appLogger`.

## Logging Reliability

- Logging is best effort and never changes the synchronization result.
- A logging sink failure is ignored after the original synchronization result is preserved.
- Parsing provider diagnostics cannot panic.
- Logging must not hold a synchronization lock across file I/O performed by the sink.
- Duplicate S3 failure records for the same failed request are avoided: the adapter owns the request record and the service owns the terminal run record.
- Concurrent note and settings requests share the run ID and retain distinct scopes.

## Verification

### Unit tests

- Parse bounded S3 XML errors and extract only the safe `Code`.
- Reject oversized, malformed, non-UTF-8, or unsafe provider codes without losing the HTTP status.
- Accept a bounded safe request ID and reject unsafe or oversized values.
- Classify connect, timeout, response-body, HTTP, integrity, and local failures.
- Preserve stable codes through `SyncRunError` and serialized sync status.
- Verify `HEAD 404` and delete `404` retain their existing success semantics.
- Verify object identifiers never contain the original path.

### Redaction tests

Construct failures containing sentinel secrets, endpoints, bucket names, regions, absolute paths, Unicode filenames, object keys, Authorization headers, and response bodies. Assert that none appear in:

- the native formatted record;
- the Tauri command error;
- serialized sync status;
- the frontend runtime log entry;
- copied log text.

The expected record must still contain method, operation, category, HTTP status, safe provider code, request ID, scope, object ID, and duration.

### Integration tests

Use the existing S3 fixture boundaries to cover:

- `PUT 403 AccessDenied`;
- `GET 500` and `503`;
- transport timeout;
- successful list followed by failed upload;
- upload success followed by failed verification;
- concurrent note and settings failures with one run ID;
- a successful no-change synchronization that emits only lifecycle summaries by default.

### Runtime verification

Against the configured real MinIO-compatible test server:

1. Start with valid read and write permissions.
2. Run Test Connection and verify the safe catalog result.
3. Create and edit a Markdown file, then verify one successful synchronization summary.
4. Temporarily use a policy that permits listing but denies `PutObject`.
5. Save another file and verify the Settings log and `QingYu.log` report `PUT`, `403`, `AccessDenied`, and a request ID without exposing protected values.
6. Correlate the request ID and UTC timestamp with `mc admin trace`.
7. Restore the valid policy and verify synchronization succeeds.

## Acceptance Criteria

- Exported QingYu logs distinguish S3 upload authorization failures from generic synchronization failures.
- All normal S3 failure paths retain a stable safe code instead of collapsing to `sync-run-failed`.
- The Settings log contains actionable high-level failure information without native trace noise.
- `QingYu.log` contains lifecycle summaries, S3 failure records, and optional detailed diagnostics in one rotating file.
- No prohibited credential, target, path, header, or body data appears in any tested log or status surface.
- MinIO or AWS-compatible server operators can correlate a failed QingYu request using UTC time and request ID.
- Successful default logging remains compact and existing rotation limits remain unchanged.
