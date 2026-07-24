import { appLogger, configureAppRuntime, createDefaultAppRuntime, resetAppRuntimeForTests } from "./index";

describe("app runtime logging", () => {
  afterEach(() => {
    resetAppRuntimeForTests();
    vi.restoreAllMocks();
  });

  it("connects the app logger to the configured runtime log backend", () => {
    const defaultRuntime = createDefaultAppRuntime();
    const writeLog = vi.fn();

    configureAppRuntime({
      ...defaultRuntime,
      logs: {
        isAvailable: () => true,
        openLogFolder: async () => undefined,
        writeLog
      }
    });

    appLogger.info("system", "Runtime logging configured", { operation: "test" });

    expect(writeLog).toHaveBeenCalledWith(expect.objectContaining({
      area: "system",
      details: {
        operation: "test"
      },
      level: "info",
      message: "Runtime logging configured"
    }));
  });

  it("retains core file, settings, web resource, and project configuration runtimes", () => {
    const runtime = createDefaultAppRuntime();

    expect(runtime).toHaveProperty("files");
    expect(runtime).toHaveProperty("settings");
    expect(runtime).toHaveProperty("webResource.downloadImage", expect.any(Function));
    expect(runtime).toHaveProperty("syncConfig");
  });

  it("does not expose legacy upload or folder-sync surfaces", () => {
    const defaultRuntime = createDefaultAppRuntime();
    const legacyFolderSyncKey = ["syncMarkdown", "Folder"].join("");

    expect(Object.keys(defaultRuntime.features)).not.toContain(["s3", "ImageUpload"].join(""));
    expect(Object.keys(defaultRuntime.files)).not.toContain(["upload", "Pic", "GoImage"].join(""));
    expect(Object.keys(defaultRuntime.files)).not.toContain(["upload", "S3Image"].join(""));
    expect(Object.keys(defaultRuntime.files)).not.toContain(["upload", "WebDavImage"].join(""));
    expect(Object.keys(defaultRuntime.files)).not.toContain(legacyFolderSyncKey);
  });
});

describe("default app runtime capabilities", () => {
  it("exposes the complete disabled feature matrix", () => {
    const runtime = createDefaultAppRuntime();

    expect(runtime.features).toEqual({
      applicationMenu: false,
      applicationShortcuts: false,
      export: false,
      fileDrop: false,
      imageImport: false,
      nativeWindowChrome: false,
      openLocalAttachments: false,
      pandoc: false,
      projectSync: false,
      resources: false,
      settingsWindow: false,
      systemFonts: false,
      updater: false
    });
  });

  it("provides a no-op system back subscriber", async () => {
    const runtime = createDefaultAppRuntime();
    const handler = vi.fn(async () => true);

    const cleanup = await runtime.navigation.subscribeToSystemBack(handler);

    expect(handler).not.toHaveBeenCalled();
    expect(cleanup()).toBeUndefined();
  });

  it("defaults to desktop form factor without a managed workspace", async () => {
    const runtime = createDefaultAppRuntime();

    expect(runtime.platform.resolveFormFactor()).toBe("desktop");
    await expect(runtime.workspace.resolveManagedRoot("personal")).resolves.toBeNull();
  });

  it("rejects primary cloud catalog delivery without a configured native runtime", async () => {
    const runtime = createDefaultAppRuntime();

    await expect(runtime.window.requestPrimaryCloudNotebookCatalog()).rejects.toThrow(
      /requestPrimaryCloudNotebookCatalog is unavailable/i
    );
  });

  it("models exact apply cancellation before accepting a new token", async () => {
    const syncConfig = createDefaultAppRuntime().syncConfig;
    await syncConfig.setEditing({ active: true, revision: "old-revision", sessionId: "old-session" });
    const oldApply = await syncConfig.requestApply({
      exitReason: "category-leave",
      revision: "old-revision",
      sessionId: "old-session",
      source: "settings-exit",
      token: "old-token"
    });

    await expect(syncConfig.requestApply({
      exitReason: "category-leave",
      revision: "old-revision",
      sessionId: "old-session",
      source: "settings-exit",
      token: "blocked-token"
    })).rejects.toThrow("sync-apply-pending:");
    await expect(syncConfig.cancelApply({
      revision: "old-revision",
      sessionId: "other-session",
      token: "old-token"
    })).rejects.toThrow("sync-apply-mismatch:");
    expect((await syncConfig.loadEditing()).pendingApply).toEqual(oldApply.event);

    const cancelled = await syncConfig.cancelApply({
      revision: "old-revision",
      sessionId: "old-session",
      token: "old-token"
    });
    expect(cancelled.event).toEqual(expect.objectContaining({
      revision: "old-revision",
      sessionId: "old-session",
      state: "completed",
      token: "old-token"
    }));

    await syncConfig.setEditing({ active: true, revision: "new-revision", sessionId: "new-session" });
    await expect(syncConfig.requestApply({
      exitReason: "window-close",
      revision: "new-revision",
      sessionId: "new-session",
      source: "settings-exit",
      token: "new-token"
    })).resolves.toEqual(expect.objectContaining({
      event: expect.objectContaining({ state: "pending", token: "new-token" })
    }));
  });

  it("compares primary workspace expected state structurally", async () => {
    const settings = createDefaultAppRuntime().settings;
    const storedState = {
      version: 1,
      onboardingCompleted: true,
      desktopPath: "/alias/Notes"
    };
    await settings.writePrimaryWorkspaceState?.({ state: storedState });

    const canonicalState = {
      desktopPath: "/canonical/Notes",
      onboardingCompleted: true,
      version: 1
    };
    await expect(settings.writePrimaryWorkspaceState?.({
      expectedState: {
        desktopPath: "/alias/Notes",
        onboardingCompleted: true,
        version: 1
      },
      state: canonicalState
    })).resolves.toEqual({ applied: true, state: canonicalState });
  });

  it("fails safely for unsupported theme activation without exposing a resource URL", async () => {
    const runtime = createDefaultAppRuntime();

    expect(runtime.themes).not.toHaveProperty("readCss");
    await expect(runtime.themes.prepareActivation("drake-ayu", "fingerprint")).rejects.toThrow(
      "prepareThemeActivation is unavailable"
    );
    await expect(runtime.themes.commitActivation("token")).rejects.toThrow(
      "commitThemeActivation is unavailable"
    );
    await expect(runtime.themes.cancelActivation("token")).rejects.toThrow(
      "cancelThemeActivation is unavailable"
    );
    await expect(runtime.themes.releaseActivation()).rejects.toThrow(
      "releaseThemeActivation is unavailable"
    );
  });
});
