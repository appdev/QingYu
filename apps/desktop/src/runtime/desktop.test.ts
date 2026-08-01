import {
  createUnavailableKernelDomainPort,
  createUnavailableNativeShellPort,
  kernelWorkspaceRoot,
  type KernelDomainPort,
  type KernelRevision,
  type KernelWorkspaceGeneration
} from "@markra/app/runtime";
import { getCurrentWindow } from "@tauri-apps/api/window";
import * as desktopFiles from "./tauri/file/desktop";
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

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: vi.fn(() => ({ label: "editor-7" })),
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
    const kernel = readyKernelPort();
    const selectRoot = vi.fn(async () => "/Workspace/B");
    const commitRoot = vi.fn(async () => undefined);
    const owner = createDesktopKernelRuntimeOwner(kernel, { commitRoot, selectRoot });
    const { runtime } = owner;

    expect(runtime.kernel).toBe(kernel);
    expect(runtime.features).toMatchObject({
      dejavuSync: false,
      fileDrop: false,
      imageImport: true,
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
      .rejects.toThrow("unavailable without a configured app runtime");
    await expect(runtime.files.trashWorkspaceResources("kernel-workspace://primary", []))
      .rejects.toThrow("unavailable for a Kernel workspace");
    expect(runtime.settings.readPrimaryWorkspaceState).toEqual(expect.any(Function));
    expect(runtime.settings.writePrimaryWorkspaceState).toEqual(expect.any(Function));
    const nativeShell = createDesktopRuntime();
    expect(runtime.files.listenOpenedMarkdownPaths)
      .toBe(nativeShell.files.listenOpenedMarkdownPaths);
    expect(runtime.files.takeOpenedMarkdownPaths)
      .toBe(nativeShell.files.takeOpenedMarkdownPaths);
    expect(runtime.files.openLocalImages).toBe(desktopFiles.openNativeLocalImages);

    const secondOwner = createDesktopKernelRuntimeOwner(kernel);
    expect(runtime.syncConfig.cancelApply).toBe(secondOwner.runtime.syncConfig.cancelApply);
    expect(runtime.syncConfig.loadEditing).toBe(secondOwner.runtime.syncConfig.loadEditing);
    expect(runtime.syncConfig.requestApply).toBe(secondOwner.runtime.syncConfig.requestApply);
    expect(runtime.syncConfig.setEditing).toBe(secondOwner.runtime.syncConfig.setEditing);

    secondOwner.release();
    owner.release();
    owner.release();
  });

  it("composes AppConfig with the desktop logical window label", async () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
    vi.mocked(getCurrentWindow).mockReturnValue({ label: "editor-7" } as never);
    const owner = createDesktopKernelRuntimeOwner(readyKernelPort());

    await expect(owner.runtime.appConfig.readWorkspaceState()).resolves.toMatchObject({
      filePath: `${kernelWorkspaceRoot}/notes/editor.md`,
    });

    owner.release();
    Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  });

  it("keeps the native image picker while routing image writes to one Kernel batch", async () => {
    const unavailable = readyKernelPort();
    const generation = "workspace-generation" as KernelWorkspaceGeneration;
    const revision = "document-revision" as KernelRevision;
    const createBatch = vi.fn(async (request: Parameters<KernelDomainPort["resources"]["createBatch"]>[0]) => request.items.map((item, index) => ({
      id: `resource-${index}`,
      kind: "image" as const,
      mediaType: item.mediaType,
      modifiedAt: "2026-08-01T00:00:00Z",
      name: item.name,
      parent: "notes/assets" as never,
      previewable: true,
      relativePath: `notes/assets/${item.name}` as never,
      revision: `resource-revision-${index}` as KernelRevision,
      sizeBytes: item.body.size,
      workspaceGeneration: generation,
    })));
    const kernel = {
      ...unavailable,
      availability: "available",
      documents: {
        ...unavailable.documents,
        list: vi.fn(async () => ({
          items: [{
            kind: "file" as const,
            locator: "document-locator" as never,
            modifiedAt: "2026-08-01T00:00:00Z",
            name: "note.md",
            parent: "notes" as never,
            relativePath: "notes/note.md" as never,
            revision,
            sizeBytes: 4,
            workspaceGeneration: generation,
          }],
          nextCursor: null,
          workspaceGeneration: generation,
        })),
      },
      resources: {
        ...unavailable.resources,
        createBatch,
        list: vi.fn(async () => ({ items: [], workspaceGeneration: generation })),
      },
      workspace: {
        read: vi.fn(async () => ({
          displayName: "Notes",
          generation,
          id: "workspace-id",
          readiness: "ready" as const,
          revision,
        })),
      },
    } as KernelDomainPort;
    const owner = createDesktopKernelRuntimeOwner(kernel);
    const images = [
      new File([new Uint8Array([1])], "fixture.avif", { type: "image/avif" }),
      new File([new Uint8Array([2])], "fixture.bmp", { type: "image/bmp" }),
      new File([new Uint8Array([3])], "fixture.gif", { type: "image/gif" }),
      new File([new Uint8Array([4])], "fixture.jpg", { type: "image/jpeg" }),
      new File([new Uint8Array([5])], "fixture.png", { type: "image/png" }),
      new File([new Uint8Array([6])], "fixture.svg", { type: "image/svg+xml" }),
      new File([new Uint8Array([7])], "fixture.webp", { type: "image/webp" }),
    ];

    expect(owner.runtime.features.imageImport).toBe(true);
    expect(owner.runtime.files.openLocalImages).toBe(desktopFiles.openNativeLocalImages);
    await expect(owner.runtime.files.importLocalFile({} as never))
      .rejects.toThrow("unavailable for a Kernel workspace");
    await expect(owner.runtime.files.saveClipboardImage({} as never))
      .rejects.toThrow("unavailable without a configured app runtime");
    await expect(owner.runtime.files.saveClipboardImages(images.map((image) => ({
      documentPath: `${kernelWorkspaceRoot}/notes/note.md`,
      fileName: image.name,
      folder: "assets",
      image,
    })))).resolves.toEqual(images.map((image) => ({
      alt: "fixture",
      src: `assets/${image.name}`,
    })));
    expect(createBatch).toHaveBeenCalledOnce();
    expect(createBatch).toHaveBeenCalledWith(expect.objectContaining({
      items: images.map((image) => ({
        body: image,
        kind: "image",
        mediaType: image.type,
        name: image.name,
      })),
    }));
    expect(owner.runtime.nativeShell.capabilities.pickers).toBe("unavailable");

    owner.release();
  });

  it("uses the raw host directory selector and dedicated Kernel switch by default", async () => {
    const owner = createDesktopKernelRuntimeOwner(readyKernelPort());
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

function readyKernelPort(): KernelDomainPort {
  const unavailable = createUnavailableKernelDomainPort();
  const bootstrap = {
    appConfigVersion: 1 as const,
    localState: {
      fileTreeSort: { direction: "ascending" as const, key: "name" as const },
      pandocPath: null,
      recentMarkdownFiles: [],
      revision: "local-1" as KernelRevision,
      uiLayout: {
        openWindows: [],
        schemaVersion: 1 as const,
        windowStates: {
          "editor-7": {
            activeDraftId: null,
            draftTabs: [],
            filePath: "notes/editor.md" as never,
            fileTreeAssetsVisible: true,
            fileTreeOpen: false,
            folderName: "notes",
            folderPath: "notes" as never,
            openFilePaths: ["notes/editor.md" as never],
            sideBySideGroup: null,
          },
        },
      },
    },
    settings: { revision: "settings-1" as KernelRevision, values: [] },
    workspace: {
      generation: "generation-1" as KernelWorkspaceGeneration,
      id: "workspace-1",
    },
  };
  return {
    ...unavailable,
    appConfig: {
      bootstrap,
      patchState: vi.fn(async () => bootstrap),
      read: vi.fn(async () => bootstrap),
    },
    availability: "available",
  } as KernelDomainPort;
}
