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

  it("runs the Windows pnpm command through cmd.exe without changing Tauri arguments", () => {
    expect(createTauriCommand(["android", "build", "--apk"], "win32")).toEqual({
      command: "cmd.exe",
      args: ["/d", "/s", "/c", "pnpm.cmd", "--filter", "@markra/desktop", "tauri", "android", "build", "--apk"],
    });
  });

  it("does not inject a macOS private API configuration override", () => {
    const invocation = createTauriCommand(["build"], "darwin");

    expect(invocation.args).not.toContain("--config");
    expect(invocation.args.join(" ")).not.toContain("macOSPrivateApi");
  });
});
