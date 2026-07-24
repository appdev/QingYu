# QingYu Copy Rebrand Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace every tracked standalone legacy display-name reference with `轻语` for Simplified Chinese, `輕語` for Traditional Chinese, and `QingYu` everywhere else, while preserving all technical identifiers.

**Architecture:** Treat the display name as localized product copy, not as a rename of package or protocol identity. Frontend translations and native menu labels own locale-specific names; Tauri and release tooling use the non-Chinese `QingYu` default. A repository verifier provides the final guard against forgotten standalone legacy copy without flagging lowercase technical identifiers.

**Tech Stack:** pnpm workspace, React/TypeScript/Vitest, Tauri v2/Rust, Node.js release scripts, GitHub Actions.

## Global Constraints

- Simplified Chinese copy uses `轻语`; Traditional Chinese copy uses `輕語`; every other locale and nonlocalized surface uses the exact casing `QingYu`.
- Keep technical identifiers unchanged, including `@markra/*`, `dev.markra.app`, `.markraignore`, `.markra-sync`, `.qingyu/`, `.qingyu/config.json`, `QingYuProjectConfig`, `qingyu://*`, the `markra` shell command and executable, `markra:*` menu/event IDs, storage keys, Rust crate/module identifiers, URLs, Homebrew token/file names, and updater endpoints.
- Translation keys such as `menu.aboutMarkra` and code identifiers such as `toggle_markra_ai` remain unchanged; only their displayed values change.
- Preserve the unrelated untracked root file `bg.png`; do not stage or edit it.
- Use `pnpm` for JavaScript workflows and `apply_patch` for hand-authored edits. Bulk replacement is allowed only after path scoping and must be followed by the verifier and protected-identifier audit.
- Execute from the isolated `.worktrees/qingyu-copy-rebrand` worktree after a fast-forward to the latest verified local `main`. Never push this local customization unless the user explicitly requests it.

## Execution Baseline

- [x] Fast-forward `codex/qingyu-copy-rebrand` to local `main` commit `03fcdfc` in the isolated worktree.
- [x] Verify the default parallel Rust suite ten consecutive times: each run reports 385 passed, 0 failed, and 21 ignored.
- [x] Verify `pnpm test` and `pnpm typecheck:test` on the same baseline.

---

## Task 1: Add a tracked-copy legacy-name verifier

**Files:**

- Create: `packages/scripts/src/branding/verify-brand-copy.mjs`
- Create: `packages/scripts/src/branding/verify-brand-copy.test.mjs`
- Modify: `package.json`

- [ ] **Step 1: Write the failing verifier tests**

  Export `scanTextForLegacyBrandReferences(text, path)` and test that it:

  - reports the standalone legacy display name, assembled in the test as `["Mar", "kra"].join("")` so the verifier test does not flag itself;
  - returns path, line, column, and source-line context;
  - ignores lowercase technical forms such as `markra`, `@markra/shared`, `dev.markra.app`, `.markraignore`, and `markra:file`;
  - does not flag camel-case translation keys such as `aboutMarkra` or uppercase constants such as `MARKRA_GITHUB_URL`.
  - reports standalone `QingYu` in Simplified Chinese and Traditional Chinese locale files while allowing lowercase `.qingyu/` paths and embedded technical identifiers such as `QingYuProjectConfig`.

- [ ] **Step 2: Run the focused test and confirm it fails**

  Run:

  ```bash
  pnpm --filter @markra/scripts exec vitest run src/branding/verify-brand-copy.test.mjs
  ```

  Expected: FAIL because `verify-brand-copy.mjs` does not exist.

- [ ] **Step 3: Implement tracked-file scanning and the CLI**

  In `verify-brand-copy.mjs`:

  - construct the case-sensitive whole-word expression from string fragments rather than embedding the legacy display name literally;
  - use `git ls-files -z` to enumerate tracked files from the repository root;
  - skip binary files containing NUL bytes;
  - decode text as UTF-8 and report each match with repository-relative path, line, column, and trimmed line text;
  - export the text scanner for tests and expose a CLI that exits nonzero when matches remain;
  - print a concise success message when the tracked tree is clean.

  Add this root script:

  ```json
  "brand:verify": "node packages/scripts/src/branding/verify-brand-copy.mjs"
  ```

- [ ] **Step 4: Run the focused test and confirm it passes**

  Run:

  ```bash
  pnpm --filter @markra/scripts exec vitest run src/branding/verify-brand-copy.test.mjs
  ```

  Expected: PASS. Do not require `pnpm brand:verify` to pass yet; the remaining tasks deliberately remove the existing findings.

- [ ] **Step 5: Commit the verifier**

  ```bash
  git add package.json packages/scripts/src/branding/verify-brand-copy.mjs packages/scripts/src/branding/verify-brand-copy.test.mjs
  git commit -m "test: guard QingYu product copy"
  ```

---

## Task 2: Replace frontend, AI, and localized application copy

**Files:**

- Modify: `packages/shared/src/i18n/index.test.ts`
- Modify: `packages/shared/src/i18n/locales/de.ts`
- Modify: `packages/shared/src/i18n/locales/en.ts`
- Modify: `packages/shared/src/i18n/locales/es.ts`
- Modify: `packages/shared/src/i18n/locales/fr.ts`
- Modify: `packages/shared/src/i18n/locales/it.ts`
- Modify: `packages/shared/src/i18n/locales/ja.ts`
- Modify: `packages/shared/src/i18n/locales/ko.ts`
- Modify: `packages/shared/src/i18n/locales/pt-BR.ts`
- Modify: `packages/shared/src/i18n/locales/ru.ts`
- Modify: `packages/shared/src/i18n/locales/zh-CN.ts`
- Modify: `packages/shared/src/i18n/locales/zh-TW.ts`
- Modify: `packages/shared/src/search.test.ts`
- Modify: `apps/desktop/index.html`
- Modify: `apps/desktop/src/runtime/tauri/file.ts`
- Modify: `apps/desktop/src/runtime/tauri/file.test.ts`
- Modify: `apps/desktop/src/runtime/tauri/shell-command.test.ts`
- Modify: `apps/web/index.html`
- Modify: `apps/web/src/runtime/web/file.ts`
- Modify: `apps/web/src/runtime/web/file.test.ts`
- Modify: `apps/web/src/runtime/web/ignore-rules.ts`
- Modify: `apps/web/src/runtime/web/window.test.ts`
- Modify: `packages/ai/src/acp/client.test.ts`
- Modify: `packages/ai/src/agent/chat-adapters.test.ts`
- Modify: `packages/ai/src/agent/document/messages.ts`
- Modify: `packages/ai/src/agent/inline-prompt.ts`
- Modify: `packages/ai/src/agent/process-trace.test.ts`
- Modify: `packages/ai/src/agent/tools/web-search.test.ts`
- Modify: `packages/app/src/App.ai.test.tsx`
- Modify: `packages/app/src/App.document-history.test.tsx`
- Modify: `packages/app/src/App.test.tsx`
- Modify: `packages/app/src/components/AiAgentPanel.test.tsx`
- Modify: `packages/app/src/components/AiCommandBar.test.tsx`
- Modify: `packages/app/src/components/AiProviderConnectionSection.test.tsx`
- Modify: `packages/app/src/components/AppErrorBoundary.test.tsx`
- Modify: `packages/app/src/components/MarkdownFileTreeDrawer.test.tsx`
- Modify: `packages/app/src/components/MarkdownPaper.test.tsx`
- Modify: `packages/app/src/components/NativeTitleBar.test.tsx`
- Modify: `packages/app/src/components/SettingsShell.test.tsx`
- Modify: `packages/app/src/components/SettingsShell.tsx`
- Modify: `packages/app/src/components/SettingsWindow.tsx`
- Modify: `packages/app/src/components/UpdateProgressToast.test.tsx`
- Modify: `packages/app/src/components/WindowsNativeTitleBar.tsx`
- Modify: `packages/app/src/components/settings/AiSettings.test.tsx`
- Modify: `packages/app/src/components/settings/EditorSettings.test.tsx`
- Modify: `packages/app/src/components/settings/GeneralSettings.test.tsx`
- Modify: `packages/app/src/components/settings/GeneralSettings.tsx`
- Modify: `packages/app/src/components/settings/KeyboardShortcutsSettings.test.tsx`
- Modify: `packages/app/src/constants/initial-markdown.ts`
- Modify: `packages/app/src/hooks/useAiAgentSession.test.tsx`
- Modify: `packages/app/src/hooks/useAutoUpdater.test.tsx`
- Modify: `packages/app/src/hooks/useDocumentSearchState.test.tsx`
- Modify: `packages/app/src/hooks/useMarkdownDocument.ts`
- Modify: `packages/app/src/hooks/useMarkdownFileTree.test.tsx`
- Modify: `packages/app/src/hooks/useSettingsWindowState.test.ts`
- Modify: `packages/app/src/lib/acp-agent.test.ts`
- Modify: `packages/app/src/lib/acp-agent.ts`
- Modify: `packages/app/src/lib/app-toast.test.ts`
- Modify: `packages/app/src/lib/diagnostics/diagnostics-report.test.ts`
- Modify: `packages/app/src/lib/diagnostics/diagnostics-report.ts`
- Modify: `packages/app/src/lib/settings/app-settings.test.ts`
- Modify: `packages/app/src/lib/settings/app-settings.ts`
- Modify: `packages/editor/src/shortcuts.ts`
- Modify: `packages/markdown/src/markdown.test.ts`
- Modify: `packages/providers/src/native-web-search.test.ts`
- Modify: `packages/providers/src/requests.test.ts`
- Modify: `packages/providers/src/requests.ts`
- Modify: `packages/providers/src/settings.ts`

- [ ] **Step 1: Add a failing localized naming-matrix test**

  Extend `packages/shared/src/i18n/index.test.ts` with a table covering all eleven locales. Assert representative product strings such as `app.aiAgent`, `menu.hide`, and `menu.quit` use:

  ```ts
  const expectedProductNames = {
    en: "QingYu",
    "zh-CN": "轻语",
    "zh-TW": "輕語",
    ja: "QingYu",
    ko: "QingYu",
    fr: "QingYu",
    de: "QingYu",
    es: "QingYu",
    "pt-BR": "QingYu",
    it: "QingYu",
    ru: "QingYu"
  } as const;
  ```

  Also assemble the legacy display name from fragments and assert that no localized message value contains it as a standalone word. Assert that the Simplified Chinese and Traditional Chinese message maps contain no standalone `QingYu`; technical lowercase `.qingyu/` paths remain allowed.

- [ ] **Step 2: Run the i18n test and confirm it fails**

  Run:

  ```bash
  pnpm --filter @markra/shared exec vitest run src/i18n/index.test.ts
  ```

  Expected: FAIL on the current locale values.

- [ ] **Step 3: Update all locale values without renaming keys**

  Apply the naming matrix to every standalone product reference in the eleven locale files. This includes update messages, accessibility names, settings descriptions, About/Hide/Quit menu text, diagnostics, AI panel labels, placeholders, and `Product AI` references. Preserve keys such as `menu.aboutMarkra` and technical strings such as `.markraignore`.

- [ ] **Step 4: Update nonlocalized frontend and prompt copy**

  Replace standalone product copy with `QingYu` in the HTML titles, settings import/export descriptions, initial document, title bars, diagnostics reports, ACP client titles/actors/prompts, AI system prompts, provider comments and tests, editor comments, Markdown fixtures, web search fixtures, and related test expectations.

  Paths such as `/Applications/<Product>.app/Contents/MacOS/markra` become `/Applications/QingYu.app/Contents/MacOS/markra`: update the display-name path segment but preserve the executable name. Stored format strings such as `markra-settings` remain unchanged.

- [ ] **Step 5: Run focused package tests**

  Run:

  ```bash
  pnpm --filter @markra/shared test
  pnpm --filter @markra/ai test
  pnpm --filter @markra/providers test
  pnpm --filter @markra/editor test
  pnpm --filter @markra/markdown test
  pnpm --filter @markra/app test
  pnpm --filter @markra/web test
  pnpm --filter @markra/desktop exec vitest run src/runtime/tauri/file.test.ts src/runtime/tauri/shell-command.test.ts
  ```

  Expected: all commands PASS with updated copy expectations.

- [ ] **Step 6: Commit frontend copy**

  ```bash
  git add apps/desktop/index.html apps/desktop/src/runtime apps/web packages/ai packages/app packages/editor packages/markdown packages/providers packages/shared
  git commit -m "feat: apply localized QingYu product copy"
  ```

---

## Task 3: Localize native menus and update desktop package identity

**Files:**

- Modify: `apps/desktop/src-tauri/src/menu_labels/mod.rs`
- Modify: `apps/desktop/src-tauri/src/menu_labels/de.rs`
- Modify: `apps/desktop/src-tauri/src/menu_labels/en.rs`
- Modify: `apps/desktop/src-tauri/src/menu_labels/es.rs`
- Modify: `apps/desktop/src-tauri/src/menu_labels/fr.rs`
- Modify: `apps/desktop/src-tauri/src/menu_labels/it.rs`
- Modify: `apps/desktop/src-tauri/src/menu_labels/ja.rs`
- Modify: `apps/desktop/src-tauri/src/menu_labels/ko.rs`
- Modify: `apps/desktop/src-tauri/src/menu_labels/pt_br.rs`
- Modify: `apps/desktop/src-tauri/src/menu_labels/ru.rs`
- Modify: `apps/desktop/src-tauri/src/menu_labels/zh_cn.rs`
- Modify: `apps/desktop/src-tauri/src/menu_labels/zh_tw.rs`
- Modify: `apps/desktop/src-tauri/src/menu.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/opened_files.rs`
- Modify: `apps/desktop/src-tauri/src/shell_command.rs`
- Modify: `apps/desktop/src-tauri/src/web_http.rs`
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/tauri.conf.json`
- Modify: `apps/desktop/src-tauri/capabilities/main.json`
- Modify: `apps/desktop/src-tauri/gen/schemas/capabilities.json`
- Modify: `apps/desktop/src-tauri/macos-locales/de.lproj/InfoPlist.strings`
- Modify: `apps/desktop/src-tauri/macos-locales/en.lproj/InfoPlist.strings`
- Modify: `apps/desktop/src-tauri/macos-locales/es.lproj/InfoPlist.strings`
- Modify: `apps/desktop/src-tauri/macos-locales/fr.lproj/InfoPlist.strings`
- Modify: `apps/desktop/src-tauri/macos-locales/it.lproj/InfoPlist.strings`
- Modify: `apps/desktop/src-tauri/macos-locales/ja.lproj/InfoPlist.strings`
- Modify: `apps/desktop/src-tauri/macos-locales/ko.lproj/InfoPlist.strings`
- Modify: `apps/desktop/src-tauri/macos-locales/pt-BR.lproj/InfoPlist.strings`
- Modify: `apps/desktop/src-tauri/macos-locales/ru.lproj/InfoPlist.strings`
- Modify: `apps/desktop/src-tauri/macos-locales/zh-Hans.lproj/InfoPlist.strings`
- Modify: `apps/desktop/src-tauri/macos-locales/zh-Hant.lproj/InfoPlist.strings`
- Modify: `apps/desktop/src/app-package-integration.test.ts`

- [ ] **Step 1: Write failing native and package-name tests**

  Add `app_name: &'static str` to the expected `MenuLabels` interface in tests before implementation. Extend Rust menu tests to call `application_about_metadata(labels.app_name)` and assert `QingYu` for English, `轻语` for Simplified Chinese, and `輕語` for Traditional Chinese.

  Extend `apps/desktop/src/app-package-integration.test.ts` to parse `src-tauri/tauri.conf.json` and the macOS localization files, asserting:

  - `productName === "QingYu"`;
  - `identifier === "dev.markra.app"`;
  - Simplified Chinese bundle display/name values are `轻语`;
  - Traditional Chinese values are `輕語`;
  - all other bundle localizations use `QingYu`.

- [ ] **Step 2: Run the focused tests and confirm they fail**

  Run:

  ```bash
  cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml menu::tests
  pnpm --filter @markra/desktop exec vitest run src/app-package-integration.test.ts
  ```

  Expected: FAIL because native labels and Tauri metadata still use the legacy display name.

- [ ] **Step 3: Implement locale-owned native application names**

  Add `app_name` to `MenuLabels`. Set it to `轻语` in `zh_cn.rs`, `輕語` in `zh_tw.rs`, and `QingYu` in every other native locale.

  Change `application_about_metadata` to accept the localized application name and use it for About metadata. Pass `labels.app_name` into the application submenu title and About item. Use `QingYu` only as the nonlocalized fallback for the Windows dialog title. Keep native menu IDs and function identifiers unchanged.

- [ ] **Step 4: Update native diagnostics, install paths, and default metadata**

  Replace standalone visible product copy in Rust diagnostics, comments, user-agent fixtures, managed shell-command marker/messages, and application-path fixtures. Update Windows display-directory segments to `QingYu` while retaining the `markra` executable/command.

  Update `tauri.conf.json` to `productName: "QingYu"` without changing `identifier`. Update contributor/permission descriptions in `Cargo.toml`, `capabilities/main.json`, and its tracked generated schema copy.

  Set both `CFBundleDisplayName` and `CFBundleName` in every macOS localization according to the naming matrix.

- [ ] **Step 5: Run native and package tests**

  Run:

  ```bash
  cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
  pnpm --filter @markra/desktop exec vitest run src/app-package-integration.test.ts src/runtime/tauri/file.test.ts src/runtime/tauri/shell-command.test.ts
  ```

  Expected: PASS, including the localized native name and preserved identifier assertions.

- [ ] **Step 6: Commit native/package copy**

  ```bash
  git add apps/desktop/src-tauri apps/desktop/src/app-package-integration.test.ts
  git commit -m "feat: brand native desktop surfaces as QingYu"
  ```

---

## Task 4: Update release assets and distribution copy

**Files:**

- Modify: `.github/workflows/release.yml`
- Modify: `scripts/release/create-macos-open-helper.mjs`
- Modify: `scripts/release/create-macos-open-helper.test.mjs`
- Modify: `scripts/release/generate-homebrew-cask.mjs`
- Modify: `scripts/release/generate-homebrew-cask.test.mjs`
- Modify: `scripts/release/normalize-release-artifacts.mjs`
- Modify: `scripts/release/normalize-release-artifacts.test.mjs`
- Modify: `scripts/release/prepend-macos-unsigned-notice.mjs`
- Modify: `scripts/release/prepend-macos-unsigned-notice.test.mjs`
- Modify: `scripts/release/rebuild-macos-dmg-with-helper.mjs`
- Modify: `scripts/release/rebuild-macos-dmg-with-helper.test.mjs`
- Modify: `scripts/release/repair-linux-appimage-gtk-ime.mjs`

- [ ] **Step 1: Change release tests to the new artifact display name**

  Update fixtures and expectations to use `QingYu.app`, `QingYu.exe`, `QingYu-*.rpm`, `QingYu-macOS-Open-Anyway.command`, the `QingYu` DMG volume name, and Homebrew display name `QingYu`. Keep `APP_SLUG=markra`, `markra.rb`, the cask token, URLs, and repository names unchanged.

- [ ] **Step 2: Run release tests and confirm they fail**

  Run:

  ```bash
  pnpm test:release
  ```

  Expected: FAIL because release script defaults and generated text still use the old display name.

- [ ] **Step 3: Update release implementation and workflow copy**

  Set `APP_PRODUCT_NAME: QingYu` in the workflow and update visible release/cask commit text. Change default product names, helper filenames/content, DMG labels, AppImage policy comments, Homebrew `name` and `app` entries, and unsigned-build notices to `QingYu`.

  Do not change the lowercase artifact slug, Homebrew cask filename/token, GitHub organization/repository paths, or updater endpoints.

- [ ] **Step 4: Run release tests and confirm they pass**

  Run:

  ```bash
  pnpm test:release
  ```

  Expected: all release-script tests PASS.

- [ ] **Step 5: Commit release copy**

  ```bash
  git add .github/workflows/release.yml scripts/release
  git commit -m "build: publish QingYu display names"
  ```

---

## Task 5: Rewrite repository prose and complete residual verification

**Files:**

- Modify: `.github/ISSUE_TEMPLATE/ai-provider.yml`
- Modify: `.github/ISSUE_TEMPLATE/bug-report.yml`
- Modify: `.github/ISSUE_TEMPLATE/editor-markdown.yml`
- Modify: `.github/ISSUE_TEMPLATE/feature-request.yml`
- Modify: `.github/ISSUE_TEMPLATE/installation.yml`
- Modify: `AGENTS.md`
- Modify: `CHANGELOG.md`
- Modify: `CONTRIBUTING.md`
- Modify: `DESIGN.md`
- Modify: `PRODUCT.md`
- Modify: `README.md`
- Modify: `README.zh-CN.md`
- Modify: `docs/privacy.md`
- Modify: `docs/superpowers/plans/2026-07-16-cross-platform-app-icon.md`
- Modify: `docs/superpowers/specs/2026-07-16-cross-platform-app-icon-design.md`
- Modify: `docs/superpowers/specs/2026-07-16-view-mode-shortcut-design.md`
- Modify: `docs/superpowers/plans/2026-07-16-qingyu-project-sync-config.md`
- Modify: `docs/superpowers/specs/2026-07-16-qingyu-project-sync-config-design.md`
- Modify: `docs/testing/results/2026-07-16-s3-sync-minio.md`
- Modify: `docs/testing/s3-sync-desktop-acceptance.md`
- Modify: `docs/testing/s3-sync-minio.md`
- Modify: any additional tracked text file reported by `pnpm brand:verify`

- [ ] **Step 1: Update current and historical prose**

  Replace standalone product copy in English/default documents with `QingYu`; use `轻语` in `README.zh-CN.md`. Rewrite historical `CHANGELOG.md` prose as requested, but retain all lowercase package names, commands, file names, issue links, repository URLs, and release endpoints.

- [ ] **Step 2: Run the tracked-copy verifier**

  Run:

  ```bash
  pnpm brand:verify
  ```

  Expected: PASS with zero tracked standalone legacy display-name references. If it reports files omitted above, classify each match as visible prose or a display-name fixture, update it according to language, and rerun until clean. Do not silence findings with broad exclusions.

- [ ] **Step 3: Audit protected technical identifiers**

  Run:

  ```bash
  rg -n '"name": "@markra/|dev\.markra\.app|\.markraignore|\.markra-sync|\.qingyu/|QingYuProjectConfig|qingyu://|COMMAND_NAME: &str = "markra"|markra:file|github\.com/markrahq/markra' package.json apps packages README.md README.zh-CN.md docs .github
  ```

  Expected: protected lowercase package, identifier, command, storage/file, menu-ID, and URL forms are still present. Review the diff to confirm no wholesale lowercase rename occurred.

- [ ] **Step 4: Run the complete verification suite**

  Run:

  ```bash
  pnpm test
  pnpm test:release
  pnpm typecheck:test
  cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
  pnpm build
  pnpm tauri build --debug --bundles app
  ```

  Expected: all tests and builds PASS. The Rust command must use its default parallel runner; do not add `--test-threads=1`. On macOS, confirm the generated application exists as `apps/desktop/src-tauri/target/debug/bundle/macos/QingYu.app` and inspect its localized `Contents/Resources/*.lproj/InfoPlist.strings` names against the matrix.

- [ ] **Step 5: Review and commit all remaining prose**

  Run:

  ```bash
  git status --short
  git diff --check
  git diff --stat
  ```

  Expected: only intended rebrand files in the isolated worktree; no generated `target/`, `dist/`, or cache content is staged. The primary checkout's pre-existing untracked `bg.png` remains untouched.

  Commit:

  ```bash
  git add .github AGENTS.md CHANGELOG.md CONTRIBUTING.md DESIGN.md PRODUCT.md README.md README.zh-CN.md docs
  git commit -m "docs: rename product copy to QingYu"
  ```

- [ ] **Step 6: Final clean-tree acceptance**

  Run:

  ```bash
  pnpm brand:verify
  git diff --check main...HEAD
  git status --short
  ```

  Expected: verifier PASS; diff check has no output; the isolated worktree is clean. Separately confirm the primary checkout still shows only `?? bg.png`.
