import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const repoRoot = fileURLToPath(new URL("../..", import.meta.url));
const tauriDir = path.join(repoRoot, "apps", "desktop", "src-tauri");
const tauriConfig = JSON.parse(
  fs.readFileSync(path.join(tauriDir, "tauri.conf.json"), "utf8"),
);

function readWorkflowProductName(relativePath) {
  const lines = fs.readFileSync(path.join(repoRoot, relativePath), "utf8").split("\n");
  const rootEnvIndex = lines.indexOf("env:");
  assert.notEqual(rootEnvIndex, -1, `${relativePath} must define a root env block`);

  for (let index = rootEnvIndex + 1; index < lines.length; index += 1) {
    const line = lines[index];
    if (line !== "" && !line.startsWith("  ")) break;
    const match = line.match(/^  APP_PRODUCT_NAME:\s*(\S+)\s*$/u);
    if (match) return match[1];
  }

  assert.fail(`${relativePath} must define root APP_PRODUCT_NAME`);
}

function assertTrackedBundleInput(sourcePath) {
  const absolutePath = path.resolve(tauriDir, sourcePath);
  const repositoryPath = path.relative(repoRoot, absolutePath);
  assert.equal(
    fs.existsSync(absolutePath),
    true,
    `Tauri bundle input does not exist: ${repositoryPath}`,
  );
  const trackedFiles = execFileSync("git", ["ls-files", "--", repositoryPath], {
    cwd: repoRoot,
    encoding: "utf8",
  }).trim();
  assert.notEqual(trackedFiles, "", `Tauri bundle input is not tracked: ${repositoryPath}`);
}

test("Tauri and Release workflows use the QingYu product name", () => {
  const releaseProductName = readWorkflowProductName(".github/workflows/release.yml");
  const finalizeProductName = readWorkflowProductName(
    ".github/workflows/finalize-release.yml",
  );

  assert.equal(releaseProductName, "QingYu");
  assert.equal(finalizeProductName, "QingYu");
  assert.equal(tauriConfig.productName, releaseProductName);
  assert.equal(tauriConfig.productName, finalizeProductName);
});

test("Tauri bundle inputs exist in the tracked checkout", () => {
  const macOSFiles = tauriConfig.bundle.macOS?.files ?? {};
  assert.equal("Resources/Assets.car" in macOSFiles, false);
  assert.equal(Object.values(macOSFiles).includes("icons/generated/Assets.car"), false);

  for (const sourcePath of [
    ...tauriConfig.bundle.icon,
    ...Object.values(macOSFiles),
  ]) {
    assertTrackedBundleInput(sourcePath);
  }
});
