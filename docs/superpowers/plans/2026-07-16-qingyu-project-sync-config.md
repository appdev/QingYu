# QingYu Project-Scoped Sync Configuration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make each opened folder own a plaintext `.qingyu/config.json` that controls its WebDAV or S3 folder sync, while unconfigured folders and single files never sync and project assets travel through the same folder engine as notes.

**Architecture:** Add a native project-configuration service with revisioned, atomic, no-follow file access; expose it through a typed runtime boundary; and make a folder-only frontend coordinator consume validated immutable snapshots. Move manifests and non-secret status to `.qingyu/sync/`, suspend automatic triggers during Settings edits, and replace independent remote image upload with context-aware local `assets/` persistence.

**Tech Stack:** Tauri v2, Rust 2021, React 19, TypeScript 6, Vitest, Testing Library, reqwest, serde/serde_json, sha2, cap-std/cap-fs-ext, Tailwind CSS, pnpm workspace.

## Global Constraints

- Use `pnpm` for every JavaScript and frontend command; keep `pnpm-lock.yaml` and do not add another package-manager lockfile.
- Keep remote project sync desktop-only. The web runtime must expose project sync as unavailable and use standalone local asset behavior.
- Do not use the TypeScript `void` keyword or operator.
- Do not migrate, read, move, rename, or delete `.markra-sync`; continue excluding it.
- Do not import existing application-global sync or remote-image settings into a project.
- Store project credentials as plaintext in `.qingyu/config.json`; never copy them through QingYu remote sync or local backup and never log them.
- Treat `.qingyu/` as protected in local scan, remote listing, file tree, search, watcher, AI workspace reads, and backup.
- Use fixed lowercase `assets/`; do not add a configurable project asset-folder field.
- Preserve the existing WebDAV/S3 upload, download, deletion, conflict-copy, checkpoint, identity, and process-wide serialization semantics.
- Do not add AWS SDK, `async-trait`, `pathdiff`, or a new UI framework.
- Keep the broader product rebrand out of this implementation except for the new `.qingyu/` paths and new user-facing feature copy.
- Preserve unrelated user changes and do not stage the existing untracked `bg.png`.

---

## Planned File Structure

New focused units:

- `apps/desktop/src-tauri/src/project_config.rs`: Tauri command facade and safe event emission.
- `apps/desktop/src-tauri/src/project_config/model.rs`: versioned schema, typed patches, readiness, snapshot, and redacted error types.
- `apps/desktop/src-tauri/src/project_config/storage.rs`: canonical-root enforcement, no-follow access, revision checks, atomic replacement, and reset backups.
- `apps/desktop/src-tauri/src/project_config/status.rs`: non-secret `.qingyu/sync/status.json` persistence.
- `packages/app/src/lib/project-config.ts`: shared TypeScript project-config and sync-status contracts.
- `packages/app/src/lib/project-config-events.ts`: root/revision-scoped cross-window events with no secrets.
- `apps/desktop/src/runtime/tauri/project-config.ts`: desktop invoke adapter.
- `packages/app/src/hooks/useProjectConfig.ts`: active editor-window project document loader.
- `packages/app/src/hooks/useProjectSyncSettingsSession.ts`: Settings-window editing session.
- `packages/app/src/hooks/useProjectSyncCoordinator.ts`: project-open/manual/save/interval/settings trigger coordination.
- `packages/app/src/lib/editor-assets.ts`: pure context/origin-to-resource-action policy.

Existing mixed units are narrowed rather than broadly refactored: `useWorkspaceBackupSync.ts` becomes backup-only after project sync moves out, `StorageSettings.tsx` retains settings import/export and the global image filename pattern, and `image-upload.ts` is reduced to local asset naming/persistence helpers before obsolete remote branches are deleted.

---

### Task 1: Define the Native Project Configuration Domain

**Files:**
- Create: `apps/desktop/src-tauri/src/project_config.rs`
- Create: `apps/desktop/src-tauri/src/project_config/model.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`

**Interfaces:**
- Produces: `ProjectConfig`, `ProjectSyncConfig`, `ProjectConfigPatch`, `ProjectConfigReadiness`, `ProjectSyncSnapshot`, and redacted `ProjectConfigIssue`.
- Produces: `PROJECT_CONFIG_VERSION = 1`, `QINGYU_CONTROL_DIR = ".qingyu"`, `QINGYU_SYNC_DIR = "sync"`, and `LEGACY_SYNC_DIR = ".markra-sync"`.
- Consumes: existing `NetworkSettings`, WebDAV request fields, and `S3SyncSettings` field semantics without importing their current storage location.

- [ ] **Step 1: Write failing schema and readiness tests**

Add tests in `project_config/model.rs` for a disabled default, readable-but-incomplete WebDAV, ready WebDAV, ready S3, unsafe remote paths, and secret-free issues:

```rust
#[test]
fn classifies_project_sync_readiness_without_exposing_secrets() {
    let mut config = ProjectConfig::default_enabled();
    assert_eq!(config.sync_readiness(), ProjectConfigReadiness::Incomplete);

    config.sync.webdav.server_url = "https://dav.example.test/files".into();
    config.sync.remote_path = "notes/personal".into();
    config.sync.webdav.password = "must-not-appear".into();

    assert_eq!(config.sync_readiness(), ProjectConfigReadiness::Ready);
    assert!(!format!("{:?}", config.sync_issues()).contains("must-not-appear"));
}

#[test]
fn rejects_remote_parent_segments() {
    let mut config = ProjectConfig::default_enabled();
    config.sync.webdav.server_url = "https://dav.example.test/files".into();
    config.sync.remote_path = "../other-project".into();

    assert_eq!(config.sync_readiness(), ProjectConfigReadiness::Incomplete);
    assert!(config.sync_issues().iter().any(|issue| issue.field == "sync.remotePath"));
}
```

- [ ] **Step 2: Run the focused Rust test and verify RED**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml project_config::model::tests -- --nocapture
```

Expected: FAIL because the `project_config` module and schema do not exist.

- [ ] **Step 3: Implement the versioned model and typed patch union**

Create the model with these exact public shapes:

```rust
pub(crate) const PROJECT_CONFIG_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectConfig {
    pub(crate) version: u32,
    pub(crate) sync: ProjectSyncConfig,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectSyncConfig {
    pub(crate) auto_sync_on_save: bool,
    pub(crate) enabled: bool,
    pub(crate) interval_minutes: u32,
    pub(crate) provider: ProjectSyncProvider,
    pub(crate) remote_path: String,
    pub(crate) s3: ProjectS3Config,
    pub(crate) webdav: ProjectWebDavConfig,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ProjectSyncProvider { S3, Webdav }

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ProjectConfigReadiness { Disabled, Incomplete, Ready }

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "field", content = "value")]
pub(crate) enum ProjectConfigPatch {
    #[serde(rename = "sync.enabled")]
    Enabled(bool),
    #[serde(rename = "sync.provider")]
    Provider(ProjectSyncProvider),
    #[serde(rename = "sync.remotePath")]
    RemotePath(String),
    #[serde(rename = "sync.autoSyncOnSave")]
    AutoSyncOnSave(bool),
    #[serde(rename = "sync.intervalMinutes")]
    IntervalMinutes(u32),
    #[serde(rename = "sync.webdav.serverUrl")]
    WebDavServerUrl(String),
    #[serde(rename = "sync.webdav.username")]
    WebDavUsername(String),
    #[serde(rename = "sync.webdav.password")]
    WebDavPassword(String),
    #[serde(rename = "sync.s3.endpointUrl")]
    S3EndpointUrl(String),
    #[serde(rename = "sync.s3.region")]
    S3Region(String),
    #[serde(rename = "sync.s3.bucket")]
    S3Bucket(String),
    #[serde(rename = "sync.s3.accessKeyId")]
    S3AccessKeyId(String),
    #[serde(rename = "sync.s3.secretAccessKey")]
    S3SecretAccessKey(String),
}
```

Implement `Default` as `enabled = false`, `provider = Webdav`, empty connections, empty remote path, `auto_sync_on_save = false`, and `interval_minutes = 0`. Implement `default_enabled()` by changing only `enabled`. Normalize trimmed endpoint/account/path fields and clamp intervals to `1440`. Keep WebDAV username/password optional; require URL and remote path. Require all S3 fields and remote path.

- [ ] **Step 4: Register the module and run the model tests**

Add `mod project_config;` to `lib.rs`, keep commands unregistered until Task 2, then run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml project_config::model::tests -- --nocapture
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
```

Expected: all project-config model tests PASS and formatting is clean.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src-tauri/src/project_config.rs apps/desktop/src-tauri/src/project_config/model.rs apps/desktop/src-tauri/src/lib.rs
git commit -m "feat(sync): define QingYu project configuration"
```

### Task 2: Add Revisioned, Atomic Project Configuration Storage

**Files:**
- Create: `apps/desktop/src-tauri/src/project_config/storage.rs`
- Modify: `apps/desktop/src-tauri/src/project_config.rs`
- Modify: `apps/desktop/src-tauri/src/project_config/model.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/Cargo.toml`

**Interfaces:**
- Consumes: Task 1 model and constants.
- Produces: Tauri commands `load_project_config`, `enable_project_config`, `patch_project_config`, `reset_project_config`, and `reveal_project_config`.
- Produces: SHA-256 string revisions over exact on-disk bytes; absent files use `revision: null`.
- Produces: `ProjectConfigLoadResponse` tagged as `absent`, `loaded`, `malformed`, or `unsupported`.

- [ ] **Step 1: Write failing storage, revision, reset, and no-follow tests**

Add deterministic temporary-root tests to `storage.rs`:

```rust
#[test]
fn absent_load_does_not_create_control_directory() {
    let root = test_root("absent");
    let loaded = load_from_root(&root).expect("absent project config should load");

    assert!(matches!(loaded, ProjectConfigLoadResponse::Absent { .. }));
    assert!(!root.join(".qingyu").exists());
}

#[test]
fn stale_revision_cannot_overwrite_newer_config() {
    let root = test_root("revision");
    let first = enable_at_root(&root, None).expect("config should be enabled");
    let stale = first.revision.clone();
    patch_at_root(&root, &first.revision, ProjectConfigPatch::RemotePath("notes".into()))
        .expect("first patch should pass");

    let error = patch_at_root(&root, &stale, ProjectConfigPatch::RemotePath("stale".into()))
        .expect_err("stale patch must fail");
    assert_eq!(error.code, "revision-conflict");
}

#[cfg(unix)]
#[test]
fn rejects_symlinked_control_directory() {
    let root = test_root("symlink");
    let outside = test_root("outside");
    std::os::unix::fs::symlink(&outside, root.join(".qingyu")).unwrap();
    assert!(load_from_root(&root).is_err());
}
```

Add fault injection around final replacement and assert the previous config bytes remain unchanged. Add malformed/future-version tests that preserve exact bytes, plus reset tests that create one `config.invalid-<timestamp>-<sequence>.json` before defaults.

- [ ] **Step 2: Run storage tests and verify RED**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml project_config::storage::tests -- --nocapture
```

Expected: FAIL because storage and commands are not implemented.

- [ ] **Step 3: Implement fixed-path no-follow storage and atomic replacement**

Implement only fixed paths derived from a canonical directory root:

```rust
pub(crate) const QINGYU_CONTROL_DIR: &str = ".qingyu";
pub(crate) const QINGYU_CONFIG_FILE: &str = "config.json";
pub(crate) const QINGYU_SYNC_DIR: &str = "sync";
pub(crate) const LEGACY_SYNC_DIR: &str = ".markra-sync";

pub(crate) fn config_path(root: &Path) -> PathBuf {
    root.join(QINGYU_CONTROL_DIR).join(QINGYU_CONFIG_FILE)
}

fn revision(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}
```

Open the canonical root with `cap_std::fs::Dir`, open `.qingyu` without following links, and create temporary siblings with `create_new`. Write all bytes, call `sync_all`, then replace the target. On Windows add `Win32_Storage_FileSystem` to the existing `windows-sys` feature list and use `ReplaceFileW` for an existing destination; on Unix use same-directory `rename` and sync the parent directory. Never implement replacement as delete-then-rename.

Return typed revision conflicts and safe path errors. Never format `ProjectConfig`, patches, or raw JSON into an error.

- [ ] **Step 4: Expose and register Tauri commands**

Add facade commands with camel-case request fields:

```rust
#[tauri::command]
pub(crate) fn load_project_config(root_path: String) -> Result<ProjectConfigLoadResponse, String>;

#[tauri::command]
pub(crate) fn enable_project_config(
    app: tauri::AppHandle,
    root_path: String,
    expected_revision: Option<String>,
) -> Result<ProjectConfigDocument, String>;

#[tauri::command]
pub(crate) fn patch_project_config(
    app: tauri::AppHandle,
    request: ProjectConfigPatchRequest,
) -> Result<ProjectConfigDocument, String>;

#[tauri::command]
pub(crate) fn reset_project_config(
    app: tauri::AppHandle,
    request: ResetProjectConfigRequest,
) -> Result<ProjectConfigDocument, String>;
```

After a successful write, emit `qingyu://project-config-changed` with only `{ rootPath, revision }`. Implement `reveal_project_config` by reusing the existing platform file-manager command logic so the file is selected/revealed without introducing a shell-interpolated command.

Register all five commands in `lib.rs`.

- [ ] **Step 5: Run focused tests and commit**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml project_config:: -- --nocapture
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
```

Expected: all project-config tests PASS, including fault injection and symlink cases.

```bash
git add apps/desktop/src-tauri/src/project_config.rs apps/desktop/src-tauri/src/project_config/model.rs apps/desktop/src-tauri/src/project_config/storage.rs apps/desktop/src-tauri/src/lib.rs apps/desktop/src-tauri/Cargo.toml apps/desktop/src-tauri/Cargo.lock
git commit -m "feat(sync): persist project configuration safely"
```

### Task 3: Add the Typed TypeScript Runtime Boundary

**Files:**
- Create: `packages/app/src/lib/project-config.ts`
- Create: `packages/app/src/lib/project-config.test.ts`
- Create: `packages/app/src/lib/project-config-events.ts`
- Create: `packages/app/src/lib/project-config-events.test.ts`
- Create: `apps/desktop/src/runtime/tauri/project-config.ts`
- Create: `apps/desktop/src/runtime/tauri/project-config.test.ts`
- Modify: `packages/app/src/runtime/index.ts`
- Modify: `apps/desktop/src/runtime/index.ts`
- Modify: `apps/web/src/runtime/index.ts`
- Modify: `apps/web/src/runtime/index.test.ts`

**Interfaces:**
- Consumes: Task 2 Tauri commands and `qingyu://project-config-changed` event.
- Produces: shared `QingYuProjectConfig`, `ProjectConfigPatch`, `ProjectConfigLoadResult`, `ProjectSyncStatus`, and `AppProjectConfigRuntime`.
- Produces: feature flag `projectSync: boolean`; desktop is `true`, web/default runtime is `false`.

- [ ] **Step 1: Write failing domain, adapter, and unavailable-runtime tests**

Use a string revision consistently:

```ts
it("preserves both provider branches in a loaded project config", () => {
  const result = normalizeProjectConfigLoadResult({
    status: "loaded",
    projectRoot: "/notes",
    revision: "abc123",
    readiness: "ready",
    issues: [],
    config: projectConfigFixture({ provider: "s3" })
  });

  expect(result.status).toBe("loaded");
  if (result.status !== "loaded") throw new Error("expected loaded result");
  expect(result.config.sync.webdav.serverUrl).toBe("https://dav.example.test");
  expect(result.config.sync.s3.bucket).toBe("notes");
});
```

In the desktop adapter test, expect `patch_project_config` to receive `{ request: { rootPath, expectedRevision, patch } }`. In the web runtime test, expect `features.projectSync` to be `false` and project-config calls to reject with the standard unsupported-feature error.

- [ ] **Step 2: Run tests and verify RED**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/lib/project-config.test.ts src/lib/project-config-events.test.ts
pnpm --filter @markra/desktop exec vitest run src/runtime/tauri/project-config.test.ts
pnpm --filter @markra/web exec vitest run src/runtime/index.test.ts
```

Expected: FAIL because the contracts and runtime namespace do not exist.

- [ ] **Step 3: Implement exact shared contracts**

Define the patch union and load result without duplicating native readiness validation:

```ts
export type ProjectSyncProvider = "s3" | "webdav";
export type ProjectConfigReadiness = "disabled" | "incomplete" | "ready";
export type ProjectSyncTrigger = "interval" | "manual" | "project-open" | "save" | "settings-exit";

export type ProjectSyncSummary = {
  bytesDownloaded: number;
  bytesUploaded: number;
  conflictFiles: number;
  downloadedFiles: number;
  scannedFiles: number;
  skippedFiles: number;
  uploadedFiles: number;
};

export type ProjectSyncSafeError = {
  code: string;
  httpStatus: number | null;
  operation: string;
  provider: ProjectSyncProvider;
  relativePath: string | null;
};

export type ProjectSyncStatus = {
  completionState: "attempting" | "failed" | "succeeded";
  error: ProjectSyncSafeError | null;
  lastAttemptAt: string;
  lastSuccessfulSyncAt: string | null;
  lastTrigger: ProjectSyncTrigger;
  provider: ProjectSyncProvider;
  summary: ProjectSyncSummary | null;
  version: 1;
};

export type ProjectSyncRunResult = {
  projectRoot: string;
  provider: ProjectSyncProvider;
  revision: string;
  summary: ProjectSyncSummary;
  trigger: ProjectSyncTrigger;
};

export type QingYuProjectConfig = {
  version: 1;
  sync: {
    autoSyncOnSave: boolean;
    enabled: boolean;
    intervalMinutes: number;
    provider: ProjectSyncProvider;
    remotePath: string;
    s3: {
      accessKeyId: string;
      bucket: string;
      endpointUrl: string;
      region: string;
      secretAccessKey: string;
    };
    webdav: { password: string; serverUrl: string; username: string };
  };
};

export type ProjectConfigPatch =
  | { field: "sync.enabled"; value: boolean }
  | { field: "sync.provider"; value: ProjectSyncProvider }
  | { field: "sync.remotePath"; value: string }
  | { field: "sync.autoSyncOnSave"; value: boolean }
  | { field: "sync.intervalMinutes"; value: number }
  | { field: "sync.webdav.serverUrl" | "sync.webdav.username" | "sync.webdav.password"; value: string }
  | { field: "sync.s3.endpointUrl" | "sync.s3.region" | "sync.s3.bucket" | "sync.s3.accessKeyId" | "sync.s3.secretAccessKey"; value: string };
```

Use a tagged `ProjectConfigLoadResult` with `absent`, `loaded`, `malformed`, and `unsupported` branches. Loaded contains `projectRoot`, `revision`, `readiness`, `issues`, and `config`. Invalid branches contain only safe error fields and never raw JSON.

- [ ] **Step 4: Add runtime methods and root/revision-scoped events**

Add:

```ts
export type AppProjectConfigRuntime = {
  enable(input: { expectedRevision: string | null; projectRoot: string }): Promise<ProjectConfigDocument>;
  load(input: { projectRoot: string }): Promise<ProjectConfigLoadResult>;
  loadStatus(input: { projectRoot: string }): Promise<ProjectSyncStatus | null>;
  patch(input: { expectedRevision: string; patch: ProjectConfigPatch; projectRoot: string }): Promise<ProjectConfigDocument>;
  reset(input: { confirmed: true; expectedRevision: string | null; projectRoot: string }): Promise<ProjectConfigDocument>;
  reveal(input: { projectRoot: string }): Promise<unknown>;
};
```

Add event helpers for `qingyu://project-config-changed`, `qingyu://project-sync-editing`, `qingyu://project-sync-apply-requested`, `qingyu://project-sync-run-requested`, `qingyu://project-sync-run-completed`, and `qingyu://project-sync-status-changed`. Every payload includes canonical `projectRoot`; configuration payloads include `revision`; editing/apply/run payloads include `sessionId`; run request/completion payloads share a `requestId` and identify the safe trigger source; no payload contains configuration values.

Wire the desktop adapter and `desktopRuntime.projectConfig`. Add unavailable methods to the default and web runtimes and the `projectSync` feature flag.

- [ ] **Step 5: Run focused tests and commit**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/lib/project-config.test.ts src/lib/project-config-events.test.ts
pnpm --filter @markra/desktop exec vitest run src/runtime/tauri/project-config.test.ts
pnpm --filter @markra/web exec vitest run src/runtime/index.test.ts
pnpm --filter @markra/app build
```

Expected: all tests and the app typecheck PASS.

```bash
git add packages/app/src/lib/project-config.ts packages/app/src/lib/project-config.test.ts packages/app/src/lib/project-config-events.ts packages/app/src/lib/project-config-events.test.ts packages/app/src/runtime/index.ts apps/desktop/src/runtime/tauri/project-config.ts apps/desktop/src/runtime/tauri/project-config.test.ts apps/desktop/src/runtime/index.ts apps/web/src/runtime/index.ts apps/web/src/runtime/index.test.ts
git commit -m "feat(sync): expose project configuration runtime"
```

### Task 4: Protect `.qingyu` Across Workspace Operations

**Files:**
- Modify: `apps/desktop/src-tauri/src/markdown_files/ignore_rules.rs`
- Modify: `apps/desktop/src-tauri/src/markdown_files/tree.rs`
- Modify: `apps/desktop/src-tauri/src/markdown_files/search.rs`
- Modify: `apps/desktop/src-tauri/src/watcher.rs`
- Modify: `apps/desktop/src-tauri/src/watcher/directory.rs`
- Modify: `apps/desktop/src-tauri/src/backup.rs`
- Modify: `packages/app/src/App.ai.test.tsx`

**Interfaces:**
- Consumes: Task 1 `QINGYU_CONTROL_DIR` and `LEGACY_SYNC_DIR` constants.
- Produces: one authoritative built-in ignore decision used by tree, search, watcher, and AI-known workspace files.
- Preserves: `.markra-sync` exclusion without migration.

- [ ] **Step 1: Extend failing ignore, tree, search, watcher, AI, and backup tests**

Add `.qingyu/config.json` and `.qingyu/sync/status.json` fixtures beside legacy metadata:

```rust
#[test]
fn built_in_control_directories_remain_authoritative() {
    let root = test_root("builtins");
    let rules = MarkdownIgnoreRules::for_root(&root, Some("!.qingyu/\n!.markra-sync/\n"));

    assert!(rules.ignores(&root.join(".qingyu/config.json"), false));
    assert!(rules.ignores(&root.join(".markra-sync/manifest.json"), false));
}
```

In backup tests assert neither control directory exists in the target while `notes.md` and `assets/image.png` do. In `App.ai.test.tsx`, assert `.qingyu/config.json` is absent from the AI workspace-file list even when global ignore negation attempts to include it.

- [ ] **Step 2: Run focused tests and verify RED**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml markdown_files::ignore_rules::tests -- --nocapture
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml backup::tests -- --nocapture
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml watcher::tests -- --nocapture
pnpm --filter @markra/app exec vitest run src/App.ai.test.tsx
```

Expected: new `.qingyu` assertions FAIL because only existing legacy product/tool directories are excluded.

- [ ] **Step 3: Centralize the protected directory names**

Expose a crate-visible helper from `project_config`:

```rust
pub(crate) fn is_qingyu_control_directory_name(name: &OsStr) -> bool {
    name == OsStr::new(QINGYU_CONTROL_DIR) || name == OsStr::new(LEGACY_SYNC_DIR)
}
```

Call it from `markdown_files/ignore_rules.rs` before the existing fixed build/dependency names. Add `.qingyu` to backup's fixed ignored directories. Do not add an exception that `.markraignore` can negate.

The tree, search, and watcher implementations should continue consuming `MarkdownIgnoreRules`; update only tests unless a direct fixed-name list is found. AI remains protected because it can only read files present in the known workspace tree.

- [ ] **Step 4: Run every focused path test**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml markdown_files::tree::tests -- --nocapture
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml markdown_files::search::tests -- --nocapture
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml watcher:: -- --nocapture
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml backup::tests -- --nocapture
pnpm --filter @markra/app exec vitest run src/App.ai.test.tsx
```

Expected: all protected-path tests PASS and legacy exclusions remain covered.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src-tauri/src/project_config.rs apps/desktop/src-tauri/src/markdown_files/ignore_rules.rs apps/desktop/src-tauri/src/markdown_files/tree.rs apps/desktop/src-tauri/src/markdown_files/search.rs apps/desktop/src-tauri/src/watcher.rs apps/desktop/src-tauri/src/watcher/directory.rs apps/desktop/src-tauri/src/backup.rs packages/app/src/App.ai.test.tsx
git commit -m "feat(sync): protect QingYu project metadata"
```

### Task 5: Make the Native Sync Engine Project-Only

**Files:**
- Create: `apps/desktop/src-tauri/src/project_config/status.rs`
- Modify: `apps/desktop/src-tauri/src/project_config.rs`
- Modify: `apps/desktop/src-tauri/src/project_config/model.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/engine.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/s3_backend.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/live_tests.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `packages/app/src/lib/project-config.ts`
- Modify: `apps/desktop/src/runtime/tauri/project-config.ts`
- Modify: `apps/desktop/src/runtime/tauri/project-config.test.ts`

**Interfaces:**
- Consumes: Task 2 fresh-load/revision service and Task 4 protected-directory helper.
- Produces: Tauri command `sync_project_folder({ rootPath, expectedRevision, trigger, network })` and a safe `ProjectSyncRunResult`.
- Produces: `.qingyu/sync/webdav-manifest.json`, `.qingyu/sync/s3-manifest.json`, and atomic `.qingyu/sync/status.json`.
- Removes from the public Tauri handler: arbitrary `sourcePath` plus caller-supplied provider credentials.

- [ ] **Step 1: Rewrite failing engine and desktop adapter tests around a project root**

Change the sync adapter test to assert no secrets are sent from TypeScript:

```ts
await syncProjectFolder({ projectRoot: "/vault", revision: "rev-1", trigger: "manual" });

expect(mockedInvoke).toHaveBeenCalledWith("sync_project_folder", {
  request: {
    expectedRevision: "rev-1",
    projectRoot: "/vault",
      network: undefined,
      trigger: "manual"
  }
});
```

In `engine.rs`, change the file-root test from “uses parent” to an error:

```rust
#[test]
fn rejects_a_file_as_project_sync_root() {
    let root = test_root("file-root");
    let note = root.join("note.md");
    fs::write(&note, "# Note").unwrap();

    assert_eq!(
        sync_source_root(&note).unwrap_err(),
        "Project sync source must be a folder"
    );
}
```

Update manifest assertions to `.qingyu/sync/s3-manifest.json` and add local/remote `.qingyu` and `.markra-sync` fixtures that remain untouched.

- [ ] **Step 2: Run focused tests and verify RED**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync::engine::tests -- --nocapture
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync::tests -- --nocapture
pnpm --filter @markra/desktop exec vitest run src/runtime/tauri/project-config.test.ts
```

Expected: FAIL because manifests still use `.markra-sync`, file roots are accepted, and the project-only command does not exist.

- [ ] **Step 3: Implement immutable snapshot loading and project-only dispatch**

Add a command request that contains no connection fields:

```rust
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncProjectFolderRequest {
    expected_revision: String,
    network: Option<NetworkSettings>,
    project_root: String,
    trigger: ProjectSyncTrigger,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ProjectSyncTrigger {
    Interval,
    Manual,
    ProjectOpen,
    Save,
    SettingsExit,
}

#[tauri::command]
pub(crate) async fn sync_project_folder(
    request: SyncProjectFolderRequest,
) -> Result<ProjectSyncRunResult, String> {
    let snapshot = project_config::ready_snapshot(
        &request.project_root,
        Some(&request.expected_revision),
    )?;
    execute_snapshot_sync(snapshot, request.network.as_ref()).await
}
```

`ready_snapshot` must fresh-read exact bytes, verify revision, require `enabled` and `Ready`, canonicalize a directory root, and return one active provider branch. Construct `WebDavBackend` or `S3Backend` from that immutable snapshot. Remove `sync_markdown_folder` from the invoke handler after the TypeScript adapter uses the new command.

- [ ] **Step 4: Move manifests, protect remote paths, and persist safe status**

Use:

```rust
fn manifest_path(source_root: &Path, manifest_file_name: &str) -> PathBuf {
    source_root
        .join(QINGYU_CONTROL_DIR)
        .join(QINGYU_SYNC_DIR)
        .join(manifest_file_name)
}

pub(crate) fn is_protected_sync_relative_path(path: &str) -> bool {
    path.split('/').any(|segment| {
        matches!(segment, QINGYU_CONTROL_DIR | LEGACY_SYNC_DIR)
    })
}
```

Apply the helper before adding WebDAV PROPFIND entries or S3 ListObjects entries. Protected remote objects are skipped and never added to the action set, so the engine cannot download or delete them.

Implement `ProjectSyncStatus` with `version`, `provider`, `lastTrigger`, `lastAttemptAt`, `lastSuccessfulSyncAt`, `completionState`, optional existing summary, and optional structured safe error. Write `attempting` before remote work, then `succeeded` or `failed` atomically. Add `load_project_sync_status` and emit `qingyu://project-sync-status-changed` with no secrets. Keep the existing manifest target fingerprint and action-by-action checkpoint writes so root, revision, target, trigger, and completed mutations remain attributable without persisting credentials.

- [ ] **Step 5: Run engine, live-fixture unit tests, and commit**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync:: -- --nocapture
pnpm --filter @markra/desktop exec vitest run src/runtime/tauri/project-config.test.ts
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
```

Expected: all non-ignored remote-sync tests PASS. Live MinIO tests compile with the new `.qingyu/sync` paths but remain ignored without credentials.

```bash
git add apps/desktop/src-tauri/src/project_config.rs apps/desktop/src-tauri/src/project_config/model.rs apps/desktop/src-tauri/src/project_config/status.rs apps/desktop/src-tauri/src/remote_sync.rs apps/desktop/src-tauri/src/remote_sync/engine.rs apps/desktop/src-tauri/src/remote_sync/s3_backend.rs apps/desktop/src-tauri/src/remote_sync/live_tests.rs apps/desktop/src-tauri/src/lib.rs packages/app/src/lib/project-config.ts apps/desktop/src/runtime/tauri/project-config.ts apps/desktop/src/runtime/tauri/project-config.test.ts
git commit -m "feat(sync): run sync from project configuration"
```

### Task 6: Add Read-Only WebDAV and S3 Connection Tests

**Files:**
- Modify: `apps/desktop/src-tauri/src/project_config.rs`
- Modify: `apps/desktop/src-tauri/src/project_config/model.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/s3_backend.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/live_tests.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `packages/app/src/lib/project-config.ts`
- Modify: `packages/app/src/runtime/index.ts`
- Modify: `apps/desktop/src/runtime/tauri/project-config.ts`
- Modify: `apps/desktop/src/runtime/tauri/project-config.test.ts`

**Interfaces:**
- Produces: `test_project_sync_connection({ projectRoot, expectedRevision, network })`.
- Produces: `ProjectConnectionTestResult = { provider, checkedTarget }` with no uploaded test object.
- Consumes: Task 5 immutable ready snapshot and current network proxy settings.

- [ ] **Step 1: Write failing method/query and adapter tests**

Add WebDAV request-builder tests that require `PROPFIND`, `Depth: 0`, and no `MKCOL`. Add an S3 canonical query assertion:

```rust
#[test]
fn connection_test_lists_at_most_one_s3_object() {
    let query = connection_test_query("notes/personal");
    assert_eq!(
        query,
        "list-type=2&max-keys=1&prefix=notes%2Fpersonal%2F"
    );
}
```

Add a desktop test expecting `test_project_sync_connection` to include only root, revision, and normalized network settings.

- [ ] **Step 2: Run tests and verify RED**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml connection_test -- --nocapture
pnpm --filter @markra/desktop exec vitest run src/runtime/tauri/project-config.test.ts -t "connection"
```

Expected: FAIL because the read-only test command and bounded helpers do not exist.

- [ ] **Step 3: Implement provider-specific read-only probes**

Implement:

```rust
#[tauri::command]
pub(crate) async fn test_project_sync_connection(
    request: TestProjectSyncConnectionRequest,
) -> Result<ProjectConnectionTestResult, String> {
    let snapshot = ready_snapshot(&request.project_root, Some(&request.expected_revision))?;
    remote_sync::test_connection(&snapshot, request.network.as_ref()).await
}
```

For WebDAV, issue a bounded Depth-0 `PROPFIND` against the configured collection; on `404`, walk only the configured relative collection parents until an existing collection or base is reached. Treat `200`/`207` as success and never call collection creation. For S3, perform signed ListObjectsV2 with `max-keys=1`; do not call HEAD, PUT, or DELETE.

Construct errors only from provider, method, status, and safe relative target. Do not include auth headers, passwords, access keys, signed queries, or response bodies.

- [ ] **Step 4: Run focused tests and optional live verification**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml connection_test -- --nocapture
pnpm --filter @markra/desktop exec vitest run src/runtime/tauri/project-config.test.ts -t "connection"
```

Expected: all focused tests PASS. If MinIO credentials are configured, also run:

```bash
pnpm test:s3-sync:live
```

Expected: the connection-test live scenario shows identical remote object snapshots before and after the probe.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src-tauri/src/project_config.rs apps/desktop/src-tauri/src/project_config/model.rs apps/desktop/src-tauri/src/remote_sync.rs apps/desktop/src-tauri/src/remote_sync/s3_backend.rs apps/desktop/src-tauri/src/remote_sync/live_tests.rs apps/desktop/src-tauri/src/lib.rs packages/app/src/lib/project-config.ts packages/app/src/runtime/index.ts apps/desktop/src/runtime/tauri/project-config.ts apps/desktop/src/runtime/tauri/project-config.test.ts
git commit -m "feat(sync): test project connections without writes"
```

### Task 7: Introduce an Explicit Folder-Only Project Root

**Files:**
- Modify: `packages/app/src/hooks/useMarkdownFileTree.ts`
- Modify: `packages/app/src/hooks/useMarkdownFileTree.test.tsx`
- Create: `packages/app/src/hooks/useProjectConfig.ts`
- Create: `packages/app/src/hooks/useProjectConfig.test.tsx`
- Modify: `packages/app/src/App.tsx`
- Modify: `packages/app/src/App.test.tsx`
- Modify: `packages/app/src/test/app-harness.tsx`

**Interfaces:**
- Consumes: Task 3 `AppProjectConfigRuntime.load` and change event.
- Produces: `useMarkdownFileTree().projectRoot: string | null`, set only by successful folder open/restore.
- Produces: `useProjectConfig({ projectRoot })` with load result, status, reload, and applied document state.
- Guarantees: a single file always clears `projectRoot`; no helper infers it from a document path.

- [ ] **Step 1: Write failing folder/single-file transition tests**

Add:

```tsx
it("never exposes a project root for a single opened file", async () => {
  const { result } = renderHook(() => useMarkdownFileTree());

  act(() => result.current.setRootFromMarkdownFilePath("/notes/one.md"));

  expect(result.current.sourcePath).toBe("/notes/one.md");
  expect(result.current.projectRoot).toBeNull();
});

it("loads project configuration only for an opened folder", async () => {
  const { result, rerender } = renderHook(
    ({ root }) => useProjectConfig({ projectRoot: root }),
    { initialProps: { root: null as string | null } }
  );
  expect(mockedProjectConfigLoad).not.toHaveBeenCalled();

  rerender({ root: "/notes" });
  await waitFor(() => expect(mockedProjectConfigLoad).toHaveBeenCalledWith({ projectRoot: "/notes" }));
});
```

In `App.test.tsx`, open a standalone Markdown file and assert the project-config runtime and project-sync command are never called.

- [ ] **Step 2: Run tests and verify RED**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/hooks/useMarkdownFileTree.test.tsx src/hooks/useProjectConfig.test.tsx src/App.test.tsx -t "project root|single opened file|project configuration"
```

Expected: FAIL because there is no folder-only project root or project hook.

- [ ] **Step 3: Add explicit root state and project loader**

In `useMarkdownFileTree` add:

```ts
const [projectRoot, setProjectRoot] = useState<string | null>(null);

const setRootFromMarkdownFilePath = useCallback((path: string) => {
  setProjectRoot(null);
  setSourcePath(path);
  setRootName(folderNameFromDocumentPath(path));
}, [abortCurrentFileTreeLoad, cancelPendingFileTreeBatchFlush]);
```

Set `projectRoot` only after `openFolderPath` has successfully loaded the folder; clear it on folder-load failure, standalone file open, blank workspace, and unmount. Return it independently from `sourcePath`.

Implement `useProjectConfig` with a request generation counter so a slow A load cannot install after B opens. Clear the previous result and secret-bearing refs synchronously before loading B. Listen to root/revision-scoped change events and ignore mismatched roots.

- [ ] **Step 4: Wire App without enabling sync triggers yet**

Pass `fileTree.projectRoot` into `useProjectConfig`; remove `setWorkspaceBackupSyncSourcePath(fileTreeSourcePath ?? document.path)` from the sync path. Keep backup's existing source handling until its later split. Do not start open sync in this task.

Run:

```bash
pnpm --filter @markra/app exec vitest run src/hooks/useMarkdownFileTree.test.tsx src/hooks/useProjectConfig.test.tsx src/App.test.tsx -t "project root|single opened file|project configuration"
pnpm --filter @markra/app build
```

Expected: focused tests and build PASS.

- [ ] **Step 5: Commit**

```bash
git add packages/app/src/hooks/useMarkdownFileTree.ts packages/app/src/hooks/useMarkdownFileTree.test.tsx packages/app/src/hooks/useProjectConfig.ts packages/app/src/hooks/useProjectConfig.test.tsx packages/app/src/App.tsx packages/app/src/App.test.tsx packages/app/src/test/app-harness.tsx
git commit -m "feat(sync): model the active project root"
```

### Task 8: Add the Settings Editing Session and Hide Handshake

**Files:**
- Create: `packages/app/src/hooks/useProjectSyncSettingsSession.ts`
- Create: `packages/app/src/hooks/useProjectSyncSettingsSession.test.tsx`
- Modify: `packages/app/src/hooks/useSettingsWindowState.ts`
- Modify: `packages/app/src/hooks/useSettingsWindowState.test.ts`
- Modify: `packages/app/src/components/SettingsWindow.tsx`
- Modify: `packages/app/src/lib/tauri/window.ts`
- Modify: `packages/app/src/runtime/index.ts`
- Modify: `packages/app/src/hooks/useNativeBindings.ts`
- Modify: `packages/app/src/hooks/useNativeBindings.test.tsx`
- Modify: `apps/desktop/src/runtime/tauri/window.ts`
- Modify: `apps/desktop/src/runtime/tauri/window.test.ts`
- Modify: `apps/desktop/src/runtime/index.ts`
- Modify: `apps/desktop/src-tauri/src/windows.rs`

**Interfaces:**
- Consumes: Task 3 project config runtime/events and stored workspace `folderPath` only.
- Produces: `useProjectSyncSettingsSession` with `begin`, `enable`, `patch`, `end`, `runImmediate`, `testConnection`, `reset`, and `reveal`.
- Produces: Settings hide request/complete handshake so every close path ends the session once.

- [ ] **Step 1: Write failing immediate-write/deferred-apply and close-handshake tests**

Add a session test:

```tsx
it("persists every patch but requests apply only when a dirty session ends", async () => {
  const { result } = renderHook(() => useProjectSyncSettingsSession({ projectRoot: "/notes" }));
  await act(() => result.current.begin());
  await act(() => result.current.patch({ field: "sync.remotePath", value: "notes/personal" }));

  expect(mockedProjectConfigPatch).toHaveBeenCalledTimes(1);
  expect(mockedEmitProjectSyncApplyRequested).not.toHaveBeenCalled();

  await act(() => result.current.end("category-leave"));
  expect(mockedEmitProjectSyncApplyRequested).toHaveBeenCalledWith(expect.objectContaining({
    projectRoot: "/notes",
    revision: "rev-2",
    source: "settings-exit"
  }));
});
```

Add native/runtime tests that a close or toggle emits `qingyu://settings-hide-requested`, does not hide immediately, and hides only after `complete_settings_window_hide` or a bounded fallback timeout.

- [ ] **Step 2: Run tests and verify RED**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/hooks/useProjectSyncSettingsSession.test.tsx src/hooks/useSettingsWindowState.test.ts src/hooks/useNativeBindings.test.tsx
pnpm --filter @markra/desktop exec vitest run src/runtime/tauri/window.test.ts
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml windows::tests -- --nocapture
```

Expected: FAIL because settings still writes global sync settings and native close bypasses React.

- [ ] **Step 3: Implement the project settings session**

Use this public state boundary:

```ts
export type ProjectSyncSettingsSession = {
  dirty: boolean;
  loadResult: ProjectConfigLoadResult | null;
  projectRoot: string | null;
  saving: boolean;
  sessionId: string | null;
  testing: boolean;
  begin: () => Promise<unknown>;
  enable: () => Promise<unknown>;
  end: (source: "category-leave" | "window-close") => Promise<unknown>;
  patch: (patch: ProjectConfigPatch) => Promise<unknown>;
  reset: () => Promise<unknown>;
  reveal: () => Promise<unknown>;
  runImmediate: () => Promise<unknown>;
  testConnection: () => Promise<unknown>;
};
```

Serialize field writes through one promise chain; each patch uses the latest returned revision. `begin` emits editing active. `end` waits for pending writes, emits one apply request only when dirty, then emits editing inactive. `runImmediate` waits for pending writes, sends a request with the current revision and a unique request ID, and remains inside the editing suspension while the editor-window coordinator performs the manual run. On a successful accepted completion, treat that revision as the applied baseline and clear `dirty`; a later field change sets it again. An incomplete/disabled/error completion leaves the session editable and never falls back to an older revision. A root change invalidates the session and prevents its pending completion from targeting the next root.

Replace Settings' sync state in `useSettingsWindowState` with this hook. Resolve current project only from stored `workspace.folderPath`; never fall back to `filePath` or `openFilePaths`.

- [ ] **Step 4: Route category and window exits through one idempotent end path**

Wrap category changes:

```ts
const handleCategoryChange = async (next: SettingsCategory) => {
  if (activeSettingsCategory === "sync" && next !== "sync") {
    await syncSession.end("category-leave");
  }
  setActiveCategory(next);
};
```

Call `begin()` whenever Sync becomes the active category, including when Settings first opens directly on Sync. Listen for the native hide request, await `end("window-close")`, then call `completeSettingsWindowHide`. Use the same function for custom window controls, Linux close button, and the settings shortcut. Guard duplicate requests with one in-flight promise. Rust uses a short fallback timeout only to prevent an unresponsive webview from trapping the window; immediate field persistence means data is already on disk.

- [ ] **Step 5: Run tests and commit**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/hooks/useProjectSyncSettingsSession.test.tsx src/hooks/useSettingsWindowState.test.ts src/hooks/useNativeBindings.test.tsx
pnpm --filter @markra/desktop exec vitest run src/runtime/tauri/window.test.ts
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml windows::tests -- --nocapture
```

Expected: all session and hide-handshake tests PASS.

```bash
git add packages/app/src/hooks/useProjectSyncSettingsSession.ts packages/app/src/hooks/useProjectSyncSettingsSession.test.tsx packages/app/src/hooks/useSettingsWindowState.ts packages/app/src/hooks/useSettingsWindowState.test.ts packages/app/src/components/SettingsWindow.tsx packages/app/src/lib/tauri/window.ts packages/app/src/runtime/index.ts packages/app/src/hooks/useNativeBindings.ts packages/app/src/hooks/useNativeBindings.test.tsx apps/desktop/src/runtime/tauri/window.ts apps/desktop/src/runtime/tauri/window.test.ts apps/desktop/src/runtime/index.ts apps/desktop/src-tauri/src/windows.rs
git commit -m "feat(sync): defer project sync until settings exit"
```

### Task 9: Rebuild the Sync Settings Page Around the Active Project

**Files:**
- Modify: `packages/app/src/components/settings/SyncSettings.tsx`
- Modify: `packages/app/src/components/settings/SyncSettings.test.tsx`
- Modify: `packages/app/src/components/SettingsWindow.tsx`
- Modify: `packages/app/src/hooks/useSettingsWindowState.ts`
- Modify: `packages/app/src/hooks/useSettingsWindowState.test.ts`
- Modify: `packages/shared/src/i18n/locales/types.ts`
- Modify: `packages/shared/src/i18n/locales/de.ts`
- Modify: `packages/shared/src/i18n/locales/en.ts`
- Modify: `packages/shared/src/i18n/locales/es.ts`
- Modify: `packages/shared/src/i18n/locales/fr.ts`
- Modify: `packages/shared/src/i18n/locales/it.ts`
- Modify: `packages/shared/src/i18n/locales/ja.ts`
- Modify: `packages/shared/src/i18n/locales/ko.ts`
- Modify: `packages/shared/src/i18n/locales/pt-BR.ts`
- Modify: `packages/shared/src/i18n/locales/ru.ts`
- Modify: `packages/shared/src/i18n/locales/zh-CN.ts`
- Modify: `packages/shared/src/i18n/locales/zh-TW.ts`

**Interfaces:**
- Consumes: Task 8 `ProjectSyncSettingsSession`; does not read or write application-global sync settings.
- Produces: project-aware empty, absent, loaded/disabled, incomplete, ready, malformed, and unsupported views.
- Produces: provider-specific WebDAV/S3 fields, plaintext/Git warning, readiness issues, connection test, manual sync, status summary, reveal, and confirmed reset controls.

- [ ] **Step 1: Replace global-settings tests with project-state tests**

Cover all page states and assert callbacks receive one typed patch per edit:

```tsx
it("shows project enablement instead of global provider controls when config is absent", () => {
  render(<SyncSettings {...baseProps} projectRoot="/notes" loadResult={{ status: "absent", projectRoot: "/notes" }} />);

  expect(screen.getByText("This folder does not have QingYu sync enabled."))
    .toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Enable sync for this folder" }))
    .toBeInTheDocument();
});

it("writes a provider field immediately without starting sync", async () => {
  const onPatch = vi.fn().mockResolvedValue(undefined);
  render(<SyncSettings {...loadedProps} onPatch={onPatch} />);

  await userEvent.clear(screen.getByLabelText("WebDAV server URL"));
  await userEvent.type(screen.getByLabelText("WebDAV server URL"), "https://dav.example.test");

  expect(onPatch).toHaveBeenLastCalledWith({
    field: "sync.webdav.serverUrl",
    value: "https://dav.example.test"
  });
  expect(loadedProps.onRunSync).not.toHaveBeenCalled();
});
```

Also assert secrets use password inputs, issues are rendered from the native result, the reset action requires explicit confirmation, and no password/access-key value appears in status or error text snapshots.

- [ ] **Step 2: Run the focused component tests and verify RED**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/components/settings/SyncSettings.test.tsx src/hooks/useSettingsWindowState.test.ts
```

Expected: FAIL because `SyncSettings` still edits application-global sync settings.

- [ ] **Step 3: Implement the project-scoped page contract**

Use this prop boundary:

```ts
export type SyncSettingsProps = {
  loadResult: ProjectConfigLoadResult | null;
  projectRoot: string | null;
  saving: boolean;
  status: ProjectSyncStatus | null;
  syncRunning: boolean;
  testing: boolean;
  onEnable: () => Promise<unknown>;
  onPatch: (patch: ProjectConfigPatch) => Promise<unknown>;
  onReset: () => Promise<unknown>;
  onRevealConfig: () => Promise<unknown>;
  onRunSync: () => Promise<unknown>;
  onTestConnection: () => Promise<unknown>;
};
```

When `projectRoot` is null, explain that only an opened folder can enable sync. For `absent`, show a single enable action. For `malformed`/`unsupported`, preserve the file, show only safe diagnostics plus reveal/reset. For `loaded`, render `enabled`, provider, remote path, automatic-save and interval policies, both provider forms with the inactive form hidden, readiness issues, connection test, manual sync, and last-attempt/last-success status.

Place a persistent warning beside the enabled control: `.qingyu/config.json` contains plaintext credentials, is excluded by QingYu sync and local backup, but must also be ignored by Git or any third-party tool. Do not claim QingYu edits `.gitignore`.

- [ ] **Step 4: Wire the session and translate every new string**

Pass Task 8 session data/actions from `useSettingsWindowState` through `SettingsWindow`. Add every new key to the locale type and all eleven locale implementations; keep technical tokens `.qingyu/config.json`, `WebDAV`, `S3`, and `assets/` unchanged.

Run:

```bash
pnpm --filter @markra/app exec vitest run src/components/settings/SyncSettings.test.tsx src/hooks/useSettingsWindowState.test.ts
pnpm --filter @markra/shared test
pnpm --filter @markra/app build
```

Expected: component/session tests, locale contract tests, and app build PASS.

- [ ] **Step 5: Commit**

```bash
git add packages/app/src/components/settings/SyncSettings.tsx packages/app/src/components/settings/SyncSettings.test.tsx packages/app/src/components/SettingsWindow.tsx packages/app/src/hooks/useSettingsWindowState.ts packages/app/src/hooks/useSettingsWindowState.test.ts packages/shared/src/i18n/locales
git commit -m "feat(sync): add project-scoped sync settings"
```

### Task 10: Coordinate Project Sync Triggers in the Editor Window

**Files:**
- Create: `packages/app/src/hooks/useProjectSyncCoordinator.ts`
- Create: `packages/app/src/hooks/useProjectSyncCoordinator.test.tsx`
- Modify: `packages/app/src/lib/sync.ts`
- Modify: `packages/app/src/lib/sync.test.ts`
- Modify: `packages/app/src/hooks/useWorkspaceBackupSync.ts`
- Modify: `packages/app/src/hooks/useWorkspaceBackupSync.test.tsx`
- Modify: `packages/app/src/App.tsx`
- Modify: `packages/app/src/App.test.tsx`
- Modify: `packages/app/src/test/app-harness.tsx`

**Interfaces:**
- Consumes: Task 7 folder-only root and current project document, Task 8 editing/apply events, Task 5 `sync_project_folder`, current proxy/network settings, and document-save completion.
- Produces: project-open, settings-exit, manual, save, and interval triggers with one process-level in-flight sync and same-root/revision coalescing.
- Preserves: local backup timing and configuration independently of project remote sync.

- [ ] **Step 1: Write failing trigger, suppression, switch, and notification tests**

Use fake timers and deferred promises to cover:

```tsx
it("holds automatic triggers during editing and runs the final revision once on apply", async () => {
  const firstRun = deferred<ProjectSyncRunResult>();
  mockedSyncProjectFolder.mockReturnValueOnce(firstRun.promise).mockResolvedValue(successResult);
  const { result } = renderHook(() => useProjectSyncCoordinator(readyProject("/A", "rev-1")));

  await emitProjectSyncEditing({ active: true, projectRoot: "/A", sessionId: "settings-1" });
  act(() => result.current.notifyDocumentSaved("/A/note.md"));
  expect(mockedSyncProjectFolder).not.toHaveBeenCalled();

  await emitProjectSyncApplyRequested({ projectRoot: "/A", revision: "rev-2", sessionId: "settings-1", source: "settings-exit" });
  expect(mockedSyncProjectFolder).toHaveBeenCalledTimes(1);
  expect(mockedSyncProjectFolder).toHaveBeenCalledWith(expect.objectContaining({ projectRoot: "/A", revision: "rev-2" }));
});
```

Also test: ready enabled folder opens and syncs once; absent/disabled/incomplete/single-file never auto-sync; save only triggers for a document inside the current project when enabled; zero interval disables the timer; manual sync reports unavailable state; two identical pending triggers coalesce; different revisions serialize; switching A to B clears A timers/queued work and never routes an A completion to B; success remains quiet; automatic failure shows one non-secret notification; manual result is always visible.

- [ ] **Step 2: Run focused tests and verify RED**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/hooks/useProjectSyncCoordinator.test.tsx src/lib/sync.test.ts src/hooks/useWorkspaceBackupSync.test.tsx src/App.test.tsx -t "project sync|backup sync"
```

Expected: FAIL because the existing global hook accepts an arbitrary source path and has no editing-session barrier.

- [ ] **Step 3: Implement coordinator ownership and coalescing**

Expose:

Use Task 3's `ProjectSyncTrigger` and expose:

```ts
export type ProjectSyncCoordinator = {
  notifyDocumentSaved: (documentPath: string) => Promise<unknown>;
  run: (trigger: ProjectSyncTrigger, revision?: string) => Promise<ProjectSyncRunResult | null>;
  running: boolean;
  status: ProjectSyncStatus | null;
};
```

Every run invokes the runtime with only `{ projectRoot, revision, trigger, network }`; native code fresh-loads credentials. Use the key `${projectRoot}\u0000${revision}` to share identical pending work. Serialize different keys through one module-level promise chain so existing Rust process serialization is not burdened by redundant calls. On root change, cancel timers, invalidate queued automatic work, clear secret-bearing project references, and reload status.

Track active Settings sessions by `{ projectRoot, sessionId }`. Suspend project-open/save/interval triggers while the matching project is edited. An apply request schedules exactly one settings-exit run for its revision after editing ends. A run request with `trigger: "manual"` is allowed during editing, returns a matching safe completion event, and is never synthesized by a field write. If a fresh native snapshot reports that config was removed, disabled, malformed, unsupported, incomplete, or revision-conflicted, reload project config, cancel its future timer, and block subsequent automatic triggers until a new ready revision is applied.

- [ ] **Step 4: Separate backup from remote sync and wire App triggers**

Remove remote sync ownership and the global sync-settings listener from `useWorkspaceBackupSync`; retain only local backup source/timer behavior and rename its internal state/functions to backup terminology without changing its public hook filename in this task.

In `App.tsx`, instantiate the project coordinator only from `fileTree.projectRoot` plus `useProjectConfig`. Call `notifyDocumentSaved` only after a successful save. Do not derive a root from `document.path`. Route the Settings manual/apply event, app project-open result, interval timer, and toolbar/manual command through the coordinator. Apply notification policy exactly:

- project-open and settings-exit: show running state and a visible success summary or safe failure;
- save and interval success: update status silently;
- every failure: show one safe warning for the coalesced run;
- manual: show running state and always show the success or failure result;
- disabled/incomplete/no-folder manual action: show a local explanatory result without a network call.

Run:

```bash
pnpm --filter @markra/app exec vitest run src/hooks/useProjectSyncCoordinator.test.tsx src/lib/sync.test.ts src/hooks/useWorkspaceBackupSync.test.tsx src/App.test.tsx -t "project sync|backup sync"
pnpm --filter @markra/app build
```

Expected: focused tests and app build PASS.

- [ ] **Step 5: Commit**

```bash
git add packages/app/src/hooks/useProjectSyncCoordinator.ts packages/app/src/hooks/useProjectSyncCoordinator.test.tsx packages/app/src/lib/sync.ts packages/app/src/lib/sync.test.ts packages/app/src/hooks/useWorkspaceBackupSync.ts packages/app/src/hooks/useWorkspaceBackupSync.test.tsx packages/app/src/App.tsx packages/app/src/App.test.tsx packages/app/src/test/app-harness.tsx
git commit -m "feat(sync): coordinate project sync triggers"
```

### Task 11: Persist Editor Resources by Project and Origin

**Files:**
- Modify: `packages/editor/src/clipboard-images.ts`
- Create: `packages/editor/src/clipboard-images.test.ts`
- Create: `packages/app/src/lib/editor-assets.ts`
- Create: `packages/app/src/lib/editor-assets.test.ts`
- Modify: `packages/app/src/lib/image-upload.ts`
- Modify: `packages/app/src/lib/image-upload.test.ts`
- Modify: `packages/app/src/components/MarkdownPaperSurface.tsx`
- Modify: `packages/app/src/App.tsx`
- Modify: `packages/app/src/App.test.tsx`
- Modify: `packages/app/src/lib/tauri/file.ts`
- Modify: `packages/app/src/runtime/index.ts`
- Modify: `apps/desktop/src/runtime/tauri/file.ts`
- Modify: `apps/desktop/src/runtime/tauri/file.test.ts`
- Modify: `apps/desktop/src-tauri/src/markdown_files/image.rs`
- Modify: `apps/desktop/src-tauri/src/markdown_files/attachment.rs`
- Modify: `apps/desktop/src-tauri/src/markdown_files/path.rs`
- Modify: `apps/web/src/runtime/web/file.ts`
- Modify: `apps/web/src/runtime/web/file.test.ts`

**Interfaces:**
- Consumes: explicit `{ mode: "sync-project", projectRootPath } | { mode: "standalone" }` and resource origin `clipboard | drop | import | remote`.
- Produces: one pure policy choosing `copy-project`, `copy-document`, or `reference`.
- Guarantees: folders whose project config has `sync.enabled: true` use fixed root `assets/` even while connection fields are incomplete; standalone/unconfigured/disabled folders use standalone behavior; standalone clipboard resources use document-adjacent `assets/`; standalone existing local files and remote URLs remain direct references.

- [ ] **Step 1: Write failing policy and path tests**

Add the decision table as data-driven tests:

```ts
it.each([
  ["sync-project", "clipboard", "copy-project"],
  ["sync-project", "drop", "copy-project"],
  ["sync-project", "import", "copy-project"],
  ["sync-project", "remote", "copy-project"],
  ["standalone", "clipboard", "copy-document"],
  ["standalone", "drop", "reference"],
  ["standalone", "import", "reference"],
  ["standalone", "remote", "reference"]
] as const)("uses %s/%s resource policy", (mode, origin, expected) => {
  expect(resolveEditorAssetAction({ mode, origin })).toBe(expected);
});
```

Add native tests that `/project/notes/day.md` produces a file under `/project/assets/` and Markdown URL `../assets/<name>`, while `/other/day.md` with `projectRootPath: /project` is rejected. Test a symlinked `assets/` directory is rejected. Test a source already inside project `assets/` is referenced without a duplicate copy. Test standalone `/notes/day.md` clipboard storage returns `assets/<name>`, while an unsaved standalone document rejects clipboard persistence with a save-first error. Test existing local drop/import paths become canonical encoded `file://` URLs without copying and remote URLs remain unchanged in standalone mode. Cover images and generic attachments.

- [ ] **Step 2: Run focused tests and verify RED**

Run:

```bash
pnpm --filter @markra/editor exec vitest run src/clipboard-images.test.ts
pnpm --filter @markra/app exec vitest run src/lib/editor-assets.test.ts src/lib/image-upload.test.ts src/App.test.tsx -t "asset|image|attachment"
pnpm --filter @markra/desktop exec vitest run src/runtime/tauri/file.test.ts -t "asset|image|attachment"
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml markdown_files::image::tests -- --nocapture
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml markdown_files::attachment::tests -- --nocapture
```

Expected: FAIL because the editor callback does not expose origin and native persistence currently derives storage only from the document parent.

- [ ] **Step 3: Pass resource origin through the editor boundary**

Change the editor callback from an unclassified file list to:

```ts
export type EditorResourceOrigin = "clipboard" | "drop" | "import" | "remote";

export type EditorResourceRequest =
  | { files: File[]; origin: "clipboard" | "drop" | "import" }
  | { origin: "remote"; urls: string[] };
```

Have clipboard paste, drag/drop, file picker/import, and remote Markdown insertion call the same `MarkdownPaperSurface` resource callback with the correct origin. Do not infer origin from filename, MIME type, or URL after the callback.

- [ ] **Step 4: Implement the pure policy and explicit native project boundary**

Define:

```ts
export type EditorAssetContext =
  | { mode: "standalone" }
  | { mode: "sync-project"; projectRootPath: string };

export type EditorAssetAction = "copy-document" | "copy-project" | "reference";
```

Build `sync-project` context only from a readable current-project config whose `sync.enabled` is true; readiness controls network sync, not local project asset placement. For `copy-project`, call file-runtime image/attachment methods with both `documentPath` and `projectRootPath`. Rust canonicalizes both, requires the document to be inside the project, opens/creates the fixed lowercase `assets` directory without following links, writes a collision-safe file atomically, and returns a slash-normalized relative Markdown URL from the document directory. If the source is already a regular file inside root `assets/`, return its relative URL without a second copy. For remote resources, download the bytes through the existing bounded application network path before the same atomic write. Keep URL encoding and filename-pattern behavior in the existing Markdown path helpers.

For `copy-document`, require a saved document and call the standalone native method that creates document-adjacent `assets/`; return a save-first error for an unsaved document. For `reference`, preserve a remote URL or convert the canonical existing local path to an encoded `file://` URL without copying. The web runtime has no project mode and preserves the browser-provided URL for reference actions while following the same standalone decision table.

- [ ] **Step 5: Run resource tests and commit**

Run:

```bash
pnpm --filter @markra/editor exec vitest run src/clipboard-images.test.ts
pnpm --filter @markra/app exec vitest run src/lib/editor-assets.test.ts src/lib/image-upload.test.ts src/App.test.tsx -t "asset|image|attachment"
pnpm --filter @markra/desktop exec vitest run src/runtime/tauri/file.test.ts -t "asset|image|attachment"
pnpm --filter @markra/web exec vitest run src/runtime/web/file.test.ts
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml markdown_files:: -- --nocapture
pnpm --filter @markra/app build
```

Expected: all resource-policy, adapter, native path-safety, and app build checks PASS.

```bash
git add packages/editor/src/clipboard-images.ts packages/editor/src/clipboard-images.test.ts packages/app/src/lib/editor-assets.ts packages/app/src/lib/editor-assets.test.ts packages/app/src/lib/image-upload.ts packages/app/src/lib/image-upload.test.ts packages/app/src/components/MarkdownPaperSurface.tsx packages/app/src/App.tsx packages/app/src/App.test.tsx packages/app/src/lib/tauri/file.ts packages/app/src/runtime/index.ts apps/desktop/src/runtime/tauri/file.ts apps/desktop/src/runtime/tauri/file.test.ts apps/desktop/src-tauri/src/markdown_files/image.rs apps/desktop/src-tauri/src/markdown_files/attachment.rs apps/desktop/src-tauri/src/markdown_files/path.rs apps/web/src/runtime/web/file.ts apps/web/src/runtime/web/file.test.ts
git commit -m "feat(editor): store synced project resources in assets"
```

### Task 12: Remove Global Remote Sync and Independent Image Upload Paths

**Files:**
- Modify: `packages/app/src/lib/settings/app-settings.ts`
- Modify: `packages/app/src/lib/settings/app-settings.test.ts`
- Delete: `packages/app/src/lib/settings/sync-settings.ts`
- Delete: `packages/app/src/lib/settings/sync-settings.test.ts`
- Modify: `packages/app/src/lib/settings/export-settings.ts`
- Modify: `packages/app/src/lib/settings/export-settings.test.ts`
- Modify: `packages/app/src/lib/settings/editor-preferences.test.ts`
- Modify: `packages/app/src/lib/settings/settings-events.ts`
- Modify: `packages/app/src/lib/settings/settings-events.test.ts`
- Delete: `packages/app/src/hooks/useSyncSettings.ts`
- Modify: `packages/app/src/components/settings/StorageSettings.tsx`
- Modify: `packages/app/src/components/settings/StorageSettings.test.tsx`
- Modify: `packages/app/src/components/settings/EditorSettings.tsx`
- Modify: `packages/app/src/components/settings/EditorSettings.test.tsx`
- Delete: `packages/app/src/components/settings/ImageUploadControls.tsx`
- Modify: `packages/app/src/hooks/useSettingsWindowState.ts`
- Modify: `packages/app/src/hooks/useSettingsWindowState.test.ts`
- Modify: `packages/app/src/components/SettingsWindow.tsx`
- Modify: `packages/app/src/lib/diagnostics/diagnostics-report.ts`
- Modify: `packages/app/src/lib/diagnostics/diagnostics-report.test.ts`
- Modify: `packages/app/src/lib/image-upload.ts`
- Modify: `packages/app/src/lib/image-upload.test.ts`
- Modify: `packages/app/src/runtime/index.ts`
- Modify: `packages/app/src/runtime/index.test.ts`
- Modify: `packages/app/src/lib/tauri/file.ts`
- Modify: `apps/desktop/src/runtime/index.ts`
- Modify: `apps/desktop/src/runtime/index.test.ts`
- Modify: `apps/desktop/src/runtime/tauri/file.ts`
- Modify: `apps/desktop/src/runtime/tauri/file.test.ts`
- Modify: `apps/desktop/src/runtime/tauri/invoke.test.ts`
- Modify: `apps/web/src/runtime/index.ts`
- Modify: `apps/web/src/runtime/index.test.ts`
- Modify: `apps/web/src/runtime/web/file.ts`
- Modify: `apps/web/src/runtime/web/file.test.ts`
- Delete: `apps/desktop/src-tauri/src/image_upload.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`

**Interfaces:**
- Removes: active application-global WebDAV/S3 sync settings, global sync change events/hook, PicGo/WebDAV/S3 image-upload settings and commands, and `copyExternalFilesToAssets`.
- Retains: Settings import/export, local backup, the global image filename pattern, project WebDAV/S3 folder sync, and `s3_http.rs` for the folder-sync backend.
- Leaves: obsolete keys already present in the application settings file untouched on disk but ignored by normalization, UI, export, diagnostics, and runtime behavior.

- [ ] **Step 1: Rewrite settings/runtime tests around the reduced public surface**

Assert the normalized editor preference shape retains only filename policy under image settings:

```ts
expect(normalizeEditorPreferences({
  imageUpload: {
    fileNamePattern: "{name}-{timestamp}",
    provider: "s3",
    s3: { accessKeyId: "old-secret" }
  }
}).imageUpload).toEqual({ fileNamePattern: "{name}-{timestamp}" });
```

Assert portable settings export and diagnostics omit global `sync`, provider credentials, PicGo, WebDAV upload, S3 upload, and `copyExternalFilesToAssets`. Assert runtime feature flags and namespaces no longer advertise `s3ImageUpload` or remote image upload methods. Assert Storage still renders import/export and filename pattern. Assert Editor settings no longer renders remote upload/provider/copy-external controls.

- [ ] **Step 2: Run focused tests and verify RED**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/lib/settings/app-settings.test.ts src/lib/settings/export-settings.test.ts src/lib/settings/editor-preferences.test.ts src/lib/settings/settings-events.test.ts src/components/settings/StorageSettings.test.tsx src/components/settings/EditorSettings.test.tsx src/lib/diagnostics/diagnostics-report.test.ts src/lib/image-upload.test.ts src/runtime/index.test.ts
pnpm --filter @markra/desktop exec vitest run src/runtime/index.test.ts
pnpm --filter @markra/web exec vitest run src/runtime/index.test.ts
```

Expected: FAIL because old global sync and remote image-upload surfaces still exist.

- [ ] **Step 3: Remove active global settings paths while preserving filename policy**

Delete global sync reads/writes/events and remove it from portable export/import. Reduce `imageUpload` to `{ fileNamePattern }`, keeping its current default and token expansion so Task 11 local asset filenames remain stable. Ignore any extra old JSON fields during normalization; do not rewrite the user's application settings file merely to delete them.

Keep `StorageSettings` import/export and filename pattern controls. Remove `ImageUploadControls`, remote provider forms, and `copyExternalFilesToAssets` from `EditorSettings` and settings state. Remove old provider data from diagnostics rather than redacting and retaining unused branches.

- [ ] **Step 4: Remove runtime/native upload commands and obsolete tests**

Delete PicGo/WebDAV/S3 remote-upload wrappers, types, commands, feature flags, and command registration. Delete `image_upload.rs`, but keep `s3_http.rs` and its tests because Task 5 S3 folder sync consumes its signing/client primitives. Remove tests whose sole purpose was proving the deleted upload feature; keep/rewrite local filename and asset persistence tests.

Run:

```bash
pnpm --filter @markra/app exec vitest run src/lib/settings/app-settings.test.ts src/lib/settings/export-settings.test.ts src/lib/settings/editor-preferences.test.ts src/lib/settings/settings-events.test.ts src/components/settings/StorageSettings.test.tsx src/components/settings/EditorSettings.test.tsx src/lib/diagnostics/diagnostics-report.test.ts src/lib/image-upload.test.ts src/runtime/index.test.ts
pnpm --filter @markra/desktop exec vitest run src/runtime/index.test.ts
pnpm --filter @markra/web exec vitest run src/runtime/index.test.ts
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml s3_http::tests -- --nocapture
pnpm --filter @markra/app build
```

Expected: reduced settings/runtime tests, S3 signing tests, and app build PASS; source search finds no active obsolete surface:

```bash
if rg -n "useSyncSettings|ImageUploadControls|s3ImageUpload|upload(WebDav|PicGo|S3)Image|upload_(webdav|picgo|s3)_image|copyExternalFilesTo(Storage|Assets)" packages apps --glob '!**/dist/**' --glob '!**/target/**'; then
  exit 1
fi
```

- [ ] **Step 5: Commit**

```bash
git add -A packages/app/src/lib/settings packages/app/src/hooks/useSyncSettings.ts packages/app/src/components/settings packages/app/src/hooks/useSettingsWindowState.ts packages/app/src/hooks/useSettingsWindowState.test.ts packages/app/src/components/SettingsWindow.tsx packages/app/src/lib/diagnostics packages/app/src/lib/image-upload.ts packages/app/src/lib/image-upload.test.ts packages/app/src/lib/tauri/file.ts packages/app/src/runtime apps/desktop/src/runtime apps/web/src/runtime apps/desktop/src-tauri/src/image_upload.rs apps/desktop/src-tauri/src/lib.rs
git commit -m "refactor(sync): remove global remote upload settings"
```

### Task 13: Verify End-to-End Project Isolation and Document the Contract

**Files:**
- Modify: `apps/desktop/src-tauri/src/remote_sync/live_tests.rs`
- Modify: `packages/app/src/App.test.tsx`
- Modify: `packages/app/src/components/settings/SyncSettings.test.tsx`
- Modify: `README.md`
- Modify: `docs/privacy.md`
- Create: `docs/testing/qingyu-project-sync-desktop-acceptance.md`

**Interfaces:**
- Verifies: independent A/B configuration and credentials, absent-folder default-off behavior, single-file non-sync behavior, settings-exit apply semantics, fixed `assets/`, control-directory exclusion, provider switching, and no secret leakage.
- Documents: plaintext credential risk, QingYu ignore boundaries, project-local provider ownership, no `.markra-sync` migration, and manual Git ignore responsibility.

- [ ] **Step 1: Add failing cross-project and acceptance-level regression tests**

Add a two-root integration fixture:

```rust
#[tokio::test]
async fn project_a_and_b_use_only_their_own_snapshots() {
    let a = configured_project("A", ProjectSyncProvider::Webdav);
    let b = configured_project("B", ProjectSyncProvider::S3);

    sync_configured_project(&a).await.expect("A sync should pass");
    sync_configured_project(&b).await.expect("B sync should pass");

    assert_remote_contains_only(&a.remote, &["A.md", "assets/a.png"]);
    assert_remote_contains_only(&b.remote, &["B.md", "assets/b.png"]);
    assert_no_remote_control_paths(&a.remote);
    assert_no_remote_control_paths(&b.remote);
}
```

Use existing mock backends for default tests and keep real MinIO/WebDAV variants ignored or environment-gated. In `App.test.tsx`, cover A-to-B switch and single-file transitions. In `SyncSettings.test.tsx`, verify the exact close/apply behavior and warning copy.

- [ ] **Step 2: Run integration tests and verify RED if any boundary is missing**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml project_a_and_b_use_only_their_own_snapshots -- --nocapture
pnpm --filter @markra/app exec vitest run src/App.test.tsx src/components/settings/SyncSettings.test.tsx -t "project A|project B|single file|settings exit|plaintext"
```

Expected: PASS only when project isolation, no-folder behavior, and the final Settings apply boundary are fully wired. If a new assertion fails, fix the owning earlier task's focused unit before continuing.

- [ ] **Step 3: Update user and privacy documentation**

Document this exact behavior in `README.md` and `docs/privacy.md`:

- sync is off unless the opened folder has `.qingyu/config.json` with `sync.enabled: true` and a ready provider;
- every folder owns its WebDAV/S3 endpoint, account, credentials, remote path, and trigger policy;
- credentials are plaintext and QingYu excludes `.qingyu/` from its own sync, local backup, tree, search, watcher, and AI workspace reads, but cannot protect Git or third-party tools;
- standalone files never remote-sync; existing local resources remain references and pasted clipboard resources use adjacent `assets/`;
- enabled projects store all imported resources in root `assets/` and the folder sync engine transfers them with all other project content;
- `.markra-sync` remains ignored and is not migrated.

Create the desktop acceptance checklist with separate A WebDAV and B S3 folders, a no-config C folder, a standalone file, settings edit/leave/close cases, network failures, connection-test remote snapshots, resource paste/drop/import cases, and visible status/notification expectations. Never include real endpoints or credentials in the document.

- [ ] **Step 4: Run the complete verification matrix**

Run from the repository root:

```bash
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
pnpm test
pnpm typecheck:test
pnpm build
git diff --check
```

Expected: every command exits 0. When the configured real MinIO test server is available, also run:

```bash
pnpm test:s3-sync:live
```

Expected: the live suite passes and confirms `.qingyu/` and `.markra-sync/` never appear remotely. When desktop packaging dependencies are available, run:

```bash
pnpm tauri build --debug
```

Expected: the debug desktop bundle builds successfully.

- [ ] **Step 5: Inspect the final diff for secrets and unintended branding scope**

Run:

```bash
git status --short
git diff --stat
git diff -- . ':(exclude)pnpm-lock.yaml' | rg -n "password|secretAccessKey|accessKeyId|markra|qingyu"
```

Expected: review output contains only schema/UI identifiers and redacted test fixtures; no real credential value, no staged `bg.png`, and no broad product rename outside the approved project-sync surface.

- [ ] **Step 6: Commit documentation and final acceptance coverage**

```bash
git add apps/desktop/src-tauri/src/remote_sync/live_tests.rs packages/app/src/App.test.tsx packages/app/src/components/settings/SyncSettings.test.tsx README.md docs/privacy.md docs/testing/qingyu-project-sync-desktop-acceptance.md
git commit -m "docs(sync): document project-scoped sync"
```
