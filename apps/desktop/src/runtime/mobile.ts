import { emit } from "@tauri-apps/api/event";
import { load } from "@tauri-apps/plugin-store";
import { createDefaultAppRuntime, type AppRuntime } from "@markra/app/runtime";
import { hasTauriRuntime } from "@markra/shared";
import { listenNativeEvent } from "./tauri/events";
import * as fileConfirm from "./tauri/file/confirm";
import * as mobileFiles from "./tauri/file/mobile";
import * as files from "./tauri/file/shared";
import * as logs from "./tauri/logs/shared";
import * as managedWorkspace from "./tauri/managed-workspace";
import * as mobileBack from "./tauri/mobile-back";
import * as mcpPolicy from "./tauri/mcp-policy";
import * as opener from "./tauri/opener";
import * as settings from "./tauri/settings";
import * as themes from "./tauri/themes/shared";
import * as syncConfig from "./tauri/sync-config/shared";
import * as syncPathGuard from "./tauri/sync-path-guard";
import * as webResource from "./tauri/web-resource";

const defaultRuntime = createDefaultAppRuntime();

export const mobileRuntime = {
  ...defaultRuntime,
  events: {
    emit,
    isAvailable: hasTauriRuntime,
    listen: listenNativeEvent
  },
  features: {
    applicationMenu: false,
    applicationShortcuts: false,
    export: false,
    fileDrop: false,
    imageImport: true,
    nativeWindowChrome: false,
    openLocalAttachments: false,
    pandoc: false,
    projectSync: true,
    resources: false,
    settingsWindow: false,
    systemFonts: false,
    updater: false
  },
  files: {
    ...defaultRuntime.files,
    confirmMarkdownFileDelete: fileConfirm.confirmNativeMarkdownFileDelete,
    confirmUnsavedMarkdownDocumentDiscard: fileConfirm.confirmNativeUnsavedMarkdownDocumentDiscard,
    createMarkdownTreeFile: files.createNativeMarkdownTreeFile,
    createMarkdownTreeFolder: files.createNativeMarkdownTreeFolder,
    deleteMarkdownTreeFile: files.deleteNativeMarkdownTreeFile,
    listMarkdownFileHistory: files.listNativeMarkdownFileHistory,
    listMarkdownFilesForPath: files.listNativeMarkdownFilesForPath,
    loadMarkdownFilesForPath: files.loadNativeMarkdownFilesForPath,
    moveMarkdownTreeFile: files.moveNativeMarkdownTreeFile,
    openLocalImages: mobileFiles.openMobileLocalImages,
    readMarkdownFile: files.readNativeMarkdownFile,
    readMarkdownFileHistory: files.readNativeMarkdownFileHistory,
    renameMarkdownTreeFile: files.renameNativeMarkdownTreeFile,
    saveClipboardImage: mobileFiles.saveMobileClipboardImage,
    saveMarkdownFile: files.saveNativeMarkdownFileInPlace,
    searchMarkdownFiles: files.searchNativeMarkdownFilesForPath,
    watchMarkdownFile: files.watchNativeMarkdownFile,
    watchMarkdownTree: files.watchNativeMarkdownTree
  },
  logs: {
    ...defaultRuntime.logs,
    isAvailable: logs.isNativeLoggingAvailable,
    writeLog: logs.writeNativeLog
  },
  mcp: {
    ...defaultRuntime.mcp,
    policyAvailable: true,
    localServiceAvailable: false,
    getSettings: mcpPolicy.getNativeMcpPolicySettings,
    updateSettings: mcpPolicy.updateNativeMcpPolicySettings
  },
  navigation: {
    subscribeToSystemBack: mobileBack.subscribeToMobileSystemBack
  },
  platform: {
    resolveDesktopOsVersion: () => null,
    resolveDesktopPlatform: () => null,
    resolveFormFactor: () => "mobile"
  },
  settings: {
    loadStore: load,
    readPrimaryWorkspaceState: settings.readNativePrimaryWorkspaceState,
    readGroup: settings.readNativeAppSettingsGroup,
    replacePortable: settings.replaceNativePortableAppSettings,
    writePrimaryWorkspaceState: settings.writeNativePrimaryWorkspaceState,
    writeGroup: settings.writeNativeAppSettingsGroup
  },
  themes: {
    ...defaultRuntime.themes,
    cancelActivation: themes.cancelNativeThemeActivation,
    capabilities: {
      canDelete: true,
      canImport: false,
      canOpenDirectory: false
    },
    commitActivation: themes.commitNativeThemeActivation,
    confirmActivation: themes.confirmNativeThemeActivation,
    delete: themes.deleteNativeTheme,
    list: themes.listNativeThemes,
    prepareActivation: themes.prepareNativeThemeActivation,
    releaseActivation: themes.releaseNativeThemeActivation
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
    stopRepositorySync: syncConfig.stopNativeDejavuRepositorySync,
    sync: syncConfig.syncApplication,
    testConnection: syncConfig.testSyncConnection
  },
  syncPathGuard: {
    acknowledge: syncPathGuard.acknowledgeNativePathGuard
  },
  window: {
    ...defaultRuntime.window,
    openExternalUrl: opener.openNativeExternalUrl
  },
  webResource: {
    downloadImage: webResource.downloadNativeWebImage
  },
  workspace: {
    isDocumentInRoot: managedWorkspace.isNativeDocumentInWorkspace,
    listManagedNotebookNames: managedWorkspace.listNativeManagedWorkspaceNames,
    resolveManagedRoot: managedWorkspace.resolveNativeManagedWorkspaceRoot
  }
} satisfies AppRuntime;
