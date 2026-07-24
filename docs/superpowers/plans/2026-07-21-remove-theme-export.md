# Remove Theme Export Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Delete theme export from every current QingYu product and native API surface without changing theme import or installation behavior.

**Architecture:** Narrow the shared theme runtime contract first, then remove its React and platform adapters. Remove the Tauri command and export-only ZIP-writing implementation while keeping archive reading/extraction intact. Update current author documentation to describe external packaging.

**Tech Stack:** React, TypeScript, Vitest, Tauri v2, Rust, Cargo

## Global Constraints

- Preserve `.css` and `.theme` import, replacement, discovery, activation, deletion, and validation.
- Preserve the user's existing `README.md`, `README.zh-CN.md`, and `macos-icon.icns` workspace changes.
- Do not change historical specifications, plans, changelogs, or Git history.
- Do not add a replacement theme packer or dependency.

---

### Task 1: Lock the removed public contract with failing tests

**Files:**
- Modify: `packages/app/src/components/settings/AppearanceSettings.test.tsx`
- Modify: `apps/desktop/src/runtime/index.test.ts`
- Modify: `apps/desktop/src/runtime/mobile.test.ts`
- Modify: `apps/web/src/runtime/index.test.ts`
- Modify: `apps/desktop/src/runtime/tauri/themes.test.ts`
- Modify: `apps/desktop/src-tauri/src/builder_boundary_tests.rs`

**Interfaces:**
- Consumes: current `ThemeRuntimeCapabilities`, `AppThemeRuntime`, and registered Tauri commands.
- Produces: tests requiring no export button, `canExport`, `exportCurrent`, `exportNativeTheme`, or `export_theme_file` command.

- [ ] **Step 1: Change component and runtime expectations**

Assert that desktop retains Import theme, Refresh themes, and Open theme folder while `queryByRole("button", { name: /Export current/u })` returns `null`. Remove `canExport` from expected capability objects and assert the runtime has no `exportCurrent` member.

- [ ] **Step 2: Change native boundary expectations**

Remove `export_theme_file` from `DESKTOP_COMMANDS` and assert the Tauri theme adapter module has no `exportNativeTheme` export.

- [ ] **Step 3: Verify RED**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/components/settings/AppearanceSettings.test.tsx
pnpm --filter @markra/desktop exec vitest run src/runtime/index.test.ts src/runtime/mobile.test.ts src/runtime/tauri/themes.test.ts
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml builder_boundary_tests
```

Expected: failures show that the export action, capability, adapter, and native command still exist.

### Task 2: Remove the TypeScript and React export surface

**Files:**
- Modify: `packages/app/src/lib/themes/theme-catalog.ts`
- Modify: `packages/app/src/runtime/index.ts`
- Modify: `packages/app/src/hooks/useThemeCatalog.ts`
- Modify: `packages/app/src/components/settings/ThemeSettingsControls.tsx`
- Modify: `packages/app/src/components/settings/AppearanceSettings.tsx`
- Modify: `packages/app/src/components/compact/CompactSettingsDetail.tsx`
- Modify: `packages/app/src/test/app-harness.tsx`
- Modify: `apps/desktop/src/runtime/desktop.ts`
- Modify: `apps/desktop/src/runtime/mobile.ts`
- Modify: `apps/desktop/src/runtime/tauri/themes.ts`
- Modify: `packages/shared/src/i18n/locales/types.ts`
- Modify: `packages/shared/src/i18n/locales/*.ts`

**Interfaces:**
- Consumes: the failing tests from Task 1.
- Produces: a theme runtime with import, replace, list, activation, delete, refresh, and open-directory operations only.

- [ ] **Step 1: Remove shared contract members**

Delete `ThemeRuntimeCapabilities.canExport` and `AppThemeRuntime.exportCurrent`, then remove those members from all runtime factories and test fixtures.

- [ ] **Step 2: Remove React behavior**

Delete the catalog hook's `exportTheme` callback, the toolbar's `currentTheme` and `onExport` props, the Download icon/button, and desktop/compact callers.

- [ ] **Step 3: Remove desktop adapter and locale key**

Delete `exportNativeTheme`, its save-dialog import/mock coverage, and `settings.theme.exportCurrent` from every locale and the locale key union.

- [ ] **Step 4: Verify GREEN**

Run the three Vitest commands from Task 1. Expected: all selected tests pass.

### Task 3: Remove the Rust command and archive writer

**Files:**
- Modify: `apps/desktop/src-tauri/src/desktop_runtime.rs`
- Modify: `apps/desktop/src-tauri/src/builder_boundary_tests.rs`
- Modify: `apps/desktop/src-tauri/src/themes/mod.rs`
- Modify: `apps/desktop/src-tauri/src/themes/catalog.rs`
- Modify: `apps/desktop/src-tauri/src/themes/archive.rs`

**Interfaces:**
- Consumes: `.theme` archives only through `prepare_external_theme`.
- Produces: no product code capable of serializing or publishing theme archives.

- [ ] **Step 1: Delete the registered command**

Remove `export_theme_file` from the Tauri command list and delete its command function.

- [ ] **Step 2: Delete catalog export**

Remove `ThemeCatalog::export`, export-specific imports, and tests whose only behavior is export generation or export publication.

- [ ] **Step 3: Delete archive writing**

Remove `ThemePackageExport`, `write_theme_archive`, generated default manifests, ZIP writing, output target validation, atomic output publication, and their helpers. Keep ZIP construction inside tests only where it creates import fixtures.

- [ ] **Step 4: Repair import fixtures**

Replace production export calls used by import tests with test-only `ZipWriter` helpers that package `manifest.json`, `theme.css`, assets, and licenses directly.

- [ ] **Step 5: Verify GREEN**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml themes::
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml builder_boundary_tests
```

Expected: all theme and command-boundary tests pass with no export command or writer.

### Task 4: Update current documentation and run repository gates

**Files:**
- Modify: `docs/theme-authoring.md`

**Interfaces:**
- Consumes: the import-only product behavior from Tasks 2 and 3.
- Produces: current author instructions that do not promise application export.

- [ ] **Step 1: Rewrite the author workflow**

Describe unpacked directories as the development/install format and `.theme` as an externally-created portable ZIP. Remove Export current steps and mobile export comparisons.

- [ ] **Step 2: Scan for current-code residue**

Run:

```bash
rg -n "export_theme_file|exportNativeTheme|exportCurrent|canExport|exportTheme|settings\.theme\.exportCurrent" apps packages docs/theme-authoring.md
```

Expected: no matches.

- [ ] **Step 3: Run repository verification**

Run:

```bash
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
pnpm test
pnpm typecheck:test
pnpm build
```

Expected: every command exits successfully.

- [ ] **Step 4: Commit and integrate**

Stage only the removal, documentation, and test files; commit with `feat(theme): remove theme export`; fast-forward local `main`; rerun the smallest merged-tree verification; remove the temporary worktree and branch. Do not push.
