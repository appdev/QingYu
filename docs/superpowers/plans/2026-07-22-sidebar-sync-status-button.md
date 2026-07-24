# Sidebar Sync Status Button Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a QingYu-styled cloud-sync status button beside Settings that immediately starts the existing application-level manual sync for the primary notebook.

**Architecture:** A focused `SidebarSyncButton` owns only status presentation and accessibility. `MarkdownFileTreeDrawer` owns placement in its expanded and collapsed footers, while `App.tsx` derives state from `useAppSyncCoordinator` and passes the existing `run("manual")` callback only for the desktop primary-notebook window.

**Tech Stack:** React 19, TypeScript, Tailwind CSS 4, Lucide React, Vitest, Testing Library, Tauri v2.

## Global Constraints

- Render only for the desktop primary-notebook window; never render in external standalone-file windows or true-mobile layouts.
- Reuse `useAppSyncCoordinator` and `appSync.run("manual")`; do not introduce a second sync implementation, local request state, or new sync configuration.
- Match the existing Settings launcher: 28 by 28 pixel ghost icon button, Lucide icons at 15 pixels, six-pixel/medium rounding, current hover/focus opacity, existing QingYu design tokens, and no new global CSS.
- Render beside Settings in both expanded and collapsed file-tree layouts; keep the update action right-aligned.
- Loading disables repeat clicks; unavailable, failed, and succeeded states remain clickable so the existing coordinator can guide, retry, or run again.
- Use existing translations for “Sync now”, “Syncing…”, “Succeeded”, “Failed”, and disabled readiness; add no new copy unless a test proves it is necessary.
- Keep external-file behavior, notebook switching, sync configuration, and the sync engine unchanged.
- Do not use the TypeScript `void` keyword or operator.

---

### Task 1: Reusable Sidebar Sync Status Control

**Files:**
- Create: `packages/app/src/components/SidebarSyncButton.tsx`
- Create: `packages/app/src/components/SidebarSyncButton.test.tsx`
- Create: `packages/app/src/components/SidebarSyncButton.preview.tsx`

**Interfaces:**
- Consumes: `IconButton` from `@markra/ui`, `t` and `AppLanguage` from `@markra/shared`, and Lucide `Cloud`, `CloudOff`, `LoaderCircle`, `Check`, and `X`.
- Produces: `export type SidebarSyncButtonState = "idle" | "unavailable" | "running" | "failed" | "succeeded"` and `export function SidebarSyncButton(props: { className?: string; disabled?: boolean; language?: AppLanguage; muted?: boolean; onSync: () => unknown | Promise<unknown>; state: SidebarSyncButtonState })`.

- [ ] **Step 1: Write the failing component tests**

Create tests that render the control in every semantic state and assert these exact behaviors:

```tsx
expect(screen.getByRole("button", { name: "Sync now" })).toHaveAttribute("data-sync-state", "idle");
expect(container.querySelector(".lucide-cloud")).toBeInTheDocument();

expect(screen.getByRole("button", { name: "Sync now · Disabled" }))
  .toHaveAttribute("data-sync-state", "unavailable");
expect(container.querySelector(".lucide-cloud-off")).toBeInTheDocument();

expect(screen.getByRole("button", { name: "Syncing..." })).toBeDisabled();
expect(screen.getByRole("button", { name: "Syncing..." })).toHaveAttribute("aria-busy", "true");
expect(container.querySelector(".lucide-loader-circle")).toHaveClass("animate-spin");

expect(screen.getByRole("button", { name: "Sync now · Failed" }))
  .toHaveAttribute("data-sync-state", "failed");
expect(screen.getByRole("button", { name: "Sync now · Succeeded" }))
  .toHaveAttribute("data-sync-state", "succeeded");
```

Also click idle, unavailable, failed, and succeeded controls and assert `onSync` is called once; click running and explicitly disabled controls and assert it is not called.

- [ ] **Step 2: Run the new test to prove RED**

Run: `pnpm --filter @markra/app exec vitest run src/components/SidebarSyncButton.test.tsx`

Expected: FAIL because `./SidebarSyncButton` does not exist.

- [ ] **Step 3: Implement the minimal visual state component**

Implement the exported types above. Build the label from existing translation keys:

```tsx
const actionLabel = t(language, "settings.sync.run");
const stateLabel = state === "running"
  ? t(language, "settings.sync.running")
  : state === "unavailable"
    ? `${actionLabel} · ${t(language, "settings.sync.readiness.disabled")}`
    : state === "failed"
      ? `${actionLabel} · ${t(language, "settings.sync.status.failed")}`
      : state === "succeeded"
        ? `${actionLabel} · ${t(language, "settings.sync.status.succeeded")}`
        : actionLabel;
const interactionDisabled = disabled || state === "running";
```

Default `muted` to `false`. Use `Cloud` for idle/failed/succeeded, `CloudOff` for unavailable, and `LoaderCircle` for running. Overlay a small `X` using `text-(--danger)` for failure and a small `Check` using `text-(--accent)` for success. The `IconButton` must include exactly one base opacity selected by `muted`:

```tsx
className={`relative rounded-md ${muted ? "opacity-40" : "opacity-70"} hover:opacity-100 focus-visible:opacity-100 active:translate-y-px motion-reduce:transform-none ${className ?? ""}`}
data-sync-state={state}
disabled={interactionDisabled}
aria-busy={state === "running" ? true : undefined}
label={stateLabel}
tooltip={stateLabel}
onClick={onSync}
```

- [ ] **Step 4: Add the Hallmark state preview**

Create a preview component that shows eight labeled rows: default, forced hover, forced focus, forced active, disabled, loading, error, and success. Use the real `SidebarSyncButton`; only forced preview classes may simulate hover/focus/active. Do not create new palette tokens.

- [ ] **Step 5: Run the component test to prove GREEN**

Run: `pnpm --filter @markra/app exec vitest run src/components/SidebarSyncButton.test.tsx`

Expected: PASS with all state, accessibility, and click assertions green.

- [ ] **Step 6: Commit Task 1**

```bash
git add packages/app/src/components/SidebarSyncButton.tsx packages/app/src/components/SidebarSyncButton.test.tsx packages/app/src/components/SidebarSyncButton.preview.tsx
git commit -m "feat: add sidebar sync status control"
```

---

### Task 2: Place Sync Beside Settings in Both Drawer Layouts

**Files:**
- Modify: `packages/app/src/components/MarkdownFileTreeDrawer.tsx`
- Modify: `packages/app/src/components/MarkdownFileTreeDrawer.test.tsx`

**Interfaces:**
- Consumes: `SidebarSyncButton` and `SidebarSyncButtonState` from Task 1.
- Produces: optional drawer props `onSyncNow?: () => unknown | Promise<unknown>` and `syncState?: SidebarSyncButtonState`; omitting `onSyncNow` omits the control.

- [ ] **Step 1: Write failing drawer integration tests**

Change the collapsed-footer test to render with `onSyncNow` and `syncState="succeeded"`, then assert both controls share the same fixed group and the sync button calls the callback:

```tsx
const footerActions = container.querySelector(".markdown-file-tree-collapsed-actions");
expect(footerActions).toHaveClass("fixed", "bottom-3", "left-3", "flex", "gap-1");
expect(footerActions).toContainElement(screen.getByRole("button", { name: "Settings" }));
expect(footerActions).toContainElement(screen.getByRole("button", { name: "Sync now · Succeeded" }));
fireEvent.click(screen.getByRole("button", { name: "Sync now · Succeeded" }));
expect(onSyncNow).toHaveBeenCalledTimes(1);
```

Change the expanded Windows-footer test to pass `onSyncNow`, assert the Settings and Sync controls share `.markdown-file-tree-primary-actions`, and keep the update control outside that group. Add a separate render without `onSyncNow` and assert `queryByRole("button", { name: "Sync now" })` is absent.

- [ ] **Step 2: Run the drawer test to prove RED**

Run: `pnpm --filter @markra/app exec vitest run src/components/MarkdownFileTreeDrawer.test.tsx`

Expected: FAIL because the new props and footer groups do not exist.

- [ ] **Step 3: Implement drawer props and layouts**

Add the optional props, default `syncState` to `"idle"`, and import Task 1. In collapsed mode, replace the single fixed Settings button with:

```tsx
<div className="markdown-file-tree-collapsed-actions fixed bottom-3 left-3 z-30 flex items-center gap-1">
  <IconButton className="opacity-40 hover:opacity-100 focus-visible:opacity-100" ... />
  {onSyncNow ? (
    <SidebarSyncButton language={language} muted onSync={onSyncNow} state={syncState} />
  ) : null}
</div>
```

In the expanded footer, wrap Settings and the conditional sync control in:

```tsx
<div className="markdown-file-tree-primary-actions flex items-center gap-1">
  {/* existing Settings control */}
  {onSyncNow ? (
    <SidebarSyncButton language={language} onSync={onSyncNow} state={syncState} />
  ) : null}
</div>
```

Do not move the update action into that group.

- [ ] **Step 4: Run the drawer test to prove GREEN**

Run: `pnpm --filter @markra/app exec vitest run src/components/MarkdownFileTreeDrawer.test.tsx`

Expected: PASS, including the existing drawer behavior suite.

- [ ] **Step 5: Commit Task 2**

```bash
git add packages/app/src/components/MarkdownFileTreeDrawer.tsx packages/app/src/components/MarkdownFileTreeDrawer.test.tsx
git commit -m "feat: place sync control beside settings"
```

---

### Task 3: Connect the Control to the Existing Manual Sync Coordinator

**Files:**
- Modify: `packages/app/src/App.tsx`
- Modify: `packages/app/src/App.test.tsx`

**Interfaces:**
- Consumes: `SidebarSyncButtonState` from Task 1, `appSync.running`, scoped `appSync.status`, `syncConfig.appliedDocument`, `primaryIntegrationRoot`, `primaryWindowOwner`, `compactMode.trueMobile`, `appFeatures.projectSync`, and `runApplicationSyncNow`.
- Produces: the drawer receives `syncState` and `onSyncNow` only when `primaryWindowOwner && !compactMode.trueMobile && appFeatures.projectSync`.

- [ ] **Step 1: Write the failing primary-window behavior test**

Extend the existing ready-sync primary-root setup, wait for the automatic `app-launch` call, clear the mock, click the sidebar button, and assert one manual request:

```tsx
const syncButton = await screen.findByRole("button", { name: "Sync now · Succeeded" });
mockedSyncApplication.mockClear();
fireEvent.click(syncButton);
await waitFor(() => expect(mockedSyncApplication).toHaveBeenCalledTimes(1));
expect(mockedSyncApplication).toHaveBeenCalledWith({
  notebookName: mockFolderPath.split("/").at(-1) ?? "",
  notesRoot: mockFolderPath,
  revision: "rev-app-ready",
  trigger: "manual"
});
```

Add an external-window test by setting `window.history` to `/?path=<encoded path>`, rendering the standalone note, and asserting every button whose accessible name starts with `Sync now` is absent.

- [ ] **Step 2: Run focused App tests to prove RED**

Run: `pnpm --filter @markra/app exec vitest run src/App.test.tsx -t "sidebar sync|external standalone"`

Expected: FAIL because `App.tsx` does not pass the sync props.

- [ ] **Step 3: Derive the presentation state without duplicating sync behavior**

Import `SidebarSyncButtonState` and derive one value after `runApplicationSyncNow`:

```tsx
const sidebarSyncState: SidebarSyncButtonState = appSync.running
  ? "running"
  : appSync.status?.completionState === "failed"
    ? "failed"
    : appSync.status?.completionState === "succeeded"
      ? "succeeded"
      : !primaryIntegrationRoot || syncConfig.appliedDocument?.readiness !== "ready"
        ? "unavailable"
        : "idle";
const sidebarSyncAvailable = primaryWindowOwner &&
  !compactMode.trueMobile &&
  appFeatures.projectSync;
```

Pass these exact drawer props:

```tsx
onSyncNow: sidebarSyncAvailable ? runApplicationSyncNow : undefined,
syncState: sidebarSyncState,
```

- [ ] **Step 4: Run the App tests to prove GREEN**

Run: `pnpm --filter @markra/app exec vitest run src/App.test.tsx -t "sidebar sync|external standalone"`

Expected: PASS, proving one immediate `manual` request and external-window omission.

- [ ] **Step 5: Run all three focused files together**

Run: `pnpm --filter @markra/app exec vitest run src/components/SidebarSyncButton.test.tsx src/components/MarkdownFileTreeDrawer.test.tsx src/App.test.tsx`

Expected: PASS.

- [ ] **Step 6: Commit Task 3**

```bash
git add packages/app/src/App.tsx packages/app/src/App.test.tsx
git commit -m "feat: trigger manual sync from sidebar"
```

---

### Task 4: Review and Full Verification

**Files:**
- Inspect: all files changed by Tasks 1–3
- Modify only if review or verification finds a concrete defect.

**Interfaces:**
- Consumes: the completed sidebar control, drawer placement, and App coordinator wiring.
- Produces: reviewed, tested, buildable desktop behavior with runtime visual evidence.

- [ ] **Step 1: Run formatting and diff hygiene checks**

Run:

```bash
git diff --check
pnpm typecheck:test
```

Expected: both commands exit 0.

- [ ] **Step 2: Run repository frontend verification**

Run:

```bash
pnpm test
pnpm build
```

Expected: all tests and workspace builds pass.

- [ ] **Step 3: Run the Rust regression suite**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml`

Expected: all non-ignored tests pass under default parallel execution.

- [ ] **Step 4: Build and launch the real desktop app**

Run: `pnpm tauri build --debug --no-sign`, then launch the generated debug application or run `pnpm tauri dev` in an isolated local application-data state.

Expected: Settings and the cloud-sync icon are adjacent in expanded and collapsed file-tree states; tooltip and focus behavior match; clicking the cloud control immediately enters running feedback and completes or shows the existing localized configuration/error guidance.

- [ ] **Step 5: Perform Hallmark handoff review**

Apply the Hallmark slop test and contract checklist. Confirm no new palette, oversized surface, redundant copy, decorative container, or second interaction pattern was introduced. Confirm default, hover, focus, active, disabled, loading, error, and success preview states exist.

- [ ] **Step 6: Review the branch and commit any verified corrections**

If review finds a concrete issue, add only the covering changes and tests, rerun the affected commands, and commit with a focused `fix:` message. If no issue is found, leave the verified commits unchanged.
