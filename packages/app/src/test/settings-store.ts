import {
  configureAppRuntime,
  createDefaultAppRuntime,
  resetAppRuntimeForTests,
  type AppConfigRuntime,
  type AppSettingsRuntime,
  type KernelAppConfigSnapshot,
  type KernelRevision,
  type KernelWorkspaceGeneration,
  type RuntimeStore
} from "../runtime";
import { defaultWorkspaceState } from "../lib/settings/workspace-state";
import { retireAppConfigStatePersistence } from "../lib/settings/app-config-state";

type MockRuntimeStoreLoader = ReturnType<typeof vi.fn> & AppSettingsRuntime["loadStore"];

export type MockRuntimeStore = {
  delete: ReturnType<typeof vi.fn>;
  get: ReturnType<typeof vi.fn>;
  save: ReturnType<typeof vi.fn>;
  set: ReturnType<typeof vi.fn>;
};

export type SettingsStoreHarness = {
  appConfig: AppConfigRuntime;
  loadStore: MockRuntimeStoreLoader;
  originalRandomUuid: Crypto["randomUUID"];
  store: MockRuntimeStore;
};

export function createAppConfigSnapshot(): KernelAppConfigSnapshot {
  return {
    appConfigVersion: 1,
    workspace: {
      generation: "settings-harness-generation" as KernelWorkspaceGeneration,
      id: "settings-harness-workspace"
    },
    settings: {
      revision: "settings-harness-settings" as KernelRevision,
      values: []
    },
    localState: {
      revision: "settings-harness-state" as KernelRevision,
      uiLayout: { schemaVersion: 1, openWindows: [], windowStates: {} },
      recentMarkdownFiles: [],
      fileTreeSort: { direction: "ascending", key: "name" },
      pandocPath: null
    }
  };
}

export function createMockRuntimeStore(): MockRuntimeStore {
  return {
    delete: vi.fn(),
    get: vi.fn(),
    save: vi.fn(),
    set: vi.fn()
  };
}

export function createSettingsStoreHarness(): SettingsStoreHarness {
  const snapshot = createAppConfigSnapshot();
  return {
    appConfig: {
      bootstrap: snapshot,
      getSnapshot: vi.fn(() => snapshot),
      patchState: vi.fn(async () => snapshot),
      readWorkspaceState: vi.fn(async () => defaultWorkspaceState),
      reload: vi.fn(async () => snapshot)
    },
    loadStore: vi.fn() as MockRuntimeStoreLoader,
    originalRandomUuid: globalThis.crypto.randomUUID,
    store: createMockRuntimeStore()
  };
}

export function setupSettingsStoreHarness(harness: SettingsStoreHarness) {
  globalThis.crypto.randomUUID = harness.originalRandomUuid;
  harness.loadStore.mockReset();
  harness.store.delete.mockReset();
  harness.store.get.mockReset();
  harness.store.save.mockReset();
  harness.store.set.mockReset();
  vi.mocked(harness.appConfig.getSnapshot).mockReset();
  vi.mocked(harness.appConfig.patchState).mockReset();
  vi.mocked(harness.appConfig.readWorkspaceState).mockReset();
  vi.mocked(harness.appConfig.reload).mockReset();
  const snapshot = createAppConfigSnapshot();
  vi.mocked(harness.appConfig.getSnapshot).mockReturnValue(snapshot);
  vi.mocked(harness.appConfig.patchState).mockResolvedValue(snapshot);
  vi.mocked(harness.appConfig.readWorkspaceState).mockResolvedValue(defaultWorkspaceState);
  vi.mocked(harness.appConfig.reload).mockResolvedValue(snapshot);
  harness.loadStore.mockImplementation(async (path) => {
    if (path === "local-state.json") throw new Error("unexpected local-state access");
    return harness.store as unknown as RuntimeStore;
  });
  configureAppRuntime({
    ...createDefaultAppRuntime(),
    appConfig: harness.appConfig,
    settings: {
      loadStore: harness.loadStore
    }
  });
}

export function resetSettingsStoreRuntime() {
  retireAppConfigStatePersistence();
  resetAppRuntimeForTests();
}
