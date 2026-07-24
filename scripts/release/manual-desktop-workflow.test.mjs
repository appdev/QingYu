import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const workflowsRoot = path.join(repoRoot, ".github", "workflows");

function readWorkflow(name) {
  return fs.readFileSync(path.join(workflowsRoot, name), "utf8");
}

function topLevelSection(source, name) {
  const lines = source.split(/\r?\n/);
  const start = lines.findIndex((line) => line === `${name}:`);
  assert.notEqual(start, -1, `${name} section should exist`);

  let end = lines.length;
  for (let index = start + 1; index < lines.length; index += 1) {
    if (/^[A-Za-z0-9_-]+:/.test(lines[index])) {
      end = index;
      break;
    }
  }
  return lines.slice(start, end).join("\n");
}

test("every GitHub workflow is manual-only", () => {
  for (const fileName of ["ci.yml", "release.yml", "desktop.yml"]) {
    const trigger = topLevelSection(readWorkflow(fileName), "on");
    assert.match(trigger, /^  workflow_dispatch:/m, `${fileName} should allow manual dispatch`);
    assert.doesNotMatch(trigger, /^  (?:push|pull_request|schedule):/m, `${fileName} should not run automatically`);
  }
});

test("release workflow derives its tag from the project version without manual version input", () => {
  const workflow = readWorkflow("release.yml");
  const trigger = topLevelSection(workflow, "on");

  assert.doesNotMatch(trigger, /^      tag_name:/m);
  assert.match(trigger, /^      draft:/m);
  assert.match(workflow, /release_version: \$\{\{ steps\.release_version\.outputs\.release_version \}\}/);
  assert.match(workflow, /release_tag: \$\{\{ steps\.release_version\.outputs\.release_tag \}\}/);
  assert.match(workflow, /const releaseTag = `v\$\{rootPackage\.version\}`;/);
  assert.equal(
    workflow.match(/RELEASE_TAG: \$\{\{ needs\.validate_release\.outputs\.release_tag \}\}/g)?.length,
    4,
  );
});

test("release updater metadata targets appdev QingYu", () => {
  const workflow = readWorkflow("release.yml");
  assert.match(
    workflow,
    /TAURI_UPDATER_ENDPOINT: https:\/\/github\.com\/appdev\/QingYu\/releases\/latest\/download\/latest\.json/
  );
  assert.doesNotMatch(workflow, /github\.com\/markrahq\/markra\/releases/);
});

test("desktop workflow builds only unsigned native desktop bundles", () => {
  const workflow = readWorkflow("desktop.yml");
  const osEntries = [...workflow.matchAll(/^\s+os: /gm)];

  assert.equal(osEntries.length, 3);
  assert.match(workflow, /name: macOS \(Apple Silicon\)[\s\S]*?os: macos-15[\s\S]*?artifact_name: macos-arm64/);
  assert.match(
    workflow,
    /name: Windows \(x64\)[\s\S]*?os: windows-latest[\s\S]*?artifact_name: windows-x64[\s\S]*?build_args: --bundles nsis/
  );
  assert.match(workflow, /name: Linux \(x64\)[\s\S]*?os: ubuntu-22\.04[\s\S]*?artifact_name: linux-x64/);
  assert.match(workflow, /run: pnpm app build desktop --no-sign \$\{\{ matrix\.build_args \}\}/);
  assert.match(workflow, /uses: actions\/upload-artifact@v4/);
  assert.match(workflow, /path: apps\/desktop\/src-tauri\/target\/release\/bundle/);
  assert.doesNotMatch(workflow, /\b(?:android|ios)\b/i);
  assert.doesNotMatch(workflow, /secrets\./);
});
