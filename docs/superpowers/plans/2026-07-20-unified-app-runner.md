# Unified App Runner Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add one `pnpm app <dev|build> <desktop|android|ios> [options]` command that launches development targets and builds release artifacts through the existing Tauri workspace.

**Architecture:** Keep `run-tauri.mjs` as the low-level cross-platform child-process adapter and remove its configuration mutation. Add `run-app.mjs` as a small, pure argument validator and mapper that delegates to `runTauri`. Expose the dispatcher through the root `package.json` while leaving Tauri responsible for native build behavior, signing, and artifact placement.

**Tech Stack:** Node.js ESM, pnpm workspace scripts, Vitest, Tauri CLI 2.11.0, Cargo, Android Gradle toolchain, Xcode.

## Global Constraints

- Public syntax is exactly `pnpm app <dev|build> <desktop|android|ios> [tauri options]`.
- Desktop builds target the current host OS; the command does not promise desktop cross-compilation.
- iOS commands are rejected on hosts other than macOS.
- Arguments after the platform are forwarded to Tauri unchanged.
- Release builds must not add `--debug`; caller-provided signing and target options remain unchanged.
- The runner does not bump versions, clean caches, upload artifacts, or manage signing secrets.
- Do not inject a `macOSPrivateApi` override; use the checked-in platform-specific Tauri configuration.
- Preserve the user's current `README.md` and `README.zh-CN.md` edits and do not stage unrelated files.
- Use pnpm for every JavaScript command and do not add dependencies or lockfiles.
- Do not use the TypeScript `void` operator.

---

### Task 1: Make the low-level Tauri runner configuration-neutral

**Files:**
- Modify: `packages/scripts/src/run-tauri.mjs`
- Modify: `packages/scripts/src/run-tauri.test.mjs`

**Interfaces:**
- Produces: `createTauriCommand(args: string[], platform?: NodeJS.Platform): { command: string; args: string[] }`.
- Produces: `runTauri(args?: string[]): ChildProcess`, which streams stdio and propagates exit status.
- Removes: `MACOS_PRIVATE_API_VALIDATION_OVERRIDE` and every automatic `--config` insertion.

- [ ] **Step 1: Replace the existing override-focused tests with failing command-construction tests**

Use this complete test content in `packages/scripts/src/run-tauri.test.mjs`:

```js
import { describe, expect, it } from "vitest";

import { createTauriCommand } from "./run-tauri.mjs";

describe("createTauriCommand", () => {
  it("runs the desktop package Tauri script without changing its arguments", () => {
    expect(createTauriCommand(["dev"], "darwin")).toEqual({
      command: "pnpm",
      args: ["--filter", "@markra/desktop", "tauri", "dev"],
    });

    expect(createTauriCommand(["build", "--no-sign"], "linux")).toEqual({
      command: "pnpm",
      args: ["--filter", "@markra/desktop", "tauri", "build", "--no-sign"],
    });
  });

  it("uses the Windows pnpm executable without changing Tauri arguments", () => {
    expect(createTauriCommand(["android", "build", "--apk"], "win32")).toEqual({
      command: "pnpm.cmd",
      args: ["--filter", "@markra/desktop", "tauri", "android", "build", "--apk"],
    });
  });

  it("does not inject a macOS private API configuration override", () => {
    const invocation = createTauriCommand(["build"], "darwin");

    expect(invocation.args).not.toContain("--config");
    expect(invocation.args.join(" ")).not.toContain("macOSPrivateApi");
  });
});
```

- [ ] **Step 2: Run the focused test and verify the old wrapper fails the new contract**

Run:

```bash
pnpm --filter @markra/scripts exec vitest run src/run-tauri.test.mjs --reporter=verbose
```

Expected: FAIL because `createTauriCommand` is not exported and the current runner injects `--config {"app":{"macOSPrivateApi":false}}`.

- [ ] **Step 3: Implement the configuration-neutral process adapter**

Replace `packages/scripts/src/run-tauri.mjs` with:

```js
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";

export function createTauriCommand(args, platform = process.platform) {
  return {
    command: platform === "win32" ? "pnpm.cmd" : "pnpm",
    args: ["--filter", "@markra/desktop", "tauri", ...args],
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
```

- [ ] **Step 4: Run the focused test and verify it passes**

Run:

```bash
pnpm --filter @markra/scripts exec vitest run src/run-tauri.test.mjs --reporter=verbose
```

Expected: 3 tests pass and no configuration override appears in the constructed command.

- [ ] **Step 5: Commit the low-level runner fix without staging unrelated files**

```bash
git add packages/scripts/src/run-tauri.mjs packages/scripts/src/run-tauri.test.mjs package.json
git diff --cached --check
git commit -m "fix: preserve platform Tauri configuration"
```

Include `package.json` in this commit only because its existing `tauri` script already points at `run-tauri.mjs`. Confirm `git diff --cached --name-only` contains exactly these three paths before committing.

### Task 2: Add the unified desktop and mobile command dispatcher

**Files:**
- Create: `packages/scripts/src/run-app.mjs`
- Create: `packages/scripts/src/run-app.test.mjs`
- Modify: `package.json`

**Interfaces:**
- Consumes: `runTauri(args: string[])` from `packages/scripts/src/run-tauri.mjs`.
- Produces: `APP_USAGE: string`.
- Produces: `AppUsageError extends Error` for invalid public arguments.
- Produces: `resolveAppArgs(args: string[], platform?: NodeJS.Platform): string[]`.
- Produces: `runApp(args?: string[]): ChildProcess | undefined`.

- [ ] **Step 1: Write failing tests for every public argument mapping and validation rule**

Create `packages/scripts/src/run-app.test.mjs`:

```js
import { describe, expect, it } from "vitest";

import { APP_USAGE, AppUsageError, resolveAppArgs } from "./run-app.mjs";

describe("resolveAppArgs", () => {
  it.each([
    [["dev", "desktop"], ["dev"]],
    [["build", "desktop", "--no-sign"], ["build", "--no-sign"]],
    [["dev", "android", "--open"], ["android", "dev", "--open"]],
    [
      ["build", "android", "--apk", "--target", "aarch64", "--ci"],
      ["android", "build", "--apk", "--target", "aarch64", "--ci"],
    ],
    [["dev", "ios", "--open"], ["ios", "dev", "--open"]],
    [
      ["build", "ios", "--target", "aarch64-sim", "--no-sign", "--ci"],
      ["ios", "build", "--target", "aarch64-sim", "--no-sign", "--ci"],
    ],
  ])("maps %j to %j", (input, expected) => {
    expect(resolveAppArgs(input, "darwin")).toEqual(expected);
  });

  it.each([
    [[]],
    [["dev"]],
    [["serve", "desktop"]],
    [["build", "web"]],
  ])("rejects invalid arguments %j", (input) => {
    expect(() => resolveAppArgs(input, "darwin")).toThrow(AppUsageError);
  });

  it("rejects iOS commands outside macOS", () => {
    expect(() => resolveAppArgs(["build", "ios"], "linux")).toThrow(
      "iOS commands require macOS",
    );
  });

  it("documents the stable public syntax", () => {
    expect(APP_USAGE).toContain("pnpm app <dev|build> <desktop|android|ios>");
  });
});
```

- [ ] **Step 2: Run the focused test and verify it fails because the dispatcher is absent**

Run:

```bash
pnpm --filter @markra/scripts exec vitest run src/run-app.test.mjs --reporter=verbose
```

Expected: FAIL with a module-not-found error for `src/run-app.mjs`.

- [ ] **Step 3: Implement the minimal dispatcher and usage errors**

Create `packages/scripts/src/run-app.mjs`:

```js
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
```

- [ ] **Step 4: Run the focused dispatcher tests and adjust only contract defects**

Run:

```bash
pnpm --filter @markra/scripts exec vitest run src/run-app.test.mjs --reporter=verbose
```

Expected: all mapping, passthrough, invalid-input, and iOS host tests pass.

- [ ] **Step 5: Expose the new command in the root workspace**

Add this entry beside the existing `tauri` script in `package.json`:

```json
"app": "node packages/scripts/src/run-app.mjs"
```

Do not change dependency declarations or lockfiles.

- [ ] **Step 6: Verify CLI help behavior and Tauri help passthrough**

Run:

```bash
pnpm app
pnpm app build desktop --help
pnpm app build android --help
pnpm app build ios --help
```

Expected: `pnpm app` exits 2 and prints the unified usage. The three valid commands print the corresponding Tauri help and exit 0 without starting a build.

- [ ] **Step 7: Run the whole scripts package gate**

Run:

```bash
pnpm --filter @markra/scripts test
pnpm --filter @markra/scripts build
```

Expected: all `@markra/scripts` Vitest tests pass and TypeScript build exits 0.

- [ ] **Step 8: Commit the dispatcher without staging user documentation changes**

```bash
git add package.json packages/scripts/src/run-app.mjs packages/scripts/src/run-app.test.mjs
git diff --cached --check
git diff --cached --name-only
git commit -m "feat: add unified desktop and mobile runner"
```

Expected staged paths: exactly `package.json`, `packages/scripts/src/run-app.mjs`, and `packages/scripts/src/run-app.test.mjs`.

### Task 3: Verify real release builds and artifacts

**Files:**
- Verify only: `apps/desktop/src-tauri/target/release/bundle/`
- Verify only: `apps/desktop/src-tauri/gen/android/app/build/outputs/`
- Verify only: `apps/desktop/src-tauri/gen/apple/build/`

**Interfaces:**
- Consumes: the `pnpm app` command from Task 2.
- Produces: verified host-native desktop installer, Android ARM64 release APK, and unsigned iOS simulator release bundle or archive.

- [ ] **Step 1: Build the current macOS desktop release without signing**

Run:

```bash
pnpm app build desktop --no-sign
```

Expected: exit 0, a release `QingYu.app`, and a DMG under `apps/desktop/src-tauri/target/release/bundle/`.

- [ ] **Step 2: Verify the desktop bundle and DMG**

Run:

```bash
test -x apps/desktop/src-tauri/target/release/bundle/macos/QingYu.app/Contents/MacOS/markra
find apps/desktop/src-tauri/target/release/bundle -maxdepth 3 -type f -print | sort
hdiutil verify apps/desktop/src-tauri/target/release/bundle/dmg/*.dmg
```

Expected: the executable exists, the bundle listing contains the release installer, and `hdiutil` reports a valid checksum.

- [ ] **Step 3: Build an Android ARM64 release APK**

Run:

```bash
pnpm app build android --apk --target aarch64 --ci
```

Expected: exit 0 and at least one release APK under `apps/desktop/src-tauri/gen/android/app/build/outputs/apk/`.

- [ ] **Step 4: Verify the Android release artifact**

Run:

```bash
find apps/desktop/src-tauri/gen/android/app/build/outputs/apk -type f -name '*.apk' -print | sort
```

Expected: the command prints an ARM64 release APK. If Gradle reports missing release signing credentials, record that external prerequisite exactly; do not add credentials or downgrade the build to debug.

- [ ] **Step 5: Build an unsigned iOS simulator release**

Run:

```bash
pnpm app build ios --target aarch64-sim --no-sign --ci
```

Expected: exit 0 and a fresh release simulator bundle or archive under `apps/desktop/src-tauri/gen/apple/build/`.

- [ ] **Step 6: Verify the iOS output**

Run:

```bash
find apps/desktop/src-tauri/gen/apple/build -maxdepth 5 \( -name '*.app' -o -name '*.xcarchive' -o -name '*.ipa' \) -print | sort
```

Expected: at least one fresh simulator `.app` or `.xcarchive`. Do not claim App Store readiness because this verification deliberately uses `--no-sign`.

- [ ] **Step 7: Run final repository-focused verification and inspect workspace scope**

Run:

```bash
pnpm --filter @markra/scripts test
pnpm --filter @markra/scripts build
git diff --check
git status --short --branch
```

Expected: tests and build exit 0; no generated bundle is tracked; the user's README changes remain present and unstaged unless the user separately requests them.
