import { fileURLToPath } from "node:url";

import { runTauri } from "./run-tauri.mjs";

const ACTIONS = new Set(["dev", "build"]);
const TARGETS = new Set(["desktop", "android", "ios"]);

export const APP_USAGE = `Usage:
  pnpm app <dev|build> <desktop|android|ios> [tauri options]

Examples:
  pnpm app dev desktop
  pnpm app dev android --open
  pnpm app build desktop --no-sign
  pnpm app build android --apk --target aarch64 --ci
  pnpm app build ios --target aarch64-sim --no-sign --ci`;

export class AppUsageError extends Error {}

export function resolveAppArgs(args, platform = process.platform) {
  const [action, target, ...options] = args;

  if (!ACTIONS.has(action) || !TARGETS.has(target)) {
    throw new AppUsageError(APP_USAGE);
  }
  if (target === "ios" && platform !== "darwin") {
    throw new AppUsageError(`iOS commands require macOS.\n\n${APP_USAGE}`);
  }

  return target === "desktop"
    ? [action, ...options]
    : [target, action, ...options];
}

export function runApp(args = process.argv.slice(2)) {
  try {
    return runTauri(resolveAppArgs(args));
  } catch (error) {
    if (error instanceof AppUsageError) {
      console.error(error.message);
      process.exitCode = 2;
      return undefined;
    }

    throw error;
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  runApp();
}
