import { emit } from "@tauri-apps/api/event";
import { platform as tauriPlatform, version as tauriVersion, type Platform as TauriPlatform } from "@tauri-apps/plugin-os";
import { load } from "@tauri-apps/plugin-store";
import { hasTauriRuntime } from "@markra/shared";
import type { AppFormFactor, AppRuntime } from "@markra/app/runtime";
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
import * as shellCommand from "./tauri/shell-command";
import * as themes from "./tauri/themes";
import * as updater from "./tauri/updater";
import * as webResource from "./tauri/web-resource";
import * as windowRuntime from "./tauri/window";
import { listenNativeEvent } from "./tauri/events";

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

export const desktopRuntime = {
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
    moveMarkdownTreeFile: files.moveNativeMarkdownTreeFile,
    openContainingFolder: files.openNativeContainingFolder,
    openLocalImages: files.openNativeLocalImages,
    openLocalFiles: files.openNativeLocalFiles,
    openMarkdownAttachment: files.openNativeMarkdownAttachment,
    openMarkdownFile: files.openNativeMarkdownFile,
    openMarkdownFileInNewWindow: files.openNativeMarkdownFileInNewWindow,
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
    saveHtmlFile: files.saveNativeHtmlFile,
    saveMarkdownFile: files.saveNativeMarkdownFile,
    savePandocFile: files.saveNativePandocFile,
    savePdfFile: files.saveNativePdfFile,
    saveSettingsFile: files.saveNativeSettingsFile,
    searchMarkdownFiles: files.searchNativeMarkdownFilesForPath,
    takeOpenedMarkdownPaths: files.takeNativeOpenedMarkdownPaths,
    trashWorkspaceResources: files.trashNativeWorkspaceResources,
    watchMarkdownFile: files.watchNativeMarkdownFile,
    watchMarkdownTree: files.watchNativeMarkdownTree,
    writeMarkdownTemplateFile: files.writeNativeMarkdownTemplateFile
  },
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
    setPrimaryWorkspace: mcp.setNativeMcpPrimaryWorkspace,
    clearAuditEntries: mcp.clearNativeMcpAuditEntries,
    getHealth: mcp.getNativeMcpHealth,
    getSettings: mcp.getNativeMcpSettings,
    listAuditEntries: mcp.listNativeMcpAuditEntries,
    updateSettings: mcp.updateNativeMcpSettings
  },
  navigation: {
    subscribeToSystemBack: async (_handler) => () => undefined
  },
  platform: {
    resolveDesktopOsVersion,
    resolveDesktopPlatform,
    resolveFormFactor
  },
  settings: {
    loadStore: load,
    readPrimaryWorkspaceState: settings.readNativePrimaryWorkspaceState,
    readGroup: settings.readNativeAppSettingsGroup,
    replacePortable: settings.replaceNativePortableAppSettings,
    writePrimaryWorkspaceState: settings.writeNativePrimaryWorkspaceState,
    writeGroup: settings.writeNativeAppSettingsGroup
  },
  syncConfig: {
    cancelApply: syncConfig.cancelNativeSyncConfigApply,
    enable: syncConfig.enableNativeSyncConfig,
    load: syncConfig.loadNativeSyncConfig,
    listNotebooks: syncConfig.listNativeNotebooks,
    loadEditing: syncConfig.loadNativeSyncConfigEditing,
    loadStatus: syncConfig.loadNativeSyncStatus,
    patch: syncConfig.patchNativeSyncConfig,
    recover: syncConfig.recoverNativeSyncConfig,
    requestApply: syncConfig.requestNativeSyncConfigApply,
    reset: syncConfig.resetNativeSyncConfig,
    setEditing: syncConfig.setNativeSyncConfigEditing,
    sync: syncConfig.syncApplication,
    testConnection: syncConfig.testSyncConnection
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
    confirmActivation: themes.confirmNativeThemeActivation,
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
    openExternalUrl: opener.openNativeExternalUrl,
    openSettingsWindow: windowRuntime.openSettingsWindow,
    requestPrimaryCloudNotebookCatalog: windowRuntime.requestNativePrimaryCloudNotebookCatalog,
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
} satisfies AppRuntime;
