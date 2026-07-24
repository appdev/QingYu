# Complete Application Proxy Removal Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Completely remove QingYu's application-owned proxy settings and request plumbing while preserving WebDAV, S3, updater, and web-image networking through default clients.

**Architecture:** Remove the feature from the outside in so every commit remains testable: first delete the desktop settings surface and capability flag, then remove portable settings and diagnostics, then simplify TypeScript runtime requests, and finally remove the Rust proxy contract and client customization. The remaining networking features keep their provider-specific configuration, security checks, and error handling.

**Tech Stack:** React, TypeScript, Vitest, Tauri v2, Rust, reqwest, pnpm workspace.

## Global Constraints

- Execute on the current `main` branch as previously requested; do not create a worktree.
- Preserve WebDAV and S3 project-folder sync, update checks, web-image downloads, private-network guards, redirect protection, credentials, and endpoint configuration.
- Do not add a migration, deprecated type, hidden compatibility field, or fallback reader for old proxy settings.
- Use `pnpm` for JavaScript and frontend workflows.
- Do not use the TypeScript `void` keyword or operator.
- Preserve the intentional untracked `bg.png` file.
- Do not push any branch unless the user explicitly requests it.

---

### Task 1: Remove the desktop Network settings surface and capability flag

**Files:**
- Delete: `packages/app/src/components/settings/NetworkSettings.tsx`
- Delete: `packages/app/src/components/settings/NetworkSettings.test.tsx`
- Modify: `packages/app/src/components/SettingsSections.tsx`
- Modify: `packages/app/src/components/SettingsShell.tsx`
- Modify: `packages/app/src/components/SettingsShell.test.tsx`
- Modify: `packages/app/src/components/SettingsWindow.tsx`
- Modify: `packages/app/src/hooks/useSettingsWindowState.ts`
- Modify: `packages/app/src/runtime/index.ts`
- Modify: `apps/desktop/src/runtime/index.ts`
- Modify: `apps/web/src/runtime/index.ts`
- Modify: `apps/web/src/runtime/index.test.ts`
- Modify: `packages/app/src/App.test.tsx`
- Modify: `packages/app/src/lib/diagnostics/diagnostics-report.ts`
- Modify: `packages/app/src/lib/diagnostics/diagnostics-report.test.ts`
- Modify: `packages/shared/src/i18n/locales/types.ts`
- Modify: `packages/shared/src/i18n/locales/en.ts`
- Modify: `packages/shared/src/i18n/locales/zh-CN.ts`
- Modify: `packages/shared/src/i18n/locales/zh-TW.ts`
- Modify: `packages/shared/src/i18n/locales/ja.ts`
- Modify: `packages/shared/src/i18n/locales/ko.ts`
- Modify: `packages/shared/src/i18n/locales/fr.ts`
- Modify: `packages/shared/src/i18n/locales/de.ts`
- Modify: `packages/shared/src/i18n/locales/es.ts`
- Modify: `packages/shared/src/i18n/locales/pt-BR.ts`
- Modify: `packages/shared/src/i18n/locales/it.ts`
- Modify: `packages/shared/src/i18n/locales/ru.ts`

**Interfaces:**
- Consumes: existing `SettingsCategory`, `AppFeatureRuntime`, and `useSettingsWindowState()` return object.
- Produces: a settings category union with no `"network"`; an `AppFeatureRuntime` with no `networkProxy`; no network settings component or translation keys.

- [ ] **Step 1: Change the settings tests to require the Network category to be absent**

Replace the positive category test in `SettingsShell.test.tsx` with:

```tsx
it("does not expose a network settings category", () => {
  renderSettingsSidebar();

  expect(screen.queryByRole("button", { name: "Network" })).not.toBeInTheDocument();
});
```

Delete the test that renders `SettingsContent activeCategory="network"`. In the independent settings-window route expectation in `App.test.tsx`, remove `"Network"` from the exact sidebar label array.

- [ ] **Step 2: Run the focused UI tests and verify the new absence assertion fails**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/components/SettingsShell.test.tsx src/App.test.tsx
```

Expected: FAIL because the sidebar still renders the Network category.

- [ ] **Step 3: Delete the UI, state, capability flag, and translations**

Make `SettingsCategory` contain only the retained categories:

```ts
export type SettingsCategory =
  | "general"
  | "sync"
  | "logs"
  | "appearance"
  | "view"
  | "editor"
  | "templates"
  | "keyboardShortcuts"
  | "export";
```

Delete the Network entry from `settingsCategories`, remove the `NetworkSettings` export/import/render branch, and remove `networkSettings` plus `handleUpdateNetworkSettings` from `useSettingsWindowState()`. Delete the network-settings load effect, save callback, and `setNetworkSettings(settings.network)` import side effect.

Remove `networkProxy` from the feature contract:

```ts
export type AppFeatureRuntime = {
  export: boolean;
  nativeWindowChrome: boolean;
  pandoc: boolean;
  projectSync: boolean;
  updater: boolean;
};
```

Remove the property from the default, desktop, and web runtime objects and from feature fixtures. Remove the `Network proxy support` line from diagnostics, but leave the stored `networkSettings` diagnostic input for Task 2 so this task does not yet delete the shared settings model.

Delete all of these locale keys from every locale and `types.ts`:

```text
settings.categories.network
settings.sections.networkProxy
settings.network.proxyTitle
settings.network.proxyDescription
settings.network.proxyEnabled
settings.network.proxyEnabledDescription
settings.network.proxyUrl
settings.network.proxyUrlDescription
settings.network.bypassLocal
settings.network.bypassLocalDescription
```

- [ ] **Step 4: Run the focused UI, diagnostics, web-runtime, and type tests**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/components/SettingsShell.test.tsx src/App.test.tsx src/lib/diagnostics/diagnostics-report.test.ts
pnpm --filter @markra/web exec vitest run src/runtime/index.test.ts
pnpm typecheck:test
```

Expected: all commands PASS; the settings route has no Network button and all runtime feature objects satisfy the smaller contract.

- [ ] **Step 5: Commit the UI removal**

```bash
git add packages/app/src/components packages/app/src/hooks/useSettingsWindowState.ts packages/app/src/runtime/index.ts packages/app/src/App.test.tsx packages/app/src/lib/diagnostics apps/desktop/src/runtime/index.ts apps/web/src/runtime packages/shared/src/i18n/locales
git commit -m "refactor: remove network settings surface"
```

---

### Task 2: Remove proxy data from portable settings and diagnostics

**Files:**
- Modify: `packages/app/src/lib/settings/app-settings.ts`
- Modify: `packages/app/src/lib/settings/app-settings.test.ts`
- Modify: `packages/app/src/lib/diagnostics/diagnostics-report.ts`
- Modify: `packages/app/src/lib/diagnostics/diagnostics-report.test.ts`
- Modify: `packages/app/src/hooks/useSettingsWindowState.ts`
- Modify: `packages/app/src/App.test.tsx`

**Interfaces:**
- Consumes: `PortableStoredAppSettings`, `exportStoredAppSettings()`, `importStoredAppSettings()`, and `generateDiagnosticsReport()`.
- Produces: portable settings with exactly eight retained top-level setting keys and diagnostics with no proxy section; the internal stored proxy accessors remain temporarily for Task 3 runtime callers.

- [ ] **Step 1: Change portable settings and diagnostics tests to reject proxy output**

Update both exact portable-key assertions in `app-settings.test.ts` to:

```ts
expect(Object.keys(settings).sort()).toEqual([
  "appearanceMode",
  "customThemeCss",
  "darkTheme",
  "editorPreferences",
  "exportSettings",
  "fileIgnoreSettings",
  "language",
  "lightTheme"
]);
```

Keep a `network` object in an import fixture as unknown input and assert that the returned normalized settings do not expose it:

```ts
expect(importedSettings).not.toHaveProperty("network");
```

In `diagnostics-report.test.ts`, add:

```ts
expect(report).not.toContain("Network proxy");
expect(report).not.toContain("Bypass local addresses");
expect(report).not.toContain("### Network");
```

- [ ] **Step 2: Run the focused tests and verify they fail for the old portable and diagnostic output**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/lib/settings/app-settings.test.ts src/lib/diagnostics/diagnostics-report.test.ts
```

Expected: FAIL because portable settings still include `network` and diagnostics still render the Network section.

- [ ] **Step 3: Remove network from the portable schema and diagnostic input**

Change the portable type to:

```ts
export type PortableStoredAppSettings = {
  appearanceMode: AppAppearanceMode;
  customThemeCss: CustomThemeCssValues;
  darkTheme: DarkEditorTheme;
  editorPreferences: EditorPreferences;
  exportSettings: ExportSettings;
  fileIgnoreSettings: FileIgnoreSettings;
  language: AppLanguage;
  lightTheme: LightEditorTheme;
};
```

Remove network reads, normalization, writes, and returned fields only from `normalizePortableStoredAppSettings()`, `readPortableStoredAppSettings()`, and `writePortableStoredAppSettings()`. Keep `networkKey`, `getStoredNetworkSettings()`, and `saveStoredNetworkSettings()` until Task 3 disconnects all runtime consumers.

Remove `NetworkSettings` and `networkSettings` from `DiagnosticsReportInput`, destructuring, and report output. Update all direct call fixtures accordingly. `useSettingsWindowState()` must continue applying only retained imported settings.

- [ ] **Step 4: Run the focused tests and application test typecheck**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/lib/settings/app-settings.test.ts src/lib/diagnostics/diagnostics-report.test.ts src/App.test.tsx
pnpm --filter @markra/app typecheck:test
```

Expected: PASS; exported/imported settings and diagnostics have no network proxy data.

- [ ] **Step 5: Commit the settings-schema cleanup**

```bash
git add packages/app/src/lib/settings/app-settings.ts packages/app/src/lib/settings/app-settings.test.ts packages/app/src/lib/diagnostics packages/app/src/hooks/useSettingsWindowState.ts packages/app/src/App.test.tsx
git commit -m "refactor: remove proxy settings data"
```

---

### Task 3: Remove TypeScript proxy loading and request fields

**Files:**
- Delete: `packages/app/src/lib/settings/network-settings.ts`
- Delete: `packages/app/src/lib/settings/network-settings.test.ts`
- Delete: `apps/desktop/src/runtime/tauri/network.ts`
- Modify: `packages/app/src/lib/settings/app-settings.ts`
- Modify: `packages/app/src/lib/settings/app-settings.test.ts`
- Modify: `packages/app/src/lib/sync.ts`
- Modify: `packages/app/src/lib/sync.test.ts`
- Modify: `packages/app/src/lib/project-config.ts`
- Modify: `packages/app/src/test/app-harness.tsx`
- Modify: `packages/app/src/App.test.tsx`
- Modify: `apps/desktop/src/runtime/tauri/updater.ts`
- Modify: `apps/desktop/src/runtime/tauri/updater.test.ts`
- Modify: `apps/desktop/src/runtime/tauri/web-resource.ts`
- Modify: `apps/desktop/src/runtime/tauri/web-resource.test.ts`
- Modify: `apps/desktop/src/runtime/tauri/file.test.ts`
- Modify: `apps/desktop/src/runtime/tauri/project-config.ts`
- Modify: `apps/desktop/src/runtime/tauri/project-config.test.ts`
- Modify: `apps/desktop/src/runtime/index.ts`

**Interfaces:**
- Consumes: `ProjectFolderSyncRequest`, `AppProjectConfigRuntime`, updater `check()`, and native `invokeNative()`.
- Produces: sync and connection-test requests with no network field; updater checks once with no proxy option; web-image requests containing only `url`; no TypeScript proxy model or store API.

- [ ] **Step 1: Rewrite retained request tests to require proxy-free contracts**

In `sync.test.ts`, require the canonical request to pass through unchanged:

```ts
const sync = vi.fn(async () => expectedResult);

await runProjectFolderSync(request, { sync });

expect(sync).toHaveBeenCalledWith(request);
```

In `updater.test.ts`, replace configured/local-proxy fallback cases with:

```ts
await checkNativeAppUpdate();

expect(mockedCheck).toHaveBeenCalledTimes(1);
expect(mockedCheck).toHaveBeenCalledWith();
```

In `web-resource.test.ts` and the retained web-image case in `file.test.ts`, require:

```ts
expect(mockedInvoke).toHaveBeenCalledWith("download_web_image", {
  request: { url: "https://images.example.com/kitten.png" }
});
```

In `project-config.test.ts`, require sync and connection-test payloads to contain only project-specific fields:

```ts
expect(mockedInvoke).toHaveBeenCalledWith("test_project_sync_connection", {
  request: {
    expectedRevision: "rev-1",
    projectRoot: "/notes"
  }
});
```

- [ ] **Step 2: Run focused TypeScript tests and verify proxy-free assertions fail**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/lib/sync.test.ts
pnpm --filter @markra/desktop exec vitest run src/runtime/tauri/updater.test.ts src/runtime/tauri/web-resource.test.ts src/runtime/tauri/file.test.ts src/runtime/tauri/project-config.test.ts
```

Expected: FAIL because current code still loads proxy settings, tries local updater proxies, and serializes network data.

- [ ] **Step 3: Simplify application and desktop request contracts**

Reduce `runProjectFolderSync()` to:

```ts
type ProjectFolderSyncDependencies = {
  sync?: (input: ProjectFolderSyncRequest) => Promise<ProjectSyncRunResult>;
};

export async function runProjectFolderSync(
  request: ProjectFolderSyncRequest,
  { sync = (input) => getAppRuntime().projectConfig.sync(input) }: ProjectFolderSyncDependencies = {}
) {
  return sync(request);
}
```

Remove `network` from both `sync` and `testConnection` in `AppProjectConfigRuntime`. Serialize only feature-specific fields in `project-config.ts`.

Replace the updater's proxy-fallback call with the plugin's default check:

```ts
const update = await check();
if (!update) return null;
```

Delete `localUpdaterProxyUrls`, `updaterProxyUrls()`, and `checkWithLocalProxyFallback()`.

Change the web-image bridge to:

```ts
export async function downloadNativeWebImage({ src }: DownloadNativeWebImageInput): Promise<File> {
  const downloadedImage = await invokeNative<WebImageDownloadResponse>("download_web_image", {
    request: { url: src }
  });
  return new File([new Uint8Array(downloadedImage.bytes)], downloadedImage.fileName, {
    type: downloadedImage.mimeType
  });
}
```

- [ ] **Step 4: Delete the TypeScript proxy model, storage APIs, and test mocks**

Delete `network-settings.ts`, its test, and the desktop `tauri/network.ts` bridge. Remove the network imports/exports, `networkKey`, `getStoredNetworkSettings()`, and `saveStoredNetworkSettings()` from `app-settings.ts`. Remove their mocks, defaults, reset logic, and exports from `app-harness.tsx` and all desktop tests.

Verify that the retained settings module exports none of `NetworkSettings`, `defaultNetworkSettings`, `normalizeNetworkSettings`, `getStoredNetworkSettings`, or `saveStoredNetworkSettings`.

- [ ] **Step 5: Run TypeScript tests, typecheck, and build**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/lib/sync.test.ts src/lib/settings/app-settings.test.ts src/App.test.tsx
pnpm --filter @markra/desktop exec vitest run src/runtime/tauri/updater.test.ts src/runtime/tauri/web-resource.test.ts src/runtime/tauri/file.test.ts src/runtime/tauri/project-config.test.ts
pnpm typecheck:test
pnpm build
```

Expected: all commands PASS; no frontend or desktop-runtime request carries application proxy data.

- [ ] **Step 6: Commit the TypeScript runtime removal**

```bash
git add packages/app/src apps/desktop/src/runtime
git commit -m "refactor: remove proxy request plumbing"
```

---

### Task 4: Remove the Rust proxy contract and HTTP client customization

**Files:**
- Delete: `apps/desktop/src-tauri/src/network.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/web_http.rs`
- Modify: `apps/desktop/src-tauri/src/project_config.rs`
- Modify: `apps/desktop/src-tauri/src/project_config/model.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/s3_backend.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/live_tests.rs`

**Interfaces:**
- Consumes: native Tauri request structs, `S3Backend::new`, WebDAV/S3 client builders, and project sync command functions.
- Produces: native request structs with no network field; `S3Backend::new(settings: S3SyncSettings)`; default reqwest clients that preserve timeouts, redirect policies, credentials, URL guards, and error redaction.

- [ ] **Step 1: Change Rust contract tests to reject the removed network field**

Replace the project connection request test with a retained-shape assertion and an unknown-field rejection:

```rust
#[test]
fn connection_test_request_accepts_only_project_and_revision() {
    let request: TestProjectSyncConnectionRequest = serde_json::from_value(serde_json::json!({
        "expectedRevision": "rev-1",
        "projectRoot": "/notes"
    }))
    .expect("safe connection-test request");
    assert_eq!(request.expected_revision, "rev-1");
    assert_eq!(request.project_root, "/notes");

    let error = serde_json::from_value::<TestProjectSyncConnectionRequest>(serde_json::json!({
        "expectedRevision": "rev-1",
        "network": {},
        "projectRoot": "/notes"
    }))
    .expect_err("network must be rejected as an unknown field");
    assert!(error.to_string().contains("unknown field `network`"));
}
```

Make the equivalent change to the `SyncProjectFolderRequest` serialization test in `remote_sync.rs`: safe input contains project root, revision, trigger, and optional apply token only; adding `network` must fail under `deny_unknown_fields`.

- [ ] **Step 2: Run focused Rust tests and verify the unknown-field assertions fail**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml connection_test_request_accepts_only_project_and_revision
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml accepts_only_project_identity_trigger_and_apply_token_in_sync_requests
```

Expected: FAIL because both request types still accept `network`.

- [ ] **Step 3: Remove network from request structs and native command flow**

Change the request structures to:

```rust
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct TestProjectSyncConnectionRequest {
    pub(crate) expected_revision: String,
    pub(crate) project_root: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct SyncProjectFolderRequest {
    apply_token: Option<String>,
    expected_revision: String,
    project_root: String,
    trigger: ProjectSyncTrigger,
}
```

Remove network parameters through `project_config.rs`, `test_connection`, `execute_snapshot_sync`, `execute_project_sync_target`, and `create_webdav_backend`.

- [ ] **Step 4: Build default HTTP clients and delete the native proxy module**

In `web_http.rs`, retain URL/private-address checks and construct the client directly:

```rust
let client = reqwest::Client::builder()
    .redirect(Policy::none())
    .timeout(Duration::from_secs(WEB_IMAGE_REQUEST_TIMEOUT_SECS))
    .build()
    .map_err(|error| error.to_string())?;
```

Change `WebImageDownloadRequest` to contain only `url: String`.

Replace the S3 client-builder portion with:

```rust
let client = Client::builder()
    .timeout(Duration::from_secs(REMOTE_SYNC_TIMEOUT_SECS))
    .build()
    .map_err(|error| error.to_string())?;
let connection_test_client = Client::builder()
    .timeout(Duration::from_secs(REMOTE_SYNC_TIMEOUT_SECS))
    .redirect(Policy::none())
    .build()
    .map_err(|error| error.to_string())?;
```

Update every `S3Backend::new(settings, None)` call in offline and live tests to `S3Backend::new(settings)`. Make `remote_sync_http_client()` and `connection_test_http_client()` parameterless while preserving timeout and redirect configuration.

Delete `network.rs`, remove `mod network;` from `lib.rs`, and delete proxy-only Rust tests including invalid-proxy redaction cases. Retain transport/status redaction tests that do not depend on proxy configuration.

- [ ] **Step 5: Run the full Rust suite**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
```

Expected: all offline tests PASS; only environment-gated live MinIO tests may be ignored.

- [ ] **Step 6: Commit the native removal**

```bash
git add apps/desktop/src-tauri/src
git commit -m "refactor: remove native proxy runtime"
```

---

### Task 5: Remove current proxy documentation and future reintroduction paths

**Files:**
- Modify: `docs/privacy.md`
- Modify: `docs/superpowers/specs/2026-07-19-qingyu-mcp-design.md`
- Modify: `docs/superpowers/plans/2026-07-19-qingyu-mcp.md`

**Interfaces:**
- Consumes: current privacy promises and the approved forward-looking QingYu MCP design/plan.
- Produces: documentation that describes default networking and exposes no application proxy settings through future MCP settings contracts.

- [ ] **Step 1: Update privacy wording to describe retained networking without proxy controls**

Replace the proxy sentence with this exact wording:

```markdown
The desktop app can access the network when you explicitly add an image from an internet URL, configure project folder sync, or check for application updates. These features use their configured service endpoints and the runtime's default network behavior.
```

- [ ] **Step 2: Remove proxy fields from the forward-looking MCP design and plan**

Delete `network proxy configuration excluding credentials` from the MCP design. In the MCP plan:

- remove `network.bypassLocalAddresses`, `network.proxyEnabled`, and `network.proxyUrl` from exposed settings;
- remove proxy URL credential validation and redaction requirements;
- remove the network settings group from getters, savers, revision calculation, and rollback steps;
- keep WebDAV/S3 credential protection, sync endpoints, background runs, and sync status behavior unchanged.

The resulting settings group list must be:

```text
appearance, language, editor, file-ignore, export
```

- [ ] **Step 3: Run a current-contract static scan**

Run:

```bash
rg -n 'NetworkSettings|defaultNetworkSettings|normalizeNetworkSettings|getStoredNetworkSettings|saveStoredNetworkSettings|proxyEnabled|proxyUrl|bypassLocalAddresses|apply_network_settings|networkProxy|settings\.network\.|settings\.categories\.network|settings\.sections\.networkProxy' packages apps docs/privacy.md docs/superpowers/specs/2026-07-19-qingyu-mcp-design.md docs/superpowers/plans/2026-07-19-qingyu-mcp.md
```

Expected: no matches. The approved removal design and older implementation-history documents are intentionally outside this active-contract scan.

- [ ] **Step 4: Commit documentation cleanup**

```bash
git add docs/privacy.md docs/superpowers/specs/2026-07-19-qingyu-mcp-design.md docs/superpowers/plans/2026-07-19-qingyu-mcp.md
git commit -m "docs: remove application proxy contracts"
```

---

### Task 6: Run final repository and desktop acceptance gates

**Files:**
- Verify only; do not modify tracked source unless a failing gate identifies an in-scope defect.

**Interfaces:**
- Consumes: the completed Tasks 1-5 and repository verification commands.
- Produces: fresh evidence that application proxy support is gone and retained networking still builds and tests.

- [ ] **Step 1: Re-run the static removal gate and retained-cloud-sync scan**

Run:

```bash
rg -n 'NetworkSettings|defaultNetworkSettings|normalizeNetworkSettings|getStoredNetworkSettings|saveStoredNetworkSettings|proxyEnabled|proxyUrl|bypassLocalAddresses|apply_network_settings|networkProxy|settings\.network\.|settings\.categories\.network|settings\.sections\.networkProxy' packages apps docs/privacy.md docs/superpowers/specs/2026-07-19-qingyu-mcp-design.md docs/superpowers/plans/2026-07-19-qingyu-mcp.md
rg -n 'useProjectSyncCoordinator|runProjectFolderSync|syncProjectFolder|WebDAV|S3' packages/app/src apps/desktop/src apps/desktop/src-tauri/src/project_config apps/desktop/src-tauri/src/remote_sync
```

Expected: the first command has no matches; the second shows retained WebDAV/S3 UI, request, model, backend, and test paths.

- [ ] **Step 2: Run all automated verification gates**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
pnpm test
pnpm typecheck:test
pnpm build
```

Expected: all commands exit 0. Record exact test totals and ignored live-test counts.

- [ ] **Step 3: Run live MinIO coverage only when configured**

Check only variable names, never values:

```bash
for name in MARKRA_TEST_S3_ENDPOINT MARKRA_TEST_S3_REGION MARKRA_TEST_S3_BUCKET MARKRA_TEST_S3_ACCESS_KEY_ID MARKRA_TEST_S3_SECRET_ACCESS_KEY; do
  if [ -n "${(P)name}" ]; then print -r -- "$name=set"; else print -r -- "$name=unset"; fi
done
```

If every required variable is set, run:

```bash
pnpm test:s3-sync:live
```

Otherwise report the environment limitation without treating it as a product failure.

- [ ] **Step 4: Build and inspect the latest Tauri debug app**

Run:

```bash
pnpm tauri build --debug
open -n 'apps/desktop/src-tauri/target/debug/bundle/macos/QingYu.app'
```

Using the computer-use runtime, verify:

1. The latest `dev.markra.app` bundle opens.
2. The settings sidebar contains no Network category.
3. The Sync category remains visible.
4. An unconfigured folder still offers to enable sync and mentions WebDAV without writing configuration.
5. No note is edited and no `.qingyu/config.json` is created during inspection.

Close the settings window and application after inspection.

- [ ] **Step 5: Verify repository hygiene**

Run:

```bash
git diff --check
git status --short --branch
git log --oneline -10
```

Expected: no uncommitted task changes; `bg.png` remains the only intentional untracked file; no remote push has occurred.
