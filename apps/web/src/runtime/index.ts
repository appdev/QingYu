import {
  createDefaultAppRuntime,
  createKernelAppConfigRuntime,
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
import { createServerFileRuntimeOwner, serverWorkspaceRoot } from "./server/files";
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
      localFileImport: false,
      nativeWindowChrome: false,
      openLocalAttachments: true,
      pandoc: false,
      projectSync: false,
      resources: false,
      settingsWindow: false,
      standaloneDocuments: true,
      systemFonts: false,
      templateMutation: true,
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

export interface ServerWebRuntimeOwner {
  readonly runtime: AppRuntime;
  readonly release: () => undefined;
}

export function createServerWebRuntime(
  kernel: KernelDomainPort,
  options: WebRuntimeOptions = {}
): ServerWebRuntimeOwner {
  const defaultRuntime = createDefaultAppRuntime();
  const fileOwner = createServerFileRuntimeOwner(kernel, options);
  const runtime: AppRuntime = {
    ...defaultRuntime,
    dialog: createWebDialogRuntime(options),
    events: createBrowserEventsRuntime(options.eventTarget),
    features: {
      applicationMenu: false,
      applicationShortcuts: true,
      export: true,
      fileDrop: false,
      imageImport: false,
      localFileImport: false,
      nativeWindowChrome: false,
      openLocalAttachments: false,
      pandoc: false,
      projectSync: true,
      resources: false,
      settingsWindow: false,
      standaloneDocuments: false,
      systemFonts: false,
      templateMutation: false,
      updater: false
    },
    files: fileOwner.files,
    kernel,
    appConfig: createKernelAppConfigRuntime(kernel, async () => "main"),
    menu: createWebMenuRuntime(defaultRuntime.menu, options),
    platform: {
      resolveDesktopOsVersion: () => null,
      resolveDesktopPlatform: () => null,
      resolveFormFactor: () => "desktop"
    },
    settings: createServerSettingsRuntime(kernel, kernel.appConfig.bootstrap.settings),
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
  return Object.freeze({
    runtime,
    release: fileOwner.release,
  });
}
