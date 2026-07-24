# Settings Storage Category Design

## Goal

Remove the misleading `Storage` settings category by placing its two unrelated controls with the settings they actually affect.

## Context

The current `Storage` page combines application-settings transfer with image-file naming. Configuration export/import operates on portable application preferences, while the file-name pattern controls images newly saved from the editor. Neither control selects the note storage location, and the existing `Backup` page separately owns note-folder safety copies.

## Considered Approaches

1. Move configuration transfer to `General`, move image naming to `Editor`, and remove `Storage`. This is the selected approach because it follows the existing settings responsibilities and removes a misleading one-item category.
2. Rename `Storage` to `Data and migration` and keep configuration transfer there. This describes configuration transfer more accurately, but leaves a dedicated sidebar category with only one row.
3. Create a broader `Data management` category. This could become useful if import, export, reset, and migration features grow later, but it adds structure the current product does not need.

## User Experience

- The `Storage` entry no longer appears in the settings sidebar.
- `Configuration backup`, including `Export settings` and `Import settings`, appears as the last section of `General`.
- `File naming pattern` appears in `Editor` next to `Clipboard image folder`, so the destination folder and generated name for newly saved images can be configured together.
- The independent `Backup` category remains unchanged and continues to describe note-folder safety copies.
- Existing setting values and persistence keys are unchanged. This is an information-architecture change, not a settings migration.

## Implementation

- Move the configuration-transfer row and its transfer callbacks/state from `StorageSettings` into `GeneralSettings`.
- Move the image file-name pattern row into the same editor section as `Clipboard image folder` in `EditorSettings`.
- Remove `StorageSettings` and the `storage` category from settings navigation and category typing.
- Keep the existing translation keys for the moved rows where their wording remains accurate. Remove only the obsolete `Storage` category label and dead component references.
- Preserve existing export/import logic, normalization, image-name generation, and error toasts without behavioral changes.

## Error Handling

Existing behavior remains in force: settings transfer disables both buttons while running, reports success or failure with toasts, and rejects invalid settings files. Image-name patterns continue to use the existing normalization and fallback rules.

## Testing and Verification

- Update focused settings component tests to assert that configuration transfer renders under `General` and image naming renders under `Editor`.
- Update sidebar and settings-window tests so `Storage` is absent and the moved callbacks still reach the existing handlers.
- Delete obsolete `StorageSettings` tests together with the removed component, preserving their behavioral assertions in the destination component tests.
- Run the smallest focused tests for the affected settings components, followed by `pnpm test` and `pnpm build`.

## Non-Goals

- Changing which values are included in configuration export/import.
- Backing up or migrating Markdown notes and attachments.
- Changing note backup or project synchronization behavior.
- Renaming existing settings or changing image storage paths.
