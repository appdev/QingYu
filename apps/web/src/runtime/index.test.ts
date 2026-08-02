import { FakeIndexedDbFactory } from "../test/web-runtime-fakes";
import {
  kernelWorkspaceRoot,
  type KernelDomainPort,
  type KernelRevision,
  type KernelWorkspaceGeneration,
} from "@markra/app/runtime";
import { createServerWebRuntime, createWebRuntime } from "./index";

describe("web runtime", () => {
  it("uses the authenticated server Kernel port without changing browser services", async () => {
    const showDirectoryPicker = vi.fn(() => {
      throw new Error("directory picker must be unreachable");
    });
    const indexedDbOpen = vi.fn(() => {
      throw new Error("IndexedDB owner must be unreachable");
    });
    const bootstrap = appConfigSnapshot();
    const committed = appConfigSnapshot({ fileTreeOpen: true, revision: "local-2" });
    const kernel = {
      appConfig: {
        bootstrap,
        patchState: vi.fn(async () => committed),
        read: vi.fn(async () => bootstrap),
      },
      documents: {
        list: vi.fn(async () => ({ items: [], nextCursor: null, workspaceGeneration: "generation-1" })),
      },
      invalidations: {
        available: false,
        subscribe: () => () => undefined,
      },
      workspace: {
        read: vi.fn(async () => ({ displayName: "Notes", generation: "generation-1" })),
      },
    } as unknown as KernelDomainPort;

    const owner = createServerWebRuntime(kernel, {
      eventTarget: new EventTarget(),
      indexedDB: { open: indexedDbOpen } as unknown as IDBFactory,
      showDirectoryPicker,
    });
    const { runtime } = owner;

    expect(runtime.kernel).toBe(kernel);
    expect(runtime.events.isAvailable()).toBe(true);
    expect(runtime.features.nativeWindowChrome).toBe(false);
    expect(runtime.features.fileDrop).toBe(false);
    expect(runtime.features.projectSync).toBe(true);
    expect(runtime.workspace.rootPolicy).toMatchObject({
      canChooseLocalRoot: false,
      kind: "fixed"
    });
    if (runtime.workspace.rootPolicy?.kind !== "fixed") {
      throw new Error("Server workspace root policy is not fixed.");
    }
    await expect(runtime.workspace.rootPolicy.resolveRoot()).resolves.toBe("kernel-workspace://primary");
    await expect(runtime.files.listMarkdownFilesForPath("kernel-workspace://primary"))
      .resolves.toEqual([]);
    await expect(runtime.appConfig.readWorkspaceState()).resolves.toMatchObject({
      filePath: `${kernelWorkspaceRoot}/notes/main.md`,
      fileTreeOpen: false,
    });
    await expect(runtime.settings.loadStore("local-state.json", {
      autoSave: false,
      defaults: {},
    })).rejects.toThrow("unavailable for a Kernel-backed runtime");
    await runtime.appConfig.patchState([{
      patch: { fileTreeOpen: true },
      type: "patch-ui-layout",
      windowLabel: "main",
    }]);
    await expect(runtime.appConfig.readWorkspaceState()).resolves.toMatchObject({
      filePath: `${kernelWorkspaceRoot}/notes/main.md`,
      fileTreeOpen: true,
    });
    expect(kernel.appConfig.patchState).toHaveBeenCalledWith({
      operations: [{
        patch: { fileTreeOpen: true },
        type: "patch-ui-layout",
        windowLabel: "main",
      }],
      workspaceGeneration: "generation-1",
    });
    expect(showDirectoryPicker).not.toHaveBeenCalled();
    expect(indexedDbOpen).not.toHaveBeenCalled();
    owner.release();
    owner.release();
  });

  it("creates a browser runtime with IndexedDB settings", async () => {
    const runtime = createWebRuntime({
      eventTarget: new EventTarget(),
      indexedDB: new FakeIndexedDbFactory().indexedDB
    });

    expect(runtime).toHaveProperty("files");
    expect(runtime).toHaveProperty("settings");
    expect(runtime).toHaveProperty("webResource.downloadImage", expect.any(Function));
    expect(runtime).toHaveProperty("syncConfig");
    expect(runtime.themes.capabilities).toEqual({
      canDelete: false,
      canImport: false,
      canOpenDirectory: false
    });
    await expect(runtime.themes.list()).resolves.toEqual({ invalidFiles: [], themes: [] });

    const store = await runtime.settings.loadStore("settings.json", {
      autoSave: false,
      defaults: { theme: "system" }
    });

    await store.set("theme", "solarized-dark");
    await store.save();

    const reloadedStore = await runtime.settings.loadStore("settings.json", {
      autoSave: false,
      defaults: {}
    });

    await expect(reloadedStore.get("theme")).resolves.toBe("solarized-dark");
    expect(runtime.events.isAvailable()).toBe(true);
    expect(runtime.features).toEqual({
      applicationMenu: false,
      applicationShortcuts: true,
      export: true,
      fileDrop: true,
      imageImport: false,
      localFileImport: false,
      nativeWindowChrome: false,
      openLocalAttachments: true,
      pandoc: false,
      projectSync: false,
      resources: false,
      settingsWindow: false,
      standaloneDocuments: true,
      systemFonts: false,
      templateMutation: true,
      updater: false
    });
    expect(Object.keys(runtime.features)).not.toContain(["s3", "ImageUpload"].join(""));
    expect(Object.keys(runtime.files)).not.toContain(["upload", "Pic", "GoImage"].join(""));
    expect(Object.keys(runtime.files)).not.toContain(["upload", "S3Image"].join(""));
    expect(Object.keys(runtime.files)).not.toContain(["upload", "WebDavImage"].join(""));
    expect(Object.keys(runtime.files)).not.toContain(["syncMarkdown", "Folder"].join(""));
    expect(runtime.platform.resolveDesktopPlatform()).toBe("windows");
    await expect(runtime.updater.checkAppUpdate()).resolves.toBeNull();
  });

  it("exposes application sync as an unsupported native-only feature", async () => {
    const runtime = createWebRuntime();

    expect(runtime.features.projectSync).toBe(false);
    await expect(runtime.syncConfig.enable({ expectedRevision: null }))
      .rejects.toThrow("enableSyncConfig is unavailable without a configured app runtime.");
    await expect(runtime.syncConfig.load())
      .rejects.toThrow("loadSyncConfig is unavailable without a configured app runtime.");
    await expect(runtime.syncConfig.loadStatus())
      .rejects.toThrow("loadSyncStatus is unavailable without a configured app runtime.");
    await expect(runtime.syncConfig.sync({
      notebookName: "notes",
      notesRoot: "/notes",
      revision: "rev-1",
      trigger: "manual"
    })).rejects.toThrow("syncApplication is unavailable without a configured app runtime.");
    await expect(runtime.syncConfig.testConnection({ revision: "rev-1" }))
      .rejects.toThrow("testSyncConnection is unavailable without a configured app runtime.");
    await expect(runtime.syncConfig.patch({
      expectedRevision: "rev-1",
      patch: { field: "enabled", value: true }
    })).rejects.toThrow("patchSyncConfig is unavailable without a configured app runtime.");
    await expect(runtime.syncConfig.reset({ confirmed: true, expectedRevision: null }))
      .rejects.toThrow("resetSyncConfig is unavailable without a configured app runtime.");
  });
});

function appConfigSnapshot({
  fileTreeOpen = false,
  revision = "local-1",
}: {
  fileTreeOpen?: boolean;
  revision?: string;
} = {}) {
  return {
    appConfigVersion: 1 as const,
    localState: {
      fileTreeSort: { direction: "ascending" as const, key: "name" as const },
      pandocPath: null,
      recentMarkdownFiles: [],
      revision: revision as KernelRevision,
      uiLayout: {
        openWindows: [],
        schemaVersion: 1 as const,
        windowStates: {
          main: {
            activeDraftId: null,
            draftTabs: [],
            filePath: "notes/main.md" as never,
            fileTreeAssetsVisible: true,
            fileTreeOpen,
            folderName: "notes",
            folderPath: "notes" as never,
            openFilePaths: ["notes/main.md" as never],
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
}
