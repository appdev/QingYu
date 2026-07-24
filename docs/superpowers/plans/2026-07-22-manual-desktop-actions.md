# Manual Desktop GitHub Actions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every QingYu GitHub workflow manually triggered and add a secret-free desktop packaging workflow for macOS, Windows, and Linux.

**Architecture:** Existing CI and signed Release workflows remain available but lose all automatic triggers. A new Desktop Build matrix packages unsigned native bundles from the dispatched commit and uploads each runner's bundle directory. A focused Node test treats the trigger and platform set as a repository contract.

**Tech Stack:** GitHub Actions YAML, Node.js test runner, pnpm workspace runner, Tauri v2, Rust stable.

## Global Constraints

- The GitHub repository is `appdev/QingYu`.
- `workflow_dispatch` is the only allowed workflow trigger.
- The first operational build covers macOS, Windows, and Linux desktop targets only.
- Android and iOS are excluded.
- Desktop CI artifacts are unsigned and do not require repository secrets.
- Existing local edits to `apps/desktop/src-tauri/Cargo.toml` and `macos-icon.icns` stay outside the commits.

---

### Task 1: Lock the manual desktop workflow contract

**Files:**
- Create: `scripts/release/manual-desktop-workflow.test.mjs`
- Modify: `.github/workflows/ci.yml`
- Modify: `.github/workflows/release.yml`
- Create: `.github/workflows/desktop.yml`

**Interfaces:**
- Consumes: the repository's existing `pnpm app build desktop --no-sign` command.
- Produces: a `Desktop Build` workflow whose matrix uploads `qingyu-desktop-macos-arm64`, `qingyu-desktop-windows-x64`, and `qingyu-desktop-linux-x64` artifacts.

- [ ] **Step 1: Write the failing workflow contract test**

Create a Node test that reads all three workflow files, extracts each top-level `on` block, requires `workflow_dispatch`, rejects `push`, `pull_request`, and `schedule`, verifies the `appdev/QingYu` updater endpoint, and checks the Desktop Build file for exactly the three desktop runner entries, `pnpm app build desktop --no-sign`, artifact upload, and absence of Android/iOS commands.

- [ ] **Step 2: Run the focused test and confirm the red state**

Run:

```bash
node --test scripts/release/manual-desktop-workflow.test.mjs
```

Expected: failure because `desktop.yml` does not exist and existing workflows still contain automatic triggers.

- [ ] **Step 3: Implement the manual trigger changes**

Change the `on` blocks in `ci.yml` and `release.yml` to:

```yaml
on:
  workflow_dispatch:
```

Change the updater endpoint in `release.yml` to:

```yaml
TAURI_UPDATER_ENDPOINT: https://github.com/appdev/QingYu/releases/latest/download/latest.json
```

- [ ] **Step 4: Add the unsigned desktop matrix**

Create `desktop.yml` with a `workflow_dispatch` trigger, read-only contents permission, a three-entry matrix using `macos-15`, `windows-latest`, and `ubuntu-22.04`, pinned pnpm `10.30.3`, Node `24`, stable Rust, Linux Tauri system dependencies, frozen dependency installation, `pnpm app build desktop --no-sign`, and `actions/upload-artifact@v4` over `apps/desktop/src-tauri/target/release/bundle`.

- [ ] **Step 5: Verify the focused contract and release tooling**

Run:

```bash
node --test scripts/release/manual-desktop-workflow.test.mjs
pnpm test:release
git diff --check
```

Expected: all tests pass and no whitespace errors are reported.

- [ ] **Step 6: Commit the workflow implementation**

Stage only the three workflows, the focused test, and this plan. Commit with:

```bash
git commit -m "ci: add manual desktop builds"
```

### Task 2: Publish and accept the live workflow

**Files:**
- No source files unless the live run exposes a reproducible workflow defect.

**Interfaces:**
- Consumes: `origin=https://github.com/appdev/QingYu.git` and the committed `Desktop Build` workflow.
- Produces: a successful manual GitHub Actions run with three uploaded desktop artifact groups.

- [ ] **Step 1: Verify the exact push state**

Run:

```bash
git status --short
git remote -v
git log -2 --oneline
```

Expected: only the preserved Cargo/icon state is outside commits, and `origin` points to `appdev/QingYu`.

- [ ] **Step 2: Push local main**

Run:

```bash
git push origin main
```

Expected: GitHub accepts the update and remote `main` resolves to local `HEAD`.

- [ ] **Step 3: Dispatch Desktop Build**

Open `appdev/QingYu` Actions, select `Desktop Build`, dispatch `main`, and record the run URL and run ID.

- [ ] **Step 4: Monitor and diagnose**

Wait until every matrix job reaches a terminal state. On failure, read the failing job log, make the smallest reproducible workflow correction, rerun focused local checks, commit, push, and dispatch again.

- [ ] **Step 5: Verify successful artifacts and final repository state**

Require a successful workflow conclusion and three non-empty uploaded artifacts. Confirm remote `main` equals local `HEAD`, no automatic run was created by the push, and the working tree still contains only the preserved Cargo/icon state.
