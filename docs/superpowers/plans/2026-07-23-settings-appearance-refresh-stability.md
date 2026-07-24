# Settings Appearance Refresh Stability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Keep the independent Settings window mounted while the Appearance category refreshes the theme catalog, eliminating the blank-window refresh loop.

**Architecture:** Keep `useAppTheme.ready` as the live theme signal, but convert the Settings shell's startup gate into a one-way latch. Prove the behavior through the real Settings route with a deliberately pending second catalog request, then make the smallest component change needed to keep the shell visible after first readiness.

**Tech Stack:** React 19, TypeScript, Vitest, React Testing Library, pnpm workspace

## Global Constraints

- Preserve the automatic theme catalog refresh when Appearance mounts.
- Preserve the initial language-and-theme readiness gate before the Settings window first renders.
- Do not change theme selection, activation, persistence, native window lifecycle, localization, or styling.
- Do not modify files outside `packages/app/src/components/SettingsWindow.tsx` and `packages/app/src/App.test.tsx` unless verification exposes a directly related requirement.
- Do not use the TypeScript `void` keyword or operator.

---

## File Structure

- `packages/app/src/App.test.tsx`: owns the independent Settings route regression test and controls the pending theme catalog refresh.
- `packages/app/src/components/SettingsWindow.tsx`: owns the Settings-only one-way startup readiness latch.

### Task 1: Keep Settings mounted during an Appearance refresh

**Files:**
- Modify: `packages/app/src/App.test.tsx:5407`
- Modify: `packages/app/src/components/SettingsWindow.tsx:1`
- Modify: `packages/app/src/components/SettingsWindow.tsx:87`

**Interfaces:**
- Consumes: `getAppRuntime().themes.list(): Promise<ThemeCatalogSnapshot>`, `appLanguage.ready: boolean`, and `appTheme.ready: boolean`.
- Produces: a local `settingsStartupReady: boolean` that changes from false to true at most once during one `SettingsWindow` mount.

- [ ] **Step 1: Add the failing independent-Settings integration test**

Insert this test beside the existing independent Settings route tests in `packages/app/src/App.test.tsx`:

```tsx
it("keeps the Settings window mounted while Appearance refreshes the theme catalog", async () => {
  mockedConsumeWelcomeDocumentState.mockResolvedValue(false);
  window.history.pushState({}, "", "/?settings=1");
  const runtime = getAppRuntime();
  const catalogSnapshot = await runtime.themes.list();
  let resolveThemeRefresh: ((snapshot: typeof catalogSnapshot) => unknown) | null = null;
  runtime.themes.list = vi.fn()
    .mockResolvedValueOnce(catalogSnapshot)
    .mockImplementationOnce(() => new Promise<typeof catalogSnapshot>((resolve) => {
      resolveThemeRefresh = resolve;
    }));

  const { container } = renderApp();

  await waitFor(() => expect(container.querySelector(".settings-window")).toBeInTheDocument());
  await waitFor(() => expect(runtime.themes.list).toHaveBeenCalledTimes(1));
  await waitFor(() => expect(mockedMarkSettingsWindowReady).toHaveBeenCalledTimes(1));

  fireEvent.click(screen.getByRole("button", { name: "Appearance" }));

  await waitFor(() => expect(runtime.themes.list).toHaveBeenCalledTimes(2));
  expect(container.querySelector(".settings-window")).toBeInTheDocument();
  expect(screen.getByRole("heading", { level: 2, name: "Appearance" })).toBeInTheDocument();

  act(() => {
    resolveThemeRefresh?.(catalogSnapshot);
  });

  await waitFor(() => expect(screen.getByRole("heading", { level: 2, name: "Appearance" })).toBeInTheDocument());
  expect(runtime.themes.list).toHaveBeenCalledTimes(2);
  expect(mockedMarkSettingsWindowReady).toHaveBeenCalledTimes(1);
});
```

- [ ] **Step 2: Run the new test and verify RED**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/App.test.tsx -t "keeps the Settings window mounted while Appearance refreshes the theme catalog"
```

Expected: FAIL because `.settings-window` and the Appearance heading disappear while the second `themes.list()` promise is pending. The test must reach the pending refresh assertion; a syntax, type, or setup error is not an acceptable RED state.

- [ ] **Step 3: Implement the one-way Settings startup latch**

Change the React import in `packages/app/src/components/SettingsWindow.tsx`:

```tsx
import { useEffect, useState } from "react";
```

Replace the reversible readiness declaration with a live condition and a local latch:

```tsx
const liveSettingsStartupReady = appLanguage.ready && appTheme.ready;
const [settingsStartupReady, setSettingsStartupReady] = useState(liveSettingsStartupReady);
useEffect(() => {
  if (!settingsStartupReady && liveSettingsStartupReady) setSettingsStartupReady(true);
}, [liveSettingsStartupReady, settingsStartupReady]);
```

Keep the existing `markSettingsWindowReady` effect and `if (!settingsStartupReady) return null;` render gate unchanged. They now consume the latched value, so initial startup remains gated and later theme refreshes cannot blank the shell or repeat the native ready notification.

- [ ] **Step 4: Run the focused test and verify GREEN**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/App.test.tsx -t "keeps the Settings window mounted while Appearance refreshes the theme catalog"
```

Expected: PASS with one initial `markSettingsWindowReady` call and exactly two catalog calls: the initial load and the bounded Appearance refresh.

- [ ] **Step 5: Run focused theme and Settings regression tests**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/components/settings/AppearanceSettings.test.tsx src/hooks/useThemeCatalog.test.tsx src/hooks/useAppTheme.test.tsx src/App.test.tsx
```

Expected: all selected test files pass with no unhandled rejection, React act warning, or infinite-update warning.

- [ ] **Step 6: Run repository verification**

Run in order:

```bash
pnpm typecheck:test
pnpm build
git diff --check
```

Expected: every command exits 0. The build may print existing bundle-size notices, but it must not report a TypeScript, Vite, or workspace build failure.

- [ ] **Step 7: Verify the real desktop behavior**

Build and install or run the current desktop app through the repository's normal Tauri path, then use the active QingYu window to perform this sequence:

1. open Settings on General;
2. switch to Appearance and keep the catalog refresh in flight long enough to observe the surface;
3. confirm that the Settings shell and Appearance heading never disappear;
4. switch General → Appearance at least three times;
5. inspect `~/Library/Logs/dev.markra.app/QingYu.log` and confirm each entry causes a bounded `list_themes` request rather than a continuous `list_themes` / `mark_settings_window_ready` storm.

Expected: no blank frames, no accessibility-tree removal/recreation cycle, and no unbounded native-command loop.

- [ ] **Step 8: Commit the bug fix**

Stage only the production and regression-test files:

```bash
git add packages/app/src/App.test.tsx packages/app/src/components/SettingsWindow.tsx
git commit -m "fix: keep appearance settings stable during refresh"
```

Expected: one focused commit containing only the readiness latch and its regression test. Preserve unrelated files and commits in the shared checkout.
