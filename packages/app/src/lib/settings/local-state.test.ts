import {
  configureAppRuntime,
  createDefaultAppRuntime,
  resetAppRuntimeForTests,
  type RuntimeStore
} from "../../runtime";
import {
  defaultPrimaryWorkspaceState,
  getStoredRecentNotebooks,
  loadLocalPandocPath,
  loadPrimaryWorkspaceState,
  normalizePrimaryWorkspaceState,
  removeStoredRecentNotebook,
  saveCanonicalPrimaryWorkspaceState,
  saveLocalPandocPath,
  saveStoredRecentNotebook,
  savePrimaryWorkspaceState,
  updatePrimaryWorkspaceState
} from "./local-state";

const stores = new Map<string, Map<string, unknown>>();

const mockedLoadStore = vi.fn(async (path: string): Promise<RuntimeStore> => {
  const values = stores.get(path) ?? new Map<string, unknown>();
  stores.set(path, values);

  return {
    delete: async (key) => values.delete(key),
    get: async <T>(key: string) => values.get(key) as T | undefined,
    save: async () => undefined,
    set: async (key, value) => {
      values.set(key, value);
    }
  };
});

function seedStore(path: string, key: string, value: unknown) {
  const values = stores.get(path) ?? new Map<string, unknown>();
  values.set(key, value);
  stores.set(path, values);
}

function storeValue(path: string, key: string) {
  return stores.get(path)?.get(key);
}

describe("device-local application state", () => {
  beforeEach(() => {
    stores.clear();
    mockedLoadStore.mockClear();
    configureAppRuntime({
      ...createDefaultAppRuntime(),
      settings: { loadStore: mockedLoadStore }
    });
  });

  afterEach(() => {
    resetAppRuntimeForTests();
  });

  it("stores primary workspace state only in local-state.json", async () => {
    await savePrimaryWorkspaceState({
      desktopWorkspaceRoot: "/Users/test",
      desktopPath: "/Users/test/Notes",
      managedName: null,
      onboardingCompleted: true,
      version: 3
    });

    expect(mockedLoadStore).toHaveBeenCalledWith("local-state.json", {
      autoSave: false,
      defaults: {}
    });
    expect(storeValue("local-state.json", "primaryWorkspace")).toEqual({
      desktopWorkspaceRoot: "/Users/test",
      desktopPath: "/Users/test/Notes",
      managedName: null,
      onboardingCompleted: true,
      version: 3
    });
    expect(storeValue("local-state.json", "schemaVersion")).toBe(2);
    expect(storeValue("settings.json", "primaryWorkspace")).toBeUndefined();
  });

  it("normalizes malformed state without reading legacy settings keys", async () => {
    seedStore("settings.json", "workspace", { folderPath: "/legacy" });
    seedStore("local-state.json", "primaryWorkspace", { version: 9, desktopPath: 7 });

    await expect(loadPrimaryWorkspaceState()).resolves.toEqual(defaultPrimaryWorkspaceState);
    expect(mockedLoadStore).toHaveBeenCalledTimes(1);
    expect(mockedLoadStore).not.toHaveBeenCalledWith("settings.json", expect.anything());
  });

  it("rejects version 1 without migrating it", () => {
    expect(normalizePrimaryWorkspaceState({
      desktopPath: "/Users/test/Notes",
      onboardingCompleted: true,
      version: 1
    })).toEqual(defaultPrimaryWorkspaceState);
  });

  it("accepts only complete version 3 workspace identities", () => {
    expect(normalizePrimaryWorkspaceState({
      desktopWorkspaceRoot: "/Users/test/Workspace",
      desktopPath: "/Users/test/Workspace/Notes",
      managedName: null,
      onboardingCompleted: true,
      version: 3
    })).toEqual({
      desktopWorkspaceRoot: "/Users/test/Workspace",
      desktopPath: "/Users/test/Workspace/Notes",
      managedName: null,
      onboardingCompleted: true,
      version: 3
    });
    expect(normalizePrimaryWorkspaceState({
      desktopWorkspaceRoot: null,
      desktopPath: null,
      managedName: "personal",
      onboardingCompleted: true,
      version: 3
    })).toEqual({
      desktopWorkspaceRoot: null,
      desktopPath: null,
      managedName: "personal",
      onboardingCompleted: true,
      version: 3
    });
    expect(normalizePrimaryWorkspaceState({
      desktopWorkspaceRoot: null,
      desktopPath: null,
      managedName: null,
      onboardingCompleted: false,
      version: 3
    })).toEqual(defaultPrimaryWorkspaceState);
  });

  it("fails closed when a present version 3 field has the wrong type", () => {
    const validState = {
      desktopWorkspaceRoot: "/Users/test/Workspace",
      desktopPath: "/Users/test/Workspace/Notes",
      managedName: null,
      onboardingCompleted: true,
      version: 3
    };
    const malformedStates = [
      { ...validState, desktopWorkspaceRoot: 7 },
      { ...validState, desktopPath: 7 },
      { ...validState, managedName: 7 },
      { ...validState, onboardingCompleted: "true" },
      { ...validState, onboardingRequestedForNextLaunch: "true" }
    ];

    malformedStates.forEach((state) => {
      expect(normalizePrimaryWorkspaceState(state)).toEqual(defaultPrimaryWorkspaceState);
    });
  });

  it("retains a complete raw desktop identity for canonical alias validation", () => {
    expect(normalizePrimaryWorkspaceState({
      desktopWorkspaceRoot: "/alias/Workspace",
      desktopPath: "/canonical/Workspace/Notes",
      managedName: null,
      onboardingCompleted: true,
      version: 3
    })).toEqual({
      desktopWorkspaceRoot: "/alias/Workspace",
      desktopPath: "/canonical/Workspace/Notes",
      managedName: null,
      onboardingCompleted: true,
      version: 3
    });
  });

  it("fails closed for version 2 and incomplete or conflicting version 3 identities", () => {
    const invalidStates = [
      {
        desktopWorkspaceRoot: "/Users/test/Workspace",
        desktopPath: "/Users/test/Workspace/Notes",
        managedName: null,
        onboardingCompleted: true,
        version: 2
      },
      {
        desktopWorkspaceRoot: "/Users/test/Workspace",
        desktopPath: null,
        managedName: null,
        onboardingCompleted: true,
        version: 3
      },
      {
        desktopWorkspaceRoot: null,
        desktopPath: "/Users/test/Workspace/Notes",
        managedName: null,
        onboardingCompleted: true,
        version: 3
      },
      {
        desktopWorkspaceRoot: "/Users/test/Workspace",
        desktopPath: "/Users/test/Workspace/Notes",
        managedName: "personal",
        onboardingCompleted: true,
        version: 3
      }
    ];

    invalidStates.forEach((state) => {
      expect(normalizePrimaryWorkspaceState(state)).toEqual(defaultPrimaryWorkspaceState);
    });
  });

  it("rejects a mixed desktop and managed identity", () => {
    expect(normalizePrimaryWorkspaceState({
      desktopWorkspaceRoot: "/Users/test",
      desktopPath: "/Users/test/Notes",
      managedName: "personal",
      onboardingCompleted: true,
      version: 3
    })).toEqual(defaultPrimaryWorkspaceState);
  });

  it("retains a Unicode managed name and its ordinary surrounding spaces exactly", () => {
    expect(normalizePrimaryWorkspaceState({
      desktopWorkspaceRoot: null,
      desktopPath: null,
      managedName: "  个人 笔记  ",
      onboardingCompleted: true,
      version: 3
    })).toEqual({
      desktopWorkspaceRoot: null,
      desktopPath: null,
      managedName: "  个人 笔记  ",
      onboardingCompleted: true,
      version: 3
    });
  });

  it("round-trips an exact recent notebook name and path", async () => {
    const notebook = {
      name: " 个人 笔记 ",
      path: " /Users/test/个人 笔记 "
    };

    await expect(saveStoredRecentNotebook(notebook)).resolves.toEqual([notebook]);
    await expect(getStoredRecentNotebooks()).resolves.toEqual([notebook]);
    expect(storeValue("local-state.json", "recentNotebooks")).toEqual([notebook]);
  });

  it("removes an exact recent notebook path with trailing whitespace", async () => {
    const notebook = { name: "Notes ", path: "/Users/test/Notes " };
    seedStore("local-state.json", "recentNotebooks", [notebook]);

    await expect(removeStoredRecentNotebook(notebook.path)).resolves.toEqual([]);
  });

  it("serializes a remove followed by a save without resurrecting the removed notebook", async () => {
    const oldNotebook = { name: "Old", path: "/Old" };
    const newNotebook = { name: "New", path: "/New" };
    seedStore("local-state.json", "recentNotebooks", [oldNotebook]);

    await Promise.all([
      removeStoredRecentNotebook(oldNotebook.path),
      saveStoredRecentNotebook(newNotebook)
    ]);

    expect(storeValue("local-state.json", "recentNotebooks")).toEqual([newNotebook]);
  });

  it("serializes a save followed by a remove without losing the saved notebook", async () => {
    const oldNotebook = { name: "Old", path: "/Old" };
    const newNotebook = { name: "New", path: "/New" };
    seedStore("local-state.json", "recentNotebooks", [oldNotebook]);

    await Promise.all([
      saveStoredRecentNotebook(newNotebook),
      removeStoredRecentNotebook(oldNotebook.path)
    ]);

    expect(storeValue("local-state.json", "recentNotebooks")).toEqual([newNotebook]);
  });

  it("merges primary workspace changes into normalized local state", async () => {
    seedStore("local-state.json", "primaryWorkspace", {
      desktopWorkspaceRoot: "/Users/test",
      desktopPath: "/Users/test/Notes",
      managedName: null,
      onboardingCompleted: false,
      version: 3
    });

    await expect(updatePrimaryWorkspaceState({ onboardingCompleted: true })).resolves.toEqual({
      desktopWorkspaceRoot: "/Users/test",
      desktopPath: "/Users/test/Notes",
      managedName: null,
      onboardingCompleted: true,
      version: 3
    });
    expect(storeValue("local-state.json", "primaryWorkspace")).toEqual({
      desktopWorkspaceRoot: "/Users/test",
      desktopPath: "/Users/test/Notes",
      managedName: null,
      onboardingCompleted: true,
      version: 3
    });
  });

  it("reloads an out-of-band primary workspace change after coordinated writes settle", async () => {
    await savePrimaryWorkspaceState({
      desktopWorkspaceRoot: "/",
      desktopPath: "/a",
      managedName: null,
      onboardingCompleted: true,
      version: 3
    });
    seedStore("local-state.json", "primaryWorkspace", {
      desktopWorkspaceRoot: "/",
      desktopPath: "/b",
      managedName: null,
      onboardingCompleted: true,
      version: 3
    });

    await expect(loadPrimaryWorkspaceState()).resolves.toEqual({
      desktopWorkspaceRoot: "/",
      desktopPath: "/b",
      managedName: null,
      onboardingCompleted: true,
      version: 3
    });
  });

  it("returns the authoritative flags when a canonical compare-and-set is stale", async () => {
    const expectedState = {
      desktopWorkspaceRoot: "/alias",
      desktopPath: "/alias/Notes-A",
      managedName: null,
      onboardingCompleted: true,
      version: 3 as const
    };
    const currentState = {
      ...expectedState,
      onboardingRequestedForNextLaunch: true as const
    };
    seedStore("local-state.json", "primaryWorkspace", currentState);

    await expect(saveCanonicalPrimaryWorkspaceState({
      ...expectedState,
      desktopWorkspaceRoot: "/canonical",
      desktopPath: "/canonical/Notes-A"
    }, expectedState)).resolves.toEqual(currentState);
    expect(storeValue("local-state.json", "primaryWorkspace")).toEqual(currentState);
  });

  it("stores normalized Pandoc paths only in local-state.json", async () => {
    await expect(saveLocalPandocPath(" /opt/homebrew/bin/pandoc ")).resolves.toBe(
      "/opt/homebrew/bin/pandoc"
    );
    await expect(loadLocalPandocPath()).resolves.toBe("/opt/homebrew/bin/pandoc");

    expect(storeValue("local-state.json", "pandocPath")).toBe("/opt/homebrew/bin/pandoc");
    expect(storeValue("local-state.json", "schemaVersion")).toBe(2);
    expect(storeValue("settings.json", "pandocPath")).toBeUndefined();
  });
});
