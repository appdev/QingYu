import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const repoRoot = fileURLToPath(new URL("../..", import.meta.url));

function makeTempDir() {
  return fs.mkdtempSync(path.join(os.tmpdir(), "markra-appimage-policy-"));
}

function writeFile(filePath, content = "") {
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  fs.writeFileSync(filePath, content);
}

function runVerify(appDir, env = {}) {
  return spawnSync(process.execPath, ["scripts/release/verify-linux-appimage-libraries.mjs", appDir], {
    cwd: repoRoot,
    encoding: "utf8",
    env: {
      ...process.env,
      ...env,
    },
  });
}

test("verify-linux-appimage-libraries accepts AppDirs without bundled Wayland client libraries", () => {
  const appDir = makeTempDir();
  writeFile(path.join(appDir, "usr", "lib", "libgtk-3.so.0"));

  const result = runVerify(appDir);

  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /No forbidden AppImage libraries found/);
});

test("verify-linux-appimage-libraries rejects bundled Wayland client library variants", () => {
  const appDir = makeTempDir();
  const libraryPath = path.join(appDir, "usr", "lib", "libwayland-client.so.0.24.0");
  writeFile(libraryPath);

  const result = runVerify(appDir);

  assert.equal(result.status, 1);
  assert.match(result.stderr, /libwayland-client\.so\.0\.24\.0/);
});

test("verify-linux-appimage-libraries can read the AppDir path from the environment", () => {
  const appDir = makeTempDir();
  writeFile(path.join(appDir, "usr", "bin", "markra"));

  const result = spawnSync(process.execPath, ["scripts/release/verify-linux-appimage-libraries.mjs"], {
    cwd: repoRoot,
    encoding: "utf8",
    env: {
      ...process.env,
      APPIMAGE_APPDIR: appDir,
    },
  });

  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /No forbidden AppImage libraries found/);
});
