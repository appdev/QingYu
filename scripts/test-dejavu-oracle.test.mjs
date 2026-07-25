import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import {
  commandOutput,
  executeOracle,
  run,
  TIMEOUTS,
  verifyPinnedSource,
} from "./test-dejavu-oracle.mjs";

function git(repository, args, timeoutMs) {
  return commandOutput("git", args, repository, timeoutMs);
}

function createRepository(t) {
  const repository = fs.mkdtempSync(path.join(os.tmpdir(), "qingyu-dejavu-oracle-test-"));
  t.after(() => fs.rmSync(repository, { recursive: true, force: true }));
  git(repository, ["init", "--quiet"], TIMEOUTS.gitQuery);
  fs.writeFileSync(path.join(repository, "tracked.txt"), "clean\n");
  git(repository, ["add", "tracked.txt"], TIMEOUTS.gitQuery);
  git(
    repository,
    [
      "-c",
      "user.name=QingYu Test",
      "-c",
      "user.email=qingyu-test@example.invalid",
      "commit",
      "--quiet",
      "-m",
      "fixture",
    ],
    TIMEOUTS.gitQuery,
  );
  return {
    repository,
    head: git(repository, ["rev-parse", "HEAD"], TIMEOUTS.gitQuery),
  };
}

test("rejects a pinned source with tracked changes", (t) => {
  const { repository, head } = createRepository(t);
  fs.writeFileSync(path.join(repository, "tracked.txt"), "dirty\n");

  assert.throws(
    () => verifyPinnedSource(repository, head),
    (error) => {
      assert.match(error.message, /dirty/);
      assert.match(error.message, /tracked\.txt/);
      return true;
    },
  );
});

test("rejects a pinned source with untracked files", (t) => {
  const { repository, head } = createRepository(t);
  fs.writeFileSync(path.join(repository, "untracked.txt"), "dirty\n");

  assert.throws(
    () => verifyPinnedSource(repository, head),
    (error) => {
      assert.match(error.message, /dirty/);
      assert.match(error.message, /untracked\.txt/);
      return true;
    },
  );
});

test("reports subprocess timeout diagnostics", () => {
  const timeoutMs = 25;

  assert.throws(
    () =>
      run(
        process.execPath,
        ["-e", "setTimeout(() => {}, 150)"],
        process.cwd(),
        timeoutMs,
      ),
    (error) => {
      assert.match(error.message, new RegExp(path.basename(process.execPath)));
      assert.match(error.message, new RegExp(`${timeoutMs}ms`));
      assert.match(error.message, /signal=SIGTERM/);
      assert.match(error.message, /status=/);
      return true;
    },
  );
});

test("bounds Git setup commands with timeout diagnostics", (t) => {
  const repository = fs.mkdtempSync(path.join(os.tmpdir(), "qingyu-dejavu-git-timeout-"));
  t.after(() => fs.rmSync(repository, { recursive: true, force: true }));
  git(repository, ["init", "--quiet"], TIMEOUTS.gitQuery);
  git(
    repository,
    ["config", "alias.hang", `!${process.execPath} -e 'setTimeout(() => {}, 150)'`],
    TIMEOUTS.gitQuery,
  );
  const timeoutMs = 25;

  assert.throws(
    () => git(repository, ["hang"], timeoutMs),
    (error) => {
      assert.match(error.message, /command=git hang/);
      assert.match(error.message, new RegExp(`${timeoutMs}ms`));
      assert.match(error.message, /signal=SIGTERM/);
      assert.match(error.message, /status=/);
      return true;
    },
  );
});

test("removes an owned clone root when cloning fails", () => {
  let ownedRoot;
  const missingClone = path.join(
    os.tmpdir(),
    `qingyu-missing-dejavu-${process.pid}-${Date.now()}`,
  );

  assert.throws(() =>
    executeOracle({
      environment: {},
      cloneUrl: missingClone,
      onOwnedRoot: (directory) => {
        ownedRoot = directory;
      },
    }),
  );
  assert.notEqual(ownedRoot, undefined);
  assert.equal(fs.existsSync(ownedRoot), false);
});
