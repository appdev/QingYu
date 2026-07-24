import { FakeIndexedDbFactory } from "../test/web-runtime-fakes";
import { createWebRuntime } from "./index";

describe("web runtime", () => {
  it("creates a browser runtime with IndexedDB settings", async () => {
    const runtime = createWebRuntime({
      eventTarget: new EventTarget(),
      indexedDB: new FakeIndexedDbFactory().indexedDB
    });

    expect(runtime).toHaveProperty("files");
    expect(runtime).toHaveProperty("settings");
    expect(runtime).toHaveProperty("webResource.downloadImage", expect.any(Function));
    expect(runtime).toHaveProperty("syncConfig");
    expect(runtime.themes.capabilities).toEqual({
      canDelete: false,
      canImport: false,
      canOpenDirectory: false
    });
    await expect(runtime.themes.list()).resolves.toEqual({ invalidFiles: [], themes: [] });

    const store = await runtime.settings.loadStore("settings.json", {
      autoSave: false,
      defaults: { theme: "system" }
    });

    await store.set("theme", "solarized-dark");
    await store.save();

    const reloadedStore = await runtime.settings.loadStore("settings.json", {
      autoSave: false,
      defaults: {}
    });

    await expect(reloadedStore.get("theme")).resolves.toBe("solarized-dark");
    expect(runtime.events.isAvailable()).toBe(true);
    expect(runtime.features).toEqual({
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
    });
    expect(Object.keys(runtime.features)).not.toContain(["s3", "ImageUpload"].join(""));
    expect(Object.keys(runtime.files)).not.toContain(["upload", "Pic", "GoImage"].join(""));
    expect(Object.keys(runtime.files)).not.toContain(["upload", "S3Image"].join(""));
    expect(Object.keys(runtime.files)).not.toContain(["upload", "WebDavImage"].join(""));
    expect(Object.keys(runtime.files)).not.toContain(["syncMarkdown", "Folder"].join(""));
    expect(runtime.platform.resolveDesktopPlatform()).toBe("windows");
    await expect(runtime.updater.checkAppUpdate()).resolves.toBeNull();
  });

  it("exposes application sync as an unsupported native-only feature", async () => {
    const runtime = createWebRuntime();

    expect(runtime.features.projectSync).toBe(false);
    await expect(runtime.syncConfig.enable({ expectedRevision: null }))
      .rejects.toThrow("enableSyncConfig is unavailable without a configured app runtime.");
    await expect(runtime.syncConfig.load())
      .rejects.toThrow("loadSyncConfig is unavailable without a configured app runtime.");
    await expect(runtime.syncConfig.loadStatus())
      .rejects.toThrow("loadSyncStatus is unavailable without a configured app runtime.");
    await expect(runtime.syncConfig.sync({
      notebookName: "notes",
      notesRoot: "/notes",
      revision: "rev-1",
      trigger: "manual"
    })).rejects.toThrow("syncApplication is unavailable without a configured app runtime.");
    await expect(runtime.syncConfig.testConnection({ revision: "rev-1" }))
      .rejects.toThrow("testSyncConnection is unavailable without a configured app runtime.");
    await expect(runtime.syncConfig.patch({
      expectedRevision: "rev-1",
      patch: { field: "enabled", value: true }
    })).rejects.toThrow("patchSyncConfig is unavailable without a configured app runtime.");
    await expect(runtime.syncConfig.reset({ confirmed: true, expectedRevision: null }))
      .rejects.toThrow("resetSyncConfig is unavailable without a configured app runtime.");
  });
});
