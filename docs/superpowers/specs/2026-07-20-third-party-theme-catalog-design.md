# Third-Party Theme Catalog Design

## Status

Approved in conversation on 2026-07-20. The user approved the recommended architecture and authorized the remaining design decisions to follow the recommended path without further confirmation prompts.

## Goal

Replace QingYu's fixed list of built-in palettes and two `custom` CSS text areas with a file-backed theme catalog:

- QingYu keeps exactly one protected default light theme and one protected default dark theme.
- Every other existing theme becomes a standard third-party CSS theme file.
- User-imported CSS files are copied into an application-owned theme directory.
- The application scans that directory and makes every valid theme available for selection.
- Import and export controls remain visible on desktop regardless of which theme is selected.
- Theme synchronization is explicitly out of scope and will be designed separately later.

## Context

The current theme system stores `appearanceMode`, `lightTheme`, and `darkTheme` in `settings.json`. Theme IDs are closed TypeScript and Rust enums. Application palettes and editor-specific rules are compiled into `packages/app/src/styles.css`. The special `custom` theme stores one light CSS string and one dark CSS string in settings, then injects the active string into a style element. The Appearance page reveals import, export, reset, and CSS editing controls only when `custom` is selected.

The repository already has a safe application-data-directory pattern for Markdown templates. A native service resolves `app_data_dir`, validates a single filename, creates the owned directory, and performs narrowly scoped file operations. The theme catalog should follow that boundary instead of exposing a general renderer filesystem API.

Current S3 and WebDAV synchronization operates on the active project folder. It does not synchronize application-level `settings.json`, and it deliberately excludes the project `.qingyu` control directory. The theme catalog must not broaden that sync boundary.

## Selected Approach

Use one managed application theme directory as the source of truth for every third-party theme:

```text
app_data_dir/
├── settings.json
└── themes/
    ├── github.css
    ├── nord.css
    ├── sepia.css
    └── ...
```

The default `light` and `dark` themes remain application-owned code and do not appear as files. On the first successful catalog initialization, QingYu writes the 18 existing non-default themes into `themes/` as standard CSS theme files. The initialization marker is stored in application settings only after every seed file has been written successfully. Initialization is one-time: deleting a seeded theme must not cause it to reappear on the next launch.

All files in `themes/` use the same parser and validation path. Seeded and user-imported themes have the same behavior and may both be deleted. Only the two application defaults are protected.

### Rejected alternatives

- Keeping seeded themes in a read-only application resource directory would simplify theme updates, but would make those third-party themes undeletable or require a separate hidden-theme index.
- Saving imported CSS back into `settings.json` would be easy across runtimes, but would not allow a user or future sync service to place a file in the theme directory and have it discovered.
- Storing themes inside each note project would allow the existing project sync engine to transfer them, but would make a global appearance preference project-specific and pollute note folders.

## Scope

### Included

- A file-backed native theme catalog under `app_data_dir/themes`.
- Conversion and one-time seeding of all current non-default themes.
- A documented CSS metadata format.
- Desktop import, export-current, delete, refresh, and open-directory actions.
- Immediate theme selection and application.
- Separate saved light and dark theme IDs under the existing system/light/dark appearance mode.
- Desktop macOS, Windows, and Linux support.
- Mobile catalog scanning, third-party theme selection, deletion, and refresh.
- Mobile loading of a theme file that already exists in its application theme directory.
- Web fallback to the protected default light and default dark themes only.
- Migration of legacy fixed-theme selections and legacy custom CSS.
- Cross-window catalog and selection updates.
- Validation, duplicate handling, missing-theme fallback, and invalid-file diagnostics.

### Not included

- S3 or WebDAV synchronization of theme files or application settings.
- A remote theme marketplace or download service.
- Automatic continuous filesystem watching.
- Multiple files for one theme or a theme package containing arbitrary assets.
- A single CSS file containing both light and dark variants.
- Web theme import or browser-backed virtual theme directories.
- Mobile import, export, or file-manager directory opening in the first version.
- A built-in source-code editor for third-party CSS.
- Automatic updates for seeded third-party themes after their initial installation.

## Theme File Format

Each UTF-8 `.css` file represents exactly one light or dark theme. Its first metadata block must have this form:

```css
/*
@qingyu-theme
id: nord
name: Nord
appearance: dark
author: Arctic Studio
version: 1.0.0
preview-background: #2e3440
preview-panel: #3b4252
preview-text: #eceff4
preview-accent: #88c0d0
*/

:root {
  --bg-primary: #2e3440;
  --bg-secondary: #3b4252;
  --text-primary: #d8dee9;
  --text-heading: #eceff4;
  --accent: #88c0d0;
}

.markdown-paper {
  --editor-paper-bg: #2e3440;
  --editor-text-primary: #d8dee9;
}
```

Required fields are:

- `id`;
- `name`;
- `appearance`;
- `preview-background`;
- `preview-panel`;
- `preview-text`;
- `preview-accent`.

`author` and `version` are optional. Unknown metadata keys are retained as forward-compatible metadata only when they use a safe normalized key, but they do not affect behavior in this version.

Theme IDs must match `^[a-z0-9][a-z0-9-]{0,63}$`. The IDs `light` and `dark`, along with an application-reserved `qingyu-` prefix, cannot be used by third-party files. Theme names and authors may contain Unicode but must be single-line trimmed text with bounded lengths.

`appearance` must be `light` or `dark`. The four preview values must parse as non-transparent CSS colors. Metadata appearance is authoritative for catalog grouping, application `color-scheme`, Mermaid light/dark rendering, and fallback choice.

## CSS Capability and Safety

A third-party theme is full CSS rather than a token-only document. It may override public theme variables and use selectors such as `:root`, `.markdown-paper`, editor content selectors, CodeMirror selectors, or application component selectors. Only the active theme's CSS is injected, so files do not need to wrap every selector in a theme-ID selector.

The native validator uses a real CSS parser. It rejects:

- invalid CSS syntax;
- `@import`;
- HTTP, HTTPS, protocol-relative, or `file:` resource URLs;
- constructs that the selected parser cannot safely classify;
- non-UTF-8 input;
- files larger than 256 KiB.

The validator permits `data:` resources and same-document fragment URLs such as `url(#marker)`. Relative asset URLs are rejected because the first version does not import multi-file theme packages.

Full CSS can still hide controls or disrupt layout, so a normal React recovery button cannot be treated as a guaranteed escape path. The first activation of each third-party content fingerprint is a guarded preview: QingYu applies the CSS, then opens a system-native confirmation dialog asking whether to keep the theme. Canceling, closing, or failing to open that dialog restores the previous theme. Confirmation records the approved ID and fingerprint; selecting the same unchanged file later does not prompt again, while replacement content has a new fingerprint and requires a new confirmation. Selecting a protected default immediately removes the injected third-party style element.

## Theme Catalog Boundary

Add a focused native theme module and a matching runtime interface. It owns only the application theme directory and exposes semantic operations rather than arbitrary paths:

- `listThemes`;
- `readThemeCss`;
- `importTheme`;
- `replaceTheme`;
- `exportTheme`;
- `deleteTheme`;
- `refreshThemes`;
- `openThemeDirectory` when supported.

The renderer receives theme descriptors, never an unrestricted filesystem handle. A descriptor includes:

- `id`;
- `name`;
- `appearance`;
- optional `author` and `version`;
- four preview colors;
- normalized file name;
- content fingerprint;
- source kind: `default` or `third-party`;
- management capabilities.

Default descriptors are created by the application layer. Third-party descriptors come from the native scan. The catalog merges them into separate light and dark arrays with the matching default first and third-party themes sorted by locale-aware name, then ID for a deterministic tie-break.

The native service never follows symbolic links. It rejects nested paths, path traversal, non-regular files, unsupported extensions, and names that do not normalize to one safe file name. Writes use a temporary file in the theme directory, flush, and atomic rename. Replacement includes the expected old fingerprint so an external change cannot be silently overwritten.

## Selection and Application

Keep the existing three appearance modes:

- `system`;
- `light`;
- `dark`.

Settings store `lightThemeId` and `darkThemeId` as validated theme-ID strings. The existing Rust settings service validates ID syntax and the protected IDs but does not use a closed theme allowlist. Catalog existence is dynamic and is checked by the application theme controller.

The application resolves the active appearance, then the selected ID for that appearance:

1. Resolve system/light/dark appearance.
2. Find the selected ID in the matching catalog group.
3. If it is missing or invalid, select and persist the matching protected default.
4. For a protected default, remove any third-party style element.
5. For a third-party theme, read CSS by ID and expected fingerprint, then inject it into one owned style element.
6. For an unapproved fingerprint, show the native keep-or-revert confirmation before committing the selection.
7. Set root attributes for theme ID and resolved appearance. Give the Markdown paper the same theme ID and appearance.
8. Set `color-scheme` and Mermaid rendering from metadata appearance rather than a hard-coded theme-name list.

Async reads use a monotonically increasing application token. A late read for a previously selected theme cannot overwrite a newer selection. While a third-party file is loading, QingYu keeps the matching protected default appearance so it never flashes the opposite color scheme. The editor becomes theme-ready after the selected CSS has loaded or the controller has fallen back.

Visual selection remains immediate. A protected default or previously approved third-party fingerprint is persisted and broadcast after local state changes. An unapproved fingerprint remains a local preview until the native confirmation succeeds; only then is its ID persisted and broadcast. Reversion never leaks the preview selection to other windows. Approved ID/fingerprint pairs are local application settings and are removed when the file is deleted; replacement naturally requires approval because its fingerprint changes. A new catalog-changed event makes every open window rescan descriptors when a file is imported, replaced, or deleted.

## User Experience

The Appearance page keeps the current appearance-mode control. Replace the existing custom CSS text areas with a persistent theme toolbar and two independent theme sections.

The desktop toolbar always shows:

- `Import CSS`;
- `Export Current`;
- `Refresh Themes`;
- `Open Theme Folder`.

`Export Current` exports the theme currently active after resolving system appearance. The button or its tooltip names that theme. For a protected default, QingYu generates the canonical default CSS file and metadata. For a third-party theme, QingYu exports the original validated CSS bytes. Exporting never changes the internal catalog.

The light and dark sections do not pair cards by position or insert placeholders when counts differ. Each section independently renders a responsive grid:

- its protected default is always first;
- the selected third-party theme is pinned into the visible collapsed area;
- remaining third-party themes are name-sorted;
- the expanded form uses full name order after the default;
- a section that exceeds two responsive rows is collapsed behind `Show N more themes`;
- the settings page owns scrolling; theme sections do not add nested scroll containers.

Every card shows the four metadata preview colors, name, optional author, selected state, and a default or third-party label. Third-party cards expose delete through an explicit action rather than making the whole card destructive.

Mobile uses the same two-section selection interface in its Compact Appearance page. It supports selection, refresh, and deletion. Import, export, and open-directory controls are capability-gated off. Web shows only the two protected defaults.

## Import, Replace, Export, and Delete Flows

### Import

1. The desktop native file picker accepts one `.css` file.
2. Native code reads and validates the external file before writing anything.
3. If its ID is new, native code chooses a safe `<id>.css` target and atomically copies the original bytes.
4. The catalog-changed event triggers a rescan.
5. The new theme appears in the appropriate section but is not automatically selected.

### Duplicate ID and replacement

If a valid import has an existing third-party ID, the service returns a typed conflict with the existing descriptor and fingerprint. The UI asks whether to replace it. Confirmation calls `replaceTheme` with the expected fingerprint. A changed fingerprint fails closed and asks the user to retry after refresh.

Protected default IDs and reserved prefixes cannot reach the replacement prompt; they are rejected during validation.

After successful replacement, the catalog rescans. If that ID is currently active, its new CSS is read and applied immediately.

### Export

The save dialog proposes `<id>.css`. Default themes export application-generated canonical CSS; third-party themes export exact stored bytes. Cancellation is not an error. Export failures appear as a non-destructive inline or toast error and do not alter selection.

### Delete

Only third-party themes can be deleted. Deletion requires confirmation naming the theme. If the theme is selected in either saved appearance slot, deletion succeeds first and then that slot switches to the matching protected default, persists, and emits selection and catalog events. If native deletion fails, the file and selection remain unchanged.

Deletion is permanent in the first version because the file is inside an application-owned catalog, but the confirmation explains that the user should export a copy first if needed.

## Scanning and Error Handling

The catalog scans on:

- application startup;
- opening Appearance settings;
- successful import, replacement, or deletion;
- pressing `Refresh Themes`.

It does not continuously watch the directory. `Open Theme Folder` lets desktop users make external changes, and Refresh makes those changes visible without restarting.

Import validation failures do not write a file. Directory scans collect invalid files without exposing them as selectable themes. The Appearance page shows `N invalid theme files`; expanding it lists file name and a localized reason. Desktop also offers `Open Theme Folder` from the diagnostic. Invalid file contents are not injected or returned wholesale in error messages.

If multiple directory files declare the same ID, every colliding file is invalid for that scan. No arbitrary winner is selected. The diagnostic lists the conflict. This prevents filesystem enumeration order from changing behavior.

If an active third-party file is missing, invalid, unreadable, or changes identity during read, QingYu falls back to the protected default for that appearance, persists the repaired selection, and reports a recoverable warning. A failure in one theme never prevents valid themes or defaults from loading.

Directory creation or complete scan failure leaves both protected defaults usable. Theme management actions show Retry where appropriate and do not block the rest of settings navigation.

## Migration

Migration runs once before the new catalog becomes authoritative:

1. Create `app_data_dir/themes` without following symlinks.
2. Generate standard CSS files for the 18 current non-default themes, preserving their existing IDs so saved selections continue to resolve.
3. If either legacy selected theme is `custom`, generate a light or dark migrated theme file from the corresponding stored custom CSS. Use a unique ID such as `migrated-custom-light` or `migrated-custom-dark`, generate stable fallback preview colors, and select that ID.
4. Preserve `appearanceMode` and convert stored `lightTheme`/`darkTheme` into `lightThemeId`/`darkThemeId`.
5. Mark catalog initialization complete only after all required writes and settings updates succeed.
6. Stop reading legacy `customThemeCss`, `lightCustomThemeCss`, and `darkCustomThemeCss` after successful migration. Obsolete UI and event paths are removed.

If migration fails, QingYu continues with protected defaults and retries migration on the next launch. Partial seed files are safe because writes are content-validated and idempotent; the final marker is the commit point. Existing files with conflicting IDs are never silently overwritten.

Portable application-settings export stores appearance mode and selected theme IDs but does not embed third-party CSS. Importing settings with missing theme IDs falls back to protected defaults and reports which themes must be imported separately. Theme CSS remains portable through the always-visible theme export action.

## Code Organization

Keep responsibilities separated:

- Rust native theme module: owned directory resolution, parsing, validation, scanning, safe writes, export, delete, and typed errors.
- Desktop/mobile runtime adapter: narrow Tauri command mapping.
- Web runtime adapter: two protected default descriptors and unsupported management capabilities.
- Shared application theme catalog types: descriptors, preview values, validation result shapes, and runtime capability flags.
- `useThemeCatalog`: scan lifecycle, refresh, cross-window catalog events, and diagnostics.
- `useAppTheme`: appearance resolution, selected IDs, async CSS activation, fallback, and selection events.
- Appearance components: toolbar, independent theme grids, cards, expansion state, confirmations, and diagnostics.
- Theme assets: one CSS source file per seeded third-party theme, separate from the main application stylesheet.

Remove the closed TypeScript and Rust allowlists for non-default theme IDs. Keep the default theme IDs and appearance modes as closed constants. Move theme-specific root variables, editor variables, typography, GitHub table rules, and similar special cases out of `styles.css` into their corresponding seeded files. Base layout and public theme-token defaults remain in `styles.css`.

Document the format and public tokens in a theme-authoring guide. Internal component selectors are allowed but explicitly described as less stable than public variables and editor selectors.

## Testing and Verification

### Native tests

- Metadata parsing for required, optional, Unicode, duplicate, malformed, and oversized values.
- CSS syntax validation and rejection of imports and disallowed URL schemes.
- Acceptance of data URLs and fragment URLs.
- ID reservation, filename normalization, traversal rejection, symlink rejection, and regular-file enforcement.
- Deterministic scan ordering, duplicate-ID invalidation, and per-file error isolation.
- First-run seeding, idempotent retry, initialization commit point, and no restoration after user deletion.
- Atomic import and fingerprint-guarded replacement.
- Protected-default deletion rejection and third-party deletion behavior.
- Exact-byte third-party export and canonical default export.

### Application tests

- System/light/dark appearance resolution with dynamic theme IDs.
- Matching default fallback for missing, invalid, and unreadable third-party themes.
- Late async CSS reads cannot replace a newer selection.
- Active third-party CSS is the only injected catalog CSS.
- First activation of an unapproved fingerprint uses the native confirmation and reverts on cancel or dialog failure.
- Replacing a previously approved theme requires approval for the new fingerprint.
- Cross-window selection and catalog events refresh all windows.
- Metadata appearance controls `color-scheme` and Mermaid light/dark rendering.
- Legacy built-in and custom-theme migration.
- Portable settings import reports missing external theme files.

### Component tests

- Persistent desktop toolbar actions.
- Independent light/dark grids with unequal counts.
- Responsive two-row collapse, selected-theme visibility, and expansion.
- Default-first and deterministic third-party ordering.
- Duplicate replacement confirmation and stale-fingerprint retry.
- Invalid-file diagnostics and recovery actions.
- Active-theme deletion fallback and failed deletion preservation.
- Mobile capability gating while retaining selection, refresh, and deletion.
- Web default-only behavior.

### Repository verification

Run the smallest focused tests while implementing each unit, then complete the repository gates:

```sh
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
pnpm test
pnpm typecheck:test
pnpm build
```

Live S3 tests are unnecessary because this design intentionally does not change project synchronization.

## Success Criteria

1. A fresh installation offers the protected default light and dark themes plus all converted existing third-party themes.
2. A desktop user can import a valid CSS theme, see it in the correct section, select it immediately, export it, replace it by ID, delete it, and refresh the directory.
3. Copying a valid CSS file directly into the owned directory makes it available after startup, opening Appearance settings, or Refresh.
4. Unequal or large light/dark catalogs remain understandable and do not introduce nested scrolling.
5. Invalid, duplicate, missing, or unsafe theme files never execute and never make protected defaults unavailable.
6. Mobile can load and select third-party themes already present in its directory without implementing import or export.
7. Web remains functional with the two protected defaults.
8. Existing non-default selections keep the same appearance after migration, and legacy custom CSS is preserved as migrated theme files.
9. Theme files and selected application appearance remain local; existing S3/WebDAV project-sync behavior is unchanged.
