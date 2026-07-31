import { emit } from "@tauri-apps/api/event";
import { load } from "@tauri-apps/plugin-store";
import {
  createDefaultAppRuntime,
  createKernelFileRuntimeOwner,
  createKernelSettingsRuntime,
  createKernelSyncConfigRuntime,
  kernelWorkspaceRoot,
  type AppRuntime,
  type KernelDomainPort,
} from "@markra/app/runtime";
import { hasTauriRuntime } from "@markra/shared";

import { createKernelObjectUrlImageSource } from "./kernel-object-url-image-source";
import type { NativeKernelBootstrap } from "../kernel-bootstrap";
import {
  createDesktopKernelDomainAdapter,
  type DesktopKernelDomainAdapter,
  type DesktopKernelDomainAdapterOptions,
} from "./kernel";
import { listenNativeEvent } from "./tauri/events";
import * as fileConfirm from "./tauri/file/confirm";
import * as mobileFiles from "./tauri/file/mobile";
import * as logs from "./tauri/logs/shared";
import * as mobileBack from "./tauri/mobile-back";
import * as mcpPolicy from "./tauri/mcp-policy";
import * as opener from "./tauri/opener";
import * as themes from "./tauri/themes/shared";

const defaultRuntime = createDefaultAppRuntime();

export const mobileRuntime = {
  ...defaultRuntime,
  events: {
    emit,
    isAvailable: hasTauriRuntime,
    listen: listenNativeEvent,
  },
  features: {
    applicationMenu: false,
    applicationShortcuts: false,
    dejavuSync: false,
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
    updater: false,
  },
  files: {
    ...defaultRuntime.files,
    confirmMarkdownFileDelete: fileConfirm.confirmNativeMarkdownFileDelete,
    confirmUnsavedMarkdownDocumentDiscard:
      fileConfirm.confirmNativeUnsavedMarkdownDocumentDiscard,
    openLocalImages: mobileFiles.openMobileLocalImages,
  },
  logs: {
    ...defaultRuntime.logs,
    isAvailable: logs.isNativeLoggingAvailable,
    writeLog: logs.writeNativeLog,
  },
  mcp: {
    ...defaultRuntime.mcp,
    policyAvailable: true,
    localServiceAvailable: false,
    getSettings: mcpPolicy.getNativeMcpPolicySettings,
    updateSettings: mcpPolicy.updateNativeMcpPolicySettings,
  },
  navigation: {
    subscribeToSystemBack: mobileBack.subscribeToMobileSystemBack,
  },
  platform: {
    resolveDesktopOsVersion: () => null,
    resolveDesktopPlatform: () => null,
    resolveFormFactor: () => "mobile",
  },
  settings: {
    ...defaultRuntime.settings,
    loadStore: load,
  },
  themes: {
    ...defaultRuntime.themes,
    cancelActivation: themes.cancelNativeThemeActivation,
    capabilities: {
      canDelete: true,
      canImport: false,
      canOpenDirectory: false,
    },
    commitActivation: themes.commitNativeThemeActivation,
    delete: themes.deleteNativeTheme,
    list: themes.listNativeThemes,
    prepareActivation: themes.prepareNativeThemeActivation,
    releaseActivation: themes.releaseNativeThemeActivation,
  },
  window: {
    ...defaultRuntime.window,
    openExternalUrl: opener.openNativeExternalUrl,
  },
} satisfies AppRuntime;

export interface MobileKernelRuntimeOwner {
  readonly runtime: AppRuntime;
  readonly release: () => undefined;
}

export function createMobileKernelDomainAdapter(
  bootstrap: NativeKernelBootstrap,
  options: DesktopKernelDomainAdapterOptions = {},
): Promise<DesktopKernelDomainAdapter> {
  return createDesktopKernelDomainAdapter(bootstrap, {
    ...options,
    profile: "mobile",
  });
}

export function createMobileKernelRuntimeOwner(
  kernel: KernelDomainPort,
): MobileKernelRuntimeOwner {
  const transient = createDefaultAppRuntime();
  const fileOwner = createKernelFileRuntimeOwner(kernel, {
    imageSource: createKernelObjectUrlImageSource(),
    invalidations: kernel.invalidations,
    nativeShell: {
      confirmMarkdownFileDelete: mobileRuntime.files.confirmMarkdownFileDelete,
      confirmUnsavedMarkdownDocumentDiscard:
        mobileRuntime.files.confirmUnsavedMarkdownDocumentDiscard,
      openLocalImages: mobileRuntime.files.openLocalImages,
    },
  });
  const runtime: AppRuntime = {
    ...mobileRuntime,
    features: {
      ...mobileRuntime.features,
      projectSync: true,
      resources: true,
    },
    files: fileOwner.files,
    kernel,
    settings: createKernelSettingsRuntime(kernel, {
      local: {
        loadStore: mobileRuntime.settings.loadStore,
        readPrimaryWorkspaceState: undefined,
        writePrimaryWorkspaceState: undefined,
      },
    }),
    syncConfig: createKernelSyncConfigRuntime(kernel, {
      local: {
        cancelApply: transient.syncConfig.cancelApply,
        loadEditing: transient.syncConfig.loadEditing,
        requestApply: transient.syncConfig.requestApply,
        setEditing: transient.syncConfig.setEditing,
        settleApply: transient.syncConfig.settleApply,
      },
    }),
    syncPathGuard: transient.syncPathGuard,
    workspace: {
      isDocumentInRoot: async (documentPath, rootPath) => (
        rootPath === kernelWorkspaceRoot &&
        (documentPath === rootPath || documentPath.startsWith(`${rootPath}/`))
      ),
      listManagedNotebookNames: async () => [],
      resolveManagedRoot: async () => null,
      rootPolicy: {
        canChooseLocalRoot: false,
        kind: "fixed",
        resolveRoot: async () => kernelWorkspaceRoot,
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
