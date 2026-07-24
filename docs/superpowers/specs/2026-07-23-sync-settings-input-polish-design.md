# Sync Settings Input Polish Design

## Goal

Make the connection fields in Sync Settings feel visually consistent and deliberate without changing sync behavior or credential handling.

## Problem

The current settings text input uses a compact 32 px control, a relatively heavy text weight, and an opaque two-pixel accent ring. In the S3 form, the endpoint field is 256 px wide while the remaining fields fall back to 176 px. The mixed widths break the right-column rhythm, and the focus treatment reads as a thick black frame in the default QingYu theme.

## Decision

Keep the existing `SettingsTextInput` API and refine that shared settings control only:

- Change the default width from 176 px (`w-44`) to 256 px (`w-64`) so the WebDAV and S3 connection fields line up with the URL fields.
- Add `max-[760px]:w-full` so the control collapses to the available row width on narrow settings layouts.
- Use a 36 px height, 13 px type, and medium text weight to match QingYu's existing `@markra/ui` text input.
- Keep the one-pixel neutral border in every state.
- Use the existing hover surface token for a quiet pointer affordance.
- On pointer or keyboard focus, deepen the one-pixel border and use a two-pixel accent ring at 20% opacity. The solid border remains the high-contrast focus signal while the ring softens the visual edge. Focus indicators must appear immediately and must not participate in the transition.
- Preserve `autoCapitalize="none"`, `autoCorrect="off"`, `spellCheck={false}`, password masking, placeholders, controlled values, and change callbacks.

No sync settings, persistence, network behavior, or credential storage logic changes.

## Files

- Modify `packages/app/src/components/settings/SettingsControls.tsx` for the shared settings text-input classes.
- Modify `packages/app/src/components/settings/SettingsControls.test.tsx` for the visual-contract regression test.

No production files are deleted or created.

## Verification

- Prove the visual-contract test fails against the old control.
- Run the focused settings-control test after implementation.
- Run the full `@markra/app` test suite and type-check/build gates.
- Inspect the final class list for constant border width, a soft focus ring, and the narrow-layout width override.
