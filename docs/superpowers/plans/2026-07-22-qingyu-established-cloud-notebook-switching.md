# QingYu Established Cloud Notebook Switching Implementation Plan

> **For Codex:** REQUIRED SUB-SKILL: Use subagent-driven-development to execute this plan task-by-task. Every production change must first be justified by an observed failing focused test, and every task must receive a fresh code-quality/spec review before the next task begins.

**Goal:** Let a configured desktop user open the existing cloud notebook catalog from Synchronization settings and switch from current `Workspace/A` to cloud notebook `B`, creating or reusing `Workspace/B` and preserving the application-global sync configuration.

**Architecture:** Persist an explicit device-local Workspace root beside the current desktop notebook in primary-workspace state version 3, updating the TypeScript and Rust state contract atomically. The settings window only ends its sync-editing lease and asks the primary window to open the catalog; the primary window remains the sole owner of catalog state, secure target preparation, remote-first bootstrap, and current-notebook publication. Established restores pass the persisted Workspace root into the existing mutually exclusive switch transaction, while first-use restores keep the one-time Workspace picker.

**Tech Stack:** React, TypeScript, Vitest/Testing Library, Tauri v2, Rust, pnpm.

---

## Task 1: Upgrade the complete primary-workspace contract to version 3

**Files:**

- Modify: `packages/app/src/lib/settings/local-state.ts`
- Modify: `packages/app/src/lib/settings/local-state.test.ts`
- Modify: `packages/app/src/hooks/usePrimaryWorkspace.ts`
- Modify: `packages/app/src/hooks/usePrimaryWorkspace.test.tsx`
- Modify: `apps/desktop/src-tauri/src/primary_workspace.rs`
- Modify: TypeScript and Rust primary-state fixtures reported by focused tests and typechecking

**Step 1: Write failing TypeScript and Rust tests**

Prove these legal version-3 forms and no others:

- configured desktop: non-null, distinct `desktopWorkspaceRoot` plus direct-child `desktopPath`, null `managedName`;
- configured mobile: null desktop fields plus valid `managedName`;
- onboarding/deferred: all three identity fields null;
- version 2, a one-sided desktop pair, mixed desktop/mobile identity, a filesystem root used as both Workspace and notebook (`/` or `C:\`), and a desktop notebook outside the Workspace all fail closed.

Also prove:

- `commitDesktopRoot("/Workspace/A")` resolves the notebook, derives and resolves its parent, atomically stores both canonical paths, and exposes `workspaceRoot`;
- reload canonicalizes both paths through the existing TypeScript compare-and-swap convergence loop;
- Rust parses the same v3 contract, canonicalizes both paths for authorization, requires an exact direct-parent relationship, and retains mobile/deferred authority behavior.

Run and observe RED:

```bash
pnpm --filter @markra/desktop exec vitest --root ../../packages/app run src/lib/settings/local-state.test.ts src/hooks/usePrimaryWorkspace.test.tsx
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml primary_workspace -- --nocapture
```

Expected: FAIL because version 3, `desktopWorkspaceRoot`, and `workspaceRoot` do not exist.

**Step 2: Implement the atomic cross-language contract**

- Extend `PrimaryWorkspaceState` and Rust `StoredPrimaryWorkspaceState` with the Workspace root and require version 3. Do not read or migrate version 2.
- Add `workspaceRoot: string | null` to `PrimaryWorkspaceController`.
- Keep canonical state rewriting and stale-state CAS convergence in `usePrimaryWorkspace`; do not duplicate that state machine in Rust.
- Make Rust authoritative-root validation canonicalize both paths at read time and require the notebook's canonical parent to equal the canonical Workspace root.
- Make desktop commits save canonical root/current together; make mobile and deferred states explicitly save null desktop fields. Preserve both fields together when reset only requests onboarding for the next launch.
- Update fixtures deliberately. Do not change the top-level local-state schema version 1 or add persisted directory identity.

**Step 3: Run focused tests, typecheck, and the Rust suite**

```bash
pnpm --filter @markra/desktop exec vitest --root ../../packages/app run src/lib/settings/local-state.test.ts src/hooks/usePrimaryWorkspace.test.tsx
pnpm typecheck:test
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml primary_workspace -- --nocapture
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
```

Expected: PASS; no intermediate commit may leave TS writing v3 while Rust accepts only v2.

**Step 4: Commit**

Commit all and only version-3 contract, controller, Rust authority, and required fixture changes:

```bash
git commit -m "feat: persist the desktop workspace root"
```

## Task 2: Reuse the persisted Workspace for established remote restores

**Files:**

- Modify: `packages/app/src/hooks/useNotebookSwitchCoordinator.ts`
- Modify: `packages/app/src/hooks/useNotebookSwitchCoordinator.test.tsx`
- Modify: `packages/app/src/App.tsx`
- Modify: `packages/app/src/App.test.tsx`

**Step 1: Write failing transaction tests**

Prove:

- restoring cloud `B` while current is `/Workspace/A` prepares `{ parentPath: "/Workspace", notebookName: "B" }` and never opens a folder picker;
- first-use restore still opens one Workspace picker;
- existing `/Workspace/B` is reused through the existing prepared lease;
- preparation/bootstrap failure leaves A current and does not publish B;
- a concurrent local or remote switch is rejected safely and retryably, rather than queued, merged, or handled by a duplicate transaction;
- configured revision validation remains and global sync configuration is not rewritten.

Run and observe RED:

```bash
pnpm --filter @markra/desktop exec vitest --root ../../packages/app run src/hooks/useNotebookSwitchCoordinator.test.tsx src/App.test.tsx
```

**Step 2: Extend only the existing restore transaction**

- Allow `restoreDesktopNotebook` to receive an optional trusted `parentPath`.
- Use `primaryWorkspace.workspaceRoot` for established restores; use the picker only when no Workspace root exists during onboarding.
- Keep validation, mutual exclusion, prepared-lease disposal, bootstrap revision checks, sync barrier release, and `commitDesktopRoot` in the current `runTransaction` path.
- Do not extend the local-path request pump or add another synchronization/catalog service.

**Step 3: Run focused tests and commit**

```bash
pnpm --filter @markra/desktop exec vitest --root ../../packages/app run src/hooks/useNotebookSwitchCoordinator.test.tsx src/App.test.tsx
git commit -m "feat: restore cloud notebooks into the workspace"
```

## Task 3: Add one credential-free primary-window catalog request contract

**Files:**

- Add: `packages/app/src/lib/cloud-notebook-catalog-events.ts`
- Add: `packages/app/src/lib/cloud-notebook-catalog-events.test.ts`
- Modify: `packages/app/src/runtime/index.ts`
- Modify: `packages/app/src/runtime/index.test.ts`
- Modify: `apps/desktop/src/runtime/tauri/window.ts`
- Modify: `apps/desktop/src/runtime/tauri/window.test.ts`
- Modify: `apps/desktop/src/runtime/desktop.ts`
- Modify: `apps/desktop/src/runtime/index.test.ts`
- Modify: `apps/desktop/src-tauri/src/desktop_runtime.rs`

**Step 1: Write failing contract tests**

Prove:

- one TS module owns the event name, request method, and validated listener; App and settings never hardcode a duplicate event string;
- `requestPrimaryCloudNotebookCatalog()` carries no path, notebook, provider, revision, or credential payload;
- the Rust command finds, shows, and focuses the primary window before emitting exactly one `qingyu://cloud-notebook-catalog-requested` unit event only to it;
- a missing or unusable primary window returns an error so the caller can keep settings visible;
- unsupported runtimes fail safely and never masquerade as a delivered request.

Run and observe RED:

```bash
pnpm --filter @markra/desktop exec vitest --root ../../packages/app run src/lib/cloud-notebook-catalog-events.test.ts src/runtime/index.test.ts
pnpm --filter @markra/desktop exec vitest run src/runtime/tauri/window.test.ts src/runtime/index.test.ts
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml desktop_runtime -- --nocapture
```

**Step 2: Implement the narrow contract**

- Centralize the event constant and generic-runtime listener in the new helper.
- Add the runtime window request method, Tauri invoke wrapper, desktop mapping, and explicit unavailable default.
- Add one Rust command that returns errors for missing/show/focus/emit failures and contains no sync or path data.

**Step 3: Run focused tests, typecheck, and commit**

```bash
pnpm --filter @markra/desktop exec vitest --root ../../packages/app run src/lib/cloud-notebook-catalog-events.test.ts src/runtime/index.test.ts
pnpm --filter @markra/desktop exec vitest run src/runtime/tauri/window.test.ts src/runtime/index.test.ts
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml desktop_runtime -- --nocapture
pnpm typecheck:test
git commit -m "feat: request the cloud catalog in the primary window"
```

## Task 4: Expose cloud notebook selection from Synchronization settings

**Files:**

- Modify: `packages/app/src/components/settings/SyncSettings.tsx`
- Modify: `packages/app/src/components/settings/SyncSettings.test.tsx`
- Modify: `packages/app/src/components/SettingsWindow.tsx`
- Modify: `packages/app/src/components/SettingsWindow.test.tsx`
- Modify: `packages/app/src/hooks/useSettingsWindowState.ts`
- Modify: `packages/app/src/hooks/useSettingsWindowState.test.ts`
- Modify: all locale dictionaries under `packages/shared/src/i18n/`

**Step 1: Write failing settings tests**

Prove:

- a loaded, fully configured S3/WebDAV document with a current desktop root shows `Select Cloud Notebook`;
- incomplete, missing-root, pending-save, testing, and synchronizing states disable it; absent/malformed/unsupported configuration does not offer it;
- `enabled: false` still permits selection when `loadResult.configured` is true—do not reuse `readiness === "ready"` as this action's eligibility test;
- invoking the action waits for `syncSession.end("category-leave")`, then requests the primary catalog, then starts the existing hide handshake;
- editing-session end or native request failure keeps settings visible and reports failure.

Run and observe RED:

```bash
pnpm --filter @markra/desktop exec vitest --root ../../packages/app run src/components/settings/SyncSettings.test.tsx src/components/SettingsWindow.test.tsx src/hooks/useSettingsWindowState.test.ts
```

**Step 2: Implement the settings action**

- Add the action row with its own configured/busy/draft guard and localized copy in every supported locale.
- Expose one settings-state handler that ends the lease before requesting; only call the normal settings hide path after delivery succeeds.
- Do not list notebooks or restore a notebook inside the settings window.

**Step 3: Run focused tests, typecheck, and commit**

```bash
pnpm --filter @markra/desktop exec vitest --root ../../packages/app run src/components/settings/SyncSettings.test.tsx src/components/SettingsWindow.test.tsx src/hooks/useSettingsWindowState.test.ts
pnpm typecheck:test
git commit -m "feat: open cloud notebooks from sync settings"
```

## Task 5: Reuse the catalog dialog in the established primary window

**Files:**

- Modify: `packages/app/src/components/notebooks/RemoteNotebookDialog.tsx`
- Modify: `packages/app/src/components/notebooks/RemoteNotebookDialog.test.tsx`
- Modify: `packages/app/src/App.tsx`
- Modify: `packages/app/src/App.test.tsx`

**Step 1: Write failing primary-window UI tests**

Prove:

- only the primary non-mobile window subscribes to the catalog request and opens the existing remote catalog after onboarding;
- the dialog has one shared desktop mount used by onboarding and the established editor, with no duplicate dialog state or list implementation;
- stale/closed catalog requests and revision changes remain ignored;
- the current notebook A is visibly marked/non-restorable, while B can be selected and invokes established restore with the persisted Workspace root;
- mobile and standalone-file behavior remain unchanged.

Run and observe RED:

```bash
pnpm --filter @markra/desktop exec vitest --root ../../packages/app run src/components/notebooks/RemoteNotebookDialog.test.tsx src/App.test.tsx
```

**Step 2: Reuse the existing primary-window flow**

- Register the centralized listener only when `primaryWindowOwner && !trueMobile`.
- Hoist the existing `RemoteNotebookDialog` overlay so onboarding and established desktop branches render the same instance.
- Preserve request-generation and sync-revision guards.
- Pass the current directory name to the dialog and disable restoring that entry; allow B to use Task 2's transaction.

**Step 3: Run focused tests, typecheck, and commit**

```bash
pnpm --filter @markra/desktop exec vitest --root ../../packages/app run src/components/notebooks/RemoteNotebookDialog.test.tsx src/App.test.tsx
pnpm typecheck:test
git commit -m "feat: switch cloud notebooks from the primary window"
```

## Task 6: Verify the complete desktop A-to-B workflow

**Files:**

- Update `scripts/test-s3-sync-live.mjs` and its tests only if existing live coverage cannot express the A-to-B scenario
- Update ignored `.superpowers/sdd/progress.md` with secret-free evidence

**Step 1: Run repository gates**

```bash
pnpm test
pnpm typecheck:test
pnpm build
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
```

**Step 2: Run real S3 coverage**

Use the supplied local MinIO endpoint and credentials only through process environment variables; never print, persist, stage, or commit them.

```bash
pnpm test:s3-sync:live
```

**Step 3: Run the actual desktop app with isolated app data**

- Back up the real QingYu app-data directory and restore it afterward.
- Configure isolated `Workspace/A`, verify A is current/synchronized, and seed or confirm remote B.
- Open Settings → Synchronization → Select Cloud Notebook, choose B, and verify QingYu creates or reuses `Workspace/B` without a parent picker.
- Verify B is remote-first synchronized and becomes current only after success; A stops receiving note-sync triggers and unrelated cloud notebooks are not downloaded.
- Restart and verify B restores as current, the Workspace root is unchanged, A remains intact, and the global sync-config file hash is unchanged.
- Open an external Markdown file and verify it remains outside note synchronization.

**Step 4: Request final whole-branch review**

Use the requesting-code-review skill with the design, plan, Task 1 base, final commit, RED/GREEN evidence, repository gates, and runtime evidence. Resolve substantive findings and rerun affected checks.

**Step 5: Record final evidence**

Append actual commands, pass counts, runtime paths, backup/restore result, and review disposition to `.superpowers/sdd/progress.md`. Do not include secrets.
