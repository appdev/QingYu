# View Mode Shortcut Design

## Goal

Add a configurable application shortcut that cycles through QingYu's preset view modes. The default shortcut is `F8`.

## User Experience

- Pressing the configured shortcut cycles view modes in this order:
  `daily -> focus -> immersive -> full -> daily`.
- Pressing the shortcut while the current mode is `custom` switches to `daily`.
- The shortcut is available in Settings under the application shortcut group with the label "Toggle view mode" and its localized equivalent.
- Users can record a replacement shortcut with the existing shortcut recorder or restore `F8` through the existing reset action.
- On macOS keyboards configured to use function keys for media controls, the user may need to press `Fn+F8` to produce the `F8` key event.

## Architecture

The existing shared shortcut registry remains the source of truth. A new `toggleViewMode` action will be added with the default binding `F8`. The shortcut parser and matcher will be extended to support unmodified function keys while preserving the existing requirement that letters, digits, and punctuation use `Mod`.

The application shortcut hook will route `toggleViewMode` to an App-level handler. That handler will reuse `nextViewMode` and the existing view-mode preference update path, so persistence and cross-window preference notifications remain consistent with menu-based selection.

No new dependency or separate shortcut subsystem will be introduced.

## Shortcut Rules

- Existing `Mod` shortcuts retain their current syntax and behavior.
- Unmodified function keys `F1` through `F12` are valid shortcut values.
- Ordinary letters, digits, and punctuation remain invalid without `Mod` to prevent collisions with editor input.
- Matching requires the event's modifier state to equal the stored shortcut, so `F8` does not match `Cmd+F8`, `Ctrl+F8`, `Alt+F8`, or `Shift+F8`.
- Shortcut normalization continues to resolve duplicate assignments with the existing conflict behavior.

## UI and Localization

The keyboard shortcut settings application group will include `toggleViewMode`. A localized `app.toggleViewMode` label will be added to every supported locale and to the i18n key type.

The shortcut recorder will display unmodified function keys without a `Cmd` or `Ctrl` prefix. It will continue to record existing modified shortcuts unchanged.

## Testing

Focused tests will cover:

- the shared registry defaulting `toggleViewMode` to `F8`;
- parsing, formatting, capture, normalization, and matching of unmodified function keys;
- rejection of unmodified ordinary typing keys;
- routing the configured shortcut through the application shortcut hook;
- displaying and changing the shortcut in settings;
- cycling the App view mode through all preset transitions, including `custom -> daily`;
- persistence through the existing editor preference update path.

After focused tests pass, the full workspace test suite and production build will be run.

## Non-Goals

- Adding a separate native menu item for view-mode cycling.
- Changing the existing title-bar view-mode selector.
- Cycling through `custom` mode.
- Assigning special semantics to `Escape`.
