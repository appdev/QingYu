# QingYu Compact Mobile Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a phone-appropriate Compact shell to the existing QingYu application while preserving the current document, file, project-sync, conflict, and desktop behavior, and while giving true mobile runtimes one fixed app-private persistent workspace.

**Architecture:** Keep `App.tsx` as the shared controller composition root, add one centralized Compact-mode decision, and route the same document/file/editor/sync actions into either the existing Desktop shell or a new full-screen Compact page stack. Add a narrow runtime bridge for mobile form-factor detection and managed-workspace resolution; reuse the existing project configuration session and sync coordinator; add only Compact-specific local autosave scheduling, save feedback, navigation, and settings presentation.

**Tech Stack:** Tauri v2, Rust 2021, React 19, TypeScript 6, Milkdown, Tailwind CSS 4, Vitest, Testing Library, pnpm workspace.

## Global Constraints

- The approved design in `docs/superpowers/specs/2026-07-17-qingyu-compact-mobile-design.md` is the product source of truth.
- Use `pnpm` for JavaScript and frontend workflows. Keep `pnpm-lock.yaml`; do not add another package-manager lockfile.
- Do not use the TypeScript `void` keyword or operator.
- Do not create a second sync engine or Compact-only conflict policy. WebDAV, S3, deletion propagation, checkpointing, and conflict copies must continue through the existing project sync runtime.
- Do not call `notifyDocumentSaved` from Compact autosave. Only the existing explicit Save action may preserve the current save-trigger sync behavior.
- Do not change the existing desktop minute-based autosave behavior or defaults.
- Use an app-private persistent data directory on true Android/iOS runtimes, never a cache directory. Desktop narrow-window mode must retain the currently opened desktop project; web narrow mode must retain the web runtime and must not gain real remote sync.
- Do not expose workspace reset, app-data clear, cloud deletion, configuration reset, raw JSON editing, configuration export, recent workspaces, or folder selection in Compact.
- Keep existing name-first file creation and `Untitled.md` behavior. Do not implement title-derived rename.
- Keep Compact WYSIWYG-only. Do not render AI, source mode, split view, export, templates, shortcut settings, network proxy, logs, or desktop-only settings.
- Touch targets must be at least 44 CSS pixels. Respect safe areas, the on-screen keyboard, reduced motion, and touch input without depending on hover.
- Prefer Tailwind in new React components. Reserve global CSS for safe-area variables, editor-generated content, and platform-level keyboard/layout polish.
- Preserve unrelated user changes. Do not stage or commit the existing untracked `bg.png`.
- Each task starts with a failing focused test, implements the smallest behavior, runs the focused test, then commits only its files.

---

## Planned File Structure

New focused units:

- `packages/app/src/hooks/useCompactMode.ts`: one form-factor plus viewport decision.
- `packages/app/src/hooks/useManagedWorkspace.ts`: true-mobile fixed-workspace bootstrap only.
- `packages/app/src/hooks/useCompactNavigation.ts`: pure full-screen page-stack reducer and Android/browser-back integration.
- `packages/app/src/hooks/useCompactAutoSave.ts`: 1.5-second debounce, lifecycle flushes, and persistent save status.
- `packages/app/src/components/compact/CompactAppShell.tsx`: Compact composition and page overlays.
- `packages/app/src/components/compact/CompactEditorScreen.tsx`: editor top bar, welcome state, more menu, and toolbar host.
- `packages/app/src/components/compact/CompactEditorToolbar.tsx`: touch formatting actions and keyboard dismissal.
- `packages/app/src/components/compact/CompactFileBrowserScreen.tsx`: full-screen hierarchy and file operations.
- `packages/app/src/components/compact/CompactMoveTargetScreen.tsx`: full-screen move destination selection.
- `packages/app/src/components/compact/CompactSettingsHome.tsx`: supported Compact settings categories only.
- `packages/app/src/components/compact/CompactSettingsDetail.tsx`: category routing and shared preference controls.
- `packages/app/src/components/compact/CompactSyncStatusScreen.tsx`: local/setup/connected/error states.
- `packages/app/src/components/compact/CompactSyncFormScreen.tsx`: WebDAV/S3 configuration editing and recovery draft.
- `packages/app/src/components/compact/types.ts`: the explicit shared controller contract passed out of `App.tsx`.
- `apps/desktop/src-tauri/src/managed_workspace.rs`: mobile-only app-private workspace path resolution.
- `apps/desktop/src/runtime/tauri/managed-workspace.ts`: typed Tauri command adapter.

Existing units remain the business source of truth:

- `packages/app/src/hooks/useMarkdownDocument.ts` continues to own document state and disk writes.
- `packages/app/src/hooks/useMarkdownFileTree.ts` continues to own tree loading and file operations.
- `packages/app/src/hooks/useProjectSyncSettingsSession.ts` continues to own revisioned settings edit sessions.
- `packages/app/src/hooks/useProjectSyncCoordinator.ts` continues to own project-open/manual/save/interval/settings-exit triggers.
- `packages/app/src/components/file-tree/file-tree-model.ts` continues to build and filter the hierarchy.
- `packages/app/src/components/settings/*` continue to own reusable desktop preference controls where their layout is already responsive.
- `packages/editor` continues to own Milkdown commands and task-list semantics.

---

### Task 1: Add Runtime Form-Factor and Managed-Workspace Capabilities

**Files:**
- Modify: `packages/app/src/runtime/index.ts`
- Modify: `packages/app/src/runtime/index.test.ts`
- Modify: `apps/desktop/src/runtime/index.ts`
- Modify: `apps/desktop/src/runtime/index.test.ts`
- Create: `apps/desktop/src/runtime/tauri/managed-workspace.ts`
- Modify: `apps/web/src/runtime/index.ts`
- Create: `apps/desktop/src-tauri/src/managed_workspace.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`

**Interfaces:**
- Produces: `AppFormFactor = "desktop" | "mobile"`.
- Produces: `AppPlatformRuntime.resolveFormFactor(): AppFormFactor`.
- Produces: `AppWorkspaceRuntime.resolveManagedRoot(): Promise<string | null>`.
- Produces: Tauri command `resolve_managed_workspace_root` returning an app-data `workspace` directory only on Android/iOS and `null` on desktop builds.

- [ ] **Step 1: Write failing TypeScript runtime-contract tests**

Add assertions to `packages/app/src/runtime/index.test.ts`:

```ts
it("defaults to desktop form factor without a managed workspace", async () => {
  const runtime = createDefaultAppRuntime();

  expect(runtime.platform.resolveFormFactor()).toBe("desktop");
  await expect(runtime.workspace.resolveManagedRoot()).resolves.toBeNull();
});
```

Add a small exported normalization test beside the desktop runtime:

```ts
expect(normalizeAppFormFactor("android")).toBe("mobile");
expect(normalizeAppFormFactor("ios")).toBe("mobile");
expect(normalizeAppFormFactor("macos")).toBe("desktop");
expect(normalizeAppFormFactor("windows")).toBe("desktop");
```

- [ ] **Step 2: Write failing Rust path tests**

In `managed_workspace.rs`, specify path derivation independently of the Tauri command:

```rust
#[test]
fn managed_workspace_is_a_child_of_persistent_app_data() {
    let root = PathBuf::from("/app-data");
    assert_eq!(managed_workspace_path(&root), root.join("workspace"));
}
```

Run:

```bash
pnpm --filter @markra/app test -- src/runtime/index.test.ts
pnpm --filter @markra/desktop test -- src/runtime/index.test.ts
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml managed_workspace -- --nocapture
```

Expected: FAIL because the new runtime properties and Rust module do not exist.

- [ ] **Step 3: Extend the shared runtime without changing feature flags**

Add these exact contracts in `packages/app/src/runtime/index.ts`:

```ts
export type AppFormFactor = "desktop" | "mobile";

export type AppPlatformRuntime = {
  resolveDesktopOsVersion: () => string | null;
  resolveDesktopPlatform: () => DesktopPlatform | null;
  resolveFormFactor: () => AppFormFactor;
};

export type AppWorkspaceRuntime = {
  resolveManagedRoot: () => Promise<string | null>;
};
```

Add `workspace: AppWorkspaceRuntime` to `AppRuntime`. The default and web implementations return `null`; both report `desktop` form factor. Do not infer mobile from browser user-agent.

- [ ] **Step 4: Implement the Tauri adapters**

In `apps/desktop/src/runtime/index.ts`, normalize `@tauri-apps/plugin-os` platform values:

```ts
export function normalizeAppFormFactor(platform: string | null | undefined): AppFormFactor {
  return platform === "android" || platform === "ios" ? "mobile" : "desktop";
}
```

The workspace adapter is only:

```ts
export function resolveNativeManagedWorkspaceRoot() {
  return invokeNative<string | null>("resolve_managed_workspace_root");
}
```

In Rust, call `app.path().app_data_dir()`, append the fixed `workspace` child, create it recursively, then canonicalize and return it. Gate the real path with `#[cfg(mobile)]`; the non-mobile command body returns `Ok(None)` and must not create a directory.

- [ ] **Step 5: Register and verify the runtime bridge**

Run:

```bash
pnpm --filter @markra/app test -- src/runtime/index.test.ts
pnpm --filter @markra/desktop test -- src/runtime/index.test.ts
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml managed_workspace -- --nocapture
pnpm typecheck:test
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
```

Expected: all focused tests PASS; desktop and web runtimes satisfy `AppRuntime`; Rust formatting is clean.

- [ ] **Step 6: Commit**

```bash
git add packages/app/src/runtime/index.ts packages/app/src/runtime/index.test.ts apps/desktop/src/runtime/index.ts apps/desktop/src/runtime/index.test.ts apps/desktop/src/runtime/tauri/managed-workspace.ts apps/web/src/runtime/index.ts apps/desktop/src-tauri/src/managed_workspace.rs apps/desktop/src-tauri/src/lib.rs
git commit -m "feat(mobile): add managed workspace runtime"
```

---

### Task 2: Centralize Compact Activation and Introduce the Shell Seam

**Files:**
- Create: `packages/app/src/hooks/useCompactMode.ts`
- Create: `packages/app/src/hooks/useCompactMode.test.tsx`
- Create: `packages/app/src/components/compact/types.ts`
- Create: `packages/app/src/components/compact/CompactAppShell.tsx`
- Create: `packages/app/src/components/compact/CompactAppShell.test.tsx`
- Modify: `packages/app/src/App.tsx`
- Modify: `packages/app/src/App.test.tsx`

**Interfaces:**
- Produces: `useCompactMode(): { compact: boolean; formFactor: AppFormFactor; trueMobile: boolean }`.
- Consumes: `getAppRuntime().platform.resolveFormFactor()` and one `matchMedia("(max-width: 720px)")` subscription.
- Produces: `CompactAppController` in `compact/types.ts`, containing only explicit state and callbacks required by Compact components.

- [ ] **Step 1: Test all activation cases before rendering Compact UI**

Cover:

```ts
it.each([
  { formFactor: "mobile", matches: false, compact: true },
  { formFactor: "mobile", matches: true, compact: true },
  { formFactor: "desktop", matches: true, compact: true },
  { formFactor: "desktop", matches: false, compact: false }
])("derives Compact from runtime or viewport", ...);
```

Also verify the hook subscribes once, responds to a media-query change, and removes the listener on unmount.

- [ ] **Step 2: Run focused tests and verify RED**

```bash
pnpm --filter @markra/app test -- src/hooks/useCompactMode.test.tsx src/components/compact/CompactAppShell.test.tsx src/App.test.tsx
```

Expected: FAIL because the hook and shell do not exist and `App` always renders the desktop layout.

- [ ] **Step 3: Implement the mode hook**

Read the runtime form factor synchronously once per mounted app and subscribe to the media query. Return `trueMobile` separately so managed-workspace and mobile lifecycle behavior never activate merely because a desktop window is narrow.

- [ ] **Step 4: Define the Compact controller contract**

Create one named object rather than passing dozens of unrelated top-level props. The initial contract should group existing actions without reimplementing them:

```ts
export type CompactAppController = {
  capabilities: { projectSync: boolean; spellcheck: boolean; trueMobile: boolean };
  document: CompactDocumentController;
  editor: CompactEditorController;
  files: CompactFilesController;
  preferences: CompactPreferencesController;
  project: CompactProjectController;
};
```

Each nested type must use existing return types from `useMarkdownDocument`, `useMarkdownFileTree`, `useEditorController`, project config, and app settings. Do not duplicate file or config document shapes.

- [ ] **Step 5: Route only the render shell in `App.tsx`**

Keep current hooks and effects in `App.tsx`. Construct `compactController` with `useMemo`, then render:

```tsx
return compactMode.compact
  ? <CompactAppShell controller={compactController} />
  : <DesktopAppShellOrExistingLayout ... />;
```

At this task, `CompactAppShell` may show a minimal editor host, but it must not render `NativeTitleBar`, desktop file drawer, AI panel, source/split controls, or bottom navigation. Desktop-wide snapshots and existing tests must remain unchanged.

- [ ] **Step 6: Verify shell switching**

```bash
pnpm --filter @markra/app test -- src/hooks/useCompactMode.test.tsx src/components/compact/CompactAppShell.test.tsx src/App.test.tsx
pnpm typecheck:test
```

Expected: Compact renders at 720px and below or on mobile; 721px desktop renders the existing shell; all tests PASS.

- [ ] **Step 7: Commit**

```bash
git add packages/app/src/hooks/useCompactMode.ts packages/app/src/hooks/useCompactMode.test.tsx packages/app/src/components/compact packages/app/src/App.tsx packages/app/src/App.test.tsx
git commit -m "feat(mobile): add Compact shell boundary"
```

---

### Task 3: Bootstrap the True-Mobile Fixed Workspace and Restore the Last Document

**Files:**
- Create: `packages/app/src/hooks/useManagedWorkspace.ts`
- Create: `packages/app/src/hooks/useManagedWorkspace.test.tsx`
- Modify: `packages/app/src/hooks/useMarkdownFileTree.ts`
- Modify: `packages/app/src/hooks/useMarkdownFileTree.test.tsx`
- Modify: `packages/app/src/lib/settings/workspace-state.ts`
- Modify: `packages/app/src/lib/settings/workspace-state.test.ts`
- Modify: `packages/app/src/App.tsx`

**Interfaces:**
- Produces: `ManagedWorkspaceState = { status: "inactive" | "loading" | "ready" | "error"; root: string | null; error: string | null }`.
- Adds: a managed-root load option that skips folder pickers, recent-workspace mutation, and workspace switching UI while preserving tree watchers and project-sync coordination.
- Stores: last document as a relative path under the managed root; rejects absolute/outside-root restoration.

- [ ] **Step 1: Write failing bootstrap and restoration tests**

Cover these cases:

- `trueMobile=false` never calls `resolveManagedRoot`.
- `trueMobile=true` calls it once and loads that root through the existing tree controller.
- the first empty workspace produces `ready` plus no active file.
- a stored relative Markdown path inside the root is reopened.
- missing, non-Markdown, absolute, `..`, or outside-root stored paths are ignored and produce the welcome empty state.
- managed-root load does not append a recent folder or invoke a picker.

- [ ] **Step 2: Run tests and verify RED**

```bash
pnpm --filter @markra/app test -- src/hooks/useManagedWorkspace.test.tsx src/hooks/useMarkdownFileTree.test.tsx src/lib/settings/workspace-state.test.ts
```

Expected: FAIL because managed bootstrap and relative-path restoration do not exist.

- [ ] **Step 3: Add the managed tree-load option**

Extend the existing programmatic folder-open options rather than creating another loader:

```ts
type OpenFolderPathOptions = {
  managed?: boolean;
  restoreDocumentPath?: string | null;
};
```

When `managed` is true, load and watch the root normally, but do not remember it in recent folders and do not expose it as a switchable source. Keep desktop callers on their current defaults.

- [ ] **Step 4: Implement safe relative-path persistence**

Add pure helpers in `workspace-state.ts`:

```ts
managedDocumentRelativePath(rootPath, filePath): string | null
managedDocumentAbsolutePath(rootPath, relativePath): string | null
```

Normalize separators, require a non-empty `.md`/`.markdown` relative path, reject `.`/`..` segments, and verify the resolved path remains below the managed root. Persist only the relative value for true mobile.

- [ ] **Step 5: Wire startup order in `App.tsx`**

For true mobile: resolve root, load tree, then restore the stored document if it still exists. Do not run the desktop recent-folder startup path first. For desktop narrow and web narrow: retain current startup behavior exactly.

- [ ] **Step 6: Verify**

```bash
pnpm --filter @markra/app test -- src/hooks/useManagedWorkspace.test.tsx src/hooks/useMarkdownFileTree.test.tsx src/lib/settings/workspace-state.test.ts src/App.test.tsx
pnpm typecheck:test
```

Expected: all tests PASS; no desktop recent-folder fixture changes beyond explicit new cases.

- [ ] **Step 7: Commit**

```bash
git add packages/app/src/hooks/useManagedWorkspace.ts packages/app/src/hooks/useManagedWorkspace.test.tsx packages/app/src/hooks/useMarkdownFileTree.ts packages/app/src/hooks/useMarkdownFileTree.test.tsx packages/app/src/lib/settings/workspace-state.ts packages/app/src/lib/settings/workspace-state.test.ts packages/app/src/App.tsx packages/app/src/App.test.tsx
git commit -m "feat(mobile): bootstrap fixed QingYu workspace"
```

---

### Task 4: Implement the Full-Screen Compact Page Stack and Back Behavior

**Files:**
- Create: `packages/app/src/hooks/useCompactNavigation.ts`
- Create: `packages/app/src/hooks/useCompactNavigation.test.tsx`
- Modify: `packages/app/src/components/compact/CompactAppShell.tsx`
- Modify: `packages/app/src/components/compact/CompactAppShell.test.tsx`

**Interfaces:**
- Produces: discriminated `CompactPage` values for editor root, file browser, move target, settings home, settings detail, sync status, and sync form.
- Produces: `push`, `replace`, `pop`, `popToEditor`, and `canGoBack`.
- Consumes: browser `popstate`; leaves room for a native Android-back event adapter without duplicating reducer semantics.

- [ ] **Step 1: Write reducer and integration tests**

Required paths:

```text
editor -> files -> move-target -> files -> editor
editor -> settings -> settings-detail -> settings -> editor
editor -> sync-status -> sync-form -> sync-status -> editor
```

Verify repeated destinations do not create duplicate adjacent stack entries; back at editor root does not close or clear the document; leaving sync form calls the settings-session exit action before popping.

- [ ] **Step 2: Run focused tests and verify RED**

```bash
pnpm --filter @markra/app test -- src/hooks/useCompactNavigation.test.tsx src/components/compact/CompactAppShell.test.tsx
```

- [ ] **Step 3: Implement a pure reducer first**

Use a discriminated union carrying only navigation parameters:

```ts
export type CompactPage =
  | { kind: "editor" }
  | { kind: "files" }
  | { kind: "move-target"; path: string }
  | { kind: "settings" }
  | { kind: "settings-detail"; category: CompactSettingsCategory }
  | { kind: "sync-status" }
  | { kind: "sync-form"; mode: "create" | "edit" | "recover" };
```

The editor is always stack index zero. Page transitions are full-screen replacements layered by the shell, never half-width drawers.

- [ ] **Step 4: Integrate browser and system back**

Push one history marker per Compact page transition in web/desktop-webview. On `popstate`, pop one Compact page. If Tauri Android exposes a dedicated back event during native validation, route it to the same `pop` action; do not add a second navigation state.

- [ ] **Step 5: Verify and commit**

```bash
pnpm --filter @markra/app test -- src/hooks/useCompactNavigation.test.tsx src/components/compact/CompactAppShell.test.tsx
git add packages/app/src/hooks/useCompactNavigation.ts packages/app/src/hooks/useCompactNavigation.test.tsx packages/app/src/components/compact/CompactAppShell.tsx packages/app/src/components/compact/CompactAppShell.test.tsx
git commit -m "feat(mobile): add Compact full-screen navigation"
```

---

### Task 5: Build the Full-Screen File Browser and Move Target

**Files:**
- Create: `packages/app/src/components/compact/CompactFileBrowserScreen.tsx`
- Create: `packages/app/src/components/compact/CompactFileBrowserScreen.test.tsx`
- Create: `packages/app/src/components/compact/CompactMoveTargetScreen.tsx`
- Create: `packages/app/src/components/compact/CompactMoveTargetScreen.test.tsx`
- Modify: `packages/app/src/components/file-tree/file-tree-model.ts`
- Modify: `packages/app/src/components/file-tree/file-tree-model.test.ts`
- Modify: `packages/app/src/components/compact/CompactAppShell.tsx`
- Modify: `packages/app/src/components/compact/types.ts`

**Interfaces:**
- Consumes: existing file-tree entries and create/rename/move/delete/open callbacks from `useMarkdownFileTree`.
- Reuses: `buildMarkdownFileTree`, `filterMarkdownFileTree`, visible-row helpers, and folder-target helpers.
- Produces: name-first file/folder creation, tap-to-open, expandable folders, search, long-press/more actions, and a separate full-screen move destination.

- [ ] **Step 1: Add failing file-browser behavior tests**

Test that:

- the screen covers the Compact shell and has a single Back action;
- tapping a file opens it and returns to editor;
- tapping a folder expands/collapses it;
- search filters through the shared model;
- New File asks for a name first, blank/whitespace cancels, and successful creation opens the file in editor;
- New Folder asks for a name first and stays in the browser;
- rename and delete use existing confirmation/business callbacks;
- move opens `CompactMoveTargetScreen`, never drag-and-drop;
- `.qingyu` and `.markra-sync` never appear;
- every interactive row/action has a 44px minimum target.

- [ ] **Step 2: Add failing move-target model tests**

The target list must exclude the moved folder itself and all descendants, show the project root as a destination, and call the existing move callback exactly once before returning to the file browser.

- [ ] **Step 3: Run tests and verify RED**

```bash
pnpm --filter @markra/app test -- src/components/compact/CompactFileBrowserScreen.test.tsx src/components/compact/CompactMoveTargetScreen.test.tsx src/components/file-tree/file-tree-model.test.ts
```

- [ ] **Step 4: Implement the screens with the shared tree model**

Do not import `MarkdownFileTreeDrawer` and hide its desktop controls. Render a touch-specific view over the same entries. Use pointer timers for long press, cancel the timer on movement/cancel/up, and provide a visible More button so every operation remains keyboard/screen-reader accessible.

- [ ] **Step 5: Preserve current creation semantics**

Call the existing create callbacks only after a non-empty trimmed name is submitted. Do not derive the name from editor contents. If the physical file creation succeeds, call the existing open-tree-file action and pop to editor. Preserve current extension normalization and collision errors.

- [ ] **Step 6: Verify and commit**

```bash
pnpm --filter @markra/app test -- src/components/compact/CompactFileBrowserScreen.test.tsx src/components/compact/CompactMoveTargetScreen.test.tsx src/components/file-tree/file-tree-model.test.ts
pnpm typecheck:test
git add packages/app/src/components/compact packages/app/src/components/file-tree/file-tree-model.ts packages/app/src/components/file-tree/file-tree-model.test.ts
git commit -m "feat(mobile): add Compact file browser"
```

---

### Task 6: Build the Compact Editor Screen, Welcome State, and Action Menu

**Files:**
- Create: `packages/app/src/components/compact/CompactEditorScreen.tsx`
- Create: `packages/app/src/components/compact/CompactEditorScreen.test.tsx`
- Create: `packages/app/src/components/compact/CompactWelcomeState.tsx`
- Create: `packages/app/src/components/compact/CompactEditorMoreMenu.tsx`
- Modify: `packages/app/src/components/compact/CompactAppShell.tsx`
- Modify: `packages/app/src/components/compact/types.ts`
- Modify: `packages/app/src/App.tsx`
- Modify: `packages/shared/src/i18n/locales/types.ts`
- Modify: `packages/shared/src/i18n/locales/en.ts`
- Modify: `packages/shared/src/i18n/locales/zh-CN.ts`
- Modify: remaining strict locale dictionaries under `packages/shared/src/i18n/locales/`
- Modify: `packages/shared/src/i18n/index.test.ts`

**Interfaces:**
- Top bar: Files, current filename plus persistent save state, and More.
- Welcome state: New Document and Configure Sync; no picker/recent-workspace action.
- More menu: explicit Save, Find, History, Sync/Configure Sync, and Settings.
- Editor host: current Milkdown WYSIWYG content only.

- [ ] **Step 1: Write failing editor-screen tests**

Test saved/saving/error filename states, menu actions, hidden desktop capabilities, and welcome behavior. Explicitly assert there is no AI, source, split, export, folder picker, or bottom-navigation control.

- [ ] **Step 2: Run and verify RED**

```bash
pnpm --filter @markra/app test -- src/components/compact/CompactEditorScreen.test.tsx
```

- [ ] **Step 3: Implement the welcome state**

Show it only when startup restoration produced no active document. `New Document` must invoke the current blank-document flow and preserve current naming behavior. `Configure Sync` is shown only when `capabilities.projectSync` is true; web narrow mode may show a clear unavailable/local-only state instead of a working remote form.

- [ ] **Step 4: Implement top bar and More menu**

Use Lucide icons with accessible labels. Display filename and one of `已保存`, `保存中`, or the exact save error summary. The More menu delegates to existing Save, find, history, manual sync, and settings actions. Explicit Save continues through the App wrapper that calls `projectSync.notifyDocumentSaved`; do not route it to the new autosave hook.

- [ ] **Step 5: Render only visual Milkdown mode**

Reuse the current visual editor element/controller. In Compact, force the presented mode to WYSIWYG without overwriting the user’s desktop view-mode preference. Source/split state remains untouched and reappears when the desktop shell returns.

- [ ] **Step 6: Add translation keys**

Add typed keys for Compact navigation, empty state, save states, file operations, supported settings, sync states, and errors. Provide reviewed Simplified Chinese and English copy; keep every strict locale dictionary type-complete with its existing fallback convention.

- [ ] **Step 7: Verify and commit**

```bash
pnpm --filter @markra/app test -- src/components/compact/CompactEditorScreen.test.tsx src/App.test.tsx
pnpm --filter @markra/shared test -- src/i18n/index.test.ts
pnpm typecheck:test
git add packages/app/src/components/compact packages/app/src/components/compact/types.ts packages/app/src/App.tsx packages/shared/src/i18n
git commit -m "feat(mobile): add Compact editor screen"
```

---

### Task 7: Add the Touch Editor Toolbar and a Real Task-List Command

**Files:**
- Modify: `packages/editor/src/task-list.ts`
- Create: `packages/editor/src/task-list.test.ts`
- Modify: `packages/editor/src/index.ts`
- Modify: `packages/app/src/hooks/useEditorController.ts`
- Modify: `packages/app/src/hooks/useEditorController.test.ts`
- Create: `packages/app/src/components/compact/CompactEditorToolbar.tsx`
- Create: `packages/app/src/components/compact/CompactEditorToolbar.test.tsx`
- Modify: `packages/app/src/components/compact/CompactEditorScreen.tsx`
- Modify: `packages/app/src/components/compact/types.ts`

**Interfaces:**
- Produces toolbar actions: undo, redo, paragraph/H1/H2/H3, bold, italic, strike, inline code, link, bullet list, ordered list, task list, quote, code block, image, and dismiss keyboard.
- Reuses existing editor shortcut and insertion actions for every supported command except task-list conversion.
- Produces a Milkdown task-list command that sets list-item `checked` attributes through a transaction rather than inserting literal Markdown into the visual editor.

- [ ] **Step 1: Specify task-list conversion in failing editor tests**

Test converting the current paragraph/list selection to an unchecked task list, toggling an already selected task list back to a normal bullet list, preserving text content, and serializing to `- [ ] item`.

- [ ] **Step 2: Specify toolbar delegation in failing component tests**

Verify each button calls exactly one controller action; heading selection uses the existing heading-level method; link/image use existing insertion flows; keyboard dismissal blurs the active editable element; disabled actions expose `aria-disabled`; horizontal overflow does not shrink targets below 44px.

- [ ] **Step 3: Run tests and verify RED**

```bash
pnpm --filter @markra/editor test -- src/task-list.test.ts
pnpm --filter @markra/app test -- src/hooks/useEditorController.test.ts src/components/compact/CompactEditorToolbar.test.tsx
```

- [ ] **Step 4: Implement one editor-level task-list command**

Keep schema/plugin code in `packages/editor`. Expose a command function consumed by `useEditorController`; do not add a fake global keyboard shortcut or modify full `KeyboardShortcutBindings` fixtures. The command must use ProseMirror list transforms and `checked: false` item attributes.

- [ ] **Step 5: Implement the toolbar**

Render it only while the editor is active in Compact. Anchor it above `env(safe-area-inset-bottom)` and the effective visual viewport/keyboard inset. Use a horizontally scrollable row, clear selected states, and pointer-down handling that preserves the editor selection before dispatching formatting.

- [ ] **Step 6: Verify and commit**

```bash
pnpm --filter @markra/editor test -- src/task-list.test.ts
pnpm --filter @markra/app test -- src/hooks/useEditorController.test.ts src/components/compact/CompactEditorToolbar.test.tsx src/components/compact/CompactEditorScreen.test.tsx
pnpm typecheck:test
git add packages/editor/src packages/app/src/hooks/useEditorController.ts packages/app/src/hooks/useEditorController.test.ts packages/app/src/components/compact
git commit -m "feat(mobile): add Compact editor toolbar"
```

---

### Task 8: Add Compact 1.5-Second Local Autosave with Persistent Failure Feedback

**Files:**
- Modify: `packages/app/src/hooks/useMarkdownDocument.ts`
- Modify: `packages/app/src/hooks/useMarkdownDocument.test.tsx`
- Create: `packages/app/src/hooks/useCompactAutoSave.ts`
- Create: `packages/app/src/hooks/useCompactAutoSave.test.tsx`
- Modify: `packages/app/src/App.tsx`
- Modify: `packages/app/src/components/compact/CompactEditorScreen.tsx`
- Modify: `packages/app/src/components/compact/types.ts`

**Interfaces:**
- Changes: `saveDirtyMarkdownFiles()` returns a result instead of swallowing success detail, while retaining single-flight behavior.
- Produces: `CompactSaveState = { status: "saved" | "dirty" | "saving" | "error"; error: string | null; retry(): Promise<unknown>; flush(reason): Promise<unknown> }`.
- Debounce: 1500 ms after the latest document edit.
- Flushes before: document switch, file browser/settings navigation, app background, `visibilitychange` hidden, and `pagehide`.

- [ ] **Step 1: Write failing document-save contract tests**

Update tests to require `saveDirtyMarkdownFiles` to reject on a disk failure and return the saved file paths on success. Assert it still does not call `notifyDocumentSaved` or any sync event.

- [ ] **Step 2: Write fake-timer autosave tests**

Cover:

- no save before 1499 ms; one save at 1500 ms;
- subsequent edits restart the timer;
- concurrent requests share one in-flight flush;
- navigation/background flush immediately cancels the timer;
- untitled/no-path documents persist draft state but do not invoke a native file write;
- failure leaves a persistent error and Retry action;
- successful retry moves to saved;
- explicit Save remains outside the hook;
- disabled Compact mode never schedules a 1.5-second save;
- desktop interval autosave tests retain their existing timing.

- [ ] **Step 3: Run and verify RED**

```bash
pnpm --filter @markra/app test -- src/hooks/useMarkdownDocument.test.tsx src/hooks/useCompactAutoSave.test.tsx
```

- [ ] **Step 4: Make the existing local save primitive observable**

Return `SavedNativeMarkdownFile[]` from `saveDirtyMarkdownFiles`. Keep `autoSaveDirtyMarkdownTabs` as the desktop interval wrapper that logs failures exactly as today. Do not add sync notification inside either function.

- [ ] **Step 5: Implement Compact scheduling and lifecycle flushes**

Track a monotonic edit revision rather than comparing Markdown strings in the timer. Serialize flushes, retain the latest pending revision, and only mark `saved` when the latest revision has completed. Convert thrown values to a stable, user-visible reason without leaking credentials or config content.

Use `visibilitychange`, `pagehide`, and Tauri mobile lifecycle events if available. Browser lifecycle handlers may begin the async flush but must not claim guaranteed completion after process termination.

- [ ] **Step 6: Gate navigation on attempted flush, not silent success**

Before switching documents or opening files/settings, await `flush`. If it fails, keep the error visible but allow the user to continue after the write attempt; the current draft persistence remains the recovery layer. Do not show a destructive discard prompt for ordinary autosave failure.

- [ ] **Step 7: Verify and commit**

```bash
pnpm --filter @markra/app test -- src/hooks/useMarkdownDocument.test.tsx src/hooks/useCompactAutoSave.test.tsx src/components/compact/CompactEditorScreen.test.tsx
pnpm typecheck:test
git add packages/app/src/hooks/useMarkdownDocument.ts packages/app/src/hooks/useMarkdownDocument.test.tsx packages/app/src/hooks/useCompactAutoSave.ts packages/app/src/hooks/useCompactAutoSave.test.tsx packages/app/src/App.tsx packages/app/src/components/compact
git commit -m "feat(mobile): add Compact local autosave"
```

---

### Task 9: Add Non-Destructive Invalid-Configuration Recovery

**Files:**
- Modify: `apps/desktop/src-tauri/src/project_config/model.rs`
- Modify: `apps/desktop/src-tauri/src/project_config/storage.rs`
- Modify: `apps/desktop/src-tauri/src/project_config.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `packages/app/src/lib/project-config.ts`
- Modify: `packages/app/src/lib/project-config.test.ts`
- Modify: `packages/app/src/runtime/index.ts`
- Modify: `apps/desktop/src/runtime/tauri/project-config.ts`
- Modify: `packages/app/src/hooks/useProjectSyncSettingsSession.ts`
- Modify: `packages/app/src/hooks/useProjectSyncSettingsSession.test.tsx`

**Interfaces:**
- Produces: `recover_project_config` and `AppProjectConfigRuntime.recover`.
- Produces: a redacted `ProjectConfigLoadIssue` (`code` plus user-safe `message`) on malformed and unsupported load results so Compact can show an exact cause without exposing file contents or credentials.
- Accepts: project root, exact current malformed/unsupported revision, and a complete valid configuration draft.
- Behavior: create an internal `config.invalid-<timestamp>-<sequence>.json` backup, atomically replace `config.json`, return the new document and revision.
- Does not expose: reset-to-defaults, raw invalid JSON, backup export, workspace reset, or cloud deletion.

- [ ] **Step 1: Write failing native recovery tests**

Test malformed JSON, unsupported future version, stale revision, valid config, backup collision sequencing, atomic-replace fault injection, and symlink/no-follow rejection. Malformed and unsupported responses must include stable issue codes and clear safe messages, but never raw JSON, URLs, usernames, passwords, keys, or secrets. Assert failed recovery preserves original bytes. Assert successful recovery preserves the invalid bytes only in the internal backup and writes the user’s submitted complete draft.

- [ ] **Step 2: Write failing TypeScript session tests**

Require `normalizeProjectConfigLoadResult` and `begin()` on malformed/unsupported results to preserve the safe issue and revision, `recover(draft)` to queue through the same serialized session write tail, and `end()` to send at most one settings-exit apply for the final recovered revision.

- [ ] **Step 3: Run and verify RED**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml project_config -- --nocapture
pnpm --filter @markra/app test -- src/lib/project-config.test.ts src/hooks/useProjectSyncSettingsSession.test.tsx
```

- [ ] **Step 4: Implement the narrow recovery API**

Reuse the existing storage validation, revision hash, invalid-backup naming, no-follow checks, temporary file, sync, and atomic replacement helpers. Do not call the existing `reset` API from Compact and do not add a Compact reset button. Leave the desktop reset API intact for existing desktop behavior.

- [ ] **Step 5: Extend the settings session**

Add:

```ts
recover: (config: QingYuProjectConfig) => Promise<unknown>;
```

It is valid only when the current load status is `malformed` or `unsupported`. On success, update the session revision/document, remain in editing state, and mark dirty until the existing settings-exit apply completes.

- [ ] **Step 6: Verify and commit**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml project_config -- --nocapture
pnpm --filter @markra/app test -- src/lib/project-config.test.ts src/hooks/useProjectSyncSettingsSession.test.tsx
pnpm typecheck:test
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
git add apps/desktop/src-tauri/src/project_config* apps/desktop/src-tauri/src/lib.rs packages/app/src/lib/project-config.ts packages/app/src/lib/project-config.test.ts packages/app/src/runtime/index.ts apps/desktop/src/runtime/tauri/project-config.ts packages/app/src/hooks/useProjectSyncSettingsSession.ts packages/app/src/hooks/useProjectSyncSettingsSession.test.tsx
git commit -m "feat(sync): add safe project config recovery"
```

---

### Task 10: Build Compact Sync Status and Configuration Screens

**Files:**
- Create: `packages/app/src/hooks/useCompactSyncSettings.ts`
- Create: `packages/app/src/hooks/useCompactSyncSettings.test.tsx`
- Create: `packages/app/src/components/compact/CompactSyncStatusScreen.tsx`
- Create: `packages/app/src/components/compact/CompactSyncStatusScreen.test.tsx`
- Create: `packages/app/src/components/compact/CompactSyncFormScreen.tsx`
- Create: `packages/app/src/components/compact/CompactSyncFormScreen.test.tsx`
- Modify: `packages/app/src/components/compact/CompactAppShell.tsx`
- Modify: `packages/app/src/components/compact/types.ts`
- Modify: `packages/app/src/App.tsx`

**Interfaces:**
- States: local, setup/incomplete, connected/status, and error.
- Error UI: exact safe error reason plus at most one primary button labeled Configure Sync.
- Form: WebDAV/S3 provider and the existing project config fields, connection test, enabled, save-trigger, and interval fields supported by the current config schema.
- Recovery: malformed/unsupported opens a blank/default draft and submits through `recover`, never through reset.

- [ ] **Step 1: Write failing controller tests**

Extract only the sync-specific logic currently embedded in `useSettingsWindowState`: project-root matching, status load/listening, edit-session begin/end, serialized patching, manual run, and connection test. Verify no behavior divergence in existing desktop sync settings tests.

- [ ] **Step 2: Write failing screen tests for every state**

Required assertions:

- absent config: local mode, no network request, one Configure Sync action;
- incomplete config: clear missing-field reasons and Configure Sync;
- ready idle/attempting/succeeded/failed status: current provider, last result, safe summary, manual Sync Now where applicable;
- malformed/unsupported: explicit cause and exactly one Configure Sync button;
- web runtime: local-only/unavailable explanation and no editable remote form;
- no reset, clear-data, raw JSON, export, cloud-delete, or workspace-switch controls anywhere.

- [ ] **Step 3: Run and verify RED**

```bash
pnpm --filter @markra/app test -- src/hooks/useCompactSyncSettings.test.tsx src/components/compact/CompactSyncStatusScreen.test.tsx src/components/compact/CompactSyncFormScreen.test.tsx src/hooks/useSettingsWindowState.test.ts
```

- [ ] **Step 4: Extract a shared sync settings controller**

`useCompactSyncSettings` may be named more generally if desktop also consumes it, but it must compose `useProjectSyncSettingsSession` rather than duplicate its revision/editing logic. Refactor the desktop Settings hook to use the same status/session controller, keeping its output contract and tests stable.

- [ ] **Step 5: Implement status and form screens**

Use full-screen Compact navigation. Patch fields through the existing typed `ProjectConfigPatch` calls. Keep the session active while the form is open, suspending automatic triggers. On Back/Done, await `end("category-leave")`; the existing settings-exit event applies the final revision once.

For `recover`, initialize a fresh default draft with the provider selected by the user. Show the load error reason outside the form, not the raw file contents. Submit the complete draft through the new recovery API.

- [ ] **Step 6: Verify and commit**

```bash
pnpm --filter @markra/app test -- src/hooks/useCompactSyncSettings.test.tsx src/components/compact/CompactSyncStatusScreen.test.tsx src/components/compact/CompactSyncFormScreen.test.tsx src/hooks/useSettingsWindowState.test.ts src/components/settings/SyncSettings.test.tsx
pnpm typecheck:test
git add packages/app/src/hooks packages/app/src/components/compact packages/app/src/components/settings packages/app/src/App.tsx
git commit -m "feat(mobile): add Compact sync settings"
```

---

### Task 11: Build the Curated Compact Settings Surface

**Files:**
- Create: `packages/app/src/components/compact/CompactSettingsHome.tsx`
- Create: `packages/app/src/components/compact/CompactSettingsHome.test.tsx`
- Create: `packages/app/src/components/compact/CompactSettingsDetail.tsx`
- Create: `packages/app/src/components/compact/CompactSettingsDetail.test.tsx`
- Create: `packages/app/src/lib/compact-settings.ts`
- Create: `packages/app/src/lib/compact-settings.test.ts`
- Modify: `packages/app/src/components/compact/CompactAppShell.tsx`
- Modify: `packages/app/src/components/compact/types.ts`

**Interfaces:**
- Supported categories: General, Storage (read-only), Sync, Appearance, Editor subset, and Spellcheck only when runtime-supported.
- Hidden categories: AI/providers, Web Search, View customization, Backup, Templates, Keyboard Shortcuts, Export, Network, Runtime Logs, and desktop shell/system integration.

- [ ] **Step 1: Write a pure category-policy test**

```ts
expect(compactSettingsCategories({ projectSync: true, spellcheck: true }))
  .toEqual(["general", "storage", "sync", "appearance", "editor", "spellcheck"]);
expect(compactSettingsCategories({ projectSync: false, spellcheck: false }))
  .toEqual(["general", "storage", "appearance", "editor"]);
```

Assert every forbidden desktop category is absent.

- [ ] **Step 2: Write failing screen tests**

Verify settings home is full screen, categories have 44px targets, Storage shows the fixed managed directory read-only on true mobile, desktop narrow shows the current project path read-only, and each detail page returns to Settings without discarding preference changes.

- [ ] **Step 3: Run and verify RED**

```bash
pnpm --filter @markra/app test -- src/lib/compact-settings.test.ts src/components/compact/CompactSettingsHome.test.tsx src/components/compact/CompactSettingsDetail.test.tsx
```

- [ ] **Step 4: Reuse existing preference sources, not the desktop shell**

Use the current app language/theme/editor preference setters. Reuse small settings controls where responsive; compose Compact detail rows around them when desktop sections contain unsupported fields. The editor subset must include only phone-relevant typography/layout/editing controls approved by the design. Do not mount hidden desktop categories and conceal them with CSS.

- [ ] **Step 5: Keep storage informational**

Show path, local file count/size when already available, and local/sync mode. Do not add Browse, Reveal, Switch, Reset, Clear App Data, or Delete Cloud actions.

- [ ] **Step 6: Verify and commit**

```bash
pnpm --filter @markra/app test -- src/lib/compact-settings.test.ts src/components/compact/CompactSettingsHome.test.tsx src/components/compact/CompactSettingsDetail.test.tsx src/components/settings/GeneralSettings.test.tsx src/components/settings/AppearanceSettings.test.tsx src/components/settings/EditorSettings.test.tsx src/components/settings/SpellcheckSettings.test.tsx
pnpm typecheck:test
git add packages/app/src/lib/compact-settings.ts packages/app/src/lib/compact-settings.test.ts packages/app/src/components/compact
git commit -m "feat(mobile): add curated Compact settings"
```

---

### Task 12: Add Safe-Area, Keyboard, Touch, and Reduced-Motion Polish

**Files:**
- Modify: `packages/app/src/styles.css`
- Modify: `packages/app/src/styles.test.ts`
- Create: `packages/app/src/hooks/useVisualViewport.ts`
- Create: `packages/app/src/hooks/useVisualViewport.test.tsx`
- Modify: `packages/app/src/components/compact/CompactAppShell.tsx`
- Modify: all Compact screen components as needed

**Interfaces:**
- Produces CSS variables for top/bottom safe area and visual-viewport keyboard inset.
- Produces a single visual-viewport hook with resize/scroll cleanup.
- Applies 44px minimum targets, focus-visible styles, scroll containment, and reduced-motion fallbacks.

- [ ] **Step 1: Write failing viewport and style tests**

Test viewport resize updates the inset, missing `visualViewport` falls back safely, listeners clean up, and styles contain `env(safe-area-inset-top)`, `env(safe-area-inset-bottom)`, `@media (prefers-reduced-motion: reduce)`, and Compact-specific overscroll/keyboard rules. Component tests must assert semantic dialogs/pages, labels, and target-size classes.

- [ ] **Step 2: Run and verify RED**

```bash
pnpm --filter @markra/app test -- src/hooks/useVisualViewport.test.tsx src/styles.test.ts src/components/compact
```

- [ ] **Step 3: Implement the platform polish**

Set variables at the Compact shell root and use them in top bars, content padding, and keyboard toolbar. Avoid globally changing desktop `html/body` behavior; scope scroll locking and overscroll rules to a `data-compact="true"` root. Replace hover-only affordances with visible buttons and focus/pressed states.

- [ ] **Step 4: Verify and commit**

```bash
pnpm --filter @markra/app test -- src/hooks/useVisualViewport.test.tsx src/styles.test.ts src/components/compact
pnpm typecheck:test
git add packages/app/src/styles.css packages/app/src/styles.test.ts packages/app/src/hooks/useVisualViewport.ts packages/app/src/hooks/useVisualViewport.test.tsx packages/app/src/components/compact
git commit -m "fix(mobile): polish Compact touch layout"
```

---

### Task 13: Run Cross-Shell Regression and Native Acceptance

**Files:**
- Modify: `packages/app/src/App.test.tsx`
- Create: `packages/app/src/components/compact/CompactAcceptance.test.tsx`
- Modify: `docs/superpowers/specs/2026-07-17-qingyu-compact-mobile-design.md` only if implementation-discovered wording needs a non-behavioral clarification

- [ ] **Step 1: Add an integration-level acceptance suite**

Cover the approved critical paths with runtime fakes:

1. true mobile starts with an empty fixed workspace and welcome state;
2. New Document uses current naming behavior, edits locally autosave after 1.5 seconds, and Save explicitly preserves save-trigger sync;
3. Files opens full screen, creates a named file, opens it, moves it through a full-screen target, and returns to editor;
4. local mode never sends a sync request;
5. valid WebDAV/S3 configuration uses the current coordinator and status events;
6. malformed/unsupported configuration shows reason plus one Configure Sync action, then recovers through internal backup and atomic replacement;
7. conflict and deletion results are displayed from the existing sync summary without Compact-specific interpretation;
8. desktop at 721px keeps the current layout and desktop autosave;
9. desktop at 720px uses Compact with the current desktop project, not the managed mobile root;
10. web at 720px uses Compact local web behavior but cannot run remote project sync.

- [ ] **Step 2: Run the full automated verification gate**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
pnpm test
pnpm typecheck:test
pnpm build
git diff --check
```

Expected: every command exits 0. The production build includes Compact chunks/styles without new TypeScript, Rust, Tailwind, or i18n errors.

- [ ] **Step 3: Run live S3 sync coverage when the configured MinIO server is available**

```bash
pnpm test:s3-sync:live
```

Expected: PASS against the configured real MinIO service. If credentials/server are unavailable, record that the gate was skipped with the exact reason; never add credentials to the repository.

- [ ] **Step 4: Validate desktop-wide and simulated phone UI in the current system**

Start the real desktop shell:

```bash
pnpm tauri dev
```

Verify at minimum:

- desktop width above 720px is visually and behaviorally unchanged;
- 390x844 and 360x800 show editor plus full-screen overlays, not half drawers;
- landscape phone dimensions remain Compact;
- empty state, name-first create, autosave states, file move, settings subset, local mode, configured sync, and config error are usable;
- no Compact screen exposes reset or cloud deletion;
- returning above 720px restores desktop shell state without reopening a different project.

- [ ] **Step 5: Validate native mobile builds when toolchains/devices are available**

Run the relevant Tauri mobile development target configured in this checkout (Android first when both are available). On-device checks:

- resolved workspace is under persistent app data, not cache;
- first launch is empty; relaunch restores the last valid document;
- OS background/foreground attempts an autosave flush;
- Android Back pops Compact pages; iOS edge-back follows the same stack where the webview/native shell supports it;
- soft keyboard does not cover the formatting toolbar or active paragraph;
- clearing app data creates a fresh local workspace on next launch and does not issue any cloud deletion request;
- WebDAV/S3 sync and conflict copies match desktop behavior.

If a mobile toolchain or device is unavailable, record this as an explicit unverified acceptance gate, not a successful check.

- [ ] **Step 6: Review scope and final diff**

```bash
git status --short
git log --oneline --decorate -15
git diff --stat a51d815..HEAD
rg -n "reset|clear app|delete cloud|folder picker|recent workspace" packages/app/src/components/compact
```

Expected: only approved Compact/runtime/recovery changes are present; `bg.png` remains untracked and unstaged; prohibited controls are absent (translation strings used only in negative tests are acceptable).

- [ ] **Step 7: Commit final acceptance tests/document clarification**

```bash
git add packages/app/src/App.test.tsx packages/app/src/components/compact/CompactAcceptance.test.tsx
git diff --cached --check
git commit -m "test(mobile): cover Compact acceptance paths"
```

Do not push. Pushing `main` or any feature branch requires a separate explicit user request.

---

## Definition of Done

- Wide desktop retains current UI, project selection, save cadence, and sync behavior.
- Desktop/browser width at or below 720px renders Compact; true mobile remains Compact in any orientation.
- True mobile uses exactly one app-private persistent workspace and restores the last valid document relative to that root.
- No document produces the approved welcome empty state.
- Files and settings are full-screen pages; move target is full screen; there is no half drawer or bottom navigation.
- File creation remains name-first and blank names cancel.
- Compact is WYSIWYG-only and excludes the approved non-goals.
- Compact local autosave waits 1.5 seconds, flushes on transitions/lifecycle, reports persistent failures, and never triggers project save-sync.
- Explicit Save preserves current desktop save-trigger sync semantics.
- Local mode makes no network sync request.
- Valid project configs use the unchanged sync engine and conflict handling.
- Malformed/unsupported configs show a clear reason and one Configure Sync action; recovery backs up internally and atomically replaces the file without exposing reset.
- Settings edit sessions suspend automatic triggers and apply once on exit.
- Touch, safe-area, keyboard, accessibility, and reduced-motion checks pass.
- Rust tests, `pnpm test`, `pnpm typecheck:test`, and `pnpm build` pass; live MinIO and native device results are explicitly recorded.
- No push occurs without explicit authorization.
