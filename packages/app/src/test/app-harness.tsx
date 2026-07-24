import { render } from "@testing-library/react";
import { defaultMarkdownShortcuts } from "@markra/editor";
import { parentPathFromPath } from "@markra/shared";
import App from "../App";
import {
  confirmNativeMarkdownFileDelete,
  confirmNativeUnsavedMarkdownDocumentDiscard,
  createNativeMarkdownTreeFile,
  createNativeMarkdownTreeFolder,
  deleteNativeMarkdownTreeFile,
  detectNativePandocPath,
  downloadNativeWebImage,
  getNativeShellCommandStatus,
  installNativeShellCommand,
  closeNativeWindow,
  destroyNativeWindow,
  exitNativeApp,
  hideSettingsWindow,
  importNativeLocalFile,
  markSettingsWindowReady,
  openNativeContainingFolder,
  openNativeLocalImages,
  openNativeLocalFiles,
  openNativeMarkdownAttachment,
  openNativeMarkdownFile,
  openNativeMarkdownFolder,
  openNativeMarkdownFileInNewWindow,
  listenNativeAppExitRequested,
  listenNativeWindowCloseRequested,
  listNativeEditorWindowRestoreStates,
  listenNativeOpenedMarkdownPaths,
  listNativeMarkdownFileHistory,
  moveNativeMarkdownTreeFile,
  readNativeLocalImageFile,
  readNativeMarkdownFile,
  readNativeMarkdownFileHistory,
  readNativeMarkdownTemplateFile,
  resolveNativeMarkdownPath,
  saveNativeClipboardImage,
  saveNativeClipboardAttachment,
  saveNativeHtmlFile,
  saveNativeMarkdownFile,
  saveNativePandocFile,
  saveNativePdfFile,
  searchNativeMarkdownFilesForPath,
  setNativeEditorWindowRestoreState,
  showNativeWindow,
  showNativeAppAbout,
  showNativePandocSetup,
  showNativeMarkdownFileTreeContextMenu,
  uninstallNativeShellCommand,
  installNativeMarkdownFileDrop,
  loadNativeMarkdownFilesForPath,
  listNativeMarkdownFilesForPath,
  takeNativeOpenedMarkdownPaths,
  toggleNativeWindowFullscreen,
  toggleNativeWindowMaximized,
  renameNativeMarkdownTreeFile,
  watchNativeMarkdownFile,
  writeNativeMarkdownTemplateFile,
  watchNativeMarkdownTree
} from "../lib/tauri";
import {
  installNativeApplicationMenu,
  installNativeEditorContextMenu,
  listenNativeApplicationMenuCommands
} from "../lib/tauri";
import { openNativeExternalUrl, openSettingsWindow } from "../lib/tauri";
import { checkNativeAppUpdate } from "../lib/tauri/updater";
import {
  clearStoredRecentMarkdownFiles,
  consumeWelcomeDocumentState,
  getStoredCustomThemeCss,
  getStoredEditorPreferences,
  getStoredExportSettings,
  getStoredFileIgnoreSettings,
  getStoredLanguage,
  getStoredRecentMarkdownFiles,
  getStoredTheme,
  getStoredThemePreferences,
  getStoredWorkspaceState,
  removeStoredRecentMarkdownFile,
  resetWelcomeDocumentState,
  saveStoredCustomThemeCss,
  saveStoredEditorPreferences,
  saveStoredExportSettings,
  saveStoredFileIgnoreSettings,
  saveStoredLanguage,
  saveStoredRecentMarkdownFile,
  saveStoredTheme,
  saveStoredThemePreferences,
  saveStoredWorkspaceState,
  type RecentMarkdownFile
} from "../lib/settings/app-settings";
import {
  listenAppCustomThemeCssChanged,
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
} from "../lib/settings/settings-events";
import {
  loadPrimaryWorkspaceState,
  savePrimaryWorkspaceState
} from "../lib/settings/local-state";
import { resolveDesktopOsVersion, resolveDesktopPlatform } from "../lib/platform";
import { defaultMcpConfig, type McpSettingsSnapshot } from "../lib/mcp";
import {
  configureAppRuntime,
  createDefaultAppRuntime,
  type AppMcpRuntime
} from "../runtime";
import type {
  PrimaryWorkspaceController,
  PrimaryWorkspaceStatus
} from "../hooks/usePrimaryWorkspace";

const primaryWorkspaceHarnessState = vi.hoisted(() => ({
  desktopController: null as PrimaryWorkspaceController | null
}));

function createApplicationMcpRuntime(): AppMcpRuntime {
  let revision = 1;
  let workspaceGeneration = 0;
  let snapshot: McpSettingsSnapshot = {
    clientCommand: "/Applications/QingYu.app/Contents/MacOS/qingyu-mcp",
    config: defaultMcpConfig(),
    endpoint: "local-ipc",
    health: { state: "disabled", endpoint: null, errorCode: null },
    revision: `application-mcp-${revision}`,
    workspace: null
  };

  const updateSnapshot = (config: McpSettingsSnapshot["config"]): McpSettingsSnapshot => {
    revision += 1;
    snapshot = {
      ...snapshot,
      config,
      health: config.enabled
        ? { state: "running", endpoint: "local-ipc", errorCode: null }
        : { state: "disabled", endpoint: null, errorCode: null },
      revision: `application-mcp-${revision}`
    };
    return snapshot;
  };

  return {
    clearAuditEntries: vi.fn(async () => undefined),
    getHealth: vi.fn(async () => snapshot.health),
    getSettings: vi.fn(async () => snapshot),
    listAuditEntries: vi.fn(async () => []),
    localServiceAvailable: true,
    policyAvailable: true,
    setPrimaryWorkspace: vi.fn(async ({ primaryRoot }) => {
      workspaceGeneration += 1;
      const leafName = primaryRoot?.split("/").filter(Boolean).at(-1) ?? "";
      snapshot = {
        ...snapshot,
        workspace: primaryRoot
          ? {
              available: true,
              displayName: leafName,
              leafName,
              workspaceGeneration,
              workspaceId: `primary-workspace-${workspaceGeneration}`
            }
          : null
      };
      return snapshot;
    }),
    updateSettings: vi.fn(async ({ config, expectedRevision }) => {
      if (expectedRevision !== snapshot.revision) {
        throw new Error("revision-conflict: application MCP policy changed");
      }
      return updateSnapshot(config);
    })
  };
}

vi.mock("../lib/tauri", () => ({
  confirmNativeMarkdownFileDelete: vi.fn(),
  confirmNativeUnsavedMarkdownDocumentDiscard: vi.fn(),
  createNativeMarkdownTreeFile: vi.fn(),
  createNativeMarkdownTreeFolder: vi.fn(),
  deleteNativeMarkdownTreeFile: vi.fn(),
  detectNativePandocPath: vi.fn(),
  downloadNativeWebImage: vi.fn(),
  getNativeShellCommandStatus: vi.fn(),
  installNativeShellCommand: vi.fn(),
  installNativeMarkdownFileDrop: vi.fn(),
  importNativeLocalFile: vi.fn(),
  openNativeContainingFolder: vi.fn(),
  openNativeLocalImages: vi.fn(),
  openNativeLocalFiles: vi.fn(),
  openNativeMarkdownAttachment: vi.fn(),
  openNativeMarkdownFile: vi.fn(),
  openNativeMarkdownFolder: vi.fn(),
  openNativeMarkdownFileInNewWindow: vi.fn(),
  listenNativeOpenedMarkdownPaths: vi.fn(),
  listNativeMarkdownFileHistory: vi.fn(),
  moveNativeMarkdownTreeFile: vi.fn(),
  readNativeLocalImageFile: vi.fn(),
  readNativeMarkdownFile: vi.fn(),
  readNativeMarkdownFileHistory: vi.fn(),
  readNativeMarkdownTemplateFile: vi.fn(),
  resolveNativeMarkdownPath: vi.fn(),
  renameNativeMarkdownTreeFile: vi.fn(),
  saveNativeClipboardImage: vi.fn(),
  saveNativeClipboardAttachment: vi.fn(),
  saveNativeHtmlFile: vi.fn(),
  saveNativeMarkdownFile: vi.fn(),
  saveNativePandocFile: vi.fn(),
  saveNativePdfFile: vi.fn(),
  searchNativeMarkdownFilesForPath: vi.fn(),
  setNativeEditorWindowRestoreState: vi.fn(),
  showNativePandocSetup: vi.fn(),
  showNativeMarkdownFileTreeContextMenu: vi.fn(),
  uninstallNativeShellCommand: vi.fn(),
  watchNativeMarkdownFile: vi.fn(),
  writeNativeMarkdownTemplateFile: vi.fn(),
  watchNativeMarkdownTree: vi.fn(),
  loadNativeMarkdownFilesForPath: vi.fn(),
  listNativeMarkdownFilesForPath: vi.fn(),
  takeNativeOpenedMarkdownPaths: vi.fn(),
  installNativeApplicationMenu: vi.fn(),
  installNativeEditorContextMenu: vi.fn(),
  listenNativeApplicationMenuCommands: vi.fn(),
  openNativeExternalUrl: vi.fn(),
  closeNativeWindow: vi.fn(),
  destroyNativeWindow: vi.fn(),
  hideSettingsWindow: vi.fn(),
  markSettingsWindowReady: vi.fn(),
  exitNativeApp: vi.fn(),
  listenNativeAppExitRequested: vi.fn(),
  listenNativeWindowCloseRequested: vi.fn(),
  listNativeEditorWindowRestoreStates: vi.fn(),
  openSettingsWindow: vi.fn(),
  setNativeWindowTitle: vi.fn(),
  showNativeWindow: vi.fn(),
  showNativeAppAbout: vi.fn(),
  toggleNativeWindowFullscreen: vi.fn(),
  toggleNativeWindowMaximized: vi.fn()
}));

vi.mock("../lib/tauri/updater", () => ({
  checkNativeAppUpdate: vi.fn()
}));

vi.mock("../lib/settings/local-state", () => ({
  getStoredRecentNotebooks: vi.fn(async () => []),
  isValidManagedNotebookName: vi.fn(() => true),
  loadPrimaryWorkspaceState: vi.fn(),
  removeStoredRecentNotebook: vi.fn(async () => []),
  saveStoredRecentNotebook: vi.fn(async (notebook) => [notebook]),
  savePrimaryWorkspaceState: vi.fn()
}));

vi.mock("../hooks/usePrimaryWorkspace", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../hooks/usePrimaryWorkspace")>();
  const deferredDesktopController: PrimaryWorkspaceController = {
    canChooseDesktopRoot: true,
    commitDesktopRoot: async () => null,
    commitManagedRoot: async () => null,
    deferDesktopSetup: async () => undefined,
    error: null,
    managedName: null,
    resetOnboarding: async () => undefined,
    retry: async () => undefined,
    root: null,
    status: "deferred",
    workspaceRoot: null
  };

  return {
    ...actual,
    usePrimaryWorkspace: (options: { trueMobile: boolean }) => {
      if (options.trueMobile) return actual.usePrimaryWorkspace(options);
      return primaryWorkspaceHarnessState.desktopController ?? deferredDesktopController;
    }
  };
});

vi.mock("../lib/platform", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/platform")>();

  return {
    ...actual,
    resolveDesktopOsVersion: vi.fn(() => null),
    resolveDesktopPlatform: vi.fn(() => "macos")
  };
});

vi.mock("../lib/settings/app-settings", () => ({
  consumeWelcomeDocumentState: vi.fn(),
  defaultStoredFileTreeSort: {
    direction: "ascending",
    key: "name"
  },
  defaultSplitVisualPanePercent: 50,
  splitVisualPanePercentMin: 25,
  splitVisualPanePercentMax: 75,
  editorParagraphSpacingPxMin: 0,
  editorParagraphSpacingPxMax: 32,
  defaultEditorPreferences: {
    autoUpdateEnabled: true,
    bodyFontSize: 16,
    clipboardImageFolder: "assets",
    contentWidth: "default",
    contentWidthPx: null,
    documentLinksOpen: true,
    documentLinksVisible: false,
    editorFontFamily: { family: null, source: "theme" },
    extendedSyntax: {
      githubAlerts: true,
      highlight: true
    },
    imageUpload: {
      fileNamePattern: "pasted-image-{timestamp}"
    },
    lineHeight: 1.65,
    markdownShortcuts: {
      bold: "Mod+B",
      bulletList: "Mod+Shift+8",
      codeBlock: "Mod+Alt+C",
      heading1: "Mod+Alt+1",
      heading2: "Mod+Alt+2",
      heading3: "Mod+Alt+3",
      inlineCode: "Mod+E",
      italic: "Mod+I",
      orderedList: "Mod+Shift+7",
      paragraph: "Mod+Alt+0",
      quote: "Mod+Shift+B",
      strikethrough: "Mod+Shift+X"
    },
    paragraphSpacingPx: 8,
    restoreWorkspaceOnStartup: true,
    showDocumentTabs: true,
    splitVisualPanePercent: 50,
    tableColumnWidthMode: "auto",
    titlebarActions: [
      { id: "viewMode", visible: true },
      { id: "sourceMode", visible: true },
      { id: "history", visible: true },
      { id: "save", visible: true },
      { id: "theme", visible: true }
    ],
    viewMode: "daily",
    viewModeCustomizations: {
      documentTabs: "visible",
      fileList: "visible",
      fileTree: "visible",
      fileTreeButton: "visible",
      openButton: "visible",
      outline: "visible",
      quickCreateButton: "visible",
      recentFolders: "visible",
      statusBar: "visible",
      titlebarActions: "visible",
      viewModeToggle: "visible"
    },
    showLineNumbers: false,
    showWordCount: true,
    wrapCodeBlocks: true
  },
  appThemeOptions: [
    "system",
    "light",
    "dark",
    "github",
    "github-dark",
    "one-dark",
    "one-light",
    "one-dark-pro",
    "gothic",
    "newsprint",
    "night",
    "pixyll",
    "whitey",
    "sepia",
    "solarized-light",
    "solarized-dark",
    "nord",
    "catppuccin-latte",
    "catppuccin-mocha",
    "academic",
    "minimal",
    "custom"
  ],
  appAppearanceModeOptions: ["system", "light", "dark"],
  editorThemeOptions: [
    "light",
    "dark",
    "github",
    "github-dark",
    "one-dark",
    "one-light",
    "one-dark-pro",
    "gothic",
    "newsprint",
    "night",
    "pixyll",
    "whitey",
    "sepia",
    "solarized-light",
    "solarized-dark",
    "nord",
    "catppuccin-latte",
    "catppuccin-mocha",
    "academic",
    "minimal",
    "custom"
  ],
  lightEditorThemeOptions: [
    "light",
    "github",
    "one-light",
    "gothic",
    "newsprint",
    "pixyll",
    "whitey",
    "sepia",
    "solarized-light",
    "catppuccin-latte",
    "academic",
    "minimal",
    "custom"
  ],
  darkEditorThemeOptions: [
    "dark",
    "github-dark",
    "one-dark",
    "one-dark-pro",
    "night",
    "solarized-dark",
    "nord",
    "catppuccin-mocha",
    "custom"
  ],
  defaultAppThemePreferences: {
    appearanceMode: "system",
    darkTheme: "dark",
    lightTheme: "light"
  },
  approveThemeFingerprint: vi.fn(async () => undefined),
  forgetApprovedThemeFingerprint: vi.fn(async () => undefined),
  getApprovedThemeFingerprint: vi.fn(async () => null),
  isThemeId: vi.fn((value) => typeof value === "string" && /^[a-z0-9][a-z0-9-]{0,63}$/u.test(value)),
  normalizeAppThemePreferences: vi.fn((preferences) => {
    const value = typeof preferences === "object" && preferences !== null
      ? preferences as Record<string, unknown>
      : {};
    const appearanceMode = ["system", "light", "dark"].includes(String(value.appearanceMode))
      ? value.appearanceMode
      : "system";
    const lightTheme = [
      "light",
      "github",
      "one-light",
      "gothic",
      "newsprint",
      "pixyll",
      "whitey",
      "sepia",
      "solarized-light",
      "catppuccin-latte",
      "academic",
      "minimal",
      "custom"
    ].includes(String(value.lightTheme)) ? value.lightTheme : "light";
    const darkTheme = [
      "dark",
      "github-dark",
      "one-dark",
      "one-dark-pro",
      "night",
      "solarized-dark",
      "nord",
      "catppuccin-mocha",
      "custom"
    ].includes(String(value.darkTheme)) ? value.darkTheme : "dark";

    return {
      appearanceMode,
      darkTheme,
      lightTheme
    };
  }),
  resolveAppAppearanceTheme: vi.fn((theme, systemTheme) => {
    const resolvedTheme = theme === "system" ? systemTheme : theme;
    return [
      "dark",
      "github-dark",
      "night",
      "one-dark",
      "one-dark-pro",
      "solarized-dark",
      "nord",
      "catppuccin-mocha"
    ].includes(resolvedTheme) ? "dark" : "light";
  }),
  resolveAppEditorTheme: vi.fn((theme, systemTheme) => theme === "system" ? systemTheme : theme),
  resolveAppThemePreferencesAppearance: vi.fn((preferences, systemTheme) =>
    preferences.appearanceMode === "system" ? systemTheme : preferences.appearanceMode
  ),
  resolveAppThemePreferencesEditorTheme: vi.fn((preferences, systemTheme) => {
    const appearance = preferences.appearanceMode === "system" ? systemTheme : preferences.appearanceMode;

    return appearance === "dark" ? preferences.darkTheme : preferences.lightTheme;
  }),
  defaultTitlebarActions: [
    { id: "viewMode", visible: true },
    { id: "sourceMode", visible: true },
    { id: "history", visible: true },
    { id: "save", visible: true },
    { id: "theme", visible: true }
  ],
  defaultExportSettings: {
    pandocArgs: "",
    pandocPath: "",
    pdfAuthor: "",
    pdfFooter: "",
    pdfHeader: "",
    pdfHeightMm: 297,
    pdfMarginMm: 18,
    pdfMarginPreset: "default",
    pdfPageBreakOnH1: false,
    pdfPageSize: "default",
    pdfWidthMm: 210
  },
  defaultFileIgnoreSettings: {
    rules: ""
  },
  getStoredCustomThemeCss: vi.fn(),
  getStoredEditorPreferences: vi.fn(),
  getStoredExportSettings: vi.fn(),
  getStoredFileIgnoreSettings: vi.fn(),
  getStoredFileTreeSortByWorkspace: vi.fn(async () => ({})),
  getStoredLanguage: vi.fn(),
  getStoredRecentMarkdownFiles: vi.fn(),
  getStoredTheme: vi.fn(),
  getStoredThemePreferences: vi.fn(),
  getStoredWorkspaceState: vi.fn(),
  normalizeEditorPreferences: vi.fn((preferences) => ({
    autoUpdateEnabled: true,
    bodyFontSize: 16,
    clipboardImageFolder: "assets",
    contentWidth: "default",
    contentWidthPx: null,
    editorFontFamily: { family: null, source: "theme" },
    extendedSyntax: {
      githubAlerts: true,
      highlight: true
    },
    imageUpload: {
      fileNamePattern: "pasted-image-{timestamp}"
    },
    lineHeight: 1.65,
    markdownShortcuts: {
      bold: "Mod+B",
      bulletList: "Mod+Shift+8",
      codeBlock: "Mod+Alt+C",
      heading1: "Mod+Alt+1",
      heading2: "Mod+Alt+2",
      heading3: "Mod+Alt+3",
      inlineCode: "Mod+E",
      italic: "Mod+I",
      orderedList: "Mod+Shift+7",
      paragraph: "Mod+Alt+0",
      quote: "Mod+Shift+B",
      strikethrough: "Mod+Shift+X"
    },
    restoreWorkspaceOnStartup: true,
    showDocumentTabs: true,
    splitVisualPanePercent: 50,
    titlebarActions: [
      { id: "viewMode", visible: true },
      { id: "sourceMode", visible: true },
      { id: "history", visible: true },
      { id: "save", visible: true },
      { id: "theme", visible: true }
    ],
    viewMode: preferences?.viewMode ?? "daily",
    viewModeCustomizations: preferences?.viewModeCustomizations ?? {
      documentTabs: "visible",
      fileTree: "visible",
      fileTreeButton: "visible",
      openButton: "visible",
      quickCreateButton: "visible",
      statusBar: "visible",
      titlebarActions: "visible",
      viewModeToggle: "visible"
    },
    showLineNumbers: preferences?.showLineNumbers ?? false,
    showWordCount: true,
    ...preferences,
    wrapCodeBlocks: preferences?.wrapCodeBlocks ?? true
  })),
  normalizeFileIgnoreSettings: vi.fn((value: unknown) => {
    const rules = typeof value === "object" && value !== null && "rules" in value
      ? (value as { rules?: unknown }).rules
      : "";

    return {
      rules: typeof rules === "string"
        ? rules.replace(/\r\n?/gu, "\n").slice(0, 50_000)
        : ""
    };
  }),
  normalizeStoredFileTreeSort: vi.fn((sort) => {
    const value = typeof sort === "object" && sort !== null
      ? sort as Record<string, unknown>
      : {};
    const key = ["createdAt", "modifiedAt", "name"].includes(String(value.key)) ? value.key : "name";
    const direction = ["ascending", "descending"].includes(String(value.direction)) ? value.direction : "ascending";

    return { direction, key };
  }),
  normalizeTitlebarActions: vi.fn((actions) => Array.isArray(actions) ? actions : [
    { id: "viewMode", visible: true },
    { id: "sourceMode", visible: true },
    { id: "save", visible: true },
    { id: "theme", visible: true }
  ]),
  reorderTitlebarActions: vi.fn((actions, draggedId, targetId) => {
    const normalized = Array.isArray(actions) ? actions : [
      { id: "viewMode", visible: true },
      { id: "sourceMode", visible: true },
      { id: "save", visible: true },
      { id: "theme", visible: true }
    ];
    const fromIndex = normalized.findIndex((action) => action.id === draggedId);
    const toIndex = normalized.findIndex((action) => action.id === targetId);
    if (fromIndex < 0 || toIndex < 0 || fromIndex === toIndex) return normalized;

    const draggedAction = normalized[fromIndex];
    const nextActions = normalized.filter((action) => action.id !== draggedId);

    nextActions.splice(toIndex, 0, draggedAction);

    return nextActions;
  }),
  normalizeExportSettings: vi.fn((settings) => ({
    pandocArgs: "",
    pandocPath: "",
    pdfAuthor: "",
    pdfFooter: "",
    pdfHeader: "",
    pdfHeightMm: 297,
    pdfMarginMm: 18,
    pdfMarginPreset: "default",
    pdfPageBreakOnH1: false,
    pdfPageSize: "default",
    pdfWidthMm: 210,
    ...settings
  })),
  prependRecentMarkdownFile: vi.fn((files: RecentMarkdownFile[], file: RecentMarkdownFile) => [
    file,
    ...files.filter((item) => item.path !== file.path)
  ].slice(0, 10)),
  resetWelcomeDocumentState: vi.fn(),
  removeStoredRecentMarkdownFile: vi.fn(),
  saveStoredCustomThemeCss: vi.fn(),
  saveStoredEditorPreferences: vi.fn(),
  saveStoredExportSettings: vi.fn(),
  saveStoredFileIgnoreSettings: vi.fn(),
  saveStoredFileTreeSortForWorkspace: vi.fn(async () => {}),
  saveStoredLanguage: vi.fn(),
  saveStoredRecentMarkdownFile: vi.fn(),
  saveStoredTheme: vi.fn(),
  saveStoredThemePreferences: vi.fn(),
  saveStoredWorkspaceState: vi.fn(),
  clearStoredRecentMarkdownFiles: vi.fn()
}));

vi.mock("../lib/settings/settings-events", () => ({
  listenAppCustomThemeCssChanged: vi.fn(),
  listenAppEditorPreferencesChanged: vi.fn(),
  listenAppExportSettingsChanged: vi.fn(),
  listenAppFileIgnoreSettingsChanged: vi.fn(),
  listenAppLanguageChanged: vi.fn(),
  listenAppThemeChanged: vi.fn(),
  listenThemeCatalogChanged: vi.fn(async () => () => undefined),
  listenMcpPolicyChanged: vi.fn(async () => () => undefined),
  listenMcpRuntimeChanged: vi.fn(async () => () => undefined),
  notifyAppCustomThemeCssChanged: vi.fn(),
  notifyAppEditorPreferencesChanged: vi.fn(),
  notifyAppExportSettingsChanged: vi.fn(),
  notifyAppFileIgnoreSettingsChanged: vi.fn(),
  notifyAppLanguageChanged: vi.fn(),
  notifyAppThemeChanged: vi.fn(),
  notifyThemeCatalogChanged: vi.fn(async () => undefined)
}));

export const mockedOpenNativeMarkdownFolder = vi.mocked(openNativeMarkdownFolder);
export const mockedOpenNativeMarkdownFile = vi.mocked(openNativeMarkdownFile);
export const mockedLoadPrimaryWorkspaceState = vi.mocked(loadPrimaryWorkspaceState);
export const mockedSavePrimaryWorkspaceState = vi.mocked(savePrimaryWorkspaceState);
export const mockedOpenNativeContainingFolder = vi.mocked(openNativeContainingFolder);
export const mockedOpenNativeLocalImages = vi.mocked(openNativeLocalImages);
export const mockedOpenNativeLocalFiles = vi.mocked(openNativeLocalFiles);
export const mockedImportNativeLocalFile = vi.mocked(importNativeLocalFile);
export const mockedOpenNativeMarkdownAttachment = vi.mocked(openNativeMarkdownAttachment);
export const mockedConfirmNativeMarkdownFileDelete = vi.mocked(confirmNativeMarkdownFileDelete);
export const mockedConfirmNativeUnsavedMarkdownDocumentDiscard = vi.mocked(confirmNativeUnsavedMarkdownDocumentDiscard);
export const mockedCreateNativeMarkdownTreeFile = vi.mocked(createNativeMarkdownTreeFile);
export const mockedCreateNativeMarkdownTreeFolder = vi.mocked(createNativeMarkdownTreeFolder);
export const mockedDeleteNativeMarkdownTreeFile = vi.mocked(deleteNativeMarkdownTreeFile);
export const mockedDetectNativePandocPath = vi.mocked(detectNativePandocPath);
export const mockedDownloadNativeWebImage = vi.mocked(downloadNativeWebImage);
export const mockedGetNativeShellCommandStatus = vi.mocked(getNativeShellCommandStatus);
export const mockedInstallNativeShellCommand = vi.mocked(installNativeShellCommand);
export const mockedOpenNativeMarkdownFileInNewWindow = vi.mocked(openNativeMarkdownFileInNewWindow);
export const mockedListenNativeOpenedMarkdownPaths = vi.mocked(listenNativeOpenedMarkdownPaths);
export const mockedReadNativeLocalImageFile = vi.mocked(readNativeLocalImageFile);
export const mockedReadNativeMarkdownFile = vi.mocked(readNativeMarkdownFile);
export const mockedReadNativeMarkdownFileHistory = vi.mocked(readNativeMarkdownFileHistory);
export const mockedReadNativeMarkdownTemplateFile = vi.mocked(readNativeMarkdownTemplateFile);
export const mockedResolveNativeMarkdownPath = vi.mocked(resolveNativeMarkdownPath);
export const mockedSaveNativeClipboardImage = vi.mocked(saveNativeClipboardImage);
export const mockedSaveNativeClipboardAttachment = vi.mocked(saveNativeClipboardAttachment);
export const mockedSaveNativeHtmlFile = vi.mocked(saveNativeHtmlFile);
export const mockedSaveNativeMarkdownFile = vi.mocked(saveNativeMarkdownFile);
export const mockedSaveNativePandocFile = vi.mocked(saveNativePandocFile);
export const mockedSaveNativePdfFile = vi.mocked(saveNativePdfFile);
export const mockedSearchNativeMarkdownFilesForPath = vi.mocked(searchNativeMarkdownFilesForPath);
export const mockedSetNativeEditorWindowRestoreState = vi.mocked(setNativeEditorWindowRestoreState);
export const mockedShowNativeWindow = vi.mocked(showNativeWindow);
export const mockedHideSettingsWindow = vi.mocked(hideSettingsWindow);
export const mockedMarkSettingsWindowReady = vi.mocked(markSettingsWindowReady);
export const mockedShowNativeAppAbout = vi.mocked(showNativeAppAbout);
export const mockedShowNativePandocSetup = vi.mocked(showNativePandocSetup);
export const mockedShowNativeMarkdownFileTreeContextMenu = vi.mocked(showNativeMarkdownFileTreeContextMenu);
export const mockedUninstallNativeShellCommand = vi.mocked(uninstallNativeShellCommand);
export const mockedInstallNativeMarkdownFileDrop = vi.mocked(installNativeMarkdownFileDrop);
export const mockedListNativeMarkdownFileHistory = vi.mocked(listNativeMarkdownFileHistory);
export const mockedMoveNativeMarkdownTreeFile = vi.mocked(moveNativeMarkdownTreeFile);
export const mockedLoadNativeMarkdownFilesForPath = vi.mocked(loadNativeMarkdownFilesForPath);
export const mockedListNativeMarkdownFilesForPath = vi.mocked(listNativeMarkdownFilesForPath);
export const mockedTakeNativeOpenedMarkdownPaths = vi.mocked(takeNativeOpenedMarkdownPaths);
export const mockedRenameNativeMarkdownTreeFile = vi.mocked(renameNativeMarkdownTreeFile);
export const mockedWatchNativeMarkdownFile = vi.mocked(watchNativeMarkdownFile);
export const mockedWriteNativeMarkdownTemplateFile = vi.mocked(writeNativeMarkdownTemplateFile);
export const mockedWatchNativeMarkdownTree = vi.mocked(watchNativeMarkdownTree);
export const mockedInstallNativeApplicationMenu = vi.mocked(installNativeApplicationMenu);
export const mockedInstallNativeEditorContextMenu = vi.mocked(installNativeEditorContextMenu);
export const mockedListenNativeApplicationMenuCommands = vi.mocked(listenNativeApplicationMenuCommands);
export const mockedOpenSettingsWindow = vi.mocked(openSettingsWindow);
export const mockedOpenNativeExternalUrl = vi.mocked(openNativeExternalUrl);
export const mockedCloseNativeWindow = vi.mocked(closeNativeWindow);
export const mockedDestroyNativeWindow = vi.mocked(destroyNativeWindow);
export const mockedToggleNativeWindowFullscreen = vi.mocked(toggleNativeWindowFullscreen);
export const mockedExitNativeApp = vi.mocked(exitNativeApp);
export const mockedListenNativeAppExitRequested = vi.mocked(listenNativeAppExitRequested);
export const mockedListenNativeWindowCloseRequested = vi.mocked(listenNativeWindowCloseRequested);
export const mockedListNativeEditorWindowRestoreStates = vi.mocked(listNativeEditorWindowRestoreStates);
export const mockedCheckNativeAppUpdate = vi.mocked(checkNativeAppUpdate);
export const mockedResolveDesktopOsVersion = vi.mocked(resolveDesktopOsVersion);
export const mockedResolveDesktopPlatform = vi.mocked(resolveDesktopPlatform);
export const mockedConsumeWelcomeDocumentState = vi.mocked(consumeWelcomeDocumentState);
export const mockedGetStoredCustomThemeCss = vi.mocked(getStoredCustomThemeCss);
export const mockedGetStoredEditorPreferences = vi.mocked(getStoredEditorPreferences);
export const mockedGetStoredExportSettings = vi.mocked(getStoredExportSettings);
export const mockedGetStoredFileIgnoreSettings = vi.mocked(getStoredFileIgnoreSettings);
export const mockedGetStoredLanguage = vi.mocked(getStoredLanguage);
export const mockedGetStoredRecentMarkdownFiles = vi.mocked(getStoredRecentMarkdownFiles);
export const mockedGetStoredTheme = vi.mocked(getStoredTheme);
export const mockedGetStoredThemePreferences = vi.mocked(getStoredThemePreferences);
export const mockedGetStoredWorkspaceState = vi.mocked(getStoredWorkspaceState);
export const mockedClearStoredRecentMarkdownFiles = vi.mocked(clearStoredRecentMarkdownFiles);
export const mockedRemoveStoredRecentMarkdownFile = vi.mocked(removeStoredRecentMarkdownFile);
export const mockedResetWelcomeDocumentState = vi.mocked(resetWelcomeDocumentState);
export const mockedSaveStoredCustomThemeCss = vi.mocked(saveStoredCustomThemeCss);
export const mockedSaveStoredEditorPreferences = vi.mocked(saveStoredEditorPreferences);
export const mockedSaveStoredExportSettings = vi.mocked(saveStoredExportSettings);
export const mockedSaveStoredFileIgnoreSettings = vi.mocked(saveStoredFileIgnoreSettings);
export const mockedSaveStoredLanguage = vi.mocked(saveStoredLanguage);
export const mockedSaveStoredRecentMarkdownFile = vi.mocked(saveStoredRecentMarkdownFile);
export const mockedSaveStoredTheme = vi.mocked(saveStoredTheme);
export const mockedSaveStoredThemePreferences = vi.mocked(saveStoredThemePreferences);
export const mockedSaveStoredWorkspaceState = vi.mocked(saveStoredWorkspaceState);

let configuredWorkspaceState: {
  filePath?: string | null;
  folderPath?: string | null;
  openFilePaths?: string[];
} = {};
let externalBlankPickerRequested = false;
const trackWorkspaceState = mockedGetStoredWorkspaceState.mockResolvedValue.bind(mockedGetStoredWorkspaceState);
mockedGetStoredWorkspaceState.mockResolvedValue = ((workspaceState) => {
  configuredWorkspaceState = workspaceState;
  return trackWorkspaceState(workspaceState);
}) as typeof mockedGetStoredWorkspaceState.mockResolvedValue;
export const mockedListenAppCustomThemeCssChanged = vi.mocked(listenAppCustomThemeCssChanged);
export const mockedListenAppEditorPreferencesChanged = vi.mocked(listenAppEditorPreferencesChanged);
export const mockedListenAppExportSettingsChanged = vi.mocked(listenAppExportSettingsChanged);
export const mockedListenAppFileIgnoreSettingsChanged = vi.mocked(listenAppFileIgnoreSettingsChanged);
export const mockedListenAppLanguageChanged = vi.mocked(listenAppLanguageChanged);
export const mockedListenAppThemeChanged = vi.mocked(listenAppThemeChanged);
export const mockedNotifyAppEditorPreferencesChanged = vi.mocked(notifyAppEditorPreferencesChanged);
export const mockedNotifyAppExportSettingsChanged = vi.mocked(notifyAppExportSettingsChanged);
export const mockedNotifyAppFileIgnoreSettingsChanged = vi.mocked(notifyAppFileIgnoreSettingsChanged);
export const mockedNotifyAppLanguageChanged = vi.mocked(notifyAppLanguageChanged);
export const mockedNotifyAppThemeChanged = vi.mocked(notifyAppThemeChanged);

export const mockNativePath = "/mock-files/native.md";
export const mockDroppedPath = "/mock-files/dropped.md";
export const mockFolderPath = "/mock-files/vault";
export const mockUntitledPath = "/mock-files/Untitled.md";

export const appHarnessResourceThemeDescriptor = {
  appearance: "dark" as const,
  author: "Jens & Pyrmont",
  fileName: null,
  fingerprint: "c".repeat(64),
  id: "drake-ayu",
  name: "Drake Ayu",
  preview: {
    accent: "#ffcc66",
    background: "#0f1419",
    panel: "#14191f",
    text: "#e6e1cf"
  },
  source: "third-party" as const,
  storageKind: "resourceDirectory" as const
};

const appHarnessThemeDescriptors = [
  ["github", "GitHub", "light"],
  ["github-dark", "GitHub Dark", "dark"],
  ["one-dark", "One Dark", "dark"],
  ["one-light", "One Light", "light"],
  ["one-dark-pro", "One Dark Pro", "dark"],
  ["gothic", "Gothic", "light"],
  ["newsprint", "Newsprint", "light"],
  ["night", "Night", "dark"],
  ["pixyll", "Pixyll", "light"],
  ["whitey", "Whitey", "light"],
  ["sepia", "Sepia", "light"],
  ["solarized-light", "Solarized Light", "light"],
  ["solarized-dark", "Solarized Dark", "dark"],
  ["nord", "Nord", "dark"],
  ["catppuccin-latte", "Catppuccin Latte", "light"],
  ["catppuccin-mocha", "Catppuccin Mocha", "dark"],
  ["academic", "Academic", "light"],
  ["minimal", "Minimal", "light"]
].map(([id, name, appearance]) => ({
  appearance: appearance as "dark" | "light",
  fileName: `${id}.css`,
  fingerprint: (appearance === "dark" ? "d" : "a").repeat(64),
  id,
  name,
  preview: {
    accent: appearance === "dark" ? "#88c0d0" : "#0969da",
    background: appearance === "dark" ? "#1e1e1e" : "#ffffff",
    panel: appearance === "dark" ? "#252526" : "#f6f8fa",
    text: appearance === "dark" ? "#f0f6fc" : "#1f2328"
  },
  source: "third-party" as const,
  storageKind: "inlineCss" as const
}));

export function mockSystemColorScheme(initiallyDark: boolean) {
  let matches = initiallyDark;
  const listeners = new Set<(event: MediaQueryListEvent) => unknown>();
  const mediaQueryList = {
    get matches() {
      return matches;
    },
    media: "(prefers-color-scheme: dark)",
    onchange: null,
    addEventListener: vi.fn((_event: "change", listener: (event: MediaQueryListEvent) => unknown) => {
      listeners.add(listener);
    }),
    removeEventListener: vi.fn((_event: "change", listener: (event: MediaQueryListEvent) => unknown) => {
      listeners.delete(listener);
    }),
    addListener: vi.fn((listener: (event: MediaQueryListEvent) => unknown) => {
      listeners.add(listener);
    }),
    removeListener: vi.fn((listener: (event: MediaQueryListEvent) => unknown) => {
      listeners.delete(listener);
    }),
    dispatchEvent: vi.fn()
  } as unknown as MediaQueryList;

  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    value: vi.fn(() => mediaQueryList)
  });

  return {
    setSystemDark(nextMatches: boolean) {
      matches = nextMatches;
      const event = { matches: nextMatches, media: "(prefers-color-scheme: dark)" } as MediaQueryListEvent;
      listeners.forEach((listener) => listener(event));
    }
  };
}

export function mockOpenMarkdownFile(file: { content: string; name: string; path: string }) {
  externalBlankPickerRequested = true;
  mockedOpenNativeMarkdownFile.mockResolvedValue(file);
}

export function mockPrimaryMarkdownFile(file: { content: string; name: string; path: string }) {
  const normalizedPath = file.path.replaceAll("\\", "/");
  const rootPath = normalizedPath.slice(0, normalizedPath.lastIndexOf("/")) || "/";
  const existingRead = mockedReadNativeMarkdownFile.getMockImplementation();
  mockDesktopPrimaryWorkspace({ root: rootPath, status: "ready" });
  mockedGetStoredWorkspaceState.mockResolvedValue({
    filePath: file.path,
    fileTreeOpen: false,
    folderName: rootPath.split("/").at(-1) ?? rootPath,
    folderPath: rootPath,
    openFilePaths: [file.path]
  });
  mockedReadNativeMarkdownFile.mockImplementation(async (path) => {
    if (path === file.path) return file;
    if (existingRead) return await existingRead(path);
    return { ...file, name: path.split(/[\\/]/).at(-1) ?? file.name, path };
  });
}

export function mockOpenMarkdownTarget(target:
  | { file: { content: string; name: string; path: string }; kind: "file" }
  | { folder: { name: string; path: string }; kind: "folder" }
) {
  if (target.kind === "file") {
    mockOpenMarkdownFile(target.file);
    return;
  }

  mockDesktopPrimaryWorkspace({ root: target.folder.path, status: "ready" });
}

export function mockOpenMarkdownFolder(folder: { name: string; path: string }) {
  mockDesktopPrimaryWorkspace({ root: folder.path, status: "ready" });
  mockedOpenNativeMarkdownFolder.mockResolvedValue(folder);
}

export type { NativeMenuHandlers } from "../lib/tauri";

export function mockDesktopPrimaryWorkspace({
  error = null,
  root,
  status
}: {
  error?: string | null;
  root: string | null;
  status: PrimaryWorkspaceStatus;
}) {
  const controller: PrimaryWorkspaceController = {
    canChooseDesktopRoot: true,
    commitDesktopRoot: vi.fn(async () => root),
    commitManagedRoot: vi.fn(async () => null),
    deferDesktopSetup: vi.fn(async () => undefined),
    error,
    managedName: null,
    resetOnboarding: vi.fn(async () => undefined),
    retry: vi.fn(async () => undefined),
    root,
    status,
    workspaceRoot: parentPathFromPath(root)
  };
  primaryWorkspaceHarnessState.desktopController = controller;
  return controller;
}

export function renderApp() {
  if (
    !primaryWorkspaceHarnessState.desktopController &&
    window.location.search.length === 0
  ) {
    const folderPath = configuredWorkspaceState.folderPath?.trim();
    const filePath = configuredWorkspaceState.filePath?.trim();
    const openFilePaths = configuredWorkspaceState.openFilePaths?.filter((path) => path.trim().length > 0) ?? [];
    if (externalBlankPickerRequested) {
      const url = new URL(window.location.href);
      url.searchParams.set("blank", "1");
      window.history.replaceState({}, "", url);
    } else if (folderPath) {
      mockDesktopPrimaryWorkspace({ root: folderPath, status: "ready" });
    } else if (openFilePaths.length > 1) {
      const directorySegments = openFilePaths.map((path) => path.split("/").filter(Boolean).slice(0, -1));
      const commonSegments = directorySegments[0]?.filter((segment, index) =>
        directorySegments.every((segments) => segments[index] === segment)
      ) ?? [];
      const restoreRoot = commonSegments.length > 0 ? `/${commonSegments.join("/")}` : null;
      if (restoreRoot) mockDesktopPrimaryWorkspace({ root: restoreRoot, status: "ready" });
    } else if (filePath) {
      const url = new URL(window.location.href);
      url.searchParams.set("path", filePath);
      window.history.replaceState({}, "", url);
    }
  }

  return render(<App />);
}

export function rerenderApp(app: ReturnType<typeof render>) {
  app.rerender(<App />);
}

export function installAppTestHarness() {
  afterAll(async () => {
    // Milkdown ctx leaves 3s listener cleanup timers pending after editor teardown.
    await new Promise((resolve) => {
      window.setTimeout(resolve, 3200);
    });
  });

  beforeEach(() => {
    primaryWorkspaceHarnessState.desktopController = null;
    configuredWorkspaceState = {};
    externalBlankPickerRequested = false;
    const runtime = createDefaultAppRuntime();
    runtime.themes.capabilities = {
      canDelete: true,
      canImport: true,
      canOpenDirectory: true
    };
    runtime.themes.confirmActivation = vi.fn(async () => true);
    runtime.themes.list = vi.fn(async () => ({ invalidFiles: [], themes: appHarnessThemeDescriptors }));
    runtime.themes.prepareActivation = vi.fn(async (id, expectedFingerprint) => ({
      fingerprint: expectedFingerprint,
      id,
      source: { kind: "inline" as const, css: `:root { --app-harness-theme: ${id}; }` },
      token: `${id}-token`
    }));
    runtime.themes.commitActivation = vi.fn(async () => undefined);
    runtime.themes.cancelActivation = vi.fn(async () => undefined);
    runtime.themes.releaseActivation = vi.fn(async () => undefined);
    configureAppRuntime({
      ...runtime,
      features: {
        ...runtime.features,
        applicationMenu: true,
        applicationShortcuts: true,
        export: true,
        fileDrop: true,
        nativeWindowChrome: true,
        openLocalAttachments: true,
        pandoc: true,
        settingsWindow: true,
        updater: true
      },
      mcp: createApplicationMcpRuntime()
    });
    window.history.pushState({}, "", "/");
    mockedConsumeWelcomeDocumentState.mockReset();
    mockedConfirmNativeMarkdownFileDelete.mockReset();
    mockedConfirmNativeUnsavedMarkdownDocumentDiscard.mockReset();
    mockedCreateNativeMarkdownTreeFile.mockReset();
    mockedCreateNativeMarkdownTreeFolder.mockReset();
    mockedDeleteNativeMarkdownTreeFile.mockReset();
    mockedDetectNativePandocPath.mockReset();
    mockedInstallNativeMarkdownFileDrop.mockReset();
    mockedOpenNativeContainingFolder.mockReset();
    mockedOpenNativeMarkdownFolder.mockReset();
    mockedOpenNativeMarkdownFile.mockReset();
    mockedLoadPrimaryWorkspaceState.mockReset();
    mockedSavePrimaryWorkspaceState.mockReset();
    mockedOpenNativeMarkdownFileInNewWindow.mockReset();
    mockedListenNativeOpenedMarkdownPaths.mockReset();
    mockedListNativeMarkdownFileHistory.mockReset();
    mockedMoveNativeMarkdownTreeFile.mockReset();
    mockedReadNativeLocalImageFile.mockReset();
    mockedReadNativeMarkdownFile.mockReset();
    mockedReadNativeMarkdownFileHistory.mockReset();
    mockedReadNativeMarkdownTemplateFile.mockReset();
    mockedResolveNativeMarkdownPath.mockReset();
    mockedSaveNativeHtmlFile.mockReset();
    mockedSaveNativePandocFile.mockReset();
    mockedSaveNativePdfFile.mockReset();
    mockedSearchNativeMarkdownFilesForPath.mockReset();
    mockedShowNativePandocSetup.mockReset();
    mockedRenameNativeMarkdownTreeFile.mockReset();
    mockedSaveNativeMarkdownFile.mockReset();
    mockedShowNativeMarkdownFileTreeContextMenu.mockReset();
    mockedSetNativeEditorWindowRestoreState.mockReset();
    mockedListNativeMarkdownFileHistory.mockReset();
    mockedLoadNativeMarkdownFilesForPath.mockReset();
    mockedListNativeMarkdownFilesForPath.mockReset();
    mockedTakeNativeOpenedMarkdownPaths.mockReset();
    mockedWatchNativeMarkdownFile.mockReset();
    mockedWriteNativeMarkdownTemplateFile.mockReset();
    mockedWatchNativeMarkdownTree.mockReset();
    mockedInstallNativeApplicationMenu.mockReset();
    mockedInstallNativeEditorContextMenu.mockReset();
    mockedListenNativeApplicationMenuCommands.mockReset();
    mockedOpenNativeExternalUrl.mockReset();
    mockedCloseNativeWindow.mockReset();
    mockedDestroyNativeWindow.mockReset();
    mockedShowNativeWindow.mockReset();
    mockedHideSettingsWindow.mockReset();
    mockedMarkSettingsWindowReady.mockReset();
    mockedShowNativeAppAbout.mockReset();
    mockedExitNativeApp.mockReset();
    mockedListenNativeAppExitRequested.mockReset();
    mockedListenNativeWindowCloseRequested.mockReset();
    mockedListNativeEditorWindowRestoreStates.mockReset();
    mockedCheckNativeAppUpdate.mockReset();
    mockedResolveDesktopOsVersion.mockReset();
    mockedResolveDesktopPlatform.mockReset();
    mockedOpenSettingsWindow.mockReset();
    mockedGetStoredLanguage.mockReset();
    mockedGetStoredRecentMarkdownFiles.mockReset();
    mockedGetStoredCustomThemeCss.mockReset();
    mockedGetStoredEditorPreferences.mockReset();
    mockedGetStoredExportSettings.mockReset();
    mockedGetStoredFileIgnoreSettings.mockReset();
    mockedGetStoredTheme.mockReset();
    mockedGetStoredThemePreferences.mockReset();
    mockedGetStoredWorkspaceState.mockReset();
    mockedClearStoredRecentMarkdownFiles.mockReset();
    mockedRemoveStoredRecentMarkdownFile.mockReset();
    mockedResetWelcomeDocumentState.mockReset();
    mockedSaveStoredCustomThemeCss.mockReset();
    mockedSaveStoredEditorPreferences.mockReset();
    mockedSaveStoredExportSettings.mockReset();
    mockedSaveStoredFileIgnoreSettings.mockReset();
    mockedSaveStoredLanguage.mockReset();
    mockedSaveStoredRecentMarkdownFile.mockReset();
    mockedSaveStoredTheme.mockReset();
    mockedSaveStoredThemePreferences.mockReset();
    mockedSaveStoredWorkspaceState.mockReset();
    mockedListenAppEditorPreferencesChanged.mockReset();
    mockedListenAppExportSettingsChanged.mockReset();
    mockedListenAppFileIgnoreSettingsChanged.mockReset();
    mockedListenAppLanguageChanged.mockReset();
    mockedListenAppThemeChanged.mockReset();
    mockedNotifyAppEditorPreferencesChanged.mockReset();
    mockedNotifyAppExportSettingsChanged.mockReset();
    mockedNotifyAppFileIgnoreSettingsChanged.mockReset();
    mockedNotifyAppLanguageChanged.mockReset();
    mockedNotifyAppThemeChanged.mockReset();
    mockedDownloadNativeWebImage.mockReset();
    mockedOpenNativeLocalImages.mockReset();
    mockedOpenNativeLocalFiles.mockReset();
    mockedImportNativeLocalFile.mockReset();
    mockedOpenNativeMarkdownAttachment.mockReset();
    mockedSaveNativeClipboardImage.mockReset();
    mockedSaveNativeClipboardAttachment.mockReset();
    mockedGetNativeShellCommandStatus.mockReset();
    mockedInstallNativeShellCommand.mockReset();
    mockedUninstallNativeShellCommand.mockReset();
    document.documentElement.removeAttribute("data-theme");
    document.documentElement.removeAttribute("data-theme-appearance");
    document.documentElement.removeAttribute("data-webkit-scroll-workaround");
    document.documentElement.removeAttribute("data-window");
    document.getElementById("markra-custom-theme-style")?.remove();
    document.getElementById("markra-third-party-theme-style")?.remove();
    document.getElementById("markra-third-party-theme-link")?.remove();
    document.querySelectorAll("[data-markra-theme-candidate]").forEach((element) => element.remove());
    document.getElementById("markra-startup-theme-style")?.remove();
    document.documentElement.style.removeProperty("background-color");
    document.documentElement.style.removeProperty("color-scheme");
    mockedWatchNativeMarkdownFile.mockResolvedValue(() => {});
    mockedWatchNativeMarkdownTree.mockResolvedValue(() => {});
    mockedListNativeMarkdownFileHistory.mockResolvedValue([]);
    mockedReadNativeMarkdownFileHistory.mockRejectedValue(new Error("markdown history file is not mocked"));
    mockedReadNativeLocalImageFile.mockRejectedValue(new Error("local image file is not mocked"));
    mockedListNativeMarkdownFilesForPath.mockResolvedValue([]);
    mockedLoadNativeMarkdownFilesForPath.mockImplementation(async (path, options = {}) => {
      const files = await mockedListNativeMarkdownFilesForPath(path, {
        managedAttachmentFolder: options.managedAttachmentFolder
      });
      if (!options.signal?.aborted) options.onBatch?.(files);
      return files;
    });
    mockedSearchNativeMarkdownFilesForPath.mockResolvedValue(null);
    mockedTakeNativeOpenedMarkdownPaths.mockResolvedValue([]);
    mockedInstallNativeMarkdownFileDrop.mockResolvedValue(() => {});
    mockedOpenNativeContainingFolder.mockResolvedValue(undefined);
    mockedOpenNativeMarkdownFile.mockResolvedValue(null);
    // Test-only baseline: legacy App cases enter the editor without opting into a production workspace.
    // Production storage still defaults to needs-onboarding when the primary-workspace key is absent.
    mockedLoadPrimaryWorkspaceState.mockResolvedValue({
      desktopWorkspaceRoot: null,
      desktopPath: null,
      managedName: "workspace",
      onboardingCompleted: true,
      version: 3
    });
    mockedSavePrimaryWorkspaceState.mockImplementation(async (state) => state);
    mockedListenNativeOpenedMarkdownPaths.mockResolvedValue(() => {});
    mockedInstallNativeApplicationMenu.mockResolvedValue(() => {});
    mockedInstallNativeEditorContextMenu.mockResolvedValue(() => {});
    mockedOpenNativeExternalUrl.mockResolvedValue(undefined);
    mockedCloseNativeWindow.mockResolvedValue(undefined);
    mockedDestroyNativeWindow.mockResolvedValue(undefined);
    mockedShowNativeWindow.mockResolvedValue(undefined);
    mockedHideSettingsWindow.mockResolvedValue(undefined);
    mockedMarkSettingsWindowReady.mockResolvedValue(undefined);
    mockedShowNativeAppAbout.mockResolvedValue(undefined);
    mockedExitNativeApp.mockResolvedValue(undefined);
    mockedListenNativeAppExitRequested.mockResolvedValue(() => {});
    mockedListenNativeWindowCloseRequested.mockResolvedValue(() => {});
    mockedListenNativeApplicationMenuCommands.mockResolvedValue(() => {});
    mockedListNativeEditorWindowRestoreStates.mockResolvedValue([]);
    mockedDownloadNativeWebImage.mockResolvedValue(new File([new Uint8Array([1, 2, 3])], "web-image.png", {
      type: "image/png"
    }));
    mockedOpenNativeLocalImages.mockResolvedValue([]);
    mockedOpenNativeLocalFiles.mockResolvedValue([]);
    mockedImportNativeLocalFile.mockResolvedValue({
      label: "Imported file",
      src: "assets/imported-file.pdf"
    });
    mockedOpenNativeMarkdownAttachment.mockResolvedValue(undefined);
    mockedSaveNativeClipboardImage.mockResolvedValue({
      alt: "Imported image",
      src: "assets/imported-image.png"
    });
    mockedSaveNativeClipboardAttachment.mockResolvedValue({
      label: "Imported file",
      src: "assets/imported-file.pdf"
    });
    mockedGetNativeShellCommandStatus.mockResolvedValue({
      commandPath: "/mock-bin/markra",
      targetPath: "/mock-app/markra",
      status: "missing"
    });
    mockedInstallNativeShellCommand.mockResolvedValue({
      commandPath: "/mock-bin/markra",
      targetPath: "/mock-app/markra",
      status: "installed"
    });
    mockedUninstallNativeShellCommand.mockResolvedValue({
      commandPath: "/mock-bin/markra",
      targetPath: "/mock-app/markra",
      status: "missing"
    });
    mockedCheckNativeAppUpdate.mockResolvedValue(null);
    mockedResolveDesktopOsVersion.mockReturnValue(null);
    mockedResolveDesktopPlatform.mockReturnValue("macos");
    mockedOpenSettingsWindow.mockResolvedValue(undefined);
    mockedReadNativeMarkdownTemplateFile.mockRejectedValue(new Error("template file is not mocked"));
    mockedWriteNativeMarkdownTemplateFile.mockResolvedValue(undefined);
    mockedResolveNativeMarkdownPath.mockImplementation(async (path) => ({
      kind: path === mockFolderPath ? "folder" : "file",
      name: path === mockFolderPath ? "vault" : path.split("/").pop() ?? path,
      path
    }));
    mockedSaveStoredEditorPreferences.mockResolvedValue(undefined);
    mockedSaveStoredCustomThemeCss.mockResolvedValue(undefined);
    mockedSaveStoredExportSettings.mockResolvedValue(undefined);
    mockedSaveNativeHtmlFile.mockResolvedValue({
      name: "Untitled.html",
      path: "/mock-files/Untitled.html"
    });
    mockedSaveNativePdfFile.mockResolvedValue({
      name: "Untitled.pdf",
      path: "/mock-files/Untitled.pdf"
    });
    mockedSaveNativePandocFile.mockResolvedValue({
      name: "Untitled.docx",
      path: "/mock-files/Untitled.docx"
    });
    mockedShowNativePandocSetup.mockResolvedValue("cancel");
    mockedShowNativeMarkdownFileTreeContextMenu.mockResolvedValue(undefined);
    mockedSetNativeEditorWindowRestoreState.mockResolvedValue(undefined);
    mockedListenAppCustomThemeCssChanged.mockResolvedValue(() => {});
    mockedListenAppEditorPreferencesChanged.mockResolvedValue(() => {});
    mockedListenAppExportSettingsChanged.mockResolvedValue(() => {});
    mockedListenAppFileIgnoreSettingsChanged.mockResolvedValue(() => {});
    mockedGetStoredFileIgnoreSettings.mockResolvedValue({ rules: "" });
    mockedSaveStoredFileIgnoreSettings.mockResolvedValue(undefined);
    mockedConsumeWelcomeDocumentState.mockResolvedValue(true);
    mockedConfirmNativeMarkdownFileDelete.mockResolvedValue(true);
    mockedConfirmNativeUnsavedMarkdownDocumentDiscard.mockResolvedValue(true);
    mockedCreateNativeMarkdownTreeFile.mockResolvedValue({
      name: "Daily note.md",
      path: "/mock-files/vault/Daily note.md",
      relativePath: "Daily note.md"
    });
    mockedCreateNativeMarkdownTreeFolder.mockResolvedValue({
      kind: "folder",
      name: "Research",
      path: "/mock-files/vault/Research",
      relativePath: "Research"
    });
    mockedDeleteNativeMarkdownTreeFile.mockResolvedValue(undefined);
    mockedMoveNativeMarkdownTreeFile.mockResolvedValue({
      name: "Moved.md",
      path: "/mock-files/vault/archive/Moved.md",
      relativePath: "archive/Moved.md"
    });
    mockedDetectNativePandocPath.mockResolvedValue(null);
    mockedRenameNativeMarkdownTreeFile.mockResolvedValue({
      name: "Renamed.md",
      path: "/mock-files/vault/Renamed.md",
      relativePath: "Renamed.md"
    });
    mockedGetStoredEditorPreferences.mockResolvedValue({
      autoRevealActiveFile: true,
      autoSaveEnabled: true,
      autoSaveIntervalMinutes: 10,
      autoUpdateEnabled: true,
      bodyFontSize: 16,
      clipboardImageFolder: "assets",
      contentWidth: "default",
      contentWidthPx: null,
      documentLinksOpen: true,
      documentLinksVisible: false,
      editorFontFamily: { family: null, source: "theme" },
      extendedSyntax: {
        githubAlerts: true,
        highlight: true
      },
      imageUpload: {
        fileNamePattern: "pasted-image-{timestamp}"
      },
      lineHeight: 1.65,
      markdownShortcuts: defaultMarkdownShortcuts,
      markdownTemplates: [],
      paragraphSpacingPx: 8,
      restoreWorkspaceOnStartup: true,
      sidebarLayoutMode: "stacked",
      showDocumentTabs: true,
      splitVisualPanePercent: 50,
      tableColumnWidthMode: "auto",
        titlebarActions: [
          { id: "viewMode", visible: true },
          { id: "sourceMode", visible: true },
        { id: "history", visible: true },
        { id: "save", visible: true },
        { id: "theme", visible: true }
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
      showWordCount: true,
      wrapCodeBlocks: true
    });
    mockedGetStoredExportSettings.mockResolvedValue({
      pandocArgs: "",
      pandocPath: "",
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
    mockedGetStoredLanguage.mockResolvedValue("en");
    mockedGetStoredRecentMarkdownFiles.mockResolvedValue([]);
    mockedGetStoredCustomThemeCss.mockResolvedValue({
      dark: ":root[data-theme=\"custom\"] { --bg-primary: #0d1117; }",
      light: ":root[data-theme=\"custom\"] { --bg-primary: #fdf6e3; }"
    });
    mockedGetStoredTheme.mockResolvedValue("light");
    mockedGetStoredThemePreferences.mockResolvedValue({
      appearanceMode: "light",
      darkTheme: "dark",
      lightTheme: "light"
    });
    mockedGetStoredWorkspaceState.mockResolvedValue({
      filePath: null,
      fileTreeOpen: false,
      folderName: null,
      folderPath: null,
      openFilePaths: []
    });
    mockedResetWelcomeDocumentState.mockResolvedValue(undefined);
    mockedClearStoredRecentMarkdownFiles.mockResolvedValue(undefined);
    mockedRemoveStoredRecentMarkdownFile.mockResolvedValue([]);
    mockedSaveStoredLanguage.mockResolvedValue(undefined);
    mockedSaveStoredRecentMarkdownFile.mockResolvedValue([]);
    mockedSaveStoredTheme.mockResolvedValue(undefined);
    mockedSaveStoredThemePreferences.mockResolvedValue(undefined);
    mockedSaveStoredWorkspaceState.mockResolvedValue(undefined);
    mockedListenAppLanguageChanged.mockResolvedValue(() => {});
    mockedListenAppThemeChanged.mockResolvedValue(() => {});
    mockedNotifyAppLanguageChanged.mockResolvedValue(undefined);
    mockedNotifyAppThemeChanged.mockResolvedValue(undefined);
    mockSystemColorScheme(false);
  });
}
