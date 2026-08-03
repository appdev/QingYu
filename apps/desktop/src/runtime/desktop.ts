import { emit } from "@tauri-apps/api/event";
import { platform as tauriPlatform, version as tauriVersion, type Platform as TauriPlatform } from "@tauri-apps/plugin-os";
import { hasTauriRuntime } from "@markra/shared";
import {
  createDefaultAppRuntime,
  createKernelAppConfigRuntime,
  createKernelFileRuntimeOwner,
  createKernelSettingsRuntime,
  createKernelSyncConfigRuntime,
  createUnavailableKernelDomainPort,
  createUnavailableAppConfigRuntime,
  createUnavailableNativeShellPort,
  kernelWorkspaceRoot,
  relativePathFromServerPath,
  resolveServerMarkdownImagePath,
  type AppFormFactor,
  type AppRuntime,
  type KernelDocumentEntrySnapshot,
  type KernelDomainPort,
  type KernelResourceSnapshot,
  type KernelWorkspaceGeneration,
  type KernelWorkspaceRelativePath,
  type NativeShellPort,
  type SaveNativeMarkdownBundleFileInput,
  type SavedNativeMarkdownFile
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
  appConfig: kernel.availability === "available"
    ? createKernelAppConfigRuntime(kernel, windowRuntime.getCurrentNativeWindowLabel)
    : createUnavailableAppConfigRuntime(),
  dialog: {
    confirm: dialog.confirmNativeAction,
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
    localFileImport: true,
    markdownBundle: true,
    nativeWindowChrome: true,
    openLocalAttachments: true,
    pandoc: true,
    projectSync: true,
    resources: true,
    settingsWindow: true,
    standaloneDocuments: true,
    systemFonts: true,
    templateMutation: true,
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
    saveMarkdownBundleFile: files.saveNativeMarkdownBundleFile,
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
    loadStore: () => Promise.reject(new Error(
      "Renderer-local stores are unavailable for a Kernel-backed runtime."
    )),
    readPrimaryWorkspaceState: settings.readNativePrimaryWorkspaceState,
    writePrimaryWorkspaceState: settings.writeNativePrimaryWorkspaceState
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
    loadJob: () => Promise.reject(new Error("Kernel sync jobs are unavailable before the Kernel runtime is active.")),
    loadKeyState: syncConfig.loadNativeDejavuKeyState,
    listNotebooks: syncConfig.listNativeNotebooks,
    listDejavuConflictHistory: syncConfig.listNativeDejavuConflictHistory,
    loadEditing: syncConfig.loadNativeSyncConfigEditing,
    loadRepositoryBinding: syncConfig.loadNativeDejavuRepositoryBinding,
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
  readonly saveMarkdownBundleSnapshot?: typeof files.saveNativeMarkdownBundleSnapshotFile;
  readonly selectRoot?: () => Promise<string | null>;
}

function parentKernelPath(path: string) {
  const separator = path.lastIndexOf("/");
  return (separator < 0 ? "" : path.slice(0, separator)) as KernelWorkspaceRelativePath;
}

function sameKernelResource(
  left: KernelResourceSnapshot,
  right: KernelResourceSnapshot
) {
  return left.id === right.id &&
    left.kind === right.kind &&
    left.mediaType === right.mediaType &&
    left.name === right.name &&
    left.relativePath === right.relativePath &&
    left.revision === right.revision &&
    left.sizeBytes === right.sizeBytes &&
    left.workspaceGeneration === right.workspaceGeneration;
}

async function listKernelDocuments(
  kernel: KernelDomainPort,
  parent: KernelWorkspaceRelativePath,
  workspaceGeneration: KernelWorkspaceGeneration
) {
  const items: KernelDocumentEntrySnapshot[] = [];
  const cursors = new Set<string>();
  let cursor: Parameters<KernelDomainPort["documents"]["list"]>[0]["cursor"];
  do {
    const page = await kernel.documents.list({
      cursor,
      limit: 100,
      parent,
      workspaceGeneration
    });
    if (page.workspaceGeneration !== workspaceGeneration) {
      throw new Error("The Kernel workspace changed during Markdown export.");
    }
    items.push(...page.items);
    cursor = page.nextCursor ?? undefined;
    if (cursor !== undefined && cursors.has(cursor)) {
      throw new Error("The Kernel document listing could not be completed.");
    }
    if (cursor !== undefined) cursors.add(cursor);
  } while (cursor !== undefined);
  return items;
}

async function listKernelResources(
  kernel: KernelDomainPort,
  parent: KernelWorkspaceRelativePath,
  workspaceGeneration: KernelWorkspaceGeneration
) {
  const inventory = await kernel.resources.list({ parent, workspaceGeneration });
  if (inventory.workspaceGeneration !== workspaceGeneration) {
    throw new Error("The Kernel workspace changed during Markdown export.");
  }
  return inventory.items.flatMap((entry) => entry.entryType === "resource" ? [entry.resource] : []);
}

function bytesToBase64(bytes: Uint8Array) {
  const chunkSize = 32_766;
  let encoded = "";
  for (let offset = 0; offset < bytes.length; offset += chunkSize) {
    const chunk = bytes.subarray(offset, Math.min(offset + chunkSize, bytes.length));
    encoded += btoa(String.fromCharCode(...chunk));
  }
  return encoded;
}

export async function saveDesktopKernelMarkdownBundleFile(
  kernel: KernelDomainPort,
  input: SaveNativeMarkdownBundleFileInput,
  saveSnapshot: typeof files.saveNativeMarkdownBundleSnapshotFile = files.saveNativeMarkdownBundleSnapshotFile
): Promise<SavedNativeMarkdownFile | null> {
  if (!input.documentPath) {
    throw new Error("Current document must be a saved Markdown file.");
  }
  const workspace = await kernel.workspace.read();
  if (workspace.readiness !== "ready") {
    throw new Error("The Kernel workspace is unavailable for Markdown export.");
  }
  const workspaceGeneration = workspace.generation;
  const documentRelativePath = relativePathFromServerPath(input.documentPath);
  if (documentRelativePath === "") {
    throw new Error("Current document must be a saved Markdown file.");
  }
  const documentParent = parentKernelPath(documentRelativePath);
  const documents = await listKernelDocuments(kernel, documentParent, workspaceGeneration);
  const document = documents.find((entry) => (
    entry.kind === "file" && entry.relativePath === documentRelativePath
  ));
  if (!document) {
    throw new Error("Current document must be a saved Markdown file.");
  }

  const references = input.references.map((reference) => {
    const resourcePath = resolveServerMarkdownImagePath(input.documentPath as string, reference.href);
    if (!resourcePath) {
      throw new Error(`Markdown resource "${reference.href}" is not available in the Kernel workspace.`);
    }
    return { ...reference, resourcePath };
  });
  const paths = [...new Set(references.map((reference) => reference.resourcePath))];
  const parents = [...new Set(paths.map(parentKernelPath))];
  const initialByPath = new Map<string, KernelResourceSnapshot>();
  for (const parent of parents) {
    const resources = await listKernelResources(kernel, parent, workspaceGeneration);
    for (const resource of resources) initialByPath.set(resource.relativePath, resource);
  }

  const resources = [];
  for (const path of paths) {
    const resource = initialByPath.get(path);
    if (!resource) {
      throw new Error(`Markdown resource "${path}" is not available in the Kernel workspace.`);
    }
    const opened = await kernel.resources.open({
      id: resource.id,
      kind: resource.kind,
      workspaceGeneration
    });
    if (
      opened.revision !== resource.revision ||
      opened.mediaType !== resource.mediaType ||
      opened.body.size !== resource.sizeBytes
    ) {
      throw new Error(`Markdown resource "${path}" changed during export.`);
    }
    resources.push({
      bodyBase64: bytesToBase64(new Uint8Array(await opened.body.arrayBuffer())),
      name: resource.name,
      path
    });
  }

  for (const parent of parents) {
    const finalResources = await listKernelResources(kernel, parent, workspaceGeneration);
    const finalByPath = new Map<string, KernelResourceSnapshot>(
      finalResources.map((resource) => [resource.relativePath, resource])
    );
    for (const path of paths.filter((candidate) => parentKernelPath(candidate) === parent)) {
      const initial = initialByPath.get(path);
      const final = finalByPath.get(path);
      if (!initial || !final || !sameKernelResource(initial, final)) {
        throw new Error(`Markdown resource "${path}" changed during export.`);
      }
    }
  }
  const finalDocuments = await listKernelDocuments(kernel, documentParent, workspaceGeneration);
  const finalDocument = finalDocuments.find((entry) => entry.relativePath === documentRelativePath);
  if (!finalDocument || finalDocument.kind !== "file" || finalDocument.locator !== document.locator) {
    throw new Error("Current document changed location during Markdown export.");
  }
  const finalWorkspace = await kernel.workspace.read();
  if (
    finalWorkspace.id !== workspace.id ||
    finalWorkspace.generation !== workspaceGeneration ||
    finalWorkspace.readiness !== "ready"
  ) {
    throw new Error("The Kernel workspace changed during Markdown export.");
  }

  return saveSnapshot({
    folder: input.folder,
    markdown: input.markdown,
    references,
    resources,
    suggestedName: input.suggestedName
  });
}

export function createDesktopKernelRuntimeOwner(
  kernel: KernelDomainPort,
  {
    commitRoot = switchDesktopKernelWorkspace,
    saveMarkdownBundleSnapshot = files.saveNativeMarkdownBundleSnapshotFile,
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
      openLocalImages: shell.files.openLocalImages,
      openContainingFolder: shell.files.openContainingFolder,
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
      dejavuSync: true,
      fileDrop: false,
      imageImport: true,
      localFileImport: false,
      openLocalAttachments: false,
      projectSync: true,
      resources: false,
      standaloneDocuments: false,
      templateMutation: false,
    },
    files: {
      ...fileOwner.files,
      requestPrimaryNotebookSwitch: undefined,
      saveClipboardImage: unavailable.files.saveClipboardImage,
      saveMarkdownBundleFile: (input) => saveDesktopKernelMarkdownBundleFile(
        kernel,
        input,
        saveMarkdownBundleSnapshot
      ),
    },
    kernel,
    mcp: shell.mcp,
    nativeShell: createUnavailableNativeShellPort(),
    settings: createKernelSettingsRuntime(kernel, shell.appConfig.bootstrap.settings, {
      local: {
        readPrimaryWorkspaceState: shell.settings.readPrimaryWorkspaceState,
        writePrimaryWorkspaceState: shell.settings.writePrimaryWorkspaceState,
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
