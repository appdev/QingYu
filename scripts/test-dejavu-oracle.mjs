import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const DEJAVU_URL = "https://github.com/siyuan-note/dejavu.git";
export const DEJAVU_COMMIT = "8462fe30163c6e6e95ae2da832cfe76058e0e830";
export const TIMEOUTS = Object.freeze({
  gitQuery: 10_000,
  gitClone: 120_000,
  gitCheckout: 30_000,
  goScenarios: 180_000,
  goAll: 300_000,
  cargoScenarios: 300_000,
});
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

function formatCommand(executable, args) {
  return [executable, ...args].join(" ");
}

function boundedOutput(output, limit = 2_000) {
  const normalized = String(output ?? "").trim();
  if (normalized.length <= limit) {
    return normalized;
  }
  return `${normalized.slice(0, limit)}... (truncated)`;
}

function commandFailure(executable, args, timeoutMs, result) {
  const command = formatCommand(executable, args);
  const signal = result.signal ?? "none";
  const status = result.status ?? "none";
  const diagnostics = `command=${command}; timeout=${timeoutMs}ms; signal=${signal}; status=${status}`;
  if (result.error?.code === "ETIMEDOUT") {
    return new Error(`command timed out: ${diagnostics}`);
  }
  if (result.error) {
    return new Error(`command failed to start: ${diagnostics}; error=${result.error.message}`);
  }
  const stderr = boundedOutput(result.stderr);
  return new Error(
    `command failed: ${diagnostics}${stderr === "" ? "" : `; stderr=${stderr}`}`,
  );
}

function spawnCommand(executable, args, cwd, timeoutMs, stdio) {
  if (!Number.isSafeInteger(timeoutMs) || timeoutMs <= 0) {
    throw new Error(
      `invalid command timeout for ${formatCommand(executable, args)}: ${String(timeoutMs)}`,
    );
  }
  const result = spawnSync(executable, args, {
    cwd,
    encoding: "utf8",
    stdio,
    timeout: timeoutMs,
    killSignal: "SIGTERM",
    maxBuffer: 1024 * 1024,
  });
  if (result.error || result.status !== 0) {
    throw commandFailure(executable, args, timeoutMs, result);
  }
  return result;
}

export function commandOutput(executable, args, cwd, timeoutMs) {
  const result = spawnCommand(executable, args, cwd, timeoutMs, ["ignore", "pipe", "pipe"]);
  return result.stdout.trim();
}

export function run(executable, args, cwd, timeoutMs) {
  process.stdout.write(`> ${executable} ${args.join(" ")}\n`);
  spawnCommand(executable, args, cwd, timeoutMs, "inherit");
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

function summarizeDirtyStatus(status) {
  const allLines = status.split(/\r?\n/).filter((line) => line !== "");
  const visibleLines = allLines.slice(0, 20);
  let summary = visibleLines.join("\n");
  if (summary.length > 2_000) {
    summary = `${summary.slice(0, 2_000)}... (truncated)`;
  } else if (allLines.length > visibleLines.length) {
    summary = `${summary}\n... (${allLines.length - visibleLines.length} more entries)`;
  }
  return summary;
}

export function verifyPinnedSource(
  sourceDirectory,
  expectedCommit = DEJAVU_COMMIT,
  timeouts = TIMEOUTS,
) {
  requireDirectory(sourceDirectory, "Dejavu source");
  let head;
  try {
    head = commandOutput(
      "git",
      ["-C", sourceDirectory, "rev-parse", "HEAD"],
      WORKSPACE_ROOT,
      timeouts.gitQuery,
    );
  } catch (error) {
    throw new Error(`cannot read Dejavu HEAD at ${sourceDirectory}: ${error.message}`);
  }
  if (head !== expectedCommit) {
    throw new Error(
      `Dejavu HEAD mismatch at ${sourceDirectory}: expected ${expectedCommit}, actual ${head}`,
    );
  }
  let status;
  try {
    status = commandOutput(
      "git",
      ["-C", sourceDirectory, "status", "--porcelain=v1", "--untracked-files=all"],
      WORKSPACE_ROOT,
      timeouts.gitQuery,
    );
  } catch (error) {
    throw new Error(`cannot read Dejavu status at ${sourceDirectory}: ${error.message}`);
  }
  if (status !== "") {
    throw new Error(
      `Dejavu source is dirty at ${sourceDirectory}:\n${summarizeDirtyStatus(status)}`,
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

export function removeOwnedClone(ownedRoot, sourceDirectory) {
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

export function executeOracle({
  environment = process.env,
  cloneUrl = DEJAVU_URL,
  onOwnedRoot = () => {},
  timeouts = TIMEOUTS,
} = {}) {
  let ownedRoot;
  let sourceDirectory;

  try {
    if (environment.DEJAVU_SOURCE_DIR) {
      sourceDirectory = path.resolve(environment.DEJAVU_SOURCE_DIR);
    } else {
      ownedRoot = fs.mkdtempSync(path.join(os.tmpdir(), "qingyu-dejavu-oracle-"));
      sourceDirectory = path.join(ownedRoot, "dejavu");
      onOwnedRoot(ownedRoot);
      run(
        "git",
        ["clone", "--no-checkout", cloneUrl, sourceDirectory],
        WORKSPACE_ROOT,
        timeouts.gitClone,
      );
      run(
        "git",
        ["-C", sourceDirectory, "checkout", "--detach", DEJAVU_COMMIT],
        WORKSPACE_ROOT,
        timeouts.gitCheckout,
      );
    }

    verifyPinnedSource(sourceDirectory, DEJAVU_COMMIT, timeouts);
    verifyFixtures(sourceDirectory);
    run(
      "go",
      ["test", "./test/sync", "-count=1", "-v"],
      sourceDirectory,
      timeouts.goScenarios,
    );
    run("go", ["test", "./...", "-count=1"], sourceDirectory, timeouts.goAll);
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
      timeouts.cargoScenarios,
    );
  } finally {
    if (ownedRoot !== undefined && sourceDirectory !== undefined) {
      removeOwnedClone(ownedRoot, sourceDirectory);
    }
  }
}

const isMain =
  process.argv[1] !== undefined &&
  path.resolve(process.argv[1]) === path.resolve(fileURLToPath(import.meta.url));

if (isMain) {
  try {
    executeOracle();
  } catch (error) {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  }
}
