import type { AppLanguage } from "@markra/shared";
import type { MarkdownShortcutMap } from "@markra/editor";
import type { DesktopPlatform } from "../lib/platform";
import type { ContextMenuEntry } from "../components/ContextMenu";
import type { NativeAppUpdate } from "../lib/tauri/updater";
import type {
  CreateNativeMarkdownTreeFileOptions,
  DownloadNativeWebImageInput,
  ImportNativeLocalFileInput,
  LoadNativeMarkdownFilesForPathOptions,
  ListNativeMarkdownFilesOptions,
  NativeMarkdownDroppedTarget,
  NativeMarkdownFile,
  NativeMarkdownFileChangeHandler,
  NativeMarkdownFileDropHandler,
  NativeMarkdownFileHistoryEntry,
  NativeMarkdownFileHistoryFile,
  NativeMarkdownFolder,
  NativeMarkdownFolderFile,
  NativeMarkdownPickerLabels,
  NativeLocalFile,
  NativeSettingsFile,
  NativeMarkdownTreeChangeHandler,
  WatchNativeMarkdownOptions,
  NativePandocExportFormat,
  OpenNativeMarkdownAttachmentInput,
  SavedNativeClipboardAttachment,
  SavedNativeClipboardImage,
  SavedNativeHtmlFile,
  SavedNativeMarkdownFile,
  SavedNativePandocFile,
  SavedNativePdfFile,
  SavedNativeSettingsFile,
  SaveNativeClipboardAttachmentInput,
  SaveNativeClipboardImageInput,
  SaveNativeHtmlFileInput,
  SaveNativeMarkdownFileInput,
  SaveNativePandocFileInput,
  SaveNativePdfFileInput,
  SaveNativeSettingsFileInput,
  TrashWorkspaceResourceInput,
  TrashWorkspaceResourceResult
} from "../lib/tauri/file";
import type {
  NativeEditorContextMenuEntryOptions,
  NativeEditorContextMenuOptions,
  NativeMarkdownFileTreeContextMenuHandlers,
  NativeMenuHandlers
} from "../lib/tauri/menu";
import type { RecentMarkdownFile } from "../lib/settings/app-settings";
import type {
  NativeEditorWindowRestoreState,
  NativeSettingsWindowTarget,
  NativeWindowCloseRequestEvent,
  SetNativeEditorWindowRestoreStateInput
} from "../lib/tauri/window";
import type { NativePandocSetupAction } from "../lib/tauri/dialog";
import type { NativeShellCommandStatus } from "../lib/tauri/shell-command";
import type { WorkspaceSearchRequest, WorkspaceSearchResponse } from "../lib/workspace-search";
import { setAppLogBackendWriter, type AppLogEvent, type AppLogWriter } from "../lib/app-logger";
import type { AppSyncConfigRuntime } from "../lib/sync-config";
import type {
  McpAuditEntry,
  McpConfig,
  McpServerHealth,
  McpSettingsSnapshot
} from "../lib/mcp";
import type {
  ThemeCatalogSnapshot,
  ThemeActivationPayload,
  ThemeDescriptor,
  ThemeImportResult,
  ThemeRuntimeCapabilities
} from "../lib/themes/theme-catalog";

export type { WorkspaceSearchRequest, WorkspaceSearchResponse } from "../lib/workspace-search";
export type { AppLogArea, AppLogEvent, AppLogLevel, AppLogWriter } from "../lib/app-logger";
export { appLogger } from "../lib/app-logger";

export type RuntimeCleanup = () => unknown;

export type AppSystemBackSubscriber = (
  handler: () => Promise<boolean>
) => Promise<RuntimeCleanup>;

export type AppNavigationRuntime = {
  subscribeToSystemBack: AppSystemBackSubscriber;
};

export type RuntimeStore = {
  delete: (key: string) => Promise<unknown>;
  get: <T>(key: string) => Promise<T | undefined>;
  save: () => Promise<unknown>;
  set: (key: string, value: unknown) => Promise<unknown>;
};

export type RuntimeStoreLoadOptions = {
  autoSave: boolean;
  defaults: Record<string, unknown>;
};

export type RuntimeEvent<TPayload> = {
  payload: TPayload;
};

export type AppSettingsGroup =
  | "appearance"
  | "customThemeCss"
  | "language"
  | "editorPreferences"
  | "fileIgnoreSettings"
  | "exportSettings";

export type AppSettingsRuntime = {
  loadStore: (path: string, options: RuntimeStoreLoadOptions) => Promise<RuntimeStore>;
  readPrimaryWorkspaceState?: () => Promise<unknown>;
  readGroup?: <TValue>(group: AppSettingsGroup) => Promise<TValue | undefined>;
  writePrimaryWorkspaceState?: (input: {
    expectedState?: unknown;
    state: unknown;
  }) => Promise<{
    applied: boolean;
    state: unknown;
  }>;
  writeGroup?: (group: AppSettingsGroup, value: unknown) => Promise<unknown>;
  replacePortable?: (settings: unknown) => Promise<unknown>;
};

export type AppThemeRuntime = {
  cancelActivation: (token: string) => Promise<unknown>;
  capabilities: ThemeRuntimeCapabilities;
  commitActivation: (token: string) => Promise<unknown>;
  confirmActivation: (themeName: string) => Promise<boolean>;
  delete: (id: string, expectedFingerprint: string) => Promise<unknown>;
  importFile: () => Promise<ThemeImportResult | null>;
  list: () => Promise<ThemeCatalogSnapshot>;
  openDirectory: () => Promise<unknown>;
  prepareActivation: (id: string, expectedFingerprint: string) => Promise<ThemeActivationPayload>;
  releaseActivation: () => Promise<unknown>;
  replaceFile: (sourcePath: string, expectedFingerprint: string) => Promise<ThemeDescriptor>;
};

export type AppMcpRuntime = {
  policyAvailable: boolean;
  localServiceAvailable: boolean;
  setPrimaryWorkspace: (input: { primaryRoot: string | null }) => Promise<McpSettingsSnapshot>;
  getSettings: () => Promise<McpSettingsSnapshot>;
  updateSettings: (input: { expectedRevision: string; config: McpConfig }) => Promise<McpSettingsSnapshot>;
  getHealth: () => Promise<McpServerHealth>;
  listAuditEntries: (offset: number, limit: number) => Promise<McpAuditEntry[]>;
  clearAuditEntries: () => Promise<unknown>;
};

export type AppEventsRuntime = {
  emit: <TPayload>(event: string, payload: TPayload) => Promise<unknown>;
  isAvailable: () => boolean;
  listen: <TPayload>(
    event: string,
    handler: (event: RuntimeEvent<TPayload>) => unknown
  ) => Promise<RuntimeCleanup>;
};

export type AppFormFactor = "desktop" | "mobile";

export type AppPlatformRuntime = {
  resolveDesktopOsVersion: () => string | null;
  resolveDesktopPlatform: () => DesktopPlatform | null;
  resolveFormFactor: () => AppFormFactor;
};

export type AppWorkspaceRuntime = {
  discardPreparedDesktopNotebookTarget?: (lease: string) => Promise<unknown>;
  isDocumentInRoot?: (documentPath: string, rootPath: string) => Promise<boolean>;
  listManagedNotebookNames?: () => Promise<string[]>;
  prepareDesktopNotebookTarget?: (input: {
    notebookName: string;
    parentPath: string;
  }) => Promise<{
    lease: string;
    notesRoot: string;
  } | null>;
  resolveManagedRoot: (name: string) => Promise<string | null>;
};

export type AppDialogRuntime = {
  showAppAbout: () => Promise<unknown>;
  showPandocSetup: (
    labels: {
      cancelLabel: string;
      installLabel: string;
      message: string;
      setPathLabel: string;
      title: string;
    }
  ) => Promise<NativePandocSetupAction>;
};

export type AppFileRuntime = {
  confirmMarkdownFileDelete: (
    fileName: string,
    labels: { cancelLabel: string; message: string; okLabel: string }
  ) => Promise<boolean>;
  confirmWorkspaceResourceTrash: (
    labels: { cancelLabel: string; message: string; okLabel: string }
  ) => Promise<boolean>;
  confirmUnsavedMarkdownDocumentDiscard: (
    fileName: string,
    labels: { cancelLabel: string; message: string; okLabel: string }
  ) => Promise<boolean>;
  createMarkdownTreeFile: (
    rootPath: string,
    fileName: string,
    optionsOrParentPath?: CreateNativeMarkdownTreeFileOptions | string | null
  ) => Promise<NativeMarkdownFolderFile>;
  createMarkdownTreeFolder: (
    rootPath: string,
    folderName: string,
    parentPath?: string | null
  ) => Promise<NativeMarkdownFolderFile>;
  deleteMarkdownTemplateFile: (fileName: string) => Promise<unknown>;
  deleteMarkdownTreeFile: (rootPath: string, path: string) => Promise<unknown>;
  detectPandocPath: () => Promise<string | null>;
  installMarkdownFileDrop: (onDrop: NativeMarkdownFileDropHandler) => Promise<RuntimeCleanup>;
  importLocalFile: (input: ImportNativeLocalFileInput) => Promise<SavedNativeClipboardAttachment>;
  listenOpenedMarkdownPaths: (
    onPaths: (paths: string[]) => unknown | Promise<unknown>
  ) => Promise<RuntimeCleanup>;
  listMarkdownFileHistory: (path: string) => Promise<NativeMarkdownFileHistoryEntry[]>;
  listMarkdownFilesForPath: (
    path: string,
    options?: ListNativeMarkdownFilesOptions
  ) => Promise<NativeMarkdownFolderFile[]>;
  loadMarkdownFilesForPath?: (
    path: string,
    options?: LoadNativeMarkdownFilesForPathOptions
  ) => Promise<NativeMarkdownFolderFile[]>;
  moveMarkdownTreeFile: (
    rootPath: string,
    path: string,
    targetParentPath?: string | null
  ) => Promise<NativeMarkdownFolderFile>;
  openContainingFolder: (path: string) => Promise<unknown>;
  openLocalImages: (labels?: NativeMarkdownPickerLabels) => Promise<File[]>;
  openLocalFiles: (labels?: NativeMarkdownPickerLabels) => Promise<NativeLocalFile[]>;
  openMarkdownAttachment: (input: OpenNativeMarkdownAttachmentInput) => Promise<unknown>;
  openMarkdownFile: (labels?: NativeMarkdownPickerLabels) => Promise<NativeMarkdownFile | null>;
  openMarkdownFileInNewWindow: (path: string) => Promise<unknown>;
  openMarkdownFolder: (labels?: NativeMarkdownPickerLabels) => Promise<NativeMarkdownFolder | null>;
  requestPrimaryNotebookSwitch?: (path: string) => Promise<unknown>;
  openSettingsFile: (labels?: NativeMarkdownPickerLabels) => Promise<NativeSettingsFile | null>;
  readLocalImageFile: (path: string) => Promise<File>;
  readMarkdownFile: (path: string) => Promise<NativeMarkdownFile>;
  readMarkdownFileHistory: (path: string, id: string) => Promise<NativeMarkdownFileHistoryFile>;
  readMarkdownTemplateFile: (fileName: string) => Promise<string>;
  renameMarkdownTreeFile: (
    rootPath: string,
    path: string,
    fileName: string
  ) => Promise<NativeMarkdownFolderFile>;
  resolveMarkdownFolder: (path: string) => Promise<NativeMarkdownFolder>;
  resolveMarkdownPath: (path: string) => Promise<NativeMarkdownDroppedTarget>;
  resolveWorkspaceResourceRoot: (sourcePath: string) => Promise<string>;
  saveClipboardImage: (input: SaveNativeClipboardImageInput) => Promise<SavedNativeClipboardImage>;
  saveClipboardAttachment: (input: SaveNativeClipboardAttachmentInput) => Promise<SavedNativeClipboardAttachment>;
  saveHtmlFile: (input: SaveNativeHtmlFileInput) => Promise<SavedNativeHtmlFile | null>;
  saveMarkdownFile: (input: SaveNativeMarkdownFileInput) => Promise<SavedNativeMarkdownFile | null>;
  savePandocFile: (input: SaveNativePandocFileInput) => Promise<SavedNativePandocFile | null>;
  savePdfFile: (input: SaveNativePdfFileInput) => Promise<SavedNativePdfFile | null>;
  saveSettingsFile: (input: SaveNativeSettingsFileInput) => Promise<SavedNativeSettingsFile | null>;
  searchMarkdownFiles?: (request: WorkspaceSearchRequest) => Promise<WorkspaceSearchResponse>;
  takeOpenedMarkdownPaths: () => Promise<string[]>;
  trashWorkspaceResources: (
    rootPath: string,
    resources: readonly TrashWorkspaceResourceInput[]
  ) => Promise<TrashWorkspaceResourceResult[]>;
  watchMarkdownFile: (
    path: string,
    onChange: NativeMarkdownFileChangeHandler,
    onTreeChange?: NativeMarkdownTreeChangeHandler,
    options?: WatchNativeMarkdownOptions
  ) => Promise<RuntimeCleanup>;
  watchMarkdownTree: (
    path: string,
    onTreeChange: NativeMarkdownTreeChangeHandler,
    options?: WatchNativeMarkdownOptions
  ) => Promise<RuntimeCleanup>;
  writeMarkdownTemplateFile: (fileName: string, contents: string) => Promise<unknown>;
};

export type AppMenuRuntime = {
  createEditorContextMenuItems: (
    handlers: NativeMenuHandlers,
    language?: AppLanguage,
    options?: NativeEditorContextMenuEntryOptions
  ) => ContextMenuEntry[];
  createMarkdownFileTreeContextMenuItems: (
    handlers: NativeMarkdownFileTreeContextMenuHandlers,
    language?: AppLanguage,
    file?: NativeMarkdownFolderFile
  ) => ContextMenuEntry[];
  installApplicationMenu: (
    handlers: NativeMenuHandlers,
    language?: AppLanguage,
    markdownShortcuts?: MarkdownShortcutMap,
    recentFiles?: readonly RecentMarkdownFile[]
  ) => Promise<RuntimeCleanup>;
  installEditorContextMenu: (
    target: Pick<EventTarget, "addEventListener" | "removeEventListener">,
    handlers: NativeMenuHandlers,
    language?: AppLanguage,
    options?: NativeEditorContextMenuOptions
  ) => Promise<RuntimeCleanup>;
  listenApplicationMenuCommands: (handlers: NativeMenuHandlers) => Promise<RuntimeCleanup>;
  readClipboardText: () => Promise<string | null>;
  showMarkdownFileTreeContextMenu: (
    handlers: NativeMarkdownFileTreeContextMenuHandlers,
    language?: AppLanguage,
    file?: NativeMarkdownFolderFile
  ) => Promise<unknown>;
};

export type AppUpdaterRuntime = {
  checkAppUpdate: () => Promise<NativeAppUpdate | null>;
};

export type AppLogsRuntime = {
  isAvailable: () => boolean;
  openLogFolder: () => Promise<unknown>;
  writeLog: AppLogWriter;
};

export type AppShellCommandRuntime = {
  getShellCommandStatus: () => Promise<NativeShellCommandStatus>;
  installShellCommand: () => Promise<NativeShellCommandStatus>;
  uninstallShellCommand: () => Promise<NativeShellCommandStatus>;
};

export type AppSystemFontFamily = {
  family: string;
  label: string;
};

export type AppSystemFontsRuntime = {
  listFontFamilies: () => Promise<AppSystemFontFamily[]>;
};

export type AppWebResourceRuntime = {
  downloadImage: (input: DownloadNativeWebImageInput) => Promise<File>;
};

export type AppFeatureRuntime = {
  applicationMenu: boolean;
  applicationShortcuts: boolean;
  export: boolean;
  fileDrop: boolean;
  imageImport: boolean;
  nativeWindowChrome: boolean;
  openLocalAttachments: boolean;
  pandoc: boolean;
  projectSync: boolean;
  resources: boolean;
  settingsWindow: boolean;
  systemFonts: boolean;
  updater: boolean;
};

export type NativeSettingsWindowHideRequest = {
  generation: number;
};

export type NativeSettingsWindowContext = {
  projectRoot: string | null;
  sourceWindowLabel: string | null;
  workspaceSourcePath: string | null;
};

export type AppWindowRuntime = {
  acknowledgeSettingsWindowHide: (generation: number) => Promise<unknown>;
  cancelSettingsWindowHide: (generation: number) => Promise<unknown>;
  closeWindow: () => Promise<unknown>;
  completeSettingsWindowHide: (generation: number) => Promise<unknown>;
  destroyWindow: () => Promise<unknown>;
  exitApp: () => Promise<unknown>;
  getCurrentWindowLabel: () => Promise<string | null>;
  listEditorWindowRestoreStates: () => Promise<NativeEditorWindowRestoreState[]>;
  listenAppExitRequested: (onExitRequested: () => unknown | Promise<unknown>) => Promise<RuntimeCleanup>;
  listenSettingsWindowHideRequested: (
    onHideRequested: (request: NativeSettingsWindowHideRequest) => unknown | Promise<unknown>
  ) => Promise<RuntimeCleanup>;
  listenSettingsWindowTarget: (
    onTarget: (target: NativeSettingsWindowTarget) => unknown,
    onContext?: (context: NativeSettingsWindowContext) => unknown
  ) => Promise<RuntimeCleanup>;
  listenWindowCloseRequested: (
    onCloseRequested: (event: NativeWindowCloseRequestEvent) => unknown | Promise<unknown>
  ) => Promise<RuntimeCleanup>;
  minimizeWindow: () => Promise<unknown>;
  openExternalUrl: (url: string) => Promise<unknown>;
  openSettingsWindow: (
    target?: NativeSettingsWindowTarget,
    projectRoot?: string | null,
    workspaceSourcePath?: string | null
  ) => Promise<unknown>;
  requestPrimaryCloudNotebookCatalog: () => Promise<unknown>;
  markSettingsWindowReady: () => Promise<unknown>;
  hideSettingsWindow: () => Promise<unknown>;
  setEditorWindowRestoreState: (input: SetNativeEditorWindowRestoreStateInput) => Promise<unknown>;
  setWindowTitle: (title: string) => Promise<unknown>;
  showWindow: () => Promise<unknown>;
  toggleWindowFullscreen: () => Promise<unknown>;
  toggleWindowMaximized: () => Promise<unknown>;
};

export type AppRuntime = {
  dialog: AppDialogRuntime;
  events: AppEventsRuntime;
  features: AppFeatureRuntime;
  files: AppFileRuntime;
  logs: AppLogsRuntime;
  menu: AppMenuRuntime;
  mcp: AppMcpRuntime;
  navigation: AppNavigationRuntime;
  platform: AppPlatformRuntime;
  syncConfig: AppSyncConfigRuntime;
  settings: AppSettingsRuntime;
  shellCommand: AppShellCommandRuntime;
  systemFonts: AppSystemFontsRuntime;
  themes: AppThemeRuntime;
  updater: AppUpdaterRuntime;
  webResource: AppWebResourceRuntime;
  window: AppWindowRuntime;
  workspace: AppWorkspaceRuntime;
};

function jsonValuesEqual(left: unknown, right: unknown): boolean {
  if (Object.is(left, right)) return true;
  if (typeof left !== "object" || left === null || typeof right !== "object" || right === null) {
    return false;
  }
  if (Array.isArray(left) || Array.isArray(right)) {
    return Array.isArray(left) && Array.isArray(right) &&
      left.length === right.length &&
      left.every((value, index) => jsonValuesEqual(value, right[index]));
  }

  const leftRecord = left as Record<string, unknown>;
  const rightRecord = right as Record<string, unknown>;
  const leftKeys = Object.keys(leftRecord);
  const rightKeys = Object.keys(rightRecord);
  return leftKeys.length === rightKeys.length && leftKeys.every((key) =>
    Object.hasOwn(rightRecord, key) && jsonValuesEqual(leftRecord[key], rightRecord[key])
  );
}

function createMemorySettingsRuntime(): AppSettingsRuntime {
  const stores = new Map<string, Map<string, unknown>>();

  function settingsStore() {
    if (!stores.has("settings.json")) {
      stores.set("settings.json", new Map());
    }

    return stores.get("settings.json")!;
  }

  return {
    async loadStore(path, options) {
      if (!stores.has(path)) {
        stores.set(path, new Map(Object.entries(options.defaults)));
      }
      const store = stores.get(path)!;

      return {
        async delete(key) {
          store.delete(key);
        },
        async get<T>(key: string) {
          return store.get(key) as T | undefined;
        },
        async save() {
          return undefined;
        },
        async set(key, value) {
          store.set(key, value);
        }
      };
    },
    async readGroup<TValue>(group: AppSettingsGroup) {
      const store = settingsStore();
      if (group === "appearance") {
        const appearanceMode = store.get("appearanceMode");
        const lightTheme = store.get("lightThemeId") ?? store.get("lightTheme");
        const darkTheme = store.get("darkThemeId") ?? store.get("darkTheme");
        if (appearanceMode === undefined && lightTheme === undefined && darkTheme === undefined) {
          return undefined;
        }

        return {
          appearanceMode: appearanceMode ?? "system",
          darkTheme: darkTheme ?? "dark",
          lightTheme: lightTheme ?? "light"
        } as TValue;
      }
      const key = group === "language" ? "language" : group;

      return store.get(key) as TValue | undefined;
    },
    async readPrimaryWorkspaceState() {
      return stores.get("local-state.json")?.get("primaryWorkspace");
    },
    async writeGroup(group, value) {
      const store = settingsStore();
      if (group === "appearance") {
        const preferences = value as Record<string, unknown>;
        store.set("appearanceMode", preferences.appearanceMode);
        store.set("lightThemeId", preferences.lightTheme);
        store.set("darkThemeId", preferences.darkTheme);
        return undefined;
      }
      const key = group === "language" ? "language" : group;
      store.set(key, value);
      return undefined;
    },
    async writePrimaryWorkspaceState(input) {
      if (!stores.has("local-state.json")) stores.set("local-state.json", new Map());
      const store = stores.get("local-state.json")!;
      const current = store.get("primaryWorkspace");
      if (
        input.expectedState !== undefined &&
        !jsonValuesEqual(current, input.expectedState)
      ) {
        return { applied: false, state: current };
      }

      store.set("schemaVersion", 2);
      store.set("primaryWorkspace", input.state);
      return { applied: true, state: input.state };
    }
  };
}

function unsupportedFeature(feature: string): Promise<never> {
  return Promise.reject(new Error(`${feature} is unavailable without a configured app runtime.`));
}

async function readBrowserClipboardText() {
  const clipboard = typeof navigator === "undefined" ? null : navigator.clipboard;
  const readText = clipboard?.readText;
  if (typeof readText !== "function") return null;

  try {
    return await readText.call(clipboard);
  } catch {
    return null;
  }
}

function createDefaultFileRuntime(): AppFileRuntime {
  return {
    confirmMarkdownFileDelete: async () => false,
    confirmWorkspaceResourceTrash: async () => false,
    confirmUnsavedMarkdownDocumentDiscard: async () => false,
    createMarkdownTreeFile: () => unsupportedFeature("createMarkdownTreeFile"),
    createMarkdownTreeFolder: () => unsupportedFeature("createMarkdownTreeFolder"),
    deleteMarkdownTemplateFile: () => unsupportedFeature("deleteMarkdownTemplateFile"),
    deleteMarkdownTreeFile: () => unsupportedFeature("deleteMarkdownTreeFile"),
    detectPandocPath: async () => null,
    installMarkdownFileDrop: async () => () => undefined,
    importLocalFile: () => unsupportedFeature("importLocalFile"),
    listenOpenedMarkdownPaths: async () => () => undefined,
    listMarkdownFileHistory: async () => [],
    listMarkdownFilesForPath: async () => [],
    moveMarkdownTreeFile: () => unsupportedFeature("moveMarkdownTreeFile"),
    openContainingFolder: () => unsupportedFeature("openContainingFolder"),
    openLocalImages: async () => [],
    openLocalFiles: async () => [],
    openMarkdownAttachment: () => unsupportedFeature("openMarkdownAttachment"),
    openMarkdownFile: async () => null,
    openMarkdownFileInNewWindow: () => unsupportedFeature("openMarkdownFileInNewWindow"),
    openMarkdownFolder: async () => null,
    openSettingsFile: async () => null,
    readLocalImageFile: () => unsupportedFeature("readLocalImageFile"),
    readMarkdownFile: () => unsupportedFeature("readMarkdownFile"),
    readMarkdownFileHistory: () => unsupportedFeature("readMarkdownFileHistory"),
    readMarkdownTemplateFile: () => unsupportedFeature("readMarkdownTemplateFile"),
    renameMarkdownTreeFile: () => unsupportedFeature("renameMarkdownTreeFile"),
    resolveMarkdownFolder: () => unsupportedFeature("resolveMarkdownFolder"),
    resolveMarkdownPath: () => unsupportedFeature("resolveMarkdownPath"),
    resolveWorkspaceResourceRoot: () => unsupportedFeature("resolveWorkspaceResourceRoot"),
    saveClipboardAttachment: () => unsupportedFeature("saveClipboardAttachment"),
    saveClipboardImage: () => unsupportedFeature("saveClipboardImage"),
    saveHtmlFile: async () => null,
    saveMarkdownFile: async () => null,
    savePandocFile: async () => null,
    savePdfFile: async () => null,
    saveSettingsFile: async () => null,
    takeOpenedMarkdownPaths: async () => [],
    trashWorkspaceResources: () => unsupportedFeature("trashWorkspaceResources"),
    watchMarkdownFile: async () => () => undefined,
    watchMarkdownTree: async () => () => undefined,
    writeMarkdownTemplateFile: () => unsupportedFeature("writeMarkdownTemplateFile")
  };
}

export function createDefaultAppRuntime(): AppRuntime {
  let syncEditingCounter = 0;
  let syncEditingState: Awaited<ReturnType<AppSyncConfigRuntime["loadEditing"]>>["state"] = null;
  let syncPendingApply: Awaited<ReturnType<AppSyncConfigRuntime["requestApply"]>>["event"] | null = null;

  return {
    dialog: {
      showAppAbout: async () => undefined,
      showPandocSetup: async () => "cancel"
    },
    events: {
      emit: async () => undefined,
      isAvailable: () => false,
      listen: async () => () => undefined
    },
    features: {
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
    },
    files: createDefaultFileRuntime(),
    logs: {
      isAvailable: () => false,
      openLogFolder: () => unsupportedFeature("openLogFolder"),
      writeLog: async (_event: AppLogEvent) => undefined
    },
    menu: {
      createEditorContextMenuItems: () => [],
      createMarkdownFileTreeContextMenuItems: () => [],
      installApplicationMenu: async () => () => undefined,
      installEditorContextMenu: async () => () => undefined,
      listenApplicationMenuCommands: async () => () => undefined,
      readClipboardText: readBrowserClipboardText,
      showMarkdownFileTreeContextMenu: async () => undefined
    },
    mcp: {
      policyAvailable: false,
      localServiceAvailable: false,
      setPrimaryWorkspace: () => unsupportedFeature("setMcpPrimaryWorkspace"),
      getSettings: () => unsupportedFeature("getMcpSettings"),
      updateSettings: () => unsupportedFeature("updateMcpSettings"),
      getHealth: () => unsupportedFeature("getMcpHealth"),
      listAuditEntries: async () => [],
      clearAuditEntries: () => unsupportedFeature("clearMcpAuditEntries")
    },
    navigation: {
      subscribeToSystemBack: async () => () => undefined
    },
    platform: {
      resolveDesktopOsVersion: () => null,
      resolveDesktopPlatform: () => null,
      resolveFormFactor: () => "desktop"
    },
    settings: createMemorySettingsRuntime(),
    syncConfig: {
      cancelApply: async (input) => {
        if (!syncPendingApply) {
          throw new Error("sync-apply-unavailable: The sync settings apply is unavailable.");
        }
        if (
          syncPendingApply.revision !== input.revision ||
          syncPendingApply.sessionId !== input.sessionId ||
          syncPendingApply.token !== input.token
        ) {
          throw new Error("sync-apply-mismatch: The sync settings apply identity changed.");
        }
        if (syncPendingApply.state !== "completed") {
          syncEditingCounter += 1;
          syncPendingApply = {
            ...syncPendingApply,
            counter: syncEditingCounter,
            state: "completed"
          };
        }
        return { broadcasted: false, event: syncPendingApply };
      },
      enable: () => unsupportedFeature("enableSyncConfig"),
      load: () => unsupportedFeature("loadSyncConfig"),
      listNotebooks: () => unsupportedFeature("listRemoteNotebooks"),
      loadEditing: async () => ({
        counter: syncEditingCounter,
        pendingApply: syncPendingApply,
        state: syncEditingState
      }),
      loadStatus: () => unsupportedFeature("loadSyncStatus"),
      patch: () => unsupportedFeature("patchSyncConfig"),
      recover: () => unsupportedFeature("recoverSyncConfig"),
      requestApply: async (input) => {
        if (!input.token.trim()) {
          throw new Error("sync-apply-session-mismatch: The sync settings session is unavailable.");
        }
        if (syncPendingApply?.token === input.token) {
          if (
            syncPendingApply.revision !== input.revision ||
            syncPendingApply.sessionId !== input.sessionId ||
            syncPendingApply.source !== input.source ||
            syncPendingApply.exitReason !== input.exitReason
          ) {
            throw new Error("sync-apply-mismatch: The sync settings apply identity changed.");
          }
          syncEditingCounter += 1;
          syncPendingApply = { ...syncPendingApply, counter: syncEditingCounter };
          return { broadcasted: false, event: syncPendingApply };
        }
        if (!input.revision.trim() || syncEditingState?.sessionId !== input.sessionId) {
          throw new Error("sync-apply-session-mismatch: The sync settings session is unavailable.");
        }
        if (syncPendingApply && syncPendingApply.state !== "completed") {
          throw new Error("sync-apply-pending: Another sync settings apply is pending.");
        }
        syncEditingCounter += 1;
        syncPendingApply = {
          ...input,
          counter: syncEditingCounter,
          state: "pending"
        };
        return { broadcasted: false, event: syncPendingApply };
      },
      reset: () => unsupportedFeature("resetSyncConfig"),
      setEditing: async (input) => {
        syncEditingCounter += 1;
        if (input.active) {
          if (syncPendingApply?.state === "completed") syncPendingApply = null;
          syncEditingState = {
            revision: input.revision,
            sessionId: input.sessionId
          };
        } else if (syncEditingState?.sessionId === input.sessionId) {
          syncEditingState = null;
        }
        return {
          broadcasted: false,
          event: syncEditingState
            ? { ...syncEditingState, active: true, counter: syncEditingCounter }
            : { ...input, active: false, counter: syncEditingCounter }
        };
      },
      sync: () => unsupportedFeature("syncApplication"),
      testConnection: () => unsupportedFeature("testSyncConnection")
    },
    shellCommand: {
      getShellCommandStatus: async () => ({ commandPath: null, targetPath: null, status: "unavailable" }),
      installShellCommand: async () => ({ commandPath: null, targetPath: null, status: "unavailable" }),
      uninstallShellCommand: async () => ({ commandPath: null, targetPath: null, status: "unavailable" })
    },
    systemFonts: {
      listFontFamilies: async () => []
    },
    themes: {
      cancelActivation: () => unsupportedFeature("cancelThemeActivation"),
      capabilities: {
        canDelete: false,
        canImport: false,
        canOpenDirectory: false
      },
      commitActivation: () => unsupportedFeature("commitThemeActivation"),
      confirmActivation: async () => false,
      delete: () => unsupportedFeature("deleteTheme"),
      importFile: () => unsupportedFeature("importTheme"),
      list: async () => ({ invalidFiles: [], themes: [] }),
      openDirectory: () => unsupportedFeature("openThemeDirectory"),
      prepareActivation: () => unsupportedFeature("prepareThemeActivation"),
      releaseActivation: () => unsupportedFeature("releaseThemeActivation"),
      replaceFile: () => unsupportedFeature("replaceTheme")
    },
    updater: {
      checkAppUpdate: async () => null
    },
    webResource: {
      downloadImage: () => unsupportedFeature("downloadWebImage")
    },
    window: {
      acknowledgeSettingsWindowHide: async () => undefined,
      cancelSettingsWindowHide: async () => undefined,
      closeWindow: async () => undefined,
      completeSettingsWindowHide: async () => undefined,
      destroyWindow: async () => undefined,
      exitApp: async () => undefined,
      getCurrentWindowLabel: async () => "main",
      listEditorWindowRestoreStates: async () => [],
      listenAppExitRequested: async () => () => undefined,
      listenSettingsWindowHideRequested: async () => () => undefined,
      listenSettingsWindowTarget: async () => () => undefined,
      listenWindowCloseRequested: async () => () => undefined,
      minimizeWindow: async () => undefined,
      openExternalUrl: async (url) => {
        if (typeof window !== "undefined") {
          window.open(url, "_blank", "noopener,noreferrer");
        }
      },
      openSettingsWindow: async () => undefined,
      requestPrimaryCloudNotebookCatalog: () => unsupportedFeature("requestPrimaryCloudNotebookCatalog"),
      markSettingsWindowReady: async () => undefined,
      hideSettingsWindow: async () => undefined,
      setEditorWindowRestoreState: async () => undefined,
      setWindowTitle: async (title) => {
        if (typeof document !== "undefined") {
          document.title = title;
        }
      },
      showWindow: async () => undefined,
      toggleWindowFullscreen: async () => undefined,
      toggleWindowMaximized: async () => undefined
    },
    workspace: {
      isDocumentInRoot: async () => false,
      listManagedNotebookNames: async () => [],
      resolveManagedRoot: async (_name) => null
    }
  };
}

let appRuntime = createDefaultAppRuntime();

export function configureAppRuntime(runtime: AppRuntime) {
  appRuntime = runtime;
  setAppLogBackendWriter(runtime.logs.writeLog);
}

export function getAppRuntime() {
  return appRuntime;
}

export function resetAppRuntimeForTests() {
  appRuntime = createDefaultAppRuntime();
  setAppLogBackendWriter(appRuntime.logs.writeLog);
}

export type {
  AppTheme,
  EditorPreferences,
  ExportSettings,
  PrimaryWorkspaceState,
  RecentMarkdownFile
} from "../lib/settings/app-settings";
export type {
  InvalidThemeFile,
  MergedThemeCatalog,
  ThemeActivationPayload,
  ThemeAppearance,
  ThemeCatalogSnapshot,
  ThemeDescriptor,
  ThemeImportResult,
  ThemePreview,
  ThemeRuntimeCapabilities,
  ThemeStorageKind
} from "../lib/themes/theme-catalog";
export {
  notebookNameFromRoot,
  normalizeSyncConfigLoadResult,
  type AppSyncConfigRuntime,
  type RemoteNotebookCatalogEntry,
  type QingYuSyncConfig,
  type SyncApplyUpdate,
  type SyncApplyWriteResult,
  type SyncConfigDocument,
  type SyncConfigIssue,
  type SyncConfigLoadIssue,
  type SyncConfigLoadResult,
  type SyncConfigPatch,
  type SyncConfigReadiness,
  type SyncConnectionTestResult,
  type SyncEditingEvent,
  type SyncEditingSnapshot,
  type SyncEditingUpdate,
  type SyncEditingWriteResult,
  type SyncPendingApply,
  type SyncProvider,
  type SyncRunRequest,
  type SyncRunResult,
  type SyncSafeError,
  type SyncStatus,
  type SyncSummary,
  type SyncTrigger
} from "../lib/sync-config";
export type {
  CreateNativeMarkdownTreeFileOptions,
  DownloadNativeWebImageInput,
  ImportNativeLocalFileInput,
  ListNativeMarkdownFilesOptions,
  NativeMarkdownDroppedTarget,
  NativeMarkdownFile,
  NativeMarkdownFileChangeHandler,
  NativeMarkdownFileDropHandler,
  NativeMarkdownFolder,
  NativeMarkdownFolderFile,
  NativeMarkdownPickerLabels,
  NativeLocalFile,
  NativeSettingsFile,
  NativeMarkdownTreeChangeHandler,
  NativePandocExportFormat,
  OpenNativeMarkdownAttachmentInput,
  SavedNativeClipboardAttachment,
  SavedNativeClipboardImage,
  SavedNativeHtmlFile,
  SavedNativeMarkdownFile,
  SavedNativePandocFile,
  SavedNativePdfFile,
  SavedNativeSettingsFile,
  SaveNativeClipboardAttachmentInput,
  SaveNativeClipboardImageInput,
  SaveNativeHtmlFileInput,
  SaveNativeMarkdownFileInput,
  SaveNativePandocFileInput,
  SaveNativePdfFileInput,
  SaveNativeSettingsFileInput,
  TrashWorkspaceResourceInput,
  TrashWorkspaceResourceResult
} from "../lib/tauri/file";
export type {
  ContextMenuEntry,
  ContextMenuPosition,
  ContextMenuProps,
  ShowContextMenuOptions
} from "../components/ContextMenu";
export {
  closeActiveContextMenu,
  contextMenuItem,
  contextMenuPositionFromEvent,
  contextMenuSeparator,
  contextMenuSubmenu,
  currentContextMenuPosition,
  showContextMenu
} from "../components/ContextMenu";
export {
  createEditorContextMenuEntries,
  createEditorContextMenuEntriesFromOptions,
  createMarkdownFileTreeContextMenuEntries,
  nativeAcceleratorsForMarkdownShortcuts,
  type ContextMenuIdPrefixes
} from "./context-menu-items";
export type {
  NativeEditorContextMenuOptions,
  NativeMarkdownFileTreeContextMenuHandlers,
  NativeMenuCommand,
  NativeMenuHandlers
} from "../lib/tauri/menu";
export type { NativeAppUpdate, NativeAppUpdateProgress } from "../lib/tauri/updater";
export type { NativePandocSetupAction } from "../lib/tauri/dialog";
export type { NativeShellCommandStatus } from "../lib/tauri/shell-command";
export type {
  NativeEditorWindowRestoreState,
  NativeSettingsWindowTarget,
  SetNativeEditorWindowRestoreStateInput
} from "../lib/tauri/window";
