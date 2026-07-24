# QingYu Mobile Platform Capabilities Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task with review checkpoints.

**Goal:** Ship one QingYu codebase that produces working Android and iOS applications with the approved Compact product scope, while preserving the current desktop editor and the existing WebDAV/S3 sync, conflict, deletion, and checkpoint behavior.

**Architecture:** Keep the React editor, managed workspace, project configuration, and sync engine shared. Select a desktop or mobile TypeScript runtime once during native startup, compile a desktop or mobile Rust builder with disjoint command/plugin sets, and expose only product-level capabilities to shared components. Mobile uses one persistent app-private workspace, system URI image selection, the official opener, and Compact navigation; it does not compile or render desktop-only actions.

**Tech Stack:** Tauri v2, Rust, React 19, TypeScript 6, Milkdown, Tailwind CSS, Vitest, `tauri-plugin-dialog`, `tauri-plugin-fs`, `tauri-plugin-opener`, Android Gradle, and Xcode/iOS Simulator.

## Global Constraints

- Complete `docs/superpowers/plans/2026-07-19-complete-network-proxy-removal.md` first. This plan assumes `networkProxy`, `network.rs`, proxy request fields, proxy dependencies, settings UI, translations, and tests have already been removed.
- Do not change the sync algorithm, manifest, checkpoint, conflict-copy naming, deletion propagation, or save-trigger semantics. Mobile calls the same `AppProjectConfigRuntime` and Rust `remote_sync` implementation as desktop.
- Do not add workspace switching, reset, clear-data, cloud deletion, arbitrary attachment import, Markdown import, file associations, share targets, background sync, title-derived file renaming, or a second mobile editor.
- Use the WebView/system text-selection clipboard on mobile. Do not add a native clipboard plugin in Tasks 1-9; the real-device acceptance gate must prove copy/paste before release.
- Mobile local-only mode and configured sync mode both use the single managed workspace below the operating system app-data directory.
- Shared React code must not test for `android`, `ios`, `macos`, `windows`, or `linux`. It reads `AppRuntime.features` or invokes an injected runtime method.
- Mobile JavaScript modules must not statically import adapters that invoke desktop-only Rust commands.
- Keep `arboard`, native menus, window state, single instance, updater, process restart, system font enumeration, shell/Pandoc, desktop file open, and desktop window code out of mobile Rust compilation units.
- Use `pnpm` only. Do not add another lockfile. Do not use the TypeScript `void` operator.
- Never commit MinIO/WebDAV credentials, private endpoints, signing files, developer-team identifiers, local SDK paths, screenshots containing secrets, or generated build output.
- Preserve the intentional untracked `bg.png` and all unrelated user changes.
- Every code task follows red-green-refactor: add the focused failing test, run it and confirm the stated failure, make the smallest implementation, rerun the focused test, then run the task-level regression command before committing.

## Blocking Preflight

Run from the repository root after completing the network-proxy removal plan:

```bash
git status --short
rg -n "networkProxy|network_proxy|proxyUrl|proxyUsername|proxyPassword|reqwest.*socks" packages apps --glob '!**/target/**'
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
pnpm test
pnpm typecheck:test
pnpm build
```

Expected:

- `git status --short` contains no unexpected tracked edits; `?? bg.png` is allowed and remains untracked.
- The proxy search exits with no matches.
- All four verification commands pass.

Do not begin the mobile tasks until this preflight passes. Fix the prerequisite plan rather than reintroducing compatibility fields here.

## Runtime Contract to Implement

Replace the post-proxy-removal `AppFeatureRuntime` with this exact product capability matrix:

```ts
export type AppFeatureRuntime = {
  applicationMenu: boolean;
  applicationShortcuts: boolean;
  export: boolean;
  fileDrop: boolean;
  imageImport: boolean;
  nativeWindowChrome: boolean;
  openLocalAttachments: boolean;
  pandoc: boolean;
  projectSync: boolean;
  settingsWindow: boolean;
  systemFonts: boolean;
  updater: boolean;
};
```

Native values are fixed as follows:

| Capability | Desktop | Mobile |
| --- | ---: | ---: |
| `applicationMenu` | `true` | `false` |
| `applicationShortcuts` | `true` | `false` |
| `export` | `true` | `false` |
| `fileDrop` | `true` | `false` |
| `imageImport` | `true` | `true` |
| `nativeWindowChrome` | `true` | `false` |
| `openLocalAttachments` | `true` | `false` |
| `pandoc` | `true` | `false` |
| `projectSync` | `true` | `true` |
| `settingsWindow` | `true` | `false` |
| `systemFonts` | `true` | `false` |
| `updater` | `true` | `false` |

Keep Web behavior explicit in `apps/web/src/runtime/index.ts` with this matrix: `applicationMenu: false`, `applicationShortcuts: true`, `export: true`, `fileDrop: true`, `imageImport: false`, `nativeWindowChrome: false`, `openLocalAttachments: true`, `pandoc: false`, `projectSync: false`, `settingsWindow: false`, `systemFonts: false`, and `updater: false`. A narrow browser viewport remains a Compact UI test surface and never reports native mobile capability.

Add navigation to `AppRuntime`:

```ts
export type AppSystemBackSubscriber = (
  handler: () => Promise<boolean>
) => Promise<RuntimeCleanup>;

export type AppNavigationRuntime = {
  subscribeToSystemBack: AppSystemBackSubscriber;
};
```

The default, Web, and desktop implementations resolve to a no-op cleanup. Mobile subscribes to `qingyu://mobile-back-requested`, awaits the Compact handler, and invokes `complete_mobile_back` with the boolean result.

## Task 1: Lock the Shared Runtime Capability Contract

**Files:**

- Modify: `packages/app/src/runtime/index.ts`
- Modify: `packages/app/src/runtime/index.test.ts`
- Modify: `apps/web/src/runtime/index.ts`
- Modify: `apps/web/src/runtime/index.test.ts`
- Modify: `packages/app/src/components/compact/types.ts`
- Modify: `packages/app/src/lib/diagnostics/diagnostics-report.ts`
- Modify: `packages/app/src/lib/diagnostics/diagnostics-report.test.ts`

**Step 1: Write failing capability tests**

In `packages/app/src/runtime/index.test.ts`, assert that `createDefaultAppRuntime()` exposes every field in the exact contract above and provides a no-op `navigation.subscribeToSystemBack`. In `apps/web/src/runtime/index.test.ts`, assert the Web feature values explicitly rather than relying on omitted defaults. Update diagnostics expectations to report only capabilities useful for diagnostics and never reference removed proxy state.

**Step 2: Run the tests to verify red**

```bash
pnpm --filter @markra/desktop exec vitest run ../../packages/app/src/runtime/index.test.ts ../../packages/app/src/lib/diagnostics/diagnostics-report.test.ts
pnpm --filter @markra/web exec vitest run src/runtime/index.test.ts
```

Expected: tests fail because the expanded fields and `navigation` runtime do not exist yet.

**Step 3: Implement the stable shared contract**

- Add the exact feature type, `AppSystemBackSubscriber`, and `AppNavigationRuntime` to `AppRuntime`. Task 8 removes the duplicate Compact-local subscriber alias and imports this runtime type into `useCompactNavigation.ts`.
- Default `subscribeToSystemBack` returns `() => undefined` and never intercepts browser navigation.
- Set every feature on `createDefaultAppRuntime()` to `false`. The concrete Web, desktop, and mobile runtimes opt into the exact matrices defined above.
- Add the capabilities needed by Compact components to `CompactAppController.capabilities`: `imageImport`, `openLocalAttachments`, `projectSync`, `systemFonts`, and `trueMobile`.
- Keep all platform decisions outside shared components.

**Step 4: Run focused and type tests**

```bash
pnpm --filter @markra/desktop exec vitest run ../../packages/app/src/runtime/index.test.ts ../../packages/app/src/lib/diagnostics/diagnostics-report.test.ts
pnpm --filter @markra/web exec vitest run src/runtime/index.test.ts
pnpm typecheck:test
```

Expected: all pass, and TypeScript reports every runtime that still needs the new fields before the task is committed.

**Step 5: Commit**

```bash
git add packages/app/src/runtime/index.ts packages/app/src/runtime/index.test.ts packages/app/src/components/compact/types.ts packages/app/src/lib/diagnostics/diagnostics-report.ts packages/app/src/lib/diagnostics/diagnostics-report.test.ts apps/web/src/runtime/index.ts apps/web/src/runtime/index.test.ts
git commit -m "refactor: define mobile runtime capabilities"
```

## Task 2: Select Desktop or Mobile Runtime Without Desktop Imports

**Files:**

- Create: `apps/desktop/src/runtime/desktop.ts`
- Create: `apps/desktop/src/runtime/mobile.ts`
- Modify: `apps/desktop/src/runtime/index.ts`
- Modify: `apps/desktop/src/runtime/index.test.ts`
- Modify: `apps/desktop/src/main.tsx`
- Modify: `apps/desktop/src/runtime/tauri/index.ts`
- Modify: `apps/desktop/package.json`
- Modify: `pnpm-lock.yaml`

**Step 1: Write failing loader tests**

Refactor `apps/desktop/src/runtime/index.test.ts` around two pure functions:

```ts
export type NativeRuntimeKind = "desktop" | "mobile";

export function nativeRuntimeKind(platform: string | null | undefined): NativeRuntimeKind;

export async function loadNativeRuntime(
  readPlatform?: () => string,
  loaders?: {
    desktop: () => Promise<{ desktopRuntime: AppRuntime }>;
    mobile: () => Promise<{ mobileRuntime: AppRuntime }>;
  }
): Promise<AppRuntime>;
```

Test Android and iOS selecting only the injected mobile loader, macOS/Windows/Linux selecting only the injected desktop loader, an OS-plugin exception falling back to desktop, and no eager execution of the unselected loader.

**Step 2: Run the loader test to verify red**

```bash
pnpm --filter @markra/desktop exec vitest run src/runtime/index.test.ts
```

Expected: failure because the runtime is a single eagerly imported `desktopRuntime`.

**Step 3: Split the native runtime**

- Move the current runtime object to `desktop.ts`; do not change its behavior except filling the Task 1 capabilities and navigation no-op.
- Make `index.ts` import only `platform` from `@tauri-apps/plugin-os`, then dynamically import `./mobile` or `./desktop` after resolving the platform.
- Build `mobileRuntime` from `createDefaultAppRuntime()` and override only shared native implementations: events, managed workspace, Markdown tree/document/history/search/watch operations, project config/sync, persistent store, log writer, Web image download, mobile image picker, external URL opener, and mobile back subscription.
- Mobile must not import `tauri/menu`, `tauri/window`, `tauri/fonts`, `tauri/shell-command`, `tauri/updater`, or a file adapter that imports desktop dialog/window APIs.
- Change `main.tsx` to await `loadNativeRuntime()`, call `configureAppRuntime(runtime)`, and only then render `App`. If loading rejects, render `AppErrorBoundary` with a small startup error component and a retry button that reloads the page; do not silently fall back to desktop on a positively identified mobile platform.
- Add `@tauri-apps/plugin-fs` `2.5.1` and `@tauri-apps/plugin-opener` `2.5.4` with `pnpm --filter @markra/desktop add` so `pnpm-lock.yaml` stays authoritative.

**Step 4: Prove module selection and compilation**

```bash
pnpm --filter @markra/desktop exec vitest run src/runtime/index.test.ts
pnpm --filter @markra/desktop typecheck:test
pnpm --filter @markra/desktop build
```

Expected: tests prove only the selected loader runs; typecheck and desktop Web build pass.

**Step 5: Commit**

```bash
git add apps/desktop/src/runtime/desktop.ts apps/desktop/src/runtime/mobile.ts apps/desktop/src/runtime/index.ts apps/desktop/src/runtime/index.test.ts apps/desktop/src/runtime/tauri/index.ts apps/desktop/src/main.tsx apps/desktop/package.json pnpm-lock.yaml
git commit -m "refactor: select native runtime by platform"
```

## Task 3: Compile Disjoint Desktop and Mobile Rust Builders

**Files:**

- Create: `apps/desktop/src-tauri/src/desktop_runtime.rs`
- Create: `apps/desktop/src-tauri/src/mobile_runtime.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/bin/qingyu-mcp.rs`
- Modify: `apps/desktop/src-tauri/src/markdown_files.rs`
- Modify: `apps/desktop/src-tauri/build.rs`
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/capabilities/main.json`
- Create: `apps/desktop/src-tauri/capabilities/mobile.json`
- Modify: `apps/desktop/src-tauri/tauri.conf.json`
- Modify: `apps/desktop/src-tauri/tauri.macos.conf.json`
- Modify: `apps/desktop/src-tauri/tauri.windows.conf.json`
- Modify: `apps/desktop/src-tauri/tauri.linux.conf.json`
- Generated: `apps/desktop/src-tauri/gen/android/**`
- Generated: `apps/desktop/src-tauri/gen/apple/**`

**Step 1: Add failing builder-boundary tests**

Keep `lib.rs` as a small dispatcher and add test helpers that extract handler identifiers from the source between `tauri::generate_handler![` and its closing bracket. Assert:

- `desktop_runtime.rs` includes the existing complete desktop command set, including both typed app-settings commands and all ten QingYu MCP commands added by `605dd3d`.
- `mobile_runtime.rs` includes shared file tree/read/write/history/search/watch commands, `resolve_managed_workspace_root`, project config/editing/status/connection/sync commands, and `download_web_image`.
- The mobile source excludes app menus, opened-path/file-association commands, window state and multiwindow commands, settings window commands, log-folder opening, clipboard/arboard, system fonts, Shell/Pandoc/export/templates, updater/process restart, arbitrary attachment import/open, containing-folder open, project reset/reveal, typed app-settings commands, every MCP command, MCP state/server initialization, and the `qingyu-mcp` sidecar.
- `lib.rs` cfg-gates `app_settings` and `mcp` to desktop application builds or the explicit sidecar feature. The `qingyu-mcp` binary has a `required-features` gate and remains buildable for supported desktop hosts without making that feature a mobile default.
- `Cargo.toml` keeps desktop-only crates under `cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))` and has no common `arboard`, `fontdb`, process, updater, single-instance, window-state, `macos-private-api`, `axum`, `keyring`, `rmcp`, `schemars`, `tower`, `tokio-util`, or other MCP-only dependency. Shared sync dependencies stay common.
- `build.rs` never creates a `qingyu-mcp-*` sidecar slot for Android or iOS targets.
- `main.json` has `platforms: ["linux", "macOS", "windows"]`; `mobile.json` has `platforms: ["iOS", "android"]` and no desktop/menu/process/updater/window-management permission.
- Base `tauri.conf.json` has no `bundle.externalBin`; each desktop platform config declares `binaries/qingyu-mcp`, and mobile platform resolution declares none.

**Step 2: Run Rust tests to verify red**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml builder_boundary -- --nocapture
```

Expected: tests fail because one builder and one broad capability still serve every target.

**Step 3: Split modules, dependencies, plugins, and handlers**

- Reduce `lib.rs` to shared module declarations, cfg-gated desktop-only module declarations, `#[cfg(desktop)] mod desktop_runtime`, `#[cfg(mobile)] mod mobile_runtime`, and a `run()` dispatcher retaining `#[cfg_attr(mobile, tauri::mobile_entry_point)]`.
- Move the existing builder, menu/window setup, desktop lifecycle, desktop constants, and desktop-only tests into `desktop_runtime.rs` without changing desktop behavior.
- Put `app_exit`, `app_logs` folder opening, `app_settings`, `clipboard`, `fonts`, `language`, `mcp`, `menu`, `menu_labels`, `opened_files`, `shell_command`, `window_state`, and `windows` behind desktop cfg. Keep `external_urls.rs` desktop cfg only; Task 7 deletes it after both runtimes switch to the opener plugin. The sidecar build may opt into `app_settings` and `mcp` through its explicit Cargo feature, but the mobile library may not.
- In `markdown_files.rs`, cfg-gate desktop-only re-exports/modules for attachment import/open, Pandoc/export, templates, containing-folder/open-in-new-window, and local `PathBuf` picker reads. Keep tree, document read/write, history, search, watcher, safe image save, and types shared.
- Create `mobile_runtime.rs` with only shared state: `MarkdownFileWatcherState`, `MarkdownTreeWatcherState`, and `MarkdownTreeLoadState`. Do not manage opened paths, native menus, or editor window restore state.
- Keep the current desktop MCP server/state initialization, ten MCP handlers, two typed app-settings handlers, sidecar metadata, and shutdown behavior in `desktop_runtime.rs`; register none of them in `mobile_runtime.rs`.
- Register common mobile plugins: store, dialog, fs, log, os, and opener. Register no process, updater, single-instance, or window-state plugin.
- Move `arboard`, `fontdb`, `tauri-plugin-process`, `tauri-plugin-updater`, and every dependency used only by desktop modules or MCP/app-settings (`axum`, `keyring`, `rmcp`, `schemars`, `tower`, `tokio-util`, and any other source-audited MCP-only crate) into the desktop target dependency table. Restrict `macos-private-api` to the macOS target and leave common Tauri with `protocol-asset` only. Keep `notify`, sync HTTP/crypto/XML, `cap-*`, serde, time, and Tokio common.
- Add a non-default `desktop-sidecar` Cargo feature, declare `qingyu-mcp` with `required-features = ["desktop-sidecar"]`, and update the sidecar preparation command to enable that feature. This gate must not be enabled by Android or iOS builds.
- Add Rust `tauri-plugin-fs = "2.5.1"` and `tauri-plugin-opener = "2.5.4"`.

**Step 4: Split capabilities and base configuration**

- Restrict `main.json` to desktop platforms and preserve its existing desktop permissions.
- Add `mobile.json` using `../gen/schemas/mobile-schema.json`, window `main`, and the minimum core app/event/path/resources/window, dialog open/confirm/message, scoped fs read, log, os, store, and opener HTTP/HTTPS permissions. Do not add general Shell or whole-device storage permissions.
- Include both capability identifiers in base `tauri.conf.json`; platform matching selects the valid one.
- Remove `bundle.externalBin` from base config and declare `binaries/qingyu-mcp` only in the macOS, Windows, and Linux platform configs. Android and iOS generated projects and resolved configs must contain no sidecar name or binary slot.
- Move `app.macOSPrivateApi` to `tauri.macos.conf.json`.
- Move Markdown file associations out of base config and duplicate them only in the macOS, Windows, and Linux platform config files. Mobile must not receive VIEW/SEND/file-association declarations.

**Step 5: Generate native projects**

```bash
pnpm tauri android init --ci
pnpm tauri ios init --ci
```

Expected: `gen/android` and `gen/apple` are generated; their own ignore rules exclude build caches, local properties, derived data, and signing material. Inspect generated Android manifest and Apple project configuration and confirm no Markdown file association/share intent, `qingyu-mcp` sidecar, sidecar placeholder, MCP service, or typed-settings command permission was inherited.

**Step 6: Run desktop tests and first mobile compile checks**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
cargo build --manifest-path apps/desktop/src-tauri/Cargo.toml --bin qingyu-mcp --features desktop-sidecar
pnpm tauri android build --debug --apk --target x86_64 --ci
pnpm tauri ios build --debug --target aarch64-sim --ci --no-sign
```

Expected: Rust tests and the explicitly featured desktop sidecar build pass; Android produces a debug APK; iOS produces an unsigned Simulator app. There are no compile errors from `arboard`, menu, window-state, window decoration, updater, process, app-settings, MCP, sidecar, or desktop-only dependency APIs, and neither mobile bundle contains `qingyu-mcp`.

**Step 7: Commit**

```bash
git add apps/desktop/src-tauri/src apps/desktop/src-tauri/build.rs apps/desktop/src-tauri/Cargo.toml apps/desktop/src-tauri/capabilities apps/desktop/src-tauri/tauri.conf.json apps/desktop/src-tauri/tauri.macos.conf.json apps/desktop/src-tauri/tauri.windows.conf.json apps/desktop/src-tauri/tauri.linux.conf.json apps/desktop/src-tauri/gen/android apps/desktop/src-tauri/gen/apple package.json packages/scripts/src/prepare-qingyu-mcp-sidecar.mjs
git commit -m "build: split desktop and mobile tauri runtimes"
```

## Task 4: Remove Desktop-Only UI Effects From the Mobile Product Surface

**Files:**

- Modify: `packages/app/src/hooks/useNativeBindings.ts`
- Modify: `packages/app/src/hooks/useNativeBindings.test.tsx`
- Modify: `packages/app/src/hooks/useStartupWindowReveal.ts`
- Modify: `packages/app/src/hooks/useStartupWindowReveal.test.tsx`
- Modify: `packages/app/src/components/compact/CompactSettingsHome.tsx`
- Modify: `packages/app/src/components/compact/CompactSettingsHome.test.tsx`
- Modify: `packages/app/src/components/compact/CompactSettingsDetail.tsx`
- Modify: `packages/app/src/components/compact/CompactSettingsDetail.test.tsx`
- Modify: `packages/app/src/components/compact/CompactSyncStatusScreen.tsx`
- Modify: `packages/app/src/components/compact/CompactSyncStatusScreen.test.tsx`
- Modify: `packages/app/src/components/SettingsWindow.tsx`
- Modify: `packages/app/src/App.tsx`
- Modify: `packages/app/src/App.test.tsx`

**Step 1: Write failing capability-gate tests**

Add tests proving that an `enabled: false` option prevents installation of file-drop listeners, native menus, application shortcuts, and startup native reveal. Add App/Compact tests with the mobile matrix proving:

- no desktop menu, file-drop, settings-window prewarm, updater, export, Pandoc, window chrome, system-font enumeration, or desktop settings category is invoked/rendered;
- Compact settings omit the system font/custom font input rather than disabling it;
- mobile still renders project sync, theme, language, editor preferences, full-screen files, and full-screen settings;
- an unsupported configuration or connection error shows the existing safe reason and no more than one action, which opens the Compact sync settings form;
- sync error text never renders an access key, secret, authorization header, or credential-bearing URL;
- capability false paths never invoke an unsupported runtime method.

**Step 2: Run focused tests to verify red**

```bash
pnpm --filter @markra/desktop exec vitest run ../../packages/app/src/hooks/useNativeBindings.test.tsx ../../packages/app/src/hooks/useStartupWindowReveal.test.tsx ../../packages/app/src/components/compact/CompactSettingsHome.test.tsx ../../packages/app/src/components/compact/CompactSettingsDetail.test.tsx ../../packages/app/src/components/compact/CompactSyncStatusScreen.test.tsx ../../packages/app/src/App.test.tsx
```

Expected: failing assertions show the hooks and settings prewarm still run unconditionally and Compact editor settings still show a raw font field.

**Step 3: Implement the product gates**

- Add `enabled` to `useNativeMarkdownDrop`, `useNativeMenus`, `useApplicationShortcuts`, and `useStartupWindowReveal`; when false they install nothing and clean up any earlier registration.
- In `App.tsx`, pass `appFeatures.fileDrop`, `applicationMenu`, `applicationShortcuts`, and `nativeWindowChrome` to those hooks.
- Only prewarm/open an independent settings window when `settingsWindow` is true. Compact navigation continues to open its full-screen in-app settings page.
- Thread `systemFonts` through `CompactAppController.capabilities` and omit the font-family/custom-font controls when false. Keep theme and system/default font rendering.
- Keep the Compact sync status error panel to one `openSyncSettings` action. Pass provider-safe error descriptions through the existing sanitization boundary and never add reset as a recovery action.
- Keep desktop and Web behavior unchanged for capabilities they explicitly support.

**Step 4: Run focused and full frontend tests**

```bash
pnpm --filter @markra/desktop exec vitest run ../../packages/app/src/hooks/useNativeBindings.test.tsx ../../packages/app/src/hooks/useStartupWindowReveal.test.tsx ../../packages/app/src/components/compact/CompactSettingsHome.test.tsx ../../packages/app/src/components/compact/CompactSettingsDetail.test.tsx ../../packages/app/src/components/compact/CompactSyncStatusScreen.test.tsx ../../packages/app/src/App.test.tsx
pnpm test
pnpm typecheck:test
```

Expected: all pass.

**Step 5: Commit**

```bash
git add packages/app/src/hooks/useNativeBindings.ts packages/app/src/hooks/useNativeBindings.test.tsx packages/app/src/hooks/useStartupWindowReveal.ts packages/app/src/hooks/useStartupWindowReveal.test.tsx packages/app/src/components/compact/CompactSettingsHome.tsx packages/app/src/components/compact/CompactSettingsHome.test.tsx packages/app/src/components/compact/CompactSettingsDetail.tsx packages/app/src/components/compact/CompactSettingsDetail.test.tsx packages/app/src/components/compact/CompactSyncStatusScreen.tsx packages/app/src/components/compact/CompactSyncStatusScreen.test.tsx packages/app/src/components/SettingsWindow.tsx packages/app/src/App.tsx packages/app/src/App.test.tsx
git commit -m "feat: gate desktop ui by runtime capability"
```

## Task 5: Replace Compact Browser Prompts With an In-App Name Dialog

**Files:**

- Create: `packages/app/src/components/compact/CompactNameDialog.tsx`
- Create: `packages/app/src/components/compact/CompactNameDialog.test.tsx`
- Modify: `packages/app/src/components/compact/CompactFileBrowserScreen.tsx`
- Modify: `packages/app/src/components/compact/CompactFileBrowserScreen.test.tsx`
- Modify: `packages/app/src/components/compact/CompactWelcomeState.tsx`
- Modify: `packages/app/src/components/compact/CompactAcceptance.test.tsx`
- Modify: `packages/app/src/components/compact/types.ts`
- Modify: `packages/app/src/App.tsx`
- Modify: `packages/app/src/App.test.tsx`
- Modify: `packages/shared/src/i18n/locales/types.ts`
- Modify: `packages/shared/src/i18n/locales/{de,en,es,fr,it,ja,ko,pt-BR,ru,zh-CN,zh-TW}.ts`

**Step 1: Write failing dialog and workflow tests**

Test `CompactNameDialog` as a controlled modal with `title`, `initialValue`, `submitLabel`, `cancelLabel`, and async `onSubmit(value)` props. Cover autofocus, trimmed non-empty submission, disabled empty submission, Enter, Escape/cancel, 44px minimum targets, pending state, and an inline backend error that keeps the dialog open.

Update file-browser and welcome tests to assert:

- create file, create folder, rename, and welcome-state new document open the in-app dialog;
- cancel performs no mutation;
- duplicate/invalid/backend errors remain visible in the dialog;
- successful file creation opens the file and returns to the editor;
- current `unsavedMarkdownFileNameFromTreeInput` naming semantics remain unchanged;
- `window.prompt` is never called.

**Step 2: Run focused tests to verify red**

```bash
pnpm --filter @markra/desktop exec vitest run ../../packages/app/src/components/compact/CompactNameDialog.test.tsx ../../packages/app/src/components/compact/CompactFileBrowserScreen.test.tsx ../../packages/app/src/components/compact/CompactAcceptance.test.tsx ../../packages/app/src/App.test.tsx
```

Expected: component import fails and existing workflows still call `window.prompt`.

**Step 3: Implement one reusable name-dialog flow**

- Give `CompactNameDialog` `role="dialog"`, an accessible label, focus restoration, safe-area-compatible positioning, and no desktop-window dependency.
- Replace the three prompts in `CompactFileBrowserScreen.tsx` with a single discriminated action state: `create-file`, `create-folder`, or `rename`, carrying parent/file context.
- Change Compact document creation to accept the submitted file name: `createBlankDocument(fileName: string): Promise<boolean>` at the Compact controller boundary. Keep the existing App naming and creation implementation; do not derive names from content or introduce “Untitled” auto-renaming.
- Let backend errors propagate to the dialog as readable operation errors. A successful operation closes the modal; failure does not.
- Add concise translations for cancel/create/rename and invalid-name/error text across every existing locale to satisfy the strict i18n type.

**Step 4: Prove prompt removal and workflow behavior**

```bash
rg -n "window\.prompt" packages/app/src
pnpm --filter @markra/desktop exec vitest run ../../packages/app/src/components/compact/CompactNameDialog.test.tsx ../../packages/app/src/components/compact/CompactFileBrowserScreen.test.tsx ../../packages/app/src/components/compact/CompactAcceptance.test.tsx ../../packages/app/src/App.test.tsx
pnpm typecheck:test
```

Expected: the search has no matches; all tests and typecheck pass.

**Step 5: Commit**

```bash
git add packages/app/src/components/compact packages/app/src/components/compact/types.ts packages/app/src/App.tsx packages/app/src/App.test.tsx packages/shared/src/i18n/locales
git commit -m "feat: add compact file name dialog"
```

## Task 6: Implement URI-Based Mobile Image Import and Safe Workspace Persistence

**Files:**

- Create: `apps/desktop/src/runtime/tauri/file/shared.ts`
- Create: `apps/desktop/src/runtime/tauri/file/desktop.ts`
- Create: `apps/desktop/src/runtime/tauri/file/mobile.ts`
- Create: `apps/desktop/src/runtime/tauri/mobile-image-file.ts`
- Create: `apps/desktop/src/runtime/tauri/mobile-image-file.test.ts`
- Modify: `apps/desktop/src/runtime/tauri/file.ts`
- Modify: `apps/desktop/src/runtime/tauri/file.test.ts`
- Modify: `apps/desktop/src/runtime/desktop.ts`
- Modify: `apps/desktop/src/runtime/mobile.ts`
- Modify: `packages/app/src/lib/editor-assets.ts`
- Modify: `packages/app/src/lib/editor-assets.test.ts`
- Modify: `packages/app/src/lib/image-upload.ts`
- Modify: `packages/app/src/lib/image-upload.test.ts`
- Modify: `packages/app/src/components/compact/CompactEditorToolbar.tsx`
- Modify: `packages/app/src/components/compact/CompactEditorToolbar.test.tsx`
- Modify: `packages/app/src/App.tsx`
- Modify: `packages/app/src/App.test.tsx`
- Modify: `apps/desktop/src-tauri/src/markdown_files/image.rs`
- Modify: `apps/desktop/src-tauri/src/markdown_files/attachment.rs`

**Step 1: Write failing mobile image-decoder tests**

Define a pure function that accepts `{ bytes: Uint8Array; uri: string }` and returns a `File`. Test PNG, JPEG, GIF, WebP, BMP, AVIF, and UTF-8 SVG signatures; safe file-name recovery from encoded file/content URIs; fallback `picked-image.<ext>`; empty bytes; URI-extension/signature disagreement; and arbitrary PDF/text rejection. Signature validation is authoritative: a mismatched URI suffix is replaced with the extension detected from bytes.

Test the mobile picker adapter with injected `open` and `readFile` functions:

- calls dialog `open` with `multiple: true`, `pickerMode: "image"`, `fileAccessMode: "scoped"`, and supported image filters;
- returns `[]` on user cancel without an error toast;
- passes Android `content://` and iOS file URI strings directly to `readFile` and never invokes Rust `read_local_image_file`;
- returns validated `File[]` or rejects with an actionable unsupported/read error.

**Step 2: Write failing persistence tests**

In `editor-assets.test.ts`, add `managedWorkspace: true` cases that resolve to:

```ts
{ mode: "managed-workspace", projectRootPath: string }
```

and make imported/clipboard/remote images use `copy-project`. In App tests, prove true mobile local-only mode passes the fixed workspace root to image save and inserts Markdown only after save succeeds. Cancellation and every failure path leave editor Markdown unchanged.

In Rust, replace fake `[1, 2, 3]` image fixtures with valid minimal signatures and add tests for MIME/signature mismatch, collision suffixing, complete atomic publish, injected write failure, and staging cleanup. Reuse the no-follow/no-clobber staging publisher from `attachment.rs` rather than creating a second weaker writer; expose the smallest `pub(super)` helper needed by `image.rs`.

**Step 3: Run focused tests to verify red**

```bash
pnpm --filter @markra/desktop exec vitest run src/runtime/tauri/mobile-image-file.test.ts src/runtime/tauri/file.test.ts ../../packages/app/src/lib/editor-assets.test.ts ../../packages/app/src/lib/image-upload.test.ts ../../packages/app/src/components/compact/CompactEditorToolbar.test.tsx ../../packages/app/src/App.test.tsx
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml markdown_files::image::tests -- --nocapture
```

Expected: tests fail because mobile URI reading, managed-workspace asset mode, content signature checks, and atomic standalone image publication are absent.

**Step 4: Split desktop and mobile file adapters**

- Move invoke-only shared tree/document/history/search/watch/save functions to `file/shared.ts`.
- Move desktop picker, drop, file/folder open, attachment open/import, export, templates, and new-window functions to `file/desktop.ts`.
- Keep `file.ts` as a desktop compatibility re-export so existing desktop imports and tests remain stable.
- Implement `file/mobile.ts` with the official dialog and fs plugins. It must read the selected URI with plugin-fs, validate bytes with `mobile-image-file.ts`, build browser `File` objects, and expose only the operations used by `mobileRuntime`.
- Do not request broad Android external-storage permissions and do not turn `content://` into `PathBuf`.

**Step 5: Persist images inside the managed workspace**

- Extend `EditorAssetContext` to `standalone | managed-workspace | sync-project`.
- Add `managedWorkspace: boolean` to `resolveEditorAssetContext`; select managed-workspace when true and a fixed root exists, even if sync is disabled.
- Route managed-workspace and sync-project images through existing project-root resource saving, producing a relative Markdown path under `<workspace>/assets`.
- Validate image bytes against normalized MIME in Rust before writing.
- Publish complete bytes with the attachment staging/no-clobber mechanism. If validation or writing fails, remove staging, do not create a final file, and return an error before editor insertion.
- In Compact true-mobile mode, wire the toolbar image action to `handleImportLocalImages`. When `trueMobile` is false, keep the current editor image skeleton action. Arbitrary file import remains unavailable.

**Step 6: Run focused, Rust, and frontend regressions**

```bash
pnpm --filter @markra/desktop exec vitest run src/runtime/tauri/mobile-image-file.test.ts src/runtime/tauri/file.test.ts ../../packages/app/src/lib/editor-assets.test.ts ../../packages/app/src/lib/image-upload.test.ts ../../packages/app/src/components/compact/CompactEditorToolbar.test.tsx ../../packages/app/src/App.test.tsx
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml markdown_files::image::tests -- --nocapture
pnpm test
pnpm typecheck:test
```

Expected: selection cancel is silent; unsupported/copy errors are visible; successful files are workspace-relative and collision-safe; all regressions pass.

**Step 7: Commit**

```bash
git add apps/desktop/src/runtime/tauri apps/desktop/src/runtime/desktop.ts apps/desktop/src/runtime/mobile.ts packages/app/src/lib/editor-assets.ts packages/app/src/lib/editor-assets.test.ts packages/app/src/lib/image-upload.ts packages/app/src/lib/image-upload.test.ts packages/app/src/components/compact/CompactEditorToolbar.tsx packages/app/src/components/compact/CompactEditorToolbar.test.tsx packages/app/src/App.tsx packages/app/src/App.test.tsx apps/desktop/src-tauri/src/markdown_files/image.rs apps/desktop/src-tauri/src/markdown_files/attachment.rs
git commit -m "feat: import mobile images into managed workspace"
```

## Task 7: Use the Official Opener and Block Unsupported Mobile Attachments

**Files:**

- Create: `apps/desktop/src/runtime/tauri/opener.ts`
- Create: `apps/desktop/src/runtime/tauri/opener.test.ts`
- Modify: `apps/desktop/src/runtime/desktop.ts`
- Modify: `apps/desktop/src/runtime/mobile.ts`
- Modify: `apps/desktop/src-tauri/capabilities/main.json`
- Delete: `apps/desktop/src-tauri/src/external_urls.rs`
- Modify: `apps/desktop/src-tauri/src/desktop_runtime.rs`
- Modify: `apps/desktop/src-tauri/src/mobile_runtime.rs`
- Modify: `packages/app/src/App.tsx`
- Modify: `packages/app/src/App.test.tsx`
- Modify: `packages/shared/src/i18n/locales/types.ts`
- Modify: `packages/shared/src/i18n/locales/{de,en,es,fr,it,ja,ko,pt-BR,ru,zh-CN,zh-TW}.ts`

**Step 1: Write failing opener and attachment tests**

- Test that the adapter calls `openUrl` only for parsed `http:` and `https:` URLs and propagates a sanitized failure for the UI; reject file/javascript/custom schemes before the plugin call.
- In App tests with `openLocalAttachments: false`, click a synced non-image attachment and assert one concise unsupported toast, no `openMarkdownAttachment` invocation, and no opener invocation.
- Test opener failure keeps the current editor/page and shows the reason.

**Step 2: Run focused tests to verify red**

```bash
pnpm --filter @markra/desktop exec vitest run src/runtime/tauri/opener.test.ts ../../packages/app/src/App.test.tsx
```

Expected: failure because external URLs still use the Rust subprocess command and attachment opening is not capability-gated.

**Step 3: Replace subprocess opening**

- Implement `opener.ts` over `@tauri-apps/plugin-opener` `openUrl`.
- Use it in both desktop and mobile runtimes.
- Register the opener plugin in both Rust builders and remove `open_external_url` from both handlers, then delete `external_urls.rs`.
- Keep capability permissions limited to HTTP and HTTPS. Do not authorize mail, telephone, file, JavaScript, or custom schemes in this implementation.
- Gate local attachment opening in both file-tree activation and Markdown link activation. If false, show `app.mobileAttachmentUnsupported` and return before native invocation. The attachment remains visible in the tree.
- Sanitize plugin errors through existing App error presentation; do not show credentials or request headers.

**Step 4: Run tests and Rust compile**

```bash
pnpm --filter @markra/desktop exec vitest run src/runtime/tauri/opener.test.ts ../../packages/app/src/App.test.tsx
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
pnpm typecheck:test
```

Expected: all pass and `rg -n "open_external_url|external_urls" apps/desktop/src-tauri/src` has no matches.

**Step 5: Commit**

```bash
git add apps/desktop/src/runtime/tauri/opener.ts apps/desktop/src/runtime/tauri/opener.test.ts apps/desktop/src/runtime/desktop.ts apps/desktop/src/runtime/mobile.ts apps/desktop/src-tauri/capabilities/main.json apps/desktop/src-tauri/src packages/app/src/App.tsx packages/app/src/App.test.tsx packages/shared/src/i18n/locales
git commit -m "feat: open mobile links with tauri opener"
```

## Task 8: Integrate Android Back, Foreground Refresh, and Sync Tree Invalidation

**Files:**

- Create: `apps/desktop/src-tauri/src/mobile_back.rs`
- Create: `apps/desktop/src/runtime/tauri/mobile-back.ts`
- Create: `apps/desktop/src/runtime/tauri/mobile-back.test.ts`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/mobile_runtime.rs`
- Modify: `apps/desktop/src/runtime/mobile.ts`
- Modify: `packages/app/src/hooks/useCompactNavigation.ts`
- Modify: `packages/app/src/hooks/useCompactNavigation.test.tsx`
- Modify: `packages/app/src/hooks/useProjectSyncCoordinator.ts`
- Modify: `packages/app/src/hooks/useProjectSyncCoordinator.test.tsx`
- Modify: `packages/app/src/App.tsx`
- Modify: `packages/app/src/App.test.tsx`

**Step 1: Write failing mobile-back tests**

Use the existing `CompactSystemBackSubscriber` boundary. Test that the mobile adapter:

- listens for `qingyu://mobile-back-requested`;
- awaits the handler and invokes `complete_mobile_back({ consumed: true })` when an overlay is popped;
- invokes `complete_mobile_back({ consumed: false })` at the root editor;
- acknowledges `true` and reports a navigation error if the handler rejects, preventing an accidental exit;
- cleans up the event listener.

Add Rust unit tests for a pure back-decision state machine: only one request may be pending, a consumed acknowledgement clears it, an unconsumed acknowledgement requests `app.exit(0)`, and rapid duplicate exit events coalesce.

**Step 2: Write failing sync-refresh tests**

Extend `ProjectSyncCoordinatorInput` with:

```ts
onFilesChanged?: (projectRoot: string) => Promise<unknown> | unknown;
```

Test exactly one callback after every successful shared sync run, including a deduplicated run shared by callers; no callback on cancelled/failed runs; callback failure does not turn a successful sync into a failed sync. In App tests, assert the callback refreshes the current managed file tree and preserves the active document path.

Retain existing `useCompactAutoSave` tests proving the 1.5-second timer plus `visibilitychange`/`pagehide` flush local content without calling `notifyDocumentSaved` or starting sync.

**Step 3: Run focused tests to verify red**

```bash
pnpm --filter @markra/desktop exec vitest run src/runtime/tauri/mobile-back.test.ts ../../packages/app/src/hooks/useCompactNavigation.test.tsx ../../packages/app/src/hooks/useProjectSyncCoordinator.test.tsx ../../packages/app/src/hooks/useCompactAutoSave.test.tsx ../../packages/app/src/App.test.tsx
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mobile_back -- --nocapture
```

Expected: failures because mobile-back acknowledgement and deterministic sync tree invalidation are not connected.

**Step 4: Implement the native/frontend back handshake**

- Remove the Compact-local synchronous subscriber alias and let `useCompactNavigation` await `AppSystemBackSubscriber`; if the component unmounts before registration resolves, immediately run the resolved cleanup.
- `mobile_back.rs` owns an `AtomicBool` pending flag.
- In the mobile run loop, intercept `RunEvent::ExitRequested { code: None, api, .. }`: if no request is pending, prevent exit and emit `qingyu://mobile-back-requested` to `main`; coalesce any further request while pending.
- Register `complete_mobile_back(consumed: bool)`. It clears pending; `false` calls `app.exit(0)`. The resulting `code: Some(0)` event is not intercepted.
- Expose the TypeScript subscriber through `mobileRuntime.navigation` and pass it to `useCompactNavigation` only when `trueMobile` is active. Existing navigation guards flush pending editor state before the pop.

**Step 5: Implement deterministic foreground file refresh**

- Add `filesChangedNotified: boolean` to `SharedProjectRun` and store `onFilesChanged` in a current ref. After the shared promise succeeds, the first caller flips that flag and awaits `onFilesChanged(root)`; every joined caller skips it. Catch callback errors separately so they do not alter successful sync status.
- In `App.tsx`, destructure `refreshMarkdownFileTree` before creating the sync coordinator, keep the active document path in a ref updated after `useMarkdownDocument`, and pass a callback that calls `refreshMarkdownFileTree(activePath)` only for the current fixed root. This supplements `notify`; it does not poll and does not change sync semantics.
- Keep known create/rename/move/delete operations using their existing immediate tree state updates.
- On document visibility returning to `visible`, refresh the managed tree and, for true mobile only, call `projectSync.run("project-open")`. Existing automatic-trigger barriers and shared-run deduplication remain authoritative; do not add background timers.

**Step 6: Run focused and regression tests**

```bash
pnpm --filter @markra/desktop exec vitest run src/runtime/tauri/mobile-back.test.ts ../../packages/app/src/hooks/useCompactNavigation.test.tsx ../../packages/app/src/hooks/useProjectSyncCoordinator.test.tsx ../../packages/app/src/hooks/useCompactAutoSave.test.tsx ../../packages/app/src/App.test.tsx
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mobile_back -- --nocapture
pnpm test
pnpm typecheck:test
```

Expected: all pass; root editor returns `consumed: false`, overlays pop, autosave remains local-only, and a successful sync refreshes the tree exactly once.

**Step 7: Commit**

```bash
git add apps/desktop/src-tauri/src/mobile_back.rs apps/desktop/src-tauri/src/lib.rs apps/desktop/src-tauri/src/mobile_runtime.rs apps/desktop/src/runtime/tauri/mobile-back.ts apps/desktop/src/runtime/tauri/mobile-back.test.ts apps/desktop/src/runtime/mobile.ts packages/app/src/hooks/useCompactNavigation.ts packages/app/src/hooks/useCompactNavigation.test.tsx packages/app/src/hooks/useProjectSyncCoordinator.ts packages/app/src/hooks/useProjectSyncCoordinator.test.tsx packages/app/src/App.tsx packages/app/src/App.test.tsx
git commit -m "feat: integrate mobile back and workspace refresh"
```

## Task 9: Harden Android/iOS Network and Persistence Configuration

**Files:**

- Create: `apps/desktop/src-tauri/tauri.android.conf.json`
- Create: `apps/desktop/src-tauri/tauri.ios.conf.json`
- Modify: `apps/desktop/src-tauri/gen/android/app/src/main/AndroidManifest.xml`
- Create: `apps/desktop/src-tauri/gen/android/app/src/debug/AndroidManifest.xml`
- Modify: `apps/desktop/src-tauri/gen/apple/project.yml`
- Modify: `apps/desktop/src-tauri/src/managed_workspace.rs`
- Modify: `apps/desktop/src-tauri/src/remote_sync/live_tests.rs`

**Step 1: Add config and workspace tests before changing metadata**

Add Rust/config tests that parse the Android manifest and Apple `project.yml` and assert:

- Android has Internet access;
- release does not globally set `usesCleartextTraffic="true"`;
- any cleartext allowance used for the local MinIO development endpoint exists only in the debug source set;
- iOS declares `NSLocalNetworkUsageDescription` and `NSAppTransportSecurity.NSAllowsLocalNetworking: true` without global arbitrary-load allowance;
- neither platform declares Markdown file associations, SEND/VIEW import intents, share targets, broad external-storage permissions, committed endpoints, or credentials;
- `managed_workspace_path(app_data_root)` is exactly `app_data_root/workspace`, creates persistently, and rejects path escape through every shared file operation test.

**Step 2: Run the config tests to verify red**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mobile_platform_config -- --nocapture
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml managed_workspace -- --nocapture
```

Expected: at least the Apple local-network purpose/ATS tests fail before metadata is added.

**Step 3: Apply minimum platform policy**

- Preserve Android `INTERNET`. Keep release cleartext disabled. Add `android:usesCleartextTraffic="true"` only in `src/debug/AndroidManifest.xml` for the approved LAN MinIO test; do not put the test host or credentials in XML.
- In Apple `project.yml`, add the user-facing local-network purpose string and `NSAllowsLocalNetworking: true`. Do not add `NSAllowsArbitraryLoads`.
- Keep the app-data workspace location unchanged across upgrade and launch. Do not expose reset or switch commands.
- Ensure live S3 tests read only `MARKRA_TEST_S3_ENDPOINT`, `MARKRA_TEST_S3_REGION`, `MARKRA_TEST_S3_BUCKET`, `MARKRA_TEST_S3_ACCESS_KEY_ID`, `MARKRA_TEST_S3_SECRET_ACCESS_KEY`, and `MARKRA_TEST_S3_PREFIX_ROOT` from the environment. Errors and logs must redact the secret and authorization header.

**Step 4: Build both platforms**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml mobile_platform_config -- --nocapture
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml managed_workspace -- --nocapture
pnpm tauri android build --debug --apk --target x86_64 --ci
pnpm tauri ios build --debug --target aarch64-sim --ci --no-sign
```

Expected: tests pass and both installable/simulator artifacts are produced without embedded test endpoint or credentials.

**Step 5: Commit**

```bash
git add apps/desktop/src-tauri/tauri.android.conf.json apps/desktop/src-tauri/tauri.ios.conf.json apps/desktop/src-tauri/gen/android apps/desktop/src-tauri/gen/apple/project.yml apps/desktop/src-tauri/src/managed_workspace.rs apps/desktop/src-tauri/src/remote_sync/live_tests.rs
git commit -m "build: configure mobile network and persistence policy"
```

## Task 10: Execute Native Acceptance, MinIO Coverage, and Desktop Regression

**Files:**

- Create: `docs/testing/qingyu-mobile-native-acceptance.md`
- Create: `docs/testing/qingyu-mobile-native-results.md`

**Step 1: Write the executable acceptance document**

Record prerequisites, commands, expected UI, failure evidence, and a result table for Android emulator, iOS Simulator, Android device, iOS device, desktop native, and narrow browser. The checklist must cover:

- first launch creates exactly one private workspace;
- local-only editing works without sync configuration;
- last document restores; missing last document shows the editor welcome empty state;
- create/rename/move/delete/search/history, name validation, duplicate name, and write failure;
- WYSIWYG typing, Chinese IME composition, system selection/copy/paste, undo/redo, keyboard appearance, and 1.5-second autosave;
- full-screen file browser and settings preserve editor state;
- image picker cancel, permission denial, PNG/JPEG/GIF/WebP/BMP/AVIF/SVG, unsupported data, `content://`, iOS URI, collision, low storage/write failure, restart display, and sync display;
- non-image synced attachment shows one unsupported message and makes no native open call;
- HTTP/HTTPS external links succeed; invalid/opener failure stays in the editor;
- Android hardware/gesture back and iOS/navigation behavior at every Compact page and editor root;
- foreground/background, force close, low-memory recreation, upgrade persistence;
- S3 and WebDAV success, manual sync, auto-on-save setting, invalid endpoint, invalid credentials, offline, DNS, timeout, TLS, config unsupported, conflict copy, checkpoint, and deletion propagation;
- every sync error shows a reason and at most one action that opens sync settings;
- no credential, secret, authorization header, or private endpoint appears in UI logs/diagnostics/screenshots.

`qingyu-mobile-native-results.md` records date, commit, device/OS, artifact path, each case pass/fail, and evidence path. It contains no secrets.

**Step 2: Run all automated gates from a clean tracked tree**

```bash
git status --short
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
pnpm test
pnpm typecheck:test
pnpm build
pnpm tauri android build --debug --apk --target x86_64 --ci
pnpm tauri ios build --debug --target aarch64-sim --ci --no-sign
```

Expected: every command passes. Only intentional untracked `bg.png` may remain outside generated ignored output.

**Step 3: Run live MinIO with local environment injection**

Set the four required and two optional `MARKRA_TEST_S3_*` variables in the invoking shell or local secret manager, then run:

```bash
pnpm test:s3-sync:live
```

Expected: create/upload/download/edit/conflict/delete/checkpoint live cases pass against the configured bucket. Do not paste the values into the acceptance documents, shell history excerpts, screenshots, commits, or final report.

**Step 4: Run simulator and desktop acceptance**

- Install the produced Android APK on the emulator and run every simulator-applicable row.
- Launch the produced iOS Simulator app and run every simulator-applicable row.
- Run `pnpm tauri dev` and verify desktop native menus, settings window, multiwindow, file/folder selection, file association/open path, attachment opening, export/Pandoc, updater surface, system fonts, window restore, and existing project-directory behavior.
- Run the Web app at a phone-sized viewport for Compact layout regression only; label it Web UI evidence, not mobile-native evidence.
- Fill `qingyu-mobile-native-results.md` with exact results and artifact/evidence paths.

**Step 5: Run real-device-only acceptance**

On at least one Android device and one iOS device, execute the rows for IME, selection/clipboard, gallery permission, Android content URI, iOS picker URI, LAN permission, physical/gesture back, low-memory recovery, and app-private file persistence. A failed row blocks completion; fix it with a focused red-green commit and rerun the full affected platform matrix.

**Step 6: Final secret and unsupported-feature audit**

```bash
for mobile_secret_value in "$MARKRA_TEST_S3_ENDPOINT" "$MARKRA_TEST_S3_ACCESS_KEY_ID" "$MARKRA_TEST_S3_SECRET_ACCESS_KEY"; do
  if test -n "$mobile_secret_value" && git grep -F -q -- "$mobile_secret_value"; then
    exit 1
  fi
done
rg -n "networkProxy|network_proxy|window\.prompt" packages apps --glob '!**/target/**' --glob '!**/node_modules/**'
rg -n "arboard|tauri_plugin_window_state|tauri_plugin_updater|tauri_plugin_process|fontdb|create_application_menu|open_settings_window|open_containing_folder|open_markdown_attachment" apps/desktop/src-tauri/src/mobile_runtime.rs
git diff --check
```

Expected: the environment-value loop exits successfully, both searches return no matches in prohibited scope, and `git diff --check` passes.

**Step 7: Commit acceptance evidence**

```bash
git add docs/testing/qingyu-mobile-native-acceptance.md docs/testing/qingyu-mobile-native-results.md
git commit -m "test: document mobile native acceptance"
```

## Final Completion Gate

Before claiming the feature complete, rerun:

```bash
git status --short
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
pnpm test
pnpm typecheck:test
pnpm build
pnpm tauri android build --debug --apk --target x86_64 --ci
pnpm tauri ios build --debug --target aarch64-sim --ci --no-sign
pnpm test:s3-sync:live
git log --oneline -12
```

Completion requires all of the following, not merely a successful Vite build:

- Android APK and iOS Simulator app exist and launch.
- Emulator, Simulator, both real-device-only rows, desktop regression, and local MinIO rows are recorded as passing for the final commit.
- Mobile cannot render or invoke any desktop-only feature.
- Local-only and configured-sync workflows both work in the same fixed persistent workspace.
- Image cancellation/failure never modifies the document or leaves a partial file.
- Sync behavior matches desktop, and a successful sync deterministically refreshes the mobile file tree.
- No secret, local endpoint, signing material, generated cache, or unrelated file is committed.
- `git status --short` contains only intentional untracked files such as `bg.png`.
