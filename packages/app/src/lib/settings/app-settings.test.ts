import { createSettingsStoreHarness, resetSettingsStoreRuntime, setupSettingsStoreHarness } from "../../test/settings-store";
import {
  configureAppRuntime,
  createDefaultAppRuntime,
  type AppSettingsGroup,
  type AppSettingsRuntime
} from "../../runtime";
import {
  darkEditorThemeOptions,
  approveThemeFingerprint,
  defaultExportSettings,
  defaultFileIgnoreSettings,
  defaultEditorPreferences,
  exportStoredAppSettings,
  getStoredThemePreferences,
  getApprovedThemeFingerprint,
  getStoredCustomThemeCss,
  getStoredWorkspaceState,
  appThemeOptions,
  consumeWelcomeDocumentState,
  editorThemeOptions,
  getStoredLanguage,
  getStoredEditorPreferences,
  getStoredExportSettings,
  getStoredFileIgnoreSettings,
  getStoredTheme,
  isAppAppearanceMode,
  isAppTheme,
  lightEditorThemeOptions,
  importStoredAppSettings,
  forgetApprovedThemeFingerprint,
  isThemeId,
  resetWelcomeDocumentState,
  normalizeEditorPreferences,
  resolveAppAppearanceTheme,
  resolveAppThemePreferencesAppearance,
  resolveAppThemePreferencesEditorTheme,
  saveStoredCustomThemeCss,
  saveStoredLanguage,
  saveStoredEditorPreferences,
  saveStoredExportSettings,
  saveStoredFileIgnoreSettings,
  saveStoredTheme,
  saveStoredThemePreferences
} from "./app-settings";

const settingsStore = createSettingsStoreHarness();
const { loadStore: mockedLoadStore, store } = settingsStore;

describe("app settings", () => {
  beforeEach(() => {
    setupSettingsStoreHarness(settingsStore);
  });

  afterEach(() => {
    resetSettingsStoreRuntime();
  });

  it("consumes and persists the first welcome document state in the Tauri app data store", async () => {
    store.get.mockResolvedValue(undefined);

    await expect(consumeWelcomeDocumentState()).resolves.toBe(true);

    expect(mockedLoadStore).toHaveBeenCalledWith("local-state.json", { autoSave: false, defaults: {} });
    expect(store.get).toHaveBeenCalledWith("welcomeDocumentSeen");
    expect(store.set).toHaveBeenCalledWith("welcomeDocumentSeen", true);
    expect(store.save).toHaveBeenCalledTimes(1);
  });

  it("does not rewrite settings after the welcome document was already seen", async () => {
    store.get.mockResolvedValue(true);

    await expect(consumeWelcomeDocumentState()).resolves.toBe(false);

    expect(store.set).not.toHaveBeenCalled();
    expect(store.save).not.toHaveBeenCalled();
  });

  it("loads a persisted global theme from settings", async () => {
    store.get.mockResolvedValue("catppuccin-mocha");

    await expect(getStoredTheme()).resolves.toBe("catppuccin-mocha");

    expect(store.get).toHaveBeenCalledWith("theme");
  });

  it("loads and persists the system color theme preference", async () => {
    store.get.mockResolvedValue("system");

    await expect(getStoredTheme()).resolves.toBe("system");

    await saveStoredTheme("system");

    expect(store.set).toHaveBeenCalledWith("theme", "system");
    expect(store.save).toHaveBeenCalledTimes(1);
  });

  it("falls back to the system theme preference when the stored theme is missing or invalid", async () => {
    store.get.mockResolvedValue("dracula");

    await expect(getStoredTheme()).resolves.toBe("system");
  });

  it("persists the selected global theme", async () => {
    await saveStoredTheme("solarized-dark");

    expect(store.set).toHaveBeenCalledWith("theme", "solarized-dark");
    expect(store.save).toHaveBeenCalledTimes(1);
  });

  it("migrates a legacy light theme into split appearance preferences", async () => {
    store.get.mockImplementation(async (key: string) => {
      if (key === "theme") return "sepia";

      return undefined;
    });

    await expect(getStoredThemePreferences()).resolves.toEqual({
      appearanceMode: "light",
      darkTheme: "dark",
      lightTheme: "sepia"
    });

    expect(store.get).toHaveBeenCalledWith("appearanceMode");
    expect(store.get).toHaveBeenCalledWith("lightTheme");
    expect(store.get).toHaveBeenCalledWith("darkTheme");
    expect(store.get).toHaveBeenCalledWith("theme");
  });

  it("loads and persists split appearance preferences", async () => {
    store.get.mockImplementation(async (key: string) => {
      if (key === "appearanceMode") return "system";
      if (key === "lightThemeId") return "solarized-light";
      if (key === "darkThemeId") return "night";

      return undefined;
    });

    await expect(getStoredThemePreferences()).resolves.toEqual({
      appearanceMode: "system",
      darkTheme: "night",
      lightTheme: "solarized-light"
    });

    await saveStoredThemePreferences({
      appearanceMode: "dark",
      darkTheme: "catppuccin-mocha",
      lightTheme: "catppuccin-latte"
    });

    expect(store.set).toHaveBeenCalledWith("appearanceMode", "dark");
    expect(store.set).toHaveBeenCalledWith("lightThemeId", "catppuccin-latte");
    expect(store.set).toHaveBeenCalledWith("darkThemeId", "catppuccin-mocha");
    expect(store.set).not.toHaveBeenCalledWith("theme", expect.any(String));
    expect(store.save).toHaveBeenCalledTimes(1);
  });

  it("accepts syntactically valid third-party theme IDs", () => {
    expect(isThemeId("user-theme-2")).toBe(true);
    expect(isThemeId("qingyu-private")).toBe(false);
    expect(isThemeId("../theme")).toBe(false);
  });

  it("stores approved theme fingerprints locally and forgets deleted themes", async () => {
    const fingerprint = "a".repeat(64);
    let approvals: unknown;
    store.get.mockImplementation(async (key: string) => key === "approvedThemeFingerprints" ? approvals : undefined);
    store.set.mockImplementation(async (key: string, value: unknown) => {
      if (key === "approvedThemeFingerprints") approvals = value;
    });

    await approveThemeFingerprint("nord", fingerprint);
    await expect(getApprovedThemeFingerprint("nord")).resolves.toBe(fingerprint);
    await forgetApprovedThemeFingerprint("nord");
    await expect(getApprovedThemeFingerprint("nord")).resolves.toBeNull();
  });

  it("resolves the active editor theme from appearance mode and saved palettes", () => {
    const preferences = {
      appearanceMode: "system" as const,
      darkTheme: "night" as const,
      lightTheme: "sepia" as const
    };

    expect(isAppAppearanceMode("system")).toBe(true);
    expect(isAppAppearanceMode("sepia")).toBe(false);
    expect(lightEditorThemeOptions).toContain("sepia");
    expect(lightEditorThemeOptions).not.toContain("night");
    expect(darkEditorThemeOptions).toContain("night");
    expect(darkEditorThemeOptions).not.toContain("sepia");
    expect(resolveAppThemePreferencesAppearance(preferences, "dark")).toBe("dark");
    expect(resolveAppThemePreferencesEditorTheme(preferences, "dark")).toBe("night");
    expect(resolveAppThemePreferencesEditorTheme(preferences, "light")).toBe("sepia");
  });

  it("recognizes GitHub and One theme options", () => {
    const requestedThemes = ["github-dark", "one-dark", "one-light", "one-dark-pro"];

    expect(editorThemeOptions).toEqual(expect.arrayContaining(requestedThemes));
    expect(appThemeOptions).toEqual(expect.arrayContaining(requestedThemes));

    for (const theme of requestedThemes) {
      expect(isAppTheme(theme)).toBe(true);
    }

    expect(resolveAppAppearanceTheme("github-dark" as Parameters<typeof resolveAppAppearanceTheme>[0], "light")).toBe("dark");
    expect(resolveAppAppearanceTheme("one-dark" as Parameters<typeof resolveAppAppearanceTheme>[0], "light")).toBe("dark");
    expect(resolveAppAppearanceTheme("one-light" as Parameters<typeof resolveAppAppearanceTheme>[0], "dark")).toBe("light");
    expect(resolveAppAppearanceTheme("one-dark-pro" as Parameters<typeof resolveAppAppearanceTheme>[0], "light")).toBe("dark");
  });

  it("loads English as the default app language", async () => {
    store.get.mockResolvedValue("pirate");

    await expect(getStoredLanguage()).resolves.toBe("en");

    expect(store.get).toHaveBeenCalledWith("language");
  });

  it("loads and persists a supported app language", async () => {
    store.get.mockResolvedValue("zh-CN");

    await expect(getStoredLanguage()).resolves.toBe("zh-CN");

    await saveStoredLanguage("ja");

    expect(store.set).toHaveBeenCalledWith("language", "ja");
    expect(store.save).toHaveBeenCalledTimes(1);
  });

  it("routes the five exposed setting groups through the group runtime when available", async () => {
    const readGroups: AppSettingsGroup[] = [];
    const writtenGroups: AppSettingsGroup[] = [];
    const readGroup: NonNullable<AppSettingsRuntime["readGroup"]> = async <TValue>(group: AppSettingsGroup) => {
      readGroups.push(group);
      let value: unknown;
      if (group === "appearance") {
        value = { appearanceMode: "dark", darkTheme: "night", lightTheme: "minimal" };
      } else if (group === "language") {
        value = "zh-CN";
      } else if (group === "editorPreferences") {
        value = { ...defaultEditorPreferences, bodyFontSize: 20 };
      } else if (group === "fileIgnoreSettings") {
        value = { rules: "generated/" };
      } else if (group === "exportSettings") {
        value = { ...defaultExportSettings, pdfAuthor: "QingYu" };
      }

      return value as TValue | undefined;
    };
    const writeGroup: NonNullable<AppSettingsRuntime["writeGroup"]> = async (group) => {
      writtenGroups.push(group);
      return undefined;
    };
    configureAppRuntime({
      ...createDefaultAppRuntime(),
      settings: {
        loadStore: mockedLoadStore,
        readGroup,
        writeGroup
      }
    });

    await expect(getStoredThemePreferences()).resolves.toMatchObject({ appearanceMode: "dark" });
    await expect(getStoredLanguage()).resolves.toBe("zh-CN");
    await expect(getStoredEditorPreferences()).resolves.toMatchObject({ bodyFontSize: 20 });
    await expect(getStoredFileIgnoreSettings()).resolves.toEqual({ rules: "generated/" });
    await expect(getStoredExportSettings()).resolves.toMatchObject({ pdfAuthor: "QingYu" });

    await saveStoredThemePreferences({ appearanceMode: "dark", darkTheme: "night", lightTheme: "minimal" });
    await saveStoredLanguage("zh-CN");
    await saveStoredEditorPreferences(defaultEditorPreferences);
    await saveStoredFileIgnoreSettings(defaultFileIgnoreSettings);
    await saveStoredExportSettings(defaultExportSettings);

    expect(readGroups).toEqual([
      "appearance",
      "language",
      "editorPreferences",
      "fileIgnoreSettings",
      "exportSettings"
    ]);
    expect(writtenGroups).toEqual([
      "appearance",
      "language",
      "editorPreferences",
      "fileIgnoreSettings",
      "exportSettings"
    ]);
    expect(mockedLoadStore).toHaveBeenCalledTimes(2);
    expect(mockedLoadStore).toHaveBeenNthCalledWith(1, "local-state.json", {
      autoSave: false,
      defaults: {}
    });
    expect(mockedLoadStore).toHaveBeenNthCalledWith(2, "local-state.json", {
      autoSave: false,
      defaults: {}
    });
  });

  it("routes custom theme CSS through the native settings writer", async () => {
    const runtime = createDefaultAppRuntime();
    const writeGroup = vi.fn(async () => undefined);
    configureAppRuntime({
      ...runtime,
      settings: { ...runtime.settings, writeGroup }
    });

    await saveStoredCustomThemeCss({ dark: "dark-css", light: "light-css" });

    expect(writeGroup).toHaveBeenCalledWith("customThemeCss", {
      dark: "dark-css",
      light: "light-css"
    });
    expect(store.set).not.toHaveBeenCalled();
    expect(store.save).not.toHaveBeenCalled();
  });

   it("normalizes the automatic active file reveal preference", () => {
    expect(defaultEditorPreferences.autoRevealActiveFile).toBe(false);
    expect(normalizeEditorPreferences({ autoRevealActiveFile: false }).autoRevealActiveFile).toBe(false);
    expect(normalizeEditorPreferences({ autoRevealActiveFile: "sometimes" }).autoRevealActiveFile).toBe(false);
  });

  it("normalizes the document links visibility preference", () => {
    expect(defaultEditorPreferences.documentLinksVisible).toBe(false);
    expect(normalizeEditorPreferences({ documentLinksVisible: true }).documentLinksVisible).toBe(true);
    expect(normalizeEditorPreferences({ documentLinksVisible: "yes" }).documentLinksVisible).toBe(false);
  });

  it("normalizes the document links collapse preference", () => {
    expect(defaultEditorPreferences.documentLinksOpen).toBe(true);
    expect(normalizeEditorPreferences({ documentLinksOpen: false }).documentLinksOpen).toBe(false);
    expect(normalizeEditorPreferences({ documentLinksOpen: "no" }).documentLinksOpen).toBe(true);
  });

  it("normalizes the source line number preference", () => {
    expect(defaultEditorPreferences.showLineNumbers).toBe(false);
    expect(normalizeEditorPreferences({ showLineNumbers: true }).showLineNumbers).toBe(true);
    expect(normalizeEditorPreferences({ showLineNumbers: "yes" }).showLineNumbers).toBe(false);
  });

  it("resets the welcome document state for the next launch", async () => {
    await resetWelcomeDocumentState();

    expect(store.delete).toHaveBeenCalledWith("welcomeDocumentSeen");
    expect(store.save).toHaveBeenCalledTimes(1);
  });

  it("loads workspace state only from the device-local store", async () => {
    store.get.mockResolvedValue(undefined);

    await getStoredWorkspaceState({ windowLabel: "main" });

    expect(mockedLoadStore).toHaveBeenCalledWith("local-state.json", { autoSave: false, defaults: {} });
    expect(mockedLoadStore).not.toHaveBeenCalledWith("settings.json", expect.anything());
    expect(store.get).toHaveBeenCalledWith("workspace");
  });

  it("exports portable app settings without workspace-local state", async () => {
    store.get.mockImplementation(async (key: string) => {
      if (key === "language") return "zh-CN";
      if (key === "appearanceMode") return "dark";
      if (key === "lightTheme") return "minimal";
      if (key === "darkTheme") return "night";
      if (key === "lightCustomThemeCss") return ":root[data-theme=\"custom\"] { --bg-primary: #fff; }";
      if (key === "darkCustomThemeCss") return ":root[data-theme=\"custom\"] { --bg-primary: #111; }";
      if (key === "editorPreferences") {
        return {
          ...defaultEditorPreferences,
          autoSaveEnabled: false,
          imageUpload: {
            ...defaultEditorPreferences.imageUpload,
            provider: "webdav",
            webdav: { password: "", serverUrl: "https://dav.example.test/images" }
          }
        };
      }
      if (key === "exportSettings") {
        return {
          ...defaultExportSettings,
          pandocPath: "/private/bin/pandoc",
          pdfAuthor: "QingYu"
        };
      }
      if (key === "workspace") {
        return {
          filePath: "/private/example.md"
        };
      }
      if (key === "recentMarkdownFiles") {
        return [{ name: "example.md", path: "/private/example.md" }];
      }
      return undefined;
    });

    const exported = JSON.parse(
      await exportStoredAppSettings(new Date("2030-01-02T03:04:05.000Z"))
    );

    expect(exported).toMatchObject({
      exportedAt: "2030-01-02T03:04:05.000Z",
      format: "markra-settings",
      version: 3,
      settings: {
        appearanceMode: "dark",
        darkTheme: "night",
        editorPreferences: {
          autoSaveEnabled: false,
          imageUpload: {
            fileNamePattern: "pasted-image-{timestamp}"
          }
        },
        language: "zh-CN",
        lightTheme: "minimal"
      }
    });
    expect(exported.settings).not.toHaveProperty("workspace");
    expect(exported.settings).not.toHaveProperty("recentMarkdownFiles");
    expect(exported.settings.exportSettings).not.toHaveProperty("pandocPath");
    expect(Object.keys(exported.settings).sort()).toEqual([
      "appearanceMode",
      "customThemeCss",
      "darkTheme",
      "editorPreferences",
      "exportSettings",
      "fileIgnoreSettings",
      "language",
      "lightTheme"
    ]);
    expect(Object.keys(exported.settings)).not.toContain("syncSettings");
    expect(Object.keys(exported.settings.editorPreferences.imageUpload)).toEqual(["fileNamePattern"]);
    expect(store.get).not.toHaveBeenCalledWith("workspace");
    expect(store.get).not.toHaveBeenCalledWith("recentMarkdownFiles");
  });

  it("imports portable app settings after validating the file contents", async () => {
    const importedSettingsFile = {
      format: "markra-settings",
      version: 1,
      exportedAt: "2030-01-02T03:04:05.000Z",
      settings: {
        appearanceMode: "dark",
        customThemeCss: {
          dark: ":root[data-theme=\"custom\"] { --bg-primary: #111; }",
          light: ":root[data-theme=\"custom\"] { --bg-primary: #fff; }"
        },
        darkTheme: "night",
        editorPreferences: {
          ...defaultEditorPreferences,
          autoSaveEnabled: false,
          bodyFontSize: 20
        },
        language: "zh-CN",
        lightTheme: "minimal",
        mcp: { version: 1, enabled: true },
        exportSettings: {
          ...defaultExportSettings,
          pandocPath: "/private/bin/pandoc"
        },
        recentMarkdownFiles: [{ name: "example.md", path: "/private/example.md" }],
        workspace: {
          filePath: "/private/example.md"
        }
      }
    };

    const importedSettings = await importStoredAppSettings(JSON.stringify(importedSettingsFile));

    expect(Object.keys(importedSettings)).not.toContain("syncSettings");
    expect(Object.keys(importedSettings).sort()).toEqual([
      "appearanceMode",
      "customThemeCss",
      "darkTheme",
      "editorPreferences",
      "exportSettings",
      "fileIgnoreSettings",
      "language",
      "lightTheme"
    ]);
    expect(store.set.mock.calls.map(([key]) => key)).not.toContain("syncSettings");

    expect(importedSettings).toMatchObject({
      appearanceMode: "dark",
      darkTheme: "night",
      language: "zh-CN",
      lightTheme: "minimal"
    });
    expect(mockedLoadStore).toHaveBeenCalledWith("settings.json", { autoSave: false, defaults: {} });
    expect(store.set).toHaveBeenCalledWith("language", "zh-CN");
    expect(store.set).toHaveBeenCalledWith("appearanceMode", "dark");
    expect(store.set).toHaveBeenCalledWith("lightThemeId", "minimal");
    expect(store.set).toHaveBeenCalledWith("darkThemeId", "night");
    expect(store.set).not.toHaveBeenCalledWith("mcp", expect.anything());
    expect(store.set).toHaveBeenCalledWith(
      "editorPreferences",
      expect.objectContaining({
        autoSaveEnabled: false,
        bodyFontSize: 20
      })
    );
    expect(store.set).not.toHaveBeenCalledWith("workspace", expect.anything());
    expect(store.set).not.toHaveBeenCalledWith("recentMarkdownFiles", expect.anything());
    expect(store.set).toHaveBeenCalledWith(
      "exportSettings",
      expect.not.objectContaining({ pandocPath: expect.anything() })
    );
    expect(store.set).not.toHaveBeenCalledWith("pandocPath", expect.anything());
    expect(store.save).toHaveBeenCalledTimes(1);
  });

  it("replaces imported portable settings through one native transaction", async () => {
    const runtime = createDefaultAppRuntime();
    const replacePortable = vi.fn(async () => undefined);
    configureAppRuntime({
      ...runtime,
      settings: { ...runtime.settings, replacePortable }
    });

    await importStoredAppSettings(JSON.stringify({
      exportedAt: "2030-01-02T03:04:05.000Z",
      format: "markra-settings",
      settings: {
        appearanceMode: "dark",
        customThemeCss: { dark: "dark-css", light: "light-css" },
        darkTheme: "night",
        language: "zh-CN",
        lightTheme: "minimal"
      },
      version: 1
    }));

    expect(replacePortable).toHaveBeenCalledTimes(1);
    expect(replacePortable).toHaveBeenCalledWith(expect.objectContaining({
      appearanceMode: "dark",
      customThemeCss: { dark: "dark-css", light: "light-css" },
      darkTheme: "night",
      language: "zh-CN",
      lightTheme: "minimal"
    }));
    expect(store.set).not.toHaveBeenCalled();
    expect(store.save).not.toHaveBeenCalled();
  });

  it.each([1, 2])("ignores MCP from legacy version %s imports", async (version) => {
    const runtime = createDefaultAppRuntime();
    const getSettings = vi.fn(runtime.mcp.getSettings);
    const updateSettings = vi.fn(runtime.mcp.updateSettings);
    configureAppRuntime({
      ...runtime,
      mcp: {
        ...runtime.mcp,
        policyAvailable: true,
        localServiceAvailable: true,
        getSettings,
        updateSettings
      },
      settings: { ...runtime.settings, loadStore: mockedLoadStore }
    });

    await importStoredAppSettings(JSON.stringify({
      exportedAt: "2030-01-02T03:04:05.000Z",
      format: "markra-settings",
      settings: { language: "zh-CN", mcp: { version: 1, enabled: true } },
      version
    }));

    expect(getSettings).not.toHaveBeenCalled();
    expect(updateSettings).not.toHaveBeenCalled();
    expect(store.set).not.toHaveBeenCalledWith("mcp", expect.anything());
    expect(store.set).toHaveBeenCalledWith("language", "zh-CN");
  });

  it("rejects invalid settings imports without writing to the settings store", async () => {
    await expect(importStoredAppSettings("{not json")).rejects.toThrow("Invalid QingYu settings file.");

    expect(mockedLoadStore).not.toHaveBeenCalled();
    expect(store.set).not.toHaveBeenCalled();
    expect(store.save).not.toHaveBeenCalled();
  });
});
