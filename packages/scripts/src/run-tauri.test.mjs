import { describe, expect, it } from "vitest";

import {
  createTauriCommand,
  launchTauri,
  tauriChildEnvironment,
  restoreTauriManifestAfterCliRun,
  tauriExitCode,
} from "./run-tauri.mjs";

describe("launchTauri", () => {
  it("prepares the Kernel before spawning desktop development", () => {
    const events = [];
    const child = { marker: "child" };

    const result = launchTauri(["dev"], {
      environment: { KEEP: "yes" },
      prepareKernel: (environment) => events.push(["prepare", environment]),
      spawnChild: (command, args, options) => {
        events.push(["spawn", command, args, options]);
        return child;
      },
    });

    expect(result).toBe(child);
    expect(events[0]).toEqual(["prepare", { KEEP: "yes" }]);
    expect(events[1][0]).toBe("spawn");
  });

  it("does not spawn Tauri when Kernel preparation fails", () => {
    let spawned = false;

    expect(() =>
      launchTauri(["dev"], {
        environment: {},
        prepareKernel: () => {
          throw new Error("Kernel build failed");
        },
        spawnChild: () => {
          spawned = true;
        },
      }),
    ).toThrow("Kernel build failed");
    expect(spawned).toBe(false);
  });

  it("bypasses Kernel preparation for non-development commands", () => {
    let prepared = false;

    launchTauri(["build"], {
      environment: {},
      prepareKernel: () => {
        prepared = true;
      },
      spawnChild: () => ({ marker: "child" }),
    });

    expect(prepared).toBe(false);
  });
});

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

describe("tauriChildEnvironment", () => {
  it.each([
    [["build", "--target", "x86_64-pc-windows-msvc"], "x86_64-pc-windows-msvc"],
    [["build", "--target=aarch64-apple-darwin"], "aarch64-apple-darwin"],
  ])("forwards the explicit desktop target to sidecar preparation", (args, target) => {
    expect(tauriChildEnvironment(args, { KEEP: "yes" })).toEqual({
      KEEP: "yes",
      MARKRA_DESKTOP_TARGET: target,
    });
  });

  it("does not treat abbreviated mobile targets as desktop triples", () => {
    expect(
      tauriChildEnvironment(["android", "build", "--target", "aarch64"], {
        KEEP: "yes",
      }),
    ).toEqual({ KEEP: "yes" });
  });
});

describe("restoreTauriManifestAfterCliRun", () => {
  it("removes only the macOS private API feature injected into the common dependency", () => {
    const before = `[dependencies]\nserde = "1"\ntauri = { version = "2.11.0", features = ["protocol-asset"] }\n\n[target.'cfg(target_os = "macos")'.dependencies]\ntauri = { version = "2.11.0", features = ["macos-private-api"] }\n`;
    const after = `[dependencies]\nserde = "1"\ntauri = { version = "2.11.0", features = ["macos-private-api", "protocol-asset"] }\nnew-build-setting = "preserved"\n\n[target.'cfg(target_os = "macos")'.dependencies]\ntauri = { version = "2.11.0", features = ["macos-private-api"] }\n`;

    expect(restoreTauriManifestAfterCliRun(before, after)).toBe(
      `[dependencies]\nserde = "1"\ntauri = { version = "2.11.0", features = ["protocol-asset"] }\nnew-build-setting = "preserved"\n\n[target.'cfg(target_os = "macos")'.dependencies]\ntauri = { version = "2.11.0", features = ["macos-private-api"] }\n`
    );
  });

  it("does not overwrite an unexpected concurrent edit to the common Tauri dependency", () => {
    const before = `[dependencies]\ntauri = { version = "2.11.0", features = ["protocol-asset"] }\n`;
    const concurrentlyEdited = `[dependencies]\ntauri = { version = "2.12.0", features = ["macos-private-api", "protocol-asset"] }\n`;

    expect(restoreTauriManifestAfterCliRun(before, concurrentlyEdited)).toBe(concurrentlyEdited);
  });
});

describe("tauriExitCode", () => {
  it("preserves child failures and maps forwarded termination signals", () => {
    expect(tauriExitCode(7, null, false)).toBe(7);
    expect(tauriExitCode(null, "SIGINT", false)).toBe(130);
    expect(tauriExitCode(null, "SIGTERM", false)).toBe(143);
    expect(tauriExitCode(null, "SIGHUP", false)).toBe(129);
    expect(tauriExitCode(0, null, true)).toBe(1);
  });
});
