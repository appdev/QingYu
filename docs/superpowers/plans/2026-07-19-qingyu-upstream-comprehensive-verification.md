# QingYu Upstream Integration and Comprehensive Verification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Integrate the 11 commits currently on `upstream/main` into QingYu without restoring removed AI or spellcheck capabilities, then verify normal and abnormal desktop, Compact, editor, file, and sync behavior with automated and live runtime evidence.

**Architecture:** Work only in the isolated `codex/upstream-2026-07-19` worktree until the integrated commit passes every gate. Treat local `main` as the product source of truth, selectively retain upstream fixes that still apply, preserve the local S3/WebDAV engine and Compact shell, and reject upstream-only AI changes. After automated coverage, use the desktop Vite runtime and an isolated-identifier Tauri QA bundle for responsive and native verification.

**Tech Stack:** Git worktrees, pnpm workspace, React 19, TypeScript, Vitest, Milkdown, Tailwind CSS, Rust, Tauri v2, Cargo, Browser/Computer Use, macOS.

## Global Constraints

- Use `pnpm` for JavaScript and frontend workflows; keep `pnpm-lock.yaml` and do not add another lockfile.
- Preserve local S3 and WebDAV sync behavior, triggers, conflict handling, recovery, and MinIO coverage.
- Preserve the Compact managed-workspace model and full-screen file/settings navigation.
- Do not restore `packages/ai`, `packages/providers`, AI UI, AI settings, AI commands, or spellcheck runtime/settings removed from local `main`.
- Do not stage, delete, rename, or modify the user-owned `/Volumes/extendData/Data/IdeaProjects/markra/bg.png`.
- Do not push to `origin` or `upstream`.
- Do not initialize a new Android platform project during verification; report the absent `src-tauri/gen/android` and absent device/AVD as environment limits.
- Do not treat missing `MARKRA_TEST_S3_*` credentials as a product failure; record live MinIO as unavailable and rely on the deterministic sync suites for this run.
- Every success statement must cite fresh command or live UI evidence from the integrated commit.

---

### Task 1: Integrate the fetched upstream commits without restoring removed product areas

**Files:**
- Modify as selected by Git: `CHANGELOG.md`, root/app package metadata, editor sources, shared shortcut/i18n sources, and `apps/desktop/src-tauri/src/remote_sync.rs`
- Preserve deletion: `packages/ai/**`, `packages/providers/**`, `packages/app/src/App.ai.test.tsx`, AI hooks/components/settings, and spellcheck-only sources
- Review closely: `packages/app/src/App.tsx`, `packages/app/src/hooks/useEditorController.ts`, `packages/app/src/lib/settings/app-settings.ts`, `packages/shared/src/i18n/locales/types.ts`
- Test: upstream-targeted Rust, editor, app, shared, and Compact tests named below

**Interfaces:**
- Consumes: local `main` at `a081464` and fetched `upstream/main` at `81c6174`
- Produces: one integration commit on `codex/upstream-2026-07-19` that has both parents and contains no restored AI/spellcheck product surface

- [ ] **Step 1: Start a no-commit merge and enumerate conflicts**

```bash
git merge --no-ff --no-commit upstream/main
git status --short
git diff --name-only --diff-filter=U
```

Expected: upstream changes are staged or conflicted; `bg.png` is absent because this worktree starts from committed content only.

- [ ] **Step 2: Resolve delete/modify conflicts with local product boundaries**

For AI-only paths, preserve the local deletion:

```bash
git status --short | rg '^(DU|UD|AA|UU) '
git rm -r --ignore-unmatch packages/ai packages/providers
```

For shared files, retain these upstream behaviors when their owning code still exists:

- WebDAV remote paths contain exactly one separator between path components.
- Quick Open arrow-key navigation remains stable.
- Paired inline HTML tags render and escape correctly in visual Markdown.
- Exported numerals retain improved rendering.
- Markdown source line numbers are controlled by the surviving editor preferences and are rendered in both main and side source editors.

Reject ACP/client-model changes whose local owners were deleted. Resolve remaining TypeScript/Rust conflicts with `apply_patch`, then verify no conflict markers remain:

```bash
rg -n '^(<<<<<<<|=======|>>>>>>>)' apps packages || true
git diff --name-only --diff-filter=U
```

Expected: both commands return no unresolved file or conflict marker.

- [ ] **Step 3: Verify removed product areas did not return**

```bash
test ! -d packages/ai
test ! -d packages/providers
rg -n '@markra/ai|@markra/providers|AiAgent|AiCommand|aiAgentSessionId|spellcheckEnabled|spellcheckLanguage' apps packages --glob '!**/dist/**' --glob '!**/target/**' || true
```

Expected: directories are absent and the search returns no runtime/test references.

- [ ] **Step 4: Run upstream-targeted regression tests**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml remote_sync::tests -- --nocapture
pnpm --filter @markra/editor exec vitest run --environment jsdom --globals src/inline-html.test.ts
pnpm --filter @markra/editor test
pnpm --filter @markra/app exec vitest run src/components/QuickOpenPanel.test.tsx src/components/MarkdownPaper.test.tsx src/components/MarkdownSourceEditor.test.tsx src/lib/document-export.test.ts src/lib/settings/app-settings.test.ts
pnpm --filter @markra/shared test
```

Expected: every targeted suite exits 0.

- [ ] **Step 5: Stage only tracked integration content and commit**

```bash
git add -u
git add CHANGELOG.md apps packages pnpm-lock.yaml package.json docs/superpowers/plans/2026-07-19-qingyu-upstream-comprehensive-verification.md
git diff --cached --check
git commit -m "merge: integrate upstream v1.7.3 fixes"
```

Expected: a merge commit is created; no generated `dist`, `target`, or `node_modules` path is staged.

### Task 2: Verify normal application behavior with deterministic suites

**Files:**
- Test: `packages/app/src/App.test.tsx`
- Test: `packages/app/src/App.compact-files.test.tsx`
- Test: `packages/app/src/components/compact/*.test.tsx`
- Test: `packages/app/src/hooks/useManagedWorkspace.test.tsx`
- Test: `packages/app/src/hooks/useMarkdownDocument.test.tsx`
- Test: `packages/app/src/hooks/useMarkdownFileTree.test.tsx`
- Test: `packages/editor/src/*.test.ts`

**Interfaces:**
- Consumes: committed integrated tree from Task 1
- Produces: passing evidence for editor/file/Compact normal flows

- [ ] **Step 1: Run Compact and managed-workspace normal-flow coverage**

```bash
pnpm --filter @markra/app exec vitest run \
  src/App.compact-files.test.tsx \
  src/components/compact/CompactAcceptance.test.tsx \
  src/components/compact/CompactAppShell.test.tsx \
  src/components/compact/CompactEditorScreen.test.tsx \
  src/components/compact/CompactEditorToolbar.test.tsx \
  src/components/compact/CompactFileBrowserScreen.test.tsx \
  src/components/compact/CompactMoveTargetScreen.test.tsx \
  src/components/compact/CompactSettingsHome.test.tsx \
  src/components/compact/CompactSettingsDetail.test.tsx \
  src/hooks/useManagedWorkspace.test.tsx
```

Expected: editor, full-screen file navigation, settings, move, persisted preferences, managed-root bootstrap, and restore tests all pass.

- [ ] **Step 2: Run document and editor normal-flow coverage**

```bash
pnpm --filter @markra/app exec vitest run \
  src/App.test.tsx \
  src/hooks/useEditorController.test.ts \
  src/hooks/useMarkdownDocument.test.tsx \
  src/hooks/useMarkdownFileTree.test.tsx
pnpm --filter @markra/editor test
```

Expected: open/edit/save/history/search/tree/task-list/inline-HTML behavior exits 0.

### Task 3: Verify abnormal, conflict, failure, and recovery states

**Files:**
- Test: `packages/app/src/hooks/useCompactAutoSave.test.tsx`
- Test: `packages/app/src/hooks/useCompactSyncSettings.test.tsx`
- Test: `packages/app/src/components/compact/CompactSyncFormScreen.test.tsx`
- Test: `packages/app/src/components/compact/CompactSyncStatusScreen.test.tsx`
- Test: `packages/app/src/hooks/useProjectSyncSettingsSession.test.tsx`
- Test: `apps/desktop/src-tauri/src/project_config/**`
- Test: `apps/desktop/src-tauri/src/remote_sync/**`

**Interfaces:**
- Consumes: local sync engine and Compact controller APIs
- Produces: evidence that failures are finite, explicit, non-destructive, retryable where specified, and do not leak credentials

- [ ] **Step 1: Run Compact autosave and sync failure-state suites**

```bash
pnpm --filter @markra/app exec vitest run \
  src/hooks/useCompactAutoSave.test.tsx \
  src/hooks/useCompactSyncSettings.test.tsx \
  src/components/compact/CompactSyncFormScreen.test.tsx \
  src/components/compact/CompactSyncStatusScreen.test.tsx \
  src/hooks/useProjectSyncSettingsSession.test.tsx \
  src/hooks/useManagedWorkspace.test.tsx \
  src/hooks/useCompactNavigation.test.tsx
```

Expected: disk-full/read-only/permission failures, malformed and unsupported sync config, connection failures, retry paths, no-workspace state, stale navigation, and teardown failures all pass their assertions.

- [ ] **Step 2: Run native project-config and sync safety suites with default parallelism**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
```

Expected: path traversal, symlink replacement, stale revision, malformed config, recovery backup, WebDAV/S3 redaction, upload/download/delete/conflict, and parallel execution tests pass; only the 21 explicitly ignored live MinIO tests remain ignored.

- [ ] **Step 3: Record external integration availability without exposing secrets**

```bash
env | sed 's/=.*//' | rg '^MARKRA_TEST_S3_' || true
command -v docker || true
command -v minio || true
adb devices -l
test -d apps/desktop/src-tauri/gen/android && echo android-project-present || echo android-project-absent
```

Expected for the current machine: no `MARKRA_TEST_S3_*`, Docker, MinIO, connected Android device, AVD, or generated Android project. Record live MinIO and Android device/APK execution as environment gaps rather than passing product tests.

### Task 4: Verify responsive Compact behavior in a live browser

**Files:**
- Runtime: `apps/desktop/src/App.tsx`
- Runtime: `packages/app/src/hooks/useCompactMode.ts`
- Runtime: `packages/app/src/components/compact/CompactAppShell.tsx`
- Evidence: ignored screenshots under `.superpowers/sdd/artifacts/`

**Interfaces:**
- Consumes: desktop Vite runtime at `http://127.0.0.1:1420/`
- Produces: DOM, accessibility, geometry, and screenshot evidence at Compact and desktop boundaries

- [ ] **Step 1: Start the Vite runtime and verify health**

```bash
pnpm --filter @markra/desktop exec vite --host 127.0.0.1 --port 1420 --strictPort
curl --fail --silent --show-error http://127.0.0.1:1420/ >/dev/null
```

Expected: Vite serves HTTP 200. If 1420 belongs to a pre-existing user process, leave it untouched and use an adjacent task-owned port; this run used 1421.

- [ ] **Step 2: Verify normal Compact layout at 390×844 and the width boundary**

Using the Browser skill, set viewports to `390×844`, `720×844`, and `721×844` and verify:

- `390` and `720` render the Compact editor; `721` renders desktop chrome.
- Compact editor, file page, settings page, and sync page have no horizontal overflow.
- File and settings pages occupy the full viewport, not a half-width drawer.
- All primary controls have at least 44px targets.
- Opening files/settings keeps the editor DOM mounted but removes it from the accessibility tree; Back restores it.
- Opening Files, Settings, Sync, and Back does not throw console errors.

- [ ] **Step 3: Verify live abnormal states**

At `390×844`, verify the no-workspace Sync page terminates with a clear message rather than a spinner and exposes at most the Back action. Verify browser runtime unavailability does not expose Configure/Sync Now actions. Save screenshots for editor, file page, settings, sync error, and the `720/721` boundary.

- [ ] **Step 4: Stop the Vite process cleanly**

Send `Ctrl-C` to the exact Vite session and verify its task-owned port is no longer served; do not stop a pre-existing process on 1420.

### Task 5: Verify the integrated native macOS application

**Files:**
- Bundle: `apps/desktop/src-tauri/target/debug/bundle/macos/QingYu Comprehensive QA.app`
- Evidence: ignored screenshots under `.superpowers/sdd/artifacts/`

**Interfaces:**
- Consumes: integrated Tauri/Rust/frontend build
- Produces: native runtime and accessibility evidence without touching the production app identifier

- [ ] **Step 1: Build an isolated-identifier debug app**

```bash
pnpm tauri build --debug --bundles app --no-sign --config '{"productName":"QingYu Comprehensive QA","identifier":"dev.markra.app.comprehensiveqa"}'
```

Expected: the `.app` bundle is produced and code signing is intentionally skipped.

- [ ] **Step 2: Verify wide and Compact native modes with Computer Use**

Launch the exact bundle path. Verify wide mode shows desktop chrome, resize to phone width, then verify Compact editor, bottom toolbar, full-screen Files, full-screen Settings, and Back transitions. In Files/Settings accessibility trees, confirm Markdown editor and format toolbar are absent; after Back they are present again.

- [ ] **Step 3: Verify native abnormal Sync state**

From the default empty workspace, open Settings → Sync. Verify the page displays a finite no-workspace/unavailable explanation, no indefinite spinner, no reset action, and no cloud mutation action. Capture a screenshot and return to the editor.

- [ ] **Step 4: Close only the QA app**

Use Computer Use to quit `QingYu Comprehensive QA`; do not close or alter any production QingYu window.

### Task 6: Run final gates, update local main, and clean up isolation

**Files:**
- Verify: complete repository
- Preserve: `/Volumes/extendData/Data/IdeaProjects/markra/bg.png`

**Interfaces:**
- Consumes: verified integration branch
- Produces: locally updated `main`, clean integration cleanup, and a final evidence report

- [ ] **Step 1: Run the complete integrated gate again**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
pnpm test
pnpm typecheck:test
pnpm build
git diff --check
git status --short
```

Expected: all executable gates exit 0 and the integration worktree has no tracked changes.

- [ ] **Step 2: Re-check topology and merge into local main**

From `/Volumes/extendData/Data/IdeaProjects/markra`, verify `main` has not advanced unexpectedly, then merge `codex/upstream-2026-07-19` with a normal merge or fast-forward as topology permits. Do not fetch-and-reset and do not push.

```bash
git log --oneline --decorate --graph --max-count=16 main codex/upstream-2026-07-19
git merge --ff-only codex/upstream-2026-07-19 || git merge --no-ff codex/upstream-2026-07-19
```

Expected: local `main` contains upstream `81c6174` and the verified integration commit.

- [ ] **Step 3: Run the final post-merge gate on main**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
pnpm test
pnpm typecheck:test
pnpm build
git diff --check
git status --short
```

Expected: all gates exit 0; only the pre-existing `?? bg.png` remains.

- [ ] **Step 4: Remove the task-owned worktree and branch**

```bash
git worktree remove /Volumes/extendData/Data/IdeaProjects/markra/.worktrees/upstream-2026-07-19
git worktree prune
git branch -d codex/upstream-2026-07-19
```

Expected: the integration worktree and temporary branch are gone; `main`, `origin`, `upstream`, and `bg.png` are unchanged except for the verified local merge.
