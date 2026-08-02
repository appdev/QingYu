# Android Fatal Sync Run Recovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Use superpowers:test-driven-development for every behavior change and superpowers:verification-before-completion before reporting success.

**Goal:** Make app-launch sync runs reach a bounded terminal state and make compact repository Join atomic, safely diagnosable, and non-duplicating across Android, Web, and macOS.

**Architecture:** Add a launch-only deadline around the shared kernel executor. Refactor shared sync-run startup into reservation and spawning phases so repository recovery can reserve admission before committing its binding. Preserve a whitelisted safe admission error in the shared compact React surface.

**Tech Stack:** Rust/Tokio shared kernel, TypeScript/React shared app, Vitest, Cargo tests, pnpm workspace.

## Global Constraints

- Work only on `codex/fix-android-sync-run-admission-aef73f8e` based on commit `aef73f8e346af1fa00d3c3a92771ced7a0e2cc59` / tree `d4dd8d320efd6086fb125ed9c30698d499010602`.
- Do not modify canonical `main`, push, install or launch macOS/Android applications, deploy Docker, or write live profile/S3 state.
- Use local fixtures and temporary directories only.
- Run focused tests during RED/GREEN iteration, then run the complete required verification once after stabilization.

---

## Task 1: Bound app-launch runs

**Files:**

- Modify: `apps/kernel/tests/sync_config_service.rs`
- Modify: `apps/kernel/src/services/sync.rs`

- [ ] Add a paused-time integration test using `BlockingExecutor`. Accept an app-launch run, assert `Attempting` before five minutes, advance to the deadline, await settlement, and assert global/per-run terminal failure, cleared active run, and safe timeout fields.
- [ ] Run only that exact test and record the expected RED timeout/failure.
- [ ] Add an `APP_LAUNCH_RUN_TIMEOUT` constant and wrap only the app-launch executor future with `tokio::time::timeout`.
- [ ] Convert elapsed timeout into the existing terminal `SyncSafeErrorDto` pipeline with operation `SyncRun`, code `RequestFailed`, category `Network`, provider error `RequestTimeout`, and exact run ID.
- [ ] Re-run the exact test and nearby run-lifecycle tests to GREEN.

## Task 2: Make repository recovery admission atomic

**Files:**

- Modify: `apps/kernel/tests/sync_config_service.rs`
- Modify: `apps/kernel/src/services/sync.rs`

- [ ] Add a local one-response HTTP catalog fixture for correct-revision repository bind tests; it must write only to temporary local state.
- [ ] Add a RED test proving closed recovery admission leaves `local-sync.json` byte-for-byte unchanged.
- [ ] Add a RED test proving a spawn failure after a successful target bind returns one accepted recovery job whose exact status is terminal failed and whose local state contains one target binding.
- [ ] Extract a queued/reserved run value and split run startup into reserve and spawn/finalize operations.
- [ ] In repository bind, reserve recovery admission under the mutation gate before calling `bind_dejavu_repository`.
- [ ] If binding fails, terminalize/release the reserved run before returning the binding error. If spawning fails after binding, return the accepted job and let the client observe its exact terminal status.
- [ ] Re-run the exact bind tests and the full `sync_config_service` integration target to GREEN.

## Task 3: Preserve compact Join error semantics

**Files:**

- Modify: `packages/app/src/components/compact/CompactRepositoryAccess.test.tsx`
- Modify: `packages/app/src/components/compact/CompactRepositoryAccess.tsx`
- Modify: relevant locale resources under `packages/shared/src/i18n/locales/`

- [ ] Add a RED shared-component test where bind rejects with `{ code: "sync_run_unavailable", message: "sensitive" }`; assert a localized active-run instruction, safe code display, no sensitive message, and exactly one bind call without automatic retry.
- [ ] Add a narrow error-code extractor that accepts only `sync_run_unavailable`; keep unknown errors generic.
- [ ] Add localized active/recovering-run copy for supported locales and render it from a distinct bind state.
- [ ] Re-run the exact compact component test to GREEN.

## Task 4: Stabilize and verify shared contracts

**Files:**

- Modify only if focused regression exposes a defect.

- [ ] Run formatting and the focused kernel/app tests after implementation stabilizes.
- [ ] Run `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml` with the installed Rust toolchain binary on `PATH`.
- [ ] Run `pnpm test`, `pnpm typecheck:test`, and `pnpm build` once each.
- [ ] If packaging-specific code changed, run an offline APK build without installing it; otherwise record why it is unnecessary.
- [ ] Inspect `git diff --check`, changed files, and repository status; ensure no generated artifacts or lockfile drift are included.

## Task 5: Commit, independent review, and handoff

**Files:**

- Review all changed production, test, localization, and design/plan files.

- [ ] Commit the verified change on the isolated branch and record exact commit/tree.
- [ ] Dispatch an independent reviewer with the fatal-symptom contract, baseline, diff, and verification evidence.
- [ ] Address findings with focused RED/GREEN tests, then repeat affected verification and amend or add a follow-up commit as appropriate.
- [ ] Send the parent task `019fbd10-0e79-7f21-9b63-b38a70acee9e` an active callback containing root cause, exact branch/commit/tree, changed files, RED/GREEN evidence, full verification, review result, residual risks, and the requirement to rebuild all three clients from one unified commit.
