import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { createDefaultAppRuntime } from "@markra/app/runtime";
import { desktopRuntime, normalizeAppFormFactor } from "./desktop";
import * as nativeRuntime from "./index";
import * as logs from "./tauri/logs";
import * as themes from "./tauri/themes";
import * as managedWorkspace from "./tauri/managed-workspace";
import * as windowRuntime from "./tauri/window";

vi.mock("@tauri-apps/api/event", () => ({
  emit: vi.fn(),
  listen: vi.fn()
}));

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: vi.fn(),
  invoke: vi.fn()
}));

vi.mock("@tauri-apps/plugin-os", () => ({
  platform: vi.fn(() => "macos"),
  version: vi.fn(() => "26.5.1")
}));

vi.mock("@tauri-apps/plugin-store", () => ({
  load: vi.fn()
}));

vi.mock("./tauri/logs", () => ({
  isNativeLoggingAvailable: vi.fn(() => true),
  openNativeLogFolder: vi.fn(),
  writeNativeLog: vi.fn()
}));

vi.mock("@markra/shared", async (importOriginal) => ({
  ...await importOriginal<typeof import("@markra/shared")>(),
  hasTauriRuntime: vi.fn(() => true)
}));

const mockedListen = vi.mocked(listen);
const mockedInvoke = vi.mocked(invoke);

function createInjectedLoaders() {
  const injectedDesktopRuntime = createDefaultAppRuntime();
  const injectedMobileRuntime = createDefaultAppRuntime();
  const desktop = vi.fn(async () => ({ desktopRuntime: injectedDesktopRuntime }));
  const mobile = vi.fn(async () => ({ mobileRuntime: injectedMobileRuntime }));

  return {
    desktop,
    injectedDesktopRuntime,
    injectedMobileRuntime,
    mobile
  };
}

describe("native runtime selection", () => {
  it.each(["android", "ios"])("classifies %s as mobile", (platform) => {
    expect(nativeRuntime.nativeRuntimeKind(platform)).toBe("mobile");
  });

  it.each(["macos", "windows", "linux", "unknown", null, undefined])(
    "classifies %s as desktop",
    (platform) => {
      expect(nativeRuntime.nativeRuntimeKind(platform)).toBe("desktop");
    }
  );

  it.each(["android", "ios"])("loads only the mobile runtime on %s", async (platform) => {
    const loaders = createInjectedLoaders();

    await expect(nativeRuntime.loadNativeRuntime(() => platform, loaders)).resolves.toBe(loaders.injectedMobileRuntime);
    expect(loaders.mobile).toHaveBeenCalledTimes(1);
    expect(loaders.desktop).not.toHaveBeenCalled();
  });

  it.each(["macos", "windows", "linux"])("loads only the desktop runtime on %s", async (platform) => {
    const loaders = createInjectedLoaders();

    await expect(nativeRuntime.loadNativeRuntime(() => platform, loaders)).resolves.toBe(loaders.injectedDesktopRuntime);
    expect(loaders.desktop).toHaveBeenCalledTimes(1);
    expect(loaders.mobile).not.toHaveBeenCalled();
  });

  it("falls back to the desktop runtime when the OS plugin throws", async () => {
    const loaders = createInjectedLoaders();
    const readPlatform = vi.fn(() => {
      throw new Error("OS plugin unavailable");
    });

    await expect(nativeRuntime.loadNativeRuntime(readPlatform, loaders)).resolves.toBe(loaders.injectedDesktopRuntime);
    expect(loaders.desktop).toHaveBeenCalledTimes(1);
    expect(loaders.mobile).not.toHaveBeenCalled();
  });

  it("does not fall back to desktop when a selected mobile loader rejects", async () => {
    const loaders = createInjectedLoaders();
    const loadError = new Error("mobile runtime failed to load");
    loaders.mobile.mockRejectedValueOnce(loadError);

    await expect(nativeRuntime.loadNativeRuntime(() => "android", loaders)).rejects.toBe(loadError);
    expect(loaders.mobile).toHaveBeenCalledTimes(1);
    expect(loaders.desktop).not.toHaveBeenCalled();
  });
});

describe("desktop runtime form factor", () => {
  it("normalizes native mobile platforms", () => {
    expect(normalizeAppFormFactor("android")).toBe("mobile");
    expect(normalizeAppFormFactor("ios")).toBe("mobile");
    expect(normalizeAppFormFactor("macos")).toBe("desktop");
    expect(normalizeAppFormFactor("windows")).toBe("desktop");
  });
});

describe("desktop runtime events", () => {
  beforeEach(() => {
    mockedListen.mockReset();
  });

  it("ignores stale Tauri unlisten failures and only cleans up once", async () => {
    const cleanup = vi.fn().mockRejectedValue(new Error("undefined is not an object (evaluating 'listeners[eventId].handlerId')"));
    mockedListen.mockResolvedValue(cleanup);

    const stopListening = await desktopRuntime.events.listen("markra://synthetic-event", () => {});

    await expect(Promise.resolve(stopListening())).resolves.toBeUndefined();
    await expect(Promise.resolve(stopListening())).resolves.toBeUndefined();
    expect(cleanup).toHaveBeenCalledTimes(1);
  });
});

describe("desktop runtime logs", () => {
  it("exposes the native log runtime", () => {
    expect(desktopRuntime.logs.isAvailable()).toBe(true);
    expect(desktopRuntime.logs.openLogFolder).toBe(logs.openNativeLogFolder);
    expect(desktopRuntime.logs.writeLog).toBe(logs.writeNativeLog);
  });
});

describe("desktop runtime retained capabilities", () => {
  it("exposes the exact enabled desktop feature matrix", () => {
    expect(desktopRuntime.features).toEqual({
      applicationMenu: true,
      applicationShortcuts: true,
      export: true,
      fileDrop: true,
      imageImport: true,
      nativeWindowChrome: true,
      openLocalAttachments: true,
      pandoc: true,
      projectSync: true,
      resources: true,
      settingsWindow: true,
      systemFonts: true,
      updater: true
    });
  });

  it("provides a no-op system back subscriber", async () => {
    const handler = vi.fn(async () => true);

    const cleanup = await desktopRuntime.navigation.subscribeToSystemBack(handler);

    expect(handler).not.toHaveBeenCalled();
    expect(cleanup()).toBeUndefined();
  });

  it("exposes file, settings, web resource, and project configuration runtimes", () => {
    expect(desktopRuntime).toHaveProperty("files");
    expect(desktopRuntime).toHaveProperty("settings");
    expect(desktopRuntime).toHaveProperty("webResource.downloadImage", expect.any(Function));
    expect(desktopRuntime).toHaveProperty("syncConfig");
    const workspace = desktopRuntime.workspace as typeof desktopRuntime.workspace & {
      listManagedNotebookNames?: () => Promise<string[]>;
    };
    const adapter = managedWorkspace as typeof managedWorkspace & {
      listNativeManagedWorkspaceNames?: () => Promise<string[]>;
    };
    expect(workspace.listManagedNotebookNames).toBe(adapter.listNativeManagedWorkspaceNames);
    expect(workspace.listManagedNotebookNames).toEqual(expect.any(Function));
  });

  it("maps primary cloud catalog delivery to the native window request", async () => {
    mockedInvoke.mockClear();
    mockedInvoke.mockResolvedValue(undefined);

    await desktopRuntime.window.requestPrimaryCloudNotebookCatalog();

    expect(desktopRuntime.window.requestPrimaryCloudNotebookCatalog).toBe(
      windowRuntime.requestNativePrimaryCloudNotebookCatalog
    );
    expect(mockedInvoke).toHaveBeenCalledWith("request_primary_cloud_notebook_catalog");
  });

  it("retains all desktop theme capabilities and activation adapters", () => {
    expect(desktopRuntime.themes.capabilities).toEqual({
      canDelete: true,
      canImport: true,
      canOpenDirectory: true
    });
    expect(desktopRuntime.themes).toMatchObject({
      cancelActivation: themes.cancelNativeThemeActivation,
      commitActivation: themes.commitNativeThemeActivation,
      prepareActivation: themes.prepareNativeThemeActivation,
      releaseActivation: themes.releaseNativeThemeActivation
    });
    expect(desktopRuntime.themes).not.toHaveProperty("readCss");
  });

  it("routes typed setting groups through native commands", async () => {
    mockedInvoke.mockClear();
    mockedInvoke
      .mockResolvedValueOnce({ appearanceMode: "dark" })
      .mockResolvedValueOnce(undefined)
      .mockResolvedValueOnce(undefined);

    await expect(desktopRuntime.settings.readGroup?.("appearance")).resolves.toEqual({ appearanceMode: "dark" });
    await expect(
      desktopRuntime.settings.writeGroup?.("appearance", { appearanceMode: "dark" })
    ).resolves.toBeUndefined();
    await expect(
      desktopRuntime.settings.replacePortable?.({ language: "zh-CN" })
    ).resolves.toBeUndefined();

    expect(mockedInvoke).toHaveBeenNthCalledWith(1, "read_app_settings_group", { group: "appearance" });
    expect(mockedInvoke).toHaveBeenNthCalledWith(2, "write_app_settings_group", {
      group: "appearance",
      value: { appearanceMode: "dark" }
    });
    expect(mockedInvoke).toHaveBeenNthCalledWith(3, "replace_portable_app_settings", {
      settings: { language: "zh-CN" }
    });
  });

  it("routes primary workspace persistence through the native application-wide boundary", async () => {
    mockedInvoke.mockClear();
    const state = {
      desktopPath: "/Notes-B",
      onboardingCompleted: true,
      version: 1
    };
    const expectedState = {
      desktopPath: "/alias/Notes-A",
      onboardingCompleted: true,
      version: 1
    };
    mockedInvoke
      .mockResolvedValueOnce(state)
      .mockResolvedValueOnce({ applied: true, state });

    await expect(desktopRuntime.settings.readPrimaryWorkspaceState?.()).resolves.toEqual(state);
    await expect(desktopRuntime.settings.writePrimaryWorkspaceState?.({
      expectedState,
      state
    })).resolves.toEqual({ applied: true, state });

    expect(mockedInvoke).toHaveBeenNthCalledWith(1, "read_primary_workspace_state");
    expect(mockedInvoke).toHaveBeenNthCalledWith(2, "write_primary_workspace_state", {
      input: {
        expectedState,
        state
      }
    });
  });

  it("maps every MCP control operation to its native command", async () => {
    mockedInvoke.mockClear();
    mockedInvoke.mockResolvedValue(undefined);

    await desktopRuntime.mcp.getSettings();
    await desktopRuntime.mcp.updateSettings({ expectedRevision: "r1", config: {} as never });
    await desktopRuntime.mcp.setPrimaryWorkspace({ primaryRoot: "/notes" });
    await desktopRuntime.mcp.getHealth();
    await desktopRuntime.mcp.listAuditEntries(0, 100);
    await desktopRuntime.mcp.clearAuditEntries();

    expect(mockedInvoke.mock.calls.map(([command]) => command)).toEqual([
      "get_mcp_settings",
      "update_mcp_settings",
      "set_mcp_primary_workspace",
      "get_mcp_health",
      "list_mcp_audit_entries",
      "clear_mcp_audit_entries"
    ]);
  });
});

describe("desktop runtime reduced upload surface", () => {
  it("does not expose legacy upload or folder-sync surfaces", () => {
    const legacyFolderSyncKey = ["syncMarkdown", "Folder"].join("");

    expect(Object.keys(desktopRuntime.features)).not.toContain(["s3", "ImageUpload"].join(""));
    expect(Object.keys(desktopRuntime.files)).not.toContain(["upload", "Pic", "GoImage"].join(""));
    expect(Object.keys(desktopRuntime.files)).not.toContain(["upload", "S3Image"].join(""));
    expect(Object.keys(desktopRuntime.files)).not.toContain(["upload", "WebDavImage"].join(""));
    expect(Object.keys(desktopRuntime.files)).not.toContain(legacyFolderSyncKey);
  });
});
