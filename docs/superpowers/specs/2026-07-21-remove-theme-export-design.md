# Remove Theme Export Design

## Goal

Remove QingYu's theme-export feature completely while preserving theme import, direct-directory authoring, discovery, activation, replacement, deletion, and `.theme` package validation.

## Considered approaches

1. Hide the export button only. This is the smallest UI change, but it leaves an undocumented native command and archive-writing surface behind.
2. Remove public UI/runtime access but keep the internal archive writer. This reduces product exposure, but preserves dead production code and its security-sensitive file-publication path.
3. Remove the feature end to end. This removes the UI, runtime contracts, native command, archive writer, export-only tests, translations, and current documentation. This is the selected approach because the request is to delete the feature rather than disable it.

## Product behavior

- Desktop Appearance settings continue to show Import theme, Refresh themes, and Open theme folder.
- Compact/mobile Appearance settings continue to show Refresh and theme selection/deletion according to their existing capabilities.
- No platform exposes an Export current action or an export capability flag.
- `.css` and `.theme` imports continue to work unchanged.
- Users can keep authoring unpacked theme directories. Distributable `.theme` files remain ordinary externally-created ZIP archives with the `.theme` extension.

## Code boundaries

The shared `AppThemeRuntime` contract loses `exportCurrent`, and `ThemeRuntimeCapabilities` loses `canExport`. The catalog hook and toolbar no longer accept or expose export callbacks. Desktop and mobile runtimes adopt the smaller shared contract.

The Tauri `export_theme_file` command and its registration are removed. `ThemeCatalog::export`, `ThemePackageExport`, the ZIP writer, atomic output publication helpers, protected-default export generation, and their export-only tests are deleted. Archive parsing, extraction, staging, validation, and import test fixtures remain.

Current product documentation and locale keys are updated. Historical design specifications and implementation plans remain unchanged as project history.

## Error and security behavior

Removing export eliminates the native save dialog and all writes outside the application-owned theme directory. Import validation, path normalization, package limits, fingerprint checks, activation confirmation, and atomic catalog replacement retain their current behavior.

## Verification

- Component tests prove desktop has import/refresh/open-folder controls and no export action.
- Runtime tests prove the shared and desktop/mobile capability contracts contain no export member.
- Tauri boundary tests prove `export_theme_file` is absent from registered commands.
- Rust theme tests prove `.css` and `.theme` imports, replacements, activation, deletion, and archive validation still pass.
- Repository gates run `cargo fmt --check`, `cargo test`, `pnpm test`, `pnpm typecheck:test`, and `pnpm build`.
