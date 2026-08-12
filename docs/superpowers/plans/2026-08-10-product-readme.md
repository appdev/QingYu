# QingYu Product README Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the four upstream-oriented technical READMEs with localized QingYu product homepages that use QingYu visuals, foreground user value, and clearly disclose the SiYuan foundation.

**Architecture:** Keep one shared nine-section information architecture across Chinese, English, Japanese, and Turkish. Use the Chinese copy as the semantic source, localize naturally, and protect the structure and attribution with the existing branding test.

**Tech Stack:** Markdown, HTML image markup, TypeScript, Node.js built-in test runner.

## Global Constraints

- Chinese product name is `轻语`; non-Chinese product name is `QingYu`.
- Chinese master slogan is `轻语 · 明窗净几，字字轻语`.
- The only README hero image is local `logo.png` with `width="128"`.
- Every README must say QingYu is based on SiYuan, follows AGPL-3.0, and is not an official SiYuan distribution.
- Do not advertise removed AI, Agent, semantic search, flashcard, graph, inbox, official cloud account, or official cloud sync features.
- Do not present SiYuan app-store packages, Docker image, pricing, community, or roadmap as QingYu services.
- Do not commit, push, publish, or rebuild.

---

### Task 1: Lock the product README contract

**Files:**
- Modify: `app/src/util/qingyuBranding.test.ts`

**Interfaces:**
- Consumes: the existing `readRepositoryFile(path)` helper and four README files.
- Produces: assertions for local visuals, upstream disclosure, removed upstream media, and removed feature claims.

- [ ] **Step 1: Add failing README assertions**

For each README, assert that it contains `src="logo.png" width="128"`, `https://github.com/siyuan-note/siyuan`, and `AGPL-3.0`. Assert that it does not contain `b3logfile.com`, `hub.docker.com/r/b3log/siyuan`, `apps.apple.com`, `play.google.com`, `b3log.org/siyuan/pricing`, `OpenAI`, or the old architecture headings. Add localized non-official markers: `不是思源笔记官方发行版`, `not an official SiYuan distribution`, `SiYuanの公式ディストリビューションではありません`, and `resmî bir SiYuan dağıtımı değildir`.

- [ ] **Step 2: Run the focused test**

Run: `cd app && node --import tsx --test src/util/qingyuBranding.test.ts`

Expected: FAIL because the current README files still contain upstream images, stores, architecture sections, and no non-official disclosure.

### Task 2: Rewrite the Chinese product README

**Files:**
- Replace: `README.zh-CN.md`

**Interfaces:**
- Consumes: `docs/superpowers/specs/2026-08-10-product-readme-design.md`.
- Produces: the semantic source for the three localized READMEs.

- [ ] **Step 1: Replace the document with the product structure**

Use these headings in order: `轻语`, `为什么是轻语`, `核心体验`, `数据与隐私`, `适合这样的你`, `当前状态`, `开发者入口`, `基于思源笔记`, `开源与致谢`. The hero uses the local 128 px logo and slogan. The first block states that QingYu is based on SiYuan and is not an official distribution.

- [ ] **Step 2: Describe only retained capabilities**

Include quiet block writing, references/backlinks, table databases, PDF annotation, clipping, OCR, import/export, encrypted notebooks, local snapshots, S3/WebDAV/local sync, themes/plugins/templates, and self-hosted access. Keep developer material to links for `docs/API.zh-CN.md`, `.github/CONTRIBUTING.zh-CN.md`, `CHANGELOG.md`, and the build scripts.

### Task 3: Localize the product README

**Files:**
- Replace: `README.md`
- Replace: `README.ja.md`
- Replace: `README.tr.md`

**Interfaces:**
- Consumes: the Chinese information architecture from Task 2.
- Produces: natural English, Japanese, and Turkish versions with identical product meaning.

- [ ] **Step 1: Write the English README**

Use headings `Why QingYu`, `Core experience`, `Your data, your space`, `Made for`, `Project status`, `For developers`, `Built on SiYuan`, and `Open source and acknowledgements`. State: `QingYu is based on the open-source project SiYuan and follows AGPL-3.0. It is not an official SiYuan distribution.`

- [ ] **Step 2: Write the Japanese README**

Use natural headings `軽語が大切にすること`, `中心となる体験`, `データとプライバシー`, `こんな方へ`, `現在の状況`, `開発者向け`, `SiYuanを基盤として`, and `オープンソースと謝辞`. State that it is based on SiYuan, follows AGPL-3.0, and `SiYuanの公式ディストリビューションではありません`.

- [ ] **Step 3: Write the Turkish README**

Use headings `QingYu neden var`, `Temel deneyim`, `Verileriniz, alanınız`, `Kimler için`, `Proje durumu`, `Geliştiriciler için`, `SiYuan üzerine inşa edildi`, and `Açık kaynak ve teşekkürler`. State that it is based on SiYuan, follows AGPL-3.0, and `resmî bir SiYuan dağıtımı değildir`.

### Task 4: Verify the frozen README set

**Files:**
- Verify: `README.zh-CN.md`
- Verify: `README.md`
- Verify: `README.ja.md`
- Verify: `README.tr.md`
- Verify: `app/src/util/qingyuBranding.test.ts`

**Interfaces:**
- Consumes: the completed localized README set.
- Produces: test, static-scan, and formatting evidence.

- [ ] **Step 1: Run the branding test**

Run: `cd app && node --import tsx --test src/util/qingyuBranding.test.ts`

Expected: all tests PASS.

- [ ] **Step 2: Scan for forbidden upstream marketing and removed features**

Run `rg` across the four README files for the forbidden URLs and removed feature names from Task 1.

Expected: no matches.

- [ ] **Step 3: Check formatting and scope**

Run: `git diff --check -- README.md README.zh-CN.md README.ja.md README.tr.md app/src/util/qingyuBranding.test.ts docs/superpowers/specs/2026-08-10-product-readme-design.md docs/superpowers/plans/2026-08-10-product-readme.md`

Expected: exit code 0. Confirm `app/weizhi-note/` remains untouched and untracked.
