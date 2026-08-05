import { describe, expect, it } from "vitest";

import {
  kernelDevCargoInvocation,
  shouldPrepareDesktopDevKernel,
} from "./prepare-qingyu-kernel-dev.mjs";

describe("shouldPrepareDesktopDevKernel", () => {
  it("prepares only desktop development launches", () => {
    expect(shouldPrepareDesktopDevKernel(["dev"])).toBe(true);
    expect(shouldPrepareDesktopDevKernel(["dev", "--target", "aarch64-apple-darwin"])).toBe(true);
    expect(shouldPrepareDesktopDevKernel(["build"])).toBe(false);
    expect(shouldPrepareDesktopDevKernel(["android", "dev"])).toBe(false);
    expect(shouldPrepareDesktopDevKernel(["ios", "dev"])).toBe(false);
  });
});

describe("kernelDevCargoInvocation", () => {
  it("builds the debug Kernel into the desktop Tauri target directory", () => {
    expect(kernelDevCargoInvocation("/repo", {})).toEqual({
      command: "cargo",
      args: [
        "build",
        "--manifest-path",
        "/repo/apps/kernel/Cargo.toml",
        "--bin",
        "qingyu-kernel",
        "--locked",
        "--target-dir",
        "/repo/apps/desktop/src-tauri/target",
      ],
      cwd: "/repo",
    });
  });

  it("uses the explicit desktop target propagated by the Tauri wrapper", () => {
    expect(
      kernelDevCargoInvocation("/repo", {
        MARKRA_DESKTOP_TARGET: "x86_64-pc-windows-msvc",
      }).args,
    ).toEqual([
      "build",
      "--manifest-path",
      "/repo/apps/kernel/Cargo.toml",
      "--bin",
      "qingyu-kernel",
      "--locked",
      "--target-dir",
      "/repo/apps/desktop/src-tauri/target",
      "--target",
      "x86_64-pc-windows-msvc",
    ]);
  });
});
