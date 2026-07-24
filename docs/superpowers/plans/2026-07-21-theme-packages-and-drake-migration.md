# QingYu Resource Theme Packages and Drake Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add validated, portable `.theme` resource packages while preserving legacy CSS themes, then ship Drake Light and Drake Ayu with their supplied WOFF2 fonts.

**Architecture:** Rust owns package validation, mixed catalog discovery, atomic lifecycle operations, content fingerprints, and narrowly scoped Tauri asset permissions. The shared React runtime receives an explicit inline-or-stylesheet activation source; `useAppTheme` keeps legacy CSS in a managed `<style>`, loads resource themes through a race-safe `<link>`, and commits or cancels the native permission token. Bundled Drake directories use the same manifest, validation, catalog, and runtime paths as user-authored themes.

**Tech Stack:** Rust 2021, Tauri v2 asset protocol, `zip`, `serde_json`, `cssparser`, `quick-xml`, `sha2`, React, TypeScript, Vitest, pnpm.

## Global Constraints

- Preserve the unrelated existing changes in `README.md`, `README.zh-CN.md`, and `macos-icon.icns`; never stage or rewrite them.
- Keep protected `light` and `dark` themes unchanged in the installed catalog.
- Keep root-level `.css` import, scanning, selection, replacement, and deletion compatible.
- Import accepts `.css` and `.theme`; export always creates one `.theme` package for the currently selected theme.
- Do not add theme synchronization to S3, WebDAV, or any settings-sync payload.
- Mobile can scan, select, activate, refresh, and delete resource themes, but keeps import, export, and open-folder capabilities disabled.
- Use direct relative WOFF2 resources. Do not Base64-encode Drake fonts.
- Never grant the whole app-data directory to the asset protocol. Grant only a validated package directory that is pending or active in at least one window.
- Do not follow symlinks while scanning, validating, extracting, exporting, replacing, or deleting themes.
- Keep every implementation task test-first: add the named failing test, run it to see the intended failure, make the smallest implementation, then rerun it.
- Use `pnpm` for JavaScript workflows and preserve `pnpm-lock.yaml`.

---

### Task 1: Define the version 1 manifest and split common CSS validation from legacy metadata

**Files:**

- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/Cargo.lock`
- Create: `apps/desktop/src-tauri/src/themes/manifest.rs`
- Modify: `apps/desktop/src-tauri/src/themes/parser.rs`
- Modify: `apps/desktop/src-tauri/src/themes/mod.rs`

- [ ] **Step 1: Add failing strict-manifest tests**

Add colocated tests in `manifest.rs` for:

- the approved `schemaVersion: 1` manifest;
- missing required fields and invalid `entry` values;
- unknown root and `preview` fields;
- reserved/malformed IDs;
- invalid or transparent preview colors;
- bounded `name`, `author`, and `version` values;
- duplicate, unsafe, or missing `licenseFiles` entries.

The public shape should be explicit and closed:

```rust
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ThemeManifest {
    pub(crate) schema_version: u32,
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) appearance: ThemeAppearance,
    pub(crate) entry: String,
    pub(crate) author: Option<String>,
    pub(crate) version: Option<String>,
    pub(crate) preview: ThemePreview,
    #[serde(default)]
    pub(crate) license_files: Vec<String>,
}
```

- [ ] **Step 2: Run the manifest test and confirm it fails**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml themes::manifest::tests
```

Expected: compilation fails because `manifest.rs` and its parser do not exist yet.

- [ ] **Step 3: Add only the dependencies needed by package validation**

Add direct dependencies with narrow ZIP support:

```toml
percent-encoding = "2.3"
unicode-normalization = "0.1"
zip = { version = "4.6.1", default-features = false, features = ["deflate-flate2-zlib-rs"] }
```

Regenerate only `apps/desktop/src-tauri/Cargo.lock` through Cargo. Stored and Deflated ZIP entries are supported; encryption and every other compression family remain unsupported.

- [ ] **Step 4: Implement strict manifest parsing and reusable metadata validators**

Move ID, bounded-text, appearance, preview-color, and reserved-ID validation into reusable functions. Parse UTF-8 JSON with `serde_json`, reject unknown fields, require `entry == "theme.css"`, normalize user-visible strings to NFC, and return `InvalidManifest` rather than exposing parser internals.

Add `InvalidArchive` and `InvalidManifest` to `ThemeErrorCode`; keep the existing legacy error codes stable.

- [ ] **Step 5: Add failing tests for the two CSS policies**

Extend `parser.rs` tests so:

- `parse_theme_file` still requires the leading `@qingyu-theme` comment and still rejects every relative URL;
- `validate_package_css` accepts `./assets/fonts/JetBrainsMono-Regular.woff2`, returns the normalized referenced path, and accepts fragments and bounded `data:` URLs;
- package CSS rejects `@import`, network/protocol-relative/file/absolute URLs, bare relative URLs, encoded traversal, query/fragment tricks on resource paths, and `./assets/../licenses/x`.

- [ ] **Step 6: Run the parser test and confirm the new cases fail**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml themes::parser::tests
```

Expected: the new package-CSS tests fail because only the legacy URL policy exists.

- [ ] **Step 7: Separate syntax walking from URL policy**

Refactor the CSS parser around a policy callback instead of duplicating token traversal:

```rust
pub(crate) struct ValidatedPackageCss {
    pub(crate) css: String,
    pub(crate) referenced_assets: BTreeSet<String>,
}

pub(crate) fn validate_package_css(bytes: &[u8]) -> Result<ValidatedPackageCss, ThemeError>;
pub(crate) fn parse_theme_file(bytes: &[u8], file_name: &str) -> Result<ParsedTheme, ThemeError>;
```

Percent-decode package paths, require UTF-8, normalize separators and Unicode, then accept only `./assets/<non-empty-safe-path>`. Keep `MAX_THEME_BYTES` at 256 KiB and bound `data:` URLs through that same CSS limit.

- [ ] **Step 8: Run focused Rust tests**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml themes::manifest::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml themes::parser::tests
```

Expected: both suites pass, including all existing legacy CSS tests.

- [ ] **Step 9: Commit the parser contract**

```bash
git add apps/desktop/src-tauri/Cargo.toml apps/desktop/src-tauri/Cargo.lock apps/desktop/src-tauri/src/themes/manifest.rs apps/desktop/src-tauri/src/themes/parser.rs apps/desktop/src-tauri/src/themes/mod.rs
git commit -m "feat(theme): define resource theme manifest"
```

---

### Task 2: Validate unpacked package directories, assets, SVG, and deterministic fingerprints

**Files:**

- Create: `apps/desktop/src-tauri/src/themes/resources.rs`
- Modify: `apps/desktop/src-tauri/src/themes/mod.rs`
- Modify: `apps/desktop/src-tauri/src/themes/parser.rs`

- [ ] **Step 1: Add failing directory-validation tests**

Use `tempfile` fixtures in `resources.rs` to cover:

- a minimal valid `manifest.json` plus `theme.css` package;
- four valid WOFF2 resources and a declared UTF-8 license;
- PNG, JPEG, WebP, GIF, and safe SVG signatures;
- a CSS reference whose asset is absent;
- unreferenced but otherwise valid assets, which remain exportable and fingerprinted;
- symlinked package roots, files, and directories;
- unsupported files, nested archives, non-UTF-8 licenses, and files outside approved roots;
- path depth, path length, per-file, entry-count, and 32 MiB aggregate limits;
- NFC aliases and case-insensitive collisions;
- fonts without at least one existing declared license;
- scripts, event attributes, `foreignObject`, external links, unsafe `url(...)`, and active content in SVG;
- a changed CSS, font, icon, manifest, or license changing the fingerprint;
- identical content created in different filesystem enumeration orders producing the same fingerprint.

- [ ] **Step 2: Run the resource tests and confirm they fail**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml themes::resources::tests
```

Expected: compilation fails because the directory validator is not implemented.

- [ ] **Step 3: Implement normalized relative paths and bounded directory walking**

Create constants matching the approved design:

```rust
const MAX_PACKAGE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_PACKAGE_ENTRIES: usize = 256;
const MAX_PATH_CHARS: usize = 240;
const MAX_PATH_DEPTH: usize = 16;
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_FONT_BYTES: u64 = 4 * 1024 * 1024;
const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;
```

Use `symlink_metadata`, reject non-file/non-directory entries, reject path aliases before reading content, and permit only:

```text
manifest.json
theme.css
assets/**/*.woff2|png|jpg|jpeg|webp|gif|svg
licenses/**/*.txt|md
```

Enforce the manifest limit as 64 KiB, every WOFF2 limit as 4 MiB, and every raster/SVG limit as 8 MiB before and while reading.

- [ ] **Step 4: Implement file-content and SVG validation**

Check WOFF2 and raster magic bytes before accepting extensions. Parse SVG with `quick_xml` and fail closed on active elements, event attributes, external references, non-fragment `href`/`xlink:href`, unsafe CSS URLs, doctypes, and entity declarations. License files must be readable UTF-8 and are never returned as asset URLs.

- [ ] **Step 5: Implement the canonical content fingerprint**

Return one validated package object used by scan, import, export, and activation:

```rust
pub(crate) struct ValidatedThemeDirectory {
    pub(crate) descriptor: ThemeDescriptor,
    pub(crate) root: PathBuf,
    pub(crate) files: Vec<ValidatedThemeFile>,
}

pub(crate) fn validate_theme_directory(root: &Path, storage_name: &str)
    -> Result<ValidatedThemeDirectory, ThemeError>;
```

Hash a version tag, canonical serialized validated manifest, then each NFC-normalized relative path, byte length, and bytes in sorted path order. Set `ThemeDescriptor.storage_kind` to `ResourceDirectory`; legacy CSS sets it to `InlineCss`.

- [ ] **Step 6: Run focused tests**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml themes::resources::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml themes::parser::tests
```

Expected: all resource, SVG, fingerprint, and legacy parser tests pass.

- [ ] **Step 7: Commit package-directory validation**

```bash
git add apps/desktop/src-tauri/src/themes/resources.rs apps/desktop/src-tauri/src/themes/parser.rs apps/desktop/src-tauri/src/themes/mod.rs
git commit -m "feat(theme): validate resource theme directories"
```

---

### Task 3: Add safe `.theme` archive extraction and package export

**Files:**

- Create: `apps/desktop/src-tauri/src/themes/archive.rs`
- Modify: `apps/desktop/src-tauri/src/themes/mod.rs`

- [ ] **Step 1: Add failing archive attack and round-trip tests**

Build ZIPs in memory with `zip::ZipWriter` and test:

- valid Stored and Deflated packages;
- source files above 16 MiB;
- declared and streamed uncompressed output above 32 MiB;
- more than 256 entries;
- absolute, drive-prefixed, UNC, NUL, empty-segment, `.`, `..`, backslash, overlong, and over-deep paths;
- percent-encoded traversal later referenced by CSS;
- duplicate normalized paths and case-insensitive collisions;
- Unix symlink, hard-link metadata conventions, and special-file modes;
- encrypted entries and unsupported compression;
- nested archive extensions;
- extraction failure leaving no files outside or below the staging directory;
- two differently compressed archives with identical content producing the same content fingerprint;
- export followed by fresh import preserving descriptor and all resource bytes.

- [ ] **Step 2: Run the archive tests and confirm they fail**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml themes::archive::tests
```

Expected: compilation fails because archive preparation and export functions do not exist.

- [ ] **Step 3: Implement bounded archive inspection and extraction**

Use a regular source file verified with `symlink_metadata`. Reject files larger than 16 MiB before opening ZIP metadata. Read raw entry-name bytes and reject non-UTF-8 instead of accepting ZIP decoder replacement characters. Normalize and reserve every entry path before creating anything. Reject link metadata and every Unix mode other than regular file or directory. Extract with new-file semantics into a unique `.qingyu-theme-<pid>-<counter>.dir` below the catalog root, stream-count actual bytes, sync files, validate the completed directory with `validate_theme_directory`, and recursively remove staging on every error.

Expose an owned prepared import that cleans itself unless catalog installation consumes it:

```rust
pub(crate) enum PreparedThemeImport {
    LegacyCss(ParsedTheme),
    ResourcePackage(PreparedThemeDirectory),
}

pub(crate) fn prepare_external_theme(source: &Path, catalog_root: &Path)
    -> Result<PreparedThemeImport, ThemeError>;
```

- [ ] **Step 4: Implement deterministic `.theme` writing**

Write `manifest.json` first and remaining files in normalized sorted order. Use Deflate, portable regular-file permissions, and no host-specific path separators or timestamps. Write to a new temporary sibling of the requested target, finish and sync it, reopen it through the same archive validator, then atomically publish it. On overwrite platforms, retain a sibling backup until publication succeeds and restore it on failure.

For generated packages:

- legacy CSS keeps its existing descriptor ID and exact CSS bytes;
- protected Light exports as importable ID `light-starter` and Dark as `dark-starter`, with generated manifests and their canonical CSS;
- generated packages contain no fake asset or license entries.

- [ ] **Step 5: Run archive and resource tests**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml themes::archive::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml themes::resources::tests
```

Expected: attack fixtures fail closed, round trips pass, and failed writes leave neither partial targets nor staging residue.

- [ ] **Step 6: Commit archive support**

```bash
git add apps/desktop/src-tauri/src/themes/archive.rs apps/desktop/src-tauri/src/themes/mod.rs
git commit -m "feat(theme): add portable theme archives"
```

---

### Task 4: Upgrade the catalog to mixed CSS/directory lifecycle operations

**Files:**

- Modify: `apps/desktop/src-tauri/src/themes/catalog.rs`
- Modify: `apps/desktop/src-tauri/src/themes/mod.rs`
- Modify: `apps/desktop/src-tauri/src/themes/migration.rs`

- [ ] **Step 1: Add failing mixed-catalog tests**

Extend `catalog.rs` tests for:

- scanning valid root `.css` files and valid root theme directories together;
- reporting invalid files and directories without hiding valid themes;
- rejecting duplicate IDs across CSS and directories;
- skipping only owned `.qingyu-theme-*` staging/backup entries;
- import conflicts returning the candidate and existing descriptor for either storage kind;
- new package install renaming only a fully validated staging directory;
- CSS→directory and directory→CSS replacement with expected-fingerprint checks;
- injected publication failure restoring the exact previous file/directory;
- resource directory deletion and legacy file deletion;
- export/reimport for protected, legacy, and resource themes;
- directly placed author directories never being rewritten by refresh.

- [ ] **Step 2: Run the catalog tests and confirm they fail**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml themes::catalog::tests
```

Expected: the mixed scan and package lifecycle assertions fail against the CSS-only catalog.

- [ ] **Step 3: Introduce explicit storage kinds and safe catalog targets**

Add serialized storage to native and parsed descriptors:

```rust
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ThemeStorageKind {
    InlineCss,
    ResourceDirectory,
}
```

Keep `file_name` as the root storage entry name for compatibility, and use `storage_kind` rather than extensions to choose read, replace, delete, export, or activation behavior.

- [ ] **Step 4: Replace byte-only lifecycle methods with prepared imports**

Refactor command flow to:

```text
read and validate external source
→ compare candidate ID against a fresh scan
→ return conflict or install
→ on approved replacement, revalidate installed fingerprint
→ stage new storage
→ move old storage to an owned backup
→ publish new storage
→ rollback old storage on failure
→ delete backup after success
```

Never remove the current version before the replacement is ready. Re-scan after publication and return the actual installed descriptor.

- [ ] **Step 5: Change command-level import/export behavior**

`read_external_theme` becomes the `.css`/`.theme` dispatcher. `import_theme_file` and `replace_theme_file` accept either extension. `export_theme_file` always calls the package writer and rejects a target without the `.theme` extension. Preserve the existing conflict-confirmation UI contract (`sourcePath` is reopened and revalidated on replace).

- [ ] **Step 6: Run focused lifecycle tests**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml themes::catalog::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml themes::archive::tests
```

Expected: CSS and directory themes pass the same conflict, fingerprint, rollback, delete, and export rules.

- [ ] **Step 7: Commit mixed catalog support**

```bash
git add apps/desktop/src-tauri/src/themes/catalog.rs apps/desktop/src-tauri/src/themes/mod.rs apps/desktop/src-tauri/src/themes/migration.rs
git commit -m "feat(theme): support mixed theme catalog storage"
```

---

### Task 5: Add Drake Light/Ayu package directories and the catalog v2 incremental seed

**Files:**

- Create: `apps/desktop/src-tauri/themes/third-party/drake-light/manifest.json`
- Create: `apps/desktop/src-tauri/themes/third-party/drake-light/theme.css`
- Create: `apps/desktop/src-tauri/themes/third-party/drake-light/assets/fonts/JetBrainsMono-Regular.woff2`
- Create: `apps/desktop/src-tauri/themes/third-party/drake-light/assets/fonts/JetBrainsMono-Bold.woff2`
- Create: `apps/desktop/src-tauri/themes/third-party/drake-light/assets/fonts/JetBrainsMono-Italic.woff2`
- Create: `apps/desktop/src-tauri/themes/third-party/drake-light/assets/fonts/JetBrainsMono-BoldItalic.woff2`
- Create: `apps/desktop/src-tauri/themes/third-party/drake-light/licenses/THEME-LICENSE.txt`
- Create: `apps/desktop/src-tauri/themes/third-party/drake-light/licenses/FONT-LICENSE.txt`
- Create: `apps/desktop/src-tauri/themes/third-party/drake-ayu/manifest.json`
- Create: `apps/desktop/src-tauri/themes/third-party/drake-ayu/theme.css`
- Create: `apps/desktop/src-tauri/themes/third-party/drake-ayu/assets/fonts/JetBrainsMono-Regular.woff2`
- Create: `apps/desktop/src-tauri/themes/third-party/drake-ayu/assets/fonts/JetBrainsMono-Bold.woff2`
- Create: `apps/desktop/src-tauri/themes/third-party/drake-ayu/assets/fonts/JetBrainsMono-Italic.woff2`
- Create: `apps/desktop/src-tauri/themes/third-party/drake-ayu/assets/fonts/JetBrainsMono-BoldItalic.woff2`
- Create: `apps/desktop/src-tauri/themes/third-party/drake-ayu/licenses/THEME-LICENSE.txt`
- Create: `apps/desktop/src-tauri/themes/third-party/drake-ayu/licenses/FONT-LICENSE.txt`
- Modify: `apps/desktop/src-tauri/src/themes/catalog.rs`
- Modify: `apps/desktop/src-tauri/src/themes/migration.rs`
- Modify: `apps/desktop/src-tauri/src/themes/mod.rs`
- Modify: `packages/app/src/styles.test.ts`

- [ ] **Step 1: Add failing Drake validation and migration tests**

Add tests asserting:

- both embedded Drake directories validate through the production validator;
- IDs are `drake-light`/light and `drake-ayu`/dark;
- each package references and contains four WOFF2 faces plus both declared licenses;
- all WOFF2 bytes begin with the WOFF2 signature and match the supplied source files byte-for-byte;
- a fresh catalog installs the original 18 CSS seeds plus both Drake directories;
- a v1 catalog installs only the missing Drake IDs, preserves current selection, does not restore a deleted old seed, and advances to v2;
- an existing user theme with a Drake ID wins without overwrite;
- an occupied destination becomes a diagnostic and does not block startup;
- rerunning v2 initialization is idempotent.

Update `packages/app/src/styles.test.ts` so it validates Drake CSS through catalog fixtures instead of assuming every bundled third-party theme is a single `.css` file.

- [ ] **Step 2: Run the tests and confirm they fail**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml themes::migration::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml drake
pnpm --filter @markra/app test -- src/styles.test.ts
```

Expected: Drake assets and catalog version 2 do not exist yet.

- [ ] **Step 3: Copy and verify the supplied fonts and license inputs**

Copy the four WOFF2 files from `/Users/ying/Downloads/DrakeTyporaTheme-2.9.6/drake/` into each package without conversion. Copy `/Users/ying/Downloads/DrakeTyporaTheme-2.9.6/LICENSE` as `THEME-LICENSE.txt`. Store the verified JetBrains Mono Patch OFL 1.1 text as `FONT-LICENSE.txt`. Compare SHA-256 values of each source/destination font pair before staging.

- [ ] **Step 4: Write semantic QingYu Drake CSS**

Each CSS file must:

- retain the upstream Drake MIT notice;
- declare regular 400, italic 400, bold 700, and bold-italic 700 `@font-face` rules using `./assets/fonts/*.woff2`;
- apply JetBrains Mono to the application root, Markdown body, headings, source editor, inline code, and fenced code;
- map the Drake Light white/coral palette or Drake Ayu blue-gray/gold palette onto QingYu application tokens;
- style stable QingYu/Milkdown selectors for centered H1, emphasized H2, heading hierarchy, links, blockquotes, pill highlights, code/syntax, striped tables, `kbd`, tasks, Mermaid, and compatible icons;
- omit Typora panels, menus, CodeMirror selectors, `@include-when-export`, external Google Fonts, and `@import`.

Use package-specific selectors such as:

```css
:root[data-theme="drake-light"] { /* app tokens */ }
.markdown-paper[data-editor-theme="drake-light"] { /* document typography */ }
.markdown-source-paper[data-editor-theme="drake-light"] { /* source mode */ }
```

- [ ] **Step 5: Implement versioned embedded package seeds**

Replace the one-shot Boolean migration with ordered versions:

```text
version 0 → seed the original CSS catalog and both current Drake packages
version 1 → seed only Drake Light and Drake Ayu
version 2 → scan only
```

Model embedded resource packages as explicit `(relative_path, include_bytes!(...))` arrays. Install a package only if neither its ID nor target exists; validate the materialized staging directory before rename. Never call the original 18-theme `seed_missing` during v1→v2.

- [ ] **Step 6: Run Drake and migration tests**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml themes::migration::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml drake
pnpm --filter @markra/app test -- src/styles.test.ts
```

Expected: both packages validate, font hashes match, and incremental seeding preserves deletion and user ownership.

- [ ] **Step 7: Commit bundled Drake themes**

```bash
git add apps/desktop/src-tauri/themes/third-party/drake-light apps/desktop/src-tauri/themes/third-party/drake-ayu apps/desktop/src-tauri/src/themes/catalog.rs apps/desktop/src-tauri/src/themes/migration.rs apps/desktop/src-tauri/src/themes/mod.rs packages/app/src/styles.test.ts
git commit -m "feat(theme): bundle Drake resource themes"
```

---

### Task 6: Implement native activation tokens and narrow asset-scope lifetime

**Files:**

- Create: `apps/desktop/src-tauri/src/themes/activation.rs`
- Modify: `apps/desktop/src-tauri/src/themes/catalog.rs`
- Modify: `apps/desktop/src-tauri/src/themes/mod.rs`
- Modify: `apps/desktop/src-tauri/src/desktop_runtime.rs`
- Modify: `apps/desktop/src-tauri/src/mobile_runtime.rs`
- Modify: `apps/desktop/src-tauri/src/builder_boundary_tests.rs`

- [ ] **Step 1: Add failing activation-state tests**

Test the state machine independently of WebView loading:

- legacy CSS preparation returns inline CSS and a token without granting a directory;
- resource preparation revalidates the full content fingerprint, grants only that package root, and returns its real `theme.css` path;
- commit replaces the current window activation and revokes an unreferenced old directory;
- cancel removes a pending token and revokes its directory only when no other pending/active window references it;
- a newer preparation invalidates older pending work for the same window;
- two windows can reference the same package without premature revocation;
- release-window and delete-theme cleanup remove pending/active references;
- modified/missing CSS, font, image, manifest, or license causes fingerprint mismatch before a path is exposed;
- permission grant/forbid failures return safe errors and never broaden scope.

Use injected `allow_directory`/`forbid_directory` closures in unit tests rather than requiring a live Tauri runtime.

- [ ] **Step 2: Run activation tests and confirm they fail**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml themes::activation::tests
```

Expected: compilation fails because the activation state and commands do not exist.

- [ ] **Step 3: Implement the native activation contract**

Use one managed mutex state with pending tokens and active package paths by window label:

```rust
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum ThemeActivationSource {
    Inline { css: String },
    Stylesheet { path: String },
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ThemeActivationPayload {
    pub(crate) token: String,
    pub(crate) id: String,
    pub(crate) fingerprint: String,
    pub(crate) source: ThemeActivationSource,
}
```

Add commands using the invoking `tauri::WebviewWindow` identity, not a caller-supplied label:

```text
prepare_theme_activation(id, expectedFingerprint)
commit_theme_activation(token)
cancel_theme_activation(token)
release_theme_activation()
```

The prepare command grants the validated resource directory recursively and returns the filesystem path, never an app-data root. The frontend will add a fingerprint query after converting it to an asset URL.

- [ ] **Step 4: Wire state and cleanup into desktop and mobile builders**

Manage `ThemeActivationState::default()` in both builders and register all four commands in both runtime command lists. In desktop `on_window_event`, call release cleanup on `WindowEvent::Destroyed`. Mobile keeps the same commands even though it has no import/export controls.

Before resource deletion, remove all activation references for that exact validated directory and forbid it; then delete it. Keep reference-count behavior when another window still uses a different theme directory.

- [ ] **Step 5: Update boundary tests and run focused Rust tests**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml themes::activation::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml builder_boundary_tests
```

Expected: activation state tests and desktop/mobile command allow-list tests pass.

- [ ] **Step 6: Commit native activation support**

```bash
git add apps/desktop/src-tauri/src/themes/activation.rs apps/desktop/src-tauri/src/themes/catalog.rs apps/desktop/src-tauri/src/themes/mod.rs apps/desktop/src-tauri/src/desktop_runtime.rs apps/desktop/src-tauri/src/mobile_runtime.rs apps/desktop/src-tauri/src/builder_boundary_tests.rs
git commit -m "feat(theme): scope resource theme activation"
```

---

### Task 7: Expose the explicit activation source through desktop and mobile runtimes

**Files:**

- Modify: `packages/app/src/lib/themes/theme-catalog.ts`
- Modify: `packages/app/src/lib/themes/theme-catalog.test.ts`
- Modify: `packages/app/src/runtime/index.ts`
- Modify: `packages/app/src/runtime/index.test.ts`
- Modify: `apps/desktop/src/runtime/tauri/themes/shared.ts`
- Modify: `apps/desktop/src/runtime/tauri/themes.ts`
- Modify: `apps/desktop/src/runtime/tauri/themes.test.ts`
- Modify: `apps/desktop/src/runtime/desktop.ts`
- Modify: `apps/desktop/src/runtime/mobile.ts`
- Modify: `apps/desktop/src/runtime/index.test.ts`
- Modify: `apps/desktop/src/runtime/mobile.test.ts`

- [ ] **Step 1: Add failing TypeScript contract and adapter tests**

Update fixtures to require `storageKind`. Test that:

- inline native payloads remain inline;
- stylesheet native paths pass through `convertFileSrc` and append `?fingerprint=<encoded>`;
- prepare, commit, cancel, and release invoke the exact semantic native commands;
- desktop import picker offers `css` and `theme`;
- desktop export defaults to `<id>.theme` and filters only `.theme`;
- desktop capabilities remain all enabled;
- mobile maps prepare/commit/cancel/release but still reports import/export/open-folder disabled;
- default web runtime provides safe unsupported implementations and no resource URL.

The shared contract should become:

```ts
export type ThemeStorageKind = "inlineCss" | "resourceDirectory";

export type ThemeActivationPayload = {
  fingerprint: string;
  id: string;
  token: string;
  source:
    | { kind: "inline"; css: string }
    | { kind: "stylesheet"; href: string };
};

export type AppThemeRuntime = {
  prepareActivation: (id: string, expectedFingerprint: string) => Promise<ThemeActivationPayload>;
  commitActivation: (token: string) => Promise<unknown>;
  cancelActivation: (token: string) => Promise<unknown>;
  releaseActivation: () => Promise<unknown>;
  // existing catalog/import/export/delete members remain
};
```

- [ ] **Step 2: Run the TypeScript tests and confirm they fail**

Run:

```bash
pnpm --filter @markra/app test -- src/lib/themes/theme-catalog.test.ts src/runtime/index.test.ts
pnpm --filter @markra/desktop test -- src/runtime/tauri/themes.test.ts src/runtime/index.test.ts src/runtime/mobile.test.ts
```

Expected: fixtures and adapters fail because they still expose `readCss` and CSS-only dialogs.

- [ ] **Step 3: Implement types and native mappings**

Replace `ThemeCssPayload`/`readCss` with the activation API across all runtimes. In the Tauri adapter, import `convertFileSrc` from `@tauri-apps/api/core`, convert only `stylesheet.path`, and add the fingerprint query so replacing the same ID cannot reuse stale CSS.

Change picker configuration to:

```ts
filters: [{ extensions: ["css", "theme"], name: "Theme" }]
```

and export configuration to:

```ts
defaultPath: `${id}.theme`,
filters: [{ extensions: ["theme"], name: "Theme" }]
```

- [ ] **Step 4: Run runtime tests and type checks**

Run:

```bash
pnpm --filter @markra/app test -- src/lib/themes/theme-catalog.test.ts src/runtime/index.test.ts
pnpm --filter @markra/desktop test -- src/runtime/tauri/themes.test.ts src/runtime/index.test.ts src/runtime/mobile.test.ts
pnpm --filter @markra/app build
pnpm --filter @markra/desktop typecheck:test
```

Expected: all runtime contracts compile and mobile remains capability-limited.

- [ ] **Step 5: Commit runtime contracts**

```bash
git add packages/app/src/lib/themes/theme-catalog.ts packages/app/src/lib/themes/theme-catalog.test.ts packages/app/src/runtime/index.ts packages/app/src/runtime/index.test.ts apps/desktop/src/runtime/tauri/themes/shared.ts apps/desktop/src/runtime/tauri/themes.ts apps/desktop/src/runtime/tauri/themes.test.ts apps/desktop/src/runtime/desktop.ts apps/desktop/src/runtime/mobile.ts apps/desktop/src/runtime/index.test.ts apps/desktop/src/runtime/mobile.test.ts
git commit -m "feat(theme): expose resource activation runtime"
```

---

### Task 8: Load resource themes through a race-safe managed stylesheet link

**Files:**

- Modify: `packages/app/src/hooks/useAppTheme.ts`
- Modify: `packages/app/src/hooks/useAppTheme.test.tsx`
- Modify: `packages/app/src/test/app-harness.tsx`
- Modify: `packages/app/src/App.test.tsx`

- [ ] **Step 1: Add failing hook tests for inline and linked activation**

Retain the current inline tests and add cases for:

- legacy activation creating `#markra-third-party-theme-style` and committing its token;
- resource activation creating a candidate `<link rel="stylesheet">` with the adapter URL;
- readiness and confirmation waiting for the candidate link `load` event;
- approved fingerprints skipping the dialog but still waiting and committing;
- link `error` canceling the token, removing all third-party style/link elements, releasing active native scope, and applying the protected default of the same appearance;
- rejection canceling the candidate and restoring the previous preferences without persistence;
- a slow first link followed by a fast second link never allowing the first to commit or replace the DOM;
- switching to protected Light/Dark releasing the prior native activation;
- switching between resource themes keeping the old active link until the new one loads, then removing it;
- unmount canceling pending activation and releasing the active window activation;
- stale fingerprint/native prepare failures using the existing repair path and error surface.

In JSDOM tests, dispatch `load` and `error` directly on the candidate link instead of using timers.

- [ ] **Step 2: Run hook tests and confirm they fail**

Run:

```bash
pnpm --filter @markra/app test -- src/hooks/useAppTheme.test.tsx
```

Expected: link-source fixtures fail because `useAppTheme` only injects CSS strings.

- [ ] **Step 3: Add a single managed theme-element loader**

Keep one active element ID, but permit a candidate alongside it while loading:

```text
#markra-third-party-theme-style        legacy active style
#markra-third-party-theme-link         resource active link
[data-markra-theme-candidate="token"] pending link
```

For an inline source, install the managed `<style>` immediately. For a stylesheet source, append a candidate link and await `load`/`error`. Only the current React activation sequence may promote a candidate. Promotion removes the prior active style/link, assigns the active link ID, applies root data attributes, then continues to confirmation and native commit.

- [ ] **Step 4: Integrate native commit/cancel/release with preference confirmation**

Use this exact order:

```text
prepare native activation
→ load candidate source
→ verify React sequence is current
→ apply preview root
→ request first-fingerprint confirmation when required
→ on accept, store approval and pending preferences
→ commit native token
→ mark ready
```

On stale work, rejection, link error, or exception, remove the candidate and call `cancelActivation(token)`. On protected fallback or hook unmount, call `releaseActivation()` and remove both managed elements. A cleanup rejection may be reported diagnostically but must not restore a stale theme.

- [ ] **Step 5: Run hook and application tests**

Run:

```bash
pnpm --filter @markra/app test -- src/hooks/useAppTheme.test.tsx src/App.test.tsx
pnpm --filter @markra/app typecheck:test
```

Expected: inline compatibility, load gating, rejection, failure fallback, race cancellation, and release cleanup all pass.

- [ ] **Step 6: Commit frontend resource loading**

```bash
git add packages/app/src/hooks/useAppTheme.ts packages/app/src/hooks/useAppTheme.test.tsx packages/app/src/test/app-harness.tsx packages/app/src/App.test.tsx
git commit -m "feat(theme): load resource theme stylesheets"
```

---

### Task 9: Update theme controls, author documentation, and package examples

**Files:**

- Modify: `packages/app/src/components/settings/ThemeSettingsControls.tsx`
- Modify: `packages/app/src/components/settings/AppearanceSettings.test.tsx`
- Modify: `packages/app/src/components/compact/CompactSettingsDetail.test.tsx`
- Modify: `packages/shared/src/i18n/locales/types.ts`
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
- Modify: `docs/theme-authoring.md`

- [ ] **Step 1: Add failing control tests**

Assert that desktop shows an “Import theme” action and exports the actual active descriptor, while compact/mobile still shows refresh/delete and theme selection but no import, export, or open-folder action. Keep the existing unequal light/dark counts and selected-theme visibility tests unchanged.

- [ ] **Step 2: Run control tests and confirm the copy assertion fails**

Run:

```bash
pnpm --filter @markra/app test -- src/components/settings/AppearanceSettings.test.tsx src/components/compact/CompactSettingsDetail.test.tsx
```

Expected: the desktop label still says “Import CSS”.

- [ ] **Step 3: Rename the import copy without changing toolbar behavior**

Rename the i18n key from `settings.theme.importCss` to `settings.theme.importTheme` in the typed locale contract and every locale. Use the localized equivalent of “Import theme”; do not add a second import button.

- [ ] **Step 4: Rewrite the author guide around ordinary directories**

Document in `docs/theme-authoring.md`:

- when a single `.css` theme is enough;
- the exact unpacked directory tree for resource themes;
- the closed version 1 manifest schema and one complete example;
- supported files and all size/path limits;
- stable app/editor selectors and variables;
- `@font-face` with `url("./assets/fonts/example.woff2")`;
- allowed and rejected URL examples;
- font-license declaration rules;
- author workflow: copy directory into the app theme folder, Refresh, select, then Export current to create `.theme`;
- `.theme` being an ordinary ZIP containing one root theme;
- desktop/mobile capability differences;
- first-activation fingerprint confirmation and replacement behavior;
- current non-goals, including theme sync and network resources.

Include a minimal copyable `manifest.json` plus `theme.css`; no future CLI is required by the guide.

- [ ] **Step 5: Run UI, locale, and build checks**

Run:

```bash
pnpm --filter @markra/app test -- src/components/settings/AppearanceSettings.test.tsx src/components/compact/CompactSettingsDetail.test.tsx
pnpm --filter @markra/shared build
pnpm --filter @markra/app build
```

Expected: desktop and compact controls match their capabilities and every locale satisfies the typed key set.

- [ ] **Step 6: Commit UI copy and author guidance**

```bash
git add packages/app/src/components/settings/ThemeSettingsControls.tsx packages/app/src/components/settings/AppearanceSettings.test.tsx packages/app/src/components/compact/CompactSettingsDetail.test.tsx packages/shared/src/i18n/locales docs/theme-authoring.md
git commit -m "docs(theme): document resource theme authoring"
```

---

### Task 10: Run full regression, archive interoperability, and live Drake verification

**Files:**

- Modify if required by discovered defects only: files already listed in Tasks 1–9

- [ ] **Step 1: Run formatting and focused package tests**

Run:

```bash
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml themes::
pnpm --filter @markra/app test -- src/lib/themes/theme-catalog.test.ts src/hooks/useThemeCatalog.test.tsx src/hooks/useAppTheme.test.tsx src/components/settings/AppearanceSettings.test.tsx src/components/compact/CompactSettingsDetail.test.tsx src/styles.test.ts src/App.test.tsx
pnpm --filter @markra/desktop test -- src/runtime/tauri/themes.test.ts src/runtime/index.test.ts src/runtime/mobile.test.ts
```

Expected: all focused native and frontend theme suites pass.

- [ ] **Step 2: Verify `.theme` interoperability outside the app**

Export Drake Light, Drake Ayu, one legacy CSS theme, and protected Light. For each output:

```bash
unzip -t /absolute/path/to/exported.theme
unzip -l /absolute/path/to/exported.theme
```

Expected: standard ZIP tools recognize every archive; root contains `manifest.json` and `theme.css`; Drake outputs also contain four WOFF2 files and both licenses; no absolute paths, staging names, or unrelated app-data files appear.

- [ ] **Step 3: Run the repository-required gates**

Run exactly:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
pnpm test
pnpm typecheck:test
pnpm build
```

Expected: every command exits 0.

- [ ] **Step 4: Run the real desktop app with isolated app data**

Create a temporary Tauri config overlay with a unique debug `identifier` and product name, start with that overlay plus `pnpm tauri dev`, and verify it does not touch the normal QingYu theme catalog. Do not edit or delete the normal app-data directory.

In the isolated app:

1. confirm fresh catalog shows Drake Light under Light and Drake Ayu under Dark;
2. select Drake Light, approve it, and verify regular/bold/italic/bold-italic faces through `document.fonts.check`;
3. select Drake Ayu and repeat the font checks;
4. test Follow system with the two Drake themes selected;
5. inspect headings, paragraphs, emphasis, links, lists, tasks, blockquotes, highlights, tables, inline/fenced code, Mermaid, images, and icons;
6. switch rapidly among both Drake themes and protected defaults and confirm no stale link wins;
7. import a valid `.theme`, import a legacy `.css`, and confirm invalid archive/path/resource fixtures leave no residue;
8. export the active default, legacy, and Drake themes and reimport each under a clean isolated catalog;
9. manually place an unpacked theme directory below the shown theme folder, Refresh, select it, and export it;
10. confirm the normal app-data catalog remains untouched after exit.

- [ ] **Step 5: Verify mobile compilation and capability boundaries**

Run the existing mobile configuration/boundary tests through the full Rust and pnpm suites. If an Android/iOS build environment is available, build the current mobile target and confirm resource activation commands compile. Do not add import/export/open-folder UI to compact settings.

- [ ] **Step 6: Inspect the final diff and commit only verification fixes**

Run:

```bash
git status --short
git diff --check
git diff --stat HEAD~9..HEAD
```

Confirm `README.md`, `README.zh-CN.md`, and `macos-icon.icns` remain outside every feature commit. If live verification exposed a defect, add a regression test first, apply the smallest fix, rerun the affected focused suite and all four repository gates, inspect `git diff --name-only`, stage only the regression test and its implementation files by their exact paths, then commit:

```bash
git commit -m "fix(theme): harden resource theme workflow"
```

If no defect was found, do not create an empty verification commit.
