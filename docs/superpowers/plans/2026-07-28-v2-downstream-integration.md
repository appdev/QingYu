# QingYu V2 Downstream Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Merge `upstream/v2` into the customized QingYu downstream while adopting CodeMirror, preserving downstream capabilities, and keeping removed AI, proxy, and theme-export functionality absent.

**Architecture:** Create an ancestry bridge from the downstream tree to `upstream/main`, merge `upstream/v2`, and resolve the result by capability. V2 owns the editor architecture; QingYu owns downstream branding, sync, lifecycle, MCP, site, and release behavior; the hard-removal contract overrides both sides.

**Tech Stack:** pnpm workspace, React, TypeScript, CodeMirror 6, Tauri v2, Rust, Vitest.

## Global Constraints

- Use `pnpm` for every JavaScript dependency, test, typecheck, and build command.
- Do not add a second package-manager lockfile.
- Do not use the TypeScript `void` keyword or operator.
- Preserve the S3 note-folder `SyncProvider`, shared remote-sync engine, triggers, conflicts, and live MinIO coverage.
- Preserve QingYu branding, icons, MCP, document history, cloud restore, Dejavu design/plan documents, site, and downstream release behavior.
- Keep AI/Agent/providers, CodeMirror AI preview, Network/proxy, SOCKS, and theme export removed.
- Do not push to `upstream` or `origin`.

---

### Task 1: Establish the isolated and reproducible baseline

**Files:**
- No product files modified.

**Interfaces:**
- Consumes: local `main` at `b409813e13c409a0295ab079d95bc5bb4a96a543` and `upstream/v2` at `801ce49815bf01657bdec886f306ad97a4892913`.
- Produces: clean `codex/v2-integration` worktree with known test-runtime commands.

- [x] **Step 1: Create the worktree**

```bash
git worktree add .worktrees/v2-integration -b codex/v2-integration main
```

- [x] **Step 2: Install dependencies**

```bash
pnpm install --frozen-lockfile
```

- [x] **Step 3: Establish the JavaScript baseline under Node 24**

```bash
PATH=/Users/ying/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/bin:/opt/homebrew/bin:$PATH pnpm test
```

Expected: all tests pass, or the known Save As test is the sole full-suite failure and passes when rerun by exact title.

- [x] **Step 4: Establish the Rust baseline with the stable toolchain**

```bash
PATH=/Users/ying/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
```

Expected: exit 0.

### Task 2: Record the downstream ancestry and merge V2

**Files:**
- Modify through Git merge: all paths changed by `upstream/v2`.
- Preserve: `docs/superpowers/specs/2026-07-28-v2-downstream-integration-design.md`.
- Preserve: `docs/superpowers/plans/2026-07-28-v2-downstream-integration.md`.

**Interfaces:**
- Consumes: clean integration branch and the fetched `upstream/main`/`upstream/v2` refs.
- Produces: one bridge commit and one pending V2 merge with conflicts exposed against the correct merge base.

- [ ] **Step 1: Commit the approved integration documents**

```bash
git add docs/superpowers/specs/2026-07-28-v2-downstream-integration-design.md docs/superpowers/plans/2026-07-28-v2-downstream-integration.md
git commit -m "docs: define V2 downstream integration"
```

- [ ] **Step 2: Create the upstream-main ancestry bridge without changing the tree**

```bash
git merge -s ours upstream/main --allow-unrelated-histories -m "chore: bridge QingYu downstream to upstream main"
```

- [ ] **Step 3: Start the V2 merge**

```bash
git merge upstream/v2 --no-ff --no-commit -X ours
```

Expected: non-conflicting V2 changes are applied automatically, conflicting hunks retain the downstream baseline, delete/modify conflicts remain explicit, and no primary-checkout file is modified. The later tasks deliberately add supported V2 behavior that was hidden by a downstream-favored conflict hunk.

### Task 3: Adopt the V2 editor without its AI seam

**Files:**
- Accept and adapt: `packages/editor/src/codemirror/**`.
- Accept and adapt: `packages/editor/src/index.ts`.
- Accept and adapt: `packages/editor/package.json`.
- Create from V2: `packages/editor-react/**`.
- Modify: `packages/app/src/components/CodeMirrorPaperSurface.tsx`.
- Modify: `packages/app/src/components/CodeMirrorEditorFloatingMenus.tsx`.
- Modify: `packages/app/src/hooks/useCodeMirrorEditorController.ts`.
- Modify: `packages/app/src/components/MarkdownPaper.tsx`.
- Delete: `packages/editor/src/ai-preview-events.ts`.
- Delete: `packages/editor/src/codemirror/ai-preview.ts`.
- Delete: `packages/editor/src/codemirror/ai-preview.test.ts`.
- Delete: `packages/editor/src/codemirror/ai-selection-hold.ts`.

**Interfaces:**
- Consumes: V2 CodeMirror controller and React provider APIs.
- Produces: Markdown-first CodeMirror editor with no AI preview parameters, events, decorations, or UI contract.

- [ ] **Step 1: Resolve editor manifests and accept the CodeMirror module graph**

Use V2 dependencies and exports for CodeMirror, then remove AI-only exports and package edges. Preserve downstream Markdown asset/path helpers.

- [ ] **Step 2: Remove AI preview modules and references**

```bash
git rm --ignore-unmatch packages/editor/src/ai-preview-events.ts packages/editor/src/codemirror/ai-preview.ts packages/editor/src/codemirror/ai-preview.test.ts packages/editor/src/codemirror/ai-selection-hold.ts
rg -n -i "ai-preview|AI_EDITOR_PREVIEW|aiSelectionHold|aiPreview" packages/editor packages/editor-react packages/app/src/components packages/app/src/hooks
```

Expected: the scan has no runtime references after mixed controller/component files are adapted.

- [ ] **Step 3: Resolve the app editor host around CodeMirror**

Keep V2 document-change, focus, selection, formatting, floating-menu, preview, source-mode, and preference contracts. Remove AI preview props and callbacks rather than stubbing them.

- [ ] **Step 4: Run focused editor tests**

```bash
PATH=/Users/ying/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/bin:/opt/homebrew/bin:$PATH pnpm --filter @markra/editor test
PATH=/Users/ying/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/bin:/opt/homebrew/bin:$PATH pnpm --filter @markra/editor-react test
```

Expected: both packages pass.

### Task 4: Reconstruct the supported application and native runtime union

**Files:**
- Modify: `packages/app/src/App.tsx`.
- Modify: `packages/app/src/runtime/index.ts`.
- Modify: `apps/desktop/src/runtime/index.ts`.
- Modify: `apps/desktop/src-tauri/src/lib.rs`.
- Modify: `apps/desktop/src-tauri/Cargo.toml`.
- Modify: `apps/desktop/src-tauri/tauri.conf.json`.
- Preserve/adapt: `apps/desktop/src-tauri/src/remote_sync/**`.
- Preserve/adapt: `apps/desktop/src-tauri/src/markdown_files/history.rs`.
- Preserve/adapt: QingYu MCP sources under `apps/desktop/src-tauri/src/mcp/**` and `apps/desktop/src-tauri/src/bin/qingyu-mcp.rs`.
- Delete: `apps/desktop/src-tauri/src/ai_http.rs`.
- Delete: `apps/desktop/src-tauri/src/network.rs`.
- Delete: `apps/desktop/src/runtime/tauri/network.ts`.

**Interfaces:**
- Consumes: V2 CodeMirror app surface and local Tauri services.
- Produces: a runtime feature contract containing editor, files, history, sync, restore, MCP, updater, and web-image behavior without AI or proxy APIs.

- [ ] **Step 1: Resolve `App.tsx` by composing V2 editor state with local product state**

Retain local S3, restore, history, MCP, window, and toast wiring. Adopt V2 editor controller and file-list controls. Remove every AI agent, command, preview, and provider branch. Do not import the separate unmerged Dejavu implementation worktree.

- [ ] **Step 2: Resolve TypeScript runtime indexes**

Export the supported local runtime adapters and V2 file/editor additions. Do not export AI, provider, attachment, or network settings adapters.

- [ ] **Step 3: Resolve the native command registry**

Register the local sync, history, settings, updater, web-resource, MCP, and V2 safe asset-cleanup commands. Do not register AI or proxy commands.

- [ ] **Step 4: Remove native proxy capability**

```bash
git rm --ignore-unmatch apps/desktop/src-tauri/src/ai_http.rs apps/desktop/src-tauri/src/network.rs apps/desktop/src/runtime/tauri/network.ts
```

Remove `socks` from the `reqwest` feature list and keep strict removed-field rejection in request boundaries.

- [ ] **Step 5: Verify focused native behavior**

```bash
PATH=/Users/ying/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync
PATH=/Users/ying/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml markdown_file_history
```

Expected: selected tests pass.

### Task 5: Resolve settings, branding, icons, locales, site, and release behavior

**Files:**
- Modify: `packages/app/src/components/SettingsWindow.tsx`.
- Modify: `packages/app/src/components/SettingsShell.tsx`.
- Modify: `packages/app/src/lib/settings/app-settings.ts`.
- Modify: `packages/app/src/hooks/useSettingsWindowState.ts`.
- Modify: `packages/shared/src/i18n/locales/types.ts`.
- Modify: `packages/shared/src/i18n/locales/*.ts`.
- Preserve: `assets/branding/**`.
- Preserve: downstream icons referenced by `apps/desktop/src-tauri/tauri.conf.json`.
- Preserve: `apps/site/**`.
- Modify: `.github/workflows/release.yml` and `scripts/release/**` only where V2 prerelease support composes with the downstream workflow.

**Interfaces:**
- Consumes: V2 non-AI editor preferences and downstream settings contracts.
- Produces: QingYu-branded settings and release surfaces with CodeMirror options but no deleted categories.

- [ ] **Step 1: Merge editor and appearance settings**

Add V2 Vim, typewriter, custom-theme-toggle, and retained appearance preferences. Keep local sync/cloud ownership and remove Network/AI settings state and persistence.

- [ ] **Step 2: Merge locale types and translations**

Add strings required by supported V2 features. Do not add AI, provider, proxy, or theme-export keys.

- [ ] **Step 3: Preserve QingYu branding and icon inputs**

Keep downstream icon files and Tauri icon paths. Do not replace them with the upstream QingYu adaptive-icon artwork.

- [ ] **Step 4: Preserve site and downstream release contracts**

Keep `apps/site` and QingYu repository links. Integrate only compatible prerelease behavior into the existing metadata-derived release workflow.

### Task 6: Regenerate dependencies and enforce the removal contract

**Files:**
- Modify: `package.json`.
- Modify: `pnpm-lock.yaml`.
- Modify: `apps/desktop/src-tauri/Cargo.lock`.
- Delete: `packages/ai/**`.
- Delete: `packages/providers/**`.

**Interfaces:**
- Consumes: final supported workspace manifests.
- Produces: deterministic dependency graphs with no removed packages or SOCKS feature.

- [ ] **Step 1: Remove deleted workspace packages and current product assets**

```bash
git rm -r --ignore-unmatch packages/ai packages/providers
git rm --ignore-unmatch .github/ISSUE_TEMPLATE/ai-provider.yml assets/screenshots/ai-agent-panel.png assets/screenshots/ai-edit-preview.png assets/screenshots/ai-provider-settings.png
```

- [ ] **Step 2: Regenerate lockfiles**

```bash
PATH=/Users/ying/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/bin:/opt/homebrew/bin:$PATH pnpm install --lockfile-only
PATH=/Users/ying/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH cargo metadata --manifest-path apps/desktop/src-tauri/Cargo.toml --format-version 1 --no-deps
```

- [ ] **Step 3: Run hard-removal scans**

```bash
rg -n -i "packages/ai|packages/providers|AiAgent|AiCommand|AI_EDITOR_PREVIEW|ai-preview|NetworkSettings|proxyUrl|proxy_url|settings\.network|canExport|exportCurrent|exportNativeTheme|write_theme_archive|ThemePackageExport|default-light\.css|default-dark\.css" --glob '!CHANGELOG.md' --glob '!docs/superpowers/**' .
rg -n 'reqwest\s*=.*socks' apps/desktop/src-tauri/Cargo.toml
```

Expected: no current-runtime or dependency matches; any test-only removed-input fixture is reviewed and retained only when it proves strict rejection.

### Task 7: Verify and land the integration

**Files:**
- Modify through merge commit: the resolved repository tree.

**Interfaces:**
- Consumes: resolved V2 integration with deletion scans clean.
- Produces: verified local `main` at the V2 integration commit and no remote push.

- [ ] **Step 1: Format-check Rust**

```bash
PATH=/Users/ying/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
```

- [ ] **Step 2: Run the full repository gate**

```bash
PATH=/Users/ying/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
PATH=/Users/ying/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/bin:/opt/homebrew/bin:$PATH pnpm test
PATH=/Users/ying/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/bin:/opt/homebrew/bin:$PATH pnpm typecheck:test
PATH=/Users/ying/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/bin:/opt/homebrew/bin:$PATH pnpm build
```

Expected: all commands exit 0. If the known Save As full-suite flake is the only failure, rerun its exact focused test and record both results.

- [ ] **Step 3: Run live S3 coverage when configured**

```bash
PATH=/Users/ying/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/bin:/opt/homebrew/bin:$PATH pnpm test:s3-sync:live
```

Expected: run only when all required `MARKRA_TEST_S3_*` variables are present; otherwise record the skip.

- [ ] **Step 4: Commit the resolved merge**

```bash
git add -A
git commit
```

- [ ] **Step 5: Fast-forward local main and clean up**

```bash
git -C /Volumes/extendData/Data/IdeaProjects/markra merge --ff-only codex/v2-integration
git worktree remove /Volumes/extendData/Data/IdeaProjects/markra/.worktrees/v2-integration
git worktree prune
git branch -d codex/v2-integration
```

Expected: local `main` points to the verified integration result, unrelated primary-checkout changes are untouched, and no push occurs.
