# QingYu Site Workbench Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Completed steps are marked with `- [x]`.

**Goal:** Redesign the existing bilingual QingYu product site around real product screenshots and the locked QingYu design system while preserving all product, SEO, locale, download, and accessibility behavior.

**Architecture:** Keep the existing single-page React/Vite application and component ownership. Replace the visual layer with a Workbench macrostructure, add a site token module and a documented marketing-site variant, and repurpose the current sections into screenshot-led narratives without deleting route or component files.

**Tech Stack:** pnpm workspace, React 19, TypeScript 6, Vite 8, Tailwind CSS 4, Vitest, Testing Library, static SSR prerendering.

## Global Constraints

- Use `pnpm`; add no dependency or lockfile.
- Preserve the current route, locale persistence, SSR, SEO, platform detection, external URLs, manifesto wording, sync claims, and unreleased-mobile restrictions.
- Use real captures from the current `@markra/web` UI; do not draw browser, editor, IDE, or phone chrome.
- Keep the root design system authoritative and add an explicit marketing-site variant before styling the page.
- Every color and `font-family` in page CSS comes through `apps/site/src/tokens.css`.
- Use a 4pt spacing scale and named timing/easing tokens.
- Verify 320, 375, 414, 768 and desktop widths with no horizontal overflow.
- Preserve unrelated README and root icon changes.
- Do not delete production component files.

---

### Task 1: Lock the Redesign Contract and Style Gates

**Files:**
- Modify: `design.md`
- Create: `apps/site/src/tokens.css`
- Modify: `apps/site/src/styles.test.ts`
- Modify: `apps/site/src/styles.css`
- Create: `.hallmark/log.json`

**Interfaces:**
- Produces: a site token namespace imported once by `styles.css`.
- Produces: a Hallmark stamp naming `editorial`, `Workbench`, N9, Ft2, and `design.md`.

- [x] **Step 1: Write failing structural style tests**

Add tests that require:

```ts
expect(siteStyles.startsWith("/* Hallmark · genre: editorial · macrostructure: Workbench"))
  .toBe(true);
expect(siteStyles).toContain('@import "./tokens.css";');
expect(siteStyles).toMatch(/html[^{]*\{[^}]*overflow-x:\s*clip/isu);
expect(siteStyles).toMatch(/body[^{]*\{[^}]*overflow-x:\s*clip/isu);
expect(siteStyles).not.toMatch(/radial-gradient|overflow-x:\s*hidden|\btransition-all\b/iu);
```

- [x] **Step 2: Run the focused test and confirm RED**

Run: `pnpm --filter @markra/site test -- src/styles.test.ts`

Expected: FAIL because the Hallmark stamp, token import, and root clip contract do not exist.

- [x] **Step 3: Add the site variant and token module**

Amend `design.md` with the marketing-site palette, typography roles, Workbench family, N9/Ft2 voice, interaction rules, and four token export formats. Create `tokens.css` with all colors, fonts, spacing, type, timing, easing, radius, rule, and z-index values. Update `styles.css` to import the file and satisfy the root safety rules.

- [x] **Step 4: Run the focused test and confirm GREEN**

Run: `pnpm --filter @markra/site test -- src/styles.test.ts`

Expected: PASS.

### Task 2: Replace the AI Navigation and Fake Hero UI

**Files:**
- Modify: `apps/site/src/components/SiteHeader.test.tsx`
- Modify: `apps/site/src/components/SiteHeader.tsx`
- Modify: `apps/site/src/components/ProductSections.test.tsx`
- Modify: `apps/site/src/components/Hero.tsx`
- Modify: `apps/site/src/components/NewsprintEditorPreview.tsx`
- Modify: `apps/site/src/content.ts`
- Modify: `apps/site/src/styles.css`

**Interfaces:**
- `SiteHeader` exposes only the N9 brand/action rail on desktop and the existing accessible compact menu on mobile.
- `NewsprintEditorPreview` renders the real `/product-editor-light.jpg` image with fixed intrinsic dimensions.

- [x] **Step 1: Write failing header and hero tests**

Require the desktop primary navigation to be absent, the release and Web editor actions to remain centralized, and the Hero image contract to be:

```tsx
expect(within(product).getByRole("img", { name: copy.accessibility.editorPreview }))
  .toHaveAttribute("src", "/product-editor-light.jpg");
expect(within(product).getByRole("img", { name: copy.accessibility.editorPreview }))
  .toHaveAttribute("width", "1440");
expect(within(product).getByRole("img", { name: copy.accessibility.editorPreview }))
  .toHaveAttribute("height", "900");
```

- [x] **Step 2: Run focused tests and confirm RED**

Run: `pnpm --filter @markra/site test -- src/components/SiteHeader.test.tsx src/components/ProductSections.test.tsx`

Expected: FAIL on the old six-link desktop navigation and CSS-built Newsprint figure.

- [x] **Step 3: Implement N9 and the real screenshot hero**

Keep the locale button, download link and mobile disclosure behavior. Replace the Hero figure contents with a real image and caption, set `fetchPriority="high"`, and remove inline preview tokens and fake aside/article chrome.

- [x] **Step 4: Run focused tests and confirm GREEN**

Run the same focused command; expected PASS.

### Task 3: Turn Feature Cards into a Product Walkthrough

**Files:**
- Modify: `apps/site/src/components/FeatureGrid.tsx`
- Modify: `apps/site/src/components/Personalization.tsx`
- Modify: `apps/site/src/components/SyncStory.tsx`
- Modify: `apps/site/src/components/ProductSections.test.tsx`
- Modify: `apps/site/src/content.ts`
- Modify: `apps/site/src/styles.css`

**Interfaces:**
- `FeatureGrid` becomes a screenshot-led article using `/product-editor-split.jpg`.
- `Personalization` becomes a screenshot-led article using `/product-appearance.jpg`.
- The export capability is shown with `/product-export.jpg` while the sync section remains factual prose/data flow.

- [x] **Step 1: Write failing screenshot-tour tests**

Assert the three real screenshot paths, localized alt/captions, `loading="lazy"` below the fold, and absence of feature SVG icons.

- [x] **Step 2: Run focused tests and confirm RED**

Run: `pnpm --filter @markra/site test -- src/components/ProductSections.test.tsx`

Expected: FAIL because the current feature section is an icon-card grid and personalization has no screenshot.

- [x] **Step 3: Implement the three Workbench narratives**

Render each narrative as a semantic article with one text column, one real image figure and an external caption/annotation list. Use different alignment and spacing for each section; do not put screenshots inside cards or fake frames.

- [x] **Step 4: Run focused tests and confirm GREEN**

Run the focused test; expected PASS.

### Task 4: Flatten Mobile, Downloads, Manifesto and Footer

**Files:**
- Modify: `apps/site/src/components/MobilePreview.tsx`
- Modify: `apps/site/src/components/PlatformDownload.tsx`
- Modify: `apps/site/src/components/PlatformDownload.test.tsx`
- Modify: `apps/site/src/components/FullManifesto.tsx`
- Modify: `apps/site/src/components/OpenSourceSection.tsx`
- Modify: `apps/site/src/components/SiteFooter.tsx`
- Modify: `apps/site/src/components/SiteFooter.test.tsx`
- Modify: `apps/site/src/styles.css`

**Interfaces:**
- Mobile remains text-only and has no link/button.
- Download entries retain `data-platform` and `data-preferred`, but render as ruled rows rather than cards.
- Footer retains every centralized URL in one inline-rule close.

- [x] **Step 1: Write failing structure tests**

Require the mobile section to contain no role=img, download items to contain no repeated logo image, and the footer to expose a single navigation landmark while retaining every link.

- [x] **Step 2: Run focused tests and confirm RED**

Run: `pnpm --filter @markra/site test -- src/components/ProductSections.test.tsx src/components/PlatformDownload.test.tsx src/components/SiteFooter.test.tsx`

Expected: FAIL on the fake phone, logo-card downloads and multi-column footer landmarks.

- [x] **Step 3: Implement the flattened close**

Repurpose the existing components without deleting files. Preserve manifesto line order, platform sorting, preferred-platform data, unreleased mobile restrictions and every external destination.

- [x] **Step 4: Run focused tests and confirm GREEN**

Run the same command; expected PASS.

### Task 5: Full Verification and Browser QA

**Files:**
- Verify: `apps/site/**`
- Verify: repository root build

- [x] **Step 1: Run automated verification**

Run:

```bash
pnpm --filter @markra/site test
pnpm --filter @markra/site typecheck:test
pnpm --filter @markra/site build
pnpm build
```

Expected: every command exits 0 and build verification includes the real screenshot assets.

- [x] **Step 2: Run the Hallmark slop test**

Load `references/slop-test.md`, audit the emitted page, fix every open gate, and rerun the focused tests/build after any correction.

- [x] **Step 3: Verify the live site**

Run the site dev server and inspect 320, 375, 414, 768 and 1280 widths. Confirm no horizontal overflow, no wrapped CTA/nav/footer labels, correct screenshot loading, Hero fold fit, mobile menu open/Escape close, locale switch, and no browser warnings/errors.

- [x] **Step 4: Confirm workspace scope**

Run `git status --short` and `git diff --check`. Confirm only the approved site, design, Hallmark, spec, plan and screenshot assets changed; leave existing README and root icon changes untouched.
