import { act, renderHook, waitFor } from "@testing-library/react";
import {
  configureAppRuntime,
  createDefaultAppRuntime,
  resetAppRuntimeForTests
} from "../runtime";
import type { PrimaryWorkspaceState } from "../lib/settings/primary-workspace-state";
import { usePrimaryWorkspace } from "./usePrimaryWorkspace";

describe("primary workspace controller", () => {
  const readPrimaryWorkspaceState = vi.fn();
  const writePrimaryWorkspaceState = vi.fn();
  const loadStore = vi.fn(async () => {
    throw new Error("unexpected local-state access");
  });

  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => resetAppRuntimeForTests());

  function configureMetadataState(state: PrimaryWorkspaceState) {
    const runtime = createDefaultAppRuntime();
    let current = state;
    readPrimaryWorkspaceState.mockImplementation(async () => current);
    writePrimaryWorkspaceState.mockImplementation(async ({ expectedState, state: nextState }) => {
      if (expectedState !== undefined && JSON.stringify(expectedState) !== JSON.stringify(current)) {
        return { applied: false, state: current };
      }
      current = nextState as PrimaryWorkspaceState;
      return { applied: true, state: current };
    });
    configureAppRuntime({
      ...runtime,
      settings: {
        loadStore,
        readPrimaryWorkspaceState,
        writePrimaryWorkspaceState
      }
    });
    return runtime;
  }

  it("uses a fixed mobile/server root without reading or writing host metadata", async () => {
    const runtime = createDefaultAppRuntime();
    const resolveRoot = vi.fn(async () => "kernel-workspace://primary");
    configureAppRuntime({
      ...runtime,
      settings: {
        loadStore,
        readPrimaryWorkspaceState,
        writePrimaryWorkspaceState
      },
      workspace: {
        ...runtime.workspace,
        rootPolicy: {
          canChooseLocalRoot: false,
          kind: "fixed",
          resolveRoot
        }
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

  it("delegates a host-selectable Desktop root without metadata persistence", async () => {
    const runtime = createDefaultAppRuntime();
    const commitRoot = vi.fn(async () => "kernel-workspace://primary");
    const resolveRoot = vi.fn(async () => "kernel-workspace://primary");
    configureAppRuntime({
      ...runtime,
      settings: {
        loadStore,
        readPrimaryWorkspaceState,
        writePrimaryWorkspaceState
      },
      workspace: {
        ...runtime.workspace,
        rootPolicy: {
          canChooseLocalRoot: true,
          commitRoot,
          kind: "host-selectable",
          resolveRoot,
          selectRoot: vi.fn(async () => "/Workspace/Notes")
        }
      }
    });

    const { result } = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
    await waitFor(() => expect(result.current.status).toBe("ready"));
    await act(async () => {
      await result.current.commitDesktopRoot("/Workspace/Notes");
    });

    expect(commitRoot).toHaveBeenCalledWith("/Workspace/Notes");
    expect(result.current.root).toBe("kernel-workspace://primary");
    expect(readPrimaryWorkspaceState).not.toHaveBeenCalled();
    expect(writePrimaryWorkspaceState).not.toHaveBeenCalled();
    expect(loadStore).not.toHaveBeenCalled();
  });

  it("resolves managed onboarding from the native metadata bridge", async () => {
    const runtime = configureMetadataState({
      desktopWorkspaceRoot: null,
      desktopPath: null,
      managedName: "personal",
      onboardingCompleted: true,
      version: 3
    });
    const resolveManagedRoot = vi.fn(async () => "kernel-workspace://primary");
    configureAppRuntime({
      ...runtime,
      settings: {
        loadStore,
        readPrimaryWorkspaceState,
        writePrimaryWorkspaceState
      },
      workspace: {
        ...runtime.workspace,
        resolveManagedRoot
      }
    });

    const { result } = renderHook(() => usePrimaryWorkspace({ trueMobile: true }));
    await waitFor(() => expect(result.current.status).toBe("ready"));

    expect(result.current).toMatchObject({
      managedName: "personal",
      root: "kernel-workspace://primary",
      status: "ready"
    });
    expect(readPrimaryWorkspaceState).toHaveBeenCalledOnce();
    expect(resolveManagedRoot).toHaveBeenCalledWith("personal");
    expect(loadStore).not.toHaveBeenCalled();
  });

  it("writes a selected desktop notebook through the native metadata bridge", async () => {
    const runtime = configureMetadataState({
      desktopWorkspaceRoot: null,
      desktopPath: null,
      managedName: null,
      onboardingCompleted: false,
      version: 3
    });
    const resolveMarkdownFolder = vi.fn(async (path: string) => ({
      name: path === "/Workspace" ? "Workspace" : "Notes",
      path
    }));
    configureAppRuntime({
      ...runtime,
      files: { ...runtime.files, resolveMarkdownFolder },
      settings: {
        loadStore,
        readPrimaryWorkspaceState,
        writePrimaryWorkspaceState
      }
    });

    const { result } = renderHook(() => usePrimaryWorkspace({ trueMobile: false }));
    await waitFor(() => expect(result.current.status).toBe("needs-onboarding"));
    await act(async () => {
      await result.current.commitDesktopRoot("/Workspace/Notes");
    });

    expect(result.current).toMatchObject({
      root: "/Workspace/Notes",
      status: "ready",
      workspaceRoot: "/Workspace"
    });
    expect(writePrimaryWorkspaceState).toHaveBeenCalledWith({
      state: {
        desktopWorkspaceRoot: "/Workspace",
        desktopPath: "/Workspace/Notes",
        managedName: null,
        onboardingCompleted: true,
        version: 3
      }
    });
    expect(loadStore).not.toHaveBeenCalled();
  });
});
