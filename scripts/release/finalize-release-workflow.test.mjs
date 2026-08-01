import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const repoRoot = fileURLToPath(new URL("../..", import.meta.url));
const workflowPath = path.join(repoRoot, ".github", "workflows", "finalize-release.yml");

test("Finalize Release validates an existing draft before publishing without rebuilding or replacing notes", () => {
  assert.equal(fs.existsSync(workflowPath), true, "Finalize Release workflow should exist");
  const workflow = fs.readFileSync(workflowPath, "utf8");

  assert.match(workflow, /^name: Finalize Release$/m);
  assert.match(workflow, /group: release-mutation-\$\{\{ github\.repository \}\}/);
  assert.match(workflow, /^on:\n  workflow_dispatch:\n    inputs:\n      tag:/m);
  assert.doesNotMatch(workflow, /^  (?:push|pull_request|schedule):/m);
  assert.match(workflow, /node scripts\/release\/validate-release-draft\.mjs/);
  assert.match(workflow, /gh release edit "\$\{RELEASE_TAG\}"[\s\S]*?--draft=false/);
  assert.match(workflow, /steps\.validate\.outputs\.already_published != 'true'/);
  assert.match(workflow, /name: Verify published release/);
  assert.doesNotMatch(workflow, /pnpm app build|tauri-action|generate-release-notes\.mjs/u);
  const publishStep = workflow.slice(
    workflow.indexOf("- name: Publish validated draft"),
    workflow.indexOf("- name: Verify published release"),
  );
  assert.doesNotMatch(publishStep, /--notes|--notes-file|--title/u);
  assert.doesNotMatch(workflow, /gh release (?:delete|create) "?\$\{RELEASE_TAG\}/u);
});

test("Finalize Release performs signed distribution only after published-state verification", () => {
  const workflow = fs.readFileSync(workflowPath, "utf8");
  const verifyIndex = workflow.indexOf("- name: Verify published release");
  const previewIndex = workflow.indexOf("- name: Publish preview updater manifest");
  const homebrewIndex = workflow.indexOf("- name: Prepare Homebrew tap checkout");

  assert.ok(verifyIndex > 0);
  assert.ok(previewIndex > verifyIndex);
  assert.ok(homebrewIndex > verifyIndex);
  assert.match(workflow, /steps\.validate\.outputs\.signed_release == 'true'/);
  assert.match(workflow, /env\.HOMEBREW_TAP_TOKEN != ''/);
  assert.match(workflow, /gh release upload preview release-assets\/latest\.json --clobber/);
  assert.match(workflow, /node scripts\/release\/generate-homebrew-cask\.mjs/);
});
