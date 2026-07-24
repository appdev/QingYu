import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const repoRoot = fileURLToPath(new URL("../..", import.meta.url));

function makeTempDir() {
  return fs.mkdtempSync(path.join(os.tmpdir(), "markra-macos-open-helper-"));
}

function runHelperScript(env) {
  return spawnSync(process.execPath, ["scripts/release/create-macos-open-helper.mjs"], {
    cwd: repoRoot,
    encoding: "utf8",
    env: {
      ...process.env,
      ...env,
    },
  });
}

test("create-macos-open-helper writes an executable quarantine repair script", () => {
  const rootDir = makeTempDir();
  const outputPath = path.join(rootDir, "QingYu-macOS-Open-Anyway.command");

  const result = runHelperScript({
    OUTPUT_PATH: outputPath,
  });

  assert.equal(result.status, 0, result.stderr);

  const script = fs.readFileSync(outputPath, "utf8");
  const mode = fs.statSync(outputPath).mode;

  assert.equal(mode & 0o111, 0o111);
  assert.match(script, /^#!\/bin\/bash/);
  assert.match(script, /APP_PATH="\/Applications\/QingYu\.app"/);
  assert.match(script, /only removes the macOS quarantine flag/);
  assert.match(script, /Continue\? \[y\/N\]/);
  assert.match(script, /xattr -dr com\.apple\.quarantine "\$APP_PATH"/);
});
