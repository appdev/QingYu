# Settings Sidebar Selection Design

## Goal

Restore a visible selected state for the active category in the desktop settings sidebar while preserving the existing navigation behavior and theme system.

## Root Cause

The active category already renders with `aria-current="page"`, and selecting a category updates the settings content correctly. The sidebar styles currently use Tailwind's `aria-current:` variant, which compiles to a selector for `aria-current="true"`. Because the rendered value is `page`, the selected background and foreground rules never match.

## User Experience

- The active settings category uses the existing `--bg-active` background token.
- Its label and icon use the existing `--accent` token.
- Hover, focus, spacing, and navigation behavior remain unchanged.
- The selected state follows every supported light, dark, and custom theme through the existing tokens.

## Implementation

Keep `aria-current="page"` because it accurately describes the active settings page. Replace the two generic `aria-current:` style variants on the settings category button with explicit `aria-[current=page]:` variants.

The change is limited to the main settings category sidebar. It does not alter project sync behavior, settings routing, theme variables, or the provider list nested inside AI settings.

## Testing and Verification

Add a focused component regression test that renders the sidebar with an active category and checks both the semantic `aria-current="page"` state and the explicit Tailwind variants used to display it. Run the test before implementation to confirm it fails for the current generic variants, then rerun it after the change.

After the focused test passes:

- run the relevant app package tests;
- run the production build so Tailwind generates the final selector;
- confirm the generated CSS targets `[aria-current=page]` for the selected-state rules;
- launch the Tauri desktop app and visually confirm that changing settings categories moves the selected background and accent color to the active row.

## Non-Goals

- Adding a left-side selection bar or introducing a stronger visual treatment.
- Changing theme colors or adding new design tokens.
- Refactoring the settings navigation state.
- Changing other components that use `aria-current`.
