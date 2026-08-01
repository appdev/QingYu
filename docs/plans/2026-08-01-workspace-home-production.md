# Workspace Home Production Implementation Plan

**Goal:** Replace the approved Workspace Home placeholder with the restrained, theme-adaptive feather composition while preserving the existing home/editor/recovery ownership and capability-filtered action contract.

**Architecture:** Keep `WorkspaceHome` presentational. The desktop app supplies platform-formatted shortcut strings, including configurable values from preferences. A small shared display formatter keeps Settings and Home consistent. A pure contrast module computes accessible local colors from the active theme, while the component resolves CSS tokens and recalculates them after theme activation changes.

**Tech Stack:** React, TypeScript, Tailwind CSS, Vite asset imports, Vitest, Testing Library.

## Constraints

- Change only `packages/app` production UI, its focused tests, and this plan.
- Preserve the existing `WorkspaceHomeProps.actions` callbacks and state ownership.
- Extend the shortcut map only for `openDocument`; do not add action state APIs.
- Keep unavailable capabilities omitted rather than disabled.
- Do not introduce a theme switcher, marketing content, navigation, footer, or card grid.
- Reuse `assets/branding/app-icon/feather.png`; do not create or generate a new asset.
- Preserve the unrelated `apps/desktop/src-tauri/src/primary_workspace.rs` working-tree change.

## Task 1: Shortcut display contract

- Add failing tests for platform-specific `Mod` formatting and invalid-value fallback.
- Extract the existing settings formatter to `packages/app/src/lib/shortcut-display.ts`.
- Update Settings to use the shared formatter.
- Extend `WorkspaceHomeProps.shortcuts` with `openDocument` and pass New/Open/Settings plus configurable Quick Open/Show Files values from `App.tsx`.

## Task 2: Theme-relative contrast

- Add failing pure tests for color parsing, contrast ratio, threshold fitting, and readable-color preservation/repair.
- Implement `packages/app/src/lib/workspace-home-contrast.ts` without dependencies.
- Keep brand targets at approximately 1.55:1 for the base and 2.05:1 for sliced bands; keep text at 4.5:1 and focus at 3:1.

## Task 3: Approved Home composition

- Replace the title/description/icon-card placeholder with an accessible hidden heading, a decorative three-slice feather, and two capability-filtered action groups.
- Match desktop and compact geometry with Tailwind utilities and local CSS variables.
- Hide shortcut hints whenever the caller does not provide values; compact currently provides none.
- Observe root theme activation attributes and recalculate local colors without exposing theme controls.
- Add component tests for visual structure, action order, capability omission, shortcuts, callbacks, compact targets, decorative semantics, and contrast recalculation.

## Task 4: Verification

- Run the focused contrast, shortcut, Workspace Home, and Settings tests.
- Run `pnpm --filter @markra/app typecheck:test` and `pnpm --filter @markra/app build`.
- Run the complete `@markra/app` test suite and inspect the final diff/status.
- After explicit production approval, commit the isolated feature and merge it into local `main`; do not push without a separate request.
