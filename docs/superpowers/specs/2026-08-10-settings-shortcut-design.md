# Settings Shortcut Design

## Goal

Use the conventional settings shortcut on every desktop platform: `Command+,` on macOS and `Ctrl+,` on Windows and Linux.

## Design

The existing `config` command remains the single settings action. Its default keymap value changes from `⌥P` to the project's cross-platform primary-modifier notation `⌘,`. The existing keyboard matching layer interprets that notation as Command on macOS and Control on Windows and Linux. The macOS native menu continues to read the same configured value and convert it to an Electron accelerator, so the application menu and renderer shortcut cannot drift apart.

User customization remains supported through the existing keymap configuration. No new platform branch, menu command, translation, or preference is introduced.

## Verification

Add a focused source-level regression assertion for the default `config` keymap and retain the native-menu test that verifies accelerators are derived from configured keymap values. Run the focused tests, TypeScript checking, and frontend lint.
