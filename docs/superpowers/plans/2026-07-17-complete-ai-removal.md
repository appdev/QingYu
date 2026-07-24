# QingYu Complete AI Removal Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove every active AI capability, interface, dependency, setting, and current product claim while preserving QingYu's ordinary note-taking, formatting, network-image, and S3 sync behavior.

**Architecture:** First detach the retained selection-formatting behavior from AI types and names. Then remove AI orchestration from the application, settings, menus, runtimes, and native shell before deleting the AI/provider packages and their dependencies. Finish by rewriting current product documentation and proving the remaining desktop application works through static, automated, and live verification.

**Tech Stack:** pnpm workspace, React 19, TypeScript 6, Milkdown/ProseMirror, Tailwind CSS, Tauri v2, Rust, Vitest.

## Global Constraints

- Delete AI implementation; do not hide it behind a feature flag or leave compatibility shells.
- Per the user-approved 2026-07-18 Task 5 amendment, delete spellcheck completely as well: editor/UI/settings/preferences/runtime/dictionary/localization/tests/dependencies, with no migration or compatibility alias.
- Do not migrate or delete prerelease development data.
- Preserve ordinary selection formatting, web image download, Markdown editing, backup, export, and S3 note-folder sync.
- Preserve `CHANGELOG.md`, Git history, existing `docs/superpowers` records, and the untracked `bg.png` file.
- Use `pnpm` for JavaScript dependency and verification commands.
- Do not use the TypeScript `void` keyword or operator.
- Do not push any commit to a remote.
- Keep each commit limited to the task being completed.

---

## File and Interface Map

- `packages/editor/src/selection-hold.ts` owns the retained visual selection decoration and must export neutral `EditorTextSelection`, `showSelectionHold`, `clearSelectionHold`, and `markraSelectionHoldPlugin` interfaces.
- `packages/app/src/components/SelectionToolbar.tsx` owns retained selection-formatting UI without AI actions.
- `packages/app/src/hooks/useEditorController.ts` and `packages/app/src/components/markdown-paper-plugins.ts` produce and consume `EditorTextSelection` without importing `@markra/ai`.
- `packages/app/src/App.tsx` composes only note-taking behavior after AI state, commands, panels, and workspace-change flows are removed.
- `packages/app/src/lib/settings/app-settings.ts` is the single persisted-settings source of truth and must contain only retained settings.
- `packages/app/src/runtime/index.ts` defines the cross-platform runtime contract; AI, ACP, and chat-attachment interfaces must disappear from it before platform implementations are deleted.
- `apps/desktop/src-tauri/src/web_http.rs` remains the native web-image download boundary and must not be deleted with `ai_http.rs`.
- `packages/shared/src/keyboard-shortcuts.ts` and the locale files define the complete shortcut and current-copy surface.
- `packages/ai` and `packages/providers` are deleted only after all consumers compile without them.

---

### Task 1: Detach Ordinary Selection Formatting from AI

**Files:**
- Rename: `packages/app/src/components/AiSelectionToolbar.tsx` → `packages/app/src/components/SelectionToolbar.tsx`
- Rename: `packages/app/src/components/AiSelectionToolbar.test.tsx` → `packages/app/src/components/SelectionToolbar.test.tsx`
- Delete: `packages/app/src/hooks/ai-selection-reveal.ts`
- Delete: `packages/app/src/hooks/ai-selection-reveal.test.tsx`
- Modify: `packages/editor/src/selection-hold.ts`
- Modify: `packages/editor/src/index.ts`
- Modify: `packages/app/src/hooks/useEditorController.ts`
- Modify: `packages/app/src/components/MarkdownPaperSurface.tsx`
- Modify: `packages/app/src/components/markdown-paper-plugins.ts`
- Modify: `packages/app/src/components/MarkdownPaper.test.tsx`
- Modify: `packages/app/src/App.tsx`
- Modify: `packages/app/src/styles.css`

**Interfaces:**
- Produces: `EditorTextSelection = { from: number; text: string; to: number }` from `@markra/editor`.
- Produces: `SelectionToolbar` with only formatting, heading, link, copy, placement, and dismissal props.
- Produces: `showSelectionHold`, `clearSelectionHold`, and `markraSelectionHoldPlugin`.
- Consumes: `SelectionAnchor`, `SelectionFormattingAction`, `SelectionFormattingToolbarAction`, and `SelectionHeadingLevel` from existing app modules.

- [ ] **Step 1: Rewrite the toolbar test around retained behavior**

Use `git mv` for both toolbar files and replace the AI-preset test with a formatting-only contract:

```tsx
describe("SelectionToolbar", () => {
  it("renders ordinary formatting actions without AI actions", () => {
    render(
      <SelectionToolbar
        anchor={anchor}
        language="en"
        open
        onCopySelection={vi.fn()}
        onInsertLink={vi.fn()}
        onRunFormattingAction={vi.fn()}
      />
    );

    expect(screen.getByRole("toolbar", { name: "Format" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Bold" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Link" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Copy" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Polish" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "AI command" })).not.toBeInTheDocument();
  });
});
```

Keep and rename the existing placement, active-formatting, heading-menu, copy, and dismissal tests. Remove AI prompt imports and the `onOpenCommand`/`onRunAction` props from every render.

- [ ] **Step 2: Run the renamed test and verify it fails**

Run:

```bash
pnpm --filter @markra/app test -- SelectionToolbar.test.tsx
```

Expected: FAIL because `SelectionToolbar` and the formatting-only accessible label are not implemented yet.

- [ ] **Step 3: Implement the neutral selection interfaces**

In `packages/editor/src/selection-hold.ts`, remove the `@markra/ai` import and use:

```ts
export type EditorTextSelection = {
  from: number;
  text: string;
  to: number;
};

export function showSelectionHold(view: EditorView, selection: EditorTextSelection) {
  view.dispatch(view.state.tr.setMeta(selectionHoldKey, { kind: "show", selection } satisfies SelectionHoldMeta));
}

export function clearSelectionHold(view: EditorView) {
  view.dispatch(view.state.tr.setMeta(selectionHoldKey, { kind: "clear" } satisfies SelectionHoldMeta));
}
```

Rename the plugin key to `markra-selection-hold`, the CSS class to `markra-selection-hold`, and the plugin export to `markraSelectionHoldPlugin`. Keep the current decoration mapping behavior.

In `SelectionToolbar.tsx`, delete the AI imports, AI action array, command button, prompt props, and AI action callbacks. Use `menu.format` as the toolbar label and `menu.copy` as the copy button label. Rename CSS classes and animation names from `ai-selection-*`/`markra-ai-*` to `selection-*`/`markra-*`.

In `useEditorController.ts`, replace `AiSelectionContext` with `EditorTextSelection` and rename `readAiSelectionContextFromView`, `holdAiSelection`, and their consumers to `readTextSelectionFromView`, `holdSelection`, and neutral equivalents. Return only `from`, `text`, and `to`.

Update `MarkdownPaperSurface.tsx`, `markdown-paper-plugins.ts`, `App.tsx`, and retained MarkdownPaper tests to consume the neutral interfaces. Remove `markraAiEditorPreviewPlugin` from `MarkdownPaperSurface.tsx`; preview-specific tests will be deleted in Task 2.

- [ ] **Step 4: Remove deferred reveal and show ordinary tools from the live selection**

Use `EditorTextSelection`. A valid editable visual-editor selection sets the toolbar anchor immediately; a cancelled or missing selection, source mode, or read-only mode clears it. Do not retain a delayed-reveal hook or add a settings switch for this behavior.

```ts
const selectionToolbarVisible =
  !sourceSurfaceActive &&
  !readOnlyMode &&
  selectionToolbarAnchor !== null &&
  Boolean(activeTextSelection?.text.trim());
```

Add App behavior coverage for ordinary formatting, link, and copy controls, plus cancelled-selection and read-only hiding. Keep selection-anchor refresh only for layout changes while the toolbar is already visible.

- [ ] **Step 5: Run focused editor and app tests**

Run:

```bash
pnpm --filter @markra/editor test -- selection-hold
pnpm --filter @markra/app test -- SelectionToolbar.test.tsx MarkdownPaper.test.tsx App.test.tsx
```

Expected: all selected test files PASS; retained selection formatting works without importing an AI type.

- [ ] **Step 6: Commit the neutral selection boundary**

```bash
git add packages/editor/src packages/app/src/components/SelectionToolbar.tsx packages/app/src/components/SelectionToolbar.test.tsx packages/app/src/components/MarkdownPaperSurface.tsx packages/app/src/components/MarkdownPaper.test.tsx packages/app/src/components/markdown-paper-plugins.ts packages/app/src/hooks/useEditorController.ts packages/app/src/App.tsx packages/app/src/styles.css
git commit -m "refactor: detach selection formatting from AI"
```

---

### Task 2: Remove Application AI Orchestration and Workspace Mutation UI

**Files:**
- Delete: `packages/app/src/App.ai.test.tsx`
- Delete: `packages/app/src/components/AiAgentPanel.tsx`
- Delete: `packages/app/src/components/AiAgentPanel.test.tsx`
- Delete: `packages/app/src/components/AiAgentProcessList.tsx`
- Delete: `packages/app/src/components/AiAgentSessionMenu.tsx`
- Delete: `packages/app/src/components/AiAgentSessionMenu.test.tsx`
- Delete: `packages/app/src/components/AiCommandBar.tsx`
- Delete: `packages/app/src/components/AiCommandBar.test.tsx`
- Delete: `packages/app/src/components/AiMarkdownMessage.tsx`
- Delete: `packages/app/src/components/AiModelCapabilities.tsx`
- Delete: `packages/app/src/components/AiModelPicker.tsx`
- Delete: `packages/app/src/components/AiModelPicker.test.tsx`
- Delete: `packages/app/src/components/WorkspaceOperationOverlay.tsx`
- Delete: `packages/app/src/components/WorkspaceOperationOverlay.test.tsx`
- Delete: `packages/app/src/hooks/ai-agent-panel-visibility.ts`
- Delete: `packages/app/src/hooks/ai-agent-panel-visibility.test.ts`
- Delete: `packages/app/src/hooks/useAiAgentPanelState.ts`
- Delete: `packages/app/src/hooks/useAiAgentPanelState.test.tsx`
- Delete: `packages/app/src/hooks/useAiAgentSession.ts`
- Delete: `packages/app/src/hooks/useAiAgentSession.test.tsx`
- Delete: `packages/app/src/hooks/useAiAgentSessionList.ts`
- Delete: `packages/app/src/hooks/useAiAgentSessionList.test.tsx`
- Delete: `packages/app/src/hooks/useAiCommandUi.ts`
- Delete: `packages/app/src/hooks/useAiCommandUi.test.tsx`
- Delete: `packages/app/src/lib/ai-actions.ts`
- Delete: `packages/app/src/lib/ai-chat-attachments.ts`
- Delete: `packages/app/src/lib/ai-chat-attachments.test.ts`
- Delete: `packages/app/src/lib/ai-selection.ts`
- Delete: `packages/app/src/lib/ai-selection.test.ts`
- Delete: `packages/app/src/lib/workspace-operation-animation.ts`
- Delete: `packages/app/src/lib/workspace-operation-animation.test.ts`
- Delete: `packages/app/src/lib/workspace-plan-apply.ts`
- Delete: `packages/app/src/lib/workspace-plan-apply.test.ts`
- Delete: `packages/app/src/lib/workspace-plan-events.ts`
- Delete: `packages/app/src/lib/workspace-plan-events.test.ts`
- Delete: `packages/app/src/test/ai-fixtures.ts`
- Modify: `packages/app/src/App.tsx`
- Modify: `packages/app/src/App.test.tsx`
- Modify: `packages/app/src/app/workspace-model.ts`
- Modify: `packages/app/src/components/MarkdownPaper.test.tsx`
- Modify: `packages/app/src/components/WorkspaceLayout.tsx`
- Modify: `packages/app/src/hooks/useMarkdownDocument.ts`
- Modify: `packages/app/src/hooks/useMarkdownDocument.test.tsx`
- Modify: `packages/app/src/hooks/useMarkdownFileTree.ts`
- Modify: `packages/app/src/hooks/useMarkdownFileTree.test.tsx`
- Modify: `packages/app/src/test/app-harness.tsx`
- Modify: `packages/app/src/styles.css`

**Interfaces:**
- Consumes: neutral selection interfaces from Task 1.
- Produces: an `App` tree with no AI panel, command bar, preview events, agent session, AI attachment, or workspace-plan state.
- Produces: note/document hooks that expose only user-driven document and file operations.

- [ ] **Step 1: Remove AI-only tests and modules**

Delete the files listed above with `git rm`. In `MarkdownPaper.test.tsx`, delete preview-event test blocks using `AI_EDITOR_PREVIEW_ACTION_EVENT`, `AI_EDITOR_PREVIEW_APPLIED_EVENT`, or `AI_EDITOR_PREVIEW_RESTORE_EVENT`; retain and rename selection-hold and formatting tests.

- [ ] **Step 2: Strip AI composition from `App.tsx`**

Remove imports, state, refs, effects, handlers, and JSX for:

```text
AiAgentPanel
AiCommandBar
AI_EDITOR_PREVIEW_ACTION_EVENT
AI_EDITOR_PREVIEW_APPLIED_EVENT
AI_EDITOR_PREVIEW_RESTORE_EVENT
activeAiSelection
aiAgentOpen
aiAgentSession
aiCommand
aiProviderSettings
aiWorkspaceOperation
webSearchSettings
```

Keep the neutral `SelectionToolbar` state and formatting handlers. Its final render must have this prop shape:

```tsx
<SelectionToolbar
  activeFormattingActions={selectionToolbarActiveActions}
  activeHeadingLevel={selectionToolbarHeadingLevel}
  anchor={selectionToolbarAnchor}
  copySucceeded={selectionToolbarCopySucceeded}
  language={language.language}
  onCopySelection={handleSelectionToolbarCopySelection}
  onDismiss={handleSelectionToolbarDismiss}
  onInsertLink={handleSelectionToolbarInsertLink}
  onRunFormattingAction={handleSelectionToolbarFormattingAction}
  onSetHeadingLevel={handleSelectionToolbarHeadingLevelAction}
  open={selectionToolbarAnchor !== null}
/>
```

Remove AI width/resizing props from `WorkspaceLayout` and delete the operation overlay render.

- [ ] **Step 3: Remove AI mutation helpers from retained hooks**

In `useMarkdownDocument.ts`, `useMarkdownFileTree.ts`, and `workspace-model.ts`, delete methods and state used only by agent edits, deferred AI markdown changes, workspace change plans, or AI operation animation. Preserve save conflict handling, file watching, document history, and project sync coordination.

Update `app-harness.tsx` to remove AI event helpers, AI fixtures, AI runtime mocks, and AI default preferences while preserving its generic document/file/settings harness.

- [ ] **Step 4: Verify the application layer has no direct AI orchestration**

Run:

```bash
rg -n 'AiAgent|AiCommand|AI_EDITOR_PREVIEW|useAiAgent|useAiCommand|workspacePlan|aiWorkspaceOperation' packages/app/src/App.tsx packages/app/src/app packages/app/src/hooks packages/app/src/components packages/app/src/lib
```

Expected: no matches except provider/settings files scheduled for Task 3 and runtime bridge files scheduled for Task 5.

- [ ] **Step 5: Run focused application tests**

Run:

```bash
pnpm --filter @markra/app test -- App.test.tsx MarkdownPaper.test.tsx useMarkdownDocument.test.tsx useMarkdownFileTree.test.tsx
```

Expected: selected tests PASS with no AI runtime mock required.

- [ ] **Step 6: Commit application removal**

```bash
git add packages/app/src
git commit -m "refactor: remove AI application flows"
```

---

### Task 3: Remove AI Settings, Providers, and Stored State

**Files:**
- Delete: `packages/app/src/components/AiProviderBadge.tsx`
- Delete: `packages/app/src/components/AiProviderBadge.test.tsx`
- Delete: `packages/app/src/components/AiProviderConnectionSection.tsx`
- Delete: `packages/app/src/components/AiProviderConnectionSection.test.tsx`
- Delete: `packages/app/src/components/AiProviderConnectionSection.lazy.test.tsx`
- Delete: `packages/app/src/components/AiProviderDetailHeader.tsx`
- Delete: `packages/app/src/components/AiProviderDetailHeader.test.tsx`
- Delete: `packages/app/src/components/AiProviderList.tsx`
- Delete: `packages/app/src/components/AiProviderModelsSection.tsx`
- Delete: `packages/app/src/components/AiProviderSettingsControls.tsx`
- Delete: `packages/app/src/components/AiProviderSettingsPanel.tsx`
- Delete: `packages/app/src/components/settings/AiSettings.tsx`
- Delete: `packages/app/src/components/settings/AiSettings.test.tsx`
- Delete: `packages/app/src/components/settings/WebSearchSettings.tsx`
- Delete: `packages/app/src/hooks/useAcpAgentSettings.ts`
- Delete: `packages/app/src/hooks/useAcpAgentSettings.test.tsx`
- Delete: `packages/app/src/hooks/useAiProviderSettingsPanelState.ts`
- Delete: `packages/app/src/hooks/useAiSettings.ts`
- Delete: `packages/app/src/hooks/useAiSettings.test.tsx`
- Delete: `packages/app/src/hooks/useWebSearchSettings.ts`
- Delete: `packages/app/src/lib/settings/ai-agent-sessions.test.ts`
- Delete: `packages/app/src/lib/settings/ai-settings.test.ts`
- Delete: `packages/app/src/lib/settings/web-search-settings.ts`
- Delete: `packages/app/src/lib/settings/web-search-settings.test.ts`
- Modify: `packages/app/src/components/SettingsShell.tsx`
- Modify: `packages/app/src/components/SettingsSections.tsx`
- Modify: `packages/app/src/components/SettingsWindow.tsx`
- Modify: `packages/app/src/components/settings/EditorSettings.tsx`
- Modify: `packages/app/src/components/settings/EditorSettings.test.tsx`
- Modify: `packages/app/src/hooks/useEditorPreferences.ts`
- Modify: `packages/app/src/hooks/useEditorPreferences.test.tsx`
- Modify: `packages/app/src/hooks/useSettingsWindowState.ts`
- Modify: `packages/app/src/hooks/useSettingsWindowState.test.ts`
- Modify: `packages/app/src/lib/diagnostics/diagnostics-report.ts`
- Modify: `packages/app/src/lib/diagnostics/diagnostics-report.test.ts`
- Modify: `packages/app/src/lib/settings/app-settings.ts`
- Modify: `packages/app/src/lib/settings/editor-preferences.test.ts`
- Modify: `packages/app/src/lib/settings/settings-events.ts`
- Modify: `packages/app/src/lib/settings/settings-events.test.ts`

**Interfaces:**
- Produces: `EditorPreferences` without an AI or neutralized selection-toolbar enablement field; ordinary toolbar visibility is derived from the live editable selection.
- Produces: `PortableStoredAppSettings` with only appearance, backup, custom themes, editor, export, file-ignore, language, network, and retained settings.
- Produces: settings categories `general | network | storage | backup | sync | logs | appearance | view | editor | templates | keyboardShortcuts | export` after the Task 5 amendment.

- [ ] **Step 1: Add settings tests for the reduced schema**

Update the complete `editor-preferences.test.ts` schema expectations and fixtures so they contain only retained editor settings. Do not add an absence-only test for the deleted switch; the full schema expectations, App behavior tests, and Task 8 scans provide the removal evidence.

Update settings import/export tests in `useSettingsWindowState.test.ts` to assert that the returned settings object has no `acpAgentSettings`, `aiAgentPreferences`, `aiProviders`, or `webSearch` property and that `editorPreferences` has no AI-prefixed property.

- [ ] **Step 2: Run the settings tests and verify failure**

Run:

```bash
pnpm --filter @markra/app test -- editor-preferences.test.ts useSettingsWindowState.test.ts
```

Expected: FAIL because the stored settings model still exposes AI fields and uses the old selection-toolbar preference name.

- [ ] **Step 3: Simplify the persisted settings model**

Remove all imports, constants, types, defaults, normalize functions, read/write calls, exports, and session store functions associated with:

```text
acpAgentSettings
aiAgentPreferences
aiProviders
aiQuickActionPrompts
aiWorkspaceAnimationEnabled
closeAiCommandOnAgentPanelOpen
showAiQuickInputOnSelection
showAiSelectionToolbarOnSelection
suggestAiPanelForComplexInlinePrompts
webSearch
ai-agent-sessions
```

Do not add a replacement selection-toolbar preference. Remove the old field from the existing `EditorPreferences` type, default object, normalizer, settings events, fixtures, tests, and diagnostics. Do not add legacy aliases. Do not load, write, export, or import removed keys.

- [ ] **Step 4: Remove settings UI and state**

Delete the provider and AI settings files listed above. Remove the `ai`, `providers`, and `web` categories from `SettingsCategory` and `SettingsShell`. Remove all matching state, effects, handlers, and return fields from `useSettingsWindowState` and all matching sections from `SettingsWindow`/`SettingsSections`.

In `EditorSettings.tsx`, remove AI switches and the `aiEnabled` filtering branch. Render and reorder only retained titlebar actions.

In diagnostics, remove AI provider, AI selection, workspace animation, Agent, and web-search lines. Keep network proxy, sync, and editor diagnostics.

- [ ] **Step 5: Run settings and diagnostics tests**

Run:

```bash
pnpm --filter @markra/app test -- SettingsShell.test.tsx EditorSettings.test.tsx useEditorPreferences.test.tsx useSettingsWindowState.test.ts diagnostics-report.test.ts settings-events.test.ts editor-preferences.test.ts
```

Expected: selected tests PASS; no settings test constructs an AI settings fixture.

- [ ] **Step 6: Commit settings removal**

```bash
git add packages/app/src
git commit -m "refactor: remove AI settings and stored state"
```

---

### Task 4: Remove AI Menus, Shortcuts, Titlebar Behavior, and Current Localization

**Files:**
- Modify: `packages/shared/src/keyboard-shortcuts.ts`
- Modify: `packages/shared/src/i18n/locales/en.ts`
- Modify: `packages/shared/src/i18n/locales/zh-CN.ts`
- Modify: `packages/shared/src/i18n/locales/zh-TW.ts`
- Modify: `packages/shared/src/i18n/locales/de.ts`
- Modify: `packages/shared/src/i18n/locales/es.ts`
- Modify: `packages/shared/src/i18n/locales/fr.ts`
- Modify: `packages/shared/src/i18n/locales/it.ts`
- Modify: `packages/shared/src/i18n/locales/ja.ts`
- Modify: `packages/shared/src/i18n/locales/ko.ts`
- Modify: `packages/shared/src/i18n/locales/pt-BR.ts`
- Modify: `packages/shared/src/i18n/locales/ru.ts`
- Modify: `packages/app/src/components/NativeTitleBar.tsx`
- Modify: `packages/app/src/components/NativeTitleBar.test.tsx`
- Modify: `packages/app/src/components/WindowsNativeTitleBar.tsx`
- Modify: `packages/app/src/components/WindowsWindowControls.test.tsx`
- Modify: `packages/app/src/components/SortableTitlebarAction.tsx`
- Modify: `packages/app/src/components/settings/KeyboardShortcutsSettings.tsx`
- Modify: `packages/app/src/components/settings/KeyboardShortcutsSettings.test.tsx`
- Modify: `packages/app/src/hooks/useNativeBindings.ts`
- Modify: `packages/app/src/hooks/useNativeBindings.test.tsx`
- Modify: `packages/app/src/lib/settings/app-settings.ts`
- Modify: `packages/app/src/runtime/context-menu-items.ts`
- Modify: `packages/app/src/runtime/context-menu-items.test.ts`
- Modify: `apps/desktop/src/runtime/tauri/menu.ts`
- Modify: `apps/desktop/src/runtime/tauri/menu.test.ts`
- Modify: `apps/desktop/src-tauri/src/menu.rs`
- Modify: `apps/desktop/src-tauri/src/menu_labels/de.rs`
- Modify: `apps/desktop/src-tauri/src/menu_labels/en.rs`
- Modify: `apps/desktop/src-tauri/src/menu_labels/es.rs`
- Modify: `apps/desktop/src-tauri/src/menu_labels/fr.rs`
- Modify: `apps/desktop/src-tauri/src/menu_labels/it.rs`
- Modify: `apps/desktop/src-tauri/src/menu_labels/ja.rs`
- Modify: `apps/desktop/src-tauri/src/menu_labels/ko.rs`
- Modify: `apps/desktop/src-tauri/src/menu_labels/pt_br.rs`
- Modify: `apps/desktop/src-tauri/src/menu_labels/ru.rs`
- Modify: `apps/desktop/src-tauri/src/menu_labels/zh_cn.rs`
- Modify: `apps/desktop/src-tauri/src/menu_labels/zh_tw.rs`

**Interfaces:**
- Produces: shortcut bindings with no `toggleAiAgent`, `toggleAiCommand`, `aiPolish`, `aiRewrite`, `aiContinueWriting`, `aiSummarize`, or `aiTranslate` fields.
- Produces: titlebar actions `viewMode | sourceMode | history | save | theme` only.
- Produces: native and simulated menus with no AI command identifiers.

- [ ] **Step 1: Update full shortcut fixtures first**

Remove the seven AI bindings from `MarkdownShortcutBindings`, `defaultMarkdownShortcuts`, shortcut normalization, conflict detection, settings rendering, native bindings, and every full binding fixture. Update the expected default action set to:

```ts
export const defaultTitlebarActions: readonly TitlebarActionPreference[] = [
  { id: "viewMode", visible: true },
  { id: "sourceMode", visible: true },
  { id: "history", visible: true },
  { id: "save", visible: true },
  { id: "theme", visible: true }
];
```

- [ ] **Step 2: Run shortcut and titlebar tests to expose remaining AI contracts**

Run:

```bash
pnpm --filter @markra/shared test -- keyboard
pnpm --filter @markra/app test -- KeyboardShortcutsSettings.test.tsx NativeTitleBar.test.tsx useNativeBindings.test.tsx context-menu-items.test.ts
```

Expected: FAIL wherever a component or fixture still requires an AI command or titlebar property.

- [ ] **Step 3: Simplify titlebars and menus**

Remove AI panel open/resizing/width props, AI toggle callbacks, translated positioning, and reserved columns from both titlebar components. Remove AI commands and the AI submenu from app context menus, Windows simulated menus, Tauri menu mappings, and Rust native menus.

Delete AI fields from every Rust `MenuLabels` struct and locale implementation. Keep all non-AI format, file, view, window, help, and sync-related menu items unchanged.

- [ ] **Step 4: Remove AI-only localization keys**

Delete matching keys from every locale for:

```text
settings.categories.ai
settings.categories.providers
settings.categories.web
settings.sections.ai*
settings.ai.*
settings.webSearch.*
settings.editor.*Ai*
app.ai*
app.toggleAiAgent
```

Update the network proxy description in every locale so it lists only retained network consumers: web images, project remote sync, and update checks. Preserve generic "provider" wording used by sync or other retained features.

- [ ] **Step 5: Run shared, app-menu, and Rust-menu tests**

Run:

```bash
pnpm --filter @markra/shared test
pnpm --filter @markra/app test -- KeyboardShortcutsSettings.test.tsx NativeTitleBar.test.tsx WindowsWindowControls.test.tsx useNativeBindings.test.tsx context-menu-items.test.ts
pnpm --filter @markra/desktop test -- menu.test.ts
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml menu
```

Expected: all selected suites PASS; complete shortcut fixtures contain only retained actions.

- [ ] **Step 6: Commit navigation and copy removal**

```bash
git add packages/shared/src packages/app/src apps/desktop/src/runtime/tauri/menu.ts apps/desktop/src/runtime/tauri/menu.test.ts apps/desktop/src-tauri/src/menu.rs apps/desktop/src-tauri/src/menu_labels
git commit -m "refactor: remove AI commands and navigation"
```

---

### Task 5: Remove Cross-Platform AI Runtimes, Spellcheck, and Generic Web Requests

**User-approved amendment (2026-07-18):** Delete spellcheck completely instead of preserving it. This Task now also removes the editor plugin/export/dependencies, app suggestion/settings/preferences/language/ignored-word surfaces, desktop/web runtime bridges, Rust dictionary commands/state, localization, shortcuts, styles, and spellcheck-only tests. Once those consumers are gone, remove `requestWebResource`/`request_web_resource`; retain only ordinary image download. Do not add absence-only tests.

**Files:**
- Delete: `packages/app/src/lib/acp-agent.ts`
- Delete: `packages/app/src/lib/acp-agent.test.ts`
- Delete: `packages/app/src/lib/tauri/native-ai.ts`
- Delete: `apps/desktop/src/runtime/tauri/acp.ts`
- Delete: `apps/desktop/src/runtime/tauri/acp.test.ts`
- Delete: `apps/desktop/src/runtime/tauri/ai-chat-attachments.ts`
- Delete: `apps/desktop/src/runtime/tauri/ai-chat-attachments.test.ts`
- Delete: `apps/desktop/src/runtime/tauri/native-ai.ts`
- Delete: `apps/desktop/src/runtime/tauri/native-ai.test.ts`
- Delete: `apps/web/src/runtime/web/ai.ts`
- Delete: `apps/web/src/runtime/web/ai.test.ts`
- Delete: `apps/web/src/runtime/web/ai-chat-attachments.ts`
- Delete: `apps/web/src/runtime/web/ai-chat-attachments.test.ts`
- Retain: `apps/web/src/runtime/web/database.ts` when the retained IndexedDB settings runtime still consumes it.
- Delete: `apps/desktop/src-tauri/src/acp.rs`
- Delete: `apps/desktop/src-tauri/src/ai_chat_attachments.rs`
- Delete: `apps/desktop/src-tauri/src/ai_http.rs`
- Modify: `packages/app/src/runtime/index.ts`
- Modify: `packages/app/src/runtime/index.test.ts`
- Modify: `packages/app/src/lib/tauri/index.ts`
- Modify: `apps/desktop/src/runtime/index.ts`
- Modify: `apps/desktop/src/runtime/index.test.ts`
- Modify: `apps/desktop/src/runtime/tauri/index.ts`
- Modify: `apps/desktop/src/runtime/tauri/invoke.ts`
- Modify: `apps/desktop/src/runtime/tauri/invoke.test.ts`
- Modify: `apps/desktop/src/runtime/tauri/dialog.ts`
- Modify: `apps/desktop/src/runtime/tauri/dialog.test.ts`
- Modify: `apps/web/src/runtime/index.ts`
- Modify: `apps/web/src/runtime/index.test.ts`
- Modify: `apps/web/src/runtime/web/index.ts`
- Modify: `apps/web/src/runtime/web/dialog.ts`
- Modify: `apps/web/src/runtime/web/dialog.test.ts`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Generated update: `apps/desktop/src-tauri/Cargo.lock`

**Interfaces:**
- Produces: `AppRuntime` without `acp`, `ai`, `aiChatAttachments`, or `spellcheck` properties and without the spellcheck feature flag.
- Preserves: `AppRuntime.webResource.downloadImage` and the Rust `download_web_image` command backed by `web_http.rs`.
- Preserves: project configuration and remote-sync commands.

- [ ] **Step 1: Record the retained runtime contract before removal**

Confirm the existing runtime tests exercise the retained capabilities that must survive:

```ts
expect(runtime).toHaveProperty("file");
expect(runtime).toHaveProperty("settings");
expect(runtime).toHaveProperty("webResource");
expect(runtime).toHaveProperty("projectConfig");
```

Do not add negative tests solely to prove deleted properties are absent. Task 8 static scans provide that evidence.

- [ ] **Step 2: Run the retained runtime baseline**

Run:

```bash
pnpm --filter @markra/app test -- runtime/index.test.ts
pnpm --filter @markra/desktop test -- runtime/index.test.ts
pnpm --filter @markra/web test -- runtime/index.test.ts
```

Expected: the retained baseline assertions PASS before the runtime interfaces are simplified.

- [ ] **Step 3: Remove TypeScript runtime interfaces and implementations**

Delete `AppAiRuntime`, `AppAcpRuntime`, AI attachment input/runtime types, AI dialog confirmations, native AI request types, unsupported AI stubs, exports, and all platform object properties. Delete the TypeScript files listed above. Also delete the amended spellcheck surface and remove the generic web-resource interface/implementation after its final dictionary-manifest consumer is gone.

Keep `webResource.downloadImage` and its desktop/web implementation. Preserve `apps/web/src/runtime/web/database.ts` when the retained settings runtime still imports it.

- [ ] **Step 4: Remove Rust AI commands and dependencies**

Delete the three Rust modules. Remove their `mod` declarations, managed state, event setup, and `generate_handler!` entries from `lib.rs`. Remove Cargo dependencies only when `rg` shows no retained Rust source import:

```bash
rg -n 'eventsource_stream|reqwest|tokio_util|futures_util|base64' apps/desktop/src-tauri/src
```

Retain `reqwest` and any shared dependency still used by `web_http.rs`, remote sync, updates, or image handling. Run `cargo check` to refresh `Cargo.lock` through Cargo rather than editing lock entries manually.

- [ ] **Step 5: Run runtime and Rust verification**

Run:

```bash
pnpm --filter @markra/app test -- runtime/index.test.ts
pnpm --filter @markra/desktop test
pnpm --filter @markra/web test
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
```

Expected: all suites PASS; `web_http.rs` and remote-sync tests remain present and passing.

- [ ] **Step 6: Commit runtime removal**

```bash
git add packages/app/src apps/desktop/src apps/web/src apps/desktop/src-tauri
git commit -m "refactor: remove AI runtimes and native commands"
```

---

### Task 6: Delete AI Packages and Prune Dependencies

The Task 5 amendment already removes `@cspell/cspell-types` and `cspell-trie-lib`; Task 6 must not reintroduce them while pruning the remaining AI packages.

**Files:**
- Delete: `packages/ai/**`
- Delete: `packages/providers/**`
- Delete: `packages/editor/src/ai-preview.ts`
- Modify: `packages/editor/src/index.ts`
- Modify: `packages/editor/package.json`
- Modify: `packages/app/package.json`
- Modify: `packages/scripts/src/vite/index.ts`
- Modify: `pnpm-lock.yaml`

**Interfaces:**
- Consumes: all application, settings, menu, and runtime imports removed by Tasks 1–5.
- Produces: a workspace graph with no AI/provider package or AI-only external dependency.

- [ ] **Step 1: Prove no active consumer remains**

Run:

```bash
rg -n '@markra/(ai|providers)|@earendil-works/pi|@ai-sdk/|from "ai"|from "@mozilla/readability"|from "turndown"' packages apps --glob '!packages/ai/**' --glob '!packages/providers/**' --glob '!**/node_modules/**'
```

Expected: matches only in package manifests and `packages/scripts/src/vite/index.ts`; no retained TypeScript/Rust application source imports the packages.

- [ ] **Step 2: Delete packages and editor preview**

Run:

```bash
git rm -r packages/ai packages/providers
git rm packages/editor/src/ai-preview.ts
```

Remove `export * from "./ai-preview.ts";` from `packages/editor/src/index.ts`.

- [ ] **Step 3: Remove package dependencies and stale vendor chunk rules**

Remove `@markra/ai`, `@markra/providers`, and `@earendil-works/pi-agent-core` from `packages/app/package.json`. Remove `@markra/ai` from `packages/editor/package.json`.

In `packages/scripts/src/vite/index.ts`, remove vendor chunk classification for AI SDK, agent, Readability, and provider packages while preserving React, Milkdown, CodeMirror, Mermaid, KaTeX, and other retained chunk rules.

Retain `react-markdown`, `remark-breaks`, `remark-gfm`, `remark-math`, and `@noble/hashes` because current non-AI export, preview, template, Markdown, math, and image-upload code imports them.

- [ ] **Step 4: Regenerate the pnpm lockfile**

Run:

```bash
pnpm install --lockfile-only
```

Expected: command exits 0 and removes the AI workspace importers and unreferenced AI SDK/provider/agent packages from `pnpm-lock.yaml`.

- [ ] **Step 5: Verify the pruned workspace**

Run:

```bash
pnpm build
rg -n '@markra/(ai|providers)|@ai-sdk/|@earendil-works/pi|mozilla/readability|turndown@' package.json packages/*/package.json pnpm-lock.yaml
```

Expected: build PASS; scan returns no matches.

- [ ] **Step 6: Commit package deletion**

```bash
git add packages pnpm-lock.yaml
git commit -m "refactor: delete AI packages and dependencies"
```

---

### Task 7: Rewrite Current Product and Engineering Documentation

**Files:**
- Modify: `packages/app/src/constants/initial-markdown.ts`
- Modify: `README.md`
- Modify: `README.zh-CN.md`
- Modify: `PRODUCT.md`
- Modify: `DESIGN.md`
- Modify: `CONTRIBUTING.md`
- Modify: `AGENTS.md`
- Modify: `docs/privacy.md`
- Preserve unchanged: `CHANGELOG.md`
- Preserve unchanged: `docs/superpowers/**` except this implementation plan's checkbox/status updates

**Interfaces:**
- Produces: current product copy centered on simple, practical note recording.
- Produces: repository guidelines whose package-boundary and testing descriptions match the remaining workspace.

- [ ] **Step 1: Rewrite the welcome document**

Replace AI positioning with concise note-taking guidance. The opening must communicate:

```md
# Welcome to QingYu

QingYu is a quiet, local-first Markdown editor for simple, practical note recording.

- Write and format notes without leaving the document
- Organize Markdown files and folders
- Keep notes local, back them up, or sync a note folder you control
```

Keep useful retained editor examples only.

- [ ] **Step 2: Update current product documentation**

Remove AI, model provider, agent, AI web-search, and second-brain claims from both READMEs, `PRODUCT.md`, `DESIGN.md`, and `docs/privacy.md`. Describe only implemented retained capabilities. In `CONTRIBUTING.md` and `AGENTS.md`, remove `packages/ai`/`packages/providers` boundaries and AI/provider-flow test guidance.

- [ ] **Step 3: Scan current documentation**

Run:

```bash
rg -n -i '\bAI\b|artificial intelligence|second brain|@markra/ai|@markra/providers|ACP agent|web search|model provider' README.md README.zh-CN.md PRODUCT.md DESIGN.md CONTRIBUTING.md AGENTS.md docs/privacy.md packages/app/src/constants/initial-markdown.ts
```

Expected: no matches.

- [ ] **Step 4: Run copy verification**

Run:

```bash
pnpm brand:verify
```

Expected: brand verification PASS. No unit test is required for the text-only welcome-document change.

- [ ] **Step 5: Commit current documentation**

```bash
git add packages/app/src/constants/initial-markdown.ts README.md README.zh-CN.md PRODUCT.md DESIGN.md CONTRIBUTING.md AGENTS.md docs/privacy.md
git commit -m "docs: focus QingYu on practical note taking"
```

---

### Task 8: Complete Static, Automated, and Live Acceptance

**Files:**
- Modify only if verification exposes a direct AI-removal defect: files already listed in Tasks 1–7
- Do not modify: `bg.png`

**Interfaces:**
- Consumes: the AI-free workspace from Tasks 1–7.
- Produces: verification evidence that the desktop product launches and retained note-taking behavior remains available.

- [ ] **Step 1: Run focused active-source scans**

Run:

```bash
test ! -e packages/ai
test ! -e packages/providers
rg -n '@markra/(ai|providers)|@ai-sdk/|@earendil-works/pi|AiAgent|AiCommand|AiProvider|AI_EDITOR_PREVIEW|toggleAi|\bai[A-Z]|ACP agent|settings\.ai\.|settings\.webSearch\.' packages apps package.json pnpm-lock.yaml --glob '!**/node_modules/**'
rg -n 'Spellchecker|markraSpellcheck|spellcheckEnabled|spellcheckIgnoredWords|spellcheckLanguage|openSpellcheckSuggestions|spellcheck_dictionary|requestWebResource|request_web_resource|cspell-trie-lib|@cspell/' packages apps package.json pnpm-lock.yaml --glob '!**/node_modules/**'
```

Expected: both `test` commands exit 0 and both `rg` commands return no active-source matches. Review generic lowercase `provider` terms separately and keep only non-AI sync/runtime uses. Browser/editor attributes that explicitly set native `spellcheck` to `false` are retained controls, not a spellcheck product surface.

- [ ] **Step 2: Run the complete workspace verification gate**

Run in this order:

```bash
pnpm test
pnpm typecheck:test
pnpm build
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
```

Expected: every command exits 0. Record failing suite names and fix only direct removal regressions before rerunning the same command.

- [ ] **Step 3: Verify repository hygiene**

Run:

```bash
git status --short
git diff --check
git ls-files bg.png
```

Expected: `git diff --check` exits 0; `git ls-files bg.png` prints nothing; `bg.png` remains untracked and untouched.

- [ ] **Step 4: Start the desktop application**

Run:

```bash
pnpm tauri dev
```

Expected: Vite serves `http://127.0.0.1:1420`, the `target/debug/markra` process launches, and the QingYu desktop window appears.

- [ ] **Step 5: Perform live UI acceptance**

In the running application:

1. Open or create a Markdown note and type text.
2. Select text and verify the formatting toolbar offers formatting, link, and copy actions only.
3. Open Settings and verify AI, Providers, Web Search, and Spellcheck categories are absent.
4. Inspect the titlebar and native menus and verify no AI action or command is present.
5. Verify Sync, Network, Backup, Export, Editor, and Appearance settings remain available; the editor shows no custom misspelling decorations or spelling-suggestion command.
6. Close the application cleanly.

Expected: every retained interaction is visible and functional, with no AI entry point or reserved AI-panel layout.

- [ ] **Step 6: Commit verification-only fixes if any**

If Steps 1–5 required direct fixes, stage only those fixes and commit:

```bash
git add packages apps README.md README.zh-CN.md PRODUCT.md DESIGN.md CONTRIBUTING.md AGENTS.md docs/privacy.md pnpm-lock.yaml
git commit -m "fix: complete AI removal verification"
```

If no files changed during verification, do not create an empty commit.

---

## Completion Criteria

- All eight tasks are checked off.
- The active source and dependency scans return no AI or custom spellcheck implementation matches.
- `pnpm test`, `pnpm typecheck:test`, `pnpm build`, and Rust tests pass.
- The desktop application launches and passes the live UI checklist.
- `bg.png` remains untracked and no remote push has occurred.
