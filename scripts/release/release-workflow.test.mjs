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

test("release workflow always creates a draft with AI permission and inspectable note inputs", () => {
  const workflow = fs.readFileSync(workflowPath, "utf8");

  assert.match(workflow, /^  publish_release:[\s\S]*?permissions:\n      contents: write\n      models: read/m);
  assert.match(workflow, /^          draft: true$/m);
  assert.match(workflow, /GITHUB_MODELS_MODEL: openai\/gpt-4\.1/);
  assert.match(workflow, /RELEASE_FACTS_PATH: release-facts\.json/);
  assert.match(workflow, /name: Upload generated release notes/);
  assert.match(workflow, /path: \|\n            release-notes\.md\n            release-facts\.json/);
  assert.doesNotMatch(workflow, /inputs\.draft/);
  assert.doesNotMatch(workflow, /Publish preview updater manifest/);
  assert.doesNotMatch(workflow, /Prepare Homebrew tap checkout/);
  assert.doesNotMatch(workflow, /Publish Homebrew cask to tap/);
});

test("release workflow excludes deb package internals from GitHub release assets", () => {
  const workflow = fs.readFileSync(workflowPath, "utf8");

  assert.match(workflow, /! -name 'control\.tar\.gz'/);
  assert.match(workflow, /! -name 'data\.tar\.gz'/);
});
