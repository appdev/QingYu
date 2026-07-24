# QingYu Primary Workspace and Onboarding Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give each device one durable primary notes workspace, present native desktop/mobile first-use flows, and keep external editing contexts isolated from that workspace.

**Architecture:** Split portable preferences in `settings.json` from device-local state in `local-state.json`, then expose the selected primary workspace through a focused hook. The main window consumes that hook; external file/folder actions always create independent editor windows. A dedicated onboarding component renders desktop and true-mobile semantics while CSS only adapts layout.

**Tech Stack:** React 19, TypeScript, Vitest, Testing Library, Tauri v2 runtime adapters, Tailwind CSS, lucide-react.

## Global Constraints

- The primary notes workspace contains no QingYu configuration or generated state.
- Desktop selects one canonical filesystem directory; true mobile uses app-data `workspace/`.
- Runtime form factor chooses product semantics; viewport width changes layout only.
- The visible slogan is exactly `明窗净几，字字轻语。`.
- True mobile has one onboarding action, `创建并开始`; it has no step counter, arbitrary directory picker, external-folder action, or skip action.
- Narrow desktop retains desktop directory and external-editing actions.
- External files and folders never replace the primary workspace and never complete primary-directory selection.
- Changing the primary workspace never moves or migrates files.
- `settings.json` is portable; `local-state.json` is device-local and is never synchronized.
- Do not read, migrate, rewrite, or delete old `.qingyu/` or `.markra-sync/` data.
- Do not add dependencies.
- Do not use the TypeScript `void` keyword or operator.

---

## File Structure

- `packages/app/src/lib/settings/local-state.ts`: owns the `local-state.json` schema, normalization, primary-workspace state, local executable paths, and local store access.
- `packages/app/src/lib/settings/app-settings.ts`: owns only portable settings and delegates workspace/recent/local-path persistence to `local-state.ts`.
- `packages/app/src/hooks/usePrimaryWorkspace.ts`: resolves desktop or true-mobile primary workspace state and performs select/defer/retry/switch actions.
- `packages/app/src/components/onboarding/WelcomeScreen.tsx`: renders approved desktop and true-mobile onboarding without owning persistence.
- `packages/app/src/components/settings/NotesWorkspaceSettings.tsx`: displays and switches the application-level primary workspace.
- `packages/app/src/lib/editor-assets.ts`: chooses primary-root `assets/` versus external-document resource behavior.
- `packages/app/src/App.tsx`: installs the primary workspace into the main window and routes external actions to new windows.

### Task 1: Separate Device-Local State from Portable Settings

**Files:**
- Create: `packages/app/src/lib/settings/local-state.ts`
- Create: `packages/app/src/lib/settings/local-state.test.ts`
- Modify: `packages/app/src/lib/settings/app-settings.ts`
- Modify: `packages/app/src/lib/settings/app-settings.test.ts`
- Modify: `packages/app/src/lib/settings/export-settings.ts`
- Modify: `packages/app/src/lib/settings/export-settings.test.ts`
- Modify: `packages/app/src/runtime/index.ts`
- Modify: `apps/desktop/src-tauri/src/app_settings.rs`

**Interfaces:**
- Produces: `loadPrimaryWorkspaceState(): Promise<PrimaryWorkspaceState>`.
- Produces: `savePrimaryWorkspaceState(state: PrimaryWorkspaceState): Promise<PrimaryWorkspaceState>`.
- Produces: `updatePrimaryWorkspaceState(change: Partial<Omit<PrimaryWorkspaceState, "version">>): Promise<PrimaryWorkspaceState>`.
- Produces: `loadLocalPandocPath(): Promise<string>` and `saveLocalPandocPath(path: string): Promise<string>`.
- Preserves existing workspace/recent-file function signatures while changing their backing store to `local-state.json`.

- [ ] **Step 1: Write failing local-state tests**

```ts
it("stores primary workspace state only in local-state.json", async () => {
  await savePrimaryWorkspaceState({
    desktopPath: "/Users/test/Notes",
    onboardingCompleted: true,
    version: 1
  });

  expect(mockedLoadStore).toHaveBeenCalledWith("local-state.json", {
    autoSave: false,
    defaults: {}
  });
  expect(storeValue("local-state.json", "primaryWorkspace")).toEqual({
    desktopPath: "/Users/test/Notes",
    onboardingCompleted: true,
    version: 1
  });
  expect(storeValue("local-state.json", "schemaVersion")).toBe(1);
  expect(storeValue("settings.json", "primaryWorkspace")).toBeUndefined();
});

it("normalizes malformed state without reading legacy settings keys", async () => {
  seedStore("settings.json", "workspace", { folderPath: "/legacy" });
  seedStore("local-state.json", "primaryWorkspace", { version: 9, desktopPath: 7 });

  await expect(loadPrimaryWorkspaceState()).resolves.toEqual({
    desktopPath: null,
    onboardingCompleted: false,
    version: 1
  });
});
```

- [ ] **Step 2: Run the focused tests and verify red state**

Run: `pnpm --filter @markra/app exec vitest run src/lib/settings/local-state.test.ts src/lib/settings/app-settings.test.ts src/lib/settings/export-settings.test.ts --environment jsdom --globals`

Expected: FAIL because `local-state.ts` and the new exports do not exist.

- [ ] **Step 3: Implement the local schema and atomic store writes**

```ts
const localStateStorePath = "local-state.json";
const localStateSchemaVersionKey = "schemaVersion";
const primaryWorkspaceKey = "primaryWorkspace";

export type PrimaryWorkspaceState = {
  desktopPath: string | null;
  onboardingCompleted: boolean;
  version: 1;
};

export const defaultPrimaryWorkspaceState: PrimaryWorkspaceState = {
  desktopPath: null,
  onboardingCompleted: false,
  version: 1
};

export function normalizePrimaryWorkspaceState(value: unknown): PrimaryWorkspaceState {
  if (!value || typeof value !== "object") return defaultPrimaryWorkspaceState;
  const candidate = value as Partial<PrimaryWorkspaceState>;
  if (candidate.version !== 1) return defaultPrimaryWorkspaceState;
  return {
    desktopPath: normalizeNullableString(candidate.desktopPath),
    onboardingCompleted: candidate.onboardingCompleted === true,
    version: 1
  };
}

async function localStore() {
  return getAppRuntime().settings.loadStore(localStateStorePath, {
    autoSave: false,
    defaults: {}
  });
}

export async function savePrimaryWorkspaceState(state: PrimaryWorkspaceState) {
  const normalized = normalizePrimaryWorkspaceState(state);
  const store = await localStore();
  await store.set(localStateSchemaVersionKey, 1);
  await store.set(primaryWorkspaceKey, normalized);
  await store.save();
  return normalized;
}
```

Move `welcomeDocumentSeen`, `workspace`, `recentMarkdownFiles`, `recentMarkdownFolders`, and `fileTreeSortByWorkspace` access to the same local store. Keep their public functions unchanged. Store portable export options in `settings.json` without `pandocPath`; combine the separately loaded local path only at the UI-facing `ExportSettings` boundary. Remove `export.pandocPath` from the Rust `PortableAppSettings` projection so remote/exported settings cannot expose a device path.

- [ ] **Step 4: Run focused and Rust settings tests**

Run: `pnpm --filter @markra/app exec vitest run src/lib/settings/local-state.test.ts src/lib/settings/app-settings.test.ts src/lib/settings/export-settings.test.ts --environment jsdom --globals`

Expected: PASS.

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml app_settings`

Expected: PASS and portable settings serialization contains no `pandocPath`.

- [ ] **Step 5: Commit**

```bash
git add packages/app/src/lib/settings/local-state.ts packages/app/src/lib/settings/local-state.test.ts packages/app/src/lib/settings/app-settings.ts packages/app/src/lib/settings/app-settings.test.ts packages/app/src/lib/settings/export-settings.ts packages/app/src/lib/settings/export-settings.test.ts packages/app/src/runtime/index.ts apps/desktop/src-tauri/src/app_settings.rs
git commit -m "refactor: separate local application state"
```

### Task 2: Resolve One Primary Workspace Per Device

**Files:**
- Create: `packages/app/src/hooks/usePrimaryWorkspace.ts`
- Create: `packages/app/src/hooks/usePrimaryWorkspace.test.tsx`
- Modify: `packages/app/src/lib/tauri/file.ts`
- Modify: `packages/app/src/runtime/index.ts`
- Modify: `apps/desktop/src/runtime/tauri/file/desktop.ts`
- Modify: `apps/desktop/src-tauri/src/markdown_files/open.rs`

**Interfaces:**
- Consumes: `loadPrimaryWorkspaceState`, `savePrimaryWorkspaceState` from Task 1.
- Produces: `PrimaryWorkspaceStatus = "loading" | "needs-onboarding" | "ready" | "deferred" | "recovery" | "error"`.
- Produces: `usePrimaryWorkspace({ trueMobile }): PrimaryWorkspaceController` with `root`, `status`, `chooseDesktopRoot`, `createMobileRoot`, `deferDesktopSetup`, `retry`, and `resetOnboarding`.
- Produces: runtime `files.resolveMarkdownFolder(path): Promise<NativeMarkdownFolder>` which validates, canonicalizes, and lists a chosen or persisted folder without a picker.

- [ ] **Step 1: Write failing hook and native validation tests**

```tsx
it("keeps narrow desktop semantics even when compact layout is active", async () => {
  mockFormFactor("desktop");
  mockViewportWidth(375);
  const { result } = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));

  await waitFor(() => expect(result.current.status).toBe("needs-onboarding"));
  expect(result.current.canChooseDesktopRoot).toBe(true);
  expect(mockResolveManagedRoot).not.toHaveBeenCalled();
});

it("resolves only the managed workspace on true mobile", async () => {
  mockResolveManagedRoot.mockResolvedValue("/app-data/workspace");
  const { result } = renderHook(() => usePrimaryWorkspace({ trueMobile: true }));

  await waitFor(() => expect(result.current.status).toBe("needs-onboarding"));
  await act(() => result.current.createMobileRoot());
  expect(result.current.root).toBe("/app-data/workspace");
  expect(mockOpenMarkdownFolder).not.toHaveBeenCalled();
});

it("reports recovery instead of substituting a recent external folder", async () => {
  seedPrimaryWorkspace("/missing/Notes", true);
  mockResolveMarkdownFolder.mockRejectedValue(new Error("folder-unavailable"));
  const { result } = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
  await waitFor(() => expect(result.current.status).toBe("recovery"));
  expect(result.current.root).toBeNull();
});
```

- [ ] **Step 2: Run tests and verify they fail for the missing controller**

Run: `pnpm --filter @markra/app exec vitest run src/hooks/usePrimaryWorkspace.test.tsx --environment jsdom --globals`

Expected: FAIL because `usePrimaryWorkspace` and `resolveMarkdownFolder` are absent.

- [ ] **Step 3: Implement the controller as an explicit state machine**

```ts
export type PrimaryWorkspaceController = {
  canChooseDesktopRoot: boolean;
  chooseDesktopRoot: () => Promise<string | null>;
  createMobileRoot: () => Promise<string | null>;
  deferDesktopSetup: () => Promise<unknown>;
  error: string | null;
  resetOnboarding: () => Promise<unknown>;
  retry: () => Promise<unknown>;
  root: string | null;
  status: PrimaryWorkspaceStatus;
};

export function usePrimaryWorkspace({ trueMobile }: { trueMobile: boolean }): PrimaryWorkspaceController {
  // Load local state once, resolve only the path appropriate to the runtime,
  // guard every async completion with a generation token, and persist only
  // after native resolution returns a canonical directory.
}
```

`chooseDesktopRoot` calls the existing picker, persists the returned canonical `folder.path`, and marks onboarding complete. `createMobileRoot` calls only `workspace.resolveManagedRoot`. `deferDesktopSetup` persists `{ desktopPath: null, onboardingCompleted: true }`. Persisted desktop paths are reopened through `files.resolveMarkdownFolder`; any failure becomes `recovery`. Keep `useManagedWorkspace` unchanged in this task so the existing App continues to build; Task 4 removes it after App consumes the new controller.

- [ ] **Step 4: Run frontend and Rust folder-resolution tests**

Run: `pnpm --filter @markra/app exec vitest run src/hooks/usePrimaryWorkspace.test.tsx --environment jsdom --globals`

Expected: PASS.

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml markdown_files::open`

Expected: PASS, including missing path, file-instead-of-directory, symlink canonicalization, and unreadable directory cases.

- [ ] **Step 5: Commit**

```bash
git add packages/app/src/hooks/usePrimaryWorkspace.ts packages/app/src/hooks/usePrimaryWorkspace.test.tsx packages/app/src/lib/tauri/file.ts packages/app/src/runtime/index.ts apps/desktop/src/runtime/tauri/file/desktop.ts apps/desktop/src-tauri/src/markdown_files/open.rs
git commit -m "feat: resolve the primary notes workspace"
```

### Task 3: Build Native Desktop and Mobile Welcome Surfaces

**Files:**
- Create: `packages/app/src/components/onboarding/WelcomeScreen.tsx`
- Create: `packages/app/src/components/onboarding/WelcomeScreen.test.tsx`
- Modify: `packages/shared/src/i18n/locales/en.ts`
- Modify: `packages/shared/src/i18n/locales/zh-CN.ts`
- Modify: `packages/app/src/styles.css`

**Interfaces:**
- Consumes: `PrimaryWorkspaceStatus` from Task 2.
- Produces: `WelcomeScreenProps` with form-factor semantics and callbacks only; it does not access stores or runtime APIs.

- [ ] **Step 1: Write semantic rendering tests before markup**

```tsx
it("renders the approved true-mobile action set", () => {
  render(<WelcomeScreen formFactor="mobile" status="needs-onboarding" {...callbacks} />);
  expect(screen.getByText("明窗净几，字字轻语。" )).toBeVisible();
  expect(screen.getByRole("button", { name: "创建并开始" })).toBeVisible();
  expect(screen.queryByText(/第一步|稍后再说|打开外部目录/u)).not.toBeInTheDocument();
});

it("keeps desktop actions at a 375 pixel viewport", () => {
  setViewport(375);
  render(<WelcomeScreen formFactor="desktop" status="needs-onboarding" {...callbacks} />);
  expect(screen.getByRole("button", { name: "选择目录…" })).toBeVisible();
  expect(screen.getByRole("button", { name: "打开单独文件" })).toBeVisible();
  expect(screen.getByRole("button", { name: "打开外部目录" })).toBeVisible();
});
```

- [ ] **Step 2: Run the component test and verify it fails**

Run: `pnpm --filter @markra/app exec vitest run src/components/onboarding/WelcomeScreen.test.tsx --environment jsdom --globals`

Expected: FAIL because the component does not exist.

- [ ] **Step 3: Implement the approved component and token-based layout**

```ts
export type WelcomeScreenProps = {
  formFactor: "desktop" | "mobile";
  onChooseDesktopRoot: () => Promise<unknown>;
  onCreateMobileRoot: () => Promise<unknown>;
  onDeferDesktopSetup: () => Promise<unknown>;
  onOpenExternalFile: () => Promise<unknown>;
  onOpenExternalFolder: () => Promise<unknown>;
  onRetry: () => Promise<unknown>;
  status: PrimaryWorkspaceStatus;
};
```

Desktop markup uses a full-height two-region layout below the native title bar: a restrained identity rail and a focused directory-selection surface. Mobile markup uses `env(safe-area-inset-*)`, one content column, and a bottom primary button. Reuse `--background-primary`, `--surface-primary`, `--text-primary`, `--text-secondary`, `--border-subtle`, the existing radius scale, system fonts, `Button`, and lucide icons. Do not introduce gradients, card grids, fake browser/device chrome, ornamental illustrations, or a second token system.

- [ ] **Step 4: Run component and i18n tests**

Run: `pnpm --filter @markra/app exec vitest run src/components/onboarding/WelcomeScreen.test.tsx --environment jsdom --globals`

Expected: PASS for desktop, narrow desktop, true mobile, recovery, and callback behavior.

Run: `pnpm --filter @markra/shared test`

Expected: PASS with complete English and Simplified Chinese key coverage.

- [ ] **Step 5: Commit**

```bash
git add packages/app/src/components/onboarding/WelcomeScreen.tsx packages/app/src/components/onboarding/WelcomeScreen.test.tsx packages/shared/src/i18n/locales/en.ts packages/shared/src/i18n/locales/zh-CN.ts packages/app/src/styles.css
git commit -m "feat: add native notes onboarding"
```

### Task 4: Install Primary-Window and External-Window Responsibilities

**Files:**
- Create: `packages/app/src/components/settings/NotesWorkspaceSettings.tsx`
- Create: `packages/app/src/components/settings/NotesWorkspaceSettings.test.tsx`
- Modify: `packages/app/src/App.tsx`
- Modify: `packages/app/src/App.test.tsx`
- Delete: `packages/app/src/hooks/useManagedWorkspace.ts`
- Delete: `packages/app/src/hooks/useManagedWorkspace.test.tsx`
- Modify: `packages/app/src/hooks/useSettingsWindowState.ts`
- Modify: `packages/app/src/hooks/useSettingsWindowState.test.ts`
- Modify: `packages/app/src/components/SettingsWindow.tsx`
- Modify: `packages/app/src/components/SettingsWindow.test.tsx`
- Modify: `packages/app/src/components/settings/translate.ts`
- Modify: `packages/shared/src/i18n/locales/en.ts`
- Modify: `packages/shared/src/i18n/locales/zh-CN.ts`

**Interfaces:**
- Consumes: `usePrimaryWorkspace` and `WelcomeScreen` from Tasks 2 and 3.
- Produces: settings category id `notesWorkspace` and `NotesWorkspaceSettingsProps` with `root`, `status`, `onChoose`, `onResetOnboarding`.
- Guarantees: the main window opens only the primary root; external file/folder actions invoke `openMarkdownFileInNewWindow` or `openMarkdownFolderInNewWindow`.

- [ ] **Step 1: Write failing application responsibility tests**

```tsx
it("opens a persisted primary workspace automatically in the main window", async () => {
  seedPrimaryWorkspace("/Notes", true);
  render(<App />);
  await waitFor(() => expect(mockOpenFolderPath).toHaveBeenCalledWith(
    "/Notes",
    expect.any(String),
    expect.any(Boolean),
    expect.any(Boolean),
    expect.any(Object)
  ));
});

it("opens an external folder in another window without replacing the primary root", async () => {
  seedPrimaryWorkspace("/Notes", true);
  mockedOpenMarkdownFolder.mockResolvedValue({ path: "/External", name: "External", files: [] });
  render(<App />);
  await user.click(await screen.findByRole("button", { name: "打开外部目录" }));
  expect(mockOpenMarkdownFolderInNewWindow).toHaveBeenCalledWith("/External");
  expect(readPrimaryWorkspace()).toEqual(expect.objectContaining({ desktopPath: "/Notes" }));
});

it("shows recovery instead of restoring an external recent folder", async () => {
  seedUnavailablePrimaryWorkspace("/Missing");
  seedRecentFolder("/External");
  render(<App />);
  expect(await screen.findByText(/找不到主笔记目录/u)).toBeVisible();
  expect(mockOpenFolderPath).not.toHaveBeenCalledWith("/External", expect.anything());
});
```

- [ ] **Step 2: Run focused App and Settings tests to verify red state**

Run: `pnpm --filter @markra/app exec vitest run src/App.test.tsx src/components/settings/NotesWorkspaceSettings.test.tsx src/components/SettingsWindow.test.tsx --environment jsdom --globals`

Expected: FAIL because primary-workspace integration and the settings category are absent.

- [ ] **Step 3: Integrate without adding more business logic to `App.tsx`**

```tsx
const primaryWorkspace = usePrimaryWorkspace({ trueMobile: compactMode.trueMobile });
const onboardingVisible = ["loading", "needs-onboarding", "recovery", "error"]
  .includes(primaryWorkspace.status);

if (onboardingVisible) {
  return (
    <WelcomeScreen
      formFactor={compactMode.trueMobile ? "mobile" : "desktop"}
      status={primaryWorkspace.status}
      onChooseDesktopRoot={primaryWorkspace.chooseDesktopRoot}
      onCreateMobileRoot={primaryWorkspace.createMobileRoot}
      onDeferDesktopSetup={primaryWorkspace.deferDesktopSetup}
      onOpenExternalFile={openExternalFileInNewWindow}
      onOpenExternalFolder={openExternalFolderInNewWindow}
      onRetry={primaryWorkspace.retry}
    />
  );
}
```

Create small callbacks that open pickers and immediately hand selected paths to the existing new-window runtime methods. Remove the main-window “open folder” path that replaces `fileTree.sourcePath`; menu and titlebar external-folder actions use the same callback. A `deferred` desktop state proceeds to the blank editor with no primary file tree so Settings and standalone-file editing remain reachable. The Notes category calls `chooseDesktopRoot`, explains that switching moves no files, and never exposes an editable path text box. Delete `useManagedWorkspace` only after every App call site uses `usePrimaryWorkspace`. Keep React hook ordering unconditional: compute the onboarding surface after all hooks are installed and choose it at the final render boundary rather than returning before later hooks execute.

- [ ] **Step 4: Run application responsibility tests**

Run: `pnpm --filter @markra/app exec vitest run src/App.test.tsx src/components/settings/NotesWorkspaceSettings.test.tsx src/components/SettingsWindow.test.tsx src/hooks/useSettingsWindowState.test.ts --environment jsdom --globals`

Expected: PASS, including primary restore, defer, recovery, root switch, operating-system open requests, external window isolation, and true-mobile storage display.

- [ ] **Step 5: Commit**

```bash
git add -A packages/app/src/components/settings/NotesWorkspaceSettings.tsx packages/app/src/components/settings/NotesWorkspaceSettings.test.tsx packages/app/src/App.tsx packages/app/src/App.test.tsx packages/app/src/hooks/useManagedWorkspace.ts packages/app/src/hooks/useManagedWorkspace.test.tsx packages/app/src/hooks/useSettingsWindowState.ts packages/app/src/hooks/useSettingsWindowState.test.ts packages/app/src/components/SettingsWindow.tsx packages/app/src/components/SettingsWindow.test.tsx packages/app/src/components/settings/translate.ts packages/shared/src/i18n/locales/en.ts packages/shared/src/i18n/locales/zh-CN.ts
git commit -m "feat: isolate primary and external workspaces"
```

### Task 5: Apply Primary-Root Asset Semantics

**Files:**
- Modify: `packages/app/src/lib/editor-assets.ts`
- Modify: `packages/app/src/lib/editor-assets.test.ts`
- Modify: `packages/app/src/App.tsx`
- Modify: `packages/app/src/App.test.tsx`
- Modify: `apps/desktop/src-tauri/src/markdown_files/image.rs`
- Modify: `apps/desktop/src-tauri/src/markdown_files/attachment.rs`

**Interfaces:**
- Consumes: canonical `primaryWorkspace.root` from Task 2.
- Produces: `resolveEditorAssetContext({ documentPath, primaryWorkspaceRoot }): EditorAssetContext` and `resolveEditorAssetAction({ mode, origin }): EditorAssetAction`.
- Guarantees: primary documents always target root-level lowercase `assets/`, independent of sync enabled state.

- [ ] **Step 1: Write failing primary/external asset tests**

```ts
it("uses root assets for a nested primary note even when sync is disabled", () => {
  expect(resolveEditorAssetContext({
    documentPath: "/Notes/journal/2026/day.md",
    primaryWorkspaceRoot: "/Notes"
  })).toEqual({ mode: "primary-workspace", primaryRootPath: "/Notes" });
});

it("keeps external resources as references except clipboard bytes", () => {
  const context = resolveEditorAssetContext({
    documentPath: "/External/post.md",
    primaryWorkspaceRoot: "/Notes"
  });
  expect(context).toEqual({ mode: "standalone" });
  expect(resolveEditorAssetAction({ mode: context.mode, origin: "import" })).toBe("reference");
  expect(resolveEditorAssetAction({ mode: context.mode, origin: "clipboard" })).toBe("copy-document");
});
```

- [ ] **Step 2: Run focused tests and verify the old project-root input fails**

Run: `pnpm --filter @markra/app exec vitest run src/lib/editor-assets.test.ts src/App.test.tsx --environment jsdom --globals`

Expected: FAIL because the resolver still depends on managed/project configuration state.

- [ ] **Step 3: Implement one containment decision**

```ts
export function resolveEditorAssetContext({
  documentPath,
  primaryWorkspaceRoot
}: {
  documentPath: string | null;
  primaryWorkspaceRoot: string | null;
}): EditorAssetContext {
  if (!documentPath) return { mode: "standalone" };
  if (primaryWorkspaceRoot && pathIsWithinRoot(primaryWorkspaceRoot, documentPath)) {
    return { mode: "primary-workspace", primaryRootPath: primaryWorkspaceRoot };
  }
  return { mode: "standalone" };
}
```

Rename the prior `managed-workspace` and `sync-project` modes to one `primary-workspace` mode. Its resource action is `copy-workspace` for clipboard, drop, import, and remote origins. Standalone mode keeps `copy-document` only for clipboard and returns `reference` for drop, import, and remote origins. Use the existing path helpers for platform separators and containment; do not duplicate string-prefix security checks. Existing local image files in external documents remain references and are not copied. Clipboard bytes for a saved external document continue using the writer's document-relative sibling `assets/`; unsaved external documents fail with the existing save-first error. Rust writers must reject path escape and always normalize the destination folder name to `assets`.

- [ ] **Step 4: Run asset and application tests**

Run: `pnpm --filter @markra/app exec vitest run src/lib/editor-assets.test.ts src/App.test.tsx --environment jsdom --globals`

Expected: PASS.

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml markdown_files::image`

Expected: PASS for primary nested paths, already-in-assets resources, and path escapes.

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml markdown_files::attachment`

Expected: PASS for external sibling assets, unsaved documents, and path escapes.

- [ ] **Step 5: Run the subsystem gate and commit**

Run: `pnpm typecheck:test`

Expected: PASS.

Run: `pnpm test`

Expected: PASS.

```bash
git add packages/app/src/lib/editor-assets.ts packages/app/src/lib/editor-assets.test.ts packages/app/src/App.tsx packages/app/src/App.test.tsx apps/desktop/src-tauri/src/markdown_files/image.rs apps/desktop/src-tauri/src/markdown_files/attachment.rs
git commit -m "feat: store primary note assets at workspace root"
```
