import { execFileSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(scriptDirectory, "../../..");

export function shouldPrepareDesktopDevKernel(args) {
  return args[0] === "dev";
}

export function kernelDevCargoInvocation(root, environment) {
  const args = [
    "build",
    "--manifest-path",
    join(root, "apps/kernel/Cargo.toml"),
    "--bin",
    "qingyu-kernel",
    "--locked",
    "--target-dir",
    join(root, "apps/desktop/src-tauri/target"),
  ];
  const target = environment.MARKRA_DESKTOP_TARGET?.trim();
  if (target) args.push("--target", target);

  return { command: "cargo", args, cwd: root };
}

export function prepareQingyuKernelDev({
  environment = process.env,
  root = repositoryRoot,
  run = execFileSync,
} = {}) {
  const invocation = kernelDevCargoInvocation(root, environment);
  run(invocation.command, invocation.args, {
    cwd: invocation.cwd,
    env: environment,
    stdio: "inherit",
  });
}
