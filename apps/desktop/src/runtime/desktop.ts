import { emit } from "@tauri-apps/api/event";
import { platform as tauriPlatform, version as tauriVersion, type Platform as TauriPlatform } from "@tauri-apps/plugin-os";
import { hasTauriRuntime } from "@markra/shared";
import {
  createDefaultAppRuntime,
  createKernelFileRuntimeOwner,
  createKernelSettingsRuntime,
  createKernelSyncConfigRuntime,
  createUnavailableKernelDomainPort,
  createUnavailableNativeShellPort,
  kernelWorkspaceRoot,
  type AppFormFactor,
  type AppRuntime,
  type KernelDomainPort,
  type NativeShellPort
} from "@markra/app/runtime";
import {
  readNativeKernelBootstrap,
  type NativeKernelBootstrap
} from "../kernel-bootstrap";
import { switchDesktopKernelWorkspace } from "../desktop-kernel-startup";
import { selectDesktopWorkspaceDirectory } from "../desktop-workspace-selector";
import {
  createDesktopKernelDomainAdapter,
  DesktopKernelDomainAdapterError,
  type DesktopKernelDomainAdapter
} from "./kernel";
import { createDesktopNativeShellPort } from "./native-shell";
import { createDesktopKernelImageSource } from "./kernel-image-source";
import * as dialog from "./tauri/dialog";
import * as files from "./tauri/file/desktop";
import * as fonts from "./tauri/fonts";
import * as logs from "./tauri/logs";
import * as managedWorkspace from "./tauri/managed-workspace";
import * as menu from "./tauri/menu";
import * as mcp from "./tauri/mcp";
import * as opener from "./tauri/opener";
import * as settings from "./tauri/settings";
import * as syncConfig from "./tauri/sync-config";
import * as syncPathGuard from "./tauri/sync-path-guard";
import * as shellCommand from "./tauri/shell-command";
import * as themes from "./tauri/themes";
import * as updater from "./tauri/updater";
import * as webResource from "./tauri/web-resource";
import * as windowRuntime from "./tauri/window";
import { listenNativeEvent } from "./tauri/events";
import { loadDesktopRuntimeStore } from "./tauri/runtime-store";

type DesktopPlatform = "macos" | "windows" | "linux";

function normalizeDesktopPlatform(platform: string | null | undefined): DesktopPlatform | null {
  if (platform === "windows" || platform === "macos" || platform === "linux") {
    return platform;
  }

  return null;
}

function resolveDesktopPlatform() {
  try {
    return normalizeDesktopPlatform(tauriPlatform() satisfies TauriPlatform);
  } catch {
    return null;
  }
}

function resolveDesktopOsVersion() {
  try {
    return tauriVersion() || null;
  } catch {
    return null;
  }
}

export function normalizeAppFormFactor(platform: string | null | undefined): AppFormFactor {
  return platform === "android" || platform === "ios" ? "mobile" : "desktop";
}

function resolveFormFactor() {
  try {
    return normalizeAppFormFactor(tauriPlatform() satisfies TauriPlatform);
  } catch {
    return "desktop";
  }
}

export type DesktopRuntimeAdapters = {
  kernel: KernelDomainPort;
  nativeShell: NativeShellPort;
};

export interface DesktopRuntimeCompositionOptions {
  readonly createKernelDomainAdapter?: (
    bootstrap: NativeKernelBootstrap
  ) => Promise<DesktopKernelDomainAdapter>;
  readonly readKernelBootstrap?: () => Promise<NativeKernelBootstrap | null>;
}

export function createDesktopRuntime({
  kernel = createUnavailableKernelDomainPort(),
  nativeShell = createDesktopNativeShellPort()
}: Partial<DesktopRuntimeAdapters> = {}): AppRuntime {
  return {
  dialog: {
    showAppAbout: dialog.showNativeAppAbout,
    showPandocSetup: dialog.showNativePandocSetup
  },
  events: {
    emit,
    isAvailable: hasTauriRuntime,
    listen: listenNativeEvent
  },
  features: {
    applicationMenu: true,
    applicationShortcuts: true,
    dejavuSync: true,
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
  },
  files: {
    confirmMarkdownFileDelete: files.confirmNativeMarkdownFileDelete,
    confirmWorkspaceResourceTrash: files.confirmNativeWorkspaceResourceTrash,
    confirmUnsavedMarkdownDocumentDiscard: files.confirmNativeUnsavedMarkdownDocumentDiscard,
    createMarkdownTreeFile: files.createNativeMarkdownTreeFile,
    createMarkdownTreeFolder: files.createNativeMarkdownTreeFolder,
    deleteMarkdownTemplateFile: files.deleteNativeMarkdownTemplateFile,
    deleteMarkdownTreeFile: files.deleteNativeMarkdownTreeFile,
    detectPandocPath: files.detectNativePandocPath,
    installMarkdownFileDrop: files.installNativeMarkdownFileDrop,
    importLocalFile: files.importNativeLocalFile,
    listenOpenedMarkdownPaths: files.listenNativeOpenedMarkdownPaths,
    listMarkdownFileHistory: files.listNativeMarkdownFileHistory,
    loadMarkdownFilesForPath: files.loadNativeMarkdownFilesForPath,
    listMarkdownFilesForPath: files.listNativeMarkdownFilesForPath,
    listMarkdownReferenceFilesForPath: files.listNativeMarkdownReferenceFilesForPath,
    moveMarkdownTreeFile: files.moveNativeMarkdownTreeFile,
    openContainingFolder: files.openNativeContainingFolder,
    openLocalImages: files.openNativeLocalImages,
    openLocalFiles: files.openNativeLocalFiles,
    openMarkdownAttachment: files.openNativeMarkdownAttachment,
    openMarkdownFile: files.openNativeMarkdownFile,
    openMarkdownFileInNewWindow: files.openNativeMarkdownFileInNewWindow,
    openMarkdownFolderInNewWindow: files.openNativeMarkdownFolderInNewWindow,
    openMarkdownFolder: files.openNativeMarkdownFolder,
    openSettingsFile: files.openNativeSettingsFile,
    readLocalImageFile: files.readNativeLocalImageFile,
    readMarkdownFile: files.readNativeMarkdownFile,
    readMarkdownFileHistory: files.readNativeMarkdownFileHistory,
    readMarkdownTemplateFile: files.readNativeMarkdownTemplateFile,
    renameMarkdownTreeFile: files.renameNativeMarkdownTreeFile,
    resolveMarkdownFolder: files.resolveNativeMarkdownFolder,
    resolveMarkdownPath: files.resolveNativeMarkdownPath,
    resolveWorkspaceResourceRoot: files.resolveNativeWorkspaceResourceRoot,
    requestPrimaryNotebookSwitch: files.requestNativePrimaryNotebookSwitch,
    saveClipboardAttachment: files.saveNativeClipboardAttachment,
    saveClipboardImage: files.saveNativeClipboardImage,
    saveClipboardImages: (inputs) => Promise.all(inputs.map(files.saveNativeClipboardImage)),
    saveHtmlFile: files.saveNativeHtmlFile,
    saveMarkdownFile: files.saveNativeMarkdownFile,
    savePandocFile: files.saveNativePandocFile,
    savePdfFile: files.saveNativePdfFile,
    saveSettingsFile: files.saveNativeSettingsFile,
    searchMarkdownFiles: files.searchNativeMarkdownFilesForPath,
    takeOpenedMarkdownPaths: files.takeNativeOpenedMarkdownPaths,
    trashMarkdownAssets: files.trashNativeMarkdownAssets,
    trashWorkspaceResources: files.trashNativeWorkspaceResources,
    watchMarkdownFile: files.watchNativeMarkdownFile,
    watchMarkdownTree: files.watchNativeMarkdownTree,
    writeMarkdownTemplateFile: files.writeNativeMarkdownTemplateFile
  },
  kernel,
  logs: {
    isAvailable: logs.isNativeLoggingAvailable,
    openLogFolder: logs.openNativeLogFolder,
    writeLog: logs.writeNativeLog
  },
  menu: {
    createEditorContextMenuItems: menu.createNativeEditorContextMenuItems,
    createMarkdownFileTreeContextMenuItems: menu.createNativeMarkdownFileTreeContextMenuItems,
    installApplicationMenu: menu.installNativeApplicationMenu,
    installEditorContextMenu: menu.installNativeEditorContextMenu,
    listenApplicationMenuCommands: menu.listenNativeApplicationMenuCommands,
    readClipboardText: menu.readNativeClipboardText,
    showMarkdownFileTreeContextMenu: menu.showNativeMarkdownFileTreeContextMenu
  },
  mcp: {
    policyAvailable: true,
    localServiceAvailable: true,
    clearAuditEntries: mcp.clearNativeMcpAuditEntries,
    getHealth: mcp.getNativeMcpHealth,
    getSettings: mcp.getNativeMcpSettings,
    listAuditEntries: mcp.listNativeMcpAuditEntries,
    updateSettings: mcp.updateNativeMcpSettings
  },
  navigation: {
    subscribeToSystemBack: async (_handler) => () => undefined
  },
  nativeShell,
  platform: {
    resolveDesktopOsVersion,
    resolveDesktopPlatform,
    resolveFormFactor
  },
  settings: {
    loadStore: loadDesktopRuntimeStore,
    readPrimaryWorkspaceState: settings.readNativePrimaryWorkspaceState,
    readGroup: settings.readNativeAppSettingsGroup,
    replacePortable: settings.replaceNativePortableAppSettings,
    writePrimaryWorkspaceState: settings.writeNativePrimaryWorkspaceState,
    writeGroup: settings.writeNativeAppSettingsGroup
  },
  syncConfig: {
    bindRepository: syncConfig.bindNativeDejavuRepository,
    cancelApply: syncConfig.cancelNativeSyncConfigApply,
    changeGlobalKey: syncConfig.changeNativeDejavuGlobalKey,
    deleteRemoteRepository: syncConfig.deleteNativeDejavuRemoteRepository,
    enable: syncConfig.enableNativeSyncConfig,
    exportGlobalKey: syncConfig.exportNativeDejavuGlobalKey,
    initializeGlobalKey: syncConfig.initializeNativeDejavuGlobalKey,
    load: syncConfig.loadNativeSyncConfig,
    loadKeyState: syncConfig.loadNativeDejavuKeyState,
    listNotebooks: syncConfig.listNativeNotebooks,
    listDejavuConflictHistory: syncConfig.listNativeDejavuConflictHistory,
    loadEditing: syncConfig.loadNativeSyncConfigEditing,
    loadRepositoryStatus: syncConfig.loadNativeDejavuRepositoryStatus,
    loadStatus: syncConfig.loadNativeSyncStatus,
    patch: syncConfig.patchNativeSyncConfig,
    purgeRemoteRepository: syncConfig.purgeNativeDejavuRemoteRepository,
    recover: syncConfig.recoverNativeSyncConfig,
    requestApply: syncConfig.requestNativeSyncConfigApply,
    readDejavuConflictHistory: syncConfig.readNativeDejavuConflictHistory,
    rebuildLocalRepository: syncConfig.rebuildNativeDejavuLocalRepository,
    reset: syncConfig.resetNativeSyncConfig,
    setEditing: syncConfig.setNativeSyncConfigEditing,
    settleApply: syncConfig.settleNativeKernelSyncConfigApply,
    stopRepositorySync: syncConfig.stopNativeDejavuRepositorySync,
    sync: syncConfig.syncApplication,
    testConnection: syncConfig.testSyncConnection
  },
  syncPathGuard: {
    acknowledge: syncPathGuard.acknowledgeNativePathGuard
  },
  shellCommand: {
    getShellCommandStatus: shellCommand.getNativeShellCommandStatus,
    installShellCommand: shellCommand.installNativeShellCommand,
    uninstallShellCommand: shellCommand.uninstallNativeShellCommand
  },
  systemFonts: {
    listFontFamilies: fonts.listNativeSystemFontFamilies
  },
  themes: {
    cancelActivation: themes.cancelNativeThemeActivation,
    capabilities: {
      canDelete: true,
      canImport: true,
      canOpenDirectory: true
    },
    commitActivation: themes.commitNativeThemeActivation,
    delete: themes.deleteNativeTheme,
    importFile: themes.importNativeTheme,
    list: themes.listNativeThemes,
    openDirectory: themes.openNativeThemeDirectory,
    prepareActivation: themes.prepareNativeThemeActivation,
    releaseActivation: themes.releaseNativeThemeActivation,
    replaceFile: themes.replaceNativeTheme
  },
  updater: {
    checkAppUpdate: updater.checkNativeAppUpdate
  },
  webResource: {
    downloadImage: webResource.downloadNativeWebImage
  },
  window: {
    acknowledgeSettingsWindowHide: windowRuntime.acknowledgeSettingsWindowHide,
    cancelSettingsWindowHide: windowRuntime.cancelSettingsWindowHide,
    closeWindow: windowRuntime.closeNativeWindow,
    completeSettingsWindowHide: windowRuntime.completeSettingsWindowHide,
    destroyWindow: windowRuntime.destroyNativeWindow,
    exitApp: windowRuntime.exitNativeApp,
    getCurrentWindowLabel: windowRuntime.getCurrentNativeWindowLabel,
    listEditorWindowRestoreStates: windowRuntime.listNativeEditorWindowRestoreStates,
    listenAppExitRequested: windowRuntime.listenNativeAppExitRequested,
    listenSettingsWindowHideRequested: windowRuntime.listenNativeSettingsWindowHideRequested,
    listenSettingsWindowTarget: windowRuntime.listenNativeSettingsWindowTarget,
    listenWindowCloseRequested: windowRuntime.listenNativeWindowCloseRequested,
    minimizeWindow: windowRuntime.minimizeNativeWindow,
    openBlankEditorWindow: windowRuntime.openNativeBlankEditorWindow,
    openExternalUrl: opener.openNativeExternalUrl,
    openSettingsWindow: windowRuntime.openSettingsWindow,
    markSettingsWindowReady: windowRuntime.markSettingsWindowReady,
    hideSettingsWindow: windowRuntime.hideSettingsWindow,
    setEditorWindowRestoreState: windowRuntime.setNativeEditorWindowRestoreState,
    setWindowTitle: windowRuntime.setNativeWindowTitle,
    showWindow: windowRuntime.showNativeWindow,
    toggleWindowFullscreen: windowRuntime.toggleNativeWindowFullscreen,
    toggleWindowMaximized: windowRuntime.toggleNativeWindowMaximized
  },
  workspace: {
    discardPreparedDesktopNotebookTarget: managedWorkspace.discardNativePreparedDesktopNotebookTarget,
    isDocumentInRoot: managedWorkspace.isNativeDocumentInWorkspace,
    listManagedNotebookNames: managedWorkspace.listNativeManagedWorkspaceNames,
    prepareDesktopNotebookTarget: managedWorkspace.prepareNativeDesktopNotebookTarget,
    resolveManagedRoot: managedWorkspace.resolveNativeManagedWorkspaceRoot
  }
  };
}

export interface DesktopKernelRuntimeOwner {
  readonly runtime: AppRuntime;
  readonly release: () => undefined;
}

export interface DesktopKernelRuntimeOwnerOptions {
  readonly commitRoot?: (path: string) => Promise<unknown>;
  readonly selectRoot?: () => Promise<string | null>;
}

export function createDesktopKernelRuntimeOwner(
  kernel: KernelDomainPort,
  {
    commitRoot = switchDesktopKernelWorkspace,
    selectRoot = selectDesktopWorkspaceDirectory,
  }: DesktopKernelRuntimeOwnerOptions = {},
): DesktopKernelRuntimeOwner {
  const shell = createDesktopRuntime({ kernel });
  const unavailable = createDefaultAppRuntime();
  const fileOwner = createKernelFileRuntimeOwner(kernel, {
    imageSource: createDesktopKernelImageSource(),
    invalidations: kernel.invalidations,
    isTerminalError: (error) => error instanceof DesktopKernelDomainAdapterError,
    nativeShell: {
      confirmMarkdownFileDelete: shell.files.confirmMarkdownFileDelete,
      confirmUnsavedMarkdownDocumentDiscard: shell.files.confirmUnsavedMarkdownDocumentDiscard,
      detectPandocPath: shell.files.detectPandocPath,
      listenOpenedMarkdownPaths: shell.files.listenOpenedMarkdownPaths,
      openSettingsFile: shell.files.openSettingsFile,
      readMarkdownTemplateFile: shell.files.readMarkdownTemplateFile,
      saveHtmlFile: shell.files.saveHtmlFile,
      savePandocFile: shell.files.savePandocFile,
      savePdfFile: shell.files.savePdfFile,
      saveSettingsFile: shell.files.saveSettingsFile,
      takeOpenedMarkdownPaths: shell.files.takeOpenedMarkdownPaths,
    },
  });
  const runtime: AppRuntime = {
    ...shell,
    features: {
      ...shell.features,
      dejavuSync: false,
      fileDrop: false,
      imageImport: false,
      openLocalAttachments: false,
      projectSync: true,
      resources: false,
    },
    files: {
      ...fileOwner.files,
      requestPrimaryNotebookSwitch: undefined,
    },
    kernel,
    mcp: shell.mcp,
    nativeShell: createUnavailableNativeShellPort(),
    settings: createKernelSettingsRuntime(kernel, {
      local: {
        loadStore: shell.settings.loadStore,
        readPrimaryWorkspaceState: undefined,
        writePrimaryWorkspaceState: undefined,
      },
    }),
    syncConfig: createKernelSyncConfigRuntime(kernel, {
      local: {
        cancelApply: shell.syncConfig.cancelApply,
        loadEditing: shell.syncConfig.loadEditing,
        requestApply: shell.syncConfig.requestApply,
        setEditing: shell.syncConfig.setEditing,
        settleApply: shell.syncConfig.settleApply,
      },
    }),
    syncPathGuard: unavailable.syncPathGuard,
    workspace: {
      isDocumentInRoot: async (documentPath, rootPath) => (
        rootPath === kernelWorkspaceRoot && (
          documentPath === rootPath || documentPath.startsWith(`${rootPath}/`)
        )
      ),
      listManagedNotebookNames: async () => [],
      resolveManagedRoot: async () => null,
      rootPolicy: {
        canChooseLocalRoot: true,
        commitRoot: async (path) => {
          await commitRoot(path);
          return kernelWorkspaceRoot;
        },
        kind: "host-selectable",
        resolveRoot: async () => kernelWorkspaceRoot,
        selectRoot,
      },
    },
  };
  let active = true;
  return Object.freeze({
    runtime,
    release: () => {
      if (!active) return undefined;
      active = false;
      fileOwner.release();
      return undefined;
    },
  });
}

export async function loadDesktopRuntime({
  createKernelDomainAdapter = createDesktopKernelDomainAdapter,
  readKernelBootstrap = readNativeKernelBootstrap
}: DesktopRuntimeCompositionOptions = {}): Promise<AppRuntime> {
  const bootstrap = await readKernelBootstrap();

  if (bootstrap === null) {
    return createDesktopRuntime();
  }

  let adapter: DesktopKernelDomainAdapter;
  try {
    adapter = await createKernelDomainAdapter(bootstrap);
  } catch (cause: unknown) {
    try {
      bootstrap.release();
    } catch {
      // Credential release is best-effort while preserving the initialization failure.
    }
    throw cause;
  }

  try {
    const runtime = createDesktopRuntime({ kernel: adapter.port });
    window.addEventListener("pagehide", adapter.release, { once: true });
    return runtime;
  } catch (cause: unknown) {
    adapter.release();
    throw cause;
  }
}

export const desktopRuntime = createDesktopRuntime();
