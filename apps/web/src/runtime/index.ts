import {
  createDefaultAppRuntime,
  type AppRuntime,
  type KernelDomainPort
} from "@markra/app/runtime";
import {
  createBrowserEventsRuntime,
  createIndexedDbSettingsRuntime,
  createWebDialogRuntime,
  createWebFileRuntime,
  createWebMenuRuntime,
  createWebResourceRuntime,
  createWebWindowRuntime,
  type WebRuntimeOptions
} from "./web";
import { createServerFileRuntime, serverWorkspaceRoot } from "./server/files";
import { createServerSyncConfigRuntime } from "./server/sync-config";
import { createServerSettingsRuntime } from "./server/settings";

export * from "./web";

export function createWebRuntime(options: WebRuntimeOptions = {}): AppRuntime {
  const defaultRuntime = createDefaultAppRuntime();
  const settings = createIndexedDbSettingsRuntime(options);

  return {
    ...defaultRuntime,
    dialog: createWebDialogRuntime(options),
    events: createBrowserEventsRuntime(options.eventTarget),
    features: {
      applicationMenu: false,
      applicationShortcuts: true,
      export: true,
      fileDrop: true,
      imageImport: false,
      nativeWindowChrome: false,
      openLocalAttachments: true,
      pandoc: false,
      projectSync: false,
      resources: false,
      settingsWindow: false,
      systemFonts: false,
      updater: false
    },
    files: createWebFileRuntime(settings, options),
    menu: createWebMenuRuntime(defaultRuntime.menu, options),
    platform: {
      resolveDesktopOsVersion: () => null,
      resolveDesktopPlatform: () => "windows",
      resolveFormFactor: () => "desktop"
    },
    settings,
    webResource: createWebResourceRuntime(options),
    window: createWebWindowRuntime(defaultRuntime.window, options),
    workspace: {
      resolveManagedRoot: async () => null
    }
  };
}

export function createServerWebRuntime(
  kernel: KernelDomainPort,
  options: WebRuntimeOptions = {}
): AppRuntime {
  const defaultRuntime = createDefaultAppRuntime();
  return {
    ...defaultRuntime,
    dialog: createWebDialogRuntime(options),
    events: createBrowserEventsRuntime(options.eventTarget),
    features: {
      applicationMenu: false,
      applicationShortcuts: true,
      export: true,
      fileDrop: false,
      imageImport: false,
      nativeWindowChrome: false,
      openLocalAttachments: false,
      pandoc: false,
      projectSync: true,
      resources: false,
      settingsWindow: false,
      systemFonts: false,
      updater: false
    },
    files: createServerFileRuntime(kernel, options),
    kernel,
    menu: createWebMenuRuntime(defaultRuntime.menu, options),
    platform: {
      resolveDesktopOsVersion: () => null,
      resolveDesktopPlatform: () => null,
      resolveFormFactor: () => "desktop"
    },
    settings: createServerSettingsRuntime(kernel),
    syncConfig: createServerSyncConfigRuntime(kernel),
    webResource: createWebResourceRuntime(options),
    window: createWebWindowRuntime(defaultRuntime.window, options),
    workspace: {
      isDocumentInRoot: async (documentPath, rootPath) =>
        rootPath === serverWorkspaceRoot && (
          documentPath === serverWorkspaceRoot || documentPath.startsWith(`${serverWorkspaceRoot}/`)
        ),
      listManagedNotebookNames: async () => [],
      resolveManagedRoot: async () => null,
      rootPolicy: {
        canChooseLocalRoot: false,
        kind: "fixed",
        resolveRoot: async () => serverWorkspaceRoot
      }
    }
  };
}
