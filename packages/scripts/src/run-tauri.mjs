import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";

export function createTauriCommand(args, platform = process.platform) {
  const pnpmArgs = ["--filter", "@markra/desktop", "tauri", ...args];

  return {
    command: platform === "win32" ? "cmd.exe" : "pnpm",
    args: platform === "win32" ? ["/d", "/s", "/c", "pnpm.cmd", ...pnpmArgs] : pnpmArgs,
  };
}

export function runTauri(args = process.argv.slice(2)) {
  const invocation = createTauriCommand(args);
  const child = spawn(invocation.command, invocation.args, { stdio: "inherit" });

  child.once("error", (error) => {
    console.error(`Failed to start Tauri CLI: ${error.message}`);
    process.exitCode = 1;
  });
  child.once("exit", (code, signal) => {
    process.exitCode = code ?? (signal === "SIGINT" ? 130 : 1);
  });

  return child;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  runTauri();
}
