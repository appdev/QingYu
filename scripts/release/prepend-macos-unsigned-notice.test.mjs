import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const repoRoot = fileURLToPath(new URL("../..", import.meta.url));

function makeTempDir() {
  return fs.mkdtempSync(path.join(os.tmpdir(), "markra-macos-notice-"));
}

function runNoticeScript(env) {
  return spawnSync(process.execPath, ["scripts/release/prepend-macos-unsigned-notice.mjs"], {
    cwd: repoRoot,
    encoding: "utf8",
    env: {
      ...process.env,
      ...env,
    },
  });
}

test("prepend-macos-unsigned-notice prepends the macOS unsigned app guidance once", () => {
  const rootDir = makeTempDir();
  const notesPath = path.join(rootDir, "release-notes.md");
  fs.writeFileSync(notesPath, "## Changes\n\n- Release v0.0.12\n");

  const env = { RELEASE_NOTES_PATH: notesPath };

  const firstResult = runNoticeScript(env);
  const secondResult = runNoticeScript(env);

  assert.equal(firstResult.status, 0, firstResult.stderr);
  assert.equal(secondResult.status, 0, secondResult.stderr);

  const notes = fs.readFileSync(notesPath, "utf8");
  const markerMatches = notes.match(/markra-macos-unsigned-notice/g) || [];

  assert.equal(markerMatches.length, 1);
  assert.match(notes, /^<!-- markra-macos-unsigned-notice -->/);
  assert.match(notes, /macOS build is currently unsigned/);
  assert.match(notes, /QingYu-macOS-Open-Anyway\.command/);
  assert.match(notes, /inside the macOS DMG/);
  assert.match(notes, /only removes the quarantine flag/);
  assert.match(notes, /## Changes\n\n- Release v0\.0\.12/);
});
