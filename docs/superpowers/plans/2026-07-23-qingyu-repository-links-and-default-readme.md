# QingYu Repository Links And Default README Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Simplified Chinese the default GitHub README and make `https://github.com/appdev/QingYu` the consistent QingYu project repository across tracked product links, metadata, tests, release fixtures, and history.

**Architecture:** Preserve the current two-language README content while swapping which filename GitHub treats as the default. Migrate each product-owned repository URL without changing path suffixes, then update the stable link contracts and focused tests that guard the website, desktop metadata, diagnostics issue drafts, and updater manifests. Keep functional third-party services and the factual read-only upstream remote unchanged.

**Tech Stack:** Markdown, React/TypeScript, Vitest, Rust/Tauri, Cargo metadata, Node.js test runner, pnpm workspace.

## Global Constraints

- `README.md` must contain the current Simplified Chinese content.
- `README.en.md` must contain the current English content.
- QingYu-owned repository URLs must use `https://github.com/appdev/QingYu` and retain their existing releases, issues, commits, comparisons, files, or fragment suffixes.
- Badge providers, contributor rendering, star-history rendering, dependency/license sites, and `https://editor.markra.app/` remain functional third-party destinations.
- The read-only `upstream` remote remains `https://github.com/markrahq/markra.git`; `origin` remains `https://github.com/appdev/QingYu.git`.
- Package scopes, bundle identifiers, executable names, protocol names, storage keys, and local paths containing `markra` are outside this change.
- Preserve the unrelated untracked `macos-icon.icns` file and stage only the files named by each task.
- Use `pnpm` for JavaScript workspace commands.

---

## File Map

- `README.md`: Simplified Chinese default project landing page.
- `README.en.md`: English alternate project landing page, created by renaming the current default README.
- `README.zh-CN.md`: removed after its content becomes `README.md`.
- `apps/site/src/links.ts`: public product-site destination contract.
- `apps/site/src/links.test.ts`: exact product-site URL expectations.
- `packages/app/src/lib/diagnostics/diagnostics-report.ts`: GitHub issue-draft base URL.
- `packages/app/src/lib/diagnostics/diagnostics-report.test.ts`: direct diagnostics URL contract.
- `packages/app/src/components/AppErrorBoundary.test.tsx`: crash-report issue destination integration.
- `packages/app/src/App.test.tsx`: runtime-error issue destination integration.
- `apps/desktop/src-tauri/src/menu.rs`: native About metadata and its focused test.
- `apps/desktop/src-tauri/Cargo.toml`: Rust package repository metadata.
- `scripts/release/generate-updater-manifest.test.mjs`: updater manifest repository fixture and expected download URLs.
- `CHANGELOG.md`: historical compare, issue, commit, and release links.
- `docs/superpowers/plans/2026-07-20-qingyu-product-site.md`: previously recorded product-site URL examples.

---

### Task 1: Make Simplified Chinese The Default README

**Files:**
- Rename: `README.md` to `README.en.md`
- Rename: `README.zh-CN.md` to `README.md`
- Modify: `README.md:12`
- Modify: `README.en.md:12`

**Interfaces:**
- Consumes: the current English and Simplified Chinese README documents.
- Produces: GitHub default `README.md` in Simplified Chinese and alternate `README.en.md` in English.

- [ ] **Step 1: Confirm the README source files and unrelated worktree state**

Run:

```bash
git status --short
sed -n '1,18p' README.md
sed -n '1,18p' README.zh-CN.md
```

Expected: `README.md` starts with the English QingYu copy, `README.zh-CN.md` starts with the Simplified Chinese copy, and `macos-icon.icns` remains the only unrelated untracked file.

- [ ] **Step 2: Rename both language documents without rewriting their bodies**

Run:

```bash
git mv README.md README.en.md
git mv README.zh-CN.md README.md
```

Expected: `git status --short` reports `README.en.md` as the English rename and `README.md` as the Chinese destination; `README.zh-CN.md` no longer exists.

- [ ] **Step 3: Update the two language selectors**

Use these exact selector bodies:

```html
<!-- README.md -->
<a href="README.en.md">English</a> | 简体中文 | <a href="https://editor.markra.app/">Web 编辑器</a> | <a href="#下载">下载</a> | <a href="#文档">文档</a> | <a href="#核心特性">核心特性</a> | <a href="#参与贡献">参与贡献</a> | <a href="#许可证">许可证</a>

<!-- README.en.md -->
English | <a href="README.md">简体中文</a> | <a href="https://editor.markra.app/">Web Editor</a> | <a href="#download">Download</a> | <a href="#documentation">Docs</a> | <a href="#key-features">Key Features</a> | <a href="#contributing">Contributing</a> | <a href="#license">License</a>
```

- [ ] **Step 4: Verify both filenames and language switchers**

Run:

```bash
test -f README.md
test -f README.en.md
test ! -e README.zh-CN.md
rg -n '<a href="README.en.md">English</a> \| 简体中文' README.md
rg -n 'English \| <a href="README.md">简体中文</a>' README.en.md
rg -n '轻语是一个面向简单、实用记录的开源 Markdown 编辑器' README.md
rg -n 'QingYu is an open-source Markdown editor' README.en.md
git diff --check -- README.md README.en.md README.zh-CN.md
```

Expected: every command exits zero and both content-language assertions print one match.

- [ ] **Step 5: Commit the README language migration**

Run:

```bash
git add -- README.md README.en.md README.zh-CN.md
git diff --cached --check
git commit -m "docs: make Chinese the default README"
```

Expected: one commit containing only the README rename and selector changes.

---

### Task 2: Migrate The Product Site Link Contract

**Files:**
- Modify: `apps/site/src/links.test.ts:3-15`
- Modify: `apps/site/src/links.ts:1-10`

**Interfaces:**
- Consumes: the existing `siteLinks` object shape.
- Produces: the same readonly object keys with all QingYu repository destinations under `appdev/QingYu`.

- [ ] **Step 1: Change the site contract test to the new repository**

Replace the expected object with:

```ts
expect(siteLinks).toEqual({
  releases: "https://github.com/appdev/QingYu/releases/latest",
  webEditor: "https://editor.markra.app/",
  github: "https://github.com/appdev/QingYu",
  docs: "https://github.com/appdev/QingYu#documentation",
  privacy: "https://github.com/appdev/QingYu/blob/main/docs/privacy.md",
  changelog: "https://github.com/appdev/QingYu/blob/main/CHANGELOG.md",
  contributing: "https://github.com/appdev/QingYu/blob/main/CONTRIBUTING.md",
  license: "https://github.com/appdev/QingYu/blob/main/LICENSE"
});
```

- [ ] **Step 2: Run the focused test and confirm the old production values fail**

Run:

```bash
pnpm --filter @markra/site exec vitest run src/links.test.ts
```

Expected: FAIL showing `markrahq/markra` received where `appdev/QingYu` is expected.

- [ ] **Step 3: Update the production link object**

Set `siteLinks` to:

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

- [ ] **Step 4: Run the focused test and typecheck**

Run:

```bash
pnpm --filter @markra/site exec vitest run src/links.test.ts
pnpm --filter @markra/site typecheck:test
```

Expected: the link test passes and the test TypeScript project exits zero.

- [ ] **Step 5: Commit the site link migration**

Run:

```bash
git add -- apps/site/src/links.ts apps/site/src/links.test.ts
git diff --cached --check
git commit -m "fix: point QingYu site links at downstream repository"
```

Expected: one commit containing only the site contract and its test.

---

### Task 3: Migrate Desktop Metadata And Diagnostics Links

**Files:**
- Modify: `packages/app/src/lib/diagnostics/diagnostics-report.test.ts:87-99`
- Modify: `packages/app/src/components/AppErrorBoundary.test.tsx:54-62`
- Modify: `packages/app/src/App.test.tsx:5753-5761`
- Modify: `apps/desktop/src-tauri/src/menu.rs:1152-1167`
- Modify: `packages/app/src/lib/diagnostics/diagnostics-report.ts:27`
- Modify: `apps/desktop/src-tauri/src/menu.rs:24`
- Modify: `apps/desktop/src-tauri/Cargo.toml:8`

**Interfaces:**
- Consumes: `generateDiagnosticsIssueUrl(report, options?)`, native `AboutMetadata`, and Cargo package metadata.
- Produces: issue drafts at `/appdev/QingYu/issues/new`, native About links to `appdev/QingYu`, and matching Rust repository metadata.

- [ ] **Step 1: Change the TypeScript integration expectations**

Use this pathname assertion in all three TypeScript tests:

```ts
expect(issueUrl.pathname).toBe("/appdev/QingYu/issues/new");
```

Update these files without changing the issue title or body assertions:

```text
packages/app/src/lib/diagnostics/diagnostics-report.test.ts
packages/app/src/components/AppErrorBoundary.test.tsx
packages/app/src/App.test.tsx
```

- [ ] **Step 2: Change the Rust About metadata expectation**

Use these exact expectations in `about_metadata_includes_version_and_github_link`:

```rust
assert_eq!(
    metadata.website.as_deref(),
    Some("https://github.com/appdev/QingYu")
);
assert!(metadata
    .credits
    .as_deref()
    .is_some_and(|credits| credits.contains("https://github.com/appdev/QingYu")));
```

- [ ] **Step 3: Run the focused tests and confirm both old production constants fail**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/lib/diagnostics/diagnostics-report.test.ts src/components/AppErrorBoundary.test.tsx src/App.test.tsx
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml about_metadata_includes_version_and_github_link
```

Expected: the TypeScript tests fail on `/markrahq/markra/issues/new`, and the Rust test fails on the old About website.

- [ ] **Step 4: Update production constants and Cargo metadata**

Use these exact values:

```ts
const diagnosticsIssueUrl = "https://github.com/appdev/QingYu/issues/new";
```

```rust
const MARKRA_GITHUB_URL: &str = "https://github.com/appdev/QingYu";
```

```toml
repository = "https://github.com/appdev/QingYu"
```

- [ ] **Step 5: Run the focused TypeScript and Rust tests again**

Run:

```bash
pnpm --filter @markra/app exec vitest run src/lib/diagnostics/diagnostics-report.test.ts src/components/AppErrorBoundary.test.tsx src/App.test.tsx
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml about_metadata_includes_version_and_github_link
```

Expected: all selected Vitest files pass and the selected Rust test passes.

- [ ] **Step 6: Commit the application repository metadata migration**

Run:

```bash
git add -- packages/app/src/lib/diagnostics/diagnostics-report.ts packages/app/src/lib/diagnostics/diagnostics-report.test.ts packages/app/src/components/AppErrorBoundary.test.tsx packages/app/src/App.test.tsx apps/desktop/src-tauri/src/menu.rs apps/desktop/src-tauri/Cargo.toml
git diff --cached --check
git commit -m "fix: point QingYu app links at downstream repository"
```

Expected: one commit containing the app, desktop, metadata, and matching test changes only.

---

### Task 4: Migrate Release Fixtures And Historical Project Links

**Files:**
- Modify: `scripts/release/generate-updater-manifest.test.mjs:47-137`
- Modify: `CHANGELOG.md`
- Modify: `docs/superpowers/plans/2026-07-20-qingyu-product-site.md:481-488`

**Interfaces:**
- Consumes: the updater generator's `GITHUB_REPOSITORY` input and existing historical URL suffixes.
- Produces: downstream updater download fixtures and history links whose issue numbers, commit hashes, tags, comparison ranges, filenames, and fragments are unchanged.

- [ ] **Step 1: Change the updater repository fixture and expected URLs**

Use this repository input in all three `runManifestScript` calls:

```js
GITHUB_REPOSITORY: "appdev/QingYu",
```

Use these five exact expected platform URLs:

```text
https://github.com/appdev/QingYu/releases/latest/download/QingYu_0.0.8_macos_arm64_updater.app.tar.gz
https://github.com/appdev/QingYu/releases/latest/download/QingYu_0.0.8_macos_x64_updater.app.tar.gz
https://github.com/appdev/QingYu/releases/latest/download/QingYu_0.0.8_linux_arm64.AppImage
https://github.com/appdev/QingYu/releases/latest/download/QingYu_0.0.8_linux_x64.AppImage
https://github.com/appdev/QingYu/releases/latest/download/QingYu_0.0.8_windows_x64_setup.exe
```

Keep every existing bundle filename and signature value unchanged.

- [ ] **Step 2: Run the release test suite**

Run:

```bash
pnpm test:release
```

Expected: all updater manifest tests pass because the generator derives URLs from `GITHUB_REPOSITORY`.

- [ ] **Step 3: Rewrite old project namespaces in the changelog and recorded site plan**

Run this mechanical URL-only rewrite:

```bash
perl -pi -e 's{https://github\.com/(?:markrahq/markra|murongg/markra)}{https://github.com/appdev/QingYu}g' CHANGELOG.md docs/superpowers/plans/2026-07-20-qingyu-product-site.md
```

Expected: only the repository owner/name portion changes; every suffix after the repository name stays byte-for-byte identical.

- [ ] **Step 4: Verify the mechanical rewrite and allowed exceptions**

Run:

```bash
git diff --check -- CHANGELOG.md docs/superpowers/plans/2026-07-20-qingyu-product-site.md scripts/release/generate-updater-manifest.test.mjs
git grep -n -E 'https://github\.com/(markrahq/markra|murongg/markra)' -- ':!AGENTS.md' ':!docs/superpowers/specs/2026-07-23-qingyu-repository-links-and-default-readme-design.md' ':!docs/superpowers/plans/2026-07-23-qingyu-repository-links-and-default-readme.md'
```

Expected: `git diff --check` exits zero and the repository URL search prints no output. `AGENTS.md` and the approved design document remain the only exact old-URL documentation exceptions.

- [ ] **Step 5: Commit release fixtures and historical link migration**

Run:

```bash
git add -- scripts/release/generate-updater-manifest.test.mjs CHANGELOG.md docs/superpowers/plans/2026-07-20-qingyu-product-site.md
git diff --cached --check
git commit -m "docs: migrate historical links to QingYu repository"
```

Expected: one commit containing only the release fixture and mechanical documentation URL migration.

---

### Task 5: Run The Final Repository Link Gate

**Files:**
- Verify: all tracked files changed by Tasks 1-4
- Preserve: `AGENTS.md`, `docs/superpowers/specs/2026-07-23-qingyu-repository-links-and-default-readme-design.md`, and untracked `macos-icon.icns`

**Interfaces:**
- Consumes: the four independently committed deliverables.
- Produces: fresh evidence that README routing, user-facing link contracts, runtime metadata, updater fixtures, and the allowed upstream exception all match the approved design.

- [ ] **Step 1: Re-run all focused JavaScript checks**

Run:

```bash
pnpm --filter @markra/site exec vitest run src/links.test.ts
pnpm --filter @markra/site typecheck:test
pnpm --filter @markra/app exec vitest run src/lib/diagnostics/diagnostics-report.test.ts src/components/AppErrorBoundary.test.tsx src/App.test.tsx
pnpm test:release
```

Expected: every command exits zero with no failing tests or type errors.

- [ ] **Step 2: Re-run the focused Rust metadata test**

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml about_metadata_includes_version_and_github_link
```

Expected: the selected test passes with zero failures.

- [ ] **Step 3: Verify the README routing and repository URLs**

Run:

```bash
test -f README.md
test -f README.en.md
test ! -e README.zh-CN.md
rg -n '<a href="README.en.md">English</a> \| 简体中文' README.md
rg -n 'English \| <a href="README.md">简体中文</a>' README.en.md
git grep -n 'https://github.com/appdev/QingYu' -- README.md README.en.md apps packages scripts CHANGELOG.md docs/superpowers/plans/2026-07-20-qingyu-product-site.md
git grep -n -E 'https://github\.com/(markrahq/markra|murongg/markra)' -- ':!AGENTS.md' ':!docs/superpowers/specs/2026-07-23-qingyu-repository-links-and-default-readme-design.md' ':!docs/superpowers/plans/2026-07-23-qingyu-repository-links-and-default-readme.md'
```

Expected: both README selector searches match; the downstream URL search prints the migrated surfaces; the obsolete project-identity search prints no output.

- [ ] **Step 4: Verify formatting, commit scope, remotes, and unrelated files**

Run:

```bash
git diff --check HEAD~4..HEAD
git log -7 --oneline --decorate
git remote -v
git status --short
```

Expected: diff check exits zero; the four implementation commits follow the committed plan and design; `origin` is `https://github.com/appdev/QingYu.git`; `upstream` fetch remains `https://github.com/markrahq/markra.git` with push disabled; only `?? macos-icon.icns` remains in worktree status.
