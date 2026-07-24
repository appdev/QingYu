# QingYu Site Copy Accuracy Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace inaccurate or vague site copy with a bilingual narrative whose poetic headlines preserve QingYu's voice and whose supporting copy is backed by current editing, file, sync, MCP, Web, mobile, and export capabilities.

**Architecture:** Keep the existing Workbench page and component ownership. Update the typed content model only where a component needs localized structured copy, then keep prerender verification, SEO metadata, the font subset, and the Open Graph image derived from the same approved narrative.

**Tech Stack:** React 19, TypeScript 6, Vitest, Node test runner, Vite SSR prerendering, Sharp, FontTools.

## Global Constraints

- Preserve the existing Workbench macrostructure, N9 navigation, Ft2 footer, tokens, CSS, routes, and real screenshots.
- Do not edit `README.md`, `README.zh-CN.md`, or `macos-icon.icns`.
- Do not claim mobile public availability, Web project sync, Web MCP, or sync-as-backup.
- Do not describe the product as removing document links or double-bracket completion.
- Keep mobile store links absent.
- Use `pnpm` for all JavaScript workflows and do not add dependencies.

---

### Task 1: Lock the corrected product story in tests

**Files:**
- Modify: `apps/site/src/components/ProductSections.test.tsx`
- Modify: `apps/site/src/components/PlatformDownload.test.tsx`
- Modify: `apps/site/src/SiteApp.test.tsx`
- Modify: `apps/site/scripts/verify-build.test.mjs`

**Interfaces:**
- Consumes: `siteContent`, rendered `SiteApp`, and `verifySiteBuild`.
- Produces: failing expectations for the new hero, feature boundaries, localized sync flow, mobile status, platform CTA labels, SEO title, description, and five product principles.

- [x] **Step 1: Replace old approved-copy assertions with the new factual narrative**

Assert the restored poetic hero, ordinary Markdown positioning, six feature groups including MCP, `项目文件夹 ↔ WebDAV / S3`, desktop-only sync note, truthful mobile verification status, and poetic-but-verifiable product principles. Add negative assertions for the removed double-link and palm-writing claims.

```tsx
expect(screen.getByRole("heading", { level: 1, name: "明窗净几，字字轻语。" }))
  .toBeInTheDocument();
expect(screen.getByText("给 MCP 一扇有边界的门")).toBeInTheDocument();
expect(screen.getByLabelText("项目文件夹 ↔ WebDAV / S3 兼容存储"))
  .toBeInTheDocument();
expect(screen.getByText("原生验证中 · 尚未发布")).toBeInTheDocument();
expect(document.body).not.toHaveTextContent("掌中随手记");
expect(document.body).not.toHaveTextContent("剥离复杂的块与双链");
```

- [x] **Step 2: Run the focused tests and verify RED**

Run: `pnpm --filter @markra/site test -- src/components/ProductSections.test.tsx src/components/PlatformDownload.test.tsx src/SiteApp.test.tsx`

Expected: FAIL because production content still contains the old headline, old sync flow, old mobile copy, and old CTA labels.

### Task 2: Implement the bilingual content and localized component copy

**Files:**
- Modify: `apps/site/src/content.ts`
- Modify: `apps/site/src/components/SyncStory.tsx`
- Modify: `apps/site/src/components/PlatformDownload.tsx`
- Modify: `apps/site/src/SiteApp.tsx`

**Interfaces:**
- Consumes: the existing `SiteCopy`, `SiteHeader`, Workbench components, and centralized `siteLinks`.
- Produces: `SiteCopy.sync.flow.{local,remote,note}` and `SiteCopy.downloads.{webLabel,webAction}` plus corrected Chinese and English strings.

- [x] **Step 1: Extend only the structured fields needed by the views**

Add localized sync-flow fragments and separate Web label/action copy. Keep existing section keys so CSS and anchors remain stable.

```ts
sync: {
  label: string;
  title: string;
  body: string;
  flow: { local: string; remote: string; note: string };
  points: string[];
};
downloads: {
  label: string;
  title: string;
  platformLabels: Record<DownloadPlatform, string>;
  webLabel: string;
  webAction: string;
  release: string;
};
```

- [x] **Step 2: Replace the bilingual narrative**

Implement the exact product boundaries from the design through poetic headings and factual supporting copy: ordinary Markdown, no account, no built-in AI, real workspace tools, desktop-only MCP, project-scoped sync, verified-but-unpublished mobile, explicit desktop/Web actions, and verifiable product principles.

```ts
hero: {
  eyebrow: "一个能安心写字的地方",
  title: "明窗净几，字字轻语。",
  description: "打开一份 Markdown，文字便自然铺开。所见即所得与源码模式，写的是同一份文件；无需账号，也不把笔记困在云端。",
  download: "下载桌面版",
  web: "打开 Web 编辑器",
  previewCaption: "Web 编辑器 · 让 Markdown 像纸页一样展开"
},
mobile: {
  label: "移动端",
  title: "案头之外，轻语正在走向掌中。",
  body: "Android 模拟器与 iOS Simulator 已走通核心编辑、自动保存与恢复；完整设备验收仍在继续，正式发布还需一些时日。",
  status: "原生验证中 · 尚未发布"
}
```

- [x] **Step 3: Render localized flow and action labels**

Remove hard-coded `Local Markdown`, `Your devices`, and `Desktop · WebDAV / S3 project sync` strings from components. Render the new content fields and keep the bidirectional arrow decorative.

```tsx
<div className="sync-flow" aria-label={copy.accessibility.syncFlow}>
  <span>{copy.sync.flow.local}</span>
  <span aria-hidden="true">{" ↔ "}</span>
  <span lang="en">WebDAV / S3</span>
  <span className="visually-hidden">{copy.sync.flow.remote}</span>
</div>
<p className="sync-note">{copy.sync.flow.note}</p>
```

- [x] **Step 4: Run the focused tests and verify GREEN**

Run: `pnpm --filter @markra/site test -- src/components/ProductSections.test.tsx src/components/PlatformDownload.test.tsx src/SiteApp.test.tsx`

Expected: all focused Vitest tests pass.

### Task 3: Align prerender, metadata, social image, and font subset

**Files:**
- Modify: `apps/site/index.html`
- Modify: `apps/site/scripts/verify-build.mjs`
- Modify: `apps/site/scripts/verify-build.test.mjs`
- Modify: `apps/site/scripts/generate-images.mjs`
- Modify: `apps/site/scripts/font-glyphs.txt`
- Modify: `apps/site/public/og-image.png`
- Modify: `apps/site/public/fonts/qingyu-wenkai-subset.woff2`

**Interfaces:**
- Consumes: the new Chinese title, description, hero body, and five product-principle lines.
- Produces: matching static metadata, build verification constants, a 1200 x 630 share image, and a WOFF2 covering every static site glyph.

- [x] **Step 1: Update build-verification expectations and verify RED**

```js
const chineseHeading = "明窗净几，字字轻语。";
const chineseTitle = "轻语 QingYu｜开源 Markdown 编辑器";
const primaryDescription = "轻语是一款无需账号的开源 Markdown 编辑器，支持所见即所得、源码编辑、本地文件夹，以及桌面端 WebDAV / S3 项目同步。";
```

Run: `node --test apps/site/scripts/verify-build.test.mjs`

Expected: FAIL until the HTML fixture and verification constants both use the corrected narrative.

- [x] **Step 2: Update metadata and deterministic share-image text**

Use `轻语 QingYu｜开源 Markdown 编辑器` and the factual description in HTML, Open Graph, Twitter, JSON-LD, and the Sharp composition.

```html
<meta name="description" content="轻语是一款无需账号的开源 Markdown 编辑器，支持所见即所得、源码编辑、本地文件夹，以及桌面端 WebDAV / S3 项目同步。" />
<title>轻语 QingYu｜开源 Markdown 编辑器</title>
```

- [x] **Step 3: Update and rebuild font assets**

Regenerate the glyph list from all `siteContent` strings plus static metadata/share-image text, verify the pinned LXGW WenKai source SHA-256, then run the existing FontTools script to replace the checked-in WOFF2.

- [x] **Step 4: Regenerate the Open Graph image**

Run: `pnpm --filter @markra/site assets -- <verified-LXGW-WenKai-TTF>`

Expected: `qingyu-logo.png`, `qingyu-logo.webp`, and `og-image.png` regenerate deterministically, with the social image remaining 1200 x 630.

### Task 4: Verify the complete site and repository

**Files:**
- Verify only: all modified site files and assets.

**Interfaces:**
- Consumes: Tasks 1–3.
- Produces: automated and browser evidence for the final result.

- [x] **Step 1: Run focused site gates**

Run: `pnpm --filter @markra/site test`

Run: `pnpm --filter @markra/site typecheck:test`

Run: `pnpm --filter @markra/site build`

Expected: every command exits 0; the build verifier finds the new H1, description, core copy, principles, font, screenshots, and no mobile-store links.

- [x] **Step 2: Run the repository build**

Run: `pnpm build`

Expected: every workspace build exits 0 and vendor-chunk verification passes.

- [x] **Step 3: Run Hallmark and browser acceptance**

Confirm every Hallmark slop-test gate remains clear, then inspect 320, 375, 414, 768, and desktop widths. Verify no horizontal overflow, no wrapped primary actions, correct Chinese/English switching, and no console error.

- [x] **Step 4: Confirm workspace boundaries**

Run: `git status --short && git diff --check`

Expected: only the named site/docs scope plus the user's pre-existing README and icon changes are present; no whitespace errors.
