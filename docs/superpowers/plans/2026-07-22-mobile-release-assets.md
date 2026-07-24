# QingYu Mobile Release Assets Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend the manual QingYu Release workflow to build desktop plus mobile artifacts, publish them to GitHub Releases, and list the exact commits since the previous published Release.

**Architecture:** Keep one manual Release orchestration workflow with a validation gate, the existing desktop matrix, separate Android and iOS jobs, and a final publishing job. A focused Node module owns GitHub Release discovery and deterministic note rendering; workflow YAML owns platform setup and artifact collection. Desktop signing remains optional and mobile outputs are explicitly labelled as unsigned Android and iOS Simulator artifacts.

**Tech Stack:** GitHub Actions, Node.js 24 ESM scripts and `node:test`, pnpm 10, Tauri v2, Rust, Android SDK/NDK, Xcode.

## Global Constraints

- Every workflow must remain `workflow_dispatch` only; no push, pull-request, tag, or schedule trigger.
- Release builds use the workflow dispatch commit and create the requested tag only after every build succeeds.
- The requested tag must equal `v` plus the application version and must not move an existing tag.
- Desktop signing/updater generation is optional and secret values must never be printed.
- Android output is `QingYu_<version>_android_arm64_unsigned.apk`.
- iOS output is `QingYu_<version>_ios_simulator_arm64_unsigned.app.zip` and is only for Apple Silicon Simulator.
- Release notes use the newest published, non-draft, ancestor Release as the previous boundary and include every commit in chronological order.
- If no prior published Release exists, release notes include all commits reachable from the release target.
- No keystore, certificate, provisioning profile, or generated build output may be committed.
- Preserve the user's existing `apps/desktop/src-tauri/Cargo.toml` feature edit and untracked `macos-icon.icns`.

---

### Task 1: Deterministic Release Notes Generator

**Files:**
- Create: `scripts/release/generate-release-notes.mjs`
- Create: `scripts/release/generate-release-notes.test.mjs`

**Interfaces:**
- Produces: `selectPreviousRelease(releases, options) -> release | null`
- Produces: `parseGitLog(output) -> Array<{ sha, shortSha, subject, author }>`
- Produces: `renderReleaseNotes({ currentTag, previousTag, commits }) -> string`
- CLI consumes: `GITHUB_REPOSITORY`, `GITHUB_TOKEN`, `RELEASE_TAG`, `RELEASE_TARGET`, and optional `RELEASE_NOTES_PATH`
- CLI writes: UTF-8 Markdown to `RELEASE_NOTES_PATH`, defaulting to `release-notes.md`

- [ ] **Step 1: Write failing pure-function tests**

Create tests that pass synthetic GitHub release objects and injected `tagExists`/`isAncestor` callbacks. Cover newest published ancestor selection, skipping drafts/current/non-ancestors, a `null` first-Release result, chronological commit rows, and the two mobile disclosure bullets.

```js
test("selectPreviousRelease chooses the newest published ancestor", () => {
  const selected = selectPreviousRelease(releases, {
    currentTag: "v1.7.5",
    tagExists: (tag) => tag !== "v9.0.0",
    isAncestor: (tag) => tag === "v1.7.4",
  });
  assert.equal(selected?.tag_name, "v1.7.4");
});

test("renderReleaseNotes discloses unsigned mobile artifacts", () => {
  const notes = renderReleaseNotes({ currentTag: "v1.7.5", previousTag: "v1.7.4", commits });
  assert.match(notes, /Android ARM64.*未签名/u);
  assert.match(notes, /iOS.*Apple Silicon.*Simulator/u);
  assert.match(notes, /`abc1234` feat: add mobile release — QingYu/u);
});
```

- [ ] **Step 2: Run the focused test and observe the expected failure**

Run: `node --test scripts/release/generate-release-notes.test.mjs`

Expected: FAIL because `generate-release-notes.mjs` does not exist or does not export the required functions.

- [ ] **Step 3: Implement the pure functions and CLI**

Use built-in `fetch` to request `GET /repos/{owner}/{repo}/releases?per_page=100&page=N` with `Authorization: Bearer`. Sort non-draft releases by `published_at`, select the first existing ancestor tag, and execute Git with `execFileSync` rather than shell interpolation.

```js
const range = previousTag ? `${previousTag}..${releaseTarget}` : releaseTarget;
const rawLog = runGit(["log", "--reverse", "--format=%H%x1f%h%x1f%s%x1f%an%x1e", range]);
const commits = parseGitLog(rawLog);
fs.writeFileSync(outputPath, renderReleaseNotes({ currentTag, previousTag, commits }));
```

The renderer must emit `## 提交记录`, the previous/current range explanation, each commit as ``- `<shortSha>` <subject> — <author>``, and `## 移动端产物说明` with exact Android/iOS scope.

- [ ] **Step 4: Run focused tests**

Run: `node --test scripts/release/generate-release-notes.test.mjs`

Expected: all release-note tests PASS.

- [ ] **Step 5: Commit the generator**

```bash
git add scripts/release/generate-release-notes.mjs scripts/release/generate-release-notes.test.mjs
git commit -m "feat: generate release notes from published releases"
```

### Task 2: Manual Multi-platform Release Workflow

**Files:**
- Modify: `.github/workflows/release.yml`
- Modify: `scripts/release/resolve-tauri-build-args.mjs`
- Modify: `scripts/release/normalize-release-artifacts.test.mjs`
- Create: `scripts/release/mobile-release-workflow.test.mjs`
- Modify: `scripts/release/manual-desktop-workflow.test.mjs`

**Interfaces:**
- Consumes: `scripts/release/generate-release-notes.mjs` CLI from Task 1
- Produces: `validate_release`, desktop `build`, `build_android`, `build_ios`, and `publish_release` jobs
- Produces: uploaded artifact families `markra-*-bundles`, `markra-Android-arm64-bundles`, and `markra-iOS-simulator-arm64-bundles`
- `resolve-tauri-build-args.mjs` additionally consumes `ENABLE_SIGNED_RELEASE=true|false`

- [ ] **Step 1: Write failing workflow contract tests**

Assert that the workflow:

```js
assert.match(workflow, /build_android:/);
assert.match(workflow, /pnpm app build android --apk --target aarch64 --ci/);
assert.match(workflow, /QingYu_\$\{version\}_android_arm64_unsigned\.apk/);
assert.match(workflow, /build_ios:/);
assert.match(workflow, /pnpm app build ios --target aarch64-sim --no-sign --ci/);
assert.match(workflow, /QingYu_\$\{version\}_ios_simulator_arm64_unsigned\.app\.zip/);
assert.match(workflow, /node scripts\/release\/generate-release-notes\.mjs/);
assert.match(workflow, /target_commitish: \$\{\{ github\.sha \}\}/);
assert.doesNotMatch(workflow, /pnpm dlx changelogen@latest/);
```

Also require `validate_release` to compare `tag_name` with every application version source, reject an existing tag on a different commit, and require all build jobs before `publish_release`.

- [ ] **Step 2: Run the release test suite and observe the expected failure**

Run: `pnpm test:release`

Expected: FAIL in the new mobile workflow contract tests because mobile jobs and the release-note script invocation are absent.

- [ ] **Step 3: Add the validation job and dispatch-commit source model**

Change every release checkout to:

```yaml
- name: Checkout repository
  uses: actions/checkout@v4
  with:
    fetch-depth: 0
    ref: ${{ github.sha }}
```

Create one `validate_release` job that validates the root, desktop, web, site, Tauri, and Cargo versions and uses `git rev-parse "refs/tags/${RELEASE_TAG}^{commit}"` to reject an existing tag that differs from `${GITHUB_SHA}`. Make all build jobs depend on it and remove the duplicate release-scripts checkout.

- [ ] **Step 4: Make desktop signing optional without removing the signed path**

Add a capability step that outputs `signed_release=true` only when updater and macOS signing/notarization secret sets are complete. Pass the result to `resolve-tauri-build-args.mjs`. When false, append `--no-sign`; when true, append `--config tauri.updater.conf.json`. Gate certificate import, notarization setup, updater metadata, AppImage signer invocation, and `latest.json` generation on that output.

```js
if (readEnv("ENABLE_SIGNED_RELEASE") === "true") {
  args.push("--config", "tauri.updater.conf.json");
} else {
  args.push("--no-sign");
}
```

- [ ] **Step 5: Add Android and iOS build jobs**

Android uses `actions/setup-java@v4`, Rust target `aarch64-linux-android`, the preinstalled Android SDK/NDK, and the exact public build command. A normalization step finds the release APK and copies it into `release-mobile/QingYu_${version}_android_arm64_unsigned.apk` before artifact upload.

iOS uses `macos-15`, Rust target `aarch64-apple-ios-sim`, and the exact no-sign simulator command. A normalization step finds `QingYu.app`, uses `ditto -c -k --sequesterRsrc --keepParent`, and uploads `release-mobile/QingYu_${version}_ios_simulator_arm64_unsigned.app.zip`.

- [ ] **Step 6: Publish all assets and deterministic notes**

Make `publish_release.needs` include validation, desktop, Android, and iOS jobs. Call the Task 1 script with `RELEASE_TARGET=${{ github.sha }}` and `GITHUB_TOKEN=${{ github.token }}`. Download `markra-*-bundles`, exclude only internal `release-metadata.json`, and publish with:

```yaml
tag_name: ${{ env.RELEASE_TAG }}
target_commitish: ${{ github.sha }}
body_path: release-notes.md
files: ${{ steps.release_files.outputs.paths }}
```

- [ ] **Step 7: Run workflow and release tests**

Run: `pnpm test:release`

Expected: all release tests PASS, including existing artifact normalization and Homebrew tests.

- [ ] **Step 8: Commit workflow implementation**

```bash
git add .github/workflows/release.yml scripts/release/resolve-tauri-build-args.mjs scripts/release/normalize-release-artifacts.test.mjs scripts/release/mobile-release-workflow.test.mjs scripts/release/manual-desktop-workflow.test.mjs
git commit -m "ci: publish desktop and mobile release assets"
```

### Task 3: Version 1.7.5 Release Sources

**Files:**
- Modify: `package.json`
- Modify: `apps/desktop/package.json`
- Modify: `apps/web/package.json`
- Modify: `apps/site/package.json`
- Modify: `apps/desktop/src-tauri/tauri.conf.json`
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/Cargo.lock`

**Interfaces:**
- Consumes: validation rules from Task 2
- Produces: consistent application version `1.7.5` and requested release tag `v1.7.5`

- [ ] **Step 1: Add a failing version contract assertion**

Extend the workflow contract test to read every version source and assert exact equality with `1.7.5`, including the first `markra` package record in `Cargo.lock`.

- [ ] **Step 2: Run the focused test and observe the expected failure**

Run: `node --test scripts/release/mobile-release-workflow.test.mjs`

Expected: FAIL because the checked-in versions are `1.7.4`.

- [ ] **Step 3: Update all version sources**

Change only the application version fields to `1.7.5`. In `Cargo.toml`, preserve the user's unrelated `macos-private-api` feature edit in the worktree but stage only the version hunk for this commit.

- [ ] **Step 4: Run version and release tests**

Run: `pnpm test:release`

Expected: all tests PASS and the version validation fixtures agree on `1.7.5`.

- [ ] **Step 5: Commit the version bump without the user-owned feature hunk**

```bash
git add package.json apps/desktop/package.json apps/web/package.json apps/site/package.json apps/desktop/src-tauri/tauri.conf.json apps/desktop/src-tauri/Cargo.lock scripts/release/mobile-release-workflow.test.mjs
git apply --cached /tmp/qingyu-cargo-version-only.patch
git commit -m "chore: release v1.7.5"
```

After the commit, `git diff -- apps/desktop/src-tauri/Cargo.toml` must show only the pre-existing `macos-private-api` feature change.

### Task 4: Repository Verification and Publication

**Files:**
- Verify only; do not add generated outputs

**Interfaces:**
- Consumes: all prior tasks
- Produces: pushed `main`, GitHub tag `v1.7.5`, successful Release workflow run, and published GitHub Release assets/notes

- [ ] **Step 1: Run the repository verification gate**

Run:

```bash
pnpm test:release
pnpm test
pnpm typecheck:test
pnpm build
git diff --check
```

Expected: every command exits 0.

- [ ] **Step 2: Recheck user-owned state and commit scope**

Verify SHA-256 for `macos-icon.icns` remains `e3b34a0159027cd09c7d501924a6f43434e402c7d64452794a40dca5dd8f5444`. Verify `Cargo.toml` contains only the original user-owned feature diff after committed version changes. Remove or restore any generated drift left by build tools without touching those two items.

- [ ] **Step 3: Push `main`**

```bash
git push origin main
```

Expected: GitHub accepts the new commits and `origin/main` equals local `HEAD`.

- [ ] **Step 4: Dispatch the Release workflow**

Use the authenticated GitHub Actions API to dispatch `.github/workflows/release.yml` on `main` with:

```json
{"ref":"main","inputs":{"tag_name":"v1.7.5","draft":"false"}}
```

Expected: one new manual Release run appears for the pushed commit.

- [ ] **Step 5: Monitor and repair until the run succeeds**

Poll the workflow run and inspect failed job logs through the authenticated GitHub API. Apply only evidence-backed fixes, rerun the local release tests, commit, push, and dispatch again if required.

- [ ] **Step 6: Verify the published Release**

Query `GET /repos/appdev/QingYu/releases/tags/v1.7.5` and verify:

- `draft=false`, `tag_name=v1.7.5`, and `target_commitish` resolves to the pushed release commit.
- Release notes contain `## 提交记录`, chronological commit rows, and both mobile disclosure bullets.
- Assets include desktop packages plus `QingYu_1.7.5_android_arm64_unsigned.apk` and `QingYu_1.7.5_ios_simulator_arm64_unsigned.app.zip`.
- Every asset has a non-zero size and a browser download URL.

- [ ] **Step 7: Confirm workflows remain manual-only**

Query Actions runs for the release commit and confirm only the explicitly dispatched Release run exists; no push-triggered workflow may have started.
