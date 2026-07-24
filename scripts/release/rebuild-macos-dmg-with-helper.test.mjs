import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const repoRoot = fileURLToPath(new URL("../..", import.meta.url));

function makeTempDir() {
  return fs.mkdtempSync(path.join(os.tmpdir(), "markra-dmg-helper-"));
}

function runRebuildScript(env) {
  return spawnSync(process.execPath, ["scripts/release/rebuild-macos-dmg-with-helper.mjs"], {
    cwd: repoRoot,
    encoding: "utf8",
    env: {
      ...process.env,
      ...env,
    },
  });
}

test("rebuild-macos-dmg-with-helper prepares bundle_dmg with the open helper file", () => {
  const rootDir = makeTempDir();
  const dmgDir = path.join(rootDir, "bundle", "dmg");
  const macosBundleDir = path.join(rootDir, "bundle", "macos");
  const helperPath = path.join(rootDir, "QingYu-macOS-Open-Anyway.command");
  const dryRunOutputPath = path.join(rootDir, "dry-run.json");

  fs.mkdirSync(path.join(macosBundleDir, "QingYu.app", "Contents"), { recursive: true });
  fs.mkdirSync(dmgDir, { recursive: true });
  fs.writeFileSync(path.join(dmgDir, "QingYu_0.0.12_aarch64.dmg"), "dmg");
  fs.writeFileSync(path.join(dmgDir, "bundle_dmg.sh"), "#!/usr/bin/env bash\n");
  fs.chmodSync(path.join(dmgDir, "bundle_dmg.sh"), 0o755);
  fs.writeFileSync(helperPath, "#!/bin/bash\n");

  const result = runRebuildScript({
    APP_PRODUCT_NAME: "QingYu",
    DMG_DIR: dmgDir,
    DRY_RUN: "1",
    DRY_RUN_OUTPUT_PATH: dryRunOutputPath,
    HELPER_PATH: helperPath,
    MACOS_BUNDLE_DIR: macosBundleDir,
  });

  assert.equal(result.status, 0, result.stderr);

  const dryRun = JSON.parse(fs.readFileSync(dryRunOutputPath, "utf8"));

  assert.equal(dryRun.dmgPath, path.join(dmgDir, "QingYu_0.0.12_aarch64.dmg"));
  assert.equal(dryRun.outputDmgPath, path.join(dmgDir, "QingYu_0.0.12_aarch64.with-helper.dmg"));
  assert.equal(dryRun.stagedAppPath.endsWith("QingYu.app"), true);
  assert.equal(dryRun.stagedAppExists, true);
  assert.deepEqual(dryRun.command.slice(0, 2), ["bash", path.join(dmgDir, "bundle_dmg.sh")]);
  assert.deepEqual(dryRun.command.slice(2, 6), ["--volname", "QingYu", "--window-size", "660"]);
  assert.equal(dryRun.command.includes("--add-file"), true);
  assert.equal(dryRun.command.includes("QingYu-macOS-Open-Anyway.command"), true);
  assert.equal(dryRun.command.includes(helperPath), true);
  assert.equal(dryRun.command.at(-2), path.join(dmgDir, "QingYu_0.0.12_aarch64.with-helper.dmg"));
});
