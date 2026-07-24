# QingYu Named Notebook Switching Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make QingYu own exactly one current notebook directory, synchronize that notebook under `<remoteRoot>/notes/<directory-name>/`, and let a new device select one remote notebook without downloading the others.

**Architecture:** Keep provider credentials and policy in the existing application-level `sync-config.json`. Add a validated notebook-name scope resolver shared by desktop, true mobile, the provider adapters, and local sync-state addressing. Route every directory-open action through one safe switch transaction; keep standalone Markdown files external and unsynchronized. Extend the existing remote-sync engine with remote-first first-run ordering and provider-specific shallow catalog listing rather than introducing a second synchronization algorithm.

**Tech Stack:** Rust, Tauri v2, serde/serde_json, reqwest, React 19, TypeScript, Vitest, cargo test, WebDAV fixtures, MinIO live tests.

## Global Constraints

- One device has exactly one current notebook and one global synchronization configuration.
- Desktop identity is the canonical selected directory basename. True-mobile identity is one validated child below app-data `workspaces/`.
- Remote notes live only below `<remoteRoot>/notes/<directory-name>/`; portable application settings remain `<remoteRoot>/app/settings.json`.
- Directory names are the only user-visible remote identity. There is no UUID, registry, binding file, hidden notebook configuration, or automatic collision prevention.
- The local state schema is version `2`; version `1` is unsupported and returns to onboarding. There is no migration or compatibility reader.
- Switching notebooks never changes sync provider, credentials, enablement, interval, save trigger, or remote root.
- A switch blocks new old-root note triggers, cancels queued old-root runs, and drains an active application-sync transaction to its atomic boundary before publishing the new primary root. It does not interrupt application-settings publication.
- Provider failure after a local switch never rolls the local notebook back.
- A cloud restore publishes the new primary root only after bootstrap succeeds. Validated partial downloads remain for retry after failure.
- First sync without a baseline downloads remote-only files and preserves remote conflict copies before uploading local-only files.
- Local manifests, staging, conflicts, coalescing, and status identities are isolated by a local-only hash of provider target, normalized remote root, notebook name, and canonical local root.
- S3 catalog uses `delimiter=/`; WebDAV catalog uses one depth-one collection listing. Neither operation recursively scans content or changes current state.
- Unsupported remote names remain visible but disabled; credentials, signed URLs, and raw provider diagnostics never enter catalog results.
- External standalone Markdown files remain supported, use adjacent lowercase `assets/`, never replace the primary notebook, and never trigger note synchronization.
- External-folder windows and folder branches in the combined file/folder picker are removed, not hidden. Recent directories remain only as local notebook-switch history owned by the central transaction.
- Existing MCP tools remain application-scoped and rooted in the current primary notebook. This change does not add remote catalog or restore tools to MCP.
- Do not add a second sync engine, provider SDK, state store, or project-local configuration.

---

## File Structure

- `apps/desktop/src-tauri/src/notebook_scope.rs`: validates notebook names, derives remote child prefixes, and computes local-only scope keys.
- `apps/desktop/src-tauri/src/managed_workspace.rs`: securely creates and resolves `workspaces/<name>/` on true mobile.
- `apps/desktop/src-tauri/src/remote_sync/catalog.rs`: exposes provider-neutral catalog entries and dispatches provider-specific shallow listing.
- `apps/desktop/src-tauri/src/remote_sync/engine.rs`: remains the only comparison, conflict, publication, and checkpoint engine.
- `apps/desktop/src-tauri/src/remote_sync/service.rs`: binds one immutable local root to one same-name remote child and one isolated state root.
- `packages/app/src/hooks/usePrimaryWorkspace.ts`: owns version-2 primary state and authoritative root publication.
- `packages/app/src/hooks/useAppSyncCoordinator.ts`: owns switch barriers and old-root run draining.
- `packages/app/src/hooks/useNotebookSwitchCoordinator.ts`: owns the user-visible local switch and cloud-restore transactions.
- `packages/app/src/components/notebooks/`: owns remote catalog and mobile notebook-selection UI without provider-specific logic.

### Task 1: Add Version-2 Primary State and Named Managed Workspaces

**Files:**
- Create: `apps/desktop/src-tauri/src/notebook_scope.rs`
- Modify: `packages/app/src/lib/settings/local-state.ts`
- Modify: `packages/app/src/lib/settings/local-state.test.ts`
- Modify: `packages/app/src/hooks/usePrimaryWorkspace.ts`
- Modify: `packages/app/src/hooks/usePrimaryWorkspace.test.tsx`
- Modify: `packages/app/src/runtime/index.ts`
- Modify: `apps/desktop/src/runtime/tauri/managed-workspace.ts`
- Modify: `apps/desktop/src/runtime/desktop.ts`
- Modify: `apps/desktop/src/runtime/mobile.ts`
- Modify: `apps/desktop/src-tauri/src/managed_workspace.rs`
- Modify: `apps/desktop/src-tauri/src/primary_workspace.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/desktop_runtime.rs`
- Modify: `apps/desktop/src-tauri/src/mobile_runtime.rs`

**Interfaces:**

```ts
export type PrimaryWorkspaceState = {
  desktopPath: string | null;
  managedName: string | null;
  onboardingCompleted: boolean;
  onboardingRequestedForNextLaunch?: true;
  version: 2;
};

export type AppWorkspaceRuntime = {
  isDocumentInRoot?: (documentPath: string, rootPath: string) => Promise<boolean>;
  resolveManagedRoot: (name: string) => Promise<string | null>;
};

export type PrimaryWorkspaceController = {
  canChooseDesktopRoot: boolean;
  commitDesktopRoot: (path: string) => Promise<string | null>;
  commitManagedRoot: (name: string) => Promise<string | null>;
  deferDesktopSetup: () => Promise<unknown>;
  error: string | null;
  managedName: string | null;
  resetOnboarding: () => Promise<unknown>;
  retry: () => Promise<unknown>;
  root: string | null;
  status: PrimaryWorkspaceStatus;
};
```

- [ ] **Step 1: Write failing normalization, validation, and managed-root tests**

Add tests proving that version `1`, mixed desktop/mobile identities, empty names, `.`, `..`, separators, `.qingyu`, and `.markra-sync` are rejected; Unicode and spaces are retained; `workspaces/personal/` is created without following symlinks; and a version-2 desktop/mobile state reopens its exact root.

- [ ] **Step 2: Run the focused tests and verify red state**

Run: `pnpm --filter @markra/app exec vitest run src/lib/settings/local-state.test.ts src/hooks/usePrimaryWorkspace.test.tsx --environment jsdom --globals`

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml notebook_scope`

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml managed_workspace`

Expected: FAIL because state version `2`, managed names, and named managed children do not exist.

- [ ] **Step 3: Implement strict version-2 state and safe single-segment resolution**

```rust
pub(crate) fn validate_notebook_name(name: &str) -> Result<String, String>;
pub(crate) fn notebook_name_from_root(root: &Path) -> Result<String, String>;
pub(crate) fn notes_remote_prefix(root: &ValidRemoteRoot, name: &str) -> Result<String, String>;

pub(crate) fn ensure_managed_workspace_path(
    app_data_root: &Path,
    name: &str,
) -> Result<PathBuf, String>;
```

Use a literal `workspaces` collection, validate one logical name before any join, reject symlink/non-directory collection and child entries, canonicalize the result, and verify the relative canonical path equals `workspaces/<name>`. Validation preserves Unicode and ordinary leading/trailing spaces exactly; it never feeds the basename through a remote-path normalizer that trims segments. `normalizePrimaryWorkspaceState` accepts only version `2`; every other shape returns `defaultPrimaryWorkspaceState`. Update native `StoredPrimaryWorkspaceState` and authoritative mobile resolution to the same version/name contract. Desktop commit clears `managedName`; mobile commit clears `desktopPath`.

- [ ] **Step 4: Run focused tests and typecheck**

Run the two commands from Step 2, then run `pnpm typecheck:test`.

Expected: PASS for schema rejection, canonical desktop roots, named managed children, protected names, symlink escape prevention, relaunch, and runtime request mapping.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src-tauri/src/notebook_scope.rs apps/desktop/src-tauri/src/managed_workspace.rs apps/desktop/src-tauri/src/primary_workspace.rs apps/desktop/src-tauri/src/lib.rs apps/desktop/src-tauri/src/desktop_runtime.rs apps/desktop/src-tauri/src/mobile_runtime.rs packages/app/src/lib/settings/local-state.ts packages/app/src/lib/settings/local-state.test.ts packages/app/src/hooks/usePrimaryWorkspace.ts packages/app/src/hooks/usePrimaryWorkspace.test.tsx packages/app/src/runtime/index.ts apps/desktop/src/runtime/tauri/managed-workspace.ts apps/desktop/src/runtime/desktop.ts apps/desktop/src/runtime/mobile.ts
git commit -m "refactor: make primary notebooks name-addressed"
```

### Task 2: Namespace Notes by Directory Name and Isolate Local Sync State

**Files:**
- Modify: `apps/desktop/src-tauri/src/notebook_scope.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/backend.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/scope.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/service.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/live_tests.rs`
- Modify: `apps/desktop/src-tauri/src/sync_config/status.rs`

**Interfaces:**

```rust
pub(crate) struct NotebookSyncScope {
    pub(crate) canonical_root: PathBuf,
    pub(crate) name: String,
    pub(crate) remote_prefix: String,
    pub(crate) state_root: PathBuf,
}

pub(crate) fn notebook_state_key(
    target_fingerprint_source: &str,
    remote_root: &str,
    notebook_name: &str,
    canonical_local_root: &Path,
) -> String;
```

- [ ] **Step 1: Write failing namespace and state-isolation tests**

Test `/Notes/A -> root/notes/A`, Unicode/spaced names, two canonical roots with basename `A` producing different hashes, provider targets producing different hashes, switching back to the same tuple producing the same hash, and settings using `sync-state/settings/<target-hash>/manifest.json` independently of notebook switches.

- [ ] **Step 2: Run focused Rust tests and verify red state**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml notebook_scope`

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync::service`

Expected: FAIL because notes still target `<remoteRoot>/notes` and share one provider manifest.

- [ ] **Step 3: Resolve backends before scopes and address local state by hash**

Create the concrete notes backend at `<remoteRoot>/notes/<name>`, derive the local-only SHA-256 key from the backend fingerprint source plus the explicit normalized fields, securely create `sync-state/notes/<hash>/`, and construct the notes scope there with manifest name `manifest.json`. Create the settings backend at `<remoteRoot>/app`, derive a global target key without any notebook root, and construct the settings scope below `sync-state/settings/<hash>/`.

Do not put the name or hash in the notebook. Keep the status file application-local, add `notebookName: string` to the Rust and TypeScript run/status payloads, and reject any result whose immutable root, name, or revision differs from its request.

- [ ] **Step 4: Run remote-sync and status suites**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync:: sync_config::status`

Expected: PASS for provider parity, isolated manifests/conflicts/staging, app settings independence, safe status, and same-name different-root baselines.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src-tauri/src/notebook_scope.rs apps/desktop/src-tauri/src/remote_sync/backend.rs apps/desktop/src-tauri/src/remote_sync/scope.rs apps/desktop/src-tauri/src/remote_sync/service.rs apps/desktop/src-tauri/src/remote_sync/live_tests.rs apps/desktop/src-tauri/src/sync_config/status.rs packages/app/src/lib/sync-config.ts apps/desktop/src/runtime/tauri/sync-config/shared.ts apps/desktop/src/runtime/tauri/sync-config.test.ts
git commit -m "feat: namespace notebook synchronization by name"
```

### Task 3: Make First Sync Remote-First Without Overwriting Either Side

**Files:**
- Modify: `apps/desktop/src-tauri/src/remote_sync/engine.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/service.rs`

**Interfaces:**

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RemoteSyncPhase {
    RemoteHydration,
    LocalPublication,
}

fn ordered_first_sync_actions(
    planned: BTreeMap<String, FileSyncAction>,
) -> Vec<(RemoteSyncPhase, String, FileSyncAction)>;
```

- [ ] **Step 1: Write failing action-order and conflict tests**

Use a recording backend to prove that, with no effective baseline, a lexically later remote-only file downloads before a lexically earlier local-only file uploads; equal files are skipped; both-different files create a visible remote conflict copy and retain the local original; a failure after one validated download leaves a resumable checkpoint; and steady-state delete semantics remain unchanged.

- [ ] **Step 2: Run engine tests and verify red state**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync::engine`

Expected: FAIL because the current path-ordered loop can upload before remote hydration.

- [ ] **Step 3: Plan once, execute in safe phases through the existing engine**

Bump the private manifest schema and add `full_scan_completed: bool`. A missing manifest, target change, local-identity change, or interrupted bootstrap has `full_scan_completed: false`; only a completely successful scan writes `true`. Keep per-action checkpoints, but do not treat a partial checkpoint as a deletion-authoritative baseline on retry.

When `full_scan_completed` is false for a notes scope, compute all actions before mutation. Execute remote-only downloads and both-side comparisons/conflicts first, then skips, then local-only uploads; save the incomplete manifest after every atomic mutation and set the completion flag only at the end. To distinguish equal both-side content from a conflict on first sync, download and hash the identity-validated remote bytes: equal content writes a skip checkpoint, different content uses the existing visible conflict-copy path. Never apply deletion actions without a completed baseline. Preserve the existing target identity checks, staged downloads, no-replace publication, conflict naming, upload revalidation, and global execution lock. Settings retain their existing remote-wins-without-baseline rule.

- [ ] **Step 4: Run the complete engine and service suites**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync::engine`

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync::service`

Expected: PASS including existing deletion, race, settings reconcile, and conflict tests.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src-tauri/src/remote_sync/engine.rs apps/desktop/src-tauri/src/remote_sync/service.rs
git commit -m "feat: hydrate remote notebooks before upload"
```

### Task 4: Add a Shallow Remote Notebook Catalog

**Files:**
- Create: `apps/desktop/src-tauri/src/remote_sync/catalog.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/s3_backend.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/backend.rs`
- Modify: `apps/desktop/src-tauri/src/sync_config.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/desktop_runtime.rs`
- Modify: `apps/desktop/src-tauri/src/mobile_runtime.rs`
- Modify: `packages/app/src/lib/sync-config.ts`
- Modify: `packages/app/src/runtime/index.ts`
- Modify: `apps/desktop/src/runtime/tauri/sync-config/shared.ts`
- Modify: `apps/desktop/src/runtime/tauri/sync-config.test.ts`
- Modify: `apps/desktop/src/runtime/desktop.ts`
- Modify: `apps/desktop/src/runtime/mobile.ts`

**Interfaces:**

```rust
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoteNotebookCatalogEntry {
    pub(crate) available: bool,
    pub(crate) disabled_reason: Option<String>,
    pub(crate) name: String,
}

pub(crate) async fn list_remote_notebooks(
    snapshot: SyncSnapshot,
) -> Result<Vec<RemoteNotebookCatalogEntry>, String>;
```

```ts
export type RemoteNotebookCatalogEntry = {
  available: boolean;
  disabledReason: string | null;
  name: string;
};

// Added to AppSyncConfigRuntime
listNotebooks(input: { revision: string }): Promise<RemoteNotebookCatalogEntry[]>;
```

- [ ] **Step 1: Write failing provider parser and command tests**

For S3, assert `list-type=2`, `prefix=<root>/notes/`, `delimiter=/`, continuation pagination, decoded one-level `CommonPrefixes`, no object bodies, and no nested descendants in results. For WebDAV, assert one `Depth: 1` PROPFIND on `<root>/notes/`, child collections only, self-response removal, URL decoding, no recursive request, and safe disabled entries for current-OS-invalid names.

- [ ] **Step 2: Run catalog and adapter tests and verify red state**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync::catalog remote_sync::s3_backend`

Run: `pnpm --filter @markra/desktop exec vitest run src/runtime/tauri/sync-config.test.ts --environment jsdom --globals`

Expected: FAIL because catalog parsing and `list_remote_notebooks` are absent.

- [ ] **Step 3: Implement read-only catalog dispatch**

Add `configured_snapshot_at_app_data`: it validates the expected revision and complete provider fields without requiring `enabled: true`. Catalog and an explicit cloud-bootstrap request use this snapshot; ordinary launch/save/interval/manual sync continues to require `ready_snapshot_at_app_data`. Listing therefore never enables synchronization or mutates config.

Construct a catalog client at the literal notes parent rather than a selected child. The S3 implementation sends only paginated GET requests. The WebDAV implementation must not reuse `create_webdav_backend`, because that function ensures collections with `MKCOL`; instead issue one direct `Depth: 1` PROPFIND and treat a missing notes parent as an empty catalog. Sanitize errors to stable codes. Sort entries by Unicode name and deduplicate exact logical names. Exclude empty, `.`, `..`, `.qingyu`, and `.markra-sync`; return representable but current-platform-invalid names as disabled.

- [ ] **Step 4: Run catalog, runtime, registration, and type tests**

Run the commands from Step 2, then run `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml builder_boundary_tests`, `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mobile_platform_config_tests`, and `pnpm typecheck:test`.

Expected: PASS with exact desktop/mobile command registration and no credential-bearing payloads.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src-tauri/src/remote_sync/catalog.rs apps/desktop/src-tauri/src/remote_sync.rs apps/desktop/src-tauri/src/remote_sync/s3_backend.rs apps/desktop/src-tauri/src/remote_sync/backend.rs apps/desktop/src-tauri/src/sync_config.rs apps/desktop/src-tauri/src/lib.rs apps/desktop/src-tauri/src/desktop_runtime.rs apps/desktop/src-tauri/src/mobile_runtime.rs packages/app/src/lib/sync-config.ts packages/app/src/runtime/index.ts apps/desktop/src/runtime/tauri/sync-config/shared.ts apps/desktop/src/runtime/tauri/sync-config.test.ts apps/desktop/src/runtime/desktop.ts apps/desktop/src/runtime/mobile.ts
git commit -m "feat: list remote notebook directories"
```

### Task 5: Add a Safe Notebook Switch and Restore Coordinator

**Files:**
- Create: `packages/app/src/hooks/useNotebookSwitchCoordinator.ts`
- Create: `packages/app/src/hooks/useNotebookSwitchCoordinator.test.tsx`
- Create: `packages/app/src/lib/notebook-switch-events.ts`
- Create: `packages/app/src/lib/notebook-switch-events.test.ts`
- Modify: `packages/app/src/hooks/useAppSyncCoordinator.ts`
- Modify: `packages/app/src/hooks/useAppSyncCoordinator.test.tsx`
- Modify: `packages/app/src/hooks/usePrimaryWorkspace.ts`
- Modify: `packages/app/src/lib/settings/recent-markdown.ts`
- Modify: `packages/app/src/lib/settings/local-state.ts`
- Modify: `packages/app/src/App.tsx`
- Modify: `packages/app/src/App.test.tsx`
- Modify: `packages/app/src/lib/sync-config.ts`
- Modify: `apps/desktop/src-tauri/src/sync_config.rs`
- Modify: `apps/desktop/src-tauri/src/notebook_scope.rs`
- Modify: `apps/desktop/src-tauri/src/primary_workspace.rs`
- Modify: `packages/app/src/runtime/index.ts`
- Modify: `apps/desktop/src/runtime/tauri/managed-workspace.ts`
- Modify: `apps/desktop/src/runtime/desktop.ts`
- Modify: `apps/desktop/src/runtime/mobile.ts`
- Modify: `apps/desktop/src-tauri/src/remote_sync/service.rs`

**Interfaces:**

```ts
export type AppSyncCoordinator = {
  beginNotebookSwitch: () => Promise<void>;
  finishNotebookSwitch: () => unknown;
  notifyDocumentSaved: (documentPath: string) => Promise<unknown>;
  run: (trigger: SyncTrigger, revision?: string) => Promise<SyncRunResult | null>;
  running: boolean;
  status: SyncStatus | null;
};

export type NotebookSwitchCoordinator = {
  restoreDesktopNotebook: (remoteName: string) => Promise<string | null>;
  restoreManagedNotebook: (remoteName: string) => Promise<string | null>;
  switchDesktopNotebook: (path?: string) => Promise<string | null>;
  switchManagedNotebook: (name: string) => Promise<string | null>;
  switching: boolean;
};

export type NotebookSwitchRequest = {
  path?: string;
  source: "file-menu" | "native-open" | "recent" | "settings" | "welcome";
};
```

```rust
#[tauri::command]
pub(crate) fn prepare_desktop_notebook_target(
    parent_path: String,
    notebook_name: String,
) -> Result<String, String>;
```

- [ ] **Step 1: Write failing transaction and stale-completion tests**

Prove the order `flush -> block -> drain -> persist -> mount -> sync`; a cancelled picker leaves the old root active; save or drain failure aborts before persistence; queued old-root runs never start; an active paired run completes its settings publication before the new root is persisted; an old completion cannot publish current status; post-persistence network failure keeps the new local root; failed cloud bootstrap keeps the old root and preserves the target directory; successful bootstrap persists exactly once.

- [ ] **Step 2: Run coordinator tests and verify red state**

Run: `pnpm --filter @markra/app exec vitest run src/hooks/useNotebookSwitchCoordinator.test.tsx src/hooks/useAppSyncCoordinator.test.tsx src/App.test.tsx --environment jsdom --globals`

Expected: FAIL because switching currently persists directly and the sync coordinator has no barrier/drain contract.

- [ ] **Step 3: Implement one transaction for every local directory change**

`beginNotebookSwitch()` sets a module-visible barrier, increments the old generation so queued runs cancel at `shouldStart`, and awaits every already-started `SharedRun` whose immutable `notesRoot` equals the old root. It does not abort the native paired transaction. `finishNotebookSwitch()` releases automatic triggers only after the new root prop is installed.

`switchDesktopNotebook` resolves either the supplied path or a directory picker, flushes the active document, enters the barrier, commits through `primaryWorkspace.commitDesktopRoot`, and releases the barrier in all outcomes. Only a successful commit prepends the canonical root to recent notebook history. Once the new root mounts, request one visible `app-launch` run if global sync is ready. Mobile follows the same transaction with a validated managed name.

Only the main primary window owns the coordinator. Settings windows and native request adapters emit `qingyu://notebook-switch-requested`; the main window validates the request and calls the coordinator. External-file windows cannot publish primary state. Coalesce simultaneous native directory requests deterministically to the last valid path.

For desktop restore, pick a parent and call `prepare_desktop_notebook_target`; the native command canonicalizes the parent, validates one exact notebook segment, rejects symlink/non-directory targets, securely creates or reuses only `<parent>/<name>`, and returns its canonical path without publishing primary ownership. Mobile uses the named managed-root resolver. Invoke native sync with `{ bootstrap: true, notesRoot, revision, trigger: "manual" }` using the configured snapshot even when global sync is disabled, and commit primary ownership only after success. The native service derives and validates the target basename; the frontend cannot supply an independent remote identity.

- [ ] **Step 4: Run hook, App, native service, and type tests**

Run the command from Step 2, then run `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml sync_config`, `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync::service`, `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml primary_workspace`, and `pnpm typecheck:test`.

Expected: PASS for safe ordering, stale suppression, current-root validation, bootstrap non-publication on failure, and unchanged global config revisions.

- [ ] **Step 5: Commit**

```bash
git add packages/app/src/hooks/useNotebookSwitchCoordinator.ts packages/app/src/hooks/useNotebookSwitchCoordinator.test.tsx packages/app/src/lib/notebook-switch-events.ts packages/app/src/lib/notebook-switch-events.test.ts packages/app/src/hooks/useAppSyncCoordinator.ts packages/app/src/hooks/useAppSyncCoordinator.test.tsx packages/app/src/hooks/usePrimaryWorkspace.ts packages/app/src/lib/settings/recent-markdown.ts packages/app/src/lib/settings/local-state.ts packages/app/src/App.tsx packages/app/src/App.test.tsx packages/app/src/lib/sync-config.ts packages/app/src/runtime/index.ts apps/desktop/src/runtime/tauri/managed-workspace.ts apps/desktop/src/runtime/desktop.ts apps/desktop/src/runtime/mobile.ts apps/desktop/src-tauri/src/sync_config.rs apps/desktop/src-tauri/src/notebook_scope.rs apps/desktop/src-tauri/src/primary_workspace.rs apps/desktop/src-tauri/src/remote_sync/service.rs
git commit -m "feat: switch notebooks through one safe transaction"
```

### Task 6: Build Desktop and Mobile Notebook Selection Surfaces

**Files:**
- Create: `packages/app/src/components/notebooks/RemoteNotebookDialog.tsx`
- Create: `packages/app/src/components/notebooks/RemoteNotebookDialog.test.tsx`
- Create: `packages/app/src/components/notebooks/MobileNotebookDialog.tsx`
- Create: `packages/app/src/components/notebooks/MobileNotebookDialog.test.tsx`
- Modify: `packages/app/src/components/onboarding/WelcomeScreen.tsx`
- Modify: `packages/app/src/components/onboarding/WelcomeScreen.test.tsx`
- Modify: `packages/app/src/components/settings/NotesWorkspaceSettings.tsx`
- Modify: `packages/app/src/components/settings/NotesWorkspaceSettings.test.tsx`
- Modify: `packages/app/src/components/SettingsWindow.tsx`
- Modify: `packages/app/src/components/SettingsWindow.test.tsx`
- Modify: `packages/app/src/components/compact/CompactSettingsDetail.tsx`
- Modify: `packages/app/src/components/compact/CompactSettingsDetail.test.tsx`
- Modify: `packages/app/src/App.tsx`
- Modify: `packages/app/src/App.test.tsx`
- Modify: `packages/app/src/styles.css`
- Modify: `packages/shared/src/i18n/locales/en.ts`
- Modify: `packages/shared/src/i18n/locales/zh-CN.ts`
- Modify: `packages/shared/src/i18n/locales/zh-TW.ts`

**Interfaces:**

```ts
export type RemoteNotebookDialogProps = {
  entries: readonly RemoteNotebookCatalogEntry[];
  error: string | null;
  loading: boolean;
  onCancel: () => unknown;
  onRefresh: () => Promise<unknown>;
  onRestore: (name: string) => Promise<unknown>;
};
```

- [ ] **Step 1: Write failing onboarding, catalog-selection, and mobile tests**

Assert desktop welcome offers choose local notebook, restore from cloud, standalone file, and defer, with no external-folder action. Assert Settings says “Switch Notebook Directory”. Assert the remote dialog loads names only, selects exactly one enabled entry, does not create other local directories, reuses an existing same-name child with a merge warning, and opens Sync settings when config is incomplete. Assert mobile can create a validated named notebook, list managed children, switch one, and restore one remote child below `workspaces/`.

- [ ] **Step 2: Run component and App tests and verify red state**

Run: `pnpm --filter @markra/app exec vitest run src/components/onboarding/WelcomeScreen.test.tsx src/components/notebooks/RemoteNotebookDialog.test.tsx src/components/notebooks/MobileNotebookDialog.test.tsx src/components/settings/NotesWorkspaceSettings.test.tsx src/components/SettingsWindow.test.tsx src/components/compact/CompactSettingsDetail.test.tsx src/App.test.tsx --environment jsdom --globals`

Expected: FAIL because cloud restore, named mobile notebooks, and switch wording are absent.

- [ ] **Step 3: Implement the focused notebook UI**

Keep the approved welcome visual language and slogan “明窗净几，字字轻语。” without a fake step indicator. Use a desktop dialog and a compact mobile sheet, both driven by the same catalog data. Desktop restore selects one remote name then a local parent; the target is `<parent>/<name>`. Mobile restore uses `workspaces/<name>` and has no filesystem picker. Loading, empty, disabled-name, connection-incomplete, bootstrap-progress, retry, and merge-warning states are explicit. The UI never displays provider URLs or credentials.

- [ ] **Step 4: Run component, i18n, full frontend, and build tests**

Run the command from Step 2, then run `pnpm test`, `pnpm typecheck:test`, and `pnpm build`.

Expected: PASS with desktop and compact/mobile render paths.

- [ ] **Step 5: Commit**

```bash
git add packages/app/src/components/notebooks packages/app/src/components/onboarding/WelcomeScreen.tsx packages/app/src/components/onboarding/WelcomeScreen.test.tsx packages/app/src/components/settings/NotesWorkspaceSettings.tsx packages/app/src/components/settings/NotesWorkspaceSettings.test.tsx packages/app/src/components/SettingsWindow.tsx packages/app/src/components/SettingsWindow.test.tsx packages/app/src/components/compact/CompactSettingsDetail.tsx packages/app/src/components/compact/CompactSettingsDetail.test.tsx packages/app/src/App.tsx packages/app/src/App.test.tsx packages/app/src/styles.css packages/shared/src/i18n/locales/en.ts packages/shared/src/i18n/locales/zh-CN.ts packages/shared/src/i18n/locales/zh-TW.ts
git commit -m "feat: add notebook switch and cloud restore surfaces"
```

### Task 7: Remove External-Folder Behavior and Route Native Entry Points

**Files:**
- Modify: `packages/app/src/App.tsx`
- Modify: `packages/app/src/App.test.tsx`
- Modify: `packages/app/src/components/MarkdownFileTreeDrawer.tsx`
- Modify: `packages/app/src/components/MarkdownFileTreeDrawer.test.tsx`
- Modify: `packages/app/src/components/NativeTitleBar.tsx`
- Modify: `packages/app/src/components/NativeTitleBar.test.tsx`
- Modify: `packages/app/src/hooks/useMarkdownFileTree.ts`
- Modify: `packages/app/src/hooks/useMarkdownFileTree.test.tsx`
- Modify: `packages/app/src/hooks/useMarkdownDocument.ts`
- Modify: `packages/app/src/hooks/useMarkdownDocument.test.tsx`
- Modify: `packages/app/src/hooks/useNativeBindings.ts`
- Modify: `packages/app/src/hooks/useNativeBindings.test.tsx`
- Modify: `packages/app/src/hooks/useNativeMarkdownDrop.ts`
- Modify: `packages/app/src/hooks/useNativeMarkdownDrop.test.tsx`
- Modify: `packages/app/src/lib/settings/local-state.ts`
- Modify: `packages/app/src/lib/settings/local-state.test.ts`
- Modify: `packages/app/src/lib/tauri/file.ts`
- Modify: `packages/app/src/lib/editor-window-context.ts`
- Modify: `packages/app/src/lib/editor-window-context.test.ts`
- Modify: `packages/app/src/runtime/index.ts`
- Modify: `apps/desktop/src/runtime/desktop.ts`
- Modify: `apps/desktop/src/runtime/tauri/file/desktop.ts`
- Modify: `apps/desktop/src/runtime/tauri/file.test.ts`
- Modify: `apps/desktop/src-tauri/src/menu.rs`
- Modify: `apps/desktop/src-tauri/src/menu_labels/en.rs`
- Modify: `apps/desktop/src-tauri/src/menu_labels/zh_cn.rs`
- Modify: `apps/desktop/src-tauri/src/windows.rs`
- Modify: `apps/desktop/src-tauri/src/markdown_files/open.rs`
- Modify: `apps/desktop/src-tauri/src/markdown_files/path.rs`
- Modify: `apps/desktop/src-tauri/src/opened_files.rs`
- Modify: `apps/desktop/src-tauri/src/builder_boundary_tests.rs`
- Modify: `apps/desktop/src-tauri/src/desktop_runtime.rs`
- Modify: `packages/shared/src/i18n/locales/en.ts`
- Modify: `packages/shared/src/i18n/locales/zh-CN.ts`
- Modify: `apps/desktop/src-tauri/src/menu_labels/de.rs`
- Modify: `apps/desktop/src-tauri/src/menu_labels/en.rs`
- Modify: `apps/desktop/src-tauri/src/menu_labels/es.rs`
- Modify: `apps/desktop/src-tauri/src/menu_labels/fr.rs`
- Modify: `apps/desktop/src-tauri/src/menu_labels/it.rs`
- Modify: `apps/desktop/src-tauri/src/menu_labels/ja.rs`
- Modify: `apps/desktop/src-tauri/src/menu_labels/ko.rs`
- Modify: `apps/desktop/src-tauri/src/menu_labels/pt_br.rs`
- Modify: `apps/desktop/src-tauri/src/menu_labels/ru.rs`
- Modify: `apps/desktop/src-tauri/src/menu_labels/zh_cn.rs`
- Modify: `apps/desktop/src-tauri/src/menu_labels/zh_tw.rs`

- [ ] **Step 1: Write failing native-routing and removal tests**

Assert File menu and titlebar folder actions dispatch “Switch Notebook Directory”; command/shortcut, picker, directory drag/drop, CLI/second-instance, and OS folder-open requests call the same switch coordinator; the file-open picker accepts Markdown files only; standalone file windows still open without mounting their parent as a notebook/file tree; no `open_markdown_folder_in_new_window`, `editor_window_url_for_folder`, or external-folder welcome callback remains; recent notebook buttons invoke the coordinator rather than directly mounting a tree; and MCP workspace/document tests still resolve only the current primary root without a new catalog tool.

- [ ] **Step 2: Run App, file adapter, menu, window, and MCP tests and verify red state**

Run: `pnpm --filter @markra/app exec vitest run src/App.test.tsx src/components/MarkdownFileTreeDrawer.test.tsx src/components/NativeTitleBar.test.tsx src/hooks/useMarkdownFileTree.test.tsx --environment jsdom --globals`

Run: `pnpm --filter @markra/desktop exec vitest run src/runtime/tauri/file.test.ts --environment jsdom --globals`

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml menu`

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml windows`

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mcp`

Expected: FAIL because folder targets still create independent windows and recent-directory actions still bypass the primary-notebook transaction.

- [ ] **Step 3: Delete external-folder branches and route folders to switching**

Keep `openMarkdownFileInNewWindow` and containing-folder reveal. Remove `openMarkdownFolderInNewWindow` from runtime types/adapters/commands, including the web runtime stub, remove folder URL construction, and make native folder-open events emit a main-window switch request. Replace the combined main-window file/folder picker with separate file-only and switch-directory actions. Stop external-file windows from deriving a file-tree root from the file parent; relative links and adjacent `assets/` continue to resolve from the file path. Preserve recent-directory persistence only as notebook history: tree mounting never writes it, successful coordinator transactions do, and selecting an entry calls the coordinator. Preserve ordinary subfolder creation/navigation inside the current notebook.

MCP has no remote catalog or arbitrary external-folder endpoint in the current codebase; keep its current-notebook authority tests and do not invent a switch or restore tool in this task.

- [ ] **Step 4: Run focused removal tests and search for stale production paths**

Run the commands from Step 2.

Run: `rg -n "openExternalFolder|openMarkdownFolderInNewWindow|open_markdown_folder_in_new_window|editor_window_url_for_folder|external-folder" packages/app/src apps/desktop/src apps/web/src apps/desktop/src-tauri/src`

Expected: no production external-folder window path; any remaining match is a deliberate deletion assertion or historical documentation.

- [ ] **Step 5: Commit**

```bash
git add packages/app/src/App.tsx packages/app/src/App.test.tsx packages/app/src/components/MarkdownFileTreeDrawer.tsx packages/app/src/components/MarkdownFileTreeDrawer.test.tsx packages/app/src/components/NativeTitleBar.tsx packages/app/src/components/NativeTitleBar.test.tsx packages/app/src/hooks/useMarkdownFileTree.ts packages/app/src/hooks/useMarkdownFileTree.test.tsx packages/app/src/hooks/useMarkdownDocument.ts packages/app/src/hooks/useMarkdownDocument.test.tsx packages/app/src/hooks/useNativeBindings.ts packages/app/src/hooks/useNativeBindings.test.tsx packages/app/src/hooks/useNativeMarkdownDrop.ts packages/app/src/hooks/useNativeMarkdownDrop.test.tsx packages/app/src/lib/settings/local-state.ts packages/app/src/lib/settings/local-state.test.ts packages/app/src/lib/tauri/file.ts packages/app/src/lib/editor-window-context.ts packages/app/src/lib/editor-window-context.test.ts packages/app/src/runtime/index.ts apps/desktop/src/runtime/desktop.ts apps/desktop/src/runtime/tauri/file/desktop.ts apps/desktop/src/runtime/tauri/file.test.ts apps/web/src/runtime/web/file.ts apps/desktop/src-tauri/src/menu.rs apps/desktop/src-tauri/src/menu_labels apps/desktop/src-tauri/src/windows.rs apps/desktop/src-tauri/src/markdown_files/open.rs apps/desktop/src-tauri/src/markdown_files/path.rs apps/desktop/src-tauri/src/opened_files.rs apps/desktop/src-tauri/src/builder_boundary_tests.rs apps/desktop/src-tauri/src/desktop_runtime.rs packages/shared/src/i18n/locales/en.ts packages/shared/src/i18n/locales/zh-CN.ts
git commit -m "refactor: make every folder open switch notebooks"
```

### Task 8: Verify Real Providers, Desktop Runtime, and Mobile Packaging

**Files:**
- Modify: `apps/desktop/src-tauri/src/remote_sync/live_tests.rs`
- Modify: `scripts/test-s3-sync-live.mjs`
- Modify: `package.json`
- Modify: `README.md`
- Modify: `docs/superpowers/specs/2026-07-21-qingyu-named-notebook-switching-design.md`

- [ ] **Step 1: Extend credential-free live-test contracts**

Add ignored/live scenarios that create two uniquely prefixed remote notebooks, verify shallow catalog names, restore exactly one, switch A -> B -> A, confirm isolated manifests and remote keys, verify same-name remote hydration occurs before local-only upload, verify `<remoteRoot>/app/settings.json` remains stable, and delete only the unique live-test prefix during cleanup. Credentials are read from process environment only and never printed or written.

- [ ] **Step 2: Run all automated gates**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml`

Run: `pnpm test`

Run: `pnpm typecheck:test`

Run: `pnpm build`

Run: `git diff --check`

Expected: every command exits `0`; default-parallel Rust tests pass without single-thread fallback.

- [ ] **Step 3: Run the real MinIO scenario when the configured server is reachable**

Run the repository live-test command with credentials supplied only to that process. Verify the remote object list contains `<unique-root>/notes/A/**`, `<unique-root>/notes/B/**`, and `<unique-root>/app/settings.json`, and that selecting A never downloads B. Remove the unique live-test root afterward and confirm cleanup by listing it again.

- [ ] **Step 4: Run desktop and mobile runtime checks**

Stop stale QingYu instances, clear only the test app-data state, build and launch the current debug `.app`, then verify: welcome copy, choose A, restart restores A, switch B, restart restores B, standalone file leaves B current, switch back to A, remote restore creates only the selected child, and a provider failure leaves the selected local notebook usable. Build the true-mobile target and verify `workspaces/<name>` resolution and mobile notebook switching through the packaged runtime.

- [ ] **Step 5: Review and commit verification/docs**

Request a code review focused on duplicate directory-open logic, duplicate provider listing, duplicate sync algorithms, stale root-level `notes/**` paths, stale version-1 state readers, credential exposure, and concurrency ownership. Address findings, rerun the smallest affected gate plus the full gates above, then commit.

```bash
git add apps/desktop/src-tauri/src/remote_sync/live_tests.rs scripts/test-s3-sync-live.mjs package.json README.md docs/superpowers/specs/2026-07-21-qingyu-named-notebook-switching-design.md
git commit -m "test: verify named notebook synchronization"
```
