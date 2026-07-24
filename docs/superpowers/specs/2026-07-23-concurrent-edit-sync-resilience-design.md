# Concurrent Edit Sync Resilience Design

## Status

Approved by the user on 2026-07-23 after comparing QingYu with the current
Remotely Save, Obsidian S3 Sync + Backup, and rclone bisync architectures.

## Problem

QingYu already keeps a per-file three-way manifest, serializes sync execution,
checks remote identities, stages downloads durably, and preserves real conflicts.
The remaining failure path is temporal: the engine hashes a local snapshot, then
treats a content change before the planned action as a fatal string error. A save
trigger arriving during that run attaches to the existing frontend promise, so
it does not guarantee a follow-up pass. The S3 backend also turns the first
transport failure or transient HTTP response into the final run failure.

This can produce a visible failure even though a later pass succeeds or would
succeed without user action.

## Goals

- Allow notes to remain editable during synchronization.
- Preserve the existing fail-closed path and symlink protections.
- Re-plan concurrent local or remote changes from a fresh snapshot.
- Coalesce saves during a run into one guaranteed follow-up pass.
- Retry transient S3 request failures with bounded exponential backoff.
- Keep intermediate retries and deferred paths in diagnostics while deriving UI
  state only from the final run outcome.
- Preserve per-file manifest checkpointing and non-destructive conflict copies.

## Non-goals

- Do not add a continuously running native filesystem watcher.
- Do not replace the current manifest format or remote S3 object layout.
- Do not merge Markdown content automatically in this change.
- Do not retry authentication, authorization, validation, integrity, or unsafe
  local-path failures.
- Do not change WebDAV transport policy beyond benefiting from engine re-plans.

## Architecture

### Snapshot passes

`execute_remote_sync_locked` becomes a bounded pass coordinator. Each pass
captures local hashes and remote identities, builds the existing three-way plan,
and checkpoints each completed action exactly as today.

An action that observes a normal regular-file content replacement, a changed
hash, a newly created or deleted local path, or the typed remote
`s3-object-changed` identity race is deferred instead of failing the entire run.
Unsafe types, symlinks, unsafe ancestors, I/O failures, malformed state, and
provider integrity failures remain fatal.

The pass records the local content state it expects after its own downloads,
deletes, and conflict-copy publications. It compares that expected state with a
fresh local scan at the end. A mismatch or deferred path requests another pass.
At most three immediate passes run under the existing global execution lock.
Multiple passes reuse the per-file manifest as the trusted baseline and aggregate
actual transfer counters without inflating the logical scanned-file count.

If the snapshot still cannot stabilize after the third pass, the engine returns
the last typed concurrent-change outcome and leaves the per-file checkpoint in
its recoverable state. This bounds one run without claiming that continuously
changing data was synchronized. Ordinary saves are expected to stabilize in the
next pass, while the frontend pending-save bit guarantees a fresh run for edits
that arrived through the application.

### Save coalescing

The frontend shared-run record gains a pending-save bit. A save request for the
same root and revision while a native run is active sets that bit and joins the
same caller promise. After a successful native pass, the shared runner consumes
the bit and performs one fresh `save` run before resolving callers. Additional
saves during that follow-up are coalesced into a separate shared run queued
behind the current work. This keeps each shared execution bounded while still
guaranteeing that later dirtiness is not dropped. The joining save also retains
its own eligibility check, so settings editing can suppress the automatic tail
without invalidating a successful manual run.

Manual and interval callers keep their current deduplication behavior. A failed
native run is not automatically repeated by this mechanism; its final failure is
handled by the existing retryable toast.

### S3 retries

The S3 backend owns reusable signed-request attempt loops. Every attempt creates
fresh SigV4 headers. Core S3 list, metadata, download, upload, and delete
requests use up to three attempts for both request sends and successful GET
response-body reads when they fail because of:

- request transport failures;
- HTTP 408 and 429;
- HTTP 500, 502, 503, and 504.

Production delays use bounded exponential backoff with small jitter. Tests use a
zero delay. Non-retryable HTTP responses and exhausted attempts preserve the
existing typed diagnostic error. Recovered attempts emit warning diagnostics;
only the exhausted final attempt emits the existing error diagnostic, so the
application status and toast represent the final outcome.

Every PUT and DELETE also carries an atomic S3 condition on every attempt:
`If-None-Match: *` for a planned create and `If-Match` with the planned ETag for
an update or delete. HTTP 409/412 becomes a typed re-plan signal rather than a
retry of the stale mutation. If an existing-object identity has no usable ETag,
the mutation fails closed. Upload success is accepted only after the response
ETag and a fresh HEAD identity agree.

## Data safety

- A local content change never updates that path's manifest entry until a later
  pass transfers or reconciles the observed bytes.
- A remote identity change never executes a stale destructive action.
- The HEAD-to-mutation race is closed by S3 conditional writes, including every
  retry attempt.
- Regular-file replacements are treated as concurrent edits only after rejecting
  symlinks and non-files.
- Existing conflict rules continue to preserve the local file and publish the
  remote version under the timestamped `remote-conflict` name.
- The S3 retry loop sends the same immutable payload bytes for every PUT attempt.

## Verification

- Engine test: change a regular note after snapshot validation; the run re-plans
  and uploads the final bytes without returning an error.
- Engine tests: create a new note during a pass and keep changing a note across
  every pass; the former is uploaded by a fresh pass and the latter stops after
  the three-pass bound.
- Engine tests: remote identity changes before upload, ordinary download,
  conflict download, and delete; a fresh pass reconciles each case without
  executing the stale action.
- Coordinator test: a second save during an in-flight save produces exactly one
  follow-up native call and resolves both callers after it.
- Coordinator test: a failed primary run does not turn its pending save into an
  unbounded automatic retry.
- S3 test: a transient PUT response succeeds on retry and performs final HEAD
  verification.
- S3 tests: HTTP and transport-disconnect retries preserve `If-None-Match`, an
  ambiguous committed create requests a re-plan, a truncated object body is
  downloaded again, and a stale conditional delete requests a re-plan without
  retrying.
- S3 test: HTTP 403 remains a single non-retried typed failure.
- Run focused Rust and Vitest suites, then the repository test, typecheck, build,
  and diff-hygiene gates.
