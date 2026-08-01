import { act, renderHook, waitFor } from "@testing-library/react";
import type { PrimaryWorkspaceState } from "../lib/settings/primary-workspace-state";
import {
  configureAppRuntime,
  createDefaultAppRuntime,
  resetAppRuntimeForTests,
  type AppWorkspaceRootPolicy
} from "../runtime";
import { usePrimaryWorkspace } from "./usePrimaryWorkspace";

type Deferred<T> = {
  promise: Promise<T>;
  reject: (error: unknown) => unknown;
  resolve: (value: T) => unknown;
};

type WorkspaceEvent = {
  payload: { generation: number; sourceId: string };
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

const emptyMetadata: PrimaryWorkspaceState = {
  desktopWorkspaceRoot: null,
  desktopPath: null,
  managedName: null,
  onboardingCompleted: false,
  version: 3
};

describe("primary workspace controller", () => {
  const readPrimaryWorkspaceState = vi.fn();
  const writePrimaryWorkspaceState = vi.fn();
  const loadStore = vi.fn(async () => {
    throw new Error("unexpected local-state access");
  });
  const eventListeners = new Set<(event: WorkspaceEvent) => unknown>();
  let metadata = emptyMetadata;

  beforeEach(() => {
    vi.clearAllMocks();
    eventListeners.clear();
    metadata = emptyMetadata;
  });

  afterEach(() => {
    resetAppRuntimeForTests();
    vi.restoreAllMocks();
  });

  function configureMetadataState(
    state: PrimaryWorkspaceState,
    options: {
      resolveManagedRoot?: (name: string) => Promise<string | null>;
      resolveMarkdownFolder?: (path: string) => Promise<{ name: string; path: string }>;
      rootPolicy?: AppWorkspaceRootPolicy;
    } = {}
  ) {
    const runtime = createDefaultAppRuntime();
    metadata = state;
    readPrimaryWorkspaceState.mockImplementation(async () => metadata);
    writePrimaryWorkspaceState.mockImplementation(async ({ expectedState, state: nextState }) => {
      if (expectedState !== undefined && JSON.stringify(expectedState) !== JSON.stringify(metadata)) {
        return { applied: false, state: metadata };
      }
      metadata = nextState as PrimaryWorkspaceState;
      return { applied: true, state: metadata };
    });
    configureAppRuntime({
      ...runtime,
      events: {
        emit: vi.fn(async (_event, payload) => {
          eventListeners.forEach((listener) => listener({
            payload: payload as WorkspaceEvent["payload"]
          }));
        }),
        isAvailable: () => true,
        listen: vi.fn(async (_event, listener) => {
          const typedListener = listener as (event: WorkspaceEvent) => unknown;
          eventListeners.add(typedListener);
          return () => eventListeners.delete(typedListener);
        })
      },
      files: {
        ...runtime.files,
        resolveMarkdownFolder: options.resolveMarkdownFolder ?? (async (path) => ({
          name: path.split("/").at(-1) ?? "",
          path
        }))
      },
      settings: {
        loadStore,
        readPrimaryWorkspaceState,
        writePrimaryWorkspaceState
      },
      workspace: {
        ...runtime.workspace,
        ...(options.resolveManagedRoot ? { resolveManagedRoot: options.resolveManagedRoot } : {}),
        ...(options.rootPolicy ? { rootPolicy: options.rootPolicy } : {})
      }
    });
  }

  it("uses a fixed mobile/server root without reading or writing host metadata", async () => {
    const resolveRoot = vi.fn(async () => "kernel-workspace://primary");
    configureMetadataState(emptyMetadata, {
      rootPolicy: {
        canChooseLocalRoot: false,
        kind: "fixed",
        resolveRoot
      }
    });

    const { result } = renderHook(() => usePrimaryWorkspace({ trueMobile: true }));
    await waitFor(() => expect(result.current.status).toBe("ready"));
    await act(async () => {
      await result.current.commitDesktopRoot("/attacker-root");
      await result.current.commitManagedRoot("attacker-root");
      await result.current.deferDesktopSetup();
      await result.current.resetOnboarding();
      await result.current.retry();
    });

    expect(result.current).toMatchObject({
      canChooseDesktopRoot: false,
      managedName: null,
      root: "kernel-workspace://primary",
      status: "ready",
      workspaceRoot: null
    });
    expect(resolveRoot).toHaveBeenCalledTimes(2);
    expect(readPrimaryWorkspaceState).not.toHaveBeenCalled();
    expect(writePrimaryWorkspaceState).not.toHaveBeenCalled();
    expect(loadStore).not.toHaveBeenCalled();
  });

  it("uses host-selectable onboarding metadata and retains the host-canonical committed identity", async () => {
    const commitRoot = vi.fn(async () => {
      metadata = {
        desktopWorkspaceRoot: "/canonical",
        desktopPath: "/canonical/Notes",
        managedName: null,
        onboardingCompleted: true,
        version: 3
      };
      return "kernel-workspace://primary";
    });
    const resolveRoot = vi.fn(async () => "kernel-workspace://primary");
    configureMetadataState(emptyMetadata, {
      rootPolicy: {
        canChooseLocalRoot: true,
        commitRoot,
        kind: "host-selectable",
        resolveRoot,
        selectRoot: vi.fn(async () => "/alias/Notes")
      }
    });

    const { result } = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
    await waitFor(() => expect(result.current.status).toBe("needs-onboarding"));
    expect(resolveRoot).not.toHaveBeenCalled();

    await act(async () => {
      await result.current.commitDesktopRoot("/alias/Notes");
    });

    expect(commitRoot).toHaveBeenCalledWith("/alias/Notes");
    expect(result.current).toMatchObject({
      root: "kernel-workspace://primary",
      status: "ready",
      workspaceRoot: null
    });
    expect(metadata).toEqual({
      desktopWorkspaceRoot: "/canonical",
      desktopPath: "/canonical/Notes",
      managedName: null,
      onboardingCompleted: true,
      version: 3
    });
    expect(readPrimaryWorkspaceState).toHaveBeenCalledTimes(2);
    expect(writePrimaryWorkspaceState).not.toHaveBeenCalled();
    expect(loadStore).not.toHaveBeenCalled();
  });

  it("restores a host-selectable active root only after native onboarding metadata permits it", async () => {
    const resolveRoot = vi.fn(async () => "kernel-workspace://primary");
    configureMetadataState({
      desktopWorkspaceRoot: "/Workspace",
      desktopPath: "/Workspace/Notes",
      managedName: null,
      onboardingCompleted: true,
      version: 3
    }, {
      rootPolicy: {
        canChooseLocalRoot: true,
        commitRoot: vi.fn(async () => "kernel-workspace://primary"),
        kind: "host-selectable",
        resolveRoot,
        selectRoot: vi.fn(async () => null)
      }
    });

    const { result } = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
    await waitFor(() => expect(result.current.status).toBe("ready"));

    expect(result.current.root).toBe("kernel-workspace://primary");
    expect(readPrimaryWorkspaceState).toHaveBeenCalledOnce();
    expect(resolveRoot).toHaveBeenCalledOnce();
  });

  it("writes host-selectable deferred and reset onboarding metadata", async () => {
    const rootPolicy = {
      canChooseLocalRoot: true as const,
      commitRoot: vi.fn(async () => "kernel-workspace://primary"),
      kind: "host-selectable" as const,
      resolveRoot: vi.fn(async () => "kernel-workspace://primary"),
      selectRoot: vi.fn(async () => null)
    };
    configureMetadataState(emptyMetadata, { rootPolicy });
    const first = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
    await waitFor(() => expect(first.result.current.status).toBe("needs-onboarding"));

    await act(async () => first.result.current.deferDesktopSetup());
    expect(first.result.current.status).toBe("deferred");
    expect(metadata).toMatchObject({ desktopPath: null, onboardingCompleted: true });
    first.unmount();

    metadata = {
      desktopWorkspaceRoot: "/Workspace",
      desktopPath: "/Workspace/Notes",
      managedName: null,
      onboardingCompleted: true,
      version: 3
    };
    const second = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
    await waitFor(() => expect(second.result.current.status).toBe("ready"));
    await act(async () => second.result.current.resetOnboarding());

    expect(metadata).toMatchObject({
      desktopPath: "/Workspace/Notes",
      onboardingRequestedForNextLaunch: true
    });
  });

  it("resolves managed onboarding from the native metadata bridge", async () => {
    const resolveManagedRoot = vi.fn(async () => "kernel-workspace://primary");
    configureMetadataState({
      desktopWorkspaceRoot: null,
      desktopPath: null,
      managedName: "personal",
      onboardingCompleted: true,
      version: 3
    }, { resolveManagedRoot });

    const { result } = renderHook(() => usePrimaryWorkspace({ trueMobile: true }));
    await waitFor(() => expect(result.current.status).toBe("ready"));

    expect(result.current).toMatchObject({
      managedName: "personal",
      root: "kernel-workspace://primary",
      status: "ready"
    });
    expect(resolveManagedRoot).toHaveBeenCalledWith("personal");
    expect(loadStore).not.toHaveBeenCalled();
  });

  it("canonicalizes a persisted desktop identity through a native CAS write", async () => {
    const original: PrimaryWorkspaceState = {
      desktopWorkspaceRoot: "/alias",
      desktopPath: "/alias/Notes",
      managedName: null,
      onboardingCompleted: true,
      version: 3
    };
    configureMetadataState(original, {
      resolveMarkdownFolder: async (path) => {
        const canonical = path === "/alias/Notes"
          ? "/canonical/Notes"
          : path === "/alias" ? "/canonical" : path;
        return { name: canonical.endsWith("Notes") ? "Notes" : "Workspace", path: canonical };
      }
    });

    const { result } = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
    await waitFor(() => expect(result.current.status).toBe("ready"));

    expect(result.current).toMatchObject({
      root: "/canonical/Notes",
      workspaceRoot: "/canonical"
    });
    expect(writePrimaryWorkspaceState).toHaveBeenCalledWith({
      expectedState: original,
      state: {
        ...original,
        desktopPath: "/canonical/Notes",
        desktopWorkspaceRoot: "/canonical"
      }
    });
  });

  it("reports desktop recovery when the persisted identity is unavailable", async () => {
    configureMetadataState({
      desktopWorkspaceRoot: "/missing",
      desktopPath: "/missing/Notes",
      managedName: null,
      onboardingCompleted: true,
      version: 3
    }, {
      resolveMarkdownFolder: async () => {
        throw new Error("folder-unavailable");
      }
    });

    const { result } = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
    await waitFor(() => expect(result.current.status).toBe("recovery"));

    expect(result.current).toMatchObject({ error: "folder-unavailable", root: null });
  });

  it("follows the authoritative native identity after a canonical CAS rejection", async () => {
    const original: PrimaryWorkspaceState = {
      desktopWorkspaceRoot: "/alias",
      desktopPath: "/alias/Notes-A",
      managedName: null,
      onboardingCompleted: true,
      version: 3
    };
    const authoritative: PrimaryWorkspaceState = {
      desktopWorkspaceRoot: "/",
      desktopPath: "/Notes-B",
      managedName: null,
      onboardingCompleted: true,
      version: 3
    };
    configureMetadataState(original, {
      resolveMarkdownFolder: async (path) => ({
        name: path.split("/").at(-1) ?? "",
        path: path === "/alias/Notes-A"
          ? "/canonical/Notes-A"
          : path === "/alias" ? "/canonical" : path
      })
    });
    writePrimaryWorkspaceState.mockImplementation(async ({ expectedState, state }) => {
      if (expectedState !== undefined) {
        metadata = authoritative;
        return { applied: false, state: authoritative };
      }
      metadata = state as PrimaryWorkspaceState;
      return { applied: true, state: metadata };
    });

    const { result } = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
    await waitFor(() => expect(result.current.status).toBe("ready"));

    expect(result.current.root).toBe("/Notes-B");
    expect(result.current.root).not.toBe("/canonical/Notes-A");
  });

  it("ignores a stale desktop resolution completed after a cross-window change", async () => {
    const stale = deferred<{ name: string; path: string }>();
    configureMetadataState({
      desktopWorkspaceRoot: "/",
      desktopPath: "/Notes-A",
      managedName: null,
      onboardingCompleted: true,
      version: 3
    }, {
      resolveMarkdownFolder: (path) => path === "/Notes-A"
        ? stale.promise
        : Promise.resolve({ name: path.split("/").at(-1) ?? "", path })
    });
    const { result } = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
    await waitFor(() => expect(eventListeners.size).toBe(1));

    metadata = {
      desktopWorkspaceRoot: "/",
      desktopPath: "/Notes-B",
      managedName: null,
      onboardingCompleted: true,
      version: 3
    };
    act(() => {
      eventListeners.forEach((listener) => listener({
        payload: { generation: 1, sourceId: "other-window" }
      }));
    });
    await waitFor(() => expect(result.current.root).toBe("/Notes-B"));

    await act(async () => {
      stale.resolve({ name: "Notes A", path: "/Notes-A" });
      await stale.promise;
    });
    expect(result.current).toMatchObject({ root: "/Notes-B", status: "ready" });
  });

  it("shows onboarding on the next launch after reset without forgetting the path", async () => {
    configureMetadataState({
      desktopWorkspaceRoot: "/Workspace",
      desktopPath: "/Workspace/Notes",
      managedName: null,
      onboardingCompleted: true,
      version: 3
    });
    const first = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
    await waitFor(() => expect(first.result.current.status).toBe("ready"));

    await act(async () => first.result.current.resetOnboarding());
    expect(first.result.current.root).toBe("/Workspace/Notes");
    first.unmount();

    const next = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
    await waitFor(() => expect(next.result.current.status).toBe("needs-onboarding"));
    expect(metadata).toMatchObject({
      desktopPath: "/Workspace/Notes",
      onboardingRequestedForNextLaunch: true
    });
  });

  it("retries native metadata loading after a transient failure", async () => {
    configureMetadataState(emptyMetadata);
    readPrimaryWorkspaceState.mockRejectedValueOnce(new Error("metadata-unavailable"));
    const { result } = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
    await waitFor(() => expect(result.current.status).toBe("error"));

    await act(async () => result.current.retry());

    expect(result.current).toMatchObject({ error: null, root: null, status: "needs-onboarding" });
    expect(readPrimaryWorkspaceState).toHaveBeenCalledTimes(2);
  });

  it("reloads host metadata when retrying after commit succeeded but its canonical read failed", async () => {
    const commitRoot = vi.fn(async () => {
      metadata = {
        desktopWorkspaceRoot: "/canonical",
        desktopPath: "/canonical/Notes",
        managedName: null,
        onboardingCompleted: true,
        version: 3
      };
      return "kernel-workspace://primary";
    });
    const resolveRoot = vi.fn(async () => "kernel-workspace://primary");
    configureMetadataState(emptyMetadata, {
      rootPolicy: {
        canChooseLocalRoot: true,
        commitRoot,
        kind: "host-selectable",
        resolveRoot,
        selectRoot: vi.fn(async () => null)
      }
    });
    readPrimaryWorkspaceState
      .mockResolvedValueOnce(emptyMetadata)
      .mockRejectedValueOnce(new Error("metadata-read-failed"))
      .mockImplementation(async () => metadata);
    const { result } = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
    await waitFor(() => expect(result.current.status).toBe("needs-onboarding"));

    await act(async () => result.current.commitDesktopRoot("/alias/Notes"));
    expect(result.current.status).toBe("error");

    await act(async () => result.current.retry());
    expect(result.current).toMatchObject({
      error: null,
      root: "kernel-workspace://primary",
      status: "ready"
    });
    expect(readPrimaryWorkspaceState).toHaveBeenCalledTimes(3);
    expect(resolveRoot).toHaveBeenCalledOnce();
  });

  it("adopts a concurrent host commit instead of clearing it during defer", async () => {
    const resolveRoot = vi.fn(async () => "kernel-workspace://primary");
    configureMetadataState(emptyMetadata, {
      rootPolicy: {
        canChooseLocalRoot: true,
        commitRoot: vi.fn(async () => "kernel-workspace://primary"),
        kind: "host-selectable",
        resolveRoot,
        selectRoot: vi.fn(async () => null)
      }
    });
    const { result } = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
    await waitFor(() => expect(result.current.status).toBe("needs-onboarding"));
    const authoritative: PrimaryWorkspaceState = {
      desktopWorkspaceRoot: "/Workspace",
      desktopPath: "/Workspace/Notes-B",
      managedName: null,
      onboardingCompleted: true,
      version: 3
    };
    metadata = authoritative;

    await act(async () => result.current.deferDesktopSetup());

    expect(metadata).toEqual(authoritative);
    expect(result.current).toMatchObject({ root: "kernel-workspace://primary", status: "ready" });
    expect(writePrimaryWorkspaceState).toHaveBeenCalledWith({
      expectedState: emptyMetadata,
      state: { ...emptyMetadata, onboardingCompleted: true }
    });
    expect(resolveRoot).toHaveBeenCalledOnce();
  });

  it("adopts a concurrent host commit instead of resetting its stale identity", async () => {
    const original: PrimaryWorkspaceState = {
      desktopWorkspaceRoot: "/Workspace",
      desktopPath: "/Workspace/Notes-A",
      managedName: null,
      onboardingCompleted: true,
      version: 3
    };
    const resolveRoot = vi.fn(async () => "kernel-workspace://primary");
    configureMetadataState(original, {
      rootPolicy: {
        canChooseLocalRoot: true,
        commitRoot: vi.fn(async () => "kernel-workspace://primary"),
        kind: "host-selectable",
        resolveRoot,
        selectRoot: vi.fn(async () => null)
      }
    });
    const { result } = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
    await waitFor(() => expect(result.current.status).toBe("ready"));
    const authoritative: PrimaryWorkspaceState = {
      ...original,
      desktopPath: "/Workspace/Notes-B"
    };
    metadata = authoritative;

    await act(async () => result.current.resetOnboarding());

    expect(metadata).toEqual(authoritative);
    expect(result.current).toMatchObject({ root: "kernel-workspace://primary", status: "ready" });
    expect(writePrimaryWorkspaceState).toHaveBeenCalledWith({
      expectedState: original,
      state: { ...original, onboardingRequestedForNextLaunch: true }
    });
    expect(resolveRoot).toHaveBeenCalledTimes(2);
  });

  it.each(["resolve", "save"] as const)(
    "rolls back to the ready desktop identity when a later %s fails",
    async (failureStage) => {
      configureMetadataState({
        desktopWorkspaceRoot: "/Workspace",
        desktopPath: "/Workspace/Notes-A",
        managedName: null,
        onboardingCompleted: true,
        version: 3
      }, {
        resolveMarkdownFolder: async (path) => {
          if (failureStage === "resolve" && path.endsWith("Notes-B")) {
            throw new Error("resolve-failed");
          }
          return { name: path.split("/").at(-1) ?? "", path };
        }
      });
      if (failureStage === "save") {
        writePrimaryWorkspaceState.mockImplementation(async ({ state }) => {
          const next = state as PrimaryWorkspaceState;
          if (next.desktopPath?.endsWith("Notes-B")) throw new Error("save-failed");
          metadata = next;
          return { applied: true, state: metadata };
        });
      }
      const { result } = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
      await waitFor(() => expect(result.current.root).toBe("/Workspace/Notes-A"));

      await act(async () => result.current.commitDesktopRoot("/Workspace/Notes-B"));

      expect(result.current).toMatchObject({
        error: null,
        root: "/Workspace/Notes-A",
        status: "ready",
        workspaceRoot: "/Workspace"
      });
    }
  );

  it("rejects a filesystem root without changing native metadata", async () => {
    configureMetadataState(emptyMetadata);
    const { result } = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
    await waitFor(() => expect(result.current.status).toBe("needs-onboarding"));

    await act(async () => result.current.commitDesktopRoot("/"));

    expect(result.current).toMatchObject({ root: null, status: "error", workspaceRoot: null });
    expect(metadata).toEqual(emptyMetadata);
    expect(writePrimaryWorkspaceState).not.toHaveBeenCalled();
  });
});
