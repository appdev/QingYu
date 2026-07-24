---
version: alpha
name: QingYu
description: Local-first WYSIWYG Markdown desktop editor with quiet writing surfaces and a restrained ink-black interaction system.
colors:
  primary: "#1A1C1E"
  primary-hover: "#0F1115"
  primary-soft: "#E8E8E9"
  primary-inverse: "#F4F4F5"
  primary-inverse-hover: "#FAFAFA"
  primary-inverse-soft: "#3A3A3D"
  background: "#FFFFFF"
  surface: "#FAFAFA"
  surface-muted: "#F8F8F8"
  surface-hover: "#F5F5F5"
  surface-active: "#EEEEEE"
  text: "#555555"
  text-strong: "#333333"
  text-muted: "#999999"
  markdown-syntax: "#C7C5C5"
  border: "#EEEEEE"
  border-strong: "#DDDDDD"
  ghost: "#C0C0C0"
  overlay-ink: "#1F232C"
typography:
  editor-h1:
    fontFamily: -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif
    fontSize: 44px
    fontWeight: 760
    lineHeight: 1.15
    letterSpacing: 0em
  editor-h2:
    fontFamily: -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif
    fontSize: 31px
    fontWeight: 760
    lineHeight: 1.22
    letterSpacing: 0em
  editor-h3:
    fontFamily: -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif
    fontSize: 24px
    fontWeight: 760
    lineHeight: 1.28
    letterSpacing: 0em
  editor-body:
    fontFamily: -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif
    fontSize: 16px
    fontWeight: 400
    lineHeight: 1.65
    letterSpacing: 0em
  ui-body:
    fontFamily: -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif
    fontSize: 13px
    fontWeight: 520
    lineHeight: 1.54
    letterSpacing: 0em
  ui-label:
    fontFamily: -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif
    fontSize: 12px
    fontWeight: 560
    lineHeight: 1.67
    letterSpacing: 0em
  ui-control:
    fontFamily: -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif
    fontSize: 12px
    fontWeight: 620
    lineHeight: 1.67
    letterSpacing: 0em
spacing:
  none: 0px
  xxs: 2px
  xs: 4px
  sm: 8px
  md: 12px
  lg: 16px
  xl: 24px
  2xl: 32px
  editor-block: 18px
  editor-section: 36px
  panel-width: 384px
rounded:
  none: 0px
  xs: 2px
  sm: 4px
  md: 6px
  lg: 8px
  full: 9999px
components:
  app-background:
    backgroundColor: "{colors.background}"
    textColor: "{colors.text}"
    typography: "{typography.ui-body}"
  editor-paper:
    backgroundColor: "{colors.background}"
    textColor: "{colors.text}"
    typography: "{typography.editor-body}"
    padding: 24px
  editor-heading:
    textColor: "{colors.text-strong}"
    typography: "{typography.editor-h1}"
  editor-markdown-syntax:
    textColor: "{colors.markdown-syntax}"
    typography: "{typography.editor-body}"
  panel-surface:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.text}"
    typography: "{typography.ui-body}"
    rounded: "{rounded.lg}"
    padding: 16px
  code-surface:
    backgroundColor: "{colors.surface-muted}"
    textColor: "{colors.text-strong}"
    typography: "{typography.editor-body}"
    rounded: "{rounded.md}"
    padding: 12px
  hover-surface:
    backgroundColor: "{colors.surface-hover}"
    textColor: "{colors.text-strong}"
    typography: "{typography.ui-body}"
    rounded: "{rounded.md}"
  active-surface:
    backgroundColor: "{colors.surface-active}"
    textColor: "{colors.text-strong}"
    typography: "{typography.ui-body}"
    rounded: "{rounded.md}"
  button-primary:
    backgroundColor: "{colors.primary}"
    textColor: "{colors.background}"
    typography: "{typography.ui-control}"
    rounded: "{rounded.md}"
    padding: 12px
  button-primary-hover:
    backgroundColor: "{colors.primary-hover}"
    textColor: "{colors.background}"
    typography: "{typography.ui-control}"
    rounded: "{rounded.md}"
    padding: 12px
  button-primary-dark:
    backgroundColor: "{colors.primary-inverse}"
    textColor: "{colors.overlay-ink}"
    typography: "{typography.ui-control}"
    rounded: "{rounded.md}"
    padding: 12px
  button-primary-dark-hover:
    backgroundColor: "{colors.primary-inverse-hover}"
    textColor: "{colors.overlay-ink}"
    typography: "{typography.ui-control}"
    rounded: "{rounded.md}"
    padding: 12px
  button-secondary:
    backgroundColor: "{colors.background}"
    textColor: "{colors.text-strong}"
    typography: "{typography.ui-control}"
    rounded: "{rounded.md}"
    padding: 12px
  toggle-active:
    backgroundColor: "{colors.primary-soft}"
    textColor: "{colors.primary-hover}"
    typography: "{typography.ui-control}"
    rounded: "{rounded.full}"
    padding: 10px
  toggle-active-dark:
    backgroundColor: "{colors.primary-inverse-soft}"
    textColor: "{colors.primary-inverse}"
    typography: "{typography.ui-control}"
    rounded: "{rounded.full}"
    padding: 10px
  input-field:
    backgroundColor: "{colors.background}"
    textColor: "{colors.text-strong}"
    typography: "{typography.ui-body}"
    rounded: "{rounded.md}"
    padding: 12px
  caption-text:
    textColor: "{colors.text-muted}"
    typography: "{typography.ui-label}"
  divider:
    backgroundColor: "{colors.border}"
    height: 1px
  divider-strong:
    backgroundColor: "{colors.border-strong}"
    height: 1px
  drag-ghost:
    backgroundColor: "{colors.background}"
    textColor: "{colors.text-strong}"
    typography: "{typography.ui-body}"
    rounded: "{rounded.sm}"
    padding: 8px
  ghost-control:
    textColor: "{colors.ghost}"
    typography: "{typography.ui-label}"
  overlay:
    backgroundColor: "{colors.overlay-ink}"
---

# QingYu Design System

## Overview

QingYu should feel like a native writing instrument: calm, fast, local, and precise. The interface gives most of its attention to the Markdown document, while file navigation, search, settings, and export tools stay quiet until needed.

The default visual language is light, neutral, and editorial. It avoids decorative branding, heavy panels, and marketing-style composition. The primary ink-black is reserved for interaction, focus, and active states; it should not flood the workspace.

The product serves writers, engineers, researchers, and documentation-heavy teams. Screens should feel trustworthy for long-form reading and repeated editing, with enough density for desktop productivity and enough air around the document to keep the writing surface comfortable.

## Colors

The palette is based on clean neutrals and one restrained ink-black accent.

- **Primary (#1A1C1E):** A deep ink-black interaction color. Use for primary actions, focus rings, active toggles, drop indicators, selected status, and links.
- **Primary Hover (#0F1115):** A near-black hover color for pressed states and selected link states.
- **Primary Soft (#E8E8E9):** A quiet ink-tinted container used behind selected pills and quiet emphasis. Pair it with Primary Hover for text when WCAG AA contrast is required.
- **Primary Inverse (#F4F4F5):** The dark-mode interaction color. Use it as the inverted form of Primary on dark surfaces instead of reintroducing blue.
- **Primary Inverse Hover (#FAFAFA):** The brighter dark-mode hover color for primary controls.
- **Primary Inverse Soft (#3A3A3D):** A dark neutral container for selected dark-mode pills and subtle active states.
- **Background (#FFFFFF):** The main application and editor surface.
- **Surface (#FAFAFA), Surface Muted (#F8F8F8), Surface Hover (#F5F5F5), Surface Active (#EEEEEE):** Layered neutral surfaces for panels, code, hover feedback, and selected controls.
- **Text (#555555) and Text Strong (#333333):** Body text and headings. Strong text is for document headings, selected labels, and critical UI labels.
- **Text Muted (#999999):** Secondary labels, empty states, helper text, metadata, and inactive controls.
- **Markdown Syntax (#C7C5C5):** Low-emphasis rendered Markdown markers and source-mode syntax characters.
- **Border (#EEEEEE) and Border Strong (#DDDDDD):** Fine separation for tool surfaces, tables, inputs, and editor structure.

Do not introduce extra brand colors for primary flows. Semantic colors may appear for table delete affordances, callout types, syntax highlighting, or destructive confirmation, but they should stay localized to those contexts.

## Typography

QingYu uses the system UI stack for both application chrome and the default editor theme. This keeps the desktop app native-feeling across macOS, Windows, and Linux while avoiding font loading cost.

Editor typography is larger and calmer than UI typography. Body copy defaults to 16px with a 1.65 line-height for comfortable long-form writing. Headings use strong weight, normal tracking, and clear scale jumps: H1 at 44px, H2 at 31px, and H3 at 24px.

Application controls use compact desktop sizes. Most settings, menus, inputs, and list controls sit between 12px and 13px, with medium-to-semibold weights for clarity. Letter spacing remains 0 across the UI.

Use typographic hierarchy before adding decoration. Prefer weight, size, tone, and spacing changes over colored badges or boxed labels.

## Layout

The layout is a desktop productivity workspace centered on the document. The editor paper owns the main visual field; file tree, tabs, source view, search, settings, and export tools are supporting surfaces.

Use a compact spacing rhythm built from 4px and 8px increments. Application controls use small gaps and stable heights. Editor content uses larger block spacing: paragraphs and block elements breathe, headings create section rhythm, and tables/code/callouts get enough room for scanning.

Resizable panes should use stable min/max boundaries. Side panels default to 384px, with enough width for settings and supporting information without compressing the document beyond usefulness. Split editor mode should keep both source and visual panes predictable, with no layout shift from hover states.

Mobile and narrow responsive states should simplify side surfaces before shrinking the document into unusable density.

## Elevation & Depth

QingYu uses tonal layering, fine borders, and selective shadows instead of heavy elevation. The main document surface should feel flat and stable. Popovers, modals, drag ghosts, and table controls may use shadows to clarify temporary floating state.

Shadows should be soft and functional. They signal that a surface is transient or layered above the editor; they should not make every panel feel like a card. Persistent workspace regions rely on background contrast and borders.

The strongest depth treatment belongs to modal overlays because they interrupt the writing flow and need clear foreground separation.

## Shapes

The shape language is quiet and practical. Standard controls use 6px radius. Larger transient surfaces such as modals, menus, and popovers may use 8px. Small editor handles and compact tool buttons may use 2px to 4px. Pills and toggles use full radius.

Avoid oversized rounded rectangles in dense desktop UI. Rounded corners should soften interaction targets without turning the app into a card-heavy landing page. Editor content itself should remain document-like, not boxed unless the Markdown element requires it, such as callouts, code blocks, and tables.

## Components

Primary buttons use the primary ink-black background with white text and switch to primary-hover on hover or pressed states. Use them only for the most important confirmation in a local surface.

Secondary buttons use the background surface, strong text, and neutral borders. Ghost buttons are text-first and should reveal hover background only when interaction is likely.

Icon buttons are preferred for toolbar actions when a familiar symbol exists. Pair them with accessible labels and visible focus rings. Use `lucide-react` icons for app controls.

Inputs, selects, textareas, and search fields use white background, neutral border, strong text, and the primary color for focus border and focus ring. Placeholder text uses muted text.

Segmented controls sit on surface backgrounds with compact spacing. The selected segment uses active neutral surface and strong text; focus uses the primary ring.

Toggles use primary for checked state and neutral surface for unchecked state. Toggle labels should be short and operational.

The editor paper uses background white, body text gray, strong headings, and neutral dividers. Code blocks use muted surfaces. Tables use strong borders, compact cell padding, and neutral alternating rows.

Temporary decision surfaces such as confirmation dialogs may use stronger shadow and accent focus. Accent shadows must be derived from the current theme token, such as `color-mix(... var(--accent) ...)`, rather than hardcoded to one color.

Callouts use localized semantic colors by callout type, but the surrounding system remains neutral. Do not reuse callout colors as global brand accents.

## Do's and Don'ts

- Do keep the document as the visual anchor of every workspace screen.
- Do use `#1A1C1E` for primary interaction, focus, active state, and selected status.
- Do use neutral surfaces and borders for persistent structure before adding shadow.
- Do keep component text compact and readable, especially in title bars, menus, settings, and panels.
- Do preserve WCAG AA contrast for interactive text and controls.
- Do make hover, focus, pressed, loading, disabled, empty, and error states explicit.
- Do use Tailwind CSS and shared UI components where practical.
- Don't introduce a second dominant brand accent beside the primary ink-black.
- Don't turn persistent app sections into floating card stacks.
- Don't use large marketing-style hero layouts inside the desktop app.
- Don't over-round dense controls; use 6px as the default radius and full radius only for pills/toggles.
- Don't hide destructive file or document actions behind ambiguous icons.
- Don't make destructive file or document changes without explicit user confirmation.

## Marketing Site Variant

The product website extends this system without changing the desktop workspace. It should feel like the public face of the same native writing instrument: quiet, factual, screenshot-led, and visibly related to the product.

### Genre and structure

- Genre: editorial with an austere product voice.
- Marketing macrostructure: **Workbench**. Real QingYu captures are the primary evidence; prose explains what is visible.
- Navigation: **N9 Edge-aligned minimal**. Brand at the start edge, language and product action at the end edge, no centred link row.
- Footer: **Ft2 Inline single line**. Project and legal links close the page without a sitemap grid.
- Feature voice: annotated real screenshots with labels outside the image. Never draw browser, editor, IDE, or phone chrome.

### Site palette

- `--color-paper`: `oklch(98.5% 0.004 82)`
- `--color-paper-2`: `oklch(96.5% 0.006 82)`
- `--color-paper-3`: `oklch(93.5% 0.007 82)`
- `--color-ink`: `oklch(20% 0.009 72)`
- `--color-ink-2`: `oklch(29% 0.009 72)`
- `--color-muted`: `oklch(51% 0.009 72)`
- `--color-rule`: `oklch(87% 0.006 82)`
- `--color-rule-2`: `oklch(72% 0.008 78)`
- `--color-accent`: the same ink-black as the app primary interaction colour.
- Official Logo yellow may appear only inside the logo asset. It is not a page-level CTA or surface colour.

### Site typography

- Display headings use `QingYu WenKai Subset`, tying the public site to writing without imitating generic SaaS display type.
- Body and controls use the native system UI stack, matching the desktop product.
- Display headings are upright, tightly tracked, and capped at `4.5rem`.
- Body copy remains at least `16px`, uses a `1.65` line-height, and stays within `65ch`.

### Site spacing, shape, and motion

- Use the 4pt named scale in `apps/site/src/tokens.css`; page styles do not introduce raw spacing values for repeated layout roles.
- Controls keep the application `6px` radius. Product captures use an `8px` radius and a single hairline border.
- The site has no scroll reveals, parallax, card lifting, gradients, or infinite decorative animation.
- Button feedback is a color change plus a one-pixel pressed translation. Focus rings appear immediately.
- Root `html` and `body` use `overflow-x: clip`; the site is verified at 320, 375, 414, and 768 CSS pixels.

### Site CTA voice

- Primary: ink-black fill, light paper text, square-soft 6px corners, direct verb such as “免费下载桌面版”.
- Secondary: paper fill, ink border, direct verb such as “打开 Web 编辑器”.
- CTA, navigation, and inline-footer labels stay on one line.

## Exports

The source of truth for the marketing site is `apps/site/src/tokens.css`.

### CSS custom properties

The complete canonical CSS custom-property export is the `:root` block in `apps/site/src/tokens.css`. It is referenced directly here instead of duplicated so the runtime and documentation cannot drift; the Tailwind, DTCG, and shadcn mappings below are derived from this canonical CSS export.

### Tailwind v4 `@theme`

```css
@theme {
  --color-paper: oklch(98.5% 0.004 82);
  --color-paper-2: oklch(96.5% 0.006 82);
  --color-paper-3: oklch(93.5% 0.007 82);
  --color-rule: oklch(87% 0.006 82);
  --color-rule-2: oklch(72% 0.008 78);
  --color-muted: oklch(51% 0.009 72);
  --color-neutral: oklch(39% 0.009 72);
  --color-ink-2: oklch(29% 0.009 72);
  --color-ink: oklch(20% 0.009 72);
  --color-accent: oklch(20% 0.009 72);
  --color-focus: oklch(35% 0.035 70);
  --font-display: "QingYu WenKai Subset", "Kaiti SC", "STKaiti", serif;
  --font-body: -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif;
  --spacing-sm: 1rem;
  --spacing-md: 1.5rem;
  --spacing-lg: 2rem;
  --spacing-xl: 3rem;
  --ease-out: cubic-bezier(0.16, 1, 0.3, 1);
  --ease-in: cubic-bezier(0.7, 0, 0.84, 0);
  --ease-in-out: cubic-bezier(0.65, 0, 0.35, 1);
}
```

### DTCG `tokens.json`

```json
{
  "$schema": "https://design-tokens.github.io/community-group/format/",
  "color": {
    "paper": { "$value": "oklch(98.5% 0.004 82)", "$type": "color" },
    "paper-2": { "$value": "oklch(96.5% 0.006 82)", "$type": "color" },
    "ink": { "$value": "oklch(20% 0.009 72)", "$type": "color" },
    "ink-2": { "$value": "oklch(29% 0.009 72)", "$type": "color" },
    "rule": { "$value": "oklch(87% 0.006 82)", "$type": "color" },
    "accent": { "$value": "oklch(20% 0.009 72)", "$type": "color" },
    "focus": { "$value": "oklch(35% 0.035 70)", "$type": "color" }
  },
  "font": {
    "display": { "$value": "QingYu WenKai Subset", "$type": "fontFamily" },
    "body": { "$value": "system UI", "$type": "fontFamily" }
  },
  "duration": {
    "micro": { "$value": "120ms", "$type": "duration" },
    "short": { "$value": "220ms", "$type": "duration" },
    "long": { "$value": "420ms", "$type": "duration" }
  }
}
```

### shadcn/ui CSS variables

```css
:root {
  --background: 98.5% 0.004 82;
  --foreground: 20% 0.009 72;
  --card: 96.5% 0.006 82;
  --card-foreground: 20% 0.009 72;
  --popover: 96.5% 0.006 82;
  --popover-foreground: 20% 0.009 72;
  --primary: 20% 0.009 72;
  --primary-foreground: 98.5% 0.004 82;
  --secondary: 93.5% 0.007 82;
  --secondary-foreground: 29% 0.009 72;
  --muted: 87% 0.006 82;
  --muted-foreground: 51% 0.009 72;
  --border: 87% 0.006 82;
  --input: 87% 0.006 82;
  --ring: 35% 0.035 70;
  --radius: 0.375rem;
}
```
