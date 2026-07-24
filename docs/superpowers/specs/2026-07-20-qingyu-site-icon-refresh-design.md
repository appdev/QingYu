# QingYu Site Icon Refresh Design

Date: 2026-07-20

## Goal

Replace the product site's low-resolution logo assets with web-appropriate derivatives of the repository-root `macos-icon.icns` without shipping its largest 1024 px layer to browsers. Preserve the existing desktop application icon pipeline.

## Current Evidence

- `macos-icon.icns` contains native 64, 128, 256, 512, and 1024 px raster layers.
- The current site PNG is only 32 × 32 px.
- The current site WebP is 256 × 256 px but was resized from the generic 1024 px platform master and shows a softer edge treatment than the native 256 px ICNS layer.
- The largest site rendering is 64 CSS px, so a 256 px visible asset covers screens up to 4× device pixel density.

## Asset Architecture

Extract two deterministic web source assets from native ICNS layers:

- `assets/branding/app-icon/web-icon-64.png`: the native 64 × 64 layer, used to produce the favicon PNG.
- `assets/branding/app-icon/web-icon-256.png`: the native 256 × 256 layer, used to produce the visible WebP logo and the icon inside the Open Graph image.

Record the source ICNS checksum and extraction command beside these assets. The untracked root ICNS remains the user's source file and is not required by site builds after extraction.

Generated public assets remain:

- `apps/site/public/qingyu-logo.png`: 64 × 64 lossless PNG, favicon only.
- `apps/site/public/qingyu-logo.webp`: 256 × 256 high-quality WebP, used by every visible site logo.
- `apps/site/public/og-image.png`: regenerated from the 256 px web source, with the existing 1200 × 630 composition preserved.

No additional crop is applied. The ICNS-native layers already contain the intended rounded-square silhouette and safe area.

## Component Changes

`SiteHeader`, `PlatformDownload`, and `MobilePreview` will switch their visible decorative logo source from the favicon PNG to the 256 px WebP. `Hero` already uses WebP and keeps that URL. `index.html` continues to use the PNG as the favicon.

This keeps one small lossless browser icon and one appropriately oversampled visible image without adding `srcset` complexity for images that never exceed 64 CSS px.

## Generation and Error Handling

The existing site image generator will read the two tracked web source assets instead of the generic 1024 px platform master. It will validate source dimensions before writing public files and fail with a clear message if either source is missing or has the wrong dimensions.

The font argument and Open Graph typography workflow remain unchanged.

## Verification

Use test-driven development:

1. Add a failing asset-boundary test that requires 64 × 64 PNG and 256 × 256 WebP outputs.
2. Update focused component tests first so they fail while visible components still reference the PNG.
3. Extract and add the approved ICNS-native sources, update generation and component references, and regenerate outputs.
4. Run focused tests, all site tests, type checking, and the production build.
5. Inspect the regenerated PNG, WebP, and Open Graph image, then verify the live site at mobile and desktop widths with no console errors.

## Scope Boundaries

- Do not modify desktop, mobile, or Tauri icon assets.
- Do not commit or alter the user's root `macos-icon.icns`.
- Do not modify the user's existing README changes.
- Do not change site layout, copy, colors, or typography.
