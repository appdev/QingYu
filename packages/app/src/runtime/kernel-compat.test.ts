import { readFileSync } from "node:fs";
import { createKernelCompatibilityRuntime } from "./kernel-compat";

describe("unused Kernel compatibility runtime", () => {
  it("maps only safe read models and strips host-only fields", async () => {
    const client = {
      system: {
        runtime: vi.fn(async () => ({
          absoluteRoot: "/private/notes",
          capabilities: {
            documents: true,
            history: true,
            portableSettings: true,
            resources: true,
            s3: true,
            search: true,
            settings: true,
            sync: true,
            webdav: true,
          },
          instanceId: "instance-id",
          profile: "desktop" as const,
          startupState: "ready",
          token: "secret",
        })),
      },
      workspace: {
        get: vi.fn(async () => ({
          absoluteRoot: "/private/notes",
          displayName: "Notes",
          generation: "generation-1",
          id: "workspace-id",
          readiness: "ready",
          revision: "workspace-revision",
        })),
      },
    };
    const runtime = createKernelCompatibilityRuntime(client);

    await expect(runtime.getRuntimeState()).resolves.toEqual({
      capabilities: {
        documents: true,
        history: true,
        portableSettings: true,
        resources: true,
        s3: true,
        search: true,
        settings: true,
        sync: true,
        webdav: true,
      },
      instanceId: "instance-id",
      profile: "desktop",
      startupState: "ready",
    });
    await expect(runtime.getWorkspace()).resolves.toEqual({
      displayName: "Notes",
      generation: "generation-1",
      id: "workspace-id",
      readiness: "ready",
      revision: "workspace-revision",
    });
    expect(runtime).not.toHaveProperty("events");
    expect(runtime).not.toHaveProperty("emit");
    expect(runtime).not.toHaveProperty("files");
    expect(runtime).not.toHaveProperty("patchSettings");
    expect(runtime).not.toHaveProperty("triggerSync");
  });

  it("is exported but not instantiated by any production runtime", () => {
    const appRuntime = readFileSync(`${process.cwd()}/src/runtime/index.ts`, "utf8");
    const desktopRuntime = readFileSync(
      `${process.cwd()}/../../apps/desktop/src/runtime/index.ts`,
      "utf8",
    );
    const webRuntime = readFileSync(
      `${process.cwd()}/../../apps/web/src/runtime/index.ts`,
      "utf8",
    );

    expect(appRuntime).toContain('export * from "./kernel-compat";');
    for (const source of [desktopRuntime, webRuntime]) {
      expect(source).not.toContain("createKernelCompatibilityRuntime");
      expect(source).not.toContain("@markra/kernel-client");
    }
  });
});
