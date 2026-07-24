# Cross-Platform App Icon Design

## Goal

Replace every existing QingYu application and repository branding icon with the supplied `logo.png`, while reusing the already processed macOS icon from the sibling QingYu project and adapting the remaining artwork to Windows, Linux, Android, iOS, and the web.

## Source Assets

- Keep the supplied root-level `logo.png` as the canonical flattened brand artwork.
- Derive reusable background and feather layers from the supplied artwork without regenerating or redrawing the brand mark.
- Preserve the yellow background treatment and white feather silhouette.
- Remove the source image's baked outer white canvas where a platform expects transparent artwork.

Generated platform assets must be reproducible from committed source assets. A pnpm-accessible script will document and run deterministic generation steps where the platform toolchain permits automation.

## Platform Outputs

### macOS 26 And Older macOS Releases

- Import `/Volumes/extendData/Data/IdeaProjects/QingYu/build/icon.icns`, which already contains the processed yellow-feather artwork with transparent outer pixels and macOS-appropriate sizing.
- Commit an in-repository canonical copy so icon generation does not depend on the sibling checkout after import.
- Use the same ICNS through Tauri's existing `CFBundleIconFile` path on macOS 26 and older macOS releases.
- Do not add an `AppIcon.icon`, `Assets.car`, or `CFBundleIconName`; the referenced QingYu application bundles use the processed ICNS directly.

### Windows

- Generate `icon.ico` with 16, 24, 32, 48, 64, and 256 pixel layers.
- Use transparent outer pixels rather than the white source canvas.
- Inspect the 16 and 24 pixel layers separately to ensure the feather remains recognizable instead of relying only on a large-size preview.
- Regenerate the existing Square and Store PNG assets used by future AppX or Microsoft Store packaging.

### Linux

- Generate square 32-bit RGBA PNG assets at the existing Tauri sizes.
- Preserve transparency outside the icon enclosure.

### iOS

- Generate the complete existing iOS AppIcon set.
- Use an opaque, full-bleed square yellow treatment with the feather composited directly.
- Let the platform apply its own mask; do not bake the rounded enclosure into iOS artwork or add outer transparency.

### Android

- Regenerate every existing density-specific launcher, round-launcher, and foreground asset.
- Preserve the adaptive-icon XML structure and set its background color from the canonical yellow artwork.
- Keep the white feather inside Android's adaptive-icon safe zone so launcher masks do not crop its tip or upper vane.

### Repository And Web Surfaces

- Change the README logo to the new canonical artwork.
- Add matching favicon assets and references to both desktop and web HTML entry points.
- Remove the obsolete `apps/desktop/app-icon.svg` so the repository has one current brand source.

## Build Integration

- Continue using the existing Tauri `bundle.icon` entries for PNG, ICNS, and ICO outputs.
- Restore the imported canonical ICNS after each Tauri standard-icon generation run so the processed macOS treatment remains reproducible.
- Use pnpm for repository scripts and dependency workflows.
- Do not add another JavaScript package manager or lockfile.

## Verification

- Confirm all generated PNG dimensions, color modes, and alpha requirements.
- Inspect the ICO layer table for all required Windows sizes.
- Inspect the ICNS contents and require the complete 16px through 1024px representation set.
- Run the smallest relevant pnpm build and a macOS Tauri application build.
- Inspect the built application `Info.plist` and `Contents/Resources` to confirm `CFBundleIconFile` and the imported ICNS are present.
- Launch the built app on macOS 26.5.2 and visually verify the icon in Finder and the Dock.
- Preserve the supplied source artwork and unrelated untracked files throughout the change.

## Imported Asset Provenance

The user identified `/Volumes/extendData/Data/IdeaProjects/QingYu` as the already processed source. Its root `logo.png` is byte-identical to QingYu's supplied `logo.png`; `build/icon.icns` is byte-identical to the icon embedded in QingYu's built macOS application and contains ten RGBA representations from 16×16 through 1024×1024.
