import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { invoke } from "@tauri-apps/api/core";
import { confirm } from "@tauri-apps/plugin-dialog";
import {
  createUnavailableKernelDomainPort,
  kernelWorkspaceRoot,
  type KernelDomainPort,
  type KernelRevision,
  type KernelWorkspaceGeneration,
} from "@markra/app/runtime";

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
vi.mock("@tauri-apps/plugin-store", () => ({ load: vi.fn() }));

const mockedConfirm = vi.mocked(confirm);
const mockedInvoke = vi.mocked(invoke);

describe("mobile Kernel runtime boundary", () => {
  beforeEach(() => {
    mockedConfirm.mockReset();
    mockedInvoke.mockReset();
  });

  it("keeps the pre-Kernel shell free of legacy document, settings, and sync adapters", () => {
    const source = readFileSync(resolve(process.cwd(), "src/runtime/mobile.ts"), "utf8");

    expect(source).toContain('from "./tauri/file/mobile"');
    expect(source).toContain('from "./tauri/file/confirm"');
    expect(source).not.toContain('from "./tauri/file/shared"');
    expect(source).not.toContain('from "./tauri/settings"');
    expect(source).not.toContain('from "./tauri/sync-config/shared"');
    expect(source).not.toContain('from "./tauri/managed-workspace"');
    expect(source).not.toContain('from "./tauri/web-resource"');
    expect(mobileRuntime.kernel.availability).toBe("unavailable");
    expect(source).not.toContain('from "./tauri/mcp-policy"');
    expect(mobileRuntime.mcp.policyAvailable).toBe(false);
    expect(mobileRuntime.mcp.localServiceAvailable).toBe(false);
  });

  it("composes document, settings, and sync over one ready Kernel port", async () => {
    const kernel = readyKernelPort();
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
    });
    expect(owner.runtime.syncConfig.load).not.toBe(mobileRuntime.syncConfig.load);
    expect(owner.runtime.syncConfig.patch).not.toBe(mobileRuntime.syncConfig.patch);
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
          main: {
            activeDraftId: null,
            draftTabs: [],
            filePath: "notes/main.md" as never,
            fileTreeAssetsVisible: true,
            fileTreeOpen: false,
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
