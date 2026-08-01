import type {
  KernelDomainPort,
  KernelRevision,
  KernelSettingsSnapshot,
} from "@markra/app/runtime";

import { createServerSettingsRuntime } from "./settings";

const revision = "settings-revision-1" as KernelRevision;

describe("server settings owner", () => {
  it("reads grouped portable settings and writes with the Kernel revision", async () => {
    const kernel = kernelPort();
    const settings = createServerSettingsRuntime(kernel);

    await expect(settings.readGroup?.("appearance")).resolves.toEqual({
      appearanceMode: "dark",
      darkTheme: "night",
      lightTheme: "minimal",
    });
    await settings.writeGroup?.("appearance", {
      appearanceMode: "light",
      darkTheme: "night",
      lightTheme: "minimal",
    });

    expect(kernel.settings.patch).toHaveBeenCalledWith({
      expectedRevision: revision,
      values: [
        { key: "appearance.mode", value: { type: "string", value: "light" } },
        { key: "appearance.lightTheme", value: { type: "string", value: "minimal" } },
        { key: "appearance.darkTheme", value: { type: "string", value: "night" } },
      ],
    });
    expect(kernel.settings.read).not.toHaveBeenCalled();
  });

  it("does not expose a browser-local store for Kernel-backed state", async () => {
    const kernel = kernelPort();
    const settings = createServerSettingsRuntime(kernel);
    await expect(settings.loadStore("local-state.json", {
      autoSave: false,
      defaults: { layout: "stacked" },
    })).rejects.toThrow("unavailable for a Kernel-backed runtime");
    expect(kernel.settings.read).not.toHaveBeenCalled();
  });
});

function snapshot(): KernelSettingsSnapshot {
  return {
    revision,
    values: [
      { key: "appearance.mode", value: { type: "string", value: "dark" } },
      { key: "appearance.lightTheme", value: { type: "string", value: "minimal" } },
      { key: "appearance.darkTheme", value: { type: "string", value: "night" } },
      { key: "language", value: { type: "string", value: "zh-CN" } },
    ],
  };
}

function kernelPort() {
  const unavailable = () => vi.fn(async () => {
    throw new Error("not used");
  });
  return {
    appConfig: {
      bootstrap: {
        appConfigVersion: 1,
        localState: {
          fileTreeSort: { direction: "ascending", key: "name" },
          pandocPath: null,
          recentMarkdownFiles: [],
          revision: "local-1",
          uiLayout: { openWindows: [], schemaVersion: 1, windowStates: {} },
        },
        settings: snapshot(),
        workspace: { generation: "generation-1", id: "workspace-1" },
      },
      patchState: unavailable(),
      read: unavailable(),
    },
    availability: "available",
    documents: {
      create: unavailable(), delete: unavailable(),
      history: { list: unavailable(), restore: unavailable() },
      list: unavailable(), move: unavailable(), read: unavailable(),
      search: unavailable(), update: unavailable(),
    },
    invalidations: {
      available: false,
      subscribe: () => () => undefined,
    },
    runtime: { read: unavailable() },
    settings: {
      patch: vi.fn(async () => snapshot()),
      read: vi.fn(async () => snapshot()),
    },
    sync: {
      patchConfig: unavailable(), readConfig: unavailable(), readStatus: unavailable(),
      testConnection: unavailable(), trigger: unavailable(),
    },
    workspace: { read: unavailable() },
  } as unknown as KernelDomainPort;
}
