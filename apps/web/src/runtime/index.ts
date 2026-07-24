import {
  createDefaultAppRuntime,
  type AppRuntime
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
