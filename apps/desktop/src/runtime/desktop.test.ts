import {
  createUnavailableKernelDomainPort,
  createUnavailableNativeShellPort
} from "@markra/app/runtime";
import type { NativeKernelBootstrap } from "../kernel-bootstrap";
import {
  createDesktopKernelRuntimeOwner,
  createDesktopRuntime,
  loadDesktopRuntime
} from "./desktop";

function createReadyBootstrap(): NativeKernelBootstrap {
  return {
    authentication: {
      getCredential: () => "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
      kind: "native-bearer"
    },
    baseUrl: "http://127.0.0.1:49152/",
    generation: "1",
    instanceId: "123e4567-e89b-42d3-a456-426614174000",
    release: vi.fn(() => undefined)
  };
}

describe("desktop runtime composition", () => {
  it("injects domain and native-shell adapters by identity", () => {
    const kernel = createUnavailableKernelDomainPort();
    const nativeShell = createUnavailableNativeShellPort();

    const runtime = createDesktopRuntime({ kernel, nativeShell });

    expect(runtime.kernel).toBe(kernel);
    expect(runtime.nativeShell).toBe(nativeShell);
  });

  it("composes a fixed Kernel workspace without legacy document writers", async () => {
    const kernel = createUnavailableKernelDomainPort();
    const owner = createDesktopKernelRuntimeOwner(kernel);
    const { runtime } = owner;

    expect(runtime.kernel).toBe(kernel);
    expect(runtime.features).toMatchObject({
      dejavuSync: false,
      fileDrop: false,
      imageImport: false,
      openLocalAttachments: false,
      projectSync: true,
      resources: false
    });
    expect(runtime.nativeShell.capabilities).toEqual({
      absolutePathClassification: "unavailable",
      operatingSystemShell: "unavailable",
      pickers: "unavailable",
      standaloneDocuments: "unavailable"
    });
    expect(runtime.mcp.localServiceAvailable).toBe(false);
    expect(runtime.workspace.rootPolicy).toMatchObject({
      canChooseLocalRoot: false,
      kind: "fixed"
    });
    if (runtime.workspace.rootPolicy?.kind !== "fixed") {
      throw new Error("fixed Kernel workspace policy unavailable");
    }
    await expect(runtime.workspace.rootPolicy.resolveRoot())
      .resolves.toBe("kernel-workspace://primary");
    await expect(runtime.files.importLocalFile({} as never))
      .rejects.toThrow("unavailable for a Kernel workspace");
    await expect(runtime.files.saveClipboardImage({} as never))
      .rejects.toThrow("unavailable for a Kernel workspace");
    await expect(runtime.files.trashWorkspaceResources("kernel-workspace://primary", []))
      .rejects.toThrow("unavailable for a Kernel workspace");
    expect(runtime.settings.readPrimaryWorkspaceState).toBeUndefined();
    expect(runtime.settings.writePrimaryWorkspaceState).toBeUndefined();

    await runtime.syncConfig.setEditing({
      active: true,
      revision: "revision-1",
      sessionId: "session-1"
    });
    const pending = await runtime.syncConfig.requestApply({
      exitReason: "window-close",
      revision: "revision-1",
      sessionId: "session-1",
      source: "settings-exit",
      token: "apply-1"
    });
    expect((await runtime.syncConfig.loadEditing()).pendingApply).toEqual(pending.event);
    await runtime.syncConfig.cancelApply({
      revision: "revision-1",
      sessionId: "session-1",
      token: "apply-1"
    });

    owner.release();
    owner.release();
  });

  it("keeps the Kernel port unavailable while the native bootstrap is dormant", async () => {
    const createKernelDomainAdapter = vi.fn();

    const runtime = await loadDesktopRuntime({
      createKernelDomainAdapter,
      readKernelBootstrap: async () => null
    });

    expect(runtime.kernel.availability).toBe("unavailable");
    expect(createKernelDomainAdapter).not.toHaveBeenCalled();
  });

  it("injects the ready native Kernel adapter into the production desktop runtime", async () => {
    const bootstrap = createReadyBootstrap();
    const kernel = createUnavailableKernelDomainPort();
    const release = vi.fn(() => undefined);
    const createKernelDomainAdapter = vi.fn(async () => ({ port: kernel, release }));

    const runtime = await loadDesktopRuntime({
      createKernelDomainAdapter,
      readKernelBootstrap: async () => bootstrap
    });

    expect(createKernelDomainAdapter).toHaveBeenCalledWith(bootstrap);
    expect(runtime.kernel).toBe(kernel);

    window.dispatchEvent(new Event("pagehide"));
  });

  it("fails closed when the native Kernel bootstrap cannot be read", async () => {
    const bootstrapError = new Error("invalid native Kernel bootstrap");
    const createKernelDomainAdapter = vi.fn();

    await expect(loadDesktopRuntime({
      createKernelDomainAdapter,
      readKernelBootstrap: async () => {
        throw bootstrapError;
      }
    })).rejects.toBe(bootstrapError);
    expect(createKernelDomainAdapter).not.toHaveBeenCalled();
  });

  it("fails closed and releases bootstrap ownership when adapter initialization rejects", async () => {
    const bootstrap = createReadyBootstrap();
    const adapterError = new Error("Kernel readiness rejected");

    await expect(loadDesktopRuntime({
      createKernelDomainAdapter: async () => {
        throw adapterError;
      },
      readKernelBootstrap: async () => bootstrap
    })).rejects.toBe(adapterError);
    expect(bootstrap.release).toHaveBeenCalledTimes(1);
  });

  it("releases the active Kernel adapter exactly once when the page lifecycle ends", async () => {
    const bootstrap = createReadyBootstrap();
    const release = vi.fn(() => undefined);

    await loadDesktopRuntime({
      createKernelDomainAdapter: async () => ({
        port: createUnavailableKernelDomainPort(),
        release
      }),
      readKernelBootstrap: async () => bootstrap
    });

    window.dispatchEvent(new Event("pagehide"));
    window.dispatchEvent(new Event("pagehide"));

    expect(release).toHaveBeenCalledTimes(1);
  });
});
