# QingYu Compact Runtime Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Compact sync terminate in an accurate state when no workspace root exists, and isolate covered editor content from focus and assistive technology without remounting it.

**Architecture:** Keep the existing `CompactSyncSettingsController` and navigation stack. Derive the no-workspace presentation directly from `available === true && candidateRoot === null`, and keep the editor DOM mounted while applying `inert` plus `aria-hidden` whenever a full-screen Compact page covers it.

**Tech Stack:** React 19, TypeScript 6, Vitest, Testing Library, Tauri v2, shared typed i18n.

## Global Constraints

- Preserve the fixed true-mobile managed workspace and its blocking startup gate.
- Preserve WebDAV/S3 engines, conflict behavior, autosave, sync triggers, and desktop wide-mode behavior.
- Do not create a managed root for desktop Compact simulation.
- Do not add a folder picker, workspace switcher, reset, clear-data, or cloud-delete action.
- Keep the editor mounted across Compact full-screen navigation.
- Do not use the TypeScript `void` keyword or operator.
- Use `pnpm`; do not add dependencies or another lockfile.

---

### Task 1: Render a Finite No-Workspace Sync State

**Files:**
- Modify: `packages/app/src/components/compact/CompactSyncStatusScreen.test.tsx`
- Modify: `packages/app/src/components/compact/CompactSyncStatusScreen.tsx`
- Modify: `packages/shared/src/i18n/locales/types.ts`
- Modify: `packages/shared/src/i18n/locales/en.ts`
- Modify: `packages/shared/src/i18n/locales/zh-CN.ts`

**Interfaces:**
- Consumes: `CompactSyncSettingsController.available`, `.candidateRoot`, and `.loadResult`.
- Produces: typed keys `compact.sync.noWorkspaceTitle` and `compact.sync.noWorkspaceDescription`.
- Leaves the controller hook and sync-session APIs unchanged.

- [ ] **Step 1: Write failing component tests for the no-root and real-loading states**

Add tests with the existing `controller()` helper:

```tsx
it("shows a finite no-workspace state when sync is available without a candidate root", () => {
  const setup = controller(null, {
    available: true,
    candidateRoot: null,
    projectRoot: null,
    requestedRoot: null
  });

  render(<CompactSyncStatusScreen controller={setup} language="en" navigation={navigation()} />);

  expect(screen.getByRole("heading", { name: "No workspace available" })).toBeInTheDocument();
  expect(screen.getByText("Open a workspace folder before configuring sync.")).toBeInTheDocument();
  expect(screen.queryByText("Loading this folder's sync configuration...")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Configure Sync" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Sync Now" })).not.toBeInTheDocument();
  expect(setup.begin).not.toHaveBeenCalled();
});

it("keeps loading when a real candidate root is waiting for its load result", () => {
  render(<CompactSyncStatusScreen controller={controller(null)} language="en" navigation={navigation()} />);

  expect(screen.getByRole("status")).toHaveTextContent("Loading this folder's sync configuration...");
  expect(screen.queryByRole("heading", { name: "No workspace available" })).not.toBeInTheDocument();
});
```

- [ ] **Step 2: Run the focused test and verify RED**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/components/compact/CompactSyncStatusScreen.test.tsx
```

Expected: the no-workspace test fails because the current screen renders the loading copy.

- [ ] **Step 3: Add typed English and Simplified Chinese copy**

Add both keys to `I18nKey`, `compactEnMessages`, and the Simplified Chinese locale:

```ts
"compact.sync.noWorkspaceTitle": "No workspace available",
"compact.sync.noWorkspaceDescription": "Open a workspace folder before configuring sync."
```

```ts
"compact.sync.noWorkspaceTitle": "没有可用的工作区",
"compact.sync.noWorkspaceDescription": "请先打开工作区文件夹，再配置同步。"
```

Other locales continue inheriting the typed Compact English fallback.

- [ ] **Step 4: Implement the no-workspace branch before the loading branch**

In `CompactSyncStatusScreen`, keep `!available` first, then add:

```tsx
} else if (!controller.candidateRoot) {
  content = (
    <div className="grid min-w-0 gap-3 text-center">
      <Cloud aria-hidden="true" className="mx-auto text-(--text-secondary)" size={32} />
      <h2 className="m-0 text-lg font-semibold">
        {t(language, "compact.sync.noWorkspaceTitle")}
      </h2>
      <p className="m-0 break-words text-sm text-(--text-secondary)">
        {t(language, "compact.sync.noWorkspaceDescription")}
      </p>
    </div>
  );
} else if (!loadResult) {
```

Do not expose a configure or picker action in this branch.

- [ ] **Step 5: Run focused App and shared i18n tests and verify GREEN**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/components/compact/CompactSyncStatusScreen.test.tsx src/components/compact/CompactAppShell.test.tsx
pnpm --filter @markra/shared test -- src/i18n/index.test.ts
```

Expected: all selected suites pass; shared locale typing remains complete.

- [ ] **Step 6: Commit Task 1**

```bash
git add packages/app/src/components/compact/CompactSyncStatusScreen.tsx \
  packages/app/src/components/compact/CompactSyncStatusScreen.test.tsx \
  packages/shared/src/i18n/locales/types.ts \
  packages/shared/src/i18n/locales/en.ts \
  packages/shared/src/i18n/locales/zh-CN.ts
git commit -m "fix(mobile): handle Compact sync without workspace"
```

---

### Task 2: Isolate the Covered Editor Layer

**Files:**
- Modify: `packages/app/src/components/compact/CompactAppShell.test.tsx`
- Modify: `packages/app/src/components/compact/CompactAppShell.tsx`

**Interfaces:**
- Consumes: `navigation.page.kind` from `useCompactNavigation`.
- Produces: `inert` and `aria-hidden="true"` only on `[data-compact-editor-layer]` while `navigation.page.kind !== "editor"`.
- Keeps `controller.editor.host` mounted and does not alter the overlay DOM.

- [ ] **Step 1: Replace the layering regression with explicit accessibility isolation coverage**

Use a custom page renderer that can open Files and return:

```tsx
it("keeps the covered editor mounted but inert and hidden from accessibility", async () => {
  function renderPage(page: CompactPage, navigation: CompactNavigation) {
    if (page.kind === "editor") {
      return <button onClick={() => navigation.push({ kind: "files" })}>Open files</button>;
    }

    return (
      <section aria-label={`${page.kind} page`}>
        <button onClick={() => navigation.pop()}>Back to editor</button>
      </section>
    );
  }

  const { container } = render(
    <CompactAppShell controller={controllerWithEditorHost()} renderPage={renderPage} />
  );
  fireEvent.click(screen.getByRole("button", { name: "Open files" }));

  const editorLayer = container.querySelector("[data-compact-editor-layer]");
  expect(editorLayer).toHaveAttribute("inert");
  expect(editorLayer).toHaveAttribute("aria-hidden", "true");
  expect(editorLayer?.querySelector('[aria-label="Compact editor host"]')).toBeInTheDocument();
  expect(screen.queryByLabelText("Compact editor host")).not.toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Back to editor" }));
  await waitFor(() => expect(screen.getByLabelText("Compact editor host")).toBeInTheDocument());
  expect(editorLayer).not.toHaveAttribute("inert");
  expect(editorLayer).not.toHaveAttribute("aria-hidden");
});
```

Retain the existing assertions that the overlay is `absolute inset-0`, `z-10`, and labelled with `data-compact-page="files"`.

- [ ] **Step 2: Run the shell test and verify RED**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/components/compact/CompactAppShell.test.tsx
```

Expected: the covered editor layer lacks `inert` and `aria-hidden`, and its host remains queryable through the accessibility tree.

- [ ] **Step 3: Apply conditional isolation attributes without remounting**

In `CompactAppShell`, derive:

```ts
const editorCovered = navigation.page.kind !== "editor";
```

Then update the existing editor layer:

```tsx
<div
  aria-hidden={editorCovered ? true : undefined}
  className="relative z-0 h-full min-h-0 overflow-hidden"
  data-compact-editor-layer
  inert={editorCovered ? true : undefined}
>
```

Do not conditionally remove `controller.editor.host`.

- [ ] **Step 4: Run Compact shell and screen tests and verify GREEN**

Run:

```bash
pnpm --filter @markra/app exec vitest run \
  src/components/compact/CompactAppShell.test.tsx \
  src/components/compact/CompactEditorScreen.test.tsx \
  src/components/compact/CompactFileBrowserScreen.test.tsx \
  src/components/compact/CompactSettingsHome.test.tsx \
  src/components/compact/CompactSyncStatusScreen.test.tsx
```

Expected: all selected tests pass, the editor host remains mounted, and only the visible page is accessible.

- [ ] **Step 5: Commit Task 2**

```bash
git add packages/app/src/components/compact/CompactAppShell.tsx \
  packages/app/src/components/compact/CompactAppShell.test.tsx
git commit -m "fix(mobile): isolate Compact overlay pages"
```

---

### Task 3: Full Verification and Native Runtime Re-Acceptance

**Files:**
- No source changes expected.
- Generated QA bundle: `apps/desktop/src-tauri/target/debug/bundle/macos/QingYu Compact QA.app` (ignored build output).
- Runtime screenshots: `.superpowers/sdd/artifacts/` (ignored evidence output).

**Interfaces:**
- Consumes: Tasks 1 and 2 commits.
- Produces: fresh automated and visible runtime evidence; no committed generated files.

- [ ] **Step 1: Run complete automated verification**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
pnpm test
pnpm typecheck:test
pnpm build
git diff --check
git status --short
```

Expected: Rust non-live tests, every workspace test, typecheck, and both production builds pass; the worktree is clean.

- [ ] **Step 2: Rebuild the isolated current-HEAD native QA bundle**

Close only the running `QingYu Compact QA` process, leaving the user's primary QingYu process untouched, then run:

```bash
pnpm tauri build --debug --bundles app --no-sign \
  --config '{"productName":"QingYu Compact QA","identifier":"dev.markra.app.compactqa"}'
open -n 'apps/desktop/src-tauri/target/debug/bundle/macos/QingYu Compact QA.app'
```

Expected: one unsigned local QA `.app` is built from the current commit and launches with bundle identifier `dev.markra.app.compactqa`.

- [ ] **Step 3: Repeat native Compact interaction checks**

Using Computer Use, shrink the QA Tauri window below 720 CSS pixels and verify:

1. editor header and touch formatting toolbar appear;
2. Files and Settings remain visually full-screen;
3. Settings → Sync with no desktop workspace root shows `没有可用的工作区`, not a spinner;
4. the sync page exposes Back only;
5. while Files or Settings is visible, the accessibility tree does not expose the Markdown editor or formatting toolbar;
6. returning to Editor restores both controls without losing document state.

- [ ] **Step 4: Verify final repository and process state**

```bash
git status --short
ps ax -o pid=,etime=,command= | rg 'QingYu Compact QA.app/Contents/MacOS/markra' | rg -v 'rg '
```

Expected: source worktree clean; only the isolated QA app is left open for user inspection; no source or configuration file was changed by runtime acceptance.
