# QingYu Site Icon Refresh Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the site's soft or undersized logo files with appropriately sized derivatives of native 64 px and 256 px layers from `macos-icon.icns`.

**Architecture:** Track two web-specific PNG sources extracted from the ICNS, validate their dimensions in the existing image generator, and generate one 64 px favicon PNG plus one 256 px visible WebP. All visible site logos use WebP; the PNG remains favicon-only; the Open Graph composition consumes the 256 px source.

**Tech Stack:** Node.js, Sharp, React, TypeScript, Vitest, Vite, macOS `iconutil`

## Global Constraints

- Do not modify desktop, mobile, or Tauri icon assets.
- Do not commit or alter the user's root `macos-icon.icns`.
- Do not modify the user's existing README changes.
- Do not change site layout, copy, colors, or typography.
- Use `pnpm` for all JavaScript workflows.
- Do not use the TypeScript `void` keyword or operator.

---

### Task 1: Add deterministic Web icon sources and generation boundaries

**Files:**
- Create: `assets/branding/app-icon/web-icon-64.png`
- Create: `assets/branding/app-icon/web-icon-256.png`
- Create: `assets/branding/app-icon/WEB_SOURCE.md`
- Create: `apps/site/src/brand-assets.test.ts`
- Modify: `apps/site/scripts/generate-images.mjs`
- Regenerate: `apps/site/public/qingyu-logo.png`
- Regenerate: `apps/site/public/qingyu-logo.webp`
- Regenerate: `apps/site/public/og-image.png`

**Interfaces:**
- Consumes: native `icon_32x32@2x.png` and `icon_256x256.png` layers extracted from `/Volumes/extendData/Data/IdeaProjects/markra/macos-icon.icns`.
- Produces: a 64 × 64 favicon PNG, a 256 × 256 visible WebP, and an unchanged-size 1200 × 630 Open Graph PNG.

- [ ] **Step 1: Write the failing asset boundary test**

Create `apps/site/src/brand-assets.test.ts` using `sharp(...).metadata()` to assert these exact dimensions and formats:

```ts
const expectedAssets = [
  ["assets/branding/app-icon/web-icon-64.png", 64, 64, "png"],
  ["assets/branding/app-icon/web-icon-256.png", 256, 256, "png"],
  ["apps/site/public/qingyu-logo.png", 64, 64, "png"],
  ["apps/site/public/qingyu-logo.webp", 256, 256, "webp"],
  ["apps/site/public/og-image.png", 1200, 630, "png"]
] as const;
```

Resolve every path from the repository root and assert `width`, `height`, and `format` with one parameterized test.

- [ ] **Step 2: Run the boundary test and verify RED**

Run:

```bash
pnpm --filter @markra/site exec vitest run src/brand-assets.test.ts
```

Expected: FAIL because both web source assets are missing and the public PNG is 32 × 32 instead of 64 × 64.

- [ ] **Step 3: Extract the approved native ICNS layers**

From the primary checkout, extract the ICNS into a temporary directory with `iconutil -c iconset`. Copy the native `icon_32x32@2x.png` bytes to `web-icon-64.png` and `icon_256x256.png` bytes to `web-icon-256.png` in the isolated worktree. Do not edit or stage `macos-icon.icns`.

Create `WEB_SOURCE.md` containing:

```md
# QingYu Web Icon Sources

Extracted from repository-root `macos-icon.icns` on 2026-07-20.

- Source SHA-256: `e3b34a0159027cd09c7d501924a6f43434e402c7d64452794a40dca5dd8f5444`
- `web-icon-64.png`: native `icon_32x32@2x.png`
- `web-icon-256.png`: native `icon_256x256.png`

Extraction: `iconutil -c iconset macos-icon.icns -o <temporary-directory>/qingyu.iconset`
```

- [ ] **Step 4: Update the generator with exact source validation**

In `apps/site/scripts/generate-images.mjs`, replace the generic platform master with `web-icon-64.png` and `web-icon-256.png`. Add:

```js
async function requireImageDimensions(path, width, height) {
  const metadata = await sharp(path).metadata();
  if (metadata.width !== width || metadata.height !== height) {
    throw new Error(`Expected ${path} to be ${width}x${height}, received ${metadata.width}x${metadata.height}.`);
  }
}
```

Validate 64 × 64 and 256 × 256 before writing. Produce the favicon as lossless PNG from the 64 px source, produce the visible icon as 256 px WebP with `quality: 92`, `alphaQuality: 100`, and `smartSubsample: true`, and composite the Open Graph icon from the 256 px source resized to 148 px.

- [ ] **Step 5: Regenerate the public assets**

Run:

```bash
pnpm --filter @markra/site assets /tmp/LXGWWenKaiGBLite-Regular-v1.522.ttf
```

Expected: `qingyu-logo.png`, `qingyu-logo.webp`, and `og-image.png` are rewritten without errors.

- [ ] **Step 6: Run the boundary test and verify GREEN**

Run:

```bash
pnpm --filter @markra/site exec vitest run src/brand-assets.test.ts
```

Expected: all five asset cases pass.

- [ ] **Step 7: Commit the asset pipeline**

```bash
git add assets/branding/app-icon/web-icon-64.png assets/branding/app-icon/web-icon-256.png assets/branding/app-icon/WEB_SOURCE.md apps/site/src/brand-assets.test.ts apps/site/scripts/generate-images.mjs apps/site/public/qingyu-logo.png apps/site/public/qingyu-logo.webp apps/site/public/og-image.png
git commit -m "fix(site): generate crisp web icons"
```

### Task 2: Use the 256 px WebP for every visible logo

**Files:**
- Modify: `apps/site/src/components/SiteHeader.test.tsx`
- Modify: `apps/site/src/components/PlatformDownload.test.tsx`
- Modify: `apps/site/src/components/ProductSections.test.tsx`
- Modify: `apps/site/src/components/SiteHeader.tsx`
- Modify: `apps/site/src/components/PlatformDownload.tsx`
- Modify: `apps/site/src/components/MobilePreview.tsx`

**Interfaces:**
- Consumes: `/qingyu-logo.webp` from Task 1.
- Produces: visible logo markup that never uses the 64 px favicon PNG.

- [ ] **Step 1: Change focused expectations to WebP**

Update header, download-card, and mobile-preview tests so every visible decorative logo is expected to use `/qingyu-logo.webp`. Keep the empty `alt` requirements unchanged. Add an assertion that `/qingyu-logo.png` does not appear in rendered component markup.

- [ ] **Step 2: Run the focused tests and verify RED**

Run:

```bash
pnpm --filter @markra/site exec vitest run src/components/SiteHeader.test.tsx src/components/PlatformDownload.test.tsx src/components/ProductSections.test.tsx
```

Expected: FAIL while `SiteHeader`, `PlatformDownload`, and `MobilePreview` still reference `/qingyu-logo.png`.

- [ ] **Step 3: Switch visible components to WebP**

Replace each visible component source:

```tsx
<img src="/qingyu-logo.webp" alt="" />
```

Do not change accessible names, layout markup, or CSS.

- [ ] **Step 4: Run focused tests and verify GREEN**

Run the same focused Vitest command. Expected: all focused tests pass.

- [ ] **Step 5: Commit component references**

```bash
git add apps/site/src/components/SiteHeader.test.tsx apps/site/src/components/PlatformDownload.test.tsx apps/site/src/components/ProductSections.test.tsx apps/site/src/components/SiteHeader.tsx apps/site/src/components/PlatformDownload.tsx apps/site/src/components/MobilePreview.tsx
git commit -m "fix(site): serve high-density visible icons"
```

### Task 3: Verify the refreshed site

**Files:**
- Verify only; no planned production edits.

**Interfaces:**
- Consumes: the committed asset pipeline and visible component references from Tasks 1–2.
- Produces: fresh automated, build, and browser evidence.

- [ ] **Step 1: Run all site gates**

```bash
pnpm --filter @markra/site test
pnpm --filter @markra/site typecheck:test
pnpm --filter @markra/site build
```

Expected: zero failures, and the build verifier lists a 64 px PNG plus 256 px WebP asset.

- [ ] **Step 2: Inspect generated images**

Open `qingyu-logo.png`, `qingyu-logo.webp`, and `og-image.png` at original detail. Confirm sharp feather edges, intact rounded corners, no added crop, and no halo around the icon.

- [ ] **Step 3: Verify in a production browser preview**

Start Vite preview from the built site. At 390 px and 1440 px widths, verify the header, hero, mobile preview, and download cards use the crisp WebP asset, have no overflow, and produce zero console warnings or errors.

- [ ] **Step 4: Run repository integration gates**

```bash
pnpm test
pnpm typecheck:test
pnpm build
```

Expected: zero failures across the workspace.

- [ ] **Step 5: Review scope and history**

Run `git diff --check`, confirm the two README files and root `macos-icon.icns` were not staged or modified by the feature branch, and review all commits since the implementation base.
