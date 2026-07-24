# Settings Storage Category Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move configuration transfer into General, move image file naming into Editor, and remove the misleading desktop Storage category without changing settings behavior.

**Architecture:** Keep the existing `useSettingsWindowState` handlers and persisted settings unchanged. Recompose the existing controls inside `GeneralSettings` and `EditorSettings`, then remove only the desktop `SettingsCategory` route and obsolete `StorageSettings` component. Compact settings retain their separate Storage detail because it displays real workspace storage information rather than these controls.

**Tech Stack:** React 19, TypeScript 6, Tailwind CSS, Vitest, Testing Library, pnpm workspace.

## Global Constraints

- Use `pnpm` for JavaScript and frontend commands.
- Do not use the TypeScript `void` keyword or operator.
- Preserve existing configuration import/export handlers, persistence keys, normalization, and toasts.
- Preserve existing image-name generation and preference persistence.
- Do not change compact settings, note backup, or project synchronization behavior.
- Preserve the intentional untracked `bg.png` file.

---

### Task 1: Rehome the two controls

**Files:**
- Modify: `packages/app/src/components/settings/GeneralSettings.test.tsx`
- Modify: `packages/app/src/components/settings/GeneralSettings.tsx`
- Modify: `packages/app/src/components/settings/EditorSettings.test.tsx`
- Modify: `packages/app/src/components/settings/EditorSettings.tsx`
- Modify: `packages/app/src/components/SettingsWindow.tsx`
- Modify: `packages/shared/src/i18n/locales/types.ts`
- Modify: all locale files under `packages/shared/src/i18n/locales/`

**Interfaces:**
- Consumes: `handleExportSettings(): Promise<unknown>`, `handleImportSettings(): Promise<unknown>`, `settingsTransferRunning: boolean`, and `handleUpdateEditorPreferences(preferences: EditorPreferences)` from `useSettingsWindowState`.
- Produces: `GeneralSettings` props `onExportSettings?: () => unknown`, `onImportSettings?: () => unknown`, and `settingsTransferRunning?: boolean`; `EditorSettings` continues to use its existing props.

- [ ] **Step 1: Write failing destination tests**

Add a General test that renders the existing transfer handlers in their new destination and checks action dispatch and running-state disabling:

```tsx
it("exports and imports portable settings from general settings", () => {
  const onExportSettings = vi.fn();
  const onImportSettings = vi.fn();
  const props = {
    appVersion: "0.0.7",
    language: "en" as const,
    preferences: defaultEditorPreferences,
    translate,
    welcomeReset: false,
    onCheckForUpdates: vi.fn(),
    onExportSettings,
    onImportSettings,
    onResetWelcomeDocument: vi.fn(),
    onSelectLanguage: vi.fn(),
    onUpdatePreferences: vi.fn()
  };

  const { rerender } = render(<GeneralSettings {...props} />);
  fireEvent.click(screen.getByRole("button", { name: "Export settings" }));
  fireEvent.click(screen.getByRole("button", { name: "Import settings" }));
  expect(onExportSettings).toHaveBeenCalledTimes(1);
  expect(onImportSettings).toHaveBeenCalledTimes(1);

  rerender(<GeneralSettings {...props} settingsTransferRunning />);
  expect(screen.getByRole("button", { name: "Export settings" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "Import settings" })).toBeDisabled();
});
```

Add an Editor test that preserves the existing file-name preference assertion:

```tsx
it("updates the image file naming pattern from editor settings", () => {
  const onUpdatePreferences = vi.fn();
  render(
    <EditorSettings
      preferences={defaultEditorPreferences}
      translate={translate}
      onUpdatePreferences={onUpdatePreferences}
    />
  );

  fireEvent.change(screen.getByRole("textbox", { name: "File naming pattern" }), {
    target: { value: "{name}-{timestamp}" }
  });
  expect(onUpdatePreferences).toHaveBeenCalledWith({
    ...defaultEditorPreferences,
    imageUpload: { fileNamePattern: "{name}-{timestamp}" }
  });
});
```

- [ ] **Step 2: Run the focused tests and confirm they fail**

Run:

```bash
pnpm --filter @markra/app test -- src/components/settings/GeneralSettings.test.tsx src/components/settings/EditorSettings.test.tsx
```

Expected: FAIL because General has no transfer buttons and Editor has no file-name input.

- [ ] **Step 3: Add the controls to their destination components**

In `GeneralSettings.tsx`, import `Download` and `Upload`, add the three transfer props, and append a final section using a new `settings.sections.settingsTransfer` label. Preserve the current button behavior:

```tsx
<SettingsSection label={translate("settings.sections.settingsTransfer")}>
  <SettingsRow
    title={translate("settings.storage.settingsBackup")}
    description={translate("settings.storage.settingsBackupDescription")}
    action={
      <div className="inline-flex items-center gap-2">
        <SettingsButton
          disabled={settingsTransferRunning || !onExportSettings}
          label={translate("settings.storage.exportSettings")}
          onClick={() => onExportSettings?.()}
        >
          <Download aria-hidden="true" size={13} />
          {translate("settings.storage.exportSettings")}
        </SettingsButton>
        <SettingsButton
          disabled={settingsTransferRunning || !onImportSettings}
          label={translate("settings.storage.importSettings")}
          onClick={() => onImportSettings?.()}
        >
          <Upload aria-hidden="true" size={13} />
          {translate("settings.storage.importSettings")}
        </SettingsButton>
      </div>
    }
  />
</SettingsSection>
```

In `EditorSettings.tsx`, import `SettingsTextInput` and append the file-name row to `settings.sections.editing`:

```tsx
<SettingsRow
  title={translate("settings.editor.imageUploadFileNamePattern")}
  description={translate("settings.editor.imageUploadFileNamePatternDescription")}
  action={
    <SettingsTextInput
      label={translate("settings.editor.imageUploadFileNamePattern")}
      value={preferences.imageUpload.fileNamePattern}
      placeholder="pasted-image-{timestamp}"
      widthClassName="w-64"
      onChange={(fileNamePattern) =>
        onUpdatePreferences({
          ...preferences,
          imageUpload: { fileNamePattern }
        })
      }
    />
  }
/>
```

Pass `handleExportSettings`, `handleImportSettings`, and `settingsTransferRunning` to `GeneralSettings` in `SettingsWindow.tsx`. Add `settings.sections.settingsTransfer` to the translation key type and every locale, using `Settings transfer` as the existing untranslated-locale fallback, `配置迁移` for Simplified Chinese, `設定移轉` for Traditional Chinese, `設定の移行` for Japanese, and `설정 이전` for Korean.

- [ ] **Step 4: Run the focused tests and confirm they pass**

Run the Task 1 command again. Expected: both files PASS.

- [ ] **Step 5: Commit the destination controls**

```bash
git add packages/app/src/components/settings/GeneralSettings.test.tsx packages/app/src/components/settings/GeneralSettings.tsx packages/app/src/components/settings/EditorSettings.test.tsx packages/app/src/components/settings/EditorSettings.tsx packages/app/src/components/SettingsWindow.tsx packages/shared/src/i18n/locales
git commit -m "refactor: group settings by responsibility"
```

---

### Task 2: Remove the obsolete desktop Storage category

**Files:**
- Modify: `packages/app/src/components/SettingsShell.test.tsx`
- Modify: `packages/app/src/components/SettingsShell.tsx`
- Modify: `packages/app/src/components/SettingsSections.tsx`
- Modify: `packages/app/src/components/SettingsWindow.tsx`
- Modify: `packages/app/src/hooks/useSettingsWindowState.ts`
- Delete: `packages/app/src/components/settings/StorageSettings.test.tsx`
- Delete: `packages/app/src/components/settings/StorageSettings.tsx`
- Modify: `packages/shared/src/i18n/locales/types.ts`
- Modify: all locale files under `packages/shared/src/i18n/locales/`

**Interfaces:**
- Consumes: the destination props and controls completed in Task 1.
- Produces: desktop `SettingsCategory` without `storage`; compact `CompactSettingsCategory` remains unchanged and still includes `storage`.

- [ ] **Step 1: Change the sidebar regression test first**

Replace the current Storage click test and active Storage heading test with:

```tsx
it("does not expose a standalone storage category", () => {
  renderSettingsSidebar();
  expect(screen.queryByRole("button", { name: "Storage" })).not.toBeInTheDocument();
});
```

- [ ] **Step 2: Run the sidebar test and confirm it fails**

Run:

```bash
pnpm --filter @markra/app test -- src/components/SettingsShell.test.tsx
```

Expected: FAIL because Storage is still rendered in the desktop sidebar.

- [ ] **Step 3: Remove the desktop route and obsolete component**

- Remove `"storage"` from `SettingsCategory` in `useSettingsWindowState.ts`.
- Remove `HardDrive` and the Storage category definition from `SettingsShell.tsx`.
- Remove the `StorageSettings` export from `SettingsSections.tsx`.
- Remove the `StorageSettings` import and conditional branch from `SettingsWindow.tsx`.
- Delete `StorageSettings.tsx` and `StorageSettings.test.tsx` after their behavior has moved to the Task 1 tests.
- Remove only `settings.categories.storage` from the locale key type and locale maps. Keep every `settings.storage.*` transfer key because General and the transfer handlers still consume them.
- Do not modify `CompactSettingsCategory`, compact history parsing, compact category lists, or `CompactSettingsDetail`.

- [ ] **Step 4: Run the affected settings tests**

Run:

```bash
pnpm --filter @markra/app test -- src/components/SettingsShell.test.tsx src/components/settings/GeneralSettings.test.tsx src/components/settings/EditorSettings.test.tsx
```

Expected: all three files PASS.

- [ ] **Step 5: Commit the category cleanup**

```bash
git add packages/app/src/components/SettingsShell.test.tsx packages/app/src/components/SettingsShell.tsx packages/app/src/components/SettingsSections.tsx packages/app/src/components/SettingsWindow.tsx packages/app/src/hooks/useSettingsWindowState.ts packages/app/src/components/settings/StorageSettings.tsx packages/app/src/components/settings/StorageSettings.test.tsx packages/shared/src/i18n/locales
git commit -m "refactor: remove desktop storage settings category"
```

---

### Task 3: Verify the complete change

**Files:**
- Verify only; no planned source changes.

**Interfaces:**
- Consumes: Tasks 1 and 2.
- Produces: test and build evidence for handoff.

- [ ] **Step 1: Confirm no obsolete desktop references remain**

Run:

```bash
rg -n 'StorageSettings|settings\.categories\.storage|activeSettingsCategory === "storage"' packages/app/src packages/shared/src
```

Expected: no `StorageSettings`, desktop route condition, or shared category translation key remains. Compact code may still contain its independent `storage` category.

- [ ] **Step 2: Run the repository frontend test suite**

Run `pnpm test`. Expected: PASS.

- [ ] **Step 3: Run test TypeScript validation**

Run `pnpm typecheck:test`. Expected: PASS.

- [ ] **Step 4: Run the production build**

Run `pnpm build`. Expected: PASS.

- [ ] **Step 5: Review the final diff**

Run:

```bash
git diff --check HEAD~2..HEAD
git status --short
```

Expected: no whitespace errors; only the intentional untracked `bg.png` remains outside committed work.
