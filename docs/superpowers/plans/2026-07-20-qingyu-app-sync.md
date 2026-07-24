# QingYu Application Sync Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace project-owned sync configuration with one application-local WebDAV/S3 configuration that synchronizes the primary notes workspace and portable `settings.json` through one shared engine.

**Architecture:** Store configuration, manifests, status, durable download staging, and quarantines under application data. Refactor the existing remote-sync engine so a run receives explicit local source, local state, remote prefix, include policy, and validation policy; use that engine once for `notes/**` and once for `app/settings.json`. Cross-filesystem atomic publication may use only a short-lived protected temp file beside the destination. The frontend coordinator is application-scoped and captures immutable primary-root/config revisions for every trigger.

**Tech Stack:** Rust, Tauri v2, serde/serde_json, reqwest, AWS SDK for Rust, React 19, TypeScript, Vitest, cargo test, MinIO live tests.

## Global Constraints

- There is exactly one sync configuration per device in app-data `sync-config.json`.
- Credentials remain plaintext in `sync-config.json` and never enter logs, events, status, diagnostics, exported settings, MCP output, or remote synchronization.
- The primary notes workspace contains no configuration, manifest, status, persistent staging, conflict, or runtime files. A protected `.markra-sync-stage-*` file may exist only transiently beside a destination while validated bytes are fsynced and atomically published across filesystems; it is never scanned, watched, manifested, or synchronized and is cleaned without following symbolic links.
- One configured remote root contains `<remoteRoot>/notes/**` and `<remoteRoot>/app/settings.json`.
- `remoteRoot` is a safe non-root relative path with no `.` or `..` segments.
- Notes include ordinary files of every supported binary/text type; empty directories and symbolic links remain unsupported.
- `.qingyu`, `.markra-sync`, and sync staging prefixes remain excluded locally and remotely.
- Settings scope includes exactly portable `settings.json` and cannot read any other app-data file or directory.
- Invalid remote settings are quarantined locally and never replace active settings.
- Settings conflicts keep local settings active and quarantine the remote version.
- Switching primary roots clears only the notes baseline; the settings baseline remains.
- Every run captures immutable primary-root, configuration revision, target fingerprint, trigger, and editing apply token.
- Settings fields save immediately; save and interval triggers pause during Sync settings editing and apply only after category leave/window close.
- External document saves never trigger synchronization.
- Do not read, migrate, rewrite, or delete legacy folder configuration.
- Do not add a second sync algorithm or dependency.

---

## File Structure

- `apps/desktop/src-tauri/src/sync_config/`: owns app-data config schema, validation, revisioned storage, editing sessions, status, and Tauri commands.
- `apps/desktop/src-tauri/src/remote_sync/scope.rs`: describes local source/state/include/validation for a shared engine run.
- `apps/desktop/src-tauri/src/remote_sync/engine.rs`: remains the single comparison/conflict/checkpoint algorithm.
- `apps/desktop/src-tauri/src/remote_sync/service.rs`: creates notes and settings scopes from immutable run snapshots.
- `packages/app/src/lib/sync-config.ts`: owns frontend config/result/event types.
- `packages/app/src/hooks/useSyncConfig.ts`: loads the one application config and reacts to revision events.
- `packages/app/src/hooks/useAppSyncCoordinator.ts`: serializes/coalesces application-level triggers and enforces editing boundaries.
- `packages/app/src/components/settings/SyncSettings.tsx`: edits app config and shows primary workspace as read-only sync target.

### Task 1: Store One Revisioned Sync Configuration in Application Data

**Files:**
- Create: `apps/desktop/src-tauri/src/sync_config.rs`
- Create: `apps/desktop/src-tauri/src/sync_config/model.rs`
- Create: `apps/desktop/src-tauri/src/sync_config/storage.rs`
- Create: `apps/desktop/src-tauri/src/sync_config/editing.rs`
- Create: `apps/desktop/src-tauri/src/sync_config/status.rs`
- Create: `packages/app/src/lib/sync-config.ts`
- Create: `packages/app/src/lib/sync-config.test.ts`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/desktop_runtime.rs`
- Modify: `apps/desktop/src-tauri/src/mobile_runtime.rs`
- Modify: `packages/app/src/runtime/index.ts`
- Modify: `apps/desktop/src/runtime/desktop.ts`
- Modify: `apps/desktop/src/runtime/mobile.ts`
- Create: `apps/desktop/src/runtime/tauri/sync-config.ts`
- Create: `apps/desktop/src/runtime/tauri/sync-config/shared.ts`
- Create: `apps/desktop/src/runtime/tauri/sync-config.test.ts`

**Interfaces:**
- Produces Rust `SyncConfig`, `SyncConfigDocument`, `SyncConfigLoadResponse`, `SyncConfigPatch`, and `SyncSnapshot` without a project-root field.
- Produces Tauri commands `load_sync_config`, `enable_sync_config`, `patch_sync_config`, `reset_sync_config`, `recover_sync_config`, `set_sync_config_editing`, and `request_sync_config_apply`.
- Produces TypeScript `AppSyncConfigRuntime` at `getAppRuntime().syncConfig`; the legacy `projectConfig` adapter remains temporarily available until Task 5 so every intermediate commit builds.
- Produces revision event `qingyu://sync-config-changed` with `{ revision: string }`.

- [ ] **Step 1: Write failing Rust storage tests**

```rust
#[test]
fn sync_config_is_written_below_app_data_only() {
    let app_data = tempdir().unwrap();
    let notes = tempdir().unwrap();
    let stored = enable_at_app_data(app_data.path(), None).unwrap();

    assert_eq!(config_path(app_data.path()), app_data.path().join("sync-config.json"));
    assert_eq!(stored.document.config.version, 1);
    assert!(!notes.path().join(".qingyu").exists());
    assert!(!notes.path().join(".markra-sync").exists());
}

#[test]
fn persisted_credentials_never_appear_in_safe_status() {
    let config = configured_s3("access-value", "secret-value");
    let serialized_status = serde_json::to_string(&status_for_failed_run(&config)).unwrap();
    assert!(!serialized_status.contains("access-value"));
    assert!(!serialized_status.contains("secret-value"));
}

#[test]
fn malformed_config_reset_preserves_a_damaged_copy_under_app_data() {
    let app_data = tempdir().unwrap();
    write(app_data.path().join("sync-config.json"), b"{broken");
    reset_at_app_data(app_data.path(), true).unwrap();
    assert!(damaged_copies(app_data.path()).iter().any(|path| {
        path.file_name().unwrap().to_string_lossy().starts_with("sync-config.damaged-")
    }));
}
```

- [ ] **Step 2: Run targeted Rust and runtime adapter tests to verify red state**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml sync_config`

Expected: FAIL because the module and commands do not exist.

Run: `pnpm --filter @markra/desktop exec vitest run src/runtime/tauri/sync-config.test.ts --environment jsdom --globals`

Expected: FAIL because the application-level adapter does not exist.

- [ ] **Step 3: Implement the app-data model and runtime contract**

```rust
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncConfig {
    pub version: u8,
    pub enabled: bool,
    pub provider: SyncProvider,
    pub remote_root: String,
    pub auto_sync_on_save: bool,
    pub interval_minutes: u32,
    pub webdav: WebDavConfig,
    pub s3: S3Config,
}

#[derive(Clone)]
pub(crate) struct SyncSnapshot {
    pub config: SyncConfig,
    pub revision: String,
    pub state_root: PathBuf,
    pub target: SyncTarget,
}
```

Resolve `app.path().app_data_dir()` inside Tauri command wrappers, then call pure storage functions that accept `&Path` for deterministic tests. Writes use the existing temporary-file, file-sync, atomic-replace, parent-directory-sync pattern. Reuse normalization and issue codes from `project_config/model.rs`, rename user-facing codes from `project-*` to `sync-*`, and delete project-root membership from the config API. Preserve optimistic revisions and the editing/apply-token registry, keyed by one app-config identity rather than a root path.

`SyncConfig::default()` returns version `1`, provider `webdav`, `enabled: false`, an empty `remoteRoot`, `autoSyncOnSave: false`, interval `0`, and empty provider fields. A confirmed reset of malformed/unsupported content first writes a timestamped `sync-config.damaged-<timestamp>.json` beside the config, then atomically installs defaults. The damaged copy is local-only and its contents never enter status or events.

```ts
export type AppSyncConfigRuntime = {
  enable(input: { expectedRevision: string | null }): Promise<SyncConfigDocument>;
  load(): Promise<SyncConfigLoadResult>;
  loadEditing(): Promise<SyncEditingSnapshot>;
  loadStatus(): Promise<SyncStatus | null>;
  patch(input: { expectedRevision: string; patch: SyncConfigPatch }): Promise<SyncConfigDocument>;
  recover(input: { config: QingYuSyncConfig; expectedRevision: string }): Promise<SyncConfigDocument>;
  requestApply(input: SyncApplyUpdate): Promise<SyncApplyWriteResult>;
  reset(input: { confirmed: true; expectedRevision: string | null }): Promise<SyncConfigDocument>;
  setEditing(input: SyncEditingUpdate): Promise<SyncEditingWriteResult>;
  sync(input: SyncRunRequest): Promise<SyncRunResult>;
  testConnection(input: { revision: string }): Promise<SyncConnectionTestResult>;
};
```

- [ ] **Step 4: Run storage, adapter, and type tests**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml sync_config`

Expected: PASS for absent, enable, patch, revision conflict, malformed, unsupported, reset, recovery, editing session, atomic durability, safe status, and app-data paths.

Run: `pnpm --filter @markra/desktop exec vitest run src/runtime/tauri/sync-config.test.ts --environment jsdom --globals`

Expected: PASS with the renamed app-level runtime adapter.

Run: `pnpm typecheck:test`

Expected: PASS with the renamed app-level runtime contract.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src-tauri/src/sync_config.rs apps/desktop/src-tauri/src/sync_config packages/app/src/lib/sync-config.ts packages/app/src/lib/sync-config.test.ts apps/desktop/src-tauri/src/lib.rs apps/desktop/src-tauri/src/desktop_runtime.rs apps/desktop/src-tauri/src/mobile_runtime.rs packages/app/src/runtime/index.ts apps/desktop/src/runtime/desktop.ts apps/desktop/src/runtime/mobile.ts apps/desktop/src/runtime/tauri/sync-config.ts apps/desktop/src/runtime/tauri/sync-config apps/desktop/src/runtime/tauri/sync-config.test.ts
git commit -m "refactor: store sync configuration in app data"
```

### Task 2: Parameterize the Shared Remote-Sync Engine by Scope

**Files:**
- Create: `apps/desktop/src-tauri/src/remote_sync/scope.rs`
- Create: `apps/desktop/src-tauri/src/protected_paths.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/engine.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/backend.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/coordinator.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/live_tests.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/markdown_files/ignore_rules.rs`
- Modify: `apps/desktop/src-tauri/src/watcher.rs`
- Modify: `apps/desktop/src-tauri/src/watcher/directory.rs`

**Interfaces:**
- Consumes: `SyncSnapshot` from Task 1.
- Produces: `RemoteSyncScope` and `RemoteSyncIncludePolicy`.
- Produces: `execute_remote_sync(scope: &RemoteSyncScope, backend: &dyn RemoteSyncBackend) -> Result<RemoteSyncSummary, RemoteSyncError>`.
- Produces: `protected_paths` as the project-independent owner of `.qingyu`, `.markra-sync`, staging-prefix constants, and protected-path predicates used by sync, file scanning, and watchers.
- Preserves the existing backend CRUD interface and process-wide run serialization.

- [ ] **Step 1: Write failing scope isolation and manifest tests**

```rust
#[tokio::test]
async fn notes_scope_keeps_manifest_outside_source_root() {
    let notes = tempdir().unwrap();
    let state = tempdir().unwrap();
    write(notes.path().join("note.md"), b"hello");
    let scope = RemoteSyncScope::notes(notes.path(), state.path(), "notes-webdav-manifest.json");

    execute_remote_sync(&scope, &fake_backend("notes")).await.unwrap();

    assert!(state.path().join("notes-webdav-manifest.json").exists());
    assert!(!notes.path().join(".qingyu").exists());
}

#[tokio::test]
async fn settings_scope_can_only_see_settings_json() {
    let app_data = tempdir().unwrap();
    write(app_data.path().join("settings.json"), b"{}");
    write(app_data.path().join("sync-config.json"), b"secret");
    let scope = RemoteSyncScope::portable_settings(app_data.path(), app_data.path().join("sync-state"));

    let entries = scan_local_scope(&scope).unwrap();
    assert_eq!(entries.keys().collect::<Vec<_>>(), vec!["settings.json"]);
}
```

- [ ] **Step 2: Run engine tests and verify hard-coded source-root state fails**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync::engine`

Expected: FAIL because manifest/conflict/staging paths are derived from the source root.

- [ ] **Step 3: Introduce explicit scope policy and thread it through the algorithm**

```rust
pub(crate) enum RemoteSyncIncludePolicy {
    Notes,
    ExactFile(&'static str),
}

pub(crate) enum RemoteContentValidator {
    None,
    PortableSettings,
}

pub(crate) struct RemoteSyncScope {
    pub source_root: PathBuf,
    pub state_root: PathBuf,
    pub manifest_name: String,
    pub conflict_root: PathBuf,
    pub staging_root: PathBuf,
    pub include: RemoteSyncIncludePolicy,
    pub validator: RemoteContentValidator,
    pub local_identity: Option<String>,
}
```

Replace every manifest, conflict, durable staging, scan, and publication path derivation in `engine.rs` with a method on `RemoteSyncScope`. Notes filtering delegates to the existing protected-path and ignore-rule functions. Exact-file filtering returns only `settings.json`. Validate every remote relative path before joining it to either source or state. For cross-filesystem publication, copy only validated state-staged bytes into a scope-generated protected temp beside the destination, fsync it, atomically publish it on the destination filesystem, and remove every failure residue. Keep comparison, deletion, conflict naming, checkpointing, identity enforcement, and no-replace publication in one algorithm.

Move `QINGYU_CONTROL_DIR`, `LEGACY_SYNC_DIR`, `SYNC_MUTATION_STAGING_PREFIX`, `is_qingyu_control_directory_name`, `path_contains_qingyu_control_directory`, and `is_protected_sync_relative_path` from `project_config.rs` into `protected_paths.rs`. Update sync, Markdown ignore, and watcher imports in this task so Task 5 can delete project configuration without weakening stale-secret exclusions.

- [ ] **Step 4: Run the complete remote-sync unit suite**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync::`

Expected: PASS for both scopes, protected paths, first sync, two-way changes, deletions, conflicts, target identity, checkpoints, recovery, concurrent triggers, no persistent source-root control writes, and cross-filesystem publication without staging residue.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src-tauri/src/remote_sync/scope.rs apps/desktop/src-tauri/src/protected_paths.rs apps/desktop/src-tauri/src/remote_sync.rs apps/desktop/src-tauri/src/remote_sync/engine.rs apps/desktop/src-tauri/src/remote_sync/backend.rs apps/desktop/src-tauri/src/remote_sync/coordinator.rs apps/desktop/src-tauri/src/remote_sync/live_tests.rs apps/desktop/src-tauri/src/lib.rs apps/desktop/src-tauri/src/markdown_files/ignore_rules.rs apps/desktop/src-tauri/src/watcher.rs apps/desktop/src-tauri/src/watcher/directory.rs
git commit -m "refactor: make remote sync scope explicit"
```

### Task 3: Synchronize Notes and Portable Settings Namespaces

**Files:**
- Create: `apps/desktop/src-tauri/src/remote_sync/settings_scope.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/service.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/s3_backend.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/backend.rs`
- Modify: `apps/desktop/src-tauri/src/app_settings.rs`
- Modify: `apps/desktop/src-tauri/src/sync_config/status.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/live_tests.rs`

**Interfaces:**
- Consumes: `RemoteSyncScope` from Task 2 and `SyncSnapshot` from Task 1.
- Produces: `run_application_sync(app, notes_root, snapshot, trigger) -> Result<SyncRunResult, SyncRunError>`.
- Produces remote prefixes `<remoteRoot>/notes` and `<remoteRoot>/app` for both providers.
- Produces settings events after a valid downloaded publication.

- [ ] **Step 1: Write failing namespace and settings-publication tests**

```rust
#[test]
fn provider_paths_use_disjoint_notes_and_app_namespaces() {
    let root = ValidRemoteRoot::parse("qingyu/team").unwrap();
    assert_eq!(root.notes_prefix(), "qingyu/team/notes");
    assert_eq!(root.app_prefix(), "qingyu/team/app");
}

#[tokio::test]
async fn invalid_remote_settings_are_quarantined_without_publication() {
    let fixture = settings_sync_fixture(br#"{"appearanceMode":7}"#);
    let before = read(fixture.app_data.join("settings.json"));
    let error = fixture.run().await.unwrap_err();

    assert_eq!(read(fixture.app_data.join("settings.json")), before);
    assert!(fixture.conflicts().iter().any(|path| path.ends_with("settings.remote-invalid.json")));
    assert_eq!(error.code(), "remote-settings-invalid");
}

#[tokio::test]
async fn valid_remote_settings_publish_atomically_and_emit_change_events() {
    let fixture = settings_sync_fixture(valid_portable_settings());
    fixture.run().await.unwrap();
    assert_eq!(fixture.reloaded_store_count(), 1);
    assert_eq!(fixture.settings_changed_event_count(), 1);
}

#[tokio::test]
async fn existing_valid_remote_settings_win_on_a_settings_scope_first_sync() {
    let fixture = first_settings_sync_fixture(local_defaults(), remote_portable_preferences());
    fixture.run().await.unwrap();
    assert_eq!(fixture.active_settings(), remote_portable_preferences());
    assert!(fixture.manifest().contains_key("settings.json"));
}
```

- [ ] **Step 2: Run service tests and verify red state**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync::service`

Expected: FAIL because the service currently has only a project source and one provider prefix.

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync::settings_scope`

Expected: FAIL because the settings scope does not exist.

- [ ] **Step 3: Implement namespace construction and the two-scope service**

```rust
pub(crate) async fn run_application_sync<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    notes_root: PathBuf,
    snapshot: SyncSnapshot,
    trigger: SyncTrigger,
) -> Result<SyncRunResult, SyncRunError> {
    let root_identity = canonical_root_identity(&notes_root)?;
    let notes = RemoteSyncScope::notes(
        notes_root,
        snapshot.state_root.clone(),
        manifest_name("notes", snapshot.config.provider),
        root_identity,
    );
    let settings = RemoteSyncScope::portable_settings(
        app.path().app_data_dir()?,
        snapshot.state_root.clone(),
        manifest_name("settings", snapshot.config.provider),
    );
    let notes_summary = execute_remote_sync(&notes, &backend_for(&snapshot, "notes")?).await?;
    let settings_summary = execute_remote_sync(&settings, &backend_for(&snapshot, "app")?).await?;
    reload_settings_if_changed(app, &settings_summary)?;
    Ok(SyncRunResult::combined(snapshot.revision, trigger, notes_summary, settings_summary))
}
```

Parse `remoteRoot` once into normalized segments and append only literal `notes` or `app`. Settings validation calls the same portable-settings schema used by `app_settings.rs`; publish through the existing no-replace atomic path, call Tauri Store `reload()`, then emit existing appearance/language/editor/file-ignore/export change events for changed groups. For a settings scope with no manifest baseline and an existing valid remote `settings.json`, apply the remote file and checkpoint it even though local defaults already exist. After a baseline exists, a concurrent settings conflict keeps local active and stores the remote copy under `sync-state/conflicts/`. Invalid content is also quarantined with a sanitized timestamped name; never echo file contents in the error.

Before a notes run, compare the persisted manifest's `localIdentity` with the canonical current root identity. If different, atomically archive/remove the notes manifest and begin without deletion propagation. Do not reset the settings manifest.

- [ ] **Step 4: Run provider, settings, and live-fixture unit tests**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync::`

Expected: PASS for WebDAV/S3 namespace parity, settings allowlist, valid reload, invalid quarantine, conflict quarantine, root identity reset, and combined safe summaries.

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml sync_config::`

Expected: PASS for immutable snapshots, revision checks, and safe status publication.

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml app_settings::`

Expected: PASS for portable settings validation and reload behavior.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src-tauri/src/remote_sync/settings_scope.rs apps/desktop/src-tauri/src/remote_sync/service.rs apps/desktop/src-tauri/src/remote_sync/s3_backend.rs apps/desktop/src-tauri/src/remote_sync/backend.rs apps/desktop/src-tauri/src/app_settings.rs apps/desktop/src-tauri/src/sync_config/status.rs apps/desktop/src-tauri/src/remote_sync/live_tests.rs
git commit -m "feat: sync notes and portable settings namespaces"
```

### Task 4: Replace Project Sync Hooks with an Application Coordinator

**Files:**
- Create: `packages/app/src/hooks/useSyncConfig.ts`
- Create: `packages/app/src/hooks/useSyncConfig.test.tsx`
- Create: `packages/app/src/hooks/useAppSyncCoordinator.ts`
- Create: `packages/app/src/hooks/useAppSyncCoordinator.test.tsx`
- Create: `packages/app/src/lib/sync-config-events.ts`
- Create: `packages/app/src/lib/sync-config-events.test.ts`
- Create: `apps/desktop/src-tauri/src/workspace_membership.rs`
- Modify: `packages/app/src/lib/sync.ts`
- Modify: `packages/app/src/runtime/index.ts`
- Modify: `packages/app/src/App.tsx`
- Modify: `packages/app/src/App.test.tsx`
- Modify: `apps/desktop/src-tauri/src/sync_config.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync.rs`
- Modify: `apps/desktop/src-tauri/src/desktop_runtime.rs`
- Modify: `apps/desktop/src-tauri/src/mobile_runtime.rs`
- Modify: `apps/desktop/src-tauri/src/builder_boundary_tests.rs`
- Modify: `apps/desktop/src/runtime/desktop.ts`
- Modify: `apps/desktop/src/runtime/mobile.ts`
- Modify: `apps/desktop/src/runtime/tauri/managed-workspace.ts`
- Modify: `apps/desktop/src/runtime/tauri/sync-config/shared.ts`
- Modify: `apps/desktop/src/runtime/tauri/sync-config.test.ts`

**Interfaces:**
- Consumes: `primaryWorkspace.root` from the workspace/onboarding plan and `AppSyncConfigRuntime` from Task 1.
- Produces: `useSyncConfig(): SyncConfigState` and `useAppSyncCoordinator(input): AppSyncCoordinator`.
- Produces: `notifyDocumentSaved(documentPath)` that checks canonical membership in the immutable primary root before a `save` trigger.
- Produces real native `sync_application` and `test_sync_connection` commands; the desktop/mobile adapters no longer fail closed with an unavailable stub.
- Moves canonical document membership out of the legacy project-config module into the application workspace runtime so Task 5 can delete project configuration without weakening symlink or traversal containment.

- [ ] **Step 1: Write failing coordinator isolation tests**

```tsx
it("runs launch sync only for the valid primary workspace", async () => {
  const { result } = renderHook(() => useAppSyncCoordinator(input({ primaryRoot: "/Notes" })));
  await waitFor(() => expect(mockSync).toHaveBeenCalledWith(expect.objectContaining({
    notesRoot: "/Notes",
    trigger: "app-launch"
  })));
  expect(result.current.status?.completionState).toBe("succeeded");
});

it("does not sync an external document save", async () => {
  const { result } = renderHook(() => useAppSyncCoordinator(input({ primaryRoot: "/Notes" })));
  mockSync.mockClear();
  await act(() => result.current.notifyDocumentSaved("/External/file.md"));
  expect(mockSync).not.toHaveBeenCalled();
});

it("pauses automatic triggers until the modified settings session exits", async () => {
  const { result } = renderHook(() => useAppSyncCoordinator(input({ primaryRoot: "/Notes" })));
  emitSyncEditing({ active: true, sessionId: "s1", revision: "r1" });
  await act(() => result.current.notifyDocumentSaved("/Notes/file.md"));
  expect(mockSync).not.toHaveBeenCalled();
  emitSyncApplyRequested({ sessionId: "s1", revision: "r2", token: "apply-1" });
  await waitFor(() => expect(mockSync).toHaveBeenCalledWith(expect.objectContaining({
    applyToken: "apply-1",
    revision: "r2",
    trigger: "settings-exit"
  })));
});
```

- [ ] **Step 2: Run hook tests and verify red state**

Run: `pnpm --filter @markra/app exec vitest run src/hooks/useSyncConfig.test.tsx src/hooks/useAppSyncCoordinator.test.tsx src/lib/sync-config-events.test.ts --environment jsdom --globals`

Expected: FAIL because app-level hooks and events do not exist.

- [ ] **Step 3: Port the proven coalescing logic with app-level keys**

```ts
export type SyncRunRequest = {
  applyToken?: string;
  notesRoot: string;
  revision: string;
  trigger: SyncTrigger;
};

export type AppSyncCoordinatorInput = {
  configDocument: SyncConfigDocument | null;
  onFilesChanged?: (primaryRoot: string) => Promise<unknown> | unknown;
  primaryRoot: string | null;
  reloadConfig: () => Promise<SyncConfigLoadResult | null>;
  translate: (key: I18nKey) => string;
};
```

Use the existing module-level promise tail and shared-run map, keyed by `notesRoot + revision + applyToken`, to preserve process serialization and trigger coalescing. Rename trigger `project-open` to `app-launch`; add a generation when `primaryRoot` changes. Every async run captures the root and revision before entering the tail. Membership checks use the existing native canonical membership command against the configured primary root. Remove project-config handling from `App.tsx`; keep the old hook/event modules only until Sync Settings migrates in Task 5.

Use `primaryIntegrationRoot` rather than the persisted root directly so external editor windows never own launch, interval, or save synchronization. Move the existing native membership implementation to `workspace_membership.rs`, expose it as `workspace.isDocumentInRoot`, and retain canonical path, regular-file, symlink, and traversal checks.

Wire `syncApplication()` to a real `sync_application` Tauri command. The command validates trigger/apply-token combinations, rejects automatic triggers while the native editing registry is active, resolves `ready_snapshot_at_app_data` for the expected revision before creating a backend, canonicalizes the requested primary notes root, and calls `run_application_sync`. A settings-exit run consumes `begin_sync_apply`/`wait_sync_apply`/`complete_sync_apply` so concurrent windows execute a token exactly once and receive the same result. Wire `testSyncConnection()` to an application-level command using the same immutable snapshot without exposing credentials.

- [ ] **Step 4: Run hook and App integration tests**

Run: `pnpm --filter @markra/app exec vitest run src/hooks/useSyncConfig.test.tsx src/hooks/useAppSyncCoordinator.test.tsx src/lib/sync-config-events.test.ts src/App.test.tsx --environment jsdom --globals`

Expected: PASS for launch, manual, save, interval, settings-exit, root switch, immutable in-flight root, failure notifications, external saves, config reload, and event coalescing.

Run: `pnpm --filter @markra/desktop exec vitest run src/runtime/tauri/sync-config.test.ts --environment jsdom --globals`

Expected: PASS for exact native request mapping and no unavailable sync/test-connection stubs.

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml sync_config`

Expected: PASS for trigger validation, editing barriers, revision freshness, apply-token exactly-once execution, safe connection tests, and desktop/mobile command registration.

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml workspace_membership`

Expected: PASS for inside/outside documents, traversal, directories, and symlink escapes.

- [ ] **Step 5: Commit**

```bash
git add packages/app/src/hooks/useSyncConfig.ts packages/app/src/hooks/useSyncConfig.test.tsx packages/app/src/hooks/useAppSyncCoordinator.ts packages/app/src/hooks/useAppSyncCoordinator.test.tsx packages/app/src/lib/sync-config-events.ts packages/app/src/lib/sync-config-events.test.ts packages/app/src/lib/sync.ts packages/app/src/runtime/index.ts packages/app/src/App.tsx packages/app/src/App.test.tsx apps/desktop/src-tauri/src/workspace_membership.rs apps/desktop/src-tauri/src/sync_config.rs apps/desktop/src-tauri/src/remote_sync.rs apps/desktop/src-tauri/src/desktop_runtime.rs apps/desktop/src-tauri/src/mobile_runtime.rs apps/desktop/src-tauri/src/builder_boundary_tests.rs apps/desktop/src/runtime/desktop.ts apps/desktop/src/runtime/mobile.ts apps/desktop/src/runtime/tauri/managed-workspace.ts apps/desktop/src/runtime/tauri/sync-config/shared.ts apps/desktop/src/runtime/tauri/sync-config.test.ts
git commit -m "refactor: coordinate sync at application scope"
```

### Task 5: Rebuild Sync Settings Around the Primary Workspace

**Files:**
- Modify: `packages/app/src/components/settings/SyncSettings.tsx`
- Modify: `packages/app/src/components/settings/SyncSettings.test.tsx`
- Modify: `packages/app/src/hooks/useSettingsWindowState.ts`
- Modify: `packages/app/src/hooks/useSettingsWindowState.test.ts`
- Modify: `packages/app/src/components/SettingsWindow.tsx`
- Modify: `packages/app/src/components/SettingsWindow.test.tsx`
- Modify: `packages/app/src/components/compact/CompactSettingsDetail.tsx`
- Modify: `packages/app/src/components/compact/CompactSettingsDetail.test.tsx`
- Modify: `packages/app/src/components/compact/types.ts`
- Modify: `apps/desktop/src-tauri/src/mcp/tools/sync.rs`
- Modify: `apps/desktop/src-tauri/src/mcp/tests.rs`
- Modify: `apps/desktop/src-tauri/src/builder_boundary_tests.rs`
- Modify: `apps/desktop/src-tauri/src/mobile_platform_config_tests.rs`
- Modify: `apps/desktop/src/runtime/mobile.test.ts`
- Modify: `packages/shared/src/i18n/locales/en.ts`
- Modify: `packages/shared/src/i18n/locales/zh-CN.ts`
- Delete: `packages/app/src/hooks/useProjectConfig.ts`
- Delete: `packages/app/src/hooks/useProjectConfig.test.tsx`
- Delete: `packages/app/src/hooks/useProjectSyncCoordinator.ts`
- Delete: `packages/app/src/hooks/useProjectSyncCoordinator.test.tsx`
- Delete: `packages/app/src/lib/project-config-events.ts`
- Delete: `packages/app/src/lib/project-config-events.test.ts`
- Delete: `packages/app/src/lib/project-config.ts`
- Delete: `packages/app/src/lib/project-config.test.ts`
- Delete: `apps/desktop/src/runtime/tauri/project-config.ts`
- Delete: `apps/desktop/src/runtime/tauri/project-config/shared.ts`
- Delete: `apps/desktop/src/runtime/tauri/project-config.test.ts`
- Delete: `apps/desktop/src-tauri/src/project_config.rs`
- Delete: `apps/desktop/src-tauri/src/project_config/`

**Interfaces:**
- Consumes: `SyncConfigState`, `AppSyncCoordinator`, and `primaryWorkspace.root`.
- Produces: `SyncSettingsProps` without candidate/project-root variants.
- Guarantees: every field writes immediately, while the coordinator begins automatic sync only at the session exit boundary.

- [ ] **Step 1: Write failing application-level settings tests**

```tsx
it("shows the primary notes directory as a read-only target", () => {
  render(<SyncSettings primaryRoot="/Notes" {...loadedSyncProps()} />);
  expect(screen.getByText("/Notes")).toBeVisible();
  expect(screen.queryByRole("textbox", { name: /笔记目录/u })).not.toBeInTheDocument();
});

it("persists each field immediately and applies once on category leave", async () => {
  render(<SettingsWindow initialCategory="sync" {...settingsProps()} />);
  await user.type(screen.getByLabelText("远端根目录"), "qingyu");
  expect(mockPatch).toHaveBeenCalled();
  expect(mockSync).not.toHaveBeenCalled();
  await user.click(screen.getByRole("button", { name: "外观" }));
  expect(mockRequestApply).toHaveBeenCalledTimes(1);
});

it("disables sync actions when no valid primary workspace exists", () => {
  render(<SyncSettings primaryRoot={null} {...loadedSyncProps()} />);
  expect(screen.getByRole("button", { name: /立即同步/u })).toBeDisabled();
  expect(screen.getByText(/请先选择主笔记目录/u)).toBeVisible();
});
```

- [ ] **Step 2: Run desktop and Compact settings tests to verify red state**

Run: `pnpm --filter @markra/app exec vitest run src/components/settings/SyncSettings.test.tsx src/hooks/useSettingsWindowState.test.ts src/components/SettingsWindow.test.tsx src/components/compact/CompactSettingsDetail.test.tsx --environment jsdom --globals`

Expected: FAIL because settings still binds to the current project root.

- [ ] **Step 3: Implement the single settings surface and remove legacy code**

```ts
export type SyncSettingsProps = {
  configDocument: SyncConfigDocument | null;
  loadResult: SyncConfigLoadResult | null;
  primaryRoot: string | null;
  saving: boolean;
  status: SyncStatus | null;
  syncRunning: boolean;
  testing: boolean;
  translate: SettingsTranslate;
  onEnable: () => Promise<unknown>;
  onPatch: (patch: SyncConfigPatch) => Promise<unknown>;
  onReset: () => Promise<unknown>;
  onRunSync: () => Promise<unknown>;
  onTestConnection: () => Promise<SyncConnectionTestResult | undefined>;
};
```

Remove “reveal project config” because the app-data file is not a note-folder artifact. Explain that `remoteRoot` creates `notes/` and `app/settings.json`. Keep the plaintext credential warning. Desktop Settings and Compact Settings consume the same runtime object. Update MCP sync tools/tests, desktop/mobile registration boundary tests, Compact controller types, and runtime import-boundary tests to the app-level sync API. Delete the obsolete project hooks, events, Rust/TypeScript config modules, adapters, and command registrations only after `rg 'project_config|projectConfig|ProjectConfig|useProject'` confirms that no production caller remains. Protected-path constants remain in `protected_paths.rs`.

- [ ] **Step 4: Run settings, full frontend, and Rust gates**

Run: `pnpm --filter @markra/app exec vitest run src/components/settings/SyncSettings.test.tsx src/hooks/useSettingsWindowState.test.ts src/components/SettingsWindow.test.tsx src/components/compact/CompactSettingsDetail.test.tsx --environment jsdom --globals`

Expected: PASS.

Run: `pnpm test`

Expected: PASS.

Run: `pnpm typecheck:test`

Expected: PASS.

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml`

Expected: PASS with no default-parallel test failures.

- [ ] **Step 5: Run the real MinIO contract without storing credentials**

Run with endpoint/access/secret/bucket supplied only as process environment variables: `pnpm test:s3-sync:live`

Expected: PASS for upload/download/delete/conflict across `notes/`, asset and arbitrary binary content, `app/settings.json`, protected-path exclusion, and cleanup of the test prefix. The command and test output must not print credentials or signed URLs.

- [ ] **Step 6: Commit**

```bash
git add -A packages/app/src/components/settings/SyncSettings.tsx packages/app/src/components/settings/SyncSettings.test.tsx packages/app/src/hooks/useSettingsWindowState.ts packages/app/src/hooks/useSettingsWindowState.test.ts packages/app/src/components/SettingsWindow.tsx packages/app/src/components/SettingsWindow.test.tsx packages/app/src/components/compact/CompactSettingsDetail.tsx packages/app/src/components/compact/CompactSettingsDetail.test.tsx packages/app/src/components/compact/types.ts packages/app/src/hooks/useProjectConfig.ts packages/app/src/hooks/useProjectConfig.test.tsx packages/app/src/hooks/useProjectSyncCoordinator.ts packages/app/src/hooks/useProjectSyncCoordinator.test.tsx packages/app/src/lib/project-config-events.ts packages/app/src/lib/project-config-events.test.ts packages/app/src/lib/project-config.ts packages/app/src/lib/project-config.test.ts apps/desktop/src-tauri/src/mcp/tools/sync.rs apps/desktop/src-tauri/src/mcp/tests.rs apps/desktop/src-tauri/src/builder_boundary_tests.rs apps/desktop/src-tauri/src/mobile_platform_config_tests.rs apps/desktop/src-tauri/src/project_config.rs apps/desktop/src-tauri/src/project_config apps/desktop/src/runtime/tauri/project-config.ts apps/desktop/src/runtime/tauri/project-config apps/desktop/src/runtime/tauri/project-config.test.ts apps/desktop/src/runtime/mobile.test.ts packages/shared/src/i18n/locales/en.ts packages/shared/src/i18n/locales/zh-CN.ts
git commit -m "feat: expose application-level sync settings"
```
