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
      dejavuSync: true,
      fileDrop: false,
      imageImport: true,
      localFileImport: false,
      markdownBundle: true,
      openLocalAttachments: false,
      projectSync: true,
      resources: false,
      standaloneDocuments: false,
      templateMutation: false
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
    expect(runtime.files.saveMarkdownBundleFile)
      .not.toBe(desktopFiles.saveNativeMarkdownBundleFile);

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
    const kernel = readyKernelPort();
    const committed = {
      ...kernel.appConfig.bootstrap,
      localState: {
        ...kernel.appConfig.bootstrap.localState,
        revision: "local-2" as KernelRevision,
        uiLayout: {
          ...kernel.appConfig.bootstrap.localState.uiLayout,
          windowStates: {
            "editor-7": {
              ...kernel.appConfig.bootstrap.localState.uiLayout.windowStates["editor-7"],
              fileTreeOpen: true,
            },
          },
        },
      },
    };
    vi.mocked(kernel.appConfig.patchState).mockResolvedValueOnce(committed);
    const owner = createDesktopKernelRuntimeOwner(kernel);

    await expect(owner.runtime.appConfig.readWorkspaceState()).resolves.toMatchObject({
      filePath: `${kernelWorkspaceRoot}/notes/editor.md`,
      fileTreeOpen: false,
    });
    await expect(owner.runtime.settings.loadStore("local-state.json", {
      autoSave: false,
      defaults: {},
    })).rejects.toThrow("Renderer-local stores are unavailable");
    await owner.runtime.appConfig.patchState([{
      patch: { fileTreeOpen: true },
      type: "patch-ui-layout",
      windowLabel: "editor-7",
    }]);
    await expect(owner.runtime.appConfig.readWorkspaceState()).resolves.toMatchObject({
      filePath: `${kernelWorkspaceRoot}/notes/editor.md`,
      fileTreeOpen: true,
    });
    expect(kernel.appConfig.patchState).toHaveBeenCalledWith({
      operations: [{
        patch: { fileTreeOpen: true },
        type: "patch-ui-layout",
        windowLabel: "editor-7",
      }],
      workspaceGeneration: "generation-1",
    });

    owner.release();
    Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  });

  it("freezes Kernel Markdown resources before opening the native export picker", async () => {
    const unavailable = readyKernelPort();
    const generation = "workspace-generation" as KernelWorkspaceGeneration;
    const revision = "sha256:resource" as KernelRevision;
    const resource = {
      id: "resource-id",
      kind: "image" as const,
      mediaType: "image/png",
      modifiedAt: "2026-08-01T00:00:00Z",
      name: "chart.png",
      parent: "notes/assets" as never,
      previewable: true,
      relativePath: "notes/assets/chart.png" as never,
      revision,
      sizeBytes: 5,
      workspaceGeneration: generation,
    };
    const workspace = {
      displayName: "Notes",
      generation,
      id: "workspace-id",
      readiness: "ready" as const,
      revision: "workspace-revision" as KernelRevision,
    };
    const document = {
      kind: "file" as const,
      locator: "document-locator" as never,
      modifiedAt: "2026-08-01T00:00:00Z",
      name: "draft.md",
      parent: "notes" as never,
      relativePath: "notes/draft.md" as never,
      revision: "document-revision" as KernelRevision,
      sizeBytes: 7,
      workspaceGeneration: generation,
    };
    const kernel = {
      ...unavailable,
      availability: "available",
      documents: {
        ...unavailable.documents,
        list: vi.fn(async () => ({
          items: [document],
          nextCursor: null,
          workspaceGeneration: generation,
        })),
      },
      resources: {
        ...unavailable.resources,
        list: vi.fn(async () => ({
          items: [{ entryType: "resource" as const, resource }],
          workspaceGeneration: generation,
        })),
        open: vi.fn(async () => ({
          body: new Blob(["image"], { type: "image/png" }),
          mediaType: "image/png",
          revision,
        })),
      },
      workspace: {
        read: vi.fn(async () => workspace),
      },
    } as KernelDomainPort;
    const saveMarkdownBundleSnapshot = vi.fn(async () => ({
      name: "draft.md",
      path: "/exports/draft/draft.md",
    }));
    const owner = createDesktopKernelRuntimeOwner(kernel, { saveMarkdownBundleSnapshot });
    const markdown = "# Draft\n\n![Chart](assets/chart.png)";
    const href = "assets/chart.png";
    const from = markdown.indexOf(href);

    await expect(owner.runtime.files.saveMarkdownBundleFile?.({
      documentPath: `${kernelWorkspaceRoot}/notes/draft.md`,
      folder: "assets",
      markdown,
      references: [{ from, href, rawHref: href, to: from + href.length }],
      rootPath: kernelWorkspaceRoot,
      suggestedName: "draft.md",
    })).resolves.toEqual({
      name: "draft.md",
      path: "/exports/draft/draft.md",
    });

    expect(kernel.resources.open).toHaveBeenCalledWith({
      id: resource.id,
      kind: resource.kind,
      workspaceGeneration: generation,
    });
    expect(kernel.resources.list).toHaveBeenCalledTimes(2);
    expect(kernel.workspace.read).toHaveBeenCalledTimes(2);
    expect(saveMarkdownBundleSnapshot).toHaveBeenCalledWith({
      folder: "assets",
      markdown,
      references: [{
        from,
        href,
        rawHref: href,
        resourcePath: "notes/assets/chart.png",
        to: from + href.length,
      }],
      resources: [{
        bodyBase64: "aW1hZ2U=",
        name: "chart.png",
        path: "notes/assets/chart.png",
      }],
      suggestedName: "draft.md",
    });

    vi.mocked(kernel.resources.open).mockResolvedValueOnce({
      body: new Blob(["image"], { type: "image/png" }),
      mediaType: "image/png",
      revision: "sha256:replacement" as KernelRevision,
    });
    await expect(owner.runtime.files.saveMarkdownBundleFile?.({
      documentPath: `${kernelWorkspaceRoot}/notes/draft.md`,
      folder: "assets",
      markdown,
      references: [{ from, href, rawHref: href, to: from + href.length }],
      rootPath: kernelWorkspaceRoot,
      suggestedName: "draft.md",
    })).rejects.toThrow('Markdown resource "notes/assets/chart.png" changed during export.');
    expect(saveMarkdownBundleSnapshot).toHaveBeenCalledTimes(1);
    owner.release();
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
