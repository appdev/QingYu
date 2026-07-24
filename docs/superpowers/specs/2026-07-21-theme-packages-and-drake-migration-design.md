# QingYu Resource Theme Packages and Drake Migration Design

Date: 2026-07-21

## Purpose

QingYu currently treats each third-party theme as one UTF-8 CSS file. That works for color and layout rules, but it cannot faithfully carry themes such as Drake that include fonts, icons, images, and license files. This design adds a portable `.theme` package format while preserving existing single-file CSS themes, then uses that format to migrate Drake Light and Drake Ayu.

Theme synchronization through S3 or WebDAV remains out of scope. Mobile continues to load installed themes but does not expose import, export, or open-folder controls.

## Confirmed product decisions

- Keep the protected default light and dark themes unchanged.
- Keep existing `.css` themes supported without forced migration.
- Add `.theme` as the resource-bearing transport format.
- A `.theme` file is a standard ZIP archive with a short product-specific extension.
- Authors develop themes as normal unpacked directories. Packaging is a distribution step, not an authoring requirement.
- One package represents one light or dark theme, matching the current selection and “export current theme” behavior.
- Import accepts `.css` and `.theme`.
- Export always emits the current theme as `.theme`, including every required asset and license file.
- Drake is delivered as two independent themes: Drake Light and Drake Ayu.
- Drake uses its supplied WOFF2 files directly. Fonts are not converted to Base64.
- Existing catalogs receive only the new Drake themes during the catalog upgrade. Previously deleted seeds are not restored and user themes are not overwritten.

## Alternatives considered

### Palette-only CSS migration

This keeps the existing format but loses Drake typography, icons, and other resource-backed styling. It does not solve the general resource problem.

### CSS plus a global application asset directory

This keeps CSS small, but exported themes depend on files installed by a particular QingYu build. User-authored themes cannot reliably carry their own resources, and moving a CSS file alone produces an incomplete theme.

### Resource theme packages

This is the selected approach. It keeps themes portable, permits direct relative WOFF2 and image references, supports normal folder-based development, and preserves the existing CSS path for simple themes.

## Package format

The transport file uses the `.theme` extension and ZIP encoding. Every archive contains exactly one theme rooted directly in the archive, not inside an extra wrapper directory.

```text
drake-light.theme
├── manifest.json
├── theme.css
├── assets/
│   ├── fonts/
│   │   ├── JetBrainsMono-Regular.woff2
│   │   ├── JetBrainsMono-Bold.woff2
│   │   ├── JetBrainsMono-Italic.woff2
│   │   └── JetBrainsMono-BoldItalic.woff2
│   └── icons/
│       └── checkbox.svg
└── licenses/
    ├── THEME-LICENSE.txt
    └── FONT-LICENSE.txt
```

The version 1 manifest shape is:

```json
{
  "schemaVersion": 1,
  "id": "drake-light",
  "name": "Drake Light",
  "appearance": "light",
  "entry": "theme.css",
  "author": "liangjingkanji",
  "version": "2.9.6",
  "preview": {
    "background": "#ffffff",
    "panel": "#f6f8fa",
    "text": "#333333",
    "accent": "#e95f59"
  },
  "licenseFiles": [
    "licenses/THEME-LICENSE.txt",
    "licenses/FONT-LICENSE.txt"
  ]
}
```

Required fields are `schemaVersion`, `id`, `name`, `appearance`, `entry`, and all preview colors. `author` and `version` are optional. `licenseFiles` is required when the package contains fonts and otherwise optional. Unknown manifest fields are rejected in schema version 1 so misspellings do not silently change package behavior.

Theme IDs retain the current lowercase ASCII rules and reserved values. The entry path must be `theme.css` in version 1. Package CSS may reference resources only below `./assets/`.

## Authoring workflow

Authors work in an unpacked directory with the same layout as the archive. QingYu scans both root-level legacy CSS files and unpacked theme directories under the app theme directory.

```text
themes/
├── github.css
├── my-theme/
│   ├── manifest.json
│   ├── theme.css
│   ├── assets/
│   └── licenses/
└── drake-ayu/
    ├── manifest.json
    ├── theme.css
    ├── assets/
    └── licenses/
```

An author can copy a starter directory into this location, edit ordinary CSS and assets, then press Refresh themes. The same validator used for package installation validates unpacked directories. No compression is needed during development.

The product documentation includes a minimal directory template, manifest reference, stable CSS variables, supported resources, security rules, and packaging instructions. A later CLI can expose `qingyu theme validate <directory>` and `qingyu theme pack <directory>`, but the initial implementation must not depend on that CLI. The application provides the pack/export operation needed to create a `.theme` file.

## Catalog storage and discovery

Installed packages are extracted to `themes/<theme-id>/`. The original archive is not retained. Legacy `.css` files remain at the theme root and continue using the existing parser and lifecycle operations.

Catalog scanning returns one unified descriptor list for:

- protected default themes;
- valid root-level CSS themes;
- valid unpacked theme directories.

Invalid CSS files and invalid directories both appear in catalog diagnostics with the failing file and a safe reason. The catalog never follows symbolic links.

Package descriptors use the same light/dark grouping, selected-theme settings, preview model, and first-activation confirmation as legacy CSS descriptors. The descriptor gains a storage kind so native and frontend code can distinguish inline CSS from an installed resource directory without inferring it from paths.

## Installation and replacement

Importing a `.theme` archive follows this sequence:

1. Open the source as a regular file without following symbolic links.
2. Verify ZIP structure and bounded archive metadata.
3. Extract into a unique temporary directory below the managed theme root.
4. Validate paths, manifest, entry CSS, every resource, and every CSS resource reference.
5. Compute the canonical content fingerprint.
6. Detect ID and destination conflicts.
7. For a new theme, atomically rename the validated temporary directory to `themes/<theme-id>/`.
8. For an approved replacement, retain the existing version until the new directory is fully validated, then atomically exchange it or restore the old version on failure.
9. Remove temporary and retained staging directories after success or failure.

Import never installs a partial theme. Replacing a theme requires the expected fingerprint of the installed version. A conflicting ID triggers the existing replacement confirmation rather than silent overwrite.

Directly placed unpacked directories are read-only from the scanner’s perspective until the user invokes an explicit lifecycle action. Refreshing must not rewrite author files.

## Archive and path validation

Version 1 applies these limits:

- maximum source archive size: 16 MiB;
- maximum total uncompressed size: 32 MiB;
- maximum entry count: 256;
- maximum normalized relative path length: 240 Unicode scalar values;
- maximum directory nesting depth: 16;
- maximum `manifest.json` size: 64 KiB;
- maximum entry CSS size: 256 KiB;
- maximum individual WOFF2 size: 4 MiB;
- maximum individual raster image or SVG size: 8 MiB.

The validator rejects:

- absolute paths, drive prefixes, UNC prefixes, empty path segments, `.` or `..` segments, NUL bytes, and non-UTF-8 names;
- symbolic links, hard links, devices, FIFOs, sockets, and nested archives;
- duplicate normalized paths and case-folded path collisions;
- encrypted entries or unsupported compression methods;
- files outside the approved manifest, CSS, asset, and license locations;
- any archive whose declared or streamed output crosses a limit.

Extraction creates files with new-file semantics and never writes through an existing path. The final destination remains below the canonical managed theme root throughout installation.

## CSS and resource validation

Package CSS reuses the existing CSS syntax and URL validation after separating that validation from the legacy CSS metadata-comment parser. Package identity and preview metadata come only from `manifest.json`; legacy CSS continues to require its current first-block metadata comment. Both forms reject invalid CSS and `@import`.

Allowed `url(...)` forms are:

- package-relative references beginning with `./assets/`;
- same-document fragments such as `url(#marker)`;
- bounded `data:` resources.

Remote URLs, protocol-relative URLs, `file:` URLs, absolute paths, parent traversal, and paths outside `assets/` are rejected. Every referenced package resource must exist as a regular validated file. Resource paths are resolved after percent-decoding and Unicode normalization so encoded traversal and ambiguous aliases cannot bypass checks.

Version 1 resource types are:

- fonts: `.woff2`;
- raster images: `.png`, `.jpg`, `.jpeg`, `.webp`, `.gif`;
- vector images and icons: `.svg`;
- human-readable licenses: `.txt`, `.md` below `licenses/`.

Magic bytes and declared extensions must agree for WOFF2 and raster images. SVG is parsed as XML and rejects active content, event-handler attributes, foreign objects, embedded HTML, scripts, external references, non-fragment links, and unsafe CSS URLs. License files are never exposed as renderable theme assets.

Packages containing fonts must declare at least one existing license file. QingYu validates the presence and readability of the declaration; legal accuracy remains the theme distributor’s responsibility.

## Fingerprints

Package fingerprints are content fingerprints, not raw ZIP-byte hashes. The native layer hashes a deterministic stream containing:

- the normalized manifest representation;
- each normalized relative path in sorted order;
- each file length and file bytes.

Different ZIP compression settings therefore produce the same fingerprint for identical theme content. Changing CSS, a font, an icon, an image, the manifest, or a license file changes the fingerprint and requires activation confirmation again.

Legacy CSS fingerprints remain byte hashes to preserve existing behavior.

## Runtime loading

Legacy CSS themes continue to be read and injected into the existing managed `<style>` element.

Resource themes are loaded through a managed `<link rel="stylesheet">` pointing to the real installed `theme.css`. Before exposing the URL, the native runtime grants recursive asset-protocol access only to the validated active theme directory. Relative font and image URLs then resolve naturally from the stylesheet location.

Activation waits for the new link’s `load` event before marking the theme ready. An `error` event, missing resource, stale fingerprint, or permission failure removes the candidate link, revokes its directory permission, and falls back to the protected default theme for the same appearance.

Only the latest activation token may commit. Rapid selection changes cannot allow an older link load to replace the newest selection. After a successful switch, the previous stylesheet is removed and its directory is forbidden from the asset scope. Deleting a resource theme also revokes its directory before removing files.

The runtime theme API returns an explicit activation source:

- inline CSS and fingerprint for legacy themes;
- a validated local stylesheet path and fingerprint for resource themes.

The desktop and mobile adapters convert native file paths to platform-correct asset URLs. The web runtime continues to expose protected defaults only.

## User interface behavior

Desktop import accepts `.css` and `.theme`. Export current always writes a `.theme` file:

- resource themes export their manifest, entry CSS, assets, and licenses;
- legacy CSS themes export a generated version 1 manifest plus `theme.css` containing the original validated bytes;
- protected defaults export a generated package containing their canonical starter CSS.

The theme toolbar remains visible. Refresh discovers root CSS files and unpacked directories. Open theme folder exposes the authoring location. Delete removes the complete installed theme directory or the legacy CSS file after fingerprint validation.

The light and dark sections remain independent and support unequal theme counts. Drake Light appears in the light section and Drake Ayu in the dark section. Both can be selected together under Follow system.

Mobile loads installed legacy and resource themes and supports refresh and deletion. It does not show import, export, or open-folder actions.

## Catalog upgrade and bundled Drake delivery

The bundled seed catalog gains Drake Light and Drake Ayu as resource theme directories with their original four WOFF2 files and required licenses.

Catalog initialization distinguishes a fresh catalog from an upgrade:

- fresh catalogs install all currently bundled seed themes;
- version 1 catalogs install only the two new Drake theme directories and then advance the catalog version;
- a user theme with the same ID wins and is never overwritten;
- an occupied destination produces a catalog diagnostic but does not prevent application startup;
- themes the user deleted from the original seed set are not restored;
- current appearance and selected theme IDs are preserved.

Future bundled additions use the same versioned incremental manifest rather than rerunning the entire initial seed set.

## Drake adaptation

The source package at `/Users/ying/Downloads/DrakeTyporaTheme-2.9.6` contains two actual source stylesheets: `drake-light.css` and `drake-ayu.css`. Other named variants in that archive are previews without corresponding CSS and are not reconstructed.

The migration is semantic rather than a literal Typora selector copy. It preserves:

- the light white, dark blue-gray, coral, and Ayu gold palettes;
- the supplied patched JetBrains Mono regular, bold, italic, and bold-italic fonts;
- centered first-level headings and Drake heading hierarchy;
- emphasized second-level headings;
- accent-colored links and blockquotes;
- outlined pill-style highlights;
- compact code blocks and adapted syntax colors;
- striped tables, keyboard styling, task states, and compatible icons.

Typora-only panels, menus, CodeMirror selectors, export hooks, and unsupported directives are omitted. QingYu application tokens style the surrounding chrome, while `.markdown-paper`, source editor, Mermaid, and stable QingYu editor selectors implement the content appearance. The font is applied to the application root, Markdown body, headings, source editor, and code surfaces, with a system monospace fallback only if the packaged WOFF2 load fails.

The theme CSS retains the Drake MIT notice. The package includes the theme license and the JetBrains Mono Patch OFL 1.1 license as distinct files.

## Failure behavior

- Invalid packages report a safe file-specific reason and leave no installed residue.
- Replacement failures leave the previous valid package and selected theme intact.
- Missing or modified resources discovered during activation invalidate the fingerprint, block activation, and fall back to the protected default for that appearance.
- An invalid unpacked directory appears in diagnostics but does not block valid themes.
- Asset permission failures never broaden scope to the full theme root or app-data directory.
- Export writes to a temporary sibling and publishes the final archive only after ZIP creation and validation succeed.

## Verification

Native tests cover:

- manifest parsing and closed schema validation;
- valid package import, replacement, deletion, and export/reimport round trips;
- deterministic content fingerprints across ZIP encodings;
- CSS resource resolution and missing-resource rejection;
- WOFF2 and image signature validation;
- unsafe SVG rejection;
- path traversal, encoded traversal, symlinks, hard links, duplicate paths, case collisions, encryption, unsupported compression, and archive limits;
- atomic installation and rollback after injected failures;
- catalog v1-to-v2 Drake installation without restoring deleted seeds or replacing user themes;
- scanning root CSS themes and unpacked directory themes together.

Frontend tests cover:

- inline legacy CSS activation;
- link-based resource theme activation and load completion;
- load failure fallback and error reporting;
- activation race cancellation;
- old link removal and native scope revocation;
- `.css` and `.theme` desktop import filters;
- current-theme package export;
- mobile resource-theme selection with import/export controls absent.

Live verification uses an isolated Debug application identity. It imports both Drake packages, selects each appearance, verifies that the stylesheet and all four WOFF2 faces load through `document.fonts`, and visually checks headings, paragraphs, emphasis, links, lists, tasks, blockquotes, highlights, tables, inline code, fenced code, Mermaid, images, and icons. The final repository gates are:

```text
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
pnpm test
pnpm typecheck:test
pnpm build
```

## Non-goals

- Theme synchronization through S3 or WebDAV.
- Network-hosted theme resources.
- Executable theme scripts or HTML components.
- Multiple independently selectable themes inside one `.theme` archive.
- Reconstructing Drake variants for which the supplied archive contains no CSS.
- A public theme marketplace or automatic remote updates.
