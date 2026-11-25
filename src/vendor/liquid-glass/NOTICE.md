# Liquid Glass for Web

Upstream: https://github.com/zakisheriff/Liquid-Glass
Package: `@zakisheriff/liquid-glass` 4.0.3 (MIT, see LICENSE). Sources were taken from the published source maps.

- `spring.ts`, `selection.ts`, `haptics.ts` are unchanged.
- `press.ts` turns the `usePress` React hook into `attachPress(element, options)`.
- `base.css` keeps the tokens and the glass material; `menu.css` comes from `GlassMenu.css`, with the row hover limited to a fine pointer and sizes read from `--qlg-menu-*` variables (phones use the iOS proportions from AGENTS.md).
- QingYu's menu in `src/liquid-menu.ts` ports `GlassMenu.tsx` without React and adds separators, submenus, menus opened from an existing button or a point, shortcuts and keyboard navigation.
- Class names, custom properties and data attributes use the `qlg-` prefix instead of `lg-`. The web build also loads glisse, which shares the `lg-` names.
