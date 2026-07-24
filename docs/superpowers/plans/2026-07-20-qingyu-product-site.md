# QingYu Product Site Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a fast, bilingual, single-page QingYu product website in a new independent `apps/site` workspace package.

**Architecture:** `apps/site` is a React/Vite/Tailwind static application that owns only marketing content and lightweight product mockups. Chinese content is rendered into the production HTML at build time, React hydrates it for language switching and navigation, and the site never imports editor, Tauri, sync, or desktop runtime packages.

**Tech Stack:** pnpm 10.30.3, React 19.2.5, TypeScript 6.0.3, Vite 8.0.10, Tailwind CSS 4.2.4, Vitest 4.1.5, Testing Library 16.3.2, lucide-react 1.14.0, FontTools through `uvx` for the committed WOFF2 subset.

## Global Constraints

- Use `pnpm` for every JavaScript dependency and workspace command; keep `pnpm-lock.yaml` and add no other lockfile.
- Create the site in `apps/site`; do not route the product website through `apps/web`.
- Do not import `@markra/app`, `@markra/editor`, Milkdown, CodeMirror, Mermaid, KaTeX, or any `@tauri-apps/*` package into the site.
- Do not use the TypeScript `void` keyword or operator.
- Keep Simplified Chinese as the initial and statically rendered locale; English is a manual client-side switch persisted in `localStorage`.
- The primary CTA is desktop download; the secondary CTA opens `https://editor.markra.app/`.
- Android and iOS must say “即将推出” / “Coming soon” and must not have download links.
- Use the repository's QingYu feather icon, the brand amber accent, and the exact existing Newsprint theme values from the approved design.
- Use a self-hosted Regular-only LXGW WenKai GB Lite subset derived from official release `v1.522`; the source TTF SHA-256 is `1675c708cce181871d9a8adc987f35a0cabc6ff980685cd99f05d2655ea08c4c`.
- Rename the derived font internally to `QingYu WenKai Subset`, ship OFL-1.1 and source metadata, preload one WOFF2 file, and never copy the original TTF into source control or `dist/`.
- Preserve unrelated working-tree changes. Do not stage README, root `package.json`, or unrelated script changes with site commits.
- Before Task 1, create an isolated feature worktree with `superpowers:using-git-worktrees`; the primary checkout already contains unrelated user changes.

---

## Planned File Structure

| Path | Responsibility |
| --- | --- |
| `apps/site/package.json` | Site-only scripts and dependencies |
| `apps/site/tsconfig.json` | App/test project references |
| `apps/site/tsconfig.app.json` | Browser and SSR TypeScript settings |
| `apps/site/tsconfig.test.json` | Vitest and Testing Library types |
| `apps/site/vite.config.ts` | React, Tailwind, test setup, and asset build config |
| `apps/site/index.html` | Static Chinese metadata and hydration root |
| `apps/site/src/main.tsx` | Browser hydration entrypoint |
| `apps/site/src/prerender.tsx` | Server-rendered Chinese markup entrypoint |
| `apps/site/src/SiteApp.tsx` | Locale state and page composition |
| `apps/site/src/content.ts` | Complete type-safe Chinese/English content |
| `apps/site/src/links.ts` | External URLs and platform download models |
| `apps/site/src/lib/locale.ts` | Safe locale read/write helpers |
| `apps/site/src/lib/platform.ts` | Pure browser-platform detection and ordering |
| `apps/site/src/lib/browser-node-stub.ts` | Site-local empty Vite alias target with no app-package coupling |
| `apps/site/src/components/*.tsx` | Focused single-page sections |
| `apps/site/src/styles.css` | Tailwind import, tokens, Newsprint mockup, responsive and motion CSS |
| `apps/site/src/test/setup.ts` | jest-dom registration and browser API fakes |
| `apps/site/src/**/*.test.ts(x)` | Unit and component tests |
| `apps/site/scripts/prerender-lib.mjs` | Pure HTML root injection helper |
| `apps/site/scripts/prerender.mjs` | Inject SSR output into `dist/index.html` |
| `apps/site/scripts/verify-build.mjs` | Production HTML/chunk/font verification |
| `apps/site/scripts/build-font-subset.py` | Reproducible font subsetting and internal rename |
| `apps/site/scripts/font-glyphs.txt` | Exact glyph source for the WOFF2 subset |
| `apps/site/scripts/generate-images.mjs` | Deterministic brand-logo and Open Graph image generation |
| `apps/site/public/fonts/qingyu-wenkai-subset.woff2` | Committed webfont subset |
| `apps/site/public/fonts/SOURCE.md` | Font version, checksum, generation command, and provenance |
| `apps/site/public/licenses/OFL-LXGW-WenKai-GB-Lite.txt` | Required font license |
| `apps/site/public/qingyu-logo.png` | Existing optimized 32px app icon for favicon/navigation fallback |
| `apps/site/public/qingyu-logo.webp` | Optimized larger brand image |
| `apps/site/public/og-image.png` | 1200x630 approved hero share image |

---

### Task 1: Scaffold the Independent Site Package

**Files:**
- Create: `apps/site/package.json`
- Create: `apps/site/tsconfig.json`
- Create: `apps/site/tsconfig.app.json`
- Create: `apps/site/tsconfig.test.json`
- Create: `apps/site/vite.config.ts`
- Create: `apps/site/index.html`
- Create: `apps/site/src/vite-env.d.ts`
- Create: `apps/site/src/test/setup.ts`
- Create: `apps/site/src/lib/browser-node-stub.ts`
- Create: `apps/site/src/SiteApp.test.tsx`
- Create: `apps/site/src/SiteApp.tsx`
- Create: `apps/site/src/main.tsx`
- Create: `apps/site/src/styles.css`
- Modify: `pnpm-lock.yaml`

**Interfaces:**
- Produces: `SiteApp({ initialLocale }: { initialLocale: SiteLocale }): ReactElement` for the client and prerender entrypoints.
- Produces: a standalone `@markra/site` package discovered automatically by the existing `apps/*` workspace glob.
- Consumes: repository versions and Vite helper from `@markra/scripts/vite`.

- [ ] **Step 1: Create package/config files and the failing smoke test**

Create `apps/site/package.json` exactly with site-only dependencies:

```json
{
  "name": "@markra/site",
  "version": "1.7.4",
  "license": "AGPL-3.0-only",
  "private": true,
  "type": "module",
  "scripts": {
    "assets": "node scripts/generate-images.mjs",
    "dev": "vite",
    "build": "tsc -p tsconfig.app.json && vite build",
    "test": "vitest run",
    "test:watch": "vitest",
    "typecheck:test": "tsc -p tsconfig.test.json --noEmit"
  },
  "dependencies": {
    "lucide-react": "1.14.0",
    "react": "19.2.5",
    "react-dom": "19.2.5"
  },
  "devDependencies": {
    "@markra/scripts": "workspace:*",
    "@tailwindcss/vite": "^4.2.4",
    "@testing-library/jest-dom": "6.9.1",
    "@testing-library/react": "16.3.2",
    "@types/node": "24.10.0",
    "@types/react": "19.2.7",
    "@types/react-dom": "19.2.3",
    "@vitejs/plugin-react": "6.0.1",
    "jsdom": "29.1.1",
    "sharp": "^0.35.3",
    "tailwindcss": "^4.2.4",
    "typescript": "6.0.3",
    "vite": "8.0.10",
    "vitest": "4.1.5"
  }
}
```

Copy the `apps/web` TypeScript project-reference shape, changing test types to include `@testing-library/jest-dom`. Configure Vite through the shared helper:

```ts
import { createMarkraAppViteConfig } from "@markra/scripts/vite";

export default createMarkraAppViteConfig({
  browserNodeStubUrl: new URL("./src/lib/browser-node-stub.ts", import.meta.url),
  packageJsonUrl: new URL("./package.json", import.meta.url),
  stripDebug: false,
  test: { setupFiles: "./src/test/setup.ts" }
});
```

Create `src/lib/browser-node-stub.ts` with only `export {};`; the site must not depend on the equivalent stub inside `@markra/app`.

Register jest-dom in `src/test/setup.ts`:

```ts
import * as matchers from "@testing-library/jest-dom/matchers";
import { afterEach, expect, vi } from "vitest";

expect.extend(matchers);

afterEach(() => {
  window.localStorage.clear();
  document.documentElement.lang = "zh-CN";
  vi.unstubAllGlobals();
});
```

Write `SiteApp.test.tsx` before creating `SiteApp.tsx`:

```tsx
import { render, screen } from "@testing-library/react";
import { SiteApp } from "./SiteApp";

describe("SiteApp", () => {
  it("renders the approved Chinese product promise", () => {
    render(<SiteApp initialLocale="zh-CN" />);

    expect(screen.getByRole("heading", {
      level: 1,
      name: "明窗净几，字字轻语。"
    })).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Install workspace dependencies and verify the test fails**

Run:

```bash
pnpm install
pnpm --filter @markra/site test -- src/SiteApp.test.tsx
```

Expected: dependency installation updates only `pnpm-lock.yaml`; the test fails because `./SiteApp` does not exist.

- [ ] **Step 3: Add the smallest hydratable app**

Create `SiteApp.tsx` and `main.tsx`:

```tsx
import type { ReactElement } from "react";

export type SiteLocale = "zh-CN" | "en";

export function SiteApp({ initialLocale }: { initialLocale: SiteLocale }): ReactElement {
  const title = initialLocale === "zh-CN"
    ? "明窗净几，字字轻语。"
    : "A clear desk. A quiet room. Every word softly spoken.";

  return <main><h1>{title}</h1></main>;
}
```

```tsx
import { StrictMode } from "react";
import { createRoot, hydrateRoot } from "react-dom/client";
import { SiteApp } from "./SiteApp";
import "./styles.css";

const root = document.getElementById("root");
if (!root) throw new Error("Missing QingYu site root.");

const app = <StrictMode><SiteApp initialLocale="zh-CN" /></StrictMode>;
if (root.hasChildNodes()) {
  hydrateRoot(root, app);
} else {
  createRoot(root).render(app);
}
```

Create `styles.css` with `@import "tailwindcss";`, and create `index.html` with Chinese title/description, `/qingyu-logo.png` favicon, one `<div id="root"></div>`, and `/src/main.tsx` module entry.

- [ ] **Step 4: Run focused verification**

Run:

```bash
pnpm --filter @markra/site test -- src/SiteApp.test.tsx
pnpm --filter @markra/site typecheck:test
pnpm --filter @markra/site build
```

Expected: one test passes, TypeScript exits 0, and Vite creates `apps/site/dist/index.html`.

- [ ] **Step 5: Commit the package skeleton**

```bash
git add apps/site pnpm-lock.yaml
git commit -m "feat(site): scaffold product website"
```

---

### Task 2: Add Type-Safe Bilingual Content and Platform Logic

**Files:**
- Create: `apps/site/src/content.ts`
- Create: `apps/site/src/links.ts`
- Create: `apps/site/src/links.test.ts`
- Create: `apps/site/src/lib/locale.ts`
- Create: `apps/site/src/lib/locale.test.ts`
- Create: `apps/site/src/lib/platform.ts`
- Create: `apps/site/src/lib/platform.test.ts`
- Modify: `apps/site/src/SiteApp.tsx`

**Interfaces:**
- Produces: `SiteLocale = "zh-CN" | "en"` and `siteContent: Record<SiteLocale, SiteCopy>`.
- Produces: `readStoredLocale(storage): SiteLocale`, `writeStoredLocale(storage, locale): boolean`.
- Produces: `DownloadPlatform = "macos" | "windows" | "linux"`, `detectDownloadPlatform(input): DownloadPlatform | null`, and `orderDownloadPlatforms(preferred): DownloadPlatform[]`.
- Produces: `siteLinks` as the single source for external URLs.

- [ ] **Step 1: Write failing locale and platform tests**

```ts
import { localeStorageKey, readStoredLocale, writeStoredLocale } from "./locale";

describe("site locale storage", () => {
  it("defaults to Chinese and accepts only known locale values", () => {
    const storage = new Map<string, string>();
    const adapter = {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => storage.set(key, value)
    };

    expect(readStoredLocale(adapter)).toBe("zh-CN");
    storage.set(localeStorageKey, "fr");
    expect(readStoredLocale(adapter)).toBe("zh-CN");
    expect(writeStoredLocale(adapter, "en")).toBe(true);
    expect(readStoredLocale(adapter)).toBe("en");
  });

  it("survives unavailable storage", () => {
    const storage = {
      getItem: () => { throw new Error("blocked"); },
      setItem: () => { throw new Error("blocked"); }
    };

    expect(readStoredLocale(storage)).toBe("zh-CN");
    expect(writeStoredLocale(storage, "en")).toBe(false);
  });
});
```

```ts
import { detectDownloadPlatform, orderDownloadPlatforms } from "./platform";

describe("download platform", () => {
  it.each([
    ["MacIntel", "macos"],
    ["Win32", "windows"],
    ["Linux x86_64", "linux"],
    ["iPhone", null]
  ])("maps %s to %s", (platform, expected) => {
    expect(detectDownloadPlatform({ platform })).toBe(expected);
  });

  it("moves the detected desktop platform first without hiding others", () => {
    expect(orderDownloadPlatforms("windows")).toEqual(["windows", "macos", "linux"]);
    expect(orderDownloadPlatforms(null)).toEqual(["macos", "windows", "linux"]);
  });
});
```

Write `links.test.ts` before `links.ts` and assert the exact release, Web editor, GitHub, documentation, privacy, changelog, contributing, and license URLs shown in Step 4. This locks every external destination to the single `siteLinks` contract.

- [ ] **Step 2: Run the focused tests to verify failure**

Run:

```bash
pnpm --filter @markra/site test -- src/lib/locale.test.ts src/lib/platform.test.ts src/links.test.ts
```

Expected: FAIL because the locale, platform, and link modules are missing.

- [ ] **Step 3: Implement safe locale and platform helpers**

```ts
import type { SiteLocale } from "../content";

export const localeStorageKey = "qingyu.site.locale";

type LocaleStorage = Pick<Storage, "getItem" | "setItem">;

export function readStoredLocale(storage: Pick<LocaleStorage, "getItem">): SiteLocale {
  try {
    return storage.getItem(localeStorageKey) === "en" ? "en" : "zh-CN";
  } catch {
    return "zh-CN";
  }
}

export function writeStoredLocale(storage: Pick<LocaleStorage, "setItem">, locale: SiteLocale) {
  try {
    storage.setItem(localeStorageKey, locale);
    return true;
  } catch {
    return false;
  }
}
```

```ts
export type DownloadPlatform = "macos" | "windows" | "linux";
type NavigatorPlatform = { platform?: string; userAgentData?: { platform?: string } };

export function detectDownloadPlatform(input: NavigatorPlatform): DownloadPlatform | null {
  const platform = input.userAgentData?.platform ?? input.platform ?? "";
  if (/mac/i.test(platform)) return "macos";
  if (/win/i.test(platform)) return "windows";
  if (/linux/i.test(platform) && !/android/i.test(platform)) return "linux";
  return null;
}

export function orderDownloadPlatforms(preferred: DownloadPlatform | null): DownloadPlatform[] {
  const platforms: DownloadPlatform[] = ["macos", "windows", "linux"];
  return preferred ? [preferred, ...platforms.filter((item) => item !== preferred)] : platforms;
}
```

- [ ] **Step 4: Add the complete content and link contracts**

Define `SiteCopy` with required fields for navigation, hero, personality, features, personalization, sync, mobile, downloads, manifesto, open-source section, and footer. Use `satisfies Record<SiteLocale, SiteCopy>` so a missing English or Chinese field is a compile error.

The Chinese hero and manifesto strings must be exact:

```ts
import type { DownloadPlatform } from "./lib/platform";

export type SiteLocale = "zh-CN" | "en";

export type SiteCopy = {
  languageLabel: string;
  nav: { product: string; features: string; sync: string; mobile: string; manifesto: string; download: string };
  hero: { eyebrow: string; title: string; description: string; download: string; web: string };
  personality: { label: string; title: string; body: string[] };
  features: { label: string; title: string; items: Array<{ title: string; body: string }> };
  personalization: { label: string; title: string; body: string; items: string[] };
  sync: { label: string; title: string; body: string; points: string[] };
  mobile: { label: string; title: string; body: string; status: string };
  downloads: { label: string; title: string; platformLabels: Record<DownloadPlatform, string>; web: string; release: string };
  manifesto: { label: string; lines: string[] };
  openSource: { label: string; title: string; body: string; github: string; docs: string };
  footer: { privacy: string; changelog: string; contribute: string; license: string };
};

export const siteContent = {
  "zh-CN": {
    languageLabel: "English",
    nav: { product: "产品", features: "功能", sync: "同步", mobile: "移动端", manifesto: "产品宣言", download: "下载" },
    hero: {
      eyebrow: "一个能安心写字的地方",
      title: "明窗净几，字字轻语。",
      description: "不建第二大脑，只给文字留一方安静。桌面精心写，掌中随手记。",
      download: "免费下载桌面版",
      web: "打开 Web 编辑器"
    },
    personality: {
      label: "轻语的选择",
      title: "不做第二大脑，只认真写字。",
      body: ["剥离复杂的块与双链，回到一篇文章自然展开的节奏。", "没有 AI 负担，没有专有格式。你的笔记始终是普通 Markdown 文件。"]
    },
    features: {
      label: "写作体验",
      title: "简单，不等于简陋。",
      items: [
        { title: "两种编辑方式", body: "在所见即所得与源码模式之间自由切换，底层始终保持 Markdown。" },
        { title: "清楚的文件结构", body: "管理普通文件与文件夹，用标签页、分栏、快速打开、工作区搜索和大纲保持有序。" },
        { title: "丰富但克制的 Markdown", body: "直接呈现链接、图片、HTML、GFM 表格、KaTeX、Mermaid、提示块与代码高亮。" },
        { title: "可靠的日常写作", body: "自动保存、标签页与工作区恢复、全文和选中文字数，让写作自然延续。" },
        { title: "普通资源，自由导出", body: "图片留在普通 assets 文件夹；按需导出 HTML、PDF 或配置 Pandoc 后的更多格式。" }
      ]
    },
    personalization: {
      label: "个性化",
      title: "让工具适应写作者。",
      body: "从主题到字体，从书写宽度到快捷键，把写作空间调整成你熟悉的样子。",
      items: ["应用与编辑器主题", "字体、字号与行高", "书写宽度", "自定义快捷键"]
    },
    sync: {
      label: "数据自主",
      title: "你的笔记，本该躺在自己的存储桶里。",
      body: "文件默认留在本地。需要跨设备时，再把项目文件夹同步到你控制的 WebDAV 或 S3 兼容存储。",
      points: ["本地优先", "普通 Markdown", "S3 / WebDAV", "不依赖托管工作区"]
    },
    mobile: {
      label: "移动端",
      title: "案头挥毫，掌中轻语。",
      body: "桌面上细细雕琢，离开案头后继续捕捉片刻灵感。轻语移动端正在准备中。",
      status: "即将推出"
    },
    downloads: {
      label: "开始写作",
      title: "选择你的轻语。",
      platformLabels: { macos: "macOS", windows: "Windows", linux: "Linux" },
      web: "Web 编辑器",
      release: "前往下载"
    },
    manifesto: {
      label: "产品宣言",
      lines: ["我们并不需要另一个‘第二大脑’，", "我们只需要一个能安心写字的地方。", "剥离复杂的块与双链，回归最纯粹的行云流水。", "数据归于你的 S3，灵感归于你的内心。", "在这里，只有你与文字的轻语。"]
    },
    openSource: { label: "开放", title: "开源，也开放你的选择。", body: "轻语采用 AGPL-3.0 开源，文件与存储位置始终由你决定。", github: "查看 GitHub", docs: "阅读文档" },
    footer: { privacy: "隐私", changelog: "更新日志", contribute: "参与贡献", license: "AGPL-3.0" }
  },
  en: {
    languageLabel: "简体中文",
    nav: { product: "Product", features: "Features", sync: "Sync", mobile: "Mobile", manifesto: "Manifesto", download: "Download" },
    hero: { eyebrow: "A quiet place to write", title: "A clear desk. A quiet room. Every word softly spoken.", description: "No second brain. No patchwork. Just a calm place for writing that flows.", download: "Download for desktop", web: "Open the Web editor" },
    personality: { label: "A deliberate choice", title: "Not a second brain. Simply a place to write.", body: ["Leave complicated blocks and backlinks behind, and return to the natural rhythm of a page.", "No AI burden and no proprietary format. Your notes remain ordinary Markdown files."] },
    features: { label: "Writing experience", title: "Simple does not mean bare.", items: [{ title: "Two ways to edit", body: "Move between a polished document view and source mode without changing the Markdown underneath." }, { title: "A clear file structure", body: "Manage ordinary files and folders with tabs, split panes, quick open, workspace search, and outline." }, { title: "Rich, restrained Markdown", body: "Render links, images, HTML, GFM tables, KaTeX, Mermaid, callouts, and highlighted code when needed." }, { title: "Reliable daily writing", body: "Auto-save, tab and workspace restoration, and document or selection word counts keep work flowing." }, { title: "Ordinary assets, open export", body: "Keep images in a regular assets folder and export HTML, PDF, or more formats through Pandoc." }] },
    personalization: { label: "Personalization", title: "Let the tool adapt to the writer.", body: "Shape the space with themes, fonts, writing width, line height, and shortcuts.", items: ["App and editor themes", "Font, size, and line height", "Writing width", "Custom shortcuts"] },
    sync: { label: "Data ownership", title: "Your notes belong in storage you control.", body: "Files stay local by default. When you want continuity across devices, sync the project folder through your own WebDAV or S3-compatible storage.", points: ["Local first", "Ordinary Markdown", "S3 / WebDAV", "No hosted workspace required"] },
    mobile: { label: "Mobile", title: "Craft at your desk. Capture in your palm.", body: "Shape long-form writing on desktop, then catch passing thoughts away from your desk. QingYu mobile is in preparation.", status: "Coming soon" },
    downloads: { label: "Start writing", title: "Choose your QingYu.", platformLabels: { macos: "macOS", windows: "Windows", linux: "Linux" }, web: "Web editor", release: "View download" },
    manifesto: { label: "Manifesto", lines: ["We do not need another ‘second brain.’", "We only need a place where we can write in peace.", "Strip away complicated blocks and backlinks, and return to writing that simply flows.", "Your data belongs in your S3; your inspiration belongs within you.", "Here, there is only you and the quiet whisper of words."] },
    openSource: { label: "Open", title: "Open source, with your choices left open.", body: "QingYu is AGPL-3.0 software. Your files and storage location remain yours to choose.", github: "View on GitHub", docs: "Read the docs" },
    footer: { privacy: "Privacy", changelog: "Changelog", contribute: "Contribute", license: "AGPL-3.0" }
  }
} satisfies Record<SiteLocale, SiteCopy>;
```

Define `siteLinks` with these exact stable URLs:

```ts
export const siteLinks = {
  releases: "https://github.com/appdev/QingYu/releases/latest",
  webEditor: "https://editor.markra.app/",
  github: "https://github.com/appdev/QingYu",
  docs: "https://github.com/appdev/QingYu#documentation",
  privacy: "https://github.com/appdev/QingYu/blob/main/docs/privacy.md",
  changelog: "https://github.com/appdev/QingYu/blob/main/CHANGELOG.md",
  contributing: "https://github.com/appdev/QingYu/blob/main/CONTRIBUTING.md",
  license: "https://github.com/appdev/QingYu/blob/main/LICENSE"
} as const;
```

- [ ] **Step 5: Run tests and type checks**

Run:

```bash
pnpm --filter @markra/site test -- src/lib/locale.test.ts src/lib/platform.test.ts src/links.test.ts
pnpm --filter @markra/site typecheck:test
```

Expected: all locale/platform cases pass and both language objects satisfy the same `SiteCopy` contract.

- [ ] **Step 6: Commit the content model**

```bash
git add apps/site/src/content.ts apps/site/src/links.ts apps/site/src/lib apps/site/src/SiteApp.tsx
git commit -m "feat(site): add bilingual product content"
```

---

### Task 3: Build the Locale-Aware Header and Page Shell

**Files:**
- Create: `apps/site/src/components/SiteHeader.tsx`
- Create: `apps/site/src/components/SiteHeader.test.tsx`
- Create: `apps/site/src/components/SiteFooter.tsx`
- Create: `apps/site/src/components/SiteFooter.test.tsx`
- Modify: `apps/site/src/SiteApp.tsx`
- Modify: `apps/site/src/SiteApp.test.tsx`

**Interfaces:**
- Consumes: `SiteLocale`, `SiteCopy`, `siteContent`, `readStoredLocale`, `writeStoredLocale`, and `siteLinks` from Task 2.
- Produces: `SiteHeader({ copy, locale, onLocaleChange })` and `SiteFooter({ copy })`.
- Produces: `<SiteApp initialLocale>` with stable SSR markup and post-hydration stored-locale restoration.

- [ ] **Step 1: Write failing shell and navigation tests**

```tsx
import { fireEvent, render, screen } from "@testing-library/react";
import { SiteApp } from "./SiteApp";

describe("locale-aware site shell", () => {
  it("switches to English and persists the choice", () => {
    render(<SiteApp initialLocale="zh-CN" />);
    fireEvent.click(screen.getByRole("button", { name: "English" }));

    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("A clear desk");
    expect(window.localStorage.getItem("qingyu.site.locale")).toBe("en");
    expect(document.documentElement.lang).toBe("en");
  });

  it("exposes all approved single-page anchors", () => {
    render(<SiteApp initialLocale="zh-CN" />);
    for (const href of ["#product", "#features", "#sync", "#mobile", "#manifesto", "#download"]) {
      expect(document.querySelector(`a[href="${href}"]`)).not.toBeNull();
    }
  });
});
```

In `SiteHeader.test.tsx`, assert that the compact-menu button sets `aria-expanded`, Escape closes the menu, and focus returns to the menu button.

In `SiteFooter.test.tsx`, render the Chinese footer and assert that desktop download, Web editor, GitHub, documentation, privacy, changelog, contribution, and license anchors use the corresponding `siteLinks` values. This proves components consume the centralized link contract rather than duplicating URLs.

- [ ] **Step 2: Run tests to verify behavior is absent**

Run:

```bash
pnpm --filter @markra/site test -- src/SiteApp.test.tsx src/components/SiteHeader.test.tsx src/components/SiteFooter.test.tsx
```

Expected: FAIL because the header, footer, locale state, and compact navigation are not implemented.

- [ ] **Step 3: Implement locale state without hydration mismatch**

Use the server-provided locale for the initial render, then restore stored state in an effect:

```tsx
export function SiteApp({ initialLocale }: { initialLocale: SiteLocale }) {
  const [locale, setLocale] = useState(initialLocale);

  useEffect(() => {
    const stored = readStoredLocale(window.localStorage);
    setLocale(stored);
  }, []);

  useEffect(() => {
    document.documentElement.lang = locale;
    document.title = locale === "zh-CN" ? "轻语 QingYu — 明窗净几，字字轻语" : "QingYu — A quiet place to write";
  }, [locale]);

  const changeLocale = (nextLocale: SiteLocale) => {
    setLocale(nextLocale);
    writeStoredLocale(window.localStorage, nextLocale);
  };

  const copy = siteContent[locale];
  return <>{/* SiteHeader, main sections, SiteFooter */}</>;
}
```

The effect may update from server-rendered Chinese to a stored English preference only after hydration; it must not read `window` during SSR.

- [ ] **Step 4: Implement accessible desktop/compact navigation**

`SiteHeader` must include the real logo, the six approved anchors, language button, and desktop-download link. The compact menu uses a button with `aria-controls`, `aria-expanded`, an Escape listener, and a ref to restore focus after close. All promises are handled directly without the TypeScript `void` operator.

Use this exact component contract:

```ts
type SiteHeaderProps = {
  copy: SiteCopy;
  locale: SiteLocale;
  onLocaleChange: (locale: SiteLocale) => unknown;
};
```

`SiteFooter({ copy }: { copy: SiteCopy })` must render ordinary links from `siteLinks`, including the repeated desktop-download and Web-editor CTAs; external navigation must not depend on JavaScript.

- [ ] **Step 5: Run shell verification**

Run:

```bash
pnpm --filter @markra/site test -- src/SiteApp.test.tsx src/components/SiteHeader.test.tsx src/components/SiteFooter.test.tsx
pnpm --filter @markra/site typecheck:test
```

Expected: locale, persistence, anchor, compact-menu, Escape, and focus tests pass.

- [ ] **Step 6: Commit the site shell**

```bash
git add apps/site/src/SiteApp.tsx apps/site/src/SiteApp.test.tsx apps/site/src/components/SiteHeader.tsx apps/site/src/components/SiteHeader.test.tsx apps/site/src/components/SiteFooter.tsx apps/site/src/components/SiteFooter.test.tsx
git commit -m "feat(site): add localized navigation shell"
```

---

### Task 4: Implement Every Product Story Section

**Files:**
- Create: `apps/site/src/components/Hero.tsx`
- Create: `apps/site/src/components/NewsprintEditorPreview.tsx`
- Create: `apps/site/src/components/ManifestoIntro.tsx`
- Create: `apps/site/src/components/FeatureGrid.tsx`
- Create: `apps/site/src/components/Personalization.tsx`
- Create: `apps/site/src/components/SyncStory.tsx`
- Create: `apps/site/src/components/MobilePreview.tsx`
- Create: `apps/site/src/components/PlatformDownload.tsx`
- Create: `apps/site/src/components/FullManifesto.tsx`
- Create: `apps/site/src/components/OpenSourceSection.tsx`
- Create: `apps/site/src/components/ProductSections.test.tsx`
- Create: `apps/site/src/components/PlatformDownload.test.tsx`
- Modify: `apps/site/src/SiteApp.tsx`

**Interfaces:**
- Consumes: the relevant nested `SiteCopy` sections from Task 2.
- Consumes: `siteLinks`, `detectDownloadPlatform`, and `orderDownloadPlatforms`.
- Produces: semantic sections with IDs `product`, `features`, `personalization`, `sync`, `mobile`, `download`, and `manifesto`.
- Produces: a mobile section with status text but no Android/iOS anchors.

- [ ] **Step 1: Write failing content and download tests**

```tsx
import { render, screen, within } from "@testing-library/react";
import { SiteApp } from "../SiteApp";

describe("product story", () => {
  it("renders every approved section and full manifesto", () => {
    render(<SiteApp initialLocale="zh-CN" />);

    expect(document.querySelector("#product")).not.toBeNull();
    expect(document.querySelector("#features")).not.toBeNull();
    expect(document.querySelector("#personalization")).not.toBeNull();
    expect(document.querySelector("#sync")).not.toBeNull();
    expect(document.querySelector("#mobile")).not.toBeNull();
    expect(document.querySelector("#download")).not.toBeNull();
    expect(document.querySelector("#manifesto")).not.toBeNull();
    expect(screen.getByText("数据归于你的 S3，灵感归于你的内心。")).toBeInTheDocument();
  });

  it("does not expose unreleased mobile downloads", () => {
    render(<SiteApp initialLocale="zh-CN" />);
    const mobile = document.querySelector("#mobile");
    if (!mobile) throw new Error("Missing mobile section");

    expect(within(mobile).getByText("即将推出")).toBeInTheDocument();
    expect(within(mobile).queryByRole("link")).toBeNull();
    expect(document.querySelector('a[href*="play.google"]')).toBeNull();
    expect(document.querySelector('a[href*="apps.apple"]')).toBeNull();
  });
});
```

`PlatformDownload.test.tsx` must stub macOS, Windows, Linux, and unknown navigator platform values with Vitest, wait for the client effect, and verify that all three desktop cards remain present while only a recognized preferred card receives `data-preferred="true"`.

- [ ] **Step 2: Run component tests to verify failure**

Run:

```bash
pnpm --filter @markra/site test -- src/components/ProductSections.test.tsx src/components/PlatformDownload.test.tsx
```

Expected: FAIL because the sections do not exist.

- [ ] **Step 3: Implement the hero and Newsprint preview**

`Hero` renders the official feather image with a readable QingYu text fallback, one `h1`, desktop download anchor `href="#download"`, Web editor external link, platform list, and `NewsprintEditorPreview`.

`NewsprintEditorPreview` is decorative but accessible as a labelled product preview. It renders lightweight file-tree and paper markup only. Use these exact CSS custom properties on its root:

```tsx
const newsprintStyle = {
  "--preview-paper": "oklch(96% 0.018 88)",
  "--preview-sidebar": "oklch(91% 0.022 86)",
  "--preview-heading": "oklch(18% 0.012 75)",
  "--preview-text": "oklch(28% 0.012 75)",
  "--preview-accent": "oklch(42% 0.12 24)",
  "--preview-border": "oklch(80% 0.025 78)"
} as React.CSSProperties;
```

Do not import editor CSS or editor packages.

- [ ] **Step 4: Implement product, features, personalization, and sync**

Each section receives only its `SiteCopy` fragment and maps the approved content. Use lucide-react icons only for meaningful feature symbols, mark decorative icons `aria-hidden="true"`, and keep every heading available as text.

The sync flow must show `Local Markdown → WebDAV / S3 → Your devices` and must say that WebDAV/S3 project sync is a desktop capability. Never call it backup and never claim the Web editor supports project sync.

- [ ] **Step 5: Implement mobile, download, manifesto, and open source**

`MobilePreview` renders a compact phone mockup with the official feather mark, `copy.status`, and no link/button. `PlatformDownload` computes preferred platform in a client effect while keeping server output in the stable macOS/Windows/Linux order. Each desktop card includes the QingYu mark and links to `siteLinks.releases`; the Web card links to `siteLinks.webEditor`.

`FullManifesto` maps all five lines in order, and `OpenSourceSection` links to GitHub and documentation.

- [ ] **Step 6: Compose sections and run verification**

Compose in this exact order:

```tsx
<SiteHeader />
<main>
  <Hero />
  <ManifestoIntro />
  <FeatureGrid />
  <Personalization />
  <SyncStory />
  <MobilePreview />
  <PlatformDownload />
  <FullManifesto />
  <OpenSourceSection />
</main>
<SiteFooter />
```

Run:

```bash
pnpm --filter @markra/site test
pnpm --filter @markra/site typecheck:test
```

Expected: every section, manifesto, mobile boundary, platform ordering, shell, locale, and utility test passes.

- [ ] **Step 7: Commit the complete page content**

```bash
git add apps/site/src/components apps/site/src/SiteApp.tsx
git commit -m "feat(site): add product story sections"
```

---

### Task 5: Apply the Approved Brand System and Lightweight Font

**Files:**
- Modify: `apps/site/src/styles.css`
- Create: `apps/site/scripts/build-font-subset.py`
- Create: `apps/site/scripts/font-glyphs.txt`
- Create: `apps/site/scripts/generate-images.mjs`
- Create: `apps/site/src/font-glyphs.test.ts`
- Create: `apps/site/public/fonts/qingyu-wenkai-subset.woff2`
- Create: `apps/site/public/fonts/SOURCE.md`
- Create: `apps/site/public/licenses/OFL-LXGW-WenKai-GB-Lite.txt`
- Create: `apps/site/public/qingyu-logo.png`
- Create: `apps/site/public/qingyu-logo.webp`
- Create: `apps/site/public/og-image.png`

**Interfaces:**
- Consumes: all static strings from `siteContent`.
- Produces: the CSS family `"QingYu WenKai Subset"` with one Regular WOFF2 asset.
- Produces: responsive B-direction paper/editorial visuals with brand amber and exact Newsprint preview tokens.

- [ ] **Step 1: Write a failing glyph-coverage test**

Export a recursive `stringsInSiteCopy` helper from `content.ts`, then test that every non-ASCII character in both locales exists in `scripts/font-glyphs.txt`:

```ts
import glyphText from "../scripts/font-glyphs.txt?raw";
import { siteContent, stringsInSiteCopy } from "./content";

describe("QingYu WenKai subset", () => {
  it("covers every non-ASCII character in static site content", () => {
    const required = new Set(stringsInSiteCopy(siteContent).join("").match(/[^\u0000-\u007f]/gu) ?? []);
    const available = new Set(glyphText);
    expect([...required].filter((character) => !available.has(character))).toEqual([]);
  });
});
```

- [ ] **Step 2: Run the test to verify the glyph manifest is missing**

Run:

```bash
pnpm --filter @markra/site test -- src/font-glyphs.test.ts
```

Expected: FAIL because `font-glyphs.txt` is absent.

- [ ] **Step 3: Add reproducible subsetting and internal rename**

`build-font-subset.py` must:

```python
from pathlib import Path
import sys
from fontTools import subset
from fontTools.ttLib import TTFont

source = Path(sys.argv[1])
glyph_file = Path(sys.argv[2])
output = Path(sys.argv[3])

font = TTFont(source)
options = subset.Options()
options.layout_features = ["*"]
subsetter = subset.Subsetter(options=options)
subsetter.populate(text=glyph_file.read_text(encoding="utf-8"))
subsetter.subset(font)

names = font["name"]
for name_id in [1, 2, 3, 4, 6, 16, 17]:
    names.names = [record for record in names.names if record.nameID != name_id]
for platform_id, encoding_id, language_id in [(3, 1, 0x409), (1, 0, 0)]:
    names.setName("QingYu WenKai Subset", 1, platform_id, encoding_id, language_id)
    names.setName("Regular", 2, platform_id, encoding_id, language_id)
    names.setName("QingYuWenKaiSubset-Regular", 3, platform_id, encoding_id, language_id)
    names.setName("QingYu WenKai Subset Regular", 4, platform_id, encoding_id, language_id)
    names.setName("QingYuWenKaiSubset-Regular", 6, platform_id, encoding_id, language_id)
    names.setName("QingYu WenKai Subset", 16, platform_id, encoding_id, language_id)
    names.setName("Regular", 17, platform_id, encoding_id, language_id)

output.parent.mkdir(parents=True, exist_ok=True)
font.flavor = "woff2"
font.save(output)
```

Populate `font-glyphs.txt` with every unique character from `stringsInSiteCopy(siteContent)`, plus punctuation used in markup and the visible Chinese strings in `NewsprintEditorPreview`.

- [ ] **Step 4: Download, verify, subset, and license the official font**

Run from a temporary directory outside the repository:

```bash
curl -fL -o /tmp/LXGWWenKaiGBLite-Regular-v1.522.ttf https://github.com/lxgw/LxgwWenkaiGB-Lite/releases/download/v1.522/LXGWWenKaiGBLite-Regular.ttf
printf '%s  %s\n' '1675c708cce181871d9a8adc987f35a0cabc6ff980685cd99f05d2655ea08c4c' '/tmp/LXGWWenKaiGBLite-Regular-v1.522.ttf' | shasum -a 256 -c -
uvx --from 'fonttools[woff]' python apps/site/scripts/build-font-subset.py /tmp/LXGWWenKaiGBLite-Regular-v1.522.ttf apps/site/scripts/font-glyphs.txt apps/site/public/fonts/qingyu-wenkai-subset.woff2
curl -fL -o apps/site/public/licenses/OFL-LXGW-WenKai-GB-Lite.txt https://raw.githubusercontent.com/lxgw/LxgwWenkaiGB-Lite/v1.522/OFL.txt
```

Expected: checksum reports `OK`, the output is WOFF2, and no TTF exists below `apps/site`.

Write `SOURCE.md` with repository URL, release `v1.522`, TTF checksum, the exact `uvx` generation command, internal renamed family, and OFL file location.

- [ ] **Step 5: Add actual brand assets without oversized source images**

Copy `apps/web/public/favicon.png` to `apps/site/public/qingyu-logo.png`. Add `scripts/generate-images.mjs` using the site's pinned `sharp` development dependency. Resolve all paths from `import.meta.url`, resize `assets/branding/app-icon/platform-master.png` to a 256x256 quality-86 WebP, and create the 1200x630 Open Graph PNG from a fixed SVG composition:

```js
import { mkdir } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import sharp from "sharp";

const siteRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repositoryRoot = resolve(siteRoot, "../..");
const publicDirectory = resolve(siteRoot, "public");
const masterIcon = resolve(repositoryRoot, "assets/branding/app-icon/platform-master.png");
const fontSourceArgument = process.argv[2];
if (!fontSourceArgument?.endsWith(".ttf")) {
  throw new Error("Pass the verified LXGW WenKai GB Lite TTF path as the first argument.");
}
const fontSource = resolve(fontSourceArgument);

await mkdir(publicDirectory, { recursive: true });
await sharp(masterIcon)
  .resize(256, 256)
  .webp({ quality: 86 })
  .toFile(resolve(publicDirectory, "qingyu-logo.webp"));

const icon = await sharp(masterIcon).resize(148, 148).png().toBuffer();
const title = await sharp({
  text: {
    text: '<span foreground="#181714">明窗净几，字字轻语。</span>',
    font: "LXGW WenKai GB Lite 58",
    fontfile: fontSource,
    rgba: true
  }
}).png().toBuffer();
const composition = Buffer.from(`
  <svg width="1200" height="630" xmlns="http://www.w3.org/2000/svg">
    <rect width="1200" height="630" fill="#f8f6f0"/>
    <rect x="64" y="56" width="1072" height="518" rx="34" fill="#fffdf7" stroke="#cfcbc1"/>
    <circle cx="138" cy="130" r="5" fill="#f9b52b"/>
    <text x="178" y="143" fill="#181714" font-family="sans-serif" font-size="38">QingYu</text>
    <text x="104" y="340" fill="#686158" font-family="sans-serif" font-size="25">A quiet place to write.</text>
    <rect x="700" y="112" width="356" height="360" rx="18" fill="#f3efe3" stroke="#cfcbc1"/>
    <rect x="700" y="112" width="92" height="360" rx="18" fill="#e8e2d3"/>
    <rect x="824" y="170" width="168" height="12" rx="6" fill="#3e3932"/>
    <rect x="824" y="218" width="192" height="7" rx="3.5" fill="#777064"/>
    <rect x="824" y="244" width="164" height="7" rx="3.5" fill="#777064"/>
    <rect x="824" y="270" width="180" height="7" rx="3.5" fill="#777064"/>
    <rect x="824" y="330" width="116" height="8" rx="4" fill="#8e3328"/>
  </svg>
`);

await sharp(composition)
  .composite([
    { input: title, left: 104, top: 198 },
    { input: icon, left: 520, top: 403 }
  ])
  .png({ compressionLevel: 9 })
  .toFile(resolve(publicDirectory, "og-image.png"));
```

Run:

```bash
cp apps/web/public/favicon.png apps/site/public/qingyu-logo.png
pnpm --filter @markra/site run assets -- /tmp/LXGWWenKaiGBLite-Regular-v1.522.ttf
```

Do not copy the 966 KB root `logo.png` or 1.5 MB `bg.png` into `apps/site`. Inspect both generated images with `view_image` before committing.

- [ ] **Step 6: Implement the full approved style system**

At the top of `styles.css`:

```css
@import "tailwindcss";

@font-face {
  font-family: "QingYu WenKai Subset";
  src: url("/fonts/qingyu-wenkai-subset.woff2") format("woff2");
  font-style: normal;
  font-weight: 400;
  font-display: swap;
}

:root {
  --site-paper: #f8f6f0;
  --site-paper-deep: #e8e5dd;
  --site-ink: #181714;
  --site-muted: #686158;
  --site-line: #cfcbc1;
  --site-amber: #f9b52b;
  --site-amber-deep: #b87b17;
  font-family: "QingYu WenKai Subset", "Kaiti SC", "STKaiti", serif;
  color: var(--site-ink);
  background: var(--site-paper);
}

.latin,
code {
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
}

@media (prefers-reduced-motion: reduce) {
  *, *::before, *::after {
    scroll-behavior: auto !important;
    animation-duration: 0.01ms !important;
    animation-iteration-count: 1 !important;
    transition-duration: 0.01ms !important;
  }
}
```

Implement desktop/tablet/mobile layouts without global CSS unrelated to the site. The 390px layout must be one column with the editor preview after hero copy; the 1440px layout uses the approved two-column hero. Keep content width readable, preserve visible focus rings, and avoid horizontal overflow.

- [ ] **Step 7: Verify glyphs, font output, and focused behavior**

Run:

```bash
pnpm --filter @markra/site test -- src/font-glyphs.test.ts
find apps/site -type f -name '*.ttf' -print
ls -lh apps/site/public/fonts/qingyu-wenkai-subset.woff2 apps/site/public/qingyu-logo.webp apps/site/public/og-image.png
pnpm --filter @markra/site build
```

Expected: glyph test passes; the TTF search prints nothing; generated asset sizes are recorded; the site builds.

- [ ] **Step 8: Commit brand and font assets**

```bash
git add apps/site/src/styles.css apps/site/src/font-glyphs.test.ts apps/site/src/content.ts apps/site/scripts apps/site/public
git commit -m "feat(site): apply QingYu editorial brand"
```

---

### Task 6: Add Static Prerendering, SEO, and Build Verification

**Files:**
- Create: `apps/site/src/prerender.tsx`
- Create: `apps/site/scripts/prerender-lib.mjs`
- Create: `apps/site/scripts/prerender-lib.test.mjs`
- Create: `apps/site/scripts/prerender.mjs`
- Create: `apps/site/scripts/verify-build.mjs`
- Create: `apps/site/scripts/verify-build.test.mjs`
- Modify: `apps/site/index.html`
- Modify: `apps/site/package.json`

**Interfaces:**
- Produces: `renderSite(): string` from the SSR bundle.
- Produces: `injectPrerenderedRoot(html, markup): string`.
- Produces: `verifySiteBuild(distDirectory): { assets: Array<{ path: string; bytes: number }> }`.
- Consumes: `SiteApp initialLocale="zh-CN"` with no `window` access during render.

- [ ] **Step 1: Write failing pure Node tests**

```js
import assert from "node:assert/strict";
import test from "node:test";
import { injectPrerenderedRoot } from "./prerender-lib.mjs";

test("injects server markup into the unique site root", () => {
  assert.equal(
    injectPrerenderedRoot('<div id="root"></div>', "<main><h1>明窗净几，字字轻语。</h1></main>"),
    '<div id="root"><main><h1>明窗净几，字字轻语。</h1></main></div>'
  );
});

test("rejects missing or duplicate roots", () => {
  assert.throws(() => injectPrerenderedRoot("<body></body>", "<main />"), /exactly one root/u);
  assert.throws(() => injectPrerenderedRoot('<div id="root"></div><div id="root"></div>', "<main />"), /exactly one root/u);
});
```

`verify-build.test.mjs` creates a temporary `dist` fixture and verifies rejection for missing Chinese `h1`, missing manifesto, any `.ttf`, multiple WOFF2 files, or asset/chunk names containing `milkdown`, `codemirror`, `tauri`, or `mermaid`.

- [ ] **Step 2: Run Node tests to verify failure**

Run:

```bash
node --test apps/site/scripts/*.test.mjs
```

Expected: FAIL because the script modules do not exist.

- [ ] **Step 3: Implement SSR markup and deterministic injection**

```tsx
import { renderToString } from "react-dom/server";
import { SiteApp } from "./SiteApp";

export function renderSite() {
  return renderToString(<SiteApp initialLocale="zh-CN" />);
}
```

```js
const rootPattern = /<div id="root"[^>]*><\/div>/gu;

export function injectPrerenderedRoot(html, markup) {
  const matches = html.match(rootPattern) ?? [];
  if (matches.length !== 1) throw new Error("Expected exactly one root element.");
  return html.replace(rootPattern, `<div id="root">${markup}</div>`);
}
```

`prerender.mjs` imports `renderSite` from `../dist-ssr/prerender.js`, reads `dist/index.html`, injects markup, and writes the result atomically through a sibling temporary file followed by rename.

- [ ] **Step 4: Implement build verification**

`verifySiteBuild` must read `dist/index.html`, assert the Chinese `h1`, one sentence of product copy, all five manifesto lines, favicon, canonical marker, JSON-LD, and one WOFF2 preload. Recursively list `dist` and reject:

- any `.ttf`, `.otf`, or `.woff` file;
- anything except one `.woff2` file;
- chunk names containing `milkdown`, `codemirror`, `tauri`, `mermaid`, or `katex`;
- a missing `og-image.png`;
- an Android/iOS store link.

Print a stable table of relative asset paths and byte sizes after passing.

- [ ] **Step 5: Add static SEO markup**

Update `index.html` with Chinese title, description, favicon, theme color, Open Graph, Twitter Card, and `SoftwareApplication` JSON-LD. Add `<link rel="canonical" href="/" data-site-origin>` and `<meta property="og:url" content="/" data-site-origin>`; do not invent a production hostname. The Open Graph image may likewise use `/og-image.png` in source. Deployment may expand only these marked relative values when a domain is chosen.

Preload `/fonts/qingyu-wenkai-subset.woff2` with `as="font"`, `type="font/woff2"`, and `crossorigin`.

- [ ] **Step 6: Wire build scripts**

Update scripts:

```json
{
  "build": "tsc -p tsconfig.app.json && vite build && vite build --ssr src/prerender.tsx --outDir dist-ssr && node scripts/prerender.mjs && node scripts/verify-build.mjs",
  "test": "vitest run && node --test scripts/*.test.mjs"
}
```

Keep `dist-ssr/` ignored by the existing root `.gitignore`.

- [ ] **Step 7: Run prerender and build verification**

Run:

```bash
pnpm --filter @markra/site test
pnpm --filter @markra/site typecheck:test
pnpm --filter @markra/site build
rg -n "明窗净几|数据归于你的 S3|application/ld\+json" apps/site/dist/index.html
find apps/site/dist -type f \( -name '*.ttf' -o -name '*.otf' -o -name '*.woff' \) -print
```

Expected: Vitest and Node tests pass, build verifier prints the asset-size table, Chinese content is present in final HTML, and the forbidden-font search prints nothing.

- [ ] **Step 8: Commit prerendering and SEO**

```bash
git add apps/site/index.html apps/site/package.json apps/site/src/prerender.tsx apps/site/scripts
git commit -m "feat(site): prerender product website"
```

---

### Task 7: Run Full Repository and Browser Acceptance

**Files:**
- Modify only if verification finds a scoped defect: `apps/site/**`
- Do not modify: `apps/web/**`, `apps/desktop/**`, or unrelated user changes to make a site-only failure disappear.

**Interfaces:**
- Consumes: the complete `@markra/site` package.
- Produces: verified static output at `apps/site/dist/` and a browser-checked single-page experience.

- [ ] **Step 1: Run all site gates from a clean command invocation**

```bash
pnpm --filter @markra/site test
pnpm --filter @markra/site typecheck:test
pnpm --filter @markra/site build
```

Expected: all tests pass, typecheck exits 0, prerender succeeds, and build verifier prints the asset inventory.

- [ ] **Step 2: Run repository-wide gates**

```bash
pnpm test
pnpm typecheck:test
pnpm build
```

Expected: existing packages plus `@markra/site` pass without editor, desktop, or Web regressions. Rust tests are not required for this frontend-only package unless the implementation unexpectedly changes Rust files.

- [ ] **Step 3: Start the site and verify three viewports in a real browser**

Read and use `browser:control-in-app-browser` for this acceptance pass so the checked state is the real rendered site, not inferred markup.

Run:

```bash
pnpm --filter @markra/site dev --host 127.0.0.1
```

Check 390x844, 768x1024, and 1440x1000:

- no horizontal overflow;
- navigation anchors reach the correct section;
- compact navigation opens, closes with Escape, and restores focus;
- Chinese is initial and English persists after reload;
- hero uses the official icon and QingYu WenKai subset;
- editor preview uses Newsprint colors;
- desktop download is primary and Web editor is secondary;
- mobile says “即将推出” / “Coming soon” without a link;
- reduced-motion mode removes nonessential animation;
- blocking the WOFF2 request leaves the page readable with stable section ordering and system-font fallback;
- no page or console errors.

- [ ] **Step 4: Verify links and production HTML**

Use a local HTTP server for `apps/site/dist` and verify HTTP 200 for `/`, `/qingyu-logo.png`, `/og-image.png`, and `/fonts/qingyu-wenkai-subset.woff2`. Check each external link destination without changing external state.

- [ ] **Step 5: Inspect the final diff and asset boundary**

```bash
git diff --check
git status --short
git log --oneline --decorate -8
rg -n "@markra/editor|@markra/app|@tauri-apps|milkdown|codemirror|mermaid|katex" apps/site
```

Expected: diff check is clean; the recent commit list clearly identifies the planned site/package-lock/spec sequence separately from earlier history, while pre-existing unrelated working-tree changes remain untouched; dependency scan has no runtime imports (copy mentioning product capabilities is allowed only in content).

- [ ] **Step 6: Commit any final scoped QA correction**

If browser verification required a site-only correction, stage only those exact `apps/site` files and commit:

```bash
git add apps/site
git commit -m "fix(site): polish responsive product page"
```

If no correction was needed, do not create an empty commit.

- [ ] **Step 7: Record final evidence for handoff**

Capture:

- site test count and pass result;
- root test/typecheck/build exit results;
- final WOFF2, JavaScript, CSS, logo, and Open Graph image sizes;
- browser viewports checked;
- current branch and commit list;
- confirmation that Android/iOS have no download links and no production deployment was performed.
