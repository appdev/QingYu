import { createHash } from "node:crypto";
import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const DEJAVU_URL = "https://github.com/siyuan-note/dejavu.git";
const DEJAVU_COMMIT = "8462fe30163c6e6e95ae2da832cfe76058e0e830";
const WORKSPACE_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const FIXTURES = [
  {
    relative: "basic/config.json",
    sha256: "1b4c0ef8c3c39e0b971260f30ff9bad4120f56d23d00cab42d78be97a1693268",
  },
  {
    relative: "edge/config.json",
    sha256: "eef8aa5389688989a39da511c30bbad87501ea04dee2fcf8b67ae288f5df1875",
  },
  {
    relative: "known-conflicts/config.json",
    sha256: "40941ce32657ff4fe08379a61e8e4e3ff2bf2ed7f489f46654b758cfb51b8596",
  },
  {
    relative: "sync-download/config.json",
    sha256: "6134a21d9deee1381498beb899a8ed2667c6c98f4b08e91f217d3cfff89fec24",
  },
];

function commandOutput(executable, args, cwd) {
  return execFileSync(executable, args, {
    cwd,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  }).trim();
}

function run(executable, args, cwd) {
  process.stdout.write(`> ${executable} ${args.join(" ")}\n`);
  const result = spawnSync(executable, args, {
    cwd,
    encoding: "utf8",
    stdio: "inherit",
  });
  if (result.error) {
    throw new Error(`failed to start ${executable}: ${result.error.message}`);
  }
  if (result.status !== 0) {
    throw new Error(`${executable} exited with status ${String(result.status)}`);
  }
}

function requireDirectory(directory, label) {
  let stat;
  try {
    stat = fs.statSync(directory);
  } catch (error) {
    throw new Error(`${label} is unavailable at ${directory}: ${error.message}`);
  }
  if (!stat.isDirectory()) {
    throw new Error(`${label} is not a directory: ${directory}`);
  }
}

function verifyPinnedHead(sourceDirectory) {
  requireDirectory(sourceDirectory, "Dejavu source");
  let head;
  try {
    head = commandOutput("git", ["-C", sourceDirectory, "rev-parse", "HEAD"], WORKSPACE_ROOT);
  } catch (error) {
    throw new Error(`cannot read Dejavu HEAD at ${sourceDirectory}: ${error.message}`);
  }
  if (head !== DEJAVU_COMMIT) {
    throw new Error(
      `Dejavu HEAD mismatch at ${sourceDirectory}: expected ${DEJAVU_COMMIT}, actual ${head}`,
    );
  }
}

function sha256(file) {
  return createHash("sha256").update(fs.readFileSync(file)).digest("hex");
}

function verifyFixtures(sourceDirectory) {
  const sourceRoot = path.join(sourceDirectory, "test", "sync", "testdata", "cases");
  const rustRoot = path.join(
    WORKSPACE_ROOT,
    "apps",
    "desktop",
    "src-tauri",
    "crates",
    "qingyu-dejavu",
    "tests",
    "fixtures",
    "dejavu",
    "cases",
  );
  for (const fixture of FIXTURES) {
    const sourceFile = path.join(sourceRoot, fixture.relative);
    const rustFile = path.join(rustRoot, fixture.relative);
    const sourceHash = sha256(sourceFile);
    const rustHash = sha256(rustFile);
    if (sourceHash !== fixture.sha256) {
      throw new Error(
        `pinned source fixture hash mismatch for ${fixture.relative}: expected ${fixture.sha256}, actual ${sourceHash}`,
      );
    }
    if (rustHash !== fixture.sha256) {
      throw new Error(
        `Rust fixture hash mismatch for ${fixture.relative}: expected ${fixture.sha256}, actual ${rustHash}`,
      );
    }
    if (!fs.readFileSync(sourceFile).equals(fs.readFileSync(rustFile))) {
      throw new Error(`fixture bytes differ for ${fixture.relative}`);
    }
  }
}

function removeOwnedClone(ownedRoot, sourceDirectory) {
  const tempRoot = path.resolve(os.tmpdir());
  const resolvedOwnedRoot = path.resolve(ownedRoot);
  const expectedSource = path.join(resolvedOwnedRoot, "dejavu");
  if (
    path.dirname(resolvedOwnedRoot) !== tempRoot ||
    !path.basename(resolvedOwnedRoot).startsWith("qingyu-dejavu-oracle-") ||
    path.resolve(sourceDirectory) !== expectedSource
  ) {
    throw new Error(`refuse to remove unverified oracle directory ${resolvedOwnedRoot}`);
  }
  fs.rmSync(resolvedOwnedRoot, { recursive: true, force: true });
}

let ownedRoot;
let sourceDirectory;

try {
  if (process.env.DEJAVU_SOURCE_DIR) {
    sourceDirectory = path.resolve(process.env.DEJAVU_SOURCE_DIR);
  } else {
    ownedRoot = fs.mkdtempSync(path.join(os.tmpdir(), "qingyu-dejavu-oracle-"));
    sourceDirectory = path.join(ownedRoot, "dejavu");
    run("git", ["clone", "--no-checkout", DEJAVU_URL, sourceDirectory], WORKSPACE_ROOT);
    run(
      "git",
      ["-C", sourceDirectory, "checkout", "--detach", DEJAVU_COMMIT],
      WORKSPACE_ROOT,
    );
  }

  verifyPinnedHead(sourceDirectory);
  verifyFixtures(sourceDirectory);
  run("go", ["test", "./test/sync", "-count=1", "-v"], sourceDirectory);
  run("go", ["test", "./...", "-count=1"], sourceDirectory);
  run(
    "cargo",
    [
      "test",
      "--manifest-path",
      path.join(WORKSPACE_ROOT, "apps/desktop/src-tauri/Cargo.toml"),
      "-p",
      "qingyu-dejavu",
      "--test",
      "scenarios",
    ],
    WORKSPACE_ROOT,
  );
} catch (error) {
  process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
  process.exitCode = 1;
} finally {
  if (ownedRoot !== undefined && sourceDirectory !== undefined) {
    try {
      removeOwnedClone(ownedRoot, sourceDirectory);
    } catch (error) {
      process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
      process.exitCode = 1;
    }
  }
}
