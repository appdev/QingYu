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
  });

  it("keeps browser-local stores in memory instead of opening IndexedDB", async () => {
    const kernel = kernelPort();
    const settings = createServerSettingsRuntime(kernel);
    const store = await settings.loadStore("local-state.json", {
      autoSave: false,
      defaults: { layout: "stacked" },
    });

    await expect(store.get("layout")).resolves.toBe("stacked");
    await store.set("layout", "tabs");
    await store.save();
    await expect(store.get("layout")).resolves.toBe("tabs");
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
    availability: "available",
    documents: {
      create: unavailable(), delete: unavailable(),
      history: { list: unavailable(), restore: unavailable() },
      list: unavailable(), move: unavailable(), read: unavailable(),
      search: unavailable(), update: unavailable(),
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
