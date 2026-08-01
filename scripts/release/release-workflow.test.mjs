import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const repoRoot = fileURLToPath(new URL("../..", import.meta.url));
const workflowPath = path.join(repoRoot, ".github", "workflows", "release.yml");

test("release workflow resolves a stable or preview updater endpoint before bundling", () => {
  const workflow = fs.readFileSync(workflowPath, "utf8");

  assert.match(workflow, /name: Resolve updater endpoint/);
  assert.match(workflow, /resolve-updater-endpoint\.mjs/);
  assert.match(workflow, /TAURI_UPDATER_ENDPOINT: \$\{\{ steps\.updater_endpoint\.outputs\.endpoint \}\}/);
});

test("release workflow creates a draft with deterministic inspectable note inputs", () => {
  const workflow = fs.readFileSync(workflowPath, "utf8");

  assert.match(workflow, /^  publish_release:[\s\S]*?permissions:\n      contents: write/m);
  assert.match(workflow, /group: release-mutation-\$\{\{ github\.repository \}\}/);
  assert.match(workflow, /RELEASE_FACTS_PATH: release-facts\.json/);
  assert.match(workflow, /name: Upload generated release notes/);
  assert.match(workflow, /name: Publish release draft/);
  assert.match(workflow, /node scripts\/release\/publish-release-draft\.mjs/);
  assert.match(workflow, /ALLOWED_STALE_DRAFT_ID: \$\{\{ inputs\.replace_stale_draft_id \}\}/);
  assert.match(workflow, /RELEASE_FILES_PATH: release-files\.txt/);
  assert.match(workflow, /path: \|\n            release-notes\.md\n            release-facts\.json/);
  assert.doesNotMatch(workflow, /models: read/);
  assert.doesNotMatch(workflow, /GITHUB_MODELS/u);
  assert.doesNotMatch(workflow, /REQUIRE_GITHUB_MODELS/u);
  assert.doesNotMatch(workflow, /inputs\.draft/);
  assert.doesNotMatch(workflow, /Publish preview updater manifest/);
  assert.doesNotMatch(workflow, /Prepare Homebrew tap checkout/);
  assert.doesNotMatch(workflow, /Publish Homebrew cask to tap/);
  assert.doesNotMatch(workflow, /softprops\/action-gh-release/u);
  assert.doesNotMatch(workflow, /\$\{\{ github\.ref \}\}/u);
});

test("release workflow excludes deb package internals from GitHub release assets", () => {
  const workflow = fs.readFileSync(workflowPath, "utf8");

  assert.match(workflow, /! -name 'control\.tar\.gz'/);
  assert.match(workflow, /! -name 'data\.tar\.gz'/);
});

test("release workflow builds and publishes an Arch Linux package from the x64 deb", () => {
  const workflow = fs.readFileSync(workflowPath, "utf8");

  assert.match(workflow, /name: Prepare Arch Linux package/);
  assert.match(workflow, /run: node scripts\/release\/prepare-arch-package\.mjs/);
  assert.doesNotMatch(workflow, /\.release-scripts\/scripts\/release\/prepare-arch-package\.mjs/);
  assert.match(workflow, /archlinux:base-devel/);
  assert.match(workflow, /makepkg --noconfirm --nodeps/);
  assert.match(workflow, /QingYu_\$\{version\}_linux_x64\.pkg\.tar\.zst/);
  assert.match(workflow, /"\.pkg\.tar\.zst"/);
});
