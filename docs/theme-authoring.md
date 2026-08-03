# 轻语主题制作指南

轻语 supports two third-party theme forms:

- A single UTF-8 `.css` file is enough when the theme needs only CSS rules, CSS variables, same-document fragments, or small passive base64 `data:` resources. It cannot carry a local font, image, icon, or license beside the CSS.
- An unpacked resource-theme directory is the authoring format when the theme has fonts, images, icons, or license files. For distribution, its root contents can be packaged externally as a portable `.theme` archive.

One CSS file, resource directory, or `.theme` package represents exactly one light-appearance or dark-appearance theme. 轻语 provides four frontend-owned built-ins: the default `light` / `dark` pair and the original neutral `classic-light` / `classic-dark` pair. Users may select all four, but cannot delete, replace, or shadow them through third-party theme operations. These built-ins never enter the native theme directory or its scan/import pipeline.

For a worked explanation of the defaults—including palette, typography, sidebar states, real font-weight mapping, frontend ownership, and the decisions that should or should not be copied—see [轻语默认臻楷主题设计](default-theme-zhenkai.md).

## Single-file CSS themes

A root-level `.css` theme begins with this metadata block. The required fields are `id`, `name`, `appearance`, `preview-background`, `preview-panel`, `preview-text`, and `preview-accent`. `author` and `version` are optional.

```css
/*
@qingyu-theme
id: ocean-night
name: Ocean Night
appearance: dark
author: Example Author
version: 1.0.0
preview-background: #101820
preview-panel: #172630
preview-text: #e8f1f5
preview-accent: #52c7c7
*/

:root[data-theme="ocean-night"] {
  --bg-primary: #101820;
  --bg-secondary: #172630;
  --text-primary: #d8e5e9;
  --text-heading: #f5fbfc;
  --accent: #52c7c7;
  color-scheme: dark;
}

.markdown-paper[data-editor-theme="ocean-night"] {
  --editor-paper-bg: #101820;
  --editor-text-primary: #d8e5e9;
  --editor-text-heading: #f5fbfc;
}
```

The file limit is 256 KiB. IDs must match `^[a-z0-9][a-z0-9-]{0,63}$`; `light`, `dark`, `classic-light`, `classic-dark`, and every ID beginning with the internal `qingyu-` namespace are reserved. Names and authors are limited to 120 Unicode scalar values, versions to 64, and preview values must be valid non-transparent CSS colors. Single-file CSS rejects `@import` and every relative or external URL. Same-document fragments such as `url(#marker)` and bounded base64 data for PNG, JPEG, GIF, WebP, or WOFF2 content are accepted; HTML, XML, SVG, unknown MIME types, and non-base64 data are rejected.

## Resource-theme directory

Authors work with ordinary files in a directory. A full directory can look exactly like this:

```text
ocean-night/
├── manifest.json
├── theme.css
├── assets/
│   ├── fonts/
│   │   └── example.woff2
│   ├── icons/
│   │   └── checkbox.svg
│   └── images/
│       └── background.webp
└── licenses/
    ├── THEME-LICENSE.txt
    └── FONT-LICENSE.txt
```

`manifest.json` and `theme.css` must be directly at the theme root. `assets/` and `licenses/` are optional when they are not needed; other root entries are not allowed. Directories below those two roots may be organized freely within the limits below.

### Closed version 1 manifest

Version 1 accepts only these fields. Unknown fields, including unknown fields inside `preview`, are rejected so a misspelling cannot silently change a theme. The required root fields are `schemaVersion`, `id`, `name`, `appearance`, `entry`, and `preview`. The `preview` object must contain all four required fields: `background`, `panel` (surface), `text` (foreground), and `accent`. `author` and `version` are optional; `licenseFiles` becomes required when the theme contains a font.

| Field | Required | Rule |
| --- | --- | --- |
| `schemaVersion` | Yes | Integer `1`. |
| `id` | Yes | Lowercase ASCII ID matching `^[a-z0-9][a-z0-9-]{0,63}$`; reserved IDs are not allowed. |
| `name` | Yes | Non-empty after trimming and Unicode normalization; at most 120 Unicode scalar values. |
| `appearance` | Yes | Exactly `light` or `dark`. |
| `entry` | Yes | Exactly `theme.css` in version 1. |
| `author` | No | If present, non-empty and at most 120 Unicode scalar values. |
| `version` | No | If present, non-empty and at most 64 Unicode scalar values. |
| `preview` | Yes | Exactly `background`, `panel` (surface), `text` (foreground), and `accent`; all four are required and each must be a valid non-transparent CSS color. |
| `licenseFiles` | With fonts | An optional array of unique `.txt` or `.md` paths below `licenses/`; at least one existing entry is required when any WOFF2 font is present. |

A complete manifest for a font-bearing theme is:

```json
{
  "schemaVersion": 1,
  "id": "ocean-night",
  "name": "Ocean Night",
  "appearance": "dark",
  "entry": "theme.css",
  "author": "Example Author",
  "version": "1.0.0",
  "preview": {
    "background": "#101820",
    "panel": "#172630",
    "text": "#e8f1f5",
    "accent": "#52c7c7"
  },
  "licenseFiles": [
    "licenses/THEME-LICENSE.txt",
    "licenses/FONT-LICENSE.txt"
  ]
}
```

Every declared license must exist and contain UTF-8 text. A package containing a font must declare at least one existing UTF-8 license. 轻语 checks the declaration and readability, while the distributor remains responsible for having the legal right to redistribute every bundled resource.

### Smallest copyable starter

For a resource theme without assets, create a directory containing only these two files.

`manifest.json`:

```json
{
  "schemaVersion": 1,
  "id": "ocean-night",
  "name": "Ocean Night",
  "appearance": "dark",
  "entry": "theme.css",
  "preview": {
    "background": "#101820",
    "panel": "#172630",
    "text": "#e8f1f5",
    "accent": "#52c7c7"
  }
}
```

`theme.css`:

```css
:root[data-theme="ocean-night"] {
  --bg-primary: #101820;
  --bg-secondary: #172630;
  --bg-code: #0d151b;
  --text-primary: #d8e5e9;
  --text-heading: #f5fbfc;
  --text-secondary: #9fb0b7;
  --border-default: #29404d;
  --accent: #52c7c7;
  --link-color: #65dada;
  color-scheme: dark;
}

.markdown-paper[data-editor-theme="ocean-night"] {
  --editor-paper-bg: #101820;
  --editor-text-primary: #d8e5e9;
  --editor-text-heading: #f5fbfc;
  --editor-text-secondary: #9fb0b7;
  --editor-link-color: #65dada;
}
```

## Stable styling surface

Only the active third-party stylesheet is loaded. Unscoped `:root`, `.markdown-paper`, and `.markdown-source-paper` rules therefore work, but ID-scoped rules are recommended:

```css
:root[data-theme="ocean-night"] { /* application chrome */ }
.markdown-paper[data-editor-theme="ocean-night"] { /* visual editor */ }
:root[data-theme="ocean-night"] .markdown-source-paper { /* source editor */ }
```

These root/editor selectors and the variables below are the stable theme surface. Standard Markdown elements below `.markdown-paper`, such as headings, paragraphs, links, lists, blockquotes, tables, `code`, `pre`, `mark`, `kbd`, and images, may also be styled. Editor-internal `.markra-*`, `.ProseMirror`, `.cm-*`, and Milkdown selectors remain available when they are descendants of the selected theme's `.markdown-paper` or `.markdown-source-paper`, but they are less stable and may change with the editor implementation.

### CSS safety boundary

The validator preserves ordinary theme authoring while preventing a stylesheet from taking over application controls. It applies the following boundaries to both single-file CSS and resource-theme CSS:

- Rules may target the active theme root, the active theme's Markdown or source editor, and descendants of those editor surfaces. Global selectors and unscoped application selectors such as `*`, `button`, `[aria-label]`, or settings/sidebar internals are rejected.
- Descendants of an ID-scoped application root may receive non-interactive paint declarations such as colors, backgrounds, borders, shadows, `fill`, and `stroke`. Layout, visibility, and interaction changes on application controls are rejected.
- Markdown descendants keep normal CSS for typography, fonts, colors, backgrounds, borders, spacing, tables, code blocks, generated content, packaged assets, and theme-specific custom variables.
- Declarations that directly remove the editor from layout, escape it into a viewport overlay, intercept clicks, or turn an app region into a native drag surface are rejected. This includes `display`, `visibility`, `opacity`, `pointer-events`, viewport positioning/insets, `z-index`, transforms, animation, selection/touch interception, and `-webkit-app-region`. `position: static` and `position: relative` remain available.
- `@font-face` is supported. `@media`, `@supports`, `@container`, and block-form `@layer` are supported, and every nested rule is checked against the same scope. `@import`, `@keyframes`, `@property`, and other global at-rules are rejected.
- CSS escapes and comments cannot be used to disguise selector or property names from validation.

CSS is treated as presentation data: it is never evaluated as JavaScript and receives no file-operation API. Resource activation grants the asset protocol access only to the validated, application-owned copy of that theme package. Rejected imports are cleaned from staging without rewriting the source package or files outside the theme catalog.

Themes should set the following application-chrome variables on their ID-scoped `:root` selector. Themes should not target sidebar internal classes. Every variable is optional and may be omitted safely; 轻语 then uses the documented fallback.

| Variable | Fallback |
| --- | --- |
| `--bg-titlebar` | Windows top app chrome `--bg-chrome`; macOS and document titlebars `--bg-primary` |
| `--bg-sidebar` | Windows `--bg-chrome`; other platforms `--bg-secondary` |
| `--bg-sidebar-header` | `--bg-sidebar`, then the platform sidebar fallback |
| `--bg-toolbar` | `--bg-sidebar`, then the platform sidebar fallback |
| `--bg-outline` | `--bg-sidebar`, then the platform sidebar fallback |
| `--bg-sidebar-footer` | `--bg-sidebar`, then the platform sidebar fallback |
| `--border-chrome` | `--border-default` |
| `--bg-tree-current` | `--bg-active` |
| `--text-tree-current` | `--text-heading` |
| `--tree-current-indicator` | `--text-secondary` |
| `--bg-tree-selected` | `--accent-soft` |
| `--text-tree-selected` | `--text-heading` |
| `--tree-selected-indicator` | `--accent` |
| `--bg-outline-current` | `--bg-active` |
| `--text-outline-current` | `--text-heading` |

Sidebar typography and outline rhythm are also optional. They are consumed by application-owned semantic classes, so themes can adapt the sidebar without targeting its internal component selectors. When omitted, these variables preserve 轻语's current compact sidebar behavior.

| Variable | Fallback |
| --- | --- |
| `--sidebar-font-family` | Inherit the active theme's root/application font |
| `--sidebar-tree-font-size` | `13px` |
| `--sidebar-tree-font-weight` | `400` |
| `--sidebar-tree-line-height` | `20px` |
| `--text-tree` | `--text-secondary` |
| `--bg-tree-hover` | `--bg-hover` |
| `--text-tree-hover` | `--text-heading` |
| `--sidebar-tree-current-font-weight` | `--sidebar-tree-font-weight`, then `400` |
| `--sidebar-outline-font-size` | `13px` |
| `--sidebar-outline-font-weight` | `400` |
| `--sidebar-outline-line-height` | `20px` |
| `--sidebar-outline-row-min-height` | `28px` |
| `--sidebar-outline-max-lines` | `1` |
| `--text-outline` | `--text-secondary` |
| `--bg-outline-hover` | `--bg-hover` |
| `--text-outline-hover` | `--text-heading` |
| `--sidebar-outline-current-font-weight` | `620` |
| `--sidebar-outline-h1-*` through `--sidebar-outline-h6-*` | The common outline value, with `0px` for spacing |

Each per-level outline prefix supports `font-size`, `font-weight`, `text`, and `space-before`, for example `--sidebar-outline-h2-font-weight` and `--sidebar-outline-h2-space-before`. The first visible outline row never receives extra leading space. `--sidebar-outline-max-lines` controls APP-owned line clamping; use a small positive integer such as `1` or `2`.

Document tabs expose a small presentation-only contract. Short tabs remain content-sized; the maximum values only affect titles long enough to reach them. Themes may override these variables on `:root` without targeting the tab component's internal selectors.

| Variable | Default |
| --- | --- |
| `--document-tab-min-width` | `88px` |
| `--document-tab-max-width` | `192px` |
| `--document-tab-active-max-width` | `min(336px, 45vw)` |

The single-row layout, hidden scrollbar, horizontal wheel handling, active-tab visibility, fixed controls, focus behavior, and all-tabs menu are application-owned behavior. Themes must not override those structural rules.

Application variables:

- backgrounds: `--bg-primary`, `--bg-secondary`, `--bg-chrome`, `--bg-code`, `--bg-hover`, `--bg-active`;
- text: `--text-primary`, `--text-heading`, `--text-secondary`, `--text-md-char`;
- borders and actions: `--border-default`, `--border-strong`, `--accent`, `--accent-soft`, `--accent-hover`, `--danger`, `--link-color`;
- scrollbars and floating surfaces: `--scrollbar-track`, `--scrollbar-thumb`, `--scrollbar-thumb-hover`, `--scrollbar-thumb-active`, `--floating-menu-shadow`, `--floating-popover-shadow`.

Visual-editor variables on `.markdown-paper`:

- surfaces and text: `--editor-paper-bg`, `--editor-text-primary`, `--editor-text-heading`, `--editor-text-secondary`, `--editor-bg-secondary`;
- borders: `--editor-border`, `--editor-border-strong`;
- code: `--editor-inline-code-bg`, `--editor-inline-code-text`, `--editor-code-bg`, `--editor-code-line-bg`, `--editor-code-text`, `--editor-code-control-bg`;
- links and syntax: `--editor-link-color`, `--editor-hl-keyword`, `--editor-hl-string`, `--editor-hl-number`, `--editor-hl-title`, `--editor-hl-type`, `--editor-hl-meta`, `--editor-hl-symbol`, `--editor-hl-deletion`;
- typography families: `--editor-font-family`, `--editor-heading-font-family`;
- heading defaults: `--editor-heading-font-weight`, `--editor-heading-letter-spacing`;
- per-level heading values: `--editor-h1-color` through `--editor-h6-color`, `--editor-h1-font-size` through `--editor-h6-font-size`, `--editor-h1-font-weight` through `--editor-h6-font-weight`, `--editor-h1-letter-spacing` through `--editor-h6-letter-spacing`, and `--editor-h1-line-height` through `--editor-h6-line-height`;
- compact heading sizes: `--editor-h1-font-size-compact` and `--editor-h2-font-size-compact`, used by 轻语 below its compact editor breakpoint;
- caret: `--editor-caret-color`.

Every per-level letter-spacing variable falls back to `--editor-heading-letter-spacing`. A theme can therefore set one common value, then override only the levels that need a different rhythm:

```css
.markdown-paper[data-editor-theme="ocean-night"] {
  --editor-heading-font-weight: 700;
  --editor-heading-letter-spacing: 0;

  --editor-h1-color: #8bd5ca;
  --editor-h1-font-size: 2rem;
  --editor-h1-font-size-compact: 1.75rem;
  --editor-h1-font-weight: var(--editor-heading-font-weight);
  --editor-h1-letter-spacing: 0.04em;
  --editor-h1-line-height: 1.5;

  --editor-h2-color: #a6da95;
  --editor-h2-font-size: 1.5rem;
  --editor-h2-font-size-compact: 1.375rem;
  --editor-h2-font-weight: var(--editor-heading-font-weight);
  --editor-h2-letter-spacing: 0.03em;
  --editor-h2-line-height: 1.6;
}
```

轻语 consumes the same per-level tokens for CodeMirror live headings and rendered Markdown headings. Do not duplicate hard-coded heading values in `.cm-markra-*` selectors unless the theme knowingly accepts an editor-internal compatibility risk.

The visual-editor font variables are theme-controlled while the editor font preference is set to use the theme; an explicitly selected user font is applied inline and wins. The source editor follows the same rule for `--source-editor-font-family` on `.markdown-source-paper`. Its colors inherit the application text, secondary-text, and accent variables. Paragraph spacing is always applied inline from the user's editor preference, so `--editor-paragraph-spacing` is not an author-controlled theme token.

### Capability and precedence model

Theme values are resolved in this order:

1. An explicit user preference wins for editor font, source-editor font, font size, line-height preference, content width, and paragraph spacing where 轻语 applies that preference inline.
2. The selected theme supplies stable semantic variables and Markdown presentation rules.
3. 轻语's base stylesheet supplies documented fallbacks for anything omitted.

The stable theme surface can change paint, type, and Markdown presentation without changing product behavior:

| Area | Stable capability | Boundary |
| --- | --- | --- |
| Application chrome | Backgrounds, foregrounds, borders, scrollbars, shadows, titlebar/sidebar/tool/outline/footer surfaces | Themes cannot reorder, hide, resize, or intercept application controls. |
| File tree | Normal, hover, current, multi-selected, indicators, font family, size, weight, and line height | Tree indentation, disclosure behavior, drag/drop, row ownership, and selection logic remain application-owned. |
| Outline | Normal, hover, current, common rhythm, per-heading-level size/weight/text/leading space, and line clamp | Heading extraction, navigation, nesting, and row structure remain application-owned. |
| Visual editor | Paper, text, headings, caret, borders, links, code, syntax colors, Markdown element presentation, and packaged passive assets | Selection behavior, cursor geometry, widgets, commands, undo, and Markdown semantics remain application-owned. |
| Source editor | Font fallback and inherited semantic colors; scoped descendant styling is available | CodeMirror implementation classes are not a stable public contract. |
| Fonts and assets | Packaged WOFF2, passive raster images, and validated SVG resources | No network resources, scripts, HTML components, animated SVG, or arbitrary file access. |

### Minimum complete-theme checklist

Before distributing a theme, verify all of these states rather than checking only the editor paper:

- titlebar, sidebar header, file toolbar, file tree, outline, sidebar footer, and the editor paper;
- file-tree normal, hover, current document, multi-selected, and keyboard-focus-visible states;
- outline normal, hover, current heading, H1 through H6 hierarchy, and long-title clamping;
- visual-editor body text, H1 through H6, links, inline code, fenced code, quotes, callouts, lists, tasks, tables, marks, images, Mermaid, selection, caret, and search matches;
- source editor headings, links, selection, caret, and long wrapped lines;
- floating menus, popovers, scrollbars, dialogs, danger actions, and disabled controls;
- both the theme's normal window size and 轻语's compact breakpoint for H1/H2.

If a light and dark design are distributed as a pair, package them as two independent themes and run the checklist for each. Do not assume that changing only the background and foreground produces a usable dark theme; borders, selection fills, syntax colors, shadows, muted text, and scrollbar contrast all need independent values.

## Fonts, assets, and URL rules

Resource themes can use a packaged WOFF2 file directly:

```css
@font-face {
  font-family: "Example Theme Font";
  src: url("./assets/fonts/example.woff2") format("woff2");
  font-display: swap;
  font-style: normal;
  font-weight: 400;
}

:root[data-theme="ocean-night"] {
  font-family: "Example Theme Font", sans-serif;
}

.markdown-paper[data-editor-theme="ocean-night"] {
  --editor-font-family: "Example Theme Font", sans-serif;
  --editor-heading-font-family: "Example Theme Font", sans-serif;
}

:root[data-theme="ocean-night"] .markdown-source-paper {
  --source-editor-font-family: "Example Theme Font", monospace;
}
```

Supported resource files and per-file limits are:

| Location/type | Supported extensions | Maximum |
| --- | --- | --- |
| Manifest | root `manifest.json` | 64 KiB |
| Stylesheet | root `theme.css` | 256 KiB |
| Fonts below `assets/` | `.woff2` | 4 MiB each |
| Raster images below `assets/` | `.png`, `.jpg`, `.jpeg`, `.webp`, `.gif` | 8 MiB each |
| Vector images/icons below `assets/` | `.svg` | 8 MiB each and at most 256 XML element levels |
| Licenses below `licenses/` | `.txt`, `.md` UTF-8 text | Up to the 32 MiB package aggregate |

WOFF2 and raster contents must match their extensions. SVG is parsed as XML and cannot contain scripts, embedded HTML, event handlers, animation/active elements, external links, processing instructions, doctypes, or unsafe CSS URLs. Unknown assets, nested `.zip` or `.theme` archives, symbolic links, devices, sockets, and other special filesystem entries are rejected. `.theme` archives also reject symbolic-link, hard-link, and special-file metadata.

Allowed CSS URL examples:

```css
src: url("./assets/fonts/example.woff2");
background-image: url("./assets/images/background.webp");
background-image: image-set("./assets/images/background.webp" type("image/webp") 1x);
mask-image: url("./assets/icons/checkbox.svg");
fill: url(#marker);
background-image: url("data:image/png;base64,iVBORw0KGgo=");
```

Every `./assets/...` reference is percent-decoded and Unicode-normalized, and the referenced regular file must exist. A `data:` resource counts toward the 256 KiB CSS limit and must use one of the passive base64 MIME types listed above.
Quoted `image-set()` and `-webkit-image-set()` candidates follow the same URL rules. Dynamic `var()`, `env()`, or `attr()` substitution is not accepted inside those functions because 轻语 cannot validate the resulting resource before the stylesheet loads.

Rejected examples include:

```css
@import "./assets/more.css";
src: url("https://example.com/font.woff2");
src: url("//example.com/font.woff2");
src: url("file:///tmp/font.woff2");
src: url("/assets/font.woff2");
src: url("font.woff2");
src: url("../font.woff2");
src: url("./assets/../licenses/FONT-LICENSE.txt");
src: url("./assets/font.woff2?version=1");
background-image: image-set("https://example.com/background.webp" 1x);
background-image: image-set(var(--dynamic-image) 1x);
background-image: url("data:text/html;base64,PHNjcmlwdD4=");
background-image: url("data:image/svg+xml;base64,PHN2Zz4=");
```

## Directory, package, and path limits

### Unpacked directory rules

The unpacked-directory scanner applies these rules:

- at most 256 total entries, counting files and directories;
- at most 32 MiB total uncompressed file content;
- at most 240 Unicode scalar values in each normalized relative path;
- at most 16 path segments in each relative path;
- UTF-8 names in Unicode NFC form, with no duplicate normalized or case-folded paths;
- no empty, `.`, or `..` path segments, NULs, backslashes, absolute paths, or paths outside `assets/` and `licenses/` except the two required root files;
- regular files and directories only; symbolic links and special filesystem entries are rejected.

These rules validate safe local loading, not every cross-platform archive name. On a filesystem that permits them, an unpacked directory component containing a colon, ending in a dot or space, or using a Windows reserved device basename may pass directory scanning. The stricter portable archive rules below still apply when an author packages the directory externally. Authors should always use portable names while developing, even when the local directory scanner accepts more.

### Portable `.theme` archive rules

A `.theme` distribution file is an ordinary, single-disk, non-encrypted ZIP renamed with the `.theme` extension. It is limited to 16 MiB compressed source size and 32 MiB total uncompressed content, at most 256 entries, and Stored or Deflated entries. ZIP64, unsupported compression, and nested archives are rejected. The archive contains one theme directly at its root—`manifest.json`, `theme.css`, and any `assets/` or `licenses/`—not an extra wrapper directory and not multiple selectable themes.

In addition to the size, count, depth, normalized-path, and allowed-location rules above, every archive entry name must be raw UTF-8 and portable. Archives reject:

- control characters (including NUL), a colon anywhere, any backslash, leading slash or backslash, drive-prefixed and UNC paths;
- empty, `.`, or `..` components, and any component ending in a dot or space;
- duplicate paths after Unicode NFC normalization or Unicode case folding;
- Windows reserved device basenames, including when the reserved stem appears before an extension: `CON`, `PRN`, `AUX`, `NUL`, `CONIN$`, `CONOUT$`, `CLOCK$`, `COM1` through `COM9`, and `LPT1` through `LPT9` (also the Windows superscript-number forms `COM¹`, `COM²`, `COM³`, `LPT¹`, `LPT²`, and `LPT³`);
- symbolic-link, hard-link, device, FIFO, socket, and other special-file metadata.

## Develop, install, and package

On desktop:

1. Choose **Settings → Appearance → Open theme folder**.
2. Copy the unpacked theme directory into that folder. A simple legacy `.css` file can instead be placed at the folder root.
3. Select **Refresh themes** after adding or editing files. Invalid directories and files are reported without blocking valid themes.
4. Select the theme in its light or dark section. Selection applies immediately without a second confirmation.

Desktop **Import theme** accepts both `.css` and `.theme`. Importing a new ID installs it. Importing an ID that already exists asks before replacement and checks the installed theme's expected fingerprint; replacement is atomic, so validation or publication failure leaves the old theme intact. Unpacked author directories are not rewritten by Refresh.

To distribute a resource theme, create a standard ZIP archive whose root contains `manifest.json`, `theme.css`, and any referenced `assets/` or declared `licenses/`, then rename the archive to use the `.theme` extension. Do not add a wrapper directory. 轻语 validates the complete archive during import and does not provide an in-app theme export or packaging action.

Compact/mobile installations can discover and use valid legacy or resource themes already present in their theme folder. They provide theme selection, Refresh, and deletion, but do not expose Import theme or Open theme folder actions. Light and dark lists are independent, may contain different numbers of themes, and retain their current selections.

## Fingerprints and atomic updates

轻语 computes a content fingerprint from the normalized manifest plus every sorted path and file byte. ZIP compression or entry order does not change it, while changing the manifest, CSS, font, image, icon, or license does. Selection activates the current validated fingerprint immediately. A stale expected fingerprint cannot replace or delete a newer theme, and a changed author directory is revalidated before its stylesheet can be exposed.

## Current non-goals

- Theme files, packages, assets, and selected theme IDs are not synchronized through S3 or WebDAV.
- Network resources are not supported; themes cannot depend on remote fonts, images, stylesheets, or updates.
- Themes cannot execute scripts or ship HTML components.
- A `.theme` package cannot contain multiple independently selectable themes.
- There is no required theme-authoring CLI; ordinary files, Refresh, selection, and standard external ZIP tooling are the supported authoring workflow.
