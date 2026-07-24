import { act, renderHook, waitFor } from "@testing-library/react";
import { requestPrimaryCloudNotebookRestore } from "../lib/cloud-notebook-restore-events";
import { showAppToast } from "../lib/app-toast";
import type { SyncConfigDocument, SyncConfigPatch } from "../lib/sync-config";
import {
  configureAppRuntime,
  createDefaultAppRuntime,
  resetAppRuntimeForTests,
  type AppRuntime,
  type NativeSettingsWindowContext,
  type NativeSettingsWindowTarget,
  type NativeSettingsWindowHideRequest
} from "../runtime";
import {
  canonicalizeEditorFontFamilyPreference,
  shellCommandActionFailureMessage,
  useSettingsWindowState
} from "./useSettingsWindowState";

vi.mock("../lib/app-toast", () => ({ showAppToast: vi.fn() }));
vi.mock("../lib/cloud-notebook-restore-events", () => ({
  requestPrimaryCloudNotebookRestore: vi.fn()
}));

const mockedRequestPrimaryCloudNotebookRestore = vi.mocked(requestPrimaryCloudNotebookRestore);

function deferred<T>() {
  let resolve!: (value: T) => undefined;
  const promise = new Promise<T>((resolvePromise) => {
    resolve = (value) => {
      resolvePromise(value);
      return undefined;
    };
  });
  return { promise, resolve };
}

function document(revision: string, remoteRoot = "qingyu"): SyncConfigDocument {
  return {
    config: {
      autoSyncOnSave: false,
      enabled: true,
      intervalMinutes: 0,
      provider: "webdav",
      remoteRoot,
      s3: {
        accessKeyId: "",
        bucket: "",
        endpointUrl: "",
        region: "",
        secretAccessKey: "",
        requestTimeoutSeconds: 60,
        addressingStyle: "auto",
        tlsVerification: "verify"
      },
      version: 2,
      webdav: { password: "", serverUrl: "https://dav.example.test", username: "" }
    },
    configured: true,
    issues: [],
    readiness: "ready",
    revision
  };
}

function installRuntime(primaryRoot: string | null, patchOverride?: AppRuntime["syncConfig"]["patch"]) {
  const defaultRuntime = createDefaultAppRuntime();
  const values = new Map<string, unknown>();
  if (primaryRoot) {
    values.set("primaryWorkspace", {
      desktopWorkspaceRoot: primaryRoot.slice(0, primaryRoot.lastIndexOf("/")) || "/",
      desktopPath: primaryRoot,
      managedName: null,
      onboardingCompleted: true,
      version: 3
    });
  } else {
    values.set("primaryWorkspace", {
      desktopWorkspaceRoot: null,
      desktopPath: null,
      managedName: null,
      onboardingCompleted: true,
      version: 3
    });
  }
  const load = vi.fn(async () => ({ ...document("rev-1"), status: "loaded" as const }));
  const patch = vi.fn(patchOverride ?? (async ({ patch }: { expectedRevision: string; patch: SyncConfigPatch }) => (
    document("rev-2", patch.field === "remoteRoot" ? String(patch.value) : "qingyu")
  )));
  const requestApply = vi.fn(defaultRuntime.syncConfig.requestApply);
  let hideListener: ((request: NativeSettingsWindowHideRequest) => unknown) | null = null;
  let targetListener: ((target: NativeSettingsWindowTarget) => unknown) | null = null;
  let contextListener: ((context: NativeSettingsWindowContext) => unknown) | null = null;
  const cancelSettingsWindowHide = vi.fn(async () => undefined);
  const completeSettingsWindowHide = vi.fn(async () => undefined);
  const hideSettingsWindow = vi.fn(async () => undefined);
  const listNotebooks = vi.fn(async () => [{
    available: true,
    disabledReason: null,
    name: "Archive"
  }]);
  configureAppRuntime({
    ...defaultRuntime,
    files: {
      ...defaultRuntime.files,
      resolveMarkdownFolder: vi.fn(async (path) => ({ name: "Notes", path }))
    },
    settings: {
      async loadStore() {
        return {
          async delete(key: string) { values.delete(key); },
          async get<T>(key: string) { return values.get(key) as T | undefined; },
          async save() { return undefined; },
          async set(key: string, value: unknown) { values.set(key, value); }
        };
      }
    },
    syncConfig: {
      ...defaultRuntime.syncConfig,
      listNotebooks,
      load,
      patch,
      requestApply
    },
    window: {
      ...defaultRuntime.window,
      cancelSettingsWindowHide,
      completeSettingsWindowHide,
      hideSettingsWindow,
      listenSettingsWindowHideRequested: async (listener) => {
        hideListener = listener;
        return () => { hideListener = null; };
      },
      listenSettingsWindowTarget: async (onTarget, onContext) => {
        targetListener = onTarget;
        contextListener = onContext ?? null;
        return () => {
          targetListener = null;
          contextListener = null;
        };
      }
    }
  });
  return {
    cancelSettingsWindowHide,
    completeSettingsWindowHide,
    hide: (request: NativeSettingsWindowHideRequest) => hideListener?.(request),
    hideSettingsWindow,
    listNotebooks,
    load,
    patch,
    reopen: async (
      target: NativeSettingsWindowTarget = "sync",
      context: NativeSettingsWindowContext = {
        projectRoot: null,
        sourceWindowLabel: "main",
        workspaceSourcePath: primaryRoot
      }
    ) => {
      targetListener?.(target);
      await contextListener?.(context);
    },
    reopenTargetless: async (context: NativeSettingsWindowContext = {
      projectRoot: null,
      sourceWindowLabel: "main",
      workspaceSourcePath: primaryRoot
    }) => {
      await contextListener?.(context);
    },
    requestApply
  };
}

describe("settings application sync session", () => {
  beforeEach(() => {
    resetAppRuntimeForTests();
    vi.mocked(showAppToast).mockReset();
    mockedRequestPrimaryCloudNotebookRestore.mockReset();
    window.history.pushState({}, "", "/?settingsTarget=sync");
  });

  afterEach(() => {
    resetAppRuntimeForTests();
    window.history.pushState({}, "", "/");
  });

  it("uses the application primary workspace instead of the invoking external root", async () => {
    installRuntime("/Notes");
    window.history.pushState({}, "", "/?settingsTarget=sync");
    const { result } = renderHook(() => useSettingsWindowState());

    await waitFor(() => {
      expect(result.current.syncView.primaryRoot).toBe("/Notes");
      expect(result.current.syncView.configDocument?.revision).toBe("rev-1");
    });
  });

  it("loads and patches app sync config without a primary workspace, then applies only on category leave", async () => {
    const runtime = installRuntime(null);
    const { result } = renderHook(() => useSettingsWindowState());
    await waitFor(() => expect(result.current.syncView.configDocument?.revision).toBe("rev-1"));

    await act(async () => {
      await result.current.syncSession.patch({ field: "remoteRoot", value: "qingyu/team" });
    });
    expect(runtime.patch).toHaveBeenCalledTimes(1);
    expect(runtime.requestApply).not.toHaveBeenCalled();
    expect(result.current.syncView.primaryRoot).toBeNull();

    await act(async () => result.current.setActiveCategory("appearance"));
    expect(runtime.requestApply).toHaveBeenCalledTimes(1);
    expect(runtime.requestApply).toHaveBeenCalledWith(expect.objectContaining({
      exitReason: "category-leave",
      revision: "rev-2"
    }));
  });

  it("waits for the app sync session boundary before completing native window close", async () => {
    const runtime = installRuntime("/Notes");
    const { result } = renderHook(() => useSettingsWindowState());
    await waitFor(() => expect(result.current.syncView.configDocument?.revision).toBe("rev-1"));
    await act(async () => result.current.syncSession.patch({ field: "remoteRoot", value: "qingyu/team" }));

    await act(async () => runtime.hide({ generation: 7 }));

    expect(runtime.requestApply).toHaveBeenCalledWith(expect.objectContaining({ exitReason: "window-close" }));
    expect(runtime.completeSettingsWindowHide).toHaveBeenCalledWith(7);
  });

  it("keeps the sync category and native settings window open after an unsaved field", async () => {
    const runtime = installRuntime("/Notes", async () => {
      throw new Error("disk full");
    });
    const { result } = renderHook(() => useSettingsWindowState());
    await waitFor(() => expect(result.current.syncView.configDocument?.revision).toBe("rev-1"));
    await act(async () => {
      await expect(result.current.syncSession.patch({ field: "remoteRoot", value: "qingyu/team" }))
        .rejects.toThrow("disk full");
    });

    await act(async () => result.current.setActiveCategory("appearance"));
    expect(result.current.activeCategory).toBe("sync");

    await act(async () => runtime.hide({ generation: 8 }));
    expect(runtime.completeSettingsWindowHide).not.toHaveBeenCalled();
    expect(runtime.cancelSettingsWindowHide).toHaveBeenCalledWith(8);
  });

  it("opens the Settings-owned cloud notebook dialog without hiding Settings", async () => {
    const runtime = installRuntime("/Workspace/A");
    const { result } = renderHook(() => useSettingsWindowState());
    await waitFor(() => expect(result.current.syncView.configDocument?.revision).toBe("rev-1"));

    await act(async () => result.current.handleSelectCloudNotebook());

    expect(runtime.listNotebooks).toHaveBeenCalledWith({ revision: "rev-1" });
    expect(result.current.remoteNotebookDialog.open).toBe(true);
    expect(result.current.remoteNotebookDialog.entries).toEqual([{
      available: true,
      disabledReason: null,
      name: "Archive"
    }]);
    expect(runtime.hideSettingsWindow).not.toHaveBeenCalled();
  });

  it("keeps settings visible and reports failure when ending sync editing fails", async () => {
    const runtime = installRuntime("/Workspace/A", async () => {
      throw new Error("disk full");
    });
    const { result } = renderHook(() => useSettingsWindowState());
    await waitFor(() => expect(result.current.syncView.configDocument?.revision).toBe("rev-1"));
    await act(async () => {
      await expect(result.current.syncSession.patch({ field: "remoteRoot", value: "qingyu/team" }))
        .rejects.toThrow("disk full");
    });

    await act(async () => result.current.handleSelectCloudNotebook());

    expect(runtime.listNotebooks).not.toHaveBeenCalled();
    expect(runtime.hideSettingsWindow).not.toHaveBeenCalled();
    expect(showAppToast).toHaveBeenCalledWith(expect.objectContaining({
      id: "application-sync-settings-exit",
      status: "error"
    }));
  });

  it("keeps Settings visible and shows a safe catalog error when listing fails", async () => {
    const runtime = installRuntime("/Workspace/A");
    runtime.listNotebooks.mockRejectedValueOnce(new Error("secret backend detail"));
    const { result } = renderHook(() => useSettingsWindowState());
    await waitFor(() => expect(result.current.syncView.configDocument?.revision).toBe("rev-1"));

    await act(async () => result.current.handleSelectCloudNotebook());

    expect(result.current.remoteNotebookDialog.open).toBe(true);
    expect(result.current.remoteNotebookDialog.error)
      .toBe("The cloud notebook list could not be refreshed.");
    expect(result.current.remoteNotebookDialog.error).not.toContain("secret backend detail");
    expect(runtime.hideSettingsWindow).not.toHaveBeenCalled();
    expect(runtime.load).toHaveBeenCalledTimes(2);
  });

  it("recovers the visible Sync session after an ordinary hide failure", async () => {
    const runtime = installRuntime("/Workspace/A");
    runtime.completeSettingsWindowHide.mockRejectedValueOnce(new Error("hide failed"));
    const { result } = renderHook(() => useSettingsWindowState());
    await waitFor(() => expect(result.current.syncView.configDocument?.revision).toBe("rev-1"));

    await act(async () => runtime.hide({ generation: 10 }));
    expect(runtime.cancelSettingsWindowHide).toHaveBeenCalledWith(10);
    expect(runtime.load).toHaveBeenCalledTimes(2);
    expect(result.current.activeCategory).toBe("sync");
  });

  it("restores the sync editing session when a retained Sync settings window reopens", async () => {
    const runtime = installRuntime("/Workspace/A");
    const { result } = renderHook(() => useSettingsWindowState());
    await waitFor(() => expect(result.current.syncView.configDocument?.revision).toBe("rev-1"));

    await act(async () => runtime.hide({ generation: 11 }));
    expect(runtime.load).toHaveBeenCalledTimes(1);

    await act(async () => runtime.reopen());

    expect(result.current.activeCategory).toBe("sync");
    expect(runtime.load).toHaveBeenCalledTimes(2);
    await act(async () => result.current.syncSession.patch({ field: "remoteRoot", value: "qingyu/reopened" }));
    expect(runtime.patch).toHaveBeenCalledTimes(1);
  });

  it("clears an open cloud notebook dialog before a retained Settings window hides and reopens", async () => {
    const runtime = installRuntime("/Workspace/A");
    const { result } = renderHook(() => useSettingsWindowState());
    await waitFor(() => expect(result.current.syncView.configDocument?.revision).toBe("rev-1"));
    await act(async () => result.current.handleSelectCloudNotebook());
    expect(result.current.remoteNotebookDialog.entries).toEqual([{
      available: true,
      disabledReason: null,
      name: "Archive"
    }]);

    await act(async () => runtime.hide({ generation: 14 }));

    expect(runtime.completeSettingsWindowHide).toHaveBeenCalledWith(14);
    expect(result.current.remoteNotebookDialog.open).toBe(false);
    expect(result.current.remoteNotebookDialog.entries).toEqual([]);
    expect(result.current.remoteNotebookDialog.error).toBeNull();

    await act(async () => runtime.reopen());

    expect(result.current.remoteNotebookDialog.open).toBe(false);
    expect(result.current.remoteNotebookDialog.entries).toEqual([]);
    await act(async () => result.current.syncSession.patch({ field: "remoteRoot", value: "qingyu/reopened" }));
    expect(runtime.patch).toHaveBeenCalledTimes(1);
  });

  it("waits for a pending dialog cancel restart before ending Sync for native hide", async () => {
    const runtime = installRuntime("/Workspace/A");
    const { result } = renderHook(() => useSettingsWindowState());
    await waitFor(() => expect(result.current.syncView.configDocument?.revision).toBe("rev-1"));
    await act(async () => result.current.handleSelectCloudNotebook());
    const restart = deferred<SyncConfigDocument & { status: "loaded" }>();
    runtime.load.mockImplementationOnce(async () => restart.promise);

    let cancel!: Promise<unknown>;
    act(() => {
      cancel = Promise.resolve(result.current.remoteNotebookDialog.cancel());
    });
    await waitFor(() => expect(runtime.load).toHaveBeenCalledTimes(3));

    let hide!: Promise<unknown>;
    act(() => {
      hide = Promise.resolve(runtime.hide({ generation: 15 }));
    });
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });
    const completedBeforeRestart = runtime.completeSettingsWindowHide.mock.calls.length;

    await act(async () => {
      restart.resolve({ ...document("rev-restarted"), status: "loaded" });
      await Promise.all([cancel, hide]);
    });

    expect(completedBeforeRestart).toBe(0);
    expect(runtime.completeSettingsWindowHide).toHaveBeenCalledWith(15);
    expect(result.current.syncSession.sessionId).toBeNull();
  });

  it("waits for a pending dialog cancel restart before leaving the Sync category", async () => {
    const runtime = installRuntime("/Workspace/A");
    const { result } = renderHook(() => useSettingsWindowState());
    await waitFor(() => expect(result.current.syncView.configDocument?.revision).toBe("rev-1"));
    await act(async () => result.current.handleSelectCloudNotebook());
    const restart = deferred<SyncConfigDocument & { status: "loaded" }>();
    runtime.load.mockImplementationOnce(async () => restart.promise);

    let cancel!: Promise<unknown>;
    act(() => {
      cancel = Promise.resolve(result.current.remoteNotebookDialog.cancel());
    });
    await waitFor(() => expect(runtime.load).toHaveBeenCalledTimes(3));

    let leave!: Promise<unknown>;
    act(() => {
      leave = Promise.resolve(result.current.setActiveCategory("appearance"));
    });
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });
    const categoryBeforeRestart = result.current.activeCategory;

    await act(async () => {
      restart.resolve({ ...document("rev-restarted"), status: "loaded" });
      await Promise.all([cancel, leave]);
    });

    expect(categoryBeforeRestart).toBe("sync");
    expect(result.current.activeCategory).toBe("appearance");
    expect(result.current.syncSession.sessionId).toBeNull();
  });

  it("invalidates an unmatched restore waiter when leaving the Sync category", async () => {
    installRuntime("/Workspace/Current");
    const { result } = renderHook(() => useSettingsWindowState());
    await waitFor(() => expect(result.current.syncView.configDocument?.revision).toBe("rev-1"));
    await act(async () => result.current.handleSelectCloudNotebook());
    mockedRequestPrimaryCloudNotebookRestore.mockResolvedValueOnce(true);
    await act(async () => result.current.remoteNotebookDialog.restore("Archive"));

    await act(async () => result.current.setActiveCategory("appearance"));
    await act(async () => result.current.primaryWorkspace.commitDesktopRoot("/Restored/Archive"));
    await waitFor(() => expect(result.current.primaryWorkspace.root).toBe("/Restored/Archive"));

    expect(result.current.activeCategory).toBe("appearance");
    expect(result.current.syncSession.sessionId).toBeNull();
  });

  it("lets the normal category effect begin Sync once when a retained window reopens from another category", async () => {
    const runtime = installRuntime("/Workspace/A");
    const { result } = renderHook(() => useSettingsWindowState());
    await waitFor(() => expect(result.current.syncView.configDocument?.revision).toBe("rev-1"));

    await act(async () => result.current.setActiveCategory("appearance"));
    expect(result.current.activeCategory).toBe("appearance");

    await act(async () => runtime.reopen());
    await waitFor(() => expect(result.current.activeCategory).toBe("sync"));
    await waitFor(() => expect(runtime.load).toHaveBeenCalledTimes(2));
  });

  it("recovers a retained active Sync page on a targetless reopen", async () => {
    window.history.pushState({}, "", "/");
    const runtime = installRuntime("/Workspace/A");
    const { result } = renderHook(() => useSettingsWindowState());

    await act(async () => result.current.setActiveCategory("sync"));
    await waitFor(() => expect(result.current.syncView.configDocument?.revision).toBe("rev-1"));
    expect(runtime.load).toHaveBeenCalledTimes(1);

    await act(async () => runtime.hide({ generation: 12 }));
    expect(runtime.load).toHaveBeenCalledTimes(1);

    await act(async () => runtime.reopenTargetless());
    expect(runtime.load).toHaveBeenCalledTimes(2);
  });

  it("consumes an explicit non-Sync target without beginning Sync and remains retryable later", async () => {
    const runtime = installRuntime("/Workspace/A");
    const { result } = renderHook(() => useSettingsWindowState());
    await waitFor(() => expect(result.current.syncView.configDocument?.revision).toBe("rev-1"));

    await act(async () => runtime.hide({ generation: 13 }));
    await act(async () => runtime.reopen("exportPandocPath"));
    await waitFor(() => expect(result.current.activeCategory).toBe("export"));
    expect(runtime.load).toHaveBeenCalledTimes(1);

    await act(async () => result.current.setActiveCategory("sync"));
    await waitFor(() => expect(runtime.load).toHaveBeenCalledTimes(2));
  });
});
describe("settings window utilities", () => {
  it("includes native shell command failure details when available", () => {
    expect(shellCommandActionFailureMessage("Install failed.", new Error("Permission denied")))
      .toBe("Install failed. Permission denied");
  });

  it("maps a saved localized font label to the CSS font family name", () => {
    const preferences = {
      editorFontFamily: { family: "思源宋体", source: "system" as const }
    } as Parameters<typeof canonicalizeEditorFontFamilyPreference>[0];
    expect(canonicalizeEditorFontFamilyPreference(preferences, [
      { family: "Source Han Serif SC", label: "思源宋体" }
    ])?.editorFontFamily.family).toBe("Source Han Serif SC");
  });
});
