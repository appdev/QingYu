import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const repoRoot = fileURLToPath(new URL("../..", import.meta.url));

function makeTempDir() {
  return fs.mkdtempSync(path.join(os.tmpdir(), "markra-appimage-repair-"));
}

function writeFile(filePath, content = "") {
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  fs.writeFileSync(filePath, content);
}

function runRepair(appDir) {
  return spawnSync(process.execPath, ["scripts/release/repair-linux-appimage-libraries.mjs", appDir], {
    cwd: repoRoot,
    encoding: "utf8",
  });
}

test("repair-linux-appimage-libraries removes bundled Wayland client libraries", () => {
  const appDir = makeTempDir();
  const libraryPath = path.join(appDir, "usr", "lib", "libwayland-client.so.0");
  writeFile(libraryPath);
  writeFile(path.join(appDir, "usr", "lib", "libgtk-3.so.0"));

  const result = runRepair(appDir);

  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /Removed 1 forbidden AppImage librar/);
  assert.equal(fs.existsSync(libraryPath), false);
  assert.equal(fs.existsSync(path.join(appDir, "usr", "lib", "libgtk-3.so.0")), true);
});

test("repair-linux-appimage-libraries succeeds when there is nothing to remove", () => {
  const appDir = makeTempDir();
  writeFile(path.join(appDir, "usr", "bin", "markra"));

  const result = runRepair(appDir);

  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /No forbidden AppImage libraries to remove/);
});
