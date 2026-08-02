# Android Fatal Sync Run Recovery Design

## Problem

An Android app-launch sync accepted at 2026-08-03 00:04 remained in `Attempting` after 00:13. While that run was active, joining another repository returned a generic recovery-start failure and left the original repository binding intact. The same remote catalog and repository worked on Web, so the failure is in shared run lifecycle/admission and compact-client error handling rather than Android storage permission or remote discovery.

The evidence establishes three separate contract defects:

1. `start_sync_run` awaits the executor without a run-level deadline. A future that stays pending leaves the run registered and the public status `Attempting` forever.
2. Repository binding writes `local-sync.json` before recovery-run admission and spawning. A failure after the write can report failure for a binding that was already committed.
3. `CompactRepositoryAccess` converts every bind error to one generic message, discarding the safe `sync_run_unavailable` code propagated by both desktop/mobile and web adapters.

## Chosen Design

### Bound app-launch runs

Only app-launch runs receive a fixed five-minute run-level deadline. Explicit manual, interval, and repository-recovery runs retain their existing policy. When the deadline expires, the background task produces a normal terminal failed result whose safe error identifies `sync_run`, `request_failed`, `network`, and provider `request_timeout`. Existing finalization then clears the active run, publishes global and per-run terminal status, and settles the run barrier.

The timeout wraps the executor future rather than individual S3 calls. Per-request timeouts remain useful, but cannot guarantee that the full multi-step executor reaches a terminal state.

### Admit recovery before committing the binding

Repository recovery is split into two phases:

1. validate and reserve a run slot while the sync-mutation gate is held;
2. write the repository binding, then spawn the already-reserved run.

If admission fails, `local-sync.json` is unchanged. If binding fails after reservation, the reserved run is terminalized and released before returning the binding error. If spawning fails after a successful binding, the request returns the accepted job instead of an ambiguous start failure; the per-run status is already terminal failed and can be polled by the client. This preserves one committed binding and one exact recovery job without prompting an unsafe retry.

The normal trigger path uses the same reservation/spawn primitives, preserving existing duplicate-run admission and status behavior.

### Preserve safe compact-client semantics

The compact repository UI recognizes only the fixed safe code `sync_run_unavailable` from an error-shaped value. It displays a localized instruction to wait for the active/recovering run and try again, never displays arbitrary backend messages, and never retries automatically. Unknown errors keep the existing generic recovery-start message.

Once a recovery job has been accepted, even if it is already terminal failed, the UI monitors that exact job and does not issue another bind automatically.

## Alternatives Rejected

- Cancelling the active app-launch run when Join is pressed adds cancellation and partial-write hazards and makes Join latency depend on an unhealthy executor.
- Retrying Join in the UI cannot repair an unbounded backend run and risks repeating a request whose binding outcome is unknown.
- Applying a global deadline to every trigger could abort legitimate large manual or recovery syncs; the fatal launch admission problem only requires a launch-specific bound.

## Verification Contract

- A paused-time kernel test proves a pending app-launch run becomes terminal failed at the deadline and releases admission.
- Kernel tests prove repository bind mutates nothing when recovery admission is closed, and that post-commit spawn failure returns one accepted terminal job with one target binding.
- A shared app test proves `sync_run_unavailable` is rendered safely and does not trigger an automatic duplicate bind.
- Existing kernel, app, desktop, and web suites plus repository-wide Cargo, pnpm tests, typecheck, and build verify the shared Android/Web/macOS contract.
