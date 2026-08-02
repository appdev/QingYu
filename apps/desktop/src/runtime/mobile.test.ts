import { invoke } from "@tauri-apps/api/core";
import { confirm } from "@tauri-apps/plugin-dialog";
import {
  configureAppRuntime,
  createUnavailableKernelDomainPort,
  kernelWorkspaceRoot,
  resetAppRuntimeForTests,
  type KernelAppConfigSnapshot,
  type KernelDomainPort,
  type KernelRevision,
  type KernelWorkspaceGeneration,
  type KernelWorkspaceRelativePath,
} from "@markra/app/runtime";
import {
  flushAppConfigStatePersistence,
  retireAppConfigStatePersistence,
  saveStoredWorkspaceState,
} from "@markra/app/settings";

import {
  createMobileKernelRuntimeOwner,
  mobileRuntime,
} from "./mobile";
import * as fileConfirm from "./tauri/file/confirm";
import * as mobileFiles from "./tauri/file/mobile";
import * as mobileBack from "./tauri/mobile-back";
import * as themes from "./tauri/themes/shared";

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: vi.fn(),
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  emit: vi.fn(),
  listen: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  confirm: vi.fn(),
  open: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-fs", () => ({ readFile: vi.fn() }));
vi.mock("@tauri-apps/plugin-log", () => ({
  error: vi.fn(),
  info: vi.fn(),
  warn: vi.fn(),
}));
const mockedConfirm = vi.mocked(confirm);
const mockedInvoke = vi.mocked(invoke);

describe("mobile Kernel runtime boundary", () => {
  beforeEach(() => {
    mockedConfirm.mockReset();
    mockedInvoke.mockReset();
  });

  afterEach(() => {
    retireAppConfigStatePersistence();
    resetAppRuntimeForTests();
  });

  it("keeps the pre-Kernel shell unavailable and free of renderer-local state", async () => {
    expect(mobileRuntime.kernel.availability).toBe("unavailable");
    expect(mobileRuntime.mcp.policyAvailable).toBe(false);
    expect(mobileRuntime.mcp.localServiceAvailable).toBe(false);
    await expect(mobileRuntime.settings.loadStore()).rejects.toThrow(
      "Renderer-local stores are unavailable",
    );
  });

  it("composes document, settings, and sync over one ready Kernel port", async () => {
    const kernel = readyKernelPort();
    const listNotebooks = vi.fn(async () => [{
      available: true,
      disabledReason: null,
      displayName: "Mobile shared notes",
      name: "Mobile shared notes",
      provider: "s3" as const,
      repositoryId: "323df833-764a-44b3-a534-492640c258f2",
    }]);
    const readRun = vi.fn(async () => ({
      acceptedAt: "2026-08-02T10:00:00Z",
      completionState: "succeeded" as const,
      configRevision: "sync-mobile-1" as KernelRevision,
      error: null,
      finishedAt: "2026-08-02T10:00:01Z",
      provider: "s3" as const,
      runId: "00000000-0000-4000-8000-0000000000d2",
      summary: null,
    }));
    Object.assign(kernel.sync, { listNotebooks, readRun });
    const committed = {
      ...kernel.appConfig.bootstrap,
      localState: {
        ...kernel.appConfig.bootstrap.localState,
        revision: "local-2" as KernelRevision,
        uiLayout: {
          ...kernel.appConfig.bootstrap.localState.uiLayout,
          windowStates: {
            main: {
              ...kernel.appConfig.bootstrap.localState.uiLayout.windowStates.main,
              fileTreeOpen: true,
            },
          },
        },
      },
    };
    vi.mocked(kernel.appConfig.patchState).mockResolvedValueOnce(committed);
    const owner = createMobileKernelRuntimeOwner(kernel);

    expect(owner.runtime.kernel).toBe(kernel);
    expect(owner.runtime.files.openLocalImages).toBe(mobileFiles.openMobileLocalImages);
    mockedConfirm.mockResolvedValue(true);
    await expect(owner.runtime.files.confirmMarkdownFileDelete("note.md", {
      cancelLabel: "Cancel",
      message: "Delete note?",
      okLabel: "Delete",
    })).resolves.toBe(true);
    expect(owner.runtime.settings.readGroup).toEqual(expect.any(Function));
    expect(owner.runtime.settings.writeGroup).toEqual(expect.any(Function));
    await expect(owner.runtime.appConfig.readWorkspaceState()).resolves.toMatchObject({
      filePath: `${kernelWorkspaceRoot}/notes/main.md`,
      fileTreeOpen: false,
    });
    await expect(owner.runtime.settings.loadStore("local-state.json", {
      autoSave: false,
      defaults: {},
    })).rejects.toThrow("Renderer-local stores are unavailable");
    await owner.runtime.appConfig.patchState([{
      patch: { fileTreeOpen: true },
      type: "patch-ui-layout",
      windowLabel: "main",
    }]);
    await expect(owner.runtime.appConfig.readWorkspaceState()).resolves.toMatchObject({
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
    expect(owner.runtime.syncConfig.load).not.toBe(mobileRuntime.syncConfig.load);
    expect(owner.runtime.syncConfig.patch).not.toBe(mobileRuntime.syncConfig.patch);
    await expect(owner.runtime.syncConfig.loadJob({
      jobId: "00000000-0000-4000-8000-0000000000d2",
    })).resolves.toMatchObject({
      completionState: "succeeded",
      jobId: "00000000-0000-4000-8000-0000000000d2",
      revision: "sync-mobile-1",
    });
    expect(readRun).toHaveBeenCalledWith("00000000-0000-4000-8000-0000000000d2");
    await expect(owner.runtime.syncConfig.listNotebooks({
      revision: "sync-mobile-1",
    })).resolves.toEqual([expect.objectContaining({
      displayName: "Mobile shared notes",
      repositoryId: "323df833-764a-44b3-a534-492640c258f2",
    })]);
    expect(listNotebooks).toHaveBeenCalledWith("sync-mobile-1");
    expect(owner.runtime.workspace.rootPolicy).toMatchObject({
      canChooseLocalRoot: false,
      kind: "fixed",
    });
    await expect(owner.runtime.workspace.rootPolicy?.kind === "fixed"
      ? owner.runtime.workspace.rootPolicy.resolveRoot()
      : Promise.reject(new Error("mobile root remained selectable")))
      .resolves.toBe(kernelWorkspaceRoot);

    owner.release();
    owner.release();
  });

  it("round-trips canonical AppConfig paths across a committed mobile runtime-owner recreation", async () => {
    const kernel = readyKernelPort();
    const canonicalFilePath = `${kernelWorkspaceRoot}/notes/relaunch.md`;
    const canonicalDraftPath = `${kernelWorkspaceRoot}/notes/drafts/idea.md`;
    const committed: KernelAppConfigSnapshot = {
      ...kernel.appConfig.bootstrap,
      localState: {
        ...kernel.appConfig.bootstrap.localState,
        revision: "local-2" as KernelRevision,
        uiLayout: {
          ...kernel.appConfig.bootstrap.localState.uiLayout,
          windowStates: {
            main: {
              ...kernel.appConfig.bootstrap.localState.uiLayout.windowStates.main,
              activeDraftId: "draft-1",
              draftTabs: [{
                content: "# Draft",
                id: "draft-1",
                name: "idea.md",
                path: "notes/drafts/idea.md" as KernelWorkspaceRelativePath,
              }],
              filePath: "notes/relaunch.md" as KernelWorkspaceRelativePath,
              openFilePaths: ["notes/relaunch.md" as KernelWorkspaceRelativePath],
            },
          },
        },
      },
    };
    vi.mocked(kernel.appConfig.patchState).mockResolvedValueOnce(committed);
    const owner = createMobileKernelRuntimeOwner(kernel);
    configureAppRuntime(owner.runtime);

    const persisted = saveStoredWorkspaceState({
      activeDraftId: "draft-1",
      draftTabs: [{
        content: "# Draft",
        id: "draft-1",
        name: "idea.md",
        path: canonicalDraftPath,
      }],
      filePath: canonicalFilePath,
      openFilePaths: [canonicalFilePath],
    }, { windowLabel: "main" });
    await flushAppConfigStatePersistence();
    await persisted;

    expect(kernel.appConfig.patchState).toHaveBeenCalledWith({
      operations: [{
        patch: {
          activeDraftId: "draft-1",
          draftTabs: [{
            content: "# Draft",
            id: "draft-1",
            name: "idea.md",
            path: "notes/drafts/idea.md",
          }],
          filePath: "notes/relaunch.md",
          openFilePaths: ["notes/relaunch.md"],
          openWindows: [],
        },
        type: "patch-ui-layout",
        windowLabel: "main",
      }],
      workspaceGeneration: "generation-1",
    });

    owner.release();
    retireAppConfigStatePersistence();
    resetAppRuntimeForTests();
    const relaunchedOwner = createMobileKernelRuntimeOwner(readyKernelPort(committed));

    await expect(relaunchedOwner.runtime.appConfig.readWorkspaceState("main"))
      .resolves.toMatchObject({
        activeDraftId: "draft-1",
        draftTabs: [{
          content: "# Draft",
          id: "draft-1",
          name: "idea.md",
          path: canonicalDraftPath,
        }],
        filePath: canonicalFilePath,
        openFilePaths: [canonicalFilePath],
      });

    relaunchedOwner.release();
  });

  it("enables image import only for the ready Kernel runtime", () => {
    const kernel = readyKernelPort();
    const owner = createMobileKernelRuntimeOwner(kernel);

    expect(mobileRuntime.features.imageImport).toBe(false);
    expect(owner.runtime.features.imageImport).toBe(true);

    owner.release();
  });

  it("keeps native confirmation, image picker, system back, external links, and themes", async () => {
    mockedConfirm.mockResolvedValue(true);

    await expect(mobileRuntime.files.confirmMarkdownFileDelete("note.md", {
      cancelLabel: "Cancel",
      message: "Delete note?",
      okLabel: "Delete",
    })).resolves.toBe(true);

    expect(mobileRuntime.files.openLocalImages).toBe(mobileFiles.openMobileLocalImages);
    expect(mobileRuntime.navigation.subscribeToSystemBack)
      .toBe(mobileBack.subscribeToMobileSystemBack);
    expect(mobileRuntime.window.openExternalUrl).toEqual(expect.any(Function));
    expect(mobileRuntime.themes).toMatchObject({
      cancelActivation: themes.cancelNativeThemeActivation,
      commitActivation: themes.commitNativeThemeActivation,
      delete: themes.deleteNativeTheme,
      list: themes.listNativeThemes,
      prepareActivation: themes.prepareNativeThemeActivation,
      releaseActivation: themes.releaseNativeThemeActivation,
    });
  });

  it("keeps MCP unavailable until its policy moves behind the mobile Kernel", () => {
    expect(mobileRuntime.mcp).toMatchObject({
      localServiceAvailable: false,
      policyAvailable: false,
    });
    expect(mockedInvoke).not.toHaveBeenCalled();
  });
});

function readyKernelPort(bootstrapOverride?: KernelAppConfigSnapshot): KernelDomainPort {
  const unavailable = createUnavailableKernelDomainPort();
  const bootstrap: KernelAppConfigSnapshot = bootstrapOverride ?? {
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
          main: {
            activeDraftId: null,
            draftTabs: [],
            filePath: "notes/main.md" as KernelWorkspaceRelativePath,
            fileTreeAssetsVisible: true,
            fileTreeOpen: false,
            folderName: "notes",
            folderPath: "notes" as KernelWorkspaceRelativePath,
            openFilePaths: ["notes/main.md" as KernelWorkspaceRelativePath],
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
