import {
  createUnavailableKernelDomainPort,
  createUnavailableNativeShellPort
} from "@markra/app/runtime";
import type { NativeKernelBootstrap } from "../kernel-bootstrap";
import { switchDesktopKernelWorkspace } from "../desktop-kernel-startup";
import { selectDesktopWorkspaceDirectory } from "../desktop-workspace-selector";
import {
  createDesktopKernelRuntimeOwner,
  createDesktopRuntime,
  loadDesktopRuntime
} from "./desktop";

vi.mock("../desktop-workspace-selector", () => ({
  selectDesktopWorkspaceDirectory: vi.fn(async () => "/Workspace/Raw"),
}));

vi.mock("../desktop-kernel-startup", () => ({
  switchDesktopKernelWorkspace: vi.fn(async () => undefined),
}));

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

  it("composes a host-selectable Kernel workspace without legacy document writers", async () => {
    const kernel = createUnavailableKernelDomainPort();
    const selectRoot = vi.fn(async () => "/Workspace/B");
    const commitRoot = vi.fn(async () => undefined);
    const owner = createDesktopKernelRuntimeOwner(kernel, { commitRoot, selectRoot });
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
    expect(runtime.mcp.localServiceAvailable).toBe(true);
    expect(runtime.workspace.rootPolicy).toMatchObject({
      canChooseLocalRoot: true,
      kind: "host-selectable"
    });
    if (runtime.workspace.rootPolicy?.kind !== "host-selectable") {
      throw new Error("host-selectable Kernel workspace policy unavailable");
    }
    await expect(runtime.workspace.rootPolicy.resolveRoot())
      .resolves.toBe("kernel-workspace://primary");
    await expect(runtime.workspace.rootPolicy.selectRoot()).resolves.toBe("/Workspace/B");
    await expect(runtime.workspace.rootPolicy.commitRoot("/Workspace/B"))
      .resolves.toBe("kernel-workspace://primary");
    expect(commitRoot).toHaveBeenCalledWith("/Workspace/B");
    await expect(runtime.files.importLocalFile({} as never))
      .rejects.toThrow("unavailable for a Kernel workspace");
    await expect(runtime.files.saveClipboardImage({} as never))
      .rejects.toThrow("unavailable for a Kernel workspace");
    await expect(runtime.files.trashWorkspaceResources("kernel-workspace://primary", []))
      .rejects.toThrow("unavailable for a Kernel workspace");
    expect(runtime.settings.readPrimaryWorkspaceState).toBeUndefined();
    expect(runtime.settings.writePrimaryWorkspaceState).toBeUndefined();
    const nativeShell = createDesktopRuntime();
    expect(runtime.files.listenOpenedMarkdownPaths)
      .toBe(nativeShell.files.listenOpenedMarkdownPaths);
    expect(runtime.files.takeOpenedMarkdownPaths)
      .toBe(nativeShell.files.takeOpenedMarkdownPaths);

    const secondOwner = createDesktopKernelRuntimeOwner(kernel);
    expect(runtime.syncConfig.cancelApply).toBe(secondOwner.runtime.syncConfig.cancelApply);
    expect(runtime.syncConfig.loadEditing).toBe(secondOwner.runtime.syncConfig.loadEditing);
    expect(runtime.syncConfig.requestApply).toBe(secondOwner.runtime.syncConfig.requestApply);
    expect(runtime.syncConfig.setEditing).toBe(secondOwner.runtime.syncConfig.setEditing);

    secondOwner.release();
    owner.release();
    owner.release();
  });

  it("uses the raw host directory selector and dedicated Kernel switch by default", async () => {
    const owner = createDesktopKernelRuntimeOwner(createUnavailableKernelDomainPort());
    const policy = owner.runtime.workspace.rootPolicy;
    if (policy?.kind !== "host-selectable") {
      throw new Error("host-selectable Kernel workspace policy unavailable");
    }

    await expect(policy.selectRoot()).resolves.toBe("/Workspace/Raw");
    await expect(policy.commitRoot("/Workspace/Raw"))
      .resolves.toBe("kernel-workspace://primary");

    expect(selectDesktopWorkspaceDirectory).toHaveBeenCalledTimes(1);
    expect(switchDesktopKernelWorkspace).toHaveBeenCalledTimes(1);
    expect(switchDesktopKernelWorkspace).toHaveBeenCalledWith("/Workspace/Raw");
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
