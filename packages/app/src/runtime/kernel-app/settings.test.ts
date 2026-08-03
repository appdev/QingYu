import {
  createUnavailableKernelDomainPort,
  type AppSettingsGroup,
  type KernelDomainPort,
  type KernelInvalidationNotice,
  type KernelRevision,
  type KernelSettingsSnapshot,
} from "../index";
import { createKernelSettingsRuntime } from "./settings";

describe("Kernel settings runtime", () => {
  it("uses the AppConfig bootstrap for every startup group read", async () => {
    const harness = settingsHarness();
    const runtime = createKernelSettingsRuntime(
      harness.kernel,
      settingsSnapshot("settings-1"),
      { local: {} },
    );

    const groups: AppSettingsGroup[] = [
      "appearance",
      "customThemeCss",
      "language",
      "editorPreferences",
      "fileIgnoreSettings",
      "exportSettings",
    ];
    const values = await Promise.all(groups.map((group) => runtime.readGroup?.(group)));

    expect(values).toEqual([
      { appearanceMode: "dark", darkTheme: "night", lightTheme: "minimal" },
      { dark: "dark css", light: "light css" },
      "zh-CN",
      { autoSaveEnabled: false, bodyFontSize: 17, showWordCount: true },
      { rules: "node_modules" },
      { pdfAuthor: "Markra" },
    ]);
    expect(harness.read).not.toHaveBeenCalled();
  });

  it("replaces its settings cache after writes and settings invalidations", async () => {
    const harness = settingsHarness();
    const runtime = createKernelSettingsRuntime(
      harness.kernel,
      settingsSnapshot("settings-1"),
      { local: {} },
    );
    harness.patch.mockResolvedValueOnce(settingsSnapshot("settings-2", "light"));

    await runtime.writeGroup?.("appearance", {
      appearanceMode: "light",
      darkTheme: "night",
      lightTheme: "minimal",
    });

    expect(harness.patch).toHaveBeenCalledWith({
      expectedRevision: "settings-1",
      values: [
        { key: "appearance.mode", value: { type: "string", value: "light" } },
        { key: "appearance.lightTheme", value: { type: "string", value: "minimal" } },
        { key: "appearance.darkTheme", value: { type: "string", value: "night" } },
      ],
    });
    await expect(runtime.readGroup?.("appearance")).resolves.toMatchObject({
      appearanceMode: "light",
    });
    expect(harness.read).not.toHaveBeenCalled();

    harness.read.mockResolvedValueOnce(settingsSnapshot("settings-3", "system"));
    harness.publish({ scopes: ["settings"] });
    await vi.waitFor(async () => {
      await expect(runtime.readGroup?.("appearance")).resolves.toMatchObject({
        appearanceMode: "system",
      });
    });
    expect(harness.read).toHaveBeenCalledOnce();
  });

  it("persists the real-time auto-save opt-out through typed Kernel settings", async () => {
    const harness = settingsHarness();
    const runtime = createKernelSettingsRuntime(
      harness.kernel,
      settingsSnapshot("settings-1"),
      { local: {} },
    );

    await runtime.writeGroup?.("editorPreferences", { autoSaveEnabled: true });

    expect(harness.patch).toHaveBeenCalledWith({
      expectedRevision: "settings-1",
      values: [{
        key: "editor.autoSaveEnabled",
        value: { type: "boolean", value: true },
      }],
    });
  });

  it("does not expose renderer local-state storage on a Kernel-backed settings runtime", async () => {
    const harness = settingsHarness();
    const runtime = createKernelSettingsRuntime(
      harness.kernel,
      settingsSnapshot("settings-1"),
      { local: {} },
    );

    await expect(runtime.loadStore("local-state.json", {
      autoSave: false,
      defaults: {},
    })).rejects.toThrow("unavailable for a Kernel-backed runtime");
  });
});

function settingsHarness() {
  const unavailable = createUnavailableKernelDomainPort();
  const listeners = new Set<(notice: KernelInvalidationNotice) => unknown>();
  const read = vi.fn(async () => settingsSnapshot("settings-1"));
  const patch = vi.fn(async () => settingsSnapshot("settings-2"));
  const kernel = {
    ...unavailable,
    availability: "available" as const,
    invalidations: {
      available: true,
      subscribe: (listener: (notice: KernelInvalidationNotice) => unknown) => {
        listeners.add(listener);
        return () => listeners.delete(listener);
      },
    },
    settings: { patch, read },
  } satisfies KernelDomainPort;
  return {
    kernel,
    patch,
    publish: (notice: KernelInvalidationNotice) => {
      for (const listener of listeners) listener(notice);
    },
    read,
  };
}

function settingsSnapshot(
  revision: string,
  appearanceMode = "dark",
): KernelSettingsSnapshot {
  return {
    revision: revision as KernelRevision,
    values: [
      { key: "appearance.mode", value: { type: "string", value: appearanceMode } },
      { key: "appearance.lightTheme", value: { type: "string", value: "minimal" } },
      { key: "appearance.darkTheme", value: { type: "string", value: "night" } },
      { key: "theme.customCss.light", value: { type: "string", value: "light css" } },
      { key: "theme.customCss.dark", value: { type: "string", value: "dark css" } },
      { key: "language", value: { type: "string", value: "zh-CN" } },
      { key: "editor.autoSaveEnabled", value: { type: "boolean", value: false } },
      { key: "editor.bodyFontSize", value: { type: "integer", value: 17 } },
      { key: "editor.showWordCount", value: { type: "boolean", value: true } },
      { key: "files.ignoreRules", value: { type: "string", value: "node_modules" } },
      { key: "export.pdfAuthor", value: { type: "string", value: "Markra" } },
    ],
  };
}
