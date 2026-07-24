# Third-Party Theme Catalog Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace QingYu's closed palette list and inline custom CSS editor with a safe, file-backed catalog containing two protected defaults and user-manageable third-party CSS themes.

**Architecture:** Rust owns `app_data_dir/themes`, parses and validates every CSS file, seeds the 18 former built-ins once, and exposes semantic Tauri commands. The shared React layer merges native descriptors with two protected defaults, persists dynamic light/dark IDs, applies exactly one active stylesheet, and renders capability-gated desktop/mobile controls. Web uses the same controller with a default-only runtime.

**Tech Stack:** Rust 2021, `cssparser` 0.36.0, SHA-256, Tauri v2, React, TypeScript, Vitest, Testing Library, Tailwind CSS, pnpm.

## Global Constraints

- Keep exactly one protected default light theme (`light`) and one protected default dark theme (`dark`).
- Store all third-party files in `app_data_dir/themes`; seeded and imported themes use the same parser and may both be deleted.
- One UTF-8 `.css` file represents exactly one `light` or `dark` theme and is limited to 256 KiB.
- Theme IDs match `^[a-z0-9][a-z0-9-]{0,63}$`; reserve `light`, `dark`, and the `qingyu-` prefix.
- Reject invalid CSS, `@import`, HTTP/HTTPS/protocol-relative/`file:` URLs, relative resource URLs, symlinks, nested paths, and non-regular files.
- Allow `data:` URLs and same-document fragment URLs such as `url(#marker)`.
- Writes use a temporary file, flush, and atomic rename; replacements and reads require the expected fingerprint.
- Keep system/light/dark appearance modes and persist independent `lightThemeId` and `darkThemeId` values.
- The first activation of each third-party ID/fingerprint is a local guarded preview; persist and broadcast only after a native confirmation succeeds.
- Desktop supports import, export-current, refresh, open-folder, selection, and deletion. Mobile supports scan, selection, refresh, and deletion but not import/export/open-folder. Web supports protected defaults only.
- Scan at startup, when Appearance opens, after catalog mutations, and on explicit refresh; do not add a continuous watcher.
- Theme synchronization remains out of scope and existing S3/WebDAV project synchronization must not change.
- Use `pnpm`; do not add another JavaScript package manager or lockfile.
- Preserve unrelated changes in `README.md`, `README.zh-CN.md`, and `macos-icon.icns`.

---

## File Map

### Native catalog

- Create `apps/desktop/src-tauri/src/themes/mod.rs`: Tauri command surface and shared public data types.
- Create `apps/desktop/src-tauri/src/themes/parser.rs`: metadata parsing, CSS token validation, URL policy, and fingerprints.
- Create `apps/desktop/src-tauri/src/themes/catalog.rs`: directory ownership, scan, duplicate invalidation, atomic import/replace/delete/export, and seeding.
- Create `apps/desktop/src-tauri/src/themes/migration.rs`: one-time legacy settings/custom-CSS migration and commit marker.
- Create `apps/desktop/src-tauri/themes/default-light.css` and `default-dark.css`: canonical protected-default exports.
- Create `apps/desktop/src-tauri/themes/third-party/*.css`: 18 seed files converted from the current stylesheet.
- Modify `apps/desktop/src-tauri/src/lib.rs`, `desktop_runtime.rs`, and `mobile_runtime.rs`: register the module and platform-appropriate commands.
- Modify `apps/desktop/src-tauri/Cargo.toml` and `Cargo.lock`: make the already-locked `cssparser` 0.36.0 a direct dependency.

### Runtime and application state

- Create `packages/app/src/lib/themes/theme-catalog.ts`: shared descriptors, protected defaults, sorting, capabilities, and catalog helpers.
- Create `packages/app/src/hooks/useThemeCatalog.ts`: scan lifecycle, catalog event listening, actions, and diagnostics.
- Create `apps/desktop/src/runtime/tauri/themes.ts`: typed command adapters plus desktop pickers and native confirmation.
- Modify `packages/app/src/runtime/index.ts`, `apps/desktop/src/runtime/desktop.ts`, `apps/desktop/src/runtime/mobile.ts`, and `apps/web/src/runtime/index.ts`: expose `AppThemeRuntime` with explicit capabilities.
- Modify `packages/app/src/lib/settings/app-settings.ts`: dynamic IDs, approval fingerprints, version-2 portable settings, and legacy import compatibility.
- Modify `packages/app/src/lib/settings/settings-events.ts`: catalog-changed event and removal of custom-CSS events.
- Modify `packages/app/src/hooks/useAppTheme.ts`: async CSS activation, race protection, fallback, guarded preview, and catalog integration.
- Modify `packages/editor/src/mermaid.ts`: receive resolved appearance rather than infer dark mode from a closed ID list.

### User interface and assets

- Replace `packages/app/src/components/settings/ThemeSettingsControls.tsx`: toolbar, cards, independent grids, collapse behavior, diagnostics, and action confirmations.
- Modify `packages/app/src/components/settings/AppearanceSettings.tsx`, `packages/app/src/components/SettingsWindow.tsx`, compact settings components, and `packages/app/src/styles.css`.
- Modify all locale files under `packages/shared/src/i18n/locales/` and `types.ts` with the new theme-management copy.
- Create `docs/theme-authoring.md`: metadata contract, supported public tokens, selector stability, safety restrictions, and install/export workflow.

### Tests

- Native unit tests live beside `parser.rs`, `catalog.rs`, and `migration.rs`; runtime command-registration assertions remain in the runtime files.
- Create `apps/desktop/src/runtime/tauri/themes.test.ts`.
- Create `packages/app/src/lib/themes/theme-catalog.test.ts` and `packages/app/src/hooks/useThemeCatalog.test.tsx`.
- Update `packages/app/src/lib/settings/app-settings.test.ts`, `packages/app/src/App.test.tsx`, `packages/app/src/test/app-harness.tsx`, and `packages/app/src/components/settings/AppearanceSettings.test.tsx`.
- Update `apps/desktop/src/runtime/index.test.ts`, `apps/web/src/runtime/index.test.ts`, and Mermaid tests.

---

### Task 1: Shared catalog contract and default-only runtime

**Files:**
- Create: `packages/app/src/lib/themes/theme-catalog.ts`
- Create: `packages/app/src/lib/themes/theme-catalog.test.ts`
- Modify: `packages/app/src/runtime/index.ts`
- Modify: `apps/web/src/runtime/index.ts`
- Test: `apps/web/src/runtime/index.test.ts`

**Interfaces:**
- Produces: `ThemeAppearance`, `ThemePreview`, `ThemeDescriptor`, `InvalidThemeFile`, `ThemeCatalogSnapshot`, `ThemeCssPayload`, `ThemeImportResult`, `ThemeRuntimeCapabilities`, `AppThemeRuntime`, `protectedThemeDescriptors`, `mergeThemeCatalog()`.
- Consumes: `AppRuntime` and the existing default-runtime factory.

- [ ] **Step 1: Write failing catalog and web-runtime tests**

```ts
expect(mergeThemeCatalog({ themes: [], invalidFiles: [] }).themes.map(({ id }) => id))
  .toEqual(["light", "dark"]);
expect(createWebRuntime().themes.capabilities).toEqual({
  canDelete: false,
  canExport: false,
  canImport: false,
  canOpenDirectory: false
});
await expect(createWebRuntime().themes.list()).resolves.toMatchObject({ invalidFiles: [], themes: [] });
```

- [ ] **Step 2: Run tests and verify the missing contract fails**

Run: `pnpm exec vitest run packages/app/src/lib/themes/theme-catalog.test.ts apps/web/src/runtime/index.test.ts`

Expected: FAIL because `theme-catalog.ts` and `AppRuntime.themes` do not exist.

- [ ] **Step 3: Define exact shared types and deterministic merge behavior**

```ts
export type ThemeAppearance = "light" | "dark";
export type ThemeSource = "default" | "third-party";
export type ThemePreview = { accent: string; background: string; panel: string; text: string };
export type ThemeDescriptor = {
  appearance: ThemeAppearance; author?: string; fileName: string | null;
  fingerprint: string; id: string; name: string; preview: ThemePreview;
  source: ThemeSource; version?: string;
};
export type InvalidThemeFile = { fileName: string; reason: string };
export type ThemeCatalogSnapshot = { invalidFiles: InvalidThemeFile[]; themes: ThemeDescriptor[] };
export type ThemeCssPayload = { css: string; fingerprint: string; id: string };
export type ThemeImportResult =
  | { kind: "imported"; theme: ThemeDescriptor }
  | { candidate: ThemeDescriptor; existing: ThemeDescriptor; kind: "conflict"; sourcePath: string };
export const protectedThemeDescriptors: readonly ThemeDescriptor[] = [
  { appearance: "light", fileName: null, fingerprint: "default:light", id: "light", name: "Light", preview: { accent: "#1a1c1e", background: "#ffffff", panel: "#f6f8fa", text: "#1f2328" }, source: "default" },
  { appearance: "dark", fileName: null, fingerprint: "default:dark", id: "dark", name: "Dark", preview: { accent: "#f4f4f5", background: "#0d1117", panel: "#161b22", text: "#f0f6fc" }, source: "default" }
];
```

`mergeThemeCatalog()` must put each matching default first, sort third-party names with `localeCompare`, use ID as the tie-break, and keep light and dark lists independent.

- [ ] **Step 4: Add `AppThemeRuntime` and the default implementation**

```ts
export type AppThemeRuntime = {
  capabilities: ThemeRuntimeCapabilities;
  confirmActivation: (themeName: string) => Promise<boolean>;
  delete: (id: string, expectedFingerprint: string) => Promise<unknown>;
  exportCurrent: (id: string, expectedFingerprint: string) => Promise<boolean>;
  importFile: () => Promise<ThemeImportResult | null>;
  list: () => Promise<ThemeCatalogSnapshot>;
  openDirectory: () => Promise<unknown>;
  readCss: (id: string, expectedFingerprint: string) => Promise<ThemeCssPayload>;
  replaceFile: (sourcePath: string, expectedFingerprint: string) => Promise<ThemeDescriptor>;
};
```

The default/web runtime returns an empty third-party snapshot, rejects management methods with the existing `unsupportedFeature()` helper, and never injects CSS.

- [ ] **Step 5: Run focused tests and commit**

Run: `pnpm exec vitest run packages/app/src/lib/themes/theme-catalog.test.ts apps/web/src/runtime/index.test.ts`

Expected: PASS.

```sh
git add packages/app/src/lib/themes packages/app/src/runtime/index.ts apps/web/src/runtime/index.ts apps/web/src/runtime/index.test.ts
git commit -m "feat(theme): define dynamic catalog runtime"
```

### Task 2: Native metadata and CSS validator

**Files:**
- Create: `apps/desktop/src-tauri/src/themes/mod.rs`
- Create: `apps/desktop/src-tauri/src/themes/parser.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/Cargo.lock`

**Interfaces:**
- Produces: `parse_theme_file(bytes: &[u8], file_name: &str) -> Result<ParsedTheme, ThemeError>` and serializable descriptor/error types.
- Consumes: `cssparser = "0.36.0"`, `sha2`, and `serde`.

- [ ] **Step 1: Add parser tests for metadata, CSS syntax, and URL policy**

```rust
#[test]
fn parses_unicode_metadata_and_safe_urls() {
    let parsed = parse_theme_file(valid_css("主题", "url(data:image/svg+xml;base64,AA==) url(#marker)"), "nord.css").unwrap();
    assert_eq!(parsed.descriptor.name, "主题");
    assert_eq!(parsed.descriptor.appearance, ThemeAppearance::Dark);
}

#[test]
fn rejects_import_remote_relative_and_file_urls() {
    for body in ["@import 'x.css';", "a{background:url(https://x)}", "a{background:url(//x)}", "a{background:url(file:///x)}", "a{background:url(asset.png)}"] {
        assert!(parse_theme_file(valid_css("Bad", body), "bad.css").is_err());
    }
}
```

Also cover duplicate/missing metadata keys, invalid/non-transparent colors, `light`/`dark`/`qingyu-` IDs, invalid UTF-8, and `256 * 1024 + 1` bytes.

- [ ] **Step 2: Run the native parser tests and verify failure**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml themes::parser::tests`

Expected: FAIL because the `themes` module is absent.

- [ ] **Step 3: Implement metadata parsing and normalized descriptor construction**

```rust
pub(crate) const MAX_THEME_BYTES: usize = 256 * 1024;
pub(crate) fn valid_theme_id(id: &str) -> bool {
    !matches!(id, "light" | "dark")
        && !id.starts_with("qingyu-")
        && id.len() <= 64
        && id.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || (index > 0 && byte == b'-')
        })
}
```

Parse only the first comment block containing a line exactly equal to `@qingyu-theme`; reject repeated required keys and bound `name` to 120 characters, `author` to 120, `version` to 64, and unknown normalized keys to 64/256.

- [ ] **Step 4: Validate the whole stylesheet with `cssparser` tokens**

Use `ParserInput`/`Parser`, recursively enter block/function tokens, reject parse errors, reject an at-keyword equal to `import`, and classify every `url()`/unquoted-url token. Accept only `data:` and `#`; reject values beginning with `http:`, `https:`, `//`, `file:`, `/`, `./`, `../`, or any other relative value. Calculate lowercase SHA-256 hex from the original bytes.

- [ ] **Step 5: Run parser tests and commit**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml themes::parser::tests`

Expected: PASS.

```sh
git add apps/desktop/src-tauri/Cargo.toml apps/desktop/src-tauri/Cargo.lock apps/desktop/src-tauri/src/lib.rs apps/desktop/src-tauri/src/themes/mod.rs apps/desktop/src-tauri/src/themes/parser.rs
git commit -m "feat(theme): validate CSS theme files"
```

### Task 3: Seed assets and safe native catalog storage

**Files:**
- Create: `apps/desktop/src-tauri/src/themes/catalog.rs`
- Create: `apps/desktop/src-tauri/themes/default-light.css`
- Create: `apps/desktop/src-tauri/themes/default-dark.css`
- Create: `apps/desktop/src-tauri/themes/third-party/{github,github-dark,one-dark,one-light,one-dark-pro,gothic,newsprint,night,pixyll,whitey,sepia,solarized-light,solarized-dark,nord,catppuccin-latte,catppuccin-mocha,academic,minimal}.css`
- Modify: `apps/desktop/src-tauri/src/themes/mod.rs`
- Modify: `packages/app/src/styles.css`

**Interfaces:**
- Produces: `ThemeCatalog::at(root: PathBuf)`, `scan()`, `seed_missing()`, `read_css()`, `import_bytes()`, `replace_bytes()`, `delete()`, and `export_to()`.
- Consumes: `ParsedTheme`, expected fingerprints, and bundled `include_bytes!` assets.

- [ ] **Step 1: Write filesystem behavior tests**

```rust
#[test]
fn scan_invalidates_every_duplicate_id_but_keeps_other_files() { /* create a.css, b.css with one ID and c.css with another; assert only c is selectable */ }
#[test]
fn read_and_replace_fail_on_stale_fingerprint() { /* scan, alter source, assert ThemeErrorCode::FingerprintMismatch */ }
#[test]
fn scan_rejects_symlink_and_nested_entries() { /* Unix symlink plus subdirectory fixture; assert diagnostics */ }
#[test]
fn export_preserves_third_party_bytes() { /* import CRLF bytes, export, compare byte-for-byte */ }
```

The same module must test stable name/ID ordering, per-file isolation, protected deletion rejection, idempotent seed writes, and atomic replacement.

- [ ] **Step 2: Run catalog tests and verify failure**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml themes::catalog::tests`

Expected: FAIL because `ThemeCatalog` is not defined.

- [ ] **Step 3: Implement the owned-directory boundary**

```rust
pub(crate) struct ThemeCatalog { root: PathBuf }
impl ThemeCatalog {
    pub(crate) fn at(root: PathBuf) -> Self { Self { root } }
    fn safe_theme_path(&self, file_name: &str) -> Result<PathBuf, ThemeError> {
        let path = Path::new(file_name);
        if path.components().count() != 1 || path.extension().and_then(OsStr::to_str) != Some("css") {
            return Err(ThemeError::unsafe_path(file_name));
        }
        Ok(self.root.join(path))
    }
}
```

Use `symlink_metadata` for the owned directory and every entry, reject a symlinked catalog root, require `file_type().is_file()`, never canonicalize through a symlink, collect invalid-file diagnostics instead of aborting a scan, group parsed files by ID, and invalidate all members of groups larger than one.

- [ ] **Step 4: Implement atomic mutations and exact export**

Create the temp file in `themes/` with `create_new(true)`, write all bytes, call `sync_all()`, reparse the temp bytes, and rename only after validation. Replacement checks the current stored fingerprint immediately before rename. Export opens the selected save path with `create_new`/truncate semantics after the native save picker and writes either exact stored bytes or bundled canonical default bytes.

- [ ] **Step 5: Convert existing CSS into 18 self-contained seed files**

Each file starts with the approved metadata block, contains its root variables and theme-specific Markdown/editor rules from `styles.css`, and changes static selectors from `[data-theme="id"]` wrappers to active-file selectors (`:root`, `.markdown-paper`, or `.markdown-source-paper`). Remove those non-default rules from `styles.css`; retain base tokens plus protected light/dark rules.

- [ ] **Step 6: Run native tests plus frontend build and commit**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml themes::catalog::tests`

Run: `pnpm build`

Expected: both PASS.

```sh
git add apps/desktop/src-tauri/src/themes apps/desktop/src-tauri/themes packages/app/src/styles.css
git commit -m "feat(theme): add file-backed theme catalog"
```

### Task 4: One-time migration and dynamic settings IDs

**Files:**
- Create: `apps/desktop/src-tauri/src/themes/migration.rs`
- Modify: `apps/desktop/src-tauri/src/themes/mod.rs`
- Modify: `apps/desktop/src-tauri/src/app_settings.rs`
- Modify: `packages/app/src/lib/settings/app-settings.ts`
- Test: `packages/app/src/lib/settings/app-settings.test.ts`

**Interfaces:**
- Produces: `initialize_catalog(app) -> Result<ThemeCatalogSnapshot, ThemeError>`, `AppThemePreferences { appearanceMode, lightThemeId, darkThemeId }`, and approved-fingerprint storage helpers.
- Consumes: legacy `theme`, `lightTheme`, `darkTheme`, `customThemeCss`, `lightCustomThemeCss`, and `darkCustomThemeCss` keys.

- [ ] **Step 1: Write failing Rust and TypeScript migration tests**

```rust
#[test]
fn migration_commits_only_after_seed_and_settings_succeed() { /* fail one backend write; assert themeCatalogVersion is absent */ }
#[test]
fn migration_rewrites_scoped_custom_selectors() { /* custom -> migrated-custom-light in both root and paper selectors */ }
#[test]
fn deleted_seed_is_not_restored_after_version_one_commit() { /* seed, delete, rerun, assert absent */ }
```

```ts
expect(normalizeAppThemePreferences({ appearanceMode: "light", lightThemeId: "my-theme", darkThemeId: "night" }))
  .toEqual({ appearanceMode: "light", lightThemeId: "my-theme", darkThemeId: "night" });
expect(normalizeAppThemePreferences({ lightThemeId: "../bad", darkThemeId: "dark" }).lightThemeId).toBe("light");
```

- [ ] **Step 2: Run focused tests and verify failure**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml themes::migration::tests`

Run: `pnpm exec vitest run packages/app/src/lib/settings/app-settings.test.ts`

Expected: FAIL on missing version marker and dynamic ID properties.

- [ ] **Step 3: Implement the native migration transaction**

Use `themeCatalogVersion = 1` as the commit marker. Seed non-conflicting files idempotently, create unique `migrated-custom-light[-N].css` and `migrated-custom-dark[-N].css` files when legacy selections equal `custom`, rewrite only `[data-theme="custom"]` and `[data-editor-theme="custom"]`, update the ID keys, save the store, then set and save the marker. On error leave the marker absent so startup retries.

- [ ] **Step 4: Replace closed TypeScript/Rust allowlists with syntactic IDs**

```ts
export type ThemeId = string;
export type AppThemePreferences = {
  appearanceMode: AppAppearanceMode;
  darkThemeId: ThemeId;
  lightThemeId: ThemeId;
};
export const defaultAppThemePreferences = { appearanceMode: "system", darkThemeId: "dark", lightThemeId: "light" } satisfies AppThemePreferences;
```

Rust appearance group and exposed MCP keys become `appearance.lightThemeId` and `appearance.darkThemeId`; values use the same ID grammar. Preserve legacy store-key migration, while version-2 portable files contain only appearance mode and IDs, never CSS content.

- [ ] **Step 5: Add local approval fingerprint helpers**

Store `approvedThemeFingerprints` as a bounded `Record<string, string>` under `settings.json`; expose `getApprovedThemeFingerprint(id)`, `approveThemeFingerprint(id, fingerprint)`, and `forgetApprovedThemeFingerprint(id)` without adding these internal values to portable settings or MCP settings.

- [ ] **Step 6: Run tests and commit**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml themes::migration::tests app_settings::tests`

Run: `pnpm exec vitest run packages/app/src/lib/settings/app-settings.test.ts`

Expected: PASS.

```sh
git add apps/desktop/src-tauri/src/themes apps/desktop/src-tauri/src/app_settings.rs packages/app/src/lib/settings/app-settings.ts packages/app/src/lib/settings/app-settings.test.ts
git commit -m "feat(theme): migrate theme settings to catalog IDs"
```

### Task 5: Tauri commands and desktop/mobile runtime adapters

**Files:**
- Modify: `apps/desktop/src-tauri/src/themes/mod.rs`
- Modify: `apps/desktop/src-tauri/src/desktop_runtime.rs`
- Modify: `apps/desktop/src-tauri/src/mobile_runtime.rs`
- Create: `apps/desktop/src/runtime/tauri/themes.ts`
- Create: `apps/desktop/src/runtime/tauri/themes.test.ts`
- Modify: `apps/desktop/src/runtime/desktop.ts`
- Modify: `apps/desktop/src/runtime/mobile.ts`
- Modify: `apps/desktop/src/runtime/index.test.ts`

**Interfaces:**
- Produces commands `list_themes`, `read_theme_css`, `import_theme_file`, `replace_theme_file`, `export_theme_file`, `delete_theme`, and `theme_directory_path`.
- Consumes the `AppThemeRuntime` contract from Task 1 and `ThemeCatalog` from Task 3.

- [ ] **Step 1: Write failing adapter and registration tests**

```ts
expect(desktopRuntime.themes.capabilities).toEqual({ canDelete: true, canExport: true, canImport: true, canOpenDirectory: true });
expect(mobileRuntime.themes.capabilities).toEqual({ canDelete: true, canExport: false, canImport: false, canOpenDirectory: false });
await desktopRuntime.themes.readCss("nord", "abc");
expect(invoke).toHaveBeenCalledWith("read_theme_css", { id: "nord", expectedFingerprint: "abc" });
```

Rust source-boundary tests assert all seven commands are registered on desktop and only `list_themes`, `read_theme_css`, and `delete_theme` are registered on mobile. Register typed app-settings commands on mobile as part of this task so migration and ID persistence share one schema.

- [ ] **Step 2: Run focused tests and verify failure**

Run: `pnpm exec vitest run apps/desktop/src/runtime/tauri/themes.test.ts apps/desktop/src/runtime/index.test.ts`

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml desktop_runtime::tests mobile_runtime`

Expected: FAIL on absent command adapters/registrations.

- [ ] **Step 3: Implement semantic Tauri commands**

Every command resolves `app.path().app_data_dir()?.join("themes")` internally. Commands accept only IDs/fingerprints plus desktop picker-returned source/target paths; they map `ThemeError` to `{ code, message, fileName? }` without returning invalid CSS contents.

- [ ] **Step 4: Implement platform adapters and guarded system dialog**

Desktop `importFile()` uses `open({ filters: [{ name: "CSS", extensions: ["css"] }], multiple: false })`; `exportCurrent()` uses `save()` and passes the resulting path to native export. `openDirectory()` asks native for the owned path and calls `openPath`. `confirmActivation()` uses `ask()` from `@tauri-apps/plugin-dialog` and returns false on cancel, close, or rejection. Mobile maps list/read/delete and confirmation, while management methods remain unsupported and capability-gated.

- [ ] **Step 5: Run tests and commit**

Run: `pnpm exec vitest run apps/desktop/src/runtime/tauri/themes.test.ts apps/desktop/src/runtime/index.test.ts`

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml desktop_runtime::tests mobile_runtime`

Expected: PASS.

```sh
git add apps/desktop/src-tauri/src/themes/mod.rs apps/desktop/src-tauri/src/desktop_runtime.rs apps/desktop/src-tauri/src/mobile_runtime.rs apps/desktop/src/runtime
git commit -m "feat(theme): expose native catalog runtime"
```

### Task 6: Catalog hook and cross-window refresh

**Files:**
- Create: `packages/app/src/hooks/useThemeCatalog.ts`
- Create: `packages/app/src/hooks/useThemeCatalog.test.tsx`
- Modify: `packages/app/src/lib/settings/settings-events.ts`
- Modify: `packages/app/src/test/app-harness.tsx`

**Interfaces:**
- Produces: `useThemeCatalog()` with `snapshot`, `lightThemes`, `darkThemes`, `loading`, `error`, `refresh`, `importTheme`, `replaceTheme`, `deleteTheme`, `exportTheme`, and `openDirectory`.
- Consumes: `getAppRuntime().themes` and event `markra://theme-catalog-changed` payload `{ revision: string }`.

- [ ] **Step 1: Write lifecycle and event tests**

```tsx
renderHook(() => useThemeCatalog());
await waitFor(() => expect(runtime.themes.list).toHaveBeenCalledTimes(1));
act(() => emitThemeCatalogChanged({ revision: "2" }));
await waitFor(() => expect(runtime.themes.list).toHaveBeenCalledTimes(2));
```

Also assert invalid-file diagnostics survive a partial scan, mutation success emits the event and refreshes, mutation failure preserves the previous snapshot, and unmount ignores late list results.

- [ ] **Step 2: Run hook tests and verify failure**

Run: `pnpm exec vitest run packages/app/src/hooks/useThemeCatalog.test.tsx`

Expected: FAIL because the hook and event do not exist.

- [ ] **Step 3: Implement the catalog controller**

Use an incrementing request token and mounted ref. `refresh()` merges native third-party descriptors with protected defaults. Successful import/replace/delete emits `markra://theme-catalog-changed` with `crypto.randomUUID()` when available and then rescans. Opening Appearance calls `refresh()` explicitly; startup scan comes from the hook mount.

- [ ] **Step 4: Run tests and commit**

Run: `pnpm exec vitest run packages/app/src/hooks/useThemeCatalog.test.tsx`

Expected: PASS.

```sh
git add packages/app/src/hooks/useThemeCatalog.ts packages/app/src/hooks/useThemeCatalog.test.tsx packages/app/src/lib/settings/settings-events.ts packages/app/src/test/app-harness.tsx
git commit -m "feat(theme): manage catalog lifecycle"
```

### Task 7: Async theme activation, guarded preview, and fallback

**Files:**
- Modify: `packages/app/src/hooks/useAppTheme.ts`
- Modify: `packages/app/src/App.test.tsx`
- Modify: `packages/app/src/test/app-harness.tsx`
- Modify: `packages/editor/src/mermaid.ts`
- Modify: corresponding Mermaid test file located by `rg --files packages/editor | rg 'mermaid.*test'`

**Interfaces:**
- Produces: `selectLightTheme(id: string)`, `selectDarkTheme(id: string)`, `activeTheme`, `catalog`, and `ready` after CSS load/fallback.
- Consumes: `useThemeCatalog()`, fingerprint approvals, `themes.readCss()`, and native confirmation.

- [ ] **Step 1: Write failing activation tests**

```ts
it("keeps the matching default until the selected CSS arrives and ignores a late prior read", async () => { /* resolve second read first; assert only second CSS is injected */ });
it("reverts an unapproved fingerprint when native confirmation is canceled", async () => { /* assert selection/store/event remain previous */ });
it("approves once but prompts again after replacement changes the fingerprint", async () => { /* assert two confirmations across fingerprints */ });
it("repairs a missing or unreadable ID to the matching protected default", async () => { /* assert persisted light or dark fallback */ });
```

- [ ] **Step 2: Run application tests and verify failure**

Run: `pnpm exec vitest run packages/app/src/App.test.tsx -t "theme catalog|fingerprint|late theme|missing theme"`

Expected: FAIL against the synchronous closed-list controller.

- [ ] **Step 3: Replace custom-CSS state with one owned catalog style element**

Use element ID `markra-third-party-theme-style`. Before an async read, set `data-theme` and `data-editor-theme` to the matching protected default and remove catalog CSS. After `readCss(id, fingerprint)` returns, verify the request token and returned identity, set `textContent`, then set root `data-theme=id`, `data-theme-appearance=appearance`, and `style.colorScheme=appearance`.

- [ ] **Step 4: Implement preview-before-persist selection**

For protected defaults or approved fingerprints, update local state, persist, and notify immediately. For an unapproved fingerprint, retain previous preferences, apply the candidate locally, call `confirmActivation(name)`, and on success save approval then persist/broadcast; on false or rejection increment the token and reapply the previous selection.

- [ ] **Step 5: Drive editor/Mermaid from metadata appearance**

Change Mermaid rendering input from closed theme IDs to `ThemeAppearance`; remove the static dark-ID set. Pass the resolved metadata appearance to Markdown paper attributes and Mermaid configuration.

- [ ] **Step 6: Run focused tests and commit**

Run: `pnpm exec vitest run packages/app/src/App.test.tsx packages/editor/src --run`

Expected: PASS.

```sh
git add packages/app/src/hooks/useAppTheme.ts packages/app/src/App.test.tsx packages/app/src/test/app-harness.tsx packages/editor/src/mermaid.ts packages/editor/src
git commit -m "feat(theme): activate catalog CSS safely"
```

### Task 8: Desktop Appearance catalog UI

**Files:**
- Replace: `packages/app/src/components/settings/ThemeSettingsControls.tsx`
- Modify: `packages/app/src/components/settings/AppearanceSettings.tsx`
- Modify: `packages/app/src/components/settings/SettingsControls.tsx` only if an existing button primitive lacks the required disabled/tooltip state
- Modify: `packages/app/src/components/SettingsWindow.tsx`
- Test: `packages/app/src/components/settings/AppearanceSettings.test.tsx`

**Interfaces:**
- Produces: `ThemeCatalogToolbar`, `ThemeCard`, `ThemeSection`, and `ThemeDiagnostics`.
- Consumes: theme descriptors/actions/capabilities returned by `useAppTheme()`.

- [ ] **Step 1: Write failing component tests**

```tsx
expect(screen.getByRole("button", { name: /Import CSS/i })).toBeVisible();
expect(screen.getByRole("button", { name: /Export Current.*Nord/i })).toBeVisible();
expect(within(lightSection).getAllByRole("radio").map(node => node.getAttribute("aria-label"))).toEqual(["Light", "Academic", "Sepia"]);
expect(within(darkSection).queryByText(/placeholder/i)).not.toBeInTheDocument();
```

Add cases for unequal counts, default-first ordering, selected card pinned into the collapsed two-row area, `Show N more themes`, expanded name order, duplicate replacement confirmation, stale replacement refresh, invalid diagnostics, deletion fallback, and failed deletion preserving selection.

- [ ] **Step 2: Run component tests and verify failure**

Run: `pnpm exec vitest run packages/app/src/components/settings/AppearanceSettings.test.tsx`

Expected: FAIL because the current UI only renders swatches and conditional custom CSS editors.

- [ ] **Step 3: Implement the persistent toolbar and independent grids**

Render desktop buttons from runtime capabilities; export label includes the currently resolved theme name. Render separate responsive light/dark CSS grids, each with its default first. Determine collapsed capacity with `ResizeObserver` from card width/gap/available width, cap at two rows, pin a selected third-party descriptor into that visible slice, and never create an inner scroll container.

- [ ] **Step 4: Implement destructive and conflict flows**

Delete confirmation names the theme and explains permanent removal/export-first. Call native delete before repairing selected IDs; on success select the matching protected default and forget approval. When import returns `conflict`, show Replace/Cancel using the existing modal/dialog conventions; pass the existing fingerprint and refresh on `fingerprint_mismatch`.

- [ ] **Step 5: Implement diagnostics and recovery**

Show `N invalid theme files` as an expandable disclosure with file name and localized reason. Keep valid themes selectable. Provide Refresh always and Open Theme Folder when capable; a total scan failure shows Retry without blocking settings navigation.

- [ ] **Step 6: Run component tests and commit**

Run: `pnpm exec vitest run packages/app/src/components/settings/AppearanceSettings.test.tsx`

Expected: PASS.

```sh
git add packages/app/src/components/settings/ThemeSettingsControls.tsx packages/app/src/components/settings/AppearanceSettings.tsx packages/app/src/components/settings/SettingsControls.tsx packages/app/src/components/SettingsWindow.tsx packages/app/src/components/settings/AppearanceSettings.test.tsx
git commit -m "feat(theme): redesign appearance catalog UI"
```

### Task 9: Compact mobile controls and web capability gating

**Files:**
- Modify: compact settings files located by `rg -l 'Appearance|appearance|selectLightTheme|selectDarkTheme' packages/app/src/components/compact`
- Modify: `packages/app/src/components/compact/types.ts`
- Modify: `apps/desktop/src/runtime/mobile.ts`
- Modify: `apps/web/src/runtime/index.test.ts`
- Modify: relevant compact component tests located by `rg --files packages/app/src/components/compact | rg 'test\.(ts|tsx)$'`

**Interfaces:**
- Consumes: `ThemeSection`, `ThemeRuntimeCapabilities`, and catalog actions from prior tasks.
- Produces: mobile selection/refresh/delete without import/export/open-folder, and web protected-default-only rendering.

- [ ] **Step 1: Write failing mobile/web capability tests**

```tsx
expect(screen.getByRole("radio", { name: "Nord" })).toBeVisible();
expect(screen.getByRole("button", { name: /Refresh Themes/i })).toBeVisible();
expect(screen.queryByRole("button", { name: /Import CSS|Export Current|Open Theme Folder/i })).not.toBeInTheDocument();
```

Web tests assert only `Light` and `Dark` cards and no management controls.

- [ ] **Step 2: Run tests and verify failure**

Run: `pnpm exec vitest run packages/app/src/components/compact apps/web/src/runtime/index.test.ts`

Expected: FAIL because compact settings do not consume catalog capabilities.

- [ ] **Step 3: Reuse the two-section catalog presentation with capability gates**

Pass the shared controller through `CompactSettingsProps`; render refresh/delete where supported, suppress desktop-only toolbar actions, and retain the same missing-theme fallback and guarded activation behavior.

- [ ] **Step 4: Run tests and commit**

Run: `pnpm exec vitest run packages/app/src/components/compact apps/web/src/runtime/index.test.ts`

Expected: PASS.

```sh
git add packages/app/src/components/compact apps/desktop/src/runtime/mobile.ts apps/web/src/runtime/index.test.ts
git commit -m "feat(theme): support mobile theme catalogs"
```

### Task 10: Portable settings, settings-window wiring, and old path removal

**Files:**
- Modify: `packages/app/src/hooks/useSettingsWindowState.ts`
- Modify: `packages/app/src/components/SettingsWindow.tsx`
- Modify: `packages/app/src/lib/settings/app-settings.ts`
- Modify: `packages/app/src/lib/settings/app-settings.test.ts`
- Modify: `packages/app/src/lib/settings/settings-events.ts`
- Modify: `packages/app/src/App.test.tsx`

**Interfaces:**
- Produces: version-2 portable import/export with `missingThemeIds: string[]` and no embedded CSS.
- Consumes: current catalog ID set and theme controller repair methods.

- [ ] **Step 1: Write failing portable-settings tests**

```ts
expect(exported.settings).toMatchObject({ appearanceMode: "dark", lightThemeId: "sepia", darkThemeId: "nord" });
expect(exported.settings).not.toHaveProperty("customThemeCss");
await expect(importStoredAppSettingsFile(file, { availableThemeIds: new Set(["light", "dark"]) }))
  .resolves.toMatchObject({ missingThemeIds: ["nord", "sepia"] });
```

- [ ] **Step 2: Run settings tests and verify failure**

Run: `pnpm exec vitest run packages/app/src/lib/settings/app-settings.test.ts packages/app/src/App.test.tsx -t "portable settings|custom theme events"`

Expected: FAIL because version 1 still embeds custom CSS and settings-window wiring emits custom-CSS events.

- [ ] **Step 3: Implement version-2 serialization and missing-ID repair**

Export IDs only. On import, require portable format version 2, compare requested IDs to the current catalog, replace missing light/dark IDs with protected defaults, return the sorted missing list for a warning, and reject malformed or unsupported versions with the existing invalid-settings-file error.

- [ ] **Step 4: Remove obsolete custom CSS runtime/UI/event code**

Delete `CustomThemeCssValues`, default CSS text-area templates, custom-CSS read/write helpers, `markra://custom-theme-css-changed`, SettingsWindow props, and tests that solely describe the removed editor. Keep legacy key constants only inside migration readers.

- [ ] **Step 5: Run tests and commit**

Run: `pnpm exec vitest run packages/app/src/lib/settings/app-settings.test.ts packages/app/src/App.test.tsx`

Expected: PASS.

```sh
git add packages/app/src/hooks/useSettingsWindowState.ts packages/app/src/components/SettingsWindow.tsx packages/app/src/lib/settings packages/app/src/App.test.tsx
git commit -m "feat(theme): finish catalog settings migration"
```

### Task 11: Localization and theme-authoring documentation

**Files:**
- Modify: `packages/shared/src/i18n/locales/types.ts`
- Modify: every locale module under `packages/shared/src/i18n/locales/`
- Create: `docs/theme-authoring.md`
- Modify: `README.md` and `README.zh-CN.md` only if the user's pre-existing changes can be preserved without overlapping edits; otherwise leave README links for a later isolated change.

**Interfaces:**
- Produces: localized UI keys and the public author contract.
- Consumes: the exact metadata/safety/capability behavior already implemented.

- [ ] **Step 1: Add the exact locale key union and translations**

Add keys for Import CSS, Export Current, Refresh Themes, Open Theme Folder, default/third-party labels, delete/replace confirmations, invalid-file disclosure, Show N more, Retry, activation confirmation, missing imported theme warning, and operation failures. Remove keys used only by the deleted CSS text editor after `rg` confirms no consumers.

- [ ] **Step 2: Write the authoring guide with a complete valid file**

The guide includes the metadata block, required/optional limits, ID grammar/reservations, four preview colors, public root/editor tokens, permitted selectors, unstable internal-selector warning, 256 KiB limit, URL rules, one-file/one-appearance constraint, desktop import/export steps, direct-directory refresh behavior, mobile limitations, and the explicit statement that themes are not synchronized.

- [ ] **Step 3: Verify locale and documentation integrity**

Run: `pnpm typecheck:test`

Run: `rg -n 'customCssTitle|importCustomCss|exportCustomCss|resetCustomCss' packages/app packages/shared`

Expected: typecheck PASS; search returns no runtime consumers of removed keys.

- [ ] **Step 4: Commit without touching unrelated README work**

```sh
git add packages/shared/src/i18n docs/theme-authoring.md
git commit -m "docs(theme): document third-party theme format"
```

### Task 12: Repository verification and live desktop smoke test

**Files:**
- Modify only files required to repair failures caused by Tasks 1–11.

**Interfaces:**
- Consumes the complete feature.
- Produces verified native, application, type, build, and live desktop evidence.

- [ ] **Step 1: Run formatting and focused static checks**

Run: `cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check`

Run: `git diff --check`

Expected: both PASS with no output beyond command summaries.

- [ ] **Step 2: Run the complete repository gates**

Run: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml`

Run: `pnpm test`

Run: `pnpm typecheck:test`

Run: `pnpm build`

Expected: all PASS. Do not run S3 live tests because sync is deliberately unchanged.

- [ ] **Step 3: Run the real desktop app and smoke the approved flows**

Run: `pnpm tauri dev`

Verify in the actual settings window: two protected defaults plus 18 seeded files; unequal independent grids; always-visible desktop toolbar; import valid CSS; duplicate replacement; immediate guarded activation; export current; delete active and fallback; invalid file diagnostic after placing an unsafe file in the owned directory; refresh; open folder; restart persistence. Stop the dev process after evidence is collected.

- [ ] **Step 4: Confirm scope and worktree hygiene**

Run: `git status --short`

Run: `git diff --name-only HEAD~11..HEAD`

Expected: user-owned `README.md`, `README.zh-CN.md`, and `macos-icon.icns` remain untouched/untracked as they were; no S3/WebDAV files, generated directories, credentials, or foreign lockfiles appear.

- [ ] **Step 5: Commit any verification-only corrections**

If verification required source corrections, stage only those named files and commit:

```sh
git commit -m "fix(theme): resolve catalog verification issues"
```

If no corrections were needed, do not create an empty commit.
