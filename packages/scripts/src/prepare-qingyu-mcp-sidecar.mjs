import { execFileSync } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  copyKernelSidecarAtomically,
  resolveKernelTargetTriple,
  validatePreparedKernelSidecar,
} from "./prepare-qingyu-kernel-sidecar.mjs";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(scriptDirectory, "../../..");

export function resolveMcpTargetTriple(options) {
  return resolveKernelTargetTriple(options);
}

export function mcpCargoBuildArgs(manifestPath, targetTriple) {
  return [
    "build",
    "--manifest-path",
    manifestPath,
    "--bin",
    "qingyu-mcp",
    "--features",
    "desktop-sidecar",
    "--locked",
    "--release",
    "--target",
    targetTriple,
  ];
}

export function mcpSidecarPaths(root, targetTriple) {
  const suffix = targetTriple.includes("windows") ? ".exe" : "";
  const tauriRoot = join(root, "apps/desktop/src-tauri");
  return {
    source: join(
      tauriRoot,
      "target",
      targetTriple,
      "release",
      `qingyu-mcp${suffix}`,
    ),
    destination: join(
      tauriRoot,
      "binaries",
      `qingyu-mcp-${targetTriple}${suffix}`,
    ),
  };
}

export function validatePreparedMcpSidecar(
  path,
  targetTriple,
  hostPlatform = process.platform,
) {
  return validatePreparedKernelSidecar(path, targetTriple, hostPlatform);
}

export function prepareMcpSidecar({
  environment = process.env,
  root = repositoryRoot,
  run = execFileSync,
} = {}) {
  const explicitTarget =
    environment.MARKRA_DESKTOP_TARGET?.trim()
    || environment.TAURI_ENV_TARGET_TRIPLE?.trim()
    || environment.CARGO_BUILD_TARGET?.trim();
  const rustcVersion = explicitTarget
    ? ""
    : run("rustc", ["-vV"], { cwd: root, encoding: "utf8" });
  const targetTriple = resolveMcpTargetTriple({ environment, rustcVersion });
  const manifestPath = join(root, "apps/desktop/src-tauri/Cargo.toml");

  run("cargo", mcpCargoBuildArgs(manifestPath, targetTriple), {
    cwd: root,
    stdio: "inherit",
  });

  const paths = mcpSidecarPaths(root, targetTriple);
  validatePreparedMcpSidecar(paths.source, targetTriple);
  mkdirSync(dirname(paths.destination), { recursive: true });
  const validation = copyKernelSidecarAtomically(
    paths.source,
    paths.destination,
    targetTriple,
    process.platform,
    { allowCargoHardLinkSource: true },
  );
  return { ...paths, ...validation };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const prepared = prepareMcpSidecar();
  process.stdout.write(`Prepared ${prepared.destination} (${prepared.byteLength} bytes)\n`);
}
