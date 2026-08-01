import {
  configureAppRuntime,
  createDefaultAppRuntime,
  resetAppRuntimeForTests
} from "../../runtime";
import {
  defaultPrimaryWorkspaceState,
  isValidManagedNotebookName,
  loadPrimaryWorkspaceState,
  normalizePrimaryWorkspaceState,
  saveCanonicalPrimaryWorkspaceState,
  savePrimaryWorkspaceState
} from "./primary-workspace-state";

describe("primary workspace host metadata", () => {
  afterEach(() => resetAppRuntimeForTests());

  it("normalizes only valid version-3 identities and managed names", () => {
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
    expect(normalizePrimaryWorkspaceState({ version: 2 })).toEqual(defaultPrimaryWorkspaceState);
    expect(isValidManagedNotebookName("个人 笔记")).toBe(true);
    expect(isValidManagedNotebookName(".markra-sync")).toBe(false);
    expect(isValidManagedNotebookName("../escape")).toBe(false);
  });

  it("uses only the native primary-workspace bridge", async () => {
    const defaultRuntime = createDefaultAppRuntime();
    const readPrimaryWorkspaceState = vi.fn(async () => ({
      desktopWorkspaceRoot: "/Workspace",
      desktopPath: "/Workspace/Notes",
      managedName: null,
      onboardingCompleted: true,
      version: 3
    }));
    const writePrimaryWorkspaceState = vi.fn(async ({ state }: { state: unknown }) => ({
      applied: true,
      state
    }));
    const loadStore = vi.fn(async () => {
      throw new Error("unexpected renderer store access");
    });
    configureAppRuntime({
      ...defaultRuntime,
      settings: { loadStore, readPrimaryWorkspaceState, writePrimaryWorkspaceState }
    });

    const current = await loadPrimaryWorkspaceState();
    await savePrimaryWorkspaceState(current);
    await saveCanonicalPrimaryWorkspaceState(current, current);

    expect(readPrimaryWorkspaceState).toHaveBeenCalledOnce();
    expect(writePrimaryWorkspaceState).toHaveBeenNthCalledWith(1, { state: current });
    expect(writePrimaryWorkspaceState).toHaveBeenNthCalledWith(2, {
      expectedState: current,
      state: current
    });
    expect(loadStore).not.toHaveBeenCalled();
  });

  it("fails closed when the native bridge is missing instead of using renderer storage", async () => {
    const defaultRuntime = createDefaultAppRuntime();
    const loadStore = vi.fn(async () => {
      throw new Error("unexpected renderer store access");
    });
    configureAppRuntime({
      ...defaultRuntime,
      settings: { loadStore }
    });

    await expect(loadPrimaryWorkspaceState()).rejects.toThrow("native primary workspace");
    await expect(savePrimaryWorkspaceState(defaultPrimaryWorkspaceState)).rejects.toThrow(
      "native primary workspace"
    );
    expect(loadStore).not.toHaveBeenCalled();
  });
});
