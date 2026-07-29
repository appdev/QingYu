import { spawn } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const tauriManifestPath = fileURLToPath(
  new URL("../../../apps/desktop/src-tauri/Cargo.toml", import.meta.url)
);

function commonTauriDependency(manifest) {
  const lines = manifest.split("\n");
  let inCommonDependencies = false;
  for (let index = 0; index < lines.length; index += 1) {
    const trimmed = lines[index].trim();
    if (trimmed.startsWith("[")) {
      inCommonDependencies = trimmed === "[dependencies]";
      continue;
    }
    if (inCommonDependencies && trimmed.startsWith("tauri =")) {
      return { index, line: lines[index] };
    }
  }
  return undefined;
}

export function restoreTauriManifestAfterCliRun(before, after) {
  const original = commonTauriDependency(before);
  const current = commonTauriDependency(after);
  if (!original || !current || original.line.includes('"macos-private-api"')) return after;

  const cliAdjusted = original.line.replace(
    /features\s*=\s*\[/,
    '$&"macos-private-api", '
  );
  if (current.line !== cliAdjusted) return after;

  const lines = after.split("\n");
  lines[current.index] = original.line;
  return lines.join("\n");
}

function restoreTauriManifestFile(before) {
  const current = readFileSync(tauriManifestPath, "utf8");
  const restored = restoreTauriManifestAfterCliRun(before, current);
  if (restored !== current) writeFileSync(tauriManifestPath, restored);

  const originalDependency = commonTauriDependency(before);
  const restoredDependency = commonTauriDependency(restored);
  if (
    originalDependency
    && !originalDependency.line.includes('"macos-private-api"')
    && restoredDependency?.line.includes('"macos-private-api"')
  ) {
    throw new Error("Tauri CLI changed the common dependency unexpectedly; Cargo.toml was not overwritten");
  }
}

export function tauriExitCode(code, signal, spawnFailed) {
  if (spawnFailed) return 1;
  if (code !== null) return code;
  if (signal === "SIGINT") return 130;
  if (signal === "SIGTERM") return 143;
  if (signal === "SIGHUP") return 129;
  return 1;
}

export function createTauriCommand(args, platform = process.platform) {
  const pnpmArgs = ["--filter", "@markra/desktop", "tauri", ...args];

  return {
    command: platform === "win32" ? "cmd.exe" : "pnpm",
    args: platform === "win32" ? ["/d", "/s", "/c", "pnpm.cmd", ...pnpmArgs] : pnpmArgs,
  };
}

export function runTauri(args = process.argv.slice(2)) {
  const invocation = createTauriCommand(args);
  const manifestBefore = readFileSync(tauriManifestPath, "utf8");
  const child = spawn(invocation.command, invocation.args, { stdio: "inherit" });
  let spawnError;
  const signalHandlers = new Map(
    ["SIGINT", "SIGTERM", "SIGHUP"].map((signal) => [
      signal,
      () => {
        if (child.exitCode === null && child.signalCode === null) child.kill(signal);
      },
    ])
  );
  for (const [signal, handler] of signalHandlers) process.on(signal, handler);

  child.once("error", (error) => {
    spawnError = error;
    console.error(`Failed to start Tauri CLI: ${error.message}`);
  });
  child.once("close", (code, signal) => {
    for (const [handledSignal, handler] of signalHandlers) {
      process.removeListener(handledSignal, handler);
    }
    let exitCode = tauriExitCode(code, signal, Boolean(spawnError));
    try {
      restoreTauriManifestFile(manifestBefore);
    } catch (error) {
      console.error(`Failed to restore Cargo.toml after Tauri CLI: ${error.message}`);
      exitCode = 1;
    }
    process.exitCode = exitCode;
  });

  return child;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  runTauri();
}
