# Configurable View Mode Shortcut Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a configurable `toggleViewMode` application shortcut, defaulting to `F8`, that cycles `daily -> focus -> immersive -> full -> daily` and maps `custom -> daily`.

**Architecture:** Extend the shared keyboard-shortcut grammar to admit unmodified function keys without admitting unmodified typing keys. Register `toggleViewMode` in the shared action registry and settings UI, then route it through `useApplicationShortcuts` to an App handler that reuses `nextViewMode` and the existing preference persistence path.

**Tech Stack:** TypeScript 6, React 19, Vitest, Testing Library, pnpm workspace.

## Global Constraints

- Use `pnpm` for dependency and verification commands.
- Do not add a dependency.
- Do not use the TypeScript `void` keyword or operator.
- Keep existing modified shortcut behavior unchanged.
- Only unmodified `F1` through `F12` are newly accepted; ordinary unmodified typing keys remain invalid.
- `F8` must match only an event with no Meta, Ctrl, Alt, or Shift modifier.
- Do not add a native menu item or change the title-bar view-mode selector.

---

### Task 1: Extend the shared shortcut grammar and registry

**Files:**
- Modify: `packages/shared/src/keyboard-shortcuts.test.ts`
- Modify: `packages/shared/src/keyboard-shortcuts.ts`

**Interfaces:**
- Produces: `KeyboardShortcutAction` containing `"toggleViewMode"`.
- Produces: `defaultKeyboardShortcuts.toggleViewMode === "F8"`.
- Produces: `ParsedKeyboardShortcut.mod: boolean`.
- Produces: `parseKeyboardShortcut`, `formatKeyboardShortcut`, `keyboardShortcutFromKeyboardEvent`, `keyboardShortcutToNativeAccelerator`, and `matchesKeyboardShortcutEvent` support for unmodified `F1` through `F12`.

- [ ] **Step 1: Write failing registry and function-key tests**

Add focused cases to `packages/shared/src/keyboard-shortcuts.test.ts`:

```ts
it("includes view mode cycling as a configurable F8 shortcut", () => {
  expect(keyboardShortcutActions).toContain("toggleViewMode");
  expect(defaultKeyboardShortcuts.toggleViewMode).toBe("F8");
  expect(normalizeKeyboardShortcuts({ toggleViewMode: "F9" }).toggleViewMode).toBe("F9");
});

it("records and matches unmodified function-key shortcuts", () => {
  const event = new KeyboardEvent("keydown", { code: "F8", key: "F8" });

  expect(formatKeyboardShortcut("F8")).toBe("F8");
  expect(keyboardShortcutFromKeyboardEvent(event)).toBe("F8");
  expect(matchesKeyboardShortcutEvent(event, "F8")).toBe(true);
});

it("does not match F8 when any modifier is pressed", () => {
  expect(matchesKeyboardShortcutEvent(new KeyboardEvent("keydown", {
    key: "F8",
    metaKey: true
  }), "F8")).toBe(false);
  expect(matchesKeyboardShortcutEvent(new KeyboardEvent("keydown", {
    key: "F8",
    shiftKey: true
  }), "F8")).toBe(false);
});

it("rejects unmodified typing keys", () => {
  expect(formatKeyboardShortcut("A")).toBeNull();
  expect(keyboardShortcutFromKeyboardEvent(new KeyboardEvent("keydown", {
    key: "a"
  }))).toBeNull();
});
```

- [ ] **Step 2: Run the focused shared test and confirm RED**

Run:

```bash
pnpm --filter @markra/shared test -- src/keyboard-shortcuts.test.ts
```

Expected: FAIL because `toggleViewMode` is absent and `F8` cannot be parsed, recorded, normalized, or matched.

- [ ] **Step 3: Implement the minimal function-key grammar and action**

In `packages/shared/src/keyboard-shortcuts.ts`, insert the new action and default binding alongside the existing application actions:

```ts
"toggleViewMode",

toggleViewMode: "F8",

export type ParsedKeyboardShortcut = {
  alt: boolean;
  key: string;
  mod: boolean;
  shift: boolean;
};

function normalizeShortcutKey(key: string) {
  if (/^f(?:[1-9]|1[0-2])$/iu.test(key)) return key.toUpperCase();
  if (/^[a-z]$/iu.test(key)) return key.toUpperCase();
  if (/^[0-9]$/u.test(key)) return key;
  if (punctuationShortcutKeys.has(key)) return key;

  return null;
}

function isFunctionShortcutKey(key: string) {
  return /^F(?:[1-9]|1[0-2])$/u.test(key);
}
```

Update parsing so `mod` is optional only when `key` is a function key and both `alt` and `shift` are false. Include `mod` in `ParsedKeyboardShortcut` and format `Mod` conditionally. Update event capture so an event without Meta/Ctrl is accepted only for `F1` through `F12` with no Alt or Shift modifier. Update matching to require `isKeyboardShortcutModKey(event) === parsed.mod` and exact Alt/Shift equality. Update native accelerator formatting to include `CmdOrCtrl` only when `parsed.mod` is true.

- [ ] **Step 4: Run the focused shared test and confirm GREEN**

Run:

```bash
pnpm --filter @markra/shared test -- src/keyboard-shortcuts.test.ts
```

Expected: PASS with all shared shortcut tests green.

- [ ] **Step 5: Commit the shared shortcut foundation**

```bash
git add packages/shared/src/keyboard-shortcuts.ts packages/shared/src/keyboard-shortcuts.test.ts
git commit -m "feat(shortcuts): support configurable function keys"
```

---

### Task 2: Expose view-mode cycling in shortcut settings

**Files:**
- Modify: `packages/app/src/components/settings/KeyboardShortcutsSettings.test.tsx`
- Modify: `packages/app/src/components/settings/KeyboardShortcutsSettings.tsx`
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
- Consumes: `MarkdownShortcutAction` now includes `"toggleViewMode"`.
- Produces: settings label key `app.toggleViewMode` in every locale.
- Produces: the application shortcut list includes `toggleViewMode` and displays plain `F8` without a platform modifier prefix.

- [ ] **Step 1: Write a failing settings test**

Add to `KeyboardShortcutsSettings.test.tsx`:

```tsx
it("displays and records the view mode function-key shortcut", () => {
  const onUpdatePreferences = vi.fn();
  const preferences: EditorPreferences = {
    ...defaultEditorPreferences,
    markdownShortcuts: defaultMarkdownShortcuts
  };

  render(
    <KeyboardShortcutsSettings
      preferences={preferences}
      translate={translate}
      onUpdatePreferences={onUpdatePreferences}
    />
  );

  const shortcut = screen.getByRole("button", { name: "Toggle view mode shortcut" });
  expect(shortcut).toHaveTextContent("F8");

  fireEvent.click(shortcut);
  fireEvent.keyDown(window, { code: "F9", key: "F9" });

  expect(onUpdatePreferences).toHaveBeenCalledWith({
    ...preferences,
    markdownShortcuts: {
      ...defaultMarkdownShortcuts,
      toggleViewMode: "F9"
    }
  });
});
```

- [ ] **Step 2: Run the settings test and confirm RED**

Run:

```bash
pnpm --filter @markra/app test -- src/components/settings/KeyboardShortcutsSettings.test.tsx
```

Expected: FAIL because no view-mode shortcut row or label exists.

- [ ] **Step 3: Add the row, formatter support, and translations**

In `KeyboardShortcutsSettings.tsx`, add:

```ts
toggleViewMode: "app.toggleViewMode"
```

to `markdownShortcutLabelKeys`, and add `"toggleViewMode"` to the application action list. Update `formatShortcutForPlatform` to prepend `Cmd`/`Ctrl` only when `parsed.mod` is true:

```ts
return [
  parsed.mod ? (platform === "macos" ? "⌘" : "Ctrl") : null,
  parsed.shift ? (platform === "macos" ? "⇧" : "Shift") : null,
  parsed.alt ? (platform === "macos" ? "⌥" : "Alt") : null,
  parsed.key
].filter((part): part is string => Boolean(part)).join("+");
```

Add `"app.toggleViewMode"` to `locales/types.ts` and use these translations:

```ts
// de.ts
"app.toggleViewMode": "Ansichtsmodus wechseln",
// en.ts
"app.toggleViewMode": "Toggle view mode",
// es.ts
"app.toggleViewMode": "Cambiar modo de vista",
// fr.ts
"app.toggleViewMode": "Changer de mode d’affichage",
// it.ts
"app.toggleViewMode": "Cambia modalità di visualizzazione",
// ja.ts
"app.toggleViewMode": "表示モードを切り替え",
// ko.ts
"app.toggleViewMode": "보기 모드 전환",
// pt-BR.ts
"app.toggleViewMode": "Alternar modo de visualização",
// ru.ts
"app.toggleViewMode": "Переключить режим просмотра",
// zh-CN.ts
"app.toggleViewMode": "切换视图模式",
// zh-TW.ts
"app.toggleViewMode": "切換檢視模式",
```

- [ ] **Step 4: Run settings and shared type checks**

Run:

```bash
pnpm --filter @markra/app test -- src/components/settings/KeyboardShortcutsSettings.test.tsx
pnpm --filter @markra/shared build
```

Expected: both commands exit 0.

- [ ] **Step 5: Commit the settings surface**

```bash
git add packages/app/src/components/settings/KeyboardShortcutsSettings.tsx packages/app/src/components/settings/KeyboardShortcutsSettings.test.tsx packages/shared/src/i18n/locales
git commit -m "feat(settings): configure view mode shortcut"
```

---

### Task 3: Route the shortcut and cycle App view modes

**Files:**
- Modify: `packages/app/src/hooks/useNativeBindings.test.tsx`
- Modify: `packages/app/src/hooks/useNativeBindings.ts`
- Modify: `packages/app/src/App.test.tsx`
- Modify: `packages/app/src/App.tsx`

**Interfaces:**
- Consumes: `defaultMarkdownShortcuts.toggleViewMode` and `nextViewMode(mode)`.
- Produces: optional `ApplicationShortcutOptions.toggleViewMode` callback.
- Produces: App handler that persists the next view mode through the same path as manual selection.

- [ ] **Step 1: Write a failing shortcut routing test**

Add to `useNativeBindings.test.tsx`:

```tsx
it("routes the configurable view mode shortcut", () => {
  const toggleViewMode = vi.fn();
  renderHook(() =>
    useApplicationShortcuts({
      ...baseOptions,
      markdownShortcuts: { toggleViewMode: "F9" },
      toggleViewMode
    })
  );

  fireEvent.keyDown(window, { code: "F9", key: "F9" });

  expect(toggleViewMode).toHaveBeenCalledTimes(1);
});
```

- [ ] **Step 2: Run the hook test and confirm RED**

Run:

```bash
pnpm --filter @markra/app test -- src/hooks/useNativeBindings.test.tsx
```

Expected: FAIL because `toggleViewMode` is not part of `ApplicationShortcutOptions` or the configurable action routing table.

- [ ] **Step 3: Add minimal hook routing**

Add `toggleViewMode?: () => unknown | Promise<unknown>` to `ApplicationShortcutOptions`, destructure it in `useApplicationShortcuts`, add this routing entry, and include it in the effect dependencies:

```ts
[normalizedMarkdownShortcuts.toggleViewMode, toggleViewMode]
```

Move the configurable-action loop before the `isModKey` guard. Keep `event.defaultPrevented` as the initial guard, run the configurable loop, then return when `!isModKey` before evaluating built-in `Cmd`/`Ctrl` shortcuts. This lets `F8` reach the configured handler without changing the modifier requirement for save, open, search, and other built-in shortcuts.

- [ ] **Step 4: Run the hook test and confirm GREEN**

Run:

```bash
pnpm --filter @markra/app test -- src/hooks/useNativeBindings.test.tsx
```

Expected: PASS.

- [ ] **Step 5: Write failing App integration tests**

In `App.test.tsx`, add tests that start from `immersive` and `custom`, send an unmodified `F8` keydown to `window`, and assert persisted transitions to `full` and `daily` respectively:

```tsx
it("cycles the workspace view mode from the F8 shortcut", async () => {
  mockedGetStoredEditorPreferences.mockResolvedValue(createStoredEditorPreferences({
    viewMode: "immersive"
  }));

  renderApp();
  await screen.findByRole("main");
  fireEvent.keyDown(window, { code: "F8", key: "F8" });

  await waitFor(() => expect(mockedSaveStoredEditorPreferences).toHaveBeenCalledWith(
    expect.objectContaining({ viewMode: "full" })
  ));
  expect(mockedNotifyAppEditorPreferencesChanged).toHaveBeenCalledWith(
    expect.objectContaining({ viewMode: "full" })
  );
});

it("cycles custom view mode back to daily", async () => {
  mockedGetStoredEditorPreferences.mockResolvedValue(createStoredEditorPreferences({
    viewMode: "custom"
  }));

  renderApp();
  await screen.findByRole("button", { name: "View mode: Custom" });
  fireEvent.keyDown(window, { code: "F8", key: "F8" });

  await waitFor(() => expect(mockedSaveStoredEditorPreferences).toHaveBeenCalledWith(
    expect.objectContaining({ viewMode: "daily" })
  ));
});
```

- [ ] **Step 6: Run the App test and confirm RED**

Run:

```bash
pnpm --filter @markra/app test -- src/App.test.tsx
```

Expected: the new integration tests FAIL because App does not pass a view-mode callback to `useApplicationShortcuts`.

- [ ] **Step 7: Implement the App cycle handler**

Import `nextViewMode` beside the existing view-mode helpers. Add a callback that reuses `handleViewModeSelect`:

```ts
const handleViewModeCycle = useCallback(() => {
  handleViewModeSelect(nextViewMode(editorPreferences.preferences.viewMode));
}, [editorPreferences.preferences.viewMode, handleViewModeSelect]);
```

Add the callback to the existing `useApplicationShortcuts` options object:

```ts
toggleViewMode: handleViewModeCycle,
```

- [ ] **Step 8: Run the focused App tests and confirm GREEN**

Run:

```bash
pnpm --filter @markra/app test -- src/hooks/useNativeBindings.test.tsx src/App.test.tsx
```

Expected: PASS.

- [ ] **Step 9: Commit the behavior**

```bash
git add packages/app/src/hooks/useNativeBindings.ts packages/app/src/hooks/useNativeBindings.test.tsx packages/app/src/App.tsx packages/app/src/App.test.tsx
git commit -m "feat(app): cycle view modes with configurable shortcut"
```

---

### Task 4: Verify the complete feature

**Files:**
- Verify only; no planned production changes.

**Interfaces:**
- Consumes: all behavior from Tasks 1-3.
- Produces: fresh evidence that tests, type checks, and production build pass.

- [ ] **Step 1: Run focused regression tests**

```bash
pnpm --filter @markra/shared test -- src/keyboard-shortcuts.test.ts
pnpm --filter @markra/app test -- src/components/settings/KeyboardShortcutsSettings.test.tsx src/hooks/useNativeBindings.test.tsx src/App.test.tsx
```

Expected: all selected test files pass with zero failures.

- [ ] **Step 2: Run workspace test and build gates**

```bash
pnpm test
pnpm typecheck:test
pnpm build
```

Expected: every command exits 0.

- [ ] **Step 3: Inspect the final diff**

```bash
git status --short
git diff HEAD~3 --check
git diff HEAD~3 --stat
```

Expected: no whitespace errors, no generated directories, and only the shortcut, settings, localization, hook, App, test, and plan/spec files are changed.
