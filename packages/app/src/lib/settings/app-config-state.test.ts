import { showAppToast } from "../app-toast";
import {
  configureAppRuntime,
  createDefaultAppRuntime,
  resetAppRuntimeForTests,
  type AppConfigRuntime,
  type KernelAppConfigSnapshot,
  type KernelAppConfigStateOperation,
  type KernelRevision,
  type KernelWorkspaceGeneration
} from "../../runtime";
import {
  clearStoredRecentMarkdownFiles,
  flushAppConfigStatePersistence,
  getStoredFileTreeSortByWorkspace,
  getStoredRecentMarkdownFiles,
  getStoredWorkspaceState,
  loadLocalPandocPath,
  removeStoredRecentMarkdownFile,
  retireAppConfigStatePersistence,
  saveLocalPandocPath,
  saveStoredFileTreeSortForWorkspace,
  saveStoredRecentMarkdownFile,
  saveStoredWorkspaceState
} from "./app-config-state";
import { defaultWorkspaceState } from "./workspace-state";

vi.mock("../app-toast", () => ({ showAppToast: vi.fn() }));

function snapshot(overrides: Partial<KernelAppConfigSnapshot["localState"]> = {}): KernelAppConfigSnapshot {
  return {
    appConfigVersion: 1,
    workspace: {
      generation: "generation-1" as KernelWorkspaceGeneration,
      id: "workspace-1"
    },
    settings: {
      revision: "settings-1" as KernelRevision,
      values: []
    },
    localState: {
      revision: "state-1" as KernelRevision,
      uiLayout: {
        schemaVersion: 1,
        openWindows: [],
        windowStates: {}
      },
      recentMarkdownFiles: [],
      fileTreeSort: { direction: "ascending", key: "name" },
      pandocPath: null,
      ...overrides
    }
  };
}

function appConfigHarness(initial = snapshot()) {
  let current = initial;
  const patchState = vi.fn(async (_operations: readonly KernelAppConfigStateOperation[]) => current);
  const readWorkspaceState = vi.fn<AppConfigRuntime["readWorkspaceState"]>(
    async () => defaultWorkspaceState
  );
  const appConfig = {
    bootstrap: current,
    getSnapshot: vi.fn(() => current),
    patchState,
    readWorkspaceState,
    reload: vi.fn(async () => current)
  } satisfies AppConfigRuntime;
  const defaultRuntime = createDefaultAppRuntime();
  configureAppRuntime({
    ...defaultRuntime,
    appConfig,
    settings: {
      ...defaultRuntime.settings,
      loadStore: vi.fn(async () => {
        throw new Error("unexpected local-state access");
      })
    }
  });
  return {
    appConfig,
    patchState,
    replaceSnapshot(next: KernelAppConfigSnapshot) {
      current = next;
    }
  };
}

function deferred<T>() {
  let reject!: (error: unknown) => unknown;
  let resolve!: (value: T) => unknown;
  const promise = new Promise<T>((promiseResolve, promiseReject) => {
    reject = promiseReject;
    resolve = promiseResolve;
  });
  return { promise, reject, resolve };
}

describe("Kernel AppConfig application state", () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.mocked(showAppToast).mockReset();
    retireAppConfigStatePersistence();
    resetAppRuntimeForTests();
    vi.restoreAllMocks();
  });

  it("loads and patches workspace state through AppConfig only", async () => {
    const { appConfig, patchState } = appConfigHarness();
    appConfig.readWorkspaceState.mockResolvedValue({
      ...defaultWorkspaceState,
      filePath: "kernel-workspace://primary/notes/a.md",
      openFilePaths: ["kernel-workspace://primary/notes/a.md"]
    });

    await expect(getStoredWorkspaceState()).resolves.toMatchObject({
      filePath: "kernel-workspace://primary/notes/a.md"
    });
    await saveStoredWorkspaceState({ fileTreeOpen: true });

    expect(patchState).toHaveBeenCalledWith([{
      type: "patch-ui-layout",
      windowLabel: "main",
      patch: { fileTreeOpen: true, openWindows: [] }
    }]);
  });

  it("maps a draft creation directory to a Kernel workspace-relative path", async () => {
    const { patchState } = appConfigHarness();

    await saveStoredWorkspaceState({
      draftTabs: [{
        content: "draft",
        creationDirectory: "kernel-workspace://primary/abc",
        id: "draft-1",
        name: "Draft.md",
        path: null
      }]
    });
    await flushAppConfigStatePersistence();

    expect(patchState).toHaveBeenCalledWith([{
      patch: {
        draftTabs: [{
          content: "draft",
          creationDirectory: "abc",
          id: "draft-1",
          name: "Draft.md",
          path: null
        }],
        openWindows: []
      },
      type: "patch-ui-layout",
      windowLabel: "main"
    }]);
  });

  it("preserves workspace-level open windows and independent window labels", async () => {
    const { appConfig, patchState } = appConfigHarness();
    appConfig.readWorkspaceState.mockImplementation(async (label: string | null | undefined) => ({
      ...defaultWorkspaceState,
      fileTreeOpen: label === "secondary",
      openWindows: [{
        filePath: "kernel-workspace://primary/notes/a.md",
        label: "secondary",
        openFilePaths: ["kernel-workspace://primary/notes/a.md"]
      }]
    }));

    await expect(getStoredWorkspaceState({ windowLabel: "secondary" })).resolves.toMatchObject({
      fileTreeOpen: true,
      openWindows: [{ label: "secondary" }]
    });
    await saveStoredWorkspaceState({
      openWindows: [{
        filePath: "kernel-workspace://primary/notes/a.md",
        label: "secondary",
        openFilePaths: ["kernel-workspace://primary/notes/a.md"]
      }]
    }, { windowLabel: "secondary" });

    expect(patchState).toHaveBeenCalledWith([{
      patch: {
        openWindows: [{ filePath: "notes/a.md", label: "secondary", openFilePaths: ["notes/a.md"] }]
      },
      type: "patch-ui-layout",
      windowLabel: "secondary"
    }]);
  });

  it("clears stale workspace-level open windows in the same window-state patch", async () => {
    const { patchState } = appConfigHarness();

    await saveStoredWorkspaceState({
      filePath: "kernel-workspace://primary/notes/current.md",
      openFilePaths: ["kernel-workspace://primary/notes/current.md"]
    }, { windowLabel: "secondary" });

    expect(patchState).toHaveBeenCalledWith([{
      patch: {
        filePath: "notes/current.md",
        openFilePaths: ["notes/current.md"],
        openWindows: []
      },
      type: "patch-ui-layout",
      windowLabel: "secondary"
    }]);
  });

  it("keeps an explicitly supplied workspace-level open-window snapshot", async () => {
    const { patchState } = appConfigHarness();

    await saveStoredWorkspaceState({
      fileTreeOpen: true,
      openWindows: [{
        filePath: "kernel-workspace://primary/notes/other.md",
        label: "secondary",
        openFilePaths: ["kernel-workspace://primary/notes/other.md"]
      }]
    });

    expect(patchState).toHaveBeenCalledWith([{
      patch: {
        fileTreeOpen: true,
        openWindows: [{
          filePath: "notes/other.md",
          label: "secondary",
          openFilePaths: ["notes/other.md"]
        }]
      },
      type: "patch-ui-layout",
      windowLabel: "main"
    }]);
  });

  it("maps recent paths between the canonical pseudo-root and Kernel relative paths", async () => {
    const { patchState } = appConfigHarness(snapshot({
      recentMarkdownFiles: [{ name: "A.md", path: "notes/a.md" as never }]
    }));

    await expect(getStoredRecentMarkdownFiles()).resolves.toEqual([{
      name: "A.md",
      path: "kernel-workspace://primary/notes/a.md"
    }]);
    await saveStoredRecentMarkdownFile({
      name: "B.md",
      path: "kernel-workspace://primary/notes/b.md"
    });
    await removeStoredRecentMarkdownFile("kernel-workspace://primary/notes/a.md");
    await clearStoredRecentMarkdownFiles();

    expect(patchState.mock.calls.map(([operations]) => operations)).toEqual([
      [{ type: "remember-recent-file", file: { name: "B.md", path: "notes/b.md" } }],
      [{ type: "remove-recent-file", path: "notes/a.md" }],
      [{ type: "clear-recent-files" }]
    ]);
  });

  it("reads and writes the active file-tree sort and Pandoc path", async () => {
    const { patchState } = appConfigHarness(snapshot({
      fileTreeSort: { direction: "descending", key: "modifiedAt" },
      pandocPath: "/opt/homebrew/bin/pandoc"
    }));

    await expect(getStoredFileTreeSortByWorkspace()).resolves.toEqual({
      "kernel-workspace://primary": { direction: "descending", key: "modifiedAt" }
    });
    await expect(loadLocalPandocPath()).resolves.toBe("/opt/homebrew/bin/pandoc");
    await saveStoredFileTreeSortForWorkspace("kernel-workspace://primary", {
      direction: "ascending",
      key: "createdAt"
    });
    await saveLocalPandocPath(" /usr/local/bin/pandoc ");

    expect(patchState.mock.calls.map(([operations]) => operations)).toEqual([
      [{ type: "set-file-tree-sort", sort: { direction: "ascending", key: "createdAt" } }],
      [{ type: "set-pandoc-path", path: "/usr/local/bin/pandoc" }]
    ]);
  });

  it.each([
    "/Users/private/notes/a.md",
    "/data/workspace/notes/a.md",
    "kernel-workspace://primary/../outside.md",
    "kernel-workspace://primary/notes%2Fa.md"
  ])("rejects unsafe persisted path %s", async (path) => {
    const { patchState } = appConfigHarness();

    await expect(saveStoredRecentMarkdownFile({ name: "a.md", path })).rejects.toThrow();
    expect(patchState).not.toHaveBeenCalled();
  });

  it("keeps immediate operations ordered", async () => {
    const { patchState } = appConfigHarness();
    const first = deferred<KernelAppConfigSnapshot>();
    patchState.mockImplementationOnce(() => first.promise).mockResolvedValue(snapshot());

    const remember = saveStoredRecentMarkdownFile({
      name: "A.md",
      path: "kernel-workspace://primary/notes/a.md"
    });
    const remove = removeStoredRecentMarkdownFile("kernel-workspace://primary/notes/a.md");
    await Promise.resolve();

    expect(patchState).toHaveBeenCalledTimes(1);
    first.resolve(snapshot());
    await Promise.all([remember, remove]);
    expect(patchState).toHaveBeenCalledTimes(2);
  });

  it("coalesces draft patches for 400ms and merges non-overlapping layout fields", async () => {
    vi.useFakeTimers();
    const { patchState } = appConfigHarness();

    const first = saveStoredWorkspaceState({
      draftTabs: [{ content: "first", id: "draft-1", name: "Draft.md", path: null }],
      fileTreeOpen: true
    });
    const second = saveStoredWorkspaceState({
      activeDraftId: "draft-1",
      draftTabs: [{ content: "newest", id: "draft-1", name: "Draft.md", path: null }]
    });
    await vi.advanceTimersByTimeAsync(399);
    expect(patchState).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    await Promise.all([first, second]);

    expect(patchState).toHaveBeenCalledWith([{
      patch: {
        activeDraftId: "draft-1",
        draftTabs: [{ content: "newest", id: "draft-1", name: "Draft.md", path: null }],
        fileTreeOpen: true,
        openWindows: []
      },
      type: "patch-ui-layout",
      windowLabel: "main"
    }]);
  });

  it("flushes a pending draft without waiting for its timer", async () => {
    vi.useFakeTimers();
    const { patchState } = appConfigHarness();

    const pending = saveStoredWorkspaceState({
      draftTabs: [{ content: "draft", id: "draft-1", name: "Draft.md", path: null }]
    });
    await vi.advanceTimersByTimeAsync(0);
    await flushAppConfigStatePersistence();
    await pending;

    expect(patchState).toHaveBeenCalledOnce();
  });

  it("retires a stale-generation queue and drops later queued work", async () => {
    const { patchState } = appConfigHarness();
    const stale = Object.assign(new Error("stale generation"), {
      code: "workspace_generation_stale"
    });
    patchState.mockRejectedValueOnce(stale);

    const first = saveStoredRecentMarkdownFile({
      name: "A.md",
      path: "kernel-workspace://primary/notes/a.md"
    });
    const second = removeStoredRecentMarkdownFile("kernel-workspace://primary/notes/a.md");

    await expect(first).rejects.toBe(stale);
    await expect(second).resolves.toBeUndefined();
    expect(patchState).toHaveBeenCalledTimes(1);
  });

  it("retries the newest merged window patch when a newer draft arrives during failure", async () => {
    vi.useFakeTimers();
    const { patchState } = appConfigHarness();
    const firstAttempt = deferred<KernelAppConfigSnapshot>();
    patchState.mockImplementationOnce(() => firstAttempt.promise).mockResolvedValue(snapshot());

    const first = saveStoredWorkspaceState({
      draftTabs: [{ content: "old", id: "draft-1", name: "Draft.md", path: null }],
      fileTreeOpen: true
    });
    await vi.advanceTimersByTimeAsync(400);
    const newer = saveStoredWorkspaceState({
      activeDraftId: "draft-1",
      draftTabs: [{ content: "newest", id: "draft-1", name: "Draft.md", path: null }]
    });
    await vi.advanceTimersByTimeAsync(0);
    firstAttempt.reject(Object.assign(new Error("kernel unavailable"), {
      code: "kernel_not_ready"
    }));
    await first;

    expect(patchState).toHaveBeenCalledTimes(2);
    expect(patchState).toHaveBeenNthCalledWith(2, [{
      patch: {
        activeDraftId: "draft-1",
        draftTabs: [{ content: "newest", id: "draft-1", name: "Draft.md", path: null }],
        fileTreeOpen: true,
        openWindows: []
      },
      type: "patch-ui-layout",
      windowLabel: "main"
    }]);
    await newer;
  });

  it.each([
    ["unclassified", new Error("validation failed")],
    ["authentication", Object.assign(new Error("authentication failed"), {
      code: "authentication_unavailable",
      kind: "network"
    })],
    ["protocol", Object.assign(new Error("protocol failed"), { code: "internal_error" })]
  ])("does not retry a %s persistence failure", async (_label, failure) => {
    const { patchState } = appConfigHarness();
    patchState.mockRejectedValue(failure);

    await expect(saveStoredRecentMarkdownFile({
      name: "A.md",
      path: "kernel-workspace://primary/notes/a.md"
    })).rejects.toBe(failure);

    expect(patchState).toHaveBeenCalledOnce();
  });

  it("retries one recognized transient patch and warns once after repeated failure", async () => {
    const { patchState } = appConfigHarness();
    patchState.mockRejectedValue(Object.assign(new Error("network unavailable"), {
      kind: "network"
    }));

    await expect(saveStoredRecentMarkdownFile({
      name: "A.md",
      path: "kernel-workspace://primary/notes/a.md"
    })).rejects.toThrow("network unavailable");

    expect(patchState).toHaveBeenCalledTimes(2);
    expect(showAppToast).toHaveBeenCalledTimes(1);
    expect(showAppToast).toHaveBeenCalledWith(expect.objectContaining({ status: "warning" }));
  });

  it("unregisters page, visibility, and native-exit listeners when retired", async () => {
    const { appConfig } = appConfigHarness();
    const unlistenExit = vi.fn();
    const runtime = createDefaultAppRuntime();
    configureAppRuntime({
      ...runtime,
      appConfig,
      window: {
        ...runtime.window,
        listenAppExitRequested: vi.fn(async () => unlistenExit)
      }
    });
    const addWindowListener = vi.spyOn(window, "addEventListener");
    const removeWindowListener = vi.spyOn(window, "removeEventListener");
    const addDocumentListener = vi.spyOn(document, "addEventListener");
    const removeDocumentListener = vi.spyOn(document, "removeEventListener");

    await saveStoredWorkspaceState({ fileTreeOpen: true });
    await Promise.resolve();
    retireAppConfigStatePersistence();

    expect(addWindowListener.mock.calls.some(([type]) => type === "pagehide")).toBe(true);
    expect(removeWindowListener.mock.calls.some(([type]) => type === "pagehide")).toBe(true);
    expect(addDocumentListener.mock.calls.some(([type]) => type === "visibilitychange")).toBe(true);
    expect(removeDocumentListener.mock.calls.some(([type]) => type === "visibilitychange")).toBe(true);
    expect(unlistenExit).toHaveBeenCalledOnce();
  });

  it("does not create a coordinator or listeners solely to retire persistence", () => {
    const runtime = createDefaultAppRuntime();
    const listenAppExitRequested = vi.fn(async () => () => undefined);
    configureAppRuntime({
      ...runtime,
      window: { ...runtime.window, listenAppExitRequested }
    });
    const addWindowListener = vi.spyOn(window, "addEventListener");
    const addDocumentListener = vi.spyOn(document, "addEventListener");

    retireAppConfigStatePersistence();

    expect(addWindowListener).not.toHaveBeenCalled();
    expect(addDocumentListener).not.toHaveBeenCalled();
    expect(listenAppExitRequested).not.toHaveBeenCalled();
  });
});
