import { defaultMarkdownShortcuts } from "@markra/editor";
import {
  configureAppRuntime,
  createDefaultAppRuntime,
  resetAppRuntimeForTests,
  type AppSettingsGroup
} from "../../runtime";
import {
  listenAppEditorPreferencesChanged,
  listenAppExportSettingsChanged,
  listenAppFileIgnoreSettingsChanged,
  listenAppLanguageChanged,
  listenAppThemeChanged,
  notifyAppEditorPreferencesChanged,
  notifyAppExportSettingsChanged,
  notifyAppFileIgnoreSettingsChanged,
  notifyAppLanguageChanged,
  notifyAppThemeChanged
} from "./settings-events";
import * as settingsEvents from "./settings-events";
import {
  defaultEditorPreferences,
  type EditorPreferences
} from "./app-settings";
import { defaultMcpConfig } from "../mcp";

const mockedEmit = vi.fn();
const mockedListen = vi.fn();

function configureSyncedExportSettingsRuntime(
  exportSettings: Record<string, unknown> | undefined,
  pandocPath = "/opt/homebrew/bin/pandoc"
) {
  const runtime = createDefaultAppRuntime();

  configureAppRuntime({
    ...runtime,
    events: {
      emit: mockedEmit,
      isAvailable: () => true,
      listen: mockedListen
    },
    settings: {
      ...runtime.settings,
      async loadStore(path) {
        return {
          async delete() {},
          async get<TValue>(key: string) {
            if (path === "local-state.json" && key === "pandocPath") return pandocPath as TValue;
            if (path === "settings.json" && key === "exportSettings") return exportSettings as TValue;
            return undefined;
          },
          async save() {},
          async set() {}
        };
      },
      async readGroup<TValue>(group: AppSettingsGroup) {
        return (group === "exportSettings" ? exportSettings : undefined) as TValue | undefined;
      }
    }
  });
}

describe("settings events", () => {
  let eventsAvailable = false;

  beforeEach(() => {
    mockedEmit.mockReset();
    mockedListen.mockReset();
    eventsAvailable = false;
    configureAppRuntime({
      ...createDefaultAppRuntime(),
      events: {
        emit: mockedEmit,
        isAvailable: () => eventsAvailable,
        listen: mockedListen
      }
    });
  });

  it("emits and listens for normalized application MCP policy changes", async () => {
    const unlisten = vi.fn();
    const onPolicyChanged = vi.fn();
    const config = { ...defaultMcpConfig(), enabled: true };
    eventsAvailable = true;
    mockedListen.mockResolvedValue(unlisten);

    const cleanup = await settingsEvents.listenMcpPolicyChanged(onPolicyChanged);
    const listener = mockedListen.mock.calls[0]?.[1];
    await settingsEvents.notifyMcpPolicyChanged(config);
    listener?.({
      payload: {
        config: { ...config, processKey: "must-not-cross-the-event-boundary" }
      }
    } as Parameters<NonNullable<typeof listener>>[0]);
    cleanup();

    expect(mockedListen).toHaveBeenCalledWith("qingyu://settings-mcp-changed", expect.any(Function));
    expect(mockedEmit).toHaveBeenCalledWith("qingyu://settings-mcp-changed", { config });
    expect(onPolicyChanged).toHaveBeenCalledWith(config);
    expect(onPolicyChanged.mock.calls[0]?.[0]).not.toHaveProperty("processKey");
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it("listens for native MCP runtime workspace changes without exposing a root path", async () => {
    const unlisten = vi.fn();
    const onRuntimeChanged = vi.fn();
    eventsAvailable = true;
    mockedListen.mockResolvedValue(unlisten);

    const cleanup = await settingsEvents.listenMcpRuntimeChanged(onRuntimeChanged);
    const listener = mockedListen.mock.calls[0]?.[1];
    listener?.({ payload: { workspaceGeneration: 7 } } as Parameters<NonNullable<typeof listener>>[0]);
    cleanup();

    expect(mockedListen).toHaveBeenCalledWith("qingyu://mcp-runtime-changed", expect.any(Function));
    expect(onRuntimeChanged).toHaveBeenCalledTimes(1);
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  afterEach(() => {
    resetAppRuntimeForTests();
  });

  it("does not expose application-global sync events", () => {
    expect(Object.keys(settingsEvents)).not.toContain("listenAppSyncSettingsChanged");
    expect(Object.keys(settingsEvents)).not.toContain("notifyAppSyncSettingsChanged");
  });

  it("does nothing outside the Tauri runtime", async () => {
    const cleanup = await listenAppThemeChanged(vi.fn());

    await notifyAppThemeChanged({
      appearanceMode: "dark",
      darkTheme: "dark",
      lightTheme: "light"
    });
    cleanup();

    expect(mockedListen).not.toHaveBeenCalled();
    expect(mockedEmit).not.toHaveBeenCalled();
  });

  it("emits and listens for theme changes inside Tauri", async () => {
    const unlisten = vi.fn();
    const onThemeChanged = vi.fn();
    eventsAvailable = true;
    mockedListen.mockResolvedValue(unlisten);

    const cleanup = await listenAppThemeChanged(onThemeChanged);
    const listener = mockedListen.mock.calls[0]?.[1];
    const preferences = {
      appearanceMode: "dark" as const,
      darkTheme: "night" as const,
      lightTheme: "sepia" as const
    };

    await notifyAppThemeChanged(preferences);
    listener?.({ payload: { preferences } } as Parameters<NonNullable<typeof listener>>[0]);
    listener?.({ payload: { theme: "newsprint" } } as Parameters<NonNullable<typeof listener>>[0]);
    listener?.({ payload: { theme: "dracula" } } as Parameters<NonNullable<typeof listener>>[0]);
    cleanup();

    expect(mockedListen).toHaveBeenCalledWith("markra://theme-changed", expect.any(Function));
    expect(mockedEmit).toHaveBeenCalledWith("markra://theme-changed", { preferences });
    expect(onThemeChanged).toHaveBeenCalledWith(preferences);
    expect(onThemeChanged).toHaveBeenCalledWith({
      appearanceMode: "light",
      darkTheme: "dark",
      lightTheme: "newsprint"
    });
    expect(onThemeChanged).toHaveBeenCalledTimes(2);
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it("emits and listens for language changes inside Tauri", async () => {
    const unlisten = vi.fn();
    const onLanguageChanged = vi.fn();
    eventsAvailable = true;
    mockedListen.mockResolvedValue(unlisten);

    const cleanup = await listenAppLanguageChanged(onLanguageChanged);
    const listener = mockedListen.mock.calls[0]?.[1];

    await notifyAppLanguageChanged("fr");
    listener?.({ payload: { language: "fr" } } as Parameters<NonNullable<typeof listener>>[0]);
    listener?.({ payload: { language: "pirate" } } as Parameters<NonNullable<typeof listener>>[0]);
    cleanup();

    expect(mockedListen).toHaveBeenCalledWith("markra://language-changed", expect.any(Function));
    expect(mockedEmit).toHaveBeenCalledWith("markra://language-changed", { language: "fr" });
    expect(onLanguageChanged).toHaveBeenCalledWith("fr");
    expect(onLanguageChanged).toHaveBeenCalledTimes(1);
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it("emits and listens for editor preference changes inside Tauri", async () => {
    const unlisten = vi.fn();
    const onPreferencesChanged = vi.fn();
    eventsAvailable = true;
    mockedListen.mockResolvedValue(unlisten);

    const cleanup = await listenAppEditorPreferencesChanged(onPreferencesChanged);
    const listener = mockedListen.mock.calls[0]?.[1];

    const preferences: EditorPreferences = {
      autoRevealActiveFile: true,
      autoSaveEnabled: true,
      autoSaveIntervalMinutes: 10,
      autoUpdateEnabled: true,
      bodyFontSize: 18,
      clipboardImageFolder: "images",
      contentWidth: "wide" as const,
      contentWidthPx: 1120,
      documentLinksOpen: true,
      documentLinksVisible: false,
      editorFontFamily: {
        family: "Example Serif",
        source: "system"
      },
      extendedSyntax: {
        githubAlerts: true,
        highlight: true
      },
      imageUpload: {
        fileNamePattern: "{name}-{timestamp}"
      },
      lineHeight: 1.8,
      markdownShortcuts: defaultMarkdownShortcuts,
      markdownTemplates: [],
      paragraphSpacingPx: 8,
      restoreWorkspaceOnStartup: false,
      sidebarLayoutMode: "stacked",
      showDocumentTabs: true,
      splitVisualPanePercent: 64,
      tableColumnWidthMode: "even",
      titlebarActions: [
        { id: "theme", visible: true },
        { id: "save", visible: false },
        { id: "sourceMode", visible: true },
        { id: "history", visible: true },
        { id: "viewMode", visible: true }
      ],
      viewMode: "daily",
      viewModeCustomizations: {
        documentLinks: "visible",
        documentTabs: "visible",
        fileList: "visible",
        fileTree: "visible",
        fileTreeButton: "visible",
        openButton: "visible",
        outline: "visible",
        quickCreateButton: "visible",
        recentFolders: "visible",
        sidebarLayout: "visible",
        statusBar: "visible",
        titlebarActions: "visible",
        viewModeToggle: "visible",
        wordCount: "visible"
      },
      showLineNumbers: false,
      showWordCount: false,
      wrapCodeBlocks: false
    };

    await notifyAppEditorPreferencesChanged(preferences);
    listener?.({ payload: { preferences } } as Parameters<NonNullable<typeof listener>>[0]);
    listener?.({
      payload: {
        preferences: {
          bodyFontSize: "nope"
        }
      }
    } as Parameters<NonNullable<typeof listener>>[0]);
    cleanup();

    expect(mockedListen).toHaveBeenCalledWith("markra://editor-preferences-changed", expect.any(Function));
    expect(mockedEmit).toHaveBeenCalledWith("markra://editor-preferences-changed", {
      preferences,
      sourceId: expect.any(String)
    });
    expect(onPreferencesChanged).toHaveBeenCalledWith(preferences);
    expect(onPreferencesChanged).toHaveBeenCalledTimes(1);
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it("ignores editor preference events emitted by the current window", async () => {
    const unlisten = vi.fn();
    const onPreferencesChanged = vi.fn();
    eventsAvailable = true;
    mockedListen.mockResolvedValue(unlisten);

    const cleanup = await listenAppEditorPreferencesChanged(onPreferencesChanged);
    const listener = mockedListen.mock.calls[0]?.[1];
    const preferences: EditorPreferences = {
      ...defaultEditorPreferences,
      imageUpload: {
        fileNamePattern: "h"
      }
    };

    await notifyAppEditorPreferencesChanged(preferences);
    listener?.({ payload: mockedEmit.mock.calls[0]?.[1] } as Parameters<NonNullable<typeof listener>>[0]);
    cleanup();

    expect(onPreferencesChanged).not.toHaveBeenCalled();
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it("emits and listens for export setting changes inside Tauri", async () => {
    const unlisten = vi.fn();
    const onSettingsChanged = vi.fn();
    eventsAvailable = true;
    mockedListen.mockResolvedValue(unlisten);

    const cleanup = await listenAppExportSettingsChanged(onSettingsChanged);
    const listener = mockedListen.mock.calls[0]?.[1];
    const settings = {
      pandocArgs: "--toc",
      pandocPath: "/usr/local/bin/pandoc",
      pdfAuthor: "",
      pdfFooter: "",
      pdfHeader: "",
      pdfHeightMm: 297,
      pdfMarginMm: 24,
      pdfMarginPreset: "custom" as const,
      pdfPageBreakOnH1: false,
      pdfPageSize: "default" as const,
      pdfWidthMm: 210
    };
    configureSyncedExportSettingsRuntime(settings, settings.pandocPath);

    await notifyAppExportSettingsChanged(settings);
    await listener?.({ payload: { settings } } as Parameters<NonNullable<typeof listener>>[0]);
    cleanup();

    expect(mockedListen).toHaveBeenCalledWith("markra://export-settings-changed", expect.any(Function));
    expect(mockedEmit).toHaveBeenCalledWith("markra://export-settings-changed", {
      settings
    });
    expect(onSettingsChanged).toHaveBeenCalledWith(settings);
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it("preserves the local Pandoc path when synchronized portable export settings change", async () => {
    const unlisten = vi.fn();
    const onSettingsChanged = vi.fn();
    const portableSettings = {
      pandocArgs: "--toc",
      pdfAuthor: "QingYu",
      pdfFooter: "",
      pdfHeader: "",
      pdfHeightMm: 297,
      pdfMarginMm: 24,
      pdfMarginPreset: "custom",
      pdfPageBreakOnH1: false,
      pdfPageSize: "default",
      pdfWidthMm: 210
    };
    mockedListen.mockResolvedValue(unlisten);
    configureSyncedExportSettingsRuntime(portableSettings);

    const cleanup = await listenAppExportSettingsChanged(onSettingsChanged);
    const listener = mockedListen.mock.calls[0]?.[1];

    await listener?.({ payload: { settings: portableSettings } } as Parameters<NonNullable<typeof listener>>[0]);
    cleanup();

    expect(onSettingsChanged).toHaveBeenCalledWith({
      ...portableSettings,
      pandocPath: "/opt/homebrew/bin/pandoc"
    });
  });

  it("preserves the local Pandoc path when synchronized portable export settings are deleted", async () => {
    const unlisten = vi.fn();
    const onSettingsChanged = vi.fn();
    mockedListen.mockResolvedValue(unlisten);
    configureSyncedExportSettingsRuntime(undefined);

    const cleanup = await listenAppExportSettingsChanged(onSettingsChanged);
    const listener = mockedListen.mock.calls[0]?.[1];

    await listener?.({ payload: { settings: {} } } as Parameters<NonNullable<typeof listener>>[0]);
    cleanup();

    expect(onSettingsChanged).toHaveBeenCalledWith({
      pandocArgs: "",
      pandocPath: "/opt/homebrew/bin/pandoc",
      pdfAuthor: "",
      pdfFooter: "",
      pdfHeader: "",
      pdfHeightMm: 297,
      pdfMarginMm: 18,
      pdfMarginPreset: "default",
      pdfPageBreakOnH1: false,
      pdfPageSize: "default",
      pdfWidthMm: 210
    });
  });

  it("emits and listens for normalized file ignore setting changes", async () => {
    const unlisten = vi.fn();
    const onSettingsChanged = vi.fn();
    eventsAvailable = true;
    mockedListen.mockResolvedValue(unlisten);

    const cleanup = await listenAppFileIgnoreSettingsChanged(onSettingsChanged);
    const listener = mockedListen.mock.calls[0]?.[1];

    await notifyAppFileIgnoreSettingsChanged({ rules: "generated/\r\n*.tmp" });
    listener?.({
      payload: { settings: { rules: "drafts/\r" } }
    } as Parameters<NonNullable<typeof listener>>[0]);
    cleanup();

    expect(mockedListen).toHaveBeenCalledWith("markra://file-ignore-settings-changed", expect.any(Function));
    expect(mockedEmit).toHaveBeenCalledWith("markra://file-ignore-settings-changed", {
      settings: { rules: "generated/\n*.tmp" }
    });
    expect(onSettingsChanged).toHaveBeenCalledWith({ rules: "drafts/\n" });
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

});
