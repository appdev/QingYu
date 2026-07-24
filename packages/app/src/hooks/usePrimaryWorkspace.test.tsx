import { parentPathFromPath } from "@markra/shared";
import { act, renderHook, waitFor } from "@testing-library/react";
import {
  loadPrimaryWorkspaceState,
  savePrimaryWorkspaceState,
  type PrimaryWorkspaceState
} from "../lib/settings/local-state";
import {
  configureAppRuntime,
  createDefaultAppRuntime,
  getAppRuntime,
  resetAppRuntimeForTests,
  type RuntimeStore
} from "../runtime";
import { usePrimaryWorkspace } from "./usePrimaryWorkspace";

type Deferred<T> = {
  promise: Promise<T>;
  reject: (error: unknown) => unknown;
  resolve: (value: T) => unknown;
};

function deferred<T>(): Deferred<T> {
  let reject!: (error: unknown) => unknown;
  let resolve!: (value: T) => unknown;
  const promise = new Promise<T>((promiseResolve, promiseReject) => {
    reject = promiseReject;
    resolve = promiseResolve;
  });
  return { promise, reject, resolve };
}

function createMemoryStore(values: Map<string, unknown>): RuntimeStore {
  return {
    delete: vi.fn(async (key: string) => values.delete(key)),
    get: async <T,>(key: string) => values.get(key) as T | undefined,
    save: vi.fn(async () => undefined),
    set: vi.fn(async (key: string, value: unknown) => values.set(key, value))
  };
}

describe("primary workspace controller", () => {
  const localState = new Map<string, unknown>();
  const mockOpenMarkdownFolder = vi.fn();
  const mockLoadStore = vi.fn();
  const mockResolveManagedRoot = vi.fn();
  const mockResolveMarkdownFolder = vi.fn();
  const eventListeners = new Set<(event: { payload: { generation: number; sourceId: string } }) => unknown>();

  function seedPrimaryWorkspace(
    desktopPath: string | null,
    onboardingCompleted: boolean,
    managedName: string | null = null
  ) {
    localState.set("primaryWorkspace", {
      desktopWorkspaceRoot: parentPathFromPath(desktopPath),
      desktopPath,
      managedName,
      onboardingCompleted,
      version: 3
    } satisfies PrimaryWorkspaceState);
  }

  beforeEach(() => {
    localState.clear();
    mockLoadStore.mockReset();
    mockLoadStore.mockImplementation(async () => createMemoryStore(localState));
    mockOpenMarkdownFolder.mockReset();
    mockResolveManagedRoot.mockReset();
    mockResolveMarkdownFolder.mockReset();
    eventListeners.clear();

    const defaultRuntime = createDefaultAppRuntime();
    configureAppRuntime({
      ...defaultRuntime,
      events: {
        emit: vi.fn(async (_event, payload) => {
          eventListeners.forEach((listener) => listener({
            payload: payload as { generation: number; sourceId: string }
          }));
        }),
        isAvailable: () => true,
        listen: vi.fn(async (_event, listener) => {
          eventListeners.add(listener as (event: { payload: { generation: number; sourceId: string } }) => unknown);
          return () => eventListeners.delete(
            listener as (event: { payload: { generation: number; sourceId: string } }) => unknown
          );
        })
      },
      files: {
        ...defaultRuntime.files,
        openMarkdownFolder: mockOpenMarkdownFolder,
        resolveMarkdownFolder: mockResolveMarkdownFolder
      },
      platform: {
        ...defaultRuntime.platform,
        resolveFormFactor: () => "desktop"
      },
      settings: {
        loadStore: mockLoadStore
      },
      workspace: {
        resolveManagedRoot: mockResolveManagedRoot
      }
    });
  });

  afterEach(() => {
    resetAppRuntimeForTests();
    vi.restoreAllMocks();
  });

  it("keeps narrow desktop semantics even when compact layout is active", async () => {
    Object.defineProperty(window, "innerWidth", { configurable: true, value: 375 });

    const { result } = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));

    await waitFor(() => expect(result.current.status).toBe("needs-onboarding"));
    expect(result.current.canChooseDesktopRoot).toBe(true);
    expect(mockResolveManagedRoot).not.toHaveBeenCalled();
    expect(mockResolveMarkdownFolder).not.toHaveBeenCalled();
  });

  it("resolves only the managed workspace after fresh true-mobile onboarding", async () => {
    mockResolveManagedRoot.mockResolvedValue("/app-data/workspaces/个人 笔记");
    const { result } = renderHook(() => usePrimaryWorkspace({ trueMobile: true }));

    await waitFor(() => expect(result.current.status).toBe("needs-onboarding"));

    let createdRoot: string | null = null;
    await act(async () => {
      createdRoot = await result.current.commitManagedRoot("个人 笔记");
    });

    expect(createdRoot).toBe("/app-data/workspaces/个人 笔记");
    expect(result.current).toMatchObject({
      error: null,
      managedName: "个人 笔记",
      root: "/app-data/workspaces/个人 笔记",
      status: "ready"
    });
    expect(mockResolveManagedRoot).toHaveBeenCalledWith("个人 笔记");
    expect(mockOpenMarkdownFolder).not.toHaveBeenCalled();
    expect(mockResolveMarkdownFolder).not.toHaveBeenCalled();
    await expect(loadPrimaryWorkspaceState()).resolves.toEqual({
      desktopWorkspaceRoot: null,
      desktopPath: null,
      managedName: "个人 笔记",
      onboardingCompleted: true,
      version: 3
    });
  });

  it("automatically resolves the managed workspace on later true-mobile launches", async () => {
    seedPrimaryWorkspace(null, true, "personal");
    mockResolveManagedRoot.mockResolvedValue("/app-data/workspaces/personal");

    const { result } = renderHook(() => usePrimaryWorkspace({ trueMobile: true }));

    await waitFor(() => expect(result.current.status).toBe("ready"));
    expect(result.current.root).toBe("/app-data/workspaces/personal");
    expect(result.current.managedName).toBe("personal");
    expect(mockResolveManagedRoot).toHaveBeenCalledTimes(1);
    expect(mockResolveManagedRoot).toHaveBeenCalledWith("personal");
    expect(mockOpenMarkdownFolder).not.toHaveBeenCalled();
  });

  it.each(["resolve", "save"] as const)(
    "keeps the previous ready managed root when a remote notebook %s fails after bootstrap",
    async (failureStage) => {
      const persistedState: PrimaryWorkspaceState = {
        desktopWorkspaceRoot: null,
        desktopPath: null,
        managedName: "A",
        onboardingCompleted: true,
        version: 3
      };
      const runtime = getAppRuntime();
      const writePrimaryWorkspaceState = vi.fn(async ({ state }: { state: unknown }) => {
        const primaryWorkspaceState = state as PrimaryWorkspaceState;
        if (failureStage === "save" && primaryWorkspaceState.managedName === "B") {
          throw new Error("managed-state-save-failed");
        }
        return { applied: true, state: primaryWorkspaceState };
      });
      configureAppRuntime({
        ...runtime,
        settings: {
          ...runtime.settings,
          readPrimaryWorkspaceState: vi.fn(async () => persistedState),
          writePrimaryWorkspaceState
        }
      });
      mockResolveManagedRoot.mockImplementation(async (name: string) => {
        if (failureStage === "resolve" && name === "B") {
          throw new Error("managed-root-resolve-failed");
        }
        return `/app-data/workspaces/${name}`;
      });
      const { result } = renderHook(() => usePrimaryWorkspace({ trueMobile: true }));
      await waitFor(() => expect(result.current).toMatchObject({
        managedName: "A",
        root: "/app-data/workspaces/A",
        status: "ready"
      }));

      let selectedRoot: string | null = "/app-data/workspaces/B";
      await act(async () => {
        selectedRoot = await result.current.commitManagedRoot("B");
      });

      expect(selectedRoot).toBeNull();
      expect(result.current).toMatchObject({
        error: null,
        managedName: "A",
        root: "/app-data/workspaces/A",
        status: "ready",
        workspaceRoot: null
      });
      if (failureStage === "resolve") expect(writePrimaryWorkspaceState).not.toHaveBeenCalled();
    }
  );

  it("reopens a persisted desktop path through native folder resolution", async () => {
    seedPrimaryWorkspace("/alias/Notes", true);
    mockResolveMarkdownFolder.mockImplementation(async (path: string) => {
      if (path === "/alias/Notes") return { name: "Notes", path: "/canonical/Notes" };
      if (path === "/alias") return { name: "canonical", path: "/canonical" };
      return { name: "canonical", path };
    });

    const { result } = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));

    await waitFor(() => expect(result.current.status).toBe("ready"));
    expect(result.current.root).toBe("/canonical/Notes");
    expect(mockResolveMarkdownFolder).toHaveBeenCalledWith("/alias/Notes");
    expect(mockOpenMarkdownFolder).not.toHaveBeenCalled();
    await expect(loadPrimaryWorkspaceState()).resolves.toEqual({
      desktopWorkspaceRoot: "/canonical",
      desktopPath: "/canonical/Notes",
      managedName: null,
      onboardingCompleted: true,
      version: 3
    });
  });

  it("reports recovery instead of substituting a recent external folder", async () => {
    seedPrimaryWorkspace("/missing/Notes", true);
    mockResolveMarkdownFolder.mockRejectedValue(new Error("folder-unavailable"));

    const { result } = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));

    await waitFor(() => expect(result.current.status).toBe("recovery"));
    expect(result.current.root).toBeNull();
    expect(result.current.error).toBe("folder-unavailable");
    expect(mockOpenMarkdownFolder).not.toHaveBeenCalled();
  });

  it("persists the canonical folder returned for a desktop commit", async () => {
    mockResolveMarkdownFolder.mockImplementation(async (path: string) => {
      if (path === "/alias/Notes") return { name: "Notes", path: "/canonical/Notes" };
      if (path === "/alias") return { name: "canonical", path: "/canonical" };
      return { name: "canonical", path };
    });
    const { result } = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
    await waitFor(() => expect(result.current.status).toBe("needs-onboarding"));

    let selectedRoot: string | null = null;
    await act(async () => {
      selectedRoot = await result.current.commitDesktopRoot("/alias/Notes");
    });

    expect(selectedRoot).toBe("/canonical/Notes");
    expect(result.current).toMatchObject({ root: "/canonical/Notes", status: "ready" });
    await expect(loadPrimaryWorkspaceState()).resolves.toEqual({
      desktopWorkspaceRoot: "/canonical",
      desktopPath: "/canonical/Notes",
      managedName: null,
      onboardingCompleted: true,
      version: 3
    });
  });

  it("persists the canonical desktop notebook and its canonical Workspace root atomically", async () => {
    mockResolveMarkdownFolder.mockImplementation(async (path: string) => {
      if (path === "/alias/Workspace/Notes") {
        return { name: "Notes", path: "/canonical/Workspace/Notes" };
      }
      if (path === "/canonical/Workspace") {
        return { name: "Workspace", path: "/canonical/Workspace" };
      }
      throw new Error(`unexpected path: ${path}`);
    });
    const { result } = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
    await waitFor(() => expect(result.current.status).toBe("needs-onboarding"));

    await act(async () => {
      await result.current.commitDesktopRoot("/alias/Workspace/Notes");
    });

    expect(mockResolveMarkdownFolder.mock.calls.map(([path]) => path)).toEqual([
      "/alias/Workspace/Notes",
      "/canonical/Workspace"
    ]);
    expect(result.current).toMatchObject({
      root: "/canonical/Workspace/Notes",
      status: "ready",
      workspaceRoot: "/canonical/Workspace"
    });
    await expect(loadPrimaryWorkspaceState()).resolves.toEqual({
      desktopWorkspaceRoot: "/canonical/Workspace",
      desktopPath: "/canonical/Workspace/Notes",
      managedName: null,
      onboardingCompleted: true,
      version: 3
    });
  });

  it.each(["resolve", "save"] as const)(
    "restores the persisted desktop authority when a new root %s fails",
    async (failureStage) => {
      const persistedState: PrimaryWorkspaceState = {
        desktopWorkspaceRoot: "/Workspace",
        desktopPath: "/Workspace/A",
        managedName: null,
        onboardingCompleted: true,
        version: 3
      };
      const runtime = getAppRuntime();
      const writePrimaryWorkspaceState = vi.fn(async () => {
        if (failureStage === "save") throw new Error("state-save-failed");
        return { applied: true, state: persistedState };
      });
      configureAppRuntime({
        ...runtime,
        settings: {
          ...runtime.settings,
          readPrimaryWorkspaceState: vi.fn(async () => persistedState),
          writePrimaryWorkspaceState
        }
      });
      mockResolveMarkdownFolder.mockImplementation(async (path: string) => {
        if (failureStage === "resolve" && path === "/Workspace/B") {
          throw new Error("target-resolve-failed");
        }
        return { name: path.split("/").at(-1) ?? "", path };
      });
      const { result } = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
      await waitFor(() => expect(result.current).toMatchObject({
        root: "/Workspace/A",
        status: "ready",
        workspaceRoot: "/Workspace"
      }));

      let selectedRoot: string | null = "/Workspace/B";
      await act(async () => {
        selectedRoot = await result.current.commitDesktopRoot("/Workspace/B");
      });

      expect(selectedRoot).toBeNull();
      expect(result.current).toMatchObject({
        error: null,
        root: "/Workspace/A",
        status: "ready",
        workspaceRoot: "/Workspace"
      });
      await expect(loadPrimaryWorkspaceState()).resolves.toEqual(persistedState);
      if (failureStage === "resolve") expect(writePrimaryWorkspaceState).not.toHaveBeenCalled();
    }
  );

  it.each([
    ["Unix", "/"],
    ["Windows", "C:\\"]
  ])("rejects the %s filesystem root as a desktop notebook", async (_platform, path) => {
    mockResolveMarkdownFolder.mockImplementation(async () => ({ name: "root", path }));
    const { result } = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
    await waitFor(() => expect(result.current.status).toBe("needs-onboarding"));

    let selectedRoot: string | null = path;
    await act(async () => {
      selectedRoot = await result.current.commitDesktopRoot(path);
    });

    expect(selectedRoot).toBeNull();
    expect(result.current).toMatchObject({ root: null, status: "error", workspaceRoot: null });
    expect(mockResolveMarkdownFolder).toHaveBeenCalledTimes(1);
    await expect(loadPrimaryWorkspaceState()).resolves.toEqual({
      desktopWorkspaceRoot: null,
      desktopPath: null,
      managedName: null,
      onboardingCompleted: false,
      version: 3
    });
  });

  it("canonicalizes the persisted Workspace root and notebook together during reload", async () => {
    localState.set("primaryWorkspace", {
      desktopWorkspaceRoot: "/alias/Workspace",
      desktopPath: "/alias/Workspace/Notes",
      managedName: null,
      onboardingCompleted: true,
      version: 3
    });
    mockResolveMarkdownFolder.mockImplementation(async (path: string) => {
      if (path === "/alias/Workspace") {
        return { name: "Workspace", path: "/canonical/Workspace" };
      }
      if (path === "/alias/Workspace/Notes") {
        return { name: "Notes", path: "/canonical/Workspace/Notes" };
      }
      return { name: path.split("/").at(-1) ?? "", path };
    });

    const { result } = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));

    await waitFor(() => expect(result.current.status).toBe("ready"));
    expect(result.current).toMatchObject({
      root: "/canonical/Workspace/Notes",
      workspaceRoot: "/canonical/Workspace"
    });
    expect(mockResolveMarkdownFolder).toHaveBeenCalledWith("/alias/Workspace");
    expect(mockResolveMarkdownFolder).toHaveBeenCalledWith("/alias/Workspace/Notes");
    await expect(loadPrimaryWorkspaceState()).resolves.toEqual({
      desktopWorkspaceRoot: "/canonical/Workspace",
      desktopPath: "/canonical/Workspace/Notes",
      managedName: null,
      onboardingCompleted: true,
      version: 3
    });
  });

  it("persists a deferred desktop setup without inventing a workspace", async () => {
    const { result } = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
    await waitFor(() => expect(result.current.status).toBe("needs-onboarding"));

    await act(async () => result.current.deferDesktopSetup());

    expect(result.current).toMatchObject({ error: null, root: null, status: "deferred" });
    await expect(loadPrimaryWorkspaceState()).resolves.toEqual({
      desktopWorkspaceRoot: null,
      desktopPath: null,
      managedName: null,
      onboardingCompleted: true,
      version: 3
    });
  });

  it("reloads another controller after a cross-window root choice", async () => {
    mockResolveMarkdownFolder.mockImplementation(async (path: string) => ({
      name: path === "/Notes-B" ? "Notes B" : "Notes A",
      path
    }));
    seedPrimaryWorkspace("/Notes-A", true);
    const first = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
    const second = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
    await waitFor(() => expect(first.result.current.root).toBe("/Notes-A"));
    await waitFor(() => expect(second.result.current.root).toBe("/Notes-A"));

    await act(async () => first.result.current.commitDesktopRoot("/Notes-B"));

    await waitFor(() => expect(second.result.current.root).toBe("/Notes-B"));
    expect(second.result.current.status).toBe("ready");
  });

  it("ignores stale root resolution completed after a cross-window change", async () => {
    const staleResolution = deferred<{ name: string; path: string }>();
    seedPrimaryWorkspace("/Notes-A", true);
    mockResolveMarkdownFolder.mockImplementation((path: string) => path === "/Notes-A"
      ? staleResolution.promise
      : Promise.resolve({ name: "Notes B", path }));
    const { result } = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
    await waitFor(() => expect(mockResolveMarkdownFolder).toHaveBeenCalledWith("/Notes-A"));

    seedPrimaryWorkspace("/Notes-B", true);
    act(() => {
      eventListeners.forEach((listener) => listener({
        payload: { generation: 9, sourceId: "other-window" }
      }));
    });
    await waitFor(() => expect(result.current.root).toBe("/Notes-B"));

    await act(async () => {
      staleResolution.resolve({ name: "Notes A", path: "/Notes-A" });
      await staleResolution.promise;
    });

    expect(result.current).toMatchObject({ root: "/Notes-B", status: "ready" });
  });

  it("ignores stale desktop resolution after the runtime changes to true mobile", async () => {
    seedPrimaryWorkspace("/desktop/Notes", true);
    const desktopResolution = deferred<{ name: string; path: string }>();
    mockResolveMarkdownFolder.mockReturnValue(desktopResolution.promise);
    mockResolveManagedRoot.mockResolvedValue("/app-data/workspaces/personal");

    const { rerender, result } = renderHook(
      ({ trueMobile }) => usePrimaryWorkspace({ trueMobile }),
      { initialProps: { trueMobile: false } }
    );
    await waitFor(() => expect(mockResolveMarkdownFolder).toHaveBeenCalledTimes(1));

    rerender({ trueMobile: true });
    await waitFor(() => expect(result.current.status).toBe("needs-onboarding"));
    expect(result.current.root).toBeNull();

    await act(async () => {
      desktopResolution.resolve({ name: "Notes", path: "/canonical/desktop/Notes" });
      await desktopResolution.promise;
    });

    expect(result.current).toMatchObject({
      root: null,
      status: "needs-onboarding"
    });
    await expect(loadPrimaryWorkspaceState()).resolves.toEqual({
      desktopWorkspaceRoot: "/desktop",
      desktopPath: "/desktop/Notes",
      managedName: null,
      onboardingCompleted: true,
      version: 3
    });
  });

  it("retries a persisted desktop path from recovery", async () => {
    seedPrimaryWorkspace("/Notes", true);
    mockResolveMarkdownFolder
      .mockRejectedValueOnce(new Error("temporarily-unavailable"))
      .mockImplementation(async (path: string) => ({ name: "Notes", path }));
    const { result } = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
    await waitFor(() => expect(result.current.status).toBe("recovery"));

    await act(async () => result.current.retry());

    expect(result.current).toMatchObject({ error: null, root: "/Notes", status: "ready" });
    expect(mockResolveMarkdownFolder).toHaveBeenCalledTimes(3);
  });

  it("shows onboarding on the next launch after reset without forgetting the desktop path", async () => {
    seedPrimaryWorkspace("/Notes", true);
    mockResolveMarkdownFolder.mockImplementation(async (path: string) => ({ name: "Notes", path }));
    const firstLaunch = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
    const alreadyOpenWindow = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
    await waitFor(() => expect(firstLaunch.result.current.status).toBe("ready"));
    await waitFor(() => expect(alreadyOpenWindow.result.current.status).toBe("ready"));

    await act(async () => firstLaunch.result.current.resetOnboarding());

    expect(firstLaunch.result.current).toMatchObject({ root: "/Notes", status: "ready" });
    expect(alreadyOpenWindow.result.current).toMatchObject({ root: "/Notes", status: "ready" });
    await expect(loadPrimaryWorkspaceState()).resolves.toEqual({
      desktopWorkspaceRoot: "/",
      desktopPath: "/Notes",
      managedName: null,
      onboardingCompleted: true,
      onboardingRequestedForNextLaunch: true,
      version: 3
    });
    firstLaunch.unmount();

    const nextLaunch = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
    await waitFor(() => expect(nextLaunch.result.current.status).toBe("needs-onboarding"));
    expect(nextLaunch.result.current.root).toBeNull();
    await expect(loadPrimaryWorkspaceState()).resolves.toEqual({
      desktopWorkspaceRoot: "/",
      desktopPath: "/Notes",
      managedName: null,
      onboardingCompleted: true,
      onboardingRequestedForNextLaunch: true,
      version: 3
    });
  });

  it("reports recovery when a retained reset-state desktop path is invalid", async () => {
    seedPrimaryWorkspace("/missing/Notes", false);
    mockResolveMarkdownFolder.mockRejectedValue(new Error("folder-unavailable"));

    const { result } = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));

    await waitFor(() => expect(result.current.status).toBe("recovery"));
    expect(result.current.root).toBeNull();
    expect(result.current.error).toBe("folder-unavailable");
    expect(mockResolveMarkdownFolder).toHaveBeenCalledWith("/missing/Notes");
  });

  it("canonicalizes a valid retained reset-state path without installing it as ready", async () => {
    seedPrimaryWorkspace("/alias/Notes", false);
    mockResolveMarkdownFolder.mockImplementation(async (path: string) => {
      if (path === "/alias/Notes") return { name: "Notes", path: "/canonical/Notes" };
      if (path === "/alias") return { name: "canonical", path: "/canonical" };
      return { name: "canonical", path };
    });

    const { result } = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));

    await waitFor(() => expect(result.current.status).toBe("needs-onboarding"));
    expect(result.current.root).toBeNull();
    expect(mockResolveMarkdownFolder).toHaveBeenCalledWith("/alias/Notes");
    await expect(loadPrimaryWorkspaceState()).resolves.toEqual({
      desktopWorkspaceRoot: "/canonical",
      desktopPath: "/canonical/Notes",
      managedName: null,
      onboardingCompleted: false,
      version: 3
    });
  });

  it("retries loading local state after a transient storage error", async () => {
    mockLoadStore.mockRejectedValueOnce(new Error("local-state-unavailable"));
    const { result } = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
    await waitFor(() => expect(result.current.status).toBe("error"));

    await act(async () => result.current.retry());

    expect(result.current).toMatchObject({
      error: null,
      root: null,
      status: "needs-onboarding"
    });
    expect(mockLoadStore).toHaveBeenCalledTimes(2);
  });

  it("keeps the later root choice after an earlier save completes last", async () => {
    const firstSaveStarted = deferred<undefined>();
    const releaseFirstSave = deferred<undefined>();
    mockLoadStore.mockImplementation(async () => {
      const snapshot = new Map(localState);
      return {
        delete: vi.fn(async (key: string) => snapshot.delete(key)),
        get: async <T,>(key: string) => snapshot.get(key) as T | undefined,
        save: vi.fn(async () => {
          const primaryWorkspace = snapshot.get("primaryWorkspace") as PrimaryWorkspaceState | undefined;
          if (primaryWorkspace?.desktopPath === "/Notes-A") {
            firstSaveStarted.resolve(undefined);
            await releaseFirstSave.promise;
          }
          snapshot.forEach((value, key) => localState.set(key, value));
        }),
        set: vi.fn(async (key: string, value: unknown) => snapshot.set(key, value))
      };
    });
    mockResolveMarkdownFolder.mockImplementation(async (path: string) => ({
      name: path.endsWith("Notes-B") ? "Notes B" : "Notes A",
      path
    }));

    const first = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
    const second = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
    await waitFor(() => expect(first.result.current.status).toBe("needs-onboarding"));
    await waitFor(() => expect(second.result.current.status).toBe("needs-onboarding"));

    let firstChoice!: Promise<string | null>;
    act(() => {
      firstChoice = first.result.current.commitDesktopRoot("/Notes-A");
    });
    await firstSaveStarted.promise;

    await act(async () => {
      await second.result.current.commitDesktopRoot("/Notes-B");
    });
    expect(second.result.current).toMatchObject({ root: "/Notes-B", status: "ready" });

    await act(async () => {
      releaseFirstSave.resolve(undefined);
      await firstChoice;
    });

    await expect(loadPrimaryWorkspaceState()).resolves.toMatchObject({
      desktopPath: "/Notes-B",
      onboardingCompleted: true
    });
    expect(localState.get("primaryWorkspace")).toMatchObject({ desktopPath: "/Notes-B" });
  });

  it("rejects a stale canonical path after a later user root choice", async () => {
    seedPrimaryWorkspace("/alias/Notes-A", true);
    const canonicalSaveStarted = deferred<undefined>();
    const releaseCanonicalSave = deferred<undefined>();
    mockLoadStore.mockImplementation(async () => {
      const snapshot = new Map(localState);
      return {
        delete: vi.fn(async (key: string) => snapshot.delete(key)),
        get: async <T,>(key: string) => snapshot.get(key) as T | undefined,
        save: vi.fn(async () => {
          const primaryWorkspace = snapshot.get("primaryWorkspace") as PrimaryWorkspaceState | undefined;
          if (primaryWorkspace?.desktopPath === "/canonical/Notes-A") {
            canonicalSaveStarted.resolve(undefined);
            await releaseCanonicalSave.promise;
          }
          snapshot.forEach((value, key) => localState.set(key, value));
        }),
        set: vi.fn(async (key: string, value: unknown) => snapshot.set(key, value))
      };
    });
    mockResolveMarkdownFolder.mockImplementation(async (path: string) => ({
      name: path.endsWith("Notes-B") ? "Notes B" : "Notes A",
      path: path === "/alias/Notes-A"
        ? "/canonical/Notes-A"
        : path === "/alias" ? "/canonical" : path
    }));

    const canonicalizingWindow = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
    await canonicalSaveStarted.promise;
    const choosingWindow = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
    await waitFor(() => expect(choosingWindow.result.current.root).toBe("/canonical/Notes-A"));

    await act(async () => {
      await choosingWindow.result.current.commitDesktopRoot("/Notes-B");
    });
    expect(choosingWindow.result.current.root).toBe("/Notes-B");

    await act(async () => {
      releaseCanonicalSave.resolve(undefined);
    });
    await waitFor(() => expect(canonicalizingWindow.result.current.root).toBe("/Notes-B"));
    expect(localState.get("primaryWorkspace")).toMatchObject({ desktopPath: "/Notes-B" });
    await expect(loadPrimaryWorkspaceState()).resolves.toMatchObject({ desktopPath: "/Notes-B" });
  });

  it("resolves the authoritative root after canonical CAS rejection without a change event", async () => {
    seedPrimaryWorkspace("/alias/Notes-A", true);
    const aliasResolution = deferred<{ name: string; path: string }>();
    mockResolveMarkdownFolder.mockImplementation(async (path: string) => {
      if (path === "/alias/Notes-A") return aliasResolution.promise;
      if (path === "/alias") return { name: "canonical", path: "/canonical" };
      return { name: "Notes B", path };
    });

    const controller = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
    await waitFor(() => expect(mockResolveMarkdownFolder).toHaveBeenCalledWith("/alias/Notes-A"));

    await savePrimaryWorkspaceState({
      desktopWorkspaceRoot: "/",
      desktopPath: "/Notes-B",
      managedName: null,
      onboardingCompleted: true,
      version: 3
    });
    await act(async () => {
      aliasResolution.resolve({ name: "Notes A", path: "/canonical/Notes-A" });
    });

    await waitFor(() => expect(controller.result.current).toMatchObject({
      error: null,
      root: "/Notes-B",
      status: "ready"
    }));
  });

  it("uses the authoritative state returned by canonical save instead of transitioning stale folder A", async () => {
    seedPrimaryWorkspace("/alias/Notes-A", true);
    const canonicalSaveStarted = deferred<undefined>();
    const releaseCanonicalSave = deferred<undefined>();
    mockLoadStore.mockImplementation(async () => {
      const snapshot = new Map(localState);
      return {
        delete: vi.fn(async (key: string) => snapshot.delete(key)),
        get: async <T,>(key: string) => snapshot.get(key) as T | undefined,
        save: vi.fn(async () => {
          const primaryWorkspace = snapshot.get("primaryWorkspace") as PrimaryWorkspaceState | undefined;
          if (primaryWorkspace?.desktopPath === "/canonical/Notes-A") {
            canonicalSaveStarted.resolve(undefined);
            await releaseCanonicalSave.promise;
          }
          snapshot.forEach((value, key) => localState.set(key, value));
        }),
        set: vi.fn(async (key: string, value: unknown) => snapshot.set(key, value))
      };
    });
    mockResolveMarkdownFolder.mockImplementation(async (path: string) => ({
      name: path.endsWith("Notes-B") ? "Notes B" : "Notes A",
      path: path === "/alias/Notes-A"
        ? "/canonical/Notes-A"
        : path === "/alias" ? "/canonical" : path
    }));

    const controller = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
    await canonicalSaveStarted.promise;
    await savePrimaryWorkspaceState({
      desktopWorkspaceRoot: "/",
      desktopPath: "/Notes-B",
      managedName: null,
      onboardingCompleted: true,
      onboardingRequestedForNextLaunch: true,
      version: 3
    });

    await act(async () => {
      releaseCanonicalSave.resolve(undefined);
    });
    await waitFor(() => expect(controller.result.current).toMatchObject({
      error: null,
      root: null,
      status: "needs-onboarding"
    }));
    expect(controller.result.current.root).not.toBe("/canonical/Notes-A");
  });

  it("preserves reset flags when another real controller canonicalizes the same alias", async () => {
    seedPrimaryWorkspace("/alias/Notes-A", true);
    const firstAliasResolution = deferred<{ name: string; path: string }>();
    let aliasResolutionCount = 0;
    mockResolveMarkdownFolder.mockImplementation(async (path: string) => {
      if (path === "/alias/Notes-A" && aliasResolutionCount === 0) {
        aliasResolutionCount += 1;
        return firstAliasResolution.promise;
      }
      if (path === "/alias/Notes-A") {
        return { name: "Notes A", path: "/canonical/Notes-A" };
      }
      if (path === "/alias") return { name: "canonical", path: "/canonical" };
      return { name: "Notes A", path };
    });

    const canonicalizingController = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
    await waitFor(() => expect(mockResolveMarkdownFolder).toHaveBeenCalledWith("/alias/Notes-A"));
    const resettingController = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
    await waitFor(() => expect(resettingController.result.current.status).toBe("ready"));

    await act(async () => resettingController.result.current.resetOnboarding());
    await act(async () => {
      firstAliasResolution.resolve({ name: "Notes A", path: "/canonical/Notes-A" });
    });

    await waitFor(() => expect(canonicalizingController.result.current).toMatchObject({
      error: null,
      root: null,
      status: "needs-onboarding"
    }));
    await expect(loadPrimaryWorkspaceState()).resolves.toEqual({
      desktopWorkspaceRoot: "/canonical",
      desktopPath: "/canonical/Notes-A",
      managedName: null,
      onboardingCompleted: true,
      onboardingRequestedForNextLaunch: true,
      version: 3
    });
  });
});
