import { invoke } from "@tauri-apps/api/core";
import { loadDesktopRuntimeStore } from "./runtime-store";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn()
}));

vi.mock("@markra/app/runtime", () => ({
  appLogger: {
    error: vi.fn(),
    info: vi.fn(),
    warn: vi.fn()
  }
}));

const mockedInvoke = vi.mocked(invoke);

describe("desktop runtime store", () => {
  beforeEach(() => {
    mockedInvoke.mockReset();
  });

  it("buffers allowlisted local UI state and commits it in one native transaction", async () => {
    mockedInvoke.mockResolvedValueOnce(undefined);
    const store = await loadDesktopRuntimeStore("local-state.json", {
      autoSave: false,
      defaults: {}
    });

    mockedInvoke.mockResolvedValueOnce({ exists: true, value: { openWindows: [] } });
    await expect(store.get("workspace")).resolves.toEqual({ openWindows: [] });
    await expect(store.set("workspace", { openWindows: ["main"] })).resolves.toBeUndefined();
    await expect(store.get("workspace")).resolves.toEqual({ openWindows: ["main"] });
    await expect(store.delete("recentMarkdownFiles")).resolves.toBeUndefined();
    await expect(store.get("recentMarkdownFiles")).resolves.toBeUndefined();

    mockedInvoke.mockResolvedValueOnce(undefined);
    await expect(store.save()).resolves.toBeUndefined();

    expect(mockedInvoke.mock.calls).toEqual([
      ["load_desktop_runtime_store", {
        options: { autoSave: false, defaults: {} },
        path: "local-state.json"
      }],
      ["get_desktop_runtime_store_value", { key: "workspace", path: "local-state.json" }],
      ["commit_desktop_runtime_store_changes", {
        changes: [
          { key: "workspace", operation: "set", value: { openWindows: ["main"] } },
          { key: "recentMarkdownFiles", operation: "delete" }
        ],
        path: "local-state.json"
      }]
    ]);
  });

  it("preserves pending changes after a failed native commit so save can retry", async () => {
    mockedInvoke.mockResolvedValueOnce(undefined);
    const store = await loadDesktopRuntimeStore("local-state.json", {
      autoSave: false,
      defaults: {}
    });
    await store.set("welcomeDocumentSeen", true);

    mockedInvoke.mockRejectedValueOnce(new Error("publish uncertain"));
    await expect(store.save()).rejects.toThrow("publish uncertain");
    mockedInvoke.mockResolvedValueOnce(undefined);
    await expect(store.save()).resolves.toBeUndefined();

    const expectedCommit = ["commit_desktop_runtime_store_changes", {
      changes: [{ key: "welcomeDocumentSeen", operation: "set", value: true }],
      path: "local-state.json"
    }];
    expect(mockedInvoke.mock.calls.slice(1)).toEqual([expectedCommit, expectedCommit]);
  });

  it("preserves a newer same-key mutation while an older save is in flight", async () => {
    mockedInvoke.mockResolvedValueOnce(undefined);
    const store = await loadDesktopRuntimeStore("local-state.json", {
      autoSave: false,
      defaults: {}
    });
    await store.set("workspace", { openWindows: ["old"] });

    let finishCommit: (() => unknown) | undefined;
    mockedInvoke.mockImplementationOnce(() => new Promise((resolve) => {
      finishCommit = () => resolve(undefined);
    }));
    const firstSave = store.save();
    await vi.waitFor(() => {
      expect(finishCommit).toBeTypeOf("function");
    });
    await store.set("workspace", { openWindows: ["new"] });
    finishCommit?.();
    await firstSave;
    await expect(store.get("workspace")).resolves.toEqual({ openWindows: ["new"] });

    mockedInvoke.mockResolvedValueOnce(undefined);
    await store.save();
    expect(mockedInvoke).toHaveBeenLastCalledWith("commit_desktop_runtime_store_changes", {
      changes: [{
        key: "workspace",
        operation: "set",
        value: { openWindows: ["new"] }
      }],
      path: "local-state.json"
    });
  });

  it("maps a missing native value to undefined while preserving stored null", async () => {
    mockedInvoke.mockResolvedValueOnce(undefined);
    const store = await loadDesktopRuntimeStore("settings.json", {
      autoSave: false,
      defaults: {}
    });
    mockedInvoke.mockResolvedValueOnce({ exists: false, value: null });
    await expect(store.get("theme")).resolves.toBeUndefined();
    mockedInvoke.mockResolvedValueOnce({ exists: true, value: null });
    await expect(store.get("theme")).resolves.toBeNull();
  });

  it.each([
    "../local-state.json",
    "/tmp/local-state.json",
    "native-host-workspaces.bin",
    "other.json"
  ])("rejects unsupported store path %s before native invocation", async (path) => {
    await expect(loadDesktopRuntimeStore(path, { autoSave: false, defaults: {} }))
      .rejects.toThrow("Unsupported desktop runtime store");
    expect(mockedInvoke).not.toHaveBeenCalled();
  });

  it("rejects broader plugin-store options before native invocation", async () => {
    await expect(loadDesktopRuntimeStore("settings.json", {
      autoSave: true,
      defaults: { injected: true }
    })).rejects.toThrow("Unsupported desktop runtime store options");
    expect(mockedInvoke).not.toHaveBeenCalled();
  });

  it.each([
    ["local-state.json", "primaryWorkspace"],
    ["local-state.json", "nativeHostWorkspaceStates"],
    ["local-state.json", "mcp"],
    ["settings.json", "mcp"],
    ["settings.json", "unapproved"]
  ])("rejects protected read key %s/%s before native invocation", async (path, key) => {
    mockedInvoke.mockResolvedValueOnce(undefined);
    const store = await loadDesktopRuntimeStore(path, { autoSave: false, defaults: {} });
    mockedInvoke.mockClear();

    await expect(store.get(key)).rejects.toThrow("Unsupported desktop runtime store key");
    expect(mockedInvoke).not.toHaveBeenCalled();
  });

  it("keeps settings.json read-only so writes use typed settings commands", async () => {
    mockedInvoke.mockResolvedValueOnce(undefined);
    const store = await loadDesktopRuntimeStore("settings.json", {
      autoSave: false,
      defaults: {}
    });

    await expect(store.set("theme", "dark")).rejects.toThrow(
      "Unsupported desktop runtime store mutation"
    );
    await expect(store.delete("theme")).rejects.toThrow(
      "Unsupported desktop runtime store mutation"
    );
    expect(mockedInvoke).toHaveBeenCalledTimes(1);
  });
});
